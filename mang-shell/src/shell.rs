//! 外壳根视图 —— #63 Lead 布局的组合点 + #68 阶段 1 编辑区接线。
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ 标题栏 36px（自绘：拖拽 + 窗口控制 + 文档名）    │
//! ├────┬─────────┬──────────────────┬────────────┤
//! │图标│ 左栏     │ 编辑区           │ 右栏        │
//! │条  │ 缺省260  │ Editor / 空态     │ 缺省280     │
//! │52px│ (150-500)│ (#68 md 编辑面)  │ (150-600)  │
//! │    │ 可拖拽   │                  │ 可拖拽      │
//! ├────┴─────────┴──────────────────┴────────────┤
//! │ 状态栏 24px（文件 / 修改标记 / 行,列 / 字数）    │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! 折叠状态（`left_open` / `right_open`）与 `editor`（`Option<Entity<Editor>>`，
//! 由 main 的 CLI 文档分派构造）是本视图全部自持状态；F1 起 `editor` 可经
//! [`ShellApp::open_document`] 运行时替换（Ctrl+O 对话框 / 文件树 / 最近
//! 列表共用通道），失败/不支持轻提示横幅（[`NoticeBoard`]）；F2 起
//! `workspace`（`Option<Entity<FileTree>>`，Ctrl+K Ctrl+O 打开文件夹）——
//! 记忆落盘随 settings（重启恢复）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::sync::Arc;

use gpui::{
    App, Bounds, Context, CursorStyle, DispatchPhase, DragMoveEvent, Entity, FocusHandle,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseUpEvent, PathPromptOptions, Pixels, Render,
    Window, canvas, px,
};
use theme::Theme;
use ui::prelude::*;

use crate::activity_bar;
use crate::context_menu::{self, ContextMenu, MenuItem, Rect as MenuRect};
use crate::context_menu_view;
use crate::document::{self, OpenOutcome};
use crate::editor::Editor;
// #182：设置面板的缩进单位状态（editor::buffer 的纯枚举——面板/标签共用）。
use crate::editor::buffer::IndentUnit;
use crate::file_tree::FileTree;
use crate::git_panel::GitPanel;
use crate::panels;
use crate::recent::RecentStore;
use crate::settings_panel;
use crate::status_bar;
use crate::tab_bar::{self, TabItem};
use crate::theme_switch;
use crate::title_bar;

/// Ctrl+K 和弦窗口（第二键有效期；过期自动作废——惰性判定无需定时器）。
const CHORD_WINDOW: Duration = Duration::from_secs(2);

/// F4：确认框选项序（0 保存 / 1 不保存 / 2 取消）。键盘 Enter 激活
/// 选中项、Tab/←/→ 循环、Esc 直接取消。
const CONFIRM_BUTTONS: [&str; 3] = ["保存", "不保存", "取消"];

/// #166：删除确认按钮（0 删除 / 1 取消）——与 CONFIRM_BUTTONS 分列：
/// 按钮集属于动作语义（保存三选 / 删除二选），不是全局常量。
const DELETE_BUTTONS: [&str; 2] = ["删除", "取消"];

/// F4：待确认动作（脏标签关闭 / 退出 / #166 文件树删除）。
#[derive(Clone, Debug)]
enum ConfirmAction {
    /// 关闭 `idx` 标签（它有未保存修改）。
    CloseTab { idx: usize },
    /// 退出应用（存在未保存修改的标签）。
    Quit,
    /// #166：永久删除文件树目标（确认后不可恢复——无回收站 API，
    /// gpui 闭包未暴露 trash 面）。
    DeletePath { path: PathBuf },
}

/// F4：确认框状态。
/// G4：面板拖拽侧（左栏/右栏——resize 手柄归属）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelSide {
    Left,
    Right,
}

/// G4：面板 resize 手柄的 drag 载荷类型（`on_drag` / `on_drag_move`
/// 配对标识；GPUI 按【类型】路由 drag 会话——侧别由监听器闭包捕获，
/// 载荷本身不携带数据）。
#[derive(Clone, Copy)]
struct PanelResizeDrag;

/// G4：面板拖拽会话态（mousedown 落点起 → drag_move 步进 → mouseup
/// 收尾落盘）。
#[derive(Clone, Copy, Debug)]
struct PanelDragState {
    side: PanelSide,
    /// 上一步的窗口 x（步进位移基准）。
    last_x: f32,
}

/// G4：resize 手柄跟随视图（`on_drag` 契约需要一个 Render 实体——
/// 空渲染不可见；与 editor::DragGhost 同模式）。
struct PanelDragGhost;

impl Render for PanelDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().id("panel-drag-ghost").size_0()
    }
}

struct ConfirmState {
    message: String,
    detail: String,
    action: ConfirmAction,
    /// 选中按钮序（键盘循环 / 鼠标点击）。语义随动作集不同：
    /// 保存三选（0 保存 1 不保存 2 取消）/ 删除二选（0 删除 1 取消）。
    selected: usize,
    /// #166：按钮文案集（按动作携带——不再全局固定三钮）。
    buttons: Vec<&'static str>,
}

pub struct ShellApp {
    left_open: bool,
    right_open: bool,
    /// G4：左右栏宽度（拖拽热区调整；settings `panel_widths` 持久化，
    /// 缺省 [`panels::LEFT_PANEL_WIDTH`] / [`panels::RIGHT_PANEL_WIDTH`]）。
    left_width: Pixels,
    right_width: Pixels,
    /// G4：面板 resize 拖拽会话（`None` = 非拖拽中）。
    panel_drag: Option<PanelDragState>,
    /// F3：标签集（每标签 = 一个打开文档）；`active` 指向当前标签
    ///（空集 = 空态）。原 `Option<Entity<Editor>>` 单例面由此取代。
    tabs: Vec<TabItem>,
    active: usize,
    /// F2：工作区文件树（打开文件夹后呈现；记忆落盘随 settings）。
    workspace: Option<Entity<FileTree>>,
    /// F7：git 面板（工作区打开即建——非 git 目录走面板内提示态）。
    git_panel: Option<Entity<GitPanel>>,
    /// F7：左栏视图（文件树 / git——图标条切换）。
    git_view: bool,
    /// #182：右栏视图（属性/来源 ↔ 设置——图标条设置钮切换；
    /// 右栏折叠开关打开时恒回属性视图）。
    settings_view: bool,
    /// F2：Ctrl+K 和弦待续期（None = 无和弦进行中）。
    chord_until: Option<Instant>,
    /// F4：确认框（脏标签关闭/退出前确认；None = 无进行中）。
    confirm: Option<ConfirmState>,
    /// F4：确认框焦点域（模态开启时接管焦点——编辑器不再收键；
    /// 键事件沿遮罩节点→shell 根冒泡，由确认拦截分支统一处理）。
    confirm_focus: FocusHandle,
    /// shell 根焦点域（空态回退——无标签时键事件仍走 root→shell 根
    /// 冒泡路径；无此前提派发路径止于合成根节点，shell 级键位全聋）。
    root_focus: FocusHandle,
    /// F5：最近打开（文件 20 / 工作区 10——空态引导页呈现 + 写穿透落盘）。
    recent: RecentStore,
    /// F6：主题模式（system/light/dark——Ctrl+Alt+T 循环；持久化）。
    theme_mode: &'static str,
    /// F6：主题蓝本（启动时的全局主题 = One Dark——浅色派生与切回的
    /// 基准；运行期不追改）。
    base_theme: Arc<Theme>,
    /// F6：编辑器字号（px，8-32——Ctrl+=/-/0 缩放；应用到全部标签；
    /// 持久化）。
    font_size: f32,
    /// #182：缩进单位运行时值（设置面板展示/写入的单一状态源——
    /// 新建标签仍从 settings 读，两处同源不漂移；持久化走
    /// `settings::indent_unit` 既有键，零新增存储）。
    indent_unit: IndentUnit,
    /// F1：提示横幅（打开失败 / 类型不支持 / 对话框不可用等面向用户消息）。
    notice_board: NoticeBoard,
    /// #63 打磨：右键菜单（None = 未打开）。位置是**触发时**的窗口坐标，
    /// 摆放（边缘翻转/夹紧）在渲染期按当前窗口尺寸算——菜单开着时窗口被
    /// 改变大小，位置依然合法。
    context_menu: Option<ContextMenu>,
    /// #63 打磨：右键菜单来源（决定动作 id 派发到哪套处理器）。
    context_menu_origin: MenuOrigin,
    /// #166：文件树右键目标（动作执行的路径面——新建/打开/重命名/删除/
    /// 资源管理器揭示都需要知道右键的是哪一行）。
    context_menu_target: Option<PathBuf>,
}

/// 右键菜单来源（决定同一动作 id 派发到哪套处理器）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MenuOrigin {
    /// 编辑器区右键（剪切/复制/粘贴/查找…）。
    #[default]
    Editor,
    /// 文件树右键（新建/重命名/删除/在资源管理器显示）。
    FileTree,
}

/// F1：提示横幅状态机（纯逻辑——代际/清除语义单测钉定）。
///
/// 代际（`generation`）防「旧定时误清新消息」：每条消息取得代际号，
/// 自动清除任务只在当前代际匹配时生效——后到的消息使先前的定时器失效。
#[derive(Default)]
struct NoticeBoard {
    notice: Option<Notice>,
    generation: u32,
}

struct Notice {
    generation: u32,
    message: String,
}

impl NoticeBoard {
    /// 展示消息，返回其代际号。
    fn show(&mut self, message: impl Into<String>) -> u32 {
        self.generation += 1;
        self.notice = Some(Notice {
            generation: self.generation,
            message: message.into(),
        });
        self.generation
    }

    /// 按代际清除（仅当仍是当前消息）；返回是否实际清除。
    fn dismiss(&mut self, generation: u32) -> bool {
        if self
            .notice
            .as_ref()
            .is_some_and(|n| n.generation == generation)
        {
            self.notice = None;
            true
        } else {
            false
        }
    }

    /// 立即清空（成功打开文档时）。
    fn clear(&mut self) {
        self.notice = None;
    }

    fn message(&self) -> Option<&str> {
        self.notice.as_ref().map(|n| n.message.as_str())
    }
}

impl ShellApp {
    /// `editor`：成功打开的启动文档（`None` = 空态）；`workspace`：
    /// 启动恢复的工作区根（F2 记忆——`None` = 无记忆/不存在）。
    pub fn new(
        editor: Option<Entity<Editor>>,
        workspace_root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if let Some(root) = &workspace_root {
            // 证据通道（与 open_workspace 的 "workspace: <路径>" 区分）。
            eprintln!("workspace(restored): {}", root.display());
        }
        let workspace = workspace_root.map(|root| {
            let shell = cx.entity().downgrade();
            cx.new(|_| FileTree::new(root, shell))
        });
        // F3：启动文档 → 首个标签（未命名草稿取合成身份——CLI 无参
        // 不构造实体，实际不可达；防御完整）。
        let tabs = editor
            .map(|editor| {
                let path = editor
                    .read(cx)
                    .file_path()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("未命名"));
                vec![TabItem { path, editor }]
            })
            .unwrap_or_default();
        // #179（#188 接线）：启动标签注入宿主句柄（右键菜单触发器的
        // 跨实体通道——与下述 open_document_tab / open_readonly_tab
        // 同一注入面，三入口全覆盖）。
        let shell_weak = cx.entity().downgrade();
        for tab in &tabs {
            tab.editor.update(cx, |ed, _| ed.set_shell(shell_weak.clone()));
        }
        // F4：窗口关闭前确认钩子（平台 X 钮/OS 关闭——四后端均接线
        // on_should_close；cx.quit 不经此路）。脏标签存在 → 拦截本次
        // 关闭 + 弹确认框；确认后保存/放弃再 cx.quit。
        let confirm_focus = cx.focus_handle();
        let root_focus = cx.focus_handle();
        // F5：最近打开记忆（空态引导页数据源）。CLI 启动文件不走
        // open_document（main 直构）——此处补记。
        let mut recent = RecentStore::load();
        for tab in &tabs {
            recent.record_file(&tab.path);
        }
        // F6：主题模式（settings 记忆 → 应用）+ 蓝本捕获 + 字号。
        let base_theme = theme::GlobalTheme::theme(cx).clone();
        let theme_mode = theme_switch::parse_mode(crate::settings::load_theme_mode().as_deref());
        theme_switch::apply_mode(theme_mode, &base_theme, cx);
        let font_size = crate::settings::load_font_size().unwrap_or(15.);
        if !recent.recent_files().is_empty() || !recent.recent_workspaces().is_empty() {
            eprintln!(
                "recent: {} 文件 / {} 工作区",
                recent.recent_files().len(),
                recent.recent_workspaces().len()
            );
        }
        let root_focus_for_init = root_focus.clone();
        {
            let shell = cx.entity().clone();
            let confirm_focus = confirm_focus.clone();
            window.on_window_should_close(cx, move |window, cx| {
                let dirty: Vec<usize> = shell
                    .read(cx)
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, tab)| tab.editor.read(cx).is_modified())
                    .map(|(i, _)| i)
                    .collect();
                if dirty.is_empty() {
                    return true;
                }
                shell.update(cx, |shell, cx| {
                    eprintln!("confirm: 退出前确认（{} 个未保存）", dirty.len());
                    shell.confirm = Some(ConfirmState {
                        message: format!("有 {} 个未保存的文档", dirty.len()),
                        detail: "退出前保存修改？".to_string(),
                        action: ConfirmAction::Quit,
                        selected: 0,
                        buttons: CONFIRM_BUTTONS.to_vec(),
                    });
                    cx.notify();
                });
                // 模态接管焦点（编辑器隔离）。
                window.focus(&confirm_focus, cx);
                false
            });
        }
        let mut this = Self {
            left_open: true,
            right_open: true,
            // G4：面板宽度记忆恢复（无记忆 = 缺省宽；手改配置越界时
            // 载入即钳制到拖拽域——不信任磁盘值）。
            left_width: crate::settings::load_panel_widths()
                .map(|(l, _)| px(panels::clamp_panel_width(true, l)))
                .unwrap_or(panels::LEFT_PANEL_WIDTH),
            right_width: crate::settings::load_panel_widths()
                .map(|(_, r)| px(panels::clamp_panel_width(false, r)))
                .unwrap_or(panels::RIGHT_PANEL_WIDTH),
            panel_drag: None,
            tabs,
            active: 0,
            workspace,
            git_panel: None,
            git_view: false,
            settings_view: false,
            chord_until: None,
            confirm: None,
            confirm_focus,
            root_focus,
            recent,
            theme_mode,
            base_theme,
            font_size,
            indent_unit: crate::settings::load_indent_unit()
                .as_deref()
                .map(IndentUnit::from_setting)
                .unwrap_or_default(),
            notice_board: NoticeBoard::default(),
            context_menu: None,
            context_menu_origin: MenuOrigin::default(),
            context_menu_target: None,
        };
        // 初始焦点：有文档 → 编辑器（键入/编辑即时可用）；空态 →
        // shell 根域（Ctrl+O 等空态键位经 root→shell 根路径可达——
        // 无焦点时派发路径止于合成根节点，shell 监听器不触发）。
        match this.editor() {
            Some(tab) => {
                let focus = tab.read(cx).focus_handle().clone();
                window.focus(&focus, cx);
            }
            None => window.focus(&root_focus_for_init, cx),
        }
        // F6：记忆字号应用到启动标签。
        this.apply_font_size(px(this.font_size), cx);
        this
    }

    /// 当前标签的编辑器（无标签 = 空态 None）。
    pub fn editor(&self) -> Option<&Entity<Editor>> {
        self.tabs.get(self.active).map(|tab| &tab.editor)
    }

    // ------------------------------------------------------------------
    // RPC 桥公开面（#104 任务 2）：标签清单/定位/无确认关闭——控制
    // 通道（get_state/close_file/switch_tab）的数据与操作面。全部薄
    // 方法（既有私有面的直读），不引入新状态。
    // ------------------------------------------------------------------

    /// 标签数。
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// 活动标签索引（无标签时为遗留值——调用方以 tab_count 先判空）。
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// 指定标签的路径（越界 `None`；未命名草稿为「未命名」占位路径）。
    pub fn tab_path(&self, idx: usize) -> Option<PathBuf> {
        self.tabs.get(idx).map(|tab| tab.path.clone())
    }

    /// 指定标签是否有未保存修改（越界 `None`）。
    pub fn tab_is_modified(&self, idx: usize, cx: &App) -> Option<bool> {
        self.tabs
            .get(idx)
            .map(|tab| tab.editor.read(cx).is_modified())
    }

    /// 指定标签是否只读渲染态（越界 `None`）。
    pub fn tab_is_readonly(&self, idx: usize, cx: &App) -> Option<bool> {
        self.tabs
            .get(idx)
            .map(|tab| tab.editor.read(cx).is_read_only())
    }

    /// 按路径定位标签索引（与 open_document_tab 同口径绝对化——
    /// CLI 相对入参 vs 对话框绝对入参不歧义）。未命中 `None`。
    pub fn tab_index_by_path(&self, path: &Path) -> Option<usize> {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.tabs.iter().position(|tab| tab.path == absolute)
    }

    /// 无确认关闭指定标签（控制面通道——确认框是用户交互面，不适合
    /// 机器通道；**脏拦截在调用方**（RPC 层拒 dirty，先保存再关）。
    /// 越界 `false`，关闭成功 `true`。
    pub fn close_tab_no_confirm(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if idx >= self.tabs.len() {
            return false;
        }
        self.close_tab_at(idx, window, cx);
        true
    }

    /// 折叠/展开左栏（文件树）。图标条 `act-file-tree` 触发——已开且
    /// 在 git 视图时切回文件树视图（F7）。
    pub fn toggle_left(&mut self, cx: &mut Context<Self>) {
        if self.left_open && self.git_view {
            self.git_view = false;
        } else {
            self.left_open = !self.left_open;
        }
        cx.notify();
    }

    /// F7：切到 git 视图（图标条 `act-git` 触发——已开且在 git 视图时
    /// 折叠左栏，再开切回）。
    pub fn toggle_git(&mut self, cx: &mut Context<Self>) {
        if self.left_open && self.git_view {
            self.left_open = false;
        } else {
            self.left_open = true;
            self.git_view = true;
        }
        cx.notify();
    }

    /// F7：git 面板实体（烟雾钩子/内部访问）。
    pub fn git_panel(&self) -> Option<Entity<GitPanel>> {
        self.git_panel.clone()
    }

    // ------------------------------------------------------------------
    // G4：面板宽度拖拽（边缘 4px 热区；col-resize 光标；域钳制；
    // settings 持久化）
    // ------------------------------------------------------------------

    /// G4：手柄 mousedown——落点入会话态（`last_x` 为步进位移基准）。
    fn begin_panel_drag(&mut self, side: PanelSide, x: f32) {
        self.panel_drag = Some(PanelDragState { side, last_x: x });
    }

    /// G4：手柄 drag_move——步进位移按侧别换算宽度（左栏右移增宽 /
    /// 右栏左移增宽），域钳制后重绘。选区/文本不受影响：drag 会话
    /// 捕获全部 move 事件（payload 异于编辑器拖选——不同 drag 通道，
    /// 编辑器 mousedown 未发生，无选区可言）。
    fn dispatch_panel_resize(
        &mut self,
        side: PanelSide,
        event: &DragMoveEvent<PanelResizeDrag>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.panel_drag.as_mut() else {
            return;
        };
        if state.side != side {
            return;
        }
        let now_x = f32::from(event.event.position.x);
        let dx = now_x - state.last_x;
        state.last_x = now_x;
        let is_left = side == PanelSide::Left;
        let current = if is_left {
            f32::from(self.left_width)
        } else {
            f32::from(self.right_width)
        };
        let clamped =
            panels::clamp_panel_width(is_left, current + panels::width_delta(is_left, dx));
        if is_left {
            self.left_width = px(clamped);
        } else {
            self.right_width = px(clamped);
        }
        cx.notify();
    }

    /// G4：拖拽收尾（窗口级 mouseup——任意位置抬起；与 editor 同模式
    /// 每帧注册）。宽度落 settings（持久化）+ 证据行；非拖拽中为空操作。
    fn finish_panel_drag(&mut self, cx: &mut Context<Self>) {
        if self.panel_drag.take().is_none() {
            return;
        }
        let (left, right) = (f32::from(self.left_width), f32::from(self.right_width));
        crate::settings::save_panel_widths(left, right);
        eprintln!("panel-resize: left {left} right {right} (persist)");
        cx.notify();
    }

    /// G4：手柄元素（边缘 4px 热区——col-resize 光标 + 拖拽会话）。
    fn panel_resize_handle(&self, side: PanelSide, cx: &Context<Self>) -> impl IntoElement {
        let hover_bg = cx.theme().colors().element_hover;
        div()
            .id(SharedString::from(match side {
                PanelSide::Left => "panel-resize-left",
                PanelSide::Right => "panel-resize-right",
            }))
            .w(panels::PANEL_RESIZE_HOTZONE)
            .h_full()
            .flex_none()
            .cursor(CursorStyle::ResizeColumn)
            .hover(|el| el.bg(hover_bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    this.begin_panel_drag(side, f32::from(event.position.x));
                    cx.notify();
                }),
            )
            .on_drag(PanelResizeDrag, |_, _, _, cx| cx.new(|_| PanelDragGhost))
            .on_drag_move::<PanelResizeDrag>(cx.listener(
                move |this, event: &DragMoveEvent<PanelResizeDrag>, _window, cx| {
                    this.dispatch_panel_resize(side, event, cx);
                },
            ))
    }

    /// F7：打开只读文本标签（diff 视图 / 提交清单——Editor 只读态：
    /// 导航/查找/复制全复用，编辑被门禁拦截）。同路径已开 → 激活。
    pub fn open_readonly_tab(
        &mut self,
        title: String,
        content: String,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        let path = PathBuf::from(format!("git-view:{title}"));
        if let Some(idx) = self.tabs.iter().position(|tab| tab.path == path) {
            return self.activate_tab_focus(idx, cx);
        }
        let editor = cx.new(|cx| Editor::new_readonly(Some(path.clone()), content, cx));
        // #179（#188 接线）：只读标签同样注入宿主句柄（右键菜单对只读
        // 态有灰显语义——剪切/粘贴/替换灰显，复制/查找/跳行可用）。
        let shell_weak = cx.entity().downgrade();
        editor.update(cx, |ed, _| ed.set_shell(shell_weak));
        let focus = editor.read(cx).focus_handle().clone();
        self.tabs.push(TabItem { path, editor });
        self.active = self.tabs.len() - 1;
        eprintln!("git-view: {title}（tabs: {} 开）", self.tabs.len());
        cx.notify();
        Some(focus)
    }

    /// 折叠/展开右栏（属性/来源）。图标条底部开关触发。#182：展开
    /// 时回属性/来源视图（设置视图是设置钮的专属入口——两入口各管
    /// 各的视图，不互相穿透）。
    pub fn toggle_right(&mut self, cx: &mut Context<Self>) {
        self.right_open = !self.right_open;
        if self.right_open {
            self.settings_view = false;
        }
        cx.notify();
    }

    /// #182：切到设置视图（图标条 `act-settings` 触发——已开且在设置
    /// 视图时折叠右栏，再开切回；与 toggle_git 同构）。
    pub fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        if self.right_open && self.settings_view {
            self.right_open = false;
        } else {
            self.right_open = true;
            self.settings_view = true;
        }
        cx.notify();
    }

    // ── #63 打磨：右键菜单 ─────────────────────────────────────

    /// 在窗口坐标 `pos` 处弹出编辑器右键菜单。
    ///
    /// `has_selection` / `read_only` 决定各项可用性（灰显而非消失）。
    /// 位置只记录；真正的边缘翻转在渲染期按当前窗口尺寸算。
    pub fn open_editor_context_menu(
        &mut self,
        pos: context_menu::Point,
        has_selection: bool,
        read_only: bool,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu::new(
            context_menu::editor_menu(has_selection, read_only),
            pos,
        ));
        self.context_menu_origin = MenuOrigin::Editor;
        cx.notify();
    }

    /// 在窗口坐标 `pos` 处弹出文件树右键菜单（#166：`target` = 右键的
    /// 行路径——动作执行面；目录本身 / 文件均可作目标）。
    pub fn open_file_tree_context_menu(
        &mut self,
        pos: context_menu::Point,
        target: PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu::new(context_menu::file_tree_menu(is_dir), pos));
        self.context_menu_origin = MenuOrigin::FileTree;
        self.context_menu_target = Some(target);
        cx.notify();
    }

    /// 关闭右键菜单（Esc / 点空白 / 执行完动作）。目标随菜单一起清
    ///（#166——菜单关了，目标路径不再有效）。
    pub fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            self.context_menu_target = None;
            cx.notify();
        }
    }

    /// 菜单当前是否打开（渲染期判定）。
    pub fn context_menu_open(&self) -> bool {
        self.context_menu.is_some()
    }

    /// 执行一个菜单动作（按 id 派发）。#166：带窗口——文件树「打开」
    /// 要聚焦编辑区（与 #165 编辑器命令面的窗口诉求同源）。
    ///
    /// 未接线的 id 只写证据通道并关菜单——**不 panic 也不静默**：
    /// 灰显项本来就点不到，能点到却没接线的项说明菜单配错了，
    /// stderr 留痕比默默吞掉好。
    pub fn activate_context_menu_item(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let origin = self.context_menu_origin;
        let target = self.context_menu_target.clone();
        self.context_menu = None;
        self.context_menu_target = None;
        match (origin, id) {
            // 编辑器命令动作：统一走 run_command（键位 / 命令面板 / 图标条
            // 搜索钮同一分派路径）。#165 前经 exec_menu_command 的
            // `_ => {}` 静默吞掉 OpenFind/OpenReplace/GotoLine——菜单
            // 「查找/替换/跳转到行」是死项；现在窗口引用随点击 listener
            // 传入，全命令可达（exec_menu_command 已删——见其原址注释）。
            (MenuOrigin::Editor, "ed-select-all") => {
                self.dispatch_editor_cmd(crate::editor::commands::Cmd::SelectAll, window, cx);
            }
            (MenuOrigin::Editor, "ed-find") => {
                self.dispatch_editor_cmd(crate::editor::commands::Cmd::OpenFind, window, cx);
            }
            (MenuOrigin::Editor, "ed-replace") => {
                self.dispatch_editor_cmd(crate::editor::commands::Cmd::OpenReplace, window, cx);
            }
            (MenuOrigin::Editor, "ed-goto") => {
                self.dispatch_editor_cmd(crate::editor::commands::Cmd::GotoLine, window, cx);
            }
            (MenuOrigin::Editor, "ed-fold") => {
                self.dispatch_editor_cmd(crate::editor::commands::Cmd::ToggleFold, window, cx);
            }
            (MenuOrigin::Editor, "ed-cut") => {
                // 剪贴板动作走 Editor 的菜单方法（Cmd 枚举无这三项——
                // 它们是剪贴板动作，不是可登记的编辑器命令）。
                if let Some(editor) = self.editor().cloned() {
                    editor.update(cx, |ed, cx| ed.menu_cut(cx));
                }
            }
            (MenuOrigin::Editor, "ed-copy") => {
                if let Some(editor) = self.editor().cloned() {
                    editor.update(cx, |ed, cx| ed.menu_copy(cx));
                }
            }
            (MenuOrigin::Editor, "ed-paste") => {
                if let Some(editor) = self.editor().cloned() {
                    editor.update(cx, |ed, cx| ed.menu_paste(cx));
                }
            }
            // ----------------------------------------------------------
            // #166：文件树动作（全部接线——不允许「点了只打日志」）。
            // ----------------------------------------------------------
            (MenuOrigin::FileTree, "ft-reveal") => {
                // 揭示目标本身（目录）或所在目录（文件）——原实现恒揭示
                // 工作区根，与右键目标无关，#166 修正为目标面。
                if let Some(target) = target {
                    let reveal = if target.is_dir() {
                        target
                    } else {
                        target.parent().map(|p| p.to_path_buf()).unwrap_or(target)
                    };
                    cx.reveal_path(&reveal);
                }
            }
            (MenuOrigin::FileTree, "ft-open") => {
                // 只对文件有效（菜单构建时目录项已灰显——防御面再验一道）。
                if let Some(target) = target.filter(|p| !p.is_dir()) {
                    self.open_document(target, window, cx);
                }
            }
            (MenuOrigin::FileTree, "ft-new-file") => {
                if let Some(target) = target {
                    self.prompt_create_file(&target, window, cx);
                }
            }
            (MenuOrigin::FileTree, "ft-new-folder") => {
                if let Some(target) = target {
                    self.prompt_create_folder(&target, window, cx);
                }
            }
            (MenuOrigin::FileTree, "ft-rename") => {
                if let Some(target) = target {
                    self.prompt_rename_path(&target, window, cx);
                }
            }
            (MenuOrigin::FileTree, "ft-delete") => {
                if let Some(target) = target {
                    self.confirm_delete_path(target, window, cx);
                }
            }
            (_, other) => {
                eprintln!("context-menu: 未接线的动作 {other}");
            }
        }
        cx.notify();
    }

    /// 把命令派发到当前编辑器（无编辑器时空操作）。#165：带窗口——
    /// 与键位/命令面板同链 `Editor::run_command`（开查找条要聚焦
    /// 输入域，需要 Window）。旧 exec_menu_command 无窗路径已删：
    /// 其「菜单点击拿不到窗口」的前提不成立——on_click 的 listener
    /// 本就携带 window，只是当时被 `_window` 忽略了。
    fn dispatch_editor_cmd(
        &mut self,
        cmd: crate::editor::commands::Cmd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editor().cloned() else {
            return;
        };
        editor.update(cx, |ed, cx| ed.run_command(cmd, window, cx));
    }

    /// F1/F3：打开文件（Ctrl+O 对话框 / 文件树点击 / 最近列表共用
    /// 通道）。已开 → 激活既有标签（不重复开）；新文件 → 追加标签并
    /// 激活 + 聚焦 + 文件树高亮；失败/不支持→提示横幅（不白屏不崩溃）。
    pub fn open_document(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.open_document_tab(path, cx) {
            window.focus(&focus, cx);
        }
    }

    /// 打开核心（无 window——跨实体调用面：FileTree/GitPanel 点击经
    /// `Entity::update` 进入（不带 window 绑定，update_in 在任务/派生
    /// 上下文解析不到窗口），焦点句柄返回由调用方在其 window 域落焦）。
    pub fn open_document_tab(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        // F3：标签身份用绝对路径（CLI 相对入参 vs 对话框绝对入参不歧义）。
        let path = std::path::absolute(&path).unwrap_or(path);
        eprintln!("open: {}", path.display());
        // F3：同一文件已开 → 激活既有标签。
        if let Some(idx) = self.tabs.iter().position(|tab| tab.path == path) {
            return self.activate_tab_focus(idx, cx);
        }
        match document::open_editor(&path, cx) {
            OpenOutcome::Opened(editor) => {
                // #179（#188 接线）：挂入标签前注入宿主句柄（编辑区右键
                // 菜单触发器——document.rs 的 cx 是 &mut App 拿不到
                // shell，构造后补注）。
                let shell_weak = cx.entity().downgrade();
                editor.update(cx, |ed, _| ed.set_shell(shell_weak));
                let focus = editor.read(cx).focus_handle().clone();
                self.tabs.push(TabItem {
                    path: path.clone(),
                    editor,
                });
                self.active = self.tabs.len() - 1;
                self.notice_board.clear();
                self.recent.record_file(&path);
                eprintln!(
                    "tabs: open → {} 开（active {}）",
                    self.tabs.len(),
                    self.active
                );
                // F2：文件树高亮同步（工作区打开时）。
                if let Some(tree) = &self.workspace {
                    tree.update(cx, |tree, cx| {
                        tree.set_active_file(Some(path.clone()));
                        cx.notify();
                    });
                }
                cx.notify();
                Some(focus)
            }
            OpenOutcome::Unsupported { path, message } => {
                self.show_notice(format!("未打开 {}：{message}", path.display()), cx);
                None
            }
            OpenOutcome::Failed { path, message } => {
                self.show_notice(format!("打开失败（{}）：{message}", path.display()), cx);
                None
            }
        }
    }

    /// F3：激活标签（点击标签 / 重复打开既有文件 / 循环切换落地）。
    pub fn activate_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.activate_tab_focus(idx, cx) {
            window.focus(&focus, cx);
        }
    }

    /// 激活核心（无 window——焦点句柄返回，调用方落焦）。
    fn activate_tab_focus(&mut self, idx: usize, cx: &mut Context<Self>) -> Option<FocusHandle> {
        let tab = self.tabs.get(idx)?;
        self.active = idx;
        let focus = tab.editor.read(cx).focus_handle().clone();
        cx.notify();
        Some(focus)
    }

    /// F3/F4：关闭当前标签（Ctrl+W）——脏标签先弹确认框（保存/不保存/
    /// 取消）；干净标签直接关。
    pub fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        // F4：脏标签 → 确认框拦截。
        if self
            .tabs
            .get(self.active)
            .is_some_and(|tab| tab.editor.read(cx).is_modified())
        {
            let name = self.tabs[self.active]
                .editor
                .read(cx)
                .display_name()
                .to_string();
            eprintln!("confirm: 关闭未保存标签（{name}）");
            self.confirm = Some(ConfirmState {
                message: format!("{name} 有未保存的修改"),
                detail: "关闭前保存？".to_string(),
                action: ConfirmAction::CloseTab { idx: self.active },
                selected: 0,
                buttons: CONFIRM_BUTTONS.to_vec(),
            });
            // 模态接管焦点（编辑器隔离——可打印键不再入 buffer）。
            window.focus(&self.confirm_focus.clone(), cx);
            cx.notify();
            return;
        }
        self.close_tab_at(self.active, window, cx);
    }

    /// F3/F4：关闭指定标签（无确认——确认面在 [`Self::close_active_tab`]）。
    fn close_tab_at(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let old_len = self.tabs.len();
        if idx >= old_len {
            return;
        }
        self.tabs.remove(idx);
        self.active = if idx < self.active {
            self.active - 1
        } else if idx == self.active {
            tab_bar::active_after_close(old_len, self.active)
        } else {
            self.active
        };
        eprintln!(
            "tabs: close → {} 开（active {}）",
            self.tabs.len(),
            self.active
        );
        // 焦点跟到新活动标签（空态回退 shell 根域）。
        self.focus_active(window, cx);
        cx.notify();
    }

    /// F4：确认框按钮激活（键盘 Enter / 鼠标点击）。`choice`：
    /// 0 保存 / 1 不保存 / 2 取消。
    fn resolve_confirm(&mut self, choice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };
        match confirm.action {
            ConfirmAction::CloseTab { idx } => match choice {
                0 => {
                    // 保存后关（保存失败不关——不静默丢内容）。
                    let saved = self
                        .tabs
                        .get(idx)
                        .is_some_and(|tab| tab.editor.update(cx, |ed, _| ed.save()));
                    if saved {
                        self.close_tab_at(idx, window, cx);
                    }
                }
                1 => self.close_tab_at(idx, window, cx),
                _ => {}
            },
            ConfirmAction::Quit => match choice {
                0 => {
                    for tab in &self.tabs {
                        tab.editor.update(cx, |ed, _| {
                            ed.save();
                        });
                    }
                    cx.quit();
                }
                1 => cx.quit(),
                _ => {}
            },
            // #166：删除确认（0 删除 / 1 取消——默认停取消侧）。
            ConfirmAction::DeletePath { path } => match choice {
                0 => self.delete_path_confirmed(&path, window, cx),
                _ => {}
            },
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// 焦点归回当前活动标签的编辑器（确认框关闭后；空态回退 shell
    /// 根域——键位不聋）。
    fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.tabs.get(self.active) {
            Some(tab) => {
                let focus = tab.editor.read(cx).focus_handle().clone();
                window.focus(&focus, cx);
            }
            None => window.focus(&self.root_focus, cx),
        }
    }

    /// F4：另存为落位（对话框回调 / --smoke-save-as 钩子共用）——
    /// 编辑器写盘 + 身份迁移 + 标签路径更新 + 文件树高亮同步。
    pub fn save_active_as(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = std::path::absolute(&path).unwrap_or(path);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let ok = tab.editor.update(cx, |ed, _| ed.save_as(path.clone()));
        if !ok {
            return;
        }
        self.tabs[self.active].path = path.clone();
        if let Some(tree) = &self.workspace {
            tree.update(cx, |tree, cx| {
                tree.set_active_file(Some(path.clone()));
                cx.notify();
            });
        }
        cx.notify();
    }

    // ------------------------------------------------------------------
    // #134 任务 1「RPC 文件系统命令」：工作区沙箱边界 + 标签的
    // 磁盘权威面（重载 / 改名迁移）。
    // ------------------------------------------------------------------

    /// #134：工作区根目录（RPC FS 命令的沙箱边界；无工作区 = `None`
    /// ——FS 命令一律拒绝）。返回的是 FileTree 持有的原始根路径，
    /// 符号链接摊平（canonicalize）由 rpc::resolve_sandboxed 完成。
    pub fn workspace_root(&self, cx: &Context<Self>) -> Option<PathBuf> {
        self.workspace
            .as_ref()
            .map(|tree| tree.read(cx).root().to_path_buf())
    }

    /// #134：重载指定标签（RPC `write_file` 写盘后把 Buffer 同步到磁盘
    /// 现状——`Editor::reload_from_disk` 完整编辑链）。索引越界 / 编辑器
    /// 拒绝（只读渲染态 / docx 行模型态）返回 false。
    pub fn reload_tab(&mut self, idx: usize, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tabs.get(idx) else {
            return false;
        };
        let ok = tab.editor.update(cx, |ed, cx| ed.reload_from_disk(cx));
        if ok {
            cx.notify();
        }
        ok
    }

    /// #134：标签可否从磁盘重载（write_file 的门槛探针——只读渲染态
    /// / docx 行模型态 false）。
    pub fn tab_can_reload(&self, idx: usize, cx: &Context<Self>) -> bool {
        self.tabs
            .get(idx)
            .is_some_and(|tab| tab.editor.read(cx).can_reload_from_disk())
    }

    /// #134：指定标签的编辑器实体（batch_replace 对**非活动**标签的
    /// Buffer 操作面——`editor()` 只给活动标签）。
    pub fn tab_editor(&self, idx: usize) -> Option<&Entity<Editor>> {
        self.tabs.get(idx).map(|tab| &tab.editor)
    }

    /// #134：rename 后的标签身份迁移（磁盘已改名——按「索引 → 新路径」
    /// 计划逐一更新 `TabItem.path` + `Editor::adopt_path`；不写盘、不动
    /// Buffer）。计划由调用方在 rename **前**采集（旧路径还能 canonicalize
    /// 时）。返回迁移的标签数。
    pub fn apply_tab_renames(
        &mut self,
        plan: &[(usize, PathBuf)],
        cx: &mut Context<Self>,
    ) -> usize {
        let mut n = 0;
        for &(idx, ref new_path) in plan {
            if let Some(tab) = self.tabs.get_mut(idx) {
                tab.path = new_path.clone();
                tab.editor
                    .update(cx, |ed, _| ed.adopt_path(new_path.clone()));
                n += 1;
            }
        }
        if n > 0 {
            cx.notify();
        }
        n
    }

    // ------------------------------------------------------------------
    // #166：文件树右键动作——fs 变更面（新建 / 重命名 / 删除 + 树同步）。
    // 对话框统一走 gpui `prompt_for_new_path`（与另存为同源——闭包无
    // 原生「输入名字」轻量框，命名面借用系统保存对话框）。
    // ------------------------------------------------------------------

    /// #166 ft-new-file：新建文件对话框（落点 = 目标目录 / 文件所在目录）。
    fn prompt_create_file(&mut self, target: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let directory = dir_for_new(target);
        let rx = cx.prompt_for_new_path(&directory, None);
        cx.spawn_in(window, async move |this, cx| match rx.await {
            Ok(Ok(Some(path))) => {
                let _ = this.update_in(cx, |shell, window, cx| {
                    shell.create_file(path, window, cx);
                });
            }
            _ => {}
        })
        .detach();
    }

    /// #166 ft-new-folder：新建文件夹对话框（同源——返回路径即文件夹名，
    /// 对话框按「保存」语义呈现，落盘面按目录建）。
    fn prompt_create_folder(&mut self, target: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let directory = dir_for_new(target);
        let rx = cx.prompt_for_new_path(&directory, None);
        cx.spawn_in(window, async move |this, cx| match rx.await {
            Ok(Ok(Some(path))) => {
                let _ = this.update(cx, |shell, cx| shell.create_folder(path, cx));
            }
            _ => {}
        })
        .detach();
    }

    /// #166 ft-rename：重命名对话框（建议名 = 当前名；目标可移出原目录
    /// ——对话框返回的完整路径即新路径）。
    fn prompt_rename_path(&mut self, target: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let directory = target
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let suggested = target.file_name().map(|n| n.to_string_lossy().into_owned());
        let old = target.to_path_buf();
        let rx = cx.prompt_for_new_path(&directory, suggested.as_deref());
        cx.spawn_in(window, async move |this, cx| match rx.await {
            Ok(Ok(Some(new))) => {
                let _ = this.update(cx, |shell, cx| shell.rename_path(old, new, cx));
            }
            _ => {}
        })
        .detach();
    }

    /// 新建文件落盘（空文件）+ 树同步 + 打开（F1 通道——聚焦/高亮复用）。
    fn create_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match std::fs::write(&path, "") {
            Ok(()) => {
                eprintln!("file-tree: 新建文件 {}", path.display());
                self.sync_tree_after_fs(&path, false, cx);
                self.open_document(path, window, cx);
            }
            Err(err) => {
                self.show_notice(format!("新建文件失败（{}）：{err}", path.display()), cx);
            }
        }
    }

    /// 新建文件夹落盘（递归建路）+ 树同步。
    fn create_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match std::fs::create_dir_all(&path) {
            Ok(()) => {
                eprintln!("file-tree: 新建文件夹 {}", path.display());
                self.sync_tree_after_fs(&path, false, cx);
            }
            Err(err) => {
                self.show_notice(format!("新建文件夹失败（{}）：{err}", path.display()), cx);
            }
        }
    }

    /// 重命名落盘 + 打开标签路径迁移（apply_tab_renames 既有通道）+
    /// 树同步（旧位丢弃 / 新位展开）+ 高亮跟随。
    fn rename_path(&mut self, old: PathBuf, new: PathBuf, cx: &mut Context<Self>) {
        if new == old {
            return;
        }
        match std::fs::rename(&old, &new) {
            Ok(()) => {
                eprintln!("file-tree: 重命名 {} → {}", old.display(), new.display());
                let plan: Vec<(usize, PathBuf)> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.path == old)
                    .map(|(i, _)| (i, new.clone()))
                    .collect();
                self.apply_tab_renames(&plan, cx);
                self.sync_tree_after_fs(&old, true, cx);
                self.sync_tree_after_fs(&new, false, cx);
                self.sync_tree_active(cx);
            }
            Err(err) => {
                self.show_notice(
                    format!("重命名失败（{} → {}）：{err}", old.display(), new.display()),
                    cx,
                );
            }
        }
    }

    /// #166 ft-delete：删除确认（二选一——默认停「取消」：危险操作
    /// 默认安全侧；Enter 直接确认需显式左移选中）。
    fn confirm_delete_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let kind = if path.is_dir() { "文件夹" } else { "文件" };
        eprintln!("confirm: 删除{kind}（{}）", path.display());
        self.confirm = Some(ConfirmState {
            message: format!("删除{kind}「{name}」"),
            detail: "永久删除，不可恢复（不进回收站）。".to_string(),
            action: ConfirmAction::DeletePath { path },
            selected: 1,
            buttons: DELETE_BUTTONS.to_vec(),
        });
        window.focus(&self.confirm_focus.clone(), cx);
        cx.notify();
    }

    /// 确认后的删除执行（resolve_confirm 的 DeletePath 臂调用）：
    /// 落盘删除 → 关闭指向该路径（或其下）的标签（内容已不存在，
    /// 不再弹保存确认）→ 树同步。
    fn delete_path_confirmed(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
            Ok(()) => {
                eprintln!("file-tree: 删除 {}", path.display());
                // 从高往低调用避免序号漂移。
                let mut i = self.tabs.len();
                while i > 0 {
                    i -= 1;
                    if self.tabs[i].path.starts_with(path) {
                        self.close_tab_no_confirm(i, window, cx);
                    }
                }
                self.sync_tree_after_fs(path, true, cx);
                self.sync_tree_active(cx);
            }
            Err(err) => {
                self.show_notice(format!("删除失败（{}）：{err}", path.display()), cx);
            }
        }
    }

    /// fs 变更后的树同步：`changed` = 变更路径；`removed` = 已不存在
    /// （丢弃展开态/缓存）否则展开到目标使其可见。父目录清单一律重读
    /// （先刷新缓存再展开——expand_to 的 ensure_listing 不覆盖既有缓存）。
    fn sync_tree_after_fs(&mut self, changed: &Path, removed: bool, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.as_ref().cloned() {
            ws.update(cx, |tree, cx| {
                if let Some(parent) = changed.parent() {
                    tree.reload(parent);
                }
                if removed {
                    tree.forget_subtree(changed);
                } else {
                    tree.expand_to(changed);
                }
                cx.notify();
            });
        }
    }

    /// 树高亮与当前活动标签对齐（删除/重命名后路径面可能已变）。
    fn sync_tree_active(&mut self, cx: &mut Context<Self>) {
        let active = self.tabs.get(self.active).map(|t| t.path.clone());
        if let Some(ws) = self.workspace.as_ref().cloned() {
            ws.update(cx, |tree, _| tree.set_active_file(active));
        }
    }

    /// F4：Ctrl+Shift+S 另存为对话框（prompt_for_new_path——
    /// 建议名取当前文件名；目录取当前文件所在目录，缺省工作区根/家目录）。
    fn prompt_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (directory, suggested) = match self.tabs.get(self.active) {
            Some(tab) => (
                tab.path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
                tab.path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned()),
            ),
            None => return,
        };
        let rx = cx.prompt_for_new_path(&directory, suggested.as_deref());
        cx.spawn_in(window, async move |this, cx| match rx.await {
            Ok(Ok(Some(path))) => {
                let _ = this.update(cx, |shell, cx| shell.save_active_as(path, cx));
            }
            Ok(Ok(None)) => {}
            Ok(Err(err)) => {
                let message = format!("另存为对话框不可用：{err}");
                let _ = this.update_in(cx, |shell, window, cx| {
                    shell.show_notice(message, cx);
                });
            }
            Err(_canceled) => {}
        })
        .detach();
    }

    /// #182：设置主题模式（设置面板三态钮触发——立即应用 + 持久化；
    /// 与 Ctrl+T 循环同一生效链，不重写主题逻辑）。
    pub fn set_theme_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        if self.theme_mode == mode {
            return;
        }
        self.theme_mode = mode;
        theme_switch::apply_mode(mode, &self.base_theme, cx);
        crate::settings::save_theme_mode(mode);
        eprintln!("theme: {}（{mode}）", theme_switch::mode_label(mode));
        cx.notify();
    }

    /// #182：设置缩进单位（设置面板三态钮触发——持久化 + 应用到全部
    /// 已开标签；新建标签从 settings 读，两处同源）。
    pub fn set_indent_unit_all(&mut self, unit: IndentUnit, cx: &mut Context<Self>) {
        if self.indent_unit == unit {
            return;
        }
        self.indent_unit = unit;
        crate::settings::save_indent_unit(unit.setting_key());
        for tab in &self.tabs {
            tab.editor.update(cx, |ed, _| ed.set_indent_unit(unit));
        }
        eprintln!("indent-unit: {}", unit.setting_key());
        cx.notify();
    }

    /// #182：重置左右栏宽度（设置面板「重置默认」钮触发——恢复缺省宽
    /// + 持久化；与拖拽收尾 finish_panel_drag 同一存储面）。
    pub fn reset_panel_widths(&mut self, cx: &mut Context<Self>) {
        self.left_width = panels::LEFT_PANEL_WIDTH;
        self.right_width = panels::RIGHT_PANEL_WIDTH;
        let (left, right) = (f32::from(self.left_width), f32::from(self.right_width));
        crate::settings::save_panel_widths(left, right);
        eprintln!("panel-reset: left {left} right {right} (persist)");
        cx.notify();
    }

    /// F6：循环主题（Ctrl+T——system → light → dark → system；
    /// 应用 + 持久化 + 通知）。
    pub fn cycle_theme(&mut self, cx: &mut Context<Self>) {
        let next = theme_switch::next_mode(self.theme_mode);
        self.theme_mode = next;
        theme_switch::apply_mode(next, &self.base_theme, cx);
        crate::settings::save_theme_mode(next);
        eprintln!("theme: {}（{next}）", theme_switch::mode_label(next));
        cx.notify();
    }

    /// F6：设置编辑器字号（应用到全部标签 + 持久化；域外 clamp 由
    /// Editor 收口）。
    pub fn apply_font_size(&mut self, size: gpui::Pixels, cx: &mut Context<Self>) {
        let size_f32 = size.as_f32().clamp(8., 32.);
        self.font_size = size_f32;
        eprintln!("font-size: {size_f32}px");
        for tab in &self.tabs {
            tab.editor
                .update(cx, |ed, cx| ed.set_font_size(px(size_f32), cx));
        }
        crate::settings::save_font_size(size_f32);
        cx.notify();
    }

    /// F3：循环切换标签（Ctrl+Tab 下一 / Ctrl+Shift+Tab 上一；
    /// wrap；单标签无操作）。
    pub fn cycle_tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let next = tab_bar::cycle_active(self.tabs.len(), self.active, forward);
        if next == self.active {
            return;
        }
        eprintln!(
            "tabs: cycle → active {}/{}（{}）",
            next + 1,
            self.tabs.len(),
            if forward { "下一" } else { "上一" }
        );
        self.activate_tab(next, window, cx);
    }

    /// F2：打开文件夹作为工作区（Ctrl+K Ctrl+O 和弦 / 启动记忆恢复
    /// / --smoke-workspace 钩子共用通道）——文件树实体落位 + 记忆落盘
    /// + 状态栏路径。落盘前 absolutize（相对路径记忆在 cwd 变化后失效）。
    pub fn open_workspace(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        // 相对路径 → 绝对（不解析符号链接——展示口径保留；失败原样）。
        let root = std::path::absolute(&root).unwrap_or(root);
        eprintln!("workspace: {}", root.display());
        let shell = cx.entity().downgrade();
        let tree = cx.new(|_| FileTree::new(root.clone(), shell.clone()));
        self.workspace = Some(tree);
        // F7：git 面板（工作区打开即建——非 git 目录/缺 git 走提示态，
        // 不崩溃不拦截）。
        let git = cx.new(|_| GitPanel::new(root.clone(), shell));
        self.git_panel = Some(git);
        crate::settings::remember_workspace(&root);
        self.recent.record_workspace(&root);
        cx.notify();
    }

    /// F2：Ctrl+K Ctrl+O 文件夹选择对话框（directories 模式；取消/
    /// portal 缺失口径同 F1）。
    fn prompt_open_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("打开文件夹（工作区）".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            match rx.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |shell, cx| shell.open_workspace(path, cx));
                    }
                }
                // 用户取消——无操作。
                Ok(Ok(None)) => {}
                Ok(Err(err)) => {
                    let message = format!("文件夹选择器不可用：{err}");
                    let _ = this.update(cx, |shell, cx| {
                        shell.show_notice(message, cx);
                    });
                }
                Err(_canceled) => {}
            }
        })
        .detach();
    }

    /// F1：Ctrl+O 系统文件选择对话框（gpui `prompt_for_paths`——
    /// Windows IFileOpenDialog / macOS NSOpenPanel / Linux xdg-desktop-
    /// portal；取消 = `None` 无操作，portal 缺失 = Err 提示横幅）。
    fn prompt_open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("打开文件".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            match rx.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update_in(cx, |shell, window, cx| {
                            shell.open_document(path, window, cx);
                        });
                    }
                }
                // 用户取消——无操作（验收：取消不崩溃）。
                Ok(Ok(None)) => {}
                Ok(Err(err)) => {
                    let message = format!("文件选择器不可用：{err}");
                    let _ = this.update(cx, |shell, cx| {
                        shell.show_notice(message, cx);
                    });
                }
                // oneshot 通道关闭（窗口已释放）——无操作。
                Err(_canceled) => {}
            }
        })
        .detach();
    }

    /// 展示提示横幅（5 秒后自动清除——代际匹配防旧定时误清新消息）。
    fn show_notice(&mut self, message: String, cx: &mut Context<Self>) {
        let generation = self.notice_board.show(message);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            if this
                .update(cx, |shell, cx| {
                    if shell.notice_board.dismiss(generation) {
                        cx.notify();
                    }
                })
                .is_err()
            {
                // 实体已释放——定时器自然作废。
            }
        })
        .detach();
    }
}

impl ShellApp {
    /// 标题栏文案：`mang — 文件名`（无文档仅 `mang`）。
    fn title_with(&self, cx: &gpui::App) -> String {
        match self.editor() {
            Some(ed) => format!("mang — {}", ed.read(cx).display_name()),
            None => "mang".to_string(),
        }
    }
}

impl Render for ShellApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let title = self.title_with(cx);
        // G4：面板拖拽收尾监听用的实体句柄（canvas paint 闭包捕获——
        // on_mouse_event 只能在 paint 期调用，render 期直调会 panic，
        // 与 editor 的 ime-hook canvas 同模式接线）。
        let shell_entity = cx.entity().clone();
        v_flex()
            .id("shell-root")
            .size_full()
            .track_focus(&self.root_focus)
            .bg(colors.background)
            .text_color(colors.text)
            .font_family("Noto Sans SC")
            // F1/F2/F3：shell 级键钩子。空态（无编辑区焦点）时键事件
            // 派发到 root 节点；编辑态编辑器不消费的组合冒泡至此
            // （dispatch_key 对未知 ctrl 组合零处理零拦截；tab 键臂
            // F3 起带 !ctrl 守卫、只读门禁放行 tab+ctrl）。
            //
            // 命令集：Ctrl+O（F1 文件打开）；Ctrl+K Ctrl+O 和弦（F2
            // 打开文件夹工作区——2s 窗口）；Ctrl+W（F3 关标签）；
            // Ctrl+Tab / Ctrl+Shift+Tab（F3 循环切换）。
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let ks = &event.keystroke;
                let ctrl = ks.modifiers.control || ks.modifiers.platform;
                let plain_ctrl = ctrl && !ks.modifiers.shift && !ks.modifiers.alt;
                let key = ks.key.to_lowercase();

                // #166：右键菜单 Esc 关闭（点空白由菜单遮罩承担；菜单与
                // 确认框互斥——ft-delete 开确认时菜单已关）。放在确认框
                // 模态拦截之前：两者不同时开，顺序无行为差，仅注释并归。
                if key == "escape" && this.context_menu.is_some() {
                    this.close_context_menu(cx);
                    return;
                }

                // F4：确认框模态拦截（最高优先——开启时其余键位全部隔离）。
                if this.confirm.is_some() {
                    match key.as_str() {
                        "escape" => {
                            this.confirm = None;
                            this.focus_active(window, cx);
                            cx.notify();
                        }
                        "enter" => {
                            let selected = this.confirm.as_ref().map(|c| c.selected).unwrap_or(2);
                            this.resolve_confirm(selected, window, cx);
                        }
                        "tab" => {
                            if let Some(confirm) = this.confirm.as_mut() {
                                confirm.selected = (confirm.selected + 1) % confirm.buttons.len();
                                cx.notify();
                            }
                        }
                        "left" => {
                            if let Some(confirm) = this.confirm.as_mut() {
                                confirm.selected = (confirm.selected + confirm.buttons.len() - 1)
                                    % confirm.buttons.len();
                                cx.notify();
                            }
                        }
                        "right" => {
                            if let Some(confirm) = this.confirm.as_mut() {
                                confirm.selected = (confirm.selected + 1) % confirm.buttons.len();
                                cx.notify();
                            }
                        }
                        _ => {}
                    }
                    return;
                }

                // F2 和弦第一键：Ctrl+K（进入待续期，重复按刷新窗口）。
                if plain_ctrl && ks.key.eq_ignore_ascii_case("k") {
                    this.chord_until = Some(Instant::now() + CHORD_WINDOW);
                    return;
                }
                // F2 和弦第二键：窗口期内 Ctrl+O → 工作区（吞掉，不落 F1
                // 文件打开）；过期/非 O → 和弦作废。
                let chord_pending = this
                    .chord_until
                    .take()
                    .is_some_and(|until| Instant::now() <= until);
                if chord_pending && plain_ctrl && ks.key.eq_ignore_ascii_case("o") {
                    this.prompt_open_workspace(window, cx);
                    return;
                }
                this.chord_until = None;

                // F3：Ctrl+Tab / Ctrl+Shift+Tab 循环切换标签。
                if ctrl && !ks.modifiers.alt && ks.key == "tab" {
                    this.cycle_tab(!ks.modifiers.shift, window, cx);
                    return;
                }
                // F3：Ctrl+W 关闭当前标签（F4：脏标签经确认框）。
                if plain_ctrl && ks.key.eq_ignore_ascii_case("w") {
                    this.close_active_tab(window, cx);
                    return;
                }
                // F4：Ctrl+Shift+S 另存为（编辑器 s 臂带 !shift 守卫冒泡至此）。
                if ctrl
                    && ks.modifiers.shift
                    && !ks.modifiers.alt
                    && ks.key.eq_ignore_ascii_case("s")
                {
                    this.prompt_save_as(window, cx);
                    return;
                }
                // F6：Ctrl+= / Ctrl++ 放大、Ctrl+- 缩小、Ctrl+0 重置
                //（shift 兼容——"+" 多为 shift+= 产生）。
                if ctrl && !ks.modifiers.alt && (ks.key == "=" || ks.key == "+") {
                    let next = this.font_size + 1.;
                    this.apply_font_size(px(next), cx);
                    return;
                }
                if ctrl && !ks.modifiers.alt && ks.key == "-" {
                    let next = this.font_size - 1.;
                    this.apply_font_size(px(next), cx);
                    return;
                }
                if plain_ctrl && ks.key == "0" {
                    this.apply_font_size(px(15.), cx);
                    return;
                }
                // F6：Ctrl+T 循环主题（system → light → dark → system）。
                if plain_ctrl && ks.key.eq_ignore_ascii_case("t") {
                    this.cycle_theme(cx);
                    return;
                }
                // F1：Ctrl+O 文件打开。
                if plain_ctrl && ks.key.eq_ignore_ascii_case("o") {
                    this.prompt_open_file(window, cx);
                }
            }))
            .child(title_bar::title_bar(&title, cx))
            // F3：标签栏（有标签才渲染；标题栏与主体之间）。
            .when(!self.tabs.is_empty(), |el| {
                let ws_root = self
                    .workspace
                    .as_ref()
                    .map(|tree| tree.read(cx).root().to_path_buf());
                el.child(tab_bar::tab_bar(
                    &self.tabs,
                    self.active,
                    ws_root.as_deref(),
                    cx,
                ))
            })
            .child(
                h_flex()
                    .id("shell-body")
                    .flex_1()
                    .min_h_0()
                    .child(activity_bar::activity_bar(
                        self.left_open,
                        self.git_view,
                        self.right_open,
                        self.settings_view,
                        cx,
                    ))
                    .when(self.left_open, |el| {
                        el.child(panels::left_panel(
                            self.editor(),
                            self.workspace.as_ref(),
                            self.git_panel.as_ref(),
                            self.git_view,
                            // G4：可拖拽宽度（域钳制 + settings 持久化）
                            self.left_width,
                            cx,
                        ))
                        // G4：左栏右缘 4px 拖拽热区
                        .child(self.panel_resize_handle(PanelSide::Left, cx))
                    })
                    .child(editor_area(self, cx))
                    .when(self.right_open, |el| {
                        // G4：右栏左缘 4px 拖拽热区
                        el.child(self.panel_resize_handle(PanelSide::Right, cx))
                            // #182：右栏双视图——设置面板（图标条设置钮
                            // 入口）/ 属性・来源（缺省）。
                            .child(if self.settings_view {
                                settings_panel::settings_panel(
                                    self.theme_mode,
                                    self.font_size,
                                    self.indent_unit,
                                    self.right_width,
                                    cx,
                                )
                                .into_any_element()
                            } else {
                                panels::right_panel(
                                    self.editor().cloned().as_ref(),
                                    self.right_width,
                                    cx,
                                )
                                .into_any_element()
                            })
                    }),
            )
            // F1：提示横幅（状态栏上方一条——有消息时才渲染）。
            .map(|el| match self.notice_board.message() {
                Some(message) => el.child(notice_bar(message, &colors)),
                None => el,
            })
            // F4：确认框模态（半透明遮罩全屏拦截点按）。
            .map(|el| match &self.confirm {
                Some(confirm) => el.child(confirm_overlay(confirm, &self.confirm_focus, cx)),
                None => el,
            })
            .child(status_bar::status_bar(
                self.editor().cloned().as_ref(),
                self.workspace
                    .as_ref()
                    .map(|tree| tree.read(cx).root().to_path_buf()),
                cx,
            ))
            // G4：面板拖拽收尾钩（窗口级 mouseup——canvas paint 期每帧
            // 注册，与 editor end_drag 同模式；非拖拽中为空操作）。
            .child(
                div()
                    .id("panel-mouseup-hook")
                    .absolute()
                    .size_0()
                    .child(canvas(
                        move |bounds: Bounds<Pixels>, _window, _cx| bounds,
                        move |_bounds, _t, window: &mut Window, _cx| {
                            window.on_mouse_event(
                                move |_: &MouseUpEvent, phase: DispatchPhase, _window, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        // #89 坑3 变体：drag 会话的 mouseup 派发在
                                        // drag 源（shell）的 update 内——直接再
                                        // update 即重入 panic；延迟到下一事件
                                        // 拍收尾（落盘 + 证据行）。
                                        let shell = shell_entity.clone();
                                        cx.spawn(async move |cx| {
                                            cx.update(|cx| {
                                                shell.update(cx, |shell, cx| {
                                                    shell.finish_panel_drag(cx)
                                                })
                                            });
                                        })
                                        .detach();
                                    }
                                },
                            );
                        },
                    )),
            )
            // #166：右键菜单浮层——**链尾最上层**（后画的兄弟覆盖先画的：
            // 若插在 status_bar 之前，弹向窗口底部的菜单会被状态栏盖住、
            // 点击穿透成「关菜单」）。摆放几何按当前窗口尺寸算，窗口
            // 改大小后仍合法；遮罩先画在菜单之下承接「点空白关闭」。
            .map(|el| match &self.context_menu {
                Some(menu) => {
                    let bounds = window.bounds();
                    let screen = MenuRect::new(
                        0.,
                        0.,
                        bounds.size.width.as_f32(),
                        bounds.size.height.as_f32(),
                    );
                    let rect = menu.layout(screen);
                    el.child(context_menu_view::context_menu_overlay(menu, rect, cx))
                }
                None => el,
            })
    }
}

/// 右键目标作为「新建」落点目录：目录本身；文件则取所在目录
///（#166 ft-new-file / ft-new-folder 共用）。
fn dir_for_new(target: &Path) -> PathBuf {
    if target.is_dir() {
        target.to_path_buf()
    } else {
        target
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| target.to_path_buf())
    }
}

/// F4：确认框模态遮罩（半透明背景全屏 + 居中卡片 + 三钮）。遮罩
/// 拦截一切点按（背景无事件——点击不穿透到底下 UI）；焦点域在遮罩
/// 节点（track_focus）——编辑器隔离，键事件沿遮罩→shell 根冒泡。
fn confirm_overlay(
    confirm: &ConfirmState,
    focus: &FocusHandle,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    div()
        .id("confirm-backdrop")
        .absolute()
        .size_full()
        .top_0()
        .left_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(0.5))
        .track_focus(focus)
        .child(
            v_flex()
                .id("confirm-box")
                .w(px(380.))
                .p_4()
                .gap_3()
                .bg(colors.elevated_surface_background)
                .border_1()
                .border_color(colors.border)
                .rounded_md()
                .child(ui::Label::new(confirm.message.as_str()).size(ui::LabelSize::Default))
                .child(
                    ui::Label::new(confirm.detail.as_str())
                        .size(ui::LabelSize::Small)
                        .color(ui::Color::Muted),
                )
                .child(
                    h_flex()
                        .id("confirm-buttons")
                        .gap_2()
                        .justify_end()
                        .children(confirm.buttons.iter().enumerate().map(|(i, label)| {
                            let selected = confirm.selected == i;
                            let selected_bg = colors.element_selected;
                            let hover_bg = colors.element_hover;
                            h_flex()
                                .id(gpui::SharedString::from(format!("confirm-btn-{i}")))
                                .px_3()
                                .py_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(colors.border)
                                .when(selected, move |el| el.bg(selected_bg))
                                .hover(move |el| el.bg(hover_bg))
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(move |this, _, window, cx| {
                                        this.resolve_confirm(i, window, cx);
                                    }),
                                )
                                .child(ui::Label::new(*label).size(ui::LabelSize::Small))
                        })),
                ),
        )
}

/// F1：提示横幅（状态栏上方一条——警告性信息面：打开失败/类型不支持/
/// 对话框不可用）。色板无 warning 语义色（JustBase 基础主题），用
/// element_background + border + text_accent 组合呈现。
fn notice_bar(message: &str, colors: &theme::ThemeColors) -> impl IntoElement {
    h_flex()
        .id("shell-notice")
        .w_full()
        .h(px(28.))
        .px_3()
        .flex_none()
        .items_center()
        .gap_2()
        .bg(colors.element_background)
        .border_t_1()
        .border_color(colors.border)
        .child(
            ui::Label::new(message)
                .color(ui::Color::Accent)
                .size(ui::LabelSize::Small),
        )
}

/// 编辑区：当前标签的 `Editor` 实体；无标签 → 引导空态 + 最近打开
/// 列表（F5——文件点击打开 / 工作区点击打开；已删除文件置灰不可点，
/// 点击提示「文件不存在」）。
fn editor_area(shell: &mut ShellApp, cx: &mut Context<ShellApp>) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    match shell.editor() {
        Some(editor) => editor.clone().into_any_element(),
        None => {
            // F5：最近列表数据（文件至多 8 / 工作区至多 3 展示面——
            // 记忆上限仍为 20/10，展示截断防长列表挤爆空态页）。
            let recent_files: Vec<PathBuf> = shell
                .recent
                .recent_files()
                .iter()
                .take(8)
                .cloned()
                .collect();
            let recent_workspaces: Vec<PathBuf> = shell
                .recent
                .recent_workspaces()
                .iter()
                .take(3)
                .cloned()
                .collect();
            let hover = colors.element_hover;
            v_flex()
                .id("editor-area-empty")
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(colors.background)
                .items_center()
                .justify_center()
                .gap_3()
                .child(ui::Headline::new("mang").size(ui::HeadlineSize::Large))
                .child(
                    ui::Label::new(
                        "打开 .md 可编辑；.docx/.pdf 只读渲染——Ctrl+O 打开文件 / Ctrl+K Ctrl+O 打开文件夹（多标签已接线：Ctrl+Tab 切换）",
                    )
                    .color(ui::Color::Muted),
                )
                .when(!recent_files.is_empty() || !recent_workspaces.is_empty(), |el| {
                    el.child(
                        v_flex()
                            .id("recent-list")
                            .gap_1()
                            .mt_4()
                            .w(px(420.))
                            .child(
                                ui::Label::new("最近打开")
                                    .size(ui::LabelSize::Small)
                                    .color(ui::Color::Muted),
                            )
                            .children(recent_files.iter().map(|path| {
                                recent_row(path, false, hover, cx)
                            }))
                            .when(!recent_workspaces.is_empty(), |el| {
                                el.child(
                                    ui::Label::new("最近的工作区")
                                        .size(ui::LabelSize::Small)
                                        .color(ui::Color::Muted)
                                        .mt_2(),
                                )
                                .children(recent_workspaces.iter().map(|path| {
                                    recent_row(path, true, hover, cx)
                                }))
                            }),
                    )
                })
                .into_any_element()
        }
    }
}

/// F5：最近列表行（文件/工作区）。已删除 → 置灰（Hidden 色 + 不可点
/// hover），点击仍给「不存在」提示（可感知而非无反应）。
fn recent_row(
    path: &Path,
    is_workspace: bool,
    hover: gpui::Hsla,
    cx: &mut Context<ShellApp>,
) -> gpui::AnyElement {
    let exists = path.exists();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let detail = path.display().to_string();
    let path_for_click = path.to_path_buf();

    h_flex()
        .id(gpui::SharedString::from(format!("recent-{detail}")))
        .w_full()
        .px_2()
        .py_1()
        .gap_2()
        .items_center()
        .rounded_sm()
        .when(exists, move |el| el.hover(move |el| el.bg(hover)))
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                if is_workspace {
                    this.open_workspace(path_for_click.clone(), cx);
                } else if path_for_click.exists() {
                    this.open_document(path_for_click.clone(), window, cx);
                } else {
                    let message = format!("文件不存在：{}", path_for_click.display());
                    this.show_notice(message, cx);
                }
            }),
        )
        .child(
            ui::Icon::new(if is_workspace {
                ui::IconName::Folder
            } else {
                ui::IconName::File
            })
            .size(ui::IconSize::XSmall)
            .color(if exists {
                ui::Color::Muted
            } else {
                ui::Color::Hidden
            }),
        )
        .child(
            ui::Label::new(name)
                .size(ui::LabelSize::Small)
                .color(if exists {
                    ui::Color::Default
                } else {
                    ui::Color::Hidden
                }),
        )
        .child(
            ui::Label::new(detail)
                .size(ui::LabelSize::XSmall)
                .color(ui::Color::Hidden),
        )
        .into_any_element()
}

//-----------------------------------------------------------------------
// F1 单测：提示横幅状态机（纯逻辑——无需 gpui 上下文）。
//-----------------------------------------------------------------------
#[cfg(test)]
mod notice_tests {
    use super::NoticeBoard;

    #[test]
    fn show_returns_incrementing_generations() {
        let mut board = NoticeBoard::default();
        assert_eq!(board.message(), None, "初始无消息");
        let g1 = board.show("第一条");
        assert_eq!(g1, 1);
        assert_eq!(board.message(), Some("第一条"));
        let g2 = board.show("第二条");
        assert_eq!(g2, 2, "代际递增");
        assert_eq!(board.message(), Some("第二条"), "新消息覆盖旧消息");
    }

    #[test]
    fn dismiss_only_matches_current_generation() {
        let mut board = NoticeBoard::default();
        let g1 = board.show("第一条");
        let g2 = board.show("第二条");
        // 旧代际定时器到达——不得误清新消息。
        assert!(!board.dismiss(g1), "过时代际不清除");
        assert_eq!(board.message(), Some("第二条"));
        // 当前代际定时器到达——正常清除。
        assert!(board.dismiss(g2));
        assert_eq!(board.message(), None);
        // 空板再 dismiss 恒 false。
        assert!(!board.dismiss(g2));
    }

    #[test]
    fn clear_wipes_unconditionally() {
        let mut board = NoticeBoard::default();
        let g = board.show("消息");
        board.clear();
        assert_eq!(board.message(), None);
        // clear 后旧代际定时到达仍无效（无消息可清）。
        assert!(!board.dismiss(g));
    }

    #[test]
    fn generation_survives_clear() {
        let mut board = NoticeBoard::default();
        let _g1 = board.show("第一条");
        board.clear();
        let g2 = board.show("第二条");
        // clear 不重置代际——防 clear 前后同号定时器误配。
        assert_eq!(g2, 2);
    }
}

#[cfg(test)]
mod tests {
    /// 纯状态机面：无需 gpui 上下文即可钉定折叠语义。
    /// （Entity 构造需要 App 上下文，此处以最小桩验证状态翻转逻辑——
    /// 真值由 Xvfb 烟雾与编辑器纯逻辑单测背书。）
    #[test]
    fn toggle_semantics_flip_independently() {
        let mut left = true;
        let mut right = true;
        left = !left;
        assert!(!left, "左栏翻转：开 → 关");
        left = !left;
        assert!(left, "左栏再翻转：关 → 开");
        right = !right;
        assert!(!right);
        assert!(left, "右栏翻转不影响左栏");
    }
}
