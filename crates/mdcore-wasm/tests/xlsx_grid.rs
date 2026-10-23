//! 表格表面数据面测试（纯内核面——JsValue-free，原生 cargo test）。
//! 夹具 = 合成簿（office-io 写面）→ read_book_from_bytes（读面安全门）。
//! p5-t2 增量：读-改-重建出口（export_with_edits_core 全环）。

use mdc_office_io::io::{read_book_from_bytes, CellData};
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdcore_wasm::xlsx_grid::{
    export_with_edits_core, parse_edits_core, sheets_core, slice_core, GridError, MAX_EDIT_CELLS,
};

fn sample_book() -> Vec<u8> {
    let mut w = WorkbookWriter::new();
    let mut sh = w.add_sheet("data").unwrap();
    // 0 基：A1=(0,0) 表头；B2=(1,1) 数值；C2=(1,2) 文本（前导零语义）
    sh.write(0, 0, CellValue::Text("名称".into()), None)
        .unwrap();
    sh.write(0, 1, CellValue::Text("数量".into()), None)
        .unwrap();
    sh.write(1, 1, CellValue::Number(12.5), None).unwrap();
    sh.write(1, 2, CellValue::Text("007".into()), None).unwrap();
    w.save_to_vec().unwrap()
}

#[test]
fn sheets_and_slice_roundtrip() {
    let bytes = sample_book();
    let book = read_book_from_bytes(&bytes).unwrap();
    let sheets = sheets_core(&book);
    assert!(sheets.contains("\"name\":\"data\""), "表名: {sheets}");
    assert!(sheets.contains("\"rows\":2"), "使用区 2 行: {sheets}");
    assert!(sheets.contains("\"cols\":3"), "使用区 3 列: {sheets}");

    // 全量窗
    let s = slice_core(&book, 0, 0, 0, 2, 3).unwrap();
    assert!(s.contains("\"k\":\"text\""));
    assert!(s.contains("\"v\":12.5"), "数值原值: {s}");
    assert!(s.contains("\"k\":\"number\""));
    assert!(s.contains("\"v\":\"007\""), "前导零文本原样: {s}");

    // 部分窗（B2 起 1×2）
    let s2 = slice_core(&book, 0, 1, 1, 1, 2).unwrap();
    assert!(s2.contains("\"r0\":1"));
    assert!(s2.contains("\"v\":12.5"));

    // 窗外钳制 = empty 填充（虚拟滚动空白区语义）
    let s3 = slice_core(&book, 0, 100, 100, 2, 2).unwrap();
    assert_eq!(s3.matches("\"k\":\"empty\"").count(), 4, "全空窗: {s3}");
}

#[test]
fn sheet_index_and_slice_limit_errors() {
    let bytes = sample_book();
    let book = read_book_from_bytes(&bytes).unwrap();
    assert!(matches!(
        slice_core(&book, 5, 0, 0, 1, 1),
        Err(GridError::SheetIndex {
            index: 5,
            sheets: 1
        })
    ));
    assert!(matches!(
        slice_core(&book, 0, 0, 0, 1000, 1000),
        Err(GridError::SliceTooLarge {
            cells: 1_000_000,
            ..
        })
    ));
    assert_eq!(mdcore_wasm::xlsx_grid::MAX_SLICE_CELLS, 100_000);
}

// ---------------------------------------------------------------------------
// p5-t2：读-改-重建出口（export_with_edits_core）
// ---------------------------------------------------------------------------

#[test]
fn export_with_edits_roundtrip() {
    let bytes = sample_book();
    let book = read_book_from_bytes(&bytes).unwrap();
    let edits = r#"[{"sheet":0,"r":1,"c":1,"k":"number","v":99.5},{"sheet":0,"r":1,"c":2,"k":"text","v":"edited"}]"#;
    let (out, stats) = export_with_edits_core(&book, edits).unwrap();
    assert!(out.starts_with(b"PK"), "容器 magic");

    // 统计形态：键全 + 编辑消费。
    for key in [
        "\"sheets\"",
        "\"cells_written\"",
        "\"merges\"",
        "\"hyperlinks\"",
        "\"edits_applied\":2",
        "\"skipped\":[]",
    ] {
        assert!(stats.contains(key), "stats 缺 {key}: {stats}");
    }

    // 读回：编辑值在位、未编辑格保持。
    let rebuilt = read_book_from_bytes(&out).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(d.typed.get_abs(1, 1), Some(&CellData::Number(99.5)));
    assert_eq!(
        d.typed.get_abs(1, 2),
        Some(&CellData::Text("edited".into()))
    );
    assert_eq!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Text("名称".into())),
        "未编辑格保持"
    );
}

#[test]
fn export_edits_json_error_faces() {
    let bytes = sample_book();
    let book = read_book_from_bytes(&bytes).unwrap();
    // 非法 JSON。
    assert!(matches!(
        export_with_edits_core(&book, "{not json"),
        Err(GridError::EditsJson(_))
    ));
    // 非数组顶層。
    assert!(matches!(
        export_with_edits_core(&book, "{\"sheet\":0}"),
        Err(GridError::EditsJson(_))
    ));
    // 未知 kind。
    assert!(matches!(
        export_with_edits_core(&book, r#"[{"sheet":0,"r":0,"c":0,"k":"blob","v":1}]"#),
        Err(GridError::EditsJson(_))
    ));
    // k=text 缺 v。
    assert!(matches!(
        export_with_edits_core(&book, r#"[{"sheet":0,"r":0,"c":0,"k":"text"}]"#),
        Err(GridError::EditsJson(_))
    ));
    // FFFE/FFFF 编辑拒（rebuild_invalid）。
    match export_with_edits_core(
        &book,
        r#"[{"sheet":0,"r":0,"c":0,"k":"text","v":"x\uFFFF"}]"#,
    ) {
        Err(GridError::Rebuild { code, .. }) => assert_eq!(code, "rebuild_invalid"),
        other => panic!("expected Rebuild, got {other:?}"),
    }
    // 账本规模上限。
    let big: Vec<String> = (0..=MAX_EDIT_CELLS)
        .map(|i| format!(r#"{{"sheet":0,"r":0,"c":{i},"k":"number","v":1}}"#))
        .collect();
    assert!(matches!(
        export_with_edits_core(&book, &format!("[{}]", big.join(","))),
        Err(GridError::EditsJson(_))
    ));
}

#[test]
fn parse_edits_accepts_all_kinds() {
    let edits = parse_edits_core(
        r#"[{"sheet":1,"r":2,"c":3,"k":"text","v":"a"},{"sheet":0,"r":0,"c":0,"k":"number","v":1.5},{"sheet":0,"r":1,"c":1,"k":"bool","v":true},{"sheet":0,"r":2,"c":2,"k":"empty"}]"#,
    )
    .unwrap();
    assert_eq!(edits.len(), 4);
    assert_eq!(edits[0].sheet, 1);
    assert_eq!(
        edits[1].value,
        mdc_office_io::rebuild::EditValue::Number(1.5)
    );
    assert_eq!(
        edits[2].value,
        mdc_office_io::rebuild::EditValue::Boolean(true)
    );
    assert_eq!(edits[3].value, mdc_office_io::rebuild::EditValue::Empty);
}
