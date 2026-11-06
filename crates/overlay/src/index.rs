//! 双向索引（issue #239 Phase 2）。
//!
//! 验收契约（#239 验收第 2 条）：
//! ```text
//! index(entry) → loc                                    （正向）
//!   → annotations_at(loc) 包含该 entry                   （反向）
//!   → anchor_of(id) == 原 anchor                          （恒等回放）
//! ```
//!
//! 键形态 [`IndexKey`] = 文档身份 + 主定位器规范键（同锚点恒同键；
//! 含文档身份是因为「跨文档索引」是本层的产品语义——不同文档的
//! 同形定位器是两个不同的索引点）。

use std::collections::{BTreeMap, BTreeSet};

use mdc_evidence::{DocumentRef, LinkageStatus};

use crate::anchor::{PrimaryLocator, SemanticAnchor};
use crate::mark::{AnnotationId, OverlayEntry};
use crate::render::RenderHint;

/// 索引键（文档身份 + 定位器规范键）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndexKey {
    /// 归属文档逻辑 ID（DocumentRef::doc_id 的字符串面——版本/指纹
    /// 不入键：同一文档演进中锚点索引点不变）。
    pub doc_id: String,
    /// 主定位器规范键（`PrimaryLocator::key`）。
    pub locator: String,
}

impl IndexKey {
    pub fn new(doc_id: &str, locator: &PrimaryLocator) -> Self {
        IndexKey {
            doc_id: doc_id.to_string(),
            locator: locator.key(),
        }
    }
}

/// 双向索引（内存态——持久化由 [`crate::store::OverlaySidecar`] 承载；
/// 索引可从任意 entries 集重建[`OverlayIndex::from_entries`]）。
#[derive(Debug, Default, Clone)]
pub struct OverlayIndex {
    /// 主定位键 → 标注 id 集（反向查询面）。
    by_locator: BTreeMap<IndexKey, BTreeSet<String>>,
    /// 标注 id → 存储单元（正向查询面 + 恒等回放面）。
    entries: BTreeMap<String, OverlayEntry>,
}

/// 索引操作错误。
#[derive(Debug, Clone, PartialEq)]
pub enum IndexError {
    /// id 重复（索引内身份唯一——重复插入按错误处理，调用方决定
    /// 是否先 remove 再 insert）。
    DuplicateId(String),
    /// entry 构造不变量破坏（透传[`crate::mark::EntryError`]）。
    InvalidEntry(crate::mark::EntryError),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexError::DuplicateId(id) => write!(f, "标注 id 重复：{id}"),
            IndexError::InvalidEntry(e) => write!(f, "无效存储单元：{e}"),
        }
    }
}
impl std::error::Error for IndexError {}

impl OverlayIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从存储单元集重建（sidecar 装载后一键重建索引；重复 id =
    /// 存储破坏——错误显式冒泡，不静默跳过）。
    pub fn from_entries(entries: &[OverlayEntry]) -> Result<Self, IndexError> {
        let mut idx = OverlayIndex::new();
        for e in entries {
            idx.insert(e.clone())?;
        }
        Ok(idx)
    }

    /// 正向：登记标注（语义锚点入定位键索引；几何标注仅入 id 索引
    /// ——其定位查询面是 GeometricSpan，不是语义锚点）。
    pub fn insert(&mut self, entry: OverlayEntry) -> Result<(), IndexError> {
        // 全量校验（sidecar 反序列化产物与新构造同一防线）。
        if let Err(e) = crate::mark::OverlayEntry::new(
            entry.id.clone(),
            entry.document.clone(),
            entry.mark.clone(),
            entry.created_at_ms,
        ) {
            return Err(IndexError::InvalidEntry(e));
        }
        if self.entries.contains_key(entry.id.as_str()) {
            return Err(IndexError::DuplicateId(entry.id.0));
        }
        if let Some(anchor) = entry.mark.anchor() {
            let key = IndexKey::new(entry.document.doc_id.as_str(), &anchor.primary);
            self.by_locator
                .entry(key)
                .or_default()
                .insert(entry.id.0.clone());
        }
        self.entries.insert(entry.id.0.clone(), entry);
        Ok(())
    }

    /// 反向：该定位器上的全部标注（同文档同锚点可多条——如同一句
    /// 既有 Emphasis 又有 Comment）。
    pub fn annotations_at(
        &self,
        doc: &DocumentRef,
        locator: &PrimaryLocator,
    ) -> Vec<&OverlayEntry> {
        let key = IndexKey::new(doc.doc_id.as_str(), locator);
        self.by_locator
            .get(&key)
            .map(|ids| ids.iter().filter_map(|id| self.entries.get(id)).collect())
            .unwrap_or_default()
    }

    /// 恒等回放：id → 语义锚点（无锚点的几何标注 = None）。
    pub fn anchor_of(&self, id: &AnnotationId) -> Option<&SemanticAnchor> {
        self.entries.get(id.as_str()).and_then(|e| e.mark.anchor())
    }

    /// id → 存储单元。
    pub fn entry(&self, id: &AnnotationId) -> Option<&OverlayEntry> {
        self.entries.get(id.as_str())
    }

    /// 移除（双向同步摘除）。
    pub fn remove(&mut self, id: &AnnotationId) -> Option<OverlayEntry> {
        let entry = self.entries.remove(id.as_str())?;
        if let Some(anchor) = entry.mark.anchor() {
            let key = IndexKey::new(entry.document.doc_id.as_str(), &anchor.primary);
            if let Some(ids) = self.by_locator.get_mut(&key) {
                ids.remove(id.as_str());
                if ids.is_empty() {
                    self.by_locator.remove(&key);
                }
            }
        }
        Some(entry)
    }

    /// 全部存储单元（id 序）。
    pub fn entries(&self) -> impl Iterator<Item = &OverlayEntry> {
        self.entries.values()
    }

    /// 全部渲染意图（语义标注面——几何标注不产 hint，见
    /// [`RenderHint::from_entry`]；失效标注含 Stale 样式，不隐藏）。
    pub fn render_hints(&self) -> Vec<RenderHint> {
        self.entries
            .values()
            .filter_map(RenderHint::from_entry)
            .collect()
    }

    /// 关联状态批量更新（验证方对源重验后降级/恢复——**只有显式
    /// 调用才变更**，索引自身不猜）。
    pub fn set_linkage(&mut self, id: &AnnotationId, status: LinkageStatus) -> bool {
        if let Some(e) = self.entries.get_mut(id.as_str()) {
            e.linkage = status;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::FallbackLocator;
    use crate::mark::OverlayMark;
    use crate::Format;
    use mdc_evidence::{ContentFingerprint, DocId, EngineKind, VersionStamp};

    fn doc() -> DocumentRef {
        DocumentRef::new(
            DocId::new("bid-tf0002"),
            EngineKind::Pdf,
            VersionStamp(3),
            ContentFingerprint("fnv64:aa".into()),
        )
    }

    fn emphasis(id: &str, cs: u32, ce: u32) -> OverlayEntry {
        OverlayEntry::new(
            AnnotationId::new(id).unwrap(),
            doc(),
            OverlayMark::Emphasis {
                anchor: SemanticAnchor::new(
                    Format::Pdf,
                    PrimaryLocator::PdfSpan {
                        page: 2,
                        line: 5,
                        char_start: cs,
                        char_end: ce,
                    },
                    vec![FallbackLocator::TextQuery {
                        needle: "上行带宽".into(),
                    }],
                    "fnv64:seg",
                )
                .unwrap(),
                note: None,
            },
            1_000,
        )
        .unwrap()
    }

    #[test]
    fn bidirectional_roundtrip_identity() {
        // 验收第 2 条：index(ann) → loc → annotations_at(loc) →
        // 包含该 ann；loc → ann → loc 恒等。
        let mut idx = OverlayIndex::new();
        let e = emphasis("m1", 100, 104);
        let anchor = e.mark.anchor().unwrap().clone();
        idx.insert(e.clone()).unwrap();

        // 反向：loc → 包含该 ann。
        let found = idx.annotations_at(&doc(), &anchor.primary);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, e.id);

        // 恒等：ann → loc == 原锚点。
        assert_eq!(idx.anchor_of(&e.id), Some(&anchor));

        // 语义标注检索按锚点命中（不同区间不命中）。
        let other = PrimaryLocator::PdfSpan {
            page: 2,
            line: 5,
            char_start: 200,
            char_end: 204,
        };
        assert!(idx.annotations_at(&doc(), &other).is_empty());
    }

    #[test]
    fn same_anchor_multiple_marks_and_removal() {
        let mut idx = OverlayIndex::new();
        idx.insert(emphasis("m1", 42, 46)).unwrap();
        let mut c = emphasis("m2", 42, 46);
        c.mark = OverlayMark::Comment {
            anchor: c.mark.anchor().unwrap().clone(),
            text: "核对带宽单位".into(),
        };
        idx.insert(c).unwrap();
        // 同锚点两条（Emphasis + Comment）。
        let loc = PrimaryLocator::PdfSpan {
            page: 2,
            line: 5,
            char_start: 42,
            char_end: 46,
        };
        assert_eq!(idx.annotations_at(&doc(), &loc).len(), 2);

        // 移除一条：另一条仍在。
        idx.remove(&AnnotationId::new("m1").unwrap());
        assert_eq!(idx.annotations_at(&doc(), &loc).len(), 1);

        // 移除最后一条：索引点消失。
        idx.remove(&AnnotationId::new("m2").unwrap());
        assert!(idx.annotations_at(&doc(), &loc).is_empty());
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut idx = OverlayIndex::new();
        idx.insert(emphasis("m1", 0, 3)).unwrap();
        let err = idx.insert(emphasis("m1", 10, 13)).unwrap_err();
        assert_eq!(err, IndexError::DuplicateId("m1".into()));
    }

    #[test]
    fn cross_document_same_locator_are_distinct_keys() {
        // 跨文档索引：不同文档同形定位器 = 不同索引点。
        let mut idx = OverlayIndex::new();
        idx.insert(emphasis("m1", 0, 3)).unwrap();
        let mut other_doc_entry = emphasis("m2", 0, 3);
        other_doc_entry.document = DocumentRef::new(
            DocId::new("another-doc"),
            EngineKind::Pdf,
            VersionStamp(1),
            ContentFingerprint("fnv64:bb".into()),
        );
        idx.insert(other_doc_entry).unwrap();

        let loc = PrimaryLocator::PdfSpan {
            page: 2,
            line: 5,
            char_start: 0,
            char_end: 3,
        };
        assert_eq!(idx.annotations_at(&doc(), &loc).len(), 1, "宿主文档命中 1");
        let other_doc = DocumentRef::new(
            DocId::new("another-doc"),
            EngineKind::Pdf,
            VersionStamp(1),
            ContentFingerprint("fnv64:bb".into()),
        );
        assert_eq!(
            idx.annotations_at(&other_doc, &loc).len(),
            1,
            "另一文档命中 1（跨文档键分离）"
        );
    }
}
