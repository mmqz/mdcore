//! RenderHint：格式无关的**渲染意图**（issue #239 §五——不是渲染指令）。
//!
//! 真正的 Renderer 在视图层（mang-shell 各格式视图适配器）；本层产出
//! 的是「这条标注应该被怎样渲染」的语义描述：
//! - **Locator ≠ 屏幕位置**——`PdfSpan` 能表达文本锚点，但屏幕高亮
//!   需要页面坐标（视图适配器经字符布局换算）；
//! - 颜色/透明度/悬浮提示等视觉细节交给 UI 主题与具体视图——本层
//!   只到 [`OverlayStyle`]（语义样式）为止。

use mdc_evidence::LinkageStatus;
use serde::{Deserialize, Serialize};

use crate::anchor::SemanticAnchor;
use crate::mark::{AnnotationId, OverlayEntry, OverlayMark};

/// 语义样式——不是 RGB 颜色（视觉细节归 UI 主题）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayStyle {
    /// 强调（Emphasis 标注的常规态）。
    Emphasis,
    /// 评论标记（Comment 标注的常规态）。
    CommentMarker,
    /// 已失效（锚点失联——LinkageStatus 非 Valid 的降级显示）。
    Stale,
    /// 当前选中（UI 交互态——非持久化面，视图层临时叠加）。
    Selected,
}

/// 渲染语义（画成什么形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderSemantic {
    /// 文本高亮（Emphasis → 视图高亮）。
    HighlightText,
    /// 边缘标记（Comment → 页边/行边评论标记）。
    MarginMarker,
    /// 区域轮廓（Region/Freehand → 画布轮廓）。
    RegionOutline,
}

/// 渲染意图（视图消费面——每条标注一个 hint）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderHint {
    pub annotation_id: AnnotationId,
    pub anchor: SemanticAnchor,
    pub semantic: RenderSemantic,
    pub style: OverlayStyle,
    /// 关联状态（Valid = 常规渲染；Stale/Missing/Ambiguous → 视图
    /// 降级显示 + Stale 样式——**渲染层不静默隐藏失效标注**）。
    pub status: LinkageStatus,
}

impl RenderHint {
    /// 从存储单元派生（失效标注的 style 强制 [`OverlayStyle::Stale`]——
    /// 语义样式优先于变体常规态，视图一眼可辨）。
    pub fn from_entry(entry: &OverlayEntry) -> Option<Self> {
        let anchor = entry.mark.anchor()?.clone();
        let (semantic, base_style) = match &entry.mark {
            OverlayMark::Emphasis { .. } => (RenderSemantic::HighlightText, OverlayStyle::Emphasis),
            OverlayMark::Comment { .. } => {
                (RenderSemantic::MarginMarker, OverlayStyle::CommentMarker)
            }
            // 几何标注无语义锚点——视图适配面走 GeometricSpan（不是
            // 本 hint 的消费者；None = 诚实「无文本锚点可渲染」）。
            OverlayMark::Freehand { .. } | OverlayMark::Region { .. } => return None,
        };
        let style = if entry.linkage == LinkageStatus::Valid {
            base_style
        } else {
            OverlayStyle::Stale
        };
        Some(RenderHint {
            annotation_id: entry.id.clone(),
            anchor,
            semantic,
            style,
            status: entry.linkage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::PrimaryLocator;
    use crate::Format;
    use mdc_evidence::{ContentFingerprint, DocId, DocumentRef, EngineKind, VersionStamp};

    fn entry(linkage: LinkageStatus) -> OverlayEntry {
        let mut e = OverlayEntry::new(
            AnnotationId::new("m1").unwrap(),
            DocumentRef::new(
                DocId::new("d1"),
                EngineKind::Pdf,
                VersionStamp(1),
                ContentFingerprint("fnv64:aa".into()),
            ),
            OverlayMark::Emphasis {
                anchor: SemanticAnchor::new(
                    Format::Pdf,
                    PrimaryLocator::PdfSpan {
                        page: 1,
                        line: 2,
                        char_start: 5,
                        char_end: 9,
                    },
                    vec![],
                    "fnv64:xx",
                )
                .unwrap(),
                note: None,
            },
            1_000,
        )
        .unwrap();
        e.linkage = linkage;
        e
    }

    #[test]
    fn emphasis_entry_yields_highlight_hint() {
        let hint = RenderHint::from_entry(&entry(LinkageStatus::Valid)).unwrap();
        assert_eq!(hint.semantic, RenderSemantic::HighlightText);
        assert_eq!(hint.style, OverlayStyle::Emphasis);
        assert_eq!(hint.status, LinkageStatus::Valid);
        assert_eq!(hint.annotation_id.as_str(), "m1");
    }

    #[test]
    fn stale_entry_forces_stale_style() {
        let hint = RenderHint::from_entry(&entry(LinkageStatus::Stale)).unwrap();
        assert_eq!(hint.style, OverlayStyle::Stale, "失效标注强制 Stale 样式");
        assert_eq!(hint.status, LinkageStatus::Stale);
    }
}
