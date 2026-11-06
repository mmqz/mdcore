"""mang-shell RPC 客户端（同步 :class:`Client` + 异步 :class:`AsyncClient`）。

架构（两个客户端同构）::

    调用线程 ──build──▶ {"id":n, "cmd":"…", <参数平铺>} ──sendall──▶ unix socket
       ▲                                                            │
       │ 按 id 匹配（响应不保证顺序）                                  ▼
    └──pending[id]── reader 线程 ◀── JSON Lines ◀── {"id":n,"ok":…} / {"type":"doc_changed"}

    - 响应按 ``id`` 匹配到 pending，事件（``type`` 字段）进事件队列，
      二者走同一条 socket 流——与协议「响应与事件同出口」一致。
    - 断线后 reader 自动重连（次数/退避可配），重连成功自动重订阅
      （协议幂等）；重连耗尽才对事件流注入中断标记。

语义要点（对照 ``mang-shell/docs/rpc-protocol.md``）：

- **重发是 at-least-once**：连接断开时命令可能已生效，默认
  ``retry_on_drop=True`` 会重连后重发一次——编辑类命令自行评估幂等性，
  恢复后先 ``get_state()`` 对齐现实。
- **超时不重试**：``RequestTimeout`` 意味着命令可能已执行，盲目重发
  编辑命令会双写——调用方决定怎么处理。
- **不做错误字符串匹配**：``ok:false`` 一律包装成 :class:`RpcError`，
  message 保留服务端中文原文；判断状态用 ``get_state()`` 结构化字段。
"""

from __future__ import annotations

import asyncio
import contextlib
import itertools
import json
import logging
import os
import queue
import socket
import threading
import time
from collections.abc import AsyncIterator, Callable, Iterator, Mapping
from typing import Any, TypeVar

from .errors import (
    ConnectionLost,
    LineTooLong,
    MangRpcError,
    ProtocolViolation,
    RequestTimeout,
    RpcError,
    TransportError,
)
from .protocol import (
    COMMANDS,
    MAX_LINE_BYTES,
    ApplyFormatResult,
    BatchReplaceResult,
    CloseFileResult,
    CreateDirResult,
    CreateFileResult,
    DeleteResult,
    DocEvent,
    FormatKind,
    GetOutlineResult,
    GetSelectionResult,
    InsertTextResult,
    ListCommandsResult,
    ListDirResult,
    OpenFileResult,
    Point,
    ReadFileResult,
    RedoResult,
    RenameResult,
    ReplaceSelectionResult,
    RunCommandResult,
    SaveAsResult,
    SaveResult,
    ScrollToResult,
    SetCursorResult,
    SetSelectionResult,
    State,
    SubscribeResult,
    SwitchTabResult,
    UndoResult,
    WriteFileResult,
)

__all__ = ["Client", "AsyncClient", "default_socket_path"]

_log = logging.getLogger("mang_rpc")

#: 客户端单行接收上限（字节）。服务端 send 侧无硬上限（list_dir 5000 条
#: / read_file 1 MiB 文本 + JSON 转义开销都会超 1 MiB），16 MiB 是防御
#: 性内存护栏，不是协议的一部分。
RECV_LINE_CAP = 16 * 1024 * 1024

T = TypeVar("T")


# ---------------------------------------------------------------------------
# 公共辅助
# ---------------------------------------------------------------------------


def default_socket_path() -> str:
    """按编辑器同序解析默认 RPC 端点。

    优先级（对照服务端 ``ControlServer`` 端点逻辑）：

    1. ``$MANG_RPC_SOCK``（SDK 扩展——测试/多实例时覆盖）
    2. ``$XDG_RUNTIME_DIR/mang-shell.sock``
    3. ``/tmp/mang-shell-$UID.sock``
    """
    env = os.environ.get("MANG_RPC_SOCK")
    if env:
        return env
    xdg = os.environ.get("XDG_RUNTIME_DIR")
    if xdg:
        return os.path.join(xdg, "mang-shell.sock")
    return f"/tmp/mang-shell-{os.getuid()}.sock"


def _to_point(p: Point) -> dict[str, int]:
    """点位归一：``{"line":l,"col":c}`` / ``(l, c)`` 两种入参形态。"""
    if isinstance(p, Mapping):
        return {"line": int(p["line"]), "col": int(p["col"])}
    seq = list(p)
    return {"line": int(seq[0]), "col": int(seq[1])}


def _encode_request(req_id: int, cmd: str, params: Mapping[str, Any]) -> bytes:
    """构造一行请求（UTF-8 JSON + ``\\n``）并做单行上限自检。

    服务端超限（>1 MiB 含换行）会回错误并**关闭连接**——客户端先挡，
    连接得以保全。
    """
    payload = {"id": req_id, "cmd": cmd, **params}
    data = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    data += b"\n"
    if len(data) > MAX_LINE_BYTES:
        raise LineTooLong(len(data), MAX_LINE_BYTES)
    return data


def _decode_line(line: bytes) -> dict[str, Any] | None:
    """解码一行服务端消息：None = 忽略（空行），dict = 消息。

    非法 JSON / 非对象 → :class:`ProtocolViolation`。
    """
    if not line.strip():
        return None
    try:
        obj: Any = json.loads(line)
    except (json.JSONDecodeError, UnicodeDecodeError) as e:
        raise ProtocolViolation(f"响应行不是合法 JSON：{e}", line) from e
    if not isinstance(obj, dict):
        raise ProtocolViolation("响应行不是 JSON 对象", line)
    return obj


def _is_event(obj: Mapping[str, Any]) -> bool:
    """ "type" 字段存在 = 事件（协议留作未来多路分发）。"""
    return "type" in obj


def _pending_key(req_id: Any) -> int | None:
    """响应 id → pending 查找键；无法转整数的敌意形态 → None（无法匹配）。

    #282（P2-29）：敌意/损坏服务端回 ``"id":"evil"``、列表、``inf`` 等形态时，
    裸 :func:`int` 会抛 ValueError/TypeError/OverflowError——两个 reader 的
    except 面只接 OSError/ProtocolViolation，异常直接炸掉 reader 线程/任务
    （连接与 id 匹配机制随后失灵）。此处统一转 None，走既有的
    「无法匹配 → 丢弃」语义，reader 存活、连接不受损。
    """
    try:
        return int(req_id)
    except (TypeError, ValueError, OverflowError):
        return None


def _reply_result(reply: Mapping[str, Any], cmd: str) -> dict[str, Any]:
    """响应二分类：ok → result dict；否则 → :class:`RpcError`。"""
    if reply.get("ok"):
        result = reply.get("result")
        return result if isinstance(result, dict) else {}
    msg = reply.get("error")
    raise RpcError(
        str(msg) if isinstance(msg, str) else "未知错误",
        id=reply.get("id"),
        cmd=cmd,
    )


class _Sentinel:
    """事件队列控制标记（关闭 / 流中断）。"""

    __slots__ = ("name",)

    def __init__(self, name: str) -> None:
        self.name = name


class _StaleConnection(ConnectionLost):
    """发送失败——连接实际已死但 read 侧可能尚未察觉。

    携带发起请求时的连接**代际**（``gen``），供重连去重：只有产出
    ``gen + 1`` 代新连接才可信（防止双检误信过时的 connected 标志
    ——发送侧的 EPIPE 总是先于 reader 的 EOF 到达）。
    """

    def __init__(self, reason: str, gen: int) -> None:
        super().__init__(reason)
        self.gen = gen


_CLOSED = _Sentinel("closed")
_DROPPED = _Sentinel("dropped")


# ===========================================================================
# 同步客户端
# ===========================================================================


class _PendingReply:
    """一个在途请求的应答槽（reader 线程填充，调用线程等待）。"""

    __slots__ = ("event", "reply", "error")

    def __init__(self) -> None:
        self.event = threading.Event()
        self.reply: dict[str, Any] | None = None
        self.error: BaseException | None = None


class Client:
    """mang-shell 同步 RPC 客户端（27 命令 + 事件流）。

    线程安全：内部 reader 线程 + per-request pending 表，可在多线程并发
    调用同一实例；响应按 ``id`` 匹配，不依赖到达顺序。

    用法::

        with Client.connect() as c:
            c.open_file("/abs/doc.md")
            c.insert_text(0, 0, "hello")
            state = c.get_state()
            print(state.active.cursor)

            for event in c.subscribe():
                if event.source == "user":
                    print(f"用户改了 {len(event.changes)} 处")

    :param sock_path: unix socket 路径；None = 按端点序解析默认值。
    :param timeout: 单请求等响应超时（秒）。
    :param reconnect: 断线自动重连（False = 断线即抛 ConnectionLost）。
    :param max_reconnect_attempts: 重连尝试次数（指数退避）。
    :param reconnect_backoff: 首次退避基数（秒），逐次翻倍。
    :param retry_on_drop: 请求中途断线时，重连后**重发一次**。
        at-least-once——编辑类命令可能双写，谨慎开启。
    """

    def __init__(
        self,
        sock_path: str | None = None,
        *,
        timeout: float = 10.0,
        reconnect: bool = True,
        max_reconnect_attempts: int = 5,
        reconnect_backoff: float = 0.25,
        retry_on_drop: bool = True,
    ) -> None:
        self._sock_path = sock_path or default_socket_path()
        self._timeout = timeout
        self._reconnect = reconnect
        self._max_attempts = max(1, max_reconnect_attempts)
        self._backoff = reconnect_backoff
        self._retry_on_drop = retry_on_drop

        self._sock: socket.socket | None = None
        self._reader_thread: threading.Thread | None = None
        self._id_counter = itertools.count(1)
        self._gen = 0  # 连接代际：每次 install +1（发送失败去重用）

        self._state_lock = threading.Lock()
        self._send_lock = threading.Lock()
        self._reconnect_lock = threading.Lock()
        self._connected = threading.Event()
        self._closed = False
        self._subscribed = False

        self._pending: dict[int, _PendingReply] = {}
        self._events: queue.Queue[DocEvent | _Sentinel] = queue.Queue()

    # ---------------- 生命周期 ----------------

    @classmethod
    def connect(
        cls,
        sock_path: str | None = None,
        **kwargs: Any,
    ) -> Client:
        """连接并返回客户端（编辑器未跑时抛 :class:`TransportError`）。

        初始连接不自动重试——AI 代理应捕获 TransportError 自行决定
        等待编辑器启动。
        """
        client = cls(sock_path, **kwargs)
        client.open()
        return client

    def open(self) -> None:
        """建立连接并启动 reader 线程（已连接则 no-op）。"""
        with self._state_lock:
            if self._closed:
                raise ConnectionLost("客户端已关闭——不可复用")
            if self._connected.is_set():
                return
        self._install_socket(self._open_socket())

    def close(self) -> None:
        """关闭连接（幂等）。事件迭代器在下次取数时干净收尾。"""
        with self._state_lock:
            if self._closed:
                return
            self._closed = True
            sock = self._sock
            self._sock = None
            self._connected.clear()
            pending = list(self._pending.values())
            self._pending.clear()
        for p in pending:
            p.error = ConnectionLost("客户端已关闭")
            p.event.set()
        self._events.put(_CLOSED)
        if sock is not None:
            with contextlib.suppress(OSError):
                sock.shutdown(socket.SHUT_RDWR)
            sock.close()
        t = self._reader_thread
        if t is not None and t is not threading.current_thread() and t.is_alive():
            t.join(timeout=2.0)

    def __enter__(self) -> Client:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    @property
    def connected(self) -> bool:
        """当前是否持有活连接。"""
        return self._connected.is_set() and not self._is_closed()

    def _is_closed(self) -> bool:
        """关闭标志的**方法**读法（非 property——mypy 会内联窄化平凡属性，
        把跨线程双检误判为 unreachable；方法调用对静态分析每次真实求值）。"""
        return self._closed

    @property
    def sock_path(self) -> str:
        return self._sock_path

    # ---------------- 传输与后台 ----------------

    def _open_socket(self) -> socket.socket:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            s.connect(self._sock_path)
        except OSError as e:
            s.close()
            raise TransportError(f"无法连接 {self._sock_path}：{e}") from e
        return s

    def _install_socket(self, sock: socket.socket) -> None:
        with self._state_lock:
            self._sock = sock
            self._connected.set()
            self._gen += 1
        self._reader_thread = threading.Thread(
            target=self._reader_loop,
            args=(sock,),
            name="mang-rpc-reader",
            daemon=True,
        )
        self._reader_thread.start()

    def _reader_loop(self, sock: socket.socket) -> None:
        """读 socket → 分流：事件进队列，响应按 id 交付 pending。"""
        cause: BaseException | None = None
        f = sock.makefile("rb")
        try:
            while True:
                line = f.readline(RECV_LINE_CAP)
                if not line:
                    break  # EOF——编辑器退出/连接关闭
                if not line.endswith(b"\n"):
                    cause = ProtocolViolation(f"服务端单行超过接收上限 {RECV_LINE_CAP} 字节")
                    break
                try:
                    obj = _decode_line(line)
                except ProtocolViolation as e:
                    cause = e
                    break
                if obj is None:
                    continue
                if _is_event(obj):
                    self._events.put(DocEvent.from_payload(obj))
                    continue
                req_id = obj.get("id")
                key = _pending_key(req_id)
                pend = self._pending.get(key) if key is not None else None
                if pend is not None:
                    pend.reply = obj
                    pend.event.set()
                else:
                    _log.debug("mang_rpc：丢弃无法匹配的响应（id=%r）", req_id)
        except OSError as e:
            cause = ConnectionLost(f"socket 读失败：{e}")
        finally:
            with contextlib.suppress(OSError):
                f.close()
            self._on_disconnect(sock, cause or ConnectionLost("连接已断开"))

    def _on_disconnect(self, sock: socket.socket, cause: BaseException) -> None:
        """连接终止：失败所有在途请求，然后按策略重连或标记事件流中断。"""
        with self._state_lock:
            if self._sock is not sock:
                return  # 旧连接的 reader——已被重连替换，无事可做
            self._sock = None
            self._connected.clear()
            pending = list(self._pending.values())
            self._pending.clear()
        for p in pending:
            p.error = cause
            p.event.set()
        if self._closed:
            return
        if self._reconnect:
            self._do_reconnect()
        else:
            self._events.put(_DROPPED)

    def _do_reconnect(self, min_gen: int = 0) -> bool:
        """重连（持锁串行化）。``min_gen``：要求产出至少这个代际的新连接——
        发送失败路径传 ``gen+1``，防止误信过时 connected 标志在死连接上重发。"""
        if self._is_closed():
            return False
        with self._reconnect_lock:
            # 双检（拿锁后重核）：close() 不需要本锁即可翻转标志。
            if self._is_closed():
                return False
            if self._gen >= min_gen and self._connected.is_set():
                return True  # 已有足够新的连接（并发去重）
            last_err = "未尝试"
            for attempt in range(self._max_attempts):
                if self._is_closed():
                    return False
                try:
                    sock = self._open_socket()
                except TransportError as e:
                    last_err = e.reason
                    if attempt < self._max_attempts - 1:
                        time.sleep(self._backoff * (2**attempt))
                    continue
                self._install_socket(sock)
                self._resubscribe()  # 失败已内部处理（注入流中断标记）
                return True
            self._events.put(_DROPPED)
            _log.warning("mang_rpc：重连耗尽（%d 次，%s）", self._max_attempts, last_err)
            return False

    def _resubscribe(self) -> bool:
        """重连后按需重发 subscribe（协议幂等）。失败注入流中断标记。"""
        if not self._subscribed:
            return True
        try:
            self._call_once("subscribe", {})
            return True
        except MangRpcError as e:
            _log.warning("mang_rpc：重连后重订阅失败——事件流中断（%s）", e)
            self._events.put(_DROPPED)
            return False

    # ---------------- 请求/响应 ----------------

    def _call_once(self, cmd: str, params: Mapping[str, Any]) -> dict[str, Any]:
        """发一次请求、按 id 等响应（不重试）。"""
        if self._closed:
            raise ConnectionLost("客户端已关闭")
        if not self._connected.is_set():
            raise ConnectionLost("连接已断开——先重连")
        req_id = next(self._id_counter)
        data = _encode_request(req_id, cmd, params)
        pend = _PendingReply()
        with self._state_lock:
            if not self._connected.is_set():
                raise ConnectionLost("连接已断开——先重连")
            sock = self._sock
            gen = self._gen
            if sock is None:
                raise ConnectionLost("连接已断开——先重连")
            self._pending[req_id] = pend
        try:
            with self._send_lock:
                try:
                    sock.sendall(data)
                except OSError as e:
                    raise _StaleConnection(f"发送失败：{e}", gen) from e
        except BaseException:
            with self._state_lock:
                if self._pending.get(req_id) is pend:
                    del self._pending[req_id]
            raise
        if not pend.event.wait(self._timeout):
            with self._state_lock:
                if self._pending.get(req_id) is pend:
                    del self._pending[req_id]
            raise RequestTimeout(cmd, self._timeout)
        if pend.error is not None:
            raise pend.error
        assert pend.reply is not None
        return _reply_result(pend.reply, cmd)

    def _call(self, cmd: str, params: Mapping[str, Any]) -> dict[str, Any]:
        """带断线恢复的调用：断线 → 重连 → （按策略）重发一次。"""
        try:
            return self._call_once(cmd, params)
        except ConnectionLost as first:
            if self._closed:
                raise
            if not (self._reconnect and self._retry_on_drop):
                raise
            # 发送失败携代际：必须产出新一代连接才可信（EPIPE 先于 EOF 察觉）
            min_gen = first.gen + 1 if isinstance(first, _StaleConnection) else 0
            if self._do_reconnect(min_gen=min_gen):
                return self._call_once(cmd, params)  # at-least-once：仅重发一次
            raise ConnectionLost("断线且自动重连失败——命令未确认执行状态") from first

    def _typed(
        self,
        cmd: str,
        params: Mapping[str, Any],
        cls: Callable[[Mapping[str, Any]], T],
    ) -> T:
        return cls(self._call(cmd, params))

    # ---------------- 原始逃生口 ----------------

    def call(self, cmd: str, /, **params: Any) -> dict[str, Any]:
        """原始调用（服务端新增命令、SDK 未跟上时的逃生口）。

        参数平铺在请求顶层（协议约定），返回 result 原始 dict。
        """
        return self._call(cmd, dict(params))

    # ---------------- 27 命令（#104 文档生命周期） ----------------

    def open_file(self, path: str) -> OpenFileResult:
        """打开文件（绝对路径）；已打开则激活既有标签，不重复开。"""
        return self._typed("open_file", {"path": path}, OpenFileResult.from_payload)

    def insert_text(self, line: int, col: int, text: str) -> InsertTextResult:
        """在指定位置插入文本（0 基；col 是字符偏移；越界钳制）。"""
        return self._typed(
            "insert_text", {"line": line, "col": col, "text": text}, InsertTextResult.from_payload
        )

    def replace_selection(self, text: str) -> ReplaceSelectionResult:
        """替换当前选区（无选区 = 光标处插入；空文本+有选区 = 删除）。"""
        return self._typed("replace_selection", {"text": text}, ReplaceSelectionResult.from_payload)

    def get_state(self) -> State:
        """标签清单 + 活动文档状态——AI 的「看屏幕」主通道。"""
        return self._typed("get_state", {}, State.from_payload)

    def close_file(self, path: str) -> CloseFileResult:
        """关闭标签（脏标签拒绝——先 save）。"""
        return self._typed("close_file", {"path": path}, CloseFileResult.from_payload)

    def switch_tab(self, index: int) -> SwitchTabResult:
        """切换标签（0 基索引）。"""
        return self._typed("switch_tab", {"index": index}, SwitchTabResult.from_payload)

    # ---------------- 27 命令（#111 导航 / 历史 / 保存 / 订阅） ----------------

    def set_cursor(self, line: int, col: int) -> SetCursorResult:
        """设光标（越界钳制；只读文档可用）。"""
        return self._typed("set_cursor", {"line": line, "col": col}, SetCursorResult.from_payload)

    def set_selection(self, start: Point, end: Point) -> SetSelectionResult:
        """设选区（两点任意次序；服务端排序后回 anchor/cursor）。"""
        return self._typed(
            "set_selection",
            {"start": _to_point(start), "end": _to_point(end)},
            SetSelectionResult.from_payload,
        )

    def get_selection(self) -> GetSelectionResult:
        """取当前选区 + 文本（上限 2000 字符；无选区 selection=None）。"""
        return self._typed("get_selection", {}, GetSelectionResult.from_payload)

    def undo(self) -> UndoResult:
        """撤销一步。栈空不是错误（undid=False），别当失败。"""
        return self._typed("undo", {}, UndoResult.from_payload)

    def redo(self) -> RedoResult:
        """重做一步。同 undo 语义。"""
        return self._typed("redo", {}, RedoResult.from_payload)

    def save(self) -> SaveResult:
        """保存当前文件（未命名/只读渲染态拒绝）。"""
        return self._typed("save", {}, SaveResult.from_payload)

    def save_as(self, path: str) -> SaveAsResult:
        """另存为（标签身份迁移到新路径）。"""
        return self._typed("save_as", {"path": path}, SaveAsResult.from_payload)

    def scroll_to(self, line: int) -> ScrollToResult:
        """滚动视口到指定行（不动光标）。"""
        return self._typed("scroll_to", {"line": line}, ScrollToResult.from_payload)

    def subscribe(self) -> Iterator[DocEvent]:
        """订阅文档变更事件，返回事件迭代器。

        - 协议幂等（重复 subscribe 替换旧通道）；断连自动注销，
          重连成功后本客户端会**自动重订阅**。
        - 断线期间的事件会丢失——恢复后对比 ``revision`` 跳号，
          跳了就 ``get_state()`` 重同步。
        - 连接彻底断开（重连耗尽/禁用）时，迭代器抛
          :class:`ConnectionLost`；客户端 close() 时迭代器干净收尾。

        用法::

            for event in c.subscribe():
                if event.source == "user":   # 人类在改
                    ...
                # event.source == "rpc" = 自己命令的回显，忽略
        """
        self._typed("subscribe", {}, SubscribeResult.from_payload)
        with self._state_lock:
            self._subscribed = True
        return self._events_iter()

    def _events_iter(self) -> Iterator[DocEvent]:
        while True:
            item = self._events.get()
            if item is _CLOSED:
                return
            if item is _DROPPED:
                raise ConnectionLost("事件流中断——连接断开且重连未成功")
            assert isinstance(item, DocEvent)
            yield item

    # ---------------- 27 命令（#134 任务 1 文件系统——沙箱内） ----------------

    def list_dir(self, path: str, *, recursive: bool = False) -> ListDirResult:
        """列目录（相对路径相对工作区根；条目上限 5000）。"""
        params: dict[str, Any] = {"path": path}
        if recursive:
            params["recursive"] = True
        return self._typed("list_dir", params, ListDirResult.from_payload)

    def read_file(self, path: str) -> ReadFileResult:
        """读文件（不碰 UI 直读盘；非 UTF-8 拒绝；上限 1 MiB）。"""
        return self._typed("read_file", {"path": path}, ReadFileResult.from_payload)

    def write_file(self, path: str, content: str) -> WriteFileResult:
        """写文件（磁盘权威；已打开干净标签自动 reload）。父目录必须存在。"""
        return self._typed(
            "write_file", {"path": path, "content": content}, WriteFileResult.from_payload
        )

    def create_file(self, path: str, content: str | None = None) -> CreateFileResult:
        """新建文件（已存在拒绝——覆盖走 write_file）。"""
        params: dict[str, Any] = {"path": path}
        if content is not None:
            params["content"] = content
        return self._typed("create_file", params, CreateFileResult.from_payload)

    def create_dir(self, path: str) -> CreateDirResult:
        """新建单层目录（父目录必须存在；已存在拒绝）。"""
        return self._typed("create_dir", {"path": path}, CreateDirResult.from_payload)

    def delete(self, path: str) -> DeleteResult:
        """删除文件/目录（目录递归；**无回收站**；工作区根自身拒绝）。"""
        return self._typed("delete", {"path": path}, DeleteResult.from_payload)

    def rename(self, src: str, dst: str) -> RenameResult:
        """重命名/移动（线上字段 from/to；不覆盖；已开标签身份迁移）。"""
        return self._typed("rename", {"from": src, "to": dst}, RenameResult.from_payload)

    # ---------------- 27 命令（#134 任务 2 编辑器命令） ----------------

    def run_command(self, name: str, args: Any | None = None) -> RunCommandResult:
        """按机器名执行编辑器命令（先 list_commands，别硬编码）。"""
        params: dict[str, Any] = {"name": name}
        if args is not None:
            params["args"] = args
        return self._typed("run_command", params, RunCommandResult.from_payload)

    def list_commands(self) -> ListCommandsResult:
        """列出可用编辑器命令（机器名 + 面板标题 + 默认键位）。"""
        return self._typed("list_commands", {}, ListCommandsResult.from_payload)

    def get_outline(self) -> GetOutlineResult:
        """取 md 标题大纲（平铺带 level；渲染态文档拒绝）。"""
        return self._typed("get_outline", {}, GetOutlineResult.from_payload)

    def apply_format(self, start: Point, end: Point, format: FormatKind) -> ApplyFormatResult:
        """应用 md 行内格式（标记对包裹；整批一条 undo；两点任意次序）。"""
        return self._typed(
            "apply_format",
            {"range": [_to_point(start), _to_point(end)], "format": format},
            ApplyFormatResult.from_payload,
        )

    # ---------------- 27 命令（#134 任务 3 批量） ----------------

    def batch_replace(
        self,
        files: list[str],
        pattern: str,
        replacement: str,
        *,
        is_regex: bool = False,
    ) -> BatchReplaceResult:
        """跨文件批量替换（沙箱内；buffer 通道可 undo、disk 通道不可）。

        想整批可撤销？先 ``open_file`` 全部文件再 batch。
        """
        params: dict[str, Any] = {
            "files": list(files),
            "pattern": pattern,
            "replacement": replacement,
        }
        if is_regex:
            params["is_regex"] = True
        return self._typed("batch_replace", params, BatchReplaceResult.from_payload)


# ===========================================================================
# 异步客户端
# ===========================================================================


class AsyncClient:
    """:class:`Client` 的 asyncio 同构版（await 调用 / async for 事件）。

    用法::

        async with await AsyncClient.connect() as ac:
            await ac.open_file("/abs/doc.md")
            await ac.insert_text(0, 0, "hello")
            state = await ac.get_state()

            async for event in await ac.subscribe():
                if event.source == "user":
                    ...

    参数语义与 :class:`Client` 完全一致（timeout / reconnect /
    max_reconnect_attempts / reconnect_backoff / retry_on_drop）。
    """

    def __init__(
        self,
        sock_path: str | None = None,
        *,
        timeout: float = 10.0,
        reconnect: bool = True,
        max_reconnect_attempts: int = 5,
        reconnect_backoff: float = 0.25,
        retry_on_drop: bool = True,
    ) -> None:
        self._sock_path = sock_path or default_socket_path()
        self._timeout = timeout
        self._reconnect = reconnect
        self._max_attempts = max(1, max_reconnect_attempts)
        self._backoff = reconnect_backoff
        self._retry_on_drop = retry_on_drop

        self._reader: asyncio.StreamReader | None = None
        self._writer: asyncio.StreamWriter | None = None
        self._reader_task: asyncio.Task[None] | None = None
        self._id_counter = itertools.count(1)
        self._gen = 0  # 连接代际：每次 install +1（发送失败去重用）

        self._reconnect_lock: asyncio.Lock | None = None  # 绑定运行循环，惰性建
        self._connected = asyncio.Event()
        self._closed = False
        self._subscribed = False

        self._pending: dict[int, asyncio.Future[dict[str, Any]]] = {}
        self._events: asyncio.Queue[DocEvent | _Sentinel] = asyncio.Queue()

    # ---------------- 生命周期 ----------------

    @classmethod
    async def connect(
        cls,
        sock_path: str | None = None,
        **kwargs: Any,
    ) -> AsyncClient:
        client = cls(sock_path, **kwargs)
        await client.open()
        return client

    async def open(self) -> None:
        if self._closed:
            raise ConnectionLost("客户端已关闭——不可复用")
        if self._connected.is_set():
            return
        await self._install_stream(await self._open_stream())

    async def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        self._connected.clear()
        for fut in self._pending.values():
            if not fut.done():
                fut.set_exception(ConnectionLost("客户端已关闭"))
        self._pending.clear()
        self._events.put_nowait(_CLOSED)
        task = self._reader_task
        self._reader_task = None
        if task is not None and not task.done():
            task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await task
        writer = self._writer
        self._writer = None
        self._reader = None
        if writer is not None:
            writer.close()
            with contextlib.suppress(OSError, asyncio.CancelledError):
                await writer.wait_closed()

    async def __aenter__(self) -> AsyncClient:
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.close()

    @property
    def connected(self) -> bool:
        return self._connected.is_set() and not self._is_closed()

    def _is_closed(self) -> bool:
        """关闭标志的方法读法（mypy 会内联窄化平凡 property——改用方法每次真实求值）。"""
        return self._closed

    @property
    def sock_path(self) -> str:
        return self._sock_path

    # ---------------- 传输与后台 ----------------

    async def _open_stream(self) -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
        try:
            return await asyncio.open_unix_connection(self._sock_path, limit=RECV_LINE_CAP)
        except OSError as e:
            raise TransportError(f"无法连接 {self._sock_path}：{e}") from e

    async def _install_stream(
        self, pair: tuple[asyncio.StreamReader, asyncio.StreamWriter]
    ) -> None:
        self._reader, self._writer = pair
        self._connected.set()
        self._gen += 1
        self._reader_task = asyncio.create_task(self._reader_loop(), name="mang-rpc-async-reader")

    async def _reader_loop(self) -> None:
        """asyncio 版 reader：事件进队列，响应按 id 交付 future。"""
        assert self._reader is not None
        cause: BaseException | None = None
        try:
            while True:
                try:
                    line = await self._reader.readline()
                except ValueError:
                    cause = ProtocolViolation(f"服务端单行超过接收上限 {RECV_LINE_CAP} 字节")
                    break
                except (OSError, asyncio.CancelledError) as e:
                    if isinstance(e, asyncio.CancelledError):
                        raise
                    cause = ConnectionLost(f"socket 读失败：{e}")
                    break
                if not line:
                    break  # EOF
                try:
                    obj = _decode_line(line)
                except ProtocolViolation as e:
                    cause = e
                    break
                if obj is None:
                    continue
                if _is_event(obj):
                    self._events.put_nowait(DocEvent.from_payload(obj))
                    continue
                req_id = obj.get("id")
                key = _pending_key(req_id)
                fut = self._pending.get(key) if key is not None else None
                if fut is not None and not fut.done():
                    fut.set_result(obj)
                else:
                    _log.debug("mang_rpc：丢弃无法匹配的响应（id=%r）", req_id)
        finally:
            await self._on_disconnect(cause or ConnectionLost("连接已断开"))

    async def _on_disconnect(self, cause: BaseException) -> None:
        if self._reader_task is None:
            return  # close() 主动收尾，不算断线
        self._reader_task = None
        self._reader = None
        self._connected.clear()
        for fut in self._pending.values():
            if not fut.done():
                fut.set_exception(cause)
        self._pending.clear()
        if self._closed:
            return
        if self._reconnect:
            await self._do_reconnect()
        else:
            self._events.put_nowait(_DROPPED)

    async def _do_reconnect(self, min_gen: int = 0) -> bool:
        if self._is_closed():
            return False
        if self._reconnect_lock is None:
            self._reconnect_lock = asyncio.Lock()
        async with self._reconnect_lock:
            if self._is_closed():
                return False
            if self._gen >= min_gen and self._connected.is_set():
                return True
            last_err = "未尝试"
            for attempt in range(self._max_attempts):
                if self._is_closed():
                    return False
                try:
                    pair = await self._open_stream()
                except TransportError as e:
                    last_err = e.reason
                    if attempt < self._max_attempts - 1:
                        await asyncio.sleep(self._backoff * (2**attempt))
                    continue
                await self._install_stream(pair)
                await self._resubscribe()
                return True
            self._events.put_nowait(_DROPPED)
            _log.warning("mang_rpc：重连耗尽（%d 次，%s）", self._max_attempts, last_err)
            return False

    async def _resubscribe(self) -> None:
        if not self._subscribed:
            return
        try:
            await self._call_once("subscribe", {})
        except MangRpcError as e:
            _log.warning("mang_rpc：重连后重订阅失败——事件流中断（%s）", e)
            self._events.put_nowait(_DROPPED)

    # ---------------- 请求/响应 ----------------

    async def _call_once(self, cmd: str, params: Mapping[str, Any]) -> dict[str, Any]:
        if self._closed:
            raise ConnectionLost("客户端已关闭")
        if not self._connected.is_set():
            raise ConnectionLost("连接已断开——先重连")
        writer = self._writer
        if writer is None:
            raise ConnectionLost("连接已断开——先重连")
        req_id = next(self._id_counter)
        data = _encode_request(req_id, cmd, params)
        gen = self._gen
        fut: asyncio.Future[dict[str, Any]] = asyncio.get_running_loop().create_future()
        self._pending[req_id] = fut
        try:
            writer.write(data)
            await writer.drain()
        except asyncio.CancelledError:
            self._pending.pop(req_id, None)
            raise
        except OSError as e:
            self._pending.pop(req_id, None)
            raise _StaleConnection(f"发送失败：{e}", gen) from e
        except BaseException:
            self._pending.pop(req_id, None)
            raise
        try:
            reply = await asyncio.wait_for(fut, timeout=self._timeout)
        except asyncio.TimeoutError:
            if self._pending.get(req_id) is fut:
                del self._pending[req_id]
            raise RequestTimeout(cmd, self._timeout) from None
        return _reply_result(reply, cmd)

    async def _call(self, cmd: str, params: Mapping[str, Any]) -> dict[str, Any]:
        try:
            return await self._call_once(cmd, params)
        except ConnectionLost as first:
            if self._closed:
                raise
            if not (self._reconnect and self._retry_on_drop):
                raise
            min_gen = first.gen + 1 if isinstance(first, _StaleConnection) else 0
            if await self._do_reconnect(min_gen=min_gen):
                return await self._call_once(cmd, params)
            raise ConnectionLost("断线且自动重连失败——命令未确认执行状态") from first

    async def _typed(
        self,
        cmd: str,
        params: Mapping[str, Any],
        cls: Callable[[Mapping[str, Any]], T],
    ) -> T:
        return cls(await self._call(cmd, params))

    # ---------------- 原始逃生口 ----------------

    async def call(self, cmd: str, /, **params: Any) -> dict[str, Any]:
        """原始调用（服务端新增命令、SDK 未跟上时的逃生口）。"""
        return await self._call(cmd, dict(params))

    # ---------------- 27 命令 ----------------

    async def open_file(self, path: str) -> OpenFileResult:
        """打开文件（绝对路径）；已打开则激活既有标签。"""
        return await self._typed("open_file", {"path": path}, OpenFileResult.from_payload)

    async def insert_text(self, line: int, col: int, text: str) -> InsertTextResult:
        """在指定位置插入文本（0 基；越界钳制）。"""
        return await self._typed(
            "insert_text", {"line": line, "col": col, "text": text}, InsertTextResult.from_payload
        )

    async def replace_selection(self, text: str) -> ReplaceSelectionResult:
        """替换当前选区。"""
        return await self._typed(
            "replace_selection", {"text": text}, ReplaceSelectionResult.from_payload
        )

    async def get_state(self) -> State:
        """标签清单 + 活动文档状态。"""
        return await self._typed("get_state", {}, State.from_payload)

    async def close_file(self, path: str) -> CloseFileResult:
        """关闭标签（脏标签拒绝）。"""
        return await self._typed("close_file", {"path": path}, CloseFileResult.from_payload)

    async def switch_tab(self, index: int) -> SwitchTabResult:
        """切换标签（0 基）。"""
        return await self._typed("switch_tab", {"index": index}, SwitchTabResult.from_payload)

    async def set_cursor(self, line: int, col: int) -> SetCursorResult:
        """设光标。"""
        return await self._typed(
            "set_cursor", {"line": line, "col": col}, SetCursorResult.from_payload
        )

    async def set_selection(self, start: Point, end: Point) -> SetSelectionResult:
        """设选区（两点任意次序）。"""
        return await self._typed(
            "set_selection",
            {"start": _to_point(start), "end": _to_point(end)},
            SetSelectionResult.from_payload,
        )

    async def get_selection(self) -> GetSelectionResult:
        """取当前选区 + 文本。"""
        return await self._typed("get_selection", {}, GetSelectionResult.from_payload)

    async def undo(self) -> UndoResult:
        """撤销一步（栈空 undid=False 不是失败）。"""
        return await self._typed("undo", {}, UndoResult.from_payload)

    async def redo(self) -> RedoResult:
        """重做一步。"""
        return await self._typed("redo", {}, RedoResult.from_payload)

    async def save(self) -> SaveResult:
        """保存当前文件。"""
        return await self._typed("save", {}, SaveResult.from_payload)

    async def save_as(self, path: str) -> SaveAsResult:
        """另存为。"""
        return await self._typed("save_as", {"path": path}, SaveAsResult.from_payload)

    async def scroll_to(self, line: int) -> ScrollToResult:
        """滚动视口（不动光标）。"""
        return await self._typed("scroll_to", {"line": line}, ScrollToResult.from_payload)

    async def subscribe(self) -> AsyncIterator[DocEvent]:
        """订阅事件，返回异步迭代器（``async for ev in await ac.subscribe()``）。

        语义同 :meth:`Client.subscribe`（幂等 / 自动重订阅 / 断流抛
        ConnectionLost）。
        """
        await self._typed("subscribe", {}, SubscribeResult.from_payload)
        self._subscribed = True
        return self._events_stream()

    async def _events_stream(self) -> AsyncIterator[DocEvent]:
        while True:
            item = await self._events.get()
            if item is _CLOSED:
                return
            if item is _DROPPED:
                raise ConnectionLost("事件流中断——连接断开且重连未成功")
            assert isinstance(item, DocEvent)
            yield item

    async def list_dir(self, path: str, *, recursive: bool = False) -> ListDirResult:
        """列目录（沙箱内；上限 5000 条）。"""
        params: dict[str, Any] = {"path": path}
        if recursive:
            params["recursive"] = True
        return await self._typed("list_dir", params, ListDirResult.from_payload)

    async def read_file(self, path: str) -> ReadFileResult:
        """读文件（不碰 UI；非 UTF-8 拒绝）。"""
        return await self._typed("read_file", {"path": path}, ReadFileResult.from_payload)

    async def write_file(self, path: str, content: str) -> WriteFileResult:
        """写文件（磁盘权威）。"""
        return await self._typed(
            "write_file", {"path": path, "content": content}, WriteFileResult.from_payload
        )

    async def create_file(self, path: str, content: str | None = None) -> CreateFileResult:
        """新建文件（已存在拒绝）。"""
        params: dict[str, Any] = {"path": path}
        if content is not None:
            params["content"] = content
        return await self._typed("create_file", params, CreateFileResult.from_payload)

    async def create_dir(self, path: str) -> CreateDirResult:
        """新建单层目录。"""
        return await self._typed("create_dir", {"path": path}, CreateDirResult.from_payload)

    async def delete(self, path: str) -> DeleteResult:
        """删除（目录递归；无回收站）。"""
        return await self._typed("delete", {"path": path}, DeleteResult.from_payload)

    async def rename(self, src: str, dst: str) -> RenameResult:
        """重命名/移动（不覆盖）。"""
        return await self._typed("rename", {"from": src, "to": dst}, RenameResult.from_payload)

    async def run_command(self, name: str, args: Any | None = None) -> RunCommandResult:
        """执行编辑器命令（先 list_commands）。"""
        params: dict[str, Any] = {"name": name}
        if args is not None:
            params["args"] = args
        return await self._typed("run_command", params, RunCommandResult.from_payload)

    async def list_commands(self) -> ListCommandsResult:
        """列出可用编辑器命令。"""
        return await self._typed("list_commands", {}, ListCommandsResult.from_payload)

    async def get_outline(self) -> GetOutlineResult:
        """取 md 大纲（渲染态拒绝）。"""
        return await self._typed("get_outline", {}, GetOutlineResult.from_payload)

    async def apply_format(self, start: Point, end: Point, format: FormatKind) -> ApplyFormatResult:
        """应用 md 行内格式。"""
        return await self._typed(
            "apply_format",
            {"range": [_to_point(start), _to_point(end)], "format": format},
            ApplyFormatResult.from_payload,
        )

    async def batch_replace(
        self,
        files: list[str],
        pattern: str,
        replacement: str,
        *,
        is_regex: bool = False,
    ) -> BatchReplaceResult:
        """跨文件批量替换（先 open_file 全部可整批 undo）。"""
        params: dict[str, Any] = {
            "files": list(files),
            "pattern": pattern,
            "replacement": replacement,
        }
        if is_regex:
            params["is_regex"] = True
        return await self._typed("batch_replace", params, BatchReplaceResult.from_payload)


# 静态自检：27 命令方法与协议命令集一一对应（导入期零成本断言——
# 漂移在 CI 的第一秒就炸，而不是用户运行时）。
def _static_check() -> None:
    for cls in (Client, AsyncClient):
        methods = {name for name in COMMANDS if callable(getattr(cls, name, None))}
        missing = COMMANDS - methods
        if missing:  # pragma: no cover——只防回归
            raise RuntimeError(f"{cls.__name__} 缺命令方法：{missing}")


_static_check()
