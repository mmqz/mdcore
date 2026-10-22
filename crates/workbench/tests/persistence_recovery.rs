//! JSON sidecar 持久化与异常恢复验证（收尾轮——用户指令：
//! "验证 JSON sidecar 的持久化与异常恢复"）。
//!
//! 覆盖面（全部合成数据）：
//! - **会话重开保真**：`Workbench::open` 后 sidecar 全量状态在位
//!   （证据/计算/审核/审计/幂等表），且 `body_doc_id`/`body_version`/
//!   来源版本从 sidecar 恢复（不硬编码重启——版本单调不回退）。
//! - **跨会话幂等重放**：会话一执行的命令，重开后同键重放返回
//!   `Replayed` 首果，不重复执行。
//! - **异常恢复 fail-closed**：截断/损坏/未来版本/缺文件 → open
//!   显式错误，绝不静默新会话（那等于丢证据库）。
//! - **中断发布残留**：`.publish.tmp` 残留不破加载；下次发布原子
//!   覆盖。
//! - **跨会话 fail-closed**：会话间正文被改 → 重开后提议拒绝
//!   （BodyStale）——指纹持久化生效。

use std::path::PathBuf;

use mdc_evidence::{
    CommandEnvelope, CommandOutcome, Decimal, ExpectedVersion, IdempotencyKey, PrecisionRule,
    RelationKind, ReviewStatus,
};
use mdc_office_io::docx::DocxWriter;
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdc_workbench::{
    BindingProposal, CalcInputSpec, CalcProposal, InputSource, PathPolicy, Workbench,
    WorkbenchCommand, WorkbenchError,
};

fn policy_for(dir: &std::path::Path) -> PathPolicy {
    PathPolicy::new(&[dir.to_path_buf()]).expect("temp dir root")
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdc-wb-pr-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_sample_xlsx(path: &std::path::Path, price_c7: f64) {
    let mut wb = WorkbookWriter::new();
    {
        let mut sh = wb.add_sheet("报价表").unwrap();
        sh.write(6, 1, CellValue::Number(12.5), None).unwrap(); // B7 qty
        sh.write(6, 2, CellValue::Number(price_c7), None).unwrap(); // C7 price
    }
    wb.save_to_path(&path.to_string_lossy()).unwrap();
}

fn write_sample_docx(path: &std::path::Path, subtotal_text: &str) {
    let mut w = DocxWriter::new();
    w.heading(1, "采购方案说明").unwrap();
    w.paragraph_text("本项目采购 A 设备 12.5 件，单价 336.8 万元。");
    w.paragraph_text(subtotal_text);
    w.save_to_path(&path.to_string_lossy()).unwrap();
}

fn binding(id: &str, para: u32, value: &str) -> BindingProposal {
    BindingProposal {
        evidence_id: id.into(),
        claim: format!("A 设备数量/金额 {value}"),
        claimed_value: Some(Decimal::parse(value).unwrap()),
        paragraph_index: para,
        source_doc_id: "src-1".into(),
        sheet: "报价表".into(),
        coord: "B7".into(),
        relation: RelationKind::DirectQuote,
        calc_ref: None,
        generation_batch: "b1".into(),
    }
}

fn envelope(op_id: &str, key: &str, version: ExpectedVersion, operator: &str) -> CommandEnvelope {
    CommandEnvelope {
        op_id: op_id.into(),
        operator: operator.into(),
        resource: "workbench".into(),
        idempotency_key: IdempotencyKey::new(key).unwrap(),
        expected_version: version,
        payload: serde_json::json!({}),
    }
}

/// 装配一个已完成 关联+计算 的会话（供多测试复用）。
fn setup_session(
    dir: &std::path::Path,
    sidecar_path: &std::path::Path,
    body: &std::path::Path,
    xlsx: &std::path::Path,
) -> Workbench {
    write_sample_xlsx(xlsx, 336.8);
    write_sample_docx(body, "A 设备小计 4210.00 万元。");
    let mut wb = Workbench::start(
        policy_for(dir),
        sidecar_path,
        body,
        "body-1",
        1000,
        "human:setup",
    )
    .unwrap();
    wb.ingest_source("src-1", xlsx, 1100, "ai:glm").unwrap();
    wb.propose_binding(binding("e-quote", 1, "12.5"), 1200, "ai:glm")
        .unwrap();
    wb.propose_binding(binding("e-calc", 2, "4210.00"), 1300, "ai:glm")
        .unwrap();
    wb.propose_calculation(
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
                    unit: Some("件".into()),
                },
                CalcInputSpec {
                    name: "price".into(),
                    source: InputSource::Cell {
                        doc_id: "src-1".into(),
                        sheet: "报价表".into(),
                        coord: "C7".into(),
                    },
                    unit: Some("万元".into()),
                },
            ],
        },
        1400,
        "ai:glm",
    )
    .unwrap();
    wb
}

#[test]
fn session_reopen_state_and_continuity() {
    let dir = temp_dir("reopen");
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.evidence.json");

    let mut wb = setup_session(&dir, &sidecar_path, &body, &xlsx);
    wb.review("e-quote", ReviewStatus::Approved, 1500, "human:wang")
        .unwrap();
    // 会话内一次正文刷新（版本 1 → 2），验证重开后版本从 sidecar 恢复
    // 而非重启为 1（否则下次刷新会产生版本 2 重复登记）。
    wb.refresh_body(1600, "human:wang").unwrap();
    let audit_len = wb.sidecar().audit.len();
    let idem_len = wb.sidecar().idempotency.len();
    drop(wb); // —— 会话结束（非正常关闭路径：无显式 close API，直接丢）——

    // —— 重开 ——
    let wb2 = Workbench::open(
        policy_for(&dir),
        &sidecar_path,
        &body,
        &[("src-1".into(), xlsx.clone())],
    )
    .unwrap();

    // sidecar 全量状态在位
    let sc = wb2.sidecar();
    assert_eq!(sc.document.doc_id.as_str(), "body-1", "正文 doc_id 恢复");
    assert_eq!(
        sc.document.version.0, 2,
        "正文版本从 sidecar 恢复（会话内已刷新到 2）"
    );
    assert_eq!(sc.evidence.len(), 2, "证据记录持久化");
    assert_eq!(sc.calculations.len(), 1, "计算记录持久化");
    let e_quote = sc
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-quote")
        .unwrap();
    assert_eq!(e_quote.review, ReviewStatus::Approved, "审核决定持久化");
    assert_eq!(sc.source_docs.len(), 1, "来源注册表持久化（doc_id + 指纹）");
    assert_eq!(sc.audit.len(), audit_len, "审计日志持久化（条数）");
    assert_eq!(sc.idempotency.len(), idem_len, "幂等表持久化");
    // 审计序号仍单调（重开后继续追加不回卷）
    assert_eq!(
        sc.audit.last().map(|e| e.seq),
        Some(sc.audit.len() as u64),
        "审计序号连续"
    );

    // —— 重开后继续操作 ——
    let mut wb2 = wb2;
    // 复算可用（来源文件与会话间未变 → 现值对照通过）
    let out = wb2.recalc_current("calc-1", 1700, "system").unwrap();
    assert_eq!(out.status, mdc_evidence::CalcStatus::Passed);
    // 正文刷新：版本继续单调（2 → 3，不回退不碰撞）；doc_id 不置空
    wb2.refresh_body(1800, "human:wang").unwrap();
    let sc = wb2.sidecar();
    assert_eq!(
        sc.document.version.0, 3,
        "重开后版本单调续增（修复点：此前硬编码重启会回退到 1→2 碰撞）"
    );
    assert_eq!(
        sc.document.doc_id.as_str(),
        "body-1",
        "重开后刷新正文不丢 doc_id（修复点：此前置空）"
    );
    // 干净导出照常
    let export = dir.join("plan-clean.docx");
    wb2.export_clean(&export, 1900, "human:wang").unwrap();
    let body_bytes = std::fs::read(&body).unwrap();
    let export_bytes = std::fs::read(&export).unwrap();
    assert_eq!(body_bytes, export_bytes, "重开会话导出 = 正文原样字节");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cross_session_idempotent_replay() {
    let dir = temp_dir("idem");
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.evidence.json");

    let mut wb = setup_session(&dir, &sidecar_path, &body, &xlsx);
    let evidence_before = wb.sidecar().evidence.len();
    let audit_before = wb.sidecar().audit.len();

    // 命令分发面执行一条命令（入幂等表）。审核 = 人工动作——阶段 2
    // 门禁下 AI 操作者过不了审核允许列表（首执行成功是本测试前提）
    let env = envelope("op-1", "key-persist-1", ExpectedVersion::Any, "human:wang");
    let out = wb.execute(
        env.clone(),
        WorkbenchCommand::Review {
            evidence_id: "e-quote".into(),
            decision: ReviewStatus::Approved,
        },
        2000,
    );
    assert!(
        matches!(out, CommandOutcome::Succeeded { .. }),
        "首执行成功：{out:?}"
    );
    drop(wb);

    // —— 重开后同键重放：返回首果，不重复执行 ——
    let mut wb2 = Workbench::open(
        policy_for(&dir),
        &sidecar_path,
        &body,
        &[("src-1".into(), xlsx.clone())],
    )
    .unwrap();
    let out2 = wb2.execute(
        env,
        WorkbenchCommand::Review {
            evidence_id: "e-quote".into(),
            decision: ReviewStatus::Approved,
        },
        2100,
    );
    match out2 {
        CommandOutcome::Replayed { first_result } => {
            assert_eq!(first_result["evidence_id"], "e-quote", "重放返回首次结果");
        }
        other => panic!("期望 Replayed，得到 {other:?}"),
    }
    // 重放不产生重复副作用：证据条数不变；审计只追加 command_replayed
    let sc = wb2.sidecar();
    assert_eq!(sc.evidence.len(), evidence_before, "重放零副作用");
    assert_eq!(sc.audit.len(), audit_before + 2, "首执行 1 条 + 重放 1 条");
    assert!(
        sc.audit.iter().any(|e| e.kind == "command_replayed"),
        "重放审计在档"
    );
    // 幂等表持久化且可继续服务新命令
    assert_eq!(sc.idempotency.len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn source_version_monotonic_across_reopen() {
    let dir = temp_dir("srcver");
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.evidence.json");

    let mut wb = setup_session(&dir, &sidecar_path, &body, &xlsx);
    assert_eq!(wb.sidecar().source_docs[0].version.0, 1, "首次登记 v1");
    // 来源改写（单价修正）→ 重读重登记 v2
    write_sample_xlsx(&xlsx, 400.0);
    let r2 = wb
        .ingest_source("src-1", &xlsx, 1500, "human:wang")
        .unwrap();
    assert_eq!(r2.version.0, 2, "会话内再登记 v2");
    drop(wb);

    // —— 重开后再次登记（文件未再变）→ 版本必须从 sidecar 续增为 3 ——
    let mut wb2 = Workbench::open(
        policy_for(&dir),
        &sidecar_path,
        &body,
        &[("src-1".into(), xlsx.clone())],
    )
    .unwrap();
    let r3 = wb2
        .ingest_source("src-1", &xlsx, 1600, "human:wang")
        .unwrap();
    assert_eq!(
        r3.version.0, 3,
        "重开后来源版本单调续增（修复点：此前硬编码 1 → 登记为 2 与历史碰撞）"
    );
    assert_eq!(wb2.sidecar().source_docs[0].version.0, 3);
    // 未登记过的来源（首次）默认 1 不受影响
    let xlsx2 = dir.join("quote2.xlsx");
    write_sample_xlsx(&xlsx2, 336.8);
    let r_new = wb2.ingest_source("src-2", &xlsx2, 1700, "ai:glm").unwrap();
    assert_eq!(r_new.version.0, 1, "新来源首登记 v1");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn corrupt_truncated_and_missing_sidecar_fail_closed_on_open() {
    let dir = temp_dir("corrupt");
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.evidence.json");

    let _wb = setup_session(&dir, &sidecar_path, &body, &xlsx);
    let good = std::fs::read_to_string(&sidecar_path).unwrap();
    let sources = vec![("src-1".into(), xlsx.clone())];

    // —— 截断 JSON ——
    std::fs::write(&sidecar_path, &good[..good.len() / 2]).unwrap();
    match Workbench::open(policy_for(&dir), &sidecar_path, &body, &sources) {
        Err(WorkbenchError::Sidecar(_)) => {}
        other => panic!(
            "截断 sidecar 期望 Sidecar 错误，得到 {:?}",
            other.err().map(|e| e.to_string())
        ),
    }

    // —— 损坏 JSON ——
    std::fs::write(&sidecar_path, b"{ not valid json !!!").unwrap();
    match Workbench::open(policy_for(&dir), &sidecar_path, &body, &sources) {
        Err(WorkbenchError::Sidecar(_)) => {}
        other => panic!(
            "损坏 sidecar 期望 Sidecar 错误，得到 {:?}",
            other.err().map(|e| e.to_string())
        ),
    }

    // —— 未来模式版本 ——
    let future = good.replace("\"schema_version\": 1", "\"schema_version\": 99");
    assert_ne!(&future, &good, "夹具确实包含版本字段");
    std::fs::write(&sidecar_path, future).unwrap();
    match Workbench::open(policy_for(&dir), &sidecar_path, &body, &sources) {
        Err(WorkbenchError::Sidecar(_)) => {}
        other => panic!(
            "未来版本 sidecar 期望 Sidecar 错误，得到 {:?}",
            other.err().map(|e| e.to_string())
        ),
    }

    // —— 缺文件（fail-closed：不静默新会话——那等于丢证据库）——
    std::fs::remove_file(&sidecar_path).unwrap();
    match Workbench::open(policy_for(&dir), &sidecar_path, &body, &sources) {
        Err(WorkbenchError::Sidecar(_)) => {}
        other => panic!(
            "缺失 sidecar 期望显式错误，得到 {:?}",
            other.err().map(|e| e.to_string())
        ),
    }

    // 恢复完好文件后一切正常（异常是可诊断可恢复的，不是永久损坏）
    std::fs::write(&sidecar_path, good).unwrap();
    let wb = Workbench::open(policy_for(&dir), &sidecar_path, &body, &sources);
    assert!(
        wb.is_ok(),
        "完好 sidecar 恢复加载失败：{}",
        wb.err().map(|e| e.to_string()).unwrap_or_default()
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn interrupted_publish_leftover_does_not_break_recovery() {
    let dir = temp_dir("tmp");
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.evidence.json");

    let wb = setup_session(&dir, &sidecar_path, &body, &xlsx);
    drop(wb);

    // —— 模拟中断发布：临时区残留垃圾（rename 未发生）——
    // publish 单点临时名 = <stem>.publish.tmp（with_extension 语义）
    let tmp = sidecar_path.with_extension("publish.tmp");
    std::fs::write(&tmp, b"garbage from interrupted publish").unwrap();
    // 残留不破加载（主文件完好 = 上次发布的原子性保证）
    let wb2 = Workbench::open(
        policy_for(&dir),
        &sidecar_path,
        &body,
        &[("src-1".into(), xlsx.clone())],
    )
    .unwrap();
    assert_eq!(wb2.sidecar().evidence.len(), 2);

    // —— 下次发布原子覆盖：垃圾被 rename 顶替，零残留 ——
    let mut wb2 = wb2;
    wb2.review("e-quote", ReviewStatus::Approved, 1600, "human:wang")
        .unwrap();
    assert!(!tmp.exists(), "发布后临时文件清除");
    // 重读验证落盘内容为最新状态（含新审核）
    let wb3 = Workbench::open(
        policy_for(&dir),
        &sidecar_path,
        &body,
        &[("src-1".into(), xlsx.clone())],
    )
    .unwrap();
    let e = wb3
        .sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-quote")
        .unwrap();
    assert_eq!(e.review, ReviewStatus::Approved, "最新状态已发布");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn body_changed_between_sessions_fails_closed() {
    let dir = temp_dir("stale");
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.evidence.json");

    let wb = setup_session(&dir, &sidecar_path, &body, &xlsx);
    // 会话一里已绑两条（供跨会话对照）
    drop(wb);

    // —— 会话之间正文被改写（未走 refresh_body）——
    write_sample_docx(&body, "A 设备小计 4999.00 万元（被外部改写）。");

    let mut wb2 = Workbench::open(
        policy_for(&dir),
        &sidecar_path,
        &body,
        &[("src-1".into(), xlsx.clone())],
    )
    .unwrap();
    // 指纹持久化生效：当前文件指纹 ≠ sidecar 登记引用 → 拒绝提议
    match wb2.propose_binding(binding("e-new", 1, "12.5"), 2000, "ai:glm") {
        Err(WorkbenchError::BodyStale) => {}
        other => panic!("跨会话改写后期望 BodyStale，得到 {other:?}"),
    }
    // 走 refresh_body（显式刷新登记 + 失效检测）后才恢复可提议
    wb2.refresh_body(2100, "human:wang").unwrap();
    wb2.propose_binding(binding("e-new", 1, "12.5"), 2200, "ai:glm")
        .unwrap();
    assert_eq!(wb2.sidecar().evidence.len(), 3);

    std::fs::remove_dir_all(&dir).ok();
}
