"""mang_rpc 异常类型。

错误分类原则（对照 `mang-shell/docs/rpc-protocol.md` 第 8 节）：

- 服务端业务拒绝（``ok:false``）→ :class:`RpcError`，``message`` 保留
  服务端原文（人可读中文）。**SDK 不做错误字符串匹配分支**——文案会随
  版本变化，协议文档明确禁止。要判断状态请用 ``get_state()`` 的
  结构化字段（如 ``active.modified``、``active.readonly``）。
- 传输层（连接拒绝/断开/超时）→ :class:`TransportError` 家族，
  与业务拒绝严格分层，调用方可按 ``except RpcError`` / ``except
  TransportError`` 区分处理。
- 协议违规（响应行不是合法 JSON、载荷超上限）→ :class:`ProtocolViolation`
  / :class:`LineTooLong`。
"""

from __future__ import annotations

from typing import Any

__all__ = [
    "MangRpcError",
    "RpcError",
    "TransportError",
    "ConnectionLost",
    "RequestTimeout",
    "ProtocolViolation",
    "LineTooLong",
]


class MangRpcError(Exception):
    """mang_rpc 全部异常的基类。

    捕获它即可兜住 SDK 可能抛出的一切（业务拒绝 + 传输 + 协议）。
    """


class RpcError(MangRpcError):
    """服务端对命令的业务拒绝（响应 ``{"ok": false, "error": "…"}``）。

    典型场景：空态没开文档、无工作区时调 FS 命令、关脏标签、路径越出
    工作区沙箱等——完整类别见协议文档第 8 节。

    注意：``message`` 是给人看的中文文案，**不要**对它做字符串匹配分支；
    程序化判断状态请用 ``get_state()`` / ``get_selection()`` 的结构化字段。

    :param message: 服务端 ``error`` 原文（保留不翻译、不解析）。
    :param id: 对应请求的 id（服务端回显；请求未带 id 时为 None）。
    :param cmd: 发出的命令名（诊断用）。
    """

    def __init__(self, message: str, *, id: int | None = None, cmd: str = "") -> None:
        super().__init__(message)
        self.message = message
        self.id = id
        self.cmd = cmd

    def __str__(self) -> str:
        if self.cmd:
            return f"[{self.cmd}] {self.message}"
        return self.message


class TransportError(MangRpcError):
    """传输层故障（连接拒绝、socket 错误、对端关闭等）。"""

    def __init__(self, reason: str) -> None:
        super().__init__(reason)
        self.reason = reason


class ConnectionLost(TransportError):
    """连接断开且自动重连未恢复（禁用重连 / 重试次数耗尽 / 对端拒绝）。

    请求中途断开时命令**可能已生效**（at-least-once）——恢复后请先
    ``get_state()`` 对齐现实，不要盲目重发编辑类命令。
    """

    def __init__(self, reason: str) -> None:
        super().__init__(reason)


class RequestTimeout(MangRpcError):
    """等待响应超时。

    超时 ≠ 失败：命令可能已在服务端执行（响应在路上丢了/编辑器卡了）。
    处理姿势同 :class:`ConnectionLost`——先 ``get_state()`` 再决定重试。
    """

    def __init__(self, cmd: str, timeout: float) -> None:
        super().__init__(f"等待 {cmd} 响应超时（{timeout}s）——命令可能已生效，先 get_state 对齐")
        self.cmd = cmd
        self.timeout = timeout


class ProtocolViolation(MangRpcError):
    """响应/事件不符合协议（JSON 损坏、无法分类的行等）。

    服务端正常时不应出现；出现说明版本错配或传输损坏。
    """

    def __init__(self, detail: str, line: Any = None) -> None:
        super().__init__(f"协议违规：{detail}")
        self.detail = detail
        self.line = line


class LineTooLong(MangRpcError):
    """请求行超过服务端 1 MiB 单行上限（``MAX_LINE_BYTES``）。

    客户端在发送前拦截（服务端超限会**直接关闭连接**）。大内容请走
    ``write_file`` / ``batch_replace``，或 ``insert_text`` 分块。
    """

    def __init__(self, size: int, limit: int) -> None:
        super().__init__(f"请求行 {size} 字节超过上限 {limit}——分块提交（大内容走 write_file）")
        self.size = size
        self.limit = limit
