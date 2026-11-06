//! mdc-mdbridge —— MD 引擎 ↔ 协作引擎 op 层对接（应用层组合点）。
//!
//! # 产品语义
//!
//! 把 **mdcore 原子 Op 流**（NodePath 寻址、u64 `BlockId`、char 域文本）
//! 翻译为 **mdc-collab 命令流**（block_id 字符串寻址），使人类/AI 的
//! MD 引擎编辑进入协同会话（wire 同步、按 origin 分栈撤销）；反向把
//! 协作态**物化**为 mdcore `BlockTree`——协作文档即时获得 MD 引擎的
//! 全部读面能力（MD 序列化、树/块指纹、结构锚点）。
//!
//! ```text
//!   mdcore Op（NodePath + u64）
//!        │ ①plan：预解析路径→block_id + 镜像协作域前置校验
//!        │ ②apply：shadow BlockTree 原子应用（id 分配/回写）
//!        │ ③execute：CollabCommand 进协作会话（失败→shadow 自愈重物化）
//!        ▼
//!   CollabSession（XmlFragment 载体；wire/undo/snapshot 由 mdc-collab 承担）
//!        │ apply_remote / undo / redo（翻译路径之外的态变更）
//!        ▼   全量重物化（family u64 保身份，foreign 稳定映射）
//!   shadow BlockTree（纯投影——MD 序列化/指纹/锚点消费面）
//! ```
//!
//! # 身份映射决策（本轮对接的核心裁决，报告 §3.1 遗留问题）
//!
//! mdcore `BlockId(u64)` 是**树内单调分配**的身份；协作域
//! `block_id(String)` 是**会话全局唯一**的字符串身份。对接规则：
//!
//! - **本端（family）**：shadow 树分配的 u64 `n` ↔ 协作域
//!   `md:{client_id}:{n}`。确定性编解码（无映射表）；`client_id`
//!   域保证多端零撞号（对端 bridge 的 family 前缀天然不同）。
//! - **外端（foreign）**：非 family 形状的协作域 id（裸协作端
//!   `{client}-{seq}`、对端 bridge 的 `md:{其他client}:{n}`、任意
//!   字符串）→ 物化时由 bridge 分配 u64，**双向映射表持久持有**
//!   （同一 id 跨多次重物化稳定映射；表随 host 生命周期，不进 wire）。
//! - **同族伪造（hostile）**：wire 上出现的 `md:{本端client}:{n}`
//!   若非本端创建——按 family 解码复用 `n` 并 `IdAllocator::observe`
//!   抬升水位：本端后续分配**永不复用**该 n（不撞车），伪造块获得
//!   稳定 u64（可操作可物化，仅身份空间被占用——登记为残留面）。
//! - **restore 换端**：client_id 变化 → 原 family id 全部落入 foreign
//!   类（身份换新族；正确性不变——block_id 字符串仍是协作域寻址面）。
//!
//! # 语义对照表（七操作）
//!
//! | mdcore Op | collab 命令 | 语义对齐 |
//! |---|---|---|
//! | InsertBlock | InsertBlock | index 同为子块逻辑位；id 回写 u64→family 编码 |
//! | RemoveBlock | RemoveBlock | 1:1；协作域锚点悬空语义由 mdc-collab 承担 |
//! | ReplaceText | ReplaceText | 同为 own-text 段 char 域逻辑编辑 |
//! | SetBlockType | SetBlockType | mdcore 原位改 kind；协作域 D-1 删旧建新（载体身份换新族、block_id 连续）——shadow u64 稳定 |
//! | SetAttrs | SetAttrs | 同为终态替换；`block_id` 保留键双侧拒绝 |
//! | SetMark | SetMark | mdcore 字面 span 增删；协作域 D-2 归一化覆盖（字面形状可差、覆盖等价） |
//! | MoveBlock | MoveBlock | **同语义钉定：两者 to_index 均为摘除后坐标**（协作域 remove→insert 同事务序）——bridge 测试锁 |
//!
//! # 已知差异（fail-closed 登记）
//!
//! - **容器面收窄**：协作域仅 Blockquote/ListItem 可承载子块；mdcore
//!   原子 op 层不限制父 kind。bridge 预检镜像协作域约束（拒绝 Heading/
//!   Table 等为父），shadow 不留中途态。
//! - **ThematicBreak 无文本面**：协作域装载/插入拒绝 hr 带文本；bridge
//!   预检同拒（mdcore 原子层允许 hr 带文本——原始树路径绕过 bridge 时
//!   的形态差异不进协作域）。
//! - **SetMark 字面形状**：协作域归一化后区间切分可与 mdcore 字面 span
//!   不同（覆盖等价——测试按逐字符标记集合比对，不断言字面区间相等）。
//! - **撤销粒度**：bridge 每命令一 undo 栈项（mdcore 的命令层组合——
//!   split/merge 等多 op 序列——在协作域为多栈项，逐 op 撤销）。
//!
//! # 零新增三方依赖
//!
//! 仅依赖本 workspace 的 mdcore 与 mdc-collab（Cargo.lock 零变化，
//! NOTICES 无增量面）。

pub mod host;
pub mod translate;

pub use host::{MdCollabHost, PublishReceipt};
pub use translate::{Plan, TranslateError};

use std::collections::BTreeMap;

/// 对接层错误（两引擎错误的显式联合 + 翻译面自身错误）。
#[derive(Debug)]
pub enum BridgeError {
    /// mdcore 侧（shadow 应用/物化构造）。
    Md(mdcore::op::OpError),
    /// mdc-collab 侧（命令执行/同步/快照）。
    Collab(mdc_collab::CollabError),
    /// 翻译面前置校验（协作域约束镜像 / 保留键 / 形状不合法）。
    Translate(TranslateError),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Md(e) => write!(f, "mdcore: {e:?}"),
            BridgeError::Collab(e) => write!(f, "collab: {e}"),
            BridgeError::Translate(e) => write!(f, "translate: {e}"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl From<mdcore::op::OpError> for BridgeError {
    fn from(e: mdcore::op::OpError) -> Self {
        BridgeError::Md(e)
    }
}

impl From<mdc_collab::CollabError> for BridgeError {
    fn from(e: mdc_collab::CollabError) -> Self {
        BridgeError::Collab(e)
    }
}

impl From<TranslateError> for BridgeError {
    fn from(e: TranslateError) -> Self {
        BridgeError::Translate(e)
    }
}

// ── BlockKind 全映射（mdcore ↔ 协作域；Document = 根特例）────────

/// mdcore kind → 协作域 kind（Document 无协作域块级对应——根即会话）。
pub fn kind_to_collab(k: mdcore::tree::BlockKind) -> Option<mdc_collab::BlockKind> {
    use mdc_collab::BlockKind as C;
    use mdcore::tree::BlockKind as M;
    Some(match k {
        M::Document => return None,
        M::Paragraph => C::Paragraph,
        M::Heading => C::Heading,
        M::ListItem => C::ListItem,
        M::Blockquote => C::Blockquote,
        M::CodeBlock => C::CodeBlock,
        M::Math => C::Math,
        M::Table => C::Table,
        M::ThematicBreak => C::ThematicBreak,
    })
}

/// 协作域 kind → mdcore kind（全函数——八 kind 一一对应）。
pub fn kind_to_md(k: mdc_collab::BlockKind) -> mdcore::tree::BlockKind {
    use mdc_collab::BlockKind as C;
    use mdcore::tree::BlockKind as M;
    match k {
        C::Paragraph => M::Paragraph,
        C::Heading => M::Heading,
        C::ListItem => M::ListItem,
        C::Blockquote => M::Blockquote,
        C::CodeBlock => M::CodeBlock,
        C::Math => M::Math,
        C::Table => M::Table,
        C::ThematicBreak => M::ThematicBreak,
    }
}

// ── 身份编解码（family 面）──────────────────────────────────────

/// family 前缀（协作域 block_id 的 bridge 保留命名空间）。
pub const FAMILY_PREFIX: &str = "md:";

/// u64 → family 协作域 block_id（`md:{client_id}:{n}`）。
pub fn family_id(client_id: u64, n: u64) -> String {
    format!("{FAMILY_PREFIX}{client_id}:{n}")
}

/// family 协作域 block_id → (client_id, n)；非 family 形状 = None。
pub fn parse_family_id(block_id: &str) -> Option<(u64, u64)> {
    let rest = block_id.strip_prefix(FAMILY_PREFIX)?;
    let (c, n) = rest.split_once(':')?;
    let client = c.parse::<u64>().ok()?;
    let num = n.parse::<u64>().ok()?;
    Some((client, num))
}

/// 属性面保留键镜像（协作域载体身份键——双侧拒绝）。
pub const RESERVED_ATTR_KEY: &str = "block_id";

/// mdcore 属性集 → 协作域属性集（同形直传；保留键预检在 plan 面）。
pub fn attrs_to_collab(attrs: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    attrs.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_codec_roundtrip() {
        let id = family_id(7_100_001, 42);
        assert_eq!(id, "md:7100001:42");
        assert_eq!(parse_family_id(&id), Some((7_100_001, 42)));
        // 非 family 形状：裸协作端 id / 坏格式。对端 family（不同
        // client）解析成功——分类（本端/外端）是 host 层职责。
        assert_eq!(parse_family_id("7100001-3"), None);
        assert_eq!(parse_family_id("md:7100002:3"), Some((7_100_002, 3)));
        assert_eq!(parse_family_id("md:7100001:x"), None);
        assert_eq!(parse_family_id("md:"), None);
        assert_eq!(parse_family_id("xmd:7100001:3"), None);
    }

    #[test]
    fn kind_mapping_total() {
        use mdc_collab::BlockKind as C;
        use mdcore::tree::BlockKind as M;
        let all = [
            (M::Paragraph, C::Paragraph),
            (M::Heading, C::Heading),
            (M::ListItem, C::ListItem),
            (M::Blockquote, C::Blockquote),
            (M::CodeBlock, C::CodeBlock),
            (M::Math, C::Math),
            (M::Table, C::Table),
            (M::ThematicBreak, C::ThematicBreak),
        ];
        for (m, c) in all {
            assert_eq!(kind_to_collab(m), Some(c));
            assert_eq!(kind_to_md(c), m);
        }
        assert_eq!(kind_to_collab(M::Document), None);
    }
}
