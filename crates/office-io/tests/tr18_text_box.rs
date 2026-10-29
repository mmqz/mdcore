//! #18（P1）：文本框（w:txbxContent）块提取——VML/DrawingML 双形态 + 宿主零截断。
//!
//! 夹具 `test-artifacts/corpusR/D2a-nonbody.docx`（源：ai-bid-research
//! corpus-R D2a——澄清文件样本，与标书仓原件字节一致 MD5 d24f1cf2）：
//! **单个** VML 文本框（`w:pict → v:shape → v:textbox → w:txbxContent`
//! 形态——引擎 docx-rs 读面不承载，office-io 文档级第二趟补提取）。
//!
//! 口径勘误（issue #18「×2」溯源）：构造脚本 `v1_corpus_r.py::_add_textbox_paragraph`
//! 单次调用、全包条款文本恰 1 处（UTF-8/UTF-16 全部件扫描实证）。issue 所引
//! bid-parse 基线的「2 块命中」是该侧**双重提取缺陷**（同一条款重复两块）——
//! 引擎正确口径 = **恰 1 命中且全块流零重复**（本套件钉定为回归锁）。
//!
//! 验收（issue #18）：文本框条款文本在读回块流 ✓ + 宿主段落文本完整
//! （无截断——bid-parse 侧缺陷在引擎面根治）✓。

use mdc_office_io::docx::{read_docx_from_bytes_with_options, Block, TextBoxData};
use mdc_office_io::io::ReadOptions;

const FIXTURE: &str = "../../test-artifacts/corpusR/D2a-nonbody.docx";
const TB_CLAUSE: &str =
    "文本框条款：本文件所附技术参数表以最新澄清版本为准（本条款载于文本框内，非正文段落）";

fn read_fixture() -> mdc_office_io::docx::DocxDocument {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("compat read of textbox fixture")
}

/// 展平全部块（含文本框内层）的段落文本流——重复检测用。
fn flattened_paragraph_texts(doc: &mdc_office_io::docx::DocxDocument) -> Vec<String> {
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            match b {
                Block::Paragraph(p) => out.push(p.text()),
                Block::Table(t) => {
                    for row in &t.rows {
                        for cell in row {
                            out.push(cell.clone());
                        }
                    }
                }
                Block::TextBox(tb) => walk(&tb.blocks, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out);
    out
}

#[test]
fn vml_text_box_clauses_in_block_flow() {
    let doc = read_fixture();
    let tb_blocks: Vec<&TextBoxData> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::TextBox(tb) => Some(tb),
            _ => None,
        })
        .collect();
    // 夹具单文本框（见模块口径勘误）——恰 1 命中。
    assert_eq!(tb_blocks.len(), 1, "单文本框块（夹具 1 个 v:textbox）");
    for tb in &tb_blocks {
        let texts: Vec<String> = tb
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|t| t == TB_CLAUSE),
            "文本框条款完整入块流: {texts:?}"
        );
        // 内层段落经正文同一机制（runs 承载 + revision_inserted 面）。
        for b in &tb.blocks {
            if let Block::Paragraph(p) = b {
                assert!(!p.runs.is_empty(), "run 级承载");
                assert!(!p.runs.iter().any(|r| r.revision_inserted));
            }
        }
    }
    // 反双重提取回归锁：bid-parse 基线「2 命中」实为同一条款重复两块
    // （其侧缺陷）——引擎全块流（含内层展平）条款文本恰出现 1 次。
    let occurrences = flattened_paragraph_texts(&doc)
        .iter()
        .filter(|t| *t == TB_CLAUSE)
        .count();
    assert_eq!(
        occurrences, 1,
        "全块流条款恰 1 次（零双重提取——bid-parse 侧缺陷不复刻）"
    );
}

/// 宿主零截断：夹具的正文段（澄清一/二/三/四…）逐字完整——文本框
/// 提取不触碰宿主段（bid-parse 侧「宿主段落后文本被截断」缺陷的
/// 引擎面根治证明）。
#[test]
fn host_paragraphs_intact_no_truncation() {
    let doc = read_fixture();
    let body: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    // 澄清四条款（#17 验收同款）完整在场——文本框段之后的正文不截断。
    assert!(
        body.iter()
            .any(|t| t.starts_with("澄清四：响应截止时间口径如下")),
        "澄清四完整: {body:?}"
    );
    // 各澄清条编号在场（全正文流完整性抽查）。
    for prefix in ["澄清一", "澄清二", "澄清三", "澄清四"] {
        assert!(body.iter().any(|t| t.contains(prefix)), "{prefix} 在场");
    }
}

/// 文档序位置：文本框块紧随宿主段（VML 第二趟的宿主块索引锚定）。
#[test]
fn text_box_positioned_after_host() {
    let doc = read_fixture();
    let mut tb_hosts: Vec<usize> = Vec::new();
    for (i, b) in doc.blocks.iter().enumerate() {
        if let Block::TextBox(_) = b {
            tb_hosts.push(i)
        }
    }
    assert_eq!(tb_hosts.len(), 1);
    // 宿主 = 文本框块的前一块（pict 载体段——空文本段在场）。
    for &i in &tb_hosts {
        let prev_empty_or_pict = matches!(
            doc.blocks.get(i - 1),
            Some(Block::Paragraph(p)) if p.text().is_empty()
        );
        assert!(
            prev_empty_or_pict,
            "块 {i} 前为宿主段（pict 载体，空文本）: {:?}",
            doc.blocks.get(i - 1).map(|b| match b {
                Block::Paragraph(p) => p.text(),
                _ => "(非段)".to_string(),
            })
        );
    }
}

/// DrawingML 形态（`w:drawing → wp:inline → a:graphic → a:graphicData →
/// wps:wsp → wps:txbx → w:txbxContent`）手构最小包读回。
///
/// docx-rs 写面对 TextBox 为 `todo!()`（未实现）——无法走写读往返，
/// 依 corpus_harness 手构 zip 模式直接构造（STORED 不压缩）。模型面
/// 提取路径（extract_blocks → paragraph_text_box_children）与 VML 第二趟
/// （apply_vml_textboxes）互补——本测锁定 DrawingML 通道 + 宿主完整。
#[test]
fn drawingml_text_box_read_from_handbuilt_package() {
    use std::io::Write;

    const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    // docx-rs read_document_rels 对该部件硬性要求（`?` 直传——缺件 =
    // FileNotFound → "Failed to read from zip"）。空表即合法。
    const DOC_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#;
    const DML_CLAUSE: &str = "DrawingML 文本框条款：图文混排附件以最新澄清版本为准";
    const HOST_TEXT: &str = "宿主段（承载浮动文本框锚点）";
    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body><w:p><w:r><w:t>宿主段（承载浮动文本框锚点）</w:t></w:r><w:r><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>DrawingML 文本框条款：图文混排附件以最新澄清版本为准</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p><w:sectPr></w:sectPr></w:body></w:document>"#;

    let mut cw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // STORED 不压缩——docx-rs 内嵌 zip 特性面窄（Deflated 支持不在其
    // 特性集），corpus_harness 同款纪律。
    let mut opts: zip::write::SimpleFileOptions = Default::default();
    opts = opts.compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("[Content_Types].xml", CT.to_vec()),
        ("_rels/.rels", RELS.to_vec()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_vec()),
        ("word/document.xml", DOC.as_bytes().to_vec()),
    ] {
        cw.start_file(name, opts).unwrap();
        cw.write_all(&data).unwrap();
    }
    let bytes = cw.finish().unwrap().into_inner();

    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("read hand-built DrawingML textbox docx");

    // 文本框块命中 + 条款文本承载（模型面路径）。
    let tb_blocks: Vec<&TextBoxData> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::TextBox(tb) => Some(tb),
            _ => None,
        })
        .collect();
    assert_eq!(tb_blocks.len(), 1, "DrawingML 文本框 1 块");
    let inner: Vec<String> = tb_blocks[0]
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    assert!(
        inner.iter().any(|t| t == DML_CLAUSE),
        "DrawingML 条款入块流: {inner:?}"
    );

    // 宿主段完整（DrawingML 锚 run 不吃掉宿主文本）。
    let hosts: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    assert!(
        hosts.iter().any(|t| t == HOST_TEXT),
        "宿主段完整: {hosts:?}"
    );
}

// ---------------------------------------------------------------------------
// 批 4 体检回归锁（health-check docx4j 语料 vml_textbox.docx 发现）：
// pretty-printed document.xml（元素间换行缩进空白）此前被门 3 误拒
// "textbox region malformed (missing txbxContent open)"——quick-xml 0.41
// buffer_position 边界计量偏移使区段起点带前导空白/终点截断闭合标签
// （docx.rs scan_vml_textbox_regions 双端前向修复 + parse_textbox_region
// 前导空白容忍）。紧凑 Word 件因"首事件任意 StartElement 均可"恰巧
// 工作过——空白形态是真实盲区（docx4j 等工具 pretty-print 输出）。
// ---------------------------------------------------------------------------

/// 构造 pretty-printed VML 文本框 docx（v:textbox 与 w:txbxContent 之间
/// 换行 + 缩进空白——复现误拒形态；文本含跨行空白干扰字节）。
#[test]
fn pretty_printed_vml_textbox_not_rejected() {
    use std::io::Write as _;

    const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    const DOC_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#;
    // pretty-printed：pict/v:shape/v:textbox/txbxContent 元素间全部换行缩进。
    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office">
  <w:body>
    <w:p>
      <w:r>
        <w:t>宿主段落：正文承载体。</w:t>
      </w:r>
      <w:r>
        <w:pict>
          <v:shape id="TB1" style="width:100pt;height:50pt">
            <v:textbox>
              <w:txbxContent>
                <w:p>
                  <w:r>
                    <w:t>框内条款：pretty-print 形态文本框内容。</w:t>
                  </w:r>
                </w:p>
              </w:txbxContent>
            </v:textbox>
          </v:shape>
        </w:pict>
      </w:r>
    </w:p>
  </w:body>
</w:document>"#;

    let mut cw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut opts: zip::write::SimpleFileOptions = Default::default();
    opts = opts.compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("[Content_Types].xml", CT.to_vec()),
        ("_rels/.rels", RELS.to_vec()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_vec()),
        ("word/document.xml", DOC.as_bytes().to_vec()),
    ] {
        cw.start_file(name, opts).unwrap();
        cw.write_all(&data).unwrap();
    }
    let bytes = cw.finish().unwrap().into_inner();

    // 修复前：Container(InvalidPart "textbox region malformed")。
    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("pretty-printed VML 文本框不再被误拒");
    let tbs: Vec<&TextBoxData> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::TextBox(tb) => Some(tb),
            _ => None,
        })
        .collect();
    assert_eq!(tbs.len(), 1, "文本框块恰 1: {}", doc.blocks.len());
    let texts = flattened_paragraph_texts(&doc);
    assert!(
        texts.iter().any(|t| t.contains("框内条款")),
        "框内文本在块流: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("宿主段落")),
        "宿主段完整: {texts:?}"
    );
}
