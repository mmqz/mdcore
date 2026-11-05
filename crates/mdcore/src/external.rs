//! 外部文件重载（P1-8 核侧，R23-c / ADR-B7）——探针、决策、身份保持采纳。
//!
//! 职责边界（与宿主分工）：
//! - **文件监视是宿主的事**（watcher / 窗口聚焦时机 / 平台 API）；核只提供
//!   纯数据探针 [`FileStamp`]（无 std::fs，wasm 兼容）与决策/采纳原语。
//! - **决策** [`plan_reload`]：匹配报告 + 本地是否有未存改动 → 三态动作。
//! - **采纳** [`Core::adopt_external`]：take-external 语义——按 ADR-B6 匹配
//!   报告携带身份全量重建（System 事务，ADR-B4 不入 undo 栈），交互史整体
//!   作废（被放弃文档态的 undo/redo 对重建后的树无意义）。
//!
//! 宿主标准流程：捕获开档/存档时 FileStamp → 聚焦/watcher 触发时比对新
//! FileStamp → stale 则读盘 + [`Core::match_external_markdown`] 预览报告 +
//! [`plan_reload`] → Noop 忽略 / SilentAdopt 直接 adopt / OfferChoice 弹
//! 冲突 UX（keep-mine = 不 adopt；take-external = adopt；merge = v0 不做）。

use serde::{Deserialize, Serialize};

use crate::fingerprint::{match_trees, BlockMatchReport, ManifestEntry, MatchParams};
use crate::history::Source;
use crate::op::Op;
use crate::tree::{NodeData, NodePath};
use crate::{ApplyMeta, Core, Reject};

// ---------------------------------------------------------------------------
// 探针：纯数据，宿主喂真实 fs 值
// ---------------------------------------------------------------------------

/// 文件戳（mtime 毫秒 + 字节长）。核不碰文件系统——宿主从平台 API 取值。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    pub modified_ms: u64,
    pub size: u64,
}

impl FileStamp {
    /// 外部文件是否已相对本戳变化（任一字段不同即 stale）。
    /// 毫秒精度 mtime 在极快连续写入下可能同值——size 兜底；双同值才判 fresh。
    pub fn is_stale(&self, current: &FileStamp) -> bool {
        self != current
    }
}

// ---------------------------------------------------------------------------
// 决策：报告 + 本地脏 → 三态
// ---------------------------------------------------------------------------

/// 重载动作（`plan_reload` 输出，宿主 UX 状态机的输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReloadAction {
    /// 外部内容与现树逐块等价（mtime 变化多为自存回声/触碰）——忽略。
    Noop,
    /// 本地无未存改动 → 静默重载（不打扰）。
    SilentAdopt,
    /// 本地有未存改动 + 外部有变化 → 冲突 UX（keep-mine / take-external）。
    OfferChoice,
}

/// 纯函数决策（ADR-B7 §3）。v0 口径：只要有**任何**外部变化且本地脏，
/// 一律 OfferChoice——不做"变化区域不相交则安全合并"的区域级推断
/// （核无逐块本地脏标记，贸然合并等于替用户做决定；v0 明确收缩）。
pub fn plan_reload(report: &BlockMatchReport, local_dirty: bool) -> ReloadAction {
    if !report.has_changes() {
        ReloadAction::Noop
    } else if !local_dirty {
        ReloadAction::SilentAdopt
    } else {
        ReloadAction::OfferChoice
    }
}

// ---------------------------------------------------------------------------
// 采纳：身份保持的全量重建（System 事务）
// ---------------------------------------------------------------------------

/// adopt 结果（事件/可观测性面）。
#[derive(Clone, Debug, Serialize)]
pub struct AdoptOutcome {
    /// 采纳时依据的匹配报告（与预览报告同构；重跑匹配保证原子一致性）。
    pub report: BlockMatchReport,
    /// 携带旧身份的块数（preserved + moved + edited + manifest 可信 added）。
    pub carried_ids: usize,
    /// 全新分配身份的块数（added 无可信 id）。
    pub fresh_ids: usize,
    pub revision_before: u64,
    pub revision_after: u64,
}

impl Core {
    /// take-external：把外部 `md` 采纳为新文档态，按匹配报告**携带身份**。
    ///
    /// - op 合成 = 清空旧顶层（RemoveBlock[0] × n）+ 先序重建
    ///   （InsertBlock 携带映射 id：preserved/moved/edited 用旧 id，
    ///   manifest 可信 added 用声明 id，其余现场分配）——与 undo 复原的
    ///   `build_rebuild_ops` 同款物化形状，log 重放不变量保持。
    /// - System 事务（ADR-B4 不入 undo 栈）；成功后交互史整体作废
    ///   （ADR-B7 §4：被放弃文档态的 undo/redo 条目对重建后的树会
    ///   BadPath，保留即坏引用）。
    /// - 无变化时零事务零 revision 推进（Noop 语义，幂等）。
    /// - 原子性：apply_ops 事务内失败 → 整树备份恢复（事务入口
    ///   `backup = tree.clone()`、失败 `self.tree = backup`），树零污染
    ///   （#220 勘误：此处原注「求逆回滚（ADR-B5）」不实——该机制
    ///   从未在 mdcore 落地，研究仓侧声称经审计 P1-27 对账证伪）；
    ///   本方法不加自己的前半段突变，天然可回滚。
    pub fn adopt_external(
        &mut self,
        md: &str,
        manifest: Option<&[ManifestEntry]>,
    ) -> Result<AdoptOutcome, Reject> {
        let parsed = crate::markdown::markdown_to_tree(md);
        let report = match_trees(&self.tree, &parsed, manifest, &MatchParams::default());
        let revision_before = self.revision;
        if !report.has_changes() {
            return Ok(AdoptOutcome {
                report,
                carried_ids: 0,
                fresh_ids: 0,
                revision_before,
                revision_after: revision_before,
            });
        }

        // 身份映射：新树路径 → 携带 id。BTreeMap（确定性，平局不可能——
        // 路径唯一）。preserved/moved/edited 的 new_path 即新树先序位。
        let mut carry: std::collections::BTreeMap<NodePath, u64> = Default::default();
        for m in report
            .preserved
            .iter()
            .chain(&report.moved)
            .chain(&report.edited)
        {
            carry.insert(m.new_path.clone(), m.id.0);
        }
        for a in &report.added {
            if let Some(id) = a.id {
                // manifest 快路径带来的可信新身份（合作写手场景）——携带，非新分配。
                carry.insert(a.path.clone(), id.0);
            }
        }

        // op 合成：RemoveBlock[0] 反复 n 次（每次摘首个顶层块，路径恒 valid）
        // + 先序重建。插入路径 = 新树内建位（旧内容已清空）。
        let old_top = self.tree.top_level().len();
        let mut ops: Vec<Op> = Vec::with_capacity(old_top + parsed.node_count());
        for _ in 0..old_top {
            ops.push(Op::RemoveBlock { path: vec![0] });
        }
        let mut carried = 0usize;
        let mut fresh = 0usize;
        let tops = parsed.top_level().to_vec();
        for (i, &root) in tops.iter().enumerate() {
            let data = parsed.collect_subtree(root);
            emit_rebuild(&data, &[], i, &carry, &mut ops, &mut carried, &mut fresh);
        }

        let receipt = self.apply_ops(
            ops,
            ApplyMeta {
                source: Source::System,
                command: Some("external_reload".into()),
            },
        )?;
        // ADR-B7 §4：take-external 放弃本地未存交互史——undo/redo/合并窗状态全清。
        self.history.clear_interaction();
        // R24-b：采纳后缓冲 == 外部磁盘内容 → 脏标记清位（Noop 早退路径
        // 不清：无变更无采纳，脏态原样保留由宿主 UX 决定）。
        self.mark_saved();
        Ok(AdoptOutcome {
            report,
            carried_ids: carried,
            fresh_ids: fresh,
            revision_before,
            revision_after: receipt.revision,
        })
    }
}

/// 先序重建 op 发射（与 op::build_rebuild_ops 同形状；marks 不入 InsertBlock——
/// 外部文件本无 marks，与 undo 复原路径的 v0 口径一致）。
fn emit_rebuild(
    data: &NodeData,
    parent: &[usize],
    index: usize,
    carry: &std::collections::BTreeMap<NodePath, u64>,
    ops: &mut Vec<Op>,
    carried: &mut usize,
    fresh: &mut usize,
) {
    let mut path = parent.to_vec();
    path.push(index);
    let id = match carry.get(&path) {
        Some(&x) => {
            *carried += 1;
            Some(x)
        }
        None => {
            *fresh += 1;
            None
        }
    };
    ops.push(Op::InsertBlock {
        parent: parent.to_vec(),
        index,
        kind: data.kind,
        attrs: data.attrs.clone(),
        text: data.text.as_str().into(),
        id,
    });
    for (j, child) in data.children.iter().enumerate() {
        emit_rebuild(child, &path, j, carry, ops, carried, fresh);
    }
}
