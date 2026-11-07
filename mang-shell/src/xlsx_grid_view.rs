//! XLSX 网格视图（#389 阶段 1——桌面端真网格渲染骨架）。
//!
//! # 定位（issue #389 阶段 1 范围声明）
//!
//! 现状：桌面 xlsx = 只读**行模型**（`editor::xlsx_doc` 把表格拍平成
//! 文本行——引擎能改 / wasm 能改 / 桌面只能看）。本模块把显示面换成
//! **真网格**：列字母行 + 行号列边缘固定、单元格点选高亮、双向虚拟
//! 化滚动、工作表切换树保留。
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ [Sheet1] [Sheet2] …   （工作表切换——簿序）    │
//! ├───────┬──────────────────────────────────────┤
//! │corner │   A     B     C     …  （列头·顶部固定）│
//! ├───────┼──────────────────────────────────────┤
//! │   1   │ cell   cell   cell   （行号列·左固定） │
//! │   2   │ cell   …       （uniform_list 纵向虚拟化）│
//! │   ⋮   │     （可见列窗口——横向虚拟化）        │
//! └───────┴──────────────────────────────────────┘
//! ```
//!
//! # 虚拟化纪律（两轴）
//!
//! - **纵向**：`uniform_list`（GPUI 原生——只构建可见行）；
//! - **横向**：`scroll_x` 自管状态 + 可见列窗口（`visible_col_window`
//!   纯函数）——滚轮 deltaX / Shift+滚轮 deltaY 驱动（`clamp_scroll_x`
//!   钳制），窗口外列零构建；底部滚动指示条按 scroll_x / 总宽渲染。
//!
//! # 点选（命中测试）
//!
//! 覆盖 canvas 的 paint 期注册窗口级鼠标观察器（pdf_pages 同模式：
//! bounds 过滤 + 不占命中区不挡滚动）——`hit_test_cell` 纯函数把局部
//! 坐标 + 滚动状态解析为 (行, 列)，选中格高亮边框 + 行号/列头联动。
//!
//! # 数据面（单一来源：office-io 读面）
//!
//! `XlsxGridModel` = `io::read_book_with_options` 的网格化转译（与
//! `xlsx_doc::load` 同一读面、同一护栏口径：行 1000 / 列 64 / 单元格宽
//! 24）。**阶段 1 不接编辑**（阶段 2 另轮）；**不动 `xlsx_doc.rs`**
//! （保留为 fallback / 文本视图——状态栏切换钮同 pdf 页面/文本模式
//! 先例）。
//!
//! # 已知边界（阶段 1 登记——issue 阶段 3 完善项）
//!
//! - 合并格显示 = 左上单点语义（与行模型同口径——真跨格渲染属阶段 3）；
//! - 超链接/数字格式面外（`CellData::display` 同行模型口径）；
//! - 沙箱无 GUI：GPUI 层编译面验证 + 逻辑面（模型/几何纯函数 + 夹具
//!   往返）全量真跑——`tests/xlsx_grid_model.rs`；真机滚动/点选体验
//!   待开发机（与 #302 pdf_pages 同纪律）。

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, DispatchPhase, FocusHandle, Focusable, Pixels, ScrollEvent,
    SharedString, UniformListScrollHandle, Window, px,
};
use mdc_office_io::io::{Book, CellData, ReadOptions};
use ui::prelude::*;

// ---------------------------------------------------------------------------
// 常量（渲染口径——与 xlsx_doc.rs 护栏同源，不另立标准）
// ---------------------------------------------------------------------------

/// 每表渲染行上限（office-io LIMIT_TOTAL_CELLS 的行模型护栏同款）。
pub const MAX_RENDER_ROWS: usize = 1000;
/// 每表渲染列上限（同上）。
pub const MAX_RENDER_COLS: usize = 64;
/// 单元格显示宽上限（East Asian cell 计——超宽截断，列宽钳制吸收）。
pub const CELL_WIDTH_CAP: usize = 24;
/// 列头高（px）。
pub const HEAD_H: f32 = 24.0;
/// 行高（px——uniform_list 等高行的基准）。
pub const ROW_H: f32 = 22.0;
/// 行号槽宽（px）。
pub const GUTTER_W: f32 = 48.0;
/// 列宽下限 / 上限（px——内容自适应 + 域钳制；上限 = CELL_WIDTH_CAP
/// 内容的自然宽：24 cell × 7.5px + 双侧内边距 12px = 192px——两钳
/// 制面串联，上限与内容封顶同源不矛盾）。
pub const COL_W_MIN: f32 = 64.0;
pub const COL_W_MAX: f32 = 192.0;
/// 列间距（px——网格线代替）。
pub const COL_GAP: f32 = 1.0;
/// 单元格内边距（px）。
const CELL_PAD: f32 = 6.0;

// ---------------------------------------------------------------------------
// 数据模型（零 GPUI——全量可测）
// ---------------------------------------------------------------------------

/// 单表网格（显示文本矩阵 + 几何 + 护栏标记）。
#[derive(Clone, Debug, Default)]
pub struct GridSheet {
    /// 表名（簿序）。
    pub name: String,
    /// 数据区起始绝对坐标（0 基——office-io 包围盒）。
    pub start_row: u32,
    pub start_col: u32,
    /// 渲染行/列数（护栏后）。
    pub rows: usize,
    pub cols: usize,
    /// 行主序显示文本（`rows × cols`；Empty = 空串；公式格 = `=公式`）。
    pub cells: Vec<Vec<String>>,
    /// 触发渲染护栏（行 > 1000 或列 > 64——非静默截断标记）。
    pub truncated: bool,
    /// 合并范围数（显示 = 左上单点语义——阶段 3 完善项计数登记；
    /// 集成测试 `grid_model_geometry_and_guards` 断言消费，bin 视图
    /// 面暂未渲染——预期内，非死代码）。
    #[allow(dead_code)]
    pub merges: usize,
}

/// 网格载荷（office-io 读面 → 网格模型——单一来源）。
#[derive(Clone, Debug, Default)]
pub struct XlsxGridModel {
    pub sheets: Vec<GridSheet>,
    /// 触发护栏的表数（状态条提示用）。
    pub truncated_sheets: usize,
}

impl XlsxGridModel {
    /// 读盘 + 网格化（`document::open` 的 xlsx 分派同源读面；office-io
    /// 内建输入字节门 + 三门——本层不重复）。
    pub fn load(path: &Path) -> Result<Self, String> {
        let book = mdc_office_io::io::read_book_with_options(
            &path.to_string_lossy(),
            &ReadOptions { compat: true },
        )
        .map_err(|e| format!("xlsx 打开失败：{e}"))?;
        Ok(Self::from_book(&book))
    }

    /// Book → 网格模型（纯函数——转换规则锚点）。
    pub fn from_book(book: &Book) -> Self {
        let mut sheets = Vec::with_capacity(book.sheets.len());
        let mut truncated_sheets = 0;
        for sheet in &book.sheets {
            let grid = &sheet.typed;
            let Some((row0, col0)) = grid.start() else {
                // 空表：零行零列（网格面 = 空态提示）。
                sheets.push(GridSheet {
                    name: sheet.name.clone(),
                    merges: sheet.merges.len(),
                    ..Default::default()
                });
                continue;
            };
            let height = grid.height();
            let width = grid.width();
            let rows = height.min(MAX_RENDER_ROWS);
            let cols = width.min(MAX_RENDER_COLS);
            let truncated = height > MAX_RENDER_ROWS || width > MAX_RENDER_COLS;
            if truncated {
                truncated_sheets += 1;
            }
            // 显示文本（cell_display 口径 = xlsx_doc 行模型同源：公式
            // 回退 `=公式`、Empty = 空串——两视图内容零漂移）。
            let cells = (0..rows)
                .map(|r| {
                    (0..cols)
                        .map(|c| {
                            let cell = grid.get_abs(row0 + r as u32, col0 + c as u32);
                            let formula = sheet.formula_at(row0 + r as u32, col0 + c as u32);
                            cell_display(cell, formula)
                        })
                        .collect()
                })
                .collect();
            sheets.push(GridSheet {
                name: sheet.name.clone(),
                start_row: row0,
                start_col: col0,
                rows,
                cols,
                cells,
                truncated,
                merges: sheet.merges.len(),
            });
        }
        Self {
            sheets,
            truncated_sheets,
        }
    }
}

/// 单元格显示文本（`xlsx_doc::cell_display` 同口径——两视图一致）。
fn cell_display(cell: Option<&CellData>, formula: Option<&str>) -> String {
    match (cell, formula) {
        (Some(CellData::Empty), Some(f)) => format!("={f}"),
        (Some(c), _) => c.display().to_string(),
        (None, Some(f)) => format!("={f}"),
        (None, None) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 几何纯函数（零 GPUI——单测锚点）
// ---------------------------------------------------------------------------

/// 绝对列号（0 基）→ 列字母（A..Z / AA..——xlsx 语义）。
pub fn col_letter(col: u32) -> String {
    let mut n = col;
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII 列字母")
}

/// East Asian 显示宽（CJK 2 cell——与 docx_doc/xlsx_doc 同口径约定）。
pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            let cjk = (0x1100..=0x115F).contains(&cp) // Hangul Jamo
                || (0x2E80..=0xA4CF).contains(&cp) // CJK 部首..Yi
                || (0xAC00..=0xD7A3).contains(&cp) // Hangul 音节
                || (0xF900..=0xFAFF).contains(&cp) // CJK 兼容表意
                || (0xFE30..=0xFE4F).contains(&cp) // CJK 兼容形式
                || (0xFF00..=0xFF60).contains(&cp) // 全角形式
                || (0xFFE0..=0xFFE6).contains(&cp); // 全角符号
            if cjk { 2 } else { 1 }
        })
        .sum()
}

/// 内容自适应列宽（px）：内容 EA 宽（封顶 CELL_WIDTH_CAP）× 字号近似
/// + 内边距，域钳制 [COL_W_MIN, COL_W_MAX]。
pub fn col_width_px(content_ea: usize) -> f32 {
    let w = content_ea.min(CELL_WIDTH_CAP) as f32 * 7.5 + CELL_PAD * 2.0;
    w.clamp(COL_W_MIN, COL_W_MAX)
}

/// 逐列 x 偏移（px，含 COL_GAP——首列 0 起）。
pub fn col_offsets(widths: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(widths.len());
    let mut x = 0.0;
    for w in widths {
        out.push(x);
        x += w + COL_GAP;
    }
    out
}

/// 可见列窗口：`[first, end)`（scroll_x 起、viewport_w 宽的窗口覆盖的
/// 列区间——窗口外列零构建）。
pub fn visible_col_window(
    offsets: &[f32],
    widths: &[f32],
    scroll_x: f32,
    viewport_w: f32,
) -> (usize, usize) {
    let n = offsets.len().min(widths.len());
    let right = scroll_x + viewport_w.max(0.0);
    let mut first = n;
    let mut end = 0;
    for i in 0..n {
        let col_right = offsets[i] + widths[i];
        let visible = col_right > scroll_x && offsets[i] < right;
        if visible {
            if i < first {
                first = i;
            }
            end = i + 1;
        }
    }
    if first >= end {
        (0, 0) // 空表 / 零列 / 超界 scroll
    } else {
        (first, end)
    }
}

/// 横向滚动钳制：[0, max(0, total_w - viewport_w)]（内容窄于视口 = 0；
/// NaN 防御归 0）。
pub fn clamp_scroll_x(requested: f32, total_w: f32, viewport_w: f32) -> f32 {
    if requested.is_nan() {
        return 0.0;
    }
    let max = (total_w - viewport_w).max(0.0);
    requested.clamp(0.0, max)
}

/// 滚轮事件 → 横向位移（px）：deltaX 直传；Shift+deltaY 转横向
/// （触摸板两指横扫 / 鼠标 Shift+滚轮——两条自然手势都通；纯纵向
/// 返回 0 = 观察器不动作，uniform_list 原生滚动不受扰）。Lines 形态
/// 经 `pixel_delta` 换算（行高基准 = ROW_H——同仓 gpui 惯例）。
pub fn wheel_delta_x(ev: &ScrollEvent) -> f32 {
    let d = ev.delta.pixel_delta(px(ROW_H));
    let dx = f32::from(d.x);
    if dx.abs() > f32::EPSILON {
        return dx;
    }
    if ev.modifiers.shift {
        f32::from(d.y)
    } else {
        0.0
    }
}

/// 网格体命中测试：局部坐标（网格体元素内，px）+ 滚动状态 → (行, 列)。
/// `first_visible_row` = 当前纵向首可见行（uniform_list 滚动偏移换算）。
/// 行号槽（x < GUTTER_W）/ 列间隙 / 越界 = None。
pub fn hit_test_cell(
    offsets: &[f32],
    widths: &[f32],
    scroll_x: f32,
    first_visible_row: usize,
    x: f32,
    y: f32,
) -> Option<(usize, usize)> {
    if x < GUTTER_W || x.is_nan() || y.is_nan() || y < 0.0 {
        return None;
    }
    let content_x = x - GUTTER_W + scroll_x;
    let n = offsets.len().min(widths.len());
    let col = (0..n).find(|&c| content_x >= offsets[c] && content_x < offsets[c] + widths[c])?;
    let row = first_visible_row + (y / ROW_H) as usize;
    Some((row, col))
}

// ---------------------------------------------------------------------------
// 视图（GPUI 层——编译面验证 + 真机登记）
// ---------------------------------------------------------------------------

/// XLSX 网格视图实体。
pub struct XlsxGridView {
    model: XlsxGridModel,
    /// 当前表（簿序索引）。
    sheet_idx: usize,
    /// 逐列宽（px——当前表；表切换时重算）。
    col_widths: Vec<f32>,
    /// 逐列 x 偏移（px——`col_offsets(col_widths)`）。
    col_offsets: Vec<f32>,
    /// 横向滚动位置（px——自管状态，`clamp_scroll_x` 钳制）。
    scroll_x: f32,
    /// 视口宽（px——最近一次消费值；首帧 0 = 未记录，下帧起算）。
    viewport_w: f32,
    /// 视口宽回写通道（覆盖 canvas paint 每帧回写——Rc<Cell> 零实体
    /// 更新零重入，pdf_pages viewport 探针同模式）。
    viewport: Rc<Cell<Option<f32>>>,
    /// 选中格（显示坐标 (行, 列)；None = 无选中）。
    selected: Option<(usize, usize)>,
    /// 纵向虚拟化滚动句柄。
    scroll: UniformListScrollHandle,
    /// 焦点域（键盘域占位——阶段 1 无键位面）。
    focus_handle: FocusHandle,
    /// 引擎面错误（构造期失败——面板明示，不白屏）。
    error: Option<String>,
}

impl XlsxGridView {
    /// 构造（模型就绪路径——`XlsxGridModel::load` 由 Shell 侧先行）。
    pub fn new(model: XlsxGridModel, cx: &mut App) -> Self {
        let focus_handle = cx.focus_handle();
        let mut this = Self {
            sheet_idx: 0,
            col_widths: Vec::new(),
            col_offsets: Vec::new(),
            scroll_x: 0.0,
            viewport_w: 0.0,
            viewport: Rc::new(Cell::new(None)),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle,
            error: None,
            model,
        };
        this.recompute_col_widths();
        this
    }

    /// error 态构造（模型装载失败——面板明示）。
    pub fn error_state(message: String, cx: &mut App) -> Self {
        Self {
            model: XlsxGridModel::default(),
            sheet_idx: 0,
            col_widths: Vec::new(),
            col_offsets: Vec::new(),
            scroll_x: 0.0,
            viewport_w: 0.0,
            viewport: Rc::new(Cell::new(None)),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            error: Some(message),
        }
    }

    /// 当前表（空簿 = error 态面板路径，不达此处）。
    fn sheet(&self) -> &GridSheet {
        self.model
            .sheets
            .get(self.sheet_idx)
            .or_else(|| self.model.sheets.first())
            .expect("空簿走 error 态（render 门）；构造期 model.sheets 空时同样")
    }

    /// 重算列宽/偏移（构造 + 表切换时——选中/横向滚动随表重置）。
    fn recompute_col_widths(&mut self) {
        let sheet = self.sheet();
        let mut widths = Vec::with_capacity(sheet.cols);
        for c in 0..sheet.cols {
            let content = sheet
                .cells
                .iter()
                .map(|row| display_width(row.get(c).map(String::as_str).unwrap_or("")))
                .max()
                .unwrap_or(0);
            widths.push(col_width_px(content));
        }
        self.col_offsets = col_offsets(&widths);
        self.col_widths = widths;
        self.selected = None;
        self.scroll_x = 0.0;
    }

    /// 切换工作表（簿序索引；越界/同表 = 无操作）。
    pub fn select_sheet(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.model.sheets.len() || idx == self.sheet_idx {
            return;
        }
        self.sheet_idx = idx;
        self.recompute_col_widths();
        cx.notify();
    }

    /// 当前表索引（状态面）。
    pub fn sheet_index(&self) -> usize {
        self.sheet_idx
    }

    /// 选中格（显示坐标 (行, 列)——阶段 2 编辑接驳锚）。
    pub fn selected_cell(&self) -> Option<(usize, usize)> {
        self.selected
    }

    /// 选中格的绝对 A1 引用（如 `B3`——显示坐标 → 绝对列字母 + 1 基
    /// 行号；无选中 = None）。
    pub fn selected_a1(&self) -> Option<String> {
        let sheet = self.sheet();
        let (r, c) = self.selected?;
        Some(format!(
            "{}{}",
            col_letter(sheet.start_col + c as u32),
            sheet.start_row + r as u32 + 1
        ))
    }

    /// 网格内容总宽（px）。
    fn total_width(&self) -> f32 {
        match (self.col_offsets.last(), self.col_widths.last()) {
            (Some(o), Some(w)) => o + w,
            _ => 0.0,
        }
    }

    /// 纵向首可见行（uniform_list 滚动偏移换算——pdf_pages
    /// `submit_visible` 同口径）。
    fn first_visible_row(&self) -> usize {
        let offset_y = f32::from(self.scroll.0.borrow().base_handle.offset().y).max(0.0);
        (offset_y / ROW_H) as usize
    }

    /// 横向滚动（滚轮 deltaX / Shift+deltaY——观察器透传；钳制域内）。
    fn scroll_horizontal(&mut self, dx: f32, cx: &mut Context<Self>) {
        if dx.abs() < f32::EPSILON {
            return;
        }
        self.scroll_x = clamp_scroll_x(self.scroll_x + dx, self.total_width(), self.viewport_w);
        cx.notify();
    }

    /// 点选收尾（命中测试 → 选中 + 越界钳制；行号槽点击 = 无操作）。
    fn select_at(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let sheet_rows = self.sheet().rows;
        let hit = hit_test_cell(
            &self.col_offsets,
            &self.col_widths,
            self.scroll_x,
            self.first_visible_row(),
            x,
            y,
        )
        .filter(|(r, _)| *r < sheet_rows);
        if hit.is_some() {
            self.selected = hit;
            cx.notify();
        }
    }

    /// 工作表切换行（簿序 tab——当前表高亮；溢出横滚）。
    fn render_sheet_tabs(&self, cx: &App) -> AnyElement {
        let colors = cx.theme().colors().clone();
        let mut tabs = h_flex()
            .id("xlsx-grid-sheet-tabs")
            .w_full()
            .h(px(28.0))
            .flex_none()
            .gap_1()
            .px_2()
            .bg(colors.background)
            .border_b_1()
            .border_color(colors.border)
            .overflow_x_scroll();
        for (idx, name) in self
            .model
            .sheets
            .iter()
            .map(|s| s.name.as_str())
            .enumerate()
        {
            let is_active = idx == self.sheet_idx;
            let selected = colors.element_selected;
            let hover = colors.element_hover;
            tabs =
                tabs.child(
                    div()
                        .id(SharedString::from(format!("xlsx-grid-tab-{idx}")))
                        .px_2()
                        .h(px(22.0))
                        .flex_none()
                        .rounded_sm()
                        .items_center()
                        .when(is_active, |el| el.bg(selected))
                        .when(!is_active, move |el| el.hover(move |el| el.bg(hover)))
                        .child(ui::Label::new(name).size(ui::LabelSize::XSmall).color(
                            if is_active {
                                ui::Color::Default
                            } else {
                                ui::Color::Muted
                            },
                        )),
                );
        }
        tabs.into_any_element()
    }

    /// 列头行元素（corner + 可见列窗口的字母格——绝对定位于行内，
    /// x = 偏移 - scroll_x；窗口外列零构建；corner 后置 = 盖住左滑
    /// 进 gutter 区的半可见列头（无 z_index——绘制序即层序））。
    fn render_col_header(&self, cx: &App) -> AnyElement {
        let colors = cx.theme().colors().clone();
        let (first, end) = visible_col_window(
            &self.col_offsets,
            &self.col_widths,
            self.scroll_x,
            self.viewport_w,
        );
        let sheet = self.sheet();
        let mut row = h_flex()
            .id("xlsx-grid-col-header")
            .relative()
            .w_full()
            .h(px(HEAD_H))
            .flex_none()
            .bg(colors.panel_background)
            .border_b_1()
            .border_color(colors.border);
        for c in first..end {
            let letter = col_letter(sheet.start_col + c as u32);
            let x = self.col_offsets[c] - self.scroll_x;
            let w = self.col_widths[c];
            let selected_col = self.selected.is_some_and(|(_, sc)| sc == c);
            row = row.child(
                div()
                    .id(SharedString::from(format!("xlsx-grid-col-{c}")))
                    .absolute()
                    .left(px(x + GUTTER_W))
                    .top_0()
                    .w(px(w))
                    .h_full()
                    .flex_none()
                    .items_center()
                    .px(px(CELL_PAD))
                    .when(selected_col, |el| el.bg(colors.element_hover))
                    .child(
                        ui::Label::new(letter)
                            .size(ui::LabelSize::XSmall)
                            .color(ui::Color::Muted),
                    ),
            );
        }
        // corner（左上固定块——后置 = 绘制在列头之上，盖住左滑半可见列）。
        row = row.child(
            div()
                .id("xlsx-grid-corner")
                .w(px(GUTTER_W))
                .h_full()
                .flex_none()
                .bg(colors.panel_background)
                .border_r_1()
                .border_color(colors.border),
        );
        row.into_any_element()
    }

    /// 行元素（uniform_list 回调——可见列窗口的单元格 + 行号槽；行内
    /// 绝对定位，窗口外零构建；选中格高亮边框 + 行号/列头联动底色；
    /// 行号槽后置 = 盖住左滑进槽区的半可见格（无 z_index——绘制序即
    /// 层序，列头 corner 同款）。
    fn render_row(&self, r: usize, cx: &App) -> AnyElement {
        let colors = cx.theme().colors().clone();
        let sheet = self.sheet();
        let (first, end) = visible_col_window(
            &self.col_offsets,
            &self.col_widths,
            self.scroll_x,
            self.viewport_w,
        );
        let row_no = sheet.start_row + r as u32 + 1; // 1 基显示
        let mut row = h_flex()
            .id(SharedString::from(format!("xlsx-grid-row-{r}")))
            .relative()
            .w_full()
            .h(px(ROW_H))
            .flex_none()
            .bg(colors.background);
        for c in first..end {
            let text = sheet
                .cells
                .get(r)
                .and_then(|row| row.get(c))
                .cloned()
                .unwrap_or_default();
            let x = self.col_offsets[c] - self.scroll_x;
            let w = self.col_widths[c];
            let is_selected = self.selected == Some((r, c));
            let border = colors.border;
            let accent = colors.element_selected;
            let hover = colors.element_hover;
            row = row.child(
                div()
                    .id(SharedString::from(format!("xlsx-grid-cell-{r}-{c}")))
                    .absolute()
                    .left(px(x + GUTTER_W))
                    .top_0()
                    .w(px(w))
                    .h_full()
                    .flex_none()
                    .items_center()
                    .px(px(CELL_PAD))
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(border)
                    .when(is_selected, move |el| {
                        el.border_1().border_color(accent).bg(hover)
                    })
                    .child(
                        ui::Label::new(text)
                            .size(ui::LabelSize::XSmall)
                            .color(ui::Color::Default),
                    ),
            );
        }
        // 行号槽（左固定——后置 = 绘制在单元格之上，盖住左滑半可见格）。
        let selected_row = self.selected.is_some_and(|(sr, _)| sr == r);
        row = row.child(
            div()
                .id(SharedString::from(format!("xlsx-grid-gutter-{r}")))
                .w(px(GUTTER_W))
                .h_full()
                .flex_none()
                .items_center()
                .justify_end()
                .pr_2()
                .bg(if selected_row {
                    colors.element_hover
                } else {
                    colors.panel_background
                })
                .border_r_1()
                .border_color(colors.border)
                .child(
                    ui::Label::new(row_no.to_string())
                        .size(ui::LabelSize::XSmall)
                        .color(ui::Color::Muted),
                ),
        );
        row.into_any_element()
    }
}

impl Focusable for XlsxGridView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for XlsxGridView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 消费覆盖 canvas 上帧记录的视口宽（pdf_pages viewport 探针同
        // 模式——Rc<Cell> 零重入）。
        if let Some(w) = self.viewport.get() {
            if w > 0.0 {
                self.viewport_w = w - GUTTER_W; // 网格体内容宽（扣除行号槽）
            }
        }
        let colors = cx.theme().colors().clone();
        // error 态 / 空簿：明示面板（不白屏——pdf_pages 同纪律）。
        if self.error.is_some() || self.model.sheets.is_empty() {
            let message = self
                .error
                .clone()
                .unwrap_or_else(|| "空工作簿（无工作表）".into());
            return v_flex()
                .id("xlsx-grid-error")
                .size_full()
                .items_center()
                .justify_center()
                .bg(colors.background)
                .child(ui::Label::new(message).color(ui::Color::Warning))
                .child(
                    ui::Label::new("网格视图不可用——文本行视图仍可查看（状态栏切换）")
                        .size(ui::LabelSize::Small)
                        .color(ui::Color::Muted),
                )
                .into_any_element();
        }
        let sheet_tabs = self.render_sheet_tabs(cx);
        let col_header = self.render_col_header(cx);
        let rows = self.sheet().rows;
        let truncated = self.sheet().truncated;
        let sheet_name = self.sheet().name.clone();
        // 行回调（uniform_list 每帧重建可见行——窗口实体 downgrade）。
        let entity = cx.entity().downgrade();
        let list = gpui::uniform_list(
            "xlsx-grid-rows",
            rows,
            move |visible: std::ops::Range<usize>, _window: &mut Window, cx: &mut App| {
                visible
                    .clone()
                    .filter_map(|r| {
                        let slot = entity.update(cx, |view, cx| view.render_row(r, cx)).ok()?;
                        Some(slot)
                    })
                    .collect::<Vec<AnyElement>>()
            },
        )
        .track_scroll(&self.scroll)
        .h_full();
        // 截断护栏提示（非静默——xlsx_doc 同口径）。
        let truncation_banner = truncated.then(|| {
            div()
                .id("xlsx-grid-truncated")
                .flex_none()
                .w_full()
                .px_2()
                .py_1()
                .bg(colors.panel_background)
                .border_t_1()
                .border_color(colors.border)
                .child(
                    ui::Label::new(format!(
                        "「{sheet_name}」超出渲染护栏（行 ≤{MAX_RENDER_ROWS} / 列 ≤{MAX_RENDER_COLS}）——网格视图已截断，完整内容见文本视图"
                    ))
                    .size(ui::LabelSize::XSmall)
                    .color(ui::Color::Warning),
                )
        });
        // 横向滚动指示条（底部——scroll_x / 总宽的比例条；内容窄于
        // 视口不占位；thumb 位置/宽按已记录的视口宽换算 px）。
        let total_w = self.total_width();
        let track_w = self.viewport_w;
        let h_scroll = (total_w > self.viewport_w && track_w > 0.0).then(|| {
            let ratio = (self.viewport_w / total_w).clamp(0.05, 1.0);
            let thumb_w = ratio * track_w;
            let thumb_x = (self.scroll_x / (total_w - self.viewport_w).max(1.0))
                * (track_w - thumb_w).max(0.0);
            div()
                .id("xlsx-grid-h-scroll")
                .relative()
                .flex_none()
                .w_full()
                .h(px(6.0))
                .bg(colors.panel_background)
                .border_t_1()
                .border_color(colors.border)
                .child(
                    div()
                        .id("xlsx-grid-h-thumb")
                        .absolute()
                        .top_0()
                        .left(px(thumb_x))
                        .w(px(thumb_w))
                        .h_full()
                        .rounded_sm()
                        .bg(colors.border),
                )
        });
        // 覆盖 canvas：paint 期注册窗口级滚轮横扫 + 点击命中观察器
        //（bounds 过滤——只落网格体内才动作；不占命中区不挡列表滚轮；
        // pdf_pages annotate_overlay 同模式）+ 视口宽回写（Rc<Cell>）。
        let ent = cx.entity().downgrade();
        let viewport_cell = self.viewport.clone();
        let overlay = div()
            .id("xlsx-grid-overlay")
            .absolute()
            .size_full()
            .child(gpui::canvas(
                move |bounds, _window, _cx| bounds,
                move |bounds: gpui::Bounds<Pixels>, _t, window: &mut Window, _cx: &mut App| {
                    // 视口宽回写（本帧 bounds——render 期消费）。
                    viewport_cell.set(Some(f32::from(bounds.size.width)));
                    let ent_wheel = ent.clone();
                    let ent_click = ent.clone();
                    // 滚轮横扫：deltaX / Shift+deltaY → scroll_x（纯纵向
                    // 不动作——uniform_list 原生滚动不受扰）。
                    window.on_mouse_event(
                        move |ev: &ScrollEvent,
                              phase: DispatchPhase,
                              _w: &mut Window,
                              cx: &mut App| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let dx = wheel_delta_x(ev);
                            if dx.abs() < f32::EPSILON {
                                return;
                            }
                            let _ = ent_wheel.update(cx, |view, cx| {
                                view.scroll_horizontal(dx, cx);
                            });
                        },
                    );
                    // 点击命中：局部坐标 → (行, 列) 选中（bounds 内才动作）。
                    window.on_mouse_event(
                        move |ev: &gpui::MouseDownEvent,
                              phase: DispatchPhase,
                              _w: &mut Window,
                              cx: &mut App| {
                            if phase != DispatchPhase::Bubble
                                || ev.button != gpui::MouseButton::Left
                            {
                                return;
                            }
                            let in_x = ev.position.x >= bounds.origin.x
                                && ev.position.x <= bounds.origin.x + bounds.size.width;
                            let in_y = ev.position.y >= bounds.origin.y
                                && ev.position.y <= bounds.origin.y + bounds.size.height;
                            if !(in_x && in_y) {
                                return;
                            }
                            let x = f32::from(ev.position.x - bounds.origin.x);
                            let y = f32::from(ev.position.y - bounds.origin.y);
                            let _ = ent_click.update(cx, |view, cx| {
                                view.select_at(x, y, cx);
                            });
                        },
                    );
                },
            ));
        v_flex()
            .id("xlsx-grid-root")
            .relative()
            .size_full()
            .min_w_0()
            .bg(colors.background)
            .child(sheet_tabs)
            .child(col_header)
            .child(
                v_flex()
                    .id("xlsx-grid-body")
                    .flex_1()
                    .min_h_0()
                    .child(list)
                    .children(h_scroll),
            )
            .children(truncation_banner)
            .child(overlay)
            .into_any_element()
    }
}

// ---------------------------------------------------------------------------
// 纯函数面单测（零 GPUI 依赖——CI 无 GUI 同样真跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn col_letter_sequence_and_wrap() {
        assert_eq!(col_letter(0), "A");
        assert_eq!(col_letter(25), "Z");
        assert_eq!(col_letter(26), "AA");
        assert_eq!(col_letter(27), "AB");
        assert_eq!(col_letter(701), "ZZ");
        assert_eq!(col_letter(702), "AAA");
    }

    #[test]
    fn display_width_ea_aware() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("中文"), 4, "CJK 双宽");
        assert_eq!(display_width("a中b"), 4, "混合");
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn col_width_px_clamps() {
        assert_eq!(col_width_px(0), COL_W_MIN, "空列 = 下限");
        assert_eq!(col_width_px(3), COL_W_MIN, "短内容仍下限");
        // 封顶内容（24 cell）的自然宽 = 24×7.5+12 = 192 = 上限。
        assert_eq!(col_width_px(24), COL_W_MAX, "封顶内容 = 上限");
        assert_eq!(
            col_width_px(200),
            COL_W_MAX,
            "超宽内容 = 上限（CELL_WIDTH_CAP 截断面）"
        );
    }

    #[test]
    fn col_offsets_cumulative_with_gap() {
        let offs = col_offsets(&[100.0, 50.0, 80.0]);
        assert_eq!(offs, vec![0.0, 101.0, 152.0], "含 COL_GAP 累计");
    }

    #[test]
    fn visible_col_window_basic() {
        let widths = vec![100.0; 6];
        let offs = col_offsets(&widths);
        // 视口 300 @ scroll 0 → 列 0..3（列 2 右缘 300 边界算可见）。
        let (f, e) = visible_col_window(&offs, &widths, 0.0, 300.0);
        assert_eq!((f, e), (0, 3), "首窗口");
        // scroll 202（列 2 左缘 202）→ 列 2..5（505 右缘钳在窗口内）。
        let (f, e) = visible_col_window(&offs, &widths, 202.0, 300.0);
        assert_eq!((f, e), (2, 5), "中窗口");
        // 滚到最右：总宽 605，视口 300 → scroll max 305 → 窗口 3..6。
        let (f, e) = visible_col_window(&offs, &widths, 305.0, 300.0);
        assert_eq!((f, e), (3, 6), "末窗口");
        // 超界 scroll = 零窗口（调用方 clamp 前置——契约防御面）。
        let (f, e) = visible_col_window(&offs, &widths, 1e9, 300.0);
        assert_eq!((f, e), (0, 0), "超界 scroll = 零窗口");
        // 空表。
        let (f, e) = visible_col_window(&[], &[], 0.0, 300.0);
        assert_eq!((f, e), (0, 0), "零列 = 零窗口");
    }

    #[test]
    fn clamp_scroll_x_domain() {
        assert_eq!(clamp_scroll_x(-50.0, 1000.0, 300.0), 0.0, "负值钳 0");
        assert_eq!(
            clamp_scroll_x(900.0, 1000.0, 300.0),
            700.0,
            "上限 = 总宽-视口"
        );
        assert_eq!(clamp_scroll_x(50.0, 100.0, 300.0), 0.0, "内容窄于视口 = 0");
        assert_eq!(clamp_scroll_x(f32::NAN, 1000.0, 300.0), 0.0, "NaN 防御");
    }

    #[test]
    fn hit_test_cell_resolves_and_rejects() {
        let widths = vec![100.0, 80.0];
        let offs = col_offsets(&widths);
        // scroll 0：x=GUTTER_W+10 → 列 0；y=45 → 行 2（首可见 0）。
        assert_eq!(
            hit_test_cell(&offs, &widths, 0.0, 0, GUTTER_W + 10.0, 45.0),
            Some((2, 0))
        );
        // 列 1（x = GUTTER_W + 101..180）。
        assert_eq!(
            hit_test_cell(&offs, &widths, 0.0, 0, GUTTER_W + 105.0, 0.0),
            Some((0, 1))
        );
        // 行号槽（x < GUTTER_W）= None。
        assert_eq!(hit_test_cell(&offs, &widths, 0.0, 0, 10.0, 10.0), None);
        // 列间隙（x = GUTTER_W + 100.5 在 100..101 间隙）= None。
        assert_eq!(
            hit_test_cell(&offs, &widths, 0.0, 0, GUTTER_W + 100.5, 10.0),
            None
        );
        // scroll_x=101：列 0 左移出窗 → x=GUTTER_W+10 命中列 1。
        assert_eq!(
            hit_test_cell(&offs, &widths, 101.0, 0, GUTTER_W + 10.0, 0.0),
            Some((0, 1))
        );
        // 首可见行偏移（纵向滚过 3 行）：y=0 → 行 3。
        assert_eq!(
            hit_test_cell(&offs, &widths, 0.0, 3, GUTTER_W + 10.0, 0.0),
            Some((3, 0))
        );
        // 负 y / NaN 防御。
        assert_eq!(
            hit_test_cell(&offs, &widths, 0.0, 0, GUTTER_W + 10.0, -5.0),
            None
        );
        assert_eq!(hit_test_cell(&offs, &widths, 0.0, 0, f32::NAN, 0.0), None);
    }

    #[test]
    fn cell_display_formula_fallback() {
        use mdc_office_io::io::CellData;
        assert_eq!(
            cell_display(Some(&CellData::Empty), Some("SUM(A1:A2)")),
            "=SUM(A1:A2)"
        );
        assert_eq!(cell_display(Some(&CellData::Number(1.5)), None), "1.5");
        assert_eq!(cell_display(None, None), "");
    }
}
