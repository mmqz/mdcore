//! 表格表面数据面测试（纯内核面——JsValue-free，原生 cargo test）。
//! 夹具 = 合成簿（office-io 写面）→ read_book_from_bytes（读面安全门）。

use mdc_office_io::io::read_book_from_bytes;
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdcore_wasm::xlsx_grid::{sheets_core, slice_core, GridError};

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
