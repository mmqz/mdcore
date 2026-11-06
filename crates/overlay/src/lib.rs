//! mdc-overlay —— 跨文档索引 / 标注 / 投影层（#239 Phase 1-4）。
//!
//! # 产品语义（一段话）
//!
//! 引擎是多格式的（PDF / DOCX / XLSX / Markdown / Canvas），用户要求
//! **所有格式都能双向索引 + 标注**。本层把标注收敛为格式无关的语义
//! 模型（[`OverlayMark`]），语义定位（[`SemanticAnchor`]——说的是哪段
//! 内容）与几何定位（[`GeometricSpan`]——画在哪）**显式分离**，统一
//! 存放在独立 sidecar（单一真源，默认不写原生文件）；视图经
//! [`RenderHint`]（渲染意图，不是渲染指令）消费；按需经
//! [`NativeAnnotationAdapter`] 契约投影为目标格式的原生标注（显式
//! 动作，产物是新文件）。
//!
//! # 三条不可违反的原则（结构性保证，非约定）
//!
//! 1. **零文档引擎依赖**：本 crate 不依赖 mdc-pdf / mdc-office-io /
//!    mdc-canvas / mang-shell（`tests/no_engine_deps.rs` 以 `cargo tree`
//!    运行时断言钉死）。允许依赖 `mdc-evidence`（issue #239 第七节
//!    用户裁定：契约层非文档引擎——复用 `DocumentRef` / 指纹 /
//!    `LinkageStatus`，**不复制一套平行实现**）。
//! 2. **默认不写原生文件**：本 crate 无任何"向源文件写字节"的 API
//!    （结构性不可能）；投影（`execute`）是 adapter 的显式动作且
//!    产物是新文件字节，落盘由宿主决定。
//! 3. **不能表达的标注不静默丢弃**：投影在 plan 阶段就产出
//!    `unsupported` / `degraded` 报告（[`crate::contract::ProjectionReport`]），
//!    用户确认后才执行。
//!
//! # 模块地图
//!
//! - [`anchor`]：语义锚点 [`SemanticAnchor`]（主定位 + 降级链 + 内容
//!   指纹）与几何定位 [`GeometricSpan`]（坐标空间显式声明的四边形/
//!   包围盒）——**两者不能混为一谈**（架构关键线）。
//! - [`mark`]：[`OverlayMark`] 四变体语义模型（Emphasis / Comment /
//!   Freehand / Region）+ [`AnnotationId`] + 存储单元 [`OverlayEntry`]
//!   （归属文档显式化）。
//! - [`render`]：[`RenderHint`]——格式无关的**渲染意图**（语义样式 +
//!   渲染语义 + 关联状态），颜色/透明度等视觉细节交给 UI 主题。
//! - [`index`]：[`OverlayIndex`] 双向索引（标注 ↔ 定位器）。
//! - [`store`]：[`OverlaySidecar`] JSON 存储 v1（原子写 + 模式版本
//!   校验——复用 mdc-evidence 的 SidecarStore 原子写纪律）。
//! - [`contract`]：[`NativeAnnotationAdapter`] 投影契约（plan 报告 →
//!   用户确认 → execute 版本核对）+ [`ProjectionPlan`] /
//!   [`ProjectionReport`]。
//! - [`geometry`]：[`CharBox`] 格式无关字符几何 + [`quads_for_range`]
//!   （字符区间 → 行带四边形——纯函数，几何源由调用方注入）。
//!
//! # 本轮边界（#239 Phase 1-4 锁定，不扩）
//!
//! 已交付：最小数据模型 + Adapter 契约 / 双向索引 / RenderHint 输出 /
//! PDF Emphasis → /Highlight 原生投影链路（投影实装在姊妹 crate
//! `mdc-overlay-pdf`——应用层组合点，循 mdc-mdbridge 先例）。
//!
//! 明确不做：DOCX / XLSX / Canvas 适配器（PDF 跑通后再判断怎么接）、
//! Phase 5 能力矩阵完整实现（仅接口）、L3 原位写回、Link 跨文档关联、
//! 全格式通用标注系统。

pub mod anchor;
pub mod contract;
pub mod geometry;
pub mod index;
pub mod mark;
pub mod render;
pub mod store;

pub use anchor::{
    AnchorError, Bounds, CoordSpace, FallbackLocator, GeometricSpan, PdfOrigin, PrimaryLocator,
    SemanticAnchor, SpanError,
};
pub use contract::{
    validate_plan, AdapterCapabilities, AdapterError, NativeAnnotationAdapter, ProjectionOperation,
    ProjectionPlan, ProjectionReport, ProjectionResult, ProjectionTarget,
};
pub use geometry::{quads_for_range, CharBox, GeometryError, PageBox, PointF, Quad};
pub use index::{IndexError, IndexKey, OverlayIndex};
pub use mark::{AnnotationId, EntryError, MarkError, OverlayEntry, OverlayMark};
pub use render::{OverlayStyle, RenderHint, RenderSemantic};
pub use store::{OverlaySidecar, OverlayStore, OverlayStoreError, OVERLAY_SCHEMA_VERSION};

/// 标注目标文档的格式域（issue #239 §三：格式域知识，非引擎依赖）。
///
/// 与 `mdc_evidence::EngineKind` 语义同构（分派面），但变体集按
/// overlay 的 `PrimaryLocator` 形状定义（如 PDF 是 span 级而非页级）；
/// 互转见 [`Format::engine_kind`] / `Format::from_engine_kind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// PDF（`PrimaryLocator::PdfSpan`——页 + 行 + 字符区间）。
    Pdf,
    /// DOCX（`PrimaryLocator::DocxSpan`——段落序 + 字符区间）。
    Docx,
    /// XLSX（`PrimaryLocator::XlsxCell`——表 + 行 + 列）。
    Xlsx,
    /// Markdown（`PrimaryLocator::MdBlock`——块 ID 寻址）。
    Md,
    /// Canvas（`PrimaryLocator::CanvasObj`——画布对象 ID）。
    Canvas,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Xlsx => "xlsx",
            Format::Md => "md",
            Format::Canvas => "canvas",
        }
    }

    /// 映射到 mdc-evidence 引擎域（DocumentRef.engine 的类型）。
    pub fn engine_kind(self) -> mdc_evidence::EngineKind {
        match self {
            Format::Pdf => mdc_evidence::EngineKind::Pdf,
            Format::Docx => mdc_evidence::EngineKind::Docx,
            Format::Xlsx => mdc_evidence::EngineKind::Xlsx,
            Format::Md => mdc_evidence::EngineKind::Markdown,
            Format::Canvas => mdc_evidence::EngineKind::Canvas,
        }
    }

    /// 引擎域 → 格式域（未知面无——两枚举变体一一对应）。
    pub fn from_engine_kind(k: mdc_evidence::EngineKind) -> Self {
        match k {
            mdc_evidence::EngineKind::Pdf => Format::Pdf,
            mdc_evidence::EngineKind::Docx => Format::Docx,
            mdc_evidence::EngineKind::Xlsx => Format::Xlsx,
            mdc_evidence::EngineKind::Markdown => Format::Md,
            mdc_evidence::EngineKind::Canvas => Format::Canvas,
        }
    }
}

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_evidence::EngineKind;

    #[test]
    fn format_engine_kind_roundtrip() {
        for f in [
            Format::Pdf,
            Format::Docx,
            Format::Xlsx,
            Format::Md,
            Format::Canvas,
        ] {
            assert_eq!(Format::from_engine_kind(f.engine_kind()), f);
        }
        assert_eq!(Format::Pdf.engine_kind(), EngineKind::Pdf);
        assert_eq!(Format::Pdf.as_str(), "pdf");
    }

    #[test]
    fn format_serde_lowercase() {
        assert_eq!(
            serde_json::to_string(&Format::Pdf).unwrap(),
            "\"pdf\"".to_string()
        );
    }
}
