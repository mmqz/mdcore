//! #19（P2）：批注/脚注/尾注读面——锚关联 + 部件第二趟 + 引用位标注。
//!
//! 三夹具三通道：
//! · `corpusR/D2a-nonbody.docx`（澄清文件样本）：批注 id=701 锚定
//!   澄清一段（commentRangeStart 段落级锚——引擎模型面承载）；
//! · `TR2/fixtures/word-complex.docx`（真 Word COM 生成）：批注
//!   id=1（作者 zi ye——真 Word 批注栈含 commentsExtended/Ids/
//!   Extensible，模型面主件读取）；
//! · 手构最小包（docx-rs 写面无 TextBox/脚注件生成面——#18 同款
//!   corpus_harness 手构 zip 纪律）：footnotes.xml 分隔符(-1/0)+
//!   实体脚注 ×2、endnotes.xml 实体尾注 ×1、正文 w:footnoteReference
//!   引用位（段落级标注）+ w:endnoteReference（尾注不标注——内容面
//!   承载）。
//!
//! 验收（issue #19）：读含批注样本断言批注文本 + 锚段关联 ✓；读含
//! 脚注样本断言 footnotes.xml 文本读回（w:footnoteReference 位置可
//! 标记）✓。写读不对称收口（写面 RunSpec::footnote 已有——CAPABILITY
//! NOTES §5 读面缺口本套件钉定补齐）。

use mdc_office_io::docx::{read_docx_from_bytes_with_options, Block};
use mdc_office_io::io::ReadOptions;

const D2A: &str = "../../test-artifacts/corpusR/D2a-nonbody.docx";
const TR2: &str = "../../test-artifacts/TR2/fixtures/word-complex.docx";

fn read_fixture(rel: &str) -> mdc_office_io::docx::DocxDocument {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("compat read of fixture")
}

/// D2a：批注文本 + 作者/日期 + 锚段关联（澄清一段——最终块序数组
/// 索引直取块内容互证）。
#[test]
fn d2a_comment_anchor_and_text() {
    let doc = read_fixture(D2A);
    assert_eq!(doc.comments.len(), 1, "D2a 单批注: {:?}", doc.comments);
    let c = &doc.comments[0];
    assert_eq!(c.id, 701);
    assert_eq!(c.author, "示例作者");
    assert_eq!(c.date, "2026-10-20T09:00:00Z");
    // 批注内容块（段落承载——run 级提取）。
    let texts: Vec<String> = c
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("评审查疑点：本条与后续补遗可能存在口径差异")),
        "批注文本读回: {texts:?}"
    );
    // 锚段关联：anchor_block_idx 直取块 = 澄清一（构造脚本锚定段）。
    let anchor = c
        .anchor_block_idx
        .expect("D2a 批注有锚（commentRangeStart id=701）");
    match &doc.blocks[anchor] {
        Block::Paragraph(p) => assert!(
            p.text().starts_with("澄清一：机房建设标准"),
            "锚段 = 澄清一: {}",
            p.text()
        ),
        other => panic!("锚块非段落: {other:?}"),
    }
    // D2a 无脚注/尾注（部件缺席零成本 + 引用位无标注）。
    assert!(doc.footnotes.is_empty());
    assert!(doc.endnotes.is_empty());
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            assert!(p.footnote_refs.is_empty(), "D2a 无脚注引用位");
        }
    }
}

/// TR2 真件：真 Word 批注（完整现代批注栈——commentsExtended/Ids/
/// Extensible 在场，模型面主件读取）。
#[test]
fn tr2_word_complex_comment() {
    let doc = read_fixture(TR2);
    assert_eq!(doc.comments.len(), 1, "TR2 单批注: {:?}", doc.comments);
    let c = &doc.comments[0];
    assert_eq!(c.id, 1);
    assert_eq!(c.author, "zi ye");
    let texts: Vec<String> = c
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t == "COMMENT-NEED-CERT"),
        "真 Word 批注文本: {texts:?}"
    );
    // 锚段 = 含智能文档平台的正文段。
    let anchor = c.anchor_block_idx.expect("TR2 批注有锚");
    match &doc.blocks[anchor] {
        Block::Paragraph(p) => assert!(p.text().contains("智能文档平台"), "锚段关联: {}", p.text()),
        other => panic!("锚块非段落: {other:?}"),
    }
    // TR2 脚注/尾注仅分隔符条目（-1/0）——过滤后空集（呈现辅助件
    // 非内容，不伪装支持）。
    assert!(
        doc.footnotes.is_empty(),
        "TR2 脚注仅分隔符: {:?}",
        doc.footnotes
    );
    assert!(
        doc.endnotes.is_empty(),
        "TR2 尾注仅分隔符: {:?}",
        doc.endnotes
    );
}

/// 手构包：脚注/尾注部件第二趟（分隔符过滤 + 实体条目读回）+ 正文
/// 引用位段落级标注（w:footnoteReference 标注 / w:endnoteReference
/// 内容面承载不标注）。
#[test]
fn handbuilt_footnotes_endnotes_read() {
    use std::io::Write;

    const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/></Types>"#;
    const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    // docx-rs read_document_rels 硬性要求（#18 踩坑钉定）：空表即合法。
    const DOC_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#;
    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>标题段</w:t></w:r></w:p><w:p><w:r><w:t>正文甲段</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p><w:p><w:r><w:t>正文乙段</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p><w:p><w:r><w:t>尾注宿主段</w:t></w:r><w:r><w:endnoteReference w:id="1"/></w:r></w:p><w:sectPr></w:sectPr></w:body></w:document>"#;
    const FOOTNOTES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>脚注一：投标保证金金额以开标一览表为准。</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>脚注二：开标地点以澄清文件载明口径为准。</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
    const ENDNOTES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:endnote><w:endnote w:id="1"><w:p><w:r><w:t>尾注一：合同条款歧义以补遗编号顺延解释。</w:t></w:r></w:p></w:endnote></w:endnotes>"#;

    let mut cw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut opts: zip::write::SimpleFileOptions = Default::default();
    opts = opts.compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("[Content_Types].xml", CT.to_vec()),
        ("_rels/.rels", RELS.to_vec()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_vec()),
        ("word/document.xml", DOC.as_bytes().to_vec()),
        ("word/footnotes.xml", FOOTNOTES.as_bytes().to_vec()),
        ("word/endnotes.xml", ENDNOTES.as_bytes().to_vec()),
    ] {
        cw.start_file(name, opts).unwrap();
        cw.write_all(&data).unwrap();
    }
    let bytes = cw.finish().unwrap().into_inner();

    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("read hand-built notes docx");

    // 脚注：分隔符（-1/0）过滤，实体 ×2 读回（id 与文本对偶）。
    assert_eq!(doc.footnotes.len(), 2, "实体脚注 2 条: {:?}", doc.footnotes);
    assert_eq!(doc.footnotes[0].id, 1);
    assert_eq!(doc.footnotes[1].id, 2);
    let fn_text = |i: usize| -> String {
        doc.footnotes[i]
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    };
    assert_eq!(fn_text(0), "脚注一：投标保证金金额以开标一览表为准。");
    assert_eq!(fn_text(1), "脚注二：开标地点以澄清文件载明口径为准。");

    // 尾注：同构通道（separator 过滤 + 实体读回）。
    assert_eq!(doc.endnotes.len(), 1, "实体尾注 1 条: {:?}", doc.endnotes);
    assert_eq!(doc.endnotes[0].id, 1);
    let en_text: String = doc.endnotes[0]
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(en_text, "尾注一：合同条款歧义以补遗编号顺延解释。");

    // 引用位标注：正文甲段 [1]、正文乙段 [2]；标题段与尾注宿主段
    // 无脚注标注（尾注引用位不标注——内容面承载，文档化语义）。
    let paras: Vec<&mdc_office_io::docx::ParagraphData> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(paras.len(), 4);
    assert!(paras[0].footnote_refs.is_empty(), "标题段无引用位");
    assert_eq!(paras[1].footnote_refs, vec![1], "正文甲段标注脚注 1");
    assert_eq!(paras[2].footnote_refs, vec![2], "正文乙段标注脚注 2");
    assert!(
        paras[3].footnote_refs.is_empty(),
        "尾注宿主段不标注（endnote 引用位非脚注标注面）"
    );

    // 手构包无批注部件——空集（部件缺席零成本）。
    assert!(doc.comments.is_empty());
}
