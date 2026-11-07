# mang-shell RPC 协议参考

> 面向 **AI 调用方**与**外部客户端作者**。目标：读完这一篇就能写出能跑的命令。
> 单一真源是 `mang-shell/src/rpc.rs`（`ControlCommand` 枚举）——本文与之
> 不一致时**以源码为准**，并请提 PR 修本文。

## 目录

- [1. 这是什么 / 边界](#1-这是什么--边界)
- [2. 连接](#2-连接)
- [3. 报文格式](#3-报文格式)
- [4. 命令速查表（27 条）](#4-命令速查表27-条)
- [5. 命令详解](#5-命令详解)
- [6. 事件流（订阅）](#6-事件流订阅)
- [7. 沙箱与安全边界](#7-沙箱与安全边界)
- [8. 错误处理](#8-错误处理)
- [9. 推荐调用流程](#9-推荐调用流程)
- [10. 完整示例](#10-完整示例)
- [11. 已知边界与坑](#11-已知边界与坑)

---

## 1. 这是什么 / 边界

mang-shell 是编辑器进程。它开一个**本机 unix domain socket**，外部进程
（AI 代理、脚本、测试）通过 JSON Lines 下发命令、读取响应、订阅文档变更。

**明确不做的事**（#104/#111 定谳，不是待办）：

- 不走网络、不加密、不认证 —— 本机通信。任何能连上 socket 的进程都能
  完全控制编辑器。**不要把这个 socket 暴露到可路由的地方。**
- 不做多客户端串行化仲裁 —— 多客户端可同时连，命令在**单任务 dispatcher**
  串行执行（不会交错执行一半），但彼此不知道对方在干什么。
- 不提供"事务回滚" —— 每条命令独立生效。要撤销用 `undo`。

**能力边界**：27 条命令覆盖"编辑文本 / 管文件 / 取状态"。**不覆盖**图形、
样式主题、插件管理、设置界面。

---

## 2. 连接

### 端点

| 优先级 | 来源 | 值 |
|---|---|---|
| 1 | `--rpc-sock PATH` | 显式指定 |
| 2 | `$XDG_RUNTIME_DIR` | `$XDG_RUNTIME_DIR/mang-shell.sock` |
| 3 | 缺省 | `/tmp/mang-shell-$UID.sock` |

关闭：`--no-rpc`。

### 生命周期

- socket 在进程退出时删除（`Drop`）。
- 异常退出的残留：下次启动会**探活清理** —— 连得上说明有别的实例在监听
  （本实例 RPC 自动禁用，日志会打 `端点被占用`）；连不上说明是陈旧文件，
  删除后重绑。
- **一次只能有一个实例持有 RPC 端点。** 多开窗口时只有第一个拿到。

### 探活

连上即可发命令，没有握手/版本协商。可以用 `{"cmd":"get_state"}` 当 ping。

---

## 3. 报文格式

**传输**：unix socket 上的 **JSON Lines**（UTF-8，一行一条 JSON，`\n` 结尾）。

### 请求

```json
{"id": 7, "cmd": "set_cursor", "line": 10, "col": 5}
```

| 字段 | 必需 | 说明 |
|---|---|---|
| `cmd` | ✅ | 命令名（见速查表） |
| `id` | ❌ | 任意 u64。响应原样回显；省略时响应 `"id": null` |
| 其余 | 视命令 | 命令参数**平铺在顶层**（不是嵌套在 `params` 里） |

- **未知字段被忽略** —— 前向兼容，老服务端能接受新客户端的多余字段。
- **单行上限 1 MiB**（`MAX_LINE_BYTES`）。超限会回一条错误然后**关闭连接**。
- 空行被忽略。

### 响应

恒为一行 JSON，`result` / `error` **二选一**（未用的键不出现）：

```json
{"id": 7, "ok": true,  "result": {"undid": true, "revision": 42}}
{"id": 7, "ok": false, "error": "无活动文档（空态——先 open_file）"}
```

- `id` 可能为 `null`（请求没带 id，或 JSON 整体损坏导致 id 取不到）。
- 命令字段错但 JSON 合法时，`id` **仍会回显**——你能对上是哪条失败。
- 响应不保证顺序与请求一致（dispatcher 是队列，但**单队列串行**，实际
  按到达顺序执行；匹配响应请用 `id`，不要靠顺序）。

---

## 4. 命令速查表（27 条）

### 4.1 文档生命周期（#104 基础面，6 条）

| 命令 | 参数 | 一句话 |
|---|---|---|
| `open_file` | `path` | 打开文件；已开则激活既有标签 |
| `insert_text` | `line`, `col`, `text` | 在指定位置插入文本 |
| `replace_selection` | `text` | 替换当前选区 |
| `get_state` | — | 标签清单 + 活动文档状态 |
| `close_file` | `path` | 关闭标签（脏标签拒绝） |
| `switch_tab` | `index` | 切换标签（0 基） |

### 4.2 导航 / 历史 / 保存 / 订阅（#111，9 条）

| 命令 | 参数 | 一句话 |
|---|---|---|
| `set_cursor` | `line`, `col` | 设光标（只读文档也可用） |
| `set_selection` | `start`, `end` | 设选区（两点任意次序） |
| `get_selection` | — | 取选区 + 文本 |
| `undo` | — | 撤销一步 |
| `redo` | — | 重做一步 |
| `save` | — | 保存（#386 走 Shell 分流：pdf 批注增量落盘/编辑器三态；未命名拒） |
| `save_as` | `path` | 另存为 |
| `scroll_to` | `line` | 滚动视口（不动光标） |
| `subscribe` | — | 订阅 `doc_changed` 事件 |

### 4.3 文件系统（#134 任务 1，7 条 · 全部受沙箱约束）

| 命令 | 参数 | 一句话 |
|---|---|---|
| `list_dir` | `path`, `recursive?` | 列目录 |
| `read_file` | `path` | 读文件（不碰 UI） |
| `write_file` | `path`, `content` | 写文件（磁盘权威） |
| `create_file` | `path`, `content?` | 新建文件（已存在拒绝） |
| `create_dir` | `path` | 新建单层目录 |
| `delete` | `path` | 删除（文件/目录递归） |
| `rename` | `from`, `to` | 重命名/移动（不覆盖） |

### 4.4 编辑器命令（#134 任务 2，4 条）

| 命令 | 参数 | 一句话 |
|---|---|---|
| `run_command` | `name`, `args?` | 按机器名执行编辑器命令 |
| `list_commands` | — | 列出可用编辑器命令 |
| `get_outline` | — | 取 md 标题大纲 |
| `apply_format` | `range`, `format` | 应用行内格式 |

### 4.5 批量（#134 任务 3，1 条）

| 命令 | 参数 | 一句话 |
|---|---|---|
| `batch_replace` | `files`, `pattern`, `replacement`, `is_regex?` | 跨文件批量替换 |

---

## 5. 命令详解

> **坐标约定**：`line` / `col` 全部 **0 基**。`line` 是行号（0 = 第一行），
> `col` 是**字符**偏移（不是字节——中文按 1 个字符算）。

### 5.1 文档生命周期

#### `open_file`

```json
{"id":1,"cmd":"open_file","path":"/ws/a.md"}
```

`path`：绝对或相对（相对工作区根）。已打开的文件**不会重复开**，而是
激活既有标签。

**#275 起受沙箱门禁**：与 FS 命令同门（无工作区拒、根外拒——见
[第 7 节](#7-沙箱与安全边界)）。此前 open_file 可开任意绝对路径，
是组合逃逸链的源头（详见第 7 节「组合链闭合」）。

#### `insert_text`

```json
{"id":2,"cmd":"insert_text","line":3,"col":4,"text":"hello\n"}
```

越界坐标**钳制**到文内最近点（不报错）。`text` 里可以有换行。

#### `replace_selection`

```json
{"id":3,"cmd":"replace_selection","text":"X"}
```

无选区 = 在光标处插入；`text` 为空且**有**选区 = 删除选区。

#### `get_state`

```json
{"cmd":"get_state"}
```

返回结构（这是 AI 的"看屏幕"主通道）：

```json
{
  "tabs": [{"path":"/a.md","modified":false,"readonly":false,"active":true}],
  "active_index": 0,
  "active": {
    "path": "/a.md", "name": "a.md", "kind": "text",
    "readonly": false, "modified": false,
    "cursor": {"line":0,"col":5},
    "selection": {"start":{"line":0,"col":0},"end":{"line":0,"col":5},
                  "chars":5,"text":"hello","text_truncated":false},
    "line_count": 42, "char_count": 1337
  }
}
```

- 空态（没有打开任何文档）时 `active` 为 `null`。
- `selection` 无选区时为 `null`。
- `selection.text` 上限 **2000 字符**，超限截断并置 `text_truncated: true`。

#### `close_file` / `switch_tab`

`{"cmd":"close_file","path":"/abs/a.md"}` —— **脏标签拒绝**，先 `save`。
`{"cmd":"switch_tab","index":1}` —— 0 基索引。

### 5.2 导航 / 历史 / 保存

#### `set_cursor` / `set_selection` / `scroll_to`

```json
{"cmd":"set_cursor","line":10,"col":5}
{"cmd":"set_selection","start":{"line":0,"col":1},"end":{"line":0,"col":5}}
{"cmd":"scroll_to","line":42}
```

- 越界一律**钳制**，不报错。
- `set_selection` 两点**任意次序**（服务端排序后定 anchor/cursor）。
- 三者对**只读文档同样可用** —— 浏览/引用场景不需要可编辑权限。
- `scroll_to` 只动视口，**不动光标**。

#### `get_selection`

同 `get_state` 的 `selection` 字段（单独取，省流量）。无选区时 `selection` 为 `null`。

#### `undo` / `redo`

```json
{"cmd":"undo"}
→ {"id":1,"ok":true,"result":{"undid":true,"revision":41}}
```

- 与 `Ctrl+Z` / `Ctrl+Y` **同一条链**（多光标编辑合并成一步）。
- **栈空不是错误** —— 返回 `ok: true` 且 `undid: false`。别把 `false` 当失败。

#### `save` / `save_as`

```json
{"cmd":"save"}
→ {"id":1,"ok":true,"result":{"saved":true,"path":"/ws/a.md","modified":false}}

{"cmd":"save_as","path":"/ws/b.md"}
```

- `save` 对**未命名文档拒绝**（提示先 `save_as`）。#386 起与 UI
  （Ctrl+S）同源走 **Shell 保存分流**：pdf 标签的未落盘批注在此
  增量写盘（`"modified"` 同源含批注脏）；只读且脏（防御不可达）或
  写盘失败才拒绝。纯只读浏览文档（xlsx 等无改动）是干净 no-op 成功。
- `save_as` 会做标签身份迁移（path / display_name 更新）。
- **#275 起 `save_as` 目标受沙箱门禁**（根外拒、无工作区拒——与
  `open_file` 同批收口，见[第 7 节](#7-沙箱与安全边界)）。`save`
  本身不引入新路径：file_path 只能源于已门禁的 `open_file`/`save_as`
  或 GUI 对话框（用户意图）。

### 5.3 文件系统（沙箱内）

> **所有 FS 命令都要求有一个打开的工作区根目录**，否则一律拒绝。
> 详见 [第 7 节](#7-沙箱与安全边界)。

#### `list_dir`

```json
{"cmd":"list_dir","path":"src","recursive":true}
→ {"id":1,"ok":true,"result":{
     "path":"/ws/src",
     "entries":[{"name":"main.rs","path":"/ws/src/main.rs","rel":"src/main.rs",
                 "is_dir":false,"size":1234}],
     "count":1,"truncated":false}}
```

- 相对路径**相对工作区根**。
- `recursive` 缺省 `false`（只列一层）。
- 条目上限 **5000**，超限截断并置 `truncated: true`。
- 递归时读不了的子目录**静默跳过**（不炸整次列举）。
- 每个条目带 `rel`（相对工作区根的路径，用起来比 `path` 方便喂给后续命令）。
- 单个条目 metadata 读失败时该条目只有 `name`/`path`/`error`（`is_dir`/`size`
  缺席）——所以解析时要容错，别假设这三个键一定在。

#### `read_file` / `write_file`

```json
{"cmd":"read_file","path":"src/main.rs"}
{"cmd":"write_file","path":"src/lib.rs","content":"…"}
```

- `read_file` **不碰 UI**，直接读盘。上限 **1 MiB**，超限截断到字符边界
  并置 `truncated: true`；**非 UTF-8 拒绝**（不是"乱码返回"）。
- `write_file`：**磁盘是权威，写盘不动 UI**。但已打开的**干净文本标签会
  自动 reload 同步**——这条路径走完整编辑链，所以订阅端**照收**
  `doc_changed`，且可以 `Ctrl+Z` 撤回。
- 写**只读渲染态 / 脏标签**会被拒绝（引导先 `close_file` 或 `save`）。
- 父目录必须已存在（不隐式建目录）。

#### `create_file` / `create_dir`

- 已存在**拒绝**（不隐式覆盖 —— 覆盖走 `write_file`）。
- `create_dir` **只建单层**，父目录必须存在。
- `create_file` 的 `content` 可省 = 空文件。

#### `delete`

```json
{"cmd":"delete","path":"old.rs"}
```

- 文件 `remove_file`，目录**递归** `remove_dir_all` —— **没有回收站**。
- 工作区根自身**拒绝删除**。
- 路径下有**脏标签拒绝**；干净标签自动关闭。

#### `rename`

```json
{"cmd":"rename","from":"a.rs","to":"b.rs"}
```

- 两端都必须在沙箱内；`to` 已存在**拒绝**（不覆盖）。
- 已打开标签的身份**自动迁移**（未保存修改跟随新名）。

### 5.4 编辑器命令

#### `list_commands`

```json
{"cmd":"list_commands"}
→ {"id":1,"ok":true,"result":{"commands":[
     {"id":"goto_line","title":"转到行…","default_key":"Ctrl+G"}],"count":18}}
```

AI 应当**先调这个**再 `run_command` —— 不要硬编码命令名（会随版本变）。

当前共 **18** 条（`Cmd::ALL`，截至 #134）：

```
move_left  move_right  move_up  move_down  word_left  word_right
move_home  move_end    doc_start  doc_end  toggle_fold
open_find  open_replace  goto_line  save  select_all  undo  redo
```

这些是**编辑器自身**的命令（光标移动/查找/折叠/保存），与 RPC 层的
`save`/`undo`/`redo` 命令功能重叠但走的是命令面板同一条分派路径。
需要带参数的行为请直接用对应的 RPC 命令，不要绕 `run_command`。

#### `run_command`

```json
{"cmd":"run_command","name":"goto_line"}
```

- `name` 用 `list_commands` 给出的 `id`（机器名，不是面板标题）。
- 与命令面板 / 键位走**同一分派路径**。
- `args` 字段保留前向兼容，**当前全部命令都是零参**，传了也被忽略。

#### `get_outline`

```json
{"cmd":"get_outline"}
→ {"id":1,"ok":true,"result":{"entries":[
     {"level":1,"line":0,"text":"标题"}],"count":1}}
```

- 与语法高亮**同一判定源**，所以判定结果一致。
- 返回**平铺**条目（带 `level`），树结构由调用方自己组。
- **只对文本态 md 文档可用**；渲染态文档（如 docx）**拒绝**。

#### `apply_format`

```json
{"cmd":"apply_format","range":[{"line":0,"col":0},{"line":0,"col":4}],"format":"bold"}
```

- 在范围两端**插入/移除标记对**（如 `**…**`）—— 是 md 行内格式，
  **不是**富文本属性。
- `format` 取值：`bold` / `italic` / `code` / `strikethrough`。
- 两点任意次序。
- 整批**一条 undo**。
- `range` 也接受 `[[0,0],[0,1]]` 数组形式。

### 5.5 批量

#### `batch_replace`

```json
{"cmd":"batch_replace","files":["a.rs","b.rs"],
 "pattern":"foo","replacement":"bar","is_regex":false}
```

```json
{"id":1,"ok":true,"result":{
  "total_replacements": 3,
  "files_changed": 2,
  "files": [
    {"path":"/ws/a.rs","count":2,"via":"buffer"},
    {"path":"/ws/b.rs","count":1,"via":"disk"}
  ],
  "undo_hint": "已打开文件（via=buffer）一条 undo 撤该文件全批；磁盘直改（via=disk）不可 undo"
}}
```

- `is_regex: true` 时 `pattern` 是正则，`replacement` 支持 `$1` 捕获引用。
- **双通道**：已打开的文件走 `buffer`（可 undo、发事件）；未打开的直接改盘
  （`via: "disk"`，**不可 undo**）。
- 想整批可撤销？**先 `open_file` 全部文件再 batch**。
- 每文件命中数有上限，超限**整文件拒绝**（不清半截）。
- 非 UTF-8 文件跳过并报错。

---

## 6. 事件流（订阅）

`subscribe` 之后，服务端在**每次文档变更**时主动推事件。响应与事件走
**同一条出口**，所以天然保序。

```json
{"type":"doc_changed","path":"/tmp/a.md",
 "revision_before":41,"revision":42,"source":"user",
 "changes":[{"kind":"replace","start":{"line":10,"col":5},
             "end":{"line":10,"col":5},"text":"hi"}]}
```

### 字段

| 字段 | 说明 |
|---|---|
| `type` | 恒 `"doc_changed"`（留作未来多路分发） |
| `path` | 文档路径；**未命名文档为 `null`** |
| `revision_before` | 变更前版本号 |
| `revision` | 变更后版本号 |
| `source` | `"user"` / `"rpc"` |
| `changes` | 变更明细，≥1 条 |

### `source` 是关键

- `"user"` = 人类在打字
- `"rpc"` = 外部命令改的

**AI 用它区分"我自己刚发的命令的回显"和"人类此刻正在改什么"** —— 否则会把
自己的编辑当成用户意图。

### `changes[].kind`

| 值 | 含义 |
|---|---|
| `replace` | 常规替换/插入/删除。`start`/`end` 是**变更前被替换域**；`start == end` = 纯插入 |
| `undo` / `redo` | 撤销/重做 |

**批量撤销/重做给不出精确域**（字节坐标在撤销后错位）—— 这类记录只有
`kind`，`start`/`end`/`text` 全部省略。收到后请 `get_state` 拉全量。

### 版本号用途

相邻事件 `revision` 连续 = 无丢失。发现跳号说明漏了事件，应当 `get_state` 重同步。

### 订阅语义

- **幂等** —— 重复 `subscribe` 替换旧通道。
- **断连自动注销**（事件泵写失败即移除），不用手动退订。

---

## 7. 沙箱与安全边界

### 规则

**全部 7 条 FS 命令 + `batch_replace` + `open_file` + `save_as` 只允许在
工作区根目录内操作。**

- **没有打开工作区 = 一律拒绝。** 设计取舍：「无边界」比「错误的边界」更
  危险，所以宁可全拒。
- 相对路径相对工作区根；绝对路径也必须落在根内。

### 怎么挡住逃逸

`resolve_sandboxed` 做 `canonicalize` 后**组件级**前缀检查：

- `..` 穿越 —— 拒绝
- 符号链接指向根外 —— 拒绝（canonicalize 会解析掉链接再比）
- 目标不存在时，canonicalize **最近存在的祖先**再拼回尾段 —— 所以新建
  文件也逃不出去

### 组合链闭合（#275）

`open_file`/`save_as` 在 #275 之前是 #134 沙箱的两处旁路
（ControlCommand 族里带路径却不走 `resolve_sandboxed` 的命令），
可组成两条越权链（经 sdk/mcp 接入敌意 LLM 即可达）：

- **读链（S-1）**：`open_file(任意绝对路径)` → `run_command("select_all")`
  → `get_state().selection.text`（≤2000 字符/次）——泄工作区外文件内容；
- **写链（S-2）**：`open_file(工作区外.md)` → `insert_text` →
  `run_command("save")`（fs::write 回原路径）——越权原地改写。

两链源头同根（open_file 无门禁），收口后同批闭合：`save` 的 file_path
源头只剩「已门禁的 open_file/save_as」与「GUI 对话框（用户意图）」。

### 附带影响

- `delete` 递归删目录，**无回收站**。AI 有责任直接删，这是 #134 给的取舍。
- 删除/覆盖前若路径下有**未保存（脏）标签**，命令拒绝 —— 防止误删用户
  正在编辑的东西。

---

## 8. 错误处理

错误 = `ok: false` + `error` 是**人可读中文字符串**（不是错误码）。

```json
{"id":5,"ok":false,"error":"文档有未保存修改——先保存再关闭"}
```

**不要用字符串匹配来做分支**（文案会变）。要判断状态请用 `get_state` 的
结构化字段。错误消息是给人看的，不是给程序解析的。

常见错误类别：

| 场景 | 典型消息 |
|---|---|
| 空态（没开文档） | `无活动文档（空态——先 open_file）` |
| 无工作区 | `无打开的工作区——FS 命令限定工作区根目录内` |
| 越界路径 | `路径越出工作区边界…——FS 命令只允许工作区内操作` |
| 脏标签冲突 | `文档有未保存修改——先保存再关闭` |
| 只读态拒存（防御不可达——只读且脏） | `只读文档（渲染态）拒绝保存` |
| 载荷超限 | `单行超过 1048576 字节上限——连接关闭` |
| JSON 损坏 | serde 的错误字符串，`id` 为 null |

---

## 9. 推荐调用流程

### 最小闭环

```
subscribe          → 开始收事件
get_state          → 看当前有什么
open_file          → 打开目标
set_selection      → 圈定范围
replace_selection  → 改
save               → 落盘
get_state          → 确认
```

### 批量改多个文件

```
open_file × N          ← 关键：先全打开，才能整批 undo
batch_replace(files=[…])
save × N
```

### 让 AI"看着"用户改

```
subscribe
… 收到 doc_changed，source=="user" → 用户改了，按需响应
… 收到 doc_changed，source=="rpc"  → 自己的回显，忽略
```

---

## 10. 完整示例

### Python

```python
import json, socket, os

sock_path = os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/tmp"),
                         f"mang-shell-{os.getuid()}.sock")
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock_path)
f = s.makefile("rwb")

def call(**kw):
    f.write((json.dumps(kw) + "\n").encode())
    f.flush()
    return json.loads(f.readline())

print(call(id=1, cmd="get_state"))
print(call(id=2, cmd="open_file", path="/tmp/a.md"))
print(call(id=3, cmd="set_selection",
           start={"line":0,"col":0}, end={"line":0,"col":5}))
print(call(id=4, cmd="replace_selection", text="hello"))
print(call(id=5, cmd="save"))
```

### netcat / socat（手工调试）

```bash
echo '{"cmd":"get_state"}' | socat - UNIX-CONNECT:/tmp/mang-shell-1000.sock
```

### 事件订阅

```python
s.sendall(b'{"id":1,"cmd":"subscribe"}\n')
while True:
    ev = json.loads(f.readline())
    if ev.get("type") == "doc_changed":
        print(ev["source"], ev["revision"], ev["changes"])
```

---

## 11. 已知边界与坑

按"踩到会浪费半天"排序：

1. **命令参数平铺在顶层**，没有 `params` 包装。照抄嵌套写法会得到
   `unknown variant` / 缺字段错误。

2. **`col` 是字符偏移不是字节。** 中文文档里按字节算坐标会错位。

3. **`undo` 栈空返回 `ok:true, undid:false`。** 不是错误。

4. **`write_file` 写盘不动 UI**，但干净标签会自动 reload —— 所以订阅端
   会收到事件。写脏标签直接拒绝。

5. **`batch_replace` 的 `via:"disk"` 不可 undo。** 要可撤销就先 `open_file`。

6. **没有工作区 = 所有 FS 命令全拒。** 这不是 bug，是刻意的"无边界即拒绝"。

7. **错误消息是中文字符串，不是错误码。** 别做字符串匹配分支。

8. **单实例限制。** 端点被占时新实例静默禁用 RPC（日志有 `端点被占用`）。
   调试"连不上"先看是不是已经有一个实例在跑。

9. **单行 1 MiB 上限**，超了连接直接关。大文件请用 `read_file` 分块取，
   别想一次 `insert_text` 塞进去。

10. **`run_command` 的命令名会变。** 先 `list_commands` 再调用。

---

## 附：与源码的对应关系

| 本文内容 | 源码位置 |
|---|---|
| 命令全集 + 参数 | `src/rpc.rs` `enum ControlCommand` |
| 命令名映射 | `ControlCommand::name()` |
| 响应结构 | `src/rpc.rs` `struct ControlResponse` |
| `get_state` 返回 | `src/rpc.rs` `state_json()` |
| 事件结构 | `src/doc_events.rs` `struct DocEvent` |
| 沙箱 | `src/rpc.rs` `resolve_sandboxed()` |
| 上限常量 | `MAX_LINE_BYTES` / `LIST_DIR_MAX_ENTRIES` / `READ_FILE_CAP_BYTES` / `SELECTION_TEXT_CAP` |
| 端到端断言 | `tests/rpc_client.py` |
