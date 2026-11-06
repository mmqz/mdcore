//! #283 产品导出编排测试（export_edited——T8/X-2 接线 + X-1 重算 +
//! P2-12 诚实账本）。
//!
//! 覆盖（对照 issue #283 建议三条）：
//! - **X-2 接线**：in-place 优先路径——结构保真实证（样式/合并/
//!   超链接部件透传）+ 非手术部件逐字节恒等 + 零编辑恒等重打包；
//! - **X-1 重算**：改值 → 依赖公式两级链缓存重算回写 + `<f>` 原文
//!   零接触；引擎不可用 → 降级保真直通（源缓存保留 + 诚实登记）；
//! - **P2-12 账本**：rebuild 兜底路径的结构性丢失（图表/条件格式/
//!   样式 xf）在场才登记；
//! - 兜底分派：公式格编辑/empty 编辑 → rebuild + 回退原因可追溯。
//!
//! 夹具纪律：与 rebuild_recalc.rs 同源（WorkbookWriter 合成簿——
//! 真实生成器面）；非法表名档 = 手工 zip（写面 add_sheet 校验会拒
//! 该名，读面/编辑面不校验——引擎重算面才拒）。

use std::io::Write as _;

use mdc_office_io::io::{read_book_from_bytes, Book, CellData};
use mdc_office_io::rebuild::{export_edited, CellEdit, EditValue, ExportPath, ExportStats};
use mdc_office_io::write::{CellValue, FormulaCache, HyperlinkSpec, WorkbookWriter};
use mdc_office_io::xlsx_chart::{ChartKind, ChartSpec};
use mdc_office_io::xlsx_condfmt::CondFormat;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

fn edit(sheet: usize, row: u32, col: u32, value: EditValue) -> CellEdit {
    CellEdit {
        sheet,
        row,
        col,
        value,
    }
}

/// 富结构簿：样式表头 + 合并 + 超链接 + 数值（无公式——纯结构面）。
fn structured_book() -> Vec<u8> {
    use mdc_office_io::write::{Align, Border, CellStyle, NumFmt};
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("销售").unwrap();
    let header = CellStyle {
        bold: true,
        font_color: Some("#9C0006".into()),
        fill_color: Some("#FFC7CE".into()),
        border: Border::Thin,
        num_fmt: Some(NumFmt::CurrencyYen),
        align: Align::Center,
    };
    s.write(0, 0, CellValue::Text("名称".into()), Some(&header))
        .unwrap();
    s.write(0, 1, CellValue::Text("数量".into()), Some(&header))
        .unwrap();
    s.write(1, 0, CellValue::Text("甲".into()), None).unwrap();
    s.write(1, 1, CellValue::Number(12.5), None).unwrap();
    s.merge(2, 0, 2, 1, "合计区域", None).unwrap();
    s.write_hyperlink(
        3,
        0,
        HyperlinkSpec {
            url: "https://example.com/a".into(),
            display_text: Some("链接文字".into()),
            tooltip: Some("提示".into()),
        },
    )
    .unwrap();
    w.save_to_vec().unwrap()
}

/// 公式簿：A1=10（值），B1==A1*2（缓存 20），C1==B1+1（缓存 21）
/// ——两级依赖链（rebuild_recalc.rs 同源）。
fn formula_book() -> Vec<u8> {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("data").unwrap();
    s.write(0, 0, CellValue::Number(10.0), None).unwrap();
    s.write(
        0,
        1,
        CellValue::Formula {
            text: "=A1*2".into(),
            cached: Some(FormulaCache::Number(20.0)),
        },
        None,
    )
    .unwrap();
    s.write(
        0,
        2,
        CellValue::Formula {
            text: "=B1+1".into(),
            cached: Some(FormulaCache::Number(21.0)),
        },
        None,
    )
    .unwrap();
    w.save_to_vec().unwrap()
}

/// 手工 zip：非法表名 + 公式（写面拒建、读面容忍、重算面拒——
/// in-place 降级纪律的专用载体）。
fn illegal_name_formula_book() -> Vec<u8> {
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>";
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
    let workbook = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Bad[Name*\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>";
    let sheet = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"1\"><c r=\"A1\"><v>5</v></c><c r=\"B1\"><f>A1*2</f><v>10</v></c></row></sheetData></worksheet>";

    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", root_rels),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", sheet),
    ] {
        w.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut e = ar.by_name(name).unwrap();
    let mut v = vec![];
    std::io::Read::read_to_end(&mut e, &mut v).unwrap();
    v
}

fn all_part_names(bytes: &[u8]) -> Vec<String> {
    let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..ar.len())
        .map(|i| ar.by_index_raw(i).unwrap().name().to_string())
        .collect()
}

fn book_of(bytes: &[u8]) -> Book {
    read_book_from_bytes(bytes).unwrap()
}

// ---------------------------------------------------------------------------
// X-2 接线：in-place 优先路径
// ---------------------------------------------------------------------------

#[test]
fn in_place_path_preserves_structure_and_parts() {
    let bytes = structured_book();
    let book = book_of(&bytes);
    let edits = vec![
        edit(0, 1, 1, EditValue::Number(99.5)),
        edit(0, 1, 0, EditValue::Text("乙".into())),
    ];
    let (out, stats) = export_edited(&book, &bytes, &edits).unwrap();

    assert_eq!(stats.path, ExportPath::InPlace, "路径分派: {stats:?}");
    assert_eq!(stats.edits_applied, 2);
    assert!(stats.skipped.is_empty(), "零诚实账本: {stats:?}");
    assert!(stats.fallback_reasons.is_empty());
    assert_eq!(stats.sheets, 1);
    assert_eq!(stats.merges, 1, "合并计数（透传口径）");
    assert_eq!(stats.hyperlinks, 1, "超链接计数（透传口径）");

    // 结构保真实证：非手术部件逐字节恒等（styles/drawing rels/docProps）。
    let src_names = all_part_names(&bytes);
    assert_eq!(src_names, all_part_names(&out), "部件清单不变");
    for n in &src_names {
        if n == "xl/worksheets/sheet1.xml" {
            continue;
        }
        assert_eq!(
            part_bytes(&bytes, n),
            part_bytes(&out, n),
            "非手术部件必须逐字节恒等: {n}"
        );
    }

    // 读回：值在位 + 合并/超链接保真（对照 rebuild 路径丢样式的
    // 有损重建——本路径结构性面零损失）。
    let back = read_book_from_bytes(&out).unwrap();
    let d = &back.sheets[0];
    assert_eq!(d.typed.get_abs(1, 1), Some(&CellData::Number(99.5)));
    assert_eq!(d.typed.get_abs(1, 0), Some(&CellData::Text("乙".into())));
    assert_eq!(d.merges.len(), 1, "合并保真");
    assert_eq!(d.hyperlinks.len(), 1, "超链接保真");
}

#[test]
fn in_place_path_recalc_updates_formula_caches() {
    let bytes = formula_book();
    let book = book_of(&bytes);
    let edits = vec![edit(0, 0, 0, EditValue::Number(30.0))];
    let (out, stats) = export_edited(&book, &bytes, &edits).unwrap();

    assert_eq!(stats.path, ExportPath::InPlace, "路径分派: {stats:?}");
    assert_eq!(stats.recalc_cells, 2, "两级链全重算: {stats:?}");
    assert!(stats.skipped.is_empty(), "无降级登记: {stats:?}");

    let back = read_book_from_bytes(&out).unwrap();
    let d = &back.sheets[0];
    assert_eq!(d.typed.get_abs(0, 0), Some(&CellData::Number(30.0)));
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(60.0)),
        "B1 缓存重算（=A1*2 → 60）"
    );
    assert_eq!(
        d.typed.get_abs(0, 2),
        Some(&CellData::Number(61.0)),
        "C1 级联重算（=B1+1 → 61）"
    );
    assert_eq!(
        d.formula_at(0, 1).map(str::to_string),
        Some("A1*2".into()),
        "公式文本透传"
    );
    // 手术部件实证：<f> 原文 + <v> 更新（两级链）。
    let sheet = String::from_utf8(part_bytes(&out, "xl/worksheets/sheet1.xml")).unwrap();
    assert!(sheet.contains("<f>A1*2</f><v>60</v>"), "{sheet}");
    assert!(sheet.contains("<f>B1+1</f><v>61</v>"), "{sheet}");
}

#[test]
fn in_place_zero_edits_is_identity_repack() {
    let bytes = structured_book();
    let book = book_of(&bytes);
    let (out, stats) = export_edited(&book, &bytes, &[]).unwrap();

    assert_eq!(stats.path, ExportPath::InPlace, "零编辑 = 恒等重打包");
    assert_eq!(stats.edits_applied, 0);
    assert_eq!(stats.cells_written, 0);
    assert!(stats.skipped.is_empty());
    // 全部件（含 sheet1）逐字节恒等。
    for n in all_part_names(&bytes) {
        assert_eq!(part_bytes(&bytes, &n), part_bytes(&out, &n), "恒等: {n}");
    }
}

#[test]
fn in_place_recalc_degrades_to_cache_passthrough() {
    let bytes = illegal_name_formula_book();
    let book = book_of(&bytes);
    assert_eq!(book.sheets[0].name, "Bad[Name*", "读面容忍非法表名");

    let edits = vec![edit(0, 0, 0, EditValue::Number(30.0))];
    let (out, stats) = export_edited(&book, &bytes, &edits).unwrap();

    // 引擎表名校验拒 → 整簿降级保真直通（源缓存保留）+ 诚实登记。
    assert_eq!(stats.path, ExportPath::InPlace, "in-place 不因重算失败回退");
    assert_eq!(stats.recalc_cells, 0);
    assert!(
        stats
            .skipped
            .iter()
            .any(|n| n.starts_with("recalc unavailable (formula caches passthrough)")),
        "降级登记: {stats:?}"
    );

    let back = read_book_from_bytes(&out).unwrap();
    let d = &back.sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Number(30.0)),
        "值编辑落格"
    );
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(10.0)),
        "公式缓存保真直通（源 10 保留——非重算新值 60）"
    );
    assert_eq!(
        d.formula_at(0, 1).map(str::to_string),
        Some("A1*2".into()),
        "公式文本透传"
    );
}

// ---------------------------------------------------------------------------
// rebuild 兜底路径（拒改面分派 + P2-12 结构性丢失账本）
// ---------------------------------------------------------------------------

#[test]
fn rebuild_fallback_on_formula_cell_edit() {
    let bytes = formula_book();
    let book = book_of(&bytes);
    // 编辑公式格 B1（in-place 拒改面）→ rebuild 兜底（值覆写公式）。
    let edits = vec![edit(0, 0, 1, EditValue::Number(45.0))];
    let (out, stats) = export_edited(&book, &bytes, &edits).unwrap();

    assert_eq!(stats.path, ExportPath::Rebuild, "路径分派: {stats:?}");
    assert!(
        stats
            .fallback_reasons
            .iter()
            .any(|r| r.contains("refused by in-place engine")),
        "回退原因可追溯: {stats:?}"
    );

    let back = read_book_from_bytes(&out).unwrap();
    let d = &back.sheets[0];
    assert_eq!(d.typed.get_abs(0, 1), Some(&CellData::Number(45.0)));
    assert!(
        d.formula_at(0, 1).is_none(),
        "公式被值覆写（rebuild 编辑语义）"
    );
    // C1 公式随 rebuild 重算（=B1+1 → 46——编辑即落位全簿重算）。
    assert_eq!(
        d.typed.get_abs(0, 2),
        Some(&CellData::Number(46.0)),
        "依赖公式重算"
    );
}

#[test]
fn rebuild_fallback_on_empty_edit() {
    let bytes = formula_book();
    let book = book_of(&bytes);
    let edits = vec![edit(0, 0, 0, EditValue::Empty)];
    let (out, stats) = export_edited(&book, &bytes, &edits).unwrap();

    assert_eq!(stats.path, ExportPath::Rebuild, "路径分派: {stats:?}");
    assert!(
        stats
            .fallback_reasons
            .iter()
            .any(|r| r.contains("kind empty/formula")),
        "回退原因登记: {stats:?}"
    );
    let back = read_book_from_bytes(&out).unwrap();
    let d = &back.sheets[0];
    assert_ne!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Number(10.0)),
        "清格生效"
    );
    // 依赖公式重算：A1 空 → B1 = 0*2 = 0。
    assert_eq!(d.typed.get_abs(0, 1), Some(&CellData::Number(0.0)));
}

#[test]
fn rebuild_registers_structural_loss_notes() {
    // 图表 + 条件格式 + 样式（三类结构性面在场）。
    use mdc_office_io::write::{Align, Border, CellStyle, NumFmt};
    let mut w = WorkbookWriter::new();
    {
        let mut s = w.add_sheet("Sheet1").unwrap();
        let header = CellStyle {
            bold: true,
            font_color: Some("#9C0006".into()),
            fill_color: Some("#FFC7CE".into()),
            border: Border::Thin,
            num_fmt: Some(NumFmt::CurrencyYen),
            align: Align::Center,
        };
        s.write(0, 0, CellValue::Text("品名".into()), Some(&header))
            .unwrap();
        s.write(1, 0, CellValue::Text("甲".into()), None).unwrap();
        s.write(1, 1, CellValue::Number(30.0), None).unwrap();
        s.write(1, 2, CellValue::Number(40.0), None).unwrap();
        s.add_cond_format(
            "B1:B4",
            CondFormat::DataBar {
                min: 0.0,
                max: 100.0,
                color: "#638EC6".into(),
            },
        )
        .unwrap();
        s.add_chart(ChartSpec {
            kind: ChartKind::Bar {
                direction: mdc_office_io::xlsx_chart::BarDirection::Vertical,
                stacked: false,
            },
            title: "对比".into(),
            data_range: "Sheet1!A1:C2".into(),
            series_in_columns: true,
            position: (4, 0),
            size: (600_000, 400_000),
        })
        .unwrap();
    }
    let bytes = w.save_to_vec().unwrap();
    let book = book_of(&bytes);

    // Empty 编辑强制 rebuild 路径 → 结构性丢失逐类登记。
    let edits = vec![edit(0, 1, 2, EditValue::Empty)];
    let (out, stats) = export_edited(&book, &bytes, &edits).unwrap();
    assert_eq!(stats.path, ExportPath::Rebuild);
    let joined = stats.skipped.join(" | ");
    assert!(
        joined.contains("charts present in source (1 chart part(s))"),
        "图表丢失登记: {joined}"
    );
    assert!(
        joined.contains("conditional formatting present in source (1 sheet(s))"),
        "条件格式丢失登记: {joined}"
    );
    assert!(
        joined.contains("cell style xfs present in source"),
        "样式丢失登记: {joined}"
    );
    assert!(
        joined.contains("images/drawings present in source"),
        "drawing（图表锚）丢失登记: {joined}"
    );
    // 登记的丢失面在产物中真实不在（图表部件缺席——诚实账本非虚言）。
    assert!(
        !all_part_names(&out)
            .iter()
            .any(|n| n.starts_with("xl/charts/")),
        "产物无图表部件（登记如实）"
    );
}

#[test]
fn in_place_path_leaves_structural_loss_ledger_empty() {
    // 同一富结构簿（图表+条件格式），值编辑 → in-place 路径：结构
    // 保真 + 零账本（对照 rebuild 的四类登记——分派价值主张）。
    let mut w = WorkbookWriter::new();
    {
        let mut s = w.add_sheet("Sheet1").unwrap();
        s.write(0, 0, CellValue::Text("品名".into()), None).unwrap();
        s.write(1, 0, CellValue::Text("甲".into()), None).unwrap();
        s.write(1, 1, CellValue::Number(30.0), None).unwrap();
        s.add_cond_format(
            "B1:B4",
            CondFormat::DataBar {
                min: 0.0,
                max: 100.0,
                color: "#638EC6".into(),
            },
        )
        .unwrap();
    }
    let bytes = w.save_to_vec().unwrap();
    let book = book_of(&bytes);
    let edits = vec![edit(0, 1, 1, EditValue::Number(31.0))];
    let (out, stats) = export_edited(&book, &bytes, &edits).unwrap();

    assert_eq!(stats.path, ExportPath::InPlace, "富结构簿走 in-place");
    assert!(stats.skipped.is_empty(), "零诚实账本: {stats:?}");
    assert!(
        all_part_names(&out)
            .iter()
            .any(|n| n.starts_with("xl/worksheets")),
        "sheet 部件在位"
    );
    // 条件格式部件（worksheet 内联）手术透传：产物 sheet 部件含
    // conditionalFormatting（对照 rebuild 路径的丢失）。
    let sheet = String::from_utf8(part_bytes(&out, "xl/worksheets/sheet1.xml")).unwrap();
    assert!(
        sheet.contains("<conditionalFormatting"),
        "条件格式透传（in-place 保真）: {sheet}"
    );
}

// ---------------------------------------------------------------------------
// 错误面（两路径共享的编辑账本防线）
// ---------------------------------------------------------------------------

#[test]
fn export_edited_rejects_out_of_range_sheet_edit() {
    let bytes = structured_book();
    let book = book_of(&bytes);
    let edits = vec![edit(5, 0, 0, EditValue::Number(1.0))];
    match export_edited(&book, &bytes, &edits) {
        Err(mdc_office_io::write::WriteError::InvalidInput(msg)) => {
            assert!(msg.contains("sheet index 5"), "消息含索引: {msg}");
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

/// 统计形态完整性（wasm JSON 面的前置：六键 + path/recalc/
/// fallback 恒在场——web 测试 grid_export_edits 键断言的引擎侧锚）。
#[test]
fn export_stats_shape_complete_on_both_paths() {
    let bytes = structured_book();
    let book = book_of(&bytes);

    let (_, s_in) = export_edited(&book, &bytes, &[edit(0, 1, 1, EditValue::Number(1.0))]).unwrap();
    assert_eq!(s_in.path, ExportPath::InPlace);
    assert!(s_in.fallback_reasons.is_empty());

    let (_, s_re) = export_edited(&book, &bytes, &[edit(0, 1, 1, EditValue::Empty)]).unwrap();
    assert_eq!(s_re.path, ExportPath::Rebuild);
    assert!(!s_re.fallback_reasons.is_empty());

    // 公共键恒在场（结构性字段全——serde/JSON 消费方无分岔）。
    let _ = (
        s_in.sheets,
        s_in.cells_written,
        s_in.merges,
        s_in.hyperlinks,
        s_in.edits_applied,
        s_in.recalc_cells,
        s_in.skipped,
    );
    let _ = (
        s_re.sheets,
        s_re.cells_written,
        s_re.merges,
        s_re.hyperlinks,
        s_re.edits_applied,
        s_re.recalc_cells,
        s_re.skipped,
    );
}

// 未使用符号防误删（ExportStats 全字段在上方被消费）。
#[allow(dead_code)]
fn _stats_type_witness(s: &ExportStats) -> usize {
    s.sheets + s.merges + s.hyperlinks + s.edits_applied + s.recalc_cells + s.skipped.len()
}
