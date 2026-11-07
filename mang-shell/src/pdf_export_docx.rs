//! #307 ②功能独立项：PDF→DOCX 文本版导出桥——组合根（产品接线面）。
//!
//! # 定位
//!
//! 三域纪律的组合点：mdc-pdf（提取——pdfium 引擎 `extract_text`）与
//! mdc-office-io（构造——[`pdf_import`] 纯数据模型 + DOCX 写面）互相
//! 零依赖，本模块是两者的**唯一装配处**（mang-shell 双依赖既有边，
//! #307 阶段① `convert` 接线同位）。
//!
//! ```text
//!   pdf bytes ──► PdfiumEngine::open ──► extract_text(全页闭区间)
//!                     │                        │
//!                     │（进程级单绑定——与       ▼
//!                     │  PDF 视图/渲染同源）   Vec<(PageIndex, String)>
//!                     ▼                        │
//!              EngineOpen 拒绝            PdfImportDoc（纯数据边界）
//!                                              │
//!                     零文本 ── NoTextLayer ◄───┤（扫描件预检——OCR 面
//!                                              │  未接，#306 显式指引）
//!                                              ▼
//!                                    pdf_import_to_docx → docx bytes
//! ```
//!
//! # 错误分类（typed——不静默失败纪律）
//!
//! [`PdfToDocxError`] 四类：`EngineOpen`（pdfium 缺席/输入超限/坏件——
//! 附 `--check-pdfium` 部署自检指引，#224 同通道）、`Extract`（文本层
//! 提取引擎面失败）、`NoTextLayer`（零提取字符——扫描件/纯图片 PDF；
//! OCR 接线是 #306 的活，本面显式拒而非产出空壳）、`Import`（office-io
//! 写面拒绝透传）。
//!
//! # 线程纪律
//!
//! pdfium 单线程库（进程级 FFI 互斥兜底——engine_pdfium 模块文档）：
//! 本函数供 shell 后台执行器调用（与渲染工人同一串行化语义，无新增
//! 分线程面）。

use mdc_office_io::pdf_import::{PdfImportDoc, PdfImportError, PdfImportPage, pdf_import_to_docx};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::page::{PageIndex, PageSpan};

/// PDF→DOCX 文本版导出失败分类。
#[derive(Debug)]
pub enum PdfToDocxError {
    /// pdfium 引擎打开失败（库缺席/输入超限/容器损坏）。
    EngineOpen(String),
    /// 文本层提取失败（引擎命令面拒绝——页区间/渲染内部错误）。
    Extract(String),
    /// 零提取字符（扫描件/纯图片 PDF——OCR 面未接，显式拒）。
    NoTextLayer,
    /// office-io 导入/写面拒绝（零页档/资源上限）。
    Import(PdfImportError),
}

impl std::fmt::Display for PdfToDocxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfToDocxError::EngineOpen(detail) => write!(
                f,
                "PDF 引擎打开失败（{detail}）。可运行 mang-shell --check-pdfium 做部署自检。"
            ),
            PdfToDocxError::Extract(detail) => {
                write!(f, "PDF 文本提取失败：{detail}")
            }
            PdfToDocxError::NoTextLayer => write!(
                f,
                "未提取到任何文本（可能是扫描件或纯图片 PDF）——文本版导出无内容。\
                OCR 识别接线见 issue #306。"
            ),
            PdfToDocxError::Import(e) => write!(f, "DOCX 构造拒绝：{e}"),
        }
    }
}

impl std::error::Error for PdfToDocxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PdfToDocxError::Import(e) => Some(e),
            _ => None,
        }
    }
}

/// PDF → DOCX 文本版导出（组合根单点——shell 后台执行器调用）。
///
/// 页序契约：引擎 `extract_text` 载荷页号升序（trait 文档），直接入
/// [`PdfImportDoc`]（页序 = 文档序）。零页 PDF 走 [`PdfImportError::
/// EmptyDocument`] fail-closed 链（不欠拟合引擎页数语义）。
pub fn pdf_to_docx_bytes(pdf: &[u8]) -> Result<Vec<u8>, PdfToDocxError> {
    let mut engine = PdfiumEngine::open(pdf.to_vec())
        .map_err(|e| PdfToDocxError::EngineOpen(format!("{e:?}")))?;
    let count = engine.page_count();
    if count == 0 {
        return Err(PdfToDocxError::Import(PdfImportError::EmptyDocument));
    }
    let span = PageSpan::new(PageIndex(0), PageIndex(count - 1))
        .map_err(|e| PdfToDocxError::Extract(format!("页区间构造失败：{e:?}")))?;
    let pages = engine
        .extract_text(span)
        .map_err(|e| PdfToDocxError::Extract(format!("{e:?}")))?;
    let doc = PdfImportDoc::new(
        pages
            .into_iter()
            .map(|(_idx, text)| PdfImportPage::new(text))
            .collect(),
    );
    // 扫描件预检：零字符显式拒（不产出空壳 docx——与 #306「暂不支持」
    // 显式化同语义，错误文案给出 issue 指引）。
    if doc.total_chars() == 0 {
        return Err(PdfToDocxError::NoTextLayer);
    }
    pdf_import_to_docx(&doc).map_err(PdfToDocxError::Import)
}
