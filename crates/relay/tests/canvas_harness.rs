//! 画布域中继测试 harness（SyncReplica 门面的领域无关接入证明）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mdc_canvas::{
    Actor, CanvasChangeSource, CanvasCommand, CanvasEvent, CanvasSession, SessionOptions,
    Subscription,
};
use mdc_relay::{RelayDelivery, RelayHub, RelayMessage, SyncReplica};

/// 画布域门面（宿主侧薄包装）。
pub struct CanvasReplica(pub CanvasSession);

impl SyncReplica for CanvasReplica {
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

pub struct CanvasNet {
    pub relay: RelayHub,
    clients: HashMap<u64, CanvasSession>,
    _subs: HashMap<u64, Subscription>,
    queues: HashMap<u64, UpdateQueue>,
}

impl Default for CanvasNet {
    fn default() -> Self {
        CanvasNet::new()
    }
}

impl CanvasNet {
    pub fn new() -> CanvasNet {
        let replica = CanvasSession::open(SessionOptions {
            client_id: 7_900_850,
            ..Default::default()
        })
        .unwrap();
        CanvasNet {
            relay: RelayHub::new(
                Box::new(CanvasReplica(replica)),
                7_900_950,
                1024 * 1024,
                None,
            )
            .unwrap(),
            clients: HashMap::new(),
            _subs: HashMap::new(),
            queues: HashMap::new(),
        }
    }

    pub fn join(&mut self, id: u64) {
        let mut s = CanvasSession::open(SessionOptions {
            client_id: id,
            ..Default::default()
        })
        .unwrap();
        let queue: UpdateQueue = Arc::default();
        let q2 = Arc::clone(&queue);
        let sub = s.subscribe(move |e: &CanvasEvent| {
            if matches!(
                e.source,
                CanvasChangeSource::Command { .. }
                    | CanvasChangeSource::Undo { .. }
                    | CanvasChangeSource::Redo { .. }
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

    pub fn client_add_node(&mut self, id: u64, node_id: &str, x: f64, y: f64) {
        let s = self.clients.get_mut(&id).unwrap();
        s.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some(node_id.into()),
                x,
                y,
                w: 100.0,
                h: 60.0,
                props: Default::default(),
            },
        )
        .unwrap();
        self.flush(id);
    }

    pub fn client_move_node(&mut self, id: u64, node_id: &str, x: f64, y: f64) {
        let s = self.clients.get_mut(&id).unwrap();
        s.execute(
            Actor::Ai,
            CanvasCommand::MoveNode {
                id: node_id.into(),
                x,
                y,
            },
        )
        .unwrap();
        self.flush(id);
    }

    fn flush(&mut self, id: u64) {
        let updates: Vec<Vec<u8>> =
            std::mem::take(&mut *self.queues.get(&id).unwrap().lock().unwrap());
        for u in updates {
            let out = self
                .relay
                .ingest(RelayMessage::DocUpdate { from: id, bytes: u })
                .unwrap();
            for (t, d) in out {
                if let RelayDelivery::DocUpdate { bytes } = d {
                    if let Some(s) = self.clients.get_mut(&t) {
                        s.apply_update(&bytes).unwrap();
                    }
                }
            }
        }
    }

    pub fn pump(&mut self) {}

    pub fn canvas_of(&self, id: u64) -> Vec<mdc_canvas::CanvasObject> {
        self.clients.get(&id).unwrap().objects()
    }
}
