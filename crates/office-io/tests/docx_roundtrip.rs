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
    read_docx, read_docx_from_bytes_with_options, Block, BlockSource, DocxWriter,
    HeaderFooterVariant, HyperlinkTarget, ListKind, ParagraphIndent, ParagraphIndentSpecial,
    ParagraphNumbering, RunSpec,
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
        revision_inserted: false,
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

/// #276（审计 B-1）：敌意 scheme 超链接读面消毒——javascript: 降级
/// Unresolved + UnsafeLink 警告（恒登记）；白名单 scheme（https）与
/// 无 scheme 相对路径不受影响。此前敌意 docx 超链接直转
/// `[text](javascript:...)` GFM 进编辑器（wasm docx_import 消费面）。
#[test]
fn hyperlink_unsafe_scheme_sanitized_on_read() {
    let p = tmp("unsafe_link");
    let mut w = DocxWriter::new();
    w.paragraph(&[
        RunSpec::text("敌意 "),
        RunSpec::link("点我", "javascript:alert(1)"),
        RunSpec::text(" 合法 "),
        RunSpec::link("正常", "https://safe.example/x"),
        RunSpec::text("。"),
    ]);
    w.save_to_path(&p).unwrap();

    let doc = read_docx(&p).unwrap();
    // UnsafeLink 恒登记（安全面不随兼容模式放松——区别于
    // BrokenReference 的模式分叉行为）。
    let unsafe_links: Vec<_> = doc
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::UnsafeLink)
        .collect();
    assert_eq!(
        unsafe_links.len(),
        1,
        "恰一条 UnsafeLink：{:?}",
        doc.warnings
    );
    assert!(
        unsafe_links[0].detail.contains("javascript"),
        "警告含拒绝原因：{}",
        unsafe_links[0].detail
    );

    match &doc.blocks[0] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.runs.len(), 5);
            // 敌意链接降级 Unresolved（下游 link_destination 返回
            // None——渲染降级纯文本）。
            assert!(
                matches!(
                    &pd.runs[1].hyperlink,
                    Some(HyperlinkTarget::Unresolved { .. })
                ),
                "javascript: 降级 Unresolved：{:?}",
                pd.runs[1].hyperlink
            );
            // 白名单 scheme 原样透传。
            assert_eq!(
                pd.runs[3].hyperlink,
                Some(HyperlinkTarget::External {
                    target: "https://safe.example/x".into()
                }),
                "https: 不受消毒影响"
            );
            // 文本全保留（消毒只动目标，不动内容）。
            assert_eq!(pd.text(), "敌意 点我 合法 正常。");
        }
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
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
            // P2-1：页眉部件内块源 = Header（读面按部件定值）。
            assert_eq!(pd.source, BlockSource::Header);
        }
        other => panic!("页眉应为段落, got {other:?}"),
    }
    // 页脚。
    assert_eq!(doc.footers.len(), 1);
    assert_eq!(doc.footers[0].variant, HeaderFooterVariant::Default);
    match &doc.footers[0].blocks[0] {
        Block::Paragraph(pd) => {
            assert_eq!(pd.text(), "第 1 页");
            // P2-1：页脚部件内块源 = Footer。
            assert_eq!(pd.source, BlockSource::Footer);
        }
        other => panic!("页脚应为段落, got {other:?}"),
    }
    // 文档体不受影响 + P2-1：正文块源恒 Body（段/表双面钉定）。
    assert_eq!(doc.blocks.len(), 6, "sample_writer 六块");
    for b in &doc.blocks {
        match b {
            Block::Paragraph(pd) => assert_eq!(pd.source, BlockSource::Body),
            Block::Table(td) => assert_eq!(td.source, BlockSource::Body),
            Block::TextBox(tb) => assert_eq!(tb.source, BlockSource::Body),
        }
    }
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

/// 媒体清单 zip 层语义钉定（#233 行为变更：重复条目名 = **容器层
/// 守恒拒绝**）。docx-r1 时代曾钉「最后条目胜」容忍语义（zip 8.6
/// IndexMap insert 后胜——read/zip_archive.rs:49-51）；#233（审计
/// INTEG-002：zip-exotica 防线缺口）升级为拒绝——EOCD 声明计数 ≠
/// crate 吸收后条目数 = 守恒破坏（敌意载体：同名字条目的 by_name
/// 取条歧义 + 声明/实取分歧；见 tests/zip_exotica.rs 五族负向）。
/// zip 8.6 ZipWriter 拒绝重名（write.rs:1380-1386）——重复名档案须
/// 净室手造（敌意面来自其他写入器的现实，our own writer 恒不产出）。
#[test]
fn image_inventory_duplicate_zip_entries_rejected_by_conservation() {
    let (p, media) = write_image_doc("img_dup");
    let last_bytes = b"\x89PNG\r\n\x1a\n__later_copy_payload__";
    let p2 = tmp("img_dup2");
    rewrite_zip_stored_with_duplicate(&p, &p2, &media, last_bytes);

    // #233：守恒门拒绝（EOCD 声明 4 ≠ 吸收后 3——同名媒体条目对）。
    let err = read_docx(&p2).unwrap_err();
    let repr = format!("{err:?}");
    assert!(
        repr.contains("PrescanRejected") && err.to_string().contains("conservation"),
        "重复名档案 = 守恒拒绝，得 {err:?}"
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

// ---------------------------------------------------------------------------
// t20 写面五面扩（numbering / 等宽 run / 块级 code 双通道 / hr / quote）
// —— spike 探针 16 断言集（office-line/docx_write_probe.rs）的产品 API
// 移植 + 写读全环文本面回归（读模型不承载 numPr/pBdr/ind/rFonts——
// 文本零损为读面断言边界，结构面 = XML 级直断言）。
// ---------------------------------------------------------------------------

/// t20 五面 XML 结构断言（探针同款输入面 + 嵌套 quote 步进扩展）。
#[test]
fn t20_write_faces_xml_level() {
    let p = tmp("t20_faces");
    let mut w = DocxWriter::new();
    // 列表：有序 ×2（ilvl 0）+ 同 numId 嵌套 ilvl 1 + 无序 ×2。
    w.list_item(&[RunSpec::text("first ordered item")], ListKind::Ordered, 0)
        .unwrap();
    w.list_item(
        &[RunSpec::text("second ordered item")],
        ListKind::Ordered,
        0,
    )
    .unwrap();
    w.list_item(
        &[RunSpec::text("nested level-1 item")],
        ListKind::Ordered,
        1,
    )
    .unwrap();
    w.list_item(&[RunSpec::text("first bullet item")], ListKind::Bullet, 0)
        .unwrap();
    w.list_item(&[RunSpec::text("second bullet item")], ListKind::Bullet, 0)
        .unwrap();
    // 行内等宽：plain + mono + plain。
    w.paragraph(&[
        RunSpec::text("before "),
        RunSpec::styled_full("inline_code", false, false, true, false, false),
        RunSpec::text(" after"),
    ]);
    // hr：空段下边框。
    w.horizontal_rule();
    // quote：depth 1（720）+ depth 3（2160 嵌套步进）。
    w.quote_paragraph(
        &[RunSpec::text(
            "quoted text with left indent and left border",
        )],
        1,
    );
    w.quote_paragraph(&[RunSpec::text("deep quote")], 3);
    // 块级 code：双通道等宽 ×2 行。
    w.code_line("let x = 1;");
    w.code_line("let y = 2; // second line");
    w.save_to_path(&p).unwrap();

    // ① zip 结构：numbering.xml part + rels 接线（has_numbering 链——
    //    Paragraph::numbering 方法路径的打包面证据）。
    let f = std::fs::File::open(&p).unwrap();
    let za = zip::ZipArchive::new(f).unwrap();
    let names: Vec<String> = za.file_names().map(str::to_string).collect();
    drop(za);
    assert!(names.iter().any(|n| n == "word/numbering.xml"), "{names:?}");
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").unwrap()).unwrap();
    assert!(
        rels.contains("relationships/numbering") && rels.contains("numbering.xml"),
        "numbering 关系（悬空 numPr 陷阱的反证）: {rels}"
    );

    // ② numbering.xml：ordered/bullet 两抽象 + num 桥 + 9 级 + 多级 lvlText。
    let numbering = String::from_utf8(zip_read_entry(&p, "word/numbering.xml").unwrap()).unwrap();
    assert!(
        numbering.contains(r#"<w:abstractNum w:abstractNumId="100""#),
        "ordered 抽象"
    );
    assert!(numbering.contains(r#"w:numFmt w:val="decimal""#));
    assert!(numbering.contains(r#"w:lvlText w:val="%1.""#));
    assert!(
        numbering.contains(r#"<w:abstractNum w:abstractNumId="101""#),
        "bullet 抽象"
    );
    assert!(numbering.contains(r#"w:numFmt w:val="bullet""#));
    assert!(numbering.contains("\u{2022}"), "• 字形");
    assert!(
        numbering.contains(r#"<w:num w:numId="100"><w:abstractNumId w:val="100" /></w:num>"#)
            && numbering
                .contains(r#"<w:num w:numId="101"><w:abstractNumId w:val="101" /></w:num>"#),
        "num 桥"
    );
    assert!(
        numbering.contains(r#"w:lvlText w:val="%1.%2.""#),
        "多级拼号"
    );
    assert!(numbering.contains(r#"w:ilvl="8""#), "9 级（ilvl 0-8）");

    // ③ document.xml：numPr 精确配对（numId+ilvl 邻接计数）。
    let doc_xml = String::from_utf8(zip_read_entry(&p, "word/document.xml").unwrap()).unwrap();
    assert_eq!(
        doc_xml
            .matches(r#"<w:numId w:val="100" /><w:ilvl w:val="0" />"#)
            .count(),
        2,
        "ordered ilvl=0 ×2"
    );
    assert_eq!(
        doc_xml
            .matches(r#"<w:numId w:val="100" /><w:ilvl w:val="1" />"#)
            .count(),
        1,
        "ordered 嵌套 ilvl=1 ×1"
    );
    assert_eq!(
        doc_xml
            .matches(r#"<w:numId w:val="101" /><w:ilvl w:val="0" />"#)
            .count(),
        2,
        "bullet ilvl=0 ×2"
    );

    // ④ 行内等宽：run 层 rFonts 三槽 Consolas。
    assert!(
        doc_xml.contains(r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas" />"#),
        "行内 mono 通道"
    );

    // ⑤ hr：pBdr 下边框（with_empty 单边——四边全画陷阱的反证）。
    assert!(
        doc_xml.contains(
            r#"<w:pBdr><w:bottom w:val="single" w:space="1" w:sz="6" w:color="auto" /></w:pBdr>"#
        ),
        "hr 下边框"
    );

    // ⑥ quote：ind 左缩进（720×depth）+ 左边框。
    assert!(doc_xml.contains(r#"w:ind w:left="720""#), "depth 1 缩进");
    assert!(
        doc_xml.contains(r#"w:ind w:left="2160""#),
        "depth 3 嵌套步进"
    );
    assert!(
        doc_xml.contains(
            r#"<w:pBdr><w:left w:val="single" w:space="4" w:sz="12" w:color="auto" /></w:pBdr>"#
        ),
        "quote 左边框"
    );

    // ⑦ 块级 code 双通道：段落级 pPr>rPr>rFonts + run 级 rFonts。
    assert!(
        doc_xml.contains(
            r#"<w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" />"#
        ),
        "段落级通道"
    );
    assert!(
        doc_xml
            .matches(
                r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" />"#
            )
            .count()
            >= 2,
        "run 级通道 ×2 行"
    );

    // ⑧ 写读全环文本面（读模型无 numPr/pBdr/ind/rFonts 载荷——文本零损）。
    let doc = read_docx(&p).unwrap();
    let texts: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(pd) => Some(pd.text()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            "first ordered item".to_string(),
            "second ordered item".to_string(),
            "nested level-1 item".to_string(),
            "first bullet item".to_string(),
            "second bullet item".to_string(),
            "before inline_code after".to_string(),
            String::new(),
            "quoted text with left indent and left border".to_string(),
            "deep quote".to_string(),
            "let x = 1;".to_string(),
            "let y = 2; // second line".to_string(),
        ],
        "5 列表段 + 行内段 + hr 空段 + 2 引用段 + 2 code 段（文本零损）"
    );
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    let _ = std::fs::remove_file(&p);
}

/// t20：numbering 惰性注册面——无列表文档的 numbering.xml 保持引擎
/// 内建形态（无 100/101 抽象）；save_to_vec 后写器基态重置（续写新
/// 档重新注册——幂等语义钉定）。
#[test]
fn t20_numbering_lazy_registration_and_reset() {
    let mut w = DocxWriter::new();
    w.paragraph_text("no lists here");
    let bytes = w.save_to_vec().unwrap();
    let cursor = std::io::Cursor::new(&bytes[..]);
    let mut za = zip::ZipArchive::new(cursor).unwrap();
    let numbering = {
        let mut entry = za.by_name("word/numbering.xml").unwrap();
        let mut v = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut v).unwrap();
        String::from_utf8(v).unwrap()
    };
    drop(za);
    assert!(
        !numbering.contains(r#"w:abstractNumId="100""#)
            && !numbering.contains(r#"w:abstractNumId="101""#),
        "无列表档 = 内建抽象独占: {numbering}"
    );
    assert!(
        numbering.contains(r#"w:abstractNumId="1""#),
        "引擎内建抽象恒在"
    );

    // 续写新档：save 后首个 list_item 重新注册（基态重置钉定）。
    w.list_item(&[RunSpec::text("after reset")], ListKind::Bullet, 0)
        .unwrap();
    let bytes2 = w.save_to_vec().unwrap();
    let cursor = std::io::Cursor::new(&bytes2[..]);
    let mut za = zip::ZipArchive::new(cursor).unwrap();
    let numbering2 = {
        let mut entry = za.by_name("word/numbering.xml").unwrap();
        let mut v = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut v).unwrap();
        String::from_utf8(v).unwrap()
    };
    drop(za);
    assert!(
        numbering2.contains(r#"w:abstractNumId="101""#),
        "重注册在位"
    );
    let doc = read_docx_from_bytes_with_options(&bytes2, &ReadOptions::default()).unwrap();
    match &doc.blocks[0] {
        Block::Paragraph(pd) => assert_eq!(pd.text(), "after reset"),
        other => panic!("应为段落, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// t22 多行列表项续行缩进（单段落 + w:br + 段落级 hanging ind——与编号
// 抽象层级同值悬挂几何；t20 登记残余翻转为已表达）
// ---------------------------------------------------------------------------

/// t22：XML 面——一个列表项 = 一个 OOXML 段落（numPr 单点 + 行间
/// `w:br w:type="textWrapping"`），段落级 `w:ind`（left = 720×(ilvl+1)、
/// hanging 360）显式直挂（Word 自产文档惯例形态；直接 pPr 优先但与
/// numbering.xml 层级同值 = 同几何）→ br 续行起于段落 left = 首行
/// 文字列（marker 列 = left−360、tab 后文字到位）——续行对齐的
/// 结构证据。单行项既有面零漂移（无显式 ind）；写读全环 br → `\n`
///（t16 读面约定）单段文本零损。
#[test]
fn t22_list_item_multiline_xml_level() {
    let p = tmp("t22_list_multiline");
    let mut w = DocxWriter::new();
    // 单行项（既有 list_item 面——本测试的零漂移对照）。
    w.list_item(&[RunSpec::text("single")], ListKind::Bullet, 0)
        .unwrap();
    // 多行项（新面）：bullet ilvl 0（ind 720/360）。
    w.list_item_lines(
        &[
            vec![RunSpec::text("first line of item")],
            vec![RunSpec::text("second line")],
        ],
        ListKind::Bullet,
        0,
    )
    .unwrap();
    // 嵌套多行（ordered ilvl 1）：ind 步进 1440；三行含空行 → br ×2。
    w.list_item_lines(
        &[
            vec![RunSpec::text("nested head")],
            vec![],
            vec![RunSpec::text("nested tail")],
        ],
        ListKind::Ordered,
        1,
    )
    .unwrap();
    w.save_to_path(&p).unwrap();

    let doc_xml = String::from_utf8(zip_read_entry(&p, "word/document.xml").unwrap()).unwrap();
    // ① 单段落 numPr 单点：一个列表项 = 一个 OOXML 段落（非平铺多段）。
    assert_eq!(
        doc_xml.matches("<w:numPr>").count(),
        3,
        "三个列表段 = 三个项（1 单行 + 2 多行各一段）"
    );
    // ② br 软换行计数：bullet 两行 ×1 + ordered 三行（空行也 br）×2。
    assert_eq!(
        doc_xml.matches(r#"<w:br w:type="textWrapping" />"#).count(),
        3,
        "br = 行数 − 项数（空行无 run 仅 br）"
    );
    // ③ ilvl 0：numPr + 段落级 hanging ind 同段共存（邻接串断言 =
    //    同段落性证明），left = 720×(0+1)、hanging = 360。
    assert!(
        doc_xml.contains(
            r#"<w:numPr><w:numId w:val="101" /><w:ilvl w:val="0" /></w:numPr><w:ind w:left="720" w:right="0" w:hanging="360" />"#
        ),
        "bullet 多行项：numPr→ind 邻接（pPr 子序）"
    );
    // ④ ilvl 1 步进：left = 720×(1+1) = 1440，hanging 同值。
    assert!(
        doc_xml.contains(
            r#"<w:numPr><w:numId w:val="100" /><w:ilvl w:val="1" /></w:numPr><w:ind w:left="1440" w:right="0" w:hanging="360" />"#
        ),
        "ordered 嵌套多行项：ind 步进"
    );
    // ⑤ 单行项零漂移：pPr 止于 numPr（无 ind）——既有 list_item 输出
    //    面字节级不变（对照形态锚定）。
    assert_eq!(
        doc_xml
            .matches(r#"<w:numPr><w:numId w:val="101" /><w:ilvl w:val="0" /></w:numPr></w:pPr>"#)
            .count(),
        1,
        "单行项 pPr 无 ind（多行项的 ind 紧随 numPr，不命中此形）"
    );
    // ⑥ 写读全环：br → `\n`（读面约定，docx.rs:1633）——单段文本零损
    //    （空行 = 连续两 br → `\n\n`）。
    let doc = read_docx(&p).unwrap();
    let texts: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(pd) => Some(pd.text()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            "single".to_string(),
            "first line of item\nsecond line".to_string(),
            "nested head\n\nnested tail".to_string(),
        ],
        "多行项读回 = 单段含 \\n（行边界保持）；列表 kind 往返不宣称（读模型无 numPr）"
    );
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    let _ = std::fs::remove_file(&p);
}

// ---------------------------------------------------------------------------
// r4-t3 DOCX 读写保真：numPr/ind 读面承载 + 悬空纪律 + 读-改-写全链
// （方案与完整矩阵 = 分析仓 office-line/docx-r1/FIDELITY.md）
// ---------------------------------------------------------------------------

/// r4-t3：numPr/ind 写读全环承载（自有写面产线 → 读面模型断言）。
/// 列表段（ordered 两级 + bullet + 多行项）承载 num_id/ilvl/kind/format；
/// 多行项与引用段承载 ind（left/right/hanging——注意引擎发射面
/// `w:right="0"` 恒在 → 读回 right=Some(0)，写面 None ≠ 读回 Some(0)
/// 的序列化不对称如实钉定）。
#[test]
fn r4t3_numbering_and_indent_read_carry() {
    let p = tmp("r4t3_carry");
    let mut w = DocxWriter::new();
    w.list_item(&[RunSpec::text("一")], ListKind::Ordered, 0)
        .unwrap();
    w.list_item(&[RunSpec::text("嵌套")], ListKind::Ordered, 1)
        .unwrap();
    w.list_item(&[RunSpec::text("圆")], ListKind::Bullet, 0)
        .unwrap();
    w.list_item_lines(
        &[vec![RunSpec::text("多行首")], vec![RunSpec::text("多行尾")]],
        ListKind::Ordered,
        0,
    )
    .unwrap();
    w.quote_paragraph(&[RunSpec::text("被引用")], 2);
    w.paragraph_text("普通段");
    w.save_to_path(&p).unwrap();

    let doc = read_docx(&p).unwrap();
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    let paras: Vec<_> = doc
        .blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(pd) => pd.clone(),
            other => panic!("应为段落, got {other:?}"),
        })
        .collect();
    assert_eq!(paras.len(), 6);
    // ① 有序 ilvl 0：自有抽象 100 全四字段承载。
    assert_eq!(
        paras[0].numbering,
        Some(ParagraphNumbering {
            num_id: 100,
            ilvl: 0,
            kind: Some(ListKind::Ordered),
            format: Some("decimal".to_string()),
        })
    );
    assert_eq!(
        paras[0].indent, None,
        "单行列表项无显式 ind（t22 零漂移面）"
    );
    // ② 有序嵌套 ilvl 1。
    assert_eq!(
        paras[1].numbering,
        Some(ParagraphNumbering {
            num_id: 100,
            ilvl: 1,
            kind: Some(ListKind::Ordered),
            format: Some("decimal".to_string()),
        })
    );
    // ③ 无序 ilvl 0。
    assert_eq!(
        paras[2].numbering,
        Some(ParagraphNumbering {
            num_id: 101,
            ilvl: 0,
            kind: Some(ListKind::Bullet),
            format: Some("bullet".to_string()),
        })
    );
    // ④ 多行项：numPr + 段落级 hanging ind（t22 几何）双承载 + br → \n。
    assert_eq!(
        paras[3].numbering.as_ref().unwrap().kind,
        Some(ListKind::Ordered)
    );
    assert_eq!(
        paras[3].indent,
        Some(ParagraphIndent {
            left: Some(720),
            right: Some(0),
            special: Some(ParagraphIndentSpecial::Hanging(360)),
        }),
        "多行项悬挂几何读回（right=Some(0)——引擎发射面恒写 w:right）"
    );
    assert_eq!(paras[3].text(), "多行首\n多行尾");
    // ⑤ 引用段：无编号 + 左缩进 720×depth（right 同为发射面恒写）。
    assert_eq!(paras[4].numbering, None);
    assert_eq!(
        paras[4].indent,
        Some(ParagraphIndent {
            left: Some(1440),
            right: Some(0),
            special: None,
        })
    );
    // ⑥ 普通段：双承载皆 None。
    assert_eq!(paras[5].numbering, None);
    assert_eq!(paras[5].indent, None);
    let _ = std::fs::remove_file(&p);
}

/// r4-t3：悬空 numPr（numId 无 numbering.xml 条目）严格拒/兼容警告 +
/// 结构承载；自定义 numFmt（lowerLetter）解析与降级可诊断性；外源
/// `w:ind` 形态（firstLine——写面不产的形态）注入承载。净室注入：
/// document.xml sectPr 前追加悬空编号段（带 firstLine ind）+
/// numbering.xml `decimal` → `lowerLetter` 改写。
#[test]
fn r4t3_numbering_dangling_custom_format_injected_ind() {
    let p = tmp("r4t3_dangle");
    let mut w = DocxWriter::new();
    w.paragraph_text("基准段");
    w.list_item(&[RunSpec::text("有序项")], ListKind::Ordered, 0)
        .unwrap();
    w.save_to_path(&p).unwrap();

    // ① 悬空 numPr 注入（末段——sectPr 前）。
    let mut xml = String::from_utf8(zip_read_entry(&p, "word/document.xml").unwrap()).unwrap();
    let pos = xml.find("<w:sectPr>").expect("sectPr 在位");
    xml.insert_str(
        pos,
        "<w:p><w:pPr><w:numPr><w:numId w:val=\"777\" /><w:ilvl w:val=\"2\" /></w:numPr>\
         <w:ind w:left=\"360\" w:right=\"120\" w:firstLine=\"480\" /></w:pPr>\
         <w:r><w:t>悬空编号段</w:t></w:r></w:p>",
    );
    let p2 = tmp("r4t3_dangle2");
    rebuild_with_part_to(&p, &p2, "word/document.xml", xml.as_bytes());

    // 严格：typed 拒（与超链接/页眉页脚悬空引用同族纪律）。
    match read_docx(&p2) {
        Err(mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::InvalidPart { part, detail },
        )) => {
            assert_eq!(part, "word/document.xml");
            assert!(
                detail.contains("numId '777'") && detail.contains("dangling"),
                "{detail}"
            );
        }
        other => panic!("严格应拒悬空 numId, got {other:?}"),
    }
    // 兼容：BrokenReference 警告 + kind=None 结构承载（num_id/ilvl 不丢）
    // + firstLine ind 承载 + 文本保留。
    let compat = ReadOptions { compat: true };
    let doc = read_docx_with(&p2, &compat).unwrap();
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.kind == WarningKind::BrokenReference
                && w.detail.contains("numId '777'")
                && w.detail.contains("dangling")),
        "{:?}",
        doc.warnings
    );
    let last = match doc.blocks.last().unwrap() {
        Block::Paragraph(pd) => pd.clone(),
        other => panic!("应为段落, got {other:?}"),
    };
    assert_eq!(
        last.numbering,
        Some(ParagraphNumbering {
            num_id: 777,
            ilvl: 2,
            kind: None,
            format: None,
        }),
        "结构承载（kind 解析不可达 = 降级可诊断）"
    );
    assert_eq!(
        last.indent,
        Some(ParagraphIndent {
            left: Some(360),
            right: Some(120),
            special: Some(ParagraphIndentSpecial::FirstLine(480)),
        }),
        "外源 w:ind（firstLine 形态——写面不产的形态）承载"
    );
    assert_eq!(last.text(), "悬空编号段");

    // ② 自定义 numFmt：numbering.xml 的 decimal 全量改写为 lowerLetter
    //    → 有序项 kind 仍 Ordered（解析成功），format = lowerLetter
    //    （写回经自有抽象降级为 decimal——format 字段使降级可诊断）。
    let numbering = String::from_utf8(zip_read_entry(&p, "word/numbering.xml").unwrap()).unwrap();
    let custom = numbering.replace(
        r#"w:numFmt w:val="decimal""#,
        r#"w:numFmt w:val="lowerLetter""#,
    );
    assert_ne!(numbering, custom, "改写命中（抽象含 decimal 级）");
    let p3 = tmp("r4t3_custom");
    rebuild_with_part_to(&p, &p3, "word/numbering.xml", custom.as_bytes());
    let doc3 = read_docx(&p3).unwrap();
    assert!(doc3.warnings.is_empty(), "{:?}", doc3.warnings);
    match &doc3.blocks[1] {
        Block::Paragraph(pd) => assert_eq!(
            pd.numbering,
            Some(ParagraphNumbering {
                num_id: 100,
                ilvl: 0,
                kind: Some(ListKind::Ordered),
                format: Some("lowerLetter".to_string()),
            }),
            "自定义 numFmt 解析（kind 归 Ordered 域；format 保留原生值）"
        ),
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
    let _ = std::fs::remove_file(&p3);
}

/// r4-t3 验证链主测试：读 → **真实修改**（改一个 run 的文本）→ 写 →
/// 重读 → 断言目标特性保真（numPr 结构 kind/ilvl/format + ind）+
/// 非目标内容不意外删除（ZIP 部件对比 + 关系对比——预期丢失集恰为
/// 登记面：图片媒体部件/图片关系 + 脚注关系〔模型不承载其内容〕；
/// footnotes.xml 部件本身引擎恒发——内容降级为空壳，脚注内容丢失
/// 在内容面断言）。
#[test]
fn r4t3_roundtrip_read_modify_write() {
    use mdc_office_io::docx::HeaderFooterVariant;

    // ---- 阶段 1：fixture（含登记丢失成员——脚注与图片）。----
    let p = tmp("r4t3_rt");
    let mut w = DocxWriter::new();
    w.heading(1, "RT 标题").unwrap();
    w.paragraph(&[
        RunSpec::text("前 "),
        RunSpec::styled("粗斜", true, true),
        RunSpec::link("站", "https://rt.example/a?x=1&y=2"),
        RunSpec::text(" 后"),
    ]);
    w.paragraph_text("MUTATE_ME 段");
    w.quote_paragraph(&[RunSpec::text("被引用")], 2);
    w.list_item(&[RunSpec::text("一")], ListKind::Ordered, 0)
        .unwrap();
    w.list_item(&[RunSpec::text("嵌套")], ListKind::Ordered, 1)
        .unwrap();
    w.list_item(&[RunSpec::text("圆")], ListKind::Bullet, 0)
        .unwrap();
    w.list_item_lines(
        &[vec![RunSpec::text("多行首")], vec![RunSpec::text("多行尾")]],
        ListKind::Ordered,
        0,
    )
    .unwrap();
    w.table(&[
        vec!["a".to_string(), "b".to_string()],
        vec!["c".to_string(), "d".to_string()],
    ]);
    // 登记丢失成员：脚注（读面引擎不读 footnotes.xml 内容）与图片
    //（清单面在但块序位置无承载 → 不回放）。
    w.paragraph(&[RunSpec::footnote("脚注内容甲")]);
    w.image(&TINY_PNG, 1, 1).unwrap();
    w.header(&[RunSpec::text("页眉甲")]);
    w.footer(&[RunSpec::text("页脚甲")]);
    w.save_to_path(&p).unwrap();

    // ---- 阶段 2：读取 + 模型断言（承载面）。----
    let mut doc = read_docx(&p).unwrap();
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    assert_eq!(
        doc.blocks.len(),
        11,
        "9 内容块 + 脚注 marker 空段 + 图片空锚段"
    );
    let para = |i: usize| match &doc.blocks[i] {
        Block::Paragraph(pd) => pd.clone(),
        other => panic!("blocks[{i}] 应为段落, got {other:?}"),
    };
    assert_eq!(para(0).style.as_deref(), Some("Heading1"));
    assert_eq!(para(1).runs.len(), 4);
    assert!(para(1).runs[1].bold && para(1).runs[1].italic);
    assert_eq!(
        para(1).runs[2].hyperlink,
        Some(HyperlinkTarget::External {
            target: "https://rt.example/a?x=1&y=2".into()
        })
    );
    assert_eq!(para(2).text(), "MUTATE_ME 段");
    assert_eq!(
        para(3).indent.map(|i| i.left),
        Some(Some(1440)),
        "引用 ind 承载"
    );
    assert_eq!(
        para(4).numbering.as_ref().unwrap().kind,
        Some(ListKind::Ordered)
    );
    assert_eq!(para(4).numbering.as_ref().unwrap().ilvl, 0);
    assert_eq!(para(5).numbering.as_ref().unwrap().ilvl, 1, "嵌套层级承载");
    assert_eq!(
        para(6).numbering.as_ref().unwrap().kind,
        Some(ListKind::Bullet)
    );
    assert_eq!(para(7).text(), "多行首\n多行尾", "br → \\n 行边界保持");
    assert!(matches!(&doc.blocks[8], Block::Table(t) if t.rows.len() == 2 && t.rows[0][0] == "a"));
    // 脚注 marker 段（引擎读面不解析 w:footnoteReference → run 在而
    // 无内容面）与图片锚段（Drawing 面外 → 空文本 run）——同为
    // 「块序在位、载荷空」的读面登记形态。
    assert_eq!(para(9).runs.len(), 1);
    assert_eq!(
        para(9).runs[0].text,
        "",
        "脚注 marker run 文本空（面外登记）"
    );
    assert_eq!(para(10).runs.len(), 1);
    assert_eq!(
        para(10).runs[0].text,
        "",
        "图片锚段 run 文本空（Drawing 面外）"
    );
    assert_eq!(doc.images.len(), 1);
    assert_eq!(doc.images[0].body_uses, 1);
    assert_eq!(doc.headers.len(), 1);
    assert_eq!(doc.headers[0].variant, HeaderFooterVariant::Default);
    assert_eq!(doc.footers.len(), 1);

    // ---- 阶段 3：真实修改（一个 run 的文本）。----
    doc.blocks[2] = Block::Paragraph({
        let mut pd = para(2);
        pd.runs[0].text = "MUTATED 就绪".to_string();
        pd
    });
    let doc = &doc;

    // ---- 阶段 4：模型 → DocxWriter 重建（消费方映射示范）。----
    fn runs_to_specs(runs: &[mdc_office_io::docx::RunData]) -> Vec<RunSpec> {
        runs.iter()
            .map(|r| {
                let mut s = RunSpec::styled_full(&r.text, r.bold, r.italic, false, false, false);
                match &r.hyperlink {
                    // T6（office-g Phase 1）：Anchor 变体不再静默丢——
                    // 读回的内部锚点链接在回放面保真。
                    Some(HyperlinkTarget::External { target }) => {
                        s.link = Some(target.clone());
                    }
                    Some(HyperlinkTarget::Anchor { anchor }) => {
                        s.anchor = Some(anchor.clone());
                    }
                    // Internal（包内相对路径）/Unresolved（悬空 r:id，
                    // 仅兼容模式）无 RunSpec 承载面——维持回放静默丢
                    //（读面 BrokenReference 警告已在读侧可观测）。
                    Some(HyperlinkTarget::Internal { .. })
                    | Some(HyperlinkTarget::Unresolved { .. })
                    | None => {}
                }
                s
            })
            .collect()
    }
    let mut w2 = DocxWriter::new();
    for block in &doc.blocks {
        match block {
            Block::Paragraph(pd) => {
                if pd.runs.is_empty() || pd.text().is_empty() {
                    // 空载荷段不回放（脚注 marker 段/图片锚段——内容 =
                    // 登记丢失集；本 fixture 中源文档无其他空段）。
                    continue;
                }
                // 列表段：kind/ilvl 承载驱动回放（多行项按 \n 切行）。
                if let Some(n) = &pd.numbering {
                    let kind = n.kind.unwrap_or(ListKind::Ordered); // 未解析回退面（本 fixture 恒 Some）
                    let ilvl = n.ilvl.min(mdc_office_io::docx::MAX_LIST_LEVEL);
                    if pd.text().contains('\n') {
                        let lines: Vec<Vec<RunSpec>> = pd
                            .text()
                            .split('\n')
                            .map(|l| vec![RunSpec::text(l)])
                            .collect();
                        w2.list_item_lines(&lines, kind, ilvl).unwrap();
                    } else {
                        w2.list_item(&runs_to_specs(&pd.runs), kind, ilvl).unwrap();
                    }
                    continue;
                }
                // 标题段（样式 ID 语义解释 = 消费方责任面的示范）。
                if let Some(style) = &pd.style {
                    if let Some(level) = style
                        .strip_prefix("Heading")
                        .and_then(|s| s.parse::<u8>().ok())
                    {
                        w2.heading(level, &pd.text()).unwrap();
                        continue;
                    }
                }
                // 引用段（ind 承载驱动：left = 720×depth——本 fixture 几何）。
                if let Some(ind) = &pd.indent {
                    if ind.left == Some(1440) {
                        w2.quote_paragraph(&runs_to_specs(&pd.runs), 2);
                        continue;
                    }
                }
                w2.paragraph(&runs_to_specs(&pd.runs));
            }
            Block::Table(t) => w2.table(&t.rows),
            // #18：文本框（写面不回放——读面加性承载；往返丢集登记
            // 与脚注/图片锚段同语义）。
            Block::TextBox(_) => {}
        }
    }
    for h in &doc.headers {
        match h.variant {
            HeaderFooterVariant::Default => {
                let specs: Vec<RunSpec> = h
                    .blocks
                    .iter()
                    .flat_map(|b| match b {
                        Block::Paragraph(pd) => runs_to_specs(&pd.runs),
                        _ => vec![],
                    })
                    .collect();
                w2.header(&specs);
            }
            other => panic!("fixture 仅 default 槽, got {other:?}"),
        }
    }
    for f in &doc.footers {
        let specs: Vec<RunSpec> = f
            .blocks
            .iter()
            .flat_map(|b| match b {
                Block::Paragraph(pd) => runs_to_specs(&pd.runs),
                _ => vec![],
            })
            .collect();
        w2.footer(&specs);
    }
    let p_out = tmp("r4t3_rt_out");
    w2.save_to_path(&p_out).unwrap();

    // ---- 阶段 5：重读断言（目标特性保真）。----
    let doc2 = read_docx(&p_out).unwrap();
    assert!(doc2.warnings.is_empty(), "{:?}", doc2.warnings);
    let para2 = |i: usize| match &doc2.blocks[i] {
        Block::Paragraph(pd) => pd.clone(),
        other => panic!("blocks[{i}] 应为段落, got {other:?}"),
    };
    assert_eq!(doc2.blocks.len(), 9, "9 块（脚注/图片锚段不回放）");
    assert_eq!(para2(0).style.as_deref(), Some("Heading1"), "标题结构保持");
    assert_eq!(para2(0).text(), "RT 标题");
    assert_eq!(para2(1).text(), "前 粗斜站 后", "行内文本零损");
    assert!(
        para2(1).runs[1].bold && para2(1).runs[1].italic,
        "行内格式保持"
    );
    assert_eq!(
        para2(1).runs[2].hyperlink,
        Some(HyperlinkTarget::External {
            target: "https://rt.example/a?x=1&y=2".into()
        }),
        "超链接目标保持"
    );
    assert_eq!(para2(2).text(), "MUTATED 就绪", "真实修改生效且落盘往返");
    assert_eq!(
        para2(3).indent.map(|i| i.left),
        Some(Some(1440)),
        "引用 ind 往返"
    );
    // numPr 全四字段往返（自有抽象 numId 100/101 + decimal/bullet 域）。
    assert_eq!(
        para2(4).numbering,
        Some(ParagraphNumbering {
            num_id: 100,
            ilvl: 0,
            kind: Some(ListKind::Ordered),
            format: Some("decimal".to_string()),
        })
    );
    assert_eq!(para2(5).numbering.as_ref().unwrap().ilvl, 1, "嵌套层级往返");
    assert_eq!(
        para2(6).numbering.as_ref().unwrap().kind,
        Some(ListKind::Bullet),
        "列表 kind 往返"
    );
    assert_eq!(para2(7).text(), "多行首\n多行尾", "多行项行边界往返");
    assert_eq!(
        para2(7).indent,
        Some(ParagraphIndent {
            left: Some(720),
            right: Some(0),
            special: Some(ParagraphIndentSpecial::Hanging(360)),
        }),
        "多行项悬挂几何往返"
    );
    assert!(matches!(&doc2.blocks[8], Block::Table(t) if t.rows.len() == 2 && t.rows[0][0] == "a"));
    assert_eq!(doc2.headers.len(), 1, "页眉块回放");
    assert_eq!(
        match &doc2.headers[0].blocks[0] {
            Block::Paragraph(pd) => pd.text(),
            other => panic!("应为段落, got {other:?}"),
        },
        "页眉甲"
    );
    assert_eq!(
        match &doc2.footers[0].blocks[0] {
            Block::Paragraph(pd) => pd.text(),
            other => panic!("应为段落, got {other:?}"),
        },
        "页脚甲"
    );
    assert!(doc2.images.is_empty(), "图片未回放（位置无承载——登记）");

    // ---- 阶段 6：ZIP 部件对比（预期丢失集恰 = 登记面）。----
    let parts_of = |path: &str| -> Vec<String> {
        let f = std::fs::File::open(path).unwrap();
        let za = zip::ZipArchive::new(f).unwrap();
        let mut v: Vec<String> = za
            .file_names()
            .filter(|n| !n.ends_with('/'))
            .map(str::to_string)
            .collect();
        v.sort();
        v
    };
    let orig_parts = parts_of(&p);
    let rebuilt_parts = parts_of(&p_out);
    // 丢失 = 恰图片媒体部件（脚注部件引擎恒发不丢——内容面另行断言）。
    let lost: Vec<String> = orig_parts
        .iter()
        .filter(|n| !rebuilt_parts.contains(n))
        .cloned()
        .collect();
    assert_eq!(lost.len(), 1, "预期丢失集恰为媒体部件: {lost:?}");
    assert!(lost[0].starts_with("word/media/"), "{lost:?}");
    // 新增部件 = 零（重建不引入模型外内容）。
    let added: Vec<String> = rebuilt_parts
        .iter()
        .filter(|n| !orig_parts.contains(n))
        .cloned()
        .collect();
    assert_eq!(added, Vec::<String>::new(), "无新增部件");

    // ---- 阶段 7：关系对比（(Type, Target) 对——Id 引擎全局计数不比）。----
    let rels_pairs_of = |path: &str| -> Vec<(String, String)> {
        let rels = String::from_utf8(
            zip_read_entry(path, "word/_rels/document.xml.rels").expect("rels 在位"),
        )
        .unwrap();
        let mut out = Vec::new();
        for line in rels.split("><") {
            // split("><") 剥离了分隔符——关系条目段以 `Relationship Id=` 起
            //（无前导 `<`），以 Type 属性存在性过滤（根元素段无 Type）。
            if !line.contains("Type=\"") {
                continue;
            }
            let ty = line
                .split("Type=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap_or_default()
                .to_string();
            let tg = line
                .split("Target=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap_or_default()
                .to_string();
            out.push((ty, tg));
        }
        out.sort();
        out
    };
    let orig_rels = rels_pairs_of(&p);
    let rebuilt_rels = rels_pairs_of(&p_out);
    let mut lost_rels: Vec<_> = orig_rels
        .iter()
        .filter(|r| !rebuilt_rels.contains(r))
        .cloned()
        .collect();
    lost_rels.sort();
    assert_eq!(
        lost_rels.len(),
        2,
        "预期关系丢失集 = 图片关系 + 脚注关系: {lost_rels:?}"
    );
    assert!(
        lost_rels.iter().any(|(t, _)| t.ends_with("/image")),
        "{lost_rels:?}"
    );
    assert!(
        lost_rels.iter().any(|(t, _)| t.ends_with("/footnotes")),
        "{lost_rels:?}"
    );
    let added_rels: Vec<_> = rebuilt_rels
        .iter()
        .filter(|r| !orig_rels.contains(r))
        .cloned()
        .collect();
    assert_eq!(added_rels, Vec::<(String, String)>::new(), "无新增关系");
    // 超链接/numbering/header/footer 关系保持（逐对在位）。
    // rels 属性值 = XML 原文（实体形 `&amp;`——读面联接时的实体补偿
    // 是另一层语义，hyperlink_roundtrip_external 已钉定；此处按原文比）。
    for (t, tg) in [
        ("hyperlink", "https://rt.example/a?x=1&amp;y=2"),
        ("numbering", "numbering.xml"),
        ("header", "header1.xml"),
        ("footer", "footer1.xml"),
    ] {
        assert!(
            rebuilt_rels
                .iter()
                .any(|(ty, target)| ty.ends_with(t) && target == tg),
            "{t} 关系保持: {rebuilt_rels:?}"
        );
    }

    // ---- 阶段 8：登记丢失的内容面诊断（非静默——断言其恰在预期处
    //      丢失）：原档脚注内容在 footnotes.xml；重建档空壳无内容。----
    let orig_fn = String::from_utf8(zip_read_entry(&p, "word/footnotes.xml").unwrap()).unwrap();
    assert!(orig_fn.contains("脚注内容甲"), "原档脚注内容在位（对照）");
    let rebuilt_fn =
        String::from_utf8(zip_read_entry(&p_out, "word/footnotes.xml").unwrap()).unwrap();
    assert!(
        !rebuilt_fn.contains("脚注内容甲"),
        "重建档脚注空壳（内容丢失 = 读面登记面，FIDELITY 矩阵行）"
    );

    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p_out);
}

// ---------------------------------------------------------------------------
// r5-t2 探针：超链接四类目标 + 书签定义的读面行为钉定（T2.2 证据基座
// ——锚点写回裁决的「未实测不宣称」底稿；全部只增，不改既有断言）
// ---------------------------------------------------------------------------

/// 探针①：**包内目标链接**（关系无 TargetMode）。引擎自身只收带
/// TargetMode 的超链接关系进 `Docx.hyperlinks`（CAPABILITY_NOTES §1
/// 「仅 TargetMode 属性存在的关入表」），但本层读面走自有 rels 层
/// （RelationshipTable 全量收条目）——整包面钉定：r:id → 无
/// TargetMode 关系 → [`HyperlinkTarget::Internal`]（目标原样透传，
/// 不解析为包绝对路径）。函数级分派已有内联单测
///（docx.rs `hyperlink_resolution_dispatch`），此处为全链包级证据。
#[test]
fn r5t2_hyperlink_internal_target_read() {
    let p = tmp("r5t2_int");
    let mut w = DocxWriter::new();
    w.paragraph(&[RunSpec::link("包内链", "https://placeholder.example/")]);
    w.save_to_path(&p).unwrap();

    // 改写 rels：Target 改包内相对路径 + 剥 TargetMode（外部关系 →
    // 包内目标关系；OPC TargetMode 缺省语义 = Internal）。
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").expect("rels 在位"))
            .unwrap();
    let patched = rels
        .replace("https://placeholder.example/", "glossary.docx#term")
        .replace(" TargetMode=\"External\"", "");
    assert_ne!(
        patched, rels,
        "改写生效前置：占位目标与 TargetMode 均在原文中"
    );
    let p2 = tmp("r5t2_int2");
    rebuild_with_part_to(&p, &p2, "word/_rels/document.xml.rels", patched.as_bytes());

    // 严格模式即过（TargetMode 可选——OPC 合法形，非降级面）。
    let doc = read_docx(&p2).unwrap();
    assert!(doc.warnings.is_empty(), "合法包零警告: {:?}", doc.warnings);
    match &doc.blocks[0] {
        Block::Paragraph(pd) => assert_eq!(
            pd.runs[0].hyperlink,
            Some(HyperlinkTarget::Internal {
                target: "glossary.docx#term".into()
            }),
            "无 TargetMode 关系 → Internal（罕见面：Word 常规外链均带 External）"
        ),
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 探针②：**锚点优先于 r:id**（同元素双属性）。引擎读面先查
/// `w:anchor`（reader/hyperlink.rs:14-20——`anchor.is_some()` 分支在
/// 前），r:id 即使可解析（关系在位）也被忽略——钉定该优先序 + 零
/// 警告（双属性形非畸形）。
#[test]
fn r5t2_hyperlink_anchor_wins_over_rid() {
    let p = tmp("r5t2_prec");
    let mut w = DocxWriter::new();
    // 先写一个真外链（产生可解析的 hyperlink 关系条目），再注入
    // 双属性超链接段引用同一 rId。
    w.paragraph(&[RunSpec::link("真外链", "https://real.example/")]);
    w.save_to_path(&p).unwrap();

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

    let doc_xml = zip_read_entry(&p, "word/document.xml").unwrap();
    let mut xml = String::from_utf8(doc_xml.clone()).unwrap();
    let sect = "<w:sectPr>";
    let pos = xml.find(sect).expect("sectPr 在位");
    xml.insert_str(
        pos,
        &format!(
            "<w:p><w:hyperlink w:anchor=\"top\" r:id=\"{hl_rid}\" w:history=\"1\">\
             <w:r><w:t>双属性链</w:t></w:r></w:hyperlink></w:p>"
        ),
    );
    let p2 = rebuild_with_part(&p, "word/document.xml", xml.as_bytes());

    let doc = read_docx(&p2).unwrap();
    assert!(
        doc.warnings.is_empty(),
        "双属性形非畸形零警告: {:?}",
        doc.warnings
    );
    let last = doc.blocks.last().expect("注入段在位");
    match last {
        Block::Paragraph(pd) => {
            assert_eq!(pd.runs[0].text, "双属性链");
            assert_eq!(
                pd.runs[0].hyperlink,
                Some(HyperlinkTarget::Anchor {
                    anchor: "top".into()
                }),
                "w:anchor 优先——可解析 r:id 亦被忽略（引擎行为镜像）"
            );
        }
        other => panic!("应为段落, got {other:?}"),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 探针③：**书签定义读面行为**（w:bookmarkStart/End）。三面钉定：
/// (a) 书签定义本身静默丢弃（引擎解析进模型 reader/paragraph.rs:56-65，
///     本层 extract_paragraph 跳过 docx.rs:1912——文本保留、零警告、
///     无模型字段）；(b) 锚点链接目标存在与否**读面不可辨**——
///     Anchor 变体恒为字符串透传（引擎无书签存在性校验，OOXML 悬空
///     锚点合法形）；(c) 被书签包裹的 run 文本零损。
#[test]
fn r5t2_bookmark_definitions_dropped_anchor_carried() {
    let p = tmp("r5t2_bm");
    sample_writer().save_to_path(&p).unwrap();
    let doc_xml = zip_read_entry(&p, "word/document.xml").unwrap();
    let mut xml = String::from_utf8(doc_xml.clone()).unwrap();
    let sect = "<w:sectPr>";
    let pos = xml.find(sect).expect("sectPr 在位");
    // 三段注入：书签定义段（Word 常规形：书签包裹 run）+ 指向存在
    // 书签的锚点链 + 指向不存在书签的悬空锚点链。
    xml.insert_str(
        pos,
        "<w:p><w:bookmarkStart w:id=\"7\" w:name=\"sec1\" />\
         <w:r><w:t>被书签段落</w:t></w:r>\
         <w:bookmarkEnd w:id=\"7\" /></w:p>\
         <w:p><w:hyperlink w:anchor=\"sec1\" w:history=\"1\">\
         <w:r><w:t>到节一</w:t></w:r></w:hyperlink></w:p>\
         <w:p><w:hyperlink w:anchor=\"no_such_bookmark\" w:history=\"1\">\
         <w:r><w:t>悬空锚</w:t></w:r></w:hyperlink></w:p>",
    );
    let p2 = rebuild_with_part(&p, "word/document.xml", xml.as_bytes());

    let doc = read_docx(&p2).unwrap();
    // (a) 书签 = 合法 OOXML：零警告（非降级面——静默跳过，登记面）。
    assert!(
        doc.warnings.is_empty(),
        "书签为合法形零警告: {:?}",
        doc.warnings
    );
    let n = doc.blocks.len();
    let tail: Vec<&Block> = doc.blocks[n - 3..].iter().collect();
    // 注入序（文档序）：被书签段落 / 到节一 / 悬空锚。
    match tail[0] {
        Block::Paragraph(pd) => {
            // (c) 被书签包裹的 run 文本零损；无目标挂接。
            assert_eq!(pd.runs.len(), 1);
            assert_eq!(pd.runs[0].text, "被书签段落");
            assert_eq!(pd.runs[0].hyperlink, None, "书签定义不产目标");
            // (a) 模型无书签字段——可断言面 = 段落数与文本（定义本身
            // 无残留通道；读取丢失登记 FIDELITY §3）。
        }
        other => panic!("应为段落, got {other:?}"),
    }
    // (b) 目标存在 vs 不存在 → 读面同形（Anchor 字符串透传）。
    for (i, anchor) in [(1usize, "sec1"), (2, "no_such_bookmark")] {
        match tail[i] {
            Block::Paragraph(pd) => {
                assert_eq!(pd.runs[0].text, if i == 1 { "到节一" } else { "悬空锚" });
                assert_eq!(
                    pd.runs[0].hyperlink,
                    Some(HyperlinkTarget::Anchor {
                        anchor: anchor.into()
                    }),
                    "锚点链接恒 Anchor 透传——目标存在性读面不可辨"
                );
            }
            other => panic!("应为段落, got {other:?}"),
        }
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(&p2);
}

/// 探针⑤（引擎直构 spike——r5-t2 锚点写回裁决的一手风险排除）：
/// docx-rs 引擎写面直构（**不经 DocxWriter**——公开模型不扩的红线
/// 下，先证引擎面可行性；同 t20 WRITE_FACE_PROBE spike 纪律的产品
/// 测试内嵌版）。三段：外链对照 / 锚点链接 / 书签定义段。四面断言：
/// (a) 锚点链接 XML = `w:anchor` 属性形（**无 r:id**）；
/// (b) 锚点**不进 rels**——update_dependencies 只收 External 进
///     hyperlink_map（documents/mod.rs:1014-1017），rels 超链接条目
///     数 = 恰 1（外链对照）；锚点写 = 自含面（无懒注册/rels 接线
///     陷阱——比 t20 numbering 面简单）；
/// (c) 书签定义 XML = `w:bookmarkStart/End` 对（id 配对 + name）；
/// (d) 引擎产出整包过**本层读面**：Anchor 透传 + 书签段零警告 +
///     文本零损——「引擎写 → 自有读」全环可行（RunSpec 扩面的实施
///     面 = 纯包装层胶水，无引擎障碍、无 HYPERLINK 域 todo!() 面
///     〔t14 红线——w:anchor 属性形与域形是两条不相交路径〕）。
#[test]
fn r5t2_engine_anchor_and_bookmark_write_spike() {
    let ext_link_para = docx_rs::Paragraph::new().add_hyperlink(
        docx_rs::Hyperlink::new("https://ext.example/", docx_rs::HyperlinkType::External)
            .add_run(docx_rs::Run::new().add_text("外链")),
    );
    let anchor_link_para = docx_rs::Paragraph::new().add_hyperlink(
        docx_rs::Hyperlink::new("sec1", docx_rs::HyperlinkType::Anchor)
            .add_run(docx_rs::Run::new().add_text("到节一")),
    );
    // 书签包裹段（引擎 API 尾插语义：调用序 = children 序）。
    let bookmarked_para = docx_rs::Paragraph::new()
        .add_bookmark_start(7, "sec1")
        .add_run(docx_rs::Run::new().add_text("被书签段落"))
        .add_bookmark_end(7);

    let engine_doc = docx_rs::Docx::new()
        .add_paragraph(ext_link_para)
        .add_paragraph(anchor_link_para)
        .add_paragraph(bookmarked_para);
    let mut cursor = std::io::Cursor::new(Vec::new());
    engine_doc.pack(&mut cursor).expect("engine pack");
    let bytes = cursor.into_inner();
    let p = tmp("r5t2_spike");
    std::fs::write(&p, &bytes).unwrap();

    // (a) 锚点 XML 形：w:anchor 属性、无 r:id。
    let xml = String::from_utf8(zip_read_entry(&p, "word/document.xml").unwrap()).unwrap();
    assert!(
        xml.contains(r#"<w:hyperlink w:anchor="sec1" w:history="1">"#),
        "锚点形 = w:anchor 属性（引擎 BuildXML 测试同构）: {xml}"
    );
    // (c) 书签定义 XML 对。
    assert!(
        xml.contains(r#"<w:bookmarkStart w:id="7" w:name="sec1" />"#)
            && xml.contains(r#"<w:bookmarkEnd w:id="7" />"#),
        "书签 Start/End 配对发射: {xml}"
    );
    // (b) rels 自含性：超链接条目恰 1（外链对照）——锚点零 rels 面。
    let rels =
        String::from_utf8(zip_read_entry(&p, "word/_rels/document.xml.rels").expect("rels 在位"))
            .unwrap();
    let hyperlink_rels = rels
        .lines()
        .filter(|l| l.contains("relationships/hyperlink"))
        .count();
    assert_eq!(
        hyperlink_rels, 1,
        "锚点不进 rels（外链对照恰 1 条）: {rels}"
    );
    assert!(!rels.contains("sec1"), "锚点名不出现在关系面: {rels}");

    // (d) 引擎写 → 本层读全环：三段承载 + 零警告。
    let doc = read_docx(&p).unwrap();
    assert!(
        doc.warnings.is_empty(),
        "引擎自产包零警告: {:?}",
        doc.warnings
    );
    assert_eq!(doc.blocks.len(), 3);
    let targets: Vec<Option<HyperlinkTarget>> = doc
        .blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(pd) => pd.runs[0].hyperlink.clone(),
            other => panic!("应为段落, got {other:?}"),
        })
        .collect();
    assert_eq!(
        targets[0],
        Some(HyperlinkTarget::External {
            target: "https://ext.example/".into()
        }),
        "外链对照经 rels 联接"
    );
    assert_eq!(
        targets[1],
        Some(HyperlinkTarget::Anchor {
            anchor: "sec1".into()
        }),
        "锚点写 → 读 Anchor 透传（全环可行）"
    );
    assert_eq!(targets[2], None, "书签定义不产链接目标");
    let texts: Vec<String> = doc
        .blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(pd) => pd.text(),
            other => panic!("应为段落, got {other:?}"),
        })
        .collect();
    assert_eq!(texts, vec!["外链", "到节一", "被书签段落"]);
    let _ = std::fs::remove_file(&p);
}
