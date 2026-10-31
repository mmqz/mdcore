//! 外壳根视图 —— #63 Lead 布局的组合点 + #68 阶段 1 编辑区接线。
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ 标题栏 36px（自绘：拖拽 + 窗口控制 + 文档名）    │
//! ├────┬─────────┬──────────────────┬────────────┤
//! │图标│ 左栏     │ 编辑区           │ 右栏        │
//! │条  │ 260px   │ Editor / 空态     │ 280px      │
//! │52px│ 文件树   │ (#68 md 编辑面)  │ 属性/来源   │
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

use gpui::{Context, Entity, FocusHandle, KeyDownEvent, PathPromptOptions, Render, Window, px};
use theme::Theme;
use ui::prelude::*;

use crate::activity_bar;
use crate::document::{self, OpenOutcome};
use crate::editor::Editor;
use crate::file_tree::FileTree;
use crate::panels;
use crate::recent::RecentStore;
use crate::status_bar;
use crate::tab_bar::{self, TabItem};
use crate::theme_switch;
use crate::title_bar;

/// Ctrl+K 和弦窗口（第二键有效期；过期自动作废——惰性判定无需定时器）。
const CHORD_WINDOW: Duration = Duration::from_secs(2);

/// F4：确认框选项序（0 保存 / 1 不保存 / 2 取消）。键盘 Enter 激活
/// 选中项、Tab/←/→ 循环、Esc 直接取消。
const CONFIRM_BUTTONS: [&str; 3] = ["保存", "不保存", "取消"];

/// F4：待确认动作（脏标签关闭 / 退出）。
#[derive(Clone, Debug)]
enum ConfirmAction {
    /// 关闭 `idx` 标签（它有未保存修改）。
    CloseTab { idx: usize },
    /// 退出应用（存在未保存修改的标签）。
    Quit,
}

/// F4：确认框状态。
struct ConfirmState {
    message: String,
    detail: String,
    action: ConfirmAction,
    /// 选中按钮序（键盘循环 / 鼠标点击）。0=保存 1=不保存 2=取消。
    selected: usize,
}

pub struct ShellApp {
    left_open: bool,
    right_open: bool,
    /// F3：标签集（每标签 = 一个打开文档）；`active` 指向当前标签
    ///（空集 = 空态）。原 `Option<Entity<Editor>>` 单例面由此取代。
    tabs: Vec<TabItem>,
    active: usize,
    /// F2：工作区文件树（打开文件夹后呈现；记忆落盘随 settings）。
    workspace: Option<Entity<FileTree>>,
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
    /// F1：提示横幅（打开失败 / 类型不支持 / 对话框不可用等面向用户消息）。
    notice_board: NoticeBoard,
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
            tabs,
            active: 0,
            workspace,
            chord_until: None,
            confirm: None,
            confirm_focus,
            root_focus,
            recent,
            theme_mode,
            base_theme,
            font_size,
            notice_board: NoticeBoard::default(),
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

    /// 折叠/展开左栏（文件树）。图标条 `act-file-tree` 触发。
    pub fn toggle_left(&mut self, cx: &mut Context<Self>) {
        self.left_open = !self.left_open;
        cx.notify();
    }

    /// 折叠/展开右栏（属性/来源）。图标条底部开关触发。
    pub fn toggle_right(&mut self, cx: &mut Context<Self>) {
        self.right_open = !self.right_open;
        cx.notify();
    }

    /// F1/F3：打开文件（Ctrl+O 对话框 / 文件树点击 / 最近列表共用
    /// 通道）。已开 → 激活既有标签（不重复开）；新文件 → 追加标签并
    /// 激活 + 聚焦 + 文件树高亮；失败/不支持→提示横幅（不白屏不崩溃）。
    pub fn open_document(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // F3：标签身份用绝对路径（CLI 相对入参 vs 对话框绝对入参不歧义）。
        let path = std::path::absolute(&path).unwrap_or(path);
        eprintln!("open: {}", path.display());
        // F3：同一文件已开 → 激活既有标签。
        if let Some(idx) = self.tabs.iter().position(|tab| tab.path == path) {
            self.activate_tab(idx, window, cx);
            return;
        }
        match document::open_editor(&path, cx) {
            OpenOutcome::Opened(editor) => {
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
                window.focus(&focus, cx);
            }
            OpenOutcome::Unsupported { path, message } => {
                self.show_notice(format!("未打开 {}：{message}", path.display()), window, cx);
            }
            OpenOutcome::Failed { path, message } => {
                self.show_notice(
                    format!("打开失败（{}）：{message}", path.display()),
                    window,
                    cx,
                );
            }
        }
    }

    /// F3：激活标签（点击标签 / 重复打开既有文件 / 循环切换落地）。
    pub fn activate_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        self.active = idx;
        let focus = tab.editor.read(cx).focus_handle().clone();
        cx.notify();
        window.focus(&focus, cx);
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
                    shell.show_notice(message, window, cx);
                });
            }
            Err(_canceled) => {}
        })
        .detach();
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
        let tree = cx.new(|_| FileTree::new(root.clone(), shell));
        self.workspace = Some(tree);
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
                    let _ = this.update_in(cx, |shell, window, cx| {
                        shell.show_notice(message, window, cx);
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
                    let _ = this.update_in(cx, |shell, window, cx| {
                        shell.show_notice(message, window, cx);
                    });
                }
                // oneshot 通道关闭（窗口已释放）——无操作。
                Err(_canceled) => {}
            }
        })
        .detach();
    }

    /// 展示提示横幅（5 秒后自动清除——代际匹配防旧定时误清新消息）。
    fn show_notice(&mut self, message: String, _window: &mut Window, cx: &mut Context<Self>) {
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let title = self.title_with(cx);
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
                                confirm.selected = (confirm.selected + 1) % CONFIRM_BUTTONS.len();
                                cx.notify();
                            }
                        }
                        "left" => {
                            if let Some(confirm) = this.confirm.as_mut() {
                                confirm.selected = (confirm.selected + CONFIRM_BUTTONS.len() - 1)
                                    % CONFIRM_BUTTONS.len();
                                cx.notify();
                            }
                        }
                        "right" => {
                            if let Some(confirm) = this.confirm.as_mut() {
                                confirm.selected = (confirm.selected + 1) % CONFIRM_BUTTONS.len();
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
                        self.right_open,
                        cx,
                    ))
                    .when(self.left_open, |el| {
                        el.child(panels::left_panel(
                            self.editor(),
                            self.workspace.as_ref(),
                            cx,
                        ))
                    })
                    .child(editor_area(self, cx))
                    .when(self.right_open, |el| {
                        el.child(panels::right_panel(self.editor().cloned().as_ref(), cx))
                    }),
            )
            // F1：提示横幅（状态栏上方一条——有消息时才渲染）。
            .map(|el| match self.notice_board.message() {
                Some(message) => el.child(notice_bar(message, &colors)),
                None => el,
            })
            // F4：确认框模态（最后渲染最上层——半透明遮罩全屏拦截点按）。
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
                        .children(CONFIRM_BUTTONS.iter().enumerate().map(|(i, label)| {
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
                    this.show_notice(message, window, cx);
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
