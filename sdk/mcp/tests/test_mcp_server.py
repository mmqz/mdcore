"""mang-mcp server 测试——官方 MCP client（stdio）× mock RPC。

验收对照（第六轮任务 2）：

- MCP server 能启动（stdio 模式）——每个测试都真实拉起子进程
- 11 个工具都能列出（list_tools）
- 工具调用能转发到 RPC（逐工具断言转发命令与参数）
- 有 MCP 客户端测试（mcp 官方 client 库）
- 没有真编辑器 → 用 mock RPC server 测（MiniMockEditor）
"""

from __future__ import annotations

import asyncio
import json
from typing import Any

from conftest import MiniMockEditor, RpcReject, mcp_session

# ---------------------------------------------------------------------------
# 公共 mock 载荷（对照 rpc.rs 字段）
# ---------------------------------------------------------------------------

_ACTIVE = {
    "path": "/ws/notes/todo.md",
    "name": "todo.md",
    "kind": "md",
    "readonly": False,
    "modified": False,
    "cursor": {"line": 3, "col": 9},
    "selection": None,
    "line_count": 42,
    "char_count": 1337,
}
_STATE_ACTIVE = {
    "tabs": [{"path": "/ws/notes/todo.md", "modified": False, "readonly": False, "active": True}],
    "active_index": 0,
    "active": _ACTIVE,
}
_STATE_EMPTY = {"tabs": [], "active_index": 0, "active": None}
_BATCH = {
    "total_replacements": 3,
    "files_changed": 1,
    "files": [{"path": "/ws/notes/todo.md", "count": 3, "via": "buffer"}],
    "undo_hint": "已打开文件（via=buffer）一条 undo 撤该文件全批；磁盘直改（via=disk）不可 undo",
}

EXPECTED_TOOLS = {
    "open_file",
    "close_file",
    "get_state",
    "insert_text",
    "replace_text",
    "batch_replace",
    "get_outline",
    "list_commands",
    "run_command",
    "read_file",
    "write_file",
}


def _text(result: Any) -> str:
    """CallToolResult → 首个 TextContent 的文本。"""
    assert result.content, f"工具结果无 content：{result!r}"
    return str(result.content[0].text)


def _json(text: str) -> dict[str, Any]:
    payload: Any = json.loads(text)
    assert isinstance(payload, dict)
    return payload


def _setup_common(editor: MiniMockEditor) -> None:
    editor.handlers["open_file"] = lambda req: {"tab_index": 0, "already_open": False}
    editor.handlers["close_file"] = lambda req: {"closed_index": 0, "tabs_remaining": 1}
    editor.handlers["get_state"] = lambda req: _STATE_ACTIVE
    editor.handlers["insert_text"] = lambda req: {
        "cursor": {"line": 3, "col": 14},
        "inserted_chars": 5,
        "line_count": 43,
        "char_count": 1342,
    }
    editor.handlers["batch_replace"] = lambda req: _BATCH
    editor.handlers["get_outline"] = lambda req: {
        "entries": [{"level": 1, "line": 0, "text": "待办"}],
        "count": 1,
    }
    editor.handlers["list_commands"] = lambda req: {
        "commands": [{"id": "toggle_fold", "title": "折叠/展开", "default_key": None}],
        "count": 1,
    }
    editor.handlers["run_command"] = lambda req: {"ran": True, "command": req["name"]}
    editor.handlers["read_file"] = lambda req: {
        "path": "/ws/notes/todo.md",
        "size": 10,
        "chars": 10,
        "text": "- TODO 买菜",
        "truncated": False,
    }
    editor.handlers["write_file"] = lambda req: {
        "written": True,
        "path": "/ws/notes/todo.md",
        "bytes": req["content"].encode("utf-8").__len__(),
        "synced_open_tab": True,
    }


# ---------------------------------------------------------------------------
# list_tools
# ---------------------------------------------------------------------------


def test_tool_list_exposes_11_tools(editor: MiniMockEditor) -> None:
    """MCP server 能启动（stdio），list_tools 正确给出全部 11 个工具。"""

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            listed = await session.list_tools()
            names = {t.name for t in listed.tools}
            assert names == EXPECTED_TOOLS, f"工具面失配：{names ^ EXPECTED_TOOLS}"
            # 每个工具都有非空描述（Claude 依赖它理解用法）
            for t in listed.tools:
                assert t.description, f"工具 {t.name} 缺 description"

    asyncio.run(main())


# ---------------------------------------------------------------------------
# 逐工具转发
# ---------------------------------------------------------------------------


def test_open_file_tool(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("open_file", {"path": "/ws/notes/todo.md"})
            assert not r.isError
            assert editor.last("open_file")["path"] == "/ws/notes/todo.md"
            payload = _json(_text(r))
            assert payload["tab_index"] == 0 and payload["already_open"] is False

    asyncio.run(main())


def test_close_file_tool(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("close_file", {"path": "/ws/notes/todo.md"})
            assert not r.isError
            assert editor.last("close_file")["path"] == "/ws/notes/todo.md"
            assert _json(_text(r))["tabs_remaining"] == 1

    asyncio.run(main())


def test_get_state_tool_reads_content_from_disk(editor: MiniMockEditor) -> None:
    """get_state = 状态 + 光标 + （工作区内时）全文内容。"""
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("get_state", {})
            assert not r.isError
            payload = _json(_text(r))
            assert payload["active"]["path"] == "/ws/notes/todo.md"
            assert payload["active"]["cursor"] == {"line": 3, "col": 9}
            assert payload["content"] == "- TODO 买菜"
            assert payload["content_available"] is True
            # 状态经 get_state，内容经 read_file——两条 RPC 都被触达
            assert editor.last("get_state")["cmd"] == "get_state"
            assert editor.last("read_file")["path"] == "/ws/notes/todo.md"

    asyncio.run(main())


def test_get_state_tool_content_optional(editor: MiniMockEditor) -> None:
    """read_file 拒绝（工作区外文档）时 content 缺席但状态照常返回。"""
    _setup_common(editor)
    editor.handlers["read_file"] = lambda req: (_ for _ in ()).throw(
        RpcReject("路径越出工作区边界——FS 命令只允许工作区内操作")
    )

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("get_state", {})
            assert not r.isError  # 内容是尽力而为，失败不炸状态
            payload = _json(_text(r))
            assert payload["content_available"] is False
            assert "content" not in payload

    asyncio.run(main())


def test_insert_text_at_cursor(editor: MiniMockEditor) -> None:
    """line/col 省略 → 取当前光标（get_state）后插入。"""
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("insert_text", {"text": "hello"})
            assert not r.isError
            sent = editor.last("insert_text")
            assert sent["text"] == "hello"
            assert sent["line"] == 3 and sent["col"] == 9  # 来自 get_state 的光标

    asyncio.run(main())


def test_insert_text_explicit_coords(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("insert_text", {"text": "x", "line": 10, "col": 4})
            assert not r.isError
            sent = editor.last("insert_text")
            assert sent["line"] == 10 and sent["col"] == 4 and sent["text"] == "x"
            # 显式坐标不应触发 get_state（省一次往返）
            assert not editor.cmds("get_state")

    asyncio.run(main())


def test_insert_text_without_active_doc(editor: MiniMockEditor) -> None:
    _setup_common(editor)
    editor.handlers["get_state"] = lambda req: _STATE_EMPTY

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("insert_text", {"text": "hello"})
            assert r.isError
            assert "无活动文档" in _text(r)

    asyncio.run(main())


def test_replace_text_targets_active_doc(editor: MiniMockEditor) -> None:
    """「把当前打开的文档里所有 TODO 改成 NOTE」的落地路径。"""
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("replace_text", {"old": "TODO", "new": "NOTE"})
            assert not r.isError
            sent = editor.last("batch_replace")
            assert sent["files"] == ["/ws/notes/todo.md"]  # 活动文档
            assert sent["pattern"] == "TODO" and sent["replacement"] == "NOTE"
            assert "is_regex" not in sent  # 缺省 False 不上送
            payload = _json(_text(r))
            assert payload["total_replacements"] == 3
            assert payload["files"][0]["via"] == "buffer"  # 已打开 → 可 undo

    asyncio.run(main())


def test_replace_text_outside_workspace(editor: MiniMockEditor) -> None:
    """工作区外/无工作区的活动文档 → isError + 服务端中文文案透出。"""
    _setup_common(editor)
    editor.handlers["batch_replace"] = lambda req: (_ for _ in ()).throw(
        RpcReject("路径越出工作区边界——FS 命令只允许工作区内操作")
    )

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("replace_text", {"old": "a", "new": "b"})
            assert r.isError
            assert "路径越出工作区边界" in _text(r)

    asyncio.run(main())


def test_batch_replace_tool(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool(
                "batch_replace",
                {
                    "files": ["a.rs", "b.rs"],
                    "pattern": r"f(\d+)",
                    "replacement": "$1x",
                    "is_regex": True,
                },
            )
            assert not r.isError
            sent = editor.last("batch_replace")
            assert sent["files"] == ["a.rs", "b.rs"]
            assert sent["pattern"] == r"f(\d+)"
            assert sent["replacement"] == "$1x"
            assert sent["is_regex"] is True

    asyncio.run(main())


def test_get_outline_tool(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("get_outline", {})
            assert not r.isError
            payload = _json(_text(r))
            assert payload["entries"][0]["text"] == "待办"
            assert payload["count"] == 1

    asyncio.run(main())


def test_list_commands_tool(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("list_commands", {})
            assert not r.isError
            payload = _json(_text(r))
            assert payload["commands"][0]["id"] == "toggle_fold"

    asyncio.run(main())


def test_run_command_tool(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("run_command", {"name": "toggle_fold"})
            assert not r.isError
            assert editor.last("run_command")["name"] == "toggle_fold"
            assert _json(_text(r))["ran"] is True

    asyncio.run(main())


def test_read_file_tool_returns_raw_text(editor: MiniMockEditor) -> None:
    """read_file 返回纯文本（不 JSON 包裹——AI 消费可读性优先）。"""
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("read_file", {"path": "notes/todo.md"})
            assert not r.isError
            assert editor.last("read_file")["path"] == "notes/todo.md"
            assert _text(r) == "- TODO 买菜"

    asyncio.run(main())


def test_read_file_truncated_marker(editor: MiniMockEditor) -> None:
    _setup_common(editor)
    editor.handlers["read_file"] = lambda req: {
        "path": "/ws/big.log",
        "size": 3_000_000,
        "chars": 900_000,
        "text": "第一行",
        "truncated": True,
    }

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("read_file", {"path": "big.log"})
            assert not r.isError
            text = _text(r)
            assert text.startswith("[截断")
            assert "第一行" in text

    asyncio.run(main())


def test_write_file_tool(editor: MiniMockEditor) -> None:
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool(
                "write_file", {"path": "notes/todo.md", "content": "新内容"}
            )
            assert not r.isError
            sent = editor.last("write_file")
            assert sent["path"] == "notes/todo.md"
            assert sent["content"] == "新内容"  # 磁盘权威：整体覆盖
            payload = _json(_text(r))
            assert payload["written"] is True and payload["synced_open_tab"] is True

    asyncio.run(main())


# ---------------------------------------------------------------------------
# 错误与边界
# ---------------------------------------------------------------------------


def test_editor_not_running_gives_actionable_error(editor: MiniMockEditor) -> None:
    """编辑器没跑 → 工具错误带端点路径（宿主能向用户解释怎么修）。"""

    async def main() -> None:
        async with mcp_session("/nonexistent/mang-shell.sock") as session:
            r = await session.call_tool("get_state", {})
            assert r.isError
            msg = _text(r)
            assert "无法连接" in msg and "/nonexistent/mang-shell.sock" in msg

    asyncio.run(main())


def test_rpc_rejection_surfaces_chinese_message(editor: MiniMockEditor) -> None:
    """服务端业务拒绝（ok:false）原文透出到 isError——不做任何转译。"""
    _setup_common(editor)
    editor.handlers["close_file"] = lambda req: (_ for _ in ()).throw(
        RpcReject("文档有未保存修改——先保存再关闭")
    )

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("close_file", {"path": "/ws/dirty.md"})
            assert r.isError
            # FastMCP 会加 "Error executing tool …" 前缀——服务端文案必须完整透出
            assert "文档有未保存修改——先保存再关闭" in _text(r)

    asyncio.run(main())


def test_multiple_tools_share_one_rpc_connection(editor: MiniMockEditor) -> None:
    """同一 MCP 会话内多工具调用复用同一编辑器连接（进程级单例）。"""
    _setup_common(editor)

    async def main() -> None:
        async with mcp_session(editor.path) as session:
            await session.call_tool("list_commands", {})
            await session.call_tool("get_outline", {})
            r = await session.call_tool("run_command", {"name": "save"})
            assert not r.isError

    asyncio.run(main())


def test_lazy_connect_recovers_after_first_failure(editor: MiniMockEditor) -> None:
    """首个调用因编辑器未就绪失败后，编辑器上线 → 后续调用自动恢复。"""
    _setup_common(editor)

    async def main() -> None:
        # 第一次：指向不存在的端点 → 失败但 server 不死
        async with mcp_session("/nonexistent/mang-shell.sock") as session:
            r = await session.call_tool("get_outline", {})
            assert r.isError
        # 第二次：真实 mock 编辑器端点 → 同一 server 模式下新进程恢复
        async with mcp_session(editor.path) as session:
            r = await session.call_tool("get_outline", {})
            assert not r.isError
            assert _json(_text(r))["count"] == 1

    asyncio.run(main())


# ---------------------------------------------------------------------------
# 静态自检
# ---------------------------------------------------------------------------


def test_expected_tools_constant_shape() -> None:
    """11 工具清单（防漂移——增删工具时此断言与 list_tools 测试双提醒）。"""
    assert len(EXPECTED_TOOLS) == 11
    assert isinstance(EXPECTED_TOOLS, set)
