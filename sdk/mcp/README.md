# mang-mcp

[mang-shell](../../mang-shell/) 编辑器的 **MCP server**（Model Context Protocol）。
让 Claude Desktop / ChatGPT 等 MCP 宿主直接操控编辑器——你说「帮我把当前打开的
文档里所有 TODO 改成 NOTE」，宿主调 `get_state` 看现状、`replace_text` 执行，
编辑器 UI 实时变化。

```text
MCP 宿主（Claude Desktop） ⇄ stdio JSON-RPC ⇄ mang-mcp ⇄ unix socket ⇄ mang-shell
                                              └── 转发层复用 mang_rpc（自动重连）
```

## 安装

```bash
pipx install mang-mcp     # 或：pip install mang-mcp
```

从仓库源码运行：

```bash
cd sdk/mcp
uv sync
uv run mang-mcp --help    # 源码方式（无需 pip 安装即可本地跑）
```

## Claude Desktop 配置

`claude_desktop_config.json`（macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`）：

```json
{
  "mcpServers": {
    "mang": {
      "command": "mang-mcp"
    }
  }
}
```

自定义 RPC 端点（如编辑器用 `--rpc-sock` 改了路径）：

```json
{
  "mcpServers": {
    "mang": {
      "command": "mang-mcp",
      "env": {
        "MANG_RPC_SOCK": "/custom/mang-shell.sock"
      }
    }
  }
}
```

配置后**重启 Claude Desktop**，对话框下方出现 `mang` 工具组即可用。

## 使用

对 Claude 说：

- 「看下编辑器现在开着什么」→ `get_state`（标签 + 光标 + 工作区内文档全文）
- 「把当前文档里所有 TODO 改成 NOTE」→ `replace_text`（buffer 通道，一条 Ctrl+Z 全撤）
- 「把第 10 行开头插入 `- [ ] 买菜`」→ `insert_text`
- 「把 src/ 下所有 .rs 里的 foo 改成 bar」→ `batch_replace`
- 「这份文档的结构是什么」→ `get_outline`
- 「折叠起来」→ `list_commands` + `run_command("toggle_fold")`

## 工具清单（11 个）

| 工具 | 对应 RPC | 说明 |
|---|---|---|
| `open_file(path)` | `open_file` | 打开/激活标签 |
| `close_file(path)` | `close_file` | 关标签（脏标签拒绝） |
| `get_state()` | `get_state` + `read_file` | 标签 + 光标 + **全文内容**（工作区内） |
| `insert_text(text, line?, col?)` | `insert_text` | 省坐标 = 当前光标处 |
| `replace_text(old, new, is_regex?)` | `batch_replace`（活动文档） | 「全部替换」；buffer 通道可 undo |
| `batch_replace(files, pattern, replacement, is_regex?)` | `batch_replace` | 跨文件批量 |
| `get_outline()` | `get_outline` | md 标题大纲 |
| `list_commands()` | `list_commands` | 编辑器命令清单（先看再 run） |
| `run_command(name)` | `run_command` | 与命令面板同一分派路径 |
| `read_file(path)` | `read_file` | 直读盘（1 MiB 上限，超限截断并标注） |
| `write_file(path, content)` | `write_file` | 直写盘（干净标签自动 reload 同步 UI） |

## 行为与边界

- **连接惰性建立**：首次工具调用时连编辑器。编辑器没跑 → 该次调用返回
  isError + 端点路径指引（宿主能向用户解释，而不是挂死）；编辑器上线后
  下次调用自动恢复，**无需重启宿主**。
- **断线自动重连**：编辑器重启后 `mang-mcp` 自动重连（mang_rpc 内建）。
- **错误原样透出**：服务端拒绝文案（中文）经 isError 直达宿主；SDK 不做
  字符串匹配分支（协议文档禁止）。
- **工作区沙箱**：`read_file` / `write_file` / `batch_replace` /
  `replace_text` / `open_file` / `save_as`（#275 起后两者同门）只在工作区
  根目录内生效（无工作区全拒）——协议级边界，详见
  [`mang-shell/docs/rpc-protocol.md`](../../mang-shell/docs/rpc-protocol.md)。
- **组合逃逸已闭合（#275）**：历史上 `open_file` 不受沙箱约束，敌意 LLM
  可组链越权——读链 `open_file(任意路径) → run_command("select_all") →
  get_state()` 泄工作区外内容；写链 `open_file(工作区外) → insert_text →
  run_command("save")` 越权改写。**把不可信 LLM 接入本 MCP 仍应保持
  警惕**：socket 无认证（同 UID 进程即全权），沙箱只界定了路径边界，
  不审查内容意图；workspace 内的破坏性操作（`delete` 无回收站、
  `batch_replace` disk 通道不可 undo）依然直达。
- **单实例**：编辑器一次只有一个实例持有 RPC 端点（端点被占时新实例
  静默禁用 RPC——「连不上」先看是否已有实例在跑）。

## 开发与测试

```bash
cd sdk/mcp
uv sync
uv run pytest           # 官方 mcp client（stdio）× mock RPC——不需要真编辑器
uv run mypy mang_mcp tests
uv run ruff check .
uv run black --check .
```

测试用 `tests/conftest.py::MiniMockEditor` 做 RPC 假编辑器、
mcp 官方 client 以 stdio 拉起本 server 子进程做端到端校验。

## 验证状态

- ✅ 单测：官方 MCP client（stdio 起真实子进程）× mock RPC——list_tools、
  逐工具转发参数断言、错误透出、编辑器未运行恢复路径全绿。
- ⚠️ **真实编辑器连接：未验证**（开发环境无 Rust 工具链/编辑器二进制）。
  首个真实环境冒烟：起编辑器 → Claude Desktop 配 `mang-mcp` → 说
  「看下编辑器现在开着什么」。

## 许可

GPL-3.0-or-later（与 mdcore 仓库一致）。
