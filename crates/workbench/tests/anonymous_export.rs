//! G3 暗标匿名导出（issue #6 / 标书 R-11 BLOCKER）——集成验证。
//!
//! 覆盖面（全部合成数据）：
//! - **文件会话**：`export_anonymous` 落地产物 = docProps 暗标全清 +
//!   正文部件逐字节同源 + 可解析 + 原子发布（tmp 不残留）+ 审计落账；
//! - **内存会话**：`export_anonymous_bytes` 字节返回版同语义；
//! - **同门复用**：带工作台标记的正文 → 匿名导出同拒（MarkerFound
//!   ——干净导出零标记门是匿名导出的前置门）；
//! - **命令层**：`WorkbenchCommand::ExportAnonymous` dispatch 走通 +
//!   审计事件 `anonymous_export` 在 sidecar。

use std::path::PathBuf;

use mdc_office_io::docx::{docx_part_bytes, DocxWriter};
use mdc_workbench::{PathPolicy, Workbench, WorkbenchCommand, WorkbenchError};

fn policy_for(dir: &std::path::Path) -> PathPolicy {
    PathPolicy::new(&[dir.to_path_buf()]).expect("temp dir root")
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdc-wb-g3-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 带暗标的 body docx（creator=张三/Company=某某科技/批注作者=赵六
/// ——模拟用户既有文档；正文含一条证据可绑定数值）。
fn write_darkmarked_body(path: &std::path::Path) {
    let mut w = DocxWriter::new();
    w.heading(1, "采购方案说明").unwrap();
    w.paragraph_text("本项目采购 A 设备 12.5 件，单价 336.8 万元。");
    let base = w.save_to_vec().unwrap();
    // 注入暗标 docProps（zip 重打包——测试构造面）。
    let mut reader = zip::ZipArchive::new(std::io::Cursor::new(&base)).unwrap();
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    use std::io::Write as _;
    for i in 0..reader.len() {
        let mut entry = reader.by_index(i).unwrap();
        let name = entry.name().to_string();
        let replacement: &[u8] = match name.as_str() {
            "docProps/core.xml" => r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:creator>张三</dc:creator><cp:lastModifiedBy>李四</cp:lastModifiedBy><dcterms:created>2026-10-26T09:00:00Z</dcterms:created><dcterms:modified>2026-10-26T10:30:00Z</dcterms:modified><cp:revision>42</cp:revision></cp:coreProperties>"#.as_bytes(),
            "docProps/custom.xml" => r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Company"><vt:lpstr>某某科技有限公司</vt:lpstr></property></Properties>"#.as_bytes(),
            _ => {
                use std::io::Read as _;
                let mut buf = vec![];
                entry.read_to_end(&mut buf).unwrap();
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&buf).unwrap();
                continue;
            }
        };
        writer
            .start_file(name.clone(), zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(replacement).unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    std::fs::write(path, bytes).unwrap();
}

/// E1：文件会话全链——暗标全清 + 正文不变 + 原子发布 + 审计。
#[test]
fn g3_e1_file_session_full_chain() {
    let dir = temp_dir("e1");
    let body = dir.join("plan.docx");
    let sidecar = dir.join("plan.evidence.json");
    write_darkmarked_body(&body);
    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar,
        &body,
        "body-1",
        1000,
        "human:setup",
    )
    .unwrap();

    let dest = dir.join("out-anon.docx");
    wb.export_anonymous(&dest, 2000, "human:bidding").unwrap();

    // 原子发布：dest 在场、tmp 零残留。
    assert!(dest.exists());
    assert!(!dir.join("out-anon.export-anon.tmp").exists());

    let src_bytes = std::fs::read(&body).unwrap();
    let out_bytes = std::fs::read(&dest).unwrap();
    // 正文部件逐字节同源（不变量）。
    assert_eq!(
        docx_part_bytes(&src_bytes, "word/document.xml"),
        docx_part_bytes(&out_bytes, "word/document.xml"),
        "正文部件字节不变"
    );
    // docProps 暗标全清。
    let core =
        String::from_utf8(docx_part_bytes(&out_bytes, "docProps/core.xml").unwrap()).unwrap();
    assert!(!core.contains("张三") && !core.contains("李四") && !core.contains("42"));
    assert!(core.contains("unknown"));
    let custom =
        String::from_utf8(docx_part_bytes(&out_bytes, "docProps/custom.xml").unwrap()).unwrap();
    assert!(!custom.contains("某某科技"));
    // 审计事件在 sidecar。
    let sj = std::fs::read_to_string(&sidecar).unwrap();
    assert!(sj.contains("anonymous_export"), "审计事件落账");
    let _ = std::fs::remove_dir_all(&dir);
}

/// E2：内存会话字节返回版。
#[test]
fn g3_e2_memory_session_bytes() {
    let dir = temp_dir("e2");
    let body = dir.join("plan.docx");
    let sidecar = dir.join("plan.evidence.json");
    write_darkmarked_body(&body);
    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar,
        &body,
        "body-1",
        1000,
        "human:setup",
    )
    .unwrap();
    let bytes = wb.export_anonymous_bytes(2000, "human:bidding").unwrap();
    let core = String::from_utf8(docx_part_bytes(&bytes, "docProps/core.xml").unwrap()).unwrap();
    assert!(core.contains("unknown") && !core.contains("张三"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// E3：零标记门复用——正文含工作台标记 → 匿名导出同拒。
#[test]
fn g3_e3_marker_gate_shared() {
    let dir = temp_dir("e3");
    let body = dir.join("plan.docx");
    let sidecar = dir.join("plan.evidence.json");
    let mut w = DocxWriter::new();
    w.paragraph_text("正常段落");
    w.paragraph_text("本段含 mdc-evidence 标记词——干净导出与匿名导出共门拒绝。");
    w.save_to_path(&body.to_string_lossy()).unwrap();
    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar,
        &body,
        "body-1",
        1000,
        "human:setup",
    )
    .unwrap();
    let dest = dir.join("out-anon.docx");
    let err = wb.export_anonymous(&dest, 2000, "human:x").unwrap_err();
    match err {
        WorkbenchError::MarkerFound { .. } => {}
        other => panic!("期望 MarkerFound，实得 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// E4：命令层 dispatch（ExportAnonymous 变体走通 + JSON 返回）。
#[test]
fn g3_e4_command_dispatch() {
    let dir = temp_dir("e4");
    let body = dir.join("plan.docx");
    let sidecar = dir.join("plan.evidence.json");
    write_darkmarked_body(&body);
    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar,
        &body,
        "body-1",
        1000,
        "human:setup",
    )
    .unwrap();
    let dest = dir.join("cmd-anon.docx");
    let env = mdc_evidence::CommandEnvelope {
        op_id: "op-g3-e4".into(),
        operator: "human:op".into(),
        resource: "workbench".into(),
        idempotency_key: mdc_evidence::IdempotencyKey::new("k-g3-e4").unwrap(),
        expected_version: mdc_evidence::ExpectedVersion::Any,
        payload: serde_json::json!({}),
    };
    let out = wb.execute(
        env,
        WorkbenchCommand::ExportAnonymous { dest: dest.clone() },
        2000,
    );
    match out {
        mdc_evidence::CommandOutcome::Succeeded { result: v } => {
            let s = v.to_string();
            assert!(s.contains("cmd-anon.docx"), "返回 dest: {s}");
        }
        other => panic!("期望 Completed，实得 {other:?}"),
    }
    assert!(dest.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
