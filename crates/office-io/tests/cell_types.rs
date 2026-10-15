//! r4-t1：XLSX 单元格类型语义钉定（读面 Data 面 + Display 面）。
//!
//! **探针来源（T1.1，2026-10-17）**：raw XML zip 自构造七类格 + 边界
//! 格，经产品读路径（`read_book_from_bytes` 容器门 + calamine 提取）
//! 钉定当前真实行为。断言依据 = calamine 0.36.1 源码一手核验
//! （cells_reader.rs `read_v` / formats.rs numFmt 判定 / datatype.rs
//! Display）+ 本测试实测。**升版 calamine 须重跑本钉定**（与
//! range_anchor_pinned.rs 同一复核纪律）。
//!
//! 构造面（styles.xml cellXfs）：
//! - xf 1 = numFmtId 14（builtin 日期 m/d/yy 家族）；
//! - xf 2 = numFmtId 164 自定义 `yyyy-mm-dd`（日期 token 检测面）；
//! - xf 3 = numFmtId 165 自定义 `[h]:mm:ss`（TimeDelta 面）。
//!
//! 七类格 + 边界（行 1，A-N 列）：
//! | 格 | 存储 | 期望 |
//! |----|------|------|
//! | A1 | `<v>123</v>`（无 t 无 s） | Float(123)——数值 |
//! | B1 | inlineStr `00123` | String——前导零文本原样保留 |
//! | C1 | t="b" `1` | Bool(true) |
//! | D1 | s=1（builtin 日期 numFmt）`44927` | DateTime——**numFmt 证据** |
//! | E1 | 无 s `44927` | Float——纯序列数**不**认定日期 |
//! | F1 | t="d" `2023-01-01T00:00:00` | DateTimeIso——显式 ISO 存储 |
//! | G1 | `<f>1+1</f><v>2</v>` | 值面 Float(2)（缓存）+ 公式文本 `1+1` 两通道 |
//! | H1 | 缺格 | Empty（in-range 缺格 = Empty） |
//! | I1 | t="e" `#DIV/0!` | Error(Div0)——错误码保留 |
//! | J1 | `9007199254740993` | Float(9007199254740992)——2^53+1 精度边界 |
//! | K1 | `0.30000000000000004` | Float——Rust Display 最短往返形态 |
//! | L1 | s=2（自定义日期 numFmt）`44927` | DateTime——自定义格式码 token 面 |
//! | M1 | s=3（[h]:mm:ss）`1.5` | DateTime(TimeDelta)——时长 |
//! | N1 | t="str" `text result` | String（公式文本结果类型存储） |

use calamine::{Data, DataType as _};
use mdc_office_io::io::read_book_from_bytes;

// ---------------------------------------------------------------------------
// 构造 helper（io.rs 内联测试同款 raw XML zip——集成面走字节入口）
// ---------------------------------------------------------------------------

/// zip 字节构造（STORED 等价——本测试用 deflate 默认项）。
fn zip_bytes(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    use std::io::Write as _;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts: zip::write::SimpleFileOptions = Default::default();
    for (name, data) in entries {
        w.start_file(name.as_str(), opts).expect("start_file");
        w.write_all(data).expect("write");
    }
    w.finish().expect("finish").into_inner()
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="2"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/><numFmt numFmtId="165" formatCode="[h]:mm:ss"/></numFmts><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="165" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs></styleSheet>"#;

/// workbook.xml（date1904 开关——1904 纪元探针）。
fn workbook_xml(date_1904: bool) -> String {
    let pr = if date_1904 {
        r#"<workbookPr date1904="1"/>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{pr}<sheets><sheet name="S1" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    )
}

/// 行 1 全类格 sheet。
fn sheet_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:N1"/><sheetData><row r="1"><c r="A1"><v>123</v></c><c r="B1" t="inlineStr"><is><t>00123</t></is></c><c r="C1" t="b"><v>1</v></c><c r="D1" s="1"><v>44927</v></c><c r="E1"><v>44927</v></c><c r="F1" t="d"><v>2023-01-01T00:00:00</v></c><c r="G1"><f>1+1</f><v>2</v></c><c r="I1" t="e"><v>#DIV/0!</v></c><c r="J1"><v>9007199254740993</v></c><c r="K1"><v>0.30000000000000004</v></c><c r="L1" s="2"><v>44927</v></c><c r="M1" s="3"><v>1.5</v></c><c r="N1" t="str"><v>text result</v></c></row></sheetData></worksheet>"#
}

/// 七类 + 边界格探针工作簿。
fn probe_xlsx(date_1904: bool) -> Vec<u8> {
    zip_bytes(&[
        (
            "[Content_Types].xml".into(),
            CONTENT_TYPES.as_bytes().to_vec(),
        ),
        ("_rels/.rels".into(), ROOT_RELS.as_bytes().to_vec()),
        (
            "xl/workbook.xml".into(),
            workbook_xml(date_1904).into_bytes(),
        ),
        (
            "xl/_rels/workbook.xml.rels".into(),
            WORKBOOK_RELS.as_bytes().to_vec(),
        ),
        ("xl/styles.xml".into(), STYLES.as_bytes().to_vec()),
        (
            "xl/worksheets/sheet1.xml".into(),
            sheet_xml().as_bytes().to_vec(),
        ),
    ])
}

// ---------------------------------------------------------------------------
// Data 面（探针钉定——calamine 0.36.1 经产品读路径的真实行为）
// ---------------------------------------------------------------------------

#[test]
fn data_face_seven_kinds_pinned() {
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    assert_eq!(book.sheet_names(), vec!["S1"]);
    let s = &book.sheets[0];
    assert_eq!(
        (s.values.height(), s.values.width()),
        (1, 14),
        "使用区包围盒"
    );

    let d = |col: usize| s.values.get((0, col)).expect("in-range cell");

    // A1 数值：无 t 无 s → Float（xlsx 数值恒 Float，Int 不可达）。
    assert!(
        matches!(d(0), Data::Float(v) if *v == 123.0),
        "A1: {:?}",
        d(0)
    );
    // B1 前导零文本：原样保留（绝不转数字）。
    assert!(
        matches!(d(1), Data::String(t) if t == "00123"),
        "B1: {:?}",
        d(1)
    );
    // C1 布尔。
    assert!(matches!(d(2), Data::Bool(true)), "C1: {:?}", d(2));
    // D1 序列数 + builtin 日期 numFmt → DateTime（numFmt 证据）。
    assert!(matches!(d(3), Data::DateTime(_)), "D1: {:?}", d(3));
    // E1 同序列数无 numFmt → Float（纯数值不认定日期）。
    assert!(
        matches!(d(4), Data::Float(v) if *v == 44927.0),
        "E1: {:?}",
        d(4)
    );
    // F1 t="d" ISO 存储 → DateTimeIso（原串）。
    assert!(
        matches!(d(5), Data::DateTimeIso(s) if s == "2023-01-01T00:00:00"),
        "F1: {:?}",
        d(5)
    );
    // G1 公式：值面 = 缓存值 Float(2)。
    assert!(
        matches!(d(6), Data::Float(v) if *v == 2.0),
        "G1: {:?}",
        d(6)
    );
    // H1 缺格 → Empty（in-range 缺格 = Empty，非 None）。
    assert!(matches!(d(7), Data::Empty), "H1: {:?}", d(7));
    // I1 错误格 → Error（错误码可判别）。
    assert!(
        matches!(d(8), Data::Error(ref e) if e.to_string() == "#DIV/0!"),
        "I1: {:?}",
        d(8)
    );
    // J1 2^53+1 长整数 → f64 精度边界（Excel 数值面 = IEEE 754 double）。
    assert!(
        matches!(d(9), Data::Float(v) if *v == 9007199254740992.0),
        "J1: {:?}",
        d(9)
    );
    // K1 浮点形态 → Rust Display 最短往返。
    assert!(
        matches!(d(10), Data::Float(v) if *v == 0.1 + 0.2),
        "K1: {:?}",
        d(10)
    );
    // L1 自定义日期格式码 → DateTime（token 检测面）。
    assert!(matches!(d(11), Data::DateTime(_)), "L1: {:?}", d(11));
    // M1 [h]:mm:ss → DateTime(TimeDelta)。
    assert!(matches!(d(12), Data::DateTime(_)), "M1: {:?}", d(12));
    // N1 t="str" → String（公式文本结果）。
    assert!(
        matches!(d(13), Data::String(t) if t == "text result"),
        "N1: {:?}",
        d(13)
    );
}

#[test]
fn display_face_data_to_string_pinned() {
    // Display 值面（t17 消费语义）：逐字钉定。
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    let s = &book.sheets[0];
    let disp = |col: usize| {
        s.values
            .get((0, col))
            .map(Data::to_string)
            .unwrap_or_default()
    };
    assert_eq!(disp(0), "123", "数值 Rust 格式化");
    assert_eq!(disp(1), "00123", "前导零文本逐字");
    assert_eq!(disp(2), "true", "布尔");
    assert_eq!(
        disp(3),
        "44927",
        "日期格 Display = 裸序列数（ExcelDateTime Display = value）"
    );
    assert_eq!(
        disp(4),
        "44927",
        "纯数值同串（与日期格 Display 不可分——kind 需类型面）"
    );
    assert_eq!(disp(5), "2023-01-01T00:00:00", "ISO 原串");
    assert_eq!(disp(6), "2", "公式格 Display = 缓存值");
    assert_eq!(disp(7), "", "空格");
    assert_eq!(disp(8), "#DIV/0!", "错误码文本");
    assert_eq!(
        disp(9),
        "9007199254740992",
        "2^53+1 精度边界 Display（f64 舍入）"
    );
    assert_eq!(disp(10), "0.30000000000000004", "浮点最短往返形态");
    assert_eq!(disp(11), "44927", "自定义日期 numFmt 格 Display 同序列数");
    assert_eq!(disp(12), "1.5", "时长格 Display = 序列数");
    assert_eq!(disp(13), "text result", "t=str 文本");
}

#[test]
fn formula_text_channel_distinct_from_cached_value() {
    // 两通道分立：值面 = 缓存值；formulas 面 = 公式文本（不带 = 前缀——
    // FIDELITY §1 在档）。公式范围锚定首个公式格（range_anchor_pinned）。
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    let s = &book.sheets[0];
    assert_eq!(s.formulas.start(), Some((0, 6)), "公式范围锚定 G1");
    assert_eq!(
        s.formulas.get((0, 0)).map(String::as_str),
        Some("1+1"),
        "公式文本通道"
    );
    assert_eq!(
        s.values.get((0, 6)).map(Data::to_string).as_deref(),
        Some("2"),
        "值面 = 缓存值（≠公式文本——两通道明确区分）"
    );
}

#[test]
fn date_serial_components_and_1904_epoch() {
    // 1900 纪元：44927 = 2023-01-01（calamine to_ymd_hms_milli——本测试
    // 钉定经产品读路径的实际组件，供显式转换 API 对照）。
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    let s = &book.sheets[0];
    let dt = s
        .values
        .get((0, 3))
        .and_then(|d| d.get_datetime())
        .expect("D1 DateTime");
    assert_eq!(
        dt.to_ymd_hms_milli(),
        (2023, 1, 1, 0, 0, 0, 0),
        "44927 = 2023-01-01"
    );
    assert!(!dt.is_duration(), "D1 非时长");
    // M1 时长面。
    let dur = s
        .values
        .get((0, 12))
        .and_then(|d| d.get_datetime())
        .expect("M1 DateTime");
    assert!(dur.is_duration(), "M1 = TimeDelta");
    assert_eq!(dur.as_f64(), 1.5, "时长序列 = 天分数");

    // 1904 纪元：同序列数不同历法（+1462 天偏移）。
    let book04 = read_book_from_bytes(&probe_xlsx(true)).expect("1904 book");
    let s04 = &book04.sheets[0];
    let dt04 = s04
        .values
        .get((0, 3))
        .and_then(|d| d.get_datetime())
        .expect("D1 DateTime (1904)");
    assert_eq!(
        dt04.to_ymd_hms_milli(),
        (2027, 1, 2, 0, 0, 0, 0),
        "1904 纪元 44927 = 2027-01-02（44927+1462 天自 1899-12-30：2023-01-01 + 1462 天）"
    );
}

// ---------------------------------------------------------------------------
// 类型化模型面（r4-t1：CellData / CellGrid / formula_at / replay_cell）
// ---------------------------------------------------------------------------

#[test]
fn typed_face_seven_kinds_pinned() {
    use mdc_office_io::io::CellData;
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    let s = &book.sheets[0];
    assert_eq!(
        (s.typed.height(), s.typed.width()),
        (1, 14),
        "typed 与 values 同一包围盒"
    );
    assert_eq!(s.typed.start(), s.values.start(), "锚点一致");
    let t = |col: usize| s.typed.get((0, col)).expect("in-range typed cell");
    assert!(
        matches!(t(0), CellData::Number(v) if *v == 123.0),
        "A1: {:?}",
        t(0)
    );
    assert!(
        matches!(t(1), CellData::Text(s) if s == "00123"),
        "B1: {:?}",
        t(1)
    );
    assert!(matches!(t(2), CellData::Boolean(true)), "C1: {:?}", t(2));
    assert!(
        matches!(t(3), CellData::DateTime { serial, is_1904: false } if *serial == 44927.0),
        "D1: {:?}",
        t(3)
    );
    assert!(
        matches!(t(4), CellData::Number(v) if *v == 44927.0),
        "E1: {:?}",
        t(4)
    );
    assert!(
        matches!(t(5), CellData::DateTimeIso(s) if s == "2023-01-01T00:00:00"),
        "F1: {:?}",
        t(5)
    );
    // G1 公式格：值面 = 缓存值（Number）——公式文本在独立通道（见
    // formula_at 测试）。
    assert!(
        matches!(t(6), CellData::Number(v) if *v == 2.0),
        "G1: {:?}",
        t(6)
    );
    assert!(matches!(t(7), CellData::Empty), "H1: {:?}", t(7));
    assert!(
        matches!(t(8), CellData::Error(c) if c == "#DIV/0!"),
        "I1: {:?}",
        t(8)
    );
    assert!(
        matches!(t(9), CellData::Number(v) if *v == 9007199254740992.0),
        "J1: {:?}",
        t(9)
    );
    assert!(
        matches!(t(10), CellData::Number(v) if *v == 0.1 + 0.2),
        "K1: {:?}",
        t(10)
    );
    assert!(
        matches!(t(11), CellData::DateTime { serial, is_1904: false } if *serial == 44927.0),
        "L1: {:?}",
        t(11)
    );
    assert!(
        matches!(t(12), CellData::Duration { serial, is_1904: false } if *serial == 1.5),
        "M1: {:?}",
        t(12)
    );
    assert!(
        matches!(t(13), CellData::Text(s) if s == "text result"),
        "N1: {:?}",
        t(13)
    );

    // 越界面：get/get_abs 双坐标系。
    assert_eq!(s.typed.get((0, 14)), None, "相对越界 → None");
    assert_eq!(s.typed.get_abs(0, 14), None, "绝对越界 → None");
    assert_eq!(s.typed.get_abs(5, 5), None, "包围盒外 → None");
    assert_eq!(
        s.typed.get_abs(0, 3),
        s.typed.get((0, 3)),
        "绝对/相对双面一致"
    );
}

#[test]
fn typed_display_equivalence_with_data_display() {
    // r4-t1 显示面兼容锚：typed.display() 与 Data::to_string 全格逐字
    // 等值（t17 消费切换的安全依据——xlsx_import.rs 渲染经 typed）。
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    let s = &book.sheets[0];
    for r in 0..s.values.height() {
        for c in 0..s.values.width() {
            let via_data = s.values.get((r, c)).map(Data::to_string);
            let via_typed = s.typed.get((r, c)).map(|d| d.display());
            assert_eq!(
                via_data, via_typed,
                "display 等值漂移 r{r} c{c}: {via_data:?} vs {via_typed:?}"
            );
        }
    }
}

#[test]
fn typed_explicit_conversions_ruled_by_evidence() {
    use mdc_office_io::io::CellData;
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    let s = &book.sheets[0];
    let t = |col: usize| s.typed.get((0, col)).expect("typed cell");

    // 有据转换：DateTime 44927 → 2023-01-01T00:00:00。
    assert_eq!(
        t(3).as_iso_datetime().as_deref(),
        Some("2023-01-01T00:00:00")
    );
    // DateTimeIso 原串直返。
    assert_eq!(
        t(5).as_iso_datetime().as_deref(),
        Some("2023-01-01T00:00:00")
    );
    // **无据不转换**：纯 Number 序列数（E1）不具备日期依据 → None。
    assert_eq!(t(4).as_iso_datetime(), None, "纯序列数不得转换为日期");
    assert_eq!(t(0).as_iso_datetime(), None, "数值非日期");
    assert_eq!(t(1).as_iso_datetime(), None, "文本非日期");
    // 时长：Duration 1.5 天 = 36h；as_iso_datetime 对 Duration → None
    //（非日期证据——时长证据只开 as_iso_duration）。
    assert_eq!(t(12).as_iso_datetime(), None, "Duration 不开 datetime 通道");
    assert_eq!(t(12).as_iso_duration().as_deref(), Some("PT36H0M0S"));
    assert_eq!(t(3).as_iso_duration(), None, "DateTime 不开 duration 通道");

    // 1904 纪元：同序列数 = 不同历法日期（+1462 天）。
    let book04 = read_book_from_bytes(&probe_xlsx(true)).expect("1904 book");
    let s04 = &book04.sheets[0];
    let d04 = s04.typed.get((0, 3)).expect("D1 (1904)");
    assert!(
        matches!(d04, CellData::DateTime { serial, is_1904: true } if *serial == 44927.0),
        "1904 纪元 kind: {d04:?}"
    );
    assert_eq!(
        d04.as_iso_datetime().as_deref(),
        Some("2027-01-02T00:00:00"),
        "1904 纪元 44927 = 2027-01-02"
    );
}

#[test]
fn formula_at_safe_join_and_replay_junction() {
    use mdc_office_io::io::CellData;
    use mdc_office_io::write::CellValue as WriteValue;
    let book = read_book_from_bytes(&probe_xlsx(false)).expect("probe book");
    let s = &book.sheets[0];

    // 公式文本安全联接：绝对坐标内化锚定换算。
    assert_eq!(s.formula_at(0, 6), Some("1+1"), "G1 公式文本（无 = 前缀）");
    assert_eq!(s.formula_at(0, 0), None, "A1 非公式格");
    assert_eq!(s.formula_at(99, 99), None, "越界");
    // 缓存值 ≠ 公式文本（两通道区分的判别断言）。
    assert_eq!(s.typed.get((0, 6)), Some(&CellData::Number(2.0)));

    // replay_cell：公式格 → Formula { text, cached }（缓存通道独立）。
    let (v, _style) = s.replay_cell(0, 6).expect("G1 replay");
    assert!(
        matches!(&v, WriteValue::Formula { text, cached: Some(_) } if text == "1+1"),
        "G1 replay: {v:?}"
    );
    // replay_cell：前导零文本 → Text 原样（kind + 值零损）。
    let (v, style) = s.replay_cell(0, 1).expect("B1 replay");
    assert!(
        matches!(&v, WriteValue::Text(t) if t == "00123"),
        "B1 replay: {v:?}"
    );
    assert!(style.is_none(), "文本格无样式");
    // replay_cell：日期格 → Number(serial) + 日期 numFmt 样式（kind 保持）。
    let (v, style) = s.replay_cell(0, 3).expect("D1 replay");
    assert!(
        matches!(&v, WriteValue::Number(n) if *n == 44927.0),
        "D1 replay: {v:?}"
    );
    let style = style.expect("D1 需日期 numFmt 样式");
    assert!(
        matches!(&style.num_fmt, Some(nf) if matches!(nf, mdc_office_io::write::NumFmt::Custom(c) if c.contains("yyyy"))),
        "D1 replay 样式: {style:?}"
    );
    // replay_cell：空格 → 不落格。
    assert_eq!(s.replay_cell(0, 7), None, "H1 空格不落格");
    assert_eq!(s.replay_cell(99, 99), None, "越界不落格");
    // replay_cell：错误格 → 码文本（可诊断保留）。
    let (v, _) = s.replay_cell(0, 8).expect("I1 replay");
    assert!(
        matches!(&v, WriteValue::Text(t) if t == "#DIV/0!"),
        "I1 replay: {v:?}"
    );
}

#[test]
fn kind_preserving_replay_roundtrip() {
    // 读 → replay_cell（kind 分派）→ 写 → 重读：值与 kind 双面断言。
    // 全链经产品读写面（容器门 + 写引擎 + 提取）。
    use mdc_office_io::io::CellData;
    use mdc_office_io::write::WorkbookWriter;

    let replay_all = |bytes: &[u8]| -> Vec<u8> {
        let book = read_book_from_bytes(bytes).expect("read");
        let mut w = WorkbookWriter::new();
        for sheet in &book.sheets {
            let mut s = w.add_sheet(&sheet.name).expect("sheet");
            let (start, end) = (
                sheet.typed.start().unwrap_or((0, 0)),
                sheet.typed.end().unwrap_or((0, 0)),
            );
            for row in start.0..=end.0 {
                for col in start.1..=end.1 {
                    if let Some((v, style)) = sheet.replay_cell(row, col) {
                        let col16 = u16::try_from(col).expect("col within u16");
                        s.write(row, col16, v, style.as_ref())
                            .expect("replay write");
                    }
                }
            }
        }
        w.save_to_vec().expect("save")
    };

    let bytes = replay_all(&probe_xlsx(false));
    let back = read_book_from_bytes(&bytes).expect("re-read");
    let s = &back.sheets[0];
    let t = |col: usize| s.typed.get((0, col)).expect("re-read typed cell");

    // kind 双面：Text/Number/Boolean 往返保持；前导零零损。
    assert!(
        matches!(t(0), CellData::Number(v) if *v == 123.0),
        "A1: {:?}",
        t(0)
    );
    assert!(
        matches!(t(1), CellData::Text(x) if x == "00123"),
        "B1 前导零往返: {:?}",
        t(1)
    );
    assert!(matches!(t(2), CellData::Boolean(true)), "C1: {:?}", t(2));
    // 日期 kind 往返：numFmt 证据再判 DateTime（serial 同值）。
    assert!(
        matches!(t(3), CellData::DateTime { serial, is_1904: false } if *serial == 44927.0),
        "D1 日期 kind 往返: {:?}",
        t(3)
    );
    // 纯数值无 numFmt → 仍 Number（kind 不漂移）。
    assert!(
        matches!(t(4), CellData::Number(v) if *v == 44927.0),
        "E1: {:?}",
        t(4)
    );
    // ISO 存储 → 写面无 t="d" 面：值保持，kind 降级为 Text（登记）。
    assert!(
        matches!(t(5), CellData::Text(x) if x == "2023-01-01T00:00:00"),
        "F1 ISO 降级: {:?}",
        t(5)
    );
    // 公式格两通道往返：值面 = 缓存值；公式文本通道独立。
    assert!(
        matches!(t(6), CellData::Number(v) if *v == 2.0),
        "G1 缓存值: {:?}",
        t(6)
    );
    assert_eq!(s.formula_at(0, 6), Some("1+1"), "G1 公式文本往返");
    // 空格 → 未落格 → in-range Empty。
    assert!(matches!(t(7), CellData::Empty), "H1: {:?}", t(7));
    // 错误格 → 码文本写回 → Text（kind 降级登记，值=码文本保持）。
    assert!(
        matches!(t(8), CellData::Text(x) if x == "#DIV/0!"),
        "I1: {:?}",
        t(8)
    );
    // 2^53+1 精度边界随存储面恒在（往返不放大）。
    assert!(
        matches!(t(9), CellData::Number(v) if *v == 9007199254740992.0),
        "J1: {:?}",
        t(9)
    );
    assert!(
        matches!(t(10), CellData::Number(v) if *v == 0.1 + 0.2),
        "K1: {:?}",
        t(10)
    );
    // 自定义日期 numFmt → DateTime kind 往返。
    assert!(
        matches!(t(11), CellData::DateTime { serial, is_1904: false } if *serial == 44927.0),
        "L1: {:?}",
        t(11)
    );
    // 时长 kind 往返（TimeDelta numFmt 证据）。
    assert!(
        matches!(t(12), CellData::Duration { serial, is_1904: false } if *serial == 1.5),
        "M1 时长 kind 往返: {:?}",
        t(12)
    );
    assert!(
        matches!(t(13), CellData::Text(x) if x == "text result"),
        "N1: {:?}",
        t(13)
    );

    // 1904 纪元链：历法日期保持（serial 换算 +1462，读回 1900 纪元）。
    let bytes04 = replay_all(&probe_xlsx(true));
    let back04 = read_book_from_bytes(&bytes04).expect("re-read 1904");
    let s04 = &back04.sheets[0];
    let d04 = s04.typed.get((0, 3)).expect("D1 (1904 replay)");
    assert!(
        matches!(d04, CellData::DateTime { serial, is_1904: false } if *serial == 46389.0),
        "1904→1900 换算 44927+1462=46389: {d04:?}"
    );
    assert_eq!(
        d04.as_iso_datetime().as_deref(),
        Some("2027-01-02T00:00:00"),
        "历法日期保持（2027-01-02 不变）"
    );
}
