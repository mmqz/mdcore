//! Anchor — (BlockId, offset) 寻址层（ADR-B1 混合式：path 变化由 ID 消化）。
//!
//! §11.3 Text-local Command 规则（v0 实现）：
//!   - 目标块存在 → 映射到当前 path 应用；
//!   - 目标块被删 → AnchorMissing → reject（绝不静默猜测，§8.4 原则）；
//!   - 区间越界 → AnchorRange → reject；
//!   - `expect` 内容不匹配 → ExpectMismatch → reject（乐观并发保护）。
//!
//! 偏移平移映射（周围文本变化自动跟移）属 R9 增强，v0 以 expect 保护。

use serde::{Deserialize, Serialize};

use crate::id::BlockId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub block: BlockId,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub anchor: Anchor,
    pub focus: Anchor,
}

/// 文本命令的目标（§10.2 replace_text/set_mark 的 target）。
#[derive(Clone, Debug, PartialEq)]
pub enum TextTarget {
    /// View 活路径直写（快路径，View 保证 path 新鲜）。
    ByPath { path: Vec<usize>, start: usize, end: usize },
    /// AI/延迟命令走 ID 锚点（结构变化后仍可解析）。
    ByAnchor { block: BlockId, start: usize, end: usize, expect: Option<String> },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum AnchorError {
    Missing(BlockId),
    OutOfRange { block: BlockId, len: usize },
    ExpectMismatch { block: BlockId },
}
