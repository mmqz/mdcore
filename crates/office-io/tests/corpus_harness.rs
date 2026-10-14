//! xlsx-r1 全语料集成 harness（阶段 B 落地，2026-10-13）。
//!
//! 分析仓 spike `read_proto.rs`（17 件三分断言 harness）+ `corpus.rs`
//! （7 维语料构造）的内联合并直移：语料在测试内构造（临时目录，
//! 不依赖 fixtures），逐件三分归类（OK / REJECT[门拒绝] / ERR[解析
//! 或容器错误]）+ 正向件锚点断言，**零 panic**（panic = 测试失败）。
//!
//! 敌意件归类为**实证钉定**（版本锁定 calamine 0.36.1 / zip 8.6.0；
//! 升版须重跑重钉——分析仓 SECURITY_POLICY §5 复核纪律）。容忍度
//! 发现（mal7c NUL 入值 / mal7e 垃圾静默降级空表 / mal7a 标签内
//! 截断分界）随断言在档。
//!
//! 语料清单与逐件构造意图 = 分析仓 t10-b manifest（xlsx-r1 七件套）。

use std::io::Write as _;

use calamine::Data;
use mdc_office_io::io::{
    read_book, read_book_with_options, Book, ReadError, ReadOptions, WarningKind,
};
use rust_xlsxwriter::{Format, Workbook};

// ---------------------------------------------------------------------------
// 三分归类（与 spike read_proto 同构）
// ---------------------------------------------------------------------------

#[derive(PartialEq, Debug, Clone, Copy)]
enum Class {
    Ok,
    Reject,
    Err,
}

impl Class {
    fn tag(&self) -> &'static str {
        match self {
            Class::Ok => "OK",
            Class::Reject => "REJECT",
            Class::Err => "ERR",
        }
    }
}

fn classify(r: &Result<Book, ReadError>) -> Class {
    match r {
        Ok(_) => Class::Ok,
        // 门拒绝族（Phase 2 起含门 3 部件良构性）。
        Err(ReadError::PrescanRejected { .. }) | Err(ReadError::InvalidPart { .. }) => {
            Class::Reject
        }
        Err(_) => Class::Err,
    }
}

// ---------------------------------------------------------------------------
// 语料构造（corpus.rs 直移——输出改临时目录）
// ---------------------------------------------------------------------------

fn out_dir(tag: &str) -> String {
    let d = std::env::temp_dir().join(format!("mdc_office_io_corpus_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const WB: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const WB_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

/// 手构 zip（STORED 不压缩——敌意样本构造确定性优先）。
fn build_zip(path: &str, entries: &[(&str, Vec<u8>)]) {
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut zw = zip::ZipWriter::new(f);
    let mut opts: zip::write::SimpleFileOptions = Default::default();
    opts = opts.compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(data).unwrap();
    }
    zw.finish().unwrap();
}

fn build_zip_bytes(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut cw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut opts: zip::write::SimpleFileOptions = Default::default();
    opts = opts.compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        cw.start_file(*name, opts).unwrap();
        cw.write_all(data).unwrap();
    }
    cw.finish().unwrap().into_inner()
}

/// 手构最小 xlsx：四部件 zip + 调用方给定的 sheet1.xml 字节。
fn build_min_xlsx(path: &str, sheet: &[u8]) {
    build_zip(
        path,
        &[
            ("[Content_Types].xml", CT.to_vec()),
            ("_rels/.rels", RELS.to_vec()),
            ("xl/workbook.xml", WB.to_vec()),
            ("xl/_rels/workbook.xml.rels", WB_RELS.to_vec()),
            ("xl/worksheets/sheet1.xml", sheet.to_vec()),
        ],
    );
}

/// dim1 公式（SUM/AVG/嵌套 IF/CONCAT；2 带缓存值 2 不带）。
fn dim1_formulas(dir: &str) -> String {
    let p = format!("{dir}/dim1_formulas.xlsx");
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Formulas").unwrap();
    for i in 0..5u32 {
        ws.write_number(i, 0, f64::from(i) * 10.0).unwrap();
    }
    ws.write_formula(0, 1, "=SUM(A1:A5)").unwrap();
    ws.set_formula_result(0, 1, "100");
    ws.write_formula(1, 1, "=AVERAGE(A1:A5)").unwrap();
    ws.write_formula(2, 1, "=IF(SUM(A1:A5)>50,MAX(A1:A5),MIN(A1:A5))")
        .unwrap();
    ws.set_formula_result(2, 1, "40");
    ws.write_formula(3, 1, r#"=CONCAT("总","计")"#).unwrap();
    ws.set_formula_result(3, 1, "总计");
    wb.save(&p).unwrap();
    p
}

/// dim2 样式（bold/红字/黄底/细边框/¥货币/百分比——值面可观测）。
fn dim2_styles(dir: &str) -> String {
    let p = format!("{dir}/dim2_styles.xlsx");
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Styles").unwrap();
    let bold = Format::new().set_bold();
    let red_text = Format::new().set_font_color("#FF0000");
    let fill = Format::new().set_background_color("#FFFF00");
    let boxed = Format::new().set_border(rust_xlsxwriter::FormatBorder::Thin);
    let currency = Format::new().set_num_format("¥#,##0.00");
    let percent = Format::new().set_num_format("0.0%");
    ws.write_string_with_format(0, 0, "加粗标题", &bold)
        .unwrap();
    ws.write_string_with_format(0, 1, "红字", &red_text)
        .unwrap();
    ws.write_string_with_format(0, 2, "黄底", &fill).unwrap();
    ws.write_string_with_format(0, 3, "边框", &boxed).unwrap();
    ws.write_number_with_format(1, 0, 1234567.891, &currency)
        .unwrap();
    ws.write_number_with_format(1, 1, 0.8765, &percent).unwrap();
    wb.save(&p).unwrap();
    p
}

/// dim3 合并（2x2 + 1x3 + 4x1 三范围 + 散点值）。
fn dim3_merged(dir: &str) -> String {
    let p = format!("{dir}/dim3_merged.xlsx");
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Merged").unwrap();
    let center = Format::new().set_align(rust_xlsxwriter::FormatAlign::Center);
    ws.merge_range(0, 0, 1, 1, "2x2 合并", &center).unwrap();
    ws.merge_range(0, 3, 0, 5, "1x3 横向", &center).unwrap();
    ws.merge_range(3, 0, 6, 0, "4x1 纵向", &center).unwrap();
    ws.write_number(3, 3, 42.0).unwrap();
    wb.save(&p).unwrap();
    p
}

/// dim4 多表（Data/RefSheet/Calc/TailEmpty 四表 + 跨表公式 2 条）。
fn dim4_multisheet(dir: &str) -> String {
    let p = format!("{dir}/dim4_multisheet.xlsx");
    let mut wb = Workbook::new();
    let data = wb.add_worksheet();
    data.set_name("Data").unwrap();
    for i in 0..10u32 {
        data.write_number(i, 0, f64::from(i) * 7.0).unwrap();
    }
    let ref_sheet = wb.add_worksheet();
    ref_sheet.set_name("RefSheet").unwrap();
    ref_sheet.write_number(0, 0, 999.0).unwrap();
    let calc = wb.add_worksheet();
    calc.set_name("Calc").unwrap();
    calc.write_formula(0, 0, "=SUM(Data!A1:A10)").unwrap();
    calc.set_formula_result(0, 0, "315");
    calc.write_formula(1, 0, "=RefSheet!A1*2").unwrap();
    calc.set_formula_result(1, 0, "1998");
    let empty_last = wb.add_worksheet();
    empty_last.set_name("TailEmpty").unwrap();
    wb.save(&p).unwrap();
    p
}

/// dim5 关系引用（外链 URL×2 + internal 内部链接 + 纯文本 URL 值）。
fn dim5_relationships(dir: &str) -> String {
    // v2：五链接形态（= 分析仓语料 dim5 v2 同构）——外链纯 /
    // 外链含 & 查询参数（实体补偿证据链）/ 内部 / tooltip /
    // file 子锚（外部工作簿引用形态，PLAN 原规格补齐）。
    let p = format!("{dir}/dim5_relationships.xlsx");
    let mut wb = Workbook::new();
    {
        let ws = wb.add_worksheet();
        ws.set_name("Links").unwrap();
        let link_fmt = Format::new()
            .set_font_color("#0563C1")
            .set_underline(rust_xlsxwriter::FormatUnderline::Single);
        ws.write_url_with_format(0, 0, "https://example.com/docs", &link_fmt)
            .unwrap();
        ws.write_url_with_text(1, 0, "https://example.org/path?q=1&x=2", "带文本链接")
            .unwrap();
        ws.write_url(2, 0, "internal:Target!A1").unwrap();
        // URL 样式字符串（非超链接单元格，纯值面）
        ws.write_string(3, 0, "https://example.net/plain").unwrap();
        ws.write_url_with_options(
            4,
            0,
            "https://example.com/tip",
            "悬停有提示的链接",
            "悬停提示文本",
            None,
        )
        .unwrap();
        ws.write_url(5, 0, "file:///C:/refs/Book2.xlsx#Sheet1!A1")
            .unwrap();
    }
    let target = wb.add_worksheet();
    target.set_name("Target").unwrap();
    target.write_string(0, 0, "目标单元格").unwrap();
    wb.save(&p).unwrap();
    p
}

/// dim6 敌意 ZIP 族（净室自构造，手构字节装配）。
fn dim6_malicious_zip(dir: &str) -> Vec<(String, Class)> {
    let mut out: Vec<(String, Class)> = Vec::new();

    // 6a 谎言头炸弹：DEFLATE 声明 16B 实际 1MB，CRC 正确——
    // 门 2（verify_actual_sizes）有界计数拦截（zip 8.6 读界无内建
    // 强制，lie86 探针定谳；zip 9 的内建拒绝为 9.x 独有）
    {
        let payload = vec![b'A'; 1 << 20];
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&payload).unwrap();
        let deflated = enc.finish().unwrap();
        let crc = crc32(&payload);
        let name = b"bomb.txt";
        let lie: u32 = 16;
        let mut z = Vec::new();
        z.extend_from_slice(b"PK\x03\x04");
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&8u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(deflated.len() as u32).to_le_bytes());
        z.extend_from_slice(&lie.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(name);
        z.extend_from_slice(&deflated);
        let cd_off = z.len() as u32;
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&8u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(deflated.len() as u32).to_le_bytes());
        z.extend_from_slice(&lie.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(name);
        let cd_size = z.len() as u32 - cd_off;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&cd_size.to_le_bytes());
        z.extend_from_slice(&cd_off.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        let p = format!("{dir}/mal6a_lie_header.zip");
        std::fs::write(&p, &z).unwrap();
        assert!(
            std::fs::metadata(&p).unwrap().len() < 4096,
            "谎言头件本体应极小"
        );
        out.push((p, Class::Reject));
    }

    // 6b 路径逃逸（../ 与 /abs 两形态 + 正常件对照）
    {
        let p = format!("{dir}/mal6b_path_escape.zip");
        build_zip(
            &p,
            &[
                ("[Content_Types].xml", CT.to_vec()),
                ("../evil.txt", b"escape".to_vec()),
                ("/abs/path.txt", b"abs".to_vec()),
                ("ok.txt", b"fine".to_vec()),
            ],
        );
        out.push((p, Class::Reject));
    }

    // 6c 真声明巨量（zip64 STORED 条目声明 320MB > 300MB 族上限）
    {
        let mut z = Vec::new();
        let name = b"blob.bin";
        let declared: u64 = 320_000_000;
        z.extend_from_slice(b"PK\x03\x04");
        z.extend_from_slice(&45u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        z.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        let extra_len_pos = z.len() - 2;
        z.extend_from_slice(name);
        z.extend_from_slice(&0x0001u16.to_le_bytes());
        z.extend_from_slice(&16u16.to_le_bytes());
        z.extend_from_slice(&declared.to_le_bytes());
        z.extend_from_slice(&declared.to_le_bytes());
        let real_extra_len = 20u16;
        z[extra_len_pos..extra_len_pos + 2].copy_from_slice(&real_extra_len.to_le_bytes());
        let lh_size = z.len();
        let cd_off = lh_size as u64;
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&45u16.to_le_bytes());
        z.extend_from_slice(&45u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        z.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        z.extend_from_slice(name);
        z.extend_from_slice(&0x0001u16.to_le_bytes());
        z.extend_from_slice(&24u16.to_le_bytes());
        z.extend_from_slice(&declared.to_le_bytes());
        z.extend_from_slice(&declared.to_le_bytes());
        z.extend_from_slice(&0u64.to_le_bytes());
        let real_cd_extra_len = 28u16;
        // extralen 字段位置 = extra 块起始 - name - 16B 固定位
        //（t10-b 双缺陷修正后的定位算式：首版 extralen 回填位算错
        // 打进 name 末两字节且按裸 0xFFFFFFFF seek 出界）
        let cd_extra_pos = z.len() - 28 - 16 - name.len();
        z[cd_extra_pos..cd_extra_pos + 2].copy_from_slice(&real_cd_extra_len.to_le_bytes());
        let cd_size = z.len() as u32 - cd_off as u32;
        let z64_eocd_off = z.len() as u64;
        // zip64 EOCD：size of record 是 u64（首版误写 u32 致整条记录
        // 错位——t10-d harness 发现后修正）
        z.extend_from_slice(b"PK\x06\x06");
        z.extend_from_slice(&44u64.to_le_bytes());
        z.extend_from_slice(&45u16.to_le_bytes());
        z.extend_from_slice(&45u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&1u64.to_le_bytes());
        z.extend_from_slice(&1u64.to_le_bytes());
        z.extend_from_slice(&(cd_size as u64).to_le_bytes());
        z.extend_from_slice(&cd_off.to_le_bytes());
        z.extend_from_slice(b"PK\x06\x07");
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&z64_eocd_off.to_le_bytes());
        z.extend_from_slice(&1u32.to_le_bytes());
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&0xFFFFu16.to_le_bytes());
        z.extend_from_slice(&0xFFFFu16.to_le_bytes());
        z.extend_from_slice(&0xFFFFu16.to_le_bytes());
        z.extend_from_slice(&0xFFFFu16.to_le_bytes());
        z.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        z.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        let p = format!("{dir}/mal6c_declared_huge.zip");
        std::fs::write(&p, &z).unwrap();
        out.push((p, Class::Reject));
    }

    // 6d 条目数超限（10,001 > 10,000）
    {
        let p = format!("{dir}/mal6d_too_many_entries.zip");
        let f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
        let mut zw = zip::ZipWriter::new(f);
        let mut opts: zip::write::SimpleFileOptions = Default::default();
        opts = opts.compression_method(zip::CompressionMethod::Stored);
        for i in 0..10_001u32 {
            zw.start_file(format!("e{i}.txt"), opts).unwrap();
            zw.write_all(b"x").unwrap();
        }
        zw.finish().unwrap();
        out.push((p, Class::Reject));
    }

    // 6e EOCD 损坏（中央目录偏移低字节破坏）
    {
        let p = format!("{dir}/mal6e_eocd_corrupt.zip");
        let mut z = build_zip_bytes(&[
            ("[Content_Types].xml", CT.to_vec()),
            ("a.txt", b"a".to_vec()),
        ]);
        let n = z.len();
        z[n - 6] = 0xEE;
        std::fs::write(&p, &z).unwrap();
        out.push((p, Class::Err));
    }

    // 6f 缺 [Content_Types].xml（zip 合法、xlsx 语义损坏）
    {
        let p = format!("{dir}/mal6f_no_content_types.zip");
        build_zip(
            &p,
            &[
                ("_rels/.rels", RELS.to_vec()),
                ("xl/workbook.xml", WB.to_vec()),
                ("xl/worksheets/sheet1.xml", b"<worksheet/>".to_vec()),
            ],
        );
        out.push((p, Class::Err));
    }

    out
}

/// dim7 异常 XML 族（最小骨架 + sheet1.xml 篡改）+ 对照件。
fn dim7_abnormal_xml(dir: &str) -> Vec<(String, Class)> {
    let head: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>"#;
    let tail: &[u8] = br#"</t></is></c></row></sheetData></worksheet>"#;
    let mut out: Vec<(String, Class)> = Vec::new();

    let sheet = |mid: &[u8]| -> Vec<u8> {
        let mut s = head.to_vec();
        s.extend_from_slice(mid);
        s.extend_from_slice(tail);
        s
    };

    // 7a 截断（截在 </sheetData 标签内——Phase 2 起门 3 在引擎前
    // 拦截：quick-xml 语法错 → Reject[InvalidPart]；兼容模式下警告
    // 降级后 calamine 自身仍报 ParseError → Err——分界双测钉定）
    {
        let p = format!("{dir}/mal7a_xml_truncated.xlsx");
        let mut s = head.to_vec();
        s.extend_from_slice(b"val");
        s.extend_from_slice(br#"</t></is></c></row></sheetData"#); // 缺 >
        build_min_xlsx(&p, &s);
        out.push((p, Class::Reject));
    }

    // 7b 非法实体
    {
        let p = format!("{dir}/mal7b_bad_entity.xlsx");
        build_min_xlsx(&p, &sheet(b"&bogus;"));
        out.push((p, Class::Err));
    }

    // 7c 非法控制字符（XML 1.0 禁 0x00）——实证钉定：读侧容忍入值
    {
        let p = format!("{dir}/mal7c_control_char.xlsx");
        build_min_xlsx(&p, &sheet(b"a\x00b"));
        out.push((p, Class::Ok));
    }

    // 7d 错误编码声明（声明 UTF-8、实写 GBK 字节）
    {
        let p = format!("{dir}/mal7d_bad_encoding.xlsx");
        build_min_xlsx(&p, &sheet(&[0xD6, 0xD0, 0xCE, 0xC4]));
        out.push((p, Class::Err));
    }

    // 7e 非 XML 二进制垃圾——Phase 2 行为变化（mal7e 类问题闭环）：
    // 不再静默降级空表——默认严格 Reject[InvalidPart]；兼容模式
    // Ok + MalformedXml 警告 + 空表（引擎容忍行为不变）
    {
        let p = format!("{dir}/mal7e_not_xml.xlsx");
        let s: Vec<u8> = (0..256u16).map(|i| i as u8).cycle().take(1024).collect();
        build_min_xlsx(&p, &s);
        out.push((p, Class::Reject));
    }

    // 对照组：正常最小档
    {
        let p = format!("{dir}/ok7_control_normal.xlsx");
        build_min_xlsx(&p, &sheet(b"normal"));
        out.push((p, Class::Ok));
    }

    out
}

// ---------------------------------------------------------------------------
// harness：17 件三分断言 + 锚点（零 panic）
// ---------------------------------------------------------------------------

/// 正向件锚点断言（与 spike read_proto 期望表一致）。
fn anchor_ok(file: &str, b: &Book) -> bool {
    match file.rsplit('/').next().unwrap_or(file) {
        "ok7_control_normal.xlsx" => b.sheets.len() == 1 && b.non_empty_cells() > 0,
        "dim1_formulas.xlsx" => {
            b.sheets.len() == 1
                && b.sheets[0]
                    .formulas
                    .rows()
                    .map(|r| r.iter().filter(|c| !c.is_empty()).count())
                    .sum::<usize>()
                    == 4
        }
        "dim2_styles.xlsx" => b.non_empty_cells() == 6,
        "dim3_merged.xlsx" => b.sheets[0].merges.len() == 3 && b.non_empty_cells() == 4,
        "dim4_multisheet.xlsx" => b.sheet_names() == vec!["Data", "RefSheet", "Calc", "TailEmpty"],
        "dim5_relationships.xlsx" => {
            // v2：6 值格（Links：4 链接格 + 纯文本 + 2 新链接格）+
            // Target 1 格 = 7；五链接形态 + 实体补偿链锚定。
            b.non_empty_cells() == 7
                && b.sheets[0].hyperlinks.len() == 5
                && b.sheets[0]
                    .hyperlinks
                    .iter()
                    .any(|h| h.target.as_deref() == Some("https://example.org/path?q=1&x=2"))
                && b.sheets[0]
                    .hyperlinks
                    .iter()
                    .any(|h| h.location.as_deref() == Some("Sheet1!A1"))
                && b.sheets[0]
                    .hyperlinks
                    .iter()
                    .any(|h| h.tooltip.as_deref() == Some("悬停提示文本"))
        }
        // 容忍度发现锚点（mal7c / mal7e）在专测钉定，此处归类即可
        _ => true,
    }
}

#[test]
fn corpus_seventeen_files_three_way_classification() {
    let dir = out_dir("all");

    // 正向五维（构造顺序 = manifest 序）
    let positive: Vec<String> = vec![
        dim1_formulas(&dir),
        dim2_styles(&dir),
        dim3_merged(&dir),
        dim4_multisheet(&dir),
        dim5_relationships(&dir),
    ];

    let mut cases: Vec<(String, Class)> = Vec::new();
    // 敌意 ⑥ 容器族 + ⑦ 异常 XML 族 + 对照件
    cases.extend(dim6_malicious_zip(&dir));
    cases.extend(dim7_abnormal_xml(&dir));

    // 期望表（三分 + 锚点闭包）——序 = spike read_proto cases 表
    let expected: &[(Class, bool)] = &[
        (Class::Reject, false), // mal6a 谎言头 → 门 2
        (Class::Reject, false), // mal6b 路径逃逸
        (Class::Reject, false), // mal6c 声明巨量
        (Class::Reject, false), // mal6d 条目数超限
        (Class::Err, false),    // mal6e EOCD 损坏
        (Class::Err, false),    // mal6f 缺 Content_Types
        (Class::Reject, false), // mal7a 标签内截断 → 门 3（Phase 2）
        (Class::Err, false),    // mal7b 非法实体（引擎层错误）
        (Class::Ok, false),     // mal7c 0x00 容忍（专测钉锚点 + 警告面）
        (Class::Err, false),    // mal7d 编码错
        (Class::Reject, false), // mal7e 垃圾部件 → 门 3（Phase 2 闭环）
        (Class::Ok, false),     // ok7 对照
    ];
    assert_eq!(cases.len(), expected.len(), "敌意/对照件构造数与期望表对齐");

    let mut failures: Vec<String> = Vec::new();
    let mut total = 0usize;

    // 正向五维：全 OK + 锚点
    for p in &positive {
        total += 1;
        let r = read_book(p);
        let actual = classify(&r);
        let file = p.rsplit('/').next().unwrap_or(p);
        match &r {
            Ok(b) => {
                if actual != Class::Ok || !anchor_ok(p, b) {
                    failures.push(format!("{file}: 正向件锚点或归类失败（{:?}）", actual));
                }
            }
            Err(e) => failures.push(format!("{file}: 正向件意外失败：{e}")),
        }
    }

    // 敌意/对照 12 件：三分归类
    for (i, (p, _)) in cases.iter().enumerate() {
        total += 1;
        let r = read_book(p);
        let actual = classify(&r);
        let (want, _) = expected[i];
        let file = p.rsplit('/').next().unwrap_or(p);
        if actual != want {
            let detail = match &r {
                Ok(_) => "unexpected Ok".to_string(),
                Err(e) => e.to_string(),
            };
            failures.push(format!(
                "{file}: expected {} got {} [{detail}]",
                want.tag(),
                actual.tag()
            ));
        }
    }

    assert_eq!(total, 17, "17 件全语料（正向 5 + 敌意 11 + 对照 1）");
    assert!(
        failures.is_empty(),
        "语料 harness 失败 {} 件：\n{}",
        failures.len(),
        failures.join("\n")
    );

    // 清理（best-effort）
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corpus_tolerance_discoveries_anchored() {
    // mal7c：0x00 读侧容忍入值（读侧不过滤——再输出须过消毒链）；
    // Phase 2 起恒登记 ControlChars 结构化警告（两模式同）。
    let dir = out_dir("tol");
    let head: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>"#;
    let tail: &[u8] = br#"</t></is></c></row></sheetData></worksheet>"#;
    let mut s = head.to_vec();
    s.extend_from_slice(b"a\x00b");
    s.extend_from_slice(tail);
    let p7c = format!("{dir}/mal7c.xlsx");
    build_min_xlsx(&p7c, &s);
    let b = read_book(&p7c).unwrap();
    assert!(
        matches!(b.sheets[0].values.get((0, 0)), Some(Data::String(v)) if v.contains('\u{0}')),
        "mal7c：值应含 NUL（容忍度实证锚点）"
    );
    assert!(
        b.warnings
            .iter()
            .any(|w| w.kind == WarningKind::ControlChars && w.part == "xl/worksheets/sheet1.xml"),
        "mal7c：控制字符发现应登记为结构化警告：{:?}",
        b.warnings
    );

    // mal7e（Phase 2 闭环）：垃圾部件默认严格拒绝——不再静默降级
    // 空表；兼容模式 = Ok + MalformedXml 警告 + 空表（引擎容忍不变）。
    let garbage: Vec<u8> = (0..256u16).map(|i| i as u8).cycle().take(1024).collect();
    let p7e = format!("{dir}/mal7e.xlsx");
    build_min_xlsx(&p7e, &garbage);
    match read_book(&p7e) {
        Err(ReadError::InvalidPart { part, .. }) => {
            assert_eq!(part, "xl/worksheets/sheet1.xml")
        }
        other => panic!("mal7e：严格模式应 InvalidPart，got {other:?}"),
    }
    let b = read_book_with_options(&p7e, &ReadOptions { compat: true }).unwrap();
    assert_eq!(b.sheets.len(), 1);
    assert_eq!(
        b.non_empty_cells(),
        0,
        "mal7e 兼容：降级为 1 表 0 格（警告可见）"
    );
    assert!(b
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::MalformedXml));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corpus_compat_mode_classification() {
    // Phase 2 兼容模式全语料复扫：只有「门 3 拦下的畸形部件」分类
    // 变化（Reject → Ok[警告] / Reject → Err[警告后引擎自身报错]），
    // 门 1/门 2/引擎错误面恒不变（兼容不放松安全）。
    let dir = out_dir("compat");
    let mut cases: Vec<(String, Class)> = Vec::new();
    cases.extend(dim6_malicious_zip(&dir));
    cases.extend(dim7_abnormal_xml(&dir));
    let compat = ReadOptions { compat: true };

    // 兼容模式期望表（与严格表逐位对照——变化仅门 3 两项）：
    let expected: &[Class] = &[
        Class::Reject, // mal6a 谎言头（门 2 恒拒——兼容不放松）
        Class::Reject, // mal6b 路径逃逸（门 1 恒拒）
        Class::Reject, // mal6c 声明巨量（门 1 恒拒）
        Class::Reject, // mal6d 条目数超限（门 1 恒拒）
        Class::Err,    // mal6e EOCD 损坏（容器面恒不变）
        Class::Err,    // mal6f 缺 Content_Types（引擎面恒不变）
        Class::Err,    // mal7a 标签内截断（警告降级后引擎 ParseError）
        Class::Err,    // mal7b 非法实体（门 3 不拦实体名，引擎错）
        Class::Ok,     // mal7c 控制字符（恒容忍 + 恒警告）
        Class::Err,    // mal7d 编码错（引擎面恒不变）
        Class::Ok,     // mal7e 垃圾部件（警告降级 + 引擎容忍空表）
        Class::Ok,     // ok7 对照
    ];
    assert_eq!(cases.len(), expected.len());

    let mut failures = Vec::new();
    for (i, (p, _)) in cases.iter().enumerate() {
        let r = read_book_with_options(p, &compat);
        let actual = classify(&r);
        if actual != expected[i] {
            let file = p.rsplit('/').next().unwrap_or(p);
            let detail = match &r {
                Ok(_) => "unexpected Ok".to_string(),
                Err(e) => e.to_string(),
            };
            failures.push(format!(
                "{file}: compat expected {} got {} [{detail}]",
                expected[i].tag(),
                actual.tag()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "兼容模式归类失败 {} 件：\n{}",
        failures.len(),
        failures.join("\n")
    );

    let _ = std::fs::remove_dir_all(&dir);
}
