//! #110 任务 1 验收：**10000 行 × 20 列重算 < 1 秒**。
//!
//! 双档阈值：debug 构建跑得动（30s 档，CI 便捷面）；release 构建
//! 自动收紧到验收线 1s（`cargo test --release -p mdc-formula` 即
//! 验收跑法）。规模 = 200,000 格（19 列常量 + 每行 1 个 SUM 公式
//! + 全列汇总 + 跨表引用），实测耗时打印进 stdout（PR 证据面）。

use mdc_formula::{CellContent, FormulaEngine, SheetInput, Value};
use std::time::Instant;

const ROWS: u32 = 10_000;
const COLS: u32 = 20;

/// 阈值：release 1s（验收线）；debug 30s（无优化档跑通面）。
const LIMIT_SECS: f64 = if cfg!(debug_assertions) { 30.0 } else { 1.0 };

#[test]
fn recalc_10k_x_20_under_1s_release() {
    let mut cells = Vec::with_capacity((ROWS * COLS) as usize + 16);
    for r in 1..=ROWS {
        // 前 19 列：常量（r * c 混合值域）。
        for c in 1..COLS {
            cells.push((
                r,
                c,
                CellContent::Value(Value::Number((r * c % 97) as f64 + 0.5)),
            ));
        }
        // 第 20 列：行汇总公式（T 列）。
        cells.push((r, COLS, CellContent::Formula(format!("=SUM(A{r}:S{r})"))));
    }
    // 全列汇总 + 跨表引用验证格（重算正确性抽检）。
    cells.push((1, COLS + 2, CellContent::Formula("=SUM(T1:T10000)".into())));
    let data = SheetInput::new("Data", cells);
    let report = SheetInput::new(
        "Report",
        vec![(1, 1, CellContent::Formula("=Data!T1+Data!T2".into()))],
    );

    let built = Instant::now();
    let mut e = FormulaEngine::new(&[data, report]).expect("构建失败");
    let build_secs = built.elapsed().as_secs_f64();

    // 全簿重算计时（构建时已含一次首算——这里测显式重算全程）。
    let t = Instant::now();
    e.recalculate();
    let recalc_secs = t.elapsed().as_secs_f64();

    println!(
        "[perf] {ROWS}x{COLS} = {} 格 | 构建+首算 {build_secs:.3}s | 重算 {recalc_secs:.3}s | 阈值 {LIMIT_SECS}s（{}）",
        ROWS * COLS,
        if cfg!(debug_assertions) { "debug 档" } else { "release 验收档" }
    );
    assert!(
        recalc_secs < LIMIT_SECS,
        "重算 {recalc_secs:.3}s 超阈值 {LIMIT_SECS}s（{} 档）",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    // 正确性抽检（防「快而错」）：行 1 汇总 = sum(1..=19 列值)。
    let expect_row1: f64 = (1..COLS).map(|c| (c % 97) as f64 + 0.5).sum();
    assert_eq!(
        e.cell_value(0, 1, COLS).unwrap(),
        Value::Number(expect_row1)
    );
    // 行 10000 汇总。
    let expect_row10k: f64 = (1..COLS).map(|c| (10_000 * c % 97) as f64 + 0.5).sum();
    assert_eq!(
        e.cell_value(0, 10_000, COLS).unwrap(),
        Value::Number(expect_row10k)
    );
    // 全列汇总 = 各行汇总之和。
    let expect_total: f64 = (1..=ROWS)
        .map(|r| (1..COLS).map(|c| (r * c % 97) as f64 + 0.5).sum::<f64>())
        .sum();
    match e.cell_value(0, 1, COLS + 2).unwrap() {
        Value::Number(n) => assert!(
            (n - expect_total).abs() < 1e-6 * expect_total.abs().max(1.0),
            "全列汇总 {n} ≠ 期望 {expect_total}"
        ),
        other => panic!("全列汇总期望数字，得 {other:?}"),
    }
    // 跨表引用抽检。
    let t1: f64 = (1..COLS).map(|c| (c % 97) as f64 + 0.5).sum();
    let t2: f64 = (1..COLS).map(|c| (2 * c % 97) as f64 + 0.5).sum();
    assert_eq!(e.cell_value(1, 1, 1).unwrap(), Value::Number(t1 + t2));
}

#[test]
fn edit_one_cell_full_recalc_latency() {
    /// 编辑单格后的全簿重算延迟（任务 2 编辑路径预算参考——v0 全簿
    /// 语义下的最坏情况测量，非增量）。
    use std::time::Instant;
    let mut cells = Vec::new();
    for r in 1..=ROWS {
        for c in 1..COLS {
            cells.push((
                r,
                c,
                CellContent::Value(Value::Number((r * c % 97) as f64 + 0.5)),
            ));
        }
        cells.push((r, COLS, CellContent::Formula(format!("=SUM(A{r}:S{r})"))));
    }
    cells.push((1, COLS + 2, CellContent::Formula("=SUM(T1:T10000)".into())));
    let mut e = FormulaEngine::new(&[SheetInput::new("Data", cells)]).expect("构建失败");

    let t = Instant::now();
    e.set_cell(0, 500, 10, CellContent::Value(Value::Number(42.0)))
        .unwrap();
    e.recalculate();
    let secs = t.elapsed().as_secs_f64();
    println!(
        "[perf] 单格编辑 + 全簿重算 = {secs:.3}s（{} 档）",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    // 编辑路径预算：debug 30s / release 3s（比首算宽——含传播）。
    let limit = if cfg!(debug_assertions) { 30.0 } else { 3.0 };
    assert!(secs < limit, "编辑+重算 {secs:.3}s 超预算 {limit}s");
    // 改动确实生效（J500=42 → 行 500 汇总变化）。
    let expect: f64 = (1..COLS)
        .map(|c| {
            if c == 10 {
                42.0
            } else {
                (500 * c % 97) as f64 + 0.5
            }
        })
        .sum();
    assert_eq!(e.cell_value(0, 500, COLS).unwrap(), Value::Number(expect));
}
