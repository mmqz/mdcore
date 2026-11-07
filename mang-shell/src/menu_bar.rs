//! 顶部菜单栏模型层（#209）——纯数据 / 纯状态机，零 GPUI 依赖。
//!
//! # 分层（与 context_menu ↔ context_menu_view 同构）
//!
//! - 本模块：**菜单结构**（四菜单 + 子菜单 + 动作映射）、**键盘导航状态机**、
//!   **下拉几何估算**——全部纯函数/纯数据，单测钉定，不起窗口可验；
//! - `menu_bar_view.rs`（bin 专属）：GPUI 渲染——标题行/浮层/遮罩/条目行/
//!   灰显，形态循 `context_menu_view.rs`；
//! - `shell.rs`：状态归属（`ShellApp.menu_bar` + `menu_focus` 焦点域）与
//!   动作派发（`activate_menu_action`——与右键菜单/命令面板同一分发单点）。
//!
//! 下拉摆放（边缘翻转/夹紧）与宽度估算**复用** `context_menu` 的
//! [`place_menu`](crate::context_menu::place_menu) /
//! [`place_submenu`](crate::context_menu::place_submenu) /
//! [`menu_size`](crate::context_menu::menu_size)——那套几何对着屏幕边界
//! 写过暴力扫描测试，不重写第二份。
//!
//! # 动作映射铁律（issue #209 层 2 / owner 指令）
//!
//! 菜单项**只做既有动作的映射**，不新增执行逻辑、不新增 Cmd 变体：
//!
//! | 菜单项类别 | 映射目标（既有单点） |
//! |---|---|
//! | 编辑器命令（保存/撤销/查找/替换…） | [`MenuAction::EditorCmd`] → `ShellApp::dispatch_editor_cmd` → `Editor::run_command`（与键位 / 命令面板 / 右键菜单 / 图标条搜索钮同一条链） |
//! | 剪贴板三件（剪切/复制/粘贴） | `Editor::menu_cut/menu_copy/menu_paste`（Cmd 枚举无剪贴板项——右键菜单 ed-cut 同款既有方法） |
//! | shell 级动作（打开/另存为/主题/字号/侧栏/退出/关于） | ShellApp 既有方法（`prompt_open_file` / `prompt_save_as` / `set_theme_mode` / `apply_font_size` / `toggle_left` / `cx.quit` / `show_notice`） |
//! | **无既有命令的项**（新建/状态栏/全屏/快捷键一览/文档） | 置灰禁用 + [`MenuAction::None`]，不绑动作——禁用态正确是本 issue 验收项 |
//!
//! # 快捷键标签的真实性
//!
//! 每个标签对应一条**真实存在的键位**（editor `dispatch_key` / shell
//! `on_key_down` 既有分支——见 `shortcut_labels_pin_real_bindings` 单测的
//! 钉定表）。无键位的项不标：标一条不存在的快捷键是可发现性欺骗，比不标
//! 更糟。重做标 `Ctrl+Shift+Z`（`dispatch_key` 的 `z if ctrl && shift` 臂
//! 实际绑定；`Ctrl+Y` 亦可，单标签取更常见的一条）。

use std::path::PathBuf;

use crate::context_menu::{self, MenuItem};
use crate::editor::commands::Cmd;

/// 主题模式串（settings.json `theme` 键的持久化契约值——与
/// `theme_switch::MODE_*` 同值；本模块在 lib 树内取不到 bin 专属的
/// theme_switch，按稳定契约自带一份，`theme_constants_pin_contract`
/// 单测钉住取值域）。
pub const THEME_SYSTEM: &str = "system";
pub const THEME_LIGHT: &str = "light";
pub const THEME_DARK: &str = "dark";

/// 最近打开子菜单的条目上限（与空态引导页 `take(8)` 同口径）。
pub const RECENT_MENU_MAX: usize = 8;

/// 最近打开子菜单的静态 id 池（`MenuItemSpec.id` 是 `&'static str`——
/// 动态条目按序取池位，超池截断；id 唯一性由 `ids_unique_across_menus`
/// 单测钉定）。
const RECENT_IDS: [&str; RECENT_MENU_MAX] = [
    "file-recent-0",
    "file-recent-1",
    "file-recent-2",
    "file-recent-3",
    "file-recent-4",
    "file-recent-5",
    "file-recent-6",
    "file-recent-7",
];

/// 菜单构建上下文（ShellApp 状态快照——纯数据，可单测）。
///
/// 每次「打开菜单 / 渲染浮层 / 键盘导航」时由 shell 现场采集：菜单可用性
/// （灰显）与勾选标记全部由这一份快照决定，模型自身零状态。
pub struct MenuContext {
    /// 有活动标签（保存/另存为/编辑类命令的门槛）。
    pub has_document: bool,
    /// 活动标签只读态（docx/pdf 渲染面——变更类命令灰显）。
    pub read_only: bool,
    /// 活动标签有选区（剪切/复制门槛——与编辑器右键菜单同判定源）。
    pub has_selection: bool,
    /// 当前编辑器字号（px——字号子菜单的域界灰显，8-32）。
    pub font_size: f32,
    /// 当前主题模式（主题子菜单的勾选标记）。
    pub theme_mode: &'static str,
    /// 左栏开合（侧栏子菜单勾选标记）。
    pub left_open: bool,
    /// 左栏当前是 git 视图（侧栏子菜单勾选标记）。
    pub git_view: bool,
    /// 活动标签是 DOCX（#307「导出为 PDF」门槛——仅 docx 形态提供）。
    pub active_is_docx: bool,
    /// 活动标签是 PDF（#307 ②「导出为 DOCX」门槛——仅 pdf 形态提供）。
    pub active_is_pdf: bool,
    /// 最近打开的文件（路径序 = 子菜单条目序；`OpenRecent(usize)` 按此序回查）。
    pub recent_files: Vec<PathBuf>,
}

/// 菜单动作目标（shell `activate_menu_action` 的分派键——只映射既有动作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// 编辑器命令（Cmd 枚举——与键位/命令面板同一分派路径）。
    EditorCmd(Cmd),
    /// 剪贴板：剪切（`Editor::menu_cut`——Cmd 枚举无剪贴板项）。
    Cut,
    /// 剪贴板：复制（`Editor::menu_copy`）。
    Copy,
    /// 剪贴板：粘贴（`Editor::menu_paste`）。
    Paste,
    /// 打开文件（shell 既有 `prompt_open_file`——Ctrl+O 同链）。
    OpenFile,
    /// 打开文件夹（shell 既有 `prompt_open_workspace`——Ctrl+K Ctrl+O 同链）。
    OpenFolder,
    /// 打开最近文件（序 = `MenuContext.recent_files` 序；shell 回查路径打开）。
    OpenRecent(usize),
    /// 另存为（shell 既有 `prompt_save_as`——Ctrl+Shift+S 同链）。
    SaveAs,
    /// #307 阶段①：导出为 PDF（shell `export_active_pdf`——DOCX 经
    /// LibreOffice headless 转 PDF；仅 docx 标签启用）。
    ExportPdf,
    /// #307 ②功能独立项：导出为 DOCX（shell `export_active_docx`——
    /// PDF 文本层经 mdc-pdf 引擎提取 + office-io 写面构造；仅 pdf 标签
    /// 启用；扫描件零文本显式拒（OCR 面 #306））。
    ExportDocx,
    /// 退出（与标题栏 × 钮同一条 `cx.quit` 既有路径）。
    Quit,
    /// 关于（既有提示横幅 `show_notice` 承载——层 3 可发现性验收项）。
    About,
    /// 命令面板（`Editor::toggle_command_palette`——Ctrl+Shift+P 键位臂
    /// 的既有逻辑提取为公开方法，两入口共用，行为零变化）。
    TogglePalette,
    /// #289-⑤：打开设置弹窗（shell `open_settings`——左条设置钮同链；
    /// 弹窗形态，不占任何 dock 视图）。
    OpenSettings,
    /// 设置主题模式（shell 既有 `set_theme_mode`）。
    SetTheme(&'static str),
    /// 字号增大 1px（shell 既有 `apply_font_size`——Ctrl+= 同链）。
    FontBigger,
    /// 字号减小 1px（Ctrl+- 同链）。
    FontSmaller,
    /// 字号重置 15px（Ctrl+0 同链）。
    FontReset,
    /// 切换文件树面板（shell 既有 `toggle_left`——图标条文件树钮同链）。
    ToggleFileTreePanel,
    /// 切换 git 面板（shell 既有 `toggle_git`——图标条 git 钮同链）。
    ToggleGitPanel,
    /// 占位（无既有命令可映射——置灰项不绑动作，派发面只留证据行）。
    None,
}

/// 菜单动作项规格（标签 / 快捷键标签 / 动作 / 灰显 / 勾选）。
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItemSpec {
    /// 派发键（渲染层 element id 与日志定位用；全菜单唯一——单测钉定）。
    pub id: &'static str,
    pub label: String,
    /// 右侧快捷键标签（**从 keymap 表生成**——单一事实源，用户换绑后
    /// 跟随；只标真实键位，见模块文档「快捷键标签的真实性」）。
    pub shortcut: Option<String>,
    pub action: MenuAction,
    /// false = 置灰不可点（灰显而非消失——菜单长短不跳变，右键菜单同款）。
    pub enabled: bool,
    /// 左侧勾选标记（主题/侧栏子菜单的当前态）。
    pub checked: bool,
}

/// 一条菜单条目（动作 / 分隔线 / 子菜单——子菜单只一层，build_menus
/// 结构保证）。
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item(MenuItemSpec),
    Separator,
    Submenu {
        label: String,
        items: Vec<MenuEntry>,
    },
}

impl MenuEntry {
    /// 动作项构造（缺省可用、无快捷键、无勾选）。
    pub fn item(id: &'static str, label: impl Into<String>) -> Self {
        MenuEntry::Item(MenuItemSpec {
            id,
            label: label.into(),
            shortcut: None,
            action: MenuAction::None,
            enabled: true,
            checked: false,
        })
    }

    /// 子菜单构造（本菜单栏只允许一层子菜单——查看菜单的「主题/字号/
    /// 侧栏」与文件菜单的「最近打开」）。
    pub fn submenu(label: impl Into<String>, items: Vec<MenuEntry>) -> Self {
        MenuEntry::Submenu {
            label: label.into(),
            items,
        }
    }

    /// 附加快捷键标签（只对动作项生效——字面量口，测试/无表动作用；
    /// 产物默认走 [`Self::shortcut_for`]）。
    pub fn shortcut(mut self, key: impl Into<String>) -> Self {
        if let MenuEntry::Item(spec) = &mut self {
            spec.shortcut = Some(key.into());
        }
        self
    }

    /// 从 keymap 表取该动作的当前键位标签（#217 单一事实源：默认表
    /// = 现状键位镜像；用户换绑后菜单标签跟随。无绑定 → 不标）。
    pub fn shortcut_for(mut self, action_id: &str) -> Self {
        if let MenuEntry::Item(spec) = &mut self {
            spec.shortcut = crate::keymap::store().label(action_id);
        }
        self
    }

    /// 绑定动作。
    pub fn act(mut self, action: MenuAction) -> Self {
        if let MenuEntry::Item(spec) = &mut self {
            spec.action = action;
        }
        self
    }

    /// 置灰/启用。
    pub fn with_enabled(mut self, on: bool) -> Self {
        if let MenuEntry::Item(spec) = &mut self {
            spec.enabled = on;
        }
        self
    }

    /// 勾选标记。
    pub fn checked(mut self, on: bool) -> Self {
        if let MenuEntry::Item(spec) = &mut self {
            spec.checked = on;
        }
        self
    }

    /// 占位高度（行）——与 `context_menu::MenuItem::height_units` 同口径
    ///（下拉高度估算/子菜单行定位共用）。
    pub fn height_units(&self) -> f32 {
        match self {
            MenuEntry::Separator => context_menu::SEPARATOR_UNITS,
            _ => 1.0,
        }
    }
}

/// 一个顶部菜单（文件/编辑/查看/帮助）。
#[derive(Clone, Debug, PartialEq)]
pub struct TopMenu {
    pub id: &'static str,
    /// 标题栏里的标题文案（也是键盘切菜单时锚点估算的宽度输入）。
    pub label: &'static str,
    pub entries: Vec<MenuEntry>,
}

/// 按上下文快照构建四菜单（纯函数——结构与灰显/勾选全部由 ctx 决定）。
pub fn build_menus(ctx: &MenuContext) -> Vec<TopMenu> {
    let doc = ctx.has_document;
    let writable = doc && !ctx.read_only;
    vec![
        TopMenu {
            id: "menu-file",
            label: "文件",
            entries: vec![
                // 无既有「新建文档」逻辑（Ctrl+N 未接线，也无空标签构造
                // 通道）——置灰不绑动作（owner 指令：无命令的项禁用）。
                MenuEntry::item("file-new", "新建").with_enabled(false),
                MenuEntry::item("file-open", "打开")
                    .shortcut_for("prompt_open_file")
                    .act(MenuAction::OpenFile),
                MenuEntry::item("file-open-folder", "打开文件夹")
                    .shortcut_for("prompt_open_workspace")
                    .act(MenuAction::OpenFolder),
                MenuEntry::item("file-save", "保存")
                    .shortcut_for("save")
                    .act(MenuAction::EditorCmd(Cmd::Save))
                    .with_enabled(doc),
                MenuEntry::item("file-save-as", "另存为")
                    .shortcut_for("prompt_save_as")
                    .act(MenuAction::SaveAs)
                    .with_enabled(doc),
                MenuEntry::item("file-export-pdf", "导出为 PDF")
                    .act(MenuAction::ExportPdf)
                    .with_enabled(ctx.active_is_docx),
                MenuEntry::item("file-export-docx", "导出为 DOCX（文本版）")
                    .act(MenuAction::ExportDocx)
                    .with_enabled(ctx.active_is_pdf),
                MenuEntry::Separator,
                MenuEntry::submenu("最近打开", recent_entries(ctx)),
                MenuEntry::Separator,
                MenuEntry::item("file-quit", "退出").act(MenuAction::Quit),
            ],
        },
        TopMenu {
            id: "menu-edit",
            label: "编辑",
            entries: vec![
                MenuEntry::item("edit-undo", "撤销")
                    .shortcut_for("undo")
                    .act(MenuAction::EditorCmd(Cmd::Undo))
                    .with_enabled(writable),
                // dispatch_key 的 redo 实际绑定是 Ctrl+Shift+Z（z if
                // ctrl && shift 臂）与 Ctrl+Y 双臂——标签取前者。
                MenuEntry::item("edit-redo", "重做")
                    .shortcut_for("redo")
                    .act(MenuAction::EditorCmd(Cmd::Redo))
                    .with_enabled(writable),
                MenuEntry::Separator,
                MenuEntry::item("edit-cut", "剪切")
                    .shortcut_for("cut")
                    .act(MenuAction::Cut)
                    .with_enabled(writable && ctx.has_selection),
                MenuEntry::item("edit-copy", "复制")
                    .shortcut_for("copy")
                    .act(MenuAction::Copy)
                    .with_enabled(doc && ctx.has_selection),
                MenuEntry::item("edit-paste", "粘贴")
                    .shortcut_for("paste")
                    .act(MenuAction::Paste)
                    .with_enabled(writable),
                MenuEntry::Separator,
                MenuEntry::item("edit-find", "查找")
                    .shortcut_for("open_find")
                    .act(MenuAction::EditorCmd(Cmd::OpenFind))
                    .with_enabled(doc),
                MenuEntry::item("edit-replace", "替换")
                    .shortcut_for("open_replace")
                    .act(MenuAction::EditorCmd(Cmd::OpenReplace))
                    .with_enabled(writable),
                MenuEntry::item("edit-palette", "命令面板")
                    .shortcut_for("toggle_command_palette")
                    .act(MenuAction::TogglePalette)
                    .with_enabled(doc),
            ],
        },
        TopMenu {
            id: "menu-view",
            label: "查看",
            entries: vec![
                MenuEntry::submenu(
                    "主题",
                    vec![
                        MenuEntry::item("view-theme-system", "跟随系统")
                            .act(MenuAction::SetTheme(THEME_SYSTEM))
                            .checked(ctx.theme_mode == THEME_SYSTEM),
                        MenuEntry::item("view-theme-light", "浅色")
                            .act(MenuAction::SetTheme(THEME_LIGHT))
                            .checked(ctx.theme_mode == THEME_LIGHT),
                        MenuEntry::item("view-theme-dark", "深色")
                            .act(MenuAction::SetTheme(THEME_DARK))
                            .checked(ctx.theme_mode == THEME_DARK),
                    ],
                ),
                MenuEntry::submenu(
                    "字号",
                    vec![
                        // 域界灰显（设置面板步进钮同口径：8-32）。
                        MenuEntry::item("view-font-inc", "增大")
                            .shortcut_for("font_bigger")
                            .act(MenuAction::FontBigger)
                            .with_enabled(ctx.font_size < 32.),
                        MenuEntry::item("view-font-dec", "减小")
                            .shortcut_for("font_smaller")
                            .act(MenuAction::FontSmaller)
                            .with_enabled(ctx.font_size > 8.),
                        MenuEntry::item("view-font-reset", "重置")
                            .shortcut_for("font_reset")
                            .act(MenuAction::FontReset),
                    ],
                ),
                MenuEntry::submenu(
                    "侧栏",
                    vec![
                        MenuEntry::item("view-side-file-tree", "文件树")
                            .act(MenuAction::ToggleFileTreePanel)
                            .checked(ctx.left_open && !ctx.git_view),
                        MenuEntry::item("view-side-git", "改动 / 历史（git）")
                            .act(MenuAction::ToggleGitPanel)
                            .checked(ctx.left_open && ctx.git_view),
                        // 搜索 = 既有 Cmd::OpenFind（图标条搜索钮同链）。
                        MenuEntry::item("view-side-search", "搜索")
                            .shortcut_for("open_find")
                            .act(MenuAction::EditorCmd(Cmd::OpenFind))
                            .with_enabled(doc),
                    ],
                ),
                MenuEntry::Separator,
                // 状态栏开关无既有命令（status_bar 恒渲染）——置灰。
                MenuEntry::item("view-status-bar", "状态栏").with_enabled(false),
                // 全屏无既有命令（owner 点名的禁用示例）——置灰。
                MenuEntry::item("view-fullscreen", "全屏").with_enabled(false),
                // #289-⑤：设置入口（弹窗形态——vscode「偏好沉底」同款
                // 位置；与左条设置钮同一条 open_settings 链，不占右栏）。
                MenuEntry::item("view-settings", "设置").act(MenuAction::OpenSettings),
            ],
        },
        TopMenu {
            id: "menu-help",
            label: "帮助",
            entries: vec![
                MenuEntry::item("help-about", "关于").act(MenuAction::About),
                // 无既有快捷键一览面——置灰。
                MenuEntry::item("help-shortcuts", "快捷键一览").with_enabled(false),
                // 无应用内文档面（owner 点名的禁用示例）——置灰。
                MenuEntry::item("help-docs", "文档").with_enabled(false),
            ],
        },
    ]
}

/// 「最近打开」子菜单（文件名标签；已删除的置灰——空态引导页同口径；
/// 空列表给一条置灰占位，子菜单不塌成零高度）。
fn recent_entries(ctx: &MenuContext) -> Vec<MenuEntry> {
    if ctx.recent_files.is_empty() {
        return vec![MenuEntry::item("file-recent-empty", "（暂无记录）").with_enabled(false)];
    }
    ctx.recent_files
        .iter()
        .take(RECENT_MENU_MAX)
        .enumerate()
        .map(|(i, path)| {
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            MenuEntry::item(RECENT_IDS[i], label)
                .act(MenuAction::OpenRecent(i))
                .with_enabled(path.exists())
        })
        .collect()
}

// ─── 键盘导航状态机 ─────────────────────────────────────────────────
//
// 状态归 ShellApp（menu_bar 字段）；本节全部纯方法，对着 build_menus
// 的结构做转移。防御口径：菜单结构在打开期间理论不变（模态），但索引
// 全部按「当前 menus 越界即自愈/无操作」处理——结构漂移不 panic。

/// 菜单栏导航状态（哪个菜单开 / 选中项 / 子菜单开合 / 下拉锚点 x）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBarState {
    /// 打开的顶部菜单序号（None = 全关）。
    pub open: Option<usize>,
    /// 顶部菜单内的选中条目序（**原始索引域**——含分隔线；导航跳过分隔线，
    /// 但索引不重编，结构漂移时天然自洽）。
    pub selected: usize,
    /// 打开的子菜单所在条目序（None = 无子菜单展开）。
    pub open_submenu: Option<usize>,
    /// 子菜单内选中条目序（原始索引域，同上）。
    pub submenu_selected: usize,
    /// 下拉锚点 x（窗口坐标——标题点击时的 event.position.x；键盘切菜单
    /// 时按 [`estimate_anchor_x`] 估算重锚）。
    pub anchor_x: f32,
}

/// 条目是否可停靠（分隔线不可——灰显项**可**停靠：桌面菜单惯例，禁用项
/// 可被高亮查看但 Enter 无操作）。
fn is_navigable(entry: &MenuEntry) -> bool {
    !matches!(entry, MenuEntry::Separator)
}

/// 首个可停靠条目序（全分隔线/空 → None）。
fn first_navigable(entries: &[MenuEntry]) -> Option<usize> {
    entries.iter().position(is_navigable)
}

/// 从 `from` 起向前/向后找下一个可停靠条目（wrap；`from` 本身可停靠且
/// 是唯一可停靠项时回到自身；无可停靠 → None）。
fn step_navigable(entries: &[MenuEntry], from: usize, forward: bool) -> Option<usize> {
    let len = entries.len();
    if len == 0 {
        return None;
    }
    let mut i = from % len;
    for _ in 0..len {
        i = if forward {
            (i + 1) % len
        } else {
            (i + len - 1) % len
        };
        if is_navigable(&entries[i]) {
            return Some(i);
        }
    }
    None
}

impl MenuBarState {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// 打开顶部菜单 `idx`（锚点 = 点击 x；选中重置到首个可停靠条目）。
    pub fn open_menu(&mut self, idx: usize, anchor_x: f32, menus: &[TopMenu]) {
        self.open = Some(idx);
        self.anchor_x = anchor_x;
        self.open_submenu = None;
        self.submenu_selected = 0;
        self.selected = menus
            .get(idx)
            .and_then(|m| first_navigable(&m.entries))
            .unwrap_or(0);
    }

    /// 全关（Esc / 动作执行后；幂等——重复关无副作用）。选中序不清零：
    /// 重开时 `open_menu` 会重置，残留值不可达。
    pub fn close(&mut self) {
        self.open = None;
        self.open_submenu = None;
    }

    /// ↑↓ 移动（子菜单展开时在子菜单层移动，否则在顶部菜单层）。
    /// 返回是否变化（调用方决定是否重绘）。
    pub fn move_vertical(&mut self, menus: &[TopMenu], forward: bool) -> bool {
        let Some(menu) = menus.get(self.open.unwrap_or(usize::MAX)) else {
            return false;
        };
        if let Some(sub_idx) = self.open_submenu {
            match menu.entries.get(sub_idx) {
                Some(MenuEntry::Submenu { items, .. }) => {
                    if let Some(next) = step_navigable(items, self.submenu_selected, forward) {
                        let changed = next != self.submenu_selected;
                        self.submenu_selected = next;
                        return changed;
                    }
                    return false;
                }
                // 结构漂移防御：子菜单项不在了——退回顶部层继续导航。
                _ => self.open_submenu = None,
            }
        }
        match step_navigable(&menu.entries, self.selected, forward) {
            Some(next) => {
                let changed = next != self.selected;
                self.selected = next;
                changed
            }
            None => false,
        }
    }

    /// 当前选中条目是否是子菜单标题（Enter/→ 进入子菜单的判定）。
    pub fn selected_is_submenu(&self, menus: &[TopMenu]) -> bool {
        matches!(
            menus
                .get(self.open.unwrap_or(usize::MAX))
                .and_then(|m| m.entries.get(self.selected)),
            Some(MenuEntry::Submenu { .. })
        )
    }

    /// 打开 `entry_idx` 处的子菜单（非子菜单条目 = 无操作）。
    pub fn open_submenu_at(&mut self, menus: &[TopMenu], entry_idx: usize) -> bool {
        let Some(menu) = menus.get(self.open.unwrap_or(usize::MAX)) else {
            return false;
        };
        match menu.entries.get(entry_idx) {
            Some(MenuEntry::Submenu { items, .. }) => {
                if self.open_submenu == Some(entry_idx) {
                    return false;
                }
                self.open_submenu = Some(entry_idx);
                self.submenu_selected = first_navigable(items).unwrap_or(0);
                true
            }
            _ => false,
        }
    }

    /// 关闭子菜单（← 在子菜单层）。返回是否变化。
    pub fn close_submenu(&mut self) -> bool {
        self.open_submenu.take().is_some()
    }

    /// ←→ 切换顶部菜单（子菜单收起、选中重置、锚点按估算重锚）。
    pub fn switch_menu(&mut self, menus: &[TopMenu], forward: bool, title_text: &str) -> bool {
        let Some(cur) = self.open else {
            return false;
        };
        let len = menus.len();
        if len == 0 {
            self.close();
            return false;
        }
        let next = (cur + if forward { 1 } else { len - 1 }) % len;
        self.open_submenu = None;
        self.open = Some(next);
        self.selected = first_navigable(&menus[next].entries).unwrap_or(0);
        self.anchor_x = estimate_anchor_x(menus, next, title_text);
        true
    }

    /// 当前键盘焦点项（子菜单展开 = 子菜单选中项；否则 = 顶部菜单选中项）。
    /// 分隔线 / 子菜单标题 / 越界 → None。
    pub fn focused_item<'a>(&self, menus: &'a [TopMenu]) -> Option<&'a MenuItemSpec> {
        let menu = menus.get(self.open?)?;
        if let Some(sub_idx) = self.open_submenu {
            return match menu.entries.get(sub_idx)? {
                MenuEntry::Submenu { items, .. } => match items.get(self.submenu_selected)? {
                    MenuEntry::Item(spec) => Some(spec),
                    _ => None,
                },
                _ => None,
            };
        }
        match menu.entries.get(self.selected)? {
            MenuEntry::Item(spec) => Some(spec),
            _ => None,
        }
    }

    /// Enter 激活焦点项（禁用 / 分隔线 / 子菜单标题 → None；激活即全关
    /// 菜单——动作执行面统一由 shell 收口焦点）。
    fn take_action(&mut self, menus: &[TopMenu]) -> Option<MenuAction> {
        let action = match self.focused_item(menus) {
            Some(spec) if spec.enabled => spec.action,
            _ => return None,
        };
        self.close();
        Some(action)
    }
}

/// 键盘导航一步（纯状态转移——动作执行在 shell，保持本函数可单测）。
///
/// 返回 `Some(action)` = 需执行动作（Enter 且焦点项可用；状态已关）；
/// `None` = 仅导航 / 无操作。键位集：Esc 关闭、↑↓ 移动、← 关子菜单或切
/// 上一菜单、→ 进子菜单或切下一菜单、Enter 进子菜单或激活。其余键
/// `None`（shell 侧整段吞掉——菜单开着 = 模态，确认框同款语义）。
pub fn navigate(
    state: &mut MenuBarState,
    menus: &[TopMenu],
    key: &str,
    title_text: &str,
) -> Option<MenuAction> {
    match key {
        "escape" => {
            state.close();
            None
        }
        "up" => {
            state.move_vertical(menus, false);
            None
        }
        "down" => {
            state.move_vertical(menus, true);
            None
        }
        "left" => {
            if state.open_submenu.is_some() {
                state.close_submenu();
            } else {
                state.switch_menu(menus, false, title_text);
            }
            None
        }
        "right" => {
            if state.open_submenu.is_none() {
                if state.selected_is_submenu(menus) {
                    state.open_submenu_at(menus, state.selected);
                } else {
                    state.switch_menu(menus, true, title_text);
                }
            }
            None
        }
        "enter" => {
            if state.open_submenu.is_none() && state.selected_is_submenu(menus) {
                state.open_submenu_at(menus, state.selected);
                None
            } else {
                state.take_action(menus)
            }
        }
        _ => None,
    }
}

// ─── 几何估算（复用 context_menu 的摆放算法） ────────────────────────

/// 把本模型条目转成 `context_menu::MenuItem`（`menu_size` / 摆放算法的
/// 入参类型——宽度/高度估算与右键菜单同源，不重写第二份）。
pub fn to_context_items(entries: &[MenuEntry]) -> Vec<MenuItem> {
    entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item(spec) => MenuItem::Action {
                id: spec.id,
                label: spec.label.clone(),
                shortcut: spec.shortcut.clone(),
                enabled: spec.enabled,
            },
            MenuEntry::Separator => MenuItem::Separator,
            // 几何估算只看子菜单标题宽度（menu_size 口径）——内容不参与。
            MenuEntry::Submenu { label, .. } => MenuItem::Submenu {
                label: label.clone(),
                items: Vec::new(),
            },
        })
        .collect()
}

/// 条目 `idx` 顶部在菜单内的 y 偏移（含菜单上内边距——`place_submenu`
/// 的 `row_y` 参数按「row 顶相对菜单顶」口径回扣 MENU_PADDING_Y）。
pub fn row_y_offset(entries: &[MenuEntry], idx: usize) -> f32 {
    let mut y = context_menu::MENU_PADDING_Y;
    for (i, entry) in entries.iter().enumerate() {
        if i >= idx {
            break;
        }
        y += context_menu::ITEM_HEIGHT * entry.height_units();
    }
    y
}

// ─── 标题栏布局常量（视图层与锚点估算共用——两处必须同值） ──────────

/// 菜单标题按钮左右内边距（px_3——menu_bar_view 同值）。
///
/// #289-②：菜单栏贴窗口左缘（旧 TITLE_LEFT_PAD/TITLE_TO_MENUS_GAP
/// 随「左侧标题列」一起废弃——首个菜单自身的 px 即左缘内边距）。
pub const TITLE_BTN_PAD_X: f32 = 12.;
/// 菜单标题按钮间距（gap_1）。
pub const TITLES_GAP: f32 = 4.;

/// 键盘切菜单时的下拉锚点估算（标题行从左到右累加宽度）。
///
/// 鼠标路径不走估算（on_mouse_down 捕获真实点击 x）；只有 ←→ 键盘切换
/// 才用本估算重锚——误差 ±10px 量级（字符宽度近似），换取零测量成本。
///
/// #289-②：菜单栏贴窗口左缘后，锚点不再受应用/文档标题影响（`title_text`
/// 参数保留是为了 navigate 调用面稳定，布局上已不参与估算）。
pub fn estimate_anchor_x(menus: &[TopMenu], idx: usize, _title_text: &str) -> f32 {
    let mut x = TITLE_BTN_PAD_X;
    for (i, menu) in menus.iter().enumerate() {
        if i >= idx {
            break;
        }
        x += TITLE_BTN_PAD_X * 2. + context_menu::estimate_width(menu.label) + TITLES_GAP;
    }
    x
}

// ─── 单测（纯逻辑——零 gpui 依赖，无窗口可验） ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 满配上下文（有文档/有选区/可写/浅色主题/左栏开文件树/有最近记录）。
    fn full_ctx() -> MenuContext {
        MenuContext {
            has_document: true,
            read_only: false,
            has_selection: true,
            font_size: 15.,
            theme_mode: THEME_LIGHT,
            left_open: true,
            git_view: false,
            active_is_docx: true,
            active_is_pdf: false,
            recent_files: vec![
                PathBuf::from("/tmp/m209-a.md"),
                PathBuf::from("/tmp/不存在-b.md"),
            ],
        }
    }

    /// 空态上下文（无文档/跟随系统/无最近记录）。
    fn empty_ctx() -> MenuContext {
        MenuContext {
            has_document: false,
            read_only: false,
            has_selection: false,
            font_size: 15.,
            theme_mode: THEME_SYSTEM,
            left_open: true,
            git_view: false,
            active_is_docx: false,
            active_is_pdf: false,
            recent_files: Vec::new(),
        }
    }

    /// 按 id 找条目（含子菜单——返回 (所在层路径, 规格)）。
    fn find<'a>(entries: &'a [MenuEntry], id: &str) -> Option<&'a MenuItemSpec> {
        for entry in entries {
            match entry {
                MenuEntry::Item(spec) if spec.id == id => return Some(spec),
                MenuEntry::Submenu { items, .. } => {
                    if let Some(found) = find(items, id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    #[test]
    fn four_menus_in_canonical_order() {
        let menus = build_menus(&empty_ctx());
        let labels: Vec<&str> = menus.iter().map(|m| m.label).collect();
        assert_eq!(
            labels,
            vec!["文件", "编辑", "查看", "帮助"],
            "四菜单按约定序"
        );
        assert!(menus.iter().all(|m| !m.entries.is_empty()), "菜单不空");
    }

    #[test]
    fn ids_unique_across_menus() {
        let menus = build_menus(&full_ctx());
        let mut seen: Vec<&'static str> = Vec::new();
        fn collect(entries: &[MenuEntry], seen: &mut Vec<&'static str>) {
            for entry in entries {
                match entry {
                    MenuEntry::Item(spec) => {
                        assert!(!seen.contains(&spec.id), "菜单项 id 重复：{}", spec.id);
                        seen.push(spec.id);
                    }
                    MenuEntry::Submenu { items, .. } => collect(items, seen),
                    MenuEntry::Separator => {}
                }
            }
        }
        for menu in &menus {
            collect(&menu.entries, &mut seen);
        }
    }

    /// 快捷键标签 ↔ 真实键位钉定表（editor dispatch_key / shell on_key_down
    /// 的既有分支——标签标了就必须真有这条键位，否则是可发现性欺骗）。
    #[test]
    fn shortcut_labels_pin_real_bindings() {
        let menus = build_menus(&full_ctx());
        let pinned: &[(&str, &str)] = &[
            ("file-open", "Ctrl+O"),
            ("file-open-folder", "Ctrl+K Ctrl+O"),
            ("file-save", "Ctrl+S"),
            ("file-save-as", "Ctrl+Shift+S"),
            ("edit-undo", "Ctrl+Z"),
            ("edit-redo", "Ctrl+Shift+Z"),
            ("edit-cut", "Ctrl+X"),
            ("edit-copy", "Ctrl+C"),
            ("edit-paste", "Ctrl+V"),
            ("edit-find", "Ctrl+F"),
            ("edit-replace", "Ctrl+H"),
            ("edit-palette", "Ctrl+Shift+P"),
            ("view-font-inc", "Ctrl+="),
            ("view-font-dec", "Ctrl+-"),
            ("view-font-reset", "Ctrl+0"),
            ("view-side-search", "Ctrl+F"),
        ];
        for (id, key) in pinned {
            let spec = find(&menus[0].entries, id)
                .or_else(|| find(&menus[1].entries, id))
                .or_else(|| find(&menus[2].entries, id))
                .unwrap_or_else(|| panic!("{id} 缺失"));
            assert_eq!(spec.shortcut.as_deref(), Some(*key), "{id} 快捷键标签漂移");
        }
        // 无键位的项不标（新建/退出/主题/侧栏切换等——不存在「标了但按了没反应」）。
        for id in [
            "file-new",
            "file-quit",
            "help-about",
            "view-status-bar",
            "view-fullscreen",
        ] {
            let spec = find(&menus[0].entries, id)
                .or_else(|| find(&menus[2].entries, id))
                .or_else(|| find(&menus[3].entries, id))
                .unwrap_or_else(|| panic!("{id} 缺失"));
            assert_eq!(spec.shortcut, None, "{id} 不应有快捷键标签（无真实键位）");
        }
    }

    /// EditorCmd 映射的对应关系（菜单项 → Cmd 变体——层 2 对接的核心表）。
    #[test]
    fn editor_cmd_mappings() {
        let menus = build_menus(&full_ctx());
        let cases: &[(&str, Cmd)] = &[
            ("file-save", Cmd::Save),
            ("edit-undo", Cmd::Undo),
            ("edit-redo", Cmd::Redo),
            ("edit-find", Cmd::OpenFind),
            ("edit-replace", Cmd::OpenReplace),
            ("view-side-search", Cmd::OpenFind),
        ];
        for (id, want) in cases {
            let spec = find(&menus[0].entries, id)
                .or_else(|| find(&menus[1].entries, id))
                .or_else(|| find(&menus[2].entries, id))
                .unwrap_or_else(|| panic!("{id} 缺失"));
            assert_eq!(
                spec.action,
                MenuAction::EditorCmd(*want),
                "{id} Cmd 映射漂移"
            );
        }
    }

    /// 无既有命令的项：置灰 + 不绑动作（禁用态是验收项）。
    #[test]
    fn placeholder_items_disabled_with_no_action() {
        let menus = build_menus(&full_ctx());
        for id in [
            "file-new",
            "view-status-bar",
            "view-fullscreen",
            "help-shortcuts",
            "help-docs",
        ] {
            let spec = find(&menus[0].entries, id)
                .or_else(|| find(&menus[2].entries, id))
                .or_else(|| find(&menus[3].entries, id))
                .unwrap_or_else(|| panic!("{id} 缺失"));
            assert!(!spec.enabled, "{id} 应置灰（无既有命令）");
            assert_eq!(spec.action, MenuAction::None, "{id} 置灰项不得绑动作");
        }
    }

    /// 空态（无文档）：需要文档的项灰显，shell 级动作保持可用。
    #[test]
    fn no_document_disables_editor_actions_only() {
        let menus = build_menus(&empty_ctx());
        let disabled = [
            "file-save",
            "file-save-as",
            "edit-undo",
            "edit-redo",
            "edit-cut",
            "edit-copy",
            "edit-paste",
            "edit-find",
            "edit-replace",
            "edit-palette",
            "view-side-search",
        ];
        for id in disabled {
            let spec = find(&menus[0].entries, id)
                .or_else(|| find(&menus[1].entries, id))
                .or_else(|| find(&menus[2].entries, id))
                .unwrap_or_else(|| panic!("{id} 缺失"));
            assert!(!spec.enabled, "无文档时 {id} 应灰显");
        }
        // shell 级动作不依赖文档。
        let enabled = ["file-open", "file-open-folder", "file-quit", "help-about"];
        for id in enabled {
            let spec = find(&menus[0].entries, id)
                .or_else(|| find(&menus[3].entries, id))
                .unwrap_or_else(|| panic!("{id} 缺失"));
            assert!(spec.enabled, "无文档时 {id} 仍应可用（shell 级动作）");
        }
    }

    /// 只读态（docx/pdf）：变更类灰显，浏览类（复制/查找）可用——与编辑器
    /// 右键菜单同口径（readonly_still_allows_copy_and_find 的镜像）。
    #[test]
    fn read_only_disables_writes_allows_browsing() {
        let mut ctx = full_ctx();
        ctx.read_only = true;
        let menus = build_menus(&ctx);
        for id in [
            "edit-undo",
            "edit-redo",
            "edit-cut",
            "edit-paste",
            "edit-replace",
        ] {
            let spec = find(&menus[1].entries, id).unwrap_or_else(|| panic!("{id} 缺失"));
            assert!(!spec.enabled, "只读态 {id} 应灰显");
        }
        for id in ["edit-copy", "edit-find"] {
            let spec = find(&menus[1].entries, id).unwrap_or_else(|| panic!("{id} 缺失"));
            assert!(spec.enabled, "只读态 {id} 仍应可用");
        }
    }

    /// 无选区：剪切/复制灰显（粘贴不受选区影响——右键菜单同判定源）。
    #[test]
    fn no_selection_disables_cut_copy() {
        let mut ctx = full_ctx();
        ctx.has_selection = false;
        let menus = build_menus(&ctx);
        for id in ["edit-cut", "edit-copy"] {
            let spec = find(&menus[1].entries, id).unwrap_or_else(|| panic!("{id} 缺失"));
            assert!(!spec.enabled, "无选区 {id} 应灰显");
        }
        let paste = find(&menus[1].entries, "edit-paste").unwrap();
        assert!(paste.enabled, "粘贴不受选区影响");
    }

    /// 主题子菜单：任一模式下恰好一项勾选（三选一 radio 语义）。
    #[test]
    fn theme_submenu_checks_exactly_one() {
        for mode in [THEME_SYSTEM, THEME_LIGHT, THEME_DARK] {
            let mut ctx = full_ctx();
            ctx.theme_mode = mode;
            let menus = build_menus(&ctx);
            let ids = ["view-theme-system", "view-theme-light", "view-theme-dark"];
            let checked: Vec<bool> = ids
                .iter()
                .map(|id| {
                    find(&menus[2].entries, id)
                        .unwrap_or_else(|| panic!("{id} 缺失"))
                        .checked
                })
                .collect();
            assert_eq!(
                checked.iter().filter(|c| **c).count(),
                1,
                "模式 {mode} 应恰好一项勾选：{checked:?}"
            );
        }
    }

    /// 主题子菜单动作值 = 持久化契约串（与 theme_switch::MODE_* 同值）。
    #[test]
    fn theme_constants_pin_contract() {
        assert_eq!(THEME_SYSTEM, "system");
        assert_eq!(THEME_LIGHT, "light");
        assert_eq!(THEME_DARK, "dark");
        let menus = build_menus(&full_ctx());
        assert_eq!(
            find(&menus[2].entries, "view-theme-system").unwrap().action,
            MenuAction::SetTheme("system")
        );
        assert_eq!(
            find(&menus[2].entries, "view-theme-light").unwrap().action,
            MenuAction::SetTheme("light")
        );
        assert_eq!(
            find(&menus[2].entries, "view-theme-dark").unwrap().action,
            MenuAction::SetTheme("dark")
        );
    }

    /// 侧栏子菜单勾选：文件树/git 互斥，跟随 left_open/git_view。
    #[test]
    fn sidebar_submenu_checks_panel_state() {
        for (left, git, filetree_checked, git_checked) in [
            (true, false, true, false),
            (true, true, false, true),
            (false, false, false, false),
        ] {
            let mut ctx = full_ctx();
            ctx.left_open = left;
            ctx.git_view = git;
            let menus = build_menus(&ctx);
            assert_eq!(
                find(&menus[2].entries, "view-side-file-tree")
                    .unwrap()
                    .checked,
                filetree_checked
            );
            assert_eq!(
                find(&menus[2].entries, "view-side-git").unwrap().checked,
                git_checked
            );
        }
    }

    /// 字号子菜单域界：8 下限不能再减、32 上限不能再增（设置面板步进钮
    /// 同口径）。
    #[test]
    fn font_submenu_bounds() {
        let mut ctx = full_ctx();
        ctx.font_size = 32.;
        let menus = build_menus(&ctx);
        assert!(
            !find(&menus[2].entries, "view-font-inc").unwrap().enabled,
            "32 上限增大应灰显"
        );
        assert!(find(&menus[2].entries, "view-font-dec").unwrap().enabled);
        ctx.font_size = 8.;
        let menus = build_menus(&ctx);
        assert!(
            !find(&menus[2].entries, "view-font-dec").unwrap().enabled,
            "8 下限减小应灰显"
        );
        assert!(find(&menus[2].entries, "view-font-inc").unwrap().enabled);
    }

    /// 最近打开子菜单：序号动作映射、文件名标签、缺失文件置灰、空态占位。
    #[test]
    fn recent_submenu_semantics() {
        // 存在性判定走真文件（temp 目录落一个临时件——exists() 是
        // build_menus 的灰显依据，不 mock）。
        let live = std::env::temp_dir().join(format!("m209-live-{}.md", std::process::id()));
        std::fs::write(&live, "").expect("临时夹具落盘");
        let mut ctx = full_ctx();
        ctx.recent_files = vec![live.clone(), PathBuf::from("/tmp/m209-不存在-b.md")];
        let menus = build_menus(&ctx);
        let first = find(&menus[0].entries, "file-recent-0").unwrap();
        assert_eq!(first.action, MenuAction::OpenRecent(0));
        assert!(
            first.label.ends_with(".md"),
            "标签取文件名：{}",
            first.label
        );
        assert!(first.enabled, "存在的文件可点");
        let missing = find(&menus[0].entries, "file-recent-1").unwrap();
        assert!(!missing.enabled, "已删除的最近文件置灰（空态引导页同口径）");
        std::fs::remove_file(&live).ok();

        let empty = build_menus(&empty_ctx());
        let placeholder = find(&empty[0].entries, "file-recent-empty").unwrap();
        assert!(!placeholder.enabled, "空记录占位置灰");
    }

    /// 最近打开超池截断（>8 条只取前 8——id 池只有 8 位）。
    #[test]
    fn recent_submenu_caps_at_pool_size() {
        let mut ctx = full_ctx();
        ctx.recent_files = (0..12)
            .map(|i| PathBuf::from(format!("/tmp/m209-{i}.md")))
            .collect();
        let menus = build_menus(&ctx);
        assert!(
            find(&menus[0].entries, "file-recent-7").is_some(),
            "第 8 条在池内"
        );
        assert!(
            find(&menus[0].entries, "file-recent-8").is_none(),
            "第 9 条截断（无池位）"
        );
    }

    // ── 导航状态机 ──────────────────────────────────────────────

    /// 打开菜单：选中落在首个可停靠条目（文件菜单首项「新建」虽置灰但
    /// 可停靠——桌面惯例：禁用项可高亮）。
    #[test]
    fn open_menu_selects_first_navigable() {
        let menus = build_menus(&empty_ctx());
        let mut state = MenuBarState::default();
        state.open_menu(0, 100., &menus);
        assert_eq!(state.open, Some(0));
        assert_eq!(state.selected, 0, "文件菜单首项（新建）可停靠");
        state.open_menu(2, 100., &menus);
        assert_eq!(state.selected, 0, "查看菜单首项（主题子菜单）");
    }

    /// ↑↓ 循环跳过分隔线且 wrap。
    #[test]
    fn vertical_moves_skip_separators_and_wrap() {
        let menus = build_menus(&full_ctx());
        let mut state = MenuBarState::default();
        state.open_menu(0, 0., &menus);
        // 文件菜单条目：新建0 打开1 打开文件夹2 保存3 另存为4 导出PDF5 导出DOCX6 |sep7| 最近打开8 |sep9| 退出10
        assert_eq!(state.selected, 0);
        state.move_vertical(&menus, true);
        assert_eq!(state.selected, 1);
        // 步进到子菜单标题（8）再下一步是 10（跳过 9 分隔线）。
        state.move_vertical(&menus, true);
        state.move_vertical(&menus, true);
        state.move_vertical(&menus, true);
        state.move_vertical(&menus, true);
        state.move_vertical(&menus, true);
        assert_eq!(state.selected, 8, "停在「最近打开」");
        state.move_vertical(&menus, true);
        assert_eq!(state.selected, 10, "跳过分隔线 9 到「退出」");
        state.move_vertical(&menus, true);
        assert_eq!(state.selected, 0, "wrap 回首项");
        state.move_vertical(&menus, false);
        assert_eq!(state.selected, 10, "反向 wrap 回末项（退出）");
    }

    /// Enter 在子菜单标题上：展开子菜单；↓ 移动后再次 Enter 激活子菜单
    /// 选中项并全关。
    #[test]
    fn enter_opens_submenu_then_activates_child() {
        let menus = build_menus(&full_ctx());
        let mut state = MenuBarState::default();
        state.open_menu(2, 0., &menus); // 查看菜单，选中 = 主题子菜单
        // → 进子菜单（选中 = 首项「跟随系统」）。
        assert!(state.open_submenu_at(&menus, state.selected));
        assert_eq!(state.submenu_selected, 0, "子菜单选中首项（跟随系统）");
        // ↓ 移到「浅色」，Enter 激活。
        state.move_vertical(&menus, true);
        assert_eq!(state.submenu_selected, 1);
        let action = navigate(&mut state, &menus, "enter", "mang");
        assert_eq!(action, Some(MenuAction::SetTheme(THEME_LIGHT)));
        assert!(!state.is_open(), "激活后全关");
    }

    /// ← 在子菜单层关子菜单；再 ← 切上一菜单；→ 在普通项上切下一菜单。
    #[test]
    fn left_closes_submenu_then_switches_menu() {
        let menus = build_menus(&full_ctx());
        let mut state = MenuBarState::default();
        state.open_menu(2, 100., &menus);
        state.open_submenu_at(&menus, 0);
        navigate(&mut state, &menus, "left", "mang");
        assert_eq!(state.open, Some(2), "← 先关子菜单，不切菜单");
        assert_eq!(state.open_submenu, None);
        navigate(&mut state, &menus, "left", "mang");
        assert_eq!(state.open, Some(1), "再 ← 切上一菜单（查看→编辑）");
        navigate(&mut state, &menus, "right", "mang");
        assert_eq!(state.open, Some(2), "→ 切回查看");
        // 锚点随菜单切换重锚（估算）。
        assert!(state.anchor_x > 0.);
    }

    /// Enter 在禁用项上：无操作、菜单不关（灰显项可高亮不可激活）。
    #[test]
    fn enter_on_disabled_item_is_noop() {
        let menus = build_menus(&empty_ctx());
        let mut state = MenuBarState::default();
        state.open_menu(0, 0., &menus);
        assert_eq!(state.selected, 0, "「新建」置灰但可停靠");
        assert_eq!(navigate(&mut state, &menus, "enter", "mang"), None);
        assert!(state.is_open(), "禁用项 Enter 不关菜单");
    }

    /// 键盘切菜单：选中重置到新菜单首个可停靠条目、wrap、子菜单收起。
    #[test]
    fn switch_menu_resets_selection_and_wraps() {
        let menus = build_menus(&full_ctx());
        let mut state = MenuBarState::default();
        state.open_menu(3, 50., &menus); // 帮助
        state.selected = 2;
        state.switch_menu(&menus, true, "mang");
        assert_eq!(state.open, Some(0), "末位 wrap 回文件");
        assert_eq!(state.selected, 0, "选中重置到首项");
        state.open_submenu = Some(0); // 模拟脏状态
        state.switch_menu(&menus, true, "mang");
        assert_eq!(state.open_submenu, None, "切菜单收起子菜单");
    }

    /// 结构漂移防御：选中/子菜单序越界时导航自愈，不 panic。
    #[test]
    fn navigation_survives_structure_drift() {
        let menus = build_menus(&full_ctx());
        let mut state = MenuBarState {
            open: Some(0),
            selected: 99,
            open_submenu: Some(99),
            submenu_selected: 99,
            anchor_x: 0.,
        };
        // ↑：不 panic；子菜单序越界 → 退回顶层导航，选中落到某个可停靠项。
        state.move_vertical(&menus, true);
        assert!(state.selected < menus[0].entries.len());
        assert_eq!(state.open_submenu, None, "越界子菜单序自愈清空");
        // Enter：与 focused_item 的投射一致——钉在普通条目上测（自愈
        // 落点随菜单结构漂移：「最近打开」子菜单标题的 Enter 语义是
        // 展开子菜单而非关闭，不作为本断言的载体）。
        state.selected = 1;
        let expect = state
            .focused_item(&menus)
            .filter(|s| s.enabled)
            .map(|s| s.action);
        assert_eq!(navigate(&mut state, &menus, "enter", "mang"), expect);
        assert!(!state.is_open());
    }

    /// 空菜单集防御（理论上不可达——build_menus 恒四菜单）。
    #[test]
    fn navigation_on_empty_menus_is_noop() {
        let mut state = MenuBarState {
            open: Some(0),
            ..Default::default()
        };
        let empty: Vec<TopMenu> = Vec::new();
        assert!(!state.move_vertical(&empty, true));
        assert_eq!(navigate(&mut state, &empty, "down", "mang"), None);
        // ← 触发 switch_menu → 空集全关自愈。
        assert_eq!(navigate(&mut state, &empty, "left", "mang"), None);
        assert!(!state.is_open(), "switch_menu 对空集全关自愈");
    }

    // ── 几何 ────────────────────────────────────────────────────

    /// 转换保宽度估算输入（标签 + 快捷键 + 灰显透传；子菜单按标题估宽）。
    #[test]
    fn to_context_items_feeds_menu_size() {
        let menus = build_menus(&full_ctx());
        let items = to_context_items(&menus[0].entries);
        let (w, h) = context_menu::menu_size(&items);
        assert!(w >= context_menu::MENU_MIN_WIDTH, "宽度不小于最小宽");
        let expected_h = context_menu::MENU_PADDING_Y * 2.
            + menus[0]
                .entries
                .iter()
                .map(|e| context_menu::ITEM_HEIGHT * e.height_units())
                .sum::<f32>();
        assert_eq!(h, expected_h, "高度 = 内边距×2 + Σ(行高×占位)");
        // 全条目数（含子菜单标题）都被计入高度——子菜单标题占一行。
        assert!(items.len() == menus[0].entries.len());
    }

    /// 行偏移：首行 = 上内边距；分隔线按占位高度累加。
    #[test]
    fn row_y_offset_accounts_separators() {
        let menus = build_menus(&full_ctx());
        let entries = &menus[0].entries;
        assert_eq!(row_y_offset(entries, 0), context_menu::MENU_PADDING_Y);
        // idx 7（最近打开）之前有六项普通条目（0..5）+ 一条分隔线（6）。
        let expect = context_menu::MENU_PADDING_Y
            + context_menu::ITEM_HEIGHT * 6.
            + context_menu::ITEM_HEIGHT * context_menu::SEPARATOR_UNITS;
        assert_eq!(row_y_offset(entries, 7), expect);
    }

    /// 锚点估算随菜单序单调递增（下拉往右跟着标题走）。
    #[test]
    fn anchor_estimate_monotonic() {
        let menus = build_menus(&full_ctx());
        let mut prev = -1.;
        for idx in 0..menus.len() {
            let x = estimate_anchor_x(&menus, idx, "mang — 某文档.md");
            assert!(x > prev, "锚点应随菜单序递增：{prev} → {x}");
            prev = x;
        }
        // #289-②：菜单栏贴窗口左缘——标题不再参与锚点估算（旧版「标题
        // 更长 → 锚点更右」已随左侧标题列废弃）。
        assert_eq!(
            estimate_anchor_x(&menus, 0, "mang — 很长很长的文档名.md"),
            estimate_anchor_x(&menus, 0, "mang")
        );
        // 首个菜单锚点 = 自身左内边距（窗口左缘起算）。
        assert_eq!(estimate_anchor_x(&menus, 0, ""), TITLE_BTN_PAD_X);
    }
}
