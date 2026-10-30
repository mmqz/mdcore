//! 编辑缓冲区 —— 纯逻辑文本模型（#68 阶段 1.2 文本编辑能力的地基；
//! r42 阶段 A 换芯：存储层 Zed `text::Buffer`（rope + 锚点 + O(log n)））。
//!
//! ## r42 阶段 A 换芯说明（#68 Lead 定调：抄设计，用干净 crate）
//!
//! - **存储层**：`text::Buffer`（Zed 编辑器同款——rope 持文 + 事务历史 +
//!   锚点基座）。编辑走 `inner.edit([(offset_range, text)])` 单点，
//!   O(log n)——替换前实现（`Vec<String>` 逐行 drain/insert，O(n)）。
//! - **语义层外壳**：光标/选区/粘性列/undo-redo 四元组/查找全部保持
//!   r39 语义不变——41 个既有单测逐一照抄零改写（公开 API 语义即契约）。
//!   text::Buffer 内建的 undo 基础设施不用（外壳四元组与既有测试/光标
//!   语义一一锚定；锚点能力在 inner 上保留，阶段 C 多光标取用）。
//! - **坐标语义**：对外 `Point { line, col }` 的 `col` 仍按 **char 索引**
//!   （CJK 列语义不变）；rope 层坐标（byte 列）由外壳双向换算，调用方
//!   无感。`col` 换算需行文本（O(行长)）——行文本每次调用即时取
//!   （`row_text`），不缓存；50 万字场景的可见行操作 O(行长) 量级
//!   （阶段 B display_map 落地前的边界登记，见 PR 说明）。
//! - **行尾语义**：`\r\n` 原样存取（`new_normalized` 直通，不经
//!   `Buffer::new` 的 normalize——mang-shell 行语义只按 `\n` 划行，
//!   `\r` 属行内容，与 r39 `split('\n')` 语义逐位一致）。
//!
//! ## 坐标语义（r39 原文保持）
//!
//! - `Point { line, col }`：`col` 一律按 **char 索引** 计（非字节）——
//!   CJK 语义安全：一个汉字占一列。
//! - 文档至少一行（空文档 = 单空行）；行不含换行符。
//! - `desired_col`：上下移动的「期望列」粘性——经过短行 clamp 后回到
//!   长行时恢复原列（经典编辑器语义）。
//!
//! ## 编辑原语
//!
//! 所有文本变更收敛到 [`Buffer::replace_range`] 单点（内部走
//! [`Buffer::apply_core`] → `inner.edit`）：删除 `[start, end)` 再插入
//! `text`（可含 `\n` 拆行）。
//!
//! undo/redo 以四元组 `(start, end_del, old_text, new_text)` 记录：
//! redo 方向 = `[start, end_del)` 的 `old` 换成 `new`；undo 方向 = 把
//! `new` 的当前占位换回 `old`（占位末点由 `end_after_insert` 重算，
//! 不依赖 `end_del`）。栈全空 ⇔ 内容等于初始（每步可逆）——
//! `is_modified` 由此判定，无需快照比对。
//!
//! ## 选区
//!
//! `anchor`（Shift 移动落下的锚点）与 `cursor`（头）构成选区；
//! 排序后 `[start, end)`。无 Shift 的移动清锚。编辑命令先消费选区。

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use text as zed_text;

/// 文本坐标：`line` 从 0 起，`col` 为行内 **char** 索引。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    pub line: usize,
    pub col: usize,
}

impl Point {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// 一次可逆编辑（四元组）：
/// - redo 方向：`[start, end_del)` 的 `old_text` 替换为 `new_text`
/// - undo 方向：`new_text` 在 `start` 处的当前占位（由 `end_after_insert`
///   重算）替换回 `old_text`——undo 不读 `end_del`。
#[derive(Clone, Debug)]
struct Edit {
    start: Point,
    end_del: Point,
    old_text: String,
    new_text: String,
}

/// 词字符判定（`word_jump` 与双击选词 [`super::mouse::word_range_at`] 共用
/// 口径：字母数字（含 CJK 汉字——连续汉字算一个词）+ 下划线）。
pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// char 索引 → 字节偏移（越界 clamp 到末尾）。
fn char_to_byte(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(b, _)| b).unwrap_or(s.len())
}

/// [`char_to_byte`] 的模块内公开转发（display_map 的段文本切片消费）。
pub(crate) fn char_to_byte_pub(s: &str, col: usize) -> usize {
    char_to_byte(s, col)
}

/// 外壳 BufferId 序列（本地无协同——id 唯一性只为 text 层语义完整；
/// `BufferId::new` 对非零恒 Ok，`expect` 消息写明）。
fn next_buffer_id() -> zed_text::BufferId {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    zed_text::BufferId::new(n).expect("BufferId::new 对非零 id 恒 Ok")
}

/// 纯逻辑编辑缓冲区。
pub struct Buffer {
    /// 存储层：Zed text::Buffer（rope 持文 + 锚点 + 事务）。
    /// 外壳不消费其 undo 历史（语义层四元组自管）；每次 `apply_core`
    /// 的 edit 会同步落入 inner 的 history（无害堆积，见头注释）。
    inner: zed_text::Buffer,
    cursor: Point,
    /// Shift 选区锚点（无选区为 `None`）。
    anchor: Option<Point>,
    /// 上下移动的期望列粘性（被短行 clamp 时记忆原列）。
    desired_col: Option<usize>,
    undo_stack: Vec<Edit>,
    redo_stack: Vec<Edit>,
    /// 保存点对应的 undo 栈深度（`mark_saved` 快照）。
    /// 修改判定 = 当前深度 != 保存点深度——undo 到底即回初始态（redo
    /// 栈非空不代表内容偏离初始）。
    saved_depth: usize,
}

impl Clone for Buffer {
    /// 深拷贝经 `inner.branch()`（text 层同构快照——克隆体与源共享
    /// 历史，编辑互不影响；`text::Buffer` 未派生 Clone，branch 是
    /// 上游提供的唯一全量复制路径）。
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.branch(),
            cursor: self.cursor,
            anchor: self.anchor,
            desired_col: self.desired_col,
            undo_stack: self.undo_stack.clone(),
            redo_stack: self.redo_stack.clone(),
            saved_depth: self.saved_depth,
        }
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::from_plain_text("")
    }
}

impl std::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
            .field("lines", &self.line_count())
            .field("chars", &self.char_count())
            .field("cursor", &self.cursor)
            .field("modified", &self.is_modified())
            .finish_non_exhaustive()
    }
}

impl Buffer {
    // ---------------------------------------------------------------------------
    // 构造与序列化
    // ---------------------------------------------------------------------------

    /// 从纯文本构造（行语义按 `\n` 划分；空文本 = 单空行）。
    /// 经 `new_normalized` 直通——不走 `Buffer::new` 的 `\r\n`→`\n`
    /// normalize（mang-shell 行语义中 `\r` 属行内容，roundtrip 逐位保真）。
    pub fn from_plain_text(text: &str) -> Self {
        let ending = zed_text::LineEnding::detect(text);
        let rope = zed_text::Rope::from(text);
        let inner = zed_text::Buffer::new_normalized(
            zed_text::ReplicaId::LOCAL,
            next_buffer_id(),
            ending,
            rope,
        );
        Self {
            inner,
            cursor: Point::default(),
            anchor: None,
            desired_col: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            saved_depth: 0,
        }
    }

    /// 序列化为纯文本（`\r\n` 等行尾原样——与构造逐位互逆）。
    pub fn to_plain_text(&self) -> String {
        self.snap().text()
    }

    // ---------------------------------------------------------------------------
    // 只读访问
    // ---------------------------------------------------------------------------

    /// 行数。
    pub fn line_count(&self) -> usize {
        self.snap().max_point().row as usize + 1
    }

    /// 指定行内容（越界返回空串——渲染层免判）。
    /// 返回 `String`（rope 无行借用面）；行长场景的分配开销见头注释。
    pub fn line(&self, line: usize) -> String {
        self.row_text(line as u32)
    }

    /// 指定行 char 数。
    pub fn line_len(&self, line: usize) -> usize {
        self.line(line).chars().count()
    }

    /// 光标点。
    pub fn cursor(&self) -> Point {
        self.cursor
    }

    /// 选区（anchor 与 cursor 排序后的 `[start, end)`；无选区 `None`）。
    pub fn selection(&self) -> Option<std::ops::Range<Point>> {
        self.anchor.map(|a| {
            if a <= self.cursor {
                a..self.cursor
            } else {
                self.cursor..a
            }
        })
    }

    /// 选区文本（含跨行 `\n`；无选区空串）。
    pub fn selection_text(&self) -> String {
        match self.selection() {
            None => String::new(),
            Some(range) => self.range_text(&range),
        }
    }

    /// 全文 char 总数（可见字符 + 换行；状态栏字数——CJK 逐字语义天然成立）。
    pub fn char_count(&self) -> usize {
        self.to_plain_text().chars().count()
    }

    /// `[start, end)` 范围文本（跨行以 `\n` 连接；两点自动排序）。
    pub fn range_text(&self, range: &std::ops::Range<Point>) -> String {
        let Range { start, end } = self.ordered_range(range.clone());
        if start == end {
            return String::new();
        }
        if start.line == end.line {
            let s = self.line(start.line);
            let b0 = char_to_byte(&s, start.col);
            let b1 = char_to_byte(&s, end.col);
            return s[b0..b1].to_string();
        }
        let mut out = String::new();
        // 首行尾段
        let first = self.line(start.line);
        out.push_str(&first[char_to_byte(&first, start.col)..]);
        // 中间整行
        for l in start.line + 1..end.line {
            out.push('\n');
            out.push_str(&self.line(l));
        }
        // 末行头段
        out.push('\n');
        let last = self.line(end.line);
        out.push_str(&last[..char_to_byte(&last, end.col)]);
        out
    }

    /// 是否相对最近保存点已修改（当前 undo 深度 != 保存点深度）。
    pub fn is_modified(&self) -> bool {
        self.undo_stack.len() != self.saved_depth
    }

    /// 显式落盘：记录保存点深度（redo 历史随之清空——保存点之后
    /// 不跨保存点 undo，与多数编辑器一致）。
    pub fn mark_saved(&mut self) {
        self.saved_depth = self.undo_stack.len();
        self.redo_stack.clear();
    }

    // ---------------------------------------------------------------------------
    // 编辑原语
    // ---------------------------------------------------------------------------

    /// **唯一文本变更入口**：删除 `[start, end)` 再插入 `text`（可含 `\n`）。
    /// 光标落在新插入文本末尾；清选区与列粘性；清 redo 栈。
    pub fn replace_range(&mut self, start: Point, end: Point, text: &str) {
        let Range { start, end } = self.ordered_range(start..end);
        let old_text = self.range_text(&(start..end));
        let cursor = self.apply_core(start, end, text);
        self.undo_stack.push(Edit {
            start,
            end_del: end,
            old_text,
            new_text: text.to_string(),
        });
        self.redo_stack.clear();
        self.anchor = None;
        self.desired_col = None;
        self.cursor = cursor;
        self.clamp_cursor();
    }

    /// 在光标处插入文本（有选区则先被替换）。
    pub fn insert(&mut self, text: &str) {
        let (s, e) = self.deletion_range();
        self.replace_range(s, e, text);
    }

    /// 换行（Enter）。
    pub fn newline(&mut self) {
        self.insert("\n");
    }

    /// Tab（md 最小语义：插 4 空格，不引入缩进块模型）。
    pub fn tab(&mut self) {
        self.insert("    ");
    }

    /// 退格（Backspace）：选区删除；行首与前行合并；否则删光标前一字符。
    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let c = self.cursor;
        if c.col > 0 {
            let s = Point::new(c.line, c.col - 1);
            self.replace_range(s, c, "");
        } else if c.line > 0 {
            // 行首：与前一行合并（删两行间的隐式换行）。
            let s = Point::new(c.line - 1, self.line_len(c.line - 1));
            self.replace_range(s, c, "");
        }
    }

    /// 前删（Delete）：选区删除；行尾与下行合并；否则删光标后一字符。
    pub fn delete_forward(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let c = self.cursor;
        if c.col < self.line_len(c.line) {
            let e = Point::new(c.line, c.col + 1);
            self.replace_range(c, e, "");
        } else if c.line + 1 < self.line_count() {
            let e = Point::new(c.line + 1, 0);
            self.replace_range(c, e, "");
        }
    }

    /// 删除当前选区（无选区无操作）。
    pub fn delete_selection(&mut self) {
        if let Some(range) = self.selection() {
            self.replace_range(range.start, range.end, "");
        }
    }

    /// 当前编辑命令的删除范围：有选区取选区，无选区取光标零宽。
    fn deletion_range(&self) -> (Point, Point) {
        match self.selection() {
            Some(r) => (r.start, r.end),
            None => (self.cursor, self.cursor),
        }
    }

    // ---------------------------------------------------------------------------
    // undo / redo
    // ---------------------------------------------------------------------------

    /// 撤销一步；返回是否发生撤销。
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo_stack.pop() else {
            return false;
        };
        // 逆方向：把 new_text 的当前占位换回 old_text。
        let end_new = self.end_after_insert(&edit.start, &edit.new_text);
        self.apply_core(edit.start, end_new, &edit.old_text);
        // redo 方向的 Edit：old_text 恢复后的占位范围即 end_del。
        self.redo_stack.push(Edit {
            start: edit.start,
            end_del: self.end_after_insert(&edit.start, &edit.old_text),
            old_text: edit.old_text,
            new_text: edit.new_text,
        });
        self.cursor = edit.start;
        self.anchor = None;
        self.desired_col = None;
        self.clamp_cursor();
        true
    }

    /// 重做一步；返回是否发生重做。
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo_stack.pop() else {
            return false;
        };
        // 重放方向：[start, end_del) 的 old_text 换成 new_text。
        let new_cursor = self.end_after_insert(&edit.start, &edit.new_text);
        self.apply_core(edit.start, edit.end_del, &edit.new_text);
        self.undo_stack.push(edit);
        self.cursor = new_cursor;
        self.anchor = None;
        self.desired_col = None;
        self.clamp_cursor();
        true
    }

    /// 核心变更（不落外壳栈，由调用方组装 undo/redo 语义）：
    /// 删除 `[start, end)` 插入 `text`——单次 `inner.edit`（O(log n)）。
    /// 返回插入文本末尾点（即新光标；纯算术，不读 inner）。
    fn apply_core(&mut self, start: Point, end: Point, text: &str) -> Point {
        debug_assert!(start <= end, "apply_core: start <= end（调用方已排序）");
        let zstart = self.point_to_zoffset(start);
        let zend = self.point_to_zoffset(end);
        // inner 的 history 同步记录（外壳不消费；见头注释——r42 边界登记）
        let _ops = self.inner.edit([(zstart..zend, text.to_string())]);
        self.end_after_insert(&start, text)
    }

    /// 在 `start` 插入 `text` 后的终点（undo / redo 的占位末点重算）。
    fn end_after_insert(&self, start: &Point, text: &str) -> Point {
        let newlines = text.matches('\n').count();
        if newlines == 0 {
            Point::new(start.line, start.col + text.chars().count())
        } else {
            let last = text.rsplit('\n').next().unwrap_or("");
            Point::new(start.line + newlines, last.chars().count())
        }
    }

    /// [`Buffer::end_after_insert`] 的模块内公开转发（multi_cursor 的
    /// 多点插入尾部定位消费——同一公式保证与 Buffer 光标语义一致）。
    pub(crate) fn end_after_insert_pub(&self, start: &Point, text: &str) -> Point {
        self.end_after_insert(start, text)
    }

    // ---------------------------------------------------------------------------
    // 光标移动
    // ---------------------------------------------------------------------------

    /// 左移一字符（行首回退到上一行行尾）。`extend`：Shift 选区。
    /// 无 Shift 且有选区：先收点到选区头（一次取消语义）。
    pub fn move_left(&mut self, extend: bool) {
        if !extend && let Some(r) = self.selection() {
            self.set_cursor(r.start);
            return;
        }
        self.begin_move(extend);
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
        }
        self.desired_col = None;
    }

    /// 右移一字符（行尾进下一行行首）。
    pub fn move_right(&mut self, extend: bool) {
        if !extend && let Some(r) = self.selection() {
            self.set_cursor(r.end);
            return;
        }
        self.begin_move(extend);
        if self.cursor.col < self.line_len(self.cursor.line) {
            self.cursor.col += 1;
        } else if self.cursor.line + 1 < self.line_count() {
            self.cursor.line += 1;
            self.cursor.col = 0;
        }
        self.desired_col = None;
    }

    /// 上移一行（列 clamp + 粘性）。
    pub fn move_up(&mut self, extend: bool) {
        self.begin_move(extend);
        if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.apply_desired_col();
        }
    }

    /// 下移一行（列 clamp + 粘性）。
    pub fn move_down(&mut self, extend: bool) {
        self.begin_move(extend);
        if self.cursor.line + 1 < self.line_count() {
            self.cursor.line += 1;
            self.apply_desired_col();
        }
    }

    /// 行首（Home）。
    pub fn move_home(&mut self, extend: bool) {
        self.begin_move(extend);
        self.cursor.col = 0;
        self.desired_col = None;
    }

    /// 行尾（End）。
    pub fn move_end(&mut self, extend: bool) {
        self.begin_move(extend);
        self.cursor.col = self.line_len(self.cursor.line);
        self.desired_col = None;
    }

    /// 文首（Ctrl+Home）。
    pub fn move_doc_start(&mut self, extend: bool) {
        self.begin_move(extend);
        self.cursor = Point::new(0, 0);
        self.desired_col = None;
    }

    /// 文尾（Ctrl+End）。
    pub fn move_doc_end(&mut self, extend: bool) {
        self.begin_move(extend);
        let last = self.line_count() - 1;
        self.cursor = Point::new(last, self.line_len(last));
        self.desired_col = None;
    }

    /// 左跳一词（Ctrl+Left）：先跨非词字符，再吃掉词体；行首再退上一行行尾。
    pub fn move_word_left(&mut self, extend: bool) {
        self.begin_move(extend);
        let mut p = self.cursor;
        let text = self.line(p.line);
        let chars: Vec<char> = text.chars().collect();
        while p.col > 0 && !is_word_char(chars[p.col - 1]) {
            p.col -= 1;
        }
        while p.col > 0 && is_word_char(chars[p.col - 1]) {
            p.col -= 1;
        }
        if p.col == 0 && p.line > 0 {
            p.line -= 1;
            p.col = self.line_len(p.line);
        }
        self.cursor = p;
        self.desired_col = None;
    }

    /// 右跳一词（Ctrl+Right）：先跨非词字符，再吃掉词体；行尾再进下一行行首。
    pub fn move_word_right(&mut self, extend: bool) {
        self.begin_move(extend);
        let mut p = self.cursor;
        let text = self.line(p.line);
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        while p.col < len && !is_word_char(chars[p.col]) {
            p.col += 1;
        }
        while p.col < len && is_word_char(chars[p.col]) {
            p.col += 1;
        }
        if p.col == len && p.line + 1 < self.line_count() {
            p.line += 1;
            p.col = 0;
        }
        self.cursor = p;
        self.desired_col = None;
    }

    /// 移动起点纪律：Shift 落锚（无锚时），无 Shift 清锚。
    fn begin_move(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    /// 上下行移动后的列 clamp 与粘性：
    /// 期望列 = 粘性记忆 ?? 当前列；超短行则 clamp 并记忆，否则应用并清记忆。
    fn apply_desired_col(&mut self) {
        let len = self.line_len(self.cursor.line);
        let want = self.desired_col.unwrap_or(self.cursor.col);
        if want > len {
            self.desired_col = Some(want);
            self.cursor.col = len;
        } else {
            self.cursor.col = want;
            self.desired_col = None;
        }
    }

    /// 直接设置光标（清锚与粘性；clamp 防御）。
    pub fn set_cursor(&mut self, p: Point) {
        self.cursor = p;
        self.anchor = None;
        self.desired_col = None;
        self.clamp_cursor();
    }

    /// 全选（Ctrl+A）：锚在文首、光标到文尾。
    pub fn select_all(&mut self) {
        let last = self.line_count() - 1;
        self.anchor = Some(Point::new(0, 0));
        self.cursor = Point::new(last, self.line_len(last));
    }

    /// 直接设置选区（锚点 + 光标头；查找跳转等程序化选中用）。
    pub fn set_selection(&mut self, anchor: Point, cursor: Point) {
        self.anchor = Some(anchor);
        self.cursor = cursor;
        self.desired_col = None;
        self.clamp_cursor();
    }

    /// 光标 clamp 到合法域（外部 set 后保证不变量）。
    fn clamp_cursor(&mut self) {
        if self.line_count() == 0 {
            return; // inner 恒非空；防御早退（不可达分支）
        }
        if self.cursor.line >= self.line_count() {
            self.cursor.line = self.line_count() - 1;
        }
        let len = self.line_len(self.cursor.line);
        if self.cursor.col > len {
            self.cursor.col = len;
        }
    }

    // ---------------------------------------------------------------------------
    // 查找（Ctrl+F）
    // ---------------------------------------------------------------------------

    /// 全文大小写不敏感查找：返回所有匹配起点（按文档序；空 query 空）。
    pub fn find_matches(&self, query: &str) -> Vec<Point> {
        if query.is_empty() {
            return Vec::new();
        }
        let needle: Vec<char> = query.to_lowercase().chars().collect();
        let mut hits = Vec::new();
        for li in 0..self.line_count() {
            let line = self.line(li);
            let lower: Vec<char> = line
                .chars()
                .map(|c| c.to_lowercase().next().unwrap_or(c))
                .collect();
            if lower.len() < needle.len() {
                continue;
            }
            for start in 0..=lower.len() - needle.len() {
                if lower[start..start + needle.len()] == needle[..] {
                    hits.push(Point::new(li, start));
                }
            }
        }
        hits
    }

    // ---------------------------------------------------------------------------
    // 内部工具（rope 坐标换算层——外壳 char 语义 <-> rope byte 列）
    // ---------------------------------------------------------------------------

    /// 存储层快照（借用 inner）。
    fn snap(&self) -> &zed_text::BufferSnapshot {
        self.inner.snapshot()
    }

    /// 行文本（rope 按 `\n` 划行；`\r` 属行内容原样；越界空串）。
    fn row_text(&self, row: u32) -> String {
        if row >= self.line_count() as u32 {
            return String::new();
        }
        let snap = self.snap();
        let start = snap.point_to_offset(zed_text::Point::new(row, 0));
        let len = snap.line_len(row) as usize;
        if len == 0 {
            return String::new();
        }
        snap.text_for_range(start..start + len).collect()
    }

    /// mang `Point`（char 列）→ rope 全文 offset（字节）。
    /// 输入先 clamp 到文档域（外部调用可传越界点——防御性收窄，
    /// 旧实现对越界索引直接 panic，此为语义超集）。
    fn point_to_zoffset(&self, p: Point) -> usize {
        let rows = self.line_count();
        let line = p.line.min(rows.saturating_sub(1));
        let col = p.col.min(self.line_len(line));
        let text = self.line(line);
        let byte_col = char_to_byte(&text, col);
        self.snap()
            .point_to_offset(zed_text::Point::new(line as u32, byte_col as u32))
    }

    /// 两点排序（start <= end）。
    fn ordered_range(&self, r: Range<Point>) -> Range<Point> {
        if r.start <= r.end { r } else { r.end..r.start }
    }
}

// =============================================================================
// 单元测试 —— 纯逻辑全绿是本模块的交付门
// （r39 的 41 个测试逐字迁移（公开 API 语义即契约）；r42 追加坐标层/换芯专项）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 快捷构造点。
    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }

    // ---------------- 构造与序列化 ----------------

    #[test]
    fn plain_text_roundtrip() {
        let cases = ["", "hello", "a\nb\nc", "中文\nEnglish\n混合 line", "\n\n"];
        for t in cases {
            let b = Buffer::from_plain_text(t);
            assert_eq!(b.to_plain_text(), t, "roundtrip: {t:?}");
        }
    }

    #[test]
    fn crlf_roundtrip_bit_fidelity() {
        // r42 专项：\r\n 属行内容原样存取（不走 Buffer::new 的 normalize）
        let b = Buffer::from_plain_text("a\r\nb\rc");
        assert_eq!(b.to_plain_text(), "a\r\nb\rc");
        assert_eq!(b.line_count(), 2, "\\n 划行（1 个 \\n）：\\r 留行内");
        assert_eq!(b.line(0), "a\r");
        assert_eq!(b.line(1), "b\rc");
    }

    #[test]
    fn empty_is_single_empty_line() {
        let b = Buffer::from_plain_text("");
        assert_eq!(b.line_count(), 1);
        assert_eq!(b.line(0), "");
    }

    #[test]
    fn trailing_newline_keeps_empty_last_line() {
        // r42 专项：末行 \n → 尾空行保位（旧 split 语义 = rope max_point 语义）
        let b = Buffer::from_plain_text("a\n");
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.line(1), "");
        assert_eq!(b.to_plain_text(), "a\n");
    }

    // ---------------- 光标移动矩阵 ----------------

    #[test]
    fn move_left_wraps_to_prev_line_tail() {
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.set_cursor(p(1, 0));
        b.move_left(false);
        assert_eq!(b.cursor(), p(0, 2), "第二行行首左移 → 第一行行尾");
        b.move_left(false);
        b.move_left(false);
        assert_eq!(b.cursor(), p(0, 0));
        b.move_left(false);
        assert_eq!(b.cursor(), p(0, 0), "文首左移不动");
    }

    #[test]
    fn move_right_wraps_to_next_line_head() {
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.set_cursor(p(0, 2));
        b.move_right(false);
        assert_eq!(b.cursor(), p(1, 0), "第一行行尾右移 → 第二行行首");
        b.move_right(false);
        b.move_right(false);
        assert_eq!(b.cursor(), p(1, 2));
        b.move_right(false);
        assert_eq!(b.cursor(), p(1, 2), "文尾右移不动");
    }

    #[test]
    fn move_up_down_clamps_and_sticks() {
        let mut b = Buffer::from_plain_text("abcdef\nxy\nabcdefgh");
        // 从第 2 行 col 6 上移到短行 "xy"（len 2）→ clamp 2，记忆 6
        b.set_cursor(p(2, 6));
        b.move_up(false);
        assert_eq!(b.cursor(), p(1, 2), "短行 clamp");
        // 再上移到长行 → 恢复记忆列 6
        b.move_up(false);
        assert_eq!(b.cursor(), p(0, 6), "粘性列恢复");
    }

    #[test]
    fn home_end_doc_bounds() {
        let mut b = Buffer::from_plain_text("ab\ncd\n ef");
        b.set_cursor(p(1, 1));
        b.move_home(false);
        assert_eq!(b.cursor(), p(1, 0));
        b.move_end(false);
        assert_eq!(b.cursor(), p(1, 2));
        b.move_doc_start(false);
        assert_eq!(b.cursor(), p(0, 0));
        b.move_doc_end(false);
        assert_eq!(b.cursor(), p(2, 3));
    }

    #[test]
    fn cjk_columns_are_char_indexed() {
        let mut b = Buffer::from_plain_text("中文编辑");
        assert_eq!(b.line_len(0), 4, "四个汉字 = 4 列");
        b.set_cursor(p(0, 2));
        b.move_right(false);
        assert_eq!(b.cursor(), p(0, 3));
        b.insert("x");
        assert_eq!(b.line(0), "中文编x辑");
    }

    // ---------------- 编辑原语 ----------------

    #[test]
    fn insert_mid_line() {
        let mut b = Buffer::from_plain_text("ac");
        b.set_cursor(p(0, 1));
        b.insert("b");
        assert_eq!(b.to_plain_text(), "abc");
        assert_eq!(b.cursor(), p(0, 2));
    }

    #[test]
    fn insert_multiline_splits() {
        let mut b = Buffer::from_plain_text("abc");
        b.set_cursor(p(0, 1));
        b.insert("X\nY\nZ");
        assert_eq!(b.to_plain_text(), "aX\nY\nZbc");
        assert_eq!(b.cursor(), p(2, 1));
        assert_eq!(b.line_count(), 3);
    }

    #[test]
    fn newline_splits_line() {
        let mut b = Buffer::from_plain_text("ab");
        b.set_cursor(p(0, 1));
        b.newline();
        assert_eq!(b.to_plain_text(), "a\nb");
        assert_eq!(b.cursor(), p(1, 0));
    }

    #[test]
    fn backspace_merges_lines() {
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.set_cursor(p(1, 0));
        b.backspace();
        assert_eq!(b.to_plain_text(), "abcd");
        assert_eq!(b.cursor(), p(0, 2));
    }

    #[test]
    fn backspace_deletes_char() {
        let mut b = Buffer::from_plain_text("abc");
        b.set_cursor(p(0, 2));
        b.backspace();
        assert_eq!(b.to_plain_text(), "ac");
    }

    #[test]
    fn backspace_cjk_single_column() {
        let mut b = Buffer::from_plain_text("中文字");
        b.set_cursor(p(0, 1));
        b.backspace();
        assert_eq!(b.to_plain_text(), "文字", "一个退格删一个汉字");
    }

    #[test]
    fn delete_forward_merges_lines() {
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.set_cursor(p(0, 2));
        b.delete_forward();
        assert_eq!(b.to_plain_text(), "abcd");
        assert_eq!(b.cursor(), p(0, 2));
    }

    #[test]
    fn delete_forward_deletes_char() {
        let mut b = Buffer::from_plain_text("abc");
        b.set_cursor(p(0, 0));
        b.delete_forward();
        assert_eq!(b.to_plain_text(), "bc");
    }

    #[test]
    fn tab_inserts_four_spaces() {
        let mut b = Buffer::from_plain_text("");
        b.tab();
        assert_eq!(b.to_plain_text(), "    ");
    }

    #[test]
    fn replace_range_cross_line_middle() {
        let mut b = Buffer::from_plain_text("aa\nbb\ncc\ndd");
        // 删 (0,1)..(3,1)（"a\nbb\ncc\nd"）换 "X\nY"
        b.replace_range(p(0, 1), p(3, 1), "X\nY");
        assert_eq!(b.to_plain_text(), "aX\nYd");
        assert_eq!(b.cursor(), p(1, 1));
    }

    #[test]
    fn replace_range_out_of_bounds_is_clamped() {
        // r42 专项：越界点防御性 clamp（旧实现 panic 面，语义超集）。
        // "ab\ncd"：p(0,99)→(0,2)，p(99,99)→(1,2)；即 (0,2)..(1,2)
        // 的 "\ncd" 被换成 "X" → "abX"。
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.replace_range(p(0, 99), p(99, 99), "X");
        assert_eq!(b.to_plain_text(), "abX");
        assert_eq!(b.cursor(), p(0, 3), "插入末尾点（start=(0,2) + 1 char）");
    }

    // ---------------- 选区 ----------------

    #[test]
    fn shift_move_creates_selection() {
        let mut b = Buffer::from_plain_text("hello world");
        b.set_cursor(p(0, 6));
        b.move_right(true);
        b.move_right(true);
        let sel = b.selection().expect("有选区");
        assert_eq!((sel.start, sel.end), (p(0, 6), p(0, 8)));
        assert_eq!(b.selection_text(), "wo");
        // 无 Shift 移动 → 收点到选区头
        b.move_left(false);
        assert_eq!(b.cursor(), p(0, 6));
        assert!(b.selection().is_none());
    }

    #[test]
    fn selection_replaced_by_insert() {
        let mut b = Buffer::from_plain_text("hello world");
        b.set_cursor(p(0, 6));
        for _ in 0..5 {
            b.move_right(true);
        }
        b.insert("GPUI");
        assert_eq!(b.to_plain_text(), "hello GPUI");
        assert!(b.selection().is_none(), "编辑后选区清空");
    }

    #[test]
    fn selection_spanning_lines() {
        let mut b = Buffer::from_plain_text("ab\ncd\nef");
        b.set_cursor(p(0, 1));
        b.move_down(true);
        b.move_down(true);
        let sel = b.selection().unwrap();
        assert_eq!(b.range_text(&sel), "b\ncd\ne");
    }

    #[test]
    fn select_all_and_delete() {
        let mut b = Buffer::from_plain_text("a\nb\nc");
        b.select_all();
        assert_eq!(b.selection_text(), "a\nb\nc");
        b.delete_selection();
        assert_eq!(b.to_plain_text(), "");
        assert_eq!(b.line_count(), 1, "删空后仍保单空行");
    }

    // ---------------- undo / redo ----------------

    #[test]
    fn undo_redo_roundtrip_simple() {
        let mut b = Buffer::from_plain_text("abc");
        b.set_cursor(p(0, 3));
        b.insert("def");
        assert_eq!(b.to_plain_text(), "abcdef");
        assert!(b.is_modified());
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "abc");
        assert!(!b.is_modified(), "undo 到底 = 回初始，修改标记清");
        assert!(b.redo());
        assert_eq!(b.to_plain_text(), "abcdef");
        assert!(b.is_modified(), "redo 后修改标记复立");
    }

    #[test]
    fn undo_restores_cursor_and_multiline() {
        let mut b = Buffer::from_plain_text("one two");
        b.set_cursor(p(0, 3)); // col3 = 空格：拆行后第二行 " two"
        b.newline();
        b.insert("three");
        assert_eq!(b.to_plain_text(), "one\nthree two");
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "one\n two");
        assert_eq!(b.cursor(), p(1, 0), "undo 后光标回该次编辑起点");
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "one two");
        assert_eq!(b.cursor(), p(0, 3), "第二次 undo 光标回首个编辑起点");
        assert!(!b.undo(), "栈空再 undo 返回 false");
        assert!(b.redo());
        assert_eq!(b.to_plain_text(), "one\n two");
        assert_eq!(b.cursor(), p(1, 0), "redo 光标在插入末尾");
    }

    #[test]
    fn undo_backspace_merge() {
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.set_cursor(p(1, 0));
        b.backspace();
        assert_eq!(b.to_plain_text(), "abcd");
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "ab\ncd");
        assert_eq!(b.cursor(), p(0, 2), "undo 合并 → 光标回上行行尾");
    }

    #[test]
    fn undo_replacement_not_just_insert() {
        let mut b = Buffer::from_plain_text("hello world");
        b.set_cursor(p(0, 6));
        for _ in 0..5 {
            b.move_right(true);
        }
        b.insert("GPUI");
        assert_eq!(b.to_plain_text(), "hello GPUI");
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "hello world", "undo 恢复被替换的 world");
        assert!(b.redo());
        assert_eq!(b.to_plain_text(), "hello GPUI");
    }

    #[test]
    fn undo_cross_line_replace() {
        let mut b = Buffer::from_plain_text("aa\nbb\ncc\ndd");
        b.replace_range(p(0, 1), p(3, 1), "X\nY");
        assert_eq!(b.to_plain_text(), "aX\nYd");
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "aa\nbb\ncc\ndd", "跨行替换完整可逆");
        assert!(b.redo());
        assert_eq!(b.to_plain_text(), "aX\nYd");
    }

    #[test]
    fn edit_after_undo_clears_redo() {
        let mut b = Buffer::from_plain_text("a");
        b.insert("b");
        assert!(b.undo());
        b.insert("c");
        assert!(!b.redo(), "新编辑后 redo 栈清空");
        assert_eq!(
            b.to_plain_text(),
            "ca",
            "undo 光标回编辑起点 (0,0)，新字符插在行首"
        );
    }

    #[test]
    fn mark_saved_resets_modified_flag() {
        let mut b = Buffer::from_plain_text("a");
        b.insert("b");
        assert!(b.is_modified());
        b.mark_saved();
        assert!(!b.is_modified(), "落盘后修改标记清零");
    }

    // ---------------- 词跳 ----------------

    #[test]
    fn word_jump_ascii() {
        let mut b = Buffer::from_plain_text("foo bar_baz  qux");
        b.set_cursor(p(0, 15)); // "qux" 词内（x 上）
        b.move_word_left(false);
        assert_eq!(b.cursor(), p(0, 13), "跳到 qux 词首");
        b.move_word_left(false);
        assert_eq!(b.cursor(), p(0, 4), "跨过空格跳到 bar_baz 词首");
        b.move_word_left(false);
        assert_eq!(b.cursor(), p(0, 0), "跳到 foo 词首");
        b.move_word_right(false);
        assert_eq!(b.cursor(), p(0, 3), "跳到 foo 词尾（空格前）");
        b.move_word_right(false);
        assert_eq!(b.cursor(), p(0, 11), "跳过空格吃掉 bar_baz 到词尾");
        b.move_word_right(false);
        assert_eq!(b.cursor(), p(0, 16), "跨双空格吃掉 qux 词体到行尾");
    }

    #[test]
    fn word_jump_cjk_treated_as_word() {
        let mut b = Buffer::from_plain_text("中文 混合text");
        b.set_cursor(p(0, 9)); // 行尾
        b.move_word_left(false);
        assert_eq!(b.cursor(), p(0, 3), "混合text 连续词字符整段一词");
        b.move_word_left(false);
        assert_eq!(b.cursor(), p(0, 0), "跨空格跳到 中文 词首");
    }

    #[test]
    fn word_jump_crosses_lines() {
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.set_cursor(p(1, 0));
        b.move_word_left(false);
        assert_eq!(b.cursor(), p(0, 2), "下行行首词跳 → 上行行尾");
    }

    // ---------------- 查找 ----------------

    #[test]
    fn find_all_matches_case_insensitive() {
        let b = Buffer::from_plain_text("Foo bar\nfoo baz\nFOO");
        let hits = b.find_matches("foo");
        assert_eq!(hits, vec![p(0, 0), p(1, 0), p(2, 0)]);
    }

    #[test]
    fn find_multiple_hits_in_line_and_cjk() {
        let b = Buffer::from_plain_text("xaxa 中文中");
        let hits = b.find_matches("a");
        assert_eq!(hits, vec![p(0, 1), p(0, 3)]);
        let hits = b.find_matches("文中");
        assert_eq!(hits, vec![p(0, 6)], "CJK 子串按 char 定位");
    }

    #[test]
    fn find_empty_query_no_hits() {
        let b = Buffer::from_plain_text("abc");
        assert!(b.find_matches("").is_empty());
    }

    // ---------------- 边界防御 ----------------

    #[test]
    fn out_of_bounds_access_is_safe() {
        let b = Buffer::from_plain_text("a");
        assert_eq!(b.line(99), "");
        assert_eq!(b.line_len(99), 0);
    }

    #[test]
    fn char_count_counts_chars_and_newlines() {
        let b = Buffer::from_plain_text("中文ab\nx");
        // 5 可见字符 + 1 换行 = 6。
        assert_eq!(b.char_count(), 6);
    }

    // ---------------- r42 换芯专项（坐标层 / Clone / 大文档） ----------------

    #[test]
    fn clone_is_independent_deep_copy() {
        let mut b = Buffer::from_plain_text("ab\ncd");
        b.set_cursor(p(1, 1));
        b.insert("X");
        let c = b.clone();
        assert_eq!(c.to_plain_text(), "ab\ncXd");
        // 克隆体独立编辑不回写源
        let mut c2 = c;
        c2.insert("Y");
        assert_eq!(c2.to_plain_text(), "ab\ncXYd");
        assert_eq!(b.to_plain_text(), "ab\ncXd", "克隆体编辑不影响源");
        // undo 历史随行（四元组外壳栈深拷贝）
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), "ab\ncd");
    }

    #[test]
    fn debug_impl_is_compact_summary() {
        let b = Buffer::from_plain_text("中文\nx");
        let s = format!("{b:?}");
        assert!(s.contains("lines: 2"), "Debug 含行数：{s}");
        assert!(s.contains("chars: 4"), "Debug 含字符数：{s}");
    }

    #[test]
    fn large_document_edit_and_navigate() {
        // r42 换芯收益冒烟：10 万行 / 50 万字级文档的随机点编辑 + 深点导航
        // （旧 Vec<String> 实现的逐行 drain/insert 在此规模显著退化）
        let lines: Vec<String> = (0..100_000)
            .map(|i| format!("第{i}行内容padding"))
            .collect();
        let text = lines.join("\n");
        let mut b = Buffer::from_plain_text(&text);
        assert_eq!(b.line_count(), 100_000);
        // 深点导航（末行行尾）
        b.set_cursor(p(99_999, b.line_len(99_999)));
        // 中段随机替换（跨 3 行）
        b.replace_range(p(50_000, 0), p(50_002, 2), "换芯");
        assert!(b.line(50_000).starts_with("换芯"));
        assert!(b.line(50_000).ends_with("padding"), "前段保留");
        // 全文序列化完整
        let round = b.to_plain_text();
        assert!(round.contains("第0行内容padding"));
        assert!(round.contains("第99999行内容padding"));
        // undo 完整可逆
        assert!(b.undo());
        assert_eq!(b.to_plain_text(), text, "undo 恢复 50 万字原文");
    }
}

// =============================================================================
// r42 性能实测（--ignored 面板）：cargo test -- --ignored
#[test]
#[ignore = "r42 性能实测（#68 阶段 A 验收证据）"]
fn perf_rope_edit_find_2m_chars() {
    use std::time::Instant;
    // 10 万行 × ~20 字/行 ≈ 200 万字符（4 倍于 50 万字红线规格）
    let text: String = {
        let lines: Vec<String> = (0..100_000)
            .map(|i| format!("第{i}行内容padding"))
            .collect();
        lines.join("\n")
    };
    let expect_chars = text.chars().count();
    assert!(
        expect_chars > 1_500_000,
        "压力规格 ≥150 万字符，实际 {expect_chars}"
    );

    // ① 构造（红线 <2s——4 倍规格下仍须达标）
    let t0 = Instant::now();
    let mut b = Buffer::from_plain_text(&text);
    let t_build = t0.elapsed();
    // ② 输入延迟（红线 p99 <50ms）：随机点 1000 次 insert
    let mut rng_state: u64 = 0x9E3779B97F4A7C15;
    let mut rng = || {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        rng_state
    };
    let mut samples: Vec<std::time::Duration> = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let line = (rng() % b.line_count() as u64) as usize;
        let col = (rng() % (b.line_len(line) as u64 + 1)) as usize;
        b.set_cursor(Point::new(line, col));
        let t = Instant::now();
        b.insert("芯");
        samples.push(t.elapsed());
    }
    samples.sort();
    let p50 = samples[500];
    let p99 = samples[990];
    let t_edit_max = samples[999];
    // ③ undo 路径（反向 apply_core，红线 <50ms/次）× 200
    let mut undo_samples: Vec<std::time::Duration> = Vec::with_capacity(200);
    for _ in 0..200 {
        let t = Instant::now();
        assert!(b.undo());
        undo_samples.push(t.elapsed());
    }
    undo_samples.sort();
    let undo_p99 = undo_samples[198];
    // ④ 全文查找（红线 <1s）
    let t = Instant::now();
    let hits = b.find_matches("行内容");
    let t_find = t.elapsed();
    assert!(!hits.is_empty());
    // ⑤ 深点导航（末行行尾 line_len + set_cursor）
    let t = Instant::now();
    let last = b.line_count() - 1;
    let len = b.line_len(last);
    b.set_cursor(Point::new(last, len));
    let t_nav = t.elapsed();
    // ⑥ 序列化
    let t = Instant::now();
    let round = b.to_plain_text();
    let t_ser = t.elapsed();
    drop(round);
    println!("perf(r42 换芯, {expect_chars} chars):");
    println!("  构造      = {:>10.1?}  (红线 <2s)", t_build);
    println!("  insert    = p50 {p50:?} / p99 {p99:?} / max {t_edit_max:?}  (红线 p99 <50ms)");
    println!("  undo      = p99 {undo_p99:?}  (红线 <50ms)");
    println!(
        "  find      = {:>10.1?}  hits={} (红线 <1s)",
        t_find,
        hits.len()
    );
    println!("  深点导航  = {:>10.1?}", t_nav);
    println!("  序列化    = {:>10.1?}", t_ser);
    assert!(
        t_build < std::time::Duration::from_secs(2),
        "构造超红线：{t_build:?}"
    );
    assert!(
        p99 < std::time::Duration::from_millis(50),
        "insert p99 超红线：{p99:?}"
    );
    assert!(
        undo_p99 < std::time::Duration::from_millis(50),
        "undo p99 超红线：{undo_p99:?}"
    );
    assert!(
        t_find < std::time::Duration::from_secs(1),
        "查找超红线：{t_find:?}"
    );
}
