# CONTRACT.md — 宿主 ↔ 桥 ↔ 核 契约（冻结 v1.1）

> R23-d 冻结 v1（2026-10-10）；R24-a minor 升版 v1.1（2026-10-10，纯增量：
> window 面增设置读写项，v1 面零破坏）。目的：消除原型母本与产品仓（mdcore/）
> 的契约漂移——两侧对同一契约面的实现必须逐签名一致；破坏性变更必须升
> `__contractVersion` 并更新本文 + 双侧契约锁测试（`web/tests/contract_gate.py` /
> `crates/shell/tests/contract_freeze.rs`）。命名域：`mdcore` / `mdshell` / `mdc-*`
>（TRADEMARK_RULES §1）。

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
| 脏标记 | `Core::is_dirty`/`mark_saved`（§5；plan_reload 权威取值） | r24_session |

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

## §4 设置面（v1.1 增补，R24-a）——contract_gate.py 锁

### window 面（冻结 v1.1）

```ts
window.__settings(): SettingsSnapshot
// 返回当前设置快照的深拷贝（防外部突变；JSON 形状 = mdshell::settings::Settings
// serde 面：schema_version + editor/ai/storage/appearance 四层）。

window.__setSetting(path: string, value: number | boolean | string): boolean
// 已知键 + 类型/值域校验通过 → 更新快照 + dispatch
//   CustomEvent('mdc-settings-changed', { detail: { path, value } }) → true
// 未知路径 / 类型不符 / 越界 → false，不派事件（快照零触碰）。
// 'schema_version' 为保留键，不可经此面写入（迁移框架独占）。

window.__contractVersion = 1.1
```

### 默认值表（双侧一致；Rust 权威 = `mdshell::settings`，web 快照锁样例）

| 键 | 默认 | 值域 |
|---|---|---|
| schema_version | 1 | 保留键（迁移独占） |
| editor.undo_coalesce_ms | 1000（= mdcore COALESCE_MS） | 1..=10000 |
| editor.progressive_open_threshold_bytes | 1048576 | 0..=2^30 |
| ai.commands_enabled | true | bool |
| storage.meta_sidecar | true | bool |
| storage.autosave_enabled | false | bool |
| storage.autosave_interval_ms | 30000 | 1000..=3600000 |
| storage.session_restore | true | bool（R24-b 消费） |
| appearance.theme | "system" | light \| dark \| system |
| appearance.font_size_px | 15 | 8..=48 |

### Rust 侧（mdshell::settings，headless 可验证；非 window 面）

- `SettingsStore`：`load`（文件不存在=Created / IO、解析失败=FileDegraded）/
  `save`（原子写 tmp+rename；未来 schema 拒写防降写毁数据）/ `set`（已知键
  封闭集，点路径，类型+值域校验）/ 状态报告（Clean / Created / Migrated /
  KeyDegraded / FileDegraded / FutureSchema）。
- 持久化 = TOML sidecar；未知键/注释/排版往返保留（前向兼容）；版本迁移
  框架（v0 无版本键 → v1 补键）。
- 宿主接线（快照注入 web 面 + 落盘时机）随 R24-d 壳实体轮落地。

### v1 → v1.1 变更记录（2026-10-10，R24-a）

- window 面新增：`__settings` / `__setSetting` / `mdc-settings-changed` 事件
  （纯增量——启动面/注入面/`__bridge` 面/键位面全部不变）。
- `__contractVersion`: 1 → 1.1。
- Rust 侧新增 `mdshell::settings` 模块（不触碰 §1 核冻结面）。

## §5 会话面（v1.1 增补，R24-b）——纯宿主面（Rust↔Rust）

> R24-b 多窗口核侧。**window 面零新增，`__contractVersion` 保持 1.1**
> （版本常量守卫的是 web window 面；本节全部为 mdshell 宿主面，
> 消费方 = 原生壳（Rust），不经 web 桥）。破坏性变更同样走本文
> + 双侧锁（`crates/shell/tests/r24_session.rs` + `contract_freeze.rs`
> 词面锁）三处同步。

### 机制合同（语义锁背书：r24_session.rs 10 条）

- **一窗一档（v0）**：同一 `document_id` 不得同时开两窗
  （`SessionError::DocumentAlreadyOpen` 携带持有窗口，宿主 UX 应改为
  聚焦该窗）；匿名文档（id=None，未另存新档）不受限。
- **资源上限**：并发会话 ≤ limit（`DEFAULT_MAX_SESSIONS = 16`）；超限
  `open` 显式 Err，不静默驱逐（驱逐策略 = 宿主 UX，核侧提供
  `dirty_windows` / `windows_in_focus_order` 决策输入）。
- **焦点序（MRU）**：`focus` 维护，末位 = 最近聚焦；会话记录按焦点序
  导出，恢复按序重开、末位获最终焦点。
- **确定性**：BTreeMap + 显式焦点 Vec，无迭代序歧义；记录 JSON
  字段序 = 结构序，同态同串。

### Rust 面（mdshell::session；§1 compile 冻结面扩展）

```rust
const DEFAULT_MAX_SESSIONS: usize = 16;
const SESSION_RECORD_VERSION: u64 = 1;
struct WindowId(pub u64);  // 宿主分配，核侧不解释
enum SessionError { LimitReached{limit}, DuplicateWindow(WindowId),
                    UnknownWindow(WindowId), DocumentAlreadyOpen{document_id, window} }
struct SessionManager { /* new_with_limit(limit) / Default */ }
impl SessionManager {
    fn open(&mut self, window: WindowId, session: DocumentSession) -> Result<(), SessionError>;
    fn close(&mut self, window: WindowId) -> Result<DocumentSession, SessionError>;
    fn focus(&mut self, window: WindowId) -> Result<(), SessionError>;
    fn focused_window(&self) -> Option<WindowId>;
    fn windows_in_focus_order(&self) -> Vec<WindowId>;
    fn window_of_document(&self, id: &DocumentId) -> Option<WindowId>;
    fn dirty_windows(&self) -> Vec<WindowId>;
    fn get / get_mut(window) -> Result<&mut DocumentSession, SessionError>;
    fn snapshot_record(&self) -> SessionRecord;   // 恢复上次打开集（焦点序）
}
struct SessionRecord { schema_version: u64, windows: Vec<WindowRecord> }  // to_json/from_json
struct WindowRecord { window: u64, document_id: Option<String>, path: String }
```

### Core 脏标记（§1 核面扩展，compile 冻结）

```rust
impl Core {
    fn is_dirty(&self) -> bool;    // 粘性：View/Ai/Undo/Redo 置位
    fn mark_saved(&mut self);      // 宿主落盘成功后调用（核不碰 fs）
}
```

- 置位规则：交互事务（View/Ai/Undo/Redo）置位；**System 装载事务
  不置位**（缓冲 == 外部内容）；`adopt_external` 成功后清位；
  undo 回到已存内容仍报脏 = 安全向假阳性（宁可多问一次存盘）。
- `plan_reload(report, local_dirty)` 的 `local_dirty` 权威取值 =
  `core.is_dirty()`（此前宿主自行估算，无权威源）。
- `mem_stats()` 增 `dirty` 字段（宿主内存面板同源）。

### 会话持久化（恢复上次打开集）

- 记录 = 纯数据 JSON（可再生缓存，非用户数据）：宿主写盘，消费方为
  `storage.session_restore`（§4 默认 true）。无路径会话（未另存新档）
  不入记录（v0 无草稿暂存）。
- 未来 schema（version > 1）→ `from_json` Err → 宿主空会话冷启动
  （与 settings 的 FutureSchema 拒写不同级：设置丢了是用户损失，
  会话记录丢了只少恢复几个窗口）。
- `DocumentSession` 增 `path: Option<String>`（宿主域透传，核不解释）。

### 宿主接线（R24-d 壳实体轮）

- 启动：`session_restore=true` → 读记录 → 按序 `open_document(md, meta)`
  → `with_path` → `SessionManager::open`（末位窗口获最终焦点）。
- 退出/崩溃恢复：定期 + 退出时 `snapshot_record().to_json()` 落盘。
- 关窗：`close` → 返回会话 `core.is_dirty()` → 宿主「保存确认」UX →
  保存则 `save_document` + 写盘 + `mark_saved`。
- 存盘后：`save_document()` 返回值落盘成功 → `core.mark_saved()`。

### v1.1 内变更记录（2026-10-10，R24-b）

- 新增 §5（宿主面文档；window 面零新增，`__contractVersion` 不变）。
- §1 mdshell 签名增 `SessionManager` / `SessionRecord` / `WindowId`；
  核面增 `Core::is_dirty` / `mark_saved`；`DocumentSession` 增 `path` 字段
  （构造函数面兼容：新增字段走 `DocumentSession::new().with_path()`
  构造器，既有结构字面量构造点 = 唯三公开构造函数，全部同步）。
