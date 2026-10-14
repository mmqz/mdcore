//! DOCX 读/写切片（xlsx-r2 Phase 3——小而可用，模型级）。
//!
//! **定位（2026-10-14，用户三阶段指令 Phase 3）**：用既有研究结论
//! （分析仓 ADR-O1 O1-3：docx-rs = docx 写 + 模型级读，bokuweb/docx-rs
//! 0.4.22 MIT）落地**小而可用**的 DOCX 切片。**模型级处理，与渲染
//! 保真分离**：本模块产出/消费结构化文本模型（块/段落/run/表），
//! 不做布局、分页、字体度量或任何渲染语义。
//!
//! ## 模块边界（CAPABILITIES.md §2 裁决——DOCX 独立模块）
//!
//! - 与 XLSX 模块（[`crate::io`]/[`crate::write`]）**不共用领域模型、
//!   不建统一 Office 接口抽象**：本模块自有模型（[`DocxDocument`]/
//!   [`Block`]/[`RunData`]），错误面自有（[`DocxError`]），按格式独立
//!   演进。
//! - 共享面仅限**容器级基础设施**：DOCX 与 XLSX 同为 zip 容器与
//!   OPC 部件约定，复用同一门结构（门 1 声明值预扫 [`crate::io::prescan`]
//!   与门 2 尺寸诚实性、门 3 部件良构性融合趟 [`crate::io`] 内部
//!   机制）以及结构化警告/兼容模式（[`crate::io::ReadOptions`]）——
//!   这是引擎侧输入防线（SECURITY_POLICY v1.1 §2.1），非领域抽象。
//! - 共享消毒面 = mdc-office sanitize 六面（上下文级纯函数，跨格式
//!   复用正当——消费方责任边界同 [`crate::write`] 模块文档）。
//!
//! ## 读面（[`read_docx`] / [`read_docx_from_bytes`]）
//!
//! 容器三门（与 XLSX 读路径同构：严格默认 + 显式兼容）→ docx-rs
//! `read_docx` 模型解析 → 自有模型提取：
//! - **块序**（段落/表，文档体顺序）；段落 = 样式 ID 透传（如
//!   `Heading1`——语义判定归消费方，模型级不做标题解释）+ run 序
//!   （文本/加粗/斜体）；超链接**内文本**提取（目标 URL 丢弃——
//!   不支持面登记）；表 = 行 × 单元格拼接文本（单元格内多段落以
//!   `\n` 连接）。
//! - Tab → `\t`、Break → `\n`、CarriageReturn → `\r`（文本模型
//!   保真）；结构化警告面与 XLSX 一致（畸形部件/控制字符）。
//!
//! ## 写面（[`DocxWriter`]）
//!
//! 新建文档（**仅新建语义**——与 [`crate::write`] 的 XLSX 边界
//! 同构，不存在已有文档编辑往返）：标题（Heading1-9 样式定义 +
//! outline 级——无字体面[模型级]）/富文本段落（run 级加粗斜体）/
//! 文本表格。写产物过读面全环往返测试（tests/docx_roundtrip.rs）。
//!
//! ## 不支持面与数据损失登记（诚实边界——模型级切片的现实）
//!
//! 读侧**跳过并登记**（文本提取不受损面外的可见损失）：
//! - 图形/图片/形状（Drawing/Shape——模型级切片不含媒体面）；
//! - 超链接目标（文本保留、URL 丢弃）；
//! - 修订追踪（Insert/Delete/MoveFrom/MoveTo 容器——内容不入提取）；
//! - 页码/总页数域（PageNum/NumPages）、书签、批注、
//!   StructuredDataTag（内容控件）；
//! - 页眉/页脚/脚注/尾注/节属性（模型在 docx-rs 内，本切片面不
//!   暴露——按需增量）；
//! - 编号/列表语义（列表段落文本保留，编号结构丢弃）。
//!
//! 写侧**不支持**（引擎能力在[docx-rs 写面远大于本切片]，未实测
//! 不暴露）：图片、表格样式、域、目录、页眉页脚、编号列表。
//!
//! 证据链：tests/docx_roundtrip.rs（写→读全环 + 门结构复用 + 敌意
//! 输入三门钉定）+ 本模块内联单测。

use crate::io::{self, CompatibilityWarning, ReadOptions};

// ---------------------------------------------------------------------------
// 错误面（DOCX 自有——按格式独立演进纪律）
// ---------------------------------------------------------------------------

/// DOCX 读/写统一错误。
#[derive(Debug)]
pub enum DocxError {
    /// 容器门拒绝（门 1/2/3——与 XLSX 同一门结构；内含具体拒绝面）。
    Container(io::ReadError),
    /// docx-rs 模型解析错误。
    Parse(String),
    /// 文件 IO 错误。
    Io(String),
}

impl std::fmt::Display for DocxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocxError::Container(e) => write!(f, "container: {e}"),
            DocxError::Parse(m) => write!(f, "parse error: {m}"),
            DocxError::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for DocxError {}

impl From<io::ReadError> for DocxError {
    fn from(e: io::ReadError) -> Self {
        DocxError::Container(e)
    }
}

// ---------------------------------------------------------------------------
// 读侧模型（自有类型——docx-rs 类型零泄漏）
// ---------------------------------------------------------------------------

/// 单个文本运行（模型级三面：文本 + 加粗 + 斜体）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunData {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

/// 段落：样式 ID 透传 + run 序。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphData {
    /// 样式 ID（如 `Heading1`/`Normal`）——语义判定归消费方。
    pub style: Option<String>,
    pub runs: Vec<RunData>,
}

impl ParagraphData {
    /// 段落拼接文本（run 文本直连——不加空格，保真 run 边界）。
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// 文档块（有序）。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(ParagraphData),
    Table(TableData),
}

/// 文本表格：行 × 单元格拼接文本。
#[derive(Debug, Clone, PartialEq)]
pub struct TableData {
    pub rows: Vec<Vec<String>>,
}

/// 整档提取结果：块序 + 结构化警告（容器门面，与 XLSX 一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct DocxDocument {
    pub blocks: Vec<Block>,
    pub warnings: Vec<CompatibilityWarning>,
}

// ---------------------------------------------------------------------------
// 读 API（容器三门 → docx-rs 模型解析 → 自有模型提取）
// ---------------------------------------------------------------------------

/// 读 DOCX 文件（严格模式默认——行为见模块文档）。
pub fn read_docx(path: &str) -> Result<DocxDocument, DocxError> {
    read_docx_with_options(path, &ReadOptions::default())
}

/// 读 DOCX 文件（带选项）。`opts.compat = true` 时畸形部件降级为
/// 结构化警告（门 1/门 2/安全上限恒不放松——语义同 [`io::read_book_with_options`]）。
pub fn read_docx_with_options(path: &str, opts: &ReadOptions) -> Result<DocxDocument, DocxError> {
    let bytes = std::fs::read(path).map_err(|e| DocxError::Io(format!("read {path}: {e}")))?;
    read_bytes_gated(bytes, opts)
}

/// 读 DOCX 字节（容器三门同样适用——字节入口不绕过输入防线）。
pub fn read_docx_from_bytes(bytes: &[u8]) -> Result<DocxDocument, DocxError> {
    read_docx_from_bytes_with_options(bytes, &ReadOptions::default())
}

/// 读 DOCX 字节（带选项）。
pub fn read_docx_from_bytes_with_options(
    bytes: &[u8],
    opts: &ReadOptions,
) -> Result<DocxDocument, DocxError> {
    read_bytes_gated(bytes.to_vec(), opts)
}

/// 门结构：门 1（prescan）→ 门 2+3 融合趟（尺寸 + 部件良构——
/// 复用 XLSX 读路径同一实现）→ docx-rs 模型解析。
fn read_bytes_gated(bytes: Vec<u8>, opts: &ReadOptions) -> Result<DocxDocument, DocxError> {
    let cursor = std::io::Cursor::new(&bytes);
    let mut za = zip::ZipArchive::new(cursor)
        .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
    io::prescan(&mut za)?;
    let mut warnings = Vec::new();
    // 门 2+3 融合趟（io 私有实现经公开管道函数复用——见 io::read_book）。
    io::verify_gates(&mut za, !opts.compat, &mut warnings)?;
    drop(za);

    let docx = docx_rs::read_docx(&bytes).map_err(|e| DocxError::Parse(format!("{e}")))?;
    Ok(DocxDocument {
        blocks: extract_blocks(&docx),
        warnings,
    })
}

/// docx-rs 模型 → 自有模型提取（读面跳过项见模块文档登记表）。
fn extract_blocks(docx: &docx_rs::Docx) -> Vec<Block> {
    docx.document
        .children
        .iter()
        .filter_map(|child| match child {
            docx_rs::DocumentChild::Paragraph(p) => Some(Block::Paragraph(extract_paragraph(p))),
            docx_rs::DocumentChild::Table(t) => Some(Block::Table(extract_table(t))),
            // 书签/批注/内容控件/TOC/节——模型级切片面外，跳过（登记）。
            _ => None,
        })
        .collect()
}

/// 段落提取：样式透传 + run 序（Run + Hyperlink 内 run[目标丢弃]）。
fn extract_paragraph(p: &docx_rs::Paragraph) -> ParagraphData {
    let style = p.property.style.as_ref().map(|s| s.val.clone());
    let mut runs = Vec::new();
    for child in &p.children {
        match child {
            docx_rs::ParagraphChild::Run(r) => runs.push(extract_run(r)),
            // 超链接：内文本保留（run 边界保持），目标 URL 丢弃
            //（不支持面登记——模型级切片无关系面）。
            docx_rs::ParagraphChild::Hyperlink(h) => {
                for c in &h.children {
                    if let docx_rs::ParagraphChild::Run(r) = c {
                        runs.push(extract_run(r));
                    }
                }
            }
            // 修订追踪/书签/批注/域/内容控件——面外跳过（登记）。
            _ => {}
        }
    }
    ParagraphData { style, runs }
}

/// run 提取：文本三源（Text/Tab/Break+CarriageReturn）+ 加粗斜体。
fn extract_run(r: &docx_rs::Run) -> RunData {
    let mut text = String::new();
    for child in &r.children {
        match child {
            docx_rs::RunChild::Text(t) => text.push_str(&t.text),
            docx_rs::RunChild::Tab(_) => text.push('\t'),
            docx_rs::RunChild::Break(_) => text.push('\n'),
            docx_rs::RunChild::CarriageReturn(_) => text.push('\r'),
            // Sym/DeleteText/Drawing/Shape/批注锚——面外跳过（登记）。
            _ => {}
        }
    }
    RunData {
        text,
        bold: r.run_property.bold.is_some(),
        italic: r.run_property.italic.is_some(),
    }
}

/// 表提取：行 × 单元格拼接文本（单元格内多段落 `\n` 连接；嵌套表/
/// 内容控件跳过——登记）。
fn extract_table(t: &docx_rs::Table) -> TableData {
    // 注：docx-rs 0.4.22 模型中 TableChild/TableRowChild 均为单变体
    //（TableRow/TableCell）——直接解构；模型升级增变体时此函数
    // 编译期报非穷尽，而面外项处理策略不变（跳过+登记）。
    let rows = t
        .rows
        .iter()
        .map(|row| match row {
            docx_rs::TableChild::TableRow(r) => r
                .cells
                .iter()
                .map(|cell| match cell {
                    docx_rs::TableRowChild::TableCell(c) => {
                        let mut parts = Vec::new();
                        for content in &c.children {
                            if let docx_rs::TableCellContent::Paragraph(p) = content {
                                let pd = extract_paragraph(p);
                                parts.push(pd.text());
                            }
                        }
                        parts.join("\n")
                    }
                })
                .collect(),
        })
        .collect();
    TableData { rows }
}

// ---------------------------------------------------------------------------
// 写 API（新建语义——DocxWriter）
// ---------------------------------------------------------------------------

/// 富文本 run 规格（写侧输入模型）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunSpec {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

impl RunSpec {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
        }
    }

    pub fn styled(text: impl Into<String>, bold: bool, italic: bool) -> Self {
        Self {
            text: text.into(),
            bold,
            italic,
        }
    }
}

/// DOCX 新建文档写器（**仅新建语义**——不存在已有文档编辑往返，
/// 与 XLSX [`crate::write::WorkbookWriter`] 边界同构）。
///
/// 预置 Heading1-9 段落样式（outline 级——模型级结构语义，无字体/
/// 字号面）——标题在 Word 大纲视图/目录中可用；正文段落样式
/// `Normal`（引擎默认）。
#[derive(Debug, Default)]
pub struct DocxWriter {
    docx: docx_rs::Docx,
}

impl DocxWriter {
    pub fn new() -> Self {
        let mut docx = docx_rs::Docx::new();
        // Heading1-9 样式定义（outline 级；based_on Normal；qFormat
        // ——样式面板可见）。无字体面 = 模型级边界（渲染语义面外）。
        for level in 1u8..=9 {
            let mut style =
                docx_rs::Style::new(format!("Heading{level}"), docx_rs::StyleType::Paragraph)
                    .name(format!("heading {level}"))
                    .based_on("Normal")
                    .next("Normal")
                    .q_format(true);
            style.paragraph_property = style
                .paragraph_property
                .clone()
                .outline_lvl((level - 1) as usize);
            docx = docx.add_style(style);
        }
        Self { docx }
    }

    /// 标题段（level 1-9；样式引用 + outline 级双通道——样式被剥除
    /// 时大纲结构仍可恢复）。
    pub fn heading(&mut self, level: u8, text: &str) -> Result<(), DocxError> {
        if !(1..=9).contains(&level) {
            return Err(DocxError::Parse(format!(
                "heading level must be 1-9, got {level}"
            )));
        }
        let mut p = docx_rs::Paragraph::new()
            .style(&format!("Heading{level}"))
            .add_run(docx_rs::Run::new().add_text(text));
        p.property = p.property.clone().outline_lvl((level - 1) as usize);
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        Ok(())
    }

    /// 单文本段落（便捷面）。
    pub fn paragraph_text(&mut self, text: &str) {
        self.paragraph(&[RunSpec::text(text)]);
    }

    /// 富文本段落（run 级加粗/斜体）。
    pub fn paragraph(&mut self, runs: &[RunSpec]) {
        let mut p = docx_rs::Paragraph::new();
        for spec in runs {
            let mut run = docx_rs::Run::new().add_text(spec.text.as_str());
            let mut rp = docx_rs::RunProperty::new();
            if spec.bold {
                rp = rp.bold();
            }
            if spec.italic {
                rp = rp.italic();
            }
            run.run_property = rp;
            p = p.add_run(run);
        }
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
    }

    /// 文本表格（行 × 单元格文本；首行不特殊化——表头样式属渲染面）。
    pub fn table(&mut self, rows: &[Vec<String>]) {
        let table_rows = rows
            .iter()
            .map(|row| {
                let cells = row
                    .iter()
                    .map(|cell| {
                        docx_rs::TableCell::new().add_paragraph(
                            docx_rs::Paragraph::new()
                                .add_run(docx_rs::Run::new().add_text(cell.as_str())),
                        )
                    })
                    .collect();
                docx_rs::TableRow::new(cells)
            })
            .collect();
        let table = docx_rs::Table::new(table_rows);
        self.docx = std::mem::take(&mut self.docx).add_table(table);
    }

    /// 保存到文件路径。
    pub fn save_to_path(&mut self, path: &str) -> Result<(), DocxError> {
        let mut buf = self.save_to_vec()?;
        std::fs::write(path, &mut buf).map_err(|e| DocxError::Io(format!("write {path}: {e}")))
    }

    /// 保存到内存缓冲（zip 容器字节）。
    pub fn save_to_vec(&mut self) -> Result<Vec<u8>, DocxError> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        let built = std::mem::take(&mut self.docx).build();
        built
            .pack(&mut cursor)
            .map_err(|e| DocxError::Io(format!("pack: {e}")))?;
        // build() 消费 self.docx —— 重建基态（写器可续写：幂等语义，
        // 已写内容已在 built 内落盘，续写 = 新档）。
        self.docx = Self::new().docx;
        Ok(cursor.into_inner())
    }
}

// ---------------------------------------------------------------------------
// 单测（全环往返与敌意输入见 tests/docx_roundtrip.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_level_validation() {
        let mut w = DocxWriter::new();
        assert!(w.heading(1, "ok").is_ok());
        assert!(w.heading(9, "ok").is_ok());
        assert!(w.heading(0, "bad").is_err());
        assert!(w.heading(10, "bad").is_err());
    }

    #[test]
    fn run_spec_constructors() {
        assert_eq!(
            RunSpec::text("a"),
            RunSpec {
                text: "a".into(),
                bold: false,
                italic: false
            }
        );
        assert!(RunSpec::styled("a", true, false).bold);
    }
}
