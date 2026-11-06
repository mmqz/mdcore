//! 原子 Op 集 — spike/core TS 契约的树化直译（主档 §10：Command Boundary 之下）。
//!
//! - 7 个原子 op；split/merge/commit_source 在命令层展开为 op 组合
//!   （spike core.ts 的 split = ReplaceText+InsertBlock 同款技巧），
//!   保证每个原子 op 都可精确求逆。
//! - `InsertBlock.id`：正常路径 None（分配新 ID）；历史/undo 物化为 Some(id)
//!   —— undo 复原时恢复原身份（§8.2 identity 硬断言的关键）。
//! - `MoveBlock.to_index` 为**摘除后语义**；inverse 可证明为
//!   (from = to_parent+[to_index], to = 原父+原下标)，见 invert。
//! - op 按 char offset 编辑（与 ropey/TS/CM6 语义对齐）。

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::tree::{Attrs, BlockKind, BlockTree, MarkSpan, NodePath, TreeError};

/// R22-a：op 文本共享类型。log 深拷贝（`ops.to_vec()`）/历史条目/事件流
/// 对同一文本只保留一份堆分配（引用计数共享）；JSON 序列化形状与
/// String 完全一致（serde "rc" feature），旧 JSONL/历史 JSON 仍可读。
pub type OpText = Arc<str>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Op {
    InsertBlock {
        parent: NodePath,
        index: usize,
        kind: BlockKind,
        #[serde(default)]
        attrs: Attrs,
        #[serde(default)]
        text: OpText,
        /// None = 现场分配新 ID；Some = 恢复原 ID（undo/redo/replay 物化）。
        #[serde(default)]
        id: Option<u64>,
    },
    RemoveBlock {
        path: NodePath,
    },
    ReplaceText {
        path: NodePath,
        start: usize,
        end: usize,
        insert: OpText,
    },
    SetBlockType {
        path: NodePath,
        kind: BlockKind,
    },
    SetAttrs {
        path: NodePath,
        attrs: Attrs,
    },
    SetMark {
        path: NodePath,
        start: usize,
        end: usize,
        kind: String,
        add: bool,
    },
    MoveBlock {
        from: NodePath,
        to_parent: NodePath,
        to_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpError {
    Tree(TreeError),
    BadRange,
    InvalidKind(&'static str),
}

impl From<TreeError> for OpError {
    fn from(e: TreeError) -> Self {
        OpError::Tree(e)
    }
}

impl Op {
    pub fn is_text_class(&self) -> bool {
        matches!(self, Op::ReplaceText { .. } | Op::SetMark { .. })
    }

    pub fn path(&self) -> &NodePath {
        match self {
            Op::InsertBlock { parent, .. } => parent,
            Op::RemoveBlock { path }
            | Op::ReplaceText { path, .. }
            | Op::SetBlockType { path, .. }
            | Op::SetAttrs { path, .. }
            | Op::SetMark { path, .. } => path,
            Op::MoveBlock { from, .. } => from,
        }
    }
}

/// 就地应用 op。InsertBlock 现场分配 ID 时**回写 op**（物化），
/// 调用方拿到的事件/历史条目携带具体 ID。
pub fn apply_op(tree: &mut BlockTree, op: &mut Op) -> Result<(), OpError> {
    match op {
        Op::InsertBlock {
            parent,
            index,
            kind,
            attrs,
            text,
            id,
        } => {
            if *kind == BlockKind::Document {
                return Err(OpError::InvalidKind("document kind is reserved for root"));
            }
            let parent_id = tree.resolve(parent).ok_or(TreeError::BadPath)?;
            let block_id = match *id {
                Some(x) => crate::id::BlockId(x),
                None => tree.alloc_id(),
            };
            *id = Some(block_id.0);
            // #244（I4/I5）：物化 ID 落树必经撞号守卫——undo 跨会话回放
            // 携旧 ID 撞上重解析存活块时，此处显式 Err（事务回滚），绝不
            // 静默覆盖 id_index。新分配路径结构性无撞（分配器单调）。
            let node =
                tree.create_node_with_id(*kind, text, attrs.clone(), Vec::new(), block_id)?;
            tree.insert_child(parent_id, *index, node)?;
        }
        Op::RemoveBlock { path } => {
            let id = tree.resolve(path).ok_or(TreeError::BadPath)?;
            tree.remove_subtree(id)?;
        }
        Op::ReplaceText {
            path,
            start,
            end,
            insert,
        } => {
            let id = tree.resolve(path).ok_or(TreeError::BadPath)?;
            let node = tree.node_mut(id);
            if *start > *end || *end > node.text.len_chars() {
                return Err(OpError::BadRange);
            }
            node.text.remove(*start..*end);
            node.text.insert(*start, insert);
            node.marks = adjust_marks(&node.marks, *start, *end, insert.chars().count());
        }
        Op::SetBlockType { path, kind } => {
            if *kind == BlockKind::Document {
                return Err(OpError::InvalidKind("cannot set block kind to document"));
            }
            let id = tree.resolve(path).ok_or(TreeError::BadPath)?;
            tree.node_mut(id).kind = *kind;
        }
        Op::SetAttrs { path, attrs } => {
            let id = tree.resolve(path).ok_or(TreeError::BadPath)?;
            tree.node_mut(id).attrs = attrs.clone();
        }
        Op::SetMark {
            path,
            start,
            end,
            kind,
            add,
        } => {
            let id = tree.resolve(path).ok_or(TreeError::BadPath)?;
            let node = tree.node_mut(id);
            if *start > *end || *end > node.text.len_chars() {
                return Err(OpError::BadRange);
            }
            let span = MarkSpan {
                start: *start,
                end: *end,
                kind: kind.clone(),
            };
            if *add {
                if !node.marks.contains(&span) {
                    node.marks.push(span);
                }
            } else {
                node.marks.retain(|s| s != &span);
            }
        }
        Op::MoveBlock {
            from,
            to_parent,
            to_index,
        } => {
            let id = tree.resolve(from).ok_or(TreeError::BadPath)?;
            let parent_id = tree.resolve(to_parent).ok_or(TreeError::BadPath)?;
            tree.move_subtree(id, parent_id, *to_index)?;
        }
    }
    Ok(())
}

/// 文本编辑后的 mark 区间映射（v0 语义，四类讨论）：
/// 1. 完全在编辑区之前 → 原样；
/// 2. 完全在编辑区之后 → 平移 delta；
/// 3. 完整包含编辑区 → span 吞下插入文本（end += delta，CM6 同款）；
/// 4. 部分重叠 → 保留头部 [s.start, start)，无头部则保留平移后的尾部。
pub fn adjust_marks(
    marks: &[MarkSpan],
    start: usize,
    end: usize,
    insert_len: usize,
) -> Vec<MarkSpan> {
    let delta = insert_len as isize - (end - start) as isize;
    let mut out = Vec::with_capacity(marks.len());
    for s in marks {
        let span = if s.end <= start {
            s.clone()
        } else if s.start >= end {
            MarkSpan {
                start: (s.start as isize + delta) as usize,
                end: (s.end as isize + delta) as usize,
                kind: s.kind.clone(),
            }
        } else if s.start <= start && s.end >= end {
            MarkSpan {
                start: s.start,
                end: (s.end as isize + delta) as usize,
                kind: s.kind.clone(),
            }
        } else if s.start < start {
            MarkSpan {
                start: s.start,
                end: start,
                kind: s.kind.clone(),
            }
        } else {
            let tail_start = (end as isize + delta) as usize;
            let tail_end = (s.end as isize + delta) as usize;
            MarkSpan {
                start: tail_start,
                end: tail_end,
                kind: s.kind.clone(),
            }
        };
        if span.end > span.start {
            out.push(span);
        }
    }
    out
}

/// 求逆（需要 state-before）。返回 op 序列（多数长度 1；
/// RemoveBlock 的逆 = 先序重建整棵子树）。
pub fn invert_op(tree_before: &BlockTree, op: &Op) -> Result<Vec<Op>, OpError> {
    match op {
        Op::InsertBlock { parent, index, .. } => Ok(vec![Op::RemoveBlock {
            path: join(parent, *index),
        }]),
        Op::RemoveBlock { path } => {
            let id = tree_before.resolve(path).ok_or(TreeError::BadPath)?;
            let data = tree_before.collect_subtree(id);
            let parent = path[..path.len() - 1].to_vec();
            let base_index = *path.last().ok_or(TreeError::BadPath)?;
            let mut ops = Vec::new();
            build_rebuild_ops(&parent, base_index, &data, &mut ops);
            Ok(ops)
        }
        Op::ReplaceText {
            path,
            start,
            end,
            insert,
        } => {
            let id = tree_before.resolve(path).ok_or(TreeError::BadPath)?;
            let node = tree_before.node(id);
            if *start > *end || *end > node.text.len_chars() {
                return Err(OpError::BadRange);
            }
            let removed: String = node.text.slice(*start..*end).into();
            Ok(vec![Op::ReplaceText {
                path: path.clone(),
                start: *start,
                end: *start + insert.chars().count(),
                insert: removed.into(),
            }])
        }
        Op::SetBlockType { path, .. } => {
            let id = tree_before.resolve(path).ok_or(TreeError::BadPath)?;
            Ok(vec![Op::SetBlockType {
                path: path.clone(),
                kind: tree_before.node(id).kind,
            }])
        }
        Op::SetAttrs { path, .. } => {
            let id = tree_before.resolve(path).ok_or(TreeError::BadPath)?;
            Ok(vec![Op::SetAttrs {
                path: path.clone(),
                attrs: tree_before.node(id).attrs.clone(),
            }])
        }
        Op::SetMark {
            path,
            start,
            end,
            kind,
            add,
        } => {
            // R24-a2 修复（求逆不对称，探针 probe_setmark_inverse 抓出）：
            // 逆的方向由 **state-before 的 span 存在性**决定——
            // - no-op（add=true 且 span 已在 / add=false 且 span 不在）→
            //   逆 = **同向**（再应用仍是 no-op，undo 恢复同态）；
            // - 有效变更 → 逆 = 反向。
            // 此前无条件翻转 add：重复加粗（no-op）后一次 undo 会把 mark
            // 删掉、移除不存在 mark（no-op）后 undo 凭空造 mark——违反
            // 「undo 精确恢复前态」不变量。
            let id = tree_before.resolve(path).ok_or(TreeError::BadPath)?;
            let node = tree_before.node(id);
            if *start > *end || *end > node.text.len_chars() {
                return Err(OpError::BadRange);
            }
            let span = MarkSpan {
                start: *start,
                end: *end,
                kind: kind.clone(),
            };
            let exists = node.marks.contains(&span);
            let inv_add = if exists == *add { *add } else { !*add };
            Ok(vec![Op::SetMark {
                path: path.clone(),
                start: *start,
                end: *end,
                kind: kind.clone(),
                add: inv_add,
            }])
        }
        Op::MoveBlock {
            from,
            to_parent,
            to_index,
        } => {
            if from.is_empty() {
                // 虚根不可移动（R11 压测逮到的边界：空路径曾致减法溢出 panic）
                return Err(OpError::Tree(TreeError::BadPath));
            }
            let new_path = join(to_parent, *to_index);
            let orig_parent = from[..from.len() - 1].to_vec();
            let orig_index = *from.last().ok_or(TreeError::BadPath)?;
            Ok(vec![Op::MoveBlock {
                from: new_path,
                to_parent: orig_parent,
                to_index: orig_index,
            }])
        }
    }
}

fn build_rebuild_ops(
    parent: &NodePath,
    index: usize,
    data: &crate::tree::NodeData,
    out: &mut Vec<Op>,
) {
    out.push(Op::InsertBlock {
        parent: parent.to_vec(),
        index,
        kind: data.kind,
        attrs: data.attrs.clone(),
        text: data.text.as_str().into(), // R22-a：&str 直达 Arc<str>，单次拷贝
        id: Some(data.id.0),             // 恢复原身份（§8.2：undo 不改写历史身份）
    });
    for (i, child) in data.children.iter().enumerate() {
        let sub = join(parent, index);
        build_rebuild_ops(&sub, i, child, out);
    }
}

fn join(path: &[usize], index: usize) -> NodePath {
    let mut p = path.to_vec();
    p.push(index);
    p
}
