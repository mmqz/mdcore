//! 分页预估（issue #29 N-5）——导出前超页判断的引擎级单一真源。
//!
//! # 为什么在引擎
//!
//! 与字数统计（[`crate::word_count`]）同构的裁决：块树持有全部内容与
//! 结构真相，页数口径必须钉在核心——任何视图/导出侧自行估算都会口径
//! 漂移。宿主/wasm 只消费不重算。
//!
//! # 预测目标（口径声明）
//!
//! 估算对象 = **本引擎自家导出链**（BlockTree →
//! `mdcore-wasm::docx_export::export_docx_styled_core` → office-io
//! G2 写器 → LibreOffice 渲染）的落纸页数。校准 = 双层实测（2026-10-28，
//! LO 25.2.3.2 + Noto CJK 字体面，`test-artifacts/pagination/`）：
//!
//! - **结构层**（微件差分——9 个受控文档逐种类钉定）：正文行高 =
//!   `字号 × (BODY_LINE_A + BODY_LINE_B × 行距倍数)`（LO 比例行距
//!   缩放 ascent/descent 面，lineGap 面不缩放——A/B 两常数承载）；
//!   等宽（代码）行高 = `字号 × MONO_LINE_FACTOR × 倍数`（Courier/
//!   Liberation Mono 度量矮于 CJK 面）；表格行 = 正文行高 +
//!   [`TABLE_ROW_OVERHEAD`]；列表/引用/标点密度零额外高度（实测
//!   微件与纯段同页数——缩进在 2 行段落粒度不显影）。
//! - **聚合层**（40 万字夹具双档收敛）：引擎缺省档 +2.7% / 中文标书
//!   档 -1.1%（N-1/N-5 同战役闭环——N-1 供真值，N-5 收敛误差）。
//!
//! # 模型（v2——与当前导出链逐点对齐）
//!
//! - **全块同字号**：Heading1-9 样式仅 outline 级（无字体/字号面——
//!   `DocxWriter::new` 注释「模型级边界」），LO/Word 渲染标题继承
//!   Normal = 正文字号 → 标题按正文行计（短标题恒 1 行）；
//! - **每行字数**（全角单位）= 版心宽 ÷ 字号宽度（CJK 全角字符宽
//!   恰 = 1em = 字号——Noto/仿宋/宋体同口径；半角字符按 0.5 计）；
//! - **行高**：正文 [`BODY_LINE_A`]+[`BODY_LINE_B`]×倍数、等宽
//!   [`MONO_LINE_FACTOR`]×倍数、表格行 + [`TABLE_ROW_OVERHEAD`]
//!   ——单位均为 em（×字号 = twips）；
//! - 块级行消耗：段落 = ceil(全角单位 ÷ 有效每行字数)（空段 = 1）；
//!   列表项 = 同段落但每行字数 -2（标记位近似）；引用 = 同段落但
//!   版心缩进 [`QUOTE_INDENT_STEP`]×深度；代码块 = 字面行数（等宽
//!   不折行近似）；表格 = 逐行「单元格折行数最大值」（单元格有效
//!   宽 = 版心宽÷列数 - [`TABLE_CELL_MARGIN`]）；
//! - **页数** = 各类行高合计 ÷ 版心高（单一流模型——段落孤行控制
//!   的页尾浪费 ≈ 每页 1 行 ≈ 2% 偏低估，误差带吸收）。
//!
//! # 精度口径
//!
//! 验收（N-5）：40 万字文档估算页数 vs LO 实际页数偏差 ≤5%
//! （实测：缺省档 +2.7%、标书档 -1.1%）。[`PageEstimate::error_margin_pct`]
//! = 模型自报误差带（按块构成分档：纯段落 ≈5；重结构[表格/代码行占比
//! >20%] ≈10——单元格折行与等宽替换的不确定面放大）。
//!
//! # 三域纪律
//!
//! 纯核心模块（零 office-io 依赖）：排版参数以 [`PaginationParams`]
//! 平面结构承载（twips/倍数——引擎域口径）；office-io
//! `DefaultTypography`/`PageGeometry` → 本结构的映射在 wasm 桥
//! （`mdcore-wasm` `docx_export::estimate_pages_core`——消费方依赖
//! 方向，见 REUSE.md §0）。

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::inline::parse_inline_marks;
use crate::tree::{BlockKind, BlockTree, MarkSpan, NodeId};
use crate::word_count::is_cjk_ideograph;

/// 正文行高常数 A（em，不随行距倍数缩放的面——LO 比例行距只缩放
/// ascent/descent，lineGap/外部行距不缩放；微件差分实测钉定）。
/// 行高 = 字号 × (A + [`BODY_LINE_B`] × 倍数)。
pub const BODY_LINE_A: f64 = 0.3115;

/// 正文行高常数 B（em × 行距倍数——比例缩放面）。
pub const BODY_LINE_B: f64 = 1.15;

/// 等宽（代码）行高系数（em × 倍数——Courier/Liberation Mono 度量
/// 矮于 CJK 字体面；微件差分实测）。
pub const MONO_LINE_FACTOR: f64 = 1.17;

/// 列表项缩进（twips/级——numbering 面左缩进近似）。
pub const LIST_INDENT_STEP: u32 = 720;

/// 引用块缩进（twips/深度——与写面 `QUOTE_INDENT_STEP` 同值）。
pub const QUOTE_INDENT_STEP: u32 = 720;

/// 表格单元格水平边距合计（twips——docx-rs tblCellMar 左右各 108）。
pub const TABLE_CELL_MARGIN: u32 = 216;

/// 表格行垂直开销（twips/行——边框 + LO 单元格上下留白，微件差分实测）。
pub const TABLE_ROW_OVERHEAD: u32 = 25;

/// 正文行高（twips）= 字号 × (A + B × 倍数)。
pub fn body_line_height(size_twips: u32, multiple: f64) -> f64 {
    size_twips as f64 * (BODY_LINE_A + BODY_LINE_B * multiple)
}

/// 等宽行高（twips）= 字号 × MONO × 倍数。
pub fn mono_line_height(size_twips: u32, multiple: f64) -> f64 {
    size_twips as f64 * MONO_LINE_FACTOR * multiple
}

/// 分页预估排版参数（平面结构——引擎域口径；wasm 桥负责从 office-io
/// 类型映射，见模块文档「三域纪律」）。单位：twips（1/20 pt）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaginationParams {
    /// 版心宽（页宽 - 左右边距 - 装订线）。
    pub text_width: u32,
    /// 版心高（页高 - 上下边距）。
    pub text_height: u32,
    /// 正文字号（twips = 半磅 × 10：四号 14pt = 280）。
    pub body_size_twips: u32,
    /// 行距倍数（auto 规则：1.0 单倍 / 1.5 / 2.0）。
    pub line_multiple: f64,
    /// 段前距（twips——docDefaults before）。
    pub spacing_before: u32,
    /// 段后距（twips——docDefaults after）。
    pub spacing_after: u32,
}

impl PaginationParams {
    /// 引擎导出链缺省档（A4 + 边距 1985/1701 + 10pt + 单倍行距——
    /// 与不传选项的 `export_docx` 产物基线一致）。
    pub fn engine_default() -> Self {
        Self {
            text_width: 11906 - 1701 - 1701,
            text_height: 16838 - 1985 - 1701,
            body_size_twips: 200,
            line_multiple: 1.0,
            spacing_before: 0,
            spacing_after: 0,
        }
    }
}

/// 分页预估结果（全部字段确定性输出——同输入同输出，无随机面）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageEstimate {
    /// 估算页数（各类行高合计 ÷ 版心高，向上取整；最小 1）。
    pub pages: u32,
    /// 正文每行全角字数（版心宽 ÷ 字号宽度，向下取整）。
    pub chars_per_line: u32,
    /// 每页行数（版心高 ÷ 正文行高，向下取整）。
    pub lines_per_page: u32,
    /// 段落行消耗（含列表项/引用内段落）。
    pub paragraph_lines: u32,
    /// 标题行消耗（当前链标题 = 正文行计）。
    pub heading_lines: u32,
    /// 表格行消耗（逐表格行的行数最大值合计）。
    pub table_lines: u32,
    /// 代码块行消耗（字面行数）。
    pub code_lines: u32,
    /// 模型自报误差带（±%——按块构成分档：纯段落 ≈5；重表格/代码
    /// 档 ≈10——校准残差实测，非承诺下限）。
    pub error_margin_pct: f64,
}

/// 全角字符判定（宽度 = 1em = 字号）：CJK 统一表意（含扩展 A）+
/// CJK 标点（U+3000 区段）+ 全角形式（U+FF00 区段）+ 中文习惯弯引号
/// /破折号/省略号（U+2013-2026 弯引号与 U+2014/U+2026——宋体/Noto
/// 均全角面）。
fn is_full_width(c: char) -> bool {
    let cp = c as u32;
    if is_cjk_ideograph(c) {
        return true;
    }
    matches!(cp,
        0x3000..=0x303F      // CJK 标点（。，、；：？！「」『』）
        | 0xFF00..=0xFF60    // 全角形式（！＂＃…￠）
        | 0x2018 | 0x2019 | 0x201C | 0x201D  // ‘’“”
        | 0x2013 | 0x2014    // – —
        | 0x2026             // …
        | 0x00B7             // ·（间隔号——中文排印全角位）
    )
}

/// 文本的全角宽度单位（半角 0.5）。
fn width_units(s: &str) -> f64 {
    s.chars()
        .map(|c| if is_full_width(c) { 1.0 } else { 0.5 })
        .sum()
}

/// 净文本投影（与 word_count 同规：marks 空 = 解析剥定界符，否则原文）。
fn net_text<'a>(text: &'a str, marks: &[MarkSpan]) -> Cow<'a, str> {
    if marks.is_empty() {
        Cow::Owned(parse_inline_marks(text).0)
    } else {
        Cow::Borrowed(text)
    }
}

/// 指定有效版心宽下的行数（全角单位折行；空文本 = 1 行——空段占位）。
fn wrapped_lines(units: f64, chars_per_line: f64) -> u32 {
    if units <= 0.0 {
        return 1;
    }
    (units / chars_per_line).ceil() as u32
}

/// 行消耗累加器。
#[derive(Default)]
struct LineBudget {
    paragraph_lines: u32,
    heading_lines: u32,
    table_lines: u32,
    code_lines: u32,
    /// 段计数（段距按段计非按行计——before/after 是段落属性）。
    paragraph_count: u32,
}

fn walk(
    tree: &BlockTree,
    id: NodeId,
    params: &PaginationParams,
    indent_twips: u32,
    b: &mut LineBudget,
) {
    let node = tree.node(id);
    let font_w = params.body_size_twips as f64;
    let cpl = ((params.text_width.saturating_sub(indent_twips)) as f64 / font_w).floor();
    // 每行至少 1 全角位（极窄版心防御——cpl ≥ 1 不除零）。
    let cpl = cpl.max(1.0);
    match node.kind {
        BlockKind::Document => {
            for &c in &node.children {
                walk(tree, c, params, indent_twips, b);
            }
        }
        BlockKind::Paragraph => {
            let text = node.text.to_string();
            let net = net_text(&text, &node.marks);
            b.paragraph_lines += wrapped_lines(width_units(&net), cpl);
            b.paragraph_count += 1;
        }
        BlockKind::Heading => {
            let text = node.text.to_string();
            let net = net_text(&text, &node.marks);
            // 当前链：Heading1-9 无字号面 → 正文行计（模型文档对齐点）。
            b.heading_lines += wrapped_lines(width_units(&net), cpl);
            b.paragraph_count += 1;
        }
        BlockKind::ListItem => {
            let text = node.text.to_string();
            let net = net_text(&text, &node.marks);
            // 列表标记位（"1. "/"● " ≈ 1-2 全角位）并入首行近似——
            // 与缩进合并为恒减 2 全角位（保守偏高，误差带吸收）。
            let cpl_li = (cpl - 2.0).max(1.0);
            b.paragraph_lines += wrapped_lines(width_units(&net), cpl_li);
            b.paragraph_count += 1;
            for &c in &node.children {
                walk(tree, c, params, indent_twips + LIST_INDENT_STEP, b);
            }
        }
        BlockKind::Blockquote => {
            if node.children.is_empty() {
                let text = node.text.to_string();
                let net = net_text(&text, &node.marks);
                b.paragraph_lines += wrapped_lines(width_units(&net), cpl);
                b.paragraph_count += 1;
            }
            for &c in &node.children {
                walk(tree, c, params, indent_twips + QUOTE_INDENT_STEP, b);
            }
        }
        BlockKind::CodeBlock => {
            // 等宽不折行近似：字面行数即行消耗（与导出面 code_line
            // 逐行发射一致）。
            for _ in node.text.to_string().split('\n') {
                b.code_lines += 1;
            }
        }
        // 数学块同构 CodeBlock：LaTeX 源逐字，字面行数即行消耗
        // （#261——导出面同构降级为等宽逐行，分页口径与发射对齐）。
        BlockKind::Math => {
            for _ in node.text.to_string().split('\n') {
                b.code_lines += 1;
            }
        }
        BlockKind::Table => {
            b.table_lines += table_line_count(&node.text.to_string(), params);
        }
        BlockKind::ThematicBreak => {
            // 分隔线 = 1 行（导出面空段近似）。
            b.paragraph_lines += 1;
        }
    }
}

/// 表格块文本 → 行消耗：逐管道行（空白行跳过、GFM 分隔行剔除——
/// 与 word_count::count_table 同规）取「单元格折行数最大值」。
fn table_line_count(text: &str, params: &PaginationParams) -> u32 {
    let font_w = params.body_size_twips as f64;
    let mut rows_emitted = 0usize;
    let mut total = 0u32;
    for line in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        if line.trim().is_empty() {
            continue;
        }
        let cells = crate::word_count::split_pipe_row_pub(line);
        if rows_emitted == 1 && crate::word_count::is_separator_row_pub(&cells) {
            continue;
        }
        rows_emitted += 1;
        let ncols = cells.len().max(1);
        // 单元格有效宽 = 版心宽÷列数 - 边距；每行至少 1 位防御。
        let cell_cpl = (((params.text_width as f64 / ncols as f64) - TABLE_CELL_MARGIN as f64)
            / font_w)
            .floor()
            .max(1.0);
        let row_lines = cells
            .iter()
            .map(|c| {
                let net = net_text(c, &[]);
                wrapped_lines(width_units(&net), cell_cpl)
            })
            .max()
            .unwrap_or(1);
        total += row_lines;
    }
    total
}

/// 块树分页预估（模块文档模型——确定性输出）。
pub fn estimate(tree: &BlockTree, params: &PaginationParams) -> PageEstimate {
    let font_w = params.body_size_twips as f64;
    let chars_per_line = ((params.text_width as f64) / font_w).floor().max(1.0) as u32;
    let lh_body = body_line_height(params.body_size_twips, params.line_multiple);
    let lh_mono = mono_line_height(params.body_size_twips, params.line_multiple);
    let lines_per_page = ((params.text_height as f64) / lh_body).floor().max(1.0) as u32;

    let mut b = LineBudget::default();
    for &child in tree.top_level() {
        walk(tree, child, params, 0, &mut b);
    }
    // 单一流高度合计：正文类行 × 正文行高 + 代码行 × 等宽行高 +
    // 表格行 × (正文行高 + 行开销) + 段距（按段计非按行计）。
    let spacing_twips =
        b.paragraph_count as f64 * (params.spacing_before + params.spacing_after) as f64;
    let total_height = b.paragraph_lines as f64 * lh_body
        + b.heading_lines as f64 * lh_body
        + b.code_lines as f64 * lh_mono
        + b.table_lines as f64 * (lh_body + TABLE_ROW_OVERHEAD as f64)
        + spacing_twips;
    let pages = ((total_height / params.text_height as f64).ceil() as u32).max(1);

    // 误差带分档：重结构（表格/代码行占比 > 20%）放大至 10%。
    let total_lines = b.paragraph_lines + b.heading_lines + b.table_lines + b.code_lines;
    let structural_ratio = (b.table_lines + b.code_lines) as f64 / (total_lines as f64).max(1.0);
    let error_margin_pct = if structural_ratio > 0.20 { 10.0 } else { 5.0 };

    PageEstimate {
        pages,
        chars_per_line,
        lines_per_page,
        paragraph_lines: b.paragraph_lines,
        heading_lines: b.heading_lines,
        table_lines: b.table_lines,
        code_lines: b.code_lines,
        error_margin_pct,
    }
}
