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
}

/// 装箱转发（`Box<dyn Rasterizer + Send>` 满足工人泛型约束——宿主以
/// 对象形态装配渲染管线的通道）。
impl<R: Rasterizer + ?Sized> Rasterizer for Box<R> {
    fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
        (**self).rasterize(request)
    }
}
