//! 画布事件/订阅面测试（集成项③——touched 对象 ID 画布域增量）。
//!
//! 钉定面：事件模型（一入口一事件/失败不产事件）+ 来源标注 +
//! **touched 提取双路径**（根 Map 自身事件 keys() vs 嵌套事件
//! path 首段）+ 转发收敛 + 撤销/远端/维护窗。

use std::sync::{Arc, Mutex};

use mdc_canvas::{Actor, CanvasChangeSource, CanvasCommand, CanvasError, SessionOptions};

fn session(id: u64) -> mdc_canvas::CanvasSession {
    mdc_canvas::CanvasSession::open(SessionOptions {
        client_id: id,
        ..Default::default()
    })
    .expect("open")
}

#[derive(Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<mdc_canvas::CanvasEvent>>>,
}

impl Recorder {
    fn new() -> Self {
        Self::default()
    }
    fn attach(&self, s: &mut mdc_canvas::CanvasSession) -> mdc_canvas::Subscription {
        let ev = Arc::clone(&self.events);
        s.subscribe(move |e: &mdc_canvas::CanvasEvent| {
            ev.lock().unwrap().push(e.clone());
        })
    }
    fn take(&self) -> Vec<mdc_canvas::CanvasEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

fn add_node(s: &mut mdc_canvas::CanvasSession, x: f64, y: f64) -> String {
    match s
        .execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: None,
                x,
                y,
                w: 100.0,
                h: 60.0,
                props: Default::default(),
            },
        )
        .expect("add node")
        .outcome
    {
        mdc_canvas::CommandOutcome::NodeAdded { id } => id,
        _ => panic!("unexpected outcome"),
    }
}

#[test]
fn command_event_touched_root_insert() {
    // 根 Map 自身键插入（AddNode/AddEdge）→ touched 从 MapEvent keys 提取。
    let mut a = session(7_300_001);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    let id1 = add_node(&mut a, 0.0, 0.0);
    let id2 = add_node(&mut a, 200.0, 0.0);

    let events = rec.take();
    assert_eq!(events.len(), 2, "一命令一事件");
    assert_eq!(events[0].touched, vec![id1.clone()], "单对象触及");
    assert_eq!(events[0].updates.len(), 1);
    assert_eq!(
        events[0].source,
        CanvasChangeSource::Command {
            actor: Actor::Human
        }
    );
    assert_eq!(events[1].touched, vec![id2.clone()]);
    // 字典序语义钉定：第二节点 id 序大（自增）。
    assert!(id1 < id2, "自增 id 字典序递增");
}

#[test]
fn move_touches_nested_path() {
    // 嵌套变更（几何字段在对象子 Map 内）→ path 首段 = 对象 ID。
    let mut a = session(7_300_002);
    let id = add_node(&mut a, 0.0, 0.0);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    a.execute(
        Actor::Ai,
        CanvasCommand::MoveNode {
            id: id.clone(),
            x: 42.0,
            y: 99.0,
        },
    )
    .expect("move");

    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].touched, vec![id], "嵌套 path 首段提取");
    assert_eq!(
        events[0].source,
        CanvasChangeSource::Command { actor: Actor::Ai }
    );
}

#[test]
fn remove_and_props_touched() {
    let mut a = session(7_300_003);
    let id = add_node(&mut a, 0.0, 0.0);
    let mut props = std::collections::BTreeMap::new();
    props.insert("label".to_string(), "甲".to_string());
    a.execute(
        Actor::Human,
        CanvasCommand::SetProps {
            id: id.clone(),
            props: props.clone(),
        },
    )
    .expect("set props");
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    a.execute(Actor::Human, CanvasCommand::RemoveObject { id: id.clone() })
        .expect("remove");
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].touched, vec![id.clone()], "根键删除触及");
    assert!(a.find_object(&id).is_none());

    // 撤销删除 → 同 ID 回归（画布域身份特性）+ touched 钉定。
    assert!(a.undo(Actor::Human), "undo remove");
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].source,
        CanvasChangeSource::Undo {
            actor: Actor::Human
        }
    );
    assert_eq!(events[0].touched, vec![id.clone()], "撤销复原触及同 ID");
    assert!(a.find_object(&id).is_some(), "同 ID 回归");
}

#[test]
fn group_touched_is_group_object() {
    // 分组 = 新分组对象键插入（成员对象自身不变）→ touched = 分组 ID。
    let mut a = session(7_300_004);
    let m1 = add_node(&mut a, 0.0, 0.0);
    let m2 = add_node(&mut a, 100.0, 0.0);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    let outcome = a
        .execute(
            Actor::Human,
            CanvasCommand::GroupObjects {
                group_id: None,
                members: vec![m1, m2],
            },
        )
        .expect("group")
        .outcome;
    let gid = match outcome {
        mdc_canvas::CommandOutcome::Grouped { group_id } => group_id,
        _ => panic!("unexpected"),
    };
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].touched, vec![gid], "分组触及 = 分组对象键");
}

#[test]
fn remote_event_touched_from_deep_observer() {
    // 远端应用：A 拖动节点 → B apply 差量 → B 事件 Remote 且
    // touched 由深观察提取（对端操作的本地镜像）。
    let mut a = session(7_300_005);
    let mut b = session(7_300_006);
    let id = add_node(&mut a, 0.0, 0.0);
    let diff = a.encode_diff(&b.state_vector()).expect("diff0");
    b.apply_update(&diff).expect("apply0");

    let rec = Recorder::new();
    let _sub = rec.attach(&mut b);
    a.execute(
        Actor::Human,
        CanvasCommand::MoveNode {
            id: id.clone(),
            x: 500.0,
            y: 500.0,
        },
    )
    .expect("move on A");
    let diff = a.encode_diff(&b.state_vector()).expect("diff1");
    b.apply_update(&diff).expect("apply on B");

    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, CanvasChangeSource::Remote);
    assert_eq!(events[0].touched, vec![id], "远端深变更触及提取");
    assert_eq!(a.objects(), b.objects(), "双端读模型收敛");
}

#[test]
fn event_updates_forwardable_roundtrip() {
    // 事件 updates 逐条转发 → 对端收敛（宿主转发模式钉定）。
    let mut a = session(7_300_007);
    let b = session(7_300_008);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    let n1 = add_node(&mut a, 0.0, 0.0);
    let n2 = add_node(&mut a, 300.0, 0.0);
    a.execute(
        Actor::Ai,
        CanvasCommand::AddEdge {
            id: None,
            from: n1.clone(),
            to: n2.clone(),
            props: Default::default(),
        },
    )
    .expect("edge");
    for e in rec.take() {
        for u in &e.updates {
            b.apply_update(u).expect("forward");
        }
    }
    assert_eq!(a.objects(), b.objects());
    assert_eq!(b.objects().len(), 3, "两节点 + 边");
}

#[test]
fn failed_command_no_event() {
    let mut a = session(7_300_009);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    let err = a
        .execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "不存在".into(),
                x: 0.0,
                y: 0.0,
            },
        )
        .unwrap_err();
    assert!(matches!(err, CanvasError::ObjectNotFound { .. }));
    assert!(rec.take().is_empty(), "失败命令不产事件");
}

#[test]
fn maintenance_event_pin() {
    let mut a = session(7_300_010);
    add_node(&mut a, 0.0, 0.0);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    let _ = a.maintenance_gc().expect("gc");
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, CanvasChangeSource::Maintenance);
    assert!(events[0].updates.is_empty(), "GC 无新操作");
    assert!(events[0].touched.is_empty(), "GC 无对象触及");
}

#[test]
fn subscription_lifecycle_and_multi_subscriber() {
    let mut a = session(7_300_011);
    let rec1 = Recorder::new();
    let rec2 = Recorder::new();
    let sub1 = rec1.attach(&mut a);
    let _sub2 = rec2.attach(&mut a);
    add_node(&mut a, 0.0, 0.0);
    assert_eq!(rec1.take().len(), 1);
    assert_eq!(rec2.take().len(), 1);
    drop(sub1);
    add_node(&mut a, 10.0, 10.0);
    assert!(rec1.take().is_empty(), "Drop 退订即失效");
    assert_eq!(rec2.take().len(), 1);
}

#[test]
fn undo_empty_and_redo_events() {
    let mut a = session(7_300_012);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    assert!(!a.undo(Actor::Human), "空栈");
    assert!(rec.take().is_empty());

    let id = add_node(&mut a, 0.0, 0.0);
    let _ = rec.take(); // 丢弃 add 事件（只验撤销面）
    assert!(a.undo(Actor::Human));
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].source,
        CanvasChangeSource::Undo {
            actor: Actor::Human
        }
    );
    assert_eq!(events[0].touched, vec![id.clone()]);

    assert!(a.redo(Actor::Human));
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].source,
        CanvasChangeSource::Redo {
            actor: Actor::Human
        }
    );
    assert_eq!(events[0].touched, vec![id]);
}
