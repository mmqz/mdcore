//! #389 阶段 1：XLSX 网格模型 e2e（真夹具 `smoke.xlsx`——黄金例进仓，
//! 不许 SKIP）。
//!
//! 锁清单（每锁一条 `#[test]`，断言消息讲清**为什么**）：
//! 1. `grid_model_sheets_match_fixture` —— 工作表清单（簿序）与行模型
//!    （`xlsx_doc::load`）逐字一致——两视图同一读面（office-io），
//!    不一致 = 网格化转译丢表/乱序；
//! 2. `grid_model_geometry_and_guards` —— 几何下界（B4 公式 → ≥4 行；
//!    B5:C5 合并 → ≥3 列；A5 超链接 → ≥5 行）+ 护栏零触发（夹具远小
//!    于 1000×64——truncated 必须全 false，非静默截断面零误报）+
//!    `cells` 矩阵形状 = rows × cols（行主序不变量）；
//! 3. `grid_model_content_cross_check` —— 网格非空格数与行模型
//!    props.cells（13 + 4）对账——读面同一计数口径（非 Empty 值格），
//!    两视图内容零漂移的量化锁；
//! 4. `grid_model_from_book_is_pure` —— `from_book` 纯函数性：同一
//!    Book 两次转译逐字段相等（无隐藏状态/坐标漂移）。
//!
//! 夹具结构锚（`render_170_xlsx.rs` 独立 XML 解析钉定，此处消费）：
//! 表 1「销售明细」= 1 公式（B4 SUM）+ 1 合并（B5:C5）+ 1 超链接（A5）
//! + 13 有值格；表 2「参数」= 4 有值格。

use std::path::Path;

use mang_shell::editor::xlsx_doc;
use mang_shell::xlsx_grid_view::XlsxGridModel;

fn fixture(name: &str) -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    if !p.is_file() {
        panic!(
            "夹具缺失（{}）——黄金例进仓，不许 SKIP；\
             由仓库内生成器再生成：cargo test gen_smoke_xlsx -- --ignored",
            p.display()
        );
    }
    p
}

#[test]
fn grid_model_sheets_match_fixture() {
    let model = XlsxGridModel::load(&fixture("smoke.xlsx")).expect("网格模型装载");
    let payload = xlsx_doc::load(&fixture("smoke.xlsx")).expect("行模型装载（对照面）");

    // 工作表清单（簿序）——与行模型工作表树逐字一致（同一读面）。
    let names: Vec<&str> = model.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["销售明细", "参数"],
        "网格面工作表清单（夹具独立 XML 解析钉定——漂移请重新预检）"
    );
    let heading_names: Vec<&str> = payload.headings.iter().map(|h| h.text.as_str()).collect();
    assert_eq!(
        names, heading_names,
        "网格面 ↔ 行模型工作表树簿序一致（同一 office-io 读面，不一致 = 转译丢表/乱序）"
    );
    assert_eq!(payload.props.sheets, 2, "行模型表数对照");
}

#[test]
fn grid_model_geometry_and_guards() {
    let model = XlsxGridModel::load(&fixture("smoke.xlsx")).expect("网格模型装载");

    let s1 = &model.sheets[0];
    let s2 = &model.sheets[1];
    // 几何下界（内容坐标锚——B4 公式 / B5:C5 合并 / A5 超链接）。
    assert!(s1.rows >= 5, "表 1 行数 ≥ 5（A5 超链接锚——得 {}）", s1.rows);
    assert!(
        s1.cols >= 3,
        "表 1 列数 ≥ 3（B5:C5 合并锚——得 {}）",
        s1.cols
    );
    assert!(s2.rows >= 1 && s2.cols >= 1, "表 2 有数据区（4 有值格）");
    // 护栏零触发（夹具远小于 1000 行 × 64 列）。
    assert!(!s1.truncated && !s2.truncated, "夹具量级不该触发渲染护栏");
    assert_eq!(model.truncated_sheets, 0, "护栏计数零误报（非静默面）");
    // 合并计数（行模型 props.merges 同源）。
    assert_eq!(s1.merges, 1, "表 1 合并范围 1（B5:C5）");
    assert_eq!(s2.merges, 0, "表 2 无合并");
    // 矩阵形状不变量：cells = rows × cols 行主序。
    assert_eq!(s1.cells.len(), s1.rows, "表 1 cells 行数");
    assert!(
        s1.cells.iter().all(|r| r.len() == s1.cols),
        "表 1 cells 列数齐整"
    );
    assert_eq!(s2.cells.len(), s2.rows, "表 2 cells 行数");
    assert!(
        s2.cells.iter().all(|r| r.len() == s2.cols),
        "表 2 cells 列数齐整"
    );
}

#[test]
fn grid_model_content_cross_check() {
    let model = XlsxGridModel::load(&fixture("smoke.xlsx")).expect("网格模型装载");
    let payload = xlsx_doc::load(&fixture("smoke.xlsx")).expect("行模型装载（对照面）");

    // 非空格对账：网格显示文本非空串计数 ↔ 行模型 props.cells
    //（13 + 4 = 17——读面非 Empty 值格同一口径；公式格 B4 有缓存值 →
    // 显示值非公式文本，与 cell_display 语义一致）。
    let non_empty: usize = model
        .sheets
        .iter()
        .flat_map(|s| s.cells.iter().flatten())
        .filter(|t| !t.is_empty())
        .count();
    assert_eq!(
        non_empty, payload.props.cells,
        "网格非空格 {} ↔ 行模型 props.cells {}（两视图内容零漂移的量化锁——\
         不一致 = 网格化丢格或计数口径漂移）",
        non_empty, payload.props.cells
    );
    assert_eq!(
        payload.props.cells, 17,
        "夹具钉定：13 + 4 有值格（漂移请重新预检）"
    );

    // 表 1 首格（A1）非空——「A1 显示月份」锚（行模型既有断言面）。
    let s1 = &model.sheets[0];
    assert!(
        !s1.cells[0][0].is_empty(),
        "表 1 A1 应有内容（得 {:?}）",
        s1.cells[0][0]
    );
}

#[test]
fn grid_model_from_book_is_pure() {
    let path = fixture("smoke.xlsx");
    let book = mdc_office_io::io::read_book_with_options(
        &path.to_string_lossy(),
        &mdc_office_io::io::ReadOptions { compat: true },
    )
    .expect("office-io 读面");
    let a = XlsxGridModel::from_book(&book);
    let b = XlsxGridModel::from_book(&book);
    assert_eq!(
        a.sheets.len(),
        b.sheets.len(),
        "同 Book 两次转译表数一致（纯函数性）"
    );
    for (sa, sb) in a.sheets.iter().zip(b.sheets.iter()) {
        assert_eq!(sa.name, sb.name, "表名");
        assert_eq!(
            (sa.start_row, sa.start_col),
            (sb.start_row, sb.start_col),
            "包围盒起点"
        );
        assert_eq!((sa.rows, sa.cols), (sb.rows, sb.cols), "渲染维度");
        assert_eq!(
            sa.cells, sb.cells,
            "显示文本矩阵逐格相等（无隐藏状态/坐标漂移）"
        );
        assert_eq!(sa.truncated, sb.truncated, "护栏标记");
        assert_eq!(sa.merges, sb.merges, "合并计数");
    }
    assert_eq!(a.truncated_sheets, b.truncated_sheets, "护栏计数");
}
