//! 中枢形态可运行演示（集成项⑥ 收口的演示面——与 e2e_demo /
//! collab_body_demo 同型：分幕叙事 + [PASS] 检查点断言自检，
//! 任何环节失守 = 非零退出）。
//!
//! 运行：`cargo run -p mdc-relay --example relay_demo`
//!
//! 场景（全部合成数据）：
//! 三端（人类工程师 A / AI 助手 B / 评审员 C）经中继常驻协作一份
//! 采购纪要；晚到端 D 经差量追平；B 撤销自己的编辑跨端传播；
//! 中继崩溃后从检查点恢复，权威态无损；全编队审计为规范形状
//! （组合⑥×audit——外部数据校验面的常驻姿态）。
//!
//! 本演示自含极简 harness（事件驱动转发——与 tests/doc_harness
//! 同构：客户端订阅自己的命令/撤销事件产 update，送中继扇出；
//! Remote 到达不回发，无回声环）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mdc_collab::{
    Actor, BlockKind, BlockSpec, ChangeSource, CollabCommand, CollabEvent, CollabSession,
    SessionOptions, Subscription,
};
use mdc_relay::{RelayDelivery, RelayError, RelayHub, RelayMessage, SyncReplica};

// ── 演示脚手架 ───────────────────────────────────────────────────

fn step(n: usize, title: &str) {
    println!("\n== 第{n}幕：{title} ==");
}

fn pass(msg: &str) {
    println!("  [PASS] {msg}");
}

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

/// 文档域门面（宿主侧薄包装——孤儿规则，与 tests 同型）。
struct DocReplica(CollabSession);

impl SyncReplica for DocReplica {
    fn state_vector(&self) -> Vec<u8> {
        self.0.state_vector()
    }
    fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, RelayError> {
        self.0
            .encode_diff(remote_sv)
            .map_err(|e| RelayError::Replica(e.to_string()))
    }
    fn apply_update(&mut self, bytes: &[u8]) -> Result<(), RelayError> {
        self.0
            .apply_update(bytes)
            .map_err(|e| RelayError::Replica(e.to_string()))
    }
    fn snapshot(&self) -> Result<Vec<u8>, RelayError> {
        self.0
            .authoritative_snapshot()
            .map_err(|e| RelayError::Replica(e.to_string()))
    }
}

/// 极简中枢网络（事件驱动——命令/撤销产物经中继扇出）。
struct Net {
    relay: RelayHub,
    clients: HashMap<u64, CollabSession>,
    _subs: HashMap<u64, Subscription>,
    queues: HashMap<u64, Arc<Mutex<Vec<Vec<u8>>>>>,
    presence_seen: HashMap<u64, usize>,
    /// 最近一次送出的发送端产物字节（字节直转发验证面——与
    /// `last_forwarded` 对照 = 中继零重编码证明）。
    last_sent: Vec<u8>,
    /// 最近一次扇出的原始字节（字节直转发验证面）。
    last_forwarded: Vec<u8>,
}

impl Net {
    /// `interval` = 自动检查点间隔（DocUpdate 计数）。
    fn new(interval: Option<u64>) -> Net {
        let replica = CollabSession::open(opts(7_900_800)).unwrap();
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
            presence_seen: HashMap::new(),
            last_sent: Vec::new(),
            last_forwarded: Vec::new(),
        }
    }

    fn join(&mut self, id: u64) {
        let mut s = CollabSession::open(opts(id)).unwrap();
        let queue: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let q2 = Arc::clone(&queue);
        let sub = s.subscribe(move |e: &CollabEvent| {
            // 只转发自己的命令/撤销/重做产出（Remote 不回发——无回声环）。
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

    /// 客户端命令（产物入队后送中继扇出）。
    fn edit(&mut self, id: u64, index: u32, text: &str, actor: Actor) {
        let s = self.clients.get_mut(&id).unwrap();
        s.execute(
            actor,
            CollabCommand::InsertBlock {
                parent: None,
                index,
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

    fn undo(&mut self, id: u64, actor: Actor) -> bool {
        let s = self.clients.get_mut(&id).unwrap();
        let r = s.undo(actor);
        if r {
            self.flush(id);
        }
        r
    }

    /// 设在场态并送中继扇出。
    fn presence(&mut self, id: u64, json: &str) {
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

    /// 差量追平请求（晚到端）。
    fn sync_request(&mut self, id: u64) {
        let sv = self.clients.get(&id).unwrap().state_vector();
        let out = self
            .relay
            .ingest(RelayMessage::SyncRequest { from: id, sv })
            .unwrap();
        for (t, d) in out {
            self.deliver(t, d);
        }
    }

    fn flush(&mut self, id: u64) {
        let updates: Vec<Vec<u8>> =
            std::mem::take(&mut *self.queues.get(&id).unwrap().lock().unwrap());
        for u in updates {
            self.last_sent = u.clone();
            let out = self
                .relay
                .ingest(RelayMessage::DocUpdate { from: id, bytes: u })
                .unwrap();
            for (t, d) in out {
                self.deliver(t, d);
            }
        }
    }

    fn deliver(&mut self, target: u64, d: RelayDelivery) {
        match d {
            RelayDelivery::DocUpdate { bytes } => {
                self.last_forwarded = bytes.clone();
                if let Some(s) = self.clients.get_mut(&target) {
                    s.apply_update(&bytes).unwrap();
                }
            }
            RelayDelivery::SyncResponse { bytes } => {
                if let Some(s) = self.clients.get_mut(&target) {
                    s.apply_update(&bytes).unwrap();
                }
            }
            RelayDelivery::PresenceUpdate { bytes } => {
                if let Some(s) = self.clients.get_mut(&target) {
                    s.presence_mut().apply_update(&bytes).unwrap();
                    let n = s.presence_mut().states().len();
                    self.presence_seen.insert(target, n);
                }
            }
        }
    }

    fn doc_texts(&self, id: u64) -> Vec<String> {
        self.clients
            .get(&id)
            .unwrap()
            .document()
            .into_iter()
            .map(|b| b.text)
            .collect()
    }
}

fn main() {
    println!("==============================================================");
    println!(" 中枢形态 —— mdc-relay 服务端中继可运行演示（headless 断言自检）");
    println!(" 三端常驻协作 / 权威副本扇出 / 差量追平 / 撤销跨端 / 检查点恢复");
    println!("==============================================================");

    // 检查点间隔 = 3 次 DocUpdate（第 2/4 幕的编辑量覆盖触发点）。
    let mut net = Net::new(Some(3));

    // ── 第 1 幕：三端上线与在场聚合 ─────────────────────────────
    step(1, "三端上线——Join 登记与在场互见");

    net.join(7_100_001); // A：人类工程师
    net.join(7_100_002); // B：AI 助手
    net.join(7_100_003); // C：评审员
    assert_eq!(net.relay.clients().len(), 3);
    // Join 幂等（重复登记不增目）。
    net.relay
        .ingest(RelayMessage::Join {
            client_id: 7_100_001,
        })
        .unwrap();
    assert_eq!(net.relay.clients().len(), 3);
    pass("① Join 登记 + 重复加入幂等（3 端恒定）");

    net.presence(7_100_001, r#"{"cursor":12,"mode":"edit"}"#);
    net.presence(7_100_002, r#"{"cursor":0,"mode":"gen"}"#);
    // 各端可见另外两端（两个远端在场态）。
    assert_eq!(net.presence_seen.get(&7_100_001), Some(&2));
    assert_eq!(net.presence_seen.get(&7_100_003), Some(&2));
    // 中继侧聚合视图（晚到端补齐通道）。
    assert_eq!(net.relay.presence_mut().states().len(), 2);
    pass("② 在场扇出互见（A/C 各见 2 端）+ 中继聚合视图在位");

    // ── 第 2 幕：并发编辑权威扇出收敛 ───────────────────────────
    step(2, "并发编辑——权威副本先应用，原样字节扇出收敛");

    net.edit(7_100_001, 0, "纪要：设备采购评审（A 起草）", Actor::Human);
    net.edit(7_100_002, 1, "AI 补充：预算上限 12000 元", Actor::Ai);
    net.edit(7_100_003, 2, "C 评审意见：三家比价后再定", Actor::Human);

    let texts_a = net.doc_texts(7_100_001);
    let texts_b = net.doc_texts(7_100_002);
    let texts_c = net.doc_texts(7_100_003);
    assert_eq!(texts_a, texts_b);
    assert_eq!(texts_b, texts_c);
    assert_eq!(texts_a.len(), 3);
    pass("③ 三端并发编辑经中继扇出——读模型三端一致（3 段）");

    // 权威副本状态 = 各端状态（经 SyncResponse 全量互证）。
    let relay_full = {
        let probe = CollabSession::open(opts(7_100_099)).unwrap();
        let out = net
            .relay
            .ingest(RelayMessage::SyncRequest {
                from: 7_100_099,
                sv: probe.state_vector(),
            })
            .unwrap();
        match &out[0].1 {
            RelayDelivery::SyncResponse { bytes } => bytes.clone(),
            _ => unreachable!(),
        }
    };
    let probe = CollabSession::open(opts(7_100_099)).unwrap();
    probe.apply_update(&relay_full).unwrap();
    let texts_relay: Vec<String> = probe.document().into_iter().map(|b| b.text).collect();
    assert_eq!(texts_relay, texts_a);
    pass("④ 权威副本态 = 各端态（SyncResponse 全量互证）");

    // 字节直转发（中继不重编码——扇出字节与发送端产物逐字节一致；
    // 注：位置插入 update 携带邻居依赖，独立应用到空端本就应待决
    // ——差量/全量才是追平通道，见第 3 幕）。
    assert!(!net.last_forwarded.is_empty());
    assert_eq!(
        net.last_sent, net.last_forwarded,
        "扇出字节与发送端产物逐字节一致"
    );
    pass("⑤ 扇出字节 = 入向原样（零重编码——逐字节对照）");

    // ── 第 3 幕：晚到端差量追平 ─────────────────────────────────
    step(3, "晚到端——Join 零投递，差量请求最小追平");

    net.join(7_100_004); // D：晚到的财务代表
    let join_out = net
        .relay
        .ingest(RelayMessage::Join {
            client_id: 7_100_004,
        })
        .unwrap();
    assert!(join_out.is_empty());
    assert!(net.doc_texts(7_100_004).is_empty(), "Join 不回放历史");
    pass("⑥ 晚到 Join 零投递（历史回放不是 Join 语义）");

    net.sync_request(7_100_004);
    assert_eq!(net.doc_texts(7_100_004), texts_a, "差量追平后与三端一致");
    pass("⑦ SyncRequest → SyncResponse 差量追平（D 与权威态一致）");

    // ── 第 4 幕：撤销跨端传播 ───────────────────────────────────
    step(4, "撤销跨端——B 撤自己的编辑，四端一致回退");

    assert!(net.undo(7_100_002, Actor::Ai));
    let texts_after: Vec<Vec<String>> = (7_100_001..=7_100_004)
        .map(|id| net.doc_texts(id))
        .collect();
    assert!(texts_after.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(texts_after[0].len(), 2, "B 的 AI 段撤销后剩 2 段");
    assert_eq!(
        texts_after[0][0], "纪要：设备采购评审（A 起草）",
        "A 的编辑不受影响"
    );
    pass("⑧ B 撤销自己编辑——四端一致回退（撤销产物同样经中继扇出）");
    pass("⑨ 分栈语义——A/C 的编辑不受 B 撤销波及");

    // ── 第 5 幕：检查点与崩溃恢复 ───────────────────────────────
    step(5, "检查点恢复——快照 = 检查点时刻态（诚实语义）");

    // 间隔检查点（间隔 3：第 2 幕恰 3 次 DocUpdate 触发；第 4 幕
    // 撤销 = 第 4 次，未到下一触发点——**撤销不在快照内**）。
    assert!(net.relay.latest_snapshot().is_some(), "自动检查点已触发");
    assert!(net.relay.applied_doc_updates() >= 3);
    pass("⑩ 间隔自动检查点在位（第 3 次 DocUpdate 触发）");

    let snapshot = net.relay.latest_snapshot().unwrap().to_vec();
    // 崩溃 = 丢弃中枢；恢复 = 新中枢 + 快照字节（DocUpdate 即状态
    // 迁移——不"重开"副本）。快照 = 检查点时刻（3 段含 AI 段；
    // 其后的撤销编辑随旧中枢丢失——间隔与丢失窗 = 部署取舍面）。
    let mut net2 = Net::new(Some(3));
    net2.relay
        .ingest(RelayMessage::DocUpdate {
            from: 0, // 传输层来源占位（快照回放非端身份面）
            bytes: snapshot,
        })
        .unwrap();
    // 各端重连（演示 harness 端会话随旧 Net 丢弃——以新端 + 差量
    // 追平等价演示"重连追平"通道）。
    for id in (7_100_001..=7_100_004).rev() {
        net2.join(id);
    }
    for id in 7_100_001..=7_100_004 {
        net2.sync_request(id);
    }
    // 新端追平后 = 检查点时刻态（3 段——AI 段在：撤销发生在快照后）。
    for id in 7_100_001..=7_100_004 {
        let t = net2.doc_texts(id);
        assert_eq!(t.len(), 3, "端 {id} 追平后 = 检查点时刻态");
        assert_eq!(t[0], "纪要：设备采购评审（A 起草）");
        assert_eq!(t[1], "AI 补充：预算上限 12000 元");
    }
    pass("⑪ 新中枢恢复 = 检查点时刻态（3 段——快照后编辑不在内）");

    // 恢复后的中枢照常扇出（后续编辑全编队收敛）。
    net2.edit(7_100_004, 3, "D 财务复核：预算口径确认", Actor::Human);
    for id in 7_100_001..=7_100_004 {
        let t = net2.doc_texts(id);
        assert_eq!(t.len(), 4, "端 {id} 恢复后继续收敛");
        assert_eq!(t[3], "D 财务复核：预算口径确认");
    }
    pass("⑫ 恢复后中枢继续扇出——四端收敛（新编辑生效）");

    // 全编队审计：规范形状恒成立（组合⑥×audit——含中继副本面）。
    for id in 7_100_001..=7_100_004 {
        let r = net2.clients.get(&id).unwrap().audit_self();
        assert!(r.is_clean(), "端 {id} 审计 Error 非空: {:?}", r.errors);
        assert!(r.warnings.is_empty(), "端 {id} 审计 Warning 非空");
    }
    let replica_snap = net2.relay.snapshot().unwrap();
    let audit_probe = CollabSession::open(opts(7_100_097)).unwrap();
    let r = audit_probe.audit_update(&replica_snap).unwrap();
    assert!(r.is_clean() && r.warnings.is_empty());
    pass("⑬ 全编队 audit 自审干净（四端 + 权威快照面）");

    println!("\n中枢形态全链检查点 ①-⑬——全部 [PASS]");
    println!("（三端常驻 / 权威扇出 / 差量追平 / 撤销跨端 / 检查点恢复 / 审计姿态）");
}
