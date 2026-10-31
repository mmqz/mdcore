//! doc_events —— 文档变更事件总线（#111 任务 2「订阅模式」的发射面）。
//!
//! ## 定位
//!
//! #109 的 RPC 桥是「请求-响应」——外部 AI 客户端不知道文档什么时候
//! 变了，只能轮询 `get_state`。本模块把「文档变了」变成**推送流**：
//! 任何文档变更（用户键入 / RPC 编辑 / 撤销重做 / 查找替换）都经
//! [`publish`] 汇入单一总线，RPC 侧的订阅泵再把事件分发给所有已
//! `Subscribe` 的客户端——「AI+Human 共享工作区」的双向回显即此链路：
//!
//! ```text
//! 用户键入 ──┐
//! RPC 编辑 ──┼─▶ Buffer.pending_changes ─▶ Editor.after_edit 收割
//! 撤销重做 ──┘         （单点）                │
//!                                              ▼
//!                                    doc_events::publish（总线）
//!                                              │
//!                        RPC 未启用 = 丢弃（no-op）│ RPC 启用
//!                                              ▼
//!                    订阅泵 → 各客户端 out 通道 → writer 任务落 socket
//! ```
//!
//! ## 事件形态（#111 原文口径 + 加性扩展）
//!
//! ```json
//! {"type": "doc_changed", "path": "/tmp/a.md", "revision": 42,
//!  "source": "user", "changes": [
//!    {"kind": "replace", "start": {"line": 10, "col": 5},
//!     "end": {"line": 10, "col": 5}, "text": "hi"}]}
//! ```
//!
//! - `revision`：文档版本号（Buffer 维护，每次变更事务 +1——含撤销
//!   /重做）。客户端靠它检测乱序与丢事件（两事件 revision 连续 = 无丢失）。
//! - `source`：`"user"`（用户编辑）或 `"rpc"`（外部命令编辑）——AI 据此
//!   区分「自己刚发的命令的回显」与「人类此刻正在改什么」。
//! - `changes[].kind`：`replace`（常规替换/插入/删除——`start`/`end` 为
//!   **变更前被替换域**，`start == end` 即纯插入）/ `undo` / `redo`。
//!   批量撤销/重做无法逐点给出精确域（字节坐标在撤销后错位）——给
//!   `kind`-only 记录（start/end/text 均省略），客户端收到后可
//!   `get_state` 拉全量。
//!
//! ## 初始化契约（与 [`crate::rpc::ControlServer`] 的握手）
//!
//! 总线是进程级单例（`OnceLock`）。[`publish`] 在未初始化时**静默
//! 丢弃**——RPC 禁用（`--no-rpc` / 端点被占）的应用照常编辑，无性能
//! 负担之外的差别；`ControlServer::start` 成功时调用 [`init_bus`]
//! 取走接收端并启动订阅泵。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use serde::Serialize;

use crate::editor::buffer::Point;

/// 单条变更记录（`changes` 数组元素）。
///
/// `end` 是**变更前被替换域的终点**（`start == end` = 纯插入）；
/// 批量撤销/重做给不出精确域时三项全 `None`（kind-only 粗粒度）。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChangeRecord {
    /// 变更类别：`replace` / `undo` / `redo`。
    pub kind: &'static str,
    /// 变更起点（变更前坐标）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<Point>,
    /// 变更前被替换域终点。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<Point>,
    /// 新文本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ChangeRecord {
    /// 精确记录（常规编辑/单条撤销重做）。
    pub fn precise(kind: &'static str, start: Point, end: Point, text: String) -> Self {
        Self {
            kind,
            start: Some(start),
            end: Some(end),
            text: Some(text),
        }
    }

    /// 粗粒度记录（批量撤销/重做——坐标域不可精确恢复，客户端拉全量）。
    pub fn coarse(kind: &'static str) -> Self {
        Self {
            kind,
            start: None,
            end: None,
            text: None,
        }
    }
}

/// 一次文档变更事件（一次「编辑收尾」一条——一次手势的多步事务在
/// `changes` 里并列，`revision_before`..`revision` 给出跨度）。
#[derive(Clone, Debug, Serialize)]
pub struct DocEvent {
    /// 事件类型（恒 `"doc_changed"`——前向兼容的事件多路分发位）。
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// 变更文档的路径（未命名文档 = None → JSON null）。
    #[serde(rename = "path")]
    pub path: Option<PathBuf>,
    /// 变更前版本（首条记录入队时的版本号）。
    pub revision_before: u64,
    /// 变更后文档版本号。
    pub revision: u64,
    /// 变更来源：`"user"` / `"rpc"`。
    pub source: &'static str,
    /// 变更明细（≥ 1 条）。
    pub changes: Vec<ChangeRecord>,
}

/// 变更来源标记（编辑链路的「谁在改」——AI 回显 vs 人类编辑的区分面）。
///
/// RPC dispatcher 在主线程执行外部命令前置位、执行完复位（RAII）；
/// Editor 的编辑收尾（`after_edit`）读取它决定事件的 `source`。全程
/// 主线程串行，无竞争。
static RPC_SOURCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 当前编辑是否源自 RPC 命令（主线程调用）。
pub fn is_rpc_source() -> bool {
    RPC_SOURCE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 标记后续编辑源自 RPC（RAII guard——drop 时复位）。
pub fn mark_rpc_source() -> RpcSourceGuard {
    RPC_SOURCE.store(true, std::sync::atomic::Ordering::Relaxed);
    RpcSourceGuard
}

/// 见 [`mark_rpc_source`]。
#[must_use = "guard drop 前标记一直生效——必须持有"]
pub struct RpcSourceGuard;

impl Drop for RpcSourceGuard {
    fn drop(&mut self) {
        RPC_SOURCE.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// 总线（进程级单例）
// ---------------------------------------------------------------------------

static BUS: OnceLock<async_channel::Sender<Arc<DocEvent>>> = OnceLock::new();

/// 初始化总线，返回接收端（`ControlServer::start` 启动订阅泵时调用；
/// 二次调用返回 `None`——单接收端契约，首个泵独占消费）。
pub fn init_bus() -> Option<async_channel::Receiver<Arc<DocEvent>>> {
    let (tx, rx) = async_channel::unbounded();
    BUS.set(tx).ok().map(|_| rx)
}

/// 发布一次文档变更（编辑链路单点调用；无订阅者 / 总线未初始化 =
/// 静默丢弃，不构成错误——RPC 禁用时应用照常编辑）。
///
/// 注意用 `try_send` 而非 `send`：本函数是**同步**面（`after_edit`
/// 收割链直调，无 async 上下文）；`send` 是 async 方法，同步调用只
/// 会创建 future 不 poll——事件静默丢失（测试 recv 挂死正是此坑）。
/// unbounded 通道的 `try_send` 恒立即入队、不阻塞。
pub fn publish(event: DocEvent) {
    if let Some(tx) = BUS.get() {
        // 接收端已丢弃（泵退出）时 TrySendError 只是事件落空——
        // 不反过来影响编辑链路。
        let _ = tx.try_send(Arc::new(event));
    }
}

// ---------------------------------------------------------------------------
// 单测（纯逻辑面：事件形态 / source 标记 / 总线握手）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, col: usize) -> Point {
        Point::new(line, col)
    }

    #[test]
    fn doc_event_json_shape_matches_issue_schema() {
        let ev = DocEvent {
            kind: "doc_changed",
            path: Some(PathBuf::from("/tmp/a.md")),
            revision_before: 41,
            revision: 42,
            source: "user",
            changes: vec![ChangeRecord::precise(
                "replace",
                p(10, 5),
                p(10, 5),
                "hi".into(),
            )],
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        assert_eq!(v["type"], "doc_changed");
        assert_eq!(v["path"], "/tmp/a.md");
        assert_eq!(v["revision_before"], 41);
        assert_eq!(v["revision"], 42);
        assert_eq!(v["source"], "user");
        let c = &v["changes"][0];
        assert_eq!(c["kind"], "replace");
        assert_eq!(c["start"]["line"], 10);
        assert_eq!(c["start"]["col"], 5);
        assert_eq!(c["end"]["col"], 5, "start == end 即纯插入");
        assert_eq!(c["text"], "hi");
    }

    #[test]
    fn coarse_record_omits_optional_fields() {
        let rec = ChangeRecord::coarse("undo");
        let s = serde_json::to_string(&rec).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["kind"], "undo");
        assert!(v.get("start").is_none(), "粗粒度记录省略 start");
        assert!(v.get("end").is_none());
        assert!(v.get("text").is_none());
    }

    #[test]
    fn unnamed_doc_path_serializes_null() {
        let ev = DocEvent {
            kind: "doc_changed",
            path: None,
            revision_before: 0,
            revision: 1,
            source: "rpc",
            changes: vec![ChangeRecord::coarse("redo")],
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        assert_eq!(v["path"], serde_json::Value::Null, "未命名文档 path = null");
    }

    #[test]
    fn rpc_source_guard_marks_and_resets() {
        assert!(!is_rpc_source(), "缺省非 RPC 来源");
        {
            let _g = mark_rpc_source();
            assert!(is_rpc_source(), "guard 存续期标记生效");
        }
        assert!(!is_rpc_source(), "guard drop 即复位");
    }

    #[test]
    fn bus_init_once_then_publish_reaches_receiver() {
        let rx = init_bus();
        assert!(rx.is_some(), "首次初始化返回接收端");
        assert!(init_bus().is_none(), "二次初始化拒绝（单接收端契约）");

        publish(DocEvent {
            kind: "doc_changed",
            path: Some(PathBuf::from("/tmp/x.md")),
            revision_before: 6,
            revision: 7,
            source: "user",
            changes: vec![ChangeRecord::precise(
                "replace",
                p(0, 0),
                p(0, 0),
                "x".into(),
            )],
        });
        let ev = rx.unwrap().recv_blocking().expect("事件应可达接收端");
        assert_eq!(ev.revision, 7);
        assert_eq!(ev.changes.len(), 1);
    }
}
