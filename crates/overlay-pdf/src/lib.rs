//! mdc-overlay-pdf —— PDF 应用层适配器（#239 Phase 3/4）。
//!
//! # 定位（应用层组合点，循 mdc-mdbridge 先例）
//!
//! `mdc-overlay` 不认识任何文档引擎（三原则①）；本 crate 把
//! overlay 契约与 `mdc-pdf` 引擎组合：
//!
//! ```text
//! mdc-overlay（契约：RenderHint / NativeAnnotationAdapter）
//!      │
//!      ├── view::PdfOverlayViewAdapter     RenderHint → PDF 页坐标
//!      │                                   视图四边形（视图渲染面）
//!      │
//!      └── native::PdfNativeAdapter        OverlayEntry → /Highlight
//!                                          + QuadPoints（原生投影面：
//!                                          plan 报告 → 确认 → execute
//!                                          指纹核对 → lopdf 增量写）
//! ```
//!
//! # 与 issue #239 规格的一处显式偏差（登记）
//!
//! issue §六 trait 草案 `plan(&self, …, marks: &[OverlayMark])`——但
//! `ProjectionReport` 的三态（exported/unsupported/degraded）全部以
//! `AnnotationId` 关联，而 `OverlayMark` 无 id 字段（id 在存储单元
//! [`mdc_overlay::OverlayEntry`] 上）。故本 crate 的 plan 输入面 =
//! `&[OverlayEntry]`（id + 标注 + 归属齐全）——「报告含该 ID」的
//! 验收契约才可能成立；这是对规格笔误的忠实修正，不是能力缩水。
//!
//! # feature 面（默认零引擎——跨环境可构建）
//!
//! - 默认：`view` 适配器 + `native::plan_with_geometry`（几何注入）。
//! - `engine-lopdf`：`execute`（增量写 + 读回全链）。
//! - `engine-pdfium`：`NativeAnnotationAdapter::plan`（pdfium
//!   `positioned_chars` 字符几何——库经 mdc-pdf 定位序绑定）。

pub mod native;
pub mod view;

use mdc_overlay::geometry::{CharBox, PageBox};

/// 页字符布局（plan/视图换算的几何注入单元——页号 1 起，与
/// `mdc_overlay::PrimaryLocator::PdfSpan` 同域）。
///
/// 几何源依赖倒置：生产路径 = pdfium `positioned_chars`（feature
/// `engine-pdfium` 的 plan）；测试/宿主已有几何时直接注入。
#[derive(Debug, Clone, PartialEq)]
pub struct PageChars {
    pub page: u32,
    pub chars: Vec<CharBox>,
    pub page_box: PageBox,
}

pub use native::{PdfNativeAdapter, PROJECTION_AUTHOR_DEFAULT};
pub use view::{PdfOverlayViewAdapter, ViewError};

/// Emphasis → /Highlight 的默认颜色（PDF 惯例黄）。
///
/// `OverlayStyle` 是语义样式（不带 RGB——视觉细节归 UI 主题）；原生
/// 投影面必须落具体颜色，取 Acrobat 高亮惯例黄（主题化后续轮）。
pub const EMPHASIS_COLOR: mdc_pdf::annotation::Rgb = mdc_pdf::annotation::Rgb {
    r: 255,
    g: 235,
    b: 59,
};
