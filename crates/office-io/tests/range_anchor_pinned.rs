//! calamine 0.36.1 范围锚定语义钉定测试（xlsx-r2 Phase 1 开发期实证）。
//!
//! **行为发现（2026-10-14 实测钉定）**：calamine `worksheet_range` 与
//! `worksheet_formula` 返回的 `Range` **锚点不同**——
//! - 值范围：锚定在**首个非空格**（本例 A1 → (0,0)）；
//! - 公式范围：锚定在**首个公式格**（本例 A2 → (1,0)，即使 A1 有值）。
//!
//! `Range::get((r, c))` 的坐标是**相对各自锚点**的。消费方跨面配对
//! （值 ↔ 公式文本）必须换算绝对坐标（fidelity.rs abs 键先例；相对
//! 键会产生整体错位的自洽假通过——值面按位缓存值保留 + 公式列均匀
//! 平移后相对坐标重锚，两面对账都看不到错位，开发中实测捕获）。
//!
//! 升版 calamine 须重跑本钉定（SECURITY_POLICY §5 复核纪律）。

use mdc_office_io::write::{CellValue, FormulaCache, WorkbookWriter};

fn tmp(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("mdc_io_anchor_{name}_{}.xlsx", std::process::id()));
    p.to_string_lossy().into_owned()
}

#[test]
fn range_anchoring_semantics_pinned() {
    let p = tmp("anchor");
    let mut w = WorkbookWriter::new();
    {
        let mut s = w.add_sheet("S").unwrap();
        s.write(0, 0, CellValue::Text("h".into()), None).unwrap();
        s.write(
            1,
            0,
            CellValue::Formula {
                text: "SUM(B1:B1)".into(),
                cached: Some(FormulaCache::Number(1.0)),
            },
            None,
        )
        .unwrap();
        s.write(
            2,
            0,
            CellValue::Formula {
                text: "=AVERAGE(B1:B1)".into(),
                cached: None,
            },
            None,
        )
        .unwrap();
    }
    w.save_to_path(&p).unwrap();

    let book = mdc_office_io::io::read_book(&p).unwrap();
    let s = &book.sheets[0];

    // 值范围锚定首格 (0,0)——A1 文本在位。
    assert_eq!(s.values.start(), Some((0, 0)), "值范围锚定首个非空格");
    assert!(matches!(
        s.values.get((0, 0)),
        Some(calamine::Data::String(_))
    ));

    // 公式范围锚定首个公式格 (1,0)——非 (0,0)。
    assert_eq!(s.formulas.start(), Some((1, 0)), "公式范围锚定首个公式格");
    // 相对坐标 (0,0) = 绝对 A2 的公式文本。
    assert_eq!(
        s.formulas.get((0, 0)).map(|s| s.to_string()),
        Some("SUM(B1:B1)".to_string())
    );
    // 相对 (1,0) = 绝对 A3（**不是** A2——相对键错位的根源）。
    assert_eq!(
        s.formulas.get((1, 0)).map(|s| s.to_string()),
        Some("AVERAGE(B1:B1)".to_string())
    );
    // 绝对坐标换算钉定：A2 公式 = start + (0,0)。
    let fs = s.formulas.start().unwrap();
    let a2_formula = s.formulas.get(((1 - fs.0) as usize, (0 - fs.1) as usize));
    assert_eq!(
        a2_formula.map(|s| s.to_string()),
        Some("SUM(B1:B1)".to_string())
    );

    let _ = std::fs::remove_file(&p);
}
