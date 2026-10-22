//! 文档域中继测试 harness（事件驱动转发——组合⑥×③：客户端订阅
//! 自己的命令事件产 update，送中继扇出）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mdc_collab::{
    Actor, BlockKind, BlockSpec, ChangeSource, CollabCommand, CollabEvent, CollabSession,
    SessionOptions, Subscription,
};
use mdc_relay::{RelayDelivery, RelayHub, RelayMessage, SyncReplica};

/// 文档域门面（宿主侧薄包装——孤儿规则）。
pub struct DocReplica(pub CollabSession);

impl SyncReplica for DocReplica {
    fn state_vector(&self) -> Vec<u8> {
        self.0.state_vector()
    }
    fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, mdc_relay::RelayError> {
        self.0
            .encode_diff(remote_sv)
            .map_err(|e| mdc_relay::RelayError::Replica(e.to_string()))
    }
    fn apply_update(&mut self, bytes: &[u8]) -> Result<(), mdc_relay::RelayError> {
        self.0
            .apply_update(bytes)
            .map_err(|e| mdc_relay::RelayError::Replica(e.to_string()))
    }
    fn snapshot(&self) -> Result<Vec<u8>, mdc_relay::RelayError> {
        self.0
            .authoritative_snapshot()
            .map_err(|e| mdc_relay::RelayError::Replica(e.to_string()))
    }
}

type UpdateQueue = Arc<Mutex<Vec<Vec<u8>>>>;

pub struct Net {
    pub relay: RelayHub,
    clients: HashMap<u64, CollabSession>,
    /// 订阅句柄（存活 = 事件回调存活——与 Net 同寿命）。
    _subs: HashMap<u64, Subscription>,
    /// 各端自己的命令/撤销产物队列（回调写入；flush 送中继）。
    queues: HashMap<u64, UpdateQueue>,
    /// 各端收到的在场态（验证面）。
    presence_states: HashMap<u64, Vec<mdc_collab::PresenceState>>,
}

impl Default for Net {
    fn default() -> Self {
        Net::new_interval(None)
    }
}

impl Net {
    pub fn new() -> Net {
        Net::new_interval(None)
    }

    pub fn new_interval(interval: Option<u64>) -> Net {
        let replica = CollabSession::open(SessionOptions {
            client_id: 7_900_800,
            ..Default::default()
        })
        .unwrap();
        Net {
            relay: RelayHub::new(
                Box::new(DocReplica(replica)),
                7_900_900,
                1024 * 1024,
                interval,
            )
            .unwrap(),
            clients: HashMap::new(),
            _subs: HashMap::new(),
            queues: HashMap::new(),
            presence_states: HashMap::new(),
        }
    }

    pub fn join(&mut self, id: u64) {
        let mut s = CollabSession::open(SessionOptions {
            client_id: id,
            ..Default::default()
        })
        .unwrap();
        let queue: UpdateQueue = Arc::default();
        let q2 = Arc::clone(&queue);
        let sub = s.subscribe(move |e: &CollabEvent| {
            // 只转发自己的命令/撤销/重做产出（Remote 到达不回发——
            // 无回声环）。
            if matches!(
                e.source,
                ChangeSource::Command { .. }
                    | ChangeSource::Undo { .. }
                    | ChangeSource::Redo { .. }
            ) {
                q2.lock().unwrap().extend(e.updates.iter().cloned());
            }
        });
        self._subs.insert(id, sub);
        self.queues.insert(id, queue);
        self.clients.insert(id, s);
        self.relay
            .ingest(RelayMessage::Join { client_id: id })
            .unwrap();
    }

    /// 客户端命令（事件产物入队列后 flush 送中继扇出）。
    pub fn client_edit(&mut self, id: u64, text: &str, actor: Actor) {
        let s = self.clients.get_mut(&id).unwrap();
        s.execute(
            actor,
            CollabCommand::InsertBlock {
                parent: None,
                index: 0,
                spec: BlockSpec {
                    kind: BlockKind::Paragraph,
                    attrs: Default::default(),
                    text: text.into(),
                },
                id: None,
            },
        )
        .unwrap();
        self.flush(id);
    }

    /// 客户端设在场态并送中继（组合⑥×④）。
    pub fn client_presence(&mut self, id: u64, json: &str) {
        let s = self.clients.get_mut(&id).unwrap();
        s.presence_mut().set_local_state(json).unwrap();
        let wire = s.presence_mut().encode_update().unwrap();
        let deliveries = self
            .relay
            .ingest(RelayMessage::PresenceUpdate {
                from: id,
                bytes: wire,
            })
            .unwrap();
        for (t, d) in deliveries {
            self.deliver(t, d);
        }
    }

    pub fn sync_request(&mut self, id: u64) {
        let sv = self.clients.get(&id).unwrap().state_vector();
        let out = self
            .relay
            .ingest(RelayMessage::SyncRequest { from: id, sv })
            .unwrap();
        for (t, d) in out {
            self.deliver(t, d);
        }
    }

    /// 队列 → 中继（逐条 DocUpdate——事件 updates 的宿主转发形态）。
    fn flush(&mut self, id: u64) {
        let updates: Vec<Vec<u8>> =
            std::mem::take(&mut *self.queues.get(&id).unwrap().lock().unwrap());
        for u in updates {
            let out = self
                .relay
                .ingest(RelayMessage::DocUpdate { from: id, bytes: u })
                .unwrap();
            for (t, d) in out {
                self.deliver(t, d);
            }
        }
    }

    /// 投递应用（文档/在场/差量应答三分派；即时应用——异步传输宿主
    /// 的收敛循环位在 pump 语义锚）。
    fn deliver(&mut self, target: u64, d: RelayDelivery) {
        match d {
            RelayDelivery::DocUpdate { bytes } | RelayDelivery::SyncResponse { bytes } => {
                if let Some(s) = self.clients.get_mut(&target) {
                    s.apply_update(&bytes).unwrap();
                }
            }
            RelayDelivery::PresenceUpdate { bytes } => {
                if let Some(s) = self.clients.get_mut(&target) {
                    s.presence_mut().apply_update(&bytes).unwrap();
                    let states = s.presence_mut().states();
                    self.presence_states.insert(target, states);
                }
            }
        }
    }

    /// 泵送（本 harness 投递即应用——pump 为异步宿主收敛循环的语义
    /// 锚，保留接口面）。
    pub fn pump(&mut self) {}

    pub fn doc_of(&self, id: u64) -> Vec<mdc_collab::BlockInfo> {
        self.clients.get(&id).unwrap().document()
    }

    pub fn presence_of(&self, id: u64) -> Vec<mdc_collab::PresenceState> {
        self.presence_states.get(&id).cloned().unwrap_or_default()
    }
}
