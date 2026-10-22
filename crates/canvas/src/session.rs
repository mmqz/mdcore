//! Canvas 会话：领域模型 + 命令接口 + CRDT 载体（根 Map "canvas"）。
//!
//! 不变量集与 mdc-collab 同源同构（见两 crate Cargo.toml 注释）：
//! skip_gc 无条件 / 捕获窗 0 / client_id 53 位域 / 入向全限额 +
//! panic 隔离 / 双撤销栈 origin 分栈 + capture_transaction 拒
//! None-origin（远端应用不入本地栈）/ 撤销栈软上限粗粒度策略。

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::time::{SystemTime, UNIX_EPOCH};

use mdc_evidence::Fingerprinter as _;
use yrs::undo::Options as UndoOptions;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{
    Any, ClientID, Doc, In, Map, MapPrelim, Number, Options as DocOptions, Origin, Out, ReadTxn,
    StateVector, Transact, UndoManager, Update,
};

use crate::events::{CanvasChangeSource, CanvasEvent, EventHub, TouchedCollector, UpdateCollector};

/// 载体根名。
const ROOT: &str = "canvas";
/// 载体字段名（私有常量——载体形状是本 crate 私事）。
const F_KIND: &str = "kind";
const F_X: &str = "x";
const F_Y: &str = "y";
const F_W: &str = "w";
const F_H: &str = "h";
const F_FROM: &str = "from";
const F_TO: &str = "to";
const F_MEMBERS: &str = "members";
const F_PROPS: &str = "props";

/// 画布对象种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Node,
    Edge,
    Group,
}

impl ObjectKind {
    fn wire(self) -> &'static str {
        match self {
            ObjectKind::Node => "node",
            ObjectKind::Edge => "edge",
            ObjectKind::Group => "group",
        }
    }

    fn from_wire(s: &str) -> Option<ObjectKind> {
        match s {
            "node" => Some(ObjectKind::Node),
            "edge" => Some(ObjectKind::Edge),
            "group" => Some(ObjectKind::Group),
            _ => None,
        }
    }
}

/// 画布对象读模型（快照式视图）。
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasObject {
    pub id: String,
    pub kind: ObjectKind,
    /// Node/Group 几何（左上角 + 尺寸；Edge 恒 0）。
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Edge 端点（节点 ID；非 Edge = None）。
    pub from: Option<String>,
    pub to: Option<String>,
    /// Group 成员（对象 ID 集；非 Group = 空）。
    pub members: Vec<String>,
    /// 开放属性（label/color/…；字符串值面）。
    pub props: BTreeMap<String, String>,
}

/// 画布域错误（公开面零 yrs 类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasError {
    Limit {
        size: usize,
        limit: usize,
    },
    Decode(String),
    Panic(String),
    InvalidInput(String),
    ObjectNotFound {
        object_id: String,
    },
    DuplicateId {
        object_id: String,
    },
    /// 边端点不存在（创建时校验——并发端点删除留下的悬空边在
    /// 读模型暴露 from/to，消费侧可检测）。
    EndpointNotFound {
        object_id: String,
    },
    /// 成员不存在（分组时校验）。
    MemberNotFound {
        object_id: String,
    },
    /// 几何非有限值（NaN/±inf 拒收——敌意输入面）。
    NonFiniteGeometry {
        field: &'static str,
    },
    KindMismatch {
        object_id: String,
        op: &'static str,
    },
}

impl std::fmt::Display for CanvasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanvasError::Limit { size, limit } => {
                write!(f, "input {size}B exceeds limit {limit}B")
            }
            CanvasError::Decode(m) => write!(f, "wire decode failed: {m}"),
            CanvasError::Panic(m) => write!(f, "engine panic isolated: {m}"),
            CanvasError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            CanvasError::ObjectNotFound { object_id } => {
                write!(f, "canvas object not found: {object_id}")
            }
            CanvasError::DuplicateId { object_id } => {
                write!(f, "duplicate object id: {object_id}")
            }
            CanvasError::EndpointNotFound { object_id } => {
                write!(f, "edge endpoint not found: {object_id}")
            }
            CanvasError::MemberNotFound { object_id } => {
                write!(f, "group member not found: {object_id}")
            }
            CanvasError::NonFiniteGeometry { field } => {
                write!(f, "geometry field {field} is not finite (NaN/inf rejected)")
            }
            CanvasError::KindMismatch { object_id, op } => {
                write!(f, "object {object_id} kind does not support {op}")
            }
        }
    }
}

impl std::error::Error for CanvasError {}

/// 编辑来源（与 mdc-collab::Actor 同构——分栈 + origin 溯源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    Human,
    Ai,
}

impl Actor {
    fn wire(self) -> Origin {
        match self {
            Actor::Human => Origin::from("mdc:human"),
            Actor::Ai => Origin::from("mdc:ai"),
        }
    }
}

/// 会话选项（与 mdc-collab::SessionOptions 同构纪律：不含 skip_gc、
/// 不含捕获窗）。
#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub client_id: u64,
    pub input_byte_limit: usize,
    /// 撤销栈软上限（粗粒度 clear_all 策略——同 mdc-collab）。
    pub undo_stack_limit: Option<usize>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        SessionOptions {
            client_id: 7_200_001,
            input_byte_limit: 52 * 1024 * 1024,
            undo_stack_limit: None,
        }
    }
}

/// 画布命令全集（人类/AI 共用——受控集，无任意路径/脚本面）。
#[derive(Debug, Clone)]
pub enum CanvasCommand {
    AddNode {
        id: Option<String>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        props: BTreeMap<String, String>,
    },
    /// 位置更新（键级 LWW——高频拖拽的自然语义）。
    MoveNode {
        id: String,
        x: f64,
        y: f64,
    },
    ResizeNode {
        id: String,
        w: f64,
        h: f64,
    },
    AddEdge {
        id: Option<String>,
        from: String,
        to: String,
        props: BTreeMap<String, String>,
    },
    /// 属性终态替换（与文档域 SetAttrs 同语义）。
    SetProps {
        id: String,
        props: BTreeMap<String, String>,
    },
    RemoveObject {
        id: String,
    },
    GroupObjects {
        group_id: Option<String>,
        members: Vec<String>,
    },
    /// 解组（删除分组对象——成员对象不动）。
    Ungroup {
        group_id: String,
    },
}

/// 命令回执。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReceipt {
    pub outcome: CommandOutcome,
    pub undo_depth: usize,
    pub undo_cleared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    NodeAdded { id: String },
    NodeMoved { id: String },
    NodeResized { id: String },
    EdgeAdded { id: String },
    PropsSet { id: String },
    ObjectRemoved { id: String },
    Grouped { group_id: String },
    Ungrouped { group_id: String },
}

/// Canvas 会话。
pub struct CanvasSession {
    doc: Doc,
    undo_human: UndoManager,
    undo_ai: UndoManager,
    options: SessionOptions,
    next_seq: u64,
    undo_cleared_last: bool,
    /// 事件订阅面（集成项③——见 events 模块）。
    pub(crate) hub: EventHub,
    /// 事务字节收集器。
    pub(crate) collector: UpdateCollector,
    /// 受触对象收集器（根 Map 深观察）。
    pub(crate) touched: TouchedCollector,
}

impl CanvasSession {
    /// 构造面唯一入口（不变量集见模块文档）。
    pub fn open(options: SessionOptions) -> Result<CanvasSession, CanvasError> {
        if options.client_id == 0 || options.client_id & (u64::MAX << 53) != 0 {
            return Err(CanvasError::InvalidInput(format!(
                "client_id {} 超出 (0, 2^53) 域（yjs 兼容）",
                options.client_id
            )));
        }
        let mut opts = DocOptions::with_client_id(ClientID::new(options.client_id));
        opts.skip_gc = true; // B1.1 无条件
        let doc = Doc::with_options(opts);
        let root = doc.get_or_insert_map(ROOT);
        let _ = root.len(&doc.transact());

        let mut undo_human = Self::new_undo_manager(Actor::Human);
        let mut undo_ai = Self::new_undo_manager(Actor::Ai);
        undo_human.expand_scope(&doc, &doc.get_or_insert_map(ROOT));
        undo_ai.expand_scope(&doc, &doc.get_or_insert_map(ROOT));

        // 事件面接线（集成项③）：doc 级 update 观察者 + 根 Map 深观察。
        let collector = UpdateCollector::default();
        collector.attach(&doc);
        let touched = TouchedCollector::default();
        touched.attach(&root);

        Ok(CanvasSession {
            doc,
            undo_human,
            undo_ai,
            options,
            next_seq: 0,
            undo_cleared_last: false,
            hub: EventHub::default(),
            collector,
            touched,
        })
    }

    fn new_undo_manager(origin: Actor) -> UndoManager {
        // 与 mdc-collab 同构：每 origin 一栈 + 窗 0 + capture_transaction
        // 拒 None-origin（远端应用不入本地栈）。
        let mut tracked = HashSet::new();
        tracked.insert(origin.wire());
        UndoManager::with_options(UndoOptions {
            capture_timeout_millis: 0,
            tracked_origins: tracked,
            capture_transaction: Some(std::sync::Arc::new(|txn| txn.origin().is_some())),
            timestamp: std::sync::Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            }),
            init_undo_stack: vec![],
            init_redo_stack: vec![],
        })
    }

    /// 故障恢复：权威快照重建（撤销栈不随快照走）。
    pub fn restore(options: SessionOptions, snapshot: &[u8]) -> Result<CanvasSession, CanvasError> {
        let s = CanvasSession::open(options)?;
        s.apply_update(snapshot)?;
        Ok(s)
    }

    pub fn options(&self) -> &SessionOptions {
        &self.options
    }

    fn check_inbound(&self, size: usize) -> Result<(), CanvasError> {
        if size > self.options.input_byte_limit {
            return Err(CanvasError::Limit {
                size,
                limit: self.options.input_byte_limit,
            });
        }
        Ok(())
    }

    fn next_id(&mut self) -> String {
        self.next_seq += 1;
        format!("{}-{}", self.options.client_id, self.next_seq)
    }

    fn root(&self) -> yrs::MapRef {
        self.doc.get_or_insert_map(ROOT)
    }

    // ── 撤销面 ─────────────────────────────────────────────────────

    pub fn undo(&mut self, actor: Actor) -> bool {
        self.collector.begin();
        self.touched.begin();
        let r = match actor {
            Actor::Human => self.undo_human.undo_blocking(),
            Actor::Ai => self.undo_ai.undo_blocking(),
        };
        let updates = self.collector.drain();
        let touched = self.touched.drain();
        self.enforce_undo_limit(actor);
        if r && !updates.is_empty() {
            self.hub.dispatch(&CanvasEvent {
                source: CanvasChangeSource::Undo { actor },
                updates,
                touched,
            });
        }
        r
    }

    pub fn redo(&mut self, actor: Actor) -> bool {
        self.collector.begin();
        self.touched.begin();
        let r = match actor {
            Actor::Human => self.undo_human.redo_blocking(),
            Actor::Ai => self.undo_ai.redo_blocking(),
        };
        let updates = self.collector.drain();
        let touched = self.touched.drain();
        if r && !updates.is_empty() {
            self.hub.dispatch(&CanvasEvent {
                source: CanvasChangeSource::Redo { actor },
                updates,
                touched,
            });
        }
        r
    }

    // ── 事件面（集成项③）────────────────────────────────

    /// 订阅变更事件（Drop 句柄即退订；派发纪律见 events 模块）。
    pub fn subscribe(
        &mut self,
        f: impl FnMut(&CanvasEvent) + Send + 'static,
    ) -> crate::events::Subscription {
        self.hub.subscribe(f)
    }

    pub fn undo_depth(&self, actor: Actor) -> usize {
        match actor {
            Actor::Human => self.undo_human.undo_stack().len(),
            Actor::Ai => self.undo_ai.undo_stack().len(),
        }
    }

    pub fn redo_depth(&self, actor: Actor) -> usize {
        match actor {
            Actor::Human => self.undo_human.redo_stack().len(),
            Actor::Ai => self.undo_ai.redo_stack().len(),
        }
    }

    fn enforce_undo_limit(&mut self, actor: Actor) {
        self.undo_cleared_last = false;
        if let Some(limit) = self.options.undo_stack_limit {
            let over = match actor {
                Actor::Human => self.undo_human.undo_stack().len() > limit,
                Actor::Ai => self.undo_ai.undo_stack().len() > limit,
            };
            if over {
                match actor {
                    Actor::Human => self.undo_human.clear_all(),
                    Actor::Ai => self.undo_ai.clear_all(),
                }
                self.undo_cleared_last = true;
            }
        }
    }

    fn take_undo_cleared(&mut self) -> bool {
        let v = self.undo_cleared_last;
        self.undo_cleared_last = false;
        v
    }

    // ── 同步/快照/维护面（与 mdc-collab 同构）──────────────────────

    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    pub fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, CanvasError> {
        self.check_inbound(remote_sv.len())?;
        let sv =
            StateVector::decode_v1(remote_sv).map_err(|e| CanvasError::Decode(e.to_string()))?;
        Ok(self.doc.transact().encode_diff_v1(&sv))
    }

    pub fn apply_update(&self, bytes: &[u8]) -> Result<(), CanvasError> {
        self.check_inbound(bytes.len())?;
        self.collector.begin();
        self.touched.begin();
        let doc = &self.doc;
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let update =
                Update::decode_v1(bytes).map_err(|e| CanvasError::Decode(e.to_string()))?;
            let mut txn = doc.transact_mut();
            txn.apply_update(update)
                .map_err(|e| CanvasError::Decode(e.to_string()))?;
            txn.commit();
            Ok(())
        }));
        match r {
            Ok(Ok(())) => {
                let updates = self.collector.drain();
                let touched = self.touched.drain();
                if !updates.is_empty() {
                    self.hub.dispatch(&CanvasEvent {
                        source: CanvasChangeSource::Remote,
                        updates,
                        touched,
                    });
                }
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = self.collector.drain();
                let _ = self.touched.drain();
                Err(e)
            }
            Err(p) => {
                let _ = self.collector.drain();
                let _ = self.touched.drain();
                Err(CanvasError::Panic(panic_message(&p)))
            }
        }
    }

    pub fn authoritative_snapshot(&self) -> Result<Vec<u8>, CanvasError> {
        let doc = &self.doc;
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let snap = doc.transact_mut().snapshot();
            let mut enc = EncoderV1::new();
            doc.transact_mut()
                .encode_state_from_snapshot(&snap, &mut enc)
                .map_err(|e| CanvasError::InvalidInput(e.to_string()))?;
            Ok(enc.to_vec())
        }))
        .unwrap_or_else(|p| Err(CanvasError::Panic(panic_message(&p))))
    }

    /// 受控压缩（清栈 + 显式 gc——同 mdc-collab 语义与代价）。
    /// 维护窗事件恒发（updates 可能为空——逻辑读模型不变）。
    pub fn maintenance_gc(&mut self) -> Result<(usize, usize), CanvasError> {
        let before = self.full_update_len();
        self.undo_human.clear_all();
        self.undo_ai.clear_all();
        self.collector.begin();
        self.touched.begin();
        let doc = &self.doc;
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut txn = doc.transact_mut();
            txn.gc(None);
            txn.commit();
        }))
        .map_err(|p| CanvasError::Panic(panic_message(&p)))?;
        let updates = self.collector.drain();
        let touched = self.touched.drain();
        self.hub.dispatch(&CanvasEvent {
            source: CanvasChangeSource::Maintenance,
            updates,
            touched,
        });
        Ok((before, self.full_update_len()))
    }

    pub fn full_update_len(&self) -> usize {
        self.doc
            .transact()
            .encode_diff_v1(&StateVector::default())
            .len()
    }

    // ── 读模型 ─────────────────────────────────────────────────────

    /// 全量对象（确定性序：按 ID 排序——读模型稳定面）。
    pub fn objects(&self) -> Vec<CanvasObject> {
        let root = self.root();
        let txn = self.doc.transact();
        let mut ids: Vec<String> = root.keys(&txn).map(|k| k.to_string()).collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|id| read_object(&root, &txn, &id))
            .collect()
    }

    pub fn find_object(&self, id: &str) -> Option<CanvasObject> {
        let root = self.root();
        let txn = self.doc.transact();
        read_object(&root, &txn, id)
    }

    /// 对象语义指纹（证据关联面）：语义字段确定性序列化 → FNV-1a 64。
    pub fn object_fingerprint(&self, id: &str) -> Result<String, CanvasError> {
        self.check_inbound(id.len())?;
        let obj = self
            .find_object(id)
            .ok_or_else(|| CanvasError::ObjectNotFound {
                object_id: id.to_string(),
            })?;
        Ok(fingerprint_object(&obj))
    }

    /// 全画布指纹（整体状态指纹——画布作为受证文档的 DocumentRef 面）。
    pub fn canvas_fingerprint(&self) -> String {
        let objs = self.objects();
        let mut acc = String::new();
        for o in &objs {
            acc.push_str(&fingerprint_object(o));
            acc.push('\n');
        }
        mdc_evidence::Fnv64.fingerprint(acc.as_bytes()).0
    }

    // ── 命令执行 ───────────────────────────────────────────────────

    /// 受控命令执行（人类与 AI 同一入口）。成功且产出 CRDT 字节的命令
    /// 恰派发一个 [`crate::events::CanvasChangeSource::Command`] 事件
    /// （touched = 受触对象 ID 字典序去重）。
    pub fn execute(
        &mut self,
        actor: Actor,
        cmd: CanvasCommand,
    ) -> Result<CommandReceipt, CanvasError> {
        self.collector.begin();
        self.touched.begin();
        let result = self.execute_inner(actor, cmd);
        let updates = self.collector.drain();
        let touched = self.touched.drain();
        match result {
            Ok(receipt) => {
                if !updates.is_empty() {
                    self.hub.dispatch(&CanvasEvent {
                        source: CanvasChangeSource::Command { actor },
                        updates,
                        touched,
                    });
                }
                Ok(receipt)
            }
            // 失败不产事件（错误经返回值显式传递）。
            Err(e) => Err(e),
        }
    }

    /// 命令实施体（事件包裹的内层——`?` 早退不绕过收集器清理）。
    fn execute_inner(
        &mut self,
        actor: Actor,
        cmd: CanvasCommand,
    ) -> Result<CommandReceipt, CanvasError> {
        let outcome = match cmd {
            CanvasCommand::AddNode {
                id,
                x,
                y,
                w,
                h,
                props,
            } => {
                let oid = self.add_node(actor, id, x, y, w, h, &props)?;
                CommandOutcome::NodeAdded { id: oid }
            }
            CanvasCommand::MoveNode { id, x, y } => {
                self.move_node(actor, &id, x, y)?;
                CommandOutcome::NodeMoved { id }
            }
            CanvasCommand::ResizeNode { id, w, h } => {
                self.resize_node(actor, &id, w, h)?;
                CommandOutcome::NodeResized { id }
            }
            CanvasCommand::AddEdge {
                id,
                from,
                to,
                props,
            } => {
                let oid = self.add_edge(actor, id, &from, &to, &props)?;
                CommandOutcome::EdgeAdded { id: oid }
            }
            CanvasCommand::SetProps { id, props } => {
                self.set_props(actor, &id, &props)?;
                CommandOutcome::PropsSet { id }
            }
            CanvasCommand::RemoveObject { id } => {
                self.remove_object(actor, &id)?;
                CommandOutcome::ObjectRemoved { id }
            }
            CanvasCommand::GroupObjects { group_id, members } => {
                let gid = self.group_objects(actor, group_id, &members)?;
                CommandOutcome::Grouped { group_id: gid }
            }
            CanvasCommand::Ungroup { group_id } => {
                self.ungroup(actor, &group_id)?;
                CommandOutcome::Ungrouped { group_id }
            }
        };
        Ok(CommandReceipt {
            outcome,
            undo_depth: self.undo_depth(actor),
            undo_cleared: self.take_undo_cleared(),
        })
    }

    fn check_finite(v: f64, field: &'static str) -> Result<f64, CanvasError> {
        if v.is_finite() {
            Ok(v)
        } else {
            Err(CanvasError::NonFiniteGeometry { field })
        }
    }

    fn ensure_absent(&self, id: &str) -> Result<(), CanvasError> {
        if self.find_object(id).is_some() {
            return Err(CanvasError::DuplicateId {
                object_id: id.to_string(),
            });
        }
        Ok(())
    }

    fn props_prelim(props: &BTreeMap<String, String>) -> MapPrelim {
        props
            .iter()
            .map(|(k, v)| (k.as_str(), In::Any(Any::String(v.as_str().into()))))
            .collect()
    }

    #[allow(clippy::too_many_arguments)] // 命令域字段直传——结构化入参组（id/几何/属性）属领域形状
    fn add_node(
        &mut self,
        actor: Actor,
        id: Option<String>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        props: &BTreeMap<String, String>,
    ) -> Result<String, CanvasError> {
        #[allow(clippy::too_many_arguments)]
        {}
        let inbound = x.to_string().len() * 4
            + props.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
            + id.as_deref().map(|i| i.len()).unwrap_or(0)
            + 128;
        self.check_inbound(inbound)?;
        Self::check_finite(x, "x")?;
        Self::check_finite(y, "y")?;
        Self::check_finite(w, "w")?;
        Self::check_finite(h, "h")?;
        if w < 0.0 || h < 0.0 {
            return Err(CanvasError::InvalidInput("尺寸不可为负".into()));
        }
        let id = match id {
            Some(given) => {
                if given.is_empty() {
                    return Err(CanvasError::InvalidInput("对象 ID 不可为空".into()));
                }
                self.ensure_absent(&given)?;
                given
            }
            None => self.next_id(),
        };
        {
            let mut obj: MapPrelim = Vec::<(&'static str, In)>::new().into_iter().collect();
            obj.insert(
                F_KIND.into(),
                In::Any(Any::String(ObjectKind::Node.wire().into())),
            );
            obj.insert(F_X.into(), In::Any(Any::Number(Number::Float(x))));
            obj.insert(F_Y.into(), In::Any(Any::Number(Number::Float(y))));
            obj.insert(F_W.into(), In::Any(Any::Number(Number::Float(w))));
            obj.insert(F_H.into(), In::Any(Any::Number(Number::Float(h))));
            obj.insert(F_PROPS.into(), In::Map(Self::props_prelim(props)));
            let root = self.root();
            let mut txn = self.doc.transact_mut_with(actor.wire());
            root.insert(&mut txn, id.clone(), In::Map(obj));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(id)
    }

    fn node_map(&self, id: &str, op: &'static str) -> Result<yrs::MapRef, CanvasError> {
        self.check_inbound(id.len())?;
        let root = self.root();
        let txn = self.doc.transact();
        match root.get(&txn, id) {
            Some(Out::YMap(m)) => {
                // kind 校验（Move/Resize 仅 Node 面）。
                if let Some(Out::Any(Any::String(k))) = m.get(&txn, F_KIND) {
                    if op == "geometry" && k.as_ref() != ObjectKind::Node.wire() {
                        return Err(CanvasError::KindMismatch {
                            object_id: id.to_string(),
                            op,
                        });
                    }
                }
                Ok(m)
            }
            _ => Err(CanvasError::ObjectNotFound {
                object_id: id.to_string(),
            }),
        }
    }

    fn move_node(&mut self, actor: Actor, id: &str, x: f64, y: f64) -> Result<(), CanvasError> {
        Self::check_finite(x, "x")?;
        Self::check_finite(y, "y")?;
        let m = self.node_map(id, "geometry")?;
        {
            let mut txn = self.doc.transact_mut_with(actor.wire());
            m.insert(&mut txn, F_X, In::Any(Any::Number(Number::Float(x))));
            m.insert(&mut txn, F_Y, In::Any(Any::Number(Number::Float(y))));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
    }

    fn resize_node(&mut self, actor: Actor, id: &str, w: f64, h: f64) -> Result<(), CanvasError> {
        Self::check_finite(w, "w")?;
        Self::check_finite(h, "h")?;
        if w < 0.0 || h < 0.0 {
            return Err(CanvasError::InvalidInput("尺寸不可为负".into()));
        }
        let m = self.node_map(id, "geometry")?;
        {
            let mut txn = self.doc.transact_mut_with(actor.wire());
            m.insert(&mut txn, F_W, In::Any(Any::Number(Number::Float(w))));
            m.insert(&mut txn, F_H, In::Any(Any::Number(Number::Float(h))));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
    }

    fn add_edge(
        &mut self,
        actor: Actor,
        id: Option<String>,
        from: &str,
        to: &str,
        props: &BTreeMap<String, String>,
    ) -> Result<String, CanvasError> {
        let inbound = from.len()
            + to.len()
            + props.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
            + id.as_deref().map(|i| i.len()).unwrap_or(0)
            + 128;
        self.check_inbound(inbound)?;
        // 端点校验（创建时——并发删除的悬空面由读模型暴露）。
        for ep in [from, to] {
            match self.find_object(ep) {
                Some(o) if o.kind == ObjectKind::Node => {}
                Some(_) => {
                    return Err(CanvasError::InvalidInput(format!("边端点 {ep:?} 不是节点")))
                }
                None => {
                    return Err(CanvasError::EndpointNotFound {
                        object_id: ep.to_string(),
                    })
                }
            }
        }
        if from == to {
            return Err(CanvasError::InvalidInput("自环边不支持".into()));
        }
        let id = match id {
            Some(given) => {
                if given.is_empty() {
                    return Err(CanvasError::InvalidInput("对象 ID 不可为空".into()));
                }
                self.ensure_absent(&given)?;
                given
            }
            None => self.next_id(),
        };
        {
            let mut obj: MapPrelim = Vec::<(&'static str, In)>::new().into_iter().collect();
            obj.insert(
                F_KIND.into(),
                In::Any(Any::String(ObjectKind::Edge.wire().into())),
            );
            obj.insert(F_FROM.into(), In::Any(Any::String(from.into())));
            obj.insert(F_TO.into(), In::Any(Any::String(to.into())));
            obj.insert(F_PROPS.into(), In::Map(Self::props_prelim(props)));
            let root = self.root();
            let mut txn = self.doc.transact_mut_with(actor.wire());
            root.insert(&mut txn, id.clone(), In::Map(obj));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(id)
    }

    fn set_props(
        &mut self,
        actor: Actor,
        id: &str,
        props: &BTreeMap<String, String>,
    ) -> Result<(), CanvasError> {
        let inbound = id.len() + props.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
        self.check_inbound(inbound)?;
        let root = self.root();
        // 存在校验 + 嵌套 map 获取。
        {
            let txn = self.doc.transact();
            if !matches!(root.get(&txn, id), Some(Out::YMap(_))) {
                return Err(CanvasError::ObjectNotFound {
                    object_id: id.to_string(),
                });
            }
        }
        // 读事务作用域内取嵌套 map 引用（MapRef 独立句柄），出域后
        // 再开写事务——yrs 读写事务同锁，遮蔽持有 = 死锁（p18 纪律）。
        let m = {
            let txn = self.doc.transact();
            match root.get(&txn, id) {
                Some(Out::YMap(m)) => m,
                _ => unreachable!("上校验已过"),
            }
        };
        {
            let mut txn = self.doc.transact_mut_with(actor.wire());
            // 终态替换：删 props 键 + 写新集（同事务原子）。
            m.remove(&mut txn, F_PROPS);
            m.insert(&mut txn, F_PROPS, In::Map(Self::props_prelim(props)));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
    }

    fn remove_object(&mut self, actor: Actor, id: &str) -> Result<(), CanvasError> {
        self.check_inbound(id.len())?;
        let root = self.root();
        {
            let txn = self.doc.transact();
            if root.get(&txn, id).is_none() {
                return Err(CanvasError::ObjectNotFound {
                    object_id: id.to_string(),
                });
            }
        }
        {
            let mut txn = self.doc.transact_mut_with(actor.wire());
            root.remove(&mut txn, id);
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
    }

    fn group_objects(
        &mut self,
        actor: Actor,
        group_id: Option<String>,
        members: &[String],
    ) -> Result<String, CanvasError> {
        let inbound = group_id.as_deref().map(|i| i.len()).unwrap_or(0)
            + members.iter().map(|m| m.len()).sum::<usize>()
            + 128;
        self.check_inbound(inbound)?;
        if members.is_empty() {
            return Err(CanvasError::InvalidInput("分组成员不可为空".into()));
        }
        for m in members {
            if self.find_object(m).is_none() {
                return Err(CanvasError::MemberNotFound {
                    object_id: m.clone(),
                });
            }
        }
        let gid = match group_id {
            Some(given) => {
                if given.is_empty() {
                    return Err(CanvasError::InvalidInput("对象 ID 不可为空".into()));
                }
                self.ensure_absent(&given)?;
                given
            }
            None => self.next_id(),
        };
        {
            let mut obj: MapPrelim = Vec::<(&'static str, In)>::new().into_iter().collect();
            obj.insert(
                F_KIND.into(),
                In::Any(Any::String(ObjectKind::Group.wire().into())),
            );
            obj.insert(
                F_MEMBERS.into(),
                In::Any(Any::Array(
                    members
                        .iter()
                        .map(|m| Any::String(m.as_str().into()))
                        .collect::<Vec<Any>>()
                        .into(),
                )),
            );
            obj.insert(F_PROPS.into(), In::Map(MapPrelim::default()));
            let root = self.root();
            let mut txn = self.doc.transact_mut_with(actor.wire());
            root.insert(&mut txn, gid.clone(), In::Map(obj));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(gid)
    }

    fn ungroup(&mut self, actor: Actor, group_id: &str) -> Result<(), CanvasError> {
        self.check_inbound(group_id.len())?;
        let obj = self
            .find_object(group_id)
            .ok_or_else(|| CanvasError::ObjectNotFound {
                object_id: group_id.to_string(),
            })?;
        if obj.kind != ObjectKind::Group {
            return Err(CanvasError::KindMismatch {
                object_id: group_id.to_string(),
                op: "ungroup",
            });
        }
        self.remove_object(actor, group_id)
    }
}

// ── 载体读取/指纹工具 ───────────────────────────────────────────

fn read_object(root: &yrs::MapRef, txn: &yrs::Transaction, id: &str) -> Option<CanvasObject> {
    let m = match root.get(txn, id) {
        Some(Out::YMap(m)) => m,
        _ => return None,
    };
    let kind = match m.get(txn, F_KIND) {
        Some(Out::Any(Any::String(k))) => ObjectKind::from_wire(&k)?,
        _ => return None,
    };
    let num = |key: &str| -> f64 {
        match m.get(txn, key) {
            Some(Out::Any(Any::Number(Number::Float(f)))) => f,
            Some(Out::Any(Any::Number(Number::Int(i)))) => i as f64,
            _ => 0.0,
        }
    };
    let str_field = |key: &str| -> Option<String> {
        match m.get(txn, key) {
            Some(Out::Any(Any::String(s))) => Some(s.to_string()),
            _ => None,
        }
    };
    let members = match m.get(txn, F_MEMBERS) {
        Some(Out::Any(Any::Array(arr))) => arr
            .iter()
            .filter_map(|a| match a {
                Any::String(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    };
    let mut props = BTreeMap::new();
    if let Some(Out::YMap(p)) = m.get(txn, F_PROPS) {
        for (k, v) in p.iter(txn) {
            if let Out::Any(Any::String(s)) = v {
                props.insert(k.to_string(), s.to_string());
            }
        }
    }
    Some(CanvasObject {
        id: id.to_string(),
        kind,
        x: num(F_X),
        y: num(F_Y),
        w: num(F_W),
        h: num(F_H),
        from: str_field(F_FROM),
        to: str_field(F_TO),
        members,
        props,
    })
}

/// 对象语义指纹：语义字段确定性序列化 → FNV-1a 64。
pub fn fingerprint_object(o: &CanvasObject) -> String {
    let mut acc = String::new();
    acc.push_str(o.kind.wire());
    acc.push('|');
    acc.push_str(&o.id);
    if o.kind != ObjectKind::Edge {
        // 几何语义面（Node/Group；Edge 无几何）。
        acc.push_str(&format!("|{:.4}|{:.4}|{:.4}|{:.4}", o.x, o.y, o.w, o.h));
    }
    if o.kind == ObjectKind::Edge {
        acc.push_str(&format!(
            "|{}->{}",
            o.from.as_deref().unwrap_or(""),
            o.to.as_deref().unwrap_or("")
        ));
    }
    if o.kind == ObjectKind::Group {
        let mut ms = o.members.clone();
        ms.sort();
        acc.push('|');
        acc.push_str(&ms.join(","));
    }
    for (k, v) in &o.props {
        acc.push_str(&format!("|p:{k}={v}"));
    }
    mdc_evidence::Fnv64.fingerprint(acc.as_bytes()).0
}

fn panic_message(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}
