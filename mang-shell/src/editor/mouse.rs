//! 鼠标 hit-test 纯逻辑 —— #68 阶段 1 批 2（点击定位 / 拖选 / 双击词 / 三击行）。
//!
//! 与 GPUI 的分工：
//!
//! - 本模块：坐标→行号、字节→列、词/行选区、点击类型、拖拽 clamp、
//!   自动滚动方向——全部纯函数，无 window 依赖，单测钉定。
//! - [`super::Editor`]：x→列的 shaping 面（`shape_line` +
//!   `closest_index_for_position`）、事件接线、行 bounds 记录。
//!
//! 坐标语义（与渲染面一致，见 [`super`] 常量）：
//!
//! - `position.y` 是窗口坐标；行 bounds 由每帧 canvas prepaint 记录
//!   （uniform_list 虚拟化——滚出可视区的行不在记录内，命中查询天然
//!   只对可见行生效）。
//! - 行内 x 的原点 = 行 bounds 左缘 + `GUTTER_WIDTH` + `TEXT_PADDING`
//!   （gutter 点击按列 0 处理）。

use std::ops::Range;

use gpui::{Bounds, Pixels, px};

use super::buffer::{Buffer, Point};

// ---------------------------------------------------------------------------
// 点击类型
// ---------------------------------------------------------------------------

/// 鼠标按下语义（GPUI `MouseDownEvent.click_count` 归一化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    /// 单击：定位光标（Shift 修饰 = 扩展选区）。
    Single,
    /// 双击：选词（CJK 连续汉字为一个词——与 `word_jump` 同口径）。
    Double,
    /// 三击：选行。
    Triple,
}

/// `click_count` → 点击类型（1=单击 / 2=双击 / ≥3=三击）。
pub fn click_kind(click_count: usize) -> ClickKind {
    match click_count {
        0 | 1 => ClickKind::Single,
        2 => ClickKind::Double,
        _ => ClickKind::Triple,
    }
}

// ---------------------------------------------------------------------------
// y → 行
// ---------------------------------------------------------------------------

/// y 坐标对可见行的命中结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YHit {
    /// 命中某可见行（行号）。
    Row(usize),
    /// 在首行之上（拖拽出上边界；单击不可达——视口裁剪）。
    Above,
    /// 在末行之下（点击文末空白 / 拖拽出下边界）。
    Below,
}

/// y（窗口坐标）→ 可见行命中。
///
/// `rows` 是本帧记录的 `(行号, bounds)`（按行号升序、行高 `LINE_HEIGHT`
/// 连续铺排）。y 落在两行间隙（浮点舍入）时归属上一行（`top <= y` 的
/// 最后一行）；全在上方 → [`YHit::Above`]；全在下方 → [`YHit::Below`]；
/// 空记录（尚未渲染）→ [`YHit::Above`]（防御：等同文首）。
pub fn line_for_y(y: Pixels, rows: &[(usize, Bounds<Pixels>)]) -> YHit {
    let mut hit = None;
    for (idx, bounds) in rows {
        if bounds.origin.y <= y {
            hit = Some(*idx);
        } else {
            break;
        }
    }
    match hit {
        // 命中行存在：y 在该行底缘之下则实为 Below（记录不连续/越界防御）。
        Some(idx) => {
            let bottom = rows
                .iter()
                .find(|(i, _)| *i == idx)
                .map(|(_, b)| b.origin.y + b.size.height)
                .unwrap_or(px(0.));
            if y < bottom {
                YHit::Row(idx)
            } else if idx == rows.last().map(|(i, _)| *i).unwrap_or(usize::MAX) {
                YHit::Below
            } else {
                // 间隙防御：归到下一行。
                let next = rows.iter().find(|(i, _)| *i > idx).map(|(i, _)| *i);
                match next {
                    Some(n) => YHit::Row(n),
                    None => YHit::Below,
                }
            }
        }
        None => YHit::Above,
    }
}

/// 拖拽/点击越界 clamp：命中行之外的 y 落到文档边界点。
///
/// - [`YHit::Above`] → 文首 `(0, 0)`
/// - [`YHit::Below`] → 文尾 `(末行, 末行长度)`（Lead 验收「点在文末空白 →
///   光标到文档末尾」）
/// - [`YHit::Row`] → `(行, col)`（col 由视图层 shaping 给出，此处透传）
pub fn clamp_point(buffer: &Buffer, hit: YHit, col_in_row: usize) -> Point {
    match hit {
        YHit::Row(line) => Point::new(line, col_in_row),
        YHit::Above => Point::new(0, 0),
        YHit::Below => {
            let last = buffer.line_count().saturating_sub(1);
            Point::new(last, buffer.line_len(last))
        }
    }
}

/// 拖拽专用的越界 clamp：渐进式——出界只落到**可见边界行**（而非文档
/// 边界），后续由自动滚动逐步扩展（标准编辑器行为：小幅出界 = 按出界
/// 量逐步选，而非一截到底）。可见区未知（防御）时退回文档边界。
///
/// - [`YHit::Above`] → `(首可见行, 0)`
/// - [`YHit::Below`] → `(末可见行, 行尾)`——末可见行 = 文档末行时
///   天然等于文末（拖到底不再动）
pub fn visible_clamp_point(
    buffer: &Buffer,
    hit: YHit,
    col_in_row: usize,
    visible: Option<(usize, usize)>,
) -> Point {
    match (hit, visible) {
        (YHit::Row(line), _) => Point::new(line, col_in_row),
        (YHit::Above, Some((first, _))) => Point::new(first, 0),
        (YHit::Below, Some((_, last))) => Point::new(last, buffer.line_len(last)),
        (hit, None) => clamp_point(buffer, hit, col_in_row),
    }
}

// ---------------------------------------------------------------------------
// x → 列（纯逻辑部分）
// ---------------------------------------------------------------------------

/// 行内字节偏移 → char 列（shaping hit-test 的输出换算；越界 clamp 到
/// 行尾）。
pub fn byte_index_to_col(line: &str, byte: usize) -> usize {
    line.char_indices().take_while(|(b, _)| *b < byte).count()
}

// ---------------------------------------------------------------------------
// 双击选词 / 三击选行
// ---------------------------------------------------------------------------

/// 双击选词范围（char 列区间，行内）。
///
/// 三类字符段语义（VS Code 口径）：词字符（字母数字/CJK/下划线）→ 词体；
/// 空白 → 连续空白段；标点 → 连续标点段。与 `Buffer::move_word_*` 的
/// `is_word_char` 同源判定。`col == 行长`（点在行尾空白）时取最后一个
/// 字符；空行返回 `None`（无可选对象）。
pub fn word_range_at(line: &str, col: usize) -> Option<Range<usize>> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let at = col.min(chars.len() - 1);
    let target_class = char_class(chars[at]);
    let mut start = at;
    let mut end = at + 1;
    while start > 0 && char_class(chars[start - 1]) == target_class {
        start -= 1;
    }
    while end < chars.len() && char_class(chars[end]) == target_class {
        end += 1;
    }
    Some(start..end)
}

/// 字符段类别（双击选词粒度：0=词 / 1=空白 / 2=标点）。
fn char_class(c: char) -> u8 {
    if super::buffer::is_word_char(c) {
        0
    } else if c.is_whitespace() {
        1
    } else {
        2
    }
}

/// 三击选行范围（`(行首, 行尾)` 一对 Point；不含换行符）。
pub fn line_range(buffer: &Buffer, line: usize) -> (Point, Point) {
    let len = buffer.line_len(line);
    (Point::new(line, 0), Point::new(line, len))
}

// ---------------------------------------------------------------------------
// 拖拽扩展语义（双击词拖 / 三击行拖）
// ---------------------------------------------------------------------------

/// 拖拽中光标头落点（按拖拽类型修正扩展粒度）。
///
/// - [`ClickKind::Single`]：命中点原样。
/// - [`ClickKind::Double`]：按词边界对齐——向右扩展取命中行命中列所在
///   词的右缘，向左取左缘（拖拽按词选择的编辑器标准行为）。
/// - [`ClickKind::Triple`]：按行边界对齐——向下扩展取命中行行尾，向上
///   取行首。
///
/// `anchor_line` 是拖拽锚（mousedown 落点）所在行，用于判定扩展方向。
pub fn drag_head(buffer: &Buffer, kind: ClickKind, anchor: Point, target: Point) -> Point {
    match kind {
        ClickKind::Single => target,
        ClickKind::Double => {
            let line = buffer.line(target.line); // r42：String（行文本即时取）
            match word_range_at(&line, target.col) {
                Some(r) => {
                    let word_end = Point::new(target.line, r.end);
                    let word_start = Point::new(target.line, r.start);
                    if Point::new(target.line, target.col) >= anchor {
                        word_end
                    } else {
                        word_start
                    }
                }
                None => target,
            }
        }
        ClickKind::Triple => {
            let (_, end) = line_range(buffer, target.line);
            let (start, _) = line_range(buffer, target.line);
            if target.line >= anchor.line {
                end
            } else {
                start
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 自动滚动
// ---------------------------------------------------------------------------

/// 拖拽出界方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDir {
    /// 鼠标在首行之上 → 向上滚。
    Up,
    /// 鼠标在末行之下 → 向下滚。
    Down,
}

/// y（窗口坐标）相对可见行区间的出界方向（界内 `None`）。
///
/// 以首行顶缘/末行底缘为界（与视口边缘至多差一行部分可见行——
/// 方向判定精度足够）。
pub fn autoscroll_dir(y: Pixels, first_top: Pixels, last_bottom: Pixels) -> Option<ScrollDir> {
    if y < first_top {
        Some(ScrollDir::Up)
    } else if y > last_bottom {
        Some(ScrollDir::Down)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    fn row(idx: usize, top: f32) -> (usize, Bounds<Pixels>) {
        (
            idx,
            Bounds {
                origin: point(px(100.), px(top)),
                size: size(px(800.), px(24.)),
            },
        )
    }

    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }

    // --- click_kind ---

    #[test]
    fn click_kind_mapping() {
        assert_eq!(click_kind(0), ClickKind::Single);
        assert_eq!(click_kind(1), ClickKind::Single);
        assert_eq!(click_kind(2), ClickKind::Double);
        assert_eq!(click_kind(3), ClickKind::Triple);
        assert_eq!(click_kind(7), ClickKind::Triple, "≥3 一律三击");
    }

    // --- line_for_y ---

    #[test]
    fn line_for_y_hits_each_row() {
        let rows = vec![row(10, 200.), row(11, 224.), row(12, 248.)];
        assert_eq!(line_for_y(px(205.), &rows), YHit::Row(10));
        assert_eq!(line_for_y(px(223.9), &rows), YHit::Row(10));
        assert_eq!(line_for_y(px(224.), &rows), YHit::Row(11), "底缘=下行顶缘");
        assert_eq!(line_for_y(px(271.9), &rows), YHit::Row(12));
    }

    #[test]
    fn line_for_y_above_and_below() {
        let rows = vec![row(5, 100.), row(6, 124.)];
        assert_eq!(line_for_y(px(50.), &rows), YHit::Above);
        assert_eq!(line_for_y(px(99.9), &rows), YHit::Above);
        assert_eq!(line_for_y(px(148.), &rows), YHit::Below);
        assert_eq!(line_for_y(px(999.), &rows), YHit::Below);
    }

    #[test]
    fn line_for_y_empty_rows_defensive() {
        assert_eq!(line_for_y(px(50.), &[]), YHit::Above);
    }

    #[test]
    fn line_for_y_gap_falls_to_next_row() {
        // 人为造间隙（24 行高 + 2px 缺口）：间隙归属下一行。
        let rows = vec![
            row(3, 100.),
            (
                4,
                Bounds {
                    origin: point(px(100.), px(126.)),
                    size: size(px(800.), px(24.)),
                },
            ),
        ];
        assert_eq!(line_for_y(px(125.), &rows), YHit::Row(4));
    }

    // --- clamp_point ---

    #[test]
    fn clamp_point_boundaries() {
        let b = Buffer::from_plain_text("第一行\n第二行\n第三行");
        assert_eq!(clamp_point(&b, YHit::Above, 5), p(0, 0));
        assert_eq!(clamp_point(&b, YHit::Below, 5), p(2, 3), "文末=末行行尾");
        assert_eq!(clamp_point(&b, YHit::Row(1), 2), p(1, 2));
    }

    #[test]
    fn visible_clamp_point_progressive() {
        let b = Buffer::from_plain_text("l0\nl1\nl2\nl3\nl4\nl5");
        // 可见区 1..=4：出界只落可见边界（渐进式——自动滚动再逐步扩展）。
        assert_eq!(
            visible_clamp_point(&b, YHit::Above, 9, Some((1, 4))),
            p(1, 0),
            "上出界 → 首可见行行首"
        );
        assert_eq!(
            visible_clamp_point(&b, YHit::Below, 9, Some((1, 4))),
            p(4, 2),
            "下出界 → 末可见行行尾（非文末）"
        );
        // 命中行不受可见区影响。
        assert_eq!(
            visible_clamp_point(&b, YHit::Row(2), 1, Some((1, 4))),
            p(2, 1)
        );
        // 可见区未知（防御）→ 文档边界（与 clamp_point 同口径）。
        assert_eq!(visible_clamp_point(&b, YHit::Below, 9, None), p(5, 2));
        assert_eq!(visible_clamp_point(&b, YHit::Above, 9, None), p(0, 0));
        // 末可见行 = 文档末行时 Below 天然等于文末。
        assert_eq!(
            visible_clamp_point(&b, YHit::Below, 9, Some((3, 5))),
            p(5, 2)
        );
    }

    // --- byte_index_to_col ---

    #[test]
    fn byte_index_to_col_ascii_cjk_emoji() {
        let line = "中文😀ab";
        // 字节偏移：中=0..3 文=3..6 😀=6..10 a=10..11 b=11..12
        assert_eq!(byte_index_to_col(line, 0), 0);
        assert_eq!(byte_index_to_col(line, 3), 1);
        assert_eq!(byte_index_to_col(line, 6), 2);
        assert_eq!(byte_index_to_col(line, 10), 3, "emoji 尾缘=下一 char");
        assert_eq!(byte_index_to_col(line, 12), 5);
        assert_eq!(byte_index_to_col(line, 999), 5, "越界 clamp 行尾");
        assert_eq!(byte_index_to_col("", 0), 0);
    }

    // --- word_range_at ---

    #[test]
    fn word_range_at_ascii_word() {
        assert_eq!(word_range_at("hello world", 0), Some(0..5));
        assert_eq!(word_range_at("hello world", 4), Some(0..5));
        assert_eq!(word_range_at("hello world", 6), Some(6..11));
    }

    #[test]
    fn word_range_at_cjk_run_is_one_word() {
        // Lead 验收口径：CJK 按连续汉字算一个词（is_word_char 含汉字）。
        assert_eq!(word_range_at("中文混排 text", 0), Some(0..4));
        assert_eq!(word_range_at("中文混排 text", 3), Some(0..4));
        assert_eq!(word_range_at("中文混排 text", 5), Some(5..9));
    }

    #[test]
    fn word_range_at_snake_case_and_separators() {
        // 下划线是词字符（is_word_char）——snake_case 整体一个词。
        assert_eq!(word_range_at("my_var = 1", 2), Some(0..6));
        // 空白段与标点段分立（VS Code 口径：三类别各自连续段）。
        assert_eq!(word_range_at("a  --  b", 1), Some(1..3), "空白段");
        assert_eq!(word_range_at("a  --  b", 3), Some(3..5), "标点段");
        // 标点段：连续标点算一段（== 整段；两侧空白不并入）。
        assert_eq!(word_range_at("a == b", 2), Some(2..4));
        assert_eq!(word_range_at("a == b", 1), Some(1..2), "空白段不并标点");
    }

    #[test]
    fn word_range_at_line_edges() {
        assert_eq!(word_range_at("word", 4), Some(0..4), "行尾空白取最后一词");
        assert_eq!(word_range_at("w", 0), Some(0..1));
        assert_eq!(word_range_at("", 0), None);
        assert_eq!(word_range_at("   ", 1), Some(0..3), "全空白行选整段空白");
    }

    // --- line_range ---

    #[test]
    fn line_range_whole_line() {
        let b = Buffer::from_plain_text("abc\n中文\n");
        assert_eq!(line_range(&b, 0), (p(0, 0), p(0, 3)));
        assert_eq!(line_range(&b, 1), (p(1, 0), p(1, 2)));
        assert_eq!(line_range(&b, 2), (p(2, 0), p(2, 0)));
    }

    // --- drag_head ---

    #[test]
    fn drag_head_single_passthrough() {
        let b = Buffer::from_plain_text("one two\nthree");
        assert_eq!(drag_head(&b, ClickKind::Single, p(0, 0), p(1, 2)), p(1, 2));
    }

    #[test]
    fn drag_head_double_snaps_to_word_edges() {
        let b = Buffer::from_plain_text("alpha beta\nx");
        // 锚在 alpha 词中，向右拖到 beta 内 → 光标对齐 beta 右缘（col 10）。
        assert_eq!(drag_head(&b, ClickKind::Double, p(0, 2), p(0, 7)), p(0, 10));
        // 向左拖回 alpha 左缘。
        assert_eq!(
            drag_head(&b, ClickKind::Double, p(0, 2), p(0, 2)),
            p(0, 5),
            "同行向右（含原地）取词右缘"
        );
        assert_eq!(drag_head(&b, ClickKind::Double, p(0, 7), p(0, 1)), p(0, 0));
    }

    #[test]
    fn drag_head_triple_snaps_to_line_edges() {
        let b = Buffer::from_plain_text("aaa\nbbb\nccc");
        assert_eq!(
            drag_head(&b, ClickKind::Triple, p(0, 1), p(1, 1)),
            p(1, 3),
            "向下扩展取行尾"
        );
        assert_eq!(
            drag_head(&b, ClickKind::Triple, p(2, 1), p(0, 1)),
            p(0, 0),
            "向上扩展取行首"
        );
    }

    // --- autoscroll_dir ---

    #[test]
    fn autoscroll_dir_outside_only() {
        assert_eq!(
            autoscroll_dir(px(50.), px(100.), px(300.)),
            Some(ScrollDir::Up)
        );
        assert_eq!(
            autoscroll_dir(px(350.), px(100.), px(300.)),
            Some(ScrollDir::Down)
        );
        assert_eq!(
            autoscroll_dir(px(100.), px(100.), px(300.)),
            None,
            "顶缘不算出界"
        );
        assert_eq!(
            autoscroll_dir(px(300.), px(100.), px(300.)),
            None,
            "底缘不算出界"
        );
        assert_eq!(autoscroll_dir(px(200.), px(100.), px(300.)), None);
    }
}
