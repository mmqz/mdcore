# C2 — mdcore-wasm 内部深审（全量 10.3K LOC）

- 基线：`main@6322f86`（audit 分支检出，2df46ef）。范围：`crates/mdcore-wasm/`（src 8 文件 10,296 行 + tests/ 8 文件 + examples/ 5 文件 + Cargo.toml）。
- 方法：逐文件精读 src 全部 8 文件；对 `web/src/` 11 个 TS 文件做导出消费对账（grep 逐方法核对）；对 mdcore / mdc-canvas / mdc-collab / office-io 的交界函数下钻验证行号。
- 置信四档：**已确认**（代码级证据闭环）/ **高度怀疑**（代码证据 + 需运行时确认）/ **需外部环境验证**（依赖 WebView/浏览器行为）/ **设计偏好**（取舍登记非缺陷）。
- 纪律：只读审计，未修改仓库任何文件。所有行号均经打开文件核对。

---

## ① API 面全清单与消费对账

### 1.1 总数

`#[wasm_bindgen]` 导出入口共 **91 个**（含 5 个构造器、4 个静态 `restore`、1 个静态 `replay`、2 个自由函数）。按类分布：

| 句柄类 | 文件 | 导出数 | 产品消费 | 死导出 |
|---|---|---|---|---|
| `WasmCore` | lib.rs(24) + docx_export.rs(3) + xlsx_export.rs(1) | 28 | 18 | 10 |
| `WasmWorkbench` | workbench.rs | 21 | 17 | 4 |
| `WasmCanvas` | canvas.rs | 17 | 17 | 0 |
| `WasmCollabSession` | collab.rs | 16 | 15 | 1 |
| `WasmBook` | xlsx_grid.rs | 7 | 6 | 1 |
| 自由函数 | docx_import.rs:775 / xlsx_import.rs:727 | 2 | 2 | 0 |
| **合计** | | **91** | **75** | **16（17.6%）** |

（注：`build_sample_docx`/`build_sample_xlsx` 为 `#[doc(hidden)]` 原生夹具产线函数，**不带** `#[wasm_bindgen]`，不计入导出面。）

### 1.2 WasmCore 28 面分组（编辑/查询/undo/导入导出/持久化/探针）

- **编辑**：`apply_ops`(lib.rs:64)、`replace_text_at_anchor`(lib.rs:81)、`sync_from_markdown`(lib.rs:168)、`extend_from_markdown`(lib.rs:178)。
- **查询**：`to_markdown`(lib.rs:119)、`blocks_json`(lib.rs:126)、`blocks_count`(lib.rs:132)、`blocks_json_range`(lib.rs:161)、`word_count_json`(lib.rs:145)、`revision`(lib.rs:245)、`take_events`(lib.rs:114)。
- **undo**：`undo`(lib.rs:101)、`redo`(lib.rs:107)。
- **导入导出（office）**：`import_docx`/`import_xlsx`（自由函数）、`export_docx`(docx_export.rs:1469)、`export_docx_styled`(docx_export.rs:1487)、`estimate_pages_json`(docx_export.rs:1502)、`export_xlsx`(xlsx_export.rs:504)。
- **持久化/身份/时钟**：`snapshot`(lib.rs:225)、`restore`(lib.rs:230 静态)、`replay`(lib.rs:239 静态)、`history_json`(lib.rs:215)、`set_history_json`(lib.rs:219)、`document_id`(lib.rs:186)、`set_document_id`(lib.rs:192)、`set_clock_ms`(lib.rs:207)、构造器 `new`(lib.rs:54)。
- **探针**：`debug_tree`(lib.rs:211)、`core_mem_stats`(lib.rs:153)。

### 1.3 死导出清单（16 个，全部**已确认**）

| 死导出 | 位置 | 语义与备注 |
|---|---|---|
| `replace_text_at_anchor` | lib.rs:81 | 文档注释自称「AI 路径」，但 web/src 零调用——AI 锚点写入路径**未接线** |
| `document_id` / `set_document_id` | lib.rs:186/192 | §8.3 文档身份面，宿主从未消费 |
| `debug_tree` | lib.rs:211 | 调试导出 |
| `history_json` / `set_history_json` | lib.rs:215/219 | undo 历史持久化双面——产品崩溃恢复只存纯文本（见 §⑧-F） |
| `snapshot` / `restore` / `replay` | lib.rs:225/230/239 | 整个核快照/重放机器在产品路径零消费 |
| `estimate_pages_json` | docx_export.rs:1502 | N-5 超页预估面——仅 examples/n1_n5_harness.rs 消费 core 函数，web 面零调用 |
| `set_review_policy` / `pending_reviews_json` / `stale_evidence_json` / `export_anonymous` | workbench.rs:476/493/498/527 | evidence_panel.ts 声明了 face 接口（60-64 行）但从不调用；`export_anonymous` = 暗标匿名导出（W-2/G3/R-11 声称的 BLOCKER 级功能）**产品未接线** |
| `find_block_json` | collab.rs:612 | 全仓零消费（web/src 与本 crate tests 均无）——零测试覆盖的死面 |
| `cell_kind` | xlsx_grid.rs:364 | xlsx_grid.ts 声明未调用 |

**要点**：死导出不是纯卫生问题——`snapshot/restore/replay/history_json` 是**整套会话持久化机器**，`export_anonymous` 是暗标合规红线功能，二者的缺席直接塑造了 §⑧-F 的持久化形状结论。`WasmCanvas` 消费率 100%（产品化最完整的表面）。

### 1.4 消费对账方法学

逐方法 grep `web/src/*.ts`（11 文件，10,739 行 TS）：WasmCore 18 面全部落在 main.ts + bench.ts + draft.ts；WasmBook 6 面在 xlsx_grid.ts（`export_with_edits` 经 1100-1116 行鸭子探测消费）；WasmWorkbench 17 面在 evidence_panel.ts（构造 485 行、restore 720 行）；WasmCollabSession 15 面在 collab_channel.ts；WasmCanvas 17 面在 canvas_view.ts。

---

## ② JSON 边界健壮性逐入口评级

总评：**typed 错误码纪律贯彻 91/91 入口，未发现任何「畸形 JSON → panic」路径**。风险集中在 panic 放大层（见 ⑤/⑦）而非解析层。

### 2.1 WasmCore 面

| 入口 | 畸形输入行为 | 证据 | 评级 |
|---|---|---|---|
| `apply_ops` | 坏 JSON → `Err("bad ops json: …")`（lib.rs:70-71）；非法 source → typed 拒（lib.rs:260-277，五合法值显式映射——G3/U2 修复后形态）；op 越界 path/range → mdcore `Reject` → `Err(JSON)`（lib.rs:74-77）；serde_json 递归深度默认 128 → 深嵌套返回 Err 不炸栈 | lib.rs:64-78 | **健壮** |
| `replace_text_at_anchor` | JS 负数/小数 → wasm-bindgen glue TypeError（可捕获）；start>end/锚点不存在 → Reject → Err | lib.rs:81-99 | 健壮（死导出） |
| `sync_from_markdown` / `extend_from_markdown` | 悬挂 chunk → `IncompleteChunk` Reject、树零污染（lib.rs:178-183 注释契约） | lib.rs:168-183 | 健壮 |
| `blocks_json_range(from,count)` | 越界截断，`from ≥ 总数 → "[]"`；`remaining` 计数器防越界 | mdcore/debug.rs:69-93；调用面 lib.rs:161-163 | 健壮 |
| `set_document_id` | 非法 hex → `Err`（lib.rs:192-203） | lib.rs:192 | 健壮 |
| `snapshot/restore/replay/set_history_json` | 坏 JSON → `Err(e.to_string())` | lib.rs:219-243 | 健壮 |
| 无参读面（`to_markdown/blocks_json/word_count_json/take_events/…`） | 无输入面；序列化失败 fallback `"[]"`/`"{}"`/`"\"invalid\""`（lib.rs:116/146/251，理论不可达） | lib.rs | 健壮 |

### 2.2 office 导入导出面

- `import_docx(bytes)`（docx_import.rs:774-779）→ `import_docx_core`（760-765）→ office-io 容器三门（prescan 条目数/单尺寸/总量/路径逃逸）+ 兼容模式读。错误码表 6+1 个（docx_import.rs:138-167：`container_prescan/container_corrupt/container_parse/container_invalid_part/docx_parse/io/docx_limit`）。**630 件真实世界敌意语料背书**（POI 129 + docx4j 501，见 tests/health_check_docx.rs:1-40，catch_unwind 硬门）。
- `import_xlsx(bytes)`（xlsx_import.rs:726-731）同构；渲染窗上限 MAX_SHEETS=64/MAX_ROWS=5,000/MAX_COLS=128（xlsx_import.rs:170-181）钳制 + 丢弃计数显式进 stats。
- `export_docx`/`export_xlsx` 无参数；映射层全函数（heading 钳 1-9：docx_export.rs:169-174；mark span 防御钳位：docx_export.rs:208-215；行/列病理截断：xlsx_export.rs:400-402/425-431）。错误仅引擎面 `docx_build/docx_limit/xlsx_build/xlsx_limit`。
- `export_docx_styled`/`estimate_pages_json` 的 options JSON：**严格白名单键**（未知顶层键即拒，docx_export.rs:1133-1147）+ 路径化字段错误（`req_str/req_int`，docx_export.rs:1166-1199+）+ 值域交引擎 V9 漏斗。抽查确认逐字段 as_str/as_u64 + `u32::try_from` 类转换都有 typed 分支。**健壮**。

### 2.3 会话类（canvas/collab/workbench/book）

- `WasmCanvas::execute(actor, cmd_json)`：`parse_cmd_core`（canvas.rs:119-221）逐字段类型检查——`id` 非串/缺 `t`/未知命令/缺字段均 typed `cv_bad_json`；`f64` 字段接受 `1e400`→inf 但下游 `CanvasError::NonFiniteGeometry` → `cv_non_finite`（canvas.rs:67）typed 拒。actor 枚举 typed（canvas.rs:335-341）。
- `WasmCollabSession::execute`：`parse_cmd_core`（collab.rs:212-348）同款纪律；`req_u32` 显式 `u32::try_from` 域检查（collab.rs:202-208）；未知 kind/attrs 非串均 typed。
- `WasmWorkbench` 21 面入参：`parse_binding_core`（workbench.rs:112-149，relation 四枚显式）、`parse_calc_core`（152-222，Decimal 串 typed 解析、target_scale u8 域检查 216-217）、`parse_review_policy_core`（238-285，**空 reviewer_prefixes fail-open 收紧为 typed 拒**——审查 S3 修复面，workbench.rs:254-261）。
- `WasmBook`：sheet 索引越界 typed `grid_sheet_index`；`slice_json` 窗超 `MAX_SLICE_CELLS=100_000` typed 拒（xlsx_grid.rs:279-285）；窗外钳制为 empty 填充（非错误——虚拟滚动语义）；`export_with_edits` 账本逐条校验 + `MAX_EDIT_CELLS=100_000`（xlsx_grid.rs:159-225）。
- **唯一静默降级族**：序列化 fallback（`"[]"`/`unwrap_or_default`）——cell_display 空格返回 `""`（xlsx_grid.rs:356-360，窗外语义一致）。

**结论**：解析层无恐慌路径（**已确认**）。残余风险 = mdcore/office-io 引擎内部不变量被打破时的 panic（无隔离无 hook，见 ⑦-4）。

---

## ③ 内存/拷贝模式

### 3.1 已知热点复述（P1-19，行号见 §⑧）

- `Core::apply_ops` 每事务 `let backup = self.tree.clone()`（mdcore/src/lib.rs:297）——wasm 侧每一击键经 main.ts:874/1157 `apply_ops` 均触发**整树深克隆**。
- **补充（本审计新增）**：undo/redo 走 `apply_raw_batch`，**同样整树 clone**（mdcore/src/lib.rs:380）——每次撤销/重做 = 再一次全树克隆 + 逆 op 应用。P1-19 的实际代价 = 每键一次 + 每次 undo/redo 一次。

### 3.2 新发现：状态栏 500ms 轮询的 O(doc) 无条件扫描（N-1，见 ⑦-1）

`main.ts:2668` `setInterval(refresh, 500)` 的 refresh（2637-2643）**无条件**调用 `core.word_count_json()` + `core.blocks_count()`——无 revision 守卫。`word_count::count` 对每文本块做 `node.text.to_string()`（Rope→String 物化，mdcore/src/word_count.rs:225/231/237 等）+ `net_text` 剥离分配。10MB/20 万块文档 = 每 500ms 全文档遍历 + 全文档临时字符串分配，**空闲时也不停**。TOC 面板的第二轮 interval（main.ts:2783）有 `panel?.hidden` 守卫（2753 行）不构成同类问题。

### 3.3 文本跨边界拷贝计数

| 路径 | 拷贝链 | 峰值倍率 |
|---|---|---|
| 导入 docx/xlsx | JS Uint8Array→wasm 内存(1) → office-io 模型(2) → markdown String(3) → serde_json 包装串(≈+1) → JS String UTF-16(4) | ≈4× 输入规模 |
| 导出 docx/xlsx | 树→writer→zip `Vec<u8>`(1) → wasm-bindgen 拷贝成新 Uint8Array(2) → `new Blob([bytes])`(3) | ≈3× 产物规模（main.ts:2376-2378/3080 → downloadDocx/Xlsx） |
| 每次键入 | op JSON 串(1) → wasm 解析(2) → 树克隆(P1-19) → take_events 事件 JSON(3) → JS push `__mdcoreEvents`(4，无上限) | 见 ⑦-2 |
| `to_markdown`（每次 pushRender/undo/redo/草稿 flush） | 全文档 String → JS String | 2× |
| canvas/collab 事件 | wire bytes → base64(+33%) → JSON 串 → JS String | ≈2.7× |

### 3.4 泄漏模式排查

- **JsValue 引用**：无——错误全部 `JsValue::from_str` 即弃；无 `Closure` 持有、无 `free` 义务面（**已确认**无 wasm 侧 JsValue 泄漏模式）。
- **Rust 侧事件缓冲**：canvas.rs:355 / collab.rs:489 的 `Mutex<Vec<String>>` **无容量上限**，仅靠宿主轮询排空（canvas 经 rAF 循环 canvas_view.ts:368-370/640；collab 经 setInterval pumpDoc collab_channel.ts:466）。后台标签页 rAF 暂停 + BroadcastChannel 继续投递 `apply_update` → 事件无限累积 → Rust 堆增长。**需外部环境验证**（WebView 后台节流行为）。
- **JS 侧 `window.__mdcoreEvents`**：main.ts:802 每事件 push、**无裁剪**，仅换核时清零（1583/1620/1662）。长会话+大文档 = JS 堆单调增长。**已确认**（代码级；幅度需运行时度量）。
- `WasmBook.book` 常驻 wasm 内存（解析后的 typed 网格整簿常驻）——设计取舍，xlsx_grid.ts 生命周期注释已登记。

---

## ④ 导出编排链（office-io / pdf）

### 4.1 DOCX 链（写）

```
JS export 按钮 (main.ts:2152/2373)
→ WasmCore.export_docx / export_docx_styled (docx_export.rs:1469/1487)
→ export_docx_core / export_docx_styled_core (docx_export.rs:632/1302)
   └─ emit_block 递归 (docx_export.rs:~460-625)：effective_inline 行内 marks 派生
      + runs_from 五位分段合并 (docx_export.rs:197-263)
→ office-io DocxWriter (heading/paragraph/list_item[_lines]/table_header_repeat/
   code_line/quote_paragraph/horizontal_rule/image/toc/六页眉脚槽)
→ save_to_vec → zip Vec<u8>
→ (styled) anonymize=true 时过 anonymize_docx_props 后处理 (docx_export.rs 模块文档 W-2)
→ wasm-bindgen Vec<u8> → JS Uint8Array（1 拷贝）
→ Blob + a.download (main.ts downloadDocx)
```

`estimate_pages_json`（docx_export.rs:1502→1447）= 同一 options 解析（单一形状真源 `parse_style_options`）→ `mdcore::pagination::estimate`——与导出口径对齐的设计正确，但**死导出**。

### 4.2 XLSX 链（写）

`WasmCore::export_xlsx`（xlsx_export.rs:504）→ `export_xlsx_core`（445-492）：表块收集 + 紧邻前驱 Heading 认领命名（批 5 表名通道）→ 非表块进 Document sheet → 每表独立 sheet → `WorkbookWriter::save_to_vec`。读-改-重建：`WasmBook::export_with_edits`（xlsx_grid.rs:386）→ `parse_edits_core` → office-io `rebuild_book` → 新簿字节 + `last_rebuild_stats_json`（RefCell interior mutability，xlsx_grid.rs:313-315）。

### 4.3 PDF：**不在本 crate**（已确认）

mdcore-wasm 的 Cargo.toml（依赖仅 mdc-workbench/mdc-canvas/mdc-evidence/mdcore/mdc-office-io/mdc-collab/serde_json/wasm-bindgen 共 8 项）**无 mdc-pdf**。PDF 表面走独立 `crates/pdf-wasm` + `web/pkg_pdf`（pdf_view.ts:15-17 模块注释：`WasmPdfSession`（mdc-pdf wasm 桥））。mdcore-wasm 内唯一 pdf 痕迹是注释里的错误码命名先例（canvas.rs:24 / collab.rs:25）。

### 4.4 feature 门控：**无**

Cargo.toml 无 `[features]` 节——canvas/collab/workbench/office 全部无条件编入单 wasm 包（体积代价换取面全）。设计偏好。

### 4.5 字节回传拷贝

全部 office 导出 = `Result<Vec<u8>, JsValue>` → wasm-bindgen 单次拷贝成新 Uint8Array；宿主再进 Blob（1 次拷贝）。无零拷贝（无 SharedArrayBuffer/IntoJsVec 写法）——对下载频率（低频用户动作）合理。

---

## ⑤ 错误处理一致性统计

### 5.1 返回形状分布（91 面）

- `Result<String, JsValue>`：16 面（apply_ops/sync/extend/set_document_id/set_history_json/replace_text_at_anchor/estimate_pages_json + canvas execute + collab execute + workbench 提议/复算/段落/sidecar 面 + WasmBook slice 等）。
- `Result<Vec<u8>, JsValue>`：8 面（export_docx/export_docx_styled/export_xlsx/export_clean/export_anonymous/export_with_edits/encode_diff×2/authoritative_snapshot×2/canvas-collab restore）。
- `Result<(), JsValue>` / `Result<bool,JsValue>` / `Result<usize,JsValue>` / `Result<T,JsValue>` 其余。
- 纯 `String`/`Option<String>`/数值：约 40 面（读面——无失败语义）。
- **错误负载统一为 JSON 串** `{"code":"…","message":"…"}`（typed 码族：wb_*/cv_*/cl_*/grid_*/rebuild_*/docx_*/xlsx_*/container_*）——main.ts 侧 `JSON.parse(raw)` 解码（2390-2400/3090-3100 的 catch 面），trap 类异常落 `unknown` 码兜底。纪律一致性**优**。

### 5.2 生产代码 panic 点统计（分级）

| 级别 | 位置 | 性质 |
|---|---|---|
| 理论不可达 | canvas.rs:239/240/247/251/258/262/284/321；collab.rs:441/442/454/461/464/606；workbench.rs:297/311-313/321 等 | `serde_json::to_string(&str/String)` unwrap——串序列化 infallible |
| 单线程不可毒化 | canvas.rs:511 / collab.rs:644 `events.lock().unwrap()` | Mutex poisoning 需闭包内 panic；闭包仅串序列化 |
| 防御不可达 | docx_import.rs:578 `unreachable!("text boxes flattened…")` | 预处理定点展开保证 |
| 可证 ASCII | xlsx_import.rs:343 `String::from_utf8(out).expect("ASCII")` | `b'A'+n` 构造域 |
| 夹具产线（非 wasm 导出） | docx_import.rs:805-833、xlsx_import.rs:750-810 的 `.expect(...)` | `#[doc(hidden)]` 测试夹具，产品路径不达 |
| **真缺口** | **无 console_error_panic_hook（Cargo.toml 无该依赖；crate 与 web/src grep 零命中）+ WasmCore 路径无 catch_unwind** | 见 ⑦-4 |

docx_export.rs / xlsx_export.rs / lib.rs 生产段 **0 unwrap/expect**（0 处 panic 点——全部走 typed Result）。

### 5.3 panic 隔离不对称（对照）

- mdc-canvas：`catch_unwind` 包裹命令执行（canvas/src/session.rs:521/558/578）→ `cv_panic` typed 错误。
- mdc-collab：`catch_unwind` 包裹 apply_update/audit（collab/src/audit.rs:161/172/181/215；lib.rs:33 模块文档「panic 隔离」）→ `cl_panic`。
- **mdcore Core::apply_ops / replace_text / undo：零隔离**——panic 直通 wasm trap。

---

## ⑥ crate 内测试可信度（148 个）

分布：src 内嵌 117（docx_export 52 / docx_import 29 / xlsx_export 19 / xlsx_import 15 / lib 2）+ tests/ 31（canvas_wasm 7 / collab_wasm 8 / workbench_wasm 5 / xlsx_grid 5 / n3_word_count 2 / health_check_docx 2 / health_check_xlsx 1 / external_corpus 1）。

**真断言为主，非形式覆盖**（已确认）：

- 负向/敌意输入测试在场：`parse_cmd_core(r#"{"t":"bogus"}"#).is_err()`、坏 JSON `"{"`、id 类型错（tests/canvas_wasm.rs:63-66）；collab 敌意命令面（tests/collab_wasm.rs:207 `hostile_and_typed_error_faces`）；workbench 坏 JSON/坏 Decimal（tests/workbench_wasm.rs:62/64/90）。
- 写读全环纪律：docx_export 测试逐件 `export → read_docx_from_bytes → 模型断言` + zip 本地头遍历 XML 直断言（docx_export.rs:1546-1556 roundtrip 助手）；assert 密度 545 处（四 office 模块）。
- **health_check 双语料 = 最硬面**：POI 129 + docx4j 501 件真实 .docx（含 clusterfuzz 畸形件）跑六维判据，`catch_unwind` 包裹导入内核，**panic = 测试失败**（tests/health_check_docx.rs:19-40）；BASELINE.json 门禁「下降即红」。
- external_corpus.rs：pandoc 全特性语料 4 件导出回归门。

**缺口**（已确认）：

1. **JsValue 胶水层零 cargo 测试**——结构性限制（JsValue 非 wasm 目标构造即 panic，lib.rs:264-265 注释自认），靠「wasm32 check + web 三件套门禁」背书；91 面的 glue 转换（BigInt/usize 域检查/Uint8Array 拷贝）实际由 web e2e 覆盖，本 crate 内无。
2. `find_block_json` 零测试零消费（双重孤儿）。
3. `apply_ops` 畸形 ops JSON 的负向用例在 mdcore 侧，本 crate lib.rs 仅测 `parse_source_str`（2 个测试）。
4. 时钟恒 0 场景（不注入 set_clock_ms 的合并窗行为）无 wasm 侧测试钉定——P1-18 因此能长期潜伏。

---

## ⑦ 新问题清单（P0-P3 + 置信）

**本审计无 P0 级新发现。**（唯一 P0 候选 = 已知 P1-18/P1-13 的组合放大，归入 ⑧。）

### N-1【P1｜已确认】状态栏 500ms 无条件 O(doc) 全文档扫描与分配churn

- 证据链：main.ts:2668 `setInterval(refresh,500)` → refresh 体内 main.ts:2638-2641 `core.word_count_json()`+`core.blocks_count()` 无 revision 守卫 → mdcore/src/lib.rs:240 `word_count::count(&self.tree)` → word_count.rs:213-244 每文本块 `node.text.to_string()`（Rope 物化）+ net_text 剥离分配。
- 影响：10MB 目标文档（产品自述场景，lib.rs:131 注释「10MB 文档 20 万块」）空闲态每秒 2 次全树遍历 + 2 次全文档字符串物化 + JSON 序列化 + JS parse。与 R16「免 to_markdown 全量比对」的省 O(n) 纪律（main.ts:963-966 注释）直接矛盾。
- 修复方向：revision 守卫（rev === last 时跳过）或 word_count 结果缓存进 Core。

### N-2【P2｜已确认】`window.__mdcoreEvents` JS 侧无上限单调增长

- 证据：main.ts:802 `push(...events)` 无裁剪；仅换核清零（1583/1620/1662）。每事件含完整 op 载荷（inserted 全文）。
- 影响：长会话（万级击键）JS 堆持续增长；`debugLine`（main.ts:813）每键渲染其 length。探针语义（「冒烟测试 op 流证据」）与生产常驻混用。

### N-3【P2｜高度怀疑→需外部环境验证】canvas/collab Rust 侧事件缓冲无上限 + rAF 排空依赖

- 证据：canvas.rs:355/510-515、collab.rs:489/643-648（`Mutex<Vec<String>>` 无 cap，base64 +33% 膨胀）；排空靠 canvas_view.ts:368-370 rAF 循环 / collab_channel.ts:466 setInterval。后台标签页 rAF 暂停而 BroadcastChannel 投递不停（collab_channel.ts:197-199 `apply_update`）→ 事件累积在 wasm 堆。
- 降级为 P2 而非 P1：WebView（wry/webkit2gtk）后台节流行为需实测；单页桌面壳场景后台概率低。

### N-4【P2｜已确认】WasmCore panic 面零隔离零钩子（P2-9 的结构性深化）

- 证据：①Cargo.toml 依赖 8 项无 console_error_panic_hook，crate/web 全 grep 零命中；②mdcore apply 路径无 catch_unwind（mdcore/src/lib.rs:297-309/377-387），对照 canvas/session.rs:521 与 collab/audit.rs:161 均有；③apply_ops backup 仅 Err 路径回滚（lib.rs:303/309），panic 中途 = 半应用树 + 无诊断；④main.ts fast path `mutate()`（789-796）try/finally **无 catch**——trap 冒泡到 CM6 侦听器；applyOpsSync（1157-1165）有 catch。
- 影响：mdcore 内任一不变量破坏 → 不透明 trap（"unreachable executed"）+ 可能的核状态损伤后编辑继续。canvas/collab/workbench 三域已有 typed panic 码（cv_panic/cl_panic），唯独最核心的 WasmCore 没有——**不对称是设计遗漏而非纪律缺失**。

### N-5【P2｜已确认】`set_clock_ms` 冻结常量语义 + 构造器零时钟（P1-18 的 crate 侧根因面）

- 证据：lib.rs:207-209 `Arc::new(move || ms)`——返回值永远等于最后一次注入值，不随时间推进；`WasmCore::new`（lib.rs:54-60）不注入任何时钟 → mdcore/src/lib.rs:128-131 wasm32 `system_ms()=0`。正确性完全依赖宿主「每次变异前注入」纪律（main.ts:788-790 已遵守；bench.ts:114 **未遵守**——apply_ops 直调无 set_clock_ms，实测面即 P1-18 形态）。
- 对照：WasmCanvas/WasmCollabSession 用 `Arc<AtomicU64>`（canvas.rs:353/370、collab.rs:487/533）——存储-推进模型，注入一次后语义仍合理。WasmCore 用闭包置换（每次注入 1 次 Arc 分配）——不对称且弱防御。
- 修复方向：WasmCore 改 AtomicU64 同款；或构造时注入恒 0 警告。

### N-6【P3｜已确认】`__inject_chunk(first=false)` 后 BlockMap 永久失步（P2-8 的机理精确化）

- 证据：main.ts:1694-1701 监听器 `extend_from_markdown` + `pushRender(skipMap:true)`，**不调用** `ingestExtend()`（main.ts:909-936 存在且 boot 渐进路径 1590-1595 正确使用）；pushRender skipMap 分支跳过 rebuildMap（main.ts:976-980）→ `blockMap.blockCount` 停在旧值 → 追加区行 `rec` 缺失 → 快路径 bail `rec@N`（main.ts:865-868）→ **每键 fullResync**（885-897，`sync_from_markdown` O(doc) 差分 + 整树重建）。
- 与原 P2-8 描述差异：不只是「每键 O(doc)」——BlockMap 数据面本身失步（__toc/跳转用 `__blocks` 直读 core 不受影响，但快路径永久失效直到换核）。

### N-7【P3｜已确认】16/91 死导出（§1.3）——含暗标合规面 `export_anonymous` 与整套会话持久化机器未接线。

### N-8【P3｜设计偏好】base64 编解码器双份逐行重复（canvas.rs:88-112 与 collab.rs:95-119）；导出字节 3 拷贝链；`Cargo.toml:28` wasm-bindgen "0.2" 浮动次版本（wasm_boot.ts:1-14 记录了 0.2.129 glue 非并发安全导致的真实 SEGV 史——已由单飞门缓解，但版本浮动 + glue 语义敏感组合值得钉死）。

### N-9【观察｜高度怀疑】fast path trap 冒泡路径

main.ts:873-875 `this.mutate(() => { this.core.apply_ops(…) })` 内无 catch——若 mdcore panic，trap 经 finally（drainEvents 自带 try/catch）冒泡至 CM6 update listener。CM6 对 listener 异常的吞/抛行为未验证——与 N-4 同根，合并修复。

---

## ⑧ 与已知问题清单的精确印证（补行号）

### P1-18（wasm 时钟恒 0 → 合并窗失效）——**机理闭环 + 现行缓解确认**

- 根因链：mdcore/src/lib.rs:128-131（wasm32 `system_ms()→0`）→ WasmCore 构造器无注入（mdcore-wasm/src/lib.rs:54-60）→ history.rs:20 `COALESCE_MS=1000`，恒 0 时间戳差恒 <1000 → 全会话并条。
- 现行缓解：web/src/main.ts:788-790 `mutate()` 每变异前注入 `Date.now()`（apply_ops 快路径 873、split 1157、undo/redo 996/1002 均经 mutate）——**产品主路径已修复**。
- 残余风险面（本审计精确化）：①`set_clock_ms` 冻结常量语义（N-5）；②bench.ts:114 无注入直调 apply_ops——任何绕过 mutate() 的消费者复现 P1-18；③canvas/collab 时钟为 AtomicU64 推进式（canvas.rs:416-418、collab.rs:551-553），collab_channel.ts:125 `nextAt()` 单调递增——两会话域时钟纪律正确，唯独 WasmCore 弱。

### P2-9（无 panic hook；备份回滚仅 Err 路径）——已确认，深化为 N-4

- 无 hook：Cargo.toml 依赖清单（10-28 行）无 console_error_panic_hook；crate 与 web/src 全文 grep 零命中。
- 备份回滚：mdcore/src/lib.rs:297 `backup = self.tree.clone()`，303/309 两处 `self.tree = backup` 仅 Err 返回路径；**panic 中途无恢复**（trap = 中止，半应用树留存）。undo/redo 的 apply_raw_batch 同构（lib.rs:380/383）但栈/边界另有 T1-fix F10 恢复（lib.rs:908-917/940-947——比 apply_ops 更完备）。
- JS 侧兜底：导出面 catch JSON 解码（main.ts:2390-2400）能吞 trap 成 typed 失败；编辑路径 mutate() 无 catch（N-9）。

### P1-13（collab 平行宇宙）——已确认

- mdcore-wasm 侧形态：`WasmCollabSession`（collab.rs:483-492）是独立会话对象，与 `WasmCore`（lib.rs:46-49）**零字段/零方法共享**——wasm 层就没有任何 Core↔CollabSession 桥接原语（连死导出都没有）。
- web 侧证据：collab_channel.ts:441-445 `docSession = new ctor(BigInt(docClientId), UNDO_LIMIT)` **从空会话启动**；全 web/src 无 `load_blocks` 调用（repo grep 零命中）——用户编辑流（CM6View→WasmCore.apply_ops）永不进入协作文档；协作面仅承载远端 BroadcastChannel 更新与探针命令（collab_channel.ts:311-312 execute）。
- 结论：P1-13 是**跨 crate 架构缺口**（需要在 wasm 层新建 Core→CollabSession 同步原语或双写桥），非 web 层局部 bug。

### P2-8（__inject_chunk 追加区每键 O(doc)）——已确认，机理精确化为 N-6

行号：main.ts:1694-1701（监听器）、962-983（pushRender skipMap）、865-868（rec@N bail）、885-897（fullResync）、909-936（ingestExtend 存在未用）、1590-1595（boot 路径正确对照）。

### P1-19（每事务整树 clone）——已确认，undo/redo 同罪

行号：mdcore/src/lib.rs:297（apply_ops backup）+ **380（apply_raw_batch backup——undo/redo 每次同样全树克隆，本审计补充）**；wasm 侧入口 lib.rs:64-78。内存高水位归因链：每键 clone + N-1 轮询物化 + N-2 事件累积三叠加。

### F 项：undo/save/meta 持久化在 wasm 侧的形状（与 P0-1 ID 撞号的交界）

- wasm 侧**存在**完整持久化机器：`snapshot/restore`（JSON 快照，保 ID 空间）、`history_json/set_history_json`（undo 栈）、`replay`（JSONL 确定性重放，ID 由重放确定性重铸）——**但全部死导出（§1.3）**。
- 产品实际持久化 = draft.ts:57-61 纯文本信封 `{v,ts,text}`（localStorage，1MiB 上限 draft.ts:45）。恢复路径 `new WasmCore(md)` → markdown 重解析 → **BlockId 从头重铸**、undo 历史全丢。
- 与 P0-1 的关系：**不放大**（产品从不持久化/回灌 ID 引用，evidence 锚用 paragraph_index/coord 而非 BlockId）；**也不缓解**（若未来接线 snapshot/restore 或 history_json，撞号面立即激活——`set_history_json` 载入与当前树不一致的历史时 undo 走 BadPath/BadRange（mdcore/src/lib.rs:905-907 注释自认该可达面））。当前形状 = 靠「不持久化」回避问题。

---

## ⑨ 裁决建议

1. **架构层（产品路径阻断项）**：P1-13（协作平行宇宙）与 P1-18 残余（时钟弱防御）需要 wasm 层新增原语：Core→CollabSession 文档同步通道（或双写桥）、WasmCore 时钟改 AtomicU64 推进式（照抄 canvas.rs:353-370 模式）。两者都在 mdcore-wasm 内落点，改造成本中。
2. **健壮层（一次改动三收益）**：引入 console_error_panic_hook + 给 `WasmCore::apply_ops/replace_text/undo/redo` 加 catch_unwind 包装（typed `core_panic` 错误码，对齐 cv_panic/cl_panic 先例）——同时消解 P2-9 全部两面与 N-4/N-9。
3. **性能层**：N-1（word_count revision 守卫——JS 侧 3 行改动收益最大）→ N-6（inject_chunk 补 `ingestExtend()` 调用——一行）→ N-2（__mdcoreEvents 环形上限）。P1-19 整树 clone 属 mdcore 域，按既有审计裁决推进。
4. **卫生层**：死导出分两批处理——`export_anonymous`（暗标红线）与 `estimate_pages_json`（N-5 验收）建议**接线而非删除**；`find_block_json/cell_kind/debug_tree` 等确认后删或标注 `#[doc(hidden)]`；`replace_text_at_anchor` 要么接线 AI 路径要么降级登记。
5. **测试层**：补三类钉定测试——「无时钟注入的合并窗行为」（防 P1-18 回归）、「find_block_json 形状」（若保留）、wasm32 check 门禁中加入 panic hook 存在性断言。
6. **总体评价**：mdcore-wasm 是一个**纪律异常严整的边界层**（91 面 typed 错误全覆盖、0 生产 unwrap 于三个最大模块、630 件敌意语料回归门、写读全环测试）——其问题几乎全部来自**跨层契约的接线缺席**（时钟、协作、持久化、AI 锚点）而非边界实现本身。审计结论：边界层质量 A-，接线完整度 C+，产品路径综合 B-。
