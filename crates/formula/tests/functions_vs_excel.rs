//! #110 任务 1 验收：**36 个必备函数全部正确**（Excel 结果对照）。
//!
//! 期望值锚定纪律：全部期望值经独立计算预验证（Python 日期/算术
//! 复核，见 PR 描述），非「跑出来什么记什么」。浮点边界值
//! （`ROUND(1.005,2)` 类二进制表示歧义面）不入对照集——用无歧义
//! 值钉定语义（half-away-from-zero 等）。
//!
//! 布局：`Data` 数字/标签/条件列，`T` 横向查找表与文本，`Calc`
//! 公式集中区（逐函数编号，A 列 = 函数序号所在行，公式放 B 列，
//! 便于失败定位）。

use mdc_formula::{CellContent, FormulaEngine, SheetInput, Value};

fn data_sheet() -> SheetInput {
    let v = |n: f64| CellContent::Value(Value::Number(n));
    let t = |s: &str| CellContent::Value(Value::Text(s.to_string()));
    SheetInput::new(
        "Data",
        vec![
            // A 列：数字 1-5；B 列：标签（查找表）；C 列：求和列；D 列：条件列。
            (1, 1, v(1.0)),
            (1, 2, t("一")),
            (1, 3, v(10.0)),
            (1, 4, t("X")),
            (2, 1, v(2.0)),
            (2, 2, t("二")),
            (2, 3, v(20.0)),
            (2, 4, t("Y")),
            (3, 1, v(3.0)),
            (3, 2, t("三")),
            (3, 3, v(30.0)),
            (3, 4, t("X")),
            (4, 1, v(4.0)),
            (4, 2, t("四")),
            (4, 3, v(40.0)),
            (4, 4, t("Y")),
            (5, 1, v(5.0)),
            (5, 2, t("五")),
            (5, 3, v(50.0)),
            (5, 4, t("X")),
        ],
    )
}

fn t_sheet() -> SheetInput {
    let v = |n: f64| CellContent::Value(Value::Number(n));
    let t = |s: &str| CellContent::Value(Value::Text(s.to_string()));
    SheetInput::new(
        "T",
        vec![
            // 横向查找表：第 1 行键 1/2/3，第 2 行值 甲/乙/丙；A4 文本。
            (1, 1, v(1.0)),
            (1, 2, v(2.0)),
            (1, 3, v(3.0)),
            (2, 1, t("甲")),
            (2, 2, t("乙")),
            (2, 3, t("丙")),
            (4, 1, t("abcdef")),
        ],
    )
}

fn calc_engine(formulas: &[(u32, u32, &str)]) -> FormulaEngine {
    let cells: Vec<(u32, u32, CellContent)> = formulas
        .iter()
        .map(|&(r, c, f)| (r, c, CellContent::Formula(f.to_string())))
        .collect();
    let mut e = FormulaEngine::new(&[data_sheet(), t_sheet(), SheetInput::new("Calc", cells)])
        .expect("构建失败");
    e.recalculate();
    e
}

fn num(e: &FormulaEngine, row: u32) -> f64 {
    match e.cell_value(2, row, 2).expect("读值失败") {
        Value::Number(n) => n,
        other => panic!("行 {row} 期望数字，得 {other:?}"),
    }
}

fn text(e: &FormulaEngine, row: u32) -> String {
    match e.cell_value(2, row, 2).expect("读值失败") {
        Value::Text(s) => s,
        other => panic!("行 {row} 期望文本，得 {other:?}"),
    }
}

fn boolean(e: &FormulaEngine, row: u32) -> bool {
    match e.cell_value(2, row, 2).expect("读值失败") {
        Value::Bool(b) => b,
        other => panic!("行 {row} 期望布尔，得 {other:?}"),
    }
}

#[test]
fn math_family() {
    let e = calc_engine(&[
        (1, 2, "=SUM(Data!A1:A5)"),
        (2, 2, "=SUM(1,2,3,4,5,6,7,8,9,10)"),
        (3, 2, "=AVERAGE(1,2,3,4)"),
        (4, 2, "=MIN(3,1,2)"),
        (5, 2, "=MAX(3,1,2)"),
        (6, 2, "=COUNT(Data!A1:A5,Data!B1:B5)"),
        (7, 2, "=COUNTA(Data!A1:A5,Data!B1:B5)"),
        (8, 2, "=ROUND(PI(),2)"),
        (9, 2, "=ROUND(2.5,0)"),
        (10, 2, "=ROUND(-2.5,0)"),
        (11, 2, "=ABS(-7)"),
        (12, 2, "=INT(8.9)"),
        (13, 2, "=INT(-8.9)"),
    ]);
    assert_eq!(num(&e, 1), 15.0, "SUM 区域");
    assert_eq!(num(&e, 2), 55.0, "SUM 字面");
    assert_eq!(num(&e, 3), 2.5, "AVERAGE");
    assert_eq!(num(&e, 4), 1.0, "MIN");
    assert_eq!(num(&e, 5), 3.0, "MAX");
    // COUNT 只计数字（标签列 5 格不计）。
    assert_eq!(num(&e, 6), 5.0, "COUNT 数字计数");
    // COUNTA 计非空（数字 5 + 标签 5）。
    assert_eq!(num(&e, 7), 10.0, "COUNTA 非空计数");
    assert_eq!(num(&e, 8), 314.0 / 100.0, "ROUND 常规（PI→2 位）");
    assert_eq!(num(&e, 9), 3.0, "ROUND half-away-from-zero（2.5→3）");
    assert_eq!(num(&e, 10), -3.0, "ROUND 负数 half-away（-2.5→-3）");
    assert_eq!(num(&e, 11), 7.0, "ABS");
    assert_eq!(num(&e, 12), 8.0, "INT 正数截断");
    assert_eq!(num(&e, 13), -9.0, "INT 负数向下（-8.9→-9，与 TRUNC 区分）");
}

#[test]
fn logical_family() {
    let e = calc_engine(&[
        (1, 2, "=IF(2>1,\"yes\",\"no\")"),
        (2, 2, "=IF(1>2,\"yes\",\"no\")"),
        (3, 2, "=IFS(7>10,\"大\",7>5,\"中\",TRUE,\"小\")"),
        (4, 2, "=IFS(3>10,\"大\",3>5,\"中\",TRUE,\"小\")"),
        (5, 2, "=AND(TRUE,1)"),
        (6, 2, "=AND(1,0)"),
        (7, 2, "=OR(0,0)"),
        (8, 2, "=OR(0,1)"),
        (9, 2, "=NOT(0)"),
        (10, 2, "=NOT(1)"),
    ]);
    assert_eq!(text(&e, 1), "yes");
    assert_eq!(text(&e, 2), "no");
    assert_eq!(text(&e, 3), "中", "IFS 命中第二分支");
    assert_eq!(text(&e, 4), "小", "IFS 落到 TRUE 兜底");
    assert!(boolean(&e, 5), "AND 非零数字=TRUE 语义");
    assert!(!boolean(&e, 6), "AND 零=FALSE");
    assert!(!boolean(&e, 7));
    assert!(boolean(&e, 8));
    assert!(boolean(&e, 9));
    assert!(!boolean(&e, 10));
}

#[test]
fn lookup_family() {
    let e = calc_engine(&[
        (1, 2, "=VLOOKUP(2,Data!A1:B5,2,FALSE)"),
        (2, 2, "=VLOOKUP(2.5,Data!A1:B5,2,TRUE)"),
        (3, 2, "=VLOOKUP(9,Data!A1:B5,2,TRUE)"),
        (4, 2, "=HLOOKUP(3,T!A1:C2,2,FALSE)"),
        (5, 2, "=INDEX(Data!A1:A5,3)"),
        (6, 2, "=INDEX(Data!B1:B5,2)"),
        (7, 2, "=MATCH(3,Data!A1:A5,0)"),
        (8, 2, "=MATCH(2.5,Data!A1:A5,1)"),
    ]);
    assert_eq!(text(&e, 1), "二", "VLOOKUP 精确");
    assert_eq!(text(&e, 2), "二", "VLOOKUP 近似区间（2.5 → ≤2.5 最大行）");
    assert_eq!(text(&e, 3), "五", "VLOOKUP 近似超上限（9 → 末行）");
    assert_eq!(text(&e, 4), "丙", "HLOOKUP 精确");
    assert_eq!(num(&e, 5), 3.0, "INDEX 数字列");
    assert_eq!(text(&e, 6), "二", "INDEX 标签列");
    assert_eq!(num(&e, 7), 3.0, "MATCH 精确位置");
    assert_eq!(num(&e, 8), 2.0, "MATCH 近似位置（2.5 → ≤2.5 的末位）");
}

#[test]
fn text_family() {
    let e = calc_engine(&[
        (1, 2, "=CONCATENATE(\"a\",1,TRUE)"),
        (2, 2, "=LEFT(T!A4,2)"),
        (3, 2, "=RIGHT(T!A4,2)"),
        (4, 2, "=MID(T!A4,2,3)"),
        (5, 2, "=LEN(T!A4)"),
        (6, 2, "=TRIM(\"  a  b  \")"),
        (7, 2, "=TEXT(1234.567,\"0.00\")"),
        (8, 2, "=TEXT(0.25,\"0%\")"),
    ]);
    assert_eq!(text(&e, 1), "a1TRUE", "CONCATENATE 隐式文本化（数字/布尔）");
    assert_eq!(text(&e, 2), "ab");
    assert_eq!(text(&e, 3), "ef");
    assert_eq!(text(&e, 4), "bcd");
    assert_eq!(num(&e, 5), 6.0);
    assert_eq!(text(&e, 6), "a b", "TRIM 两端剥除 + 中间连续空格压一");
    assert_eq!(text(&e, 7), "1234.57", "TEXT 数值格式化四舍五入");
    assert_eq!(text(&e, 8), "25%", "TEXT 百分比");
}

#[test]
fn datetime_family() {
    // Excel 序列数：1900 纪元（1899-12-30 = 0），2026-10-31 = 46326
    // （Python datetime 独立复核）。
    let e = calc_engine(&[
        (1, 2, "=DATE(2026,10,31)"),
        (2, 2, "=YEAR(DATE(2026,10,31))"),
        (3, 2, "=MONTH(DATE(2026,10,31))"),
        (4, 2, "=DAY(DATE(2026,10,31))"),
        (5, 2, "=TODAY()"),
        (6, 2, "=NOW()"),
    ]);
    assert_eq!(num(&e, 1), 46_326.0, "DATE 序列数（1900 纪元）");
    assert_eq!(num(&e, 2), 2026.0);
    assert_eq!(num(&e, 3), 10.0);
    assert_eq!(num(&e, 4), 31.0);
    // TODAY/NOW 真时钟——域断言（2026 年序列数 46000+，带时间小数）。
    let today = num(&e, 5);
    assert!(
        today > 46_000.0 && today < 50_000.0,
        "TODAY 域异常：{today}"
    );
    let now = num(&e, 6);
    assert!(now > 46_000.0 && now < 50_000.0, "NOW 域异常：{now}");
    assert!(now >= today, "NOW ≥ TODAY");
}

#[test]
fn criteria_family() {
    let e = calc_engine(&[
        (1, 2, "=COUNTIF(Data!A1:A5,\">2\")"),
        (2, 2, "=COUNTIF(Data!D1:D5,\"X\")"),
        (3, 2, "=SUMIF(Data!A1:A5,\">2\")"),
        (4, 2, "=SUMIF(Data!D1:D5,\"X\",Data!C1:C5)"),
        (5, 2, "=COUNTIFS(Data!A1:A5,\">=2\",Data!A1:A5,\"<=4\")"),
        (
            6,
            2,
            "=SUMIFS(Data!C1:C5,Data!A1:A5,\">=2\",Data!D1:D5,\"X\")",
        ),
        (7, 2, "=AVERAGEIF(Data!A1:A5,\">2\")"),
        (8, 2, "=COUNTIFS(Data!D1:D5,\"X\",Data!A1:A5,\">3\")"),
    ]);
    assert_eq!(num(&e, 1), 3.0, "COUNTIF 数字条件");
    assert_eq!(num(&e, 2), 3.0, "COUNTIF 文本条件");
    assert_eq!(num(&e, 3), 12.0, "SUMIF 数字条件（3+4+5）");
    assert_eq!(num(&e, 4), 90.0, "SUMIF 文本条件 + 求和列（10+30+50）");
    assert_eq!(num(&e, 5), 3.0, "COUNTIFS 双条件（2,3,4）");
    assert_eq!(num(&e, 6), 80.0, "SUMIFS（A≥2 且 D=X：行 3+行 5 = 30+50）");
    assert_eq!(num(&e, 7), 4.0, "AVERAGEIF（(3+4+5)/3）");
    assert_eq!(num(&e, 8), 1.0, "COUNTIFS 交叉条件（X 且 A>3：仅行 5）");
}

#[test]
fn error_semantics_typed() {
    // 错误码族对照（#110 坑表：#DIV/0! 沿链传播已在引擎内嵌测试；
    // 此处钉定查找/名称/数值错误码 typed 面）。
    let e = calc_engine(&[
        (1, 2, "=VLOOKUP(99,Data!A1:B5,2,FALSE)"),
        (2, 2, "=NOSUCHFUNC(1)"),
        (3, 2, "=1/0"),
    ]);
    assert!(
        matches!(
            e.cell_value(2, 1, 2).unwrap(),
            Value::Error(mdc_formula::FormulaError::Na)
        ),
        "VLOOKUP 精确未命中 = #N/A"
    );
    let unknown_fn = e.cell_value(2, 2, 2).unwrap();
    assert!(
        matches!(unknown_fn, Value::Error(mdc_formula::FormulaError::Name))
            || matches!(
                unknown_fn,
                Value::Error(mdc_formula::FormulaError::NotImplemented)
            ),
        "未知函数 = #NAME?/#NIMPL（引擎语义面）：{unknown_fn:?}"
    );
    assert!(matches!(
        e.cell_value(2, 3, 2).unwrap(),
        Value::Error(mdc_formula::FormulaError::DivByZero)
    ));
}
