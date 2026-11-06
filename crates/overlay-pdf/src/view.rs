//! PDF 视图适配器（issue #239 §五「格式视图适配层」——本轮唯一实现
//! PdfOverlayViewAdapter）。
//!
//! 职责：[`RenderHint`]（语义锚点 `PdfSpan`）+ 页字符布局（注入）→
//! **PDF 页坐标四边形**（视图渲染的输入——屏幕像素换算/缩放归
//! mang-shell 视图层，本层止步于页坐标）。
//!
//! 「Locator ≠ 屏幕位置」的落点：本适配器不猜屏幕，只做锚点 → 页
//! 坐标的换算；字符布局来源（pdfium `positioned_chars` 经
//! [`crate::native::PageChars`] / 测试注入）由调用方提供。

use mdc_overlay::anchor::PrimaryLocator;
use mdc_overlay::geometry::{GeometryError, Quad};
use mdc_overlay::render::RenderHint;

use crate::PageChars;

/// 视图适配错误。
#[derive(Debug, Clone, PartialEq)]
pub enum ViewError {
    /// 非文本语义锚点（几何标注不产 RenderHint——防御面：进来的
    /// hint 必须持 PdfSpan）。
    NotPdfSpan { locator: &'static str },
    /// hint 锚点页与提供的页布局不匹配（页号不符——调用方页布局
    /// 集与 hint 不同页）。
    PageMismatch { hint_page: u32, provided_page: u32 },
    /// 字符区间不在页字符流域内（锚点与布局源不一致——如布局源
    /// 来自文档的另一版本）。
    Geometry(GeometryError),
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewError::NotPdfSpan { locator } => {
                write!(f, "锚点 {locator} 非 PdfSpan（视图高亮仅消费文本锚点）")
            }
            ViewError::PageMismatch {
                hint_page,
                provided_page,
            } => write!(
                f,
                "hint 锚点在第 {hint_page} 页，提供的布局是第 {provided_page} 页"
            ),
            ViewError::Geometry(e) => write!(f, "锚点 → 四边形派生失败：{e}"),
        }
    }
}
impl std::error::Error for ViewError {}

impl From<GeometryError> for ViewError {
    fn from(e: GeometryError) -> Self {
        ViewError::Geometry(e)
    }
}

/// PDF 视图适配器（无状态——纯换算面）。
#[derive(Debug, Clone, Copy, Default)]
pub struct PdfOverlayViewAdapter;

impl PdfOverlayViewAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 渲染意图 → PDF 页坐标高亮四边形。
    ///
    /// `page` 必须与 hint 锚点同页（页号 1 起——与
    /// [`PrimaryLocator::PdfSpan`] 同域）；字符区间闭区间 `[char_start,
    /// char_end]` 索引 `page.chars`。
    pub fn highlight_quads(
        &self,
        hint: &RenderHint,
        page: &PageChars,
    ) -> Result<Vec<Quad>, ViewError> {
        let span = match &hint.anchor.primary {
            PrimaryLocator::PdfSpan {
                page,
                char_start,
                char_end,
                ..
            } => (*page, *char_start, *char_end),
            other => {
                return Err(ViewError::NotPdfSpan {
                    locator: other.kind_name(),
                })
            }
        };
        if span.0 != page.page {
            return Err(ViewError::PageMismatch {
                hint_page: span.0,
                provided_page: page.page,
            });
        }
        let quads = mdc_overlay::geometry::quads_for_range(
            &page.chars,
            span.1 as usize,
            span.2 as usize,
            &page.page_box,
        )?;
        Ok(quads)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_evidence::LinkageStatus;
    use mdc_overlay::anchor::SemanticAnchor;
    use mdc_overlay::geometry::{CharBox, PageBox};
    use mdc_overlay::render::{OverlayStyle, RenderSemantic};
    use mdc_overlay::{AnnotationId, Format};

    fn page_chars(page: u32) -> PageChars {
        let chars: Vec<CharBox> = "投标文件综合评估"
            .chars()
            .enumerate()
            .map(|(i, c)| CharBox {
                ch: c,
                left: 100.0 + i as f64 * 10.0,
                bottom: 700.0,
                right: 100.0 + (i as f64 + 1.0) * 10.0,
                top: 710.0,
                font_size: 10.0,
            })
            .collect();
        PageChars {
            page,
            chars,
            page_box: PageBox {
                width_pt: 595.0,
                height_pt: 842.0,
            },
        }
    }

    fn hint(page: u32, cs: u32, ce: u32) -> RenderHint {
        RenderHint {
            annotation_id: AnnotationId::new("m1").unwrap(),
            anchor: SemanticAnchor::new(
                Format::Pdf,
                PrimaryLocator::PdfSpan {
                    page,
                    line: 1,
                    char_start: cs,
                    char_end: ce,
                },
                vec![],
                "fnv64:s",
            )
            .unwrap(),
            semantic: RenderSemantic::HighlightText,
            style: OverlayStyle::Emphasis,
            status: LinkageStatus::Valid,
        }
    }

    #[test]
    fn hint_to_view_quads_roundtrip() {
        let a = PdfOverlayViewAdapter::new();
        let quads = a.highlight_quads(&hint(1, 2, 5), &page_chars(1)).unwrap();
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].points[0].x, 120.0, "第 3 字符左缘（闭区间）");
        assert_eq!(quads[0].points[1].x, 160.0, "第 6 字符右缘");
    }

    #[test]
    fn page_mismatch_rejected() {
        let a = PdfOverlayViewAdapter::new();
        let err = a
            .highlight_quads(&hint(2, 0, 3), &page_chars(1))
            .unwrap_err();
        assert!(matches!(err, ViewError::PageMismatch { .. }));
    }

    #[test]
    fn range_beyond_layout_rejected() {
        let a = PdfOverlayViewAdapter::new();
        let err = a
            .highlight_quads(&hint(1, 0, 99), &page_chars(1))
            .unwrap_err();
        assert!(matches!(
            err,
            ViewError::Geometry(GeometryError::BadRange { .. })
        ));
    }
}
