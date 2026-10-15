//! t18：编辑器核心文档 → DOCX 导出切片（office-io 写面经 wasm 桥）。
//!
//! **定位（2026-10-14，t18 立项）**：编辑器当前文档（mdcore
//! [`BlockTree`]——唯一语义真理，主档 §9）→ [`mdc_office_io::docx`]
//! 的 [`DocxWriter`] 写面 → zip 容器字节 → web 侧 `Blob + a.download`
//! 下载 `document.docx`。与 t16（DOCX 导入）合流 = DOCX 往返闭环；
//! 三域纪律唯一新边沿用 t16/t17 的 mdcore-wasm → office-io（消费方
//! 边界；office-io 对 mdcore 零依赖不变）。
//!
//! **t20 增量（2026-10-15）**：office-io 写面五面扩（底稿 = 分析仓
//! `office-line/docx-r1/WRITE_FACE_PROBE.md` §4 映射草案）后，t18
//! 登记的四件降级 + 块级 code 等宽 + 行内 del·u **整批消解**；
//! 残余降级 = code lang/fence（库无语法高亮/元数据面）与 link
//! mark（模型 [`MarkSpan`] 无 URL 载荷——mdcore MD 域上游缺口）。
//!
//! **t22 增量（2026-10-16）**：多行 item 续行缩进（t20 登记残余
//! 翻转为已表达）——office-io 新面 [`DocxWriter::list_item_lines`]：
//! 一个列表项 = 一个 OOXML 段落（行间 `w:br w:type="textWrapping"`
//! 软换行）+ 段落级 hanging `w:ind`（left = 720×(ilvl+1)、hanging
//! 360——与编号抽象层级同值）→ **续行与首行文字列对齐**；单行项
//! 既有路径字节级零变化（纯增量路径）。
//!
//! ## 暴露面（[`WasmCore::export_docx`]——既有类增量方法，状态面零变化）
//!
//! - 入参：无（导出 `self` 当前块树——宿主编辑器文档本体）。
//! - 成功：DOCX 字节（wasm-bindgen `Vec<u8>` → JS `Uint8Array`）。
//! - 失败：抛 JSON 字符串 `{"code": "...", "message": "..."}`
//!   （t16/t17 错误面约定，宿主 `JSON.parse` 出结构化错误）。
//!
//! ## 错误码表
//!
//! | code | 含义 |
//! |---|---|
//! | `docx_build` | 引擎写/打包错误（[`DocxError::Io`] 打包失败为唯一现实路径；[`DocxError::Parse`] = heading 级越界 / list ilvl 越界，映射层恒钳位故不可达，完备性仍映射）。**映射层是全函数——降级永不报错**；`Container` 变体只在读面产生，写侧不可达（完备性登记）。 |
//! | `docx_limit` | **r4-t2 写面资源限**（office-io `mdc_office_io::docx::MAX_PARAGRAPHS`/`MAX_RUNS`——save 面 typed 拒；超限导出**无产物字节**——错误路径即无返回值，无半成品 Vec；常量单一真源在 office-io，本层零自立数字）。 |
//!
//! ## 映射表（BlockKind → DocxWriter；模型级，非保真渲染）
//!
//! | mdcore 块 | DocxWriter 调用 | 降级（不可表达面，显式登记） |
//! |---|---|---|
//! | `Heading`（attrs.level，缺省/不可解析 → 1；钳 1-9） | [`DocxWriter::heading`]（Heading1-9 样式 + outline 级） | 模型 level 可为任意 usize（op 面），>9 钳 9；**标题 marks 丢弃**（[`DocxWriter::heading`] 纯文本面——html.rs 导入可产带 marks 的标题） |
//! | `Paragraph`（text + marks） | [`DocxWriter::paragraph`] / 引用上下文 [`DocxWriter::quote_paragraph`] run 序 | mark kind 自由字符串：`strong`/`bold` → 加粗、`em`/`italic` → 斜体、`code` → 等宽（Consolas rFonts）、`del` → 删除线、`u` → 下画线（t20 消解）；**link kind → 纯文本**——模型 [`MarkSpan`] 无 URL 载荷，目标不可表达（html.rs 亦无 `<a>` 处理面——模型级无链接表示）；未知 kind → 纯文本 |
//! | `CodeBlock`（attrs.lang + text） | 每行 [`DocxWriter::code_line`] 双通道等宽（段落级+run 级 Courier New） | **等宽面已表达（t20 消解）；lang、fence 结构仍丢弃**（docx-rs 无语法高亮/元数据面——库层不可达，登记维持）；文本逐字保留 |
//! | `Table`（text = 规范管道行） | [`DocxWriter::table`] | 第 2 行分隔行剔除（GFM 语义位）；`\|` 反转义 → `\|`；单元格内 `\n`/`\r` → 空格（GFM 单元格单行面，t17 同向）；孤立 `\` 原样透传（模型无内联转义层——t16 同边界登记）；单元格内 marks 本就不可承载（Table = 扁平文本面，html.rs 同边界）；0 行表跳过（空 `w:tbl` 非 OOXML 合法形——防御，markdown 解析不产空表） |
//! | `Blockquote` | 引用面 [`DocxWriter::quote_paragraph`]（ind 720×depth 钳 8 + 左边框，嵌套步进）；子块递归（段落/嵌套引用承载引用面——markdown 解析器产出的唯一形态） | 空引用（无子且 text 非空）按 text 发射（markdown.rs 序列化语义镜像）；**引用上下文中的 heading/table/code/list/hr 子块不挂 ind/pBdr**（markdown 解析器不产此形态——attach 恒挂 root；op 面可构造，防御递归保持 t18 平铺行为，登记） |
//! | `ListItem`（attrs.marker/depth） | 单行：[`DocxWriter::list_item`]：marker 判序（数字型 → Ordered，`-`/`*`/`+`/缺失 → Bullet）+ depth→ilvl（钳 0-8；ilvl 越界防御：>8 钳 8——映射层全函数）；多行（t22 消解）：[`DocxWriter::list_item_lines`]——单段落 + 行间 `w:br` + 段落级 hanging `w:ind`（left = 720×(ilvl+1)、hanging 360，与编号抽象层级同值）→ 续行与首行文字列对齐 | **列表结构已表达（t20 消解）；多行续行缩进已表达（t22 消解——行内 marks 经 rich_lines 逐行平移保持）**；子块递归保持 |
//! | `ThematicBreak` | [`DocxWriter::horizontal_rule`]（空段下边框） | 无降级（t20 消解；OOXML 无原生 hr——下边框为惯用表达） |
//!
//! 文本行处理（全局规则）：块文本按 `\n` 分行、每行剥一个行尾 `\r`
//! （CRLF 容忍——markdown.rs `split('\n')` 保留 `\r`），多行文本 →
//! 每行一个 docx 段落（mdcore 行级哲学，t16 w:br→`\n` 同向登记；
//! **例外**：ListItem 多行 = 单段 + 行间 `w:br`——t22 续行缩进面，
//! 项内软换行语义，读面 br→`\n` 约定对称）；
//! 行内孤立 `\r` 原样透传。mark 偏移单位 = char（op.rs SetMark 契约），
//! 多行切分时按行区间裁剪平移。
//!
//! ## 已知边界（登记，非缺陷）
//!
//! - run 相邻同 (bold,italic,mono,strike,underline) 合并（docx 段落
//!   run 边界不承载模型语义，t16 导入侧同款合并登记的反向；t20
//!   合并键扩至五位）；空 run 不发射；
//! - **导出→t16 重导入的读面承载边界**：numPr/pBdr/ind/rFonts 不入
//!   office-io 读模型（[`RunData`] 无字体/删除线/下画线字段，段落无
//!   缩进/边框/编号字段）——**不宣称 list/quote/hr/等宽的往返等值**；
//!   本模块测试 = 导出 XML 面直断言 + 既有文本面回归（文本零损）；
//! - 导出的 heading 样式读回 = `Heading1`..`Heading9`（office-io 读面
//!   样式 ID 透传）——全环断言见本模块测试；
//! - 不暴露面（DocxWriter 能力中本切片未接线）：页眉/页脚槽位、PNG
//!   图片（模型无图片块型——BlockKind 无 image 变体，非降级而是
//!   无源可映射）、脚注、域。
//!
//! 纯内核（[`export_docx_core`]）不触碰 JsValue → 原生 cargo test
//! 直测（写→`read_docx_from_bytes`→模型断言 + XML 面 zip 本地头遍历
//! 直断言，t16/t17/t18 同款写读全环纪律）；wasm 壳由 wasm32 check +
//! web 三件套门禁背书。

use crate::WasmCore;
#[cfg(test)]
use mdc_office_io::docx::read_docx_from_bytes;
use mdc_office_io::docx::{DocxError, DocxWriter, ListKind, RunSpec};
use mdcore::tree::{Attrs, BlockKind, BlockTree, MarkSpan, NodeId};
use wasm_bindgen::prelude::*;

/// typed 导出错误（错误码 + 人类可读消息——见模块文档错误码表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
}

/// 引擎写面错误 → 错误码（r4-t2：资源限 `docx_limit` 与引擎面
/// `docx_build` 可区分——见模块文档错误码表）。
fn map_error(e: &DocxError) -> ExportError {
    match e {
        DocxError::Limit(m) => ExportError {
            code: "docx_limit",
            message: format!("docx writer resource limit: {m}"),
        },
        other => ExportError {
            code: "docx_build",
            message: format!("docx writer engine error: {other}"),
        },
    }
}

// ---------------------------------------------------------------------------
// 纯映射内核（原生可测——BlockTree 直构，零 zip 依赖直到 save_to_vec）
// ---------------------------------------------------------------------------

/// Heading attrs["level"] → 1-9（不可解析/缺失 → 1；>9 钳 9）。
/// markdown.rs 解析只产 1-6；op 面可设任意值——钳位登记见模块文档。
fn heading_level(attrs: &Attrs) -> u8 {
    attrs
        .get("level")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 9) as u8
}

/// mark kind → (bold, italic, mono, strike, underline) 五位。模型 kind
/// 为自由字符串（op.rs SetMark）：`strong`/`bold`（html.rs 规范 /
/// spike 命名）→ 加粗；`em`/`italic` → 斜体；`code` → 等宽（t20
/// 消解）；`del` → 删除线（t20 消解）；`u` → 下画线（t20 消解）；
/// 其余（link/未知）→ 无面可表达 → 纯文本（link = 模型无 URL 载荷，
/// 上游缺口登记）。
fn mark_style(kind: &str) -> (bool, bool, bool, bool, bool) {
    match kind {
        "strong" | "bold" => (true, false, false, false, false),
        "em" | "italic" => (false, true, false, false, false),
        "code" => (false, false, true, false, false),
        "del" => (false, false, false, true, false),
        "u" => (false, false, false, false, true),
        _ => (false, false, false, false, false),
    }
}

/// 单行文本 + char 偏移 marks → run 序（边界集分段 + 相邻同五位
/// (bold,italic,mono,strike,underline) 合并 + 空段跳过）。空文本 →
/// 空 run 序。
fn runs_from(text: &str, marks: &[MarkSpan]) -> Vec<RunSpec> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    // 五位覆盖区间集（mark 交叠 = 位叠加——t18 bold/italic 交叠语义
    // 的 t20 扩面）。
    let mut bold: Vec<(usize, usize)> = Vec::new();
    let mut italic: Vec<(usize, usize)> = Vec::new();
    let mut mono: Vec<(usize, usize)> = Vec::new();
    let mut strike: Vec<(usize, usize)> = Vec::new();
    let mut underline: Vec<(usize, usize)> = Vec::new();
    for m in marks {
        // 防御钳位（op 面写入时已校验 ≤ len_chars；adjust_marks 维护不变量）
        let s = m.start.min(len);
        let e = m.end.min(len).max(s);
        if s == e {
            continue;
        }
        let (b, i, mo, st, u) = mark_style(&m.kind);
        if b {
            bold.push((s, e));
        }
        if i {
            italic.push((s, e));
        }
        if mo {
            mono.push((s, e));
        }
        if st {
            strike.push((s, e));
        }
        if u {
            underline.push((s, e));
        }
    }
    let mut points: Vec<usize> = vec![0, len];
    points.extend(
        bold.iter()
            .chain(italic.iter())
            .chain(mono.iter())
            .chain(strike.iter())
            .chain(underline.iter())
            .flat_map(|&(s, e)| [s, e]),
    );
    points.sort_unstable();
    points.dedup();
    let covers = |spans: &[(usize, usize)], x: usize| spans.iter().any(|&(s, e)| s <= x && x < e);
    let mut runs: Vec<RunSpec> = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a == b {
            continue;
        }
        let (bold_seg, italic_seg) = (covers(&bold, a), covers(&italic, a));
        let (mono_seg, strike_seg, underline_seg) =
            (covers(&mono, a), covers(&strike, a), covers(&underline, a));
        let seg: String = chars[a..b].iter().collect();
        // 相邻同样式合并（模型无链接 run——link 恒 None，样式同即并）
        if let Some(last) = runs.last_mut() {
            if last.bold == bold_seg
                && last.italic == italic_seg
                && last.mono == mono_seg
                && last.strike == strike_seg
                && last.underline == underline_seg
            {
                last.text.push_str(&seg);
                continue;
            }
        }
        runs.push(RunSpec::styled_full(
            seg,
            bold_seg,
            italic_seg,
            mono_seg,
            strike_seg,
            underline_seg,
        ));
    }
    runs
}

/// 块文本 → docx 段落行序列（'\n' 分行 + 每行剥一个行尾 '\r'——
/// CRLF 容忍；孤立 '\r' 原样透传，登记见模块文档）。
/// t19：GFM 行切分语义单一真源 —— [`crate::xlsx_export`] 共用
///（pub(crate)，无行为变化）。
pub(crate) fn text_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// 富文本（可多行）→ (行, 行内 marks) 序列：'\n' 分行 + 行尾 '\r'
/// 仅在发射面剥除 + mark char 偏移按行区间裁剪平移（多行切分不丢
/// 格式）。行切分 + mark 平移的单一真源（ListItem 首行/余行分道与
/// 引用段落共用）。
fn rich_lines(text: &str, marks: &[MarkSpan]) -> Vec<(String, Vec<MarkSpan>)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    loop {
        let nl = chars[start..]
            .iter()
            .position(|&c| c == '\n')
            .map(|i| start + i)
            .unwrap_or(chars.len());
        // 行尾 '\r' 只在发射面剥除；start/nl 坐标保持原文本系（偏移恒等）
        let mut emit_end = nl;
        if emit_end > start && chars[emit_end - 1] == '\r' {
            emit_end -= 1;
        }
        let line: String = chars[start..emit_end].iter().collect();
        let line_marks: Vec<MarkSpan> = marks
            .iter()
            .filter_map(|m| {
                let s = m.start.clamp(start, emit_end);
                let e = m.end.clamp(start, emit_end).max(s);
                if s == e {
                    None
                } else {
                    Some(MarkSpan {
                        start: s - start,
                        end: e - start,
                        kind: m.kind.clone(),
                    })
                }
            })
            .collect();
        out.push((line, line_marks));
        if nl == chars.len() {
            break;
        }
        start = nl + 1;
    }
    out
}

/// 富文本（可多行）→ 每行一个 docx 段落；`quote_depth > 0` 时走引用面
///（ind + 左边框，嵌套步进语义由调用方传深度）。
fn emit_rich_paragraphs(w: &mut DocxWriter, text: &str, marks: &[MarkSpan], quote_depth: usize) {
    for (line, line_marks) in rich_lines(text, marks) {
        let runs = runs_from(&line, &line_marks);
        if quote_depth > 0 {
            w.quote_paragraph(&runs, quote_depth);
        } else {
            w.paragraph(&runs);
        }
    }
}

/// ListItem attrs → (列表类型, ilvl)：marker 首字符为 ASCII 数字 →
/// Ordered（markdown.rs `1.`/`1)` 两形态）；`-`/`*`/`+`/缺失/其他 →
/// Bullet（缺失 = op 面防御缺省，登记）。depth（1 基，markdown.rs
/// indent/2+1）→ ilvl = depth-1 钳 0-8（九级抽象域；越界防御钳位——
/// 映射全函数）。
fn list_shape(attrs: &Attrs) -> (ListKind, usize) {
    let marker = attrs.get("marker").map(|s| s.as_str()).unwrap_or("");
    let kind = if marker.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        ListKind::Ordered
    } else {
        ListKind::Bullet
    };
    let depth = attrs
        .get("depth")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    (kind, (depth - 1).min(8))
}

/// 单管道行 → 单元格序列：前导/尾随 `|` 剥离；未转义 `|` 分隔；
/// `\|` → `|` 反转义；单元格 trim；孤立 `\` 原样透传（无内联转义层）。
fn split_pipe_row(line: &str) -> Vec<String> {
    let s = line.strip_prefix('|').unwrap_or(line);
    let s = s.strip_suffix('|').unwrap_or(s);
    let mut cells: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c == '|' {
                cur.push('|');
            } else {
                cur.push('\\');
                cur.push(c);
            }
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur = String::new();
        } else {
            cur.push(c);
        }
    }
    if esc {
        cur.push('\\');
    }
    cells.push(cur.trim().to_string());
    cells
}

/// GFM 分隔行判定（每个 trim 后单元格 `:?-+:?` 形）。
fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            let core = t.strip_prefix(':').unwrap_or(t);
            let core = core.strip_suffix(':').unwrap_or(core);
            !core.is_empty() && core.chars().all(|ch| ch == '-')
        })
}

/// Table 块文本 → 表行序列（首个发射行后紧跟的分隔形行剔除；
/// 单元格 `\n`/`\r` → 空格；空白行跳过）。0 行 = 空表 → 调用方跳过。
/// t19：GFM 管道行语义单一真源 —— [`crate::xlsx_export`] 共用
///（pub(crate)，无行为变化）。
pub(crate) fn table_rows(text: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for line in text_lines(text) {
        if line.trim().is_empty() {
            continue;
        }
        let cells = split_pipe_row(line);
        if out.len() == 1 && is_separator_row(&cells) {
            continue;
        }
        out.push(
            cells
                .into_iter()
                .map(|c| c.replace(['\n', '\r'], " "))
                .collect(),
        );
    }
    out
}

/// 单块发射（先序递归；降级语义见模块文档映射表）。`quote_depth` =
/// 引用上下文深度（0 = 无引用面；Blockquote 子块递归传 depth+1 嵌套
/// 步进——仅段落/嵌套引用承载 ind/pBdr，其余子块登记见模块文档）。
fn emit_block(
    tree: &BlockTree,
    id: NodeId,
    w: &mut DocxWriter,
    quote_depth: usize,
) -> Result<(), ExportError> {
    let node = tree.node(id);
    let text = node.text.to_string();
    match node.kind {
        // 虚根透明递归（top_level 已绕过；op 面禁止 Document 子块——防御）
        BlockKind::Document => {
            for &c in &node.children {
                emit_block(tree, c, w, quote_depth)?;
            }
        }
        BlockKind::Heading => {
            let level = heading_level(&node.attrs);
            for line in text_lines(&text) {
                w.heading(level, line).map_err(|e| map_error(&e))?;
            }
        }
        BlockKind::Paragraph => emit_rich_paragraphs(w, &text, &node.marks, quote_depth),
        BlockKind::CodeBlock => {
            // 双通道等宽（t20 消解）；lang/fence 降级登记见模块文档。
            for line in text_lines(&text) {
                w.code_line(line);
            }
        }
        BlockKind::Table => {
            let rows = table_rows(&text);
            if !rows.is_empty() {
                w.table(&rows);
            }
        }
        BlockKind::Blockquote => {
            // 引用面（t20 消解）：嵌套步进 depth+1；空引用（无子）按
            // text 发射——镜像 markdown.rs 序列化语义。
            let d = quote_depth + 1;
            if node.children.is_empty() {
                emit_rich_paragraphs(w, &text, &node.marks, d);
            }
            for &c in &node.children {
                emit_block(tree, c, w, d)?;
            }
        }
        BlockKind::ListItem => {
            // 列表结构（t20 消解）：marker 判序 + depth→ilvl。多行 item
            // 文本（t22 消解）：单行 → 编号段（既有面，字节级零变化）；
            // 多行 → [`DocxWriter::list_item_lines`]——一个列表项 = 一个
            // OOXML 段落（行间 w:br 软换行 + 段落级 hanging ind），
            // 续行与首行文字列对齐（每行 run 序由 rich_lines 平移行内
            // marks）。子块递归（嵌套 item 各自编号段，次序保持）。
            let (kind, ilvl) = list_shape(&node.attrs);
            let lines = rich_lines(&text, &node.marks);
            let line_runs: Vec<Vec<RunSpec>> = lines
                .iter()
                .map(|(line, line_marks)| runs_from(line, line_marks))
                .collect();
            if line_runs.len() <= 1 {
                // 单行：既有 list_item 路径不动（无显式 ind——稳定面）。
                if let Some(first) = line_runs.first() {
                    w.list_item(first, kind, ilvl).map_err(|e| map_error(&e))?;
                }
            } else {
                w.list_item_lines(&line_runs, kind, ilvl)
                    .map_err(|e| map_error(&e))?;
            }
            for &c in &node.children {
                emit_block(tree, c, w, quote_depth)?;
            }
        }
        BlockKind::ThematicBreak => {
            // 水平线（t20 消解）：空段下边框。
            w.horizontal_rule();
        }
    }
    Ok(())
}

/// 编辑器核心块树 → DOCX 字节（zip 容器）。映射全函数：不可表达面
/// 显式降级（模块文档映射表），映射层永不报错；错误面 = 引擎写/打包。
pub fn export_docx_core(tree: &BlockTree) -> Result<Vec<u8>, ExportError> {
    let mut w = DocxWriter::new();
    for &child in tree.top_level() {
        emit_block(tree, child, &mut w, 0)?;
    }
    w.save_to_vec().map_err(|e| map_error(&e))
}

// ---------------------------------------------------------------------------
// wasm 壳（WasmCore 增量方法——既有类面零变化，t16/t17 自由函数模式的
// 差异 = 导出读 self 状态，故为方法；本 crate 内 impl 分文件合法）
// ---------------------------------------------------------------------------

#[wasm_bindgen]
impl WasmCore {
    /// 当前文档 → DOCX 字节（JS `Uint8Array`）。失败 = 抛 JSON 串
    /// `{"code":"docx_build"|"docx_limit","message":"..."}`
    /// （t16/t17 错误面约定 + r4-t2 资源限码）。
    #[wasm_bindgen]
    pub fn export_docx(&self) -> Result<Vec<u8>, JsValue> {
        export_docx_core(&self.inner.tree).map_err(|e| {
            JsValue::from_str(
                &serde_json::json!({ "code": e.code, "message": e.message }).to_string(),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// 原生测试（写 → read_docx_from_bytes → 模型断言；t16/t17 写读全环纪律）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_office_io::docx::{Block, ParagraphData, TableData};
    use mdcore::markdown::markdown_to_tree;

    /// 直构单块树（marks/attrs 精确控制——op/html 导入面的模型等价物）。
    fn single(kind: BlockKind, text: &str, attrs: Attrs, marks: Vec<MarkSpan>) -> BlockTree {
        let mut tree = BlockTree::new();
        let id = tree.create_node(kind, text, attrs, marks, None);
        let root = tree.root();
        let len = tree.node(root).children.len();
        tree.insert_child(root, len, id).expect("test attach");
        tree
    }

    fn span(start: usize, end: usize, kind: &str) -> MarkSpan {
        MarkSpan {
            start,
            end,
            kind: kind.into(),
        }
    }

    fn attr(k: &str, v: &str) -> Attrs {
        let mut a = Attrs::new();
        a.insert(k.into(), v.into());
        a
    }

    /// 写→读全环：字节必须过 office-io 读面（容器门 + 模型解析）。
    fn roundtrip(tree: &BlockTree) -> Vec<Block> {
        let bytes = export_docx_core(tree).expect("export");
        assert!(
            bytes.len() > 4 && &bytes[..2] == b"PK",
            "zip local header magic, {} bytes",
            bytes.len()
        );
        read_docx_from_bytes(&bytes).expect("read back").blocks
    }

    fn paras(blocks: &[Block]) -> Vec<ParagraphData> {
        blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.clone()),
                Block::Table(_) => None,
            })
            .collect()
    }

    /// (style, text, bold, italic) 四元组平铺——run 级断言便捷面。
    fn para_tuples(p: &ParagraphData) -> (Option<String>, Vec<(String, bool, bool)>) {
        (
            p.style.clone(),
            p.runs
                .iter()
                .map(|r| (r.text.clone(), r.bold, r.italic))
                .collect(),
        )
    }

    /// zip 本地头遍历取部件（t20 XML 面断言辅助）：docx-rs 打包恒
    /// `CompressionMethod::Stored` 无压缩（zipper/mod.rs:80-81）→ 本地
    /// 头记录的 size 即数据长度，直接切片零解压。office-io zip 依赖
    /// 不进 mdc-wasm（Cargo.lock 零 diff 纪律）——本遍历为测试面专用
    /// 最小实现（PK\x03\x04 签名步进：偏移 18/22 尺寸、26/28 名长）。
    fn zip_stored_part(bytes: &[u8], name: &str) -> String {
        let mut i = 0usize;
        while i + 30 <= bytes.len() {
            if &bytes[i..i + 4] != b"PK\x03\x04" {
                break; // 中央目录/EOCD 起点或非 zip——终止
            }
            let size =
                u32::from_le_bytes([bytes[i + 18], bytes[i + 19], bytes[i + 20], bytes[i + 21]])
                    as usize;
            let name_len = u16::from_le_bytes([bytes[i + 26], bytes[i + 27]]) as usize;
            let extra_len = u16::from_le_bytes([bytes[i + 28], bytes[i + 29]]) as usize;
            let entry_name =
                String::from_utf8_lossy(&bytes[i + 30..i + 30 + name_len]).into_owned();
            let data_start = i + 30 + name_len + extra_len;
            if entry_name == name {
                return String::from_utf8_lossy(&bytes[data_start..data_start + size]).into_owned();
            }
            i = data_start + size;
        }
        panic!("part {name} not found");
    }

    /// 导出字节 → document.xml（XML 面断言便捷面）。
    fn doc_xml(tree: &BlockTree) -> String {
        let bytes = export_docx_core(tree).expect("export");
        zip_stored_part(&bytes, "word/document.xml")
    }

    #[test]
    fn headings_map_to_styles_with_levels() {
        let tree = markdown_to_tree("# A\n## B\n###### F\nplain");
        let blocks = roundtrip(&tree);
        let ps = paras(&blocks);
        assert_eq!(ps.len(), 4, "3 headings + 1 paragraph");
        assert_eq!(ps[0].style.as_deref(), Some("Heading1"));
        assert_eq!(ps[1].style.as_deref(), Some("Heading2"));
        assert_eq!(ps[2].style.as_deref(), Some("Heading6"));
        assert_eq!(ps[3].style, None, "plain paragraph = no style");
        for (p, t) in ps.iter().zip(["A", "B", "F", "plain"]) {
            assert_eq!(p.text(), t);
        }
    }

    #[test]
    fn heading_level_default_and_clamp() {
        let over = single(BlockKind::Heading, "C", attr("level", "42"), vec![]);
        let bad = single(BlockKind::Heading, "D", attr("level", "abc"), vec![]);
        let none = single(BlockKind::Heading, "E", Attrs::new(), vec![]);
        let blocks = roundtrip(&over);
        assert_eq!(
            paras(&blocks)[0].style.as_deref(),
            Some("Heading9"),
            ">9 钳 9"
        );
        for tree in [&bad, &none] {
            let blocks = roundtrip(tree);
            assert_eq!(
                paras(&blocks)[0].style.as_deref(),
                Some("Heading1"),
                "不可解析/缺失 level → 1"
            );
        }
    }

    #[test]
    fn bold_italic_marks_roundtrip() {
        let tree = single(
            BlockKind::Paragraph,
            "hello bold world",
            Attrs::new(),
            vec![span(6, 10, "strong"), span(11, 16, "em")],
        );
        let ps = paras(&roundtrip(&tree));
        let (style, runs) = para_tuples(&ps[0]);
        assert_eq!(style, None);
        assert_eq!(
            runs,
            vec![
                ("hello ".into(), false, false),
                ("bold".into(), true, false),
                (" ".into(), false, false),
                ("world".into(), false, true),
            ],
            "char 偏移分段：plain / bold / 间隔空格 / italic"
        );
    }

    #[test]
    fn mark_adjacent_merge_and_overlap() {
        // strong[0,3]+strong[3,6] 邻接同式合并；strong[0,6] 与 em[3,9] 交叠三分
        let merged = single(
            BlockKind::Paragraph,
            "abcdef",
            Attrs::new(),
            vec![span(0, 3, "strong"), span(3, 6, "strong")],
        );
        let (_, runs) = para_tuples(&paras(&roundtrip(&merged))[0]);
        assert_eq!(
            runs,
            vec![("abcdef".into(), true, false)],
            "邻接同样式并 1 run"
        );

        let overlap = single(
            BlockKind::Paragraph,
            "abcdefghij",
            Attrs::new(),
            vec![span(0, 6, "strong"), span(3, 9, "em")],
        );
        let (_, runs) = para_tuples(&paras(&roundtrip(&overlap))[0]);
        assert_eq!(
            runs,
            vec![
                ("abc".into(), true, false),
                ("def".into(), true, true),
                ("ghi".into(), false, true),
                ("j".into(), false, false),
            ],
            "交叠 = 四段（bold / bold+italic / italic / 越界尾 plain）"
        );
    }

    /// t20 翻转（原 t18 `unsupported_inline_marks_degrade_to_plain`）：
    /// code/del/u 三 mark 降级已消解——XML 面直接断言（rFonts/strike/u）
    /// 与文本面回归（拼接零损；读模型 [`RunData`] 无字体/删除线/下画线
    /// 载荷——读面 run 分段不宣称往返等值，登记见模块文档）；
    /// link 维持纯文本（模型无 URL 载荷——上游缺口非降级）。
    #[test]
    fn inline_mono_del_u_expressed_link_plain() {
        let tree = single(
            BlockKind::Paragraph,
            "mono del u link",
            Attrs::new(),
            vec![
                span(0, 4, "code"),
                span(5, 8, "del"),
                span(0, 15, "u"),
                span(5, 8, "link"),
            ],
        );
        // XML 面：code → rFonts 三槽 Consolas；del → w:strike；u → w:u single。
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas" />"#)
                .count(),
            1,
            "code 段等宽面"
        );
        assert_eq!(xml.matches("<w:strike />").count(), 1, "del 段删除线");
        assert_eq!(
            xml.matches(r#"<w:u w:val="single" />"#).count(),
            4,
            "u 全段下画线（四 run 逐段）"
        );
        // 文本面：拼接零损；读模型不分段承载新位（四 run 均无 bold/italic）。
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps[0].text(), "mono del u link", "文本零损");
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![
                ("mono".into(), false, false),
                (" ".into(), false, false),
                ("del".into(), false, false),
                (" u link".into(), false, false),
            ],
            "读模型无新位载荷——分段可见但不宣称往返等值（登记）"
        );
    }

    #[test]
    fn table_roundtrip_escaped_pipe_and_separator_drop() {
        let tree = markdown_to_tree("| a | b \\| c |\n| --- | --- |\n| 1 | 2 |");
        let blocks = roundtrip(&tree);
        let tables: Vec<TableData> = blocks
            .into_iter()
            .filter_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tables.len(), 1);
        assert_eq!(
            tables[0].rows,
            vec![
                vec!["a".to_string(), "b | c".to_string()],
                vec!["1".to_string(), "2".to_string()],
            ],
            "分隔行剔除 + \\| 反转义"
        );
    }

    #[test]
    fn code_block_degrades_to_plain_paragraphs() {
        let tree = markdown_to_tree("```rust\nfn main() {}\nlet x = 1;\n```");
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps.len(), 2);
        for (p, t) in ps.iter().zip(["fn main() {}", "let x = 1;"]) {
            assert_eq!(
                p.style, None,
                "lang 丢弃 → 无样式段（等宽面已表达但非样式通道）"
            );
            assert_eq!(p.text(), t);
        }
    }

    /// t20：块级 code 双通道等宽——XML 面直断言（段落级 pPr>rPr>rFonts
    /// 与每 run rFonts，Courier New）；文本面回归 = 既有
    /// `code_block_degrades_to_plain_paragraphs`（读模型无字体面）。
    #[test]
    fn code_block_mono_dual_channel_xml() {
        let tree = markdown_to_tree("```rust\nfn main() {}\nlet x = 1;\n```");
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(
                r#"<w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" />"#
            )
            .count(),
            2,
            "段落级通道 ×2 行"
        );
        assert_eq!(
            xml.matches(
                r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" />"#
            )
            .count(),
            2,
            "run 级通道 ×2 行（每行单 run）"
        );
        // 空行 code 块：空段也走双通道（行级哲学零例外）。
        let empty = markdown_to_tree("```\n\n```");
        let xml = doc_xml(&empty);
        assert!(xml.contains(r#"<w:pPr><w:rPr><w:rFonts w:ascii="Courier New""#));
    }

    /// t20：列表结构消解——marker 判序（数字型→Ordered，`-`→Bullet）
    /// + depth→ilvl；numPr 精确配对 + numbering.xml 双抽象（XML 面）
    /// + 文本面回归（次序保持）。
    #[test]
    fn list_mapping_numbering_xml() {
        let tree = markdown_to_tree("1. a\n2. b\n- c\n  - nested");
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="100" /><w:ilvl w:val="0" />"#)
                .count(),
            2,
            "有序 ilvl=0 ×2"
        );
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="0" />"#)
                .count(),
            1,
            "无序 ilvl=0 ×1"
        );
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="1" />"#)
                .count(),
            1,
            "嵌套无序 ilvl=1 ×1（depth 2 → ilvl 1）"
        );
        // 文本面：次序保持。
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["a", "b", "c", "nested"]);
    }

    /// t20：marker 形态面 + depth 钳位（直构 attrs——`1)` 形 / 深度越界
    /// / 缺失防御缺省）。
    #[test]
    fn list_marker_forms_and_depth_clamp() {
        // `1)` 形 → Ordered。
        let mut attrs = Attrs::new();
        attrs.insert("marker".into(), "1)".into());
        attrs.insert("depth".into(), "1".into());
        let paren = single(BlockKind::ListItem, "p", attrs, vec![]);
        let xml = doc_xml(&paren);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="100" /><w:ilvl w:val="0" />"#)
                .count(),
            1,
            "`1)` 形数字 marker → Ordered"
        );
        // depth 99 → ilvl 钳 8（映射层全函数，不报错）。
        let mut attrs = Attrs::new();
        attrs.insert("marker".into(), "-".into());
        attrs.insert("depth".into(), "99".into());
        let deep = single(BlockKind::ListItem, "d", attrs, vec![]);
        let xml = doc_xml(&deep);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="8" />"#)
                .count(),
            1,
            "depth 99 → ilvl 钳 8"
        );
        // attrs 全缺失 → Bullet + ilvl 0（防御缺省）。
        let bare = single(BlockKind::ListItem, "x", Attrs::new(), vec![]);
        let xml = doc_xml(&bare);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="0" />"#)
                .count(),
            1,
            "缺失 attrs → Bullet ilvl=0"
        );
    }

    /// t22 翻转（原 t20 `list_multiline_item_first_numbered_rest_plain`
    /// 平铺断言）：多行 item 续行缩进已表达——一个列表项 = 一个 OOXML
    /// 段落（numPr 单点 + 行间 w:br + 段落级 hanging ind，续行与首行
    /// 文字列对齐）。读面 br → `\n`（t16 约定）→ 单段文本零损；列表
    /// kind 往返等值仍不宣称（读模型无 numPr——登记维持）。
    /// CRLF：行尾 `\r` 在发射面剥除（rich_lines），br 替位 `\n`。
    #[test]
    fn list_multiline_item_single_paragraph_br_hanging() {
        let tree = single(
            BlockKind::ListItem,
            "head line\ntail line",
            attr("marker", "-"),
            vec![],
        );
        // XML 面：单段 numPr（邻接串 = 同段性）+ br 软换行 + 段落级
        // hanging ind（left=720、hanging=360——与编号抽象层级同值）。
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(r#"<w:numPr><w:numId w:val="101" /><w:ilvl w:val="0" /></w:numPr><w:ind w:left="720" w:right="0" w:hanging="360" />"#)
                .count(),
            1,
            "多行项 = 单段（numPr→ind 邻接）"
        );
        assert_eq!(
            xml.matches(r#"<w:br w:type="textWrapping" />"#).count(),
            1,
            "两行一 br"
        );
        // 文本面：读回单段含 `\n`（br→\n），非平铺两段。
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["head line\ntail line"]);

        // CRLF 容忍："x\r\ny" → br 单段（\r 剥除）。
        let crlf = single(BlockKind::ListItem, "x\r\ny", attr("marker", "-"), vec![]);
        let ps = paras(&roundtrip(&crlf));
        assert_eq!(ps.len(), 1, "CRLF 多行同为单段");
        assert_eq!(ps[0].text(), "x\ny");

        // 单行项零漂移对照：无段落级 ind（既有 list_item 面）。
        let single_line = single(BlockKind::ListItem, "solo", attr("marker", "-"), vec![]);
        let xml = doc_xml(&single_line);
        assert_eq!(
            xml.matches(
                r#"<w:numPr><w:numId w:val="101" /><w:ilvl w:val="0" /></w:numPr></w:pPr>"#
            )
            .count(),
            1,
            "单行项 pPr 止于 numPr（无 ind）"
        );
        assert_eq!(xml.matches("<w:br").count(), 0, "单行无 br");
    }

    /// t22：嵌套多行 item（depth 2 → ilvl 1）——ind 步进 1440、
    /// hanging 同值 360；三行（含空行）→ br ×2；行内 marks 跨行
    /// 平移保持（rich_lines 单一真源——多行列表路径接线验证）。
    #[test]
    fn list_multiline_item_nested_depth_and_marks() {
        // "top\n\nbottom"：中空行；strong 覆盖第二行前 4 char。
        let mut attrs = Attrs::new();
        attrs.insert("marker".into(), "1.".into());
        attrs.insert("depth".into(), "2".into());
        let tree = single(
            BlockKind::ListItem,
            "top\n\nbottom",
            attrs,
            vec![span(5, 9, "strong")],
        );
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(
                r#"<w:numPr><w:numId w:val="100" /><w:ilvl w:val="1" /></w:numPr><w:ind w:left="1440" w:right="0" w:hanging="360" />"#
            )
            .count(),
            1,
            "depth 2 → ilvl 1：ind left = 720×(1+1) = 1440"
        );
        assert_eq!(
            xml.matches(r#"<w:br w:type="textWrapping" />"#).count(),
            2,
            "三行（空行居中）→ br ×2"
        );
        // 文本面：单段 "top\n\nbottom"；第二行 strong 平移（bold run）。
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text(), "top\n\nbottom");
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![
                ("top".into(), false, false),
                ("\n".into(), false, false),
                ("\n".into(), false, false),
                ("bott".into(), true, false),
                ("om".into(), false, false),
            ],
            "strong[5,9] → 第二行 [0,4]（char 平移），br run 读为独立 \\n"
        );
    }

    /// t20：引用面 + hr 消解——ind 720×depth 嵌套步进 + 左边框 +
    /// 空段下边框（XML 面）+ 文本面回归（次序保持）。
    #[test]
    fn quote_and_hr_mapping_xml() {
        let tree = markdown_to_tree("> q\n>> deep\n---");
        let xml = doc_xml(&tree);
        assert!(xml.contains(r#"w:ind w:left="720""#), "引用 depth 1");
        assert!(
            xml.contains(r#"w:ind w:left="1440""#),
            "嵌套引用 depth 2 步进"
        );
        assert_eq!(
            xml.matches(
                r#"<w:pBdr><w:left w:val="single" w:space="4" w:sz="12" w:color="auto" /></w:pBdr>"#
            )
            .count(),
            2,
            "两引用段各带左边框"
        );
        assert!(
            xml.contains(
                r#"<w:pBdr><w:bottom w:val="single" w:space="1" w:sz="6" w:color="auto" /></w:pBdr>"#
            ),
            "hr 空段下边框"
        );
        // 文本面：次序保持（hr = 空段）。
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["q", "deep", ""]);
    }

    #[test]
    fn quote_list_divider_flatten_in_order() {
        let tree = markdown_to_tree("> quoted\n- item\n  - nested\n---");
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(
            texts,
            vec![
                "quoted".to_string(),
                "item".to_string(),
                "nested".to_string(),
                String::new(),
            ],
            "引用/列表平铺（次序保持）+ 分隔线 → 空段"
        );
        assert!(ps.iter().all(|p| p.style.is_none()));
    }

    #[test]
    fn multiline_paragraph_splits_with_marks_and_crlf() {
        // 多行文本：每行一段，mark 按行区间平移（第一行 bold）
        let tree = single(
            BlockKind::Paragraph,
            "bold a\nplain b",
            Attrs::new(),
            vec![span(0, 6, "strong")],
        );
        let ps = paras(&roundtrip(&tree));
        let (_, r0) = para_tuples(&ps[0]);
        let (_, r1) = para_tuples(&ps[1]);
        assert_eq!(r0, vec![("bold a".into(), true, false)]);
        assert_eq!(r1, vec![("plain b".into(), false, false)]);

        // CRLF：行尾 \r 剥离（markdown.rs split('\n') 保留 \r 的容忍面）
        let crlf = single(BlockKind::Paragraph, "x\r\ny", Attrs::new(), vec![]);
        let ps = paras(&roundtrip(&crlf));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn empty_tree_yields_valid_minimal_docx() {
        let tree = BlockTree::new();
        let bytes = export_docx_core(&tree).expect("export empty");
        assert!(bytes.len() > 4 && &bytes[..2] == b"PK");
        let doc = read_docx_from_bytes(&bytes).expect("read back");
        assert!(doc.blocks.is_empty(), "空文档 = 0 块（最小合法 docx）");
        assert!(doc.headers.is_empty() && doc.footers.is_empty());
    }

    /// r4-t2：资源限错误码可区分——`docx_limit`（office-io
    /// `MAX_PARAGRAPHS`/`MAX_RUNS`，save 面 typed 拒、超限导出无产物
    /// 字节）≠ `docx_build`（引擎面既有单族）。常量单一真源在
    /// office-io docx.rs，本层零自立数字。
    #[test]
    fn limit_error_code_mapping() {
        let e = map_error(&mdc_office_io::docx::DocxError::Limit(
            "paragraph count over limit 1000000".into(),
        ));
        assert_eq!(e.code, "docx_limit");
        assert!(
            e.message.starts_with("docx writer resource limit"),
            "{:?}",
            e.message
        );
        // 既有面不动：Container/Parse/Io 仍归 container_prescan/
        // docx_parse/io（读面）与 docx_build（写面引擎）。
        for (err, code) in [
            (
                mdc_office_io::docx::DocxError::Parse("x".into()),
                "docx_build",
            ),
            (mdc_office_io::docx::DocxError::Io("x".into()), "docx_build"),
        ] {
            assert_eq!(map_error(&err).code, code);
        }
    }
}
