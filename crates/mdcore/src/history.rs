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
//!   持久化格式带版本号（[`HISTORY_FORMAT_VERSION`]）：恢复路径对
//!   缺版本/旧版本/未知版本的栈降级丢弃（#220 错序 inverses 不可执行），
//!   见 [`History::from_json`] 版本闸门。

use serde::{Deserialize, Serialize};

use crate::anchor::Selection;
use crate::op::Op;

pub const COALESCE_MS: u64 = 1000;

/// 持久化 History 格式版本（#221 补充修复，承 #220）。
///
/// - **v2**（当前）：`Entry.inverses` 存储序 = 组间逆序、组内保序
///   （#220 修复后 `apply_ops` 构造，见 `Entry.inverses` 注释）。
/// - 缺失（serde default = 0）= **#220 修复前**的旧档：inverses 为各组
///   摊平后整体 `reverse()` 的形态——单 op 逆组（长度 1）下与 v2 逐位
///   相同，但非叶块删除条目（逆组长度 ≥ 2）的组内重建序被倒转：undo
///   先插子后插父，BadPath 硬失败（T1-F10 条目压回后整栈卡死）或
///   顶替路径静默错挂（undo 返回 Some 但树损坏）。旧档打开后**不可
///   执行 undo/redo**。
/// - 未来版本（> 2）：语义未知，同样不执行（前向保护——宁可弃栈
///   不赌树）。
///
/// 降级语义（[`History::from_json`]）：丢弃 undo/redo 栈与合并窗边界
/// 元数据，回落全新 History；正文 / document_id / revision 由
/// `open_document` 独立恢复，不受影响。JSON 本身畸形仍显式报错
/// （“格式旧”降级、“格式坏”拒绝，两语义分开）。
pub const HISTORY_FORMAT_VERSION: u32 = 2;

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
    /// 与 ops **组间逆序、组内保序**对应（undo 按数组顺序应用）。
    /// `invert_op` 返回逆组（多数长度 1；RemoveBlock 的逆 = 先序重建
    /// 整棵子树），组内序 = 重建发射序（父先子后），不可倒转（#220
    /// / 审计 P1-27：扁平整体反转会在非叶删除场景倒转组内序 → undo
    /// 先插子后插父 → BadPath 卡死或静默错挂）。仍为扁平 `Vec<Op>`：
    /// serde/历史 JSON 形状不变，只改序。
    pub inverses: Vec<Op>,
    pub selection: Option<Selection>,
    #[serde(default)]
    pub command: Option<String>,
    pub at_ms: u64,
    pub kind: Kind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct History {
    /// 持久化格式版本（[`HISTORY_FORMAT_VERSION`]）。序列化**无条件**
    /// 写出（不能 skip：新档缺此字段会被当作旧档降级）；反序列化缺失
    /// = 0（旧档）→ `from_json` 版本闸门降级。内存新构造恒为当前版本
    /// （`Default`），运行时方法不触碰此字段——它是格式标识，不是运行态。
    #[serde(default)]
    format_version: u32,
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

    /// 新条目入栈（清空 redo）。`inverses` 必须已按「组间逆序、组内
    /// 保序」排好（`apply_ops` 侧构造，见 Entry.inverses 注释）。
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

    /// 合并进最后一条：ops 追加，inverses 前插（undo 序 = 最新组在前）。
    /// 合并面只收文本类事务（`should_coalesce` 要求 kind==Text ⇒ 全部
    /// `is_text_class` ⇒ 逆组全长度 1），前插 = 组前插，与组语义逐位一致
    /// （#220 验证：r25 `coalesced_text_entry_undo_order_unchanged` 锁定）。
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

    /// 版本闸门：缺版本（旧档）/ 旧版本 / 未知未来版本的持久化栈
    /// 不可信——降级为全新 History（丢弃 undo/redo 栈与合并窗边界：
    /// 重载后第一击键必开新条目，不与旧会话残留合并）。JSON 畸形仍
    /// 返回 Err（显式失败语义不变）。“不可信”的实锤见
    /// [`HISTORY_FORMAT_VERSION`]：#220 前的非叶删除条目 inverses 组内
    /// 序倒转，undo 必坏。
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        // R10 审查修正：合并窗元数据（last_at_ms/last_kind）随栈一并恢复——
        // save/open 往返要求逐字节一致；1s 窗口使跨档残留合并不可能发生。
        // （版本闸门在解析成功之后：合法 v2 才走到这里恢复的语义。）
        let h: History = serde_json::from_str(json)?;
        if h.format_version != HISTORY_FORMAT_VERSION {
            return Ok(History::default());
        }
        Ok(h)
    }
}

/// 默认态 = 空 stacks + 当前格式版本（降级 / 新建共用此构造）。
/// 手写 impl（不 derive）：u32 的 derive default = 0 会被版本闸门
/// 当作旧档——内存态必须生而 v2。
// 注意：此 impl 必须与字段声明同步维护（编译器无缺字段保护）。
impl Default for History {
    fn default() -> Self {
        Self {
            format_version: HISTORY_FORMAT_VERSION,
            undo: Vec::new(),
            redo: Vec::new(),
            last_at_ms: 0,
            last_kind: None,
            last_source: None,
        }
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
