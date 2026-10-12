//! mdcore-wasm — Headless Core v0 的 WASM 绑定。
//!
//! 桥语义（R7 报告 §5 + R9 web/）：WebView 内 CM6 View ↔ 本核为权威状态；
//! 通信 = op JSON 流 + 事件轮询（take_events），与 spike/b 的
//! 快路径（apply_ops）+ 全量重同步兜底（sync_from_markdown）两层对接。
//!
//! 所有方法入参/出参均为 JSON 字符串（wasm-bindgen 边界保持零宏魔法）。
//! u64 往返为 JS BigInt（revision / id 由宿主按需 Number() 化）。

use std::sync::Arc;

use wasm_bindgen::prelude::*;

use mdcore::anchor::TextTarget;
use mdcore::history::Source;
use mdcore::ApplyMeta;

#[wasm_bindgen]
pub struct WasmCore {
    inner: mdcore::Core,
}

#[wasm_bindgen]
impl WasmCore {
    #[wasm_bindgen(constructor)]
    pub fn new(markdown: Option<String>) -> WasmCore {
        let inner = match markdown {
            Some(md) => mdcore::Core::from_markdown(&md),
            None => mdcore::Core::new(),
        };
        WasmCore { inner }
    }

    /// 应用一批 op（JSON 数组）。op 形状见 mdcore::op::Op 的 serde。
    /// 失败返回 Err(JSON)（Reject 序列化）。
    pub fn apply_ops(
        &mut self,
        ops_json: &str,
        source: &str,
        command: Option<String>,
    ) -> Result<String, JsValue> {
        let ops: Vec<mdcore::op::Op> = serde_json::from_str(ops_json)
            .map_err(|e| JsValue::from_str(&format!("bad ops json: {e}")))?;
        let source = parse_source(source)?;
        let meta = ApplyMeta { source, command };
        match self.inner.apply_ops(ops, meta) {
            Ok(r) => Ok(serde_json::json!({ "revision": r.revision }).to_string()),
            Err(reject) => Err(JsValue::from_str(&serialize_reject(&reject))),
        }
    }

    /// 文本命令：锚点寻址（AI 路径）。expect 为可选内容保护。
    pub fn replace_text_at_anchor(
        &mut self,
        block_id: u64,
        start: usize,
        end: usize,
        insert: &str,
        expect: Option<String>,
    ) -> Result<String, JsValue> {
        let target = TextTarget::ByAnchor {
            block: mdcore::id::BlockId(block_id),
            start,
            end,
            expect,
        };
        match self.inner.replace_text(target, insert.to_string()) {
            Ok(r) => Ok(serde_json::json!({ "revision": r.revision }).to_string()),
            Err(reject) => Err(JsValue::from_str(&serialize_reject(&reject))),
        }
    }

    pub fn undo(&mut self) -> Option<String> {
        self.inner
            .undo()
            .map(|r| serde_json::json!({ "revision": r.revision }).to_string())
    }

    pub fn redo(&mut self) -> Option<String> {
        self.inner
            .redo()
            .map(|r| serde_json::json!({ "revision": r.revision }).to_string())
    }

    /// 取走事件队列（op 流），JSON 数组。
    pub fn take_events(&mut self) -> String {
        let events = self.inner.take_events();
        serde_json::to_string(&events).unwrap_or_else(|_| "[]".into())
    }

    pub fn to_markdown(&self) -> String {
        mdcore::markdown::tree_to_markdown(&self.inner.tree)
    }

    /// 机器可读块树平铺列表（先序含子块，`depth` 标 nesting）。
    /// 形如 [{"id":1,"type":"paragraph","depth":0,"attrs":{},"text":"..."},...]
    /// —— JS 侧用它 + to_markdown() 重建 doc offset ↔ block 映射（BlockMap）。
    pub fn blocks_json(&self) -> String {
        mdcore::debug::blocks_json(&self.inner.tree)
    }

    /// 先序块总数（R16：= blocks_json 列表长度）。分片游标基准，
    /// 免去为取计数而全量 dump（10MB 文档 20 万块）。
    pub fn blocks_count(&self) -> usize {
        mdcore::debug::blocks_count(&self.inner.tree)
    }

    /// R20 内存审计：组件级活数据 JSON（宿主内存面板数据源）。
    /// tree（per-block Rope/slots/id_index）/log（op 流克隆）/history（undo 双份）
    /// /events_pending，逐项字节估计——配合 JS 侧 __wasmMemory.buffer.byteLength
    /// 可分辨活数据与分配器高水位。
    pub fn core_mem_stats(&self) -> String {
        self.inner.mem_stats().to_string()
    }

    /// 先序区间 [from, from+count) 的块列表 JSON（R16 分片出口）。
    /// 渐进 extend 只在文档尾部追加完整子树，JS 侧持先序游标即可增量
    /// 建图（BlockMap.extend），不再每次追加全量 dump + parse。
    /// 越界截断；from ≥ 总数 → "[]"。
    pub fn blocks_json_range(&self, from: usize, count: usize) -> String {
        mdcore::debug::blocks_json_range(&self.inner.tree, from, count)
    }

    /// 全量重同步兜底（R9）：把 CM6 当前 doc 文本交给核，核内做按位置
    /// 朴素差分 → 最小 op 序列 → apply_ops 管道（history/事件/revision 一致）。
    /// 返回 {"revision":N}；拒绝时 Err(JSON Reject)。
    pub fn sync_from_markdown(&mut self, md: &str) -> Result<String, JsValue> {
        match self.inner.sync_from_markdown(md) {
            Ok(r) => Ok(serde_json::json!({ "revision": r.revision }).to_string()),
            Err(reject) => Err(JsValue::from_str(&serialize_reject(&reject))),
        }
    }

    /// 渐进打开（R10-a）：追加式吃入文档后缀 chunk（宿主分块器负责在
    /// 块边界切割）。返回 {"revision":N}；悬挂 chunk（未闭合 fence）
    /// Err(JSON Reject，IncompleteChunk)，树零污染。
    pub fn extend_from_markdown(&mut self, chunk: &str) -> Result<String, JsValue> {
        match self.inner.extend_from_markdown(chunk) {
            Ok(r) => Ok(serde_json::json!({ "revision": r.revision }).to_string()),
            Err(reject) => Err(JsValue::from_str(&serialize_reject(&reject))),
        }
    }

    /// 文档身份（§8.3）：hex 字符串或 None（未设置）。
    pub fn document_id(&self) -> Option<String> {
        self.inner.document_id().map(|id| id.to_string())
    }

    /// 设置文档身份：None = 用注入时钟现场生成一枚；Some(hex) = 显式指定
    /// （非法格式/与已持值不同 → Err，幂等同值 → Ok）。
    pub fn set_document_id(&mut self, id: Option<String>) -> Result<(), JsValue> {
        let parsed = match id {
            None => None,
            Some(s) => Some(
                s.parse::<mdcore::document::DocumentId>()
                    .map_err(|e| JsValue::from_str(&e.to_string()))?,
            ),
        };
        self.inner
            .set_document_id(parsed)
            .map_err(|reject| JsValue::from_str(&serialize_reject(&reject)))
    }

    /// 宿主时钟注入：wasm32 无 SystemTime，合并窗 / 历史时间戳由 JS 提供
    /// （Date.now()）。每次变异调用前设置一次即可。
    pub fn set_clock_ms(&mut self, ms: u64) {
        self.inner.set_clock(Arc::new(move || ms));
    }

    pub fn debug_tree(&self) -> String {
        mdcore::debug::dump_tree(&self.inner.tree)
    }

    pub fn history_json(&self) -> String {
        self.inner.history_json()
    }

    pub fn set_history_json(&mut self, json: &str) -> Result<(), JsValue> {
        self.inner
            .set_history_json(json)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn snapshot(&self) -> String {
        self.inner.snapshot().to_json()
    }

    #[wasm_bindgen(static_method_of = WasmCore)]
    pub fn restore(snapshot_json: &str) -> Result<WasmCore, JsValue> {
        let snap = mdcore::snapshot::Snapshot::from_json(snapshot_json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(WasmCore {
            inner: mdcore::Core::restore(snap),
        })
    }

    #[wasm_bindgen(static_method_of = WasmCore)]
    pub fn replay(jsonl: &str) -> Result<WasmCore, JsValue> {
        mdcore::Core::replay_jsonl(jsonl)
            .map(|inner| WasmCore { inner })
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn revision(&self) -> u64 {
        self.inner.revision()
    }
}

fn serialize_reject(reject: &mdcore::Reject) -> String {
    serde_json::to_string(reject).unwrap_or_else(|_| "\"invalid\"".into())
}

/// T3 复用地基轮：source 字符串 → Source 的**封闭映射**（typed reject）。
///
/// 修复前缺陷（审计 P3 G3/U2）：`_ => Source::View` 静默兜底——非法
/// source 字符串被错标为人手编辑（View），污染 Human/AI 溯源（数据
/// 正确性级缺口）。修复后五合法值显式映射，非法值显式拒绝；合法值
/// 集合与 CONTRACT §3 面零变化（仅收紧未定义的非法输入路径）。
fn parse_source(source: &str) -> Result<Source, JsValue> {
    parse_source_str(source).map_err(|e| JsValue::from_str(&e))
}

/// 纯函数内核（host 可测——JsValue 在非 wasm 目标的构造即 panic，
/// 故拒绝路径的测试走本函数；JsValue 包装由 wasm32 + web 三门禁背书）。
fn parse_source_str(source: &str) -> Result<Source, String> {
    match source {
        "view" => Ok(Source::View),
        "ai" => Ok(Source::Ai),
        "undo" => Ok(Source::Undo),
        "redo" => Ok(Source::Redo),
        "system" => Ok(Source::System),
        other => Err(format!(
            "bad source: {other:?} (legal: view/ai/undo/redo/system)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_source_str;
    use mdcore::history::Source;

    #[test]
    fn five_legal_sources_map_explicitly() {
        assert_eq!(parse_source_str("view").unwrap(), Source::View);
        assert_eq!(parse_source_str("ai").unwrap(), Source::Ai);
        assert_eq!(parse_source_str("undo").unwrap(), Source::Undo);
        assert_eq!(parse_source_str("redo").unwrap(), Source::Redo);
        assert_eq!(parse_source_str("system").unwrap(), Source::System);
    }

    #[test]
    fn unknown_source_is_rejected_not_silently_view() {
        // 修复前行为：非法串静默兜底为 View（溯源污染）。现必须显式拒绝。
        for bad in ["", "human", "Human", "VIEW", "external", "ai "] {
            assert!(
                parse_source_str(bad).is_err(),
                "source={bad:?} 必须拒绝——静默兜底为 View 会污染 Human/AI 溯源"
            );
        }
    }
}
