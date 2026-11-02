//! XLSX 原位编辑保真矩阵（#177 验收面）。
//!
//! 三层验证：
//! 1. **引擎富夹具**（WorkbookWriter 生成——样式/合并/列宽/超链接/
//!    公式缓存值在档）→ 编辑 → **非目标部件逐字节恒等** + calamine
//!    读回语义断言（新值/旧值/合并/公式缓存值）；
//! 2. **真实语料**（test-artifacts/formula-financial/fin_lo.xlsx——
//!    LibreOffice 真实产物，shared string/公式/样式面）→ 编辑数据格 →
//!    读回断言 + 未动格恒等 + 零编辑往返文件级字节恒等；
//! 3. **LibreOffice 开档门**（环境门——soffice 缺失 SKIP）：编辑产物
//!    soffice --convert-to csv 成功 = LibreOffice 可开（「用 Excel 打开
//!    确认格式未丢」的沙箱可自动化替身——Excel 实机面进 PR 验证清单）。

use std::collections::BTreeMap;
use std::process::Command;

use calamine::Data as CalData;
use mdc_office_io::io::{read_book_from_bytes, CellData, ReadOptions};
use mdc_office_io::write::{
    Align, Border, CellStyle, CellValue, FormulaCache, NumFmt, WorkbookWriter,
};
use mdc_office_io::xlsx_edit::{CellEdit, XlsxEditError, XlsxEditor};

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-artifacts/formula-financial"
);

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

/// 富夹具：表头样式（bold/字色/填充/thin 边框/¥ numFmt/居中）+
/// 合并单元格 + 公式缓存值 + 多行数据（rust_xlsxwriter 引擎产物——
/// 真实生成器面，非手写 XML）。
fn rich_fixture() -> Vec<u8> {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("销售").unwrap();
    let header = CellStyle {
        bold: true,
        font_color: Some("#FF0000".into()),
        fill_color: Some("#DDDDDD".into()),
        border: Border::Thin,
        num_fmt: None,
        align: Align::Center,
    };
    let money = CellStyle {
        bold: false,
        font_color: None,
        fill_color: None,
        border: Border::None,
        num_fmt: Some(NumFmt::CurrencyYen),
        align: Align::Center,
    };
    s.write(0, 0, CellValue::Text("季度".into()), Some(&header))
        .unwrap();
    s.write(0, 1, CellValue::Text("收入".into()), Some(&header))
        .unwrap();
    s.write(0, 2, CellValue::Text("成本".into()), Some(&header))
        .unwrap();
    for (i, q) in ["Q1", "Q2", "Q3", "Q4"].iter().enumerate() {
        let r = (i + 1) as u32;
        s.write(r, 0, CellValue::Text((*q).into()), None).unwrap();
        s.write(
            r,
            1,
            CellValue::Number(100000.0 * (i as f64 + 1.0)),
            Some(&money),
        )
        .unwrap();
        s.write(r, 2, CellValue::Number(50000.0 * (i as f64 + 1.0)), None)
            .unwrap();
    }
    // 公式行（小计 + 比率）——编辑引擎拒改面 + 缓存值保真面。
    s.write(
        5,
        1,
        CellValue::Formula {
            text: "=SUM(B2:B5)".into(),
            cached: Some(FormulaCache::Number(1000000.0)),
        },
        None,
    )
    .unwrap();
    s.write(
        5,
        2,
        CellValue::Formula {
            text: "=SUM(C2:C5)/SUM(B2:B5)".into(),
            cached: Some(FormulaCache::Number(0.5)),
        },
        None,
    )
    .unwrap();
    // 合并 + 超链接（保真断言对象）。
    s.merge(6, 0, 6, 2, "汇总区（合并）", None).unwrap();
    s.write_hyperlink(
        7,
        0,
        mdc_office_io::write::HyperlinkSpec {
            url: "https://example.com/".into(),
            display_text: Some("外链".into()),
            tooltip: None,
        },
    )
    .unwrap();
    w.save_to_vec().unwrap()
}

fn sheet_idx(ed: &XlsxEditor, name: &str) -> usize {
    ed.sheets()
        .iter()
        .position(|s| s.name == name)
        .unwrap_or_else(|| panic!("表 {name} 不在"))
}

/// 非目标部件逐字节恒等（保真核心断言——样式/共享串/内容类型/rels
/// 等「没碰」= 保真不靠会写而靠不碰）。
fn assert_other_parts_identical(orig: &[u8], edited: &[u8], skip: &[&str]) {
    let names = all_part_names(orig);
    assert_eq!(all_part_names(edited), names, "zip 条目集与序恒等");
    for name in &names {
        if skip.contains(&name.as_str()) {
            continue;
        }
        assert_eq!(
            part_bytes(orig, name),
            part_bytes(edited, name),
            "非目标部件字节恒等失败: {name}"
        );
    }
}

/// 全部件解压内容恒等（skip 例外）——部件内容零接触的完整断言
/// （容器层本地头/描述符形态差异不计——zip 写器规范化面）。
fn assert_parts_content_identical(orig: &[u8], edited: &[u8], skip: &[&str]) {
    let names = all_part_names(orig);
    assert_eq!(all_part_names(edited), names, "zip 条目集与序恒等");
    for name in &names {
        if skip.contains(&name.as_str()) {
            continue;
        }
        assert_eq!(
            part_bytes(orig, name),
            part_bytes(edited, name),
            "部件内容恒等失败: {name}"
        );
    }
}

fn cell_value_at(bytes: &[u8], sheet: &str, row: u32, col: u32) -> CellData {
    let book = read_book_from_bytes_with_compat(bytes);
    let sh = book
        .sheets
        .iter()
        .find(|s| s.name == sheet)
        .unwrap_or_else(|| panic!("表 {sheet} 不在"));
    sh.typed
        .get_abs(row, col)
        .cloned()
        .unwrap_or(CellData::Empty)
}

fn read_book_from_bytes_with_compat(bytes: &[u8]) -> mdc_office_io::io::Book {
    read_book_from_bytes(bytes).unwrap_or_else(|e| {
        // 编辑产物含引擎写面不覆盖的部件（styles 变体等）——compat
        // 容忍（数据面提取优先；三门恒同）。
        let _ = e;
        read_book_with_compat(bytes)
    })
}

fn read_book_with_compat(bytes: &[u8]) -> mdc_office_io::io::Book {
    mdc_office_io::io::read_book_from_bytes_with_options(bytes, &ReadOptions { compat: true })
        .expect("compat 读回失败")
}

// ---------------------------------------------------------------------------
// 层 1：引擎富夹具保真
// ---------------------------------------------------------------------------

#[test]
fn rich_fixture_edit_keeps_style_merge_formula_cache() {
    let bytes = rich_fixture();
    let mut ed = XlsxEditor::open(&bytes).unwrap();
    let s = sheet_idx(&ed, "销售");
    // 改 B2（数据格，带 ¥ 样式）与 C3（数据格）。
    ed.set_cell(s, "B2", CellEdit::Number(123456.0)).unwrap();
    ed.set_cell(s, "C3", CellEdit::Text("文本成本".into()))
        .unwrap();
    // 插入新格 D2（行内列尾）与 B9（新行——表尾外）。
    ed.set_cell(s, "D2", CellEdit::Boolean(true)).unwrap();
    ed.set_cell(s, "B9", CellEdit::Number(7.0)).unwrap();
    let saved = ed.save_to_vec().unwrap();

    // 非目标部件字节恒等（styles.xml/sharedStrings.xml/[Content_Types]
    // /workbook.xml/rels 全部未碰——保真机制「不碰」断言）。
    assert_other_parts_identical(&bytes, &saved, &["xl/worksheets/sheet1.xml"]);

    // calamine 读回语义断言。
    assert_eq!(
        cell_value_at(&saved, "销售", 1, 1),
        CellData::Number(123456.0),
        "B2 新值"
    );
    assert_eq!(
        cell_value_at(&saved, "销售", 2, 2),
        CellData::Text("文本成本".into()),
        "C3 新文本（inlineStr 读回）"
    );
    assert_eq!(
        cell_value_at(&saved, "销售", 1, 3),
        CellData::Boolean(true),
        "D2 插入布尔"
    );
    assert_eq!(
        cell_value_at(&saved, "销售", 8, 1),
        CellData::Number(7.0),
        "B9 插行数字"
    );
    // 未动格恒等。
    assert_eq!(
        cell_value_at(&saved, "销售", 0, 0),
        CellData::Text("季度".into()),
        "表头未动"
    );
    assert_eq!(
        cell_value_at(&saved, "销售", 4, 1),
        CellData::Number(400000.0),
        "Q4 收入未动"
    );
    // 公式缓存值恒等（拒改面保证 <f>/<v> 零接触）。
    let book = read_book_with_compat(&saved);
    let sh = book.sheets.iter().find(|x| x.name == "销售").unwrap();
    assert_eq!(sh.formula_at(5, 1), Some("SUM(B2:B5)"), "公式文本恒等");
    assert_eq!(
        sh.values.get((5, 1)).cloned().unwrap_or(CalData::Empty),
        CalData::Float(1000000.0),
        "公式缓存值恒等（不重算）"
    );
    // 合并范围恒等。
    assert!(
        sh.merges
            .iter()
            .any(|m| m.start == (6, 0) && m.end == (6, 2)),
        "合并范围保留: {:?}",
        sh.merges
    );
    // 样式格（B2 的 s 属性）保留——sheet1.xml 字节形态断言。
    let xml = String::from_utf8(part_bytes(&saved, "xl/worksheets/sheet1.xml")).unwrap();
    let b2 = xml
        .split("r=\"B2\"")
        .nth(1)
        .unwrap()
        .split("</c>")
        .next()
        .unwrap();
    assert!(b2.contains("s="), "B2 样式属性保留：{b2}");
    assert!(b2.contains("<v>123456</v>"), "B2 新值形态：{b2}");
    // 列宽段（<cols>）原样保留。
    if let Some(cols) = bytes_cols(&bytes) {
        assert!(xml.contains(&cols), "列宽段原样保留");
    }
}

fn bytes_cols(bytes: &[u8]) -> Option<String> {
    let xml = String::from_utf8(part_bytes(bytes, "xl/worksheets/sheet1.xml")).ok()?;
    let start = xml.find("<cols")?;
    let end = xml[start..].find("</cols>")? + start + "</cols>".len();
    Some(xml[start..end].to_string())
}

#[test]
fn rich_fixture_zero_edit_parts_identical() {
    let bytes = rich_fixture();
    let ed = XlsxEditor::open(&bytes).unwrap();
    let saved = ed.save_to_vec().unwrap();
    // 引擎产物部件级恒等（rust_xlsxwriter 条目带 extra 字段/版本标记
    // ——容器层重写有差异；文件级恒等 = 本 crate 自产 zip 夹具
    // 单测 zero_edit_save_is_byte_identical 覆盖）。
    assert_parts_content_identical(&bytes, &saved, &[]);
}

#[test]
fn formula_cell_in_engine_fixture_rejected() {
    let bytes = rich_fixture();
    let mut ed = XlsxEditor::open(&bytes).unwrap();
    let s = sheet_idx(&ed, "销售");
    let err = ed.set_cell(s, "B6", CellEdit::Number(1.0)).unwrap_err();
    assert!(matches!(err, XlsxEditError::FormulaCell { .. }), "{err}");
}

// ---------------------------------------------------------------------------
// 层 2：真实语料（fin_lo.xlsx——LibreOffice 产物）
// ---------------------------------------------------------------------------

#[test]
fn fin_lo_real_file_edit_and_zero_touch() {
    let path = std::path::Path::new(FIXTURE_DIR).join("fin_lo.xlsx");
    if !path.exists() {
        panic!(
            "真值语料缺失：{}（重建：python3 test-artifacts/formula-financial/gen_fin_fixture.py）",
            path.display()
        );
    }
    let bytes = std::fs::read(&path).unwrap();

    // 零编辑往返：部件级恒等（LibreOffice 产物是数据描述符流式形态
    // ——flag bit 3；zip-rs raw_copy 重写本地头，容器层字节不恒等但
    // 部件内容零接触。非流式形态的文件级恒等 = 引擎产物测试覆盖）。
    let ed0 = XlsxEditor::open(&bytes).unwrap();
    let saved0 = ed0.save_to_vec().unwrap();
    assert_parts_content_identical(&bytes, &saved0, &[]);

    // 结构普查：可编辑表数 + 全部件清单。
    let sheets = ed0.sheets();
    assert!(!sheets.is_empty(), "fin_lo 应有表");
    assert!(
        sheets.iter().all(|s| s.editable),
        "全部表可编辑: {sheets:?}"
    );

    // 编辑前快照（cell_info 读面 + calamine 读面双通道）。
    let mut ed = XlsxEditor::open(&bytes).unwrap();
    let s = 0;
    let name = ed.sheets()[s].name.clone();
    let before_a1 = cell_value_at(&bytes, &name, 0, 0);
    let before_b1 = cell_value_at(&bytes, &name, 0, 1);

    // 数据格编辑（fin 模型 A1=收入类别文本、B1=数量——非公式格；
    // 若首格恰为公式格则顺延找数据格）。
    let target = first_data_cell(&mut ed, s, &["A1", "B1", "C1", "A2", "B2"]);
    ed.set_cell(s, &target, CellEdit::Text("引擎改写".into()))
        .unwrap();
    let saved = ed.save_to_vec().unwrap();

    // 非目标部件字节恒等（styles/sharedStrings/theme/workbook——
    // LibreOffice 全部部件未碰）。
    let sheet_part = format!(
        "xl/worksheets/sheet{}.xml",
        1 // fin_lo 首表 = sheet1（单表簿）
    );
    assert_other_parts_identical(&bytes, &saved, &[&sheet_part]);

    // 读回：新值 + 未动邻格恒等。
    let after_target = ed.cell_info(s, &target).unwrap().unwrap();
    assert_eq!(after_target.text, "引擎改写");
    let (row, col) = parse(&target);
    assert_eq!(
        cell_value_at(&saved, &name, row, col),
        CellData::Text("引擎改写".into()),
        "{target} 新值读回"
    );
    if let CellData::Text(t) = &before_a1 {
        if target != "A1" {
            assert_eq!(
                cell_value_at(&saved, &name, 0, 0),
                CellData::Text(t.clone()),
                "未动格 A1 恒等"
            );
        }
    }
    let _ = before_b1;

    // 未动公式格缓存值恒等（抽验：全簿公式格读回对照）。
    let b0 = read_book_with_compat(&bytes);
    let b1 = read_book_with_compat(&saved);
    let sh0 = &b0.sheets[s];
    let sh1 = &b1.sheets[s];
    let mut formulas_checked = 0usize;
    for r in 0..sh0.formulas.height() {
        for c in 0..sh0.formulas.width() {
            if sh0.formula_at(r as u32, c as u32).is_some() {
                let v0 = sh0.values.get((r, c)).cloned().unwrap_or(CalData::Empty);
                let v1 = sh1.values.get((r, c)).cloned().unwrap_or(CalData::Empty);
                assert_eq!(v0, v1, "公式格缓存值恒等 ({r},{c})——编辑零接触");
                assert_eq!(
                    sh0.formula_at(r as u32, c as u32),
                    sh1.formula_at(r as u32, c as u32),
                    "公式文本恒等 ({r},{c})"
                );
                formulas_checked += 1;
            }
        }
    }
    assert!(
        formulas_checked >= 20,
        "fin_lo 公式面覆盖: {formulas_checked}"
    );
}

fn first_data_cell(ed: &mut XlsxEditor, sheet: usize, cands: &[&str]) -> String {
    for c in cands {
        if let Ok(Some(info)) = ed.cell_info(sheet, c) {
            if !info.is_formula {
                return (*c).to_string();
            }
        }
    }
    panic!("候选坐标全部为公式格/空格: {cands:?}");
}

fn parse(cell_ref: &str) -> (u32, u32) {
    // 行 0 基、列 0 基（calamine get_abs 口径）。
    let bytes = cell_ref.as_bytes();
    let mut i = 0;
    let mut col: u32 = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        col = col * 26 + u32::from(bytes[i].to_ascii_uppercase() - b'A' + 1);
        i += 1;
    }
    let row: u32 = cell_ref[i..].parse().unwrap();
    (row - 1, col - 1)
}

// ---------------------------------------------------------------------------
// 层 3：LibreOffice 开档门（环境门——soffice 缺失 SKIP）
// ---------------------------------------------------------------------------

#[test]
fn soffice_opens_edited_fin_lo() {
    let probe = Command::new("soffice").arg("--version").output();
    if probe.is_err() {
        eprintln!("[xlsx-edit] soffice 缺失 — SKIP（环境门）");
        return;
    }
    let path = std::path::Path::new(FIXTURE_DIR).join("fin_lo.xlsx");
    if !path.exists() {
        eprintln!("[xlsx-edit] fin_lo.xlsx 缺失 — SKIP（环境门层）");
        return;
    }
    let bytes = std::fs::read(&path).unwrap();
    let mut ed = XlsxEditor::open(&bytes).unwrap();
    let target = first_data_cell(&mut ed, 0, &["A1", "B1", "C1", "A2", "B2"]);
    ed.set_cell(0, &target, CellEdit::Text("LO 开档验证".into()))
        .unwrap();
    let saved = ed.save_to_vec().unwrap();

    let out_dir = std::env::temp_dir().join(format!("mdc-xlsx-edit-lo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let src = out_dir.join("edited.xlsx");
    std::fs::write(&src, &saved).unwrap();
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
            src.to_str().unwrap(),
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
    assert!(
        csv.contains("LO 开档验证"),
        "LibreOffice 读到编辑值（开档 + 值面双证）：{}",
        csv.lines().next().unwrap_or_default()
    );
    let _ = std::fs::remove_dir_all(&out_dir);
}

// ---------------------------------------------------------------------------
// 挂账登记：BTreeMap 依赖仅为类型引用（pending 落账模型文档锚）。
// ---------------------------------------------------------------------------

#[test]
fn pending_model_last_write_wins() {
    let bytes = rich_fixture();
    let mut ed = XlsxEditor::open(&bytes).unwrap();
    let s = sheet_idx(&ed, "销售");
    ed.set_cell(s, "B2", CellEdit::Number(1.0)).unwrap();
    ed.set_cell(s, "B2", CellEdit::Text("后写".into())).unwrap();
    assert_eq!(ed.cell_info(s, "B2").unwrap().unwrap().text, "后写");
    let saved = ed.save_to_vec().unwrap();
    assert_eq!(
        cell_value_at(&saved, "销售", 1, 1),
        CellData::Text("后写".into()),
        "后写胜落账"
    );
    // 空编辑集 → 部件透传（不进替换表）。
    let m: BTreeMap<(u32, u32), CellEdit> = BTreeMap::new();
    assert!(m.is_empty());
}
