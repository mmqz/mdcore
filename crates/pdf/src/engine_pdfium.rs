//! pdfium 引擎实装（feature `engine-pdfium`）——[`DocumentEngine`] +
//! [`Rasterizer`] 双实现（PDF-c 阅读面）。
//!
//! ## 绑定纪律（决策记录 → gates.md PDF-c 节）
//!
//! - **定位序**：env `MDC_PDFIUM_LIB`（文件路径）→ 候选清单 →
//!   系统（`bind_to_system_library`）。候选清单见
//!   [`PdfiumEngine::library_candidates`]（**exe 同目录部署位最前**
//!   ——#210 真实环境验证收口：INSTALL.md「放 exe 旁」的显式代码面
//!   对应，不依赖 `bind_to_system_library` 的 LoadLibrary 隐式搜索序；
//!   后接产品仓 `bin/` 取件位与分析仓 spike 取件位——门禁环境路径）；
//! - **进程级单绑定**：pdfium-render 的绑定表是进程全局
//!   （`PdfiumLibraryBindings` 全局 OnceCell；二次绑定报
//!   `PdfiumLibraryBindingsAlreadyInitialized` 且 crate 外无复用构造器）。
//!   本模块自持 `&'static Pdfium` 单例（`Box::leak`——Pdfium 结构体
//!   极小，真实绑定表在 pdfium-render 全局中永久存活，泄漏成本可
//!   忽略）。绑定失败**不缓存**（下次调用重试）；绑定由互斥锁串行化
//!   （并发首个 `open` 竞争安全）；
//! - **加载**：`load_pdf_from_byte_vec`（`FPDF_LoadMemDocument64`）——
//!   **全内存、路径无关**（S4.4 文件字节快照隔离的前提：渲染只读
//!   打开时字节快照）；文档独占字节缓冲（`set_source_byte_buffer`）。
//!   单进程多引擎 = 多个独立 pdfium 文档句柄（字节各自持有）。
//!
//! ## 线程纪律（S4.1/S4.4）
//!
//! pdfium 是单线程库。本 crate 以**两层**防线落地分线程隔离：
//!
//! 1. **主纪律（S4.1）**：[`PdfiumEngine`] 持文档独占，只在归属线程
//!    使用——典型形态 = 经 [`PdfSession::attach_engine`]（文档状态
//!    线程）或 [`crate::worker::RenderWorker`]（渲染工人线程，引擎以
//!    **所有权转移**进入，不跨线程共享引用）；
//! 2. **兜底**：pdfium-render `thread_safe` 特性把每次 FFI 调用经全局
//!    互斥串行化——文档线程与渲染线程**并存**（各自引擎实例）时，
//!    进程级 pdfium 调用被串行化（不会并发进 C 库）。互斥是兜底而非
//!    许可：归属纪律不被绕过（单工人渲染 v0 面无争用）。
//!
//! [`PdfiumEngine: Send`](PdfiumEngine) 由 thread_safe 特性背书
//! （语义锁见证，tests/pdfc_engine.rs）。
//!
//! ## 渲染路径（零 image）
//!
//! `PdfBitmap::from_bytes`（调用方缓冲，`FPDFBitmap_CreateEx`）+
//! `render_into_bitmap_with_config`（`FPDF_RenderPageBitmap`）+
//! `as_raw_bytes` 拷出——**不引入 image crate**（pdfium-render
//! default-features = false）。产物 BGRA 非预乘（pdfium 头文件
//! FPDFBitmap_BGRA 注记："Pixel components are independent of alpha"）。
//! 输出高按 `FPDF_GetPageWidthF/HeightF`（含 `/Rotate` 的显示尺寸）
//! 等比计算——与 pdfium 渲染器的可见域一致（`page_geometry` 的
//! MediaBox 几何是模型语义，两者在 CropBox ⊂ MediaBox 的文档上
//! 可有轻微差异，v0 取渲染一致性优先）。
//!
//! ## S3-L2 限额（渲染面）
//!
//! `rasterize` 以 `Limits::default()` 执行 ❷钳制：单边 ≤ 8192px、
//! 总像素 ≤ 64M（超限收敛 + 实际尺寸在 [`crate::render::RenderedPage`]
//! 报告，调用方读报告值——不假装请求值生效）。钳制后另有 S3-L0
//! 渲染目标面 ❶硬闸（宽×高×4 缓冲字节 vs `decode_output_max_bytes`，
//! 先于分配——PDF-e 接线；冻结数值下不触发的纵深防线）。会话自定义
//! 限额的接线属 PDF-d+（数值权威在 [`crate::security::Limits`] 不变）。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use pdfium_render::prelude::*;

use crate::command::CommandError;
use crate::engine::DocumentEngine;
use crate::outline::{OutlineNode, OutlineTarget, OutlineTree};
use crate::page::{PageBox, PageGeometry, PageIndex, PageSpan, Rotation};
use crate::render::{Rasterizer, RenderRequest, RenderedPage};
use crate::security::{LimitBehavior, Limits, REASON_DECODE_OUTPUT};
use crate::table::PositionedChar;

/// 库定位环境变量（文件路径；优先级最高）。
pub const PDFIUM_LIB_ENV: &str = "MDC_PDFIUM_LIB";

/// 进程级 pdfium 实例（成功绑定后缓存；失败不缓存——重试语义）。
static PDFIUM: OnceLock<&'static Pdfium> = OnceLock::new();

/// 绑定串行锁（并发首个 open 的竞争仲裁；锁毒化不复位——绑定失败
/// 不持锁状态，下次调用重来）。
static BIND_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// pdfium 引擎（文档独占；线程纪律见模块文档）。
///
/// 构造出口：[`PdfiumEngine::open`]（定位序绑定）/ 首个成功绑定的
/// [`PdfiumEngine::open_with_library`]。进程内首个绑定生效，后续
/// `open` 复用单例。
pub struct PdfiumEngine {
    document: PdfDocument<'static>,
}

impl PdfiumEngine {
    /// 打开文档（定位序绑定：env → 候选清单 → 系统；绑定失败返回
    /// `CommandError::Engine`——消息含全部已尝试路径）。打开面限额
    /// （PDF-e 接线，open 家族入口统一）：S3-L7 输入字节 ❶先于装载；
    /// S3-L4 打开预算 ❶（装载耗时，超限 typed 拒绝）。
    pub fn open(bytes: Vec<u8>) -> Result<Self, CommandError> {
        let limits = Limits::default();
        limits.check_open_input_bytes(bytes.len() as u64)?;
        let started = Instant::now();
        let pdfium = shared_pdfium()?;
        let document = pdfium
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(CommandError::engine)?;
        limits.check_open_budget(started)?;
        Ok(Self { document })
    }

    /// 显式单路径绑定 + 打开（无回退——宿主引导/诊断路径）。限额面
    /// 与 [`PdfiumEngine::open`] 同款（S3-L7 输入字节 + S3-L4 打开预算，
    /// PDF-e 接线）。
    ///
    /// 语义：**始终**尝试 `bind_to_library(path)`——路径无效或进程已
    /// 绑定均返回 `Engine` 错误（诊断确定性：不因单例已在位而静默改用
    /// 旧绑定）。首个成功者成为进程单例（后续 [`Self::open`] 复用）。
    pub fn open_with_library(path: &Path, bytes: Vec<u8>) -> Result<Self, CommandError> {
        let limits = Limits::default();
        limits.check_open_input_bytes(bytes.len() as u64)?;
        let started = Instant::now();
        let pdfium = bind_explicit(path)?;
        let document = pdfium
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(CommandError::engine)?;
        limits.check_open_budget(started)?;
        Ok(Self { document })
    }

    /// 进程级 pdfium 单例引用（**交叉读面**：测试/宿主在同一绑定
    /// 纪律下复用单例打开其他文档——pdfium 进程级单实例，禁止二次
    /// 绑定；S2-2 互通 / S2-3 交叉测试消费）。绑定失败 = `Engine`（与
    /// [`PdfiumEngine::open`] 同定位序合同）。
    pub fn shared_handle() -> Result<&'static pdfium_render::prelude::Pdfium, CommandError> {
        shared_pdfium()
    }

    /// 库定位候选清单（观测/诊断面，绑定按序尝试）：
    ///
    /// 0. **exe 同目录**（产品部署位——#210 真实环境验证收口。
    ///    INSTALL.md 指引「把库放到 exe 同目录」的显式候选：此前该
    ///    放置形态只能经 `bind_to_system_library()` 的 LoadLibrary
    ///    隐式搜索序兜底——理论可行（Windows DLL 搜索序 exe 目录
    ///    第一位）但从未被真实验证，且绑定失败时定位详情不含此
    ///    位置（用户按文档操作失败后不可诊断）。显式候选后：部署
    ///    语义与代码直接对应，报错「定位详情」可见该路径已尝试）；
    /// 1. 产品仓 `bin/lib/`（fetch-pdfium.sh 取件位，PDF-e 接线）；
    /// 2. 分析仓 spike `bin/lib/`（门禁环境取件位）；
    /// 3. crate 本地 `bin/lib/`。
    ///
    /// exe 同目录经运行期 `current_exe()` 推导（部署语义跟着二进制
    /// 走——用户把库放在哪个 exe 旁，哪个 exe 就能找到；与 1-3 的
    /// 编译期 `CARGO_MANIFEST_DIR` 相对推导不同源）。推导失败（
    /// `current_exe` 不可得的极端环境）静默跳过，后续候选与系统
    /// 绑定兜底。优先序实证见 `tests/pdfk_deploy.rs`（进程级首绑定
    /// 验证）；形状锁 `exe_dir_is_first_candidate`。
    pub fn library_candidates() -> Vec<PathBuf> {
        Self::library_candidate_roles()
            .into_iter()
            .map(|(_, path)| path)
            .collect()
    }

    /// 候选清单（角色名 + 路径）——[`Self::library_candidates`] 的带角色面。
    /// #68 方向一（`--check-pdfium` 部署自检）的逐层探针标题与绑定清单
    /// **同源**（单一事实源：角色/顺序/路径只在本函数维护，探针展示与
    /// 绑定走查不会漂移）。角色语义见 [`Self::library_candidates`] 文档。
    pub fn library_candidate_roles() -> Vec<(&'static str, PathBuf)> {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut candidates: Vec<(&'static str, PathBuf)> = Vec::with_capacity(4);
        // 0. exe 同目录（产品部署位——运行期推导，最前：用户放置位
        //    优先于开发取件位）。
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push((
                    "exe 同目录（产品部署位）",
                    Pdfium::pdfium_platform_library_name_at_path(dir),
                ));
            }
        }
        // 1-3. 开发/门禁取件位（编译期 manifest 相对）。
        candidates.extend(
            [
                (
                    "产品仓 bin/lib（开发取件位）",
                    // 产品仓根/bin/lib（crates/pdf → 上两级）
                    manifest.join("../../bin/lib"),
                ),
                (
                    "分析仓 spike（门禁取件位）",
                    // 分析仓 spike（crates/pdf → 上三级 = 仓根同级）
                    manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/bin/lib"),
                ),
                ("crate 本地 bin/lib", manifest.join("bin/lib")),
            ]
            .into_iter()
            .map(|(role, dir)| (role, Pdfium::pdfium_platform_library_name_at_path(&dir))),
        );
        candidates
    }

    /// 单页句柄（页界防御在先——引擎层不信任调用方）。
    fn page(&self, idx: PageIndex) -> Result<PdfPage<'_>, CommandError> {
        let pages = self.document.pages();
        let len = pages.len();
        if idx.0 as i64 >= i64::from(len) {
            return Err(CommandError::InvalidPage(idx));
        }
        pages.get(idx.0 as i32).map_err(CommandError::engine)
    }
}

/// 手写 Debug（pdfium 文档句柄不可推导——打印页数计数面；句柄值
/// 不进 Debug 输出：C 指针值是诊断噪声且可变性无语义）。
impl std::fmt::Debug for PdfiumEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfiumEngine")
            .field("pages", &self.document.pages().len())
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// DocumentEngine（只读数据面）
// ---------------------------------------------------------------------------

impl DocumentEngine for PdfiumEngine {
    /// 文档页数（`FPDF_GetPageCount`；原 inherent 方法升入 trait——
    /// attach_engine 装配期一致性校验的引擎侧权威，宿主构造
    /// `DocumentMeta` 同源用）。
    fn page_count(&self) -> u32 {
        self.document.pages().len() as u32
    }

    /// 单页几何：原始 MediaBox（`FPDFPage_GetMediaBox`）+ `/Rotate`
    /// （`FPDFPage_GetRotation`）——`/Rotate` 归一化取
    /// [`PageGeometry::effective_width`] 消费（消费方不自行处理旋转）。
    ///
    /// #304 起 `media_box`/`crop_box` 同步产出（pdfium
    /// `FPDFPage_GetMediaBox`/`GetCropBox`；无 `/CropBox` 时 pdfium
    /// 返回 MediaBox 形态——如实按 `None` 归档，与 spec 7.7.3.3 缺省
    /// 语义一致，变换层兜底）。设备↔用户空间变换 =
    /// [`crate::transform::PageTransform::from_geometry`]。
    fn page_geometry(&self, idx: PageIndex) -> Result<PageGeometry, CommandError> {
        let page = self.page(idx)?;
        let media = page
            .boundaries()
            .media()
            .map_err(CommandError::engine)?
            .bounds;
        let (ml, mb, mr, mt) = (
            media.left().value,
            media.bottom().value,
            media.right().value,
            media.top().value,
        );
        // CropBox：读失败按「页无 /CropBox」处理（None——pdfium 对无
        // /CropBox 页通常回落 MediaBox，两形态等价，不区别报错）。
        let crop = page
            .boundaries()
            .crop()
            .ok()
            .map(|b| {
                let cb = b.bounds;
                PageBox {
                    x0: cb.left().value,
                    y0: cb.bottom().value,
                    x1: cb.right().value,
                    y1: cb.top().value,
                }
            })
            .filter(|c| {
                // pdfium 回落形态（crop ≡ media，容差 0.5pt）归 None——
                // 「真裁剪」与「缺省」在变换层同路（crop=media 兜底），
                // 归档保持语义干净。
                (c.x0 - ml).abs() > 0.5
                    || (c.y0 - mb).abs() > 0.5
                    || (c.x1 - mr).abs() > 0.5
                    || (c.y1 - mt).abs() > 0.5
            });
        let rotation = match page.rotation().map_err(CommandError::engine)? {
            PdfPageRenderRotation::None => Rotation::Deg0,
            PdfPageRenderRotation::Degrees90 => Rotation::Deg90,
            PdfPageRenderRotation::Degrees180 => Rotation::Deg180,
            PdfPageRenderRotation::Degrees270 => Rotation::Deg270,
        };
        Ok(PageGeometry {
            width_pt: media.width().value,
            height_pt: media.height().value,
            rotation,
            media_box: PageBox {
                x0: ml,
                y0: mb,
                x1: mr,
                y1: mt,
            },
            crop_box: crop,
        })
    }

    /// 大纲：pdfium 书签树 → [`OutlineTree`]（前序文档序由树结构承载）。
    /// 无大纲文档 → 空树（不 Err）。**页目标解析失败/越界的节点被丢弃**
    /// （含其子树——v0 目标只有页变体；命名/URI 目标随链接面扩展时重访）。
    fn outline(&self) -> Result<OutlineTree, CommandError> {
        let page_count = self.document.pages().len() as u32;
        let mut roots = Vec::new();
        if let Some(root) = self.document.bookmarks().root() {
            // pdfium-render 语义：`root()` 返回**首个顶层书签本体**（非伪根，
            // `PdfBookmarks::root` = `FPDFBookmark_GetFirstChild(doc, NULL)`），
            // 而 `iter_siblings()` 迭代时自排除（skip_over: Some(self)）——
            // 首条必须显式并入，否则首个顶层书签恒丢失（正路径首证：
            // #68 批 4 带书签夹具 3 条实得 2；负路径此前仅有
            // `outline_empty_tree_for_plain_fixture` 背书，正路径零实证）。
            for bookmark in std::iter::once(root.clone()).chain(root.iter_siblings()) {
                if let Some(node) = engine_outline_node(&bookmark, page_count) {
                    roots.push(node);
                }
            }
        }
        Ok(OutlineTree::from_roots(roots))
    }

    /// 页区间文本抽取（页号升序；`PdfPageText` 句柄逐页创建即释放）。
    fn extract_text(&self, span: PageSpan) -> Result<Vec<(PageIndex, String)>, CommandError> {
        let pages = self.document.pages();
        let count = pages.len();
        let mut out = Vec::with_capacity(span.len() as usize);
        for idx in span.iter() {
            if idx.0 as i64 >= i64::from(count) {
                // 引擎层防御（会话层已校验——双保险，引擎独立可用性）。
                return Err(CommandError::InvalidPage(idx));
            }
            let page = pages.get(idx.0 as i32).map_err(CommandError::engine)?;
            let text = page.text().map_err(CommandError::engine)?.all();
            out.push((idx, text));
        }
        Ok(out)
    }
}

/// 书签 → 大纲节点（目标解析失败/越界 → None（丢弃子树）；标题缺失
/// → 空串（pdfium 行为，不造词））。
fn engine_outline_node(bookmark: &PdfBookmark<'_>, page_count: u32) -> Option<OutlineNode> {
    let destination = bookmark.destination()?;
    let page_index = destination.page_index().ok()?;
    if !(0..page_count as i64).contains(&i64::from(page_index)) {
        return None; // 越界目标（畸形文档）——丢弃
    }
    let page_no = page_index as u32;
    let children = bookmark
        .iter_direct_children()
        .filter_map(|child| engine_outline_node(&child, page_count))
        .collect();
    Some(OutlineNode {
        title: bookmark.title().unwrap_or_default(),
        target: OutlineTarget::Page(PageIndex(page_no)),
        children,
    })
}

// ---------------------------------------------------------------------------
// Rasterizer（渲染面）
// ---------------------------------------------------------------------------

impl Rasterizer for PdfiumEngine {
    /// 渲染单页：目标宽驱动等比缩放；S3-L2 ❷钳制（单边/总像素）——
    /// 生效尺寸在 [`RenderedPage`] 报告。
    fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
        let page = self.page(request.page)?;
        let limits = Limits::default();

        // 单边钳制（❷：收敛继续，不拒绝）。
        let mut width_px = request.width_px.clamp(1, limits.raster_max_side_px);

        // 等比高：按 pdfium 显示尺寸（FPDF_GetPageWidthF/HeightF——
        // 含 /Rotate 与可见域），与渲染器的映射面一致。
        let page_w = f64::from(page.width().value).max(0.001);
        let page_h = f64::from(page.height().value).max(0.001);
        let mut height_px = ((f64::from(width_px) * page_h / page_w).round() as u32)
            .clamp(1, limits.raster_max_side_px);

        // 总像素钳制（❷）：超预算等比收缩（宽高同缩，保纵横比）。
        let budget = limits.raster_max_pixels as f64;
        if f64::from(width_px) * f64::from(height_px) > budget {
            let scale = (budget / (f64::from(width_px) * f64::from(height_px))).sqrt();
            width_px = ((f64::from(width_px) * scale).floor() as u32).max(1);
            height_px = ((f64::from(height_px) * scale).floor() as u32).max(1);
        }

        // S3-L0 结构层 → 渲染目标面（❶硬拒绝，先于分配——PDF-e 接线）：
        // 目标缓冲字节（宽×高×4，BGRA）超 `decode_output_max_bytes` →
        // typed 拒绝。冻结数值下的纵深硬闸（64M px × 4B = 256 MiB <
        // 1 GiB 不触发）——未来限额调整（如放大像素预算）时此处收口。
        let buffer_bytes = u64::from(width_px) * u64::from(height_px) * 4;
        let (effective, behavior) = limits.enforce_decode_output_bytes(buffer_bytes);
        if behavior == LimitBehavior::HardReject && effective < buffer_bytes {
            return Err(CommandError::SecurityDeny {
                reason: REASON_DECODE_OUTPUT,
            });
        }

        // 调用方缓冲 + 低层位图（FPDFBitmap_CreateEx/FPDF_RenderPageBitmap
        // 路线——零 image）。BGRA 4 字节/像素恒 4 对齐（stride = w*4）。
        let mut buffer = vec![0u8; width_px as usize * height_px as usize * 4];
        let mut bitmap = PdfBitmap::from_bytes(
            width_px as i32,
            height_px as i32,
            PdfBitmapFormat::BGRA,
            &mut buffer,
        )
        .map_err(CommandError::engine)?;
        let config = PdfRenderConfig::new()
            // 输出尺寸 = 位图尺寸（宽主导；等比已自行计算）。
            .set_fixed_size_to_bitmap(&bitmap)
            .set_format(PdfBitmapFormat::BGRA)
            // 关闭 FPDF_REVERSE_BYTE_ORDER——原生 BGRA 直读
            // （默认 true 会把通道翻成 RGB 序）。
            .set_reverse_byte_order(false)
            // S2-3（#103 任务 3）：注释渲染显式钉定——PDF 内标注
            // （高亮/便签/形状…）随页面光栅化可见（FPDF_ANNOT）。
            // pdfium-render 缺省即 true，此处**显式声明**而非依赖缺省：
            // 行为契约进代码面（升级 pdfium-render 时缺省漂移即编译期
            // 不可见的行为红——显式调用使意图持久）。语义锁
            // pdff3_render_ext::annotations_visible_in_render。
            .render_annotations(true);
        page.render_into_bitmap_with_config(&mut bitmap, &config)
            .map_err(CommandError::engine)?;
        let bgra = bitmap.as_raw_bytes();
        debug_assert_eq!(
            bgra.len(),
            width_px as usize * height_px as usize * 4,
            "BGRA 缓冲长度契约（语义锁）"
        );
        Ok(RenderedPage {
            width_px,
            height_px,
            bgra,
        })
    }
}

// ---------------------------------------------------------------------------
// 绑定（进程级单例）
// ---------------------------------------------------------------------------

/// 取进程单例（未绑定则按定位序绑定；失败不缓存——重试）。
fn shared_pdfium() -> Result<&'static Pdfium, CommandError> {
    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium);
    }
    let lock = BIND_LOCK.get_or_init(|| Mutex::new(()));
    // 锁毒化不复位（into_inner）：绑定失败不持锁状态，下个调用方直接进。
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium); // 双检（等待期间他线程完成绑定）
    }
    let pdfium = bind_by_resolution()?;
    let _ = PDFIUM.set(pdfium);
    Ok(pdfium)
}

/// 显式单路径绑定（无回退；见 [`PdfiumEngine::open_with_library`] 语义）。
fn bind_explicit(path: &Path) -> Result<&'static Pdfium, CommandError> {
    let lock = BIND_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match Pdfium::bind_to_library(path) {
        Ok(bindings) => {
            let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
            let _ = PDFIUM.set(pdfium);
            Ok(pdfium)
        }
        Err(why) => Err(CommandError::engine(format!(
            "pdfium 显式绑定失败（{}）：{why}",
            path.display()
        ))),
    }
}

/// 定位序绑定：env → 编译期候选 → 系统（错误消息含全部尝试记录——
/// 门禁环境必须可诊断，不静默降级）。
fn bind_by_resolution() -> Result<&'static Pdfium, CommandError> {
    let mut tried: Vec<String> = Vec::new();
    if let Ok(path) = std::env::var(PDFIUM_LIB_ENV) {
        match try_bind(Path::new(&path)) {
            Ok(pdfium) => return Ok(pdfium),
            Err(why) => tried.push(format!("{PDFIUM_LIB_ENV}={path} → {why}")),
        }
    }
    for candidate in PdfiumEngine::library_candidates() {
        match try_bind(&candidate) {
            Ok(pdfium) => return Ok(pdfium),
            Err(why) => tried.push(format!("{} → {why}", candidate.display())),
        }
    }
    match try_bind_system() {
        Ok(pdfium) => return Ok(pdfium),
        Err(why) => tried.push(format!(
            "system({}) → {why}",
            Pdfium::pdfium_platform_library_name().to_string_lossy()
        )),
    }
    let hint = pdfium_missing_library_hint();
    Err(CommandError::engine(format!(
        "{hint} 定位详情：{}",
        tried.join("；")
    )))
}

fn pdfium_missing_library_hint() -> String {
    let library_name = Pdfium::pdfium_platform_library_name()
        .to_string_lossy()
        .into_owned();
    format!(
        "PDF 引擎不可用：未找到或无法加载 {library_name}。修复：下载 https://github.com/bblanchon/pdfium-binaries/releases，解压后将 bin/{library_name} 放到 <exe 同目录>，或设置环境变量 {PDFIUM_LIB_ENV}=<库完整路径>。请确认下载包与操作系统和架构匹配。"
    )
}

/// 系统装载尝试（成功 = 进程单例就位）。pub(crate)：deploy_check
/// 部署自检的绑定走查与生产定位序共用同一实现（行为面不双轨）。
pub(crate) fn try_bind_system() -> Result<&'static Pdfium, String> {
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => {
            let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
            let _ = PDFIUM.set(pdfium);
            Ok(pdfium)
        }
        Err(why) => Err(format!("库加载失败：{why}")),
    }
}

/// 单路径尝试（成功 = 进程单例就位）。pub(crate)：deploy_check 的绑定
/// 走查按定位序逐层调用本函数——与生产 [`bind_by_resolution`] 同一尝试
/// 语义（成功副作用：crate 单例就位，后续 open 复用——自检进程与
/// 应用进程的绑定行为面一致）。
pub(crate) fn try_bind(path: &Path) -> Result<&'static Pdfium, String> {
    match Pdfium::bind_to_library(path) {
        Ok(bindings) => {
            let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
            let _ = PDFIUM.set(pdfium);
            Ok(pdfium)
        }
        Err(PdfiumError::LoadLibraryError(why)) => Err(format!("库加载失败：{why}")),
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
            Err("进程已被绑定（外部绑定者不受支持——本 crate 单例未就位的竞争窗口）".to_string())
        }
        Err(why) => Err(format!("绑定失败：{why}")),
    }
}

// #232 / R9·P1：表格结构化提取的位置层——逐字符 loose_bounds（整字型
// 包围盒；tight 为字形精裁，行带聚类用 loose 跨字号更稳）+ 实际字号。
// 算法层（行带/切格/列锚/落列）在 crate::table，纯函数零引擎依赖。
impl PdfiumEngine {
    /// 页位置字符流（表格结构化提取输入；pdfium 路径）。
    ///
    /// 无 unicode 映射的字符与包围盒读取失败项跳过（不静默丢文本面
    /// 之外的语义——这些字符不参与几何聚类，登记于 crate::table 边界）。
    pub fn positioned_chars(&self, page: PageIndex) -> Result<Vec<PositionedChar>, CommandError> {
        let pages = self.document.pages();
        let count = pages.len();
        if page.0 as i64 >= i64::from(count) {
            // 引擎层防御（与 extract_text 同口径——会话层已校验双保险）。
            return Err(CommandError::InvalidPage(page));
        }
        let page_obj = pages.get(page.0 as i32).map_err(CommandError::engine)?;
        let text = page_obj.text().map_err(CommandError::engine)?;
        let mut out = Vec::new();
        for ch in text.chars().iter() {
            let (Some(c), Ok(rect)) = (ch.unicode_char(), ch.loose_bounds()) else {
                continue;
            };
            out.push(PositionedChar {
                ch: c,
                left: rect.left().value,
                bottom: rect.bottom().value,
                right: rect.right().value,
                top: rect.top().value,
                font_size: ch.scaled_font_size().value,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_dir_is_first_candidate() {
        // #210 部署位验证（形状面）：exe 同目录必须是候选 0——
        // INSTALL.md「放 exe 同目录」与定位序直接对应（产品部署位
        // 优先于开发取件位）。绑定优先序的进程级实证见
        // tests/pdfk_deploy.rs（独立测试二进制 = 独立进程的首绑定）。
        let candidates = PdfiumEngine::library_candidates();
        assert!(
            !candidates.is_empty(),
            "候选清单非空（exe 位 + 三个取件位）"
        );
        let exe_dir = std::env::current_exe()
            .expect("测试环境 current_exe 可得")
            .parent()
            .expect("exe 路径有父目录")
            .to_path_buf();
        let expected = Pdfium::pdfium_platform_library_name_at_path(&exe_dir);
        assert_eq!(candidates[0], expected, "exe 同目录部署位必须是候选 0");
        // 开发取件位保持（门禁环境取件序不变——纯加性变更）。
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(
            candidates.contains(&Pdfium::pdfium_platform_library_name_at_path(
                &manifest.join("../../bin/lib")
            )),
            "产品仓取件位仍在候选清单"
        );
    }

    #[test]
    fn missing_library_hint_is_actionable() {
        let hint = pdfium_missing_library_hint();
        let library_name = Pdfium::pdfium_platform_library_name()
            .to_string_lossy()
            .into_owned();

        assert!(hint.contains(&library_name));
        assert!(hint.contains("https://github.com/bblanchon/pdfium-binaries/releases"));
        assert!(hint.contains("bin/"));
        assert!(hint.contains("<exe 同目录>"));
        assert!(hint.contains(PDFIUM_LIB_ENV));
        assert!(hint.contains("<库完整路径>"));
    }
}
