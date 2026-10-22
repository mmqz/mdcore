//! 集成测试共用件（bridge 三套件共享）。

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use mdc_collab::SessionOptions;
use mdc_mdbridge::MdCollabHost;
use mdcore::tree::{BlockKind, BlockTree, MarkSpan, NodeId};
use mdcore::{markdown, op::Op};

pub fn opts(client_id: u64) -> SessionOptions {
    SessionOptions {
        client_id,
        ..SessionOptions::default()
    }
}

pub fn host(client_id: u64) -> MdCollabHost {
    MdCollabHost::open(opts(client_id)).expect("host open")
}

/// 双向同步到收敛（A→B + B→A）。
pub fn sync(a: &mut MdCollabHost, b: &mut MdCollabHost) {
    let sv_a = a.state_vector();
    let sv_b = b.state_vector();
    let diff_ab = a.encode_diff(&sv_b).expect("diff a→b");
    let diff_ba = b.encode_diff(&sv_a).expect("diff b→a");
    a.apply_remote(&diff_ba).expect("a applies b");
    b.apply_remote(&diff_ab).expect("b applies a");
}

/// shadow 的 markdown 视图（文本+结构面；marks 是 payload 不入序列化）。
pub fn md(host: &MdCollabHost) -> String {
    markdown::tree_to_markdown(host.tree())
}

pub fn md_of(tree: &BlockTree) -> String {
    markdown::tree_to_markdown(tree)
}

/// 逐字符标记覆盖（marks 字面形状归一化比较基准——D-2 等价口径）。
pub fn coverage(marks: &[MarkSpan], _text: &str) -> BTreeMap<usize, BTreeSet<String>> {
    let mut out: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for s in marks {
        for i in s.start..s.end {
            out.entry(i).or_default().insert(s.kind.clone());
        }
    }
    out
}

fn coverage_of_node(tree: &BlockTree, id: NodeId) -> BTreeMap<usize, BTreeSet<String>> {
    let node = tree.node(id);
    let text = node.text.to_string();
    coverage(&node.marks, &text)
}

/// 两树覆盖等价断言（结构/文本/属性/BlockId 逐位 + marks 按逐字符
/// 标记集合——字面区间形状不断言，D-2 归一化语义下的等价口径）。
pub fn assert_trees_coverage_eq(a: &BlockTree, b: &BlockTree, ctx: &str) {
    let ta = a.top_level();
    let tb = b.top_level();
    assert_eq!(ta.len(), tb.len(), "{ctx}: top-level count");
    for (i, (x, y)) in ta.iter().zip(tb.iter()).enumerate() {
        assert_node_eq(a, *x, b, *y, &format!("{ctx}[{i}]"));
    }
}

fn assert_node_eq(a: &BlockTree, x: NodeId, b: &BlockTree, y: NodeId, path: &str) {
    let nx = a.node(x);
    let ny = b.node(y);
    assert_eq!(nx.kind, ny.kind, "{path}: kind");
    assert_eq!(nx.text.to_string(), ny.text.to_string(), "{path}: text");
    assert_eq!(nx.attrs, ny.attrs, "{path}: attrs");
    assert_eq!(
        nx.id.0, ny.id.0,
        "{path}: BlockId u64（family 延续/foreign 稳定映射）"
    );
    assert_eq!(
        coverage_of_node(a, x),
        coverage_of_node(b, y),
        "{path}: mark coverage"
    );
    let ca = &nx.children;
    let cb = &ny.children;
    assert_eq!(ca.len(), cb.len(), "{path}: children count");
    for (i, (cx, cy)) in ca.iter().zip(cb.iter()).enumerate() {
        assert_node_eq(a, *cx, b, *cy, &format!("{path}/{i}"));
    }
}

/// 忽略 BlockId 的结构等价断言（跨端比较——两端 u64 各自独立分配）。
pub fn assert_trees_structure_eq(a: &BlockTree, b: &BlockTree, ctx: &str) {
    let ta = a.top_level();
    let tb = b.top_level();
    assert_eq!(ta.len(), tb.len(), "{ctx}: top-level count");
    for (i, (x, y)) in ta.iter().zip(tb.iter()).enumerate() {
        assert_node_structure_eq(a, *x, b, *y, &format!("{ctx}[{i}]"));
    }
}

fn assert_node_structure_eq(a: &BlockTree, x: NodeId, b: &BlockTree, y: NodeId, path: &str) {
    let nx = a.node(x);
    let ny = b.node(y);
    assert_eq!(nx.kind, ny.kind, "{path}: kind");
    assert_eq!(nx.text.to_string(), ny.text.to_string(), "{path}: text");
    assert_eq!(nx.attrs, ny.attrs, "{path}: attrs");
    assert_eq!(
        coverage_of_node(a, x),
        coverage_of_node(b, y),
        "{path}: mark coverage"
    );
    assert_eq!(
        nx.children.len(),
        ny.children.len(),
        "{path}: children count"
    );
    for (i, (cx, cy)) in nx.children.iter().zip(ny.children.iter()).enumerate() {
        assert_node_structure_eq(a, *cx, b, *cy, &format!("{path}/{i}"));
    }
}

/// 快捷构造：mdcore 文本插入 op。
pub fn insert_text(parent: Vec<usize>, index: usize, kind: BlockKind, text: &str) -> Op {
    Op::InsertBlock {
        parent,
        index,
        kind,
        attrs: Default::default(),
        text: text.into(),
        id: None,
    }
}
