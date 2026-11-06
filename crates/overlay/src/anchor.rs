//! 语义定位与几何定位（issue #239 §三——本架构最关键的一条线）。
//!
//! ```text
//! 语义定位（SemanticAnchor）          几何定位（GeometricSpan）
//!   标注的是哪段文字                    应该画在哪个页面区域
//!   要求：编辑后仍能找回                要求：能生成 QuadPoints/矩形
//!   （主定位 + 降级链 + 指纹）          （坐标空间必须显式声明）
//!
//!   ✗ 把页面坐标当稳定的文本锚点
//!   ✗ 指望文本偏移本身能生成 QuadPoints
//! ```
//!
//! 两者在 [`crate::mark::OverlayMark`] 的不同变体上各就其位：文本语义
//! 标注（Emphasis/Comment）持锚点；几何标注（Freehand/Region）持
//! span。一条 Emphasis 要投影为 PDF /Highlight 时，几何由投影方
//! （adapter）经字符布局（[`crate::geometry`]）从锚点**派生**——派生
//! 失败显式报错，不静默丢。

use serde::{Deserialize, Serialize};

use crate::geometry::Quad;
use crate::Format;

/// 语义锚点构造/校验错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorError {
    /// 主定位器变体与 format 声明不一致（如 format=Pdf 而主定位是
    /// DocxSpan——分派面自相矛盾，后续一切按 format 分派的逻辑
    /// 都会走错分支）。
    FormatMismatch {
        format: Format,
        locator: &'static str,
    },
    /// 指纹为空（锚点必须携带定位时刻的内容指纹——空指纹使
    /// "位置还在但内容已换"的二级失效判定永远不可能成立）。
    EmptyFingerprint,
    /// 字符区间反序（char_start > char_end）。
    BadCharRange { start: u32, end: u32 },
    /// 降级链查询串为空。
    EmptyFallbackNeedle,
    /// PDF 页号为 0（页号 1 起——与 mdc-evidence PdfPageText 同域）。
    ZeroPage,
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = match self {
            AnchorError::FormatMismatch { format, locator } => {
                write!(f, "format {format:?} 与主定位器 {locator} 不一致")
            }
            AnchorError::EmptyFingerprint => f.write_str("锚点指纹为空"),
            AnchorError::BadCharRange { start, end } => {
                write!(f, "字符区间反序：{start} > {end}")
            }
            AnchorError::EmptyFallbackNeedle => f.write_str("降级链查询串为空"),
            AnchorError::ZeroPage => f.write_str("PDF 页号必须 1 起"),
        };
        m
    }
}
impl std::error::Error for AnchorError {}

/// 语义锚点：说的是"哪段内容"，与几何无关。
///
/// `primary` 是最精确的定位；`fallback` 是降级链（主定位失效时依次
/// 尝试——v0 形状：文本查询；寻址策略属宿主/adapter，本层只保证链
/// 非空性与可序列化）。`fingerprint` 是定位时刻的内容指纹（算法前缀
/// 编码，循 mdc-evidence `ContentFingerprint` 约定——非防篡改，
/// 用于"位置还在但内容已换"的二级失效判定）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticAnchor {
    pub format: Format,
    pub primary: PrimaryLocator,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<FallbackLocator>,
    /// 定位时刻内容指纹（`algorithm:value` 形态，如 `fnv64:…`）。
    pub fingerprint: String,
}

impl SemanticAnchor {
    /// 构造即校验（format ↔ primary 一致性 + 区间序 + 指纹非空 +
    /// 降级链形状）。
    pub fn new(
        format: Format,
        primary: PrimaryLocator,
        fallback: Vec<FallbackLocator>,
        fingerprint: impl Into<String>,
    ) -> Result<Self, AnchorError> {
        let anchor = SemanticAnchor {
            format,
            primary,
            fallback,
            fingerprint: fingerprint.into(),
        };
        anchor.validate()?;
        Ok(anchor)
    }

    /// 校验（反序列化入口复用同一套不变量）。
    pub fn validate(&self) -> Result<(), AnchorError> {
        use PrimaryLocator::*;
        let ok = match &self.primary {
            PdfSpan {
                page,
                char_start,
                char_end,
                ..
            } => {
                if *page == 0 {
                    return Err(AnchorError::ZeroPage);
                }
                if char_start > char_end {
                    return Err(AnchorError::BadCharRange {
                        start: *char_start,
                        end: *char_end,
                    });
                }
                self.format == Format::Pdf
            }
            DocxSpan {
                char_start,
                char_end,
                ..
            } => {
                if char_start > char_end {
                    return Err(AnchorError::BadCharRange {
                        start: *char_start,
                        end: *char_end,
                    });
                }
                self.format == Format::Docx
            }
            XlsxCell { .. } => self.format == Format::Xlsx,
            MdBlock { .. } => self.format == Format::Md,
            CanvasObj { .. } => self.format == Format::Canvas,
        };
        if !ok {
            return Err(AnchorError::FormatMismatch {
                format: self.format,
                locator: self.primary.kind_name(),
            });
        }
        if self.fingerprint.trim().is_empty() {
            return Err(AnchorError::EmptyFingerprint);
        }
        for fb in &self.fallback {
            if fb.needle().trim().is_empty() {
                return Err(AnchorError::EmptyFallbackNeedle);
            }
        }
        Ok(())
    }
}

/// 主定位器（每变体只携带该格式内**可稳定重寻址**的最小信息）。
///
/// 刻意不做的事：不发明跨格式统一坐标（分格式变体 = 分派面）；
/// 不携带页面几何（那是 [`GeometricSpan`] / adapter 派生面的职责）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrimaryLocator {
    /// PDF 文本区间：1 起页号 + 行号（同一视觉行的字符带）+ 页内
    /// 字符流闭区间（`char_start`/`char_end`——字符流 = 文本布局
    /// 引擎的输出序，视图/锚点创建/投影必须用同一字符流源）。
    PdfSpan {
        page: u32,
        line: u32,
        char_start: u32,
        char_end: u32,
    },
    /// DOCX 文本区间：0 起段落序 + 段内字符闭区间。
    DocxSpan {
        paragraph_index: u32,
        char_start: u32,
        char_end: u32,
    },
    /// XLSX 单元格：工作表 + 1 起行列。
    XlsxCell { sheet: String, row: u32, col: u32 },
    /// Markdown 块（块 ID 寻址是 mdcore 域既有能力）。
    MdBlock { block_id: String },
    /// Canvas 画布对象（对象身份 = 画布对象 ID）。
    CanvasObj { object_id: String },
}

impl PrimaryLocator {
    /// 变体名（错误消息与索引键的组成部分）。
    pub fn kind_name(&self) -> &'static str {
        match self {
            PrimaryLocator::PdfSpan { .. } => "PdfSpan",
            PrimaryLocator::DocxSpan { .. } => "DocxSpan",
            PrimaryLocator::XlsxCell { .. } => "XlsxCell",
            PrimaryLocator::MdBlock { .. } => "MdBlock",
            PrimaryLocator::CanvasObj { .. } => "CanvasObj",
        }
    }

    /// 规范键（`format + 定位坐标` 的确定性字符串——双向索引的键
    /// 形态；同锚点恒同键，与序列化形态解耦）。
    pub fn key(&self) -> String {
        match self {
            PrimaryLocator::PdfSpan {
                page,
                line,
                char_start,
                char_end,
            } => {
                format!("pdf:{page}:{line}:{char_start}:{char_end}")
            }
            PrimaryLocator::DocxSpan {
                paragraph_index,
                char_start,
                char_end,
            } => {
                format!("docx:{paragraph_index}:{char_start}:{char_end}")
            }
            PrimaryLocator::XlsxCell { sheet, row, col } => {
                format!("xlsx:{sheet}:{row}:{col}")
            }
            PrimaryLocator::MdBlock { block_id } => format!("md:{block_id}"),
            PrimaryLocator::CanvasObj { object_id } => format!("canvas:{object_id}"),
        }
    }
}

/// 降级链节点（主定位失效时依次尝试）。v0 唯一形状 = 文本查询；
/// 链的**寻址策略**（在哪搜、怎么消歧）属宿主/adapter，本层只保证
/// 查询串非空可序列化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FallbackLocator {
    /// 文本查询（如原文摘录——内容指纹失效后按文本重寻）。
    TextQuery { needle: String },
}

impl FallbackLocator {
    pub fn needle(&self) -> &str {
        match self {
            FallbackLocator::TextQuery { needle } => needle,
        }
    }
}

// ---------------------------------------------------------------------------
// 几何定位（画在哪——坐标空间必须显式）
// ---------------------------------------------------------------------------

/// PDF 用户空间原点约定（v0 唯一：PDF 32000 默认左下原点；枚举形状
/// 预留翻转面——Windows GDI 系源数据转换时用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PdfOrigin {
    /// 左下原点（PDF 用户空间标准——y 向上）。
    BottomLeft,
}

/// 坐标空间（持久化模型只认文档空间；视图/屏幕坐标不入持久化——
/// 那是临时的，缩放/滚动即失效）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CoordSpace {
    /// PDF 页面空间：页宽/高（pt）+ 原点约定。
    PdfPage {
        width_pt: f64,
        height_pt: f64,
        origin: PdfOrigin,
    },
    /// Canvas 世界空间：画布宽/高（世界单位）。
    CanvasWorld { width: f64, height: f64 },
}

/// 轴对齐包围盒（区域标注的最小几何）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// 几何定位：说的是"画在哪"。空间标注必须带：目标页面/画布对象 +
/// 坐标空间及原点 + 尺寸变化后的定位规则（`geom_fingerprint`——
/// 重新排版后失效的快照指纹）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometricSpan {
    /// 坐标空间（必须显式，不能默认）。
    pub space: CoordSpace,
    /// 分页格式才有（PDF 页号 1 起；Canvas = None）。
    pub page: Option<u32>,
    /// 文本高亮的四边形列表（Freehand 墨迹点列在 v0 不建模——
    /// 见 issue #239「候选设计不进第一版」边界）。
    pub quads: Vec<Quad>,
    /// 区域标注的包围盒。
    pub bounds: Option<Bounds>,
    /// 几何指纹（几何快照的指纹——重新排版后失效）。
    pub geom_fingerprint: String,
}

impl GeometricSpan {
    /// 构造即校验：坐标全有限 + 空间/页号一致（PDF ⇒ Some(1 起页)，
    /// Canvas ⇒ None）+ quads/bounds 至少一样非空 + 几何指纹非空。
    pub fn new(
        space: CoordSpace,
        page: Option<u32>,
        quads: Vec<Quad>,
        bounds: Option<Bounds>,
        geom_fingerprint: impl Into<String>,
    ) -> Result<Self, SpanError> {
        let span = GeometricSpan {
            space,
            page,
            quads,
            bounds,
            geom_fingerprint: geom_fingerprint.into(),
        };
        span.validate()?;
        Ok(span)
    }

    pub fn validate(&self) -> Result<(), SpanError> {
        match self.space {
            CoordSpace::PdfPage { .. } => {
                if self.page.is_none() {
                    return Err(SpanError::MissingPage);
                }
                if self.page == Some(0) {
                    return Err(SpanError::ZeroPage);
                }
            }
            CoordSpace::CanvasWorld { .. } => {
                if self.page.is_some() {
                    return Err(SpanError::UnexpectedPage);
                }
            }
        }
        if self.quads.is_empty() && self.bounds.is_none() {
            return Err(SpanError::EmptyGeometry);
        }
        let finite = self
            .quads
            .iter()
            .all(|q| q.points.iter().all(|p| p.x.is_finite() && p.y.is_finite()))
            && self.bounds.is_none_or(|b| {
                b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite()
            });
        if !finite {
            return Err(SpanError::NonFinite);
        }
        if let Some(b) = self.bounds {
            if b.x0 > b.x1 || b.y0 > b.y1 {
                return Err(SpanError::InvertedBounds);
            }
        }
        if self.geom_fingerprint.trim().is_empty() {
            return Err(SpanError::EmptyFingerprint);
        }
        Ok(())
    }
}

/// 几何 span 构造/校验错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanError {
    /// PDF 空间缺页号。
    MissingPage,
    /// 页号 0（1 起）。
    ZeroPage,
    /// Canvas 空间带页号。
    UnexpectedPage,
    /// quads 与 bounds 双空（几何 span 必须有几何）。
    EmptyGeometry,
    /// 坐标非有限值（NaN/Inf 不入持久化模型）。
    NonFinite,
    /// 包围盒反序。
    InvertedBounds,
    /// 几何指纹为空。
    EmptyFingerprint,
}

impl std::fmt::Display for SpanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = match self {
            SpanError::MissingPage => "PDF 空间必须带页号",
            SpanError::ZeroPage => "页号必须 1 起",
            SpanError::UnexpectedPage => "Canvas 世界空间无页号",
            SpanError::EmptyGeometry => "quads 与 bounds 双空",
            SpanError::NonFinite => "坐标含非有限值",
            SpanError::InvertedBounds => "包围盒反序",
            SpanError::EmptyFingerprint => "几何指纹为空",
        };
        f.write_str(m)
    }
}
impl std::error::Error for SpanError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf_span() -> PrimaryLocator {
        PrimaryLocator::PdfSpan {
            page: 3,
            line: 7,
            char_start: 120,
            char_end: 131,
        }
    }

    #[test]
    fn anchor_validates_format_primary_consistency() {
        // format=Pdf + PdfSpan：过。
        let a = SemanticAnchor::new(Format::Pdf, pdf_span(), vec![], "fnv64:ab12").unwrap();
        assert_eq!(a.primary.key(), "pdf:3:7:120:131");

        // format=Docx + PdfSpan：拒（分派面自相矛盾）。
        let err = SemanticAnchor::new(Format::Docx, pdf_span(), vec![], "fnv64:ab12").unwrap_err();
        assert!(matches!(err, AnchorError::FormatMismatch { .. }));
    }

    #[test]
    fn anchor_rejects_bad_shapes() {
        // 空指纹拒。
        let e = SemanticAnchor::new(Format::Pdf, pdf_span(), vec![], "  ").unwrap_err();
        assert_eq!(e, AnchorError::EmptyFingerprint);
        // 反序区间拒。
        let bad = PrimaryLocator::PdfSpan {
            page: 1,
            line: 0,
            char_start: 9,
            char_end: 3,
        };
        let e = SemanticAnchor::new(Format::Pdf, bad, vec![], "fnv64:x").unwrap_err();
        assert_eq!(e, AnchorError::BadCharRange { start: 9, end: 3 });
        // 0 页拒。
        let bad = PrimaryLocator::PdfSpan {
            page: 0,
            line: 0,
            char_start: 0,
            char_end: 1,
        };
        let e = SemanticAnchor::new(Format::Pdf, bad, vec![], "fnv64:x").unwrap_err();
        assert_eq!(e, AnchorError::ZeroPage);
        // 空降级查询串拒。
        let e = SemanticAnchor::new(
            Format::Pdf,
            pdf_span(),
            vec![FallbackLocator::TextQuery { needle: " ".into() }],
            "fnv64:x",
        )
        .unwrap_err();
        assert_eq!(e, AnchorError::EmptyFallbackNeedle);
    }

    #[test]
    fn geometric_span_space_page_contract() {
        let quad = Quad::from_rect(0.0, 0.0, 10.0, 10.0);
        let space = CoordSpace::PdfPage {
            width_pt: 595.0,
            height_pt: 842.0,
            origin: PdfOrigin::BottomLeft,
        };
        // PDF + 页号 + 几何：过。
        assert!(GeometricSpan::new(space, Some(1), vec![quad], None, "fnv64:g1").is_ok());
        // PDF 无页号：拒。
        assert_eq!(
            GeometricSpan::new(space, None, vec![quad], None, "fnv64:g1").unwrap_err(),
            SpanError::MissingPage
        );
        // 双空几何：拒。
        assert_eq!(
            GeometricSpan::new(space, Some(1), vec![], None, "fnv64:g1").unwrap_err(),
            SpanError::EmptyGeometry
        );
        // Canvas 空间 + 页号：拒。
        let cs = CoordSpace::CanvasWorld {
            width: 100.0,
            height: 100.0,
        };
        assert_eq!(
            GeometricSpan::new(cs, Some(1), vec![quad], None, "fnv64:g1").unwrap_err(),
            SpanError::UnexpectedPage
        );
        // Canvas + bounds：过（区域圈选形态）。
        assert!(GeometricSpan::new(
            cs,
            None,
            vec![],
            Some(Bounds {
                x0: 0.0,
                y0: 0.0,
                x1: 50.0,
                y1: 50.0
            }),
            "fnv64:g2"
        )
        .is_ok());
    }

    #[test]
    fn geometric_span_rejects_nonfinite_and_inverted() {
        let space = CoordSpace::PdfPage {
            width_pt: 595.0,
            height_pt: 842.0,
            origin: PdfOrigin::BottomLeft,
        };
        let mut bad_quad = Quad::from_rect(0.0, 0.0, 10.0, 10.0);
        bad_quad.points[1].x = f64::NAN;
        assert_eq!(
            GeometricSpan::new(space, Some(1), vec![bad_quad], None, "fnv64:g").unwrap_err(),
            SpanError::NonFinite
        );
        assert_eq!(
            GeometricSpan::new(
                space,
                Some(1),
                vec![],
                Some(Bounds {
                    x0: 10.0,
                    y0: 0.0,
                    x1: 5.0,
                    y1: 5.0
                }),
                "fnv64:g"
            )
            .unwrap_err(),
            SpanError::InvertedBounds
        );
    }
}
