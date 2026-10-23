//! mdc-pdf 会话 wasm 出口（阶段 5 §八.4 PDF 表面）。
//!
//! ## 架构定谳（渲染分离、真相不分离）
//! pdf.js = 纯显示面（canvas 像素 + text layer 选区几何——从不参与
//! 模型裁决）；[`WasmPdfSession`] = 模型层单一真相：安全评估
//! （`open_scan` S2/S5/S6 全扫）、搜索/文本（v0 宿主注入通道——
//! pdf.js 逐页 `getTextContent` 喂入，**会话命中权威 + pdf.js 几何
//! 显示，双面诚实**）、标注库（四 kind 命令面）、落盘字节
//! （lopdf 增量写——前缀逐位保持/幂等/加密写拒语义锁全数继承）。
//!
//! pdfium 引擎/LRU/渲染工人/看门狗在 web 面**不消费**（wasm 无线程
//! ——S4 架构在 wasm 塌缩是 A 路线否决理由之一）；native 路径资产
//! 保留为壳实体（wry）落地后的 C 升级位（protocol.rs 信封形态使
//! 渲染器可替换）。
//!
//! ## 出口形状
//! - [`pdf_open_scan`]：字节 → `OpenAssessment` JSON（加密态/危险
//!   对象/表单探针——打开评估先行，宿主决定去留）。
//! - [`WasmPdfSession::new`]：宿主注入数据路径（meta/assessment/
//!   pages/outline JSON——pdf.js 实数据喂入，`PdfSession::open` 的
//!   v0 既有形态）。
//! - [`WasmPdfSession::set_page_text`]：v0 文本层注入（搜索/抽取的
//!   唯一数据源——无引擎环境的降级通道）。
//! - [`WasmPdfSession::execute`]：六命令 JSON（serde externally-
//!   tagged 原生形——与协议面同构）。
//! - [`WasmPdfSession::attach_writer`] / [`Self::flush_annotations`]
//!   / [`Self::product_bytes`]：lopdf 增量写闭环（落盘位置由宿主
//!   决定——会话零文件系统）。
//!
//! ## 错误面
//! typed JSON 串 `{"code":"pdf_*","message":"..."}`——打开/命令/
//! 写器三族分码；`CommandError` 的 `SecurityDeny.reason` 稳定码
//! 透传（S5 加密写拒等）。

use wasm_bindgen::prelude::*;

/// panic → console（wasm trap 定位面——mdcore-wasm 同款）。
#[wasm_bindgen]
pub fn pdf_wasm_init() {
    console_error_panic_hook::set_once();
}

use mdc_pdf::annotation::{AnnotationDraft, AnnotationPatch};
use mdc_pdf::command::{
    AnnotationFilter, Command, CommandError, SearchOptions, Source, TextTarget,
};
use mdc_pdf::document::{DocumentMeta, OpenError, PdfSession};
use mdc_pdf::page::{PageGeometry, PageIndex, PageSpan};
use mdc_pdf::security::{Limits, OpenAssessment};
use mdc_pdf::security_scan::open_scan;
use mdc_pdf::writer_lopdf::LopdfWriter;

// ---------------------------------------------------------------------------
// 错误面
// ---------------------------------------------------------------------------

/// wasm 边界错误（typed 码——宿主可 JSON.parse）。
#[derive(Debug)]
pub struct PdfWasmError {
    pub code: &'static str,
    pub message: String,
}

impl PdfWasmError {
    fn to_js(&self) -> JsValue {
        JsValue::from_str(
            &serde_json::json!({"code": self.code, "message": self.message}).to_string(),
        )
    }
}

fn err(code: &'static str, message: impl Into<String>) -> PdfWasmError {
    PdfWasmError {
        code,
        message: message.into(),
    }
}

impl From<OpenError> for PdfWasmError {
    fn from(e: OpenError) -> Self {
        err("pdf_open", format!("{e:?}"))
    }
}

impl From<CommandError> for PdfWasmError {
    fn from(e: CommandError) -> Self {
        // SecurityDeny 的稳定策略码透传（S5/S3——宿主可本地化文案）。
        match &e {
            CommandError::SecurityDeny { reason } => {
                err("pdf_security_deny", format!("{reason}: {e:?}"))
            }
            _ => err("pdf_cmd", format!("{e:?}")),
        }
    }
}

// ---------------------------------------------------------------------------
// 纯内核（JsValue-free——原生 cargo test 面）
// ---------------------------------------------------------------------------

/// 打开扫描（S2 危险对象 + S5 加密态 + S6 表单探针——只报告不阻断）。
pub fn open_scan_core(bytes: &[u8]) -> Result<String, PdfWasmError> {
    let assessment = open_scan(bytes, &Limits::default())
        .map_err(|e| err("pdf_scan_failed", format!("{e:?}")))?;
    serde_json::to_string(&assessment).map_err(|e| err("pdf_serialize", e.to_string()))
}

/// 会话构造（宿主注入数据路径——pdf.js 实数据喂入）。
pub fn session_new_core(
    meta_json: &str,
    assessment_json: &str,
    pages_json: &str,
    outline_json: &str,
) -> Result<PdfSession, PdfWasmError> {
    let meta: DocumentMeta =
        serde_json::from_str(meta_json).map_err(|e| err("pdf_bad_json", format!("meta: {e}")))?;
    let assessment: OpenAssessment = serde_json::from_str(assessment_json)
        .map_err(|e| err("pdf_bad_json", format!("assessment: {e}")))?;
    let pages: Vec<PageGeometry> =
        serde_json::from_str(pages_json).map_err(|e| err("pdf_bad_json", format!("pages: {e}")))?;
    let outline = serde_json::from_str(outline_json)
        .map_err(|e| err("pdf_bad_json", format!("outline: {e}")))?;
    PdfSession::open(meta, assessment, pages, outline).map_err(PdfWasmError::from)
}

/// 命令执行（六命令 serde externally-tagged 原生形透传）。
pub fn execute_core(session: &mut PdfSession, cmd_json: &str) -> Result<String, PdfWasmError> {
    let cmd: Command =
        serde_json::from_str(cmd_json).map_err(|e| err("pdf_bad_json", format!("command: {e}")))?;
    let outcome = session.execute(cmd).map_err(PdfWasmError::from)?;
    serde_json::to_string(&outcome).map_err(|e| err("pdf_serialize", e.to_string()))
}

/// 增量 flush 统计 JSON：`{bytes_appended, prefix_bytes}`。
pub fn flush_stats_json(stats: &mdc_pdf::writer::IncrementalSave) -> String {
    serde_json::json!({
        "bytes_appended": stats.bytes_appended,
        "prefix_bytes": stats.prefix_bytes,
    })
    .to_string()
}

// 命令构造辅助（测试面用——与协议面 serde 形同构）。
pub fn extract_text_cmd(pages: &[u32], source: Source) -> String {
    let target = if pages.is_empty() {
        TextTarget::All
    } else {
        let last = pages[pages.len() - 1];
        TextTarget::Pages(PageSpan::new(PageIndex(pages[0]), PageIndex(last)).expect("span"))
    };
    serde_json::to_string(&Command::ExtractText { target, source }).unwrap()
}

pub fn annotate_add_cmd(draft: AnnotationDraft, source: Source) -> String {
    serde_json::to_string(&Command::AnnotateAdd { draft, source }).unwrap()
}

pub fn annotate_list_cmd(filter: AnnotationFilter, source: Source) -> String {
    serde_json::to_string(&Command::AnnotateList { filter, source }).unwrap()
}

pub fn search_cmd(query: &str, source: Source) -> String {
    serde_json::to_string(&Command::Search {
        query: query.to_string(),
        opts: SearchOptions {
            case_insensitive: false,
            max_hits: 1000,
        },
        source,
    })
    .unwrap()
}

/// AnnotationPatch 的空构造（测试面）。
pub fn noop_patch() -> AnnotationPatch {
    AnnotationPatch::default()
}

/// 页索引构造（测试面）。
pub fn page_index(n: u32) -> PageIndex {
    PageIndex(n)
}

// ---------------------------------------------------------------------------
// wasm-bindgen 面
// ---------------------------------------------------------------------------

/// 打开扫描：字节 → OpenAssessment JSON（加密态/危险对象/表单探针）。
#[wasm_bindgen]
pub fn pdf_open_scan(bytes: &[u8]) -> Result<String, JsValue> {
    open_scan_core(bytes).map_err(|e| e.to_js())
}

/// PDF 会话句柄（模型层单一真相——标注/搜索/安全/落盘字节）。
#[wasm_bindgen]
pub struct WasmPdfSession {
    inner: PdfSession,
}

#[wasm_bindgen]
impl WasmPdfSession {
    /// 会话构造（宿主注入：meta/assessment/pages/outline JSON——
    /// pdf.js 实数据喂入，`PdfSession::open` v0 既有形态）。
    #[wasm_bindgen(constructor)]
    pub fn new(
        meta_json: &str,
        assessment_json: &str,
        pages_json: &str,
        outline_json: &str,
    ) -> Result<WasmPdfSession, JsValue> {
        let inner = session_new_core(meta_json, assessment_json, pages_json, outline_json)
            .map_err(|e| e.to_js())?;
        Ok(WasmPdfSession { inner })
    }

    /// v0 文本层注入（搜索/抽取的唯一数据源——pdf.js 逐页
    /// getTextContent 喂入；页界校验即时执行）。
    pub fn set_page_text(&mut self, page: u32, text: &str) -> Result<(), JsValue> {
        self.inner
            .set_page_text(PageIndex(page), text)
            .map_err(|e| PdfWasmError::from(e).to_js())
    }

    /// 会话时钟注入（epoch ms——核不取系统时间）。
    pub fn set_clock_ms(&mut self, now_ms: u64) {
        self.inner.set_clock_ms(now_ms);
    }

    /// 六命令执行（serde externally-tagged 原生形——与协议面同构）。
    /// 成功返回 Outcome JSON；失败抛 typed 错误串。
    pub fn execute(&mut self, cmd_json: &str) -> Result<String, JsValue> {
        execute_core(&mut self.inner, cmd_json).map_err(|e| e.to_js())
    }

    /// lopdf 写器装载（含 open_scan 安全门共用——与打开面同一实现）。
    pub fn attach_writer(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let writer =
            LopdfWriter::from_bytes(bytes.to_vec()).map_err(|e| PdfWasmError::from(e).to_js())?;
        self.inner.attach_writer(Box::new(writer));
        Ok(())
    }

    /// 增量保存（零脏幂等；加密档 S5 写拒——reason 码透传）。
    /// 返回 `{bytes_appended, prefix_bytes}` JSON。
    pub fn flush_annotations(&mut self) -> Result<String, JsValue> {
        let stats = self
            .inner
            .flush_annotations()
            .map_err(|e| PdfWasmError::from(e).to_js())?;
        Ok(flush_stats_json(&stats))
    }

    /// 当前产物字节（前缀 + 既往修订——落盘位置由宿主决定）。
    pub fn product_bytes(&self) -> Vec<u8> {
        self.inner.product_bytes().unwrap_or_default()
    }
}
