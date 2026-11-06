//! 验收第 3 条（#239）：Overlay 输出 RenderHint，PDF 视图通过独立
//! 适配器消费。
//!
//! 测试：给定 3 个 Emphasis → 3 个 RenderHint →
//! `PdfOverlayViewAdapter` 能转成视图坐标（页坐标四边形）。
//!
//! 「独立适配器」：本 crate（mdc-overlay-pdf）与 overlay 契约层的
//! 唯一交集是 `RenderHint`——视图层（mang-shell）将来消费的也是这个
//! 面（Locator → 页坐标的换算不进 overlay，也不进渲染器）。

use mdc_evidence::{DocId, DocumentRef, EngineKind, VersionStamp};
use mdc_overlay::anchor::{PrimaryLocator, SemanticAnchor};
use mdc_overlay::geometry::{CharBox, PageBox};
use mdc_overlay::mark::{AnnotationId, OverlayEntry, OverlayMark};
use mdc_overlay::render::RenderSemantic;
use mdc_overlay::{Format, OverlayIndex};
use mdc_overlay_pdf::{PageChars, PdfOverlayViewAdapter};

fn doc() -> DocumentRef {
    DocumentRef::new(
        DocId::new("tf-0002"),
        EngineKind::Pdf,
        VersionStamp(1),
        mdc_evidence::ContentFingerprint("fnv64:doc".into()),
    )
}

/// 第 1 页字符流（14 字一行——generate 系 A4 布局的注入模拟；
/// 真实几何源 = pdfium positioned_chars，见 projection_e2e_pdfium.rs）。
fn page1() -> PageChars {
    let text = "投标文件须逐项对比招标文件要求";
    let chars: Vec<CharBox> = text
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

fn emphasis(id: &str, cs: u32, ce: u32) -> OverlayEntry {
    OverlayEntry::new(
        AnnotationId::new(id).unwrap(),
        doc(),
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
fn acceptance_3_three_emphasis_to_view_coordinates() {
    // 3 个 Emphasis（三处关键条款区间）。
    let entries = vec![
        emphasis("m1", 0, 3),  // 「投标文件」
        emphasis("m2", 4, 7),  // 「须逐项对比」→ 目标子句
        emphasis("m3", 8, 13), // 「招标文件要求」
    ];
    let mut idx = OverlayIndex::new();
    for e in &entries {
        idx.insert(e.clone()).unwrap();
    }

    // → 3 个 RenderHint（overlay 出口——视图消费面）。
    let hints = idx.render_hints();
    assert_eq!(hints.len(), 3, "3 个 Emphasis → 3 个 RenderHint");
    assert!(hints
        .iter()
        .all(|h| h.semantic == RenderSemantic::HighlightText));

    // → PdfOverlayViewAdapter 转成视图坐标（页坐标四边形）。
    let adapter = PdfOverlayViewAdapter::new();
    let page = page1();
    for h in &hints {
        let quads = adapter.highlight_quads(h, &page).expect("视图坐标换算");
        assert!(!quads.is_empty(), "视图四边形非空");
        let q = quads[0];
        // 页坐标（不是屏幕像素——屏幕换算归 mang-shell 视图层）。
        assert!(q.points[0].x >= 0.0 && q.points[1].x <= 595.0);
        assert!(q.points[0].y >= 0.0 && q.points[2].y <= 842.0);
    }

    // 区间 → 几何的数值锚定（第 2 条标注 = 字符 [4,7]）。
    let h2 = hints
        .iter()
        .find(|h| h.annotation_id.as_str() == "m2")
        .unwrap();
    let quads = adapter.highlight_quads(h2, &page).unwrap();
    assert_eq!(quads.len(), 1, "单行区间 = 单视图四边形");
    assert_eq!(quads[0].points[0].x, 72.0 + 4.0 * 12.0, "左缘 = 第 5 字符");
    assert_eq!(
        quads[0].points[1].x,
        72.0 + 8.0 * 12.0,
        "右缘 = 第 8 字符右"
    );
}

#[test]
fn view_adapter_rejects_foreign_page_layout() {
    // 适配器纪律：hint 页与布局页不符 = 显式错误（不是空结果）。
    let entries = vec![emphasis("m1", 0, 3)];
    let idx = OverlayIndex::from_entries(&entries).unwrap();
    let hint = &idx.render_hints()[0];
    let mut wrong_page = page1();
    wrong_page.page = 2;
    let err = PdfOverlayViewAdapter::new()
        .highlight_quads(hint, &wrong_page)
        .unwrap_err();
    assert!(
        err.to_string().contains("第 1 页"),
        "错误含 hint 页号：{err}"
    );
}
