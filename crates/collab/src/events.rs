//! 事件/订阅面（集成项③：前端宿主推式通知——交付报告 §3.3）。
//!
//! # 产品语义
//!
//! 会话变更的**推式通知**：此前读模型为拉取式（`document()` /
//! `find_block()` 快照视图），前端宿主无法得知"何时该重读"。本模块
//! 在不破坏隔离不变量（公开 API 零 `yrs::` 类型）的前提下，把 yrs
//! 的事务观察者包装为受控订阅面。
//!
//! # 事件模型（刻意最小）
//!
//! - **一个公共变更入口 = 恰一个事件**：`execute`（含批量装载的
//!   多小批事务）/ `undo` / `redo` / `apply_update`（远端）/
//!   `maintenance_gc` 各产一个事件；**失败命令不产事件**（错误经
//!   返回值显式传递，宿主在错误路径自行重读——双通道不重复）。
//! - `updates` = 本次变更内**逐事务**的 wire update 字节（v1 编码，
//!   与 `apply_update` 对称——宿主可逐条转发对端，不必重编码）。
//!   批量装载 = 多条（512/事务小批路径）；单命令/单事务 = 一条。
//! - **touched 面刻意不提供**（v1）：XML 载体的 block 级提取需
//!   属性级事件解释（嵌套 mark 元素/重建换族），复杂且脆——诚实
//!   登记延后（画布域 Map 键免费获得 touched，见 mdc-canvas 事件
//!   面；两域形状不对称是载体语义差异的如实反映）。
//!
//! # 派发纪律（结构性安全）
//!
//! - **延迟派发**：观察者回调只在事务提交期收集字节（不触引擎
//!   读面）；订阅者派发发生在公共方法返回前、**事务已释放后**——
//!   订阅者回调内读会话状态不与事务锁交叉（p18 读写同锁纪律）。
//! - **回调不可重入会话**：回调只收 `&CollabEvent`，不持会话引用
//!   （借用检查器结构性阻止同步回调会话方法——UI 宿主标准
//!   dirty-标记模式：回调记脏，下一帧重读）。
//! - **回调 panic 正常传播**（不隔离）：订阅者是宿主代码而非引擎
//!   代码——引擎 panic 隔离纪律（`apply_update` 面）不延伸到宿主
//!   回调；fail-fast 优于吞错。
//! - **退订即时性**：`Subscription` Drop 即从注册表移除；已在派发
//!   中的回调可能收到**至多一次**尾随调用（Arc 已克隆——文档化
//!   语义，非缺陷）。
//!
//! # 恢复语义
//!
//! `restore` 产出的新会话无订阅者（注册表随会话实例新生）——恢复
//! 事务不派发；宿主在恢复点自行重读。撤销栈不随快照走（B3.1），
//! 事件面与之同语义：恢复后第一个事件 = 恢复后的首个新变更。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use crate::session::Actor;

/// 变更来源（事件与公共入口一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    /// 本地命令（`execute`——人类或 AI）。
    Command { actor: Actor },
    /// 本地撤销（`undo`——栈本地，B3.2 origin 分栈）。
    Undo { actor: Actor },
    /// 本地重做（`redo`）。
    Redo { actor: Actor },
    /// 远端 update 应用（`apply_update`——含 restore 内部应用，
    /// 但恢复会话无订阅者，实际不可见）。
    Remote,
    /// 受控压缩（`maintenance_gc`——逻辑读模型不变，wire 形态
    /// 变化；`updates` 可能为空[GC 不产新操作]，事件仍发——
    /// 维护窗显式通知面）。
    Maintenance,
}

/// 会话变更事件。
#[derive(Debug, Clone)]
pub struct CollabEvent {
    pub source: ChangeSource,
    /// 逐事务 wire update 字节（每条可独立 `apply_update` 转发）。
    /// 空序 = 无 CRDT 操作产出的变更面（仅 `Maintenance` 可能）。
    pub updates: Vec<Vec<u8>>,
}

/// 订阅句柄（Drop = 退订；不可跨线程 Send——与会话同线程纪律）。
pub struct Subscription {
    registry: Weak<Mutex<HashMap<u64, Arc<Mutex<SubscriberFn>>>>>,
    id: u64,
}

type SubscriberFn = Box<dyn FnMut(&CollabEvent) + Send + 'static>;

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(reg) = self.registry.upgrade() {
            if let Ok(mut map) = reg.lock() {
                map.remove(&self.id);
            }
        }
    }
}

/// 事件中枢（订阅注册表 + 派发）。
#[derive(Default)]
pub(crate) struct EventHub {
    registry: Arc<Mutex<HashMap<u64, Arc<Mutex<SubscriberFn>>>>>,
    next_id: u64,
}

impl EventHub {
    pub(crate) fn subscribe(
        &mut self,
        f: impl FnMut(&CollabEvent) + Send + 'static,
    ) -> Subscription {
        let id = self.next_id;
        self.next_id += 1;
        let slot = Arc::new(Mutex::new(Box::new(f) as SubscriberFn));
        // 注册表面锁只在此处短暂持有（派发期不持有——见 dispatch）。
        if let Ok(mut map) = self.registry.lock() {
            map.insert(id, slot);
        }
        Subscription {
            registry: Arc::downgrade(&self.registry),
            id,
        }
    }

    /// 派发（事务已全部释放后调用）。锁序：注册表锁 → 释放 →
    /// 逐订阅者自身锁——注册表锁不在派发期持有，订阅/退订在回调
    /// 内执行不自杀锁（订阅者锁仅在自身调用期持有）。
    pub(crate) fn dispatch(&self, event: &CollabEvent) {
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

/// 事务字节收集器（观察者回调 → 缓冲 → 公共方法返回前取走）。
///
/// 观察者回调在**事务提交期**执行（引擎内部）——只做字节入缓冲，
/// 不触任何读面（p18 读写同锁纪律的观察者面扩展）。
#[derive(Clone, Default)]
pub(crate) struct UpdateCollector {
    buf: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl UpdateCollector {
    /// 注册到 Doc（构造期单次调用；key 与 actor origin 域隔离）。
    ///
    /// 空更新过滤：`authoritative_snapshot` 等只读封装内部的写事务
    /// 不产操作——yrs 仍可能以空 update 触发观察者，此处滤除
    /// （事件语义 = 有产出的变更）。
    pub(crate) fn attach(&self, doc: &yrs::Doc) {
        let buf = Arc::clone(&self.buf);
        let r = doc.observe_update_v1(COLLECTOR_KEY, move |_txn, e| {
            if !e.update.is_empty() {
                if let Ok(mut b) = buf.lock() {
                    b.push(e.update.clone());
                }
            }
        });
        // 构造期无并发事务，注册失败 = 引擎状态异常（fail-fast）。
        if r.is_err() {
            panic!("update collector registration failed (transaction held at open?)");
        }
    }

    /// 开始收集（公共变更入口首行调用——丢弃上次残留）。
    pub(crate) fn begin(&self) {
        if let Ok(mut b) = self.buf.lock() {
            b.clear();
        }
    }

    /// 取走（公共方法返回前调用）。
    pub(crate) fn drain(&self) -> Vec<Vec<u8>> {
        match self.buf.lock() {
            Ok(mut b) => std::mem::take(&mut *b),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }
}

/// 收集器观察者 key（与 `mdc:human`/`mdc:ai` origin 域不冲突的
/// 私有常量——不进 wire 语义面）。
const COLLECTOR_KEY: &str = "mdc:collector";
