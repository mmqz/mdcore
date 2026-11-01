//! display_map —— buffer 行列 ↔ 显示行列映射（#68 阶段 B）。
//!
//! 抄 Zed display map 的**设计**（上游实现在 editor crate 内——62 crate
//! 耦合面，不搬；算法思想在此自研干净实现，r42 阶段 A 同款纪律）：
//!
//! ```text
//! 变换链：buffer Point(line, col) --fold--> 行系 --wrap--> DisplayPoint(row, col)
//! ```
//!
//! - **折叠**（fold）：整行区间折叠为 1 个显示行；区间内 buffer 行不占
//!   display row；区间内的 buffer 点映射到折叠行首（col=0）——编辑器
//!   语义「折叠区不可进入」，光标/mapping 到头。
//! - **软换行**（wrap）：断点由渲染层回填（shaping 测宽不可纯逻辑化）；
//!   贪心算法本体 [`wrap_points_from_advances`] 是纯函数（单测覆盖），
//!   渲染层只喂 advances 序列。断点语义：**c 列后断行**——`(l, c)` 起
//!   归下一段行首（display col 重计为 0）。
//! - **惰性回填**：`wraps[line] = None` = 未回填（渲染层据此避免重算；
//!   未回填行按无断点参与映射——**未回填行的映射是保守近似**，渲染
//!   后按段渲染并回填，`invalidate()` 后缓存重建即精确）。
//! - **失效协议**：编辑（buffer 变更）/ 回填断点 / fold 变更后由调用方
//!   调 [`DisplayMap::invalidate`]；映射走行表缓存（重建 O(行数)，之后
//!   每次 O(log 行数) 二分）。
//!
//! col 语义与 buffer 一致：**char 索引**（CJK 列语义安全，r39 口径）。

use std::cell::RefCell;
use std::ops::Range;

use super::buffer::{Buffer, Point};

/// 显示坐标：`row` 为显示行（软换行/折叠后的行序），`col` 为显示行内
/// **char 索引**（软换行段内重计）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DisplayPoint {
    pub row: usize,
    pub col: usize,
}

impl DisplayPoint {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

/// 折叠区间登记项（区间互不重叠；id 机制阶段 D 折叠指示物需要时再加——
/// 零桩原则：无生产消费者的 API 面不预留）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    pub range: Range<Point>,
}

/// 行表缓存（display row ↔ buffer 行段 双向索引）。
///
/// - `row_src[row]`：显示行的来源——`(buffer_line, seg_idx)`；折叠行
///   的 seg_idx 标记 `FOLD_SEG`（渲染层据此区分折叠行）。
/// - `line_row_start[line]`：buffer 行首个显示行的 row；被折叠跳过的
///   行为 `NO_ROW`。
#[derive(Debug)]
struct RowTable {
    row_src: Vec<(usize, usize)>,
    line_row_start: Vec<usize>,
}

/// 折叠行的段标记（正常段 idx 从 0 起；折叠行用 [`FOLD_SEG`]）。
pub const FOLD_SEG: usize = usize::MAX;
/// 无显示行（被折叠跳过的 buffer 行）。
const NO_ROW: usize = usize::MAX;

/// 纯逻辑显示映射。
///
/// 生命周期归 `Editor`（同 `row_bounds`/`fence_cache` 的 RefCell 缓存
/// 模式）；多实例间无共享。
#[derive(Debug, Default)]
pub struct DisplayMap {
    /// per buffer line 的软断点列（None = 未回填）。
    wraps: Vec<Option<Vec<usize>>>,
    /// 折叠区间（互不重叠——`fold` 重叠输入返回 None 拒绝）。
    folds: Vec<Fold>,
    /// 行表缓存（`invalidate` 清空；首次映射重建）。
    cache: RefCell<Option<Box<RowTable>>>,
}

impl DisplayMap {
    // ------------------------------------------------------------------
    // 软换行：回填与查询
    // ------------------------------------------------------------------

    /// 回填 `line` 的软断点（升序去重；`>= line_len` 的断点无意义被滤除
    /// ——贪心产出天然满足，此处防御）。变更后自动 invalidate。
    pub fn set_wrap_points(&mut self, buffer: &Buffer, line: usize, cols: Vec<usize>) {
        if line >= buffer.line_count() {
            return;
        }
        let line_len = buffer.line_len(line);
        let mut cleaned: Vec<usize> = cols;
        cleaned.retain(|&c| c < line_len);
        cleaned.sort_unstable();
        cleaned.dedup();
        if self.wraps.len() <= line {
            self.wraps.resize_with(line + 1, || None);
        }
        let changed = self.wraps[line].as_ref() != Some(&cleaned);
        if changed {
            self.wraps[line] = Some(cleaned);
            self.invalidate();
        }
    }

    /// `line` 的断点引用（未回填 → None；回填空 = 显式无断点）。
    pub fn wrap_points(&self, line: usize) -> Option<&[usize]> {
        self.wraps.get(line)?.as_deref()
    }

    /// 是否已回填（渲染层据此跳过重算）。
    pub fn has_wrap_points(&self, line: usize) -> bool {
        self.wrap_points(line).is_some()
    }

    // ------------------------------------------------------------------
    // 折叠
    // ------------------------------------------------------------------

    /// 折叠一个**整行**区间（`start.col == 0 && end.col == 0`，start 行
    /// < end 行——至少跨 1 个换行；单行内区间拒绝）。与既有区间重叠
    /// 拒绝（None）。成功返回 `Some(())`。
    pub fn fold(&mut self, buffer: &Buffer, range: Range<Point>) -> Option<()> {
        if range.start.col != 0 || range.end.col != 0 {
            return None;
        }
        if range.start.line >= range.end.line {
            return None;
        }
        if range.end.line > buffer.line_count() {
            return None;
        }
        // 重叠拒绝（折叠区间之间互不重叠——渲染展开/行号语义依赖）。
        for f in &self.folds {
            if range.start.line < f.range.end.line && f.range.start.line < range.end.line {
                return None;
            }
        }
        self.folds.push(Fold {
            range: range.start..range.end,
        });
        self.folds.sort_by_key(|f| f.range.start.line);
        self.invalidate();
        Some(())
    }

    /// 清空全部已回填断点 + 失效缓存（编辑/宽度变更后由调用方触发；
    /// 下一帧重新回填可见行）。
    pub fn reset_wraps(&mut self) {
        if !self.wraps.is_empty() {
            self.wraps.clear();
            self.invalidate();
        }
    }

    /// 已登记折叠头行（升序——持久化/诊断面）。
    pub fn fold_heads(&self) -> Vec<usize> {
        self.folds.iter().map(|f| f.range.start.line).collect()
    }

    /// 折叠区间快照（渲染/持久化恢复共用——`(start_line, end_line)`）。
    pub fn fold_ranges(&self) -> Vec<(usize, usize)> {
        self.folds
            .iter()
            .map(|f| (f.range.start.line, f.range.end.line))
            .collect()
    }

    /// 清空全部折叠（「全部展开」；返回是否有变化）。
    pub fn unfold_all(&mut self) -> bool {
        if self.folds.is_empty() {
            return false;
        }
        self.folds.clear();
        self.invalidate();
        true
    }

    /// 展开包含 `line` 的折叠（含折叠头/内部行命中；返回是否发生展开）。
    pub fn unfold_at(&mut self, line: usize) -> bool {
        let before = self.folds.len();
        self.folds
            .retain(|f| !(line >= f.range.start.line && line <= f.range.end.line));
        let changed = before != self.folds.len();
        if changed {
            self.invalidate();
        }
        changed
    }

    /// `line` 是否处于折叠区间内（含端点——端点行本身可见：start 行是
    /// 折叠头（可见），end 行是折叠尾（可见）。**内部行**（开区间内）
    /// 不可见）。
    pub fn fold_containing(&self, line: usize) -> Option<&Fold> {
        self.folds
            .iter()
            .find(|f| line > f.range.start.line && line < f.range.end.line)
    }

    /// `line` 为折叠头行时返回该折叠（渲染层概要装饰用）。
    pub fn fold_head_at(&self, line: usize) -> Option<&Fold> {
        self.folds.iter().find(|f| f.range.start.line == line)
    }

    // ------------------------------------------------------------------
    // 映射（buffer <-> display）
    // ------------------------------------------------------------------

    /// 显示行总数。
    pub fn display_row_count(&self, buffer: &Buffer) -> usize {
        self.with_table(buffer, |t| t.row_src.len())
    }

    /// buffer 点 → 显示点。折叠区间内部的点映射到折叠行首（col 0）。
    pub fn to_display(&self, buffer: &Buffer, p: Point) -> DisplayPoint {
        let (line, col) = (p.line, p.col);
        // 折叠内部（严格内部——start/end 行可见）→ 折叠行首。
        if let Some(f) = self.fold_containing(line) {
            let head = f.range.start.line;
            return self.with_table(buffer, |t| {
                let row = t.line_row_start[head];
                debug_assert!(row != NO_ROW);
                DisplayPoint::new(row, 0)
            });
        }
        self.with_table(buffer, |t| {
            let mut line = line;
            let mut row = *t.line_row_start.get(line).unwrap_or(&NO_ROW);
            if row == NO_ROW {
                // 越界防御：行号超界映射到末行（与 buffer clamp 语义一致）。
                // 折叠内部行已在上方 fold_containing 分流——此处 NO_ROW 仅剩越界。
                let last = buffer.line_count() - 1;
                line = last;
                row = t.line_row_start.get(last).copied().unwrap_or(0);
            }
            let line_len = buffer.line_len(line);
            let col = col.min(line_len); // r42 补：越界 col clamp（防御性收窄）
            // 段号 = 断点表中 <= col 的个数（断点 c 后断行：c 归下段首）。
            let seg = self.seg_of(line, col);
            row += seg;
            let dcol = col - self.seg_start_col(line, seg);
            DisplayPoint::new(row, dcol)
        })
    }

    /// 显示点 → buffer 点（显示 col clamp 到段尾——防御性收窄）。
    pub fn to_buffer(&self, buffer: &Buffer, dp: DisplayPoint) -> Point {
        self.with_table(buffer, |t| {
            let Some(&(line, seg)) = t.row_src.get(dp.row) else {
                // 越界防御：row 超界映射到文档尾。
                let last = buffer.line_count() - 1;
                return Point::new(last, buffer.line_len(last));
            };
            if seg == FOLD_SEG {
                // 折叠行：映射到区间头行首（区间文本定位交给渲染层 range_text）。
                return Point::new(line, 0);
            }
            let base = self.seg_start_col(line, seg);
            let line_len = buffer.line_len(line);
            let col = (base + dp.col).min(line_len);
            Point::new(line, col)
        })
    }

    /// `row` 的来源行段（渲染层逐显示行取文本：折叠行 → 区间概要；
    /// 正常段 → 行文本切片）。
    pub fn row_source(&self, buffer: &Buffer, row: usize) -> Option<(usize, usize)> {
        self.with_table(buffer, |t| t.row_src.get(row).copied())
    }

    /// buffer 行 `line` 的显示行起点（被折叠跳过 → None）。
    pub fn line_row(&self, buffer: &Buffer, line: usize) -> Option<usize> {
        self.with_table(buffer, |t| {
            match t.line_row_start.get(line).copied().unwrap_or(NO_ROW) {
                NO_ROW => None,
                r => Some(r),
            }
        })
    }

    /// 显示行的渲染文本（软换行段切片；折叠行给区间首行文本——
    /// 概要装饰（“…N 行”）属渲染层，此处给原始行文本首段）。
    pub fn row_text(&self, buffer: &Buffer, row: usize) -> String {
        let Some((line, seg)) = self.row_source(buffer, row) else {
            return String::new();
        };
        if seg == FOLD_SEG {
            return buffer.line(line);
        }
        let text = buffer.line(line);
        let start = self.seg_start_col(line, seg);
        let pts = self.wrap_points(line).unwrap_or(&[]);
        let end = pts.get(seg).copied().unwrap_or(text.chars().count());
        let b0 = super::buffer::char_to_byte_pub(&text, start);
        let b1 = super::buffer::char_to_byte_pub(&text, end);
        text[b0..b1].to_string()
    }

    // ------------------------------------------------------------------
    // 内部
    // ------------------------------------------------------------------

    /// 断点表中 `col` 所属段号（断点 c 后断行：col==c 归下段）。
    fn seg_of(&self, line: usize, col: usize) -> usize {
        match self.wrap_points(line) {
            Some(pts) => pts.partition_point(|&c| c <= col),
            None => 0,
        }
    }

    /// 段 `seg` 的起始 char 列（段 0 = 0；段 n = 断点[n-1]）。
    fn seg_start_col(&self, line: usize, seg: usize) -> usize {
        match self.wrap_points(line) {
            Some(pts) if seg > 0 => pts[seg - 1],
            _ if seg == 0 => 0,
            _ => 0,
        }
    }

    /// 行表（缓存未建时先建；闭包持借用——guard 生命周期限本调用）。
    fn with_table<R>(&self, buffer: &Buffer, f: impl FnOnce(&RowTable) -> R) -> R {
        if self.cache.borrow().is_none() {
            *self.cache.borrow_mut() = Some(Box::new(self.build_table(buffer)));
        }
        let guard = self.cache.borrow();
        f(guard.as_ref().expect("just built"))
    }

    fn build_table(&self, buffer: &Buffer) -> RowTable {
        let lines = buffer.line_count();
        let mut row_src: Vec<(usize, usize)> = Vec::with_capacity(lines);
        let mut line_row_start: Vec<usize> = Vec::with_capacity(lines);
        line_row_start.resize(lines, NO_ROW);
        for (line, slot) in line_row_start.iter_mut().enumerate() {
            // 折叠头行：折叠显示行 1 个（FOLD_SEG）。
            if self.folds.iter().any(|f| f.range.start.line == line) {
                *slot = row_src.len();
                row_src.push((line, FOLD_SEG));
                continue;
            }
            // 折叠内部行：跳过（slot 保持 NO_ROW）。
            if self.fold_containing(line).is_some() {
                continue;
            }
            let segs = self.wrap_points(line).map(|p| p.len()).unwrap_or(0) + 1;
            *slot = row_src.len();
            for seg in 0..segs {
                row_src.push((line, seg));
            }
        }
        RowTable {
            row_src,
            line_row_start,
        }
    }

    /// 失效缓存（编辑/回填/折叠变更后由调用方触发；幂等）。
    pub fn invalidate(&mut self) {
        *self.cache.borrow_mut() = None;
    }
}

/// 软换行贪心（纯函数，单测覆盖）：给定每 char 的**累计 advance 宽度**
/// （`advances[i]` = 前 i+1 个 char 的总宽——渲染层从 shaping 拿），
/// 在不超过 `max_width` 的最右 char 边界处断行。
///
/// 断点语义：返回**断行处前一行的末 char 列**——即断点 c 表示“c 列后
/// 断行，c+1 起归下段”（与 [`DisplayMap::set_wrap_points`] 一致：断点
/// < 行长）。永不产生 0 列断点（空行/首 char 超宽 → 孤字成段）。
pub fn wrap_points_from_advances(
    advances: &[f32],
    char_count: usize,
    max_width: f32,
) -> Vec<usize> {
    let n = advances.len().min(char_count);
    let mut pts = Vec::new();
    let mut seg_start = 0usize; // 当前段首 char 列
    let mut last_fit: Option<usize> = None; // 当前段内最右可容 char 边界
    for i in 0..n {
        let seg_x = boundary(advances, seg_start); // 段起点 x（前 seg_start 个 char 的总宽）
        if advances[i] - seg_x <= max_width {
            last_fit = Some(i);
        } else if let Some(b) = last_fit {
            if b + 1 > seg_start {
                pts.push(b + 1); // b+1 列后断行：b+1 起归下段
                seg_start = b + 1;
            }
            let seg_x = boundary(advances, seg_start);
            last_fit = (advances[i] - seg_x <= max_width).then_some(i);
        } else {
            // 段内无一 char 可容（首 char 超宽）：孤字成段，不断行（避免 0 列断点）
            seg_start = i;
            let seg_x = boundary(advances, seg_start);
            last_fit = (advances[i] - seg_x <= max_width).then_some(i);
        }
    }
    pts
}

/// 前 k 个 char 的累计宽度（k=0 → 0；越界取末值——防御）。
fn boundary(advances: &[f32], k: usize) -> f32 {
    if k == 0 {
        0.
    } else {
        advances.get(k - 1).copied().unwrap_or(f32::MAX)
    }
}

// =============================================================================
// 单元测试 —— 映射算法全覆盖（#68 质量红线：核心算法必须有单元测试）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }
    fn dp(row: usize, col: usize) -> DisplayPoint {
        DisplayPoint::new(row, col)
    }

    fn map3() -> Buffer {
        // 3 行：行长 25 / 10 / 4
        Buffer::from_plain_text("aaaaaaaaaabbbbbbbbbbccccc\n0123456789\n中文")
    }

    // ---------------- 恒等（无 wrap 无 fold） ----------------

    #[test]
    fn identity_without_wrap_or_fold() {
        let b = map3();
        let m = DisplayMap::default();
        assert_eq!(m.display_row_count(&b), 3);
        assert_eq!(m.to_display(&b, p(1, 5)), dp(1, 5));
        assert_eq!(m.to_display(&b, p(2, 1)), dp(2, 1), "行内有效 col 直映");
        assert_eq!(
            m.to_buffer(&b, dp(2, 3)),
            p(2, 2),
            "超段尾 col clamp 到段尾（防御）"
        );
        assert_eq!(m.row_text(&b, 0), "aaaaaaaaaabbbbbbbbbbccccc");
    }

    // ---------------- 软换行 ----------------

    #[test]
    fn wrap_splits_rows_and_maps_both_ways() {
        let b = map3();
        let mut m = DisplayMap::default();
        // 行 0（长 25）断点 [10, 20]：3 段 10/10/5
        m.set_wrap_points(&b, 0, vec![10, 20]);
        assert_eq!(m.display_row_count(&b), 5, "0 行拆 3 段 + 行 1 + 行 2");
        // 断点处归下段首
        assert_eq!(
            m.to_display(&b, p(0, 10)),
            dp(1, 0),
            "断点 10 归第 2 段行首"
        );
        assert_eq!(m.to_display(&b, p(0, 15)), dp(1, 5));
        assert_eq!(
            m.to_display(&b, p(0, 20)),
            dp(2, 0),
            "断点 20 归第 3 段行首"
        );
        assert_eq!(m.to_display(&b, p(0, 25)), dp(2, 5), "行尾");
        assert_eq!(m.to_display(&b, p(0, 7)), dp(0, 7), "首段");
        // 后续行 row 平移
        assert_eq!(m.to_display(&b, p(1, 0)), dp(3, 0));
        // 逆向
        assert_eq!(m.to_buffer(&b, dp(1, 5)), p(0, 15));
        assert_eq!(m.to_buffer(&b, dp(2, 5)), p(0, 25));
        assert_eq!(m.to_buffer(&b, dp(3, 4)), p(1, 4));
        // 段文本
        assert_eq!(m.row_text(&b, 0), "aaaaaaaaaa");
        assert_eq!(m.row_text(&b, 1), "bbbbbbbbbb");
        assert_eq!(m.row_text(&b, 2), "ccccc");
    }

    #[test]
    fn wrap_points_dedup_filter_and_late_fill() {
        let b = map3();
        let mut m = DisplayMap::default();
        // 乱序/重复/越界（>=行长 25）过滤
        m.set_wrap_points(&b, 0, vec![20, 10, 20, 99, 10]);
        assert_eq!(m.wrap_points(0), Some(&[10, 20][..]));
        // 行 2 未回填：has=false，映射保守按无断点
        assert!(!m.has_wrap_points(2));
        // 回填空 = 显式无断点（区别于未回填）
        m.set_wrap_points(&b, 2, vec![]);
        assert!(m.has_wrap_points(2));
        assert_eq!(m.wrap_points(2), Some(&[][..]));
    }

    #[test]
    fn set_wrap_points_after_mapping_invalidates() {
        let b = map3();
        let mut m = DisplayMap::default();
        assert_eq!(m.to_display(&b, p(1, 0)), dp(1, 0));
        m.set_wrap_points(&b, 0, vec![10, 20]);
        // 缓存已失效：新映射反映断点
        assert_eq!(m.to_display(&b, p(1, 0)), dp(3, 0));
    }

    // ---------------- 软换行贪心（纯函数） ----------------

    #[test]
    fn greedy_wrap_basic_two_segments() {
        // 每 char 宽 10；max 100 → 断点 10（第 11 char 前放不下？——前 10 char 宽 100 边界可容）
        let advances: Vec<f32> = (1..=25).map(|i| (i * 10) as f32).collect();
        let pts = wrap_points_from_advances(&advances, 25, 100.);
        assert_eq!(pts, vec![10, 20], "每 10 char 断一次（边界可容）");
    }

    #[test]
    fn greedy_wrap_uneven_widths() {
        // char 宽不均：aaab（3+3+3+2=11 > 10 时断 3）……
        // advances = [3,6,9,11,14,...] max=10 → 边界 9（3 char）可容，11 超 → 断 3
        let advances: Vec<f32> = vec![3., 6., 9., 11., 13., 16., 18., 21., 23.];
        let pts = wrap_points_from_advances(&advances, 9, 10.);
        // 段 1：[0,3)（宽 9）；char3 宽 11 超 → 断 3。段 2 从 3：宽 11-9=2 → 13-9=4 → 16-9=7 → 18-9=9 → 21-9=12 超 → 断 7。
        assert_eq!(pts, vec![3, 7]);
    }

    #[test]
    fn greedy_wrap_single_char_overwide_never_zero_break() {
        // 首 char 即超宽：孤字成段，不产生 0 列断点
        let advances: Vec<f32> = vec![50., 100., 150.];
        let pts = wrap_points_from_advances(&advances, 3, 40.);
        assert!(pts.is_empty(), "每 char 都超宽——不 0 列断行：{pts:?}");
    }

    #[test]
    fn greedy_wrap_empty_and_short() {
        assert!(wrap_points_from_advances(&[], 0, 100.).is_empty());
        // 全宽 < max：无断点
        let advances: Vec<f32> = vec![1., 2., 3.];
        assert!(wrap_points_from_advances(&advances, 3, 100.).is_empty());
        // advances 比 char_count 长（防御截断）
        let advances: Vec<f32> = vec![1., 2., 3., 999.];
        assert!(wrap_points_from_advances(&advances, 3, 100.).is_empty());
    }

    // ---------------- 折叠 ----------------

    #[test]
    fn fold_collapses_rows_and_maps_interior_to_head() {
        let b = Buffer::from_plain_text("L0\nL1\nL2\nL3\nL4"); // 5 行
        let mut m = DisplayMap::default();
        m.fold(&b, p(1, 0)..p(3, 0))
            .expect("折叠 1..3 行（含 L1、L2）");
        assert_eq!(m.display_row_count(&b), 4, "L1+L2 折成 1 行：5-2+1=4");
        // 头行/尾行可见，内部行不可见
        assert!(m.fold_containing(1).is_none(), "start 行可见");
        assert!(m.fold_containing(2).is_some(), "内部行不可见");
        assert!(m.fold_containing(3).is_none(), "end 行可见");
        // 内部点 → 折叠行首
        assert_eq!(m.to_display(&b, p(2, 1)), dp(1, 0));
        // 头行 = 折叠行（FOLD_SEG）；头行上的有效 col 直映
        assert_eq!(m.to_display(&b, p(1, 1)), dp(1, 1));
        assert_eq!(m.row_source(&b, 1), Some((1, FOLD_SEG)));
        assert_eq!(m.row_text(&b, 1), "L1", "折叠行文本 = 区间首行");
        // 折叠行映射回区间头行首
        assert_eq!(m.to_buffer(&b, dp(1, 0)), p(1, 0));
        // 尾行 row 平移
        assert_eq!(m.to_display(&b, p(3, 0)), dp(2, 0));
        assert_eq!(m.to_display(&b, p(4, 1)), dp(3, 1));
        // 被跳过的行 line_row = None
        assert_eq!(m.line_row(&b, 2), None);
    }

    #[test]
    fn fold_rejects_malformed_and_overlapping() {
        let b = Buffer::from_plain_text("L0\nL1\nL2\nL3\nL4");
        let mut m = DisplayMap::default();
        assert!(m.fold(&b, p(1, 2)..p(3, 0)).is_none(), "非整行 start");
        assert!(m.fold(&b, p(1, 0)..p(3, 2)).is_none(), "非整行 end");
        assert!(m.fold(&b, p(2, 0)..p(2, 0)).is_none(), "零区间");
        assert!(m.fold(&b, p(2, 0)..p(1, 0)).is_none(), "倒序");
        assert!(m.fold(&b, p(1, 0)..p(2, 0)).is_some(), "相邻两行可折叠");
        assert!(m.fold(&b, p(1, 0)..p(2, 0)).is_none(), "重复");
        assert!(m.fold(&b, p(0, 0)..p(4, 0)).is_none(), "与既有重叠");
        assert_eq!(m.folds_count_for_test(), 1);
    }

    #[test]
    fn unfold_at_hit_semantics() {
        let b = Buffer::from_plain_text("L0\nL1\nL2\nL3\nL4\nL5");
        let mut m = DisplayMap::default();
        m.fold(&b, p(1, 0)..p(3, 0)).unwrap();
        m.fold(&b, p(4, 0)..p(6, 0)).unwrap();
        assert_eq!(
            m.display_row_count(&b),
            4,
            "6 行折两段：各净 -1（区间 2 行→1 行）"
        );
        // unfold_at 命中内部行
        assert!(m.unfold_at(2));
        assert!(!m.unfold_at(2), "重复展开无操作");
        assert_eq!(m.display_row_count(&b), 5);
        // 展开（含头行命中——alt-f 手势的 toggle 语义）
        assert!(m.unfold_at(4), "命中折叠头行同样展开");
        assert_eq!(m.display_row_count(&b), 6);
        // 折叠头行/内部行命中区间判定
        assert!(m.fold_containing(1).is_none(), "头行不属于内部");
    }

    // ---------------- 复合（fold + wrap） ----------------

    #[test]
    fn fold_and_wrap_compose() {
        let b = Buffer::from_plain_text("aaaaaaaaaabbbbbbbbbbccccc\n0123456789\n中文\nL3");
        let mut m = DisplayMap::default();
        m.set_wrap_points(&b, 0, vec![10, 20]); // 行 0 → 3 段
        m.fold(&b, p(1, 0)..p(3, 0)).unwrap(); // L1+中文 折 1 行
        // 行 0：row 0..2；折叠行 row 3；L3 row 4
        assert_eq!(m.display_row_count(&b), 5);
        assert_eq!(m.to_display(&b, p(0, 15)), dp(1, 5));
        assert_eq!(m.to_display(&b, p(2, 0)), dp(3, 0), "折叠内部 → 折叠行首");
        assert_eq!(m.to_display(&b, p(3, 2)), dp(4, 2));
        // 折叠行 idempotent 读取
        assert_eq!(m.row_source(&b, 3), Some((1, FOLD_SEG)));
        // 双向往返
        let dp1 = m.to_display(&b, p(2, 3));
        assert_eq!(m.to_buffer(&b, dp1), p(1, 0), "折叠行 → 区间头行首");
    }

    #[test]
    fn invalidate_is_idempotent_and_rebuilds() {
        let b = map3();
        let mut m = DisplayMap::default();
        m.invalidate();
        m.invalidate();
        assert_eq!(m.display_row_count(&b), 3);
    }

    // ---------------- 越界防御 ----------------

    #[test]
    fn out_of_bounds_maps_to_domain() {
        let b = map3();
        let m = DisplayMap::default();
        // to_display 行超界 → 末行；col 超行尾 clamp
        assert_eq!(
            m.to_display(&b, p(99, 99)),
            dp(2, 2),
            "行 2「中文」长 2：col 99 clamp"
        );
        // to_buffer row 超界 → 文档尾（行 2「中文」长 2）
        assert_eq!(m.to_buffer(&b, dp(99, 99)), p(2, 2));
        // row_source/line_row 越界
        assert_eq!(m.row_source(&b, 99), None);
        assert_eq!(m.line_row(&b, 99), None);
        assert_eq!(m.row_text(&b, 99), "");
    }

    #[test]
    fn empty_document_single_row() {
        let b = Buffer::from_plain_text("");
        let mut m = DisplayMap::default();
        m.set_wrap_points(&b, 0, vec![]); // 空行显式无断点
        assert_eq!(m.display_row_count(&b), 1);
        assert_eq!(m.to_display(&b, p(0, 0)), dp(0, 0));
        assert_eq!(m.row_text(&b, 0), "");
    }
}

#[cfg(test)]
impl DisplayMap {
    /// 测试面：折叠数。
    fn folds_count_for_test(&self) -> usize {
        self.folds.len()
    }
}
