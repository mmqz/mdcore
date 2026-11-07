//! PDF-c — pdfium 引擎语义锁（feature `engine-pdfium`；语义基线 =
//! PDF-a spike 三探针实测：Letter 612×792 fixture、文本抽取非空、
//! 光栅化位图契约）。
//!
//! **环境合同（不静默 skip）**：库定位走 [`PdfiumEngine::open`] 的定位序
//! （env `MDC_PDFIUM_LIB` → 编译期候选清单（含分析仓 spike 取件位）→
//! 系统）；fixtures 走 env `MDC_PDF_TEST_FIXTURES`（目录）→ 编译期候选
//! （crate 本地 fixtures/ 优先，spike 位兜底）。**任一缺失 = 本文件明确
//! panic 并打印全部已尝试候选**——门禁环境必须可诊断，静默跳过 = 测试面造假。
//!
//! 锁清单（每锁一条 `#[test]`）：
//! 1. `engine_binds_and_opens_fixture`——定位序绑定 + 打开：small.pdf
//!    3 页（spike 探针 1 基线复现）；
//! 2. `geometry_letter_media_box`——单页几何：MediaBox 612×792 pt +
//!    /Rotate 0（fixture 构造参数即基线；effective_* 不交换宽高）；
//! 3. `geometry_out_of_range_is_invalid_page`——页界防御：越界查询/
//!    抽取 → typed `InvalidPage`（引擎层不信任调用方）；
//! 4. `extract_text_nonempty_shape_and_order`——文本抽取非空 + 页集
//!    形状（页号升序、逐页标签 "Spike fixture page N of 3"）；
//! 5. `outline_empty_tree_for_plain_fixture`——无书签文档 → 空大纲
//!    （Ok 而非 Err——空树语义）；
//! 6. `session_engine_data_source_end_to_end`——会话接线：attach 后
//!    ExtractText/Search 走引擎实数据（注入占位几何被旁路）；
//! 7. `session_search_limit_semantics_unchanged`——限额语义单一出口：
//!    引擎路径 max_hits 钳制 → truncated + 空 query 硬拒绝（与注入路径
//!    同锁——两路径不漂移）；
//! 8. `rasterize_buffer_contract_letter_aspect`——位图合同：缓冲长 =
//!    w*h*4、高按显示几何等比（300 → 388）；
//! 9. `rasterize_paints_nonblank_bitmap`——渲染真实落笔：非全零 +
//!    白底主导（pdfium 渲染器确实写过缓冲）；
//! 10. `rasterize_s3_clamp_bounds`——S3-L2 ❷钳制：请求 0 → 1px；请求
//!     999,999 → 单边 ≤ 8192 / 总像素 ≤ 64M（8000×8000 收敛值锁定）；
//! 11. `engine_error_surface_binding_and_load`——typed 错误面：绑定
//!     失败 / 装载垃圾字节 → `CommandError::Engine`（消息含路径）；
//! 12. `send_witness_and_worker_roundtrip`——`PdfiumEngine: Send` 见证
//!     （thread_safe 特性背书）+ 真实引擎过渲染工人（S4.1 所有权转移）；
//! 13. `full_chain_cache_hit_zero_rasterize`——全链路：工人渲染 → 缓存
//!     → 二次命中**零 rasterize 调用**（阅读面翻页性能根保障的引擎实证）；
//! 14. `outline_positive_tree_includes_first_top_level`——带书签正路径
//!     首证（#68 批 4 修复回归锁）：前序树全量，首个顶层书签不丢
//!     （修复前症状：`root()` 首个顶层书签被 `iter_siblings()` 自排除
//!     恒丢失——3 条实得 2）。

// feature 门（默认构建零引擎依赖——本文件整体退化为空目标，跨环境
// 可构建纪律保持）。
#![cfg(feature = "engine-pdfium")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mdc_pdf::cache::PageCache;
use mdc_pdf::command::{Command, CommandError, SearchOptions, Source, TextTarget};
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageGeometry, PageIndex, PageSpan, Rotation};
use mdc_pdf::render::{Rasterizer, RenderRequest, RenderedPage};
use mdc_pdf::worker::{RenderWorker, WorkerReply};

// ---------------------------------------------------------------------------
// 环境合同：库（定位序在引擎内）+ fixtures（env → 候选清单）
// ---------------------------------------------------------------------------

/// fixtures 定位环境变量（目录形态）。
const FIXTURES_ENV: &str = "MDC_PDF_TEST_FIXTURES";

/// 编译期 fixtures 候选清单（crate manifest 相对——与库候选同根推导）。
/// crate 本地优先（T1 审计 F12：产品仓自持素材位在前——fixtures 入库后
/// fresh clone 即可复现，不受仓外目录状态影响）；spike 位 = bootstrap
/// 生成器/门禁环境兜底。
fn fixture_candidates() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        // crate 本地 fixtures/（产品仓自带素材位，PDF-e 起入库）。
        manifest.join("fixtures"),
        // 分析仓 spike fixtures（bootstrap 生成位/门禁环境取件位）。
        manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/fixtures"),
    ]
}

/// 读取 fixture 字节（缺失 = panic 打印全部候选——不静默 skip）。
fn fixture_bytes(name: &str) -> Vec<u8> {
    let mut tried: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var(FIXTURES_ENV) {
        dirs.push(PathBuf::from(dir));
    }
    dirs.extend(fixture_candidates());
    for dir in &dirs {
        let path = dir.join(name);
        match std::fs::read(&path) {
            Ok(bytes) => return bytes,
            Err(why) => tried.push(format!("{} → {why}", path.display())),
        }
    }
    panic!(
        "fixture {name} 定位失败（定位序：{FIXTURES_ENV} → 编译期候选）；已尝试：{}",
        tried.join("；")
    );
}

/// 打开 small.pdf 引擎（绑定失败 = panic 打印定位序错误——门禁可诊断）。
fn open_small() -> PdfiumEngine {
    match PdfiumEngine::open(fixture_bytes("small.pdf")) {
        Ok(engine) => engine,
        Err(CommandError::Engine(detail)) => panic!(
            "pdfium 绑定/打开失败（库定位序：MDC_PDFIUM_LIB → 候选 {} → 系统）：{detail}",
            PdfiumEngine::library_candidates()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" → ")
        ),
        Err(other) => panic!("打开 small.pdf 应成功或 Engine 错误，得 {other:?}"),
    }
}

/// 装配会话（占位几何数组——引擎接线后几何查询应旁路占位值）。
/// meta.page_count 与引擎页数同源构造（T1 审计 O3 项后 attach_engine
/// 装配期一致性校验——两侧不一致 = typed 拒绝，见 pdfb_skeleton 锁）。
fn session_with_engine(engine: PdfiumEngine, placeholder: PageGeometry) -> PdfSession {
    let count = engine.page_count();
    let mut session = PdfSession::open(
        DocumentMeta {
            id: PdfDocumentId(1),
            file_name: "small.pdf".into(),
            page_count: mdc_pdf::page::PageCount(count),
        },
        Default::default(),
        vec![placeholder; count as usize],
        OutlineTree::new(),
    )
    .expect("会话装配（元数据与占位几何自洽）");
    session
        .attach_engine(Box::new(engine))
        .expect("引擎页数与元数据一致（同源构造）");
    session
}

// ---------------------------------------------------------------------------
// 1-5：引擎只读数据面（DocumentEngine）
// ---------------------------------------------------------------------------

#[test]
fn engine_binds_and_opens_fixture() {
    // 定位序绑定 + 全内存加载：small.pdf = 3 页（spike 探针 1 基线）。
    // 本测试即环境合同的一部分——库缺失 = panic 打印候选，不静默跳过。
    let engine = open_small();
    assert_eq!(engine.page_count(), 3, "small.pdf 页数（fixture 构造参数）");
}

#[test]
fn geometry_letter_media_box() {
    // 单页几何 = MediaBox 612×792 pt + /Rotate 0（Letter 纵向；fixture
    // 构造参数即基线——几何漂移 = 渲染/文本对位全错，第一锚点）。
    let engine = open_small();
    let geometry = engine.page_geometry(PageIndex(0)).expect("首页几何");
    assert_eq!(geometry.width_pt, 612.0, "Letter 宽（pt）");
    assert_eq!(geometry.height_pt, 792.0, "Letter 高（pt）");
    assert_eq!(geometry.rotation, Rotation::Deg0, "fixture 无 /Rotate");
    assert_eq!(geometry.effective_width(), 612.0, "0° 不交换宽高");
    assert_eq!(geometry.effective_height(), 792.0, "0° 不交换宽高");
}

#[test]
fn geometry_out_of_range_is_invalid_page() {
    // 页界防御：越界几何查询/文本抽取 → typed InvalidPage（引擎层不信任
    // 调用方——会话校验之外的独立防线，引擎单独可用）。
    let engine = open_small();
    assert_eq!(
        engine.page_geometry(PageIndex(3)),
        Err(CommandError::InvalidPage(PageIndex(3))),
        "几何越界 → InvalidPage 携带肇事页号"
    );
    let span = PageSpan::new(PageIndex(0), PageIndex(3)).expect("非反转");
    assert_eq!(
        engine.extract_text(span),
        Err(CommandError::InvalidPage(PageIndex(3))),
        "抽取越界 → InvalidPage"
    );
}

#[test]
fn extract_text_nonempty_shape_and_order() {
    // 文本抽取：非空 + 页集形状（页号升序 + 逐页标签）——PDF-a 探针 1
    // 的文本断言升级为语义锁（抽取真实发生，非空串占位）。
    let engine = open_small();
    let span = PageSpan::new(PageIndex(0), PageIndex(2)).expect("非反转");
    let pages = engine.extract_text(span).expect("抽取成功");
    assert_eq!(pages.len(), 3, "页集形状：三页全抽");
    for (i, (idx, text)) in pages.iter().enumerate() {
        assert_eq!(idx.0, i as u32, "页号升序");
        let expect = format!("Spike fixture page {} of 3.", i + 1);
        assert!(
            text.contains(&expect),
            "页 {i} 文本应含标签 {expect:?}（实测前 60 字：{}）",
            text.chars().take(60).collect::<String>()
        );
        assert!(!text.is_empty(), "非空");
    }
}

#[test]
fn outline_empty_tree_for_plain_fixture() {
    // 无书签文档 → 空大纲 Ok（空树而非 Err——"没有大纲"是正常态）。
    let engine = open_small();
    let outline = engine.outline().expect("大纲查询不因无书签失败");
    assert!(outline.is_empty(), "plain fixture 无书签 → 空树");
}

#[test]
fn outline_positive_tree_includes_first_top_level() {
    // 正路径首证（#68 批 4 修复回归锁）：带书签文档 → 前序树全量。
    // 修复前症状：`PdfBookmarks::root()` 返回首个顶层书签本体（非伪根，
    // = `FPDFBookmark_GetFirstChild(doc, NULL)`），而 `iter_siblings()`
    // 迭代时自排除——首个顶层书签恒丢失（3 条实得 2）。
    // 夹具：bookmarked.pdf（3 页 + 嵌套大纲 5 条，生成器
    // scripts/gen_bookmarked_fixture.py——crates/pdf/fixtures/ 入库）。
    let engine = match PdfiumEngine::open(fixture_bytes("bookmarked.pdf")) {
        Ok(engine) => engine,
        Err(CommandError::Engine(detail)) => {
            panic!("pdfium 绑定/打开失败（库定位序：MDC_PDFIUM_LIB → 候选 → 系统）：{detail}")
        }
        Err(other) => panic!("打开 bookmarked.pdf 失败：{other:?}"),
    };
    let outline = engine.outline().expect("带书签文档大纲查询成功");
    let flat: Vec<(usize, String, u32)> = outline
        .iter()
        .map(|(depth, node)| {
            (
                depth,
                node.title.clone(),
                match node.target {
                    mdc_pdf::outline::OutlineTarget::Page(p) => p.0,
                },
            )
        })
        .collect();
    assert_eq!(
        flat,
        vec![
            (0, "第一章".to_string(), 0),
            (0, "第二章".to_string(), 1),
            (1, "2.1".to_string(), 1),
            (1, "2.2".to_string(), 2),
            (0, "第三章".to_string(), 2),
        ],
        "前序文档序 + 层级 + 目标页——首顶层书签不丢、嵌套子级全量"
    );
}

// ---------------------------------------------------------------------------
// 6-7：会话接线（命令面零变化 + 限额语义单一出口）
// ---------------------------------------------------------------------------

#[test]
fn session_engine_data_source_end_to_end() {
    // 会话接线端到端：attach 后 ExtractText/Search 走引擎实数据——
    // 占位几何（全零）被旁路、搜索命中真实文本（命令面/载荷形状不变）。
    let placeholder = PageGeometry::simple(0.0, 0.0, Rotation::Deg0);
    let mut session = session_with_engine(open_small(), placeholder);
    assert!(session.engine_attached(), "引擎在位观测");

    // 几何查询走引擎（占位 0×0 不再可见）。
    let geometry = session.page_geometry(PageIndex(0)).expect("引擎几何");
    assert_eq!((geometry.width_pt, geometry.height_pt), (612.0, 792.0));

    // ExtractText：载荷形状 = 页号升序 + 非空实文本。
    let outcome = session
        .execute(Command::ExtractText {
            target: TextTarget::All,
            source: Source::View,
        })
        .expect("ExtractText 成功");
    let mdc_pdf::command::Outcome::ExtractedText { pages } = outcome else {
        panic!("ExtractText 载荷形状");
    };
    assert_eq!(pages.len(), 3);
    assert!(pages[0].1.contains("Spike fixture page 1 of 3."), "实文本");

    // Search：命中真实文本（引擎路径的收集器复用 = 限额语义同锁）。
    let outcome = session
        .execute(Command::Search {
            query: "Spike fixture".into(),
            opts: SearchOptions {
                case_insensitive: false,
                max_hits: 100,
            },
            source: Source::View,
        })
        .expect("Search 成功");
    let mdc_pdf::command::Outcome::SearchResults { hits, truncated } = outcome else {
        panic!("Search 载荷形状");
    };
    assert_eq!(hits.len(), 3, "逐页各一命中（fixture 每页恰一处标签）");
    assert!(!truncated, "预算内不截断");
    assert_eq!(hits[0].page, PageIndex(0), "命中页关联");
}

#[test]
fn session_search_limit_semantics_unchanged() {
    // 限额语义单一出口：引擎路径 max_hits 钳制 → truncated 告知、空
    // query 硬拒绝——与注入路径（PDF-b 锁）逐字一致，两数据源不漂移。
    let placeholder = PageGeometry::simple(612.0, 792.0, Rotation::Deg0);
    let mut session = session_with_engine(open_small(), placeholder);
    let outcome = session
        .execute(Command::Search {
            query: "fixture".into(),
            opts: SearchOptions {
                case_insensitive: true,
                max_hits: 2,
            },
            source: Source::Ai,
        })
        .expect("Search 成功");
    let mdc_pdf::command::Outcome::SearchResults { hits, truncated } = outcome else {
        panic!("Search 载荷形状");
    };
    assert_eq!(hits.len(), 2, "max_hits=2 钳制（❷收敛继续）");
    assert!(truncated, "第三页命中溢出 → truncated 告知（不静默丢弃）");

    let empty = session.execute(Command::Search {
        query: String::new(),
        opts: SearchOptions {
            case_insensitive: false,
            max_hits: 100,
        },
        source: Source::View,
    });
    assert_eq!(empty, Err(CommandError::EmptyQuery), "空 query ❶硬拒绝");
}

// ---------------------------------------------------------------------------
// 8-11：渲染面（Rasterizer）
// ---------------------------------------------------------------------------

#[test]
fn rasterize_buffer_contract_letter_aspect() {
    // 位图合同：缓冲长 = w*h*4（BGRA）；高 = round(宽 × 792/612)（显示
    // 几何等比——Letter 纵向）。300 → 388（300×1.2941… = 388.23…）。
    let mut engine = open_small();
    let page = engine
        .rasterize(RenderRequest {
            page: PageIndex(0),
            width_px: 300,
        })
        .expect("渲染成功");
    assert_eq!(page.width_px, 300, "报告宽 = 请求宽（预算内）");
    assert_eq!(page.height_px, 388, "Letter 等比高（round(300×792/612)）");
    assert_eq!(
        page.bgra.len(),
        (page.width_px as usize) * (page.height_px as usize) * 4,
        "BGRA 缓冲长契约（语义锁）"
    );
}

#[test]
fn rasterize_paints_nonblank_bitmap() {
    // 渲染真实落笔：pdfium 渲染器确实写过缓冲——非全零 + 白底主导
    // （fixture 为白底单行文本；全零 = FPDF_RenderPageBitmap 未生效）。
    let mut engine = open_small();
    let page = engine
        .rasterize(RenderRequest {
            page: PageIndex(0),
            width_px: 200,
        })
        .expect("渲染成功");
    assert!(page.bgra.iter().any(|&b| b != 0), "非全零（渲染落笔）");
    let white = page.bgra.iter().filter(|&&b| b == 0xFF).count();
    let total = page.bgra.len();
    assert!(
        white * 2 > total,
        "白底主导（0xFF 字节过半；实测 {white}/{total}）——fixture 白底单行文本"
    );
}

#[test]
fn rasterize_s3_clamp_bounds() {
    // S3-L2 ❷钳制（单边 8192 / 总像素 64M）：请求 0 → 收敛 1px；请求
    // 999,999 → 8192 边钳后总像素超预算 → 等比收缩到 8000×8000（收敛
    // 值锁定 = 公式锁定；生效尺寸以报告为准，不假装请求值生效）。
    let mut engine = open_small();
    let zero = engine
        .rasterize(RenderRequest {
            page: PageIndex(0),
            width_px: 0,
        })
        .expect("0 宽请求收敛继续（❷非拒绝）");
    assert_eq!((zero.width_px, zero.height_px), (1, 1), "下界钳制到 1px");
    assert_eq!(zero.bgra.len(), 4, "1×1×4");

    let huge = engine
        .rasterize(RenderRequest {
            page: PageIndex(1),
            width_px: 999_999,
        })
        .expect("超大请求收敛继续（❷非拒绝）");
    assert!(huge.width_px >= 1 && huge.width_px <= 8_192, "单边 ≤ 8192");
    assert!(
        huge.height_px >= 1 && huge.height_px <= 8_192,
        "单边 ≤ 8192"
    );
    assert!(
        u64::from(huge.width_px) * u64::from(huge.height_px) <= 64_000_000,
        "总像素 ≤ 64M"
    );
    assert_eq!(
        (huge.width_px, huge.height_px),
        (8_000, 8_000),
        "钳制收敛值"
    );
    assert_eq!(
        huge.bgra.len(),
        (huge.width_px as usize) * (huge.height_px as usize) * 4,
        "钳后缓冲长契约"
    );
}

#[test]
fn engine_error_surface_binding_and_load() {
    // typed 错误面：显式绑定无效路径 → Engine（消息含路径——可诊断）；
    // 装载垃圾字节 → Engine（pdfium 装载失败不 panic、不 String 错误）。
    let bogus = Path::new("/nonexistent/mdc-pdf-test/libpdfium.so");
    match PdfiumEngine::open_with_library(bogus, fixture_bytes("small.pdf")) {
        Err(CommandError::Engine(detail)) => assert!(
            detail.contains("nonexistent"),
            "错误消息含尝试路径（诊断面）：{detail}"
        ),
        other => panic!("无效路径绑定应 Engine 错误，得 {other:?}"),
    }
    let garbage = b"this is not a pdf, pdfium must reject it".to_vec();
    match PdfiumEngine::open(garbage) {
        Err(CommandError::Engine(_)) => {}
        other => panic!("垃圾字节装载应 Engine 错误，得 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 12-13：线程纪律 + 全链路（S4.1/S4.2 + 缓存命中）
// ---------------------------------------------------------------------------

/// Send 见证（编译期）：thread_safe 特性背书引擎可移入工人线程。
fn require_send<T: Send>() {}

#[test]
fn send_witness_and_worker_roundtrip() {
    // Send 见证 + 真实引擎过渲染工人（S4.1：所有权转移进专职线程，
    // pdfium 调用与文档状态线程隔离）——两作业回执按提交序、id 关联、
    // 缓冲长契约在工人路径同样成立。
    require_send::<PdfiumEngine>();
    let mut worker = RenderWorker::new(open_small());
    let jobs = [
        (
            worker.submit(RenderRequest {
                page: PageIndex(0),
                width_px: 120,
            }),
            120u32,
        ),
        (
            worker.submit(RenderRequest {
                page: PageIndex(2),
                width_px: 160,
            }),
            160u32,
        ),
    ];
    for (job_id, want_width) in jobs {
        match recv_until(&mut worker, job_id) {
            WorkerReply::Done {
                id,
                result: Ok(page),
            } => {
                assert_eq!(id, job_id, "回执 id 关联（提交序）");
                assert_eq!(page.width_px, want_width, "报告宽 = 请求宽");
                assert_eq!(
                    page.bgra.len(),
                    (page.width_px as usize) * (page.height_px as usize) * 4,
                    "工人路径缓冲长契约"
                );
            }
            other => panic!("真实引擎作业应成功，得 {other:?}"),
        }
    }
    assert!(worker.try_recv().is_none(), "恰两回执，无多余");
}

/// 限时收报（阻塞至目标 id 到达——单工人 FIFO 下中间不会插队）。
fn recv_until(worker: &mut RenderWorker<PdfiumEngine>, job_id: u64) -> WorkerReply {
    let start = std::time::Instant::now();
    loop {
        if let Some(reply) = worker.recv_timeout(Duration::from_millis(200)) {
            assert!(
                matches!(&reply, WorkerReply::Done { id, .. } | WorkerReply::Cancelled { id } if *id == job_id),
                "回执 id 关联：期望 {job_id}，得 {reply:?}"
            );
            return reply;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "作业 {job_id} 回执超时（工人疑似死亡/悬挂）"
        );
    }
}

/// 计数光栅化器（全链路锁的调用计数面——包裹真实引擎，mock-free）。
struct CountingRasterizer {
    inner: PdfiumEngine,
    calls: Arc<AtomicUsize>,
}

impl Rasterizer for CountingRasterizer {
    fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.rasterize(request)
    }
}

#[test]
fn full_chain_cache_hit_zero_rasterize() {
    // 全链路（阅读面组合语义的引擎实证）：第二个引擎实例（同字节独立
    // 句柄——双线程形态：会话/工人各持引擎）→ 包计数面 → 工人渲染 →
    // 缓存填充 → 二次取页**零 rasterize 调用**（缓存命中 = 翻页性能
    // 的根保障，此处用真实 pdfium 而非 mock 复证）。
    let engine = open_small();
    let calls = Arc::new(AtomicUsize::new(0));
    let counting = CountingRasterizer {
        inner: engine,
        calls: calls.clone(),
    };
    let mut worker = RenderWorker::new(counting);
    let mut cache: PageCache<(u32, u32), RenderedPage> = PageCache::new(1 << 24); // 16 MiB

    let key = (1u32, 500u32);
    let id = worker.submit(RenderRequest {
        page: PageIndex(key.0),
        width_px: key.1,
    });
    let page = match recv_until_counting(&mut worker, id, &calls) {
        WorkerReply::Done {
            result: Ok(page), ..
        } => page,
        other => panic!("真实引擎渲染应成功，得 {other:?}"),
    };
    let byte_cost = page.bgra.len();
    assert_eq!(
        byte_cost,
        (500 * 647) * 4,
        "500 宽 Letter 等比高 647（round(500×792/612)）"
    );
    assert!(cache.put(key, page, byte_cost).is_empty(), "预算内无驱逐");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "填充期恰一次渲染");

    // 二次取页：缓存命中（零 rasterize——真实引擎调用计数不增）。
    let hit = cache.get(&key).expect("二次取页必命中");
    assert_eq!((hit.width_px, hit.height_px), (500, 647));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "缓存命中零 rasterize 调用（全链路性能根保障）"
    );
}

/// 计数工人的限时收报（同 recv_until，泛型面不同不复用）。
fn recv_until_counting(
    worker: &mut RenderWorker<CountingRasterizer>,
    job_id: u64,
    _calls: &Arc<AtomicUsize>,
) -> WorkerReply {
    let start = std::time::Instant::now();
    loop {
        if let Some(reply) = worker.recv_timeout(Duration::from_millis(200)) {
            return reply;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "作业 {job_id} 回执超时"
        );
    }
}
