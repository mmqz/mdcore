"""测试夹具：忠实协议的 mock 编辑器（unix socket + JSON Lines）。

对照 ``mang-shell/src/rpc.rs`` 的服务端行为逐条模拟：

- 请求 ``{"id":n,"cmd":"…",<参数平铺>}`` → 响应 ``{"id":n,"ok":true,"result":…}``
  或 ``{"id":n,"ok":false,"error":"…"}``（id 原样回显）
- handler 抛 :class:`RpcReject` → ``ok:false``；抛 :class:`DropConnection` →
  直接关连接（模拟编辑器退出/崩溃）；返回 None → 不回（模拟无响应）
- ``subscribe`` 后该连接被标记为订阅者，:meth:`MockEditor.push_event`
  向全部订阅连接主动推 ``doc_changed`` 行（响应与事件同一条流）
- 空行忽略、行读上限、逐连接服务线程——与服务端同构
"""

from __future__ import annotations

import contextlib
import json
import socket
import threading
from collections.abc import Callable, Iterator, Mapping
from pathlib import Path
from typing import Any

import pytest

#: 与被测客户端同值的接收护栏。
_MOCK_RECV_CAP = 16 * 1024 * 1024


class RpcReject(Exception):
    """handler 抛出 → 服务端回 ``ok:false`` + error 原文。"""

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


class DropConnection(Exception):
    """handler 抛出 → 不回响应、直接断连（模拟编辑器退出）。"""


#: handler 形态：请求 dict → result dict（None = 不回）。
Handler = Callable[[Mapping[str, Any]], Mapping[str, Any] | None]


class MockEditor:
    """mang-shell 的可编程假编辑器。"""

    def __init__(self, sock_path: str) -> None:
        self.path = sock_path
        self.handlers: dict[str, Handler] = {}
        self.received: list[dict[str, Any]] = []  # 全部收到过的请求（跨连接）
        self.connections = 0  # 已接受的连接数（重连测试断言用）
        self.subscribed_conns: set[int] = set()
        self.drop_all_on_connect = False  # 新连接立即断（模拟端点被占后崩溃）

        self._lock = threading.Lock()
        self._live: dict[int, socket.socket] = {}
        self._next_conn = 0
        self._server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._server.bind(sock_path)
        self._server.listen(8)
        self._accept_thread = threading.Thread(
            target=self._accept_loop, name="mock-editor-accept", daemon=True
        )
        self._accept_thread.start()

    # ---------------- 对外控制面 ----------------

    def push_event(self, payload: Mapping[str, Any]) -> None:
        """向**已订阅**连接推一行事件（真服务端只推订阅者——保持忠实）。"""
        raw = (json.dumps(dict(payload), ensure_ascii=False) + "\n").encode("utf-8")
        with self._lock:
            targets = [self._live[cid] for cid in self.subscribed_conns if cid in self._live]
        for conn in targets:
            with contextlib.suppress(OSError):
                conn.sendall(raw)

    def send_raw(self, payload: Mapping[str, Any] | str) -> None:
        """向所有活动连接推任意行（协议违规注入——测坏 JSON/超限行）。"""
        text = (
            payload if isinstance(payload, str) else json.dumps(dict(payload), ensure_ascii=False)
        )
        raw = (text + "\n").encode("utf-8")
        with self._lock:
            targets = list(self._live.values())
        for conn in targets:
            with contextlib.suppress(OSError):
                conn.sendall(raw)

    def drop_all(self) -> None:
        """断开所有活动连接（监听继续——模拟编辑器重启后可重连）。"""
        with self._lock:
            conns = list(self._live.values())
            self._live.clear()
        for c in conns:
            with contextlib.suppress(OSError):
                c.shutdown(socket.SHUT_RDWR)
            c.close()

    def close_listener(self) -> None:
        """关监听 socket（新连接全拒——重连耗尽场景）。"""
        with contextlib.suppress(OSError):
            self._server.close()

    def wait_subscribed(self, n: int = 1, timeout: float = 5.0) -> None:
        """等已订阅连接数 ≥ n（事件推送前同步用）。"""
        import time

        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                if len(self.subscribed_conns) >= n:
                    return
            time.sleep(0.01)
        raise AssertionError(f"等订阅连接 ≥ {n} 超时")

    def wait_connections(self, n: int, timeout: float = 5.0) -> None:
        """等已接受连接数 ≥ n（重连时序同步）。"""
        import time

        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                if self.connections >= n:
                    return
            time.sleep(0.01)
        raise AssertionError(f"等连接数 ≥ {n} 超时（当前 {self.connections}）")

    def close(self) -> None:
        self.close_listener()
        self.drop_all()

    # ---------------- 服务面 ----------------

    def _accept_loop(self) -> None:
        while True:
            try:
                conn, _ = self._server.accept()
            except OSError:
                return  # 监听 socket 已关
            with self._lock:
                self.connections += 1
                conn_id = self._next_conn
                self._next_conn += 1
                self._live[conn_id] = conn
                drop = self.drop_all_on_connect
            if drop:
                conn.close()
                with self._lock:
                    self._live.pop(conn_id, None)
                continue
            threading.Thread(
                target=self._serve_conn,
                args=(conn, conn_id),
                name=f"mock-editor-conn-{conn_id}",
                daemon=True,
            ).start()

    def _serve_conn(self, conn: socket.socket, conn_id: int) -> None:
        f = conn.makefile("rb")
        try:
            while True:
                line = f.readline(_MOCK_RECV_CAP)
                if not line or not line.endswith(b"\n"):
                    break  # EOF 或超限行
                stripped = line.strip()
                if not stripped:
                    continue  # 空行忽略（协议）
                try:
                    req: Any = json.loads(stripped)
                except (json.JSONDecodeError, UnicodeDecodeError):
                    self._send(
                        conn, {"id": None, "ok": False, "error": "JSON 损坏（mock 与真服务端一致）"}
                    )
                    continue
                if not isinstance(req, dict):
                    self._send(conn, {"id": None, "ok": False, "error": "非 JSON 对象"})
                    continue
                with self._lock:
                    self.received.append(dict(req))
                    handler = self.handlers.get(str(req.get("cmd")))
                try:
                    if handler is None:
                        raise RpcReject(f"未知命令（{req.get('cmd')}）")
                    result = handler(req)
                except RpcReject as e:
                    self._send(conn, {"id": req.get("id"), "ok": False, "error": e.message})
                except DropConnection:
                    break  # 断连——不回响应
                except Exception as e:  # noqa: BLE001  # 测试失败必须可见
                    self._send(
                        conn,
                        {"id": req.get("id"), "ok": False, "error": f"mock handler 崩溃：{e!r}"},
                    )
                else:
                    if result is not None:
                        self._send(conn, {"id": req.get("id"), "ok": True, "result": dict(result)})
                    if req.get("cmd") == "subscribe" and result is not None:
                        with self._lock:
                            self.subscribed_conns.add(conn_id)
        except OSError:
            pass
        finally:
            with contextlib.suppress(OSError):
                f.close()
            conn.close()
            with self._lock:
                self._live.pop(conn_id, None)
                self.subscribed_conns.discard(conn_id)

    @staticmethod
    def _send(conn: socket.socket, payload: Mapping[str, Any]) -> None:
        with contextlib.suppress(OSError):
            conn.sendall((json.dumps(dict(payload), ensure_ascii=False) + "\n").encode("utf-8"))


@pytest.fixture
def server(tmp_path: Path) -> Iterator[MockEditor]:
    """每个测试一台假编辑器，路径在 tmp 下（自动清理）。"""
    editor = MockEditor(str(tmp_path / "mang-shell-test.sock"))
    yield editor
    editor.close()
