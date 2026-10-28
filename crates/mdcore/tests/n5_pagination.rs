//! N-5（issue #29）：分页预估单元测试——模型数学与分块行消耗钉定。
//!
//! 校准证据链（LO 25.2 实测 14 测点、40 万字双档验收 +0.61%/-2.46%）
//! = `test-artifacts/pagination/CALIBRATION.md`；本文件钉定纯数学面
//!（确定性——无渲染依赖，CI 可跑）。

use mdcore::markdown::markdown_to_tree;
use mdcore::pagination::{body_line_height, estimate, mono_line_height, PaginationParams};

/// 引擎缺省档数学钉定：A4 边距 1985/1701 → 版心 8504×13152 twips；
/// 10pt=200twips → 每行 42 全角位；单倍行高 = 200×(0.3115+1.15) =
/// 292.3 → 每页 floor(13152/292.3) = 44 行（44.997 边界——洛实测容量
/// 区间 [44,45]，钉 floor 值 44；校准报告同口径）。
#[test]
fn engine_default_math() {
    let p = PaginationParams::engine_default();
    assert_eq!(p.text_width, 8504);
    assert_eq!(p.text_height, 13152);
    assert_eq!(p.body_size_twips, 200);
    let tree = markdown_to_tree("单段");
    let est = estimate(&tree, &p);
    assert_eq!(est.chars_per_line, 42, "8504/200 = 42.52 → 42 全角位");
    assert_eq!(
        est.lines_per_page, 44,
        "floor(13152/292.3) = 44（恰在 45 边界下）"
    );
}

/// 行高公式钉定：正文 = 字号×(A+B×倍数)（LO 比例行距 lineGap 面不
/// 缩放——微件差分结构结论）；等宽 = 字号×1.17×倍数。
#[test]
fn line_height_formulas() {
    // 单倍：200 × 1.4615 = 292.3
    assert!((body_line_height(200, 1.0) - 292.3).abs() < 0.01);
    // 1.5 倍（四号 280）：280 × (0.3115+1.725) = 570.22
    assert!((body_line_height(280, 1.5) - 570.22).abs() < 0.01);
    // 等宽单倍：200 × 1.17 = 234
    assert!((mono_line_height(200, 1.0) - 234.0).abs() < 0.01);
    // 等宽 1.5 倍：280 × 1.755 = 491.4
    assert!((mono_line_height(280, 1.5) - 491.4).abs() < 0.01);
    // 等宽恒矮于正文（Courier/ Liberation Mono 度量 < CJK 面）。
    assert!(mono_line_height(200, 1.0) < body_line_height(200, 1.0));
}

/// 分块行消耗：段落折行（全角单位）、标题/空段/列表标记位/引用缩进/
/// 代码字面行/表格最大单元格折行。
#[test]
fn block_line_consumption() {
    let p = PaginationParams::engine_default(); // cpl=42
                                                // 100 全角字 → ceil(100/42) = 3 行。
    let t1 = markdown_to_tree(&"汉".repeat(100));
    let e1 = estimate(&t1, &p);
    assert_eq!(e1.paragraph_lines, 3);

    // 空文档：零块 → 零行消耗，但页数最小 1（防御面）。
    let t2 = markdown_to_tree("");
    let e2 = estimate(&t2, &p);
    assert_eq!(e2.paragraph_lines, 0, "零块零行");
    assert_eq!(e2.pages, 1, "空文档最小 1 页");

    // 半角 0.5 计：84 个 ASCII ≈ 42 单位 = 恰 1 行。
    let t3 = markdown_to_tree(&"a".repeat(84));
    let e3 = estimate(&t3, &p);
    assert_eq!(e3.paragraph_lines, 1, "84 半角 = 42 全角位 = 1 行");

    // 全角标点全宽面：「，」×42 = 42 单位 = 1 行（不折）。
    let t4 = markdown_to_tree(&"，".repeat(42));
    let e4 = estimate(&t4, &p);
    assert_eq!(e4.paragraph_lines, 1);

    // 标题按正文行计（当前链 Heading1-9 无字号面）。
    let t5 = markdown_to_tree("# 标题");
    let e5 = estimate(&t5, &p);
    assert_eq!(e5.heading_lines, 1);
}

/// 代码块 = 字面行数（等宽不折行近似）；表格 = 逐行最大单元格折行。
#[test]
fn code_and_table_lines() {
    let p = PaginationParams::engine_default();
    let t = markdown_to_tree("```\nline1\nline2\nline3\n```\n");
    let e = estimate(&t, &p);
    assert_eq!(e.code_lines, 3, "字面 3 行（尾行发射语义）");

    // 3 列短格 × 2 数据行：每行最大单元格 1 行 → 2 行（GFM 分隔行剔除）。
    let t2 = markdown_to_tree("| a | b | c |\n|---|---|---|\n| 甲 | 乙 | 丙 |\n");
    let e2 = estimate(&t2, &p);
    assert_eq!(e2.table_lines, 2, "表头 + 1 数据行（分隔行不计）");
}

/// 误差带分档：纯段落 5%；重结构（表/码行占比 > 20%）10%。
#[test]
fn error_margin_tiers() {
    let p = PaginationParams::engine_default();
    let pure = markdown_to_tree(&format!("{}\n", "汉".repeat(100)).repeat(20));
    let e1 = estimate(&pure, &p);
    assert_eq!(e1.error_margin_pct, 5.0);

    let heavy = markdown_to_tree(&"```\ncode\n```\n".repeat(50));
    let e2 = estimate(&heavy, &p);
    assert_eq!(e2.error_margin_pct, 10.0, "代码行占比 100% > 20% → 10% 档");
}

/// 段距按段计非按行计：before/after 逐段叠加（段落属性）。
#[test]
fn spacing_per_paragraph() {
    let tree = markdown_to_tree("甲\n\n乙\n\n丙");
    let base = PaginationParams::engine_default();
    let spaced = PaginationParams {
        spacing_before: 100,
        spacing_after: 100,
        ..base
    };
    let e_base = estimate(&tree, &base);
    let e_spaced = estimate(&tree, &spaced);
    // 3 段 × 200 twips = 600 twips 段距 → 页数不小于基线。
    assert!(e_spaced.pages >= e_base.pages);
    assert_eq!(e_spaced.paragraph_lines, e_base.paragraph_lines);
}

/// 行距倍数放大页数（tender 档方向性：1.5 倍页数 > 单倍 × 1.5——
/// A 项不缩放）。
#[test]
fn line_multiple_scales_pages() {
    let tree = markdown_to_tree(&format!("{}\n", "汉".repeat(40)).repeat(200));
    let single = estimate(
        &tree,
        &PaginationParams {
            line_multiple: 1.0,
            ..PaginationParams::engine_default()
        },
    );
    let one_point_five = estimate(
        &tree,
        &PaginationParams {
            line_multiple: 1.5,
            ..PaginationParams::engine_default()
        },
    );
    // 单倍：292.3；1.5 倍：405.65（×1.388——A 项拖底）。
    let ratio = one_point_five.pages as f64 / single.pages as f64;
    assert!(
        ratio > 1.3 && ratio < 1.5,
        "1.5 倍行距页数比 ∈ (1.3, 1.5)：{ratio}"
    );
}
