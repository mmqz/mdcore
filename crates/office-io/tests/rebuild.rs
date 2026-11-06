//! rebuild（读-改-重建，C3 唯一允许口径）——行为锚点测试。
//!
//! 覆盖面：五面保真往返（表序/值/公式文本/合并/超链接）/ 编辑账本
//! 应用（窗内覆盖 + 扩展区写入 + Empty 清空 + 后写胜）/ 非字符编辑
//! typed 拒 / sheet 索引越界拒 / 表名清洗 + skipped 登记 / 敌意
//! 超链接与单格合并跳过登记（非静默纪律）。
//!
//! 断言口径与 write_roundtrip.rs `read_modify_rebuild_path_evidence`
//! 同源（绝对坐标配对纪律——range_anchor_pinned.rs 钉定）。

use calamine::{Data, Dimensions, Hyperlink};
use mdc_office_io::io::{read_book, read_book_from_bytes};
use mdc_office_io::rebuild::{rebuild_book, CellEdit, EditValue};
use mdc_office_io::write::{CellValue, FormulaCache, HyperlinkSpec, WorkbookWriter, WriteError};

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("rebuild_test_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(format!("{name}.xlsx"))
        .to_string_lossy()
        .into_owned()
}

/// 源档：值四面（text/number/bool/公式+缓存）+ 合并 + 外链——
/// 五面保真的最小完备载体。
fn sample_workbook() -> WorkbookWriter {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("data").unwrap();
    s.write(0, 0, CellValue::Text("H0".into()), None).unwrap();
    s.write(0, 1, CellValue::Text("H1".into()), None).unwrap();
    s.write(1, 0, CellValue::Number(10.5), None).unwrap();
    s.write(
        1,
        1,
        CellValue::Formula {
            text: "SUM(1,2)".into(),
            cached: Some(FormulaCache::Number(3.0)),
        },
        None,
    )
    .unwrap();
    s.write(2, 0, CellValue::Boolean(true), None).unwrap();
    s.write(2, 1, CellValue::Text("M".into()), None).unwrap();
    s.merge(2, 1, 2, 2, "M", None).unwrap();
    s.write_hyperlink(
        3,
        0,
        HyperlinkSpec {
            url: "https://example.com".into(),
            display_text: Some("docs".into()),
            tooltip: Some("tip".into()),
        },
    )
    .unwrap();
    let mut s2 = w.add_sheet("second").unwrap();
    s2.write(0, 0, CellValue::Text("S2".into()), None).unwrap();
    w
}

#[test]
fn rebuild_roundtrip_five_faces() {
    let src = tmp("rt_src");
    sample_workbook().save_to_path(&src).unwrap();
    let book = read_book(&src).unwrap();

    let (bytes, stats) = rebuild_book(&book, &[]).unwrap();
    assert_eq!(stats.sheets, 2);
    assert_eq!(stats.edits_applied, 0);
    assert!(
        stats.skipped.is_empty(),
        "clean source: no skips, got {:?}",
        stats.skipped
    );
    assert!(stats.merges >= 1 && stats.hyperlinks >= 1);
    assert!(bytes.starts_with(b"PK"), "xlsx container magic");

    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    // 表序。
    assert_eq!(rebuilt.sheet_names(), book.sheet_names());
    // 值面：源档全部非空格在重建档同绝对坐标相等。
    for (orig, new) in book.sheets.iter().zip(rebuilt.sheets.iter()) {
        let (os, ns) = (orig.typed.start().unwrap(), new.typed.start().unwrap());
        let (oe, ne) = (orig.typed.end().unwrap(), new.typed.end().unwrap());
        assert_eq!(os, ns, "使用区起点漂移（{}）", orig.name);
        assert_eq!(oe.0, ne.0, "使用区行终点漂移（{}）", orig.name);
        for r in os.0..=oe.0 {
            for c in os.1..=oe.1 {
                let ov = orig.typed.get_abs(r, c);
                let nv = new.typed.get_abs(r, c);
                assert_eq!(ov, nv, "值面漂移 {orig:?} at r{r}c{c}");
            }
        }
        // 公式文本（绝对坐标配对）。
        for r in os.0..=oe.0 {
            for c in os.1..=oe.1 {
                assert_eq!(
                    orig.formula_at(r, c).map(str::to_string),
                    new.formula_at(r, c).map(str::to_string),
                    "公式文本漂移 at r{r}c{c}"
                );
            }
        }
    }
    // 合并 + 超链接计数面。
    let (om, nm): (usize, usize) = book
        .sheets
        .iter()
        .zip(rebuilt.sheets.iter())
        .map(|(a, b)| (a.merges.len(), b.merges.len()))
        .fold((0, 0), |(x, y), (a, b)| (x + a, y + b));
    assert_eq!(om, nm, "合并范围数漂移");
    let (oh, nh): (usize, usize) = book
        .sheets
        .iter()
        .zip(rebuilt.sheets.iter())
        .map(|(a, b)| (a.hyperlinks.len(), b.hyperlinks.len()))
        .fold((0, 0), |x, y| (x.0 + y.0, x.1 + y.1));
    assert_eq!(oh, nh, "超链接数漂移");
}

#[test]
fn rebuild_applies_edits() {
    let src = tmp("edits_src");
    sample_workbook().save_to_path(&src).unwrap();
    let book = read_book(&src).unwrap();

    let edits = vec![
        // 窗内覆盖：number → text。
        CellEdit {
            sheet: 0,
            row: 1,
            col: 0,
            value: EditValue::Text("renamed".into()),
        },
        // 同格后写胜（账本序）。
        CellEdit {
            sheet: 0,
            row: 1,
            col: 0,
            value: EditValue::Number(99.0),
        },
        // 公式格覆盖为普通值。
        CellEdit {
            sheet: 0,
            row: 1,
            col: 1,
            value: EditValue::Number(42.0),
        },
        // bool 覆盖。
        CellEdit {
            sheet: 0,
            row: 2,
            col: 0,
            value: EditValue::Boolean(false),
        },
        // Empty = 清空（重放后该格缺位）。
        CellEdit {
            sheet: 0,
            row: 0,
            col: 1,
            value: EditValue::Empty,
        },
        // 扩展区写入（超出使用区右下）。
        CellEdit {
            sheet: 0,
            row: 5,
            col: 5,
            value: EditValue::Text("extended".into()),
        },
        // 第二表编辑。
        CellEdit {
            sheet: 1,
            row: 0,
            col: 0,
            value: EditValue::Number(7.0),
        },
    ];
    let (bytes, stats) = rebuild_book(&book, &edits).unwrap();
    // 同格后写胜：账本分组（HashMap 后写胜）后 6 格——与宿主侧
    // editsMap 最新值语义镜像（显示/导出/重建三面一致）。
    assert_eq!(
        stats.edits_applied, 6,
        "编辑格数 = 分组后键数（同格双写合流）"
    );

    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.typed.get_abs(1, 0),
        Some(&mdc_office_io::io::CellData::Number(99.0)),
        "后写胜"
    );
    assert_eq!(
        d.typed.get_abs(1, 1),
        Some(&mdc_office_io::io::CellData::Number(42.0)),
        "公式格覆盖"
    );
    assert_eq!(
        d.typed.get_abs(2, 0),
        Some(&mdc_office_io::io::CellData::Boolean(false))
    );
    // 清空 = 使用区内空格（包围盒由 H0 撑起——get_abs in-range
    // 缺格 = Empty 语义，CellGrid 契约）。
    assert_eq!(
        d.typed.get_abs(0, 1),
        Some(&mdc_office_io::io::CellData::Empty),
        "Empty 清空"
    );
    assert_eq!(
        d.typed.get_abs(5, 5),
        Some(&mdc_office_io::io::CellData::Text("extended".into())),
        "扩展区写入"
    );
    assert_eq!(
        rebuilt.sheets[1].typed.get_abs(0, 0),
        Some(&mdc_office_io::io::CellData::Number(7.0))
    );
}

#[test]
fn rebuild_rejects_noncharacter_edit() {
    let src = tmp("fffe_src");
    sample_workbook().save_to_path(&src).unwrap();
    let book = read_book(&src).unwrap();
    let edits = vec![CellEdit {
        sheet: 0,
        row: 0,
        col: 0,
        value: EditValue::Text("bad \u{FFFF} char".into()),
    }];
    match rebuild_book(&book, &edits) {
        Err(WriteError::InvalidInput(m)) => {
            assert!(m.contains("U+FFFF"), "消息含码点：{m}");
            assert!(m.contains("row 0"), "消息含坐标：{m}");
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn rebuild_rejects_sheet_index_edit() {
    let src = tmp("idx_src");
    sample_workbook().save_to_path(&src).unwrap();
    let book = read_book(&src).unwrap();
    let edits = vec![CellEdit {
        sheet: 9,
        row: 0,
        col: 0,
        value: EditValue::Number(1.0),
    }];
    match rebuild_book(&book, &edits) {
        Err(WriteError::InvalidInput(m)) => assert!(m.contains("sheet index 9"), "{m}"),
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn rebuild_sanitizes_sheet_name() {
    let src = tmp("name_src");
    sample_workbook().save_to_path(&src).unwrap();
    let mut book = read_book(&src).unwrap();
    book.sheets[0].name = "Bad[Name*".into();
    book.sheets[1].name = "Bad[Name*".into(); // 同名清洗 → 去重路径。
    let (bytes, stats) = rebuild_book(&book, &[]).unwrap();
    assert!(!stats.skipped.is_empty(), "清洗登记：{:?}", stats.skipped);
    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let names = rebuilt.sheet_names();
    assert!(
        names.iter().all(|n| !n.contains('[') && !n.contains('*')),
        "清洗后合法：{names:?}"
    );
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1], "去重后不冲突");
}

#[test]
fn rebuild_skips_hostile_entries() {
    let src = tmp("hostile_src");
    sample_workbook().save_to_path(&src).unwrap();
    let mut book = read_book(&src).unwrap();
    let sh = &mut book.sheets[0];
    // 超长 URL（> MAX_URL_TEXT=2080）。
    sh.hyperlinks.push(Hyperlink {
        range: Dimensions::new((10, 0), (10, 0)),
        target: Some("https://example.com/".to_string() + &"a".repeat(3000)),
        location: None,
        displayed_text: None,
        tooltip: None,
    });
    // NUL URL。
    sh.hyperlinks.push(Hyperlink {
        range: Dimensions::new((11, 0), (11, 0)),
        target: Some("https://ex\0ample.com".into()),
        location: None,
        displayed_text: None,
        tooltip: None,
    });
    // 敵意 scheme（#276 B-1：url_policy 白名单门——此前 rebuild 重放
    // 仅长度/NUL 两道防线，javascript: 原样写回 xlsx）。
    sh.hyperlinks.push(Hyperlink {
        range: Dimensions::new((13, 0), (13, 0)),
        target: Some("javascript:alert(1)".into()),
        location: None,
        displayed_text: None,
        tooltip: None,
    });
    // 单格合并（Excel 不允许）。
    sh.merges.push(Dimensions::new((12, 0), (12, 0)));

    let (bytes, stats) = rebuild_book(&book, &[]).unwrap();
    assert!(
        stats.skipped.iter().any(|s| s.contains("url")),
        "超长 URL 登记：{:?}",
        stats.skipped
    );
    assert!(
        stats.skipped.iter().any(|s| s.contains("NUL")),
        "NUL URL 登记"
    );
    assert!(
        stats.skipped.iter().any(|s| s.contains("url policy")),
        "敵意 scheme 登记（#276）：{:?}",
        stats.skipped
    );
    assert!(
        stats.skipped.iter().any(|s| s.contains("single cell")),
        "单格合并登记"
    );
    // 敌意面不阻断整体重建（跳过 + 登记继续）。
    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let hostile = rebuilt.sheets[0]
        .hyperlinks
        .iter()
        .filter(|h| {
            h.range.start == (10, 0) || h.range.start == (11, 0) || h.range.start == (13, 0)
        })
        .count();
    assert_eq!(hostile, 0, "敌意链接零落盘");
}

#[test]
fn rebuild_edit_on_formula_sheet_keeps_other_formulas() {
    // 编辑不破坏邻近公式格（公式两通道联接的回归锚）。
    let src = tmp("formula_src");
    sample_workbook().save_to_path(&src).unwrap();
    let book = read_book(&src).unwrap();
    let edits = vec![CellEdit {
        sheet: 0,
        row: 2,
        col: 0,
        value: EditValue::Text("x".into()),
    }];
    let (bytes, _) = rebuild_book(&book, &edits).unwrap();
    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let d = &rebuilt.sheets[0];
    assert_eq!(
        d.formula_at(1, 1).map(str::to_string),
        Some("SUM(1,2)".into()),
        "未编辑公式格保持"
    );
    assert_eq!(
        d.typed.get_abs(1, 1),
        Some(&mdc_office_io::io::CellData::Number(3.0)),
        "公式缓存值保持"
    );
}

// calamine Data 未直接使用（typed 面断言）——保留导入锚定以备扩展
// 断言切换到 values 原始面时零改动。
#[allow(dead_code)]
fn _data_type_anchor(_: Data) {}

// ---------------------------------------------------------------------------
// 审查回归锚（M1/M2/S3——表名防线终止性与通过性 + merge 扁平化登记）
// ---------------------------------------------------------------------------

#[test]
fn rebuild_sheet_name_quote31_passes() {
    // M2：第 31 位引号（截断后尾引号残留 → add_sheet 拒绝位——
    // 重 trim 修复后须恒过）。
    let src = tmp("name_quote_src");
    sample_workbook().save_to_path(&src).unwrap();
    let mut book = read_book(&src).unwrap();
    let mut quote31 = "C".repeat(30);
    quote31.push('\'');
    quote31.push_str("tail");
    book.sheets[0].name = quote31;
    let (bytes, stats) = rebuild_book(&book, &[]).unwrap();
    assert!(!stats.skipped.is_empty(), "清洗登记：{:?}", stats.skipped);
    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let names = rebuilt.sheet_names();
    assert!(
        names
            .iter()
            .all(|n| !n.ends_with('\'') && n.chars().count() <= 31),
        "清洗后合法且 ≤31 chars：{names:?}"
    );
    // 读回成功本身 = add_sheet 全过（清洗职责达成）。
}

#[test]
fn rebuild_duplicate_long_names_terminate() {
    // M1：三张清洗后同为 30 字符基底的表——去重须在有限步内产出
    // ≤31 且互不冲突的名（原实现 `format!("{base} {n}")` 恒超 31
    // 死循环——基底重截修复后恒可终止）。
    let src = tmp("dup_long_src");
    sample_workbook().save_to_path(&src).unwrap();
    let mut book = read_book(&src).unwrap();
    let base: String = "B".repeat(30);
    book.sheets[0].name = base.clone();
    book.sheets[1].name = base.clone();
    book.sheets.push(mdc_office_io::io::SheetData {
        name: base.clone(),
        ..Default::default()
    });
    let start = std::time::Instant::now();
    let (bytes, _) = rebuild_book(&book, &[]).unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 5,
        "去重恒可终止（实测 {elapsed:?}——M1 死循环回归锚）"
    );
    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let names = rebuilt.sheet_names();
    assert_eq!(names.len(), 3);
    let mut seen = std::collections::BTreeSet::new();
    for n in names {
        assert!(n.chars().count() <= 31, "≤31：{n}");
        assert!(seen.insert(n.to_lowercase()), "互不冲突（忽略大小写）：{n}");
    }
}

#[test]
fn rebuild_merge_flattening_registered() {
    // S3：Number 左上合并 → 文本扁平化 + skipped 登记（非静默纪律）。
    let src = tmp("merge_flat_src");
    sample_workbook().save_to_path(&src).unwrap();
    let mut book = read_book(&src).unwrap();
    book.sheets[0].merges.push(Dimensions::new((1, 0), (1, 1)));
    // (1,0) = Number 10.5（sample_workbook 既有）——非 Text 左上。
    let (bytes, stats) = rebuild_book(&book, &[]).unwrap();
    assert!(
        stats
            .skipped
            .iter()
            .any(|s| s.contains("merge flattened (non-text top-left number)")),
        "扁平化登记：{:?}",
        stats.skipped
    );
    let rebuilt = read_book_from_bytes(&bytes).unwrap();
    let merged: Vec<_> = rebuilt.sheets[0]
        .merges
        .iter()
        .filter(|m| m.start == (1, 0))
        .collect();
    assert_eq!(merged.len(), 1, "合并区仍在位（文本形态）");
}
