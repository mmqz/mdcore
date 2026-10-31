//! rpc —— 本机控制通道（#104 任务 2「RPC 桥接准备」）。
//!
//! ## 定位：Lead 长期设计的第一块地基
//!
//! Lead 的长期设计是「AI 操作运行中的 UI，实时反馈」。现状 mang-shell
//! 用自己的 Buffer（不走 mdcore 的 BlockTree/Op），AI 改不了文档、也
//! 没法共享状态。本模块**不是**完整实现（那要 5-6 周），而是打通链路
//! 的第一步：让外部进程（AI 客户端 / CLI 工具）能把命令送进运行中的
//! mang-shell，并在 UI 上实时看到效果。
//!
//! ## 架构（最小可用）
//!
//! ```text
//! 外部客户端（python/nc …）                mang-shell 进程
//! ─────────────────────                  ──────────────────────────────
//!   AF_UNIX socket  ──JSON 行──▶ accept 循环 ──▶ async_channel 队列
//!   （每连接一个后台任务）                        │
//!                                               ▼
//!                                    dispatcher（单任务串行）
//!                                    cx.update → window.update
//!                                    （主线程走 GPUI 修改 UI）
//!                                               │
//!   ◀──JSON 行（响应）── 每连接 oneshot ◀────────┘
//! ```
//!
//! - **传输**：unix domain socket，JSON Lines（一行一 JSON 对象，UTF-8）。
//!   本机通信——不做加密/认证/网络（#104 明确边界）。
//! - **端点**：`$XDG_RUNTIME_DIR/mang-shell.sock`，缺省
//!   `/tmp/mang-shell-$UID.sock`；`--rpc-sock PATH` 显式覆盖（烟雾与
//!   多实例场景）；`--no-rpc` 关闭。
//! - **并发**：每连接独立后台任务（收发不互阻）；命令全部经单一
//!   dispatcher 串行落主线程——GPUI 实体修改只能在主线程，串行化即
//!   正确性保证（多客户端「同时连、不互扰」，命令天然排队）。
//! - **退出清理**：`ControlServer` Drop 时删除 socket 文件；异常退出
//!   的残留由下次启动 [`prepare_socket`] 探活清理（连得上 = 别的实例
//!   在监听 → 本实例禁用 RPC；连不上 = 陈旧文件 → 删除后重绑）。
//!
//! ## 协议（6 命令，#104 原文口径）
//!
//! 请求（`id` 可省——省略时响应 `id` 为 null；未知字段忽略——前向兼容）：
//!
//! ```json
//! {"id": 1, "cmd": "open_file",         "path": "/tmp/a.md"}
//! {"id": 2, "cmd": "insert_text",       "line": 0, "col": 4, "text": "hi"}
//! {"id": 3, "cmd": "replace_selection", "text": "X"}
//! {"id": 4, "cmd": "get_state"}
//! {"id": 5, "cmd": "close_file",        "path": "/tmp/a.md"}
//! {"id": 6, "cmd": "switch_tab",        "index": 1}
//! ```
//!
//! 响应（恒一行 JSON）：
//!
//! ```json
//! {"id": 2, "ok": true,  "result": {"cursor": {"line": 0, "col": 6}, ...}}
//! {"id": 5, "ok": false, "error": "文档有未保存修改——先保存再关闭"}
//! ```
//!
//! ## 命令语义要点
//!
//! - `insert_text`/`replace_selection`：走与用户键入同一编辑链
//!   （`Buffer::replace_range` 单点 + undo 一条 + 光标落插入尾 + 滚入
//!   可视区）——「AI 改一下，用户看到文字出现」的实时反馈即此路径。
//!   只读态（docx/pdf/xlsx 渲染）拒绝；空 `replace_selection` 无选区
//!   时为无操作，有选区时为删除选区。
//! - `close_file`：**脏标签拒绝**（返回错误让客户端先保存）——确认框
//!   是用户交互面，不适合机器通道（改完不保存就关 = 静默丢数据）。
//! - `get_state`：标签清单 + 活动文档（路径/光标/选区/字数/脏/只读），
//!   AI 决策「接下来改哪里」的数据源。
//!
//! ## 验证
//!
//! - 单测：协议解析/序列化、端点解析、陈旧 socket 探活、行分帧
//!   （见文件尾 `mod tests`）。
//! - 端到端：`rpc_smoke.sh`（Xvfb 启动 + `tests/rpc_client.py` 外部
//!   客户端六命令全链 + 双客户端并发）。

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

/// 控制命令（#104 任务书 6 命令原文口径）。
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

/// 一条待执行命令（dispatcher 投递单元；`reply` 为每命令 oneshot）。
struct Job {
    id: Option<u64>,
    command: ControlCommand,
    reply: async_channel::Sender<ControlResponse>,
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
    /// 启动控制服务：预备 socket → 绑定 → 派生 accept 循环与
    /// dispatcher（均为 detached 任务，随应用退出消亡）→ **挂应用
    /// 全局**（存活到 App 退出——见类型文档的 Drop 陷阱）。
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
        // WindowHandle 实现 Copy——直传免 clone（clippy::clone_on_copy）。
        cx.spawn(async move |cx| dispatcher_loop(rx, window, cx).await)
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
async fn accept_loop(listener: UnixListener, tx: async_channel::Sender<Job>, cx: &mut AsyncApp) {
    let live = Arc::new(AtomicUsize::new(0));
    let mut errors: u32 = 0;
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                errors = 0;
                let live_now = live.fetch_add(1, Ordering::SeqCst) + 1;
                eprintln!("rpc: client connected（{live_now} 路）");
                let tx = tx.clone();
                let live = live.clone();
                let task: Task<()> = cx.background_spawn(async move {
                    connection_task(stream, tx).await;
                    let live_now = live.fetch_sub(1, Ordering::SeqCst) - 1;
                    eprintln!("rpc: client disconnected（{live_now} 路）");
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

/// 单连接任务：读行 → 解析 → 投递 dispatcher → 等响应 → 回写。
async fn connection_task(mut stream: UnixStream, tx: async_channel::Sender<Job>) {
    let mut asm = LineAssembler::new(MAX_LINE_BYTES);
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) => break, // EOF
            Ok(n) => {
                asm.feed(&chunk[..n]);
                while let Some(line) = asm.next_line() {
                    // 排水循环：空行无响应继续；写失败（对端断）即收工。
                    // 注意不能折叠成 let-chain && 写失败条件——那会把
                    // 「写成功继续循环」变成「单轮即出」（never_loop）。
                    let Some(resp) = handle_line(&line, &tx).await else {
                        continue;
                    };
                    if stream.write_all(resp.to_line().as_bytes()).await.is_err() {
                        return; // 对端已断——写失败即收工
                    }
                }
                if asm.overflowed() {
                    let resp = ControlResponse::err(
                        None,
                        format!("单行超过 {MAX_LINE_BYTES} 字节上限——连接关闭"),
                    );
                    let _ = stream.write_all(resp.to_line().as_bytes()).await;
                    return;
                }
            }
            Err(_) => break, // 读错（对端异常断）——静默收工
        }
    }
    // EOF 尾包：最后一条没带换行的行也处理（宽容 netcat/echo 习惯）。
    if let Some(line) = asm.take_remainder()
        && let Some(resp) = handle_line(&line, &tx).await
    {
        let _ = stream.write_all(resp.to_line().as_bytes()).await;
    }
}

/// 单行处理：空行忽略；解析失败回错；成功投递 dispatcher 等 oneshot。
async fn handle_line(line: &str, tx: &async_channel::Sender<Job>) -> Option<ControlResponse> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (id, parsed) = parse_command(line);
    match parsed {
        Ok(command) => {
            let (reply_tx, reply_rx) = async_channel::bounded::<ControlResponse>(1);
            if tx
                .send(Job {
                    id,
                    command,
                    reply: reply_tx,
                })
                .await
                .is_err()
            {
                return Some(ControlResponse::err(
                    id,
                    "控制服务已停止（dispatcher 通道关闭）".into(),
                ));
            }
            match reply_rx.recv().await {
                Ok(resp) => Some(resp),
                Err(_) => Some(ControlResponse::err(
                    id,
                    "命令无响应（应用退出中？）".into(),
                )),
            }
        }
        Err(err) => Some(ControlResponse::err(id, err)),
    }
}

/// dispatcher：单任务串行——命令全部落主线程执行（GPUI 实体修改的
/// 正确性保证；多客户端命令在此天然排队，互不穿插半执行）。
async fn dispatcher_loop(
    rx: async_channel::Receiver<Job>,
    window: WindowHandle<ShellApp>,
    cx: &mut AsyncApp,
) {
    while let Ok(job) = rx.recv().await {
        let executed = cx.update(|cx| {
            window
                .update(cx, |shell, window, cx| {
                    execute_command(shell, window, cx, &job.command)
                })
                .unwrap_or_else(|err| Err(format!("窗口不可用：{err}")))
        });
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
        let _ = job.reply.send(resp).await;
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
