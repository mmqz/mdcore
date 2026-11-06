//! Sidecar 存储 v1（issue #239 Phase 1/2——单一真源 + 原子写）。
//!
//! 复用 mdc-evidence 的 SidecarStore 纪律（issue 第七节用户裁定：
//! **不复制一套平行实现**——但 evidence 的 `Sidecar` 是证据域容器
//! [evidence/calcs/audit 载荷]，overlay 的标注载荷形状不同，故本层
//! 有自己的容器类型，而**原子写/模式版本校验/幂等装载的存储纪律**
//! 与 `mdc_evidence::SidecarStore` 逐条对齐：同目录临时文件 + rename
//! 的原子保存、schema_version 不匹配显式拒绝、损坏文件 typed 错误）。

use std::fs;
use std::path::{Path, PathBuf};

use mdc_evidence::DocumentRef;
use serde::{Deserialize, Serialize};

use crate::index::OverlayIndex;
use crate::mark::{AnnotationId, OverlayEntry};

/// Sidecar 模式版本（破坏性变更 → 递增 + 迁移函数——issue #239 纪
/// 律 5）。
pub const OVERLAY_SCHEMA_VERSION: u32 = 1;

/// 标注 sidecar（JSON 单文件——一个宿主文档一个 sidecar；v0 纪律：
/// 全部 entry 归属宿主文档）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlaySidecar {
    pub schema_version: u32,
    /// 宿主文档身份（entry.document 必须与之一致——upsert 校验）。
    pub document: DocumentRef,
    pub entries: Vec<OverlayEntry>,
}

impl OverlaySidecar {
    pub fn new(document: DocumentRef) -> Self {
        OverlaySidecar {
            schema_version: OVERLAY_SCHEMA_VERSION,
            document,
            entries: Vec::new(),
        }
    }

    /// 登记/替换（同 id = 替换——编辑语义；归属不一致/无效单元拒）。
    pub fn upsert(&mut self, entry: OverlayEntry) -> Result<(), OverlayStoreError> {
        if entry.document != self.document {
            return Err(OverlayStoreError::DocumentMismatch {
                sidecar: self.document.doc_id.as_str().to_string(),
                entry: entry.document.doc_id.as_str().to_string(),
            });
        }
        // 全量不变量校验（新构造与索引同一防线）。
        crate::mark::OverlayEntry::new(
            entry.id.clone(),
            entry.document.clone(),
            entry.mark.clone(),
            entry.created_at_ms,
        )
        .map_err(OverlayStoreError::InvalidEntry)?;
        match self.entries.iter().position(|e| e.id == entry.id) {
            Some(i) => self.entries[i] = entry,
            None => self.entries.push(entry),
        }
        Ok(())
    }

    /// 移除（None = id 不在场）。
    pub fn remove(&mut self, id: &AnnotationId) -> Option<OverlayEntry> {
        self.entries
            .iter()
            .position(|e| e.id == *id)
            .map(|i| self.entries.remove(i))
    }

    pub fn entries(&self) -> &[OverlayEntry] {
        &self.entries
    }

    /// 重建内存索引（装载/落盘后的统一消费面——「同一份数据同时
    /// 服务 sidecar/视图/投影」的出口）。
    pub fn to_index(&self) -> Result<OverlayIndex, OverlayStoreError> {
        OverlayIndex::from_entries(&self.entries).map_err(OverlayStoreError::Index)
    }
}

/// 存储错误（typed——损坏不静默）。
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayStoreError {
    Io(String),
    Serialize(String),
    /// JSON 可解析但结构破坏（serde 分类 + 行号）。
    Corrupt(String),
    /// schema 版本不支持（显式拒绝——升级路径在档）。
    SchemaUnsupported {
        found: u32,
        supported: u32,
    },
    /// 归属不一致（sidecar 单文档纪律）。
    DocumentMismatch {
        sidecar: String,
        entry: String,
    },
    InvalidEntry(crate::mark::EntryError),
    Index(crate::index::IndexError),
}

impl std::fmt::Display for OverlayStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayStoreError::Io(e) => write!(f, "IO：{e}"),
            OverlayStoreError::Serialize(e) => write!(f, "序列化：{e}"),
            OverlayStoreError::Corrupt(e) => write!(f, "sidecar 损坏：{e}"),
            OverlayStoreError::SchemaUnsupported { found, supported } => write!(
                f,
                "schema 版本 {found} 不受支持（当前 {supported}——需要升级/迁移）"
            ),
            OverlayStoreError::DocumentMismatch { sidecar, entry } => {
                write!(f, "entry 归属文档 {entry} 与 sidecar 宿主 {sidecar} 不一致")
            }
            OverlayStoreError::InvalidEntry(e) => write!(f, "无效单元：{e}"),
            OverlayStoreError::Index(e) => write!(f, "索引重建：{e}"),
        }
    }
}
impl std::error::Error for OverlayStoreError {}

impl From<std::io::Error> for OverlayStoreError {
    fn from(e: std::io::Error) -> Self {
        OverlayStoreError::Io(e.to_string())
    }
}
impl From<serde_json::Error> for OverlayStoreError {
    fn from(e: serde_json::Error) -> Self {
        OverlayStoreError::Serialize(e.to_string())
    }
}

/// 文件存储（一个 sidecar 一个文件；路径由调用方管理——与
/// `mdc_evidence::SidecarStore` 同纪律：路径决策[允许列表等]在宿主
/// 层，本层不做路径决策）。
#[derive(Debug, Clone)]
pub struct OverlayStore {
    path: PathBuf,
}

impl OverlayStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        OverlayStore {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 装载（不存在 → None[新文档首次标注]；存在 → 版本校验 + 结构
    /// 校验 + entry 全量不变量校验）。
    pub fn load(&self) -> Result<Option<OverlaySidecar>, OverlayStoreError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.path)?;
        let parsed: OverlaySidecar = serde_json::from_str(&raw).map_err(|e| {
            OverlayStoreError::Corrupt(format!("{:?} at line {}", e.classify(), e.line()))
        })?;
        if parsed.schema_version != OVERLAY_SCHEMA_VERSION {
            return Err(OverlayStoreError::SchemaUnsupported {
                found: parsed.schema_version,
                supported: OVERLAY_SCHEMA_VERSION,
            });
        }
        // entry 不变量在装载面再验一次（防手改 sidecar 文件绕过
        // upsert 校验——「装载即信任」是静默重绑的入口）。
        for e in &parsed.entries {
            if e.document != parsed.document {
                return Err(OverlayStoreError::DocumentMismatch {
                    sidecar: parsed.document.doc_id.as_str().to_string(),
                    entry: e.document.doc_id.as_str().to_string(),
                });
            }
            crate::mark::OverlayEntry::new(
                e.id.clone(),
                e.document.clone(),
                e.mark.clone(),
                e.created_at_ms,
            )
            .map_err(OverlayStoreError::InvalidEntry)?;
        }
        Ok(Some(parsed))
    }

    /// 原子保存（同目录临时文件 + rename——与 mdc-evidence
    /// SidecarStore::save 同机制：同文件系统保证原子性，写失败时
    /// 原文件不动）。
    pub fn save(&self, sidecar: &OverlaySidecar) -> Result<(), OverlayStoreError> {
        let json = serde_json::to_string_pretty(sidecar)?;
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, json.as_bytes())?;
        // 同目录 rename（POSIX 原子）。
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::PrimaryLocator;
    use crate::mark::OverlayMark;
    use crate::Format;
    use mdc_evidence::{ContentFingerprint, DocId, EngineKind, VersionStamp};

    fn doc() -> DocumentRef {
        DocumentRef::new(
            DocId::new("tf-0002"),
            EngineKind::Pdf,
            VersionStamp(1),
            ContentFingerprint("fnv64:aa".into()),
        )
    }

    fn entry(id: &str) -> OverlayEntry {
        OverlayEntry::new(
            AnnotationId::new(id).unwrap(),
            doc(),
            OverlayMark::Emphasis {
                anchor: SemanticAnchor::new(
                    Format::Pdf,
                    PrimaryLocator::PdfSpan {
                        page: 1,
                        line: 1,
                        char_start: 3,
                        char_end: 7,
                    },
                    vec![],
                    "fnv64:seg",
                )
                .unwrap(),
                note: Some("关键条款".into()),
            },
            1_000,
        )
        .unwrap()
    }

    use crate::anchor::SemanticAnchor;

    #[test]
    fn sidecar_roundtrip_and_index_rebuild() {
        let mut sc = OverlaySidecar::new(doc());
        sc.upsert(entry("m1")).unwrap();
        sc.upsert(entry("m2")).unwrap();

        let dir = std::env::temp_dir().join(format!("mdc-overlay-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tf-0002.overlay.json");
        let store = OverlayStore::new(&path);

        store.save(&sc).unwrap();
        let loaded = store.load().unwrap().expect("落盘后可装载");
        assert_eq!(loaded, sc, "sidecar 往返恒等");
        assert_eq!(loaded.schema_version, OVERLAY_SCHEMA_VERSION);

        // 重建索引（同一份数据服务三面——sidecar/视图/投影的统一出口）。
        let idx = loaded.to_index().unwrap();
        assert_eq!(idx.entries().count(), 2);
        assert_eq!(idx.render_hints().len(), 2);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_loads_none() {
        let dir = std::env::temp_dir().join(format!("mdc-overlay-none-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = OverlayStore::new(dir.join("absent.overlay.json"));
        assert!(store.load().unwrap().is_none(), "不存在 → None（新文档）");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_version_mismatch_rejected() {
        let dir = std::env::temp_dir().join(format!("mdc-overlay-ver-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v2.overlay.json");
        fs::write(
            &path,
            r#"{"schema_version": 99, "document": {"doc_id": "d", "engine": "pdf", "version": 0, "fingerprint": "fnv64:a"}, "entries": []}"#,
        )
        .unwrap();
        let err = OverlayStore::new(&path).load().unwrap_err();
        assert_eq!(
            err,
            OverlayStoreError::SchemaUnsupported {
                found: 99,
                supported: OVERLAY_SCHEMA_VERSION
            }
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn foreign_document_entry_rejected() {
        let mut sc = OverlaySidecar::new(doc());
        let mut e = entry("m9");
        e.document = DocumentRef::new(
            DocId::new("foreign"),
            EngineKind::Pdf,
            VersionStamp(1),
            ContentFingerprint("fnv64:bb".into()),
        );
        assert!(matches!(
            sc.upsert(e),
            Err(OverlayStoreError::DocumentMismatch { .. })
        ));
    }

    #[test]
    fn hand_edited_sidecar_fails_invariants_on_load() {
        let dir = std::env::temp_dir().join(format!("mdc-overlay-hand-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hand.overlay.json");
        // 手改文件：entry 归属与宿主不一致（绕过 upsert 的防线必须
        // 在装载面被抓）。
        let hand_edited = r#"{
            "schema_version": 1,
            "document": {"doc_id": "host", "engine": "pdf", "version": 0, "fingerprint": "fnv64:a"},
            "entries": [
                {
                    "id": "m1",
                    "document": {"doc_id": "other", "engine": "pdf", "version": 0, "fingerprint": "fnv64:b"},
                    "mark": {
                        "kind": "emphasis",
                        "anchor": {
                            "format": "pdf",
                            "primary": {"kind": "pdf_span", "page": 1, "line": 1, "char_start": 0, "char_end": 3},
                            "fingerprint": "fnv64:s"
                        }
                    },
                    "linkage": "valid",
                    "created_at_ms": 0
                }
            ]
        }"#;
        fs::write(&path, hand_edited).unwrap();
        let err = OverlayStore::new(&path).load().unwrap_err();
        assert!(
            matches!(err, OverlayStoreError::DocumentMismatch { .. }),
            "实际错误：{err:?}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
