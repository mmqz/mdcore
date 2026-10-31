//! 重算接入（issue #113 任务 1）——行为锚点测试。
//!
//! 覆盖（对照 issue 验收清单）：
//! - 改格后依赖公式级联更新（两级链）+ 默认开关 + 关闭对照；
//! - 外部引用保留源缓存 + skipped 登记（依赖者以缓存常量级联）；
//! - 循环引用 → `#CIRC!` 缓存（引擎 CIRC 检测——不死循环）；
//! - 编辑公式格（`EditValue::Formula`：窗内覆盖 + 扩展区新格）；
//! - 引擎构建失败 → 整簿降级保真直通（非法表名档）；
//! - 无缓存公式格：重算补值 / 非重算维持历史行为（读回 0 两义性）；
//! - 共享公式（calamine 从格位移展开——探针钉定）逐格独立重算；
//! - 数组公式（单格 ref 形态——探针钉定等同普通公式）；
//! - 错误常量格排除灌入 + 登记（引用它的公式按空格语义：实测
//!   `=E1+1` → 1——Excel 语义为错误传播，能力边界如实断言）；
//! - LibreOffice 显示值对照（环境门——soffice 缺失 SKIP）。
//!
//! 行为依据：tests/rebuild_probe.rs 探针（已删，结论沉淀于本文件
//! 断言与 rebuild.rs 模块文档）——calamine shared 从格展开 / 纯
//! 公式格 Empty 占位进 typed 网 / 引擎 new 不首算（#112 文档缺陷，
//! rebuild 侧显式 recalculate 补偿）。

use std::io::Write as _;
use std::process::Command;

use mdc_office_io::io::{read_book_from_bytes, CellData};
use mdc_office_io::rebuild::{
    rebuild_book, rebuild_book_with, CellEdit, EditValue, RebuildOptions,
};
use mdc_office_io::write::{CellValue, FormulaCache, WorkbookWriter};

// ---------------------------------------------------------------------------
// 源档构造
// ---------------------------------------------------------------------------

/// 标准公式簿：A1=10（值），B1==A1*2（缓存 20），C1==B1+1（缓存 21）
/// ——两级依赖链的最小载体。
fn formula_workbook() -> WorkbookWriter {
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
    w
}

fn edit_a1(v: f64) -> CellEdit {
    CellEdit {
        sheet: 0,
        row: 0,
        col: 0,
        value: EditValue::Number(v),
    }
}

/// 重建 → 读回。
fn rebuild_read(book: &mdc_office_io::io::Book, edits: &[CellEdit]) -> mdc_office_io::io::Book {
    let (bytes, _) = rebuild_book(book, edits).unwrap();
    read_book_from_bytes(&bytes).unwrap()
}

// ---------------------------------------------------------------------------
// 手构造最小 xlsx（精确控制 <f> 形态——shared/array/无缓存/外部引用/
// 错误常量；calamine 读面行为探针已钉定）
// ---------------------------------------------------------------------------

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn sheet(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{NS_MAIN}"><sheetData>{body}</sheetData></worksheet>"#
    )
}

fn build_xlsx(sheet_body: &str) -> Vec<u8> {
    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{NS_MAIN}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let wb_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let opts = zip::write::SimpleFileOptions::default();
    for (name, data) in [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", root_rels),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", wb_rels),
        ("xl/worksheets/sheet1.xml", &sheet(sheet_body)),
    ] {
        zip.start_file::<_, ()>(name, opts).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

// ---------------------------------------------------------------------------
// 验收 1：改 A1，依赖它的公式全部更新（两级链）
// ---------------------------------------------------------------------------

#[test]
fn recalc_updates_dependents() {
    let bytes0 = formula_workbook().save_to_vec().unwrap();
    let book = read_book_from_bytes(&bytes0).unwrap();

    let (bytes, stats) = rebuild_book(&book, &[edit_a1(15.0)]).unwrap();
    assert_eq!(stats.recalc_cells, 2, "两级链 2 公式格重算");

    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Number(15.0)),
        "编辑格"
    );
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(30.0)),
        "一级依赖 B1=A1*2 → 30"
    );
    assert_eq!(
        d.typed.get_abs(0, 2),
        Some(&CellData::Number(31.0)),
        "二级依赖 C1=B1+1 → 31（级联）"
    );
    // 公式文本保持（重算只换缓存，不动文本）。
    assert_eq!(d.formula_at(0, 1).map(str::to_string), Some("A1*2".into()));
    assert_eq!(d.formula_at(0, 2).map(str::to_string), Some("B1+1".into()));
}

#[test]
fn recalc_disabled_keeps_source_cache() {
    let bytes0 = formula_workbook().save_to_vec().unwrap();
    let book = read_book_from_bytes(&bytes0).unwrap();

    let (bytes, stats) = rebuild_book_with(
        &book,
        &[edit_a1(15.0)],
        &RebuildOptions { recalculate: false },
    )
    .unwrap();
    assert_eq!(stats.recalc_cells, 0, "关闭重算");

    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(d.typed.get_abs(0, 0), Some(&CellData::Number(15.0)));
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(20.0)),
        "B1 源缓存保持（历史行为）"
    );
    assert_eq!(d.typed.get_abs(0, 2), Some(&CellData::Number(21.0)));
}

#[test]
fn recalc_default_is_on() {
    // rebuild_book 兼容入口默认重算（issue RebuildOptions 草图默认
    // true——语义变更的显式钉定）。
    let bytes0 = formula_workbook().save_to_vec().unwrap();
    let book = read_book_from_bytes(&bytes0).unwrap();
    let d = &rebuild_read(&book, &[edit_a1(15.0)]).sheets[0];
    assert_eq!(d.typed.get_abs(0, 1), Some(&CellData::Number(30.0)));
}

// ---------------------------------------------------------------------------
// 验收 5：外部引用保留缓存 + 警告（依赖者以缓存常量级联）
// ---------------------------------------------------------------------------

#[test]
fn recalc_external_ref_keeps_cache_and_registers() {
    // D1 = [Book1]Sheet1!A1（缓存 99）；G1 = =D1*3（缓存 297）。
    let bytes0 = build_xlsx(
        r#"<row r="1"><c r="A1"><v>5</v></c><c r="D1"><f>[Book1]Sheet1!A1</f><v>99</v></c><c r="G1"><f>D1*3</f><v>297</v></c></row>"#,
    );
    let book = read_book_from_bytes(&bytes0).unwrap();
    let s = &book.sheets[0];
    assert_eq!(s.formula_at(0, 3), Some("[Book1]Sheet1!A1"), "外部引用读面");
    assert_eq!(s.typed.get_abs(0, 3), Some(&CellData::Number(99.0)));

    // 编辑 G1 为新公式 =D1*4 → 以 D1 缓存常量 99 级联 → 396。
    let edits = vec![CellEdit {
        sheet: 0,
        row: 0,
        col: 6,
        value: EditValue::Formula("=D1*4".into()),
    }];
    let (bytes, stats) = rebuild_book(&book, &edits).unwrap();
    assert!(
        stats
            .skipped
            .iter()
            .any(|n| n.contains("external-reference") && n.contains("1")),
        "外部引用登记：{:?}",
        stats.skipped
    );

    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 3),
        Some(&CellData::Number(99.0)),
        "外部引用格源缓存保留（不被 #ERROR! 覆盖）"
    );
    assert_eq!(
        d.formula_at(0, 3).map(str::to_string),
        Some("[Book1]Sheet1!A1".into()),
        "外部引用公式文本保持"
    );
    assert_eq!(
        d.typed.get_abs(0, 6),
        Some(&CellData::Number(396.0)),
        "依赖者以缓存常量级联（99*4）"
    );
}

// ---------------------------------------------------------------------------
// 验收 4c：循环引用 → #CIRC!（不死循环）
// ---------------------------------------------------------------------------

#[test]
fn recalc_circular_reference_circ_code() {
    let bytes0 = build_xlsx(
        r#"<row r="1"><c r="A1"><f>B1</f><v>0</v></c><c r="B1"><f>A1</f><v>0</v></c></row>"#,
    );
    let book = read_book_from_bytes(&bytes0).unwrap();

    // 测试完成本身 = 未死循环（引擎 CIRC 检测）。
    let (bytes, _) = rebuild_book(&book, &[]).unwrap();
    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Text("#CIRC!".into())),
        "循环引用 → CIRC 码文本缓存（引擎扩展码如实落盘）"
    );
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Text("#CIRC!".into()))
    );
}

// ---------------------------------------------------------------------------
// 编辑公式格（EditValue::Formula）
// ---------------------------------------------------------------------------

#[test]
fn recalc_edit_formula_cell() {
    let bytes0 = formula_workbook().save_to_vec().unwrap();
    let book = read_book_from_bytes(&bytes0).unwrap();

    // 窗内：值格 A1 覆盖为公式；扩展区：新公式格。
    let edits = vec![
        CellEdit {
            sheet: 0,
            row: 0,
            col: 0,
            value: EditValue::Number(7.0),
        },
        CellEdit {
            sheet: 0,
            row: 3,
            col: 0,
            value: EditValue::Formula("A1*10".into()), // 不带 = 前缀形态
        },
    ];
    let (bytes, stats) = rebuild_book(&book, &edits).unwrap();
    assert_eq!(stats.recalc_cells, 3, "2 源公式 + 1 编辑公式");

    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.formula_at(3, 0).map(str::to_string),
        Some("A1*10".into()),
        "扩展区新公式格（无 = 前缀编辑形态——写面规范化）"
    );
    assert_eq!(
        d.typed.get_abs(3, 0),
        Some(&CellData::Number(70.0)),
        "新公式构建即首算得缓存（A1=7 → 70）"
    );
    // 源公式链以新 A1 重算。
    assert_eq!(d.typed.get_abs(0, 1), Some(&CellData::Number(14.0)));
    assert_eq!(d.typed.get_abs(0, 2), Some(&CellData::Number(15.0)));
}

#[test]
fn recalc_formula_edit_overwrites_formula() {
    // 公式格被编辑为新公式：旧公式替换（非叠加）。
    let bytes0 = formula_workbook().save_to_vec().unwrap();
    let book = read_book_from_bytes(&bytes0).unwrap();
    let edits = vec![CellEdit {
        sheet: 0,
        row: 0,
        col: 1,
        value: EditValue::Formula("=A1+100".into()),
    }];
    let d = &rebuild_read(&book, &edits).sheets[0];
    assert_eq!(
        d.formula_at(0, 1).map(str::to_string),
        Some("A1+100".into()),
        "公式覆盖公式"
    );
    assert_eq!(d.typed.get_abs(0, 1), Some(&CellData::Number(110.0)));
    assert_eq!(
        d.typed.get_abs(0, 2),
        Some(&CellData::Number(111.0)),
        "下游以新公式结果级联"
    );
}

#[test]
fn recalc_rejects_noncharacter_formula_edit() {
    // 公式编辑同样过 XML 非字符防线（U+FFFE/U+FFFF typed 拒）。
    let bytes0 = formula_workbook().save_to_vec().unwrap();
    let book = read_book_from_bytes(&bytes0).unwrap();
    let edits = vec![CellEdit {
        sheet: 0,
        row: 0,
        col: 0,
        value: EditValue::Formula("=\u{FFFF}".into()),
    }];
    match rebuild_book(&book, &edits) {
        Err(mdc_office_io::write::WriteError::InvalidInput(m)) => {
            assert!(m.contains("U+FFFF"), "消息含码点：{m}")
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 引擎构建失败 → 整簿降级保真直通
// ---------------------------------------------------------------------------

#[test]
fn recalc_engine_failure_degrades_to_passthrough() {
    let bytes0 = formula_workbook().save_to_vec().unwrap();
    let mut book = read_book_from_bytes(&bytes0).unwrap();
    book.sheets[0].name = "Bad[Name*".into(); // 引擎表名校验拒

    let (bytes, stats) = rebuild_book(&book, &[edit_a1(15.0)]).unwrap();
    assert!(
        stats
            .skipped
            .iter()
            .any(|n| n.starts_with("recalc unavailable")),
        "整簿降级登记：{:?}",
        stats.skipped
    );
    assert_eq!(stats.recalc_cells, 0);

    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Number(15.0)),
        "编辑仍落格"
    );
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(20.0)),
        "公式缓存保真直通（降级）"
    );
    assert_eq!(
        d.formula_at(0, 1).map(str::to_string),
        Some("A1*2".into()),
        "公式文本保真"
    );
}

// ---------------------------------------------------------------------------
// 无缓存公式格（<f> 无 <v>——纯公式格以 Empty 占位进 typed 网，探针钉定）
// ---------------------------------------------------------------------------

#[test]
fn recalc_no_cache_formula_cell() {
    let bytes0 = build_xlsx(r#"<row r="1"><c r="A1"><v>5</v></c><c r="C1"><f>1+1</f></c></row>"#);
    let book = read_book_from_bytes(&bytes0).unwrap();

    // 重算模式：构建即算 → 缓存 2。
    let (bytes, stats) = rebuild_book(&book, &[]).unwrap();
    assert_eq!(stats.recalc_cells, 1);
    let d = &read_book_from_bytes(&bytes).unwrap().sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 2),
        Some(&CellData::Number(2.0)),
        "无缓存公式格重算得值（「新建公式算不出结果」缺口在 rebuild 面闭合）"
    );

    // 非重算模式：历史行为钉定——calamine values 面不收纯 f 无 v 格
    //（探针钉定），C1 在 typed 使用区外不写 = 静默丢失；重算模式的
    // typed ∪ formulas 並集扩展闭合此缺口（上文已验）。
    let (bytes, _) = rebuild_book_with(&book, &[], &RebuildOptions { recalculate: false }).unwrap();
    let d = &read_book_from_bytes(&bytes).unwrap().sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 2),
        None,
        "非重算模式纯公式格（无缓存）不写——历史行为（重算模式闭合）"
    );
    assert_eq!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Number(5.0)),
        "值格不受影响"
    );
}

// ---------------------------------------------------------------------------
// 验收 4a：共享公式（<f t="shared">）——calamine 从格位移展开（探针
// 钉定：B2 → "A2*2"），逐格独立重算，主从关系不落盘（写出独立公式
// 格——语义等价）
// ---------------------------------------------------------------------------

#[test]
fn recalc_shared_formula_expanded_and_recalculated() {
    let bytes0 = build_xlsx(
        r#"<row r="1"><c r="A1"><v>10</v></c><c r="B1"><f t="shared" ref="B1:B3" si="0">A1*2</f><v>20</v></c></row>
            <row r="2"><c r="A2"><v>11</v></c><c r="B2"><f t="shared" si="0"/></c></row>
            <row r="3"><c r="A3"><v>12</v></c><c r="B3"><f t="shared" si="0"/></c></row>"#,
    );
    let book = read_book_from_bytes(&bytes0).unwrap();
    // 探针钉定行为锚：从格展开为位移公式。
    assert_eq!(
        book.sheets[0].formula_at(1, 1).map(str::to_string),
        Some("A2*2".into()),
        "calamine 从格位移展开（钉定）"
    );

    // 改 A2=110 → B2 重算 220；B1（A1 未变）保持 20；B3 无源缓存 → 24。
    let edits = vec![CellEdit {
        sheet: 0,
        row: 1,
        col: 0,
        value: EditValue::Number(110.0),
    }];
    let (bytes, stats) = rebuild_book(&book, &edits).unwrap();
    assert_eq!(stats.recalc_cells, 3, "主从三格均独立重算");

    let d = &read_book_from_bytes(&bytes).unwrap().sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(20.0)),
        "B1 源缓存同值"
    );
    assert_eq!(
        d.typed.get_abs(1, 1),
        Some(&CellData::Number(220.0)),
        "B2 展开 A2*2 重算（依赖更新）"
    );
    assert_eq!(
        d.typed.get_abs(2, 1),
        Some(&CellData::Number(24.0)),
        "B3 展开 A3*2 重算（原无缓存 → 补值）"
    );
    assert_eq!(
        d.formula_at(1, 1).map(str::to_string),
        Some("A2*2".into()),
        "从格写出独立公式（语义等价）"
    );
}

// ---------------------------------------------------------------------------
// 验收 4b：数组公式（<f t="array" ref="B1">——单格 ref 形态，探针
// 钉定 calamine 读面等同普通公式文本）
// ---------------------------------------------------------------------------

#[test]
fn recalc_array_formula_single_cell() {
    let bytes0 = build_xlsx(
        r#"<row r="1"><c r="A1"><v>10</v></c><c r="B1"><f t="array" ref="B1">A1*2</f><v>20</v></c></row>"#,
    );
    let book = read_book_from_bytes(&bytes0).unwrap();
    assert_eq!(
        book.sheets[0].formula_at(0, 1).map(str::to_string),
        Some("A1*2".into()),
        "数组公式 f 文本读面（钉定）"
    );

    let edits = vec![CellEdit {
        sheet: 0,
        row: 0,
        col: 0,
        value: EditValue::Number(50.0),
    }];
    let d = &rebuild_read(&book, &edits).sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(100.0)),
        "单格数组公式重算（多格 spill 区形态为引擎能力边界——见模块文档）"
    );
}

// ---------------------------------------------------------------------------
// 错误常量格（t="e" 无公式）——排除灌入 + 登记；引用它的公式按空格
// 语义（实测 =E1+1 → 1；Excel 语义为错误传播——能力边界如实断言）
// ---------------------------------------------------------------------------

#[test]
fn recalc_error_const_cell_excluded_and_registered() {
    let bytes0 = build_xlsx(
        r#"<row r="1"><c r="A1"><v>5</v></c><c r="E1" t="e"><v>#DIV/0!</v></c><c r="F1"><f>E1+1</f><v>2</v></c></row>"#,
    );
    let book = read_book_from_bytes(&bytes0).unwrap();
    assert_eq!(
        book.sheets[0].typed.get_abs(0, 4),
        Some(&CellData::Error("#DIV/0!".into())),
        "错误常量格读面（钉定）"
    );

    let (bytes, stats) = rebuild_book(&book, &[]).unwrap();
    assert!(
        stats
            .skipped
            .iter()
            .any(|n| n.contains("1 error-constant cells excluded")),
        "排除登记：{:?}",
        stats.skipped
    );

    let d = &read_book_from_bytes(&bytes).unwrap().sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 4),
        Some(&CellData::Text("#DIV/0!".into())),
        "错误常量格值面直通为文本（写面 CellValue 无错误型——kind 降级\n\
          为写面既有行为，非重算引入；非重算模式同形态）"
    );
    assert_eq!(
        d.typed.get_abs(0, 5),
        Some(&CellData::Number(1.0)),
        "引用错误常量格的公式按空格语义（E1 缺位 + 1 = 1——引擎实测；\
          Excel 为错误传播，能力边界如实断言）"
    );
}

// ---------------------------------------------------------------------------
// 验收 2：LibreOffice 显示值对照（环境门——soffice 缺失 SKIP）
// ---------------------------------------------------------------------------

#[test]
fn recalc_lo_display_cross_check() {
    let probe = Command::new("soffice").arg("--version").output();
    if probe.is_err() {
        eprintln!("[recalc] soffice 缺失 — SKIP（环境门）");
        return;
    }

    // 源档：A1=10，B1==A1*2（缓存 20），C1==SUM(A1:B1)（缓存 30）。
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
            text: "=SUM(A1:B1)".into(),
            cached: Some(FormulaCache::Number(30.0)),
        },
        None,
    )
    .unwrap();
    let bytes0 = w.save_to_vec().unwrap();
    let book = read_book_from_bytes(&bytes0).unwrap();

    // 编辑 A1=15 → 重算：B1=30，C1=SUM(15,30)=45。
    let (bytes, _) = rebuild_book(&book, &[edit_a1(15.0)]).unwrap();

    // LO 打开导出 csv（外来 xlsx 默认不重算——csv 显示缓存值）。
    let out_dir = std::env::temp_dir().join(format!("mdc-recalc-lo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let src_path = out_dir.join("edited.xlsx");
    std::fs::write(&src_path, &bytes).unwrap();
    let status = Command::new("soffice")
        .args([
            "--headless",
            &format!(
                "-env:UserInstallation=file:///{}",
                out_dir.join("lo-profile").display()
            ),
            "--convert-to",
            "csv",
            "--outdir",
            out_dir.to_str().unwrap(),
            src_path.to_str().unwrap(),
        ])
        .output()
        .expect("soffice 调用失败");
    assert!(
        status.status.success(),
        "soffice convert 失败: {:?}",
        status
    );
    let csv_path = out_dir.join("edited.csv");
    assert!(csv_path.exists(), "csv 产物缺失");
    let csv = std::fs::read_to_string(&csv_path).unwrap();
    let first_line = csv.lines().next().unwrap_or_default();
    // LO csv 数值显示形态：整数无小数尾（15/30/45）。
    assert!(
        first_line.contains("15") && first_line.contains("30") && first_line.contains("45"),
        "LO 显示值对照（缓存 30/45 应出现在首行）：{first_line:?}"
    );
    let _ = std::fs::remove_dir_all(&out_dir);
}
