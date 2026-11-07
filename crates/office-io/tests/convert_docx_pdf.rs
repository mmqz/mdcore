//! #307 阶段①：DOCX → PDF 导出桥真 LO 验证（集成面）。
//!
//! 夹具 = docx-rs 现场构造（本 crate 测试惯例——读面同源上游模型，
//! 见 docx_nested_tables.rs / docx_merged_cells.rs 头注释）。构造面
//! 覆盖「质量验收」关心的三形态：段落文本、多行表格（含中文）、
//! 加粗 run（字体面 LO 侧继承默认字体）。
//!
//! 环境门：无 soffice → 显式 SKIP 横幅（不伪绿——AGENTS.md §13.1
//! 纪律；CI 无 LO 时本测试让位，本机有 LO 才真跑）。
//!
//! 深度质量面（PDF 文本抽取对照源文档）在 mang-shell 侧集成测试
//! （那边同时依赖 office-io 与 mdc-pdf，才是产品真实消费链）。

use mdc_office_io::convert::{detect_libreoffice, docx_to_pdf, ExportPdfError};

fn text_cell(text: &str) -> docx_rs::TableCell {
    docx_rs::TableCell::new()
        .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)))
}

/// 段落 + 中文表格 + 加粗 run 的综合夹具（导出质量三形态）。
fn fixture_docx() -> Vec<u8> {
    let table = docx_rs::Table::new(vec![])
        .set_grid(vec![2000, 2000, 2000])
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("项目"),
            text_cell("数量"),
            text_cell("单价"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("超融合节点"),
            text_cell("12"),
            text_cell("85,000"),
        ]))
        .add_row(docx_rs::TableRow::new(vec![
            text_cell("核心交换机"),
            text_cell("2"),
            text_cell("120,000"),
        ]));
    let built = docx_rs::Docx::new()
        .add_paragraph(
            docx_rs::Paragraph::new().add_run(
                docx_rs::Run::new()
                    .add_text("mdc-convert 集成夹具：段落文本（导出质量验证用）。")
                    .bold(),
            ),
        )
        .add_table(table)
        .add_paragraph(
            docx_rs::Paragraph::new()
                .add_run(docx_rs::Run::new().add_text("尾段：确认表格与文本双形态承载。")),
        )
        .build();
    let mut buf = std::io::Cursor::new(Vec::new());
    built.pack(&mut buf).expect("docx-rs pack");
    buf.into_inner()
}

#[test]
fn docx_to_pdf_real_lo() {
    let Some(lo) = detect_libreoffice() else {
        eprintln!(
            "SKIP: docx_to_pdf_real_lo —— 本机无 LibreOffice（探测为空；指引见 ExportPdfError::NoLibreOffice）"
        );
        return;
    };
    eprintln!(
        "[convert] LO 在位：{}（解析版本 {:?}）",
        lo.version_line, lo.version
    );

    let docx = fixture_docx();
    let pdf = docx_to_pdf(&docx)
        .unwrap_or_else(|e| panic!("LO 在位但转换失败：{e}（指引：{}）", e.guidance()));

    // ① PDF 容器魔数。
    assert_eq!(&pdf[..4], b"%PDF", "产物非 PDF 容器");
    // ② 非平凡体积（三行表格 + 三段落 → 远大于空 PDF 的 ~800B）。
    assert!(
        pdf.len() > 4_000,
        "PDF 过小（{} B）——内容疑似丢失",
        pdf.len()
    );
    // ③ PDF 尾部结构（EOF 标记——LO writer_pdf_Export 恒有）。
    let tail = &pdf[pdf.len().saturating_sub(2048)..];
    assert!(
        tail.windows(5).any(|w| w == b"%%EOF"),
        "PDF 缺 %%EOF 尾标——产物不完整"
    );
    eprintln!(
        "[convert] OK：{} 字节 PDF（魔数/体积/尾标三验过）",
        pdf.len()
    );
}

/// 无 LO 路径：PATH 掏空 + 未知安装位 → 探测为空 → NoLibreOffice
/// （分类错误 + 指引文案——「不静默失败」的负路径钉定）。
/// 注：本机装了 LO 时以受控 PATH 环境模拟；候选安装位因绝对路径
/// 不可掏空，仅在「本机探测本来为空」时跑（装了 LO 的开发机 SKIP）。
#[test]
fn no_libreoffice_yields_actionable_error() {
    if detect_libreoffice().is_some() {
        eprintln!(
            "SKIP: no_libreoffice_yields_actionable_error —— 本机 LO 在位（负路径由 CI 无 LO 环境覆盖）"
        );
        return;
    }
    let err = docx_to_pdf(&fixture_docx()).expect_err("无 LO 环境应失败");
    assert!(matches!(err, ExportPdfError::NoLibreOffice));
    assert!(err.guidance().contains("libreoffice.org") || err.guidance().contains("apt"));
}
