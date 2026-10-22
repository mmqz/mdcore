//! [`MdCollabHost`] —— MD 引擎编辑接入协同会话的宿主（对接执行序）。
//!
//! **真相源纪律**：`CollabSession` 是协同文档唯一真相源；`shadow`
//! BlockTree 是纯投影。本地 op 走「plan → shadow 增量 → collab 命令」
//! 三段；一切翻译路径之外的态变更（远端同步 / 撤销重做 / 发布 /
//! collab 命令失败自愈）= 全量重物化（family u64 保身份，foreign
//! 稳定映射——见 lib.rs 身份映射决策）。

use std::collections::{HashMap, HashSet};

use mdc_collab::{
    Actor, BlockSpec, CollabCommand, CollabError, CollabSession, CommandReceipt, CrdtAnchor,
    ResolvedAnchor, SessionOptions,
};
use mdcore::id::BlockId;
use mdcore::op::apply_op;
use mdcore::op::Op;
use mdcore::tree::{BlockTree, MarkSpan, NodeData, NodePath};

use crate::translate::{IdResolver, Plan};
use crate::{family_id, kind_to_md, parse_family_id, BridgeError};

/// 协作会话宿主（shadow BlockTree 投影持有者）。
pub struct MdCollabHost {
    session: CollabSession,
    shadow: BlockTree,
    client_id: u64,
    /// foreign 映射（协作域 id → 物化 u64；跨重物化稳定）。
    foreign: HashMap<String, u64>,
    /// 反查（物化 u64 → 协作域 id；materialize 轮末由 foreign 重建）。
    inverse_foreign: HashMap<u64, String>,
}

impl IdResolver for MdCollabHost {
    /// shadow u64 → 协作域 block_id（foreign 反查或 family 编码）。
    fn collab_id_of(&self, n: u64) -> Option<String> {
        if let Some(c) = self.inverse_foreign.get(&n) {
            return Some(c.clone());
        }
        // 未登记 foreign = 本端 family 面（编解码确定性）。
        Some(family_id(self.client_id, n))
    }
}

impl MdCollabHost {
    /// 开新宿主（空文档 + 空 shadow）。
    pub fn open(options: SessionOptions) -> Result<MdCollabHost, BridgeError> {
        let client_id = options.client_id;
        Ok(MdCollabHost {
            session: CollabSession::open(options)?,
            shadow: BlockTree::new(),
            client_id,
            foreign: HashMap::new(),
            inverse_foreign: HashMap::new(),
        })
    }

    /// 从权威快照恢复（撤销栈不随快照走——mdc-collab 语义）。
    ///
    /// client_id 与快照原端一致 → family u64 全保留；不一致 → 原
    /// family 落 foreign 类（身份换新族，正确性不变——lib.rs 登记）。
    pub fn restore(options: SessionOptions, snapshot: &[u8]) -> Result<MdCollabHost, BridgeError> {
        let client_id = options.client_id;
        let mut host = MdCollabHost {
            session: CollabSession::restore(options, snapshot)?,
            shadow: BlockTree::new(),
            client_id,
            foreign: HashMap::new(),
            inverse_foreign: HashMap::new(),
        };
        host.materialize()?;
        Ok(host)
    }

    // ── 只读面 ───────────────────────────────────────────────

    /// shadow 投影（MD 引擎消费面：序列化 / 指纹 / 锚点 / 调试）。
    pub fn tree(&self) -> &BlockTree {
        &self.shadow
    }

    /// 协作会话只读引用（读模型/锚点查询；态变更走宿主方法）。
    pub fn session(&self) -> &CollabSession {
        &self.session
    }

    pub fn options(&self) -> &SessionOptions {
        self.session.options()
    }

    /// shadow 路径 → 协作域 block_id。
    pub fn block_id_of(&self, path: &[usize]) -> Option<String> {
        let nid = self.shadow.resolve(path)?;
        let n = self.shadow.node(nid).id.0;
        self.collab_id_of(n)
    }

    /// 协作域 block_id → shadow 路径（family/foreign 双面）。
    pub fn path_of(&self, block_id: &str) -> Option<NodePath> {
        let u64id = self.foreign.get(block_id).copied().or_else(|| {
            parse_family_id(block_id).and_then(|(c, n)| (c == self.client_id).then_some(n))
        })?;
        let nid = self.shadow.lookup_id(BlockId(u64id))?;
        self.shadow.path_of(nid)
    }

    // ── 同步/快照/锚点委托（只读或不动文档态的面）────────────

    pub fn state_vector(&self) -> Vec<u8> {
        self.session.state_vector()
    }

    pub fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, CollabError> {
        self.session.encode_diff(remote_sv)
    }

    pub fn authoritative_snapshot(&self) -> Result<Vec<u8>, CollabError> {
        self.session.authoritative_snapshot()
    }

    pub fn full_update_len(&self) -> usize {
        self.session.full_update_len()
    }

    pub fn undo_depth(&self, actor: Actor) -> usize {
        self.session.undo_depth(actor)
    }

    pub fn redo_depth(&self, actor: Actor) -> usize {
        self.session.redo_depth(actor)
    }

    /// 绑定证据锚（锚点表面——不动文档态，shadow 无需同步）。
    pub fn bind_anchor(&mut self, block_id: &str) -> Result<CrdtAnchor, CollabError> {
        self.session.bind_anchor(block_id)
    }

    pub fn anchor_of(&self, block_id: &str) -> Option<CrdtAnchor> {
        self.session.anchor_of(block_id)
    }

    pub fn resolve_anchor(&self, anchor: CrdtAnchor) -> Result<ResolvedAnchor, CollabError> {
        self.session.resolve_anchor(anchor)
    }

    pub fn export_anchor_sidecar(&self) -> Vec<u8> {
        self.session.export_anchor_sidecar()
    }

    /// foreign 映射观测面（确定性序——测试/诊断；`Vec<(协作域 id, u64)>`
    /// 按 id 字典序）。family 面（编解码确定性）不在此表。
    pub fn foreign_snapshot(&self) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = self.foreign.iter().map(|(k, n)| (k.clone(), *n)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    // ── 发布面（本地既有文档 → 协同会话）────────────────────

    /// 发布本地 BlockTree 进协作会话（**顶层追加**；预序逐块插入 +
    /// mark 逐条 SetMark）。
    ///
    /// **身份语义 = 重映射（import）**：源树 u64 与会话 family u64 是
    /// 两个独立分配器空间（两者都从 1 起——保身份直传必然撞号）；发布
    /// 为每块分配**新 family u64**（shadow 分配器——水位恒高于全部已
    /// 见 id），identity 延续由回执 `top_ids` 报告（源树 BlockId → 新
    /// block_id 的映射表即回执）。调用方发布后应以宿主 shadow 为准，
    /// 源树仅作导入格式。（需要保 u64 的活会话交接面留显式 adopt 裁决
    /// ——当前无消费方，不预制。）
    ///
    /// - 撤销粒度：每块一栈项（+ 每 mark 一栈项）。
    /// - 全量前置检查（保留键/hr 文本面/Document kind）先行，通过后
    ///   逐命令执行；中途失败 = 自愈重物化 + 显式错误（会话停在已插入
    ///   的部分态——调用方 undo 或续发，不静默回滚）。
    /// - shadow 于轮末全量重物化（family u64 = 本轮新分配的 ns）。
    pub fn publish(
        &mut self,
        tree: &BlockTree,
        actor: Actor,
    ) -> Result<PublishReceipt, BridgeError> {
        if tree.top_level().is_empty() {
            return Ok(PublishReceipt {
                blocks: 0,
                marks: 0,
                top_ids: Vec::new(),
            });
        }
        // 计划：源树预序展开（spec 约束镜像）+ 每块 fresh family u64。
        // （新分配 id 恒不与会话现存 block_id 撞号——family 命名空间由
        // 本端独占，fresh n 高于全部已见水位。）
        let mut planned: Vec<PlannedInsert> = Vec::new();
        let base_index = self.session.document().len() as u32;
        for (i, &top) in tree.top_level().iter().enumerate() {
            let data = tree.collect_subtree(top);
            self.plan_publish(&data, None, base_index + i as u32, &mut planned)?;
        }
        // 执行：逐命令（中途失败 = 部分态 + 自愈 + 显式错误）。
        let mut inserted = 0usize;
        let mut marks_done = 0usize;
        let mut top_ids = Vec::new();
        for p in &planned {
            let cmd = CollabCommand::InsertBlock {
                parent: p.parent.map(|u| family_id(self.client_id, u)),
                index: p.index,
                spec: p.spec.clone(),
                id: Some(family_id(self.client_id, p.self_id)),
            };
            if let Err(e) = self.session.execute(actor, cmd) {
                self.materialize()?;
                return Err(BridgeError::Collab(e));
            }
            inserted += 1;
            if p.parent.is_none() {
                top_ids.push(family_id(self.client_id, p.self_id));
            }
            for m in &p.marks {
                if m.end <= m.start {
                    continue;
                }
                let cmd = CollabCommand::SetMark {
                    block_id: family_id(self.client_id, p.self_id),
                    start: m.start as u32,
                    end: m.end as u32,
                    mark: m.kind.clone(),
                    add: true,
                };
                if let Err(e) = self.session.execute(actor, cmd) {
                    self.materialize()?;
                    return Err(BridgeError::Collab(e));
                }
                marks_done += 1;
            }
        }
        // shadow = 会话真相重物化（family u64 = 本轮新 ns）。
        self.materialize()?;
        Ok(PublishReceipt {
            blocks: inserted,
            marks: marks_done,
            top_ids,
        })
    }

    fn plan_publish(
        &mut self,
        data: &NodeData,
        parent: Option<u64>,
        index: u32,
        planned: &mut Vec<PlannedInsert>,
    ) -> Result<(), BridgeError> {
        let spec = crate::translate::spec_of_node_kind(data)?;
        // fresh family u64（shadow 分配器——水位恒高于已见）。
        let fresh = self.shadow.alloc.allocate().0;
        planned.push(PlannedInsert {
            parent,
            index,
            spec,
            self_id: fresh,
            marks: data.marks.clone(),
        });
        for (i, child) in data.children.iter().enumerate() {
            self.plan_publish(child, Some(fresh), i as u32, planned)?;
        }
        Ok(())
    }

    // ── 核心执行序 ───────────────────────────────────────────

    /// 本地 op 进协同会话（plan → shadow 增量 → collab 命令；
    /// collab 失败 → shadow 自愈重物化后返回错误）。
    ///
    /// `op` 语义与 `mdcore::op::apply_op` 一致：InsertBlock 现场分配
    /// 时**回写 op**（物化 u64）——调用方历史/undo 面可依赖。
    pub fn apply_op(&mut self, actor: Actor, op: &mut Op) -> Result<CommandReceipt, BridgeError> {
        let plan = Plan::make(op, &self.shadow, self)?;
        apply_op(&mut self.shadow, op)?;
        // InsertBlock 现场分配 → apply_op 已写回 op.id（物化 u64）。
        let materialized = match op {
            Op::InsertBlock { id, .. } => *id,
            _ => None,
        };
        let cmd = plan.into_command(self.client_id, materialized)?;
        match self.session.execute(actor, cmd) {
            Ok(receipt) => Ok(receipt),
            Err(e) => {
                // 自愈：collab 是真相源，shadow 回投会话态。
                self.materialize()?;
                Err(BridgeError::Collab(e))
            }
        }
    }

    /// 撤销（origin 分栈；shadow 全量重物化）。
    pub fn undo(&mut self, actor: Actor) -> Result<bool, BridgeError> {
        let r = self.session.undo(actor);
        self.materialize()?;
        Ok(r)
    }

    /// 重做（origin 分栈；shadow 全量重物化）。
    pub fn redo(&mut self, actor: Actor) -> Result<bool, BridgeError> {
        let r = self.session.redo(actor);
        self.materialize()?;
        Ok(r)
    }

    /// 远端 update 应用（入向限额 + panic 隔离由 mdc-collab 承担）；
    /// 成功后 shadow 全量重物化（新块 foreign 稳定登记）。
    pub fn apply_remote(&mut self, bytes: &[u8]) -> Result<(), BridgeError> {
        self.session.apply_update(bytes)?;
        self.materialize()?;
        Ok(())
    }

    // ── 物化面（协作态 → shadow BlockTree）──────────────────

    /// 全量重物化：`session.document()` → 全新 BlockTree。
    ///
    /// 身份规则（lib.rs 决策）：family（`md:{本端client}:{n}`）复用
    /// u64 + 水位观察（后续本地分配永不复用）；foreign 查表复用（被
    /// family 占位 → 换新[身份churn登记]）；未登记 foreign 分配新
    /// u64 入表；**空 block_id / 撞号重复 id（敌意载体形态）= 孤儿
    /// 分配**（不入表——身份跨轮不稳定，结构保真优先）。
    pub fn materialize(&mut self) -> Result<usize, BridgeError> {
        let doc = self.session.document();
        // 预序展平（路径 + 视图）。
        let mut flat: Vec<(NodePath, mdc_collab::BlockInfo)> = Vec::new();
        flatten(&doc, &mut Vec::new(), &mut flat);
        // 分配轮：family/foreign/孤儿三面（used 集合保证轮内唯一）。
        let mut shadow = BlockTree::new();
        let mut used: HashSet<u64> = HashSet::new();
        let mut new_foreign: HashMap<String, u64> = self.foreign.clone();
        let mut assigned: Vec<u64> = Vec::with_capacity(flat.len());
        for (_, info) in &flat {
            let uid = assign_u64(
                info.block_id.as_str(),
                self.client_id,
                &mut new_foreign,
                &mut shadow,
                &mut used,
            );
            assigned.push(uid);
        }
        // 建树（预序直插——物化 id；父先于子[预序保证]）。
        let mut pending_marks: Vec<(NodePath, Vec<MarkSpan>)> = Vec::new();
        let mut count = 0usize;
        for ((path, info), uid) in flat.iter().zip(assigned.iter()) {
            let parent: NodePath = path[..path.len() - 1].to_vec();
            let index = *path.last().expect("非空路径");
            let mut op = Op::InsertBlock {
                parent,
                index,
                kind: kind_to_md(info.kind),
                attrs: info.attrs.clone(),
                text: info.text.as_str().into(),
                id: Some(*uid),
            };
            apply_op(&mut shadow, &mut op).map_err(BridgeError::Md)?;
            count += 1;
            if !info.marks.is_empty() {
                let spans = info
                    .marks
                    .iter()
                    .map(|m| MarkSpan {
                        start: m.start as usize,
                        end: m.end as usize,
                        kind: m.mark.clone(),
                    })
                    .collect();
                pending_marks.push((path.clone(), spans));
            }
        }
        // marks 后置通道（插入 op 无 span 承载面——SetMark 逐条）。
        for (path, spans) in pending_marks {
            for s in spans {
                if s.end <= s.start {
                    continue; // 空区间零操作（敌意载体防御）
                }
                let mut op = Op::SetMark {
                    path: path.clone(),
                    start: s.start,
                    end: s.end,
                    kind: s.kind,
                    add: true,
                };
                apply_op(&mut shadow, &mut op).map_err(BridgeError::Md)?;
            }
        }
        // 提交：shadow + foreign 双表（inverse 由 foreign 全量重建）。
        self.inverse_foreign = new_foreign.iter().map(|(k, v)| (*v, k.clone())).collect();
        self.foreign = new_foreign;
        self.shadow = shadow;
        Ok(count)
    }
}

/// 发布回执。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReceipt {
    /// 发布块数（含子块）。
    pub blocks: usize,
    /// 发布 mark 条数。
    pub marks: usize,
    /// 顶层 family id 序（发布块的身份面——供调用方登记定位器）。
    pub top_ids: Vec<String>,
}

/// 发布计划行（预序）。
struct PlannedInsert {
    parent: Option<u64>,
    index: u32,
    spec: BlockSpec,
    self_id: u64,
    marks: Vec<MarkSpan>,
}

fn flatten(
    blocks: &[mdc_collab::BlockInfo],
    path: &mut NodePath,
    out: &mut Vec<(NodePath, mdc_collab::BlockInfo)>,
) {
    for (i, b) in blocks.iter().enumerate() {
        path.push(i);
        out.push((path.clone(), b.clone()));
        flatten(&b.children, path, out);
        path.pop();
    }
}

/// 物化 u64 分配（family 复用 / foreign 查表 / 孤儿兜底）。
///
/// 水位纪律：family 复用与 foreign 复用均 `observe` 入分配器——后续
/// 本地现场分配（`allocate`）恒高于全部已见 u64，永不撞车。
fn assign_u64(
    collab_id: &str,
    client_id: u64,
    foreign: &mut HashMap<String, u64>,
    shadow: &mut BlockTree,
    used: &mut HashSet<u64>,
) -> u64 {
    // family：本端 client 前缀 → 复用 n + 水位观察（后续分配永不复用）。
    if let Some((c, n)) = parse_family_id(collab_id) {
        if c == client_id && !used.contains(&n) {
            shadow.alloc.observe(BlockId(n));
            used.insert(n);
            return n;
        }
    }
    // foreign 已登记：复用（被 family 占位 → 落到孤儿分配换新）。
    if let Some(&m) = foreign.get(collab_id) {
        if !used.contains(&m) {
            shadow.alloc.observe(BlockId(m));
            used.insert(m);
            return m;
        }
    }
    // 孤儿/新 foreign：现场分配（水位已在全部已见 u64 之上）。
    let fresh = shadow.alloc.allocate().0;
    used.insert(fresh);
    if !collab_id.is_empty() {
        foreign.insert(collab_id.to_string(), fresh);
    }
    fresh
}
