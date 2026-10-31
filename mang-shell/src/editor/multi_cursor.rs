//! multi_cursor —— 多光标 + 列选择（#68 阶段 C）。
//!
//! 抄 Zed `selections_collection` 的**设计**（上游实现在 editor crate——
//! 62 依赖耦合面不搬，r42 前两阶段同款纪律自研干净实现）：
//!
//! - **Buffer 保持单光标契约**（41 项测试锚定的公开语义不变）；多光标态
//!   完全在编辑器层——`Cursors` 持有 `Vec<(anchor, head)>`。
//! - **主光标** = 列表末项（Zed 惯例：最后操作/添加的光标为主）。
//!   常态（单光标）下与 `Buffer.cursor()` 同步。
//! - **多点编辑按文档序尾→头应用**（先行编辑不影响其后点的坐标）；
//!   批撤销 = 连续 `undo` 本批点数（外壳四元组栈顶恰为本批）。
//! - 多点编辑后各光标重置为**裸光标**于各插入尾部（纯算术
//!   `end_after_insert`——与 Buffer 光标语义同一公式）。

use super::buffer::{Buffer, Point};

/// 单个光标（锚点 + 头；`anchor == head` 为裸光标）。
pub type Sel = (Point, Point);

/// 多光标集合（编辑器层；Buffer 单光标契约不变）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursors {
    /// 按头排序（`head` 升序；主光标 = 末项）。
    sels: Vec<Sel>,
    /// 上一次多点编辑的批点数（撤销一次撤整批；`take_undo_batch` 消费）。
    pending_batch: usize,
}

/// 光标移动方向（多光标同向独立移动；r42 阶段 D：词跳入集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    Home,
    End,
    DocStart,
    DocEnd,
}

impl Cursors {
    pub fn single(p: Point) -> Self {
        Self {
            sels: vec![(p, p)],
            pending_batch: 0,
        }
    }

    /// 由单选区构造（mod.rs 的 Buffer→Cursors 同步路径）。
    pub fn from_single_sel(anchor: Point, head: Point) -> Self {
        Self {
            sels: vec![(anchor, head)],
            pending_batch: 0,
        }
    }

    /// 光标数（1 = 常态单光标）。
    pub fn len(&self) -> usize {
        self.sels.len()
    }

    pub fn is_multiple(&self) -> bool {
        self.sels.len() > 1
    }

    /// 只读视图（按头排序）。
    pub fn entries(&self) -> &[Sel] {
        &self.sels
    }

    /// 主光标（末项；空集防御返回文首）。
    pub fn primary(&self) -> Sel {
        self.sels
            .last()
            .copied()
            .unwrap_or((Point::new(0, 0), Point::new(0, 0)))
    }

    /// 收拢单光标（Esc）：主光标落主光标头（经典收拢方向）。
    pub fn collapse_to_primary(&mut self) {
        let (_, h) = self.primary();
        self.sels = vec![(h, h)];
        self.pending_batch = 0;
    }

    /// 加入一个光标（完全相同 → 忽略；与既有选区重叠 → 合并为并集）。
    /// 维持头排序。返回是否发生变化。
    pub fn push(&mut self, anchor: Point, head: Point) -> bool {
        let (s, e) = if anchor <= head {
            (anchor, head)
        } else {
            (head, anchor)
        };
        for (a2, h2) in self.sels.iter_mut() {
            let (s2, e2) = if *a2 <= *h2 { (*a2, *h2) } else { (*h2, *a2) };
            if s <= e2 && s2 <= e {
                // 重叠 → 并集；方向随原选区（头在尾 → 头在并集尾）。
                // 完全相同（并集=原值）→ 无变化（push 幂等返回 false）。
                let ns = s.min(s2);
                let ne = e.max(e2);
                let head_at_end = *h2 >= *a2;
                let (na, nh) = if head_at_end { (ns, ne) } else { (ne, ns) };
                let changed = (na, nh) != (*a2, *h2);
                *a2 = na;
                *h2 = nh;
                return changed;
            }
        }
        self.sels.push((anchor, head));
        self.sels.sort_by_key(|(_, h)| *h);
        true
    }

    // ------------------------------------------------------------------
    // 多点编辑（Buffer 原语循环——按文档序尾→头；批撤销锚定）
    // ------------------------------------------------------------------

    /// 每个选区处插入 `text`（选区被替换）。各光标重置为裸光标于
    /// 各插入尾部（`end_after_insert` 公式；相邻同点去重）。
    pub fn insert_all(&mut self, buffer: &mut Buffer, text: &str) {
        let ordered = self.ordered_entries();
        self.pending_batch = ordered.len();
        for (s, e) in ordered.iter().rev() {
            buffer.replace_range(*s, *e, text);
        }
        let mut tails: Vec<Sel> = ordered
            .iter()
            .map(|(s, _)| {
                let t = buffer.end_after_insert_pub(s, text);
                (t, t)
            })
            .collect();
        tails.sort_by_key(|(_, h)| *h);
        tails.dedup_by_key(|(_, h)| *h);
        self.sels = tails;
    }

    /// 每个光标处执行退格语义（选区删/行首与前行合并/删前一字符）。
    /// 各光标重置为裸光标于各删除点（Buffer 光标=编辑起点同公式）。
    pub fn backspace_all(&mut self, buffer: &mut Buffer) {
        let ordered = self.ordered_entries();
        self.pending_batch = ordered.len();
        for (s, e) in ordered.iter().rev() {
            if s != e {
                buffer.replace_range(*s, *e, "");
                continue;
            }
            if e.col > 0 {
                let st = Point::new(e.line, e.col - 1);
                buffer.replace_range(st, *e, "");
            } else if e.line > 0 {
                let st = Point::new(e.line - 1, buffer.line_len(e.line - 1));
                buffer.replace_range(st, *e, "");
            }
        }
        let mut heads: Vec<Sel> = ordered.iter().map(|(s, _)| (*s, *s)).collect();
        heads.sort_by_key(|(_, h)| *h);
        heads.dedup_by_key(|(_, h)| *h);
        self.sels = heads;
    }

    /// 每个光标处执行前删语义（选区删/行尾与下行合并/删后一字符）。
    /// 各光标重置为裸光标于各删除点。
    pub fn delete_all(&mut self, buffer: &mut Buffer) {
        let ordered = self.ordered_entries();
        self.pending_batch = ordered.len();
        for (s, e) in ordered.iter().rev() {
            if s != e {
                buffer.replace_range(*s, *e, "");
                continue;
            }
            if e.col < buffer.line_len(e.line) {
                let en = Point::new(e.line, e.col + 1);
                buffer.replace_range(*e, en, "");
            } else if e.line + 1 < buffer.line_count() {
                let en = Point::new(e.line + 1, 0);
                buffer.replace_range(*e, en, "");
            }
        }
        let mut heads: Vec<Sel> = ordered.iter().map(|(s, _)| (*s, *s)).collect();
        heads.sort_by_key(|(_, h)| *h);
        heads.dedup_by_key(|(_, h)| *h);
        self.sels = heads;
    }

    /// 同向移动所有光标（`extend`：各锚不动扩选区；无 extend 各清锚）。
    /// Up/Down 的列取各光标原列（不做全局粘性——多光标下粘性语义
    /// 留阶段 D 精化，登记）。
    pub fn move_all(&mut self, buffer: &Buffer, dir: Move, extend: bool) {
        let mut out: Vec<Sel> = Vec::with_capacity(self.sels.len());
        for (a, h) in &self.sels {
            let na = if extend { *a } else { *h };
            let nh = match dir {
                Move::WordLeft => word_left(buffer, *h),
                Move::WordRight => word_right(buffer, *h),
                Move::Left => {
                    if h.col > 0 {
                        Point::new(h.line, h.col - 1)
                    } else if h.line > 0 {
                        Point::new(h.line - 1, buffer.line_len(h.line - 1))
                    } else {
                        *h
                    }
                }
                Move::Right => {
                    if h.col < buffer.line_len(h.line) {
                        Point::new(h.line, h.col + 1)
                    } else if h.line + 1 < buffer.line_count() {
                        Point::new(h.line + 1, 0)
                    } else {
                        *h
                    }
                }
                Move::Up => {
                    if h.line > 0 {
                        let up = h.line - 1;
                        Point::new(up, h.col.min(buffer.line_len(up)))
                    } else {
                        *h
                    }
                }
                Move::Down => {
                    if h.line + 1 < buffer.line_count() {
                        let dn = h.line + 1;
                        Point::new(dn, h.col.min(buffer.line_len(dn)))
                    } else {
                        *h
                    }
                }
                Move::Home => Point::new(h.line, 0),
                Move::End => Point::new(h.line, buffer.line_len(h.line)),
                Move::DocStart => Point::new(0, 0),
                Move::DocEnd => {
                    let last = buffer.line_count() - 1;
                    Point::new(last, buffer.line_len(last))
                }
            };
            out.push((na, nh));
        }
        self.sels = out;
        self.pending_batch = 0; // 移动不产生编辑历史
    }

    /// 消费待撤销批点数（0 = 无批）。调用方按返回值连撤。
    pub fn take_undo_batch(&mut self) -> usize {
        std::mem::replace(&mut self.pending_batch, 0)
    }

    /// G3：**累加**登记批撤销点数（行缩进/反缩进等非 `insert_all` 路径
    /// 的批量编辑——一次手势产生的 N 条编辑记录经此登记，Ctrl+Z 一键
    /// 连撤整批；与裸光标插入批同手势时累加合并）。
    pub fn add_undo_batch(&mut self, n: usize) {
        self.pending_batch += n;
    }

    /// G3：按行位移光标列（缩进 +delta / 反缩进 -delta；列下限 0）。
    /// `delta_of(line)` 返回该行的位移量（未涉及行 0）。
    pub fn shift_cols_by_line(&mut self, delta_of: impl Fn(usize) -> i64) {
        for (a, h) in self.sels.iter_mut() {
            for pt in [&mut *a, &mut *h] {
                let d = delta_of(pt.line);
                if d != 0 {
                    let col = pt.col as i64 + d;
                    pt.col = col.max(0) as usize;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // 构造器
    // ------------------------------------------------------------------

    /// **列选择**（Alt+拖）：`anchor..head` 矩形——行范围每行各置一光标，
    /// 列区间 clamp 行长；`head` 侧决定选区方向（head 在锚行上方 →
    /// 各行选区头在列首；否则头在列尾——拖拽方向的经典语义）。
    pub fn column_select(buffer: &Buffer, anchor: Point, head: Point) -> Self {
        let l0 = anchor.line.min(head.line);
        let l1 = anchor.line.max(head.line);
        let (c0, c1) = if anchor.col <= head.col {
            (anchor.col, head.col)
        } else {
            (head.col, anchor.col)
        };
        let head_above = head.line < anchor.line;
        let mut sels = Vec::with_capacity(l1 - l0 + 1);
        for line in l0..=l1 {
            let len = buffer.line_len(line);
            let s = c0.min(len);
            let e = c1.min(len);
            let sel = if head_above {
                (Point::new(line, e), Point::new(line, s))
            } else {
                (Point::new(line, s), Point::new(line, e))
            };
            sels.push(sel);
        }
        Self {
            sels,
            pending_batch: 0,
        }
    }

    /// **Ctrl+D**：主光标词 → 文档序下一个同词完整匹配（无 → None）。
    /// 词源：主选区有选区时取选区文本（词尾后一格场景）；裸光标时
    /// 取头处词。扫描起点 = 主词之后（同行词尾起，再后续行）。
    pub fn add_next_word_match(buffer: &Buffer, cur: &Self) -> Option<Self> {
        let (pa, ph) = cur.primary();
        let (word, scan_start): (Vec<char>, Point) = if pa != ph {
            let w: Vec<char> = buffer
                .range_text(&(if pa <= ph { pa } else { ph }..if pa <= ph { ph } else { pa }))
                .chars()
                .collect();
            (w, if pa <= ph { ph } else { pa })
        } else {
            let line_text = buffer.line(ph.line);
            let chars: Vec<char> = line_text.chars().collect();
            let (w_s, w_e) = word_span_at(&chars, ph.col)?;
            (chars[w_s..w_e].to_vec(), Point::new(ph.line, w_e))
        };
        if word.is_empty() {
            return None;
        }
        // 扫描：同行词尾之后，再后续行；词边界完整匹配
        for li in scan_start.line..buffer.line_count() {
            let lt = buffer.line(li);
            let lc: Vec<char> = lt.chars().collect();
            let start = if li == scan_start.line {
                scan_start.col
            } else {
                0
            };
            for c in start..lc.len() {
                if same_word(&lc, c, &word) {
                    let e2 = c + word.len();
                    let mut out = cur.clone();
                    out.push(Point::new(li, c), Point::new(li, e2));
                    return Some(out);
                }
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // 内部
    // ------------------------------------------------------------------

    fn ordered_entries(&self) -> Vec<(Point, Point)> {
        self.sels
            .iter()
            .map(|(a, h)| if a <= h { (*a, *h) } else { (*h, *a) })
            .collect()
    }
}

/// 行内 char 数组中 `col` 所在词的区间（无词字符 → None）。
fn word_span_at(chars: &[char], col: usize) -> Option<(usize, usize)> {
    let in_word = |c: char| c.is_alphanumeric() || c == '_';
    if col >= chars.len() || !in_word(chars[col]) {
        return None;
    }
    let mut s = col;
    while s > 0 && in_word(chars[s - 1]) {
        s -= 1;
    }
    let mut e = col;
    while e < chars.len() && in_word(chars[e]) {
        e += 1;
    }
    Some((s, e))
}

/// `chars[c..]` 起始处是否恰为 `word`（词边界完整匹配）。
fn same_word(chars: &[char], c: usize, word: &[char]) -> bool {
    if c + word.len() > chars.len() {
        return false;
    }
    if &chars[c..c + word.len()] != word {
        return false;
    }
    let in_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let before_ok = c == 0 || !in_word(chars[c - 1]);
    let after = c + word.len();
    let after_ok = after >= chars.len() || !in_word(chars[after]);
    before_ok && after_ok
}

// =============================================================================
// 单元测试（#68 质量红线：核心算法必须有单元测试）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn p(l: usize, c: usize) -> Point {
        Point::new(l, c)
    }

    // ---------------- 基础状态 ----------------

    #[test]
    fn single_state_is_one_cursor() {
        let c = Cursors::single(p(1, 2));
        assert_eq!(c.len(), 1);
        assert!(!c.is_multiple());
        assert_eq!(c.primary(), (p(1, 2), p(1, 2)));
    }

    #[test]
    fn push_dedup_and_merge() {
        let mut c = Cursors::single(p(0, 0));
        assert!(c.push(p(0, 5), p(0, 5)));
        assert_eq!(c.len(), 2, "加入不同位置");
        assert!(!c.push(p(0, 5), p(0, 5)), "完全相同 → 忽略");
        assert_eq!(c.len(), 2);
        // 重叠选区合并：(0,0)..(0,0) 与 (0,2)..(0,7) 不重叠——先构造重叠
        let mut m = Cursors::single(p(0, 0));
        m.push(p(0, 3), p(0, 8)); // 光标 (0,0) 与 (0,3..8)
        assert_eq!(m.len(), 2);
        assert!(
            m.push(p(0, 7), p(0, 12)),
            "(0,7..12) 与 (0,3..8) 重叠 → 合并"
        );
        assert_eq!(m.len(), 2, "合并不增数");
        // 合并后区间为并集 (0,3..12)：验证方式——文档序视图含该区间头
        let heads: Vec<Point> = m.entries().iter().map(|(_, h)| *h).collect();
        assert!(heads.contains(&p(0, 12)), "合并并集头 12：{heads:?}");
    }

    #[test]
    fn collapse_esc_gathers_to_primary_head() {
        let mut c = Cursors::column_select(
            &Buffer::from_plain_text("aaaa\nbbbb\ncccc"),
            p(0, 1),
            p(2, 3),
        );
        assert_eq!(c.len(), 3, "3 行列选择");
        c.collapse_to_primary();
        assert_eq!(c.len(), 1);
        assert_eq!(
            c.primary(),
            (p(2, 3), p(2, 3)),
            "主光标=末项头（列选择头侧）"
        );
    }

    // ---------------- 多点编辑（尾→头序） ----------------

    #[test]
    fn insert_all_applies_tail_first() {
        let mut b = Buffer::from_plain_text("ab\ncd\nef");
        let mut c = Cursors::single(p(1, 1)); // cd 中
        c.push(p(0, 1), p(0, 1)); // ab 中
        // 尾→头：先 (1,1) 再 (0,1)——若顺序错，第二点坐标漂移
        c.insert_all(&mut b, "X");
        assert_eq!(b.to_plain_text(), "aXb\ncXd\nef");
        assert_eq!(c.len(), 2, "两点各留光标");
        assert_eq!(c.entries()[0], (p(0, 2), p(0, 2)), "第一光标在插入尾部");
        assert_eq!(c.entries()[1], (p(1, 2), p(1, 2)));
    }

    #[test]
    fn insert_all_selections_replaced() {
        let mut b = Buffer::from_plain_text("hello world");
        let mut c = Cursors::single(p(0, 6)); // word 头
        c.push(p(0, 0), p(0, 5)); // hello 选区
        // 尾→头：先 (0,6)（col6=w 前）插 X\nY → "hello X\nYworld"；
        // 再 (0,0..5) 替换 hello → "X\nY"+" X\nYworld"
        c.insert_all(&mut b, "X\nY");
        assert_eq!(b.to_plain_text(), "X\nY X\nYworld");
        // 尾：end_after_insert((0,0),"X\nY")=(1,1)；end_after_insert((0,6))=(1,1)
        // —— 两点同文本（1 换行）尾重合 → dedup 合一（len 2 → 1）
        assert_eq!(c.len(), 1, "两尾重合（(1,1)/(1,1)）去重合一");
        assert_eq!(c.entries()[0], (p(1, 1), p(1, 1)));
    }

    #[test]
    fn backspace_all_semantics() {
        let mut b = Buffer::from_plain_text("ab\ncd\nef");
        let mut c = Cursors::single(p(2, 0)); // ef 行首 → 合并
        c.push(p(1, 1), p(1, 1)); // cd 中 → 删 c
        c.backspace_all(&mut b);
        // 尾→头：先 (2,0)（合并 ef 到 cd 行尾）再 (1,1)（此时点在旧坐标）
        // (2,0) 删 "\n"（行1行尾..行2行首）→ "ab\ncdef"
        // (1,1) 删 c → "ab\ndef"
        assert_eq!(b.to_plain_text(), "ab\ndef");
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn undo_batch_all_at_once() {
        let mut b = Buffer::from_plain_text("aa\nbb\ncc");
        let mut c = Cursors::single(p(2, 0));
        c.push(p(0, 0), p(0, 0));
        c.insert_all(&mut b, "X");
        assert_eq!(b.to_plain_text(), "Xaa\nbb\nXcc");
        let n = c.take_undo_batch();
        assert_eq!(n, 2, "批=2 点");
        // 连撤 2 次 = 撤整批
        for _ in 0..n {
            assert!(b.undo());
        }
        assert_eq!(b.to_plain_text(), "aa\nbb\ncc");
        assert_eq!(c.take_undo_batch(), 0, "批已消费");
    }

    // ---------------- G3：行级批量编辑的批撤销登记 ----------------

    #[test]
    fn set_undo_batch_registers_external_batch() {
        let mut b = Buffer::from_plain_text("aa\nbb\ncc");
        // 模拟 Editor 行缩进路径：3 条独立编辑 + 登记批 3
        b.replace_range(p(0, 0), p(0, 0), "  ");
        b.replace_range(p(1, 0), p(1, 0), "  ");
        b.replace_range(p(2, 0), p(2, 0), "  ");
        assert_eq!(b.to_plain_text(), "  aa\n  bb\n  cc");
        let mut c = Cursors::single(p(0, 0));
        c.add_undo_batch(3);
        let n = c.take_undo_batch();
        assert_eq!(n, 3, "外登记批 = 3 条");
        for _ in 0..n {
            assert!(b.undo());
        }
        assert_eq!(b.to_plain_text(), "aa\nbb\ncc", "一次手势连撤整批");
        assert_eq!(c.take_undo_batch(), 0, "批已消费");
    }

    #[test]
    fn shift_cols_by_line_clamps_at_zero() {
        let mut c = Cursors::single(p(0, 3));
        c.push(p(2, 1), p(2, 5));
        c.shift_cols_by_line(|line| if line == 0 { -10 } else { 0 });
        assert_eq!(c.entries()[0].1, p(0, 0), "列下限 0");
        assert_eq!(c.entries()[1].1, p(2, 5), "未涉及行不动");
        c.shift_cols_by_line(|line| if line == 2 { 4 } else { 0 });
        assert_eq!(
            c.entries()[1],
            (p(2, 5), p(2, 9)),
            "锚与头同位移（选区保持）"
        );
    }

    #[test]
    fn add_undo_batch_accumulates() {
        let mut c = Cursors::single(p(0, 0));
        c.add_undo_batch(2);
        c.add_undo_batch(3);
        assert_eq!(c.take_undo_batch(), 5, "同手势多路径累加");
        assert_eq!(c.take_undo_batch(), 0);
    }

    // ---------------- 移动 ----------------

    #[test]
    fn move_all_independently() {
        let b = Buffer::from_plain_text("ab\ncd");
        let mut c = Cursors::single(p(1, 0));
        c.push(p(0, 2), p(0, 2));
        // 文档序排序后：(0,2) 在前 (1,0) 在后
        c.move_all(&b, Move::Left, false);
        assert_eq!(c.entries()[0].1, p(0, 1), "(0,2) 左移");
        assert_eq!(c.entries()[1].1, p(0, 2), "(1,0) 行首左移 → 上行行尾");
        // extend：各锚不动
        let mut e = Cursors::single(p(0, 0));
        e.push(p(1, 1), p(1, 1));
        e.move_all(&b, Move::Right, true);
        assert_eq!(e.entries()[0], (p(0, 0), p(0, 1)), "锚不动头右移");
        assert_eq!(e.entries()[1], (p(1, 1), p(1, 2)));
    }

    #[test]
    fn move_all_updown_clamps_short_lines() {
        let b = Buffer::from_plain_text("long line\nxy\nanother long");
        let mut c = Cursors::single(p(2, 12));
        c.push(p(0, 9), p(0, 9));
        c.move_all(&b, Move::Up, false);
        // (2,12) 上移到 "xy"（len 2）→ clamp 2；(0,9) 上移出界 → 原地
        assert_eq!(c.entries()[0].1, p(0, 9), "文首行上移不动");
        assert_eq!(c.entries()[1].1, p(1, 2), "列 12 clamp 到短行 2");
    }

    // ---------------- 列选择 ----------------

    #[test]
    fn column_select_down_direction() {
        let b = Buffer::from_plain_text("aaaa\nbb\ncccc");
        let c = Cursors::column_select(&b, p(0, 1), p(2, 3));
        assert_eq!(c.len(), 3);
        assert_eq!(c.entries()[0], (p(0, 1), p(0, 3)), "行 0：1..3");
        assert_eq!(
            c.entries()[1],
            (p(1, 1), p(1, 2)),
            "短行 clamp：列 1..3 → 行内 1..2（剩 1 列可见）"
        );
        assert_eq!(c.entries()[2], (p(2, 1), p(2, 3)));
    }

    #[test]
    fn column_select_up_direction_heads_at_low_col() {
        let b = Buffer::from_plain_text("aaaa\nbb\ncccc");
        // head 在锚行上方：各行选区头在列首（拖拽向上）
        let c = Cursors::column_select(&b, p(2, 3), p(0, 1));
        assert_eq!(c.entries()[0], (p(0, 3), p(0, 1)), "头在低列（方向反转）");
        assert_eq!(c.entries()[2], (p(2, 3), p(2, 1)));
    }

    // ---------------- Ctrl+D ----------------

    #[test]
    fn add_next_word_match_advances() {
        let b = Buffer::from_plain_text("foo bar foo baz foo");
        let mut c = Cursors::single(p(0, 0)); // foo 内
        let c2 = Cursors::add_next_word_match(&b, &c).expect("下一个 foo 在 bar 后");
        assert_eq!(c2.len(), 2);
        assert_eq!(c2.entries()[1].1, p(0, 11), "第二个 foo 尾（8..11）");
        let c3 = Cursors::add_next_word_match(&b, &c2).expect("再下一个");
        assert_eq!(c3.len(), 3);
        // 末尾无匹配
        assert!(Cursors::add_next_word_match(&b, &c3).is_none());
    }

    #[test]
    fn add_next_word_match_respects_word_boundary() {
        // foo 不应匹配 foobar 的 foo 前缀（词边界完整匹配）
        let b = Buffer::from_plain_text("foo\nfoobar foo");
        let c = Cursors::single(p(0, 1));
        let c2 = Cursors::add_next_word_match(&b, &c).expect("跳过 foobar 匹配纯 foo");
        // 行 1 的 foobar 处 foo 不是完整词（后跟 bar）——下一个是行 1 的独立 foo（7..10）
        assert_eq!(
            c2.entries()[1].1,
            p(1, 10),
            "命中独立 foo 尾，跳过 foobar 前缀"
        );
    }

    #[test]
    fn add_next_word_match_none_when_not_on_word() {
        let b = Buffer::from_plain_text("a b");
        let c = Cursors::single(p(0, 1)); // 空格上
        assert!(Cursors::add_next_word_match(&b, &c).is_none());
    }

    // ---------------- 编辑后光标（end_after_insert 公式） ----------------

    #[test]
    fn insert_tails_with_newlines_cross_lines() {
        let mut b = Buffer::from_plain_text("aa\nbb");
        let mut c = Cursors::single(p(1, 1));
        c.insert_all(&mut b, "X\nY\nZ");
        // 插入尾部 = (1+2, 1) = (3, 1)
        assert_eq!(
            c.entries()[0].1,
            p(3, 1),
            "含 2 换行：行 +2 尾段列=末段 chars"
        );
        assert_eq!(b.to_plain_text(), "aa\nbX\nY\nZb");
    }
}

// r42 阶段 D：词跳（与 Buffer.move_word_* 同口径——is_word_char：
// 字母数字含 CJK + 下划线；行首/行尾跨界）。
fn word_left(buffer: &Buffer, h: Point) -> Point {
    let text = buffer.line(h.line);
    let chars: Vec<char> = text.chars().collect();
    let in_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut col = h.col;
    while col > 0 && !in_word(chars[col - 1]) {
        col -= 1;
    }
    while col > 0 && in_word(chars[col - 1]) {
        col -= 1;
    }
    if col == 0 && h.line > 0 {
        Point::new(h.line - 1, buffer.line_len(h.line - 1))
    } else {
        Point::new(h.line, col)
    }
}

fn word_right(buffer: &Buffer, h: Point) -> Point {
    let text = buffer.line(h.line);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let in_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut col = h.col;
    while col < len && !in_word(chars[col]) {
        col += 1;
    }
    while col < len && in_word(chars[col]) {
        col += 1;
    }
    if col == len && h.line + 1 < buffer.line_count() {
        Point::new(h.line + 1, 0)
    } else {
        Point::new(h.line, col)
    }
}

#[cfg(test)]
mod word_move_tests {
    use super::*;

    fn p(l: usize, c: usize) -> Point {
        Point::new(l, c)
    }

    #[test]
    fn word_move_all_multi_cursor() {
        let b = Buffer::from_plain_text("foo bar\n中文 混合x");
        let mut c = Cursors::single(p(1, 5)); // 混合x 词内
        c.push(p(0, 4), p(0, 4)); // bar 词内
        // 多光标词左跳：(0,4)=bar 词首——从词首再跳 → 前一词 foo=(0,0)；
        // (1,5)=混合x 词内 → 词首 (1,3)
        c.move_all(&b, Move::WordLeft, false);
        assert_eq!(c.entries()[0].1, p(0, 0), "bar 词首再跳 → foo 词首");
        assert_eq!(c.entries()[1].1, p(1, 3), "混合x 词首（col5→3）");
        // 再跳：foo 文首不动；(1,3)→吃词体到 (1,0) 后跨行 → bar 尾 (0,7)
        //（Buffer.move_word_left 同款语义：词首在行首时跳到上行行尾）
        c.move_all(&b, Move::WordLeft, false);
        assert_eq!(c.entries()[0].1, p(0, 0), "文首不动");
        assert_eq!(c.entries()[1].1, p(0, 7), "中文 词首在行首 → 跨行 bar 尾");
        // 词右跳：(0,0)→foo 尾；(0,7)→跨行 (1,0)
        c.move_all(&b, Move::WordRight, false);
        assert_eq!(c.entries()[0].1, p(0, 3), "foo 词尾");
        assert_eq!(c.entries()[1].1, p(1, 0), "行尾词跳 → 下行行首");
    }

    #[test]
    fn word_move_crosses_line_boundaries() {
        let b = Buffer::from_plain_text("ab cd\nef");
        let mut c = Cursors::single(p(1, 0));
        c.move_all(&b, Move::WordLeft, false);
        assert_eq!(c.primary().1, p(0, 5), "行首词跳 → 上行行尾");
        let mut d = Cursors::single(p(0, 5));
        d.move_all(&b, Move::WordRight, false);
        assert_eq!(d.primary().1, p(1, 0), "行尾词跳 → 下行行首");
    }
}
