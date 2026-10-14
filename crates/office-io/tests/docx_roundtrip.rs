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
