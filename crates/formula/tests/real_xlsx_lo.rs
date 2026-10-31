//! #110 任务 1 验收：**真实 xlsx 验证**（含公式的财务表）。
//!
//! 语料 = `test-artifacts/formula-financial/`（季度财务模型，~50 公式：
//! SUM 行小计 / SUMIF 类别汇总 / IF 除零防护比率 / 环比）。
//!
//! 双层验证：
//!
//! 1. **恒跑**（无环境依赖）：`fin_lo.xlsx`（soffice 预重算，缓存值 =
//!    LibreOffice 真值 oracle）→ calamine 读公式 + 缓存值 → mdc-formula
//!    重算 → 逐公式格对照（相对容差 1e-9——引擎求和顺序 ulp 级差异）。
//! 2. **环境门**（soffice 在位才跑）：`fin_raw.xlsx`（公式无缓存值）→
//!    即时 soffice 转换重算 → 同样对照（链路活性——防入库 oracle 陈旧）。

use calamine::{Data, Reader, Xlsx};
use mdc_formula::{CellContent, FormulaEngine, SheetInput, Value};
use std::path::Path;
use std::process::Command;

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test-artifacts/formula-financial"
);

/// calamine 读取产物：`(行数, 列数, 值矩阵, 公式矩阵)`——公式文本
/// 补 `=` 前缀（xlsx `<f>` 存储面无前缀，mdc-formula 入参带前缀）。
struct LoadedBook {
    rows: usize,
    cols: usize,
    values: Vec<Vec<Data>>,
    formulas: Vec<Vec<String>>,
}

fn load_xlsx(path: &Path) -> Result<LoadedBook, Box<dyn std::error::Error>> {
    let mut book: Xlsx<_> = calamine::open_workbook(path)?;
    let values = book.worksheet_range("Fin")?;
    let formulas = book.worksheet_formula("Fin")?;
    // **绝对坐标归一**：calamine `Range::get((r, c))` 相对各自网格
    // 左上角（values 起点 = A1，formulas 起点 = 首个公式格 G2——
    // 两网格不同源！office-io `formula_at` 同款 start 换算）。
    let vs = values.start().unwrap_or((0, 0));
    let fs = formulas.start().unwrap_or((0, 0));
    let v_end = (
        vs.0 as usize + values.height(),
        vs.1 as usize + values.width(),
    );
    let f_end = (
        fs.0 as usize + formulas.height(),
        fs.1 as usize + formulas.width(),
    );
    let rows = v_end.0.max(f_end.0);
    let cols = v_end.1.max(f_end.1);
    let mut v_grid = vec![vec![Data::Empty; cols]; rows];
    let mut f_rows = vec![vec![String::new(); cols]; rows];
    for r in 0..rows {
        for c in 0..cols {
            let vr = r.checked_sub(vs.0 as usize);
            let vc = c.checked_sub(vs.1 as usize);
            if let (Some(vr), Some(vc)) = (vr, vc) {
                if let Some(v) = values.get((vr, vc)) {
                    v_grid[r][c] = v.clone();
                }
            }
            let fr = r.checked_sub(fs.0 as usize);
            let fc = c.checked_sub(fs.1 as usize);
            if let (Some(fr), Some(fc)) = (fr, fc) {
                if let Some(f) = formulas.get((fr, fc)) {
                    if !f.is_empty() {
                        f_rows[r][c] = format!("={f}");
                    }
                }
            }
        }
    }
    Ok(LoadedBook {
        rows,
        cols,
        values: v_grid,
        formulas: f_rows,
    })
}

/// 引擎重算 vs LibreOffice 缓存值逐公式格对照。返回对照格数。
fn compare_engine_vs_cached(book: &LoadedBook) -> Result<usize, String> {
    // 灌入：公式格给公式，非公式格给常量值。
    let mut cells = Vec::new();
    for r in 0..book.rows {
        for c in 0..book.cols {
            let formula = &book.formulas[r][c];
            if !formula.is_empty() {
                cells.push((
                    (r + 1) as u32,
                    (c + 1) as u32,
                    CellContent::Formula(formula.clone()),
                ));
            } else {
                if let Some(content) = data_to_content(&book.values[r][c]) {
                    cells.push(((r + 1) as u32, (c + 1) as u32, content));
                }
            }
        }
    }
    let mut engine = FormulaEngine::new(&[SheetInput::new("Fin", cells)])
        .map_err(|e| format!("构建失败：{e}"))?;
    engine.recalculate();

    let mut compared = 0usize;
    let mut mismatches = Vec::new();
    for r in 0..book.rows {
        for c in 0..book.cols {
            if book.formulas[r][c].is_empty() {
                continue;
            }
            let (row, col) = ((r + 1) as u32, (c + 1) as u32);
            let lo_cached = &book.values[r][c];
            let ours = engine
                .cell_value(0, row, col)
                .map_err(|e| format!("读值失败 {row},{col}：{e}"))?;
            compared += 1;
            match (lo_cached, &ours) {
                (Data::Float(lo), Value::Number(ours_n)) => {
                    let scale = lo.abs().max(ours_n.abs()).max(1.0);
                    if (lo - ours_n).abs() > 1e-9 * scale {
                        mismatches.push(format!(
                            "{},{} 公式 {} LO={} 我们={}",
                            row, col, book.formulas[r][c], lo, ours_n
                        ));
                    }
                }
                (Data::Int(lo), Value::Number(ours_n)) => {
                    let (lo_f, scale) = (*lo as f64, (*lo as f64).abs().max(1.0));
                    if (lo_f - ours_n).abs() > 1e-9 * scale {
                        mismatches.push(format!(
                            "{},{} 公式 {} LO={} 我们={}",
                            row, col, book.formulas[r][c], lo_f, ours_n
                        ));
                    }
                }
                (Data::String(lo), Value::Text(ours_s)) => {
                    if lo != ours_s {
                        mismatches.push(format!(
                            "{},{} 公式 {} LO={lo:?} 我们={ours_s:?}",
                            row, col, book.formulas[r][c]
                        ));
                    }
                }
                (Data::Empty, Value::Empty) => {}
                other => mismatches.push(format!(
                    "{},{} 公式 {} 类型面 LO/我们 = {other:?}",
                    row, col, book.formulas[r][c]
                )),
            }
        }
    }
    if !mismatches.is_empty() {
        return Err(format!(
            "重算与 LibreOffice 真值不符（{} 处）：\n  {}",
            mismatches.len(),
            mismatches.join("\n  ")
        ));
    }
    Ok(compared)
}

fn data_to_content(d: &Data) -> Option<CellContent> {
    Some(match d {
        Data::Empty => return None,
        Data::Float(f) => CellContent::Value(Value::Number(*f)),
        Data::Int(i) => CellContent::Value(Value::Number(*i as f64)),
        Data::String(s) => CellContent::Value(Value::Text(s.clone())),
        Data::Bool(b) => CellContent::Value(Value::Bool(*b)),
        // 日期/错误缓存值在财务模型语料中不出现（常量面纯数字/文本）。
        other => return Err(format!("语料常量面出现未预期类型：{other:?}")).ok()?,
    })
}

#[test]
fn financial_model_vs_libreoffice_cached_truth() {
    let path = Path::new(FIXTURE_DIR).join("fin_lo.xlsx");
    if !path.exists() {
        panic!(
            "真值语料缺失：{}（重建：python3 test-artifacts/formula-financial/gen_fin_fixture.py）",
            path.display()
        );
    }
    let book = load_xlsx(&path).expect("calamine 读取失败");
    let compared = compare_engine_vs_cached(&book).expect("对照失败");
    // 模型 ~50 公式；下限护栏防「对照了 0 格也算过」。
    assert!(compared >= 45, "对照公式格数 {compared} 异常（预期 ≥45）");
    println!(
        "[real-xlsx] 财务模型 {} 公式格 vs LibreOffice 缓存值全部一致",
        compared
    );

    // 手算锚点（生成器 README 钉定值，Python 独立复核）+ 编辑重算
    // 闭环（任务 2 预演）。
    fn num_at(e: &FormulaEngine, cell: &str) -> f64 {
        match e.evaluate(0, cell).unwrap() {
            Value::Number(n) => n,
            other => panic!("{cell} 期望数字，得 {other:?}"),
        }
    }
    let mut cells = Vec::new();
    for r in 0..book.rows {
        for c in 0..book.cols {
            let f = &book.formulas[r][c];
            if !f.is_empty() {
                cells.push((
                    (r + 1) as u32,
                    (c + 1) as u32,
                    CellContent::Formula(f.clone()),
                ));
            } else if let Some(content) = data_to_content(&book.values[r][c]) {
                cells.push(((r + 1) as u32, (c + 1) as u32, content));
            }
        }
    }
    let mut engine = FormulaEngine::new(&[SheetInput::new("Fin", cells)]).unwrap();
    engine.recalculate();
    assert_eq!(
        num_at(&engine, "C8"),
        165_000.0,
        "Q1 收入合计（120000+45000）"
    );
    assert_eq!(num_at(&engine, "C11"), 66_000.0, "Q1 毛利（165000-99000）");
    assert!(
        (num_at(&engine, "C12") - 0.4).abs() < 1e-12,
        "Q1 毛利率 0.4"
    );
    assert_eq!(
        num_at(&engine, "G8"),
        755_500.0,
        "全年收入合计（165000+183000+193500+214000）"
    );
    assert_eq!(
        num_at(&engine, "G13"),
        213_200.0,
        "全年净利润（46000+51700+54400+61100）"
    );
    assert!(
        (num_at(&engine, "G16") - 0.105943152454778).abs() < 1e-9,
        "Q4 收入环比"
    );

    // 编辑重算闭环：改 C2（Q1 产品收入 120000→130000）→ 收入合计/
    // 毛利率级联更新（依赖链验收在真模型上的复现）。
    engine
        .set_cell(0, 2, 3, CellContent::Value(Value::Number(130_000.0)))
        .unwrap();
    engine.recalculate();
    assert_eq!(num_at(&engine, "C8"), 175_000.0, "改后 Q1 收入合计");
    assert_eq!(
        num_at(&engine, "C11"),
        76_000.0,
        "改后 Q1 毛利（175000-99000）"
    );
    assert!(
        (num_at(&engine, "C12") - 76_000.0 / 175_000.0).abs() < 1e-12,
        "改后毛利率级联"
    );
}

#[test]
fn soffice_live_recalc_pipeline() {
    // 环境门（bid-parse 式）：soffice 不在位 → SKIP。
    let probe = Command::new("soffice").arg("--version").output();
    if probe.is_err() {
        eprintln!("[real-xlsx] soffice 缺失 — SKIP（环境门：fin_lo.xlsx 恒跑层已覆盖真值对照）");
        return;
    }
    let raw = Path::new(FIXTURE_DIR).join("fin_raw.xlsx");
    assert!(raw.exists(), "原始件缺失：{}", raw.display());
    // 隔离用户配置目录（并发/残留锁面）。
    let out_dir = std::env::temp_dir().join("mdc-formula-soffice-live");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let status = Command::new("soffice")
        .args([
            "--headless",
            &format!(
                "-env:UserInstallation=file:///{}",
                out_dir.join("lo-profile").display()
            ),
            "--convert-to",
            "xlsx",
            "--outdir",
            out_dir.to_str().unwrap(),
            raw.to_str().unwrap(),
        ])
        .output()
        .expect("soffice 调用失败");
    assert!(
        status.status.success(),
        "soffice 转换失败：{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let live = out_dir.join("fin_raw.xlsx");
    let book = load_xlsx(&live).expect("soffice 产物读取失败");
    let compared = compare_engine_vs_cached(&book).expect("活性对照失败");
    println!(
        "[real-xlsx] soffice 即时重算链路：{} 公式格一致（活性复核）",
        compared
    );
    assert!(compared >= 45);
    let _ = std::fs::remove_dir_all(&out_dir);
}
