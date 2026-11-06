//! mdc-collab 会话 wasm 出口（阶段 6 §九.5/§九.8——协同双端 web 化）。
//!
//! ## 定位
//! [`WasmCollabSession`] = mdc-collab headless 协作会话的 wasm 句柄
//! （与 [`crate::canvas::WasmCanvas`] 同模式同纪律）。**单一真相**：
//! 文档块模型（p/h/li/blockquote/pre/tbl/hr）、命令校验、双栈撤销
//! （origin 分栈 + 捕获窗 0——p20 缓解）、wire 同步四件套、事件、
//! 快照恢复——全部在 mdc-collab；本模块零语义逻辑，只做边界互译
//! （flat snake_case 命令 JSON ↔ `CollabCommand` + typed 错误串）。
//!
//! ## 时钟注入
//! wasm32 无 std 时间系统（`SystemTime::now()` = trap）——构造时经
//! `SessionOptions::clock_ms` 注入宿主时钟（内部 `RefCell` 纪律在
//! collab 侧为 `AtomicU64`——与 `WasmCanvas::set_clock_ms` 同款）。
//!
//! ## 事件面
//! `take_events_json()` 排空式轮询（`WasmCanvas` 先例）：订阅闭包把
//! `CollabEvent` 转 JSON 累积到内部缓冲。`updates` 为逐事务 wire
//! 字节的 base64（可独立 `apply_update` 转发——对端直连/中继直接
//! 消费）。远端应用面（`apply_update`）产 `remote` 事件——宿主
//! 转发纪律「remote 不回发」防回声。
//!
//! ## 错误面
//! typed JSON 串 `{"code":"cl_*","message":"..."}`——`CollabError`
//! 全变体一码一义（cv_* / wb_* / pdf_* 先例）。
//!
//! ## 边界登记
//! presence（在场层）与锚点侧车面**不在本出口**（传输层/锚点身份
//! 层——宿主集成轮事项，web 组合测试面零需求）；`maintenance_gc`
//! 同不暴露（维护窗由宿主策略驱动，wasm MVP 面刻意最小）。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use wasm_bindgen::prelude::*;

use mdc_collab::{
    Actor, BlockInfo, BlockKind, BlockSpec, ChangeSource, CollabCommand, CollabError,
    CollabSession, CommandOutcome, SessionOptions,
};

// ---------------------------------------------------------------------------
// 错误面
// ---------------------------------------------------------------------------

/// wasm 边界错误（typed 码——宿主可 JSON.parse）。
#[derive(Debug)]
pub struct ClError {
    pub code: &'static str,
    pub message: String,
}

impl ClError {
    fn to_js(&self) -> JsValue {
        JsValue::from_str(
            &serde_json::json!({"code": self.code, "message": self.message}).to_string(),
        )
    }
}

impl From<CollabError> for ClError {
    fn from(e: CollabError) -> Self {
        let code = match &e {
            CollabError::Limit { .. } => "cl_limit",
            CollabError::Decode(_) => "cl_codec",
            CollabError::Panic(_) => "cl_panic",
            CollabError::InvalidInput(_) => "cl_invalid_input",
            CollabError::BlockNotFound { .. } => "cl_block_not_found",
            CollabError::BadRange { .. } => "cl_bad_range",
            CollabError::NotAContainer { .. } => "cl_not_a_container",
            CollabError::KindIncompatible { .. } => "cl_kind_incompatible",
            CollabError::ReservedAttrKey { .. } => "cl_reserved_attr_key",
            CollabError::AnchorDangling { .. } => "cl_anchor_dangling",
            CollabError::SidecarFormat(_) => "cl_sidecar_format",
        };
        ClError {
            code,
            message: format!("{e}"),
        }
    }
}

fn bad_json(what: &str, detail: String) -> ClError {
    ClError {
        code: "cl_bad_json",
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

/// 块种类 wire 名（协作载体元素名域——yrs-e §2.2 映射镜像）。
fn kind_from_tag(tag: &str) -> Result<BlockKind, ClError> {
    match tag {
        "p" => Ok(BlockKind::Paragraph),
        "h" => Ok(BlockKind::Heading),
        "li" => Ok(BlockKind::ListItem),
        "blockquote" => Ok(BlockKind::Blockquote),
        "pre" => Ok(BlockKind::CodeBlock),
        "math" => Ok(BlockKind::Math),
        "tbl" => Ok(BlockKind::Table),
        "hr" => Ok(BlockKind::ThematicBreak),
        other => Err(bad_json(
            "kind",
            format!("unknown `{other}` (p|h|li|blockquote|pre|math|tbl|hr)"),
        )),
    }
}

fn kind_tag(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "p",
        BlockKind::Heading => "h",
        BlockKind::ListItem => "li",
        BlockKind::Blockquote => "blockquote",
        BlockKind::CodeBlock => "pre",
        BlockKind::Math => "math",
        BlockKind::Table => "tbl",
        BlockKind::ThematicBreak => "hr",
    }
}

fn parse_attrs(v: &serde_json::Value) -> Result<BTreeMap<String, String>, ClError> {
    let obj = v
        .as_object()
        .ok_or_else(|| bad_json("attrs", "must be an object".into()))?;
    let mut out = BTreeMap::new();
    for (k, val) in obj {
        let s = val
            .as_str()
            .ok_or_else(|| bad_json("attrs", format!("`{k}` value must be a string")))?;
        out.insert(k.clone(), s.to_string());
    }
    Ok(out)
}

fn parse_spec(v: &serde_json::Value) -> Result<BlockSpec, ClError> {
    let obj = v
        .as_object()
        .ok_or_else(|| bad_json("spec", "must be an object".into()))?;
    let kind = kind_from_tag(
        obj.get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| bad_json("spec.kind", "missing string field".into()))?,
    )?;
    let attrs = match obj.get("attrs") {
        Some(a) => parse_attrs(a)?,
        None => BTreeMap::new(),
    };
    let text = obj
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    Ok(BlockSpec { kind, attrs, text })
}

fn opt_string(
    v: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<String>, ClError> {
    match v.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(s) => s
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| bad_json(key, "must be a string or null".into())),
    }
}

fn req_u32(v: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<u32, ClError> {
    let n = v
        .get(key)
        .and_then(|x| x.as_u64())
        .ok_or_else(|| bad_json(key, "missing non-negative integer field".into()))?;
    u32::try_from(n).map_err(|_| bad_json(key, format!("{n} exceeds u32 domain")))
}

/// flat 命令 JSON → `CollabCommand`（`{"t":"...",...}` 八命令——
/// 人类与 AI 共用同一入口）。
pub fn parse_cmd_core(json: &str) -> Result<CollabCommand, ClError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| bad_json("cmd", format!("invalid JSON ({e})")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| bad_json("cmd", "must be an object".into()))?;
    let t = obj
        .get("t")
        .and_then(|t| t.as_str())
        .ok_or_else(|| bad_json("cmd.t", "missing command tag".into()))?;
    match t {
        "load_blocks" => {
            let arr = obj
                .get("blocks")
                .and_then(|b| b.as_array())
                .ok_or_else(|| bad_json("blocks", "missing array field".into()))?;
            let blocks = arr
                .iter()
                .map(parse_spec)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(CollabCommand::LoadBlocks { blocks })
        }
        "insert_block" => {
            let parent = opt_string(obj, "parent")?;
            let index = req_u32(obj, "index")?;
            let spec = parse_spec(
                obj.get("spec")
                    .ok_or_else(|| bad_json("spec", "missing object field".into()))?,
            )?;
            let id = opt_string(obj, "id")?;
            Ok(CollabCommand::InsertBlock {
                parent,
                index,
                spec,
                id,
            })
        }
        "remove_block" => {
            let block_id = obj
                .get("block_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("block_id", "missing string field".into()))?
                .to_string();
            Ok(CollabCommand::RemoveBlock { block_id })
        }
        "replace_text" => {
            let block_id = obj
                .get("block_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("block_id", "missing string field".into()))?
                .to_string();
            let start = req_u32(obj, "start")?;
            let end = req_u32(obj, "end")?;
            let insert = obj
                .get("insert")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("insert", "missing string field".into()))?
                .to_string();
            Ok(CollabCommand::ReplaceText {
                block_id,
                start,
                end,
                insert,
            })
        }
        "set_block_type" => {
            let block_id = obj
                .get("block_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("block_id", "missing string field".into()))?
                .to_string();
            let kind = kind_from_tag(
                obj.get("kind")
                    .and_then(|k| k.as_str())
                    .ok_or_else(|| bad_json("kind", "missing string field".into()))?,
            )?;
            Ok(CollabCommand::SetBlockType { block_id, kind })
        }
        "set_attrs" => {
            let block_id = obj
                .get("block_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("block_id", "missing string field".into()))?
                .to_string();
            let attrs = parse_attrs(
                obj.get("attrs")
                    .ok_or_else(|| bad_json("attrs", "missing object field".into()))?,
            )?;
            Ok(CollabCommand::SetAttrs { block_id, attrs })
        }
        "set_mark" => {
            let block_id = obj
                .get("block_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("block_id", "missing string field".into()))?
                .to_string();
            let start = req_u32(obj, "start")?;
            let end = req_u32(obj, "end")?;
            let mark = obj
                .get("mark")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("mark", "missing string field".into()))?
                .to_string();
            let add = obj
                .get("add")
                .and_then(|b| b.as_bool())
                .ok_or_else(|| bad_json("add", "missing boolean field".into()))?;
            Ok(CollabCommand::SetMark {
                block_id,
                start,
                end,
                mark,
                add,
            })
        }
        "move_block" => {
            let block_id = obj
                .get("block_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| bad_json("block_id", "missing string field".into()))?
                .to_string();
            let to_parent = opt_string(obj, "to_parent")?;
            let to_index = req_u32(obj, "to_index")?;
            Ok(CollabCommand::MoveBlock {
                block_id,
                to_parent,
                to_index,
            })
        }
        other => Err(bad_json(
            "cmd.t",
            format!(
                "unknown `{other}` (load_blocks|insert_block|remove_block|replace_text|set_block_type|set_attrs|set_mark|move_block)"
            ),
        )),
    }
}

pub fn receipt_json(outcome: &CommandOutcome, undo_depth: usize, undo_cleared: bool) -> String {
    let (kind, payload) = match outcome {
        CommandOutcome::BlocksLoaded { ids } => (
            "blocks_loaded",
            format!(
                "\"ids\":{}",
                serde_json::to_string(ids).unwrap_or_else(|_| "[]".into())
            ),
        ),
        CommandOutcome::BlockInserted { block_id }
        | CommandOutcome::BlockRemoved { block_id }
        | CommandOutcome::TypeChanged { block_id }
        | CommandOutcome::AttrsSet { block_id }
        | CommandOutcome::BlockMoved { block_id } => (
            match outcome {
                CommandOutcome::BlockInserted { .. } => "block_inserted",
                CommandOutcome::BlockRemoved { .. } => "block_removed",
                CommandOutcome::TypeChanged { .. } => "type_changed",
                CommandOutcome::AttrsSet { .. } => "attrs_set",
                _ => "block_moved",
            },
            format!(
                "\"block_id\":{}",
                serde_json::to_string(block_id).unwrap_or_else(|_| "\"\"".into())
            ),
        ),
        CommandOutcome::TextReplaced { block_id, new_len } => (
            "text_replaced",
            format!(
                "\"block_id\":{},\"new_len\":{new_len}",
                serde_json::to_string(block_id).unwrap_or_else(|_| "\"\"".into())
            ),
        ),
        CommandOutcome::MarkSet { block_id, add } => (
            "mark_set",
            format!(
                "\"block_id\":{},\"add\":{add}",
                serde_json::to_string(block_id).unwrap_or_else(|_| "\"\"".into())
            ),
        ),
    };
    format!(
        "{{\"outcome\":\"{kind}\",{payload},\"undo_depth\":{undo_depth},\"undo_cleared\":{undo_cleared}}}"
    )
}

/// 事件 → JSON（updates base64——可独立 apply_update 转发）。
pub fn event_json(source: &ChangeSource, updates: &[Vec<u8>]) -> String {
    let (source_s, actor) = match source {
        ChangeSource::Command { actor }
        | ChangeSource::Undo { actor }
        | ChangeSource::Redo { actor } => {
            let s = match source {
                ChangeSource::Command { .. } => "command",
                ChangeSource::Undo { .. } => "undo",
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
        ChangeSource::Remote => ("remote", ""),
        ChangeSource::Maintenance => ("maintenance", ""),
    };
    let ups: Vec<String> = updates
        .iter()
        .map(|u| format!("\"{}\"", b64_encode(u)))
        .collect();
    let actor_field = if actor.is_empty() {
        String::new()
    } else {
        format!(",\"actor\":\"{actor}\"")
    };
    format!(
        "{{\"source\":\"{source_s}\"{actor_field},\"updates\":[{}]}}",
        ups.join(",")
    )
}

pub fn block_json(b: &BlockInfo) -> String {
    let attrs: Vec<String> = b
        .attrs
        .iter()
        .map(|(k, v)| {
            format!(
                "{}:{}",
                serde_json::to_string(k).unwrap(),
                serde_json::to_string(v).unwrap()
            )
        })
        .collect();
    let marks: Vec<String> = b
        .marks
        .iter()
        .map(|m| {
            format!(
                "{{\"start\":{},\"end\":{},\"mark\":{}}}",
                m.start,
                m.end,
                serde_json::to_string(&m.mark).unwrap()
            )
        })
        .collect();
    let children: Vec<String> = b.children.iter().map(block_json).collect();
    format!(
        "{{\"block_id\":{},\"kind\":\"{}\",\"attrs\":{{{}}},\"text\":{},\"marks\":[{}],\"children\":[{}]}}",
        serde_json::to_string(&b.block_id).unwrap(),
        kind_tag(b.kind),
        attrs.join(","),
        serde_json::to_string(&b.text).unwrap(),
        marks.join(","),
        children.join(",")
    )
}

fn parse_actor(s: &str) -> Result<Actor, ClError> {
    match s {
        "human" => Ok(Actor::Human),
        "ai" => Ok(Actor::Ai),
        other => Err(bad_json("actor", format!("unknown `{other}` (human|ai)"))),
    }
}

// ---------------------------------------------------------------------------
// wasm-bindgen 面
// ---------------------------------------------------------------------------

/// 协作会话句柄（文档块模型单一真相——命令/双栈撤销/同步四件套/事件）。
#[wasm_bindgen]
pub struct WasmCollabSession {
    session: CollabSession,
    /// 宿主时钟（AtomicU64 + set_clock_ms 推进——wasm 无系统时间）。
    clock: Arc<AtomicU64>,
    /// 事件缓冲（Mutex 累积；take 排空——订阅闭包 Send 边界）。
    events: Arc<std::sync::Mutex<Vec<String>>>,
    /// 订阅保持（Drop = 退订——字段顺序：drop 先于 session）。
    _sub: mdc_collab::Subscription,
}

#[wasm_bindgen]
impl WasmCollabSession {
    /// 新会话（client_id 建议全局唯一——53 位域上界由引擎拒收）。
    #[wasm_bindgen(constructor)]
    pub fn new(client_id: u64, undo_limit: Option<usize>) -> Result<WasmCollabSession, JsValue> {
        let clock = Arc::new(AtomicU64::new(0));
        Self::build(Self::options(client_id, undo_limit, &clock), clock)
    }

    /// 快照恢复（撤销栈不随快照走——B3.1 既有语义）。
    #[wasm_bindgen(static_method_of = WasmCollabSession)]
    pub fn restore(
        client_id: u64,
        undo_limit: Option<usize>,
        snapshot: &[u8],
    ) -> Result<WasmCollabSession, JsValue> {
        let clock = Arc::new(AtomicU64::new(0));
        let options = Self::options(client_id, undo_limit, &clock);
        let mut session =
            CollabSession::restore(options, snapshot).map_err(|e| ClError::from(e).to_js())?;
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sub = subscribe_bridge(&mut session, &events);
        Ok(WasmCollabSession {
            session,
            clock,
            events,
            _sub: sub,
        })
    }

    fn options(
        client_id: u64,
        undo_limit: Option<usize>,
        clock: &Arc<AtomicU64>,
    ) -> SessionOptions {
        let clock_for_options = clock.clone();
        SessionOptions {
            client_id,
            undo_stack_limit: undo_limit,
            clock_ms: Some(Arc::new(move || clock_for_options.load(Ordering::SeqCst))),
            ..Default::default()
        }
    }

    fn build(options: SessionOptions, clock: Arc<AtomicU64>) -> Result<WasmCollabSession, JsValue> {
        let mut session = CollabSession::open(options).map_err(|e| ClError::from(e).to_js())?;
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sub = subscribe_bridge(&mut session, &events);
        Ok(WasmCollabSession {
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

    /// 命令执行（八命令 flat JSON；回执 JSON——人类/AI 同一入口）。
    pub fn execute(&mut self, actor: &str, cmd_json: &str) -> Result<String, JsValue> {
        let actor = parse_actor(actor).map_err(|e| e.to_js())?;
        let cmd = parse_cmd_core(cmd_json).map_err(|e| e.to_js())?;
        let receipt = self
            .session
            .execute(actor, cmd)
            .map_err(|e| ClError::from(e).to_js())?;
        Ok(receipt_json(
            &receipt.outcome,
            receipt.undo_depth,
            receipt.undo_cleared,
        ))
    }

    /// 撤销（按 origin 分栈——栈本地，永不进 wire）。
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

    /// 读模型快照（递归块树 JSON）。
    pub fn document_json(&self) -> String {
        let blocks: Vec<String> = self.session.document().iter().map(block_json).collect();
        format!("[{}]", blocks.join(","))
    }

    /// 顶层块 ID 序列（确定性）。
    pub fn top_level_ids_json(&self) -> String {
        let ids: Vec<String> = self
            .session
            .top_level_ids()
            .iter()
            .map(|id| serde_json::to_string(id).unwrap())
            .collect();
        format!("[{}]", ids.join(","))
    }

    /// 单块读回（`Option<String>` → JS `string | null`）。
    pub fn find_block_json(&self, block_id: &str) -> Option<String> {
        self.session.find_block(block_id).as_ref().map(block_json)
    }

    /// 协同：状态向量。
    pub fn state_vector(&self) -> Vec<u8> {
        self.session.state_vector()
    }

    /// 协同：差量（remote_sv 之后的新事务 wire 字节）。
    pub fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.session
            .encode_diff(remote_sv)
            .map_err(|e| ClError::from(e).to_js())
    }

    /// 协同：应用远端更新（panic 隔离；失败仍可重试广播——CRDT 幂等）。
    pub fn apply_update(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.session
            .apply_update(bytes)
            .map_err(|e| ClError::from(e).to_js())
    }

    /// 权威快照（二进制 wire——持久化由宿主决定）。
    pub fn authoritative_snapshot(&self) -> Result<Vec<u8>, JsValue> {
        self.session
            .authoritative_snapshot()
            .map_err(|e| ClError::from(e).to_js())
    }

    /// 事件轮询（排空式）：[{source,actor?,updates:<base64>[]}]。
    pub fn take_events_json(&self) -> String {
        let mut buf = self.events.lock().unwrap();
        let out = format!("[{}]", buf.join(","));
        buf.clear();
        out
    }
}

/// 订阅桥：CollabEvent → JSON 累积（Session 保持不可变借用——
/// 订阅在 open/restore 后立即挂载）。
fn subscribe_bridge(
    session: &mut CollabSession,
    events: &Arc<std::sync::Mutex<Vec<String>>>,
) -> mdc_collab::Subscription {
    let sink = events.clone();
    session.subscribe(move |ev: &mdc_collab::CollabEvent| {
        if let Ok(mut buf) = sink.lock() {
            buf.push(event_json(&ev.source, &ev.updates));
        }
    })
}
