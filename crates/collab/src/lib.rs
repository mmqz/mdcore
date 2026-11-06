//! mdc-collab —— 协作引擎（人类与 AI 共用的可同步共享文档状态）。
//!
//! # 产品语义
//!
//! Headless 协作子系统：一份结构化文档（块树形状）由多个会话
//! （人类编辑器 / AI 生成端）共享，经 CRDT wire 字节同步收敛；
//! 人类与 AI 走**同一套命令接口**（[`CollabCommand`]），撤销/重做
//! 按 origin（[`Actor::Human`]/[`Actor::Ai`]）分栈互不干扰。
//!
//! ```text
//! 会话 A（人类）──┐                        ┌── 会话 B（AI）
//!   execute(Human, cmd)                    execute(Ai, cmd)
//!        │ origin=mdc:human                     │ origin=mdc:ai
//!        ▼                                      ▼
//!   CollabSession（XmlFragment 载体，skip_gc 不变量）
//!        │ state_vector()/encode_diff()          │
//!        └──────────── wire 字节（宿主传输）──────┘
//!                     apply_update() 双向收敛
//! ```
//!
//! # 结构性纪律（证据链：分析仓 collab-line）
//!
//! - **公开 API 零 `yrs::` 类型**（隔离不变量——撤除 yrs 只拆本
//!   crate）。
//! - **`skip_gc: true` 无条件**（B1.1 全编队不变量，上游 #676 UAF
//!   未修复期间；构造期强制，无配置面）。
//! - **捕获窗 0**（p20 缺陷缓解，2026-10-21 S0 指令；硬编码——
//!   窗>0 + 双栈 + 同窗非连续合并项 = undo 只应用首段 + redo
//!   panic，上游未修复前不作为可调参数暴露）。
//! - **每命令一事务**（B2.1(b) 逐笔路径——一个 undo 栈项 + 一个
//!   origin 溯源单位；批量装载走 512/事务小批路径）。
//! - **入向全限额**（B1.6：装载/编辑/同步/锚点/侧车全路径字节门）
//!   **+ panic 隔离**（apply_update/restore 面 catch_unwind）。
//! - **身份双轨**：`block_id`（应用级字符串，载体属性承载，跨端
//!   不进 wire 协议面语义）× [`CrdtAnchor`]（CRDT 级 ID，证据锚
//!   本位币——结构性编辑后仍可解算，删除后显式悬空错误）。
//!
//! # 语义差异登记（yrs-e §6 三条，均已按矩阵形状实现）
//!
//! - **D-1** `SetBlockType`/`MoveBlock` 载体无原位改名/搬移 API →
//!   删旧建新搬子树（同事务原子）；子树载体身份换新族，`block_id`
//!   语义连续（属性随迁）；锚点表自动重指。
//! - **D-2** marks 重叠 → 非重叠最小区间等价覆盖（SetMark 归一化
//!   重建策略）；读回语义等价、字面区间切分可不同。
//! - **D-3** 载体 XmlText 偏移域 = UTF-8 字节域 → 适配层单点
//!   char↔byte 无损换算（公开面恒 char 域，字节域不泄漏）。

pub mod audit;
pub mod command;
pub mod events;
pub mod identity;
pub mod ops;
pub mod presence;
pub mod session;

pub use audit::{AuditReport, AuditViolation};
pub use command::{CollabCommand, CommandOutcome, CommandReceipt};
pub use events::{ChangeSource, CollabEvent, Subscription};
pub use identity::CrdtAnchor;
pub use presence::{Presence, PresenceEvent, PresenceState, PresenceSubscription};
pub use session::{Actor, CollabError, CollabSession, SessionOptions};

pub use identity::ResolvedAnchor;

/// 块种类（mdcore BlockKind 非根 7 kind 的协作域镜像——Document
/// = 会话根本身，不在块级枚举内）。
///
/// 名称与语义与 mdcore `tree.rs::BlockKind` 一一对应，但这是**协作
/// 域自有类型**（领域模型独立纪律：不依赖 mdcore crate）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Paragraph,
    Heading,
    ListItem,
    Blockquote,
    CodeBlock,
    /// 数学块（#261）：LaTeX 显示公式，text = 源码逐字（语义同构
    /// CodeBlock——own-text 叶块，非容器）。
    Math,
    Table,
    ThematicBreak,
}

impl BlockKind {
    /// 是否可承载文本（own-text 段——ReplaceText/SetMark 的合法面）。
    pub fn carries_text(self) -> bool {
        !matches!(self, BlockKind::ThematicBreak)
    }

    /// 是否可承载子块（InsertBlock 的合法父面）。
    pub fn carries_children(self) -> bool {
        matches!(self, BlockKind::Blockquote | BlockKind::ListItem)
    }
}

/// 块规格（插入/装载入参）。
#[derive(Debug, Clone)]
pub struct BlockSpec {
    pub kind: BlockKind,
    /// 开放属性集（`level`/`lang`/`marker` 等已观测键 + 未知键透传；
    /// `block_id` 为保留键——载体身份由引擎管理）。
    pub attrs: std::collections::BTreeMap<String, String>,
    /// 初始文本（own-text 段；ThematicBreak 恒空）。
    pub text: String,
}

impl BlockKind {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            BlockKind::Paragraph => "p",
            BlockKind::Heading => "h",
            BlockKind::ListItem => "li",
            BlockKind::Blockquote => "blockquote",
            BlockKind::CodeBlock => "pre",
            BlockKind::Math => "math",
            BlockKind::Table => "tbl",
            BlockKind::ThematicBreak => "hr",
        }
    }

    pub(crate) fn from_tag(tag: &str) -> Option<BlockKind> {
        match tag {
            "p" => Some(BlockKind::Paragraph),
            "h" => Some(BlockKind::Heading),
            "li" => Some(BlockKind::ListItem),
            "blockquote" => Some(BlockKind::Blockquote),
            "pre" => Some(BlockKind::CodeBlock),
            "math" => Some(BlockKind::Math),
            "tbl" => Some(BlockKind::Table),
            "hr" => Some(BlockKind::ThematicBreak),
            _ => None,
        }
    }
}

/// 行内标记区间读回视图（`BlockInfo.marks` 元素——own-text 段 char 域，
/// 与 mdcore `MarkSpan` 字段形状一致；领域模型独立纪律：协作域自有类型）。
///
/// 形状 = 展平区间（per-kind）：同一区间多重标记 = 多条记录；
/// 序确定（区间 start 升序，同区间 kind 字典序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkInfo {
    pub start: u32,
    pub end: u32,
    pub mark: String,
}

/// 读模型块视图（`document()`/`find_block()` 产物——快照式，非引用）。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockInfo {
    pub block_id: String,
    pub kind: BlockKind,
    /// 用户属性（`block_id` 载体身份键不在此面）。
    pub attrs: std::collections::BTreeMap<String, String>,
    /// own-text 段逻辑文本（Text 子节点 + mark 元素内文本按序拼接；
    /// 容器块 = 首段自有文本，不含子块文本）。
    pub text: String,
    /// 行内标记（own-text 段 char 域展平区间——载体 m 元素嵌套的
    /// 逻辑读回；materialize 消费面，mdbridge 对接轮引入）。
    pub marks: Vec<MarkInfo>,
    /// 子块（递归）。
    pub children: Vec<BlockInfo>,
}
