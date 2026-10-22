//! 服务端中继测试（集成项⑥——权威端/中继形态）。
//!
//! 钉定面：
//! - 文档域三端扇出收敛（客户端事件驱动转发——组合⑥×③）；
//! - 晚到端差量追平（SyncRequest/SyncResponse——不发快照）；
//! - 在场层扇出（组合⑥×④）；
//! - 画布域同构接入（SyncReplica 门面——领域无关证明）；
//! - 快照检查点 + 新中继恢复（崩溃恢复形态）；
//! - 间隔自动检查点 / 离开停投 / 失败零扇出（fail-closed）。

mod canvas_harness;
mod doc_harness;

use doc_harness::DocReplica;
use mdc_collab::{CollabSession, Presence, SessionOptions};
use mdc_relay::{RelayDelivery, RelayError, RelayHub, RelayMessage, SyncReplica};

// ── 文档域门面已在 doc_harness（测试件共享）──

fn doc_session(id: u64) -> CollabSession {
    CollabSession::open(SessionOptions {
        client_id: id,
        ..Default::default()
    })
    .unwrap()
}

fn relay(replica: Box<dyn SyncReplica>, interval: Option<u64>) -> RelayHub {
    RelayHub::new(replica, 7_900_900, 1024 * 1024, interval).unwrap()
}

#[test]
fn doc_three_client_fanout_convergence() {
    // 三端经中继：各端自己的命令 update → 中继 → 其余端；收敛。
    let mut net = doc_harness::Net::new();
    let ids = [7_900_001u64, 7_900_002, 7_900_003];
    for id in ids {
        net.join(id);
    }
    net.client_edit(7_900_001, "A 端段落", mdc_collab::Actor::Human);
    net.client_edit(7_900_002, "B 端段落", mdc_collab::Actor::Ai);
    net.client_edit(7_900_003, "C 端段落", mdc_collab::Actor::Human);
    net.pump();

    let views: Vec<_> = ids.iter().map(|id| net.doc_of(*id)).collect();
    assert_eq!(views[0], views[1], "A==B");
    assert_eq!(views[1], views[2], "B==C");
    assert_eq!(views[0].len(), 3, "三段齐备");
    assert_eq!(net.relay.applied_doc_updates(), 3, "权威副本应用三次");
}

#[test]
fn late_joiner_diff_sync() {
    let mut net = doc_harness::Net::new();
    net.join(7_900_011);
    net.join(7_900_012);
    net.client_edit(7_900_011, "早期编辑一", mdc_collab::Actor::Human);
    net.client_edit(7_900_011, "早期编辑二", mdc_collab::Actor::Ai);
    net.pump();

    // C 晚到加入 → 差量追平（不经快照）。
    net.join(7_900_013);
    net.sync_request(7_900_013);
    net.pump();
    assert_eq!(
        net.doc_of(7_900_013),
        net.doc_of(7_900_011),
        "晚到端差量追平"
    );
    assert_eq!(net.doc_of(7_900_013).len(), 2);
}

#[test]
fn presence_fanout_via_relay() {
    let mut net = doc_harness::Net::new();
    let ids = [7_900_021u64, 7_900_022, 7_900_023];
    for id in ids {
        net.join(id);
    }
    // A 设光标在场态 → 中继 → B/C 聚合。
    net.client_presence(7_900_021, r#"{"cursor":{"block":"b1"},"user":"甲"}"#);
    net.pump();
    for id in ids[1..].iter() {
        let states = net.presence_of(*id);
        let a_state = states
            .iter()
            .find(|s| s.client_id == 7_900_021)
            .expect("B/C 见 A 在场态");
        assert_eq!(
            a_state.json.as_deref(),
            Some(r#"{"cursor":{"block":"b1"},"user":"甲"}"#)
        );
    }
    // 中继侧聚合视图亦含 A。
    let relay_view: Vec<_> = net
        .relay
        .presence_mut()
        .states()
        .into_iter()
        .filter(|s| s.client_id == 7_900_021)
        .collect();
    assert_eq!(relay_view.len(), 1);
}

#[test]
fn canvas_domain_same_relay() {
    // SyncReplica 门面的领域无关证明：画布域经同一中继形态收敛。
    let mut net = canvas_harness::CanvasNet::new();
    let ids = [7_900_031u64, 7_900_032];
    for id in ids {
        net.join(id);
    }
    net.client_add_node(7_900_031, "n1", 10.0, 20.0);
    net.client_move_node(7_900_032, "n1", 99.0, 88.0);
    net.pump();
    let a = net.canvas_of(7_900_031);
    let b = net.canvas_of(7_900_032);
    assert_eq!(a, b, "画布双端收敛");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].x, 99.0);
    assert_eq!(a[0].y, 88.0);
}

#[test]
fn snapshot_checkpoint_and_new_relay_recovery() {
    // 崩溃恢复形态：快照 → 新中继（新副本 + 快照应用）→ 新端追平。
    let mut net = doc_harness::Net::new();
    net.join(7_900_041);
    net.join(7_900_042);
    net.client_edit(7_900_041, "恢复前编辑", mdc_collab::Actor::Human);
    net.pump();
    let snap = net.relay.snapshot().unwrap();
    assert!(!snap.is_empty());

    // 新中继：新副本 + 快照经 DocUpdate 应用（update 即状态迁移）。
    let mut hub2 = relay(Box::new(DocReplica(doc_session(7_900_900))), None);
    hub2.ingest(RelayMessage::DocUpdate {
        from: 7_900_900,
        bytes: snap,
    })
    .unwrap();

    // 新端从恢复态差量追平。
    let c = doc_session(7_900_043);
    let diff = hub2
        .ingest(RelayMessage::SyncRequest {
            from: 7_900_043,
            sv: c.state_vector(),
        })
        .unwrap();
    for (target, d) in diff {
        assert_eq!(target, 7_900_043);
        match d {
            RelayDelivery::SyncResponse { bytes } => c.apply_update(&bytes).unwrap(),
            other => panic!("期望 SyncResponse，得到 {other:?}"),
        }
    }
    assert_eq!(c.document(), net.doc_of(7_900_041), "恢复态 + 新端追平一致");
}

#[test]
fn interval_auto_checkpoint() {
    let mut net = doc_harness::Net::new_interval(Some(2));
    net.join(7_900_051);
    net.client_edit(7_900_051, "一", mdc_collab::Actor::Human);
    net.pump();
    assert!(net.relay.latest_snapshot().is_none(), "1 次未到间隔");
    net.client_edit(7_900_051, "二", mdc_collab::Actor::Human);
    net.pump();
    let snap = net
        .relay
        .latest_snapshot()
        .expect("2 次触发检查点")
        .to_vec();
    let manual = net.relay.snapshot().unwrap();
    // 检查点与手动快照语义一致（同为权威副本单点编码）。
    assert_eq!(snap.len(), manual.len());
}

#[test]
fn leave_stops_delivery() {
    let mut net = doc_harness::Net::new();
    net.join(7_900_061);
    net.join(7_900_062);
    net.relay
        .ingest(RelayMessage::Leave {
            client_id: 7_900_062,
        })
        .unwrap();
    assert!(!net.relay.clients().contains(&7_900_062), "已移出扇出集");
    net.client_edit(7_900_061, "B 已离开后的编辑", mdc_collab::Actor::Human);
    net.pump();
    assert_eq!(net.doc_of(7_900_062).len(), 0, "离开端零投递");
}

#[test]
fn failed_update_zero_fanout() {
    let mut net = doc_harness::Net::new();
    net.join(7_900_071);
    net.join(7_900_072);
    // 垃圾字节 → 副本拒绝 → 零扇出（fail-closed）。
    let err = net
        .relay
        .ingest(RelayMessage::DocUpdate {
            from: 7_900_071,
            bytes: vec![0xFF; 16],
        })
        .unwrap_err();
    assert!(matches!(err, RelayError::Replica(_)));
    // 权威副本未受影响：后续合法 update 正常应用扇出。
    net.client_edit(7_900_071, "恢复后编辑", mdc_collab::Actor::Human);
    net.pump();
    assert_eq!(net.doc_of(7_900_072).len(), 1);
}

// ── 直接 API 面小检（不经 harness）──

#[test]
fn join_idempotent_and_presence_limit_inherited() {
    // 中枢在场层限额 8B（构造参数）——超限在场 update 拒收。
    let mut hub = RelayHub::new(
        Box::new(DocReplica(doc_session(7_900_901))),
        7_900_900,
        8,
        None,
    )
    .unwrap();
    hub.ingest(RelayMessage::Join { client_id: 42 }).unwrap();
    hub.ingest(RelayMessage::Join { client_id: 42 }).unwrap();
    assert_eq!(hub.clients().len(), 1, "重复加入幂等");
    // 合法但超中枢限额的在场 wire（构造端限额宽裕）。
    let mut p = Presence::standalone(7_900_902, 1024 * 1024).unwrap();
    p.set_local_state(r#"{"u":"x"}"#).unwrap();
    let wire = p.encode_update().unwrap();
    assert!(wire.len() > 8);
    let err = hub
        .ingest(RelayMessage::PresenceUpdate {
            from: 42,
            bytes: wire,
        })
        .unwrap_err();
    assert!(matches!(err, RelayError::Replica(_)), "在场限额拒收");
}
