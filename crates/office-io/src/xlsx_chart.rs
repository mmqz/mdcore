//! XLSX 图表写面（issue #113 任务 3）——`ChartSpec` → rust_xlsxwriter
//! `Chart` 翻译面，引擎类型零泄漏。
//!
//! # 定位
//!
//! issue #113 任务 3 草图：四种图表（柱/条/折线/饼/散点）写入 xlsx，
//! DrawingML 由引擎（rust_xlsxwriter 0.99，已在产品闭包——write.rs
//! 单元格写面同引擎）生成；本模块是**规格翻译 + 校验层**：不手写
//! chart1.xml/drawing1.xml（引擎产出，[Content_Types].xml / rels 关联
//! 引擎自理）。
//!
//! # 系列语义（Excel「插入图表」惯例——文档化契约）
//!
//! `data_range` 必须含**表头**：首行/首列为标注面，余下为数据面：
//!
//! ```text
//! series_in_columns = true（默认心智：每列一个系列）
//!   首列 = 类别轴（categories/X），首行 = 系列名（各值列表头）
//!   其余每列 = 一个系列（values/Y）
//! series_in_columns = false（每行一个系列——镜像）
//!   首行 = 类别轴，首列 = 系列名，其余每行 = 一个系列
//! ```
//!
//! 最小范围 ≥ 2×2（表头 + ≥1 数据 + 类别 + ≥1 值）。系列引用是
//! **范围引用而非值快照**（改数据图表跟随——引擎 chart cache 写入
//! `c:numCache` 仅作打开期显示，`c:f` 引用公式是权威）。
//!
//! # 单位换算
//!
//! - `position: (行, 列)`：0 基单元格坐标（左上锚点，引擎
//!   `insert_chart(row, col)` one-cell-anchor 语义）。
//! - `size: (宽, 高)`：EMU（1/914400 英寸）。引擎 `set_width/
//!   set_height` 取像素（96dpi）——换算 `px = emu / 9525`，四舍五入，
//!   0 像素拒绝（typed `InvalidInput`）。
//!
//! # 校验面（typed，引擎调用前）
//!
//! - data_range 语法（`[']表名'[']!A1:C10`，`$` 前缀容忍，可选 `=`
//!   前缀）+ 行列界（[`MAX_ROW`]/[`MAX_COLS`]）+ ≥2×2 维度；
//! - position 行列界；size 换算后 ≥1 像素；hole_size ∈ (0, 90]
//!   （引擎孔径域，越界拒绝不静默钳制）；标题长度 ≤
//!   [`crate::write::MAX_CELL_TEXT`]。
//!
//! # 能力边界（如实登记）
//!
//! - 表名引号（含空格等）**输入面**容忍（剥离引号，引擎输出面自引号
//!   `quote_sheet_name`）；表名内单引号转义（`''`）不支持；
//! - 系列名/类别固定取表头行/列（不自定义系列子范围——v0 约定即
//!   契约）；标题为空串 = 引擎默认（无标题）；
//! - 轴样式/图例/数据标签等外观面未暴露（引擎支持，宿主后续增量）；
//! - 读取面（calamine）不读图表——图表是写面能力。

use rust_xlsxwriter::{Chart, ChartType, Worksheet};

use crate::write::{WriteError, MAX_CELL_TEXT, MAX_ROW};

/// 列上界（0 基，16384 列 —— 与写面 `MAX_COL` 同界；本地命名避免
/// 私有常量跨模块可见性问题）。
const MAX_COLS: u16 = 16_383;

/// EMU → 像素换算分母（96dpi：1px = 9525 EMU）。
const EMU_PER_PX: u32 = 9_525;

/// 柱状方向：Vertical = Excel「柱形图」（Column，类别横轴）；Horizontal
/// = Excel「条形图」（Bar，类别纵轴）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarDirection {
    Vertical,
    Horizontal,
}

/// 图表种类（issue #113 任务 3 草图形态）。
#[derive(Debug, Clone, PartialEq)]
pub enum ChartKind {
    /// 柱状/条形。`stacked` = 堆积百分比前置的普通堆积。
    Bar {
        direction: BarDirection,
        stacked: bool,
    },
    /// 折线。`smooth` = 平滑曲线（逐系列应用）。
    Line { smooth: bool },
    /// 饼图。`hole_size: Some(pct)` = 环形（Doughnut，孔径百分比
    /// (0,90]）；`None` = 实心饼。
    Pie { hole_size: Option<f64> },
    /// 散点。`lines: true` = 点间直线连接（标记保留）；`false` = 仅标记点。
    Scatter { lines: bool },
}

/// 图表规格（issue #113 任务 3 草图；单位语义见模块文档）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSpec {
    pub kind: ChartKind,
    /// 图表标题（空串 = 引擎默认无标题）。
    pub title: String,
    /// 数据范围 `"Sheet1!A1:C10"`（含表头行/列——系列语义见模块文档）。
    pub data_range: String,
    /// true = 每列一个系列（首列表头为类别）；false = 每行一个系列。
    pub series_in_columns: bool,
    /// 左上锚点（行, 列）——0 基单元格坐标。
    pub position: (u32, u32),
    /// 尺寸（宽, 高）——EMU。
    pub size: (u32, u32),
}

// ---------------------------------------------------------------------------
// 范围解析（"Sheet1!A1:C10" → 结构化；引擎元组 API 免字符串回写）
// ---------------------------------------------------------------------------

/// 结构化范围（0 基闭区间，规范化 r0≤r1 / c0≤c1；表名已剥引号）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RangeRef {
    sheet: String,
    r0: u32,
    c0: u16,
    r1: u32,
    c1: u16,
}

/// 解析 `"[']表名[']!A1:C10"`（`$`/前导 `=` 容忍）。
fn parse_range(input: &str) -> Result<RangeRef, WriteError> {
    let s = input.strip_prefix('=').unwrap_or(input);
    let bang = s.rfind('!').ok_or_else(|| {
        WriteError::InvalidInput(format!(
            "data_range 缺表名前缀（应为 'Sheet1!A1:C10' 形态）：{input}"
        ))
    })?;
    let (sheet_raw, range_raw) = s.split_at(bang);
    let sheet = sheet_raw
        .strip_prefix('\'')
        .and_then(|x| x.strip_suffix('\''))
        .unwrap_or(sheet_raw);
    if sheet.is_empty() {
        return Err(WriteError::InvalidInput(format!(
            "data_range 表名为空：{input}"
        )));
    }
    let range = range_raw[1..].replace('$', "");
    let (a, b) = match range.split_once(':') {
        Some((a, b)) => (a, b),
        None => (range.as_str(), range.as_str()),
    };
    let (r0, c0) = parse_cell(a, input)?;
    let (r1, c1) = parse_cell(b, input)?;
    Ok(RangeRef {
        sheet: sheet.to_string(),
        r0: r0.min(r1),
        c0: c0.min(c1),
        r1: r0.max(r1),
        c1: c0.max(c1),
    })
}

/// `"B3"` → (0 基行, 0 基列)。形态 = 前导字母段 + 尾随数字段
/// （`[A-Za-z]+[0-9]+`）；其他形态 = typed 拒绝。
fn parse_cell(cell: &str, ctx: &str) -> Result<(u32, u16), WriteError> {
    let bad = || WriteError::InvalidInput(format!("data_range 单元格引用非法（{cell:?}）：{ctx}"));
    let nums = cell.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let letters = &cell[..cell.len() - nums.len()]; // 前导 ASCII 字母段（字节界安全）
    if letters.is_empty() || nums.is_empty() || !nums.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let mut col: u32 = 0;
    for b in letters.bytes() {
        col = col
            .checked_mul(26)
            .and_then(|v| v.checked_add(u32::from(b.to_ascii_uppercase() - b'A' + 1)))
            .ok_or_else(bad)?;
    }
    let row: u32 = nums.parse().map_err(|_| bad())?;
    let (row0, col0) = (
        row.checked_sub(1).ok_or_else(bad)?,
        col.checked_sub(1).ok_or_else(bad)?,
    );
    if row0 > MAX_ROW || col0 > u32::from(MAX_COLS) {
        return Err(WriteError::InvalidInput(format!(
            "data_range 越界（{cell:?} 超 {MAX_ROW} 行 / {MAX_COLS} 列界）：{ctx}"
        )));
    }
    Ok((row0, col0 as u16))
}

// ---------------------------------------------------------------------------
// 规格翻译（ChartSpec → 引擎 Chart）
// ---------------------------------------------------------------------------

/// EMU → 像素（96dpi 向上取整；0 像素 = typed 拒绝——空图无意义）。
fn emu_to_px(emu: u32, what: &str) -> Result<u32, WriteError> {
    let px = emu.div_ceil(EMU_PER_PX);
    if px == 0 {
        return Err(WriteError::InvalidInput(format!(
            "chart {what} {emu} EMU 换算 0 像素（最小 1px = {EMU_PER_PX} EMU）"
        )));
    }
    Ok(px)
}

/// 构建引擎 Chart（校验全部在引擎调用前——typed 纪律）。系列范围用
/// 引擎元组 API（`("Sheet1", r, c, r, c)`）——表名引号由引擎输出面
/// `quote_sheet_name` 自理，免 A1 回写。
fn build(spec: &ChartSpec) -> Result<Chart, WriteError> {
    let range = parse_range(&spec.data_range)?;
    // 维度校验：表头行+列 与 类别+≥1 值（两方向对称的 ≥2×2）。
    if range.r0 == range.r1 || range.c0 == range.c1 {
        return Err(WriteError::InvalidInput(format!(
            "data_range 需要 ≥2×2（表头行/列 + 数据面）：{}",
            spec.data_range
        )));
    }
    // 位置界。
    let (row, col) = spec.position;
    if row > MAX_ROW || col > u32::from(MAX_COLS) {
        return Err(WriteError::InvalidInput(format!(
            "chart position ({row},{col}) 越界（{MAX_ROW} 行 / {MAX_COLS} 列界）"
        )));
    }
    // 标题限长（空串 = 引擎默认无标题）。
    if !spec.title.is_empty() && spec.title.len() > MAX_CELL_TEXT {
        return Err(WriteError::InvalidInput(format!(
            "chart title exceeds {MAX_CELL_TEXT} characters"
        )));
    }
    // 尺寸（EMU → px）。
    let (w, h) = (emu_to_px(spec.size.0, "宽")?, emu_to_px(spec.size.1, "高")?);

    let series_count = if spec.series_in_columns {
        range.c1 - range.c0 // 值列数（总列 − 类别列；u16 天然）
    } else {
        (range.r1 - range.r0) as u16 // 值行数（总行 − 表头行）
    };

    let (chart_type, line_smooth) = kind_to_engine(&spec.kind)?;
    let mut chart = Chart::new(chart_type);
    if !spec.title.is_empty() {
        chart.title().set_name(&spec.title);
    }
    chart.set_width(w).set_height(h);

    let sheet = range.sheet.as_str();
    for i in 0..series_count {
        let s = chart.add_series();
        let (cat, val, name) = if spec.series_in_columns {
            (
                (sheet, range.r0 + 1, range.c0, range.r1, range.c0),
                (
                    sheet,
                    range.r0 + 1,
                    range.c0 + 1 + i,
                    range.r1,
                    range.c0 + 1 + i,
                ),
                (sheet, range.r0, range.c0 + 1 + i),
            )
        } else {
            let ri = u32::from(i);
            (
                (sheet, range.r0, range.c0 + 1, range.r0, range.c1),
                // 值域剔除系列名列（c0）：B..C，非 A..C。
                (
                    sheet,
                    range.r0 + 1 + ri,
                    range.c0 + 1,
                    range.r0 + 1 + ri,
                    range.c1,
                ),
                (sheet, range.r0 + 1 + ri, range.c0),
            )
        };
        s.set_name(name).set_categories(cat).set_values(val);
        if line_smooth {
            s.set_smooth(true);
        }
    }
    // 饼图孔径：验证在建型面已拒 (0,90] 外值；舍入边界（如 0.4% → 0）
    // 钳 ≥1——引擎域 0..=90 含 0 但 0 孔径无意义，闭合舍入缝隙。
    if let ChartKind::Pie { hole_size: Some(p) } = &spec.kind {
        chart.set_hole_size(p.round().max(1.0) as u8);
    }
    Ok(chart)
}

/// 模型 kind → 引擎 ChartType（+ 折线平滑旗标；散点连线已内在于
/// ScatterStraight vs Scatter 选型）。
fn kind_to_engine(kind: &ChartKind) -> Result<(ChartType, bool), WriteError> {
    Ok(match kind {
        ChartKind::Bar { direction, stacked } => (
            match (direction, stacked) {
                (BarDirection::Vertical, false) => ChartType::Column,
                (BarDirection::Vertical, true) => ChartType::ColumnStacked,
                (BarDirection::Horizontal, false) => ChartType::Bar,
                (BarDirection::Horizontal, true) => ChartType::BarStacked,
            },
            false,
        ),
        ChartKind::Line { smooth } => (ChartType::Line, *smooth),
        ChartKind::Pie { hole_size } => match hole_size {
            None => (ChartType::Pie, false),
            Some(p) => {
                if !p.is_finite() || *p <= 0.0 || *p > 90.0 {
                    return Err(WriteError::InvalidInput(format!(
                        "pie hole_size {p} 越界（孔径域 (0,90] 百分比）"
                    )));
                }
                (ChartType::Doughnut, false)
            }
        },
        ChartKind::Scatter { lines } => (
            // 标记恒在（Excel 散点默认心暂）；lines 只控连线。
            if *lines {
                ChartType::ScatterStraightWithMarkers
            } else {
                ChartType::Scatter
            },
            false,
        ),
    })
}

// ---------------------------------------------------------------------------
// 应用面（write.rs 唯一入口——引擎类型不出公开签名）
// ---------------------------------------------------------------------------

/// 写入图表到工作表（`insert_chart`——one-cell-anchor 锚定）。所有
/// 校验与建图在 [`build`]；引擎错误统一 `From<XlsxError>` 包装。
pub(crate) fn apply(ws: &mut Worksheet, spec: &ChartSpec) -> Result<(), WriteError> {
    let chart = build(spec)?;
    let (row, col) = (spec.position.0, spec.position.1 as u16);
    ws.insert_chart(row, col, &chart)
        .map_err(WriteError::from)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 模块测试（解析面单测；XML/LO 行为面见 tests/xlsx_chart.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_forms() {
        let r = parse_range("Sheet1!A1:C10").unwrap();
        assert_eq!((r.r0, r.c0, r.r1, r.c1), (0, 0, 9, 2));
        assert_eq!(r.sheet, "Sheet1");
        // 引号表名 / $ / 前导 = / 反序 / 两位行号（字母段/数字段切分）。
        let r = parse_range("='My Data'!$B$2:$A$5").unwrap();
        assert_eq!((r.r0, r.c0, r.r1, r.c1), (1, 0, 4, 1));
        assert_eq!(r.sheet, "My Data");
        assert_eq!(parse_range("Sheet1!AA1:AB20").unwrap().c0, 26);
        assert!(parse_range("Sheet1").is_err(), "缺 !");
        assert!(parse_range("Sheet1!A1:B").is_err(), "半引用");
        assert!(parse_range("Sheet1!1A:2B").is_err(), "列字母缺失");
        assert!(parse_range("Sheet1!A1B:2B").is_err(), "字母数字交错");
        assert!(parse_range("!A1:B2").is_err(), "空表名");
        assert!(parse_range("Sheet1!A0:B2").is_err(), "0 行（1 基语义）");
        assert!(parse_range("Sheet1!A1:XFE1").is_err(), "列越界");
    }

    #[test]
    fn emu_conversion_bounds() {
        assert_eq!(emu_to_px(9_525, "宽").unwrap(), 1);
        assert_eq!(emu_to_px(9_525 * 480, "宽").unwrap(), 480);
        // 亚像素向上取整为 1px（最小可见尺寸）；仅 0 EMU 拒绝。
        assert_eq!(emu_to_px(9_524, "宽").unwrap(), 1);
        assert!(emu_to_px(0, "宽").is_err(), "0 EMU 拒绝");
    }

    #[test]
    fn kind_validation() {
        assert!(kind_to_engine(&ChartKind::Pie {
            hole_size: Some(90.0)
        })
        .is_ok());
        assert!(kind_to_engine(&ChartKind::Pie {
            hole_size: Some(0.0)
        })
        .is_err());
        assert!(kind_to_engine(&ChartKind::Pie {
            hole_size: Some(90.1)
        })
        .is_err());
        assert!(kind_to_engine(&ChartKind::Pie {
            hole_size: Some(f64::NAN)
        })
        .is_err());
    }
}
