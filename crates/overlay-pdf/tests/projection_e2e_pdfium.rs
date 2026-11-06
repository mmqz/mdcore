//! 验收第 4/5 条的生产路径 e2e（#239——feature `engine-pdfium`，CI 面）。
//!
//! 与 `projection_lopdf.rs` 的分工：那边注入字符几何（本地无 pdfium
//! 也可全链验证 lopdf 写/读）；本文件走**真实 pdfium
//! `positioned_chars`**——`NativeAnnotationAdapter::plan` 的生产路径
//! （字符几何来自引擎，非测试构造）。
//!
//! 环境合同（与 pdfc_engine.rs / pdfn_table_extract.rs 同源）：pdfium
//! 库走 mdc-pdf 定位序（MDC_PDFIUM_LIB / 取件位 / 系统），**不静默
//! skip**——库缺失即失败（CI windows-test 的 mdc-pdf 步骤提供库）。
//!
//! 验证链路（issue #239 §十的合成夹具版——真实语料
//! songling-232762/TF-0002 为本地资产不入仓，链路形状逐点对齐）：
//!
//! ```text
//! 合成 PDF（generate：q31 字体 + 覆盖内文本）
//!   → pdfium positioned_chars（真实字符几何）
//!   → 在字符流中定位「逐项对比」→ PdfSpan 文本锚点
//!   → adapter.plan()（trait 生产路径——涉及页按需取几何）
//!   → QuadPoints（真实布局派生，页界内）
//!   → execute（lopdf 增量写 /Highlight）
//!   → read_annotations 读回（标注在场 + 坐标一致）
//! ```
#![cfg(feature = "engine-pdfium")]

use mdc_evidence::{DocId, DocumentRef, EngineKind, Fingerprinter, Fnv64, VersionStamp};
use mdc_overlay::anchor::PrimaryLocator;
use mdc_overlay::anchor::SemanticAnchor;
use mdc_overlay::contract::NativeAnnotationAdapter;
use mdc_overlay::geometry::CharBox;
use mdc_overlay::mark::{AnnotationId, OverlayEntry, OverlayMark};
use mdc_overlay::{Format, ProjectionTarget};
use mdc_overlay_pdf::PdfNativeAdapter;
use mdc_pdf::engine::DocumentEngine;

const DOC_TEXT: &str = "投标文件须逐项对比招标文件要求";
const TARGET: &str = "逐项对比";

fn font_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pdf/tests/fixtures/q31_test_font.ttf"
    );
    std::fs::read(path).expect("q31 子集字体夹具在读仓内")
}

fn source_pdf() -> Vec<u8> {
    let layout = mdc_pdf::generate::PageLayout {
        margin_pt: 72.0,
        font_size_pt: 12.0,
        leading_pt: 18.0,
    };
    let generated =
        mdc_pdf::generate::generate_text_pdf(&font_bytes(), DOC_TEXT, &layout).expect("合成 PDF");
    assert_eq!(generated.skipped_chars, 0);
    generated.bytes
}

#[test]
fn e2e_pdfium_plan_execute_readback() {
    let pdf = source_pdf();
    let doc = DocumentRef::new(
        DocId::new("tf-0002-e2e"),
        EngineKind::Pdf,
        VersionStamp(1),
        Fnv64.fingerprint(&pdf),
    );

    // ① 真实字符几何（pdfium——生产几何源）。
    let engine =
        mdc_pdf::engine_pdfium::PdfiumEngine::open(pdf.clone()).expect("pdfium 打开（定位序取库）");
    let chars = engine
        .positioned_chars(mdc_pdf::page::PageIndex(0))
        .expect("第 1 页字符流");
    assert!(!chars.is_empty(), "生成 PDF 必有字符");

    // ② 在字符流中定位目标子句 → PdfSpan（锚点创建路径 = 同一字符流）。
    let target: Vec<char> = TARGET.chars().collect();
    let mut found: Option<(usize, usize)> = None;
    for i in 0..chars.len().saturating_sub(target.len() - 1) {
        if (0..target.len()).all(|k| chars[i + k].ch == target[k]) {
            found = Some((i, i + target.len() - 1));
            break;
        }
    }
    let (cs, ce) = found.expect("字符流中可寻得目标子句");

    // ③ 文本锚点（PdfSpan）+ Emphasis 标注。
    let entry = OverlayEntry::new(
        AnnotationId::new("m-e2e").unwrap(),
        doc.clone(),
        OverlayMark::Emphasis {
            anchor: SemanticAnchor::new(
                Format::Pdf,
                PrimaryLocator::PdfSpan {
                    page: 1,
                    line: 1,
                    char_start: cs as u32,
                    char_end: ce as u32,
                },
                vec![],
                Fnv64.fingerprint(TARGET.as_bytes()).0,
            )
            .unwrap(),
            note: Some("关键条款（e2e）".into()),
        },
        1_000,
    )
    .unwrap();

    // ④ plan：trait 生产路径（几何 = pdfium positioned_chars）。
    let adapter = PdfNativeAdapter::new();
    let plan = adapter
        .plan(&doc, &pdf, &[entry])
        .expect("plan（pdfium 几何源）");
    assert!(plan.report.is_clean(), "计划干净：{:?}", plan.report);
    let ProjectionTarget::PdfHighlight { page, quads } = &plan.operations[0].target else {
        panic!("Emphasis → PdfHighlight");
    };
    assert_eq!(*page, 1);
    assert!(!quads.is_empty(), "真实布局派生 QuadPoints 非空");

    // 真实几何锚定：目标字符的包围盒并集（与注入几何不同——pdfium
    // 产真实 y/x，断言与 positioned_chars 的逐字盒一致）。
    let expect_x0 = chars[cs].left as f64;
    let expect_x1 = chars[ce].right as f64;
    let got_x0 = quads[0].points[0].x;
    let got_x1 = quads[0].points[1].x;
    assert!(
        (got_x0 - expect_x0).abs() < 0.5,
        "QuadPoint 左缘 ≈ 目标首字符左缘：{got_x0} vs {expect_x0}"
    );
    assert!(
        (got_x1 - expect_x1).abs() < 0.5,
        "QuadPoint 右缘 ≈ 目标末字符右缘：{got_x1} vs {expect_x1}"
    );
    for q in quads {
        for p in q.points {
            assert!(
                p.x >= 0.0 && p.x <= 595.0 && p.y >= 0.0 && p.y <= 842.0,
                "页界内：({x}, {y})",
                x = p.x,
                y = p.y
            );
        }
    }

    // ⑤ execute → 新文件字节（增量写）。
    let result = adapter.execute(&pdf, &plan).expect("execute");
    assert!(result.bytes.starts_with(&pdf), "增量写前缀保持");

    // ⑥ read_annotations 读回（/Highlight 在场 + QuadPoints 一致）。
    let (annots, report) = mdc_pdf::annotate::read_annotations(
        result.bytes.clone(),
        mdc_pdf::security::Limits::default(),
    )
    .expect("读回");
    assert_eq!(report.parsed, 1);
    let a = &annots[0];
    assert_eq!(a.page.0, 0);
    match &a.kind {
        mdc_pdf::annotation::AnnotationKind::Highlight { quads, .. } => {
            assert_eq!(quads.len(), quads.len(), "读回 QuadPoints 数一致");
            assert!(!quads.is_empty());
            assert!(
                (quads[0].points[0].x as f64 - got_x0).abs() < 0.5,
                "读回坐标与计划一致：{} vs {got_x0}",
                quads[0].points[0].x
            );
        }
        other => panic!("非 /Highlight：{other:?}"),
    }
}

/// plan 只取涉及页（大文档不整本扫描——几何源按需的形状面）。
#[test]
fn e2e_plan_only_fetches_wanted_pages() {
    let pdf = source_pdf();
    let doc = DocumentRef::new(
        DocId::new("tf-0002-e2e-2"),
        EngineKind::Pdf,
        VersionStamp(1),
        Fnv64.fingerprint(&pdf),
    );
    // 单页文档 + 第 1 页锚点 → plan 成功（单页面：按需 = 全部涉及）。
    let entry = OverlayEntry::new(
        AnnotationId::new("m-p1").unwrap(),
        doc.clone(),
        OverlayMark::Emphasis {
            anchor: SemanticAnchor::new(
                Format::Pdf,
                PrimaryLocator::PdfSpan {
                    page: 1,
                    line: 1,
                    char_start: 0,
                    char_end: 3,
                },
                vec![],
                "fnv64:s",
            )
            .unwrap(),
            note: None,
        },
        1_000,
    )
    .unwrap();
    let adapter = PdfNativeAdapter::new();
    let plan = adapter
        .plan(&doc, &pdf, std::slice::from_ref(&entry))
        .unwrap();
    assert_eq!(plan.report.exported.len(), 1);

    // 反例锚点（第 99 页）→ unsupported（页不在文档——引擎页界错误
    // 转显式报告，不 panic 不静默）。
    let mut bad = entry.clone();
    bad.id = AnnotationId::new("m-p99").unwrap();
    if let OverlayMark::Emphasis { anchor, .. } = &mut bad.mark {
        anchor.primary = PrimaryLocator::PdfSpan {
            page: 99,
            line: 1,
            char_start: 0,
            char_end: 3,
        };
    }
    let plan = adapter.plan(&doc, &pdf, &[bad]).unwrap();
    assert_eq!(plan.report.unsupported.len(), 1);
    assert!(plan.operations.is_empty());
}

/// CharBox 注入面与 pdfium 生产面的一致性（同 PDF 两路 plan 恒等
/// 的目标形状：几何源可替换——依赖倒置的实证）。
#[test]
fn e2e_geometry_source_is_injectable() {
    let pdf = source_pdf();
    let doc = DocumentRef::new(
        DocId::new("tf-0002-e2e-3"),
        EngineKind::Pdf,
        VersionStamp(1),
        Fnv64.fingerprint(&pdf),
    );
    let engine = mdc_pdf::engine_pdfium::PdfiumEngine::open(pdf.clone()).unwrap();
    let raw = engine
        .positioned_chars(mdc_pdf::page::PageIndex(0))
        .unwrap();
    let geom = engine.page_geometry(mdc_pdf::page::PageIndex(0)).unwrap();
    let injected = mdc_overlay_pdf::PageChars {
        page: 1,
        chars: raw
            .iter()
            .map(|c| CharBox {
                ch: c.ch,
                left: c.left as f64,
                bottom: c.bottom as f64,
                right: c.right as f64,
                top: c.top as f64,
                font_size: c.font_size as f64,
            })
            .collect(),
        page_box: mdc_overlay::PageBox {
            width_pt: geom.width_pt as f64,
            height_pt: geom.height_pt as f64,
        },
    };
    let entry = OverlayEntry::new(
        AnnotationId::new("m-inj").unwrap(),
        doc.clone(),
        OverlayMark::Emphasis {
            anchor: SemanticAnchor::new(
                Format::Pdf,
                PrimaryLocator::PdfSpan {
                    page: 1,
                    line: 1,
                    char_start: 0,
                    char_end: 5,
                },
                vec![],
                "fnv64:s",
            )
            .unwrap(),
            note: None,
        },
        1_000,
    )
    .unwrap();
    let adapter = PdfNativeAdapter::new();
    let via_trait = adapter
        .plan(&doc, &pdf, std::slice::from_ref(&entry))
        .unwrap();
    let via_inject = adapter
        .plan_with_geometry(&doc, &pdf, &[entry], &[injected])
        .unwrap();
    assert_eq!(
        via_trait.operations, via_inject.operations,
        "几何源注入与 pdfium 生产路径产出恒等（依赖倒置实证）"
    );
}
