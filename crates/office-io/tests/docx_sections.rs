//! #257：多节分节符读写面（office-io）——读面 P1-1 修复 + 写面
//! `section_break` + 编辑面幸存回归。
//!
//! # 测试面
//!
//! | 面 | 用例 | 断言口径 |
//! |---|---|---|
//! | 写→读回环 | `section_break_write_read_roundtrip` | 封面/正文双节：节数/来源/几何/titlePg/起始页码/槽位/块区间连续切分 |
//! | 单节基线 | `single_section_document_has_exactly_one_final_section` | 恒 ≥1 条末节（新字段不破单节文档） |
//! | 真实语料（docx4j） | `corpus_docx4j_mid_section_slots_recovered` | **P1-1 钉面**：中段节 3 页眉 + 3 页脚此前静默丢，现在全量可读；末节行为零变化（文档级 = 末节） |
//! | 真实语料（POI） | `corpus_poi_three_sections_each_with_own_header` | 3 节各自 default 页眉（Section 1/2/3——前两节此前丢） |
//! | 编辑面 | `edit_preserves_sections` | 手术编辑（改文本）→ sectPr 字节不动 → 节结构不变 |
//! | 校验门 | `section_break_geometry_validation` | 几何越域 = 粘性拒绝（save 漏斗 InvalidInput，零产物） |
//!
//! 语料门（缺件 SKIP 显式横幅——非 PASS 伪装）：docx4j
//! `headers_footers_sections_multiple.docx` / POI `Headers.docx`
//! （`test-artifacts/health-check/corpus/`——本测试直接点名子目录路径，
//! 不经 style_corpus 的顶层扫描）。

use mdc_office_io::docx::{
    read_docx_from_bytes, DocxWriter, HeaderFooterVariant, PageGeometry, RunSpec, SectionBreakSpec,
    SectionData, SectionSlots, SectionSource,
};
use mdc_office_io::docx_edit::DocxEditor;

// ---------------------------------------------------------------- 工具面

/// 槽位文本拼接（变体过滤——None 槽位不存在）。
fn slot_text(
    slots: &[mdc_office_io::docx::HeaderFooterData],
    variant: HeaderFooterVariant,
) -> Option<String> {
    slots.iter().find(|s| s.variant == variant).map(|s| {
        s.blocks
            .iter()
            .filter_map(|b| match b {
                mdc_office_io::docx::Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// 节断言工具：块区间连续切分（节 i end = 节 i+1 start；末节 end =
/// blocks.len()）。
fn assert_partition(sections: &[SectionData], block_count: usize) {
    assert!(
        !sections.is_empty(),
        "sections 恒 ≥1 条（单节文档 = 1 条末节）"
    );
    assert_eq!(sections[0].block_range.0, 0, "首节起点 = 0");
    for w in sections.windows(2) {
        assert_eq!(
            w[0].block_range.1, w[1].block_range.0,
            "节 {} end = 节 {} start（连续切分）",
            w[0].index, w[1].index
        );
    }
    assert_eq!(
        sections.last().unwrap().block_range.1,
        block_count,
        "末节 end = blocks.len()"
    );
    for (i, s) in sections.iter().enumerate() {
        assert_eq!(s.index, i, "节序 = 文档序");
    }
}

/// 语料路径（直接点名子目录——style_corpus 顶层扫描不覆盖
/// poi-docx/docx4j 子目录，见审计 P1-3 登记）。
fn corpus_path(rel: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-artifacts/health-check/corpus")
        .join(rel)
}

/// 语料门（缺件 SKIP 横幅——非 PASS 伪装）。
fn gated_bytes(rel: &str, test_id: &str) -> Option<Vec<u8>> {
    let p = corpus_path(rel);
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!(
                "[SKIP] {test_id}: corpus fixture absent ({}: {e}) — 真实语料面未验证",
                p.display()
            );
            None
        }
    }
}

// ---------------------------------------------------------------- 写→读回环

/// 封面/正文异构双节：封面（titlePg、无页眉页脚）+ 正文（页眉/页脚/
/// 页码域/自定义边距/起始页码 1）——投标文件典型形态。
#[test]
fn section_break_write_read_roundtrip() {
    let mut w = DocxWriter::new();
    // 封面节内容。
    w.paragraph_text("投标文件封面");
    w.paragraph_text("项目名称：多节分节符验证");
    // 闭合一节（封面：titlePg、无槽位、A4 默认几何）。
    w.section_break(SectionBreakSpec {
        geometry: None,
        title_page: true,
        page_number_start: None,
        section_type: None,
        columns: None,
        headers: SectionSlots::default(),
        footers: SectionSlots::default(),
    });
    // 正文节内容。
    w.paragraph_text("第一章 技术方案");
    w.paragraph_text("详见下文。");
    // 末节属性（正文：异构边距 + 页眉 + 页脚页码域 + 起始页码 1）。
    w.page_geometry(PageGeometry {
        width: 11906,
        height: 16838,
        margin_top: 2500,
        margin_right: 1200,
        margin_bottom: 2500,
        margin_left: 1500,
        margin_header: 851,
        margin_footer: 992,
        gutter: 0,
    });
    w.page_number_start(1);
    w.header(&[RunSpec::text("投标人：测试公司")]);
    w.footer(&[RunSpec::field_page()]);

    let bytes = w.save_to_vec().expect("双节 docx 生成");
    // OOXML 结构直检：两处 sectPr，其一为段落级（<w:pPr><w:sectPr），
    // 封面节 titlePg 在场。
    {
        use std::io::Read;
        let mut za = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut xml = String::new();
        za.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert_eq!(xml.matches("<w:sectPr").count(), 2, "恰好两节");
        assert!(
            xml.contains("<w:pPr><w:sectPr"),
            "中段节 = 段落级 sectPr（OOXML 语义）"
        );
        assert!(xml.contains("<w:titlePg"), "封面节 titlePg 在场");
        assert!(
            xml.contains("w:start=\"1\""),
            "正文节起始页码 1 在场（pgNumType）"
        );
    }

    let doc = read_docx_from_bytes(&bytes).expect("回读");
    assert_eq!(doc.sections.len(), 2, "双节");
    assert_partition(&doc.sections, doc.blocks.len());
    // 块序：封面 2 段 + 节断点段（承载 sectPr 的空段——诚实计数）+
    // 正文 2 段 = 5 块。
    assert_eq!(doc.blocks.len(), 5);
    assert_eq!(doc.sections[0].block_range, (0, 3), "封面节区间");
    assert_eq!(doc.sections[1].block_range, (3, 5), "正文节区间");

    // 封面节。
    let cover = &doc.sections[0];
    assert_eq!(cover.source, SectionSource::Mid);
    assert!(cover.title_page, "封面 titlePg");
    assert!(cover.headers.is_empty(), "封面无页眉槽位");
    assert!(cover.footers.is_empty(), "封面无页脚槽位");
    assert_eq!(cover.geometry, PageGeometry::a4_default(), "封面默认几何");

    // 正文节。
    let body = &doc.sections[1];
    assert_eq!(body.source, SectionSource::Final);
    assert_eq!(body.page_number_start, Some(1), "正文起始页码 1");
    assert_eq!(body.geometry.margin_top, 2500, "正文异构上边距");
    assert_eq!(body.geometry.margin_left, 1500, "正文异构左边距");
    let header_text =
        slot_text(&body.headers, HeaderFooterVariant::Default).expect("正文默认页眉槽位在场");
    assert_eq!(header_text, "投标人：测试公司");
    // 页脚 = PAGE 域（volatile 域缓存结果被 FieldMachine 剥离——
    // 槽位在场 + 域指令不入正文，结果计数不在此面断言）。
    assert!(
        slot_text(&body.footers, HeaderFooterVariant::Default).is_some(),
        "正文默认页脚槽位在场"
    );
    // 末节槽位 = 文档级（既有单一真源不复制漂移——语义一致性）。
    assert_eq!(doc.headers.len(), body.headers.len());
}

/// 单节文档：恰 1 条末节（新字段不破既有文档形态）。
#[test]
fn single_section_document_has_exactly_one_final_section() {
    let mut w = DocxWriter::new();
    w.paragraph_text("只有一节");
    let bytes = w.save_to_vec().unwrap();
    let doc = read_docx_from_bytes(&bytes).unwrap();
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].source, SectionSource::Final);
    assert!(!doc.sections[0].title_page);
    assert_eq!(doc.sections[0].block_range, (0, doc.blocks.len()));
    assert_partition(&doc.sections, doc.blocks.len());
}

// ---------------------------------------------------------------- 真实语料

/// docx4j `headers_footers_sections_multiple.docx`——**P1-1 钉面**：
/// 中段节（第 1 节）6 槽位全挂接（even/default/first × header/footer），
/// 此前 docx-rs 只联末节 → 中段节静默丢；引擎第二趟补齐后全量可读。
#[test]
fn corpus_docx4j_mid_section_slots_recovered() {
    let Some(bytes) = gated_bytes(
        "docx4j/headers_footers_sections_multiple.docx",
        "corpus_docx4j_mid_section_slots_recovered",
    ) else {
        return;
    };
    let doc = read_docx_from_bytes(&bytes).expect("语料导入（门禁已有基线）");
    assert_eq!(doc.sections.len(), 2, "两节（中段节 + 末节）");
    assert_partition(&doc.sections, doc.blocks.len());
    assert_eq!(doc.blocks.len(), 9, "9 段（语料已知形态）");
    assert_eq!(doc.sections[0].block_range, (0, 5), "第 1 节 = 段 1-5");
    assert_eq!(doc.sections[1].block_range, (5, 9), "第 2 节 = 段 6-9");

    // ---- P1-1 核心：中段节槽位全量恢复 ----
    let s1 = &doc.sections[0];
    assert_eq!(s1.source, SectionSource::Mid);
    assert!(s1.title_page, "第 1 节 titlePg（语料已知）");
    assert_eq!(
        s1.headers.len(),
        3,
        "第 1 节 3 页眉槽位（even/default/first）"
    );
    assert_eq!(s1.footers.len(), 3, "第 1 节 3 页脚槽位");
    let first = slot_text(&s1.headers, HeaderFooterVariant::First).expect("first 页眉槽位");
    assert!(
        first.contains("Section 1 – first page"),
        "first 页眉内容恢复（此前静默丢）：{first:?}"
    );
    let default = slot_text(&s1.headers, HeaderFooterVariant::Default).expect("default 页眉槽位");
    assert!(
        default.contains("Section 1 – subsequent page"),
        "default 页眉内容恢复：{default:?}"
    );
    assert!(
        slot_text(&s1.headers, HeaderFooterVariant::Even).is_some(),
        "even 页眉槽位在场（空部件也是挂接态）"
    );
    let fdef = slot_text(&s1.footers, HeaderFooterVariant::Default).expect("default 页脚槽位");
    assert!(
        fdef.contains("Just a footer."),
        "default 页脚内容恢复：{fdef:?}"
    );

    // ---- 末节行为零变化（文档级 = 末节，既有语义）----
    let s2 = &doc.sections[1];
    assert_eq!(s2.source, SectionSource::Final);
    assert_eq!(s2.headers.len(), 1, "第 2 节仅 default 页眉（语料已知）");
    let s2h = slot_text(&s2.headers, HeaderFooterVariant::Default).unwrap();
    assert!(s2h.contains("Section 2"), "末节页眉：{s2h:?}");
    // 文档级（旧 API）与末节一致——基线零漂移口径。
    assert_eq!(doc.headers.len(), 1);
    let doc_h = slot_text(&doc.headers, HeaderFooterVariant::Default).unwrap();
    assert!(
        doc_h.contains("Section 2"),
        "文档级页眉仍 = 末节：{doc_h:?}"
    );
}

/// POI `Headers.docx`——3 节各自 default 页眉（Section 1/2/3）：
/// 前两节此前静默丢，现全量可读。
#[test]
fn corpus_poi_three_sections_each_with_own_header() {
    let Some(bytes) = gated_bytes(
        "poi-docx/Headers.docx",
        "corpus_poi_three_sections_each_with_own_header",
    ) else {
        return;
    };
    let doc = read_docx_from_bytes(&bytes).expect("语料导入");
    assert_eq!(doc.sections.len(), 3, "三节");
    assert_partition(&doc.sections, doc.blocks.len());
    assert_eq!(doc.blocks.len(), 5, "5 段（语料已知形态）");
    assert_eq!(doc.sections[0].block_range, (0, 2));
    assert_eq!(doc.sections[1].block_range, (2, 4));
    assert_eq!(doc.sections[2].block_range, (4, 5));

    for (i, expect) in [(0usize, "Section 1"), (1, "Section 2"), (2, "Section 3")] {
        let h = slot_text(&doc.sections[i].headers, HeaderFooterVariant::Default)
            .unwrap_or_else(|| panic!("节 {} default 页眉槽位", i + 1));
        assert!(
            h.contains(expect),
            "节 {} 页眉 = {}（实际 {h:?}）",
            i + 1,
            expect,
            h = h
        );
    }
    // 文档级 = 末节（既有行为零变化）。
    let doc_h = slot_text(&doc.headers, HeaderFooterVariant::Default).unwrap();
    assert!(doc_h.contains("Section 3"), "文档级页眉仍 = 末节");
}

// ---------------------------------------------------------------- 编辑面

/// 手术编辑幸存：多节文档改文本 → sectPr 字节不动 → 节结构不变。
#[test]
fn edit_preserves_sections() {
    let mut w = DocxWriter::new();
    w.paragraph_text("封面段");
    w.section_break(SectionBreakSpec {
        title_page: true,
        ..Default::default()
    });
    w.paragraph_text("正文段甲");
    w.paragraph_text("正文段乙");
    w.page_geometry(PageGeometry {
        margin_top: 2500,
        ..PageGeometry::a4_default()
    });
    w.page_number_start(1);
    w.header(&[RunSpec::text("正文页眉")]);
    let bytes = w.save_to_vec().unwrap();

    let before = read_docx_from_bytes(&bytes).unwrap();
    assert_eq!(before.sections.len(), 2);

    // 手术编辑：改正文段（跨节编辑面）。
    let mut ed = DocxEditor::open(&bytes).unwrap();
    // 段序：0=封面段、1=节断点段（空段）、2=正文段甲、3=正文段乙。
    ed.set_paragraph_text(2, "正文段甲（已编辑）").unwrap();
    let edited = ed.save_to_vec().unwrap();

    let after = read_docx_from_bytes(&edited).unwrap();
    // 编辑生效。
    match &after.blocks[2] {
        mdc_office_io::docx::Block::Paragraph(p) => {
            assert_eq!(p.text(), "正文段甲（已编辑）")
        }
        other => panic!("块 2 应为段落：{other:?}"),
    }
    // 节结构不变（几何/来源/titlePg/起始页码/槽位/区间逐一对照）。
    assert_eq!(after.sections.len(), before.sections.len());
    for (a, b) in after.sections.iter().zip(before.sections.iter()) {
        assert_eq!(a.source, b.source, "来源不变");
        assert_eq!(a.geometry, b.geometry, "几何不变");
        assert_eq!(a.title_page, b.title_page, "titlePg 不变");
        assert_eq!(a.page_number_start, b.page_number_start, "起始页码不变");
        assert_eq!(a.block_range, b.block_range, "块区间不变");
        assert_eq!(a.headers.len(), b.headers.len(), "页眉槽位数不变");
        assert_eq!(a.footers.len(), b.footers.len(), "页脚槽位数不变");
    }
    assert_eq!(
        slot_text(&after.sections[1].headers, HeaderFooterVariant::Default),
        slot_text(&before.sections[1].headers, HeaderFooterVariant::Default),
        "末节页眉内容不变"
    );
}

// ---------------------------------------------------------------- 校验门

/// 几何越域 = 粘性拒绝（构造期落账，save 漏斗拒——零产物）。
#[test]
fn section_break_geometry_validation() {
    let mut w = DocxWriter::new();
    w.paragraph_text("段");
    w.section_break(SectionBreakSpec {
        geometry: Some(PageGeometry {
            width: 0, // 越域（pgSz 正值域 1-40000）
            ..PageGeometry::a4_default()
        }),
        ..Default::default()
    });
    let err = w.save_to_vec().expect_err("越域几何必须拒");
    match err {
        mdc_office_io::docx::DocxError::InvalidInput(msg) => {
            assert!(msg.contains("section_break"), "错误消息含面锚点：{msg}");
        }
        other => panic!("应为 InvalidInput（粘性校验），实际 {other:?}"),
    }
}

// ---------------------------------------------------------------- #268 分栏

/// #268 手搓最小 OPC 包（门 1-3 全绿形态——style.rs `build_test_docx`
/// 同族骨架）：document.xml 参数化（分栏节 + 显式分栏符）。
fn build_columns_docx(document_xml: &str) -> Vec<u8> {
    use std::io::Write as _;
    let ct = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
    let root_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
    let styles = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let add = |zw: &mut zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>>,
                   name: &str,
                   data: &[u8]|
         -> zip::result::ZipResult<()> {
            zw.start_file(name, zip::write::SimpleFileOptions::default())?;
            zw.write_all(data)?;
            Ok(())
        };
        add(&mut zw, "[Content_Types].xml", ct).unwrap();
        add(&mut zw, "_rels/.rels", root_rels).unwrap();
        add(&mut zw, "word/document.xml", document_xml.as_bytes()).unwrap();
        add(&mut zw, "word/_rels/document.xml.rels", doc_rels).unwrap();
        add(&mut zw, "word/styles.xml", styles).unwrap();
        zw.finish().unwrap();
    }
    buf.into_inner()
}

/// #268：分栏读面全链——cols 全字段（num/space/equalWidth/sep/w:col
/// 子表）+ 显式分栏符 + 文本流栏归属 + 越栏钳制 + 越界 None。
#[test]
fn columns_spec_breaks_and_attribution() {
    let doc_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>P1</w:t></w:r></w:p>
<w:p><w:r><w:t>before</w:t><w:br w:type="column"/><w:t>after</w:t></w:r></w:p>
<w:p><w:r><w:t>P3</w:t></w:r></w:p>
<w:p><w:r><w:t>P4</w:t></w:r><w:r><w:br w:type="column"/></w:r></w:p>
<w:p><w:r><w:t>P5</w:t></w:r></w:p>
<w:p><w:pPr><w:sectPr><w:cols w:num="3" w:space="708" w:equalWidth="false" w:sep="1"><w:col w:w="2669" w:space="708"/><w:col w:w="2315" w:space="708"/><w:col w:w="2671"/></w:cols></w:sectPr></w:pPr><w:r><w:t>sec1end</w:t></w:r></w:p>
<w:p><w:r><w:t>f1</w:t></w:r></w:p>
<w:p><w:r><w:br w:type="column"/></w:r></w:p>
<w:p><w:r><w:t>f2</w:t></w:r></w:p>
<w:p><w:r><w:br w:type="column"/></w:r></w:p>
<w:p><w:r><w:br w:type="column"/></w:r></w:p>
<w:p><w:r><w:t>f3</w:t></w:r></w:p>
<w:sectPr><w:cols w:num="2" w:space="720"/></w:sectPr>
</w:body></w:document>"#;
    let bytes = build_columns_docx(std::str::from_utf8(doc_xml).unwrap());
    let doc = read_docx_from_bytes(&bytes).expect("手搓档可读");
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    assert_eq!(doc.sections.len(), 2, "中段节 + 末节");

    // ---- 节 1：三栏全字段 ----
    let s1 = &doc.sections[0];
    assert_eq!(s1.columns, 3);
    let spec = s1.columns_spec.as_ref().expect("cols 在场");
    assert_eq!(spec.num, Some(3));
    assert_eq!(spec.space, Some(708));
    assert_eq!(spec.equal_width, Some(false));
    assert_eq!(spec.separator, Some(true));
    assert_eq!(
        spec.columns,
        vec![
            mdc_office_io::docx::ColumnWidth {
                width: Some(2669),
                space: Some(708)
            },
            mdc_office_io::docx::ColumnWidth {
                width: Some(2315),
                space: Some(708)
            },
            mdc_office_io::docx::ColumnWidth {
                width: Some(2671),
                space: None
            },
        ],
        "w:col 子表（文档序，缺省属性 None 不代填）"
    );
    // 显式分栏符块：b1（before/after 同块）+ b3。
    assert_eq!(s1.column_break_blocks, vec![1, 3]);
    // 栏归属（块起始栏口径）。
    assert_eq!(s1.column_of(0), Some(0), "P1：栏 0");
    assert_eq!(s1.column_of(1), Some(0), "含符块取起始栏（before 在栏 0）");
    assert_eq!(s1.column_of(2), Some(1), "P3：b1 符后 → 栏 1");
    assert_eq!(s1.column_of(3), Some(1), "P4：起始栏 1");
    assert_eq!(s1.column_of(4), Some(2), "P5：两符后 → 栏 2");
    assert_eq!(s1.column_of(5), Some(2), "sec1end 段：栏 2");
    assert_eq!(s1.column_of(6), None, "块不在本节 → None");

    // ---- 节 2（末节）：双栏 + 越栏钳制 ----
    let s2 = &doc.sections[1];
    assert_eq!(s2.columns, 2);
    let spec2 = s2.columns_spec.as_ref().expect("末节 cols 在场");
    assert_eq!(spec2.num, Some(2));
    assert_eq!(spec2.space, Some(720));
    assert_eq!(spec2.equal_width, None, "equalWidth 缺席不代填");
    assert!(spec2.columns.is_empty());
    // f2 = b8：一符后 → 栏 1；f3 = b11：三符后 → 钳制到栏 1（num=2）。
    assert_eq!(s2.column_break_blocks.len(), 3);
    assert_eq!(s2.column_of(6), Some(0), "f1：栏 0");
    assert_eq!(s2.column_of(8), Some(1), "f2：栏 1");
    assert_eq!(s2.column_of(11), Some(1), "f3：三符超栏数 → 钳制栏 1");
    // 单栏语义：无分栏符的节恒栏 0（显式钳制面在下方单栏测试）。

    // ---- 无 cols 的节：columns_spec None（缺省单栏形态） ----
    let plain = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>only</w:t></w:r></w:p>
<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>
</w:body></w:document>"#;
    let doc2 = read_docx_from_bytes(&build_columns_docx(std::str::from_utf8(plain).unwrap()))
        .expect("无 cols 档可读");
    assert_eq!(doc2.sections.len(), 1);
    let sp = &doc2.sections[0];
    assert_eq!(sp.columns, 1, "无 cols = 单栏缺省");
    assert!(sp.columns_spec.is_none(), "cols 元素缺席 → None");
    assert!(sp.column_break_blocks.is_empty());
    assert_eq!(sp.column_of(0), Some(0), "单栏恒栏 0");
    // 分栏符在单栏节 = 排版期 no-op，归属仍恒 0（钳制到 num-1=0）。
    let single_break = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:br w:type="column"/></w:r></w:p>
<w:p><w:r><w:t>after</w:t></w:r></w:p>
<w:sectPr><w:cols w:num="1"/></w:sectPr>
</w:body></w:document>"#;
    let doc3 = read_docx_from_bytes(&build_columns_docx(
        std::str::from_utf8(single_break).unwrap(),
    ))
    .expect("单栏分栏符档可读");
    let s0 = &doc3.sections[0];
    assert_eq!(s0.column_break_blocks, vec![0]);
    assert_eq!(s0.column_of(1), Some(0), "单栏 + 符 → 钳制栏 0");
}
