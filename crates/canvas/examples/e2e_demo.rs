//! 端到端可运行演示（用户指令④）——AI + Human 文档工作台全链：
//! 文档协作 × 画布协作 × 证据关联 × 撤销/重做 × 删除与复原。
//!
//! 运行：`cargo run -p mdc-canvas --example e2e_demo`
//! 全程断言自检（任何环节失守 = 非零退出）；输出 = 分幕叙事 +
//! [PASS] 检查点——独立单测之外的可运行演示形态。
//!
//! 场景（全部合成数据）：
//! 人类工程师 + AI 助手协作编写设备采购方案：AI 生成文档骨架，
//! 人类修订正文；人类在画布上搭报价图（节点 = 报价/小计，边 =
//! 推导关系），AI 并发补充；证据系统把画布小计节点与文档主张
//! 挂钩；拖动/改写触发失效，重验恢复，删除撤销后身份复原。

use std::collections::BTreeMap;

use mdc_canvas::{Actor as CActor, CanvasCommand, CanvasSession, SessionOptions as CanvasOpts};
use mdc_collab::{
    Actor as DActor, BlockKind, BlockSpec, CollabCommand, CollabSession, SessionOptions as DocOpts,
};
use mdc_evidence::{
    CalcStatus, DocId, DocumentRef, EvidenceRecord, LinkageStatus, Locator, RelationKind,
    ReviewStatus, Sidecar, VersionStamp,
};

fn step(n: usize, title: &str) {
    println!("\n== 第{n}幕：{title} ==");
}

fn pass(msg: &str) {
    println!("  [PASS] {msg}");
}

fn doc_opts(id: u64) -> DocOpts {
    DocOpts {
        client_id: id,
        ..Default::default()
    }
}

fn canvas_opts(id: u64) -> CanvasOpts {
    CanvasOpts {
        client_id: id,
        ..Default::default()
    }
}

fn sync_doc(from: &CollabSession, to: &CollabSession) {
    let sv = to.state_vector();
    let diff = from.encode_diff(&sv).unwrap();
    to.apply_update(&diff).unwrap();
}

fn sync_canvas(from: &CanvasSession, to: &CanvasSession) {
    let sv = to.state_vector();
    let diff = from.encode_diff(&sv).unwrap();
    to.apply_update(&diff).unwrap();
}

fn main() {
    println!("==============================================================");
    println!(" AI + Human 文档工作台 —— 端到端协作演示（headless，断言自检）");
    println!(" 文档域 mdc-collab × 画布域 mdc-canvas × 证据域 mdc-evidence");
    println!("==============================================================");

    // ── 第 1 幕：文档协作（AI 生成 + 人类修订 + 并发收敛）───────
    step(1, "文档协作——AI 生成骨架，人类修订，并发收敛");

    let mut ai_doc = CollabSession::open(doc_opts(7_100_001)).unwrap();
    let mut human_doc = CollabSession::open(doc_opts(7_100_002)).unwrap();

    // AI 生成报告骨架（Ai origin——AI 的撤销域）。
    let skeleton = vec![
        BlockSpec {
            kind: BlockKind::Heading,
            attrs: BTreeMap::from([("level".into(), "1".into())]),
            text: "设备采购方案（草案）".into(),
        },
        BlockSpec {
            kind: BlockKind::Paragraph,
            attrs: BTreeMap::new(),
            text: "本项目采购 A 设备 12.5 件，单价 336.8 万元。".into(),
        },
        BlockSpec {
            kind: BlockKind::Paragraph,
            attrs: BTreeMap::new(),
            text: "小计金额待核定。".into(),
        },
    ];
    ai_doc
        .execute(DActor::Ai, CollabCommand::LoadBlocks { blocks: skeleton })
        .unwrap();
    sync_doc(&ai_doc, &human_doc);
    assert_eq!(human_doc.document().len(), 3);
    pass("AI 生成三块骨架 → 人类端同步收敛");

    // 人类修订小计段（Human origin）。
    let ids = human_doc.top_level_ids();
    let subtotal_id = ids[2].clone();
    human_doc
        .execute(
            DActor::Human,
            CollabCommand::ReplaceText {
                block_id: subtotal_id.clone(),
                start: 0,
                end: 7,
                insert: "A 设备小计 4210.00 万元。".into(),
            },
        )
        .unwrap();
    // 并发：AI 端同时插入来源说明段。
    ai_doc
        .execute(
            DActor::Ai,
            CollabCommand::InsertBlock {
                parent: None,
                index: 3,
                spec: BlockSpec {
                    kind: BlockKind::Paragraph,
                    attrs: BTreeMap::new(),
                    text: "报价数据来源于《设备报价表》。".into(),
                },
                id: Some("doc-src-note".into()),
            },
        )
        .unwrap();
    // 双向同步 → 双端一致。
    sync_doc(&human_doc, &ai_doc);
    sync_doc(&ai_doc, &human_doc);
    let h_texts: Vec<String> = human_doc
        .document()
        .iter()
        .map(|b| b.text.clone())
        .collect();
    let a_texts: Vec<String> = ai_doc.document().iter().map(|b| b.text.clone()).collect();
    assert_eq!(h_texts, a_texts, "双端收敛");
    assert!(h_texts[2].contains("4210.00"));
    assert_eq!(h_texts.len(), 4);
    pass("人类修订 + AI 并发插入 → 双向同步双端一致");

    // 撤销域分离：人类撤自己的修订；AI 撤自己的插入。
    assert!(human_doc.undo(DActor::Human));
    sync_doc(&human_doc, &ai_doc);
    assert!(
        ai_doc
            .find_block(&subtotal_id)
            .unwrap()
            .text
            .contains("待核定"),
        "人类撤销传到 AI 端"
    );
    assert!(ai_doc.undo(DActor::Ai));
    sync_doc(&ai_doc, &human_doc);
    assert!(
        human_doc.find_block("doc-src-note").is_none(),
        "AI 撤销传到人类端"
    );
    // 重做恢复全量。
    assert!(human_doc.redo(DActor::Human));
    assert!(ai_doc.redo(DActor::Ai));
    sync_doc(&human_doc, &ai_doc);
    sync_doc(&ai_doc, &human_doc);
    assert!(human_doc.find_block("doc-src-note").is_some());
    assert!(human_doc
        .find_block(&subtotal_id)
        .unwrap()
        .text
        .contains("4210.00"));
    pass("撤销/重做按 origin 分域（各撤各的，跨端传播，redo 复原）");

    // ── 第 2 幕：画布协作（人类搭图 + AI 并发补充 + LWW）──────
    step(2, "画布协作——人类搭报价图，AI 并发补充，LWW 收敛");

    let mut human_canvas = CanvasSession::open(canvas_opts(7_200_002)).unwrap();
    let mut ai_canvas = CanvasSession::open(canvas_opts(7_200_001)).unwrap();

    // 人类搭图：报价节点 → 小计节点 + 推导边。
    human_canvas
        .execute(
            CActor::Human,
            CanvasCommand::AddNode {
                id: Some("n-quote".into()),
                x: 40.0,
                y: 80.0,
                w: 220.0,
                h: 70.0,
                props: BTreeMap::from([("label".into(), "A 设备 12.5 件 × 336.8 万元".into())]),
            },
        )
        .unwrap();
    human_canvas
        .execute(
            CActor::Human,
            CanvasCommand::AddNode {
                id: Some("n-total".into()),
                x: 340.0,
                y: 80.0,
                w: 220.0,
                h: 70.0,
                props: BTreeMap::from([("label".into(), "小计 4210.00 万元".into())]),
            },
        )
        .unwrap();
    human_canvas
        .execute(
            CActor::Human,
            CanvasCommand::AddEdge {
                id: Some("e-derive".into()),
                from: "n-quote".into(),
                to: "n-total".into(),
                props: BTreeMap::from([("label".into(), "数量×单价".into())]),
            },
        )
        .unwrap();
    sync_canvas(&human_canvas, &ai_canvas);
    assert_eq!(ai_canvas.objects().len(), 3);
    pass("人类搭三对象报价图 → AI 端同步收敛");

    // 并发：人类拖动小计节点；AI 补充审批节点 + 连线。
    human_canvas
        .execute(
            CActor::Human,
            CanvasCommand::MoveNode {
                id: "n-total".into(),
                x: 600.0,
                y: 40.0,
            },
        )
        .unwrap();
    ai_canvas
        .execute(
            CActor::Ai,
            CanvasCommand::AddNode {
                id: Some("n-approve".into()),
                x: 340.0,
                y: 240.0,
                w: 220.0,
                h: 70.0,
                props: BTreeMap::from([("label".into(), "人工审核（待办）".into())]),
            },
        )
        .unwrap();
    sync_canvas(&human_canvas, &ai_canvas);
    sync_canvas(&ai_canvas, &human_canvas);
    assert_eq!(human_canvas.objects().len(), 4, "AI 新增节点到达人类端");
    assert_eq!(ai_canvas.find_object("n-total").unwrap().x, 600.0);
    assert_eq!(
        human_canvas.objects(),
        ai_canvas.objects(),
        "画布双端读模型一致"
    );
    pass("人类拖动 + AI 并发补充 → 双向同步双端一致（同一命令接口）");

    // ── 第 3 幕：证据关联（画布受证 + 失效检测 + 重验恢复）─────
    step(3, "证据关联——画布对象受证，拖动失效，重验恢复");

    // 证据：来源 = 文档小计主张（MarkdownBlock 定位器）；受证画布对象
    // = 小计节点（CanvasObject 定位器）——「文档主张 ↔ 画布对象」
    // 的跨引擎证据链（引擎各自 Locator，应用层组合——§六架构约束）。
    let doc_fp = {
        let mut acc = String::new();
        for b in human_doc.document() {
            acc.push_str(&b.text);
            acc.push('\n');
        }
        format!("fnv64:{:016x}", fnv(acc.as_bytes()))
    };
    let doc_ref = DocumentRef::new(
        DocId::new("tender-doc-1"),
        mdc_evidence::EngineKind::Markdown,
        VersionStamp(1),
        mdc_evidence::ContentFingerprint(doc_fp),
    );
    let canvas_ref = DocumentRef::new(
        DocId::new("tender-canvas-1"),
        mdc_evidence::EngineKind::Canvas,
        VersionStamp(1),
        mdc_evidence::ContentFingerprint(human_canvas.canvas_fingerprint()),
    );
    let mut sidecar = Sidecar::new(doc_ref.clone());
    sidecar.register_source_doc(doc_ref.clone(), 1_000, "human:setup");
    sidecar.register_source_doc(canvas_ref.clone(), 1_100, "human:setup");
    let canvas_locator = Locator::CanvasObject {
        object_id: "n-total".into(),
        object_fingerprint: human_canvas.object_fingerprint("n-total").unwrap(),
    };
    let evidence = EvidenceRecord {
        evidence_id: "e-total".into(),
        claim: "文档与画布一致主张：A 设备小计 4210.00 万元".into(),
        claimed_value: Some(mdc_evidence::Decimal::parse("4210.00").unwrap()),
        claim_locator: Locator::MarkdownBlock {
            block_id: subtotal_id.clone(),
            text_fingerprint: format!(
                "fnv64:{:016x}",
                fnv(human_doc.find_block(&subtotal_id).unwrap().text.as_bytes())
            ),
        },
        claim_doc: doc_ref.clone(),
        source_locator: canvas_locator.clone(),
        source_doc: canvas_ref.clone(),
        excerpt: "画布小计节点：小计 4210.00 万元".into(),
        relation: RelationKind::Calculated,
        calc_ref: Some("calc-1".into()),
        linkage: LinkageStatus::Valid,
        calc: CalcStatus::NotApplicable,
        review: ReviewStatus::Pending,
        generation_batch: "demo-batch-1".into(),
        created_at_ms: 2_000,
    };
    evidence.validate().unwrap();
    sidecar.insert_evidence(evidence, 2_100, "ai:glm").unwrap();
    assert_eq!(sidecar.evidence[0].linkage, LinkageStatus::Valid);
    pass("证据登记：文档主张 ↔ 画布小计节点（跨引擎 Locator）→ Valid");

    // 人类拖动小计节点 → 画布语义指纹变 → 证据失效（只降不升）。
    human_canvas
        .execute(
            CActor::Human,
            CanvasCommand::MoveNode {
                id: "n-total".into(),
                x: 800.0,
                y: 300.0,
            },
        )
        .unwrap();
    let canvas_ref_v2 = DocumentRef::new(
        DocId::new("tender-canvas-1"),
        mdc_evidence::EngineKind::Canvas,
        VersionStamp(2),
        mdc_evidence::ContentFingerprint(human_canvas.canvas_fingerprint()),
    );
    sidecar.register_source_doc(canvas_ref_v2.clone(), 3_000, "system");
    assert_eq!(sidecar.evidence[0].linkage, LinkageStatus::Stale);
    pass("人类拖动小计节点 → 画布指纹失配 → 证据 Stale（失效检测）");

    // 显式重验（新指纹锚定）→ 恢复 Valid；审核面不动（三态正交）。
    sidecar
        .reverify_evidence(
            "e-total",
            canvas_ref_v2,
            Locator::CanvasObject {
                object_id: "n-total".into(),
                object_fingerprint: human_canvas.object_fingerprint("n-total").unwrap(),
            },
            3_500,
            "human:wang",
        )
        .unwrap();
    assert_eq!(sidecar.evidence[0].linkage, LinkageStatus::Valid);
    assert_eq!(sidecar.evidence[0].review, ReviewStatus::Pending);
    pass("显式重验 → Valid 恢复（审核面正交不动）");

    // ── 第 4 幕：删除与复原（双域身份语义）──────────────────────
    step(4, "删除与复原——文档块与画布节点的身份语义");

    // 文档域：删除块 → 撤销 → block_id 复原（载体身份换新族）。
    let anchor_before = human_doc.bind_anchor(&subtotal_id).unwrap();
    human_doc
        .execute(
            DActor::Human,
            CollabCommand::RemoveBlock {
                block_id: subtotal_id.clone(),
            },
        )
        .unwrap();
    assert!(human_doc.find_block(&subtotal_id).is_none());
    assert!(matches!(
        human_doc.resolve_anchor(anchor_before),
        Err(mdc_collab::CollabError::AnchorDangling { .. })
    ));
    assert!(human_doc.undo(DActor::Human));
    assert!(
        human_doc
            .find_block(&subtotal_id)
            .unwrap()
            .text
            .contains("4210.00"),
        "撤销删除 → 块复原（block_id 连续）"
    );
    let anchor_after = human_doc.bind_anchor(&subtotal_id).unwrap();
    assert_ne!(anchor_before, anchor_after, "复活 = 新载体身份（重绑恢复）");
    pass("文档块删除→撤销：block_id 连续 + 载体身份换新族（重绑通道）");

    // 画布域：删除节点 → 撤销 → 同 ID + 同指纹（Map 键身份稳定）。
    let fp_before = human_canvas.object_fingerprint("n-total").unwrap();
    human_canvas
        .execute(
            CActor::Human,
            CanvasCommand::RemoveObject {
                id: "n-total".into(),
            },
        )
        .unwrap();
    assert!(human_canvas.find_object("n-total").is_none());
    assert!(human_canvas.undo(CActor::Human));
    assert_eq!(
        human_canvas.object_fingerprint("n-total").unwrap(),
        fp_before,
        "画布删除→撤销：同 ID + 指纹复原"
    );
    pass("画布节点删除→撤销：同 ID 回归 + 指纹同一（证据锚连续）");

    // 跨端复原传播：画布撤销传到 AI 端。
    sync_canvas(&human_canvas, &ai_canvas);
    assert!(ai_canvas.find_object("n-total").is_some());
    pass("复原状态跨端传播（AI 端同步一致）");

    // ── 收尾：权威快照存档（故障恢复面）─────────────────────────
    step(5, "存档——权威快照与故障恢复");

    let doc_snap = human_doc.authoritative_snapshot().unwrap();
    let canvas_snap = human_canvas.authoritative_snapshot().unwrap();
    let doc_restored = CollabSession::restore(doc_opts(7_100_099), &doc_snap).unwrap();
    let canvas_restored = CanvasSession::restore(canvas_opts(7_200_099), &canvas_snap).unwrap();
    assert_eq!(doc_restored.document(), human_doc.document());
    assert_eq!(canvas_restored.objects(), human_canvas.objects());
    pass("双域权威快照 → 新会话恢复全量一致（撤销栈不随快照走）");

    println!("\n==============================================================");
    println!(" 演示全链通过：文档协作 / 画布协作 / 证据关联 / 撤销重做 /");
    println!(" 删除复原 / 跨端传播 / 快照恢复 —— 全部检查点 [PASS]");
    println!("==============================================================");
}

/// FNV-1a 64（与 mdc-evidence::Fnv64 同算法——演示内联避免公开面依赖）。
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
