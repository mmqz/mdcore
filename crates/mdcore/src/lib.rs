//! mdcore — Headless Core v0（主档 R8：九项清单的 Rust 落地）。
//!
//! 职责边界（主档 §9 State Ownership）：
//!   - Canonical Document Model（BlockTree）= 唯一语义真理；
//!   - Rope = 文本存储实现（per-block）；
//!   - View（CM6）持有 cursor/IME/pending，不是状态所有者（R7 裁决）；
//!   - Command History = 唯一历史。
//!
//! 管线：`apply_ops`（唯一变异入口）→ 物化 ID → 顺序求逆+应用（带回滚）→
//! 合并或新条目 → revision 推进 → RevisionLog 记录 → 事件出队。
//!
//! 十条语义不变量（主档 §22）对应：不变量 1/2/3/5/6/7/8/9 在本 crate
//! 有直接实现与测试；4（target/precondition）= Reject 枚举；10（View 可替换）
//! = Core 不含任何 View 概念。

pub mod anchor;
pub mod debug;
pub mod document;
pub mod external;
pub mod fingerprint;
pub mod history;
pub mod html;
pub mod id;
pub mod inline;
pub mod markdown;
pub mod op;
pub mod revision;
pub mod snapshot;
pub mod tree;

/// R24-c 版本单源：workspace.package（根 Cargo.toml）→ CARGO_PKG_VERSION
/// → 本常量。发布产物（打包脚本 / 宿主 --version / 关于页）一律取此值，
/// 禁止手抄版本号（r24_packaging.rs 跨 crate 一致性锁背书）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use std::collections::VecDeque;
use std::sync::Arc;

use anchor::{Anchor, AnchorError, Selection, TextTarget};
use history::{Entry, History, Kind, Source};
use id::BlockId;
use op::{apply_op, invert_op, Op, OpError};
use revision::RevisionLog;
use tree::{Attrs, BlockKind, BlockTree, NodePath};

// ---------------------------------------------------------------------------
// 公共类型
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ApplyMeta {
    pub source: Source,
    pub command: Option<String>,
}

impl ApplyMeta {
    pub fn view(command: &str) -> Self {
        Self {
            source: Source::View,
            command: Some(command.to_string()),
        }
    }
    pub fn ai(command: &str) -> Self {
        Self {
            source: Source::Ai,
            command: Some(command.to_string()),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct EventMeta {
    pub source: Source,
    pub coalesced: bool,
    pub command: Option<String>,
}

/// View 与 AI 订阅的同一 op 流事件（轮询式取走）。
#[derive(Clone, Debug, serde::Serialize)]
pub struct Event {
    pub ops: Vec<Op>,
    pub meta: EventMeta,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub revision: u64,
}

/// 命令拒绝（§11.2/§11.3：结构性 stale 必须 reject，不做静默 rebase）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Reject {
    Stale {
        current: u64,
    },
    Anchor(AnchorError),
    Invalid(String),
    /// 渐进打开（R10-a）：chunk 悬挂在未完结的块构造上（如未闭合 fence
    /// 收在 EOF）。在构造任何 op 之前拒绝，树零污染。
    IncompleteChunk(String),
}

impl From<OpError> for Reject {
    fn from(e: OpError) -> Self {
        Reject::Invalid(format!("{e:?}"))
    }
}

type ClockFn = Arc<dyn Fn() -> u64 + Send + Sync>;

#[cfg(not(target_arch = "wasm32"))]
fn system_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// wasm32 无 std 时钟：宿主（JS Date.now / performance.now）经 with_clock 注入。
#[cfg(target_arch = "wasm32")]
fn system_ms() -> u64 {
    0
}

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

pub struct Core {
    pub tree: BlockTree,
    pub history: History,
    pub log: RevisionLog,
    revision: u64,
    /// 文档身份（§8.3 / R10-a 文件层，见 document.rs）。
    document_id: Option<document::DocumentId>,
    selection: Option<Selection>,
    events: VecDeque<Event>,
    clock: ClockFn,
    /// R24-b 脏标记：自上次 `mark_saved` 以来是否有状态变更。粘性语义
    /// （只增不减，mark_saved 清位）：View/Ai/Undo/Redo 置位；System 装载
    /// 事务（渐进打开/adopt_external）不置位（缓冲 == 外部内容）；
    /// undo 回到已存内容仍报脏 = 安全向假阳性（宁可多问一次存盘，
    /// 不可漏报）。供宿主「关闭前确认」/ 会话驱逐策略 /
    /// `plan_reload(local_dirty)` 权威取值。
    dirty: bool,
}

impl Core {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(system_ms))
    }

    pub fn with_clock(clock: ClockFn) -> Self {
        Self {
            tree: BlockTree::new(),
            history: History::default(),
            log: RevisionLog::default(),
            revision: 0,
            document_id: None,
            selection: None,
            events: VecDeque::new(),
            clock,
            dirty: false,
        }
    }

    pub fn from_markdown(md: &str) -> Self {
        Self::from_markdown_with_clock(md, Arc::new(system_ms))
    }

    pub fn from_markdown_with_clock(md: &str, clock: ClockFn) -> Self {
        let mut core = Self::with_clock(clock);
        core.tree = markdown::markdown_to_tree(md);
        // genesis 记录：使 Revision 日志自含初始文档（重放 = 空核 + 日志即可复原）。
        // 物化 ID（Some）：重放时按原身份重建（R8 §8.2 语义）。
        let mut ops = Vec::new();
        let root_path: NodePath = Vec::new();
        for (i, &t) in core.tree.top_level().iter().enumerate() {
            push_subtree_ops(&core.tree, t, &root_path, i, &mut ops, true);
        }
        core.log
            .record(0, &ops, Source::View, Some("genesis".into()));
        core
    }

    /// 宿主时钟注入（wasm32 无 SystemTime；合并窗 / entry.at_ms 由宿主提供）。
    /// 语义不变：只换时钟源，不碰任何状态。
    pub fn set_clock(&mut self, clock: ClockFn) {
        self.clock = clock;
    }

    // ---- 检视 ----

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// R20 内存审计：组件级活数据估计聚合（JSON）。
    /// 供宿主内存面板与性能轮归因使用（ropey per-block 开销 / 日志克隆 /
    /// undo 栈 / 待发事件）。
    pub fn mem_stats(&self) -> serde_json::Value {
        serde_json::json!({
            "tree": self.tree.mem_stats(),
            "log": self.log.mem_stats(),
            "history": self.history.mem_stats(),
            "events_pending": self.events.len(),
            "revision": self.revision,
            "dirty": self.dirty,
        })
    }

    /// R24-b：自上次 [`mark_saved`] 以来是否有状态变更（脏标记，粘性
    /// 语义）。新核/重放/快照复原 = clean；View/Ai/Undo/Redo 事务置位；
    /// System 装载事务不置位；`adopt_external` 成功后清位（缓冲 == 外部
    /// 磁盘内容）。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// R24-b：保存落盘成功后由宿主调用（核不碰 fs，写盘成败宿主域）。
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn now_ms(&self) -> u64 {
        (self.clock)()
    }

    pub fn set_selection(&mut self, sel: Selection) {
        self.selection = Some(sel);
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }

    // -----------------------------------------------------------------------
    // apply 管线：唯一变异入口
    // -----------------------------------------------------------------------

    /// 应用一批 op = 一个事务。合并规则见 history；结构类 op 一律新条目。
    ///
    /// R21 / ADR-B4：`Source::System` 事务（核自驱装载，如
    /// extend_from_markdown）**不入 undo 栈**——undo 栈 = 交互编辑史
    /// （行为锚点：打开文档不可撤销）。System 分支仍 bump revision、
    /// 仍记 revision log（重放 == 实时状态）、仍 push_event，并推进合并窗
    /// 边界（System 后第一个 View 击键必开新条目）；唯一差异是历史栈
    /// 通道不记录。判定边界规则见 ADR-B4：source = 谁驱动了内容变化。
    pub fn apply_ops(&mut self, mut ops: Vec<Op>, meta: ApplyMeta) -> Result<Receipt, Reject> {
        if ops.is_empty() {
            return Ok(Receipt {
                revision: self.revision,
            });
        }
        let kind = if ops.iter().any(|o| !o.is_text_class()) {
            Kind::Structural
        } else {
            Kind::Text
        };
        let now = self.now_ms();

        // 顺序求逆 + 应用；任一失败 → 整体回滚（事务原子性）
        let backup = self.tree.clone();
        let mut inverses_seq: Vec<Op> = Vec::new();
        for op in ops.iter_mut() {
            // 逐 op 求逆必须基于该 op 之前的现场
            let inv = match invert_op(&self.tree, op) {
                Ok(v) => v,
                Err(e) => {
                    self.tree = backup;
                    return Err(e.into());
                }
            };
            if let Err(e) = apply_op(&mut self.tree, op) {
                self.tree = backup;
                return Err(e.into());
            }
            inverses_seq.extend(inv);
        }
        inverses_seq.reverse(); // 存储为逆序（undo 按数组序应用）

        if meta.source == Source::System {
            // ADR-B4：装载级事务——log + 事件 + 边界照常，历史栈跳过。
            let rev = self.bump_revision();
            self.log
                .record(rev, &ops, meta.source, meta.command.clone());
            self.history.note_boundary(now, kind, meta.source);
            self.push_event(
                ops,
                EventMeta {
                    source: meta.source,
                    coalesced: false,
                    command: meta.command,
                },
            );
            return Ok(Receipt { revision: rev });
        }

        let coalesce =
            self.history.should_coalesce(now, kind, meta.source) && self.history.redo_depth() == 0;

        if coalesce {
            self.history
                .coalesce_into_last(ops.clone(), inverses_seq, now, meta.source);
            self.log
                .record(self.revision, &ops, meta.source, meta.command.clone());
            self.push_event(
                ops,
                EventMeta {
                    source: meta.source,
                    coalesced: true,
                    command: meta.command,
                },
            );
        } else {
            let entry = Entry {
                revision: self.bump_revision(),
                ops: ops.clone(),
                inverses: inverses_seq,
                selection: self.selection,
                command: meta.command.clone(),
                at_ms: now,
                kind,
            };
            self.log
                .record(entry.revision, &ops, meta.source, meta.command.clone());
            self.history.push_entry(entry, meta.source);
            self.push_event(
                ops,
                EventMeta {
                    source: meta.source,
                    coalesced: false,
                    command: meta.command,
                },
            );
        }
        // R24-b：交互事务（View/Ai）置脏——System 分支已在上方早退。
        self.dirty = true;
        Ok(Receipt {
            revision: self.revision,
        })
    }

    /// undo/redo 与重放使用的原始应用（不进历史；备份回滚仍然生效）。
    fn apply_raw_batch(&mut self, mut ops: Vec<Op>) -> Result<(), Reject> {
        let backup = self.tree.clone();
        for op in ops.iter_mut() {
            if let Err(e) = apply_op(&mut self.tree, op) {
                self.tree = backup;
                return Err(Reject::from(e));
            }
        }
        Ok(())
    }

    fn bump_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }

    fn push_event(&mut self, ops: Vec<Op>, meta: EventMeta) {
        self.events.push_back(Event {
            ops,
            meta,
            revision: self.revision,
        });
    }

    // -----------------------------------------------------------------------
    // §10.2 第一阶段原语（十原语 + transaction）
    // -----------------------------------------------------------------------

    /// §11.2 结构命令的 base_revision 硬门禁：不等即 reject，不 rebase。
    fn gate(&self, base_revision: u64) -> Result<(), Reject> {
        if base_revision != self.revision {
            return Err(Reject::Stale {
                current: self.revision,
            });
        }
        Ok(())
    }

    pub fn insert_block(
        &mut self,
        base: u64,
        parent: NodePath,
        index: usize,
        kind: BlockKind,
        attrs: Attrs,
        text: String,
    ) -> Result<Receipt, Reject> {
        self.gate(base)?;
        self.apply_ops(
            vec![Op::InsertBlock {
                parent,
                index,
                kind,
                attrs,
                text: text.into(),
                id: None,
            }],
            ApplyMeta::view("insert_block"),
        )
    }

    pub fn delete_block(&mut self, base: u64, path: NodePath) -> Result<Receipt, Reject> {
        self.gate(base)?;
        self.apply_ops(
            vec![Op::RemoveBlock { path }],
            ApplyMeta::view("delete_block"),
        )
    }

    pub fn set_block_type(
        &mut self,
        base: u64,
        path: NodePath,
        kind: BlockKind,
    ) -> Result<Receipt, Reject> {
        self.gate(base)?;
        self.apply_ops(
            vec![Op::SetBlockType { path, kind }],
            ApplyMeta::view("set_block_type"),
        )
    }

    pub fn set_attrs(
        &mut self,
        base: u64,
        path: NodePath,
        attrs: Attrs,
    ) -> Result<Receipt, Reject> {
        self.gate(base)?;
        self.apply_ops(
            vec![Op::SetAttrs { path, attrs }],
            ApplyMeta::view("set_attrs"),
        )
    }

    pub fn move_block(
        &mut self,
        base: u64,
        from: NodePath,
        to_parent: NodePath,
        to_index: usize,
    ) -> Result<Receipt, Reject> {
        self.gate(base)?;
        self.apply_ops(
            vec![Op::MoveBlock {
                from,
                to_parent,
                to_index,
            }],
            ApplyMeta::view("move_block"),
        )
    }

    /// §8.2 split：原 ID 留左侧，右侧新 ID。code_block/table/heading 的
    /// 尾部降级为 paragraph（TS spike 同规则）；list/quote 尾部继承 marker/depth。
    pub fn split_block(
        &mut self,
        base: u64,
        path: NodePath,
        offset: usize,
    ) -> Result<Receipt, Reject> {
        self.gate(base)?;
        let node = self
            .tree
            .resolve(&path)
            .ok_or_else(|| Reject::Invalid("split: bad path".into()))?;
        let n = self.tree.node(node);
        let len = n.text.len_chars();
        if offset > len {
            return Err(Reject::Invalid("split: offset beyond text".into()));
        }
        let tail_text = n.text.slice(offset..len).to_string();
        let (tail_kind, tail_attrs) = match n.kind {
            BlockKind::ListItem => (BlockKind::ListItem, {
                let mut a = Attrs::new();
                if let Some(m) = n.attrs.get("marker") {
                    a.insert("marker".into(), m.clone());
                }
                if let Some(d) = n.attrs.get("depth") {
                    a.insert("depth".into(), d.clone());
                }
                a
            }),
            BlockKind::Blockquote => (BlockKind::Blockquote, {
                let mut a = Attrs::new();
                if let Some(d) = n.attrs.get("depth") {
                    a.insert("depth".into(), d.clone());
                }
                a
            }),
            BlockKind::CodeBlock | BlockKind::Table | BlockKind::Heading => {
                (BlockKind::Paragraph, Attrs::new())
            }
            other => (other, Attrs::new()),
        };
        let parent = path[..path.len() - 1].to_vec();
        let index = *path
            .last()
            .ok_or_else(|| Reject::Invalid("split: root".into()))?;
        let ops = vec![
            Op::ReplaceText {
                path: path.clone(),
                start: offset,
                end: len,
                insert: "".into(),
            },
            Op::InsertBlock {
                parent,
                index: index + 1,
                kind: tail_kind,
                attrs: tail_attrs,
                text: tail_text.into(),
                id: None,
            },
        ];
        self.apply_ops(ops, ApplyMeta::view("split_block"))
    }

    /// §8.2 merge：survivor 明确 = 前一个兄弟（默认前/目标 ID），不得自作主张。
    pub fn merge_block(&mut self, base: u64, path: NodePath) -> Result<Receipt, Reject> {
        self.gate(base)?;
        let index = *path
            .last()
            .ok_or_else(|| Reject::Invalid("merge: root".into()))?;
        if index == 0 {
            return Err(Reject::Invalid("merge: no previous sibling".into()));
        }
        let node = self
            .tree
            .resolve(&path)
            .ok_or_else(|| Reject::Invalid("merge: bad path".into()))?;
        let text = self.tree.node(node).text.to_string();
        let prev_path: NodePath = path[..path.len() - 1]
            .iter()
            .copied()
            .chain(std::iter::once(index - 1))
            .collect();
        let prev = self
            .tree
            .resolve(&prev_path)
            .ok_or_else(|| Reject::Invalid("merge: no previous sibling".into()))?;
        let prev_len = self.tree.node(prev).text.len_chars();
        let ops = vec![
            Op::ReplaceText {
                path: prev_path,
                start: prev_len,
                end: prev_len,
                insert: text.into(),
            },
            Op::RemoveBlock { path },
        ];
        self.apply_ops(ops, ApplyMeta::view("merge_block"))
    }

    /// 文本命令（§11.3 text-local）：不做 base 硬门禁；由锚点/区间/expect 保护。
    pub fn replace_text(&mut self, target: TextTarget, insert: String) -> Result<Receipt, Reject> {
        let op = self.resolve_text_target(target, insert)?;
        self.apply_ops(vec![op], ApplyMeta::view("replace_text"))
    }

    pub fn set_mark(
        &mut self,
        target: TextTarget,
        kind: String,
        add: bool,
    ) -> Result<Receipt, Reject> {
        let op = match target {
            TextTarget::ByPath { path, start, end } => Op::SetMark {
                path,
                start,
                end,
                kind,
                add,
            },
            TextTarget::ByAnchor {
                block, start, end, ..
            } => {
                let path = self.anchor_path(block)?;
                Op::SetMark {
                    path,
                    start,
                    end,
                    kind,
                    add,
                }
            }
        };
        self.apply_ops(vec![op], ApplyMeta::view("set_mark"))
    }

    /// `transaction` 原语：一批预构造 op 作为一个事务/一条历史提交。
    pub fn commit_transaction(&mut self, base: u64, ops: Vec<Op>) -> Result<Receipt, Reject> {
        self.commit_transaction_as(base, ops, ApplyMeta::view("transaction"))
    }

    /// R12（Human+AI 闭环）：带源的 gate 版事务提交。此前所有带 base_revision
    /// 硬门禁的命令面都硬编码 Source::View——AI 只能二选一：走 `apply_ops`
    /// （保住 Source::Ai 溯源，但拿不到 §11.2 的 stale gate），或走
    /// `commit_transaction`（拿到 gate，却把 AI 命令误记成 View 源）。
    /// 本方法让调用方（宿主/Planner）同时声明两者；执行仍走既有
    /// gate → apply_ops 管道，无旁路（§10.1/§22 不变量 2/5/8）。
    pub fn commit_transaction_as(
        &mut self,
        base: u64,
        ops: Vec<Op>,
        meta: ApplyMeta,
    ) -> Result<Receipt, Reject> {
        self.gate(base)?;
        self.apply_ops(ops, meta)
    }

    /// 源码态提交（View A 块编辑回解析）：单原子块原位改（ID 保持）；
    /// 容器/多块结果则移除重建（新 ID——§8.3 匹配失败宁可新 ID）。
    pub fn commit_source(
        &mut self,
        base: u64,
        path: NodePath,
        source: &str,
    ) -> Result<Receipt, Reject> {
        self.gate(base)?;
        let parsed = markdown::markdown_to_tree(source);
        let top: Vec<_> = parsed.top_level().to_vec();
        if top.len() == 1 && parsed.node(top[0]).children.is_empty() {
            let data = parsed.collect_subtree(top[0]);
            let node = self
                .tree
                .resolve(&path)
                .ok_or_else(|| Reject::Invalid("commit_source: bad path".into()))?;
            let len = self.tree.node(node).text.len_chars();
            let ops = vec![
                Op::SetBlockType {
                    path: path.clone(),
                    kind: data.kind,
                },
                Op::SetAttrs {
                    path: path.clone(),
                    attrs: data.attrs.clone(),
                },
                Op::ReplaceText {
                    path,
                    start: 0,
                    end: len,
                    insert: data.text.into(),
                },
            ];
            return self.apply_ops(ops, ApplyMeta::view("commit_source"));
        }
        // 重建路径：remove 后在同位置插入解析森林（post-removal 语义下 index 不变）。
        // R9 修复：id 一律 None（现场分配新 ID）。此前携带解析树自造的 ID
        // （从 1 起号），与现存注册表冲突会覆盖 BlockId→槽位映射（§8.2）。
        let parent = path[..path.len() - 1].to_vec();
        let index = *path
            .last()
            .ok_or_else(|| Reject::Invalid("commit_source: root".into()))?;
        let mut ops = vec![Op::RemoveBlock { path: path.clone() }];
        for (i, &t) in top.iter().enumerate() {
            push_subtree_ops(&parsed, t, &parent, index + i, &mut ops, false);
        }
        self.apply_ops(ops, ApplyMeta::view("commit_source"))
    }

    // ---- 锚点解析（§11.3） ----

    pub fn resolve_anchor(&self, anchor: &Anchor) -> Result<(NodePath, usize), AnchorError> {
        let slot = self
            .tree
            .lookup_id(anchor.block)
            .ok_or(AnchorError::Missing(anchor.block))?;
        let path = self
            .tree
            .path_of(slot)
            .ok_or(AnchorError::Missing(anchor.block))?;
        let len = self.tree.node(slot).text.len_chars();
        if anchor.offset > len {
            return Err(AnchorError::OutOfRange {
                block: anchor.block,
                len,
            });
        }
        Ok((path, anchor.offset))
    }

    fn anchor_path(&self, block: BlockId) -> Result<NodePath, Reject> {
        let (path, _) = self
            .resolve_anchor(&Anchor { block, offset: 0 })
            .map_err(Reject::Anchor)?;
        Ok(path)
    }

    fn resolve_text_target(&self, target: TextTarget, insert: String) -> Result<Op, Reject> {
        match target {
            TextTarget::ByPath { path, start, end } => Ok(Op::ReplaceText {
                path,
                start,
                end,
                insert: insert.into(),
            }),
            TextTarget::ByAnchor {
                block,
                start,
                end,
                expect,
            } => {
                // T1-fix F2：反转区间（start > end）在锚点解析、切片与 op
                // 构造之前拒绝。修复前仅校验 start ≤ len / end ≤ len：
                // expect=Some 时下方 `slice(start..end)` 在反转区间直接
                // panic（ropey 1.6 明示，wasm replace_text_at_anchor 公开
                // 可达）；expect=None 虽经 op 层 BadRange 安全拒绝，但两
                // 路径应统一入口防线（§11.3 承诺 typed-reject）。
                if start > end {
                    return Err(Reject::Anchor(AnchorError::Inverted { block, start, end }));
                }
                let (path, _) = self
                    .resolve_anchor(&Anchor {
                        block,
                        offset: start,
                    })
                    .map_err(Reject::Anchor)?;
                let slot = self.tree.lookup_id(block).expect("resolved above");
                let len = self.tree.node(slot).text.len_chars();
                if end > len {
                    return Err(Reject::Anchor(AnchorError::OutOfRange { block, len }));
                }
                if let Some(expected) = expect {
                    let current: String = self.tree.node(slot).text.slice(start..end).into();
                    if current != expected {
                        return Err(Reject::Anchor(AnchorError::ExpectMismatch { block }));
                    }
                }
                Ok(Op::ReplaceText {
                    path,
                    start,
                    end,
                    insert: insert.into(),
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // 全量重同步兜底（R9：CM6 桥 fallback，与既有 rebuild 行为同款）
    // -----------------------------------------------------------------------

    /// 把 `md` 解析成新树，与当前树做**按位置朴素对照**的最小 op 差分，
    /// 然后走既有 apply_ops 管道（Source::View，command = "sync_from_markdown"），
    /// 保证 history / 事件 / revision 语义与手打一致。
    ///
    /// 差分规则（允许非最优，不追求树编辑距离）：
    /// - 同位置：kind 不同 → SetBlockType；attrs 不同 → SetAttrs；text 不同 →
    ///   ReplaceText(0..len)（原位 op，ID 保持——这是“最小性”的来源）；
    ///   随后递归 diff 子块；
    /// - 旧多新少：按位置从高到低 RemoveBlock（保持低位置路径有效）；
    /// - 新多旧少：从旧长度处起顺序 InsertBlock（id = None，现场分配——
    ///   解析树自造 ID 与现树注册表冲突，绝不可物化进现树）。
    ///
    /// 空差分 = 无 op = 不 bump revision（apply_ops 早退）。
    pub fn sync_from_markdown(&mut self, md: &str) -> Result<Receipt, Reject> {
        let parsed = markdown::markdown_to_tree(md);
        let old_top: Vec<tree::NodeId> = self.tree.top_level().to_vec();
        let new_top: Vec<tree::NodeId> = parsed.top_level().to_vec();
        let mut ops = Vec::new();
        let root_path: NodePath = Vec::new();
        Self::diff_forest(
            &self.tree, &old_top, &parsed, &new_top, &root_path, &mut ops,
        );
        self.apply_ops(
            ops,
            ApplyMeta {
                source: Source::View,
                command: Some("sync_from_markdown".into()),
            },
        )
    }

    /// 按位置对照差分（先序；parent = 当前层相对虚根的路径）。
    fn diff_forest(
        old: &BlockTree,
        old_nodes: &[tree::NodeId],
        new: &BlockTree,
        new_nodes: &[tree::NodeId],
        parent: &NodePath,
        ops: &mut Vec<Op>,
    ) {
        let common = old_nodes.len().min(new_nodes.len());
        for i in 0..common {
            let (o, n) = (old.node(old_nodes[i]), new.node(new_nodes[i]));
            let mut path = parent.to_vec();
            path.push(i);
            if o.kind != n.kind {
                ops.push(Op::SetBlockType {
                    path: path.clone(),
                    kind: n.kind,
                });
            }
            if o.attrs != n.attrs {
                ops.push(Op::SetAttrs {
                    path: path.clone(),
                    attrs: n.attrs.clone(),
                });
            }
            if o.text != n.text {
                ops.push(Op::ReplaceText {
                    path: path.clone(),
                    start: 0,
                    end: o.text.len_chars(),
                    insert: n.text.to_string().into(),
                });
            }
            Self::diff_forest(old, &o.children, new, &n.children, &path, ops);
        }
        // 尾部删除：从高 index 往低删，低位置路径不受影响
        for i in (common..old_nodes.len()).rev() {
            let mut path = parent.to_vec();
            path.push(i);
            ops.push(Op::RemoveBlock { path });
        }
        // 尾部插入：从旧长度处顺序追加（post-removal 语义下 index 连续递增）
        for (j, &nn) in new_nodes[common..].iter().enumerate() {
            push_subtree_ops(new, nn, parent, common + j, ops, false);
        }
    }

    // -----------------------------------------------------------------------
    // 渐进打开（R10-a：大文档分段喍核，调用方/分块器负责块边界切割）
    // -----------------------------------------------------------------------

    /// 把 `chunk` 解析成块森林，全部根块 **append 到文档虚根末尾**。
    /// 语义 = 追加式吃文档后缀：不做差分、不碰已有块（与 sync_from_markdown
    /// 的全量重同步相对）。内部产生 InsertBlock op（id 现场物化）走既有
    /// apply_ops 管道：Source::System（核自驱，永不进合并窗）、
    /// command = "extend_from_markdown"，事件 / 历史 / revision 与手打同源。
    ///
    /// 硬保证（ID 一致性）：one-shot `from_markdown(full)` 与分 K 段 extend
    /// （每段在块边界切开的同一 full）的最终树先序 ID 序列完全一致——
    /// 两边都是文档序先序分配：one-shot 的 genesis 按解析序物化，
    /// extend 的活树分配器按 chunk 先序逐一发号。
    ///
    /// 防御：chunk 必须在块边界切开（调用方/分块器负责）；核内拒绝悬挂
    /// 状态（未闭合 fence 收在 EOF）→ `Reject::IncompleteChunk`，在任何
    /// op 构造之前返回，树零污染（apply_ops 的事务回滚是第二道保险）。
    pub fn extend_from_markdown(&mut self, chunk: &str) -> Result<Receipt, Reject> {
        if !markdown::is_complete_chunk(chunk) {
            return Err(Reject::IncompleteChunk(
                "chunk ends inside an unclosed code fence".into(),
            ));
        }
        let parsed = markdown::markdown_to_tree(chunk);
        let base = self.tree.top_level().len();
        let root_path: NodePath = Vec::new();
        let mut ops = Vec::new();
        for (i, &t) in parsed.top_level().iter().enumerate() {
            push_subtree_ops(&parsed, t, &root_path, base + i, &mut ops, false);
        }
        self.apply_ops(
            ops,
            ApplyMeta {
                source: Source::System,
                command: Some("extend_from_markdown".into()),
            },
        )
    }

    // -----------------------------------------------------------------------
    // History 操作
    // -----------------------------------------------------------------------

    pub fn undo(&mut self) -> Option<Receipt> {
        let boundary = self.history.boundary();
        let entry = self.history.pop_undo()?;
        let inverses = entry.inverses.clone();
        if self.apply_raw_batch(inverses.clone()).is_err() {
            // T1-fix F10：失败不再静默丢历史。可达面：set_history_json /
            // open_document 可载入与当前树不一致的历史（invert/apply 走
            // BadPath/BadRange）。条目压回 undo 栈 + 合并窗边界拨回调用前；
            // 树由 apply_raw_batch 的既有 backup 恢复——失败后状态与
            // 调用前完全一致。
            self.history.push_back_undo(entry);
            self.history.restore_boundary(boundary);
            return None;
        }
        self.history.push_redo(entry);
        let rev = self.bump_revision();
        // R11 审查修正：undo 记入日志（此前不记 → 重放与实时状态分叉，
        // 违背“日志自含 + 确定性重放”承诺）。重放侧按普通 op 序列应用。
        self.log
            .record(rev, &inverses, Source::Undo, Some("undo".into()));
        self.push_event(
            inverses,
            EventMeta {
                source: Source::Undo,
                coalesced: false,
                command: Some("undo".into()),
            },
        );
        self.dirty = true; // R24-b：状态相对保存点变更（粘性，见字段注）
        Some(Receipt { revision: rev })
    }

    pub fn redo(&mut self) -> Option<Receipt> {
        let boundary = self.history.boundary();
        let entry = self.history.pop_redo()?;
        let ops = entry.ops.clone();
        if self.apply_raw_batch(ops.clone()).is_err() {
            // T1-fix F10：同 undo——条目压回 redo 栈 + 边界拨回，树由既有
            // backup 恢复，失败后状态与调用前完全一致。
            self.history.push_redo(entry);
            self.history.restore_boundary(boundary);
            return None;
        }
        self.history.push_back_undo(entry);
        let rev = self.bump_revision();
        self.log
            .record(rev, &ops, Source::Redo, Some("redo".into()));
        self.push_event(
            ops,
            EventMeta {
                source: Source::Redo,
                coalesced: false,
                command: Some("redo".into()),
            },
        );
        self.dirty = true; // R24-b：同 undo
        Some(Receipt { revision: rev })
    }

    pub fn set_history_json(&mut self, json: &str) -> Result<(), serde_json::Error> {
        self.history = History::from_json(json)?;
        Ok(())
    }

    pub fn history_json(&self) -> String {
        self.history.to_json()
    }

    // -----------------------------------------------------------------------
    // Snapshot / Replay
    // -----------------------------------------------------------------------

    pub fn snapshot(&self) -> snapshot::Snapshot {
        snapshot::Snapshot::capture(&self.tree, self.revision)
    }

    /// 由 snapshot 重建核。`from_json` 产物已过 [`snapshot::Snapshot::validate`]
    /// 不变量校验（T1-fix O4：损坏/恶意 JSON 在反序列化边界拒收，树不经
    /// build 零污染）；`capture` 产物恒合法（活树不变量 + 分配器单调性）。
    /// 程序化构造 snapshot 的宿主请先自行 `validate()`。
    pub fn restore(snap: snapshot::Snapshot) -> Self {
        let (tree, revision) = snap.into_tree();
        let mut core = Self::new();
        core.tree = tree;
        core.revision = revision;
        core
    }

    /// Revision 日志重放：空核逐条 raw 应用（op 内 ID 已物化，重放确定）。
    /// 重建文档状态与 revision；undo 栈不重建（设计如此，见 revision.rs）。
    /// R10-a：genesis 记录携带的 document_id 一并恢复（多记录冲突 = 拒绝）。
    pub fn replay_jsonl(jsonl: &str) -> Result<Self, String> {
        let records = RevisionLog::from_jsonl(jsonl).map_err(|e| e.to_string())?;
        let mut document_id: Option<document::DocumentId> = None;
        for r in &records.records {
            if let Some(id) = r.document_id {
                match document_id {
                    None => document_id = Some(id),
                    Some(prev) if prev != id => {
                        return Err(format!(
                            "conflicting document_id in revision log: {prev} vs {id}"
                        ));
                    }
                    _ => {}
                }
            }
        }
        let mut core = Self::new();
        for r in records.records {
            let mut ops = r.ops;
            let backup = core.tree.clone();
            for op in ops.iter_mut() {
                if let Err(e) = apply_op(&mut core.tree, op) {
                    core.tree = backup;
                    return Err(format!("replay failed at revision {}: {e:?}", r.revision));
                }
            }
            core.revision = r.revision;
        }
        core.document_id = document_id;
        Ok(core)
    }
}

/// 将解析子树展开为 InsertBlock op 序列（先序）。
/// `materialize_ids = true` 仅用于 genesis 重放记录（按原身份重建，§8.2）；
/// 向**活树**插入时必须传 false（id = None，现场分配），因为解析树的自造
/// ID 会与现树注册表冲突（R9 修复 commit_source 的这一隐患）。
fn push_subtree_ops(
    src: &BlockTree,
    node: tree::NodeId,
    parent: &NodePath,
    index: usize,
    out: &mut Vec<Op>,
    materialize_ids: bool,
) {
    let n = src.node(node);
    out.push(Op::InsertBlock {
        parent: parent.to_vec(),
        index,
        kind: n.kind,
        attrs: n.attrs.clone(),
        text: n.text.to_string().into(),
        id: if materialize_ids { Some(n.id.0) } else { None },
    });
    for (i, &c) in n.children.iter().enumerate() {
        let mut sub = parent.to_vec();
        sub.push(index);
        push_subtree_ops(src, c, &sub, i, out, materialize_ids);
    }
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_core_invariants() {
        let mut c = Core::new();
        assert_eq!(c.revision(), 0);
        assert!(!c.history.can_undo());
        assert!(c.undo().is_none());
        assert!(c.take_events().is_empty());
        assert!(c.snapshot().to_json().contains("\"revision\":0"));
    }

    /// R21 / ADR-B4：System 事务不入 undo 栈——即使 op 全为 Text 类
    /// （wasm apply_ops source="system" 同路径；extend 的 Structural 场景
    /// 另见 tests/document_progressive.rs）。log / 事件 / revision 照常；
    /// System 边界破组靠 last_source=System（kind 同为 Text、同钟同窗
    /// 也不得合并）。
    #[test]
    fn system_apply_ops_filtered_from_undo_stack() {
        let mut c = Core::from_markdown("alpha\nbeta");
        // View 击键 → 条目 1（边界 = Text/View）
        c.apply_ops(
            vec![Op::ReplaceText {
                path: vec![0],
                start: 0,
                end: 5,
                insert: "ALPHA".into(),
            }],
            ApplyMeta {
                source: Source::View,
                command: Some("type".into()),
            },
        )
        .unwrap();
        assert_eq!(c.history.undo_depth(), 1);

        // System Text 事务（固定时钟 42_000，与两侧 View 击键同窗）：
        // 不 push_entry / 不 coalesce——栈深度不变；revision/log/事件照常
        c.set_clock(Arc::new(|| 42_000));
        c.apply_ops(
            vec![Op::ReplaceText {
                path: vec![1],
                start: 0,
                end: 4,
                insert: "BETA".into(),
            }],
            ApplyMeta {
                source: Source::System,
                command: Some("programmatic".into()),
            },
        )
        .unwrap();
        assert_eq!(
            c.history.undo_depth(),
            1,
            "System 事务不入栈：栈上仍是前一条 View 条目"
        );
        assert_eq!(c.revision(), 2, "System 事务照常 bump revision");
        let ev = c.take_events();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1].meta.source, Source::System);
        assert!(!ev[1].meta.coalesced);
        let last = c.log.records.last().unwrap();
        assert_eq!(last.source, Source::System);
        assert_eq!(last.revision, 2);

        // 同钟 View 击键必开新条目（last_source=System 破组；kind 同为
        // Text、时间同窗——若边界未推进会被吸进条目 1）
        c.apply_ops(
            vec![Op::ReplaceText {
                path: vec![1],
                start: 0,
                end: 4,
                insert: "GAMMA".into(),
            }],
            ApplyMeta {
                source: Source::View,
                command: Some("type2".into()),
            },
        )
        .unwrap();
        assert_eq!(
            c.history.undo_depth(),
            2,
            "System 边界破组：View 击键开新条目"
        );

        // undo 撤 edit2（GAMMA→BETA）：System 的 BETA 与条目 1 的 ALPHA 均不动
        c.undo().unwrap();
        let text_at = |p: usize| c.tree.node(c.tree.resolve(&[p]).unwrap()).text.to_string();
        assert_eq!(text_at(0), "ALPHA");
        assert_eq!(text_at(1), "BETA");
        assert_eq!(c.history.undo_depth(), 1);
    }

    /// R22-a：op 文本共享回归证明——`OpText = Arc<str>` 使 log 深拷贝
    /// （`RevisionLog::record` 的 `ops.to_vec()`）与事件流不再逐字复制文本，
    /// 三方共享同一堆分配（引用计数）；`Op::Clone` 同机制。JSON 形状不变
    /// （serde "rc"），旧 JSONL / 历史 JSON 仍可读。
    #[test]
    fn op_text_shared_across_log_events_and_clones() {
        let mut c = Core::new();
        let ops = vec![Op::InsertBlock {
            parent: vec![],
            index: 0,
            kind: BlockKind::Paragraph,
            attrs: Attrs::new(),
            text: "shared-body".into(),
            id: None,
        }];
        c.apply_ops(
            ops.clone(),
            ApplyMeta {
                source: Source::System,
                command: Some("extend".into()),
            },
        )
        .unwrap();

        let orig = match &ops[0] {
            Op::InsertBlock { text, .. } => text,
            _ => unreachable!("test constructs InsertBlock only"),
        };
        // 1) log 克隆与原 op 共享文本分配（String 时代是独立深拷贝）
        let logged = match &c.log.records[0].ops[0] {
            Op::InsertBlock { text, .. } => text,
            _ => unreachable!(),
        };
        assert!(Arc::ptr_eq(orig, logged), "log 克隆必须共享文本分配");

        // 2) 事件流持有被 move 的原 ops，同样共享
        let events = c.take_events();
        let ev = match &events[0].ops[0] {
            Op::InsertBlock { text, .. } => text,
            _ => unreachable!(),
        };
        assert!(Arc::ptr_eq(orig, ev), "事件流必须共享文本分配");

        // 3) Op::Clone 共享（历史条目 entry.ops = ops.clone() 同机制）
        let cloned = ops[0].clone();
        match cloned {
            Op::InsertBlock { text, .. } => assert!(Arc::ptr_eq(orig, &text)),
            _ => unreachable!(),
        }

        // 4) JSON 形状不变：序列化含原文，反序列化后 PartialEq 成立
        let json = serde_json::to_string(&ops[0]).expect("op serialize");
        assert!(json.contains("shared-body"), "序列化形状与 String 时代一致");
        let back: Op = serde_json::from_str(&json).expect("op deserialize");
        assert_eq!(back, ops[0], "JSON 往返等价");
    }
}
