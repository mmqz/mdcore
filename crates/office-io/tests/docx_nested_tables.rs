//! #39：DOCX 嵌套表格读面——`w:tbl` 嵌于外层表 `w:tc`（depth≥2）的
//! 内层表块流读回（方案 A：内层表沿文档序以 `Block::Table` 独立发射，
//! `TableData::nested = true` 标记分流；外层表在前、内层行主序随后
//! ——表数守恒）。
//!
//! 背景（标书侧 B3-3 全量差分实证）：telecom 三档采购文件的外层表格
//! 单元格内嵌费率表——旧读面静默丢弃（huaian 21 表读出 20、费率
//! 8 档整块缺失 84 块）。本文件钉定递归提取语义：
//! - 块序：[外层, 内层…]（行主序单元序、深度优先）；
//! - 单元格文本 = 段落联串（内层表不进单元格文本——独立块承载）；
//! - 深度上限 16（敌意深嵌防御：超限整块跳过 + `SkippedElement`
//!   warning，不栈溢出）。
//!
//! 单元级构造面 = docx-rs 写侧 builder（`TableCell::add_table`——
//! 读面同源上游模型，与 docx_merged_cells.rs 同模式）。

use mdc_office_io::docx::{read_docx_from_bytes, Block, TableData};
use mdc_office_io::io::WarningKind;

fn text_cell(text: &str) -> docx_rs::TableCell {
    docx_rs::TableCell::new()
        .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)))
}

/// 单元格 = 段落 + 内嵌表（huaian 费率表形态：说明文字 + 明细表）。
fn cell_with_table(text: &str, inner: docx_rs::Table) -> docx_rs::TableCell {
    text_cell(text).add_table(inner)
}

fn rate_table() -> docx_rs::Table {
    // huaian 主文件第二章费率表缩影（表头 + 两档）。
    docx_rs::Table::new(vec![])
        .set_grid(vec![1500, 1000, 1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("费率"),
            text_cell("中标金额"),
            text_cell("货物招标"),
            text_cell("服务招标"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("100万元以下"),
            text_cell("1.5%"),
            text_cell("1.5%"),
            text_cell("1.0%"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("5000万元～1亿元"),
            text_cell("0.25%"),
            text_cell("0.1%"),
            text_cell("0.2%"),
        ]))
}

fn pack_table(table: docx_rs::Table) -> Vec<u8> {
    let built = docx_rs::Docx::new().add_table(table).build();
    let mut buf = std::io::Cursor::new(Vec::new());
    built.pack(&mut buf).expect("docx-rs pack");
    buf.into_inner()
}

fn body_tables(bytes: &[u8]) -> Vec<TableData> {
    let doc = read_docx_from_bytes(bytes).expect("read_docx_from_bytes");
    doc.blocks
        .into_iter()
        .filter_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn owned(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| r.iter().map(|s| s.to_string()).collect())
        .collect()
}

/// 主路径：外层表 + 单元格内嵌费率表 → 两块（外层 nested=false 在前、
/// 内层 nested=true 随后）；内层行逐位读回（费率档不再缺失）；
/// 单元格文本 = 段落文本（内层表不进单元格）。
#[test]
fn nested_table_emitted_as_block_with_marker() {
    let outer = docx_rs::Table::new(vec![])
        .set_grid(vec![2000, 3000])
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("招标代理服务费费率"),
            cell_with_table("详见下表", rate_table()),
        ]));
    let tables = body_tables(&pack_table(outer));
    assert_eq!(tables.len(), 2, "外层 + 内层两块（表数守恒）");
    assert!(!tables[0].nested, "外层表 nested=false");
    assert_eq!(
        tables[0].rows,
        owned(&[&["招标代理服务费费率", "详见下表"]]),
        "外层单元格文本 = 段落文本"
    );
    assert!(tables[1].nested, "内层表 nested=true");
    assert_eq!(
        tables[1].rows,
        owned(&[
            &["费率", "中标金额", "货物招标", "服务招标"],
            &["100万元以下", "1.5%", "1.5%", "1.0%"],
            &["5000万元～1亿元", "0.25%", "0.1%", "0.2%"],
        ]),
        "费率表逐行入块流"
    );
}

/// 文档序：两单元格各嵌一表 → 发射序 = 行主序单元序
///（[外层, (0,0) 内层, (1,1) 内层]——消费方按块序重排即文档序）。
#[test]
fn nested_tables_in_document_order() {
    fn mini(tag: &str) -> docx_rs::Table {
        docx_rs::Table::new(vec![])
            .set_grid(vec![1000])
            .add_row(docx_rs::TableRow::new(vec![text_cell(tag)]))
    }
    let outer = docx_rs::Table::new(vec![])
        .set_grid(vec![2000, 2000])
        .add_row(docx_rs::TableRow::new(vec![
            cell_with_table("A 区", mini("inner-A")),
            text_cell("普通格"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("普通格"),
            cell_with_table("B 区", mini("inner-B")),
        ]));
    let tables = body_tables(&pack_table(outer));
    assert_eq!(tables.len(), 3, "外层 + 两内层");
    assert_eq!(
        tables[1].rows,
        owned(&[&["inner-A"]]),
        "行 0 单元 0 的内层先发"
    );
    assert_eq!(
        tables[2].rows,
        owned(&[&["inner-B"]]),
        "行 1 单元 1 的内层后发"
    );
    assert!(tables[1].nested && tables[2].nested);
}

/// 深度 3（表套表套表）：深度优先——中层表先于其内层表，
/// 全链 nested=true（除最外层）。
#[test]
fn depth_three_chain_depth_first() {
    let innermost = docx_rs::Table::new(vec![])
        .set_grid(vec![1000])
        .add_row(docx_rs::TableRow::new(vec![text_cell("L3")]));
    let mid = docx_rs::Table::new(vec![])
        .set_grid(vec![1000])
        .add_row(docx_rs::TableRow::new(vec![cell_with_table(
            "L2 宿主",
            innermost,
        )]));
    let outer = docx_rs::Table::new(vec![])
        .set_grid(vec![1000])
        .add_row(docx_rs::TableRow::new(vec![cell_with_table(
            "L1 宿主",
            mid,
        )]));
    let tables = body_tables(&pack_table(outer));
    assert_eq!(tables.len(), 3, "三层三块");
    assert!(!tables[0].nested && tables[1].nested && tables[2].nested);
    assert_eq!(tables[1].rows, owned(&[&["L2 宿主"]]), "中层先于内层");
    assert_eq!(tables[2].rows, owned(&[&["L3"]]));
}

/// 敌意深嵌（20 层）：深度 16 以内照发（16 内层块），超限整块跳过
/// + `SkippedElement` warning——不栈溢出、诚实降级。
#[test]
fn hostile_deep_nesting_capped_with_warning() {
    let mut t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000])
        .add_row(docx_rs::TableRow::new(vec![text_cell("最深层")]));
    for _ in 0..19 {
        t = docx_rs::Table::new(vec![])
            .set_grid(vec![1000])
            .add_row(docx_rs::TableRow::new(vec![cell_with_table("层", t)]));
    }
    let bytes = pack_table(t);
    let doc = read_docx_from_bytes(&bytes).expect("深嵌读回不崩溃");
    let tables: Vec<&TableData> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 17, "外层 + 深度 1..=16 内层（17 块）");
    assert!(
        tables.iter().skip(1).all(|t| t.nested),
        "全部内层 nested=true"
    );
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.kind == WarningKind::SkippedElement),
        "超限跳过 warning 在档：{:?}",
        doc.warnings
    );
}
