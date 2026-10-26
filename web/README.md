# web/ — CM6 View ↔ mdcore-wasm 桥（主档 R9）

> 状态：✅ W1-W11 冒烟 11/11 绿（Playwright chromium headless，2026-10-08；
> W7 = R10 渐进打开，1MB TTI 153ms vs 全量 361ms；W11 = R12 Human+AI 闭环）。
> 单一真理在 Rust 核（主档 §9）；CM6 doc = `core.to_markdown()` 的镜像。
> 本目录产物（`dist/`）即宿主壳（tao+wry+GTK3，ADR-H1）的加载物。

## 1. 架构

```text
┌─ WebView (chromium / wry) ──────────────────────────────┐
│  CM6 EditorView                                          │
│    ├─ decoField (StateField<{deco, atomic}>)              │
│    │    ├─ EditorView.decorations.from(...)  ← DOM 渲染  │
│    │    └─ EditorView.atomicRanges.from(...) ← 光标原子跳 │
│    │      （R15：纯 selection 事务增量窗口化重算——新旧选区覆盖行    │
│    │       ±1 真并集，等价性护栏 W12；atomic 集成对维护 O(1) 取用） │
│    └─ updateListener ──┐                                 │
│                        ▼                                 │
│              bridge (src/main.ts)                        │
│    快路径: 单 change + 块体内 + 行数保持                  │
│            → ReplaceText op → apply_ops                  │
│    兜底:   其余一切 → sync_from_markdown(doc)            │
│                        │ JSON 边界                       │
└────────────────────────┼────────────────────────────────┘
                         ▼
            WasmCore (crates/mdcore-wasm, Rust)
  apply_ops / sync_from_markdown / undo / redo / take_events /
  blocks_json / to_markdown / set_clock_ms
```

- **op 形状** = `crates/mdcore/src/op.rs` serde（`tag="t"` snake_case）；偏移
  单位 char（ropey），CM6 侧 UTF-16 offset 经 code-point 计数换算。
- **undo/redo 核驱动**，CM6 自带 history 刻意不装；undo 后 doc 回写 =
  全量重渲染（`pushRender`），取舍：简单可靠，代价是 view 独有状态
  （核无法表示的末尾空行）被吸收、选区仅钳制保活。
- **事件出口**：每次核变异后 `take_events()` 累积到
  `window.__mdcoreEvents`——冒烟测试与未来 AI 侧共用的 op 流证据
  （主档 §22 不变量 1/10：View 与 AI 消费同一条流）。
- **时钟注入**：wasm32 无 SystemTime，每次变异前
  `set_clock_ms(Date.now())`（1s 合并窗依赖）。

## 2. 构建与运行

```bash
./build-wasm.sh            # 步骤一：cargo build (wasm32) + wasm-bindgen → pkg/
npm install                # 首次
npm run build              # 步骤二：esbuild → dist/（自包含：main.js + index.html + .wasm）
npm run serve              # 静态服务 dist/（或 python3 -m http.server -d dist）
```

**PDF 模型面（pkg_pdf——issue #5）**：PDF 表面的标注写回/页内文本
注入走 `mdc-pdf` 独立 wasm 包（`web/pkg_pdf/`）。**默认 `npm run
build` 不重编译 wasm**（前端构建 ~30s 语义）；`pkg_pdf` 缺席时
pdf_view 降级 display-only（`__pdfState().wasm = false`，
`pdf_view.py` 会话用例走 SKIP 门）。启用模型面：

```bash
npm run build:wasm:pdf      # 仅构建 PDF 模型面包（build-pdf-wasm.sh → pkg_pdf/）
npm run build:full          # 一键：主 wasm + PDF wasm + 前端
```

依赖：`cargo`（rust-toolchain.toml 钉 1.99.0 + wasm32 target）+
`wasm-bindgen-cli 0.2.129`（与 Cargo.lock 精确一致——版本漂移 =
wire 面破坏）。

页面 query 参数：`?doc=big&kb=1024` 加载 1MB 生成文档（W4/压测入口）；
`&chunks=K` 启用 R10 渐进打开（首段 TTI 后追加余段，ingest 窗口
`locked` 拒绝编辑）。
`window.__core / __view / __blocks() / __md() / __mdcoreEvents / __perf`
为调试与测试探针。

## 3. 测试

```bash
python3 tests/web_smoke.py    # 起 dist 静态服务 → chromium headless → W1-W11
python3 tests/ime_cdp.py      # R23-a：CDP 真实 IME 组合流（Input.imeSetComposition）→ results_ime.json
```

| # | 实验 | 断言面 |
|---|------|--------|
| W1 | 语法隐藏渲染 | DOM 类级：h1/bold/code/hr widget/list/quote 标记隐藏 + 严格重叠显隐往返 |
| W2 | 光标协商 | 标记原子跳（ArrowLeft/Right 越过 marker）；快路径编辑 + 核 undo 重渲染后选区存活 |
| W3 | IME（synthetic） | composition 三事件流后 view doc 与核树一致；真 IME 留 R11 |
| W4 | 大文档 1MB | 加载 + 10 次快路径打字 p95 + 兜底 sync + undo 重渲染计时（results.json） |
| W5 | 块操作 | Enter 分段 / Backspace 行首合并 / 列表核驱动续行（marker 继承）+ 新 item 打字进核 |
| W6 | undo 集成 | 合并窗打字一次全撤；结构 split undo 后 ID 恢复；redo 重新分段 |
| W7 | 渐进打开（R10） | `?chunks=8`：首段 TTI < 全量加载（同机相对断言）；终态 = 全文档；ingest 后快路径可编辑 |
| W11 | Human+AI 闭环（R12） | AI 经 JSON 边界 `apply_ops source="ai"` 注入 op 流：CM6 渲染 AI 改动且 Human 编辑共存；view→ai→view 同一事件流（AI op id 物化）；revision 严格递增（AI 不进合并窗、AI 后击键独立条目）；undo 先撤人手再整撤 AI 批 |
| G1 | 硬 Gate 2 量化（R14） | 三路拆分：selection-only 光标路径 p95（判 §15.3 <16ms）+ typing 全路径 p95 + WasmCore 旁路核内份额；另记 1MB 终态 JS 堆。本轮实测：光标 23.9ms GAP（归因=装饰随光标全量重建，见 gates.md）；核内仅 8.7ms |
| G2 | 10MB 量化（R14 §28 P0-3） | `kb=10240&chunks=16`：渐进 TTI 362.7ms / 完整 ingest 11.85s / undo 2268ms / 终态 200576 块；JS 堆 803MB（非门预算 600MB 超出 → FLAG）。判定与 1MB 对照见 gates.md |

结果证据：`tests/results/results.json`（入库）。G 前缀 = R14 硬 Gate 收口轮
（回归 checks 与门判定分离：checks 绿、预算判定在 gates.md 与各 G 项
gate_verdict 字段）。

## 4. R9 审查战役记录（三处真实缺陷）

1. **装饰未入 DOM**：StateField 装饰缺 `EditorView.decorations.from(...)` 接线。
   spike/b 的 S1 只断言了 JS 侧 RangeSet（`view.decorations.between`），
   DOM 通路从未验证——潜伏两代才暴露。教训：断言要打到最终渲染面。
2. **redo 键位失配**：`w3c-keyname` 对 Shift+字母保留 `event.key` 大小写，
   真实浏览器 Ctrl+Shift+z → 先查 `Ctrl-Z`（未绑）→ 回落 `Ctrl-Shift-z` ✓；
   但 Playwright 合成事件 key 保持小写 `'z'` + shiftKey → 首查命中
   `Ctrl-z` 的 undo 绑定 → redo 永不执行。修复 = 双写绑定
   （`Mod-Shift-z` + `Mod-Shift-Z`）+ 测试用大写 `Z` 规格（与真浏览器一致）。
3. **快路径选区映射**：裸 dispatch 在插入点不前移光标（CM6 默认 assoc），
   测试需带显式 selection 模拟真实打字，断言的才是用户看到的行为。

## 5. R10 对接点（wry 宿主壳）

- 宿主加载物 = `dist/`（自包含，`file://` 或自定义协议均可，wasm 走
  `import.meta.url` 相对解析）。
- 宿主最小面：窗口/标题/文件对话框；**大文档注入改走
  `window.__inject(md)`（待实现，见 crates/shell-native 蓝图对接点 2），
  替代 URL 参数**；渐进打开宿主侧契约 = `crates/shell` 的
  `open_document_progressive` + `OpenProgress` 事件（进度条输入）。
- 渐进打开实测（W7）：1MB 8 段 TTI 153.2ms vs 全量 360.7ms；ingest
  690ms 含每段全量重渲染（O(K·n)，v0 登记）；终态块数与一次性一致。

## 6. R11 验证轮（W8-W10）与真 IME 人工验收协议

新增三实验（结果同入 results.json）：

| # | 实验 | 要点与实测 |
|---|---|---|
| W8 | 长事务桥压测 | 1MB：200-op 批量事务 14.9ms / 重渲染 111.8ms / 幂等 sync 零差分（关键收敛断言）/ 批量 undo 127.3ms / CM6 三 change 单事务兜底 127.2ms |
| W9 | 深合成 IME | 三轮连续 composition（含 compositionupdate）+ 取消路径零残留；**CM6 异步吸收 → 断言须 await tick，同步检查假阴性** |
| W10 | 大文档块操作 | 20448 块：核驱动 split 156.1ms / fallback merge 275.2ms / 结构 undo×2 322.7ms（ID 逐一还原） |

**真 IME 人工验收协议**（合成事件无法覆盖的 OS 级路径，逐项打勾）：

1. macOS 拼音/日文 ROMAJI + Windows 微软拼音 + Linux iBus 各过一遍；
2. 候选词翻页选词期间：预编辑串下划线区不被装饰撕裂、不移位；
3. 组词中按 Esc 取消：无残留、光标回原位；
4. 组词中按方向键 / 点击别处：composition 强制提交，无重复文本；
5. 在 code fence / 表格 / quote 前缀内开组词：fallback sync 后块身份与
   光标存活；
6. 长句连打（>30s 不间断）：undo 一次回滚到组词起点（合并窗语义）。

桥/核侧对应保障：composition 全程走兜底 sync（核内差分幂等）、
compositionend 后无额外 dispatch 需求（该 100ms 级修复针对
CM5 legacy，本桥未复现该缺陷）。

## 7. R12（W11）：Human + AI 闭环浏览器半边

`w11_ai_loop`：宿主侧把 AI 放进同一条链路——① CM6 快路径人手打字；
② AI 用 `blocks_json()` 读状态 → 确定性 `insert_block` op（文本由读到的
首个块内容决定）→ `apply_ops(ops, "ai", "ai.note")` 经 JSON 边界提交；
③ 桥 `pushRender` 后 CM6 渲染出 AI 改动（文首 AI note，Human 尾缀保留）；
④ 人手再打字 → `__mdcoreEvents` 里 view→ai→view 同流、AI op id 已物化；
⑤ 两次 `__bridge.undo()`：先撤末尾人手击键、再整撤 AI 单条目。
语义完全由核保证（Rust 侧 r12_ai_loop 10 条）；浏览器侧只验渲染与
事件流接线。AI 提交带 base gate 的 `commit_transaction_as` 未透传到
wasm 绑定（见 prototype README §9 简化项）。

## 8. TR-UI 产品化战役（issues #3 深化 + #9/#10 闭环，dev-r1..r20）

### 8.1 issues 闭环（#9 / #10）

- **#9（TR-1）行尾镜像**：`markdown_to_tree` 入口 CRLF/孤立 CR 统一
  规范化 LF（模型 = 权威层；三解析入口漏斗单点覆盖）——`__md()`
  保存内容 = CM6 所见。回归锁 `tr1_line_endings.rs` 六件 + web
  `tr1_web_e2e.py` 两件。
- **#10（TR-4）导出格式**：`WasmCore::export_docx_styled(options_json)`
  加性面（镜像 office-io snake_case API；形状面 `docx_options` /
  值域面 V9 漏斗 `docx_invalid_input` 分层校验）+ ⚙ 导出选项面板
  （4 预设：默认/公文 A4[四号·1.5 倍·90pt 边距 = G2 LO 实测口径]/
  紧凑/大字 + 自定义三选）+ 运行时鸭子探测（旧 pkg 回退兼容）。

### 8.2 格式工具条（程序化文本编辑模式）

工具条 **不做独立 op 面**——按钮 = `view.dispatch` 文本 changes，
语义由既有双层管道承载（body 快路径 / 前缀区 fullResync 核内差分）；
独立 op 轨被否决（模型 marks 不写入 MD 投影 = 视图零反馈）。
**forceFullSync 标志**：行首前缀变更强制全量同步（快路径会把
`# ` 吸收为字面文本 = 块类型漂移，e2e 实证）。11 键 + 8 快捷键
（Mod-b/i/e/Shift-x/Shift-1/2/3/0）。

### 8.3 状态栏 / Ctrl+S / 自动保存提醒

5 槽位（字符/词数混合计数/块数/光标 Ln,Col/rev + dirty 指示）；
Ctrl+S = `mdc-save` 事件（宿主 CONTRACT 桥路径 = `__md()` +
`save_document`）；dirty 超 5s → `mdc-autosave-hint` 提醒事件
（不自动写核——保存动作恒经宿主桥/用户）。

### 8.4 TOC 大纲 / 查找替换 / 帮助 / 字号 / 打印

TOC（fixed 右侧零布局侵入 + 层级缩进 + 文本定位跳转）；查找替换
（零新增依赖面——手写 indexOf 收集 + 单事务倒序替换 = undo 一次
撤全）；帮助面板（F1，12 行速查表）；字号 Ctrl+=/-/0（clamp
11-28 + localStorage）；`@media print`（chrome 全隐只留正文）。

### 8.5 测试面

`editor_toolbar.py`（26 checks）+ `hostile_ui.py`（8 checks：
空文档/200KB 大文档/emoji 代理对/BIDI/面板叠加/连击 20 次/
localStorage 污染/行号+自动保存）+ `tr1_web_e2e.py`（2）。
web 全量 15→18 套件 **239 checks** 全绿。
