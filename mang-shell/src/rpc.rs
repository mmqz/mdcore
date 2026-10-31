//! rpc —— 本机控制通道（#104 任务 2「RPC 桥接」+ #111 任务 2「RPC 扩展」）。
//!
//! ## 定位：Lead 长期设计「AI 操作运行中的 UI，实时反馈」的地基
//!
//! #109 打通了「请求-响应」链路（6 命令最小版）；#111 在其上扩展：
//! **订阅模式**——客户端 `subscribe` 后，服务端在每次文档变更（用户
//! 编辑 / AI 编辑 / 撤销重做）时**主动推送** `doc_changed` 事件，双向
//! 回显即成：AI 发命令改文档 → 用户界面实时看到；用户手动编辑 →
//! AI 通过订阅收到通知。两边走同一个事件流（[`crate::doc_events`]）。
//!
//! ## 架构（#111：响应与事件同一条出口）
//!
//! ```text
//! 外部客户端（python/nc …）                mang-shell 进程
//! ─────────────────────                  ────────────────────────────
//!   AF_UNIX socket  ──JSON 行──▶ accept 循环 ──▶ Job 队列（带 conn id）
//!   （每连接一个任务）                        │
//!                                            ▼
//!                                 dispatcher（单任务串行，主线程执行）
//!                                 cx.update → window.update
//!                                     │                │（编辑 → doc_events
//!   ◀──JSON 行（响应）─── out 通道 ◀────┘                │  总线 → 事件泵）
//!   ◀──JSON 行（事件）─── out 通道 ◀── 事件泵（订阅表广播）
//! ```
//!
//! - 每连接一条 `out` 通道（多生产者：dispatcher 写响应 / 事件泵写
//!   事件；单消费者：connection 任务内 `select` 写 socket）——**响应
//!   与事件同出口，天然保序**；多客户端各自独立通道互不干扰。
//! - `select` 读写交替（`futures_lite::future::select`）：读 future 半途
//!   被事件分支抢先而丢弃是安全的——数据留内核缓冲，下轮照读。
//! - **传输**：unix domain socket，JSON Lines（一行一 JSON，UTF-8）。
//!   本机通信——不做加密/认证/网络（#104/#111 明确边界）。
//! - **端点**：`$XDG_RUNTIME_DIR/mang-shell.sock`，缺省
//!   `/tmp/mang-shell-$UID.sock`；`--rpc-sock PATH` 显式覆盖（烟雾与
//!   多实例场景）；`--no-rpc` 关闭。
//! - **退出清理**：`ControlServer` Drop 时删除 socket 文件；异常退出
//!   的残留由下次启动 [`prepare_socket`] 探活清理（连得上 = 别的实例
//!   在监听 → 本实例禁用 RPC；连不上 = 陈旧文件 → 删除后重绑）。
//!
//! ## 协议（#111 后 15 命令 + 事件流）
//!
//! 请求（`id` 可省——省略时响应 `id` 为 null；未知字段忽略——前向
//! 兼容）。#104 六命令（open_file / insert_text / replace_selection /
//! get_state / close_file / switch_tab）之上，#111 任务 2 新增：
//!
//! ```json
//! {"id": 7,  "cmd": "set_cursor",     "line": 10, "col": 5}
//! {"id": 8,  "cmd": "set_selection",  "start": {"line": 0, "col": 1},
//!                                        "end":   {"line": 0, "col": 5}}
//! {"id": 9,  "cmd": "get_selection"}
//! {"id": 10, "cmd": "undo"}
//! {"id": 11, "cmd": "redo"}
//! {"id": 12, "cmd": "save"}
//! {"id": 13, "cmd": "save_as",        "path": "/tmp/b.md"}
//! {"id": 14, "cmd": "scroll_to",      "line": 42}
//! {"id": 15, "cmd": "subscribe"}
//! ```
//!
//! 响应（恒一行 JSON；`ok` 分支二选一 `result`/`error`）：
//!
//! ```json
//! {"id": 2, "ok": true,  "result": {"cursor": {"line": 0, "col": 6}, ...}}
//! {"id": 5, "ok": false, "error": "文档有未保存修改——先保存再关闭"}
//! ```
//!
//! ## 事件流（订阅后服务端主动推）
//!
//! ```json
//! {"type": "doc_changed", "path": "/tmp/a.md", "revision_before": 41,
//!  "revision": 42, "source": "user", "changes": [
//!    {"kind": "replace", "start": {"line": 10, "col": 5},
//!     "end": {"line": 10, "col": 5}, "text": "hi"}]}
//! ```
//!
//! 语义详见 [`crate::doc_events`]（事件源头与字段口径）；端到端断言
//! 见 `tests/rpc_client.py`（订阅 / 双向回显 / 多客户端互不干扰）。
//!
//! ## 命令语义要点（#111 新面）
//!
//! - `set_cursor`/`set_selection`/`scroll_to`/`get_selection`：导航面
//!   ——只读文档同样可用（浏览场景）；越界 clamp 到文内最近点。
//! - `undo`/`redo`：与 `Ctrl+Z`/`Ctrl+Y` 同链（多光标批合并）；栈空
//!   不是错误（`undid`/`redid` = false）。
//! - `save`：未命名文档拒绝（无路径——引导 `save_as`）；只读渲染态
//!   拒绝（无保存语义）。`save_as`：绝对化后写盘 + 标签身份迁移。
//! - `subscribe`：幂等（重复订阅替换旧通道）；断连自动注销（事件泵
//!   send 失败即移除）。
//!
//! ## 验证
//!
//! - 单测：协议解析/序列化、事件行形态、订阅表逻辑（文件尾 mod tests）。
//! - 端到端：`rpc_smoke.sh`（Xvfb + `tests/rpc_client.py`——15 命令
//!   全链 + 订阅/双向回显（`--smoke-user-type` 合成用户键入路径）+
//!   双客户端并发）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_net::unix::{UnixListener, UnixStream};
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};
use serde::{Deserialize, Serialize};

use gpui::{App, AppContext, AsyncApp, Context, Task, Window, WindowHandle};

use crate::editor::buffer::Point;
use crate::shell::ShellApp;

/// 单行请求字节上限（1 MiB）——防无换行洪水把内存吃爆；超限回错误
/// 并断连（客户端侧修包重连）。
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// accept 连续失败上限——不可恢复错误（如 fd 耗尽）下停止服务打日志，
/// 不无限刷屏。
const ACCEPT_ERROR_LIMIT: u32 = 32;

// ---------------------------------------------------------------------------
// 协议面（纯数据 + 纯函数——单测主战场）
// ---------------------------------------------------------------------------

/// 控制命令（#104 六命令 + #111 任务 2 八命令 + 订阅）。
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "cmd")]
pub enum ControlCommand {
    /// 打开文件（已开 → 激活既有标签，不重复开）。
    #[serde(rename = "open_file")]
    OpenFile { path: String },

    /// 在指定行列插入文本（行列 0 基；越界由 Buffer 钳制到文内最近点）。
    #[serde(rename = "insert_text")]
    InsertText {
        line: usize,
        col: usize,
        text: String,
    },

    /// 替换当前选区（无选区 = 光标处插入；空文本 + 有选区 = 删除选区）。
    #[serde(rename = "replace_selection")]
    ReplaceSelection { text: String },

    /// 获取当前状态（标签清单 + 活动文档光标/选区/字数）。
    #[serde(rename = "get_state")]
    GetState,

    /// 关闭指定文件的标签（脏标签拒绝——先保存）。
    #[serde(rename = "close_file")]
    CloseFile { path: String },

    /// 切换到指定标签（索引 0 基）。
    #[serde(rename = "switch_tab")]
    SwitchTab { index: usize },

    // ---------------- #111 任务 2：8 新命令 + 订阅 ----------------
    /// 设置光标位置（0 基；越界钳制。只读文档可用——导航面）。
    #[serde(rename = "set_cursor")]
    SetCursor { line: usize, col: usize },

    /// 设置选区（两点任意次序，排序后 anchor/cursor；越界钳制）。
    #[serde(rename = "set_selection")]
    SetSelection { start: PointWire, end: PointWire },

    /// 获取当前选区（含文本——AI 上下文消费主通道；无选区时 result
    /// 里 `selection` 为 null）。
    #[serde(rename = "get_selection")]
    GetSelection,

    /// 撤销一步手势（同 `Ctrl+Z`——多光标批合并；栈空不是错误）。
    #[serde(rename = "undo")]
    Undo,

    /// 重做一步（同 `Ctrl+Y`）。
    #[serde(rename = "redo")]
    Redo,

    /// 保存当前文件（未命名/只读拒绝——引导 save_as / 明确无语义）。
    #[serde(rename = "save")]
    Save,

    /// 另存为（绝对化后写盘 + 标签身份迁移——路径/display_name 更新）。
    #[serde(rename = "save_as")]
    SaveAs { path: String },

    /// 滚动到指定行（纯视口移动——不动光标；只读文档可用）。
    #[serde(rename = "scroll_to")]
    ScrollTo { line: usize },

    /// 订阅文档变更事件（后续每次变更服务端主动推 `doc_changed`；
    /// 幂等；断连自动注销）。
    #[serde(rename = "subscribe")]
    Subscribe,
}

/// 点位线上形态（`{"line": n, "col": n}`——与响应 `point_json` 同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct PointWire {
    pub line: usize,
    pub col: usize,
}

impl PointWire {
    fn into_point(self) -> Point {
        Point::new(self.line, self.col)
    }
}

impl ControlCommand {
    /// 命令名（证据行/日志面——与线上 `cmd` 字段一致）。
    pub fn name(&self) -> &'static str {
        match self {
            ControlCommand::OpenFile { .. } => "open_file",
            ControlCommand::InsertText { .. } => "insert_text",
            ControlCommand::ReplaceSelection { .. } => "replace_selection",
            ControlCommand::GetState => "get_state",
            ControlCommand::CloseFile { .. } => "close_file",
            ControlCommand::SwitchTab { .. } => "switch_tab",
            ControlCommand::SetCursor { .. } => "set_cursor",
            ControlCommand::SetSelection { .. } => "set_selection",
            ControlCommand::GetSelection => "get_selection",
            ControlCommand::Undo => "undo",
            ControlCommand::Redo => "redo",
            ControlCommand::Save => "save",
            ControlCommand::SaveAs { .. } => "save_as",
            ControlCommand::ScrollTo { .. } => "scroll_to",
            ControlCommand::Subscribe => "subscribe",
        }
    }
}

/// 响应（`ok` 分支二选一：`result` 或 `error`，未用的键不序列化）。
#[derive(Debug, Serialize)]
pub struct ControlResponse {
    pub id: Option<u64>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ControlResponse {
    fn ok(id: Option<u64>, result: serde_json::Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Option<u64>, error: String) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error),
        }
    }

    /// 序列化（Value 面恒成功；兜底仅防未知值形态）。
    fn to_line(&self) -> String {
        let mut s = serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"id":null,"ok":false,"error":"serialize failed"}"#.into());
        s.push('\n');
        s
    }
}

/// 解析一行请求。返回 `(id 回显, 命令或错误消息)`——JSON 整体损坏时
/// id 取不到（None），命令字段错但 JSON 合法时 id 可回显（客户端能对
/// 上是哪条失败）。
pub fn parse_command(line: &str) -> (Option<u64>, Result<ControlCommand, String>) {
    // 两步走：先宽容取 id（整体 Value），再严格反序列化命令（内部
    // 标签枚举直接吃整对象——未知键自动忽略）。
    let id = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("id").and_then(|i| i.as_u64()));
    let parsed = serde_json::from_str::<ControlCommand>(line).map_err(|e| e.to_string());
    (id, parsed)
}

// ---------------------------------------------------------------------------
// 端点解析与 socket 预备（纯函数面——单测）
// ---------------------------------------------------------------------------

/// 端点解析（纯函数核心——env 读取在 [`resolve_socket_path`] 薄壳）。
///
/// 优先级：显式覆盖 > `$XDG_RUNTIME_DIR`（XDG 规范要求绝对路径，
/// 相对值忽略）> `/tmp/mang-shell-$UID.sock`（无 uid 信息时退
/// `/tmp/mang-shell.sock`——单用户机兜底）。
pub fn default_socket_path(
    explicit: Option<PathBuf>,
    xdg_runtime: Option<PathBuf>,
    uid: Option<u32>,
) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(dir) = xdg_runtime {
        return dir.join("mang-shell.sock");
    }
    match uid {
        Some(uid) => PathBuf::from(format!("/tmp/mang-shell-{uid}.sock")),
        None => PathBuf::from("/tmp/mang-shell.sock"),
    }
}

/// 端点解析（进程面薄壳——读 env 与 /proc 属主 uid）。
pub fn resolve_socket_path(explicit: Option<&str>) -> PathBuf {
    default_socket_path(
        explicit.map(PathBuf::from),
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute()),
        process_uid(),
    )
}

/// 本进程 uid（std-only 口径：/proc/self 属主即进程 uid——免引 libc）。
fn process_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// socket 预备结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepared {
    /// 可绑（路径不存在，或陈旧残留已清）。
    Ready,
    /// 路径上有活实例在监听——本实例应禁用 RPC。
    Busy,
}

/// socket 预备：探活 + 清残留。
///
/// - 路径不存在 → `Ready`。
/// - 存在且能连通 → `Busy`（另一实例在监听）。
/// - 存在但连不通 → 陈旧残留（上次异常退出）→ 删除后 `Ready`。
/// - 存在但不是 socket 文件（普通文件/目录）→ 报错不动它（防
///   `--rpc-sock` 误指重要文件被删）。
pub fn prepare_socket(path: &Path) -> std::io::Result<Prepared> {
    use std::os::unix::fs::FileTypeExt;

    if !path.exists() {
        return Ok(Prepared::Ready);
    }
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        return Ok(Prepared::Busy);
    }
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_socket() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("路径已存在且不是 socket 文件：{}", path.display()),
        ));
    }
    std::fs::remove_file(path)?;
    Ok(Prepared::Ready)
}

// ---------------------------------------------------------------------------
// 行分帧（连接读缓冲——纯逻辑，单测钉定）
// ---------------------------------------------------------------------------

/// 字节流 → 行分帧器（feed 追加；next_line 吐完整行；overflowed 防洪水）。
struct LineAssembler {
    buf: Vec<u8>,
    cap: usize,
}

impl LineAssembler {
    fn new(cap: usize) -> Self {
        Self {
            buf: Vec::new(),
            cap,
        }
    }

    fn feed(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// 弹出下一条完整行（到 `\n`，含 `\r\n` 归一）；无完整行 `None`。
    /// 行内容 UTF-8 有损解码（坏字节不炸连接）。
    fn next_line(&mut self) -> Option<String> {
        let pos = self.buf.iter().position(|&b| b == b'\n')?;
        let line: Vec<u8> = self.buf.drain(..=pos).collect();
        let mut line = &line[..line.len() - 1]; // 去 \n
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        Some(String::from_utf8_lossy(line).into_owned())
    }

    /// 待处理字节超上限（首个未完行已超 cap）。
    fn overflowed(&self) -> bool {
        self.buf.len() > self.cap
    }

    /// EOF 尾包：无换行的末行（宽容客户端不补换行）。
    fn take_remainder(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let line = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        Some(line)
    }
}

// ---------------------------------------------------------------------------
// 服务面
// ---------------------------------------------------------------------------

/// 一条待执行命令（dispatcher 投递单元）。
///
/// `reply` = 该连接的 out 通道发送端（#111：响应不再 per-command
/// oneshot——与事件流同一条出口，单 writer 消费保序）；`conn` = 连接
/// id（`subscribe` 注册订阅的键；断连注销的键）。
struct Job {
    id: Option<u64>,
    conn: u64,
    command: ControlCommand,
    reply: async_channel::Sender<String>,
}

/// 订阅表（conn id → 该连接 out 通道发送端）。
///
/// dispatcher（Subscribe 命令时写入）与事件泵（每次文档变更时遍历
/// 广播）共享；Mutex 短临界区（clone sender + send 都是非阻塞面）。
/// send 失败（对端已断）即移除——断连自动注销。
type Subscriptions =
    std::sync::Mutex<std::collections::HashMap<u64, async_channel::Sender<String>>>;

/// 注册订阅（幂等：同 conn id 重复订阅替换旧通道）。
fn subscribe_insert(subs: &Subscriptions, conn: u64, out: async_channel::Sender<String>) {
    subs.lock().expect("订阅表锁中毒").insert(conn, out);
}

/// 广播一条已序列化的事件行（send 失败即移除订阅——断连自动注销）。
/// 返回实际送达的订阅者数。
fn subscribe_broadcast(subs: &Subscriptions, line: &str) -> usize {
    let mut guard = subs.lock().expect("订阅表锁中毒");
    let mut delivered = 0;
    let mut dead: Vec<u64> = Vec::new();
    for (conn, out) in guard.iter() {
        // try_send：unbounded 通道发送不阻塞——失败即对端已断。
        if out.try_send(line.to_string()).is_ok() {
            delivered += 1;
        } else {
            dead.push(*conn);
        }
    }
    for conn in dead {
        guard.remove(&conn);
        eprintln!("rpc: 订阅注销（conn {conn} 对端已断）");
    }
    delivered
}

/// 控制服务句柄。存活周期 = 应用全局（GPUI `Global`）——注意 `run`
/// 闭包只是 setup，闭包返回后事件循环才开跑，局部变量会在 setup 尾
/// 被 Drop 导致 socket 立即解除链接，**必须**挂全局存到 App 退出；
/// Drop 时清 socket 文件（正常退出不留残；异常退出的残留由下次启动
/// [`prepare_socket`] 探活清理）。
pub struct ControlServer {
    path: PathBuf,
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl gpui::Global for ControlServer {}

impl ControlServer {
    /// 启动控制服务：预备 socket → 绑定 → 派生 accept 循环、
    /// dispatcher 与事件泵（均为 detached 任务，随应用退出消亡）→
    /// **挂应用全局**（存活到 App 退出——见类型文档的 Drop 陷阱）。
    ///
    /// #111：成功时初始化 [`crate::doc_events`] 总线并启动事件泵
    ///（总线接收端 → 序列化 → 订阅表广播）——「订阅模式」的服务端面。
    ///
    /// 失败不致命（返回 `false`，应用照常跑）：端点被占（另一实例）/
    /// 绑定失败/预备失败只损失 RPC 能力，桌面外壳本体不受影响。
    /// 成功返回 `true`。
    pub fn start(path: PathBuf, window: WindowHandle<ShellApp>, cx: &mut App) -> bool {
        match prepare_socket(&path) {
            Ok(Prepared::Ready) => {}
            Ok(Prepared::Busy) => {
                eprintln!(
                    "rpc: 端点被占用（{}）——另一实例在监听，本实例 RPC 禁用",
                    path.display()
                );
                return false;
            }
            Err(err) => {
                eprintln!("rpc: socket 预备失败（{}）：{err}", path.display());
                return false;
            }
        }
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("rpc: 绑定失败（{}）：{err}", path.display());
                return false;
            }
        };
        let (tx, rx) = async_channel::unbounded::<Job>();
        let subs: Arc<Subscriptions> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        // #111：事件总线初始化（单接收端契约）+ 事件泵（detached）。
        // 失败（已被初始化）只损失事件面——命令面照常。
        match crate::doc_events::init_bus() {
            Some(bus_rx) => {
                let pump_subs = subs.clone();
                cx.spawn(async move |cx| event_pump(bus_rx, pump_subs, cx).await)
                    .detach();
                eprintln!("rpc: 事件泵已启动（订阅模式就绪）");
            }
            None => eprintln!("rpc: 事件总线已被占用——订阅模式不可用（命令面照常）"),
        }
        // WindowHandle 实现 Copy——直传免 clone（clippy::clone_on_copy）。
        cx.spawn(async move |cx| dispatcher_loop(rx, window, subs, cx).await)
            .detach();
        cx.spawn(async move |cx| accept_loop(listener, tx, cx).await)
            .detach();
        eprintln!("rpc: listening at {}", path.display());
        // 挂应用全局（存活到 App 退出——run 闭包返回后事件循环开跑，
        // 局部绑定会在 setup 尾被 Drop 把 socket 立即解除链接）。
        cx.set_global(Self { path });
        true
    }
}

/// accept 循环：每连接一个后台任务（收发不互阻——多客户端并发基础）。
/// #111：每连接分配递增 conn id（订阅键）+ 独立 out 通道（响应与
/// 事件同出口）。
async fn accept_loop(listener: UnixListener, tx: async_channel::Sender<Job>, cx: &mut AsyncApp) {
    let live = Arc::new(AtomicUsize::new(0));
    let next_conn = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let mut errors: u32 = 0;
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                errors = 0;
                let live_now = live.fetch_add(1, Ordering::SeqCst) + 1;
                let conn = next_conn.fetch_add(1, Ordering::SeqCst);
                eprintln!("rpc: client connected（conn {conn}，{live_now} 路）");
                let tx = tx.clone();
                let live = live.clone();
                let task: Task<()> = cx.background_spawn(async move {
                    connection_task(stream, tx, conn).await;
                    let live_now = live.fetch_sub(1, Ordering::SeqCst) - 1;
                    eprintln!("rpc: client disconnected（conn {conn}，{live_now} 路）");
                });
                task.detach();
            }
            Err(err) => {
                errors += 1;
                eprintln!("rpc: accept 失败（{errors}/{ACCEPT_ERROR_LIMIT}）：{err}");
                if errors >= ACCEPT_ERROR_LIMIT {
                    eprintln!("rpc: accept 连续失败达上限——控制服务停止");
                    return;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
            }
        }
    }
}

/// 单连接任务：`race` 交替「读 socket（命令）」与「写 out 通道消费
///（响应/事件）」。
///
/// #111 关键改造（原：读行 → oneshot 等响应 → 回写）：
/// - 响应不再被读循环等待——dispatcher 执行完直接 send 到本连接的
///   out 通道（fire-and-forget）；读循环可以连续投递命令。
/// - 事件泵也向同一 out 通道 send——**响应与事件同一出口、单消费
///   者写出，天然保序**（async_channel FIFO）。
/// - `race`（futures-lite 无 `select`——同输出型双 future 竞速替代）
///   先完成者赢，另一个被 drop：读 future 半途被事件分支抢先而丢弃
///   是安全的——数据留内核缓冲，下轮照读（out 通道的 recv 半途丢弃
///   同样安全——async_channel 消息未取走不丢失）。
async fn connection_task(mut stream: UnixStream, tx: async_channel::Sender<Job>, conn: u64) {
    /// race 双分支统一输出（futures-lite 无 select/Completed——同型
    /// 竞速面）。
    enum Step {
        Read(std::io::Result<usize>),
        Out(Result<String, async_channel::RecvError>),
    }
    let (out_tx, out_rx) = async_channel::unbounded::<String>();
    let mut asm = LineAssembler::new(MAX_LINE_BYTES);
    let mut chunk = [0u8; 8192];
    loop {
        let read_step = async { Step::Read(stream.read(&mut chunk).await) };
        let out_step = async { Step::Out(out_rx.recv().await) };
        match futures_lite::future::race(read_step, out_step).await {
            Step::Read(read) => match read {
                Ok(0) => break, // EOF
                Ok(n) => {
                    asm.feed(&chunk[..n]);
                    while let Some(line) = asm.next_line() {
                        // 投递不等待响应（响应走 out 通道）；写失败
                        //（对端断）由后续轮次的事件写出面发现。
                        handle_line(&line, &tx, &out_tx, conn);
                    }
                    if asm.overflowed() {
                        let resp = ControlResponse::err(
                            None,
                            format!("单行超过 {MAX_LINE_BYTES} 字节上限——连接关闭"),
                        );
                        let _ = out_tx.send(resp.to_line()).await;
                        return;
                    }
                }
                Err(_) => break, // 读错（对端异常断）——静默收工
            },
            Step::Out(out) => match out {
                Ok(line) => {
                    if stream.write_all(line.as_bytes()).await.is_err() {
                        return; // 对端已断——写失败即收工
                    }
                }
                Err(_) => {
                    // out 通道关闭（所有 sender 已 drop）——但本任务还持
                    // 有 out_tx，不可达；防御早退。
                    return;
                }
            },
        }
    }
    // EOF 尾包：最后一条没带换行的行也处理（宽容 netcat/echo 习惯）。
    if let Some(line) = asm.take_remainder() {
        let (reply_tx, _reply_rx) = async_channel::bounded::<String>(1);
        handle_line(&line, &tx, &reply_tx, conn);
    }
    // EOF 后排水剩余响应（尽力而为——对端可能不再读；写错即止）。
    while let Ok(line) = out_rx.try_recv() {
        if stream.write_all(line.as_bytes()).await.is_err() {
            break;
        }
    }
}

/// 单行处理：空行忽略；解析失败回错（走 out 通道）；成功投递
/// dispatcher（不等响应——#111 fire-and-forget）。
fn handle_line(
    line: &str,
    tx: &async_channel::Sender<Job>,
    out: &async_channel::Sender<String>,
    conn: u64,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let (id, parsed) = parse_command(line);
    match parsed {
        Ok(command) => {
            let _ = tx.try_send(Job {
                id,
                conn,
                command,
                reply: out.clone(),
            });
        }
        Err(err) => {
            let resp = ControlResponse::err(id, err);
            let _ = out.try_send(resp.to_line());
        }
    }
}

/// #111 事件泵：文档事件总线接收端 → 序列化 JSON 行 → 订阅表广播。
///（detached 任务，随应用退出消亡；总线接收端 drop 即 publish 面静默
/// 落空——不影响编辑链路。）
async fn event_pump(
    bus: async_channel::Receiver<std::sync::Arc<crate::doc_events::DocEvent>>,
    subs: Arc<Subscriptions>,
    _cx: &mut AsyncApp,
) {
    while let Ok(event) = bus.recv().await {
        let mut line = match serde_json::to_string(event.as_ref()) {
            Ok(s) => s,
            Err(_) => continue, // 序列化失败只丢这一条（不可达——Value 面恒成功）
        };
        line.push('\n');
        let delivered = subscribe_broadcast(&subs, &line);
        eprintln!(
            "rpc: doc-event push（rev {}，{} 订阅者）",
            event.revision, delivered
        );
    }
}

/// dispatcher：单任务串行——命令全部落主线程执行（GPUI 实体修改的
/// 正确性保证；多客户端命令在此天然排队，互不穿插半执行）。
///
/// #111：执行期间置 [`crate::doc_events::mark_rpc_source`]（RAII）——
/// 编辑命令触发的文档变更事件 `source = "rpc"`；用户键入路径无此标记
///（`source = "user"`）。响应回写与订阅注册共用 Job.reply。
async fn dispatcher_loop(
    rx: async_channel::Receiver<Job>,
    window: WindowHandle<ShellApp>,
    subs: Arc<Subscriptions>,
    cx: &mut AsyncApp,
) {
    while let Ok(job) = rx.recv().await {
        // #111 Subscribe 特判：纯通道注册（不碰 GPUI 实体，无需落
        // 主线程）——把本连接 out 通道挂进订阅表，响应与后续事件同一
        // 出口。注册即达——幂等（同 conn id 替换旧通道）。
        if matches!(job.command, ControlCommand::Subscribe) {
            subscribe_insert(&subs, job.conn, job.reply.clone());
            eprintln!(
                "rpc: exec subscribe ok=true（conn {}，现 {} 订阅者）",
                job.conn,
                subs.lock().expect("订阅表锁中毒").len()
            );
            let resp = ControlResponse::ok(job.id, serde_json::json!({ "subscribed": true }));
            let _ = job.reply.send(resp.to_line()).await;
            continue;
        }
        let executed = {
            let _rpc_source = crate::doc_events::mark_rpc_source();
            cx.update(|cx| {
                window
                    .update(cx, |shell, window, cx| {
                        execute_command(shell, window, cx, &job.command)
                    })
                    .unwrap_or_else(|err| Err(format!("窗口不可用：{err}")))
            })
        };
        let resp = match executed {
            Ok(result) => {
                eprintln!("rpc: exec {} ok=true", job.command.name());
                ControlResponse::ok(job.id, result)
            }
            Err(err) => {
                eprintln!("rpc: exec {} ok=false（{err}）", job.command.name());
                ControlResponse::err(job.id, err)
            }
        };
        let _ = job.reply.send(resp.to_line()).await;
    }
}

// ---------------------------------------------------------------------------
// 命令执行（主线程——ShellApp/Editor 公开面上的薄编排）
// ---------------------------------------------------------------------------

/// 执行一条命令（主线程内调用——`window.update` 闭包体）。
///
/// 编辑类命令与用户键入同链（Buffer::replace_range 单点 + undo 一条 +
/// after_edit 收尾）——「AI 操作，UI 实时反馈」走的正是同一渲染管线。
fn execute_command(
    shell: &mut ShellApp,
    window: &mut Window,
    cx: &mut Context<ShellApp>,
    command: &ControlCommand,
) -> Result<serde_json::Value, String> {
    match command {
        ControlCommand::OpenFile { path } => cmd_open_file(shell, window, cx, path),
        ControlCommand::InsertText { line, col, text } => {
            cmd_insert_text(shell, cx, *line, *col, text)
        }
        ControlCommand::ReplaceSelection { text } => cmd_replace_selection(shell, cx, text),
        ControlCommand::GetState => Ok(state_json(shell, cx)),
        ControlCommand::CloseFile { path } => cmd_close_file(shell, window, cx, path),
        ControlCommand::SwitchTab { index } => cmd_switch_tab(shell, window, cx, *index),
        // ---------------- #111：导航面（只读文档可用） ----------------
        ControlCommand::SetCursor { line, col } => cmd_set_cursor(shell, cx, *line, *col),
        ControlCommand::SetSelection { start, end } => cmd_set_selection(shell, cx, *start, *end),
        ControlCommand::GetSelection => cmd_get_selection(shell, cx),
        ControlCommand::ScrollTo { line } => cmd_scroll_to(shell, cx, *line),
        // ---------------- #111：历史面 ----------------
        ControlCommand::Undo => cmd_undo(shell, cx),
        ControlCommand::Redo => cmd_redo(shell, cx),
        // ---------------- #111：保存面 ----------------
        ControlCommand::Save => cmd_save(shell, cx),
        ControlCommand::SaveAs { path } => cmd_save_as(shell, cx, path),
        // #111 Subscribe 在 dispatcher_loop 内特判（纯通道注册，不经
        // 主线程）——此分支不可达，但 match 穷尽性要求列出。
        ControlCommand::Subscribe => unreachable!("subscribe 在 dispatcher 侧特判"),
    }
}

fn cmd_open_file(
    shell: &mut ShellApp,
    window: &mut Window,
    cx: &mut Context<ShellApp>,
    path: &str,
) -> Result<serde_json::Value, String> {
    let path = std::path::absolute(Path::new(path)).unwrap_or_else(|_| PathBuf::from(path));
    // 已开 → 激活既有标签（与 open_document_tab 语义一致；先探便于
    // 响应区分 already_open）。
    if let Some(idx) = shell.tab_index_by_path(&path) {
        shell.activate_tab(idx, window, cx);
        return Ok(serde_json::json!({
            "tab_index": idx,
            "already_open": true,
        }));
    }
    match shell.open_document_tab(path.clone(), cx) {
        Some(focus) => {
            window.focus(&focus, cx);
            Ok(serde_json::json!({
                "tab_index": shell.active_index(),
                "already_open": false,
            }))
        }
        None => Err(format!(
            "打开失败（{}）——不支持或读取错误（详见应用提示横幅）",
            path.display()
        )),
    }
}

fn cmd_insert_text(
    shell: &mut ShellApp,
    cx: &mut Context<ShellApp>,
    line: usize,
    col: usize,
    text: &str,
) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let inserted = text.chars().count();
    match editor.update(cx, |ed, cx| ed.insert_text_at(line, col, text, cx)) {
        Some(cursor) => Ok(serde_json::json!({
            "cursor": point_json(cursor),
            "inserted_chars": inserted,
            "line_count": editor.read(cx).line_count(),
            "char_count": editor.read(cx).char_count(),
        })),
        None => Err("只读文档（渲染态）拒绝编辑".into()),
    }
}

fn cmd_replace_selection(
    shell: &mut ShellApp,
    cx: &mut Context<ShellApp>,
    text: &str,
) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    match editor.update(cx, |ed, cx| ed.replace_selection_text(text, cx)) {
        Some((had_selection, cursor)) => Ok(serde_json::json!({
            "had_selection": had_selection,
            "cursor": point_json(cursor),
            "char_count": editor.read(cx).char_count(),
        })),
        None => Err("只读文档（渲染态）拒绝编辑".into()),
    }
}

fn cmd_close_file(
    shell: &mut ShellApp,
    window: &mut Window,
    cx: &mut Context<ShellApp>,
    path: &str,
) -> Result<serde_json::Value, String> {
    let path = std::path::absolute(Path::new(path)).unwrap_or_else(|_| PathBuf::from(path));
    let Some(idx) = shell.tab_index_by_path(&path) else {
        return Err(format!("标签未找到：{}", path.display()));
    };
    if shell.tab_is_modified(idx, cx).unwrap_or(false) {
        return Err(format!(
            "文档有未保存修改（{}）——先保存再关闭",
            path.display()
        ));
    }
    if !shell.close_tab_no_confirm(idx, window, cx) {
        return Err(format!("关闭失败（索引越界？）：{}", path.display()));
    }
    Ok(serde_json::json!({
        "closed_index": idx,
        "tabs_remaining": shell.tab_count(),
    }))
}

fn cmd_switch_tab(
    shell: &mut ShellApp,
    window: &mut Window,
    cx: &mut Context<ShellApp>,
    index: usize,
) -> Result<serde_json::Value, String> {
    if index >= shell.tab_count() {
        return Err(format!(
            "标签索引越界：{index}（共 {} 个）",
            shell.tab_count()
        ));
    }
    shell.activate_tab(index, window, cx);
    Ok(serde_json::json!({ "active_index": index }))
}

// ---------------------------------------------------------------------------
// #111 任务 2 新命令（主线程薄编排——Editor 公开面）
// ---------------------------------------------------------------------------

fn cmd_set_cursor(
    shell: &mut ShellApp,
    cx: &mut Context<ShellApp>,
    line: usize,
    col: usize,
) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let cursor = editor.update(cx, |ed, cx| ed.move_cursor_to(line, col, cx));
    Ok(serde_json::json!({ "cursor": point_json(cursor) }))
}

fn cmd_set_selection(
    shell: &mut ShellApp,
    cx: &mut Context<ShellApp>,
    start: PointWire,
    end: PointWire,
) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let (a, b) = editor.update(cx, |ed, cx| {
        ed.set_selection_range(start.into_point(), end.into_point(), cx)
    });
    Ok(serde_json::json!({
        "selection": { "start": point_json(a), "end": point_json(b) },
    }))
}

fn cmd_get_selection(
    shell: &mut ShellApp,
    cx: &mut Context<ShellApp>,
) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let editor = editor.read(cx);
    let selection = editor.selection_points().map(|(start, end)| {
        let text = editor.selection_text();
        let chars = text.chars().count();
        // 与 get_state 同口径：选区文本带回（截断防爆载荷）。
        let (text, truncated) = if chars <= SELECTION_TEXT_CAP {
            (text, false)
        } else {
            (text.chars().take(SELECTION_TEXT_CAP).collect(), true)
        };
        serde_json::json!({
            "start": point_json(start),
            "end": point_json(end),
            "chars": chars,
            "text": text,
            "text_truncated": truncated,
        })
    });
    Ok(serde_json::json!({
        "path": editor.file_path().map(|p| p.display().to_string()),
        "revision": editor.revision(),
        "selection": selection,  // 无选区 = null
    }))
}

fn cmd_scroll_to(
    shell: &mut ShellApp,
    cx: &mut Context<ShellApp>,
    line: usize,
) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let scrolled = editor.update(cx, |ed, cx| ed.scroll_to_line(line, cx));
    Ok(serde_json::json!({
        "scrolled_to": scrolled,
        "line_count": editor.read(cx).line_count(),
    }))
}

fn cmd_undo(shell: &mut ShellApp, cx: &mut Context<ShellApp>) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let undid = editor.update(cx, |ed, cx| {
        let did = ed.undo_edit();
        cx.notify();
        did
    });
    let editor = editor.read(cx);
    Ok(serde_json::json!({
        "undid": undid,
        "revision": editor.revision(),
    }))
}

fn cmd_redo(shell: &mut ShellApp, cx: &mut Context<ShellApp>) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let redid = editor.update(cx, |ed, cx| {
        let did = ed.redo_edit();
        cx.notify();
        did
    });
    let editor = editor.read(cx);
    Ok(serde_json::json!({
        "redid": redid,
        "revision": editor.revision(),
    }))
}

fn cmd_save(shell: &mut ShellApp, cx: &mut Context<ShellApp>) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    let ed = editor.read(cx);
    if ed.is_read_only() {
        return Err("只读文档（渲染态）无保存语义".into());
    }
    let Some(path) = ed.file_path().map(|p| p.to_path_buf()) else {
        return Err("未命名文档无路径——先 save_as 指定目标".into());
    };
    // ed（只读借用）在此自然结束——file_path 已拷出，update 可变借用无冲突。
    if !editor.update(cx, |ed, cx| {
        let ok = ed.save();
        cx.notify();
        ok
    }) {
        return Err(format!(
            "保存失败（{}）——写盘错误（详见应用日志）",
            path.display()
        ));
    }
    Ok(serde_json::json!({
        "saved": true,
        "path": path.display().to_string(),
        "modified": editor.read(cx).is_modified(),
    }))
}

fn cmd_save_as(
    shell: &mut ShellApp,
    cx: &mut Context<ShellApp>,
    path: &str,
) -> Result<serde_json::Value, String> {
    let Some(editor) = shell.editor().cloned() else {
        return Err("无活动文档（空态——先 open_file）".into());
    };
    if editor.read(cx).is_read_only() {
        return Err("只读文档（渲染态）不可另存".into());
    }
    let path = std::path::absolute(Path::new(path)).unwrap_or_else(|_| PathBuf::from(path));
    // 走 ShellApp::save_active_as 完整链（editor.save_as + TabItem.path
    // 迁移 + 文件树 active 同步 + notify）——直调 editor.save_as 会漏
    // tab 身份迁移（tab_index_by_path 按旧路径查找失败——#111 烟雾
    // 实测踩到）。
    shell.save_active_as(path.clone(), cx);
    // 成功判定：标签身份已迁移到新路径（失败路径 save_active_as 早退
    // 不改 tab path）。
    match shell.tab_index_by_path(&path) {
        Some(idx) => Ok(serde_json::json!({
            "saved": true,
            "path": path.display().to_string(),
            "tab_index": idx,
        })),
        None => Err(format!(
            "另存失败（{}）——写盘错误（目录不存在/无权限？）",
            path.display()
        )),
    }
}

/// 全量状态（get_state 的 result 载荷）。
fn state_json(shell: &ShellApp, cx: &Context<ShellApp>) -> serde_json::Value {
    let tabs: Vec<serde_json::Value> = (0..shell.tab_count())
        .map(|i| {
            serde_json::json!({
                "path": shell.tab_path(i).map(|p| p.display().to_string()),
                "modified": shell.tab_is_modified(i, cx).unwrap_or(false),
                "readonly": shell.tab_is_readonly(i, cx).unwrap_or(false),
                "active": i == shell.active_index(),
            })
        })
        .collect();
    let active = shell.editor().cloned().map(|ed| {
        let editor = ed.read(cx);
        let selection = editor.selection_points().map(|(start, end)| {
            let text = editor.selection_text();
            let chars = text.chars().count();
            // 选区文本带回（AI 消费主通道）；超长截断防爆载荷。
            let (text, truncated) = if chars <= SELECTION_TEXT_CAP {
                (text, false)
            } else {
                (text.chars().take(SELECTION_TEXT_CAP).collect(), true)
            };
            serde_json::json!({
                "start": point_json(start),
                "end": point_json(end),
                "chars": chars,
                "text": text,
                "text_truncated": truncated,
            })
        });
        Some(serde_json::json!({
            "path": editor.file_path().map(|p| p.display().to_string()),
            "name": editor.display_name(),
            "kind": editor.doc_kind(),
            "readonly": editor.is_read_only(),
            "modified": editor.is_modified(),
            "cursor": point_json(editor.cursor_point()),
            "selection": selection,
            "line_count": editor.line_count(),
            "char_count": editor.char_count(),
        }))
    });
    serde_json::json!({
        "tabs": tabs,
        "active_index": shell.active_index(),
        "active": active,
    })
}

/// get_state 选区文本带回上限（字符）。
const SELECTION_TEXT_CAP: usize = 2000;

fn point_json(p: Point) -> serde_json::Value {
    serde_json::json!({ "line": p.line, "col": p.col })
}

// ---------------------------------------------------------------------------
// 单测（纯函数面——协议/端点/分帧/探活；执行面由 rpc_smoke.sh 端到端）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------- 协议解析 ----------------

    #[test]
    fn parse_six_commands_roundtrip() {
        let (id, cmd) = parse_command(r#"{"id":1,"cmd":"open_file","path":"/tmp/a.md"}"#);
        assert_eq!(id, Some(1));
        assert_eq!(
            cmd,
            Ok(ControlCommand::OpenFile {
                path: "/tmp/a.md".into()
            })
        );

        let (id, cmd) =
            parse_command(r#"{"id":2,"cmd":"insert_text","line":3,"col":4,"text":"hi\n"}"#);
        assert_eq!(id, Some(2));
        assert_eq!(
            cmd,
            Ok(ControlCommand::InsertText {
                line: 3,
                col: 4,
                text: "hi\n".into()
            })
        );

        let (id, cmd) = parse_command(r#"{"id":3,"cmd":"replace_selection","text":"X"}"#);
        assert_eq!(id, Some(3));
        assert_eq!(
            cmd,
            Ok(ControlCommand::ReplaceSelection { text: "X".into() })
        );

        let (id, cmd) = parse_command(r#"{"id":4,"cmd":"get_state"}"#);
        assert_eq!(id, Some(4));
        assert_eq!(cmd, Ok(ControlCommand::GetState));

        let (id, cmd) = parse_command(r#"{"id":5,"cmd":"close_file","path":"/tmp/a.md"}"#);
        assert_eq!(id, Some(5));
        assert_eq!(
            cmd,
            Ok(ControlCommand::CloseFile {
                path: "/tmp/a.md".into()
            })
        );

        let (id, cmd) = parse_command(r#"{"id":6,"cmd":"switch_tab","index":1}"#);
        assert_eq!(id, Some(6));
        assert_eq!(cmd, Ok(ControlCommand::SwitchTab { index: 1 }));
    }

    #[test]
    fn parse_id_optional_and_extra_fields_ignored() {
        // 无 id → None（响应回 null）
        let (id, cmd) = parse_command(r#"{"cmd":"get_state"}"#);
        assert_eq!(id, None);
        assert_eq!(cmd, Ok(ControlCommand::GetState));

        // 未知字段忽略（前向兼容——新客户端给旧服务发附加键不炸）
        let (id, cmd) = parse_command(r#"{"id":9,"cmd":"get_state","future_field":true}"#);
        assert_eq!(id, Some(9));
        assert_eq!(cmd, Ok(ControlCommand::GetState));
    }

    #[test]
    fn parse_malformed_json_yields_null_id_and_error() {
        let (id, cmd) = parse_command("not json at all");
        assert_eq!(id, None);
        assert!(cmd.is_err());

        let (id, cmd) = parse_command(r#"{"id":7,"cmd":"get_state""#); // 截断
        assert_eq!(id, None, "截断 JSON 取不到 id——回 null");
        assert!(cmd.is_err());
    }

    #[test]
    fn parse_unknown_command_echoes_id() {
        let (id, cmd) = parse_command(r#"{"id":42,"cmd":"open_fiel","path":"/x"}"#);
        assert_eq!(id, Some(42), "JSON 合法但命令拼错——id 应回显");
        let err = cmd.unwrap_err();
        assert!(err.contains("open_fiel"), "错误消息含未知命令名：{err}");
    }

    #[test]
    fn parse_missing_or_wrong_typed_fields_error() {
        // 缺 cmd
        let (id, cmd) = parse_command(r#"{"id":1,"path":"/x"}"#);
        assert_eq!(id, Some(1));
        assert!(cmd.is_err());

        // insert_text 缺 text
        let (_, cmd) = parse_command(r#"{"id":2,"cmd":"insert_text","line":0,"col":0}"#);
        assert!(cmd.is_err());

        // line 类型错（字符串）
        let (_, cmd) =
            parse_command(r#"{"id":3,"cmd":"insert_text","line":"0","col":0,"text":"x"}"#);
        assert!(cmd.is_err());

        // switch_tab 缺 index
        let (_, cmd) = parse_command(r#"{"id":4,"cmd":"switch_tab"}"#);
        assert!(cmd.is_err());
    }

    #[test]
    fn response_serialization_shapes() {
        let ok = ControlResponse::ok(Some(5), serde_json::json!({"n": 1}));
        let s = ok.to_line();
        assert!(s.ends_with('\n'), "行协议以换行收尾");
        let v: serde_json::Value = serde_json::from_str(s.trim()).unwrap();
        assert_eq!(v["id"], 5);
        assert_eq!(v["ok"], true);
        assert_eq!(v["result"]["n"], 1);
        assert!(v.get("error").is_none(), "ok 分支不出现 error 键");

        let err = ControlResponse::err(None, "炸了".into());
        let v: serde_json::Value = serde_json::from_str(err.to_line().trim()).unwrap();
        assert_eq!(v["id"], serde_json::Value::Null);
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "炸了");
        assert!(v.get("result").is_none(), "err 分支不出现 result 键");
    }

    #[test]
    fn command_names_match_wire_tags() {
        // 证据行/日志面与线上 cmd 字段一致性——防止改名漂移
        assert_eq!(
            ControlCommand::OpenFile {
                path: String::new()
            }
            .name(),
            "open_file"
        );
        assert_eq!(
            ControlCommand::InsertText {
                line: 0,
                col: 0,
                text: String::new()
            }
            .name(),
            "insert_text"
        );
        assert_eq!(
            ControlCommand::ReplaceSelection {
                text: String::new()
            }
            .name(),
            "replace_selection"
        );
        assert_eq!(ControlCommand::GetState.name(), "get_state");
        assert_eq!(
            ControlCommand::CloseFile {
                path: String::new()
            }
            .name(),
            "close_file"
        );
        assert_eq!(ControlCommand::SwitchTab { index: 0 }.name(), "switch_tab");
    }

    // ---------------- #111：新命令解析（8 命令 + 订阅） ----------------

    #[test]
    fn parse_eight_new_commands_roundtrip() {
        let (id, cmd) = parse_command(r#"{"id":7,"cmd":"set_cursor","line":10,"col":5}"#);
        assert_eq!(id, Some(7));
        assert_eq!(cmd, Ok(ControlCommand::SetCursor { line: 10, col: 5 }));

        let (id, cmd) = parse_command(
            r#"{"id":8,"cmd":"set_selection","start":{"line":0,"col":1},"end":{"line":0,"col":5}}"#,
        );
        assert_eq!(id, Some(8));
        assert_eq!(
            cmd,
            Ok(ControlCommand::SetSelection {
                start: PointWire { line: 0, col: 1 },
                end: PointWire { line: 0, col: 5 },
            })
        );

        let (_, cmd) = parse_command(r#"{"id":9,"cmd":"get_selection"}"#);
        assert_eq!(cmd, Ok(ControlCommand::GetSelection));

        let (_, cmd) = parse_command(r#"{"id":10,"cmd":"undo"}"#);
        assert_eq!(cmd, Ok(ControlCommand::Undo));

        let (_, cmd) = parse_command(r#"{"id":11,"cmd":"redo"}"#);
        assert_eq!(cmd, Ok(ControlCommand::Redo));

        let (_, cmd) = parse_command(r#"{"id":12,"cmd":"save"}"#);
        assert_eq!(cmd, Ok(ControlCommand::Save));

        let (_, cmd) = parse_command(r#"{"id":13,"cmd":"save_as","path":"/tmp/b.md"}"#);
        assert_eq!(
            cmd,
            Ok(ControlCommand::SaveAs {
                path: "/tmp/b.md".into()
            })
        );

        let (_, cmd) = parse_command(r#"{"id":14,"cmd":"scroll_to","line":42}"#);
        assert_eq!(cmd, Ok(ControlCommand::ScrollTo { line: 42 }));

        let (id, cmd) = parse_command(r#"{"id":15,"cmd":"subscribe"}"#);
        assert_eq!(id, Some(15));
        assert_eq!(cmd, Ok(ControlCommand::Subscribe));
        assert_eq!(cmd.unwrap().name(), "subscribe");
    }

    #[test]
    fn parse_new_commands_field_errors() {
        // set_cursor 缺 col
        let (_, cmd) = parse_command(r#"{"cmd":"set_cursor","line":1}"#);
        assert!(cmd.is_err());

        // set_selection 的点位也接受数组形态（serde struct 的 seq 形式
        // ——[line, col] 按位置解释；issue 事件示例即数组口径，两种
        // 写法互通是宽容输入面）。
        let (_, cmd) =
            parse_command(r#"{"cmd":"set_selection","start":[0,1],"end":{"line":0,"col":5}}"#);
        assert_eq!(
            cmd,
            Ok(ControlCommand::SetSelection {
                start: PointWire { line: 0, col: 1 },
                end: PointWire { line: 0, col: 5 },
            }),
            "数组点位按 [line, col] 位置解释——宽容输入"
        );

        // save_as 缺 path
        let (_, cmd) = parse_command(r#"{"cmd":"save_as"}"#);
        assert!(cmd.is_err());

        // scroll_to 的 line 类型错（字符串）
        let (_, cmd) = parse_command(r#"{"cmd":"scroll_to","line":"42"}"#);
        assert!(cmd.is_err());
    }

    #[test]
    fn new_command_names_match_wire_tags() {
        assert_eq!(
            ControlCommand::SetCursor { line: 0, col: 0 }.name(),
            "set_cursor"
        );
        assert_eq!(
            ControlCommand::SetSelection {
                start: PointWire { line: 0, col: 0 },
                end: PointWire { line: 0, col: 0 },
            }
            .name(),
            "set_selection"
        );
        assert_eq!(ControlCommand::GetSelection.name(), "get_selection");
        assert_eq!(ControlCommand::Undo.name(), "undo");
        assert_eq!(ControlCommand::Redo.name(), "redo");
        assert_eq!(ControlCommand::Save.name(), "save");
        assert_eq!(
            ControlCommand::SaveAs {
                path: String::new()
            }
            .name(),
            "save_as"
        );
        assert_eq!(ControlCommand::ScrollTo { line: 0 }.name(), "scroll_to");
    }

    // ---------------- #111：订阅表逻辑 ----------------

    #[test]
    fn subscribe_insert_is_idempotent_and_broadcast_reaches_all() {
        let subs: Subscriptions = std::sync::Mutex::new(std::collections::HashMap::new());
        let (a_tx, a_rx) = async_channel::unbounded::<String>();
        let (b_tx, b_rx) = async_channel::unbounded::<String>();
        subscribe_insert(&subs, 1, a_tx);
        subscribe_insert(&subs, 2, b_tx);
        // 幂等：同 conn 重新订阅替换旧通道（旧 rx 不再收）
        let (a2_tx, a2_rx) = async_channel::unbounded::<String>();
        subscribe_insert(&subs, 1, a2_tx);
        assert_eq!(subs.lock().unwrap().len(), 2, "同 conn 重复订阅不增条目");

        let delivered = subscribe_broadcast(&subs, "event\n");
        assert_eq!(delivered, 2, "两路存活订阅均送达");
        assert!(a_rx.is_empty(), "旧通道被替换——不再投递");
        assert_eq!(
            a2_rx.recv_blocking().as_deref(),
            Ok("event\n"),
            "重新订阅的新通道收到"
        );
        assert_eq!(
            b_rx.recv_blocking().as_deref(),
            Ok("event\n"),
            "未受重订阅影响的另一路照常收到"
        );
    }

    #[test]
    fn subscribe_broadcast_drops_dead_subscribers() {
        let subs: Subscriptions = std::sync::Mutex::new(std::collections::HashMap::new());
        let (tx, rx) = async_channel::unbounded::<String>();
        // 接收端立即 drop——通道已死
        drop(rx);
        subscribe_insert(&subs, 7, tx);
        let delivered = subscribe_broadcast(&subs, "line\n");
        assert_eq!(delivered, 0, "死通道不计送达");
        assert!(subs.lock().unwrap().is_empty(), "广播后死订阅被自动移除");
    }

    #[test]
    fn doc_event_line_serialization_for_subscribers() {
        // 事件 → JSON 行（事件泵路径的形态钉定：以 \n 收尾、type 字段名）
        let ev = crate::doc_events::DocEvent {
            kind: "doc_changed",
            path: Some(PathBuf::from("/tmp/a.md")),
            revision_before: 0,
            revision: 1,
            source: "rpc",
            changes: vec![crate::doc_events::ChangeRecord::precise(
                "replace",
                Point::new(0, 0),
                Point::new(0, 0),
                "hi".into(),
            )],
        };
        let mut line = serde_json::to_string(&ev).unwrap();
        line.push('\n');
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["type"], "doc_changed");
        assert_eq!(parsed["source"], "rpc");
        assert_eq!(parsed["changes"][0]["kind"], "replace");
    }

    // ---------------- 端点解析 ----------------

    #[test]
    fn socket_path_explicit_override_wins() {
        let p = default_socket_path(
            Some(PathBuf::from("/tmp/custom.sock")),
            Some(PathBuf::from("/run/user/1000")),
            Some(1000),
        );
        assert_eq!(p, PathBuf::from("/tmp/custom.sock"));
    }

    #[test]
    fn socket_path_xdg_runtime_then_tmp_uid_fallback() {
        let p = default_socket_path(None, Some(PathBuf::from("/run/user/1000")), None);
        assert_eq!(p, PathBuf::from("/run/user/1000/mang-shell.sock"));

        let p = default_socket_path(None, None, Some(1234));
        assert_eq!(p, PathBuf::from("/tmp/mang-shell-1234.sock"));

        let p = default_socket_path(None, None, None);
        assert_eq!(p, PathBuf::from("/tmp/mang-shell.sock"));
    }

    // ---------------- socket 预备（探活/清残留/防误删） ----------------

    /// 测试临时路径（进程内唯一——避开并行测试互踩；G3 全局 config_dir
    /// 污染教训：测试不共享可变全局路径）。
    fn temp_sock(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mang-rpc-test-{}-{name}.sock", std::process::id()))
    }

    #[test]
    fn prepare_socket_missing_path_is_ready() {
        let path = temp_sock("missing");
        let _ = std::fs::remove_file(&path);
        assert!(matches!(prepare_socket(&path), Ok(Prepared::Ready)));
    }

    #[test]
    fn prepare_socket_live_listener_is_busy() {
        let path = temp_sock("live");
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(
            matches!(prepare_socket(&path), Ok(Prepared::Busy)),
            "活实例在听——应报 Busy 而不是清它的文件"
        );
        drop(listener);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn prepare_socket_stale_socket_removed() {
        let path = temp_sock("stale");
        let _ = std::fs::remove_file(&path);
        // 真 socket 残留：绑一个监听后直接丢弃（不 unlink——上次异常
        // 退出的准确模拟；普通文件残留走下面的拒绝测试）。
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        drop(listener);
        assert!(path.exists(), "丢弃监听者不删 socket 文件——残留成立");
        assert!(matches!(prepare_socket(&path), Ok(Prepared::Ready)));
        assert!(!path.exists(), "陈旧残留应被清除");
    }

    #[test]
    fn prepare_socket_refuses_to_delete_non_socket_file() {
        let path =
            std::env::temp_dir().join(format!("mang-rpc-test-{}-regular.txt", std::process::id()));
        std::fs::write(&path, b"important data").unwrap();
        let result = prepare_socket(&path);
        assert!(result.is_err(), "普通文件必须拒绝而不是删除");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "important data",
            "内容不得被破坏"
        );
        let _ = std::fs::remove_file(&path);
    }

    // ---------------- 行分帧 ----------------

    #[test]
    fn line_assembler_splits_multiple_commands_in_one_chunk() {
        let mut asm = LineAssembler::new(1024);
        asm.feed(b"{\"id\":1,\"cmd\":\"get_state\"}\n{\"id\":2,\"cmd\":\"get_state\"}\r\n");
        assert_eq!(
            asm.next_line().as_deref(),
            Some(r#"{"id":1,"cmd":"get_state"}"#)
        );
        assert_eq!(
            asm.next_line().as_deref(),
            Some(r#"{"id":2,"cmd":"get_state"}"#),
            "CRLF 行尾归一（\\r 不入命令行）"
        );
        assert_eq!(asm.next_line(), None, "无更多完整行");
        assert!(!asm.overflowed());
    }

    #[test]
    fn line_assembler_partial_then_completed() {
        let mut asm = LineAssembler::new(1024);
        asm.feed(br#"{"id":1,"cmd":"ge"#);
        assert_eq!(asm.next_line(), None, "半行不吐");
        asm.feed(br#"t_state"}"#);
        assert_eq!(asm.next_line(), None, "仍无换行——继续攒");
        asm.feed(b"\n");
        assert_eq!(
            asm.next_line().as_deref(),
            Some(r#"{"id":1,"cmd":"get_state"}"#)
        );
    }

    #[test]
    fn line_assembler_enforces_cap_and_takes_remainder() {
        let mut asm = LineAssembler::new(16);
        asm.feed(&[b'a'; 17]);
        assert!(asm.overflowed(), "无换行待处理字节超上限");
        // 上限以「待处理字节」计：完整行弹出后即清——不误伤合法长行
        let mut asm = LineAssembler::new(16);
        asm.feed(&[b'a'; 10]);
        assert!(!asm.overflowed());
        asm.feed(b"\n");
        assert!(!asm.overflowed(), "完整行在缓冲内不算超限");
        assert!(asm.next_line().is_some());
        assert!(!asm.overflowed());
    }

    #[test]
    fn line_assembler_remainder_without_newline() {
        let mut asm = LineAssembler::new(1024);
        asm.feed(b"{\"id\":3,\"cmd\":\"get_state\"}");
        assert_eq!(asm.next_line(), None);
        assert_eq!(
            asm.take_remainder().as_deref(),
            Some(r#"{"id":3,"cmd":"get_state"}"#),
            "EOF 尾包（无换行的末行）也应被处理"
        );
        assert_eq!(asm.take_remainder(), None);
    }
}
