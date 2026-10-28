//! #22 P2-2：DOCX 表格合并单元格读面——栅格展开（gridSpan 覆盖列 /
//! vMerge 续格置空，内容只居原点格——Word 显示语义）+ `merges` 矩形
//! 清单（结构丢失显式承载）。
//!
//! 单元级构造面 = docx-rs 写侧 builder（本 crate 产品依赖——读面的
//! 同源上游模型，非第二实现：`TableCell::vertical_merge`/`grid_span`
//! → `w:vMerge`/`w:gridSpan` XML）；真 Word 验收 =
//! `TR2/fixtures/word-merged.docx`（纵合并 R1C1-R2C1 + 横合并
//! R1C2-R1C3，生成日志 gen-log.txt「vmerge ok / hmerge ok」）。
//!
//! 语义面（extract_table 模块文档同源）：
//! - 纵向：restart-continue 链 → 单条矩形；续格内容置空（Word 显示
//!   语义——续格文本不可见，部分生成器放副本，本层丢弃）；
//! - 横向：gridSpan > 1 → 覆盖列置空 + 单条矩形；
//! - 块合并（叠加）：单条矩形不拆两条；
//! - 单行单列 restart 未续：**非合并**（无覆盖格，不发区域）；
//! - 孤儿续格（continue 无匹配 restart，OOXML 未定义）：普通格处理
//!   （内容保留，不发明区域）。

use mdc_office_io::docx::{read_docx_from_bytes, Block, MergeRange, TableData};

const FIXTURE: &str = "../../test-artifacts/TR2/fixtures/word-merged.docx";

fn fixture_bytes() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "fixture missing: {} ({e}) — test-artifacts/TR2/fixtures/word-merged.docx \
             是 #22 P2-2 验收证据件，缺失 = 检出不全",
            path.display()
        )
    })
}

// ---------------------------------------------------------------------------
// docx-rs 构造面（单元级）
// ---------------------------------------------------------------------------

fn text_cell(text: &str) -> docx_rs::TableCell {
    docx_rs::TableCell::new()
        .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)))
}

fn vmerge_cell(text: &str, t: docx_rs::VMergeType) -> docx_rs::TableCell {
    text_cell(text).vertical_merge(t)
}

fn span_cell(text: &str, span: usize) -> docx_rs::TableCell {
    text_cell(text).grid_span(span)
}

/// builder 表 → docx 字节（docx-rs pack = office-io 写面同一序列化上游）。
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

fn mr(r0: usize, c0: usize, r1: usize, c1: usize) -> MergeRange {
    MergeRange {
        start_row: r0,
        start_col: c0,
        end_row: r1,
        end_col: c1,
    }
}

fn owned(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| r.iter().map(|s| s.to_string()).collect())
        .collect()
}

// ---------------------------------------------------------------------------
// 单元级矩阵
// ---------------------------------------------------------------------------

/// 纵向合并全链：restart → continue → 链尾；续格副本内容置空（原点承载）。
#[test]
fn vertical_merge_chain_content_once() {
    let t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("A", docx_rs::VMergeType::Restart),
            text_cell("x"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("续格副本-Word不可见", docx_rs::VMergeType::Continue),
            text_cell("y"),
        ]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].merges, vec![mr(0, 0, 1, 0)]);
    assert_eq!(tables[0].rows, owned(&[&["A", "x"], &["", "y"]]));
}

/// 横向合并（gridSpan）：覆盖列置空，内容居原点格。
#[test]
fn horizontal_grid_span_covers_columns() {
    let t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000, 1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            span_cell("AB", 2),
            text_cell("C"),
        ]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].merges, vec![mr(0, 0, 0, 1)]);
    assert_eq!(tables[0].rows, owned(&[&["AB", "", "C"]]));
}

/// 块合并（vMerge + gridSpan 叠加）：单条矩形，不拆两条。
#[test]
fn block_merge_single_rectangle() {
    let t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000, 1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("A", docx_rs::VMergeType::Restart).grid_span(2),
            text_cell("c"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("", docx_rs::VMergeType::Continue).grid_span(2),
            text_cell("f"),
        ]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].merges, vec![mr(0, 0, 1, 1)]);
    assert_eq!(tables[0].rows, owned(&[&["A", "", "c"], &["", "", "f"]]));
}

/// 单行单列 restart 未续：非合并（无覆盖格——不发区域，栅格原样）。
#[test]
fn single_row_restart_is_not_a_merge() {
    let t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("A", docx_rs::VMergeType::Restart),
            text_cell("x"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![text_cell("p"), text_cell("q")]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    assert!(
        tables[0].merges.is_empty(),
        "无续格 = 非合并：{:?}",
        tables[0].merges
    );
    assert_eq!(tables[0].rows, owned(&[&["A", "x"], &["p", "q"]]));
}

/// 孤儿续格（continue 无前置 restart——OOXML 未定义）：内容保留，
/// 不发明区域。
#[test]
fn orphan_continue_keeps_content() {
    let t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("O", docx_rs::VMergeType::Continue),
            text_cell("x"),
        ]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    assert!(tables[0].merges.is_empty());
    assert_eq!(tables[0].rows, owned(&[&["O", "x"]]));
}

/// 链中断关账：restart → continue → 无续行（区域终止于最后续行）。
#[test]
fn chain_gap_closes_at_last_continued_row() {
    let t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("A", docx_rs::VMergeType::Restart),
            text_cell("x"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("", docx_rs::VMergeType::Continue),
            text_cell("y"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![text_cell("p"), text_cell("q")]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].merges, vec![mr(0, 0, 1, 0)]);
    assert_eq!(
        tables[0].rows,
        owned(&[&["A", "x"], &["", "y"], &["p", "q"]])
    );
}

/// 续格 span 与 restart 不一致（畸形面）：不匹配开放区 → 孤儿处理。
#[test]
fn span_mismatch_continue_is_orphan() {
    let t = docx_rs::Table::new(vec![])
        .set_grid(vec![1000, 1000, 1000])
        .add_row(docx_rs::TableRow::new(vec![
            vmerge_cell("A", docx_rs::VMergeType::Restart),
            text_cell("x"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![vmerge_cell(
            "W",
            docx_rs::VMergeType::Continue,
        )
        .grid_span(2)]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    // restart 单行未续（span 不匹配）非合并；span-2 续格孤儿不发明区域。
    assert!(
        tables[0].merges.is_empty(),
        "span 不匹配孤儿面：{:?}",
        tables[0].merges
    );
    assert_eq!(tables[0].rows, owned(&[&["A", "x", ""], &["W", "", ""]]));
}

/// 无 tblGrid 的 builder 表（grid 空）：行和兜底栅格宽。
#[test]
fn missing_tbl_grid_falls_back_to_row_sums() {
    let t = docx_rs::Table::new(vec![]).add_row(docx_rs::TableRow::new(vec![
        span_cell("AB", 2),
        text_cell("C"),
    ]));
    let tables = body_tables(&pack_table(t));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].merges, vec![mr(0, 0, 0, 1)]);
    assert_eq!(tables[0].rows, owned(&[&["AB", "", "C"]]));
}

// ---------------------------------------------------------------------------
// 真 Word 验收（#22 P2-2 验收标准：「2 处合并」+ 内容不重复）
// ---------------------------------------------------------------------------

/// `word-merged.docx`：纵合并 R1C1-R2C1 + 横合并 R1C2-R1C3 →
/// 表 1 恰 2 处合并（矩形精确）+ 内容每标记恰一次（续格/覆盖格空）。
#[test]
fn word_merged_fixture_two_merges_content_once() {
    let doc = read_docx_from_bytes(&fixture_bytes()).expect("read fixture");
    let tables: Vec<&TableData> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 2, "夹具 = 两表（普通 + 合并）");

    // 表 0：普通表——无合并、栅格恒等。
    assert!(tables[0].merges.is_empty());
    assert_eq!(
        tables[0].rows,
        owned(&[&["阶段", "时间"], &["一期", "2026 Q1"]])
    );

    // 表 1：纵 (0,0)-(1,0) + 横 (0,1)-(0,2) = 恰 2 处合并。
    let t1 = tables[1];
    assert_eq!(t1.merges.len(), 2, "验收：2 处合并（实际 {:?}）", t1.merges);
    assert!(t1.merges.contains(&mr(0, 0, 1, 0)), "纵合并 R1C1-R2C1");
    assert!(t1.merges.contains(&mr(0, 1, 0, 2)), "横合并 R1C2-R1C3");

    // 栅格展开：内容只居原点格（多段原点格 \n 连接），续格/覆盖格空。
    assert_eq!(
        t1.rows,
        owned(&[
            &["R1C1\nR2C1", "R1C2\nR1C3", ""],
            &["", "R2C2", "R2C3"],
            &["R3C1", "R3C2", "R3C3"],
            &["R4C1", "R4C2", "R4C3"],
        ])
    );

    // 内容不重复：全表平铺后每标记恰出现一次。
    let flat: Vec<&str> = t1
        .rows
        .iter()
        .flat_map(|r| r.iter().map(String::as_str))
        .collect();
    for marker in [
        "R1C1", "R2C1", "R1C2", "R1C3", "R2C2", "R2C3", "R3C1", "R3C2", "R3C3", "R4C1", "R4C2",
        "R4C3",
    ] {
        let hits = flat.iter().filter(|c| c.contains(marker)).count();
        assert_eq!(hits, 1, "标记 {marker} 应恰出现一次（实际 {hits}）");
    }
}
