//! 渲染工人——专用渲染线程 + mpsc 双通道 + **panic 隔离** + **看门狗
//! 换代**（PDF-c 阅读面常开模块；S4.1/S4.2 落地，看门狗/换代 = PDF-e
//! 安全姿态对齐）。
//!
//! 架构（SECURITY_POLICY.md S3-L2/S4）：
//! - **专用线程**（S4.1）：`RenderWorker` 把 `Rasterizer` **移入**专职
//!   渲染线程——pdfium 调用与文档状态/UI 线程隔离；作业进（命令通道）
//!   / 结果出（回执通道）双向 mpsc；
//! - **panic 隔离**（S4.2）：作业执行体包 `catch_unwind`
//!   （`AssertUnwindSafe`——`&mut R` 跨 unwind 传递；Rust panic 只在
//!   Rust 帧展开，C 侧句柄不被 unwind 破坏）。panic 转化为
//!   [`CommandError::RenderPanic`]（页级错误而非进程崩溃——一坏页不
//!   毒全档）。**诚实边界**：隔离的是 Rust 侧 panic；pdfium C++ 侧的
//!   段错误/abort 无法在本层捕获（进程隔离属 PDF-e 后的强化面）；
//! - **看门狗换代**（S3-L2 ❸弃置 + S4.2 工人重建，PDF-e 实装）：
//!   [`RenderWorker::watchdog`] 工厂构造起**看门狗线程**监控在跑作业
//!   （作业开跑时刻起算，非提交时刻）；超 `render_watchdog_ms` →
//!   ① 该作业回执 [`WorkerReply::Timeout`]（❸弃置——结果永不采用，
//!   迟到回执由包装器按「已回执 id」静默丢弃——机械保证）；② 工人
//!   毒化；③ **替代工人**（工厂闭包重开引擎——S4.4 字节快照纪律：
//!   闭包内含字节快照、首代与替代代引擎都在渲染线程内构造；**工厂
//!   panic 不炸宿主调用帧**——渲染线程内捕获后**先**置看门狗终态
//!   （看门狗随之退出、其回执克隆 drop → 通道断连）**再**传播：宿主
//!   收报得 `None`，死亡可观测、不悬挂、不泄漏看门狗——T2-fix）；
//!   在飞作业/未决取消令牌按提交序重放给新代，旧线程**断连弃置**
//!   （C 调用返回后见断连即退、排队作业不再执行；句柄转入 `retired`
//!   **跟踪式弃置**——退出即收割回收、永挂计数可观测
//!   （[`RenderWorker::retired_alive`]），敌意永挂 C 调用不得悬挂
//!   停机）；
//!   **换代上限**（#279 / 审计 P1-12）：一生至多 [`MAX_GENERATIONS`]
//!   代（含首代）——敌意反复超时的线程/引擎实例硬上界。达上限
//!   工人进入**退役态**：不替代（在飞作业合成 `Timeout`——等待面
//!   不悬挂）、后续 `submit` 即刻合成 `Timeout`（协议面零变化）、
//!   通道换哑元断连（recv 可观测死亡）。永挂线程的进程级回收
//!   （杀线程需进程隔离）属架构级裁定，登记 Lead（#279）。
//!   [`RenderWorker::new`] 保持无看门狗裸工人（无重建能力即无换代——
//!   能力面拆分在档）；单工人常态 + 看门狗触发的工人重建 = v0 形态
//!   （非常驻工人池）；
//! - **取消令牌**（❶排队面语义）：`cancel` 对**尚未开跑**的排队作业
//!   生效（工人执行队首作业前先非阻塞排空命令通道吸收全部已送达取消
//!   令牌）；已开跑的作业不可中断（不假装能杀线程）——回执照常送达，
//!   取消方自行丢弃结果（"结果永不采用"）。换代时未决令牌随作业
//!   重放（新代 FIFO 吸收路径同首播）；
//! - **停机**：`drop` 发停机信号并 join **当前代**渲染线程（不悬挂；
//!   在跑作业完成后才退出，停机时刻的排队作业静默终止无回执——通道
//!   关闭可见）。已退役旧代不 join（断连弃置，见上）。
//!
//! 回执合同：每个 `submit` 的作业 id **至多**一条回执（`Done` /
//! `Cancelled` / `Timeout`，按提交序到达——单工人 FIFO + 换代重放
//! 保序；例外 = 停机/断连时未跑完的排队作业无回执），作业 id 由工人
//! 单调分配（0 起步，不复用）。已回执作业的迟到回执（换代竞态窗口）
//!   一律静默丢弃。

use std::collections::{HashSet, VecDeque};
use std::marker::PhantomData;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::command::CommandError;
use crate::render::{Rasterizer, RenderRequest, RenderedPage};

/// 看门狗轮询间隔（实现细节，非策略值——20s 预算下 2000 次空转，
/// CPU 可忽略；检测延迟上界 = 本值）。
const WATCHDOG_POLL: Duration = Duration::from_millis(10);

/// 换代上限（#279 / 审计 P1-12）：一个工人一生至多派生这么多代
/// （**含首代**）——敌意反复超时的资源硬上界（每代 = 1 渲染线程 +
/// 1 引擎实例；看门狗线程一枪即退不计入）。达上限不再换代，工人
/// 进入**退役态**（详见 [`RenderWorker::submit`] 与模块文档）。
///
/// 取值权衡：合法场景看门狗（默认预算 20s）极少触发——一生 16 次
/// 超预算换代已属病态环境；敌意场景上界 = 16 个永挂线程 + 16 个
/// 引擎实例（有界可存活）。进程级回收（杀挂线程需进程隔离）属
/// 架构级裁定，登记 Lead（#279），v0 以计数上界兜底。
pub const MAX_GENERATIONS: u64 = 16;

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
/// `Timeout` 变体 = PDF-e 增（v1.3——CONTRACT §6 在档）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum WorkerReply {
    /// 作业完成（成功载荷或 typed 错误——含 panic 转化的 RenderPanic）。
    Done {
        id: u64,
        result: Result<RenderedPage, CommandError>,
    },
    /// 作业被取消（开跑前命中取消令牌——未执行任何渲染）。
    Cancelled { id: u64 },
    /// 作业看门狗超时（S3-L2 ❸弃置，PDF-e）：渲染超预算——结果弃置
    /// 永不采用，工人已毒化换代（替代工人服务后续作业；本作业的
    /// 迟到 `Done` 若存在由包装器丢弃）。**换代上限耗尽后**（#279）
    /// 同变体合成：退役态工人的在飞/后续作业均以本回执收尾（等待面
    /// 不悬挂；协议面零变化——复用既有变体，宿主无需新增分支）。
    Timeout { id: u64 },
}

impl WorkerReply {
    /// 关联作业 id（三变体公共面）。
    fn job_id(&self) -> u64 {
        match self {
            WorkerReply::Done { id, .. }
            | WorkerReply::Cancelled { id }
            | WorkerReply::Timeout { id } => *id,
        }
    }
}

/// 看门狗观测态（渲染线程 ↔ 看门狗线程共享；短锁 + 纯赋值——锁内
/// 不可 panic，无中毒面）。
#[derive(Debug)]
enum WatchState {
    /// 空闲（无在跑作业）。
    Idle,
    /// 作业开跑（看门狗计时的锚——**开跑时刻**起算，非提交时刻）。
    Running { id: u64, started: Instant },
    /// 终态：看门狗触发（毒化标记）/ 工厂 panic（T2-fix：先置终态再
    /// 传播）/ 工人线程退出——三者皆使看门狗随之退出（不泄漏轮询
    /// 线程）。
    Retired,
}

/// 监督面（[`RenderWorker::watchdog`] 工厂构造在位时）。
struct Supervise<R>
where
    R: Rasterizer + Send + 'static,
{
    /// 引擎工厂（字节快照重开——S4.4 纪律；`Arc` 共享跨代调用）。
    factory: Arc<dyn Fn() -> R + Send + Sync>,
    /// 看门狗预算（常量跨代沿用）。
    watchdog: Duration,
    /// 换代事件通道（看门狗 → 包装器；一枪一代：触发即发一事件退出）。
    retire_rx: Receiver<u64>,
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
    /// 监督面（None = `new` 裸工人——无看门狗/无换代/无在飞跟踪）。
    supervise: Option<Supervise<R>>,
    /// 退役代已到未投递回执（drain 自旧代通道——先于新代回执送达，
    /// 保持提交序前缀）。
    stashed: VecDeque<WorkerReply>,
    /// 在飞作业（提交未回执——换代重放源；序 = 提交序）。
    in_flight: VecDeque<(u64, RenderRequest)>,
    /// 已发取消令牌且作业未回执（换代时随作业重放）。
    pending_cancels: HashSet<u64>,
    /// 工人代次（0 起，每次换代 +1——观测面：宿主可感知换代事实）。
    generation: u64,
    /// 退役代渲染线程句柄（跟踪式弃置：finished 即收割回收、
    /// 永挂计数在案——[`RenderWorker::retired_alive`] 观测面；
    /// 长度上界 = [`MAX_GENERATIONS`]（每代至多入账一次：替换路径
    /// 旧代 + 退役收尾的末代，#279）。
    retired: Vec<JoinHandle<()>>,
    /// 换代上限耗尽标志（true = 退役态：不再派生新代，submit 即刻
    /// 合成 `Timeout`，通道已换哑元断连——死亡可观测不悬挂）。
    exhausted: bool,
    /// `R` 在构造时移入渲染线程——类型参数只约束构造面（工人本身
    /// 不持有引擎；fn 指针标记 = 零所有权，不影响 Send/Sync）。
    _rasterizer: PhantomData<fn() -> R>,
}

impl<R> RenderWorker<R>
where
    R: Rasterizer + Send + 'static,
{
    /// 启动工人（派生渲染线程，`rasterizer` 移入线程独占持有）——
    /// **无看门狗裸工人**（无引擎重建能力即无换代面；需要超时监控/
    /// 换代用 [`RenderWorker::watchdog`]）。
    pub fn new(rasterizer: R) -> Self {
        let (cmd_tx, cmd_rx) = channel::<WorkerCommand>();
        let (reply_tx, reply_rx) = channel::<WorkerReply>();
        let state = Arc::new(Mutex::new(WatchState::Idle));
        let join = thread::Builder::new()
            .name("mdc-pdf-render-worker".into())
            .spawn(move || worker_loop(rasterizer, cmd_rx, reply_tx, state))
            .expect("渲染工人线程派生失败（线程资源耗尽——fail-fast）");
        Self {
            tx: cmd_tx,
            rx: reply_rx,
            join: Some(join),
            next_job_id: 0,
            supervise: None,
            stashed: VecDeque::new(),
            in_flight: VecDeque::new(),
            pending_cancels: HashSet::new(),
            generation: 0,
            retired: Vec::new(),
            exhausted: false,
            _rasterizer: PhantomData,
        }
    }

    /// 看门狗 + 工厂构造（S3-L2 ❸弃置 + S4.2 工人重建，PDF-e 实装）。
    ///
    /// `factory` = 引擎重建闭包（**字节快照重开**——S4.4 纪律：闭包
    /// 持有打开时的字节快照，例如
    /// `move || PdfiumEngine::open(bytes.clone()).expect(...)`；首代与
    /// 替代代引擎都在渲染线程内构造——pdfium 单线程归属纪律不变）。
    /// 工厂 panic 语义（T2-fix）：工厂在渲染线程闭包内执行（含上述
    /// `.expect` 失败路径），panic **不在本构造/宿主调用帧内传播**——
    /// 先置看门狗终态再 resume：宿主收报得 `None`（通道断连——死亡
    /// 可观测、不悬挂、不泄漏看门狗）。
    /// 在跑作业超 `watchdog` → 该作业 [`WorkerReply::Timeout`] + 工人
    /// 毒化换代（在飞作业/未决取消令牌按提交序重放；旧线程断连弃置）。
    pub fn watchdog(factory: impl Fn() -> R + Send + Sync + 'static, watchdog: Duration) -> Self {
        let factory: Arc<dyn Fn() -> R + Send + Sync> = Arc::new(factory);
        let (tx, rx, join, retire_rx) = spawn_generation(&factory, watchdog);
        Self {
            tx,
            rx,
            join: Some(join),
            next_job_id: 0,
            supervise: Some(Supervise {
                factory,
                watchdog,
                retire_rx,
            }),
            stashed: VecDeque::new(),
            in_flight: VecDeque::new(),
            pending_cancels: HashSet::new(),
            generation: 0,
            retired: Vec::new(),
            exhausted: false,
            _rasterizer: PhantomData,
        }
    }

    /// 工人代次（0 起，每次看门狗换代 +1——观测面；公共调用入口先
    /// 处理待决换代事件，故返回值反映最新代；上限耗尽后停在
    /// [`MAX_GENERATIONS`] - 1——退役态事实由 [`Self::retired_alive`]
    /// / submit 合成回执可观测）。
    pub fn generation(&mut self) -> u64 {
        self.process_retirements();
        self.generation
    }

    /// 仍存活的退役代渲染线程数（#279 / 审计 P1-12 观测面）。
    ///
    /// 先处理待决换代事件，再收割已退出的退役线程（join 即回收），
    /// 返回**永挂未退**的计数——敌意反复超时场景下这是泄漏资源
    /// 的直接量度：上界恒 ≤ [`MAX_GENERATIONS`]（换代上限的机械
    /// 保证）。健康路径（旧代 C 调用返回后自然退出）恒归零。
    pub fn retired_alive(&mut self) -> usize {
        self.process_retirements();
        self.sweep_retired();
        self.retired.len()
    }

    /// 换代上限是否已耗尽（#279 观测面：true = 退役态——不再派生
    /// 新代，submit 即刻合成 [`WorkerReply::Timeout`]；宿主可据此
    /// 提示「渲染工人退役，重启会话可复位」）。
    pub fn is_exhausted(&mut self) -> bool {
        self.process_retirements();
        self.exhausted
    }

    /// 收割已退出的退役线程（finished → join 回收；永挂保留在案）。
    /// join 一个已结束线程立即返回——全程零阻塞，不影响停机不悬挂
    /// 语义。
    fn sweep_retired(&mut self) {
        let mut i = 0;
        while i < self.retired.len() {
            if self.retired[i].is_finished() {
                let handle = self.retired.remove(i);
                let _ = handle.join();
            } else {
                i += 1;
            }
        }
    }

    /// 提交渲染作业（异步；返回作业 id 供回执关联与取消）。
    ///
    /// 退役态（换代上限耗尽，#279）：不送通道（旧代已死/永挂），
    /// 在飞登记后**即刻合成 [`WorkerReply::Timeout`]**——宿主下一次
    /// 收报立即拿到回执（等待面不悬挂），语义与看门狗超时一致
    /// （❸弃置：结果永不采用；协议面零变化）。
    pub fn submit(&mut self, request: RenderRequest) -> u64 {
        self.process_retirements();
        let id = self.next_job_id;
        self.next_job_id += 1;
        if self.exhausted {
            self.in_flight.push_back((id, request));
            self.stashed.push_back(WorkerReply::Timeout { id });
            return id;
        }
        if self.supervise.is_some() {
            // 在飞跟踪（换代重放源——裸工人零跟踪零开销）。
            self.in_flight.push_back((id, request));
        }
        // 无界通道：send 只在工人停机后失败（drop 前后时序竞争）——
        // 丢弃即可，作业无回执（工人已死，drop 的 join 保证不悬挂）。
        let _ = self.tx.send(WorkerCommand::Job { id, request });
        id
    }

    /// 取消令牌（作用于尚未开跑的排队作业；已回执的 id 无效果）。
    pub fn cancel(&mut self, job_id: u64) {
        self.process_retirements();
        // 未决令牌登记（仅监督面——换代重放用；已回执作业的迟到令牌
        // 不登记：工人侧 last_replied/幽灵令牌剪枝已兜住）。
        if self.supervise.is_some() && self.in_flight.iter().any(|(id, _)| *id == job_id) {
            self.pending_cancels.insert(job_id);
        }
        let _ = self.tx.send(WorkerCommand::Cancel { id: job_id });
    }

    /// 非阻塞收报（无回执 None；工人线程死亡后同样 None）。
    pub fn try_recv(&mut self) -> Option<WorkerReply> {
        self.process_retirements();
        loop {
            let reply = match self.stashed.pop_front() {
                Some(reply) => reply,
                None => self.rx.try_recv().ok()?,
            };
            if let Some(reply) = self.accept(reply) {
                return Some(reply);
            }
        }
    }

    /// 限时收报（超时 None；语义与 `try_recv` 同——超时与工人死亡
    /// 不作区分，v0 观测面保持最小）。
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<WorkerReply> {
        self.process_retirements();
        // 预算分摊：迟到回执丢弃后继续等待**剩余**时间（一次调用的
        // 总预算不因内部丢弃而重置）。
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let reply = match self.stashed.pop_front() {
                Some(reply) => reply,
                None => match self.rx.recv_timeout(remaining) {
                    Ok(reply) => reply,
                    Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                        return None;
                    }
                },
            };
            if let Some(reply) = self.accept(reply) {
                return Some(reply);
            }
        }
    }

    /// 阻塞收报（工人线程死亡且通道排空后 None）。
    pub fn recv(&mut self) -> Option<WorkerReply> {
        self.process_retirements();
        loop {
            let reply = match self.stashed.pop_front() {
                Some(reply) => reply,
                None => self.rx.recv().ok()?,
            };
            if let Some(reply) = self.accept(reply) {
                return Some(reply);
            }
        }
    }

    /// 回执验收：有效（在飞作业首回执 / 裸工人无跟踪直通）→ 落账
    /// 放行；**已回执 id 的迟到回执**（换代竞态窗口内旧代完成的作业
    /// /重复回执）→ 静默丢弃返回 None（❸弃置的机械保证——结果永不
    /// 采用、不重复投递；调用方循环继续收报）。
    fn accept(&mut self, reply: WorkerReply) -> Option<WorkerReply> {
        let id = reply.job_id();
        let valid = self.supervise.is_none() || self.in_flight.iter().any(|(i, _)| *i == id);
        if valid {
            self.prune_replied(id);
            Some(reply)
        } else {
            None
        }
    }

    /// 作业回执落账（在飞/未决令牌移除——该 id 后续迟到回执走弃置）。
    fn prune_replied(&mut self, id: u64) {
        if self.supervise.is_some() {
            self.in_flight.retain(|(i, _)| *i != id);
            self.pending_cancels.remove(&id);
        }
    }

    /// 换代处理（公共调用入口先执行；无监督面或无待决事件 = 原样）。
    ///
    /// 步骤：① drain 旧代通道全部已到回执入 `stashed`（含看门狗
    /// `Timeout`——看门狗程序序先送回执再送事件，事件可见时回执必在
    /// 通道；已回执 id 的重复回执丢弃）→ ② **换代上限判定**（#279：
    /// 已达 [`MAX_GENERATIONS`] 代则退役收尾，不替代）→ ③ 旧代断连
    /// 弃置（句柄转入 `retired` 跟踪——永挂计数可观测、退出即收割）
    /// → ④ 工厂起替代工人（字节快照重开）→ ⑤ 在飞作业与未决取消
    /// 令牌按提交序重放（作业先于令牌——新代 FIFO 吸收路径同首播）。
    fn process_retirements(&mut self) {
        let (factory, watchdog, timed_out) = match self.supervise.as_ref() {
            // 无监督面：无换代。
            None => return,
            Some(sup) => match sup.retire_rx.try_recv() {
                // 无待决事件：当前代健康（一枪一代，队列深度 ≤ 1）。
                Err(_) => return,
                Ok(timed_out) => (Arc::clone(&sup.factory), sup.watchdog, timed_out),
            },
        };
        // ① 旧代回执 drain（提交序前缀入 stashed；重复/迟到丢弃）。
        while let Ok(reply) = self.rx.try_recv() {
            let id = reply.job_id();
            if self.in_flight.iter().any(|(i, _)| *i == id) {
                self.prune_replied(id);
                self.stashed.push_back(reply);
            }
        }
        // 防御：超时作业若仍无回执（理论不可达——看门狗程序序保证），
        // 合成 Timeout——「至多一条」不退化为零条（宿主等待面不悬挂）。
        if self.in_flight.iter().any(|(i, _)| *i == timed_out) {
            self.prune_replied(timed_out);
            self.stashed
                .push_back(WorkerReply::Timeout { id: timed_out });
        }
        // ② 换代上限（#279 / 审计 P1-12）：一生至多 MAX_GENERATIONS
        // 代——敌意反复超时的线程/引擎实例硬上界。达上限不再派生
        // 新代，退役收尾：在飞作业全部合成 Timeout（❸弃置语义，等待
        // 面不悬挂）；当前代句柄转入 retired 跟踪（永挂不 join——
        // 敌意 C 调用不得悬挂 drop）；通道换哑元断连（recv 面可观测
        // 死亡，send 静默失败——与工厂 panic 死亡同款观测语义）。
        if self.exhausted || self.generation + 1 >= MAX_GENERATIONS {
            // 在飞作业合成 Timeout（❸弃置语义——等待面不悬挂）。
            // **先 stash 后落账**：in_flight 保留至回执送达（accept 逐个
            // validate + prune——先清则合成回执被验收面当无效丢弃，
            // 「至多一条」退化为零条）。
            let ids: Vec<u64> = self.in_flight.iter().map(|(i, _)| *i).collect();
            for id in ids {
                self.stashed.push_back(WorkerReply::Timeout { id });
            }
            self.pending_cancels.clear();
            if let Some(join) = self.join.take() {
                self.retired.push(join);
            }
            let (_dead_tx, rx) = channel::<WorkerReply>();
            self.rx = rx;
            self.tx = channel::<WorkerCommand>().0;
            self.exhausted = true;
            return;
        }
        // ③ 旧代弃置：join 句柄转入 retired 跟踪（ detach 升级——退出
        // 即收割回收，永挂计数在案；敌意永挂 C 调用不得悬挂停机不变）；
        // 通道在 ④ 赋值时 drop（断连信号：旧代在跑作业完成后见断连
        // 即退，排队作业不再执行）。
        if let Some(join) = self.join.take() {
            self.retired.push(join);
        }
        self.sweep_retired();
        // ④ 替代工人（字节快照重开——S4.4）。
        let (tx, rx, join, retire_rx) = spawn_generation(&factory, watchdog);
        self.tx = tx;
        self.rx = rx;
        self.join = Some(join);
        if let Some(sup) = self.supervise.as_mut() {
            sup.retire_rx = retire_rx;
        }
        self.generation += 1;
        // ⑤ 在飞作业 + 未决取消令牌重放（提交序；令牌在作业后——
        // 新代 watermark 先覆盖再吸收）。
        let replay: Vec<(u64, RenderRequest)> = self.in_flight.drain(..).collect();
        let recancels: Vec<u64> = self.pending_cancels.iter().copied().collect();
        for (id, request) in replay {
            // RenderRequest 为 Copy——发送与回填各取一份（无堆分配）。
            let _ = self.tx.send(WorkerCommand::Job { id, request });
            self.in_flight.push_back((id, request));
        }
        for id in recancels {
            let _ = self.tx.send(WorkerCommand::Cancel { id });
        }
    }
}

impl<R> Drop for RenderWorker<R>
where
    R: Rasterizer + Send + 'static,
{
    /// 停机：先处理待决换代（若有——停的是健康新代；旧代已断连弃置
    /// 不等待），再发信号 + join **当前代**（在跑作业完成后线程退出；
    /// join 保证线程资源回收完成后 drop 才返回——停机不悬挂语义锁
    /// 背书）。退役态（换代上限耗尽，#279）：当前代句柄已在
    /// `retired`（永挂不 join——敌意 C 调用不得悬挂 drop，与旧代
    /// 弃置同纪律）；收割已退出的退役线程（零阻塞）。永挂线程留由
    /// OS 回收（进程级回收 = 架构级裁定，登记 Lead #279）。
    fn drop(&mut self) {
        self.process_retirements();
        let _ = self.tx.send(WorkerCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        self.sweep_retired();
    }
}

/// 派生一代工人（渲染线程 + 看门狗线程）——`watchdog` 构造与换代共用。
fn spawn_generation<R>(
    factory: &Arc<dyn Fn() -> R + Send + Sync>,
    watchdog: Duration,
) -> (
    Sender<WorkerCommand>,
    Receiver<WorkerReply>,
    JoinHandle<()>,
    Receiver<u64>,
)
where
    R: Rasterizer + Send + 'static,
{
    let (cmd_tx, cmd_rx) = channel::<WorkerCommand>();
    let (reply_tx, reply_rx) = channel::<WorkerReply>();
    let (retire_tx, retire_rx) = channel::<u64>();
    let state = Arc::new(Mutex::new(WatchState::Idle));
    // 看门狗回执通道克隆（回执通道本体移入渲染线程——克隆先行）。
    let watchdog_reply_tx = reply_tx.clone();
    let join = thread::Builder::new()
        .name("mdc-pdf-render-worker".into())
        .spawn({
            // 引擎在**渲染线程**闭包内构造（与模块文档同一纪律：首代
            // 与替代代同款——`R: Send` 所有权进入，pdfium thread_safe
            // 兜底跨线程移动；T2-fix 前工厂实在构造线程块内执行、与
            // 文档相反——本轮以代码向文档统一）。
            // 工厂 panic 语义（T2-review ❶ 处方，T2-fix）：catch_unwind
            // 捕获 → **先**置 state=Retired（看门狗见终态即退——不
            // 泄漏 10ms 轮询线程；其 reply_tx 克隆随线程退出 drop →
            // 回执通道发送端尽弃 → 断连 → 宿主 recv()/recv_timeout()
            // 返 None——死亡可观测、不悬挂）→ **再** resume_unwind
            // 传播（渲染线程 panic 死亡，drop 的 join 句柄可观测——
            // 不静默）。
            let state = Arc::clone(&state);
            let factory = Arc::clone(factory);
            move || {
                let rasterizer = match catch_unwind(AssertUnwindSafe(|| factory())) {
                    Ok(rasterizer) => rasterizer,
                    Err(payload) => {
                        *state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) = WatchState::Retired;
                        resume_unwind(payload);
                    }
                };
                worker_loop(rasterizer, cmd_rx, reply_tx, state)
            }
        })
        .expect("渲染工人线程派生失败（线程资源耗尽——fail-fast）");
    let _watchdog = thread::Builder::new()
        .name("mdc-pdf-render-watchdog".into())
        .spawn({
            let state = Arc::clone(&state);
            move || watchdog_loop(state, watchdog_reply_tx, retire_tx, watchdog)
        })
        .expect("看门狗线程派生失败（线程资源耗尽——fail-fast）");
    (cmd_tx, reply_rx, join, retire_rx)
}

/// 看门狗线程（一枪一代：触发或见终态即退）。轮询共享观测态——在跑
/// 作业自**开跑时刻**起超预算 → ① `Timeout` 回执（经当时代回执通道
/// ——阻塞收报的宿主即刻解锁）→ ② 换代事件（包装器在下一公共调用
/// 执行换代）。
fn watchdog_loop(
    state: Arc<Mutex<WatchState>>,
    reply_tx: Sender<WorkerReply>,
    retire_tx: Sender<u64>,
    watchdog: Duration,
) {
    loop {
        thread::sleep(WATCHDOG_POLL);
        let mut guard = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*guard {
            WatchState::Retired => return,
            WatchState::Idle => {}
            WatchState::Running { id, started } => {
                if started.elapsed() >= watchdog {
                    let id = *id;
                    // 毒化终态（工人线程后续赋值仅发生在 C 调用返回后
                    // ——此时本看门狗已退出，无观测者，覆写无害）。
                    *guard = WatchState::Retired;
                    drop(guard);
                    // ① 回执先于事件（程序序：包装器见事件时回执必在
                    // 通道——drain 兜底送达）。
                    let _ = reply_tx.send(WorkerReply::Timeout { id });
                    // ② 换代事件。
                    let _ = retire_tx.send(id);
                    return;
                }
            }
        }
    }
}

/// 渲染线程主循环：内部待跑队列 + 命令通道 + 看门狗观测态更新。
///
/// 执行队首作业前**非阻塞排空命令通道**（吸收取消令牌/追加后续作业/
/// 响应停机）——保证队首开跑前已见过全部已送达取消（FIFO 下 cancel
/// 消息必晚于其作业消息，不排空则取消永不可见）。**通道断连 = 退役**
/// （换代后旧代：立即退出，排队作业不再执行——新代已接管重放；正常
/// 停机走显式 Shutdown 消息）。
///
/// `last_replied` 单调（回执 id 递增；`Option`：None = 尚未回执过任何
/// 作业——下界哨兵不与合法 id 0 撞值，T1 审计 F1 修复：旧实现以 0
/// 为哨兵，首作业 id 0 的排队取消令牌必被 `0 < 0` 误弃）：取消集以
/// 它剪枝（已回执作业的迟到取消令牌直接丢弃）；`job_watermark` =
/// 迄今见过的最大作业 id，幽灵令牌防御（pdf-c-1r 审计轻微项）：
/// id > watermark = 调用方取消了从未提交的作业（调用方 bug），
/// 丢弃不入驻——集合上界 = 在飞作业数，不随会话泄漏。
fn worker_loop<R>(
    rasterizer: R,
    cmd_rx: Receiver<WorkerCommand>,
    reply_tx: Sender<WorkerReply>,
    state: Arc<Mutex<WatchState>>,
) where
    R: Rasterizer + Send,
{
    worker_loop_inner(rasterizer, cmd_rx, reply_tx, &state);
    // 全退出路径统一终态（看门狗随之退出——不泄漏轮询线程）。
    *state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = WatchState::Retired;
}

fn worker_loop_inner<R>(
    mut rasterizer: R,
    cmd_rx: Receiver<WorkerCommand>,
    reply_tx: Sender<WorkerReply>,
    state: &Mutex<WatchState>,
) where
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
        // 就绪：排空命令通道（取消令牌先于队首开跑生效）。断连 =
        // 退役（见函数文档）——排队作业静默终止（无回执——通道关闭
        // 可见，新代已接管重放）。
        loop {
            match cmd_rx.try_recv() {
                Ok(WorkerCommand::Job { id, request }) => {
                    job_watermark = job_watermark.max(id);
                    pending.push_back((id, request));
                }
                Ok(WorkerCommand::Cancel { id }) => {
                    absorb_cancel(&mut cancelled, id, last_replied, job_watermark)
                }
                // 停机优先：排队作业静默终止（无回执——通道关闭可见）。
                Ok(WorkerCommand::Shutdown) => return,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let (id, request) = pending.pop_front().expect("pending 非空（分支条件）");
        if cancelled.remove(&id) {
            // 开跑前命中取消令牌——跳过执行（零渲染调用）。
            let _ = reply_tx.send(WorkerReply::Cancelled { id });
            last_replied = Some(id);
            continue;
        }
        // 看门狗观测：作业开跑（短锁 + 纯赋值——锁内不可 panic）。
        *state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = WatchState::Running {
            id,
            started: Instant::now(),
        };
        // panic 隔离（S4.2）：AssertUnwindSafe 的理由——&mut R 跨
        // unwind 后无法证明一致性，但 pdfium 句柄在 C 堆（Rust
        // unwind 不触碰），后续作业继续尝试（工人存活语义锁）。
        let result = catch_unwind(AssertUnwindSafe(|| rasterizer.rasterize(request)))
            .unwrap_or(Err(CommandError::RenderPanic));
        *state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = WatchState::Idle;
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
