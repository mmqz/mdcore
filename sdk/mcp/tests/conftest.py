"""sdk/mcp 测试夹具。

两层：

- :class:`MiniMockEditor`——精简版假编辑器（unix socket + JSON Lines），
  只实现 MCP 工具触达的 RPC 命令子集。协议行为（id 回显 / ok-error
  二选一 / 参数平铺）与 ``mang-shell/src/rpc.rs`` 一致；完整协议行为
  已由 ``sdk/python/tests`` 覆盖，此处聚焦 **MCP 转发层**。
- :func:`mcp_session`——用 mcp 官方 client 以 stdio 拉起 ``mang-mcp``
  子进程，yield 已完成 initialize 的 :class:`ClientSession`。

子进程环境只注入 ``MANG_RPC_SOCK``（stdio 的 env 参数是整体替换，
不与默认环境合并——command 用绝对路径故无需 PATH）。
"""

from __future__ import annotations

import json
import socket
import sys
import threading
from collections.abc import AsyncIterator, Callable, Iterator, Mapping
from contextlib import asynccontextmanager, suppress
from pathlib import Path
from typing import Any

import pytest
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

_MOCK_RECV_CAP = 16 * 1024 * 1024


class RpcReject(Exception):
    """handler 抛出 → 服务端回 ``ok:false`` + error 原文。"""


Handler = Callable[[Mapping[str, Any]], Mapping[str, Any] | None]


class MiniMockEditor:
    """mang-shell 的可编程假编辑器（MCP 转发层测试专用）。"""

    def __init__(self, sock_path: str) -> None:
        self.path = sock_path
        self.handlers: dict[str, Handler] = {}
        self.received: list[dict[str, Any]] = []  # 全部收到过的请求
        self._lock = threading.Lock()
        self._live: list[socket.socket] = []
        self._server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._server.bind(sock_path)
        self._server.listen(8)
        self._accept_thread = threading.Thread(
            target=self._accept_loop, name="mini-editor-accept", daemon=True
        )
        self._accept_thread.start()

    def _accept_loop(self) -> None:
        while True:
            try:
                conn, _ = self._server.accept()
            except OSError:
                return
            with self._lock:
                self._live.append(conn)
            threading.Thread(
                target=self._serve_conn, args=(conn,), name="mini-editor-conn", daemon=True
            ).start()

    def _serve_conn(self, conn: socket.socket) -> None:
        f = conn.makefile("rb")
        try:
            while True:
                line = f.readline(_MOCK_RECV_CAP)
                if not line or not line.endswith(b"\n"):
                    break
                stripped = line.strip()
                if not stripped:
                    continue
                req: Any = json.loads(stripped)
                if not isinstance(req, dict):
                    continue
                with self._lock:
                    self.received.append(dict(req))
                    handler = self.handlers.get(str(req.get("cmd")))
                try:
                    if handler is None:
                        raise RpcReject(f"未知命令（{req.get('cmd')}）")
                    result = handler(req)
                except RpcReject as e:
                    self._send(conn, {"id": req.get("id"), "ok": False, "error": str(e)})
                except Exception as e:  # 测试失败必须可见
                    self._send(
                        conn, {"id": req.get("id"), "ok": False, "error": f"mock 崩溃：{e!r}"}
                    )
                else:
                    if result is not None:
                        self._send(conn, {"id": req.get("id"), "ok": True, "result": dict(result)})
        except OSError:
            pass
        finally:
            with suppress(OSError):
                f.close()
            conn.close()
            with self._lock:
                if conn in self._live:
                    self._live.remove(conn)

    @staticmethod
    def _send(conn: socket.socket, payload: Mapping[str, Any]) -> None:
        with suppress(OSError):
            conn.sendall((json.dumps(dict(payload), ensure_ascii=False) + "\n").encode("utf-8"))

    def close(self) -> None:
        with suppress(OSError):
            self._server.close()
        with self._lock:
            conns = list(self._live)
            self._live.clear()
        for c in conns:
            c.close()

    # ---------------- 断言辅助 ----------------

    def cmds(self, *names: str) -> list[dict[str, Any]]:
        """按名过滤收到的请求。"""
        with self._lock:
            return [r for r in self.received if r.get("cmd") in set(names)]

    def last(self, cmd: str) -> dict[str, Any]:
        rows = self.cmds(cmd)
        assert rows, f"未收到命令 {cmd}（收到 {[r.get('cmd') for r in self.received]}）"
        return rows[-1]


@asynccontextmanager
async def mcp_session(sock_path: str) -> AsyncIterator[ClientSession]:
    """以 stdio 拉起 ``mang-mcp`` console 脚本（官方 client），yield 初始化好的会话。

    用 console 脚本（与 Claude Desktop 实际拉起方式一致）而非
    ``python -m mang_mcp.server``——后者会触发 runpy 的双导入警告。
    env 是整体替换（不继承默认环境），command 用绝对路径故无需 PATH。
    """
    mang_mcp_bin = str(Path(sys.executable).with_name("mang-mcp"))
    params = StdioServerParameters(
        command=mang_mcp_bin,
        args=[],
        env={"MANG_RPC_SOCK": sock_path},
    )
    async with stdio_client(params) as (read, write), ClientSession(read, write) as session:
        await session.initialize()
        yield session


@pytest.fixture
def editor(tmp_path: Path) -> Iterator[MiniMockEditor]:
    mock = MiniMockEditor(str(tmp_path / "mang-shell-mcp-test.sock"))
    yield mock
    mock.close()
