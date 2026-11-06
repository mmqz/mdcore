//! mdc-pdf — 第二 Document Model（PDF 面，PDF-b 骨架 + PDF-c 阅读面 +
//! **PDF-d 标注面**）。
//!
//! 定位（pdf_carrier_plan §1）：与 mdcore 块核**并列**的版式文档模型，
//! 互不嵌套、互不依赖（crate 间零 path 依赖——Source 等共享语义为
//! **镜像**而非类型复用）。PDF = 页 + 绘制指令 + 对象图，与块结构模型
//! 异构；产品边界 = 阅读 + 标注（不做内容编辑，主档边界不变）。
//!
//! 职责分层：
//! - **PDF-b 骨架**：页/大纲/标注三层模型 + 命令边界 + 安全策略 +
//!   协议草案 + 语义锁（零引擎依赖——默认构建不变，跨环境可构建）；
//! - **PDF-c 阅读面**（本轮）：[`cache`]（字节预算 LRU 页缓存）、
//!   [`render`]（光栅化抽象 + 引擎无关 mock 面）、[`worker`]（专用
//!   渲染线程 + panic 隔离）、[`document::PdfSession::attach_engine`]
//!   （引擎数据源：文本/搜索/大纲/几何实数据）+ [`engine_pdfium`]
//!   （pdfium 实装，feature `engine-pdfium`——库定位序/线程纪律/零
//!   image 渲染路径，决策见 gates.md PDF-c 节）；
//! - **PDF-d 标注面**（本轮）：[`writer`]（增量写抽象——常开契约层 +
//!   mock + Acrobat 互通纯函数约定）+ [`writer_lopdf`]（lopdf 增量写
//!   实装，feature `engine-lopdf`——四 kind 标准 Annot 对象、零私有
//!   扩展、原字节前缀逐位保持）+ 会话 flush
//!   （[`document::PdfSession::flush_annotations`]：S5 先于一切 + 脏集合
//!   精确跟踪 + 幂等语义；命令面六命令零变化）。
//! - **页模型**（[`page`]）：页号/页数/页几何/`/Rotate` 归一化
//!   （`effective_*` 按 90/270 交换宽高）/页区间（构造即校验）；
//! - **大纲**（[`outline`]）：书签树 + 深度优先文档序迭代（单一排序出口）；
//! - **标注模型**（[`annotation`]）：PDF 标准 Annot 语义四变体
//!   （PDF-d 落盘）+ Human/Ai 溯源 + patch 面（kind 不可变）；
//! - **命令边界**（[`command`]）：六命令 + typed 错误面（七变体——
//!   PDF-c 增 RenderPanic/Engine，v1.2 内部增量）；Human 与 AI 走
//!   同一路径（source 溯源镜像 mdcore 事件源语义）；
//! - **安全策略**（[`security`]）：S2 危险对象族（八族）+ S3 限额三
//!   行为学 + S5 加密写拒 + S6 表单探针（SECURITY_POLICY.md 的类型层
//!   编码，数值权威）；PDF-e 接线 [`security_scan`]（feature
//!   `engine-lopdf`：打开时对象图扫描 → `OpenAssessment`）与
//!   [`worker`] 看门狗换代；
//! - **会话**（[`document`]）：`PdfSession` 单一调度入口（`execute`）——
//!   只有打开后形态；数据源两级（v0 宿主注入 / PDF-c 引擎实装）+ PDF-d
//!   增量写器 attach；
//! - **协议草案**（[`protocol`]）：`mdcore://pdf` 面与 `mdc-pdf-*`
//!   事件族（PDF-c 视图层落地时定稿）；
//! - **引擎集成点**（[`engine`]）：`DocumentEngine` trait（PDF-c 绑
//!   pdfium，non-V8 构建纪律——S1.1 JS 不接 / S1.2 XFA 不启用）。
//!
//! # ⚠️ PDF 字典三形态（后来人必读）
//!
//! PDF 字典里同一个键的值可能是**三种形态**，读法各不相同：
//!
//! | 形态 | 写法 | 读法 |
//! |---|---|---|
//! | 名字 | `/ETSI.CAdES.detached` | `Object::Name` → `as_name()` |
//! | 字符串 | `(text)` 或 `<hex>` | `Object::String` → `as_str()` |
//! | 间接引用 | `12 0 R` | `Object::Reference` → `as_reference()` 再查对象表 |
//!
//! **惰性解包会静默失败**：`as_str()` 对名字返回 `None`、`as_name()` 对
//! 字符串返回 `None`，两者都**不报错、不 panic**，只是拿到空值。于是
//! 「结构看着对、实际读出来是空的」——只能靠测试发现。
//!
//! ## 已踩过的三个坑（都靠测试发现，不是靠读代码）
//!
//! 1. **`/SMask` 必须是间接引用**——写成内联字典时 MuPDF 报
//!    `invalid key in dict`，且 `images on page: 0`（图根本不渲染）。
//!    见 [`stamp`]。
//! 2. **复选框 `/V` 是名字对象**（`/Yes` / `/Off`）——用 `as_str()`
//!    拿到 `None`，勾选态永远读成 `false`（写回正确、读回错误）。
//!    见 [`forms`]。
//! 3. **`/SubFilter` 是名字对象**（`/ETSI.CAdES.detached`）——用
//!    `as_str()` 拿到空串，签名子过滤器永远报空。见 [`signature`]。
//!
//! ## 纪律
//!
//! - 读任何字典字段前，先想清楚它**可能**是什么形态；
//! - 字符串/名字两可的，用 [`forms::dict_text`]（兼容两种形态）；
//! - 需要引用的（如 `/SMask`、`/Font`），**永远**写成间接引用；
//! - 新写的字段读者配上「三形态」测试用例——尤其是**名字形态**那个分支，
//!   它不写测试就一定会漏。

pub mod annotation;
pub mod cache;
pub mod command;
pub mod document;
pub mod engine;
#[cfg(feature = "engine-pdfium")]
pub mod engine_pdfium;
// #68 方向一：pdfium 部署自检（逐层探针 + 绑定走查 + 可操作修复
// 指引——mang-shell --check-pdfium 的核心面；同 feature 门同退化为
// 空目标，跨环境可构建纪律保持）。
#[cfg(feature = "engine-pdfium")]
pub mod deploy_check;
pub mod outline;
pub mod page;
// #240 / Acrobat parity P1：打印渲染原语（DPI 驱动 + 页区间 +
// 注释打印开关——[`crate::render::Rasterizer::rasterize_with_options`]
// 的编排面；零引擎依赖，mock 可测）。
pub mod print;
pub mod protocol;
pub mod render;
pub mod security;
#[cfg(feature = "engine-lopdf")]
pub mod security_scan;
// #232 / R9·P1：表格结构化提取（读面）——位置字符 → 单元格文本 +
// 列序 + 包围盒（行列可重建）。算法层纯函数零引擎依赖（位置层 =
// engine_pdfium::positioned_chars，feature 门内）。
pub mod table;
pub mod worker;
pub mod writer;
#[cfg(feature = "engine-lopdf")]
pub mod writer_lopdf;
// Q3-1 R1（#29 方案 (a)）：文本 → PDF 生成器（Type0/CID 自建面——
// lopdf font_embedding 仅单字节简单字体，CJK 必须手工构建）。
#[cfg(feature = "engine-lopdf")]
pub mod generate;
// S2-1（#103 任务 1）：页面组织——lopdf 页树手术（增删/旋转/重排/
// 合并/拆分 + 书签/链接/表单域联动重映射 + 丢失面显式登记）。
#[cfg(feature = "engine-lopdf")]
pub mod organize;
// S2-3（#103 任务 3）：链接跳转读取——/Link 标注 → 目标页/URI/登记面。
#[cfg(feature = "engine-lopdf")]
pub mod links;
// S2-3（#103 任务 3）：图层（OCG）读取 + 默认可见性持久化开关。
#[cfg(feature = "engine-lopdf")]
pub mod layers;
// S2-4（#103 任务 4）：扫描件 OCR——pdfium 渲染 → 灰度 PGM（零
// image 依赖）→ tesseract 识别。环境门 ocr_available（bid-parse 式）。
#[cfg(feature = "engine-pdfium")]
pub mod ocr;
// S2-2（#103 任务 2）：标注写回扩展——外部标注读取 + PdfAnnotator
// 门面（add/remove/list/save）+ 增量删除面 + 11 kind 模型/写面。
#[cfg(feature = "engine-lopdf")]
pub mod annotate;
// S2-5（issue #116 任务 1）：PDF 表单（AcroForm）读写——域读取/填写/
// 增量写回/扁平化，中文走 CIDFont 嵌入（复用 generate 的 Type0 面）。
#[cfg(feature = "engine-lopdf")]
pub mod forms;
// S2-6（issue #116 任务 3）：盖章——图片叠加进页面内容流（打印必带，
// 不是注释），PNG 自带 alpha 走 SMask，零 image crate 依赖。
#[cfg(feature = "engine-lopdf")]
pub mod stamp;
// S2-7（issue #116 任务 2）：数字签名读取/结构验证——手写 DER 解析
// + CMS SignedData 遍历，零新增依赖；不验证证书链（第一版范围）。
#[cfg(feature = "engine-lopdf")]
pub mod signature;

#[cfg(feature = "engine-lopdf")]
pub use annotate::{read_annotations, PdfAnnotator, ReadReport};
pub use cache::PageCache;
pub use command::{Command, CommandError, Outcome, Source};
pub use document::{AttachEngineError, DocumentMeta, OpenError, PdfDocumentId, PdfSession};
#[cfg(feature = "engine-pdfium")]
pub use ocr::{ocr_available, ocr_page, OcrEnv};
#[cfg(feature = "engine-lopdf")]
pub use organize::{OrganizeReport, PageSize, PdfOrganizer};
pub use print::{expand_selection, render_for_print, PageSelection, PrintSpec};
pub use render::{Rasterizer, RenderOptions, RenderRequest, RenderedPage};
pub use security::{DangerousFamily, DangerousHit, FormField, FormProbe, Limits, OpenAssessment};
pub use worker::{RenderWorker, WorkerReply};
pub use writer::{AnnotationWriter, IncrementalSave, MockWriter};
