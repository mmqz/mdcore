//! z 序会话面测试（#402）：渲染序 / 重排四向 / 新建置顶 / 并发合并
//! 收敛 / 批量插入不退化 / 撤销重做 / 指纹失效 / 读模型稳定面。
//!
//! 断言对着可观测事实：`(z_index, id)` 全序、渲染 shapes 序、回执、
//! 撤销栈深度、指纹值——不写「跑通就算过」。

use std::collections::BTreeMap;

use mdc_canvas::render::CanvasRendering;
use mdc_canvas::{
    Actor, CanvasCommand, CanvasError, CanvasObject, CanvasSession, CommandOutcome, ObjectKind,
    ReorderOp, SessionOptions, Shape, StrokeTool,
};

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

fn session() -> CanvasSession {
    CanvasSession::open(opts(7_300_402)).unwrap()
}

/// 加一个带 fill 的矩形节点（fill 用于在渲染序中互区分）。
fn add_rect(s: &mut CanvasSession, id: &str, fill: &str) {
    let mut props = BTreeMap::new();
    props.insert("fill".to_string(), fill.to_string());
    s.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some(id.to_string()),
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
            props,
        },
    )
    .unwrap();
}

fn reorder(s: &mut CanvasSession, id: &str, op: ReorderOp) {
    s.execute(
        Actor::Human,
        CanvasCommand::Reorder {
            id: id.to_string(),
            op,
        },
    )
    .unwrap();
}

/// 渲染序中的 fill 颜色序列（矩形 fill 唯一 → 对象序的可观测面）。
fn render_fills(s: &CanvasSession) -> Vec<String> {
    CanvasRendering::from_session(s)
        .shapes
        .iter()
        .map(|(shape, _)| match shape {
            Shape::Rect { style, .. } => style
                .fill
                .map(|c| c.to_hex())
                .unwrap_or_else(|| "none".to_string()),
            _ => "other".to_string(),
        })
        .collect()
}

fn z_of(s: &CanvasSession, id: &str) -> String {
    s.find_object(id).unwrap().z_index
}

/// 双向同步（并发测试同款通道）。
fn sync(from: &CanvasSession, to: &CanvasSession) {
    let sv = to.state_vector();
    let diff = from.encode_diff(&sv).unwrap();
    to.apply_update(&diff).unwrap();
}

fn sync_both(a: &CanvasSession, b: &CanvasSession) {
    sync(a, b);
    sync(b, a);
}

fn z_order_ids(s: &CanvasSession) -> Vec<String> {
    s.objects_z_ordered().into_iter().map(|o| o.id).collect()
}

// ── 验收 1：重叠元素 bringToFront 后渲染顺序变化 ─────────────────

#[test]
fn bring_to_front_changes_render_order() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    // 新建置顶：b 后建 → b 在上（shapes 末位 = 顶层）
    assert_eq!(render_fills(&s), vec!["#ff0000", "#0000ff"]);

    // a 置顶：渲染序翻转（shapes[0] = 底层）
    reorder(&mut s, "a", ReorderOp::BringToFront);
    assert_eq!(render_fills(&s), vec!["#0000ff", "#ff0000"]);
    assert!(z_of(&s, "a").as_str() > z_of(&s, "b").as_str());
}

#[test]
fn send_to_back_changes_render_order() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    // b 置底
    reorder(&mut s, "b", ReorderOp::SendToBack);
    assert_eq!(render_fills(&s), vec!["#0000ff", "#ff0000"]);
    assert!(z_of(&s, "b").as_str() < z_of(&s, "a").as_str());
}

#[test]
fn relative_steps_move_one_layer() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    add_rect(&mut s, "c", "#00ff00");
    assert_eq!(z_order_ids(&s), vec!["a", "b", "c"]);

    // c 下移一层：c ↔ b 交换
    reorder(&mut s, "c", ReorderOp::SendBackward);
    assert_eq!(z_order_ids(&s), vec!["a", "c", "b"]);

    // a 上移一层：a ↔ c 交换
    reorder(&mut s, "a", ReorderOp::BringForward);
    assert_eq!(z_order_ids(&s), vec!["c", "a", "b"]);
}

#[test]
fn relative_steps_idempotent_at_ends() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    let z_top = z_of(&s, "b");
    let z_bottom = z_of(&s, "a");

    // 已在顶层再上移 / 已在底层再下移：幂等（z 不变、无事件副作用）
    reorder(&mut s, "b", ReorderOp::BringForward);
    reorder(&mut s, "a", ReorderOp::SendBackward);
    assert_eq!(z_of(&s, "b"), z_top, "顶层上移应无变化");
    assert_eq!(z_of(&s, "a"), z_bottom, "底层下移应无变化");
    assert_eq!(z_order_ids(&s), vec!["a", "b"]);
}

// ── 新建置顶 + 读模型稳定面 ─────────────────────────────────────

#[test]
fn new_objects_stack_on_top() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    add_rect(&mut s, "c", "#00ff00");
    // 新建即置顶（行业惯例）：a0 < a1 < a2
    let za = z_of(&s, "a");
    let zb = z_of(&s, "b");
    let zc = z_of(&s, "c");
    assert!(za.as_str() < zb.as_str() && zb.as_str() < zc.as_str());
    assert_eq!(render_fills(&s), vec!["#ff0000", "#0000ff", "#00ff00"]);
}

#[test]
fn objects_stays_id_sorted_and_z_view_separate() {
    let mut s = session();
    add_rect(&mut s, "z-node", "#ff0000");
    add_rect(&mut s, "a-node", "#0000ff");
    reorder(&mut s, "a-node", ReorderOp::BringToFront);
    // objects() 恒 ID 序（读模型稳定面契约不动）
    let ids: Vec<String> = s.objects().into_iter().map(|o| o.id).collect();
    assert_eq!(ids, vec!["a-node", "z-node"]);
    // z 视图按 (z, id)
    assert_eq!(z_order_ids(&s), vec!["z-node", "a-node"]);
}

// ── 验收 2：批量插入 1000 次不重复不退化 ─────────────────────────

#[test]
fn batch_create_1000_unique_bounded_z() {
    let mut s = session();
    let mut zs: Vec<String> = Vec::with_capacity(1000);
    for i in 0..1000 {
        let mut props = BTreeMap::new();
        props.insert("fill".to_string(), "#ff0000".to_string());
        s.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some(format!("n{i}")),
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                props,
            },
        )
        .unwrap();
        zs.push(z_of(&s, &format!("n{i}")));
    }
    let mut sorted = zs.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 1000, "z 键全不重复");
    let max_len = zs.iter().map(|z| z.len()).max().unwrap();
    assert!(
        max_len <= 4,
        "新建置顶走整数步进，键长应摊还常数（实得 {max_len}）"
    );
    // 全序与创建序一致（置顶语义：后建在上）
    assert_eq!(z_order_ids(&s)[0], "n0");
    assert_eq!(z_order_ids(&s)[999], "n999");
}

#[test]
fn repeated_bring_to_front_1000_bounded() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    let mut max_len = 0;
    for _ in 0..1000 {
        // b 恒置顶（相对 a 的绝对置顶——整数步进路径）
        reorder(&mut s, "b", ReorderOp::BringToFront);
        max_len = max_len.max(z_of(&s, "b").len());
    }
    assert!(max_len <= 5, "绝对置顶走整数步进（实得 {max_len}）");
    assert_eq!(z_order_ids(&s), vec!["a", "b"]);
}

// ── 验收 3：yrs 合并后 z 序收敛 ─────────────────────────────────

#[test]
fn z_order_converges_after_merge() {
    let mut alice = CanvasSession::open(opts(201)).unwrap();
    let mut bob = CanvasSession::open(opts(202)).unwrap();

    // 场景一：两副本各自独立建对象（同邻位插入 → 预期撞键）→ 合并
    for (s, prefix) in [(&mut alice, "a"), (&mut bob, "b")] {
        for i in 0..5 {
            let mut props = BTreeMap::new();
            props.insert("fill".to_string(), "#ff0000".to_string());
            s.execute(
                Actor::Human,
                CanvasCommand::AddNode {
                    id: Some(format!("{prefix}{i}")),
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                    props,
                },
            )
            .unwrap();
        }
    }
    // 互通（双向 sync——收方 state vector 取差）
    sync_both(&alice, &bob);

    let order_a = z_order_ids(&alice);
    let order_b = z_order_ids(&bob);
    assert_eq!(order_a, order_b, "合并后两副本 z 全序一致（收敛）");
    assert_eq!(order_a.len(), 10);
    // 撞键由 (z, id) 决胜吸收：同键对象按 ID 稳定排列
    let zs: Vec<String> = alice
        .objects_z_ordered()
        .into_iter()
        .map(|o| o.z_index)
        .collect();
    let mut sorted = zs.clone();
    sorted.sort();
    assert_eq!(zs, sorted, "z 视图按键升序");

    // 场景二：同步后的副本各自**继续**置顶插入 → 再合并 → 仍收敛
    add_rect(&mut alice, "a-new", "#00ff00");
    add_rect(&mut bob, "b-new", "#00ff00");
    sync_both(&alice, &bob);
    assert_eq!(z_order_ids(&alice), z_order_ids(&bob), "二次合并仍收敛");
    // 后同步插入方（按合并序）在顶层附近——顶层由键值决定，两副本一致
    let top = z_order_ids(&alice)[11].clone();
    assert!(top == "a-new" || top == "b-new", "顶层必为最后插入者之一");
}

// ── 撤销 / 重做 ────────────────────────────────────────────────

#[test]
fn reorder_undo_redo_restores_z() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    let z_before = z_of(&s, "a");
    reorder(&mut s, "a", ReorderOp::BringToFront);
    let z_after = z_of(&s, "a");
    assert_ne!(z_before, z_after);

    assert!(s.undo(Actor::Human), "撤销重排");
    assert_eq!(z_of(&s, "a"), z_before, "撤销恢复原 z 键");
    assert!(s.redo(Actor::Human), "重做重排");
    assert_eq!(z_of(&s, "a"), z_after, "重做恢复新 z 键");
}

// ── 回执与错误面 ────────────────────────────────────────────────

#[test]
fn reorder_receipt_and_not_found() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    let receipt = s
        .execute(
            Actor::Human,
            CanvasCommand::Reorder {
                id: "a".into(),
                op: ReorderOp::SendToBack,
            },
        )
        .unwrap();
    assert_eq!(
        receipt.outcome,
        CommandOutcome::Reordered { id: "a".into() }
    );

    match s.execute(
        Actor::Human,
        CanvasCommand::Reorder {
            id: "ghost".into(),
            op: ReorderOp::BringToFront,
        },
    ) {
        Err(CanvasError::ObjectNotFound { object_id }) => assert_eq!(object_id, "ghost"),
        other => panic!("应报 ObjectNotFound，实得 {other:?}"),
    }
}

#[test]
fn all_kinds_reorderable() {
    let mut s = session();
    add_rect(&mut s, "n1", "#ff0000");
    add_rect(&mut s, "n2", "#0000ff");
    // 边（AddEdge 需端点存在）
    s.execute(
        Actor::Human,
        CanvasCommand::AddEdge {
            id: Some("e".into()),
            from: "n1".into(),
            to: "n2".into(),
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    // 笔迹
    s.execute(
        Actor::Human,
        CanvasCommand::AddStroke {
            id: Some("st".into()),
            points: vec![(0.0, 0.0), (10.0, 10.0)],
            color: "#00ff00".into(),
            width: 2.0,
            tool: StrokeTool::Pen,
        },
    )
    .unwrap();
    // 分组
    s.execute(
        Actor::Human,
        CanvasCommand::GroupObjects {
            group_id: Some("g".into()),
            members: vec!["n1".into(), "n2".into()],
        },
    )
    .unwrap();
    // 四种对象都可重排（Group 的 z 不影响成员渲染——模型一致性面）
    for id in ["e", "st", "g"] {
        reorder(&mut s, id, ReorderOp::BringToFront);
        assert_eq!(z_order_ids(&s).last().unwrap(), id, "{id} 置顶");
    }
    assert_eq!(s.objects_z_ordered().len(), 5);
}

// ── 证据面：重排 = 语义变更 → 指纹失效 ──────────────────────────

#[test]
fn reorder_invalidates_object_fingerprint() {
    let mut s = session();
    add_rect(&mut s, "a", "#ff0000");
    add_rect(&mut s, "b", "#0000ff");
    let fp_before = s.object_fingerprint("a").unwrap();
    reorder(&mut s, "a", ReorderOp::BringToFront);
    let fp_after = s.object_fingerprint("a").unwrap();
    assert_ne!(fp_before, fp_after, "z 键入指纹——重排即语义变更");
    // 画布指纹（全对象）同样变化
    assert_ne!(
        {
            let mut s2 = session();
            add_rect(&mut s2, "a", "#ff0000");
            add_rect(&mut s2, "b", "#0000ff");
            s2.canvas_fingerprint()
        },
        s.canvas_fingerprint()
    );
}

// ── 存量对象兼容（缺 z 字段 → DEFAULT_KEY）─────────────────────

#[test]
fn legacy_object_without_z_reads_default() {
    // 字面量构造（Default 兜底——AGENTS 0.0.5 方案 A 的消费面证明）
    let o = CanvasObject {
        id: "legacy".into(),
        kind: ObjectKind::Node,
        ..Default::default()
    };
    assert_eq!(o.z_index, mdc_canvas::zorder::DEFAULT_KEY);
}
