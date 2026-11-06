//! Snapshot / restore — 主档 R8 第 8 项 + 语义不变量 9（Document State 完整快照）。
//!
//! - 含 revision 与 allocator 水位：restore 后新分配绝不与历史 ID 撞车。
//! - undo/redo 栈不入 snapshot（文档状态 ≠ 编辑历史）；跨会话历史恢复
//!   走 History::to_json / from_json（S6 已验证）。
//! - 结构 = 递归 SnapNode（serde 直出 JSON）；根 SnapNode 为虚根，
//!   restore 时只重建其 children（虚根由 BlockTree::new 自带）。
//! - 反序列化即校验（T1 修复）：`from_json` 不再直接信任外部 JSON——
//!   恶意/损坏 snapshot 可注入 duplicate BlockId / next_id 水位倒挂 /
//!   越界 marks / 非法 Document 子块，绕过 op 层不变量直写树内。
//!   自定义 Deserialize 先过 [`Snapshot::validate`]（pdf-b PageSpan
//!   serde 不变量旁路的同款防线：JSON 面拒收，Serialize 保持派生）；
//!   `capture` 产物恒满足全部校验（由活树不变量 + 分配器单调性保证）。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::BlockId;
use crate::tree::{Attrs, BlockKind, BlockTree, MarkSpan, NodeData, NodeId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapNode {
    pub id: u64,
    pub kind: BlockKind,
    pub text: String,
    #[serde(default)]
    pub attrs: Attrs,
    #[serde(default)]
    pub marks: Vec<MarkSpan>,
    #[serde(default)]
    pub children: Vec<SnapNode>,
}

/// serde 裸字段镜像（仅用于反序列化入口——校验后升格为 [`Snapshot`]）。
#[derive(Deserialize)]
struct RawSnapshot {
    revision: u64,
    next_id: u64,
    root: SnapNode,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub next_id: u64,
    pub root: SnapNode,
}

/// T1-fix O4：snapshot 不变量校验错误（反序列化入口拒收面）。
///
/// 修复前 `from_json` 直接反序列化 + `build` 原样重建，restore 面
///（含 wasm `restore(snapshot_json)`）可绕过 op 层对 InsertBlock /
/// ReplaceText / SetMark 的全部同类校验。变体与 op 层错误逐项对应，
/// 错误面可诊断（Display 直出，供 wasm 边界转 JsValue 字符串）。
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotError {
    /// 两个节点共用同一数值 BlockId（含与虚根保留 ID 0 撞号）。
    /// capture 产物恒唯一（分配器从 1 起号只前进不回收，§8.2）；
    /// 撞号会使 id_index 后写覆盖前写，被覆盖块从此不可寻址
    ///（anchor 面 Missing 假阴性，undo 注销连带误伤同号活块）。
    DuplicateBlockId { id: u64 },
    /// next_id 不高于树中最大数值 ID——与分配器单调语义矛盾
    ///（capture 产物恒满足 watermark > 全部已分配 ID）。build 路径
    /// create_node 的 observe 虽会自愈水位，但倒挂只能来自损坏/手改
    /// 数据，与其放行不如拒绝（对齐 op 层严格性）。
    NextIdNotAbove { next_id: u64, max_id: u64 },
    /// mark 区间反转或超出所在块文本的码点长度（op 层 SetMark 同款
    /// `start > end || end > len` → BadRange）。合法树内 span 恒满足
    /// 0 ≤ start ≤ end ≤ len_chars（start == end 为合法空区间）。
    MarkOutOfRange {
        id: u64,
        start: usize,
        end: usize,
        len: usize,
    },
    /// 非根节点携带 Document 类型：虚根保留（apply_op 的 InsertBlock /
    /// SetBlockType 均拒绝），restore 面不得绕过。
    ReservedKind { id: u64 },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::DuplicateBlockId { id } => {
                write!(f, "duplicate block id {id} in snapshot")
            }
            SnapshotError::NextIdNotAbove { next_id, max_id } => {
                write!(
                    f,
                    "snapshot next_id {next_id} not above max block id {max_id}"
                )
            }
            SnapshotError::MarkOutOfRange {
                id,
                start,
                end,
                len,
            } => {
                write!(
                    f,
                    "mark range {start}..{end} out of bounds (len {len}) on block {id}"
                )
            }
            SnapshotError::ReservedKind { id } => {
                write!(f, "block {id} uses reserved root kind document")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

impl<'de> Deserialize<'de> for Snapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawSnapshot::deserialize(deserializer)?;
        let snap = Self {
            revision: raw.revision,
            next_id: raw.next_id,
            root: raw.root,
        };
        snap.validate().map_err(serde::de::Error::custom)?;
        Ok(snap)
    }
}

impl Snapshot {
    pub fn capture(tree: &BlockTree, revision: u64) -> Self {
        Self {
            revision,
            next_id: tree.alloc.watermark(),
            root: to_snap(&tree.collect_subtree(tree.root())),
        }
    }

    /// T1-fix O4：不变量校验（`from_json` 反序列化即调，也可供宿主对
    /// 程序化构造的 snapshot 自检）。校验项与 op 层逐项对齐：
    /// (a) 全树 BlockId 唯一（含虚根——capture 根恒 ID 0，分配器从 1
    ///     起号，合法产物无撞号）；
    /// (b) next_id > 全树最大数值 ID（IdAllocator 单调性，见 id.rs）；
    /// (c) 每节点 marks 恒满足 start ≤ end ≤ 该块文本码点数（SetMark
    ///     同款，offset 单位 = char）；
    /// (d) 非根节点 kind 不得为 Document（虚根保留，apply_op 同款拒绝）。
    /// 失败 → [`SnapshotError`]；校验先于任何 build，树零污染。
    pub fn validate(&self) -> Result<(), SnapshotError> {
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        let mut max_id = 0u64;
        validate_node(&self.root, true, &mut seen, &mut max_id)?;
        if self.next_id <= max_id {
            return Err(SnapshotError::NextIdNotAbove {
                next_id: self.next_id,
                max_id,
            });
        }
        Ok(())
    }

    /// 重建整树。返回 (tree, revision)。
    pub fn into_tree(self) -> (BlockTree, u64) {
        let mut tree = BlockTree::new();
        if self.next_id > 0 {
            tree.alloc.observe(BlockId(self.next_id - 1));
        }
        let root = tree.root();
        for child in &self.root.children {
            build(&mut tree, root, child);
        }
        (tree, self.revision)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("snapshot serialize")
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

fn to_snap(data: &NodeData) -> SnapNode {
    SnapNode {
        id: data.id.0,
        kind: data.kind,
        text: data.text.clone(),
        attrs: data.attrs.clone(),
        marks: data.marks.clone(),
        children: data.children.iter().map(to_snap).collect(),
    }
}

fn build(tree: &mut BlockTree, parent: NodeId, snap: &SnapNode) {
    // #244：物化 ID 落树走撞号守卫。from_json 产物已过 validate（重复
    // ID 在反序列化边界拒收）——此 expect 仅对「程序化构造未 validate」
    // 的误用路径响亮报错（Core::restore 文档要求先自行 validate），此前
    // 为静默覆盖毒化树。
    let node = tree
        .create_node_with_id(
            snap.kind,
            &snap.text,
            snap.attrs.clone(),
            snap.marks.clone(),
            BlockId(snap.id),
        )
        .expect("snapshot rebuild: duplicate block id — from_json output is validated; programmatic snapshots must call validate() first");
    let len = tree.node(parent).children.len();
    tree.insert_child(parent, len, node)
        .expect("snapshot rebuild: append under parent");
    for child in &snap.children {
        build(tree, node, child);
    }
}

/// [`Snapshot::validate`] 的先序游走（根节点由 `is_root` 标记，豁免
/// Document 类型检查）。max_id 随游走累计全树（含虚根 ID 0）最大数值 ID。
fn validate_node(
    node: &SnapNode,
    is_root: bool,
    seen: &mut BTreeSet<u64>,
    max_id: &mut u64,
) -> Result<(), SnapshotError> {
    if !seen.insert(node.id) {
        return Err(SnapshotError::DuplicateBlockId { id: node.id });
    }
    if node.id > *max_id {
        *max_id = node.id;
    }
    if !is_root && node.kind == BlockKind::Document {
        return Err(SnapshotError::ReservedKind { id: node.id });
    }
    let len = node.text.chars().count();
    for m in &node.marks {
        if m.start > m.end || m.end > len {
            return Err(SnapshotError::MarkOutOfRange {
                id: node.id,
                start: m.start,
                end: m.end,
                len,
            });
        }
    }
    for child in &node.children {
        validate_node(child, false, seen, max_id)?;
    }
    Ok(())
}
