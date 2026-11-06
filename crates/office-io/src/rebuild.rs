//! 读-改-重建（C3 唯一允许口径——`write` 模块文档「修改已有工作簿
//! 不宣称支持」的产品化替代路径）。
//!
//! ## 定位
//!
//! [`rebuild_book`]：读面 [`Book`]（+ 单格编辑账本 [`CellEdit`]）→
//! 经产品写 API [`crate::write::WorkbookWriter`] 全量重放 → 新簿字节。
//! 保真面 = 表序/值/公式文本（含缓存值配对）/合并/超链接（FIDELITY
//! v2 五档矩阵定谳）；**结构性丢失**（样式原档/文档元数据/列宽行高
//! 等）由 [`RebuildStats::skipped`] 显式登记 + 调用方透传宿主——
//! 非静默降级纪律。
//!
//! ## 单格联接点
//!
//! 值面主路径 = [`crate::io::SheetData::replay_cell`]（r4-t1 单一
//! 安全调用点：kind 分派 + 公式两通道联接[绝对坐标] + 日期 numFmt
//! 样式配对成对返回防漏配）。重放器**不重复实现**配对逻辑。
//!
//! ## 编辑账本
//!
//! [`CellEdit`] 按 `(sheet, row, col)` 覆盖值面（后写胜——账本序
//! 即命令序）；编辑格**超出使用区**时扩展写入（编辑账本先行分组，
//! 值面循环消费组内项，窗余项循环后补写）。编辑值纪律：
//! - `Text` 含 U+FFFE/U+FFFF（XML 1.0 非法非字符，写引擎内建
//!   `_xHHHH_` 转义仅覆盖 `\x00-\x1F` 面）→ typed 拒
//!   （[`crate::write::WriteError::InvalidInput`]，消息含坐标）；
//! - 控制字符 `\x00-\x1F` **不预转义**——依赖 rust_xlsxwriter 引擎
//!   内建同约定转义（源级一手核验，write 模块文档在档；预转义会
//!   双重编码失真——`_x000B_` 被引擎字面消歧再转义）。
//!
//! ## 重算面（issue #113 任务 1）
//!
//! [`RebuildOptions::recalculate`]（默认 true）：编辑账本落位后经
//! mdc-formula（#112 引入的 ironcalc_base 包装）全簿重算，公式格
//! 缓存值以引擎结果更新后落盘——「改了单元格，依赖它的公式不变；
//! 导出带旧缓存值」缺口闭合。
//!
//! 机制：灌入面 = 值面（typed 类型化 + 1904 纪元归一）+ 公式面
//! （`=` 前缀补全）+ 编辑账本（编辑即落位——构建 + 显式
//! [`FormulaEngine::recalculate`] 一次全簿重算，免 set_cell 二段式；
//! 引擎面实测 200k 格全簿重算 release 0.018s，脏标记优化在 rebuild
//! 面无预算意义）。产物 = 公式格新缓存表，值面重放时**原位替换** [`crate::write::CellValue::Formula`] 的
//! `cached`（公式文本与配对样式保持——DateTime 缓存的 numFmt 配对
//! 不被重算破坏；重放器不重复实现配对逻辑的纪律延续）。
//!
//! 降级纪律（保真优先——重算失败**绝不阻断**重建）：
//! - 引擎构建失败（非法/重复表名、超 [`mdc_formula::LIMIT_TOTAL_CELLS`]
//!   预算）→ 整簿源缓存直通 + skipped 登记；
//! - 单格求值 Err（解析失败/表名不存在——**外部引用**
//!   `[Book1]Sheet1!A1` 属此类）→ 该格源缓存保持 + skipped 登记
//!   （逐格封顶 20 条 + 总数汇总）；
//! - 错误常量格（无公式的 `t="e"` 格）→ 引擎无错误常量输入面，
//!   排除灌入 + 登记（引用它的公式按空格语义求值）。
//!
//! 能力边界（实测钉定见 tests/rebuild_recalc.rs）：
//! - 循环引用：引擎 CIRC 检测 → 缓存 `#CIRC!` 码文本（引擎扩展码
//!   如实落盘——非 Excel 默认迭代关闭时的 0 显示）；
//! - 非重算模式（`recalculate: false`）：纯公式格（无缓存值的 `<f>`
//!   无 `<v>` 格——calamine values 面不收，探针钉定）在 typed 使用区
//!   外不写 = **历史静默丢失行为保持**；重算模式的 typed ∪ formulas
//!   並集扩展闭合此缺口（含编辑扩展区补灌）。
//!
//! ## 防线
//!
//! - 表名：`add_sheet` 对非法/重复名 typed 拒——重放器先行清洗
//!   （与 mdcore-wasm xlsx_export 同型：控制/非法字符→空格、剥
//!   `[]:*?/\`、trim + 首尾单引号、截 31、**重 trim**、空/History
//!   回退）+ 去重（` {n}` 后缀、基底重截保总长 ≤31 恒可终止）。
//!   **表名清洗即 skipped 登记**（保真例外显式化）。
//! - 超链接：URL > `MAX_URL_TEXT` / tooltip > `MAX_TOOLTIP_TEXT` /
//!   含 NUL → **跳过 + skipped 登记**（源档敌意面防御——不可信
//!   读数据不静默入写面）。
//! - 合并：单格范围（start==end，Excel 不允许）或列越界（> `MAX_COL`）
//!   → 跳过 + 登记；**非 Text 左上 → 文本扁平化 + 登记**（合并面
//!   仅字符串——保真例外显式化）。
//! - 坐标域：**值面/编辑格/合并与超链接的行坐标越界（> `MAX_ROW`）
//!   及值面列越界 = typed 拒**（`rebuild_invalid`——响亮非静默；
//!   行列越界罕见且非跳过可修复面）；合并/超链接的**列**越界走
//!   跳过登记（跳过单部件不断整簿）。
//! - 预算：写面 `MAX_CELLS`（全簿累计）→ [`crate::write::WriteError::
//!   Limit`] 自然传播（读面 4M 格 > 写面 1M 格的已知不对称——部分
//!   写入后失败 = 无产物字节，错误即回滚语义）。
//!
//! ## 错误面
//!
//! 复用 [`crate::write::WriteError`] 三变体（InvalidInput 含编辑
//! FFFE/FFFF 拒）；wasm 层映射 `rebuild_invalid` / `rebuild_limit` /
//! `rebuild_engine`（mdcore-wasm xlsx_grid）。

use std::collections::HashMap;

use mdc_formula::{CellContent, FormulaEngine, SheetInput, Value as FormulaValue};

use crate::io::{epoch_to_1900, Book, CellData, SheetData};
use crate::write::{
    FormulaCache, HyperlinkSpec, WorkbookWriter, WriteError, MAX_COL, MAX_TOOLTIP_TEXT,
    MAX_URL_TEXT,
};

/// 单格编辑值（命令账本载荷——kind 指导 typed 写入）。
#[derive(Debug, Clone, PartialEq)]
pub enum EditValue {
    Text(String),
    Number(f64),
    Boolean(bool),
    /// 公式（带/不带 `=` 前缀——写面规范化同 [`crate::write::
    /// CellValue::Formula`]；重算面补 `=` 灌入，构建即首算得缓存）。
    /// wasm JSON 协议面（mdcore-wasm parse_edits_core 的 k ∈ text/
    /// number/bool/empty）尚未暴露 formula 形态——宿主侧后续增量。
    Formula(String),
    /// 清空（值面重放跳过该格 + 扩展区写入跳过 = 格缺位 = 空语义）。
    Empty,
}

/// 重建选项（issue #113 任务 1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildOptions {
    /// 重算公式（默认 true——[`rebuild_book`] 兼容入口即此默认）。
    /// false = 纯保真直通（源缓存值原样落盘，公式不重算）。
    pub recalculate: bool,
}

impl Default for RebuildOptions {
    fn default() -> Self {
        Self { recalculate: true }
    }
}

/// 单格编辑（`sheet` 0 基索引；`row`/`col` 0 基绝对坐标）。
#[derive(Debug, Clone, PartialEq)]
pub struct CellEdit {
    pub sheet: usize,
    pub row: u32,
    pub col: u32,
    pub value: EditValue,
}

/// 重放统计（保真例外显式化——`skipped` 非空时调用方须向宿主警示）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RebuildStats {
    pub sheets: usize,
    pub cells_written: u64,
    pub merges: usize,
    pub hyperlinks: usize,
    pub edits_applied: usize,
    /// 跳过/降级登记（表名清洗/超链接跳过/合并跳过/列越界/重算降级
    /// ——逐条含坐标或表名）。
    pub skipped: Vec<String>,
    /// 重算成功的公式格数（0 = 未启用/整簿降级——wasm JSON 面尚未
    /// 暴露，宿主侧后续增量）。
    pub recalc_cells: usize,
}

/// XML 1.0 非法非字符（写引擎内建转义面外——U+FFFE/U+FFFF）。
fn is_xml_noncharacter(c: char) -> bool {
    matches!(c, '\u{FFFE}' | '\u{FFFF}')
}

/// 编辑文本值防线：非字符 typed 拒（消息含坐标——调用方可定位）。
fn check_edit_text(sheet: usize, row: u32, col: u32, text: &str) -> Result<(), WriteError> {
    if let Some(c) = text.chars().find(|c| is_xml_noncharacter(*c)) {
        return Err(WriteError::InvalidInput(format!(
            "edit at sheet {sheet} row {row} col {col}: text contains XML noncharacter U+{:04X}",
            c as u32
        )));
    }
    Ok(())
}

/// 表名清洗（与 mdcore-wasm xlsx_export 同型：控制/非法字符→空格、
/// 剥 `[]:*?/\`、trim + 首尾单引号、截 31 chars、**重 trim**（截断
/// 可留尾空白/引号——审查 M2：无重 trim 时第 31 位引号残留会使
/// add_sheet 拒绝，清洗职责破灭）；本 crate 不跨依赖——三域纪律
/// 下同型重写）。空/History 回退 `Sheet`（忽略大小写，保留名）。
fn sanitize_sheet_name(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let stripped: String = mapped
        .chars()
        .filter(|c| !matches!(*c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .collect();
    let taken: String = stripped
        .trim()
        .trim_matches('\'')
        .chars()
        .take(31)
        .collect();
    let out = taken.trim().trim_matches('\'').trim().to_string();
    if out.is_empty() || out.eq_ignore_ascii_case("history") {
        "Sheet".to_string()
    } else {
        out
    }
}

/// 表名去重（` {n}` 后缀，n 从 2）；**基底重截保总长 ≤31**
/// （审查 M1：基底 ≥30 chars 时 `format!("{base} {n}")` 恒超 31 →
/// 循环永不终止——release 下 wasm 调用挂死。后缀先截基底再拼接，
/// 恒可终止）。
fn unique_sheet_name(base: &str, used: &mut Vec<String>) -> String {
    if !used.iter().any(|u| u.eq_ignore_ascii_case(base)) {
        used.push(base.to_string());
        return base.to_string();
    }
    let mut n = 2usize;
    loop {
        let suffix = format!(" {n}");
        // 31 = write.rs 表名上限（char 计）——基底重截恒可终止。
        let keep = 31usize.saturating_sub(suffix.chars().count());
        let head: String = base.chars().take(keep).collect();
        let candidate = format!("{head}{suffix}");
        if !used.iter().any(|u| u.eq_ignore_ascii_case(&candidate)) {
            used.push(candidate.clone());
            return candidate;
        }
        n += 1;
    }
}

/// 读-改-重建主入口（兼容面——默认 [`RebuildOptions::recalculate
/// = true`]，重算开）。
///
/// 值面（编辑覆盖优先）→ 合并 → 超链接（后发射者胜——与源档语义
/// 一致，write_roundtrip.rs 定谳次序）。返回 `(新簿字节, 统计)`。
pub fn rebuild_book(
    book: &Book,
    edits: &[CellEdit],
) -> Result<(Vec<u8>, RebuildStats), WriteError> {
    rebuild_book_with(book, edits, &RebuildOptions::default())
}

/// 读-改-重建主入口（选项面——issue #113 任务 1 重算开关）。
///
/// 重算模式（`recalculate: true`）：编辑账本落位后经 mdc-formula
/// 全簿重算，公式格缓存以引擎结果替换落盘（机制与降级纪律见模块
/// 文档「重算面」节）。非重算模式 = 保真直通（历史行为）。
pub fn rebuild_book_with(
    book: &Book,
    edits: &[CellEdit],
    opts: &RebuildOptions,
) -> Result<(Vec<u8>, RebuildStats), WriteError> {
    // 编辑账本分组：sheet → (row,col) → 编辑（同格后写胜——账本序）。
    let mut by_sheet: Vec<HashMap<(u32, u32), &CellEdit>> = vec![HashMap::new(); book.sheets.len()];
    for e in edits {
        if let Some(m) = by_sheet.get_mut(e.sheet) {
            m.insert((e.row, e.col), e);
        }
        // sheet 索引越界的编辑：静默忽略？——不：重放器职责是把编辑
        // 落到簿上，越界编辑即调用方 bug，typed 拒。
        else {
            return Err(WriteError::InvalidInput(format!(
                "edit references sheet index {} out of range ({})",
                e.sheet,
                book.sheets.len()
            )));
        }
    }

    let mut stats = RebuildStats::default();

    // 重算准备（构建即首算含编辑效果；失败 → 整簿降级保真直通
    // ——非阻断纪律，保真优先）。
    let recalc = if opts.recalculate {
        match prepare_recalc(book, &by_sheet) {
            Ok(mut outcome) => {
                stats.recalc_cells = outcome.recalculated;
                stats.skipped.append(&mut outcome.notes);
                Some(outcome)
            }
            Err(e) => {
                stats.skipped.push(format!(
                    "recalc unavailable (source cache passthrough): {e}"
                ));
                None
            }
        }
    } else {
        None
    };

    let mut w = WorkbookWriter::new();
    let mut used_names: Vec<String> = Vec::new();

    for (si, sheet) in book.sheets.iter().enumerate() {
        // 表名清洗 + 去重（保真例外显式登记）。
        let clean = sanitize_sheet_name(&sheet.name);
        let final_name = unique_sheet_name(&clean, &mut used_names);
        if final_name != sheet.name {
            stats.skipped.push(format!(
                "sheet name sanitized: '{}' -> '{}'",
                sheet.name, final_name
            ));
        }
        let mut s = w.add_sheet(&final_name)?;
        let edit_map = &by_sheet[si];
        let mut consumed: HashMap<(u32, u32), ()> = HashMap::new();

        // 值面：使用区循环（replay_cell 单格联接点）+ 编辑覆盖。
        // 重算模式 = typed ∪ formulas 并集（无缓存公式格补写）；
        // 非重算模式 = typed 使用区（历史行为——使用区外不写）。
        if let Some(((r0, c0), (r1, c1))) = value_region(sheet, recalc.is_some()) {
            for row in r0..=r1 {
                for col in c0..=c1 {
                    if let Some(edit) = edit_map.get(&(row, col)) {
                        consumed.insert((row, col), ());
                        stats.edits_applied += 1;
                        apply_edit(&mut s, edit, recalc.as_ref())?;
                    } else if let Some((mut v, style)) = sheet.replay_cell(row, col) {
                        // 公式格缓存原位替换（公式文本与配对样式保持
                        // ——DateTime 缓存的 numFmt 配对不被重算破坏）。
                        if let Some(fresh) = recalc_fresh(&recalc, si, row, col) {
                            if let crate::write::CellValue::Formula { cached, .. } = &mut v {
                                *cached = Some(fresh.clone());
                            }
                        }
                        s.write(row, to_col16(col)?, v, style.as_ref())?;
                        stats.cells_written += 1;
                    } else if recalc.is_some() {
                        // typed 区外公式格（无缓存值——重算模式补写；
                        // 非重算模式维持历史行为不写）。
                        if let Some(text) = sheet.formula_at(row, col) {
                            let cached = recalc_fresh(&recalc, si, row, col).cloned();
                            s.write(
                                row,
                                to_col16(col)?,
                                crate::write::CellValue::Formula {
                                    text: text.to_string(),
                                    cached,
                                },
                                None,
                            )?;
                            stats.cells_written += 1;
                        }
                    }
                }
            }
        }

        // 窗余编辑（超出使用区——扩展写入；Empty 的窗余编辑 = 无操作）。
        let mut leftover: Vec<&CellEdit> = edit_map
            .iter()
            .filter(|(k, _)| !consumed.contains_key(*k))
            .map(|(_, e)| *e)
            .collect();
        leftover.sort_by_key(|e| (e.row, e.col));
        for edit in leftover {
            stats.edits_applied += 1;
            apply_edit(&mut s, edit, recalc.as_ref())?;
        }

        // 合并重放（值面后；左上值已在值面——merge 文本面取源档字符串值）。
        for m in &sheet.merges {
            if m.start == m.end {
                stats.skipped.push(format!(
                    "merge skipped (single cell) at {}",
                    fmt_pos(m.start.0, m.start.1)
                ));
                continue;
            }
            if m.end.1 > MAX_COL as u32 {
                stats.skipped.push(format!(
                    "merge skipped (col exceeds write domain) at {}..{}",
                    fmt_pos(m.start.0, m.start.1),
                    fmt_pos(m.end.0, m.end.1)
                ));
                continue;
            }
            let top = sheet.typed.get_abs(m.start.0, m.start.1);
            // 仅 Text 左上取原文（与 write_roundtrip.rs 参考对齐）；
            // 非 Text 左上 → 文本扁平化（合并面仅字符串）→ skipped
            // 登记（非静默纪律——审查 S3）。
            let text = match top {
                Some(crate::io::CellData::Text(t)) => t.clone(),
                Some(cd) => {
                    stats.skipped.push(format!(
                        "merge flattened (non-text top-left {}) at {}",
                        kind_label(cd),
                        fmt_pos(m.start.0, m.start.1)
                    ));
                    cd.display()
                }
                None => String::new(),
            };
            s.merge(
                m.start.0,
                m.start.1 as u16,
                m.end.0,
                m.end.1 as u16,
                &text,
                None,
            )?;
            stats.merges += 1;
        }

        // 超链接重放（值面 + 合并后；四形态 + 长度/NUL 防线）。
        for h in &sheet.hyperlinks {
            let url = match (&h.target, &h.location) {
                (Some(t), Some(loc)) => format!("{t}#{loc}"),
                (Some(t), None) => t.clone(),
                (None, Some(loc)) => format!("internal:{loc}"),
                (None, None) => continue,
            };
            let pos = fmt_pos(h.range.start.0, h.range.start.1);
            if url.len() > MAX_URL_TEXT {
                stats
                    .skipped
                    .push(format!("hyperlink skipped (url > {MAX_URL_TEXT}) at {pos}"));
                continue;
            }
            if url.contains('\0') {
                stats
                    .skipped
                    .push(format!("hyperlink skipped (url contains NUL) at {pos}"));
                continue;
            }
            // #276（审计 B-1）：重放出口 scheme 门——mdc-office url_policy
            // 白名单（http/https/mailto/internal；`internal:` 前缀是本
            // 函数 location-only 形态的自造 scheme，恰在白名单）。敌意
            // `javascript:` 超链接经 rebuild 写回 xlsx 时原样透传的缺口
            // 就此闭合（此前仅长度/NUL 两道防线——scheme 无检查）。
            if let Err(e) = mdc_office::sanitize::check_url(&url) {
                stats.skipped.push(format!(
                    "hyperlink skipped (url policy rejected: {e}) at {pos}"
                ));
                continue;
            }
            if h.tooltip
                .as_deref()
                .is_some_and(|t| t.len() > MAX_TOOLTIP_TEXT)
            {
                stats.skipped.push(format!(
                    "hyperlink skipped (tooltip > {MAX_TOOLTIP_TEXT}) at {pos}"
                ));
                continue;
            }
            let anchor_text = sheet
                .typed
                .get_abs(h.range.start.0, h.range.start.1)
                .map(|cd| cd.display());
            let display = match anchor_text {
                Some(t) if t != url => Some(t),
                _ => None,
            };
            if h.range.start.1 > MAX_COL as u32 {
                stats.skipped.push(format!(
                    "hyperlink skipped (col exceeds write domain) at {pos}"
                ));
                continue;
            }
            s.write_hyperlink(
                h.range.start.0,
                h.range.start.1 as u16,
                HyperlinkSpec {
                    url,
                    display_text: display,
                    tooltip: h.tooltip.clone(),
                },
            )?;
            stats.hyperlinks += 1;
        }
        stats.sheets += 1;
    }

    let bytes = w.save_to_vec()?;
    Ok((bytes, stats))
}

fn fmt_pos(r: u32, c: u32) -> String {
    format!("r{r}c{c}")
}

/// skipped 登记的 kind 标签（与 xlsx_grid `cell_kind` 同集）。
fn kind_label(cd: &crate::io::CellData) -> &'static str {
    use crate::io::CellData;
    match cd {
        CellData::Text(_) => "text",
        CellData::Number(_) => "number",
        CellData::Boolean(_) => "bool",
        CellData::DateTime { .. } | CellData::Duration { .. } => "datetime",
        CellData::DateTimeIso(_) | CellData::DurationIso(_) => "datetime_iso",
        CellData::Error(_) => "error",
        CellData::Empty => "empty",
    }
}

/// 编辑落格（Empty = 不落格 = 空语义；Text/Formula 过非字符防线；
/// Formula 的缓存取重算表——构建即首算已含该编辑）。
fn apply_edit<'a>(
    s: &mut crate::write::SheetWriter<'a>,
    edit: &CellEdit,
    recalc: Option<&RecalcOutcome>,
) -> Result<(), WriteError> {
    let col16 = u16::try_from(edit.col).map_err(|_| {
        WriteError::InvalidInput(format!(
            "edit at sheet {} row {} col {}: col exceeds write domain",
            edit.sheet, edit.row, edit.col
        ))
    })?;
    match &edit.value {
        EditValue::Text(t) => {
            check_edit_text(edit.sheet, edit.row, edit.col, t)?;
            s.write(
                edit.row,
                col16,
                crate::write::CellValue::Text(t.clone()),
                None,
            )
        }
        EditValue::Number(v) => s.write(edit.row, col16, crate::write::CellValue::Number(*v), None),
        EditValue::Boolean(b) => {
            s.write(edit.row, col16, crate::write::CellValue::Boolean(*b), None)
        }
        EditValue::Formula(f) => {
            check_edit_text(edit.sheet, edit.row, edit.col, f)?;
            let cached = recalc
                .and_then(|o| o.table.get(&(edit.sheet, edit.row, edit.col)))
                .cloned();
            s.write(
                edit.row,
                col16,
                crate::write::CellValue::Formula {
                    text: f.clone(),
                    cached,
                },
                None,
            )
        }
        EditValue::Empty => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// 重算面（issue #113 任务 1）——引擎灌入/重算表/降级登记
// ---------------------------------------------------------------------------

/// 重算产物：公式格新缓存表（key = (sheet 索引, 0 基 row, 0 基 col)）。
struct RecalcOutcome {
    table: HashMap<(usize, u32, u32), FormulaCache>,
    /// 重算成功格数（= table.len()）。
    recalculated: usize,
    /// 降级/输入排除登记（rebuild 主体事后并入 skipped）。
    notes: Vec<String>,
}

/// 单格降级登记封顶（逐格 20 条 + 总数汇总——大档全降级时不炸
/// skipped 列表）。
const DEGRADED_NOTE_CAP: usize = 20;

/// 重算准备：灌入（编辑即落位）→ `FormulaEngine::new` 构建即首算 →
/// 公式格新缓存表。Err = 整簿降级理由（调用方登记后保真直通）。
fn prepare_recalc(
    book: &Book,
    by_sheet: &[HashMap<(u32, u32), &CellEdit>],
) -> Result<RecalcOutcome, String> {
    // 预算预检（面积上界——O(表数)；稀疏大包围盒保守降级，登记措辞
    // 如实「面积上界」）。
    let area: u64 = book
        .sheets
        .iter()
        .map(|s| region_area(value_region(s, true)))
        .sum();
    if area > mdc_formula::LIMIT_TOTAL_CELLS as u64 {
        return Err(format!(
            "used-range area upper bound {area} exceeds engine budget {}",
            mdc_formula::LIMIT_TOTAL_CELLS
        ));
    }

    // 灌入：值面（typed 类型化 + 1904 归一）+ 公式面（= 前缀补全）+
    // 编辑账本（编辑即落位——构建 + 显式 recalculate 一次全簿重算，
    // 免 set_cell 二段式）。公式格坐标清单顺带收集（重算表遍历面 =
    // O(公式格数)）。外部引用公式（`[` 启发式识别——保守：字符串
    // 字面量含 `[` 的普通公式会假阳性降级，方向为宁可不重算）不灌
    // 公式、改灌**源缓存值作常量**——本格保留源缓存（issue #113
    // 验收「外部引用保留缓存 + 警告」），依赖者以缓存值级联（引擎
    // 里它是常量而非缺位——缺位会使依赖者算 0 错误级联）。
    let mut formula_cells: Vec<(usize, u32, u32)> = Vec::new();
    let mut excluded_error_const = 0usize;
    let mut external_ref_cells = 0usize;
    let mut inputs: Vec<SheetInput> = Vec::with_capacity(book.sheets.len());
    for (si, sheet) in book.sheets.iter().enumerate() {
        let mut cells = Vec::new();
        let region = value_region(sheet, true);
        if let Some(((r0, c0), (r1, c1))) = region {
            for row in r0..=r1 {
                for col in c0..=c1 {
                    let pos = (row + 1, col + 1);
                    if let Some(edit) = by_sheet.get(si).and_then(|m| m.get(&(row, col))) {
                        if let EditValue::Formula(_) = &edit.value {
                            formula_cells.push((si, row, col));
                        }
                        if let Some(content) = edit_content(edit) {
                            cells.push((pos.0, pos.1, content));
                        }
                    } else if let Some(text) = sheet.formula_at(row, col) {
                        if text.contains('[') {
                            // 外部引用形态（[Book1]Sheet1!A1）——不重算：
                            // 本格不进重算表（源缓存保留），灌源缓存常量。
                            external_ref_cells += 1;
                            if let Some(cd) = sheet.typed.get_abs(row, col) {
                                match cd {
                                    CellData::Error(_) => excluded_error_const += 1,
                                    _ => {
                                        if let Some(v) = celldata_to_value(cd) {
                                            cells.push((pos.0, pos.1, CellContent::Value(v)));
                                        }
                                    }
                                }
                            }
                        } else {
                            formula_cells.push((si, row, col));
                            cells.push((
                                pos.0,
                                pos.1,
                                CellContent::Formula(ensure_formula_prefix(text)),
                            ));
                        }
                    } else if let Some(cd) = sheet.typed.get_abs(row, col) {
                        match cd {
                            // 引擎无错误常量输入面（CellContent::Value 的
                            // Value::Error 恒拒——错误格由求值产生）——排除
                            // 灌入 + 登记（引用它的公式按空格语义求值：
                            // 实测 =E1+1 → 1，Excel 语义为错误传播——
                            // 能力边界如实登记）。
                            CellData::Error(_) => excluded_error_const += 1,
                            _ => {
                                if let Some(v) = celldata_to_value(cd) {
                                    cells.push((pos.0, pos.1, CellContent::Value(v)));
                                }
                            }
                        }
                    }
                }
            }
        }
        // 扩展区编辑补灌（编辑格在 typed/formulas 两网格外——值编辑
        // 供依赖者级联，公式编辑进重算表）。
        if let Some(edit_map) = by_sheet.get(si) {
            for (&(row, col), &edit) in edit_map.iter() {
                if region_contains(region, row, col) {
                    continue;
                }
                if let EditValue::Formula(_) = &edit.value {
                    formula_cells.push((si, row, col));
                }
                if let Some(content) = edit_content(edit) {
                    cells.push((row + 1, col + 1, content));
                }
            }
        }
        inputs.push(SheetInput {
            name: sheet.name.clone(),
            cells,
        });
    }

    // 构建后显式全簿重算（mdc-formula 的 `new` 只灌入不求值——其
    // lib.rs 文档「构建即首算」与实现不符，#112 遗留文档缺陷，本轮
    // 探针钉定（formula crate 本轮禁改——rebuild 侧显式 recalculate
    // 补偿；PR 说明登记））。
    let mut engine = FormulaEngine::new(&inputs).map_err(|e| e.to_string())?;
    engine.recalculate();

    // 重算表：全部公式格新缓存。
    let mut table = HashMap::new();
    let mut notes = Vec::new();
    let mut degraded = 0usize;
    for &(si, row, col) in &formula_cells {
        match engine.cell_value(si as u32, row + 1, col + 1) {
            Ok(FormulaValue::Number(v)) => {
                table.insert((si, row, col), FormulaCache::Number(v));
            }
            Ok(FormulaValue::Text(s)) => {
                table.insert((si, row, col), FormulaCache::Text(s));
            }
            Ok(FormulaValue::Bool(b)) => {
                table.insert((si, row, col), FormulaCache::Boolean(b));
            }
            // 错误格（含 CIRC 循环引用/SPILL 等）——错误码文本缓存
            // （与源档 Error 缓存的 kind 降级形态同型）。
            Ok(FormulaValue::Error(e)) => {
                table.insert((si, row, col), FormulaCache::Text(e.code()));
            }
            // 引擎判无值——保留源缓存（查询 miss 即回退）。
            Ok(FormulaValue::Empty) => {}
            Err(e) => {
                degraded += 1;
                if notes.len() < DEGRADED_NOTE_CAP {
                    notes.push(format!(
                        "recalc degraded at r{row}c{col} on sheet '{}': {e} (source cache kept)",
                        book.sheets[si].name
                    ));
                }
            }
        }
    }
    if degraded > DEGRADED_NOTE_CAP {
        notes.push(format!(
            "recalc degraded: {degraded} formula cells kept source cache in total"
        ));
    }
    if excluded_error_const > 0 {
        notes.push(format!(
            "recalc input: {excluded_error_const} error-constant cells excluded (engine input face has no error constants)"
        ));
    }
    if external_ref_cells > 0 {
        notes.push(format!(
            "recalc input: {external_ref_cells} external-reference formula cell(s) kept source cache (fed to engine as constants for dependents; '[' heuristic)"
        ));
    }
    let recalculated = table.len();
    Ok(RecalcOutcome {
        table,
        recalculated,
        notes,
    })
}

/// typed 类型化值 → 引擎灌入值（DateTime 1904 纪元归一——与写面
/// replay 同纪元换算单点 `epoch_to_1900` 同源；ISO 串以文本参与
/// 重算——t="d" 罕见面的语义近似，模块文档登记）。
fn celldata_to_value(cd: &CellData) -> Option<FormulaValue> {
    match cd {
        CellData::Text(s) => Some(FormulaValue::Text(s.clone())),
        CellData::Number(v) => Some(FormulaValue::Number(*v)),
        CellData::Boolean(b) => Some(FormulaValue::Bool(*b)),
        CellData::DateTime { serial, is_1904 } => {
            Some(FormulaValue::Number(epoch_to_1900(*serial, *is_1904)))
        }
        CellData::Duration { serial, .. } => Some(FormulaValue::Number(*serial)),
        CellData::DateTimeIso(s) | CellData::DurationIso(s) => Some(FormulaValue::Text(s.clone())),
        CellData::Error(_) | CellData::Empty => None,
    }
}

/// 编辑 → 引擎灌入内容（Empty = 清空不灌——缺位语义；调用方对
/// Formula 变体另行登记重算表）。
fn edit_content(edit: &CellEdit) -> Option<CellContent> {
    match &edit.value {
        EditValue::Text(t) => Some(CellContent::Value(FormulaValue::Text(t.clone()))),
        EditValue::Number(v) => Some(CellContent::Value(FormulaValue::Number(*v))),
        EditValue::Boolean(b) => Some(CellContent::Value(FormulaValue::Bool(*b))),
        EditValue::Formula(f) => Some(CellContent::Formula(ensure_formula_prefix(f))),
        EditValue::Empty => None,
    }
}

/// 坐标是否在区域内。
fn region_contains(region: Option<((u32, u32), (u32, u32))>, row: u32, col: u32) -> bool {
    region.is_some_and(|((r0, c0), (r1, c1))| row >= r0 && row <= r1 && col >= c0 && col <= c1)
}

/// 公式 `=` 前缀补全（calamine 读面无前缀——FIDELITY §1；引擎入参
/// 恒要求带前缀）。`_xlfn.` 透传（写面前缀规范化同型）。
fn ensure_formula_prefix(text: &str) -> String {
    if text.starts_with('=') {
        text.to_string()
    } else {
        format!("={text}")
    }
}

/// 值面重放区域：非重算模式 = typed 使用区（历史行为）；重算模式 =
/// typed ∪ formulas 并集（无缓存公式格补写）。
fn value_region(sheet: &SheetData, extend: bool) -> Option<((u32, u32), (u32, u32))> {
    let typed = sheet.typed.start().zip(sheet.typed.end());
    if !extend {
        return typed;
    }
    let formulas = sheet.formulas.start().zip(sheet.formulas.end());
    match (typed, formulas) {
        (Some((a0, a1)), Some((b0, b1))) => Some((
            (a0.0.min(b0.0), a0.1.min(b0.1)),
            (a1.0.max(b1.0), a1.1.max(b1.1)),
        )),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

/// 区域格数（面积上界——u64 域防 u32 溢出）。
fn region_area(region: Option<((u32, u32), (u32, u32))>) -> u64 {
    region
        .map(|((r0, c0), (r1, c1))| (r1 as u64 - r0 as u64 + 1) * (c1 as u64 - c0 as u64 + 1))
        .unwrap_or(0)
}

/// 值面重放列域转换（u16 写域）。
fn to_col16(col: u32) -> Result<u16, WriteError> {
    u16::try_from(col)
        .map_err(|_| WriteError::InvalidInput(format!("col {col} exceeds write domain")))
}

/// 重算表查询（None = 未启用/降级格——调用方回退源缓存）。
fn recalc_fresh(
    recalc: &Option<RecalcOutcome>,
    sheet: usize,
    row: u32,
    col: u32,
) -> Option<&FormulaCache> {
    recalc
        .as_ref()
        .and_then(|o| o.table.get(&(sheet, row, col)))
}
