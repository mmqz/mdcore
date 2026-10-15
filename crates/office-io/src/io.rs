//! XLSX 读路径产品模块——容器预扫门 + 尺寸/部件融合验证 + calamine
//! 包装 + 统一错误面。
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
//! **xlsx-r2 Phase 2 增量（2026-10-14，用户三阶段指令）**：门 3 部件
//! 良构性验证 + 默认严格模式 + 显式兼容模式。mal7e 类问题闭环：
//! 畸形/无效 XML 部件**不再静默变空/部分数据**——默认严格拒拒
//! （[`ReadError::InvalidPart`]）；[`ReadOptions::compat`] 显式兼容
//! 模式下降级为结构化警告（[`Book::warnings`]，数据照常提取）。
//! 门 3 融合在门 2 的既有解压趟内（零额外解压成本），校验面 =
//! 标签配对（quick-xml check_end_names）+ 根元素唯一/在位 + EOF
//! 未闭合元素（截断检测）+ 元素深度上限（[`LIMIT_XML_DEPTH`]，
//! 嵌套炸弹面——SECURITY_POLICY §2.2.3 挂账项闭项）+ 控制字符
//! 发现登记（XML 1.0 非法字节恒警告——mal7c 值面容忍行为不变，
//! 见模块尾容忍度登记）。门 1/门 2 与全部安全上限在两种模式下
//! **恒等**（兼容模式不放松任何安全防线）。
//!
//! 安全依据：分析仓 SECURITY_POLICY §2.1 四检查 + §3 限额表 +
//! §2.1 第 5 项（门 3，v1.1）；实证：xlsx-r1 t10-d harness 17/17
//! 零 panic（read_proto.rs）+ 本模块 Phase 2 测试组。
//!
//! **t17 增量（2026-10-14）**：字节面读入口
//! （[`read_book_from_bytes`] / [`read_book_from_bytes_with_options`]）
//! ——wasm 桥消费面（wasm32 无文件系统），门结构与路径入口恒等
//! （门 1/门 2+3/全部上限两模式恒同）；五面提取抽出共享核
//! `extract_sheets`（两入口同一实现——行为单一真源）。
//!
//! **r4-t1 增量（2026-10-17，Round 4 T1——XLSX 类型推断与转换规则，
//! 用户裁决落地）**：读面新增**类型化单元格值模型**（[`CellData`] /
//! [`CellGrid`] / [`SheetData::typed`]）——三分纪律：**原始值**（枚举
//! 载荷 = 存储面事实）/ **推断类型**（变体即 kind，日期类携带依据）/
//! **显式转换**（[`CellData::as_iso_datetime`] 等——消费方明确调用，
//! 映射层不做静默语义转换）。kind 判定 = calamine 存储类型 + numFmt
//! 证据的忠实转译（**零新增推断**）：纯序列数无 numFmt 证据不认定
//! 日期；前导零/数字文本原样保留不转数字；错误格保留码文本；公式格
//! = 缓存值（[`SheetData::values`] / [`SheetData::typed`]）与公式文本
//! （[`SheetData::formulas`] / [`SheetData::formula_at`]）**两通道明确
//! 区分**（禁止把缓存值误当公式文本）。读-改-重建类型保持联接 =
//! [`SheetData::replay_cell`]（kind 分派 + 纪元换算 + numFmt 样式
//! 配对单调用点）。规则依据表 = 分析仓 xlsx-r1/FIDELITY.md 类型规则
//! 节 + xlsx-r2/REPORT.md r4-t1 节（每条规则一行：规则/依据/测试锚点）。

use calamine::{
    Data, Dimensions, ExcelDateTime, ExcelDateTimeType, Hyperlink, Range, Reader, Xlsx,
};

// ---------------------------------------------------------------------------
// §3 限额常量（SECURITY_POLICY v1 接线——xlsx 族；后续可移配置面）
// ---------------------------------------------------------------------------

/// zip 声明总量上限（xlsx 族 300MB；office-b default.json 一手参照）。
pub const LIMIT_TOTAL_DECLARED: u64 = 300 * 1024 * 1024;
/// 单条目声明尺寸上限（= 族总量上限，对称设计）。
pub const LIMIT_SINGLE_ENTRY: u64 = LIMIT_TOTAL_DECLARED;
/// 条目数上限（v1 新设 10,000；mal6d 实测校准）。
pub const LIMIT_ENTRY_COUNT: usize = 10_000;
/// XML 部件元素嵌套深度上限（门 3，Phase 2）。真实 xlsx 部件深度
/// 个位到十位级（worksheet/共享字符串嵌套）；嵌套炸弹面在此截断
/// （quick-xml 拉取式无栈溢出风险，上限防下游消费面）。
pub const LIMIT_XML_DEPTH: usize = 1_000;

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
    /// 门 3 部件良构性拒绝（Phase 2 默认严格）：畸形 XML 部件不再
    /// 静默变空/部分数据——typed reject；[`ReadOptions::compat`]
    /// 兼容模式下降级为 [`CompatibilityWarning`]。
    InvalidPart { part: String, detail: String },
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
            ReadError::InvalidPart { part, detail } => {
                write!(f, "invalid part: {part}: {detail}")
            }
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

// ---------------------------------------------------------------------------
// 兼容模式类型（Phase 2：默认严格 + 显式兼容）
// ---------------------------------------------------------------------------

/// 结构化警告类别（兼容模式下降级的畸形部件 + 任何模式下恒登记的
/// 控制字符发现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// 畸形 XML 部件（兼容模式容忍：垃圾/截断/失衡/多根/超深）。
    MalformedXml,
    /// XML 1.0 非法控制字节（0x00-0x08/0x0B/0x0C/0x0E-0x1F）在部件内
    /// 发现——值面容忍行为不变（mal7c 登记：读侧不过滤，再输出须过
    /// mdc-office 消毒链），此处仅提升可见性（两模式恒登记）。
    ControlChars,
    /// 跨部件引用悬空（docx-r1：r:id 无关系条目 / 关系目标部件缺失
    /// / 目标路径越出包根——兼容模式容忍降级：目标不暴露或槽位跳过，
    /// 警告随文档返回；严格模式为 typed reject 不产警告）。XLSX 面
    /// 无此类别使用面（关系引用不进 XLSX 模型）。
    BrokenReference,
}

/// 兼容模式结构化警告（随 [`Book::warnings`] 返回）。
#[derive(Debug, Clone, PartialEq)]
pub struct CompatibilityWarning {
    pub part: String,
    pub kind: WarningKind,
    pub detail: String,
}

/// 读路径选项（Phase 2）。
///
/// - 默认（`compat: false`）= **严格模式**：畸形 XML 部件 typed
///   reject（[`ReadError::InvalidPart`]）——mal7e 类静默空/部分数据
///   闭环；
/// - `compat: true` = **兼容模式**：畸形部件降级为结构化警告，数据
///   照常提取（下游 calamine 自身仍可能报错——兼容模式只放宽本层
///   验证，不改变引擎行为）；
/// - **门 1/门 2 与全部安全上限在两模式恒等**（兼容不放松安全）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadOptions {
    pub compat: bool,
}

// ---------------------------------------------------------------------------
// 门 2+3 融合验证趟（单次解压：尺寸诚实性 + 部件良构性）
// ---------------------------------------------------------------------------

/// 尺寸计数读适配器：包裹 zip 条目，读时累计实际字节（门 2 谎言头
/// 检出：实际 > 声明即置位）+ 扫描 XML 1.0 非法控制字节（门 3 登记面）。
struct CountingReader<'a, R> {
    inner: &'a mut R,
    declared: u64,
    count: u64,
    lied: bool,
    /// 仅 XML 部件扫描控制字节（非 XML 二进制部件不适用 XML 1.0
    /// 字符律——如内嵌图片本就是任意字节）。
    scan_ctrl: bool,
    ctrl_at: Option<u64>,
}

impl<'a, R> CountingReader<'a, R> {
    fn new(inner: &'a mut R, declared: u64, scan_ctrl: bool) -> Self {
        Self {
            inner,
            declared,
            count: 0,
            lied: false,
            scan_ctrl,
            ctrl_at: None,
        }
    }
}

impl<R: std::io::Read> std::io::Read for CountingReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        for &b in &buf[..n] {
            // XML 1.0 Char 产法定律：仅 #x9/#xA/#xD 与 #x20 起合法
            //（仅 XML 部件域扫描）。
            if self.scan_ctrl
                && matches!(b, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F)
                && self.ctrl_at.is_none()
            {
                self.ctrl_at = Some(self.count);
            }
            self.count += 1;
            if self.count > self.declared {
                self.lied = true;
            }
        }
        Ok(n)
    }
}

/// 门 3 流式结构校验：quick-xml 拉取解析 + 自有深度/根追踪。
///
/// 校验面（全部结构级，非 DTD/实体级——GeneralRef 透传不展开，
/// 十亿笑声面免疫）：标签配对（quick-xml check_end_names 默认 true）/
/// 根元素在位且唯一 / EOF 未闭合元素（截断） / 深度上限
/// （[`LIMIT_XML_DEPTH`]）。
fn validate_xml_structure<R: std::io::BufRead>(reader: R) -> Result<(), String> {
    let mut xml = quick_xml::Reader::from_reader(reader);
    let mut buf = Vec::with_capacity(8 * 1024);
    let mut depth: usize = 0;
    let mut saw_root = false;
    use quick_xml::events::Event;
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(_)) => {
                if saw_root && depth == 0 {
                    return Err("multiple root elements".into());
                }
                saw_root = true;
                depth += 1;
                if depth > LIMIT_XML_DEPTH {
                    return Err(format!("element depth over limit {LIMIT_XML_DEPTH}"));
                }
            }
            Ok(Event::Empty(_)) => {
                if saw_root && depth == 0 {
                    return Err("multiple root elements".into());
                }
                saw_root = true;
            }
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return Err("unbalanced end tag".into());
                }
                depth -= 1;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
        buf.clear();
    }
    if !saw_root {
        return Err("no root element".into());
    }
    if depth > 0 {
        return Err(format!("truncated: {depth} unclosed element(s) at EOF"));
    }
    Ok(())
}

/// 门 2+3 融合单趟：逐条目有界读取计数（实际 > 声明即拒——谎言头
/// 闭环）+ .xml/.rels 部件的良构性流式校验（`validate_parts` 开关）。
///
/// 成本结构（与 xlsx-r1 相比零额外解压）：门 2 本就读全量一次，
/// 门 3 校验在同一趟的字节流上过 quick-xml（结构级拉取解析）。
fn verify_pass<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
    validate_parts: bool,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<(), ReadError> {
    let n = za.len();
    for i in 0..n {
        let mut zf = za
            .by_index(i)
            .map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
        let name = zf.name().to_string();
        let declared = zf.size();
        let is_xml = validate_parts && (name.ends_with(".xml") || name.ends_with(".rels"));
        let mut counting = CountingReader::new(&mut zf, declared, is_xml);
        if is_xml {
            let mut br = std::io::BufReader::new(&mut counting);
            let parse = validate_xml_structure(&mut br);
            // 解析中止（错误）时未读尽：补完谎言检查（有界——lied
            // 即停；成功路径 BufReader 已到 EOF 为 no-op）。
            let drain = drain_bounded(&mut br);
            if let Err(e) = drain {
                return Err(ReadError::ContainerCorrupt(format!("{e}")));
            }
            if counting.lied {
                return Err(ReadError::PrescanRejected {
                    reason: "actual size exceeds declared (lie header)",
                    entry: Some(name),
                });
            }
            match parse {
                Ok(()) => {}
                Err(detail) => {
                    if strict {
                        return Err(ReadError::InvalidPart {
                            part: name.clone(),
                            detail,
                        });
                    }
                    warnings.push(CompatibilityWarning {
                        part: name.clone(),
                        kind: WarningKind::MalformedXml,
                        detail: format!("compat mode: malformed XML tolerated: {detail}"),
                    });
                }
            }
        } else {
            // 纯尺寸路径（门 2 原实现语义：即时检出即拒）。
            let mut sink = vec![0u8; 64 * 1024];
            loop {
                use std::io::Read as _;
                match counting.read(&mut sink) {
                    Ok(0) => break,
                    Ok(_) => {
                        if counting.lied {
                            return Err(ReadError::PrescanRejected {
                                reason: "actual size exceeds declared (lie header)",
                                entry: Some(name),
                            });
                        }
                    }
                    Err(e) => {
                        return Err(ReadError::ContainerCorrupt(format!("{e}")));
                    }
                }
            }
        }
        if let Some(off) = counting.ctrl_at {
            warnings.push(CompatibilityWarning {
                part: name.clone(),
                kind: WarningKind::ControlChars,
                detail: format!(
                    "XML 1.0 illegal control byte at offset {off} (value-face tolerance unchanged; sanitize on re-emission)"
                ),
            });
        }
    }
    Ok(())
}

/// 有界排空（谎言检出即停——实际量受声明值与门 1 总量上限约束）。
fn drain_bounded<R: std::io::Read>(r: &mut R) -> std::io::Result<()> {
    let mut sink = vec![0u8; 64 * 1024];
    loop {
        match r.read(&mut sink) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }
}

/// 实际尺寸诚实性验证（谎言头闭环，策略层实现——门 2 独立面，
/// 历史公开 API 保持：仅尺寸计数，不含门 3 校验）。
///
/// 依据（xlsx-r1 实测定谳 2026-10-13）：zip 8.6 读界**无**声明值强制
/// （谎言头条目 read_to_end 全量读回零错误，仅 CRC 校验可被正确
/// CRC 绕过）；zip 9 的内建拒绝（T3 3b）为 9.x 独有。calamine
/// 0.36.1 内部依赖 zip 8.6 → 实际读路径无此防线，由本层补。
///
/// 成本注记：合法档全量解压一次（总量已由 prescan 锁 ≤300MB）；
/// 随后 calamine 重读。双读为 v1 已知成本；受信源跳过开关经
/// 裁决**不设**（无基准证据不得加跳过——xlsx-r2 用户指令）。
pub fn verify_actual_sizes<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
) -> Result<(), ReadError> {
    let mut sink = Vec::new();
    verify_pass(za, false, true, &mut sink).map(|_| ())
}

/// 门 2+3 融合趟的 crate 内复用面（xlsx-r2 Phase 3）：DOCX 读路径
/// 与 XLSX 同为 zip 容器 + OPC 部件约定，复用**同一门实现**——
/// 容器级基础设施共享（SECURITY_POLICY v1.1 §2.1），非领域抽象
/// （DOCX 模块边界见 docx.rs 模块文档）。外部消费方经
/// [`read_book`]/[`crate::docx`] 管线间接使用。
pub(crate) fn verify_gates<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<(), ReadError> {
    verify_pass(za, true, strict, warnings)
}

// ---------------------------------------------------------------------------
// 类型化单元格值模型（r4-t1：原始值 / 推断类型 / 显式转换三分）
// ---------------------------------------------------------------------------

/// 读面类型化单元格值（r4-t1——office-io 自有模型，calamine 类型不出
/// 此面；与写侧 [`crate::write::CellValue`] 对偶）。
///
/// 三分纪律：**原始值**（本枚举载荷 = 存储面事实）+ **推断类型**
/// （变体即 kind，日期类携带依据）+ **显式转换**（[`CellData::
/// as_iso_datetime`] / [`CellData::as_iso_duration`]——消费方明确
/// 调用，映射层不做静默语义转换）。显示面（t17 Display 语义）经
/// [`CellData::display`]。
///
/// ## kind 判定依据表（一手核验 calamine 0.36.1 cells_reader.rs
/// `read_v` + formats.rs；**本层零新增推断**——kind = 存储类型 +
/// numFmt 证据的忠实转译；实测钉定 = tests/cell_types.rs）
///
/// | 变体 | 依据 |
/// |------|------|
/// | [`Text`] | 存储类型 = 字符串（t="s" 共享串 / t="str" 公式文本结果 / inlineStr / 无 t 非数字回退）——标识符/前导零/数字文本**原样保留，绝不转数字**（用户裁决默认值） |
/// | [`Number`] | 存储类型 = 数字（t="n" / 无 t 可解析 f64）——xlsx 数值面恒 IEEE 754 double（`Data::Int` 仅 pivot 路径不可达）；**>2^53 整数精度边界**（9007199254740993 落盘即 9007199254740992——Excel 自身同限） |
/// | [`Boolean`] | t="b" 存储 |
/// | [`DateTime`] | **numFmt 日期证据**：styles.xml cellXfs numFmtId = builtin 14-22/45/47 或自定义格式码含 d/m/h/y/s 计 token（formats.rs 判定，非「Float+猜测」）；serial = Excel 序列数（天数自 1899-12-30），is_1904 = workbookPr date1904 纪元。**纯 Float 无证据 → [`Number`]，不认定日期** |
/// | [`DateTimeIso`] | t="d" 显式 ISO 8601 存储（文件生产者定型——第二类明确依据） |
/// | [`Duration`] | numFmt TimeDelta 证据（builtin 46 `[h]:mm:ss` 或自定义 `[…]` 时长格式码）；serial = 天分数 |
/// | [`DurationIso`] | ODS 路径变体——**xlsx 读路径不可达**（t="d" 恒产 DateTimeIso；防御性承载保 kind 完整性） |
/// | [`Error`] | t="e" 存储——错误码文本保留（#DIV/0! / #N/A / #NAME? / #NULL! / #NUM! / #REF! / #VALUE! / #DATA!），可诊断不丢弃不 panic |
/// | [`Empty`] | 无值格（缺格 / 空 `<v>`） |
///
/// [`Text`]: CellData::Text
/// [`Number`]: CellData::Number
/// [`Boolean`]: CellData::Boolean
/// [`DateTime`]: CellData::DateTime
/// [`DateTimeIso`]: CellData::DateTimeIso
/// [`Duration`]: CellData::Duration
/// [`DurationIso`]: CellData::DurationIso
/// [`Error`]: CellData::Error
/// [`Empty`]: CellData::Empty
#[derive(Debug, Clone, PartialEq)]
pub enum CellData {
    /// 文本（含前导零/数字文本/标识符——原样保留）。
    Text(String),
    /// 数值（xlsx 存储面 f64；display = Rust 格式化——t17 显示面同源）。
    Number(f64),
    /// 布尔。
    Boolean(bool),
    /// 日期/日期时间：序列数 + numFmt 证据 + 纪元（见依据表）。
    DateTime { serial: f64, is_1904: bool },
    /// ISO 8601 日期/日期时间串（t="d" 存储，原串）。
    DateTimeIso(String),
    /// 时长：序列数（天分数）+ TimeDelta numFmt 证据。
    Duration { serial: f64, is_1904: bool },
    /// ISO 8601 时长串（xlsx 不可达——防御性承载）。
    DurationIso(String),
    /// 错误格（错误码文本保留——可诊断）。
    Error(String),
    /// 空格。
    Empty,
}

/// [`CellData::to_write_parts`] 的 DateTime kind 保持 numFmt 格式码
/// （写回后读面经 numFmt 日期证据再判 DateTime——kind 往返钉定见
/// tests/cell_types.rs `kind_preserving_replay_roundtrip`）。
pub const DATE_REPLAY_FORMAT: &str = "yyyy-mm-dd hh:mm:ss";
/// Duration kind 保持 numFmt 格式码（TimeDelta 证据往返）。
pub const DURATION_REPLAY_FORMAT: &str = "[h]:mm:ss";

/// 1904 → 1900 纪元换算（写面引擎恒 1900 纪元——rust_xlsxwriter 包装
/// 无 1904 开关；1462 = 两纪元天数差，与 calamine
/// EXCEL_1900_1904_DIFF 同值）：历法日期保持，序列数换算。
fn epoch_to_1900(serial: f64, is_1904: bool) -> f64 {
    if is_1904 {
        serial + 1462.0
    } else {
        serial
    }
}

impl CellData {
    /// 显示面（t17 Display 语义同源——与 calamine `Data::to_string`
    /// 逐字等值，tests/cell_types.rs 全格等值钉定）：文本原样 / 数值
    /// Rust f64 格式化 / 布尔 / 日期与时长 = 裸序列数
    /// （`ExcelDateTime` Display = value） / ISO 串原样 / 错误码 /
    /// 空 = `""`。
    pub fn display(&self) -> String {
        match self {
            CellData::Text(s) => s.clone(),
            CellData::Number(v) => format!("{v}"),
            CellData::Boolean(b) => format!("{b}"),
            CellData::DateTime { serial, .. } | CellData::Duration { serial, .. } => {
                format!("{serial}")
            }
            CellData::DateTimeIso(s) | CellData::DurationIso(s) => s.clone(),
            CellData::Error(code) => code.clone(),
            CellData::Empty => String::new(),
        }
    }

    /// **显式转换**：日期/日期时间 → ISO 8601 字符串
    /// （`YYYY-MM-DDTHH:MM:SS[.mmm]`；毫秒非零才输出小数位）。
    ///
    /// - [`CellData::DateTime`]：序列数 → 历法组件（calamine
    ///   `ExcelDateTime::to_ymd_hms_milli`——1900/1904 双纪元与 Excel
    ///   1900 闰年怪癖由引擎实现承载）；
    /// - [`CellData::DateTimeIso`]：原串直返（已是 ISO）；
    /// - 其余变体 → `None`（**无据转换不发生**——纯 Number 序列数
    ///   不具备日期依据，裁决「日期转换必须有明确依据」）。
    pub fn as_iso_datetime(&self) -> Option<String> {
        match self {
            CellData::DateTime { serial, is_1904 } => {
                let (y, mo, d, h, mi, s, ms) =
                    ExcelDateTime::new(*serial, ExcelDateTimeType::DateTime, *is_1904)
                        .to_ymd_hms_milli();
                if ms == 0 {
                    Some(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}"))
                } else {
                    Some(format!(
                        "{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}"
                    ))
                }
            }
            CellData::DateTimeIso(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// **显式转换**：时长 → ISO 8601 时长串（`PT…H…M…S`；序列数按
    /// 天分数 × 86400 秒换算，秒级舍入）。[`CellData::Duration`] 与
    /// [`CellData::DurationIso`]（原串直返）之外 → `None`（无据不转换）。
    pub fn as_iso_duration(&self) -> Option<String> {
        match self {
            CellData::Duration { serial, .. } => {
                let total_secs = (serial * 86_400.0).round() as i64;
                let (h, m, s) = (total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60);
                Some(format!("PT{h}H{m}M{s}S"))
            }
            CellData::DurationIso(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// **kind 分派 → 写面值 + 配套样式**（值面联接——读-改-重建路径
    /// 的类型保持基础；完整单格联接（含公式通道）见 [`SheetData::
    /// replay_cell`]）。返回 `None` = 不落格（[`CellData::Empty`]）。
    ///
    /// | kind | 写面落点 | kind 保持 |
    /// |------|----------|-----------|
    /// | Text | `Text` 原样 | ✓（前导零零损） |
    /// | Number | `Number(f64)` | ✓（>2^53 精度边界随 Excel 存储面恒在） |
    /// | Boolean | `Boolean` | ✓ |
    /// | DateTime | `Number(serial)` + [`DATE_REPLAY_FORMAT`] 样式（1904 纪元换算 +1462，见 `epoch_to_1900`） | ✓（读面经 numFmt 证据再判 DateTime） |
    /// | Duration | `Number(serial)` + [`DURATION_REPLAY_FORMAT`] 样式 | ✓（TimeDelta 证据往返） |
    /// | DateTimeIso / DurationIso | `Text` 原串 | **降级登记**（写面引擎无 t="d" 面——值保持，kind 降为文本） |
    /// | Error | `Text(码文本)` | **降级登记**（写面引擎无错误格——码文本可诊断保留） |
    /// | Empty | 不落格 | — |
    pub fn to_write_parts(
        &self,
    ) -> Option<(crate::write::CellValue, Option<crate::write::CellStyle>)> {
        use crate::write::{CellStyle, CellValue as WriteValue, NumFmt};
        let date_style = || {
            Some(CellStyle {
                num_fmt: Some(NumFmt::Custom(DATE_REPLAY_FORMAT.to_string())),
                ..Default::default()
            })
        };
        let duration_style = || {
            Some(CellStyle {
                num_fmt: Some(NumFmt::Custom(DURATION_REPLAY_FORMAT.to_string())),
                ..Default::default()
            })
        };
        match self {
            CellData::Text(s) => Some((WriteValue::Text(s.clone()), None)),
            CellData::Number(v) => Some((WriteValue::Number(*v), None)),
            CellData::Boolean(b) => Some((WriteValue::Boolean(*b), None)),
            CellData::DateTime { serial, is_1904 } => Some((
                WriteValue::Number(epoch_to_1900(*serial, *is_1904)),
                date_style(),
            )),
            CellData::Duration { serial, .. } => {
                Some((WriteValue::Number(*serial), duration_style()))
            }
            CellData::DateTimeIso(s) | CellData::DurationIso(s) => {
                Some((WriteValue::Text(s.clone()), None))
            }
            CellData::Error(code) => Some((WriteValue::Text(code.clone()), None)),
            CellData::Empty => None,
        }
    }
}

/// calamine `Data` → [`CellData`]（crate 内单点转译——kind 依据表的
/// 实现面；is_1904 来自 `Xlsx::has_1904_epoch()`[构造期 eager 解析]）。
fn cell_data_from(d: &Data, is_1904: bool) -> CellData {
    match d {
        Data::Empty => CellData::Empty,
        Data::String(s) => CellData::Text(s.clone()),
        Data::Float(v) => CellData::Number(*v),
        // xlsx 读路径不可达（worksheet 数值恒 Float；Int 仅 pivot 缓存
        // 记录路径）——防御性承载：i64 直落 f64（精度边界随存储面）。
        Data::Int(v) => CellData::Number(*v as f64),
        Data::Bool(b) => CellData::Boolean(*b),
        Data::DateTime(dt) => {
            if dt.is_duration() {
                CellData::Duration {
                    serial: dt.as_f64(),
                    is_1904,
                }
            } else {
                CellData::DateTime {
                    serial: dt.as_f64(),
                    is_1904,
                }
            }
        }
        Data::DateTimeIso(s) => CellData::DateTimeIso(s.clone()),
        Data::DurationIso(s) => CellData::DurationIso(s.clone()),
        Data::Error(e) => CellData::Error(e.to_string()),
    }
}

/// 类型化值矩阵（r4-t1）——与 [`SheetData::values`] **同一使用区包围盒、
/// 同一相对坐标系**（单点派生自 values，形状恒一致；空表同 values
/// 高宽 0）。坐标系双面暴露：[`CellGrid::get`] 相对 start 锚（与
/// `Range::get` 同语义——in-range 缺格 = [`CellData::Empty`]）；
/// [`CellGrid::get_abs`] 绝对坐标（跨面配对建议恒绝对坐标——
/// range_anchor_pinned.rs 锚定语义的免坑面）。
#[derive(Debug, Default, Clone)]
pub struct CellGrid {
    start: (u32, u32),
    end: (u32, u32),
    inner: Vec<CellData>,
}

impl CellGrid {
    /// values Range → 类型化网格（crate 内单点派生）。
    fn from_values(values: &Range<Data>, is_1904: bool) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        let start = values.start().expect("non-empty range has start");
        let end = values.end().expect("non-empty range has end");
        let height = values.height();
        let width = values.width();
        let mut inner = Vec::with_capacity(height * width);
        for r in 0..height {
            for c in 0..width {
                let cell = values
                    .get((r, c))
                    .map(|d| cell_data_from(d, is_1904))
                    .unwrap_or(CellData::Empty);
                inner.push(cell);
            }
        }
        Self { start, end, inner }
    }

    /// 使用区左上锚（绝对坐标；空表 `None`——与 `Range::start` 同一
    /// 语义）。
    pub fn start(&self) -> Option<(u32, u32)> {
        if self.inner.is_empty() {
            None
        } else {
            Some(self.start)
        }
    }

    /// 使用区右下角（绝对坐标；空表 `None`）。
    pub fn end(&self) -> Option<(u32, u32)> {
        if self.inner.is_empty() {
            None
        } else {
            Some(self.end)
        }
    }

    /// 行数（空表 0）。
    pub fn height(&self) -> usize {
        if self.inner.is_empty() {
            0
        } else {
            (self.end.0 - self.start.0 + 1) as usize
        }
    }

    /// 列数（空表 0）。
    pub fn width(&self) -> usize {
        if self.inner.is_empty() {
            0
        } else {
            (self.end.1 - self.start.1 + 1) as usize
        }
    }

    /// 相对坐标（相对 start 锚）——in-range 恒 `Some`（缺格 =
    /// [`CellData::Empty`]），越界 `None`（与 `Range::get` 同语义）。
    pub fn get(&self, pos: (usize, usize)) -> Option<&CellData> {
        let (row, col) = pos;
        let (height, width) = (self.height(), self.width());
        if col >= width || row >= height {
            None
        } else {
            self.inner.get(row * width + col)
        }
    }

    /// 绝对坐标（工作表 0 基行列）。
    pub fn get_abs(&self, row: u32, col: u32) -> Option<&CellData> {
        let (start, end) = (self.start, self.end);
        if self.inner.is_empty() || row < start.0 || col < start.1 || row > end.0 || col > end.1 {
            return None;
        }
        self.get(((row - start.0) as usize, (col - start.1) as usize))
    }
}

// ---------------------------------------------------------------------------
// calamine 读取包装（五 API 面）
// ---------------------------------------------------------------------------

/// 单表快照：值矩阵（含缓存值）/ 公式文本 / 合并范围 / 超链接（v2）/
/// 类型化值矩阵（r4-t1）。
#[derive(Debug, Default)]
pub struct SheetData {
    pub name: String,
    pub values: Range<Data>,
    pub formulas: Range<String>,
    pub merges: Vec<Dimensions>,
    /// 超链接（v2）：calamine Hyperlink 四字段，已过 IO 边界实体补偿。
    pub hyperlinks: Vec<Hyperlink>,
    /// 类型化值矩阵（r4-t1）——`values` 的类型化转译（同一包围盒/
    /// 坐标系，单点派生）；kind 依据与显式转换见 [`CellData`]。
    pub typed: CellGrid,
}

impl SheetData {
    /// 公式文本（**独立通道**——与值面缓存值明确区分，r4-t1 安全联接
    /// 面：绝对坐标入参，公式范围锚定换算在本方法内完成，消费方免于
    /// range_anchor_pinned.rs 锚定语义的配对陷阱）。`None` = 该格无
    /// 公式文本（非公式格 / 坐标越界 / 空公式面）。
    ///
    /// 公式文本形态：**不带 `=` 前缀**（calamine 读侧行为，FIDELITY
    /// §1 在档；写面 [`crate::write::CellValue::Formula`] 接受带/不带
    /// 前缀——往返一致性由写面规范化承载）。
    pub fn formula_at(&self, row: u32, col: u32) -> Option<&str> {
        let start = self.formulas.start()?;
        let r = usize::try_from(row.checked_sub(start.0)?).ok()?;
        let c = usize::try_from(col.checked_sub(start.1)?).ok()?;
        match self.formulas.get((r, c)) {
            Some(f) if !f.is_empty() => Some(f),
            _ => None,
        }
    }

    /// **读-改-重建单格类型保持联接**（r4-t1——kind 分派 + 公式两通道
    /// 联接的单一安全调用点）。绝对坐标入参。
    ///
    /// - 公式格（[`Self::formula_at`] 命中）：值 = **缓存计算结果**，
    ///   公式文本独立通道——写面落 `Formula { text, cached }`（缓存
    ///   值 ≠ 公式文本，两通道区分）；缓存 kind 映射：Number/Boolean/
    ///   Text 直传，DateTime/Duration → 序列数缓存 + 日期 numFmt 样式
    ///   （纪元换算），DateTimeIso/DurationIso → 文本缓存（值保持），
    ///   Empty（无缓存）→ `None`（读回 Float(0.0) 两义性在档——
    ///   FIDELITY §1），Error → 码文本缓存（kind 降级登记）。
    /// - 非公式格：[`CellData::to_write_parts`] kind 分派。
    /// - `None` = 不落格（越界 / [`CellData::Empty`] 且非公式格）。
    pub fn replay_cell(
        &self,
        row: u32,
        col: u32,
    ) -> Option<(crate::write::CellValue, Option<crate::write::CellStyle>)> {
        let value = self.typed.get_abs(row, col)?;
        if let Some(text) = self.formula_at(row, col) {
            use crate::write::FormulaCache;
            let (cached, style) = match value {
                CellData::Number(v) => (Some(FormulaCache::Number(*v)), None),
                CellData::Boolean(b) => (Some(FormulaCache::Boolean(*b)), None),
                CellData::Text(t) => (Some(FormulaCache::Text(t.clone())), None),
                CellData::DateTime { serial, is_1904 } => (
                    Some(FormulaCache::Number(epoch_to_1900(*serial, *is_1904))),
                    Some(crate::write::CellStyle {
                        num_fmt: Some(crate::write::NumFmt::Custom(DATE_REPLAY_FORMAT.to_string())),
                        ..Default::default()
                    }),
                ),
                CellData::Duration { serial, .. } => (
                    Some(FormulaCache::Number(*serial)),
                    Some(crate::write::CellStyle {
                        num_fmt: Some(crate::write::NumFmt::Custom(
                            DURATION_REPLAY_FORMAT.to_string(),
                        )),
                        ..Default::default()
                    }),
                ),
                // ISO 串缓存：文本形态值保持（写面无 t="d" 缓存型）。
                CellData::DateTimeIso(s) | CellData::DurationIso(s) => {
                    (Some(FormulaCache::Text(s.clone())), None)
                }
                // 无缓存（读回 Float(0.0) 两义性在档）/ 错误码文本缓存
                //（写面缓存无错误型——kind 降级登记）。
                CellData::Empty => (None, None),
                CellData::Error(code) => (Some(FormulaCache::Text(code.clone())), None),
            };
            return Some((
                crate::write::CellValue::Formula {
                    text: text.to_string(),
                    cached,
                },
                style,
            ));
        }
        value.to_write_parts()
    }
}

/// 整档提取：表序 + 逐表四面（v2：+超链接；C1 读取能力产品面）
/// 以及结构化警告（Phase 2：兼容模式下降级的畸形部件与控制字符
/// 发现；严格模式下畸形部件不降级而是直接 typed reject，故
/// warnings 至多含 ControlChars 项）。
#[derive(Debug)]
pub struct Book {
    pub sheets: Vec<SheetData>,
    pub warnings: Vec<CompatibilityWarning>,
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

/// 读入口（严格模式默认）：预扫门 → 尺寸/部件融合验证 → calamine
/// 全量提取（五面）。
///
/// 门在前（敌意输入零解析层接触）：①prescan 声明值四检查；
/// ②尺寸诚实性 + 部件良构性融合趟（门 2+3，单次解压——Phase 2
/// 起畸形 XML 部件 typed reject，mal7e 类静默空/部分数据闭环）。
/// 门过后错误归 ParseError。
pub fn read_book(path: &str) -> Result<Book, ReadError> {
    read_book_with_options(path, &ReadOptions::default())
}

/// 读入口（带选项——Phase 2）：[`ReadOptions::compat`] = true 时
/// 畸形部件降级为结构化警告（数据照常提取，门 1/门 2/安全上限
/// 恒不放松）；默认严格行为同 [`read_book`]。
pub fn read_book_with_options(path: &str, opts: &ReadOptions) -> Result<Book, ReadError> {
    let f =
        std::fs::File::open(path).map_err(|e| ReadError::ContainerCorrupt(format!("open: {e}")))?;
    let mut za =
        zip::ZipArchive::new(f).map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
    prescan(&mut za)?; // 门 1：声明值四检查
    let mut warnings = Vec::new();
    // 门 2+3 融合趟：尺寸诚实性 + 部件良构性（单次解压）。
    verify_pass(&mut za, true, !opts.compat, &mut warnings)?;
    drop(za);

    let mut book: Xlsx<_> =
        calamine::open_workbook(path).map_err(|e| ReadError::ParseError(format!("open: {e}")))?;
    extract_sheets(&mut book, warnings)
}

/// 读入口（字节面——t17 增量，wasm/内存消费方；门结构与 [`read_book`]
/// 恒等）。动机与 docx.rs `read_docx_from_bytes`（xlsx-r2 Phase 3）
/// 同款：wasm32-unknown-unknown 无文件系统，非路径消费方（wasm 桥）
/// 需要字节入口过同一套门。calamine 侧 `Xlsx::new(Cursor)` 直读
/// （`open_workbook` 为文件便利面）。
pub fn read_book_from_bytes(bytes: &[u8]) -> Result<Book, ReadError> {
    read_book_from_bytes_with_options(bytes, &ReadOptions::default())
}

/// 读入口（字节面 + 选项——t17）：[`ReadOptions::compat`] 语义与
/// [`read_book_with_options`] 恒等（门 1/门 2/安全上限两模式恒不放松）。
pub fn read_book_from_bytes_with_options(
    bytes: &[u8],
    opts: &ReadOptions,
) -> Result<Book, ReadError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut za =
        zip::ZipArchive::new(cursor).map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
    prescan(&mut za)?; // 门 1：声明值四检查
    let mut warnings = Vec::new();
    // 门 2+3 融合趟：尺寸诚实性 + 部件良构性（单次解压）。
    verify_pass(&mut za, true, !opts.compat, &mut warnings)?;
    drop(za);

    let mut book: Xlsx<_> = Xlsx::new(std::io::Cursor::new(bytes))
        .map_err(|e| ReadError::ParseError(format!("open: {e}")))?;
    extract_sheets(&mut book, warnings)
}

/// calamine 五面提取共享核（路径/字节两入口同一实现——单一行为
/// 真源，t17 抽出；提取逻辑与 xlsx-r2 Phase 2 起的 [`read_book`]
/// 逐行相同）。r4-t1：逐表附建类型化值矩阵（`typed`——values 的
/// 单点派生；纪元来自 `has_1904_epoch`，workbookPr 构造期已解析）。
fn extract_sheets<R: std::io::Read + std::io::Seek>(
    book: &mut Xlsx<R>,
    warnings: Vec<CompatibilityWarning>,
) -> Result<Book, ReadError> {
    let is_1904 = book.has_1904_epoch();
    let names = book.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    for name in &names {
        let values = book
            .worksheet_range(name)
            .map_err(|e| ReadError::ParseError(format!("range[{name}]: {e}")))?;
        let typed = CellGrid::from_values(&values, is_1904);
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
            typed,
        });
    }
    Ok(Book { sheets, warnings })
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
        // t10-d mal7c 实证钉定：0x00 在文本位被读侧容忍，值含 NUL
        // ——值面行为 Phase 2 不变；新增可见性：结构化警告恒登记
        //（两模式同）。
        let p = tmp("nul");
        let entries = minimal_xlsx("a\0b");
        write_zip(&p, &entries);
        let book = read_book(&p).unwrap();
        match book.sheets[0].values.get((0, 0)) {
            Some(Data::String(s)) => assert!(s.contains('\u{0}'), "值应含 NUL：{s:?}"),
            other => panic!("expected string, got {other:?}"),
        }
        // 警告面：ControlChars 在位（严格模式不拒——值面容忍）。
        assert!(
            book.warnings
                .iter()
                .any(|w| w.kind == WarningKind::ControlChars),
            "控制字符发现应登记为警告：{:?}",
            book.warnings
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn garbage_part_strict_reject_compat_warns() {
        // t10-d mal7e 实证钉定（Phase 2 行为变化）：垃圾部件不再
        // 静默降级空表——默认严格 typed reject；兼容模式降级为结构化
        // 警告 + 数据照常提取（calamine 容忍行为不变）。
        let p = tmp("garbage");
        let mut entries = minimal_xlsx("unused");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                *d = (0..256u16).map(|i| i as u8).cycle().take(1024).collect();
            }
        }
        write_zip(&p, &entries);
        // 严格（默认）：typed reject，部件名在错误内。
        match read_book(&p) {
            Err(ReadError::InvalidPart { part, .. }) => {
                assert_eq!(part, "xl/worksheets/sheet1.xml");
            }
            other => panic!("expected InvalidPart, got {other:?}"),
        }
        // 兼容：Ok + MalformedXml 警告 + 空表（calamine 行为不变）。
        let book = read_book_with_options(&p, &ReadOptions { compat: true }).unwrap();
        assert_eq!(book.sheets.len(), 1);
        assert_eq!(book.non_empty_cells(), 0);
        assert!(
            book.warnings.iter().any(
                |w| w.kind == WarningKind::MalformedXml && w.part == "xl/worksheets/sheet1.xml"
            ),
            "兼容模式应降级为警告：{:?}",
            book.warnings
        );
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
        // t10-d 补充实证（lib 测试发现，2026-10-13）+ Phase 2 行为
        // 变化（2026-10-14）：截断容忍度分界在门 3 收敛——两种截断
        // 形态在严格模式下均 typed reject（缺根闭合 = EOF 未闭合元素；
        // 标签内截断 = quick-xml 语法错）；兼容模式下维持 xlsx-r1 实测
        // 分界：缺根闭合 → 警告 + calamine 容忍读回；标签内截断 →
        // 警告后 calamine 自身报 ParseError（兼容只放宽本层验证，
        // 不改变引擎行为）。
        let p = tmp("trunc_ok");
        let mut entries = minimal_xlsx("v");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                d.truncate(d.len() - 12); // 恰移除 </worksheet>
            }
        }
        write_zip(&p, &entries);
        match read_book(&p) {
            Err(ReadError::InvalidPart { part, detail }) => {
                assert_eq!(part, "xl/worksheets/sheet1.xml");
                assert!(detail.contains("unclosed"), "detail: {detail}");
            }
            other => panic!("expected strict InvalidPart, got {other:?}"),
        }
        // 兼容：警告 + calamine 容忍读回（xlsx-r1 实测行为保持）。
        let book = read_book_with_options(&p, &ReadOptions { compat: true }).unwrap();
        assert_eq!(book.non_empty_cells(), 1);
        assert!(book
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::MalformedXml));
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
            Err(ReadError::InvalidPart { .. }) => {} // 严格：语法错拒拒
            other => panic!("expected strict InvalidPart, got {other:?}"),
        }
        // 兼容：警告降级后 calamine 自身 ParseError（不改变引擎行为）。
        match read_book_with_options(&p2, &ReadOptions { compat: true }) {
            Err(ReadError::ParseError(_)) => {}
            other => panic!("expected compat ParseError, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn gate3_multiple_roots_rejected() {
        // 门 3：多根元素 = 畸形 XML（严格拒；兼容警告）。
        let p = tmp("multiroot");
        let mut entries = minimal_xlsx("v");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                let malformed = br#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet><extra/>"#;
                *d = malformed.to_vec();
            }
        }
        write_zip(&p, &entries);
        match read_book(&p) {
            Err(ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("multiple root"), "detail: {detail}");
            }
            other => panic!("expected InvalidPart, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn gate3_empty_xml_part_rejected() {
        // 门 3：零字节 XML 部件 = 无根（严格拒）。
        let p = tmp("emptyxml");
        let mut entries = minimal_xlsx("v");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                d.clear();
            }
        }
        write_zip(&p, &entries);
        match read_book(&p) {
            Err(ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("no root"), "detail: {detail}");
            }
            other => panic!("expected InvalidPart, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn gate3_depth_limit_enforced() {
        // 深度 = LIMIT_XML_DEPTH + 1 的部件（root 1 层 + n×1000）。
        let p = tmp("deepbomb");
        let mut entries = minimal_xlsx("v");
        let depth = LIMIT_XML_DEPTH;
        let mut xml = String::from("<?xml version=\"1.0\"?><root>");
        for _ in 0..depth {
            xml.push_str("<n>");
        }
        for _ in 0..depth {
            xml.push_str("</n>");
        }
        xml.push_str("</root>");
        let xml_bytes = xml.into_bytes();
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                *d = xml_bytes.clone();
            }
        }
        write_zip(&p, &entries);
        match read_book(&p) {
            Err(ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("depth over limit"), "detail: {detail}");
            }
            other => panic!("expected InvalidPart, got {other:?}"),
        }
        // 深度恰好 = 上限的部件合法（边界对测：root 1 层 + n×999 =
        // LIMIT_XML_DEPTH 整）。
        let p2 = tmp("deepok");
        let mut entries2 = minimal_xlsx("v");
        let depth2 = LIMIT_XML_DEPTH - 1;
        let mut xml2 = String::from("<?xml version=\"1.0\"?><root>");
        for _ in 0..depth2 {
            xml2.push_str("<n>");
        }
        for _ in 0..depth2 {
            xml2.push_str("</n>");
        }
        xml2.push_str("</root>");
        let xml2_bytes = xml2.into_bytes();
        for (n, d) in entries2.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                // 嵌套合法但非 worksheet 结构 → calamine 提取空表
                //（门 3 过 + 引擎容忍——不属畸形面）。
                *d = xml2_bytes.clone();
            }
        }
        write_zip(&p2, &entries2);
        match read_book(&p2) {
            Ok(_) => {}
            other => panic!("depth == limit 应过门, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn gate3_non_xml_parts_not_validated() {
        // 门 3 校验域 = .xml/.rels 后缀部件；非 XML 后缀的二进制部件
        //（如内嵌图片）不校验（OPC 内容类型分界——图片本就是二进制）。
        let p = tmp("binpart");
        let mut entries = minimal_xlsx("v");
        entries.push((
            "xl/media/image1.png".into(),
            (0..256u16).map(|i| i as u8).collect(),
        ));
        write_zip(&p, &entries);
        let book = read_book(&p).unwrap(); // 二进制部件不拒
        assert_eq!(book.non_empty_cells(), 1);
        // 且其内容不产生控制字符警告（不在校验域）。
        assert!(
            !book
                .warnings
                .iter()
                .any(|w| w.part == "xl/media/image1.png"),
            "非 XML 部件不应入警告面：{:?}",
            book.warnings
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn verify_actual_sizes_standalone_still_works() {
        // 门 2 独立面（历史公开 API）：仅尺寸校验，不含门 3——
        // 畸形 XML 部件在独立面下不拒（门 3 属 read_book 管线）。
        let p = tmp("standalone");
        let mut entries = minimal_xlsx("unused");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                *d = (0..256u16).map(|i| i as u8).cycle().take(1024).collect();
            }
        }
        write_zip(&p, &entries);
        let f = std::fs::File::open(&p).unwrap();
        let mut za = zip::ZipArchive::new(f).unwrap();
        assert!(verify_actual_sizes(&mut za).is_ok()); // 尺寸诚实 → 过
        let _ = std::fs::remove_file(&p);
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

    /// t17：字节入口与路径入口行为等价（同一共享核 extract_sheets）
    /// ——同字节两入口读出同构 Book。
    #[test]
    fn bytes_entry_matches_path_entry() {
        let p = tmp("bytes_parity");
        let entries = minimal_xlsx("hello bytes");
        write_zip(&p, &entries);
        let bytes = std::fs::read(&p).unwrap();
        let by_path = read_book(&p).unwrap();
        let by_bytes = read_book_from_bytes(&bytes).unwrap();
        assert_eq!(by_path.sheet_names(), by_bytes.sheet_names());
        assert_eq!(by_path.non_empty_cells(), by_bytes.non_empty_cells());
        assert_eq!(by_bytes.sheet_names(), vec!["S1"]);
        assert_eq!(by_bytes.non_empty_cells(), 1);
        let _ = std::fs::remove_file(&p);
    }

    /// t17：字节入口敌意面——垃圾字节/截断档案 → ContainerCorrupt
    ///（门结构同路径入口：EOCD 不可读即拒）。
    #[test]
    fn bytes_entry_garbage_and_truncated_reject() {
        for garbage in [&b""[..], &[0u8, 1, 2, 99, 255], b"not a zip at all"] {
            match read_book_from_bytes(garbage) {
                Err(ReadError::ContainerCorrupt(_)) => {}
                other => panic!("expected ContainerCorrupt, got {other:?}"),
            }
        }
        let p = tmp("bytes_trunc");
        let entries = minimal_xlsx("truncate me");
        write_zip(&p, &entries);
        let bytes = std::fs::read(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        match read_book_from_bytes(&bytes[..bytes.len() / 2]) {
            Err(ReadError::ContainerCorrupt(_)) => {}
            other => panic!("expected ContainerCorrupt for truncated, got {other:?}"),
        }
    }
}
