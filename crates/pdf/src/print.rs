//! #240 打印渲染原语——[`Rasterizer`] 输出的打印面编排。
//!
//! 分层定位（issue #240 裁定）：
//!
//! ```text
//! 渲染（Rasterizer）          → 引擎（已有，本模块复用不改）
//! 打印（送系统打印流）         → 壳（mang-shell/src/print.rs，平台相关）
//! ```
//!
//! 本模块补齐引擎侧缺口：**目标 DPI 驱动**（72pt 基准 × DPI 缩放——
//! 屏显 `width_px` 驱动换成印刷口径）、**页区间选择**（全部 / 闭区间）、
//! **注释打印开关**（[`RenderOptions::render_annotations`] 位——Acrobat
//! 「文档和标注 / 仅文档」对齐项）。产物仍是 [`RenderedPage`]（BGRA
//! 位图合同不变）——打印预览/送打印机/未来的壳内直印（方案 A 迭代位）
//! 都从这同一原语出发，保证「打印输出 = 屏幕渲染」的同源口径。
//!
//! ## 钳制哲学（与 S3-L2 同源）
//!
//! DPI 是尺寸旋钮（同 `width_px`），**钳制收敛而非拒绝**：有效域
//! [36, 2400]（36 = 草稿档，2400 = 高精度制版档）；生效尺寸以
//! [`RenderedPage`] 报告为准（引擎侧 S3-L2 单边/总像素钳制仍在
//! `rasterize` 内执行——本模块只做 DPI 域钳制，双层钳制互不越权）。
//!
//! ## 消费面纪律
//!
//! - 阅读面（预览/工人/OCR）**不经本模块**——既有 `rasterize` 路径
//!   零波及；
//! - 打印面一律 `rasterize_with_options`（注释位显式）；
//! - `PageSelection` 用 0 基 [`PageIndex`]（与全仓一致）；宿主把
//!   用户 1 基输入（打印对话框「页码 1-3」）转成 0 基再进来。

use crate::command::CommandError;
use crate::engine::DocumentEngine;
use crate::page::PageIndex;
use crate::render::{Rasterizer, RenderOptions, RenderRequest, RenderedPage};

/// 打印渲染规格（#240）。
///
/// `dpi` 有效域 [36, 2400]，构造时钳制（[`PrintSpec::new`]）——直接
/// 结构体字面量构造（测试面）由 [`Self::clamped`] 语义兜底：渲染入口
/// [`render_for_print`] 内部恒走钳制换算，不信任裸字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrintSpec {
    /// 目标 DPI（72 = 1:1 pt→px，屏显基准；150 = 常规打印档；
    /// 300 = 高质量打印档）。
    pub dpi: u32,
    /// 注释打印开关（true = 「文档和标注」，false = 「仅文档」）。
    pub annotations: bool,
}

/// DPI 有效域下界（草稿档——低于此值文字不可辨认，视为误输入）。
pub const DPI_MIN: u32 = 36;
/// DPI 有效域上界（高精度制版档——高于此值超出 pdfium S3-L2 单边
/// 8192px 钳制的合理使用面，且放大只是插值假精度）。
pub const DPI_MAX: u32 = 2400;

impl PrintSpec {
    /// 构造（DPI 钳制到有效域——收敛不拒绝）。
    pub fn new(dpi: u32, annotations: bool) -> Self {
        Self {
            dpi: dpi.clamp(DPI_MIN, DPI_MAX),
            annotations,
        }
    }

    /// 高质量打印档缺省（300 DPI + 注释可见——Acrobat 打印缺省口径）。
    pub const HIGH_QUALITY: PrintSpec = PrintSpec {
        dpi: 300,
        annotations: true,
    };
}

/// 打印页区间（0 基，闭区间——与 [`crate::page::PageSpan`] 同语义；
/// 独立类型而非复用：`PageSpan` 是抽取面的既有冻结形状，打印面
/// 语义「All」在 `PageSpan` 无对应形态，不硬造）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSelection {
    /// 全部页。
    All,
    /// 闭区间 `[from, to]`（0 基；`from > to` 或 `to` 越界 →
    /// [`CommandError::InvalidPage`]——from 为肇事页号）。
    Range { from: PageIndex, to: PageIndex },
}

/// DPI → 目标像素宽换算：`page_width_pt × dpi / 72`（72pt 基准）。
///
/// 钳制序：DPI 先钳到 [`DPI_MIN`, `DPI_MAX`]，再四舍五入取整、
/// 下限 1px（空页宽防御）。**引擎侧 S3-L2 单边/总像素钳制仍在
/// `rasterize` 内生效**（本函数只负责印刷口径的换算，不越权）。
pub fn dpi_width_px(dpi: u32, page_width_pt: f32) -> u32 {
    let dpi = dpi.clamp(DPI_MIN, DPI_MAX);
    ((page_width_pt.max(0.0) * (dpi as f32) / 72.0).round() as u32).max(1)
}

/// 页区间展开（0 基升序页号序列）。
///
/// 错误面（typed，不静默）：
/// - `Range` 翻转（`from > to`）→ `InvalidPage(from)`（起点页号
///   非法——不存在负宽度区间）；
/// - `to` 越界（`to >= page_count`）→ `InvalidPage(to)`；
/// - `All` 在 0 页文档 → 空序列（引擎打开面已拒空文档
///   [`crate::document::OpenError::EmptyDocument`]，此处防御性返回
///   空——「打印空文档 = 零作业」而非错误）。
pub fn expand_selection(
    selection: &PageSelection,
    page_count: u32,
) -> Result<Vec<PageIndex>, CommandError> {
    match selection {
        PageSelection::All => Ok((0..page_count).map(PageIndex).collect()),
        PageSelection::Range { from, to } => {
            if from > to {
                return Err(CommandError::InvalidPage(*from));
            }
            if to.0 >= page_count {
                return Err(CommandError::InvalidPage(*to));
            }
            Ok((from.0..=to.0).map(PageIndex).collect())
        }
    }
}

/// 打印渲染入口：按规格渲染所选页区间（页号升序，产物序 = 页序）。
///
/// 逐页流程：几何（`DocumentEngine::page_geometry`——含 `/Rotate`
/// 归一，取 [`PageGeometry::effective_width`]）→ DPI 换算目标宽 →
/// [`Rasterizer::rasterize_with_options`]（注释位显式）。
///
/// 泛型双约束（`DocumentEngine + Rasterizer`）：几何与渲染同源单次
/// 装配（同引擎实例串行——与工人线程纪律一致）；mock 实装两 trait
/// 即可测（本文件单测路径），pdfium 引擎天然满足。
pub fn render_for_print<E>(
    engine: &mut E,
    spec: &PrintSpec,
    selection: &PageSelection,
) -> Result<Vec<RenderedPage>, CommandError>
where
    E: Rasterizer + DocumentEngine + ?Sized,
{
    let count = engine.page_count();
    let pages = expand_selection(selection, count)?;
    let options = RenderOptions {
        render_annotations: spec.annotations,
    };
    let mut out = Vec::with_capacity(pages.len());
    for page in pages {
        let geometry = engine.page_geometry(page)?;
        let width_px = dpi_width_px(spec.dpi, geometry.effective_width());
        out.push(engine.rasterize_with_options(RenderRequest { page, width_px }, options)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 单测（默认面——零引擎依赖，mock 双 trait 实装；CI 裸环境全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::OutlineTree;
    use crate::page::PageGeometry;

    /// 记账 mock：几何固定 A4（595×842pt 无旋转），逐请求记录
    /// （请求 + 选项），产物宽 = 请求宽、高 = 宽 × 842/595（等比）。
    struct MockEngine {
        count: u32,
        requests: Vec<(RenderRequest, RenderOptions)>,
    }

    impl MockEngine {
        fn new(count: u32) -> Self {
            Self {
                count,
                requests: Vec::new(),
            }
        }
    }

    impl DocumentEngine for MockEngine {
        fn page_count(&self) -> u32 {
            self.count
        }

        fn page_geometry(&self, _idx: PageIndex) -> Result<PageGeometry, CommandError> {
            Ok(PageGeometry {
                width_pt: 595.0,
                height_pt: 842.0,
                rotation: crate::page::Rotation::Deg0,
                ..Default::default()
            })
        }

        fn outline(&self) -> Result<OutlineTree, CommandError> {
            Ok(OutlineTree::default())
        }

        fn extract_text(
            &self,
            _span: crate::page::PageSpan,
        ) -> Result<Vec<(PageIndex, String)>, CommandError> {
            Ok(Vec::new())
        }
    }

    impl Rasterizer for MockEngine {
        fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
            self.requests.push((request, RenderOptions::READING));
            let h = ((f64::from(request.width_px) * 842.0 / 595.0).round() as u32).max(1);
            Ok(RenderedPage {
                width_px: request.width_px,
                height_px: h,
                bgra: vec![0u8; request.width_px as usize * h as usize * 4],
            })
        }

        fn rasterize_with_options(
            &mut self,
            request: RenderRequest,
            options: RenderOptions,
        ) -> Result<RenderedPage, CommandError> {
            self.requests.push((request, options));
            let h = ((f64::from(request.width_px) * 842.0 / 595.0).round() as u32).max(1);
            Ok(RenderedPage {
                width_px: request.width_px,
                height_px: h,
                bgra: vec![0u8; request.width_px as usize * h as usize * 4],
            })
        }
    }

    #[test]
    fn dpi_width_px_letter_150dpi() {
        // Letter 612pt @ 150dpi → 1275px（#240 验收口径锚）。
        assert_eq!(dpi_width_px(150, 612.0), 1275);
        // 72dpi = 1:1 pt→px（屏显基准）。
        assert_eq!(dpi_width_px(72, 595.0), 595);
        // A4 300dpi 高质量档。
        assert_eq!(dpi_width_px(300, 595.0), 2479);
    }

    #[test]
    fn dpi_width_px_clamps_dpi_domain() {
        // DPI 域钳制（收敛不拒绝）：0 → 36（A4 → 298px）。
        assert_eq!(dpi_width_px(0, 595.0), 298);
        // 超上界 → 2400（A4 → 19833px——引擎 S3-L2 单边钳制仍兜底）。
        assert_eq!(dpi_width_px(100_000, 595.0), 19_833);
        // 负宽防御（几何异常页）：下限 1px。
        assert_eq!(dpi_width_px(72, -3.0), 1);
    }

    #[test]
    fn print_spec_new_clamps() {
        assert_eq!(PrintSpec::new(10, true).dpi, DPI_MIN);
        assert_eq!(PrintSpec::new(9_999, false).dpi, DPI_MAX);
        assert_eq!(PrintSpec::new(300, true).dpi, 300);
    }

    #[test]
    fn expand_all_covers_every_page() {
        let pages = expand_selection(&PageSelection::All, 3).unwrap();
        assert_eq!(pages, vec![PageIndex(0), PageIndex(1), PageIndex(2)]);
        // 0 页文档（防御路径——引擎打开面已拒）：空序列非错误。
        assert!(expand_selection(&PageSelection::All, 0).unwrap().is_empty());
    }

    #[test]
    fn expand_range_is_closed_interval() {
        let pages = expand_selection(
            &PageSelection::Range {
                from: PageIndex(1),
                to: PageIndex(3),
            },
            10,
        )
        .unwrap();
        assert_eq!(
            pages,
            vec![PageIndex(1), PageIndex(2), PageIndex(3)],
            "闭区间 [1,3] = 3 页"
        );
        // 单页区间。
        let one = expand_selection(
            &PageSelection::Range {
                from: PageIndex(2),
                to: PageIndex(2),
            },
            5,
        )
        .unwrap();
        assert_eq!(one, vec![PageIndex(2)]);
    }

    #[test]
    fn expand_range_rejects_inverted_and_overflow() {
        // 翻转区间 → InvalidPage(from)。
        assert_eq!(
            expand_selection(
                &PageSelection::Range {
                    from: PageIndex(3),
                    to: PageIndex(1),
                },
                10,
            ),
            Err(CommandError::InvalidPage(PageIndex(3)))
        );
        // 尾页越界 → InvalidPage(to)。
        assert_eq!(
            expand_selection(
                &PageSelection::Range {
                    from: PageIndex(0),
                    to: PageIndex(3),
                },
                3,
            ),
            Err(CommandError::InvalidPage(PageIndex(3)))
        );
    }

    #[test]
    fn render_for_print_dpi_range_and_options() {
        let mut engine = MockEngine::new(4);
        // 144dpi（2× 屏显）+ 不带注释 + 页区间 [1,2]。
        let spec = PrintSpec::new(144, false);
        let pages = render_for_print(
            &mut engine,
            &spec,
            &PageSelection::Range {
                from: PageIndex(1),
                to: PageIndex(2),
            },
        )
        .unwrap();

        assert_eq!(pages.len(), 2, "闭区间 [1,2] → 2 页");
        // A4 595pt @ 144dpi → 1190px（595×2）。
        assert_eq!(pages[0].width_px, 1190);
        assert_eq!(pages[1].width_px, 1190);
        // 等比高（842×2 = 1684）。
        assert_eq!(pages[0].height_px, 1684);
        // 请求页号 = 区间展开序（升序）。
        let requested: Vec<u32> = engine.requests.iter().map(|(r, _)| r.page.0).collect();
        assert_eq!(requested, vec![1, 2]);
        // 注释位透传（false——打印「仅文档」档）。
        assert!(engine.requests.iter().all(|(_, o)| !o.render_annotations));
        // 位图缓冲合同。
        assert_eq!(pages[0].bgra.len(), 1190 * 1684 * 4);
    }

    #[test]
    fn render_for_print_all_pages_with_annotations() {
        let mut engine = MockEngine::new(3);
        let spec = PrintSpec::new(72, true);
        let pages = render_for_print(&mut engine, &spec, &PageSelection::All).unwrap();
        assert_eq!(pages.len(), 3);
        // 72dpi = 1:1 → A4 595px 宽。
        assert!(pages.iter().all(|p| p.width_px == 595));
        assert!(engine.requests.iter().all(|(_, o)| o.render_annotations));
    }

    #[test]
    fn render_for_print_propagates_invalid_page() {
        let mut engine = MockEngine::new(2);
        let spec = PrintSpec::HIGH_QUALITY;
        assert_eq!(
            render_for_print(
                &mut engine,
                &spec,
                &PageSelection::Range {
                    from: PageIndex(0),
                    to: PageIndex(5),
                },
            ),
            Err(CommandError::InvalidPage(PageIndex(5)))
        );
        // 首页失败前零请求落地（区间校验先行——不渲染半途而废的作业）。
        assert!(engine.requests.is_empty());
    }

    /// 阅读面兼容锚：只实装 `rasterize` 的既有 Rasterizer（不覆写
    /// `rasterize_with_options`）经打印入口仍可用（trait 缺省 = 注释
    /// 恒可见 + 尺寸换算照常）——mock/轻量实装零成本兼容的合同钉定。
    struct ReadingOnlyRasterizer;

    impl Rasterizer for ReadingOnlyRasterizer {
        fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
            Ok(RenderedPage {
                width_px: request.width_px,
                height_px: 100,
                bgra: vec![0u8; request.width_px as usize * 100 * 4],
            })
        }
    }

    impl DocumentEngine for ReadingOnlyRasterizer {
        fn page_count(&self) -> u32 {
            1
        }

        fn page_geometry(&self, _idx: PageIndex) -> Result<PageGeometry, CommandError> {
            Ok(PageGeometry {
                width_pt: 612.0,
                height_pt: 792.0,
                rotation: crate::page::Rotation::Deg0,
                ..Default::default()
            })
        }

        fn outline(&self) -> Result<OutlineTree, CommandError> {
            Ok(OutlineTree::default())
        }

        fn extract_text(
            &self,
            _span: crate::page::PageSpan,
        ) -> Result<Vec<(PageIndex, String)>, CommandError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn reading_only_rasterizer_still_serves_print() {
        let mut engine = ReadingOnlyRasterizer;
        let spec = PrintSpec::new(150, false);
        let pages = render_for_print(&mut engine, &spec, &PageSelection::All).unwrap();
        // Letter 612pt @ 150dpi → 1275px（尺寸换算不依赖选项位覆写）。
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].width_px, 1275);
    }
}
