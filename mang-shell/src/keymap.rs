//! 键位表（#217 keymap 可配置——#68 阶段 E 收口）。
//!
//! 抄 Zed `keymap` crate 的**设计**（上游在 zed crate 深耦合面，不搬；
//! 与 commands.rs / menu_bar.rs 同款纪律自研干净实现）：数据与执行分离
//! ——「换绑 = 改表」。本模块是纯逻辑层：**零 gpui、零 serde 依赖**
//! （纯 std），可脱离窗口单测（harness `#[path]` 直挂真实源文件）。
//!
//! # 分层
//!
//! - **数据**：[`KeyPattern`]（键 + 三态修饰）/ [`Binding`]（单键或
//!   二键 chord）/ [`EditorAction`]·[`ShellAction`]（两域动作枚举——
//!   **只映射既有执行体，不写新逻辑**，与 #209 菜单栏同纪律）；
//! - **默认表**：[`KeymapStore::default_tables`] = 现状键位的**精确镜像**
//!   （editor `dispatch_key_inner` + shell `on_key_down` 两处匹配臂全部
//!   入表，含 chord 与前置分支——逐臂对照清单见下）；
//! - **匹配引擎**：[`KeymapStore::resolve_pre`] /
//!   [`KeymapStore::resolve_editor_main`] / [`KeymapStore::resolve_shell`]
//!   （chord 挂起态 = **首键实际按键快照**——续接匹配 = 首键能触发的
//!   chord 中尾键匹配当前键，尾键精确度/表序取舍；挂起/续接/作废
//!   语义与现状 `ctrl_k_pending` / `chord_until` 状态机一致）；
//! - **用户覆盖**：settings.json `keymap` 键 → [`UserOverride`] →
//!   [`KeymapStore::apply_user`] 合并（换绑 = 改表：旧键让位 + 域内
//!   默认条目整体接管；同域 canonical 赢家检测 + 跨域结构相交
//!   拒绝；解绑 = 移除该键实际触发的赢家——策略见 [`KeymapStore::apply_user`]
//!   文档）；
//! - **全局装配**：[`store()`]（未 init = 默认表——单测/harness 默认
//!   面）/ [`init()`]（bin 启动时装载用户覆盖）。
//!
//! # 匹配语义（与现状逐臂等价的根据）
//!
//! 现状是 Rust `match` 臂序 + 每臂独立守卫；守卫对三个修饰键的检查
//! 不统一（`"a" if ctrl` 不看 shift/alt、`"k" if ctrl && !shift` 不看
//! alt、shell `plain_ctrl` 三查）。因此修饰模型必须是**三态**而非
//! 「精确集合相等」：
//!
//! - [`Mod::Any`]：该修饰键按下与否都匹配（对应臂守卫未提及该修饰）；
//! - [`Mod::Yes`]：必须按下；[`Mod::No`]：必须未按下。
//!
//! 同一查询命中多条目时的取舍（现状臂序的抽象）：**约束数（Yes/No
//! 总数）多者优先，同数按表序**（表序 = 原 match 臂序）。默认表下
//! 每个物理按键至多一条目胜出，与单臂执行等价；用户覆盖引入并存时
//! （如更精确的 `alt+left` 叠在方向键家族的 `Any` 条目上），精确者胜。
//!
//! # chord（和弦）语义
//!
//! 现状两处状态机：
//!
//! - **编辑器** `ctrl_k_pending: bool`（无超时）：Ctrl+K 只置标志不
//!   吞内容；下一键 0/J 完成组合（守卫**只看 key 不看修饰**——
//!   Ctrl+K Ctrl+0 同样触发全部折叠，现状怪癖照抄）；其余键**取消
//!   前缀并照常下沉**（不吞键）；
//! - **shell** `chord_until: Option<Instant>`（`CHORD_WINDOW` 2s
//!   窗口）：窗口期内 Ctrl+O → 打开工作区；过期/非 O → 和弦作废、
//!   键继续走后续分支。
//!
//! 引擎把两者统一为「挂起首键快照」：编辑器挂起无期限、shell 挂起带
//! deadline（过期视作无挂起，作废不吞键）。续接匹配 = 首键快照能
//! 触发的全部 chord 中找尾键匹配者（尾键精确度/表序取舍——用户
//! 和弦首键 Any 变体与默认首键同可达）；续接落空则挂起终结、本键
//! 当新序列起点（对齐现状「取消前缀并照常下沉」）。
//!
//! # 逐臂对照清单（默认表 ↔ 现状代码——行为零变化红线）
//!
//! ## 编辑器域 · 前置层（守卫序列中的查表点）
//!
//! | 条目 | 键位（pattern） | 动作 | 现状位置（editor/mod.rs） |
//! |---|---|---|---|
//! | E0 | ctrl+shift+p（alt Any） | [`EditorAction::PaletteToggle`] | `dispatch_key_inner` Ctrl+Shift+P 前置分支（palette 模态之前） |
//! | E1 | ctrl+g（!shift，alt Any） | [`EditorAction::GotoOpen`] | Ctrl+G 前置分支（goto 模态之前；`goto.is_none()` 条件保留在执行层） |
//!
//! ## 编辑器域 · 主层（match 主体，表序 = 臂序）
//!
//! | 条目 | 键位（pattern） | 动作 | 现状臂 |
//! |---|---|---|---|
//! | E2 | chord [ctrl+k(!shift), 0(Any)] | [`EditorAction::FoldAll`] | `"0" if self.ctrl_k_pending` |
//! | E3 | chord [ctrl+k(!shift), j(Any)] | [`EditorAction::UnfoldAll`] | `"j" if self.ctrl_k_pending` |
//! | E4 | left(Any) | [`EditorAction::MoveKey`]（Left） | `"left"`（ctrl/shift 参数化家族） |
//! | E5 | right(Any) | MoveKey（Right） | `"right"` |
//! | E6 | up(Any) | MoveKey（Up） | `"up"` |
//! | E7 | down(Any) | MoveKey（Down） | `"down"` |
//! | E8 | home(Any) | [`EditorAction::HomeKey`] | `"home"`（ctrl 分流文首在执行层） |
//! | E9 | end(Any) | [`EditorAction::EndKey`] | `"end"`（同上） |
//! | E10 | pageup(Any) | [`EditorAction::PageUpKey`] | `"pageup"` |
//! | E11 | pagedown(Any) | [`EditorAction::PageDownKey`] | `"pagedown"` |
//! | E12 | enter(Any) | [`EditorAction::Newline`] | `"enter"` |
//! | E13 | backspace(Any) | [`EditorAction::Backspace`] | `"backspace"` |
//! | E14 | delete(Any) | [`EditorAction::DeleteForward`] | `"delete"` |
//! | E15 | tab(!ctrl) | [`EditorAction::Indent`] | `"tab" if !ctrl`（Ctrl+Tab 冒泡 shell） |
//! | E16 | escape(Any) | [`EditorAction::EscapeClearSelection`] | `"escape"` |
//! | E17 | a(ctrl) | [`EditorAction::SelectAll`] | `"a" if ctrl` |
//! | E18 | c(ctrl) | [`EditorAction::Copy`] | `"c" if ctrl` |
//! | E19 | x(ctrl) | [`EditorAction::Cut`] | `"x" if ctrl` |
//! | E20 | v(ctrl) | [`EditorAction::Paste`] | `"v" if ctrl` |
//! | E21 | z(ctrl+shift) | [`EditorAction::Redo`] | `"z" if ctrl && shift` |
//! | E22 | z(ctrl) | [`EditorAction::Undo`] | `"z" if ctrl` |
//! | E23 | y(ctrl) | [`EditorAction::RedoY`] | `"y" if ctrl`（redo 第二键位） |
//! | E24 | d(ctrl) | [`EditorAction::AddNextWordMatch`] | `"d" if ctrl` |
//! | E25 | f(alt) | [`EditorAction::ToggleFold`] | `"f" if ks.modifiers.alt` |
//! | E26 | [(ctrl+shift) | [`EditorAction::FoldCurrent`] | `"[" if ctrl && shift` |
//! | E27 | ](ctrl+shift) | [`EditorAction::UnfoldCurrent`] | `"]" if ctrl && shift` |
//! | E28 | f(ctrl) | [`EditorAction::OpenFind`] | `"f" if ctrl` |
//! | E29 | h(ctrl) | [`EditorAction::OpenReplace`] | `"h" if ctrl` |
//! | E30 | s(ctrl，shift Any) | [`EditorAction::Pass`] | `"s" if ctrl => {}`（空臂——编辑器不消费，冒泡 shell；#386 起 Ctrl+S 亦冒泡至 ShellAction::Save，Ctrl+Shift+S 至另存为） |
//! | E32 | chord 首键 ctrl+k(!shift) | 挂起（引擎 [`EditorOutcome::Pending`]） | `"k" if ctrl && !shift`（提前 return 不落尾部） |
//!
//! ## shell 域（on_key_down 模态守卫后，表序 = 分支序）
//!
//! | 条目 | 键位（pattern） | 动作 | 现状分支（shell.rs） |
//! |---|---|---|---|
//! | S1 | chord [ctrl+k(plain), ctrl+o(plain)] | [`ShellAction::PromptOpenWorkspace`] | 和弦第二键分支 |
//! | S2 | tab(ctrl，!alt，!shift) | [`ShellAction::NextTab`] | Ctrl+Tab |
//! | S3 | tab(ctrl，!alt，shift) | [`ShellAction::PrevTab`] | Ctrl+Shift+Tab |
//! | S4 | w(plain) | [`ShellAction::CloseTab`] | Ctrl+W |
//! | S5 | s(ctrl+shift，!alt) | [`ShellAction::PromptSaveAs`] | Ctrl+Shift+S（编辑器 E30 放行冒泡至此） |
//! | S12 | s(ctrl，!shift，!alt) | [`ShellAction::Save`] | #386 Ctrl+S 保存分流（pdf 批注增量落盘/编辑器三态；编辑器 E30 旧默认键位移除冒泡至此） |
//! | S6 | =(ctrl，!alt，shift Any) | [`ShellAction::FontBigger`] | Ctrl+= |
//! | S7 | +(ctrl，!alt，shift Any) | [`ShellAction::FontBigger`] | Ctrl++（同分支双键位） |
//! | S8 | -(ctrl，!alt，shift Any) | [`ShellAction::FontSmaller`] | Ctrl+- |
//! | S9 | 0(plain) | [`ShellAction::FontReset`] | Ctrl+0 |
//! | S10 | t(plain) | [`ShellAction::CycleTheme`] | Ctrl+T |
//! | S11 | o(plain) | [`ShellAction::PromptOpenFile`] | Ctrl+O |
//!
//! （plain = ctrl 按下且 shift/alt 未按下——对应 shell `plain_ctrl`。）
//!
//! **不入表的面**（守卫/模态，保留原位原序）：canvas 键域、命令面板
//! /跳行框/查找条的**模态内**键处理、菜单栏/确认框/右键菜单模态拦截、
//! 只读门禁（`docx_doc::readonly_allows`——键白名单在动作执行层
//! 之前，改绑后的键按该键的白名单状态判定）、`_` 分支可打印字符
//! 插入（引擎无命中时走原逻辑）。
//!
//! # 用户覆盖格式（settings.json `keymap` 键）
//!
//! ```json
//! {
//!   "keymap": [
//!     { "bind": "ctrl+alt+p", "action": "toggle_command_palette" },
//!     { "unbind": "ctrl+shift+p" }
//!   ]
//! }
//! ```
//!
//! - 键串：`ctrl+k ctrl+o`（空格分 chord，二键为限）；修饰词
//!   `ctrl`/`shift`/`alt`（大小写不敏感，可组合；meta/cmd 等其余
//!   修饰词**不支持**——解析即报错，防幻影键静默度掉换绑，#218
//!   review）；`ctrl` 在运行时归一为 control 或 platform（macOS ⌘
//!   ——与现状 `ctrl` 变量同义）；键名小写（`=`/`+`/`-`/`[`/字母/
//!   `home`…）；
//! - `action`：[`Action::id`] 机器名（`prompt_open_file`/
//!   `toggle_command_palette`/…——`from_id` 反查全集见单测）；
//! - 合并与冲突策略：见 [`KeymapStore::apply_user`]。

use std::time::Instant;

// ============================================================================
// 数据模型
// ============================================================================

/// 修饰键三态匹配（对应现状各臂守卫对修饰键的不同检查口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mod {
    /// 按下与否都匹配（臂守卫未提及该修饰）。
    Any,
    /// 必须按下。
    Yes,
    /// 必须未按下。
    No,
}

impl Mod {
    /// 查询值是否满足本约束。
    fn satisfies(self, on: bool) -> bool {
        match self {
            Mod::Any => true,
            Mod::Yes => on,
            Mod::No => !on,
        }
    }

    /// 约束强度（Any=0，Yes/No=1——匹配排序的精确度权重）。
    fn weight(self) -> usize {
        match self {
            Mod::Any => 0,
            Mod::Yes | Mod::No => 1,
        }
    }
}

/// 键位 pattern：键名（小写规范化——对齐现状 `ks.key.to_lowercase()`）
/// + 三个修饰键的三态约束。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPattern {
    pub key: String,
    pub ctrl: Mod,
    pub shift: Mod,
    pub alt: Mod,
}

impl KeyPattern {
    /// 构造（修饰全 Any 的裸键）。
    pub fn key(key: &str) -> Self {
        KeyPattern {
            key: key.to_lowercase(),
            ctrl: Mod::Any,
            shift: Mod::Any,
            alt: Mod::Any,
        }
    }

    /// 叠加修饰约束（builder 风格——默认表构造与测试用）。
    pub fn with(mut self, ctrl: Mod, shift: Mod, alt: Mod) -> Self {
        self.ctrl = ctrl;
        self.shift = shift;
        self.alt = alt;
        self
    }

    /// 查询是否匹配本 pattern。
    fn matches(&self, q: &KeyQuery) -> bool {
        self.key == q.key
            && self.ctrl.satisfies(q.ctrl)
            && self.shift.satisfies(q.shift)
            && self.alt.satisfies(q.alt)
    }

    /// 精确度（修饰约束总数——匹配排序权重；冲突检测的豁免依据）。
    fn specificity(&self) -> usize {
        self.ctrl.weight() + self.shift.weight() + self.alt.weight()
    }

    /// 两个 pattern 是否存在共同按键（同键且每维约束兼容——
    /// Yes/No 对撞即不相交）。
    fn intersects(&self, other: &KeyPattern) -> bool {
        self.key == other.key
            && dim_compatible(self.ctrl, other.ctrl)
            && dim_compatible(self.shift, other.shift)
            && dim_compatible(self.alt, other.alt)
    }

    /// 本 pattern 的匹配域是否被 `user` 的匹配域覆盖（本条目的一切
    /// 按键组合都在用户键串的覆盖内——bind 替换面的判定：用户说
    /// `ctrl+k`（shift Any），覆盖默认表 `ctrl+k(!shift)` 的域；反之
    /// `alt+f` 的域含无 ctrl 变体，不被 `ctrl+f` 覆盖——不会误伤）。
    fn covered_by(&self, user: &KeyPattern) -> bool {
        self.key == user.key
            && dim_covered(self.ctrl, user.ctrl)
            && dim_covered(self.shift, user.shift)
            && dim_covered(self.alt, user.alt)
    }
}

/// 同维两约束是否兼容（都不为对撞的 Yes/No）。
fn dim_compatible(a: Mod, b: Mod) -> bool {
    !matches!((a, b), (Mod::Yes, Mod::No) | (Mod::No, Mod::Yes))
}

/// 域包含判定的一维：`self_mod` 的匹配域 ⊆ `user_mod` 的匹配域
/// （`user` 为 Any 覆盖一切；两侧同为 Yes / 同为 No 相等包含；
/// `self` 为 Any 而 `user` 收窄 = self 域含 user 之外的按键 = 不包含）。
fn dim_covered(a: Mod, b: Mod) -> bool {
    b == Mod::Any || a == b
}

/// 绑定：单键或二键 chord（长度固定 2——现状 chord 先例 Ctrl+K
/// Ctrl+O；更长的序列无既有语义，不做）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    Single(KeyPattern),
    Chord(KeyPattern, KeyPattern),
}

impl Binding {
    /// 首键 pattern（Single 即自身；Chord 为第一键——挂起态与
    /// 单键竞争判定的比较面）。
    fn head(&self) -> &KeyPattern {
        match self {
            Binding::Single(p) => p,
            Binding::Chord(p, _) => p,
        }
    }

    /// 与另一绑定是否冲突（存在歧义按键序列）。
    ///
    /// - 单键 × 单键：pattern 相交（运行时同键多候选——精确度/表序
    ///   取舍或 canonical 检测解决）；
    /// - chord × 单键：单键与 chord **首键**相交（挂起语义歧义：
    ///   该键既是某动作单键又是某 chord 前缀——保守拒绝）；
    /// - chord × chord：首键相交**且**续接键相交才冲突（同前缀不同
    ///   续接可并存——挂起后按续接分流，无歧义；跨域首键相交而续接
    ///   相交 = Ctrl+K Ctrl+X 两域叠加执行，冲突）。
    fn conflicts(&self, other: &Binding) -> bool {
        match (self, other) {
            (Binding::Single(a), Binding::Single(b)) => a.intersects(b),
            (Binding::Chord(a1, a2), Binding::Chord(b1, b2)) => {
                a1.intersects(b1) && a2.intersects(b2)
            }
            (Binding::Chord(a1, _), Binding::Single(b))
            | (Binding::Single(b), Binding::Chord(a1, _)) => a1.intersects(b),
        }
    }

    /// 本绑定的匹配域是否被 `user` 覆盖（unbind / bind 替换面的判定：
    /// 用户键串覆盖的既有条目整体让位）。同形态比较（Single↔Single /
    /// Chord↔Chord 两键分别覆盖）；跨形态 false（chord 与单键的并存
    /// 由 conflicts 拒绝，不悄悄替换）。
    fn covered_by(&self, user: &Binding) -> bool {
        match (self, user) {
            (Binding::Single(a), Binding::Single(b)) => a.covered_by(b),
            (Binding::Chord(a1, a2), Binding::Chord(b1, b2)) => {
                a1.covered_by(b1) && a2.covered_by(b2)
            }
            _ => false,
        }
    }
}

/// 按键查询（接线层从 GPUI `KeyDownEvent` 构造一次，各查表点共用）。
///
/// - `ctrl` 已归一：`modifiers.control || modifiers.platform`（macOS ⌘
///   与 Linux/Windows Ctrl 同键位——现状 `ctrl` 变量口径）；
/// - `key` 已小写规范化（现状 `key.to_lowercase()` 口径）。
#[derive(Clone, Debug)]
pub struct KeyQuery {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

// ============================================================================
// 动作模型（只映射既有执行体——接线层 exec_* 按臂体 1:1 搬运）
// ============================================================================

/// 方向键家族的移动目标（`do_move` 的 dir 参数——ctrl 词跳/shift
/// 选区在**执行层**参数化，与现状家族臂结构一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveDir {
    Left,
    Right,
    Up,
    Down,
}

/// 编辑器域动作（`Editor::exec_editor_action` 的分派目标）。
///
/// 命名纪律：`id()` 与 `commands::Cmd::id()` 重叠的项**同串**
/// （save/open_find/undo/…——菜单栏 `EditorCmd(cmd)` 标签直接用
/// `cmd.id()` 查表）；Cmd 有而本枚举无的项（WordLeft/DocStart 等）
/// 是家族臂的**执行层参数形态**，不是独立键位，不设变体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// 命令面板开/关（原 Ctrl+Shift+P 前置分支）。
    PaletteToggle,
    /// 跳行框开启（原 Ctrl+G 前置分支；`goto.is_none()` 条件在执行层）。
    GotoOpen,
    /// 方向键移动家族（left/right/up/down 键，修饰参数化）。
    MoveKey(MoveDir),
    /// home 键家族（ctrl 分流文首在执行层）。
    HomeKey,
    /// end 键家族（同上）。
    EndKey,
    /// PageUp 翻页（`PAGE_LINES` 步进在执行层）。
    PageUpKey,
    /// PageDown 翻页。
    PageDownKey,
    /// 回车换行。
    Newline,
    /// 退格。
    Backspace,
    /// Delete 前删。
    DeleteForward,
    /// Tab 缩进 / Shift+Tab 反缩进（!ctrl 守卫——Ctrl+Tab 冒泡 shell）。
    Indent,
    /// Esc 收拢多光标 / 清选区。
    EscapeClearSelection,
    /// Ctrl+A 全选。
    SelectAll,
    /// Ctrl+C 复制。
    Copy,
    /// Ctrl+X 剪切。
    Cut,
    /// Ctrl+V 粘贴。
    Paste,
    /// Ctrl+Shift+Z 重做（主键位）。
    Redo,
    /// Ctrl+Z 撤销（批撤销语义在执行层）。
    Undo,
    /// Ctrl+Y 重做（第二键位——独立变体允许单独换绑）。
    RedoY,
    /// Ctrl+D 加同词光标。
    AddNextWordMatch,
    /// Alt+F 围栏块折叠 toggle。
    ToggleFold,
    /// Ctrl+Shift+[ 折叠当前块。
    FoldCurrent,
    /// Ctrl+Shift+] 展开当前块。
    UnfoldCurrent,
    /// Ctrl+K Ctrl+0 全部折叠（chord）。
    FoldAll,
    /// Ctrl+K Ctrl+J 全部展开（chord）。
    UnfoldAll,
    /// Ctrl+F 打开查找条。
    OpenFind,
    /// Ctrl+H 打开替换条。
    OpenReplace,
    /// Ctrl+S 保存（!shift 守卫）。
    Save,
    /// Ctrl+Shift+S 的编辑器空臂（`"s" if ctrl => {}`——不消费，
    /// 冒泡 shell 另存为；执行体恒空，冲突检测豁免面）。
    Pass,
}

/// shell 域动作（`ShellApp::exec_shell_action` 的分派目标——
/// 每变体指向一个既有方法，不写新执行逻辑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellAction {
    /// Ctrl+O 打开文件（`prompt_open_file`）。
    PromptOpenFile,
    /// Ctrl+K Ctrl+O 打开文件夹工作区（`prompt_open_workspace`）。
    PromptOpenWorkspace,
    /// Ctrl+Tab 下一标签（`cycle_tab(true)`）。
    NextTab,
    /// Ctrl+Shift+Tab 上一标签（`cycle_tab(false)`）。
    PrevTab,
    /// Ctrl+W 关闭当前标签（`close_active_tab`）。
    CloseTab,
    /// #386 Ctrl+S 保存（Shell `save_tab` 分流——pdf 批注增量落盘/
    /// 编辑器三态；编辑器层 E30 已移除冒泡至此）。
    Save,
    /// Ctrl+Shift+S 另存为（`prompt_save_as`）。
    PromptSaveAs,
    /// Ctrl+= / Ctrl++ 字号增大（`apply_font_size(+1)`）。
    FontBigger,
    /// Ctrl+- 字号减小。
    FontSmaller,
    /// Ctrl+0 字号重置 15px。
    FontReset,
    /// Ctrl+T 循环主题（`cycle_theme`）。
    CycleTheme,
}

/// 两域动作统一面（用户覆盖的 `action` 机器名解析目标 / 冲突检测的
/// 全表遍历面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Editor(EditorAction),
    Shell(ShellAction),
}

impl Action {
    /// 机器名（snake_case——settings.json `action` 字段与标签查询的
    /// 键；与 `Cmd::id()` 重叠项同串）。
    pub fn id(&self) -> &'static str {
        match self {
            Action::Editor(a) => a.id(),
            Action::Shell(a) => a.id(),
        }
    }

    /// 按机器名反查（未知 None——调用方给可读警告）。
    pub fn from_id(id: &str) -> Option<Action> {
        EDITOR_ACTION_IDS
            .iter()
            .copied()
            .map(Action::Editor)
            .chain(SHELL_ACTION_IDS.iter().copied().map(Action::Shell))
            .find(|a| a.id() == id)
    }
}

impl EditorAction {
    /// 机器名。
    pub fn id(&self) -> &'static str {
        match self {
            EditorAction::PaletteToggle => "toggle_command_palette",
            EditorAction::GotoOpen => "goto_line",
            EditorAction::MoveKey(MoveDir::Left) => "move_left",
            EditorAction::MoveKey(MoveDir::Right) => "move_right",
            EditorAction::MoveKey(MoveDir::Up) => "move_up",
            EditorAction::MoveKey(MoveDir::Down) => "move_down",
            EditorAction::HomeKey => "move_home",
            EditorAction::EndKey => "move_end",
            EditorAction::PageUpKey => "page_up",
            EditorAction::PageDownKey => "page_down",
            EditorAction::Newline => "newline",
            EditorAction::Backspace => "backspace",
            EditorAction::DeleteForward => "delete",
            EditorAction::Indent => "indent",
            EditorAction::EscapeClearSelection => "escape",
            EditorAction::SelectAll => "select_all",
            EditorAction::Copy => "copy",
            EditorAction::Cut => "cut",
            EditorAction::Paste => "paste",
            EditorAction::Redo => "redo",
            EditorAction::Undo => "undo",
            EditorAction::RedoY => "redo_y",
            EditorAction::AddNextWordMatch => "add_next_word_match",
            EditorAction::ToggleFold => "toggle_fold",
            EditorAction::FoldCurrent => "fold_current",
            EditorAction::UnfoldCurrent => "unfold_current",
            EditorAction::FoldAll => "fold_all",
            EditorAction::UnfoldAll => "unfold_all",
            EditorAction::OpenFind => "open_find",
            EditorAction::OpenReplace => "open_replace",
            EditorAction::Save => "editor_save",
            EditorAction::Pass => "editor_pass",
        }
    }

    /// 只读模式下的动作安全性（动作级兜底——#218 review 修复）。
    ///
    /// 物理键级守卫（editor/mod.rs 的 `docx_doc::readonly_allows`）只认
    /// **按键**不认动作：用户把放行键（如 ctrl+c）换绑到破坏性动作
    /// （如 delete）后，物理守卫照常放行、动作照样执行。本函数在动作
    /// 执行层兜底：接线层对 `Matched(action)` 且 `read_only` 且
    /// `!readonly_safe()` 整体拒绝（双层防御第二层）。
    ///
    /// 口径镜像 `docx_doc::readonly_allows`（docx_doc.rs:1101）的白名单
    /// 语义——浏览类放行、变更类拒绝；折键/替换/加光标等的默认键
    /// （ctrl+shift+[、ctrl+h、ctrl+d）本就未过该白名单，保守封堵与
    /// 现状一致。默认表不变量（凡 canonical 键过白名单者动作必 safe）
    /// 由单测 `default_table_readonly_guard_invariant` 钉定——默认行为
    /// 零变化。
    pub fn readonly_safe(&self) -> bool {
        match self {
            // 浏览类：面板/跳行/移动/home/end/翻页/清选区/全选/复制/
            // 查找（readonly_allows 对 left..escape 恒放行、a/c/f 仅
            // ctrl 放行的动作面对应物；Pass 空臂无行为）。
            EditorAction::PaletteToggle
            | EditorAction::GotoOpen
            | EditorAction::MoveKey(_)
            | EditorAction::HomeKey
            | EditorAction::EndKey
            | EditorAction::PageUpKey
            | EditorAction::PageDownKey
            | EditorAction::EscapeClearSelection
            | EditorAction::SelectAll
            | EditorAction::Copy
            | EditorAction::OpenFind
            | EditorAction::Pass => true,
            // 变更类：输入/删除/缩进/剪贴板写/撤销重做/加光标/折叠
            // （折叠改持久化状态）/替换/保存——只读态一律拒绝。
            EditorAction::Newline
            | EditorAction::Backspace
            | EditorAction::DeleteForward
            | EditorAction::Indent
            | EditorAction::Cut
            | EditorAction::Paste
            | EditorAction::Redo
            | EditorAction::Undo
            | EditorAction::RedoY
            | EditorAction::AddNextWordMatch
            | EditorAction::Save
            | EditorAction::OpenReplace
            | EditorAction::ToggleFold
            | EditorAction::FoldCurrent
            | EditorAction::UnfoldCurrent
            | EditorAction::FoldAll
            | EditorAction::UnfoldAll => false,
        }
    }
}

impl ShellAction {
    /// 机器名。
    pub fn id(&self) -> &'static str {
        match self {
            ShellAction::PromptOpenFile => "prompt_open_file",
            ShellAction::PromptOpenWorkspace => "prompt_open_workspace",
            ShellAction::NextTab => "next_tab",
            ShellAction::PrevTab => "prev_tab",
            ShellAction::CloseTab => "close_tab",
            ShellAction::Save => "save",
            ShellAction::PromptSaveAs => "prompt_save_as",
            ShellAction::FontBigger => "font_bigger",
            ShellAction::FontSmaller => "font_smaller",
            ShellAction::FontReset => "font_reset",
            ShellAction::CycleTheme => "cycle_theme",
        }
    }
}

/// 编辑器动作全集（id 反查遍历面——顺序即文档序）。
const EDITOR_ACTION_IDS: &[EditorAction] = &[
    EditorAction::PaletteToggle,
    EditorAction::GotoOpen,
    EditorAction::MoveKey(MoveDir::Left),
    EditorAction::MoveKey(MoveDir::Right),
    EditorAction::MoveKey(MoveDir::Up),
    EditorAction::MoveKey(MoveDir::Down),
    EditorAction::HomeKey,
    EditorAction::EndKey,
    EditorAction::PageUpKey,
    EditorAction::PageDownKey,
    EditorAction::Newline,
    EditorAction::Backspace,
    EditorAction::DeleteForward,
    EditorAction::Indent,
    EditorAction::EscapeClearSelection,
    EditorAction::SelectAll,
    EditorAction::Copy,
    EditorAction::Cut,
    EditorAction::Paste,
    EditorAction::Redo,
    EditorAction::Undo,
    EditorAction::RedoY,
    EditorAction::AddNextWordMatch,
    EditorAction::ToggleFold,
    EditorAction::FoldCurrent,
    EditorAction::UnfoldCurrent,
    EditorAction::FoldAll,
    EditorAction::UnfoldAll,
    EditorAction::OpenFind,
    EditorAction::OpenReplace,
    EditorAction::Save,
    EditorAction::Pass,
];

/// shell 动作全集。
const SHELL_ACTION_IDS: &[ShellAction] = &[
    ShellAction::PromptOpenFile,
    ShellAction::PromptOpenWorkspace,
    ShellAction::NextTab,
    ShellAction::PrevTab,
    ShellAction::CloseTab,
    ShellAction::Save,
    ShellAction::PromptSaveAs,
    ShellAction::FontBigger,
    ShellAction::FontSmaller,
    ShellAction::FontReset,
    ShellAction::CycleTheme,
];

/// 查表点（dispatch 守卫序列中的位置——绑定条目的归属域）。
///
/// 前置层动作在守卫序列的**早期**查表（其位置即现状分支位置）；
/// 主层在只读门禁之后。分层是行为零变化的根据：模态守卫（canvas/
/// palette/goto/只读门禁）保留原位原序，查表点嵌在原分支位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// 编辑器前置：命令面板 toggle（palette 模态之前）。
    EditorPalette,
    /// 编辑器前置：跳行框开启（goto 模态之前、只读门禁之前）。
    EditorGoto,
    /// 编辑器主层（chord + match 主体——只读门禁之后）。
    EditorMain,
    /// shell 层（菜单/确认框/右键菜单模态守卫之后）。
    Shell,
}

/// 一条绑定（pattern 序列 + 动作）。
#[derive(Clone, Debug)]
pub struct Entry {
    pub binding: Binding,
    pub action: Action,
}

// ============================================================================
// 键位表
// ============================================================================

/// 键位表（默认表 + 用户覆盖的合并产物——只读消费面）。
#[derive(Clone, Debug)]
pub struct KeymapStore {
    /// 前置层：palette toggle（原 Ctrl+Shift+P 臂位置）。
    editor_palette: Vec<Entry>,
    /// 前置层：goto 开框（原 Ctrl+G 臂位置）。
    editor_goto: Vec<Entry>,
    /// 主层：chord + match 主体（原 `ctrl_k_pending` 预处理 + match）。
    editor_main: Vec<Entry>,
    /// shell 层：模态守卫后的全部分支。
    shell: Vec<Entry>,
}

/// 默认编辑器主层表（表序 = 原 match 臂序——匹配取舍的兜底序）。
fn default_editor_main() -> Vec<Entry> {
    use EditorAction as A;
    use Mod::{Any, No, Yes};
    let e = |binding: Binding, action: A| Entry {
        binding,
        action: Action::Editor(action),
    };
    vec![
        // 移动家族（E4-E7：键位→方向；ctrl 词跳/shift 选区在执行层
        // 参数化——现状 do_move 签名，家族内语义不可拆分重绑）。
        e(
            Binding::Single(KeyPattern::key("left")),
            A::MoveKey(MoveDir::Left),
        ),
        e(
            Binding::Single(KeyPattern::key("right")),
            A::MoveKey(MoveDir::Right),
        ),
        e(
            Binding::Single(KeyPattern::key("up")),
            A::MoveKey(MoveDir::Up),
        ),
        e(
            Binding::Single(KeyPattern::key("down")),
            A::MoveKey(MoveDir::Down),
        ),
        // home/end 家族（E8-E9：ctrl 分流文首/文尾在执行层）。
        e(Binding::Single(KeyPattern::key("home")), A::HomeKey),
        e(Binding::Single(KeyPattern::key("end")), A::EndKey),
        // 翻页（E10-E11：PAGE_LINES 步进 + 固定非 ctrl 在执行层）。
        e(Binding::Single(KeyPattern::key("pageup")), A::PageUpKey),
        e(Binding::Single(KeyPattern::key("pagedown")), A::PageDownKey),
        // 编辑（E12-E15：tab 带 !ctrl 守卫——Ctrl+Tab 冒泡 shell 标签循环）。
        e(Binding::Single(KeyPattern::key("enter")), A::Newline),
        e(Binding::Single(KeyPattern::key("backspace")), A::Backspace),
        e(Binding::Single(KeyPattern::key("delete")), A::DeleteForward),
        e(
            Binding::Single(KeyPattern::key("tab").with(No, Any, Any)),
            A::Indent,
        ),
        // Esc 清选区（E16）。
        e(
            Binding::Single(KeyPattern::key("escape")),
            A::EscapeClearSelection,
        ),
        // 选区/剪贴板（E17-E20）。
        e(
            Binding::Single(KeyPattern::key("a").with(Yes, Any, Any)),
            A::SelectAll,
        ),
        e(
            Binding::Single(KeyPattern::key("c").with(Yes, Any, Any)),
            A::Copy,
        ),
        e(
            Binding::Single(KeyPattern::key("x").with(Yes, Any, Any)),
            A::Cut,
        ),
        e(
            Binding::Single(KeyPattern::key("v").with(Yes, Any, Any)),
            A::Paste,
        ),
        // undo/redo（E21-E23：z 双臂约束数不同——精确度排序复现臂序）。
        e(
            Binding::Single(KeyPattern::key("z").with(Yes, Yes, Any)),
            A::Redo,
        ),
        e(
            Binding::Single(KeyPattern::key("z").with(Yes, Any, Any)),
            A::Undo,
        ),
        e(
            Binding::Single(KeyPattern::key("y").with(Yes, Any, Any)),
            A::RedoY,
        ),
        // Ctrl+D 加同词光标（E24）。
        e(
            Binding::Single(KeyPattern::key("d").with(Yes, Any, Any)),
            A::AddNextWordMatch,
        ),
        // Alt+F 折叠 toggle（E25——臂序在 [-] 臂前、"f" if ctrl 臂后……
        // 不：match 臂序 f-alt 在 [ 前；同约束数并存时 ctrl+alt+f 走
        // toggle——精确复现 match 臂序）。
        e(
            Binding::Single(KeyPattern::key("f").with(Any, Any, Yes)),
            A::ToggleFold,
        ),
        // 折叠快捷键（E26-E27）。
        e(
            Binding::Single(KeyPattern::key("[").with(Yes, Yes, Any)),
            A::FoldCurrent,
        ),
        e(
            Binding::Single(KeyPattern::key("]").with(Yes, Yes, Any)),
            A::UnfoldCurrent,
        ),
        // 查找/替换（E28-E29：f-ctrl 臂在 [-] 臂之后——表序复现）。
        e(
            Binding::Single(KeyPattern::key("f").with(Yes, Any, Any)),
            A::OpenFind,
        ),
        e(
            Binding::Single(KeyPattern::key("h").with(Yes, Any, Any)),
            A::OpenReplace,
        ),
        // 保存（#386：E30 旧默认键位（ctrl+!shift → Save）已移除
        // ——Ctrl+S 不再由编辑器自处理，整键放行冒泡至 shell 域
        // ShellAction::Save[保存分流在 Shell：pdf 批注增量落盘/编辑器
        // 三态；Ctrl+Shift+S 冒泡至另存为]。EditorAction::Save 变体
        // 仍在（用户可重绑到其它键 + id 反查面），仅无默认键位）。
        e(
            Binding::Single(KeyPattern::key("s").with(Yes, Any, Any)),
            A::Pass,
        ),
        // chord（E2-E3 + E32 首键挂起——续接键修饰 Any：现状守卫
        // `if self.ctrl_k_pending` 只看 key，Ctrl+K Ctrl+0 亦触发）。
        e(
            Binding::Chord(
                KeyPattern::key("k").with(Yes, No, Any),
                KeyPattern::key("0"),
            ),
            A::FoldAll,
        ),
        e(
            Binding::Chord(
                KeyPattern::key("k").with(Yes, No, Any),
                KeyPattern::key("j"),
            ),
            A::UnfoldAll,
        ),
    ]
}

/// 默认 shell 层表（表序 = 原分支序）。
fn default_shell() -> Vec<Entry> {
    use Mod::{Any, No, Yes};
    use ShellAction as A;
    let e = |binding: Binding, action: A| Entry {
        binding,
        action: Action::Shell(action),
    };
    // plain = ctrl 且 !shift 且 !alt（现状 `plain_ctrl`）。
    let plain = |key: &str| KeyPattern::key(key).with(Yes, No, No);
    vec![
        // S1：Ctrl+K Ctrl+O 和弦（2s 窗口在接线层——引擎只管 pattern）。
        e(
            Binding::Chord(plain("k"), plain("o")),
            A::PromptOpenWorkspace,
        ),
        // S2-S3：Ctrl+Tab / Ctrl+Shift+Tab（alt 守卫）。
        e(
            Binding::Single(KeyPattern::key("tab").with(Yes, No, No)),
            A::NextTab,
        ),
        e(
            Binding::Single(KeyPattern::key("tab").with(Yes, Yes, No)),
            A::PrevTab,
        ),
        // S4：Ctrl+W 关标签。
        e(Binding::Single(plain("w")), A::CloseTab),
        // #386 S12：Ctrl+S 保存（编辑器 E30 已移除整键放行冒泡至此
        // ——「谁负责保存」归 Shell：pdf 标签走批注增量落盘，其余走
        // Editor 三态；失败 toast 示警不静默）。
        e(Binding::Single(plain("s")), A::Save),
        // S5：Ctrl+Shift+S 另存为（编辑器 s 空臂放行冒泡至此）。
        e(
            Binding::Single(KeyPattern::key("s").with(Yes, Yes, No)),
            A::PromptSaveAs,
        ),
        // S6-S8：字号（"=" 与 "+" 同分支双键位；shift Any——"+" 多为
        // shift+= 产生）。
        e(
            Binding::Single(KeyPattern::key("=").with(Yes, Any, No)),
            A::FontBigger,
        ),
        e(
            Binding::Single(KeyPattern::key("+").with(Yes, Any, No)),
            A::FontBigger,
        ),
        e(
            Binding::Single(KeyPattern::key("-").with(Yes, Any, No)),
            A::FontSmaller,
        ),
        // S9：Ctrl+0 重置。
        e(Binding::Single(plain("0")), A::FontReset),
        // S10：Ctrl+T 循环主题。
        e(Binding::Single(plain("t")), A::CycleTheme),
        // S11：Ctrl+O 打开文件（和弦续接优先——无挂起时才到此）。
        e(Binding::Single(plain("o")), A::PromptOpenFile),
    ]
}

impl KeymapStore {
    /// 默认表（现状两处匹配臂的精确镜像——见模块文档逐臂清单）。
    pub fn default_tables() -> Self {
        KeymapStore {
            editor_palette: vec![Entry {
                binding: Binding::Single(KeyPattern::key("p").with(Mod::Yes, Mod::Yes, Mod::Any)),
                action: Action::Editor(EditorAction::PaletteToggle),
            }],
            editor_goto: vec![Entry {
                binding: Binding::Single(KeyPattern::key("g").with(Mod::Yes, Mod::No, Mod::Any)),
                action: Action::Editor(EditorAction::GotoOpen),
            }],
            editor_main: default_editor_main(),
            shell: default_shell(),
        }
    }

    /// 四域快照（冲突检测/标签反查的遍历面——按值返回避免借用悬挂）。
    fn domains(&self) -> [(&[Entry], Site); 4] {
        [
            (&self.editor_palette, Site::EditorPalette),
            (&self.editor_goto, Site::EditorGoto),
            (&self.editor_main, Site::EditorMain),
            (&self.shell, Site::Shell),
        ]
    }

    /// 前置层查表（EditorPalette / EditorGoto——该层无 chord，返回
    /// 至多一条动作；接线层按 site 在守卫序列的正确位置调用）。
    pub fn resolve_pre(&self, site: Site, q: &KeyQuery) -> Option<Action> {
        let entries = match site {
            Site::EditorPalette => &self.editor_palette,
            Site::EditorGoto => &self.editor_goto,
            _ => return None,
        };
        best_single(entries, q).map(|e| e.action)
    }

    /// 编辑器主层查表（chord + match 主体）。
    ///
    /// `pending`：当前挂起的 chord **首键实际按键快照**（`None` = 无挂起
    /// ——接线层的 `Editor.chord_pending` 字段存 [`KeyQuery`]）。返回：
    /// - [`EditorOutcome::Matched`]：执行动作（挂起已终结——续接命中
    ///   或挂起作废后的单键命中）；
    /// - [`EditorOutcome::Pending`]：chord 首键命中，进入/刷新挂起
    ///   （接线层存本键快照并**提前 return 不落尾部**——现状
    ///   `"k"` 臂语义）；
    /// - [`EditorOutcome::None`]：无命中（挂起终结；接线层走原 `_`
    ///   分支可打印字符插入）。
    pub fn resolve_editor_main(&self, q: &KeyQuery, pending: Option<&KeyQuery>) -> EditorOutcome {
        // 挂起中：先查续接（首键快照能触发的 chord + 续接键匹配，
        // 尾键精确度/表序取舍——用户和弦首键 Any 变体同样可达）。
        if let Some(first) = pending
            && let Some(entry) = best_continuation(&self.editor_main, first, q)
        {
            return match entry.action {
                Action::Editor(a) => EditorOutcome::Matched(a),
                Action::Shell(_) => EditorOutcome::None,
            };
        }
        // 挂起未续接（或本无挂起）：挂起终结，本键当新序列起点下沉。
        match best_single(&self.editor_main, q) {
            Some(entry) => match &entry.binding {
                Binding::Single(_) => match entry.action {
                    Action::Editor(a) => EditorOutcome::Matched(a),
                    Action::Shell(_) => EditorOutcome::None,
                },
                Binding::Chord(..) => EditorOutcome::Pending,
            },
            None => EditorOutcome::None,
        }
    }

    /// shell 层查表（模态守卫后）。
    ///
    /// `chord`：挂起首键按键快照 + deadline（接线层的 `ShellApp.chord`
    /// 字段）；`now`：当前时刻（过期视作无挂起——**作废不吞键**，本键
    /// 照常单键匹配，对齐现状 `chord_until` take 检查语义）。
    pub fn resolve_shell(
        &self,
        q: &KeyQuery,
        chord: Option<&(KeyQuery, Instant)>,
        now: Instant,
    ) -> ShellOutcome {
        if let Some((first, until)) = chord
            && now <= *until
            && let Some(entry) = best_continuation(&self.shell, first, q)
        {
            return match entry.action {
                Action::Shell(a) => ShellOutcome::Matched(a),
                Action::Editor(_) => ShellOutcome::None,
            };
        }
        // 挂起作废/过期/无挂起：单键匹配（本键不吞——对齐现状分支序
        // 「和弦检查后继续走后续分支」）。
        match best_single(&self.shell, q) {
            Some(entry) => match &entry.binding {
                Binding::Single(_) => match entry.action {
                    Action::Shell(a) => ShellOutcome::Matched(a),
                    Action::Editor(_) => ShellOutcome::None,
                },
                Binding::Chord(..) => ShellOutcome::Pending,
            },
            None => ShellOutcome::None,
        }
    }

    /// 动作 → 人读标签（该动作的**第一条**绑定——menu_bar 快捷键
    /// 标签的单一事实源；无绑定 → None，菜单不标）。
    ///
    /// 格式对齐 menu_bar 既有标签字面量：`Ctrl+K Ctrl+O`（修饰首字母
    /// 大写、字母键大写、非字母键原样、chord 空格连接）。
    pub fn label(&self, action_id: &str) -> Option<String> {
        for (entries, _) in self.domains() {
            for entry in entries {
                if entry.action.id() == action_id {
                    return Some(label_binding(&entry.binding));
                }
            }
        }
        None
    }

    /// 应用用户覆盖（settings.json `keymap` 键——逐条按序，返回警告
    /// 列表由调用方落 stderr）。
    ///
    /// # 合并与冲突策略（#217）
    ///
    /// - **`bind` = 换绑**（不是追加）：检测通过后移除该动作的**全部**
    ///   既有绑定（旧键不再触发——issue 验收口径），且被新键串**域
    ///   覆盖**的同形态条目整体让位（用户说 `ctrl+k`，默认表
    ///   `ctrl+k(!shift)` 在其域内——由新条目接管；新条目插入被覆盖
    ///   首条目的原表位，保留表序语义）；
    /// - **同域冲突检测（canonical 赢家）**：用户键串的 canonical
    ///   按键（写出的修饰=按下、未写=未按下）在**换绑后的表**里必须
    ///   触发本动作——否则该键被更精确/更早的既有条目抢走，拒绝
    ///   （动作保持既有绑定；抢走者可以是 chord 首键——单键 bind
    ///   被更高精确度的 chord 前缀抢赢时该键恒 Pending，新绑定永不
    ///   触发，同样拒绝，#218）；更精确的新条目（如 `alt+left` 叠在
    ///   方向键家族的 Any 条目上）能赢下 canonical → 允许并存；
    /// - **跨域检测（结构相交）**：与另一域（非 `Pass`）存在相交按键
    ///   组合即拒绝——编辑器不拦冒泡，同键两域会叠加执行（保守面：
    ///   只要有一种按键组合双触发就拒绝）；`Pass` 条目无行为，豁免；
    /// - **`unbind`**：移除该键串 canonical 按键**实际触发**的绑定
    ///   （全域四表各至多一条赢家，赢家规则 = dispatch 同款取舍
    ///   ——单键按精确度/表序、chord 按尾键精确度/表序，见
    ///   [`Self::remove_triggered`]；chord 需写全两键）；无命中给警告；
    /// - 解析失败（键串/动作名）、结构错误：**单条拒绝** + 可读警告，
    ///   其余条目照常生效（一条坏配置不废掉全部自定义——与 settings
    ///   容错文化一致：记忆是增强不是依赖）。
    pub fn apply_user(&mut self, overrides: &[UserOverride]) -> Vec<String> {
        let mut warnings = Vec::new();
        for o in overrides {
            match o {
                UserOverride::Bind { bind, action } => {
                    self.apply_bind(bind, action, &mut warnings);
                }
                UserOverride::Unbind { bind } => {
                    let binding = match parse_binding(bind) {
                        Ok(b) => b,
                        Err(err) => {
                            warnings.push(format!("unbind \"{bind}\"：{err}，已跳过"));
                            continue;
                        }
                    };
                    if !self.remove_triggered(&binding) {
                        warnings.push(format!(
                            "unbind \"{bind}\"：无此绑定（chord 需写全两键），已跳过"
                        ));
                    }
                }
            }
        }
        warnings
    }

    /// `bind` 单条应用（[`Self::apply_user`] 的实现体——先预演检测
    /// 后落地，拒绝 = 该条完全不生效）。
    fn apply_bind(&mut self, bind: &str, action_id: &str, warnings: &mut Vec<String>) {
        let binding = match parse_binding(bind) {
            Ok(b) => b,
            Err(err) => {
                warnings.push(format!("bind \"{bind}\" → {action_id}：{err}，已拒绝"));
                return;
            }
        };
        let Some(action) = Action::from_id(action_id) else {
            warnings.push(format!(
                "bind \"{bind}\"：未知动作 \"{action_id}\"，已拒绝（可绑定动作名见 keymap 模块文档）"
            ));
            return;
        };
        // 前置层动作（palette/goto 查表点无 chord 状态机）不支持 chord。
        if matches!(
            action,
            Action::Editor(EditorAction::PaletteToggle | EditorAction::GotoOpen)
        ) && matches!(binding, Binding::Chord(..))
        {
            warnings.push(format!(
                "bind \"{bind}\" → {action_id}：前置层动作不支持 chord 键串，已拒绝"
            ));
            return;
        }
        let is_editor = matches!(action, Action::Editor(_));
        let canon = canonical_queries(&binding);
        // 预演表：同域表 −（该动作全部绑定）−（被新键串域覆盖的同形态
        // 条目）；跨域表不动（覆盖面不越域——跨域相交由 ② 检测拒绝，
        // 不能悄悄接管对域动作）+ 新条目（目标表插入被覆盖首条目的
        // 原表位，无则表尾）。
        let mut sim = self.clone();
        for (list, same) in [
            (&mut sim.editor_palette, is_editor),
            (&mut sim.editor_goto, is_editor),
            (&mut sim.editor_main, is_editor),
            (&mut sim.shell, !is_editor),
        ] {
            if same {
                list.retain(|e| e.action != action && !e.binding.covered_by(&binding));
            }
        }
        let insert_idx = {
            let orig: &[Entry] = match action {
                Action::Editor(EditorAction::PaletteToggle) => &self.editor_palette,
                Action::Editor(EditorAction::GotoOpen) => &self.editor_goto,
                Action::Editor(_) => &self.editor_main,
                Action::Shell(_) => &self.shell,
            };
            let mut surviving = 0usize;
            let mut found = None;
            for e in orig {
                if e.action == action {
                    continue; // 换绑让位面（不计替换位）
                }
                if e.binding.covered_by(&binding) {
                    found = Some(surviving);
                    break;
                }
                surviving += 1;
            }
            found.unwrap_or_else(|| {
                let len = match action {
                    Action::Editor(EditorAction::PaletteToggle) => sim.editor_palette.len(),
                    Action::Editor(EditorAction::GotoOpen) => sim.editor_goto.len(),
                    Action::Editor(_) => sim.editor_main.len(),
                    Action::Shell(_) => sim.shell.len(),
                };
                len
            })
        };
        let new_entry = Entry {
            binding: binding.clone(),
            action,
        };
        match action {
            Action::Editor(EditorAction::PaletteToggle) => {
                sim.editor_palette.insert(insert_idx, new_entry)
            }
            Action::Editor(EditorAction::GotoOpen) => sim.editor_goto.insert(insert_idx, new_entry),
            Action::Editor(_) => sim.editor_main.insert(insert_idx, new_entry),
            Action::Shell(_) => sim.shell.insert(insert_idx, new_entry),
        }
        // ① 同域 canonical 检测：用户写的那个按键，在换绑后的表里沿
        // 接线序（palette → goto → main / shell）第一个赢家必须是本
        // 动作——被其他条目抢走即拒绝。
        let same_lists: Vec<&[Entry]> = if is_editor {
            vec![
                sim.editor_palette.as_slice(),
                sim.editor_goto.as_slice(),
                sim.editor_main.as_slice(),
            ]
        } else {
            vec![sim.shell.as_slice()]
        };
        let mut hit = false;
        for list in &same_lists {
            let Some(w) = best_single(list, &canon[0]) else {
                continue;
            };
            match &w.binding {
                Binding::Single(_) => {
                    if w.action != action {
                        warnings.push(format!(
                            "bind \"{bind}\" → {action_id}：与既有绑定 \"{}\" → {} 冲突（该键当前触发它；如需覆盖请先 unbind），已拒绝",
                            label_binding(&w.binding),
                            w.action.id()
                        ));
                        return;
                    }
                    hit = true;
                    break; // 单键本动作命中——后续点被遮蔽无妨
                }
                Binding::Chord(..) => {
                    // 用户绑定单键而该键赢家是 chord 首键：dispatch 产生
                    // Pending（chord 首键精确度/表序胜出），新单键绑定永不
                    // 触发——按 canonical 赢家策略拒绝（原实现直接取
                    // canon[1]，单键绑定无第二键会越界 panic——#218
                    // review 附加审查发现）。
                    let Some(second) = canon.get(1) else {
                        warnings.push(format!(
                            "bind \"{bind}\" → {action_id}：与既有绑定 \"{}\" → {} 冲突（该键当前触发它；如需覆盖请先 unbind），已拒绝",
                            label_binding(&w.binding),
                            w.action.id()
                        ));
                        return;
                    };
                    let Some(cont) = best_continuation(list, &canon[0], second) else {
                        continue;
                    };
                    if cont.action != action {
                        warnings.push(format!(
                            "bind \"{bind}\" → {action_id}：与既有绑定 \"{}\" → {} 冲突（该键当前触发它；如需覆盖请先 unbind），已拒绝",
                            label_binding(&cont.binding),
                            cont.action.id()
                        ));
                        return;
                    }
                    hit = true;
                    break;
                }
            }
        }
        if !hit {
            // 不可达（新条目恒匹配自身 canonical）——防御性拒绝。
            warnings.push(format!(
                "bind \"{bind}\" → {action_id}：内部检测未命中赢家，已拒绝"
            ));
            return;
        }
        // ② 跨域结构相交（非 Pass）：存在叠加执行的按键组合即拒绝。
        let cross_lists: Vec<&[Entry]> = if is_editor {
            vec![sim.shell.as_slice()]
        } else {
            vec![
                sim.editor_palette.as_slice(),
                sim.editor_goto.as_slice(),
                sim.editor_main.as_slice(),
            ]
        };
        for list in cross_lists {
            for entry in list {
                if e_action_is_pass(entry) || !entry.binding.conflicts(&binding) {
                    continue;
                }
                warnings.push(format!(
                    "bind \"{bind}\" → {action_id}：与{}域既有绑定 \"{}\" → {} 冲突（同键两域会叠加执行），已拒绝",
                    if is_editor { "shell" } else { "编辑器" },
                    label_binding(&entry.binding),
                    entry.action.id()
                ));
                return;
            }
        }
        // 检测全过——落地预演表。
        *self = sim;
    }

    /// 解绑落地：移除该键串 canonical 按键**实际触发**的绑定（每表
    /// 至多一条赢家；全域四表都查——「这个键不再触发任何东西」的可
    /// 预期语义）。返回是否有移除。
    ///
    /// 赢家规则**精确镜像 dispatch 两段式**（#218 review 修复——原
    /// `(首键, 尾键)` 字典序会移错绑定）：
    /// - 单键解绑：候选 = canonical 键匹配的 Single 条目，赢家 = 精确度
    ///   最大、同度表序先者（= dispatch 单键选择 [`best_single`] 的
    ///   取舍；chord 首键在 dispatch 产生 Pending 而非触发，不入候选
    ///   ——移除 chord 需写全两键）；
    /// - chord 解绑：候选 = 首键匹配 canonical 首键**且**尾键匹配
    ///   canonical 尾键的 chord 条目，赢家 = **尾键**精确度最大、同度
    ///   表序先者（= dispatch 续接选择 [`best_continuation`] 的取舍
    ///   ——首键已发生，分流只看尾键；与 `spec > best_spec` 严格比较
    ///   保持一致语义）。
    fn remove_triggered(&mut self, binding: &Binding) -> bool {
        let canon = canonical_queries(binding);
        let mut removed = false;
        for list in [
            &mut self.editor_palette,
            &mut self.editor_goto,
            &mut self.editor_main,
            &mut self.shell,
        ] {
            let winner: Option<usize> = match binding {
                Binding::Single(_) => {
                    let [c1] = canon.as_slice() else {
                        unreachable!("单键绑定的 canonical 恰一键")
                    };
                    single_unbind_winner(list, c1)
                }
                Binding::Chord(..) => {
                    let [c1, c2] = canon.as_slice() else {
                        unreachable!("chord 绑定的 canonical 恰两键")
                    };
                    chord_unbind_winner(list, c1, c2)
                }
            };
            if let Some(i) = winner {
                list.remove(i);
                removed = true;
            }
        }
        removed
    }

    /// 表自检：报告真歧义（默认表/合法覆盖应恒空——单测钉定）。
    ///
    /// 豁免（取舍确定，非歧义）：
    /// - 同域 pattern 相交但**不等**：精确度排序（严格不等时）或表序
    ///   （同精确度）给出确定胜者——默认表的 f/alt vs f/ctrl（臂序）、
    ///   z 双臂（约束数）、Save vs Pass（约束数）均属此类；
    /// - `Pass` 条目（无行为）；
    /// - chord 首键相交而续接键不相交（挂起后续接分流）。
    ///
    /// 报告（不确定/叠加）：
    /// - **跨域**相交（非 Pass）——两域叠加执行（冒泡不拦）；
    /// - 同域 **pattern 完全相等**的重复绑定——表序遮蔽，后者永不触发。
    pub fn check_conflicts(&self) -> Vec<String> {
        let flat: Vec<(&Entry, Site)> = self
            .domains()
            .iter()
            .flat_map(|&(entries, site)| entries.iter().map(move |e| (e, site)))
            .collect();
        let mut out = Vec::new();
        for i in 0..flat.len() {
            for j in (i + 1)..flat.len() {
                let (a, sa) = flat[i];
                let (b, sb) = flat[j];
                if a.action == b.action || e_action_is_pass(a) || e_action_is_pass(b) {
                    continue;
                }
                if !a.binding.conflicts(&b.binding) {
                    continue;
                }
                // 域比较（编辑器三查表点同域——palette/goto/main 间是
                // 接线序遮蔽而非叠加执行，不按跨域误报）。
                if same_domain(sa, true) != same_domain(sb, true) {
                    out.push(format!(
                        "\"{}\" → {}（编辑器域）与 \"{}\" → {}（shell 域）同键叠加执行",
                        label_binding(&a.binding),
                        a.action.id(),
                        label_binding(&b.binding),
                        b.action.id()
                    ));
                } else if a.binding == b.binding {
                    out.push(format!(
                        "\"{}\" 同时绑定 {} 与 {}（表序遮蔽后者）",
                        label_binding(&a.binding),
                        a.action.id(),
                        b.action.id()
                    ));
                }
                // 同域相交不等：取舍确定（精确度/表序）——合法。
            }
        }
        out
    }
}

/// 条目动作是否为编辑器 Pass（冲突豁免面）。
fn e_action_is_pass(e: &Entry) -> bool {
    matches!(e.action, Action::Editor(EditorAction::Pass))
}

/// site 是否与动作域（编辑器 true / shell false）同域。
fn same_domain(site: Site, is_editor_action: bool) -> bool {
    let site_is_editor = matches!(
        site,
        Site::EditorPalette | Site::EditorGoto | Site::EditorMain
    );
    site_is_editor == is_editor_action
}

/// 编辑器主层查表结果。
#[derive(Clone, Debug, PartialEq)]
pub enum EditorOutcome {
    /// 命中动作（挂起已终结）。
    Matched(EditorAction),
    /// chord 首键命中（接线层：存本键 [`KeyQuery`] 快照进入/刷新挂起
    /// 并提前 return，不落尾部 after_edit/notify）。
    Pending,
    /// 无命中（挂起终结；接线层走原 `_` 分支）。
    None,
}

/// shell 层查表结果。
#[derive(Clone, Debug, PartialEq)]
pub enum ShellOutcome {
    Matched(ShellAction),
    /// chord 首键命中（接线层：存本键快照 + deadline 并 return）。
    Pending,
    /// 无命中（挂起作废；接线层清挂起后自然结束）。
    None,
}

/// 单键匹配最佳条目：约束数降序、同数表序优先（现状 match 臂序的
/// 抽象——默认表下每键至多一条胜出，与单臂执行等价）。
///
/// chord 条目按首键参与（命中即挂起候选——与单键条目同规则竞争）。
fn best_single<'a>(entries: &'a [Entry], q: &KeyQuery) -> Option<&'a Entry> {
    let mut best: Option<(usize, &'a Entry)> = None;
    for entry in entries {
        if !entry.binding.head().matches(q) {
            continue;
        }
        let spec = entry.binding.head().specificity();
        let better = match best {
            None => true,
            Some((best_spec, _)) => spec > best_spec,
        };
        if better {
            best = Some((spec, entry));
        }
    }
    best.map(|(_, e)| e)
}

/// chord 续接最佳条目：首键快照能触发的 chord 中，续接键匹配当前
/// 查询者，按**尾键精确度**降序、同数表序优先（首键已发生——分流
/// 只看续接键；用户和弦首键 Any 变体与默认首键同可达）。
fn best_continuation<'a>(
    entries: &'a [Entry],
    first: &KeyQuery,
    q: &KeyQuery,
) -> Option<&'a Entry> {
    let mut best: Option<(usize, &'a Entry)> = None;
    for entry in entries {
        let Binding::Chord(p1, p2) = &entry.binding else {
            continue;
        };
        if p1.matches(first) && p2.matches(q) {
            let spec = p2.specificity();
            let better = match best {
                None => true,
                Some((best_spec, _)) => spec > best_spec,
            };
            if better {
                best = Some((spec, entry));
            }
        }
    }
    best.map(|(_, e)| e)
}

/// 单键解绑赢家：候选 = canonical 键匹配的 Single 条目，赢家 = 精确度
/// 最大、同度表序先者（比较语义与 [`best_single`] 一致——`spec >
/// best_spec` 严格大于，迭代序即表序）。返回赢家下标。
fn single_unbind_winner(entries: &[Entry], q: &KeyQuery) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (i, e) in entries.iter().enumerate() {
        let Binding::Single(p) = &e.binding else {
            continue;
        };
        if !p.matches(q) {
            continue;
        }
        let spec = p.specificity();
        let better = match best {
            None => true,
            Some((best_spec, _)) => spec > best_spec,
        };
        if better {
            best = Some((spec, i));
        }
    }
    best.map(|(_, i)| i)
}

/// chord 解绑赢家：候选 = 首键匹配 `c1` 且尾键匹配 `c2` 的 chord
/// 条目，赢家 = **尾键**精确度最大、同度表序先者（比较语义与
/// [`best_continuation`] 一致——首键已发生，分流只看尾键；#218
/// review：原 `(首键, 尾键)` 字典序会解绑 dispatch 不会触发的那条，
/// 如表同时有 `ctrl+shift+k x` 与 `ctrl+k ctrl+x` 时，序列
/// （ctrl+shift+k, ctrl+x）dispatch 触发后者（尾 spec 1>0），字典序
/// 却按头优先移除前者）。返回赢家下标。
fn chord_unbind_winner(entries: &[Entry], c1: &KeyQuery, c2: &KeyQuery) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (i, e) in entries.iter().enumerate() {
        let Binding::Chord(p1, p2) = &e.binding else {
            continue;
        };
        if !(p1.matches(c1) && p2.matches(c2)) {
            continue;
        }
        let spec = p2.specificity();
        let better = match best {
            None => true,
            Some((best_spec, _)) => spec > best_spec,
        };
        if better {
            best = Some((spec, i));
        }
    }
    best.map(|(_, i)| i)
}

/// 绑定的 canonical 按键序列（写出修饰=按下、未写=未按下——用户
/// 键串的「字面按键」；冲突检测与解绑的基准面）。
fn canonical_queries(b: &Binding) -> Vec<KeyQuery> {
    let one = |p: &KeyPattern| KeyQuery {
        key: p.key.clone(),
        ctrl: p.ctrl == Mod::Yes,
        shift: p.shift == Mod::Yes,
        alt: p.alt == Mod::Yes,
    };
    match b {
        Binding::Single(p) => vec![one(p)],
        Binding::Chord(p1, p2) => vec![one(p1), one(p2)],
    }
}

// ============================================================================
// 键串解析（settings.json `bind`/`unbind` 的值 → Binding）
// ============================================================================

/// 解析错误（可读文案——警告直出 stderr）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 解析键串：`ctrl+k ctrl+o`（空格分 chord，二键为限）、`ctrl+shift+[`、
/// `ctrl++`（"+" 键名——前缀扫描而非 split，天然正确）。
///
/// - 修饰词 `ctrl`/`shift`/`alt`（大小写不敏感，可组合，顺序任意；
///   重复修饰报错）；`ctrl` 运行时归一 control||platform（macOS ⌘）；
///   其余修饰词（meta/cmd/command/win/windows/super/option/altgr/
///   control）不支持——报错（防幻影键，见 [`parse_pattern`]）；
/// - 键名小写规范化（裸修饰词不能单独作键名）；
/// - 单 token = 单键；两 token = chord；空串/超两键/修饰后缺键名报错。
pub fn parse_binding(s: &str) -> Result<Binding, ParseError> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    match tokens.len() {
        1 => Ok(Binding::Single(parse_pattern(tokens[0])?)),
        2 => Ok(Binding::Chord(
            parse_pattern(tokens[0])?,
            parse_pattern(tokens[1])?,
        )),
        0 => Err(ParseError("键串为空".into())),
        n => Err(ParseError(format!("chord 至多两键（收到 {n} 段）"))),
    }
}

/// 大小写不敏感前缀消耗（`to_lowercase` 对多字节字符可能变长，不能用
/// 长度差回切原串——逐字节比较 ASCII 前缀）。
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len()
        && s.is_char_boundary(prefix.len())
        && s[..prefix.len()].eq_ignore_ascii_case(prefix)
    {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// 已知但**不支持**的修饰词（meta/cmd/command/win/windows/super/
/// option/altgr/control——平台修饰词的常见别名；引擎三态模型只有
/// ctrl/shift/alt）。显式报错而非静默当键名——否则整串落成 dispatch
/// 永不产生的**幻影键**（如 `meta+k`），bind 会悄悄移除旧绑定且新键
/// 永不触发（把 save 换绑到 meta+k = Ctrl+S 失效——#218 review）。
const UNSUPPORTED_MODIFIERS: &[&str] = &[
    "meta", "cmd", "command", "win", "windows", "super", "option", "altgr", "control",
];

/// 裸修饰词禁作键名（ctrl/shift/alt 及上述不支持词单独出现同样是
/// 幻影键——dispatch 的键名永不可能是修饰词本身）。
const BARE_MODIFIERS: &[&str] = &[
    "ctrl", "control", "shift", "alt", "meta", "cmd", "command", "win", "windows", "super",
    "option", "altgr",
];

/// 解析单键 pattern（`ctrl+shift+[` 形态——前缀扫描消耗修饰词）。
///
/// 解析失败即整条拒绝（apply 侧先 parse 后变异——旧绑定不受影响）。
/// 除既有错误（空串/重复修饰/修饰后缺键名）外，还拒绝三类幻影键
/// 形态（#218 review）：
/// - 已知但不支持的修饰词前缀（`meta+k` / `cmd+s` / `control+k`…）；
/// - 残段仍含 `+` 且不以 `+` 开头（`foo+bar`；`ctrl++` 的键名 "+"
///   以 `+` 开头，继续合法——前缀扫描天然不误拆）；
/// - 裸修饰词单独作键名（`meta` / `ctrl` / `alt`…）。
fn parse_pattern(token: &str) -> Result<KeyPattern, ParseError> {
    let mut ctrl = Mod::Any;
    let mut shift = Mod::Any;
    let mut alt = Mod::Any;
    let mut rest = token;
    loop {
        if let Some(r) = strip_prefix_ci(rest, "ctrl+") {
            if ctrl != Mod::Any {
                return Err(ParseError(format!("\"{token}\"：ctrl 重复")));
            }
            ctrl = Mod::Yes;
            rest = r;
        } else if let Some(r) = strip_prefix_ci(rest, "shift+") {
            if shift != Mod::Any {
                return Err(ParseError(format!("\"{token}\"：shift 重复")));
            }
            shift = Mod::Yes;
            rest = r;
        } else if let Some(r) = strip_prefix_ci(rest, "alt+") {
            if alt != Mod::Any {
                return Err(ParseError(format!("\"{token}\"：alt 重复")));
            }
            alt = Mod::Yes;
            rest = r;
        } else if let Some(word) = UNSUPPORTED_MODIFIERS
            .iter()
            .find(|&&w| strip_modifier_ci(rest, w).is_some())
        {
            return Err(ParseError(format!(
                "\"{token}\"：不支持的修饰词「{word}」（支持 ctrl/shift/alt）"
            )));
        } else {
            break;
        }
    }
    if rest.is_empty() {
        return Err(ParseError(format!("\"{token}\"：修饰词后缺键名")));
    }
    // 残段含 `+` 且不以 `+` 开头（如 "foo+bar"）：键名不含 "+"——
    // 以 "+" 开头的残段是键名 "+"（`ctrl++`），继续合法。
    if rest.contains('+') && !rest.starts_with('+') {
        let head = rest.split('+').next().unwrap_or_default();
        return Err(ParseError(format!(
            "\"{token}\"：不支持的修饰词「{head}」（支持 ctrl/shift/alt）"
        )));
    }
    // 裸修饰词单独作键名（dispatch 键名永不可能是修饰词本身）。
    if let Some(word) = BARE_MODIFIERS
        .iter()
        .find(|&&w| rest.eq_ignore_ascii_case(w))
    {
        return Err(ParseError(format!(
            "\"{token}\"：修饰词「{word}」不能单独作键名"
        )));
    }
    Ok(KeyPattern {
        key: rest.to_lowercase(),
        ctrl,
        shift,
        alt,
    })
}

/// 大小写不敏感修饰词前缀消耗（`meta+` → 剥离后剩余段；词后必须恰好
/// 是 `+`——`strip_prefix_ci` 的词级匹配保证不误伤 "windows" 起头的
/// "win" 等部分词）。
fn strip_modifier_ci<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    strip_prefix_ci(s, word)?.strip_prefix('+')
}

/// 用户覆盖条目（settings.json `keymap` 数组的元素——serde_json 手工
/// 遍历在 settings.rs 侧构造，本层不依赖 serde）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserOverride {
    /// 换绑：`{"bind": "ctrl+alt+p", "action": "toggle_command_palette"}`。
    Bind { bind: String, action: String },
    /// 解绑：`{"unbind": "ctrl+shift+p"}`。
    Unbind { bind: String },
}

// ============================================================================
// 标签生成（menu_bar 快捷键标签的单一事实源）
// ============================================================================

/// Binding → 人读标签（`Ctrl+K Ctrl+O` / `Ctrl+=` / `Ctrl+Shift+[`）。
///
/// 格式契约（与 menu_bar 既有 16 对标签字面量及 `Cmd::default_key`
/// 逐字对齐，钉定测试对拍）：修饰 `Ctrl`/`Shift`/`Alt` 首字母大写按
/// ctrl→shift→alt 序；键名字母大写、其余原样（`=`/`-`/`0`/`[`/`]`）；
/// chord 空格连接。`Any` 修饰不显示（`Ctrl+=` 的 shift Any——现状
/// 标签同款）；**chord 第二键的 ctrl 继承首键**（未显式排除时）——
/// `Cmd::default_key` 的 `Ctrl+K Ctrl+0` 口径（续接键修饰 Any 的
/// 惯常按法展示：ctrl 按住接第二键）。
pub fn label_binding(b: &Binding) -> String {
    match b {
        Binding::Single(p) => label_pattern(p),
        Binding::Chord(a, b) => {
            let mut b2 = b.clone();
            if b2.ctrl == Mod::Any && a.ctrl == Mod::Yes {
                b2.ctrl = Mod::Yes;
            }
            format!("{} {}", label_pattern(a), label_pattern(&b2))
        }
    }
}

fn label_pattern(p: &KeyPattern) -> String {
    let mut out = String::new();
    if p.ctrl == Mod::Yes {
        out.push_str("Ctrl+");
    }
    if p.shift == Mod::Yes {
        out.push_str("Shift+");
    }
    if p.alt == Mod::Yes {
        out.push_str("Alt+");
    }
    let key = p.key.as_str();
    if key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() {
        out.push_str(&key.to_ascii_uppercase());
    } else {
        out.push_str(key);
    }
    out
}

// ============================================================================
// 全局装配（bin 启动装载；未 init = 默认表——单测/harness 默认面）
// ============================================================================

/// 全局表（OnceLock——settings::ui_font / perf::boot 同款先例；
/// 未 init 时惰性落默认表，任何查表点零初始化负担）。
static STORE: std::sync::OnceLock<KeymapStore> = std::sync::OnceLock::new();

/// 全局表引用（未 [`init`] 时 = 默认表）。
pub fn store() -> &'static KeymapStore {
    STORE.get_or_init(KeymapStore::default_tables)
}

/// 装载用户覆盖（bin main 启动时一次：默认表 + settings.json `keymap`
/// → 合并产物落全局；警告逐条落 stderr——「记忆是增强不是依赖」）。
///
/// 二次调用为空操作（单进程一次；幂等不覆盖——测试进程互不污染）。
pub fn init(overrides: &[UserOverride]) {
    if STORE.get().is_some() {
        return;
    }
    let mut merged = KeymapStore::default_tables();
    for warning in merged.apply_user(overrides) {
        eprintln!("keymap: {warning}");
    }
    let _ = STORE.set(merged);
}

// ============================================================================
// 单元测试（owner 门禁 `cargo test` 生效；harness `#[path]` 挂载时
// 同一份测试同样执行——双环境同源）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use Mod::{Any, Yes};

    /// 查询构造（接线层 `KeyQuery` 的测试面）。
    fn q(key: &str, ctrl: bool, shift: bool, alt: bool) -> KeyQuery {
        KeyQuery {
            key: key.to_string(),
            ctrl,
            shift,
            alt,
        }
    }

    /// 断言默认编辑器主表命中（无挂起）。
    fn editor_hit(key: &str, ctrl: bool, shift: bool, alt: bool, want: EditorAction) {
        let store = KeymapStore::default_tables();
        let got = store.resolve_editor_main(&q(key, ctrl, shift, alt), None);
        assert_eq!(
            got,
            EditorOutcome::Matched(want),
            "键 {key}（ctrl={ctrl} shift={shift} alt={alt}）应命中 {want:?}，实得 {got:?}"
        );
    }

    /// 断言默认编辑器主表无命中（`_` 分支——可打印插入/冒泡面）。
    fn editor_miss(key: &str, ctrl: bool, shift: bool, alt: bool) {
        let store = KeymapStore::default_tables();
        let got = store.resolve_editor_main(&q(key, ctrl, shift, alt), None);
        assert_eq!(
            got,
            EditorOutcome::None,
            "键 {key}（ctrl={ctrl} shift={shift} alt={alt}）应无命中"
        );
    }

    /// 断言默认 shell 表命中（无挂起）。
    fn shell_hit(key: &str, ctrl: bool, shift: bool, alt: bool, want: ShellAction) {
        let store = KeymapStore::default_tables();
        let got = store.resolve_shell(&q(key, ctrl, shift, alt), None, Instant::now());
        assert_eq!(
            got,
            ShellOutcome::Matched(want),
            "shell 键 {key}（ctrl={ctrl} shift={shift} alt={alt}）应命中 {want:?}，实得 {got:?}"
        );
    }

    /// 断言默认 shell 表无命中。
    fn shell_miss(key: &str, ctrl: bool, shift: bool, alt: bool) {
        let store = KeymapStore::default_tables();
        let got = store.resolve_shell(&q(key, ctrl, shift, alt), None, Instant::now());
        assert_eq!(
            got,
            ShellOutcome::None,
            "shell 键 {key}（ctrl={ctrl} shift={shift} alt={alt}）应无命中"
        );
    }

    // ─── 默认表自检 ─────────────────────────────────────────────────

    /// 默认表无歧义绑定（同域相交不等的合法取舍 + Pass 豁免之外无冲突
    /// ——check_conflicts 的豁免规则）。
    #[test]
    fn default_tables_conflict_free() {
        let store = KeymapStore::default_tables();
        let conflicts = store.check_conflicts();
        assert!(conflicts.is_empty(), "默认表存在歧义：{conflicts:?}");
    }

    /// 机器名全域唯一 + id/from_id 往返（用户覆盖解析的根据）。
    #[test]
    fn action_ids_unique_and_roundtrip() {
        let mut ids: Vec<&str> = Vec::new();
        for a in EDITOR_ACTION_IDS
            .iter()
            .copied()
            .map(Action::Editor)
            .chain(SHELL_ACTION_IDS.iter().copied().map(Action::Shell))
        {
            ids.push(a.id());
            assert_eq!(
                Action::from_id(a.id()),
                Some(a),
                "{} 反查应命中自身",
                a.id()
            );
        }
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "机器名不得重复：{ids:?}");
        assert_eq!(Action::from_id("no_such_action"), None);
        assert_eq!(Action::from_id(""), None);
    }

    // ─── 编辑器主层逐臂对照（E2-E32——默认行为零变化红线） ──────────

    /// 移动家族 E4-E7：裸键/shift 选区/ctrl 词跳（修饰进执行层参数——
    /// 表层只认键，Any 修饰全命中）。
    #[test]
    fn editor_move_family_all_modifier_combos() {
        for (key, dir) in [
            ("left", MoveDir::Left),
            ("right", MoveDir::Right),
            ("up", MoveDir::Up),
            ("down", MoveDir::Down),
        ] {
            for (ctrl, shift, alt) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, true, true),
            ] {
                editor_hit(key, ctrl, shift, alt, EditorAction::MoveKey(dir));
            }
        }
    }

    /// home/end 家族 E8-E9：ctrl 分流在执行层——表层全修饰命中同动作。
    #[test]
    fn editor_home_end_family() {
        for (ctrl, shift) in [(false, false), (true, false), (false, true), (true, true)] {
            editor_hit("home", ctrl, shift, false, EditorAction::HomeKey);
            editor_hit("end", ctrl, shift, false, EditorAction::EndKey);
        }
    }

    /// 翻页/编辑键 E10-E14。
    #[test]
    fn editor_paging_and_edit_keys() {
        for (ctrl, shift) in [(false, false), (true, false), (false, true)] {
            editor_hit("pageup", ctrl, shift, false, EditorAction::PageUpKey);
            editor_hit("pagedown", ctrl, shift, false, EditorAction::PageDownKey);
            editor_hit("enter", ctrl, shift, false, EditorAction::Newline);
            editor_hit("backspace", ctrl, shift, false, EditorAction::Backspace);
            editor_hit("delete", ctrl, shift, false, EditorAction::DeleteForward);
        }
    }

    /// E15：Tab 带 !ctrl 守卫——Ctrl+Tab 不命中（冒泡 shell 标签循环）。
    #[test]
    fn editor_tab_guard_bubbles_ctrl_tab() {
        editor_hit("tab", false, false, false, EditorAction::Indent);
        editor_hit("tab", false, true, false, EditorAction::Indent);
        editor_miss("tab", true, false, false); // Ctrl+Tab → shell
        editor_miss("tab", true, true, false); // Ctrl+Shift+Tab → shell
    }

    /// E16-E20：Esc / 选区 / 剪贴板。
    #[test]
    fn editor_escape_clipboard_select_all() {
        editor_hit(
            "escape",
            false,
            false,
            false,
            EditorAction::EscapeClearSelection,
        );
        editor_hit("a", true, false, false, EditorAction::SelectAll);
        editor_hit("c", true, true, false, EditorAction::Copy);
        editor_hit("x", true, false, true, EditorAction::Cut);
        editor_hit("v", true, true, true, EditorAction::Paste);
        // 无 ctrl 不命中（`_` 分支可打印插入）。
        editor_miss("a", false, false, false);
        editor_miss("c", false, false, false);
    }

    /// E21-E23：undo/redo 双键位——约束度排序复现 match 臂序。
    #[test]
    fn editor_undo_redo_disambiguation() {
        editor_hit("z", true, true, false, EditorAction::Redo);
        editor_hit("z", true, true, true, EditorAction::Redo); // alt Any
        editor_hit("z", true, false, false, EditorAction::Undo);
        editor_hit("y", true, false, false, EditorAction::RedoY);
        editor_hit("y", true, true, false, EditorAction::RedoY);
        editor_miss("z", false, false, false);
    }

    /// E24-E28：加同词光标 / Alt+F 折叠 / 折叠快捷键 / Ctrl+F 查找。
    #[test]
    fn editor_fold_word_match_and_find() {
        editor_hit("d", true, false, false, EditorAction::AddNextWordMatch);
        // E25：Alt+F（alt 守卫——ctrl/shift 任意；臂序在 "f" if ctrl 前，
        // ctrl+alt+f 同走 ToggleFold——现状 match 臂序精确复现）。
        editor_hit("f", false, false, true, EditorAction::ToggleFold);
        editor_hit("f", true, false, true, EditorAction::ToggleFold);
        editor_hit("f", false, true, true, EditorAction::ToggleFold);
        // E28：Ctrl+F 查找（无 alt）。
        editor_hit("f", true, false, false, EditorAction::OpenFind);
        editor_hit("f", true, true, false, EditorAction::OpenFind);
        editor_hit("[", true, true, false, EditorAction::FoldCurrent);
        editor_hit("[", true, true, true, EditorAction::FoldCurrent);
        editor_hit("]", true, true, false, EditorAction::UnfoldCurrent);
        editor_miss("[", false, false, false);
        editor_miss("[", true, false, false); // 无 shift 不命中
    }

    /// E29-E30：替换 / Ctrl+S·Ctrl+Shift+S 空臂放行（#386：保存分流
    /// 归 shell 域——两键位全冒泡）。
    #[test]
    fn editor_replace_save_pass() {
        editor_hit("h", true, false, false, EditorAction::OpenReplace);
        editor_hit("h", true, true, true, EditorAction::OpenReplace);
        // #386：Ctrl+S 不再由编辑器自处理（保存分流归 Shell）——
        // 整键空臂放行冒泡（shell 域 ShellAction::Save 接管）。
        editor_hit("s", true, false, false, EditorAction::Pass);
        editor_hit("s", true, false, true, EditorAction::Pass);
        editor_hit("s", true, true, false, EditorAction::Pass); // 冒泡 shell
        editor_hit("s", true, true, true, EditorAction::Pass);
        editor_miss("s", false, false, false);
    }

    /// E0：前置层 palette toggle（查表点在 palette 模态之前）。
    #[test]
    fn editor_palette_toggle_pre_layer() {
        let store = KeymapStore::default_tables();
        assert_eq!(
            store.resolve_pre(Site::EditorPalette, &q("p", true, true, false)),
            Some(Action::Editor(EditorAction::PaletteToggle))
        );
        // alt Any：Ctrl+Shift+Alt+P 同样命中（现状 `ctrl && shift` 守卫）。
        assert_eq!(
            store.resolve_pre(Site::EditorPalette, &q("p", true, true, true)),
            Some(Action::Editor(EditorAction::PaletteToggle))
        );
        assert_eq!(
            store.resolve_pre(Site::EditorPalette, &q("p", true, false, false)),
            None
        );
        assert_eq!(
            store.resolve_pre(Site::EditorPalette, &q("p", false, true, false)),
            None
        );
    }

    /// E1：前置层 goto 开框（!shift 守卫；is_none 条件在执行层）。
    #[test]
    fn editor_goto_open_pre_layer() {
        let store = KeymapStore::default_tables();
        assert_eq!(
            store.resolve_pre(Site::EditorGoto, &q("g", true, false, false)),
            Some(Action::Editor(EditorAction::GotoOpen))
        );
        assert_eq!(
            store.resolve_pre(Site::EditorGoto, &q("g", true, false, true)),
            Some(Action::Editor(EditorAction::GotoOpen)) // alt Any
        );
        assert_eq!(
            store.resolve_pre(Site::EditorGoto, &q("g", true, true, false)),
            None
        );
        assert_eq!(
            store.resolve_pre(Site::EditorGoto, &q("g", false, false, false)),
            None
        );
    }

    /// E2/E3/E32：编辑器 chord 全流程——挂起/续接/作废转新起点/刷新
    /// （对齐 `ctrl_k_pending` 状态机）。
    #[test]
    fn editor_chord_full_lifecycle() {
        let store = KeymapStore::default_tables();
        // 首键挂起（k 臂提前 return——接线层存本键快照）。
        let out = store.resolve_editor_main(&q("k", true, false, false), None);
        assert_eq!(out, EditorOutcome::Pending, "Ctrl+K 应挂起，实得 {out:?}");
        let pending = q("k", true, false, false);
        // 挂起中 0 → 全部折叠（续接键修饰 Any——Ctrl+K Ctrl+0 亦触发，
        // 现状怪癖照抄）。
        for (ctrl, shift) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(
                store.resolve_editor_main(&q("0", ctrl, shift, false), Some(&pending)),
                EditorOutcome::Matched(EditorAction::FoldAll),
                "挂起中 0（ctrl={ctrl} shift={shift}）应全部折叠"
            );
            assert_eq!(
                store.resolve_editor_main(&q("j", ctrl, shift, false), Some(&pending)),
                EditorOutcome::Matched(EditorAction::UnfoldAll)
            );
        }
        // 挂起中 Ctrl+K → 刷新挂起（前缀作废后单键再命中首键）。
        assert_eq!(
            store.resolve_editor_main(&q("k", true, false, false), Some(&pending)),
            EditorOutcome::Pending
        );
        // 挂起中续接落空 → 挂起终结 + 该键当新起点（不吞键）：
        // Ctrl+F 照常打开查找。
        assert_eq!(
            store.resolve_editor_main(&q("f", true, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::OpenFind)
        );
        // 挂起中可打印字符 x → 无命中（接线层走 `_` 插入）。
        assert_eq!(
            store.resolve_editor_main(&q("x", false, false, false), Some(&pending)),
            EditorOutcome::None
        );
        // 无挂起时 0/J 是普通键（Ctrl+0 冒泡 shell 字号重置）。
        assert_eq!(
            store.resolve_editor_main(&q("0", true, false, false), None),
            EditorOutcome::None
        );
        editor_miss("j", false, false, false);
        // Shift+K 不挂起（!shift 守卫）。
        assert_eq!(
            store.resolve_editor_main(&q("k", true, true, false), None),
            EditorOutcome::None
        );
        // Alt+K 挂起（alt Any——现状守卫不查 alt）。
        assert_eq!(
            store.resolve_editor_main(&q("k", true, false, true), None),
            EditorOutcome::Pending
        );
    }

    // ─── shell 层逐臂对照（S1-S11） ─────────────────────────────────

    /// shell 全部单键分支（chord 除外）。
    #[test]
    fn shell_single_key_branches() {
        shell_hit("tab", true, false, false, ShellAction::NextTab);
        shell_hit("tab", true, true, false, ShellAction::PrevTab);
        shell_miss("tab", true, false, true); // alt 守卫
        shell_miss("tab", false, false, false); // 无 ctrl → 编辑器缩进
        shell_hit("w", true, false, false, ShellAction::CloseTab);
        shell_miss("w", true, true, false); // plain_ctrl
        shell_miss("w", true, false, true);
        // #386 S12：Ctrl+S 保存分流（编辑器 E30 旧键位移除后冒泡至此）。
        shell_hit("s", true, false, false, ShellAction::Save);
        shell_miss("s", true, false, true); // plain（!alt）守卫
        shell_miss("s", false, false, false); // 无 ctrl → 可打印字符
        shell_hit("s", true, true, false, ShellAction::PromptSaveAs);
        shell_miss("s", true, true, true); // alt 守卫（S5 带 !alt）
        shell_hit("=", true, false, false, ShellAction::FontBigger);
        shell_hit("=", true, true, false, ShellAction::FontBigger); // shift Any
        shell_miss("=", true, true, true); // alt 守卫（S6/S7/S8 带 !alt）
        shell_hit("+", true, true, false, ShellAction::FontBigger);
        shell_hit("-", true, false, false, ShellAction::FontSmaller);
        shell_hit("0", true, false, false, ShellAction::FontReset);
        shell_miss("0", true, true, false); // plain_ctrl
        shell_hit("t", true, false, false, ShellAction::CycleTheme);
        shell_hit("o", true, false, false, ShellAction::PromptOpenFile);
        shell_miss("o", true, true, false);
        shell_miss("o", false, false, false);
    }

    /// S1：shell chord 全流程——挂起/续接/过期作废/作废后继续分支
    /// （对齐 `chord_until` 状态机）。
    #[test]
    fn shell_chord_full_lifecycle() {
        let store = KeymapStore::default_tables();
        let now = Instant::now();
        // 首键挂起（接线层存快照 + deadline）。
        let pending = q("k", true, false, false);
        assert_eq!(
            store.resolve_shell(&pending, None, now),
            ShellOutcome::Pending,
            "shell Ctrl+K 应挂起"
        );
        // 窗口期内 Ctrl+O → 打开工作区（吞掉，不落 Ctrl+O 打开文件）。
        let deadline = now + std::time::Duration::from_secs(2);
        assert_eq!(
            store.resolve_shell(
                &q("o", true, false, false),
                Some(&(pending.clone(), deadline)),
                now
            ),
            ShellOutcome::Matched(ShellAction::PromptOpenWorkspace)
        );
        // 过期 → 作废不吞键：Ctrl+O 落单键分支 = 打开文件。
        let expired = now - std::time::Duration::from_secs(1);
        assert_eq!(
            store.resolve_shell(
                &q("o", true, false, false),
                Some(&(pending.clone(), expired)),
                now
            ),
            ShellOutcome::Matched(ShellAction::PromptOpenFile)
        );
        // 窗口期内续接落空（Ctrl+W）→ 作废 + 该键继续后续分支 = 关标签
        // （现状「过期/非 O → 和弦作废」后分支序不变）。
        assert_eq!(
            store.resolve_shell(
                &q("w", true, false, false),
                Some(&(pending.clone(), deadline)),
                now
            ),
            ShellOutcome::Matched(ShellAction::CloseTab)
        );
        // 挂起中普通字符 → 无 shell 命中。
        assert_eq!(
            store.resolve_shell(
                &q("x", false, false, false),
                Some(&(pending.clone(), deadline)),
                now
            ),
            ShellOutcome::None
        );
        // 挂起中再按 Ctrl+K → 刷新挂起。
        assert_eq!(
            store.resolve_shell(&q("k", true, false, false), Some(&(pending, deadline)), now),
            ShellOutcome::Pending
        );
    }

    // ─── 键串解析 ───────────────────────────────────────────────────

    /// 解析全形态（含 "+" 键名、大小写、chord、错误）。
    #[test]
    fn parse_binding_forms() {
        use Binding::*;
        assert_eq!(
            parse_binding("ctrl+k ctrl+o").unwrap(),
            Chord(
                KeyPattern::key("k").with(Yes, Any, Any),
                KeyPattern::key("o").with(Yes, Any, Any)
            )
        );
        assert_eq!(
            parse_binding("ctrl+shift+[").unwrap(),
            Single(KeyPattern::key("[").with(Yes, Yes, Any))
        );
        // "+" 键名（前缀扫描不误拆）。
        assert_eq!(
            parse_binding("ctrl++").unwrap(),
            Single(KeyPattern::key("+").with(Yes, Any, Any))
        );
        assert_eq!(
            parse_binding("ctrl+-").unwrap(),
            Single(KeyPattern::key("-").with(Yes, Any, Any))
        );
        // 大小写不敏感 + 修饰序任意。
        assert_eq!(
            parse_binding("SHIFT+Ctrl+A").unwrap(),
            Single(KeyPattern::key("a").with(Yes, Yes, Any))
        );
        assert_eq!(
            parse_binding("alt+f").unwrap(),
            Single(KeyPattern::key("f").with(Any, Any, Yes))
        );
        // 错误形态。
        assert!(parse_binding("").is_err());
        assert!(parse_binding("ctrl+k ctrl+o ctrl+j").is_err());
        assert!(parse_binding("ctrl+").is_err());
        assert!(parse_binding("ctrl+ctrl+k").is_err());
        // 不支持的修饰词（meta/cmd/…）显式报错——幻影键防线
        // （#218 review），逐词面见 parse_pattern_rejects_phantom_modifiers。
        assert!(parse_binding("meta+k").is_err());
    }

    /// 幻影修饰词防线（#218 review）：已知但不支持的修饰词前缀、
    /// 混入支持修饰词后的不支持词、含 `+` 的未知词、裸修饰词作键名
    /// ——全部 Err（dispatch 永不产生这些键名）；既有合法形态（含
    /// "+" 键名的 `ctrl++`）回归钉定不受影响。
    #[test]
    fn parse_pattern_rejects_phantom_modifiers() {
        for bad in [
            // 已知但不支持的修饰词前缀（大小写不敏感）。
            "meta+k",
            "Meta+K",
            "META+k",
            "cmd+s",
            "command+s",
            "win+e",
            "windows+d",
            "super+x",
            "option+v",
            "altgr+n",
            "control+k",
            "Control+K",
            // 混入支持修饰词后（扫描逐轮检查——第二段照样抓）。
            "ctrl+meta+k",
            "ctrl+alt+cmd+k",
            "shift+win+e",
            // 未知词带 "+"（"foo+bar" 类）。
            "foo+bar",
            "ctrl+foo+bar",
            // 裸修饰词单独作键名。
            "meta",
            "Meta",
            "cmd",
            "ctrl",
            "Ctrl",
            "control",
            "shift",
            "alt",
            "Alt",
        ] {
            assert!(parse_binding(bad).is_err(), "\"{bad}\" 应报错（幻影键）");
        }
        // 既有合法形态回归钉定（"+" 键名必须继续合法——前缀扫描不误拆）。
        for ok in [
            "ctrl++",
            "ctrl+-",
            "ctrl+shift+[",
            "SHIFT+Ctrl+A",
            "alt+f",
            "ctrl+k ctrl+o",
            "ctrl+alt+home",
        ] {
            assert!(parse_binding(ok).is_ok(), "\"{ok}\" 应保持合法");
        }
        // 错误文案可读（警告直出 stderr 的口径）。
        let err = parse_binding("meta+k").unwrap_err().0;
        assert!(
            err.contains("meta") && err.contains("ctrl/shift/alt"),
            "{err}"
        );
        let err = parse_binding("meta").unwrap_err().0;
        assert!(
            err.contains("meta") && err.contains("不能单独作键名"),
            "{err}"
        );
    }

    /// 幻影键换绑的关键回归（#218 review 原例）：bind "meta+k" → save
    /// 被拒且 **ctrl+s 默认绑定完好**（解析失败发生在移除旧绑定之前
    /// ——apply 先 parse 后变异，结构性保证）；unbind 幻影键同样被拒
    /// 且零移除。
    #[test]
    fn phantom_modifier_bind_rejected_keeps_default_save() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[
            UserOverride::Bind {
                bind: "meta+k".into(),
                action: "save".into(),
            },
            UserOverride::Unbind {
                bind: "meta+k".into(),
            },
        ]);
        assert_eq!(warnings.len(), 2, "两条都应被拒：{warnings:?}");
        assert!(
            warnings[0].contains("不支持的修饰词「meta」"),
            "bind 拒绝文案应指明修饰词：{}",
            warnings[0]
        );
        assert!(
            warnings[1].contains("不支持的修饰词「meta」"),
            "unbind 拒绝文案应指明修饰词：{}",
            warnings[1]
        );
        // #386：ctrl+s 默认形态 = 空臂放行冒泡（保存分流归 shell 域
        // ——E30 旧键位已移除；meta+k 被拒不得影响 s 键族默认形态）。
        assert_eq!(
            store.resolve_editor_main(&q("s", true, false, false), None),
            EditorOutcome::Matched(EditorAction::Pass),
            "meta+k 被拒不得影响 ctrl+s 默认（空臂放行）"
        );
        // 全表零移除：默认 chord 完好（unbind 幻影键零副作用）。
        let pending = q("k", true, false, false);
        assert_eq!(
            store.resolve_editor_main(&q("0", false, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::FoldAll)
        );
    }

    // ─── 标签生成（menu_bar 字面量对拍——格式零漂移） ───────────────

    /// 默认表标签 = menu_bar 既有 16 对标签字面量（单一事实源转移，
    /// 值零漂移——edit-find 与 view-side-search 共用 open_find 的标签）。
    #[test]
    fn default_labels_match_menu_bar_literals() {
        let store = KeymapStore::default_tables();
        let pinned: &[(&str, &str)] = &[
            ("prompt_open_file", "Ctrl+O"),
            ("prompt_open_workspace", "Ctrl+K Ctrl+O"),
            ("save", "Ctrl+S"),
            ("prompt_save_as", "Ctrl+Shift+S"),
            ("undo", "Ctrl+Z"),
            ("redo", "Ctrl+Shift+Z"),
            ("cut", "Ctrl+X"),
            ("copy", "Ctrl+C"),
            ("paste", "Ctrl+V"),
            ("open_find", "Ctrl+F"),
            ("open_replace", "Ctrl+H"),
            ("toggle_command_palette", "Ctrl+Shift+P"),
            ("font_bigger", "Ctrl+="),
            ("font_smaller", "Ctrl+-"),
            ("font_reset", "Ctrl+0"),
        ];
        for (id, label) in pinned {
            assert_eq!(
                store.label(id).as_deref(),
                Some(*label),
                "{id} 标签漂移（menu_bar 字面量对拍）"
            );
        }
        // redo 第二键位独立标签（表序：主键位在前）。
        assert_eq!(store.label("redo_y").as_deref(), Some("Ctrl+Y"));
        // 折叠/跳行家族标签格式（Cmd::default_key 同款口径对拍）。
        assert_eq!(store.label("fold_all").as_deref(), Some("Ctrl+K Ctrl+0"));
        assert_eq!(store.label("unfold_all").as_deref(), Some("Ctrl+K Ctrl+J"));
        assert_eq!(store.label("toggle_fold").as_deref(), Some("Alt+F"));
        assert_eq!(store.label("fold_current").as_deref(), Some("Ctrl+Shift+["));
        assert_eq!(
            store.label("unfold_current").as_deref(),
            Some("Ctrl+Shift+]")
        );
        assert_eq!(store.label("goto_line").as_deref(), Some("Ctrl+G"));
    }

    // ─── 用户覆盖（换绑/解绑/冲突——#217 验收面） ───────────────────

    /// 换绑生效：新键触发原动作、旧键不再触发（issue 验收标准）。
    #[test]
    fn bind_replaces_old_binding() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+alt+p".into(),
            action: "toggle_command_palette".into(),
        }]);
        assert!(warnings.is_empty(), "无冲突换绑不应警告：{warnings:?}");
        // 新键触发原动作（前置层查表点）。
        assert_eq!(
            store.resolve_pre(Site::EditorPalette, &q("p", true, true, true)),
            Some(Action::Editor(EditorAction::PaletteToggle))
        );
        // 旧键不再触发。
        assert_eq!(
            store.resolve_pre(Site::EditorPalette, &q("p", true, true, false)),
            None
        );
        // 标签跟随新绑定（menu_bar 单一事实源）。
        assert_eq!(
            store.label("toggle_command_palette").as_deref(),
            Some("Ctrl+Alt+P")
        );
    }

    /// 同键换动作（pattern 相等 → 替换——覆盖默认表最直觉路径）。
    #[test]
    fn bind_same_key_replaces_action() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+f".into(),
            // #386：编辑器保存动作改名 editor_save（"save" 归 shell
            // 保存分流——本测试钉定编辑器动作仍可同键替换）。
            action: "editor_save".into(),
        }]);
        assert!(warnings.is_empty(), "同键替换应放行：{warnings:?}");
        // ctrl+f → save（替换了 open_find）。
        assert_eq!(
            store.resolve_editor_main(&q("f", true, false, false), None),
            EditorOutcome::Matched(EditorAction::Save)
        );
        // open_find 无绑定（标签消失）。
        assert_eq!(store.label("open_find"), None);
        // save 原键 ctrl+s 不再保存——E31 Pass 接管该键（不消费冒泡，
        // 接线面与 None 同效）。
        assert_eq!(
            store.resolve_editor_main(&q("s", true, false, false), None),
            EditorOutcome::Matched(EditorAction::Pass)
        );
    }

    /// 冲突拒绝：同域相交且不更精确（新条目永不触发）——拒绝时动作
    /// **保持既有绑定**（先检测后改）。
    ///
    /// #386 后冲突对换型：旧例（ctrl+alt+s × 编辑器 Save ctrl+!shift+s）
    /// 随 E30 移除失效（编辑器表再无 No-约束 2 约束条目，同精度不相
    /// 容相交在编辑器域不可构造）。现用 shell 域同构例：S8 `-`(ctrl,!alt)
    /// × 新 ctrl+shift+-（同为 2 约束、互不覆盖——canonical 双命中，
    /// 表序早者 S8 赢）→ 拒绝。
    #[test]
    fn bind_conflicting_key_rejected() {
        let mut store = KeymapStore::default_tables();
        // font_reset 换绑到 ctrl+shift+-（2 约束）：S8 FontSmaller
        // -（ctrl,!alt）同为 2 约束、互不覆盖，canonical 双命中 →
        // 表序早者 S8 赢 → 新条目永不触发 → 拒绝。
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+shift+-".into(),
            action: "font_reset".into(),
        }]);
        assert_eq!(warnings.len(), 1, "应恰好一条警告：{warnings:?}");
        assert!(
            warnings[0].contains("冲突"),
            "警告应说明冲突：{}",
            warnings[0]
        );
        // 拒绝 = 该条完全不生效：font_reset 保持 ctrl+0 原绑、S8
        // FontSmaller 原样（断言直查本 store，不用 shell_hit——它自
        // 建默认表，验不到拒绑后状态）。
        assert_eq!(
            store.resolve_shell(&q("0", true, false, false), None, Instant::now()),
            ShellOutcome::Matched(ShellAction::FontReset),
            "拒绑后 font_reset 原绑不动"
        );
        assert_eq!(
            store.resolve_shell(&q("-", true, true, false), None, Instant::now()),
            ShellOutcome::Matched(ShellAction::FontSmaller),
            "拒绑后 S8 原样（ctrl+shift+- 仍 FontSmaller——S8 的 shift Any）"
        );
        // save 不受影响（#386：shell 域 S12 ctrl+s 保存分流——与拒绑
        // 前同形态）。
        assert_eq!(
            store.resolve_shell(&q("s", true, false, false), None, Instant::now()),
            ShellOutcome::Matched(ShellAction::Save),
            "S12 保存分流不受拒绑影响"
        );
    }

    /// 跨域冲突拒绝：同键两域叠加执行（编辑器不拦冒泡）。
    #[test]
    fn bind_cross_domain_conflict_rejected() {
        let mut store = KeymapStore::default_tables();
        // ctrl+o 已被 shell prompt_open_file 占用——编辑器动作 bind 上去
        // 会被拒绝（#386：编辑器保存动作改名 editor_save）。
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+o".into(),
            action: "editor_save".into(),
        }]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("shell"),
            "警告应指明跨域：{}",
            warnings[0]
        );
        // shell 的 ctrl+o 不受影响；#386：save 默认无键位（ctrl+s
        // 空臂放行——保存分流归 shell 域）。
        shell_hit("o", true, false, false, ShellAction::PromptOpenFile);
        assert_eq!(
            store.resolve_editor_main(&q("s", true, false, false), None),
            EditorOutcome::Matched(EditorAction::Pass)
        );
    }

    /// 精确度豁免：更精确的用户条目与家族条目并存（alt+left 叠在
    /// 方向键家族的 Any 条目上——运行时精确者胜）。
    #[test]
    fn bind_more_specific_coexists_with_family() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "alt+left".into(),
            action: "open_replace".into(),
        }]);
        assert!(warnings.is_empty(), "更精确条目应放行：{warnings:?}");
        // alt+left → 新动作（约束数 1 > 家族 0）。
        assert_eq!(
            store.resolve_editor_main(&q("left", false, false, true), None),
            EditorOutcome::Matched(EditorAction::OpenReplace)
        );
        // 裸方向键/ctrl/shift 组合仍走家族（执行层移动语义不变）。
        assert_eq!(
            store.resolve_editor_main(&q("left", false, false, false), None),
            EditorOutcome::Matched(EditorAction::MoveKey(MoveDir::Left))
        );
        assert_eq!(
            store.resolve_editor_main(&q("left", true, true, false), None),
            EditorOutcome::Matched(EditorAction::MoveKey(MoveDir::Left))
        );
    }

    /// chord 跨域冲突：续接键相同的两域 chord（Ctrl+K Ctrl+O 双执行）
    /// 被拒绝。
    #[test]
    fn bind_chord_cross_domain_rejected() {
        let mut store = KeymapStore::default_tables();
        // 编辑器 bind [ctrl+k ctrl+o]→editor_save：与 shell S1 [ctrl+k
        // ctrl+o] 首键相交且续接键相交 → 跨域拒绝。
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+k ctrl+o".into(),
            action: "editor_save".into(),
        }]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("叠加执行"), "{}", warnings[0]);
    }

    /// 同域新 chord 与默认 chord 续接分流并存（[ctrl+k x] 与 [ctrl+k 0]）。
    #[test]
    fn bind_new_chord_coexists_by_followup() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+k x".into(),
            action: "open_replace".into(),
        }]);
        assert!(warnings.is_empty(), "续接分流应放行：{warnings:?}");
        // 挂起快照 = 默认首键（Ctrl+K 竞争中约束数 2 胜过用户首键 1）。
        let pending = q("k", true, false, false);
        // 续接 x → 新动作（用户和弦首键 Any 变体经快照可达）。
        assert_eq!(
            store.resolve_editor_main(&q("x", true, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::OpenReplace)
        );
        // 默认续接 0/j 不受影响。
        assert_eq!(
            store.resolve_editor_main(&q("0", false, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::FoldAll)
        );
    }

    /// 解绑：移除该键实际触发的绑定（chord 写全两键；拼写修饰宽于
    /// 默认守卫也能命中——按 canonical 按键解析赢家）。
    #[test]
    fn unbind_removes_exact_binding() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[UserOverride::Unbind {
            bind: "ctrl+k ctrl+0".into(),
        }]);
        assert!(warnings.is_empty(), "{warnings:?}");
        // ctrl+k 仍挂起（ctrl+k ctrl+j 还在）。
        assert_eq!(
            store.resolve_editor_main(&q("k", true, false, false), None),
            EditorOutcome::Pending
        );
        // 挂起后 0 不再折叠——当新起点（无单键 0 绑定 → None）。
        let pending = q("k", true, false, false);
        assert_eq!(
            store.resolve_editor_main(&q("0", false, false, false), Some(&pending)),
            EditorOutcome::None
        );
        // j 仍触发全部展开。
        assert_eq!(
            store.resolve_editor_main(&q("j", false, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::UnfoldAll)
        );
        // 无此绑定 → 警告。
        let warnings = store.apply_user(&[UserOverride::Unbind {
            bind: "ctrl+f9".into(),
        }]);
        assert_eq!(warnings.len(), 1);
    }

    /// chord 解绑赢家镜像 dispatch（#218 review 原例）：表同时有
    /// `ctrl+shift+k x` 与 `ctrl+k ctrl+x` 时，序列（ctrl+shift+k,
    /// ctrl+x）dispatch 触发**后者**（续接尾键精确度 1>0）——
    /// unbind "ctrl+shift+k ctrl+x" 必须恰移除后者（原 `(首键, 尾键)`
    /// 字典序按头优先移除了前者——dispatch 不会触发的那条），另一条
    /// 完好。
    #[test]
    fn unbind_chord_winner_mirrors_dispatch() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[
            UserOverride::Bind {
                bind: "ctrl+shift+k x".into(),
                action: "open_replace".into(),
            },
            UserOverride::Bind {
                bind: "ctrl+k ctrl+x".into(),
                action: "open_find".into(),
            },
        ]);
        assert!(warnings.is_empty(), "两条 chord 换绑均应放行：{warnings:?}");
        // dispatch 前提对拍：序列（ctrl+shift+k, ctrl+x）触发 open_find
        // 的 chord（首键竞争 ctrl+shift+k 头 spec 2 胜出挂起；续接
        // ctrl+x 尾 spec 1 > open_replace 裸 x 尾 spec 0）。
        let first = q("k", true, true, false);
        assert_eq!(
            store.resolve_editor_main(&first, None),
            EditorOutcome::Pending
        );
        assert_eq!(
            store.resolve_editor_main(&q("x", true, false, false), Some(&first)),
            EditorOutcome::Matched(EditorAction::OpenFind),
            "dispatch 应触发 ctrl+k ctrl+x（尾键精确者胜）"
        );
        // 反向序列（ctrl+shift+k, 裸 x）触发 open_replace 的 chord。
        assert_eq!(
            store.resolve_editor_main(&q("x", false, false, false), Some(&first)),
            EditorOutcome::Matched(EditorAction::OpenReplace)
        );
        // 解绑 dispatch 会触发的那条（review 原例的键串）。
        let warnings = store.apply_user(&[UserOverride::Unbind {
            bind: "ctrl+shift+k ctrl+x".into(),
        }]);
        assert!(warnings.is_empty(), "应恰命中一条移除：{warnings:?}");
        // 另一条完好：同序列现在触发 open_replace 的 chord（若移错了
        // 条，此解析会变 None/Cut——旧字典序赢家恰在此暴露）。
        assert_eq!(
            store.resolve_editor_main(&q("x", true, false, false), Some(&first)),
            EditorOutcome::Matched(EditorAction::OpenReplace),
            "解绑后应由幸存的 ctrl+shift+k x 接管该序列"
        );
        assert_eq!(
            store.resolve_editor_main(&q("x", false, false, false), Some(&first)),
            EditorOutcome::Matched(EditorAction::OpenReplace)
        );
        // 被解绑的 chord 不再触发：ctrl+k 挂起后续接 ctrl+x 落空 →
        // 本键当新起点（ctrl+x = Cut——挂起终结语义）。
        let pending = q("k", true, false, false);
        assert_eq!(
            store.resolve_editor_main(&q("x", true, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::Cut)
        );
    }

    /// 反向序列对称用例：unbind "ctrl+k ctrl+x" 恰移除 dispatch 在
    /// （ctrl+k, ctrl+x）序列上触发的那条；ctrl+shift+k x 不受影响。
    #[test]
    fn unbind_chord_reverse_sequence_symmetric() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[
            UserOverride::Bind {
                bind: "ctrl+shift+k x".into(),
                action: "open_replace".into(),
            },
            UserOverride::Bind {
                bind: "ctrl+k ctrl+x".into(),
                action: "open_find".into(),
            },
        ]);
        assert!(warnings.is_empty(), "{warnings:?}");
        let pending = q("k", true, false, false);
        // dispatch 前提：序列（ctrl+k, ctrl+x）触发 open_find 的 chord
        //（首键默认 chord 头 spec 2 胜出挂起；续接只它匹配 ctrl+x）。
        assert_eq!(
            store.resolve_editor_main(&q("x", true, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::OpenFind)
        );
        // 解绑该序列实际触发的绑定。
        let warnings = store.apply_user(&[UserOverride::Unbind {
            bind: "ctrl+k ctrl+x".into(),
        }]);
        assert!(warnings.is_empty(), "{warnings:?}");
        // 同序列不再触发 open_find：续接落空 → 本键当新起点（Cut）。
        assert_eq!(
            store.resolve_editor_main(&q("x", true, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::Cut)
        );
        // open_find 无绑定（默认 ctrl+f 已在换绑时让位，chord 又被解绑）。
        assert_eq!(store.label("open_find"), None);
        // 另一条不受影响：序列（ctrl+shift+k, x）照常触发 open_replace。
        let first = q("k", true, true, false);
        assert_eq!(
            store.resolve_editor_main(&q("x", false, false, false), Some(&first)),
            EditorOutcome::Matched(EditorAction::OpenReplace)
        );
    }

    /// 单键 bind 被 chord 首键抢赢 → 拒绝（原实现 canon[1] 越界
    /// panic——单键绑定无第二键；#218 review 附加审查）。dispatch 语义
    /// 对拍：ctrl+k 的赢家是 chord 首（spec 2 > 新单键 1）→ 恒 Pending，
    /// 新单键绑定永不触发 → canonical 赢家策略拒绝，既有绑定保持。
    #[test]
    fn bind_single_key_stolen_by_chord_head_rejected() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+k".into(),
            action: "fold_all".into(),
        }]);
        assert_eq!(warnings.len(), 1, "应恰好一条冲突警告：{warnings:?}");
        assert!(
            warnings[0].contains("冲突") && warnings[0].contains("unfold_all"),
            "警告应指明抢占者（Ctrl+K Ctrl+J → unfold_all）：{}",
            warnings[0]
        );
        // 拒绝 = 该条完全不生效：默认 chord 全部保持。
        assert_eq!(
            store.resolve_editor_main(&q("k", true, false, false), None),
            EditorOutcome::Pending
        );
        let pending = q("k", true, false, false);
        assert_eq!(
            store.resolve_editor_main(&q("0", false, false, false), Some(&pending)),
            EditorOutcome::Matched(EditorAction::FoldAll),
            "fold_all 默认 chord 完好（拒绝不落任何变更）"
        );
    }

    /// 格式错误：未知动作 / 坏键串——单条拒绝 + 可读警告，其余生效。
    #[test]
    fn malformed_overrides_rejected_with_readable_warnings() {
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[
            UserOverride::Bind {
                bind: "ctrl+alt+f12".into(),
                action: "no_such_action".into(),
            },
            UserOverride::Bind {
                bind: "bad key str".into(), // 空格分 chord → 三段超限
                action: "save".into(),
            },
            UserOverride::Bind {
                bind: "alt+home".into(),
                action: "open_find".into(),
            },
        ]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("未知动作"));
        assert!(warnings[1].contains("两键"));
        // 第三条好配置照常生效（alt+home 更精确于 home 家族 Any 条目
        // ——canonical 赢家检测放行，运行时精确者胜）。
        assert_eq!(
            store.resolve_editor_main(&q("home", false, false, true), None),
            EditorOutcome::Matched(EditorAction::OpenFind)
        );
    }

    /// 覆盖后的表通过冲突自检（check_conflicts 与 apply_user 策略一致）。
    #[test]
    fn store_after_overrides_still_conflict_checked() {
        let mut store = KeymapStore::default_tables();
        store.apply_user(&[
            UserOverride::Bind {
                bind: "ctrl+alt+p".into(),
                action: "toggle_command_palette".into(),
            },
            UserOverride::Bind {
                bind: "ctrl+alt+left".into(),
                action: "open_replace".into(),
            },
        ]);
        assert!(
            store.check_conflicts().is_empty(),
            "合法覆盖后不应有歧义：{:?}",
            store.check_conflicts()
        );
    }

    // ─── 只读门禁（动作级兜底——#218 review 修复） ─────────────────

    /// readonly_allows 镜像夹具（**镜像自 docx_doc.rs:1101，两处需同步
    /// 改**——keymap 层纯 std 红线不依赖 docx_doc，单测用誊抄副本对拍；
    /// docx_doc 侧有同名单测钉定其自身）。
    fn readonly_allows_mirror(key: &str, ctrl: bool) -> bool {
        match key {
            "left" | "right" | "up" | "down" | "home" | "end" | "pageup" | "pagedown"
            | "escape" => true,
            "a" | "c" | "f" | "tab" | "=" | "+" | "-" | "0" | "t" | "g" => ctrl,
            _ => false,
        }
    }

    /// 分类表逐变体钉定（29 变体全覆盖——接线层兜底的判定面）。
    #[test]
    fn readonly_safe_classification_pinned() {
        let safe: Vec<EditorAction> = vec![
            EditorAction::PaletteToggle,
            EditorAction::GotoOpen,
            EditorAction::MoveKey(MoveDir::Left),
            EditorAction::MoveKey(MoveDir::Right),
            EditorAction::MoveKey(MoveDir::Up),
            EditorAction::MoveKey(MoveDir::Down),
            EditorAction::HomeKey,
            EditorAction::EndKey,
            EditorAction::PageUpKey,
            EditorAction::PageDownKey,
            EditorAction::EscapeClearSelection,
            EditorAction::SelectAll,
            EditorAction::Copy,
            EditorAction::OpenFind,
            EditorAction::Pass,
        ];
        let unsafe_: Vec<EditorAction> = vec![
            EditorAction::Newline,
            EditorAction::Backspace,
            EditorAction::DeleteForward,
            EditorAction::Indent,
            EditorAction::Cut,
            EditorAction::Paste,
            EditorAction::Redo,
            EditorAction::Undo,
            EditorAction::RedoY,
            EditorAction::AddNextWordMatch,
            EditorAction::Save,
            EditorAction::OpenReplace,
            EditorAction::ToggleFold,
            EditorAction::FoldCurrent,
            EditorAction::UnfoldCurrent,
            EditorAction::FoldAll,
            EditorAction::UnfoldAll,
        ];
        for a in &safe {
            assert!(a.readonly_safe(), "{a:?} 应为只读安全（浏览类）");
        }
        for a in &unsafe_ {
            assert!(!a.readonly_safe(), "{a:?} 应为只读不安全（变更类）");
        }
        // 全集对账：EDITOR_ACTION_IDS 29 变体恰好二分（新增变体漏分类
        // 即在此爆——分类表不得有漏网）。
        assert_eq!(
            safe.len() + unsafe_.len(),
            EDITOR_ACTION_IDS.len(),
            "分类表必须覆盖全部变体"
        );
        for a in EDITOR_ACTION_IDS {
            let classified = safe.contains(a) || unsafe_.contains(a);
            assert!(classified, "{}（{:?}）未入分类表", a.id(), a);
        }
    }

    /// 默认表不变量：凡 canonical 键能过只读物理守卫（readonly_allows
    /// 白名单）的条目，其动作必须 readonly_safe——证明动作级兜底对
    /// **默认表**零行为变化（两层守卫在默认键位上从不冲突：物理守卫
    /// 放行的键解析出的动作，兜底层必然放行）。
    #[test]
    fn default_table_readonly_guard_invariant() {
        let store = KeymapStore::default_tables();
        for (entries, _) in store.domains() {
            for e in entries {
                let Action::Editor(a) = e.action else {
                    continue; // shell 域动作不经编辑器只读门禁
                };
                let canon = canonical_queries(&e.binding);
                let passes = match canon.as_slice() {
                    // 单键：物理守卫放行该键 → 动作必须 safe。
                    [c1] => readonly_allows_mirror(&c1.key, c1.ctrl),
                    // chord：首键过守卫（挂起）且尾键过守卫（续接执行）
                    // → 动作必须 safe（默认表首键 ctrl+k 不过白名单，
                    // 恒 false——断言对默认表恒空转，用户换绑后生效）。
                    [c1, c2] => {
                        readonly_allows_mirror(&c1.key, c1.ctrl)
                            && readonly_allows_mirror(&c2.key, c2.ctrl)
                    }
                    _ => false,
                };
                if passes {
                    assert!(
                        a.readonly_safe(),
                        "默认条目 \"{}\" → {} 过物理守卫但动作非只读安全（默认行为漂移）",
                        label_binding(&e.binding),
                        a.id()
                    );
                }
            }
        }
    }

    /// 绕道回归：ctrl+c（物理守卫放行键）换绑 delete 后——物理守卫仍
    /// 放行（白名单镜像对拍），但解析出 DeleteForward 非只读安全 →
    /// 接线层兜底条件成立（read_only && !readonly_safe → 拒绝）；
    /// 安全动作（copy）换绑不受影响。
    #[test]
    fn readonly_bypass_blocked_at_action_level() {
        // 物理守卫对 ctrl+c 放行（绕道前提——镜像夹具对拍）。
        assert!(readonly_allows_mirror("c", true));
        assert!(!readonly_allows_mirror("c", false));
        let mut store = KeymapStore::default_tables();
        let warnings = store.apply_user(&[UserOverride::Bind {
            bind: "ctrl+c".into(),
            action: "delete".into(),
        }]);
        assert!(warnings.is_empty(), "无冲突换绑不应警告：{warnings:?}");
        // 换绑生效：ctrl+c 解析出 DeleteForward（物理守卫放行该键）……
        assert_eq!(
            store.resolve_editor_main(&q("c", true, false, false), None),
            EditorOutcome::Matched(EditorAction::DeleteForward)
        );
        // ……且 DeleteForward 非只读安全 → 新接线层必拒（兜底条件成立）。
        assert!(!EditorAction::DeleteForward.readonly_safe());
        // 其余默认放行键不受换绑影响（select_all 完好）。
        assert_eq!(
            store.resolve_editor_main(&q("a", true, false, false), None),
            EditorOutcome::Matched(EditorAction::SelectAll)
        );
        // 安全动作换绑不受影响：ctrl+alt+c → copy 照常解析且安全。
        let mut store2 = KeymapStore::default_tables();
        let warnings = store2.apply_user(&[UserOverride::Bind {
            bind: "ctrl+alt+c".into(),
            action: "copy".into(),
        }]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            store2.resolve_editor_main(&q("c", true, false, true), None),
            EditorOutcome::Matched(EditorAction::Copy)
        );
        assert!(EditorAction::Copy.readonly_safe());
    }

    // ─── 全局装配 ───────────────────────────────────────────────────

    /// store() 未 init 时 = 默认表（单测/harness 默认面——测试不碰全局
    /// 写接口，进程内并行测试互不污染）。
    #[test]
    fn global_store_defaults_without_init() {
        let label = store().label("save").expect("默认表应可查");
        assert_eq!(label, "Ctrl+S");
    }
}
