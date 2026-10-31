//! S2-4（PDF-f4）· 扫描件 OCR——pdfium 渲染 → tesseract 识别
//! （feature `engine-pdfium`）。
//!
//! issue #103 任务 4：`ocr_page(pdf, page, lang) -> String`——扫描件
//! PDF 页 → 中英混合文本。
//!
//! # 管线（零新依赖纪律）
//!
//! ```text
//! pdfium 光栅化（200 DPI 等效宽）→ BGRA → 灰度 PGM（P5——tesseract/
//! leptonica 原生输入格式，**零 image crate 零 zlib 零 PNG 编码**：
//! 头三行 ASCII + 逐字节灰度，Y = 0.299R + 0.587G + 0.114B）
//!   → tesseract <pgm> stdout -l <lang> --psm 3
//!   → 识别文本
//! ```
//!
//! # 环境门（issue 指令「沙盒缺就 SKIP——参照 bid-parse」）
//!
//! [`ocr_available`] 探针：tesseract 二进制 + 语言包清单。产物 API 面：
//! - 二进制缺失 / 语言包缺失 / 识别失败 → typed `Engine` 错误（调用方
//!   决策——产品面不静默）；
//! - **测试面**按探针门控：缺件跳过并打印（pdff4_ocr 环境合同）。
//!
//! # 环境旋钮
//!
//! - `MDC_OCR_BIN`：tesseract 可执行路径（缺省 `PATH` 上的 `tesseract`）；
//! - `MDC_OCR_TESSDATA`：语言包目录（缺省 tesseract 系统目录——本仓
//!   `test-artifacts/ocr-tessdata/` 携带 chi_sim[gitignore 大二进制，
//!   取件脚本见该目录 README]，经此变量注入）。
//!
//! # 语义边界（如实登记）
//!
//! - OCR 结果**非确定性**（tesseract 版本/字体/渲染分辨率敏感）——
//!   断言面取「关键词条出现」而非全文相等（pdff4 钉定英文词 + 中文
//!   短语两个锚点）；
//! - `--psm 3`（全页自动分割）——版面分析面不暴露旋钮（v0 单页扫描件
//!   语义；复杂版面属产品面迭代）；
//! - 渲染宽 = 200 DPI 等效（`pt / 72 × 200`）——识别质量/速度平衡点
//!   （实测锚点；不暴露旋钮——调参面留产品迭代）。

use std::io::Write as _;
use std::process::Command;

use crate::command::CommandError;
use crate::engine::DocumentEngine;
use crate::engine_pdfium::PdfiumEngine;
use crate::page::PageIndex;
use crate::render::{Rasterizer, RenderRequest};
use crate::security::Limits;

/// OCR 环境探针（bid-parse 式环境门——测试门控与产品诊断共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrEnv {
    /// tesseract 二进制可用（`--version` 过）。
    pub binary_ok: bool,
    /// 可用语言包清单（`--list-langs`）。
    pub languages: Vec<String>,
}

impl OcrEnv {
    /// 语言包是否可用。
    pub fn has_lang(&self, lang: &str) -> bool {
        self.languages.iter().any(|l| l == lang)
    }
}

/// 探测 OCR 环境（二进制 + 语言包；tessdata 目录经
/// `MDC_OCR_TESSDATA` 注入）。探测失败面 = 如实返回（不 panic——
/// 环境门语义：调用方据此 SKIP 或提示）。
pub fn ocr_available() -> OcrEnv {
    let bin = tesseract_binary();
    let version = Command::new(&bin).arg("--version").output();
    let binary_ok = version.map(|o| o.status.success()).unwrap_or(false);
    if !binary_ok {
        return OcrEnv {
            binary_ok: false,
            languages: Vec::new(),
        };
    }
    let langs = Command::new(&bin)
        .arg("--list-langs")
        .env("TESSDATA_PREFIX", tessdata_dir())
        .output();
    let languages = langs
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .skip(1) // 首行 = 目录说明
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    OcrEnv {
        binary_ok,
        languages,
    }
}

/// 扫描件 OCR 单页（issue #103 草图签名；`page` 0 基）。
///
/// 管线见模块文档；失败面全 typed `Engine`（二进制缺失/语言包缺失/
/// 渲染失败/识别进程失败——含 stderr 摘要，不静默）。
pub fn ocr_page(pdf: &[u8], page: usize, lang: &str) -> Result<String, CommandError> {
    // ① 光栅化（200 DPI 等效宽——S3-L2 钳制在引擎侧照常生效）。
    let limits = Limits::default();
    limits.check_open_input_bytes(pdf.len() as u64)?;
    let mut engine = PdfiumEngine::open(pdf.to_vec())?;
    // 页界（引擎 rasterize 的 InvalidPage 面前移——更早的明确拒绝）。
    let count = engine.page_count();
    if page >= count as usize {
        return Err(CommandError::InvalidPage(PageIndex(page as u32)));
    }
    // 页宽（pt）→ 200 DPI 像素宽（等比高引擎自适应）。
    let geometry = engine.page_geometry(PageIndex(page as u32))?;
    let width_px = (geometry.effective_width() / 72.0 * 200.0).round() as u32;
    let width_px = width_px.clamp(200, 4000);
    let rendered = engine.rasterize(RenderRequest {
        page: PageIndex(page as u32),
        width_px,
    })?;

    // ② BGRA → 灰度 PGM（P5；零依赖路径）。
    let pgm = bgra_to_pgm(&rendered.bgra, rendered.width_px, rendered.height_px);

    // ③ tesseract（临时文件中转——stdin 不受支持面[tesseract CLI 需
    // 文件路径或 stdin 图像格式头，PGM 走文件最稳]）。
    let bin = tesseract_binary();
    let dir = std::env::temp_dir().join(format!("mdc-ocr-{}-{}", std::process::id(), line_nonce()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| CommandError::engine(format!("OCR 临时目录建立失败：{e}")))?;
    let img_path = dir.join("page.pgm");
    std::fs::write(&img_path, pgm)
        .map_err(|e| CommandError::engine(format!("OCR 图像写入失败：{e}")))?;
    let output = Command::new(&bin)
        .arg(&img_path)
        .arg("stdout")
        .arg("-l")
        .arg(lang)
        .arg("--psm")
        .arg("3")
        .env("TESSDATA_PREFIX", tessdata_dir())
        .output()
        .map_err(|e| CommandError::engine(format!("tesseract 启动失败（{bin}）：{e}")))?;
    let _ = std::fs::remove_dir_all(&dir);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::engine(format!(
            "tesseract 识别失败（exit {:?}）：{}",
            output.status.code(),
            stderr.chars().take(300).collect::<String>()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// 内部
// ---------------------------------------------------------------------------

/// tesseract 二进制路径（env `MDC_OCR_BIN` → PATH 缺省）。
fn tesseract_binary() -> String {
    std::env::var("MDC_OCR_BIN").unwrap_or_else(|_| "tesseract".to_string())
}

/// tessdata 目录（env `MDC_OCR_TESSDATA` → 系统缺省[空串 = tesseract
/// 内建目录]）。
fn tessdata_dir() -> String {
    std::env::var("MDC_OCR_TESSDATA").unwrap_or_default()
}

/// BGRA 缓冲 → 灰度 PGM（P5：`P5\n<w> <h>\n255\n` + 逐字节 Y）。
fn bgra_to_pgm(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len() / 4 + 32);
    let _ = write!(out, "P5\n{width} {height}\n255\n");
    let (chunks, _) = bgra.as_chunks::<4>();
    for p in chunks {
        // BGRA → ITU-R BT.601 亮度。
        let y = (0.299 * f32::from(p[2]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[0]))
            .round()
            .clamp(0.0, 255.0) as u8;
        out.push(y);
    }
    out
}

/// 临时目录唯一后缀（并行调用安全——进程内单调计数）。
fn line_nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}
