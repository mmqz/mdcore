//! Anchor — (BlockId, offset) 寻址层（ADR-B1 混合式：path 变化由 ID 消化）。
//!
//! §11.3 Text-local Command 规则（v0 实现）：
//!   - 目标块存在 → 映射到当前 path 应用；
//!   - 目标块被删 → AnchorMissing → reject（绝不静默猜测，§8.4 原则）；
//!   - 区间越界 → AnchorRange → reject；
//!   - 区间反转（start > end）→ Inverted → reject（T1 修复：ByAnchor
//!     入口前置拦截，防 ropey slice panic）；
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
    ByPath {
        path: Vec<usize>,
        start: usize,
        end: usize,
    },
    /// AI/延迟命令走 ID 锚点（结构变化后仍可解析）。
    ByAnchor {
        block: BlockId,
        start: usize,
        end: usize,
        expect: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum AnchorError {
    Missing(BlockId),
    /// 区间反转（start > end）。T1 修复：ByAnchor 路径此前只校验
    /// start ≤ len / end ≤ len，expect=Some 时 `Rope::slice(start..end)`
    /// 在反转区间上直接 panic（ropey 文档明示）；现于锚点解析入口
    /// 拒绝，expect=Some/None 两路径统一防线。start == end 合法
    /// （空区间 = 纯插入语义）。
    Inverted {
        block: BlockId,
        start: usize,
        end: usize,
    },
    OutOfRange {
        block: BlockId,
        len: usize,
    },
    ExpectMismatch {
        block: BlockId,
    },
}
