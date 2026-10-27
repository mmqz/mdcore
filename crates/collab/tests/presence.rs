//! 在场层（awareness）测试（集成项④——交付报告 §3.4）。
//!
//! 钉定面：本地状态 fail-closed（JSON 校验）/ wire 双向（encode ↔
//! apply 收敛）/ null 状态离线语义 / clock 陈旧取舍 / 入向限额 +
//! panic 隔离 / standalone 构造（画布宿主路径）/ 订阅生命周期 /
//! 会话 client_id 对齐。

use std::sync::{Arc, Mutex};

use mdc_collab::{CollabError, Presence, PresenceState, SessionOptions};

fn session(id: u64) -> mdc_collab::CollabSession {
    mdc_collab::CollabSession::open(SessionOptions {
        client_id: id,
        ..Default::default()
    })
    .expect("open")
}

#[derive(Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<mdc_collab::PresenceEvent>>>,
}

impl Recorder {
    fn new() -> Self {
        Self::default()
    }
    fn attach(&self, p: &mut Presence) -> mdc_collab::PresenceSubscription {
        let ev = Arc::clone(&self.events);
        p.subscribe(move |e: &mdc_collab::PresenceEvent| {
            ev.lock().unwrap().push(e.clone());
        })
    }
    fn take(&self) -> Vec<mdc_collab::PresenceEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

const CURSOR: &str = r#"{"cursor":{"block":"7-1","anchor":1,"head":3},"user":"人类甲"}"#;

#[test]
fn local_state_lifecycle_and_events() {
    let mut s = session(7_400_001);
    let rec = Recorder::new();
    let _sub = rec.attach(s.presence_mut());

    // 首次设置 = added。
    s.presence_mut().set_local_state(CURSOR).expect("set");
    assert_eq!(s.presence_mut().local_state(), Some(CURSOR.into()));
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].added, vec![7_400_001]);
    assert!(events[0].updated.is_empty());
    assert!(events[0].removed.is_empty());

    // 再设置 = updated（时钟推进）。
    s.presence_mut()
        .set_local_state(r#"{"cursor":null,"user":"人类甲"}"#)
        .expect("set 2");
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].updated, vec![7_400_001]);

    // 清除 = removed（null 离线语义）。
    s.presence_mut().clear_local_state();
    assert_eq!(s.presence_mut().local_state(), None);
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].removed, vec![7_400_001]);
}

#[test]
fn invalid_json_rejected() {
    let mut s = session(7_400_002);
    let err = s.presence_mut().set_local_state("这不是 JSON").unwrap_err();
    assert!(matches!(err, CollabError::InvalidInput(_)));
    assert_eq!(s.presence_mut().local_state(), None, "拒收零残留");
}

#[test]
fn wire_roundtrip_and_peer_event() {
    let mut a = session(7_400_003);
    let mut b = session(7_400_004);
    a.presence_mut().set_local_state(CURSOR).expect("set A");

    let wire = a.presence_mut().encode_update().expect("encode");
    let rec = Recorder::new();
    let _sub = rec.attach(b.presence_mut());
    b.presence_mut().apply_update(&wire).expect("apply B");

    let states = b.presence_mut().states();
    assert_eq!(states.len(), 1, "B 见 A 的状态");
    assert_eq!(states[0].client_id, 7_400_003);
    assert_eq!(states[0].json.as_deref(), Some(CURSOR));
    assert_eq!(states[0].clock, 1);
    let events = rec.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].added, vec![7_400_003], "对端 added 事件");
}

#[test]
fn offline_null_state_propagation() {
    let mut a = session(7_400_005);
    let mut b = session(7_400_006);
    a.presence_mut().set_local_state(CURSOR).expect("set");
    let wire = a.presence_mut().encode_update().expect("enc 1");
    b.presence_mut().apply_update(&wire).expect("apply 1");

    // A 离线（null + clock+1）→ B 收敛为离线态。
    a.presence_mut().clear_local_state();
    let wire = a.presence_mut().encode_update().expect("enc 2");
    let rec = Recorder::new();
    let _sub = rec.attach(b.presence_mut());
    b.presence_mut().apply_update(&wire).expect("apply 2");
    let states = b.presence_mut().states();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].json, None, "null 状态 = 离线");
    assert_eq!(states[0].clock, 2, "时钟推进");
    let events = rec.take();
    assert_eq!(events[0].removed, vec![7_400_005], "离线 = removed 事件");
}

#[test]
fn stale_clock_ignored() {
    // 陈旧 update（clock 低）不回退已知状态——yjs 语义钉定。
    let mut a = session(7_400_007);
    let mut b = session(7_400_008);
    a.presence_mut().set_local_state(CURSOR).expect("set 1");
    let stale_wire = a.presence_mut().encode_update().expect("enc stale");
    a.presence_mut()
        .set_local_state(r#"{"cursor":null,"user":"甲"}"#)
        .expect("set 2");
    let fresh_wire = a.presence_mut().encode_update().expect("enc fresh");

    b.presence_mut().apply_update(&fresh_wire).expect("fresh");
    let before = b.presence_mut().states();
    b.presence_mut().apply_update(&stale_wire).expect("stale");
    let after = b.presence_mut().states();
    assert_eq!(before, after, "陈旧 update 零回退");
    assert_eq!(after[0].clock, 2);
}

#[test]
fn byte_limit_and_garbage_isolation() {
    // 限额：构造小限额 standalone，真实 update 字节超限 → Limit。
    let mut p = Presence::standalone(7_400_009, 8).expect("standalone");
    p.set_local_state(CURSOR).expect("set");
    let wire = p.encode_update().expect("encode");
    assert!(wire.len() > 8);
    let err = p.apply_update(&wire).unwrap_err();
    assert!(matches!(err, CollabError::Limit { .. }));

    // 垃圾字节 → Decode（panic 隔离面——不崩进程）。
    let mut q = Presence::standalone(7_400_010, 1024 * 1024).expect("standalone");
    let err = q.apply_update(&[0xFF; 32]).unwrap_err();
    assert!(matches!(err, CollabError::Decode(_)));
}

#[test]
fn standalone_construction_domain() {
    // 画布宿主路径：standalone 构造 + client_id 读回 + 53 位域校验。
    let mut p = Presence::standalone(7_400_011, 1024 * 1024).expect("standalone");
    assert_eq!(p.client_id(), 7_400_011);
    p.set_local_state(r#"{"domain":"canvas","cursor":{"node":"n1"}}"#)
        .expect("canvas state");
    assert!(Presence::standalone(0, 1024).is_err(), "client_id 0 拒");
    assert!(
        Presence::standalone(1u64 << 53, 1024).is_err(),
        "53 位上界拒"
    );
    assert!(
        Presence::standalone((1u64 << 53) - 1, 1024).is_ok(),
        "域内恰过"
    );
}

#[test]
fn session_presence_client_id_aligned() {
    let mut s = session(7_400_012);
    assert_eq!(
        s.presence_mut().client_id(),
        7_400_012,
        "会话在场层 client_id 对齐"
    );
}

#[test]
fn multi_client_states_sorted_and_merge() {
    // 多客户端状态合并（三方各自 set → 两两 apply → 全员一致，
    // client_id 升序）。
    let mut a = session(7_400_013);
    let mut b = session(7_400_014);
    let mut c = session(7_400_015);
    a.presence_mut().set_local_state(r#"{"u":"A"}"#).expect("A");
    b.presence_mut().set_local_state(r#"{"u":"B"}"#).expect("B");
    c.presence_mut().set_local_state(r#"{"u":"C"}"#).expect("C");

    // 全量互发（宿主中继形态的最小模型：各端 encode → 逐端 apply）。
    let wa = a.presence_mut().encode_update().expect("wa");
    let wb = b.presence_mut().encode_update().expect("wb");
    let wc = c.presence_mut().encode_update().expect("wc");
    b.presence_mut().apply_update(&wa).expect("b←a");
    c.presence_mut().apply_update(&wa).expect("c←a");
    a.presence_mut().apply_update(&wb).expect("a←b");
    c.presence_mut().apply_update(&wb).expect("c←b");
    a.presence_mut().apply_update(&wc).expect("a←c");
    b.presence_mut().apply_update(&wc).expect("b←c");
    let sa = a.presence_mut().states();
    let sb = b.presence_mut().states();
    let sc = c.presence_mut().states();
    // 跨视图收敛断言只比较语义面（client_id/clock/json）：
    // last_updated_ms 是接收方本地活性时钟（yrs Awareness 语义
    // ——「何时见到该状态」），本端 set 时刻与对端 apply 时刻
    // 天然不同，毫秒边界跨拍即合法失配，不属收敛不变量（新鲜
    // 环境实测复现：同条目两视图 ...019 vs ...020）。
    let semantic = |v: &[PresenceState]| -> Vec<(u64, u32, Option<String>)> {
        v.iter()
            .map(|s| (s.client_id, s.clock, s.json.clone()))
            .collect()
    };
    assert_eq!(semantic(&sa), semantic(&sb), "A/B 视图语义收敛");
    assert_eq!(semantic(&sb), semantic(&sc), "B/C 视图语义收敛");
    assert_eq!(sa.len(), 3);
    assert!(
        sa.windows(2).all(|w| w[0].client_id < w[1].client_id),
        "client_id 升序"
    );
}

#[test]
fn subscription_lifecycle() {
    let mut s = session(7_400_016);
    let rec1 = Recorder::new();
    let rec2 = Recorder::new();
    let sub1 = rec1.attach(s.presence_mut());
    let _sub2 = rec2.attach(s.presence_mut());
    s.presence_mut().set_local_state(CURSOR).expect("set");
    assert_eq!(rec1.take().len(), 1);
    assert_eq!(rec2.take().len(), 1);
    drop(sub1);
    s.presence_mut()
        .set_local_state(r#"{"u":"x"}"#)
        .expect("set2");
    assert!(rec1.take().is_empty(), "Drop 退订即失效");
    assert_eq!(rec2.take().len(), 1);
}

#[test]
fn targeted_encode_for_known_clients() {
    let mut a = session(7_400_017);
    let mut b = session(7_400_018);
    a.presence_mut().set_local_state(r#"{"u":"A"}"#).expect("A");
    b.presence_mut().set_local_state(r#"{"u":"B"}"#).expect("B");
    let wb = b.presence_mut().encode_update().expect("wb");
    a.presence_mut().apply_update(&wb).expect("B 进 A");

    // 定向编码：A 只发自己的状态给新端。
    let targeted = a
        .presence_mut()
        .encode_update_for(&[7_400_017])
        .expect("targeted");
    let mut c = session(7_400_019);
    c.presence_mut().apply_update(&targeted).expect("apply");
    let states = c.presence_mut().states();
    assert_eq!(states.len(), 1, "只含 A");
    assert_eq!(states[0].client_id, 7_400_017);

    // 未知客户端 → 显式错误。
    assert!(a.presence_mut().encode_update_for(&[999]).is_err());
}
