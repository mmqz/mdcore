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

// ---------------------------------------------------------------------------
// 统一错误面（写侧二分：输入可修 / 引擎失败）
// ---------------------------------------------------------------------------

/// 写路径统一错误：输入验证失败（调用方可修输入重试）/ 引擎失败。
#[derive(Debug)]
pub enum WriteError {
    /// 输入验证失败（表名/坐标/次序/流式纪律）——typed，可修输入重试。
    InvalidInput(String),
    /// 写引擎失败（IO 错误/URL 类型未知/引擎内部约束），消息含引擎
    /// 原始错误文本（诊断用，不承诺跨版本稳定）。
    Engine(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            WriteError::Engine(m) => write!(f, "engine error: {m}"),
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

/// 样式模型 → 引擎 Format（私有翻译面——引擎类型不出公开签名）。
fn style_to_format(style: &CellStyle) -> rust_xlsxwriter::Format {
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
}

impl std::fmt::Debug for WorkbookWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 引擎句柄不进 Debug（引擎内部结构非公开契约）。
        f.debug_struct("WorkbookWriter")
            .field("sheets", &self.sheet_names)
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
        }
    }

    /// 加普通表（全内存——随机行列写）。
    pub fn add_sheet(&mut self, name: &str) -> Result<SheetWriter<'_>, WriteError> {
        self.add_sheet_inner(name, false)
    }

    /// 加流式表（constant_memory——行序前向写纪律由包装层 typed 强制；
    /// 大表场景，T3 探针 100 万格量级验证）。同一工作簿可混用两种表。
    pub fn add_sheet_streaming(&mut self, name: &str) -> Result<SheetWriter<'_>, WriteError> {
        self.add_sheet_inner(name, true)
    }

    fn add_sheet_inner(
        &mut self,
        name: &str,
        streaming: bool,
    ) -> Result<SheetWriter<'_>, WriteError> {
        validate_sheet_name(name, &self.sheet_names)?;
        let ws = if streaming {
            self.wb.add_worksheet_with_constant_memory()
        } else {
            self.wb.add_worksheet()
        };
        ws.set_name(name)
            .map_err(|e| WriteError::Engine(format!("set_name: {e}")))?;
        self.sheet_names.push(name.to_string());
        Ok(SheetWriter {
            ws,
            streaming,
            high_water_row: 0,
            wrote_any: false,
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

    /// 写单元格（值 + 可选样式）。
    pub fn write(
        &mut self,
        row: u32,
        col: u16,
        value: CellValue,
        style: Option<&CellStyle>,
    ) -> Result<(), WriteError> {
        self.check_coord(row, col)?;
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
        let fmt = style.map(style_to_format).unwrap_or_default();
        self.ws
            .merge_range(first_row, first_col, last_row, last_col, text, &fmt)?;
        self.advance_water(first_row);
        Ok(())
    }

    /// 写超链接（五形态见 [`HyperlinkSpec`]；display_text = None 时
    /// 值面 = URL——write_url 纯链接语义）。
    pub fn write_hyperlink(
        &mut self,
        row: u32,
        col: u16,
        spec: HyperlinkSpec,
    ) -> Result<(), WriteError> {
        self.check_coord(row, col)?;
        let text = spec.display_text.unwrap_or_else(|| spec.url.clone());
        let tooltip = spec.tooltip.unwrap_or_default();
        self.ws
            .write_url_with_options(row, col, spec.url.as_str(), text, tooltip, None)?;
        self.advance_water(row);
        Ok(())
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
}
