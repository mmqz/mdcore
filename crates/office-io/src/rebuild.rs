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

use crate::io::Book;
use crate::write::{
    HyperlinkSpec, WorkbookWriter, WriteError, MAX_COL, MAX_TOOLTIP_TEXT, MAX_URL_TEXT,
};

/// 单格编辑值（命令账本载荷——kind 指导 typed 写入）。
#[derive(Debug, Clone, PartialEq)]
pub enum EditValue {
    Text(String),
    Number(f64),
    Boolean(bool),
    /// 清空（值面重放跳过该格 + 扩展区写入跳过 = 格缺位 = 空语义）。
    Empty,
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
    /// 跳过/降级登记（表名清洗/超链接跳过/合并跳过/列越界——
    /// 逐条含坐标或表名）。
    pub skipped: Vec<String>,
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

/// 读-改-重建主入口。
///
/// 值面（编辑覆盖优先）→ 合并 → 超链接（后发射者胜——与源档语义
/// 一致，write_roundtrip.rs 定谳次序）。返回 `(新簿字节, 统计)`。
pub fn rebuild_book(
    book: &Book,
    edits: &[CellEdit],
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
        if let (Some((r0, c0)), Some((r1, c1))) = (sheet.typed.start(), sheet.typed.end()) {
            for row in r0..=r1 {
                for col in c0..=c1 {
                    if let Some(edit) = edit_map.get(&(row, col)) {
                        consumed.insert((row, col), ());
                        stats.edits_applied += 1;
                        apply_edit(&mut s, edit)?;
                    } else if let Some((v, style)) = sheet.replay_cell(row, col) {
                        let col16 = u16::try_from(col).map_err(|_| {
                            WriteError::InvalidInput(format!("col {col} exceeds write domain"))
                        })?;
                        s.write(row, col16, v, style.as_ref())?;
                        stats.cells_written += 1;
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
            apply_edit(&mut s, edit)?;
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

/// 编辑落格（Empty = 不落格 = 空语义；Text 过非字符防线）。
fn apply_edit<'a>(
    s: &mut crate::write::SheetWriter<'a>,
    edit: &CellEdit,
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
        EditValue::Empty => Ok(()),
    }
}
