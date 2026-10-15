//! t16：DOCX → Markdown 导入切片（office-io 首个消费边，经 wasm 桥）。
//!
//! **定位（2026-10-17，t16 立项）**：把 [`mdc_office_io::docx`] 的硬化
//! 读面（容器三门 + 自有 OPC 关系层 + docx-rs 模型解析）经本 crate 暴露
//! 给 web 编辑器——「在编辑器里打开 .docx」的最小真实切片。三域纪律
//! 的唯一新边 = mdcore-wasm → office-io（消费方边界；office-io 对
//! mdcore 零依赖不变）。
//!
//! ## 暴露面（[`import_docx`]，wasm-bindgen 自由函数——不碰 WasmCore
//! 状态，既有类面零变化）
//!
//! - 入参：DOCX 字节（JS `Uint8Array` → `&[u8]`）。
//! - 成功：JSON 字符串 `{"markdown": "...", "stats": {...}}`——
//!   markdown 直接喂 [`crate::WasmCore::new`] / 宿主注入契约
//!   （web 侧 `window.__inject`）；stats = 紧凑摘要（块计数 + 兼容
//!   模式容器警告 + 不导入面计数）。
//! - 失败：抛 JSON 字符串 `{"code": "...", "message": "..."}`——
//!   typed 错误码（下表），宿主可 `JSON.parse` 出结构化错误。
//!
//! ## 错误码表（office-io 错误面 → 码——容器门拒与引擎解析错误分离）
//!
//! | office-io 错误 | code | 含义 |
//! |---|---|---|
//! | [`DocxError::Container`]([`mdc_office_io::io::ReadError::PrescanRejected`]) | `container_prescan` | 门 1 拒（条目数/单尺寸/总量/路径逃逸——敌意输入零解析层接触） |
//! | `Container(ContainerCorrupt)` | `container_corrupt` | zip 容器损坏（EOCD/中央目录不可读——垃圾字节/截断档案） |
//! | `Container(ParseError)` | `container_parse` | 过门后解析层错误（zip 读界） |
//! | `Container(InvalidPart)` | `container_invalid_part` | 门 3 部件良构性拒（畸形 XML——兼容模式下仅特定面仍可到这） |
//! | [`DocxError::Parse`] | `docx_parse` | docx-rs 模型解析错误（引擎面） |
//! | [`DocxError::Io`] | `io` | 文件 IO（字节入口实际不可达，完备性映射） |
//!
//! 读模式 = **兼容**（[`mdc_office_io::io::ReadOptions::compat`]）：
//! 可恢复的畸形部件降级为结构化警告随 stats 返回（可见性），敌意
//! 输入防线（门 1/门 2 与全部安全上限）两模式恒等——垃圾/截断档案
//! 仍 typed 拒绝。
//!
//! ## 映射规则（DocxDocument → markdown；模型级，非保真渲染）
//!
//! - 段落样式 `Heading1`..`Heading9` → `#`×level 标题（**7-9 钳到 6**，
//!   MD 上限 h6）；其余样式 ID（Title/Subtitle/自定义）→ 普通段落
//!   （office-io 模型级不做样式语义解释，本层只认 Word 规范标题 ID）；
//! - run：加粗 → `**`、斜体 → `*`、超链接 → `[text](target)`（外链
//!   URL 含空白/`)`/空 → `<...>` 角括号形；锚点 → `#anchor`；包内
//!   目标原样透传；r:id 无对应条目 [`HyperlinkTarget::Unresolved`] →
//!   降级纯文本 + BrokenReference 警告随 stats）；
//!   **相邻同格式 run 合并**（MD 不承载 run 边界；`*a*`+`*b*` 直连
//!   会产出 CommonMark 歧义串 `*a**b*`）；
//! - 表 → GFM 管道表（首行表头 + `| --- |` 分隔行 + 体行——与
//!   mdcore `BlockKind::Table` 规范文本同构，html.rs
//!   `canonical_table_text` 同形）；单元格清洗：`|` → `\|` 转义、
//!   `\n`/`\r` → 空格（GFM 单元格不承载换行）、短行补空格对齐表头宽；
//! - 空段落跳过（mdcore 空行不建块）；段落内软换行（w:br → `\n`）
//!   原样输出——mdcore 行级解析按行建块（一段拆多块，登记）。
//!
//! ## 不导入面（模型级数据在 MD 中不可表示——计数进 stats，诚实边界）
//!
//! - 图片（`images_skipped` = 正文/页眉页脚 run 级引用数 >0 的资产；
//!   孤儿媒体不计——非用户可见内容）；
//! - 页眉/页脚（`headers_skipped`/`footers_skipped` = 六槽位中实际
//!   挂接数）；脚注（引擎读面受限，office-io 侧登记面原样继承）。
//!
//! ## 已知边界（登记，非缺陷）
//!
//! - 段落文本以 MD 结构字符开头（`#`/`|`/`>`/`- `/数字`.` 等）会被
//!   mdcore 行级解析解释为对应块型——导入是结构映射而非显示保真；
//! - 表格单元格内超链接目标（office-io TableData 扁平文本面）随
//!   单元格文本合并，目标丢弃；
//! - 内联 `*`/`_`/`[` 等字符不转义（mdcore 无内联转义层，转义会
//!   把反斜杠写进文本）。
//!
//! 纯内核（[`import_docx_core`] 等）不触碰 JsValue → 原生 cargo test
//! 直测（JsValue 非 wasm 目标构造即 panic，见 lib.rs `parse_source_str`
//! 先例）；wasm 壳由 wasm32 check + web 三件套门禁背书。

use mdc_office_io::docx::{
    read_docx_from_bytes_with_options, Block, DocxDocument, DocxError, HyperlinkTarget, RunData,
    TableData,
};
use mdc_office_io::io::{ReadError, ReadOptions};
use wasm_bindgen::prelude::*;

/// typed 导入错误（错误码 + 人类可读消息——见模块文档错误码表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError {
    pub code: &'static str,
    pub message: String,
}

/// office-io 错误面 → 错误码映射（容器门拒与引擎解析错误分离；
/// r4-t2：写面资源限 `docx_limit` 与格式/引擎类可区分）。
fn map_error(e: &DocxError) -> ImportError {
    match e {
        DocxError::Container(read) => ImportError {
            code: match read {
                ReadError::PrescanRejected { .. } => "container_prescan",
                ReadError::ContainerCorrupt(_) => "container_corrupt",
                ReadError::ParseError(_) => "container_parse",
                ReadError::InvalidPart { .. } => "container_invalid_part",
            },
            message: read.to_string(),
        },
        DocxError::Parse(m) => ImportError {
            code: "docx_parse",
            message: format!("docx-rs model parse error: {m}"),
        },
        DocxError::Io(m) => ImportError {
            code: "io",
            message: format!("io error: {m}"),
        },
        DocxError::Limit(m) => ImportError {
            code: "docx_limit",
            message: format!("docx writer resource limit: {m}"),
        },
    }
}

/// 兼容模式读选项（可恢复畸形部件降级警告；敌意防线两模式恒等）。
fn compat_options() -> ReadOptions {
    ReadOptions { compat: true }
}

// ---------------------------------------------------------------------------
// 纯映射内核（原生可测——构造 DocxDocument 直测，零 zip 依赖）
// ---------------------------------------------------------------------------

/// 段落样式 ID → 标题级别（Word 规范 `Heading1`..`Heading9`；其余
/// 含 Title/Subtitle/自定义 → None = 普通段落——模型级样式语义边界）。
fn heading_level(style: Option<&str>) -> Option<usize> {
    let rest = style?.strip_prefix("Heading")?;
    let level: usize = rest.parse().ok()?;
    (1..=9).contains(&level).then_some(level)
}

/// 相邻同格式 run 合并（bold/italic/超链接目标三元组相等——MD 不承载
/// run 边界，直连产出 CommonMark 歧义串）。
fn merge_runs(runs: &[RunData]) -> Vec<RunData> {
    let mut merged: Vec<RunData> = Vec::with_capacity(runs.len());
    for r in runs {
        if let Some(last) = merged.last_mut() {
            if last.bold == r.bold && last.italic == r.italic && last.hyperlink == r.hyperlink {
                last.text.push_str(&r.text);
                continue;
            }
        }
        merged.push(r.clone());
    }
    merged
}

/// 链接目标 → GFM destination（空/含空白/含 `)` → 角括号形；锚点加
/// `#` 前缀；Unresolved → None = 降级纯文本）。
fn link_destination(target: &HyperlinkTarget) -> Option<String> {
    match target {
        HyperlinkTarget::External { target } | HyperlinkTarget::Internal { target } => {
            Some(angle_wrap(target))
        }
        HyperlinkTarget::Anchor { anchor } => Some(format!("#{}", angle_wrap(anchor))),
        HyperlinkTarget::Unresolved { .. } => None,
    }
}

/// URL 形式规整（空/空白/`)` 需 `<...>` 包裹——GFM destination 语法）。
fn angle_wrap(target: &str) -> String {
    if target.is_empty()
        || target
            .chars()
            .any(|c| c.is_whitespace() || c == ')' || c == '<' || c == '>')
    {
        format!("<{target}>")
    } else {
        target.to_string()
    }
}

/// 单个（已合并）run → 内联 markdown。强调标记只裹住非空白核（`** x **`
/// 不构成 CommonMark 强调）；链接在外、强调在内（`[**b**](url)`）。
fn render_inline(run: &RunData) -> String {
    let text = &run.text;
    let core = text.trim();
    if core.is_empty() {
        // 纯空白 run：原样直出（标记裹空白 = 非法强调，无信息量丢弃）。
        return text.clone();
    }
    let lead_ws = &text[..text.len() - text.trim_start().len()];
    let trail_ws = &text[text.trim_end().len()..];
    let mut inner = core.to_string();
    if run.bold {
        inner = format!("**{inner}**");
    }
    if run.italic {
        inner = format!("*{inner}*");
    }
    let body = match run.hyperlink.as_ref().and_then(link_destination) {
        Some(dest) => format!("[{inner}]({dest})"),
        None => inner,
    };
    format!("{lead_ws}{body}{trail_ws}")
}

/// run 序 → 段落内联文本（返回文本 + 解析成功的链接计数）。
fn render_runs(runs: &[RunData]) -> (String, usize) {
    let mut out = String::new();
    let mut links = 0usize;
    for run in merge_runs(runs) {
        if run.hyperlink.as_ref().and_then(link_destination).is_some()
            && !run.text.trim().is_empty()
        {
            links += 1;
        }
        out.push_str(&render_inline(&run));
    }
    (out, links)
}

/// 单元格清洗：`|` 转义（GFM 列分隔符冲突）、`\n`/`\r` → 空格（单元格
/// 不承载换行）、首尾空白去（GFM 解析器惯例）。
fn sanitize_cell(cell: &str) -> String {
    let flat = if cell.contains('\n') || cell.contains('\r') {
        cell.chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect::<String>()
    } else {
        cell.to_string()
    };
    flat.trim().replace('|', "\\|")
}

/// 表 → GFM 管道表（表头 + `---` 分隔行 + 体行；与 mdcore Table 块
/// 规范文本同构）。空表 → None（跳过）；短行补空格到全表最大列宽。
fn render_table(table: &TableData) -> Option<String> {
    if table.rows.is_empty() {
        return None;
    }
    let width = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return None;
    }
    let render_row = |cells: &[String]| -> String {
        let padded = (0..width).map(|i| cells.get(i).map(|c| sanitize_cell(c)).unwrap_or_default());
        let mut line = String::from("|");
        for cell in padded {
            line.push(' ');
            line.push_str(&cell);
            line.push_str(" |");
        }
        line
    };
    let mut lines = vec![render_row(&table.rows[0])];
    let sep_cells: Vec<String> = std::iter::repeat_n(String::from("---"), width).collect();
    lines.push(render_row(&sep_cells));
    for row in &table.rows[1..] {
        lines.push(render_row(row));
    }
    Some(lines.join("\n"))
}

/// stats 的警告 kind → snake_case 字符串。
fn warning_kind(kind: &mdc_office_io::io::WarningKind) -> &'static str {
    match kind {
        mdc_office_io::io::WarningKind::MalformedXml => "malformed_xml",
        mdc_office_io::io::WarningKind::ControlChars => "control_chars",
        mdc_office_io::io::WarningKind::BrokenReference => "broken_reference",
    }
}

/// DocxDocument → (markdown, stats JSON)。纯函数（原生直测；宿主消费
/// 经 [`import_docx_core`] 字节入口）。
fn docx_to_markdown(doc: &DocxDocument) -> (String, serde_json::Value) {
    let mut lines: Vec<String> = Vec::new();
    let mut headings = 0usize;
    let mut paragraphs = 0usize;
    let mut tables = 0usize;
    let mut hyperlinks = 0usize;
    for block in &doc.blocks {
        match block {
            Block::Paragraph(p) => {
                let (text, links) = render_runs(&p.runs);
                hyperlinks += links;
                match heading_level(p.style.as_deref()) {
                    Some(level) => {
                        // MD 上限 h6（mdcore heading_of 1-6）——7-9 钳 6，登记。
                        let level = level.min(6);
                        lines.push(format!("{} {}", "#".repeat(level), text));
                        headings += 1;
                    }
                    None => {
                        // 空段落跳过不计数（mdcore 空行不建块）；软换行段落
                        // 原样多行——mdcore 行级解析拆多块，模块文档登记。
                        if !text.is_empty() {
                            lines.push(text);
                            paragraphs += 1;
                        }
                    }
                }
            }
            Block::Table(t) => {
                if let Some(md) = render_table(t) {
                    lines.push(md);
                    tables += 1;
                }
            }
        }
    }
    let images_skipped = doc.images.iter().filter(|a| a.body_uses > 0).count();
    let stats = serde_json::json!({
        "blocks": lines.len(),
        "headings": headings,
        "paragraphs": paragraphs,
        "tables": tables,
        "hyperlinks": hyperlinks,
        "images_skipped": images_skipped,
        "headers_skipped": doc.headers.len(),
        "footers_skipped": doc.footers.len(),
        "warnings": doc.warnings.iter().map(|w| serde_json::json!({
            "part": w.part,
            "kind": warning_kind(&w.kind),
            "detail": w.detail,
        })).collect::<Vec<_>>(),
    });
    (lines.join("\n\n"), stats)
}

/// 导入内核（字节入口；纯函数——无 JsValue，原生 cargo test 直测）。
/// 成功返回与 wasm 面同形的 JSON 串 `{"markdown": ..., "stats": ...}`。
pub(crate) fn import_docx_core(bytes: &[u8]) -> Result<String, ImportError> {
    let doc: DocxDocument =
        read_docx_from_bytes_with_options(bytes, &compat_options()).map_err(|e| map_error(&e))?;
    let (markdown, stats) = docx_to_markdown(&doc);
    Ok(serde_json::json!({ "markdown": markdown, "stats": stats }).to_string())
}

// ---------------------------------------------------------------------------
// wasm-bindgen 壳（JsValue 仅此层——lib.rs parse_source 先例同构）
// ---------------------------------------------------------------------------

/// 导入 .docx 字节 → `{"markdown": "...", "stats": {...}}`（JSON 串，
/// 形状见模块文档）；失败抛 JSON 串 `{"code": "...", "message": "..."}`
///（typed 错误码表见模块文档——宿主 `JSON.parse(e)` 即结构化错误）。
#[wasm_bindgen]
pub fn import_docx(bytes: &[u8]) -> Result<String, JsValue> {
    import_docx_core(bytes).map_err(|e| {
        JsValue::from_str(&serde_json::json!({ "code": e.code, "message": e.message }).to_string())
    })
}

// ---------------------------------------------------------------------------
// 样例文档（单一真源：examples/gen_docx_fixture.rs 生成 web e2e 夹具
// + 本模块测试消费——office-io 写面即夹具产线，零外部工具依赖）
// ---------------------------------------------------------------------------

/// 经典 1×1 透明 PNG（70 字节）——图片清单计数的 body_uses 面。
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// 样例 DOCX 字节（夹具产线单一真源）：标题 + 富文本段（加粗/斜体/
/// 外链）+ 两列表 + 图片 + 页眉 + 页脚。消费方 = 本模块测试 +
/// examples/gen_docx_fixture.rs（web e2e 夹具生成器）。
/// `#[doc(hidden)]`：非产品面（wasm 导出面无此项——无
/// #[wasm_bindgen]），仅为避免产线双份漂移而公开。
#[doc(hidden)]
pub fn build_sample_docx() -> Vec<u8> {
    use mdc_office_io::docx::{DocxWriter, RunSpec};
    let mut w = DocxWriter::new();
    w.heading(1, "Imported Title").expect("heading");
    w.paragraph(&[
        RunSpec::text("plain "),
        RunSpec::styled("bold", true, false),
        RunSpec::text(" and "),
        RunSpec::styled("italic", false, true),
        RunSpec::text(" and "),
        RunSpec::link("docs", "https://example.com/docs"),
    ]);
    w.table(&[
        vec!["Name".to_string(), "Value".to_string()],
        vec!["alpha".to_string(), "1".to_string()],
    ]);
    w.image(PNG_1X1, 1, 1).expect("image");
    w.header(&[RunSpec::text("Header Slot")]);
    w.footer(&[RunSpec::text("Footer Slot")]);
    w.save_to_vec().expect("save")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_office_io::docx::{HeaderFooterData, HeaderFooterVariant, ParagraphData};

    fn para(style: Option<&str>, runs: Vec<RunData>) -> Block {
        Block::Paragraph(ParagraphData {
            style: style.map(str::to_string),
            runs,
        })
    }

    fn run(text: &str) -> RunData {
        RunData {
            text: text.to_string(),
            bold: false,
            italic: false,
            hyperlink: None,
        }
    }

    fn style_run(text: &str, bold: bool, italic: bool) -> RunData {
        RunData {
            bold,
            italic,
            ..run(text)
        }
    }

    fn doc_of(blocks: Vec<Block>) -> DocxDocument {
        DocxDocument {
            blocks,
            headers: Vec::new(),
            footers: Vec::new(),
            images: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn import_json(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_str(&import_docx_core(bytes).expect("import ok")).expect("payload json")
    }

    #[test]
    fn writer_roundtrip_import_maps_structure() {
        let payload = import_json(&build_sample_docx());
        let md = payload["markdown"].as_str().expect("markdown");
        assert_eq!(
            md,
            "# Imported Title\n\
             \n\
             plain **bold** and *italic* and [docs](https://example.com/docs)\n\
             \n\
             | Name | Value |\n\
             | --- | --- |\n\
             | alpha | 1 |"
        );
        let stats = &payload["stats"];
        assert_eq!(stats["blocks"], 3);
        assert_eq!(stats["headings"], 1);
        assert_eq!(stats["paragraphs"], 1); // 富文本段（非标题）
        assert_eq!(stats["tables"], 1);
        assert_eq!(stats["hyperlinks"], 1);
        assert_eq!(stats["images_skipped"], 1);
        assert_eq!(stats["headers_skipped"], 1);
        assert_eq!(stats["footers_skipped"], 1);
        assert_eq!(stats["warnings"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn heading_styles_map_with_clamp() {
        let doc = doc_of(vec![
            para(Some("Heading1"), vec![run("一")]),
            para(Some("Heading3"), vec![run("三")]),
            para(Some("Heading9"), vec![run("九")]),
            para(Some("Title"), vec![run("题")]),
            para(Some("Normal"), vec![run("常")]),
            para(None, vec![run("无")]),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        let lines: Vec<&str> = md.split("\n\n").collect();
        assert_eq!(lines[0], "# 一");
        assert_eq!(lines[1], "### 三");
        // Heading7-9 钳 6（MD 上限 h6——mdcore heading_of 1-6）
        assert_eq!(lines[2], "###### 九");
        assert_eq!(lines[3], "题");
        assert_eq!(lines[4], "常");
        assert_eq!(lines[5], "无");
        assert_eq!(stats["headings"], 3);
        assert_eq!(stats["paragraphs"], 3);
    }

    #[test]
    fn adjacent_same_format_runs_merge() {
        let doc = doc_of(vec![para(
            None,
            vec![
                style_run("a", true, false),
                style_run("b", true, false),
                style_run("c", false, false),
                style_run("  ", true, true),
            ],
        )]);
        let (md, _) = docx_to_markdown(&doc);
        // 合并消除 `**a**` + `**b**` 直连歧义；纯空白 run 原样直出无标记。
        assert_eq!(md, "**ab**c  ");
    }

    #[test]
    fn link_variants_render() {
        let doc = doc_of(vec![
            para(
                None,
                vec![RunData {
                    hyperlink: Some(HyperlinkTarget::External {
                        target: "https://ex.com/a b".into(),
                    }),
                    ..run("ext")
                }],
            ),
            para(
                None,
                vec![RunData {
                    hyperlink: Some(HyperlinkTarget::Anchor {
                        anchor: "toc".into(),
                    }),
                    ..run("anc")
                }],
            ),
            para(
                None,
                vec![RunData {
                    hyperlink: Some(HyperlinkTarget::Unresolved { rid: "rId9".into() }),
                    ..style_run("unres", true, false)
                }],
            ),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        let lines: Vec<&str> = md.split("\n\n").collect();
        assert_eq!(lines[0], "[ext](<https://ex.com/a b>)");
        assert_eq!(lines[1], "[anc](#toc)");
        // Unresolved：目标不可用 → 降级纯文本（强调保留），不计链接数。
        assert_eq!(lines[2], "**unres**");
        assert_eq!(stats["hyperlinks"], 2);
    }

    #[test]
    fn table_cell_sanitization_and_padding() {
        let doc = doc_of(vec![Block::Table(TableData {
            rows: vec![
                vec!["a|b".to_string(), "x".to_string()],
                vec!["two\nlines".to_string()],
            ],
        })]);
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(md, "| a\\|b | x |\n| --- | --- |\n| two lines |  |");
        assert_eq!(stats["tables"], 1);
        // 空表跳过。
        let (md2, stats2) =
            docx_to_markdown(&doc_of(vec![Block::Table(TableData { rows: vec![] })]));
        assert_eq!(md2, "");
        assert_eq!(stats2["tables"], 0);
    }

    #[test]
    fn skip_counts_cover_headers_footers_images() {
        let doc = DocxDocument {
            blocks: vec![para(None, vec![run("body")])],
            headers: vec![HeaderFooterData {
                variant: HeaderFooterVariant::Default,
                blocks: vec![para(None, vec![run("h")])],
            }],
            footers: vec![],
            images: vec![
                mdc_office_io::docx::ImageAsset {
                    part_name: "word/media/a.png".into(),
                    content_type: Some("image/png".into()),
                    bytes: vec![1],
                    references: vec![],
                    body_uses: 2,
                },
                mdc_office_io::docx::ImageAsset {
                    part_name: "word/media/orphan.png".into(),
                    content_type: Some("image/png".into()),
                    bytes: vec![1],
                    references: vec![],
                    body_uses: 0,
                },
            ],
            warnings: vec![mdc_office_io::io::CompatibilityWarning {
                part: "word/document.xml".into(),
                kind: mdc_office_io::io::WarningKind::BrokenReference,
                detail: "dangling r:id rId7".into(),
            }],
        };
        let (_, stats) = docx_to_markdown(&doc);
        assert_eq!(stats["headers_skipped"], 1);
        assert_eq!(stats["footers_skipped"], 0);
        // 孤儿媒体（body_uses=0）不计——非用户可见内容。
        assert_eq!(stats["images_skipped"], 1);
        assert_eq!(stats["warnings"][0]["kind"], "broken_reference");
    }

    #[test]
    fn garbage_bytes_typed_container_error() {
        for garbage in [
            vec![],
            vec![0u8, 1, 2, 99, 255],
            b"not a zip at all".to_vec(),
        ] {
            let err = import_docx_core(&garbage).expect_err("garbage must reject");
            assert_eq!(err.code, "container_corrupt", "bytes={garbage:?}");
            assert!(!err.message.is_empty());
        }
    }

    #[test]
    fn truncated_docx_typed_container_error() {
        let full = build_sample_docx();
        let cut = &full[..full.len() / 2];
        let err = import_docx_core(cut).expect_err("truncated zip must reject");
        assert_eq!(err.code, "container_corrupt");
    }

    /// 夹具漂移守卫：web e2e 夹具（examples/gen_docx_fixture.rs 产线，
    /// 提交于 web/tests/fixtures/import_sample.docx）必须仍产出期望
    /// markdown——夹具损坏/漂移在原生层先红，带清晰定位信息。
    #[test]
    fn web_e2e_fixture_imports_expected_markdown() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/tests/fixtures/import_sample.docx"
        );
        let bytes = std::fs::read(path)
            .unwrap_or_else(|e| panic!("read fixture {path}: {e} — regenerate: cargo run -p mdcore-wasm --example gen_docx_fixture -- ../../web/tests/fixtures/import_sample.docx"));
        let payload = import_json(&bytes);
        assert!(
            payload["markdown"]
                .as_str()
                .expect("markdown")
                .contains("# Imported Title"),
            "fixture markdown drifted: {}",
            payload["markdown"]
        );
        assert_eq!(payload["stats"]["tables"], 1);
        assert_eq!(payload["stats"]["images_skipped"], 1);
    }

    /// r4-t2：错误面完备性——`DocxError::Limit`（写面资源限，读路径
    /// 不可达——office-io `MAX_PARAGRAPHS`/`MAX_RUNS`）在导入面映射为
    /// `docx_limit`（与写导出面同码；完备性映射，模块文档错误码表）。
    /// 读面既有码不动：Container(PrescanRejected) → container_prescan。
    #[test]
    fn limit_variant_maps_to_docx_limit_code() {
        let e = map_error(&mdc_office_io::docx::DocxError::Limit(
            "run count over limit 4000000".into(),
        ));
        assert_eq!(e.code, "docx_limit");
        assert!(
            e.message.starts_with("docx writer resource limit"),
            "{:?}",
            e.message
        );
        let prescan = map_error(&mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::PrescanRejected {
                reason: "input size over limit",
                entry: None,
            },
        ));
        assert_eq!(prescan.code, "container_prescan", "读面既有码零漂移");
    }
}
