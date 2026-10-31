//! PDF-f4 — OCR 接入语义锁（feature `engine-pdfium` + tesseract 环境门）。
//!
//! 环境合同（issue 指令「沙盒缺就 SKIP——参照 bid-parse」）：
//! - tesseract 二进制缺失 → 全部测试**跳过并打印**（不失败）；
//! - 语言包：`MDC_OCR_TESSDATA` 注入（本仓 `test-artifacts/ocr-tessdata/`
//!   携带 chi_sim——取件脚本见该目录 README）；chi_sim 缺失 → 中文面
//!   跳过并打印，英文面照常（eng 系统包）；
//! - pdfium 库定位序同 pdfc_engine（MDC_PDFIUM_LIB → 候选 → 系统）。
//!
//! 锁清单：
//! 1. `scanned_mixed_ocr_english`——扫描件（PIL 真字体渲染 → JPEG →
//!    DCTDecode 嵌入，零文本层）→ ocr_page(eng) → 英文关键词条
//!    （"Specification" / "Requirements"）；
//! 2. `scanned_mixed_ocr_chinese_mixed`——中英混合（chi_sim+eng）→
//!    中文短语锚点（「招标文件」/「验收标准」）——issue 验收
//!    「OCR 出文本（中英混合）」的字面证明；
//! 3. `text_pdf_ocr_sanity`——真文本 PDF（small.pdf）经同一管线
//!    OCR——渲染→识别全链健康度（英文词 "Spike"）；
//! 4. `ocr_env_probe_semantics`——探针面（二进制/语言清单/has_lang）；
//! 5. `missing_lang_typed_error`——缺失语言包 → typed Engine 错误
//!    （产品面不静默）；页越界 → InvalidPage。

#![cfg(feature = "engine-pdfium")]

use std::path::Path;

use mdc_pdf::command::CommandError;
use mdc_pdf::ocr::{ocr_available, ocr_page};
use mdc_pdf::page::PageIndex;

/// tessdata 注入（本仓携带目录存在时）。
fn setup_tessdata() {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/ocr-tessdata");
    if std::env::var_os("MDC_OCR_TESSDATA").is_none() && local.is_dir() {
        // 仅当本地目录存在且含 chi_sim（eng/osd 系统自带）。
        if local.join("chi_sim.traineddata").exists() {
            std::env::set_var("MDC_OCR_TESSDATA", local);
        }
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} 读取失败：{e}"))
}

#[test]
fn scanned_mixed_ocr_english() {
    setup_tessdata();
    let env = ocr_available();
    if !env.binary_ok {
        eprintln!("[pdff4] tesseract 缺失 — SKIP（环境门：issue 指令沙盒缺就 SKIP）");
        return;
    }
    if !env.has_lang("eng") {
        eprintln!("[pdff4] eng 语言包缺失 — SKIP");
        return;
    }
    let scanned = fixture_bytes("scanned_mixed.pdf");
    let text = ocr_page(&scanned, 0, "eng").unwrap();
    eprintln!(
        "[pdff4] eng OCR 输出前 3 行：{}",
        text.lines().take(3).collect::<Vec<_>>().join(" ⏎ ")
    );
    // 关键词条（OCR 非确定性——词条级断言非全文相等）。
    let lower = text.to_lowercase();
    assert!(
        lower.contains("specification"),
        "英文锚点 Specification 须识别（得：{text:?}）"
    );
    assert!(
        lower.contains("requirements"),
        "英文锚点 Requirements 须识别"
    );
}

#[test]
fn scanned_mixed_ocr_chinese_mixed() {
    setup_tessdata();
    let env = ocr_available();
    if !env.binary_ok {
        eprintln!("[pdff4] tesseract 缺失 — SKIP");
        return;
    }
    if !env.has_lang("chi_sim") {
        eprintln!(
            "[pdff4] chi_sim 语言包缺失 — SKIP（取件：test-artifacts/ocr-tessdata/README.md）"
        );
        return;
    }
    let scanned = fixture_bytes("scanned_mixed.pdf");
    // 中英混合（issue 验收字面）。
    let text = ocr_page(&scanned, 0, "chi_sim+eng").unwrap();
    eprintln!(
        "[pdff4] 中英混合 OCR 输出前 4 行：{}",
        text.lines().take(4).collect::<Vec<_>>().join(" ⏎ ")
    );
    assert!(
        text.contains("招标文件"),
        "中文锚点「招标文件」须识别（得：{text:?}）"
    );
    assert!(
        text.contains("验收标准") || text.contains("总体要求"),
        "中文锚点（第二章标题）须识别"
    );
    assert!(
        text.to_lowercase().contains("specification"),
        "混合模式下英文锚点须识别"
    );
}

#[test]
fn text_pdf_ocr_sanity() {
    setup_tessdata();
    let env = ocr_available();
    if !env.binary_ok || !env.has_lang("eng") {
        eprintln!("[pdff4] tesseract/eng 缺失 — SKIP");
        return;
    }
    let text = ocr_page(&fixture_bytes("small.pdf"), 0, "eng").unwrap();
    eprintln!(
        "[pdff4] small.pdf OCR：{:?}",
        text.lines().take(2).collect::<Vec<_>>()
    );
    assert!(
        text.to_lowercase().contains("spike"),
        "small.pdf 页 0 的 'Spike' 须识别——渲染→识别全链健康度"
    );
}

#[test]
fn ocr_env_probe_semantics() {
    setup_tessdata();
    let env = ocr_available();
    eprintln!(
        "[pdff4] OCR 环境：binary={} langs={:?}",
        env.binary_ok, env.languages
    );
    if !env.binary_ok {
        eprintln!("[pdff4] tesseract 缺失 — 探针语义按缺席面断言");
        assert!(env.languages.is_empty());
        assert!(!env.has_lang("eng"));
        return;
    }
    // 沙盒实测面：eng 系统包恒在（测试环境合同）；chi_sim 经注入目录。
    assert!(env.has_lang("eng"));
    assert!(!env.has_lang("nonexistent_lang"));
}

#[test]
fn missing_lang_typed_error() {
    setup_tessdata();
    let env = ocr_available();
    if !env.binary_ok {
        eprintln!("[pdff4] tesseract 缺失 — SKIP");
        return;
    }
    let scanned = fixture_bytes("scanned_mixed.pdf");
    // 缺失语言包 → tesseract 退出非零 → typed Engine（产品面不静默）。
    match ocr_page(&scanned, 0, "klingon_fake") {
        Err(CommandError::Engine(msg)) => {
            assert!(
                msg.contains("识别失败") || msg.contains("tesseract"),
                "{msg}"
            )
        }
        other => panic!("缺失语言包应 Engine 错误，得 {other:?}"),
    }
    // 页越界 → InvalidPage。
    match ocr_page(&scanned, 99, "eng") {
        Err(CommandError::InvalidPage(PageIndex(99))) => {}
        other => panic!("页越界应 InvalidPage(99)，得 {other:?}"),
    }
}
