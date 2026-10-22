//! 事件/订阅面测试（集成项③——交付报告 §3.3）。
//!
//! 钉定面：
//! - 事件模型：一个公共变更入口恰一个事件；updates = 逐事务 wire
//!   字节（可独立转发）；失败命令不产事件。
//! - 来源标注：Command/Undo/Redo/Remote/Maintenance 与公共入口
//!   一一对应（方法上下文标注——不依赖 origin 推断）。
//! - 派发纪律：延迟派发（事务释放后）；订阅者只收 &CollabEvent。
//! - 生命周期：Drop 退订；多订阅者全派发。

use std::sync::{Arc, Mutex};

use mdc_collab::{
    Actor, BlockKind, BlockSpec, ChangeSource, CollabCommand, CollabError, SessionOptions,
};

fn spec(text: &str) -> BlockSpec {
    BlockSpec {
        kind: BlockKind::Paragraph,
        attrs: Default::default(),
        text: text.to_string(),
    }
}

fn session(id: u64) -> mdc_collab::CollabSession {
    mdc_collab::CollabSession::open(SessionOptions {
        client_id: id,
        ..Default::default()
    })
    .expect("open")
}

/// 记录器（跨回调收集事件——Arc<Mutex> 因回调需 Send + 'static）。
#[derive(Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<mdc_collab::CollabEvent>>>,
}

impl Recorder {
    fn new() -> Self {
        Self::default()
    }
    fn attach(&self, s: &mut mdc_collab::CollabSession) -> mdc_collab::Subscription {
        let ev = Arc::clone(&self.events);
        s.subscribe(move |e: &mdc_collab::CollabEvent| {
            ev.lock().unwrap().push(e.clone());
        })
    }
    fn take(&self) -> Vec<mdc_collab::CollabEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

#[test]
fn command_event_source_and_updates() {
    let mut a = session(7_200_001);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    a.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("正文一"),
            id: None,
        },
    )
    .expect("insert");

    let events = rec.take();
    assert_eq!(events.len(), 1, "一命令恰一事件");
    assert_eq!(
        events[0].source,
        ChangeSource::Command {
            actor: Actor::Human
        }
    );
    assert_eq!(events[0].updates.len(), 1, "单命令单事务");
    assert!(!events[0].updates[0].is_empty(), "update 字节非空");
}

#[test]
fn ai_actor_source_distinction() {
    let mut a = session(7_200_002);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    a.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("AI 生成段"),
            id: None,
        },
    )
    .expect("insert");
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, ChangeSource::Command { actor: Actor::Ai });
}

#[test]
fn event_updates_forwardable_to_peer() {
    // 宿主转发模式钉定：A 事件 updates 逐条 apply 到 B → 双端读模型
    // 收敛（事件字节与 encode_diff 产物同为 v1 update——对称面）。
    let mut a = session(7_200_003);
    let b = session(7_200_004);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    a.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("转发段"),
            id: None,
        },
    )
    .expect("insert");
    a.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec("第二段"),
            id: None,
        },
    )
    .expect("insert 2");

    let events = rec.take();
    assert_eq!(events.len(), 2);
    for e in &events {
        for u in &e.updates {
            b.apply_update(u).expect("forward apply");
        }
    }
    assert_eq!(a.document(), b.document(), "逐条转发后读模型收敛");
}

#[test]
fn load_blocks_multi_txn_single_event() {
    // 批量装载 = 512/事务小批路径 → 一命令一事件，updates = 每小批
    // 一条（600 块 = 2 批）。
    let mut a = session(7_200_005);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    let blocks: Vec<BlockSpec> = (0..600).map(|i| spec(&format!("块 {i}"))).collect();
    a.execute(Actor::Ai, CollabCommand::LoadBlocks { blocks })
        .expect("load");

    let events = rec.take();
    assert_eq!(events.len(), 1, "装载 = 一命令一事件");
    assert_eq!(events[0].updates.len(), 2, "600 块 = 2 小批事务");
    assert_eq!(a.document().len(), 600);
}

#[test]
fn remote_event_on_apply() {
    let mut a = session(7_200_006);
    let mut b = session(7_200_007);
    a.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("远端源"),
            id: None,
        },
    )
    .expect("insert");

    let diff = a.encode_diff(&b.state_vector()).expect("diff");
    let rec = Recorder::new();
    let _sub = rec.attach(&mut b);
    b.apply_update(&diff).expect("apply");

    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, ChangeSource::Remote);
    assert_eq!(events[0].updates.len(), 1);
    assert_eq!(a.document(), b.document());
}

#[test]
fn undo_redo_events() {
    let mut a = session(7_200_008);
    a.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("待撤销"),
            id: None,
        },
    )
    .expect("insert");
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    assert!(a.undo(Actor::Human), "撤销成功");
    let events = rec.take();
    assert_eq!(events.len(), 1, "undo 恰一事件");
    assert_eq!(
        events[0].source,
        ChangeSource::Undo {
            actor: Actor::Human
        }
    );
    assert!(!events[0].updates.is_empty(), "撤销事务产 update 字节");
    assert!(a.document().is_empty(), "撤销后文档空");

    assert!(a.redo(Actor::Human), "重做成功");
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].source,
        ChangeSource::Redo {
            actor: Actor::Human
        }
    );
    assert_eq!(a.document().len(), 1);
}

#[test]
fn undo_empty_stack_no_event() {
    let mut a = session(7_200_009);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    assert!(!a.undo(Actor::Human), "空栈撤销 = false");
    assert!(rec.take().is_empty(), "无产出无事件");
}

#[test]
fn failed_command_no_event() {
    let mut a = session(7_200_010);
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);

    let err = a
        .execute(
            Actor::Human,
            CollabCommand::RemoveBlock {
                block_id: "不存在".into(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, CollabError::BlockNotFound { .. }));
    assert!(rec.take().is_empty(), "失败命令不产事件");
}

#[test]
fn subscription_lifecycle() {
    let mut a = session(7_200_011);
    let rec1 = Recorder::new();
    let rec2 = Recorder::new();
    let sub1 = rec1.attach(&mut a);
    let _sub2 = rec2.attach(&mut a);

    a.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("双订阅者"),
            id: None,
        },
    )
    .expect("insert");
    assert_eq!(rec1.take().len(), 1, "订阅者 1 收到");
    assert_eq!(rec2.take().len(), 1, "订阅者 2 收到");

    drop(sub1);
    a.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec("退订后"),
            id: None,
        },
    )
    .expect("insert 2");
    assert!(rec1.take().is_empty(), "Drop 退订即失效");
    assert_eq!(rec2.take().len(), 1, "存续订阅者不受影响");
}

#[test]
fn maintenance_event_pin() {
    // 维护窗事件钉定：maintenance_gc 恒产 Maintenance 事件；GC 不产
    // 新 CRDT 操作 → updates 恒空（观测钉定——若上游行为变化此处
    // 即红）。
    let mut a = session(7_200_012);
    a.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("压缩前内容"),
            id: None,
        },
    )
    .expect("insert");
    let rec = Recorder::new();
    let _sub = rec.attach(&mut a);
    let _ = a.maintenance_gc().expect("gc");
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, ChangeSource::Maintenance);
    assert!(events[0].updates.is_empty(), "GC 无新操作——updates 空");
}

#[test]
fn restored_session_events_after_recovery() {
    // 恢复语义：restore 的新会话无历史事件（订阅随实例新生）；恢复后
    // 首个新变更正常派发。
    let mut src = session(7_200_013);
    src.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec("快照内容"),
            id: None,
        },
    )
    .expect("insert");
    let snap = src.authoritative_snapshot().expect("snapshot");

    let mut restored = mdc_collab::CollabSession::restore(
        SessionOptions {
            client_id: 7_200_014,
            ..Default::default()
        },
        &snap,
    )
    .expect("restore");
    assert_eq!(restored.document().len(), 1, "恢复态保真");

    let rec = Recorder::new();
    let _sub = rec.attach(&mut restored);
    restored
        .execute(
            Actor::Ai,
            CollabCommand::InsertBlock {
                parent: None,
                index: 1,
                spec: spec("恢复后新变更"),
                id: None,
            },
        )
        .expect("insert after restore");
    let events = rec.take();
    assert_eq!(events.len(), 1, "恢复后首个变更正常派发");
    assert_eq!(events[0].source, ChangeSource::Command { actor: Actor::Ai });
}
