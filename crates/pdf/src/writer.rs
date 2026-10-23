//! 标注写抽象——PDF-d 标注面的常开契约层。
//!
//! 分层（与 `engine` / `engine_pdfium` 的读写分工同构）：
//! - **本模块常开**（零依赖，默认构建可测）：
//!   - [`AnnotationWriter`] trait（文档级会话面，非命令面——落盘是
//!     宿主触发的保存事务，不是六命令之一，命令面零破坏）；
//!   - [`IncrementalSave`] 统计面（增量语义的**证据载荷**：
//!     `bytes_appended` 最小化 = 幂等 flush 的可断言面）；
//!   - [`MockWriter`]（契约面可测）；
//!   - 三条 Acrobat 互通约定的纯函数：[`encode_author`] / [`decode_author`]
//!     的 /T origin 约定 + [`pdf_date_string`] 的 /M 日期串（值时钟语义，
//!     无系统时区依赖）；
//! - **feature `engine-lopdf`**（[`crate::writer_lopdf::LopdfWriter`]）：
//!   lopdf 0.45 IncrementalDocument 增量写实装（原字节前缀逐位保持）。
//!
//! 语义合同（实现方纪律，写入方文档）：
//! - `write_annotation` 幂等：同 id 重复写 = **原位替换**（编辑后重写不
//!   产生重复对象）——flush 失败重试安全性由本合同背书；
//! - `save_incremental` 增量语义：产物 = 原字节前缀逐位保持 + 追加修订；
//!   无新写内容时**不追加字节**（`bytes_appended = 0`，幂等）。

use crate::annotation::{AnnotId, Annotation, Origin};
use crate::command::CommandError;
use crate::page::PageIndex;

/// 增量保存统计（增量语义的证据载荷；纯数据面——flush 会话 API 的
/// 返回值，非命令载荷，刻意不进 serde 协议面，PDF-e 视图层定稿时再议）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncrementalSave {
    /// 本次保存追加的字节数（零脏 flush = 0——幂等语义的可断言面）。
    pub bytes_appended: u64,
    /// 保存前产物长度（= 原字节 + 既往修订——「前缀逐位保持」的基线；
    /// 产物长度 = prefix_bytes + bytes_appended）。
    pub prefix_bytes: u64,
}

/// 标注增量写器（文档级会话面）。实现方纪律见模块文档；会话侧调度
/// （脏集合跟踪 + flush）见 [`crate::document::PdfSession::flush_annotations`]。
pub trait AnnotationWriter {
    /// 将单条标注写入待保存修订（页对象解析 + 标准对象构造 + 页 /Annots
    /// 挂载）。同 id 重复调用 = 原位替换。
    fn write_annotation(&mut self, page: PageIndex, annot: &Annotation)
        -> Result<(), CommandError>;

    /// 保存为增量修订：原字节前缀逐位保持 + 追加。返回统计；无未保存
    /// 内容时不追加字节（幂等）。
    fn save_incremental(&mut self) -> Result<IncrementalSave, CommandError>;

    /// 当前产物字节（前缀 + 既往追加修订——宿主取回面，PDF-e 视图
    /// 层定稿：落盘位置由宿主决定，会话自身不写文件）。
    fn product_bytes(&self) -> Vec<u8>;
}

// ---------------------------------------------------------------------------
// /T origin 编码约定（Acrobat 互通决策，PDF-d 定稿）
// ---------------------------------------------------------------------------

/// Ai 溯源后缀（[`encode_author`] 约定）：`"{author} [ai]"`——可逆
/// （后缀剥离）且在 Acrobat 标注作者栏可读；Human 作者不带后缀。
/// 已知歧义（在档不修）：Human 作者名字面恰以 `" [ai]"` 结尾时解码侧
/// 会误判为 Ai——v0 接受（解码仅用于自家回读对账，写侧权威在模型）。
pub const ORIGIN_SUFFIX_AI: &str = " [ai]";

/// /T（author）编码：Human = 原文；Ai = `"{author} [ai]"`。
pub fn encode_author(author: &str, origin: Origin) -> String {
    match origin {
        Origin::Human => author.to_string(),
        Origin::Ai => format!("{author}{ORIGIN_SUFFIX_AI}"),
    }
}

/// /T 解码（编码的逆）：以 [`ORIGIN_SUFFIX_AI`] 结尾 → 剥离后缀 + Ai；
/// 否则原文 + Human。
pub fn decode_author(t: &str) -> (&str, Origin) {
    match t.strip_suffix(ORIGIN_SUFFIX_AI) {
        Some(stripped) => (stripped, Origin::Ai),
        None => (t, Origin::Human),
    }
}

// ---------------------------------------------------------------------------
// /M 日期串（PDF 32000 7.9.4；纯函数 + 值时钟——无系统时区/时钟依赖）
// ---------------------------------------------------------------------------

/// epoch ms → PDF 日期串 `D:YYYYMMDDHHmmSS+00'00'`（UTC 固定偏移——
/// 值时钟语义：会话时钟注入（[`crate::document::PdfSession::set_clock_ms`]）
/// 是唯一时间源，时区取 UTC 使同值时钟在任何环境产出同一字符串）。
///
/// 纯函数（自实现 civil-from-days，零 chrono 依赖——lopdf 特性面
/// default-features = false，日期约定不引入时钟 crate）。
pub fn pdf_date_string(epoch_ms: u64) -> String {
    let secs = epoch_ms / 1000;
    let days = (secs / 86400) as i64; // 1970-01-01 起
    let sod = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}+00'00'",
        y,
        m,
        d,
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// 天数（自 1970-01-01）→ 公历年/月/日（Howard Hinnant 算法——无依赖
/// 纯整数实现；1970 前的日期不可能出现（u64 epoch），负分支仅为算法
/// 完整性）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// MockWriter（常开契约面测试用）
// ---------------------------------------------------------------------------

/// mock 写器（常开）：记录 [`AnnotationWriter`] 调用面供断言（写入序、
/// 保存次数、失败注入）。字节模型确定性：每次保存追加
/// `8 + 32 × 待保存写入数`（幂等可断言）；`fail_next_save` 保存失败一次
/// 后自动复位（重试路径可测）。
#[derive(Debug, Default)]
pub struct MockWriter {
    /// `write_annotation` 调用记录（页号 + 标注 id，调用序）。
    pub writes: Vec<(PageIndex, AnnotId)>,
    /// `save_incremental` 调用次数（含失败次）。
    pub save_calls: usize,
    /// 下次保存失败（一次后自动复位）。
    pub fail_next_save: bool,
    /// 当前产物长度（模拟面：从 0 起按追加模型累计）。
    product_bytes: u64,
    /// 上次成功保存以来的写入数。
    pending: usize,
}

impl MockWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前产物长度（断言用）。
    pub fn product_bytes(&self) -> u64 {
        self.product_bytes
    }
}

impl AnnotationWriter for MockWriter {
    fn write_annotation(
        &mut self,
        page: PageIndex,
        annot: &Annotation,
    ) -> Result<(), CommandError> {
        self.writes.push((page, annot.id));
        self.pending += 1;
        Ok(())
    }

    // mock 无产物面——长度断言走固有 product_bytes() -> u64。
    fn product_bytes(&self) -> Vec<u8> {
        Vec::new()
    }

    fn save_incremental(&mut self) -> Result<IncrementalSave, CommandError> {
        self.save_calls += 1;
        if self.fail_next_save {
            self.fail_next_save = false;
            return Err(CommandError::engine("mock 保存失败（注入）"));
        }
        // 零脏幂等：无待保存写入 → 不追加（与 LopdfWriter 同合同）。
        let bytes_appended = if self.pending == 0 {
            0
        } else {
            8 + 32 * self.pending as u64
        };
        let prefix_bytes = self.product_bytes;
        self.product_bytes += bytes_appended;
        self.pending = 0;
        Ok(IncrementalSave {
            bytes_appended,
            prefix_bytes,
        })
    }
}
