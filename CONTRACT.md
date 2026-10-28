# CONTRACT.md — 宿主 ↔ 桥 ↔ 核 契约（冻结 v1.6）

> R23-d 冻结 v1（2026-10-10）；R24-a minor 升版 v1.1（2026-10-10，纯增量：
> window 面增设置读写项，v1 面零破坏）；PDF-b minor 升版 v1.2（2026-10-10，
> 纯增量：新增 §6 PDF 面（草案），window 面零变化，`__contractVersion`
> 保持 1.1）；PDF-e minor 升版 v1.3（2026-10-11，纯增量：§6 内 serde
> 协议面增量——危险族 5→8 族、`OpenAssessment` 增 `form_probe` 字段、
> `WorkerReply` 增 `Timeout` 变体、`SecurityDeny` 新理由码三枚；window 面
> 零变化，`__contractVersion` 保持 1.1）；#16/A4 minor 升版 v1.4
>（2026-10-27，纯增量：§4 设置快照增第五层 `docx_export` + window 面
> 测试桥 `__exportDocxStyled`，`__contractVersion` 1.1 → 1.2，v1.3 面零
> 破坏）；W-1/W-2 minor 升版 v1.5（2026-10-28，纯增量：§4 `docx_export`
> 层增 `toc`/`anonymize` 两布尔键（wasm 导出 options 两新可选键的
> 设置面消费），`__contractVersion` 1.2 → 1.3，v1.4 面零破坏）；
> #43 minor 升版 v1.6（2026-10-28，纯增量：§4 `appearance` 层增
> `language` 键（界面语言 auto/en/zh-CN——web i18n 消费单一真源），
> `__contractVersion` 1.3 → 1.4，v1.5 面零破坏）。目的：
> 消除原型母本与产品仓（mdcore/）的契约漂移——两侧
> 对同一契约面的实现必须逐签名一致；破坏性变更必须升
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
`__perf`（loadMs/typeMs/fallbackSyncMs/undoMs/progressive）；`__contractVersion: 1.4`。

**渐进路径专属**（仅 `?chunks=K>1` 渐进启动安装；一次性启动与 `__inject`
重建不安装——T3 复用地基轮勘误对齐，此前误记为恒在）：
`__wasmMemory`；`__heapTrace`。

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

## §4 设置面（v1.6 增补 #43；v1.5 增补 W-1/W-2；v1.4 增补 #16/A4；window 面版本 `__contractVersion` 1.4）

### window 面（冻结：`__contractVersion` = 1.4）

```ts
window.__settings(): SettingsSnapshot
// 返回当前设置快照的深拷贝（防外部突变；JSON 形状 = mdshell::settings::Settings
// serde 面：schema_version + editor/ai/storage/appearance 四层）。

window.__setSetting(path: string, value: number | boolean | string): boolean
// 已知键 + 类型/值域校验通过 → 更新快照 + dispatch
//   CustomEvent('mdc-settings-changed', { detail: { path, value } }) → true
// 未知路径 / 类型不符 / 越界 → false，不派事件（快照零触碰）。
// 'schema_version' 为保留键，不可经此面写入（迁移框架独占）。

window.__contractVersion = 1.4
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
| appearance.language | "auto" | auto \| en \| zh-CN（#43：auto = 跟随 navigator.language；web i18n 消费） |
| appearance.font_size_px | 15 | 8..=48 |
| docx_export.preset | "default" | default \| official-a4 \| compact \| large-print \| custom |
| docx_export.size_half_points | 28 | 21 \| 24 \| 28 \| 32（UI 三选离散域） |
| docx_export.line_spacing | 360 | 240 \| 360 \| 480 |
| docx_export.margin_twips | 1800 | 1134 \| 1440 \| 1800 |
| docx_export.toc | false | bool（W-1：目录域开关） |
| docx_export.anonymize | false | bool（W-2：暗标匿名导出开关） |

> #16（A4）：`docx_export` 层由 ⚙ 导出面板与 `__setSetting` 双源写入
>（设置快照 = 权威态 + `mdc-docx-export` localStorage 镜像作 web 侧
> 重载种子）；导出按钮经 `export_docx_styled(options JSON)` 消费
>（wasm 面——issue #10）。默认 `default` = 空 options ≡ 旧
> `export_docx` 字节级等价（零破坏）。测试桥：`__exportDocxStyled`。
>
> W-1/W-2（issue #29 阶段 A0）：`toc`/`anonymize` 两布尔键同层注册
>（同一双源写入/镜像/消费链）——`toc=true` → 导出 options 增
> `"toc": {"enabled": true, "level_from": 1, "level_to": 3}`（层级
> 用默认 1-3，不 UI 化）；`anonymize=true` → 增 `"anonymize": true`
>（docProps 元数据清零后处理——暗标 G3/R-11）。两键默认 false =
> options 不携带对应键 ≡ 既有导出字节级等价（纯加性）。wasm 面
> options 键语义单一真源 = `crates/mdcore-wasm/src/docx_export.rs`
> 模块文档。

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

### v1.3 → v1.4 变更记录（2026-10-27，#16/A4；`__contractVersion` 1.1 → 1.2）

- 设置快照新增第五层 `docx_export`（四键：preset/size_half_points/
  line_spacing/margin_twips——离散值域与 Rust `bounds` 同表）。
- window 面新增：`__exportDocxStyled(optionsJson?)` 测试桥（直通 wasm
  `export_docx_styled`，零副作用——不下载/不触碰状态行）。
- `__contractVersion`: 1.1 → 1.2。既有 v1.1 键位/面全部零变化（纯增量
  ——§4 v1.1 九键 + §1/§2/§3/§5/§6 全部零触碰）。

### v1.4 → v1.5 变更记录（2026-10-28，W-1/W-2；`__contractVersion` 1.2 → 1.3）

- 设置快照 `docx_export` 层增两布尔键：`toc`（目录域开关——导出
  options `toc` 键消费）/ `anonymize`（暗标匿名导出开关——导出
  options `anonymize` 键消费）。默认均 false（缺省 = 既有导出字节级
  等价——纯加性；wasm options 键为可选键，缺省行为不变）。
- `__contractVersion`: 1.2 → 1.3。v1.4 全部键位/面零变化（含
  `__exportDocxStyled` 桥签名——options JSON 增可选键不属桥面变化）。

### v1.5 → v1.6 变更记录（2026-10-28，#43；`__contractVersion` 1.3 → 1.4）

- 设置快照 `appearance` 层增 `language` 键：`auto`（默认——跟随
  `navigator.language`，zh* → zh-CN，其余 en）| `en` | `zh-CN`（显式
  覆盖）。消费面 = web i18n（`initI18n` 单一真源；语言切换经
  `__setSetting('appearance.language', …)` → localStorage 镜像
  `mdc-language` → 全页重建）。Rust 侧 `bounds::LANGUAGES` 同表；旧
  TOML/JSON 快照缺键 = serde 默认 `auto`（前向兼容）。
- `__contractVersion`: 1.3 → 1.4。v1.5 全部键位/面零变化（纯加性）。

## §5 会话面（v1.1 增补，R24-b）——纯宿主面（Rust↔Rust）

> R24-b 多窗口核侧。**window 面零新增（`__contractVersion` 变更历史见
> 头部——当前 1.4，随 §4 v1.6）**
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

## §6 PDF 面（v1.2 增补，PDF-b——草案；v1.3 增补，PDF-e）

> PDF-b 骨架轮（mdc-pdf crate，crates/pdf；md-line/pdf_carrier_plan §5）。
> **window 面零新增（§6 从未触碰 web window 面——当前 `__contractVersion`
> 1.2 来自 §4 v1.4，与本节无关）**——版本常量守卫的是 web window 面；本节全部为独立 crate Rust 面 + 宿主协议草案
> （`mdcore://pdf` 静态视图层不经 CM6），仿 §5 R24-b 先例并显式在档，
> 防误升版/漏升版两向漂移。PDF 面协议在 PDF-c 视图层落地时定稿。

### 架构位置

- **mdc-pdf = 第二 Document Model**：与 mdcore 块核**并列、互不嵌套**，
  crate 间零 path 依赖（Source 等共享语义 = **镜像**而非复用；serde 名
  与 `mdcore::history::Source` 逐词面一致，语义锁背书）。PDF = 版式
  文档（页 + 绘制指令 + 对象图），与块结构模型异构；产品边界 =
  阅读 + 标注，不做内容编辑（主档边界不变）。
- 标注模型复用 PDF 标准 Annot 对象（v0 四种：Highlight / Note /
  FreeText / Ink），**不自研锚定层**；Human 与 AI 走同一 Command 路径
  （PDF Annot `/T` author 字段承载 Human/Ai 溯源）。

### 命令面摘要（mdc-pdf::command；单一调度入口 = PdfSession::execute）

六命令（每命令必带 `source` 溯源）：`ExtractText{target}`（页粒度，
文本级范围 PDF-c）/ `Search{query,opts}` / `Outline` / `AnnotateAdd{draft}`
/ `AnnotateEdit{id,patch}`（kind 不可变） / `AnnotateList{filter}`。
错误面（typed，不 panic、不 String 错误）：`NotOpen`（**协议层专用**——
会话类型层只有打开后形态）/ `InvalidPage(PageIndex)` /
`AnnotNotFound(AnnotId)` / `SecurityDeny{reason}`（稳定策略码） /
`EmptyQuery`（S3 ❶）。

### mdcore://pdf 协议草案（mdc-pdf::protocol）

- scheme `mdcore`、face `pdf`：`mdcore://pdf/<document-slot>/…`——独立
  轻量视图层（不经 CM6）；路径段与槽位语义 PDF-c 视图层定稿。
- 事件族（`mdc-*` 命名域）：`mdc-pdf-cmd`（命令信封
  `CommandEnvelope{id, command}`）/ `mdc-pdf-event`（回执与自发事件）。

### 安全策略摘要（mdc-pdf::security；SECURITY_POLICY.md v1 权威）

- **S1 引擎能力面**：pdfium 只取 non-V8 构建（JS 不接、XFA 不启用）+
  零网络 + mupdf 禁入（AGPL）——选型即安全决策（ADR-P1）。
- **S2 危险对象族（v1.3 起八族）**：`/OpenAction` `/AA` `/Launch`
  `/URI` `/EmbeddedFiles` `/RichMedia` `/JavaScript` `/Names` JS 树
  ——打开时只读检测告警（`OpenAssessment.warnings`，PDF-e 实装：
  feature `engine-lopdf` 下 lopdf 对象图全扫，`open_scan()` /
  `LopdfWriter::from_bytes` 装载后填充；serde 族名冻结锁背书），
  不执行、不阻断打开（S2.4 只读告警语义）。
- **S3 限额三行为学**：❶硬拒绝（空查询；作者名 > 200 字符；打开
  输入 > 1 GiB；打开预算 3s 超时；渲染目标缓冲 > 1 GiB）/ ❷钳制
  （搜索 `max_hits` ≤ 1000 + `truncated` 告知；摘录 ≤ 200 字符）/
  ❸弃置（渲染看门狗 20s——PDF-e 实装：超时作业 `Timeout` 回执 +
  工人毒化换代）。打开面限额接线（v1.3，PDF-e）：L7 输入字节 /
  L4 打开预算在 lopdf `from_bytes` / `open_scan` 与 pdfium
  `PdfiumEngine::open` 入口统一执行；lopdf 内部无挂点的两限额
  （xref 对象数、解析器内逐对象深度）登记为 v0 边界。
- **S4 沙箱与进程边界**：专职渲染线程（单工人常态）+ 逐页 panic
  隔离（catch_unwind → `RenderPanic` 回执，同工人续跑）+ 看门狗
  超时换代（`RenderWorker::watchdog` 工厂：引擎字节快照重开替代
  工人，PDF-e 实装；**非常驻工人池**——S4.1 措辞与实装对齐）+
  读写分线程快照（PDF-c/d 接线；trait 面 object-safe 已冻结）。
- **S5 加密签名面**：仍加密态（`OpenAssessment.encrypted`）拒绝一切
  写类命令（`SecurityDeny`，先于存在性检查）；增量写保持原字节前缀
  （签名文档友好，PDF-d 语义锁）。
- **S6 表单探针（v1.3 增）**：/AcroForm 存在时字段枚举进
  `OpenAssessment.form_probe`（字段全名 + 可读现值 + 总数；PDF-e
  实装）——被动可读面告警，**永不初始化交互式 formfill**；serde
  增量兼容（旧 JSON 缺省 = None，None 序列化省略字段）。

### 时间线

- **PDF-c 阅读面（已完成 2026-10-10）**：`DocumentEngine` 实装
  （pdfium 绑定，feature `engine-pdfium`，non-V8）+ LRU 页缓存 +
  渲染工人（S4 panic 隔离）+ 会话引擎接线 + 文本层；性能门实测入
  gates.md PDF-c 节（打开 0.18ms / 翻页命中 5µs / p95 渲染 1.1ms）。
- **PDF-d 标注面（已完成 2026-10-10）**：lopdf 增量写落盘（feature
  `engine-lopdf`，四 kind 标准 Annot 零私有扩展）、Acrobat 互通引擎层
  预证（lopdf 写 → pdfium 读；GUI 层 = 开发环境轮）、原字节前缀逐位
  保持语义锁；命令面不变（flush = 会话 API 非命令，六命令零变化）。
  gates.md PDF-d 节在档。
- **PDF-e 安全姿态对齐（已完成 2026-10-11）**：S2/S6 打开时扫描实装
  （`security_scan`，八族 + 表单探针）、S3 看门狗换代 + 打开面限额
  接线、文档与实装对齐（本轮 v1.3）；gates.md PDF-e 节在档。

### v1.2 → v1.3 变更记录（2026-10-11，PDF-e）

- §6 内 serde 协议面增量（全部为**纯增量**，旧消费者零破坏）：
  - `DangerousFamily` 5 族 → **8 族**（新增 `UriAction` / `JavaScript` /
    `NamesJavaScript`——对齐 SECURITY_POLICY S2.2/S2.3 全清单；serde
    名冻结锁同步扩）；
  - `OpenAssessment` 增 `form_probe: Option<FormProbe>` 字段
    （`#[serde(default, skip_serializing_if)]`——旧 JSON 缺省反序列化
    = None，None 序列化省略字段；`FormField{name, value}` / `FormProbe
    {field_count, fields}` 新 serde 形状）；
  - `WorkerReply` 增 `Timeout { id }` 变体（Serialize-only 错误面单向，
    与 `CommandError` 同纪律）；
  - `SecurityDeny.reason` 新理由码三枚：`S3-L7-open-file-bytes` /
    `S3-L4-open-budget` / `S3-L0-decode-output`（稳定策略码语义不变）。
- **`__contractVersion` 保持 1.1**：window 面零变化（版本常量守卫 web
  window 面；PDF 面为独立 crate Rust 面，未进 web 桥）——仿 v1.2
  先例并显式在档。
- Rust 面新增 `security_scan` 模块（feature `engine-lopdf`）与
  `RenderWorker::watchdog` 工厂 / `generation()` 观测面、
  `LopdfWriter::from_bytes_with_limits` / `assessment()`、
  `Limits::check_open_input_bytes` / `check_open_budget` /
  `enforce_decode_output_bytes`（§1 compile 冻结面扩展，非 serde 面）。

### v1.1 → v1.2 变更记录（2026-10-10，PDF-b）

- 新增 §6（PDF 面草案；纯增量——§1-§5 全部不变）。
- **`__contractVersion` 保持 1.1**：window 面零变化（版本常量守卫 web
  window 面；PDF 面为独立 crate Rust 面 + 协议草案，未进 web 桥）。
  仿 R24-b §5 先例并显式在档，防误升版/漏升版两向漂移。
- workspace 新增 crates/pdf（package `mdc-pdf`，第二 Document Model；
  **零引擎依赖**——pdfium/lopdf 自 PDF-c/d 引入，骨架轮不耦合原生库，
  workspace 在无 pdfium 二进制的环境保持全绿）。
