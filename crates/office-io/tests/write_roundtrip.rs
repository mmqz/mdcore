//! xlsx-r2 Phase 1 写读全环集成测试（create → save → reopen → verify）。
//!
//! 验证链：产品写 API（[`mdc_office_io::write`]）落盘 → **产品读路径**
//! （[`mdc_office_io::io::read_book`]，含容器双门）重开 → 逐面对账。
//! 这是 C2 写面产品化的闭环证据：写面与读面经同一安全门结构互证，
//! 非 spike 旁路。
//!
//! 覆盖面：
//! 1. 五面全环（多表序/值四型/公式+缓存/合并/超链接五形态）；
//! 2. 流式表（constant_memory）全环 + 计数对账；
//! 3. 样式 XML 级验证（style_probe 9/9 断言在产品 API 上的复现）；
//! 4. 控制字符 XML 安全边界（写产物恒为合法 XML 1.0——`_xHHHH_`
//!    转义约定 + 无裸控制字节；读回值往返）；
//! 5. 「读-改-重建」替代路径证据（C3 唯一允许口径的行为锚点：五面
//!    保真、样式/元数据不在面）；
//! 6. save_to_vec 内存产物可落盘重开。
//!
//! 全部断言 = 实测钉定（calamine 0.36.1 / rust_xlsxwriter 0.99.1 /
//! zip 8.6.0 锁定版；升版须重跑重钉——SECURITY_POLICY §5）。

use calamine::Data;
use mdc_office_io::io::read_book;
use mdc_office_io::write::{
    Align, Border, CellStyle, CellValue, FormulaCache, HyperlinkSpec, NumFmt, WorkbookWriter,
};

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

fn tmp(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("mdc_io_write_{name}_{}.xlsx", std::process::id()));
    p.to_string_lossy().into_owned()
}

/// 读 zip 条目为字符串（样式断言用——zip 为本 crate 产品依赖）。
fn zip_entry(path: &str, want: &str) -> String {
    use std::io::Read as _;
    let f = std::fs::File::open(path).unwrap();
    let mut za = zip::ZipArchive::new(f).unwrap();
    for i in 0..za.len() {
        let mut zf = za.by_index(i).unwrap();
        if zf.name() == want {
            let mut s = String::new();
            zf.read_to_string(&mut s).unwrap();
            return s;
        }
    }
    panic!("entry not found: {want}");
}

/// 五面全环样本工作簿（与 io.rs 超链接五形态测试同构——经产品写 API）。
fn sample_workbook() -> WorkbookWriter {
    let mut w = WorkbookWriter::new();
    {
        let mut s = w.add_sheet("数据").unwrap();
        s.write(0, 0, CellValue::Text("标题A".into()), None)
            .unwrap();
        s.write(0, 1, CellValue::Number(123.45), None).unwrap();
        s.write(0, 2, CellValue::Boolean(true), None).unwrap();
        s.write(
            1,
            0,
            CellValue::Formula {
                text: "SUM(B1:B1)*2".into(),
                cached: Some(FormulaCache::Number(246.9)),
            },
            None,
        )
        .unwrap();
        s.write(
            2,
            0,
            CellValue::Formula {
                text: "=AVERAGE(B1:B1)".into(), // 带前缀形态（规范化）
                cached: None,                   // 无缓存 → 读回 Float(0.0)
            },
            None,
        )
        .unwrap();
        s.write(
            3,
            0,
            CellValue::Formula {
                text: r#"=_xlfn.CONCAT("前","后")"#.into(), // _xlfn. 透传
                cached: Some(FormulaCache::Text("前后".into())),
            },
            None,
        )
        .unwrap();
        s.merge(4, 0, 4, 2, "合并表头", None).unwrap();
        // 超链接五形态（v2 勘误后的完整面）。
        s.write_hyperlink(
            5,
            0,
            HyperlinkSpec {
                url: "https://example.com/docs".into(),
                display_text: None,
                tooltip: None,
            },
        )
        .unwrap();
        s.write_hyperlink(
            6,
            0,
            HyperlinkSpec {
                url: "https://example.org/path?q=1&x=2".into(),
                display_text: Some("带文本链接".into()),
                tooltip: None,
            },
        )
        .unwrap();
        s.write_hyperlink(
            7,
            0,
            HyperlinkSpec {
                url: "internal:目标!A1".into(),
                display_text: None,
                tooltip: None,
            },
        )
        .unwrap();
        s.write_hyperlink(
            8,
            0,
            HyperlinkSpec {
                url: "https://example.com/tip".into(),
                display_text: Some("悬停有提示的链接".into()),
                tooltip: Some("悬停提示文本".into()),
            },
        )
        .unwrap();
        s.write_hyperlink(
            9,
            0,
            HyperlinkSpec {
                url: "file:///C:/refs/Book2.xlsx#Sheet1!A1".into(),
                display_text: None,
                tooltip: None,
            },
        )
        .unwrap();
    }
    {
        let mut s = w.add_sheet("目标").unwrap();
        s.write(0, 0, CellValue::Text("目标单元格".into()), None)
            .unwrap();
    }
    w
}

// ---------------------------------------------------------------------------
// 1. 五面全环
// ---------------------------------------------------------------------------

#[test]
fn five_face_full_circle() {
    let p = tmp("five_face");
    sample_workbook().save_to_path(&p).unwrap();

    // 重开走产品读路径（含容器双门——写产物必须无害过门）。
    let book = read_book(&p).unwrap();
    assert_eq!(book.sheet_names(), vec!["数据", "目标"], "表序保持");

    let s = &book.sheets[0];
    // 值面四型 + 公式缓存值。
    match s.values.get((0, 0)) {
        Some(Data::String(v)) => assert_eq!(v, "标题A"),
        other => panic!("A1 应为字符串, got {other:?}"),
    }
    match s.values.get((0, 1)) {
        Some(Data::Float(v)) => assert!((v - 123.45).abs() < 1e-9),
        other => panic!("B1 应为数字, got {other:?}"),
    }
    match s.values.get((0, 2)) {
        Some(Data::Bool(v)) => assert!(v),
        other => panic!("C1 应为布尔, got {other:?}"),
    }
    match s.values.get((1, 0)) {
        Some(Data::Float(v)) => assert!((v - 246.9).abs() < 1e-9, "公式缓存值"),
        other => panic!("A2 应为缓存值, got {other:?}"),
    }
    match s.values.get((2, 0)) {
        Some(Data::Float(v)) => assert!(*v == 0.0, "无缓存公式读回 Float(0.0)（两义性在档）"),
        other => panic!("A3 应为 0.0, got {other:?}"),
    }
    match s.values.get((3, 0)) {
        Some(Data::String(v)) => assert_eq!(v, "前后", "文本缓存值"),
        other => panic!("A4 应为文本缓存, got {other:?}"),
    }
    // 合并表头值 + 范围。
    match s.values.get((4, 0)) {
        Some(Data::String(v)) => assert_eq!(v, "合并表头"),
        other => panic!("A5 应为合并左上值, got {other:?}"),
    }
    assert_eq!(s.merges.len(), 1, "合并范围数");
    let m = &s.merges[0];
    assert_eq!((m.start.0, m.start.1), (4, 0));
    assert_eq!((m.end.0, m.end.1), (4, 2));
    // 公式文本（读侧无 = 前缀——FIDELITY §1 登记行为）。
    // 公式范围锚定在**首个公式格**（实测钉定：值范围锚定首格、公式
    // 范围锚定首个公式格——相对坐标 = 绝对 - start）。
    let fs = s.formulas.start().unwrap_or((0, 0));
    let frel = |abs: (u32, u32)| ((abs.0 - fs.0) as usize, (abs.1 - fs.1) as usize);
    let f1 = s.formulas.get(frel((1, 0))).cloned().unwrap_or_default();
    assert_eq!(f1, "SUM(B1:B1)*2", "公式文本不带 = 前缀（往返一致）");
    let f3 = s.formulas.get(frel((3, 0))).cloned().unwrap_or_default();
    assert!(
        f3.starts_with("_xlfn.CONCAT"),
        "_xlfn. 前缀透传往返一致: {f3}"
    );
    // 超链接五形态（经 IO 边界实体补偿后 = 逻辑 URL）。
    assert_eq!(s.hyperlinks.len(), 5, "五链接形态计数");
    assert_eq!(
        s.hyperlinks[0].target.as_deref(),
        Some("https://example.com/docs")
    );
    assert_eq!(
        s.hyperlinks[1].target.as_deref(),
        Some("https://example.org/path?q=1&x=2"),
        "实体补偿：& 查询 URL 逻辑形态"
    );
    assert_eq!(s.hyperlinks[2].location.as_deref(), Some("目标!A1"));
    assert_eq!(s.hyperlinks[3].tooltip.as_deref(), Some("悬停提示文本"));
    assert_eq!(
        s.hyperlinks[4].target.as_deref(),
        Some("file:///C:/refs/Book2.xlsx")
    );
    assert_eq!(s.hyperlinks[4].location.as_deref(), Some("Sheet1!A1"));

    let _ = std::fs::remove_file(&p);
}

// ---------------------------------------------------------------------------
// 2. 流式表全环
// ---------------------------------------------------------------------------

// t21：wasm32 结构性缺席，native 全跑（add_sheet_streaming = 引擎
// tempfile 背书，wasm32 无文件系统——write.rs 模块文档 t19 注记）。
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn streaming_sheet_full_circle() {
    let p = tmp("stream");
    let mut w = WorkbookWriter::new();
    {
        let rows: u32 = 2_000;
        let mut s = w.add_sheet_streaming("大表").unwrap();
        s.write_row(
            0,
            &[CellValue::Text("行号".into()), CellValue::Text("值".into())],
        )
        .unwrap();
        for r in 1..rows {
            s.write_row(
                r,
                &[
                    CellValue::Number(f64::from(r)),
                    CellValue::Number(f64::from(r) * 10.0),
                ],
            )
            .unwrap();
        }
    }
    w.save_to_path(&p).unwrap();

    let book = read_book(&p).unwrap();
    assert_eq!(book.sheet_names(), vec!["大表"]);
    let s = &book.sheets[0];
    // 计数对账：表头 2 + 数据 2×1999 = 4000 非空格。
    assert_eq!(s.non_empty_cells_count(), 4000);
    // 首尾行抽查（流式表行序完整性的端点验证）。
    match s.values.get((1, 1)) {
        Some(Data::Float(v)) => assert!(*v == 10.0),
        other => panic!("行 1 列 1 应为 10.0, got {other:?}"),
    }
    match s.values.get((1999, 1)) {
        Some(Data::Float(v)) => assert!(*v == 19990.0, "末行完整性"),
        other => panic!("行 1999 列 1 应为 19990.0, got {other:?}"),
    }

    let _ = std::fs::remove_file(&p);
}

/// 非空格计数（SheetData 私有辅助的同构——读面非空格断言用）。
// t21：唯一消费方 = streaming 测试（上），随其 wasm32 门控防 dead_code。
#[cfg(not(target_arch = "wasm32"))]
trait NonEmpty {
    fn non_empty_cells_count(&self) -> usize;
}

#[cfg(not(target_arch = "wasm32"))]
impl NonEmpty for mdc_office_io::io::SheetData {
    fn non_empty_cells_count(&self) -> usize {
        self.values
            .rows()
            .map(|r| r.iter().filter(|c| !matches!(c, Data::Empty)).count())
            .sum()
    }
}

// ---------------------------------------------------------------------------
// 3. 样式 XML 级验证（style_probe 9/9 在产品 API 上的复现）
// ---------------------------------------------------------------------------

#[test]
fn style_xml_nine_assertions_via_product_api() {
    let p = tmp("styles");
    let mut w = WorkbookWriter::new();
    {
        let mut s = w.add_sheet("样式").unwrap();
        let bold = CellStyle {
            bold: true,
            ..Default::default()
        };
        let red = CellStyle {
            font_color: Some("#FF0000".into()),
            ..Default::default()
        };
        let fill = CellStyle {
            fill_color: Some("#FFFF00".into()),
            ..Default::default()
        };
        let boxed = CellStyle {
            border: Border::Thin,
            ..Default::default()
        };
        let currency = CellStyle {
            num_fmt: Some(NumFmt::CurrencyYen),
            ..Default::default()
        };
        let percent = CellStyle {
            num_fmt: Some(NumFmt::PercentOneDecimal),
            ..Default::default()
        };
        let center = CellStyle {
            align: Align::Center,
            ..Default::default()
        };
        s.write(0, 0, CellValue::Text("加粗标题".into()), Some(&bold))
            .unwrap();
        s.write(0, 1, CellValue::Text("红字".into()), Some(&red))
            .unwrap();
        s.write(0, 2, CellValue::Text("黄底".into()), Some(&fill))
            .unwrap();
        s.write(0, 3, CellValue::Text("边框".into()), Some(&boxed))
            .unwrap();
        s.write(1, 0, CellValue::Number(1234567.891), Some(&currency))
            .unwrap();
        s.write(1, 1, CellValue::Number(0.8765), Some(&percent))
            .unwrap();
        s.write(1, 2, CellValue::Text("居中".into()), Some(&center))
            .unwrap();
    }
    w.save_to_path(&p).unwrap();

    let styles = zip_entry(&p, "xl/styles.xml");
    let sheet = zip_entry(&p, "xl/worksheets/sheet1.xml");

    // 9/9 断言（= 分析仓 style_probe 探针同组，产品 API 复现）。
    let checks: Vec<(&str, bool)> = vec![
        (
            "1 字体 bold",
            styles.contains("<b/>") || styles.contains("<b "),
        ),
        ("2 字色 FF0000", styles.contains("rgb=\"FFFF0000\"")),
        ("3 填充 FFFF00", styles.contains("rgb=\"FFFFFF00\"")),
        ("4 边框 thin", styles.contains("style=\"thin\"")),
        (
            "5 自定义 numFmt 区",
            styles.contains("<numFmts") && styles.contains("numFmtId=\"164\""),
        ),
        ("6 ¥ 货币码", styles.contains("¥")),
        ("7 百分比码", styles.contains("0.0%")),
        (
            "8 cellXfs 应用链",
            styles.contains("<cellXfs") && styles.contains("<xf "),
        ),
        ("9 sheet 引用 s=", sheet.contains("s=\"")),
    ];
    for (name, ok) in checks {
        assert!(ok, "样式断言失败: {name}");
    }

    let _ = std::fs::remove_file(&p);
}

// ---------------------------------------------------------------------------
// 4. 控制字符 XML 安全边界
// ---------------------------------------------------------------------------

#[test]
fn control_chars_escaped_and_roundtrip() {
    let p = tmp("ctrl");
    let mut w = WorkbookWriter::new();
    {
        let mut s = w.add_sheet("S").unwrap();
        // NUL + 其他控制字符（\t \n 引擎原样保留——XML 1.0 合法白空格）。
        s.write(0, 0, CellValue::Text("a\0b".into()), None).unwrap();
        s.write(1, 0, CellValue::Text("x\u{1}y".into()), None)
            .unwrap();
        s.write(2, 0, CellValue::Text("tab\tnew\nline".into()), None)
            .unwrap();
        // 字面 _x0000_ 形态（前导转义防歧义——与 mdc-office X-03 同约定）。
        s.write(3, 0, CellValue::Text("_x0000_literal".into()), None)
            .unwrap();
    }
    w.save_to_path(&p).unwrap();

    // XML 级（全部件扫描）：所有 .xml 部件无裸控制字节（\x00-\x08
    // \x0B \x0C \x0E-\x1F；\t\n 为 XML 合法白空格保留）——写产物
    // 恒为合法 XML 1.0。字符串默认落 sharedStrings.xml（非流式表），
    // 故转义形态检查也走全部件聚合。
    let f = std::fs::File::open(&p).unwrap();
    let mut za = zip::ZipArchive::new(f).unwrap();
    let mut all_xml = String::new();
    for i in 0..za.len() {
        use std::io::Read as _;
        let mut zf = za.by_index(i).unwrap();
        if zf.name().ends_with(".xml") {
            let mut s = String::new();
            zf.read_to_string(&mut s).unwrap();
            for (j, b) in s.bytes().enumerate() {
                let is_raw_ctrl = matches!(b, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F);
                assert!(
                    !is_raw_ctrl,
                    "部件 {} 偏移 {j} 含裸控制字节 0x{b:02X}——写产物必须恒为合法 XML 1.0",
                    zf.name()
                );
            }
            all_xml.push_str(&s);
        }
    }
    drop(za);
    assert!(
        all_xml.contains("_x0000_"),
        "NUL 转义形态在位（sharedStrings）"
    );
    assert!(
        all_xml.contains("_x005F_"),
        "字面 _xHHHH_ 形态的前导转义在位"
    );

    // 读回：值面往返（calamine unescape_xml 逆转 _x00HH_ 约定）。
    let book = read_book(&p).unwrap();
    let s = &book.sheets[0];
    match s.values.get((0, 0)) {
        Some(Data::String(v)) => assert_eq!(v, "a\0b", "NUL 值往返零失真"),
        other => panic!("A1 应为字符串, got {other:?}"),
    }
    match s.values.get((1, 0)) {
        Some(Data::String(v)) => assert_eq!(v, "x\u{1}y"),
        other => panic!("A2 应为字符串, got {other:?}"),
    }
    match s.values.get((2, 0)) {
        Some(Data::String(v)) => assert_eq!(v, "tab\tnew\nline"),
        other => panic!("A3 应为字符串, got {other:?}"),
    }

    let _ = std::fs::remove_file(&p);
}

// ---------------------------------------------------------------------------
// 5. 读-改-重建替代路径（C3 唯一允许口径的行为锚点）
// ---------------------------------------------------------------------------

#[test]
fn read_modify_rebuild_path_evidence() {
    // 造源档 → 读 → 修改一格 → 经产品写 API 重建 → 重开对账：
    // 五面（表序/值/公式文本/合并/超链接）保真 = FIDELITY v2 矩阵复现。
    let src = tmp("rebuild_src");
    sample_workbook().save_to_path(&src).unwrap();

    let extracted = read_book(&src).unwrap();
    let modified_cell = (10usize, 0usize, "R-MODIFIED".to_string());

    // 重建（值面驱动超链接形态——fidelity.rs 回放策略同构）。
    // 坐标纪律（probe 实测钉定）：值范围锚定首格、公式范围锚定首个
    // 公式格——**全部配对按绝对坐标**（fidelity.rs abs 键同构；相对键
    // 会产生整体错位的自洽假通过，已在开发中实测捕获并修复）。
    let mut w = WorkbookWriter::new();
    for sheet in &extracted.sheets {
        let mut s = w.add_sheet(&sheet.name).unwrap();
        let vstart = sheet.values.start().unwrap_or((0, 0));
        let fstart = sheet.formulas.start().unwrap_or((0, 0));
        let mut formula_idx = std::collections::BTreeMap::new();
        for r in 0..sheet.formulas.height() {
            for c in 0..sheet.formulas.width() {
                if let Some(f) = sheet.formulas.get((r, c)) {
                    if !f.is_empty() {
                        // 绝对坐标键（fstart + 相对）。
                        formula_idx.insert(
                            ((fstart.0 as usize) + r, (fstart.1 as usize) + c),
                            f.clone(),
                        );
                    }
                }
            }
        }
        let vreloff = |abs: (u32, u32)| {
            (
                (abs.0 as usize).saturating_sub(vstart.0 as usize),
                (abs.1 as usize).saturating_sub(vstart.1 as usize),
            )
        };
        for r in 0..sheet.values.height() {
            for c in 0..sheet.values.width() {
                let cell = match sheet.values.get((r, c)) {
                    Some(v) => v,
                    None => continue,
                };
                // 绝对坐标（vstart + 相对）——配对与落盘同键。
                let abs = ((vstart.0 as usize) + r, (vstart.1 as usize) + c);
                let value: Option<CellValue> = if let Some(f) = formula_idx.get(&abs) {
                    let cached = match cell {
                        Data::Float(v) => Some(FormulaCache::Number(*v)),
                        Data::Int(v) => Some(FormulaCache::Number(*v as f64)),
                        Data::Bool(b) => Some(FormulaCache::Boolean(*b)),
                        Data::String(s2) => Some(FormulaCache::Text(s2.to_string())),
                        Data::Empty => None,
                        _ => None,
                    };
                    Some(CellValue::Formula {
                        text: f.clone(),
                        cached,
                    })
                } else {
                    match cell {
                        Data::String(s2) => Some(CellValue::Text(s2.to_string())),
                        Data::Float(v) => Some(CellValue::Number(*v)),
                        Data::Int(v) => Some(CellValue::Number(*v as f64)),
                        Data::Bool(b) => Some(CellValue::Boolean(*b)),
                        Data::Empty
                        | Data::DateTime(_)
                        | Data::DateTimeIso(_)
                        | Data::DurationIso(_)
                        | Data::Error(_) => None,
                    }
                };
                if (abs.0, abs.1) == (modified_cell.0, modified_cell.1) {
                    continue; // 修改格走统一落位（下方）
                }
                if let Some(v) = value {
                    s.write(abs.0 as u32, abs.1 as u16, v, None).unwrap();
                }
            }
        }
        // 修改标记（数据区外统一位）。
        s.write(
            modified_cell.0 as u32,
            modified_cell.1 as u16,
            CellValue::Text(modified_cell.2.clone()),
            None,
        )
        .unwrap();
        // 合并重放（左上值已在值面；m.start 为绝对坐标）。
        for m in &sheet.merges {
            let text = match sheet.values.get(vreloff((m.start.0, m.start.1))) {
                Some(Data::String(v)) => v.to_string(),
                _ => String::new(),
            };
            s.merge(
                m.start.0,
                m.start.1 as u16,
                m.end.0,
                m.end.1 as u16,
                &text,
                None,
            )
            .unwrap();
        }
        // 超链接重放（值面驱动形态 + tooltip 透传）。
        for h in &sheet.hyperlinks {
            let url = match (&h.target, &h.location) {
                (Some(t), Some(loc)) => format!("{t}#{loc}"),
                (Some(t), None) => t.clone(),
                (None, Some(loc)) => format!("internal:{loc}"),
                (None, None) => continue,
            };
            let anchor_text = match sheet
                .values
                .get(vreloff((h.range.start.0, h.range.start.1)))
            {
                Some(Data::String(v)) => Some(v.to_string()),
                _ => None,
            };
            let display = match anchor_text {
                Some(t) if t != url => Some(t),
                _ => None,
            };
            s.write_hyperlink(
                h.range.start.0,
                h.range.start.1 as u16,
                HyperlinkSpec {
                    url,
                    display_text: display,
                    tooltip: h.tooltip.clone(),
                },
            )
            .unwrap();
        }
    }
    let dst = tmp("rebuild_dst");
    w.save_to_path(&dst).unwrap();

    // 对账：五面保真 + 修改格在位。
    let rebuilt = read_book(&dst).unwrap();
    assert_eq!(rebuilt.sheet_names(), extracted.sheet_names(), "表序保真");
    for (orig, new) in extracted.sheets.iter().zip(rebuilt.sheets.iter()) {
        assert_eq!(orig.name, new.name);
        // 值面：源档全部非空格在重建档同绝对坐标在位且相等（修改格
        // 除外——它新增）；两侧各自按本档 start 换算相对坐标。
        let ovs = orig.values.start().unwrap_or((0, 0));
        let nvs = new.values.start().unwrap_or((0, 0));
        for r in 0..orig.values.height() {
            for c in 0..orig.values.width() {
                let ov = orig.values.get((r, c));
                if matches!(ov, Some(Data::Empty) | None) {
                    continue;
                }
                let abs = ((ovs.0 as usize) + r, (ovs.1 as usize) + c);
                let nv = new
                    .values
                    .get((abs.0 - nvs.0 as usize, abs.1 - nvs.1 as usize));
                assert_eq!(
                    format!("{ov:?}"),
                    format!("{nv:?}"),
                    "值面漂移 {} r{} c{}",
                    orig.name,
                    abs.0,
                    abs.1
                );
            }
        }
        // 公式文本面（同绝对坐标配对）。
        let ofs = orig.formulas.start().unwrap_or((0, 0));
        let nfs = new.formulas.start().unwrap_or((0, 0));
        for r in 0..orig.formulas.height() {
            for c in 0..orig.formulas.width() {
                let of = orig
                    .formulas
                    .get((r, c))
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if of.is_empty() {
                    continue;
                }
                let abs = ((ofs.0 as usize) + r, (ofs.1 as usize) + c);
                let nf = new
                    .formulas
                    .get((abs.0 - nfs.0 as usize, abs.1 - nfs.1 as usize))
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                assert_eq!(of, nf, "公式文本面 {} r{} c{}", orig.name, abs.0, abs.1);
            }
        }
        // 合并范围面。
        let om: Vec<_> = orig.merges.iter().map(|d| (d.start, d.end)).collect();
        let nm: Vec<_> = new.merges.iter().map(|d| (d.start, d.end)).collect();
        assert_eq!(om, nm, "合并范围保真 {}", orig.name);
        // 超链接面（target/location/tooltip 逐字段）。
        assert_eq!(
            orig.hyperlinks.len(),
            new.hyperlinks.len(),
            "超链接计数 {}",
            orig.name
        );
        for (oh, nh) in orig.hyperlinks.iter().zip(new.hyperlinks.iter()) {
            assert_eq!(oh.target, nh.target, "target 保真");
            assert_eq!(oh.location, nh.location, "location 保真");
            assert_eq!(oh.tooltip, nh.tooltip, "tooltip 保真");
        }
    }
    // 修改格在位。
    match rebuilt.sheets[0]
        .values
        .get((modified_cell.0, modified_cell.1))
    {
        Some(Data::String(v)) => assert_eq!(v, "R-MODIFIED"),
        other => panic!("修改格应在位, got {other:?}"),
    }

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
}

// ---------------------------------------------------------------------------
// 6. save_to_vec 内存产物
// ---------------------------------------------------------------------------

#[test]
fn save_to_vec_bytes_are_reopenable() {
    let mut w = sample_workbook();
    let bytes = w.save_to_vec().unwrap();
    assert_eq!(&bytes[..2], b"PK");

    let p = tmp("from_vec");
    std::fs::write(&p, &bytes).unwrap();
    let book = read_book(&p).unwrap();
    assert_eq!(book.sheet_names(), vec!["数据", "目标"]);
    assert_eq!(book.sheets[0].hyperlinks.len(), 5);

    let _ = std::fs::remove_file(&p);
}
