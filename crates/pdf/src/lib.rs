//! mdc-pdf — 第二 Document Model（PDF 面，PDF-b 骨架轮）。
//!
//! 定位（pdf_carrier_plan §1）：与 mdcore 块核**并列**的版式文档模型，
//! 互不嵌套、互不依赖（crate 间零 path 依赖——Source 等共享语义为
//! **镜像**而非类型复用）。PDF = 页 + 绘制指令 + 对象图，与块结构模型
//! 异构；产品边界 = 阅读 + 标注（不做内容编辑，主档边界不变）。
//!
//! 职责边界（本轮 = 骨架：纯 Rust 类型 + 命令边界 + 安全策略 + 协议
//! 草案 + 语义锁；**零引擎依赖**——pdfium/lopdf 自 PDF-c/d 引入，决策
//! 记录见 gates.md PDF-b 节）：
//! - **页模型**（[`page`]）：页号/页数/页几何/`/Rotate` 归一化
//!   （`effective_*` 按 90/270 交换宽高）/页区间（构造即校验）；
//! - **大纲**（[`outline`]）：书签树 + 深度优先文档序迭代（单一排序出口）；
//! - **标注模型**（[`annotation`]）：PDF 标准 Annot 语义四变体
//!   （v0 内存态，落盘 PDF-d）+ Human/Ai 溯源 + patch 面（kind 不可变）；
//! - **命令边界**（[`command`]）：六命令 + typed 错误面——Human 与 AI
//!   走同一路径（source 溯源镜像 mdcore 事件源语义）；
//! - **安全策略**（[`security`]）：S2 危险对象族 / S3 限额三行为学 /
//!   S5 加密写拒（SECURITY_POLICY.md v1 的类型层编码，数值权威）；
//! - **会话**（[`document`]）：`PdfSession` 单一调度入口（`execute`）——
//!   只有打开后形态，未打开态在类型层不存在；
//! - **协议草案**（[`protocol`]）：`mdcore://pdf` 面与 `mdc-pdf-*`
//!   事件族（PDF-c 视图层落地时定稿）；
//! - **引擎集成点**（[`engine`]）：`DocumentEngine` trait（PDF-c 绑
//!   pdfium，non-V8 构建纪律——S1.1 JS 不接 / S1.2 XFA 不启用）。

pub mod annotation;
pub mod command;
pub mod document;
pub mod engine;
pub mod outline;
pub mod page;
pub mod protocol;
pub mod security;

pub use command::{Command, CommandError, Outcome, Source};
pub use document::{DocumentMeta, OpenError, PdfDocumentId, PdfSession};
pub use security::{DangerousFamily, Limits, OpenAssessment};
