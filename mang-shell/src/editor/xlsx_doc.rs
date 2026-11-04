//! xlsx 只读文档模型（#68 阶段 G6——office-io 读面 → 行模型）。
//!
//! 职责：把 [`mdc_office_io::io`] 的 xlsx 读面（calamine 五 API + 三门
//! 结构）转换为编辑器可渲染的**行模型**（纯文本行 + 逐行样式 span +
//! 工作表树 + 工档属性）——渲染/滚动/查找/选区/光标全复用批 1-2 的
//! Editor 管线，仅以只读门禁关闭变更类命令（docx/pdf 同口径）。
//!
//! ## 转换规则（sheet → 行）
//!
//! - **表分隔行**：`── 工作表: <name> ──`（整行 Quote——pdf 页分隔
//!   同风格惯例）+ 记入工作表树（[`HeadingEntry`] level=1，点击跳转）。
//! - **空表**：分隔行下落 `（空表——无数据格）` 提示行（Quote——
//!   明确呈现而非白屏）。
//! - **列字母行**：数据行之前一行列字母（A/B/…，绝对列号——起始
//!   偏移换算），逐字母 Bold span，与数据列按列宽右对齐。
//! - **数据行**：`<行号>  <cell> | <cell> | …`——行号为**绝对行号**
//!   （网格起始偏移 +1，1 基），ListMarker span（弱化色——与 md 列表
//!   标记同色语义）；单元格文本 = [`CellData::display`]（与 calamine
//!   `to_string` 逐字同源）；列宽按 East Asian 宽度对齐（CJK 2 cell，
//!   与 docx 表格同口径），超宽截断 + `…`。
//! - **公式格**：有缓存值 → 显示缓存值；无缓存（Empty）→ `=公式文本`
//!   Italic span（读面公式文本不带 `=` 前缀——office-io FIDELITY §1
//!   在档；此形态真实文件可达，写面引擎恒落缓存值）。
//! - **多表**：表间一空行；工作表树按簿序全量展开。
//!
//! ## 已知边界（G6 登记）
//!
//! - 单元格宽上限 24 cell（超宽截断 + `…`）；
//! - 每表渲染上限 1000 行 × 64 列（office-io 提取上限 4M 格的**行模型
//!   护栏**——超限渲染前 N 行 + 显式提示行 + 属性登记，绝不静默截断）；
//! - 合并格显示 = 左上单点语义（非左上格空串——calamine 值路径不触
//!   merged_regions，两 API 面无交集；#202 实证钉定，#203 勘误）；
//! - 超链接显示不渲染链接样式（计数入属性面）；
//! - 主题/数字格式面外（display 语义与 office-io t17 显示面同源）。

use std::ops::Range;
use std::path::Path;

use mdc_office_io::io::{self, Book, CellData, ReadOptions};

use super::docx_doc::HeadingEntry;
use super::highlight::TokenKind;

/// 单元格显示宽上限（East Asian cell 计，超宽截断 + `…`）。
const CELL_WIDTH_CAP: usize = 24;
/// 每表渲染行上限（office-io LIMIT_TOTAL_CELLS 的行模型护栏）。
const MAX_RENDER_ROWS: usize = 1000;
/// 每表渲染列上限（同上）。
const MAX_RENDER_COLS: usize = 64;
/// 行号槽宽下限（不足 4 位补齐——右对齐基准）。
const GUTTER_MIN: usize = 4;

/// 工档属性（右栏属性面板 + 启动日志证据通道）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XlsxProps {
    /// 工作表数。
    pub sheets: usize,
    /// 非空数据格数（读面口径——公式缓存值计入）。
    pub cells: usize,
    /// 公式格数（公式文本面独立计数）。
    pub formulas: usize,
    /// 合并范围数。
    pub merges: usize,
    /// 超链接数。
    pub hyperlinks: usize,
    /// 兼容警告数（compat 模式降级登记）。
    pub warnings: usize,
    /// 触发行模型渲染护栏的表数（截断非静默——提示行 + 此计数）。
    pub truncated_sheets: usize,
}

/// xlsx 只读载荷（行模型 + 工作表树 + 属性——`document::open` 的
/// xlsx 分派产物，`Editor::new_xlsx` 消费）。
#[derive(Clone, Debug)]
pub struct XlsxPayload {
    /// 全文纯文本（行以 `\n` 连接——喂 [`super::buffer::Buffer`]）。
    pub text: String,
    /// 逐行样式 span（char 索引区间 + token 类别；与 text 行严格对齐）。
    pub spans: Vec<Vec<(Range<usize>, TokenKind)>>,
    /// 工作表树（簿序；line = 表分隔行号——点击跳转锚）。
    pub headings: Vec<HeadingEntry>,
    /// 工档属性。
    pub props: XlsxProps,
}

/// 读盘 + 解析 + 转换（单入口——`document::open` 的 xlsx 分派目标）。
///
/// 兼容模式（`compat: true`）：畸形部件降级为结构化警告（数据照常
/// 提取，警告计数入属性）——查看器语义（docx 同口径）；三门与全部
/// 安全上限两模式恒等不放松；输入字节前置门（门 0）在 office-io 读
/// 路径内建（惰性 Seek——无全量入内存面）。
pub fn load(path: &Path) -> Result<XlsxPayload, String> {
    let book = io::read_book_with_options(&path.to_string_lossy(), &ReadOptions { compat: true })
        .map_err(|e| format!("xlsx 打开失败：{e}"))?;
    Ok(convert(book))
}

/// Book → 行模型（纯函数——单测锚点）。
fn convert(book: Book) -> XlsxPayload {
    let mut lines: Vec<String> = Vec::new();
    let mut spans: Vec<Vec<(Range<usize>, TokenKind)>> = Vec::new();
    let mut headings = Vec::new();
    let mut props = XlsxProps {
        sheets: book.sheets.len(),
        warnings: book.warnings.len(),
        ..XlsxProps::default()
    };

    let mut push = |text: String, line_spans: Vec<(Range<usize>, TokenKind)>| -> usize {
        let idx = lines.len();
        lines.push(text);
        spans.push(line_spans);
        idx
    };

    for (i, sheet) in book.sheets.iter().enumerate() {
        if i > 0 {
            push(String::new(), Vec::new());
        }
        // 表分隔行（Quote 整行——pdf 页分隔同风格）+ 工作表树条目。
        let sep = format!("── 工作表: {} ──", sheet.name);
        let sep_len = sep.chars().count();
        let sep_line = push(sep, vec![(0..sep_len, TokenKind::Quote)]);
        headings.push(HeadingEntry {
            level: 1,
            text: sheet.name.clone(),
            line: sep_line,
        });

        // 公式/合并/超链接计数（属性面）。
        props.formulas += sheet
            .formulas
            .rows()
            .map(|r| r.iter().filter(|f| !f.is_empty()).count())
            .sum::<usize>();
        props.merges += sheet.merges.len();
        props.hyperlinks += sheet.hyperlinks.len();

        // 类型化网格（绝对坐标消费——get_abs/formula_at 同坐标系）。
        let grid = &sheet.typed;
        let Some((row0, col0)) = grid.start() else {
            let notice = "（空表——无数据格）";
            let len = notice.chars().count();
            push(notice.to_string(), vec![(0..len, TokenKind::Quote)]);
            continue;
        };
        let height = grid.height();
        let width = grid.width();
        props.cells += (0..height)
            .map(|r| {
                (0..width)
                    .filter(|c| !matches!(grid.get((r, *c)), Some(CellData::Empty) | None))
                    .count()
            })
            .sum::<usize>();

        let render_rows = height.min(MAX_RENDER_ROWS);
        let render_cols = width.min(MAX_RENDER_COLS);
        let truncated = height > MAX_RENDER_ROWS || width > MAX_RENDER_COLS;
        if truncated {
            props.truncated_sheets += 1;
        }

        // 显示矩阵（截断前先定文本——列宽按渲染区实测；(文本, 公式回退)
        // 携带——Italic 判定不重算）。
        let cells_text: Vec<Vec<(String, bool)>> = (0..render_rows)
            .map(|r| {
                (0..render_cols)
                    .map(|c| {
                        let cell = grid.get_abs(row0 + r as u32, col0 + c as u16 as u32);
                        let formula = sheet.formula_at(row0 + r as u32, col0 + c as u16 as u32);
                        cell_display(cell, formula)
                    })
                    .collect()
            })
            .collect();

        // 列宽（EA cell 计，封顶 CELL_WIDTH_CAP）。
        let col_widths: Vec<usize> = (0..render_cols)
            .map(|c| {
                cells_text
                    .iter()
                    .map(|row| display_width(&row[c].0))
                    .max()
                    .unwrap_or(0)
                    .min(CELL_WIDTH_CAP)
            })
            .collect();

        // 列字母行（Bold 逐字母——绝对列号 0 基换字母）。
        let max_row_no = row0 + render_rows as u32 + 1; // 1 基末行号
        let gutter = GUTTER_MIN.max(format!("{max_row_no}").chars().count());
        let letters: Vec<String> = (0..render_cols)
            .map(|c| col_letter(col0 + c as u16 as u32))
            .collect();
        let mut letters_line = " ".repeat(gutter + 2);
        let mut letters_spans = Vec::new();
        let mut cursor = letters_line.chars().count();
        for (c, letter) in letters.iter().enumerate() {
            if c > 0 {
                letters_line.push_str(" | ");
                cursor += 3;
            }
            let start = cursor;
            let padded = pad_ea(letter, col_widths[c]);
            letters_line.push_str(&padded);
            cursor += padded.chars().count();
            letters_spans.push((start..start + letter.chars().count(), TokenKind::Bold));
        }
        push(letters_line, letters_spans);

        // 数据行：行号（ListMarker）+ 2 空格 + 单元格 EA 对齐拼接。
        for (r, row) in cells_text.iter().enumerate() {
            let row_no = row0 + r as u32 + 1; // 绝对行号（1 基）
            let no_text = format!("{row_no:>gutter$}");
            let mut line = String::new();
            let mut line_spans = Vec::new();
            line.push_str(&no_text);
            line_spans.push((0..gutter, TokenKind::ListMarker));
            line.push_str("  ");
            let mut cursor = gutter + 2;
            for (c, (cell, formula_fallback)) in row.iter().enumerate() {
                if c > 0 {
                    line.push_str(" | ");
                    cursor += 3;
                }
                let start = cursor;
                line.push_str(cell);
                cursor += cell.chars().count();
                // 公式回退形态（无缓存值 → `=公式`）：Italic 弱化呈现。
                if *formula_fallback && !cell.is_empty() {
                    line_spans.push((start..cursor, TokenKind::Italic));
                }
                let pad = col_widths[c].saturating_sub(display_width(cell));
                let padding = " ".repeat(pad);
                line.push_str(&padding);
                cursor += pad;
            }
            push(line, line_spans);
        }

        // 截断提示（非静默——行模型护栏登记）。
        if truncated {
            let notice = format!(
                "⚠ 表过大——仅渲染前 {render_rows} 行 × {render_cols} 列（全表 {height} × {width}）；完整数据见原始文件"
            );
            let len = notice.chars().count();
            push(notice, vec![(0..len, TokenKind::Quote)]);
        }
    }

    XlsxPayload {
        text: lines.join("\n"),
        spans,
        headings,
        props,
    }
}

/// 单格显示（cell + 公式文本 → (显示文本, 公式回退形态)）。
///
/// - 有值（含公式缓存值）→ [`CellData::display`]（calamine `to_string`
///   逐字同源）；
/// - 无值 + 有公式文本 → `=公式文本`（Italic 呈现——读面公式文本不带
///   `=` 前缀，office-io FIDELITY §1 在档）；
/// - 双无 → 空串。
fn cell_display(cell: Option<&CellData>, formula: Option<&str>) -> (String, bool) {
    match (cell, formula) {
        (Some(CellData::Empty), Some(f)) => (format!("={f}"), true),
        (Some(c), _) => (c.display(), false),
        (None, Some(f)) => (format!("={f}"), true),
        (None, None) => (String::new(), false),
    }
}

/// East Asian 显示宽（CJK 2 cell——与 docx_doc 同口径约定）。
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            let cjk = (0x1100..=0x115F).contains(&cp) // Hangul Jamo
                || (0x2E80..=0xA4CF).contains(&cp)   // CJK 部首..Yi
                || (0xAC00..=0xD7A3).contains(&cp)   // Hangul 音节
                || (0xF900..=0xFAFF).contains(&cp)   // CJK 兼容表意
                || (0xFE30..=0xFE4F).contains(&cp)   // CJK 兼容形式
                || (0xFF00..=0xFF60).contains(&cp)   // 全角形式
                || (0xFFE0..=0xFFE6).contains(&cp); // 全角符号
            if cjk { 2 } else { 1 }
        })
        .sum()
}

/// 尾部补空到显示宽 `w`（EA 感知——多余空格不计列宽漂移）。
fn pad_ea(s: &str, w: usize) -> String {
    let pad = w.saturating_sub(display_width(s));
    format!("{s}{}", " ".repeat(pad))
}

/// 绝对列号（0 基）→ Excel 列字母（A..Z, AA..——标准 base-26）。
fn col_letter(col: u32) -> String {
    let mut n = col;
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII 列字母")
}

// =============================================================================
// 单测（转换规则锚定——写面构格 + 读面 bytes 入口，无盘依赖）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_office_io::io::read_book_from_bytes_with_options;
    use mdc_office_io::write::{CellValue, WorkbookWriter};

    /// 写面构建 bytes → 读面（兼容模式）→ 行模型转换（测试共用道）。
    fn build_convert(
        build: impl FnOnce(&mut WorkbookWriter) -> Result<(), mdc_office_io::write::WriteError>,
    ) -> XlsxPayload {
        let mut w = WorkbookWriter::new();
        build(&mut w).expect("写面构建");
        let bytes = w.save_to_vec().expect("写盘内存");
        let book =
            read_book_from_bytes_with_options(&bytes, &ReadOptions { compat: true }).expect("读回");
        convert(book)
    }

    fn lines(p: &XlsxPayload) -> Vec<&str> {
        p.text.split('\n').collect()
    }

    #[test]
    fn grid_rows_letters_and_absolute_row_numbers() {
        // 写面 row 参数 0 基：起始行 2 → 绝对行号 3..（1 基显示）
        let p = build_convert(|w| {
            let mut s = w.add_sheet("数据")?;
            s.write_row(
                2,
                &[CellValue::Text("月份".into()), CellValue::Number(100.)],
            )?;
            s.write_row(
                3,
                &[CellValue::Text("二月".into()), CellValue::Number(200.5)],
            )?;
            Ok(())
        });
        let ls = lines(&p);
        assert_eq!(ls[0], "── 工作表: 数据 ──", "分隔行");
        // 列字母：起始列 0 → A/B；4 位行号槽 + 2 空格
        assert!(
            ls[1].contains("A") && ls[1].contains("B"),
            "列字母行：{}",
            ls[1]
        );
        assert!(
            ls[1].starts_with("      "),
            "行号槽 4 + 2 空格：{:?}",
            ls[1]
        );
        // 绝对行号 3 / 4（写面 0 基行 2/3 → 1 基 3/4）
        assert!(ls[2].starts_with("   3  "), "行 3：{:?}", ls[2]);
        assert!(ls[3].starts_with("   4  "), "行 4：{:?}", ls[3]);
        assert!(ls[2].contains("月份") && ls[2].contains("100"));
        assert!(ls[3].contains("200.5"), "数值 Rust f64 格式化：{:?}", ls[3]);
        assert_eq!(p.props.sheets, 1);
        assert_eq!(p.props.cells, 4, "非空格计数");
    }

    #[test]
    fn cjk_column_alignment_by_ea_width() {
        // CJK 2 cell：列宽按 EA 对齐——「月份」4 cell 与「abcd」4 cell 同宽
        let p = build_convert(|w| {
            let mut s = w.add_sheet("对齐")?;
            s.write_row(
                0,
                &[
                    CellValue::Text("月份".into()),
                    CellValue::Text("abcd".into()),
                ],
            )?;
            s.write_row(
                1,
                &[CellValue::Text("二月".into()), CellValue::Text("x".into())],
            )?;
            Ok(())
        });
        let ls = lines(&p);
        // 行序：ls[2] = 行 1（月份/abcd），ls[3] = 行 2（二月/x）
        assert!(ls[2].contains("月份 | abcd"), "行 1 内容：{:?}", ls[2]);
        assert!(ls[3].contains("二月 | x"), "行 2 内容：{:?}", ls[3]);
        assert!(ls[3].contains("x   "), "窄值尾部补齐列宽：{:?}", ls[3]);
        // EA 对齐实证：两行 ` | ` 管道 char 位一致（CJK 2 cell——否则
        // 「月份」按 2 宽补空，行 2 管道位左移）
        assert_eq!(
            ls[2].find('|'),
            ls[3].find('|'),
            "管道位对齐：{:?} vs {:?}",
            ls[2],
            ls[3]
        );
    }

    #[test]
    fn formula_cell_shows_cached_value() {
        // 公式格带缓存值 → 显示缓存值（不显示公式文本）
        let p = build_convert(|w| {
            let mut s = w.add_sheet("公式")?;
            s.write_row(
                0,
                &[
                    CellValue::Number(1.),
                    CellValue::Number(2.),
                    CellValue::Formula {
                        text: "SUM(A1:B1)".into(),
                        cached: Some(mdc_office_io::write::FormulaCache::Number(3.)),
                    },
                ],
            )?;
            Ok(())
        });
        let ls = lines(&p);
        assert!(ls[2].contains("3"), "缓存值显示：{:?}", ls[2]);
        assert!(!ls[2].contains("SUM"), "不显示公式文本：{:?}", ls[2]);
        assert_eq!(p.props.formulas, 1, "公式格计数");
        // Italic 回退 span 不出现（有缓存值）
        assert!(
            !p.spans
                .iter()
                .flatten()
                .any(|(_, k)| *k == TokenKind::Italic),
            "缓存值形态无 Italic span"
        );
    }

    #[test]
    fn multi_sheet_tree_and_blank_separator() {
        let p = build_convert(|w| {
            let mut a = w.add_sheet("甲")?;
            a.write_row(0, &[CellValue::Text("a1".into())])?;
            let mut b = w.add_sheet("乙")?;
            b.write_row(0, &[CellValue::Text("b1".into())])?;
            Ok(())
        });
        let ls = lines(&p);
        assert_eq!(p.headings.len(), 2, "工作表树两表");
        assert_eq!(p.headings[0].text, "甲");
        assert_eq!(p.headings[0].level, 1);
        assert_eq!(ls[p.headings[0].line], "── 工作表: 甲 ──", "树锚点=分隔行");
        assert_eq!(ls[p.headings[1].line], "── 工作表: 乙 ──");
        // 表间一空行
        let between = p.headings[1].line - 1;
        assert_eq!(ls[between], "", "表间空行");
    }

    #[test]
    fn empty_sheet_renders_notice() {
        let p = build_convert(|w| {
            w.add_sheet("空表")?;
            Ok(())
        });
        let ls = lines(&p);
        assert_eq!(ls[0], "── 工作表: 空表 ──");
        assert_eq!(ls[1], "（空表——无数据格）", "空表提示行");
        assert_eq!(p.props.cells, 0);
        assert_eq!(p.headings.len(), 1, "空表仍入树");
    }

    #[test]
    fn oversize_rows_truncate_with_explicit_notice() {
        let p = build_convert(|w| {
            let mut s = w.add_sheet("巨表")?;
            for r in 0..(MAX_RENDER_ROWS + 5) {
                s.write_row(r as u32, &[CellValue::Number(r as f64)])?;
            }
            Ok(())
        });
        let ls = lines(&p);
        assert_eq!(p.props.truncated_sheets, 1, "护栏登记");
        assert_eq!(p.props.cells, MAX_RENDER_ROWS + 5, "属性面计全量非空格");
        let notice = ls
            .iter()
            .find(|l| l.contains("⚠ 表过大"))
            .expect("截断提示行");
        assert!(notice.contains("1000 行"), "渲染上限标注：{notice}");
        // 渲染数据行恰为上限（行号槽以数字开头——分隔/字母/提示行排除）
        let data_rows = ls
            .iter()
            .filter(|l| {
                l.trim_start()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            })
            .count();
        assert_eq!(data_rows, MAX_RENDER_ROWS, "渲染行数恰为上限");
    }

    #[test]
    fn cell_display_formula_fallback_pure() {
        // Empty + 公式文本 → `=公式`（Italic 形态标记）
        let (t, fb) = cell_display(Some(&CellData::Empty), Some("SUM(A1:A2)"));
        assert_eq!(t, "=SUM(A1:A2)");
        assert!(fb);
        // 有值 → display（含公式缓存值——数值 Rust 格式化）
        let (t2, fb2) = cell_display(Some(&CellData::Number(3.5)), Some("SUM(A1:A2)"));
        assert_eq!(t2, "3.5");
        assert!(!fb2);
        // 双无 → 空串
        let (t3, _) = cell_display(None, None);
        assert_eq!(t3, "");
        // 边界外坐标（None cell）+ 公式文本 → 同回退（防御性承载）
        let (t4, fb4) = cell_display(None, Some("A1+1"));
        assert_eq!(t4, "=A1+1");
        assert!(fb4);
    }

    #[test]
    fn col_letter_sequence_and_wrap() {
        assert_eq!(col_letter(0), "A");
        assert_eq!(col_letter(25), "Z");
        assert_eq!(col_letter(26), "AA");
        assert_eq!(col_letter(27), "AB");
        assert_eq!(col_letter(52), "BA");
    }

    #[test]
    fn ea_width_cjk_two_cells() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("１２"), 4, "全角 2 cell");
    }

    // ---------------- 烟雾夹具生成器（不随常规测试跑） ----------------

    /// 烟雾 GUI 夹具生成器（写面构建 → tests/fixtures/smoke.xlsx）：
    ///
    /// ```bash
    /// cargo test gen_smoke_xlsx -- --ignored
    /// ./smoke.sh 6 tests/fixtures/smoke.xlsx
    /// ```
    #[test]
    #[ignore = "烟雾夹具生成器（写 tests/fixtures/smoke.xlsx）"]
    fn gen_smoke_xlsx() {
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet("销售明细").expect("表 1");
        s.write_row(
            0,
            &[
                CellValue::Text("月份".into()),
                CellValue::Text("销量".into()),
                CellValue::Text("备注".into()),
            ],
        )
        .unwrap();
        s.write_row(
            1,
            &[
                CellValue::Text("一月".into()),
                CellValue::Number(100.),
                CellValue::Text("春节备货".into()),
            ],
        )
        .unwrap();
        s.write_row(
            2,
            &[
                CellValue::Text("二月".into()),
                CellValue::Number(200.5),
                CellValue::Text("".into()),
            ],
        )
        .unwrap();
        s.write_row(
            3,
            &[
                CellValue::Text("三月".into()),
                CellValue::Formula {
                    text: "SUM(B2:B3)".into(),
                    cached: Some(mdc_office_io::write::FormulaCache::Number(300.5)),
                },
                CellValue::Text("季度汇总".into()),
            ],
        )
        .unwrap();
        s.merge(4, 1, 4, 2, "外部资料区", None).expect("合并");
        s.write_hyperlink(
            4,
            0,
            mdc_office_io::write::HyperlinkSpec {
                url: "https://example.com/report".to_string(),
                display_text: Some("季度报表门户".to_string()),
                tooltip: Some("外部链接".to_string()),
            },
        )
        .expect("超链接");
        let mut t = w.add_sheet("参数").expect("表 2");
        t.write_row(
            0,
            &[CellValue::Text("税率".into()), CellValue::Number(0.13)],
        )
        .unwrap();
        t.write_row(
            1,
            &[CellValue::Text("启用".into()), CellValue::Boolean(true)],
        )
        .unwrap();

        let bytes = w.save_to_vec().expect("写盘内存");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/smoke.xlsx");
        std::fs::write(path, &bytes).expect("落盘夹具");
        // 落盘件自校验：读回 + 转换可过。
        let payload = load(std::path::Path::new(path)).expect("夹具读回");
        assert_eq!(payload.headings.len(), 2, "两表树");
        assert_eq!(payload.props.sheets, 2);
        assert_eq!(payload.props.formulas, 1, "一格公式");
        assert!(payload.props.merges >= 1, "合并范围");
        assert!(payload.props.hyperlinks >= 1, "超链接");
        eprintln!(
            "xlsx 夹具已生成：{path}（{} 字节，{} 行，{} 表）",
            bytes.len(),
            payload.text.split('\n').count(),
            payload.props.sheets
        );
    }
}
