//! 验收第 4/5/6/7 条（#239）——lopdf 全链（本地可跑面）。
//!
//! 链路形状：**真实 PDF 字节**（`generate_text_pdf` 合成——q31 子集
//! 字体 + 覆盖内文本「投标文件须逐项对比招标文件要求」）+ **注入
//! 字符几何**（`PageChars`——pdfium `positioned_chars` 的模拟面；真实
//! 几何源的生产路径验证在 `projection_e2e_pdfium.rs`[CI，engine-pdfium
//! feature]——两层合起来 = 验收第 4/5 条的完整证明）。
//!
//! ```text
//! 文本锚点（PdfSpan）→ QuadPoints（plan_with_geometry）
//!   → /Highlight 写入（execute：lopdf 增量写）
//!   → 新文件字节 → read_annotations 读回（标注在场 + 坐标一致）
//! ```
#![cfg(feature = "engine-lopdf")]

use mdc_evidence::{DocId, DocumentRef, EngineKind, Fingerprinter, Fnv64, VersionStamp};
use mdc_overlay::anchor::{CoordSpace, GeometricSpan, PdfOrigin, PrimaryLocator, SemanticAnchor};
use mdc_overlay::contract::{NativeAnnotationAdapter, ProjectionTarget};
use mdc_overlay::geometry::{CharBox, PageBox, Quad};
use mdc_overlay::mark::{AnnotationId, OverlayEntry, OverlayMark};
use mdc_overlay::{Format, OverlayIndex};
use mdc_overlay_pdf::{PageChars, PdfNativeAdapter};

/// 测试文本（q31 子集字体覆盖内——字体 cmap 已验证全字覆盖）。
const DOC_TEXT: &str = "投标文件须逐项对比招标文件要求";
/// Emphasis 目标子句（DOC_TEXT 内唯一次出现，字符区间 [4, 7]）。
const TARGET: &str = "逐项对比";
const TARGET_START: u32 = 4;
const TARGET_END: u32 = 7;

fn font_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pdf/tests/fixtures/q31_test_font.ttf"
    );
    std::fs::read(path).expect("q31 子集字体夹具在读仓内")
}

/// 真实 PDF 字节（A4 · margin 72 · 12pt——与注入几何同参数系）。
fn source_pdf() -> Vec<u8> {
    let layout = mdc_pdf::generate::PageLayout {
        margin_pt: 72.0,
        font_size_pt: 12.0,
        leading_pt: 18.0,
    };
    let generated =
        mdc_pdf::generate::generate_text_pdf(&font_bytes(), DOC_TEXT, &layout).expect("合成 PDF");
    assert_eq!(generated.skipped_chars, 0, "测试文本必须全字覆盖（0 跳过）");
    assert_eq!(generated.pages, 1, "14 字单行 = 单页");
    generated.bytes
}

fn source_doc(bytes: &[u8]) -> DocumentRef {
    DocumentRef::new(
        DocId::new("tf-0002-synth"),
        EngineKind::Pdf,
        VersionStamp(1),
        Fnv64.fingerprint(bytes),
    )
}

/// 注入几何（pdfium positioned_chars 的模拟面：单行 14 字 @ y=700）。
fn page1_chars() -> PageChars {
    let chars: Vec<CharBox> = DOC_TEXT
        .chars()
        .enumerate()
        .map(|(i, c)| CharBox {
            ch: c,
            left: 72.0 + i as f64 * 12.0,
            bottom: 700.0,
            right: 72.0 + (i as f64 + 1.0) * 12.0,
            top: 712.0,
            font_size: 12.0,
        })
        .collect();
    PageChars {
        page: 1,
        chars,
        page_box: PageBox {
            width_pt: 595.0,
            height_pt: 842.0,
        },
    }
}

fn emphasis_entry(doc: &DocumentRef, cs: u32, ce: u32) -> OverlayEntry {
    OverlayEntry::new(
        AnnotationId::new("m-emph").unwrap(),
        doc.clone(),
        OverlayMark::Emphasis {
            anchor: SemanticAnchor::new(
                Format::Pdf,
                PrimaryLocator::PdfSpan {
                    page: 1,
                    line: 1,
                    char_start: cs,
                    char_end: ce,
                },
                vec![],
                Fnv64.fingerprint(TARGET.as_bytes()).0,
            )
            .unwrap(),
            note: None,
        },
        1_000,
    )
    .unwrap()
}

fn freehand_entry(doc: &DocumentRef) -> OverlayEntry {
    let span = GeometricSpan::new(
        CoordSpace::PdfPage {
            width_pt: 595.0,
            height_pt: 842.0,
            origin: PdfOrigin::BottomLeft,
        },
        Some(1),
        vec![Quad::from_rect(100.0, 100.0, 200.0, 130.0)],
        None,
        "fnv64:geom",
    )
    .unwrap();
    OverlayEntry::new(
        AnnotationId::new("m-free").unwrap(),
        doc.clone(),
        OverlayMark::Freehand {
            document: doc.clone(),
            span,
            note: None,
        },
        1_000,
    )
    .unwrap()
}

/// 验收第 4 条：PdfSpan → QuadPoints 非空且落在页面范围内。
#[test]
fn acceptance_4_text_anchor_to_in_page_quads() {
    let pdf = source_pdf();
    let doc = source_doc(&pdf);
    let adapter = PdfNativeAdapter::new();
    let entries = vec![emphasis_entry(&doc, TARGET_START, TARGET_END)];
    let plan = adapter
        .plan_with_geometry(&doc, &pdf, &entries, &[page1_chars()])
        .expect("plan");

    assert!(plan.report.is_clean(), "计划干净：{:?}", plan.report);
    assert_eq!(plan.report.exported.len(), 1);
    let ProjectionTarget::PdfHighlight { page, quads } = &plan.operations[0].target else {
        panic!("Emphasis 必须映射 PdfHighlight");
    };
    assert_eq!(*page, 1);
    assert!(!quads.is_empty(), "QuadPoints 非空");
    for q in quads {
        for p in q.points {
            assert!(
                p.x >= 0.0 && p.x <= 595.0 && p.y >= 0.0 && p.y <= 842.0,
                "QuadPoint 越页界：({x}, {y})",
                x = p.x,
                y = p.y
            );
        }
    }
    // 区间 → 几何数值锚定：[4,7] = x ∈ [120, 168]。
    assert_eq!(quads[0].points[0].x, 72.0 + TARGET_START as f64 * 12.0);
    assert_eq!(
        quads[0].points[1].x,
        72.0 + (TARGET_END as f64 + 1.0) * 12.0
    );
}

/// 验收第 5 条：导出 → read_annotations 读回 → 标注在场。
#[test]
fn acceptance_5_export_then_read_back() {
    let pdf = source_pdf();
    let doc = source_doc(&pdf);
    let adapter = PdfNativeAdapter::new();
    let entries = [emphasis_entry(&doc, TARGET_START, TARGET_END)];

    // 三面同源的第三面：从**索引**取同一份数据投影（①sidecar/②视图
    // 在 mdc-overlay 侧 acceptance_model.rs 演示）。
    let mut idx = OverlayIndex::new();
    idx.insert(entries[0].clone()).unwrap();
    let from_index: Vec<OverlayEntry> = idx.entries().cloned().collect();

    let plan = adapter
        .plan_with_geometry(&doc, &pdf, &from_index, &[page1_chars()])
        .unwrap();
    let result = adapter.execute(&pdf, &plan).expect("execute");

    // 产物是新文件（增量写：原字节为前缀 + 追加修订）。
    assert!(
        result.bytes.len() > pdf.len(),
        "产物 = 原字节前缀 + 追加（{} > {}）",
        result.bytes.len(),
        pdf.len()
    );
    assert!(
        result.bytes.starts_with(&pdf),
        "增量写前缀逐位保持（lopdf 增量语义）"
    );
    assert_eq!(result.report.exported.len(), 1);

    // 读回：read_annotations → /Highlight 在场 + QuadPoints 一致。
    let (annots, report) = mdc_pdf::annotate::read_annotations(
        result.bytes.clone(),
        mdc_pdf::security::Limits::default(),
    )
    .expect("读回");
    assert_eq!(report.parsed, 1, "解析 1 条标注（非跳非坏）");
    assert_eq!(annots.len(), 1);
    let a = &annots[0];
    assert_eq!(a.page.0, 0, "PageIndex 0 起 = PDF 第 1 页");
    match &a.kind {
        mdc_pdf::annotation::AnnotationKind::Highlight { quads, color } => {
            assert_eq!(quads.len(), 1, "读回 QuadPoints 数一致");
            // f64→f32 换算容差内锚定。
            assert!((quads[0].points[0].x - 120.0).abs() < 0.01);
            assert!((quads[0].points[1].x - 168.0).abs() < 0.01);
            assert_eq!(*color, mdc_overlay_pdf::EMPHASIS_COLOR);
        }
        other => panic!("非 /Highlight：{other:?}"),
    }
    // /T 溯源（投影作者）。
    assert_eq!(a.author, "mdcore-overlay");
}

/// 验收第 6 条：修改源文件后，旧 ProjectionPlan 被拒绝执行。
#[test]
fn acceptance_6_stale_source_plan_rejected() {
    let pdf = source_pdf();
    let doc = source_doc(&pdf);
    let adapter = PdfNativeAdapter::new();
    let entries = vec![emphasis_entry(&doc, TARGET_START, TARGET_END)];
    let plan = adapter
        .plan_with_geometry(&doc, &pdf, &entries, &[page1_chars()])
        .unwrap();

    // plan 之后源文件被修改（哪怕一个字节）。
    let mut modified = pdf.clone();
    modified.push(b'%');
    let err = adapter.execute(&modified, &plan).unwrap_err();
    match &err {
        mdc_overlay::AdapterError::StaleSource { expected, found } => {
            assert_eq!(expected, &plan.source_fingerprint);
            assert_ne!(expected, found);
        }
        other => panic!("非 StaleSource：{other:?}"),
    }
    assert!(err.to_string().contains("重新 plan"), "错误可操作：{err}");
}

/// 验收第 7 条：不能映射的标注有明确报告，不静默丢弃。
#[test]
fn acceptance_7_unsupported_mark_reported() {
    let pdf = source_pdf();
    let doc = source_doc(&pdf);
    let adapter = PdfNativeAdapter::new();
    let entries = vec![
        emphasis_entry(&doc, TARGET_START, TARGET_END),
        freehand_entry(&doc),
    ];
    let plan = adapter
        .plan_with_geometry(&doc, &pdf, &entries, &[page1_chars()])
        .unwrap();

    // Freehand 显式 unsupported：id + 人读原因。
    assert_eq!(plan.report.unsupported.len(), 1);
    let (id, reason) = &plan.report.unsupported[0];
    assert_eq!(id.as_str(), "m-free");
    assert!(reason.contains("Freehand"), "原因可诊断：{reason}");
    // Emphasis 照常导出（一坏不拖全批）。
    assert_eq!(plan.report.exported.len(), 1);
    assert_eq!(plan.report.exported[0].as_str(), "m-emph");

    // 执行面：只写 exported（unsupported 不进文件）。
    let result = adapter.execute(&pdf, &plan).unwrap();
    let (annots, _) = mdc_pdf::annotate::read_annotations(
        result.bytes.clone(),
        mdc_pdf::security::Limits::default(),
    )
    .unwrap();
    assert_eq!(annots.len(), 1, "仅 Emphasis 落 /Highlight");
}

/// 空计划拒绝执行（调用方应先看 report——不产生空产物）。
#[test]
fn empty_plan_execute_rejected() {
    let pdf = source_pdf();
    let doc = source_doc(&pdf);
    let adapter = PdfNativeAdapter::new();
    // 只给 Freehand → 全 unsupported → operations 空。
    let plan = adapter
        .plan_with_geometry(&doc, &pdf, &[freehand_entry(&doc)], &[page1_chars()])
        .unwrap();
    assert!(plan.operations.is_empty());
    assert!(!plan.report.is_clean());
    let err = adapter.execute(&pdf, &plan).unwrap_err();
    assert!(matches!(err, mdc_overlay::AdapterError::EmptyPlan));
}
