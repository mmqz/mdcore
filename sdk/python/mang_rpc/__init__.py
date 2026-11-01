"""mang_rpc —— mang-shell 编辑器 RPC 的 Python 客户端。

pip 安装后三行上手::

    from mang_rpc import Client

    with Client.connect() as c:
        c.open_file("/abs/doc.md")
        c.insert_text(0, 0, "hello")
        state = c.get_state()

协议（27 命令 + doc_changed 事件流）见
``mang-shell/docs/rpc-protocol.md``；本包类型定义与之逐字段对齐。
"""

from .client import AsyncClient, Client, default_socket_path
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
    BATCH_MAX_FILES,
    BATCH_MAX_HITS_PER_FILE,
    COMMANDS,
    LIST_DIR_MAX_ENTRIES,
    MAX_LINE_BYTES,
    READ_FILE_CAP_BYTES,
    SELECTION_TEXT_CAP,
    ActiveDoc,
    ApplyFormatResult,
    BatchFileResult,
    BatchReplaceResult,
    ChangeKind,
    CloseFileResult,
    CommandInfo,
    CreateDirResult,
    CreateFileResult,
    DeleteResult,
    DocChange,
    DocEvent,
    DocKind,
    EventSource,
    FormatKind,
    GetOutlineResult,
    GetSelectionResult,
    InsertTextResult,
    ListCommandsResult,
    ListDirResult,
    OpenFileResult,
    OutlineEntry,
    Point,
    Position,
    ReadFileResult,
    RedoResult,
    RenameResult,
    ReplaceSelectionResult,
    RunCommandResult,
    SaveAsResult,
    SaveResult,
    ScrollToResult,
    Selection,
    SetCursorResult,
    SetSelectionResult,
    State,
    SubscribeResult,
    SwitchTabResult,
    TabInfo,
    UndoResult,
    WriteFileResult,
)

__version__ = "0.1.0"

__all__ = [
    # 客户端
    "Client",
    "AsyncClient",
    "default_socket_path",
    # 协议常量
    "COMMANDS",
    "MAX_LINE_BYTES",
    "LIST_DIR_MAX_ENTRIES",
    "SELECTION_TEXT_CAP",
    "READ_FILE_CAP_BYTES",
    "BATCH_MAX_FILES",
    "BATCH_MAX_HITS_PER_FILE",
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
    # 命令结果
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
    # 异常
    "MangRpcError",
    "RpcError",
    "TransportError",
    "ConnectionLost",
    "RequestTimeout",
    "ProtocolViolation",
    "LineTooLong",
]
