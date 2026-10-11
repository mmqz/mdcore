//! 渲染工人——专用渲染线程 + mpsc 双通道 + **panic 隔离**（PDF-c 阅读面，
//! 常开模块；S4.1/S4.2 落地）。
//!
//! 架构（SECURITY_POLICY.md S4）：
//! - **专用线程**（S4.1）：`RenderWorker` 把 `Rasterizer` **移入**专职
//!   渲染线程——pdfium 调用与文档状态/UI 线程隔离；作业进（命令通道）
//!   / 结果出（回执通道）双向 mpsc；
//! - **panic 隔离**（S4.2）：作业执行体包 `catch_unwind`
//!   （`AssertUnwindSafe`——`&mut R` 跨 unwind 传递；Rust panic 只在
//!   Rust 帧展开，C 侧句柄不被 unwind 破坏）。panic 转化为
//!   [`CommandError::RenderPanic`]（页级错误而非进程崩溃——一坏页不
//!   毒全档）。**诚实边界**：隔离的是 Rust 侧 panic；pdfium C++ 侧的
//!   段错误/abort 无法在本层捕获（进程隔离属 PDF-e 后的强化面）；
//! - **取消令牌**（❸弃置语义）：`cancel` 对**尚未开跑**的排队作业生效
//!   （工人执行队首作业前先非阻塞排空命令通道吸收全部已送达取消令牌）；
//!   已开跑的作业不可中断（不假装能杀线程）——回执照常送达，取消方
//!   自行丢弃结果（"结果永不采用"）；
//! - **停机**：`drop` 发停机信号并 join 渲染线程（不悬挂；在跑作业
//!   完成后才退出，停机时刻的排队作业静默终止无回执——通道关闭可见）。
//!
//! 回执合同：每个 `submit` 的作业 id **至多**一条回执（`Done` 或
//! `Cancelled`，按提交序到达——单工人 FIFO；例外 = 停机时未跑完的排队
//! 作业无回执），作业 id 由工人单调分配（0 起步，不复用）。

use std::collections::{HashSet, VecDeque};
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::command::CommandError;
use crate::render::{Rasterizer, RenderRequest, RenderedPage};

/// 工人命令（宿主 → 渲染线程；内部通道协议面，不进 serde 协议）。
enum WorkerCommand {
    /// 渲染作业（id 由工人分配）。
    Job { id: u64, request: RenderRequest },
    /// 取消令牌：作用于尚未开跑的排队作业（❸弃置——见模块文档）。
    Cancel { id: u64 },
    /// 停机（drop 时发送；在跑作业完成后退出）。
    Shutdown,
}

/// 作业回执（渲染线程 → 宿主）。
///
/// serde 面：Serialize-only（错误面为 Rust → 宿主单向，对齐
/// [`CommandError`] 纪律；`Failed` 携带 typed 错误供宿主穷举分支）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum WorkerReply {
    /// 作业完成（成功载荷或 typed 错误——含 panic 转化的 RenderPanic）。
    Done {
        id: u64,
        result: Result<RenderedPage, CommandError>,
    },
    /// 作业被取消（开跑前命中取消令牌——未执行任何渲染）。
    Cancelled { id: u64 },
}

/// 渲染工人（持专用线程；`R` 移入线程独占使用）。
///
/// 线程纪律：`R: Rasterizer + Send`——引擎以**所有权转移**进入工人
/// （不跨线程共享引用）；pdfium 全局互斥（pdfium-render `thread_safe`
/// 特性）为跨引擎并发兜底。drop 即停机。
pub struct RenderWorker<R>
where
    R: Rasterizer + Send + 'static,
{
    tx: Sender<WorkerCommand>,
    rx: Receiver<WorkerReply>,
    join: Option<JoinHandle<()>>,
    next_job_id: u64,
    /// `R` 在构造时移入渲染线程——类型参数只约束构造面（工人本身
    /// 不持有引擎；fn 指针标记 = 零所有权，不影响 Send/Sync）。
    _rasterizer: PhantomData<fn() -> R>,
}

impl<R> RenderWorker<R>
where
    R: Rasterizer + Send + 'static,
{
    /// 启动工人（派生渲染线程，`rasterizer` 移入线程独占持有）。
    pub fn new(rasterizer: R) -> Self {
        let (cmd_tx, cmd_rx) = channel::<WorkerCommand>();
        let (reply_tx, reply_rx) = channel::<WorkerReply>();
        let join = thread::Builder::new()
            .name("mdc-pdf-render-worker".into())
            .spawn(move || worker_loop(rasterizer, cmd_rx, reply_tx))
            .expect("渲染工人线程派生失败（线程资源耗尽——fail-fast）");
        Self {
            tx: cmd_tx,
            rx: reply_rx,
            join: Some(join),
            next_job_id: 0,
            _rasterizer: PhantomData,
        }
    }

    /// 提交渲染作业（异步；返回作业 id 供回执关联与取消）。
    pub fn submit(&mut self, request: RenderRequest) -> u64 {
        let id = self.next_job_id;
        self.next_job_id += 1;
        // 无界通道：send 只在工人停机后失败（drop 前后时序竞争）——
        // 丢弃即可，作业无回执（工人已死，drop 的 join 保证不悬挂）。
        let _ = self.tx.send(WorkerCommand::Job { id, request });
        id
    }

    /// 取消令牌（作用于尚未开跑的排队作业；已回执的 id 无效果）。
    pub fn cancel(&mut self, job_id: u64) {
        let _ = self.tx.send(WorkerCommand::Cancel { id: job_id });
    }

    /// 非阻塞收报（无回执 None；工人线程死亡后同样 None）。
    pub fn try_recv(&mut self) -> Option<WorkerReply> {
        self.rx.try_recv().ok()
    }

    /// 限时收报（超时 None；语义与 `try_recv` 同——超时与工人死亡
    /// 不作区分，v0 观测面保持最小）。
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<WorkerReply> {
        match self.rx.recv_timeout(timeout) {
            Ok(reply) => Some(reply),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// 阻塞收报（工人线程死亡且通道排空后 None）。
    pub fn recv(&mut self) -> Option<WorkerReply> {
        self.rx.recv().ok()
    }
}

impl<R> Drop for RenderWorker<R>
where
    R: Rasterizer + Send + 'static,
{
    /// 停机：发信号 + join（在跑作业完成后线程退出；join 保证线程
    /// 资源回收完成后 drop 才返回——停机不悬挂语义锁背书）。
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 渲染线程主循环：内部待跑队列 + 命令通道。
///
/// 执行队首作业前**非阻塞排空命令通道**（吸收取消令牌/追加后续作业/
/// 响应停机）——保证队首开跑前已见过全部已送达取消（FIFO 下 cancel
/// 消息必晚于其作业消息，不排空则取消永不可见）。
///
/// `last_replied` 单调（回执 id 递增；`Option`：None = 尚未回执过任何
/// 作业——下界哨兵不与合法 id 0 撞值，T1 审计 F1 修复：旧实现以 0
/// 为哨兵，首作业 id 0 的排队取消令牌必被 `0 < 0` 误弃）：取消集以
/// 它剪枝（已回执作业的迟到取消令牌直接丢弃）；`job_watermark` =
/// 迄今见过的最大作业 id，幽灵令牌防御（pdf-c-1r 审计轻微项）：
/// id > watermark = 调用方取消了从未提交的作业（调用方 bug），
/// 丢弃不入驻——集合上界 = 在飞作业数，不随会话泄漏。
fn worker_loop<R>(mut rasterizer: R, cmd_rx: Receiver<WorkerCommand>, reply_tx: Sender<WorkerReply>)
where
    R: Rasterizer + Send,
{
    let mut cancelled: HashSet<u64> = HashSet::new();
    let mut last_replied: Option<u64> = None;
    let mut job_watermark: u64 = 0;
    let mut pending: VecDeque<(u64, RenderRequest)> = VecDeque::new();
    loop {
        if pending.is_empty() {
            // 空闲：阻塞等命令。
            match cmd_rx.recv() {
                Ok(WorkerCommand::Job { id, request }) => {
                    job_watermark = job_watermark.max(id);
                    pending.push_back((id, request));
                }
                Ok(WorkerCommand::Cancel { id }) => {
                    absorb_cancel(&mut cancelled, id, last_replied, job_watermark)
                }
                Ok(WorkerCommand::Shutdown) | Err(_) => break,
            }
            continue;
        }
        // 就绪：排空命令通道（取消令牌先于队首开跑生效）。
        while let Ok(command) = cmd_rx.try_recv() {
            match command {
                WorkerCommand::Job { id, request } => {
                    job_watermark = job_watermark.max(id);
                    pending.push_back((id, request));
                }
                WorkerCommand::Cancel { id } => {
                    absorb_cancel(&mut cancelled, id, last_replied, job_watermark)
                }
                // 停机优先：排队作业静默终止（无回执——通道关闭可见）。
                WorkerCommand::Shutdown => return,
            }
        }
        let (id, request) = pending.pop_front().expect("pending 非空（分支条件）");
        if cancelled.remove(&id) {
            // 开跑前命中取消令牌——跳过执行（零渲染调用）。
            let _ = reply_tx.send(WorkerReply::Cancelled { id });
            last_replied = Some(id);
            continue;
        }
        // panic 隔离（S4.2）：AssertUnwindSafe 的理由——&mut R 跨
        // unwind 后无法证明一致性，但 pdfium 句柄在 C 堆（Rust
        // unwind 不触碰），后续作业继续尝试（工人存活语义锁）。
        let result = catch_unwind(AssertUnwindSafe(|| rasterizer.rasterize(request)))
            .unwrap_or(Err(CommandError::RenderPanic));
        let _ = reply_tx.send(WorkerReply::Done { id, result });
        last_replied = Some(id);
    }
}

/// 取消令牌吸收：双侧剪枝——已回执（`last_replied` 及更早；None =
/// 尚未回执任何作业，不剪枝——首作业 id 0 的令牌不得被哨兵撞值误弃
/// [T1 审计 F1]）丢弃（结果已被取消方按弃置语义处理）；从未提交
/// （> job_watermark，调用方 bug 的幽灵令牌）丢弃（pdf-c-1r 审计防御）；
/// 只驻留在飞作业的取消令牌。
fn absorb_cancel(
    cancelled: &mut HashSet<u64>,
    id: u64,
    last_replied: Option<u64>,
    job_watermark: u64,
) {
    if last_replied.is_none_or(|lr| lr < id) && id <= job_watermark {
        cancelled.insert(id);
    }
}
