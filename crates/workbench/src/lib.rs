//! mdc-workbench —— 工作台编排层（S1-R3，修订实施计划 §四·2026-10-21）。
//!
//! # 产品语义
//!
//! headless 编排器：把 [`mdc_evidence`]（契约/三态/sidecar/求值器）
//! 与 [`mdc_office_io`]（XLSX 读 / DOCX 读写）组合成**合成样例八步
//! 闭环**：
//!
//! ```text
//! ①导入 XLSX 来源 → ②登记 DOCX 正文 → ③关联（AI 提议/程序验证）
//! → ④计算（程序提取数值并求值）→ ⑤报告（三态分列）
//! → ⑥改写失效检测（来源侧 + 正文侧）→ ⑦审核清单（人工通过/拒绝）
//! → ⑧干净导出（正文零痕迹）
//! ```
//!
//! # 三条纪律（结构性保证，非约定）
//!
//! - **正文零写入**：本 crate 对 DOCX 正文只读；证据只进 sidecar；
//!   "干净导出" = 字节拷贝 + 双验证（字节同一 + 无工作台标记）。
//! - **AI 提议 / 程序验证**（§四.3）：提议方（AI 或人）给出**映射与
//!   公式**（哪个段落 ↔ 哪个格、表达式、精度规则），**数值一律由
//!   程序从真实文件提取**并经指纹验证；唯一例外 = 无来源常量（正当
//!   性归人工审核面，提议时显式声明）。
//! - **无任意路径/SQL/Shell**：路径为显式入参并登记于会话注册表；
//!   本 crate 无任何进程/网络/查询面（S2 命令层在此之上加允许列表）。
//!
//! # 指纹口径（MVP 登记）
//!
//! - XLSX 语义指纹：FNV-1a 64 over 全部非空格的
//!   `sheet|A1坐标|值表示` 序（表序→行→列确定性序；样式/列宽等
//!   非语义面不参与——语义变更才失效，噪声失效不制造）。
//! - DOCX 正文指纹：FNV-1a 64 over 段落规范化文本（trim）按 `\n`
//!   连接；段落定位器指纹 = 单段 trim 文本。
//! - 时间：`at_ms` 全由调用方注入（确定性；无时钟依赖）。

pub mod command;
pub mod ingest;
pub mod pipeline;
pub mod report;
pub mod security;

pub use command::{expect_exactly, WorkbenchCommand};
pub use ingest::{
    docx_body_ref, ingest_docx, ingest_xlsx, xlsx_source_ref, DocxBody, ParagraphInfo, SourceCell,
    XlsxSource,
};
pub use pipeline::{
    BindingProposal, CalcInputSpec, CalcProposal, InputSource, Workbench, WorkbenchError,
};
pub use report::{pending_reviews, render_report, stale_evidence};
pub use security::{
    check_counts, check_sidecar_size, publish, PathPolicy, PolicyError, ReviewPolicy,
    MAX_CALCULATIONS, MAX_EVIDENCE, MAX_SIDECAR_BYTES,
};

/// 工作台标记（干净导出验证用——正文出现任一即脏）。
pub const WORKBENCH_MARKERS: &[&str] = &[
    "mdc-evidence",
    "mdc-workbench",
    "evidence_id",
    "claim_locator",
    "source_locator",
    "calc_ref",
    "generation_batch",
    "sidecar",
];
