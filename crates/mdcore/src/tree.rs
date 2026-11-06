//! Block Tree — 主档 §8.1 最小语义模型 + §9 Canonical Document Model。
//!
//! - arena 槽位树：`Vec<Option<Node>>`，NodeId = 槽位号（稳定，永不压缩）。
//! - 虚根 Document 节点：root.children = 顶层块序列。
//! - 寻址双轨（ADR-B1 混合式）：mutation 走 `NodePath`（子索引链，重放安全），
//!   意图/锚点走 `BlockId`（注册表索引，结构变化后仍可解析）。
//! - 文本 = per-block `ropey::Rope`。Rope 只是文本存储实现（§9），
//!   不是第二个语义真理；整篇 source rope 由 View 层 CM6 承担（R7 裁决）。

use std::collections::BTreeMap;

use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::id::{BlockId, IdAllocator};

pub type Attrs = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Document,
    Paragraph,
    Heading,
    ListItem,
    Blockquote,
    CodeBlock,
    /// 数学块（#261）：`$$` 定界的 LaTeX 显示公式，text = 源码逐字。
    /// 语义同构 CodeBlock（own-text 叶块、无子块、内容不参与行内解析）。
    Math,
    Table,
    ThematicBreak,
}

impl BlockKind {
    pub fn is_structural_container(self) -> bool {
        matches!(self, BlockKind::Document | BlockKind::Blockquote)
    }
}

/// 行内 mark 区间（§8.1：第一阶段 text/mark 不要求独立 Stable ID）。
/// offset 单位 = char（与 ReplaceText 一致）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkSpan {
    pub start: usize,
    pub end: usize,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: BlockId,
    pub kind: BlockKind,
    pub text: Rope,
    pub attrs: Attrs,
    pub marks: Vec<MarkSpan>,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

/// 相对虚根的子索引链。空路径 = 根。
pub type NodePath = Vec<usize>;

/// R20 内存审计：树组件级活数据估计（字节）。
#[derive(Debug, serde::Serialize)]
pub struct TreeMemStats {
    pub live_nodes: usize,
    pub slots_capacity_bytes: usize,
    pub rope_est_bytes: usize,
    pub rope_text_bytes: usize,
    pub id_index_entries: usize,
    pub children_heap_bytes: usize,
    pub marks_heap_bytes: usize,
    pub attrs_entries: usize,
    pub attrs_string_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TreeError {
    BadPath,
    BadIndex,
    Cycle,
    RootForbidden,
    /// #244（审计 P0-1 / I4）：register 撞号守卫——待注册 ID 已在册且指向
    /// **不同槽位**（同槽幂等重挂载除外）时显式拒绝。此前 `BTreeMap::insert`
    /// 静默覆盖：活树出现重复 BlockId，先注册者从 id_index 失联（锚点
    /// 解析错块 / 快照 validate 毒化 / 协作寻址失效）。
    IdCollision {
        id: BlockId,
        existing: NodeId,
        incoming: NodeId,
    },
    /// #244（路线 A）：`reassign_ids` 输入表内部重复——持久化 blocks 表
    /// 不得携带重复 ID（校验先行，树零变更）。
    DuplicateInputId {
        id: BlockId,
    },
    /// #244（路线 A）：持久化 blocks 表长度与活树节点数不符（meta 与
    /// 正文失同步，如正文被外部改动）。
    IdCountMismatch {
        ids: usize,
        nodes: usize,
    },
}

/// 子树快照节点（remove 的 inverse / deep_clone / snapshot 共用）。
#[derive(Clone, Debug)]
pub struct NodeData {
    pub id: BlockId,
    pub kind: BlockKind,
    pub text: String,
    pub attrs: Attrs,
    pub marks: Vec<MarkSpan>,
    pub children: Vec<NodeData>,
}

#[derive(Clone, Debug)]
pub struct BlockTree {
    slots: Vec<Option<Node>>,
    root: NodeId,
    pub alloc: IdAllocator,
    id_index: BTreeMap<BlockId, NodeId>,
}

impl BlockTree {
    pub fn new() -> Self {
        let slots: Vec<Option<Node>> = vec![Some(Node {
            id: BlockId(0), // 根节点保留 ID 0；对外不可寻址
            kind: BlockKind::Document,
            text: Rope::new(),
            attrs: Attrs::new(),
            marks: Vec::new(),
            parent: None,
            children: Vec::new(),
        })];
        Self {
            slots,
            root: NodeId(0),
            alloc: IdAllocator::new(),
            id_index: BTreeMap::new(),
        }
    }

    // ---- 槽位访问 ----

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> &Node {
        self.slots[id.0].as_ref().expect("node slot occupied")
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.slots[id.0].as_mut().expect("node slot occupied")
    }

    pub fn try_node(&self, id: NodeId) -> Option<&Node> {
        self.slots[id.0].as_ref()
    }

    fn slot(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.slots.len());
        self.slots.push(Some(node));
        id
    }

    // ---- ID 注册表（ADR-B1：cid 注册表，只索引不回收） ----

    pub fn lookup_id(&self, id: BlockId) -> Option<NodeId> {
        self.id_index.get(&id).copied()
    }

    /// #244（I4）：撞号守卫——同 ID 已指向不同槽位 = 显式 Err；同槽幂等
    /// 重挂载（insert_child 对已注册节点的路径）= Ok。旧档（无 blocks 表）
    /// 跨会话 undo 的物化 ID 撞号在此被响亮拦截，不再静默覆盖毒化活树。
    fn register(&mut self, id: BlockId, slot: NodeId) -> Result<(), TreeError> {
        if let Some(&existing) = self.id_index.get(&id) {
            if existing != slot {
                return Err(TreeError::IdCollision {
                    id,
                    existing,
                    incoming: slot,
                });
            }
            return Ok(());
        }
        self.id_index.insert(id, slot);
        Ok(())
    }

    fn unregister(&mut self, id: BlockId) {
        self.id_index.remove(&id);
    }

    // ---- 分配 ----

    pub fn alloc_id(&mut self) -> BlockId {
        self.alloc.allocate()
    }

    /// 创建未挂载节点（**新分配 ID**，挂载用 [`BlockTree::insert_child`]）。
    ///
    /// 新 ID 结构性无撞：分配器单调 + 每个在册 ID 注册时都经 `observe`
    /// 抬升水位 ⇒ 在册 ID 恒小于水位，而 `allocate` 只发水位号（id.rs）。
    pub fn create_node(
        &mut self,
        kind: BlockKind,
        text: &str,
        attrs: Attrs,
        marks: Vec<MarkSpan>,
    ) -> NodeId {
        let block_id = self.alloc.allocate();
        self.try_spawn(kind, text, attrs, marks, block_id)
            .expect("fresh id is structurally collision-free (monotonic allocator)")
    }

    /// #244：按**物化 ID** 创建（undo 复原 / replay / open 持久化表 /
    /// snapshot 重建路径）。撞号 = 显式 [`TreeError::IdCollision`]，失败
    /// 零副作用（守卫先于落槽）；成功则抬升 allocator 水位（observe），
    /// 保证此后新分配绝不复用该 ID（§8.2 防撞车）。
    pub fn create_node_with_id(
        &mut self,
        kind: BlockKind,
        text: &str,
        attrs: Attrs,
        marks: Vec<MarkSpan>,
        id: BlockId,
    ) -> Result<NodeId, TreeError> {
        if let Some(&existing) = self.id_index.get(&id) {
            return Err(TreeError::IdCollision {
                id,
                existing,
                incoming: NodeId(self.slots.len()),
            });
        }
        self.alloc.observe(id);
        self.try_spawn(kind, text, attrs, marks, id)
    }

    /// 落槽 + 注册（两公开构造器的共用尾段）。
    fn try_spawn(
        &mut self,
        kind: BlockKind,
        text: &str,
        attrs: Attrs,
        marks: Vec<MarkSpan>,
        block_id: BlockId,
    ) -> Result<NodeId, TreeError> {
        let node = Node {
            id: block_id,
            kind,
            text: Rope::from_str(text),
            attrs,
            marks,
            parent: None,
            children: Vec::new(),
        };
        let slot = self.slot(node);
        self.register(block_id, slot)?;
        Ok(slot)
    }

    // ---- 路径 ----

    /// NodePath → NodeId。空路径 = 根。
    pub fn resolve(&self, path: &[usize]) -> Option<NodeId> {
        let mut cur = self.root;
        for &i in path {
            cur = self.node(cur).children.get(i).copied()?;
        }
        Some(cur)
    }

    /// NodeId → NodePath（沿父链回溯）。#244/F-12：父链终止于非根节点
    /// （已摘除子树 / deep_clone 未挂载根）= `None`——此前为 assert panic
    /// （`pub tree` 误用可达，审计 F-12）。挂载在根下的节点恒返回路径。
    pub fn path_of(&self, id: NodeId) -> Option<NodePath> {
        let mut path = Vec::new();
        let mut cur = id;
        while let Some(p) = self.node(cur).parent {
            let parent = self.node(p);
            let idx = parent.children.iter().position(|&c| c == cur)?;
            path.push(idx);
            cur = p;
        }
        if cur != self.root {
            return None; // 未挂载/已摘除：不在活树寻址面内
        }
        path.reverse();
        Some(path)
    }

    // ---- 结构原语（op 层使用；全部保持 §8.2 ID 不变量） ----

    /// 挂载：把 `node` 插为 `parent` 的第 `index` 个孩子（index <= len）。
    /// #244/F-12：挂载同步注册整棵子树（与 detach 的注销对称；同槽幂等）。
    /// 先整树预检撞号再动结构——失败时树零变更。
    pub fn insert_child(
        &mut self,
        parent: NodeId,
        index: usize,
        node: NodeId,
    ) -> Result<(), TreeError> {
        if index > self.node(parent).children.len() {
            return Err(TreeError::BadIndex);
        }
        self.check_subtree_free(node)?;
        self.node_mut(parent).children.insert(index, node);
        self.node_mut(node).parent = Some(parent);
        self.register_subtree(node)
    }

    /// 子树预检：子树内每个 ID 要么不在册、要么在册指向同槽（幂等重挂）。
    fn check_subtree_free(&self, node: NodeId) -> Result<(), TreeError> {
        let id = self.node(node).id;
        if let Some(&existing) = self.id_index.get(&id) {
            if existing != node {
                return Err(TreeError::IdCollision {
                    id,
                    existing,
                    incoming: node,
                });
            }
        }
        for &c in &self.node(node).children {
            self.check_subtree_free(c)?;
        }
        Ok(())
    }

    /// 子树整树注册（guard 在册内；预检通过后不可失败）。
    fn register_subtree(&mut self, node: NodeId) -> Result<(), TreeError> {
        let id = self.node(node).id;
        self.register(id, node)?;
        let children: Vec<NodeId> = self.node(node).children.clone();
        for c in children {
            self.register_subtree(c)?;
        }
        Ok(())
    }

    /// 从父链摘除（不回收槽位、不碰子树）。#244/F-12：同步注销整棵子树
    /// 的 ID——卸载节点不得残留在册（否则 lookup_id → path_of 撞断言）。
    /// 重挂载（move_subtree = detach + insert_child）由 insert_child 侧
    /// 对称恢复注册。
    pub fn detach(&mut self, node: NodeId) -> Result<(), TreeError> {
        let parent = self.node(node).parent.ok_or(TreeError::RootForbidden)?;
        self.node_mut(parent).children.retain(|&c| c != node);
        self.node_mut(node).parent = None;
        self.unregister_subtree(node);
        Ok(())
    }

    fn unregister_subtree(&mut self, node: NodeId) {
        let id = self.node(node).id;
        self.unregister(id);
        let children: Vec<NodeId> = self.node(node).children.clone();
        for c in children {
            self.unregister_subtree(c);
        }
    }

    /// 活树先序 ID 表（跳过虚根 ID 0；父先子后、同层按序）。#244 路线 A：
    /// `save_document` 持久化 blocks 表的数据源——与 markdown 解析的节点
    /// 创建序同构（父先子后），open 侧按位恢复。
    pub fn preorder_block_ids(&self) -> Vec<BlockId> {
        let mut out = Vec::new();
        fn walk(tree: &BlockTree, n: NodeId, out: &mut Vec<BlockId>) {
            out.push(tree.node(n).id);
            for &c in &tree.node(n).children {
                walk(tree, c, out);
            }
        }
        let top: Vec<NodeId> = self.node(self.root).children.clone();
        for t in top {
            walk(self, t, &mut out);
        }
        out
    }

    /// #244 路线 A：按文档先序整体重指派活树 ID（open 恢复持久化表）。
    ///
    /// 原子性：数量与输入重复均**先校验后动手**，失败树零变更；两遍式
    /// 执行——先把旧 ID 全部注销（避免 [3,2,1] 类重排的中间态被守卫
    /// 误伤），再逐一落新 ID（observe 抬水位 + 注册）。
    pub fn reassign_ids(&mut self, ids: &[BlockId]) -> Result<(), TreeError> {
        let order = self.preorder_slots();
        if ids.len() != order.len() {
            return Err(TreeError::IdCountMismatch {
                ids: ids.len(),
                nodes: order.len(),
            });
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in ids {
            if !seen.insert(id.0) {
                return Err(TreeError::DuplicateInputId { id: *id });
            }
        }
        // 遍一：注销全部旧 ID（幂等——重复 ID 的历史损伤在此被顺带清理）
        for &slot in &order {
            let old = self.node(slot).id;
            self.unregister(old);
        }
        // 遍二：落新 ID（旧已清空 + 输入无重复 ⇒ register 不可失败）
        for (i, &slot) in order.iter().enumerate() {
            let new_id = ids[i];
            self.node_mut(slot).id = new_id;
            self.alloc.observe(new_id);
            self.register(new_id, slot)
                .expect("reassign: old ids unregistered + inputs dup-free");
        }
        Ok(())
    }

    /// 挂载树先序槽位表（跳过虚根；reassign_ids 内部用）。
    fn preorder_slots(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        fn walk(tree: &BlockTree, n: NodeId, out: &mut Vec<NodeId>) {
            out.push(n);
            for &c in &tree.node(n).children {
                walk(tree, c, out);
            }
        }
        let top: Vec<NodeId> = self.node(self.root).children.clone();
        for t in top {
            walk(self, t, &mut out);
        }
        out
    }

    /// 子树先序快照（含自身）。`data.text` 从 Rope 展开。
    pub fn collect_subtree(&self, id: NodeId) -> NodeData {
        let n = self.node(id);
        NodeData {
            id: n.id,
            kind: n.kind,
            text: n.text.to_string(),
            attrs: n.attrs.clone(),
            marks: n.marks.clone(),
            children: n
                .children
                .iter()
                .map(|&c| self.collect_subtree(c))
                .collect(),
        }
    }

    /// 硬删除子树：先快照、先取父指针，再清理槽位与注册表。返回快照供 inverse 使用。
    pub fn remove_subtree(&mut self, node: NodeId) -> Result<NodeData, TreeError> {
        if node == self.root {
            return Err(TreeError::RootForbidden);
        }
        let data = self.collect_subtree(node);
        let parent = self.node(node).parent;
        let mut stack = vec![node];
        while let Some(cur) = stack.pop() {
            let n = self.slots[cur.0].as_ref().ok_or(TreeError::BadPath)?;
            stack.extend(n.children.iter().copied());
            self.unregister(n.id);
            self.slots[cur.0] = None;
        }
        if let Some(p) = parent {
            self.node_mut(p).children.retain(|&c| c != node);
        }
        Ok(data)
    }

    /// 移动子树。`to_index` 为**摘除后语义**（post-removal indexing）：
    /// 先 detach，再插到目标父的第 to_index 位。同父跨位无需偏移校正。
    pub fn move_subtree(
        &mut self,
        node: NodeId,
        to_parent: NodeId,
        to_index: usize,
    ) -> Result<(), TreeError> {
        if node == self.root {
            return Err(TreeError::RootForbidden);
        }
        // 环检查：目标父的祖先链不得穿过被移动节点。
        let mut cur = Some(to_parent);
        while let Some(c) = cur {
            if c == node {
                return Err(TreeError::Cycle);
            }
            cur = self.node(c).parent;
        }
        if self.node(node).parent.is_none() {
            return Err(TreeError::RootForbidden);
        }
        self.detach(node)?;
        self.insert_child(to_parent, to_index, node)
    }

    /// 深克隆子树（§8.2 copy → 全部新 ID），返回新子树根（未挂载）。
    pub fn deep_clone(&mut self, data: &NodeData) -> NodeId {
        let slot = self.create_node(
            data.kind,
            &data.text,
            data.attrs.clone(),
            data.marks.clone(),
        );
        for child in &data.children {
            let c = self.deep_clone(child);
            let len = self.node(slot).children.len();
            let _ = self.insert_child(slot, len, c); // append
        }
        slot
    }

    // ---- 检视 ----

    pub fn top_level(&self) -> &[NodeId] {
        &self.node(self.root).children
    }

    pub fn node_count(&self) -> usize {
        self.slots.iter().flatten().count() - 1
    }

    // ---- 内存审计（R20）----

    /// 组件级内存估计（R20）：活数据近似值（非精确 RSS），供
    /// Core::mem_stats 聚合后经 wasm 出口给宿主内存面板。
    pub fn mem_stats(&self) -> TreeMemStats {
        let mut live_nodes = 0usize;
        let mut rope_text_bytes = 0usize;
        let mut rope_est_bytes = 0usize;
        let mut children_heap_bytes = 0usize;
        let mut marks_heap_bytes = 0usize;
        let mut attrs_entries = 0usize;
        let mut attrs_string_bytes = 0usize;
        for n in self.slots.iter().flatten() {
            live_nodes += 1;
            rope_text_bytes += n.text.len_bytes();
            // 每块独立 Rope：结构体 + 堆内容近似（短文本为主）。
            rope_est_bytes += std::mem::size_of::<Rope>() + n.text.len_bytes();
            if n.children.capacity() > 0 {
                children_heap_bytes += n.children.capacity() * std::mem::size_of::<NodeId>();
            }
            if n.marks.capacity() > 0 {
                marks_heap_bytes += n.marks.capacity() * std::mem::size_of::<MarkSpan>();
            }
            for (k, v) in &n.attrs {
                attrs_entries += 1;
                attrs_string_bytes += k.capacity() + v.capacity();
            }
        }
        TreeMemStats {
            live_nodes,
            slots_capacity_bytes: self.slots.capacity() * std::mem::size_of::<Option<Node>>(),
            rope_est_bytes,
            rope_text_bytes,
            id_index_entries: self.id_index.len(),
            children_heap_bytes,
            marks_heap_bytes,
            attrs_entries,
            attrs_string_bytes,
        }
    }
}

impl Default for BlockTree {
    fn default() -> Self {
        Self::new()
    }
}
