"""mang-mcp —— mang-shell 编辑器的 MCP server（stdio）。

把 mang-shell 的 RPC 包成 Model Context Protocol 工具，让
Claude Desktop / ChatGPT 等 MCP 宿主直接操控编辑器::

    用户：「帮我把当前打开的文档里所有 TODO 改成 NOTE」
    Claude → mang.get_state()       看当前文档
           → mang.replace_text("TODO", "NOTE")
           → （编辑器 UI 实时变化，结果 JSON 回给 Claude）

架构（两层薄封装，无业务逻辑）::

    MCP 宿主 ⇄ stdio (JSON-RPC) ⇄ 本 server ⇄ unix socket (JSON Lines) ⇄ mang-shell
                                    └── 全部转发经 mang_rpc.AsyncClient（自动重连）

连接与端点：

- 惰性连接：首次工具调用时连编辑器；编辑器没跑 → 该次调用返回
  isError + 明确指引（宿主能向用户解释，而不是挂死）。
- 端点解析同 mang_rpc：``MANG_RPC_SOCK`` → ``$XDG_RUNTIME_DIR`` →
  ``/tmp/mang-shell-$UID.sock``。
- 单进程单连接；断线由 AsyncClient 自动重连（编辑器重启后无需重启宿主）。

边界（继承自 RPC 协议，见 ``mang-shell/docs/rpc-protocol.md``）：

- 文件读写/批量替换受**工作区沙箱**约束（无工作区全拒）。
- ``replace_text`` 只作用于**活动文档**且要求其在工作区内。
- 错误是服务端人可读中文文案，经 isError 原样透出——宿主能向用户转述。
"""

from __future__ import annotations

import dataclasses
import json
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager, suppress
from typing import Any

from mang_rpc import AsyncClient
from mcp.server.fastmcp import FastMCP

__all__ = ["mcp", "main"]

#: 进程级单例——整个进程一个连接，断线自动重连（AsyncClient 内建）。
_client: AsyncClient | None = None


@asynccontextmanager
async def _lifespan(server: FastMCP) -> AsyncIterator[dict[str, Any]]:
    """进程生命周期钩子：退出前优雅关闭编辑器连接。

    在事件循环存活期内执行（比 atexit 早一步）——否则 reader 任务会
    悬挂在已关闭的循环上，退出时抛 RuntimeError + 刷 stderr 噪音。
    """
    try:
        yield {}
    finally:
        global _client
        client = _client
        _client = None
        if client is not None:
            with suppress(Exception):  # 关闭尽力而为，不阻断退出
                await client.close()


mcp = FastMCP(
    "mang",
    instructions=(
        "操控 mang-shell 编辑器：打开/编辑/保存文档、读写工作区文件、批量替换、"
        "执行编辑器命令。编辑器没在跑时工具会返回错误与启动指引。"
        "改动实时反映在编辑器 UI 上。"
    ),
    lifespan=_lifespan,
)


async def _editor() -> AsyncClient:
    """取编辑器连接（惰性建连；编辑器未跑 → TransportError → 工具错误）。"""
    global _client
    if _client is None:
        client = AsyncClient()  # 端点解析：MANG_RPC_SOCK → XDG → /tmp 兜底
        await client.open()
        _client = client
    return _client


def _dump(result: Any) -> str:
    """dataclass 结果 → 中文可读 JSON（结构化 + 人类双友好）。"""
    return json.dumps(dataclasses.asdict(result), ensure_ascii=False, indent=2)


# ---------------------------------------------------------------------------
# 文档生命周期
# ---------------------------------------------------------------------------


@mcp.tool()
async def open_file(path: str) -> str:
    """在编辑器里打开文件（绝对路径）。已打开则激活既有标签。

    参数 path：文件的绝对路径，如 /home/me/notes/todo.md
    """
    return _dump(await (await _editor()).open_file(path))


@mcp.tool()
async def close_file(path: str) -> str:
    """关闭指定文件的标签。有未保存修改时会被拒绝（先保存）。

    参数 path：标签对应文件的绝对路径
    """
    return _dump(await (await _editor()).close_file(path))


@mcp.tool()
async def get_state() -> str:
    """读编辑器现状：标签清单 + 活动文档（路径/光标/字数/**全文内容**）。

    这是「看屏幕」主通道——先调它再决定改什么。
    全文内容经 read_file 从盘上取：只在工作区内的文档带 content 字段；
    只读渲染态（docx/pdf/xlsx）或盘上读不到时不带，用 content_available=false 标记。
    """
    editor = await _editor()
    state = await editor.get_state()
    payload: dict[str, Any] = dataclasses.asdict(state)
    active = state.active
    if active is not None and active.path is not None:
        try:
            r = await editor.read_file(active.path)
            payload["content"] = r.text
            payload["content_available"] = True
        except Exception:  # 内容是尽力而为的附加信息，失败不炸状态
            payload["content_available"] = False
    else:
        payload["content_available"] = False
    return json.dumps(payload, ensure_ascii=False, indent=2)


# ---------------------------------------------------------------------------
# 编辑
# ---------------------------------------------------------------------------


@mcp.tool()
async def insert_text(text: str, line: int | None = None, col: int | None = None) -> str:
    """在文档里插入文本（行列 0 基，col 是字符偏移——中文按 1 算）。

    line/col 省略 = 插在**当前光标处**；越界坐标自动钳制到文内最近点。

    参数 text：要插入的文本，可含换行
    参数 line：目标行（0 基，省略 = 光标行）
    参数 col：目标列（0 基，省略 = 光标列）
    """
    editor = await _editor()
    if line is None or col is None:
        state = await editor.get_state()
        active = state.active
        if active is None:
            raise RuntimeError("无活动文档（空态——先 open_file）")
        line = active.cursor.line if line is None else line
        col = active.cursor.col if col is None else col
    return _dump(await editor.insert_text(line, col, text))


@mcp.tool()
async def replace_text(old: str, new: str, is_regex: bool = False) -> str:
    """把**当前活动文档**里所有 old 替换成 new（相当于「全部替换」）。

    实现走 batch_replace 的 buffer 通道：文档已打开时可一条 undo 撤销全部替换。
    边界：活动文档必须在工作区内（工作区外的文档无法批量替换）。

    参数 old：要查找的文本（或正则模式）
    参数 new：替换文本（正则模式支持 $1 捕获引用）
    参数 is_regex：old 是否按正则解释（默认 False = 字面文本）
    """
    editor = await _editor()
    state = await editor.get_state()
    active = state.active
    if active is None:
        raise RuntimeError("无活动文档（空态——先 open_file）")
    if active.path is None:
        raise RuntimeError("活动文档未命名（无路径）——先 save_as")
    return _dump(await editor.batch_replace([active.path], old, new, is_regex=is_regex))


@mcp.tool()
async def batch_replace(
    files: list[str], pattern: str, replacement: str, is_regex: bool = False
) -> str:
    """跨文件批量替换（工作区内）。返回每文件命中数。

    想整批可撤销？先用 open_file 把全部文件打开再 batch（buffer 通道一条
    undo 撤该文件全批；未打开文件直接改盘，不可 undo）。

    参数 files：文件路径清单（相对工作区根，或工作区内绝对路径）
    参数 pattern：查找文本（或正则模式）
    参数 replacement：替换文本（正则支持 $1 捕获引用）
    参数 is_regex：pattern 是否按正则解释（默认 False）
    """
    return _dump(
        await (await _editor()).batch_replace(files, pattern, replacement, is_regex=is_regex)
    )


# ---------------------------------------------------------------------------
# 文档结构 / 编辑器命令
# ---------------------------------------------------------------------------


@mcp.tool()
async def get_outline() -> str:
    """读当前 md 文档的标题大纲（层级 + 行号）——快速了解长文结构。"""
    return _dump(await (await _editor()).get_outline())


@mcp.tool()
async def list_commands() -> str:
    """列出编辑器可用命令（机器名 + 面板标题 + 默认快捷键）。

    调 run_command 前先看这个——命令名随版本变，不要凭记忆硬编码。
    """
    return _dump(await (await _editor()).list_commands())


@mcp.tool()
async def run_command(name: str) -> str:
    """按机器名执行编辑器命令（与命令面板/快捷键同一分派路径）。

    常用：toggle_fold（折叠/展开）、open_find（打开查找）、goto_line、
    select_all、undo、redo。先 list_commands 看可用清单。

    参数 name：list_commands 给出的命令机器名
    """
    return _dump(await (await _editor()).run_command(name))


# ---------------------------------------------------------------------------
# 文件系统（工作区沙箱内，不经过编辑器缓冲区）
# ---------------------------------------------------------------------------


@mcp.tool()
async def read_file(path: str) -> str:
    """直接从盘读文件内容（不打开标签、不碰 UI）。

    上限 1 MiB（超限截断并在文末标注）；非 UTF-8 文件拒绝。
    参数 path：相对工作区根的路径，或工作区内绝对路径
    """
    r = await (await _editor()).read_file(path)
    if r.truncated:
        return f"[截断：文件 {r.size} 字节超过 1 MiB 上限，仅返回前 {r.chars} 字符]\n\n" f"{r.text}"
    return r.text


@mcp.tool()
async def write_file(path: str, content: str) -> str:
    """直接把内容写到盘上（不经过编辑器缓冲区）。

    已打开的干净标签会自动 reload 同步到 UI。覆盖语义：磁盘是权威。
    参数 path：相对工作区根的路径，或工作区内绝对路径
    参数 content：完整文件内容（整体覆盖，不是追加）
    """
    return _dump(await (await _editor()).write_file(path, content))


# ---------------------------------------------------------------------------
# 入口
# ---------------------------------------------------------------------------


def main() -> None:
    """stdio 模式启动（MCP 宿主以 `mang-mcp` 命令拉起本进程）。"""
    mcp.run()  # 缺省 transport="stdio"


if __name__ == "__main__":
    main()
