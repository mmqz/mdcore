//! awareness/在线状态面（集成项④：在线状态/光标——交付报告 §3.4）。
//!
//! # 产品语义
//!
//! 协同编辑的**瞬态在场层**：每客户端一份 JSON 状态（光标/选区/
//! 在线标记/姓名颜色等），独立于文档内容流走自己的 wire 通道
//! （`encode_update`/`apply_update` 对称面），不持久化、不进撤销
//! （yjs 生态语义：断线前广播 null 状态 = 离线标记）。
//!
//! # 归属与消费路径（领域独立纪律的反向应用）
//!
//! 在场层是**会话层协同基础设施**而非领域逻辑——单点落位于
//! mdc-collab（协作引擎的章程范围），画布宿主**不重复镜像**：
//! `Presence::standalone(client_id)` 以相同 client_id 独立构造
//! （宿主保证 doc 流与 presence 流的 client_id 一致——身份关联
//! 纪律文档化）。
//!
//! # 安全与纪律
//!
//! - **本地状态 fail-closed**：`set_local_state` 的 JSON 必须可解析
//!   （本端永不发射垃圾）；**对端状态不校验**（yjs 生态兼容面——
//!   恶意对端可注入任意字符串，本 crate 零解析即零 panic 面；
//!   `states()` 消费方解析须防御性——文档化边界）。
//! - **入向限额 + panic 隔离**：`apply_update` 字节门（默认 1MiB
//!   ——在场 JSON 为小载荷，远小于文档 52MiB 族）+ catch_unwind
//!   （B1.6 同构纪律）。
//! - **client_id 53 位域**：standalone 构造显式校验（与
//!   SessionOptions 同规则——yjs 兼容域）。
//! - **确定性读模型**：`states()` 按 client_id 排序。
//!
//! # 事件面
//!
//! `subscribe` 镜像事件层订阅纪律（Drop 退订/延迟语义不适用——
//! awareness 回调本身在状态变更点同步触发，无事务锁交叉：awareness
//! 状态在 DashMap 中，不经 doc 事务）。事件三分类：
//! `added`（新客户端）/ `updated`（时钟推进——含同 JSON 保活刷新）
//! / `removed`（null 状态 = 离线）。需要"实际 JSON 变化"过滤的
//! 消费方自行比对（yrs 摘要面 updated = 全部时钟推进）。

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, Weak};

use yrs::sync::awareness::{Awareness, AwarenessUpdate};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ClientID, Doc, Options as DocOptions};

use crate::session::CollabError;

/// 在场状态条目（读模型——快照式）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceState {
    pub client_id: u64,
    /// 状态时钟（每次更新 +1；接收方按 clock 新旧取舍）。
    pub clock: u32,
    /// 最近更新时间（UNIX 毫秒——陈旧判定由消费方按策略执行）。
    pub last_updated_ms: u64,
    /// JSON 状态（`None` = 已清除——离线语义）。
    pub json: Option<String>,
}

/// 在场变更事件（added/updated/removed = 客户端 ID 集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceEvent {
    pub added: Vec<u64>,
    pub updated: Vec<u64>,
    pub removed: Vec<u64>,
}

/// 在场订阅句柄（Drop = 退订——与事件层同语义）。
pub struct PresenceSubscription {
    registry: Weak<Mutex<HashMap<u64, Arc<Mutex<PresenceFn>>>>>,
    id: u64,
}

type PresenceFn = Box<dyn FnMut(&PresenceEvent) + Send + 'static>;

impl Drop for PresenceSubscription {
    fn drop(&mut self) {
        if let Some(reg) = self.registry.upgrade() {
            if let Ok(mut map) = reg.lock() {
                map.remove(&self.id);
            }
        }
    }
}

/// 在场层（yrs Awareness 受控包装——公开面零 yrs 类型）。
pub struct Presence {
    inner: Awareness,
    input_byte_limit: usize,
    hub: PresenceHub,
}

#[derive(Default)]
struct PresenceHub {
    registry: Arc<Mutex<HashMap<u64, Arc<Mutex<PresenceFn>>>>>,
    next_id: u64,
}

impl PresenceHub {
    fn subscribe(
        &mut self,
        f: impl FnMut(&PresenceEvent) + Send + 'static,
    ) -> PresenceSubscription {
        let id = self.next_id;
        self.next_id += 1;
        let slot = Arc::new(Mutex::new(Box::new(f) as PresenceFn));
        if let Ok(mut map) = self.registry.lock() {
            map.insert(id, slot);
        }
        PresenceSubscription {
            registry: Arc::downgrade(&self.registry),
            id,
        }
    }
}

/// awareness 中枢回调 key（私有常量）。
const HUB_KEY: &str = "mdc:presence-hub";

impl Presence {
    /// 会话绑定构造（client_id 与会话对齐——Doc 克隆共享底层态）。
    pub(crate) fn bound_to_session(doc: &Doc, input_byte_limit: usize) -> Presence {
        Self::build(Self::awareness(doc.clone()), input_byte_limit)
    }

    /// 独立构造（画布宿主等无 CollabSession 场景）。client_id 必须
    /// 与该宿主的各协同会话一致（doc 流与 presence 流的身份关联）。
    pub fn standalone(client_id: u64, input_byte_limit: usize) -> Result<Presence, CollabError> {
        if client_id == 0 || client_id & (u64::MAX << 53) != 0 {
            return Err(CollabError::InvalidInput(format!(
                "client_id {} 超出 (0, 2^53) 域（yjs 兼容）",
                client_id
            )));
        }
        // 私有载体 Doc：仅承载 client_id（awareness 状态不写入 doc——
        // DashMap 内部态）；skip_gc 对空 doc 无行为面，保持全编队一致。
        let mut opts = DocOptions::with_client_id(ClientID::new(client_id));
        opts.skip_gc = true;
        let carrier = Doc::with_options(opts);
        Ok(Self::build(Self::awareness(carrier), input_byte_limit))
    }

    /// Awareness 构造单点：`with_clock` + SystemTime 回退 0 时钟
    /// （`Awareness::new`[SystemClock] 在 wasm32-unknown 被上游
    /// cfg 排除；本 crate 与撤销管理器 timestamp 同款时钟模式——
    /// 时间源不可用时恒 0，排序仍由 clock 字段保证）。
    fn awareness(doc: Doc) -> Awareness {
        Awareness::with_clock(doc, || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        })
    }

    fn build(inner: Awareness, input_byte_limit: usize) -> Presence {
        let mut presence = Presence {
            inner,
            input_byte_limit,
            hub: PresenceHub::default(),
        };
        // 中枢接线：yrs on_update → PresenceEvent → 订阅者派发。
        // 回调只触 hub 注册表（不持 self 借用——无重入面）。
        let registry = Arc::clone(&presence.hub.registry);
        presence.inner.on_update(HUB_KEY, move |_a, e, _origin| {
            let s = e.summary();
            let event = PresenceEvent {
                added: s.added.iter().map(|c| c.get()).collect(),
                updated: s.updated.iter().map(|c| c.get()).collect(),
                removed: s.removed.iter().map(|c| c.get()).collect(),
            };
            let slots: Vec<Arc<Mutex<PresenceFn>>> = {
                match registry.lock() {
                    Ok(map) => map.values().cloned().collect(),
                    Err(poisoned) => poisoned.into_inner().values().cloned().collect(),
                }
            };
            for slot in slots {
                if let Ok(mut f) = slot.lock() {
                    f(&event);
                }
            }
        });
        presence
    }

    /// 本客户端 ID。
    pub fn client_id(&self) -> u64 {
        self.inner.client_id().get()
    }

    /// 设置本地状态（JSON 必须可解析——fail-closed；时钟 +1 并触发
    /// 事件）。
    pub fn set_local_state(&mut self, json: &str) -> Result<(), CollabError> {
        // serde_json 已在依赖闭包内（yrs 传递依赖）——本 crate 显式
        // 声明消费（Cargo.toml）。
        if serde_json::from_str::<serde_json::Value>(json).is_err() {
            return Err(CollabError::InvalidInput(
                "presence state must be valid JSON".into(),
            ));
        }
        self.inner.set_local_state_raw(json);
        Ok(())
    }

    /// 本地状态读回（None = 未设置或已清除）。
    pub fn local_state(&self) -> Option<String> {
        self.inner.local_state_raw().map(|s| s.to_string())
    }

    /// 清除本地状态（离线语义：null 状态 + clock+1 广播——断线前
    /// 调用使对端及时收敛）。
    pub fn clear_local_state(&mut self) {
        self.inner.clean_local_state();
    }

    /// 全部客户端状态（client_id 升序——确定性读模型）。
    pub fn states(&self) -> Vec<PresenceState> {
        let mut out: Vec<PresenceState> = self
            .inner
            .iter()
            .map(|(client_id, state)| PresenceState {
                client_id: client_id.get(),
                clock: state.clock,
                last_updated_ms: state.last_updated,
                json: state.data.as_ref().map(|s| s.to_string()),
            })
            .collect();
        out.sort_by_key(|s| s.client_id);
        out
    }

    /// 全量 wire 编码（v1）。**含已清除（null）条目**——离线语义的
    /// 传播面：晚到对端需 null+clock 才能退休旧态（yrs `update()`
    /// 刻意排除 null，故走 `update_with_clients` 全键集；null 条目
    /// 保留使条目数随历史客户端有界增长——登记）。
    pub fn encode_update(&self) -> Result<Vec<u8>, CollabError> {
        let clients: Vec<ClientID> = self.inner.iter().map(|(c, _)| c).collect();
        let update = self
            .inner
            .update_with_clients(clients)
            .map_err(|e| CollabError::InvalidInput(e.to_string()))?;
        Ok(update.encode_v1())
    }

    /// 定向 wire 编码（仅指定客户端——须全部已知，否则 InvalidInput）。
    pub fn encode_update_for(&self, clients: &[u64]) -> Result<Vec<u8>, CollabError> {
        let ids: Vec<ClientID> = clients.iter().map(|c| ClientID::new(*c)).collect();
        let update = self
            .inner
            .update_with_clients(ids)
            .map_err(|e| CollabError::InvalidInput(e.to_string()))?;
        Ok(update.encode_v1())
    }

    /// 应用远端在场 update（入向限额 + panic 隔离；clock 旧的条目
    /// 自动忽略——yjs 语义）。
    pub fn apply_update(&mut self, bytes: &[u8]) -> Result<(), CollabError> {
        if bytes.len() > self.input_byte_limit {
            return Err(CollabError::Limit {
                size: bytes.len(),
                limit: self.input_byte_limit,
            });
        }
        let inner = &mut self.inner;
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let update = AwarenessUpdate::decode_v1(bytes)
                .map_err(|e| CollabError::Decode(e.to_string()))?;
            inner
                .apply_update(update)
                .map_err(|e| CollabError::Decode(e.to_string()))
        }));
        match r {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(p) => Err(CollabError::Panic(crate::session::panic_message_pub(&p))),
        }
    }

    /// 订阅在场变更（Drop 句柄即退订）。
    pub fn subscribe(
        &mut self,
        f: impl FnMut(&PresenceEvent) + Send + 'static,
    ) -> PresenceSubscription {
        self.hub.subscribe(f)
    }
}
