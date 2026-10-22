//! 命令接口（人类与 AI 共用——同一入口、同一验证、origin 分栈）。
//!
//! 命令层纪律：本枚举是文档变更的**受控全集**（七操作 + 装载）；
//! 无任意路径/脚本面。`execute` 每命令恰好一个语义动作（装载 =
//! 小批多事务，undo 粒度登记于各命令文档）。

use std::collections::BTreeMap;

use crate::events::{ChangeSource, CollabEvent};
use crate::session::{Actor, CollabError, CollabSession};
use crate::{BlockKind, BlockSpec};

/// 文档命令全集（人类/AI 共用）。
#[derive(Debug, Clone)]
pub enum CollabCommand {
    /// 批量装载（AI 生成态典型形状；顶层追加；512/事务——undo 粒度
    /// = 每小批一栈项）。
    LoadBlocks { blocks: Vec<BlockSpec> },
    /// 插入块（`id = None` 现场分配 / `Some` 物化恢复[undo/replay
    /// 回写路径]）。
    InsertBlock {
        parent: Option<String>,
        index: u32,
        spec: BlockSpec,
        id: Option<String>,
    },
    /// 删除块（按 block_id；锚点悬空态语义）。
    RemoveBlock { block_id: String },
    /// 文本替换（char 域；own-text 段全域——跨 mark 逻辑位置）。
    ReplaceText {
        block_id: String,
        start: u32,
        end: u32,
        insert: String,
    },
    /// 种类变更（D-1 重建语义）。
    SetBlockType { block_id: String, kind: BlockKind },
    /// 属性终态替换（`block_id` 保留键拒收）。
    SetAttrs {
        block_id: String,
        attrs: BTreeMap<String, String>,
    },
    /// 行内标记（D-2 非重叠最小区间等价覆盖）。
    SetMark {
        block_id: String,
        start: u32,
        end: u32,
        mark: String,
        add: bool,
    },
    /// 块搬移（同事务原子；环回拒）。
    MoveBlock {
        block_id: String,
        to_parent: Option<String>,
        to_index: u32,
    },
}

/// 命令回执（结构化结果 + 撤销面观测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReceipt {
    pub outcome: CommandOutcome,
    /// 执行方 origin 的撤销栈深度（操作后）。
    pub undo_depth: usize,
    /// 软上限触发 clear_all（撤销历史全失的显式回执——不静默）。
    pub undo_cleared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    BlocksLoaded { ids: Vec<String> },
    BlockInserted { block_id: String },
    BlockRemoved { block_id: String },
    TextReplaced { block_id: String, new_len: u32 },
    TypeChanged { block_id: String },
    AttrsSet { block_id: String },
    MarkSet { block_id: String, add: bool },
    BlockMoved { block_id: String },
}

impl CollabSession {
    /// 受控命令执行（人类与 AI 的同一入口）。成功且产出 CRDT 字节的
    /// 命令派发 [`crate::events::ChangeSource::Command`] 事件（一个
    /// 命令恰一个事件；批量装载的逐小批事务合并为 updates 序）。
    pub fn execute(
        &mut self,
        actor: Actor,
        cmd: CollabCommand,
    ) -> Result<CommandReceipt, CollabError> {
        self.collector.begin();
        let result = self.execute_inner(actor, cmd);
        let updates = self.collector.drain();
        match result {
            Ok(receipt) => {
                if !updates.is_empty() {
                    self.hub.dispatch(&CollabEvent {
                        source: ChangeSource::Command { actor },
                        updates,
                    });
                }
                Ok(receipt)
            }
            // 失败不产事件（错误经返回值显式传递；残留字节丢弃——
            // 部分提交面由错误返回驱动宿主重读，双通道不重复）。
            Err(e) => Err(e),
        }
    }

    /// 命令实施体（事件包裹的内层——`?` 早退不绕过收集器清理）。
    fn execute_inner(
        &mut self,
        actor: Actor,
        cmd: CollabCommand,
    ) -> Result<CommandReceipt, CollabError> {
        let outcome = match cmd {
            CollabCommand::LoadBlocks { blocks } => {
                let ids = self.load_blocks(actor, &blocks)?;
                CommandOutcome::BlocksLoaded { ids }
            }
            CollabCommand::InsertBlock {
                parent,
                index,
                spec,
                id,
            } => {
                let block_id = self.insert_block(actor, parent.as_deref(), index, &spec, id)?;
                CommandOutcome::BlockInserted { block_id }
            }
            CollabCommand::RemoveBlock { block_id } => {
                self.remove_block(actor, &block_id)?;
                CommandOutcome::BlockRemoved { block_id }
            }
            CollabCommand::ReplaceText {
                block_id,
                start,
                end,
                insert,
            } => {
                let new_len = self.replace_text(actor, &block_id, start, end, &insert)?;
                CommandOutcome::TextReplaced { block_id, new_len }
            }
            CollabCommand::SetBlockType { block_id, kind } => {
                self.set_block_type(actor, &block_id, kind)?;
                CommandOutcome::TypeChanged { block_id }
            }
            CollabCommand::SetAttrs { block_id, attrs } => {
                self.set_attrs(actor, &block_id, &attrs)?;
                CommandOutcome::AttrsSet { block_id }
            }
            CollabCommand::SetMark {
                block_id,
                start,
                end,
                mark,
                add,
            } => {
                self.set_mark(actor, &block_id, start, end, &mark, add)?;
                CommandOutcome::MarkSet { block_id, add }
            }
            CollabCommand::MoveBlock {
                block_id,
                to_parent,
                to_index,
            } => {
                self.move_block(actor, &block_id, to_parent.as_deref(), to_index)?;
                CommandOutcome::BlockMoved { block_id }
            }
        };
        let receipt = CommandReceipt {
            outcome,
            undo_depth: self.undo_depth(actor),
            undo_cleared: self.take_undo_cleared(),
        };
        Ok(receipt)
    }
}
