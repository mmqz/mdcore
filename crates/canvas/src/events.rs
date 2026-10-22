//! 事件/订阅面（集成项③：前端宿主推式通知——交付报告 §3.3）。
//!
//! # 产品语义
//!
//! 与 mdc-collab 事件面**同构**（领域独立纪律：两 crate 零依赖，
//! 形状镜像）：一个公共变更入口恰一个事件；`updates` = 逐事务
//! wire 字节（可独立转发）；失败命令不产事件。
//!
//! # 画布域增量：touched 对象 ID
//!
//! 载体根 Map 的对象 ID = 键 → 深观察事件免费给出受影响对象：
//! - 根自身事件（增删对象键）path 为空 → 变更键在 `MapEvent::keys`；
//! - 嵌套事件（几何/属性/边端点/组成员的深变更）path 首段 = 对象键。
//!
//! `touched` = 收集序去重后**字典序**（确定性读模型纪律）。对比文档
//! 域（XML 载体 block 级提取需属性级事件解释，v1 刻意不提供）——
//! 两域形状不对称是载体语义差异的如实反映。
//!
//! # 派发纪律
//!
//! 与 mdc-collab 同构：延迟派发（事务释放后）/ 回调不可重入会话 /
//! 回调 panic 正常传播 / Drop 退订即时（派发中至多一次尾随调用）。
//! 详见 mdc-collab events 模块文档（此处不重复论证）。

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, Weak};

use yrs::types::{DeepObservable as _, Event, PathSegment};
use yrs::{MapRef, TransactionMut};

use crate::session::Actor;

/// 变更来源（与公共入口一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasChangeSource {
    Command {
        actor: Actor,
    },
    Undo {
        actor: Actor,
    },
    Redo {
        actor: Actor,
    },
    Remote,
    /// 维护窗（逻辑读模型不变；updates 可能为空——事件仍发）。
    Maintenance,
}

/// 画布变更事件。
#[derive(Debug, Clone)]
pub struct CanvasEvent {
    pub source: CanvasChangeSource,
    /// 逐事务 wire update 字节（每条可独立 `apply_update` 转发）。
    pub updates: Vec<Vec<u8>>,
    /// 本次变更触及的对象 ID（字典序去重——读模型确定性纪律）。
    pub touched: Vec<String>,
}

/// 订阅句柄（Drop = 退订）。
pub struct Subscription {
    registry: Weak<Mutex<HashMap<u64, Arc<Mutex<SubscriberFn>>>>>,
    id: u64,
}

type SubscriberFn = Box<dyn FnMut(&CanvasEvent) + Send + 'static>;

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(reg) = self.registry.upgrade() {
            if let Ok(mut map) = reg.lock() {
                map.remove(&self.id);
            }
        }
    }
}

/// 事件中枢（注册表 + 派发——锁序纪律与 mdc-collab 同构）。
#[derive(Default)]
pub(crate) struct EventHub {
    registry: Arc<Mutex<HashMap<u64, Arc<Mutex<SubscriberFn>>>>>,
    next_id: u64,
}

impl EventHub {
    pub(crate) fn subscribe(
        &mut self,
        f: impl FnMut(&CanvasEvent) + Send + 'static,
    ) -> Subscription {
        let id = self.next_id;
        self.next_id += 1;
        let slot = Arc::new(Mutex::new(Box::new(f) as SubscriberFn));
        if let Ok(mut map) = self.registry.lock() {
            map.insert(id, slot);
        }
        Subscription {
            registry: Arc::downgrade(&self.registry),
            id,
        }
    }

    pub(crate) fn dispatch(&self, event: &CanvasEvent) {
        let slots: Vec<Arc<Mutex<SubscriberFn>>> = {
            match self.registry.lock() {
                Ok(map) => map.values().cloned().collect(),
                Err(poisoned) => poisoned.into_inner().values().cloned().collect(),
            }
        };
        for slot in slots {
            if let Ok(mut f) = slot.lock() {
                f(event);
            }
        }
    }
}

/// 事务字节收集器（与 mdc-collab 同构——空更新滤除）。
#[derive(Clone, Default)]
pub(crate) struct UpdateCollector {
    buf: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl UpdateCollector {
    pub(crate) fn attach(&self, doc: &yrs::Doc) {
        let buf = Arc::clone(&self.buf);
        let r = doc.observe_update_v1(COLLECTOR_KEY, move |_txn, e| {
            if !e.update.is_empty() {
                if let Ok(mut b) = buf.lock() {
                    b.push(e.update.clone());
                }
            }
        });
        if r.is_err() {
            panic!("update collector registration failed (transaction held at open?)");
        }
    }

    pub(crate) fn begin(&self) {
        if let Ok(mut b) = self.buf.lock() {
            b.clear();
        }
    }

    pub(crate) fn drain(&self) -> Vec<Vec<u8>> {
        match self.buf.lock() {
            Ok(mut b) => std::mem::take(&mut *b),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }
}

/// 受触对象收集器（根 Map 深观察 → 对象 ID 集）。
#[derive(Clone, Default)]
pub(crate) struct TouchedCollector {
    buf: Arc<Mutex<BTreeSet<String>>>,
}

impl TouchedCollector {
    /// 注册深观察（构造期单次调用——根 Map 自身 + 全部嵌套层）。
    pub(crate) fn attach(&self, root: &MapRef) {
        let buf = Arc::clone(&self.buf);
        root.observe_deep(TOUCHED_KEY, move |txn: &TransactionMut, e| {
            let mut set = match buf.lock() {
                Ok(b) => b,
                Err(poisoned) => poisoned.into_inner(),
            };
            for event in e.iter() {
                let path = event.path();
                if path.is_empty() {
                    // 根 Map 自身事件：变更键 = 对象 ID（MapEvent::keys）。
                    if let Event::Map(me) = event {
                        for key in me.keys(txn).keys() {
                            set.insert(key.to_string());
                        }
                    }
                } else if let Some(PathSegment::Key(k)) = path.front() {
                    // 嵌套事件：路径首段 = 所属对象 ID。
                    set.insert(k.to_string());
                }
            }
        });
    }

    pub(crate) fn begin(&self) {
        if let Ok(mut b) = self.buf.lock() {
            b.clear();
        }
    }

    pub(crate) fn drain(&self) -> Vec<String> {
        match self.buf.lock() {
            Ok(mut b) => std::mem::take(&mut *b).into_iter().collect(),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner())
                .into_iter()
                .collect(),
        }
    }
}

const COLLECTOR_KEY: &str = "mdc:collector";
const TOUCHED_KEY: &str = "mdc:touched";
