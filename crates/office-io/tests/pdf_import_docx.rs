//! #307 ②功能独立项：PDF→DOCX 文本版导出桥——office-io 面测试。
//!
//! 验证面（issue #384 验收对照）：
//! · **导出面**——模型 → DOCX 字节 → `read_docx_from_bytes` 读回段落
//!   序对照（全文在场 + 段落切分符合启发式）；
//! · **分页符结构面**——document.xml `w:br w:type="page"` 计数 = 页数−1
//!   （页位保持 1:1 可对照——与 #309② 测试同源的 XML 级断言）；
//! · **typed 拒绝面**——零页文档 fail-closed（无产物）；
//! · **无文本页占位**——图片页/空白页不发射段落但保持页位。
//!
//! 本测试面**零外部依赖**（纯模型驱动，不需要 pdfium/LibreOffice——
//! 引擎侧真实链路在 mang-shell `convert_pdf_docx.rs` 走环境门验证）。

use mdc_office_io::docx::{read_docx_from_bytes, Block};
use mdc_office_io::pdf_import::{pdf_import_to_docx, PdfImportDoc, PdfImportError, PdfImportPage};

/// 解出 document.xml（zip 级——与 docx_paragraph_layout.rs 同源模式）。
fn document_xml(bytes: &[u8]) -> String {
    let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    use std::io::Read as _;
    let mut v = Vec::new();
    ar.by_name("word/document.xml")
        .unwrap()
        .read_to_end(&mut v)
        .unwrap();
    String::from_utf8(v).unwrap()
}

/// 读回全部段落文本（只取非空白段——分页符段读回为 `"\n"` 文本段：
/// docx-rs 读层把 `w:br` 表示为换行文本，非语义文本，滤除）。
fn roundtrip_paragraph_texts(bytes: &[u8]) -> Vec<String> {
    let doc = read_docx_from_bytes(bytes).unwrap();
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => {
                let t = p.text();
                if t.trim().is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            _ => None,
        })
        .collect()
}

/// 全链：三页模型（中文段落 + 软换行 + 空行分段 + 无文本页）→ 读回
/// 段落序对照 + 分页符计数。
#[test]
fn three_page_roundtrip_paragraphs_and_page_breaks() {
    let doc = PdfImportDoc::new(vec![
        PdfImportPage::new("招标文件第一页\n第二行软换行\n\n第二段开头"),
        PdfImportPage::new(""), // 图片页/空白页——无文本层。
        PdfImportPage::new("third page line one\nline two"),
    ]);
    let bytes = pdf_import_to_docx(&doc).unwrap();

    // 段落序：页 1 两段（CJK 直连 + 空行切段）+ 页 3 一段（拉丁补空格）。
    let texts = roundtrip_paragraph_texts(&bytes);
    assert_eq!(
        texts,
        vec![
            "招标文件第一页第二行软换行".to_string(),
            "第二段开头".to_string(),
            "third page line one line two".to_string(),
        ],
        "读回段落序应与启发式输出一致"
    );

    // 分页符：3 页 = 2 个页间分页符（首页前置无）。
    let xml = document_xml(&bytes);
    assert_eq!(
        xml.matches(r#"w:type="page""#).count(),
        2,
        "三页文档应恰有 2 个页间分页符"
    );
}

/// 单页单段最小档：读回可解析 + 断言容器完好。
#[test]
fn single_page_minimal_document() {
    let doc = PdfImportDoc::new(vec![PdfImportPage::new("唯一段落")]);
    let bytes = pdf_import_to_docx(&doc).unwrap();
    assert_eq!(
        roundtrip_paragraph_texts(&bytes),
        vec!["唯一段落".to_string()]
    );
    // 单页零分页符。
    assert_eq!(document_xml(&bytes).matches(r#"w:type="page""#).count(), 0);
}

/// 零页文档：typed 拒绝（fail-closed，无产物）。
#[test]
fn zero_pages_rejected_typed() {
    let doc = PdfImportDoc::default();
    match pdf_import_to_docx(&doc) {
        Err(PdfImportError::EmptyDocument) => {}
        other => panic!("零页文档应 EmptyDocument 拒绝，得 {other:?}"),
    }
}

/// 无文本页占位：三页中页空文本——前后页文本在场 + 页位保持（2 分页符）。
#[test]
fn blank_middle_page_keeps_page_position() {
    let doc = PdfImportDoc::new(vec![
        PdfImportPage::new("页一"),
        PdfImportPage::new(""),
        PdfImportPage::new("页三"),
    ]);
    let bytes = pdf_import_to_docx(&doc).unwrap();
    assert_eq!(
        roundtrip_paragraph_texts(&bytes),
        vec!["页一".to_string(), "页三".to_string()]
    );
    assert_eq!(document_xml(&bytes).matches(r#"w:type="page""#).count(), 2);
}

/// 全空白文本文档：不拒（页位真实）——产物可解析、零段落。
#[test]
fn all_blank_pages_yield_parseable_doc() {
    let doc = PdfImportDoc::new(vec![PdfImportPage::new(" \n \n"), PdfImportPage::new("")]);
    let bytes = pdf_import_to_docx(&doc).unwrap();
    assert!(roundtrip_paragraph_texts(&bytes).is_empty());
    assert_eq!(document_xml(&bytes).matches(r#"w:type="page""#).count(), 1);
    assert_eq!(doc.total_chars(), 4, "预检面：空白字符计入扫描件判定");
}

/// 错误面 Display 可呈现（不静默纪律的载体）。
#[test]
fn error_display_is_presentable() {
    let e = PdfImportError::EmptyDocument;
    assert!(
        e.to_string().contains("零页"),
        "EmptyDocument 文案应可行动：{e}"
    );
}
