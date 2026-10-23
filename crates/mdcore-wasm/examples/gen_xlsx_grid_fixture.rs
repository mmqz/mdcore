//! 阶段 5 §八.3 e2e 夹具生成器（表格 UI 产品化）：样例 XLSX →
//! `web/tests/fixtures/grid_sample.xlsx`（web/tests/xlsx_grid.py 消费）。
//! 产线 = office-io `WorkbookWriter`（与 gen_xlsx_fixture.rs 同模式，
//! C2 写面直出——写→读全环复证）。
//!
//! ## 确定性值函数（双侧镜像契约）
//! Sheet "data"（300 行 × 40 列，全格非空 → 使用区恰 300×40）：
//!   - r=0 表头行：text "H{c}"；c=0 首列：text "R{r}"
//!   - 其余 (r,c)：类型函数 (r*4+c*2)%5 ——
//!     0→number r*100+c（c 偶 +0.5；r 奇取负——负数/小数混合）
//!     1→text "cell-{r}-{c}"；2→bool (r+c)%2==0；
//!     3→text 前导零 "00{r}{c}"（≥3 个前导零样本）；4→number r+c
//!
//! **注意**：web/tests/xlsx_grid.py 的 `expected(r,c)` 是本函数的
//! Python 镜像（display 断言期望值）——改动必须双侧同步。
//!
//! Sheet "types"（4×3 小表）：公式两格（1/0 缓存 #DIV/0! / SUM(1,2)
//! 缓存 3）+ datetime 模拟（number 序列 + Custom numFmt）+ 合并 2×1
//! （(3,0)-(3,1)）+ 超链接（https://github.com）。
//!
//! 用法（仓库根）：`cargo run -p mdcore-wasm --example gen_xlsx_grid_fixture`
//!（可选显式路径参数；默认 web/tests/fixtures/grid_sample.xlsx）。
//! 生成后自检：read_book_from_bytes → sheets_core 断言使用区 300×40
//!（夹具不过读面门则 panic，不带病出厂）。

use mdc_office_io::io::read_book_from_bytes;
use mdc_office_io::write::{
    CellStyle, CellValue, FormulaCache, HyperlinkSpec, NumFmt, WorkbookWriter,
};

const DATA_ROWS: u32 = 300;
const DATA_COLS: u16 = 40;

/// 确定性值函数（与 web/tests/xlsx_grid.py::expected 双侧镜像）。
fn data_cell(r: u32, c: u16) -> CellValue {
    if r == 0 {
        return CellValue::Text(format!("H{c}"));
    }
    if c == 0 {
        return CellValue::Text(format!("R{r}"));
    }
    match (r * 4 + u32::from(c) * 2) % 5 {
        0 => {
            // number：r*100+c，c 偶 +0.5（小数混合），r 奇取负（负数混合）
            let mut v = f64::from(r * 100 + u32::from(c));
            if c.is_multiple_of(2) {
                v += 0.5;
            }
            if r % 2 == 1 {
                v = -v;
            }
            CellValue::Number(v)
        }
        1 => CellValue::Text(format!("cell-{r}-{c}")),
        2 => CellValue::Boolean((r + u32::from(c)).is_multiple_of(2)),
        3 => CellValue::Text(format!("00{r}{c}")), // 前导零文本（读回原样）
        _ => CellValue::Number(f64::from(r + u32::from(c))),
    }
}

fn build_grid_sample_xlsx() -> Vec<u8> {
    let mut w = WorkbookWriter::new();

    // ---- Sheet "data"：300×40 全格确定性值 ----
    {
        let mut s = w.add_sheet("data").expect("add sheet data");
        for r in 0..DATA_ROWS {
            for c in 0..DATA_COLS {
                s.write(r, c, data_cell(r, c), None)
                    .unwrap_or_else(|e| panic!("write ({r},{c}): {e}"));
            }
        }
    }

    // ---- Sheet "types"：4×3 类型面小表 ----
    {
        let mut s = w.add_sheet("types").expect("add sheet types");
        s.write(0, 0, CellValue::Text("kind".into()), None)
            .expect("write (0,0)");
        s.write(0, 1, CellValue::Text("sample".into()), None)
            .expect("write (0,1)");
        s.write(0, 2, CellValue::Text("note".into()), None)
            .expect("write (0,2)");

        // 公式两格：error kind 只能来自公式缓存（写面无 Error 值形态）
        s.write(1, 0, CellValue::Text("formula".into()), None)
            .expect("write (1,0)");
        s.write(
            1,
            1,
            CellValue::Formula {
                text: "1/0".into(),
                cached: Some(FormulaCache::Text("#DIV/0!".into())),
            },
            None,
        )
        .expect("write (1,1)");
        s.write(
            1,
            2,
            CellValue::Formula {
                text: "SUM(1,2)".into(),
                cached: Some(FormulaCache::Number(3.0)),
            },
            None,
        )
        .expect("write (1,2)");

        // datetime 模拟：number 序列 + DATE_REPLAY_FORMAT 形态自定义样式
        s.write(2, 0, CellValue::Text("datetime".into()), None)
            .expect("write (2,0)");
        let date_style = CellStyle {
            num_fmt: Some(NumFmt::Custom("yyyy-mm-dd".into())),
            ..Default::default()
        };
        s.write(2, 1, CellValue::Number(44927.0), Some(&date_style))
            .expect("write (2,1)"); // 2023-01-01 序列
        s.write(2, 2, CellValue::Text("2023-01-01".into()), None)
            .expect("write (2,2)");

        // 合并 2×1（(3,0)-(3,1)，首格文本 + 范围 blank 填充）
        s.merge(3, 0, 3, 1, "merged cell", None)
            .expect("merge (3,0)-(3,1)");
        // 超链接（display = URL 本串）
        s.write_hyperlink(
            3,
            2,
            HyperlinkSpec {
                url: "https://github.com".into(),
                display_text: None,
                tooltip: None,
            },
        )
        .expect("write hyperlink (3,2)");
    }

    w.save_to_vec().expect("save_to_vec")
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "web/tests/fixtures/grid_sample.xlsx".to_string());
    let bytes = build_grid_sample_xlsx();

    // 生成后自检：读面 → 使用区维度断言（夹具不带病出厂）
    let book = read_book_from_bytes(&bytes)
        .unwrap_or_else(|e| panic!("fixture self-check read failed: {e}"));
    let sheets = mdcore_wasm::xlsx_grid::sheets_core(&book);
    assert_eq!(
        sheets, r#"[{"name":"data","rows":300,"cols":40},{"name":"types","rows":4,"cols":3}]"#,
        "used-range mismatch"
    );

    let path = std::path::Path::new(&out);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("create fixture dir {}: {e}", dir.display()));
    }
    std::fs::write(path, &bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    println!(
        "fixture written: {} ({} bytes)",
        path.display(),
        bytes.len()
    );
    println!(
        "sheets: data {}x{} ({} cells), types 4x3; used-range self-check passed",
        DATA_ROWS,
        DATA_COLS,
        u64::from(DATA_ROWS) * u64::from(DATA_COLS)
    );
}
