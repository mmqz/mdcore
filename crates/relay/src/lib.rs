//! mdc-relay —— 服务端中继（集成项⑥：权威端/中继形态）。
//!
//! # 产品语义
//!
//! 多端常驻协作的中枢状态机：客户端把 update 送入中继，中继应用到
//! **权威副本**（[`SyncReplica`] 门面）并扇出给其他端；晚到端经
//! 差量同步（sv → diff）追平；快照检查点（手动或间隔触发）提供
//! 崩溃恢复点。当前 P2P 直连形态（各端 encode_diff/apply_update
//! 互发）在 >2 端时的扇出与常驻问题由本形态收口。
//!
//! ```text
//! 客户端 A ──DocUpdate──▶ ┌──────────┐ ──DocUpdate──▶ 客户端 B
//!                        │ 中继中枢  │
//! 客户端 C ──SyncRequest─▶ 权威副本  │ ──SyncResponse─▶ 客户端 C
//!                        └──────────┘
//! ```
//!
//! # 架构纪律
//!
//! - **传输无关**：本 crate 是纯状态机（入向消息 → 出向投递表）；
//!   零网络/线程/进程面。`from` 字段 = 传输层已认证身份（中继不
//!   重签——宿主责任，文档化边界）。
//! - **领域无关**：副本经 [`SyncReplica`] 门面（文档域/画布域同步
//!   API 同构）；门面类型属宿主侧（孤儿规则），本 crate 不依赖
//!   任何域引擎类型。
//! - **字节直转发**：入向 update 原样扇出（CRDT 幂等/交换律）；
//!   中继不重编码、不解析内容。
//! - **限额与隔离继承**：字节门与 panic 隔离在会话侧（B1.6 全入
//!   向限额），门面把错误映射为 [`RelayError`]——中继层无第二套
//!   限额面。
//!
//! # 恢复语义
//!
//! 快照 = 权威副本的 update 编码（skip_gc 单点编码）。新中继实例
//! 恢复 = 新副本 + `ingest(DocUpdate{..快照字节..})`（update 应用
//! 即状态迁移——不"重开"副本）；撤销栈不随快照走（B3.1）与端侧
//! 语义一致。

use std::collections::BTreeSet;

use mdc_collab::Presence;

/// 副本门面（宿主侧实现——文档域/画布域同步 API 的同构抽象）。
///
/// 孤儿规则注：trait 在本 crate，域会话类型在域 crate——宿主以
/// 薄包装类型（`struct DocReplica(CollabSession)`）实现本 trait
/// 接入；本 crate 零域引擎依赖。
pub trait SyncReplica {
    /// 本端状态向量（不透明 wire 字节）。
    fn state_vector(&self) -> Vec<u8>;
    /// 差量编码（对端 sv → 本端新增部分）。
    fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, RelayError>;
    /// 应用 update（字节门 + panic 隔离在域会话内）。
    fn apply_update(&mut self, bytes: &[u8]) -> Result<(), RelayError>;
    /// 权威快照（单点编码语义）。
    fn snapshot(&self) -> Result<Vec<u8>, RelayError>;
}

/// 中继错误（公开面零域引擎类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayError {
    /// 副本面错误（域会话错误的字符串化映射——原始类型不穿透）。
    Replica(String),
    /// 消息面语义错误。
    InvalidInput(String),
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelayError::Replica(m) => write!(f, "replica: {m}"),
            RelayError::InvalidInput(m) => write!(f, "invalid input: {m}"),
        }
    }
}

impl std::error::Error for RelayError {}

/// 入向消息（客户端 → 中继；宿主桥接传输）。
#[derive(Debug, Clone)]
pub enum RelayMessage {
    /// 端加入（登记扇出目标；重复加入幂等）。
    Join { client_id: u64 },
    /// 端离开（移出扇出目标）。
    Leave { client_id: u64 },
    /// 文档 update（应用到权威副本 + 扇出其他端）。
    DocUpdate { from: u64, bytes: Vec<u8> },
    /// 在场 update（聚合到中继在场层 + 扇出其他端）。
    PresenceUpdate { from: u64, bytes: Vec<u8> },
    /// 差量同步请求（定向应答 SyncResponse）。
    SyncRequest { from: u64, sv: Vec<u8> },
}

/// 出向投递（中继 → 客户端；(目标端, 投递) 序列由 [`RelayHub::ingest`]
/// 返回——宿主负责实际送达与重试语义）。
#[derive(Debug, Clone)]
pub enum RelayDelivery {
    /// 文档 update 广播（字节 = 入向原样）。
    DocUpdate { bytes: Vec<u8> },
    /// 在场 update 广播（字节 = 入向原样）。
    PresenceUpdate { bytes: Vec<u8> },
    /// 差量同步应答（定向）。
    SyncResponse { bytes: Vec<u8> },
}

/// 中继中枢。
pub struct RelayHub {
    replica: Box<dyn SyncReplica>,
    presence: Presence,
    clients: BTreeSet<u64>,
    /// 自动检查点间隔（每 N 次成功应用的 DocUpdate 触发一次快照
    /// 缓存；`None` = 手动）。
    snapshot_interval: Option<u64>,
    applied_doc_updates: u64,
    latest_snapshot: Option<Vec<u8>>,
}

impl RelayHub {
    /// 构造（`relay_client_id` = 中继自身身份——在场层聚合用，
    /// 必须与各端 client_id 互异；`presence_limit` = 在场入向限额）。
    pub fn new(
        replica: Box<dyn SyncReplica>,
        relay_client_id: u64,
        presence_limit: usize,
        snapshot_interval: Option<u64>,
    ) -> Result<RelayHub, RelayError> {
        let presence =
            Presence::standalone(relay_client_id, presence_limit).map_err(|e| match e {
                mdc_collab::CollabError::Limit { size, limit } => {
                    RelayError::InvalidInput(format!("presence limit {size}>{limit}"))
                }
                mdc_collab::CollabError::InvalidInput(m) => RelayError::InvalidInput(m),
                other => RelayError::InvalidInput(other.to_string()),
            })?;
        Ok(RelayHub {
            replica,
            presence,
            clients: BTreeSet::new(),
            snapshot_interval,
            applied_doc_updates: 0,
            latest_snapshot: None,
        })
    }

    /// 在场层访问（中继侧聚合视图——晚到端补齐/观测面）。
    pub fn presence_mut(&mut self) -> &mut Presence {
        &mut self.presence
    }

    /// 已登记端集合（观测面）。
    pub fn clients(&self) -> &BTreeSet<u64> {
        &self.clients
    }

    /// 处理一条入向消息 → 出向投递表。
    ///
    /// 语义表：
    /// - `Join`：登记（幂等）；无投递。新端追平 = 宿主对齐后续
    ///   `SyncRequest`（差量）或快照路径。
    /// - `Leave`：移出；无投递。
    /// - `DocUpdate`：权威副本应用（失败 = 整条拒绝零扇出——fail
    ///   closed）→ 原样字节扇出其余端；间隔检查点触发时刷新
    ///   `latest_snapshot`。
    /// - `PresenceUpdate`：中继在场层应用 + 原样扇出其余端。
    /// - `SyncRequest`：sv 差量 → 定向 SyncResponse（不发快照——
    ///   差量是最小追平通道）。
    pub fn ingest(&mut self, msg: RelayMessage) -> Result<Vec<(u64, RelayDelivery)>, RelayError> {
        match msg {
            RelayMessage::Join { client_id } => {
                self.clients.insert(client_id);
                Ok(vec![])
            }
            RelayMessage::Leave { client_id } => {
                self.clients.remove(&client_id);
                Ok(vec![])
            }
            RelayMessage::DocUpdate { from, bytes } => {
                // 权威副本先应用（fail-closed：应用失败零扇出）。
                self.replica.apply_update(&bytes)?;
                self.applied_doc_updates += 1;
                if let Some(interval) = self.snapshot_interval {
                    if interval > 0 && self.applied_doc_updates.is_multiple_of(interval) {
                        self.latest_snapshot = Some(self.replica.snapshot()?);
                    }
                }
                Ok(self.broadcast_except(from, RelayDelivery::DocUpdate { bytes }))
            }
            RelayMessage::PresenceUpdate { from, bytes } => {
                // 聚合到中继在场层（限额/隔离在 Presence 内）。
                self.presence
                    .apply_update(&bytes)
                    .map_err(|e| RelayError::Replica(e.to_string()))?;
                Ok(self.broadcast_except(from, RelayDelivery::PresenceUpdate { bytes }))
            }
            RelayMessage::SyncRequest { from, sv } => {
                let diff = self.replica.encode_diff(&sv)?;
                Ok(vec![(from, RelayDelivery::SyncResponse { bytes: diff })])
            }
        }
    }

    /// 手动快照（同时刷新检查点缓存）。
    pub fn snapshot(&mut self) -> Result<Vec<u8>, RelayError> {
        let snap = self.replica.snapshot()?;
        self.latest_snapshot = Some(snap.clone());
        Ok(snap)
    }

    /// 最近检查点（间隔触发或手动 `snapshot` 的缓存；`None` = 无）。
    pub fn latest_snapshot(&self) -> Option<&[u8]> {
        self.latest_snapshot.as_deref()
    }

    /// 已应用 DocUpdate 计数（观测面——检查点触发审计）。
    pub fn applied_doc_updates(&self) -> u64 {
        self.applied_doc_updates
    }

    fn broadcast_except(&self, from: u64, delivery: RelayDelivery) -> Vec<(u64, RelayDelivery)> {
        self.clients
            .iter()
            .filter(|c| **c != from)
            .map(|c| (*c, delivery.clone()))
            .collect()
    }
}
