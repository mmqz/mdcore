// G2 LO 实开探针（issue #6）：自定义页面几何+排版 docx → LO PDF
// 字体/页宽实测（跨层证据——产物落 /tmp/g2/）。
use mdc_office_io::docx::{
    DefaultTypography, DocxWriter, LineRuleSpec, LineSpacingSpec, PageGeometry,
};

#[test]
#[ignore = "探针：需 LO 环境显式跑"]
fn g2_lo_emit_fixture() {
    let mut w = DocxWriter::new();
    w.page_geometry(PageGeometry {
        width: 11906,
        height: 16838,
        margin_top: 1440,
        margin_right: 1800,
        margin_bottom: 1440,
        margin_left: 1800,
        margin_header: 851,
        margin_footer: 992,
        gutter: 0,
    });
    w.default_typography(DefaultTypography {
        ascii_font: "Times New Roman".into(),
        east_asia_font: "Noto Sans CJK SC".into(),
        complex_script_font: "Times New Roman".into(),
        size_half_points: 28,
        line_spacing: Some(LineSpacingSpec {
            line: Some(360),
            line_rule: Some(LineRuleSpec::Auto),
            before: Some(120),
            after: Some(120),
        }),
    });
    w.heading(1, "G2 排版验证").unwrap();
    for i in 1..=30 {
        w.paragraph_text(&format!(
            "第 {i} 段：自定义四号字 1.5 倍行距左右 1800 twips 边距验证文本。"
        ));
    }
    std::fs::create_dir_all("/tmp/g2").unwrap();
    w.save_to_path("/tmp/g2/geom.docx").unwrap();
    println!("fixture -> /tmp/g2/geom.docx");
}
