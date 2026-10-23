//! 内存会话八步闭环（p5-t3——wasm 前置件的行为锚点）。
//!
//! 与 `eight_step_loop.rs` 同场景同断言口径，但全程零文件系统：
//! 字节构造 → `start_memory`/`ingest_source_bytes`/`refresh_body_bytes`/
//! `export_clean_bytes`/`open_memory` 恢复全链。文件会话与内存会话
//! 共享同一验证核心（`Io` 后端分派）——本测试锚定「无文件系统时
//! 行为不漂移」。

use mdc_evidence::{CalcStatus, Decimal, LinkageStatus, PrecisionRule, RelationKind, ReviewStatus};
use mdc_office_io::docx::DocxWriter;
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdc_workbench::{
    BindingProposal, CalcInputSpec, CalcProposal, InputSource, Workbench, WorkbenchError,
};

fn sample_xlsx_bytes(price_c7: f64) -> Vec<u8> {
    let mut wb = WorkbookWriter::new();
    {
        let mut sh = wb.add_sheet("报价表").unwrap();
        sh.write(6, 1, CellValue::Number(12.5), None).unwrap(); // B7 qty
        sh.write(6, 2, CellValue::Number(price_c7), None).unwrap(); // C7 price
        sh.write(7, 1, CellValue::Number(20.0), None).unwrap();
        sh.write(7, 2, CellValue::Number(100.5), None).unwrap();
        sh.write(0, 4, CellValue::Text("备注".into()), None)
            .unwrap();
    }
    wb.save_to_vec().unwrap()
}

fn sample_docx_bytes(subtotal_text: &str) -> Vec<u8> {
    sample_docx_bytes2(
        subtotal_text,
        "本项目采购 A 设备 12.5 件，单价 336.8 万元。",
    )
}

/// 可变采购段（段落 2 = e-quote claim 锚——refresh 场景用）。
fn sample_docx_bytes2(subtotal_text: &str, purchase_line: &str) -> Vec<u8> {
    let mut w = DocxWriter::new();
    w.heading(1, "采购方案说明").unwrap();
    w.paragraph_text("本方案所列金额均为含税价，数据来源见文末。");
    w.paragraph_text(purchase_line);
    w.paragraph_text(subtotal_text);
    w.paragraph_text("报价数据来源于《设备报价表》。");
    w.save_to_vec().unwrap()
}

fn quote_proposal() -> BindingProposal {
    BindingProposal {
        evidence_id: "e-quote".into(),
        claim: "单价 336.8 万元".into(),
        claimed_value: Some(Decimal::parse("336.8").unwrap()),
        paragraph_index: 2,
        source_doc_id: "src-1".into(),
        sheet: "报价表".into(),
        coord: "C7".into(),
        relation: RelationKind::DirectQuote,
        calc_ref: None,
        generation_batch: "g1".into(),
    }
}

fn calc_proposal() -> CalcProposal {
    CalcProposal {
        calc_id: "calc-1".into(),
        expression: "qty * price".into(),
        unit: Some("万元".into()),
        precision: PrecisionRule {
            target_scale: 3,
            rounding: mdc_evidence::Rounding::HalfUp,
        },
        inputs: vec![
            CalcInputSpec {
                name: "qty".into(),
                source: InputSource::Cell {
                    doc_id: "src-1".into(),
                    sheet: "报价表".into(),
                    coord: "B7".into(),
                },
                unit: None,
            },
            CalcInputSpec {
                name: "price".into(),
                source: InputSource::Cell {
                    doc_id: "src-1".into(),
                    sheet: "报价表".into(),
                    coord: "C7".into(),
                },
                unit: None,
            },
        ],
    }
}

#[test]
fn memory_session_eight_step_closed_loop() {
    let t = &mut 1000u64;
    let mut tick = || {
        *t += 100;
        *t
    };

    // ② 登记正文（内存会话——零文件系统）。
    let body = sample_docx_bytes("A 设备小计 4210.00 万元。");
    let mut wb = Workbench::start_memory(&body, "body-1", tick(), "human:setup").unwrap();
    assert_eq!(wb.sidecar().document.doc_id.as_str(), "body-1");

    // ① 来源摄取（字节入口）。
    let src_bytes = sample_xlsx_bytes(336.8);
    let doc_ref = wb
        .ingest_source_bytes("src-1", &src_bytes, tick(), "ai:glm")
        .unwrap();
    assert_eq!(doc_ref.version.0, 1);
    assert_eq!(wb.sidecar().source_docs.len(), 1);

    // ③ 双关联（引用 + 计算）。
    let e_quote = wb
        .propose_binding(quote_proposal(), tick(), "ai:glm")
        .unwrap();
    assert_eq!(e_quote, "e-quote");
    let mut p2 = quote_proposal();
    p2.evidence_id = "e-calc".into();
    p2.claim = "小计 4210.00 万元".into();
    p2.claimed_value = Some(Decimal::parse("4210.00").unwrap());
    p2.paragraph_index = 3;
    p2.relation = RelationKind::Calculated;
    p2.calc_ref = Some("calc-1".into());
    wb.propose_binding(p2, tick(), "ai:glm").unwrap();
    let ev = &wb.sidecar().evidence[0];
    assert!(matches!(ev.review, ReviewStatus::Pending));

    // ④ 计算 + 复算（值来自当前内存源）。
    let v = wb
        .propose_calculation(calc_proposal(), tick(), "ai:glm")
        .unwrap();
    assert_eq!(v.to_string(), "4210.000");
    let outcome = wb.recalc_current("calc-1", tick(), "system").unwrap();
    let e_calc = wb
        .sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-calc")
        .unwrap();
    assert!(matches!(e_calc.calc, CalcStatus::Passed), "{outcome:?}");

    // ⑤ 报告。
    assert!(wb.report().contains("e-quote"));

    // ⑥ 来源变更（新字节）→ 失效级联。
    let src_v2 = sample_xlsx_bytes(400.0);
    wb.ingest_source_bytes("src-1", &src_v2, tick(), "ai:glm")
        .unwrap();
    for e in &wb.sidecar().evidence {
        assert!(matches!(e.linkage, LinkageStatus::Stale));
    }

    // ⑦ 审核（门槛 + 拒绝路径）。
    let pending = wb.pending_reviews();
    assert_eq!(pending.len(), 2);
    wb.review("e-calc", ReviewStatus::Rejected, tick(), "human:wang")
        .unwrap();
    // AI 操作者被门槛拒（零状态变化）。
    let ai = wb.review("e-quote", ReviewStatus::Approved, tick(), "ai:glm");
    assert!(matches!(ai, Err(WorkbenchError::ReviewGate { .. })));

    // ⑥ 恢复通道：来源侧重验（以现值重绑）。
    wb.reverify_source("e-quote", "src-1", "报价表", "C7", tick(), "human:wang")
        .unwrap();
    let e_quote = wb
        .sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-quote")
        .unwrap();
    assert!(matches!(e_quote.linkage, LinkageStatus::Valid));

    // ⑥ 正文变更（新字节）→ 选择性失效（指纹面）：e-calc 在来源
    // 变更时已 Stale（不重复降级计数——degraded = 本次新增降级数，
    // 既有语义）；e-quote 锚（段落 2，未变）保持 Valid。
    let body_v2 = sample_docx_bytes("A 设备小计 5000.00 万元。");
    let degraded = wb
        .refresh_body_bytes(&body_v2, tick(), "human:sync")
        .unwrap();
    assert_eq!(degraded, 0, "已 Stale 者不重复降级；未变段落锚保持 Valid");
    let e_quote_now = wb
        .sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-quote")
        .unwrap();
    assert!(
        matches!(e_quote_now.linkage, LinkageStatus::Valid),
        "未变段落锚保持 Valid（选择性失效）"
    );
    // 段落 3 重验 → e-calc claim 锚更新到新段落 → 恢复 Valid → 过审。
    wb.reverify_claim("e-calc", 3, tick(), "human:wang")
        .unwrap();
    let e_calc_now = wb
        .sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-calc")
        .unwrap();
    assert!(matches!(e_calc_now.linkage, LinkageStatus::Valid));
    wb.review("e-calc", ReviewStatus::Approved, tick(), "human:wang")
        .unwrap();

    // ⑧ 干净导出（字节返回）。
    let exported = wb.export_clean_bytes(tick(), "human:wang").unwrap();
    assert_eq!(exported, body_v2, "导出 = 正文原样（零变换）");

    // 审计链 seq 单调。
    let seqs: Vec<u64> = wb.sidecar().audit.iter().map(|a| a.seq).collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    assert_eq!(seqs, sorted, "audit seq 单调");
}

#[test]
fn memory_session_sidecar_json_roundtrip() {
    // sidecar_json 与磁盘 sidecar 字节同构（serde 原生形）→
    // open_memory 恢复保真（身份/版本/证据/审计全量）。
    let t = &mut 5000u64;
    let mut tick = || {
        *t += 100;
        *t
    };
    let body = sample_docx_bytes("A 设备小计 4210.00 万元。");
    let mut wb = Workbench::start_memory(&body, "body-m", tick(), "human:setup").unwrap();
    let src = sample_xlsx_bytes(336.8);
    wb.ingest_source_bytes("src-1", &src, tick(), "ai:glm")
        .unwrap();
    wb.propose_binding(quote_proposal(), tick(), "ai:glm")
        .unwrap();
    wb.review("e-quote", ReviewStatus::Approved, tick(), "human:wang")
        .unwrap();

    let json = wb.sidecar_json().unwrap();
    // 结构键齐（与磁盘 sidecar 同 schema）。
    for key in [
        "schema_version",
        "document",
        "source_docs",
        "evidence",
        "calculations",
        "idempotency",
        "audit",
    ] {
        assert!(json.contains(key), "sidecar 缺 {key}");
    }

    // 恢复（正文/来源字节由调用方重供——路径不入 sidecar）。
    let mut reopened = Workbench::open_memory(&json, &body, &[("src-1".into(), src)]).unwrap();
    assert_eq!(reopened.sidecar().document.doc_id.as_str(), "body-m");
    assert_eq!(reopened.sidecar().evidence.len(), 1);
    let ev = &reopened.sidecar().evidence[0];
    assert!(
        matches!(ev.review, ReviewStatus::Approved),
        "审核态跨会话保持"
    );
    // 审计链全量。
    assert!(reopened.sidecar().audit.len() >= 4);

    // 恢复后可继续操作（body_version 自增不回退）——改 e-quote 锚
    // 段落（段落 2）→ 恰 1 降级（恢复会话的级联照常）。
    let body_v2 = sample_docx_bytes2(
        "A 设备小计 4210.00 万元。",
        "本项目采购 B 设备 12.5 件，单价 336.8 万元。",
    );
    let degraded = reopened
        .refresh_body_bytes(&body_v2, tick(), "human:sync")
        .unwrap();
    assert_eq!(degraded, 1, "恢复后刷新照常级联");
}

#[test]
fn memory_session_fail_closed_faces() {
    let t = &mut 9000u64;
    let mut tick = || {
        *t += 100;
        *t
    };
    // 垃圾正文字节 → typed 拒。
    assert!(matches!(
        Workbench::start_memory(b"not a docx", "body-x", tick(), "human:setup"),
        Err(WorkbenchError::DocxRead(_))
    ));
    // 垃圾来源字节 → typed 拒。
    let body = sample_docx_bytes("A 设备小计 4210.00 万元。");
    let mut wb = Workbench::start_memory(&body, "body-y", tick(), "human:setup").unwrap();
    assert!(matches!(
        wb.ingest_source_bytes("src-1", b"junk", tick(), "ai:glm"),
        Err(WorkbenchError::XlsxRead(_))
    ));
    // 未注册来源的关联 → typed 拒。
    let mut p = quote_proposal();
    p.source_doc_id = "nope".into();
    assert!(matches!(
        wb.propose_binding(p, tick(), "ai:glm"),
        Err(WorkbenchError::SourceNotRegistered { .. })
    ));
    // 损坏 sidecar JSON → typed 拒。
    assert!(matches!(
        Workbench::open_memory("{broken", &body, &[]),
        Err(WorkbenchError::Sidecar(_))
    ));
    // 正文字节与 sidecar 登记指纹错配 → BodyStale（fail-closed）。
    let src = sample_xlsx_bytes(336.8);
    let mut wb2 = Workbench::start_memory(&body, "body-z", tick(), "human:setup").unwrap();
    wb2.ingest_source_bytes("src-1", &src, tick(), "ai:glm")
        .unwrap();
    let json = wb2.sidecar_json().unwrap();
    let other_body = sample_docx_bytes("完全不同的正文内容。");
    assert!(matches!(
        Workbench::open_memory(&json, &other_body, &[("src-1".into(), src)]),
        Err(WorkbenchError::BodyStale)
    ));
    // 内存会话走文件面 ingest → typed 拒（IO 后端不混用）。
    use std::path::Path;
    assert!(matches!(
        wb2.ingest_source("src-2", Path::new("/tmp/x.xlsx"), tick(), "ai:glm"),
        Err(WorkbenchError::Sidecar(_))
    ));
}
