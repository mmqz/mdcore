//! S2 写入安全门（路径允许列表 / 幂等重放 / 预期版本 / 规模限制 /
//! 发布纪律）——命令分发面的行为钉定。

use std::path::PathBuf;

use mdc_evidence::{
    CommandEnvelope, CommandOutcome, Decimal, ExpectedVersion, IdempotencyKey, PrecisionRule,
    RelationKind, ReviewStatus,
};
use mdc_office_io::docx::DocxWriter;
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdc_workbench::{
    expect_exactly, BindingProposal, CalcInputSpec, CalcProposal, InputSource, PathPolicy,
    Workbench, WorkbenchCommand, MAX_CALCULATIONS, MAX_EVIDENCE, MAX_SIDECAR_BYTES,
};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdc-wb-s2-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn policy_for(dir: &std::path::Path) -> PathPolicy {
    PathPolicy::new(&[dir.to_path_buf()]).expect("temp dir root")
}

fn write_sample_xlsx(path: &std::path::Path, price: f64) {
    let mut wb = WorkbookWriter::new();
    {
        let mut sh = wb.add_sheet("报价表").unwrap();
        sh.write(6, 1, CellValue::Number(12.5), None).unwrap();
        sh.write(6, 2, CellValue::Number(price), None).unwrap();
    }
    wb.save_to_path(&path.to_string_lossy()).unwrap();
}

fn write_sample_docx(path: &std::path::Path) {
    let mut w = DocxWriter::new();
    w.heading(1, "方案").unwrap();
    w.paragraph_text("本项目采购 A 设备 12.5 件。");
    w.paragraph_text("A 设备小计 4210.00 万元。");
    w.save_to_path(&path.to_string_lossy()).unwrap();
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

fn binding(id: &str, para: u32) -> BindingProposal {
    BindingProposal {
        evidence_id: id.into(),
        claim: "A 设备小计 4210.00 万元".into(),
        claimed_value: Some(Decimal::parse("4210.00").unwrap()),
        paragraph_index: para,
        source_doc_id: "src-1".into(),
        sheet: "报价表".into(),
        coord: "B7".into(),
        relation: RelationKind::DirectQuote,
        calc_ref: None,
        generation_batch: "b1".into(),
    }
}

#[test]
fn path_policy_blocks_out_of_root_everywhere() {
    let dir = temp_dir("policy");
    let outside = temp_dir("outside"); // 另一根（policy 不含）
    let xlsx = dir.join("q.xlsx");
    let body = dir.join("p.docx");
    let sidecar_path = dir.join("p.evidence.json");
    write_sample_xlsx(&xlsx, 336.8);
    write_sample_docx(&body);

    // ① start 拒根外正文
    let outside_body = outside.join("p.docx");
    write_sample_docx(&outside_body);
    assert!(
        Workbench::start(
            policy_for(&dir),
            &sidecar_path,
            &outside_body,
            "body-1",
            1,
            "op"
        )
        .is_err(),
        "根外正文拒"
    );
    // ② start 拒根外 sidecar
    assert!(
        Workbench::start(
            policy_for(&dir),
            &outside.join("sc.json"),
            &body,
            "body-1",
            1,
            "op"
        )
        .is_err(),
        "根外 sidecar 拒"
    );
    // ③ 命令分发面：根外来源 → Rejected
    let mut wb =
        Workbench::start(policy_for(&dir), &sidecar_path, &body, "body-1", 10, "op").unwrap();
    let outside_xlsx = outside.join("q.xlsx");
    write_sample_xlsx(&outside_xlsx, 336.8);
    let out = wb.execute(
        envelope("op-1", "k-1", ExpectedVersion::Any, "ai:glm"),
        WorkbenchCommand::IngestSource {
            doc_id: "src-x".into(),
            path: outside_xlsx,
        },
        20,
    );
    assert!(
        matches!(out, CommandOutcome::Rejected { ref reason } if reason.contains("path policy")),
        "根外来源 Rejected，得到 {out:?}"
    );
    // ④ 根外导出目的地 → Rejected
    let out2 = wb.execute(
        envelope("op-2", "k-2", ExpectedVersion::Any, "human:wang"),
        WorkbenchCommand::ExportClean {
            dest: outside.join("out.docx"),
        },
        30,
    );
    assert!(
        matches!(out2, CommandOutcome::Rejected { .. }),
        "根外导出拒"
    );
    assert!(!outside.join("out.docx").exists(), "拒绝零产出");

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&outside).ok();
}

/// 符号链接构造是 Unix 面能力——非 Unix 目标（wasm32 编译检查面）
/// 不编译本用例（加载态行为由 PathPolicy::check 的 canonicalize 语义
/// 覆盖，与平台无关）。
#[cfg(all(test, target_family = "unix"))]
#[test]
fn path_policy_blocks_symlink_escape() {
    let dir = temp_dir("symlink");
    let outside = temp_dir("symlink-out");
    let body = dir.join("p.docx");
    write_sample_docx(&body);
    // 根外真实文件 + 根内符号链接指向它
    let real = outside.join("real.xlsx");
    write_sample_xlsx(&real, 336.8);
    let link = dir.join("escape.xlsx");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let mut wb =
        Workbench::start(policy_for(&dir), &dir.join("sc.json"), &body, "b", 1, "op").unwrap();
    let out = wb.execute(
        envelope("op-1", "k-1", ExpectedVersion::Any, "ai:glm"),
        WorkbenchCommand::IngestSource {
            doc_id: "src-1".into(),
            path: link,
        },
        2,
    );
    assert!(
        matches!(out, CommandOutcome::Rejected { .. }),
        "符号链接逃逸拒（canonicalize 解析后落根外），得到 {out:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn idempotent_replay_and_version_conflict_semantics() {
    let dir = temp_dir("idem");
    let xlsx = dir.join("q.xlsx");
    let body = dir.join("p.docx");
    let sidecar_path = dir.join("p.evidence.json");
    write_sample_xlsx(&xlsx, 336.8);
    write_sample_docx(&body);

    let mut wb =
        Workbench::start(policy_for(&dir), &sidecar_path, &body, "body-1", 1, "setup").unwrap();
    wb.execute(
        envelope("op-0", "k-src", ExpectedVersion::Any, "ai:glm"),
        WorkbenchCommand::IngestSource {
            doc_id: "src-1".into(),
            path: xlsx.clone(),
        },
        2,
    );

    // 幂等首执行
    let first = wb.execute(
        envelope("op-1", "k-bind", ExpectedVersion::Any, "ai:glm"),
        WorkbenchCommand::ProposeBinding(binding("e1", 2)),
        3,
    );
    assert!(matches!(first, CommandOutcome::Succeeded { .. }));
    assert_eq!(wb.sidecar().evidence.len(), 1);

    // 同键重放：不重复执行（否则 DuplicateEvidenceId）
    let replay = wb.execute(
        envelope("op-2", "k-bind", ExpectedVersion::Any, "ai:glm"),
        WorkbenchCommand::ProposeBinding(binding("e1", 2)),
        4,
    );
    match replay {
        CommandOutcome::Replayed { first_result } => {
            assert_eq!(first_result, serde_json::json!({"evidence_id": "e1"}));
        }
        other => panic!("期望 Replayed，得到 {other:?}"),
    }
    assert_eq!(wb.sidecar().evidence.len(), 1, "重放零重复副作用");
    // 重放审计在场
    assert!(wb
        .sidecar()
        .audit
        .iter()
        .any(|e| e.kind == "command_replayed"));

    // 版本冲突：Exactly(99) ≠ 当前 1 → VersionConflict
    let conflict = wb.execute(
        envelope("op-3", "k-conflict", expect_exactly(99), "ai:glm"),
        WorkbenchCommand::Review {
            evidence_id: "e1".into(),
            decision: ReviewStatus::Approved,
        },
        5,
    );
    assert!(matches!(
        conflict,
        CommandOutcome::VersionConflict {
            expected: mdc_evidence::VersionStamp(99),
            actual: mdc_evidence::VersionStamp(1)
        }
    ));
    // 冲突不入幂等表：同键重试（修正版本）可执行
    let retry = wb.execute(
        envelope("op-4", "k-conflict", expect_exactly(1), "human:wang"),
        WorkbenchCommand::Review {
            evidence_id: "e1".into(),
            decision: ReviewStatus::Approved,
        },
        6,
    );
    assert!(
        matches!(retry, CommandOutcome::Succeeded { .. }),
        "冲突不毒化重试"
    );

    // MustBeNew 违反（已有记录）→ Rejected
    let mbn = wb.execute(
        envelope("op-5", "k-mbn", ExpectedVersion::MustBeNew, "ai:glm"),
        WorkbenchCommand::ProposeBinding(binding("e2", 2)),
        7,
    );
    assert!(matches!(mbn, CommandOutcome::Rejected { .. }));

    // 失败入表（确定性）：坏段落的绑定 → Failed；同键重放失败结果
    let fail = wb.execute(
        envelope("op-6", "k-fail", ExpectedVersion::Any, "ai:glm"),
        WorkbenchCommand::ProposeBinding(binding("e9", 99)),
        8,
    );
    assert!(matches!(fail, CommandOutcome::Failed { .. }));
    let replay_fail = wb.execute(
        envelope("op-7", "k-fail", ExpectedVersion::Any, "ai:glm"),
        WorkbenchCommand::ProposeBinding(binding("e9", 99)),
        9,
    );
    assert!(
        matches!(replay_fail, CommandOutcome::Replayed { .. }),
        "失败重放同果"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn full_loop_through_command_dispatch() {
    let dir = temp_dir("dispatch");
    let xlsx = dir.join("q.xlsx");
    let body = dir.join("p.docx");
    let sidecar_path = dir.join("p.evidence.json");
    let export = dir.join("clean.docx");
    write_sample_xlsx(&xlsx, 336.8);
    write_sample_docx(&body);

    let t = &mut 100u64;
    // 审核时间戳快照（tick 闭包借用 t 之前取——阶段2人工信封用；
    // 语义 = 全部 exec 命令之后的时间点，单调即可）
    let review_at = 900u64;
    let mut tick = || {
        *t += 10;
        *t
    };
    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar_path,
        &body,
        "body-1",
        tick(),
        "setup",
    )
    .unwrap();

    // 八步全走命令分发面（受控命令集——无任意路径/SQL/Shell）
    let mut k = 0;
    let mut exec = |wb: &mut Workbench, cmd: WorkbenchCommand, version: ExpectedVersion| {
        k += 1;
        let env = envelope(&format!("op-{k}"), &format!("key-{k}"), version, "ai:glm");
        let at = tick();
        wb.execute(env, cmd, at)
    };

    assert!(matches!(
        exec(
            &mut wb,
            WorkbenchCommand::IngestSource {
                doc_id: "src-1".into(),
                path: xlsx.clone()
            },
            ExpectedVersion::Any
        ),
        CommandOutcome::Succeeded { .. }
    ));
    assert!(matches!(
        exec(
            &mut wb,
            WorkbenchCommand::ProposeCalculation(CalcProposal {
                calc_id: "c1".into(),
                expression: "qty * price".into(),
                unit: Some("万元".into()),
                precision: PrecisionRule {
                    target_scale: 2,
                    rounding: mdc_evidence::Rounding::HalfUp
                },
                inputs: vec![
                    CalcInputSpec {
                        name: "qty".into(),
                        source: InputSource::Cell {
                            doc_id: "src-1".into(),
                            sheet: "报价表".into(),
                            coord: "B7".into()
                        },
                        unit: Some("件".into())
                    },
                    CalcInputSpec {
                        name: "price".into(),
                        source: InputSource::Cell {
                            doc_id: "src-1".into(),
                            sheet: "报价表".into(),
                            coord: "C7".into()
                        },
                        unit: Some("万元".into())
                    },
                ],
            }),
            ExpectedVersion::Any
        ),
        CommandOutcome::Succeeded { .. }
    ));
    // 计算型绑定（前向引用已落地 c1）
    let mut calc_binding = binding("e-calc", 2);
    calc_binding.relation = RelationKind::Calculated;
    calc_binding.calc_ref = Some("c1".into());
    assert!(matches!(
        exec(
            &mut wb,
            WorkbenchCommand::ProposeBinding(calc_binding),
            ExpectedVersion::Any
        ),
        CommandOutcome::Succeeded { .. }
    ));
    let recalc = exec(
        &mut wb,
        WorkbenchCommand::Recalc {
            calc_id: "c1".into(),
        },
        ExpectedVersion::Any,
    );
    assert!(matches!(recalc, CommandOutcome::Succeeded { .. }));
    assert_eq!(
        wb.sidecar().evidence[0].calc,
        mdc_evidence::CalcStatus::Passed
    );
    // 审核命令 = 人工动作（阶段2门禁：exec 统一 "ai:glm" 操作者会被
    // 审核允许列表拒绝——审核面显式用人工信封）
    assert!(matches!(
        wb.execute(
            envelope(
                "op-review",
                "key-review",
                ExpectedVersion::Any,
                "human:wang"
            ),
            WorkbenchCommand::Review {
                evidence_id: "e-calc".into(),
                decision: ReviewStatus::Approved
            },
            review_at
        ),
        CommandOutcome::Succeeded { .. }
    ));
    assert!(matches!(
        exec(
            &mut wb,
            WorkbenchCommand::ExportClean {
                dest: export.clone()
            },
            ExpectedVersion::Any
        ),
        CommandOutcome::Succeeded { .. }
    ));
    assert_eq!(
        std::fs::read(&body).unwrap(),
        std::fs::read(&export).unwrap(),
        "命令面干净导出 = 正文原样"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scale_limits_enforced() {
    // 条数门（恰过恰拒）
    assert!(mdc_workbench::check_counts(MAX_EVIDENCE - 1, 0, true, false).is_ok());
    assert!(
        mdc_workbench::check_counts(MAX_EVIDENCE, 0, true, false).is_err(),
        "恰在上限再加拒"
    );
    assert!(mdc_workbench::check_counts(0, MAX_CALCULATIONS - 1, false, true).is_ok());
    assert!(mdc_workbench::check_counts(0, MAX_CALCULATIONS, false, true).is_err());
    // 字节门
    assert!(
        mdc_workbench::check_sidecar_size(MAX_SIDECAR_BYTES).is_ok(),
        "恰在上限过"
    );
    assert!(mdc_workbench::check_sidecar_size(MAX_SIDECAR_BYTES + 1).is_err());
    // 发布单点：临时文件清零 + 原子落盘
    let dir = temp_dir("publish");
    let dest = dir.join("out.bin");
    mdc_workbench::publish(&dest, b"hello").unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
    assert!(
        !dir.join("out.publish.tmp").exists(),
        "临时文件已 rename 清除"
    );
    std::fs::remove_dir_all(&dir).ok();
}
