//! Snapshot / restore — 主档 R8 第 8 项 + 语义不变量 9（Document State 完整快照）。
//!
//! - 含 revision 与 allocator 水位：restore 后新分配绝不与历史 ID 撞车。
//! - undo/redo 栈不入 snapshot（文档状态 ≠ 编辑历史）；跨会话历史恢复
//!   走 History::to_json / from_json（S6 已验证）。
//! - 结构 = 递归 SnapNode（serde 直出 JSON）；根 SnapNode 为虚根，
//!   restore 时只重建其 children（虚根由 BlockTree::new 自带）。

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: u64,
    pub next_id: u64,
    pub root: SnapNode,
}

impl Snapshot {
    pub fn capture(tree: &BlockTree, revision: u64) -> Self {
        Self {
            revision,
            next_id: tree.alloc.watermark(),
            root: to_snap(&tree.collect_subtree(tree.root())),
        }
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
    let node = tree.create_node(
        snap.kind,
        &snap.text,
        snap.attrs.clone(),
        snap.marks.clone(),
        Some(BlockId(snap.id)),
    );
    let len = tree.node(parent).children.len();
    tree.insert_child(parent, len, node)
        .expect("snapshot rebuild: append under parent");
    for child in &snap.children {
        build(tree, node, child);
    }
}
