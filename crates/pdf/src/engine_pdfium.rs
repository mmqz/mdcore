//! pdfium 引擎实装（feature `engine-pdfium`）——[`DocumentEngine`] +
//! [`Rasterizer`] 双实现（PDF-c 阅读面）。
//!
//! ## 绑定纪律（决策记录 → gates.md PDF-c 节）
//!
//! - **定位序**：env `MDC_PDFIUM_LIB`（文件路径）→ 编译期候选清单 →
//!   系统（`bind_to_system_library`）。候选清单见
//!   [`PdfiumEngine::library_candidates`]（含产品仓 `bin/` 预留位与
//!   分析仓 spike 取件位——门禁环境路径）；
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
use crate::page::{PageGeometry, PageIndex, PageSpan, Rotation};
use crate::render::{Rasterizer, RenderRequest, RenderedPage};
use crate::security::{LimitBehavior, Limits, REASON_DECODE_OUTPUT};

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
    /// 绑定；S2-3 交叉测试消费）。
    pub fn shared_handle() -> Result<&'static pdfium_render::prelude::Pdfium, CommandError> {
        shared_pdfium()
    }

    /// 库定位候选清单（编译期 `CARGO_MANIFEST_DIR` 相对——观测/诊断面，
    /// 绑定按序尝试）：
    ///
    /// 1. 产品仓 `bin/lib/`（fetch-pdfium.sh 取件位，PDF-e 接线）；
    /// 2. 分析仓 spike `bin/lib/`（门禁环境取件位）；
    /// 3. crate 本地 `bin/lib/`。
    pub fn library_candidates() -> Vec<PathBuf> {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        [
            // 产品仓根/bin/lib（crates/pdf → 上两级）
            manifest.join("../../bin/lib"),
            // 分析仓 spike（crates/pdf → 上三级 = 仓根同级）
            manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/bin/lib"),
            // crate 本地
            manifest.join("bin/lib"),
        ]
        .into_iter()
        .map(|dir| Pdfium::pdfium_platform_library_name_at_path(&dir))
        .collect()
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
    fn page_geometry(&self, idx: PageIndex) -> Result<PageGeometry, CommandError> {
        let page = self.page(idx)?;
        let media = page
            .boundaries()
            .media()
            .map_err(CommandError::engine)?
            .bounds;
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
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => {
            let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
            return Ok(pdfium);
        }
        Err(why) => tried.push(format!(
            "system({}) → {why}",
            Pdfium::pdfium_platform_library_name().to_string_lossy()
        )),
    }
    Err(CommandError::engine(format!(
        "pdfium 库定位失败（定位序：{PDFIUM_LIB_ENV} → 编译期候选清单 → 系统）；已尝试：{}",
        tried.join("；")
    )))
}

/// 单路径尝试（成功 = 进程单例就位）。
fn try_bind(path: &Path) -> Result<&'static Pdfium, String> {
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
