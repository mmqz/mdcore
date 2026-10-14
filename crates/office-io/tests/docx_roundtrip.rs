//! xlsx-r2 Phase 3 DOCX 切片集成测试（写 → 读全环 + 容器门复用钉定）。
//!
//! 验证链：
//! 1. 写读全环：DocxWriter（标题/富文本段落/表格）→ 落盘 →
//!    [`read_docx`]（含容器三门）→ 自有模型逐面断言；
//! 2. 容器门复用：DOCX 读路径走 XLSX 同一门实现——敌意 DOCX
//!    （畸形 document.xml / 谎言头 / 路径逃逸）的归类钉定 +
//!    兼容模式降级；
//! 3. 字节入口不绕门：read_docx_from_bytes 同三门；
//! 4. 供应链纪律常驻化（ADR-O1 风险表）：docx-rs 升级破坏性变更
//!    由本往返测试首跑拦截（office-e §4 观察项）。

use mdc_office_io::docx::{
    read_docx, read_docx_from_bytes_with_options, Block, DocxWriter, RunSpec,
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
