"""mang-shell RPC 协议类型定义（27 命令 + doc_changed 事件）。

全部结构对照 ``mang-shell/src/rpc.rs``（``ControlCommand`` / ``state_json`` /
各 ``cmd_*`` 的 ``json!`` 载荷）逐字段核实，非凭文档示例编写。协议文档：
``mang-shell/docs/rpc-protocol.md``。

设计约定：

- 响应模型为 frozen dataclass，经 ``from_payload`` 从 JSON dict 宽容解析：
  未知字段忽略（前向兼容），可选字段缺席取 None。
- 坐标（line/col）0 基，col 是**字符**偏移不是字节——中文按 1 个字符算。
- 服务端上限常量在客户端侧同样可见（发送前自检用），值与服务端
  ``rpc.rs`` 常量一一对应，漂移会被 ``test_protocol_constants`` 钉住。
"""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from typing import Any, Literal

__all__ = [
    # 常量（与服务端 rpc.rs 对齐）
    "MAX_LINE_BYTES",
    "LIST_DIR_MAX_ENTRIES",
    "SELECTION_TEXT_CAP",
    "READ_FILE_CAP_BYTES",
    "BATCH_MAX_FILES",
    "BATCH_MAX_HITS_PER_FILE",
    "COMMANDS",
    # 字面量类型
    "Point",
    "FormatKind",
    "DocKind",
    "EventSource",
    "ChangeKind",
    # 公共子结构
    "Position",
    "Selection",
    "TabInfo",
    "ActiveDoc",
    "State",
    "DocChange",
    "DocEvent",
    # 各命令结果
    "OpenFileResult",
    "InsertTextResult",
    "ReplaceSelectionResult",
    "CloseFileResult",
    "SwitchTabResult",
    "SetCursorResult",
    "SetSelectionResult",
    "GetSelectionResult",
    "UndoResult",
    "RedoResult",
    "SaveResult",
    "SaveAsResult",
    "ScrollToResult",
    "SubscribeResult",
    "DirEntry",
    "ListDirResult",
    "ReadFileResult",
    "WriteFileResult",
    "CreateFileResult",
    "CreateDirResult",
    "DeleteResult",
    "RenameResult",
    "RunCommandResult",
    "CommandInfo",
    "ListCommandsResult",
    "OutlineEntry",
    "GetOutlineResult",
    "ApplyFormatResult",
    "BatchFileResult",
    "BatchReplaceResult",
]

# ---------------------------------------------------------------------------
# 常量（值 = 服务端 mang-shell/src/rpc.rs 同名常量）
# ---------------------------------------------------------------------------

#: 单行（请求）字节上限——超限服务端回错误并关闭连接，客户端发送前自检。
MAX_LINE_BYTES = 1024 * 1024

#: ``list_dir`` 条目上限——超限截断并置 ``truncated=true``。
LIST_DIR_MAX_ENTRIES = 5000

#: ``get_state`` / ``get_selection`` 选区文本带回上限（字符）。
SELECTION_TEXT_CAP = 2000

#: ``read_file`` 载荷上限（字节）。
READ_FILE_CAP_BYTES = 1024 * 1024

#: ``batch_replace`` 文件数上限。
BATCH_MAX_FILES = 200

#: ``batch_replace`` 单文件命中数上限——超限整文件拒绝（不清半截）。
BATCH_MAX_HITS_PER_FILE = 10_000

#: 27 命令名全集（与 ``ControlCommand::name()`` 一致）。
COMMANDS: frozenset[str] = frozenset(
    {
        # 文档生命周期（#104，6 条）
        "open_file",
        "insert_text",
        "replace_selection",
        "get_state",
        "close_file",
        "switch_tab",
        # 导航 / 历史 / 保存 / 订阅（#111，9 条）
        "set_cursor",
        "set_selection",
        "get_selection",
        "undo",
        "redo",
        "save",
        "save_as",
        "scroll_to",
        "subscribe",
        # 文件系统（#134 任务 1，7 条——沙箱内）
        "list_dir",
        "read_file",
        "write_file",
        "create_file",
        "create_dir",
        "delete",
        "rename",
        # 编辑器命令（#134 任务 2，4 条）
        "run_command",
        "list_commands",
        "get_outline",
        "apply_format",
        # 批量（#134 任务 3，1 条）
        "batch_replace",
    }
)

#: 点位（请求与响应同构：``{"line": n, "col": n}``）。
Point = Mapping[str, int] | Sequence[int]

#: ``apply_format`` 的 format 取值（``editor::FormatKind::id()``）。
FormatKind = Literal["bold", "italic", "code", "strikethrough"]

#: 文档形态（``Editor::doc_kind()``）。
DocKind = Literal["md", "docx", "pdf", "xlsx", "canvas"]

#: 事件来源——AI 区分「自身命令回显」与「人类编辑」的关键。
EventSource = Literal["user", "rpc"]

#: 变更记录类别。replace 的 start/end 是变更前被替换域（start == end 即纯插入）。
ChangeKind = Literal["replace", "undo", "redo"]


# ---------------------------------------------------------------------------
# 解析辅助
# ---------------------------------------------------------------------------


def _req(obj: Mapping[str, Any], key: str) -> Any:
    """必取字段（缺席让 KeyError 透出——已是协议违规场景）。"""
    return obj[key]


def _get(obj: Mapping[str, Any], key: str, default: Any = None) -> Any:
    v = obj.get(key, default)
    return default if v is None and default is not None else v


# ---------------------------------------------------------------------------
# 公共子结构
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Position:
    """行列点位（0 基；col 为字符偏移）。"""

    line: int
    col: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> Position:
        return cls(line=int(_req(obj, "line")), col=int(_req(obj, "col")))

    def to_payload(self) -> dict[str, int]:
        return {"line": self.line, "col": self.col}


@dataclass(frozen=True)
class Selection:
    """选区（含文本回带——截断防爆载荷）。

    ``text`` 上限 2000 字符（SELECTION_TEXT_CAP），超限截断且
    ``text_truncated=True``、``chars`` 保留真实总数。
    """

    start: Position
    end: Position
    chars: int
    text: str
    text_truncated: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> Selection:
        return cls(
            start=Position.from_payload(_req(obj, "start")),
            end=Position.from_payload(_req(obj, "end")),
            chars=int(_get(obj, "chars", 0)),
            text=str(_get(obj, "text", "")),
            text_truncated=bool(_get(obj, "text_truncated", False)),
        )


@dataclass(frozen=True)
class TabInfo:
    """标签清单条目（``get_state().tabs[i]``）。"""

    path: str | None
    modified: bool
    readonly: bool
    active: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> TabInfo:
        return cls(
            path=_get(obj, "path"),
            modified=bool(_get(obj, "modified", False)),
            readonly=bool(_get(obj, "readonly", False)),
            active=bool(_get(obj, "active", False)),
        )


@dataclass(frozen=True)
class ActiveDoc:
    """活动文档状态（``get_state().active``；空态为 None）。"""

    path: str | None
    name: str
    kind: DocKind | str
    readonly: bool
    modified: bool
    cursor: Position
    selection: Selection | None
    line_count: int
    char_count: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> ActiveDoc:
        sel = _get(obj, "selection")
        return cls(
            path=_get(obj, "path"),
            name=str(_get(obj, "name", "")),
            kind=str(_get(obj, "kind", "md")),
            readonly=bool(_get(obj, "readonly", False)),
            modified=bool(_get(obj, "modified", False)),
            cursor=Position.from_payload(_req(obj, "cursor")),
            selection=Selection.from_payload(sel) if sel is not None else None,
            line_count=int(_get(obj, "line_count", 0)),
            char_count=int(_get(obj, "char_count", 0)),
        )


@dataclass(frozen=True)
class State:
    """``get_state`` 结果——AI 的「看屏幕」主通道。"""

    tabs: tuple[TabInfo, ...]
    active_index: int
    active: ActiveDoc | None

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> State:
        return cls(
            tabs=tuple(TabInfo.from_payload(t) for t in _get(obj, "tabs", [])),
            active_index=int(_get(obj, "active_index", 0)),
            active=(ActiveDoc.from_payload(a) if (a := _get(obj, "active")) is not None else None),
        )


# ---------------------------------------------------------------------------
# 事件流（doc_changed）
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DocChange:
    """单条变更记录。

    ``kind="replace"``：``start``/``end`` 是**变更前**被替换域，
    ``start == end`` 即纯插入。
    ``kind="undo"/"redo"``：批量撤销重做给不出精确域，三项全 None
    （kind-only 粗粒度）——收到后请 ``get_state()`` 拉全量。
    """

    kind: ChangeKind | str
    start: Position | None = None
    end: Position | None = None
    text: str | None = None

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> DocChange:
        start = _get(obj, "start")
        end = _get(obj, "end")
        return cls(
            kind=str(_req(obj, "kind")),
            start=Position.from_payload(start) if start is not None else None,
            end=Position.from_payload(end) if end is not None else None,
            text=_get(obj, "text"),
        )


@dataclass(frozen=True)
class DocEvent:
    """``doc_changed`` 事件（subscribe 后服务端主动推）。

    - ``source == "user"``：人类在编辑；``"rpc"``：外部命令的回显。
    - 相邻事件 ``revision`` 连续 = 无丢失；跳号说明漏了事件，
      应当 ``get_state()`` 重同步。
    - ``path``：未命名文档为 None。
    """

    type: str
    path: str | None
    revision_before: int
    revision: int
    source: EventSource | str
    changes: tuple[DocChange, ...] = field(default=())

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> DocEvent:
        return cls(
            type=str(_req(obj, "type")),
            path=_get(obj, "path"),
            revision_before=int(_get(obj, "revision_before", 0)),
            revision=int(_get(obj, "revision", 0)),
            source=str(_get(obj, "source", "")),
            changes=tuple(DocChange.from_payload(c) for c in _get(obj, "changes", [])),
        )


# ---------------------------------------------------------------------------
# 命令结果（#104 文档生命周期）
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class OpenFileResult:
    """``open_file``——已打开的文件不重复开，激活既有标签。"""

    tab_index: int
    already_open: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> OpenFileResult:
        return cls(
            tab_index=int(_get(obj, "tab_index", 0)),
            already_open=bool(_get(obj, "already_open", False)),
        )


@dataclass(frozen=True)
class InsertTextResult:
    """``insert_text``——越界坐标钳制到文内最近点（不报错）。"""

    cursor: Position
    inserted_chars: int
    line_count: int
    char_count: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> InsertTextResult:
        return cls(
            cursor=Position.from_payload(_req(obj, "cursor")),
            inserted_chars=int(_get(obj, "inserted_chars", 0)),
            line_count=int(_get(obj, "line_count", 0)),
            char_count=int(_get(obj, "char_count", 0)),
        )


@dataclass(frozen=True)
class ReplaceSelectionResult:
    """``replace_selection``——无选区 = 光标处插入；空文本+有选区 = 删除。"""

    had_selection: bool
    cursor: Position
    char_count: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> ReplaceSelectionResult:
        return cls(
            had_selection=bool(_get(obj, "had_selection", False)),
            cursor=Position.from_payload(_req(obj, "cursor")),
            char_count=int(_get(obj, "char_count", 0)),
        )


@dataclass(frozen=True)
class CloseFileResult:
    """``close_file``——脏标签拒绝（先 save）。"""

    closed_index: int
    tabs_remaining: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> CloseFileResult:
        return cls(
            closed_index=int(_get(obj, "closed_index", 0)),
            tabs_remaining=int(_get(obj, "tabs_remaining", 0)),
        )


@dataclass(frozen=True)
class SwitchTabResult:
    """``switch_tab``——0 基索引。"""

    active_index: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> SwitchTabResult:
        return cls(active_index=int(_get(obj, "active_index", 0)))


# ---------------------------------------------------------------------------
# 命令结果（#111 导航 / 历史 / 保存 / 订阅）
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SetCursorResult:
    """``set_cursor``——越界钳制；只读文档可用。"""

    cursor: Position

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> SetCursorResult:
        return cls(cursor=Position.from_payload(_req(obj, "cursor")))


@dataclass(frozen=True)
class SetSelectionResult:
    """``set_selection``——服务端排序后回 anchor/cursor 两点。"""

    start: Position
    end: Position

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> SetSelectionResult:
        sel = _req(obj, "selection")
        return cls(
            start=Position.from_payload(_req(sel, "start")),
            end=Position.from_payload(_req(sel, "end")),
        )


@dataclass(frozen=True)
class GetSelectionResult:
    """``get_selection``——同 get_state 的 selection 字段（单独取，省流量）。"""

    path: str | None
    revision: int
    selection: Selection | None

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> GetSelectionResult:
        sel = _get(obj, "selection")
        return cls(
            path=_get(obj, "path"),
            revision=int(_get(obj, "revision", 0)),
            selection=Selection.from_payload(sel) if sel is not None else None,
        )


@dataclass(frozen=True)
class UndoResult:
    """``undo``——栈空不是错误（``undid=False``），别把 False 当失败。"""

    undid: bool
    revision: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> UndoResult:
        return cls(undid=bool(_get(obj, "undid", False)), revision=int(_get(obj, "revision", 0)))


@dataclass(frozen=True)
class RedoResult:
    """``redo``——同 undo 语义。"""

    redid: bool
    revision: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> RedoResult:
        return cls(redid=bool(_get(obj, "redid", False)), revision=int(_get(obj, "revision", 0)))


@dataclass(frozen=True)
class SaveResult:
    """``save``——未命名文档拒绝（先 save_as）；只读渲染态拒绝。"""

    saved: bool
    path: str
    modified: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> SaveResult:
        return cls(
            saved=bool(_get(obj, "saved", False)),
            path=str(_get(obj, "path", "")),
            modified=bool(_get(obj, "modified", False)),
        )


@dataclass(frozen=True)
class SaveAsResult:
    """``save_as``——标签身份迁移到新路径。"""

    saved: bool
    path: str
    tab_index: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> SaveAsResult:
        return cls(
            saved=bool(_get(obj, "saved", False)),
            path=str(_get(obj, "path", "")),
            tab_index=int(_get(obj, "tab_index", 0)),
        )


@dataclass(frozen=True)
class ScrollToResult:
    """``scroll_to``——只动视口不动光标。"""

    scrolled_to: int
    line_count: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> ScrollToResult:
        return cls(
            scrolled_to=int(_get(obj, "scrolled_to", 0)),
            line_count=int(_get(obj, "line_count", 0)),
        )


@dataclass(frozen=True)
class SubscribeResult:
    """``subscribe``——幂等（重复订阅替换旧通道）；断连自动注销。"""

    subscribed: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> SubscribeResult:
        return cls(subscribed=bool(_get(obj, "subscribed", False)))


# ---------------------------------------------------------------------------
# 命令结果（#134 任务 1 文件系统——全部受工作区沙箱约束）
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DirEntry:
    """``list_dir`` 条目。

    metadata 读失败时服务端只回 ``name``/``path``/``error``——
    ``is_dir``/``size``/``rel`` 可能缺席（None），解析要容错。
    """

    name: str
    path: str
    rel: str | None = None
    is_dir: bool | None = None
    size: int | None = None
    error: str | None = None

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> DirEntry:
        is_dir = _get(obj, "is_dir")
        size = _get(obj, "size")
        return cls(
            name=str(_req(obj, "name")),
            path=str(_req(obj, "path")),
            rel=_get(obj, "rel"),
            is_dir=bool(is_dir) if is_dir is not None else None,
            size=int(size) if size is not None else None,
            error=_get(obj, "error"),
        )


@dataclass(frozen=True)
class ListDirResult:
    """``list_dir``——条目上限 5000，超限 ``truncated=True``。"""

    path: str
    entries: tuple[DirEntry, ...]
    count: int
    truncated: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> ListDirResult:
        return cls(
            path=str(_get(obj, "path", "")),
            entries=tuple(DirEntry.from_payload(e) for e in _get(obj, "entries", [])),
            count=int(_get(obj, "count", 0)),
            truncated=bool(_get(obj, "truncated", False)),
        )


@dataclass(frozen=True)
class ReadFileResult:
    """``read_file``——不碰 UI 直接读盘；非 UTF-8 拒绝；上限 1 MiB。"""

    path: str
    size: int
    chars: int
    text: str
    truncated: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> ReadFileResult:
        return cls(
            path=str(_get(obj, "path", "")),
            size=int(_get(obj, "size", 0)),
            chars=int(_get(obj, "chars", 0)),
            text=str(_get(obj, "text", "")),
            truncated=bool(_get(obj, "truncated", False)),
        )


@dataclass(frozen=True)
class WriteFileResult:
    """``write_file``——磁盘权威；已打开干净标签自动 reload（``synced_open_tab``）。"""

    written: bool
    path: str
    bytes: int
    synced_open_tab: bool

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> WriteFileResult:
        return cls(
            written=bool(_get(obj, "written", False)),
            path=str(_get(obj, "path", "")),
            bytes=int(_get(obj, "bytes", 0)),
            synced_open_tab=bool(_get(obj, "synced_open_tab", False)),
        )


@dataclass(frozen=True)
class CreateFileResult:
    """``create_file``——已存在拒绝（覆盖走 write_file）。"""

    created: bool
    path: str
    bytes: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> CreateFileResult:
        return cls(
            created=bool(_get(obj, "created", False)),
            path=str(_get(obj, "path", "")),
            bytes=int(_get(obj, "bytes", 0)),
        )


@dataclass(frozen=True)
class CreateDirResult:
    """``create_dir``——单层；父目录必须存在；已存在拒绝。"""

    created: bool
    path: str

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> CreateDirResult:
        return cls(created=bool(_get(obj, "created", False)), path=str(_get(obj, "path", "")))


@dataclass(frozen=True)
class DeleteResult:
    """``delete``——目录递归删，**无回收站**；工作区根自身拒绝。"""

    deleted: bool
    path: str
    was_dir: bool
    closed_tabs: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> DeleteResult:
        return cls(
            deleted=bool(_get(obj, "deleted", False)),
            path=str(_get(obj, "path", "")),
            was_dir=bool(_get(obj, "was_dir", False)),
            closed_tabs=int(_get(obj, "closed_tabs", 0)),
        )


@dataclass(frozen=True)
class RenameResult:
    """``rename``——不覆盖（目标已存在拒绝）；已开标签身份自动迁移。"""

    renamed: bool
    src: str
    dst: str
    migrated_tabs: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> RenameResult:
        return cls(
            renamed=bool(_get(obj, "renamed", False)),
            src=str(_get(obj, "from", "")),
            dst=str(_get(obj, "to", "")),
            migrated_tabs=int(_get(obj, "migrated_tabs", 0)),
        )


# ---------------------------------------------------------------------------
# 命令结果（#134 任务 2 编辑器命令）
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class RunCommandResult:
    """``run_command``——与命令面板/键位同一分派路径。"""

    ran: bool
    command: str

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> RunCommandResult:
        return cls(ran=bool(_get(obj, "ran", False)), command=str(_get(obj, "command", "")))


@dataclass(frozen=True)
class CommandInfo:
    """``list_commands`` 条目——先调 list_commands 再 run_command，别硬编码命令名。"""

    id: str
    title: str
    default_key: str | None

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> CommandInfo:
        return cls(
            id=str(_req(obj, "id")),
            title=str(_get(obj, "title", "")),
            default_key=_get(obj, "default_key"),
        )


@dataclass(frozen=True)
class ListCommandsResult:
    """``list_commands``——截至 #134 共 18 条（随版本增减）。"""

    commands: tuple[CommandInfo, ...]
    count: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> ListCommandsResult:
        return cls(
            commands=tuple(CommandInfo.from_payload(c) for c in _get(obj, "commands", [])),
            count=int(_get(obj, "count", 0)),
        )


@dataclass(frozen=True)
class OutlineEntry:
    """大纲条目——平铺带 level，树结构由调用方自己组。"""

    level: int
    line: int
    text: str

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> OutlineEntry:
        return cls(
            level=int(_req(obj, "level")),
            line=int(_req(obj, "line")),
            text=str(_req(obj, "text")),
        )


@dataclass(frozen=True)
class GetOutlineResult:
    """``get_outline``——只对文本态 md 文档可用（渲染态拒绝）。"""

    entries: tuple[OutlineEntry, ...]
    count: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> GetOutlineResult:
        return cls(
            entries=tuple(OutlineEntry.from_payload(e) for e in _get(obj, "entries", [])),
            count=int(_get(obj, "count", 0)),
        )


@dataclass(frozen=True)
class ApplyFormatResult:
    """``apply_format``——md 行内标记对包裹（不是富文本属性）；整批一条 undo。"""

    applied: bool
    format: str
    cursor: Position
    chars: int

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> ApplyFormatResult:
        return cls(
            applied=bool(_get(obj, "applied", False)),
            format=str(_get(obj, "format", "")),
            cursor=Position.from_payload(_req(obj, "cursor")),
            chars=int(_get(obj, "chars", 0)),
        )


# ---------------------------------------------------------------------------
# 命令结果（#134 任务 3 批量）
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class BatchFileResult:
    """单文件结果。

    ``via``：``"buffer"``（已打开——一条 undo 撤该文件全批）/ ``"disk"``
    （直改盘——不可 undo）/ ``"skipped"``（出错拒绝，看 ``error``）。
    无命中时 ``note == "无命中"``。
    """

    path: str
    count: int
    via: str
    error: str | None = None
    note: str | None = None

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> BatchFileResult:
        return cls(
            path=str(_get(obj, "path", "")),
            count=int(_get(obj, "count", 0)),
            via=str(_get(obj, "via", "")),
            error=_get(obj, "error"),
            note=_get(obj, "note"),
        )


@dataclass(frozen=True)
class BatchReplaceResult:
    """``batch_replace``——想整批可撤销？先 open_file 全部文件再 batch。"""

    total_replacements: int
    files_changed: int
    files: tuple[BatchFileResult, ...]
    undo_hint: str

    @classmethod
    def from_payload(cls, obj: Mapping[str, Any]) -> BatchReplaceResult:
        return cls(
            total_replacements=int(_get(obj, "total_replacements", 0)),
            files_changed=int(_get(obj, "files_changed", 0)),
            files=tuple(BatchFileResult.from_payload(f) for f in _get(obj, "files", [])),
            undo_hint=str(_get(obj, "undo_hint", "")),
        )
