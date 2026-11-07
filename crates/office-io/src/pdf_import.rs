//! PDF→DOCX 文本版导出桥（#307 ②功能独立项）——office-io 侧写面。
//!
//! # 定位
//!
//! #307 任务②「PDF → DOCX」的**文本版**切片：「可以先做『PDF → 文本 +
//! 图片』的简单版，再逐步加结构识别」——本模块承载文本半边：提取方
//! （mdc-pdf `ExtractText` 命令面）给出的逐页原始文本 → 段落启发式切分
//! → [`DocxWriter`] 写面构造 DOCX。图片半边被两处既有纪律卡住（docx-rs
//! `default-features = false` 供应链闭包 + mdc-pdf 无图片提取 API），
//! 待 Lead 裁定路线后另立项，按同模型扩展 [`PdfImportPage`]。
//!
//! # 边界（三域纪律）
//!
//! 输入 [`PdfImportDoc`] 是**纯数据边界**：本模块对 mdc-pdf 零编译依赖
//! （office-io 三域纪律不破），装配由组合根（mang-shell）完成——
//! `PdfSession` + 引擎 → `ExtractText` → 逐页文本 → 本模型。
//!
//! # 段落启发式（简单版——诚实账本）
//!
//! ```text
//!   归一：CRLF / 孤立 CR → LF
//!   切段①：空白行为界（提取器给出空行时——实测 pdfium 对生成器
//!          段间空行不产生文本对象，此规则对真实空行提取面生效）
//!   切段②：行尾 CJK 句末终止符（。！？；）收段——句段启发
//!          （**过切偏向**：宁多段不粘连，编辑面段落=可操作单元；
//!          冒号「：」引入后续不入集——未完给句不应切）
//!   段内：软换行连接——断点两侧任一侧为 CJK 字符则直连（中文文档
//!         主流场景无词间空格），否则拉丁词间补单空格
//!   行面：每行 trim（缩进/行尾空白是版面信息不是语义——文本版不保）
//! ```
//!
//! 实测口径（探针实证，smoke.pdf）：pdfium 提取对无标点文档退化为
//! 整页一段——几何面结构识别（行距/行宽/位置）属 #307 后续项
//!
//! # 局限（#307 验收 5「标注局限」的载体）
//!
//! 不保真：图片（图片面另立项）、表格结构（表格文本进段落流）、版面
//! （多栏/对齐/缩进）、字体（字号/粗斜体）、页眉页脚。页位以分页符
//! 保持（页间 `w:br w:type="page"`——阅读侧页序可对照源 PDF）。
//!
//! # 错误分类
//!
//! [`PdfImportError`] 两类：`EmptyDocument`（零页——fail-closed 无
//! 产物；单页无文本**不拒**——图片页/空白页是真实页位，发射空位 +
//! 分页符是诚实表示）；`Write`（写面上限/输入域拒绝透传——
//! `MAX_PARAGRAPHS`/`MAX_RUNS` 敌意大文档的 typed 硬上界）。

use crate::docx::{DocxError, DocxWriter};

/// 单页提取文本（原始载荷——pdfium `ExtractText` 全页串，行界 `\r\n`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfImportPage {
    /// 页原始文本（空串 = 无文本层页——图片页/空白页）。
    pub text: String,
}

impl PdfImportPage {
    /// 从原始提取串构造。
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// PDF→DOCX 导入模型（页序 = 文档序）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdfImportDoc {
    /// 逐页原始文本（提取方保证页号升序——引擎 `ExtractText` 载荷形状）。
    pub pages: Vec<PdfImportPage>,
}

impl PdfImportDoc {
    /// 从逐页文本构造。
    pub fn new(pages: Vec<PdfImportPage>) -> Self {
        Self { pages }
    }

    /// 全文档提取字符数（组合根的「扫描件零文本」预检面——零字符时
    /// 显式指引 OCR 面未接，不产出空壳）。
    pub fn total_chars(&self) -> usize {
        self.pages.iter().map(|p| p.text.chars().count()).sum()
    }
}

/// 导入失败分类（typed——不静默）。
#[derive(Debug)]
pub enum PdfImportError {
    /// 零页文档（fail-closed：无页即无从谈导出，零产物返回）。
    EmptyDocument,
    /// 写面拒绝（[`DocxWriter::save_to_vec`] 漏斗——上限/输入域透传）。
    Write(DocxError),
}

impl std::fmt::Display for PdfImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfImportError::EmptyDocument => {
                write!(f, "PDF 导入为空文档：零页（无内容可导出）")
            }
            PdfImportError::Write(e) => write!(f, "DOCX 写面拒绝：{e}"),
        }
    }
}

impl std::error::Error for PdfImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PdfImportError::Write(e) => Some(e),
            _ => None,
        }
    }
}

/// PDF→DOCX 文本版导出（模型 → DOCX 字节）。
///
/// 页序保持：页间发射分页符（首页前置无）——单页无文本仍占页位
/// （空位 + 分页符），与源 PDF 页序 1:1 可对照。
pub fn pdf_import_to_docx(doc: &PdfImportDoc) -> Result<Vec<u8>, PdfImportError> {
    if doc.pages.is_empty() {
        return Err(PdfImportError::EmptyDocument);
    }
    let mut writer = DocxWriter::new();
    for (idx, page) in doc.pages.iter().enumerate() {
        if idx > 0 {
            writer.page_break();
        }
        for para in paragraphs_from_page_text(&page.text) {
            writer.paragraph_text(&para);
        }
    }
    writer.save_to_vec().map_err(PdfImportError::Write)
}

/// 段落启发式（纯函数——规则面见模块文档）。
///
/// 空白行切段 + 行尾 CJK 句末终止符（。！？；）收段 + 段内软换行
/// CJK 感知连接；全空白输入返回空段集（调用方按「无文本页」处理，
/// 不发射段落）。**过切偏向**：宁可多段不粘连（编辑面段落 = 可操作
/// 单元——几何面结构识别属 #307 后续项）。
pub fn paragraphs_from_page_text(text: &str) -> Vec<String> {
    // 归一：CRLF / 孤立 CR → LF（pdfium 行界 \r\n；防其它源混入）。
    let normalized = if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    };
    let mut paragraphs: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in normalized.split('\n') {
        if line.trim().is_empty() {
            // 空白行 = 段界（连续空白行合为单界——cur 空即无操作）。
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            // 行面 trim：缩进/行尾空白是版面信息，文本版不保。
            let line = line.trim();
            let closes_paragraph = line.ends_with('。')
                || line.ends_with('！')
                || line.ends_with('？')
                || line.ends_with('；');
            current.push(line);
            if closes_paragraph {
                paragraphs.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs
        .into_iter()
        .map(|lines| join_soft_wrapped(&lines))
        .collect()
}

/// 段内软换行连接：断点两侧任一侧 CJK → 直连；否则补单空格。
fn join_soft_wrapped(lines: &[&str]) -> String {
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            match (out.chars().last(), line.chars().next()) {
                (Some(prev), Some(next)) if !is_cjk(prev) && !is_cjk(next) => out.push(' '),
                _ => {}
            }
        }
        out.push_str(line);
    }
    out
}

/// CJK 字符判定（表意文字 + 假名 + 谚文 + CJK 标点 + 全角形式）。
///
/// 中文文档（本仓主流语料——招标文件）词间无空格：软换行直连是
/// 正确语义；拉丁词间空格在断点补齐（`is_cjk` 判据两侧任一命中即
/// 直连——中英混排断点按中文侧语义处理）。
fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3000..=0x30FF      // CJK 标点 + 假名
            | 0x3400..=0x4DBF // 表意扩展 A
            | 0x4E00..=0x9FFF // CJK 统一表意
            | 0xAC00..=0xD7AF // 谚文音节
            | 0xF900..=0xFAFF // 兼容表意
            | 0xFF00..=0xFFEF // 全角形式（含全角标点）
            | 0x20000..=0x2FA1F // 表意扩展 B-F + 兼容补充
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_soft_wrap_joins_directly() {
        assert_eq!(
            paragraphs_from_page_text("第一行文本\n第二行文本"),
            vec!["第一行文本第二行文本".to_string()]
        );
    }

    #[test]
    fn sentence_terminal_closes_paragraph() {
        // 句末终止符收段（过切偏向）——无标点行继续软连接。
        assert_eq!(
            paragraphs_from_page_text("第一句结束。\n第二句开始\n继续软换行"),
            vec![
                "第一句结束。".to_string(),
                "第二句开始继续软换行".to_string()
            ]
        );
        // 全角叹号/问号/分号同族；冒号「：」引入后续——不切。
        assert_eq!(
            paragraphs_from_page_text("警报！\n下文"),
            vec!["警报！".to_string(), "下文".to_string()]
        );
        assert_eq!(
            paragraphs_from_page_text("项目清单：\n明细一行"),
            vec!["项目清单：明细一行".to_string()]
        );
    }

    #[test]
    fn latin_soft_wrap_inserts_space() {
        assert_eq!(
            paragraphs_from_page_text("hello\nworld"),
            vec!["hello world".to_string()]
        );
    }

    #[test]
    fn mixed_boundary_follows_cjk_side() {
        // 断点两侧：「世界」尾 + 「文」首——两侧皆 CJK 直连；拉丁段内
        // 换行补空格（中英混排一行内两种断点）。
        assert_eq!(
            paragraphs_from_page_text("hello 世界\n文本 abc\nnext"),
            vec!["hello 世界文本 abc next".to_string()]
        );
    }

    #[test]
    fn blank_line_splits_paragraphs() {
        assert_eq!(
            paragraphs_from_page_text("段一第一行\n段一第二行\n\n\n段二"),
            vec!["段一第一行段一第二行".to_string(), "段二".to_string()]
        );
    }

    #[test]
    fn crlf_and_line_trim_normalized() {
        assert_eq!(
            paragraphs_from_page_text("  a\r\nb  \r\n\r\nc\r\n"),
            vec!["a b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn whitespace_only_page_yields_no_paragraphs() {
        assert!(paragraphs_from_page_text("").is_empty());
        assert!(paragraphs_from_page_text(" \n \n  \n").is_empty());
    }

    #[test]
    fn total_chars_counts_all_pages() {
        let doc = PdfImportDoc::new(vec![
            PdfImportPage::new("abc"),
            PdfImportPage::new("中文"),
            PdfImportPage::new(""),
        ]);
        assert_eq!(doc.total_chars(), 5);
    }
}
