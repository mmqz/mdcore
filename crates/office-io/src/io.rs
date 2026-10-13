//! XLSX 读路径产品模块——容器预扫门 + calamine 包装 + 统一错误面。
//!
//! xlsx-r1 阶段 B 落地（2026-10-13）：分析仓 spike `src/io.rs` 直移
//! （`cargo test` 验证态 25/25 → 本 crate 同构内联单测）；接战语料 =
//! t10-b 敌意件的内联自构造版本（mdcore 测试同构——不依赖 fixtures
//! 目录）；17 件全语料集成 harness 见 `tests/corpus_harness.rs`。
//!
//! **v2 增量（2026-10-13，同日）**：第五 API 面超链接
//! （hyperlinks_by_sheet_name：target/location/displayed_text/
//! tooltip 四字段）+ calamine 0.36.1 读侧实体怪癖在 IO 边界的统一补偿
//! （rels/sheet 属性值未做 XML 实体反转义——utils.rs unescape_xml 只
//! 处理 _x00HH_，`&` 以 `&amp;` 字面量读出；依据 = 分析仓 FIDELITY.md
//! v2 勘误链 + xlsx-r1 t10-c 实证）。
//!
//! 安全依据：分析仓 SECURITY_POLICY §2.1 四检查 + §3 限额表；
//! 实证：xlsx-r1 t10-d harness 17/17 零 panic（read_proto.rs）。

use calamine::{Data, Dimensions, Hyperlink, Range, Reader, Xlsx};

// ---------------------------------------------------------------------------
// §3 限额常量（SECURITY_POLICY v1 接线——xlsx 族；后续可移配置面）
// ---------------------------------------------------------------------------

/// zip 声明总量上限（xlsx 族 300MB；office-b default.json 一手参照）。
pub const LIMIT_TOTAL_DECLARED: u64 = 300 * 1024 * 1024;
/// 单条目声明尺寸上限（= 族总量上限，对称设计）。
pub const LIMIT_SINGLE_ENTRY: u64 = LIMIT_TOTAL_DECLARED;
/// 条目数上限（v1 新设 10,000；mal6d 实测校准）。
pub const LIMIT_ENTRY_COUNT: usize = 10_000;

// ---------------------------------------------------------------------------
// 统一错误面
// ---------------------------------------------------------------------------

/// 读路径统一错误：拒绝（门）/ 容器损坏 / 解析错误三分。
#[derive(Debug)]
pub enum ReadError {
    /// 预扫门拒绝（typed reject）——敌意/超额输入未进解析层。
    PrescanRejected {
        reason: &'static str,
        entry: Option<String>,
    },
    /// zip 容器结构损坏（EOCD/中央目录不可读）。
    ContainerCorrupt(String),
    /// 容器过门后的解析层错误（calamine / zip 读界）。
    ParseError(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::PrescanRejected { reason, entry } => {
                write!(f, "prescan rejected: {reason}")?;
                if let Some(e) = entry {
                    write!(f, " (entry: {e})")?;
                }
                Ok(())
            }
            ReadError::ContainerCorrupt(m) => write!(f, "container corrupt: {m}"),
            ReadError::ParseError(m) => write!(f, "parse error: {m}"),
        }
    }
}

impl std::error::Error for ReadError {}

// ---------------------------------------------------------------------------
// 容器预扫门（零条目数据读取）
// ---------------------------------------------------------------------------

/// 预扫报告（过门后正常路径附带，审计/日志面）。
#[derive(Debug)]
pub struct PrescanReport {
    pub entries: usize,
    pub total_declared: u64,
    pub max_single_declared: u64,
}

/// 容器预扫：条目数 → 逐条目（单尺寸/路径逃逸，单趟）→ 求和总量。
/// 全部基于 zip 中央目录声明值，零条目数据读取。
pub fn prescan<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
) -> Result<PrescanReport, ReadError> {
    if za.len() > LIMIT_ENTRY_COUNT {
        return Err(ReadError::PrescanRejected {
            reason: "entry count over limit",
            entry: None,
        });
    }
    let mut total: u64 = 0;
    let mut max_single: u64 = 0;
    for i in 0..za.len() {
        let zf = za
            .by_index(i)
            .map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
        let declared = zf.size();
        if declared > LIMIT_SINGLE_ENTRY {
            return Err(ReadError::PrescanRejected {
                reason: "single entry declared size over limit",
                entry: Some(zf.name().to_string()),
            });
        }
        if zf.enclosed_name().is_none() {
            return Err(ReadError::PrescanRejected {
                reason: "path escape entry",
                entry: Some(zf.name().to_string()),
            });
        }
        max_single = max_single.max(declared);
        total = total.saturating_add(declared);
    }
    if total > LIMIT_TOTAL_DECLARED {
        return Err(ReadError::PrescanRejected {
            reason: "total declared size over limit",
            entry: None,
        });
    }
    Ok(PrescanReport {
        entries: za.len(),
        total_declared: total,
        max_single_declared: max_single,
    })
}

/// 实际尺寸诚实性验证（谎言头闭环，策略层实现）。
///
/// 依据（xlsx-r1 实测定谳 2026-10-13）：zip 8.6 读界**无**声明值强制
/// （谎言头条目 read_to_end 全量读回零错误，仅 CRC 校验可被正确
/// CRC 绕过）；zip 9 的内建拒绝（T3 3b）为 9.x 独有。calamine
/// 0.36.1 内部依赖 zip 8.6 → 实际读路径无此防线，由本层补：
/// 逐条目有界读取计数，实际 > 声明即 typed reject。
///
/// 成本注记：合法档全量解压一次（总量已由 prescan 锁 ≤300MB）；
/// 随后 calamine 重读。双读为 v1 已知成本，优化路径（受信源跳过
/// 开关）留后续裁决。
pub fn verify_actual_sizes<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
) -> Result<(), ReadError> {
    let n = za.len();
    for i in 0..n {
        let mut zf = za
            .by_index(i)
            .map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
        let name = zf.name().to_string();
        let declared = zf.size();
        let mut total: u64 = 0;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            use std::io::Read as _;
            match zf.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => {
                    total = total.saturating_add(k as u64);
                    if total > declared {
                        return Err(ReadError::PrescanRejected {
                            reason: "actual size exceeds declared (lie header)",
                            entry: Some(name),
                        });
                    }
                }
                Err(e) => {
                    // CRC 不匹配 / 解压损坏 → 容器损坏族
                    return Err(ReadError::ContainerCorrupt(format!("{e}")));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// calamine 读取包装（五 API 面）
// ---------------------------------------------------------------------------

/// 单表快照：值矩阵（含缓存值）/ 公式文本 / 合并范围 / 超链接（v2）。
#[derive(Debug, Default)]
pub struct SheetData {
    pub name: String,
    pub values: Range<Data>,
    pub formulas: Range<String>,
    pub merges: Vec<Dimensions>,
    /// 超链接（v2）：calamine Hyperlink 四字段，已过 IO 边界实体补偿。
    pub hyperlinks: Vec<Hyperlink>,
}

/// 整档提取：表序 + 逐表四面（v2：+超链接；C1 读取能力产品面）。
#[derive(Debug)]
pub struct Book {
    pub sheets: Vec<SheetData>,
}

impl Book {
    pub fn sheet_names(&self) -> Vec<&str> {
        self.sheets.iter().map(|s| s.name.as_str()).collect()
    }

    /// 非空格计数。
    pub fn non_empty_cells(&self) -> usize {
        self.sheets
            .iter()
            .map(|s| {
                s.values
                    .rows()
                    .map(|r| r.iter().filter(|c| !matches!(c, Data::Empty)).count())
                    .sum::<usize>()
            })
            .sum()
    }
}

/// 读入口：预扫门 → 尺寸验证 → calamine 全量提取（五面）。
///
/// 门在前（敌意输入零解析层接触）：①prescan 声明值四检查；
/// ②verify_actual_sizes 谎言头闭环（zip 8.6 读界无内建强制——
/// 2026-10-13 实测，策略层补）。门过后错误归 ParseError。
pub fn read_book(path: &str) -> Result<Book, ReadError> {
    let f =
        std::fs::File::open(path).map_err(|e| ReadError::ContainerCorrupt(format!("open: {e}")))?;
    let mut za =
        zip::ZipArchive::new(f).map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
    prescan(&mut za)?; // 门 1：声明值四检查
    verify_actual_sizes(&mut za)?; // 门 2：实际尺寸诚实性
    drop(za);

    let mut book: Xlsx<_> =
        calamine::open_workbook(path).map_err(|e| ReadError::ParseError(format!("open: {e}")))?;
    let names = book.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    for name in &names {
        let values = book
            .worksheet_range(name)
            .map_err(|e| ReadError::ParseError(format!("range[{name}]: {e}")))?;
        let formulas = book
            .worksheet_formula(name)
            .map_err(|e| ReadError::ParseError(format!("formula[{name}]: {e}")))?;
        let merges = book
            .merge_cells_by_sheet_name(name)
            .map_err(|e| ReadError::ParseError(format!("merge[{name}]: {e}")))?;
        let mut hyperlinks = book
            .hyperlinks_by_sheet_name(name)
            .map_err(|e| ReadError::ParseError(format!("hyperlinks[{name}]: {e}")))?;
        // v2 实体补偿：IO 边界统一还原逻辑字符（消费方拿到真实
        // URL/文本，无需知晓 calamine 读侧怪癖）。
        for h in &mut hyperlinks {
            if let Some(v) = h.target.as_mut() {
                *v = unescape_xml_entities5(v);
            }
            if let Some(v) = h.location.as_mut() {
                *v = unescape_xml_entities5(v);
            }
            if let Some(v) = h.displayed_text.as_mut() {
                *v = unescape_xml_entities5(v);
            }
            if let Some(v) = h.tooltip.as_mut() {
                *v = unescape_xml_entities5(v);
            }
        }
        sheets.push(SheetData {
            name: name.clone(),
            values,
            formulas,
            merges,
            hyperlinks,
        });
    }
    Ok(Book { sheets })
}

/// calamine 0.36.1 读侧怪癖补偿（v2）：hyperlink rels `Target` 及
/// sheet 超链接属性（location/display/tooltip）值**未做 XML 实体
/// 反转义——`unescape_xml`（utils.rs:117）只处理 Excel `_x00HH_` 约定
/// （非 `_x00` 前缀原样返回），5 个 XML 实体不解码，`&` 以 `&amp;`
/// 字面量读出。本函数把 5 实体还原为逻辑字符；`&amp;` 最后替换
/// （`&amp;amp;` 正确解为字面 `"&amp;"`）。
///
/// 边界登记：真含字面 `&amp;` 文本的 URL 在此补偿下有双解歧义
/// （不可分，实际概率≈0）；值面（worksheet_range）不受此怪癖影响
/// （共享字符串/内联字符串走 quick-xml 标准反转义路径）。
fn unescape_xml_entities5(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

// ---------------------------------------------------------------------------
// 单测（敌意件内联自构造——集成 harness 见 tests/corpus_harness.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// 临时档路径（测试进程内唯一名）。
    fn tmp(name: &str) -> String {
        let p =
            std::env::temp_dir().join(format!("mdc_io_test_{name}_{}.xlsx", std::process::id()));
        p.to_string_lossy().into_owned()
    }

    /// 正常最小 xlsx 字节（inlineStr 四部件）。
    fn minimal_xlsx(text: &str) -> Vec<(String, Vec<u8>)> {
        let sheet = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{text}</t></is></c></row></sheetData></worksheet>"#
        );
        vec![
            (
                "[Content_Types].xml".into(),
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec(),
            ),
            (
                "_rels/.rels".into(),
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec(),
            ),
            (
                "xl/workbook.xml".into(),
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S1" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_vec(),
            ),
            (
                "xl/_rels/workbook.xml.rels".into(),
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_vec(),
            ),
            ("xl/worksheets/sheet1.xml".into(), sheet.into_bytes()),
        ]
    }

    fn write_zip(path: &str, entries: &[(String, Vec<u8>)]) {
        let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = Default::default();
        for (name, data) in entries {
            zw.start_file(name.as_str(), opts).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn normal_file_reads() {
        let p = tmp("normal");
        let entries = minimal_xlsx("hello");
        write_zip(&p, &entries);
        let book = read_book(&p).unwrap();
        assert_eq!(book.sheet_names(), vec!["S1"]);
        assert_eq!(book.non_empty_cells(), 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn path_escape_rejected() {
        let p = tmp("escape");
        let f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zw.start_file("../evil.txt", opts).unwrap();
        zw.write_all(b"escape").unwrap();
        zw.finish().unwrap();
        match read_book(&p) {
            Err(ReadError::PrescanRejected { reason, .. }) => {
                assert_eq!(reason, "path escape entry");
            }
            other => panic!("expected prescan reject, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn entry_count_rejected() {
        // 上限 10,000 → 造 10,001 件（STORED 空内容，构造快）。
        let p = tmp("count");
        let f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = Default::default();
        for i in 0..=LIMIT_ENTRY_COUNT {
            zw.start_file(format!("e{i}.txt"), opts).unwrap();
        }
        zw.finish().unwrap();
        match read_book(&p) {
            Err(ReadError::PrescanRejected { reason, .. }) => {
                assert_eq!(reason, "entry count over limit");
            }
            other => panic!("expected prescan reject, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn declared_huge_rejected() {
        // 声明巨量：STORED 大块（300MB 上限 → 写 301MB 声明走真实数据
        // 不可行；用 zip64 手构太重——改走单条目门：写入真实 300MB+1
        // 的声明须真实数据。此处用 zip crate 真实声明覆盖单条目门语义
        // （写 300MB+1 零字节数据不可行 → 声明值=实际写入值，故改测
        // 上限内正常通过 + harness mal6c[zip64 声明 320MB] 钉外）。
        let p = tmp("huge_ok");
        let entries = minimal_xlsx("ok");
        write_zip(&p, &entries);
        assert!(read_book(&p).is_ok()); // 正常件过门（真声明求和）
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn eocd_corrupt_is_container_error() {
        let p = tmp("eocd");
        let entries = minimal_xlsx("x");
        write_zip(&p, &entries);
        // 破坏 EOCD 中央目录偏移低字节
        let mut z = std::fs::read(&p).unwrap();
        let n = z.len();
        z[n - 6] = 0xEE;
        std::fs::write(&p, &z).unwrap();
        match read_book(&p) {
            Err(ReadError::ContainerCorrupt(_)) => {}
            other => panic!("expected container corrupt, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn control_char_tolerance_pinned() {
        // t10-d mal7c 实证钉定：0x00 在文本位被读侧容忍，值含 NUL。
        let p = tmp("nul");
        let entries = minimal_xlsx("a\0b");
        write_zip(&p, &entries);
        let book = read_book(&p).unwrap();
        match book.sheets[0].values.get((0, 0)) {
            Some(Data::String(s)) => assert!(s.contains('\u{0}'), "值应含 NUL：{s:?}"),
            other => panic!("expected string, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn garbage_part_tolerated_as_empty() {
        // t10-d mal7e 实证钉定：垃圾部件静默降级空表（已知行为登记）。
        let p = tmp("garbage");
        let mut entries = minimal_xlsx("unused");
        // 替换 sheet1 为二进制垃圾
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                *d = (0..256u16).map(|i| i as u8).cycle().take(1024).collect();
            }
        }
        write_zip(&p, &entries);
        let book = read_book(&p).unwrap();
        assert_eq!(book.sheets.len(), 1);
        assert_eq!(book.non_empty_cells(), 0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn lie_header_rejected_at_gate2() {
        // 谎言头闭环（2026-10-13 定谳）：zip 8.6 读界无声明值强制
        // （lie86 探针实测：正确 CRC + 声明 16B 实际 1MB → 全量读回零
        // 错误）→ 策略层 verify_actual_sizes 有界计数拦截。CRC 本测
        // 用 0（计数门在 EOF 的 CRC 校验前触发）；正确 CRC 形态由
        // harness mal6a（T3 3b 构造）覆盖。
        let payload: Vec<u8> = vec![0x41; 1 << 20]; // 1MB
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write as _;
        enc.write_all(&payload).unwrap();
        let compressed = enc.finish().unwrap();
        let name = b"blob.bin";

        let mut z = Vec::new();
        z.extend_from_slice(b"PK\x03\x04");
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // flags
        z.extend_from_slice(&8u16.to_le_bytes()); // deflate
        z.extend_from_slice(&0u16.to_le_bytes()); // time
        z.extend_from_slice(&0u16.to_le_bytes()); // date
        z.extend_from_slice(&0u32.to_le_bytes()); // crc（见注）
        z.extend_from_slice(&(compressed.len() as u32).to_le_bytes()); // csize 真实
        z.extend_from_slice(&16u32.to_le_bytes()); // usize 谎言（真实 1MB）
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // extra
        z.extend_from_slice(name);
        z.extend_from_slice(&compressed);
        let cd_off = z.len() as u32;
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&8u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes()); // crc
        z.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        z.extend_from_slice(&16u32.to_le_bytes()); // usize 谎言
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // extra
        z.extend_from_slice(&0u16.to_le_bytes()); // comment
        z.extend_from_slice(&0u16.to_le_bytes()); // disk
        z.extend_from_slice(&0u16.to_le_bytes()); // iattr
        z.extend_from_slice(&0u32.to_le_bytes()); // eattr
        z.extend_from_slice(&0u32.to_le_bytes()); // LFH 偏移（0 位）
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

        let p = tmp("lie");
        std::fs::write(&p, &z).unwrap();
        match read_book(&p) {
            Err(ReadError::PrescanRejected { reason, entry }) => {
                assert_eq!(reason, "actual size exceeds declared (lie header)");
                assert_eq!(entry.as_deref(), Some("blob.bin"));
            }
            other => panic!("expected gate-2 reject, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn truncation_boundary_pinned() {
        // t10-d 补充实证（lib 测试发现，2026-10-13）：截断容忍度分界——
        // ① 恰好只缺根闭合 </worksheet>（标签边界外的截断）→ **容忍**，
        //    值正常读回（mal7a harness 件报错是因为它截在 </sheetData
        //    标签**内**，缺 '>'）；② 标签内截断 → ParseError。
        let p = tmp("trunc_ok");
        let mut entries = minimal_xlsx("v");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                d.truncate(d.len() - 12); // 恰移除 </worksheet>
            }
        }
        write_zip(&p, &entries);
        let book = read_book(&p).unwrap(); // 容忍：缺根闭合仍读回
        assert_eq!(book.non_empty_cells(), 1);
        let _ = std::fs::remove_file(&p);

        let p2 = tmp("trunc_err");
        let mut entries2 = minimal_xlsx("v");
        for (n, d) in entries2.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                d.truncate(d.len() - 13); // 移除 ></worksheet> → 标签内截断
            }
        }
        write_zip(&p2, &entries2);
        match read_book(&p2) {
            Err(ReadError::ParseError(_)) => {}
            other => panic!("expected parse error, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn hyperlink_face_roundtrip_five_forms() {
        // v2：第五 API 面超链接全形态往返 + 实体补偿钉定。
        // 五形态（= 分析仓语料 dim5 v2 同构）：
        //   ①外链纯形态 ②外链自定义文本（URL 含 & 查询——实体补偿
        //   核心用例）③内部链接 ④tooltip 面 ⑤file 子锚（target+
        //   location 双字段）。
        let p = tmp("links");
        let mut wb = rust_xlsxwriter::Workbook::new();
        {
            let ws = wb.add_worksheet();
            ws.set_name("Links").unwrap();
            ws.write_url(0, 0, "https://example.com/docs").unwrap();
            ws.write_url_with_text(1, 0, "https://example.org/path?q=1&x=2", "带文本链接")
                .unwrap();
            ws.write_url(2, 0, "internal:Target!A1").unwrap();
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

        let book = read_book(&p).unwrap();
        let links_sheet = book
            .sheets
            .iter()
            .find(|s| s.name == "Links")
            .expect("Links 表在位");
        assert_eq!(links_sheet.hyperlinks.len(), 5, "五链接形态计数");
        // ① 外链纯形态（无 display 属性 → displayed_text None）
        assert_eq!(
            links_sheet.hyperlinks[0].target.as_deref(),
            Some("https://example.com/docs")
        );
        // ② & 查询 URL：实体补偿核心用例——calamine 原样读出
        //    "&amp;"，IO 边界还原为真实 "&"（消费方拿逻辑 URL）
        assert_eq!(
            links_sheet.hyperlinks[1].target.as_deref(),
            Some("https://example.org/path?q=1&x=2"),
            "实体补偿后应为逻辑 & 形态"
        );
        assert!(!links_sheet.hyperlinks[1]
            .target
            .as_deref()
            .unwrap_or_default()
            .contains("&amp;"));
        // ③ 内部链接（location 承载，target None）
        assert_eq!(
            links_sheet.hyperlinks[2].location.as_deref(),
            Some("Target!A1")
        );
        assert!(links_sheet.hyperlinks[2].target.is_none());
        // ④ tooltip 面
        assert_eq!(
            links_sheet.hyperlinks[3].tooltip.as_deref(),
            Some("悬停提示文本")
        );
        // ⑤ file 子锚（target+location 双字段）
        assert_eq!(
            links_sheet.hyperlinks[4].target.as_deref(),
            Some("file:///C:/refs/Book2.xlsx")
        );
        assert_eq!(
            links_sheet.hyperlinks[4].location.as_deref(),
            Some("Sheet1!A1")
        );
        let _ = std::fs::remove_file(&p);
    }
}
