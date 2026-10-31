//! docx 只读文档模型（#68 阶段 1 批 3——mdc-office-io path 依赖接入）。
//!
//! 职责：把 [`mdc_office_io::docx`] 的块序模型转换为编辑器可渲染的
//! **行模型**（纯文本行 + 逐行样式 span + 标题树 + 文档属性）——
//! 渲染/滚动/查找/选区/光标全复用批 1-2 的 Editor 管线，仅以
//! 只读门禁（[`readonly_allows`]）关闭变更类命令。
//!
//! ## 转换规则（docx 块 → 行）
//!
//! - 段落：文本 = run 顺序拼接；标题段（`heading_level` 1-9）整段
//!   Heading 样式 + 记入标题树；列表段按 numId+ilvl 维持有序计数
//!   （`N. `）/ 无序 `• `，层级以缩进表达；`w:ind` 左缩进按 360
//!   twips ≈ 1 空格近似（≤16 空格封顶）；run 级 bold/italic/link →
//!   Bold/Italic/Link span（超链接内文本不入 bold/italic——链接样式
//!   优先；bold+italic 叠加取 Bold，斜体面降级登记）。
//! - 表格：栅格已由读面展开（合并格 = 空串），列宽按 East Asian
//!   宽度对齐（CJK 2 cell），行内 ` | ` 连接；首行整行 Bold + 下缀
//!   分隔线；内嵌表平铺为独立表块（无边框定位）。
//! - 文本框：内层块平铺渲染（无边框/锚定定位）。
//! - 页眉/页脚/图片/批注/脚注/尾注：**不入正文行流**——计数入
//!   [`DocxProps`]（属性面板呈现）；图片位置模型面外（office-io
//!   登记面）。
//!
//! ## 已知边界（批 3 登记，后续批补）
//!
//! - 表格列宽上限 28 cell（超宽截断单元格文本 + `…`）
//! - 有序列表计数不重置（OOXML 层级重启语义未实现——跨段重启按
//!   同 numId 续计）
//! - 软换行（`w:br`）由读面并入同段文本，本层不再拆行
//! - 分页语义面外（docx 无显式页边界入流——阶段 3 页码/行号按行号
//!   呈现）

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use mdc_office_io::docx::{self, Block, ListKind, read_docx_from_bytes_with_options};
use mdc_office_io::io;

use super::highlight::TokenKind;

/// 表格单元格列宽上限（East Asian cell 计，超宽截断 + `…`）。
const CELL_WIDTH_CAP: usize = 28;
/// 左缩进换算：360 twips ≈ 1 空格（0.25 英寸），封顶 16 空格。
const INDENT_UNIT_TWIPS: i32 = 360;
const INDENT_SPACES_CAP: usize = 16;
/// 列表层级缩进（每级 2 空格，封顶 4 级）。
const LIST_INDENT_PER_LEVEL: usize = 2;
const LIST_LEVEL_CAP: usize = 4;

/// 标题树条目（左栏「文档结构」——阶段 3.1 docx 面提前落地）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadingEntry {
    /// 标题层级（1-9，读面钳位后域）。
    pub level: usize,
    /// 标题文本（run 拼接）。
    pub text: String,
    /// 转换后行模型的 0 基行号（点击跳转锚）。
    pub line: usize,
}

/// 文档属性（右栏属性面板 + 启动日志证据通道）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxProps {
    pub blocks: usize,
    pub paragraphs: usize,
    pub tables: usize,
    pub table_rows: usize,
    pub textboxes: usize,
    pub images: usize,
    pub headers: usize,
    pub footers: usize,
    pub comments: usize,
    pub footnotes: usize,
    pub endnotes: usize,
    pub warnings: usize,
    /// 空段落计数（诊断面——docx 常见空行布局）。
    pub empty_paragraphs: usize,
}

/// docx 转换产物（Editor 只读渲染的全部数据面）。
#[derive(Clone, Debug, Default)]
pub struct DocxPayload {
    /// 全文纯文本（行以 `\n` 连接——喂 [`super::buffer::Buffer`]）。
    pub text: String,
    /// 逐行样式 span（char 索引区间 + token 类别；与 text 行严格对齐）。
    pub spans: Vec<Vec<(Range<usize>, TokenKind)>>,
    /// 标题树（文档序）。
    pub headings: Vec<HeadingEntry>,
    /// 文档属性。
    pub props: DocxProps,
}

/// 读盘 + 解析 + 转换（单入口——`document::open` 的 docx 分派目标）。
///
/// 兼容模式（`compat: true`）：畸形部件降级为结构化警告（数据照常
/// 提取，警告计数入属性）——查看器语义；安全门（预扫/尺寸/深度上限）
/// 两模式恒等不放松。输入尺寸前置（先 `fs::metadata` 再读——避免
/// 大文件全量入内存，同 office-io path 入口语义）。
pub fn load(path: &Path) -> Result<DocxPayload, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件信息失败：{e}"))?;
    if meta.len() > io::LIMIT_INPUT_BYTES_DOCX {
        return Err(format!(
            "文件超过 docx 输入上限（{} 字节 > {} 字节）",
            meta.len(),
            io::LIMIT_INPUT_BYTES_DOCX
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取失败：{e}"))?;
    let opts = io::ReadOptions { compat: true };
    let doc = read_docx_from_bytes_with_options(&bytes, &opts)
        .map_err(|e| format!("docx 解析失败：{e}"))?;
    Ok(convert(&doc))
}

/// 块序模型 → 行模型（纯函数——单测锚点）。
pub fn convert(doc: &docx::DocxDocument) -> DocxPayload {
    let mut out = DocxPayload {
        props: DocxProps {
            blocks: doc.blocks.len(),
            images: doc.images.len(),
            headers: doc.headers.len(),
            footers: doc.footers.len(),
            comments: doc.comments.len(),
            footnotes: doc.footnotes.len(),
            endnotes: doc.endnotes.len(),
            warnings: doc.warnings.len(),
            ..DocxProps::default()
        },
        ..DocxPayload::default()
    };
    let mut ctx = ConvertCtx::default();
    for block in &doc.blocks {
        match block {
            Block::Paragraph(p) => ctx.push_paragraph(p, &mut out),
            Block::Table(t) => ctx.push_table(t, &mut out),
            Block::TextBox(tb) => {
                out.props.textboxes += 1;
                // 内层块平铺（无边框/锚定——登记边界）。
                for inner in &tb.blocks {
                    match inner {
                        Block::Paragraph(p) => ctx.push_paragraph(p, &mut out),
                        Block::Table(t) => ctx.push_table(t, &mut out),
                        Block::TextBox(nested) => {
                            for deep in &nested.blocks {
                                if let Block::Paragraph(p) = deep {
                                    ctx.push_paragraph(p, &mut out);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out.text = ctx.lines.join("\n");
    out.spans = ctx.spans;
    out
}

/// 转换累积态。
#[derive(Default)]
struct ConvertCtx {
    lines: Vec<String>,
    spans: Vec<Vec<(Range<usize>, TokenKind)>>,
    /// 有序列表计数（numId+ilvl → 当前序号——跨段连续，不重置）。
    ordered: HashMap<(usize, usize), usize>,
}

impl ConvertCtx {
    fn push_paragraph(&mut self, p: &docx::ParagraphData, out: &mut DocxPayload) {
        out.props.paragraphs += 1;
        let mut line = String::new();
        let mut spans: Vec<(Range<usize>, TokenKind)> = Vec::new();

        // 左缩进（twips → 空格近似）。
        if let Some(ind) = p.indent {
            let twips = ind.left.unwrap_or(0).max(0) as usize;
            let spaces = (twips / INDENT_UNIT_TWIPS as usize).min(INDENT_SPACES_CAP);
            for _ in 0..spaces {
                line.push(' ');
            }
        }

        // 列表标记（层级缩进 + 序号/圆点 + ListMarker span）。
        if let Some(num) = &p.numbering {
            let lvl = num.ilvl.min(LIST_LEVEL_CAP);
            for _ in 0..lvl * LIST_INDENT_PER_LEVEL {
                line.push(' ');
            }
            let start = line.chars().count();
            match num.kind {
                Some(ListKind::Bullet) => line.push_str("• "),
                Some(ListKind::Ordered) => {
                    let c = self.ordered.entry((num.num_id, num.ilvl)).or_insert(0);
                    *c += 1;
                    line.push_str(&format!("{}. ", c));
                }
                // 解析链不可达（num 桥悬空/抽象层级缺定义）——中性标记。
                None => line.push_str("· "),
            }
            let end = line.chars().count();
            spans.push((start..end, TokenKind::ListMarker));
        }

        // 标题：整段 Heading span（标题行内 run 级样式不再叠加——
        // 样式统一由 Heading 呈现）。
        if let Some(level) = p.heading_level {
            let text_start = line.chars().count();
            for run in &p.runs {
                line.push_str(&run.text);
            }
            let text_end = line.chars().count();
            if text_end > text_start {
                spans.push((text_start..text_end, TokenKind::Heading));
            }
            out.headings.push(HeadingEntry {
                level,
                text: p.text(),
                line: self.lines.len(),
            });
        } else {
            // run 级样式（链接优先于 bold/italic；bold+italic 取 Bold）。
            for run in &p.runs {
                let start = line.chars().count();
                line.push_str(&run.text);
                let end = line.chars().count();
                if end == start {
                    continue;
                }
                let kind = if run.hyperlink.is_some() {
                    Some(TokenKind::Link)
                } else if run.bold {
                    Some(TokenKind::Bold)
                } else if run.italic {
                    Some(TokenKind::Italic)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    spans.push((start..end, kind));
                }
            }
        }

        if line.is_empty() {
            out.props.empty_paragraphs += 1;
        }
        self.lines.push(line);
        self.spans.push(spans);
    }

    fn push_table(&mut self, t: &docx::TableData, out: &mut DocxPayload) {
        out.props.tables += 1;
        out.props.table_rows += t.rows.len();
        if t.rows.is_empty() {
            return;
        }
        // 列数 = 最宽行；列宽 = 列内最大 East Asian 宽度（截断封顶）。
        let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0usize; cols];
        for row in &t.rows {
            for (c, cell) in row.iter().enumerate() {
                let w = display_width(&truncate_cell(cell)).min(CELL_WIDTH_CAP);
                widths[c] = widths[c].max(w);
            }
        }
        // 首行 = 表头（Bold 整行 + 分隔线）；其余行平铺。
        for (ri, row) in t.rows.iter().enumerate() {
            if ri == 1 {
                // 表头分隔线（列宽对齐的虚线）。
                let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
                let sep = sep.join("-+-");
                self.lines.push(sep);
                self.spans.push(Vec::new());
            }
            let cells: Vec<String> = (0..cols)
                .map(|c| {
                    let raw = row.get(c).map(String::as_str).unwrap_or("");
                    pad_cell(&truncate_cell(raw), widths[c])
                })
                .collect();
            let line = cells.join(" | ");
            let spans = if ri == 0 && !line.is_empty() {
                vec![(0..line.chars().count(), TokenKind::Bold)]
            } else {
                Vec::new()
            };
            self.lines.push(line);
            self.spans.push(spans);
        }
        // 表尾空行（块间呼吸——与 md 渲染节奏一致）。
        self.lines.push(String::new());
        self.spans.push(Vec::new());
    }
}

/// East Asian 显示宽度（CJK/全角 = 2 cell，其余 1——Sarasa Mono SC
/// 等宽口径的近似；组合字符面外）。
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            let cjk = (0x1100..=0x115F).contains(&cp) // Hangul Jamo
                || (0x2E80..=0xA4CF).contains(&cp)   // CJK 部首..Yi
                || (0xAC00..=0xD7A3).contains(&cp)   // Hangul 音节
                || (0xF900..=0xFAFF).contains(&cp)   // CJK 兼容表意
                || (0xFE30..=0xFE4F).contains(&cp)   // CJK 兼容形式
                || (0xFF00..=0xFF60).contains(&cp)   // 全角形式
                || (0xFFE0..=0xFFE6).contains(&cp)   // 全角符号
                || (0x20000..=0x3FFFD).contains(&cp); // CJK 扩展
            if cjk { 2 } else { 1 }
        })
        .sum()
}

/// 超宽单元格截断（`…` 收尾——1 cell）。
fn truncate_cell(s: &str) -> String {
    if display_width(s) <= CELL_WIDTH_CAP {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = display_width(&c.to_string());
        if w + cw > CELL_WIDTH_CAP - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// 列宽补齐（East Asian 宽度对齐——空格右补）。
fn pad_cell(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(display_width(s));
    let mut out = s.to_string();
    for _ in 0..pad {
        out.push(' ');
    }
    out
}

/// 只读键门禁（纯函数——单测锚点）：浏览类命令放行，变更类整段拒绝。
///
/// 放行：移动（含 Shift 选区扩展 / Ctrl 词跳）/ Home / End / PageUp /
/// PageDown / Esc 清选区 / Ctrl+A 全选 / Ctrl+C 复制 / Ctrl+F 查找。
/// 拒绝：输入 / 删除 / 回车 / Tab / Ctrl+X / Ctrl+V / Ctrl+Z / Ctrl+Y /
/// Ctrl+S（可打印字符经 `key_char` 兜底分支——非放行键一律拒绝）。
pub fn readonly_allows(key: &str, ctrl: bool) -> bool {
    match key {
        "left" | "right" | "up" | "down" | "home" | "end" | "pageup" | "pagedown" | "escape" => {
            true
        }
        "a" | "c" | "f" => ctrl,
        // F3：Ctrl+Tab 标签循环是导航组合非编辑命令——只读态放行
        //（冒泡至 shell）；无 Ctrl 的 Tab 是缩进编辑——拒绝如故。
        "tab" => ctrl,
        _ => false,
    }
}

// =============================================================================
// 单元测试（DocxWriter 写面内存生成 → 读面解析 → 转换断言——零二进制
// 夹具依赖；烟雾 GUI 夹具由 `gen_smoke_docx`（--ignored）落盘生成）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_office_io::docx::{DocxWriter, RunSpec};

    /// 内存往返：写 → 读（兼容模式）。
    fn roundtrip(w: &mut DocxWriter) -> docx::DocxDocument {
        let bytes = w.save_to_vec().expect("写盘内存");
        let opts = io::ReadOptions { compat: true };
        read_docx_from_bytes_with_options(&bytes, &opts).expect("读回")
    }

    #[test]
    fn heading_tree_and_props() {
        let mut w = DocxWriter::new();
        w.heading(1, "项目概述").unwrap();
        w.paragraph_text("本项目承建数据中心机房。");
        w.heading(2, "技术方案").unwrap();
        w.paragraph_text("冷热通道隔离设计。");
        w.heading(3, "详细设计").unwrap();
        w.paragraph_text("机柜布局见下表。");
        let doc = roundtrip(&mut w);
        let out = convert(&doc);

        // 标题树：层级 + 行号（标题行在正文行前交错）。
        assert_eq!(out.headings.len(), 3);
        assert_eq!(
            out.headings[0],
            HeadingEntry {
                level: 1,
                text: "项目概述".into(),
                line: 0
            }
        );
        assert_eq!(
            out.headings[1],
            HeadingEntry {
                level: 2,
                text: "技术方案".into(),
                line: 2
            }
        );
        assert_eq!(
            out.headings[2],
            HeadingEntry {
                level: 3,
                text: "详细设计".into(),
                line: 4
            }
        );

        // 行模型：标题与正文交错。
        assert_eq!(
            out.text,
            "项目概述\n本项目承建数据中心机房。\n技术方案\n冷热通道隔离设计。\n详细设计\n机柜布局见下表。"
        );

        // Heading span：标题行整段。
        assert_eq!(out.spans[0], vec![(0..4, TokenKind::Heading)]);

        // 属性。
        assert_eq!(out.props.blocks, 6);
        assert_eq!(out.props.paragraphs, 6);
        assert_eq!(out.props.tables, 0);
        assert_eq!(out.props.warnings, 0);
    }

    #[test]
    fn run_styles_bold_italic_link() {
        let mut w = DocxWriter::new();
        w.paragraph(&[
            RunSpec::text("普通"),
            RunSpec::styled("加粗", true, false),
            RunSpec::styled("斜体", false, true),
            RunSpec::link("官网", "https://example.com"),
        ]);
        let doc = roundtrip(&mut w);
        let out = convert(&doc);

        assert_eq!(out.text, "普通加粗斜体官网");
        let span_of = |i: usize| out.spans[0][i].clone();
        // 加粗 2..4 / 斜体 4..6 / 链接 6..8（char 索引）。
        assert_eq!(span_of(0), (2..4, TokenKind::Bold));
        assert_eq!(span_of(1), (4..6, TokenKind::Italic));
        assert_eq!(span_of(2), (6..8, TokenKind::Link));
    }

    #[test]
    fn list_ordered_bullet_and_indent() {
        let mut w = DocxWriter::new();
        w.paragraph_text("清单：");
        w.list_item(&[RunSpec::text("需求调研")], ListKind::Ordered, 0)
            .unwrap();
        w.list_item(&[RunSpec::text("方案设计")], ListKind::Ordered, 0)
            .unwrap();
        w.list_item(&[RunSpec::text("现场勘测")], ListKind::Bullet, 0)
            .unwrap();
        let doc = roundtrip(&mut w);
        let out = convert(&doc);

        let lines: Vec<&str> = out.text.split('\n').collect();
        assert_eq!(lines[1], "1. 需求调研");
        assert_eq!(lines[2], "2. 方案设计");
        assert_eq!(lines[3], "• 现场勘测");
        // ListMarker span 覆盖 "1. "。
        assert_eq!(out.spans[1][0], (0..3, TokenKind::ListMarker));
    }

    #[test]
    fn table_aligned_columns_and_header_bold() {
        let mut w = DocxWriter::new();
        w.table(&[
            vec!["项目".into(), "数量".into(), "单价".into()],
            vec!["机柜".into(), "42".into(), "18000".into()],
            vec!["UPS".into(), "2".into(), "350000".into()],
        ]);
        let doc = roundtrip(&mut w);
        let out = convert(&doc);

        let lines: Vec<&str> = out.text.split('\n').collect();
        // 表头 + 分隔线 + 2 数据行 + 尾空行。
        assert_eq!(lines.len(), 5, "行数：{lines:?}");
        // 表头单元格同样补齐：单价(4) 补至 col2 宽 6（尾 2 空格）。
        assert_eq!(lines[0], "项目 | 数量 | 单价  ");
        // 列宽：col0=max(项目4,机柜4,UPS3)=4 / col1=max(数量4,42·2,2·1)=4 /
        // col2=max(单价4,18000·5,350000·6)=6。分隔线 join("-+-") 的首尾
        // dash 与列 dash 邻接合并：4+1 / 4+1+1 / 6+1 = 5/6/7——「+」
        // 恰落表头「|」列位（cell 5/12，对齐口径一致）。
        assert_eq!(lines[1], "-----+------+-------");
        // CJK 对齐：机柜(4) 免补；42 补至 4；18000 补至 6（尾空格）。
        assert_eq!(lines[2], "机柜 | 42   | 18000 ");
        assert_eq!(lines[3], "UPS  | 2    | 350000");
        // 表头整行 Bold（14 char——含补齐空格）。
        assert_eq!(out.spans[0], vec![(0..14, TokenKind::Bold)]);
        assert_eq!(out.props.tables, 1);
        assert_eq!(out.props.table_rows, 3);
    }

    #[test]
    fn display_width_cjk_vs_ascii() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("中a"), 3);
        assert_eq!(display_width("１２"), 4, "全角数字 2 cell");
    }

    #[test]
    fn truncate_cell_caps_width() {
        // 25 char × 2 cell = 50 > 28 → 截断至 13 char（26 cell）+ …（27 ≤ 28）。
        let s = "这是一个非常非常长的单元格内容肯定需要被截断处理掉";
        assert_eq!(display_width(s), 50);
        let t = truncate_cell(s);
        assert!(
            display_width(&t) <= CELL_WIDTH_CAP,
            "截断后宽度：{}",
            display_width(&t)
        );
        assert!(t.ends_with('…'));
        assert_eq!(display_width(&t), 27);
        // 短串免截断。
        assert_eq!(truncate_cell("短串"), "短串");
    }

    #[test]
    fn readonly_gate_browsing_passes_mutations_blocked() {
        // 浏览面放行。
        for key in [
            "left", "right", "up", "down", "home", "end", "pageup", "pagedown", "escape",
        ] {
            assert!(readonly_allows(key, false), "{key} 应放行");
        }
        assert!(readonly_allows("a", true), "Ctrl+A 全选放行");
        assert!(readonly_allows("c", true), "Ctrl+C 复制放行");
        assert!(readonly_allows("f", true), "Ctrl+F 查找放行");
        assert!(
            readonly_allows("tab", true),
            "F3：Ctrl+Tab 标签循环导航放行"
        );
        // 变更面拒绝。
        assert!(!readonly_allows("enter", false));
        assert!(!readonly_allows("backspace", false));
        assert!(!readonly_allows("delete", false));
        assert!(!readonly_allows("tab", false));
        assert!(!readonly_allows("z", true), "Ctrl+Z 撤销拒绝");
        assert!(!readonly_allows("y", true), "Ctrl+Y 重做拒绝");
        assert!(!readonly_allows("x", true), "Ctrl+X 剪切拒绝");
        assert!(!readonly_allows("v", true), "Ctrl+V 粘贴拒绝");
        assert!(!readonly_allows("s", true), "Ctrl+S 保存拒绝");
        assert!(!readonly_allows("s", false), "可打印字符插入拒绝");
        assert!(!readonly_allows("f", false), "无 Ctrl 的 f 是输入——拒绝");
    }

    #[test]
    fn textbox_blocks_flatten() {
        let mut w = DocxWriter::new();
        w.paragraph_text("正文段");
        // 文本框：write 面经 bookmark/特殊路径？——直接构造块序代价高，
        // 读面 TextBox 由 w:txbxContent 触发；write 面无直接入口。
        // 以属性计数断言为主（textboxes=0 时平铺分支不触发——覆盖由
        // poi 语料烟雾承担）。
        let doc = roundtrip(&mut w);
        let out = convert(&doc);
        assert_eq!(out.props.textboxes, 0);
        assert_eq!(out.text, "正文段");
    }

    #[test]
    fn empty_paragraphs_counted() {
        let mut w = DocxWriter::new();
        w.paragraph_text("段一");
        w.paragraph_text("");
        w.paragraph_text("段二");
        let doc = roundtrip(&mut w);
        let out = convert(&doc);
        assert_eq!(out.props.empty_paragraphs, 1);
        assert_eq!(out.text, "段一\n\n段二");
    }

    /// 烟雾 GUI 夹具生成器（不随常规测试跑）：
    ///
    /// ```bash
    /// cargo test gen_smoke_docx -- --ignored
    /// ./smoke.sh 5 tests/fixtures/smoke.docx
    /// ```
    #[test]
    #[ignore = "烟雾夹具生成器（写 tests/fixtures/smoke.docx）"]
    fn gen_smoke_docx() {
        let mut w = DocxWriter::new();
        w.heading(1, "智慧园区综合管理平台投标文件").unwrap();
        w.paragraph_text("本文件为技术标书演示样张（mang-shell 批 3 只读渲染烟雾夹具）。");
        w.heading(2, "一、项目概述").unwrap();
        w.paragraph(&[
            RunSpec::text("本项目承建 "),
            RunSpec::styled("智慧园区综合管理平台", true, false),
            RunSpec::text("，含数据中心机房、安防与物联网接入三大部分。"),
        ]);
        w.heading(2, "二、技术方案").unwrap();
        w.paragraph_text("平台采用云原生微服务架构，核心组件如下：");
        w.list_item(
            &[RunSpec::text("接入网关集群（双活）")],
            ListKind::Ordered,
            0,
        )
        .unwrap();
        w.list_item(
            &[RunSpec::text("消息中间件（Kafka）")],
            ListKind::Ordered,
            0,
        )
        .unwrap();
        w.list_item(
            &[RunSpec::text("时序数据库（TDengine）")],
            ListKind::Ordered,
            0,
        )
        .unwrap();
        w.list_item(&[RunSpec::text("现场勘测已完成")], ListKind::Bullet, 0)
            .unwrap();
        w.heading(2, "三、报价明细").unwrap();
        w.table(&[
            vec!["设备".into(), "数量".into(), "单价（元）".into()],
            vec!["机柜".into(), "42".into(), "18000".into()],
            vec!["UPS 电源".into(), "2".into(), "350000".into()],
            vec!["核心交换机".into(), "4".into(), "128000".into()],
        ]);
        w.heading(2, "四、实施计划").unwrap();
        w.paragraph(&[
            RunSpec::text("总工期 "),
            RunSpec::styled("180", false, true),
            RunSpec::text(" 个日历天，详见 "),
            RunSpec::link("项目门户", "https://example.com"),
            RunSpec::text("。"),
        ]);
        w.quote_paragraph(
            &[RunSpec::text(
                "注：本样张由 mdc-office-io 写面生成，用于只读渲染验证。",
            )],
            1,
        );
        let bytes = w.save_to_vec().expect("写盘内存");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/smoke.docx");
        std::fs::write(path, &bytes).expect("落盘夹具");
        // 落盘件自校验：读回 + 转换可过。
        let payload = load(std::path::Path::new(path)).expect("夹具读回");
        assert_eq!(payload.headings.len(), 5, "五级标题树");
        assert_eq!(payload.props.tables, 1);
        eprintln!(
            "docx 夹具已生成：{path}（{} 字节，{} 行，{} 标题）",
            bytes.len(),
            payload.text.split('\n').count(),
            payload.headings.len()
        );
    }
}
