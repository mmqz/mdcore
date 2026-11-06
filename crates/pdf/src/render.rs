//! 渲染抽象——光栅化请求/产物 + [`Rasterizer`] trait（PDF-c 阅读面，
//! 常开模块）。
//!
//! 分层纪律：本模块只冻结**抽象面**（mock 可测——worker/cache 的语义锁
//! 不依赖引擎）；pdfium 实装在 [`crate::engine_pdfium`]（feature
//! `engine-pdfium`）。S4.3：FFI 边界只在引擎模块内，业务层只见本抽象。
//!
//! 产物格式注记（BGRA）：`RenderedPage::bgra` 为逐像素 4 字节
//! B/G/R/A，行序自上而下，行内 stride = `width_px * 4`（4 字节/像素
//! 恒对齐，无行尾填充）。**非预乘**——pdfium `FPDFBitmap_BGRA` 头文件
//! 注记 "Pixel components are independent of alpha"（分量与 alpha
//! 独立）；消费方按直通 alpha 处理。渲染路径刻意**不引入 image crate**
//! （走 `PdfBitmap::from_bytes` 调用方缓冲 + `as_raw_bytes` 低层路线，
//! 决策记录见 gates.md PDF-c 节）。

use serde::{Deserialize, Serialize};

use crate::command::CommandError;
use crate::page::PageIndex;

/// 光栅化请求：目标页 + 目标像素宽（高等比自适应——引擎按页几何
/// 计算；S3-L2 钳制在引擎侧执行，生效尺寸以 [`RenderedPage`] 报告为准）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RenderRequest {
    pub page: PageIndex,
    pub width_px: u32,
}

/// 光栅化产物（BGRA 位图；格式语义见模块文档）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedPage {
    pub width_px: u32,
    pub height_px: u32,
    /// 像素缓冲：长度恒为 `width_px * height_px * 4`（语义锁背书）。
    pub bgra: Vec<u8>,
}

/// 光栅化器抽象（object-safe——S4 渲染工人以对象引用形态持有引擎）。
///
/// `&mut self`：渲染是引擎态操作（pdfium 文档句柄独占使用）；同一
/// 引擎实例串行渲染（工人线程内单线程调用——见 worker.rs 线程纪律）。
pub trait Rasterizer {
    /// 渲染单页。失败走 typed 错误（引擎失败 → `CommandError::Engine`；
    /// 页越界 → `InvalidPage`）；作业 panic 由工人层捕获（S4.2——引擎
    /// 实现不承诺不 panic，隔离责任在 [`crate::worker`]）。
    fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError>;

    /// #240 打印面增补：注释可见位可调的渲染入口。
    ///
    /// 默认实现**忽略 `options`** 委托 [`Self::rasterize`]（注释恒
    /// 可见——阅读面 S2-3 行为契约不变，mock/轻量实装零成本兼容）；
    /// 真实引擎实装覆写本方法以真接线 pdfium `FPDF_ANNOT` 位
    /// （[`crate::engine_pdfium`]）。消费面：[`crate::print`]（打印
    /// 渲染原语——阅读面预览/工人/OCR 继续走 `rasterize`，零波及）。
    fn rasterize_with_options(
        &mut self,
        request: RenderRequest,
        options: RenderOptions,
    ) -> Result<RenderedPage, CommandError> {
        let _ = options;
        self.rasterize(request)
    }
}

/// 渲染选项（#240 打印面增补）：页内标注（高亮/便签/Ink/形状…）
/// 是否随页面光栅化可见。
///
/// 既有阅读面不经过本类型（`rasterize` 注释恒可见——S2-3 显式钉定
/// 的行为契约）；打印面按用户选择开关（Acrobat「注释打印开关」对齐项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderOptions {
    /// pdfium `FPDF_ANNOT` 位：true = 标注随页渲染可见（阅读面缺省），
    /// false = 只渲染页面内容流（打印「不带注释」档）。
    pub render_annotations: bool,
}

impl RenderOptions {
    /// 阅读面缺省（注释可见——与既有 `rasterize` 行为一致）。
    pub const READING: RenderOptions = RenderOptions {
        render_annotations: true,
    };
}

/// 装箱转发（`Box<dyn Rasterizer + Send>` 满足工人泛型约束——宿主以
/// 对象形态装配渲染管线的通道）。`rasterize_with_options` 同步转发
/// （#240——打印面经对象形态装配时选项位不丢）。
impl<R: Rasterizer + ?Sized> Rasterizer for Box<R> {
    fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
        (**self).rasterize(request)
    }

    fn rasterize_with_options(
        &mut self,
        request: RenderRequest,
        options: RenderOptions,
    ) -> Result<RenderedPage, CommandError> {
        (**self).rasterize_with_options(request, options)
    }
}
