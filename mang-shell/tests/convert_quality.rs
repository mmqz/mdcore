//! #307 阶段①：DOCX → PDF 导出质量门（产品消费链真验证）。
//!
//! 与 `crates/office-io/tests/convert_docx_pdf.rs`（引擎面：魔数/体积/
//! 尾标）分层：本测试走**产品真实消费链**——office-io 转换 → **mdc-pdf
//! （产品自身 PDF 读面）**打开并抽取文本 → 与源 DOCX 文本对照。
//!
//! 独立 oracle 语义：不问 LO「你觉得转好了吗」，而是问**我们自己的
//! PDF 引擎**「读得回内容吗」——读不回 = 转换质量不合格（表格文本
//! 丢失/字体乱码都会在此暴露）。
//!
//! 环境门：无 soffice → 显式 SKIP 横幅（不伪绿）。

use mdc_office_io::convert::{detect_libreoffice, docx_to_pdf};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::page::{PageIndex, PageSpan};

fn text_cell(text: &str) -> docx_rs::TableCell {
    docx_rs::TableCell::new()
        .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)))
}

/// 源文档三形态：段落（中英混排）+ 表格（含中文单元格）+ 加粗 run。
fn fixture_docx() -> Vec<u8> {
    let table = docx_rs::Table::new(vec![])
        .set_grid(vec![2200, 1800, 1800])
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("设备名称"),
            text_cell("数量"),
            text_cell("单价（元）"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("超融合一体机"),
            text_cell("12"),
            text_cell("85000"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("核心交换机"),
            text_cell("2"),
            text_cell("120000"),
        ]));
    let built = docx_rs::Docx::new()
        .add_paragraph(
            docx_rs::Paragraph::new().add_run(
                docx_rs::Run::new()
                    .add_text("Export quality probe: paragraph text (mixed 中英文).")
                    .bold(),
            ),
        )
        .add_table(table)
        .add_paragraph(docx_rs::Paragraph::new().add_run(
            docx_rs::Run::new().add_text("Trailing paragraph for structure completeness."),
        ))
        .build();
    let mut buf = std::io::Cursor::new(Vec::new());
    built.pack(&mut buf).expect("docx-rs pack");
    buf.into_inner()
}

#[test]
fn docx_to_pdf_quality_via_product_pdf_reader() {
    if detect_libreoffice().is_none() {
        eprintln!(
            "SKIP: docx_to_pdf_quality_via_product_pdf_reader —— 本机无 LibreOffice（office-io convert 指引见 ExportPdfError::NoLibreOffice）"
        );
        return;
    }

    let pdf_bytes = docx_to_pdf(&fixture_docx())
        .unwrap_or_else(|e| panic!("转换失败：{e}（指引：{}）", e.guidance()));

    // 产品 PDF 读面作独立 oracle：与桌面打开 PDF 同一入口
    //（`PdfiumEngine::open` 统一绑定 + `extract_text` 文本层）。
    let engine = PdfiumEngine::open(pdf_bytes)
        .expect("mdc-pdf 打开转换产物（pdfium 定位序 = bin/lib 取件位或系统装载）");
    let count = engine.page_count();
    assert!(count >= 1, "转换产物无页面");
    let span = PageSpan::new(PageIndex(0), PageIndex(count - 1)).expect("页区间构造");
    let texts = engine
        .extract_text(span)
        .expect("pdf 文本层抽取（与桌面读路径同 API）");
    let full_text: String = texts.into_iter().map(|(_, t)| t).collect();
    eprintln!("[quality] 抽取文本 {} 字符（{count} 页）", full_text.len());

    // 源文本关键锚点逐项对照：段落（中英混排）、表头、数据单元格、
    // 加粗 run 文本、尾段。全部在场 = 三形态承载完整。
    for anchor in [
        "Export quality probe",
        "mixed",
        "paragraph text",
        "设备名称",
        "超融合一体机",
        "85000",
        "核心交换机",
        "120000",
        "Trailing paragraph",
    ] {
        assert!(
            full_text.contains(anchor),
            "转换产物文本丢失锚点「{anchor}」（抽取文本 {} 字符）",
            full_text.len()
        );
    }
    eprintln!("[quality] 9/9 文本锚点全部读回（表格/中英文/加粗承载完整）");
}
