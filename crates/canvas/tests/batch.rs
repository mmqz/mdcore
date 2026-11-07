//! 批量命令原子化 + 撤销单元化（#352 / #44 步6）测试。
//!
//! 覆盖：原子性（首/中/尾失败三位置——真实 doc 零接触）/依赖序列
//! （cmd N 可见 cmd N-1 效果）/单撤销单元（一次 undo 全撤、redo 全恢
//! 复）/空批拒收/事件面（恰一个 Command 事件 + touched 去重）/双会话
//! 收敛（批差量可转发）/批内自动 ID/聚跟入向限额门/既有单命令路径
//! 零变化/撤销栈软上限按批计一。

use std::collections::BTreeMap;
use std::sync::Arc;

use mdc_canvas::{
    Actor, BatchReceipt, CanvasCommand, CanvasError, CanvasSession, CommandOutcome, SessionOptions,
};

fn session(tag: u64) -> CanvasSession {
    CanvasSession::open(SessionOptions {
        client_id: tag,
        clock_ms: Some(Arc::new(move || 1_700_000_000_000 + tag)),
        ..Default::default()
    })
    .unwrap()
}

fn add_node_cmd(id: &str, x: f64, y: f64) -> CanvasCommand {
    CanvasCommand::AddNode {
        id: Some(id.into()),
        x,
        y,
        w: 100.0,
        h: 50.0,
        props: BTreeMap::new(),
    }
}

fn add_edge_cmd(id: &str, from: &str, to: &str) -> CanvasCommand {
    CanvasCommand::AddEdge {
        id: Some(id.into()),
        from: from.into(),
        to: to.into(),
        props: BTreeMap::new(),
    }
}

fn move_cmd(id: &str, x: f64, y: f64) -> CanvasCommand {
    CanvasCommand::MoveNode {
        id: id.into(),
        x,
        y,
    }
}

// ── 原子性 ───────────────────────────────────────────────────────

#[test]
fn batch_failure_leaves_real_doc_untouched() {
    // 失败位置矩阵：首（第 1 条）/ 中（第 3 条）/ 尾（最后一条）。
    for fail_pos in [0usize, 2, 3] {
        let mut s = session(1);
        s.execute(Actor::Human, add_node_cmd("a", 0.0, 0.0))
            .unwrap();
        let fp_before = s.canvas_fingerprint();
        let undo_before = s.undo_depth(Actor::Human);
        let mut cmds = vec![
            add_node_cmd("n1", 10.0, 10.0),
            add_node_cmd("n2", 20.0, 20.0),
            add_edge_cmd("e1", "a", "n1"),
            CanvasCommand::SetProps {
                id: "n1".into(),
                props: BTreeMap::new(),
            },
        ];
        // 在 fail_pos 注入一条必失败命令（引用不存在的端点）。
        cmds.insert(fail_pos, add_edge_cmd("bad", "a", "ghost-endpoint"));
        let e = s.execute_batch(Actor::Human, cmds).unwrap_err();
        assert!(
            matches!(e, CanvasError::EndpointNotFound { .. }),
            "pos {fail_pos}: {e:?}"
        );
        // 真实 doc 零接触：对象集、指纹、撤销深度全部不变。
        assert_eq!(s.objects().len(), 1, "pos {fail_pos}：无半成品状态");
        assert_eq!(
            s.canvas_fingerprint(),
            fp_before,
            "pos {fail_pos}：指纹不变"
        );
        assert_eq!(
            s.undo_depth(Actor::Human),
            undo_before,
            "pos {fail_pos}：失败不入撤销栈"
        );
        // 失败后真实会话可继续正常工作。
        s.execute(Actor::Human, add_node_cmd("after", 1.0, 1.0))
            .unwrap();
        assert_eq!(s.objects().len(), 2);
    }
}

#[test]
fn batch_failure_dispatches_no_events() {
    use mdc_canvas::CanvasChangeSource;
    let mut s = session(1);
    s.execute(Actor::Human, add_node_cmd("a", 0.0, 0.0))
        .unwrap();
    let seen = Arc::new(std::sync::Mutex::new(0usize));
    let sink = seen.clone();
    let _sub = s.subscribe(move |ev| {
        if matches!(ev.source, CanvasChangeSource::Command { .. }) {
            *sink.lock().unwrap() += 1;
        }
    });
    let e = s
        .execute_batch(
            Actor::Human,
            vec![
                add_node_cmd("n1", 0.0, 0.0),
                add_edge_cmd("bad", "n1", "ghost"),
            ],
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::EndpointNotFound { .. }));
    assert_eq!(*seen.lock().unwrap(), 0, "失败路径零事件");
}

#[test]
fn empty_batch_is_rejected() {
    let mut s = session(1);
    let e = s.execute_batch(Actor::Human, vec![]).unwrap_err();
    assert!(
        matches!(e, CanvasError::InvalidInput(ref m) if m.contains("空批")),
        "{e:?}"
    );
}

// ── 成功面：单撤销单元 + 依赖序列 ────────────────────────────────

#[test]
fn successful_batch_is_one_undo_unit_with_dependencies() {
    let mut s = session(1);
    s.execute(Actor::Human, add_node_cmd("a", 0.0, 0.0))
        .unwrap();
    let receipt: BatchReceipt = s
        .execute_batch(
            Actor::Human,
            vec![
                add_node_cmd("n1", 10.0, 10.0),
                // cmd 2 依赖 cmd 1 的效果（同批内可见）。
                move_cmd("n1", 30.0, 30.0),
                // cmd 3 依赖 cmd 1 的存在。
                add_edge_cmd("e1", "a", "n1"),
                // cmd 4 依赖 cmd 1/2。
                CanvasCommand::GroupObjects {
                    group_id: Some("g1".into()),
                    members: vec!["a".into(), "n1".into()],
                },
                CanvasCommand::SetProps {
                    id: "n1".into(),
                    props: [("label".to_string(), "批内".to_string())].into(),
                },
            ],
        )
        .unwrap();
    assert_eq!(receipt.outcomes.len(), 5);
    assert!(matches!(
        receipt.outcomes[1],
        CommandOutcome::NodeMoved { ref id } if id == "n1"
    ));
    // 整批 = 一个撤销单元。
    assert_eq!(s.undo_depth(Actor::Human), 2, "基线 1 + 批 1");
    assert_eq!(receipt.undo_depth, 2);
    // 批效果全部落地（move 生效 = 依赖序列可见性）。
    let n1 = s.find_object("n1").unwrap();
    assert_eq!((n1.x, n1.y), (30.0, 30.0));
    assert_eq!(n1.props.get("label"), Some(&"批内".to_string()));
    assert!(s.find_object("e1").is_some());
    assert!(s.find_object("g1").is_some());
    // 一次 undo → 全撤（回到基线 1 对象 + move/props/边/组全消失）。
    assert!(s.undo(Actor::Human));
    assert_eq!(s.objects().len(), 1, "批效果全撤，仅剩基线对象 a");
    assert_eq!(s.find_object("a").unwrap().x, 0.0, "a 未被批触碰");
    // 一次 redo → 全恢复。
    assert!(s.redo(Actor::Human));
    assert_eq!(s.objects().len(), 4, "全恢复：a/n1/e1/g1");
    assert_eq!(s.find_object("n1").unwrap().x, 30.0, "move 效果恢复");
    assert_eq!(
        s.find_object("n1").unwrap().props.get("label"),
        Some(&"批内".to_string())
    );
}

#[test]
fn batch_auto_ids_are_returned_and_usable() {
    let mut s = session(7);
    let receipt = s
        .execute_batch(
            Actor::Ai,
            vec![
                CanvasCommand::AddNode {
                    id: None,
                    x: 1.0,
                    y: 2.0,
                    w: 3.0,
                    h: 4.0,
                    props: BTreeMap::new(),
                },
                CanvasCommand::AddNode {
                    id: None,
                    x: 5.0,
                    y: 6.0,
                    w: 7.0,
                    h: 8.0,
                    props: BTreeMap::new(),
                },
            ],
        )
        .unwrap();
    let ids: Vec<String> = receipt
        .outcomes
        .iter()
        .filter_map(|o| match o {
            CommandOutcome::NodeAdded { id } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2, "回执含批内自动分配 ID");
    assert_ne!(ids[0], ids[1]);
    for id in &ids {
        assert!(s.find_object(id).is_some(), "自动 ID {id} 真实存在");
    }
    // 批后单命令自动分配照常（不同命名空间，不撞号）。
    let single = s
        .execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: None,
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                props: BTreeMap::new(),
            },
        )
        .unwrap();
    match single.outcome {
        CommandOutcome::NodeAdded { id } => {
            assert!(!ids.contains(&id), "单命令 ID 与批 ID 不撞");
            assert!(s.find_object(&id).is_some());
        }
        other => panic!("应为 NodeAdded，实际 {other:?}"),
    }
    assert_eq!(s.objects().len(), 3);
}

// ── 事件面 ───────────────────────────────────────────────────────

#[test]
fn successful_batch_dispatches_single_command_event() {
    use mdc_canvas::CanvasChangeSource;
    let mut s = session(1);
    s.execute(Actor::Human, add_node_cmd("a", 0.0, 0.0))
        .unwrap();
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = seen.clone();
    let _sub = s.subscribe(move |ev| {
        if let CanvasChangeSource::Command { .. } = ev.source {
            sink.lock().unwrap().push(ev.touched.clone());
        }
    });
    s.execute_batch(
        Actor::Human,
        vec![
            add_node_cmd("n1", 0.0, 0.0),
            move_cmd("n1", 5.0, 5.0),
            add_node_cmd("n2", 1.0, 1.0),
            CanvasCommand::SetProps {
                id: "n1".into(),
                props: BTreeMap::new(),
            },
        ],
    )
    .unwrap();
    let got = seen.lock().unwrap();
    assert_eq!(got.len(), 1, "整批恰一个 Command 事件");
    assert_eq!(
        got[0],
        vec!["n1".to_string(), "n2".to_string()],
        "touched = 批内受触对象去重（字典序）"
    );
}

// ── 协同面 ───────────────────────────────────────────────────────

#[test]
fn batch_updates_forward_via_events_converge() {
    use mdc_canvas::CanvasChangeSource;
    let mut a = session(1);
    let mut b = session(2);
    a.execute(Actor::Human, add_node_cmd("a", 0.0, 0.0))
        .unwrap();
    // 对端先同步基线。
    let sv_b = b.state_vector();
    b.apply_update(&a.encode_diff(&sv_b).unwrap()).unwrap();
    // 订阅 a 的事件拿批差量。
    let inbox = Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = inbox.clone();
    let _sub = a.subscribe(move |ev| {
        if let CanvasChangeSource::Command { .. } = ev.source {
            for u in &ev.updates {
                sink.lock().unwrap().push(u.clone());
            }
        }
    });
    a.execute_batch(
        Actor::Human,
        vec![
            add_node_cmd("n1", 10.0, 10.0),
            add_node_cmd("n2", 20.0, 20.0),
            add_edge_cmd("e1", "n1", "n2"),
        ],
    )
    .unwrap();
    let updates = inbox.lock().unwrap();
    assert_eq!(updates.len(), 1, "单事务单 update 载荷");
    for u in updates.iter() {
        b.apply_update(u).unwrap();
    }
    let (oa, ob) = (a.objects(), b.objects());
    assert_eq!(oa, ob, "批差量转发后双端收敛（含边端点）");
    assert_eq!(oa.len(), 4);
}

// ── 限额与栈深 ───────────────────────────────────────────────────

#[test]
fn batch_effect_bytes_count_against_inbound_limit() {
    // 小限额会话：单条命令逐条能过、批效果聚跟超限 → 整批拒。
    let mut s = CanvasSession::open(SessionOptions {
        client_id: 9,
        input_byte_limit: 900,
        clock_ms: Some(Arc::new(|| 42)),
        ..Default::default()
    })
    .unwrap();
    let fp_before = s.canvas_fingerprint();
    // 8 × add_node（props 带 ~80B 标签）：单条 inbound ≈ 200B < 900 ✓。
    let cmds: Vec<CanvasCommand> = (0..8)
        .map(|i| CanvasCommand::AddNode {
            id: Some(format!("n{i}")),
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            props: [("label".to_string(), format!("{i}{}", "x".repeat(80)))].into(),
        })
        .collect();
    let e = s.execute_batch(Actor::Human, cmds).unwrap_err();
    assert!(matches!(e, CanvasError::Limit { .. }), "{e:?}");
    assert_eq!(s.objects().len(), 0, "超限批零落地");
    assert_eq!(s.canvas_fingerprint(), fp_before);
    // 对照：单条同尺寸命令可过（逐条门未被批级门误伤）。
    s.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("solo".into()),
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            props: [("label".to_string(), "x".repeat(80))].into(),
        },
    )
    .unwrap();
    assert_eq!(s.objects().len(), 1);
}

#[test]
fn undo_stack_limit_counts_batch_as_one() {
    let mut s = CanvasSession::open(SessionOptions {
        client_id: 3,
        undo_stack_limit: Some(2),
        clock_ms: Some(Arc::new(|| 42)),
        ..Default::default()
    })
    .unwrap();
    for round in 0..3 {
        let receipt = s
            .execute_batch(
                Actor::Human,
                vec![
                    add_node_cmd(&format!("a{round}"), 0.0, 0.0),
                    add_node_cmd(&format!("b{round}"), 1.0, 1.0),
                ],
            )
            .unwrap();
        if round < 2 {
            assert_eq!(receipt.undo_depth, round + 1, "每批计一单元");
            assert!(!receipt.undo_cleared);
        } else {
            // 第 3 批超软上限（3 > 2）→ 清栈标记。
            assert!(receipt.undo_cleared, "第 3 批触发粗粒度清栈");
        }
    }
}

// ── 既有单命令路径零变化 ─────────────────────────────────────────

#[test]
fn single_command_path_still_one_unit_per_command() {
    let mut s = session(1);
    s.execute(Actor::Human, add_node_cmd("a", 0.0, 0.0))
        .unwrap();
    s.execute(Actor::Human, add_node_cmd("b", 1.0, 1.0))
        .unwrap();
    s.execute(Actor::Human, move_cmd("a", 9.0, 9.0)).unwrap();
    assert_eq!(
        s.undo_depth(Actor::Human),
        3,
        "单命令 = 每命令一单元（既有语义）"
    );
    assert!(s.undo(Actor::Human));
    assert_eq!(s.find_object("a").unwrap().x, 0.0, "仅撤最后一条");
    assert!(s.find_object("b").is_some());
}
