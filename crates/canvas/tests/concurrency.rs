//! 并发与同步 + 故障恢复（快照/坏 wire/维护压缩）。

use std::collections::BTreeMap;

use mdc_canvas::{
    Actor, CanvasCommand, CanvasError, CanvasSession, CommandOutcome, SessionOptions,
};

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

fn sync(from: &CanvasSession, to: &CanvasSession) -> Result<(), CanvasError> {
    let sv = to.state_vector();
    let diff = from.encode_diff(&sv)?;
    to.apply_update(&diff)
}

#[test]
fn concurrent_human_and_ai_converge() {
    let mut alice = CanvasSession::open(opts(101)).unwrap();
    let mut bob = CanvasSession::open(opts(102)).unwrap();
    // Alice（人类）建两节点 → Bob 全量同步。
    for (id, x) in [("a", 0.0), ("b", 200.0)] {
        alice
            .execute(
                Actor::Human,
                CanvasCommand::AddNode {
                    id: Some(id.into()),
                    x,
                    y: 0.0,
                    w: 50.0,
                    h: 50.0,
                    props: BTreeMap::new(),
                },
            )
            .unwrap();
    }
    sync(&alice, &bob).unwrap();
    assert_eq!(bob.objects().len(), 2);

    // 并发：Alice 拖动 a；Bob（AI）新增节点 c。双向同步。
    alice
        .execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "a".into(),
                x: 42.0,
                y: 24.0,
            },
        )
        .unwrap();
    bob.execute(
        Actor::Ai,
        CanvasCommand::AddNode {
            id: Some("c".into()),
            x: 400.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    sync(&alice, &bob).unwrap();
    sync(&bob, &alice).unwrap();
    assert_eq!(alice.objects().len(), 3, "AI 新增节点到达人类端");
    assert_eq!(alice.find_object("a").unwrap().x, 42.0);
    assert_eq!(bob.find_object("a").unwrap().x, 42.0, "人类拖动到达 AI 端");
    // 双端读模型一致。
    assert_eq!(alice.objects(), bob.objects());
}

/// 并发同键写（LWW）：双端对同节点不同位置 → 收敛到同一位置
/// （yrs Map 键级 LWW；两端一致即收敛证明，不断言具体胜者）。
#[test]
fn concurrent_move_lww_converges() {
    let mut a = CanvasSession::open(opts(201)).unwrap();
    let mut b = CanvasSession::open(opts(202)).unwrap();
    a.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n".into()),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    sync(&a, &b).unwrap();
    a.execute(
        Actor::Human,
        CanvasCommand::MoveNode {
            id: "n".into(),
            x: 111.0,
            y: 0.0,
        },
    )
    .unwrap();
    b.execute(
        Actor::Ai,
        CanvasCommand::MoveNode {
            id: "n".into(),
            x: 222.0,
            y: 0.0,
        },
    )
    .unwrap();
    sync(&a, &b).unwrap();
    sync(&b, &a).unwrap();
    let pa = a.find_object("n").unwrap().x;
    let pb = b.find_object("n").unwrap().x;
    assert_eq!(pa, pb, "并发写后双端收敛同一（LWW）");
}

/// 权威快照恢复：全量状态 + 可继续编辑 + 可继续同步 + 撤销栈空。
#[test]
fn snapshot_restore_and_continue() {
    let mut orig = CanvasSession::open(opts(301)).unwrap();
    orig.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("a".into()),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    orig.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("b".into()),
            x: 100.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    orig.execute(
        Actor::Ai,
        CanvasCommand::AddEdge {
            id: Some("e".into()),
            from: "a".into(),
            to: "b".into(),
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    let snap = orig.authoritative_snapshot().unwrap();
    let mut restored = CanvasSession::restore(opts(401), &snap).unwrap();
    assert_eq!(restored.objects().len(), 3);
    assert_eq!(
        restored.find_object("e").unwrap().from.as_deref(),
        Some("a")
    );
    // 撤销栈不随快照走。
    assert_eq!(restored.undo_depth(Actor::Human), 0);
    assert_eq!(restored.undo_depth(Actor::Ai), 0);
    // 指纹同一（快照 = 全量语义）。
    assert_eq!(restored.canvas_fingerprint(), orig.canvas_fingerprint());
    // 恢复会话继续编辑 → 原会话经差量追平。
    restored
        .execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "a".into(),
                x: 77.0,
                y: 0.0,
            },
        )
        .unwrap();
    let sv = orig.state_vector();
    let diff = restored.encode_diff(&sv).unwrap();
    orig.apply_update(&diff).unwrap();
    assert_eq!(orig.find_object("a").unwrap().x, 77.0);
}

/// 坏 wire 输入 fail-closed + 会话零污染 + 维护压缩。
#[test]
fn hostile_wire_and_maintenance_gc() {
    let mut c = CanvasSession::open(opts(501)).unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n".into()),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    let before = c.objects();
    assert!(matches!(
        c.apply_update(&[0xFF, 0xFE, 0x00]),
        Err(CanvasError::Decode(_))
    ));
    assert_eq!(c.objects(), before, "坏 wire 零污染");
    // 维护压缩（清栈 + gc）。
    assert!(c.undo_depth(Actor::Human) >= 1);
    let (b, a) = c.maintenance_gc().unwrap();
    assert!(a <= b, "wire 字节不增（{b} → {a}）");
    assert_eq!(c.undo_depth(Actor::Human), 0);
    // 会话仍可用。
    c.execute(
        Actor::Human,
        CanvasCommand::MoveNode {
            id: "n".into(),
            x: 5.0,
            y: 5.0,
        },
    )
    .unwrap();
    assert_eq!(c.find_object("n").unwrap().x, 5.0);
}

/// p6-t6 发现修正：同 client 域既有 id（对端复活面）时 next_id 跳段
/// 分配——同槽 reload 计数器归零 + apply_update 复活旧段号对象后，
/// 新分配不撞号（对象不丢、id 域无重复）。
#[test]
fn next_id_skips_ids_revived_from_remote_state() {
    use mdc_canvas::{CanvasSession, SessionOptions};

    let mut a = CanvasSession::open(SessionOptions {
        client_id: 7_200_011,
        ..Default::default()
    })
    .unwrap();
    let mut b = CanvasSession::open(SessionOptions {
        client_id: 7_200_011, // 同槽 reload 语义：同一 client 域
        ..Default::default()
    })
    .unwrap();
    // 第一页面世：3 对象（id 7_200_011-1..3）→ 快照
    for i in 0..3 {
        a.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: None,
                x: 10.0 + i as f64,
                y: 20.0,
                w: 100.0,
                h: 40.0,
                props: BTreeMap::new(),
            },
        )
        .unwrap();
    }
    let snap = a.authoritative_snapshot().unwrap();
    // 同槽新页面世（计数器 0）：先复活旧状态
    b.apply_update(&snap).unwrap();
    assert_eq!(b.objects().len(), 3);
    // 新分配必须跳过复活占用的 -1..-3 → 首个新 id = 7_200_011-4
    let r = b
        .execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: None,
                x: 99.0,
                y: 99.0,
                w: 100.0,
                h: 40.0,
                props: BTreeMap::new(),
            },
        )
        .unwrap();
    let new_id = match r.outcome {
        CommandOutcome::NodeAdded { id } => id,
        _ => panic!("expected NodeAdded"),
    };
    assert_eq!(new_id, "7200011-4", "revived ids must be skipped");
    // 对象零丢失：3 复活 + 1 新增 = 4，且 id 域无重复
    let ids: Vec<String> = b.objects().iter().map(|o| o.id.clone()).collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        4
    );
}
