# CONTRACT.md — 宿主 ↔ 桥 ↔ 核 契约（冻结 v1）

> R23-d 冻结（2026-10-10）。目的：消除原型母本与产品仓（mdcore/）的契约漂移——
> 两侧对同一契约面的实现必须逐签名一致；破坏性变更必须升 `__contractVersion` 并
> 更新本文 + 双侧契约锁测试（`web/tests/contract_gate.py` / `crates/shell/tests/
> contract_freeze.rs`）。命名域：`mdcore` / `mdshell` / `mdc-*`（TRADEMARK_RULES §1）。

## §1 核（mdcore，Rust）——compile 冻结面

核的 pub API 即契约：签名变化 = 编译错（天然冻结）。语义锚点由语义锁测试背书：

| 面 | 契约 | 锁 |
|---|---|---|
| 事件流 | `apply_ops` 产生 `Event`（source: View/Undo/Redo/Ai/System），宿主 drain | r11/r12 |
| undo | `Core::undo/redo`，System 事务不入栈（ADR-B4） | r14/r21 |
| 渐进打开 | `extend_from_markdown`：悬挂 fence Reject 且树零污染 | document_progressive |
| 文件层 | `open_document/save_document` + `document_id` 重开复原 | r14/shell tests |
| 外部重载 | `FileStamp`/`plan_reload`/`adopt_external`（ADR-B7） | r23_external |
| 结构匹配 | `match_external_markdown`/`fingerprint_manifest`（ADR-B6） | r23_fingerprint |

宿主契约层（mdshell，Rust）签名（ADR-H1 宿主实现同一 trait）：

```rust
enum OpenProgress { FirstChunkInteractive { blocks }, ChunkAppended { chunk_index, total_chunks }, Completed { blocks } }
trait Host { fn surface_load_markdown(&mut self, markdown: &str) -> Result<(), String>; }
fn open_document(md: &str, meta_json: Option<&str>) -> Result<DocumentSession, String>;
fn open_document_progressive<H: Host>(chunks: &[String], meta_json: Option<&str>, host: &mut H, on_progress: impl FnMut(OpenProgress)) -> Result<DocumentSession, String>;
fn save_document(session: &DocumentSession) -> (String, String);   // (markdown, meta_json)
```

## §2 宿主壳（tao+wry+GTK3，产品仓 R22-d/e 实装；母本 = 蓝图）

- `mdcore://app` 自定义协议：服务 dist/ bundle（wry `with_custom_protocol`；URL 形状
  `mdcore://app/index.html`；静态资源只读，无网络面）。
- 事件名族：`mdc-inject` / `mdc-inject-chunk`（RC 轮统一 `md-*` → `mdc-*`；
  **禁** `md-inject*` 旧名——contract_freeze.rs 词面锁）。
- 注入方式：`webview.evaluate_script("window.__inject(<json-escaped md>)")`；
  大文档不宜走 URL 参数。
- 文件生命周期：打开（全量/渐进+OpenProgress 驱动进度条）→ 编辑 → 保存
  （Ctrl+S 拦截 → `__md()` + `save_document()` meta → md+meta 双文件写盘）→
  重开（`open_document(md, meta)` 身份复原）。
- 外部重载接线（R23-c）：watcher/聚焦 → `FileStamp` 比对 → stale 读盘 →
  `match_external_markdown` 预览 → `plan_reload` → Noop 忽略 / SilentAdopt
  adopt / OfferChoice 弹窗（keep-mine / take-external；文案注明 take 不可撤销）。

## §3 web 桥（window 面，冻结 v1）——contract_gate.py 锁

### 启动面（boot 后恒在）

`__ready: boolean`；`__core`（WasmCore）；`__view`（CM6 EditorView）；`__bridge`；
`__blocks(): BlockInfo[]`；`__blockCount(): number`；`__md(): string`；
`__memStats(): Record`；`__mdcoreEvents: CoreEvent[]`（新核事件流起点，重建清零）；
`__perf`（loadMs/typeMs/fallbackSyncMs/undoMs/progressive）；`__wasmMemory`；
`__heapTrace`；`__contractVersion: 1`。

### 宿主注入面（R23-d 冻结）

```ts
window.__inject(md: string): void
// → dispatch CustomEvent('mdc-inject', { detail: md })
// → 监听器：整体重建核（旧 EditorView.destroy() + 新 WasmCore/CM6View +
//   window 面全量重装；__mdcoreEvents 清零）

window.__inject_chunk(chunk: string, first: boolean): void
// → dispatch CustomEvent('mdc-inject-chunk', { detail: { chunk, first } })
// → first=true：重建核（渐进首段，TTI 时刻）；false：extend_from_markdown
//   追加 + pushRender 全量重渲染
```

实现侧允许优化渐进追加的渲染路径（分片建图等），**只要 window 面语义不变**。

### __bridge 面（R11-R17 沉淀）

`pushRender(anchor?, opts?)`；`locked: boolean`（ingest 窗口拒绝编辑）；
`undo(): boolean`——**键位契约**：恒 true 表示"按键已消费"（CM6 键位语义），
不是撤销是否发生；核侧 no-op 判据 = revision 不前进 + 无 source=undo 事件
（R21 勘误，G2 断言已按此口径）；`redo(): boolean` 同理；
`computeDecorations(): number`；`dumpDecorations()`；`setSelections(list)`；
`debugMapVsFullRebuild()`；`unit.utf16ToCp(s, utf16Pos?) / cpToUtf16(s, cpPos)`
（R17 单位宪法唯一换算出口）。

### 键位/快捷键面（宿主拦截 → 桥）

Ctrl+S（宿主存盘）、Ctrl+Z / Ctrl+Shift+Z（`__bridge.undo/redo`）——宿主
快捷键表变更须升契约版本。
