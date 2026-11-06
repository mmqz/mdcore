# C4 — web/ 前端 TS 深审（全量正确性与安全面）

- 仓库：`/home/z/my-project/mdcore-audit`（audit 分支，源码 = main@6322f86）
- 范围：`web/src/` 全部 13 个 .ts（含 i18n 3 件）、`web/index.html`、`web/build.mjs`；`web/tests/` 29 个 .py 摸覆盖面（docstring 级）
- 方式：纯静态逐行深读（沙箱不可构建：`node_modules/` 与 `web/pkg/` 均缺席，CM6 内部行为按 @codemirror/view ^6.36.5 公开语义推演）
- 置信四档：**已确认**（代码结构直接可证）/ **高度怀疑**（结构清晰但依赖运行时数据形态）/ **需外部环境验证**（需构建+浏览器/CDP）/ **设计偏好**（有意的取舍，非缺陷）
- 行号纪律：全部行号来自本次逐行实读，未发明。产出只写入本文件，仓库零触碰（`git status` 复核干净）。

---

## ⓪ 架构总览与已知问题复核

**架构一句话**：wasm 核是唯一权威，CM6 doc = `core.to_markdown()` 投影；编辑走「快路径（单 change 单行 body → 一个 ReplaceText op）+ 兜底（`sync_from_markdown` 核内差分）」双层管道；undo 由核驱动（CM6 history 刻意不装）；周边四个表面（grid/pdf/canvas/evidence）各自持有独立 wasm 会话，与 WasmCore 生命周期零耦合（main.ts:1710-1745 装配注释明确交代）。

**已知问题交叉复核**（本轮逐行验证行号与语义，均属实，不计为新发现）：

| 已知 | 本轮复核证据 | 复核结论 |
|---|---|---|
| P0-4 快路径后 `BlockMap.byId.textChars` 永不更新 → list Enter 文本损坏 | 建图写入点 main.ts:210（rebuild）/317（extend）；快路径 858-886 只平移同块后续行 `textOffset`（877-885），**不动 byId.textChars**；handleEnter 1144 `end: meta.textChars` 消费 stale 值 | ✅ 属实。另注：`toggleLinePrefix` 走 forceFullSync→fullResync→rebuildMap（1083, 898），textChars 会刷新——损坏面限定在「快路径打字后直接 Enter」序列 |
| P0-5 xlsx_grid onCopy 敌意单元格文本原样写剪贴板 TSV | xlsx_grid.ts:940-965：`slice.cells[i][j]?.d ?? ''`（或台账显示文本）直接 `join('\t')` 后 `setData('text/plain')`；无任何 `=`/`+`/`-`/`@` 前缀消毒；`MAX_COPY_ROWS/COLS` 只管量 | ✅ 属实。补充：`document` 级 copy 监听以 `surfaceActive()`（936-938，activeElement===viewport）门控——攻击面限定在 grid 聚焦时用户主动 Ctrl+C |
| P2-7 fullResync 失败仍推 syncedRev + 旧树重建 map | main.ts:890-900：catch 只 console.error，随后 898 `rebuildMap`（读**旧核** blocks_json）+ 899 `syncedRev = revision()`；pushRender 守卫 970 以 syncedRev 判「核未变」→ 分叉后核驱动重渲染被永久跳过 | ✅ 属实，且推演出第二层后果：undo 在此态操作核 → pushRender 守卫因 syncedRev 同步推进而免重渲染 → 视图与核各走各路 |
| P2-8 `__inject_chunk(first=false)` 无 ingestExtend → O(doc) | main.ts:1679-1690：`extend_from_markdown` 后直接 `pushRender(undefined, {skipMap:true})`——skipMap 跳过 rebuildMap，而 map 也从未被 extend → BlockMap 停留在首段；boot 渐进路径（1589-1595）则正确调用 `view.ingestExtend()` | ✅ 属实。宿主分片注入路径与 boot 路径行为分叉 |

---

## ① 文件清单与职责矩阵

| 文件 | LOC | 职责 | 质量印象 |
|---|---|---|---|
| `src/main.ts` | 3347 | CM6↔wasm 桥（BlockMap/装饰双源/undo/Enter 核驱动拆块）+ 全部产品 UI（docx/xlsx 导入导出、格式工具栏、状态栏、TOC、查找替换、字号、帮助、主题、设置面板、导出选项面板） | 密度极高、注释与实现一致性好；单文件承载过多职责；少量监听器/overlay 生命周期瑕疵（见⑤N2）|
| `src/pdf_view.ts` | 2127 | pdf.js 显示面（canvas 位图+textLayer+LRU 页缓存）+ WasmPdfSession 模型面（扫描评估/四类标注/搜索/lopdf 增量写保存） | 工程质量好：renderToken 防错序、单页渲染串行链、L2 像素钳（1403-1415）；**2 处 innerHTML 属性插值**（见②X1/X2）|
| `src/canvas_view.ts` | 1680 | SVG 保留模式画布 + WasmCanvas 双实例（main+AI 镜像）协同演示 + 跨页传输钩 | DOM 全 createElementNS/setAttribute/textContent——XSS 安全典范；rAF 泵空转 2×FFI/帧有自知（366-370）|
| `src/xlsx_grid.ts` | 1277 | 三层虚拟化网格（主层/冻结行/冻结列+池化 DOM）+ 编辑台账（命令模式）+ 剪贴板 TSV + 读-改-重建导出 | 虚拟化、写域钳（157-161）、台账上限扎实；onCopy 公式注入（P0-5 已知）消毒未接线；`crypto.randomUUID` 依赖 secure context（823）|
| `src/evidence_panel.ts` | 983 | WasmWorkbench 八步证据闭环 web 面（body 导入/源摄入/绑定/计算/复核/导出/恢复） | 纯 textContent 卡片渲染；typed 包裹完整；reverify 双通道（claim 锚指纹 vs source 通道）有详细审计注释（608-641）|
| `src/bench.ts` | 491 | 阶段1宿主选型基准页（正文/证据/表格/画布四场景，非产品路径） | 与产品同纪律；textContent 渲染；LCG 与 Rust gen.rs 同常数对齐 |
| `src/collab_channel.ts` | 470 | BroadcastChannel 跨页协同 harness（doc WasmCollabSession + canvas 传输钩；hello/hi/bye/update/sync_req/sync_resp 协议） | 协议小而清；remote 不回发防回声环（600, 166）；40ms 永续 interval（466）|
| `src/draft.ts` | 240 | localStorage 草稿（1MiB 上限/信封版本化/四触发落稿）+ beforeunload/pagehide 拦截 | 设计成熟：三重激活门（URL cc/doc/chunks + 设置键）；pagehide 同步终刷兜底 beforeunload 被吞 |
| `src/i18n/index.ts` | 83 | 语言解析（设置键→auto→navigator）+ t()/tCount() 插值 + data-i18n 三属性静态应用 | 插值纯字符串替换、消费面全 textContent——无 HTML 注入面；未知键回退 en→键名（漏译可见）|
| `src/i18n/en.ts` | 317 | 英文词典（键封闭集，`Dict` 类型锁 zh 完备性） | 良好 |
| `src/i18n/zh-CN.ts` | 309 | 中文词典 | 良好 |
| `src/empty_state.ts` | 80 | PDF/Grid/Canvas 三表面空态引导卡（内联 SVG 图标） | setAttribute+textContent，安全 |
| `src/wasm_boot.ts` | 44 | wasm-bindgen `__wbg_init` 单飞门（双实例化 SEGV 根治，2026-10-23） | 简洁正确；失败重置门可重试；WeakMap 按模块实例分门 |
| `index.html` | 637 | 静态骨架（工具栏/页签/编辑器/四面板/帮助/调试行）+ 主题防闪白内联脚本 | 无事件处理器属性、无外链 img/iframe；缺 CSP meta（⑤N18）|
| `build.mjs` | 83 | esbuild 双入口（main/bench）+ wasm/worker/pkg_pdf 静态拷贝 | 无 minify（可调试性优先）；canvas 胶水第二实例（独立线性内存）注释交代（72-78）|

---

## ② XSS 面审计表

全量搜索结果（rg 于 web/src + index.html + bench.html）：`innerHTML` **4 处**；`insertAdjacentHTML`/`outerHTML`/`document.write`/`eval(`/`new Function`/`srcdoc`/`DOMParser`/`createContextualFragment`/`javascript:` **0 处**。

| # | 位置 | 代码形态 | 输入来源 | 判定 |
|---|---|---|---|---|
| X1 | `pdf_view.ts:1741-1742` | `inkPreview.innerHTML = \`<polyline points="${pts.join(' ')}" stroke="${this.inkColor}" stroke-width="${this.inkWidth}"…\`` | `pts` = pointer 事件 → `convertToPdfPoint` → `toFixed(1)`（恒数字）；`inkColor` = `<select>` 六个固定 hex（593-609）；`inkWidth` = `Number(select.value)` ∈ {1,2,4} | **当前安全**（插值全为数字/固定枚举）。防御深度缺口：select 值若被既有脚本/DOM 污染可断属性——但彼时攻击者已有脚本权，无独立链 |
| X2 | `pdf_view.ts:1826-1829` | `svg.innerHTML = \`<polyline points="${pts.join(' ')}" … stroke="${strokeCss}" stroke-width="${ink.width}"…\`` | `strokeCss = rgb(${ink.color.r},…)`、`ink.width`、`pts` ← **wasm 会话 `AnnotateListed` JSON**（refreshAnnotations 1033-1041 → annotations → renderAnnotOverlay 1773-1834）；TS 声明 number 但 `JSON.parse` 无运行时校验 | **当前不可达、形态可直达的属性注入 sink**。v0 标注仅由本地 `addAnnotation` 构造（数值恒真）；一旦未来解析 PDF 文件既有标注（/C /BS /Contents）或 wasm 会话返回非数字字段，含 `"` 的字符串可终止 `stroke="` 属性并注入 `onload=` 事件属性（SVG 经 innerHTML 解析会执行 handler）→ 完整 XSS。**canvas_view 同场景用 createElementNS+setAttribute（811-831）——两模块纪律不一致** |
| X3 | `main.ts:2762` | `listEl.innerHTML = '<div class="toc-empty">暂无标题——用 H1-H3 或 # 前缀创建。</div>'` | 纯静态字符串 | **安全**（TOC 动态路径 2765-2773 用 replaceChildren+textContent，标题文本进 `item.textContent`/`item.title` 属性赋值 2769-2770——赋值不解析 HTML） |
| X4 | `main.ts:1652` | `parent.innerHTML = ''`（installNewCore 清空编辑器容器） | 无 | **安全** |
| X5 | 下载链五处：`main.ts:2326-2335`（docx）、`main.ts:3038-3047`（xlsx）、`xlsx_grid.ts:1137-1145`（台账导出）、`evidence_panel.ts:239-248`（body-clean/sidecar）、`pdf_view.ts:273-283`（annotated.pdf） | Blob + 临时 `<a download>` + click + revokeObjectURL | 文件名全部硬编码常量；`a.href` 全为 blob: URL | **安全**——无 DOM 派生文件名、无 `a.href` 字符串拼接（无 javascript: 面） |
| X6 | i18n 全链 `i18n/index.ts:42-47, 70-83` | `t()` 正则插值；applyStaticI18n 写 textContent/title/placeholder 三属性 | 词典静态 + 参数来自程序数值/文件字节计数 | **安全**（插值含 HTML 也只被 textContent 化；敌意文件名若进 `{parts}` 参数同样被安全化） |
| X7 | `index.html:468-479` | 内联主题防闪白脚本：localStorage → `documentElement.dataset.theme` | `mdc-theme` 值仅映射为固定 dark/light | **安全** |
| X8 | 其余全部动态渲染面 | canvas_view（setAttribute 数字插值 822-889, 1172-1213）、xlsx_grid（paintCell textContent 624-630；表头 632-682）、evidence_panel（buildCard 803-862 全 textContent + dataset）、empty_state（34-80）、bench（140-155）、导入警告面板（main.ts:1818-1836，注释明示「textContent-only……永不进 innerHTML」1815-1817） | 敌意 docx/xlsx/PDF/canvas 文本 | **安全** |
| X9 | 设置面板 `main.ts:3216-3322` | 动态 createElement + textContent + setAttribute(min/max/data-path) | 标签/hint 为代码内常量 | **安全** |

**XSS 总裁决**：**当前代码库无可利用的 XSS**。CM6 自身文本渲染安全；周边 UI 的 textContent/createElementNS 纪律一致性很高；敌意文档内容（docx/xlsx 单元格文本、PDF 元数据/警告 detail、文件名）在全部消费面上要么进 textContent、要么进 wasm JSON（不落 DOM）。唯一实质风险是 X2 的 innerHTML 属性插值模式——**未来接线即活化的潜伏 sink**，应在标注导入面落地前改写（P2，高度怀疑）。

---

## ③ 专题发现（C–J + B 补充）

### C. 撤销/重做前端接线
- 架构：**单栈**——CM6 history 刻意不装（main.ts:15-16 注释），`Mod-z / Mod-Shift-z / Mod-Shift-Z` keymap（666-671，大小写双绑定有 R9 审查注释）→ `core.undo()/redo()`（995-1005）→ `pushRender()` 全量重渲染。**无双栈冲突面**——这是本桥最干净的设计决策之一。
- `undo()` 恒 `return true`（998 注释：没有 CM6 history 可退）——无东西可撤也消费按键，防浏览器自身 undo 破坏 doc。设计偏好。
- pushRender 的 revision 守卫（968-984）+ 选区钳制（985-989：核驱动重渲染后 selection 塌缩单光标）——README §undo 已登记取舍。**多光标/跨块选区在 undo/redo/导入后丢失**（设计偏好）。
- 快路径 `byId.textChars` 不更新（已知 P0-4：210/317 建图、872-886 零维护、1144 消费）→ list Enter 拆块文本损坏。复核属实（见⓪）。
- fullResync 失败仍推 syncedRev（已知 P2-7：890-900）→ undo 在分叉态继续操作，历史与视图漂移不自愈；且推演出 pushRender 守卫被 syncedRev「假同步」欺骗后的免渲染连锁（见⓪）。
- **新发现 N16**：`__inject_chunk(first=false)`（main.ts:1679-1690）中 `core.extend_from_markdown(chunk)` **无 try/catch**——wasm panic/敌意 chunk 抛出后异常逸出事件监听器（uncaught error），`__ready=true`（1689）永不置位，宿主悬挂等待。全库其他 wasm 调用点皆有 typed 包裹，唯此一处裸奔。（已确认）
- 台账面 undo（xlsx_grid）为逐格 pop（854-870）：粘贴 4 格需 4 次 Ctrl+Z，无动作分组——设计偏好级（与编辑器「一次拖拽一项」的 canvas LWW 粒度对照）。

### D. 选区/光标同步
- wasm→CM6 无独立选区事件面：核驱动变更后 `pushRender` 只做 doc 全量重写 + 光标钳制（968-991）；handleEnter 新块定位 = 事件流物化新块 ID → `blockMap.lines.findIndex` → 行首+prefix（1168-1181），newId=0 时退化为 oldAnchor+1（1176，事件 JSON 损坏时的兜底，位置可能偏但文本正确）。
- 跨块选区：装饰 revealed = 严格重叠判定（771-772，`r.to > from && r.from < to`——紧邻不揭示）；`setSelections` 预校验整数/非负防 lineAt 越界炸开（744-757）。✅
- 只读切换（ingestion `locked`，523/827）：**事后回滚而非事前阻止**——CM6 未设 `EditorState.readOnly`，ingest 窗口内键入先落 CM6 doc、onDocChanged 早退不同步核，ingest 结束 pushRender 全量重写时静默丢弃（1586-1599）。半开窗口丢 ≤ingest 时长键入。（已确认，⑤N4）
- IME 期间 keymap `Enter/Backspace`（663-685）无 composition 守卫——@codemirror/view 6.36.5 的 keymap 在 composing 期是否短路 keydown 无法本地验证（node_modules 缺席）。真实 IME 链有 ime_cdp.py 覆盖但断言面是 op 流，未见 composition 中 Enter/Backspace 用例。（需外部环境验证，⑤N14）
- atomicRanges 双源（decoField hr block + plugin 视口原子缓存 617-659）+ 窗口外端点 ±2 行补算（621-657，R19 缺口自修补）——程序化跳转的原子协商覆盖完整。

### E. 文件导入链
- 路径：`file input change → arrayBuffer() → Uint8Array → wasm import_docx/import_xlsx → __inject 整核重建`（main.ts:1968-1975 docx、2136-2143 xlsx）；grid/pdf/evidence 独立开簿（xlsx_grid.ts:428-438、pdf_view.ts:689-695、evidence_panel.ts:430-447）。全部 `input.value=''` 允许同名重选。✅
- 错误呈现：wasm 异常（含 panic → JS 异常串）→ 双层 catch → JSON 解析 typed `{ok:false, code, message}` → 状态行红显（docxImport 1874-1891、xlsxImport 2015-2032，`code:'unknown'` 兜底）；启动竞态 `__inject` 缺席兜底 `not_ready`（1954-1960, 2100-2106）。**不向顶层抛出、编辑器现状零触碰——panic 不会白屏**。✅
- `#42` grid 联动：同一份 bytes fire-and-forget 二灌 Grid 会话（2125-2133），`__lastXlsxImport !== curImport` 身份守卫防旧异步回填覆盖新导入。✅
- **缺口**：前端无文件大小预检——500MB 文件直接 arrayBuffer 进 JS 堆（wasm 侧有 sheets_dropped 等上限，但内存峰值先发生在 JS）。oversized_web.py 覆盖 abnormal input 但量级 200KB。（P3，⑤N12）

### F. 文件导出链
- 下载实现统一 Blob + 临时 `<a download>` + click + revokeObjectURL（五处见②X5）。文件名全硬编码——**无 DOM 污染面、无可被敌意文档控制的下载名**。✅
- xlsx_grid 导出组装：editLog → `EditPayload[]`（`k:'empty'` 不带 v，1107-1111 契约形状）→ `export_with_edits(JSON)`；旧 pkg 鸭子探测 → typed `bridge_absent`（1100-1106）；stats 面缺失容忍降级（1114-1119）。
- docx 导出选项 `buildDocxExportOptionsJson`（2279-2315）：预设字符串 parse→stringify 往返 + toc/anonymize 开关合并（关闭态逐字节一致——纯加性设计有注释钉定）；`export_docx_styled` 运行时鸭子探测回退旧面（2372-2378）。
- **N8**：localStorage 种子 `seedDocxExportOptionsFromPersisted`（2258-2274）无值域校验 → 污染值（如 size='999' 或非数字）进 `docxExportOptions` → `Number()` → NaN → `JSON.stringify({a:NaN})` = `{"a":null}` → wasm serde 拒绝 → **导出恒失败且报错难懂**。`syncDocxExportOptionsToSettings`（2240-2255）经 SETTING_VALIDATORS 只保护快照，不清洗面板态本体。（已确认，推演）

### G. 状态机与生命周期
- `__ready`：boot 主路径尾 1747；`mdc-inject`/`mdc-inject-chunk` 各自置位（1677/1689）——chunk 异常路径见 N16。`__gridReady/__pdfReady/__canvasReady/__evReady/__collabReady` 独立旗标 + 各自 e2e SKIP 门。✅
- wasm init 单飞门（wasm_boot.ts:31-44）覆盖全部五个调用点（main boot / canvas / collab / evidence / grid + pdf 独立门），失败重置可重试。✅
- `installNewCore`（1649-1674）：旧 view.destroy + 容器清空 + window 面重装 + `mdc-core-reinstalled` 事件（状态栏 dirty 重基线 2681-2686 已防御「导入即未保存」复活缺陷）。✅
- **N2（已确认）**：设置面板 `openSettingsPanel`（3216-3322）只 remove 旧 `#settings-panel`（3217）**不删旧 `#settings-overlay`**——重复点设置按钮叠出空 overlay 遮屏（点击空白才消）；且 `document.addEventListener('keydown', esc)`（3322）仅在 Esc 关闭路径解绑（3320），经关闭按钮（3283）/点空白（3288）关闭后监听器 + 已分离 overlay 子树泄漏累积（每开一次叠一个）。
- 页面卸载：beforeunload + pagehide 双路草稿终刷（draft.ts:201-221）✅；BroadcastChannel / intervals / rAF 泵无显式清理——浏览器页面卸载自动回收，但**宿主壳（wry）若复用 webview 跨会话**则永续：canvas rAF 泵每帧 2 次空 FFI（canvas_view.ts:366-370，代码自知「空转 = 两次空 FFI」）+ collab 40ms 泵（collab_channel.ts:466）。（P3，⑤N9）
- **N11（已确认）**：设置面板 9 键中 **6 键无任何消费方**（undo_coalesce_ms / progressive_open_threshold_bytes / ai.commands_enabled / meta_sidecar / autosave_enabled+interval / appearance.font_size_px）——写入仅落 JS 快照（1464-1511 无核侧转发）；渐进开档实际由 `?chunks=` URL 参数决定（1545）而非阈值键；编辑器字号实际由 Ctrl+= 的 `--ed-font-size` 路径决定（2940-2983），`appearance.font_size_px` 无 boot 种子接线。仅 theme/language/session_restore 三键真实生效。代码注释承认待宿主 R24-d 接线，但 UI 未作 inert 提示——面板呈现"可生效"控件实为摆设。
- collab disconnect/reconnect 幂等（395-397）+ transport 钩保留重连复用（386-394）✅；canvas 恢复用独立 client 段位防 ID 撞车（RESTORE_CLIENT_BASE 1415-1417）✅。

### H. 大文档路径（1MB+）
- 已到位的虚拟化：CM6 viewport ✅；装饰视口惰性 ±DECO_PAD 行（578-593，0.8ms 实测注释）✅；渐进 ingest 分片建图 + `{absorbed:N}` 事件聚合（918-936，防 20 万 op 进 JS 堆）✅；pushRender revision 守卫免 O(n) 重渲染（968-984）✅；buildBigChunks 流式切片免全量字符串物化（1246-1271）✅。
- **N1（已确认）**：`scanHr` 在**每个 docChanged 事务**全 doc 逐行扫描（decoField.update 563 → scanHr 537-544 `for ln=1..doc.lines` 逐行正则）——代码自认「~2-4ms@1MB，R19 候选：按 changes 受影响行区间增量维护」（535-536 注释）**未修**；10MB（R16 终态目标）线性外推 20-40ms/键击，直接侵蚀 R16 打字路径收益与渐进 TTI 的意义。
- **N10（高度怀疑）**：状态栏 500ms 轮询 `core.word_count_json()`（2639-2642, 2668）——若 Rust 侧无缓存则每 500ms 全 ropey 遍历，10MB 档周期性主线程负担。需外部环境验证。
- TOC 500ms 轮询有 hidden 门（2756 早退）✅；面板开启时 `__blocks()` 全量 dump+parse（2757）可接受（低频查看）。
- **N13（已确认）**：查找面板 `collect()` 每次 input `doc.toString()` 物化全文字符串 + 全文 indexOf（2811-2823）——10MB 文档每键 10MB 字符串分配（仅在查找面板激活时）。
- `tryFastPath` 多行块 textOffset 平移 O(lines)（877-885）——单一巨型 code_block 内连续打字每键线性扫 lines 数组（边缘场景）。
- **N3（已确认）**：`window.__mdcoreEvents` 事件数组**无上限增长**（drainEvents 799-807 push 无 cap；仅整核重装清零 1583/1662）——生产路径挂无界探针数组，长编辑会话线性吃内存。
- xlsx_grid `effectiveDims()`（496-506）每次 render/keydown 遍历全 editLog（上限 10 万条）——O(edits) 每键，台账满时 10 万次循环。边缘。

### I. i18n
- 解析序单一真源 `initI18n`（index.ts:31-34）：设置键（auto/en/zh-CN）优先 → navigator.language 前缀 zh → zh-CN，否则 en；en 兜底 → 键名（42-47，漏译开发期可见）。语言键 localStorage 镜像重载存活（main.ts:1515-1524）✅。
- 切语言 = `location.reload()`（main.ts:1535-1540）——全页重建避免逐组件重渲染状态漂移（设计偏好，注释「设置变更低频，刷新 = 诚实简单」）；reload 会触发 beforeunload（draft 拦截在场 draft.ts:216-221，未保存稿有兜底）。
- **无 HTML 注入面**：词典值只进 textContent/title/placeholder（index.ts:70-83）；`t()` 插值即使参数含 HTML 也只会被消费方 textContent 化。✅
- **N7（已确认）**：i18n 覆盖不完整——main.ts 硬编码中文残留：refreshHint 三分支（2430-2434）、SETTINGS_FIELDS 全部标签/提示（3196-3205）、设置面板标题/aria（3224-3226）、关闭钮「关闭 (Esc)」（3282）、设置状态文案（3303-3311）、toc-empty（2762，硬编码于 innerHTML 内）；pdf_view 硬编码英文状态（652-653、1934、1944-1945、2065）；canvas_view 硬编码英文状态（1036、1230、1333、1341、1359、1380 等）。**en locale 下设置面板全中文；zh locale 下 pdf/canvas 状态行全英文**——双语面均有破窗。i18n_ui.py 断言面未覆盖这些残留。

### J. 竞态与重入
- updateListener↔drainEvents 回调环：`syncing` 布尔双段守卫——onDocChanged 入口（826-848）+ pushRender dispatch 包裹（968-984 try/finally 复位）——同步 dispatch 重入被正确短路；pushRender 的 selection-only dispatch（985-989）在 syncing=false 后执行但不触发 docChanged。✅
- `mutate()` finally drainEvents（789-796）保证事件不滞留核侧。✅
- okDimTimer 逐次重置 + text 守卫防旧定时器污染新状态（1937-1942、2084-2088、2350-2354、3062-3066 四处同构）——竞态意识好。✅
- `gridOpenBytesWhenReady` 有界等待（xlsx_grid.ts:1241-1253，25ms 线性重试 + 5s typed not_ready 而非悬挂）。✅
- collab：remote/maintenance 不回发（canvas_view.ts:600、collab_channel.ts:166-169）+ PUMP_MAX_ROUNDS=8 防御性上限（183）——回声环双保险。✅
- 已知 P2-7 本质即「失败后竞态不自愈」（见⓪）。
- **N15（已确认，结构）**：boot 渐进 `ingestExtend` 的 `take_events` 异常分支**提前 return**（main.ts:919-930 catch → return）——跳过 `blocks_json_range` 增量建图，blockCount 游标永久落后；后续 chunk 从旧游标 extend → map 与核持续错位（罕见触发：wasm 事件面抛错；后果与 P2-8 同族）。
- status bar dirty 检测 `mdc-core-reinstalled` 同步锚定（2681-2686，防 500ms 轮询窗内编辑不可见——注释记录实测复现与修复）。✅

### B. IME composition（补充实证）
- 桥无 composition 专用代码：预编辑 update 经 CM6 MutationObserver → 每步一个事务 → 快路径逐个 `replace_text`（861 `inserted.includes('\n')` 拒跨行；864 crossline 拒）；undo 粒度依赖核侧 1s 合并窗（`undo_coalesce_ms:1000` 默认 1451；每变异前 `set_clock_ms(Date.now())` 789-790 注入，wasm32 无时钟——宿主是唯一时间源）。
- **>1s 的长组合会被切分为多条 undo**——ime_cdp.py 断言「1s 合并窗把整段组合收进单个 undo 条目」仅对窗内组合成立（边界语义，非缺陷但应知）。
- 合成取消（空 compositionend）→ CM6 回滚预编辑 = 单删除事务 → 快路径收敛。跨行提交触发兜底 sync（结构性 op 不并条，undo 分次回退——ime_cdp docstring 钉定）。✅
- 覆盖：W3/W9 合成流 + ime_cdp.py 真实 CDP IME（Input.imeSetComposition/insertText，含逐事件日志/op 流/undo 深度证据落 results_ime.json）；**组合中途 Enter/Backspace 的 keymap 交互未见用例**（N14）。

---

## ④ 测试覆盖面对照（K 项）

29 个 .py = **24 个 e2e 套件** + 4 个 one-off 探针（probe_gate2 / probe_ime_cdp / diag_heap / diag_heap_cdp）+ 1 个夹具生成器（gen_pdf_fixture）。**全部 serve web/dist → 全部依赖真实构建产物**（本沙箱不可复现，静态清单如下）：

| 套件 | 覆盖目标（docstring 摘要） | 缺口提示 |
|---|---|---|
| web_smoke | W1 语法隐藏 / W2 光标协商 / W3+W9 合成+深度 IME / W4 1MB 打字性能 / W5 块 op / W6 核 undo / W7 渐进开档+建图等价 / W8 长事务压力 / W10 大文档块 op / W11 AI 闭环 / W12 装饰增量等价 / W13 非 BMP 单位宪法 | W4 只到 1MB；scanHr/word_count 在 10MB 每键成本无锁 |
| contract_gate | CONTRACT v1 + 设置面 v1.1 契约锁（桥形状/默认值表） | 只锁形状，不锁设置键实际生效（N11 漏网） |
| editor_toolbar | 格式工具条 + 快捷键 + `__format` 程序化命令同源 | — |
| empty_state | 三表面空态引导卡 | — |
| i18n_ui | zh 全中文面 / en 零回归 / 静态属性三面 | 未覆盖设置面板/refreshHint/toc-empty 等硬编码残留（N7 漏网） |
| hostile_ui | 空文档 / 200KB 超大 / emoji-CJK-BIDI / 面板叠加 / 连击 20 次 / localStorage 字号污染 | **无 HTML 注入负测**；localStorage 污染只测字号（N8 漏网）；200KB ≠ 10MB |
| ime_cdp | 真实 CDP IME 组合流 + op 流 + undo 深度证据 | 无组合中 Enter/Backspace 用例（N14） |
| tr1_web_e2e | CRLF 注入 → `__md()` 投影零 CR + 视图一致 | — |
| crash_recovery_web | beforeunload 拦截 + localStorage 草稿恢复闭环 | — |
| stress_web | 四场景真实并发压力 | — |
| oversized_web | 超大/异常输入/超时/资源耗尽 | 前端导入大小预检（N12）未触发 |
| lifecycle_web | 退出 / 重启 / 断连 / 恢复 | — |
| concurrent_web | 并发修改 / 过期命令 / 撤销重做 / 删除复原 | — |
| docx_import / docx_export | office-io 读/写面往返 + 导入后可编辑性 + 契约面 | — |
| xlsx_import / xlsx_export / xlsx_export_openpyxl | xlsx 往返 + openpyxl 独立校验 | — |
| xlsx_grid | 三层虚拟化 / 台账 commit-undo-delete-paste / copy TSV（真实剪贴板权限）/ evidence 高亮 / 换表 | **copy TSV 只测保真，无 `=cmd` 公式注入负测**（P0-5 验收缺口） |
| evidence_panel / sidecar_faults_web | 工作台 web 面 + sidecar 故障路径 | — |
| pdf_view | canvas/textLayer/搜索/标注/增量保存（wasm SKIP 门） | 无敌意 PDF 标注 JSON 注入测（X2 未来面漏网） |
| canvas_view | SVG 分层渲染 / 拖拽 / 双实例收敛 / 快照往返 | — |
| integration_journey | 四表面真实使用路径串联 | — |

**覆盖缺口汇总**（按对应新问题）：P0-5 剪贴板注入负测缺失；X2 未来 XSS 面无测；N1/N10 大文档每键 O(doc) 无性能锁；N2 设置面板重复打开无测；N16 `__inject_chunk` 异常路径无测；N6 非安全上下文无测；N7 i18n 完整性锁缺（可静态 grep 锁）。

---

## ⑤ 新问题清单（P0–P3 分级 + 置信，逐条展开）

> 已知问题 P0-4 / P0-5 / P2-7 / P2-8 复核属实（见⓪），不计入本表。**本轮无新 P0 / 无新 P1**。

### P2 级（3 项）

**N1 — scanHr 全文档逐行扫描挂每个 docChanged 事务（大文档打字税）**
- 证据：`main.ts:563`（decoField.update：`if (tr.docChanged || this.hrIdx === null) this.scanHr(tr.state)`）→ `main.ts:537-544`（`for (let ln = 1; ln <= doc.lines; ln++)` 逐行正则 `/^(---|\*\*\*|___)\s*$/`）。
- 代码自知：`main.ts:535-536` 注释「打字路径 ~2-4ms@1MB，R19 候选：按 changes 受影响行区间增量维护」——**登记未修**。
- 量级推演：10MB（R16 终态目标）≈ 20 万行 → 线性外推 20-40ms/键击，配合 CM6 输入即视感直接破坏打字流畅性；R16 的渐进 TTI 收益被后续每键成本侵蚀。
- 置信：结构**已确认**；10MB 实测毫秒数**需外部环境验证**。

**N5 — pdf_view Ink 标注渲染 innerHTML 属性插值（潜伏 XSS sink）**
- 证据：`pdf_view.ts:1826-1829`：`strokeCss = rgb(${ink.color.r}, ${ink.color.g}, ${ink.color.b})` 与 `stroke-width="${ink.width}"`、`points="${pts.join(' ')}"` 全部插值进 `svg.innerHTML`。
- 数据源链：`refreshAnnotations()`（1033-1041）→ `JSON.parse(this.session.execute(...))` → `this.annotations` → `renderAnnotOverlay`（1773-1834）。TS 类型声明 number，**`JSON.parse` 无运行时类型校验**。
- 攻击形态：若 wasm 会话返回 `color.r = 'red" onload="alert(1)'`（字符串），插值产出 `stroke="rgb(red" onload="alert(1)"..."` → 属性断裂 + `onload` 事件属性 → SVG 经 innerHTML 解析时执行 handler → 完整 XSS。
- 当前不可达性：v0 标注仅由本地 `addAnnotation`（1051-1069）构造，inkPayload 的 r/g/b 经 hex 正则解析（1760-1763）恒为数字。**活化条件**：未来「从 PDF 文件解析既有标注」（flush/attach_writer 面已预留）或 wasm 会话返回敌意 JSON。
- 一致性对照：`canvas_view.ts:811-831` 同类 Ink/polyline 渲染用 `createElementNS + setAttribute + textContent`——**零此面**。两模块纪律不一致即改写范本。
- 置信：**高度怀疑**（v0 不可达、形态可直达）。

**N10 — 状态栏 500ms 轮询 word_count_json 疑 O(doc)**
- 证据：`main.ts:2639-2642`（`JSON.parse(core.word_count_json())` 七字段）+ `main.ts:2668`（`window.setInterval(refresh, 500)`）。N-3 注释说明这是「引擎口径单一真源」改造（JS 不再自行数数）——正确性换性能。
- 若 Rust 侧每次全 ropey 遍历：10MB 文档 = 每 500ms 一次 O(doc) 主线程 FFI 阻塞。
- 置信：**需外部环境验证**（Rust 侧是否缓存词数；本沙箱无 wasm 产物）。

### P3 级（15 项）

**N2 — 设置面板 overlay 叠加泄漏**（已确认）
`main.ts:3217` 只 `remove()` 旧 `#settings-panel` 不删 `#settings-overlay` → 重复点设置钮叠出空 overlay 遮屏；`main.ts:3322` 的 `document.addEventListener('keydown', esc)` 仅在 Esc 路径解绑（3320），关闭钮（3283）/点空白（3288）路径泄漏监听器 + 分离 DOM，每开一次叠一个。

**N3 — __mdcoreEvents 无上限增长**（已确认）
`main.ts:799-807` drainEvents push 无 cap；仅整核重装清零（1583/1662）。生产路径挂无界探针数组，长编辑会话线性内存增长（每 op 事件 ~百字节，万字级编辑即 MB 级滞留）。

**N4 — locked ingestion 事后回滚非事前阻止**（已确认）
`main.ts:523`（locked 字段注释「期间拒绝编辑」）+ `main.ts:827`（onDocChanged 早退）+ `main.ts:1586-1599`（ingest 窗口内置 true）。CM6 未设 `EditorState.readOnly`——键入先落视图再被 pushRender 全量重写静默丢弃，用户无反馈。

**N6 — crypto.randomUUID 依赖 secure context**（API 语义已确认 / 触发需外部环境验证）
`xlsx_grid.ts:823`（`crypto.randomUUID()` 为台账记录 id）。非安全源（http 非 localhost）下该 API undefined → 编辑首键 TypeError。e2e 全部 localhost（豁免）；wry 自定义 scheme 的安全性需宿主侧确认。

**N7 — i18n 双语破窗**（已确认）
硬编码中文：main.ts:2430-2434（refreshHint）、2762（toc-empty）、3196-3205（SETTINGS_FIELDS 全部）、3224-3226（面板标题/aria）、3282（关闭钮）、3303-3311（设置状态）。硬编码英文：pdf_view.ts:652-653、1934、1944-1945、2065；canvas_view.ts:1036、1230、1333、1341、1359、1380。en locale 设置面板全中文 / zh locale pdf+canvas 状态行全英文。

**N8 — localStorage 污染导出选项致导出恒败**（已确认，推演）
`main.ts:2258-2274` seed 无值域校验 → 污染 size/spacing/margin 进 `buildDocxExportOptionsJson`（2279-2315）→ `Number()` NaN → `JSON.stringify` 产出 `null` → wasm serde 拒绝。`syncDocxExportOptionsToSettings`（2240-2255）校验器只保护快照不清洗面板态。hostile_ui.py 的 localStorage 污染组只测字号键。

**N9 — 空闲 FFI 常耗**（已确认，代码自知）
canvas rAF 泵 60Hz×2 空 `take_events_json`（canvas_view.ts:366-370，注释「空转 = 两次空 FFI」）+ collab 40ms 泵永续（collab_channel.ts:466）+ hookTimer 50ms×30s（449-463）。页面卸载由浏览器回收；宿主壳复用 webview 跨「会话」时无清理点。

**N11 — 设置面板 6/9 键为摆设**（已确认）
消费方核查（rg 全量）：仅 theme（setupThemeControl 3137-3169）、language（seedLanguageSetting 1517-1524 + reload 1535-1540）、session_restore（draft.ts:78）三键生效。undo_coalesce_ms / progressive_open_threshold_bytes / ai.commands_enabled / meta_sidecar / autosave_enabled+interval_ms / appearance.font_size_px 六键写入仅落 JS 快照（1464-1511）无核侧转发；渐进开档实际由 `?chunks=` URL 参数决定（1545）；字号实际由 Ctrl+= 的 `--ed-font-size` 决定（2940-2983），`appearance.font_size_px` 无 boot 种子。代码注释承认待宿主 R24-d 接线，但 UI 未作 inert 提示。

**N12 — 导入无大小预检**（已确认，结构）
`main.ts:1968-1975 / 2136-2143`：`await file.arrayBuffer()` 直接全量进 JS 堆；wasm 侧有 sheets_dropped 等截断，但内存峰值先到 JS。

**N13 — 查找面板全文字符串物化**（已确认，结构）
`main.ts:2811-2823`：`view.state.doc.toString()` + 全文 indexOf，每次 input 触发。10MB 文档每键 10MB 字符串分配（仅查找面板激活时）。

**N14 — IME 组合期 keymap 交互未证**（需外部环境验证）
`main.ts:663-685` keymap Enter/Backspace 无 composition 守卫；@codemirror/view 6.36.5 composing 期 keydown 短路行为无法本地验证（node_modules 缺席）。ime_cdp.py 无组合中 Enter/Backspace 用例。

**N15 — ingestExtend 异常分支游标永久落后**（已确认，结构 / 触发罕见）
`main.ts:919-930`：`take_events` 抛错 → catch 内 `return` → 跳过 `blocks_json_range` 增量建图 → blockCount 停滞 → 后续 chunk 从旧游标 extend 持续错位（与 P2-8 同族后果）。

**N16 — __inject_chunk(first=false) 裸 wasm 调用**（已确认，结构）
`main.ts:1684-1688`：`core.extend_from_markdown(chunk)` 无 try/catch——全库唯一未 typed 包裹的 wasm 调用点。异常逸出事件监听器（uncaught）且 `__ready=true`（1689）永不置位 → 宿主悬挂。一行 try/catch 即闭合。

**N17 — addNoteAt 用 window.prompt**（需外部环境验证）
`pdf_view.ts:1658-1659`：Note 文本输入走原生 prompt——wry/webkit2gtk 宿主壳的 prompt 实现需确认（缺则 Note 流程死路）。

**N18 — 无 CSP meta**（设计偏好/加固建议）
index.html 全文无 Content-Security-Policy。当前无注入面；X2 类潜伏 sink 落地时无第二道闸。wry 宿主可由壳层注入 CSP 头替代。

---

## ⑥ 正面工程实践确认（无需修改）

1. **wasm 错误 typed 包裹纪律全库一致**：每个模块一份 `wasmFail()` + 双层 catch + 状态行红显——除 N16 一处例外。panic/trap 不会白屏（E 项验收通过）。
2. **textContent 渲染纪律**：敌意文档文本（docx 段落/xlsx 单元格/PDF 警告 detail/文件名）在全部消费面无 DOM 解析路径（②X8/X9）。
3. **wasm 单飞门**（wasm_boot.ts）根治 wasm-bindgen `__wbg_init` 并发双实例化 SEGV——注释含 chromium headless 实证（signal 11 落 JIT wasm 帧）。
4. **R17 单位宪法**：UTF-16↔码点换算集中 `utf16ToCp/cpToUtf16` 两函数（main.ts:93-114），全桥无散落 `[...s].length`（BlockMap 内 cpLen 委托 173/699-701）；W13 语义锁。
5. **草稿系统**：信封版本化（读侧拒旧版不猜结构 100-108）+ 三重激活门（URL 参数阻断协同/夹具污染 69-72）+ pagehide 同步终刷兜底 beforeunload 被吞 + 超限清稿不留死键（111-119）。
6. **pdf_view 渲染纪律**：renderToken 防错序（1396/1428/1433）、单页渲染串行链防同 canvas 并发 render（1373-1380）、L2 像素钳镜像引擎侧限制（1397-1415）、LRU 驱逐含孤立超预算条目（319-330 审查 S-2 修复）。
7. **下载链统一**：五处同构 Blob+a[download]+revoke，文件名硬编码——无 DOM 污染面。
8. **竞态意识**：syncing 双段守卫、okDimTimer text 守卫四处、curImport 身份守卫、collab 回声环双保险（J 项）。

---

## ⑦ 裁决建议（含修复成本估计）

1. **XSS 裁决：当前无可利用 XSS，安全面放行**。但 X2（pdf_view.ts:1826-1829 innerHTML 属性插值）应作为**标注导入面（PDF 既有标注解析）落地的前置阻断项**改写为 createElementNS + setAttribute（canvas_view.ts:811-831 即现成安全范式，改动 <20 行，无行为面变化）；顺带收编 1741 预览路径。
2. **P0-5（已知 onCopy 公式注入）维持最高修复优先级**：消毒函数接线是唯一直接可达的敌意数据外流面；同步补一条 copy 注入负测进 xlsx_grid.py（`=cmd|...` 前缀断言消毒后形）。
3. **N16 一行 try/catch 即闭合**（异常→typed 拒绝→`__ready` 仍置位或改语义）——宿主契约面健壮性洞，修复成本最低收益明确。
4. **N1（scanHr O(doc)/键击）与 N10（word_count 轮询）是大文档产品化的两个真实税**：N1 按代码内 R19 候选方案（受影响行区间增量维护 hrIdx）排期；N10 先在 Rust 侧确认缓存，无则状态栏轮询加 revision 门（rev 未变跳过重算）。
5. **N2（设置面板 overlay 泄漏）与 N11（6 个摆设设置键）**：前者改 `#settings-overlay` 整体 remove + esc 监听器统一解绑；后者给无消费键加 disabled + 「host-wired only」提示，或收敛面板至 web 态真实生效三键（theme/language/session_restore）。
6. **测试补位优先序**：① 剪贴板注入负测（P0-5 验收）；② `__inject_chunk` 异常路径；③ 10MB 打字性能锁（N1/N10 回归护栏，W4 扩档）；④ 设置面板重复开/关循环（N2）；⑤ i18n 完整性静态断言（N7，grep 锁即可）。
7. **总体评价**：本轮**无新 P0/P1**。新发现集中在防御深度（N5）、生命周期卫生（N2/N3/N16）与大文档性能税（N1/N10），均次级于既有 P0-4/P0-5/P2-7/P2-8。桥核心（快路径/兜底/undo/IME/单位宪法/装饰双源）在静态审读下语义自洽，注释与实现一致性好——这是一份**注释可信度高**的代码库（多处注释主动登记已知边界与审查修复编号），静态审计因此收益很高。

---

## 附录：审计方法与局限

- 逐文件全量深读：main.ts 分 5 段（1-400/400-849/849-1298/1298-1797/1798-2297/2298-2797/2798-3348）、pdf_view.ts 分 4 段、canvas_view/xlsx_grid/evidence_panel 分 2-3 段，其余单遍；i18n 词典抽查键面完整性。
- rg 全量面搜索：innerHTML 族、eval/new Function 族、srcdoc/DOMParser/createContextualFragment、javascript:、Blob/createObjectURL/download、setInterval、addEventListener、composing（node_modules 缺席未能查 CM6 内部）。
- 局限：① 沙箱不可构建（node_modules/pkg 缺席）——CM6 6.36.5 keymap-composition 交互（N14）、wasm 侧 word_count 缓存（N10）、wry secure context（N6）三处只能标「需外部环境验证」；② 行为语义依赖 Rust 侧（如空 list_item 连续 Enter 的核行为、sync_from_markdown 差分质量）不在本轮范围；③ 测试套件只读 docstring 与关键 rg 探针，未逐用例核对断言强度。
