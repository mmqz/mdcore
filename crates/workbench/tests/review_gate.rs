//! 审核门槛测试（阶段 2——计划 §五「不同业务场景可配置的审核门槛」）。
//!
//! 三维门槛的门面化验证：
//! 1. 操作者允许列表（默认 `human:`——AI 不可改审核态）；
//! 2. 失效关联过审必须先显式重验（默认拒绝）；
//! 3. 主张值门槛——达到即要求计算面 Passed（业务场景差异面）。
//!
//! 拒绝 = 零状态变化 + sidecar 零落盘（门禁先于状态机与 save）。

use std::path::PathBuf;

use mdc_evidence::{
    CalcStatus, CommandEnvelope, CommandOutcome, Decimal, ExpectedVersion, IdempotencyKey,
    LinkageStatus, PrecisionRule, RelationKind, ReviewStatus,
};
use mdc_workbench::{
    BindingProposal, CalcInputSpec, CalcProposal, InputSource, ReviewPolicy, Workbench,
    WorkbenchCommand,
};

fn envelope(op_id: &str, key: &str, operator: &str) -> CommandEnvelope {
    CommandEnvelope {
        op_id: op_id.into(),
        operator: operator.into(),
        resource: "workbench".into(),
        idempotency_key: IdempotencyKey::new(key).unwrap(),
        expected_version: ExpectedVersion::Any,
        payload: serde_json::json!({}),
    }
}

fn policy_for(dir: &std::path::Path) -> mdc_workbench::PathPolicy {
    mdc_workbench::PathPolicy::new(&[dir.to_path_buf()]).expect("temp dir root")
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdc-review-gate-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_sample_xlsx(path: &std::path::Path, price_c7: f64) {
    use mdc_office_io::write::{CellValue, WorkbookWriter};
    let mut w = WorkbookWriter::new();
    let mut sh = w.add_sheet("报价表").unwrap();
    // 0 基坐标：write(row, col) —— B7 = (6,1)，C7 = (6,2)
    sh.write(0, 0, CellValue::Text("数量".into()), None)
        .unwrap();
    sh.write(0, 1, CellValue::Text("单价".into()), None)
        .unwrap();
    sh.write(6, 1, CellValue::Number(12.5), None).unwrap();
    sh.write(6, 2, CellValue::Number(price_c7), None).unwrap();
    w.save_to_path(&path.to_string_lossy()).unwrap();
}

fn write_sample_docx(path: &std::path::Path) {
    use mdc_office_io::docx::DocxWriter;
    let mut w = DocxWriter::new();
    w.heading(1, "采购方案").unwrap();
    w.paragraph_text("引言段。");
    w.paragraph_text("A 设备采购数量 12.5 件。");
    w.paragraph_text("A 设备小计 4210.00 万元。");
    w.paragraph_text("来源见附表。");
    w.save_to_path(&path.to_string_lossy()).unwrap();
}

/// 装配「关联+计算已过复算」的会话（e-calc 主张 4210.00 = 12.5×336.8）。
fn setup(dir: &std::path::Path, price: f64) -> Workbench {
    let xlsx = dir.join("q.xlsx");
    let body = dir.join("p.docx");
    write_sample_xlsx(&xlsx, price);
    write_sample_docx(&body);
    let sidecar = dir.join("p.docx.evidence.json");
    let mut wb = Workbench::start(
        policy_for(dir),
        &sidecar,
        &body,
        "body-1",
        1000,
        "human:setup",
    )
    .unwrap();
    wb.ingest_source("src-1", &xlsx, 1100, "ai:glm").unwrap();
    wb.propose_binding(
        BindingProposal {
            evidence_id: "e-calc".into(),
            claim: "A 设备小计 4210.00 万元".into(),
            claimed_value: Some(Decimal::parse("4210.00").unwrap()),
            paragraph_index: 3,
            source_doc_id: "src-1".into(),
            sheet: "报价表".into(),
            coord: "B7".into(),
            relation: RelationKind::Calculated,
            calc_ref: Some("c1".into()),
            generation_batch: "b1".into(),
        },
        1200,
        "ai:glm",
    )
    .unwrap();
    wb.propose_calculation(
        CalcProposal {
            calc_id: "c1".into(),
            expression: "qty * price".into(),
            unit: Some("万元".into()),
            precision: PrecisionRule {
                target_scale: 2,
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
        },
        1300,
        "ai:glm",
    )
    .unwrap();
    wb.recalc_current("c1", 1400, "system").unwrap();
    wb
}

fn review_of(wb: &Workbench, id: &str) -> ReviewStatus {
    wb.sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == id)
        .unwrap()
        .review
}

#[test]
fn default_gate_blocks_ai_reviewer() {
    let dir = temp_dir("ai-op");
    let wb = &mut setup(&dir, 336.8);
    // 默认档：AI 操作者不可执行任何审核方向（含拒绝）
    let err = wb
        .review("e-calc", ReviewStatus::Approved, 1500, "ai:glm")
        .unwrap_err();
    assert!(matches!(
        err,
        mdc_workbench::WorkbenchError::ReviewGate { .. }
    ));
    let err = wb
        .review("e-calc", ReviewStatus::Rejected, 1500, "ai:glm")
        .unwrap_err();
    assert!(matches!(
        err,
        mdc_workbench::WorkbenchError::ReviewGate { .. }
    ));
    // 零状态变化
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Pending);
    // 人工操作者照常
    wb.review("e-calc", ReviewStatus::Approved, 1600, "human:wang")
        .unwrap();
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Approved);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn default_gate_blocks_approval_of_stale_linkage() {
    let dir = temp_dir("stale");
    let wb = &mut setup(&dir, 336.8);
    // 来源修正 → Stale
    let xlsx = dir.join("q.xlsx");
    write_sample_xlsx(&xlsx, 400.0);
    wb.ingest_source("src-1", &xlsx, 1500, "human:wang")
        .unwrap();
    assert_eq!(wb.sidecar().evidence[0].linkage, LinkageStatus::Stale);
    // 过审被拒（失效关联不得直接过审——不得自动恢复为已核验的门面）
    let err = wb
        .review("e-calc", ReviewStatus::Approved, 1600, "human:wang")
        .unwrap_err();
    match err {
        mdc_workbench::WorkbenchError::ReviewGate { reason, .. } => {
            assert!(reason.contains("linkage"), "拒绝原因指向关联态: {reason}");
        }
        other => panic!("expected ReviewGate, got {other:?}"),
    }
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Pending);
    // 拒绝方向不受关联门槛辖（人工可拒绝失效证据）
    wb.review("e-calc", ReviewStatus::Rejected, 1700, "human:wang")
        .unwrap();
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Rejected);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn value_threshold_requires_passed_calc() {
    let dir = temp_dir("threshold");
    let wb = &mut setup(&dir, 336.8);
    // 业务场景档：主张值 ≥ 1000 需计算面 Passed
    wb.set_review_policy(ReviewPolicy {
        calc_required_at_or_above: Some(Decimal::parse("1000").unwrap()),
        ..ReviewPolicy::default()
    });
    // 现状：calc Passed → 过审放行
    wb.review("e-calc", ReviewStatus::Approved, 1500, "human:wang")
        .unwrap();

    // 回滚到 Pending 不可（状态机单程）——换新证据演示失败面：
    // 来源修正使计算面 InputMismatch + 关联 Stale，双门齐拒
    let xlsx = dir.join("q.xlsx");
    write_sample_xlsx(&xlsx, 400.0);
    wb.ingest_source("src-1", &xlsx, 1600, "human:wang")
        .unwrap();
    wb.recalc_current("c1", 1700, "system").unwrap();
    let sc = wb.sidecar();
    let ev = &sc.evidence[0];
    assert_eq!(ev.linkage, LinkageStatus::Stale);
    // Stale 级联诚实性：linkage 非 Valid → 复算不经（InputUnavailable）。
    // 门禁拒绝过审与此独立成立（计算面非 Passed + 关联非 Valid 双事实）。
    assert_eq!(
        ev.calc,
        CalcStatus::Failed(mdc_evidence::CalcFailure::InputUnavailable)
    );
    // Rejected → Approved 的翻案也吃门（Reviewed→Approved 路径同受门槛辖）
    let err = wb
        .review("e-calc", ReviewStatus::Approved, 1800, "human:wang")
        .unwrap_err();
    assert!(matches!(
        err,
        mdc_workbench::WorkbenchError::ReviewGate { .. }
    ));
    assert_eq!(
        review_of(wb, "e-calc"),
        ReviewStatus::Approved,
        "状态零变化"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn relaxed_scenario_allows_broken_linkage_approval() {
    let dir = temp_dir("relaxed");
    let wb = &mut setup(&dir, 336.8);
    // 宽松业务档：允许失效关联直接过审（如草稿/内部评审场景）
    wb.set_review_policy(ReviewPolicy {
        allow_approval_of_broken_linkage: true,
        ..ReviewPolicy::default()
    });
    let xlsx = dir.join("q.xlsx");
    write_sample_xlsx(&xlsx, 400.0);
    wb.ingest_source("src-1", &xlsx, 1500, "human:wang")
        .unwrap();
    assert_eq!(wb.sidecar().evidence[0].linkage, LinkageStatus::Stale);
    // 宽松档放行（显式配置的语义面——审计链仍记录原状态）
    wb.review("e-calc", ReviewStatus::Approved, 1600, "human:wang")
        .unwrap();
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Approved);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn custom_reviewer_prefixes() {
    let dir = temp_dir("prefix");
    let wb = &mut setup(&dir, 336.8);
    // 场景档：只有审计员角色可审
    wb.set_review_policy(ReviewPolicy {
        reviewer_prefixes: vec!["auditor:".to_string()],
        ..ReviewPolicy::default()
    });
    let err = wb
        .review("e-calc", ReviewStatus::Approved, 1500, "human:wang")
        .unwrap_err();
    assert!(matches!(
        err,
        mdc_workbench::WorkbenchError::ReviewGate { .. }
    ));
    wb.review("e-calc", ReviewStatus::Approved, 1600, "auditor:li")
        .unwrap();
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Approved);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn gate_enforced_through_command_dispatch() {
    let dir = temp_dir("dispatch");
    let wb = &mut setup(&dir, 336.8);
    // 命令层同门（UI 统一入口 = 命令执行——门禁在 Workbench::review 单点）
    let env = envelope("op-gate-1", "gate-1", "ai:glm");
    let cmd = WorkbenchCommand::Review {
        evidence_id: "e-calc".into(),
        decision: ReviewStatus::Approved,
    };
    let out = wb.execute(env, cmd, 1500);
    assert!(
        matches!(out, CommandOutcome::Rejected { .. }),
        "AI 操作者经命令面同样被门禁拒绝: {out:?}"
    );
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Pending);
    // 重试 = 同结果（Rejected 不入幂等表——每笔独立校验）
    let env2 = envelope("op-gate-2", "gate-2", "ai:glm");
    let cmd2 = WorkbenchCommand::Review {
        evidence_id: "e-calc".into(),
        decision: ReviewStatus::Approved,
    };
    let out2 = wb.execute(env2, cmd2, 1600);
    assert!(matches!(out2, CommandOutcome::Rejected { .. }));
    assert_eq!(review_of(wb, "e-calc"), ReviewStatus::Pending);
    std::fs::remove_dir_all(&dir).ok();
}
