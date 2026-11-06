//! 字符几何与行带四边形（纯函数，零引擎依赖）。
//!
//! 「文本锚点 → QuadPoints」的算法面在这里；**字符几何的来源**
//! （pdfium `positioned_chars` / 其他布局引擎 / 测试注入）由调用方
//! 注入为 [`CharBox`] 列表——依赖倒置，本层不知道任何引擎。
//!
//! 算法（与 mdc-pdf 表格提取 #232 的行带聚类同族思想）：
//! 1. 取 `chars[start..=end]`（闭区间——与 `PrimaryLocator::PdfSpan`
//!    的 `char_start`/`char_end` 同语义）；
//! 2. 按字符垂直中心 `(top + bottom) / 2` 聚成行带（容差 = 行带内
//!    字高的一半——跨字号行取保守值）；
//! 3. 每行带产一个 [`Quad`]：x = 行内字符包围盒并集，y = 行带
//!    上下界（**不加 pad**——纯字符 bbox 并集，诚实几何；Acrobat
//!    视觉加粗属阅读器渲染）；
//! 4. 每个_quad 落页界内校验（[`PageBox`]）——越界 = 显式错误，
//!    不静默截断。

use serde::{Deserialize, Serialize};

/// 平面点（几何模型的标量面 = f64；具体引擎的精度由 adapter 换算）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointF {
    pub x: f64,
    pub y: f64,
}

/// 几何四边形（PDF QuadPoints 语义：四角点序 = 环序，p0→p1 为文本行
/// 方向边；`Quad::from_rect` 产出底边 = 文本方向边的标准环序）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quad {
    pub points: [PointF; 4],
}

impl Quad {
    /// 轴对齐矩形 → 环序四边形：
    /// p0=(x0,y0) → p1=(x1,y0)（底边，文本方向边）→ p2=(x1,y1) →
    /// p3=(x0,y1)。要求 x0 ≤ x1 且 y0 ≤ y1（有限性由调用方保证——
    /// 本构造点在 [`quads_for_range`] 内部，输入已过有限性检查）。
    pub fn from_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Quad {
            points: [
                PointF { x: x0, y: y0 },
                PointF { x: x1, y: y0 },
                PointF { x: x1, y: y1 },
                PointF { x: x0, y: y1 },
            ],
        }
    }

    /// 四点全有限。
    pub fn is_finite(&self) -> bool {
        self.points
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite())
    }
}

/// 页面盒（页界校验的基准——PDF 用户空间 pt）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageBox {
    pub width_pt: f64,
    pub height_pt: f64,
}

/// 格式无关的字符几何（一字符一包围盒 + 字号——布局引擎输出的
/// 最小公共面；pdfium `PositionedChar` 经 adapter 转入）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CharBox {
    pub ch: char,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
    pub font_size: f64,
}

/// 字符区间 → 四边形的错误。
#[derive(Debug, Clone, PartialEq)]
pub enum GeometryError {
    /// 区间不在字符流域内（`start > end` 或 `end >= chars.len()`）。
    BadRange {
        len: usize,
        start: usize,
        end: usize,
    },
    /// 坐标非有限（布局引擎产出了 NaN/Inf——污染不入投影）。
    NonFinite { index: usize },
    /// 派生四边形越出页界（含坐标——报告层可诊断，不静默截断）。
    OutOfBounds { page: PageBox, quad: Quad },
}

impl std::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeometryError::BadRange { len, start, end } => {
                write!(f, "字符区间 [{start}, {end}] 不在字符流域内（len={len}）")
            }
            GeometryError::NonFinite { index } => {
                write!(f, "字符 #{index} 坐标非有限")
            }
            GeometryError::OutOfBounds { page, quad } => {
                write!(
                    f,
                    "四边形越出页界 {width}×{height}：{quad:?}",
                    width = page.width_pt,
                    height = page.height_pt,
                )
            }
        }
    }
}
impl std::error::Error for GeometryError {}

/// 字符区间（闭区间）→ 行带四边形列表。
///
/// 页界校验在产 quad 后逐个进行——**任何**越界都整体报错（部分
/// 成功部分失败会导致「高亮只画了一半」的静默降级，违反三原则③）。
pub fn quads_for_range(
    chars: &[CharBox],
    start: usize,
    end: usize,
    page: &PageBox,
) -> Result<Vec<Quad>, GeometryError> {
    if start > end || end >= chars.len() {
        return Err(GeometryError::BadRange {
            len: chars.len(),
            start,
            end,
        });
    }
    let slice = &chars[start..=end];
    for (i, c) in slice.iter().enumerate() {
        let finite =
            c.left.is_finite() && c.bottom.is_finite() && c.right.is_finite() && c.top.is_finite();
        if !finite {
            return Err(GeometryError::NonFinite { index: start + i });
        }
    }

    // 行带聚类：垂直中心排序后按容差切带（容差 = 当前带最大字高的
    // 一半——跨字号行的保守聚类；行序 = y 降序[页面上行在前]）。
    let mut order: Vec<&CharBox> = slice.iter().collect();
    order.sort_by(|a, b| {
        let ca = (a.top + a.bottom) as f32;
        let cb = (b.top + b.bottom) as f32;
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut quads: Vec<Quad> = Vec::new();
    let mut band: Vec<&CharBox> = Vec::new();
    let mut band_tolerance = 0.0f64;
    let flush = |band: &Vec<&CharBox>, quads: &mut Vec<Quad>| {
        if band.is_empty() {
            return;
        }
        let x0 = band.iter().map(|c| c.left).fold(f64::INFINITY, f64::min);
        let x1 = band
            .iter()
            .map(|c| c.right)
            .fold(f64::NEG_INFINITY, f64::max);
        let y0 = band.iter().map(|c| c.bottom).fold(f64::INFINITY, f64::min);
        let y1 = band.iter().map(|c| c.top).fold(f64::NEG_INFINITY, f64::max);
        quads.push(Quad::from_rect(x0, y0, x1, y1));
    };
    for c in order {
        let center = (c.top + c.bottom) / 2.0;
        let char_half = (c.top - c.bottom).abs() / 2.0;
        let tolerance = char_half.max(f64::MIN_POSITIVE);
        if band.is_empty() {
            band.push(c);
            band_tolerance = tolerance;
            continue;
        }
        let band_center = {
            let f = band.last().unwrap();
            (f.top + f.bottom) / 2.0
        };
        if (center - band_center).abs() <= band_tolerance.max(tolerance) {
            band.push(c);
            band_tolerance = band_tolerance.max(tolerance);
        } else {
            flush(&band, &mut quads);
            band.clear();
            band.push(c);
            band_tolerance = tolerance;
        }
    }
    flush(&band, &mut quads);

    // 页界校验（全量成功或整体失败——不静默截断）。
    for q in &quads {
        let xs = [q.points[0].x, q.points[1].x, q.points[2].x, q.points[3].x];
        let ys = [q.points[0].y, q.points[1].y, q.points[2].y, q.points[3].y];
        let x0 = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let x1 = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let y0 = ys.iter().cloned().fold(f64::INFINITY, f64::min);
        let y1 = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if x0 < 0.0 || y0 < 0.0 || x1 > page.width_pt || y1 > page.height_pt {
            return Err(GeometryError::OutOfBounds {
                page: *page,
                quad: *q,
            });
        }
    }
    Ok(quads)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单行网格字符（CJK 形态：宽 = 高 = 字号）。
    fn ch(c: char, x: f64, y: f64, size: f64) -> CharBox {
        CharBox {
            ch: c,
            left: x,
            bottom: y,
            right: x + size,
            top: y + size,
            font_size: size,
        }
    }

    const PAGE: PageBox = PageBox {
        width_pt: 595.0,
        height_pt: 842.0,
    };

    #[test]
    fn single_line_range_yields_one_quad() {
        // 「综合评估」四字一行（x = 100 起步，10pt）。
        let chars: Vec<CharBox> = "综合评估投标办法"
            .chars()
            .enumerate()
            .map(|(i, c)| ch(c, 100.0 + i as f64 * 10.0, 700.0, 10.0))
            .collect();
        let quads = quads_for_range(&chars, 0, 3, &PAGE).unwrap();
        assert_eq!(quads.len(), 1, "单行 = 单 quad");
        let q = quads[0];
        assert_eq!(q.points[0].x, 100.0);
        assert_eq!(q.points[1].x, 140.0, "x 并集 = 行宽");
        assert_eq!(q.points[0].y, 700.0, "底边 y0");
        assert_eq!(q.points[2].y, 710.0, "顶边 y1");
        // 环序：p0→p1 为文本方向底边。
        assert_eq!(q.points[0], PointF { x: 100.0, y: 700.0 });
        assert_eq!(q.points[1], PointF { x: 140.0, y: 700.0 });
        assert_eq!(q.points[2], PointF { x: 140.0, y: 710.0 });
        assert_eq!(q.points[3], PointF { x: 100.0, y: 710.0 });
    }

    #[test]
    fn two_visual_lines_yield_two_quads_in_page_order() {
        // 8 字符：前 4 在 y=700 行，后 4 在 y=680 行（页面上行在前）。
        let mut chars = Vec::new();
        for (i, c) in "第一行文字内容".chars().enumerate() {
            chars.push(ch(c, 100.0 + (i % 4) as f64 * 10.0, 700.0, 10.0));
        }
        for (i, c) in "第二行文字内容".chars().enumerate() {
            chars.push(ch(c, 100.0 + i as f64 * 10.0, 680.0, 10.0));
        }
        let quads = quads_for_range(&chars, 0, 7, &PAGE).unwrap();
        assert_eq!(quads.len(), 2, "两行 = 两 quad");
        // 行序 = 页面上行在前（y 降序）。
        assert!(quads[0].points[0].y > quads[1].points[0].y);
    }

    #[test]
    fn bad_range_is_rejected() {
        let chars = vec![ch('a', 0.0, 0.0, 10.0)];
        assert!(matches!(
            quads_for_range(&chars, 1, 0, &PAGE).unwrap_err(),
            GeometryError::BadRange { .. }
        ));
        assert!(matches!(
            quads_for_range(&chars, 0, 1, &PAGE).unwrap_err(),
            GeometryError::BadRange { .. }
        ));
    }

    #[test]
    fn out_of_bounds_quad_is_rejected_not_clipped() {
        // 字符右缘超页宽（595）→ 整体拒（不静默截断）。
        let chars = vec![ch('越', 590.0, 700.0, 10.0)];
        let err = quads_for_range(&chars, 0, 0, &PAGE).unwrap_err();
        assert!(matches!(err, GeometryError::OutOfBounds { .. }));
    }

    #[test]
    fn nonfinite_char_is_rejected() {
        let mut c = ch('坏', 100.0, 700.0, 10.0);
        c.right = f64::NAN;
        assert!(matches!(
            quads_for_range(&[c], 0, 0, &PAGE).unwrap_err(),
            GeometryError::NonFinite { .. }
        ));
    }
}
