//! #345（#318 D2）：表内 DrawingML 文本框（`w:tc` 内
//! `w:drawing > wps:txbx > w:txbxContent`）提取——10 件真实语料锚
//! + 合成件定位钉 + 零双提取回归锁。
//!
//! 修复前缺陷：单元格路径只取 `TableCellContent::Paragraph` 文本
//! （`extract_table_inner`），DrawingML 文本框子树静默双丢——结构与
//! 内容均不可见（费率表等条款区常见形态）。修复 = 文档级第二趟
//! （`scan_tc_textbox_regions` / `apply_tc_textboxes`——与 #18 VML 趟
//! 同构），发射宿主表块完整跨度后。
//!
//! 实锚语料（git 跟踪，`test-artifacts/health-check/corpus/`）：
//! - extraction-probe 族 8 件（docx4j）：单元格段落文本 `In a cell: `
//!   加表内框 `TBX25_IN_CELL`（裸 drawing 或 AC Choice 主选——word
//!   resave 变体 Choice/Fallback 双写同文，锚定**恰一次**）；
//! - mc-fun 2 件（AC 包裹形态）：Choice 主选 `CELL_CHOICE_BOX`（基础
//!   件 Fallback 副本异文 `CELL_FALLBACK_BOX`——锚定缺席；word365 件
//!   双副本同文——锚定恰一次）。
//!
//! 验收（issue #345）：
//! 1. 实锚 10 件：表内文本框文本提取在场（条款文本断言）✓
//! 2. 零双提取：Choice 主选恰一次、Fallback 副本缺席 ✓
//! 3. 宿主表完整：单元格文本零损（修复不触碰宿主表）✓
//! 4. 发射位：宿主表块跨度后（含合成嵌套表定位钉）✓
//!
//! 账本收紧（`txbx_in_tc` 扣除项移除）见 docx_structural_ledger.rs。

use mdc_office_io::docx::{read_docx_from_bytes_with_options, Block, DocxDocument, TextBoxData};
use mdc_office_io::io::ReadOptions;

const CORPUS: &str = "../../test-artifacts/health-check/corpus";

fn read_corpus_file(rel: &str) -> DocxDocument {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(CORPUS)
        .join(rel);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("corpus fixture missing: {rel} ({e})——语料经 git 跟踪（#260 账本同源），缺席 = 检出异常"));
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .unwrap_or_else(|e| panic!("compat read of {rel}: {e}"))
}

/// 展平块流全部段落文本（含文本框内层——重复检测面）。
fn flattened_paragraph_texts(doc: &DocxDocument) -> Vec<String> {
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

/// 块流中承载指定文本的 TextBox 块索引（外层定位面）。
fn textbox_blocks_with_text(doc: &DocxDocument, needle: &str) -> Vec<usize> {
    let mut hits = Vec::new();
    for (i, b) in doc.blocks.iter().enumerate() {
        if let Block::TextBox(tb) = b {
            if flattened_paragraph_texts_inner(tb)
                .iter()
                .any(|t| t == needle)
            {
                hits.push(i);
            }
        }
    }
    hits
}

fn flattened_paragraph_texts_inner(tb: &TextBoxData) -> Vec<String> {
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
    walk(&tb.blocks, &mut out);
    out
}

// ---------------------------------------------------------------------------
// 实锚 ①：extraction-probe 族 8 件（TBX25_IN_CELL——裸 drawing 与
// AC Choice 主选两形态；word-resave 变体 Choice/Fallback 双写同文）
// ---------------------------------------------------------------------------

const PROBE_FAMILY: &[&str] = &[
    "docx4j/text-extraction_extraction-probe.docx",
    "docx4j/text-extraction_extraction-probe.word-resave.docx",
    "docx4j/text-extraction_phase0_evidence_checked-52_extraction-probe.docx",
    "docx4j/text-extraction_phase0_evidence_extraction-probe-r2-word-save.docx",
    "docx4j/text-extraction_phase0_evidence_extraction-probe-r3-word-save.docx",
    "docx4j/text-extraction_phase0_evidence_extraction-probe-v1-rejected.docx",
    "docx4j/text-extraction_phase0_evidence_extraction-probe-v1-word-repaired.docx",
    "docx4j/text-extraction_phase0_extraction-probe.docx",
];

const IN_CELL_CLAUSE: &str = "TBX25_IN_CELL";
const HOST_CELL_TEXT: &str = "In a cell: ";

#[test]
fn tc_textbox_clause_in_block_flow_probe_family() {
    for rel in PROBE_FAMILY {
        let doc = read_corpus_file(rel);
        // ① 条款文本在场（表内框提取——修复面）。
        let flat = flattened_paragraph_texts(&doc);
        let occurrences = flat.iter().filter(|t| *t == IN_CELL_CLAUSE).count();
        assert_eq!(
            occurrences, 1,
            "{rel}: 表内框条款恰 1 次（word-resave 变体 Choice/Fallback 双写同文——Fallback 副本不得双提取）"
        );
        // ② 结构承载：TextBox 块（非单元格文本混入——结构面独立成块）。
        let hits = textbox_blocks_with_text(&doc, IN_CELL_CLAUSE);
        assert_eq!(
            hits.len(),
            1,
            "{rel}: 条款承载于恰 1 个 TextBox 块（结构面发射）"
        );
        // ③ 宿主单元格文本零损（修复不触碰宿主表提取）。
        assert!(
            flat.iter().any(|t| t == HOST_CELL_TEXT),
            "{rel}: 宿主单元格段落在场（零损）"
        );
        // ④ 条款不得混入单元格文本（表内框独立成块——单元格栅格
        // 面保持既有语义，消费方分流不歧义）。
        let in_cell_grid = doc.blocks.iter().any(|b| match b {
            Block::Table(t) => t.rows.iter().any(|r| r.iter().any(|c| c == IN_CELL_CLAUSE)),
            _ => false,
        });
        assert!(
            !in_cell_grid,
            "{rel}: 表内框文本不入单元格栅格（独立 TextBox 块承载）"
        );
    }
}

#[test]
fn tc_textbox_emitted_after_host_table_span_probe_family() {
    for rel in PROBE_FAMILY {
        let doc = read_corpus_file(rel);
        let hits = textbox_blocks_with_text(&doc, IN_CELL_CLAUSE);
        assert_eq!(hits.len(), 1, "{rel}: 恰 1 个表内框块");
        let tb_idx = hits[0];
        // 宿主表 = 含「In a cell: 」单元格的表块；发射位 = 该表块跨度
        // （本块 + 后续连续内层表块）之后紧邻。
        let mut host_span_end = None;
        for (i, b) in doc.blocks.iter().enumerate() {
            if let Block::Table(t) = b {
                if !t.nested && t.rows.iter().any(|r| r.iter().any(|c| c == HOST_CELL_TEXT)) {
                    let mut j = i + 1;
                    while j < doc.blocks.len() {
                        match &doc.blocks[j] {
                            Block::Table(nt) if nt.nested => j += 1,
                            _ => break,
                        }
                    }
                    host_span_end = Some(j);
                    break;
                }
            }
        }
        let span_end = host_span_end.unwrap_or_else(|| panic!("{rel}: 宿主表块未定位"));
        assert_eq!(
            tb_idx, span_end,
            "{rel}: 表内框紧邻宿主表块跨度后（span_end={span_end}, tb={tb_idx}）"
        );
        // TextBox 源 = 正文（第二趟仅扫 document.xml——与 VML 趟同口径）。
        if let Block::TextBox(tb) = &doc.blocks[tb_idx] {
            assert_eq!(
                tb.source,
                mdc_office_io::docx::BlockSource::Body,
                "{rel}: 表内框源 = Body"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 实锚 ②：mc-fun AC 包裹 2 件（Choice 主选在场 + Fallback 副本缺席）
// ---------------------------------------------------------------------------

#[test]
fn tc_textbox_ac_choice_taken_fallback_skipped() {
    // 基础件：Choice=CELL_CHOICE_BOX / Fallback=CELL_FALLBACK_BOX（异文
    // ——副本缺席可直接断言）。
    let doc = read_corpus_file("docx4j/mc-fun_mc-txbx-tc-block.docx");
    let flat = flattened_paragraph_texts(&doc);
    assert_eq!(
        flat.iter().filter(|t| *t == "CELL_CHOICE_BOX").count(),
        1,
        "Choice 主选恰 1 次（AC 包裹形态——整个单元格内容在 AC 内）"
    );
    assert!(
        !flat.iter().any(|t| t == "CELL_FALLBACK_BOX"),
        "Fallback 副本文本缺席（防双提取）"
    );
    // 宿主表其余单元格零损（「I prefer」在 AC 外）。
    assert!(
        flat.iter().any(|t| t == "I prefer"),
        "宿主表 AC 外单元格在场（零损）"
    );

    // word365 件：双副本同文——恰一次即证 Fallback 未双取。
    let doc = read_corpus_file("docx4j/mc-fun_mc-txbx-tc-block-word365.docx");
    let flat = flattened_paragraph_texts(&doc);
    assert_eq!(
        flat.iter().filter(|t| *t == "CELL_CHOICE_BOX").count(),
        1,
        "word365 件双副本同文：恰 1 次（Fallback 副本不双取）"
    );
}

// ---------------------------------------------------------------------------
// 合成件：嵌套表定位钉（表内框在**内层表**单元格——发射位须在
// 外层表块 + 全部内层表块之后；语料无此形态，合成钉定 lockstep 面）
// ---------------------------------------------------------------------------

#[test]
fn tc_textbox_after_nested_table_span_handbuilt() {
    use std::io::Write;

    const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    const DOC_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#;
    const OUTER_CELL: &str = "外层表单元格（承载内层表与文本框锚）";
    const NESTED_CELL: &str = "内层表单元格";
    const TC_CLAUSE: &str = "表内文本框条款：费率以最新澄清版本为准";
    const TAIL: &str = "表后正文段";
    // 结构：body = [p0, tbl_outer{ tc: [tbl_nested{tc: 内层表单元格},
    // p(外层文本 + drawing(表内框))] }, p2]——表内框锚在内层表**之后**
    // 的同单元格段落里（docx 语义：单元格内容序 = 内层表在前、锚段在后）。
    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body><w:p><w:r><w:t>表前正文段</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>内层表单元格</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>外层表单元格（承载内层表与文本框锚）</w:t></w:r><w:r><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>表内文本框条款：费率以最新澄清版本为准</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>表后正文段</w:t></w:r></w:p><w:sectPr></w:sectPr></w:body></w:document>"#;

    let mut cw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // STORED 不压缩——docx-rs 内嵌 zip 特性面窄（tr18 同款纪律）。
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
        .expect("read hand-built nested-table + tc-textbox docx");

    // 块序断言（精确定位）：[p0, tbl_outer(!nested), tbl_nested(nested),
    // TBX(表内框), p2]——表内框在宿主表完整跨度（外层 + 内层）后。
    let kinds: Vec<String> = doc
        .blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(_) => "p".to_string(),
            Block::Table(t) => format!("tbl(nested={})", t.nested),
            Block::TextBox(_) => "txbx".to_string(),
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "p".to_string(),
            "tbl(nested=false)".to_string(),
            "tbl(nested=true)".to_string(),
            "txbx".to_string(),
            "p".to_string(),
        ],
        "块序：表内框紧邻宿主表完整跨度（外层+内层表块）后"
    );
    // 条款文本 + 宿主面零损。
    let flat = flattened_paragraph_texts(&doc);
    assert_eq!(flat.iter().filter(|t| *t == TC_CLAUSE).count(), 1);
    assert!(flat.iter().any(|t| t == OUTER_CELL));
    assert!(flat.iter().any(|t| t == NESTED_CELL));
    assert!(flat.iter().any(|t| t == TAIL));
}
