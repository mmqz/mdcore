//! 面 1：CSV 出口公式首字符消毒（office-d §5.1）。
//!
//! **威胁面**：以 `=` `+` `-` `@` 首字符开头的单元格值在目标表格应用
//! 打开/粘贴 CSV 时被重解释为公式——DDE 命令注入（`=cmd|...`）、
//! 超链接钓鱼（`=HYPERLINK(...)`）、算式改写（`+1+1`）等。生态写库
//! 对此零消毒（其一主动以 `=` 前缀重建公式文本——公式注入是其特性
//! 而非被防的面，一手代码裁决）；语法层 quoting 只防分隔符/引号/换行
//! 歧义，不防公式注入。**消毒责任在管线出口层。**
//!
//! **职位（豁免注记）**：仅 CSV/TSV 出口。xlsx 写路线豁免——目标应用
//! 对包内 t="s" 共享字符串不重解释为公式（公式注入只在 CSV/TSV 粘贴
//! 与打开时发生，office-d §5.1 裁决 2）。误用于 xlsx 出口 = 规格外
//! 行为（无害但无意义）。
//!
//! **触发集口径**：首字符 ∈ {`=` `+` `-` `@`}（office-d §5.1 口径）。
//! 前导空白不剥离、不扩集（OWASP 扩展口径含 \t/\r 前导——调用方按需
//! 自行预处理，本模块保持审计规格最小面）。
//!
//! ```
//! use mdc_office::sanitize::csv_formula::{guard_csv_cell, CsvFormulaMode};
//!
//! // 默认模式：前置单引号（业界惯例——目标应用按文本展示，无公式语义）
//! let out = guard_csv_cell("=HYPERLINK(\"http://evil\",\"click\")", CsvFormulaMode::PrefixApostrophe).unwrap();
//! assert_eq!(out, "'=HYPERLINK(\"http://evil\",\"click\")");
//!
//! // 正常值原样放行（含安全首字符，如负数之外的普通文本）
//! assert_eq!(guard_csv_cell("plain text", CsvFormulaMode::PrefixApostrophe).unwrap(), "plain text");
//!
//! // 拒写模式：typed reject（审计口径的可选策略——按目标应用策略拒写）
//! assert!(guard_csv_cell("@SUM(A1:A9)", CsvFormulaMode::Reject).is_err());
//! ```

/// 消毒模式（office-d §5.1 业界惯例三选项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsvFormulaMode {
    /// 前置 `'`（默认，业界惯例）：目标应用按文本展示，公式语义消除。
    PrefixApostrophe,
    /// 前置空格：公式语义消除（展示面多一前导空格）。
    PrefixSpace,
    /// 拒写（typed reject）：按目标应用策略拒绝该单元格值。
    Reject,
}

/// CSV 消毒错误（拒写模式的 typed reject 面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CsvGuardError {
    /// 首字符命中触发集且模式为拒写。携带触发字符与原值。
    FormulaPrefixRejected { ch: char, cell: String },
}

impl std::fmt::Display for CsvGuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CsvGuardError::FormulaPrefixRejected { ch, cell } => write!(
                f,
                "CSV 公式首字符消毒：拒写模式命中触发字符 {ch:?}（单元格值 {cell:?}）"
            ),
        }
    }
}

impl std::error::Error for CsvGuardError {}

/// 判定首字符是否命中公式触发集（`=` `+` `-` `@`；office-d §5.1 口径）。
pub fn is_formula_trigger(ch: char) -> bool {
    matches!(ch, '=' | '+' | '-' | '@')
}

/// CSV 出口单元格值消毒。
///
/// 触发集命中时按模式处置（前置 `'` / 前置空格 / typed reject）；
/// 未命中或空值原样放行。**作用于 CSV 序列化之前的原始单元格值**
/// （语法层 quoting 由调用方的 CSV 序列化器负责——两层职责分离）。
pub fn guard_csv_cell(value: &str, mode: CsvFormulaMode) -> Result<String, CsvGuardError> {
    let Some(ch) = value.chars().next() else {
        return Ok(value.to_string()); // 空值：无首字符，放行
    };
    if !is_formula_trigger(ch) {
        return Ok(value.to_string()); // 正常值：原样放行（零拷贝语义的字符串返回）
    }
    match mode {
        CsvFormulaMode::PrefixApostrophe => Ok(format!("'{value}")),
        CsvFormulaMode::PrefixSpace => Ok(format!(" {value}")),
        CsvFormulaMode::Reject => Err(CsvGuardError::FormulaPrefixRejected {
            ch,
            cell: value.to_string(),
        }),
    }
}
