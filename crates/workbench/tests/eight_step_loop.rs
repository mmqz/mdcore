//! 合成样例八步端到端闭环（S1-R3 验收主测试——全部合成数据，
//! 零真实敏感资料；验收门禁 12 条的执行面锚点）。
//!
//! 场景（通用化表述）：采购方案 DOCX 引用设备报价 XLSX 的数量与
//! 单价，主张小计金额。八步全走真文件（office-io 读写）+ 真 sidecar。

use std::path::PathBuf;

use mdc_evidence::{CalcStatus, Decimal, LinkageStatus, PrecisionRule, RelationKind, ReviewStatus};
use mdc_office_io::docx::DocxWriter;
use mdc_office_io::io::read_book;
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdc_workbench::{
    BindingProposal, CalcInputSpec, CalcProposal, InputSource, Workbench, WorkbenchError,
};

fn policy_for(dir: &std::path::Path) -> mdc_workbench::PathPolicy {
    mdc_workbench::PathPolicy::new(&[dir.to_path_buf()]).expect("temp dir root")
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdc-wb-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 合成 XLSX（报价表）：B7 数量 / C7 单价 / B8、C8 第二行。
fn write_sample_xlsx(path: &std::path::Path, price_c7: f64) {
    let mut wb = WorkbookWriter::new();
    {
        let mut sh = wb.add_sheet("报价表").unwrap();
        sh.write(6, 1, CellValue::Number(12.5), None).unwrap(); // B7 qty
        sh.write(6, 2, CellValue::Number(price_c7), None).unwrap(); // C7 price
        sh.write(7, 1, CellValue::Number(20.0), None).unwrap(); // B8
        sh.write(7, 2, CellValue::Number(100.5), None).unwrap(); // C8
        sh.write(0, 4, CellValue::Text("备注".into()), None)
            .unwrap(); // E1 文本格
    }
    wb.save_to_path(&path.to_string_lossy()).unwrap();
}

/// 合成 DOCX 正文（五段：标题/引言/采购段/小计段/来源段）。
fn write_sample_docx(path: &std::path::Path, subtotal_text: &str) {
    let mut w = DocxWriter::new();
    w.heading(1, "采购方案说明").unwrap();
    w.paragraph_text("本方案所列金额均为含税价，数据来源见文末。");
    w.paragraph_text("本项目采购 A 设备 12.5 件，单价 336.8 万元。");
    w.paragraph_text(subtotal_text);
    w.paragraph_text("报价数据来源于《设备报价表》。");
    w.save_to_path(&path.to_string_lossy()).unwrap();
}

#[test]
fn eight_step_closed_loop() {
    let dir = temp_dir("loop");
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.docx.evidence.json");
    let export = dir.join("plan-clean-export.docx");

    // —— 夹具（合成样例）——
    write_sample_xlsx(&xlsx, 336.8);
    write_sample_docx(&body, "A 设备小计 4210.00 万元。");

    let t = &mut 1000u64;
    let mut tick = || {
        *t += 100;
        *t
    };

    // ② 登记 DOCX 正文（建 sidecar）
    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar_path,
        &body,
        "body-1",
        tick(),
        "human:setup",
    )
    .unwrap();
    assert_eq!(wb.sidecar().document.doc_id.as_str(), "body-1");

    // ① 导入 XLSX 来源
    let src_ref = wb.ingest_source("src-1", &xlsx, tick(), "ai:glm").unwrap();
    assert_eq!(src_ref.engine, mdc_evidence::EngineKind::Xlsx);
    assert_eq!(wb.sidecar().source_docs.len(), 1);

    // ③ 关联（AI 提议 / 程序验证——数值与摘录全部程序构造）
    wb.propose_binding(
        BindingProposal {
            evidence_id: "e-quote".into(),
            claim: "A 设备采购数量 12.5 件".into(),
            claimed_value: Some(Decimal::parse("12.5").unwrap()),
            paragraph_index: 2,
            source_doc_id: "src-1".into(),
            sheet: "报价表".into(),
            coord: "B7".into(),
            relation: RelationKind::DirectQuote,
            calc_ref: None,
            generation_batch: "batch-1".into(),
        },
        tick(),
        "ai:glm",
    )
    .unwrap();
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
            calc_ref: Some("calc-1".into()),
            generation_batch: "batch-1".into(),
        },
        tick(),
        "ai:glm",
    )
    .unwrap();
    let sc = wb.sidecar();
    assert_eq!(sc.evidence.len(), 2);
    assert!(
        sc.evidence
            .iter()
            .all(|e| e.review == ReviewStatus::Pending),
        "AI 提议一律 Pending——不因程序验证通过而自动过审"
    );
    // 摘录由程序构造（含真实格值）
    assert!(sc.evidence[0].excerpt.contains("B7"));
    assert!(sc.evidence[0].excerpt.contains("n:12.5"));

    // ④ 计算（程序提取数值 + 求值）
    let value = wb
        .propose_calculation(
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
            tick(),
            "ai:glm",
        )
        .unwrap();
    assert_eq!(
        value.to_string(),
        "4210.000",
        "12.5 × 336.8 = 4210.000（目标标度 3）"
    );
    // 复算闭环：现值一致 → 通过；e-calc 主张值吻合 → Passed
    let out = wb.recalc_current("calc-1", tick(), "system").unwrap();
    assert_eq!(out.status, CalcStatus::Passed);
    let sc = wb.sidecar();
    let e_calc = sc
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-calc")
        .unwrap();
    assert_eq!(e_calc.calc, CalcStatus::Passed);
    let e_quote = sc
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-quote")
        .unwrap();
    assert_eq!(
        e_quote.calc,
        CalcStatus::NotApplicable,
        "引用型主张计算面不适用"
    );

    // ⑤ 报告（三态分列）
    let report = wb.report();
    assert!(report.contains("e-calc"));
    assert!(report.contains("通过"));
    assert!(report.contains("qty * price"));
    assert!(report.contains("4210.00"), "报告含登记值与主张值");
    assert!(report.contains("qty * price = 4210.00"), "报告含乘法子步");
    assert!(
        report.contains("rescale(4210.00 -> scale 3, HalfUp) = 4210.000"),
        "报告含 rescale 轨迹行"
    );

    // ⑥ 改写失效检测（来源侧：C7 单价修正 336.8 → 400.0）
    write_sample_xlsx(&xlsx, 400.0);
    wb.ingest_source("src-1", &xlsx, tick(), "human:wang")
        .unwrap();
    let sc = wb.sidecar();
    assert!(
        sc.evidence
            .iter()
            .all(|e| e.linkage == LinkageStatus::Stale),
        "来源语义指纹变化 → 全部证据只降不升（Stale）"
    );
    // 复算：linkage 非 Valid → 级联诚实性（InputUnavailable，不经复算）
    let out2 = wb.recalc_current("calc-1", tick(), "system").unwrap();
    let sc = wb.sidecar();
    let e_calc = sc
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e-calc")
        .unwrap();
    assert_eq!(
        e_calc.calc,
        CalcStatus::Failed(mdc_evidence::CalcFailure::InputUnavailable),
        "来源不实不许复算通过"
    );
    // 计算记录自身：现值 ≠ 登记值（336.8 → 400.0）
    let rec = sc
        .calculations
        .iter()
        .find(|c| c.calc_id == "calc-1")
        .unwrap();
    assert_eq!(
        rec.calc_status,
        CalcStatus::Failed(mdc_evidence::CalcFailure::InputMismatch)
    );
    let _ = out2;

    // ⑦ 审核清单 + 人工动作
    let pending = wb.pending_reviews();
    assert_eq!(pending.len(), 2, "失效不改审核面（三态正交）");
    wb.review("e-calc", ReviewStatus::Rejected, tick(), "human:wang")
        .unwrap();
    // 人工决策不静默复位
    assert!(matches!(
        wb.review("e-calc", ReviewStatus::Pending, tick(), "human:wang"),
        Err(WorkbenchError::Sidecar(_))
    ));

    // ⑧ 干净导出（正文零痕迹）
    wb.export_clean(&export, tick(), "human:wang").unwrap();
    let body_bytes = std::fs::read(&body).unwrap();
    let export_bytes = std::fs::read(&export).unwrap();
    assert_eq!(body_bytes, export_bytes, "导出 = 正文原样字节");
    // sidecar 独立性：删除 sidecar 后正文与导出照常可用
    std::fs::remove_file(&sidecar_path).unwrap();
    let book = read_book(&xlsx.to_string_lossy()).unwrap();
    assert_eq!(book.sheets.len(), 1);
    let doc = mdc_office_io::docx::read_docx(&export.to_string_lossy()).unwrap();
    assert_eq!(
        doc.blocks.len(),
        5,
        "导出文档结构完整（Word/WPS 可开的最小代理）"
    );
    assert!(!sidecar_path.exists());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn source_correction_full_recovery_path() {
    let dir = temp_dir("recovery");
    let xlsx = dir.join("q.xlsx");
    let body = dir.join("p.docx");
    let sidecar_path = dir.join("p.docx.evidence.json");

    write_sample_xlsx(&xlsx, 336.8);
    write_sample_docx(&body, "A 设备小计 4210.00 万元。");

    let t = &mut 5000u64;
    let mut tick = || {
        *t += 100;
        *t
    };

    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar_path,
        &body,
        "body-1",
        tick(),
        "human:setup",
    )
    .unwrap();
    wb.ingest_source("src-1", &xlsx, tick(), "ai:glm").unwrap();
    wb.propose_binding(
        BindingProposal {
            evidence_id: "e1".into(),
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
        tick(),
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
        tick(),
        "ai:glm",
    )
    .unwrap();
    wb.recalc_current("c1", tick(), "system").unwrap();

    // —— 来源修正（C7 → 400.0）+ 显式重验 ——
    write_sample_xlsx(&xlsx, 400.0);
    wb.ingest_source("src-1", &xlsx, tick(), "human:wang")
        .unwrap();
    assert_eq!(wb.sidecar().evidence[0].linkage, LinkageStatus::Stale);

    // 显式重验（source 侧唯一恢复通道）：重读当前文件重绑 B7
    wb.reverify_source("e1", "src-1", "报价表", "B7", tick(), "human:wang")
        .unwrap();
    assert_eq!(
        wb.sidecar().evidence[0].linkage,
        LinkageStatus::Valid,
        "显式重验恢复"
    );

    // 复算：登记输入（336.8）≠ 现值（400.0）→ InputMismatch（记录失真）
    let out = wb.recalc_current("c1", tick(), "system").unwrap();
    assert_eq!(
        out.status,
        CalcStatus::Failed(mdc_evidence::CalcFailure::InputMismatch)
    );
    // 旧证据被拒（主张 4210 已不被当前来源支持）
    wb.review("e1", ReviewStatus::Rejected, tick(), "human:wang")
        .unwrap();

    // —— 新计算（当前来源现值）+ 正文更正 + 新证据 ——
    let v2 = wb
        .propose_calculation(
            CalcProposal {
                calc_id: "c2".into(),
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
            tick(),
            "ai:glm",
        )
        .unwrap();
    assert_eq!(v2.to_string(), "5000.00");

    // 正文更正（小计段改 5000.00）→ refresh_body → 旧证据 claim 锚断
    write_sample_docx(&body, "A 设备小计 5000.00 万元。");
    let degraded = wb.refresh_body(tick(), "human:wang").unwrap();
    assert_eq!(degraded, 1, "e1 的 claim 锚（段落 3）已断——claim 侧失效");
    assert_eq!(wb.sidecar().evidence[0].linkage, LinkageStatus::Stale);
    // e1 已 Rejected + Stale——终态诚实（审核与关联正交，互不复位）

    // 新证据绑定到更正后的正文段（fail-closed 已通过 refresh 解锁）
    wb.propose_binding(
        BindingProposal {
            evidence_id: "e2".into(),
            claim: "A 设备小计 5000.00 万元".into(),
            claimed_value: Some(Decimal::parse("5000.00").unwrap()),
            paragraph_index: 3,
            source_doc_id: "src-1".into(),
            sheet: "报价表".into(),
            coord: "B7".into(),
            relation: RelationKind::Calculated,
            calc_ref: Some("c2".into()),
            generation_batch: "b2".into(),
        },
        tick(),
        "ai:glm",
    )
    .unwrap();
    let out2 = wb.recalc_current("c2", tick(), "system").unwrap();
    assert_eq!(out2.status, CalcStatus::Passed);
    let e2 = wb
        .sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == "e2")
        .unwrap();
    assert_eq!(e2.calc, CalcStatus::Passed);
    assert_eq!(e2.linkage, LinkageStatus::Valid);
    assert_eq!(e2.review, ReviewStatus::Pending, "仍待人工——通过≠过审");
    wb.review("e2", ReviewStatus::Approved, tick(), "human:wang")
        .unwrap();

    // 审计链完整性：重验/降级/审核/正文刷新事件全在场
    let kinds: Vec<&str> = wb.sidecar().audit.iter().map(|e| e.kind.as_str()).collect();
    for expected in [
        "linkage_degraded",
        "linkage_reverified",
        "body_refresh",
        "evidence_review",
        "calc_recalc",
    ] {
        assert!(kinds.contains(&expected), "审计缺 {expected}");
    }
    // 序号单调无空洞
    let seqs: Vec<u64> = wb.sidecar().audit.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn proposals_fail_closed_on_stale_views_and_bad_targets() {
    let dir = temp_dir("failclosed");
    let xlsx = dir.join("q.xlsx");
    let body = dir.join("p.docx");
    let sidecar_path = dir.join("p.docx.evidence.json");

    write_sample_xlsx(&xlsx, 336.8);
    write_sample_docx(&body, "A 设备小计 4210.00 万元。");

    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar_path,
        &body,
        "body-1",
        100,
        "human:setup",
    )
    .unwrap();
    wb.ingest_source("src-1", &xlsx, 200, "ai:glm").unwrap();

    let proposal = |par: u32, coord: &str, value: Option<Decimal>| BindingProposal {
        evidence_id: format!("e-{par}-{coord}"),
        claim: "x".into(),
        claimed_value: value,
        paragraph_index: par,
        source_doc_id: "src-1".into(),
        sheet: "报价表".into(),
        coord: coord.into(),
        relation: RelationKind::DirectQuote,
        calc_ref: None,
        generation_batch: "b".into(),
    };

    // 段落越界
    assert!(matches!(
        wb.propose_binding(proposal(99, "B7", None), 300, "ai:glm"),
        Err(WorkbenchError::ParagraphNotFound { .. })
    ));
    // 主张数值不在段落文本
    assert!(matches!(
        wb.propose_binding(
            proposal(3, "B7", Some(Decimal::parse("9999.99").unwrap())),
            300,
            "ai:glm"
        ),
        Err(WorkbenchError::ClaimedValueNotInParagraph { .. })
    ));
    // 单元格不存在
    assert!(matches!(
        wb.propose_binding(proposal(2, "Z99", None), 300, "ai:glm"),
        Err(WorkbenchError::CellNotFound { .. })
    ));

    // 正文已变未刷新 → 拒绑（fail-closed）
    write_sample_docx(&body, "A 设备小计 9999.00 万元。");
    assert!(matches!(
        wb.propose_binding(proposal(2, "B7", None), 300, "ai:glm"),
        Err(WorkbenchError::BodyStale)
    ));
    // 来源已变未重登记 → 拒绑
    write_sample_xlsx(&xlsx, 400.0);
    write_sample_docx(&body, "A 设备小计 4210.00 万元。"); // 恢复正文（只测来源侧）
    wb.refresh_body(310, "human:wang").unwrap(); // 解锁正文
    assert!(matches!(
        wb.propose_binding(proposal(2, "B7", None), 320, "ai:glm"),
        Err(WorkbenchError::SourceStale { .. })
    ));

    // 计算面：文本格作输入 / 重复 calc_id
    wb.ingest_source("src-1", &xlsx, 330, "human:wang").unwrap(); // 解锁来源
    let text_input = CalcInputSpec {
        name: "note".into(),
        source: InputSource::Cell {
            doc_id: "src-1".into(),
            sheet: "报价表".into(),
            coord: "E1".into(),
        },
        unit: None,
    };
    let calc = |id: &str, inputs: Vec<CalcInputSpec>| CalcProposal {
        calc_id: id.into(),
        expression: "note + 1".into(),
        unit: None,
        precision: PrecisionRule {
            target_scale: 2,
            rounding: mdc_evidence::Rounding::HalfUp,
        },
        inputs,
    };
    assert!(matches!(
        wb.propose_calculation(calc("c-x", vec![text_input]), 340, "ai:glm"),
        Err(WorkbenchError::CellNotNumeric { .. })
    ));
    // 可数值计算先落地，再测重复 id
    let ok_inputs = vec![CalcInputSpec {
        name: "qty".into(),
        source: InputSource::Cell {
            doc_id: "src-1".into(),
            sheet: "报价表".into(),
            coord: "B7".into(),
        },
        unit: None,
    }];
    let ok_calc = CalcProposal {
        calc_id: "c-ok".into(),
        expression: "qty * 2".into(),
        unit: None,
        precision: PrecisionRule {
            target_scale: 2,
            rounding: mdc_evidence::Rounding::HalfUp,
        },
        inputs: ok_inputs.clone(),
    };
    wb.propose_calculation(ok_calc, 350, "ai:glm").unwrap();
    assert!(matches!(
        wb.propose_calculation(calc("c-ok", ok_inputs), 360, "ai:glm"),
        Err(WorkbenchError::DuplicateCalcId { .. })
    ));
    // 未登记来源
    let unreg = CalcProposal {
        calc_id: "c-unreg".into(),
        expression: "qty * 2".into(),
        unit: None,
        precision: PrecisionRule {
            target_scale: 2,
            rounding: mdc_evidence::Rounding::HalfUp,
        },
        inputs: vec![CalcInputSpec {
            name: "qty".into(),
            source: InputSource::Cell {
                doc_id: "no-such".into(),
                sheet: "报价表".into(),
                coord: "B7".into(),
            },
            unit: None,
        }],
    };
    assert!(matches!(
        wb.propose_calculation(unreg, 370, "ai:glm"),
        Err(WorkbenchError::SourceNotRegistered { .. })
    ));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn clean_export_detects_markers() {
    let dir = temp_dir("markers");
    let body = dir.join("p.docx");
    let sidecar_path = dir.join("p.docx.evidence.json");
    let export = dir.join("out.docx");

    // 正文被人为塞入工作台标记（模拟污染）→ 导出验证必须抓住
    let mut w = DocxWriter::new();
    w.heading(1, "正常标题").unwrap();
    w.paragraph_text("这段正文包含 evidence_id 污染标记。");
    w.save_to_path(&body.to_string_lossy()).unwrap();

    let mut wb = Workbench::start(
        policy_for(&dir),
        &sidecar_path,
        &body,
        "body-1",
        100,
        "human:setup",
    )
    .unwrap();
    assert!(matches!(
        wb.export_clean(&export, 200, "human:wang"),
        Err(WorkbenchError::MarkerFound { .. })
    ));
    assert!(!export.exists(), "验证失败不产出已发布结果");

    std::fs::remove_dir_all(&dir).ok();
}
