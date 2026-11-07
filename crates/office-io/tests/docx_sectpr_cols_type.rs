//! #423：sectPr 分栏与分节类型写面（office-io）——w:cols 全字段 +
//! w:type。
//!
//! # 测试面
//!
//! | 面 | 用例 | 断言口径 |
//! |---|---|---|
//! | 中段节 roundtrip | `mid_section_type_and_columns_roundtrip` | continuous + 三栏：读回 `SectionData::section_type`/`columns_spec` 全字段一致；XML 注入形态直检（cols 置换 + type 在 titlePg 前） |
//! | 末节分栏 exotic | `final_columns_exotic_roundtrip` | equalWidth=0 + sep + `w:col` 子表（docx-rs 零表达面）：读回逐字段一致（None 不代填） |
//! | 末节类型原生 | `document_level_section_type_native` | `DocxWriter::section_type`：docx-rs 原生发射 `<w:type w:val="…" />` + 读回 |
//! | None 语义保真 | `all_none_columns_spec_roundtrips_as_absent_attrs` | 全缺省 spec → `<w:cols />` 属性缺席 → 读回全 None（原生通道不可表达的保真面） |
//! | 账本定靶 | `multi_section_ledger_targeting` | 三节混合（中段 0 有增补 / 中段 1 无 / 末节有）：注入只打目标节，无增补节原样透传 |
//! | 字节级零变化 | `no_face_leaves_native_emission_untouched` | 不用本面：document.xml 保持引擎缺省 `<w:cols w:space="425" w:num="1" />`、零 `<w:type`（结构性短路 + 形态直检） |
//! | 校验门 | `columns_domain_validation_sticky` | num=0/num=64/space 越域/equalWidth=0 栏数不一致/col 宽 0 → save 漏斗 InvalidInput 零产出 + 粘性（二次 save 恒拒） |
//! | 覆盖语义 | `columns_overwrite_single_value` | 文档级 columns 两调 → 后值覆盖（末节单值语义） |
//!
//! 关联读面测试：`docx_sections.rs`（#257 节结构）/ 同文件
//! `columns_spec_breaks_and_attribution`（#268 读面全字段）。

use mdc_office_io::docx::{
    docx_part_bytes, read_docx_from_bytes, ColumnWidth, ColumnsSpec, DocxWriter, SectionBreakSpec,
    SectionKind,
};

// ---------------------------------------------------------------- 工具面

/// 提取 document.xml 文本（zip 单部件直读）。
fn document_xml(bytes: &[u8]) -> String {
    let raw = docx_part_bytes(bytes, "word/document.xml").expect("document.xml 在场");
    String::from_utf8(raw).expect("UTF-8")
}

/// 三栏等宽 spec（num + space 简形）。
fn three_equal_columns() -> ColumnsSpec {
    ColumnsSpec {
        num: Some(3),
        space: Some(425),
        ..ColumnsSpec::default()
    }
}

// ---------------------------------------------------------------- 测试面

/// 中段节 continuous + 三栏：写 → 读全字段 roundtrip + XML 注入形态
/// 直检（cols 置换、type 插入位在 titlePg 前——docx-rs 自身发射序
/// 同位）。
#[test]
fn mid_section_type_and_columns_roundtrip() {
    let mut w = DocxWriter::new();
    w.paragraph_text("第一节第一段");
    w.section_break(SectionBreakSpec {
        section_type: Some(SectionKind::Continuous),
        columns: Some(three_equal_columns()),
        title_page: true,
        ..Default::default()
    });
    w.paragraph_text("第二节第一段");
    let bytes = w.save_to_vec().expect("双节生成");

    // XML 直检：注入形态（置换后的 cols + type 位）。
    let xml = document_xml(&bytes);
    assert!(
        xml.contains("<w:cols w:num=\"3\" w:space=\"425\" />"),
        "cols 置换为注入形态：{xml}"
    );
    // type 插入位 = titlePg 前（docx-rs 发射序同位）。
    let type_pos = xml
        .find("<w:type w:val=\"continuous\" />")
        .expect("type 在场");
    let title_pos = xml.find("<w:titlePg").expect("titlePg 在场");
    assert!(type_pos < title_pos, "type 恒在 titlePg 前");

    // 读回 roundtrip。
    let doc = read_docx_from_bytes(&bytes).expect("回读");
    assert_eq!(doc.sections.len(), 2);
    let mid = &doc.sections[0];
    assert_eq!(mid.section_type, Some(SectionKind::Continuous));
    assert_eq!(mid.columns, 3, "读面栏数派生");
    let spec = mid.columns_spec.as_ref().expect("cols spec 在场");
    assert_eq!(spec.num, Some(3));
    assert_eq!(spec.space, Some(425));
    assert_eq!(spec.equal_width, None, "等宽缺省语义不代填");
    assert_eq!(spec.separator, None);
    assert!(spec.columns.is_empty());
    // 无增补末节 = 引擎缺省形态读回。
    let fin = &doc.sections[1];
    assert_eq!(fin.section_type, None, "末节未设类型");
    let fspec = fin.columns_spec.as_ref().expect("末节缺省 cols 在场");
    assert_eq!(fspec.num, Some(1));
    assert_eq!(fspec.space, Some(425));
}

/// 末节 exotic 分栏（equalWidth=0 + sep + w:col 子表——docx-rs 0.4.22
/// 零表达面）：读回逐字段一致。
#[test]
fn final_columns_exotic_roundtrip() {
    let mut w = DocxWriter::new();
    w.paragraph_text("双栏不等宽文档");
    w.columns(ColumnsSpec {
        num: Some(2),
        space: Some(708),
        equal_width: Some(false),
        separator: Some(true),
        columns: vec![
            ColumnWidth {
                width: Some(4674),
                space: Some(708),
            },
            ColumnWidth {
                width: Some(4674),
                space: None,
            },
        ],
    });
    let bytes = w.save_to_vec().expect("生成");

    let xml = document_xml(&bytes);
    assert!(
        xml.contains(
            "<w:cols w:num=\"2\" w:space=\"708\" w:equalWidth=\"0\" w:sep=\"1\">\
             <w:col w:w=\"4674\" w:space=\"708\" /><w:col w:w=\"4674\" /></w:cols>"
        ),
        "exotic 注入形态（开形 + 子表）：{xml}"
    );

    let doc = read_docx_from_bytes(&bytes).expect("回读");
    let fin = &doc.sections[0];
    assert_eq!(fin.source, mdc_office_io::docx::SectionSource::Final);
    let spec = fin.columns_spec.as_ref().expect("spec 在场");
    assert_eq!(spec.num, Some(2));
    assert_eq!(spec.space, Some(708));
    assert_eq!(spec.equal_width, Some(false));
    assert_eq!(spec.separator, Some(true));
    assert_eq!(
        spec.columns,
        vec![
            ColumnWidth {
                width: Some(4674),
                space: Some(708)
            },
            ColumnWidth {
                width: Some(4674),
                space: None
            },
        ],
        "w:col 子表逐字段（缺省属性 None 不代填）"
    );
}

/// 末节分节符类型：docx-rs 原生通道（零手术）+ 读回。
#[test]
fn document_level_section_type_native() {
    let mut w = DocxWriter::new();
    w.paragraph_text("本页内换节文档");
    w.section_type(SectionKind::Continuous);
    let bytes = w.save_to_vec().expect("生成");
    let xml = document_xml(&bytes);
    assert!(
        xml.contains("<w:type w:val=\"continuous\" />"),
        "原生发射形态在场：{xml}"
    );
    // 原生通道不经注入：cols 保持引擎缺省形态。
    assert!(xml.contains("<w:cols w:space=\"425\" w:num=\"1\" />"));

    let doc = read_docx_from_bytes(&bytes).expect("回读");
    assert_eq!(doc.sections[0].section_type, Some(SectionKind::Continuous));
}

/// None 语义保真：全缺省 spec → `<w:cols />` 属性缺席 → 读回全 None
/// （docx-rs 原生 columns/space usize 通道不可表达的面）。
#[test]
fn all_none_columns_spec_roundtrips_as_absent_attrs() {
    let mut w = DocxWriter::new();
    w.paragraph_text("全缺省分栏规格");
    w.columns(ColumnsSpec::default());
    let bytes = w.save_to_vec().expect("生成");
    let xml = document_xml(&bytes);
    assert!(xml.contains("<w:cols />"), "属性缺席形态：{xml}");

    let doc = read_docx_from_bytes(&bytes).expect("回读");
    let spec = doc.sections[0].columns_spec.as_ref().expect("spec 在场");
    assert_eq!(*spec, ColumnsSpec::default(), "None 全保真");
}

/// 账本定靶：三节混合（中段 0 增补 / 中段 1 无增补 / 末节增补）——
/// 注入只打目标节，无增补节原样透传（引擎缺省 cols 形态保留）。
#[test]
fn multi_section_ledger_targeting() {
    let mut w = DocxWriter::new();
    w.paragraph_text("节 0 段");
    w.section_break(SectionBreakSpec {
        columns: Some(three_equal_columns()),
        ..Default::default()
    });
    w.paragraph_text("节 1 段");
    w.section_break(SectionBreakSpec::default());
    w.paragraph_text("节 2 段");
    w.columns(ColumnsSpec {
        num: Some(2),
        ..ColumnsSpec::default()
    });
    let bytes = w.save_to_vec().expect("三节生成");

    let doc = read_docx_from_bytes(&bytes).expect("回读");
    assert_eq!(doc.sections.len(), 3, "两中段 + 一末节");
    // 节 0：三栏注入。
    assert_eq!(
        doc.sections[0].columns_spec.as_ref().unwrap().num,
        Some(3),
        "中段 0 定靶"
    );
    // 节 1：无增补 = 引擎缺省形态读回（透传面）。
    let s1 = doc.sections[1].columns_spec.as_ref().unwrap();
    assert_eq!(s1.num, Some(1), "中段 1 未被打中（缺省透传）");
    assert_eq!(s1.space, Some(425));
    // 节 2：末节两栏。
    assert_eq!(
        doc.sections[2].columns_spec.as_ref().unwrap().num,
        Some(2),
        "末节定靶"
    );
}

/// 字节级零变化：不用本面的文档——引擎缺省 cols 形态原样、零
/// `<w:type`（账本空 → save 漏斗结构性短路，无 zip 重写）。
#[test]
fn no_face_leaves_native_emission_untouched() {
    let mut w = DocxWriter::new();
    w.paragraph_text("普通单节文档");
    w.heading(1, "标题").expect("标题");
    w.section_break(SectionBreakSpec {
        title_page: true,
        ..Default::default()
    });
    w.paragraph_text("第二节");
    let bytes = w.save_to_vec().expect("生成");
    let xml = document_xml(&bytes);
    // 引擎缺省形态逐字节在场（未置换）。
    assert_eq!(
        xml.matches("<w:cols w:space=\"425\" w:num=\"1\" />")
            .count(),
        2,
        "两节均为引擎缺省 cols 形态"
    );
    assert!(!xml.contains("<w:type"), "零类型注入");
}

/// V10 域校验：五形态越域 → save 漏斗 InvalidInput 零产出 + 粘性
/// （二次 save 恒拒——与 V7/V9 同漏斗纪律）。
#[test]
fn columns_domain_validation_sticky() {
    let cases: Vec<ColumnsSpec> = vec![
        // num = 0（下界外）。
        ColumnsSpec {
            num: Some(0),
            ..ColumnsSpec::default()
        },
        // num = 64（上界外——Word 分栏上限 63）。
        ColumnsSpec {
            num: Some(64),
            ..ColumnsSpec::default()
        },
        // space 越域（> 31680 twips）。
        ColumnsSpec {
            space: Some(31681),
            ..ColumnsSpec::default()
        },
        // equalWidth=0 形态栏数不一致（num=2 但子表 3 条）。
        ColumnsSpec {
            num: Some(2),
            equal_width: Some(false),
            columns: vec![
                ColumnWidth::default(),
                ColumnWidth::default(),
                ColumnWidth::default(),
            ],
            ..ColumnsSpec::default()
        },
        // col 宽 0（正值域外）。
        ColumnsSpec {
            equal_width: Some(false),
            num: Some(1),
            columns: vec![ColumnWidth {
                width: Some(0),
                space: None,
            }],
            ..ColumnsSpec::default()
        },
    ];
    for (i, spec) in cases.iter().enumerate() {
        let mut w = DocxWriter::new();
        w.paragraph_text("越域用例");
        w.columns(spec.clone());
        let err = w.save_to_vec().expect_err(&format!("用例 {i} 必须拒"));
        assert!(
            matches!(err, mdc_office_io::docx::DocxError::InvalidInput(ref m) if m.contains("V10")),
            "用例 {i}：V10 域违规信号，实得 {err:?}"
        );
        // 粘性：二次 save 恒拒（不因首次拒绝而清账）。
        assert!(w.save_to_vec().is_err(), "用例 {i}：粘性拒绝");
    }
    // 中段节同域（section_break 入口同漏斗）。
    let mut w = DocxWriter::new();
    w.paragraph_text("中段越域");
    w.section_break(SectionBreakSpec {
        columns: Some(ColumnsSpec {
            num: Some(99),
            ..ColumnsSpec::default()
        }),
        ..Default::default()
    });
    assert!(
        matches!(
            w.save_to_vec().expect_err("中段越域必须拒"),
            mdc_office_io::docx::DocxError::InvalidInput(ref m) if m.contains("V10")
        ),
        "中段节 V10 同判据"
    );
}

/// 末节单值覆盖语义：columns 两调 → 后值覆盖（page_geometry 同口径）。
#[test]
fn columns_overwrite_single_value() {
    let mut w = DocxWriter::new();
    w.paragraph_text("覆盖语义");
    w.columns(three_equal_columns());
    w.columns(ColumnsSpec {
        num: Some(2),
        ..ColumnsSpec::default()
    });
    let bytes = w.save_to_vec().expect("生成");
    let doc = read_docx_from_bytes(&bytes).expect("回读");
    assert_eq!(
        doc.sections[0].columns_spec.as_ref().unwrap().num,
        Some(2),
        "后值覆盖先值"
    );
}
