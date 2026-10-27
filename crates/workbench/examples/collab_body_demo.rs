//! 集成项② 可运行演示：协同文档作为受证正文（交付报告 §3.2）。
//!
//! ```text
//! cargo run -p mdc-workbench --example collab_body_demo
//! ```
//!
//! 五幕 12 检查点（断言内嵌——非零退出码即失败）：
//!
//! - 幕一 证据闭环：XLSX 来源 + 双绑定（引用/计算）+ 程序求值通过。
//! - 幕二 协同正文上线：DOCX 正文发布为协同会话（mdbridge 重映射），
//!   读模型与原正文段落序一致。
//! - 幕三 并发编辑：A 端 AI 润色受证段 + B 端人类补记来源段 →
//!   双向收敛；裸对端 C 观察 Remote 事件（组合②×③）。
//! - 幕四 失效检测：协同终态落盘 → 恰受证主张 Stale（选择性）。
//! - 幕五 人工闭环：审核通过 + 重绑恢复 → AI 撤销润色回退 →
//!   再失效再重绑 → 干净导出 = 协同终态零标记。
//!
//! 架构语义：会话是唯一编辑面（真相源），文件是持久化投影（检查点
//! 落盘）；workbench 的失效/重绑/导出机件对协同正文照常运转。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mdc_evidence::{CalcStatus, Decimal, LinkageStatus, PrecisionRule, RelationKind, ReviewStatus};
use mdc_mdbridge::MdCollabHost;
use mdc_office_io::docx::{read_docx_from_bytes, DocxWriter};
use mdc_office_io::write::{CellValue, WorkbookWriter};
use mdc_workbench::{BindingProposal, CalcInputSpec, CalcProposal, InputSource, Workbench};
use mdcore::op::Op;

const SUBTOTAL: &str = "A 设备小计 4210.00 万元。";
const POLISH_INSERT: &str = "合计";

fn ck(n: usize, msg: &str) {
    println!("✓ 检查点 {n:02}: {msg}");
}

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("wb-collab-demo-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

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
            // #18：文本框内层递归（demo 面）。
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

fn linkage_of(wb: &Workbench, id: &str) -> LinkageStatus {
    wb.sidecar()
        .evidence
        .iter()
        .find(|e| e.evidence_id == id)
        .unwrap()
        .linkage
}

fn main() {
    println!("== 集成项② 协同文档作为受证正文——五幕演示 ==\n");

    // ── 幕一：证据闭环（既有八步机件）──
    let dir = temp_dir();
    let xlsx = dir.join("quote.xlsx");
    let body = dir.join("plan.docx");
    let sidecar_path = dir.join("plan.docx.evidence.json");
    {
        let mut w = WorkbookWriter::new();
        let mut sh = w.add_sheet("报价表").unwrap();
        sh.write(6, 1, CellValue::Number(12.5), None).unwrap();
        sh.write(6, 2, CellValue::Number(336.8), None).unwrap();
        w.save_to_path(&xlsx.to_string_lossy()).unwrap();
        let mut w = DocxWriter::new();
        w.heading(1, "采购方案说明").unwrap();
        w.paragraph_text("本方案所列金额均为含税价，数据来源见文末。");
        w.paragraph_text("本项目采购 A 设备 12.5 件，单价 336.8 万元。");
        w.paragraph_text(SUBTOTAL);
        w.paragraph_text("报价数据来源于《设备报价表》。");
        w.save_to_path(&body.to_string_lossy()).unwrap();
    }
    let policy = mdc_workbench::PathPolicy::new(std::slice::from_ref(&dir)).unwrap();
    let mut wb =
        Workbench::start(policy, &sidecar_path, &body, "body-1", 1000, "human:setup").unwrap();
    wb.ingest_source("src-1", &xlsx, 1100, "ai:glm").unwrap();
    for (eid, idx, claim, val, rel, calc) in [
        (
            "e-quote",
            2u32,
            "A 设备采购数量 12.5 件",
            "12.5",
            RelationKind::DirectQuote,
            None,
        ),
        (
            "e-calc",
            3u32,
            "A 设备小计 4210.00 万元",
            "4210.00",
            RelationKind::Calculated,
            Some("calc-1"),
        ),
    ] {
        wb.propose_binding(
            BindingProposal {
                evidence_id: eid.into(),
                claim: claim.into(),
                claimed_value: Some(Decimal::parse(val).unwrap()),
                paragraph_index: idx,
                source_doc_id: "src-1".into(),
                sheet: "报价表".into(),
                coord: "B7".into(),
                relation: rel,
                calc_ref: calc.map(|c| c.to_string()),
                generation_batch: "batch-1".into(),
            },
            1200,
            "ai:glm",
        )
        .unwrap();
    }
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
            1300,
            "ai:glm",
        )
        .unwrap();
    assert_eq!(v.to_string(), "4210.000");
    let out = wb.recalc_current("calc-1", 1400, "system").unwrap();
    assert_eq!(out.status, CalcStatus::Passed);
    ck(
        1,
        "幕一：来源摄取 + 双绑定 + 程序求值 12.5×336.8=4210.000 通过",
    );
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Valid);

    // ── 幕二：协同正文上线（发布 = 重映射身份）──
    let paras = docx_paragraphs(&body);
    let mut md = String::new();
    for (i, p) in paras.iter().enumerate() {
        if i == 0 {
            md.push_str(&format!("# {p}\n"));
        } else {
            md.push_str(&format!("\n{p}\n"));
        }
    }
    let src_tree = mdcore::markdown::markdown_to_tree(&md);
    let mut a = MdCollabHost::open(mdc_collab::SessionOptions {
        client_id: 7_500_101,
        ..Default::default()
    })
    .unwrap();
    let receipt = a.publish(&src_tree, mdc_collab::Actor::Human).unwrap();
    assert_eq!(receipt.blocks, src_tree.node_count());
    let doc = a.session().document();
    assert_eq!(doc.len(), paras.len());
    for (b, p) in doc.iter().zip(paras.iter()) {
        assert_eq!(&b.text, p);
    }
    ck(
        2,
        "幕二：DOCX 正文发布为协同会话（5 块重映射），读模型与段落序一致",
    );

    // 对端 B（人类编辑位）+ 裸对端 C（事件观察位——组合②×③）。
    let mut b = MdCollabHost::open(mdc_collab::SessionOptions {
        client_id: 7_500_102,
        ..Default::default()
    })
    .unwrap();
    let mut c = mdc_collab::CollabSession::open(mdc_collab::SessionOptions {
        client_id: 7_500_103,
        ..Default::default()
    })
    .unwrap();
    let sv_b = b.state_vector();
    b.apply_remote(&a.encode_diff(&sv_b).unwrap()).unwrap();
    let sv_c = c.state_vector();
    c.apply_update(&a.encode_diff(&sv_c).unwrap()).unwrap();
    let seen: Arc<Mutex<Vec<mdc_collab::CollabEvent>>> = Arc::new(Mutex::new(vec![]));
    let sink = Arc::clone(&seen);
    let _sub = c.subscribe(move |e: &mdc_collab::CollabEvent| {
        sink.lock().unwrap().push(e.clone());
    });
    ck(3, "幕二：B（人类对端）与 C（事件观察端）加入共享态");

    // ── 幕三：并发编辑 + 双向收敛 ──
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
    ck(
        4,
        "幕三：A 端 AI 润色受证段（数字保持）+ B 端人类补记来源段",
    );

    let sv_a = a.state_vector();
    let sv_b2 = b.state_vector();
    let diff_ba = b.encode_diff(&sv_a).unwrap();
    let diff_ab = a.encode_diff(&sv_b2).unwrap();
    a.apply_remote(&diff_ba).unwrap();
    b.apply_remote(&diff_ab).unwrap();
    let (da, db) = (a.session().document(), b.session().document());
    assert_eq!(da.len(), db.len());
    for (x, y) in da.iter().zip(db.iter()) {
        assert_eq!(x.text, y.text);
    }
    ck(5, "幕三：双向同步后 A/B 读模型逐段一致（CRDT 收敛）");

    let sv_c = c.state_vector();
    c.apply_update(&a.encode_diff(&sv_c).unwrap()).unwrap();
    let events = seen.lock().unwrap().clone();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, mdc_collab::ChangeSource::Remote);
    assert!(!events[0].updates.is_empty());
    ck(
        6,
        "幕三：C 端观察 Remote 事件（组合②×③——updates 可直接转发）",
    );

    // ── 幕四：失效检测（协同终态落盘 → 选择性失效）──
    checkpoint_write_body(&a, &body);
    let degraded = wb.refresh_body(2000, "human:sync").unwrap();
    assert_eq!(degraded, 1);
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Stale);
    assert_eq!(linkage_of(&wb, "e-quote"), LinkageStatus::Valid);
    ck(7, "幕四：AI 润色段主张 e-calc → Stale");
    ck(8, "幕四：相邻主张 e-quote 保持 Valid（选择性失效——指纹面）");

    // ── 幕五：人工闭环（重绑 + 审核 + 撤销回退 + 干净导出）。
    // 阶段2审核门禁次序：先显式重验恢复 Valid → 再过审（先审后验
    // 的旧序被严格档拒绝——人工只能核验过关联的证据）
    wb.reverify_claim("e-calc", 3, 2100, "human:reviewer")
        .unwrap();
    wb.review("e-calc", ReviewStatus::Approved, 2200, "human:reviewer")
        .unwrap();
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Valid);
    ck(
        9,
        "幕五：人工审核通过 + 重绑（§四.5 润色后允许失效）→ Valid",
    );

    assert!(a.undo(mdc_collab::Actor::Ai).unwrap());
    checkpoint_write_body(&a, &body);
    let degraded = wb.refresh_body(2300, "human:sync").unwrap();
    assert_eq!(degraded, 1);
    assert_eq!(docx_paragraphs(&body)[3], SUBTOTAL);
    ck(10, "幕五：AI 撤销润色 → 正文回退（指纹双向失效）");

    wb.reverify_claim("e-calc", 3, 2400, "human:reviewer")
        .unwrap();
    assert_eq!(linkage_of(&wb, "e-calc"), LinkageStatus::Valid);
    let export = dir.join("clean.docx");
    wb.export_clean(&export, 2500, "human:exporter").unwrap();
    let out_paras = docx_paragraphs(&export);
    assert_eq!(out_paras.len(), paras.len());
    assert_eq!(out_paras[3], SUBTOTAL);
    assert_eq!(out_paras[4], "报价数据来源于《设备报价表》。双方已确认。");
    ck(
        11,
        "幕五：重绑恢复 Valid + 干净导出（回退态 + 人类补记保留）",
    );
    ck(
        12,
        "幕五：导出零工作台标记 + 可解析（export_clean 双验证内置）",
    );

    println!("\n== 五幕 12 检查点全部通过——协同正文 × 证据闭环组合闭合 ==");
    println!("产物目录: {}", dir.display());
}
