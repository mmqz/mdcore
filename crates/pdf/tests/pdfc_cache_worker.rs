//! PDF-c — LRU 页缓存 + 渲染工人语义锁（常开：零引擎依赖，mock 可测；
//! pdf_carrier_plan §5 PDF-c + SECURITY_POLICY.md S3-L2/S4.1/S4.2）。
//!
//! 锁清单（每锁一条 `#[test]`，断言消息讲清**为什么**）：
//! 1. `lru_get_promotes_recency`——命中即提权：A B C → get(A) → put(D)
//!    驱逐 B（而非 A）——LRU 序是缓存正确性的根，错了就是"驱逐最热页"；
//! 2. `lru_eviction_order_is_lru_first`——驱逐顺序：超预算从尾逐起，
//!    evicted 集合序 = LRU→MRU（调用方按序作废关联资源）；
//! 3. `lru_byte_budget_is_exact`——字节预算精确性：bytes() == Σ生效成本、
//!    超预算后 bytes() <= 预算且恰等于保留条目和（预算 = 字节非条目数）；
//! 4. `lru_duplicate_put_updates_in_place`——重复 put 原位更新不重复
//!    计费：len 不变、bytes 只计新值、值替换、且提权；
//! 5. `lru_miss_and_contains_do_not_promote`——未命中 get 与 contains
//!    观测不提权（纯查询不得扰动驱逐序——否则缓存出现隐式冷启动）；
//! 6. `lru_oversized_entry_self_evicts`——超大条目自逐：单条成本 > 预算
//!    时自身作为最后一个"最旧"条目被逐出（预算不变量保持，不静默
//!    超载）；零成本条目不受预算驱逐；
//! 7. `lru_clear_resets_all_counters`——clear 全量复位（len/bytes/entries
//!    归零，值即刻释放——预算的意义就是内存上限）；
//! 8. `lru_entries_iterate_lru_to_mru`——entries() 观测序 = LRU→MRU
//!    （驱逐候选序，观测面与驱逐面同序才能推理缓存行为）；
//! 9. `worker_sequential_roundtrip_in_order`——顺序作业往返：两作业
//!    回执按提交序、id 关联回执（回执合同——单工人 FIFO）；
//! 10. `worker_panic_isolated_and_recovers`——panic 隔离：作业 panic →
//!     `CommandError::RenderPanic` 回执（进程不崩），后续作业仍可用
//!     （一坏页不毒全档——S4.2）；
//! 11. `worker_cancel_skips_queued_job`——取消令牌：排队中（未开跑）
//!     作业被取消 → Cancelled 回执 + 零 rasterize 调用（❸弃置：不假装
//!     能杀正在跑的线程，取消只作用于排队面）；
//! 12. `worker_shutdown_joins_promptly`——停机不悬挂：drop 发停机信号
//!     并 join（drop 返回即线程回收完成）；submit 后立即 drop 的在途
//!     路径同样不悬挂；
//! 13. `mock_end_to_end_cache_fill_and_hit`——mock 端到端：异步渲染 →
//!     缓存填充 → 二次取页**零 rasterize 调用**（缓存命中语义——阅读面
//!     翻页性能的根保障）；
//! 14. `boxed_rasterizer_object_form`——`Box<dyn Rasterizer + Send>`
//!     装箱形态可用（S4 对象引用形态——宿主以 trait 对象装配管线）；
//! 15. `render_types_serde_roundtrip`——RenderRequest/RenderedPage
//!     serde 往返（协议草案锚——载荷形状先行冻结）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mdc_pdf::cache::PageCache;
use mdc_pdf::command::CommandError;
use mdc_pdf::page::PageIndex;
use mdc_pdf::render::{Rasterizer, RenderRequest, RenderedPage};
use mdc_pdf::worker::{RenderWorker, WorkerReply};

// ---------------------------------------------------------------------------
// mock 光栅化器（常开测试面——零引擎依赖下锁渲染/缓存语义）
// ---------------------------------------------------------------------------

/// mock：计数器（跨线程观测）+ 可配置 panic 页 + 可配置延迟 + 固定
/// 纵横比 3:4 输出。
#[derive(Clone)]
struct MockRasterizer {
    calls: Arc<AtomicUsize>,
    /// panic 页（None = 不 panic；触发一次后自动失效——panic 隔离锁
    /// 需要后续作业正常执行）。
    panic_on: Arc<std::sync::Mutex<Option<u32>>>,
    delay: Duration,
}

impl MockRasterizer {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            panic_on: Arc::new(std::sync::Mutex::new(None)),
            delay: Duration::ZERO,
        }
    }

    fn with_delay(delay: Duration) -> Self {
        Self {
            delay,
            ..Self::new()
        }
    }

    /// 计数句柄（worker 移走 mock 后仍可观测——跨线程共享计数面）。
    fn calls(&self) -> Arc<AtomicUsize> {
        self.calls.clone()
    }
}

impl Rasterizer for MockRasterizer {
    fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.delay > Duration::ZERO {
            std::thread::sleep(self.delay);
        }
        // panic 页触发（单发雷）。**锁纪律**：读取与清除分两条短作用域
        // 语句——std Mutex 非重入，`if let` 条件中的临时 guard 存活至块尾
        // （Rust 2021 临时生命周期延展），块内二次 lock = 自锁死锁；且
        // guard 跨 panic 释放会毒化 mutex（后续作业连锁 panic）。短锁 +
        // 局部副本 = panic 不持锁、不毒化。
        let panic_page = *self.panic_on.lock().unwrap();
        if let Some(page) = panic_page {
            if page == request.page.0 {
                *self.panic_on.lock().unwrap() = None; // 单发雷清除
                panic!("mock rasterizer 受控 panic（页 {page}）");
            }
        }
        // 3:4 纵横比，标记字节 = 页号——产物可溯源。
        let width = request.width_px;
        let height = width * 4 / 3;
        Ok(RenderedPage {
            width_px: width,
            height_px: height,
            bgra: vec![(request.page.0 as u8).wrapping_add(1); (width * height * 4) as usize],
        })
    }
}

/// 等待回执（顺序收取直到拿到目标 id——工人回执 FIFO，目标必达）。
fn await_reply(worker: &mut RenderWorker<MockRasterizer>, job_id: u64) -> WorkerReply {
    let deadline = Duration::from_secs(10);
    let start = std::time::Instant::now();
    loop {
        if let Some(reply) = worker.recv_timeout(Duration::from_millis(50)) {
            assert!(
                matches!(&reply, WorkerReply::Done { id, .. } | WorkerReply::Cancelled { id } if *id == job_id),
                "回执 id 关联：期望 {job_id}，得 {reply:?}"
            );
            return reply;
        }
        assert!(
            start.elapsed() < deadline,
            "作业 {job_id} 回执超时（工人疑似死亡/悬挂）"
        );
    }
}

// ---------------------------------------------------------------------------
// 1-8：LRU 缓存语义
// ---------------------------------------------------------------------------

#[test]
fn lru_get_promotes_recency() {
    // 预算 3 条（每条成本 10）；A B C 满载。get(A) 命中提权 → LRU 变 B
    // → put(D) 应驱逐 B。若 get 不提权，会驱逐 A（最热页）——语义反转。
    let mut cache: PageCache<u32, &str> = PageCache::new(30);
    assert!(cache.put(1, "A", 10).is_empty());
    assert!(cache.put(2, "B", 10).is_empty());
    assert!(cache.put(3, "C", 10).is_empty());
    assert_eq!(cache.get(&1), Some(&"A"), "命中返回值");
    let evicted = cache.put(4, "D", 10);
    assert_eq!(
        evicted,
        vec![2],
        "get(1) 提权后 LRU = B（键 2）——驱逐最久未用"
    );
    assert!(cache.contains(&1) && cache.contains(&3) && cache.contains(&4));
    assert!(!cache.contains(&2), "被驱逐键不在位");
}

#[test]
fn lru_eviction_order_is_lru_first() {
    // 预算 20（两条件 10）：put 三条 → 依 LRU 序驱逐 1 再 2；evicted
    // 集合顺序 = 驱逐顺序（调用方按序作废关联资源——顺序是合同）。
    let mut cache: PageCache<u32, &str> = PageCache::new(20);
    cache.put(1, "A", 10);
    cache.put(2, "B", 10);
    let evicted = cache.put(3, "C", 10);
    assert_eq!(evicted, vec![1], "超预算先逐最旧（1）");
    let evicted = cache.put(4, "D", 10);
    assert_eq!(evicted, vec![2], "再逐次旧（2）");
    assert_eq!(cache.len(), 2, "预算内恰两条件");
}

#[test]
fn lru_byte_budget_is_exact() {
    // 字节预算精确性：bytes == Σ生效成本；超预算后 bytes <= 预算且
    // 等于保留条目和（预算 = 字节数非条目数——S3 精神：以真实内存计）。
    let mut cache: PageCache<u32, Vec<u8>> = PageCache::new(100);
    cache.put(1, vec![0; 30], 30);
    cache.put(2, vec![0; 30], 30);
    assert_eq!(cache.bytes(), 60, "两条件成本和");
    cache.put(3, vec![0; 50], 50); // 60+50=110 > 100 → 逐 1（30）→ 80 ≤ 100
    assert_eq!(cache.bytes(), 80, "驱逐后恰等于保留和（30+50）");
    assert!(cache.bytes() <= cache.byte_budget(), "预算不变量");
    assert!(!cache.contains(&1), "被逐条目");
    // 非均匀成本：两条小 + 一条大——大条目置入只逐一条小（字节视角）。
    let mut cache: PageCache<u32, &str> = PageCache::new(100);
    cache.put(1, "a", 40);
    cache.put(2, "b", 40);
    let evicted = cache.put(3, "c", 70); // 80+70=150 → 逐 1（40）→ 110 → 逐 2（40）→ 70
    assert_eq!(evicted, vec![1, 2], "按字节逐到预算内（LRU 序）");
    assert_eq!(cache.bytes(), 70);
    assert_eq!(cache.len(), 1);
}

#[test]
fn lru_duplicate_put_updates_in_place() {
    // 重复 put：原位更新——len 不变（不双条目）、bytes 只计新值（先退
    // 旧成本再计新——不重复计费）、值替换、且提权到 MRU。
    // 预算 45：更新后 bytes=40 ≤ 45 不驱逐；put(D) 后 50 > 45 触发
    // 驱逐（预算内/外的两侧边界都走到——恰好贴线时边界条件才被锁住）。
    let mut cache: PageCache<u32, &str> = PageCache::new(45);
    cache.put(1, "A", 10);
    cache.put(2, "B", 10);
    cache.put(3, "C", 10);
    assert!(cache.put(1, "A2", 20).is_empty(), "更新后 40 ≤ 45 不驱逐");
    assert_eq!(cache.len(), 3, "同键更新不产生新条目");
    assert_eq!(cache.bytes(), 40, "10-10+20 = 40（旧成本退出，非叠加）");
    assert_eq!(cache.get(&1), Some(&"A2"), "值替换生效");
    // 提权验证：更新后 1 是 MRU → put(D) 超预算逐 LRU（2）。
    let evicted = cache.put(4, "D", 10);
    assert_eq!(evicted, vec![2], "更新提权后 LRU = 2（若未提权会逐 1）");
    assert_eq!(cache.bytes(), 40, "驱逐后 40 ≤ 45（预算不变量）");
    assert!(cache.contains(&1), "更新键保留");
}

#[test]
fn lru_miss_and_contains_do_not_promote() {
    // 未命中 get 与 contains 观测不提权——纯查询扰动驱逐序 = 缓存
    // 出现不可推理的隐式冷启动（性能面语义锁）。
    let mut cache: PageCache<u32, &str> = PageCache::new(30);
    cache.put(1, "A", 10);
    cache.put(2, "B", 10);
    cache.put(3, "C", 10);
    assert_eq!(cache.get(&99), None, "未命中");
    assert!(cache.contains(&1), "contains 命中（观测）");
    let evicted = cache.put(4, "D", 10);
    assert_eq!(evicted, vec![1], "miss/contains 均不提权——LRU 仍是 1");
    assert_eq!(cache.entries().last().map(|(k, _)| *k), Some(4), "MRU = 4");
}

#[test]
fn lru_oversized_entry_self_evicts() {
    // 超大条目（成本 > 预算）：其余条目先逐、自身最后自逐——预算不变量
    // bytes <= budget 保持成立，不静默超载。零成本条目不受预算驱逐。
    let mut cache: PageCache<u32, &str> = PageCache::new(10);
    cache.put(1, "A", 5);
    cache.put(2, "B", 5);
    let evicted = cache.put(3, "HUGE", 100);
    assert_eq!(
        evicted,
        vec![1, 2, 3],
        "先逐他条再自逐（自身键在驱逐集末尾）"
    );
    assert_eq!(cache.len(), 0, "超大条目不驻留");
    assert_eq!(cache.bytes(), 0);
    // 零成本条目：不占预算——零预算缓存仍可存（成本视角的一致推论）。
    let mut zero: PageCache<u32, &str> = PageCache::new(0);
    assert!(zero.put(9, "FREE", 0).is_empty());
    assert!(zero.contains(&9));
    assert_eq!(zero.bytes(), 0);
}

#[test]
fn lru_clear_resets_all_counters() {
    // clear 全量复位：len/bytes/entries 归零；后续 put 正常（缓存可
    // 复用——clear 是内存回收动作而非一次性）。
    let mut cache: PageCache<u32, Vec<u8>> = PageCache::new(100);
    cache.put(1, vec![1; 40], 40);
    cache.put(2, vec![2; 40], 40);
    assert_eq!(cache.bytes(), 80);
    cache.clear();
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.bytes(), 0);
    assert!(cache.entries().is_empty());
    assert_eq!(cache.byte_budget(), 100, "预算保持构造值（clear 不改预算）");
    assert!(cache.put(3, vec![3; 10], 10).is_empty());
    assert_eq!(cache.get(&3).map(|v| v.len()), Some(10), "clear 后可用");
}

#[test]
fn lru_entries_iterate_lru_to_mru() {
    // entries() 序 = LRU→MRU（驱逐候选序）：与驱逐面同序，观测才能
    // 推理缓存行为（哪个键将是下一个被逐）。
    let mut cache: PageCache<u32, &str> = PageCache::new(100);
    cache.put(1, "A", 10);
    cache.put(2, "B", 10);
    cache.put(3, "C", 10);
    cache.get(&1); // 提权 1 → 序变 2, 3, 1
    let order: Vec<u32> = cache.entries().into_iter().map(|(k, _)| k).collect();
    assert_eq!(order, vec![2, 3, 1], "LRU→MRU：驱逐候选序");
}

// ---------------------------------------------------------------------------
// 9-12：渲染工人语义
// ---------------------------------------------------------------------------

#[test]
fn worker_sequential_roundtrip_in_order() {
    // 顺序作业往返：回执按提交序、id 关联回执（单工人 FIFO——回执
    // 合同是宿主侧渲染管线装配的前提）。
    let mock = MockRasterizer::new();
    let calls = mock.calls();
    let mut worker = RenderWorker::new(mock);
    let id_a = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 60,
    });
    let id_b = worker.submit(RenderRequest {
        page: PageIndex(1),
        width_px: 60,
    });
    assert_eq!((id_a, id_b), (0, 1), "作业 id 单调分配");
    let reply_a = await_reply(&mut worker, id_a);
    let reply_b = await_reply(&mut worker, id_b);
    match (reply_a, reply_b) {
        (
            WorkerReply::Done {
                result: Ok(page_a), ..
            },
            WorkerReply::Done {
                result: Ok(page_b), ..
            },
        ) => {
            // mock 产物可溯源：标记字节 = 页号 + 1。
            assert_eq!(page_a.bgra.first(), Some(&1), "页 0 产物");
            assert_eq!(page_b.bgra.first(), Some(&2), "页 1 产物");
            assert_eq!(page_a.height_px, page_a.width_px * 4 / 3, "mock 纵横比");
        }
        other => panic!("两作业应顺序成功，得 {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2, "恰两次渲染调用");
}

#[test]
fn worker_panic_isolated_and_recovers() {
    // panic 隔离（S4.2）：作业 panic → RenderPanic typed 回执（进程不
    // 崩——本测试能继续即证明）；**后续作业仍可用**（工人线程存活，
    // 一坏页不毒全档）。
    let mock = MockRasterizer::new();
    *mock.panic_on.lock().unwrap() = Some(5); // 页 5 单发雷
    let calls = mock.calls();
    let mut worker = RenderWorker::new(mock);
    let id_boom = worker.submit(RenderRequest {
        page: PageIndex(5),
        width_px: 30,
    });
    match await_reply(&mut worker, id_boom) {
        WorkerReply::Done {
            result: Err(CommandError::RenderPanic),
            ..
        } => {}
        other => panic!("panic 作业应回执 RenderPanic，得 {other:?}"),
    }
    // 后续作业正常（工人未死——隔离的意义）。
    let id_ok = worker.submit(RenderRequest {
        page: PageIndex(6),
        width_px: 30,
    });
    match await_reply(&mut worker, id_ok) {
        WorkerReply::Done {
            result: Ok(page), ..
        } => assert_eq!(page.bgra.first(), Some(&7), "panic 后作业正常"),
        other => panic!("panic 后续作业应成功，得 {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2, "两次调用都进入 mock");
}

#[test]
fn worker_cancel_skips_queued_job() {
    // 取消令牌（❸弃置）：取消只作用于**尚未开跑**的排队作业——长作业
    // A 在跑时排队的 B 被取消 → B 回执 Cancelled 且零 rasterize 调用。
    // （已开跑作业不可中断是诚实边界——不假装能杀线程。）
    let mock = MockRasterizer::with_delay(Duration::from_millis(120));
    let calls = mock.calls();
    let mut worker = RenderWorker::new(mock);
    let id_a = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 30,
    }); // 长作业占住工人
    let id_b = worker.submit(RenderRequest {
        page: PageIndex(1),
        width_px: 30,
    }); // B 排队
    worker.cancel(id_b); // 取消排队中的 B
    match await_reply(&mut worker, id_a) {
        WorkerReply::Done { result: Ok(_), .. } => {}
        other => panic!("作业 A 不受取消影响，得 {other:?}"),
    }
    match await_reply(&mut worker, id_b) {
        WorkerReply::Cancelled { id } => assert_eq!(id, id_b),
        other => panic!("排队作业 B 应被取消（零执行），得 {other:?}"),
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "B 被跳过——恰一次渲染调用（A）"
    );
}

#[test]
fn worker_shutdown_joins_promptly() {
    // 停机不悬挂：drop 发停机信号并 join 渲染线程——drop 返回即线程
    // 已回收。含 submit 后立即 drop 的在途路径（作业未回执即停机——
    // drop 不等待在跑/排队作业完成以外的任何东西）。
    let start = std::time::Instant::now();
    {
        let mock = MockRasterizer::new();
        let mut worker = RenderWorker::new(mock);
        worker.submit(RenderRequest {
            page: PageIndex(0),
            width_px: 30,
        });
        // 立即 drop：在途作业 + 停机信号竞争——两条路径都不悬挂。
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "drop 应及时 join（实测 {elapsed:?}）——停机不悬挂"
    );
    // 正常停机路径（无在途作业）。
    let mock = MockRasterizer::new();
    let mut worker = RenderWorker::new(mock);
    let id = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 30,
    });
    let _ = await_reply(&mut worker, id);
    drop(worker); // 通道关闭——后续 try_recv 恒 None。
}

// ---------------------------------------------------------------------------
// 13-15：mock 端到端 + 对象形态 + 协议锚
// ---------------------------------------------------------------------------

#[test]
fn mock_end_to_end_cache_fill_and_hit() {
    // mock 端到端（阅读面全链路形状）：异步渲染 → 缓存填充 → 二次取页
    // **零 rasterize 调用**——缓存命中是翻页性能的根保障（LRU + 工人
    // 的组合语义）。
    let mock = MockRasterizer::new();
    let calls = mock.calls();
    let mut worker = RenderWorker::new(mock);
    // 预算 4 MiB：600×800×4 = 1.92 MiB 产物可整驻（预算 < 产物 = 超大
    // 条目自逐，端到端锁要求"填充后驻留"——预算必须覆盖单页产物）。
    let mut cache: PageCache<(u32, u32), RenderedPage> = PageCache::new(4 << 20);

    let key = (0u32, 600u32); // (页号, 目标宽)
    let id = worker.submit(RenderRequest {
        page: PageIndex(key.0),
        width_px: key.1,
    });
    let page = match await_reply(&mut worker, id) {
        WorkerReply::Done {
            result: Ok(page), ..
        } => page,
        other => panic!("渲染应成功，得 {other:?}"),
    };
    // byte_cost = 产物实际字节长（阅读面合同：bgra.len()——先取长再移值）。
    let byte_cost = page.bgra.len();
    let evicted = cache.put(key, page, byte_cost);
    assert!(evicted.is_empty(), "预算内无驱逐");
    assert_eq!(cache.bytes(), page_len_probe(&cache, key), "字节精确");
    let calls_after_fill = calls.load(Ordering::SeqCst);
    assert_eq!(calls_after_fill, 1, "填充期恰一次渲染");

    // 二次取页：命中（零 rasterize——mock 计数不增）。
    let hit = cache.get(&key).expect("二次取页必命中（预算内无驱逐）");
    assert_eq!(hit.width_px, 600);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        calls_after_fill,
        "缓存命中零 rasterize 调用（性能根保障）"
    );
    assert!(
        worker.try_recv().is_none(),
        "无新作业提交（命中路径不触工人）"
    );
}

/// 缓存内指定键的产物字节长（字节精确断言辅助）。
fn page_len_probe(cache: &PageCache<(u32, u32), RenderedPage>, key: (u32, u32)) -> usize {
    cache
        .entries()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.bgra.len())
        .expect("键在位")
}

#[test]
fn boxed_rasterizer_object_form() {
    // 对象形态（S4 前瞻）：Box<dyn Rasterizer + Send> 装箱可用——宿主
    // 以 trait 对象装配渲染管线（引擎实现可替换而不动装配代码）。
    let boxed: Box<dyn Rasterizer + Send> = Box::new(MockRasterizer::new());
    let mut worker: RenderWorker<Box<dyn Rasterizer + Send>> = RenderWorker::new(boxed);
    let id = worker.submit(RenderRequest {
        page: PageIndex(2),
        width_px: 30,
    });
    match await_reply_boxed(&mut worker, id) {
        WorkerReply::Done {
            result: Ok(page), ..
        } => assert_eq!(page.bgra.first(), Some(&3)),
        other => panic!("装箱形态作业应成功，得 {other:?}"),
    }
}

/// 装箱工人的回执等待（MockRasterizer 专属 await_reply 的装箱版——
/// 泛型参数不同，不复用同一函数）。
fn await_reply_boxed(
    worker: &mut RenderWorker<Box<dyn Rasterizer + Send>>,
    job_id: u64,
) -> WorkerReply {
    let start = std::time::Instant::now();
    loop {
        if let Some(reply) = worker.recv_timeout(Duration::from_millis(50)) {
            return reply;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "装箱作业 {job_id} 回执超时"
        );
    }
}

#[test]
fn render_types_serde_roundtrip() {
    // 协议锚：RenderRequest/RenderedPage serde 往返（PDF-c 视图事件面
    // 定稿前的载荷形状冻结——往返不变形即协议可用）。
    let request = RenderRequest {
        page: PageIndex(3),
        width_px: 800,
    };
    let page = RenderedPage {
        width_px: 800,
        height_px: 1066,
        bgra: vec![0xBA; 800 * 1066 * 4],
    };
    let request_json = serde_json::to_string(&request).expect("请求序列化");
    let page_json = serde_json::to_string(&page).expect("产物序列化");
    assert_eq!(
        serde_json::from_str::<RenderRequest>(&request_json).expect("请求反序列化"),
        request
    );
    assert_eq!(
        serde_json::from_str::<RenderedPage>(&page_json).expect("产物反序列化"),
        page
    );
    assert!(
        request_json.contains("\"width_px\":800"),
        "字段名蛇形（协议词面）：{request_json}"
    );
}

// ---------------------------------------------------------------------------
// 16. Engine 错误受限构造钳制（pdf-c-1r 审计轻微项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn engine_error_constructor_clamps_message() {
    // CommandError::engine 是 Engine 变体的唯一规范构造出口：512 字符
    // 钳制必须真实生效（错误面不得成为内存/日志滥用面）；用 CJK 多字节
    // 字符同时锁 char 边界安全（截断不得撕裂字符）。
    let long: String = "错".repeat(600);
    let err = CommandError::engine(&long);
    let CommandError::Engine(msg) = &err else {
        panic!("应得 Engine 变体");
    };
    assert_eq!(
        msg.chars().count(),
        512,
        "超长消息钳制到 512 字符（char 计，非字节）"
    );
    // 空串回退固定描述（不透传空错误）。
    let empty = CommandError::engine("");
    assert!(
        matches!(&empty, CommandError::Engine(m) if !m.is_empty()),
        "空细节回退固定描述"
    );
    // 短消息原样保留。
    assert!(
        matches!(CommandError::engine("lib 缺失"), CommandError::Engine(m) if m == "lib 缺失"),
        "512 内消息原样透传"
    );
}

// ---------------------------------------------------------------------------
// 17. 重复 put 更新路径的超大值自逐（pdf-c-1r 审计轻微项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn lru_put_update_with_oversized_value_self_evicts() {
    // 锁 6 只覆盖了 put-miss 的超大条目路径；更新路径（键已在缓存）换
    // 超大值时同样必须自逐且不残留旧值——预算不变量在两条路径上一致。
    let mut cache: PageCache<u32, &str> = PageCache::new(100);
    cache.put(1, "a", 40);
    assert_eq!(cache.len(), 1);
    // 已在缓存中的键 put 超大值（150 > 预算 100）。
    let evicted = cache.put(1, "big", 150);
    assert_eq!(
        cache.len(),
        0,
        "更新值超预算 → 该条目自逐，不残留旧值（旧值已被替换后驱逐）"
    );
    assert_eq!(cache.bytes(), 0, "bytes 归零");
    assert_eq!(
        evicted,
        vec![1],
        "自逐键进驱逐返回集（与锁 6 put-miss 路径同构——调用方按返回集作废关联资源）"
    );
    // 缓存仍可用（预算不变量恢复后继续服务）。
    cache.put(2, "b", 40);
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.get(&2), Some(&"b"));
}
