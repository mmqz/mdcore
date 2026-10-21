//! 证据系统对画布对象的关联能力（用户指令④的画布腿）：
//! 画布作为受证来源（EngineKind::Canvas + Locator::CanvasObject）——
//! 画布改写 → 指纹失配 → 证据只降不升（Stale）→ 显式重验恢复。

use std::collections::BTreeMap;

use mdc_canvas::{Actor, CanvasCommand, CanvasSession, SessionOptions};
use mdc_evidence::{
    DocId, DocumentRef, EvidenceRecord, LinkageStatus, Locator, RelationKind, ReviewStatus,
    Sidecar, VersionStamp,
};

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

fn canvas_ref(c: &CanvasSession, version: u64) -> DocumentRef {
    DocumentRef::new(
        DocId::new("canvas-1"),
        mdc_evidence::EngineKind::Canvas,
        VersionStamp(version),
        mdc_evidence::ContentFingerprint(c.canvas_fingerprint()),
    )
}

#[test]
fn evidence_binds_to_canvas_object_and_degrades_on_move() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    // 画布上的人类操作：报价节点（AI 辅助标注的语义标签）。
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-quote".into()),
            x: 10.0,
            y: 10.0,
            w: 160.0,
            h: 60.0,
            props: [(
                "label".to_string(),
                "A 设备报价 12.5 件 × 336.8 万元".to_string(),
            )]
            .into_iter()
            .collect(),
        },
    )
    .unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-total".into()),
            x: 220.0,
            y: 10.0,
            w: 160.0,
            h: 60.0,
            props: [("label".to_string(), "小计 4210.00 万元".to_string())]
                .into_iter()
                .collect(),
        },
    )
    .unwrap();
    c.execute(
        Actor::Ai,
        CanvasCommand::AddEdge {
            id: Some("e-derive".into()),
            from: "n-quote".into(),
            to: "n-total".into(),
            props: [("label".to_string(), "数量×单价".to_string())]
                .into_iter()
                .collect(),
        },
    )
    .unwrap();

    // 证据登记：画布 = 来源文档；主张锚 = 画布对象（小计节点）。
    let mut sidecar = Sidecar::new(canvas_ref(&c, 1));
    sidecar.register_source_doc(canvas_ref(&c, 1), 1_000, "human:setup");
    let locator_total = Locator::CanvasObject {
        object_id: "n-total".into(),
        object_fingerprint: c.object_fingerprint("n-total").unwrap(),
    };
    let evidence = EvidenceRecord {
        evidence_id: "e-canvas-total".into(),
        claim: "画布小计节点主张 4210.00 万元".into(),
        claimed_value: Some(mdc_evidence::Decimal::parse("4210.00").unwrap()),
        claim_locator: locator_total.clone(),
        claim_doc: canvas_ref(&c, 1),
        source_locator: locator_total.clone(),
        source_doc: canvas_ref(&c, 1),
        excerpt: "n-total: 小计 4210.00 万元".into(),
        relation: RelationKind::Calculated,
        calc_ref: Some("calc-1".into()),
        linkage: LinkageStatus::Valid,
        calc: mdc_evidence::CalcStatus::NotApplicable,
        review: ReviewStatus::Pending,
        generation_batch: "batch-1".into(),
        created_at_ms: 2_000,
    };
    evidence.validate().unwrap();
    sidecar.insert_evidence(evidence, 3_000, "ai:glm").unwrap();
    assert_eq!(sidecar.evidence[0].linkage, LinkageStatus::Valid);

    // —— 画布改写：人类拖动小计节点（位置 = 语义字段）——
    c.execute(
        Actor::Human,
        CanvasCommand::MoveNode {
            id: "n-total".into(),
            x: 500.0,
            y: 300.0,
        },
    )
    .unwrap();
    // 重登记新画布引用（版本自增）→ 失效检测只降不升。
    sidecar.register_source_doc(canvas_ref(&c, 2), 4_000, "system");
    assert_eq!(
        sidecar.evidence[0].linkage,
        LinkageStatus::Stale,
        "画布改写（含位置漂移）→ 证据失效"
    );
    // 对象级指纹同时失配（定位器证据锚）。
    assert_ne!(
        sidecar.evidence[0].source_locator,
        Locator::CanvasObject {
            object_id: "n-total".into(),
            object_fingerprint: c.object_fingerprint("n-total").unwrap(),
        }
    );

    // —— 显式重验：更新定位器指纹 + 新画布引用 → 恢复 Valid ——
    sidecar
        .reverify_evidence(
            "e-canvas-total",
            canvas_ref(&c, 2),
            Locator::CanvasObject {
                object_id: "n-total".into(),
                object_fingerprint: c.object_fingerprint("n-total").unwrap(),
            },
            5_000,
            "human:wang",
        )
        .unwrap();
    assert_eq!(sidecar.evidence[0].linkage, LinkageStatus::Valid);
    // 审计链完整。
    let kinds: Vec<&str> = sidecar.audit.iter().map(|e| e.kind.as_str()).collect();
    assert!(kinds.contains(&"source_doc_register"));
    assert!(kinds.contains(&"linkage_degraded"));
    assert!(kinds.contains(&"linkage_reverified"));

    // —— 协同面：AI 端并发改写同样触发失效（同步链）——
    let mut ai_side = CanvasSession::open(opts(2)).unwrap();
    let sv = ai_side.state_vector();
    let diff = c.encode_diff(&sv).unwrap();
    ai_side.apply_update(&diff).unwrap();
    assert_eq!(ai_side.objects(), c.objects(), "AI 端收敛");
    ai_side
        .execute(
            Actor::Ai,
            CanvasCommand::SetProps {
                id: "n-total".into(),
                props: BTreeMap::from([(
                    "label".to_string(),
                    "小计 4999.00 万元（AI 修订）".to_string(),
                )]),
            },
        )
        .unwrap();
    // AI 端编辑回传 → 人类端画布指纹变 → 再登记 → 证据再失效。
    let sv = c.state_vector();
    let diff = ai_side.encode_diff(&sv).unwrap();
    c.apply_update(&diff).unwrap();
    sidecar.register_source_doc(canvas_ref(&c, 3), 6_000, "system");
    assert_eq!(sidecar.evidence[0].linkage, LinkageStatus::Stale);
    // 撤销 AI 端编辑（AI 自己的栈）→ 语义回归 → 重验后指纹可复原。
    assert!(ai_side.undo(Actor::Ai));
    let sv = c.state_vector();
    let diff = ai_side.encode_diff(&sv).unwrap();
    c.apply_update(&diff).unwrap();
    sidecar
        .reverify_evidence(
            "e-canvas-total",
            canvas_ref(&c, 4),
            Locator::CanvasObject {
                object_id: "n-total".into(),
                object_fingerprint: c.object_fingerprint("n-total").unwrap(),
            },
            7_000,
            "human:wang",
        )
        .unwrap();
    assert_eq!(sidecar.evidence[0].linkage, LinkageStatus::Valid);
    assert_eq!(
        sidecar.evidence[0].review,
        ReviewStatus::Pending,
        "三态正交：重验不复位审核面"
    );
}
