//! PDF-c — 性能实测面（feature `engine-pdfium` + `#[ignore]`：
//! `cargo test -p mdc-pdf --features engine-pdfium --test pdfc_perf -- --ignored --nocapture`）。
//!
//! 载荷 = large.pdf（500 页，spike fixtures）；环境合同与 pdfc_engine.rs
//! 相同（库/fixture 定位失败 = panic 打印候选，不静默 skip）。
//!
//! **阈值纪律**：断言阈值 = 实测值 × 3（gates.md PDF-c 实测性能表为
//! 权威——阈值常量集中在 [`THRESHOLDS`]，随实测更新；× 3 裕度覆盖
//! 沙箱负载波动，防性能面回归放行（阈值过松）与误报（过紧））。
//!
//! 计时口径注记：本套件为**测试 profile（debug）**——pdfium 侧为原生
//! C++（release 品质，不受宿主编译 profile 影响）；Rust 侧热路径 =
//! 缓冲清零/memcpy/通道往返（memset 类原语，profile 敏感度低）。渲染
//! 主导成本在 pdfium 内部，故 debug 轮数字对渲染面有代表性；Rust 侧
//! 逻辑无逐像素处理（零 image 依赖的直接收益）。

// feature 门（默认构建零引擎依赖——本文件整体退化为空目标，跨环境
// 可构建纪律保持）。
#![cfg(feature = "engine-pdfium")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mdc_pdf::cache::PageCache;
use mdc_pdf::command::{Command, Source, TextTarget};
use mdc_pdf::command::{CommandError, SearchOptions};
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageCount, PageIndex, PageSpan};
use mdc_pdf::render::{Rasterizer, RenderRequest, RenderedPage};
use mdc_pdf::worker::{RenderWorker, WorkerReply};

/// 性能阈值（= 实测 × 3，见文件头阈值纪律；实测值见下方注释与
/// gates.md PDF-c 实测性能表）。
struct Thresholds {
    /// large.pdf 打开（含装载解析）中位耗时上限（ms）。
    open_ms: u64,
    /// 首页 1000px 渲染耗时上限（ms）。
    first_render_ms: u64,
    /// 顺序渲染 50 页（800px）p95 单页耗时上限（ms）。
    seq_render_p95_ms: u64,
    /// 命中路径中位耗时上限（µs）——顺序翻页 LRU 命中面。
    hit_median_us: u64,
    /// 命中中位 < 未命中中位 / 该倍数（缓存收益下限）。
    hit_vs_miss_divisor: u32,
    /// 逐页文本抽取 500 页总耗时上限（ms）。
    text_all_ms: u64,
}

/// 实测值（本沙箱 2026-10-10，debug 测试 profile；pdfium 原生侧不受
/// profile 影响，见文件头口径注记）：
/// - 打开 TTI 中位 0.18ms（max 3.41ms = 首次装载抖动，断言面 = 中位）；
/// - 首页 1000px 渲染 8.19ms；顺序 50 页 800px p50 0.91 / p95 1.09ms；
/// - 顺序翻页（两阶段 0..20）：未命中中位 2.14ms / 命中中位 5.15µs（415× 差距）；
/// - 逐页文本抽取 500 页全量 30.32ms（≈16,493 页/s）。
///
/// 阈值 = 实测 × 3 后向上对齐到稳定数（1/2/5 序列——防沙箱负载抖动
/// 误报；防回归下限仍由 ×3 保障）。
const THRESHOLDS: Thresholds = Thresholds {
    open_ms: 1,
    first_render_ms: 25,
    seq_render_p95_ms: 5,
    hit_median_us: 20,
    hit_vs_miss_divisor: 10,
    text_all_ms: 100,
};

// ---------------------------------------------------------------------------
// 环境合同（与 pdfc_engine.rs 同构——性能面同不静默 skip）
// ---------------------------------------------------------------------------

fn fixture_candidates() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // crate 本地优先 / spike 兜底（顺序依据见 pdfc_engine.rs——T1 审计 F12）。
    vec![
        manifest.join("fixtures"),
        manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/fixtures"),
    ]
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let mut tried: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("MDC_PDF_TEST_FIXTURES") {
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
    panic!("fixture {name} 定位失败；已尝试：{}", tried.join("；"));
}

fn open_large() -> PdfiumEngine {
    match PdfiumEngine::open(fixture_bytes("large.pdf")) {
        Ok(engine) => engine,
        Err(CommandError::Engine(detail)) => panic!("pdfium 打开 large.pdf 失败：{detail}"),
        Err(other) => panic!("打开 large.pdf 应成功或 Engine 错误，得 {other:?}"),
    }
}

/// 计数光栅化器（命中/未命中面的调用计数）。
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

/// 中位数（升序取中——n 偶数取下中位，报告口径稳定）。
fn median_ns(samples: &[Duration]) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

/// p 分位（0 < p <= 1；升序取 ceil(p*n)-1）。
fn percentile_ns(samples: &[Duration], p: f64) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort();
    let idx = ((p * sorted.len() as f64).ceil() as usize - 1).min(sorted.len() - 1);
    sorted[idx]
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn us(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

// ---------------------------------------------------------------------------
// 指标 1：打开 TTI（500 页）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "性能实测（--ignored --nocapture 运行）"]
fn perf_open_tti_500p() {
    // 口径：绑定成本已由首次 open 支付（进程单例）——本指标量的是
    // **文档装载解析**（load_pdf_from_byte_vec 全内存路径）。5 次取中位。
    let bytes = fixture_bytes("large.pdf");
    let warmup = PdfiumEngine::open(fixture_bytes("small.pdf"));
    drop(warmup); // 支付绑定 + FPDF_InitLibrary 首次成本
    let mut runs: Vec<Duration> = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let engine = PdfiumEngine::open(bytes.clone()).expect("打开");
        let elapsed = t.elapsed();
        assert_eq!(engine.page_count(), 500, "500 页基线");
        drop(engine);
        runs.push(elapsed);
    }
    let med = median_ns(&runs);
    println!(
        "| 打开 TTI（500 页，5 次取中位）| min {:.2}ms / 中位 {:.2}ms / max {:.2}ms | 阈值 {}ms |",
        ms(runs.iter().min().copied().unwrap()),
        ms(med),
        ms(runs.iter().max().copied().unwrap()),
        THRESHOLDS.open_ms
    );
    assert!(
        med < Duration::from_millis(THRESHOLDS.open_ms),
        "打开中位 {:.2}ms 超阈值 {}ms",
        ms(med),
        THRESHOLDS.open_ms
    );
}

// ---------------------------------------------------------------------------
// 指标 2：首页渲染（1000px，冷工人）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "性能实测（--ignored --nocapture 运行）"]
fn perf_first_page_render() {
    let engine = open_large();
    let mut worker = RenderWorker::new(engine);
    let t = Instant::now();
    let id = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 1000,
    });
    // 单收报：首页渲染冷启动上限远小于回执超时（30s vs 实测 ~10ms）
    // ——超时即工人异常，无需重试环。
    let page = match worker.recv_timeout(Duration::from_secs(30)) {
        Some(WorkerReply::Done {
            id: rid,
            result: Ok(page),
        }) if rid == id => page,
        Some(other) => panic!("首页渲染应成功，得 {other:?}"),
        None => panic!("首页渲染回执超时（工人疑似死亡/悬挂）"),
    };
    let elapsed = t.elapsed();
    println!(
        "| 首页渲染（1000px，冷工人）| {}×{} | {:.2}ms | 阈值 {}ms |",
        page.width_px,
        page.height_px,
        ms(elapsed),
        THRESHOLDS.first_render_ms
    );
    assert!(
        elapsed < Duration::from_millis(THRESHOLDS.first_render_ms),
        "首页渲染 {:.2}ms 超阈值 {}ms",
        ms(elapsed),
        THRESHOLDS.first_render_ms
    );
}

// ---------------------------------------------------------------------------
// 指标 3：顺序渲染 50 页 p50/p95（800px）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "性能实测（--ignored --nocapture 运行）"]
fn perf_sequential_render_50p() {
    let engine = open_large();
    let mut worker = RenderWorker::new(engine);
    // 预热：首页（工人体感与分配器热身）不进样本。
    let warm = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 800,
    });
    let _ = await_reply(&mut worker, warm);

    let mut samples: Vec<Duration> = Vec::with_capacity(50);
    for p in 1..=50u32 {
        let t = Instant::now();
        let id = worker.submit(RenderRequest {
            page: PageIndex(p),
            width_px: 800,
        });
        let page = await_reply(&mut worker, id);
        let elapsed = t.elapsed();
        assert_eq!(page.width_px, 800);
        samples.push(elapsed);
    }
    let p50 = percentile_ns(&samples, 0.50);
    let p95 = percentile_ns(&samples, 0.95);
    println!(
        "| 顺序渲染 50 页（800px）| p50 {:.2}ms / p95 {:.2}ms | p95 阈值 {}ms |",
        ms(p50),
        ms(p95),
        THRESHOLDS.seq_render_p95_ms
    );
    assert!(
        p95 < Duration::from_millis(THRESHOLDS.seq_render_p95_ms),
        "顺序渲染 p95 {:.2}ms 超阈值 {}ms",
        ms(p95),
        THRESHOLDS.seq_render_p95_ms
    );
}

/// 等待指定 id 回执（渲染成功形态）。
fn await_reply(worker: &mut RenderWorker<PdfiumEngine>, job_id: u64) -> RenderedPage {
    let start = Instant::now();
    loop {
        if let Some(reply) = worker.recv_timeout(Duration::from_millis(200)) {
            match reply {
                WorkerReply::Done {
                    id,
                    result: Ok(page),
                } if id == job_id => return page,
                other => panic!("作业 {job_id} 应成功，得 {other:?}"),
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "作业 {job_id} 回执超时"
        );
    }
}

// ---------------------------------------------------------------------------
// 指标 4：顺序翻页 LRU 命中 vs 未命中（pdf-c-1r 审计正名：两阶段顺序取页，非随机序列）
// ---------------------------------------------------------------------------

/// get_or_render：命中路径（纯缓存 get——计时不含返回值 clone）/ 未命中
/// 路径（工人渲染 + 入缓，计时含 put 的拷贝——两侧均为主线程真实成本）。
/// 返回 (页产物, 是否命中, 耗时)。
fn get_or_render(
    cache: &mut PageCache<(u32, u32), RenderedPage>,
    worker: &mut RenderWorker<CountingRasterizer>,
    key: (u32, u32),
) -> (RenderedPage, bool, Duration) {
    let t = Instant::now();
    if let Some(page) = cache.get(&key) {
        let elapsed = t.elapsed();
        return (page.clone(), true, elapsed);
    }
    let id = worker.submit(RenderRequest {
        page: PageIndex(key.0),
        width_px: key.1,
    });
    let page = match worker.recv_timeout(Duration::from_millis(200)) {
        Some(WorkerReply::Done {
            id: rid,
            result: Ok(page),
        }) if rid == id => page,
        other => panic!("翻页渲染应成功，得 {other:?}"),
    };
    let cost = page.bgra.len();
    cache.put(key, page.clone(), cost);
    let elapsed = t.elapsed();
    (page, false, elapsed)
}

#[test]
#[ignore = "性能实测（--ignored --nocapture 运行）"]
fn perf_sequential_flip_lru_hit_vs_miss() {
    // 载荷形态：阅读面 800px 翻页（Letter 高 1035px，3.16MiB/页）。
    // 缓存预算 = 20 页整（恰好装满——预算内零驱逐，两阶段不互相污染）。
    // 阶段 A（未命中）：0..20 顺序首访 = 20 次真实渲染；
    // 阶段 B（命中）：同 20 页立即回访 = 20 次纯缓存命中。
    const WIDTH: u32 = 800;
    const PAGES: u32 = 20;
    let per_page = (WIDTH as usize) * 1035 * 4; // 800×1035×4 = 3,312,000B
    let engine = open_large();
    let calls = Arc::new(AtomicUsize::new(0));
    let counting = CountingRasterizer {
        inner: engine,
        calls: calls.clone(),
    };
    let mut worker = RenderWorker::new(counting);
    let mut cache: PageCache<(u32, u32), RenderedPage> = PageCache::new(per_page * PAGES as usize);

    let mut miss_durations: Vec<Duration> = Vec::new();
    let mut hit_durations: Vec<Duration> = Vec::new();
    for p in 0..PAGES {
        let (_, is_hit, elapsed) = get_or_render(&mut cache, &mut worker, (p, WIDTH));
        assert!(!is_hit, "阶段 A 全未命中（首访）");
        miss_durations.push(elapsed);
    }
    for p in 0..PAGES {
        let (_, is_hit, elapsed) = get_or_render(&mut cache, &mut worker, (p, WIDTH));
        assert!(is_hit, "阶段 B 全命中（预算内零驱逐）");
        hit_durations.push(elapsed);
    }
    let miss_med = median_ns(&miss_durations);
    let hit_med = median_ns(&hit_durations);
    println!(
        "| 顺序翻页（800px×20 页）| 未命中中位 {:.2}ms / 命中中位 {:.2}µs | 命中阈值 {}µs 且 < 未命中/{} |",
        ms(miss_med),
        us(hit_med),
        THRESHOLDS.hit_median_us,
        THRESHOLDS.hit_vs_miss_divisor
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        PAGES as usize,
        "rasterize 总调用 = 20（阶段 B 零渲染——缓存命中根保障）"
    );
    assert!(
        hit_med < Duration::from_micros(THRESHOLDS.hit_median_us),
        "命中中位 {:.2}µs 超阈值 {}µs",
        us(hit_med),
        THRESHOLDS.hit_median_us
    );
    assert!(
        hit_med * THRESHOLDS.hit_vs_miss_divisor < miss_med,
        "命中中位（{:.2}µs）应低于未命中中位（{:.2}ms）/{}",
        us(hit_med),
        ms(miss_med),
        THRESHOLDS.hit_vs_miss_divisor
    );
}

// ---------------------------------------------------------------------------
// 指标 5：逐页文本抽取吞吐（500 页）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "性能实测（--ignored --nocapture 运行）"]
fn perf_text_extraction_throughput() {
    let engine = open_large();
    // 预热：首页抽取（字体/编码表装载）不进样本。
    let warm = PageSpan::new(PageIndex(0), PageIndex(0)).expect("单页");
    let _ = engine.extract_text(warm).expect("预热");

    let t = Instant::now();
    let mut total_chars = 0usize;
    for p in 0..500u32 {
        let span = PageSpan::new(PageIndex(p), PageIndex(p)).expect("单页");
        let pages = engine.extract_text(span).expect("抽取");
        total_chars += pages.first().map(|(_, s)| s.len()).unwrap_or(0);
    }
    let elapsed = t.elapsed();
    let throughput = 500.0 / elapsed.as_secs_f64();
    println!(
        "| 逐页文本抽取（500 页）| {} 页 / {:.2}ms（{:.0} 页/s，Σ{} 字节）| 阈值 {}ms |",
        500,
        ms(elapsed),
        throughput,
        total_chars,
        THRESHOLDS.text_all_ms
    );
    assert!(
        elapsed < Duration::from_millis(THRESHOLDS.text_all_ms),
        "文本抽取总耗 {:.2}ms 超阈值 {}ms",
        ms(elapsed),
        THRESHOLDS.text_all_ms
    );
    assert!(total_chars > 0, "抽取总字节非零（吞吐指标有载荷）");
}

// ---------------------------------------------------------------------------
// 会话全链路端到端计时（阅读面组合面的冒烟——非门指标，进 gates 记录）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "性能实测（--ignored --nocapture 运行）"]
fn perf_session_e2e_smoke() {
    // 500 页会话：attach 引擎 → 全档 ExtractText → 全档 Search（命中
    // 逐页收集）——阅读面打开后的首次文本化成本（宿主首屏可用的上界）。
    let engine = open_large();
    let count = engine.page_count();
    let placeholder = mdc_pdf::page::PageGeometry {
        width_pt: 0.0,
        height_pt: 0.0,
        rotation: mdc_pdf::page::Rotation::Deg0,
    };
    let mut session = PdfSession::open(
        DocumentMeta {
            id: PdfDocumentId(7),
            file_name: "large.pdf".into(),
            page_count: PageCount(count),
        },
        Default::default(),
        vec![placeholder; count as usize],
        OutlineTree::new(),
    )
    .expect("会话（占位几何与页数自洽）");
    session
        .attach_engine(Box::new(engine))
        .expect("引擎页数与元数据一致（同源构造——T1 审计 O3 装配期校验）");

    let t = Instant::now();
    let outcome = session
        .execute(Command::ExtractText {
            target: TextTarget::All,
            source: Source::System,
        })
        .expect("ExtractText");
    let mdc_pdf::command::Outcome::ExtractedText { pages } = outcome else {
        panic!("载荷形状");
    };
    let extract_ms = t.elapsed();
    assert_eq!(pages.len(), 500);

    let t = Instant::now();
    let outcome = session
        .execute(Command::Search {
            query: "Spike fixture page 250".into(),
            opts: SearchOptions {
                case_insensitive: false,
                max_hits: 100,
            },
            source: Source::View,
        })
        .expect("Search");
    let mdc_pdf::command::Outcome::SearchResults { hits, truncated } = outcome else {
        panic!("载荷形状");
    };
    let search_ms = t.elapsed();
    assert_eq!(hits.len(), 1, "恰页 250 一处（全档流式扫到即停）");
    assert!(!truncated);
    println!(
        "| 会话端到端（500 页）| 全档 ExtractText {:.2}ms / 定点 Search(页 250 标签) {:.2}ms | 冒烟记录（无阈值）|",
        ms(extract_ms),
        ms(search_ms)
    );
}
