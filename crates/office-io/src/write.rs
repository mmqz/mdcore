//! XLSX 写路径产品模块——新建工作簿（C2 写面产品化，xlsx-r2 Phase 1）。
//!
//! **定位（2026-10-14，用户三阶段指令 Phase 1）**：把分析仓 xlsx-r1
//! 已验证的写面证据链（C2：spike 写读互证 + T3 探针 100 万格
//! constant_memory 972ms + style_probe XML 级 9/9）产品化为正式写
//! API。引擎 = rust_xlsxwriter 0.99.1（DEPS.md §1 锁定，MIT OR
//! Apache-2.0 按 MIT 履行），本模块为**引擎边界包装**：
//!
//! - **引擎类型零泄漏**：公开 API 只暴露本 crate 自有类型
//!   （[`CellValue`]/[`CellStyle`]/[`HyperlinkSpec`] 等），rust_xlsxwriter
//!   类型不出现在任何公开签名——引擎升级/替换不破坏消费方（引擎边界
//!   独立纪律，xlsx-r2 Phase 1 指令）。
//! - **输入验证前移**：表名规则（非空 / ≤31 字符 / 禁 `[]:*?/\` /
//!   禁首尾单引号 / 禁保留名 History / 禁重复）、坐标边界
//!   （行 ≤ 1,048,575 / 列 ≤ 16,383）、合并范围次序——全部在包装层
//!   预验证为 [`WriteError::InvalidInput`]（typed，可重试），不依赖
//!   引擎错误形态（契约独立于引擎版本）。
//! - **流式纪律 typed 拦截（反静默丢失）**：rust_xlsxwriter
//!   constant_memory 模式下对已刷写行的写入**静默忽略**（源级一手
//!   核验：worksheet.rs insert_cell `row < current_row` → eprintln +
//!   return，无类型化错误）——本包装层跟踪行高水位，回退写入提前
//!   [`WriteError::InvalidInput`] 拒绝，**绝不静默丢数据**（与 Phase 2
//!   mal7e 类「畸形输入不得静默变空数据」同一纪律的写侧对偶）。
//! - **r4-t2 资源与格式上限（四件套：常量 + 执行点 + typed 错误 +
//!   测试）**：
//!   - [`MAX_SHEETS`]（表数）：引擎 `constant_memory` 特性下每表
//!     急切 tempfile（fd 消耗 = 表数，含非流式表）；256 = b 轮实测
//!     校准（a 轮 512 的单进程假设被双簿并发 EMFILE 证伪——双簿
//!     并发 512 fd + 开销对软限 1024 余量 1.8×）+ 自产文件须过自身
//!     读面条目数限（10,000）；
//!   - [`MAX_CELLS`]（全簿单元格累计）：非流式引擎内存 Vec 与流式
//!     tempfile 尺寸的无界增长面统一界（引擎无内建——一手核验）；
//!     1,000,000 = T3 探针 100 万格 constant_memory 972ms 实测面，
//!     基准见 examples/bench_limits.rs；值 < 2^20 保证本限先于
//!     mdcore-wasm 导出面的防御截断触发；
//!   - [`MAX_CELL_TEXT`] / [`MAX_FORMULA_TEXT`] / [`MAX_URL_TEXT`]
//!     （格式面）：Excel 物理限 32767 字符/格（引擎 write_string 路径
//!     同值内建但归 Engine 错误类且 cached 面不检——包装层统一前移
//!     InvalidInput，含合并左上格与超链接显示文本）、公式 8192 字符
//!     （Excel 规格）、超链接 url 2080（引擎 url.rs MAX_URL_LEN 镜像
//!     ——b 轮实测校准，a 轮「url 同 32767」被引擎真值证伪）。
//!   - 错误分类：资源限 → [`WriteError::Limit`]（wasm 码
//!     `xlsx_limit`）；格式限 → [`WriteError::InvalidInput`]（`xlsx_build`
//!     外可区分）；引擎/IO → [`WriteError::Engine`]。
//!
//! ## 新建 vs 编辑边界（与 CAPABILITIES.md C2/C3 对齐——唯一真源）
//!
//! - **本模块 = 仅新建工作簿**（create-from-scratch）。不存在「打开
//!   已有工作簿 → 修改 → 保存」路径，**不宣称已有工作簿无损编辑**。
//! - 已有工作簿的已知替代路径 = 「读-改-重建」（[`io::read_book`] 提取
//!   → 本模块从零重建），实测保真边界 = 保 表序/值/公式文本/合并/
//!   超链接五面、**丢** 样式/文档元数据（FIDELITY.md v2 丢失矩阵
//!   定谳——calamine 无样式可观测面的结构性限制，非实现缺陷）。
//!   该路径含样式保真需求时为数据丢失风险面，调用方须显式告知用户，
//!   不得静默降级。
//!
//! ## 重放消毒契约（消费方责任边界）
//!
//! 写面信任调用方输入（与 rust_xlsxwriter 语义一致）。**重放来源 =
//! [`io::read_book`] 的不可信数据时**（读-改-重建路径），字符串值在
//! 写入前**必须**过 mdc-office 消毒链（`mdc_office::sanitize` 的
//! xml_control_chars / xml_entities 面——SANITIZE_TESTS.md §8 联动
//! 纪律）。本 crate 不依赖 mdc-office（保持当前 crate 图与三域纪律），
//! 消毒由消费方在读写两侧之间显式接线。
//!
//! XML 级结构性安全（引擎内建，一手核验 0.99.1 xmlwriter.rs）：
//! 控制字符 `\x00`-`\x1F`（`\t`/`\n` 除外）按 Excel 约定转义为
//! `_xHHHH_`；字面 `_xHHHH_` 形态加前导转义（`_x0000_` →
//! `_x005F_x0000_`）——与 mdc-office 消毒面 X-03 同一约定，写产物
//! 恒为合法 XML 1.0（往返测试钉定，见 tests/write_roundtrip.rs）。
//!
//! ## 公式语义（读侧对偶锚点 = FIDELITY.md §1 读回行为登记）
//!
//! - [`CellValue::Formula`] 的 `text` 接受带或不带 `=` 前缀（内部
//!   规范化为引擎要求的带 `=` 形态）；`_xlfn.` 前缀透传（calamine
//!   读回同形态，往返一致）。
//! - `cached` = None 时读回 `Float(0.0)`（与真值 0 不可区分——读侧
//!   两义性在档，FIDELITY §1）；需要缓存值可判别时显式给
//!   [`FormulaCache`]。
//! - 日期/时间值以序列数（[`CellValue::Number`]）写入——无 numFmt
//!   时无日历显示形态（显示层语义，超出写面宣称）。
//!
//! - **流式面目标条件化（t19，2026-10-14）**：[`WorkbookWriter::
//!   add_sheet_streaming`] 仅在非 wasm32 目标存在——引擎流式背书 =
//!   tempfile（`constant_memory` 特性下 `Worksheet::new()` 对每张表
//!   急切 `tempfile_in(temp_dir())`，worksheet.rs:1742），wasm32 无
//!   文件系统 → 该面在 wasm32 结构性不可用（API 缺席，非降级；
//!   Cargo.toml t19 注记 + wasm32 消费方即 mdcore-wasm 仅用非流式面）。
//!
//! ## 不支持面登记（诚实边界，未实测不宣称）
//!
//! - 完整样式模型（条件格式/主题色/富文本内联样式——style_probe
//!   9/9 仅覆盖 bold/字色/填充/thin 边框/自定义 numFmt/居中对齐）；
//! - 数值型合并区左上格（引擎 merge_range 仅字符串面——数值合并区
//!   以文本形态落盘，类型扁平化如实登记）；
//! - 图表/透视表/图片/数据验证/冻结窗格/列宽行高（引擎能力在，本
//!   包装面未实测不宣称，按需增量）。
//!
//! 证据链：tests/write_roundtrip.rs（create → save → reopen → verify
//! 全环）+ 本模块内联单测（验证/纪律/错误面）。

use rust_xlsxwriter::Workbook;

// ---------------------------------------------------------------------------
// 引擎边界常量（公开 = 写面契约的一部分）
// ---------------------------------------------------------------------------

/// 最大行号（0 基，含）——Excel 工作表物理边界 2^20 行。
pub const MAX_ROW: u32 = 1_048_575;
/// 最大列号（0 基，含）——Excel 工作表物理边界 2^14 列。
pub const MAX_COL: u16 = 16_383;
/// 工作簿表数上限（r4-t2 资源面）：①fd 面——引擎 `constant_memory`
/// 特性下 `Worksheet::new()` 对**每张表**（含非流式——t19 核验
/// worksheet.rs:1742）急切 tempfile → fd 消耗 = 表数；**b 轮实测校准
/// （2026-10-17）**：a 轮原值 512 的「进程其余 fd 需求 ≤ 数十」假设
/// 被证伪——单簿 512 过（隔离 0.39s），但同进程两簿并发即达
/// 1024 fd 软限 → EMFILE（code 24，TooManyOpenFiles）；本套件自身
/// 门禁即两个 max-scale 写测试并发形态。256 使双簿并发 = 512 fd +
/// 进程开销 ≈ 560，对软限 1024 余量 1.8×；②自产文件须过自身读面
/// 条目数上限（io::LIMIT_ENTRY_COUNT = 10,000）：每表 ≥ 1 部件 +
/// ~8 固定部件，256 表 ≈ 264 条目安全在内；真实工作簿百表级以内
/// （读面 1,000 为 C1 大档能力面，写面独立限不镜像）。
pub const MAX_SHEETS: usize = 256;
/// 全簿单元格累计上限（r4-t2 资源面）：非流式引擎全内存模型 Vec
/// 与流式 tempfile 尺寸的统一界（引擎无内建上限——一手核验）；
/// 值 = T3 写面探针 100 万格实测面（constant_memory 972ms；非流式
/// 基准见 examples/bench_limits.rs），且 < 2^20 保证本限先于
/// mdcore-wasm 导出面 MAX_ROW 防御截断（静默降级）触发——资源限
/// typed 拒绝优先于静默截断。
pub const MAX_CELLS: u64 = 1_000_000;
/// 单元格文本字符上限（r4-t2 格式面）：Excel 物理限 32767
///（引擎 worksheet.rs MAX_STRING_LEN 同值内建于 write_string 路径；
/// 公式缓存文本面引擎不检——包装层统一，含超链接显示文本）。
pub const MAX_CELL_TEXT: usize = 32_767;
/// 超链接 URL 字符上限（r4-t2 格式面）：**b 轮实测校准
/// （2026-10-17）**——a 轮原设计「url 同 32767」被证伪：引擎
/// write_url 路径**有**自己的限（url.rs:13 `MAX_URL_LEN` = 2_080
///，Excel 规格限——探针实测 2080 过/2081 `MaxUrlLengthExceeded`），
/// a 轮的 32767 检查在 2081-32767 区间永不触发（引擎先拒、落
/// Engine 类）。本常量镜像引擎真值，包装层 typed InvalidInput 前移
///（探针：恰 2080 过 / 2081 拒）。
pub const MAX_URL_TEXT: usize = 2_080;

/// 超链接 tooltip 文本上限 = 引擎限镜像（rust_xlsxwriter
/// `MAX_PARAMETER_LEN = 255`——worksheet.rs:1417，Excel 鼠标悬停
/// 提示限；引擎 initialize() 对超限抛 ParameterError，探针 255/256
/// 钉定，typed 前移不落 Engine 类；d1e-t2 补检）。
pub const MAX_TOOLTIP_TEXT: usize = 255;
/// 公式文本字符上限（r4-t2 格式面）：Excel 规格限 8192
///（引擎不检——包装层 typed 前移）。
pub const MAX_FORMULA_TEXT: usize = 8_192;

// ---------------------------------------------------------------------------
// 统一错误面（写侧二分：输入可修 / 引擎失败）
// ---------------------------------------------------------------------------

/// 写路径统一错误三分：格式验证失败（调用方可修输入重试，
/// [`WriteError::InvalidInput`]）/ 资源上限拒绝（[`WriteError::Limit`]
/// ——r4-t2，安全类，与格式错误可区分）/ 引擎失败
/// （[`WriteError::Engine`]）。
#[derive(Debug)]
pub enum WriteError {
    /// 输入验证失败（表名/坐标/次序/流式纪律/文本格式限）——typed，
    /// 可修输入重试。
    InvalidInput(String),
    /// 写引擎失败（IO 错误/URL 类型未知/引擎内部约束），消息含引擎
    /// 原始错误文本（诊断用，不承诺跨版本稳定）。
    Engine(String),
    /// 资源上限拒绝（r4-t2：表数/全簿单元格累计——安全类，与格式
    /// 错误可区分；消费方可修规模重试）。
    Limit(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            WriteError::Engine(m) => write!(f, "engine error: {m}"),
            WriteError::Limit(m) => write!(f, "resource limit: {m}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<rust_xlsxwriter::XlsxError> for WriteError {
    fn from(e: rust_xlsxwriter::XlsxError) -> Self {
        WriteError::Engine(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// 写侧值模型（与读侧 calamine::Data 对偶）
// ---------------------------------------------------------------------------

/// 公式缓存值（读侧 Data 的可判别重放形态——fidelity data_to_formula_
/// result 同构）。
#[derive(Debug, Clone, PartialEq)]
pub enum FormulaCache {
    Number(f64),
    Text(String),
    Boolean(bool),
}

impl FormulaCache {
    /// 引擎 set_formula_result 的字符串协议（数值用 Rust 默认格式化，
    /// "4.5"/"4"——读回 Float 同值）。
    fn as_result_string(&self) -> String {
        match self {
            FormulaCache::Number(v) => format!("{v}"),
            FormulaCache::Text(s) => s.clone(),
            FormulaCache::Boolean(b) => format!("{b}"),
        }
    }
}

/// 写侧单元格值。
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Text(String),
    Number(f64),
    Boolean(bool),
    /// 公式格：`text` 接受带/不带 `=` 前缀（内部规范化）；`cached`
    /// 可选缓存值（None → 读回 Float(0.0)，两义性在档见模块文档）。
    Formula {
        text: String,
        cached: Option<FormulaCache>,
    },
}

// ---------------------------------------------------------------------------
// 样式模型（style_probe 9/9 已验证面的产品化——超面不宣称）
// ---------------------------------------------------------------------------

/// 边框（验证面 = thin）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Border {
    #[default]
    None,
    Thin,
}

/// 数字格式（验证面 = ¥ 货币 + 一位小数百分比 + 自定义格式码）。
#[derive(Debug, Clone, PartialEq)]
pub enum NumFmt {
    /// `¥#,##0.00`（style_probe 验证码）。
    CurrencyYen,
    /// `0.0%`（style_probe 验证码）。
    PercentOneDecimal,
    /// 自定义格式码（透传引擎——正确性由调用方负责，验证面外）。
    Custom(String),
}

impl NumFmt {
    fn format_code(&self) -> String {
        match self {
            NumFmt::CurrencyYen => "¥#,##0.00".to_string(),
            NumFmt::PercentOneDecimal => "0.0%".to_string(),
            NumFmt::Custom(code) => code.clone(),
        }
    }
}

/// 对齐（验证面 = 水平居中）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    None,
    Center,
}

/// 单元格样式（style_probe 9/9 验证面的产品化模型；全部字段可选，
/// [`Default`] = 无样式）。颜色 = `#RRGGBB` 形态（与 corpus.rs 验证
/// 形态一致）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellStyle {
    pub bold: bool,
    pub font_color: Option<String>,
    pub fill_color: Option<String>,
    pub border: Border,
    pub num_fmt: Option<NumFmt>,
    pub align: Align,
}

/// 样式模型 → 引擎 Format（条件格式 dxf 面与单元格样式面共用——
/// pub(crate)：xlsx_condfmt 翻译面同源单点；引擎类型不出公开签名）。
pub(crate) fn style_to_format(style: &CellStyle) -> rust_xlsxwriter::Format {
    let mut fmt = rust_xlsxwriter::Format::new();
    if style.bold {
        fmt = fmt.set_bold();
    }
    if let Some(color) = &style.font_color {
        fmt = fmt.set_font_color(color.as_str());
    }
    if let Some(color) = &style.fill_color {
        fmt = fmt.set_background_color(color.as_str());
    }
    if style.border == Border::Thin {
        fmt = fmt.set_border(rust_xlsxwriter::FormatBorder::Thin);
    }
    if let Some(num_fmt) = &style.num_fmt {
        fmt = fmt.set_num_format(num_fmt.format_code());
    }
    if style.align == Align::Center {
        fmt = fmt.set_align(rust_xlsxwriter::FormatAlign::Center);
    }
    fmt
}

// ---------------------------------------------------------------------------
// 超链接规格（五形态——与读侧 Hyperlink 四字段对偶，FIDELITY §1）
// ---------------------------------------------------------------------------

/// 超链接规格。五已验证形态的构造方式：
/// ①外链纯形态 = `url` + `display_text: None`（值面 = URL）；
/// ②外链自定义文本 = `url` + `Some(text)`；
/// ③内部链接 = `url = "internal:Sheet!A1"`；
/// ④tooltip 面 = 任意形态 + `tooltip`；
/// ⑤file 子锚 = `url = "file:///path#Sheet1!A1"`（引擎按 `#` 拆分
/// target/anchor）。
#[derive(Debug, Clone, PartialEq)]
pub struct HyperlinkSpec {
    pub url: String,
    pub display_text: Option<String>,
    pub tooltip: Option<String>,
}

// ---------------------------------------------------------------------------
// 工作簿写器
// ---------------------------------------------------------------------------

/// 新建工作簿写器（**仅新建语义**——不是已有工作簿编辑器，见模块
/// 文档「新建 vs 编辑边界」）。
pub struct WorkbookWriter {
    wb: Workbook,
    sheet_names: Vec<String>,
    /// 全簿已写单元格累计（r4-t2 资源面——含合并范围填充格；与
    /// [`MAX_CELLS`] 对应的非流式内存面与流式 tempfile 面统一界）。
    cells_written: u64,
}

impl std::fmt::Debug for WorkbookWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 引擎句柄不进 Debug（引擎内部结构非公开契约）。
        f.debug_struct("WorkbookWriter")
            .field("sheets", &self.sheet_names)
            .field("cells_written", &self.cells_written)
            .finish()
    }
}

impl Default for WorkbookWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkbookWriter {
    pub fn new() -> Self {
        Self {
            wb: Workbook::new(),
            sheet_names: Vec::new(),
            cells_written: 0,
        }
    }

    /// 加普通表（全内存——随机行列写）。
    pub fn add_sheet(&mut self, name: &str) -> Result<SheetWriter<'_>, WriteError> {
        self.add_sheet_inner(name, false)
    }

    /// 加流式表（constant_memory——行序前向写纪律由包装层 typed 强制；
    /// 大表场景，T3 探针 100 万格量级验证）。同一工作簿可混用两种表。
    ///
    /// **仅非 wasm32 目标**（t19）：引擎流式背书 = tempfile
    /// （`constant_memory` 特性使 `Worksheet::new()` 急切建临时文件
    /// ——wasm32 无文件系统，结构性不可用，见模块文档）。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_sheet_streaming(&mut self, name: &str) -> Result<SheetWriter<'_>, WriteError> {
        self.add_sheet_inner(name, true)
    }

    fn add_sheet_inner(
        &mut self,
        name: &str,
        streaming: bool,
    ) -> Result<SheetWriter<'_>, WriteError> {
        validate_sheet_name(name, &self.sheet_names)?;
        // r4-t2 表数资源限（typed Limit——先于引擎建表/建 tempfile）。
        if self.sheet_names.len() >= MAX_SHEETS {
            return Err(WriteError::Limit(format!(
                "sheet count over limit {MAX_SHEETS}"
            )));
        }
        // wasm32：流式面结构性不可用（引擎 tempfile 背书，模块文档）——
        // add_sheet_streaming 已目标条件化缺席，此分支恒 false。
        #[cfg(target_arch = "wasm32")]
        let _ = streaming;
        #[cfg(not(target_arch = "wasm32"))]
        let ws = if streaming {
            self.wb.add_worksheet_with_constant_memory()
        } else {
            self.wb.add_worksheet()
        };
        #[cfg(target_arch = "wasm32")]
        let ws = self.wb.add_worksheet();
        ws.set_name(name)
            .map_err(|e| WriteError::Engine(format!("set_name: {e}")))?;
        self.sheet_names.push(name.to_string());
        Ok(SheetWriter {
            ws,
            streaming,
            high_water_row: 0,
            wrote_any: false,
            cells: &mut self.cells_written,
        })
    }

    /// 保存到文件路径。
    pub fn save_to_path(&mut self, path: &str) -> Result<(), WriteError> {
        self.wb
            .save(path)
            .map_err(|e| WriteError::Engine(format!("save: {e}")))
    }

    /// 保存到内存缓冲（无临时文件——小档/测试/网络面消费）。
    pub fn save_to_vec(&mut self) -> Result<Vec<u8>, WriteError> {
        self.wb
            .save_to_buffer()
            .map_err(|e| WriteError::Engine(format!("save_to_buffer: {e}")))
    }
}

/// 表名规则（镜像引擎 set_name 文档规则——typed 前移，契约独立于
/// 引擎版本）：非空 / ≤31 字符 / 禁 `[ ] : * ? / \` / 禁首尾单引号 /
/// 禁保留名 History（大小写不敏感）/ 禁重复。
fn validate_sheet_name(name: &str, existing: &[String]) -> Result<(), WriteError> {
    if name.is_empty() {
        return Err(WriteError::InvalidInput(
            "sheet name cannot be blank".to_string(),
        ));
    }
    let char_count = name.chars().count();
    if char_count > 31 {
        return Err(WriteError::InvalidInput(format!(
            "sheet name exceeds 31 characters ({char_count})"
        )));
    }
    if name.contains(['[', ']', ':', '*', '?', '/', '\\']) {
        return Err(WriteError::InvalidInput(
            "sheet name cannot contain any of [ ] : * ? / \\".to_string(),
        ));
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return Err(WriteError::InvalidInput(
            "sheet name cannot start or end with an apostrophe".to_string(),
        ));
    }
    if name.eq_ignore_ascii_case("history") {
        return Err(WriteError::InvalidInput(
            "sheet name 'History' is reserved by Excel".to_string(),
        ));
    }
    if existing.iter().any(|n| n == name) {
        return Err(WriteError::InvalidInput(format!(
            "duplicate sheet name: {name}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 表写器（引擎句柄包装 + 纪律强制）
// ---------------------------------------------------------------------------

/// 单表写器（由 [`WorkbookWriter::add_sheet`] 取得，借用期独占）。
pub struct SheetWriter<'a> {
    ws: &'a mut rust_xlsxwriter::Worksheet,
    streaming: bool,
    /// 流式纪律水位：已写过的最大行号（含）。回退写（row < 水位）
    /// 在流式模式下 = 引擎静默丢弃路径 → 包装层 typed 拒绝。
    high_water_row: u32,
    wrote_any: bool,
    /// 全簿单元格累计（r4-t2——借用工作簿计数器：非流式/流式两模式
    /// 同界；合并范围按面积计——引擎对范围内格全部物化，一手核验
    /// worksheet.rs merge_range）。
    cells: &'a mut u64,
}

impl std::fmt::Debug for SheetWriter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SheetWriter")
            .field("streaming", &self.streaming)
            .field("high_water_row", &self.high_water_row)
            .field("wrote_any", &self.wrote_any)
            .finish()
    }
}

/// 格式面长度检查（r4-t2——typed InvalidInput，可修输入重试）。
fn check_text_len(len: usize, limit: usize, what: &str) -> Result<(), WriteError> {
    if len > limit {
        return Err(WriteError::InvalidInput(format!(
            "{what} exceeds {limit} characters ({len})"
        )));
    }
    Ok(())
}

/// 资源面单元格预算检查（r4-t2——typed Limit；增量制：本次写入前
/// 已达上限即拒，恰好 = 上限的写入通过）。
fn check_cell_budget(cells: u64, increment: u64) -> Result<(), WriteError> {
    if cells.saturating_add(increment) > MAX_CELLS {
        return Err(WriteError::Limit(format!(
            "total cell count over limit {MAX_CELLS}"
        )));
    }
    Ok(())
}

impl SheetWriter<'_> {
    /// 坐标预验证（typed InvalidInput；越界值不触引擎）。
    fn check_coord(&self, row: u32, col: u16) -> Result<(), WriteError> {
        if row > MAX_ROW {
            return Err(WriteError::InvalidInput(format!(
                "row {row} exceeds max {MAX_ROW}"
            )));
        }
        if col > MAX_COL {
            return Err(WriteError::InvalidInput(format!(
                "col {col} exceeds max {MAX_COL}"
            )));
        }
        if self.streaming && self.wrote_any && row < self.high_water_row {
            return Err(WriteError::InvalidInput(format!(
                "streaming sheet requires forward row order: row {row} is behind high-water row {} (already flushed to disk — the engine would silently drop this write)",
                self.high_water_row
            )));
        }
        Ok(())
    }

    fn advance_water(&mut self, row: u32) {
        if !self.wrote_any || row > self.high_water_row {
            self.high_water_row = row;
        }
        self.wrote_any = true;
    }

    /// 写单元格（值 + 可选样式）。r4-t2：格式面长度限（单元格文本/
    /// 公式文本/缓存文本——typed InvalidInput）与资源面全簿单元格
    /// 累计限（typed Limit）均在引擎调用之前。
    pub fn write(
        &mut self,
        row: u32,
        col: u16,
        value: CellValue,
        style: Option<&CellStyle>,
    ) -> Result<(), WriteError> {
        self.check_coord(row, col)?;
        check_cell_budget(*self.cells, 1)?;
        match &value {
            CellValue::Text(s) => check_text_len(s.chars().count(), MAX_CELL_TEXT, "cell text")?,
            CellValue::Formula { text, cached } => {
                check_text_len(text.chars().count(), MAX_FORMULA_TEXT, "formula text")?;
                if let Some(FormulaCache::Text(s)) = cached {
                    check_text_len(s.chars().count(), MAX_CELL_TEXT, "formula cached text")?;
                }
            }
            _ => {}
        }
        match value {
            CellValue::Text(s) => {
                if let Some(st) = style {
                    self.ws
                        .write_string_with_format(row, col, s, &style_to_format(st))?;
                } else {
                    self.ws.write_string(row, col, s)?;
                }
            }
            CellValue::Number(v) => {
                if let Some(st) = style {
                    self.ws
                        .write_number_with_format(row, col, v, &style_to_format(st))?;
                } else {
                    self.ws.write_number(row, col, v)?;
                }
            }
            CellValue::Boolean(b) => {
                if let Some(st) = style {
                    self.ws
                        .write_boolean_with_format(row, col, b, &style_to_format(st))?;
                } else {
                    self.ws.write_boolean(row, col, b)?;
                }
            }
            CellValue::Formula { text, cached } => {
                // 前缀规范化：引擎要求带 = 形态；_xlfn. 透传（读回
                // 同形态——FIDELITY §1 往返一致）。
                let formula = if text.starts_with('=') {
                    text
                } else {
                    format!("={text}")
                };
                if let Some(st) = style {
                    self.ws.write_formula_with_format(
                        row,
                        col,
                        formula.as_str(),
                        &style_to_format(st),
                    )?;
                } else {
                    self.ws.write_formula(row, col, formula.as_str())?;
                }
                if let Some(cache) = cached {
                    // 引擎 0.99.1：set_formula_result 返回 &mut Worksheet
                    //（非 Result）。流式模式下引擎按 current_row 查找——
                    // 本调用紧跟同格 write_formula（水位已推进到该行），
                    // 行上下文恒一致。
                    let result = cache.as_result_string();
                    self.ws.set_formula_result(row, col, result);
                }
            }
        }
        *self.cells += 1;
        self.advance_water(row);
        Ok(())
    }

    /// 行便捷写：自列 0 起顺序写一行（无样式——表格生成主路径）。
    pub fn write_row(&mut self, row: u32, cells: &[CellValue]) -> Result<(), WriteError> {
        for (col, value) in cells.iter().enumerate() {
            let col =
                u16::try_from(col).map_err(|_| WriteError::InvalidInput("row too wide".into()))?;
            self.write(row, col, value.clone(), None)?;
        }
        Ok(())
    }

    /// 条件格式（issue #113 任务 2——翻译面 [`crate::xlsx_condfmt`]，
    /// 引擎类型零泄漏）。`range` A1 记法 `"A1:C10"` / 单格 / `$`
    /// 前缀容忍；形态与能力边界见 [`crate::xlsx_condfmt`] 模块文档。
    pub fn add_cond_format(
        &mut self,
        range: &str,
        cf: crate::xlsx_condfmt::CondFormat,
    ) -> Result<(), WriteError> {
        let (r0, c0, r1, c1) = crate::xlsx_condfmt::parse_range(range)?;
        crate::xlsx_condfmt::apply(self.ws, r0, c0, r1, c1, &cf)
    }

    /// 合并范围（左上格写 `text` 值 + 可选样式；引擎语义 = 范围内
    /// 其余格为带格式空格）。**数值型合并左上格不支持**（引擎
    /// merge_range 仅字符串面——数值以文本形态落盘的类型扁平化已
    /// 在模块文档登记，故本 API 直接收字符串，不提供数值重载）。
    pub fn merge(
        &mut self,
        first_row: u32,
        first_col: u16,
        last_row: u32,
        last_col: u16,
        text: &str,
        style: Option<&CellStyle>,
    ) -> Result<(), WriteError> {
        if first_row > last_row || first_col > last_col {
            return Err(WriteError::InvalidInput(
                "merge range order: first must be <= last".to_string(),
            ));
        }
        // 流式纪律：合并首行不得回退（范围内 blank 填充同样受引擎
        // 静默丢弃影响）。
        self.check_coord(first_row, first_col)?;
        self.check_coord(last_row, last_col)?;
        // r4-t2：合并范围面积计入全簿单元格预算（引擎对范围内全部
        // 格物化——write_blank 逐格，一手核验 worksheet.rs merge_range；
        // MAX_ROW×MAX_COL 量级面积在此 typed 拒绝，不进引擎）+
        // 左上格文本长度统一 MAX_CELL_TEXT（同 write() 单格文本面）。
        check_text_len(text.chars().count(), MAX_CELL_TEXT, "merged cell text")?;
        let area = (last_row - first_row + 1) as u64 * (last_col - first_col + 1) as u64;
        check_cell_budget(*self.cells, area)?;
        let fmt = style.map(style_to_format).unwrap_or_default();
        self.ws
            .merge_range(first_row, first_col, last_row, last_col, text, &fmt)?;
        *self.cells += area;
        self.advance_water(first_row);
        Ok(())
    }

    /// 写超链接（五形态见 [`HyperlinkSpec`]；display_text = None 时
    /// 值面 = URL——write_url 纯链接语义）。r4-t2：url = 引擎限镜像
    /// [`MAX_URL_TEXT`]（2080——引擎 url.rs `MAX_URL_LEN` 同值，探针
    /// 2080/2081 钉定，typed 前移不落 Engine 类）；显示文本统一
    /// [`MAX_CELL_TEXT`]（引擎 write_string 路径同值）。tooltip =
    /// 引擎限镜像 [`MAX_TOOLTIP_TEXT`]（255——引擎 url.rs initialize()
    /// 对超限抛 ParameterError；d1e-t2 补检：超链接面第三字符串
    /// 字段补齐同族 typed 前移纪律）。
    pub fn write_hyperlink(
        &mut self,
        row: u32,
        col: u16,
        spec: HyperlinkSpec,
    ) -> Result<(), WriteError> {
        self.check_coord(row, col)?;
        check_cell_budget(*self.cells, 1)?;
        check_text_len(spec.url.chars().count(), MAX_URL_TEXT, "hyperlink url")?;
        let text = spec.display_text.clone();
        if let Some(t) = &text {
            check_text_len(t.chars().count(), MAX_CELL_TEXT, "hyperlink display text")?;
        }
        if let Some(t) = &spec.tooltip {
            check_text_len(t.chars().count(), MAX_TOOLTIP_TEXT, "hyperlink tooltip")?;
        }
        let text = text.unwrap_or_else(|| spec.url.clone());
        let tooltip = spec.tooltip.unwrap_or_default();
        self.ws
            .write_url_with_options(row, col, spec.url.as_str(), text, tooltip, None)?;
        *self.cells += 1;
        self.advance_water(row);
        Ok(())
    }

    /// 图表挂载（issue #113 任务 3——翻译面 [`crate::xlsx_chart`]，
    /// 引擎类型零泄漏）。规格语义（系列拆分约定/EMU 尺寸/校验面）见
    /// [`crate::xlsx_chart`] 模块文档；两模式（流式/非流式）均可用
    /// （图表不触单元格写路径，不参与行序水位纪律）。
    pub fn add_chart(&mut self, spec: crate::xlsx_chart::ChartSpec) -> Result<(), WriteError> {
        crate::xlsx_chart::apply(self.ws, &spec)
    }
}

// ---------------------------------------------------------------------------
// 单测（验证面 / 纪律面 / 错误面——全环往返见 tests/write_roundtrip.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_name_rules_enforced() {
        let mut w = WorkbookWriter::new();
        assert!(w.add_sheet("Data").is_ok());
        for bad in [
            "",
            "a-very-long-sheet-name-over-31-characters",
            "bad[name",
            "bad:name",
            "bad*name",
            "bad?name",
            "bad/name",
            "bad\\name",
            "'quoted",
            "quoted'",
            "history",
            "History",
            "Data", // 重复
        ] {
            match w.add_sheet(bad) {
                Err(WriteError::InvalidInput(_)) => {}
                other => panic!("expected InvalidInput for {bad:?}, got {other:?}"),
            }
        }
        // 31 字符（含）合法。
        assert!(w.add_sheet(&"x".repeat(31)).is_ok());
    }

    #[test]
    fn coordinate_bounds_enforced() {
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("S").unwrap();
            assert!(s
                .write(MAX_ROW, MAX_COL, CellValue::Number(1.0), None)
                .is_ok());
            match s.write(MAX_ROW + 1, 0, CellValue::Number(1.0), None) {
                Err(WriteError::InvalidInput(m)) => assert!(m.contains("row")),
                other => panic!("expected row bound reject, got {other:?}"),
            }
            match s.write(0, MAX_COL + 1, CellValue::Number(1.0), None) {
                Err(WriteError::InvalidInput(m)) => assert!(m.contains("col")),
                other => panic!("expected col bound reject, got {other:?}"),
            }
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn streaming_backward_write_typed_rejected() {
        // 引擎静默丢弃路径（worksheet.rs insert_cell：row < current_row
        // → eprintln + return）→ 包装层 typed 拦截，绝不静默丢数据。
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet_streaming("S").unwrap();
        s.write(2, 0, CellValue::Number(1.0), None).unwrap();
        s.write(2, 1, CellValue::Number(2.0), None).unwrap(); // 同行可
        s.write(3, 0, CellValue::Number(3.0), None).unwrap(); // 前向可
        match s.write(1, 0, CellValue::Number(4.0), None) {
            Err(WriteError::InvalidInput(m)) => {
                assert!(m.contains("forward row order"), "msg: {m}");
            }
            other => panic!("expected streaming discipline reject, got {other:?}"),
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn streaming_merge_backward_rejected() {
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet_streaming("S").unwrap();
        s.write(5, 0, CellValue::Number(1.0), None).unwrap();
        match s.merge(1, 0, 1, 2, "m", None) {
            Err(WriteError::InvalidInput(_)) => {}
            other => panic!("expected merge discipline reject, got {other:?}"),
        }
    }

    #[test]
    fn merge_order_and_single_cell() {
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet("S").unwrap();
        match s.merge(2, 2, 0, 0, "m", None) {
            Err(WriteError::InvalidInput(m)) => assert!(m.contains("order")),
            other => panic!("expected order reject, got {other:?}"),
        }
        // 单格合并 = 引擎错误（Excel 不允许）——落 Engine 面。
        match s.merge(0, 0, 0, 0, "m", None) {
            Err(WriteError::Engine(_)) => {}
            other => panic!("expected engine error for single-cell merge, got {other:?}"),
        }
        assert!(s.merge(0, 0, 0, 2, "表头", None).is_ok());
    }

    #[test]
    fn formula_prefix_normalization() {
        // 带/不带 = 前缀均可；引擎只接受带 = 形态。
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet("S").unwrap();
        assert!(s
            .write(
                0,
                0,
                CellValue::Formula {
                    text: "SUM(A1:A5)".to_string(),
                    cached: None
                },
                None
            )
            .is_ok());
        assert!(s
            .write(
                1,
                0,
                CellValue::Formula {
                    text: "=SUM(A1:A5)".to_string(),
                    cached: Some(FormulaCache::Number(15.0))
                },
                None
            )
            .is_ok());
    }

    #[test]
    fn cache_result_string_forms() {
        assert_eq!(FormulaCache::Number(4.0).as_result_string(), "4");
        assert_eq!(FormulaCache::Number(4.5).as_result_string(), "4.5");
        assert_eq!(
            FormulaCache::Text("是".to_string()).as_result_string(),
            "是"
        );
        assert_eq!(FormulaCache::Boolean(true).as_result_string(), "true");
    }

    #[test]
    fn numfmt_format_codes() {
        assert_eq!(NumFmt::CurrencyYen.format_code(), "¥#,##0.00");
        assert_eq!(NumFmt::PercentOneDecimal.format_code(), "0.0%");
        assert_eq!(
            NumFmt::Custom("0.00E+00".to_string()).format_code(),
            "0.00E+00"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn write_row_sequential() {
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet_streaming("S").unwrap();
        s.write_row(
            0,
            &[
                CellValue::Text("列一".into()),
                CellValue::Number(1.0),
                CellValue::Boolean(true),
            ],
        )
        .unwrap();
        s.write_row(1, &[CellValue::Number(2.0)]).unwrap();
        // write_row 不抬列水位约束（行级 API）——回退行仍拒绝。
        match s.write(0, 5, CellValue::Number(9.0), None) {
            Err(WriteError::InvalidInput(_)) => {}
            other => panic!("expected discipline reject, got {other:?}"),
        }
    }

    #[test]
    fn save_to_vec_produces_zip_magic() {
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("S").unwrap();
            s.write(0, 0, CellValue::Text("hello".into()), None)
                .unwrap();
        }
        let bytes = w.save_to_vec().unwrap();
        assert_eq!(&bytes[..2], b"PK", "产物应为 zip 容器");
        assert!(bytes.len() > 100);
    }

    // -----------------------------------------------------------------
    // r4-t2 资源/格式上限（表数 / 全簿格预算 / 文本长度）
    // -----------------------------------------------------------------

    /// W3 表数上限：恰 256 表在位可存；第 257 表 typed Limit（先于
    /// 引擎建表）。自产 256 表档 ≈ 264 zip 条目 < 读面
    /// LIMIT_ENTRY_COUNT 10,000（自产文件须过自身读面）。fd 面
    ///（引擎每表急切 tempfile，含非流式）的并发双簿安全余量见
    /// MAX_SHEETS 常量注记（b 轮 EMFILE 实测校准）。
    #[test]
    fn sheet_count_write_limit() {
        let mut w = WorkbookWriter::new();
        for i in 0..MAX_SHEETS {
            w.add_sheet(&format!("S{i}")).unwrap();
        }
        match w.add_sheet("S_extra") {
            Err(WriteError::Limit(m)) => {
                assert!(m.contains("sheet count over limit"), "{m}");
            }
            other => panic!("expected Limit at 257th sheet, got {other:?}"),
        }
        // 拒绝后簿仍完整可存（256 表合法档）。
        let bytes = w.save_to_vec().unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    /// W3 流式面对偶（fd 消耗的一手钉定）：256 流式表 = 每表急切
    /// tempfile（t19 核验 worksheet.rs:1742，constant_memory 特性下
    /// 非流式表同）→ 256 并发 fd；双簿并发（本套件门禁真实形态）
    /// 512 fd + 进程开销 ≈ 560 < 软限 1024（a 轮 512 双簿并发 EMFILE
    /// 证伪 → b 轮校准证据链）。wasm32 结构性缺席（t19 target-split，
    /// cfg 同步纪律）。
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn streaming_sheet_count_256_fd_face() {
        let mut w = WorkbookWriter::new();
        for i in 0..MAX_SHEETS {
            w.add_sheet_streaming(&format!("S{i}")).unwrap();
        }
        match w.add_sheet_streaming("S_extra") {
            Err(WriteError::Limit(_)) => {}
            other => panic!("expected Limit at 257th streaming sheet, got {other:?}"),
        }
        let p =
            std::env::temp_dir().join(format!("mdc_write_test_fd256_{}.xlsx", std::process::id()));
        w.save_to_path(&p.to_string_lossy()).unwrap();
        assert!(p.metadata().map(|m| m.len() > 100).unwrap_or(false));
        let _ = std::fs::remove_file(&p);
    }

    /// W2 全簿格预算（增量制：已达上限再写即拒；恰 = 限的写入通过）
    /// ——私有计数直置边界（真实 1M 格全链在 examples/bench_limits.rs
    /// 发布面基准）+ 小规模计数算术钉定（write 逐格 + merge 按面积）。
    #[test]
    fn cell_budget_limit_and_arithmetic() {
        // 边界：cells = MAX-1 → 写 1 格恰达限；再写 → typed Limit。
        let mut w = WorkbookWriter::new();
        w.cells_written = MAX_CELLS - 1;
        {
            let mut s = w.add_sheet("S").unwrap();
            assert!(s.write(0, 0, CellValue::Number(1.0), None).is_ok());
            match s.write(0, 1, CellValue::Number(2.0), None) {
                Err(WriteError::Limit(m)) => {
                    assert!(m.contains("total cell count over limit"), "{m}");
                }
                other => panic!("expected Limit at budget exhaustion, got {other:?}"),
            }
        }
        assert_eq!(w.cells_written, MAX_CELLS);
        // merge 面积预算：cells = MAX-4 → 2×2 恰达限；再 2×2 → Limit。
        let mut w2 = WorkbookWriter::new();
        w2.cells_written = MAX_CELLS - 4;
        {
            let mut s = w2.add_sheet("S").unwrap();
            assert!(s.merge(0, 0, 1, 1, "m", None).is_ok());
            match s.merge(2, 0, 3, 1, "m", None) {
                Err(WriteError::Limit(_)) => {}
                other => panic!("expected Limit for merge over budget, got {other:?}"),
            }
        }
        // 计数算术（小规模真实全链）：3 write + 1×6 merge = 9。
        let mut w3 = WorkbookWriter::new();
        {
            let mut s = w3.add_sheet("S").unwrap();
            s.write(0, 0, CellValue::Number(1.0), None).unwrap();
            s.write(0, 1, CellValue::Number(2.0), None).unwrap();
            s.write(0, 2, CellValue::Number(3.0), None).unwrap();
            s.merge(2, 0, 2, 5, "m", None).unwrap();
        }
        assert_eq!(w3.cells_written, 9);
    }

    /// W4/W5 格式面长度限（typed InvalidInput 先于引擎）：单格文本/
    /// 公式文本/公式缓存文本/合并左上格文本/超链接 url 与显示文本
    /// ——恰 = 限全过（引擎同值限 32767 chars；公式 8192 = Excel
    /// 规格），限 + 1 全拒且消息可诊断。
    #[test]
    fn text_and_formula_length_limits() {
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("S").unwrap();
            // 单格文本：恰 32767 过（引擎 write_string 同值）。
            assert!(s
                .write(0, 0, CellValue::Text("x".repeat(MAX_CELL_TEXT)), None)
                .is_ok());
            match s.write(0, 1, CellValue::Text("x".repeat(MAX_CELL_TEXT + 1)), None) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("cell text"), "{m}");
                }
                other => panic!("expected cell text reject, got {other:?}"),
            }
            // 公式文本：恰 8192 过 / 8193 拒。
            assert!(s
                .write(
                    1,
                    0,
                    CellValue::Formula {
                        text: "1+".to_string() + &"1".repeat(MAX_FORMULA_TEXT - 2),
                        cached: None,
                    },
                    None,
                )
                .is_ok());
            match s.write(
                1,
                1,
                CellValue::Formula {
                    text: "1+".to_string() + &"1".repeat(MAX_FORMULA_TEXT - 1),
                    cached: None,
                },
                None,
            ) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("formula text"), "{m}");
                }
                other => panic!("expected formula text reject, got {other:?}"),
            }
            // 公式缓存文本（引擎不检面——包装层统一）：恰/超。
            assert!(s
                .write(
                    2,
                    0,
                    CellValue::Formula {
                        text: "SUM(A1:A2)".into(),
                        cached: Some(FormulaCache::Text("v".repeat(MAX_CELL_TEXT))),
                    },
                    None,
                )
                .is_ok());
            match s.write(
                2,
                1,
                CellValue::Formula {
                    text: "SUM(A1:A2)".into(),
                    cached: Some(FormulaCache::Text("v".repeat(MAX_CELL_TEXT + 1))),
                },
                None,
            ) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("formula cached text"), "{m}");
                }
                other => panic!("expected cached text reject, got {other:?}"),
            }
            // 合并左上格文本（W4 补面——引擎 write_string 路径外的
            // 单格文本面）：恰 32767 过 / 32768 拒。
            assert!(s
                .merge(4, 0, 4, 1, &"m".repeat(MAX_CELL_TEXT), None)
                .is_ok());
            match s.merge(5, 0, 5, 1, &"m".repeat(MAX_CELL_TEXT + 1), None) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("merged cell text"), "{m}");
                }
                other => panic!("expected merged text reject, got {other:?}"),
            }
            // 超链接：url = 引擎限镜像 MAX_URL_TEXT（2080——探针
            // 钉定引擎 2080 过/2081 MaxUrlLengthExceeded，typed 前移）；
            // 显示文本统一 MAX_CELL_TEXT（引擎同值）。
            let url_prefix = "https://e.example/";
            assert!(s
                .write_hyperlink(
                    6,
                    0,
                    HyperlinkSpec {
                        url: "https://e.example/".to_string()
                            + &"a".repeat(MAX_URL_TEXT - url_prefix.len()),
                        display_text: None,
                        tooltip: None,
                    },
                )
                .is_ok());
            match s.write_hyperlink(
                7,
                0,
                HyperlinkSpec {
                    url: "https://e.example/".to_string()
                        + &"a".repeat(MAX_URL_TEXT - url_prefix.len() + 1),
                    display_text: None,
                    tooltip: None,
                },
            ) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("hyperlink url"), "{m}");
                }
                other => panic!("expected hyperlink url reject, got {other:?}"),
            }
            match s.write_hyperlink(
                8,
                0,
                HyperlinkSpec {
                    url: "https://e.example/".into(),
                    display_text: Some("d".repeat(MAX_CELL_TEXT + 1)),
                    tooltip: None,
                },
            ) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("hyperlink display text"), "{m}");
                }
                other => panic!("expected display text reject, got {other:?}"),
            }
            // tooltip（d1e-t2 补检——引擎 url.rs initialize() 对超限
            // 抛 ParameterError 的 typed 前移）：恰 255 过 / 256 拒。
            assert!(s
                .write_hyperlink(
                    9,
                    0,
                    HyperlinkSpec {
                        url: "https://e.example/".into(),
                        display_text: None,
                        tooltip: Some("t".repeat(MAX_TOOLTIP_TEXT)),
                    },
                )
                .is_ok());
            match s.write_hyperlink(
                10,
                0,
                HyperlinkSpec {
                    url: "https://e.example/".into(),
                    display_text: None,
                    tooltip: Some("t".repeat(MAX_TOOLTIP_TEXT + 1)),
                },
            ) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("hyperlink tooltip"), "{m}");
                }
                other => panic!("expected tooltip reject, got {other:?}"),
            }
        }
        // 限内写入簿仍可存（长度限不破坏合法档）。
        assert_eq!(&w.save_to_vec().unwrap()[..2], b"PK");
    }

    /// write_row 列宽边界（行级 API 的列域漏斗）：恰 16384 列
    ///（col 0..=MAX_COL=16383——引擎列域镜像）过；16385 列 →
    /// typed InvalidInput（check_coord 列域检查先于引擎调用）。
    /// d1e-t2 补检（此前该入口无边界钉定）。附注：write_row 的
    /// u16::try_from 宽度检查为**不可达防御**（u16 域 65535 > 列域
    /// 16383，check_coord 恒先触发）——防御纵深保留，非行为面。
    #[test]
    fn write_row_column_width_boundary() {
        let mut w = WorkbookWriter::new();
        {
            // 恰 16384 列（col 0..=16383——MAX_COL 域内满宽行）。
            let row: Vec<CellValue> = (0..=MAX_COL).map(|i| CellValue::Number(i as f64)).collect();
            assert_eq!(row.len(), 16_384);
            let mut s = w.add_sheet("S").unwrap();
            s.write_row(0, &row)
                .expect("16384 列恰过（MAX_COL 域满宽）");
            // 16385 列 → 第 16385 格（col 索引 16384）超列域拒绝。
            let wide: Vec<CellValue> = (0..=MAX_COL)
                .map(|_| CellValue::Number(0.0))
                .chain(std::iter::once(CellValue::Number(0.0)))
                .collect();
            assert_eq!(wide.len(), 16_385);
            match s.write_row(1, &wide) {
                Err(WriteError::InvalidInput(m)) => {
                    assert!(m.contains("col 16384 exceeds max 16383"), "{m}");
                }
                other => panic!("expected col-domain reject, got {other:?}"),
            }
        }
        assert_eq!(&w.save_to_vec().unwrap()[..2], b"PK");
    }
}
