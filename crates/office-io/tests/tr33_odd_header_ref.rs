//! #347（#318 D5）：`w:type="odd"` 页眉/页脚引用旧值容错挂接——
//! 实锚 + 合成件三路径（挂接 / 冲突跳过 / 中段节）回归锁。
//!
//! 修复前缺陷：ECMA-376 合法值 default/first/even；Word 容忍旧值
//! **odd**（pre-97 语义：奇数页槽位 = 现代 default 语义）并照常挂接；
//! docx-rs 0.4.22 typed 词汇表外整体丢弃（引用 + 内容双丢——读面
//! 已知损失 #5）。修复 = 自有 quick-xml 第二趟
//! （`parse_legacy_odd_refs`）+ Default 语义挂接
//! （`attach_legacy_odd_slots`——Default 槽位空缺才挂 + 可见性
//! Warning 恒登记）。
//!
//! 实锚语料（git 跟踪）：`poi-docx/60293.docx`（POI Bug60293 真实
//! 回归件——6 引用含 1 odd 页眉）。
//!
//! 验收（issue #347）：
//! 1. 实锚件：odd 页眉槽位挂接在场（断言页眉文本提取）✓
//! 2. 合成件：odd 页脚路径 + 冲突跳过路径（default 先占）+ 中段节
//!    挂接路径 ✓
//! 3. 可见性：挂接/跳过均登记 Warning（容错不静默）✓
//!
//! 账本收紧（`refs_odd` 计入挂接面）见 docx_structural_ledger.rs。

use mdc_office_io::docx::{
    read_docx_from_bytes_with_options, Block, DocxDocument, HeaderFooterVariant,
};
use mdc_office_io::io::ReadOptions;

const CORPUS: &str = "../../test-artifacts/health-check/corpus";

fn read_corpus_file(rel: &str) -> DocxDocument {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(CORPUS)
        .join(rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "corpus fixture missing: {rel} ({e})——语料经 git 跟踪（#260 账本同源），缺席 = 检出异常"
        )
    });
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .unwrap_or_else(|e| panic!("compat read of {rel}: {e}"))
}

/// 展平页眉/页脚槽位文本（槽位断言面）。
fn hf_texts(doc: &DocxDocument) -> Vec<(HeaderFooterVariant, String)> {
    let mut out = Vec::new();
    for hf in doc.headers.iter().chain(doc.footers.iter()) {
        let mut text = String::new();
        for b in &hf.blocks {
            if let Block::Paragraph(p) = b {
                text.push_str(&p.text());
            }
        }
        out.push((hf.variant, text));
    }
    out
}

// ---------------------------------------------------------------------------
// 实锚：60293（POI Bug60293——6 引用：even/odd/even/default/first/first）
// ---------------------------------------------------------------------------

const ODD_HEADER_TEXT: &str = "ODD_PAGE_HEADER";

#[test]
fn odd_header_reference_attached_as_default_slot_60293() {
    let doc = read_corpus_file("poi-docx/60293.docx");
    // ① 挂接在场：odd 页眉文本提取（rId7 → header2.xml）。
    let slots = hf_texts(&doc);
    assert!(
        slots
            .iter()
            .any(|(v, t)| *v == HeaderFooterVariant::Default && t == ODD_HEADER_TEXT),
        "odd 页眉以 Default 槽位挂接（文本在场）: {slots:?}"
    );
    // ② 槽位数：6 挂接 = 6 引用（5 既有 + 1 odd 补）。
    let header_slots = doc.headers.len();
    let footer_slots = doc.footers.len();
    assert_eq!(header_slots, 3, "页眉槽位（even + first + odd→default）");
    assert_eq!(footer_slots, 3, "页脚槽位（default + even + first）");
    // ③ 末节槽与文档级同源（单一真源——#347 可变传播面）。
    let final_section = doc.sections.last().expect("恒 ≥1 节");
    assert_eq!(
        final_section.headers.len(),
        header_slots,
        "末节 = 文档级（同源）"
    );
    assert_eq!(
        final_section.footers.len(),
        footer_slots,
        "末节 = 文档级（同源）"
    );
    // ④ 可见性：容错挂接 Warning 在档（odd → default 不静默）。
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.detail.contains("legacy odd headerReference")
                && w.detail.contains("attached as default slot")),
        "挂接 Warning 在档: {:?}",
        doc.warnings
            .iter()
            .map(|w| w.detail.as_str())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// 合成件：三路径（odd 页脚挂接 / 冲突跳过 / 中段节挂接）
// ---------------------------------------------------------------------------

/// 合成 docx（tr18 handbuilt 同款纪律——STORED 不压缩）。
fn handbuilt_docx(document_xml: &str) -> DocxDocument {
    use std::io::Write;
    const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#;
    const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    const DOC_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdF1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#;
    const HEADER1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>合成件页眉文本</w:t></w:r></w:p></w:hdr>"#;
    const FOOTER1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>合成件页脚文本</w:t></w:r></w:p></w:ftr>"#;
    let mut cw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut opts: zip::write::SimpleFileOptions = Default::default();
    opts = opts.compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("[Content_Types].xml", CT.to_vec()),
        ("_rels/.rels", RELS.to_vec()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_vec()),
        ("word/header1.xml", HEADER1.as_bytes().to_vec()),
        ("word/footer1.xml", FOOTER1.as_bytes().to_vec()),
        ("word/document.xml", document_xml.as_bytes().to_vec()),
    ] {
        cw.start_file(name, opts).unwrap();
        cw.write_all(&data).unwrap();
    }
    let bytes = cw.finish().unwrap().into_inner();
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("read hand-built odd-reference docx")
}

/// 路径 ①：仅 odd 页脚引用 → Default 槽位挂接。
#[test]
fn odd_footer_reference_attached_as_default_slot() {
    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>正文段</w:t></w:r></w:p><w:sectPr><w:footerReference w:type="odd" r:id="rIdF1"/></w:sectPr></w:body></w:document>"#;
    let doc = handbuilt_docx(DOC);
    assert_eq!(doc.headers.len(), 0, "无页眉引用 = 无页眉槽");
    assert_eq!(doc.footers.len(), 1, "odd 页脚 → Default 槽挂接");
    assert_eq!(doc.footers[0].variant, HeaderFooterVariant::Default);
    let text = doc.footers[0]
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "合成件页脚文本", "odd 页脚内容提取在场");
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.detail.contains("legacy odd footerReference")
                && w.detail.contains("attached as default slot")),
        "挂接 Warning 在档"
    );
}

/// 路径 ②：default 先占 + odd 并存 → 冲突跳过（以合法值为准）+ Warning。
#[test]
fn odd_reference_conflicting_with_legal_default_skipped() {
    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>正文段</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdH1"/><w:headerReference w:type="odd" r:id="rIdH1"/></w:sectPr></w:body></w:document>"#;
    let doc = handbuilt_docx(DOC);
    // 合法 default 先占——odd 不双挂（Default 槽恰 1，内容 = 合法引用）。
    assert_eq!(doc.headers.len(), 1, "Default 槽恰 1（合法引用先占）");
    assert_eq!(doc.headers[0].variant, HeaderFooterVariant::Default);
    assert!(
        doc.warnings.iter().any(
            |w| w.detail.contains("headerReference (type=odd") && w.detail.contains("skipped")
        ),
        "冲突跳过 Warning 在档"
    );
}

/// 路径 ③：中段节（段落级 sectPr）odd 引用 → 该节 Default 槽挂接。
#[test]
fn odd_reference_mid_section_attached() {
    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:pPr><w:sectPr><w:headerReference w:type="odd" r:id="rIdH1"/></w:sectPr></w:pPr><w:r><w:t>第一节正文</w:t></w:r></w:p><w:p><w:r><w:t>第二节正文</w:t></w:r></w:p><w:sectPr></w:sectPr></w:body></w:document>"#;
    let doc = handbuilt_docx(DOC);
    assert_eq!(doc.sections.len(), 2, "两节（中段 + 末节）");
    let mid = &doc.sections[0];
    assert_eq!(mid.headers.len(), 1, "中段节 odd 页眉挂接");
    assert_eq!(mid.headers[0].variant, HeaderFooterVariant::Default);
    let text = mid.headers[0]
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "合成件页眉文本", "中段节 odd 页眉内容在场");
    // 末节无 odd 引用 = 零挂接（odd 不跨节泄漏）。
    assert_eq!(doc.sections[1].headers.len(), 0, "末节零槽（无引用）");
    // 文档级（末节口径）零挂接——odd 只进所属节。
    assert_eq!(doc.headers.len(), 0, "文档级 = 末节（中段 odd 不上浮）");
}
