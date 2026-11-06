//! pdfn_table_extract —— issue #232（R9·P1）：表格结构化提取验收面。
//!
//! 两层验收（与 crate::table 分层同构）：
//!
//! **算法层（纯函数，零引擎依赖——`PositionedChar` 注入）**
//! 1. `grid_reconstructs_rows_columns_and_order`——3×3 网格：行数/列数/
//!    单元格文本/列序全量重建；
//! 2. `merged_cell_midstart_span`——合并格：列中起笔 + 跨锚 →
//!    col_index/col_span 语义；
//! 3. `single_cell_lines_never_form_tables`——标题/段落单格行不入表；
//!    垂直断档拆表；
//! 4. `empty_and_degenerate_inputs_yield_no_tables`——空输入/全零尺寸。
//!
//! **位置层 + e2e（feature `engine-pdfium`）**
//! 5. `pdfium_fixture_gold_standard`——合成评分表夹具（`table_grid.pdf`，
//! 生成器 `gen_table_fixture.py` 头注即金标准）：行/列/文本/列序/
//! 合并格逐项对照 + 包围盒有序性与页界内检查；
//! 6. `pdfium_positioned_chars_invalid_page`——位置层页界防御。
//!
//! 环境合同与 pdfc_engine.rs 同源（pdfium 库走定位序，不静默 skip）。

use mdc_pdf::table::{extract_tables, PositionedChar, Table, TableCell};

/// 构造网格字符（宽=高=字号——CJK 形态；位置即几何）。
fn ch(c: char, x: f32, y: f32, size: f32) -> PositionedChar {
    PositionedChar {
        ch: c,
        left: x,
        bottom: y,
        right: x + size,
        top: y + size,
        font_size: size,
    }
}

/// 一段文本落格（从 (x, y) 起逐字排布，字宽 = size）。
fn text_at(s: &str, x: f32, y: f32, size: f32) -> Vec<PositionedChar> {
    s.chars()
        .enumerate()
        .map(|(i, c)| ch(c, x + i as f32 * size, y, size))
        .collect()
}

fn cell_texts_row(table: &Table, row: usize) -> Vec<&str> {
    table.rows[row]
        .cells
        .iter()
        .map(|c| c.text.as_str())
        .collect()
}

// ---------------------------------------------------------------- 算法层

#[test]
fn grid_reconstructs_rows_columns_and_order() {
    // 3 列锚（60/200/340）× 3 行（y = 700/670/640），11pt。
    let mut chars = Vec::new();
    let rows = [["甲", "乙", "丙"], ["一", "二", "三"], ["1", "2", "3"]];
    let xs = [60.0, 200.0, 340.0];
    let ys = [700.0, 670.0, 640.0];
    for (row, texts) in rows.iter().enumerate() {
        for (col, t) in texts.iter().enumerate() {
            chars.extend(text_at(t, xs[col], ys[row], 11.0));
        }
    }

    let tables = extract_tables(&chars);
    assert_eq!(tables.len(), 1, "单一连续表块");
    let t = &tables[0];
    assert_eq!(t.row_count(), 3, "行数重建");
    assert_eq!(t.column_count(), 3, "列数重建（col_index+span 上界）");

    for (ri, expect) in [["甲", "乙", "丙"], ["一", "二", "三"], ["1", "2", "3"]]
        .iter()
        .enumerate()
    {
        assert_eq!(cell_texts_row(t, ri), expect.to_vec(), "行 {ri} 文本");
        let cols: Vec<usize> = t.rows[ri].cells.iter().map(|c| c.col_index).collect();
        assert_eq!(cols, vec![0, 1, 2], "行 {ri} 列序");
        // 行内左缘升序。
        let lefts: Vec<f32> = t.rows[ri].cells.iter().map(|c| c.left).collect();
        let mut sorted = lefts.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(lefts, sorted, "行 {ri} 单元格左缘升序");
    }

    // 行序自上而下（y 降序）：行 0 的 top > 行 2 的 top。
    assert!(
        t.rows[0].cells[0].top > t.rows[2].cells[0].top,
        "行序 = 自上而下"
    );
    // 全表包围盒包含全部单元格。
    assert!(t.left <= 60.0 && t.right >= 341.0, "表 bbox 横向并集");
    assert!(t.top >= 711.0 && t.bottom <= 640.0, "表 bbox 纵向并集");
}

#[test]
fn merged_cell_midstart_span() {
    // 列锚 60/200/340；首行：合并格从 90 起笔（列 0 区间内）跨过锚 200
    // （12 字右缘 ≈ 90 + 12×11 = 222 > 200），列 2 独立格。
    let mut chars = Vec::new();
    chars.extend(text_at("合并单元格跨两列语义样例", 90.0, 700.0, 11.0));
    chars.extend(text_at("值", 340.0, 700.0, 11.0));
    // 第二行普通三格，锚定列网格。
    chars.extend(text_at("头", 60.0, 660.0, 11.0));
    chars.extend(text_at("中", 200.0, 660.0, 11.0));
    chars.extend(text_at("尾", 340.0, 660.0, 11.0));

    let tables = extract_tables(&chars);
    assert_eq!(tables.len(), 1);
    let t = &tables[0];
    assert_eq!(t.row_count(), 2);
    let merged = &t.rows[0].cells[0];
    assert_eq!(merged.col_index, 0, "列中起笔（90 > 60+eps）→ 起始列 0");
    assert_eq!(merged.col_span, 2, "右缘 222 覆盖锚 200 → 跨两列");
    assert_eq!(t.rows[0].cells[1].col_index, 2);
    assert_eq!(t.rows[0].cells[1].col_span, 1);
}

#[test]
fn single_cell_lines_never_form_tables() {
    let mut chars = Vec::new();
    // 标题（单格行）。
    chars.extend(text_at("评分表标题行", 260.0, 760.0, 18.0));
    // 两行表格（紧随标题，垂直连续）。
    chars.extend(text_at("甲", 60.0, 700.0, 11.0));
    chars.extend(text_at("乙", 200.0, 700.0, 11.0));
    chars.extend(text_at("一", 60.0, 670.0, 11.0));
    chars.extend(text_at("二", 200.0, 670.0, 11.0));
    // 表后段落（单格行）。
    chars.extend(text_at("注：本表为合成夹具。", 60.0, 600.0, 11.0));

    let tables = extract_tables(&chars);
    assert_eq!(tables.len(), 1, "标题/段落单格行不入表——恰一张表");
    assert_eq!(tables[0].row_count(), 2);
    assert!(
        !tables[0]
            .rows
            .iter()
            .flat_map(|r| r.cells.iter())
            .any(|c| c.text.contains("标题") || c.text.contains("注：")),
        "单格行文本不得混入表"
    );
}

#[test]
fn empty_and_degenerate_inputs_yield_no_tables() {
    assert!(extract_tables(&[]).is_empty(), "空输入 → 无表");
    let degenerate = vec![ch('甲', 0.0, 0.0, 0.0), ch('乙', 100.0, 0.0, 0.0)];
    assert!(
        extract_tables(&degenerate).is_empty(),
        "全零尺寸字符 → 卫生过滤后无表"
    );
    // 仅一行单格（段落）→ 不成表。
    let para = text_at("只有一段话", 60.0, 700.0, 11.0);
    assert!(extract_tables(&para).is_empty());
}

// ------------------------------------------------- 位置层 + e2e（pdfium）

#[cfg(feature = "engine-pdfium")]
mod pdfium_face {
    use super::*;
    use mdc_pdf::engine_pdfium::PdfiumEngine;
    use mdc_pdf::page::PageIndex;

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} 读取失败：{e}"))
    }

    /// 金标准 e2e：生成器 `gen_table_fixture.py` 头注（列锚
    /// 60/140/330/400；表头+4 数据行+合计行；合计格 col 0 span 2）。
    #[test]
    fn pdfium_fixture_gold_standard() {
        let engine = PdfiumEngine::open(fixture_bytes("table_grid.pdf")).unwrap();
        let chars = engine.positioned_chars(PageIndex(0)).unwrap();
        assert!(!chars.is_empty(), "位置字符流非空（pdfium loose_bounds）");

        let tables = extract_tables(&chars);
        assert_eq!(tables.len(), 1, "恰一张表（标题/段落单格行不入表）");
        let t = &tables[0];

        // A4 = 595×842 pt：包围盒页界内。
        assert!(t.left >= 0.0 && t.right <= 595.0, "表 bbox 横向页界内");
        assert!(t.bottom >= 0.0 && t.top <= 842.0, "表 bbox 纵向页界内");

        // 行数：表头 + 4 数据 + 合计 = 6。
        assert_eq!(t.row_count(), 6, "行数重建（金标准）");
        assert_eq!(t.column_count(), 4, "列数重建（金标准）");

        // 表头行：文本 + 列序。
        assert_eq!(
            cell_texts_row(t, 0),
            vec!["序号", "评分项", "分值", "评分标准"],
            "表头文本"
        );
        let cols: Vec<usize> = t.rows[0].cells.iter().map(|c| c.col_index).collect();
        assert_eq!(cols, vec![0, 1, 2, 3], "表头列序");

        // 数据行（抽查首行与末行）。
        assert_eq!(
            cell_texts_row(t, 1),
            vec!["1", "技术方案", "20", "方案完整、技术路线合理"]
        );
        assert_eq!(
            cell_texts_row(t, 4),
            vec!["4", "售后服务", "5", "服务承诺明确"]
        );

        // 合计行：合并格 col 0 span 2 + 分值 35 col 2。
        let total = &t.rows[5].cells;
        assert_eq!(total.len(), 2, "合计行两格（合并格 + 分值）");
        assert_eq!(total[0].text, "合计（技术+商务）");
        assert_eq!(total[0].col_index, 0, "合并格起始于列 0");
        assert_eq!(total[0].col_span, 2, "合并格跨序号+评分项两列锚");
        assert_eq!(total[1].text, "35");
        assert_eq!(total[1].col_index, 2);

        // 行序自上而下 + 行内左缘升序（全表）。
        for w in t.rows.windows(2) {
            assert!(
                w[0].cells[0].top > w[1].cells[0].top,
                "行序自上而下（top 降序）"
            );
        }
        for r in &t.rows {
            let lefts: Vec<f32> = r.cells.iter().map(|c: &TableCell| c.left).collect();
            let mut sorted = lefts.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(lefts, sorted, "行内左缘升序");
        }

        // 标题「评分表」不得以单元格形态出现。
        assert!(
            !t.rows
                .iter()
                .flat_map(|r| r.cells.iter())
                .any(|c| c.text == "评分表"),
            "页首标题（单格行）不入表"
        );
    }

    /// 位置层页界防御（与 extract_text 同口径）。
    #[test]
    fn pdfium_positioned_chars_invalid_page() {
        let engine = PdfiumEngine::open(fixture_bytes("table_grid.pdf")).unwrap();
        let err = engine.positioned_chars(PageIndex(9));
        assert!(
            matches!(err, Err(mdc_pdf::command::CommandError::InvalidPage(_))),
            "越界页 → typed InvalidPage，得 {err:?}"
        );
    }
}
