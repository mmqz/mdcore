//! mdc-canvas 会话 wasm 出口（阶段 5 §八.5 Canvas 表面）。
//!
//! ## 定位
//! [`WasmCanvas`] = mdc-canvas headless 会话的 wasm 句柄（四表面
//! 第四员——与 [`crate::WasmCore`]/[`crate::xlsx_grid::WasmBook`]/
//! [`crate::workbench::WasmWorkbench`] 同模式）。**单一真相**：领域
//! 模型（节点/连线/分组）、命令校验、LWW 合并、双栈撤销、指纹、
//! 事件——全部在 mdc-canvas；本模块零语义逻辑，只做边界互译
//! （flat snake_case 命令 JSON ↔ `CanvasCommand` + typed 错误串）。
//!
//! ## 时钟注入
//! wasm32 无 std 时间系统（`SystemTime::now()` = trap）——构造时经
//! `SessionOptions::clock_ms` 注入宿主时钟（内部 `RefCell<u64>` +
//! [`Self::set_clock_ms`] 推进——与 `WasmCore::set_clock_ms` 同纪律）。
//!
//! ## 事件面
//! `take_events_json()` 轮询（排空式——`WasmCore::take_events` 先例）：
//! 订阅闭包把 `CanvasEvent` 转 JSON 累积到内部缓冲，宿主 rAF 轮询取
//! 走。`updates` 为逐事务 wire 字节的 base64（可独立 `apply_update`
//! 转发——中继/对端直连直接消费）。
//!
//! ## 错误面
//! typed JSON 串 `{"code":"cv_*","message":"..."}`——`CanvasError`
//! 全变体一码一义（wb_* / pdf_* 先例）。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use wasm_bindgen::prelude::*;

use mdc_canvas::{
    Actor, CanvasCommand, CanvasError, CanvasObject, CanvasSession, CommandOutcome, ObjectKind,
    ReorderOp, SessionOptions, StrokeTool,
};

// ---------------------------------------------------------------------------
// 错误面
// ---------------------------------------------------------------------------

/// wasm 边界错误（typed 码——宿主可 JSON.parse）。
#[derive(Debug)]
pub struct CvError {
    pub code: &'static str,
    pub message: String,
}

impl CvError {
    fn to_js(&self) -> JsValue {
        JsValue::from_str(
            &serde_json::json!({"code": self.code, "message": self.message}).to_string(),
        )
    }
}

impl From<CanvasError> for CvError {
    fn from(e: CanvasError) -> Self {
        let code = match &e {
            CanvasError::Limit { .. } => "cv_limit",
            CanvasError::Decode(_) => "cv_codec",
            CanvasError::Panic(_) => "cv_panic",
            CanvasError::InvalidInput(_) => "cv_invalid_input",
            CanvasError::ObjectNotFound { .. } => "cv_object_not_found",
            CanvasError::DuplicateId { .. } => "cv_duplicate_id",
            CanvasError::EndpointNotFound { .. } => "cv_endpoint_not_found",
            CanvasError::MemberNotFound { .. } => "cv_member_not_found",
            CanvasError::NonFiniteGeometry { .. } => "cv_non_finite",
            CanvasError::KindMismatch { .. } => "cv_kind_mismatch",
        };
        CvError {
            code,
            message: format!("{e}"),
        }
    }
}

fn bad_json(what: &str, detail: String) -> CvError {
    CvError {
        code: "cv_bad_json",
        message: format!("{what}: {detail}"),
    }
}

// ---------------------------------------------------------------------------
// base64（updates wire 字节 ↔ 事件 JSON 字符串——零三方）
// ---------------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        out.push(B64[(b[0] >> 2) as usize] as char);
        out.push(B64[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(b[2] & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

// ---------------------------------------------------------------------------
// 纯内核（JsValue-free——原生 cargo test 面）
// ---------------------------------------------------------------------------

/// flat 命令 JSON → `CanvasCommand`（`{"t":"add_node",...}` 八命令）。
pub fn parse_cmd_core(json: &str) -> Result<CanvasCommand, CvError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| bad_json("command", e.to_string()))?;
    let t = v
        .get("t")
        .and_then(|x| x.as_str())
        .ok_or_else(|| bad_json("command", "missing `t`".into()))?;
    let opt_id = || -> Result<Option<String>, CvError> {
        match v.get("id") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
            Some(other) => Err(bad_json(
                "command",
                format!("`id` must be string or null, got {other}"),
            )),
        }
    };
    let f = |field: &str| -> Result<f64, CvError> {
        v.get(field)
            .and_then(|x| x.as_f64())
            .ok_or_else(|| bad_json("command", format!("missing/invalid `{field}`")))
    };
    let props = |field: &str| -> Result<BTreeMap<String, String>, CvError> {
        match v.get(field) {
            None => Ok(BTreeMap::new()),
            Some(serde_json::Value::Object(m)) => {
                let mut out = BTreeMap::new();
                for (k, val) in m {
                    let s = val.as_str().ok_or_else(|| {
                        bad_json("command", format!("`{field}.{k}` must be string"))
                    })?;
                    out.insert(k.clone(), s.to_string());
                }
                Ok(out)
            }
            Some(other) => Err(bad_json(
                "command",
                format!("`{field}` must be object, got {other}"),
            )),
        }
    };
    let s = |field: &str| -> Result<String, CvError> {
        v.get(field)
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .ok_or_else(|| bad_json("command", format!("missing/invalid `{field}`")))
    };
    let str_vec = |field: &str| -> Result<Vec<String>, CvError> {
        match v.get(field) {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .map(|x| {
                    x.as_str().map(str::to_string).ok_or_else(|| {
                        bad_json("command", format!("`{field}` entries must be strings"))
                    })
                })
                .collect(),
            _ => Err(bad_json("command", format!("missing/invalid `{field}`"))),
        }
    };
    // 笔迹点列：[[x,y],…]（perfect-freehand 消费形状）→ (x,y) 对。
    let points = |field: &str| -> Result<Vec<(f64, f64)>, CvError> {
        match v.get(field) {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .map(|x| {
                    let pair = x.as_array().ok_or_else(|| {
                        bad_json("command", format!("`{field}` entries must be [x,y] pairs"))
                    })?;
                    if pair.len() != 2 {
                        return Err(bad_json(
                            "command",
                            format!("`{field}` entries must be [x,y] pairs, got {x}"),
                        ));
                    }
                    let x = pair[0].as_f64().ok_or_else(|| {
                        bad_json(
                            "command",
                            format!("`{field}` x must be number, got {}", pair[0]),
                        )
                    })?;
                    let y = pair[1].as_f64().ok_or_else(|| {
                        bad_json(
                            "command",
                            format!("`{field}` y must be number, got {}", pair[1]),
                        )
                    })?;
                    Ok((x, y))
                })
                .collect(),
            _ => Err(bad_json("command", format!("missing/invalid `{field}`"))),
        }
    };
    let tool = |field: &str| -> Result<StrokeTool, CvError> {
        match s(field)?.as_str() {
            "pen" => Ok(StrokeTool::Pen),
            "highlighter" => Ok(StrokeTool::Highlighter),
            other => Err(bad_json(
                "command",
                format!("`{field}` must be pen/highlighter, got {other}"),
            )),
        }
    };
    match t {
        "add_node" => Ok(CanvasCommand::AddNode {
            id: opt_id()?,
            x: f("x")?,
            y: f("y")?,
            w: f("w")?,
            h: f("h")?,
            props: props("props")?,
        }),
        "move_node" => Ok(CanvasCommand::MoveNode {
            id: s("id")?,
            x: f("x")?,
            y: f("y")?,
        }),
        "resize_node" => Ok(CanvasCommand::ResizeNode {
            id: s("id")?,
            w: f("w")?,
            h: f("h")?,
        }),
        "add_edge" => Ok(CanvasCommand::AddEdge {
            id: opt_id()?,
            from: s("from")?,
            to: s("to")?,
            props: props("props")?,
        }),
        "set_props" => Ok(CanvasCommand::SetProps {
            id: s("id")?,
            props: props("props")?,
        }),
        "remove_object" => Ok(CanvasCommand::RemoveObject { id: s("id")? }),
        "group_objects" => Ok(CanvasCommand::GroupObjects {
            group_id: opt_id()?,
            members: str_vec("members")?,
        }),
        "ungroup" => Ok(CanvasCommand::Ungroup {
            group_id: s("group_id")?,
        }),
        "add_stroke" => Ok(CanvasCommand::AddStroke {
            id: opt_id()?,
            points: points("points")?,
            color: s("color")?,
            width: f("width")?,
            tool: tool("tool")?,
        }),
        "set_stroke_style" => Ok(CanvasCommand::SetStrokeStyle {
            id: s("id")?,
            color: s("color")?,
            width: f("width")?,
            tool: tool("tool")?,
        }),
        "reorder" => {
            // #402：z 序重排（op = front/back/forward/backward）。
            let op = ReorderOp::from_wire(&s("op")?).ok_or_else(|| {
                bad_json(
                    "command",
                    "`op` must be front/back/forward/backward".to_string(),
                )
            })?;
            Ok(CanvasCommand::Reorder { id: s("id")?, op })
        }
        other => Err(bad_json(
            "command",
            format!("unknown command `{other}` (11 commands: add_node/move_node/resize_node/add_edge/set_props/remove_object/group_objects/ungroup/add_stroke/set_stroke_style/reorder)"),
        )),
    }
}

fn kind_wire(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Node => "node",
        ObjectKind::Edge => "edge",
        ObjectKind::Group => "group",
        ObjectKind::Stroke => "stroke",
    }
}

/// 读模型对象 → JSON（字段面与公开结构 1:1；笔迹四字段在非 Stroke
/// 对象上为缺省形状——空点列/null，同 from/to 的 null 纪律）。
pub fn object_json(o: &CanvasObject) -> String {
    let props: Vec<String> = o
        .props
        .iter()
        .map(|(k, v)| {
            format!(
                "{}:{}",
                serde_json::to_string(k).unwrap(),
                serde_json::to_string(v).unwrap()
            )
        })
        .collect();
    let members: Vec<String> = o
        .members
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    // 笔迹点列 [[x,y],…]（perfect-freehand 消费形状）。
    let points: Vec<String> = o.points.iter().map(|(x, y)| format!("[{x},{y}]")).collect();
    let opt_str = |v: &Option<String>| -> String {
        match v {
            Some(x) => serde_json::to_string(x).unwrap(),
            None => "null".into(),
        }
    };
    let width = match o.width {
        Some(w) => format!("{w}"),
        None => "null".into(),
    };
    let tool = match o.tool {
        Some(StrokeTool::Pen) => "\"pen\"".to_string(),
        Some(StrokeTool::Highlighter) => "\"highlighter\"".to_string(),
        None => "null".to_string(),
    };
    format!(
        "{{\"id\":{},\"kind\":\"{}\",\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"from\":{},\"to\":{},\"members\":[{}],\"points\":[{}],\"color\":{},\"width\":{},\"tool\":{},\"z_index\":{},\"props\":{{{}}}}}",
        serde_json::to_string(&o.id).unwrap(),
        kind_wire(o.kind),
        o.x,
        o.y,
        o.w,
        o.h,
        opt_str(&o.from),
        opt_str(&o.to),
        members.join(","),
        points.join(","),
        opt_str(&o.color),
        width,
        tool,
        serde_json::to_string(&o.z_index).unwrap(),
        props.join(","),
    )
}

/// 回执 → JSON（outcome 扁平化 + undo 面）。
pub fn receipt_json(outcome: &CommandOutcome, undo_depth: usize, undo_cleared: bool) -> String {
    let (kind, key, val) = match outcome {
        CommandOutcome::NodeAdded { id } => ("node_added", "id", id.clone()),
        CommandOutcome::NodeMoved { id } => ("node_moved", "id", id.clone()),
        CommandOutcome::NodeResized { id } => ("node_resized", "id", id.clone()),
        CommandOutcome::EdgeAdded { id } => ("edge_added", "id", id.clone()),
        CommandOutcome::PropsSet { id } => ("props_set", "id", id.clone()),
        CommandOutcome::ObjectRemoved { id } => ("object_removed", "id", id.clone()),
        CommandOutcome::Grouped { group_id } => ("grouped", "group_id", group_id.clone()),
        CommandOutcome::Ungrouped { group_id } => ("ungrouped", "group_id", group_id.clone()),
        CommandOutcome::StrokeAdded { id } => ("stroke_added", "id", id.clone()),
        CommandOutcome::StrokeStyled { id } => ("stroke_styled", "id", id.clone()),
        CommandOutcome::Reordered { id } => ("reordered", "id", id.clone()),
    };
    format!(
        "{{\"outcome\":\"{kind}\",\"{key}\":{},\"undo_depth\":{undo_depth},\"undo_cleared\":{undo_cleared}}}",
        serde_json::to_string(&val).unwrap()
    )
}

/// 事件 → JSON（updates base64——可独立 apply_update 转发）。
pub fn event_json(
    source: &mdc_canvas::CanvasChangeSource,
    updates: &[Vec<u8>],
    touched: &[String],
) -> String {
    let (source_s, actor) = match source {
        mdc_canvas::CanvasChangeSource::Command { actor }
        | mdc_canvas::CanvasChangeSource::Undo { actor }
        | mdc_canvas::CanvasChangeSource::Redo { actor } => {
            let s = match source {
                mdc_canvas::CanvasChangeSource::Command { .. } => "command",
                mdc_canvas::CanvasChangeSource::Undo { .. } => "undo",
                _ => "redo",
            };
            (
                s,
                if matches!(actor, Actor::Human) {
                    "human"
                } else {
                    "ai"
                },
            )
        }
        mdc_canvas::CanvasChangeSource::Remote => ("remote", ""),
        mdc_canvas::CanvasChangeSource::Maintenance => ("maintenance", ""),
    };
    let ups: Vec<String> = updates
        .iter()
        .map(|u| format!("\"{}\"", b64_encode(u)))
        .collect();
    let ts: Vec<String> = touched
        .iter()
        .map(|t| serde_json::to_string(t).unwrap())
        .collect();
    let actor_field = if actor.is_empty() {
        String::new()
    } else {
        format!(",\"actor\":\"{actor}\"")
    };
    format!(
        "{{\"source\":\"{source_s}\"{actor_field},\"updates\":[{}],\"touched\":[{}]}}",
        ups.join(","),
        ts.join(",")
    )
}

fn parse_actor(s: &str) -> Result<Actor, CvError> {
    match s {
        "human" => Ok(Actor::Human),
        "ai" => Ok(Actor::Ai),
        other => Err(bad_json("actor", format!("unknown `{other}` (human|ai)"))),
    }
}

// ---------------------------------------------------------------------------
// wasm-bindgen 面
// ---------------------------------------------------------------------------

/// 画布会话句柄（模型层单一真相——领域模型/命令/LWW/双栈撤销/指纹/事件）。
#[wasm_bindgen]
pub struct WasmCanvas {
    session: CanvasSession,
    /// 宿主时钟（AtomicU64 + set_clock_ms 推进——wasm 无系统时间；
    /// Send+Sync 满足 SessionOptions 边界，单线程无争用）。
    clock: Arc<AtomicU64>,
    /// 事件缓冲（Mutex 累积；take 排空——订阅闭包 Send 边界）。
    events: Arc<std::sync::Mutex<Vec<String>>>,
    /// 订阅保持（Drop = 退订——字段顺序：drop 先于 session）。
    _sub: mdc_canvas::Subscription,
}

#[wasm_bindgen]
impl WasmCanvas {
    /// 新会话（client_id 建议全局唯一——自动 ID `{client_id}-{seq}`）。
    #[wasm_bindgen(constructor)]
    pub fn new(client_id: u64, undo_limit: Option<usize>) -> Result<WasmCanvas, JsValue> {
        let clock = Arc::new(AtomicU64::new(0));
        let clock_for_options = clock.clone();
        let options = SessionOptions {
            client_id,
            undo_stack_limit: undo_limit,
            clock_ms: Some(Arc::new(move || clock_for_options.load(Ordering::SeqCst))),
            ..Default::default()
        };
        Self::build(options, clock)
    }

    /// 快照恢复（撤销栈不随快照走——既有语义）。
    #[wasm_bindgen(static_method_of = WasmCanvas)]
    pub fn restore(
        client_id: u64,
        undo_limit: Option<usize>,
        snapshot: &[u8],
    ) -> Result<WasmCanvas, JsValue> {
        let clock = Arc::new(AtomicU64::new(0));
        let clock_for_options = clock.clone();
        let options = SessionOptions {
            client_id,
            undo_stack_limit: undo_limit,
            clock_ms: Some(Arc::new(move || clock_for_options.load(Ordering::SeqCst))),
            ..Default::default()
        };
        let mut session =
            CanvasSession::restore(options, snapshot).map_err(|e| CvError::from(e).to_js())?;
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sub = subscribe_bridge(&mut session, &events);
        Ok(WasmCanvas {
            session,
            clock,
            events,
            _sub: sub,
        })
    }

    fn build(options: SessionOptions, clock: Arc<AtomicU64>) -> Result<WasmCanvas, JsValue> {
        let mut session = CanvasSession::open(options).map_err(|e| CvError::from(e).to_js())?;
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sub = subscribe_bridge(&mut session, &events);
        Ok(WasmCanvas {
            session,
            clock,
            events,
            _sub: sub,
        })
    }

    /// 宿主时钟注入（epoch ms；撤销栈时间戳源——单调推进纪律）。
    pub fn set_clock_ms(&self, ms: u64) {
        self.clock.store(ms, Ordering::SeqCst);
    }

    /// 命令执行（八命令 flat JSON；回执 JSON）。
    pub fn execute(&mut self, actor: &str, cmd_json: &str) -> Result<String, JsValue> {
        let actor = parse_actor(actor).map_err(|e| e.to_js())?;
        let cmd = parse_cmd_core(cmd_json).map_err(|e| e.to_js())?;
        let receipt = self
            .session
            .execute(actor, cmd)
            .map_err(|e| CvError::from(e).to_js())?;
        Ok(receipt_json(
            &receipt.outcome,
            receipt.undo_depth,
            receipt.undo_cleared,
        ))
    }

    /// 撤销（按 origin 分栈）。
    pub fn undo(&mut self, actor: &str) -> Result<bool, JsValue> {
        let actor = parse_actor(actor).map_err(|e| e.to_js())?;
        Ok(self.session.undo(actor))
    }

    /// 重做。
    pub fn redo(&mut self, actor: &str) -> Result<bool, JsValue> {
        let actor = parse_actor(actor).map_err(|e| e.to_js())?;
        Ok(self.session.redo(actor))
    }

    pub fn undo_depth(&self, actor: &str) -> Result<usize, JsValue> {
        Ok(self
            .session
            .undo_depth(parse_actor(actor).map_err(|e| e.to_js())?))
    }

    pub fn redo_depth(&self, actor: &str) -> Result<usize, JsValue> {
        Ok(self
            .session
            .redo_depth(parse_actor(actor).map_err(|e| e.to_js())?))
    }

    /// 读模型快照（ID 字典序确定性）。
    pub fn objects_json(&self) -> String {
        let objs: Vec<String> = self.session.objects().iter().map(object_json).collect();
        format!("[{}]", objs.join(","))
    }

    /// 单对象（`Option<String>` → JS `string | null`——不存在 = null，
    /// 与宿主侧 `raw === null` 检查契约直配）。
    pub fn object_json(&self, id: &str) -> Option<String> {
        self.session.find_object(id).map(|o| object_json(&o))
    }

    /// 对象语义指纹（证据关联锚——"fnv64:{:016x}"）。
    pub fn object_fingerprint(&self, id: &str) -> Result<String, JsValue> {
        self.session
            .object_fingerprint(id)
            .map_err(|e| CvError::from(e).to_js())
    }

    /// 全画布指纹。
    pub fn canvas_fingerprint(&self) -> String {
        self.session.canvas_fingerprint()
    }

    /// 协同：状态向量。
    pub fn state_vector(&self) -> Vec<u8> {
        self.session.state_vector()
    }

    /// 协同：差量（remote_sv 之后的新事务 wire 字节）。
    pub fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.session
            .encode_diff(remote_sv)
            .map_err(|e| CvError::from(e).to_js())
    }

    /// 协同：应用远端更新（panic 隔离；失败仍可重试广播——CRDT 幂等）。
    pub fn apply_update(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.session
            .apply_update(bytes)
            .map_err(|e| CvError::from(e).to_js())
    }

    /// 权威快照（二进制 wire——持久化由宿主决定）。
    pub fn authoritative_snapshot(&self) -> Result<Vec<u8>, JsValue> {
        self.session
            .authoritative_snapshot()
            .map_err(|e| CvError::from(e).to_js())
    }

    /// 事件轮询（排空式）：[{source,actor?,updates:<base64>[],touched:[]}]。
    pub fn take_events_json(&self) -> String {
        let mut buf = self.events.lock().unwrap();
        let out = format!("[{}]", buf.join(","));
        buf.clear();
        out
    }
}

/// 订阅桥：CanvasEvent → JSON 累积（Session 保持不可变借用——
/// 订阅在 open/restore 后立即挂载）。
fn subscribe_bridge(
    session: &mut CanvasSession,
    events: &Arc<std::sync::Mutex<Vec<String>>>,
) -> mdc_canvas::Subscription {
    let sink = events.clone();
    session.subscribe(move |ev: &mdc_canvas::CanvasEvent| {
        if let Ok(mut buf) = sink.lock() {
            buf.push(event_json(&ev.source, &ev.updates, &ev.touched));
        }
    })
}
