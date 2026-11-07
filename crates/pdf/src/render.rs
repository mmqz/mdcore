//! 渲染抽象——光栅化请求/产物 + [`Rasterizer`] trait + [`RenderKey`]
//! 渲染缓存键（PDF-c 阅读面，常开模块）。
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

use crate::cache::PageCache;
use crate::command::CommandError;
use crate::page::PageIndex;

/// 光栅化请求：目标页 + 目标像素宽（高等比自适应——引擎按页几何
/// 计算；S3-L2 钳制在引擎侧执行，生效尺寸以 [`RenderedPage`] 报告为准）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RenderRequest {
    pub page: PageIndex,
    pub width_px: u32,
}

/// 渲染缓存键（#305 方案 A——键控失效）：光栅产物身份的完整因子集。
///
/// **语义合同**：`RenderKey` 相等 ⟺ 光栅输出可复用。四因子缺一不可：
/// - `page`：页不同 → 内容不同；
/// - `width_px` / `height_px`：目标尺寸不同 → 位图不同（高度独立于宽
///   入键——几何/裁剪域差异下同宽可以异高，报告尺寸
///   [`RenderedPage::height_px`] 是引擎侧事实，不是宽的函数）；
/// - `annotation_rev`：**标注修订号**（[`crate::document::PdfSession::
///   annotation_rev`]——flush 成功且实际追加字节时 +1）。渲染面
///   `render_annotations(true)` 位图含标注（engine_pdfium.rs 显式钉定），
///   标注落盘后旧位图即陈旧——rev 入键使旧条目自然 miss，新渲染以新
///   rev 入缓存（同 rev 重复取 = 命中，翻页性能不因键控受损）。
///
/// **失效配套**（方案 B——预算回收，与键控互补）：键控让旧 rev 条目
/// miss，但它们仍占预算直到 LRU 驱逐；`invalidate_stale_revisions`
/// 即刻摘除回收。见 [`PageCache<RenderKey, V>`] 实现块。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RenderKey {
    /// 0 基页号（与 [`RenderRequest::page`] 的 `PageIndex.0` 同值域）。
    pub page: u32,
    /// 目标像素宽（= [`RenderRequest::width_px`]）。
    pub width_px: u32,
    /// 目标像素高（= 产物 [`RenderedPage::height_px`]——引擎侧报告值）。
    pub height_px: u32,
    /// 标注修订号（会话 flush 推进——见类型文档语义合同）。
    pub annotation_rev: u64,
}

impl RenderKey {
    /// 全因子构造（请求 + 引擎报告高度 + 会话 rev——渲染完成时三源齐）。
    pub fn new(page: u32, width_px: u32, height_px: u32, annotation_rev: u64) -> Self {
        Self {
            page,
            width_px,
            height_px,
            annotation_rev,
        }
    }

    /// 由请求 + 产物 + 会话 rev 构造（渲染回执落账的最短路径）。
    pub fn from_outcome(
        request: &RenderRequest,
        rendered: &RenderedPage,
        annotation_rev: u64,
    ) -> Self {
        Self {
            page: request.page.0,
            width_px: rendered.width_px,
            height_px: rendered.height_px,
            annotation_rev,
        }
    }
}

/// PDF 感知失效粒度（#305 方案 B——`PageCache<RenderKey, V>` 专用面）。
///
/// 三个粒度按「变更影响域」递增；返回值 = 被摘除键集（LRU→MRU 序，
/// 与 [`PageCache::put`] 驱逐键集同款载荷——调用方据此作废关联资源
/// 如 GPU 纹理/宿主位图句柄）：
/// - **单页**（[`invalidate_page`](Self::invalidate_page)）：页几何/
///   单页内容变更——其它页的命中零扰动；
/// - **陈旧修订**（[`invalidate_stale_revisions`](Self::invalidate_stale_revisions)）：
///   flush 后回收旧 rev 条目（当前 rev 条目全部在位——「加标注 → 重
///   渲染」的恰好一轮，不多不少）；
/// - **全量**（[`invalidate_all`](Self::invalidate_all)）：标注渲染开关
///   翻转/会话换代等影响全部条目的变更（`clear` 的键回报版——
///   [`PageCache::clear`] 是无载荷全量复位，两者按「要不要键集」分工）。
impl<V> PageCache<RenderKey, V> {
    /// 失效指定页的全部条目（全部宽度/全部修订——页级变更影响域）。
    pub fn invalidate_page(&mut self, page: u32) -> Vec<RenderKey> {
        self.invalidate_matching(|k| k.page == page)
    }

    /// 失效全部**陈旧修订**条目（`annotation_rev < current`——flush 后
    /// 旧位图即刻回收；当前修订条目零扰动，命中率保持）。
    pub fn invalidate_stale_revisions(&mut self, current_rev: u64) -> Vec<RenderKey> {
        self.invalidate_matching(|k| k.annotation_rev < current_rev)
    }

    /// 失效全部条目（影响域 = 全量时用——见实现块文档与 `clear` 的分工）。
    pub fn invalidate_all(&mut self) -> Vec<RenderKey> {
        self.invalidate_matching(|_| true)
    }
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
