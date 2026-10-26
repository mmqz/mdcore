// G1 LO 实开探针（issue #6）：TOC 域 docx → LibreOffice headless PDF
// 转换存活验证（跨层证据——产物落 /tmp/g1/）。
use mdc_office_io::docx::{DocxWriter, RunSpec};

#[test]
#[ignore = "探针：需 LO 环境显式跑"]
fn g1_lo_emit_fixture() {
    let mut w = DocxWriter::new();
    w.paragraph(&[RunSpec::field_toc_heading_range(1, 3)]);
    w.heading(1, "第一章 总体方案").unwrap();
    w.paragraph_text("章内容。");
    w.heading(2, "1.1 分项说明").unwrap();
    w.paragraph_text("节内容。");
    w.heading(1, "第二章 报价构成").unwrap();
    w.paragraph_text("章内容二。");
    std::fs::create_dir_all("/tmp/g1").unwrap();
    w.save_to_path("/tmp/g1/toc.docx").unwrap();
    println!("fixture -> /tmp/g1/toc.docx");
}
