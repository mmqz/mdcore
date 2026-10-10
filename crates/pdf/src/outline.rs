//! 大纲（书签树）——PDF `/Outlines` 面的内存模型（PDF-b 骨架轮）。
//!
//! 深度优先文档序由 [`OutlineTree::iter`] **唯一**产出（前序：父先于
//! 子、同级按声明序）——消费方不得自行递归（单一排序出口 = 确定性，
//! 导航面板与 Outline 命令同源）。目标锚定 v0 只有页目标；命名目标/
//! URL 目标随 PDF-c 链接面追加变体（纯增量，非破坏）。

use serde::{Deserialize, Serialize};

use crate::page::PageIndex;

/// 大纲节点目标。v0 = 页目标（0 基页号）；PDF-c 链接面扩展命名/URI 目标。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutlineTarget {
    Page(PageIndex),
}

/// 大纲节点（书签项）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineNode {
    pub title: String,
    pub target: OutlineTarget,
    pub children: Vec<OutlineNode>,
}

impl OutlineNode {
    /// 叶节点（无子级）。
    pub fn leaf(title: impl Into<String>, target: OutlineTarget) -> Self {
        Self {
            title: title.into(),
            target,
            children: Vec::new(),
        }
    }

    /// 分支节点（带子级；子序即声明序 = 文档序）。
    pub fn branch(
        title: impl Into<String>,
        target: OutlineTarget,
        children: Vec<OutlineNode>,
    ) -> Self {
        Self {
            title: title.into(),
            target,
            children,
        }
    }
}

/// 大纲树（根列表 + 逐层 children）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineTree {
    roots: Vec<OutlineNode>,
}

impl OutlineTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从根列表构造（空列表 = 无大纲文档，合法形态）。
    pub fn from_roots(roots: Vec<OutlineNode>) -> Self {
        Self { roots }
    }

    pub fn roots(&self) -> &[OutlineNode] {
        &self.roots
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// 深度优先文档序迭代：产出 `(depth, &OutlineNode)`，根为 depth 0。
    pub fn iter(&self) -> OutlineIter<'_> {
        OutlineIter {
            stack: vec![(0, self.roots.iter())],
        }
    }
}

/// 深度优先前序迭代器（栈式展开——惰性，不预扁平化）。
pub struct OutlineIter<'a> {
    /// 栈：每层一个同级迭代器；栈顶 = 当前最深待访层。
    /// 栈内 Item 的生命周期是树的 `'a`（与 `&mut self` 借用解耦）。
    stack: Vec<(usize, std::slice::Iter<'a, OutlineNode>)>,
}

impl<'a> Iterator for OutlineIter<'a> {
    type Item = (usize, &'a OutlineNode);

    fn next(&mut self) -> Option<Self::Item> {
        while !self.stack.is_empty() {
            // 取栈顶层下一个节点；该层耗尽则退栈（回到父级继续同级）。
            let next_node = self
                .stack
                .last_mut()
                .and_then(|(depth, siblings)| siblings.next().map(|node| (*depth, node)));
            match next_node {
                Some((depth, node)) => {
                    // 先产出本节点（前序），再压入其子层（下一轮先访子）。
                    self.stack.push((depth + 1, node.children.iter()));
                    return Some((depth, node));
                }
                None => {
                    self.stack.pop();
                }
            }
        }
        None
    }
}
