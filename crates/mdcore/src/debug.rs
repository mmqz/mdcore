//! Debug 面 — 对象级检视（可诊断性内建，不靠println考古）。
//!
//! 两个出口分工：
//! - `dump_tree` / `inspect` = 人读文本；
//! - `blocks_json` = 机器可读的块树平铺列表（R9：JS 侧 BlockMap 重建用），
//!   先序展开（含子块），`depth` 标 nesting（0 = 顶层），同序可由
//!   stack-by-depth 复原层级。

use crate::id::BlockId;
use crate::tree::{BlockTree, NodeId};

const TEXT_PREVIEW: usize = 40;

/// 机器可读块列表（先序平铺）。`type` 沿用 op serde 的 snake_case 命名。
#[derive(serde::Serialize)]
struct BlockJson {
    id: u64,
    #[serde(rename = "type")]
    kind: crate::tree::BlockKind,
    depth: usize,
    attrs: crate::tree::Attrs,
    text: String,
}

fn collect(tree: &BlockTree, id: NodeId, depth: usize, out: &mut Vec<BlockJson>) {
    let n = tree.node(id);
    out.push(BlockJson {
        id: n.id.0,
        kind: n.kind,
        depth,
        attrs: n.attrs.clone(),
        text: n.text.to_string(),
    });
    for &c in &n.children {
        collect(tree, c, depth + 1, out);
    }
}

pub fn blocks_json(tree: &BlockTree) -> String {
    let mut out = Vec::new();
    for &child in tree.top_level() {
        collect(tree, child, 0, &mut out);
    }
    serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
}

/// 先序块总数（= `blocks_json` 平铺列表长度）。R16：JS 侧分片游标基准，
/// 免去为取一个计数而全量 dump 20 万块。
pub fn blocks_count(tree: &BlockTree) -> usize {
    fn walk(tree: &BlockTree, id: NodeId, out: &mut usize) {
        *out += 1;
        for &c in &tree.node(id).children {
            walk(tree, c, out);
        }
    }
    let mut n = 0;
    for &child in tree.top_level() {
        walk(tree, child, &mut n);
    }
    n
}

/// 先序区间 `[from, from+count)` 的块列表 JSON（R16 分片出口）。
///
/// 语义与 `blocks_json` 的对应切片逐字段一致（同一 BlockJson 形状）：
/// 渐进 extend 只追加完整子树到文档尾部，故 JS 侧持先序游标即可增量
/// 建图，不再为每次追加全量 dump + parse（10MB 文档每次 ~20 万块对象）。
/// 越界截断：`from >= blocks_count` → `"[]"`；`from+count` 超尾则收到尾。
pub fn blocks_json_range(tree: &BlockTree, from: usize, count: usize) -> String {
    fn walk(
        tree: &BlockTree,
        id: NodeId,
        depth: usize,
        cursor: &mut usize,
        from: usize,
        remaining: &mut usize,
        out: &mut Vec<BlockJson>,
    ) {
        if *remaining == 0 {
            return;
        }
        let idx = *cursor;
        *cursor += 1;
        if idx >= from {
            let n = tree.node(id);
            out.push(BlockJson {
                id: n.id.0,
                kind: n.kind,
                depth,
                attrs: n.attrs.clone(),
                text: n.text.to_string(),
            });
            *remaining -= 1;
        }
        for &c in &tree.node(id).children {
            if *remaining == 0 {
                break;
            }
            walk(tree, c, depth + 1, cursor, from, remaining, out);
        }
    }
    let mut out = Vec::new();
    let mut cursor = 0;
    let mut remaining = count;
    for &child in tree.top_level() {
        if remaining == 0 {
            break;
        }
        walk(tree, child, 0, &mut cursor, from, &mut remaining, &mut out);
    }
    serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
}

pub fn dump_tree(tree: &BlockTree) -> String {
    let mut out = String::new();
    for (i, &child) in tree.top_level().iter().enumerate() {
        dump_node(tree, child, 0, &format!("[{i}]"), &mut out);
    }
    out
}

fn dump_node(tree: &BlockTree, id: NodeId, depth: usize, pos: &str, out: &mut String) {
    let n = tree.node(id);
    let indent = "  ".repeat(depth);
    let text: String = n.text.to_string();
    let preview: String = text.chars().take(TEXT_PREVIEW).collect();
    let ellipsis = if text.chars().count() > TEXT_PREVIEW {
        "…"
    } else {
        ""
    };
    out.push_str(&format!(
        "{indent}{pos} {} {:?} attrs={:?} marks={} text={:?}{}\n",
        n.id.debug_label(),
        n.kind,
        n.attrs,
        n.marks.len(),
        preview,
        ellipsis,
    ));
    for (i, &c) in n.children.iter().enumerate() {
        dump_node(tree, c, depth + 1, &format!("[{i}]"), out);
    }
}

pub fn inspect(tree: &BlockTree, id: BlockId) -> Option<String> {
    let slot = tree.lookup_id(id)?;
    let path = tree.path_of(slot)?;
    let n = tree.node(slot);
    Some(format!(
        "{} path={:?} kind={:?} attrs={:?} marks={:?} children={} text={:?}",
        n.id.debug_label(),
        path,
        n.kind,
        n.attrs,
        n.marks,
        n.children.len(),
        n.text.to_string(),
    ))
}
