//! Op 翻译面：mdcore `Op` → 协作域 `CollabCommand`（纯函数两段式）。
//!
//! - [`Plan::make`]：**预态**解析（路径 → block_id）+ 协作域约束镜像
//!   预检（容器面 / ThematicBreak 文本面 / 保留键 / mark 非空）。
//!   失败 = 零副作用（shadow 与会话均不动）。
//! - [`Plan::into_command`]：**后态**组命令（InsertBlock 的 id 取
//!   shadow 应用后写回的物化 u64——见 host::apply_op 执行序）。
//!
//! MoveBlock 的 to_index 语义：mdcore 文档声明**摘除后坐标**；协作域
//! 实现 = 同事务 remove→insert（insert 落点即摘除后数组坐标）——两者
//! 同语义，**直传不改写**（bridge_matrix 测试锁）。

use std::collections::BTreeMap;

use mdc_collab::{BlockKind as CKind, BlockSpec, CollabCommand};
use mdcore::op::Op;
use mdcore::tree::{BlockTree, NodeData, NodePath};

use crate::{kind_to_collab, RESERVED_ATTR_KEY};

/// 翻译面前置校验错误（协作域约束镜像——预检失败零副作用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    /// 路径在 shadow 中不可解析（块不存在/已被删除）。
    BadPath(NodePath),
    /// 父块不可承载子块（协作域容器面：Blockquote/ListItem）。
    NotAContainer(NodePath),
    /// ThematicBreak 无文本面（带文本插入 / 文本或标记编辑目标）。
    KindIncompatible(&'static str),
    /// 属性集含协作域保留键 `block_id`。
    ReservedAttrKey,
    /// mark kind 为空串（协作域拒绝）。
    EmptyMark,
    /// InsertBlock 的 kind = Document（根保留——mdcore 侧同拒，
    /// 此处镜像以保零副作用预检）。
    DocumentKind,
    /// 内部不变量破裂（翻译后态缺物化 id 等——apply_op 写回保证下
    /// 不可达；显式错误而非 panic，便于调用方 fail-closed）。
    Internal(&'static str),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslateError::BadPath(p) => write!(f, "path unresolvable in shadow: {p:?}"),
            TranslateError::NotAContainer(p) => {
                write!(
                    f,
                    "parent cannot carry child blocks (collab container face): {p:?}"
                )
            }
            TranslateError::KindIncompatible(op) => {
                write!(f, "ThematicBreak has no {op} face (collab mirror)")
            }
            TranslateError::ReservedAttrKey => {
                write!(f, "attrs contain reserved key `block_id`")
            }
            TranslateError::EmptyMark => write!(f, "mark kind must be non-empty"),
            TranslateError::DocumentKind => write!(f, "Document kind is reserved for root"),
            TranslateError::Internal(m) => write!(f, "internal invariant broken: {m}"),
        }
    }
}

impl std::error::Error for TranslateError {}

/// 预态解析产物（一个 op → 一个命令；mdcore 原子 op 与协作域命令
/// 一一对应——命令层组合由调用方逐 op 驱动）。
pub enum Plan {
    Insert {
        /// 协作域父 id（None = 顶层）。
        parent: Option<String>,
        index: u32,
        spec: BlockSpec,
        /// shadow 物化 u64（None = 现场分配；命令组时填 family id）。
        id: Option<u64>,
    },
    Remove {
        block_id: String,
    },
    ReplaceText {
        block_id: String,
        start: u32,
        end: u32,
        insert: String,
    },
    SetType {
        block_id: String,
        kind: CKind,
    },
    SetAttrs {
        block_id: String,
        attrs: BTreeMap<String, String>,
    },
    SetMark {
        block_id: String,
        start: u32,
        end: u32,
        mark: String,
        add: bool,
    },
    Move {
        block_id: String,
        to_parent: Option<String>,
        to_index: u32,
    },
}

/// 协作域 id 解析器（host 提供：family 编码 + foreign 反查）。
pub trait IdResolver {
    /// shadow u64 → 协作域 block_id。
    fn collab_id_of(&self, n: u64) -> Option<String>;
}

impl Plan {
    /// 预态解析 + 协作域约束镜像预检（零副作用）。
    pub fn make(op: &Op, shadow: &BlockTree, ids: &dyn IdResolver) -> Result<Plan, TranslateError> {
        match op {
            Op::InsertBlock {
                parent,
                index,
                kind,
                attrs,
                text,
                id,
            } => {
                let ckind = kind_to_collab(*kind).ok_or(TranslateError::DocumentKind)?;
                // 父面：空路径 = 顶层；容器面镜像（Blockquote/ListItem）。
                let parent_id = if parent.is_empty() {
                    None
                } else {
                    let pid = shadow
                        .resolve(parent)
                        .ok_or_else(|| TranslateError::BadPath(parent.clone()))?;
                    let pnode = shadow.node(pid);
                    match kind_to_collab(pnode.kind) {
                        Some(k) if k.carries_children() => {}
                        _ => return Err(TranslateError::NotAContainer(parent.clone())),
                    }
                    let cid = ids
                        .collab_id_of(pnode.id.0)
                        .ok_or_else(|| TranslateError::BadPath(parent.clone()))?;
                    Some(cid)
                };
                if attrs.contains_key(RESERVED_ATTR_KEY) {
                    return Err(TranslateError::ReservedAttrKey);
                }
                if ckind == CKind::ThematicBreak && !text.is_empty() {
                    return Err(TranslateError::KindIncompatible("text"));
                }
                Ok(Plan::Insert {
                    parent: parent_id,
                    index: *index as u32,
                    spec: BlockSpec {
                        kind: ckind,
                        attrs: attrs.clone(),
                        text: text.to_string(),
                    },
                    id: *id,
                })
            }
            Op::RemoveBlock { path } => Ok(Plan::Remove {
                block_id: resolve_id(shadow, ids, path)?,
            }),
            Op::ReplaceText {
                path,
                start,
                end,
                insert,
            } => {
                check_carries_text(shadow, ids, path)?;
                Ok(Plan::ReplaceText {
                    block_id: resolve_id(shadow, ids, path)?,
                    start: *start as u32,
                    end: *end as u32,
                    insert: insert.to_string(),
                })
            }
            Op::SetBlockType { path, kind } => {
                let ckind = kind_to_collab(*kind).ok_or(TranslateError::DocumentKind)?;
                Ok(Plan::SetType {
                    block_id: resolve_id(shadow, ids, path)?,
                    kind: ckind,
                })
            }
            Op::SetAttrs { path, attrs } => {
                if attrs.contains_key(RESERVED_ATTR_KEY) {
                    return Err(TranslateError::ReservedAttrKey);
                }
                Ok(Plan::SetAttrs {
                    block_id: resolve_id(shadow, ids, path)?,
                    attrs: attrs.clone(),
                })
            }
            Op::SetMark {
                path,
                start,
                end,
                kind,
                add,
            } => {
                if kind.is_empty() {
                    return Err(TranslateError::EmptyMark);
                }
                check_carries_text(shadow, ids, path)?;
                Ok(Plan::SetMark {
                    block_id: resolve_id(shadow, ids, path)?,
                    start: *start as u32,
                    end: *end as u32,
                    mark: kind.clone(),
                    add: *add,
                })
            }
            Op::MoveBlock {
                from,
                to_parent,
                to_index,
            } => {
                let block_id = resolve_id(shadow, ids, from)?;
                // 目标父面：空路径 = 顶层；容器面镜像。
                let dest_parent = if to_parent.is_empty() {
                    None
                } else {
                    let pid = shadow
                        .resolve(to_parent)
                        .ok_or_else(|| TranslateError::BadPath(to_parent.clone()))?;
                    let pnode = shadow.node(pid);
                    match kind_to_collab(pnode.kind) {
                        Some(k) if k.carries_children() => {}
                        _ => return Err(TranslateError::NotAContainer(to_parent.clone())),
                    }
                    Some(
                        ids.collab_id_of(pnode.id.0)
                            .ok_or_else(|| TranslateError::BadPath(to_parent.clone()))?,
                    )
                };
                Ok(Plan::Move {
                    block_id,
                    to_parent: dest_parent,
                    // 两侧同语义（摘除后坐标）——直传。
                    to_index: *to_index as u32,
                })
            }
        }
    }

    /// 后态组命令。`materialized_id`：InsertBlock 现场分配时 shadow
    /// 写回的 u64（plan.id = None 场景的唯一补充输入）。
    pub fn into_command(
        self,
        client_id: u64,
        materialized_id: Option<u64>,
    ) -> Result<CollabCommand, TranslateError> {
        match self {
            Plan::Insert {
                parent,
                index,
                spec,
                id,
            } => {
                let n = id
                    .or(materialized_id)
                    .ok_or(TranslateError::Internal("insert 物化 u64 缺失"))?;
                Ok(CollabCommand::InsertBlock {
                    parent,
                    index,
                    spec,
                    id: Some(crate::family_id(client_id, n)),
                })
            }
            Plan::Remove { block_id } => Ok(CollabCommand::RemoveBlock { block_id }),
            Plan::ReplaceText {
                block_id,
                start,
                end,
                insert,
            } => Ok(CollabCommand::ReplaceText {
                block_id,
                start,
                end,
                insert,
            }),
            Plan::SetType { block_id, kind } => Ok(CollabCommand::SetBlockType { block_id, kind }),
            Plan::SetAttrs { block_id, attrs } => Ok(CollabCommand::SetAttrs { block_id, attrs }),
            Plan::SetMark {
                block_id,
                start,
                end,
                mark,
                add,
            } => Ok(CollabCommand::SetMark {
                block_id,
                start,
                end,
                mark,
                add,
            }),
            Plan::Move {
                block_id,
                to_parent,
                to_index,
            } => Ok(CollabCommand::MoveBlock {
                block_id,
                to_parent,
                to_index,
            }),
        }
    }
}

fn resolve_id(
    shadow: &BlockTree,
    ids: &dyn IdResolver,
    path: &NodePath,
) -> Result<String, TranslateError> {
    let nid = shadow
        .resolve(path)
        .ok_or_else(|| TranslateError::BadPath(path.clone()))?;
    let node = shadow.node(nid);
    ids.collab_id_of(node.id.0)
        .ok_or_else(|| TranslateError::BadPath(path.clone()))
}

fn check_carries_text(
    shadow: &BlockTree,
    ids: &dyn IdResolver,
    path: &NodePath,
) -> Result<String, TranslateError> {
    let nid = shadow
        .resolve(path)
        .ok_or_else(|| TranslateError::BadPath(path.clone()))?;
    let node = shadow.node(nid);
    let ckind = kind_to_collab(node.kind).ok_or(TranslateError::DocumentKind)?;
    if !ckind.carries_text() {
        return Err(TranslateError::KindIncompatible("text"));
    }
    ids.collab_id_of(node.id.0)
        .ok_or_else(|| TranslateError::BadPath(path.clone()))
}

/// shadow 树文本面 spec 提取（publish 面共用：kind/attrs/text 三元组
/// + 约束镜像预检——保留键 / hr 文本面 / Document kind）。
pub fn spec_of_node_kind(data: &NodeData) -> Result<BlockSpec, TranslateError> {
    let ckind = kind_to_collab(data.kind).ok_or(TranslateError::DocumentKind)?;
    if data.attrs.contains_key(RESERVED_ATTR_KEY) {
        return Err(TranslateError::ReservedAttrKey);
    }
    if ckind == CKind::ThematicBreak && !data.text.is_empty() {
        return Err(TranslateError::KindIncompatible("text"));
    }
    Ok(BlockSpec {
        kind: ckind,
        attrs: data.attrs.clone(),
        text: data.text.clone(),
    })
}
