# mang-rpc

[mang-shell](../../mang-shell/) 编辑器 RPC 的官方 Python 客户端。
27 命令全覆盖 + `doc_changed` 事件流，同步 / 异步双客户端，类型注解完整（mypy strict）。

```bash
pip install mang-rpc
```

- **运行时零第三方依赖**——纯标准库（socket / json / dataclasses）。
- 协议参考：[`mang-shell/docs/rpc-protocol.md`](../../mang-shell/docs/rpc-protocol.md)。
- 本包类型定义与服务端 `mang-shell/src/rpc.rs` **逐字段对齐**，漂移由单测钉住。

---

## 快速开始

```python
from mang_rpc import Client

with Client.connect() as c:          # 端点自动解析（见下）
    c.open_file("/abs/doc.md")      # 打开文件（绝对路径）
    c.insert_text(0, 0, "hello")     # 行列 0 基，col 是字符偏移（不是字节）
    state = c.get_state()            # AI 的「看屏幕」主通道
    print(state.active.name, state.active.cursor, state.active.char_count)

    c.set_selection((0, 0), (0, 5)) # 选区两点任意次序；也接受 {"line":..,"col":..}
    c.replace_selection("world")    # 替换当前选区
    c.save()                         # 落盘
```

### 事件流——AI「看着」用户编辑

```python
with Client.connect() as c:
    c.open_file("/abs/doc.md")
    for event in c.subscribe():
        if event.source == "user":
            # 人类此刻正在改什么
            print(f"用户改了 {len(event.changes)} 处，revision {event.revision_before}→{event.revision}")
        # event.source == "rpc" = 自己刚发的命令的回显，忽略
```

- 相邻事件 `revision` 连续 = 无丢失；**跳号说明漏了事件，`get_state()` 重同步**。
- `changes[].kind == "replace"` 的 `start`/`end` 是变更前被替换域（`start == end` 即纯插入）。
- `kind == "undo"/"redo"` 且无坐标 = 批量撤销的粗粒度记录——收到后拉 `get_state()`。

### 跨文件批量替换

```python
with Client.connect() as c:
    # 想整批可撤销？先 open_file 全部文件再 batch（buffer 通道一条 undo 全撤）
    for f in ["src/a.rs", "src/b.rs"]:
        c.open_file(f"/abs/ws/{f}")
    r = c.batch_replace(["src/a.rs", "src/b.rs"], "foo", "bar")
    print(f"共 {r.total_replacements} 处，涉及 {r.files_changed} 个文件")
    for f in r.files:
        print(f"  {f.path}: {f.count} 处（via={f.via}）")
```

### 异步客户端

```python
import asyncio
from mang_rpc import AsyncClient

async def main():
    async with await AsyncClient.connect() as ac:
        await ac.open_file("/abs/doc.md")
        await ac.insert_text(0, 0, "hello")
        state = await ac.get_state()

        async for event in await ac.subscribe():
            if event.source == "user":
                print("用户在改：", event.changes)

asyncio.run(main())
```

---

## API 一览（27 命令）

方法名 = 协议命令名；全部返回类型化结果（frozen dataclass）。

| 组 | 方法 | 说明 |
|---|---|---|
| 文档生命周期 | `open_file(path)` / `insert_text(line, col, text)` / `replace_selection(text)` / `get_state()` / `close_file(path)` / `switch_tab(index)` | 打开、编辑、看状态；脏标签关闭会被拒（先 `save`） |
| 导航 / 历史 / 保存 | `set_cursor(line, col)` / `set_selection(start, end)` / `get_selection()` / `undo()` / `redo()` / `save()` / `save_as(path)` / `scroll_to(line)` / `subscribe()` | 越界钳制不报错；`undo` 栈空返回 `undid=False` 不是错误 |
| 文件系统 | `list_dir(path, recursive=False)` / `read_file(path)` / `write_file(path, content)` / `create_file(path, content=None)` / `create_dir(path)` / `delete(path)` / `rename(src, dst)` | 全部受**工作区沙箱**约束（无工作区一律拒绝）；`delete` 无回收站 |
| 编辑器命令 | `run_command(name)` / `list_commands()` / `get_outline()` / `apply_format(start, end, format)` | 先 `list_commands()` 再 `run_command`，别硬编码命令名 |
| 批量 | `batch_replace(files, pattern, replacement, is_regex=False)` | 双通道：已打开走 buffer（可 undo），未打开直改盘 |

原始逃生口：`c.call("future_command", **params)`——服务端新增命令、SDK 未跟上时可用。

点位参数（`start`/`end`）接受 `{"line": 0, "col": 5}` 或 `(0, 5)` 两种形态。

---

## 连接与端点

端点解析优先级（与服务端一致）：

1. `Client.connect("/explicit/sock")` 显式路径
2. `$MANG_RPC_SOCK` 环境变量（SDK 扩展——测试/多实例用）
3. `$XDG_RUNTIME_DIR/mang-shell.sock`
4. `/tmp/mang-shell-$UID.sock`

编辑器侧：默认开 RPC；`--rpc-sock PATH` 改端点；`--no-rpc` 关闭。
**单实例限制**：端点被占时新实例静默禁用 RPC（应用日志有「端点被占用」）。

## 错误处理

| 异常 | 场景 | 处理姿势 |
|---|---|---|
| `RpcError` | 服务端业务拒绝（`ok:false`），`.message` 保留中文原文 | **不要做字符串匹配分支**（文案会变）；判断状态用 `get_state()` 结构化字段 |
| `TransportError` | 连接拒绝 / socket 故障 | 编辑器没开？先启动再 `connect()` |
| `ConnectionLost` | 断线且重连未恢复 | 命令**可能已生效**（at-least-once）——恢复后先 `get_state()` 对齐 |
| `RequestTimeout` | 等响应超时 | 同上：超时 ≠ 失败，先对齐再决定重试 |
| `ProtocolViolation` | 响应行不是合法 JSON | 版本错配的信号 |
| `LineTooLong` | 请求行 > 1 MiB（客户端先挡） | 大内容走 `write_file` / `batch_replace`，或分块 `insert_text` |

```python
from mang_rpc import Client, RpcError, ConnectionLost

try:
    c.close_file("/abs/dirty.md")
except RpcError as e:
    print(f"编辑器拒绝：{e.message}")   # 人看的信息；程序分支请用 get_state()
except ConnectionLost:
    state = c.get_state()               # 重连恢复后先对齐现实
```

## 自动重连

```python
c = Client.connect(reconnect=True,            # 断线自动重连（默认开）
                   max_reconnect_attempts=5,  # 尝试次数
                   reconnect_backoff=0.25,    # 指数退避基数（秒）
                   retry_on_drop=True)        # 请求中途断线 → 重连后重发一次
```

- 重连成功后**自动重订阅**（协议幂等）。
- **at-least-once**：断线时命令可能已执行，重发可能双写。谨慎场景用
  `retry_on_drop=False`，断线直接抛 `ConnectionLost`，由调用方对齐后重试。
- 断线期间的事件会丢失——恢复后 `revision` 跳号即重同步信号。
- 重连耗尽：进行中的调用抛 `ConnectionLost`，事件迭代器同样终止。
- 事件迭代器语义：客户端 `close()` → 迭代器干净收尾；不可恢复断线 → 抛
  `ConnectionLost`。

## 线程安全性

`Client` 可在多线程并发使用：内部 reader 线程 + per-request 应答槽，响应按
`id` 匹配（协议不保证响应顺序）。`subscribe()` 迭代器建议单消费者。

## 类型

- 全部结果为 frozen dataclass；`mypy --strict` 通过（本仓库 CI 验证）。
- 坐标 0 基；`col` 是**字符**偏移（中文 1 字符 = 1 col）。
- 可空字段显式可选：`DirEntry.is_dir`（metadata 失败变体）、
  `DocChange.start/end/text`（批量撤销的 kind-only 记录）等。

## 开发

```bash
cd sdk/python
uv sync               # dev 工具（pytest/mypy/ruff/black）随 dependency-groups 默认装入
uv run pytest          # 单测（mock socket，不需要真编辑器）
uv run mypy mang_rpc tests
uv run ruff check .
uv run black --check .
```

测试用 `tests/conftest.py` 的 `MockEditor`——按服务端 `rpc.rs` 行为逐条模拟
的 unix socket 假编辑器（JSON Lines / id 回显 / 订阅推送 / 断连注入）。

## 验证状态

- ✅ 单测：mock socket 全绿（27 命令回环 / 错误 / 重连 / 事件流 / 并发 / 异步）。
- ⚠️ **真实编辑器连接验证：未验证**。本 SDK 开发环境（沙箱）没有可运行的
  mang-shell 二进制（Rust + GPUI 需完整编译），mock 按源码 `rpc.rs`
  逐字段实现，协议行为与真实服务端一致性由对照审核保证。首个真实环境
  请跑：`connect() → get_state() → open_file() → insert_text() → subscribe()`
  五步冒烟。

## 许可

GPL-3.0-or-later（与 mdcore 仓库一致）。
