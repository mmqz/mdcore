//! 撤销/重做 + 删除复原身份（画布域特性：Map 键身份稳定）。

use std::collections::BTreeMap;

use mdc_canvas::{Actor, CanvasCommand, CanvasSession, ObjectKind, SessionOptions};

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

#[test]
fn undo_redo_all_op_types() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    // 加节点 → 撤 → 无 → redo → 复原（同 ID）。
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-1".into()),
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 50.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    assert!(c.undo(Actor::Human));
    assert!(c.find_object("n-1").is_none());
    assert!(c.redo(Actor::Human));
    assert_eq!(c.find_object("n-1").unwrap().x, 10.0);
    // 移动撤销。
    c.execute(
        Actor::Human,
        CanvasCommand::MoveNode {
            id: "n-1".into(),
            x: 500.0,
            y: 500.0,
        },
    )
    .unwrap();
    assert_eq!(c.find_object("n-1").unwrap().x, 500.0);
    assert!(c.undo(Actor::Human));
    assert_eq!(c.find_object("n-1").unwrap().x, 10.0, "撤销移动回原位");
    // 缩放撤销。
    c.execute(
        Actor::Human,
        CanvasCommand::ResizeNode {
            id: "n-1".into(),
            w: 999.0,
            h: 999.0,
        },
    )
    .unwrap();
    assert!(c.undo(Actor::Human));
    assert_eq!(c.find_object("n-1").unwrap().w, 50.0);
    // 属性撤销。
    let mut p = BTreeMap::new();
    p.insert("label".to_string(), "新标签".to_string());
    c.execute(
        Actor::Ai,
        CanvasCommand::SetProps {
            id: "n-1".into(),
            props: p,
        },
    )
    .unwrap();
    assert!(c.undo(Actor::Ai));
    assert!(c.find_object("n-1").unwrap().props.is_empty());
    // 边撤销（端点节点随之恢复的复合场景在删除复原测试覆盖）。
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-2".into()),
            x: 200.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddEdge {
            id: Some("e-1".into()),
            from: "n-1".into(),
            to: "n-2".into(),
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    assert!(c.undo(Actor::Human));
    assert!(c.find_object("e-1").is_none());
    assert!(c.redo(Actor::Human));
    assert!(c.find_object("e-1").is_some());
    // 分组/解组撤销。
    c.execute(
        Actor::Human,
        CanvasCommand::GroupObjects {
            group_id: Some("g-1".into()),
            members: vec!["n-1".into(), "n-2".into()],
        },
    )
    .unwrap();
    assert!(c.undo(Actor::Human));
    assert!(c.find_object("g-1").is_none());
    assert!(c.redo(Actor::Human));
    c.execute(
        Actor::Human,
        CanvasCommand::Ungroup {
            group_id: "g-1".into(),
        },
    )
    .unwrap();
    assert!(c.undo(Actor::Human), "撤销解组 → 组回归");
    assert!(c.find_object("g-1").is_some());
}

/// **画布域身份特性**：删除 → 撤销 → 同 ID 回归（Map 键身份稳定，
/// 对比文档域 XML 元素复活 = 新 ID 族——测试钉定该域差异）。
#[test]
fn delete_then_undo_restores_same_identity() {
    let mut c = CanvasSession::open(opts(2)).unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-quote".into()),
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 40.0,
            props: [("label".to_string(), "报价节点".to_string())]
                .into_iter()
                .collect(),
        },
    )
    .unwrap();
    let fp_before = c.object_fingerprint("n-quote").unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::RemoveObject {
            id: "n-quote".into(),
        },
    )
    .unwrap();
    assert!(c.find_object("n-quote").is_none());
    // 撤销删除 → 同 ID 回归 + 指纹复原（证据锚的删除复原语义连续性）。
    assert!(c.undo(Actor::Human));
    let o = c.find_object("n-quote").unwrap();
    assert_eq!(o.kind, ObjectKind::Node);
    assert_eq!(o.props.get("label").map(|s| s.as_str()), Some("报价节点"));
    assert_eq!(
        c.object_fingerprint("n-quote").unwrap(),
        fp_before,
        "删除撤销复原后指纹同一（同 ID + 同语义 = 同指纹）"
    );
}

/// origin 分栈：Human/Ai 交错互不干扰（窗 0 = 每事务一栈项）。
#[test]
fn per_actor_interleaved_undo() {
    let mut c = CanvasSession::open(opts(3)).unwrap();
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
    // 交错全坐标移动（MoveNode 写 x+y 双键——LWW 逐键；Ai 恒后手）。
    for i in 0..3 {
        c.execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "n".into(),
                x: (i + 1) as f64,
                y: 100.0,
            },
        )
        .unwrap();
        c.execute(
            Actor::Ai,
            CanvasCommand::MoveNode {
                id: "n".into(),
                x: 200.0,
                y: (i + 1) as f64,
            },
        )
        .unwrap();
    }
    // 终态 = Ai 末笔 LWW 胜（200, 3）。
    let o = c.find_object("n").unwrap();
    assert_eq!((o.x, o.y), (200.0, 3.0));
    // Human 栈 = 建节点 + 3 移动；Ai 栈 = 3 移动。
    assert_eq!(c.undo_depth(Actor::Human), 4);
    assert_eq!(c.undo_depth(Actor::Ai), 3);
    // 撤 Ai 3 笔——**yrs 0.28 map-undo 交错语义（探针钉定）**：同会话
    // 内同键交错写（Human/Ai 轮流 MoveNode）的撤销 = 逐笔回退——
    // 撤第 k 笔 Ai 操作恢复到该操作历史点的值（3→2→1），而非"全部
    // Ai 写移除后的 Human 末笔"。跨会话编辑的撤销不受此影响（见
    // evidence_assoc 测试：AI 独立会话的编辑撤销干净复原）。已登记
    // 为已知语义（上游询问候选），此处钉定观测行为防静默漂移。
    for (k, want_x) in [(1usize, 3.0f64), (2, 2.0), (3, 1.0)] {
        assert!(c.undo(Actor::Ai), "第 {k} 笔 Ai 撤销");
        let o = c.find_object("n").unwrap();
        assert_eq!(
            (o.x, o.y),
            (want_x, 100.0),
            "逐笔回退语义：第 {k} 笔撤后恢复该历史点值"
        );
    }
    // Ai 栈空；Human 栈不受影响（分栈成立）。
    assert_eq!(c.undo_depth(Actor::Ai), 0);
    assert_eq!(c.undo_depth(Actor::Human), 4);
    // Human 面（同键交错 + 对栈先撤后的 yrs pop-until-productive 语义，
    // 探针钉定）：undo() 弹到有产出为止——H3/H2 的插入项内容已被 Ai
    // 撤销的 redo 链复刻（原项保持墓碑）= 无产出项被丢弃，H1 产出
    // → 一次 undo 回初始位（depth 4→1）；再一次 undo 撤建块插入
    // （节点消失）；栈尽 false。跨会话编辑撤销不受此影响。
    assert!(c.undo(Actor::Human));
    let o = c.find_object("n").unwrap();
    assert_eq!((o.x, o.y), (0.0, 0.0), "一次 undo 消耗无产出项 + H1");
    assert_eq!(c.undo_depth(Actor::Human), 1);
    assert!(c.undo(Actor::Human), "撤建块插入");
    assert!(c.find_object("n").is_none());
    assert!(!c.undo(Actor::Human), "栈尽");
}

/// 撤销栈软上限（粗粒度 clear_all 显式回执——与 mdc-collab 同构）。
#[test]
fn undo_stack_soft_limit() {
    let mut c = CanvasSession::open(SessionOptions {
        client_id: 4,
        undo_stack_limit: Some(2),
        ..Default::default()
    })
    .unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n".into()),
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    // 两笔恰在限内（depth ≤ 2）。
    let r1 = c
        .execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "n".into(),
                x: 1.0,
                y: 0.0,
            },
        )
        .unwrap();
    assert!(!r1.undo_cleared);
    assert_eq!(r1.undo_depth, 2);
    // 第三笔 → 超限 → clear_all 显式回执。
    let r2 = c
        .execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "n".into(),
                x: 2.0,
                y: 0.0,
            },
        )
        .unwrap();
    assert!(r2.undo_cleared);
    assert_eq!(r2.undo_depth, 0);
    assert_eq!(c.find_object("n").unwrap().x, 2.0, "数据面不受清理影响");
}
