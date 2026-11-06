"""mang_rpc 客户端测试——全程 mock socket（不需要真编辑器）。

覆盖面：

- 27 命令逐一回环：命令名、参数平铺、类型化结果解析（表驱动）
- 错误包装（ok:false → RpcError，原文保留，不做字符串匹配）
- 上下文管理器 / 线程并发 / id 乱序匹配 / 超时
- 断连自动重连 + 重发 + 自动重订阅 / 重连耗尽 / 禁用重连
- 事件流（user/rpc 双源、kind-only undo 记录、断流抛错、close 收尾）
- 发送侧 1 MiB 单行护栏（连接不破）
- AsyncClient 主路径（含重连与事件流）
"""

from __future__ import annotations

import asyncio
import itertools
import threading
import time
from collections.abc import Callable, Mapping
from typing import Any

import pytest
from conftest import DropConnection, MockEditor, RpcReject

import mang_rpc
from mang_rpc import (
    COMMANDS,
    MAX_LINE_BYTES,
    ActiveDoc,
    AsyncClient,
    Client,
    ConnectionLost,
    DocEvent,
    LineTooLong,
    Position,
    ProtocolViolation,
    RequestTimeout,
    RpcError,
    State,
    TransportError,
    default_socket_path,
)

# ---------------------------------------------------------------------------
# 协议面静态断言
# ---------------------------------------------------------------------------


def test_commands_set_is_27_and_methods_exist() -> None:
    """27 命令集齐全；两个客户端都有同名方法（缺一导入期就会炸）。"""
    assert len(COMMANDS) == 27
    for cls in (Client, AsyncClient):
        for name in sorted(COMMANDS):
            assert callable(getattr(cls, name, None)), f"{cls.__name__}.{name} 缺失"


def test_protocol_constants_pinned() -> None:
    """与服务端 rpc.rs 常量对齐（漂移即测试失败——防静默失配）。"""
    assert MAX_LINE_BYTES == 1024 * 1024
    assert mang_rpc.LIST_DIR_MAX_ENTRIES == 5000
    assert mang_rpc.SELECTION_TEXT_CAP == 2000
    assert mang_rpc.READ_FILE_CAP_BYTES == 1024 * 1024
    assert mang_rpc.BATCH_MAX_FILES == 200
    assert mang_rpc.BATCH_MAX_HITS_PER_FILE == 10_000


def test_default_socket_path_resolution(monkeypatch: Any) -> None:
    monkeypatch.delenv("MANG_RPC_SOCK", raising=False)
    monkeypatch.setenv("XDG_RUNTIME_DIR", "/run/user/1000")
    assert default_socket_path() == "/run/user/1000/mang-shell.sock"

    monkeypatch.setenv("MANG_RPC_SOCK", "/custom/mine.sock")
    assert default_socket_path() == "/custom/mine.sock"

    monkeypatch.delenv("MANG_RPC_SOCK", raising=False)
    monkeypatch.delenv("XDG_RUNTIME_DIR", raising=False)
    path = default_socket_path()
    assert path.startswith("/tmp/mang-shell-") and path.endswith(".sock")


# ---------------------------------------------------------------------------
# 27 命令表驱动回环
# ---------------------------------------------------------------------------


def assertion(cond: bool) -> bool:
    """表驱动断言：不满足即炸；返回 bool 供 lambda 组合（mypy 口径）。"""
    assert cond
    return True


def _check_state(r: Any) -> None:
    assert isinstance(r, State)
    assert len(r.tabs) == 1
    assert r.tabs[0].path == "/ws/a.md" and r.tabs[0].modified is True
    assert isinstance(r.active, ActiveDoc)
    assert r.active.name == "a.md" and r.active.kind == "md"
    assert r.active.cursor == Position(3, 9)
    assert r.active.selection is not None
    assert r.active.selection.chars == 20 and r.active.selection.text == "hello"
    assert r.active.line_count == 42 and r.active.char_count == 1337


def _check_batch(r: Any) -> None:
    assert r.total_replacements == 3
    assert r.files_changed == 2
    via = {f.path: f.via for f in r.files}
    assert via == {
        "/ws/a.rs": "buffer",
        "/ws/b.rs": "disk",
        "/ws/c.rs": "skipped",
        "/ws/d.rs": "disk",
    }
    skipped = r.files[2]
    assert skipped.error is not None and skipped.count == 0
    assert r.files[3].note == "无命中"
    assert "undo" in r.undo_hint


_CURSOR: dict[str, Any] = {"line": 3, "col": 9}
_SELECTION: dict[str, Any] = {
    "start": {"line": 0, "col": 0},
    "end": {"line": 1, "col": 4},
    "chars": 20,
    "text": "hello",
    "text_truncated": False,
}
_ACTIVE_DOC: dict[str, Any] = {
    "path": "/ws/a.md",
    "name": "a.md",
    "kind": "md",
    "readonly": False,
    "modified": True,
    "cursor": _CURSOR,
    "selection": _SELECTION,
    "line_count": 42,
    "char_count": 1337,
}
_STATE: dict[str, Any] = {
    "tabs": [{"path": "/ws/a.md", "modified": True, "readonly": False, "active": True}],
    "active_index": 0,
    "active": _ACTIVE_DOC,
}
_LIST_DIR: dict[str, Any] = {
    "path": "/ws/src",
    "entries": [
        {
            "name": "main.rs",
            "path": "/ws/src/main.rs",
            "rel": "src/main.rs",
            "is_dir": False,
            "size": 1234,
        },
        {"name": "util", "path": "/ws/src/util", "rel": "src/util", "is_dir": True, "size": 0},
        # metadata 失败变体：只有 name/path/error
        {"name": "broken", "path": "/ws/src/broken", "error": "metadata 失败：fake"},
    ],
    "count": 3,
    "truncated": False,
}
_BATCH: dict[str, Any] = {
    "total_replacements": 3,
    "files_changed": 2,
    "files": [
        {"path": "/ws/a.rs", "count": 2, "via": "buffer"},
        {"path": "/ws/b.rs", "count": 1, "via": "disk"},
        {"path": "/ws/c.rs", "count": 0, "via": "skipped", "error": "非 UTF-8 文本"},
        {"path": "/ws/d.rs", "count": 0, "via": "disk", "note": "无命中"},
    ],
    "undo_hint": "已打开文件（via=buffer）一条 undo 撤该文件全批；磁盘直改（via=disk）不可 undo",
}

#: 每条：(命令名, 调用表达式, 期望请求参数（不含 id/cmd）, 服务端 result, 结果断言)
_CASES: list[
    tuple[str, Callable[[Client], Any], dict[str, Any], dict[str, Any], Callable[[Any], Any]]
] = [
    (
        "open_file",
        lambda c: c.open_file("/tmp/a.md"),
        {"path": "/tmp/a.md"},
        {"tab_index": 2, "already_open": True},
        lambda r: (assertion(r.tab_index == 2 and r.already_open is True)),
    ),
    (
        "insert_text",
        lambda c: c.insert_text(3, 4, "hi\n"),
        {"line": 3, "col": 4, "text": "hi\n"},
        {"cursor": _CURSOR, "inserted_chars": 5, "line_count": 10, "char_count": 100},
        lambda r: (
            assertion(r.cursor == Position(3, 9)),
            assertion(r.inserted_chars == 5 and r.line_count == 10 and r.char_count == 100),
        ),
    ),
    (
        "replace_selection",
        lambda c: c.replace_selection("X"),
        {"text": "X"},
        {"had_selection": True, "cursor": _CURSOR, "char_count": 42},
        lambda r: assertion(r.had_selection is True and r.char_count == 42),
    ),
    (
        "get_state",
        lambda c: c.get_state(),
        {},
        _STATE,
        _check_state,
    ),
    (
        "close_file",
        lambda c: c.close_file("/tmp/a.md"),
        {"path": "/tmp/a.md"},
        {"closed_index": 1, "tabs_remaining": 2},
        lambda r: assertion(r.closed_index == 1 and r.tabs_remaining == 2),
    ),
    (
        "switch_tab",
        lambda c: c.switch_tab(2),
        {"index": 2},
        {"active_index": 2},
        lambda r: assertion(r.active_index == 2),
    ),
    (
        "set_cursor",
        lambda c: c.set_cursor(10, 5),
        {"line": 10, "col": 5},
        {"cursor": {"line": 11, "col": 6}},
        lambda r: assertion(r.cursor == Position(11, 6)),  # 服务端钳制后回实际值
    ),
    (
        "set_selection",
        lambda c: c.set_selection((0, 1), (1, 5)),
        {"start": {"line": 0, "col": 1}, "end": {"line": 1, "col": 5}},
        {"selection": {"start": _SELECTION["start"], "end": _SELECTION["end"]}},
        lambda r: assertion(r.start == Position(0, 0) and r.end == Position(1, 4)),
    ),
    (
        "get_selection",
        lambda c: c.get_selection(),
        {},
        {"path": "/ws/a.md", "revision": 7, "selection": _SELECTION},
        lambda r: (
            assertion(r.path == "/ws/a.md" and r.revision == 7),
            assertion(r.selection is not None and r.selection.chars == 20),
        ),
    ),
    (
        "undo",
        lambda c: c.undo(),
        {},
        {"undid": True, "revision": 41},
        lambda r: assertion(r.undid is True and r.revision == 41),
    ),
    (
        "redo",
        lambda c: c.redo(),
        {},
        {"redid": False, "revision": 41},  # 栈空 ok:true + redid:false——不是错误
        lambda r: assertion(r.redid is False and r.revision == 41),
    ),
    (
        "save",
        lambda c: c.save(),
        {},
        {"saved": True, "path": "/ws/a.md", "modified": False},
        lambda r: assertion(r.saved is True and r.modified is False),
    ),
    (
        "save_as",
        lambda c: c.save_as("/tmp/b.md"),
        {"path": "/tmp/b.md"},
        {"saved": True, "path": "/tmp/b.md", "tab_index": 0},
        lambda r: assertion(r.saved is True and r.tab_index == 0),
    ),
    (
        "scroll_to",
        lambda c: c.scroll_to(42),
        {"line": 42},
        {"scrolled_to": 42, "line_count": 100},
        lambda r: assertion(r.scrolled_to == 42 and r.line_count == 100),
    ),
    (
        "list_dir",
        lambda c: c.list_dir("src"),
        {"path": "src"},  # recursive 缺省 False → 不出现在请求里
        _LIST_DIR,
        lambda r: (
            assertion(r.count == 3 and r.truncated is False),
            assertion(r.entries[0].is_dir is False and r.entries[0].size == 1234),
            assertion(r.entries[1].is_dir is True),
            assertion(r.entries[2].error is not None and r.entries[2].is_dir is None),
        ),
    ),
    (
        "list_dir",
        lambda c: c.list_dir("src", recursive=True),
        {"path": "src", "recursive": True},
        _LIST_DIR,
        lambda r: assertion(r.count == 3),
    ),
    (
        "read_file",
        lambda c: c.read_file("src/main.rs"),
        {"path": "src/main.rs"},
        {
            "path": "/ws/src/main.rs",
            "size": 11,
            "chars": 11,
            "text": "fn main(){}",
            "truncated": False,
        },
        lambda r: assertion(r.text == "fn main(){}" and r.truncated is False),
    ),
    (
        "write_file",
        lambda c: c.write_file("src/lib.rs", "content"),
        {"path": "src/lib.rs", "content": "content"},
        {"written": True, "path": "/ws/src/lib.rs", "bytes": 7, "synced_open_tab": True},
        lambda r: assertion(r.written is True and r.synced_open_tab is True),
    ),
    (
        "create_file",
        lambda c: c.create_file("src/new.rs"),
        {"path": "src/new.rs"},  # content 缺省 None → 不出现
        {"created": True, "path": "/ws/src/new.rs", "bytes": 0},
        lambda r: assertion(r.created is True),
    ),
    (
        "create_file",
        lambda c: c.create_file("src/new.rs", "x = 1"),
        {"path": "src/new.rs", "content": "x = 1"},
        {"created": True, "path": "/ws/src/new.rs", "bytes": 5},
        lambda r: assertion(r.bytes == 5),
    ),
    (
        "create_dir",
        lambda c: c.create_dir("src/util"),
        {"path": "src/util"},
        {"created": True, "path": "/ws/src/util"},
        lambda r: assertion(r.created is True),
    ),
    (
        "delete",
        lambda c: c.delete("src/old.rs"),
        {"path": "src/old.rs"},
        {"deleted": True, "path": "/ws/src/old.rs", "was_dir": False, "closed_tabs": 1},
        lambda r: assertion(r.deleted is True and r.closed_tabs == 1),
    ),
    (
        "rename",
        lambda c: c.rename("src/a.rs", "src/b.rs"),
        {"from": "src/a.rs", "to": "src/b.rs"},  # 线上字段 from/to
        {"renamed": True, "from": "/ws/src/a.rs", "to": "/ws/src/b.rs", "migrated_tabs": 1},
        lambda r: assertion(
            r.renamed is True and r.src == "/ws/src/a.rs" and r.dst == "/ws/src/b.rs"
        ),
    ),
    (
        "run_command",
        lambda c: c.run_command("goto_line"),
        {"name": "goto_line"},
        {"ran": True, "command": "goto_line"},
        lambda r: assertion(r.ran is True and r.command == "goto_line"),
    ),
    (
        "run_command",
        lambda c: c.run_command("goto_line", {"line": 5}),
        {"name": "goto_line", "args": {"line": 5}},  # 前向兼容字段透传
        {"ran": True, "command": "goto_line"},
        lambda r: assertion(r.ran is True),
    ),
    (
        "list_commands",
        lambda c: c.list_commands(),
        {},
        {
            "commands": [
                {"id": "goto_line", "title": "转到行…", "default_key": "Ctrl+G"},
                {"id": "undo", "title": "撤销", "default_key": None},
            ],
            "count": 2,
        },
        lambda r: (
            assertion(r.count == 2),
            assertion(r.commands[0].default_key == "Ctrl+G"),
            assertion(r.commands[1].default_key is None),
        ),
    ),
    (
        "get_outline",
        lambda c: c.get_outline(),
        {},
        {
            "entries": [
                {"level": 1, "line": 0, "text": "标题"},
                {"level": 2, "line": 5, "text": "子"},
            ],
            "count": 2,
        },
        lambda r: (
            assertion(r.count == 2),
            assertion(r.entries[0].level == 1 and r.entries[1].text == "子"),
        ),
    ),
    (
        "apply_format",
        lambda c: c.apply_format((0, 0), (0, 4), "bold"),
        {"range": [{"line": 0, "col": 0}, {"line": 0, "col": 4}], "format": "bold"},
        {"applied": True, "format": "bold", "cursor": _CURSOR, "chars": 100},
        lambda r: assertion(r.applied is True and r.format == "bold" and r.chars == 100),
    ),
    (
        "batch_replace",
        lambda c: c.batch_replace(["a.rs", "b.rs"], "foo", "bar"),
        {"files": ["a.rs", "b.rs"], "pattern": "foo", "replacement": "bar"},
        _BATCH,
        _check_batch,
    ),
    (
        "batch_replace",
        lambda c: c.batch_replace(["a.rs"], r"f(\d+)", "$1x", is_regex=True),
        {"files": ["a.rs"], "pattern": r"f(\d+)", "replacement": "$1x", "is_regex": True},
        _BATCH,
        _check_batch,
    ),
]


@pytest.mark.parametrize(
    ("cmd", "invoke", "expected_params", "result", "check"),
    [
        pytest.param(*case, id=f"{i}")
        for i, case in enumerate((c[0], c[1], c[2], c[3], c[4]) for c in _CASES)
    ],
)
def test_command_roundtrip(
    cmd: str,
    invoke: Callable[[Client], Any],
    expected_params: dict[str, Any],
    result: dict[str, Any],
    check: Callable[[Any], None],
    server: MockEditor,
) -> None:
    """每条命令：请求名/参数平铺正确 + 响应解析成类型化结果。"""
    server.handlers[cmd] = lambda req: result

    with Client.connect(server.path) as c:
        out = invoke(c)
        check(out)

    # 请求核对：cmd 名、id 自增、参数平铺在顶层（不是 params 包装）
    sent = [r for r in server.received if r.get("cmd") == cmd]
    assert sent, f"{cmd} 请求未到达"
    req = sent[-1]
    assert set(req) - {"id", "cmd"} == set(expected_params), f"{cmd} 参数失配：{req}"
    for key, val in expected_params.items():
        assert req[key] == val, f"{cmd}.{key} 期望 {val!r}，实发 {req[key]!r}"
    assert isinstance(req["id"], int)
    assert all("params" not in r for r in server.received), "协议要求参数平铺，不得出现 params 包装"


def test_subscribe_sends_command_and_returns_iterator(server: MockEditor) -> None:
    """subscribe() 立即发命令（非惰性），返回迭代器。"""
    server.handlers["subscribe"] = lambda req: {"subscribed": True}
    with Client.connect(server.path) as c:
        it = c.subscribe()
        server.wait_subscribed(1)
        assert hasattr(it, "__next__")
        reqs = [r for r in server.received if r.get("cmd") == "subscribe"]
        assert reqs, "subscribe 命令未发出（不得等到首迭代才发）"
        assert reqs[-1].get("id") is not None


# ---------------------------------------------------------------------------
# 错误处理
# ---------------------------------------------------------------------------


def test_rpc_error_wrapped_with_original_message(server: MockEditor) -> None:
    """ok:false → RpcError；中文原文保留；id/cmd 供诊断（不匹配字符串）。"""
    server.handlers["close_file"] = lambda req: (_ for _ in ()).throw(
        RpcReject("文档有未保存修改——先保存再关闭")
    )
    with Client.connect(server.path) as c, pytest.raises(RpcError) as ei:
        c.close_file("/tmp/dirty.md")
    assert ei.value.message == "文档有未保存修改——先保存再关闭"
    assert ei.value.cmd == "close_file"
    assert ei.value.id is not None


def test_transport_error_when_editor_not_running(tmp_path: Any) -> None:
    with pytest.raises(TransportError):
        Client.connect(str(tmp_path / "nonexistent.sock"))


def test_malformed_response_is_protocol_violation(server: MockEditor) -> None:
    """服务端吐非法 JSON → 在途请求以 ProtocolViolation 失败（连接断）。"""

    def inject_garbage(req: Mapping[str, Any]) -> dict[str, Any] | None:
        server.send_raw("this is not json")  # 回应位置注入坏行
        return None  # 不回正常响应

    server.handlers["get_state"] = inject_garbage
    with Client.connect(server.path) as c, pytest.raises(ProtocolViolation):
        c.get_state()


def test_hostile_non_numeric_id_dropped_reader_survives(server: MockEditor) -> None:
    """P2-29（#282）：敌意响应 ``"id":"evil"`` → 无法匹配被丢弃，reader 存活。

    旧行为：裸 ``int("evil")`` 抛 ValueError——reader 的 except 面只接
    OSError/ProtocolViolation，线程直接炸；在途请求以 ConnectionLost
    失败，后续调用全靠重连。新行为：该行走既有「无法匹配 → 丢弃」路径，
    正常响应仍按 id 匹配交付。
    """

    def hostile_then_normal(req: Mapping[str, Any]) -> dict[str, Any]:
        server.send_raw({"id": "evil", "ok": True, "result": {}})
        return dict(_STATE)

    server.handlers["get_state"] = hostile_then_normal
    with Client.connect(server.path) as c:
        st = c.get_state()  # 敌意行已吞——正常响应照常交付
        assert st.active is not None and st.active.char_count == 1337
        st2 = c.get_state()  # reader 未崩——连接与 id 匹配机制完好
        assert st2.active is not None and c.connected


def test_hostile_id_variants_dropped_not_reader_crash(server: MockEditor) -> None:
    """P2-29（#282）：列表/对象/inf/id 缺席等敌意形态同走丢弃路径不炸 reader。

    ``1e999`` 经 Python json 两端宽容解析为 float("inf")——钉住
    OverflowError 分支（裸 int(inf) 的第三种异常面）。
    """
    variants: list[dict[str, Any]] = [
        {"id": ["evil"], "ok": True, "result": {}},  # TypeError 面
        {"id": {"evil": 1}, "ok": True, "result": {}},  # TypeError 面
        {"id": 1e999, "ok": True, "result": {}},  # OverflowError 面
        {"ok": True, "result": {}},  # id 缺席（旧代码靠 is not None 守卫，新代码统一走 None）
    ]

    def hostile_then_normal(req: Mapping[str, Any]) -> dict[str, Any]:
        for v in variants:
            server.send_raw(v)
        return dict(_STATE)

    server.handlers["get_state"] = hostile_then_normal
    with Client.connect(server.path) as c:
        st = c.get_state()  # 四行敌意全吞——正常响应照常交付
        assert st.active is not None
        st2 = c.get_state()  # reader 仍活
        assert st2.active is not None and c.connected


def test_request_timeout_keeps_connection_usable(server: MockEditor) -> None:
    """第一次不回 → RequestTimeout；连接与 id 匹配机制完好，第二次照常。"""
    seen = {"n": 0}

    def swallow_first(req: Mapping[str, Any]) -> dict[str, Any] | None:
        seen["n"] += 1
        if seen["n"] == 1:
            return None  # 模拟无响应（不发回任何行）
        return {"ok_marker": 1}

    server.handlers["get_state"] = swallow_first
    with Client.connect(server.path, timeout=0.2) as c:
        with pytest.raises(RequestTimeout):
            c.call("get_state")
        assert c.call("get_state") == {"ok_marker": 1}
    assert seen["n"] == 2


def test_line_too_long_guarded_before_send(server: MockEditor) -> None:
    """1 MiB 单行上限客户端先挡——连接不破，后续命令照常。"""
    server.handlers["get_state"] = lambda req: {}
    big = "x" * (MAX_LINE_BYTES + 10)
    with Client.connect(server.path) as c:
        with pytest.raises(LineTooLong):
            c.insert_text(0, 0, big)
        assert c.call("get_state") == {}  # 连接仍然健康


def test_raw_call_escape_hatch(server: MockEditor) -> None:
    """call() 原始逃生口：SDK 未收录的未来新命令可用。"""
    server.handlers["future_cmd"] = lambda req: {"whatever": 1}
    with Client.connect(server.path) as c:
        assert c.call("future_cmd", a=1, b="x") == {"whatever": 1}
    assert server.received[-1]["a"] == 1 and server.received[-1]["b"] == "x"


# ---------------------------------------------------------------------------
# 并发 / id 匹配
# ---------------------------------------------------------------------------


def test_out_of_order_responses_matched_by_id(server: MockEditor) -> None:
    """响应乱序到达——按 id 匹配（慢响应后到，快响应先到，互不串线）。"""

    def delayed_undo(req: Mapping[str, Any]) -> dict[str, Any] | None:
        def later() -> None:
            time.sleep(0.3)  # 晚于后续请求的响应——制造乱序
            server.send_raw({"id": req["id"], "ok": True, "result": {"undid": True, "revision": 2}})

        threading.Thread(target=later, daemon=True).start()
        return None  # 本轮不回——交给延迟线程

    server.handlers["undo"] = delayed_undo
    server.handlers["get_state"] = lambda req: {"tabs": [], "active_index": 0, "active": None}

    with Client.connect(server.path, timeout=5.0) as c:
        out: dict[str, Any] = {}

        def do_undo() -> None:
            out["undo"] = c.undo()

        def do_state() -> None:
            out["state"] = c.get_state()

        t1 = threading.Thread(target=do_undo)
        t1.start()
        time.sleep(0.05)  # undo 请求先到（响应被延迟）
        t2 = threading.Thread(target=do_state)
        t2.start()  # get_state 后发但先回——乱序场景成立
        t1.join(timeout=5)
        t2.join(timeout=5)

    # 各自拿到**自己**命令的结果（id 匹配正确性的直接证据）
    assert out["undo"].undid is True and out["undo"].revision == 2
    assert out["state"].active is None


def test_concurrent_calls_thread_safety(server: MockEditor) -> None:
    """4 线程 × 27 请求并发——id 匹配不串线（响应 marker 必须等于自身命令）。"""

    def echo(req: Mapping[str, Any]) -> dict[str, Any]:
        return {"marker": req["cmd"]}

    for cmd in ("get_state", "undo", "redo"):
        server.handlers[cmd] = echo

    with Client.connect(server.path, timeout=10.0) as c:
        errors: list[BaseException] = []

        def worker(cmds: list[str]) -> None:
            for cmd in cmds:
                try:
                    result = c.call(cmd)
                    if result.get("marker") != cmd:
                        errors.append(AssertionError(f"响应串线：{cmd} 收到 {result!r}"))
                except BaseException as e:  # noqa: BLE001  # 测试收集一切
                    errors.append(e)

        seqs = [["get_state", "undo", "redo"] * 9 for _ in range(4)]
        threads = [threading.Thread(target=worker, args=(s,)) for s in seqs]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, errors[:3]
        assert len(server.received) == 108
        for r in server.received:
            assert r["cmd"] in {"get_state", "undo", "redo"}


# ---------------------------------------------------------------------------
# 断连 / 自动重连
# ---------------------------------------------------------------------------


def test_reconnect_and_resend_transparently(server: MockEditor) -> None:
    """请求途中断连 → 自动重连 + 重发一次，调用方拿到正常结果。"""

    def handler(req: Mapping[str, Any]) -> dict[str, Any]:
        if server.connections <= 1:
            raise DropConnection  # 第一条连接：收到即断（不回）
        return {
            "tabs": [{"path": "/a.md", "modified": False, "readonly": False, "active": True}],
            "active_index": 0,
            "active": None,
        }

    server.handlers["get_state"] = handler
    with Client.connect(server.path, reconnect_backoff=0.05) as c:
        state = c.get_state()
    assert state.active is None
    assert server.connections == 2


def test_resubscribe_after_reconnect(server: MockEditor) -> None:
    """订阅中断连 → 重连后自动重订阅；事件在新连接上继续流入。"""
    server.handlers["subscribe"] = lambda req: {"subscribed": True}
    server.handlers["get_state"] = lambda req: {"marker": 1}

    with Client.connect(server.path, reconnect_backoff=0.05) as c:
        it = c.subscribe()
        server.wait_subscribed(1)

        server.drop_all()  # 编辑器崩了

        # 断线期间发起命令 → 触发重连 → 自动重订阅
        assert c.call("get_state") == {"marker": 1}
        server.wait_connections(2)
        server.wait_subscribed(1)

        server.push_event(_doc_changed(source="user", revision=42, revision_before=41))
        got = list(itertools.islice(it, 1))
        assert len(got) == 1
        assert got[0].source == "user" and got[0].revision == 42


def test_reconnect_exhaustion_raises_connection_lost(server: MockEditor) -> None:
    """重连次数耗尽 → ConnectionLost（调用方明确失败，而不是挂死）。"""
    with Client.connect(server.path, reconnect_backoff=0.02, max_reconnect_attempts=3) as c:
        server.handlers["get_state"] = lambda req: (_ for _ in ()).throw(DropConnection)
        server.drop_all()  # 当前连接全断
        server.close_listener()  # 新连接全拒（模拟编辑器真退了）
        with pytest.raises((ConnectionLost, TransportError)):
            c.get_state()
    # 客户端 close() 幂等
    c.close()


def test_reconnect_disabled_fails_fast(server: MockEditor) -> None:
    server.handlers["get_state"] = lambda req: (_ for _ in ()).throw(DropConnection)
    with Client.connect(server.path, reconnect=False) as c, pytest.raises(ConnectionLost):
        c.get_state()


def test_subscribe_stream_interrupted_raises(server: MockEditor) -> None:
    """事件流中连接断且不可恢复 → 迭代器抛 ConnectionLost。"""
    server.handlers["subscribe"] = lambda req: {"subscribed": True}
    with Client.connect(server.path, reconnect=False) as c:
        it = c.subscribe()
        server.wait_connections(1)
        server.drop_all()
        with pytest.raises(ConnectionLost):
            next(it)


def test_close_ends_iterator_cleanly(server: MockEditor) -> None:
    """close() 后事件迭代器干净收尾（StopIteration，不抛错）。"""
    server.handlers["subscribe"] = lambda req: {"subscribed": True}
    c = Client.connect(server.path)
    it = c.subscribe()
    server.wait_connections(1)
    c.close()
    with pytest.raises(StopIteration):
        next(it)


# ---------------------------------------------------------------------------
# 事件流
# ---------------------------------------------------------------------------


def _doc_changed(
    source: str = "user",
    revision: int = 42,
    revision_before: int = 41,
    changes: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    return {
        "type": "doc_changed",
        "path": "/tmp/a.md",
        "revision_before": revision_before,
        "revision": revision,
        "source": source,
        "changes": (
            changes
            if changes is not None
            else [
                {
                    "kind": "replace",
                    "start": {"line": 10, "col": 5},
                    "end": {"line": 10, "col": 5},
                    "text": "hi",
                }
            ]
        ),
    }


def test_subscribe_yields_typed_events(server: MockEditor) -> None:
    """事件与响应同流分发；user/rpc 双源可区分；结构化字段完整。"""
    server.handlers["subscribe"] = lambda req: {"subscribed": True}
    server.handlers["get_state"] = lambda req: {}

    with Client.connect(server.path) as c:
        it = c.subscribe()
        server.wait_subscribed(1)

        # 响应与事件交错——客户端必须正确分流（事件进队列、响应归 id）
        server.push_event(_doc_changed(source="user", revision=42, revision_before=41))
        server.push_event(_doc_changed(source="rpc", revision=43, revision_before=42))
        assert c.call("get_state") == {}
        server.push_event(
            _doc_changed(
                source="user",
                revision=44,
                revision_before=43,
                changes=[{"kind": "undo"}],  # 批量撤销：kind-only 粗粒度
            )
        )

        events = list(itertools.islice(it, 3))

    assert [e.source for e in events] == ["user", "rpc", "user"]
    assert [e.revision for e in events] == [42, 43, 44]
    ev = events[0]
    assert isinstance(ev, DocEvent)
    assert ev.path == "/tmp/a.md"
    ch = ev.changes[0]
    assert ch.kind == "replace" and ch.text == "hi"
    assert ch.start == Position(10, 5) and ch.end == Position(10, 5)
    # kind-only 记录：start/end/text 全空
    undo_ch = events[2].changes[0]
    assert undo_ch.kind == "undo" and undo_ch.start is None and undo_ch.text is None


def test_get_state_empty_editor(server: MockEditor) -> None:
    """空态（无文档）：active=None，tabs 空。"""
    server.handlers["get_state"] = lambda req: {"tabs": [], "active_index": 0, "active": None}
    with Client.connect(server.path) as c:
        state = c.get_state()
    assert state.tabs == () and state.active is None


def test_context_manager_closes(server: MockEditor) -> None:
    server.handlers["get_state"] = lambda req: {"marker": 1}
    with Client.connect(server.path) as c:
        assert c.connected
    # 已关闭：后续调用应立刻失败，而不是挂死
    with pytest.raises(ConnectionLost):
        c.get_state()
    assert not c.connected


# ---------------------------------------------------------------------------
# AsyncClient
# ---------------------------------------------------------------------------


def test_async_roundtrip_and_context_manager(server: MockEditor) -> None:
    """异步客户端主路径：连接、命令、类型化结果、自动关闭。"""

    async def main() -> None:
        server.handlers["open_file"] = lambda req: {"tab_index": 0, "already_open": False}
        server.handlers["insert_text"] = lambda req: {
            "cursor": {"line": 0, "col": 5},
            "inserted_chars": 5,
            "line_count": 1,
            "char_count": 5,
        }
        server.handlers["get_state"] = lambda req: _STATE

        async with await AsyncClient.connect(server.path) as ac:
            r = await ac.open_file("/tmp/a.md")
            assert r.tab_index == 0 and r.already_open is False
            r2 = await ac.insert_text(0, 0, "hello")
            assert r2.inserted_chars == 5
            state = await ac.get_state()
            assert isinstance(state, State) and state.active is not None
            assert state.active is not None and state.active.char_count == 1337
        assert not ac.connected

    asyncio.run(main())


def test_async_error_wrapped(server: MockEditor) -> None:
    async def main() -> None:
        server.handlers["save"] = lambda req: (_ for _ in ()).throw(
            RpcReject("未命名文档无路径——先 save_as 指定目标")
        )
        async with await AsyncClient.connect(server.path) as ac:
            with pytest.raises(RpcError) as ei:
                await ac.save()
        assert ei.value.message == "未命名文档无路径——先 save_as 指定目标"

    asyncio.run(main())


def test_async_hostile_non_numeric_id_dropped_reader_survives(server: MockEditor) -> None:
    """P2-29（#282）：async reader 同修——敌意 id 不炸任务，后续调用照常。"""

    async def main() -> None:
        def hostile_then_normal(req: Mapping[str, Any]) -> dict[str, Any]:
            server.send_raw({"id": "evil", "ok": True, "result": {}})
            server.send_raw({"id": 1e999, "ok": True, "result": {}})
            return dict(_STATE)

        server.handlers["get_state"] = hostile_then_normal
        async with await AsyncClient.connect(server.path) as ac:
            st = await ac.get_state()
            assert st.active is not None and st.active.char_count == 1337
            st2 = await ac.get_state()  # reader 任务未炸——后续照常
            assert st2.active is not None and ac.connected

    asyncio.run(main())


def test_async_subscribe_stream(server: MockEditor) -> None:
    """异步事件流：async for 消费 + user/rpc 区分。"""

    async def main() -> None:
        server.handlers["subscribe"] = lambda req: {"subscribed": True}
        async with await AsyncClient.connect(server.path) as ac:
            stream = await ac.subscribe()
            server.wait_subscribed(1)
            server.push_event(_doc_changed(source="user", revision=1, revision_before=0))
            server.push_event(_doc_changed(source="rpc", revision=2, revision_before=1))
            got = []
            async for ev in stream:
                got.append(ev)
                if len(got) == 2:
                    break
        assert [e.source for e in got] == ["user", "rpc"]

    asyncio.run(main())


def test_async_reconnect_and_resend(server: MockEditor) -> None:
    """异步断连 → 自动重连重发。"""

    async def main() -> None:
        def handler(req: Mapping[str, Any]) -> dict[str, Any]:
            if server.connections <= 1:
                raise DropConnection
            return {"marker": "ok"}

        server.handlers["get_state"] = handler
        async with await AsyncClient.connect(server.path, reconnect_backoff=0.05) as ac:
            result = await ac.call("get_state")
        assert result == {"marker": "ok"}
        assert server.connections == 2

    asyncio.run(main())


def test_async_timeout(server: MockEditor) -> None:
    async def main() -> None:
        def slow(_req: Mapping[str, Any]) -> dict[str, Any]:
            time.sleep(0.8)
            return {}

        server.handlers["get_state"] = slow
        async with await AsyncClient.connect(server.path, timeout=0.2) as ac:
            with pytest.raises(RequestTimeout):
                await ac.get_state()

    asyncio.run(main())


def test_async_stream_interrupted(server: MockEditor) -> None:
    async def main() -> None:
        server.handlers["subscribe"] = lambda req: {"subscribed": True}
        async with await AsyncClient.connect(server.path, reconnect=False) as ac:
            stream = await ac.subscribe()
            server.wait_connections(1)
            server.drop_all()
            with pytest.raises((ConnectionLost, TransportError)):
                async for _ in stream:
                    pass

    asyncio.run(main())
