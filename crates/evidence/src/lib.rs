//! mdc-evidence —— 外挂式文档证据系统通用基础层（S1，修订实施计划
//! §四·2026-10-21）。
//!
//! # 产品语义（一段话）
//!
//! 正文（DOCX 等）保持普通文件形态，可在 Word/WPS 正常打开且不含任何
//! 工作台专属标记；关键数字与结论的证据（来源定位、原文摘录、计算
//! 过程、审核记录）全部存放在**独立 sidecar**，经文档版本指纹与引擎
//! 专属定位器关联正文。正文或来源变更后指纹不匹配 → 证据显式失效，
//! 只有重新验证才能恢复有效，**绝不静默重绑**。
//!
//! # 模块地图
//!
//! - [`refs`]：文档身份——`DocumentRef`（文档 ID/引擎/版本/内容指纹）、
//!   `EngineKind`。
//! - [`fingerprint`]：指纹算法 trait + FNV-1a 64 默认实现（算法前缀
//!   编码；非防篡改，sha256 升级路径在档）。
//! - [`locator`]：分引擎定位器 `Locator`（XLSX 单元格/区域、DOCX
//!   段落/表格行、Markdown 块、PDF 页——后两者为后续扩展预留的最小
//!   形状）。
//! - [`status`]：三态正交模型——`LinkageStatus`（来源关联）×
//!   `CalcStatus`（计算）× `ReviewStatus`（人工审核）。三者独立表达，
//!   见模块文档的语义边界。
//! - [`evidence`]：`EvidenceRecord`（正文主张 → 来源 + 摘录 + 关系
//!   类型 + 主张数值）与 `CalculationRecord`（输入/公式/单位/精度/
//!   中间值/复算）。
//! - [`calc`]：确定性十进制 `Decimal`（i128 定点 + 显式舍入规则）——
//!   财务数据不走二进制浮点。
//! - [`expr`]：表达式白名单求值器（§四.4 四式覆盖：数量×单价/求和/
//!   比例/单位换算；资源四限 + 求值轨迹 + 单位传播 + `recalc`
//!   复算链）。
//! - [`command`]：命令信封 `CommandEnvelope`（幂等键/预期版本/操作者）
//!   与 `CommandOutcome`；`audit`：追加式 `AuditEvent`。
//! - [`sidecar`]：sidecar 存储 v1（JSON、原子写、模式版本校验、
//!   幂等重放表）。
//!
//! # 纪律锚点
//!
//! - 正文与证据分离：本 crate 无任何"向正文写标记"的 API（结构性
//!   不可能，非约定）。
//! - 验证诚实性：程序只验证**可程序验证**的面（定位器可达、公式可
//!   复算、版本一致）；无法自动验证的文本主张必须停留
//!   `ReviewStatus::Pending`，不因模型置信度自动通过（§四.3）。
//! - 存储敏感面：sidecar 含原文摘录 = 敏感数据，默认本地，不上传
//!   （§二.3）；本 crate 不含任何网络代码（结构性保证）。
//!
//! # S1 轮次边界（v0.2.0 = 契约层 + 求值器）
//!
//! 已交付：数据契约 + 状态机语义 + sidecar v1 + 白名单求值器 +
//! 复算级联。**不含**：office-io 真实文档接入（依赖倒置，调用方
//! 注入指纹/定位能力——s1-r3 工作台层）、命令分发执行（S2）、
//! UI（不预设）。

pub mod audit;
pub mod calc;
pub mod command;
pub mod evidence;
pub mod expr;
pub mod fingerprint;
pub mod locator;
pub mod refs;
pub mod sidecar;
pub mod status;

pub use audit::AuditSeq;
pub use calc::{Decimal, Rounding};
pub use command::{AuditEvent, CommandEnvelope, CommandOutcome, ExpectedVersion, IdempotencyKey};
pub use evidence::{CalcInput, CalculationRecord, EvidenceRecord, PrecisionRule, RelationKind};
pub use expr::{evaluate, recalc, CurrentInput, EvalError, EvalOutcome, RecalcOutcome};
pub use fingerprint::{fingerprint_of, Fingerprinter, Fnv64};
pub use locator::{CellCoord, Locator};
pub use refs::{ContentFingerprint, DocId, DocumentRef, EngineKind, VersionStamp};
pub use sidecar::{Sidecar, SidecarError, SidecarStore};
pub use status::{CalcFailure, CalcStatus, LinkageStatus, ReviewStatus};
