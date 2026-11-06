//! Stable Block ID — 主档 §8.2/§8.3/§8.4。
//!
//! - 单调分配器，**永不回收**（delete 失效即弃，§8.2）。
//! - 明确否决 Merkle/内容哈希 ID（§8.4）：内容变化不传播身份。
//! - 不写入 Markdown（§8.3）；持久身份走 workspace metadata 映射——
//!   #244（审计 P0-1）已落地：`save_document` 把先序 ID 表写进 meta
//!   （`meta_format` v2），`open_document` 按表恢复（见 document.rs）。
//! - undo/redo/replay 中 ID 复原：history entry 与 inverse op 携带已物化的
//!   具体 ID（见 lib.rs `materialize`），allocator 只前进不回退；物化 ID
//!   落树必经 `BlockTree::create_node_with_id` 撞号守卫（I4，#244）。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockId(pub u64);

impl BlockId {
    pub fn debug_label(self) -> String {
        format!("b{}", self.0)
    }
}

/// 单调分配器。`next` 只增不减；恢复历史/重放时若遇到更大的 ID，
/// 以 `observe` 抬升水位，保证重启后绝不复用旧 ID（§8.2 delete 失效 + 防撞车）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdAllocator {
    next: u64,
}

impl IdAllocator {
    pub fn new() -> Self {
        Self { next: 1 }
    }

    pub fn allocate(&mut self) -> BlockId {
        let id = BlockId(self.next);
        self.next += 1;
        id
    }

    pub fn observe(&mut self, id: BlockId) {
        if id.0 >= self.next {
            self.next = id.0 + 1;
        }
    }

    pub fn watermark(&self) -> u64 {
        self.next
    }
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_never_recycles() {
        let mut a = IdAllocator::new();
        let x = a.allocate();
        let y = a.allocate();
        assert_eq!(x, BlockId(1));
        assert_eq!(y, BlockId(2));
        a.observe(BlockId(100));
        assert_eq!(a.allocate(), BlockId(101));
    }
}
