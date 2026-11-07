//! pdf 只读文档模型（#68 阶段 1 批 4——mdc-pdf path 依赖接入）。
//!
//! 职责：把 [`mdc_pdf`] 的**文本层**（pdfium `DocumentEngine::extract_text`）
//! 与书签树（`outline`）转换为编辑器可渲染的**行模型**——渲染/滚动/
//! 查找/选区/光标全复用批 1-2 的 Editor 管线，只读门禁复用批 3 的
//! [`docx_doc::readonly_allows`]（单一真源——不在本模块复制第二份）。
//!
//! ## 转换规则（pdf 文本层 → 行模型）
//!
//! - 每页先落一行**页分隔行** `── 第 N / M 页 ──`（整行 [`TokenKind::Quote`]
//!   span——muted 视觉），页正文行随后，页尾补一空行（块间呼吸）；
//! - 页文本行规整：`\r\n`/`\r` 统一为 `\n`、剔除 NUL、行尾空白裁剪；
//! - 页内空行**保留**（折叠连续空行为单行——段间呼吸）；页首/页尾
//!   空行剔除（页边界语义由页分隔行承担）；
//! - 页无文本（扫描页/图形页）→ 落 `（本页无可提取文本——扫描页或
//!   图形页）` 标记行 + `empty_pages` 计数——**明确提示而非白屏**；
//! - 全文档无文本层（`scanned`）→ 行模型首行落全局提示行
//!   （「渲染需要 OCR，当前版本暂不支持」）——扫描件语义显式化；
//! - 书签树（`OutlineTree` 前序文档序）→ 扁平标题树条目
//!   （[`HeadingEntry`]：level=深度 1 基，line=目标页的页分隔行号）。
//!
//! ## 输入上限（双层防线）
//!
//! - 应用层：[`INPUT_CAP_BYTES`]（100MB，Lead 批 4 派单口径）——
//!   `load` 读盘**前**前置拒绝（同 docx_doc 的 LIMIT 前置模式）；
//! - 引擎层：mdc-pdf `Limits` S3-L7 1GiB（`PdfiumEngine::open` 入口
//!   统一执行）——两层独立，应用层更严。
//!
//! ## 打开时安全扫描（S2/S6，#277 / 审计 P1-11 桌面接线）
//!
//! `load_from_bytes` 在引擎打开前跑 mdc-pdf `open_scan`（此前桌面读
//! 路径零扫描命中——web 面有、桌面没有的双标）：
//! - **S2.4 只读告警语义**：S2 危险族命中/S6 表单探针转全局提示行落
//!   行模型首行区（与扫描件提示同通道），**不阻断打开**——本视图
//!   只读、零执行面，用户自行决定去留；
//! - **扫描失败不阻断**：lopdf 与 pdfium 是独立解析器，lopdf 不可解析
//!   不代表 pdfium 打不开——装载判定权在引擎；
//! - **仍加密态诊断前置**：扫描见加密旗 + 引擎打开失败 → 错误面附
//!   加密提示（比泛化引擎错误对用户更可行动）；S5 写拒不在此面
//!   （本视图零写面，写拒在 stamp/forms/writer_lopdf 各自入口）。
//!
//! ## 已知边界（批 4 登记，后续批补）
//!
//! - 表格行检测（同 y 坐标多文本块聚合）未实现——`extract_text` 公开面
//!   为纯文本层（无坐标载荷），表格行按普通文本行呈现（#68 派单提示
//!   项，非验收硬项）；
//! - run 级样式（bold/italic）面外——文本层无样式载荷，一律普通文本；
//! - 页正文行的软换行（同页同段折行）以 pdfium 文本层的行结构为准
//!   （渲染器行序，非排版段序）；
//! - 书签目标页越界（引擎层已丢弃畸形目标；合成输入防御性钳到 0 行）。
//!
//! ## 引擎绑定纪律（mdc-pdf 既有面）
//!
//! libpdfium.so 定位序：env `MDC_PDFIUM_LIB` → 编译期候选清单（产品仓
//! `bin/lib` / spike 取件位 / crate 本地）→ 系统。本模块不重复绑定
//! 逻辑——`load` 首次调用触发进程级单绑定（`PdfiumEngine::open`）。
//! 无 libpdfium 环境下 `cargo test`（默认面）不受影响：纯函数单测零
//! 引擎依赖；引擎相关测试 `#[ignore]`（烟雾通道显式跑），唯
//! [`load_rejects_invalid_bytes`] 例外面（无 libpdfium 时绑定失败 Err、
//! 有 libpdfium 时装载失败 Err——两环境同判）。

use std::ops::Range;
use std::path::Path;

use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::outline::OutlineTarget;
use mdc_pdf::page::{PageIndex, PageSpan};
use mdc_pdf::security::{DangerousFamily, Limits, OpenAssessment};
use mdc_pdf::security_scan::open_scan;

use super::docx_doc::HeadingEntry;
use super::highlight::TokenKind;

/// 应用层输入上限（100MB——Lead 批 4 派单「大文件不卡」前置校验口径）。
/// 引擎层另有 S3-L7 1GiB 双防线（mdc-pdf `Limits`），两层独立。
const INPUT_CAP_BYTES: u64 = 100 * 1024 * 1024;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// pdf 文档属性（右栏属性面板 + 启动日志证据通道）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PdfProps {
    /// 页数。
    pub pages: usize,
    /// 可提取字符数（非空白——页正文非空行累计）。
    pub chars: usize,
    /// 页正文行数（非空行——不含空行/页分隔行/页尾空行/标记行）。
    pub content_lines: usize,
    /// 无文本页计数（扫描页/图形页——部分扫描提示）。
    pub empty_pages: usize,
    /// 书签条目数（扁平文档序）。
    pub bookmarks: usize,
    /// 全文档无文本层（扫描件——全局提示已落行模型首行）。
    pub scanned: bool,
}

/// 单页在行模型中的落位（页码标签与书签跳转的映射基表）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageEntry {
    /// 0 基页号。
    pub index: usize,
    /// 该页页分隔行的 0 基行号（书签跳转锚 + 页码区间起点）。
    pub first_line: usize,
    /// 该页最后一行（含页尾空行）的 0 基行号。
    pub last_line: usize,
}

/// pdf 转换产物（Editor 只读渲染的全部数据面）。
#[derive(Clone, Debug, Default)]
pub struct PdfPayload {
    /// 全文纯文本（行以 `\n` 连接——喂 [`super::buffer::Buffer`]）。
    pub text: String,
    /// 逐行样式 span（char 索引区间 + token 类别；与 text 行严格对齐）。
    pub spans: Vec<Vec<(Range<usize>, TokenKind)>>,
    /// 书签树（扁平文档序；level=深度 1 基）。
    pub headings: Vec<HeadingEntry>,
    /// 页表（文档序）。
    pub pages: Vec<PageEntry>,
    /// 文档属性。
    pub props: PdfProps,
}

/// 引擎抽取中转（`load` 的产物 = `convert` 的纯函数输入——单测锚点）。
#[derive(Clone, Debug, Default)]
pub struct PdfRaw {
    /// 页文本层（文档序；index 为 0 基页号）。
    pub pages: Vec<PdfTextPage>,
    /// 书签树（扁平文档序；page 为 0 基页号）。
    pub outline: Vec<PdfOutlineEntry>,
    /// 打开时安全评估提示行（S2 危险族/S6 表单探针——
    /// `load_from_bytes` 经 `open_scan` 产出、convert 落行模型首行区；
    /// 默认空 = 无评估路径行为不变）。
    pub security_notices: Vec<String>,
}

/// 单页文本层。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfTextPage {
    pub index: usize,
    pub text: String,
}

/// 扁平书签条目（`OutlineTree::iter` 前序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfOutlineEntry {
    /// 深度 1 基（根=1）。
    pub level: usize,
    pub title: String,
    /// 0 基目标页号。
    pub page: usize,
}

// ---------------------------------------------------------------------------
// load（引擎面）——读盘 + 限额前置 + 引擎抽取 + convert
// ---------------------------------------------------------------------------

/// 读盘 + 解析 + 转换（单入口——`document::open` 的 pdf 分派目标）。
///
/// 限额序：应用层 100MB 前置（读盘**前**，`fs::metadata`）→ 引擎层
/// S3-L7/S3-L4（`PdfiumEngine::open` 统一入口）。错误一律 `String`
/// （与 docx_doc::load 同口径——`document::open` 的 OpenError 面）。
pub fn load(path: &Path) -> Result<PdfPayload, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件信息失败：{e}"))?;
    if meta.len() > INPUT_CAP_BYTES {
        return Err(format!(
            "文件超过 pdf 输入上限（{} 字节 > {} 字节）",
            meta.len(),
            INPUT_CAP_BYTES
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取失败：{e}"))?;
    load_from_bytes(bytes)
}

/// 字节入口（夹具读回自校验共用）。
///
/// 门序（#277 / 审计 P1-11 桌面接线）：S2/S6 `open_scan`（只读评估，
/// 失败不阻断——lopdf 与 pdfium 是独立解析器，装载判定权在引擎）→
/// 引擎打开（S3-L7/L4 在引擎入口）→ 评估转提示行落行模型首行区
/// （S2.4 只读告警语义——不阻断打开）。
pub fn load_from_bytes(bytes: Vec<u8>) -> Result<PdfPayload, String> {
    // S2/S6 扫描（只读评估——不阻断打开；失败 = 无评估，引擎判定照旧）。
    let assessment = open_scan(&bytes, &Limits::default()).ok();
    let engine = match PdfiumEngine::open(bytes) {
        Ok(engine) => engine,
        Err(e) => {
            let mut msg = format!("pdf 引擎打开失败（libpdfium 定位/装载）：{e:?}");
            // 仍加密态诊断前置：扫描已见加密旗（口令不可得）——引擎
            // 失败大概率源于加密；诚实对冲：绑定失败也可能独立存在。
            if assessment.as_ref().is_some_and(|a| a.encrypted) {
                msg.push_str(
                    "\n提示：安全扫描显示文档处于加密态（口令不可得）——\
                     加密文档的只读打开需要口令，当前版本不支持。",
                );
            }
            return Err(msg);
        }
    };
    let notices = assessment
        .as_ref()
        .map(security_notice_lines)
        .unwrap_or_default();
    extract_with(&engine, notices)
}

/// S2/S6 打开评估 → 行模型全局提示行（S2.4 只读告警——用户可见
/// 即可见，打开不阻断；桌面视图零执行面，告警语义到此为止）。
fn security_notice_lines(assessment: &OpenAssessment) -> Vec<String> {
    let mut lines = Vec::new();
    if !assessment.warnings.is_empty() {
        // 族名去重呈现（同族多命中一行；细节对象号不进提示行——
        // 面向用户可读性优先，结构化细节保留在 assessment 内）。
        let mut families: Vec<String> = Vec::new();
        for hit in &assessment.warnings {
            let label = family_label(hit.family);
            if !families.contains(&label) {
                families.push(label);
            }
        }
        lines.push(format!(
            "【安全提示】检出 {} 处潜在危险对象（{}）——本视图只读不执行任何动作，请自行确认文件来源可信。",
            assessment.warnings.len(),
            families.join("、")
        ));
    }
    if let Some(probe) = &assessment.form_probe {
        lines.push(format!(
            "【表单提示】文档含交互式表单（{} 个字段）——只读视图仅呈现文本层，不初始化表单交互。",
            probe.field_count
        ));
    }
    lines
}

/// S2 危险族 → 用户可读名（serde 协议名不作展示串——代码语义与
/// 用户文案分离，与 docx_doc 面板文案同口径）。
fn family_label(family: DangerousFamily) -> String {
    match family {
        DangerousFamily::OpenAction => "打开动作 /OpenAction".into(),
        DangerousFamily::AdditionalActions => "附加动作 /AA".into(),
        DangerousFamily::LaunchAction => "外部程序启动 /Launch".into(),
        DangerousFamily::UriAction => "外链动作 /URI".into(),
        DangerousFamily::EmbeddedFiles => "嵌入文件 /EmbeddedFiles".into(),
        DangerousFamily::RichMedia => "富媒体 /RichMedia".into(),
        DangerousFamily::JavaScript => "JavaScript 动作".into(),
        DangerousFamily::NamesJavaScript => "文档级 JavaScript".into(),
    }
}

/// 引擎抽取 + 转换（`PdfiumEngine` 具体类型——不引入 dyn 面）。
fn extract_with(
    engine: &PdfiumEngine,
    security_notices: Vec<String>,
) -> Result<PdfPayload, String> {
    let count = engine.page_count();
    if count == 0 {
        return Err("pdf 无页面（空文档）".into());
    }
    let outline_tree = engine
        .outline()
        .map_err(|e| format!("pdf 大纲读取失败：{e:?}"))?;
    let span = PageSpan::new(PageIndex(0), PageIndex(count - 1))
        .map_err(|_| "页区间构造失败".to_string())?;
    let texts = engine
        .extract_text(span)
        .map_err(|e| format!("pdf 文本层抽取失败：{e:?}"))?;
    let raw = PdfRaw {
        pages: texts
            .into_iter()
            .map(|(idx, text)| PdfTextPage {
                index: idx.0 as usize,
                text,
            })
            .collect(),
        outline: outline_tree
            .iter()
            .map(|(depth, node)| PdfOutlineEntry {
                level: depth + 1,
                title: node.title.clone(),
                page: match node.target {
                    OutlineTarget::Page(p) => p.0 as usize,
                },
            })
            .collect(),
        security_notices,
    };
    Ok(convert(&raw))
}

// ---------------------------------------------------------------------------
// convert（纯函数）——单测锚点
// ---------------------------------------------------------------------------

/// 引擎抽取产物 → 行模型（纯函数——零引擎依赖）。
pub fn convert(raw: &PdfRaw) -> PdfPayload {
    let total = raw.pages.len();
    let mut props = PdfProps {
        pages: total,
        ..PdfProps::default()
    };

    // 全文档无文本层判定（任一页有非空白字符即非扫描件）。
    props.scanned = total > 0 && !raw.pages.iter().any(|p| page_has_text(&p.text));

    let mut ctx = Ctx::default();
    // 安全评估提示行区（行模型最首——先于扫描件提示：安全语义优先于
    // 可用性提示；S2.4 只读告警——可见但不阻断打开）。
    for notice in &raw.security_notices {
        ctx.push(notice.clone(), Some(TokenKind::Quote));
    }
    // 扫描件全局提示行（行模型首行——明确提示而非白屏）。
    if props.scanned {
        let notice = "【提示】全文无文本层（疑似扫描件）——渲染需要 OCR，当前版本暂不支持。";
        ctx.push(notice.to_string(), Some(TokenKind::Quote));
    }

    let mut pages: Vec<PageEntry> = Vec::new();
    for (i, page) in raw.pages.iter().enumerate() {
        // 页分隔行（Quote 整行）。
        let sep = format!("── 第 {} / {} 页 ──", i + 1, total);
        let first_line = ctx.lines.len();
        ctx.push(sep, Some(TokenKind::Quote));

        let body = page_body_lines(&page.text);
        if body.is_empty() {
            props.empty_pages += 1;
            ctx.push(
                "（本页无可提取文本——扫描页或图形页）".to_string(),
                Some(TokenKind::Quote),
            );
        } else {
            for l in body {
                if l.is_empty() {
                    // 页内空行保留（段间呼吸；连续空行已在 body 折叠为单行）。
                    ctx.push(l, None);
                } else {
                    props.chars += l.chars().filter(|c| !c.is_whitespace()).count();
                    props.content_lines += 1;
                    ctx.push(l, None);
                }
            }
        }
        // 页尾空行（块间呼吸——与 docx 表尾空行同款节奏）。
        ctx.push(String::new(), None);
        pages.push(PageEntry {
            index: i,
            first_line,
            last_line: ctx.lines.len() - 1,
        });
    }

    // 书签树 → 扁平条目（line = 目标页的页分隔行号；越界防御钳 0）。
    let headings: Vec<HeadingEntry> = raw
        .outline
        .iter()
        .map(|e| HeadingEntry {
            level: e.level,
            text: e.title.clone(),
            line: pages.get(e.page).map(|p| p.first_line).unwrap_or(0),
        })
        .collect();
    props.bookmarks = headings.len();

    PdfPayload {
        text: ctx.lines.join("\n"),
        spans: ctx.spans,
        headings,
        pages,
        props,
    }
}

/// convert 行/span 双轨累积态（与 docx_doc::ConvertCtx 同构）。
#[derive(Default)]
struct Ctx {
    lines: Vec<String>,
    spans: Vec<Vec<(Range<usize>, TokenKind)>>,
}

impl Ctx {
    /// 单行落账（kind None = 普通文本行）。
    fn push(&mut self, line: String, kind: Option<TokenKind>) {
        let spans = kind.map(|k| {
            let n = line.chars().count();
            vec![(0..n, k)]
        });
        self.lines.push(line);
        self.spans.push(spans.unwrap_or_default());
    }
}

/// 页文本 → 正文行集（规整 + 边缘剔除 + 连续空行折叠）。
///
/// 产出语义：空 `Vec` = 该页无文本（空页标记路径）；非空时首尾恒为
/// 非空行，内部至多连续 1 个空行（段间呼吸）。
fn page_body_lines(text: &str) -> Vec<String> {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let normalized: Vec<String> = unified
        .split('\n')
        .map(|l| {
            let cleaned: String = l.chars().filter(|c| *c != '\0').collect();
            cleaned.trim_end().to_string()
        })
        .collect();
    // 边缘空行剔除。
    let start = normalized.iter().position(|l| !l.is_empty());
    let Some(start) = start else {
        return Vec::new();
    };
    let end = normalized
        .iter()
        .rposition(|l| !l.is_empty())
        .expect("start 存在则 end 存在");
    // 连续空行折叠为单行。
    let mut body: Vec<String> = Vec::new();
    for l in &normalized[start..=end] {
        if l.is_empty() && body.last().is_some_and(|prev: &String| prev.is_empty()) {
            continue;
        }
        body.push(l.clone());
    }
    body
}

/// 页是否有文本（非空白字符存在）。
fn page_has_text(text: &str) -> bool {
    text.chars().any(|c| !c.is_whitespace() && c != '\0')
}

impl PdfPayload {
    /// 行号 → 页号（1 基；状态栏页码与书签标签的映射出口）。
    ///
    /// 行在首页分隔行之前（扫描件全局提示行）→ 首页；行在末页区间
    /// 之后（防御）→ 末页；空页表 → `None`。
    pub fn page_of_line(&self, line: usize) -> Option<usize> {
        if self.pages.is_empty() {
            return None;
        }
        let mut cur = None;
        for p in &self.pages {
            if p.first_line <= line {
                cur = Some(p.index + 1);
            } else {
                break;
            }
        }
        cur.or_else(|| self.pages.first().map(|p| p.index + 1))
    }
}

// =============================================================================
// 单元测试（纯函数面零引擎依赖；引擎面 #[ignore] 烟雾通道显式跑——
// 与 docx_doc::gen_smoke_docx 同款纪律：cargo test 默认绿）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 合成两页文本层（含 \r\n、行尾空白、页内空行——规整锚点）。
    fn raw_two_pages() -> PdfRaw {
        PdfRaw {
            pages: vec![
                PdfTextPage {
                    index: 0,
                    text:
                        "第一章 项目概述\r\n本项目承建数据中心机房。 \r\n\r\n冷热通道隔离设计。\r"
                            .to_string(),
                },
                PdfTextPage {
                    index: 1,
                    text: "第二章 技术方案\n平台采用云原生微服务架构。\n\n现场勘测已完成。"
                        .to_string(),
                },
            ],
            outline: vec![
                PdfOutlineEntry {
                    level: 1,
                    title: "第一章 项目概述".into(),
                    page: 0,
                },
                PdfOutlineEntry {
                    level: 2,
                    title: "机房建设".into(),
                    page: 0,
                },
                PdfOutlineEntry {
                    level: 1,
                    title: "第二章 技术方案".into(),
                    page: 1,
                },
            ],
            security_notices: vec![],
        }
    }

    #[test]
    fn separators_pages_table_and_props() {
        let out = convert(&raw_two_pages());
        let lines: Vec<&str> = out.text.split('\n').collect();

        // 行形态：分隔 / 内容×3（页内空行保留） / 页尾空行 / 次页分隔…
        assert_eq!(lines[0], "── 第 1 / 2 页 ──");
        assert_eq!(lines[1], "第一章 项目概述");
        // \r\n 规整 + 行尾空白裁剪；页内空行保留（连续折叠后单行）。
        assert_eq!(lines[2], "本项目承建数据中心机房。");
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "冷热通道隔离设计。");
        assert_eq!(lines[5], ""); // 页尾空行
        assert_eq!(lines[6], "── 第 2 / 2 页 ──");
        assert_eq!(lines[7], "第二章 技术方案");
        assert_eq!(lines[8], "平台采用云原生微服务架构。");
        assert_eq!(lines[9], ""); // 页内空行（折叠后）
        assert_eq!(lines[10], "现场勘测已完成。");
        assert_eq!(lines[11], ""); // 页尾空行

        // 页表。
        assert_eq!(out.pages.len(), 2);
        assert_eq!(
            out.pages[0],
            PageEntry {
                index: 0,
                first_line: 0,
                last_line: 5
            }
        );
        assert_eq!(
            out.pages[1],
            PageEntry {
                index: 1,
                first_line: 6,
                last_line: 11
            }
        );

        // 分隔行 Quote 整行 span；内容行无 span。
        assert_eq!(
            out.spans[0],
            vec![(0..lines[0].chars().count(), TokenKind::Quote)]
        );
        assert!(out.spans[1].is_empty());

        // 属性：非空内容行 3+3；字符为非空行非空白累计；无空页非扫描。
        assert_eq!(out.props.pages, 2);
        assert_eq!(out.props.content_lines, 6);
        assert_eq!(out.props.empty_pages, 0);
        assert!(!out.props.scanned);
        assert_eq!(
            out.props.chars,
            "第一章 项目概述本项目承建数据中心机房。冷热通道隔离设计。第二章 技术方案平台采用云原生微服务架构。现场勘测已完成。"
                .chars()
                .filter(|c| !c.is_whitespace())
                .count()
        );
    }

    #[test]
    fn bookmarks_map_to_separator_lines() {
        let out = convert(&raw_two_pages());
        assert_eq!(out.props.bookmarks, 3);
        // level 透传；line = 目标页页分隔行号。
        assert_eq!(
            out.headings[0],
            HeadingEntry {
                level: 1,
                text: "第一章 项目概述".into(),
                line: 0
            }
        );
        assert_eq!(
            out.headings[1],
            HeadingEntry {
                level: 2,
                text: "机房建设".into(),
                line: 0
            }
        );
        assert_eq!(
            out.headings[2],
            HeadingEntry {
                level: 1,
                text: "第二章 技术方案".into(),
                line: 6
            }
        );
    }

    #[test]
    fn page_of_line_boundaries() {
        let out = convert(&raw_two_pages());
        assert_eq!(out.page_of_line(0), Some(1), "首页分隔行 → 1");
        assert_eq!(out.page_of_line(5), Some(1), "首页页尾空行 → 1");
        assert_eq!(out.page_of_line(6), Some(2), "次页分隔行 → 2");
        assert_eq!(out.page_of_line(11), Some(2), "次页页尾空行 → 2");
        assert_eq!(out.page_of_line(99), Some(2), "防御越界 → 末页 2");
    }

    #[test]
    fn empty_page_marker_and_partial_scan() {
        let raw = PdfRaw {
            pages: vec![
                PdfTextPage {
                    index: 0,
                    text: "目录".into(),
                },
                PdfTextPage {
                    index: 1,
                    text: "".into(),
                },
                PdfTextPage {
                    index: 2,
                    text: "\n  \n".into(),
                },
            ],
            outline: vec![],
            security_notices: vec![],
        };
        let out = convert(&raw);
        let lines: Vec<&str> = out.text.split('\n').collect();
        // 页 2/3 空文本 → 标记行 ×2；页 1 有文本。
        assert_eq!(out.props.empty_pages, 2);
        assert!(!out.props.scanned, "部分有文本不判扫描件");
        let marker = "（本页无可提取文本——扫描页或图形页）";
        assert!(lines.contains(&marker));
        assert_eq!(lines[0], "── 第 1 / 3 页 ──");
        assert_eq!(lines[1], "目录");
        // 页 2 的分隔行后紧跟标记行。
        assert_eq!(lines[3], "── 第 2 / 3 页 ──");
        assert_eq!(lines[4], marker);
    }

    #[test]
    fn scanned_notice_first_line_and_page_labels() {
        let raw = PdfRaw {
            pages: vec![
                PdfTextPage {
                    index: 0,
                    text: "".into(),
                },
                PdfTextPage {
                    index: 1,
                    text: " \n ".into(),
                },
            ],
            outline: vec![],
            security_notices: vec![],
        };
        let out = convert(&raw);
        assert!(out.props.scanned);
        let lines: Vec<&str> = out.text.split('\n').collect();
        // 首行 = 全局提示（Quote span）。
        assert!(lines[0].starts_with("【提示】全文无文本层"));
        assert_eq!(out.spans[0][0].1, TokenKind::Quote);
        // 页分隔照常（页码标签可用）；空文本页落标记行。
        assert_eq!(lines[1], "── 第 1 / 2 页 ──");
        assert_eq!(lines[2], "（本页无可提取文本——扫描页或图形页）");
        // 页码归属：提示行与页 1 全部行 → 1；页 2 分隔行起 → 2。
        assert_eq!(out.page_of_line(0), Some(1), "提示行归属首页");
        assert_eq!(out.page_of_line(1), Some(1), "页 1 分隔行 → 1");
        assert_eq!(out.page_of_line(3), Some(1), "页 1 页尾空行 → 1");
        assert_eq!(out.page_of_line(4), Some(2), "页 2 分隔行 → 2");
        assert_eq!(out.page_of_line(99), Some(2), "防御越界 → 末页");
    }

    #[test]
    fn nul_and_crlf_normalization() {
        let raw = PdfRaw {
            pages: vec![PdfTextPage {
                index: 0,
                text: "甲\u{0}乙\r\n丙  \r丁\n\n".into(),
            }],
            outline: vec![],
            security_notices: vec![],
        };
        let out = convert(&raw);
        let lines: Vec<&str> = out.text.split('\n').collect();
        // NUL 剔除（首行「甲乙」）；行尾空白裁剪；孤立 \r → \n；
        // 尾部空行剔除（页边界语义由页分隔行承担）。
        assert_eq!(lines[1], "甲乙");
        assert_eq!(lines[2], "丙");
        assert_eq!(lines[3], "丁");
        assert_eq!(lines[4], ""); // 页尾空行
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn cap_constant_is_100mb() {
        assert_eq!(INPUT_CAP_BYTES, 100 * 1024 * 1024);
    }

    /// 引擎打开失败面（无效字节 → Err；有无 libpdfium 两环境同判）。
    #[test]
    fn load_rejects_invalid_bytes() {
        let out = load_from_bytes(b"not a pdf".to_vec());
        assert!(out.is_err(), "非 pdf 字节必须 Err（引擎装载失败面）");
    }

    // -------------------------------------------------------------------------
    // S2/S6 打开扫描接线（#277 / 审计 P1-11 桌面接线——纯函数面）
    // -------------------------------------------------------------------------

    /// 评估 → 提示行格式化：S2 命中数 + 族名去重 + S6 表单计数。
    #[test]
    fn security_notice_lines_format_and_dedup() {
        use mdc_pdf::security::{DangerousHit, FormProbe};
        let hit = |family: DangerousFamily, detail: &str| DangerousHit {
            family,
            detail: detail.into(),
        };
        let assessment = OpenAssessment {
            encrypted: false,
            warnings: vec![
                hit(DangerousFamily::OpenAction, "obj 5 0: /OpenAction"),
                // 同族第二命中（去重锁：族名一行一次，计数仍累计）。
                hit(DangerousFamily::OpenAction, "obj 9 0: /OpenAction"),
                hit(DangerousFamily::JavaScript, "obj 7 0: /JavaScript 动作"),
            ],
            form_probe: Some(FormProbe {
                field_count: 3,
                fields: vec![],
            }),
        };
        let lines = security_notice_lines(&assessment);
        assert_eq!(lines.len(), 2, "S2 一行 + S6 一行：{lines:?}");
        assert!(
            lines[0].contains("3 处"),
            "命中计数含同族多命中：{}",
            lines[0]
        );
        assert!(
            lines[0].contains("打开动作") && lines[0].contains("JavaScript"),
            "族名呈现：{}",
            lines[0]
        );
        // 族名去重：同族不重复出现。
        assert_eq!(
            lines[0].matches("打开动作").count(),
            1,
            "同族去重（一处一行）：{}",
            lines[0]
        );
        assert!(lines[1].contains("3 个字段"), "表单计数：{}", lines[1]);

        // 零命中零表单 → 零行（良性文档无提示——误报 = 告警面失效）。
        let clean = security_notice_lines(&OpenAssessment::default());
        assert!(clean.is_empty(), "良性档零提示行：{clean:?}");
    }

    /// 提示行落位：安全区最首（先于扫描件提示）+ 页表/书签行号整体
    /// 平移不漂（PageEntry/HeadingEntry 锚点仍指向正确行）。
    #[test]
    fn security_notices_land_first_and_shift_anchors() {
        let mut raw = raw_two_pages();
        raw.security_notices = vec![
            "【安全提示】测试告警甲。".into(),
            "【表单提示】测试表单乙。".into(),
        ];
        let out = convert(&raw);
        let lines: Vec<&str> = out.text.split('\n').collect();
        assert_eq!(lines[0], "【安全提示】测试告警甲。", "安全行 0");
        assert_eq!(lines[1], "【表单提示】测试表单乙。", "安全行 1");
        assert_eq!(lines[2], "── 第 1 / 2 页 ──", "页分隔行平移到 2");
        // 页表锚点随平移（first_line 0 → 2；整表 +2）。
        assert_eq!(out.pages[0].first_line, 2, "首页锚点 = 0 + 提示行数");
        assert_eq!(out.pages[1].first_line, 8, "次页锚点同平移");
        // 书签锚点仍指向对应页分隔行。
        let heading = out
            .headings
            .iter()
            .find(|h| h.text == "第二章 技术方案")
            .unwrap();
        assert_eq!(heading.line, out.pages[1].first_line, "书签锚点与页表一致");
        // 提示行 Quote 整行 span（与页分隔行同视觉面）。
        assert_eq!(
            out.spans[0],
            vec![(0..lines[0].chars().count(), TokenKind::Quote)]
        );
    }

    // -------------------------------------------------------------------------
    // 引擎面（#[ignore]——需 libpdfium + q31 字体夹具；烟雾通道显式跑）
    // -------------------------------------------------------------------------

    /// 烟雾夹具生成器（不随常规测试跑）：
    ///
    /// ```bash
    /// cargo test gen_smoke_pdf -- --ignored
    /// ./smoke.sh 5 tests/fixtures/smoke.pdf
    /// ./screenshot.sh pdf_shot.png 6 tests/fixtures/smoke.pdf
    /// ```
    ///
    /// 链路：mdc-pdf 写面（`generate_text_pdf`，Type0/CID 中文嵌入；字体
    /// = NotoSerifSC 子集烘焙入库 `tests/fixtures/smoke_font.ttf`——252 唯一
    /// 字符构造性全覆盖，skipped=0 由子集自检背书）+
    /// lopdf 书签注入（测试面 dev-dep——不进 shipped 依赖图）→
    /// 落盘 `tests/fixtures/smoke.pdf` → `load` 读回自校验（引擎面）。
    #[test]
    #[ignore = "烟雾夹具生成器（需 libpdfium；字体夹具已烘焙入库 smoke_font.ttf）"]
    fn gen_smoke_pdf() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let font_path = manifest.join("tests/fixtures/smoke_font.ttf");
        let font = std::fs::read(&font_path).expect("smoke_font.ttf 读取");
        let text = smoke_text();
        let layout = mdc_pdf::generate::PageLayout {
            margin_pt: 72.0,
            font_size_pt: 12.0,
            leading_pt: 17.0,
        };
        let generated =
            mdc_pdf::generate::generate_text_pdf(&font, &text, &layout).expect("generate_text_pdf");
        assert_eq!(
            generated.skipped_chars, 0,
            "夹具文本必须全字符覆盖（skipped=0）"
        );
        assert!(
            generated.pages >= 2,
            "至少 2 页（页分隔验收面）——实际 {} 页",
            generated.pages
        );

        let last = generated.pages - 1;
        let mid = generated.pages / 2;
        let with_outline = inject_outlines(
            generated.bytes,
            &[
                (0usize, "第一章 项目概述"),
                (mid, "第二章 技术方案"),
                (last, "第三章 报价明细"),
            ],
        )
        .expect("书签注入");

        let path = manifest.join("tests/fixtures/smoke.pdf");
        std::fs::write(&path, &with_outline).expect("落盘夹具");
        // 读回自校验（load 需 libpdfium——本测试整体 #[ignore] 口径）。
        let payload = load(&path).expect("夹具读回");
        // 诊断通道（outline 正路径首次带书签夹具实证——主仓此前仅有
        // 「无书签 → 空树」负路径背书）。
        eprintln!(
            "读回诊断：pages={} headings={} bookmarks={} scanned={} empty_pages={}",
            payload.props.pages,
            payload.headings.len(),
            payload.props.bookmarks,
            payload.props.scanned,
            payload.props.empty_pages
        );
        for h in &payload.headings {
            eprintln!("  · {h:?}");
        }
        assert!(payload.props.pages >= 2);
        assert!(payload.headings.len() >= 3, "书签树 ≥ 3（面板验收面）");
        assert!(!payload.props.scanned);
        assert!(payload.props.chars > 0);
        eprintln!(
            "pdf 夹具已生成：{}（{} 页 / {} 书签 / {} 字符 / {} 字节）",
            path.display(),
            payload.props.pages,
            payload.headings.len(),
            payload.props.chars,
            with_outline.len()
        );
    }

    /// 书签注入（lopdf 测试面——`/Outlines` + 前序条目 + 页 Dest）。
    fn inject_outlines(bytes: Vec<u8>, bookmarks: &[(usize, &str)]) -> Result<Vec<u8>, String> {
        use lopdf::{Object, StringFormat, dictionary};
        let mut doc = lopdf::Document::load_mem(&bytes).map_err(|e| format!("lopdf 装载：{e}"))?;
        let pages = doc.get_pages();
        let page_ref = |p: usize| -> Result<Object, String> {
            pages
                .get(&((p + 1) as u32))
                .copied()
                .map(Object::Reference)
                .ok_or_else(|| format!("书签指向不存在页 {p}"))
        };

        let mut ids: Vec<lopdf::ObjectId> = Vec::new();
        for (page, title) in bookmarks {
            let dest_page = page_ref(*page)?;
            let mut t = vec![0xFE, 0xFF]; // UTF-16BE BOM（CJK 文本串标准形）
            for u in title.encode_utf16() {
                t.push((u >> 8) as u8);
                t.push((u & 0xFF) as u8);
            }
            let item = dictionary! {
                "Title" => Object::String(t, StringFormat::Literal),
                "Parent" => Object::Integer(0), // 占位——outlines 落 id 后回填
                "Count" => Object::Integer(0),
                "Dest" => Object::Array(vec![dest_page, Object::Name(b"Fit".to_vec())]),
            };
            ids.push(doc.add_object(item));
        }

        let outlines_id = doc.add_object(dictionary! {
            "Type" => Object::Name(b"Outlines".to_vec()),
            "First" => Object::Reference(ids[0]),
            "Last" => Object::Reference(*ids.last().unwrap()),
            "Count" => Object::Integer(bookmarks.len() as i64),
        });
        // 逐项回填 Parent/Prev/Next。
        for (i, id) in ids.iter().enumerate() {
            let obj = match doc.get_object_mut(*id) {
                Ok(o) => o,
                Err(e) => return Err(format!("条目对象失踪：{e}")),
            };
            if let Object::Dictionary(d) = obj {
                d.set("Parent", Object::Reference(outlines_id));
                if i > 0 {
                    d.set("Prev", Object::Reference(ids[i - 1]));
                }
                if i + 1 < ids.len() {
                    d.set("Next", Object::Reference(ids[i + 1]));
                }
            }
        }
        // catalog 挂 /Outlines。
        let root = match doc.trailer.get(b"Root") {
            Ok(Object::Reference(r)) => *r,
            _ => return Err("catalog 缺失".into()),
        };
        if let Ok(Object::Dictionary(cat)) = doc.get_object_mut(root) {
            cat.set("Outlines", Object::Reference(outlines_id));
        }
        let mut out = Vec::new();
        doc.save_to(&mut out)
            .map_err(|e| format!("lopdf 序列化：{e}"))?;
        Ok(out)
    }

    /// 夹具正文（标书形——行宽 ≤ 37 全角位，generate 全角近似排版口径）。
    fn smoke_text() -> String {
        let sections: [&[&str]; 6] = [
            &[
                "智慧园区综合管理平台投标文件",
                "本文件为技术标书演示样张",
                "由mdc-pdf写面生成Type0与CID字体嵌入",
                "用于mang-shell批4只读渲染烟雾验证",
            ],
            &[
                "第一章项目概述",
                "本项目承建智慧园区综合管理平台",
                "含数据中心机房安防与物联网接入三大部分",
                "建设范围覆盖园区南北两个片区",
                "总体架构采用云原生微服务设计",
                "服务网格统一治理东西向流量",
            ],
            &[
                "第二章技术方案",
                "接入网关集群双活部署",
                "消息中间件采用Kafka三副本",
                "时序数据库选用TDengine集群",
                "现场勘测已完成具备施工条件",
                "冷热通道隔离设计降低PUE至1.3以下",
                "机房承重满足每平米1200公斤要求",
            ],
            &[
                "第三章报价明细",
                "机柜42台单价18000元",
                "UPS电源2套350000元",
                "核心交换机4台128000元",
                "精密空调12台96000元",
                "总价包含五年原厂维保",
            ],
            &[
                "第四章实施计划",
                "总工期180个日历天",
                "分四个阶段交付验收",
                "第一阶段完成基础平台部署",
                "第二阶段完成核心业务割接",
                "第三阶段完成压力测试与调优",
                "第四阶段完成终验与移交",
            ],
            &[
                "第五章服务承诺",
                "七年原厂维保与响应",
                "故障响应时间不超过三十分钟",
                "重大故障四小时内到场处置",
                "每季度提供巡检报告",
                "培训不少于四十学时",
            ],
        ];
        let mut s = String::new();
        for (i, sec) in sections.iter().enumerate() {
            if i > 0 {
                s.push('\n'); // 段间空行（generate 的 \n 语义：行界 + 空行）
            }
            for line in *sec {
                s.push_str(line);
                s.push('\n');
            }
        }
        s
    }
}
