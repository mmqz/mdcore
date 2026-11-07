//! #307 ②功能独立项：PDF→DOCX 文本版导出桥——产品链路质量门。
//!
//! 与 `crates/office-io/tests/pdf_import_docx.rs`（纯模型面——启发式
//! 单测 + 往返 + 分页符结构断言，零外部依赖）分层：本测试走**组合根
//! 真实消费链**——夹具 PDF（`tests/fixtures/smoke.pdf`，mdc-pdf 写面
//! 生成的多页中文 Type0/CID 档）→ `pdf_export_docx::pdf_to_docx_bytes`
//! （pdfium 引擎提取 + office-io 写面构造）→ `read_docx_from_bytes`
//! 读回对照。
//!
//! 独立 oracle 语义（与 `convert_quality.rs` 同族）：不问「转出来了没」，
//! 问**我们自己的 DOCX 读面**「读得回内容吗」+ 页序分页符计数。
//!
//! 环境门：无 libpdfium（env `MDC_PDFIUM_LIB` / 引擎候选清单全缺席）→
//! 显式 SKIP 横幅（不伪绿）。**CI test-shell job 不装 pdfium——本测试
//! 在 CI 面恒 SKIP，只在 pdfium 在场环境真跑**（与 convert_quality 的
//! LibreOffice 门同族先例；主 CI job 的 mdc-pdf 引擎面已有独立覆盖）。

use std::path::PathBuf;

use mang_shell::pdf_export_docx::pdf_to_docx_bytes;
use mdc_office_io::docx::{Block, read_docx_from_bytes};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::page::{PageIndex, PageSpan};

/// libpdfium 在场性（env → 候选清单——引擎定位序同源）。
fn pdfium_available() -> bool {
    if std::env::var_os("MDC_PDFIUM_LIB").is_some_and(|v| !v.is_empty()) {
        return true;
    }
    PdfiumEngine::library_candidates()
        .iter()
        .any(|p| p.exists())
}

/// 极简 zip 条目扫描（与 docx_bid_e2e/docx_edit_e2e 同口径——独立于
/// 被测代码，不引 zip dev-dep）。本测试只消费 `word/document.xml`。
fn zip_entry(bytes: &[u8], wanted: &str) -> Vec<u8> {
    let n = bytes.len();
    let mut eocd = None;
    let mut i = n.saturating_sub(22);
    loop {
        if bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            eocd = Some(i);
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    let eocd = eocd.expect("EOCD");
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
    let cd_off = u32::from_le_bytes([
        bytes[eocd + 16],
        bytes[eocd + 17],
        bytes[eocd + 18],
        bytes[eocd + 19],
    ]) as usize;
    let mut p = cd_off;
    for _ in 0..count {
        assert_eq!(&bytes[p..p + 4], &[0x50, 0x4b, 0x01, 0x02], "CD header");
        let method = u16::from_le_bytes([bytes[p + 10], bytes[p + 11]]);
        let csize = u32::from_le_bytes([bytes[p + 20], bytes[p + 21], bytes[p + 22], bytes[p + 23]])
            as usize;
        let name_len = u16::from_le_bytes([bytes[p + 28], bytes[p + 29]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[p + 30], bytes[p + 31]]) as usize;
        let comm_len = u16::from_le_bytes([bytes[p + 32], bytes[p + 33]]) as usize;
        let lho = u32::from_le_bytes([bytes[p + 42], bytes[p + 43], bytes[p + 44], bytes[p + 45]])
            as usize;
        let name = String::from_utf8_lossy(&bytes[p + 46..p + 46 + name_len]).into_owned();
        let lname_len = u16::from_le_bytes([bytes[lho + 26], bytes[lho + 27]]) as usize;
        let lextra_len = u16::from_le_bytes([bytes[lho + 28], bytes[lho + 29]]) as usize;
        let data_start = lho + 30 + lname_len + lextra_len;
        let data = &bytes[data_start..data_start + csize];
        if name == wanted {
            if method == 0 {
                return data.to_vec();
            }
            use std::io::Read as _;
            let mut decoder = flate2::read::DeflateDecoder::new(data);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).unwrap();
            return out;
        }
        p += 46 + name_len + extra_len + comm_len;
    }
    panic!("条目缺席：{wanted}");
}

/// 解出 document.xml（zip 级——分页符结构断言面）。
fn document_xml(bytes: &[u8]) -> String {
    String::from_utf8(zip_entry(bytes, "word/document.xml")).expect("document.xml utf-8")
}

#[test]
fn pdf_to_docx_text_quality_via_product_docx_reader() {
    if !pdfium_available() {
        eprintln!(
            "SKIP: pdf_to_docx_text_quality_via_product_docx_reader —— 本机无 libpdfium \
             （定位序 = env MDC_PDFIUM_LIB → 候选清单；部署自检 = mang-shell --check-pdfium）"
        );
        return;
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pdf_path = manifest.join("tests/fixtures/smoke.pdf");
    if !pdf_path.exists() {
        eprintln!(
            "SKIP: pdf_to_docx_text_quality_via_product_docx_reader —— 夹具缺席：{}",
            pdf_path.display()
        );
        return;
    }
    let pdf = std::fs::read(&pdf_path).expect("smoke.pdf 读取");

    // 页数基准（引擎面——分页符计数断言的分母）。
    let engine = PdfiumEngine::open(pdf.clone()).expect("引擎打开（夹具自校验）");
    let page_count = engine.page_count();
    drop(engine);

    // 组合根全链：提取 → 段落启发式 → DOCX 写面。
    let docx = pdf_to_docx_bytes(&pdf).expect("文本版导出（引擎提取 + 写面构造）");

    // ① 容器级：产品 DOCX 读面可解析。
    let doc = read_docx_from_bytes(&docx).expect("产品读面读回导出产物");

    // ② 文本在场：非空白段落拼接全文，关键内容片段（章节标题——生成器
    //    单行发射，提取面单行保形）逐个在场。
    let texts: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => {
                let t = p.text();
                if t.trim().is_empty() { None } else { Some(t) }
            }
            _ => None,
        })
        .collect();
    let joined: String = texts.concat();
    assert!(
        joined.chars().count() > 400,
        "提取文本量级异常（{} 字符——夹具为多页中文档）",
        joined.chars().count()
    );
    for heading in [
        "智慧园区综合管理平台投标文件",
        "第一章项目概述",
        "第五章服务承诺",
    ] {
        assert!(joined.contains(heading), "关键内容缺席「{heading}」");
    }

    // ③ 页序：分页符计数 = 引擎页数 − 1（页位 1:1 保持）。
    assert_eq!(
        document_xml(&docx).matches(r#"w:type="page""#).count(),
        (page_count - 1) as usize,
        "页间分页符计数应 = 页数-1（引擎页数 {page_count}）"
    );
}

/// 无文本层（扫描件面）：组合根显式拒（typed NoTextLayer——不产出
/// 空壳 docx；OCR 接线是 #306 的活）。夹具 = mdc-pdf 仓真实扫描件
/// 语料（gen_scanned_fixture.py 产物，1 页 0 文本——探针实证）。
#[test]
fn scanned_pdf_without_text_layer_is_rejected_typed() {
    if !pdfium_available() {
        eprintln!("SKIP: scanned_pdf_without_text_layer_is_rejected_typed —— 本机无 libpdfium");
        return;
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pdf_path = manifest.join("../crates/pdf/fixtures/scanned_mixed.pdf");
    if !pdf_path.exists() {
        eprintln!(
            "SKIP: scanned_pdf_without_text_layer_is_rejected_typed —— 夹具缺席：{}",
            pdf_path.display()
        );
        return;
    }
    let pdf = std::fs::read(&pdf_path).expect("scanned_mixed.pdf 读取");
    match pdf_to_docx_bytes(&pdf) {
        Err(mang_shell::pdf_export_docx::PdfToDocxError::NoTextLayer) => {}
        Err(other) => panic!("扫描件应 NoTextLayer 拒（OCR 面 #306），得：{other}"),
        Ok(_) => panic!("无文本层 PDF 不应产出 DOCX（空壳纪律）"),
    }
}

/// 引擎页数一致性辅助（组合根契约断言——extract_text 载荷页数与引擎
/// page_count 一致）。
#[test]
fn engine_extract_payload_shape_matches_page_count() {
    if !pdfium_available() {
        eprintln!("SKIP: engine_extract_payload_shape_matches_page_count —— 本机无 libpdfium");
        return;
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pdf_path = manifest.join("tests/fixtures/smoke.pdf");
    if !pdf_path.exists() {
        eprintln!("SKIP: engine_extract_payload_shape_matches_page_count —— 夹具缺席");
        return;
    }
    let pdf = std::fs::read(&pdf_path).expect("smoke.pdf 读取");
    let engine = PdfiumEngine::open(pdf).expect("引擎打开");
    let count = engine.page_count();
    let span = PageSpan::new(PageIndex(0), PageIndex(count - 1)).expect("页区间");
    let payload = engine.extract_text(span).expect("提取");
    assert_eq!(payload.len() as u32, count, "载荷页数应与引擎页数一致");
}
