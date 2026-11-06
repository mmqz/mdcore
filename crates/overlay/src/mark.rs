//! OverlayMark：格式无关的语义模型（issue #239 §四）。
//!
//! **不复用 mdc-pdf::AnnotationKind**（用户原话：PDF 的 QuadPoints/
//! Ink/Rect 是 PDF 的表达方式，应由 PDF Adapter 负责映射）——本模型
//! 表达的是**语义**（这段重要 / 对这段的评论 / 手绘 / 区域圈选），
//! 格式映射见 issue §四映射表（本轮只实装 PDF Emphasis → /Highlight）。

use mdc_evidence::{DocumentRef, LinkageStatus};
use serde::{Deserialize, Serialize};

use crate::anchor::{GeometricSpan, SemanticAnchor};

/// 标注 id（sidecar 稳定身份——跨会话/跨视图/跨投影关联的锚；非空
/// 字符串，查重由 [`crate::store::OverlaySidecar`] / [`crate::index::OverlayIndex`]
/// 强制）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnnotationId(pub String);

impl AnnotationId {
    /// 非空校验构造（空白 id 使「按 id 关联投影报告/索引/渲染」全链
    /// 不可判——构造点拒绝）。
    pub fn new(s: impl Into<String>) -> Result<Self, MarkError> {
        let s = s.into();
        if s.trim().is_empty() {
            return Err(MarkError::EmptyAnnotationId);
        }
        Ok(AnnotationId(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 标注模型层错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkError {
    EmptyAnnotationId,
    /// Freehand/Region 变体的 `document` 与存储单元宿主文档不一致
    ///（sidecar 单文档纪律：entry.document 是唯一归属真相）。
    DocumentMismatch {
        entry: String,
        mark: String,
    },
    /// Comment 文本为空（评论必须有内容——空评论文本无语义）。
    EmptyComment,
}

impl std::fmt::Display for MarkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarkError::EmptyAnnotationId => f.write_str("标注 id 为空"),
            MarkError::DocumentMismatch { entry, mark } => write!(
                f,
                "几何标注归属文档（{mark}）与存储单元宿主（{entry}）不一致"
            ),
            MarkError::EmptyComment => f.write_str("Comment 文本为空"),
        }
    }
}
impl std::error::Error for MarkError {}

/// 格式无关的语义标注模型（四变体——跨文档关系 Link 是候选设计，
/// 不进第一版）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayMark {
    /// 强调某段文字（语义：这段重要）。→ PDF /Highlight + QuadPoints
    ///（投影实装面，本轮唯一）。
    Emphasis {
        anchor: SemanticAnchor,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// 对某段文字的评论。→ PDF /Text Note（本轮投影 unsupported——
    /// 能力矩阵形状已定义，实装等 PDF Emphasis 链路验证后）。
    Comment {
        anchor: SemanticAnchor,
        text: String,
    },
    /// 手绘笔迹（几何标注——坐标空间显式）。
    Freehand {
        /// 明确属于哪份文档（几何标注的归属必须显式）。
        document: DocumentRef,
        span: GeometricSpan,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// 区域圈选（几何标注）。
    Region {
        document: DocumentRef,
        span: GeometricSpan,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

impl OverlayMark {
    /// 变体名（报告与错误消息用）。
    pub fn kind_name(&self) -> &'static str {
        match self {
            OverlayMark::Emphasis { .. } => "Emphasis",
            OverlayMark::Comment { .. } => "Comment",
            OverlayMark::Freehand { .. } => "Freehand",
            OverlayMark::Region { .. } => "Region",
        }
    }

    /// 语义锚点（Emphasis/Comment 有；几何标注无——None 是诚实值，
    /// 不是缺数据）。
    pub fn anchor(&self) -> Option<&SemanticAnchor> {
        match self {
            OverlayMark::Emphasis { anchor, .. } | OverlayMark::Comment { anchor, .. } => {
                Some(anchor)
            }
            OverlayMark::Freehand { .. } | OverlayMark::Region { .. } => None,
        }
    }

    /// 几何标注的归属文档（语义标注的归属由存储单元宿主承载）。
    pub fn geometric_document(&self) -> Option<&DocumentRef> {
        match self {
            OverlayMark::Freehand { document, .. } | OverlayMark::Region { document, .. } => {
                Some(document)
            }
            OverlayMark::Emphasis { .. } | OverlayMark::Comment { .. } => None,
        }
    }
}

/// 存储与索引单元：标注 + 稳定身份 + 归属文档 + 关联状态。
///
/// 归属文档在单元层显式化（issue 数据结构里 Freehand/Region 自带
/// `document`——本类型把「每条标注属于哪份文档」提升为所有变体的
/// 统一不变量：`document` 字段是唯一归属真相，几何标注变体内的
/// `document` 必须与之一致[构造校验]，语义标注经单元归属）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayEntry {
    pub id: AnnotationId,
    /// 归属文档（v0 sidecar 单文档：全部 entry 与 sidecar 宿主一致）。
    pub document: DocumentRef,
    pub mark: OverlayMark,
    /// 来源关联状态（复用 mdc-evidence 三态——登记时 Valid，源变更
    /// 后由验证方降级 Stale，**不得静默重绑**）。
    #[serde(default = "default_linkage")]
    pub linkage: LinkageStatus,
    /// 登记时刻（epoch ms——宿主时钟注入，本层不取系统时间）。
    pub created_at_ms: u64,
}

fn default_linkage() -> LinkageStatus {
    LinkageStatus::Valid
}

impl OverlayEntry {
    /// 构造即校验：id 非空 + 几何标注归属一致 + Comment 非空文本 +
    /// 锚点/span 自身合法（`SemanticAnchor::validate` /
    /// `GeometricSpan::validate` 透传）。
    pub fn new(
        id: AnnotationId,
        document: DocumentRef,
        mark: OverlayMark,
        created_at_ms: u64,
    ) -> Result<Self, EntryError> {
        AnnotationId::new(id.as_str()).map_err(EntryError::Mark)?;
        if let OverlayMark::Comment { text, .. } = &mark {
            if text.trim().is_empty() {
                return Err(EntryError::Mark(MarkError::EmptyComment));
            }
        }
        if let Some(mark_doc) = mark.geometric_document() {
            if mark_doc != &document {
                return Err(EntryError::Mark(MarkError::DocumentMismatch {
                    entry: document.doc_id.as_str().to_string(),
                    mark: mark_doc.doc_id.as_str().to_string(),
                }));
            }
        }
        if let Some(anchor) = mark.anchor() {
            anchor.validate().map_err(EntryError::Anchor)?;
        }
        if let OverlayMark::Freehand { span, .. } | OverlayMark::Region { span, .. } = &mark {
            span.validate().map_err(EntryError::Span)?;
        }
        Ok(OverlayEntry {
            id,
            document,
            mark,
            linkage: LinkageStatus::Valid,
            created_at_ms,
        })
    }
}

/// 存储单元构造/校验错误（三个子层的透传汇总）。
#[derive(Debug, Clone, PartialEq)]
pub enum EntryError {
    Mark(MarkError),
    Anchor(crate::anchor::AnchorError),
    Span(crate::anchor::SpanError),
}

impl std::fmt::Display for EntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryError::Mark(e) => write!(f, "标注模型：{e}"),
            EntryError::Anchor(e) => write!(f, "语义锚点：{e}"),
            EntryError::Span(e) => write!(f, "几何定位：{e}"),
        }
    }
}
impl std::error::Error for EntryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::PrimaryLocator;
    use crate::Format;
    use mdc_evidence::{ContentFingerprint, DocId, EngineKind, VersionStamp};

    fn doc(id: &str) -> DocumentRef {
        DocumentRef::new(
            DocId::new(id),
            EngineKind::Pdf,
            VersionStamp(1),
            ContentFingerprint("fnv64:aa".into()),
        )
    }

    fn emphasis() -> OverlayMark {
        OverlayMark::Emphasis {
            anchor: SemanticAnchor::new(
                Format::Pdf,
                PrimaryLocator::PdfSpan {
                    page: 1,
                    line: 3,
                    char_start: 10,
                    char_end: 14,
                },
                vec![],
                "fnv64:xx",
            )
            .unwrap(),
            note: None,
        }
    }

    #[test]
    fn entry_validates_geometric_document_consistency() {
        let e = OverlayEntry::new(
            AnnotationId::new("m1").unwrap(),
            doc("host"),
            emphasis(),
            1_000,
        )
        .unwrap();
        assert_eq!(e.linkage, LinkageStatus::Valid);

        // Freehand 归属另一文档 → 拒（归属真相唯一）。
        let span = crate::anchor::GeometricSpan::new(
            crate::anchor::CoordSpace::PdfPage {
                width_pt: 595.0,
                height_pt: 842.0,
                origin: crate::anchor::PdfOrigin::BottomLeft,
            },
            Some(1),
            vec![crate::geometry::Quad::from_rect(0.0, 0.0, 10.0, 10.0)],
            None,
            "fnv64:g",
        )
        .unwrap();
        let freehand = OverlayMark::Freehand {
            document: doc("other"),
            span,
            note: None,
        };
        let err = OverlayEntry::new(
            AnnotationId::new("m2").unwrap(),
            doc("host"),
            freehand,
            1_000,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            EntryError::Mark(MarkError::DocumentMismatch { .. })
        ));
    }

    #[test]
    fn empty_comment_and_id_rejected() {
        let err = AnnotationId::new("  ").unwrap_err();
        assert_eq!(err, MarkError::EmptyAnnotationId);
        let c = OverlayMark::Comment {
            anchor: SemanticAnchor::new(
                Format::Pdf,
                PrimaryLocator::PdfSpan {
                    page: 1,
                    line: 0,
                    char_start: 0,
                    char_end: 1,
                },
                vec![],
                "fnv64:xx",
            )
            .unwrap(),
            text: " ".into(),
        };
        let err = OverlayEntry::new(AnnotationId::new("m3").unwrap(), doc("h"), c, 0).unwrap_err();
        assert!(matches!(err, EntryError::Mark(MarkError::EmptyComment)));
    }
}
