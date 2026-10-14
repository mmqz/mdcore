//! xlsx-r2 Phase 3 DOCX 切片集成测试（写 → 读全环 + 容器门复用钉定）
//! → docx-r1 增量第一轮扩展。
//!
//! 验证链：
//! 1. 写读全环：DocxWriter（标题/富文本段落/表格/超链接/页眉页脚）
//!    → 落盘 → [`read_docx`]（含容器三门 + OPC 关系层）→ 自有模型
//!    逐面断言；
//! 2. 容器门复用：DOCX 读路径走 XLSX 同一门实现——敌意 DOCX
//!    （畸形 document.xml / 谎言头 / 路径逃逸）的归类钉定 +
//!    兼容模式降级；
//! 3. 字节入口不绕门：read_docx_from_bytes 同三门；
//! 4. 供应链纪律常驻化（ADR-O1 风险表）：docx-rs 升级破坏性变更
//!    由本往返测试首跑拦截（office-e §4 观察项）；
//! 5. docx-r1：OPC 关系层敌意面（.rels 词汇表/重复 Id/悬空引用/
//!    目标路径逃逸/目标部件缺失 × 严格/兼容）+ 超链接目标四变体
//!    + 页眉页脚六槽位读/写 + 引擎崩溃面直探针（#[should_panic]）。

use mdc_office_io::docx::{
    read_docx, read_docx_from_bytes_with_options, Block, DocxWriter, HeaderFooterVariant,
    HyperlinkTarget, RunSpec,
};
use mdc_office_io::io::{ReadOptions, WarningKind};
use std::io::Write as _;

fn tmp(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("mdc_io_docx_{name}_{}.docx", std::process::id()));
    p.to_string_lossy().into_owned()
}

/// 全环样本：标题 + 富文本段 + 表。
fn sample_writer() -> DocxWriter {
    let mut w = DocxWriter::new();
    w.heading(1, "季度报告").unwrap();
    w.heading(2, "概述").unwrap();
    w.paragraph(&[
        RunSpec::text("本季度 "),
        RunSpec::styled("关键指标", true, false),
        RunSpec::styled("显著", true, true),
        RunSpec::text("改善。"),
    ]);
    w.paragraph_text("普通段落文本。");
    w.heading(3, "数据表").unwrap();
    w.table(&[
        vec!["指标".to_string(), "Q1".to_string(), "Q2".to_string()],
        vec!["收入".to_string(), "100".to_string(), "180".to_string()],
        vec!["成本".to_string(), "60".to_string(), "70".to_string()],
    ]);
    w
}

#[test]
fn write_read_full_roundtrip() {
    let p = tmp("roundtrip");
    sample_writer().save_to_path(&p).unwrap();

    let doc = read_docx(&p).unwrap();
    assert!(doc.warnings.is_empty(), "合法档零警告：{:?}", doc.warnings);

    // 块序 + 类型。
    let blocks: Vec<&Block> = doc.blocks.iter().collect();
    assert_eq!(blocks.len(), 6, "块序：标题×3 + 段落×2 + 表×1");

    // 标题：样式 ID 透传 + outline（样式面在 styles.xml——读面以
    // style ID 为准[模型级]，outline 属结构语义不进读模型）。
    match &blocks[0] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.style.as_deref(), Some("Heading1"));
            assert_eq!(pd.text(), "季度报告");
        }
        other => panic!("block 0 应为段落, got {other:?}"),
    }
    match &blocks[1] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.style.as_deref(), Some("Heading2"));
            assert_eq!(pd.text(), "概述");
        }
        other => panic!("block 1 应为段落, got {other:?}"),
    }
    // 富文本段：run 级边界 + 加粗/斜体往返。
    match &blocks[2] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.style.as_deref(), None, "普通段无样式引用");
            assert_eq!(pd.runs.len(), 4);
            assert_eq!(pd.runs[0], run_data(RunSpec::text("本季度 ")));
            assert_eq!(pd.runs[1].text, "关键指标");
            assert!(pd.runs[1].bold && !pd.runs[1].italic);
            assert_eq!(pd.runs[2].text, "显著");
            assert!(pd.runs[2].bold && pd.runs[2].italic);
            assert!(!pd.runs[3].bold && !pd.runs[3].italic);
            assert_eq!(pd.text(), "本季度 关键指标显著改善。");
        }
        other => panic!("block 2 应为段落, got {other:?}"),
    }
    match &blocks[3] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.text(), "普通段落文本。");
        }
        other => panic!("block 3 应为段落, got {other:?}"),
    }
    match &blocks[4] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.style.as_deref(), Some("Heading3"));
        }
        other => panic!("block 4 应为段落, got {other:?}"),
    }
    // 表：行 × 单元格。
    match &blocks[5] {
        Block::Table(t) => {
            assert_eq!(t.rows.len(), 3);
            assert_eq!(t.rows[0], vec!["指标", "Q1", "Q2"]);
            assert_eq!(t.rows[1], vec!["收入", "100", "180"]);
            assert_eq!(t.rows[2], vec!["成本", "60", "70"]);
        }
        other => panic!("block 5 应为表, got {other:?}"),
    }

    let _ = std::fs::remove_file(&p);
}

/// 写侧规格 → 读侧模型（同构对照；自由函数避免孤儿规则）。
fn run_data(spec: RunSpec) -> mdc_office_io::docx::RunData {
    mdc_office_io::docx::RunData {
        text: spec.text,
        bold: spec.bold,
        italic: spec.italic,
        hyperlink: None,
    }
}

#[test]
fn save_to_vec_bytes_are_reopenable() {
    let mut w = sample_writer();
    let bytes = w.save_to_vec().unwrap();
    assert_eq!(&bytes[..2], b"PK", "docx = zip 容器");
    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(doc.blocks.len(), 6);
}

// ---------------------------------------------------------------------------
// 容器门复用钉定（DOCX 与 XLSX 同一门实现——基础设施共享）
// ---------------------------------------------------------------------------

/// 读 zip 条目为字节。
fn zip_read_entry(path: &str, want: &str) -> Option<Vec<u8>> {
    let f = std::fs::File::open(path).ok()?;
    let mut za = zip::ZipArchive::new(f).ok()?;
    for i in 0..za.len() {
        let mut zf = za.by_index(i).ok()?;
        if zf.name() == want {
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut zf, &mut v).ok()?;
            return Some(v);
        }
    }
    None
}

/// 以改写部件重建 zip（保留其余条目原样）。
fn rebuild_with_part(path: &str, part: &str, new_data: &[u8]) -> String {
    let out = path.replace(".docx", "_rebuilt.docx");
    let f = std::fs::File::open(path).unwrap();
    let mut za = zip::ZipArchive::new(f).unwrap();
    let of = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    let mut zw = zip::ZipWriter::new(of);
    let opts: zip::write::SimpleFileOptions = Default::default();
    for i in 0..za.len() {
        let mut zf = za.by_index(i).unwrap();
        let name = zf.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut zf, &mut data).unwrap();
        if name == part {
            zw.start_file(name.as_str(), opts).unwrap();
            zw.write_all(new_data).unwrap();
        } else {
            zw.start_file(name.as_str(), opts).unwrap();
            zw.write_all(&data).unwrap();
        }
    }
    zw.finish().unwrap();
    out
}

#[test]
fn gates_shared_with_xlsx_path() {
    // 基准合法档。
    let p = tmp("gates");
    sample_writer().save_to_path(&p).unwrap();
    let doc_xml = zip_read_entry(&p, "word/document.xml").expect("document.xml 在位");

    // ① 门 3：截断 document.xml（缺根闭合）→ 严格 InvalidPart /
    //    兼容警告 + 模型照常提取。
    let truncated = &doc_xml[..doc_xml.len() - s_len(&doc_xml)];
    let p_trunc = rebuild_with_part(&p, "word/document.xml", truncated);
    match read_docx(&p_trunc) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { part, .. },
        )) => assert_eq!(part, "word/document.xml"),
        other => panic!("严格模式应 InvalidPart, got {other:?}"),
    }
    let compat = ReadOptions { compat: true };
    match read_docx_with(&p_trunc, &compat) {
        Ok(doc) => {
            assert!(doc
                .warnings
                .iter()
                .any(|w| w.kind == WarningKind::MalformedXml));
        }
        other => panic!("兼容模式应降级为警告, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p_trunc);

    // ② 门 3：垃圾 document.xml → 严格拒。
    let garbage: Vec<u8> = (0..256u16).map(|i| i as u8).cycle().take(512).collect();
    let p_garbage = rebuild_with_part(&p, "word/document.xml", &garbage);
    match read_docx(&p_garbage) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { .. },
        )) => {}
        other => panic!("垃圾部件应 InvalidPart, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p_garbage);

    // ③ 门 1：路径逃逸条目 → typed reject（DOCX 容器面与 XLSX 同构）。
    let p_escape = tmp("escape");
    let f = std::io::BufWriter::new(std::fs::File::create(&p_escape).unwrap());
    let mut zw = zip::ZipWriter::new(f);
    let opts: zip::write::SimpleFileOptions = Default::default();
    zw.start_file("../evil.docx", opts).unwrap();
    zw.write_all(b"escape").unwrap();
    zw.finish().unwrap();
    match read_docx(&p_escape) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::PrescanRejected { reason, .. },
        )) => assert_eq!(reason, "path escape entry"),
        other => panic!("路径逃逸应门 1 拒, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p_escape);
    let _ = std::fs::remove_file(&p);
}

/// read_docx_with_options 的短名包装（测试内多处用）。
fn read_docx_with(
    path: &str,
    opts: &ReadOptions,
) -> Result<mdc_office_io::docx::DocxDocument, mdc_office_io::docx::DocxError> {
    mdc_office_io::docx::read_docx_with_options(path, opts)
}

/// 移除尾部的 `</w:document>`（12 字符）。
fn s_len(_doc: &[u8]) -> usize {
    "</w:document>".len()
}

#[test]
fn byte_entry_runs_same_gates() {
    // 字节入口不绕门：垃圾 document.xml 的 zip 字节 → InvalidPart。
    let p = tmp("bytes");
    sample_writer().save_to_path(&p).unwrap();
    let garbage: Vec<u8> = (0..256u16).map(|i| i as u8).cycle().take(512).collect();
    let p_garbage = rebuild_with_part(&p, "word/document.xml", &garbage);
    let bytes = std::fs::read(&p_garbage).unwrap();
    match read_docx_from_bytes_with_options(&bytes, &ReadOptions::default()) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { .. },
        )) => {}
        other => panic!("字节入口应同门拒绝, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p_garbage);
}

#[test]
fn control_chars_in_docx_engine_rejects() {
    // 实测钉定（与 XLSX 面的行为差异——诚实登记）：原始 NUL 注入
    // document.xml 文本位后，**docx-rs 读引擎自身硬拒**
    //（Parse("Failed to read xml."））——较 calamine 对 XLSX 的
    // 值面容忍更严；门 3 的 ControlChars 警告在该错误路径不可达
    //（引擎错误优先于返回值携带的警告面）。两模式同拒（兼容只
    // 放宽本层门验证，不改变引擎行为——与 mal7a 兼容语义一致）。
    let p = tmp("ctrl");
    sample_writer().save_to_path(&p).unwrap();
    let doc_xml = zip_read_entry(&p, "word/document.xml").unwrap();
    // 在首个 <w:t xml:space="preserve"> 后插入原始 NUL（文本位；
    // 实测 docx-rs 写出形态带 xml:space 属性）。
    let mut injected = doc_xml.clone();
    if let Some(pos) = find_sub(&doc_xml, b"<w:t xml:space=\"preserve\">") {
        injected.splice(pos + 27..pos + 27, [0x00u8]);
    } else {
        panic!("未找到文本标签——docx-rs 写出形态变化？");
    }
    let p_ctrl = rebuild_with_part(&p, "word/document.xml", &injected);
    for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
        match mdc_office_io::docx::read_docx_with_options(&p_ctrl, &opts) {
            Err(mdc_office_io::docx::DocxError::Parse(_)) => {}
            other => panic!("控制字符应引擎级拒绝（{:?}）, got {other:?}", opts.compat),
        }
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p_ctrl);
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// docx-r1 增量第一轮：OPC 关系层 + 超链接目标 + 页眉页脚
// ---------------------------------------------------------------------------

/// 超链接写读全环：外部 URL（含 `&` 实体补偿链——写侧引擎
/// escape_owned 单次转义 + 本层读回单次反转义 = 逻辑值复原）。
#[test]
fn hyperlink_roundtrip_external() {
    let p = tmp("link");
    let mut w = DocxWriter::new();
    w.paragraph(&[
        RunSpec::text("参见 "),
        RunSpec::link("文档站点", "https://doc.example/?a=1&b=2"),
        RunSpec::text(" 与 "),
        RunSpec::link("内部引用", "https://in.example/x"),
        RunSpec::text("。"),
    ]);
    w.save_to_path(&p).unwrap();

    let doc = read_docx(&p).unwrap();
    assert!(doc.warnings.is_empty(), "合法档零警告：{:?}", doc.warnings);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.runs.len(), 5);
            assert_eq!(pd.runs[0].hyperlink, None, "普通 run 无目标");
            assert_eq!(
                pd.runs[1].hyperlink,
                Some(HyperlinkTarget::External {
                    target: "https://doc.example/?a=1&b=2".into()
                }),
                "实体补偿：& 复原为逻辑值"
            );
            assert_eq!(pd.runs[2].hyperlink, None, "链接后的普通 run 边界正确");
            assert_eq!(
                pd.runs[3].hyperlink,
                Some(HyperlinkTarget::External {
                    target: "https://in.example/x".into()
                })
            );
            assert_eq!(pd.runs[4].hyperlink, None);
            assert_eq!(pd.text(), "参见 文档站点 与 内部引用。");
        }
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
}

/// 锚点超链接读（w:anchor——无关系面；净室自构造：改写 document.xml
/// 注入锚点链接段）。
#[test]
fn hyperlink_anchor_read() {
    let p = tmp("anchor");
    sample_writer().save_to_path(&p).unwrap();
    let doc_xml = zip_read_entry(&p, "word/document.xml").unwrap();
    let mut xml = String::from_utf8(doc_xml.clone()).unwrap();
    let sect = "<w:sectPr>";
    let pos = xml.find(sect).expect("sectPr 在位");
    xml.insert_str(
        pos,
        "<w:p><w:hyperlink w:anchor=\"top\" w:history=\"1\">\
         <w:r><w:t>回到顶部</w:t></w:r></w:hyperlink></w:p>",
    );
    let p2 = rebuild_with_part(&p, "word/document.xml", xml.as_bytes());

    let doc = read_docx(&p2).unwrap();
    let last = doc.blocks.last().expect("注入段在位");
    match last {
        Block::Paragraph(pd) => {
            assert_eq!(pd.runs.len(), 1);
            assert_eq!(
                pd.runs[0].hyperlink,
                Some(HyperlinkTarget::Anchor {
                    anchor: "top".into()
                })
            );
            assert_eq!(pd.runs[0].text, "回到顶部");
        }
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 悬空超链接引用（r:id 无关系条目）：严格 typed 拒 / 兼容
/// Unresolved + BrokenReference 警告（文本保留）。
#[test]
fn hyperlink_dangling_strict_rejects_compat_unresolved() {
    let p = tmp("dangling");
    sample_writer().save_to_path(&p).unwrap();
    let doc_xml = zip_read_entry(&p, "word/document.xml").unwrap();
    let mut xml = String::from_utf8(doc_xml.clone()).unwrap();
    let sect = "<w:sectPr>";
    let pos = xml.find(sect).unwrap();
    xml.insert_str(
        pos,
        "<w:p><w:hyperlink r:id=\"rId999\" w:history=\"1\">\
         <w:r><w:t>断链</w:t></w:r></w:hyperlink></w:p>",
    );
    let p2 = rebuild_with_part(&p, "word/document.xml", xml.as_bytes());

    // 严格：typed 拒（InvalidPart——dangling reference）。
    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { part, detail },
        )) => {
            assert_eq!(part, "word/document.xml");
            assert!(detail.contains("rId999"), "{detail}");
        }
        other => panic!("严格模式应拒, got {other:?}"),
    }
    // 兼容：Unresolved + 警告 + 文本保留。
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::BrokenReference && w.detail.contains("rId999")));
    let last = doc.blocks.last().unwrap();
    match last {
        Block::Paragraph(pd) => {
            assert_eq!(pd.runs[0].text, "断链", "文本保留");
            assert_eq!(
                pd.runs[0].hyperlink,
                Some(HyperlinkTarget::Unresolved {
                    rid: "rId999".into()
                })
            );
        }
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 页眉/页脚写读全环（default 槽位 + 重复调用替换语义 + 变体透传）。
#[test]
fn header_footer_roundtrip() {
    let p = tmp("hf");
    let mut w = sample_writer();
    w.header(&[
        RunSpec::text("公司文档 · "),
        RunSpec::styled("机密", true, false),
    ]);
    w.header(&[RunSpec::text("最终页眉")]); // 重复调用 = 原位替换
    w.footer(&[RunSpec::text("第 1 页")]);
    w.save_to_path(&p).unwrap();

    let doc = read_docx(&p).unwrap();
    assert!(doc.warnings.is_empty(), "合法档零警告：{:?}", doc.warnings);
    // 页眉：恰一个 default 槽位，替换语义生效。
    assert_eq!(doc.headers.len(), 1, "恰一个槽位（替换不增计数）");
    assert_eq!(doc.headers[0].variant, HeaderFooterVariant::Default);
    assert_eq!(doc.headers[0].blocks.len(), 1);
    match &doc.headers[0].blocks[0] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.text(), "最终页眉", "第二次调用替换第一次");
        }
        other => panic!("页眉应为段落, got {other:?}"),
    }
    // 页脚。
    assert_eq!(doc.footers.len(), 1);
    assert_eq!(doc.footers[0].variant, HeaderFooterVariant::Default);
    match &doc.footers[0].blocks[0] {
        Block::Paragraph(pd) => assert_eq!(pd.text(), "第 1 页"),
        other => panic!("页脚应为段落, got {other:?}"),
    }
    // 文档体不受影响。
    assert_eq!(doc.blocks.len(), 6, "sample_writer 六块");
    let _ = std::fs::remove_file(&p);
}

/// 页眉引用悬空三面：r:id 无条目 / 目标部件缺失 / 目标路径逃逸
/// （严格 typed 拒；兼容警告 + 槽位跳过 + 正文照常提取）。
#[test]
fn header_reference_hostile_strict_compat() {
    let p = tmp("hhref");
    sample_writer().save_to_path(&p).unwrap();
    let doc_xml = zip_read_entry(&p, "word/document.xml").unwrap();
    let rels_xml = zip_read_entry(&p, "word/_rels/document.xml.rels").expect("document rels 在位");

    // 注入 sectPr 页眉引用（引擎不挂接内容——r:id 不可解析）。
    let inject_reference = |rid: &str| -> String {
        let mut xml = String::from_utf8(doc_xml.clone()).unwrap();
        let pos = xml.find("<w:sectPr>").unwrap();
        xml.insert_str(
            pos + "<w:sectPr>".len(),
            &format!("<w:headerReference w:type=\"default\" r:id=\"{rid}\"/>"),
        );
        xml
    };

    // ① r:id 无关系条目。
    let xml = inject_reference("rId9");
    let p1 = rebuild_with_part(&p, "word/document.xml", xml.as_bytes());
    match read_docx(&p1) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { detail, .. },
        )) => assert!(detail.contains("rId9"), "{detail}"),
        other => panic!("① 严格应拒, got {other:?}"),
    }
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p1, &compat).unwrap();
    assert!(doc.headers.is_empty(), "兼容：槽位跳过");
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::BrokenReference && w.detail.contains("rId9")));
    assert_eq!(doc.blocks.len(), 6, "正文照常提取");
    let _ = std::fs::remove_file(&p1);

    // ② r:id 有条目但目标部件缺失（rels 追加 + document 引用）。
    let mut rels = String::from_utf8(rels_xml.clone()).unwrap();
    rels.insert_str(
        rels.len() - "</Relationships>".len(),
        "<Relationship Id=\"rIdH9\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" \
         Target=\"header9.xml\"/>",
    );
    let xml = inject_reference("rIdH9");
    let p2 = tmp("hhref2");
    write_two_parts(&p, &p2, "word/document.xml", xml.as_bytes(), &rels);
    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { detail, .. },
        )) => assert!(detail.contains("header9.xml"), "{detail}"),
        other => panic!("② 严格应拒, got {other:?}"),
    }
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert!(doc.headers.is_empty());
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::BrokenReference && w.detail.contains("header9.xml")));
    let _ = std::fs::remove_file(&p2);

    // ③ 目标路径逃逸（../../evil/evil.xml——词法归一化越出包根）。
    let mut rels = String::from_utf8(rels_xml.clone()).unwrap();
    rels.insert_str(
        rels.len() - "</Relationships>".len(),
        "<Relationship Id=\"rIdH8\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" \
         Target=\"../../evil/evil.xml\"/>",
    );
    let xml = inject_reference("rIdH8");
    let p3 = tmp("hhref3");
    write_two_parts(&p, &p3, "word/document.xml", xml.as_bytes(), &rels);
    match read_docx(&p3) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { detail, .. },
        )) => assert!(detail.contains("escapes package root"), "{detail}"),
        other => panic!("③ 严格应拒, got {other:?}"),
    }
    let doc = read_docx_with(&p3, &compat).unwrap();
    assert!(doc.headers.is_empty());
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::BrokenReference && w.detail.contains("escapes")));
    let _ = std::fs::remove_file(&p3);
    let _ = std::fs::remove_file(&p);
}

/// 以两处改写部件重建 zip（document.xml + document.xml.rels）。
fn write_two_parts(src: &str, out: &str, part1: &str, data1: &[u8], rels: &str) {
    let f = std::fs::File::open(src).unwrap();
    let mut za = zip::ZipArchive::new(f).unwrap();
    let of = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    let mut zw = zip::ZipWriter::new(of);
    let opts: zip::write::SimpleFileOptions = Default::default();
    for i in 0..za.len() {
        let mut zf = za.by_index(i).unwrap();
        let name = zf.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut zf, &mut data).unwrap();
        zw.start_file(name.as_str(), opts).unwrap();
        if name == part1 {
            zw.write_all(data1).unwrap();
        } else if name == "word/_rels/document.xml.rels" {
            zw.write_all(rels.as_bytes()).unwrap();
        } else {
            zw.write_all(&data).unwrap();
        }
    }
    zw.finish().unwrap();
}

/// .rels 重复 Id（整包面）：严格 typed 拒 / 兼容警告 + 首胜
/// （链接目标 = 文档序首条）。
#[test]
fn rels_duplicate_id_in_package_strict_compat() {
    let p = tmp("dup");
    let mut w = DocxWriter::new();
    w.paragraph(&[RunSpec::link("链接", "https://first.example/")]);
    w.save_to_path(&p).unwrap();

    // 定位引擎实际发射的超链接 rId（全局计数器——不硬编码）。
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").expect("rels 在位"))
            .unwrap();
    let hl_rid = rels
        .lines()
        .find_map(|l| {
            if l.contains("relationships/hyperlink") {
                let start = l.find("Id=\"")? + 4;
                let end = l[start..].find('"')? + start;
                Some(l[start..end].to_string())
            } else {
                None
            }
        })
        .expect("超链接关系在位");

    // 追加重复 Id 条目（不同目标）。
    let mut dup = rels.clone();
    dup.insert_str(
        dup.len() - "</Relationships>".len(),
        &format!(
            "<Relationship Id=\"{hl_rid}\" \
             Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" \
             Target=\"https://second.example/\" TargetMode=\"External\"/>"
        ),
    );
    let p2 = tmp("dup2");
    rebuild_with_part_to(&p, &p2, "word/_rels/document.xml.rels", dup.as_bytes());

    // 严格：typed 拒。
    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { part, detail },
        )) => {
            assert_eq!(part, "word/_rels/document.xml.rels");
            assert!(detail.contains("duplicate"), "{detail}");
        }
        other => panic!("严格应拒, got {other:?}"),
    }
    // 兼容：警告 + 首胜（文档序首条 = 引擎原发条目）。
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.part == "word/_rels/document.xml.rels"
            && w.kind == WarningKind::MalformedXml
            && w.detail.contains("duplicate")));
    match &doc.blocks[0] {
        Block::Paragraph(pd) => assert_eq!(
            pd.runs[0].hyperlink,
            Some(HyperlinkTarget::External {
                target: "https://first.example/".into()
            }),
            "首胜"
        ),
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// rebuild_with_part 的目标路径可指定版（两处调用点共用逻辑）。
fn rebuild_with_part_to(src: &str, out: &str, part: &str, new_data: &[u8]) {
    let f = std::fs::File::open(src).unwrap();
    let mut za = zip::ZipArchive::new(f).unwrap();
    let of = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    let mut zw = zip::ZipWriter::new(of);
    let opts: zip::write::SimpleFileOptions = Default::default();
    for i in 0..za.len() {
        let mut zf = za.by_index(i).unwrap();
        let name = zf.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut zf, &mut data).unwrap();
        zw.start_file(name.as_str(), opts).unwrap();
        if name == part {
            zw.write_all(new_data).unwrap();
        } else {
            zw.write_all(&data).unwrap();
        }
    }
    zw.finish().unwrap();
}

/// .rels 词汇表外元素（整包面）：严格 typed 拒 / 兼容警告 + 跳过
/// （引擎同等容忍——CAPABILITY_NOTES §8；链接照常联接）。
#[test]
fn rels_unknown_element_in_package_strict_compat() {
    let p = tmp("vocab");
    sample_writer().save_to_path(&p).unwrap();
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").expect("rels 在位"))
            .unwrap();
    let mut hostile = rels.clone();
    hostile.insert_str(
        hostile.len() - "</Relationships>".len(),
        "<zzz Id=\"rIdZ\" Type=\"t\" Target=\"z.xml\"/>",
    );
    let p2 = tmp("vocab2");
    rebuild_with_part_to(&p, &p2, "word/_rels/document.xml.rels", hostile.as_bytes());

    // 严格：typed 拒。
    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { part, detail },
        )) => {
            assert_eq!(part, "word/_rels/document.xml.rels");
            assert!(detail.contains("unknown element"), "{detail}");
        }
        other => panic!("严格应拒, got {other:?}"),
    }
    // 兼容：警告 + 跳过（数据照常提取）。
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.part == "word/_rels/document.xml.rels"
            && w.kind == WarningKind::MalformedXml
            && w.detail.contains("unknown element")));
    assert_eq!(doc.blocks.len(), 6, "数据照常提取");
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 引擎容忍面直探针（CAPABILITY_NOTES §8 实证勘误）：词汇表外元素
/// 直达 docx-rs `read_rels_xml`——`XMLElement::from_str` 全名映射
/// `Ok(Unsupported)`（xml_element.rs:447 catch-all）永不 Err，
/// `if let XMLElement::Relationship` 忽略之 → **引擎静默容忍、不崩溃**。
/// 产品面语义 = 上方测试：严格拒 / 兼容警告 + 跳过（引擎容忍 + 可见性）。
#[test]
fn engine_rels_unknown_element_tolerated() {
    let p = tmp("panic");
    sample_writer().save_to_path(&p).unwrap();
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").expect("rels 在位"))
            .unwrap();
    let mut hostile = rels.clone();
    hostile.insert_str(
        hostile.len() - "</Relationships>".len(),
        "<zzz Id=\"rIdZ\" Type=\"t\" Target=\"z.xml\"/>",
    );
    let p2 = tmp("panic2");
    rebuild_with_part_to(&p, &p2, "word/_rels/document.xml.rels", hostile.as_bytes());
    let bytes = std::fs::read(&p2).unwrap();
    // 引擎直调（绕过本层门/关系层）——容忍面实证：不 panic，
    // 正常返回（未知元素被静默忽略，合法关系照常收集）。
    let engine_doc =
        docx_rs::read_docx(&bytes).expect("引擎容忍未知 rels 元素（CAPABILITY_NOTES §8 勘误）");
    assert!(!engine_doc.document.children.is_empty(), "正文照常解析");
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 页眉内超链接目标（页眉部件自身 rels 联接——净室自构造：改写
/// 页眉 XML + 注入页眉 rels 部件）。
#[test]
fn hyperlink_inside_header_resolved_via_header_rels() {
    let p = tmp("hhlink");
    let mut w = DocxWriter::new();
    w.header(&[RunSpec::text("页眉文本")]);
    w.paragraph_text("正文。");
    w.save_to_path(&p).unwrap();

    // 改写页眉：追加外链段（r:id 指向页眉自身 rels 的条目）。
    let hdr = String::from_utf8(zip_read_entry(&p, "word/header1.xml").unwrap()).unwrap();
    let mut new_hdr = hdr.clone();
    let close = "</w:hdr>";
    let pos = new_hdr.rfind(close).unwrap();
    new_hdr.insert_str(
        pos,
        "<w:p><w:hyperlink r:id=\"rIdH1\" w:history=\"1\">\
         <w:r><w:t>页眉链接</w:t></w:r></w:hyperlink></w:p>",
    );
    // 注入页眉自身 rels 部件（word/_rels/header1.xml.rels）。
    let header_rels = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rIdH1\" ",
        "Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" ",
        "Target=\"https://header.example/\" TargetMode=\"External\"/>",
        "</Relationships>"
    );
    let p2 = tmp("hhlink2");
    {
        let f = std::fs::File::open(&p).unwrap();
        let mut za = zip::ZipArchive::new(f).unwrap();
        let of = std::io::BufWriter::new(std::fs::File::create(&p2).unwrap());
        let mut zw = zip::ZipWriter::new(of);
        let opts: zip::write::SimpleFileOptions = Default::default();
        for i in 0..za.len() {
            let mut zf = za.by_index(i).unwrap();
            let name = zf.name().to_string();
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut zf, &mut data).unwrap();
            zw.start_file(name.as_str(), opts).unwrap();
            if name == "word/header1.xml" {
                zw.write_all(new_hdr.as_bytes()).unwrap();
            } else if name == "word/_rels/header1.xml.rels" {
                // 写器恒发射页眉 rels 部件（空表）——原位改写。
                zw.write_all(header_rels.as_bytes()).unwrap();
            } else {
                zw.write_all(&data).unwrap();
            }
        }
        zw.finish().unwrap();
    }

    let doc = read_docx(&p2).unwrap();
    assert!(
        doc.warnings.is_empty(),
        "页眉部件 rels 联接合法：{:?}",
        doc.warnings
    );
    assert_eq!(doc.headers.len(), 1);
    let hdr_blocks = &doc.headers[0].blocks;
    assert_eq!(hdr_blocks.len(), 2, "原段 + 注入链接段");
    match &hdr_blocks[1] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.runs[0].text, "页眉链接");
            assert_eq!(
                pd.runs[0].hyperlink,
                Some(HyperlinkTarget::External {
                    target: "https://header.example/".into()
                }),
                "页眉内链接经页眉自身 rels 联接"
            );
        }
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

// ---------------------------------------------------------------------------
// docx-r2 增量第二轮：脚注（写·XML 级）/ 简单域（写）/ 图片（读清单 +
// 写 PNG-only）/ first-even 槽位（写）
// ---------------------------------------------------------------------------

/// 最小合法 1×1 RGBA PNG（70 字节——hand-constructed，IHDR/IDAT/IEND）。
const TINY_PNG: [u8; 70] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x10, 0x54, 0x32, 0xFE,
    0x0F, 0x00, 0x02, 0x14, 0x01, 0x66, 0x04, 0xE0, 0xDE, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// 向已有包追加条目（保留原条目原样）。
fn add_entries_to(src: &str, out: &str, new_entries: &[(&str, &[u8])]) {
    let f = std::fs::File::open(src).unwrap();
    let mut za = zip::ZipArchive::new(f).unwrap();
    let of = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    let mut zw = zip::ZipWriter::new(of);
    let opts: zip::write::SimpleFileOptions = Default::default();
    for i in 0..za.len() {
        let mut zf = za.by_index(i).unwrap();
        let name = zf.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut zf, &mut data).unwrap();
        zw.start_file(name.as_str(), opts).unwrap();
        zw.write_all(&data).unwrap();
    }
    for (name, data) in new_entries {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(data).unwrap();
    }
    zw.finish().unwrap();
}

/// 产出「一段文本 + 一图」基准包，返回路径与媒体部件名。
fn write_image_doc(tag: &str) -> (String, String) {
    let p = tmp(tag);
    let mut w = DocxWriter::new();
    w.paragraph_text("带图段落。");
    w.image(&TINY_PNG, 1, 1).unwrap();
    w.save_to_path(&p).unwrap();
    // 媒体部件名动态定位（引擎 rIdImage{n} 全局计数——不硬编码；
    // 排除目录条目 word/media/）。
    let f = std::fs::File::open(&p).unwrap();
    let za = zip::ZipArchive::new(f).unwrap();
    let media: Vec<String> = za
        .file_names()
        .filter(|n| n.starts_with("word/media/") && !n.ends_with('/'))
        .map(str::to_string)
        .collect();
    assert_eq!(media.len(), 1, "恰一个媒体部件");
    (p, media[0].clone())
}

/// 脚注写面全环（XML 级断言——读面引擎受限登记：reader 从不读
/// footnotes.xml，往返验证不依赖读面）。
#[test]
fn footnote_write_xml_level_roundtrip() {
    let p = tmp("footnote");
    let mut w = DocxWriter::new();
    w.paragraph(&[RunSpec::text("正文甲"), RunSpec::footnote("脚注甲内容")]);
    w.paragraph(&[RunSpec::text("正文乙"), RunSpec::footnote("脚注乙内容")]);
    w.save_to_path(&p).unwrap();

    // ① footnotes.xml 部件在位 + 两条脚注内容。
    let footnotes =
        String::from_utf8(zip_read_entry(&p, "word/footnotes.xml").expect("footnotes.xml 在位"))
            .unwrap();
    assert!(footnotes.contains("脚注甲内容"), "{footnotes}");
    assert!(footnotes.contains("脚注乙内容"), "{footnotes}");
    let note_ids: Vec<&str> = footnotes
        .split("<w:footnote ")
        .skip(1)
        .map(|s| {
            let start = s.find("w:id=\"").unwrap() + 6;
            &s[start..s[start..].find('"').unwrap() + start]
        })
        .collect();
    assert_eq!(note_ids.len(), 2, "两条脚注: {footnotes}");
    assert_ne!(note_ids[0], note_ids[1], "脚注 id 互异");

    // ② document.xml 引用 id 与 footnotes.xml 条目 id 一一对应。
    let doc_xml = String::from_utf8(zip_read_entry(&p, "word/document.xml").unwrap()).unwrap();
    let ref_ids: Vec<&str> = doc_xml
        .split("<w:footnoteReference ")
        .skip(1)
        .map(|s| {
            let start = s.find("w:id=\"").unwrap() + 6;
            &s[start..s[start..].find('"').unwrap() + start]
        })
        .collect();
    assert_eq!(ref_ids.len(), 2, "两个引用 marker: {doc_xml}");
    for rid in &ref_ids {
        assert!(
            footnotes.contains(&format!("<w:footnote w:id=\"{rid}\"")),
            "引用 id {rid} 有对应脚注条目"
        );
    }
    // ③ 关系 + 内容类型 + 激活面（rels rId8 + footnotes override）。
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").unwrap()).unwrap();
    assert!(rels.contains("relationships/footnotes"), "{rels}");
    let ct = String::from_utf8(zip_read_entry(&p, "[Content_Types].xml").unwrap()).unwrap();
    assert!(ct.contains("footnotes+xml"), "{ct}");

    // ④ 读回（严格）：文本提取不含脚注内容（引擎受限登记面钉定），
    //    footnotes.xml 部件存在不致引擎/门拒。
    let doc = read_docx(&p).unwrap();
    match &doc.blocks[0] {
        Block::Paragraph(pd) => assert_eq!(pd.text(), "正文甲"),
        other => panic!("应为段落, got {other:?}"),
    }
    assert!(doc.warnings.is_empty());
    let _ = std::fs::remove_file(&p);
}

/// 简单域写面（PAGE/NUMPAGES）：XML 级三连断言 + 读回文本不含域值
/// （域值 = 渲染期计算面，登记）。
#[test]
fn simple_fields_write_xml_level_roundtrip() {
    let p = tmp("fields");
    let mut w = DocxWriter::new();
    w.footer(&[
        RunSpec::text("第 "),
        RunSpec::field_page(),
        RunSpec::text(" 页 / 共 "),
        RunSpec::field_num_pages(),
        RunSpec::text(" 页"),
    ]);
    w.save_to_path(&p).unwrap();

    let footer = String::from_utf8(zip_read_entry(&p, "word/footer1.xml").unwrap()).unwrap();
    assert!(
        footer.contains("<w:instrText>PAGE</w:instrText>"),
        "{footer}"
    );
    assert!(
        footer.contains("<w:instrText>NUMPAGES</w:instrText>"),
        "{footer}"
    );
    assert_eq!(
        footer.matches("w:fldCharType=\"begin\"").count(),
        2,
        "{footer}"
    );
    assert_eq!(
        footer.matches("w:fldCharType=\"end\"").count(),
        2,
        "{footer}"
    );

    // 读回：域 run 无文本（模型级钉定——域结果文本不入读回文本）。
    let doc = read_docx(&p).unwrap();
    assert_eq!(doc.footers.len(), 1);
    match &doc.footers[0].blocks[0] {
        Block::Paragraph(pd) => assert_eq!(pd.text(), "第  页 / 共  页"),
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
}

/// 图片写读全环（模型级：字节/内容类型/引用关系/正文引用计数）。
#[test]
fn image_write_read_roundtrip() {
    let (p, media) = write_image_doc("img_roundtrip");

    let doc = read_docx(&p).unwrap();
    assert_eq!(doc.images.len(), 1);
    let asset = &doc.images[0];
    assert_eq!(asset.part_name, media);
    assert_eq!(asset.content_type.as_deref(), Some("image/png"));
    assert_eq!(asset.bytes, TINY_PNG.to_vec(), "字节全环逐位一致");
    assert_eq!(asset.references.len(), 1, "恰一条引用关系");
    assert_eq!(asset.references[0].owner_part, "word/document.xml");
    assert_eq!(asset.body_uses, 1, "正文 run 级 a:blip 引用解析计数");
    assert!(doc.warnings.is_empty());
    // 兼容模式同包同清单（无降级面）。
    let compat = ReadOptions { compat: true };
    let doc2 = read_docx_with(&p, &compat).unwrap();
    assert_eq!(doc2.images.len(), 1);
    // XML 级：blip embed 经 rels 可解析（rid 动态定位）。
    let doc_xml = String::from_utf8(zip_read_entry(&p, "word/document.xml").unwrap()).unwrap();
    let start = doc_xml.find("r:embed=\"").expect("blip embed 在位") + 9;
    let rid = &doc_xml[start..start + doc_xml[start..].find('"').unwrap()];
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").unwrap()).unwrap();
    assert!(
        rels.contains(&format!("Id=\"{rid}\"")),
        "blip rid {rid} 在 document rels: {rels}"
    );
    let _ = std::fs::remove_file(&p);
}

/// 媒体清单敌意面一：悬空 image 关系（目标部件缺失）——严格拒 /
/// 兼容 BrokenReference 警告 + 资产不增。
#[test]
fn image_inventory_dangling_rel_strict_compat() {
    let (p, _media) = write_image_doc("img_dangling");
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").unwrap()).unwrap();
    let mut with_ghost = rels.clone();
    with_ghost.insert_str(
        with_ghost.len() - "</Relationships>".len(),
        "<Relationship Id=\"rIdImgGhost\" \
         Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" \
         Target=\"media/ghost.png\"/>",
    );
    let p2 = tmp("img_dangling2");
    rebuild_with_part_to(
        &p,
        &p2,
        "word/_rels/document.xml.rels",
        with_ghost.as_bytes(),
    );

    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { detail, .. },
        )) => assert!(detail.contains("ghost.png"), "{detail}"),
        other => panic!("严格应拒, got {other:?}"),
    }
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert_eq!(doc.images.len(), 1, "悬空关系不增资产");
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::BrokenReference && w.detail.contains("ghost.png")));
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 媒体清单敌意面二：媒体部件无内容类型声明——严格拒 / 兼容警告 +
/// content_type=None 保留；孤儿媒体（无关系引用）两模式保留不警告。
#[test]
fn image_inventory_undeclared_and_orphan_media() {
    let (p, _media) = write_image_doc("img_ct");

    // ① 未声明扩展（tiff 不在引擎恒发射的 png/jpeg/jpg/bmp/gif Default 集）。
    let tiff_bytes = b"II*\x00 fake tiff payload";
    let p2 = tmp("img_ct2");
    add_entries_to(&p, &p2, &[("word/media/evil.tiff", tiff_bytes.as_slice())]);
    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { detail, .. },
        )) => assert!(detail.contains("evil.tiff"), "{detail}"),
        other => panic!("严格应拒, got {other:?}"),
    }
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert_eq!(doc.images.len(), 2);
    let evil = doc
        .images
        .iter()
        .find(|a| a.part_name == "word/media/evil.tiff")
        .expect("兼容保留资产");
    assert_eq!(evil.content_type, None);
    assert!(evil.references.is_empty());
    assert!(evil.bytes == tiff_bytes);
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::MalformedXml && w.detail.contains("evil.tiff")));

    // ② 孤儿媒体（合法 png 声明、无任何关系引用）——保留 + 无警告。
    let p3 = tmp("img_ct3");
    add_entries_to(&p, &p3, &[("word/media/orphan.png", TINY_PNG.as_slice())]);
    let doc = read_docx(&p3).unwrap();
    assert_eq!(doc.images.len(), 2);
    let orphan = doc
        .images
        .iter()
        .find(|a| a.part_name == "word/media/orphan.png")
        .unwrap();
    assert_eq!(orphan.content_type.as_deref(), Some("image/png"));
    assert!(orphan.references.is_empty());
    assert_eq!(orphan.body_uses, 0);
    assert!(
        !doc.warnings.iter().any(|w| w.detail.contains("orphan")),
        "孤儿媒体不警告"
    );
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
    let _ = std::fs::remove_file(&p3);
}

/// 媒体清单敌意面三：悬空 r:embed（正文 Drawing 引用不存在的关系
/// Id）——严格拒 / 兼容 BrokenReference 警告 + 计数不增。
#[test]
fn image_inventory_dangling_embed_strict_compat() {
    let (p, _media) = write_image_doc("img_embed");
    let doc_xml_bytes = zip_read_entry(&p, "word/document.xml").unwrap();
    let doc_xml = String::from_utf8(doc_xml_bytes.clone()).unwrap();
    let start = doc_xml.find("r:embed=\"").expect("blip 在位") + 9;
    let rid_len = doc_xml[start..].find('"').unwrap();
    let rid = doc_xml[start..start + rid_len].to_string();
    // 改写 embed rid 为不存在的关系 Id（rels 保持原样——原 rid 关系
    // 仍在，资产 references 不受影响；仅 run 级引用悬空）。
    let mutated = doc_xml.replace(&format!("r:embed=\"{rid}\""), "r:embed=\"rIdGhost\"");
    assert_ne!(mutated, doc_xml);
    let p2 = tmp("img_embed2");
    rebuild_with_part_to(&p, &p2, "word/document.xml", mutated.as_bytes());

    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { detail, .. },
        )) => assert!(detail.contains("rIdGhost"), "{detail}"),
        other => panic!("严格应拒, got {other:?}"),
    }
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert_eq!(doc.images.len(), 1);
    assert_eq!(doc.images[0].references.len(), 1, "rels 侧关系不受影响");
    assert_eq!(doc.images[0].body_uses, 0, "悬空 embed 不计数");
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.kind == WarningKind::BrokenReference && w.detail.contains("rIdGhost")));
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// CRC32（测试内小实现——zip 条目头校验值用）。
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// 净室自构造 zip（stored 无压缩 + 手写本地头/中央目录/EOCD）：
/// 复制源包全部条目 + 指定部件追加同名第二副本（不同字节）。
/// zip 8.6 ZipWriter 拒绝重名（write.rs:1380-1386）——重复名档案
/// 只能低层手造（敌意面来自其他写入器的现实）。
fn rewrite_zip_stored_with_duplicate(src: &str, out: &str, dup_name: &str, dup_data: &[u8]) {
    type Buf = std::io::Cursor<Vec<u8>>;
    fn put_u16(w: &mut Buf, v: u16) {
        w.write_all(&v.to_le_bytes()).unwrap();
    }
    fn put_u32(w: &mut Buf, v: u32) {
        w.write_all(&v.to_le_bytes()).unwrap();
    }
    fn local_header(w: &mut Buf, name: &str, data: &[u8]) {
        put_u32(w, 0x0403_4B50);
        put_u16(w, 20); // version needed
        put_u16(w, 0); // flags
        put_u16(w, 0); // method stored
        put_u16(w, 0); // mtime 00:00:00
        put_u16(w, 0x21); // mdate 1980-01-01
        put_u32(w, crc32(data));
        put_u32(w, data.len() as u32);
        put_u32(w, data.len() as u32);
        put_u16(w, name.len() as u16);
        put_u16(w, 0); // extra len
        w.write_all(name.as_bytes()).unwrap();
        w.write_all(data).unwrap();
    }
    let f = std::fs::File::open(src).unwrap();
    let mut za = zip::ZipArchive::new(f).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..za.len() {
        let mut zf = za.by_index(i).unwrap();
        let name = zf.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut zf, &mut data).unwrap();
        entries.push((name, data));
    }
    let mut zw: Buf = std::io::Cursor::new(Vec::new());
    // 本地头序（重名副本紧跟原件之后）；usize::MAX = 第二副本。
    // offset 在各本地头**写前**记录（中央目录回填用）。
    let mut order: Vec<(String, usize)> = Vec::new();
    let mut cd_offsets: Vec<u32> = Vec::new();
    for (idx, (name, data)) in entries.iter().enumerate() {
        cd_offsets.push(zw.position() as u32);
        local_header(&mut zw, name, data);
        order.push((name.clone(), idx));
        if name == dup_name {
            cd_offsets.push(zw.position() as u32);
            local_header(&mut zw, name, dup_data);
            order.push((name.clone(), usize::MAX));
        }
    }
    let mut cd: Vec<u8> = Vec::new();
    for (name, idx) in &order {
        let data: &[u8] = if *idx == usize::MAX {
            dup_data
        } else {
            &entries[*idx].1
        };
        cd.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes()); // version made by
        cd.extend_from_slice(&20u16.to_le_bytes()); // version needed
        cd.extend_from_slice(&0u16.to_le_bytes()); // flags
        cd.extend_from_slice(&0u16.to_le_bytes()); // method
        cd.extend_from_slice(&0u16.to_le_bytes()); // mtime
        cd.extend_from_slice(&0x21u16.to_le_bytes()); // mdate
        cd.extend_from_slice(&crc32(data).to_le_bytes());
        cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes()); // extra
        cd.extend_from_slice(&0u16.to_le_bytes()); // comment
        cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
        cd.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        cd.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        cd.extend_from_slice(&0u32.to_le_bytes()); // offset 占位（回填）
        cd.extend_from_slice(name.as_bytes());
    }
    // 回填 offset（中央目录内逐条目）。
    let mut pos = 0usize;
    for (i, (name, _)) in order.iter().enumerate() {
        let entry_len = 46 + name.len();
        cd[pos + 42..pos + 46].copy_from_slice(&cd_offsets[i].to_le_bytes());
        pos += entry_len;
    }
    let cd_start = zw.position() as u32;
    zw.write_all(&cd).unwrap();
    // EOCD。
    put_u32(&mut zw, 0x0605_4B50);
    put_u16(&mut zw, 0); // disk
    put_u16(&mut zw, 0); // cd disk
    put_u16(&mut zw, order.len() as u16); // entries this disk
    put_u16(&mut zw, order.len() as u16); // entries total
    put_u32(&mut zw, cd.len() as u32);
    put_u32(&mut zw, cd_start);
    put_u16(&mut zw, 0); // comment len
    std::fs::write(out, zw.into_inner()).unwrap();
}

/// 媒体清单 zip 层语义钉定：重复条目名 = 最后条目胜（zip 8.6
/// IndexMap insert 后胜——read/zip_archive.rs:49-51；门 2 按索引
/// 验全部副本；读层与引擎读路径同语义）。zip 8.6 ZipWriter 拒绝
/// 重名（write.rs:1380-1386）——重复名档案须净室手造（敌意面来自
/// 其他写入器的现实，our own writer 恒不产出）。
#[test]
fn image_inventory_duplicate_zip_entries_last_wins() {
    let (p, media) = write_image_doc("img_dup");
    let last_bytes = b"\x89PNG\r\n\x1a\n__later_copy_payload__";
    let p2 = tmp("img_dup2");
    rewrite_zip_stored_with_duplicate(&p, &p2, &media, last_bytes);

    let doc = read_docx(&p2).unwrap();
    assert_eq!(doc.images.len(), 1, "重名折叠为单资产（IndexMap 键去重）");
    assert_eq!(doc.images[0].bytes, last_bytes, "最后条目胜");
    assert_eq!(doc.images[0].body_uses, 1);
    assert!(
        doc.warnings.is_empty(),
        "重复名非违面——不警告（门 2 验过两副本）"
    );
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// first/even 页眉页脚写面全环：六槽位 + 激活语义（titlePg/
/// evenAndOddHeaders）+ 原位替换（rid 保持——多槽组合下不踩踏）。
#[test]
fn first_even_header_footer_roundtrip() {
    let p = tmp("first_even");
    let mut w = DocxWriter::new();
    // 顺序刻意 first 先于 default：first 拿 rIdHeader1、default 拿
    // rIdHeader2——随后 default 槽原位替换（rid 保持 rIdHeader2）。
    // r1 版无条件 rIdHeader1 替换会让 default 槽错指 first 内容。
    w.first_header(&[RunSpec::text("首页页眉一版")]);
    w.header(&[RunSpec::text("默认页眉一版")]);
    w.header(&[RunSpec::text("默认页眉二版")]); // 原位替换
    w.even_header(&[RunSpec::text("偶页页眉")]);
    w.footer(&[RunSpec::text("默认页脚")]);
    w.first_footer(&[RunSpec::text("首页页脚")]);
    w.even_footer(&[RunSpec::text("偶页页脚")]);
    w.paragraph_text("正文。");
    w.save_to_path(&p).unwrap();

    // ① 包结构：3 页眉部件 + 3 页脚部件 + rels 恰 3+3 条（替换不增计数）。
    let f = std::fs::File::open(&p).unwrap();
    let za = zip::ZipArchive::new(f).unwrap();
    let names: Vec<String> = za.file_names().map(str::to_string).collect();
    assert!(
        names
            .iter()
            .filter(|n| n.starts_with("word/header") && n.ends_with(".xml"))
            .count()
            == 3,
        "3 页眉部件: {names:?}"
    );
    assert!(
        names
            .iter()
            .filter(|n| n.starts_with("word/footer") && n.ends_with(".xml"))
            .count()
            == 3,
        "3 页脚部件"
    );
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").unwrap()).unwrap();
    assert_eq!(
        rels.matches("relationships/header").count(),
        3,
        "恰 3 条页眉关系: {rels}"
    );
    assert_eq!(rels.matches("relationships/footer").count(), 3);

    // ② 激活语义：titlePg（first 槽）+ evenAndOddHeaders（even 槽）。
    let doc_xml = String::from_utf8(zip_read_entry(&p, "word/document.xml").unwrap()).unwrap();
    assert!(doc_xml.contains("<w:titlePg"), "{doc_xml}");
    assert_eq!(doc_xml.matches("w:headerReference").count(), 3);
    assert_eq!(doc_xml.matches("w:footerReference").count(), 3);
    let settings = String::from_utf8(zip_read_entry(&p, "word/settings.xml").unwrap()).unwrap();
    assert!(settings.contains("<w:evenAndOddHeaders"), "{settings}");

    // ③ 读回：三变体透传 + default 槽为替换后内容（rid 保持修复钉定）。
    let doc = read_docx(&p).unwrap();
    let text_of = |d: &mdc_office_io::docx::HeaderFooterData| -> String {
        match &d.blocks[0] {
            Block::Paragraph(pd) => pd.text(),
            other => panic!("应为段落, got {other:?}"),
        }
    };
    assert_eq!(doc.headers.len(), 3);
    let by_variant = |v: mdc_office_io::docx::HeaderFooterVariant| {
        doc.headers.iter().find(|h| h.variant == v).unwrap()
    };
    assert_eq!(
        text_of(by_variant(HeaderFooterVariant::Default)),
        "默认页眉二版"
    );
    assert_eq!(
        text_of(by_variant(HeaderFooterVariant::First)),
        "首页页眉一版"
    );
    assert_eq!(text_of(by_variant(HeaderFooterVariant::Even)), "偶页页眉");
    assert_eq!(doc.footers.len(), 3);
    let f_by_variant = |v: mdc_office_io::docx::HeaderFooterVariant| {
        doc.footers.iter().find(|h| h.variant == v).unwrap()
    };
    assert_eq!(
        text_of(f_by_variant(HeaderFooterVariant::Default)),
        "默认页脚"
    );
    assert_eq!(
        text_of(f_by_variant(HeaderFooterVariant::First)),
        "首页页脚"
    );
    assert_eq!(text_of(f_by_variant(HeaderFooterVariant::Even)), "偶页页脚");
    // 正文不受影响。
    match &doc.blocks[0] {
        Block::Paragraph(pd) => assert_eq!(pd.text(), "正文。"),
        other => panic!("应为段落, got {other:?}"),
    }
    assert!(doc.warnings.is_empty());
    let _ = std::fs::remove_file(&p);
}
