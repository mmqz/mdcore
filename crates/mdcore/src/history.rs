//! History — op 栈式 undo/redo（多参考实现行为互证：可序列化）。
//!
//! - 合并规则（spike core.ts 直译 + ADR-B2 v0 子集）：同为 text 类 + View 源 +
//!   1s 时间窗内 → 并入最后一条（每次新条目清空 redo 栈）。
//! - R12 收紧：合并要求**条目两端都是 View**（incoming source 与 last_source）。
//!   此前只查 incoming 侧，AI/System 事务后的 1s 内 Human 击键会被吸进
//!   AI 条目——一次 undo 连人手字符一起吞掉，违背 R10 验证点"undo 回退
//!   AI 整批而不吞 Human 编辑"。
//! - 语义破组：kind 切换（文本↔结构）天然破组；undo/redo 后 last_kind 置空。
//! - 词边界/输入类型破组、AI 组粒度 → ADR-B1/B2 留档，R9 实测调参。
//! - R21 / ADR-B4：undo 栈 = 交互编辑史——System 事务不入栈
//!   （`note_boundary` 只推进合并窗边界，调用方跳过 push/coalesce）。
//! - 序列化：整栈 JSON 往返（S6），entry.ops/inverses 均为物化 op（ID 具体）。

use serde::{Deserialize, Serialize};

use crate::anchor::Selection;
use crate::op::Op;

pub const COALESCE_MS: u64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    View,
    Undo,
    Redo,
    Ai,
    /// 核自驱（R10-a：渐进打开 extend_from_markdown）——非人手打、非 AI
    /// 编辑指令，借用 Ai/View 都会污染合并窗语义（System 永不 coalesce）。
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Text,
    Structural,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub revision: u64,
    pub ops: Vec<Op>,
    /// 与 ops 逆序对应（undo 按数组顺序应用）。
    pub inverses: Vec<Op>,
    pub selection: Option<Selection>,
    #[serde(default)]
    pub command: Option<String>,
    pub at_ms: u64,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    #[serde(default)]
    last_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_kind: Option<Kind>,
    /// 最后一条条目的来源（R12）：合并窗要求两端都是 View 击键，
    /// AI/System/undo 边界后的 1s 内人手击键必须开新条目。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_source: Option<Source>,
}

/// R20 内存审计：undo/redo 栈活数据估计（字节）。
#[derive(Debug, Serialize)]
pub struct HistoryMemStats {
    pub undo_entries: usize,
    pub redo_entries: usize,
    pub undo_ops: usize,
    pub redo_ops: usize,
    pub content_bytes: usize,
    pub headers_est_bytes: usize,
}

/// T1-fix F10：合并窗边界快照。pop_undo/pop_redo 除了出栈还推进边界
///（last_at_ms / last_kind=None），undo/redo 应用失败时若不拨回，
/// 调用后的状态就不再是「与调用前完全一致」（边界漂移会改变后续
/// 击键的合并分组）。undo/redo 在 pop 前取快照，失败路径拨回。
#[derive(Clone, Copy, Debug)]
pub struct BoundarySnapshot {
    at_ms: u64,
    kind: Option<Kind>,
    source: Option<Source>,
}

impl History {
    pub fn should_coalesce(&self, now_ms: u64, kind: Kind, source: Source) -> bool {
        kind == Kind::Text
            && self.last_kind == Some(Kind::Text)
            && self.last_source == Some(Source::View)
            && source == Source::View
            && now_ms.saturating_sub(self.last_at_ms) < COALESCE_MS
            && !self.undo.is_empty()
    }

    /// 新条目入栈（清空 redo）。`inverses` 必须已按逆序排好。
    /// `source` = 产生该条目的命令源（R12：随条目记入合并窗状态）。
    pub fn push_entry(&mut self, entry: Entry, source: Source) {
        self.note_boundary(entry.at_ms, entry.kind, source);
        self.redo.clear();
        self.undo.push(entry);
    }

    /// R21 / ADR-B4：只推进合并窗边界状态、不入栈。
    /// System 事务（extend_from_markdown 等核自驱装载）不进 undo 栈，
    /// 但边界必须按该事务更新——System 之后的第一个 View 击键因
    /// last_source != View 必开新条目（跨 ingest 边界不得错误合并）。
    pub fn note_boundary(&mut self, at_ms: u64, kind: Kind, source: Source) {
        self.last_at_ms = at_ms;
        self.last_kind = Some(kind);
        self.last_source = Some(source);
    }

    /// 合并进最后一条：ops 追加，inverses 前插（保持逆序不变量）。
    pub fn coalesce_into_last(
        &mut self,
        ops: Vec<Op>,
        mut inverses: Vec<Op>,
        at_ms: u64,
        source: Source,
    ) {
        if let Some(e) = self.undo.last_mut() {
            inverses.append(&mut e.inverses);
            e.inverses = inverses; // 新 inverse 在前
            e.ops.extend(ops);
            e.at_ms = at_ms;
        }
        self.last_at_ms = at_ms;
        self.last_source = Some(source);
    }

    pub fn pop_undo(&mut self) -> Option<Entry> {
        let e = self.undo.pop()?;
        self.last_at_ms = e.at_ms;
        self.last_kind = None; // undo 边界破组
        Some(e)
    }

    /// 当前合并窗边界快照（undo/redo 尝试前取）。
    pub fn boundary(&self) -> BoundarySnapshot {
        BoundarySnapshot {
            at_ms: self.last_at_ms,
            kind: self.last_kind,
            source: self.last_source,
        }
    }

    /// 拨回边界快照（T1-fix F10 失败路径专用；与条目压回原栈配套，
    /// 调用后 History 与 pop 前完全一致）。
    pub fn restore_boundary(&mut self, b: BoundarySnapshot) {
        self.last_at_ms = b.at_ms;
        self.last_kind = b.kind;
        self.last_source = b.source;
    }

    pub fn push_redo(&mut self, entry: Entry) {
        self.redo.push(entry);
    }

    pub fn pop_redo(&mut self) -> Option<Entry> {
        let e = self.redo.pop()?;
        self.last_at_ms = e.at_ms;
        self.last_kind = None;
        Some(e)
    }

    pub fn push_back_undo(&mut self, entry: Entry) {
        self.undo.push(entry);
    }

    /// 交互史整体作废（R23-c / ADR-B7：take-external 重载后，被放弃文档态的
    /// undo/redo 条目对重建后的树失去意义——按路径/身份回放会 BadPath）。
    /// 清栈 + 重置合并窗状态（last_kind/last_source/last_at_ms）：
    /// 重载后的第一个 View 击键必须开新条目，绝不与重载前状态合并。
    /// System 事务本身不入栈（ADR-B4），本方法只清交互面。
    pub fn clear_interaction(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_at_ms = 0;
        self.last_kind = None;
        self.last_source = None;
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// R20 内存审计：undo/redo 栈的 op 活数据估计（字节）。
    /// Entry 双持 ops + inverses，各按 op 内容字节累计。
    pub fn mem_stats(&self) -> HistoryMemStats {
        fn entry_bytes(entries: &[Entry]) -> (usize, usize, usize) {
            let mut ops = 0usize;
            let mut content = 0usize;
            let mut headers = 0usize;
            for e in entries {
                ops += e.ops.len() + e.inverses.len();
                headers += e.ops.len() + e.inverses.len() + 3;
                for o in e.ops.iter().chain(e.inverses.iter()) {
                    match o {
                        Op::InsertBlock {
                            parent,
                            attrs,
                            text,
                            ..
                        } => {
                            content += text.len();
                            headers += parent.len() / 4 + attrs.len();
                        }
                        Op::ReplaceText { insert, .. } => content += insert.len(),
                        Op::SetAttrs { attrs, .. } => headers += attrs.len(),
                        Op::SetMark { kind, .. } => content += kind.len(),
                        _ => {}
                    }
                }
            }
            (ops, content, headers * 24)
        }
        let (undo_ops, undo_content, undo_headers) = entry_bytes(&self.undo);
        let (redo_ops, redo_content, redo_headers) = entry_bytes(&self.redo);
        HistoryMemStats {
            undo_entries: self.undo.len(),
            redo_entries: self.redo.len(),
            undo_ops,
            redo_ops,
            content_bytes: undo_content + redo_content,
            headers_est_bytes: undo_headers + redo_headers,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn last_kind(&self) -> Option<Kind> {
        self.last_kind
    }

    /// S6：整栈 JSON（含物化 ID 的 op 与 inverse）。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("history serialize")
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        // R10 审查修正：合并窗元数据（last_at_ms/last_kind）随栈一并恢复——
        // save/open 往返要求逐字节一致；1s 窗口使跨档残留合并不可能发生。
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R21 / ADR-B4：边界更新与入栈解耦——note_boundary 只推进合并窗
    /// 状态（last_at_ms/last_kind/last_source），不触 undo/redo 栈；
    /// System 边界元数据随整栈 JSON 往返无损。
    #[test]
    fn note_boundary_updates_merge_window_without_stack_effects() {
        let mut h = History::default();
        h.push_entry(
            Entry {
                revision: 1,
                ops: vec![],
                inverses: vec![],
                selection: None,
                command: None,
                at_ms: 1_000,
                kind: Kind::Text,
            },
            Source::View,
        );
        assert_eq!(h.undo_depth(), 1);

        // 模拟 System 事务：只推进边界，不动栈
        h.note_boundary(2_000, Kind::Structural, Source::System);
        assert_eq!(h.undo_depth(), 1, "note_boundary 不入栈");
        assert_eq!(h.redo_depth(), 0);
        // System 边界后的 View 击键不合并（合并要求两端都是 View）
        assert!(!h.should_coalesce(2_500, Kind::Text, Source::View));

        // 序列化兼容：last_source=System 随整栈 JSON 恢复（S6 无损语义）
        let back = History::from_json(&h.to_json()).unwrap();
        assert!(
            !back.should_coalesce(2_500, Kind::Text, Source::View),
            "边界元数据必须随 JSON 恢复"
        );
    }
}
