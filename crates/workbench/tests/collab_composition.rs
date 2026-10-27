//! 集成项② 组合测试：workbench 八步闭环 ↔ 协同会话（交付报告 §3.2）。
//!
//! **协同文档作为受证正文**：正文不再只是静态 DOCX 文件——发布为
//! mdc-collab 协同会话（经 mdc-mdbridge），人类/AI 经同一命令接口
//! 并发编辑，编辑产物落盘后 workbench 的失效检测/重绑/干净导出
//! 全链照常运转。
//!
//! 钉定面：
//! - 协同正文身份连续（发布后 session 读模型 = 原 DOCX 段落序）；
//! - AI 润色受证段 → 恰该主张 Stale（选择性失效——指纹面），
//!   相邻未触及主张保持 Valid；
//! - 人工重绑（§四.5 润色后允许失效 → reverify）恢复 Valid；
//! - 会话撤销（AI 润色撤回）→ 正文回退 → 再失效 → 再重绑恢复；
//! - 双端收敛（Human 对端编辑 + AI 本端编辑）后落盘单点；
//! - 干净导出 = 协同终态（零标记 + 可解析）。

use std::path::PathBuf;

use mdc_evidence::{CalcStatus, Decimal, LinkageStatus, PrecisionRule, RelationKind, ReviewStatus};
use mdc_mdbridge::MdCollabHost;
use mdc_office_io::docx::{read_docx_from_bytes, DocxWriter};
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdc_workbench::{BindingProposal, CalcInputSpec, CalcProposal, InputSource, Workbench};
use mdcore::op::Op;

const SUBTOTAL: &str = "A 设备小计 4210.00 万元。";
const POLISH_INSERT: &str = "合计";

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wb-collab-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn policy_for(dir: &std::path::Path) -> mdc_workbench::PathPolicy {
    mdc_workbench::PathPolicy::new(&[dir.to_path_buf()]).expect("temp dir root")
}

fn write_sample_xlsx(path: &std::path::Path, price_c7: f64) {
    let mut w = WorkbookWriter::new();
    let mut sh = w.add_sheet("报价表").unwrap();
    sh.write(6, 1, CellValue::Number(12.5), None).unwrap(); // B7 qty
    sh.write(6, 2, CellValue::Number(price_c7), None).unwrap(); // C7 price
    w.save_to_path(&path.to_string_lossy()).unwrap();
}

fn write_sample_docx(path: &std::path::Path, subtotal_text: &str) {
    let mut w = DocxWriter::new();
    w.heading(1, "采购方案说明").unwrap();
    w.paragraph_text("本方案所列金额均为含税价，数据来源见文末。");
    w.paragraph_text("本项目采购 A 设备 12.5 件，单价 336.8 万元。");
    w.paragraph_text(subtotal_text);
    w.paragraph_text("报价数据来源于《设备报价表》。");
    w.save_to_path(&path.to_string_lossy()).unwrap();
}

/// 正文段落文本序（office-io 读面——与 ingest 的段落序一致）。
fn docx_paragraphs(path: &std::path::Path) -> Vec<String> {
    let bytes = std::fs::read(path).unwrap();
    let doc = read_docx_from_bytes(&bytes).unwrap();
    let mut out = vec![];
    for block in &doc.blocks {
        match block {
            mdc_office_io::docx::Block::Paragraph(p) => out.push(p.text()),
            mdc_office_io::docx::Block::Table(t) => {
                for row in &t.rows {
                    for cell in row {
                        out.push(cell.clone());
                    }
                }
            }
            // #18：文本框内层递归（协同体文本收集——测试面）。
            mdc_office_io::docx::Block::TextBox(tb) => {
                for inner in &tb.blocks {
                    if let mdc_office_io::docx::Block::Paragraph(p) = inner {
                        out.push(p.text());
                    }
                }
            }
        }
    }
    out
}

/// 协同终态 → DOCX 落盘（检查点写回——会话是唯一编辑面，文件是
/// 持久化投影）。
fn checkpoint_write_body(host: &MdCollabHost, path: &std::path::Path) {
    let doc = host.session().document();
    let mut w = DocxWriter::new();
    for b in &doc {
        match b.kind {
            mdc_collab::BlockKind::Heading => {
                w.heading(1, &b.text).unwrap();
            }
            _ => {
                w.paragraph_text(&b.text);
            }
        }
    }
    w.save_to_path(&path.to_string_lossy()).unwrap();
}

/// 建立工作台 + 双绑定 + 计算（既有八步闭环机件——Act 1 复用）。
fn setup_with_two_claims(dir: &std::path::Path) -> (Workbench, std::path::PathBuf) {
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.docx.evidence.json");
    write_sample_xlsx(&xlsx, 336.8);
    write_sample_docx(&body, SUBTOTAL);

    let mut wb = Workbench::start(
        policy_for(dir),
        &sidecar_path,
        &body,
        "body-1",
        1000,
        "human:setup",
    )
    .unwrap();
    wb.ingest_source("src-1", &xlsx, 1100, "ai:glm").unwrap();
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
        1200,
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
        1300,
        "ai:glm",
    )
    .unwrap();
    let v = wb
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
            1400,
            "ai:glm",
        )
        .unwrap();
    assert_eq!(v.to_string(), "4210.000");
    let out = wb.recalc_current("calc-1", 1500, "system").unwrap();
    assert_eq!(out.status, CalcStatus::Passed);
    (wb, body)
}

fn linkage_of(wb: &Workbench, id: &str) -> LinkageStatus {
    wb.sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == id)
        .unwrap()
        .linkage
}

/// 由正文段落文本构建 mdcore BlockTree（标题 + 段落——与 DOCX 夹具
/// 同构；发布通道的源树）。
fn body_tree_from_paragraphs(paragraphs: &[String]) -> mdcore::tree::BlockTree {
    let mut md = String::new();
    for (i, p) in paragraphs.iter().enumerate() {
        if i == 0 {
            md.push_str(&format!("# {p}\n"));
        } else {
            md.push_str(&format!("\n{p}\n"));
        }
    }
    mdcore::markdown::markdown_to_tree(&md)
}

#[test]
fn collab_body_ai_polish_stale_rebind_and_undo_recovery() {
    let dir = temp_dir("polish");
    let (mut wb, body) = setup_with_two_claims(&dir);
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Valid);

    // ── 协同正文上线：发布（重映射身份）+ 读模型段落序一致 ──
    let paras = docx_paragraphs(&body);
    let src_tree = body_tree_from_paragraphs(&paras);
    let mut a = MdCollabHost::open(mdc_collab::SessionOptions {
        client_id: 7_500_001,
        ..Default::default()
    })
    .unwrap();
    let receipt = a.publish(&src_tree, mdc_collab::Actor::Human).unwrap();
    assert_eq!(receipt.blocks, src_tree.node_count(), "发布全块");
    let doc = a.session().document();
    assert_eq!(doc.len(), paras.len(), "会话顶层块 = 正文段落数");
    for (b, p) in doc.iter().zip(paras.iter()) {
        assert_eq!(&b.text, p, "段落文本序一致（协同正文身份）");
    }

    // ── AI 润色受证段（e-calc 锚段 idx 3；数字保持——纯润色）──
    let mut op = Op::ReplaceText {
        path: vec![3],
        start: 6,
        end: 6,
        insert: POLISH_INSERT.into(),
    };
    a.apply_op(mdc_collab::Actor::Ai, &mut op).unwrap();
    let polished: String = {
        let doc = a.session().document();
        doc[3].text.clone()
    };
    assert_eq!(
        polished,
        format!("A 设备小计{POLISH_INSERT} 4210.00 万元。")
    );

    // ── 落盘 + 失效检测：恰 e-calc Stale（选择性）──
    checkpoint_write_body(&a, &body);
    let degraded = wb.refresh_body(2000, "human:sync").unwrap();
    assert_eq!(degraded, 1, "恰一条降级");
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Stale);
    assert_eq!(
        linkage_of(&wb, "e-quote"),
        LinkageStatus::Valid,
        "未触及段落的主张不失效（选择性）"
    );

    // ── 人工重绑（§四.5 润色后允许失效 → reverify 恢复 Valid → 审核通过。
    // 阶段2审核门禁的次序守护：过审前必须先显式重验——先审后验的旧序
    // 在严格档下被拒，人工动作批次内的次序本非本测试语义本体）──
    wb.reverify_claim("e-calc", 3, 2100, "human:reviewer")
        .unwrap();
    wb.review("e-calc", ReviewStatus::Approved, 2200, "human:reviewer")
        .unwrap();
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Valid);

    // ── 撤销恢复路径：AI 撤回润色 → 正文回退 → 再失效 → 再重绑 ──
    assert!(a.undo(mdc_collab::Actor::Ai).unwrap(), "undo");
    checkpoint_write_body(&a, &body);
    let degraded = wb.refresh_body(2300, "human:sync").unwrap();
    assert_eq!(degraded, 1, "回退也触发失效（指纹面双向）");
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Stale);
    let restored = docx_paragraphs(&body);
    assert_eq!(restored[3], SUBTOTAL, "正文回退到润色前");
    wb.reverify_claim("e-calc", 3, 2400, "human:reviewer")
        .unwrap();
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Valid);

    // ── 干净导出 = 协同终态 ──
    let export = dir.join("clean.docx");
    wb.export_clean(&export, 2500, "human:exporter").unwrap();
    let out_paras = docx_paragraphs(&export);
    assert_eq!(out_paras[3], SUBTOTAL);
    assert_eq!(out_paras.len(), paras.len());
}

#[test]
fn collab_body_two_peer_convergence_human_and_ai() {
    // 双端并发（A 端 AI 润色受证段 + B 端人类补记来源段）→ 收敛 →
    // 单点落盘 → 选择性失效（e-calc Stale / e-quote Valid）→ 重绑。
    let dir = temp_dir("converge");
    let (mut wb, body) = setup_with_two_claims(&dir);

    let paras = docx_paragraphs(&body);
    let src_tree = body_tree_from_paragraphs(&paras);
    let mut a = MdCollabHost::open(mdc_collab::SessionOptions {
        client_id: 7_500_011,
        ..Default::default()
    })
    .unwrap();
    let mut b = MdCollabHost::open(mdc_collab::SessionOptions {
        client_id: 7_500_012,
        ..Default::default()
    })
    .unwrap();
    a.publish(&src_tree, mdc_collab::Actor::Human).unwrap();

    // 双向同步（A→B 建立共享态）。
    let sv_b = b.state_vector();
    let diff_ab = a.encode_diff(&sv_b).unwrap();
    b.apply_remote(&diff_ab).unwrap();

    // A 端 AI 润色 idx3；B 端人类补记 idx4（末段追加）。
    let mut op_ai = Op::ReplaceText {
        path: vec![3],
        start: 6,
        end: 6,
        insert: POLISH_INSERT.into(),
    };
    a.apply_op(mdc_collab::Actor::Ai, &mut op_ai).unwrap();
    let mut op_human = Op::ReplaceText {
        path: vec![4],
        start: 15,
        end: 15,
        insert: "双方已确认。".into(),
    };
    b.apply_op(mdc_collab::Actor::Human, &mut op_human).unwrap();

    // 双向再收敛（A 的 AI 润色 → B；B 的人类补记 → A）。
    let sv_a = a.state_vector();
    let sv_b = b.state_vector();
    let diff_ba = b.encode_diff(&sv_a).unwrap();
    let diff_ab = a.encode_diff(&sv_b).unwrap();
    a.apply_remote(&diff_ba).unwrap();
    b.apply_remote(&diff_ab).unwrap();
    let doc_a = a.session().document();
    let doc_b = b.session().document();
    assert_eq!(doc_a.len(), doc_b.len());
    for (x, y) in doc_a.iter().zip(doc_b.iter()) {
        assert_eq!(x.text, y.text, "双端收敛");
    }
    assert_eq!(doc_a[4].text, "报价数据来源于《设备报价表》。双方已确认。");

    // 单点落盘（A 为本检检查点端）+ 选择性失效。
    checkpoint_write_body(&a, &body);
    let degraded = wb.refresh_body(3200, "human:sync").unwrap();
    assert_eq!(degraded, 1, "AI 润色段失效");
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Stale);
    assert_eq!(linkage_of(&wb, "e-quote"), LinkageStatus::Valid);
    wb.reverify_claim("e-calc", 3, 3300, "human:reviewer")
        .unwrap();
    wb.review("e-calc", ReviewStatus::Approved, 3400, "human:reviewer")
        .unwrap();
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Valid);
}

#[test]
fn collab_body_remote_event_observed_by_raw_peer() {
    // 组合②×③：裸对端会话订阅事件——AI 润色经 wire 到达时 Remote
    // 事件 + updates 可再转发（事件面在协同正文组合中的观测面）。
    use std::sync::{Arc, Mutex};

    let dir = temp_dir("events");
    let (_wb, body) = setup_with_two_claims(&dir);
    let paras = docx_paragraphs(&body);
    let src_tree = body_tree_from_paragraphs(&paras);

    let mut a = MdCollabHost::open(mdc_collab::SessionOptions {
        client_id: 7_500_021,
        ..Default::default()
    })
    .unwrap();
    a.publish(&src_tree, mdc_collab::Actor::Human).unwrap();

    let mut c = mdc_collab::CollabSession::open(mdc_collab::SessionOptions {
        client_id: 7_500_022,
        ..Default::default()
    })
    .unwrap();
    // 初始共享态。
    let sv_c = c.state_vector();
    let diff = a.encode_diff(&sv_c).unwrap();
    c.apply_update(&diff).unwrap();

    let seen: Arc<Mutex<Vec<mdc_collab::CollabEvent>>> = Arc::new(Mutex::new(vec![]));
    let sink = Arc::clone(&seen);
    let _sub = c.subscribe(move |e: &mdc_collab::CollabEvent| {
        sink.lock().unwrap().push(e.clone());
    });

    // A 端 AI 润色 → wire → C 端 Remote 事件。
    let mut op = Op::ReplaceText {
        path: vec![3],
        start: 6,
        end: 6,
        insert: POLISH_INSERT.into(),
    };
    a.apply_op(mdc_collab::Actor::Ai, &mut op).unwrap();
    let sv_c = c.state_vector();
    let diff = a.encode_diff(&sv_c).unwrap();
    c.apply_update(&diff).unwrap();

    let events = seen.lock().unwrap().clone();
    assert_eq!(events.len(), 1, "一 apply 一事件");
    assert_eq!(events[0].source, mdc_collab::ChangeSource::Remote);
    assert!(!events[0].updates.is_empty(), "updates 可转发");
    let doc = c.document();
    assert_eq!(
        doc[3].text,
        format!("A 设备小计{POLISH_INSERT} 4210.00 万元。")
    );
}
