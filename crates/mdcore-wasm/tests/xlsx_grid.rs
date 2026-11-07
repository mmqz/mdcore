//! 表格表面数据面测试（纯内核面——JsValue-free，原生 cargo test）。
//! 夹具 = 合成簿（office-io 写面）→ read_book_from_bytes（读面安全门）。
//! p5-t2 增量：读-改-写出口（export_with_edits_core 全环）。
//! #283 增量：in-place 优先路径（结构保真实证 + 重算接线） +
//! rebuild 兜底路径（empty 编辑 → 诚实账本）。

use mdc_office_io::io::{read_book_from_bytes, CellData};
use mdc_office_io::write::{CellValue, FormulaCache, WorkbookWriter};
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

/// 公式簿（#283 重算接线面）：A1=10（值），B1==A1*2（缓存 20）。
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
    w.save_to_vec().unwrap()
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
// p5-t2/#283：读-改-写出口（export_with_edits_core——in-place 优先
// + rebuild 兜底）
// ---------------------------------------------------------------------------

#[test]
fn export_with_edits_roundtrip() {
    let bytes = sample_book();
    let book = read_book_from_bytes(&bytes).unwrap();
    let edits = r#"[{"sheet":0,"r":1,"c":1,"k":"number","v":99.5},{"sheet":0,"r":1,"c":2,"k":"text","v":"edited"}]"#;
    let (out, stats) = export_with_edits_core(&book, &bytes, edits).unwrap();
    assert!(out.starts_with(b"PK"), "容器 magic");

    // 统计形态（#283 双路径统一形状）：六键恒在 + in-place 路径标注 +
    // 编辑消费 + 零诚实账本（值编辑、无公式 → 无降级登记）。
    for key in [
        "\"path\":\"in_place\"",
        "\"sheets\":1",
        "\"cells_written\":2",
        "\"merges\":0",
        "\"hyperlinks\":0",
        "\"edits_applied\":2",
        "\"recalc_cells\":0",
        "\"skipped\":[]",
        "\"fallback_reasons\":[]",
    ] {
        assert!(stats.contains(key), "stats 缺 {key}: {stats}");
    }

    // 结构保真实证（#283 核心价值主张）：除手术部件外全部件逐字节
    // 恒等——styles/docProps/[Content_Types] 零接触。
    let src_names = all_part_names(&bytes);
    let out_names = all_part_names(&out);
    assert_eq!(src_names, out_names, "部件清单不变");
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
fn export_with_edits_formula_recalc_in_place() {
    let bytes = formula_book();
    let book = read_book_from_bytes(&bytes).unwrap();
    // 改 A1=30 → in-place 值手术 + B1 缓存重算回写（X-1）。
    let edits = r#"[{"sheet":0,"r":0,"c":0,"k":"number","v":30}]"#;
    let (out, stats) = export_with_edits_core(&book, &bytes, edits).unwrap();

    for key in [
        "\"path\":\"in_place\"",
        "\"edits_applied\":1",
        "\"recalc_cells\":1",
        "\"skipped\":[]",
    ] {
        assert!(stats.contains(key), "stats 缺 {key}: {stats}");
    }

    let rebuilt = read_book_from_bytes(&out).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.typed.get_abs(0, 0),
        Some(&CellData::Number(30.0)),
        "值编辑落格"
    );
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&CellData::Number(60.0)),
        "依赖公式缓存重算回写（=A1*2 → 60）"
    );
    assert_eq!(
        d.formula_at(0, 1).map(str::to_string),
        Some("A1*2".into()),
        "公式文本透传（<f> 零接触）"
    );
    // 手术部件实证：公式原文在位、仅 <v> 更新。
    let sheet = part_bytes(&out, "xl/worksheets/sheet1.xml");
    let sheet_str = String::from_utf8(sheet).unwrap();
    assert!(sheet_str.contains("<f>A1*2</f><v>60</v>"), "{sheet_str}");
}

#[test]
fn export_with_edits_rebuild_fallback() {
    let bytes = sample_book();
    let book = read_book_from_bytes(&bytes).unwrap();
    // empty 编辑（清格）在 in-place v1 值面之外 → rebuild 兜底。
    let edits = r#"[{"sheet":0,"r":1,"c":1,"k":"empty"}]"#;
    let (out, stats) = export_with_edits_core(&book, &bytes, edits).unwrap();

    assert!(stats.contains("\"path\":\"rebuild\""), "stats: {stats}");
    assert!(
        stats.contains("\"fallback_reasons\":[\"1 edit(s) of kind empty/formula"),
        "回退原因登记: {stats}"
    );

    // 清格语义：B2 不再是 12.5（缺席或 Empty 占位——rebuild 不落格）。
    let rebuilt = read_book_from_bytes(&out).unwrap();
    let d = &rebuilt.sheets[0];
    assert_ne!(
        d.typed.get_abs(1, 1),
        Some(&CellData::Number(12.5)),
        "清格生效"
    );
    assert!(matches!(
        d.typed.get_abs(1, 1),
        None | Some(CellData::Empty)
    ));
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
        export_with_edits_core(&book, &bytes, "{not json"),
        Err(GridError::EditsJson(_))
    ));
    // 非数组顶层。
    assert!(matches!(
        export_with_edits_core(&book, &bytes, "{\"sheet\":0}"),
        Err(GridError::EditsJson(_))
    ));
    // 未知 kind。
    assert!(matches!(
        export_with_edits_core(
            &book,
            &bytes,
            r#"[{"sheet":0,"r":0,"c":0,"k":"blob","v":1}]"#
        ),
        Err(GridError::EditsJson(_))
    ));
    // k=text 缺 v。
    assert!(matches!(
        export_with_edits_core(&book, &bytes, r#"[{"sheet":0,"r":0,"c":0,"k":"text"}]"#),
        Err(GridError::EditsJson(_))
    ));
    // FFFE/FFFF 编辑拒（in-place 拒 → rebuild 兜底同拒 → rebuild_invalid）。
    match export_with_edits_core(
        &book,
        &bytes,
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
        export_with_edits_core(&book, &bytes, &format!("[{}]", big.join(","))),
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
