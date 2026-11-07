//! mdc-pdf-wasm 纯内核测试（JsValue-free——原生 cargo test 面）。
//!
//! 覆盖：打开扫描 → 会话构造（注入路径）→ 文本注入 → 六命令 JSON
//! 全链（ExtractText/Search/AnnotateAdd/AnnotateList）→ lopdf 写器
//! 增量 flush（前缀逐位保持 + 零脏幂等）→ 产物字节取回。
//! 夹具 = 内联最小 PDF 1.4 构造器（与 web/tests/gen_pdf_fixture.py
//! 同构——经典 10 对象手写路径）。

use mdc_pdf::annotation::{AnnotationDraft, AnnotationKind, Origin, Point, Quad, Rgb};
use mdc_pdf::command::Source;
use mdc_pdf::document::PdfSession;
use mdc_pdf::document::{DocumentMeta, PdfDocumentId};
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageGeometry, Rotation};
use mdc_pdf::security::OpenAssessment;
use mdc_pdf::writer::IncrementalSave;
use mdc_pdf_wasm::{
    annotate_add_cmd, annotate_list_cmd, execute_core, extract_text_cmd, flush_stats_json,
    open_scan_core, search_cmd, session_new_core,
};

// ---------------------------------------------------------------------------
// 最小 PDF 夹具（10 对象——与 web/tests/gen_pdf_fixture.py 同构）
// ---------------------------------------------------------------------------

fn fixture_pdf() -> Vec<u8> {
    let titles = [
        ("Page 1 - mdcore pdf fixture", "alpha line"),
        ("Page 2 - mdcore pdf fixture", "beta line"),
        ("Page 3 - mdcore pdf fixture", "gamma line"),
    ];
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = vec![0; 13];

    let mut push_obj = |out: &mut Vec<u8>, num: usize, body: &str| {
        offsets[num] = out.len();
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(b"\nendobj\n");
    };

    out.extend_from_slice(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n");
    let kids = (0..3)
        .map(|i| format!("{} 0 R", 3 + i))
        .collect::<Vec<_>>()
        .join(" ");
    push_obj(&mut out, 1, "<< /Type /Catalog /Pages 2 0 R >>");
    push_obj(
        &mut out,
        2,
        &format!("<< /Type /Pages /Kids [{kids}] /Count 3 >>"),
    );
    for (i, _title_extra) in titles.iter().enumerate() {
        push_obj(
            &mut out,
            3 + i,
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 6 0 R /F2 7 0 R >> >> /Contents {} 0 R >>",
                8 + i
            ),
        );
    }
    push_obj(
        &mut out,
        6,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    );
    push_obj(
        &mut out,
        7,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    );
    for (i, (title, extra)) in titles.iter().enumerate() {
        let content = format!(
            "BT /F1 24 Tf 72 720 Td ({title}) Tj ET\nBT /F2 12 Tf 72 680 Td ({extra}) Tj ET\n"
        );
        push_obj(
            &mut out,
            8 + i,
            &format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        );
    }

    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", 11).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for (n, off) in offsets.iter().enumerate().take(11).skip(1) {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        let _ = n;
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 11 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    out
}

fn meta_json() -> String {
    serde_json::to_string(&DocumentMeta {
        id: PdfDocumentId(1),
        file_name: "sample.pdf".into(),
        page_count: mdc_pdf::page::PageCount(3),
    })
    .unwrap()
}

fn pages_json() -> String {
    let pages: Vec<PageGeometry> = (0..3)
        .map(|_| PageGeometry::simple(612.0, 792.0, Rotation::Deg0))
        .collect();
    serde_json::to_string(&pages).unwrap()
}

fn session(pdf: &[u8]) -> PdfSession {
    let assessment_json = open_scan_core(pdf).unwrap();
    let outline = serde_json::to_string(&OutlineTree::new()).unwrap();
    session_new_core(&meta_json(), &assessment_json, &pages_json(), &outline).unwrap()
}

fn highlight_draft() -> AnnotationDraft {
    AnnotationDraft {
        page: mdc_pdf::page::PageIndex(0),
        kind: AnnotationKind::Highlight {
            quads: vec![Quad {
                points: [
                    Point { x: 72.0, y: 724.0 },
                    Point { x: 300.0, y: 724.0 },
                    Point { x: 300.0, y: 700.0 },
                    Point { x: 72.0, y: 700.0 },
                ],
            }],
            color: Rgb {
                r: 255,
                g: 232,
                b: 0,
            },
        },
        author: "human:web".into(),
        origin: Origin::Human,
    }
}

#[test]
fn open_scan_reports_clean() {
    let pdf = fixture_pdf();
    let json = open_scan_core(&pdf).unwrap();
    assert!(json.contains("\"encrypted\":false"), "非加密：{json}");
    assert!(json.contains("\"warnings\":[]"), "无危险对象：{json}");
}

#[test]
fn session_flow_text_search_annotate() {
    let pdf = fixture_pdf();
    let mut s = session(&pdf);
    // v0 文本注入（宿主通道）。
    for (i, text) in ["Page 1 text", "Page 2 text", "Page 3 text"]
        .iter()
        .enumerate()
    {
        s.set_page_text(mdc_pdf::page::PageIndex(i as u32), *text)
            .unwrap();
    }
    // 抽取。
    let out = execute_core(&mut s, &extract_text_cmd(&[], Source::View)).unwrap();
    assert!(out.contains("ExtractedText"), "{out}");
    assert!(out.contains("Page 2 text"), "{out}");
    // 搜索。
    let out = execute_core(&mut s, &search_cmd("Page", Source::View)).unwrap();
    assert!(out.contains("SearchResults"), "{out}");
    assert!(out.matches("page").count() >= 3, "三页命中：{out}");
    // 标注。
    let out = execute_core(&mut s, &annotate_add_cmd(highlight_draft(), Source::Ai)).unwrap();
    assert!(out.contains("AnnotAdded"), "{out}");
    assert!(out.contains("Highlight"), "{out}");
    // 列表。
    let list = mdc_pdf::command::AnnotationFilter::default();
    let out = execute_core(&mut s, &annotate_list_cmd(list, Source::View)).unwrap();
    assert!(out.contains("AnnotListed"), "{out}");
}

#[test]
fn writer_incremental_flush_prefix_and_idempotence() {
    let pdf = fixture_pdf();
    let mut s = session(&pdf);
    // 写器装载 + 标注 + flush。
    let writer = mdc_pdf::writer_lopdf::LopdfWriter::from_bytes(pdf.clone()).unwrap();
    s.attach_writer(Box::new(writer));
    s.set_clock_ms(1_700_000_000_000);
    execute_core(&mut s, &annotate_add_cmd(highlight_draft(), Source::Ai)).unwrap();

    let stats1: IncrementalSave = {
        let stats = s.flush_annotations().unwrap();
        let json = flush_stats_json(&stats);
        assert!(json.contains("\"bytes_appended\":"), "{json}");
        stats
    };
    assert!(stats1.bytes_appended > 0, "首存追加非零");
    assert_eq!(stats1.prefix_bytes, pdf.len() as u64, "前缀 = 原字节长");

    // 产物：前缀逐位保持 + 长度 = prefix + appended。
    let product = s.product_bytes().unwrap();
    assert_eq!(&product[..pdf.len()], &pdf[..], "前缀逐位保持");
    assert_eq!(
        product.len() as u64,
        stats1.prefix_bytes + stats1.bytes_appended
    );

    // 零脏二存：幂等（+0）。
    let stats2 = s.flush_annotations().unwrap();
    assert_eq!(stats2.bytes_appended, 0, "零脏幂等");

    // 产物可再扫描（增量档仍合法 PDF）。
    let json = open_scan_core(&product).unwrap();
    assert!(json.contains("\"encrypted\":false"), "增量档合法：{json}");
}

#[test]
fn bad_json_and_open_error_faces() {
    let pdf = fixture_pdf();
    let assessment_json = open_scan_core(&pdf).unwrap();
    let outline = serde_json::to_string(&OutlineTree::new()).unwrap();
    // 坏 JSON。
    let e = session_new_core("{", &assessment_json, &pages_json(), &outline).unwrap_err();
    assert_eq!(e.code, "pdf_bad_json");
    // 页几何数不匹配。
    let e = session_new_core(&meta_json(), &assessment_json, "[]", &outline).unwrap_err();
    assert_eq!(e.code, "pdf_open");
    // 垃圾字节扫描。
    let e = open_scan_core(b"not a pdf").unwrap_err();
    assert_eq!(e.code, "pdf_scan_failed");
    // 命令坏 JSON。
    let mut s = session(&pdf);
    let e = execute_core(&mut s, "{").unwrap_err();
    assert_eq!(e.code, "pdf_bad_json");
}

#[test]
fn assessment_passthrough_roundtrip() {
    // assessment JSON 与 mdc-pdf serde 原生形同构（宿主可直接复用）。
    let pdf = fixture_pdf();
    let json = open_scan_core(&pdf).unwrap();
    let a: OpenAssessment = serde_json::from_str(&json).unwrap();
    assert!(!a.encrypted);
}
