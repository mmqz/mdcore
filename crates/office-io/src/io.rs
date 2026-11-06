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
//!
//! **r4-t2 增量（2026-10-17，Round 4 T2——导入/导出资源上限与安全
//! 边界）**：读面五类资源上限补面（分析仓 SECURITY_POLICY v1.2 §3
//! 全量校准——每项带执行点锚点 + 测试锚点）：
//! - **输入字节上限**（[`LIMIT_INPUT_BYTES`]；docx 族同构
//!   [`LIMIT_INPUT_BYTES_DOCX`]）：四入口（xlsx/docx × path/bytes）
//!   在**任何全量内存分配或解析之前**拒绝超限输入——DOCX 路径入口
//!   此前 `fs::read` 全量入内存无前置检查（真实内存面）；xlsx 路径
//!   惰性读无分配面，同限统一（前缀垃圾面拒绝——office 文档无 SFX
//!   合法形态）。
//! - **族分化声明限**（[`LIMIT_TOTAL_DECLARED_DOCX`] = 50MB）：此前
//!   DOCX 读路径误用 xlsx 族 300MB 常量（SECURITY_POLICY §3 docx
//!   族 50MB 行无执行点），本轮经 [`prescan_docx`] 对齐 office-b
//!   checkInputLimits 格式族分档（条目数/路径逃逸/谎言头两族共享）。
//! - **稠密包围盒格总数上限**（[`LIMIT_TOTAL_CELLS`]）：**稀疏包围盒
//!   炸弹实证闭项**——calamine 0.36.1 `Range::from_sparse` 按实际
//!   单元格**包围盒稠密化**（`vec![T::default(); cols×rows]`），
//!   1588 字节输入（A1 + XFD1048576 两格）实测触发 **549,755,813,888
//!   字节（~550GB）分配 → 进程 abort**（分析仓 spike sparse_bomb
//!   探针，2026-10-17）。修复 = 门 3 融合趟内**坐标流式扫描**
//!   （[`GridScan`]——镜像 calamine cells_reader 的位置语义：row/@r
//!   显式定位 + 行结束 +1 + c/@r 显式定位 + 列游标推进 + 载荷格 =
//!   有 v/is/f 子元素[值格与公式格的并集上界——styled 空格 calamine
//!   两面均不收，无假阳]）→ 单部件稠密面积检查先于 calamine；
//!   [`extract_sheets`] 再做全簿累计（多表名指向同部件的倍增面）。
//! - **工作表数量上限**（[`LIMIT_SHEET_COUNT`]）：空表名洪水 CPU 面
//!   （workbook.xml ≤300MB 可载 ~1200 万个 `<sheet>` 名，逐表重解析
//!   部件）在提取循环之前检查。
//! - 全部新限 **严格/兼容两模式恒等**（安全上限不随兼容放松），typed
//!   [`ReadError::PrescanRejected`]（安全拒绝类——与容器损坏[格式]
//!   /解析错误三分可区分）。

use calamine::{
    Data, Dimensions, ExcelDateTime, ExcelDateTimeType, Hyperlink, Range, Reader, Xlsx,
};

// ---------------------------------------------------------------------------
// §3 限额常量（SECURITY_POLICY 接线——r4-t2 起全量校准，v1.2）
// ---------------------------------------------------------------------------

/// zip 声明总量上限（xlsx 族 300MB；office-b default.json 一手参照）。
pub const LIMIT_TOTAL_DECLARED: u64 = 300 * 1024 * 1024;
/// 单条目声明尺寸上限（= 族总量上限，对称设计）。
pub const LIMIT_SINGLE_ENTRY: u64 = LIMIT_TOTAL_DECLARED;
/// zip 声明总量上限（docx 族 50MB；office-b checkInputLimits 格式族
/// 分档同构——r4-t2：此前 docx 路径误用 xlsx 族常量，本轮族分化
/// 对齐 SECURITY_POLICY §3）。
pub const LIMIT_TOTAL_DECLARED_DOCX: u64 = 50 * 1024 * 1024;
/// 输入字节上限的 zip 结构开销边距（r4-t2）：本地文件头 30B + 中央
/// 目录 46B + 名字/数据描述 ≈ 116B/条 × [`LIMIT_ENTRY_COUNT`] 上限
/// ≈ 1.16MB + EOCD 注释 ≤64KB → 2MB 覆盖合法满额档的结构开销。
pub const LIMIT_INPUT_MARGIN: u64 = 2 * 1024 * 1024;
/// 输入字节上限（xlsx 族，r4-t2）：= 族声明总量 + 结构边距——STORED
/// 条目（压缩后 = 声明值）的合法满额档 ≈ 声明总量 + 边距以内；超限
/// 即前缀垃圾/伪装载荷（office 文档无 SFX 合法形态）。四入口统一
/// （xlsx/docx × path/bytes），拒于任何全量读/解析之前。
pub const LIMIT_INPUT_BYTES: u64 = LIMIT_TOTAL_DECLARED + LIMIT_INPUT_MARGIN;
/// 输入字节上限（docx 族，r4-t2）：= docx 族声明总量 + 结构边距。
pub const LIMIT_INPUT_BYTES_DOCX: u64 = LIMIT_TOTAL_DECLARED_DOCX + LIMIT_INPUT_MARGIN;

/// OLE2 复合文档魔数（CFB——Word 97-2003 .doc / Excel .xls 旧格式的
/// 容器签名）。#234：异源 .doc 改名 .docx 后 zip 读入口误报「EOCD
/// 容器损坏」误导用户；魔数识别 → 专用错误 + 可操作指引。
pub const OLE2_CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// OLE2 复合文档魔数探测（前 8 字节；不足 8 字节 = false——短输入走
/// 既有 zip 容器错误路径，不劫持）。pub：docx 读/编两入口共用，
/// wasm 错误码面同步映射。
pub fn is_ole2(bytes: &[u8]) -> bool {
    bytes.len() >= OLE2_CFB_MAGIC.len() && bytes[..OLE2_CFB_MAGIC.len()] == OLE2_CFB_MAGIC
}
/// 条目数上限（v1 新设 10,000；mal6d 实测校准）。
pub const LIMIT_ENTRY_COUNT: usize = 10_000;
/// XML 部件元素嵌套深度上限（门 3，Phase 2）。真实 xlsx 部件深度
/// 个位到十位级（worksheet/共享字符串嵌套）；嵌套炸弹面在此截断
/// （quick-xml 拉取式无栈溢出风险，上限防下游消费面）。
pub const LIMIT_XML_DEPTH: usize = 1_000;
/// 工作表数量上限（读面语义层，r4-t2）：workbook.xml ≤300MB 可载
/// ~1200 万个 `<sheet>` 名（每名 ~25B）——逐表重解析部件的 CPU 面
/// （空表名洪水）；提取循环之前检查。真实工作簿百表级以内；wasm
/// 渲染层 16 表（mdcore-wasm MAX_SHEETS）为消费子集，两层关系
/// 登记（读面全提取 = C1 大档能力宣称面）。
pub const LIMIT_SHEET_COUNT: usize = 1_000;
/// 稠密包围盒格总数上限（读面语义层，r4-t2）：calamine
/// `Range::from_sparse` 按实际单元格包围盒**稠密化**（sparse_bomb
/// 探针：1588B 输入 → ~550GB 分配 abort）——本限界单部件稠密面积
/// （门 3 扫描）与全簿累计提取面积。值 = C1 宣称面（50 万格）的
/// 8× 边距；三网格（values/typed/formulas）× ~96B/格 ≈ 峰值预算
/// ~400MB 级（examples/bench_limits.rs 可复现基准在档，不设性能
/// 门禁阈值）。
pub const LIMIT_TOTAL_CELLS: u64 = 4_000_000;
/// 压缩比观察门限（#233 / zip-exotica 防线——**警告口径不拒**）。
/// 硬拒有实证误报面：研究仓 engine-probe s7b 合法 149:1 高压缩比
/// 件被引擎正常接受（6000 段全解析）——100:1 硬门会误杀合法件。
/// 威胁面上界已由既有门钉死：门 0 输入字节 + 门 1 声明总量（膨胀
/// 上限 = 声明总量）+ 门 2 谎言头 CountingReader（实读超 declared
/// 即拒）——本门限 = 纵深可见性（超限 → CompatibilityWarning，
/// 两模式恒登记，不静默也不阻断）。乘法判据：
/// `declared > LIMIT_COMPRESS_RATIO × max(compressed, 1)`（研究仓
/// R5 P-2 整数除法截断漏洞[100,999/1000 商 100 不>100 过门]的教训
/// 同源——必须乘法比较）。
pub const LIMIT_COMPRESS_RATIO: u64 = 100;

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
    /// 旧版 Office 二进制格式（OLE2 复合文档——Word 97-2003 .doc /
    /// Excel .xls，#234）：不是 zip 容器——改名 .docx/.xlsx 不改变
    /// 载荷形态。此前误报 EOCD 容器损坏，误导用户以为文件已损坏；
    /// 专用错误 + 可操作指引（Display 面）。
    LegacyOfficeFormat {
        /// 期望的入口格式（"docx" 等——错误文案上下文）。
        expect: &'static str,
    },
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
            ReadError::LegacyOfficeFormat { expect } => write!(
                f,
                "legacy office format: OLE2 复合文档（Word 97-2003 旧格式 .doc），不是 {expect} zip 容器——请用 Word「另存为 .docx」或 soffice --headless --convert-to docx 转换后再打开"
            ),
        }
    }
}

impl std::error::Error for ReadError {}

// ---------------------------------------------------------------------------
// 输入字节上限（四入口共享——r4-t2）
// ---------------------------------------------------------------------------

/// 输入字节检查（xlsx/docx × path/bytes 四入口单点：`len > limit` 即
/// 安全拒绝——拒于任何全量读/解析之前）。
fn check_input_size(len: u64, limit: u64) -> Result<(), ReadError> {
    if len > limit {
        return Err(ReadError::PrescanRejected {
            reason: "input size over limit",
            entry: None,
        });
    }
    Ok(())
}

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

/// 容器预扫（xlsx 族限额）：条目数 → 逐条目（单尺寸/路径逃逸，单趟）
/// → 求和总量。全部基于 zip 中央目录声明值，零条目数据读取。
pub fn prescan<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
) -> Result<PrescanReport, ReadError> {
    prescan_family(za, LIMIT_TOTAL_DECLARED, LIMIT_SINGLE_ENTRY)
}

/// 容器预扫（docx 族限额——r4-t2 族分化：单条目 = 族总量对称设计；
/// 条目数/路径逃逸两族共享）。crate 内消费 = docx 读管线
/// （[`crate::docx`]）。pub(crate)：docx 族限额常量的执行点即此。
pub(crate) fn prescan_docx<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
) -> Result<PrescanReport, ReadError> {
    prescan_family(za, LIMIT_TOTAL_DECLARED_DOCX, LIMIT_TOTAL_DECLARED_DOCX)
}

/// EOCD 守恒门（#233 / zip-exotica 防线——研究仓 guard.rs M1.6 镜像的
/// 轻量移植：只比计数，不重建目录解析）。
///
/// 背景：zip crate（4.6.1 与 8.6 同——#233 实测）解析层用名字映射
/// **吸收**中央目录里的物理同名条目：`len()`/`by_index` 视野看不到
/// 重复，重复名门对字节级 patch 的同名对失效。守恒门比对
/// **EOCD/zip64 声明条目数**与 crate 实际解析条目数：分歧（EOCD 双
/// 计数字段互异，或声明 ≠ 实际）= 守恒破坏 → 拒（fail-closed）。
///
/// 宽松回扫（与研究仓 M1.6 同口径）：自 EOF 反向找 `PK\x05\x06` 候选，
/// 合法 = `pos + 22 + comment_len ≤ len`（容忍尾垃圾——crate 同款宽松
/// 接受；影子 EOCD 的 comment 越 EOF → 不合法 → 继续向前）。全部候选
/// 不可验证 → 拒（fail-closed 非跳过）。zip64：计数/尺寸/偏移任一为
/// 标记（0xFFFF / 0xFFFFFFFF）→ 经 EOCD 前 locator（`PK\x06\x07`）
/// 链取 zip64 EOCD 记录（`PK\x06\x06`）的真实双计数；标记而无合法
/// 链 = 不可验证 → 拒。
pub(crate) fn eocd_conservation<R: std::io::Read + std::io::Seek>(
    bytes: &[u8],
    za: &zip::ZipArchive<R>,
) -> Result<(), ReadError> {
    let actual = za.len() as u64;
    match eocd_declared_count(bytes)? {
        None => Err(ReadError::ContainerCorrupt(
            "eocd unverifiable (no candidate fits the file)".into(),
        )),
        Some((this_disk, total)) => {
            if this_disk != total {
                return Err(ReadError::PrescanRejected {
                    reason: "eocd count fields diverge (this disk vs total)",
                    entry: None,
                });
            }
            if this_disk != actual {
                return Err(ReadError::PrescanRejected {
                    reason: "eocd declared entry count != archive entries (conservation)",
                    entry: None,
                });
            }
            Ok(())
        }
    }
}

/// EOCD/zip64 声明条目数（宽松回扫 + zip64 链；不可验证 = None——
/// 调用方 fail-closed）。见 [`eocd_conservation`] 文档。
fn eocd_declared_count(bytes: &[u8]) -> Result<Option<(u64, u64)>, ReadError> {
    let _ = &bytes; // （形态检查在下方；空/短文件 = None）
    if bytes.len() < 22 {
        return Ok(None);
    }
    let mut pos: Option<usize> = None;
    let mut i = bytes.len() - 22;
    loop {
        if &bytes[i..i + 4] == b"PK\x05\x06" {
            let comment = u16::from_le_bytes(bytes[i + 20..i + 22].try_into().unwrap()) as usize;
            if i + 22 + comment <= bytes.len() {
                pos = Some(i);
                break;
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    let Some(eocd) = pos else {
        return Ok(None);
    };
    let this = u16::from_le_bytes(bytes[eocd + 8..eocd + 10].try_into().unwrap()) as u64;
    let total = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap()) as u64;
    let cd_size = u32::from_le_bytes(bytes[eocd + 12..eocd + 16].try_into().unwrap()) as u64;
    let cd_off = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as u64;
    let is_zip64_marker =
        this == 0xFFFF || total == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF;
    if !is_zip64_marker {
        return Ok(Some((this, total)));
    }
    // zip64 链：locator 紧邻 EOCD 之前（20B：sig + 盘号 + z64 偏移 u64 + 总盘数）。
    if eocd >= 20 && &bytes[eocd - 20..eocd - 16] == b"PK\x06\x07" {
        let z64_off = u64::from_le_bytes(bytes[eocd - 12..eocd - 4].try_into().unwrap()) as usize;
        // zip64 EOCD 记录：sig(0-4) + 尺寸(4-12) + 版本对(12-16) +
        // 盘号对(16-24) + 本盘条目(24-32) + 总条目(32-40) + …（56B 定长）。
        if z64_off + 56 <= bytes.len() && &bytes[z64_off..z64_off + 4] == b"PK\x06\x06" {
            let z_this = u64::from_le_bytes(bytes[z64_off + 24..z64_off + 32].try_into().unwrap());
            let z_total = u64::from_le_bytes(bytes[z64_off + 32..z64_off + 40].try_into().unwrap());
            return Ok(Some((z_this, z_total)));
        }
    }
    // 标记而无合法 zip64 链 = 不可验证（fail-closed）。
    Ok(None)
}

/// 预扫共享核（单点实现——xlsx/docx 两族仅限额数字不同，检查面恒同）。
fn prescan_family<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
    total_limit: u64,
    single_limit: u64,
) -> Result<PrescanReport, ReadError> {
    if za.len() > LIMIT_ENTRY_COUNT {
        return Err(ReadError::PrescanRejected {
            reason: "entry count over limit",
            entry: None,
        });
    }
    let mut total: u64 = 0;
    let mut max_single: u64 = 0;
    // #233（zip-exotica 防线）：重复条目名拒绝。OPC/zip 规范不允许
    // 同名条目；攻击面 = EOCD/zip64 守恒绕过族的端态载体（研究仓
    // guard.rs R5 实证 V6/V7/V8/zip64-dup：crate 层 HashMap 吸收后
    // 声明数与实取数分歧、by_name 取条歧义）。合法 office 文档零
    // 同名（Word/Excel 产档验证）。两族共享（单点实现）。
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for i in 0..za.len() {
        let zf = za
            .by_index(i)
            .map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
        let declared = zf.size();
        if !seen.insert(zf.name().to_string()) {
            return Err(ReadError::PrescanRejected {
                reason: "duplicate entry name",
                entry: Some(zf.name().to_string()),
            });
        }
        if declared > single_limit {
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
    if total > total_limit {
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
    /// #17：修订删除文本（w:delText）按 OOXML 语义不提取（仅当前生效
    /// 文本入读回——delText 是已删除内容）；逐段登记计数提升可见性
    /// （澄清/更正类采购文件的修订通道不静默）。
    RevisionDelete,
    /// #39：面外元素跳过（当前唯一使用面：嵌套表深度超限——敌意/
    /// 退化嵌套整块跳过而非栈溢出崩溃；诚实降级登记）。
    SkippedElement,
    /// #233（zip-exotica）：单条目压缩比超观察门限
    /// （[`LIMIT_COMPRESS_RATIO`]，警告口径不拒——硬拒有实证误报面
    /// s7b 149:1 合法件；膨胀上界已由门 0/1/2 钉死，见常量文档）。
    /// 两模式恒登记——炸弹面可见性，不静默。
    HighCompressionRatio,
    /// #276（B-1）：超链接目标未过 URL 策略（scheme 白名单
    /// [`mdc_office::sanitize::url_policy`]——`javascript:`/`vbscript:`
    /// /`data:` 等敌意 scheme、UNC、NUL、RFC 3986 非法原生字符）。
    /// 读面消毒降级（目标转 [`crate::docx::HyperlinkTarget::Unresolved`]
    /// ——下游链接渲染降级纯文本，不废整个文档）；两模式恒登记。
    UnsafeLink,
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
///
/// r4-t2：同一趟事件流上附带 [`GridScan`] 坐标扫描（稀疏包围盒炸弹
/// 面——零额外解压成本；语义镜像见 GridScan 文档）。
fn validate_xml_structure<R: std::io::BufRead>(
    reader: R,
    grid: &mut GridScan,
) -> Result<(), String> {
    let mut xml = quick_xml::Reader::from_reader(reader);
    let mut buf = Vec::with_capacity(8 * 1024);
    let mut depth: usize = 0;
    let mut saw_root = false;
    use quick_xml::events::Event;
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                grid.on_start(&e);
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
                // calamine cells_reader 只见 Start 形态的 row/c（自闭合
                // 不产 Cell 记录）——GridScan 同语义，Empty 不入扫描。
                if saw_root && depth == 0 {
                    return Err("multiple root elements".into());
                }
                saw_root = true;
            }
            Ok(Event::End(e)) => {
                grid.on_end(&e);
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

// ---------------------------------------------------------------------------
// 稠密包围盒坐标扫描（r4-t2——稀疏包围盒炸弹面）
// ---------------------------------------------------------------------------

/// 行引用属性解析（镜像 calamine `RawAttributes::raw_attr(b"r")` +
/// `get_row`：完整键名精确匹配——带命名空间前缀的 `x:r` 不匹配；
/// 前导字母忽略、数字 1 基 → 0 基；不可解析 → None）。
fn row_attr_ref(e: &quick_xml::events::BytesStart<'_>) -> Option<u32> {
    for attr in e.attributes() {
        let a = attr.ok()?;
        if a.key.as_ref() == b"r" {
            return parse_row_ref(&a.value);
        }
    }
    None
}

/// 格引用属性解析（镜像 `raw_attr(b"r")` + `get_row_column`：字母列
/// bijective base-26 [A=1] + 数字行，双 1 基 → 0 基；不可解析 → None）。
fn cell_attr_ref(e: &quick_xml::events::BytesStart<'_>) -> Option<(u32, u32)> {
    for attr in e.attributes() {
        let a = attr.ok()?;
        if a.key.as_ref() == b"r" {
            return parse_cell_ref(&a.value);
        }
    }
    None
}

/// 行引用解析（前导字母忽略、数字部分 1 基 → 0 基）。
fn parse_row_ref(v: &[u8]) -> Option<u32> {
    let mut i = 0;
    while i < v.len() && v[i].is_ascii_alphabetic() {
        i += 1;
    }
    let mut row: u32 = 0;
    let mut digits = 0;
    while i < v.len() {
        let b = v[i];
        if !b.is_ascii_digit() {
            return None;
        }
        row = row.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        digits += 1;
        i += 1;
    }
    if digits == 0 {
        return None;
    }
    row.checked_sub(1)
}

/// 格引用解析（镜像 calamine `get_row_column`：字母列 bijective base-26
/// [A=1] + 数字行，双 1 基 → 0 基；缺字母列/不可解析 → None）。
fn parse_cell_ref(v: &[u8]) -> Option<(u32, u32)> {
    let mut i = 0;
    let mut col: u32 = 0;
    let mut letters = 0;
    while i < v.len() {
        let b = v[i];
        if b.is_ascii_uppercase() {
            col = col.checked_mul(26)?.checked_add((b - b'A') as u32 + 1)?;
        } else if b.is_ascii_lowercase() {
            col = col.checked_mul(26)?.checked_add((b - b'a') as u32 + 1)?;
        } else {
            break;
        }
        letters += 1;
        i += 1;
    }
    if letters == 0 {
        return None; // RangeWithoutColumnComponent 同语义
    }
    let mut row: u32 = 0;
    let mut digits = 0;
    while i < v.len() {
        let b = v[i];
        if !b.is_ascii_digit() {
            return None;
        }
        row = row.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        digits += 1;
        i += 1;
    }
    if digits == 0 {
        return None;
    }
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

/// 工作表稠密包围盒坐标扫描（r4-t2——镜像 calamine 0.36.1
/// `cells_reader.rs` `next_cell`/`next_formula` 的位置语义，单点同源
/// 纪律：calamine 升版时此镜像须复核）：
///
/// - `<row>` Start 带 `r` → `row_index = r`（0 基）；`</row>` →
///   `row_index += 1`、`col_index = 0`；
/// - `<c>` Start 带 `r` → `pos = (row, col)` 且 `col_index = col`；
///   不带 → `pos = (row_index, col_index)`；格结束后 `col_index += 1`；
/// - **格体内部**（Start(c)…End(c)）不更新行/列游标（calamine 内层
///   循环只认载荷子元素与 End(c)）；
/// - 载荷格 = 有 `v`/`is`/`f` 任一子元素的格——值面网格
///   （`next_cell` 收非 Empty 值格：有 v/is）与公式面网格
///   （`next_formula` 收公式文本格：有 f）的**并集上界**；styled
///   空格（仅 s 属性）calamine 两面均不收 → 扫描亦不收（无假阳）；
/// - `</sheetData>` 后 calamine 停读（done）；自闭合 `<c/>`/`<row/>`
///   calamine 不见（忽略）。
///
/// 面积语义 = from_sparse 的稠密化：`(max_r−min_r+1) × (max_c−min_c+1)`
/// ——两格 A1 + XFD1048576 = 17,179,869,184 格。
struct GridScan {
    row_index: u32,
    col_index: u32,
    in_cell: bool,
    cell_pos: (u32, u32),
    has_payload: bool,
    min_r: u32,
    max_r: u32,
    min_c: u32,
    max_c: u32,
    saw: bool,
    done: bool,
}

impl GridScan {
    fn new() -> Self {
        Self {
            row_index: 0,
            col_index: 0,
            in_cell: false,
            cell_pos: (0, 0),
            has_payload: false,
            min_r: 0,
            max_r: 0,
            min_c: 0,
            max_c: 0,
            saw: false,
            done: false,
        }
    }

    fn absorb(&mut self, pos: (u32, u32)) {
        if !self.saw {
            self.min_r = pos.0;
            self.max_r = pos.0;
            self.min_c = pos.1;
            self.max_c = pos.1;
            self.saw = true;
        } else {
            self.min_r = self.min_r.min(pos.0);
            self.max_r = self.max_r.max(pos.0);
            self.min_c = self.min_c.min(pos.1);
            self.max_c = self.max_c.max(pos.1);
        }
    }

    fn on_start(&mut self, e: &quick_xml::events::BytesStart<'_>) {
        if self.done {
            return;
        }
        let local = e.name().local_name();
        let local = local.as_ref();
        if self.in_cell {
            // calamine 内层循环：载荷子元素标记（v=值/is=内联串/f=公式）。
            if local == b"v" || local == b"is" || local == b"f" {
                self.has_payload = true;
            }
            return;
        }
        match local {
            b"row" => {
                if let Some(row) = row_attr_ref(e) {
                    self.row_index = row;
                }
            }
            b"c" => {
                let pos = match cell_attr_ref(e) {
                    Some((row, col)) => {
                        self.col_index = col;
                        (row, col)
                    }
                    None => (self.row_index, self.col_index),
                };
                self.cell_pos = pos;
                self.has_payload = false;
                self.in_cell = true;
            }
            _ => {}
        }
    }

    fn on_end(&mut self, e: &quick_xml::events::BytesEnd<'_>) {
        if self.done {
            return;
        }
        let local = e.name().local_name();
        let local = local.as_ref();
        if self.in_cell {
            if local == b"c" {
                self.in_cell = false;
                if self.has_payload {
                    self.absorb(self.cell_pos);
                }
                self.col_index = self.col_index.wrapping_add(1);
            }
            return;
        }
        match local {
            b"row" => {
                self.row_index = self.row_index.wrapping_add(1);
                self.col_index = 0;
            }
            b"sheetData" => self.done = true,
            _ => {}
        }
    }

    /// 稠密包围盒面积（u64 饱和——故意的敌意极值不溢出）。
    fn area(&self) -> u64 {
        if !self.saw {
            return 0;
        }
        let rows = (self.max_r - self.min_r + 1) as u64;
        let cols = (self.max_c - self.min_c + 1) as u64;
        rows.saturating_mul(cols)
    }
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
        // #233：压缩比观察（警告口径——见 [`LIMIT_COMPRESS_RATIO`]
        // 常量文档：s7b 149:1 合法件实证误报面 + 既有门钉死膨胀上界）。
        // 乘法判据（整数除法截断漏洞教训同源）。
        let compressed = zf.compressed_size();
        if declared > LIMIT_COMPRESS_RATIO.saturating_mul(compressed.max(1)) {
            warnings.push(CompatibilityWarning {
                part: name.clone(),
                kind: WarningKind::HighCompressionRatio,
                detail: format!(
                    "compression ratio {declared}:{} over {LIMIT_COMPRESS_RATIO} \
                     (observability only — declared-sum/lie-header gates bound the inflation)",
                    compressed.max(1)
                ),
            });
        }
        let is_xml = validate_parts && (name.ends_with(".xml") || name.ends_with(".rels"));
        let mut counting = CountingReader::new(&mut zf, declared, is_xml);
        if is_xml {
            let mut grid = GridScan::new();
            let mut br = std::io::BufReader::new(&mut counting);
            let parse = validate_xml_structure(&mut br, &mut grid);
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
            // 稠密包围盒面积上限（r4-t2，安全上限——先于 calamine 的
            // from_sparse 稠密化分配，且在 strict/compat 分派之前：
            // 两模式恒拒，不随兼容降级）。
            let area = grid.area();
            if area > LIMIT_TOTAL_CELLS {
                return Err(ReadError::PrescanRejected {
                    reason: "worksheet dense grid area over limit",
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
pub(crate) fn epoch_to_1900(serial: f64, is_1904: bool) -> f64 {
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
/// 门在前（敌意输入零解析层接触）：⓪输入字节上限（r4-t2——元数据
/// 检查，拒于任何全量读/解析之前）；①prescan 声明值四检查；
/// ②尺寸诚实性 + 部件良构性融合趟（门 2+3，单次解压——Phase 2
/// 起畸形 XML 部件 typed reject，mal7e 类静默空/部分数据闭环；
/// r4-t2 同趟附稠密包围盒坐标扫描）。
/// 门过后错误归 ParseError。
pub fn read_book(path: &str) -> Result<Book, ReadError> {
    read_book_with_options(path, &ReadOptions::default())
}

/// 读入口（带选项——Phase 2）：[`ReadOptions::compat`] = true 时
/// 畸形部件降级为结构化警告（数据照常提取，门 1/门 2/安全上限
/// 恒不放松）；默认严格行为同 [`read_book`]。
pub fn read_book_with_options(path: &str, opts: &ReadOptions) -> Result<Book, ReadError> {
    // 门 0（r4-t2）：输入字节上限——元数据检查先于打开/读取
    //（xlsx 读面惰性 Seek 无全量分配，本限统一四入口字节纪律：
    // 前缀垃圾面拒绝）。
    let meta =
        std::fs::metadata(path).map_err(|e| ReadError::ContainerCorrupt(format!("stat: {e}")))?;
    check_input_size(meta.len(), LIMIT_INPUT_BYTES)?;
    let f =
        std::fs::File::open(path).map_err(|e| ReadError::ContainerCorrupt(format!("open: {e}")))?;
    let mut za =
        zip::ZipArchive::new(f).map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
    prescan(&mut za)?; // 门 1：声明值四检查
    let mut warnings = Vec::new();
    // 门 2+3 融合趟：尺寸诚实性 + 部件良构性 + 稠密包围盒扫描（单次解压）。
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
    // 门 0（r4-t2）：输入字节上限（字节入口同限——四入口统一）。
    check_input_size(bytes.len() as u64, LIMIT_INPUT_BYTES)?;
    let cursor = std::io::Cursor::new(bytes);
    let mut za =
        zip::ZipArchive::new(cursor).map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
    // #233：EOCD 守恒门（zip-exotica 防线）——crate 吸收物理同名条目
    // 使 by_index 视野失明；字节入口全量在手，先于 prescan。
    //（路径入口 read_book_with_options 保惰性 Seek 无全量分配设计，
    // 不接本门——登记：路径面为桌面本地消费，敌意主面在 bytes 入口。）
    eocd_conservation(bytes, &za)?;
    prescan(&mut za)?; // 门 1：声明值四检查
    let mut warnings = Vec::new();
    // 门 2+3 融合趟：尺寸诚实性 + 部件良构性 + 稠密包围盒扫描（单次解压）。
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
///
/// r4-t2 提取期资源上限（安全类——typed 安全拒绝，先于 typed 网格
/// 分配以控瞬态）：①表数上限（[`LIMIT_SHEET_COUNT`]——空表名洪水
/// CPU 面，循环之前检查）；②全簿累计稠密面积上限
/// （[`LIMIT_TOTAL_CELLS`]——多表名指向同部件的倍增保留面：门 3
/// 单部件扫描无法覆盖跨名累计，此处按已提取网格实际面积累计）。
fn extract_sheets<R: std::io::Read + std::io::Seek>(
    book: &mut Xlsx<R>,
    warnings: Vec<CompatibilityWarning>,
) -> Result<Book, ReadError> {
    let is_1904 = book.has_1904_epoch();
    let names = book.sheet_names().to_vec();
    if names.len() > LIMIT_SHEET_COUNT {
        return Err(ReadError::PrescanRejected {
            reason: "sheet count over limit",
            entry: None,
        });
    }
    let mut total_grid_area: u64 = 0;
    let mut sheets = Vec::with_capacity(names.len());
    for name in &names {
        let values = book
            .worksheet_range(name)
            .map_err(|e| ReadError::ParseError(format!("range[{name}]: {e}")))?;
        let formulas = book
            .worksheet_formula(name)
            .map_err(|e| ReadError::ParseError(format!("formula[{name}]: {e}")))?;
        // 累计稠密面积（values + formulas 两网格——typed 为 values 的
        // 同面积拷贝不另计）：超限即拒，先于本表 typed 分配。
        total_grid_area =
            total_grid_area.saturating_add((values.width() as u64) * (values.height() as u64));
        total_grid_area =
            total_grid_area.saturating_add((formulas.width() as u64) * (formulas.height() as u64));
        if total_grid_area > LIMIT_TOTAL_CELLS {
            return Err(ReadError::PrescanRejected {
                reason: "cumulative grid area over limit",
                entry: Some(name.clone()),
            });
        }
        let typed = CellGrid::from_values(&values, is_1904);
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

    // -----------------------------------------------------------------
    // r4-t2 资源上限（输入字节 / 表数 / 稠密包围盒面积 / docx 族分化）
    // -----------------------------------------------------------------

    /// 列名（0 基 → Excel 字母，bijective base-26——边界坐标测试内自算）。
    fn col_name(mut c0: u32) -> String {
        let mut s = Vec::new();
        loop {
            s.push(b'A' + (c0 % 26) as u8);
            if c0 < 26 {
                break;
            }
            c0 = c0 / 26 - 1;
        }
        s.reverse();
        String::from_utf8(s).unwrap()
    }

    /// 两载荷格工作表 XML（A1 + 远角格——稀疏包围盒形态；远角格坐标
    /// 由 c@r 显式定位，镜像 calamine 语义）。
    fn sparse_sheet_xml(far_row: u32, far_col0: u32) -> String {
        let far = format!("{}{}", col_name(far_col0), far_row + 1);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c></row><row r="{row}"><c r="{far}" t="inlineStr"><is><t>b</t></is></c></row></sheetData></worksheet>"#,
            row = far_row + 1,
        )
    }

    /// R11 门 3 面积上限——**稀疏包围盒炸弹**（a 轮探针实证：1588B
    /// 输入 → calamine `Range::from_sparse` 尝试 549,755,813,888 字节
    ///（~550GB）分配 → 进程 abort）。两载荷格 A1 + XFD1048576 →
    /// 稠密面积 17,179,869,184 > LIMIT_TOTAL_CELLS → 门 3 typed 拒
    ///（先于 calamine 分配），strict/compat 两模式恒拒（安全上限
    /// 不随兼容放松）。
    #[test]
    fn sparse_grid_bomb_rejected_both_modes() {
        let mut entries = minimal_xlsx("unused");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                // 两格同排（c@r 显式定位行 1048576——row 元素行号被
                // 格自身 r 覆盖，calamine 同语义）。
                *d = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c><c r="XFD1048576" t="inlineStr"><is><t>b</t></is></c></row></sheetData></worksheet>"#.to_vec();
            }
        }
        let p = tmp("sparsebomb");
        write_zip(&p, &entries);
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_book_with_options(&p, &opts) {
                Err(ReadError::PrescanRejected { reason, entry }) => {
                    assert_eq!(reason, "worksheet dense grid area over limit");
                    assert_eq!(entry.as_deref(), Some("xl/worksheets/sheet1.xml"));
                }
                other => panic!("expected grid area reject, got {other:?}"),
            }
        }
        let _ = std::fs::remove_file(&p);
    }

    /// R11 边界对测（门 3 面积 + extract Σ 累计同一比较的两面）：
    /// 载荷格 (0,0)+(1999,1999) → 稠密面积恰 = LIMIT_TOTAL_CELLS
    ///（2000×2000 = 4,000,000）→ 门 3 过 + 全提取（from_sparse 稠密化
    /// 4M 格——debug 峰值 ~300MB，发布面可复现基准 =
    /// examples/bench_limits.rs）；(0,0)+(1999,2000) → 2000×2001 →
    /// 门 3 拒。Σ「恰=限过」同比较语义（单表 values 4M + formulas 0
    /// → total == LIMIT → 不拒）由本测试通过面覆盖。
    #[test]
    fn grid_area_boundary_exact_limit_passes_over_rejected() {
        // 限 + 1 列 → 门 3 typed 拒（strict/compat 同）。
        let mut entries = minimal_xlsx("unused");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                *d = sparse_sheet_xml(1999, 2000).into_bytes();
            }
        }
        let p = tmp("gridover");
        write_zip(&p, &entries);
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_book_with_options(&p, &opts) {
                Err(ReadError::PrescanRejected { reason, .. }) => {
                    assert_eq!(reason, "worksheet dense grid area over limit");
                }
                other => panic!("expected grid area reject, got {other:?}"),
            }
        }
        let _ = std::fs::remove_file(&p);

        // 恰 = 限 → 全程通过（门 3 + Σ + typed 网格分配）。
        let mut entries = minimal_xlsx("unused");
        for (n, d) in entries.iter_mut() {
            if n == "xl/worksheets/sheet1.xml" {
                *d = sparse_sheet_xml(1999, 1999).into_bytes();
            }
        }
        let p2 = tmp("gridexact");
        write_zip(&p2, &entries);
        let book = read_book(&p2).unwrap();
        assert_eq!(book.sheets.len(), 1);
        assert_eq!(book.non_empty_cells(), 2, "两载荷格保持可见");
        let _ = std::fs::remove_file(&p2);
    }

    /// R11 extract Σ（跨名倍增面——门 3 单部件扫描覆盖不了多表累计）：
    /// S1 面积 1,999,396（1414²）+ S2 面积 2,002,225（1415²）→
    /// Σ = 4,001,621 > 4M → S2 提取处 typed 拒（entry = 表名，可诊断）。
    #[test]
    fn cumulative_grid_area_over_limit_rejected() {
        let two_sheets = |s1: String, s2: String| {
            vec![
                (
                    "[Content_Types].xml".into(),
                    br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec(),
                ),
                (
                    "_rels/.rels".into(),
                    br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec(),
                ),
                (
                    "xl/workbook.xml".into(),
                    br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S1" sheetId="1" r:id="rId1"/><sheet name="S2" sheetId="2" r:id="rId2"/></sheets></workbook>"#.to_vec(),
                ),
                (
                    "xl/_rels/workbook.xml.rels".into(),
                    br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#.to_vec(),
                ),
                ("xl/worksheets/sheet1.xml".into(), s1.into_bytes()),
                ("xl/worksheets/sheet2.xml".into(), s2.into_bytes()),
            ]
        };
        let entries = two_sheets(sparse_sheet_xml(1413, 1413), sparse_sheet_xml(1414, 1414));
        let p = tmp("cumul");
        write_zip(&p, &entries);
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_book_with_options(&p, &opts) {
                Err(ReadError::PrescanRejected { reason, entry }) => {
                    assert_eq!(reason, "cumulative grid area over limit");
                    assert_eq!(entry.as_deref(), Some("S2"), "拒于第二表，表名可诊断");
                }
                other => panic!("expected cumulative reject, got {other:?}"),
            }
        }
        let _ = std::fs::remove_file(&p);
    }

    /// R10 表数上限：恰 1,000 表全读通过（每表 1 格——C1 大档能力
    /// 面）；1,001 → 提取循环之前 typed 拒（空表名洪水 CPU 面）；
    /// compat 同限。表数为语义计数非声明值——无「极端声明」类
    ///（边界即诊断面，登记）。
    #[test]
    fn sheet_count_limit_enforced() {
        let multi_sheet_xlsx = |n: usize| {
            let mut overrides = String::new();
            let mut sheets = String::new();
            let mut rels = String::new();
            for i in 1..=n {
                overrides.push_str(&format!(
                    r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
                ));
                sheets.push_str(&format!(
                    r#"<sheet name="S{i}" sheetId="{i}" r:id="rId{i}"/>"#
                ));
                rels.push_str(&format!(
                    r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#
                ));
            }
            let mut entries = vec![
                (
                    "[Content_Types].xml".to_string(),
                    format!(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>{overrides}</Types>"#
                    )
                    .into_bytes(),
                ),
                (
                    "_rels/.rels".to_string(),
                    br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec(),
                ),
                (
                    "xl/workbook.xml".to_string(),
                    format!(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheets}</sheets></workbook>"#
                    )
                    .into_bytes(),
                ),
                (
                    "xl/_rels/workbook.xml.rels".to_string(),
                    format!(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
                    )
                    .into_bytes(),
                ),
            ];
            let one_cell = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>v</t></is></c></row></sheetData></worksheet>"#;
            for i in 1..=n {
                entries.push((format!("xl/worksheets/sheet{i}.xml"), one_cell.to_vec()));
            }
            entries
        };

        // 恰 = 限：1,000 表全读通过。
        let p = tmp("sheets1000");
        write_zip(&p, &multi_sheet_xlsx(LIMIT_SHEET_COUNT));
        let book = read_book(&p).unwrap();
        assert_eq!(book.sheets.len(), LIMIT_SHEET_COUNT);
        assert_eq!(book.non_empty_cells(), LIMIT_SHEET_COUNT);
        let _ = std::fs::remove_file(&p);

        // 限 + 1：提取前 typed 拒（strict/compat 同）。
        let p2 = tmp("sheets1001");
        write_zip(&p2, &multi_sheet_xlsx(LIMIT_SHEET_COUNT + 1));
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_book_with_options(&p2, &opts) {
                Err(ReadError::PrescanRejected { reason, .. }) => {
                    assert_eq!(reason, "sheet count over limit");
                }
                other => panic!("expected sheet count reject, got {other:?}"),
            }
        }
        let _ = std::fs::remove_file(&p2);
    }

    /// R1 输入字节上限（path 入口）：元数据检查先于打开/读取——
    /// 恰 = LIMIT_INPUT_BYTES 过门 0（零字节稀疏档非 zip → 后续
    /// ContainerCorrupt，可区分）；限 + 1 与极端 4× → typed 拒；
    /// compat 同限。稀疏文件（set_len）构造——零实际磁盘写。
    #[test]
    fn input_size_limit_path_entry() {
        let p = tmp("inputsize");
        let set_len = |n: u64| {
            let f = std::fs::File::create(&p).unwrap();
            f.set_len(n).unwrap();
        };
        // 恰 = 限：过门 0，败于 zip 结构（ContainerCorrupt ≠ prescan 拒）。
        set_len(LIMIT_INPUT_BYTES);
        match read_book(&p) {
            Err(ReadError::ContainerCorrupt(_)) => {}
            other => panic!("expected container corrupt at exact limit, got {other:?}"),
        }
        // 限 + 1：两模式同拒。
        set_len(LIMIT_INPUT_BYTES + 1);
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_book_with_options(&p, &opts) {
                Err(ReadError::PrescanRejected { reason, .. }) => {
                    assert_eq!(reason, "input size over limit");
                }
                other => panic!("expected input size reject, got {other:?}"),
            }
        }
        // 极端：4× 限（前缀垃圾/伪装载荷面）。
        set_len(LIMIT_INPUT_BYTES * 4);
        match read_book(&p) {
            Err(ReadError::PrescanRejected { reason, .. }) => {
                assert_eq!(reason, "input size over limit");
            }
            other => panic!("expected input size reject, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// R2 输入字节上限（bytes 入口）——四入口统一同限。
    #[test]
    fn input_size_limit_bytes_entry() {
        // 恰 = 限：过门 0，败于 zip 结构。
        let at = vec![0u8; LIMIT_INPUT_BYTES as usize];
        match read_book_from_bytes(&at) {
            Err(ReadError::ContainerCorrupt(_)) => {}
            other => panic!("expected container corrupt at exact limit, got {other:?}"),
        }
        drop(at);
        // 限 + 1：两模式同拒。
        let over = vec![0u8; LIMIT_INPUT_BYTES as usize + 1];
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_book_from_bytes_with_options(&over, &opts) {
                Err(ReadError::PrescanRejected { reason, .. }) => {
                    assert_eq!(reason, "input size over limit");
                }
                other => panic!("expected input size reject, got {other:?}"),
            }
        }
    }

    /// 伪造声明尺寸的原始 zip（STORED、零数据——prescan 只读中央
    /// 目录，条目数据无需在位；LFH/CD 声明一致，非谎言头面）。
    fn forged_declared_zip(entries: &[(&str, u64)]) -> Vec<u8> {
        let mut z = Vec::new();
        let mut offsets = Vec::new();
        for (name, declared) in entries {
            offsets.push(z.len() as u32);
            z.extend_from_slice(b"PK\x03\x04");
            z.extend_from_slice(&20u16.to_le_bytes());
            z.extend_from_slice(&0u16.to_le_bytes()); // flags
            z.extend_from_slice(&0u16.to_le_bytes()); // STORED
            z.extend_from_slice(&0u16.to_le_bytes()); // time
            z.extend_from_slice(&0u16.to_le_bytes()); // date
            z.extend_from_slice(&0u32.to_le_bytes()); // crc
            z.extend_from_slice(&(*declared as u32).to_le_bytes()); // csize
            z.extend_from_slice(&(*declared as u32).to_le_bytes()); // usize
            z.extend_from_slice(&(name.len() as u16).to_le_bytes());
            z.extend_from_slice(&0u16.to_le_bytes()); // extra
            z.extend_from_slice(name.as_bytes());
        }
        let cd_off = z.len() as u32;
        for ((name, declared), off) in entries.iter().zip(&offsets) {
            z.extend_from_slice(b"PK\x01\x02");
            z.extend_from_slice(&20u16.to_le_bytes());
            z.extend_from_slice(&20u16.to_le_bytes());
            z.extend_from_slice(&0u16.to_le_bytes());
            z.extend_from_slice(&0u16.to_le_bytes()); // STORED
            z.extend_from_slice(&0u16.to_le_bytes()); // time
            z.extend_from_slice(&0u16.to_le_bytes()); // date
            z.extend_from_slice(&0u32.to_le_bytes()); // crc
            z.extend_from_slice(&(*declared as u32).to_le_bytes());
            z.extend_from_slice(&(*declared as u32).to_le_bytes());
            z.extend_from_slice(&(name.len() as u16).to_le_bytes());
            z.extend_from_slice(&0u16.to_le_bytes()); // extra
            z.extend_from_slice(&0u16.to_le_bytes()); // comment
            z.extend_from_slice(&0u16.to_le_bytes()); // disk
            z.extend_from_slice(&0u16.to_le_bytes()); // iattr
            z.extend_from_slice(&0u32.to_le_bytes()); // eattr
            z.extend_from_slice(&off.to_le_bytes());
            z.extend_from_slice(name.as_bytes());
        }
        let cd_size = z.len() as u32 - cd_off;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        z.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        z.extend_from_slice(&cd_size.to_le_bytes());
        z.extend_from_slice(&cd_off.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z
    }

    /// D3 docx 族声明限（prescan_docx——族分化执行点，r4-t2）：
    /// 单条目 > 50MB 拒；两分条目 Σ > 50MB 拒；恰 = 50MB 过门 1
    ///（PrescanReport 带真实求和）。xlsx 族 300MB 既有锚点 =
    /// harness mal6c（zip64 声明 320MB）+ T3——不重复造。
    #[test]
    fn prescan_docx_family_limits() {
        fn open(z: &[u8]) -> zip::ZipArchive<std::io::Cursor<&[u8]>> {
            zip::ZipArchive::new(std::io::Cursor::new(z)).expect("zip openable")
        }
        // 单条目 > 族限 → 单条目门拒。
        let z = forged_declared_zip(&[("blob.bin", LIMIT_TOTAL_DECLARED_DOCX + 1)]);
        let mut za = open(&z);
        match prescan_docx(&mut za) {
            Err(ReadError::PrescanRejected { reason, entry }) => {
                assert_eq!(reason, "single entry declared size over limit");
                assert_eq!(entry.as_deref(), Some("blob.bin"));
            }
            other => panic!("expected single-entry reject, got {other:?}"),
        }
        // 两分条目各 ≤ 族限、Σ > 族限 → 总量门拒。
        let half = LIMIT_TOTAL_DECLARED_DOCX / 2 + 1; // 26,214,401 × 2 = 52,428,802 > 52,428,800
        let z2 = forged_declared_zip(&[("a.bin", half), ("b.bin", half)]);
        let mut za2 = open(&z2);
        match prescan_docx(&mut za2) {
            Err(ReadError::PrescanRejected { reason, .. }) => {
                assert_eq!(reason, "total declared size over limit");
            }
            other => panic!("expected total reject, got {other:?}"),
        }
        // 恰 = 族限 → 过门（report 求和 == 族限）。
        let z3 = forged_declared_zip(&[("ok.bin", LIMIT_TOTAL_DECLARED_DOCX)]);
        let mut za3 = open(&z3);
        let report = prescan_docx(&mut za3).expect("exact limit passes");
        assert_eq!(report.total_declared, LIMIT_TOTAL_DECLARED_DOCX);
        assert_eq!(report.max_single_declared, LIMIT_TOTAL_DECLARED_DOCX);
    }

    /// R11 GridScan 位置语义镜像锚定（calamine 0.36.1 cells_reader
    /// next_cell/next_formula——单点同源纪律；升版复核锚点）：
    /// 显式 r 定位 / 缺 r 顺序游标 / 行结束推进 / 载荷判定（v/is/f
    /// 并集上界）/ styled 空格不计 / sheetData 后停读 / 自闭合不计
    /// / 巨型引用解析与边界。
    #[test]
    fn grid_scan_mirror_semantics() {
        // ① 显式定位：C5（0 基 (4,2)）单格 → 面积 1；</sheetData> 后
        //    的 row/c（镜像 done 停读）不吸收。
        let xml1 = br#"<worksheet><sheetData><row r="3"><c r="C5" t="inlineStr"><is><t>x</t></is></c></row></sheetData><row r="9"><c r="A9"><v>1</v></c></row></worksheet>"#;
        let mut g = GridScan::new();
        validate_xml_structure(&xml1[..], &mut g).unwrap();
        assert_eq!(g.area(), 1);

        // ② 缺 r 顺序游标：首行 c1=(0,0) c2=(0,1)（v 载荷）；
        //    </row> → row_index+1、col 归零；次行 c=(1,0)（f 载荷）。
        let xml2 = br#"<worksheet><sheetData><row><c><v>1</v></c><c><v>2</v></c></row><row><c><f>SUM(A1:A2)</f></c></row></sheetData></worksheet>"#;
        let mut g2 = GridScan::new();
        validate_xml_structure(&xml2[..], &mut g2).unwrap();
        assert_eq!(g2.area(), 4, "两行两列包围盒");
        assert_eq!((g2.min_r, g2.min_c, g2.max_r, g2.max_c), (0, 0, 1, 1));

        // ③ styled 空格不计（仅 s 属性无 v/is/f 子——calamine 两面
        //    均不收）：纯 styled 空表 → 面积 0；载荷格不受 styled
        //    邻居扩包围盒。
        let xml3a = br#"<worksheet><sheetData><row r="1"><c r="XFD1048576" s="7"></c></row></sheetData></worksheet>"#;
        let mut g3a = GridScan::new();
        validate_xml_structure(&xml3a[..], &mut g3a).unwrap();
        assert_eq!(g3a.area(), 0, "纯 styled 空表零面积");
        let xml3 = br#"<worksheet><sheetData><row r="1"><c r="B2" s="7"></c><c r="C3" s="7"><v>5</v></c></row></sheetData></worksheet>"#;
        let mut g3 = GridScan::new();
        validate_xml_structure(&xml3[..], &mut g3).unwrap();
        assert_eq!(g3.area(), 1, "仅载荷格 (2,2) 入包围盒");

        // ④ 自闭合 c/row 不入扫描（calamine Empty 形态不见——无假阳
        //    反面：自闭合 styled 格不扩包围盒）。
        let xml4 = br#"<worksheet><sheetData><row r="1"><c r="A1"><is><t>a</t></is></c><c r="XFD1048576" s="1"/></row></sheetData></worksheet>"#;
        let mut g4 = GridScan::new();
        validate_xml_structure(&xml4[..], &mut g4).unwrap();
        assert_eq!(g4.area(), 1, "自闭合格不吸收");

        // ⑤ 稀疏炸弹形态（探针复刻）：面积 = 全表稠密化 17,179,869,184。
        let xml5 = br#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c><c r="XFD1048576" t="inlineStr"><is><t>b</t></is></c></row></sheetData></worksheet>"#;
        let mut g5 = GridScan::new();
        validate_xml_structure(&xml5[..], &mut g5).unwrap();
        assert_eq!(g5.area(), 17_179_869_184);

        // ⑥ 引用解析边界：小写列同值 / 行引用前导字母忽略 / 不可
        //    解析（无数字/溢出/行 0）→ None（顺序游标兜底，不 panic）。
        assert_eq!(parse_cell_ref(b"a1"), Some((0, 0)));
        assert_eq!(parse_cell_ref(b"XFD1048576"), Some((1048575, 16383)));
        assert_eq!(parse_cell_ref(b"1048576"), None, "缺字母列");
        assert_eq!(parse_cell_ref(b"A"), None, "缺数字行");
        assert_eq!(parse_cell_ref(b"A0"), None, "行 0 不可解析（1 基）");
        assert_eq!(parse_cell_ref(b"AAAA0"), None, "列溢出 u32");
        assert_eq!(parse_row_ref(b"42"), Some(41));
        assert_eq!(parse_row_ref(b"r42"), Some(41), "前导字母忽略");
        assert_eq!(parse_row_ref(b"abc"), None);
    }
}
