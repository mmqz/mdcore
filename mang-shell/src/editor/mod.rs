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
//! ## 已知边界（批 1 登记，#68 后续批补）
//!
//! - 鼠标点击仅聚焦（不定位行列）；拖选未做（Shift+方向键选区可用）
//! - 行高统一（md 标题不放大字号——用粗体+色区分）
//! - 光标不闪烁（恒显）
//! - 保存为同步 `fs::write`（P2-5 异步化设计已有，暂不接线）
//! - IME 候选框定位 `bounds_for_range` 返回 `None`（精化登记阶段 7）

pub mod buffer;
pub mod find_bar;
pub mod highlight;

use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler,
    FocusHandle, FontStyle, FontWeight, HighlightStyle, IntoElement, KeyDownEvent, Pixels, Render,
    ScrollStrategy, SharedString, StyledText, TextStyle, UTF16Selection, UnderlineStyle,
    UniformListScrollHandle, Window, canvas, px, uniform_list,
};
use theme::ThemeColors;
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

use buffer::{Buffer, Point};
use find_bar::{FindKeyOutcome, FindState};
use highlight::{TokenKind, highlight_line};

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
        };
        this.refresh_caches();
        // 初始聚焦由 ShellApp::new 完成（需要 window 上下文）。
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

    /// 保存（Ctrl+S）：同步写盘 + 清修改标记。未命名文档无路径——
    /// 批 1 无「另存为」对话框，返回 false 登记。
    pub fn save(&mut self) -> bool {
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
        // fence：逐行推进。
        let mut states = Vec::with_capacity(self.buffer.line_count());
        let mut fence = false;
        for i in 0..self.buffer.line_count() {
            states.push(fence);
            fence = highlight_line(self.buffer.line(i), fence).fence_at_end;
        }
        self.fence_cache = states;
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

    /// 编辑区主体：焦点域 + 键盘 + 虚拟化行列表 + IME 接线。
    fn render_editor_area(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let line_count = self.buffer.line_count();
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let weak = entity.downgrade();
        let ime_entity = entity.clone();

        div()
            .id("editor-surface")
            .size_full()
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::dispatch_key))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _event: &gpui::MouseDownEvent, window, cx| {
                    // 点击聚焦（批 1：点击不定位行列——登记后续批）。
                    window.focus(&this.focus, cx);
                    cx.notify();
                }),
            )
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
            // IME 接线：paint 期把 ElementInputHandler 绑到编辑区焦点域
            //（canvas size_0 不占布局——仅借 paint 时机与 bounds）。
            .child(div().id("ime-hook").absolute().size_0().child(canvas(
                move |bounds, _window, _cx| bounds,
                move |bounds: Bounds<Pixels>, _t, window: &mut Window, cx: &mut App| {
                    let focus = ime_entity.read(cx).focus_handle().clone();
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, ime_entity.clone()),
                        cx,
                    );
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

        (visible.start..visible.end)
            .map(|idx| {
                let line = self.buffer.line(idx).to_string();
                let tokens = highlight_line(&line, self.fence_at(idx));
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

/// 编辑器正文 TextStyle（Sarasa Mono SC 15px / 24px 行高；shape 定宽用）。
fn editor_text_style(colors: &ThemeColors) -> TextStyle {
    TextStyle {
        color: colors.text,
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
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // 鼠标位置 → 字符索引（iOS/Android 文本交互面；桌面批 1 不需要）。
        None
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
