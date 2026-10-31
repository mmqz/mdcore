//! 条件格式写面（issue #113 任务 2）——五种形态 + A1 范围解析 +
//! 引擎翻译（rust_xlsxwriter 包装——引擎类型零泄漏公开面纪律同源）。
//!
//! ## 形态（issue 草图面）
//!
//! [`CondFormat`] 五变体：数据条 [`CondFormat::DataBar`]（数值阈值 +
//! 单色）/ 色阶 [`CondFormat::ColorScale`]（2 或 3 色配对阈值——
//! `colors.len() == thresholds.len() ∈ {2,3}` 校验）/ 图标集
//! [`CondFormat::IconSet`]（阈值数 = 档数 - 1——首档恒显示，引擎
//! 面首档固定 `Percent(0)`）/ 单元格规则 [`CondFormat::CellRule`]
//! （单值比较 + 命中样式）/ 重复值 [`CondFormat::Duplicate`]。
//!
//! 颜色 = `#RRGGBB` 字符串（与 [`crate::write::CellStyle::fill_color`]
//! 同型）；样式 = [`crate::write::CellStyle`]（复用写面样式模型——
//! 条件格式 dxf 面 Excel 限字体/填充/边框子集，超面属性由引擎忽略，
//! 非本层职责）。
//!
//! ## 范围解析
//!
//! [`parse_range`]："A1:C10" / 单格 "B2" / `$` 前缀容忍（Excel 常态
//! 形态）；非法记法、起点 > 终点、超写域（行 > [`MAX_ROW`] / 列 >
//! [`MAX_COL`]）→ typed 拒（[`crate::write::WriteError::InvalidInput`]，
//! 消息含 range 与原因）。
//!
//! ## 能力边界（如实登记）
//!
//! - `Between`/`NotBetween` 双值比较未暴露（引擎
//!   `ConditionalFormatCellRule::Between(T, T)` 支持——issue 草图为
//!   单值形态，后续增量）；
//! - 多区间（`"A1:A5 C1:C5"` 空格分隔）未暴露（引擎 `multi_range`
//!   支持）；
//! - 读取面（calamine 条件格式读）不在本轮——issue 现状注记
//!   「calamine 能读到，没有写」，本层补写面。
//!
//! 验证：tests/xlsx_condfmt.rs——XML 面断言（conditionalFormatting
//! 元素/类型/阈值/dxf）+ LibreOffice 渲染像素对照（环境门）。

use rust_xlsxwriter::{
    Color, ConditionalFormat2ColorScale, ConditionalFormat3ColorScale, ConditionalFormatCell,
    ConditionalFormatCellRule, ConditionalFormatCustomIcon, ConditionalFormatDataBar,
    ConditionalFormatDuplicate, ConditionalFormatIconSet, ConditionalFormatIconType,
    ConditionalFormatType,
};

use crate::write::{style_to_format, CellStyle, WriteError, MAX_COL, MAX_ROW};

/// 条件格式（issue #113 任务 2 草图面——五种）。
#[derive(Debug, Clone, PartialEq)]
pub enum CondFormat {
    /// 数据条：数值阈值 [min, max] + 单色填充。
    DataBar { min: f64, max: f64, color: String },
    /// 色阶：2 色或 3 色（`colors.len() == thresholds.len() ∈ {2,3}`
    /// ——2 色双端阈值，3 色含中点）。
    ColorScale {
        colors: Vec<String>,
        thresholds: Vec<f64>,
    },
    /// 图标集：`thresholds.len() == 档数 - 1`（首档恒显示——引擎面
    /// 固定 `Percent(0)`；阈值按数值比较）。
    IconSet {
        kind: IconSetKind,
        thresholds: Vec<f64>,
    },
    /// 单元格规则：单值比较 + 命中样式。
    CellRule {
        op: CompareOp,
        value: f64,
        style: CellStyle,
    },
    /// 重复值：命中样式（唯一值面 = 引擎 invert，未暴露——登记）。
    Duplicate { style: CellStyle },
}

/// 图标集类型（常用子集——引擎全集见 `ConditionalFormatIconType`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSetKind {
    ThreeArrows,
    ThreeTrafficLights,
    ThreeSymbols,
    FourArrows,
    FiveArrows,
}

impl IconSetKind {
    /// 图标档数（阈值期望数 = 档数 - 1）。
    pub fn icon_count(self) -> usize {
        match self {
            Self::ThreeArrows | Self::ThreeTrafficLights | Self::ThreeSymbols => 3,
            Self::FourArrows => 4,
            Self::FiveArrows => 5,
        }
    }

    fn to_engine(self) -> ConditionalFormatIconType {
        match self {
            Self::ThreeArrows => ConditionalFormatIconType::ThreeArrows,
            Self::ThreeTrafficLights => ConditionalFormatIconType::ThreeTrafficLights,
            Self::ThreeSymbols => ConditionalFormatIconType::ThreeSymbols,
            Self::FourArrows => ConditionalFormatIconType::FourArrows,
            Self::FiveArrows => ConditionalFormatIconType::FiveArrows,
        }
    }
}

/// 单值比较运算（双值 Between/NotBetween 未暴露——模块文档登记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equal,
    NotEqual,
    Greater,
    GreaterEq,
    Less,
    LessEq,
}

/// A1 记法范围解析："A1:C10" / 单格 "B2" / `$` 前缀容忍。产物 =
/// 0 基 (first_row, first_col, last_row, last_col)。
pub(crate) fn parse_range(range: &str) -> Result<(u32, u16, u32, u16), WriteError> {
    let parts: Vec<&str> = range.split(':').collect();
    let (a, b) = match parts.as_slice() {
        [a, b] => (*a, *b),
        [a] => (*a, *a),
        _ => {
            return Err(WriteError::InvalidInput(format!(
                "invalid conditional-format range '{range}' (expected 'A1:C10')"
            )))
        }
    };
    let bad = |why: &str| {
        WriteError::InvalidInput(format!("invalid conditional-format range '{range}': {why}"))
    };
    let (r0, c0) = parse_a1_cell(a).ok_or_else(|| bad("not A1 notation"))?;
    let (r1, c1) = parse_a1_cell(b).ok_or_else(|| bad("not A1 notation"))?;
    if r0 > r1 || c0 > c1 {
        return Err(bad("start must be <= end"));
    }
    if r1 > MAX_ROW {
        return Err(bad(&format!("row exceeds write domain {MAX_ROW}")));
    }
    if c1 > MAX_COL {
        return Err(bad(&format!("col exceeds write domain {MAX_COL}")));
    }
    Ok((r0, c0, r1, c1))
}

/// 单格 A1 解析：`$` 前缀容忍；列 = 字母 26 进制（A=1，AA=27）；
/// 行 = 十进制。产物 0 基 (row, col)。
fn parse_a1_cell(cell: &str) -> Option<(u32, u16)> {
    let s: String = cell.chars().filter(|c| *c != '$').collect();
    if s.is_empty() {
        return None;
    }
    let col_str: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row_str = &s[col_str.len()..];
    if col_str.is_empty() || row_str.is_empty() || !row_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut col: u32 = 0;
    for c in col_str.chars() {
        if !c.is_ascii_uppercase() {
            return None;
        }
        col = col
            .checked_mul(26)?
            .checked_add(u32::from(c as u8 - b'A' + 1))?;
    }
    let row: u32 = row_str.parse().ok()?;
    Some((
        row.checked_sub(1)?,
        u16::try_from(col.checked_sub(1)?).ok()?,
    ))
}

/// `#RRGGBB` → 引擎颜色（格式前置校验——引擎字符串解析面不进
/// 防线，typed 拒消息含颜色与期望形态）。
fn parse_color(color: &str) -> Result<Color, WriteError> {
    let bad = || {
        WriteError::InvalidInput(format!(
            "invalid conditional-format color '{color}' (expected '#RRGGBB')"
        ))
    };
    let hex = color
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(bad)?;
    u32::from_str_radix(hex, 16)
        .map(Color::RGB)
        .map_err(|_| bad())
}

/// 引擎翻译 + 落表（[`crate::write::SheetWriter::add_cond_format`]
/// 单点调用面——引擎类型不出公开签名）。
pub(crate) fn apply(
    ws: &mut rust_xlsxwriter::Worksheet,
    r0: u32,
    c0: u16,
    r1: u32,
    c1: u16,
    cf: &CondFormat,
) -> Result<(), WriteError> {
    match cf {
        CondFormat::DataBar { min, max, color } => {
            if min > max {
                return Err(WriteError::InvalidInput(format!(
                    "data bar min {min} > max {max}"
                )));
            }
            let bar = ConditionalFormatDataBar::new()
                .set_fill_color(parse_color(color)?)
                .set_minimum(ConditionalFormatType::Number, *min)
                .set_maximum(ConditionalFormatType::Number, *max);
            ws.add_conditional_format(r0, c0, r1, c1, &bar)?;
        }
        CondFormat::ColorScale { colors, thresholds } => {
            let n = colors.len();
            if n != thresholds.len() || !(2..=3).contains(&n) {
                return Err(WriteError::InvalidInput(format!(
                    "color scale requires 2 or 3 paired colors/thresholds (got {n} colors / {} thresholds)",
                    thresholds.len()
                )));
            }
            let parsed: Vec<Color> = colors
                .iter()
                .map(|c| parse_color(c))
                .collect::<Result<_, _>>()?;
            match n {
                2 => {
                    let cs = ConditionalFormat2ColorScale::new()
                        .set_minimum(ConditionalFormatType::Number, thresholds[0])
                        .set_maximum(ConditionalFormatType::Number, thresholds[1])
                        .set_minimum_color(parsed[0])
                        .set_maximum_color(parsed[1]);
                    ws.add_conditional_format(r0, c0, r1, c1, &cs)?;
                }
                3 => {
                    let cs = ConditionalFormat3ColorScale::new()
                        .set_minimum(ConditionalFormatType::Number, thresholds[0])
                        .set_midpoint(ConditionalFormatType::Number, thresholds[1])
                        .set_maximum(ConditionalFormatType::Number, thresholds[2])
                        .set_minimum_color(parsed[0])
                        .set_midpoint_color(parsed[1])
                        .set_maximum_color(parsed[2]);
                    ws.add_conditional_format(r0, c0, r1, c1, &cs)?;
                }
                _ => unreachable!("length guarded above"),
            }
        }
        CondFormat::IconSet { kind, thresholds } => {
            let count = kind.icon_count();
            if thresholds.len() != count - 1 {
                return Err(WriteError::InvalidInput(format!(
                    "icon set {:?} expects {} threshold(s) (got {})",
                    kind,
                    count - 1,
                    thresholds.len()
                )));
            }
            let mut icons = vec![
                ConditionalFormatCustomIcon::new().set_rule(ConditionalFormatType::Percent, 0)
            ];
            for t in thresholds {
                icons.push(
                    ConditionalFormatCustomIcon::new().set_rule(ConditionalFormatType::Number, *t),
                );
            }
            let is = ConditionalFormatIconSet::new()
                .set_icon_type(kind.to_engine())
                .set_icons(&icons);
            ws.add_conditional_format(r0, c0, r1, c1, &is)?;
        }
        CondFormat::CellRule { op, value, style } => {
            let rule = match op {
                CompareOp::Equal => ConditionalFormatCellRule::EqualTo(*value),
                CompareOp::NotEqual => ConditionalFormatCellRule::NotEqualTo(*value),
                CompareOp::Greater => ConditionalFormatCellRule::GreaterThan(*value),
                CompareOp::GreaterEq => ConditionalFormatCellRule::GreaterThanOrEqualTo(*value),
                CompareOp::Less => ConditionalFormatCellRule::LessThan(*value),
                CompareOp::LessEq => ConditionalFormatCellRule::LessThanOrEqualTo(*value),
            };
            let cell = ConditionalFormatCell::new()
                .set_rule(rule)
                .set_format(style_to_format(style));
            ws.add_conditional_format(r0, c0, r1, c1, &cell)?;
        }
        CondFormat::Duplicate { style } => {
            let dup = ConditionalFormatDuplicate::new().set_format(style_to_format(style));
            ws.add_conditional_format(r0, c0, r1, c1, &dup)?;
        }
    }
    Ok(())
}
