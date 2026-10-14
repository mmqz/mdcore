//! t18：编辑器核心文档 → DOCX 导出切片（office-io 写面经 wasm 桥）。
//!
//! **定位（2026-10-14，t18 立项）**：编辑器当前文档（mdcore
//! [`BlockTree`]——唯一语义真理，主档 §9）→ [`mdc_office_io::docx`]
//! 的 [`DocxWriter`] 写面 → zip 容器字节 → web 侧 `Blob + a.download`
//! 下载 `document.docx`。与 t16（DOCX 导入）合流 = DOCX 往返闭环；
//! 三域纪律唯一新边沿用 t16/t17 的 mdcore-wasm → office-io（消费方
//! 边界；office-io 对 mdcore 零依赖不变）。
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
//! | `docx_build` | 引擎写/打包错误（[`DocxError::Io`] 打包失败为唯一现实路径；[`DocxError::Parse`] = heading 级越界，映射层恒钳位 1-9 故不可达，完备性仍映射）。**映射层是全函数——降级永不报错**；`Container` 变体只在读面产生，写侧不可达（完备性登记）。 |
//!
//! ## 映射表（BlockKind → DocxWriter；模型级，非保真渲染）
//!
//! | mdcore 块 | DocxWriter 调用 | 降级（不可表达面，显式登记） |
//! |---|---|---|
//! | `Heading`（attrs.level，缺省/不可解析 → 1；钳 1-9） | [`DocxWriter::heading`]（Heading1-9 样式 + outline 级） | 模型 level 可为任意 usize（op 面），>9 钳 9；**标题 marks 丢弃**（[`DocxWriter::heading`] 纯文本面——html.rs 导入可产带 marks 的标题） |
//! | `Paragraph`（text + marks） | [`DocxWriter::paragraph`] run 序 | mark kind 自由字符串：`strong`/`bold` → 加粗、`em`/`italic` → 斜体（html.rs 规范 = strong/em，spike 测试 = bold，双拼写并收）；**`code`/`del`/`u` 及未知 kind → 纯文本**（RunSpec 无等宽/删除线/下面线面）；**link kind → 纯文本**——模型 [`MarkSpan`] 无 URL 载荷，目标不可表达（html.rs 亦无 `<a>` 处理面——模型级无链接表示） |
//! | `CodeBlock`（attrs.lang + text） | 每行一个纯文本段 | **等宽/字体面、lang、fence 结构全部丢弃**（RunSpec 无字体面；DocxWriter 仅预置 Heading1-9 样式）；文本逐字保留 |
//! | `Table`（text = 规范管道行） | [`DocxWriter::table`] | 第 2 行分隔行剔除（GFM 语义位）；`\|` 反转义 → `\|`；单元格内 `\n`/`\r` → 空格（GFM 单元格单行面，t17 同向）；孤立 `\` 原样透传（模型无内联转义层——t16 同边界登记）；单元格内 marks 本就不可承载（Table = 扁平文本面，html.rs 同边界）；0 行表跳过（空 `w:tbl` 非 OOXML 合法形——防御，markdown 解析不产空表） |
//! | `Blockquote` | 子块平铺递归 | 引用容器/缩进丢失；空引用（无子且 text 非空）镜像 markdown.rs 序列化语义（L90-93）按 text 发射 |
//! | `ListItem`（attrs.marker/depth） | item text 富段 + 子块递归 | **marker/depth（列表结构）丢弃**（t14 已登记 office-io 列表写面未暴露；富文档语义不注入 `- ` 伪 markdown）；嵌套项 = 各自段落，次序保持 |
//! | `ThematicBreak` | 空段落 | 无水平线面；空段 = 分隔视觉提示 |
//!
//! 文本行处理（全局规则）：块文本按 `\n` 分行、每行剥一个行尾 `\r`
//! （CRLF 容忍——markdown.rs `split('\n')` 保留 `\r`），多行文本 →
//! 每行一个 docx 段落（mdcore 行级哲学，t16 w:br→`\n` 同向登记）；
//! 行内孤立 `\r` 原样透传。mark 偏移单位 = char（op.rs SetMark 契约），
//! 多行切分时按行区间裁剪平移。
//!
//! ## 已知边界（登记，非缺陷）
//!
//! - run 相邻同 (bold,italic) 合并（docx 段落 run 边界不承载模型语义，
//!   t16 导入侧同款合并登记的反向）；空 run 不发射；
//! - 导出的 heading 样式读回 = `Heading1`..`Heading9`（office-io 读面
//!   样式 ID 透传）——全环断言见本模块测试；
//! - 不暴露面（DocxWriter 能力中本切片未接线）：页眉/页脚槽位、PNG
//!   图片（模型无图片块型——BlockKind 无 image 变体，非降级而是
//!   无源可映射）、脚注、域。
//!
//! 纯内核（[`export_docx_core`]）不触碰 JsValue → 原生 cargo test
//! 直测（写→`read_docx_from_bytes`→模型断言，t16/t17 同款写读全环
//! 纪律）；wasm 壳由 wasm32 check + web 三件套门禁背书。

use crate::WasmCore;
#[cfg(test)]
use mdc_office_io::docx::read_docx_from_bytes;
use mdc_office_io::docx::{DocxError, DocxWriter, RunSpec};
use mdcore::tree::{Attrs, BlockKind, BlockTree, MarkSpan, NodeId};
use wasm_bindgen::prelude::*;

/// typed 导出错误（错误码 + 人类可读消息——见模块文档错误码表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
}

/// 引擎写面错误 → 错误码（写面唯一族 `docx_build`——见模块文档）。
fn map_error(e: &DocxError) -> ExportError {
    ExportError {
        code: "docx_build",
        message: format!("docx writer engine error: {e}"),
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

/// mark kind → (bold, italic)。模型 kind 为自由字符串（op.rs SetMark）：
/// `strong`/`bold`（html.rs 规范 / spike 命名）→ 加粗；`em`/`italic` →
/// 斜体；其余（code/del/u/未知/link）→ 无 docx run 面可表达 → 纯文本。
fn mark_style(kind: &str) -> (bool, bool) {
    match kind {
        "strong" | "bold" => (true, false),
        "em" | "italic" => (false, true),
        _ => (false, false),
    }
}

/// 单行文本 + char 偏移 marks → run 序（边界集分段 + 相邻同
/// (bold,italic) 合并 + 空段跳过）。空文本 → 空 run 序。
fn runs_from(text: &str, marks: &[MarkSpan]) -> Vec<RunSpec> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut bold: Vec<(usize, usize)> = Vec::new();
    let mut italic: Vec<(usize, usize)> = Vec::new();
    for m in marks {
        // 防御钳位（op 面写入时已校验 ≤ len_chars；adjust_marks 维护不变量）
        let s = m.start.min(len);
        let e = m.end.min(len).max(s);
        if s == e {
            continue;
        }
        let (b, i) = mark_style(&m.kind);
        if b {
            bold.push((s, e));
        }
        if i {
            italic.push((s, e));
        }
    }
    let mut points: Vec<usize> = vec![0, len];
    points.extend(bold.iter().chain(italic.iter()).flat_map(|&(s, e)| [s, e]));
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
        let seg: String = chars[a..b].iter().collect();
        // 相邻同样式合并（模型无链接 run——link 恒 None，样式同即并）
        if let Some(last) = runs.last_mut() {
            if last.bold == bold_seg && last.italic == italic_seg {
                last.text.push_str(&seg);
                continue;
            }
        }
        runs.push(RunSpec::styled(seg, bold_seg, italic_seg));
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

/// 富文本（可多行）→ 每行一个 docx 段落。mark 的 char 偏移按行区间
/// 裁剪平移到行内坐标（多行切分不丢格式）。
fn emit_rich_paragraphs(w: &mut DocxWriter, text: &str, marks: &[MarkSpan]) {
    let chars: Vec<char> = text.chars().collect();
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
        w.paragraph(&runs_from(&line, &line_marks));
        if nl == chars.len() {
            break;
        }
        start = nl + 1;
    }
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

/// 单块发射（先序递归；降级语义见模块文档映射表）。
fn emit_block(tree: &BlockTree, id: NodeId, w: &mut DocxWriter) -> Result<(), ExportError> {
    let node = tree.node(id);
    let text = node.text.to_string();
    match node.kind {
        // 虚根透明递归（top_level 已绕过；op 面禁止 Document 子块——防御）
        BlockKind::Document => {
            for &c in &node.children {
                emit_block(tree, c, w)?;
            }
        }
        BlockKind::Heading => {
            let level = heading_level(&node.attrs);
            for line in text_lines(&text) {
                w.heading(level, line).map_err(|e| map_error(&e))?;
            }
        }
        BlockKind::Paragraph => emit_rich_paragraphs(w, &text, &node.marks),
        BlockKind::CodeBlock => {
            for line in text_lines(&text) {
                w.paragraph_text(line);
            }
        }
        BlockKind::Table => {
            let rows = table_rows(&text);
            if !rows.is_empty() {
                w.table(&rows);
            }
        }
        BlockKind::Blockquote => {
            // 空引用（无子）按 text 发射——镜像 markdown.rs 序列化语义
            if node.children.is_empty() {
                emit_rich_paragraphs(w, &text, &node.marks);
            }
            for &c in &node.children {
                emit_block(tree, c, w)?;
            }
        }
        BlockKind::ListItem => {
            // marker/depth 丢弃（列表写面未暴露——模块文档登记）
            emit_rich_paragraphs(w, &text, &node.marks);
            for &c in &node.children {
                emit_block(tree, c, w)?;
            }
        }
        BlockKind::ThematicBreak => {
            // 无水平线面：空段 = 分隔视觉提示
            w.paragraph(&[]);
        }
    }
    Ok(())
}

/// 编辑器核心块树 → DOCX 字节（zip 容器）。映射全函数：不可表达面
/// 显式降级（模块文档映射表），映射层永不报错；错误面 = 引擎写/打包。
pub fn export_docx_core(tree: &BlockTree) -> Result<Vec<u8>, ExportError> {
    let mut w = DocxWriter::new();
    for &child in tree.top_level() {
        emit_block(tree, child, &mut w)?;
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
    /// `{"code":"docx_build","message":"..."}`（t16/t17 错误面约定）。
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

    #[test]
    fn unsupported_inline_marks_degrade_to_plain() {
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
        let ps = paras(&roundtrip(&tree));
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![("mono del u link".into(), false, false)],
            "code/del/u/link 无 docx run 面 → 纯文本（文本零损）"
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
            assert_eq!(p.style, None, "lang/等宽面丢弃 → 无样式段");
            assert_eq!(p.text(), t);
        }
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
}
