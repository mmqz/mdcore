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
use yrs::sync::time::Clock;
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
/// 笔迹专有四键（#350——非 Stroke 对象不写不读）。
const F_POINTS: &str = "points";
const F_COLOR: &str = "color";
const F_SWIDTH: &str = "swidth";
const F_TOOL: &str = "tool";
/// #402：z 序键（fractional index 字符串——字典序即 z 序；缺省
/// `zorder::DEFAULT_KEY`，存量对象读侧兼容）。
const F_ZINDEX: &str = "zindex";

/// 画布对象种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Node,
    Edge,
    Group,
    /// 笔迹（#44 第 1 步）：原始采样点列 + 笔色/线宽/工具——存点不存
    /// outline（换平滑算法/改粗细不迁移数据；outline 体积 = 点列 5-10 倍）。
    Stroke,
}

impl ObjectKind {
    fn wire(self) -> &'static str {
        match self {
            ObjectKind::Node => "node",
            ObjectKind::Edge => "edge",
            ObjectKind::Group => "group",
            ObjectKind::Stroke => "stroke",
        }
    }

    fn from_wire(s: &str) -> Option<ObjectKind> {
        match s {
            "node" => Some(ObjectKind::Node),
            "edge" => Some(ObjectKind::Edge),
            "group" => Some(ObjectKind::Group),
            "stroke" => Some(ObjectKind::Stroke),
            _ => None,
        }
    }
}

/// 笔迹工具（渲染语义差：Pen = 不透明圆头线；Highlighter = 半透明
/// 平头宽线——半透明是**渲染态**，模型颜色恒存不透明值）。
///
/// 橡皮擦**不建模为笔种**（#44 引 canvas-a §3：两段式橡皮擦 =
/// pointer-up 才提交的**软删交互**，引擎面 = 既有 [`CanvasCommand::RemoveObject`]；
/// 建模为笔种会产生「画出来的删除」语义矛盾）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeTool {
    Pen,
    Highlighter,
}

impl StrokeTool {
    fn wire(self) -> &'static str {
        match self {
            StrokeTool::Pen => "pen",
            StrokeTool::Highlighter => "highlighter",
        }
    }

    fn from_wire(s: &str) -> Option<StrokeTool> {
        match s {
            "pen" => Some(StrokeTool::Pen),
            "highlighter" => Some(StrokeTool::Highlighter),
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
    /// 笔迹采样点（(x, y) 原始序列；非 Stroke = 空）——**不存
    /// outline**（#44 裁决：平滑/加粗由消费侧重算，数据免迁移）。
    pub points: Vec<(f64, f64)>,
    /// 笔迹颜色（#RGB/#RRGGBB；非 Stroke = None）。
    pub color: Option<String>,
    /// 笔迹线宽（域单位；非 Stroke = None）。
    pub width: Option<f64>,
    /// 笔迹工具（非 Stroke = None）。
    pub tool: Option<StrokeTool>,
    /// #402：z 序键（fractional index——字典序即 z 序；渲染按
    /// `(z_index, id)` 排序，ID 决胜吸收并发同键）。新建对象置顶
    /// （generate(max, None)）；存量对象缺省 [`zorder::DEFAULT_KEY`]。
    /// Group 对象的 z 不影响成员渲染（成员各自成图元），字段统一
    /// 仅为模型一致性。
    pub z_index: String,
    /// 开放属性（label/color/…；字符串值面）。
    pub props: BTreeMap<String, String>,
}

impl Default for CanvasObject {
    /// 缺省 = 零几何 Node（AGENTS.md 0.0.5 方案 A：字面量构造点用
    /// `..Default::default()` 跟住未来新增字段——kind 专属字段的缺省
    /// 形状 = 非 Stroke/非 Edge 的空/None，读模型语义安全）。
    fn default() -> Self {
        CanvasObject {
            id: String::new(),
            kind: ObjectKind::Node,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            from: None,
            to: None,
            members: vec![],
            points: vec![],
            color: None,
            width: None,
            tool: None,
            z_index: crate::zorder::DEFAULT_KEY.to_string(),
            props: BTreeMap::new(),
        }
    }
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

/// #402：z 序重排操作（四向——Figma/Excalidraw/tldraw 标准集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderOp {
    /// 置顶（绝对——越过所有其他对象）。
    BringToFront,
    /// 置底（绝对）。
    SendToBack,
    /// 上移一层（相对；已在顶层则无变化）。
    BringForward,
    /// 下移一层（相对；已在底层则无变化）。
    SendBackward,
}

impl ReorderOp {
    /// 线格式（wasm 桥 / JSON 命令面）。
    pub fn wire(self) -> &'static str {
        match self {
            ReorderOp::BringToFront => "front",
            ReorderOp::SendToBack => "back",
            ReorderOp::BringForward => "forward",
            ReorderOp::SendBackward => "backward",
        }
    }

    /// 线格式反解（未知串 → None——敌意面缺省拒）。
    pub fn from_wire(s: &str) -> Option<ReorderOp> {
        match s {
            "front" => Some(ReorderOp::BringToFront),
            "back" => Some(ReorderOp::SendToBack),
            "forward" => Some(ReorderOp::BringForward),
            "backward" => Some(ReorderOp::SendBackward),
            _ => None,
        }
    }
}

/// 会话选项（与 mdc-collab::SessionOptions 同构纪律：不含 skip_gc、
/// 不含捕获窗）。
#[derive(Clone)]
pub struct SessionOptions {
    pub client_id: u64,
    pub input_byte_limit: usize,
    /// 撤销栈软上限（粗粒度 clear_all 策略——同 mdc-collab）。
    pub undo_stack_limit: Option<usize>,
    /// 时钟注入面（p5-t5 wasm 前置）：撤销栈时间戳源。`None` = 系统
    /// 时钟（native 默认——既有行为零变化）；`Some(f)` = 宿主注入
    /// （wasm32 无 std 时间系统——`SystemTime::now()` = trap；注入
    /// 确定性时钟，与 mdcore-wasm `set_clock_ms`/mdc-pdf
    /// `PortableInstant` 同纪律）。
    pub clock_ms: Option<std::sync::Arc<dyn Fn() -> u64 + Send + Sync>>,
}

impl std::fmt::Debug for SessionOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 闭包面非 Debug——以占位标记呈现（Debug derive 与 dyn Fn 冲突）。
        f.debug_struct("SessionOptions")
            .field("client_id", &self.client_id)
            .field("input_byte_limit", &self.input_byte_limit)
            .field("undo_stack_limit", &self.undo_stack_limit)
            .field("clock_ms", &self.clock_ms.as_ref().map(|_| "<clock>"))
            .finish()
    }
}

impl Default for SessionOptions {
    fn default() -> Self {
        SessionOptions {
            client_id: 7_200_001,
            input_byte_limit: 52 * 1024 * 1024,
            undo_stack_limit: None,
            clock_ms: None,
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
    /// 加一笔笔迹（#350——#44 第 1 步）：原始采样点 ≥ 2，不存 outline。
    AddStroke {
        id: Option<String>,
        points: Vec<(f64, f64)>,
        /// #RGB/#RRGGBB（不透明值——Highlighter 半透明是渲染态）。
        color: String,
        width: f64,
        tool: StrokeTool,
    },
    /// 笔迹样式终态替换（#44 第 3 步选择器的引擎前置；不动点列——
    /// 平滑 outline 由消费侧重算，改粗细零数据迁移）。
    SetStrokeStyle {
        id: String,
        color: String,
        width: f64,
        tool: StrokeTool,
    },
    /// #402：z 序重排（fractional index 改写——单对象原子属性，
    /// O(1) 无写放大）。
    Reorder {
        id: String,
        op: ReorderOp,
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
    NodeAdded {
        id: String,
    },
    NodeMoved {
        id: String,
    },
    NodeResized {
        id: String,
    },
    EdgeAdded {
        id: String,
    },
    PropsSet {
        id: String,
    },
    ObjectRemoved {
        id: String,
    },
    Grouped {
        group_id: String,
    },
    Ungrouped {
        group_id: String,
    },
    StrokeAdded {
        id: String,
    },
    /// #402：z 序已重排（新键已写入；幂等无变化也回此回执）。
    Reordered {
        id: String,
    },
    StrokeStyled {
        id: String,
    },
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

        let mut undo_human = Self::new_undo_manager(Actor::Human, &options.clock_ms);
        let mut undo_ai = Self::new_undo_manager(Actor::Ai, &options.clock_ms);
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

    fn new_undo_manager(
        origin: Actor,
        clock_ms: &Option<std::sync::Arc<dyn Fn() -> u64 + Send + Sync>>,
    ) -> UndoManager {
        // 与 mdc-collab 同构：每 origin 一栈 + 窗 0 + capture_transaction
        // 拒 None-origin（远端应用不入本地栈）。
        let mut tracked = HashSet::new();
        tracked.insert(origin.wire());
        /// 宿主闭包 → yrs Clock 适配器（注入面）。
        struct HostClock<F: Fn() -> u64 + Send + Sync>(F);
        impl<F: Fn() -> u64 + Send + Sync> Clock for HostClock<F> {
            fn now(&self) -> u64 {
                (self.0)()
            }
        }
        /// 系统时钟（native 默认——wasm32 不可达：SessionOptions 时钟
        /// 注入是 wasm 构造前置）。
        struct SystemClock;
        impl Clock for SystemClock {
            fn now(&self) -> u64 {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            }
        }
        let timestamp: std::sync::Arc<dyn Clock> = match clock_ms {
            Some(f) => {
                let f = f.clone();
                std::sync::Arc::new(HostClock(move || f()))
            }
            None => std::sync::Arc::new(SystemClock),
        };
        UndoManager::with_options(UndoOptions {
            capture_timeout_millis: 0,
            tracked_origins: tracked,
            capture_transaction: Some(std::sync::Arc::new(|txn| txn.origin().is_some())),
            timestamp,
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
        // 撞号跳过环：同 client 域既有 id（对端复活/重放携带的同段号
        // 对象——同槽 reload 后计数器归零的经典撞号面）逐段跳过。分配
        // 是稀有路径（用户命令级），O(1) map 键查找零成本；无既有对象
        // 时恰一次循环 = 与原实现同代价。
        loop {
            self.next_seq += 1;
            let id = format!("{}-{}", self.options.client_id, self.next_seq);
            let occupied = {
                let root = self.root();
                let txn = self.doc.transact();
                root.get(&txn, id.as_str()).is_some()
            };
            if !occupied {
                return id;
            }
        }
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

    /// #402：全量对象（z 序：`(z_index, id)` 决胜——渲染消费面）。
    ///
    /// 与 [`Self::objects`]（ID 序）并存：指纹/断言等稳定性敏感面用
    /// ID 序；绘制顺序（遮挡关系）用本面。并发同键（两副本同邻位
    /// 插入）由 ID 决胜吸收——合并后两副本全序一致。
    pub fn objects_z_ordered(&self) -> Vec<CanvasObject> {
        let mut out = self.objects();
        out.sort_by(|a, b| (&a.z_index, &a.id).cmp(&(&b.z_index, &b.id)));
        out
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
            CanvasCommand::AddStroke {
                id,
                points,
                color,
                width,
                tool,
            } => {
                let sid = self.add_stroke(actor, id, &points, &color, width, tool)?;
                CommandOutcome::StrokeAdded { id: sid }
            }
            CanvasCommand::SetStrokeStyle {
                id,
                color,
                width,
                tool,
            } => {
                self.set_stroke_style(actor, &id, &color, width, tool)?;
                CommandOutcome::StrokeStyled { id }
            }
            CanvasCommand::Reorder { id, op } => {
                self.reorder_object(actor, &id, op)?;
                CommandOutcome::Reordered { id }
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

    // ── z 序（#402）─────────────────────────────────────────────

    /// 新建对象的置顶 z 键（max 之上生成——新形状在上层，行业惯例）。
    ///
    /// 现存 z 键含敌意非法值时生成失败 → 兜底 `DEFAULT_KEY`（宁缺毋错：
    /// 新对象落缺省位，不 panic；重排命令面会拒绝非法键并报错）。
    fn next_top_z(&self) -> String {
        let max = self.objects().into_iter().map(|o| o.z_index).max();
        match crate::zorder::generate_between(max.as_deref(), None) {
            Ok(z) => z,
            Err(_) => crate::zorder::DEFAULT_KEY.to_string(),
        }
    }

    /// 对象裸 Map 引用（重排/属性写面的公共取位——无 kind 门禁）。
    fn object_map(&self, id: &str) -> Result<yrs::MapRef, CanvasError> {
        self.check_inbound(id.len())?;
        let root = self.root();
        let txn = self.doc.transact();
        match root.get(&txn, id) {
            Some(Out::YMap(m)) => Ok(m),
            _ => Err(CanvasError::ObjectNotFound {
                object_id: id.to_string(),
            }),
        }
    }

    /// #402：z 序重排——按当前全序计算新键并单事务写入。
    ///
    /// - BringToFront / SendToBack：绝对（越过所有其他对象）；
    /// - BringForward / SendBackward：相对一步——跨过最近的**不同 z**
    ///   邻层（同键并列对象视为同层，一步整层跨过）；
    /// - 相对操作已在端点 → 幂等无变化（不产生空写 = 零 CRDT 字节
    ///   = 零事件，与既有命令的事件语义一致）；
    /// - 邻层 z 键非法（敌意面）→ `InvalidInput` 拒绝（fail-closed，
    ///   不静默排错）。
    fn reorder_object(&mut self, actor: Actor, id: &str, op: ReorderOp) -> Result<(), CanvasError> {
        self.check_inbound(id.len())?;
        // 全序快照 (z, id)——邻域计算面（排序在 objects_z_ordered 同口径）
        let mut zs: Vec<(String, String)> = self
            .objects()
            .into_iter()
            .map(|o| (o.z_index, o.id))
            .collect();
        zs.sort();
        let Some(pos) = zs.iter().position(|(_, oid)| oid == id) else {
            return Err(CanvasError::ObjectNotFound {
                object_id: id.to_string(),
            });
        };
        let cur_z = zs[pos].0.clone();
        let map_err = |e: crate::zorder::ZOrderError| {
            CanvasError::InvalidInput(format!("z 序键生成失败（{e}）——现存键面非法"))
        };
        let new_z = match op {
            ReorderOp::BringToFront => {
                let max_other = zs
                    .iter()
                    .filter(|(_, oid)| oid != id)
                    .map(|(z, _)| z.clone())
                    .max();
                crate::zorder::generate_between(max_other.as_deref(), None).map_err(map_err)?
            }
            ReorderOp::SendToBack => {
                let min_other = zs
                    .iter()
                    .filter(|(_, oid)| oid != id)
                    .map(|(z, _)| z.clone())
                    .min();
                crate::zorder::generate_between(None, min_other.as_deref()).map_err(map_err)?
            }
            ReorderOp::BringForward => {
                // 上邻 u = 第一个不同 z 的在上方对象；新键 ∈ (u, u 之上
                // 第一层)——落在 u 的**上方**（跨过整层）。
                let upper = zs[pos + 1..]
                    .iter()
                    .find(|(z, _)| z.as_str() > cur_z.as_str())
                    .map(|(z, _)| z.clone());
                match upper {
                    None => cur_z.clone(), // 已在顶层：幂等
                    Some(u) => {
                        let above_u = zs[pos + 1..]
                            .iter()
                            .find(|(z, _)| z.as_str() > u.as_str())
                            .map(|(z, _)| z.clone());
                        crate::zorder::generate_between(Some(&u), above_u.as_deref())
                            .map_err(map_err)?
                    }
                }
            }
            ReorderOp::SendBackward => {
                // 下邻 l = 第一个不同 z 的在下方对象；新键 ∈ (l 之下
                // 第一层, l)——落在 l 的**下方**（跨过整层）。
                let lower = zs[..pos]
                    .iter()
                    .rev()
                    .find(|(z, _)| z.as_str() < cur_z.as_str())
                    .map(|(z, _)| z.clone());
                match lower {
                    None => cur_z.clone(), // 已在底层：幂等
                    Some(l) => {
                        let below_l = zs[..pos]
                            .iter()
                            .rev()
                            .find(|(z, _)| z.as_str() < l.as_str())
                            .map(|(z, _)| z.clone());
                        crate::zorder::generate_between(below_l.as_deref(), Some(&l))
                            .map_err(map_err)?
                    }
                }
            }
        };
        if new_z == cur_z {
            return Ok(()); // 幂等：零写入零事件
        }
        let m = self.object_map(id)?;
        {
            let mut txn = self.doc.transact_mut_with(actor.wire());
            m.insert(&mut txn, F_ZINDEX, In::Any(Any::String(new_z.into())));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
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
            let z = self.next_top_z();
            let mut obj: MapPrelim = Vec::<(&'static str, In)>::new().into_iter().collect();
            obj.insert(
                F_KIND.into(),
                In::Any(Any::String(ObjectKind::Node.wire().into())),
            );
            obj.insert(F_X.into(), In::Any(Any::Number(Number::Float(x))));
            obj.insert(F_Y.into(), In::Any(Any::Number(Number::Float(y))));
            obj.insert(F_W.into(), In::Any(Any::Number(Number::Float(w))));
            obj.insert(F_H.into(), In::Any(Any::Number(Number::Float(h))));
            obj.insert(F_ZINDEX.into(), In::Any(Any::String(z.into())));
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
            let z = self.next_top_z();
            let mut obj: MapPrelim = Vec::<(&'static str, In)>::new().into_iter().collect();
            obj.insert(
                F_KIND.into(),
                In::Any(Any::String(ObjectKind::Edge.wire().into())),
            );
            obj.insert(F_FROM.into(), In::Any(Any::String(from.into())));
            obj.insert(F_TO.into(), In::Any(Any::String(to.into())));
            obj.insert(F_ZINDEX.into(), In::Any(Any::String(z.into())));
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
            let z = self.next_top_z();
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
            obj.insert(F_ZINDEX.into(), In::Any(Any::String(z.into())));
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

    // ── 笔迹面（#350——#44 第 1 步）────────────────────────────

    /// 颜色合法面：#RGB/#RRGGBB（严格——8 位 RGBA 带透明语义，归
    /// Highlighter 渲染态，不入模型面）。
    fn valid_stroke_color(s: &str) -> bool {
        let Some(hex) = s.strip_prefix('#') else {
            return false;
        };
        matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
    }

    /// 笔迹字段校验（Add/SetStrokeStyle 共用——错误面全走既有变体）。
    fn check_stroke_style(color: &str, width: f64) -> Result<(), CanvasError> {
        if !Self::valid_stroke_color(color) {
            return Err(CanvasError::InvalidInput(format!(
                "笔迹颜色 {color:?} 不合 #RGB/#RRGGBB"
            )));
        }
        if !width.is_finite() {
            return Err(CanvasError::NonFiniteGeometry { field: "width" });
        }
        if width <= 0.0 {
            return Err(CanvasError::InvalidInput("笔迹线宽必须 > 0".into()));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // 命令域字段直传（同 add_node 纪律）
    fn add_stroke(
        &mut self,
        actor: Actor,
        id: Option<String>,
        points: &[(f64, f64)],
        color: &str,
        width: f64,
        tool: StrokeTool,
    ) -> Result<String, CanvasError> {
        // 入站估算：每点两坐标（f64 字面宽 ~24）+ 颜色/工具/框架开销。
        let inbound = points.len() * 48
            + color.len()
            + tool.wire().len()
            + 128
            + id.as_deref().map(|i| i.len()).unwrap_or(0);
        self.check_inbound(inbound)?;
        if points.len() < 2 {
            return Err(CanvasError::InvalidInput(
                "笔迹至少需 2 个采样点（单击不成笔）".into(),
            ));
        }
        for &(x, y) in points {
            Self::check_finite(x, "x")?;
            Self::check_finite(y, "y")?;
            // 重复点无害（采样抖动）；零长笔仍合法——渲染面退化不 panic。
        }
        Self::check_stroke_style(color, width)?;
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
            // 扁平点列 [x0,y0,x1,y1,…]——单 Any::Array，CRDT 面最简
            //（嵌套数组读回成本高，逐点对象无必要）。
            let flat: Vec<Any> = points
                .iter()
                .flat_map(|(x, y)| {
                    [
                        Any::Number(Number::Float(*x)),
                        Any::Number(Number::Float(*y)),
                    ]
                })
                .collect();
            let z = self.next_top_z();
            let mut obj: MapPrelim = Vec::<(&'static str, In)>::new().into_iter().collect();
            obj.insert(
                F_KIND.into(),
                In::Any(Any::String(ObjectKind::Stroke.wire().into())),
            );
            obj.insert(F_POINTS.into(), In::Any(Any::Array(flat.into())));
            obj.insert(F_COLOR.into(), In::Any(Any::String(color.into())));
            obj.insert(F_SWIDTH.into(), In::Any(Any::Number(Number::Float(width))));
            obj.insert(F_TOOL.into(), In::Any(Any::String(tool.wire().into())));
            obj.insert(F_ZINDEX.into(), In::Any(Any::String(z.into())));
            obj.insert(F_PROPS.into(), In::Map(MapPrelim::default()));
            let root = self.root();
            let mut txn = self.doc.transact_mut_with(actor.wire());
            root.insert(&mut txn, id.clone(), In::Map(obj));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(id)
    }

    fn set_stroke_style(
        &mut self,
        actor: Actor,
        id: &str,
        color: &str,
        width: f64,
        tool: StrokeTool,
    ) -> Result<(), CanvasError> {
        self.check_inbound(id.len() + color.len() + 64)?;
        Self::check_stroke_style(color, width)?;
        // 存在 + 类型校验（同 node_map 纪律：读域取引用，出域再写——
        // yrs 读写同锁，遮蔽持有 = 死锁 p18）。
        let m = {
            let root = self.root();
            let txn = self.doc.transact();
            match root.get(&txn, id) {
                Some(Out::YMap(m)) => {
                    let is_stroke = matches!(
                        m.get(&txn, F_KIND),
                        Some(Out::Any(Any::String(k))) if k.as_ref() == ObjectKind::Stroke.wire()
                    );
                    if !is_stroke {
                        // 已知对象但非笔迹 → 类型不符；不存在 → 下枝 404。
                        return Err(CanvasError::KindMismatch {
                            object_id: id.to_string(),
                            op: "set_stroke_style",
                        });
                    }
                    m
                }
                _ => {
                    return Err(CanvasError::ObjectNotFound {
                        object_id: id.to_string(),
                    })
                }
            }
        };
        {
            let mut txn = self.doc.transact_mut_with(actor.wire());
            m.insert(&mut txn, F_COLOR, In::Any(Any::String(color.into())));
            m.insert(
                &mut txn,
                F_SWIDTH,
                In::Any(Any::Number(Number::Float(width))),
            );
            m.insert(&mut txn, F_TOOL, In::Any(Any::String(tool.wire().into())));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
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
    // 笔迹面（#350）：扁平点列 [x0,y0,x1,y1,…] → (x,y) 对；奇数长
    // /非数值条目按敌意载体处理——截到最近的完整对（读模型不 panic）。
    let points = match m.get(txn, F_POINTS) {
        Some(Out::Any(Any::Array(arr))) => arr
            .iter()
            .filter_map(|a| match a {
                Any::Number(Number::Float(f)) => Some(*f),
                Any::Number(Number::Int(i)) => Some(*i as f64),
                _ => None,
            })
            .collect::<Vec<f64>>()
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (c[0], c[1]))
            .collect(),
        _ => vec![],
    };
    let (color, width, tool) = if kind == ObjectKind::Stroke {
        (
            str_field(F_COLOR),
            match m.get(txn, F_SWIDTH) {
                Some(Out::Any(Any::Number(Number::Float(f)))) => Some(f),
                Some(Out::Any(Any::Number(Number::Int(i)))) => Some(i as f64),
                _ => None,
            },
            str_field(F_TOOL).as_deref().and_then(StrokeTool::from_wire),
        )
    } else {
        (None, None, None)
    };
    // #402：z 序键（缺省 DEFAULT_KEY——存量对象/敌意缺字段兼容；
    // 不在此校验键型：排序面只做字典序比较，重排命令面才校验）。
    let z_index = str_field(F_ZINDEX).unwrap_or_else(|| crate::zorder::DEFAULT_KEY.to_string());
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
        points,
        color,
        width,
        tool,
        z_index,
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
    if o.kind == ObjectKind::Stroke {
        // 笔迹语义面：全点列 + 样式三件（改笔色/改粗细/换工具 = 语义
        // 变更 → 证据失效；点序不可交换——按存序指纹，非排序）。
        acc.push('|');
        for (x, y) in &o.points {
            acc.push_str(&format!("|{:.4},{:.4}", x, y));
        }
        acc.push('|');
        acc.push_str(o.color.as_deref().unwrap_or(""));
        acc.push('|');
        if let Some(w) = o.width {
            acc.push_str(&format!("{:.4}", w));
        }
        acc.push('|');
        acc.push_str(o.tool.map(|t| t.wire()).unwrap_or(""));
    }
    for (k, v) in &o.props {
        acc.push_str(&format!("|p:{k}={v}"));
    }
    // #402：z 序键入指纹（重排 = 语义变更 → 证据失效——与「位置漂移
    // 即语义变更」同粒度：遮挡关系是视觉语义面）。
    acc.push_str(&format!("|z={}", o.z_index));
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
