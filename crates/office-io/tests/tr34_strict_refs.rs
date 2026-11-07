//! #349（#318 D7）：strict OOXML（purl 命名空间）页眉/页脚引用
//! 恢复挂接——实锚 + 合成件 + 过渡期零噪声回归锁。
//!
//! 修复前缺陷：strict 文档（purl.oclc.org 命名空间）的 typed 引用
//! **在场**（docx-rs 引用元素解析命名空间无关），但**内容联接面**
//! 按过渡期关系 Type（schemas.openxmlformats.org）锁定——purl 关系
//! 类型不联内容 → `extract_headers_footers` 的「引用在而内容未联接」
//! 分支静默跳过 → 末节槽丢（读面已知损失 #7；中段节不受影响——
//! `mid_section_slots` 本就按引用直解析部件）。
//!
//! 修复 = 该分支的**部件直解析恢复**（mid_section_slots 同构：引用
//! → part path → 部件 XML 直解析；恢复可见性 Warning）+
//! `attach_unresolved_refs` 第二趟防御纵深（typed 完全未承载的引用
//! 兜底——见 docx.rs #347/#349 联合注释）。
//!
//! 实锚语料（git 跟踪）：`docx4j/strict_strict-sample-docx.docx`
//! （1 引用：default 页眉 rId11 → header1.xml「My header」；段落
//! 63 条完整——非全文性损失）。
//!
//! 验收（issue #349）：
//! 1. 实锚件：页眉页脚引用挂接在场 ✓
//! 2. 合成件：purl 形态页脚路径 + 三槽变体 ✓
//! 3. 过渡期零噪声：正常联接文档不触发恢复（typed 通道常态）✓
//!
//! 账本收紧（`refs_strict_dropped` 计入挂接面）见
//! docx_structural_ledger.rs。

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

fn hf_slot_texts(doc: &DocxDocument) -> Vec<(HeaderFooterVariant, String)> {
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

/// 实锚：strict-sample——default 页眉恢复挂接（「My header」在场）。
#[test]
fn strict_header_reference_recovered_real_anchor() {
    let doc = read_corpus_file("docx4j/strict_strict-sample-docx.docx");
    // ① 恢复挂接在场：页眉文本提取（rId11 → header1.xml）。
    let slots = hf_slot_texts(&doc);
    assert!(
        slots
            .iter()
            .any(|(v, t)| *v == HeaderFooterVariant::Default && t == "My header"),
        "strict 页眉以 Default 槽恢复挂接: {slots:?}"
    );
    // 恰 1 槽（零双挂——typed 联接缺失面只恢复一次）。
    assert_eq!(doc.headers.len(), 1, "strict 页眉恰 1 槽");
    // ② 末节与文档级同源。
    let final_section = doc.sections.last().expect("恒 ≥1 节");
    assert_eq!(final_section.headers.len(), 1, "末节 = 文档级（同源）");
    // ③ 可见性：恢复 Warning 在档（不静默）。
    assert!(
        doc.warnings.iter().any(|w| w
            .detail
            .contains("headerReference (rid rId11) content not linked")
            && w.detail.contains("#349 D7")),
        "恢复 Warning 在档: {:?}",
        doc.warnings
            .iter()
            .map(|w| w.detail.as_str())
            .collect::<Vec<_>>()
    );
    // ④ 段落内容不受影响（63 条——非全文性损失面）。
    let top_para = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Paragraph(_)))
        .count();
    assert_eq!(top_para, 63, "strict 段落数（内容不受影响）");
}

/// 合成件（purl 命名空间全形态）：页脚恢复 + 三槽变体 + rels purl Type。
#[test]
fn strict_footer_and_variants_recovered_handbuilt() {
    use std::io::Write;
    // purl 命名空间 document + 三槽引用（default 页脚 + first 页眉 +
    // even 页眉）+ rels 用 purl 关系 Type——复现「引用在场、联接面
    // 按过渡期 Type 锁定不联内容」的 strict 形态。
    const W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
    const R: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
    const R_T: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#
        .to_string();
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH1" Type="{R_T}/header" Target="header1.xml"/><Relationship Id="rIdH2" Type="{R_T}/header" Target="header2.xml"/><Relationship Id="rIdF1" Type="{R_T}/footer" Target="footer1.xml"/></Relationships>"#
    );
    let header1 = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W}"><w:p><w:r><w:t>strict 首页页眉</w:t></w:r></w:p></w:hdr>"#
    );
    let header2 = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W}"><w:p><w:r><w:t>strict 偶数页页眉</w:t></w:r></w:p></w:hdr>"#
    );
    let footer1 = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:ftr xmlns:w="{W}"><w:p><w:r><w:t>strict 默认页脚</w:t></w:r></w:p></w:ftr>"#
    );
    // 注意：pgSz/pgMar 用纯数值（docx-rs width 解析 panic 面——引擎
    // panic_guard 会修复 pt 单位形态，合成件直接用数值避开无关面）。
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" xmlns:r="{R}"><w:body><w:p><w:r><w:t>strict 正文段</w:t></w:r></w:p><w:sectPr><w:footerReference w:type="default" r:id="rIdF1"/><w:headerReference w:type="first" r:id="rIdH1"/><w:headerReference w:type="even" r:id="rIdH2"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="851" w:footer="992" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );

    let mut cw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut opts: zip::write::SimpleFileOptions = Default::default();
    opts = opts.compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("[Content_Types].xml", ct.as_bytes().to_vec()),
        ("_rels/.rels", rels.as_bytes().to_vec()),
        ("word/_rels/document.xml.rels", doc_rels.as_bytes().to_vec()),
        ("word/header1.xml", header1.as_bytes().to_vec()),
        ("word/header2.xml", header2.as_bytes().to_vec()),
        ("word/footer1.xml", footer1.as_bytes().to_vec()),
        ("word/document.xml", document.as_bytes().to_vec()),
    ] {
        cw.start_file(name, opts).unwrap();
        cw.write_all(&data).unwrap();
    }
    let bytes = cw.finish().unwrap().into_inner();

    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("read hand-built strict docx");

    // 三槽全恢复（default 页脚 + first/even 页眉——内容未联接面）。
    let slots = hf_slot_texts(&doc);
    assert_eq!(doc.headers.len(), 2, "first + even 两页眉槽恢复");
    assert_eq!(doc.footers.len(), 1, "default 页脚槽恢复");
    assert!(
        slots
            .iter()
            .any(|(v, t)| *v == HeaderFooterVariant::First && t == "strict 首页页眉"),
        "first 槽恢复: {slots:?}"
    );
    assert!(
        slots
            .iter()
            .any(|(v, t)| *v == HeaderFooterVariant::Even && t == "strict 偶数页页眉"),
        "even 槽恢复: {slots:?}"
    );
    assert!(
        slots
            .iter()
            .any(|(v, t)| *v == HeaderFooterVariant::Default && t == "strict 默认页脚"),
        "default 页脚槽恢复: {slots:?}"
    );
    // 恢复 Warning ×3（每槽一条——可见性）。
    let recovered = doc
        .warnings
        .iter()
        .filter(|w| w.detail.contains("content not linked") && w.detail.contains("#349 D7"))
        .count();
    assert_eq!(recovered, 3, "三槽恢复 Warning 各一条");
}

/// 过渡期零噪声：正常联接文档不触发恢复（typed 通道常态——恢复
/// 面零误报回归锁）。
#[test]
fn transitional_linked_headers_no_recovery_noise() {
    let doc = read_corpus_file("docx4j/headers_footers_sections_multiple.docx");
    let slots: usize = doc.headers.len() + doc.footers.len();
    assert!(slots > 0, "过渡期多槽件正常挂接（7 槽基线）");
    assert!(
        !doc.warnings
            .iter()
            .any(|w| w.detail.contains("content not linked")),
        "过渡期正常联接零恢复噪声: {:?}",
        doc.warnings
            .iter()
            .map(|w| w.detail.as_str())
            .collect::<Vec<_>>()
    );
}
