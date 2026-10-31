//! mdc-formula —— XLSX 公式计算引擎（S3 任务 1，#110）。
//!
//! # 定位
//!
//! workspace 第 16 成员（循 mdc-pdf / mdc-office-io 引擎侧先例）：
//! 公式解析/求值/重算的领域引擎。**公开面引擎类型零泄漏**——所有
//! 签名只含本 crate 类型（`Value`/`CellContent`/`CellRef`/
//! `SheetInput`/`FormulaError`/[`FormulaEngine`]），ironcalc 类型
//! 不出现在公开 API。对 mdcore 零编译依赖（三域纪律）。
//!
//! # 引擎选型（#110 调研定谳，2026-10-31）
//!
//! [`ironcalc_base`] `=0.9.1`（MIT OR Apache-2.0）：纯计算引擎零
//! 文件 IO、~470 函数（#110 必备 36 个全在）、`#CIRC!` 循环检测、
//! 13 错误码结构化枚举、求值深度/栈限额（DoS 防护）。版本**精确
//! 钉定**（pre-1.0 纪律）；升版守门 = `tests/functions_vs_excel.rs`
//! 全量对照复跑。
//!
//! # 能力边界（不宣称面显式登记）
//!
//! - **R1C1 记法**：不支持（xlsx 存储面恒 A1）；
//! - **定义名/表结构化引用**：依赖提取
//!   （[`FormulaEngine::dependencies_at`]）不覆盖
//!   `DefinedNameKind`/`TableNameKind` 节点（重建输入面无 defined
//!   names——office-io 读面五 API 不含；求值面经引擎原生支持不受
//!   此限）；
//! - **错误格写入**：[`CellContent::Value`] 携带 [`Value::Error`]
//!   恒拒（错误格由求值产生，不由外部注入——回放既有错误格的
//!   场景由消费方以文本承载，office-io 降级登记先例同源）；
//! - **`+`/`-` 开头公式形态**：恒拒（xlsx 存储恒 `=` 形态）；
//! - **单表重算**：无——[`FormulaEngine::recalculate`] 恒全簿
//!   （跨表依赖正确性优先；单表入口语义上等价全簿，性能验收见
//!   `tests/perf.rs`）。
//!
//! # 限额（S3 纪律同构——mdc-pdf 限额表先例）
//!
//! | 常量 | 值 | 语义 |
//! |---|---|---|
//! | [`LIMIT_TOTAL_CELLS`] | 10,000,000 | 全簿灌入格数上限（❶硬拒绝） |
//! | [`LIMIT_SHEETS`] | 1,024 | 表数上限（❶） |
//! | [`LIMIT_FORMULA_BYTES`] | 8,192 | 公式文本字节上限（Excel 域，❶） |
//! | [`LIMIT_CELL_TEXT_BYTES`] | 32,767 | 单格文本字节上限（Excel 域，❶） |
//!
//! 求值侧深度/栈限额透传 [`FormulaEngine::set_max_evaluation_depth`]/
//! [`FormulaEngine::set_max_evaluation_stack`]（引擎原生 DoS 防护）。

pub mod refs;

use std::collections::HashMap;

use ironcalc_base::expressions::parser::{new_parser_english, Node};
use ironcalc_base::expressions::token::Error as EngineError;
use ironcalc_base::expressions::types::CellReferenceRC;
use ironcalc_base::types::{Cell, FormulaValue, SpillValue};
use ironcalc_base::Model;

pub use refs::{format_a1, parse_a1, MAX_COLUMN, MAX_ROW};

/// 全簿灌入格数上限（见模块文档限额表）。
pub const LIMIT_TOTAL_CELLS: usize = 10_000_000;
/// 表数上限。
pub const LIMIT_SHEETS: usize = 1_024;
/// 公式文本字节上限（Excel 域）。
pub const LIMIT_FORMULA_BYTES: usize = 8_192;
/// 单格文本字节上限（Excel 域）。
pub const LIMIT_CELL_TEXT_BYTES: usize = 32_767;

// ---------------------------------------------------------------------------
// 公开模型
// ---------------------------------------------------------------------------

/// 求值结果值（issue #110 草图形态）。
///
/// Excel 语义：空格 ≠ 空串（`Empty` = 无内容；`Text("")` = 显式空
/// 串——写面区分，读面 [`Cell::EmptyCell`] → `Empty`）。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 数字（日期/时间是序列数——消费方以 numFmt 证据判型，本层无
    /// 日期面）。
    Number(f64),
    /// 文本。
    Text(String),
    /// 布尔。
    Bool(bool),
    /// 错误格（求值产生——错误码见 [`FormulaError::code`]）。
    Error(FormulaError),
    /// 空。
    Empty,
}

/// 单元格内容（灌入/编辑面）。
///
/// [`Formula`] 文本**必须带 `=` 前缀**（xlsx 存储形态；`+`/`-`
/// 开头形态拒——见模块文档能力边界）。
///
/// [`Formula`]: CellContent::Formula
#[derive(Debug, Clone, PartialEq)]
pub enum CellContent {
    /// 常量值（`Value::Error` 恒拒——错误格由求值产生）。
    Value(Value),
    /// 公式（带 `=` 前缀）。
    Formula(String),
}

/// 单元格引用（依赖提取产物；均 1-based）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRef {
    /// 表索引（引擎内 worksheets 序 = [`SheetInput`] 序）。
    pub sheet: u32,
    /// 行（1-based）。
    pub row: u32,
    /// 列（1-based）。
    pub col: u32,
}

/// 工作表灌入输入。
#[derive(Debug, Clone, Default)]
pub struct SheetInput {
    /// 表名（Excel 规则：非空、≤31 字符、不含 `[]:*?/\`、簿内唯一——
    /// 引擎 `rename` 校验透传）。
    pub name: String,
    /// 非空格清单 `(row, col, content)`（1-based；坐标域见
    /// [`parse_a1`]；重复坐标后写赢——清单语义 = 按序灌入）。
    pub cells: Vec<(u32, u32, CellContent)>,
}

impl SheetInput {
    /// 便捷构造。
    pub fn new(name: impl Into<String>, cells: Vec<(u32, u32, CellContent)>) -> Self {
        Self {
            name: name.into(),
            cells,
        }
    }
}

/// 公式错误面：Excel 语义错误码（值面）+ 调用面 typed 拒绝。
#[derive(Debug, Clone, PartialEq)]
pub enum FormulaError {
    // --- Excel 错误码（单元格值面；`#CIRC!`/`#ERROR!`/`#NIMPL` 为
    // 引擎扩展码，非 ECMA 标准） ---
    /// `#DIV/0!` 除零。
    DivByZero,
    /// `#REF!` 引用失效。
    Ref,
    /// `#NAME?` 未知名称/函数。
    Name,
    /// `#VALUE!` 类型错。
    Value,
    /// `#NUM!` 数值域错。
    Num,
    /// `#N/A` 查找未命中。
    Na,
    /// `#CIRC!` 循环引用（引擎扩展码）。
    Circ,
    /// `#NULL!` 空区域交集。
    Null,
    /// `#SPILL!` 动态数组溢出受阻（引擎扩展码）。
    Spill,
    /// `#CALC!` 计算中断（引擎扩展码）。
    Calc,
    /// `#ERROR!`（引擎扩展码）。
    Error,
    /// `#NIMPL` 函数未实装（引擎扩展码）。
    NotImplemented,
    // --- 调用面 typed 拒绝 ---
    /// 公式解析失败。
    Parse { message: String },
    /// 表名不存在（求值/依赖提取的目标表或公式内引用的表）。
    UnknownSheet { name: String },
    /// 非法单元格引用文本。
    InvalidCell { text: String },
    /// 非法输入（表名规则/坐标越界/限额/错误格写入/非有限数字等）。
    InvalidInput { message: String },
}

impl FormulaError {
    /// 错误码文本（Excel 形态；调用面错误给可诊断短句）。
    pub fn code(&self) -> String {
        match self {
            Self::DivByZero => "#DIV/0!".into(),
            Self::Ref => "#REF!".into(),
            Self::Name => "#NAME?".into(),
            Self::Value => "#VALUE!".into(),
            Self::Num => "#NUM!".into(),
            Self::Na => "#N/A".into(),
            Self::Circ => "#CIRC!".into(),
            Self::Null => "#NULL!".into(),
            Self::Spill => "#SPILL!".into(),
            Self::Calc => "#CALC!".into(),
            Self::Error => "#ERROR!".into(),
            Self::NotImplemented => "#NIMPL".into(),
            Self::Parse { .. } => "#PARSE!".into(),
            Self::UnknownSheet { name } => format!("#SHEET!({name})"),
            Self::InvalidCell { .. } => "#CELLREF!".into(),
            Self::InvalidInput { .. } => "#INPUT!".into(),
        }
    }
}

impl std::fmt::Display for FormulaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse { message } => write!(f, "公式解析失败：{message}"),
            Self::UnknownSheet { name } => write!(f, "表不存在：{name}"),
            Self::InvalidCell { text } => write!(f, "非法单元格引用：{text}"),
            Self::InvalidInput { message } => write!(f, "非法输入：{message}"),
            other => write!(f, "Excel 错误 {}", other.code()),
        }
    }
}

impl std::error::Error for FormulaError {}

// ---------------------------------------------------------------------------
// 引擎错误映射（ironcalc String 错误 → typed）
// ---------------------------------------------------------------------------

/// ironcalc `Result<_, String>` 错误映射（调用面错误统一入口——
/// 引擎字符串错误按模式归类，不可识别的保真透传进 `InvalidInput`）。
fn map_engine_err(e: String) -> FormulaError {
    if e.starts_with("Index out of bounds") || e.contains("Invalid name for a sheet") {
        FormulaError::InvalidInput { message: e }
    } else if e.starts_with("Sheet not found") || e.starts_with("Incorrect index for sheet") {
        // 形态实测锚定：worksheet(index) 越界 / 表名解析失败。
        FormulaError::UnknownSheet {
            name: String::new(),
        }
    } else {
        FormulaError::InvalidInput { message: e }
    }
}

/// Excel 语义错误码映射（`Error::NIMPL` 等扩展码如实映射——公开面
/// 码表 = 引擎码表全量，无静默降级）。
fn map_cell_error(ei: &EngineError) -> FormulaError {
    match ei {
        EngineError::DIV => FormulaError::DivByZero,
        EngineError::REF => FormulaError::Ref,
        EngineError::NAME => FormulaError::Name,
        EngineError::VALUE => FormulaError::Value,
        EngineError::NUM => FormulaError::Num,
        EngineError::NA => FormulaError::Na,
        EngineError::CIRC => FormulaError::Circ,
        EngineError::NULL => FormulaError::Null,
        EngineError::SPILL => FormulaError::Spill,
        EngineError::CALC => FormulaError::Calc,
        EngineError::ERROR => FormulaError::Error,
        EngineError::NIMPL => FormulaError::NotImplemented,
    }
}

// ---------------------------------------------------------------------------
// FormulaEngine
// ---------------------------------------------------------------------------

/// 公式引擎（issue #110 草图面 + 编辑面）。
///
/// 生命周期：[`new`]（构建即求值）→ [`set_cell`] 编辑 →
/// [`recalculate`] 重算 → [`evaluate`]/[`cell_value`] 读值。
///
/// [`new`]: FormulaEngine::new
/// [`set_cell`]: FormulaEngine::set_cell
/// [`recalculate`]: FormulaEngine::recalculate
/// [`evaluate`]: FormulaEngine::evaluate
/// [`cell_value`]: FormulaEngine::cell_value
pub struct FormulaEngine {
    model: Model<'static>,
    /// 表名 → 索引（镜像只读；`Model` 无按名查询公开面——自持）。
    name_to_index: HashMap<String, u32>,
    /// 表名序（索引 → 名；`sheet_names()` 快路径）。
    index_to_name: Vec<String>,
}

impl FormulaEngine {
    /// 构建：灌入全部表并**立即首次求值**（新公式即时得值——
    /// issue #110「新建公式算不出结果」缺口闭合点）。
    ///
    /// 校验（❶硬拒绝 [`FormulaError::InvalidInput`]）：表数 ≤
    /// [`LIMIT_SHEETS`]；总格数 ≤ [`LIMIT_TOTAL_CELLS`]；坐标域；
    /// 公式/文本长度；`Value::Error` 写入；非有限数字。
    pub fn new(sheets: &[SheetInput]) -> Result<Self, FormulaError> {
        if sheets.is_empty() {
            return Err(FormulaError::InvalidInput {
                message: "至少需要一个工作表".into(),
            });
        }
        if sheets.len() > LIMIT_SHEETS {
            return Err(FormulaError::InvalidInput {
                message: format!("表数 {} 超上限 {}", sheets.len(), LIMIT_SHEETS),
            });
        }
        let total: usize = sheets.iter().map(|s| s.cells.len()).sum();
        if total > LIMIT_TOTAL_CELLS {
            return Err(FormulaError::InvalidInput {
                message: format!("总格数 {total} 超上限 {LIMIT_TOTAL_CELLS}"),
            });
        }
        let mut model =
            Model::new_empty("mdc-formula", "en", "UTC", "en").map_err(map_engine_err)?;
        let mut name_to_index = HashMap::new();
        let mut index_to_name = Vec::with_capacity(sheets.len());
        for (i, sheet) in sheets.iter().enumerate() {
            // 首表复用 new_empty 自带表，其余 new_sheet；重命名到目标名
            // （引擎校验表名规则/唯一性——InvalidInput 透传）。
            if i > 0 {
                model.new_sheet();
            }
            model
                .rename_sheet_by_index(i as u32, &sheet.name)
                .map_err(map_engine_err)?;
            name_to_index.insert(sheet.name.clone(), i as u32);
            index_to_name.push(sheet.name.clone());
            for &(row, col, ref content) in &sheet.cells {
                Self::validate_coords(row, col)?;
                Self::apply_content(&mut model, i as u32, row, col, content)?;
            }
        }
        let engine = Self {
            model,
            name_to_index,
            index_to_name,
        };
        Ok(engine)
    }

    /// 编辑单元格（任务 2 编辑面入口：改值/改公式/清格）。
    ///
    /// **不自动重算**——调用方编辑完统一 [`recalculate`]（脏标记
    /// 由消费方按 [`dependencies_at`] 构建；本面保持最小）。
    pub fn set_cell(
        &mut self,
        sheet: u32,
        row: u32,
        col: u32,
        content: CellContent,
    ) -> Result<(), FormulaError> {
        self.check_sheet(sheet)?;
        Self::validate_coords(row, col)?;
        Self::apply_content(&mut self.model, sheet, row, col, &content)
    }

    /// 全簿重算（单表入口无——模块文档能力边界；跨表依赖正确性
    /// 优先）。错误语义：循环引用等**不由此调用返回**——它们是
    /// 单元格值（[`Value::Error`]），读值面看。
    pub fn recalculate(&mut self) {
        self.model.evaluate();
    }

    /// 读单元格当前值（issue #110 草图面：`cell` 为 A1 文本）。
    pub fn evaluate(&self, sheet: u32, cell: &str) -> Result<Value, FormulaError> {
        let (row, col) = parse_a1(cell)?;
        self.cell_value(sheet, row, col)
    }

    /// 读单元格当前值（坐标面；钻取结构化——错误格给
    /// [`Value::Error`] 携带类型化错误码，不字符串化）。
    pub fn cell_value(&self, sheet: u32, row: u32, col: u32) -> Result<Value, FormulaError> {
        self.check_sheet(sheet)?;
        Self::validate_coords(row, col)?;
        let ws = self
            .model
            .workbook
            .worksheet(sheet)
            .map_err(map_engine_err)?;
        let Some(cell) = ws.cell(row as i32, col as i32) else {
            return Ok(Value::Empty);
        };
        match &*cell {
            Cell::EmptyCell { .. } => Ok(Value::Empty),
            Cell::BooleanCell { v, .. } => Ok(Value::Bool(*v)),
            Cell::NumberCell { v, .. } => Ok(Value::Number(*v)),
            Cell::ErrorCell { ei, .. } => Ok(Value::Error(map_cell_error(ei))),
            Cell::SharedString { si, .. } => {
                let text = self
                    .model
                    .workbook
                    .shared_strings
                    .get(*si as usize)
                    .ok_or_else(|| FormulaError::InvalidInput {
                        message: format!("共享字符串索引 {si} 越界（引擎内部不一致）"),
                    })?;
                Ok(Value::Text(text.clone()))
            }
            Cell::CellFormula { v, .. } | Cell::ArrayFormula { v, .. } => Self::formula_value(v),
            Cell::SpillCell { v, .. } => Ok(match v {
                SpillValue::Number(n) => Value::Number(*n),
                SpillValue::Text(t) => Value::Text(t.clone()),
                SpillValue::Boolean(b) => Value::Bool(*b),
                SpillValue::Error(ei) => Value::Error(map_cell_error(ei)),
            }),
        }
    }

    /// 公式静态依赖提取（issue #110 草图面）。
    ///
    /// **context = 首表 A1**（草图无位置参数——相对引用按此基准
    /// 解析为绝对坐标；真实依赖请用 [`dependencies_at`]）。
    pub fn dependencies(&self, formula: &str) -> Result<Vec<CellRef>, FormulaError> {
        self.dependencies_at(0, 1, 1, formula)
    }

    /// 公式静态依赖提取（严谨面：以公式所在格为 context——相对
    /// 引用 `A1` 在 B5 处指 A1，在 C7 处指 A3，加回 context 还原）。
    ///
    /// 产物：引用端点坐标（区域 `A1:B10` 给两端点两格；调用方按
    /// 区域展开）。公式须带 `=` 前缀（与 [`CellContent::Formula`]
    /// 同规）；解析失败/表名不存在 → typed 错误。
    pub fn dependencies_at(
        &self,
        sheet: u32,
        row: u32,
        col: u32,
        formula: &str,
    ) -> Result<Vec<CellRef>, FormulaError> {
        self.check_sheet(sheet)?;
        Self::validate_coords(row, col)?;
        Self::validate_formula(formula)?;
        let ctx = CellReferenceRC {
            sheet: self.index_to_name[sheet as usize].clone(),
            column: col as i32,
            row: row as i32,
        };
        let body = formula.strip_prefix('=').unwrap_or(formula);
        let mut parser = new_parser_english(self.index_to_name.clone(), vec![], HashMap::new());
        let node = parser.parse(body, &ctx);
        let mut out = Vec::new();
        self.collect_refs(&node, &ctx, &mut out)?;
        Ok(out)
    }

    /// 表名清单（索引序）。
    pub fn sheet_names(&self) -> &[String] {
        &self.index_to_name
    }

    /// 表名 → 索引。
    pub fn sheet_index(&self, name: &str) -> Option<u32> {
        self.name_to_index.get(name).copied()
    }

    /// 表数。
    pub fn sheet_count(&self) -> usize {
        self.index_to_name.len()
    }

    /// 求值深度上限（引擎原生 DoS 防护透传；默认引擎值）。
    pub fn set_max_evaluation_depth(&mut self, depth: usize) {
        self.model.set_max_evaluation_depth(depth);
    }

    /// 求值栈上限（字节；引擎原生 DoS 防护透传）。
    pub fn set_max_evaluation_stack(&mut self, bytes: usize) {
        self.model.set_max_evaluation_stack(bytes);
    }

    // --- 内部 ---

    fn check_sheet(&self, sheet: u32) -> Result<(), FormulaError> {
        if (sheet as usize) < self.index_to_name.len() {
            Ok(())
        } else {
            Err(FormulaError::UnknownSheet {
                name: format!("索引 {sheet}"),
            })
        }
    }

    fn validate_coords(row: u32, col: u32) -> Result<(), FormulaError> {
        if row == 0 || row > MAX_ROW || col == 0 || col > MAX_COLUMN {
            return Err(FormulaError::InvalidInput {
                message: format!("坐标 ({row},{col}) 超域（行 ≤{MAX_ROW}，列 ≤{MAX_COLUMN}）"),
            });
        }
        Ok(())
    }

    fn validate_formula(formula: &str) -> Result<(), FormulaError> {
        if !formula.starts_with('=') {
            return Err(FormulaError::InvalidInput {
                message: format!("公式必须以 '=' 开头：{formula:?}"),
            });
        }
        if formula.len() > LIMIT_FORMULA_BYTES {
            return Err(FormulaError::InvalidInput {
                message: format!(
                    "公式 {len} 字节超上限 {LIMIT_FORMULA_BYTES}",
                    len = formula.len()
                ),
            });
        }
        Ok(())
    }

    /// 内容落格（构建与编辑共用单点——校验矩阵在此）。
    fn apply_content(
        model: &mut Model<'static>,
        sheet: u32,
        row: u32,
        col: u32,
        content: &CellContent,
    ) -> Result<(), FormulaError> {
        let (r, c) = (row as i32, col as i32);
        match content {
            CellContent::Value(Value::Number(n)) => {
                if !n.is_finite() {
                    return Err(FormulaError::InvalidInput {
                        message: format!("非有限数字不能落格：{n}"),
                    });
                }
                model
                    .update_cell_with_number(sheet, r, c, *n)
                    .map_err(map_engine_err)
            }
            CellContent::Value(Value::Text(t)) => {
                if t.len() > LIMIT_CELL_TEXT_BYTES {
                    return Err(FormulaError::InvalidInput {
                        message: format!(
                            "文本 {len} 字节超上限 {LIMIT_CELL_TEXT_BYTES}",
                            len = t.len()
                        ),
                    });
                }
                model
                    .update_cell_with_text(sheet, r, c, t)
                    .map_err(map_engine_err)
            }
            CellContent::Value(Value::Bool(b)) => model
                .update_cell_with_bool(sheet, r, c, *b)
                .map_err(map_engine_err),
            CellContent::Value(Value::Empty) => model
                .set_user_input(sheet, r, c, String::new())
                .map_err(map_engine_err),
            CellContent::Value(Value::Error(e)) => Err(FormulaError::InvalidInput {
                message: format!("错误格由求值产生，不由外部写入：{}", e.code()),
            }),
            CellContent::Formula(f) => {
                Self::validate_formula(f)?;
                model
                    .update_cell_with_formula(sheet, r, c, f.clone())
                    .map_err(map_engine_err)
            }
        }
    }

    /// 公式值 → 公开值（`Unevaluated` = 未重算读到——显式错误面
    /// 而非静默空值）。
    fn formula_value(v: &FormulaValue) -> Result<Value, FormulaError> {
        Ok(match v {
            FormulaValue::Unevaluated => {
                return Err(FormulaError::InvalidInput {
                    message: "公式未求值（先调用 recalculate）".into(),
                })
            }
            FormulaValue::Boolean(b) => Value::Bool(*b),
            FormulaValue::Number(n) => Value::Number(*n),
            FormulaValue::Text(t) => Value::Text(t.clone()),
            FormulaValue::Error { ei, .. } => Value::Error(map_cell_error(ei)),
        })
    }

    /// AST 引用收集（相对引用加回 context——POC 钉定语义；
    /// `Wrong*Kind` = 表名不存在 → typed 错误）。
    fn collect_refs(
        &self,
        node: &Node,
        ctx: &CellReferenceRC,
        out: &mut Vec<CellRef>,
    ) -> Result<(), FormulaError> {
        match node {
            Node::ReferenceKind {
                sheet_index,
                absolute_row,
                absolute_column,
                row,
                column,
                ..
            } => {
                let (r, c) = rel_to_abs(*row, *column, *absolute_row, *absolute_column, ctx)?;
                out.push(CellRef {
                    sheet: *sheet_index,
                    row: r,
                    col: c,
                });
            }
            Node::RangeKind {
                sheet_index,
                absolute_row1,
                absolute_column1,
                row1,
                column1,
                absolute_row2,
                absolute_column2,
                row2,
                column2,
                ..
            } => {
                let (r1, c1) = rel_to_abs(*row1, *column1, *absolute_row1, *absolute_column1, ctx)?;
                let (r2, c2) = rel_to_abs(*row2, *column2, *absolute_row2, *absolute_column2, ctx)?;
                out.push(CellRef {
                    sheet: *sheet_index,
                    row: r1,
                    col: c1,
                });
                out.push(CellRef {
                    sheet: *sheet_index,
                    row: r2,
                    col: c2,
                });
            }
            Node::WrongReferenceKind { sheet_name, .. }
            | Node::WrongRangeKind { sheet_name, .. } => {
                return Err(FormulaError::UnknownSheet {
                    name: sheet_name.clone().unwrap_or_default(),
                });
            }
            Node::ParseErrorKind { message, .. } => {
                return Err(FormulaError::Parse {
                    message: message.clone(),
                });
            }
            // 值/名/表节点：定义名与结构化引用不在依赖提取面（模块
            // 文档能力边界）——不产坐标。
            Node::FunctionKind { args, .. } | Node::NamedFunctionKind { args, .. } => {
                for a in args {
                    self.collect_refs(a, ctx, out)?;
                }
            }
            Node::LambdaDefKind { body, .. } => self.collect_refs(body, ctx, out)?,
            Node::LambdaCallKind { lambda, args } => {
                self.collect_refs(lambda, ctx, out)?;
                for a in args {
                    self.collect_refs(a, ctx, out)?;
                }
            }
            Node::OpRangeKind { left, right }
            | Node::OpConcatenateKind { left, right }
            | Node::OpSumKind { left, right, .. }
            | Node::OpProductKind { left, right, .. }
            | Node::OpPowerKind { left, right }
            | Node::CompareKind { left, right, .. } => {
                self.collect_refs(left, ctx, out)?;
                self.collect_refs(right, ctx, out)?;
            }
            Node::UnaryKind { right, .. } => self.collect_refs(right, ctx, out)?,
            Node::ImplicitIntersection { child, .. } | Node::SpillRangeOperator { child } => {
                self.collect_refs(child, ctx, out)?
            }
            Node::BooleanKind(_)
            | Node::NumberKind(_)
            | Node::StringKind(_)
            | Node::ErrorKind(_)
            | Node::EmptyArgKind
            | Node::ArrayKind(_)
            | Node::DefinedNameKind(_)
            | Node::TableNameKind(_)
            | Node::NamedVariableKind { .. } => {}
        }
        Ok(())
    }
}

/// 相对偏移 → 绝对坐标（A1 模式：非绝对分量 = 相对 context 的偏移，
/// POC 钉定；越界/非正 → typed 错误）。
fn rel_to_abs(
    row: i32,
    col: i32,
    abs_row: bool,
    abs_col: bool,
    ctx: &CellReferenceRC,
) -> Result<(u32, u32), FormulaError> {
    let r = if abs_row { row } else { row + ctx.row };
    let c = if abs_col { col } else { col + ctx.column };
    if r < 1 || c < 1 {
        return Err(FormulaError::InvalidCell {
            text: format!("相对引用解析越界（row={r}, col={c}）"),
        });
    }
    Ok((r as u32, c as u32))
}

// ---------------------------------------------------------------------------
// 引擎语义内嵌测试（集成面见 tests/：函数对照/重算语义/性能/真实 xlsx）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod engine_tests {
    use super::*;

    fn engine(sheets: &[SheetInput]) -> FormulaEngine {
        let mut e = FormulaEngine::new(sheets).expect("构建失败");
        e.recalculate();
        e
    }

    fn num(v: f64) -> Value {
        Value::Number(v)
    }

    #[test]
    fn build_and_first_eval() {
        // 新建公式即时得值（issue #110 核心缺口闭合点）。
        let e = engine(&[SheetInput::new(
            "S",
            vec![
                (1, 1, CellContent::Value(num(1.0))),
                (2, 1, CellContent::Value(num(2.0))),
                (3, 1, CellContent::Value(num(3.0))),
                (1, 2, CellContent::Formula("=SUM(A1:A3)".into())),
            ],
        )]);
        assert_eq!(e.evaluate(0, "B1").unwrap(), num(6.0));
        assert_eq!(e.cell_value(0, 1, 2).unwrap(), num(6.0));
    }

    #[test]
    fn text_number_distinction() {
        // 文本型数字保持文本（坑表「"123" vs 123」）。
        let e = engine(&[SheetInput::new(
            "S",
            vec![
                (1, 1, CellContent::Value(Value::Text("123".into()))),
                (1, 2, CellContent::Value(num(123.0))),
                (1, 3, CellContent::Formula("=LEN(A1)".into())),
                (1, 4, CellContent::Formula("=LEN(B1)".into())),
            ],
        )]);
        assert_eq!(
            e.evaluate(0, "C1").unwrap(),
            num(3.0),
            "LEN(\"123\")=3 文本语义"
        );
        // Excel 宽松转换：LEN(123) 数字隐转文本 = 3（非 #VALUE!——
        // 实测钉定，修正「数字 LEN 报错」的错误预期）。
        assert_eq!(e.evaluate(0, "D1").unwrap(), num(3.0));
        // 类型区分钉定：文本 "123" ≠ 数字 123（Excel 比较语义不跨
        // 类型等值）；算术面文本数字隐转（"123"+0=124 vs 123+0）。
        let e2 = engine(&[SheetInput::new(
            "S",
            vec![
                (1, 1, CellContent::Value(Value::Text("123".into()))),
                (1, 2, CellContent::Value(num(123.0))),
                (1, 3, CellContent::Formula("=A1=B1".into())),
                (1, 4, CellContent::Formula("=A1+1".into())),
            ],
        )]);
        assert_eq!(
            e2.evaluate(0, "C1").unwrap(),
            Value::Bool(false),
            "文本≠数字"
        );
        assert_eq!(
            e2.evaluate(0, "D1").unwrap(),
            num(124.0),
            "文本数字算术隐转"
        );
    }

    #[test]
    fn edit_then_recalc_chain() {
        // 依赖链：A1=B1+1, B1=C1+1；改 C1 → B1/A1 都更新（验收点）。
        let mut e = engine(&[SheetInput::new(
            "S",
            vec![
                (1, 1, CellContent::Formula("=B1+1".into())),
                (1, 2, CellContent::Formula("=C1+1".into())),
                (1, 3, CellContent::Value(num(5.0))),
            ],
        )]);
        assert_eq!(e.evaluate(0, "A1").unwrap(), num(7.0));
        e.set_cell(0, 1, 3, CellContent::Value(num(10.0))).unwrap();
        e.recalculate();
        assert_eq!(
            e.evaluate(0, "B1").unwrap(),
            num(11.0),
            "改 C1 后 B1 须更新"
        );
        assert_eq!(
            e.evaluate(0, "A1").unwrap(),
            num(12.0),
            "改 C1 后 A1 须级联更新"
        );
    }

    #[test]
    fn circular_detected() {
        let e = engine(&[SheetInput::new(
            "S",
            vec![
                (1, 1, CellContent::Formula("=A2+1".into())),
                (2, 1, CellContent::Formula("=A1+1".into())),
            ],
        )]);
        assert_eq!(
            e.evaluate(0, "A1").unwrap(),
            Value::Error(FormulaError::Circ)
        );
        assert_eq!(
            e.evaluate(0, "A2").unwrap(),
            Value::Error(FormulaError::Circ)
        );
    }

    #[test]
    fn error_propagation() {
        // #DIV/0! 沿公式链传播（坑表「错误传播」）。
        let e = engine(&[SheetInput::new(
            "S",
            vec![
                (1, 1, CellContent::Value(num(1.0))),
                (1, 2, CellContent::Formula("=A1/0".into())),
                (1, 3, CellContent::Formula("=B1+1".into())),
            ],
        )]);
        assert_eq!(
            e.evaluate(0, "B1").unwrap(),
            Value::Error(FormulaError::DivByZero)
        );
        assert_eq!(
            e.evaluate(0, "C1").unwrap(),
            Value::Error(FormulaError::DivByZero)
        );
    }

    #[test]
    fn cross_sheet_reference() {
        let e = engine(&[
            SheetInput::new("Data", vec![(1, 1, CellContent::Value(num(21.0)))]),
            SheetInput::new(
                "Report",
                vec![(1, 1, CellContent::Formula("=Data!A1*2".into()))],
            ),
        ]);
        assert_eq!(e.evaluate(1, "A1").unwrap(), num(42.0));
        assert_eq!(e.sheet_names(), ["Data", "Report"]);
        assert_eq!(e.sheet_index("Report"), Some(1));
    }

    #[test]
    fn dependencies_relative_absolute() {
        // B5 处的公式：相对 A1 → (0,1,1)；Data!C2:C10 两端点；$D$4 绝对。
        let e = engine(&[
            SheetInput::new("S", vec![]),
            SheetInput::new("Data", vec![]),
        ]);
        let deps = e
            .dependencies_at(0, 5, 2, "=IF(A1>0, Data!C2:C10, $D$4)")
            .unwrap();
        assert!(
            deps.contains(&CellRef {
                sheet: 0,
                row: 1,
                col: 1
            }),
            "相对 A1@B5 → (0,1,1)：{deps:?}"
        );
        assert!(
            deps.contains(&CellRef {
                sheet: 1,
                row: 2,
                col: 3
            }),
            "Data!C2：{deps:?}"
        );
        assert!(
            deps.contains(&CellRef {
                sheet: 1,
                row: 10,
                col: 3
            }),
            "Data!C10：{deps:?}"
        );
        assert!(
            deps.contains(&CellRef {
                sheet: 0,
                row: 4,
                col: 4
            }),
            "$D$4：{deps:?}"
        );
        // 同一公式在 C7 处：相对 A1 仍指 A1（偏移加回 context）。
        let deps2 = e.dependencies_at(0, 7, 3, "=A1+1").unwrap();
        assert_eq!(
            deps2,
            vec![CellRef {
                sheet: 0,
                row: 1,
                col: 1
            }]
        );
        // 相对偏移引用：B5 处 "=A[-1]C[-1]" 形态不存在（A1 记法无偏移
        // 语法），改用整行/整列区域（无界形态→端点含边界值）不测——
        // 有界区域已覆盖。跨 context 位移验证：C7 处 "=B6" → B6。
        let deps3 = e.dependencies_at(0, 7, 3, "=B6").unwrap();
        assert_eq!(
            deps3,
            vec![CellRef {
                sheet: 0,
                row: 6,
                col: 2
            }]
        );
    }

    #[test]
    fn dependencies_errors() {
        let e = engine(&[SheetInput::new("S", vec![])]);
        assert!(matches!(
            e.dependencies_at(0, 1, 1, "=Nope!A1+1"),
            Err(FormulaError::UnknownSheet { .. })
        ));
        assert!(matches!(
            e.dependencies_at(0, 1, 1, "=SUM("),
            Err(FormulaError::Parse { .. })
        ));
        assert!(
            matches!(
                e.dependencies_at(0, 1, 1, "A1"),
                Err(FormulaError::InvalidInput { .. })
            ),
            "无 = 前缀须拒"
        );
    }

    #[test]
    fn value_error_write_rejected() {
        let e = FormulaEngine::new(&[SheetInput::new("S", vec![])]);
        assert!(e.is_ok());
        let mut e = e.unwrap();
        let r = e.set_cell(
            0,
            1,
            1,
            CellContent::Value(Value::Error(FormulaError::Name)),
        );
        assert!(
            matches!(r, Err(FormulaError::InvalidInput { .. })),
            "错误格写入须拒"
        );
    }

    #[test]
    fn limits_enforced() {
        // 表数上限。
        let many: Vec<SheetInput> = (0..=LIMIT_SHEETS)
            .map(|i| SheetInput::new(format!("S{i}"), vec![]))
            .collect();
        assert!(matches!(
            FormulaEngine::new(&many),
            Err(FormulaError::InvalidInput { .. })
        ));
        // 公式长度。
        let long = format!("={}+1", "1".repeat(LIMIT_FORMULA_BYTES));
        let e = FormulaEngine::new(&[SheetInput::new(
            "S",
            vec![(1, 1, CellContent::Formula(long))],
        )]);
        assert!(matches!(e, Err(FormulaError::InvalidInput { .. })));
        // 坐标域。
        let e = FormulaEngine::new(&[SheetInput::new(
            "S",
            vec![(0, 1, CellContent::Value(num(1.0)))],
        )]);
        assert!(matches!(e, Err(FormulaError::InvalidInput { .. })));
        // 非有限数字。
        let e = FormulaEngine::new(&[SheetInput::new(
            "S",
            vec![(1, 1, CellContent::Value(num(f64::NAN)))],
        )]);
        assert!(matches!(e, Err(FormulaError::InvalidInput { .. })));
    }

    #[test]
    fn sheet_name_rules_passthrough() {
        // 引擎表名校验透传（非法字符/空名/撞名）。
        assert!(matches!(
            FormulaEngine::new(&[SheetInput::new("A/B", vec![])]),
            Err(FormulaError::InvalidInput { .. })
        ));
        assert!(matches!(
            FormulaEngine::new(&[SheetInput::new("", vec![])]),
            Err(FormulaError::InvalidInput { .. })
        ));
        assert!(matches!(
            FormulaEngine::new(&[SheetInput::new("S", vec![]), SheetInput::new("S", vec![]),]),
            Err(FormulaError::InvalidInput { .. })
        ));
    }

    #[test]
    fn error_codes_stable() {
        assert_eq!(FormulaError::DivByZero.code(), "#DIV/0!");
        assert_eq!(FormulaError::Name.code(), "#NAME?");
        assert_eq!(FormulaError::Na.code(), "#N/A");
        assert_eq!(FormulaError::Circ.code(), "#CIRC!");
    }

    #[test]
    fn unevaluated_formula_reads_typed_error() {
        // 构建即求值——但 set_cell 后未 recalculate 的公式读值 =
        // 显式 typed 错误（不静默空值/旧值）。
        let mut e = engine(&[SheetInput::new("S", vec![])]);
        e.set_cell(0, 1, 1, CellContent::Formula("=1+1".into()))
            .unwrap();
        assert!(matches!(
            e.cell_value(0, 1, 1),
            Err(FormulaError::InvalidInput { .. })
        ));
        e.recalculate();
        assert_eq!(e.cell_value(0, 1, 1).unwrap(), num(2.0));
    }
}
