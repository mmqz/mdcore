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
//!     serde 往返（协议草案锚——载荷形状先行冻结）；
//! 16. `engine_error_constructor_clamps_message`——Engine 错误受限构造
//!     （pdf-c-1r 审计）；17. `lru_put_update_with_oversized_value_self_
//!     evicts`（同轮）；18. `worker_cancel_first_job_id_zero_is_honored`
//!     （T1 审计 F1 项——id 0 哨兵）；
//! 19. `worker_watchdog_times_out_poisons_and_replaces`——看门狗超时
//!     （PDF-e S3-L2 ❸弃置 + S4.2 工人重建）：慢作业 Timeout 回执 +
//!     毒化换代（后续作业由替代代服务——代次日志证据）+ 超时作业不
//!     重放 + 旧工人 C 调用返回后自然退出；
//! 20. `worker_watchdog_replays_queue_and_cancels_in_order`——换代重
//!     放：排队作业按提交序重放 + 未决取消令牌随作业重放（Cancelled
//!     零执行）；
//! 21. `worker_without_watchdog_serves_slow_job_to_completion`——能力
//!     面拆分：`new` 裸工人无看门狗（慢作业 Done、代次恒 0）；
//! 22. `worker_factory_panic_graceful_disconnect`——工厂 panic 断连
//!     不悬挂（T2-review ❶ 处方，T2-fix）：渲染线程内捕获 → 先置
//!     看门狗终态（其克隆 drop → 通道断连 → 宿主收报 None）→ 再
//!     传播；看门狗线程退出（Linux /proc 复核）+ 代次恒 0（死亡
//!     ≠换代）；
//! 23. `worker_replacement_factory_panic_disconnects`——替代代工厂
//!     panic 面：换代处理（宿主公共调用帧）不 panic 逃逸 + 新代
//!     死亡经断连可观测（收报 None）+ 替代代零服务。
//! 24. `worker_generation_cap_bounds_stuck_threads`——换代上限
//!     （#279 / 审计 P1-12）：敌意反复超时（每代永挂）→ 代数封顶
//!     [`MAX_GENERATIONS`]（线程/引擎实例硬上界）+ 退役态 submit
//!     即刻合成 `Timeout`（等待面不悬挂）+ 断连可观测 + 永挂计数
//!     有界（= 上限）+ 释放后全部收割 + drop 不悬挂。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// 等待回执（顺序收取直到拿到目标 id——工人回执 FIFO，目标必达；
/// 泛型化：MockRasterizer 与看门狗锁的 WatchdogMock 共用——PDF-e 起
/// 回执含 Timeout 变体，三形态都属目标 id 的合法回执）。
fn await_reply<R>(worker: &mut RenderWorker<R>, job_id: u64) -> WorkerReply
where
    R: Rasterizer + Send + 'static,
{
    let deadline = Duration::from_secs(10);
    let start = std::time::Instant::now();
    loop {
        if let Some(reply) = worker.recv_timeout(Duration::from_millis(50)) {
            assert!(
                matches!(
                    &reply,
                    WorkerReply::Done { id, .. }
                    | WorkerReply::Cancelled { id }
                    | WorkerReply::Timeout { id }
                    if *id == job_id
                ),
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

// ---------------------------------------------------------------------------
// 18. 首作业 id 0 的排队取消令牌（T1 审计 F1 项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn worker_cancel_first_job_id_zero_is_honored() {
    // F1 回归锁：旧实现 `last_replied: u64 = 0` 哨兵与首个合法作业 id 0
    // 撞值（`0 < 0` 恒 false），首作业的排队取消令牌必被 absorb_cancel
    // 丢弃——回执 Done 而非 Cancelled，违反取消合同（模块文档 + CONTRACT
    // §6 ❸弃置语义）。修复后哨兵改 Option<u64>（None = 尚未回执，不
    // 剪枝），id 0 的令牌照常吸收。
    //
    // 时序构造（确定性依据）：**取消令牌先于作业消息送达**——通道 FIFO
    // 保证工人在入队作业 0 之前必已吸收其取消令牌（吸收点 = 阻塞 recv
    // 或开跑前排空，两路径都先于队首弹出）。这是「id 0 在队且取消已
    // 吸收」的唯一确定性构造：id 0 是首作业，无前置长作业可借以加宽
    // 取消窗口（先提交后取消的次序依赖线程唤醒时序，存在不可消除的
    // 竞态窗口——本锁不依赖它）。
    let mock = MockRasterizer::new();
    let calls = mock.calls();
    let mut worker = RenderWorker::new(mock);
    worker.cancel(0); // 取消首个作业（id 0——令牌先于作业送达）
    let id_a = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 30,
    });
    let id_b = worker.submit(RenderRequest {
        page: PageIndex(1),
        width_px: 30,
    });
    assert_eq!((id_a, id_b), (0, 1), "作业 id 从 0 单调分配（合同不变）");
    match await_reply(&mut worker, id_a) {
        WorkerReply::Cancelled { id } => {
            assert_eq!(id, 0, "首作业 id 0 命中取消令牌（旧哨兵缺陷回执 Done）")
        }
        other => panic!("首作业 id 0 应被取消（零执行），得 {other:?}"),
    }
    match await_reply(&mut worker, id_b) {
        WorkerReply::Done { result: Ok(_), .. } => {}
        other => panic!("次作业不受首作业取消影响，得 {other:?}"),
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "仅次作业执行渲染——id 0 被跳过（零 rasterize 调用）"
    );
}

// ---------------------------------------------------------------------------
// 19. 看门狗超时：Timeout 回执 + 工人毒化换代 + 旧工人退出（PDF-e S3-L2）
// ---------------------------------------------------------------------------

/// 看门狗锁 mock：代次标记（工厂分配——「哪个工人服务了作业」的观测
/// 面）+ 首代慢页受控（触发超时）+ 实例退出观测（Drop 置位——旧工人
/// C 调用返回后自然退出回收的观测锚）。
struct WatchdogMock {
    generation: usize,
    /// 本代实例的慢页（None = 健康代；Some = 该页渲染睡眠 250ms）。
    slow_page: Option<u32>,
    /// (代次, 页号) 调用日志（服务证据：作业由哪一代执行）。
    log: Arc<std::sync::Mutex<Vec<(usize, u32)>>>,
    /// 本实例退出旗标（线程结束 → mock drop → 置位）。
    exited: Arc<AtomicUsize>,
}

impl Drop for WatchdogMock {
    fn drop(&mut self) {
        self.exited.fetch_add(1, Ordering::SeqCst);
    }
}

impl Rasterizer for WatchdogMock {
    fn rasterize(&mut self, request: RenderRequest) -> Result<RenderedPage, CommandError> {
        self.log
            .lock()
            .unwrap()
            .push((self.generation, request.page.0));
        if self.slow_page == Some(request.page.0) {
            // 模拟敌意病态页：渲染耗时远超看门狗预算（不可中断的 C 调用）。
            std::thread::sleep(Duration::from_millis(250));
        }
        let width = request.width_px;
        Ok(RenderedPage {
            width_px: width,
            height_px: width * 4 / 3,
            bgra: vec![7; (width * width * 4 / 3 * 4) as usize],
        })
    }
}

/// 看门狗锁共用装配台：调用日志（代次×页号）+ 各代退出旗标 +
/// 工厂（首代带敌意慢页 0，替代代健康）。
struct WatchdogRig {
    log: Arc<std::sync::Mutex<Vec<(usize, u32)>>>,
    exits: Arc<std::sync::Mutex<Vec<Arc<AtomicUsize>>>>,
}

impl WatchdogRig {
    fn new() -> Self {
        Self {
            log: Arc::new(std::sync::Mutex::new(Vec::new())),
            exits: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// 工厂闭包（'static：共享柄克隆入闭包——RenderWorker::watchdog
    /// 的构造合同）：首代带敌意慢页 0（250ms ≫ 看门狗预算）；替代代
    /// 健康。
    fn factory(&self) -> impl Fn() -> WatchdogMock + Send + Sync {
        let log = Arc::clone(&self.log);
        let exits = Arc::clone(&self.exits);
        let generations = Arc::new(AtomicUsize::new(0));
        move || {
            let gen = generations.fetch_add(1, Ordering::SeqCst);
            let exited = Arc::new(AtomicUsize::new(0));
            exits.lock().unwrap().push(Arc::clone(&exited));
            WatchdogMock {
                generation: gen,
                slow_page: (gen == 0).then_some(0),
                log: Arc::clone(&log),
                exited,
            }
        }
    }

    /// 调用日志快照（(代次, 页号) 序列——服务证据面）。
    fn log_snapshot(&self) -> Vec<(usize, u32)> {
        self.log.lock().unwrap().clone()
    }

    /// 指定代实例是否已退出（线程结束 → mock drop → 旗标 > 0）。
    fn generation_exited(&self, gen: usize) -> bool {
        self.exits
            .lock()
            .unwrap()
            .get(gen)
            .is_some_and(|flag| flag.load(Ordering::SeqCst) > 0)
    }
}

/// 轮询等待条件成立（deadline 有界——事件落地为微秒级，轮询间隔
/// 只影响检测延迟不影响确定性）。
fn poll_until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(
            std::time::Instant::now() < deadline,
            "等待超时：{what}（10s 内未成立——机制疑似失效）"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn worker_watchdog_times_out_poisons_and_replaces() {
    // S3-L2 ❸弃置 + S4.2 工人重建（PDF-e 实装）回归锁：慢作业超看门狗
    // → ① 该作业 Timeout 回执（结果弃置不采用）；② 工人毒化换代
    // （后续作业由替代代服务——调用日志的代次证据）；③ 旧工人 C 调用
    // 返回后自然退出（实例 Drop 旗标）；④ 超时作业不重放（恰执行一次）。
    let rig = WatchdogRig::new();
    let mut worker = RenderWorker::watchdog(rig.factory(), Duration::from_millis(80));
    let slow = worker.submit(RenderRequest {
        page: PageIndex(0), // 首代敌意页（250ms ≫ 80ms 预算）
        width_px: 8,
    });
    match await_reply(&mut worker, slow) {
        WorkerReply::Timeout { id } => {
            assert_eq!(id, slow, "慢作业回执 Timeout（❸弃置——typed）")
        }
        other => panic!("超预算作业应 Timeout，得 {other:?}"),
    }
    // 换代观测（确定性：轮询 generation——事件由看门狗在 Timeout 后
    // 微秒级送达，公共调用入口处理）。
    poll_until(
        "看门狗换代事件落地（generation 0 → 1）",
        || worker.generation() == 1,
    );
    // 后续作业由替代工人服务（代次证据 = 日志 (1, page)）。
    let next = worker.submit(RenderRequest {
        page: PageIndex(1),
        width_px: 8,
    });
    match await_reply(&mut worker, next) {
        WorkerReply::Done {
            id,
            result: Ok(page),
        } => {
            assert_eq!(id, next);
            assert_eq!(page.width_px, 8, "新代产物形状正常");
        }
        other => panic!("换代后作业应 Done（新代服务），得 {other:?}"),
    }
    // 执行证据：敌意页恰执行一次（首代开跑后看门狗才可能计时超限——
    // 不重放）；后续页由新代执行恰一次。
    let log = rig.log_snapshot();
    assert_eq!(
        log.iter().filter(|(_, page)| *page == 0).count(),
        1,
        "超时作业不重放（弃置非重试）：{log:?}"
    );
    assert!(log.contains(&(1, 1)), "后续作业由替代代服务：{log:?}");
    // 旧工人自然退出（C 调用 250ms 返回 → 断连 → 线程退出 → mock drop）。
    poll_until(
        "旧工人线程退出（敌意渲染返回后自然回收）",
        || rig.generation_exited(0),
    );
    // 收尾：drop 不悬挂（当前代健康，join 即回；旧代已 detach）。
    drop(worker);
}

#[test]
fn worker_watchdog_replays_queue_and_cancels_in_order() {
    // 换代重放语义锁：慢作业超时触发换代时，排队作业按提交序重放、
    // 未决取消令牌随作业重放（新代 FIFO 吸收路径同首播）——回执序
    // Timeout{0} → Done{1} → Cancelled{2}，被取消作业零执行。
    let rig = WatchdogRig::new();
    let mut worker = RenderWorker::watchdog(rig.factory(), Duration::from_millis(80));
    let id0 = worker.submit(RenderRequest {
        page: PageIndex(0), // 首代敌意页（看门狗触发点）
        width_px: 8,
    });
    let id1 = worker.submit(RenderRequest {
        page: PageIndex(1),
        width_px: 8,
    });
    let id2 = worker.submit(RenderRequest {
        page: PageIndex(2),
        width_px: 8,
    });
    // 排队取消 id2（首代卡在 id0——令牌必未吸收，随换代重放）。
    worker.cancel(id2);
    match await_reply(&mut worker, id0) {
        WorkerReply::Timeout { id } => assert_eq!(id, id0, "慢作业 Timeout"),
        other => panic!("作业 0 应 Timeout，得 {other:?}"),
    }
    poll_until("换代事件落地", || worker.generation() == 1);
    match await_reply(&mut worker, id1) {
        WorkerReply::Done { id, .. } => assert_eq!(id, id1, "重放作业 1 完成于新代"),
        other => panic!("重放作业 1 应 Done，得 {other:?}"),
    }
    match await_reply(&mut worker, id2) {
        WorkerReply::Cancelled { id } => assert_eq!(id, id2, "取消令牌随作业重放——Cancelled"),
        other => panic!("重放作业 2 应被取消，得 {other:?}"),
    }
    // 执行证据：页 0 恰一次（首代）、页 1 恰一次（新代）、页 2 零次
    // （取消先于执行）。
    let log = rig.log_snapshot();
    assert_eq!(log.iter().filter(|(_, p)| *p == 0).count(), 1, "{log:?}");
    assert_eq!(log.iter().filter(|(_, p)| *p == 1).count(), 1, "{log:?}");
    assert_eq!(log.iter().filter(|(_, p)| *p == 2).count(), 0, "{log:?}");
    poll_until("旧工人线程退出", || rig.generation_exited(0));
    drop(worker);
}

#[test]
fn worker_without_watchdog_serves_slow_job_to_completion() {
    // 能力面拆分锁：`new` = 无看门狗裸工人（无引擎重建能力即无换代
    // 面）——慢作业照常完成（Done 而非 Timeout），代次恒 0。对照锁：
    // 看门狗是 `watchdog` 工厂构造的增益面，不隐式施于所有工人。
    let mock = MockRasterizer::with_delay(Duration::from_millis(150));
    let mut worker = RenderWorker::new(mock);
    let id = worker.submit(RenderRequest {
        page: PageIndex(3),
        width_px: 8,
    });
    match await_reply(&mut worker, id) {
        WorkerReply::Done {
            id: reply_id,
            result: Ok(_),
        } => assert_eq!(reply_id, id, "无看门狗工人慢作业照常完成"),
        other => panic!("裸工人不产生 Timeout（看门狗非隐式），得 {other:?}"),
    }
    assert_eq!(worker.generation(), 0, "无换代（代次恒 0）");
}

// ---------------------------------------------------------------------------
// 22. 工厂 panic：断连不悬挂 + 看门狗退出 + 代次恒 0（T2-review ❶ 回归锁）
// ---------------------------------------------------------------------------

#[test]
fn worker_factory_panic_graceful_disconnect() {
    // 工厂 panic 面（T2-review ❶ 处方，T2-fix 回归锁）：宿主照抄文档
    // 示例闭包（`move || PdfiumEngine::open(bytes.clone()).expect(...)`）
    // 的 expect 失败路径 = 工厂 panic。修复语义：渲染线程闭包内
    // catch_unwind → **先**置 state=Retired（看门狗见终态即退——不
    // 泄漏 10ms 轮询线程 → 其回执通道克隆 drop）→ **再** resume 传播
    // （渲染线程死亡）。
    //
    // 断言面：① 宿主收报得 None **且远早于收报预算**（断连而非超时：
    // recv_timeout 防真悬挂卡死套件——若看门狗泄漏持克隆，通道永不
    // 断连，recv_timeout 将吃满 5s 预算，耗时断言判红）；② 看门狗
    // 线程退出（断连已机械证明其克隆 drop——克隆为 watchdog_loop 按
    // 值局部，drop 即函数返回、此后不可能再轮询；Linux 面再以
    // /proc/self/task 线程计数回归基线直接复核线程本体）；③ 死亡
    // 不是换代（代次恒 0——无 Timeout 合成、无换代事件：工厂 panic
    // 与看门狗毒化两机制分立）。
    #[cfg(target_os = "linux")]
    let baseline_tasks = std::fs::read_dir("/proc/self/task")
        .map(|entries| entries.count())
        .unwrap_or(usize::MAX);
    let mut worker = RenderWorker::watchdog(
        || -> MockRasterizer { panic!("工厂受控 panic（expect 失败路径）") },
        Duration::from_millis(80),
    );
    worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 8,
    });
    // ① 断连不悬挂：None 且远早于预算（断连解阻塞，超时解阻塞的
    // 耗时 = 全预算——两者由耗时断言区分）。
    let started = std::time::Instant::now();
    let reply = worker.recv_timeout(Duration::from_secs(5));
    let elapsed = started.elapsed();
    assert!(
        reply.is_none(),
        "工厂 panic 后宿主收报应得 None（通道断连）；得 {reply:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "断连应在看门狗轮询间隔量级内落地（实测 {elapsed:?}）；吃满预算 = 通道未断连（悬挂面）"
    );
    // ③ 死亡不是换代（代次恒 0——与看门狗毒化换代分立的两机制）。
    assert_eq!(
        worker.generation(),
        0,
        "工厂 panic 是死亡不是换代（代次不得 +1）"
    );
    // ② 看门狗线程退出（Linux 直接复核线程本体；非 Linux 断连证明
    // 已足——克隆 drop 即 watchdog_loop 返回，无再轮询可能）。
    #[cfg(target_os = "linux")]
    {
        poll_until(
            "看门狗线程退出（/proc/self/task 线程计数回归基线）",
            || {
                std::fs::read_dir("/proc/self/task")
                    .map(|entries| entries.count())
                    .is_ok_and(|count| count <= baseline_tasks)
            },
        );
    }
    drop(worker); // 收尾不悬挂（渲染线程已死，join 即回）。
}

// ---------------------------------------------------------------------------
// 23. 替代代工厂 panic：换代处理不 panic 逃逸 + 断连（T2-review ❶ 换代侧）
// ---------------------------------------------------------------------------

#[test]
fn worker_replacement_factory_panic_disconnects() {
    // 替代代工厂 panic 面（T2-review ❶ 处方的换代侧收口）：修复前
    // 工厂在包装器线程（process_retirements 的宿主公共调用帧内）执行
    // ——替代代重开失败 = panic 直接炸穿宿主 recv/submit/generation
    // 调用（非受控逃逸）；修复后工厂移入渲染线程闭包 + catch_unwind
    // ——换代处理正常返回，替代代死亡经通道断连可观测（收报 None）。
    let calls = Arc::new(AtomicUsize::new(0));
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let factory = {
        let calls = Arc::clone(&calls);
        let factory_calls = Arc::clone(&factory_calls);
        move || -> MockRasterizer {
            // 首代健康（带敌意慢页 0——触发看门狗换代）；替代代 panic
            //（字节快照重开失败路径：快照损坏/装载失败）。
            if factory_calls.fetch_add(1, Ordering::SeqCst) > 0 {
                panic!("替代代工厂受控 panic（字节快照重开失败）");
            }
            MockRasterizer {
                calls: Arc::clone(&calls),
                panic_on: Arc::new(std::sync::Mutex::new(None)),
                delay: Duration::from_millis(250), // ≫ 80ms 看门狗预算
            }
        }
    };
    let mut worker = RenderWorker::watchdog(factory, Duration::from_millis(80));
    let slow = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 8,
    });
    // 首代慢作业 Timeout——随后的 generation() 公共调用即修复前的
    // panic 逃逸点（替代代工厂曾在该宿主帧内执行）。
    match await_reply(&mut worker, slow) {
        WorkerReply::Timeout { id } => assert_eq!(id, slow, "首代慢作业 Timeout（触发换代）"),
        other => panic!("首代慢作业应 Timeout，得 {other:?}"),
    }
    poll_until(
        "换代事件落地（generation 0 → 1，宿主调用帧无 panic 逃逸）",
        || worker.generation() == 1,
    );
    // 替代代工厂 panic → 渲染线程死亡 + 看门狗终态退出 → 新代回执
    // 通道断连：宿主收报 None（recv_timeout 防真悬挂；耗时远小于
    // 预算 = 断连而非超时）。
    let started = std::time::Instant::now();
    let reply = worker.recv_timeout(Duration::from_secs(5));
    let elapsed = started.elapsed();
    assert!(
        reply.is_none(),
        "替代代死亡后宿主收报应得 None（断连）；得 {reply:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "断连应在看门狗轮询间隔量级内落地（实测 {elapsed:?}）；吃满预算 = 未断连（悬挂面）"
    );
    assert_eq!(worker.generation(), 1, "死亡不再触发换代（代次停在 1）");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "仅首代慢作业执行过渲染（替代代未服务任何作业）"
    );
    assert_eq!(
        factory_calls.load(Ordering::SeqCst),
        2,
        "工厂恰调两次（首代 + 替代代 panic 面）"
    );
    drop(worker); // 收尾不悬挂（渲染线程已死，join 即回）。
}

// ---------------------------------------------------------------------------
// 24：换代上限——敌意反复超时的资源硬上界（#279 / 审计 P1-12）
// ---------------------------------------------------------------------------

/// 永挂 mock（#279 压力面）：模拟「不可中断的敌意 C 调用」——渲染
/// 进入后自旋等待释放旗标（永不返回直到测试收尾置位 → 线程退出 →
/// 可收割验证）。每一代都挂（最坏敌意形态：换代后的新代同样病态）。
struct HangMock {
    /// 释放旗标（全部代共享——测试收尾统一置位让线程退出）。
    release: Arc<AtomicBool>,
}

impl Rasterizer for HangMock {
    fn rasterize(&mut self, _request: RenderRequest) -> Result<RenderedPage, CommandError> {
        while !self.release.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(RenderedPage {
            width_px: 8,
            height_px: 10,
            bgra: vec![0; 8 * 10 * 4],
        })
    }
}

/// 敌意反复超时压力锁：超上限次数触发看门狗 → 代数封顶
/// [`MAX_GENERATIONS`]（含首代）、退役线程计数有界（≤ 上限）、
/// 退役态 submit 即刻合成 `Timeout`（等待面不悬挂——修复前此路径
/// 送死通道永无回执）、断连可观测、释放后全部收割回收、drop 不悬挂。
#[test]
fn worker_generation_cap_bounds_stuck_threads() {
    use mdc_pdf::worker::MAX_GENERATIONS;

    let release = Arc::new(AtomicBool::new(false));
    let factory = {
        let release = Arc::clone(&release);
        move || HangMock {
            release: Arc::clone(&release),
        }
    };
    // 看门狗预算 40ms（永挂渲染 ≫ 预算——每代必触发）。
    let mut worker = RenderWorker::watchdog(factory, Duration::from_millis(40));

    // 敌意循环：超上限次数（上限 16 代 → 提交 20 个作业）。
    // 前半：每作业真超时（看门狗 40ms 后 Timeout）+ 换代；
    // 后半（退役态）：即刻合成 Timeout（无真渲染）。
    let mut ids = Vec::new();
    for _ in 0..20 {
        let id = worker.submit(RenderRequest {
            page: PageIndex(0),
            width_px: 8,
        });
        ids.push(id);
        match await_reply(&mut worker, id) {
            WorkerReply::Timeout { id: got } => assert_eq!(got, id, "敌意作业 Timeout（❸弃置）"),
            other => panic!("永挂渲染应 Timeout，得 {other:?}"),
        }
    }
    // 每代恰执行一个作业（一枪一代：超时即毒化，不重放）——
    // 前 16 代真挂（16 作业），后 4 个合成（零执行）。
    assert!(
        worker.is_exhausted(),
        "20 次敌意超时后必达换代上限（上限 {}）",
        MAX_GENERATIONS
    );
    // 代数封顶：generation 停在上限 - 1（0 基末代；不再递增）。
    assert_eq!(
        worker.generation(),
        MAX_GENERATIONS - 1,
        "代数封顶（含首代至多 {MAX_GENERATIONS} 代——线程/引擎实例硬上界）"
    );

    // 退役态 submit：即刻合成 Timeout（等待面不悬挂的核心锁——
    // 修复前：送死通道永无回执，await_reply 10s 死限必 panic）。
    let started = std::time::Instant::now();
    let post = worker.submit(RenderRequest {
        page: PageIndex(0),
        width_px: 8,
    });
    match await_reply(&mut worker, post) {
        WorkerReply::Timeout { id } => assert_eq!(id, post, "退役态 submit 合成 Timeout"),
        other => panic!("退役态应合成 Timeout，得 {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "合成回执应即刻送达（实测 {:?}——疑似走了真超时路径）",
        started.elapsed()
    );
    // 死亡可观测：全部合成回执送达后，收报 None（哑元通道断连）。
    assert!(
        worker.recv_timeout(Duration::from_millis(50)).is_none(),
        "退役态收报应断连 None（死亡可观测）"
    );

    // 资源上界（悬挂态）：退役线程计数恰 = 上限（每代一个、全部
    // 永挂未退——最坏敌意形态的精确上界）。
    assert_eq!(
        worker.retired_alive(),
        MAX_GENERATIONS as usize,
        "永挂线程计数 = 换代上限（有界泄漏的直接量度）"
    );

    // 收尾：释放旗标 → 永挂线程全部退出 → 收割归零（跟踪式弃置
    // 的回收证明——修复前 detach 弃置无此观测面）。
    release.store(true, Ordering::SeqCst);
    poll_until(
        "释放后永挂线程全部退出并收割（retired_alive → 0）",
        || worker.retired_alive() == 0,
    );
    // drop 不悬挂（退役态当前代句柄已入 retired，不 join 永挂线程）。
    let started = std::time::Instant::now();
    drop(worker);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "退役态 drop 不悬挂（实测 {:?}）",
        started.elapsed()
    );
}
