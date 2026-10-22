//! 七操作载体翻译（yrs-e §5 操作面映射——每命令一事务 + origin）。
//!
//! 事务粒度 = B2.1(b) 逐笔路径（一个 undo 栈项 + 一个 origin 溯源
//! 单位）；批量装载走 512/事务小批路径（内存放大缓解既证结论）。

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use yrs::types::xml::{XmlElementPrelim, XmlIn, XmlOut, XmlTextPrelim};
use yrs::{GetString, IndexedSequence, Text, Transact, Xml, XmlFragment};

use crate::identity::CrdtAnchor;
use crate::session::{
    attr_str, block_id_of, c2b, deep_copy, escape_key, Actor, CollabError, CollabSession, ATTR_ID,
    MARK_TAG, ROOT,
};
use crate::{BlockKind, BlockSpec};

/// 小批尺寸（B2.1(b)：每小批独立事务）。
const BLOCK_BATCH: usize = 512;

/// 父容器引用（fragment 根或容器元素）。
#[derive(Clone)]
pub(crate) enum ParentRef {
    Fragment(yrs::XmlFragmentRef),
    Element(yrs::XmlElementRef),
}

impl ParentRef {
    fn len(&self, txn: &yrs::Transaction) -> u32 {
        match self {
            ParentRef::Fragment(f) => f.len(txn),
            ParentRef::Element(e) => e.len(txn),
        }
    }

    fn child(&self, txn: &yrs::Transaction, index: u32) -> Option<XmlOut> {
        match self {
            ParentRef::Fragment(f) => f.get(txn, index),
            ParentRef::Element(e) => e.get(txn, index),
        }
    }

    fn sticky_child_id(&self, txn: &yrs::Transaction, index: u32) -> Option<yrs::ID> {
        match self {
            ParentRef::Fragment(f) => f
                .sticky_index(txn, index, yrs::Assoc::After)
                .and_then(|s| s.id().cloned()),
            ParentRef::Element(e) => e
                .sticky_index(txn, index, yrs::Assoc::After)
                .and_then(|s| s.id().cloned()),
        }
    }
}

/// 定位结果：父容器 + 物理子位 + 元素引用。
pub(crate) struct Located {
    pub parent: ParentRef,
    pub index: u32,
    pub element: yrs::XmlElementRef,
}

impl CollabSession {
    /// 全树定位块（属性扫描——远端块免侧车寻址）。
    pub(crate) fn locate(&self, block_id: &str) -> Result<Located, CollabError> {
        self.check_inbound(block_id.len())?;
        let xml = self.doc().get_or_insert_xml_fragment(ROOT);
        let txn = self.doc().transact();
        if let Some(l) = locate_in(&ParentRef::Fragment(xml), &txn, block_id) {
            return Ok(l);
        }
        Err(CollabError::BlockNotFound {
            block_id: block_id.to_string(),
        })
    }

    /// own-text 段子节点数（leading Text/m run 长度——容器插入位偏移）。
    fn own_segment_len(el: &yrs::XmlElementRef, txn: &yrs::Transaction) -> u32 {
        let mut n = 0u32;
        for c in el.children(txn) {
            match c {
                XmlOut::Text(_) => n += 1,
                XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG => n += 1,
                _ => break,
            }
        }
        n
    }

    /// 子块计数（元素子节点中非 mark 者）。
    pub(crate) fn child_block_count(parent: &ParentRef, txn: &yrs::Transaction) -> u32 {
        let mut n = 0u32;
        for i in 0..parent.len(txn) {
            if let Some(XmlOut::Element(e)) = parent.child(txn, i) {
                if e.tag().as_ref() != MARK_TAG {
                    n += 1;
                }
            }
        }
        n
    }

    /// 解析插入父容器（None = 顶层）+ 校验容器面。
    fn resolve_parent(
        &self,
        parent: Option<&str>,
    ) -> Result<(ParentRef, Option<String>), CollabError> {
        match parent {
            None => {
                let xml = self.doc().get_or_insert_xml_fragment(ROOT);
                Ok((ParentRef::Fragment(xml), None))
            }
            Some(pid) => {
                let loc = self.locate(pid)?;
                let kind = BlockKind::from_tag(loc.element.tag());
                if !matches!(kind, Some(k) if k.carries_children()) {
                    return Err(CollabError::NotAContainer {
                        block_id: pid.to_string(),
                    });
                }
                Ok((
                    ParentRef::Element(loc.element.clone()),
                    Some(pid.to_string()),
                ))
            }
        }
    }

    /// 子块逻辑 index → 物理子位（容器 = own-text 段之后）。
    fn physical_index(
        &self,
        parent: &ParentRef,
        index: u32,
        txn: &yrs::Transaction,
    ) -> Result<u32, CollabError> {
        match parent {
            ParentRef::Fragment(_) => Ok(index),
            ParentRef::Element(e) => Ok(Self::own_segment_len(e, txn) + index),
        }
    }

    // ── 装载面 ─────────────────────────────────────────────────────

    /// 批量装载（顶层追加；512/事务小批路径；undo 粒度 = 每小批一栈项）。
    pub fn load_blocks(
        &mut self,
        actor: Actor,
        blocks: &[BlockSpec],
    ) -> Result<Vec<String>, CollabError> {
        let total: usize = blocks
            .iter()
            .map(|b| b.text.len() + b.attrs.keys().map(|k| k.len()).sum::<usize>() + 128)
            .sum();
        self.check_inbound(total)?;
        for b in blocks {
            if b.attrs.contains_key(ATTR_ID) {
                return Err(CollabError::ReservedAttrKey {
                    key: ATTR_ID.into(),
                });
            }
            if b.kind == BlockKind::ThematicBreak && !b.text.is_empty() {
                return Err(CollabError::KindIncompatible {
                    block_id: "<spec>".into(),
                    op: "text",
                });
            }
        }
        let xml = self.doc().get_or_insert_xml_fragment(ROOT);
        let mut ids = Vec::with_capacity(blocks.len());
        for batch in blocks.chunks(BLOCK_BATCH) {
            // 预分配本批身份（事务借用期间不可 &mut self——p18 事务
            // 纪律：借用随 Drop 释放）。
            let batch_ids: Vec<String> = (0..batch.len()).map(|_| self.next_block_id()).collect();
            {
                let mut txn = self.doc().transact_mut_with(actor.wire());
                for (spec, id) in batch.iter().zip(batch_ids.iter()) {
                    let prelim = build_prelim(id, spec);
                    let idx = xml.len(&txn);
                    xml.insert(&mut txn, idx, prelim);
                }
                txn.commit();
            }
            ids.extend(batch_ids);
        }
        self.enforce_undo_limit(actor);
        Ok(ids)
    }

    // ── 七操作 ─────────────────────────────────────────────────────

    /// InsertBlock（含身份物化面：`id = Some` 为 undo/replay 回写路径）。
    pub fn insert_block(
        &mut self,
        actor: Actor,
        parent: Option<&str>,
        index: u32,
        spec: &BlockSpec,
        id: Option<String>,
    ) -> Result<String, CollabError> {
        let inbound = spec.text.len()
            + spec.attrs.keys().map(|k| k.len()).sum::<usize>()
            + parent.map(|p| p.len()).unwrap_or(0)
            + id.as_deref().map(|i| i.len()).unwrap_or(0)
            + 128;
        self.check_inbound(inbound)?;
        if spec.attrs.contains_key(ATTR_ID) {
            return Err(CollabError::ReservedAttrKey {
                key: ATTR_ID.into(),
            });
        }
        if spec.kind == BlockKind::ThematicBreak && !spec.text.is_empty() {
            return Err(CollabError::KindIncompatible {
                block_id: "<spec>".into(),
                op: "text",
            });
        }
        let block_id = match id {
            Some(given) => {
                // 物化身份不得与现存块撞号（重放安全面）。
                if self.find_block(&given).is_some() {
                    return Err(CollabError::InvalidInput(format!(
                        "block_id {given:?} already exists"
                    )));
                }
                given
            }
            None => self.next_block_id(),
        };
        let (parent_ref, _) = self.resolve_parent(parent)?;
        {
            let txn = self.doc().transact();
            let count = Self::child_block_count(&parent_ref, &txn);
            if index > count {
                return Err(CollabError::InvalidInput(format!(
                    "index {index} out of bounds (child blocks {count})"
                )));
            }
        }
        let prelim = build_prelim(&block_id, spec);
        {
            let phys = {
                let txn = self.doc().transact();
                self.physical_index(&parent_ref, index, &txn)?
            };
            let mut txn = self.doc().transact_mut_with(actor.wire());
            match &parent_ref {
                ParentRef::Fragment(f) => {
                    f.insert(&mut txn, phys, prelim);
                }
                ParentRef::Element(e) => {
                    e.insert(&mut txn, phys, prelim);
                }
            }
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(block_id)
    }

    /// RemoveBlock（锚点表条目保留——悬空态；undo 复活为新 ID 族）。
    pub fn remove_block(&mut self, actor: Actor, block_id: &str) -> Result<(), CollabError> {
        self.check_inbound(block_id.len())?;
        let loc = self.locate(block_id)?;
        {
            let mut txn = self.doc().transact_mut_with(actor.wire());
            match &loc.parent {
                ParentRef::Fragment(f) => f.remove_range(&mut txn, loc.index, 1),
                ParentRef::Element(e) => e.remove_range(&mut txn, loc.index, 1),
            }
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
    }

    /// ReplaceText（char 域入参；D-3 字节域换算在事务内单点完成；
    /// own-text 段全域——跨 mark 元素按逻辑位置编辑）。
    pub fn replace_text(
        &mut self,
        actor: Actor,
        block_id: &str,
        start: u32,
        end: u32,
        insert: &str,
    ) -> Result<u32, CollabError> {
        self.check_inbound(block_id.len() + insert.len())?;
        let loc = self.locate(block_id)?;
        let kind = BlockKind::from_tag(loc.element.tag().as_ref()).ok_or_else(|| {
            CollabError::BlockNotFound {
                block_id: block_id.into(),
            }
        })?;
        if !kind.carries_text() {
            return Err(CollabError::KindIncompatible {
                block_id: block_id.into(),
                op: "text",
            });
        }
        // 预读：own-text 段文本节点序列 + 逻辑文本。
        let nodes: Vec<yrs::XmlTextRef> = {
            let txn = self.doc().transact();
            own_text_nodes(&loc.element, &txn)
        };
        let texts: Vec<String> = {
            let txn = self.doc().transact();
            nodes.iter().map(|t| t.get_string(&txn)).collect()
        };
        let total: u32 = texts.iter().map(|t| t.chars().count() as u32).sum();
        if start > end || end > total {
            return Err(CollabError::BadRange {
                start,
                end,
                len: total,
            });
        }
        if start == end && insert.is_empty() {
            return Ok(total); // 空操作：不开事务（零 undo 栈项）
        }
        {
            let mut txn = self.doc().transact_mut_with(actor.wire());
            // 各节点 char 域区间账目（off_i 基于原始文本——删除互不影响
            // 其他节点的偏移）。
            let mut off = 0u32;
            let mut target: Option<(usize, u32)> = None; // (node idx, char pos in node)
            for (i, t) in texts.iter().enumerate() {
                let len = t.chars().count() as u32;
                if target.is_none() && start >= off && start <= off + len {
                    target = Some((i, start - off));
                }
                // 删除区间与本节点交集。
                let ls = start.saturating_sub(off).min(len);
                let le = end.saturating_sub(off).min(len);
                if le > ls {
                    let bs = c2b(&texts[i], ls);
                    let be = c2b(&texts[i], le);
                    if be > bs {
                        nodes[i].remove_range(&mut txn, bs, be - bs);
                    }
                }
                off += len;
            }
            // 插入：落入节点尾部边界时并入该节点；无文本节点（空块）
            // → 新建文本子节点于位 0。
            match target {
                Some((i, char_pos)) => {
                    let new_text = nodes[i].get_string(&txn);
                    let bpos = c2b(&new_text, char_pos);
                    nodes[i].insert(&mut txn, bpos, insert);
                }
                None => {
                    loc.element.insert(&mut txn, 0, XmlTextPrelim::new(insert));
                }
            }
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        let removed = end - start;
        Ok(total - removed + insert.chars().count() as u32)
    }

    /// SetBlockType（D-1：删旧建新搬子树，同事务原子；子树载体身份
    /// 换新族，block_id 属性随迁语义连续；锚点表自动重指）。
    pub fn set_block_type(
        &mut self,
        actor: Actor,
        block_id: &str,
        new_kind: BlockKind,
    ) -> Result<(), CollabError> {
        self.check_inbound(block_id.len())?;
        let loc = self.locate(block_id)?;
        let old_kind = BlockKind::from_tag(loc.element.tag().as_ref());
        if old_kind == Some(new_kind) {
            return Ok(()); // 同类：零操作零栈项
        }
        // 目标种类承载面校验（镜像 insert/load 时不变量——文本/子块
        // 无去处则拒，不静默丢内容；audit 走查发现的对齐缺口：此前
        // 重建无条件复制子节点，可产出 hr带文本 / 非容器带子块 等
        // 本引擎书写约定外的形状）。
        {
            let txn = self.doc().transact();
            let (text, _) = read_coverage(&loc.element, &txn);
            if !new_kind.carries_text() && !text.is_empty() {
                return Err(CollabError::KindIncompatible {
                    block_id: block_id.into(),
                    op: "text",
                });
            }
            let child_blocks =
                Self::child_block_count(&ParentRef::Element(loc.element.clone()), &txn);
            if !new_kind.carries_children() && child_blocks > 0 {
                return Err(CollabError::KindIncompatible {
                    block_id: block_id.into(),
                    op: "children",
                });
            }
        }
        // 子树 block_id 先登记（重建后锚点重指面）。
        let subtree_ids = {
            let txn = self.doc().transact();
            let mut ids = vec![block_id.to_string()];
            collect_subtree_ids(&loc.element, &txn, &mut ids);
            ids
        };
        // 重建体先在读事务内构造（读事务出域后再开写事务——yrs 读写
        // 事务同锁，持有读事务再开写事务 = 死锁）。
        let rebuilt = {
            let txn = self.doc().transact();
            let mut rebuilt = XmlElementPrelim::empty(new_kind.tag());
            for (k, v) in loc.element.attributes(&txn) {
                if let Some(v) = attr_str(&v) {
                    rebuilt.attributes.insert(k.into(), v);
                }
            }
            let keep_text = new_kind.carries_text();
            let mut pushed_text = false;
            for c in loc.element.children(&txn) {
                match c {
                    XmlOut::Text(t) if keep_text => {
                        rebuilt
                            .children
                            .push(XmlIn::Text(XmlTextPrelim::new(t.get_string(&txn)).into()));
                        pushed_text = true;
                    }
                    XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG && keep_text => {
                        rebuilt.children.push(XmlIn::Element(deep_copy(&e, &txn)));
                    }
                    XmlOut::Element(e) if e.tag().as_ref() != MARK_TAG => {
                        // 子块（前置校验保证目标必为容器）。
                        rebuilt.children.push(XmlIn::Element(deep_copy(&e, &txn)));
                    }
                    _ => {} // 目标不承载的面：丢弃（前置校验保证非空时已拒）
                }
            }
            // 文本承载目标恒带出生文本节点（撤销耦合防护——与
            // build_prelim 同一约定；hr→p 类转换此前会产出无文本
            // 节点的文本承载块 = 耦合隐患）。
            if keep_text && !pushed_text {
                rebuilt
                    .children
                    .insert(0, XmlIn::Text(XmlTextPrelim::new("").into()));
            }
            rebuilt
        };
        {
            let mut txn = self.doc().transact_mut_with(actor.wire());
            match &loc.parent {
                ParentRef::Fragment(f) => {
                    f.remove_range(&mut txn, loc.index, 1);
                    f.insert(&mut txn, loc.index, rebuilt);
                }
                ParentRef::Element(e) => {
                    e.remove_range(&mut txn, loc.index, 1);
                    e.insert(&mut txn, loc.index, rebuilt);
                }
            }
            txn.commit();
        }
        // 事务已出域（Drop 释放借用）——锚点重指 + 撤销限检。
        self.rebind_anchors(&subtree_ids);
        self.enforce_undo_limit(actor);
        Ok(())
    }

    /// SetAttrs（终态钦定：清用户属性写目标集；block_id 载体键保留）。
    pub fn set_attrs(
        &mut self,
        actor: Actor,
        block_id: &str,
        attrs: &BTreeMap<String, String>,
    ) -> Result<(), CollabError> {
        let inbound = block_id.len() + attrs.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
        self.check_inbound(inbound)?;
        if attrs.contains_key(ATTR_ID) {
            return Err(CollabError::ReservedAttrKey {
                key: ATTR_ID.into(),
            });
        }
        let loc = self.locate(block_id)?;
        let existing: Vec<String> = {
            let txn = self.doc().transact();
            loc.element
                .attributes(&txn)
                .filter(|(k, _)| *k != ATTR_ID)
                .map(|(k, _)| k.to_string())
                .collect()
        };
        {
            let mut txn = self.doc().transact_mut_with(actor.wire());
            for k in existing {
                loc.element.remove_attribute(&mut txn, &k);
            }
            for (k, v) in attrs {
                loc.element
                    .insert_attribute(&mut txn, escape_key(k), v.clone());
            }
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
    }

    /// SetMark（D-2：非重叠最小区间等价覆盖——归一化重建策略：
    /// 读 own-text 段覆盖 → 集合代数 → 整段重建。文本节点载体身份
    /// 重建面登记：块级锚点不受影响，同块并发文本编辑可能被本归一
    /// 化终态取代——收敛性恒成立[CRDT]，字面叠加语义注册为已知面）。
    pub fn set_mark(
        &mut self,
        actor: Actor,
        block_id: &str,
        start: u32,
        end: u32,
        mark: &str,
        add: bool,
    ) -> Result<(), CollabError> {
        self.check_inbound(block_id.len() + mark.len())?;
        if mark.is_empty() {
            return Err(CollabError::InvalidInput("mark kind 不能为空".into()));
        }
        let loc = self.locate(block_id)?;
        let kind = BlockKind::from_tag(loc.element.tag().as_ref()).ok_or_else(|| {
            CollabError::BlockNotFound {
                block_id: block_id.into(),
            }
        })?;
        if !kind.carries_text() {
            return Err(CollabError::KindIncompatible {
                block_id: block_id.into(),
                op: "mark",
            });
        }
        // 预读覆盖区间（own-text 段）。
        let (text, intervals) = {
            let txn = self.doc().transact();
            read_coverage(&loc.element, &txn)
        };
        let total = text.chars().count() as u32;
        if start > end || end > total {
            return Err(CollabError::BadRange {
                start,
                end,
                len: total,
            });
        }
        if start == end {
            return Ok(()); // 空区间零操作
        }
        // 集合代数：[start,end) 内 add → 插入 kind / remove → 剔除。
        let mut updated: Vec<(u32, u32, BTreeSet<String>)> = Vec::new();
        for (s, e, set) in intervals {
            // 与操作区间不相交：原样保留。
            if e <= start || s >= end {
                push_interval(&mut updated, s, e, &set);
                continue;
            }
            // 前残段。
            if s < start {
                push_interval(&mut updated, s, start, &set);
            }
            // 操作段。
            let os = s.max(start);
            let oe = e.min(end);
            let mut new_set = set.clone();
            if add {
                new_set.insert(mark.to_string());
            } else {
                new_set.remove(mark);
            }
            push_interval(&mut updated, os, oe, &new_set);
            // 后残段。
            if e > end {
                push_interval(&mut updated, end, e, &set);
            }
        }
        // 整段重建：own-text 段子节点（leading Text/m run）全替换。
        let n_seg = {
            let txn = self.doc().transact();
            let mut n = 0u32;
            for c in loc.element.children(&txn) {
                match c {
                    XmlOut::Text(_) => n += 1,
                    XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG => n += 1,
                    _ => break,
                }
            }
            n
        };
        {
            let chars: Vec<char> = text.chars().collect();
            let mut txn = self.doc().transact_mut_with(actor.wire());
            if n_seg > 0 {
                loc.element.remove_range(&mut txn, 0, n_seg);
            }
            // 正向递增位插入（终态区间序 = 升序）。
            let mut at = 0u32;
            for (s, e, set) in &updated {
                if e <= s {
                    continue;
                }
                let seg: String = chars[*s as usize..*e as usize].iter().collect();
                if set.is_empty() {
                    loc.element.insert(&mut txn, at, XmlTextPrelim::new(&seg));
                } else {
                    // 嵌套序确定性编码（yrs-e §3.2）：升序逐层包裹 →
                    // 外层 = 字典序最大者（降序外层）。
                    let mut kinds = set.iter();
                    let mut node = {
                        let mut m = XmlElementPrelim::empty(MARK_TAG);
                        m.attributes
                            .insert("kind".into(), kinds.next().unwrap().clone());
                        m.children
                            .push(XmlIn::Text(XmlTextPrelim::new(&seg).into()));
                        m
                    };
                    for k in kinds {
                        let mut outer = XmlElementPrelim::empty(MARK_TAG);
                        outer.attributes.insert("kind".into(), k.clone());
                        outer.children.push(XmlIn::Element(node));
                        node = outer;
                    }
                    loc.element.insert(&mut txn, at, node);
                }
                at += 1;
            }
            // 尾随空文本节点（撤销耦合防护——见 build_prelim 注释；
            // 全 mark 段亦保持存在可寻址文本节点，replace 永不新建）。
            loc.element.insert(&mut txn, at, XmlTextPrelim::new(""));
            txn.commit();
        }
        self.enforce_undo_limit(actor);
        Ok(())
    }

    /// MoveBlock（同事务 detach+insert 原子；D-1 同族身份换新 + 锚点
    /// 重指；环回拒绝——块不可移入自身子树）。
    pub fn move_block(
        &mut self,
        actor: Actor,
        block_id: &str,
        to_parent: Option<&str>,
        to_index: u32,
    ) -> Result<(), CollabError> {
        self.check_inbound(block_id.len() + to_parent.map(|p| p.len()).unwrap_or(0))?;
        let loc = self.locate(block_id)?;
        // 目标父解析 + 环回检测：to_parent 在 block 子树内 = 拒。
        if let Some(tp) = to_parent {
            if tp == block_id {
                return Err(CollabError::InvalidInput(format!(
                    "move target parent {tp:?} = block itself"
                )));
            }
            let txn = self.doc().transact();
            let mut ids = vec![block_id.to_string()];
            collect_subtree_ids(&loc.element, &txn, &mut ids);
            drop(txn);
            if ids.iter().any(|i| i == tp) {
                return Err(CollabError::InvalidInput(format!(
                    "move target parent {tp:?} inside moved subtree (cycle)"
                )));
            }
        }
        let (dest, _) = self.resolve_parent(to_parent)?;
        // 源子树深拷贝（先读后写——同事务内两步，中途态不进 wire）。
        let (subtree_ids, prelim) = {
            let txn = self.doc().transact();
            let mut ids = vec![block_id.to_string()];
            collect_subtree_ids(&loc.element, &txn, &mut ids);
            (ids, deep_copy(&loc.element, &txn))
        };
        let phys = {
            let txn = self.doc().transact();
            let count = Self::child_block_count(&dest, &txn);
            if to_index > count {
                return Err(CollabError::InvalidInput(format!(
                    "to_index {to_index} out of bounds (child blocks {count})"
                )));
            }
            self.physical_index(&dest, to_index, &txn)?
        };
        {
            let mut txn = self.doc().transact_mut_with(actor.wire());
            match &loc.parent {
                ParentRef::Fragment(f) => f.remove_range(&mut txn, loc.index, 1),
                ParentRef::Element(e) => e.remove_range(&mut txn, loc.index, 1),
            }
            match &dest {
                ParentRef::Fragment(f) => {
                    f.insert(&mut txn, phys, prelim);
                }
                ParentRef::Element(e) => {
                    e.insert(&mut txn, phys, prelim);
                }
            }
            txn.commit();
        }
        self.rebind_anchors(&subtree_ids);
        self.enforce_undo_limit(actor);
        Ok(())
    }

    // ── 锚点重指（结构性重建面——D-1/MoveBlock 后保持锚点表一致）──

    pub(crate) fn rebind_anchors(&mut self, subtree_ids: &[String]) {
        if self.anchors().is_empty() {
            return;
        }
        let stale: Vec<String> = subtree_ids
            .iter()
            .filter(|id| self.anchors().contains_key(*id))
            .cloned()
            .collect();
        for id in stale {
            // 重指 = 按属性重定位新元素取新载体 ID（旧锚点值的外部
            // 持有者将得到 AnchorDangling——重验通道恢复，d1e-t1 语义）。
            if let Ok(new_anchor) = self.bind_anchor_internal(&id) {
                self.anchors_mut().insert(id, new_anchor);
            }
        }
    }

    pub(crate) fn bind_anchor_internal(&self, block_id: &str) -> Result<CrdtAnchor, CollabError> {
        let loc = self.locate(block_id)?;
        let txn = self.doc().transact();
        let id = loc
            .parent
            .sticky_child_id(&txn, loc.index)
            .ok_or_else(|| CollabError::InvalidInput("sticky_index 解析失败".into()))?;
        Ok(CrdtAnchor {
            client: id.client.get(),
            clock: id.clock,
        })
    }
}

// ── 自由函数工具 ─────────────────────────────────────────────────

fn locate_in(parent: &ParentRef, txn: &yrs::Transaction, block_id: &str) -> Option<Located> {
    for i in 0..parent.len(txn) {
        if let Some(XmlOut::Element(e)) = parent.child(txn, i) {
            if e.tag().as_ref() == MARK_TAG {
                continue;
            }
            if block_id_of(&e, txn).as_deref() == Some(block_id) {
                return Some(Located {
                    parent: parent.clone(),
                    index: i,
                    element: e,
                });
            }
            if let Some(l) = locate_in(&ParentRef::Element(e.clone()), txn, block_id) {
                return Some(l);
            }
        }
    }
    None
}

fn build_prelim(block_id: &str, spec: &BlockSpec) -> XmlElementPrelim {
    let mut prelim = XmlElementPrelim::empty(spec.kind.tag());
    prelim
        .attributes
        .insert(ATTR_ID.into(), block_id.to_string());
    for (k, v) in &spec.attrs {
        prelim.attributes.insert(escape_key(k).into(), v.clone());
    }
    // 文本承载块恒带文本子节点（空文本亦然——撤销耦合防护：若首笔
    // 编辑才建节点，undo 首笔 = 删整个节点[含后续并发内容]；出生
    // 即带节点则 undo 恒只撤字符项。ThematicBreak 无文本面除外）。
    if spec.kind.carries_text() {
        prelim
            .children
            .push(XmlIn::Text(XmlTextPrelim::new(&spec.text).into()));
    }
    prelim
}

/// own-text 段文本节点序列（leading Text/m run，m 内递归）。
pub(crate) fn own_text_nodes(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
) -> Vec<yrs::XmlTextRef> {
    let mut out = Vec::new();
    for c in el.children(txn) {
        match c {
            XmlOut::Text(t) => out.push(t),
            XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG => {
                collect_text_nodes(&e, txn, &mut out);
            }
            _ => break,
        }
    }
    out
}

fn collect_text_nodes(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    out: &mut Vec<yrs::XmlTextRef>,
) {
    for c in el.children(txn) {
        match c {
            XmlOut::Text(t) => out.push(t),
            XmlOut::Element(e) => collect_text_nodes(&e, txn, out),
            _ => {}
        }
    }
}

/// 子树 block_id 收集（含自身）。
fn collect_subtree_ids(el: &yrs::XmlElementRef, txn: &yrs::Transaction, out: &mut Vec<String>) {
    for c in el.children(txn) {
        if let XmlOut::Element(e) = c {
            if e.tag().as_ref() == MARK_TAG {
                continue;
            }
            if let Some(id) = block_id_of(&e, txn) {
                out.push(id);
            }
            collect_subtree_ids(&e, txn, out);
        }
    }
}

/// 读 own-text 段覆盖（逻辑文本 + 区间-标记集序列）。
fn read_coverage(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
) -> (String, Vec<(u32, u32, BTreeSet<String>)>) {
    let mut text = String::new();
    let mut intervals: Vec<(u32, u32, BTreeSet<String>)> = Vec::new();
    let mut pos = 0u32;
    walk_coverage(
        el,
        txn,
        &mut Vec::new(),
        &mut text,
        &mut intervals,
        &mut pos,
    );
    (text, intervals)
}

fn walk_coverage(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    active: &mut Vec<String>,
    text: &mut String,
    intervals: &mut Vec<(u32, u32, BTreeSet<String>)>,
    pos: &mut u32,
) {
    for c in el.children(txn) {
        match c {
            XmlOut::Text(t) => {
                let s = t.get_string(txn);
                let n = s.chars().count() as u32;
                let set: BTreeSet<String> = active.iter().cloned().collect();
                push_interval(intervals, *pos, *pos + n, &set);
                text.push_str(&s);
                *pos += n;
            }
            XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG => {
                let mut pushed = false;
                for (k, v) in e.attributes(txn) {
                    if k == "kind" {
                        if let Some(kind) = attr_str(&v) {
                            active.push(kind);
                            pushed = true;
                        }
                    }
                }
                walk_coverage(&e, txn, active, text, intervals, pos);
                if pushed {
                    active.pop();
                }
            }
            _ => {} // 子块不入 own-text 段
        }
    }
}

fn push_interval(
    out: &mut Vec<(u32, u32, BTreeSet<String>)>,
    s: u32,
    e: u32,
    set: &BTreeSet<String>,
) {
    if e <= s {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.1 == s && &last.2 == set {
            last.1 = e;
            return;
        }
    }
    out.push((s, e, set.clone()));
}
