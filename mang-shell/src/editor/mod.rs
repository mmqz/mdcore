//! 编辑器视图 —— #68 阶段 1「编辑区接文档模型」的 md 面。
//!
//! 组合三个纯逻辑模块成 GPUI 视图：
//!
//! - [`buffer::Buffer`]：行存储 + 光标 + 选区 + undo/redo + 查找
//! - [`highlight`]：md 行级 token 识别
//! - [`find_bar::FindState`]：查找条状态机（本视图的内嵌状态）
//!
//! ## 渲染架构
//!
//! ```text
//! v_flex
//! ├─ find_bar（Option——Ctrl+F 开 / Esc 关；独立 FocusHandle）
//! └─ div（track_focus + on_key_down + on_click 聚焦）
//!    └─ uniform_list（虚拟化行渲染：行号 gutter + StyledText 高亮行
//!       + 光标竖线[shape_line 前缀测宽] + 选区/查找命中底色叠加）
//!       └─ canvas（size_0——paint 期 IME handle_input 接线）
//! ```
//!
//! ## 输入通路
//!
//! - 键盘命令（移动/编辑/undo/redo/剪贴板/查找）：`on_key_down` →
//!   [`Editor::dispatch_key`] 单点分发（修饰键语义见该函数文档）
//! - IME（中文输入法）：`EntityInputHandler` 实现 + paint 期
//!   `window.handle_input` 接线（X11 预编辑实测登记于 #68 阶段 7）
//! - 可打印字符兜底：`keystroke.key_char`（无 IME 的直输路径）
//!
//! ## 鼠标通路（批 2：点击定位 / 拖选 / 双击词 / 三击行 / 滚轮）
//!
//! - mousedown → [`Editor::dispatch_mouse_down`]：单击定位（Shift=扩展）/
//!   双击选词（`click_count` 原生归一）/ 三击选行
//! - 拖选：`on_drag` + `on_drag_move`（锚不动、光标头跟随；双击/三击拖拽
//!   按词/行粒度对齐扩展）；拖出可见行区间自动滚动（40ms 任务步进）
//! - mouseup：窗口级监听（canvas paint 期每帧注册——任意位置抬起收尾）
//! - 行定位：每帧行 bounds 由行内 canvas prepaint 记录（虚拟化行准确），
//!   行内列定位 `shape_line` + `closest_index_for_position`
//! - 纯逻辑面（命中/clamp/词行选区/扩展粒度）在 [`mouse`]——单测钉定
//!
//! ## 已知边界（批 2 登记，#68 后续批补）
//!
//! - 软换行命中近似：行高固定 24px、长行视觉裁切——按未换行 shaping 命中
//! - gutter 点击按列 0 处理（不做 gutter 选行）
//! - 自动滚动步进 1 行/40ms（非像素级平滑）
//! - 行高统一（md 标题不放大字号——用粗体+色区分）
//! - 光标不闪烁（恒显）
//! - 保存为同步 `fs::write`（P2-5 异步化设计已有，暂不接线）
//! - IME 候选框定位 `bounds_for_range` 返回 `None`（精化登记阶段 7）

pub mod buffer;
pub mod docx_doc;
pub mod find_bar;
pub mod highlight;
pub mod mouse;
pub mod pdf_doc;

use std::cell::RefCell;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, DispatchPhase, DragMoveEvent,
    ElementInputHandler, EntityInputHandler, FocusHandle, FontStyle, FontWeight, HighlightStyle,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Render,
    ScrollStrategy, SharedString, StyledText, Task, TextStyle, UTF16Selection, UnderlineStyle,
    UniformListScrollHandle, Window, canvas, px, uniform_list,
};
use mouse::ClickKind;
use theme::ThemeColors;
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

use buffer::{Buffer, Point};
use docx_doc::{DocxPayload, DocxProps, HeadingEntry};
use find_bar::{FindKeyOutcome, FindState};
use highlight::{TokenKind, highlight_line};
use pdf_doc::{PdfPayload, PdfProps};

// ---------------------------------------------------------------------------
// 拖选状态与载荷
// ---------------------------------------------------------------------------

/// 拖选会话状态（mousedown 落锚 → drag_move 扩展 → mouseup 收尾）。
#[derive(Clone, Copy, Debug)]
struct MouseDrag {
    /// 拖拽锚（单击=命中点；双击=词首；三击=行首）。
    anchor: Point,
    /// 扩展粒度（单击=字符级；双击=词边界；三击=行边界）。
    kind: ClickKind,
}

/// 拖选 drag 载荷类型（`on_drag` / `on_drag_move` 配对标识；值本身无语义）。
#[derive(Clone, Copy)]
struct EditorDrag;

/// 拖选跟随视图（`on_drag` 契约需要一个 Render 实体——空渲染不可见）。
struct DragGhost;

/// 每帧行 bounds 记录（行号 → 窗口 bounds；canvas prepaint 每帧重填）。
type RowBounds = Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>>;

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().id("drag-ghost").size_0()
    }
}

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 编辑器字体：Sarasa Mono SC（CJK 等宽——中英混排对齐 + 光标定位稳定）。
const EDITOR_FONT: &str = "Sarasa Mono SC";
/// 正文字号（px）。
const FONT_SIZE: Pixels = px(15.);
/// 行高（px，uniform_list 等高行基准）。
const LINE_HEIGHT: Pixels = px(24.);
/// 行号 gutter 宽（px，容纳 5 位数 + 右缘留白）。
const GUTTER_WIDTH: Pixels = px(52.);
/// 正文左内边距（px）。
const TEXT_PADDING: Pixels = px(8.);
/// PageUp/PageDown 步进（行，近似一屏）。
const PAGE_LINES: usize = 20;

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

/// 文档编辑器视图（md 源文本编辑；docx/pdf 只读面后续批接线）。
pub struct Editor {
    buffer: Buffer,
    file_path: Option<PathBuf>,
    display_name: SharedString,
    /// 编辑区焦点（键盘命令域）。
    focus: FocusHandle,
    /// 查找条焦点（query 输入域）。
    find_focus: FocusHandle,
    /// 查找条状态（`None` = 关闭）。
    find: Option<FindState>,
    /// 行列表滚动句柄（光标行滚入可视区用）。
    scroll: UniformListScrollHandle,
    /// 每行行首的代码围栏状态缓存（文档变更时重算）。
    fence_cache: Vec<bool>,
    /// IME 预编辑段（全文 UTF16 偏移区间）。
    marked: Option<Range<usize>>,
    /// 本帧可见行 bounds（行内 canvas prepaint 记录——鼠标 hit-test 行
    /// 定位；虚拟化滚动后每帧重填）。
    row_bounds: RowBounds,
    /// 拖选会话（`None` = 非拖拽中）。
    drag: Option<MouseDrag>,
    /// 拖拽自动滚动目标（窗口坐标；`None` = 界内不滚）。
    autoscroll_target: Option<gpui::Point<Pixels>>,
    /// 自动滚动任务（40ms 步进；随拖拽会话存亡）。
    autoscroll_task: Option<Task<()>>,
    /// 鼠标事件 stdout 日志（`--smoke-mouse` 烟雾断言通道）。
    mouse_log: bool,
    /// 只读模式（批 3：docx 渲染面——变更类命令整段拒绝；批 4 复用同门）。
    read_only: bool,
    /// docx 转换产物（行模型 + 标题树 + 属性；`None` = 非 docx 态）。
    docx: Option<DocxPayload>,
    /// pdf 转换产物（批 4：行模型 + 书签树 + 页表；`None` = 非 pdf 态）。
    pdf: Option<PdfPayload>,
}

impl Editor {
    /// 打开文档：`path` 为 `None` 时是未命名新文档。
    pub fn new(path: Option<PathBuf>, content: String, cx: &mut Context<Self>) -> Self {
        let buffer = Buffer::from_plain_text(&content);
        let display_name = match &path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名".to_string()),
            None => "未命名".to_string(),
        };
        let mut this = Self {
            buffer,
            file_path: path,
            display_name: SharedString::from(display_name),
            focus: cx.focus_handle(),
            find_focus: cx.focus_handle(),
            find: None,
            scroll: UniformListScrollHandle::new(),
            fence_cache: Vec::new(),
            marked: None,
            row_bounds: Rc::new(RefCell::new(Vec::new())),
            drag: None,
            autoscroll_target: None,
            autoscroll_task: None,
            mouse_log: false,
            read_only: false,
            docx: None,
            pdf: None,
        };
        this.refresh_caches();
        // 初始聚焦由 ShellApp::new 完成（需要 window 上下文）。
        this
    }

    /// 打开 docx 只读文档（批 3）：行模型喂 Buffer（移动/选区/查找/
    /// 滚动全复用），样式 span 与标题树由 [`docx_doc`] 预计算承载。
    pub fn new_docx(path: PathBuf, payload: DocxPayload, cx: &mut Context<Self>) -> Self {
        let buffer = Buffer::from_plain_text(&payload.text);
        let display_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "未命名".to_string());
        let line_count = buffer.line_count();
        let mut this = Self {
            buffer,
            file_path: Some(path),
            display_name: SharedString::from(display_name),
            focus: cx.focus_handle(),
            find_focus: cx.focus_handle(),
            find: None,
            scroll: UniformListScrollHandle::new(),
            // docx 无围栏语义——fence 恒 false（refresh_caches 在 docx
            // 模式下跳过 md 逐行推进）。
            fence_cache: vec![false; line_count],
            marked: None,
            row_bounds: Rc::new(RefCell::new(Vec::new())),
            drag: None,
            autoscroll_target: None,
            autoscroll_task: None,
            mouse_log: false,
            read_only: true,
            docx: Some(payload),
            pdf: None,
        };
        this.refresh_caches();
        this
    }

    /// 打开 pdf 只读文档（批 4）：行模型喂 Buffer（移动/选区/查找/滚轮
    /// 批 1-2 管线全复用），样式 span（页分隔行）与书签树/页表由
    /// [`pdf_doc`] 预计算承载；页码标签走 [`Self::page_label`]。
    pub fn new_pdf(path: PathBuf, payload: PdfPayload, cx: &mut Context<Self>) -> Self {
        let buffer = Buffer::from_plain_text(&payload.text);
        let display_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "未命名".to_string());
        let line_count = buffer.line_count();
        let mut this = Self {
            buffer,
            file_path: Some(path),
            display_name: SharedString::from(display_name),
            focus: cx.focus_handle(),
            find_focus: cx.focus_handle(),
            find: None,
            scroll: UniformListScrollHandle::new(),
            // pdf 无围栏语义——fence 恒 false（同 docx 态口径）。
            fence_cache: vec![false; line_count],
            marked: None,
            row_bounds: Rc::new(RefCell::new(Vec::new())),
            drag: None,
            autoscroll_target: None,
            autoscroll_task: None,
            mouse_log: false,
            read_only: true,
            docx: None,
            pdf: Some(payload),
        };
        this.refresh_caches();
        this
    }

    /// 标题栏/状态栏显示名（文件名）。
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// 编辑区焦点句柄（IME 接线等）。
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// 修改标记（状态栏「●」）。
    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }

    /// 光标位置标签（状态栏「行, 列」——1 基显示）。
    pub fn cursor_label(&self) -> String {
        let c = self.buffer.cursor();
        format!("{}, {}", c.line + 1, c.col + 1)
    }

    /// 字数标签（状态栏——char 计数，CJK 逐字）。
    pub fn char_label(&self) -> String {
        format!("{} 字符", self.buffer.char_count())
    }

    /// 只读标记（状态栏「只读」——docx 渲染面）。
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// docx 文档属性（右栏属性面板；`None` = md 可编辑态）。
    pub fn docx_props(&self) -> Option<&DocxProps> {
        self.docx.as_ref().map(|d| &d.props)
    }

    /// pdf 文档属性（右栏属性面板；批 4）。
    pub fn pdf_props(&self) -> Option<&PdfProps> {
        self.pdf.as_ref().map(|p| &p.props)
    }

    /// 文档形态标识（panels/status_bar 分派："md" / "docx" / "pdf"）。
    pub fn doc_kind(&self) -> &'static str {
        if self.docx.is_some() {
            "docx"
        } else if self.pdf.is_some() {
            "pdf"
        } else {
            "md"
        }
    }

    /// 页码标签（状态栏——pdf 态：「第 N / M 页」由光标行经页表映射；
    /// 其余形态 `None`）。
    pub fn page_label(&self) -> Option<String> {
        let p = self.pdf.as_ref()?;
        let line = self.buffer.cursor().line;
        let no = p.page_of_line(line)?;
        Some(format!("第 {} / {} 页", no, p.props.pages))
    }

    /// 标题树（左栏「文档结构」）；md 态为空。
    /// docx = 标题树；pdf = 书签树（扁平文档序——批 4）。
    pub fn headings(&self) -> &[HeadingEntry] {
        self.docx
            .as_ref()
            .map(|d| d.headings.as_slice())
            .or_else(|| self.pdf.as_ref().map(|p| p.headings.as_slice()))
            .unwrap_or(&[])
    }

    /// 结构条目显示标签（左栏）：docx `Hn · 文本`；pdf `第 N 页 · 文本`
    /// （书签带页号前缀——跳转目标即该页页分隔行）。
    pub fn heading_label(&self, h: &HeadingEntry) -> String {
        match &self.pdf {
            Some(p) => match p.page_of_line(h.line) {
                Some(no) => format!("第 {no} 页 · {}", h.text),
                None => h.text.clone(),
            },
            None => format!("H{} · {}", h.level, h.text),
        }
    }

    /// 跳转到行（标题树点击锚点——光标落行首 + 滚入可视区顶部）。
    pub fn jump_to_line(&mut self, line: usize) {
        let line = line.min(self.buffer.line_count().saturating_sub(1));
        self.buffer.set_cursor(Point::new(line, 0));
        self.scroll.scroll_to_item(line, ScrollStrategy::Top);
    }

    /// 保存（Ctrl+S）：同步写盘 + 清修改标记。未命名文档无路径——
    /// 批 1 无「另存为」对话框，返回 false 登记。
    /// 只读文档（docx）无保存语义——双保险早退（键分发已拒 Ctrl+S，
    /// 此处防止任何其他调用面把行模型文本覆写 docx 二进制）。
    pub fn save(&mut self) -> bool {
        if self.read_only {
            return true;
        }
        match &self.file_path {
            Some(path) => {
                let content = self.buffer.to_plain_text();
                match std::fs::write(path, content) {
                    Ok(()) => {
                        self.buffer.mark_saved();
                        true
                    }
                    Err(err) => {
                        eprintln!("mang-shell: 保存失败 {}: {err}", path.display());
                        false
                    }
                }
            }
            None => false,
        }
    }

    /// 文档变更后的缓存刷新（fence 状态 + 查找匹配）。
    fn refresh_caches(&mut self) {
        // fence：逐行推进（md 模式；docx 只读态无围栏语义且免大文档
        // 逐行 md 高亮的无效开销——fence_cache 构造时恒 false）。
        if self.docx.is_none() {
            let mut states = Vec::with_capacity(self.buffer.line_count());
            let mut fence = false;
            for i in 0..self.buffer.line_count() {
                states.push(fence);
                fence = highlight_line(self.buffer.line(i), fence).fence_at_end;
            }
            self.fence_cache = states;
        }
        if let Some(f) = &mut self.find {
            f.refresh(&self.buffer);
        }
    }

    /// 行 `idx` 行首的围栏状态（缓存查询；越界 false）。
    fn fence_at(&self, idx: usize) -> bool {
        self.fence_cache.get(idx).copied().unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // 键盘命令分发
    // -----------------------------------------------------------------------

    /// 编辑区 `on_key_down` 单点分发。
    ///
    /// 修饰键：`Ctrl`（Linux/Windows）与 `platform`（macOS ⌘ / Windows 键）
    /// 等价取或。命令集：移动（含 Shift 选区 / Ctrl 词跳 / Home / End /
    /// PageUp / PageDown）、编辑（Backspace / Delete / Enter / Tab）、
    /// undo / redo、剪贴板、全选、查找（Ctrl+F）、保存（Ctrl+S）、
    /// Esc 清选区、可打印字符插入。
    fn dispatch_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let ctrl = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;
        let key = ks.key.to_lowercase();

        // 只读门禁（批 3）：浏览类命令放行（移动/选区/全选/复制/查找），
        // 变更类整段拒绝（输入/删除/回车/Tab/剪切/粘贴/撤销/重做/保存）
        // ——纯函数面单测钉定（docx_doc::readonly_allows）。
        if self.read_only && !docx_doc::readonly_allows(&key, ctrl) {
            cx.notify();
            return;
        }

        match key.as_str() {
            "left" => {
                if ctrl {
                    self.buffer.move_word_left(shift);
                } else {
                    self.buffer.move_left(shift);
                }
            }
            "right" => {
                if ctrl {
                    self.buffer.move_word_right(shift);
                } else {
                    self.buffer.move_right(shift);
                }
            }
            "up" => self.buffer.move_up(shift),
            "down" => self.buffer.move_down(shift),
            "home" => {
                if ctrl {
                    self.buffer.move_doc_start(shift);
                } else {
                    self.buffer.move_home(shift);
                }
            }
            "end" => {
                if ctrl {
                    self.buffer.move_doc_end(shift);
                } else {
                    self.buffer.move_end(shift);
                }
            }
            "pageup" => {
                for _ in 0..PAGE_LINES {
                    self.buffer.move_up(shift);
                }
            }
            "pagedown" => {
                for _ in 0..PAGE_LINES {
                    self.buffer.move_down(shift);
                }
            }
            "enter" => self.buffer.newline(),
            "backspace" => self.buffer.backspace(),
            "delete" => self.buffer.delete_forward(),
            "tab" => self.buffer.tab(),
            "escape" => {
                if self.buffer.selection().is_some() {
                    let head = self.buffer.cursor();
                    self.buffer.set_cursor(head);
                }
            }
            "a" if ctrl => self.buffer.select_all(),
            "c" if ctrl => self.copy_selection(cx),
            "x" if ctrl => {
                let text = self.buffer.selection_text();
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.buffer.delete_selection();
                }
            }
            "v" if ctrl => {
                if let Some(item) = cx.read_from_clipboard()
                    && let Some(text) = item.text()
                {
                    self.buffer.insert(&text);
                }
            }
            "z" if ctrl && shift => {
                self.buffer.redo();
            }
            "z" if ctrl => {
                self.buffer.undo();
            }
            "y" if ctrl => {
                self.buffer.redo();
            }
            "f" if ctrl => self.open_find(window, cx),
            "s" if ctrl => {
                self.save();
            }
            _ => {
                // 可打印字符（无修饰或仅 shift）：插入。
                // key_char 为平台布局产生的实际字符（IME 未接管时的直输路径；
                // 控制字符（含 \n/\r/tab）已被上方分支截获，此处双保险过滤）。
                if !ctrl
                    && !ks.modifiers.alt
                    && let Some(c) = &ks.key_char
                    && !c.is_empty()
                    && c.chars().all(|ch| !ch.is_control())
                {
                    self.buffer.insert(c);
                }
            }
        }
        self.after_edit();
        cx.notify();
    }

    /// 编辑命令后的公共收尾：缓存刷新 + 光标行滚入可视区。
    fn after_edit(&mut self) {
        self.refresh_caches();
        let line = self.buffer.cursor().line;
        self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
    }

    /// 行样式源分派：docx/pdf 只读态取预计算 span，md 态走围栏感知高亮。
    fn line_tokens(&self, idx: usize, line: &str) -> highlight::LineHighlight {
        let runs = match (&self.docx, &self.pdf) {
            (Some(d), _) => d.spans.get(idx).cloned(),
            (None, Some(p)) => p.spans.get(idx).cloned(),
            (None, None) => return highlight_line(line, self.fence_at(idx)),
        };
        highlight::LineHighlight {
            runs: runs.unwrap_or_default(),
            fence_at_end: false,
        }
    }

    /// 复制选区（无选区无操作）。
    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let text = self.buffer.selection_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// 打开查找条（Ctrl+F）：初始化 query（选中文本优先）、聚焦输入域。
    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut state = FindState::default();
        state.open(&self.buffer);
        self.find = Some(state);
        window.focus(&self.find_focus, cx);
        cx.notify();
    }

    /// 关闭查找条：焦点回编辑区。
    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// 查找条按键分发（`find_focus` 聚焦时）。
    fn dispatch_find_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        match find.handle_key(&self.buffer, &event.keystroke) {
            FindKeyOutcome::Handled => {}
            FindKeyOutcome::QueryChanged => {
                // refresh 已在 handle_key 内联完成（与测试同一语义）。
            }
            FindKeyOutcome::JumpForward => {
                if let Some(f) = self.find.as_mut() {
                    f.jump(&mut self.buffer, false);
                }
                self.after_edit();
            }
            FindKeyOutcome::JumpBackward => {
                if let Some(f) = self.find.as_mut() {
                    f.jump(&mut self.buffer, true);
                }
                self.after_edit();
            }
            FindKeyOutcome::Close => {
                self.close_find(window, cx);
                return;
            }
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // 鼠标（#68 阶段 1 批 2：点击定位 / 拖选 / 双击词 / 三击行 / 自动滚动）
    // -----------------------------------------------------------------------

    /// 鼠标事件 stdout 日志开关（`--smoke-mouse` 烟雾断言通道）。
    pub fn set_mouse_log(&mut self, on: bool) {
        self.mouse_log = on;
    }

    /// 编辑区 mousedown 分派（`click_count` 原生归一：单击定位 / Shift
    /// 扩展 / 双击选词 / 三击选行——词/行边界纯逻辑见 [`mouse`]）。
    fn dispatch_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let hit = self.hit_test(event.position, window);
        let kind = mouse::click_kind(event.click_count);
        let anchor = match kind {
            ClickKind::Single => {
                if event.modifiers.shift {
                    // Shift+单击：扩展现有选区（无选区以当前光标为锚）。
                    let anchor = self
                        .buffer
                        .selection()
                        .map(|s| s.start)
                        .unwrap_or_else(|| self.buffer.cursor());
                    self.buffer.set_selection(anchor, hit);
                    anchor
                } else {
                    self.buffer.set_cursor(hit);
                    hit
                }
            }
            ClickKind::Double => match mouse::word_range_at(self.buffer.line(hit.line), hit.col) {
                Some(r) => {
                    let (start, end) = (Point::new(hit.line, r.start), Point::new(hit.line, r.end));
                    self.buffer.set_selection(start, end);
                    start
                }
                None => {
                    // 空行无可选词——仅定位。
                    self.buffer.set_cursor(hit);
                    hit
                }
            },
            ClickKind::Triple => {
                let (start, end) = mouse::line_range(&self.buffer, hit.line);
                self.buffer.set_selection(start, end);
                start
            }
        };
        self.drag = Some(MouseDrag { anchor, kind });
        self.stop_autoscroll();
        if self.mouse_log {
            println!(
                "mouse: down count={} ({}, {}) -> line {}, col {}",
                event.click_count,
                event.position.x,
                event.position.y,
                hit.line + 1,
                hit.col + 1
            );
        }
        cx.notify();
    }

    /// 拖选扩展（`on_drag_move`：锚不动、光标头跟随鼠标；扩展粒度按
    /// mousedown 类型对齐词/行边界；出界挂自动滚动）。
    fn dispatch_drag_move(
        &mut self,
        event: &DragMoveEvent<EditorDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.drag else {
            return;
        };
        let pos = event.event.position;
        // 拖拽语义 clamp：出界落**可见边界行**（渐进式——自动滚动逐步
        // 扩展；与点击的文档边界 clamp 区分）。
        let (yhit, col) = self.hit_test_parts(pos, window);
        let target = mouse::visible_clamp_point(&self.buffer, yhit, col, self.visible_lines());
        let head = mouse::drag_head(&self.buffer, drag.kind, drag.anchor, target);
        self.buffer.set_selection(drag.anchor, head);
        // 出界自动滚动（回入界内即停目标——任务待命随会话存亡）。
        if let Some((first_top, last_bottom)) = self.rows_extent() {
            if mouse::autoscroll_dir(pos.y, first_top, last_bottom).is_some() {
                self.autoscroll_target = Some(pos);
                self.ensure_autoscroll_task(cx);
            } else {
                self.autoscroll_target = None;
            }
        }
        if self.mouse_log {
            let c = self.buffer.cursor();
            println!(
                "mouse: drag ({}, {}) -> line {}, col {}",
                pos.x,
                pos.y,
                c.line + 1,
                c.col + 1
            );
        }
        cx.notify();
    }

    /// 拖拽收尾（窗口级 mouseup——任意位置抬起都结束拖选与自动滚动）。
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_none() {
            return;
        }
        self.autoscroll_target = None;
        self.autoscroll_task = None;
        if self.mouse_log {
            match self.buffer.selection() {
                Some(r) => {
                    let text = self.buffer.selection_text();
                    let snippet: String = text.chars().take(24).collect();
                    println!(
                        "mouse: up -- selection {}..{} ({} chars) [{}]",
                        format_point(&r.start),
                        format_point(&r.end),
                        text.chars().count(),
                        snippet
                    );
                }
                None => println!("mouse: up -- no selection"),
            }
        }
        cx.notify();
    }

    /// 启动自动滚动任务（已运行则仅由调用方更新目标）。
    fn ensure_autoscroll_task(&mut self, cx: &mut Context<Self>) {
        if self.autoscroll_task.is_some() {
            return;
        }
        self.autoscroll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(40))
                    .await;
                let again = this
                    .update(cx, |editor, cx| editor.autoscroll_tick(cx))
                    .unwrap_or(false);
                if !again {
                    break;
                }
            }
        }));
    }

    /// 自动滚动一步（任务每 40ms 驱动；`ScrollStrategy::Top/Bottom` 单行
    /// 步进）。返回任务是否继续。
    fn autoscroll_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.autoscroll_target else {
            return true; // 界内待命（拖拽会话未结束）
        };
        let Some(drag) = self.drag else {
            return false; // 防御：拖拽已结束（end_drag 应已停任务）
        };
        let Some((first_line, last_line)) = self.visible_lines() else {
            return true;
        };
        let Some((first_top, last_bottom)) = self.rows_extent() else {
            return true;
        };
        let Some(dir) = mouse::autoscroll_dir(target.y, first_top, last_bottom) else {
            self.autoscroll_target = None;
            return true;
        };
        let (target_line, strategy) = match dir {
            mouse::ScrollDir::Up => (first_line.saturating_sub(1), ScrollStrategy::Top),
            mouse::ScrollDir::Down => (
                (last_line + 1).min(self.buffer.line_count() - 1),
                ScrollStrategy::Bottom,
            ),
        };
        self.scroll.scroll_to_item(target_line, strategy);
        // 选区头跟到边界行（列取当前列 clamp——纵向意图保持）。
        let col = self
            .buffer
            .cursor()
            .col
            .min(self.buffer.line_len(target_line));
        let head = mouse::drag_head(
            &self.buffer,
            drag.kind,
            drag.anchor,
            Point::new(target_line, col),
        );
        self.buffer.set_selection(drag.anchor, head);
        if self.mouse_log {
            println!("mouse: autoscroll {:?} -> line {}", dir, target_line + 1);
        }
        cx.notify();
        true
    }

    /// 停止自动滚动（新 mousedown / 拖拽收尾——drop 任务即取消）。
    fn stop_autoscroll(&mut self) {
        self.autoscroll_target = None;
        self.autoscroll_task = None;
    }

    /// 可见行区间（首/末行号；无渲染记录 `None`）。
    fn visible_lines(&self) -> Option<(usize, usize)> {
        let rows = self.row_bounds.borrow();
        Some((rows.first()?.0, rows.last()?.0))
    }

    /// 可见行 y 区间（首行顶缘 / 末行底缘——自动滚动出界判定基准）。
    fn rows_extent(&self) -> Option<(Pixels, Pixels)> {
        let rows = self.row_bounds.borrow();
        let first = rows.first()?;
        let last = rows.last()?;
        Some((first.1.origin.y, last.1.origin.y + last.1.size.height))
    }

    /// 窗口坐标 → 点击语义文档 Point（y=可见行命中 + 文档边界 clamp；
    /// x=shaping 最近字符边界）。gutter 与正文左侧 → 列 0；行尾外 →
    /// 行尾；文末空白 → 文末（Lead 验收口径）。
    fn hit_test(&self, position: gpui::Point<Pixels>, window: &mut Window) -> Point {
        let (yhit, col) = self.hit_test_parts(position, window);
        mouse::clamp_point(&self.buffer, yhit, col)
    }

    /// 窗口坐标 → (y 原始命中, 行内列)——clamp 语义由调用方选择（点击=
    /// 文档边界 [`mouse::clamp_point`]；拖拽=可见边界 [`mouse::visible_clamp_point`]）。
    fn hit_test_parts(
        &self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
    ) -> (mouse::YHit, usize) {
        let rows = self.row_bounds.borrow();
        match mouse::line_for_y(position.y, &rows) {
            mouse::YHit::Row(idx) => {
                let row_x = rows
                    .iter()
                    .find(|(i, _)| *i == idx)
                    .map(|(_, b)| b.origin.x)
                    .unwrap_or(px(0.));
                let x_in_text = position.x - row_x - GUTTER_WIDTH - TEXT_PADDING;
                let col = self.column_for_x(idx, x_in_text, window);
                (mouse::YHit::Row(idx), col)
            }
            other => (other, 0),
        }
    }

    /// 行内 x（相对正文原点）→ char 列（`shape_line` 单 run + 最近字符
    /// 边界——与渲染同字体面；高亮不改变字形位置）。
    fn column_for_x(&self, idx: usize, x: Pixels, window: &mut Window) -> usize {
        if x <= px(0.) {
            return 0;
        }
        let line = self.buffer.line(idx);
        if line.is_empty() {
            return 0;
        }
        let style = editor_hit_text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let run = style.to_run(line.len());
        let shaped = window
            .text_system()
            .shape_line(line.into(), font_size, &[run], None);
        // 行内未换行 shaping——最近字符边界（越界=行尾字节偏移）。
        let byte = shaped.closest_index_for_x(x);
        mouse::byte_index_to_col(line, byte)
    }

    // -----------------------------------------------------------------------
    // 渲染
    // -----------------------------------------------------------------------

    /// 查找条（编辑区顶部）：放大镜 + query 输入区 + 计数 + 关闭钮。
    fn render_find_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let query: SharedString = self
            .find
            .as_ref()
            .map(|f| f.query().to_string())
            .unwrap_or_default()
            .into();
        let label = self
            .find
            .as_ref()
            .map(|f| f.match_label())
            .unwrap_or_default();
        let cursor_col = self.find.as_ref().map(|f| f.cursor_col()).unwrap_or(0);
        // 输入光标 x：shape query 前缀宽度。
        let prefix: String = query.chars().take(cursor_col).collect();
        let text_style = find_text_style(&colors);
        let prefix_width = shape_text_width(window, &prefix, &text_style);

        h_flex()
            .id("find-bar")
            .w_full()
            .flex_none()
            .h(px(36.))
            .px_3()
            .gap_2()
            .bg(colors.title_bar_background)
            .child(
                Icon::new(IconName::MagnifyingGlass)
                    .size(IconSize::Small)
                    .color(ui::Color::Muted),
            )
            .child(
                div()
                    .id("find-input")
                    .track_focus(&self.find_focus)
                    .on_key_down(cx.listener(Self::dispatch_find_key))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .child(
                        div()
                            .id("find-input-text")
                            .h_full()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_family(EDITOR_FONT)
                            .text_color(colors.text)
                            .child(StyledText::new(query)),
                    )
                    // 输入光标竖线
                    .child(
                        div()
                            .id("find-caret")
                            .absolute()
                            .left(prefix_width)
                            .top(px(9.))
                            .w(px(2.))
                            .h(px(18.))
                            .bg(colors.text),
                    ),
            )
            .child(
                Label::new(label)
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                div()
                    .id("find-close")
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .hover(|el| el.bg(colors.element_hover))
                    .child(
                        Icon::new(IconName::Close)
                            .size(IconSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_find(window, cx);
                    })),
            )
    }

    /// 编辑区主体：焦点域 + 键盘 + 鼠标（批 2）+ 虚拟化行列表 + IME 接线。
    fn render_editor_area(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let line_count = self.buffer.line_count();
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let weak = entity.downgrade();
        let ime_entity = entity.clone();
        let up_entity = entity.clone();

        div()
            .id("editor-surface")
            .size_full()
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::dispatch_key))
            // 批 2：mousedown 分派（单击定位 / Shift 扩展 / 双击词 / 三击行）。
            .on_mouse_down(MouseButton::Left, cx.listener(Self::dispatch_mouse_down))
            // 批 2：拖选（drag 会话内 move 事件全量回调——含元素外；
            // `DragMoveEvent.bounds` 不用，行定位走每帧行 bounds 记录）。
            .on_drag(EditorDrag, |_, _, _, cx| cx.new(|_| DragGhost))
            .on_drag_move::<EditorDrag>(cx.listener(Self::dispatch_drag_move))
            .child(
                uniform_list(
                    "editor-lines",
                    line_count,
                    move |visible: Range<usize>, window: &mut Window, cx: &mut App| {
                        weak.update(cx, |this, cx| {
                            this.render_lines(visible, window, cx, focused)
                        })
                        .unwrap_or_default()
                    },
                )
                .track_scroll(&self.scroll)
                .h_full(),
            )
            // IME 接线 + 批 2 mouseup 收尾（paint 期每帧注册；canvas
            // size_0 不占布局——仅借 paint 时机与 bounds）。
            .child(div().id("ime-hook").absolute().size_0().child(canvas(
                move |bounds, _window, _cx| bounds,
                move |bounds: Bounds<Pixels>, _t, window: &mut Window, cx: &mut App| {
                    let focus = ime_entity.read(cx).focus_handle().clone();
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, ime_entity.clone()),
                        cx,
                    );
                    // 批 2：窗口级 mouseup（任意位置抬起都收尾拖选/停自动
                    // 滚动；本编辑器不可见时 canvas 不 paint，监听自然失活）。
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            up_entity.update(cx, |this, cx| this.end_drag(cx));
                        }
                    });
                },
            )))
            .when(!focused, |el| {
                // 未聚焦时提示层（点击任意处聚焦）
                el.child(
                    div()
                        .id("unfocused-hint")
                        .absolute()
                        .top_0()
                        .right_0()
                        .px_2()
                        .py_1()
                        .text_size(px(11.))
                        .text_color(colors.text_muted)
                        .child("点击编辑区以聚焦"),
                )
            })
    }

    /// uniform_list 可见行批量渲染。
    fn render_lines(
        &mut self,
        visible: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
        focused: bool,
    ) -> Vec<AnyElement> {
        let colors = cx.theme().colors().clone();
        let text_style = editor_text_style(&colors);
        let cursor = self.buffer.cursor();
        let selection = self.buffer.selection();
        // 本可见区间的查找命中（含是否激活项）。
        let find_hits: Vec<(Point, bool)> = self
            .find
            .as_ref()
            .map(|f| {
                f.matches()
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| visible.contains(&m.line))
                    .map(|(i, m)| (*m, i == f.active_index()))
                    .collect()
            })
            .unwrap_or_default();
        let query_len = self
            .find
            .as_ref()
            .map(|f| f.query().chars().count())
            .unwrap_or(0);
        let marked = self.marked.clone();
        // 批 2：本帧行 bounds 记录重置（下方各行的 canvas prepaint 重填）。
        self.row_bounds.borrow_mut().clear();

        (visible.start..visible.end)
            .map(|idx| {
                let line = self.buffer.line(idx).to_string();
                let tokens = self.line_tokens(idx, &line);
                let runs = self.styled_runs_for_line(
                    &line,
                    &tokens,
                    idx,
                    selection.clone(),
                    &find_hits,
                    query_len,
                    marked.clone(),
                    &colors,
                );
                let is_cursor_line = idx == cursor.line;
                let line_number = (idx + 1).to_string();

                // 光标竖线 x：shape 光标前前缀。
                let caret_x = if is_cursor_line && focused {
                    let prefix: String = line.chars().take(cursor.col).collect();
                    GUTTER_WIDTH + TEXT_PADDING + shape_text_width(window, &prefix, &text_style)
                } else {
                    px(0.)
                };

                let num_color = if is_cursor_line && focused {
                    colors.editor_active_line_number
                } else {
                    colors.editor_line_number
                };

                div()
                    .id(SharedString::from(format!("line-{idx}")))
                    .h(LINE_HEIGHT)
                    .w_full()
                    .flex()
                    .items_center()
                    .relative()
                    .when(is_cursor_line && focused, |el| {
                        el.bg(colors.editor_active_line_background)
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("gutter-{idx}")))
                            .w(GUTTER_WIDTH)
                            .flex_none()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_end()
                            .pr(px(10.))
                            .text_size(px(12.))
                            .text_color(num_color)
                            .child(line_number),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("text-{idx}")))
                            .pl(TEXT_PADDING)
                            .min_w_0()
                            .flex_1()
                            .text_size(FONT_SIZE)
                            .line_height(LINE_HEIGHT)
                            .font_family(EDITOR_FONT)
                            .text_color(colors.text)
                            .child(StyledText::new(line).with_highlights(runs)),
                    )
                    .when(is_cursor_line && focused, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("caret-{idx}")))
                                .absolute()
                                .left(caret_x)
                                .top(px(2.))
                                .w(px(2.))
                                .h(LINE_HEIGHT - px(4.))
                                .bg(colors.text),
                        )
                    })
                    // 批 2：行 bounds 记录（absolute 左上角=行原点；size_0
                    // 不占布局——prepaint 收 bounds，paint 空）。
                    .child({
                        let rows = self.row_bounds.clone();
                        canvas(
                            move |bounds, _, _| rows.borrow_mut().push((idx, bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_0()
                    })
                    .into_any_element()
            })
            .collect()
    }

    /// 行内样式 runs 合成：语法 token + 选区 + 查找命中 + IME 预编辑下划线
    /// （char 索引 → 逐 char 风格合成 → byte 区间压缩输出）。
    #[allow(clippy::too_many_arguments)]
    fn styled_runs_for_line(
        &self,
        line: &str,
        tokens: &highlight::LineHighlight,
        line_idx: usize,
        selection: Option<Range<Point>>,
        find_hits: &[(Point, bool)],
        query_len: usize,
        marked: Option<Range<usize>>,
        colors: &ThemeColors,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let chars: Vec<char> = line.chars().collect();
        let n = chars.len();
        let base = base_highlight(colors);

        // 该行选区 char 范围。
        let sel_range: Option<Range<usize>> = selection.map(|s| {
            let start = if s.start.line == line_idx {
                s.start.col
            } else {
                0
            };
            let end = if s.end.line == line_idx { s.end.col } else { n };
            start.min(end)..end.max(start)
        });
        // 本行查找命中 char 范围集。
        let hit_ranges: Vec<(Range<usize>, bool)> = find_hits
            .iter()
            .filter(|(m, _)| m.line == line_idx)
            .map(|(m, active)| (m.col..(m.col + query_len).min(n), *active))
            .collect();
        // IME 预编辑（全文 UTF16 区间的近似渲染：覆盖整段的下划线——
        // 精确 char 映射由 utf16_to_point 换算，此处简化为行内 span）。
        let marked_range = marked;

        // 逐 char 合成最终样式。
        let mut runs: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        let mut i = 0;
        while i < n {
            let mut s = base;
            // 语法 token。
            for (r, kind) in &tokens.runs {
                if r.contains(&i) {
                    s = merge_highlight(s, token_style(*kind, colors));
                    break;
                }
            }
            // 查找命中底色。
            for (r, active) in &hit_ranges {
                if r.contains(&i) {
                    s.background_color = Some(if *active {
                        colors.element_selection_background
                    } else {
                        colors.editor_document_highlight_read_background
                    });
                    break;
                }
            }
            // 选区底色（最高优先）。
            if sel_range.as_ref().is_some_and(|r| r.contains(&i)) {
                s.background_color = Some(colors.element_selection_background);
            }
            // IME 预编辑下划线。
            if marked_range.as_ref().is_some_and(|r| r.contains(&i)) {
                s.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(colors.text_accent),
                    wavy: false,
                });
            }
            // 压缩：与前一 run 同样式且连续则延长。
            if let Some((r, prev)) = runs.last_mut()
                && *prev == s
                && r.end == i
            {
                r.end = i + 1;
                i += 1;
                continue;
            }
            runs.push((i..i + 1, s));
            i += 1;
        }

        // char 区间 → byte 区间。
        let byte_of: Vec<usize> = {
            let mut v = Vec::with_capacity(n + 1);
            v.push(0);
            let mut acc = 0;
            for c in &chars {
                acc += c.len_utf8();
                v.push(acc);
            }
            v
        };
        runs.into_iter()
            .map(|(r, s)| (byte_of[r.start]..byte_of[r.end], s))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 文本样式与颜色映射
// ---------------------------------------------------------------------------

/// 编辑器正文 TextStyle 基座（字体面——hit-test shaping 与渲染共用；
/// 颜色不影响字形位置，hit-test 用占位色）。
fn editor_hit_text_style() -> TextStyle {
    TextStyle {
        color: gpui::black(),
        font_family: EDITOR_FONT.into(),
        font_features: Default::default(),
        font_fallbacks: None,
        font_size: FONT_SIZE.into(),
        line_height: LINE_HEIGHT.into(),
        font_weight: FontWeight::default(),
        font_style: FontStyle::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
        white_space: Default::default(),
        text_overflow: None,
        text_align: Default::default(),
        line_clamp: None,
    }
}

/// 编辑器正文 TextStyle（Sarasa Mono SC 15px / 24px 行高；shape 定宽用）。
fn editor_text_style(colors: &ThemeColors) -> TextStyle {
    TextStyle {
        color: colors.text,
        ..editor_hit_text_style()
    }
}

/// 查找条输入 TextStyle。
fn find_text_style(colors: &ThemeColors) -> TextStyle {
    TextStyle {
        font_size: px(14.).into(),
        line_height: px(20.).into(),
        ..editor_text_style(colors)
    }
}

/// 无 token 的基础 HighlightStyle。
fn base_highlight(colors: &ThemeColors) -> HighlightStyle {
    HighlightStyle {
        color: Some(colors.text),
        ..Default::default()
    }
}

/// token 类别 → 颜色/字重/字形（主题语义字段映射，不依赖语法主题加载）。
fn token_style(kind: TokenKind, colors: &ThemeColors) -> HighlightStyle {
    let mut s = base_highlight(colors);
    match kind {
        TokenKind::Heading => {
            s.color = Some(colors.text_accent);
            s.font_weight = Some(FontWeight::BOLD);
        }
        TokenKind::Bold => {
            s.font_weight = Some(FontWeight::BOLD);
        }
        TokenKind::Italic => {
            s.font_style = Some(FontStyle::Italic);
        }
        TokenKind::InlineCode | TokenKind::BlockCode => {
            s.background_color = Some(colors.element_hover);
        }
        TokenKind::Quote => {
            s.color = Some(colors.text_muted);
        }
        TokenKind::ListMarker => {
            s.color = Some(colors.text_accent);
        }
        TokenKind::FenceLang => {
            s.color = Some(colors.text_muted);
            s.font_style = Some(FontStyle::Italic);
        }
        TokenKind::Link => {
            s.color = Some(colors.text_accent);
            s.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(colors.text_accent),
                wavy: false,
            });
        }
    }
    s
}

/// 样式叠加（`over` 的 `Some` 字段覆盖 `base`）。
fn merge_highlight(mut base: HighlightStyle, over: HighlightStyle) -> HighlightStyle {
    if over.color.is_some() {
        base.color = over.color;
    }
    if over.font_weight.is_some() {
        base.font_weight = over.font_weight;
    }
    if over.font_style.is_some() {
        base.font_style = over.font_style;
    }
    if over.background_color.is_some() {
        base.background_color = over.background_color;
    }
    if over.underline.is_some() {
        base.underline = over.underline;
    }
    base
}

/// shape 单段文本取像素宽（光标竖线定位）。
fn shape_text_width(window: &mut Window, text: &str, style: &TextStyle) -> Pixels {
    if text.is_empty() {
        return px(0.);
    }
    let font_size = style.font_size.to_pixels(window.rem_size());
    let run = style.to_run(text.len());
    window
        .text_system()
        .shape_line(text.into(), font_size, &[run], None)
        .width()
}

/// Point 日志格式（1 基「行:列」——`--smoke-mouse` 断言通道）。
fn format_point(p: &Point) -> String {
    format!("{}:{}", p.line + 1, p.col + 1)
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let find_open = self.find.is_some();
        v_flex()
            .id("editor-root")
            .size_full()
            .bg(cx.theme().colors().background)
            .when(find_open, |el| el.child(self.render_find_bar(window, cx)))
            .child(self.render_editor_area(window, cx))
    }
}

// ---------------------------------------------------------------------------
// IME（EntityInputHandler——UTF16 坐标）
// ---------------------------------------------------------------------------

impl Editor {
    /// Point → 全文 UTF16 偏移。
    fn point_to_utf16(&self, p: Point) -> usize {
        point_to_utf16_offset(&self.buffer, p)
    }

    /// 全文 UTF16 偏移 → Point（clamp 到文档域）。
    fn utf16_to_point(&self, offset: usize) -> Point {
        utf16_offset_to_point(&self.buffer, offset)
    }
}

/// Point → 全文 UTF16 偏移（换行按 1 码元）。
fn point_to_utf16_offset(buffer: &Buffer, p: Point) -> usize {
    let mut offset = 0;
    for i in 0..p.line {
        offset += buffer.line(i).chars().map(|c| c.len_utf16()).sum::<usize>() + 1;
    }
    offset
        + buffer
            .line(p.line)
            .chars()
            .take(p.col)
            .map(|c| c.len_utf16())
            .sum::<usize>()
}

/// 全文 UTF16 偏移 → Point（clamp 到文档域；代理对内部偏移落到码元边界）。
fn utf16_offset_to_point(buffer: &Buffer, mut offset: usize) -> Point {
    for i in 0..buffer.line_count() {
        let line_len: usize = buffer.line(i).chars().map(|c| c.len_utf16()).sum();
        if offset <= line_len {
            let mut u16_acc = 0;
            let mut col = 0;
            for c in buffer.line(i).chars() {
                if u16_acc >= offset {
                    break;
                }
                u16_acc += c.len_utf16();
                col += 1;
            }
            return Point::new(i, col);
        }
        offset -= line_len + 1;
    }
    let last = buffer.line_count() - 1;
    Point::new(last, buffer.line_len(last))
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        _adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let start = self.utf16_to_point(range.start);
        let end = self.utf16_to_point(range.end);
        Some(self.buffer.range_text(&(start..end)))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (start, end) = match self.buffer.selection() {
            Some(r) => (r.start, r.end),
            None => {
                let c = self.buffer.cursor();
                (c, c)
            }
        };
        Some(UTF16Selection {
            range: self.point_to_utf16(start)..self.point_to_utf16(end),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 只读门禁（IME 通道——键分发同口径）。
        if self.read_only {
            return;
        }
        let (s, e) = match range {
            Some(r) => (self.utf16_to_point(r.start), self.utf16_to_point(r.end)),
            None => match self.buffer.selection() {
                Some(sel) => (sel.start, sel.end),
                None => {
                    let c = self.buffer.cursor();
                    (c, c)
                }
            },
        };
        self.marked = None;
        self.buffer.replace_range(s, e, text);
        self.after_edit();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 只读门禁（IME 预编辑通道——只读文档不接预编辑）。
        if self.read_only {
            return;
        }
        // 预编辑语义：替换旧预编辑段（或指定范围），记录新段。
        let (s, e) = match (&self.marked, range) {
            (Some(r), _) => (self.utf16_to_point(r.start), self.utf16_to_point(r.end)),
            (None, Some(r)) => (self.utf16_to_point(r.start), self.utf16_to_point(r.end)),
            (None, None) => {
                let c = self.buffer.cursor();
                (c, c)
            }
        };
        self.buffer.replace_range(s, e, new_text);
        let new_start = self.point_to_utf16(s);
        self.marked =
            Some(new_start..new_start + new_text.chars().map(|c| c.len_utf16()).sum::<usize>());
        if let Some(sel) = new_selected_range {
            let a = self.utf16_to_point(new_start + sel.start);
            let b = self.utf16_to_point(new_start + sel.end);
            self.buffer.set_selection(a, b);
        }
        self.after_edit();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // IME 候选框定位（批 1 返回 None——精化登记 #68 阶段 7 Windows 验证）。
        None
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // 批 2：接鼠标 hit-test（iOS/Android 文本交互面；桌面同路径复用）。
        // 输出全文 UTF16 偏移（IME 坐标同制）。
        let p = self.hit_test(point, window);
        Some(self.point_to_utf16(p))
    }
}

// =============================================================================
// 单元测试（UTF16 映射——IME 坐标换算；Buffer/highlight/find 的逻辑面
// 各自模块内钉定，此处只测视图层的换算）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }

    #[test]
    fn utf16_roundtrip_ascii() {
        let b = Buffer::from_plain_text("hello\nworld");
        assert_eq!(point_to_utf16_offset(&b, p(0, 0)), 0);
        assert_eq!(point_to_utf16_offset(&b, p(0, 5)), 5);
        assert_eq!(point_to_utf16_offset(&b, p(1, 0)), 6, "跨行 +1（换行符）");
        assert_eq!(point_to_utf16_offset(&b, p(1, 5)), 11);
        // 逆映射
        assert_eq!(utf16_offset_to_point(&b, 0), p(0, 0));
        assert_eq!(utf16_offset_to_point(&b, 6), p(1, 0));
        assert_eq!(utf16_offset_to_point(&b, 11), p(1, 5));
    }

    #[test]
    fn utf16_roundtrip_cjk() {
        // 中文：每 char 3 字节 / 1 个 UTF16 码元；emoji "😀" 是 2 个码元。
        let b = Buffer::from_plain_text("中文😀\nx");
        // 行 0：中(1)文(1)😀(2) = 4 码元。
        assert_eq!(
            point_to_utf16_offset(&b, p(0, 3)),
            4,
            "3 个 char = 4 个 UTF16 码元"
        );
        assert_eq!(point_to_utf16_offset(&b, p(1, 0)), 5);
        assert_eq!(point_to_utf16_offset(&b, p(1, 1)), 6);
        // 逆：码元 3 落在 emoji 内 → clamp 到其后的 char 边界（col 3）。
        assert_eq!(utf16_offset_to_point(&b, 3), p(0, 3));
        assert_eq!(utf16_offset_to_point(&b, 5), p(1, 0));
        assert_eq!(utf16_offset_to_point(&b, 6), p(1, 1));
    }

    #[test]
    fn utf16_clamp_to_document_end() {
        let b = Buffer::from_plain_text("ab");
        // 越界偏移 clamp 到文尾。
        assert_eq!(utf16_offset_to_point(&b, 999), p(0, 2));
    }

    /// 状态标签语义（构造 Editor 全字段有 App 依赖，标签逻辑钉定在
    /// Buffer 纯逻辑层——此处验证组合格式）。
    #[test]
    fn label_format_semantics() {
        let mut b = Buffer::from_plain_text("中文字");
        let label = |b: &Buffer| {
            let c = b.cursor();
            format!("{}, {}", c.line + 1, c.col + 1)
        };
        assert_eq!(label(&b), "1, 1");
        b.set_cursor(p(0, 2));
        assert_eq!(label(&b), "1, 3");
        assert_eq!(format!("{} 字符", b.char_count()), "3 字符");
        assert!(!b.is_modified());
        b.insert("新增");
        assert!(b.is_modified());
    }
}
