//! 验收第 2 条（#239）：标注与 Locator 双向索引往返一致。
//!
//! ```text
//! index(ann) → loc → annotations_at(loc) → 包含该 ann
//! loc → ann → loc 恒等
//! ```
//!
//! 同时演示「同一份 Overlay 数据服务 ①独立 Sidecar（本文件）与
//! ②视图渲染（render_hints）」；第 ③面（原生文件投影）在
//! `mdc-overlay-pdf` 的测试里从**同一份 entries** 出发——三面合起来
//! 才是 issue #239 的核心命题（PDF 垂直链路跑通后再谈其他格式）。

use mdc_evidence::{
    ContentFingerprint, DocId, DocumentRef, EngineKind, Fingerprinter, Fnv64, LinkageStatus,
    VersionStamp,
};
use mdc_overlay::anchor::{FallbackLocator, PrimaryLocator, SemanticAnchor};
use mdc_overlay::mark::{AnnotationId, OverlayEntry, OverlayMark};
use mdc_overlay::render::{OverlayStyle, RenderSemantic};
use mdc_overlay::{Format, OverlayIndex, OverlaySidecar, OverlayStore};

/// 真实形状的宿主文档：TF-0002 招标 PDF（指纹 = 对示例字节的 FNV）。
fn tender_doc() -> DocumentRef {
    let bytes = b"TF-0002 pdf bytes (fixture stand-in)";
    DocumentRef::new(
        DocId::new("tf-0002"),
        EngineKind::Pdf,
        VersionStamp(1),
        Fnv64.fingerprint(bytes),
    )
}

/// 「上行带宽 50M/s」式关键条款锚点（语义面：页 + 行 + 字符区间）。
fn span_anchor(cs: u32, ce: u32) -> SemanticAnchor {
    SemanticAnchor::new(
        Format::Pdf,
        PrimaryLocator::PdfSpan {
            page: 12,
            line: 34,
            char_start: cs,
            char_end: ce,
        },
        vec![FallbackLocator::TextQuery {
            needle: "投标文件须逐项对比招标文件要求".into(),
        }],
        Fnv64.fingerprint("关键条款指纹".as_bytes()).0,
    )
    .unwrap()
}

fn emphasis(id: &str, cs: u32, ce: u32) -> OverlayEntry {
    OverlayEntry::new(
        AnnotationId::new(id).unwrap(),
        tender_doc(),
        OverlayMark::Emphasis {
            anchor: span_anchor(cs, ce),
            note: Some("关键条款".into()),
        },
        1_000,
    )
    .unwrap()
}

#[test]
fn acceptance_2_bidirectional_index_roundtrip() {
    let mut idx = OverlayIndex::new();
    let e1 = emphasis("m1", 120, 124);
    let e2 = emphasis("m2", 480, 489);
    let original_anchor_1 = e1.mark.anchor().unwrap().clone();
    idx.insert(e1.clone()).unwrap();
    idx.insert(e2).unwrap();

    // index(ann) → loc：恒等回放。
    assert_eq!(idx.anchor_of(&e1.id), Some(&original_anchor_1));

    // loc → annotations_at(loc) → 包含该 ann。
    let found = idx.annotations_at(&tender_doc(), &original_anchor_1.primary);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, e1.id);

    // loc → ann → loc 恒等（第二条标注同样成立）。
    let anchor_2 = idx.anchor_of(&AnnotationId::new("m2").unwrap()).unwrap();
    let back = idx
        .annotations_at(&tender_doc(), &anchor_2.primary)
        .into_iter()
        .map(|e| e.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(back, vec![AnnotationId::new("m2").unwrap()]);
}

#[test]
fn same_data_serves_sidecar_and_view_render() {
    // ① 独立 Sidecar：同一份 entries 落盘/装载恒等。
    let entries = vec![emphasis("m1", 120, 124), emphasis("m2", 480, 489)];
    let mut sidecar = OverlaySidecar::new(tender_doc());
    for e in &entries {
        sidecar.upsert(e.clone()).unwrap();
    }
    let dir = std::env::temp_dir().join(format!("ovr-accept-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = OverlayStore::new(dir.join("tf-0002.overlay.json"));
    store.save(&sidecar).unwrap();
    let loaded = store.load().unwrap().unwrap();
    assert_eq!(loaded.entries(), entries.as_slice(), "sidecar 往返恒等");

    // ② 视图渲染：同一份 entries → RenderHint（渲染意图）。
    let idx = loaded.to_index().unwrap();
    let hints = idx.render_hints();
    assert_eq!(hints.len(), 2);
    for h in &hints {
        assert_eq!(h.semantic, RenderSemantic::HighlightText);
        assert_eq!(h.style, OverlayStyle::Emphasis);
        assert_eq!(h.status, LinkageStatus::Valid);
    }

    // 失效面：源变更验证后降级 → Stale 样式（渲染层不静默隐藏）。
    let mut sc2 = loaded.clone();
    let mut e = entries[0].clone();
    e.linkage = LinkageStatus::Stale;
    sc2.upsert(e).unwrap();
    let idx2 = sc2.to_index().unwrap();
    let stale_hint = idx2
        .render_hints()
        .into_iter()
        .find(|h| h.annotation_id.as_str() == "m1")
        .unwrap();
    assert_eq!(stale_hint.style, OverlayStyle::Stale);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn fingerprint_algorithm_prefix_contract() {
    // 指纹形态锚定（fnv64: + 16 hex——与 mdc-evidence 算法前缀纪律
    // 一致；锚点指纹的「二级失效判定」依赖此前缀可辨识）。
    let fp = Fnv64.fingerprint(b"abc");
    assert!(fp.0.starts_with("fnv64:"));
    assert_eq!(fp.0.len(), "fnv64:".len() + 16);
    let _ = ContentFingerprint(fp.0.clone());
}
