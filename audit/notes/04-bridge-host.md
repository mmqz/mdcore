# 04 — Markdown/Web 桥与宿主壳体系深度审计（只读）

- 审计对象：`main@96596d7`（Merge PR #214 perf/205-incremental-fence）
- 范围：`crates/mdcore-wasm`（10.3K LOC）、`web/`（13 ts ≈11.4K LOC + 35 py ≈11.6K LOC）、`crates/mdbridge`、`crates/shell`、`crates/shell-native`、`crates/settings`
- 方法：纯静态（Read/Grep/git log），未运行 cargo/npm/浏览器（纪律约束）；所有结论附 文件:行号 证据链
- 参照：`audit-notes/loc-stats.md`、`gates.md`、`CONTRACT.md`（冻结 v1.6）、`web/README.md`

---

## 0. 结论速览

**web 桥（CM6 + mdcore-wasm）是 mdcore 唯一现役产品路径**：mang-shell 不依赖 mdcore（独立 workspace + Zed text::Buffer 换芯），shell-native 是冒烟级骨架，mdbridge 无生产消费者。桥的同步协议总体严谨（逐事务同步收敛、单栈核驱动 undo、单位宪法单一换算对），但存在 **1 个 P1 数据损坏级缺陷**（BlockMap.byId.textChars 快路径陈旧 → list item 内 Enter 错误切分）、2 个 P2 分叉/退化窗口、若干 P3。测试矩阵（35 个 Playwright 真浏览器脚本）行为级断言强度高于业界平均，但恰好未覆盖 P1 的触发序列，且结果证据 JSON 不入库、无法仓内复核。

问题分级汇总：

| 级别 | 编号 | 一句话 | 置信 |
|---|---|---|---|
| P1 | B-1 | 快路径后 `byId.textChars` 陈旧，list_item 行内 Enter 用陈旧 end 切分 → 尾文本重复/错位（数据损坏） | 高（代码级推导；未实测复现） |
| P2 | B-2 | `fullResync` 失败仍推进 `syncedRev` + 用旧树重建 BlockMap → 永久分叉窗口（触发面罕见） | 高 |
| P2 | B-3 | 宿主渐进通道 `__inject_chunk(first=false)` 走 `pushRender(skipMap)` 不增量建图、不设 `locked` → 追加区每键 O(doc) 兜底 | 高 |
| P2 | B-4 | mdcore-wasm 无 `console_error_panic_hook`；`apply_ops` backup 回滚仅覆盖 Err 路径 → panic 后核处于半改状态且无诊断 | 高（可达面≈0，后果严重度低频） |
| P3 | B-5 | 孤立 surrogate 经 wasm-bindgen TextEncoder → U+FFFD，view/core 一字符差至下次全量重渲染 | 中 |
| P3 | B-6 | `drainEvents`/`ingestExtend` 静默吞 JSON 异常（事件流证据断流、增量建图停追） | 高 |
| P3 | B-7 | apply_ops 每事务整树 `tree.clone()`（backup）——10MB wasm 425MB 高水位主驱动（R22-b 已登记未治理） | 高（gates.md R22-b 同结论） |
| P3 | B-8 | `__save`/`mdc-save` 无宿主时为无操作保存（假保存）；TOC 重名跳首等 v1 限制 | 高 |

---

## 1. 三条宿主路径的裁决（问题 6）

### 1.1 活跃度与依赖事实

| 路径 | 规模 | git 活跃度（2026-09-01 后） | 最后提交 | 依赖 mdcore? |
|---|---|---|---|---|
| web/（CM6+mdcore-wasm） | 13 ts / 11.4K LOC | **57 commits**（web/ 全目录） | main.ts 2026-10-28 | 是（wasm 桥） |
| mang-shell（GPUI） | 43 rs / 33.8K LOC | **106 commits** | 2026-11-04（309a24a，全仓最新） | **否** |
| crates/shell-native（tao+wry） | 2 rs / 568 LOC | 6 commits | 2026-10-26（da3e85e issue #4 平台门） | 是（经 mdshell） |
| crates/mdbridge | 3 rs / 1037 LOC | 2 commits | 2026-10-22 | 是（Op 翻译层） |

证据链：
- mang-shell 有**自己的 Cargo.lock**（`ls mang-shell/Cargo.lock` → 存在），不在根 workspace members（`Cargo.toml` members 列表无 mang-shell）；其依赖 = `mdc-office-io`、`mdc-pdf`、`mdc-canvas`、Zed `gpui/gpui_platform/theme/ui/zlog/text`（`mang-shell/Cargo.toml`），**没有 mdcore/mdshell/mdbridge**。
- mang-shell 编辑器缓冲 = Zed `text::Buffer` 换芯（`mang-shell/src/editor/buffer.rs:1-14`「r42 阶段 A 换芯：存储层 Zed text::Buffer」），自带 undo/查找/光标语义——与 mdcore 块树模型**平行无关**。
- shell-native 非纯蓝图但仅冒烟级：`crates/shell-native/src/lib.rs:59-67`「范围纪律：仅为实机连通性服务（cargo check + Xvfb 冒烟）——对接点清单 1–5 的完整壳功能不在 s5a-env 范围」。实体面 = `run_smoke`（看门狗+探针导航回传，lib.rs:248-366）+ `mdapp://` 静态服务含目录穿越防护（lib.rs:189-242）+ `WryHost::surface_load_markdown` 最小注入（lib.rs:119-130）。**没有**文件对话框、保存接线、进度条 GTK、多窗口（README.md:45 自认「shell-native 当前为蓝图态」）。
- README.md:8 平台矩阵自述：`平台 = 引擎核心（Rust crates）| wasm（mdcore-wasm + web）| 桌面壳（shell-native）`——即官方产品路径 = web 桥 + （未来的）shell-native 桌面壳。

### 1.2 裁决

1. **现役产品路径 = web CM6 + mdcore-wasm**：契约冻结（CONTRACT.md v1.6 + `web/tests/contract_gate.py` 双侧锁）、测试矩阵最全、目录活跃度第二高、README 平台矩阵背书。
2. **mang-shell = 最活跃但属另一条产品线**（office/PDF/Canvas 套件，Zed 系编辑器），**不构成 mdcore 的宿主**——任务假设的「三条宿主路径」实为「mdcore 一条（web）+ 另一产品一条（mang-shell）+ 骨架一条（shell-native）」。这对「mdcore 块树核心是否有第二消费面」是重要否定证据。
3. **mdbridge 职责与消费者**：`translate.rs` 把 **mdcore 原子 Op 流（NodePath + u64 BlockId + char 域）翻译为 mdc-collab 命令流（block_id 字符串寻址）**，`host.rs` 的 `MdCollabHost` 持 shadow BlockTree 投影 + family(`md:{client}:{n}`)/foreign 双向身份映射（lib.rs:23-40, 154-171）。唯一消费者 = `crates/workbench`（`Cargo.toml:38`，`tests/collab_composition.rs` + `examples/collab_body_demo.rs`）——**测试/示例级，无生产宿主**。web 侧协同 harness（`collab_channel.ts`）直接用 `WasmCollabSession`（mdc-collab 原生），**未经 mdbridge**——即 CM6 文档与协同会话的双向翻译在生产上尚未接线。
4. **shell（mdshell，480 LOC）真实消费者**：`shell-native`（冒烟）+ 自身 4 个测试文件（`contract_freeze.rs` 词面锁、`r24_packaging/r24_session/r24_settings.rs`）。它是「headless 可验证契约层」——`Host` trait + `open_document/open_document_progressive/save_document`（lib.rs:71-128）+ `SessionManager`（session.rs:68-208，一窗一档/上限 16/MRU 焦点序）。
5. **settings（958 LOC）**：域无关四层 schema + TOML sidecar，被 mdshell `pub use` re-export（shell/src/lib.rs:18），web 侧 `__settings/__setSetting` 与其同表校验（main.ts:1466-1490）——双侧一致性由 `contract_gate.py` settings_* 检锁。

---

## 2. 问题 1：双重权威状态与分叉路径

### 2.1 权威声明与同步协议

- 权威 = **Rust 核**（web/README.md:5「单一真理在 Rust 核（主档 §9）；CM6 doc = core.to_markdown() 的镜像」；mdcore-wasm/src/lib.rs:3-5 同义）。
- 协议 = **逐事务同步（无异步窗口）**：CM6 updateListener → `onDocChanged`（main.ts:826-848）→ 单 change 且行内 body 区 → `tryFastPath`（858-887，`core.apply_ops` 同步调用）；否则 `fullResync` → `core.sync_from_markdown`（890-900）。核→view 全量回写仅在核驱动命令（undo/redo/Enter 分段）后经 `pushRender`（968-991），以 `syncing` 标志防回环（827）。
- 事件 = 拉取式（`take_events`，mutate() finally 排空，main.ts:789-807）；revision 守卫（`syncedRev`）防无谓重渲染（970）。

**逐事务同步意味着常规路径不存在「短暂不一致窗口」**——每个 CM6 事务在同一 JS tick 内落核。真正的分叉只出现在以下路径：

### 2.2 分叉路径枚举（含共同损坏）

**B-1（P1，数据损坏；注意这是 view/core「共同损坏」而非分叉）**：见 §4 详述——快路径成功后 `blockMap.byId.textChars` 不更新（唯一写点 = rebuild/extend：main.ts:210/317），而 `handleEnter` 用它作 ReplaceText 的 `end`（main.ts:1144）。

**B-2（P2，永久分叉窗口）**：`fullResync` 捕获 `sync_from_markdown` 异常后仅 console.error（main.ts:893-896），但**仍执行** `rebuildMap(docText)`（用核内**旧树**的 `blocks_json()` 对照 view **新文本**重建行记录——行错位）并 `syncedRev = revision()`（899）——rev 未变 ⇒ 后续 `pushRender` 守卫恒跳过（970）⇒ doc≠core 的分叉**不会自愈**，直到下一次编辑触发成功 sync 或 undo。触发前提 = `sync_from_markdown` reject——其 diff 自算 range（mdcore lib.rs:795-859，按位置朴素差分，ReplaceText end 取自现树 len_chars），实际 reject 面≈0（防御级）。**低概率、高后果（静默永久分叉）**。
- 修复建议：catch 分支不推进 syncedRev、不 rebuildMap，改为置 dirty 并重试/降级 UI 提示。

**B-3（P2，宿主渐进通道退化）**：内置渐进路径（`?chunks=`）每段 `extend_from_markdown` + `ingestExtend()`（分片建图）+ `locked=true` 拒编辑（main.ts:1586-1599）；但**宿主通道** `mdc-inject-chunk`（1679-1690）只调 `pushRender(undefined, {skipMap:true})`——**不调 ingestExtend（BlockMap 停在首段）也不设 locked**。后果：①追加区域的任何编辑 → 行记录缺失 → 快路径 bail → 每键 O(doc) 全量兜底重同步（性能退化，非正确性——fullResync 会重建图自愈）；②chunk 间隙用户可编辑（顺序执行无竞态，但语义上宿主应复用 locked 门）。
- 证据：main.ts:1683-1688（else 分支无 ingestExtend/无 locked）；对照 1591-1592。

**B-5（P3，孤立 surrogate）**：`apply_ops` 经 wasm-bindgen 传参，JS 串先经 TextEncoder UTF-8 化——孤立代理（U+D800-DFFF 单独出现）被替换为 U+FFFD ⇒ 核内文本与 view 一字符差，`docIsMd` 为假，直到下次 `pushRender` 全量重写规范化。CM6 正常 DOM/IME 输入不产孤立代理；仅程序化 `dispatch`/`execCommand` 可达。`utf16ToCp` 对孤立代理按 1 码点计（main.ts:97-101，codePointAt ≤0xFFFF → i+=1）——JS 侧计数自洽，跨界语义靠 U+FFFD 收敛。

**IME composition 期间核状态**：无专门 composition 处理代码（grep `composition` 仅 main.ts:9 注释）——依赖 CM6 原生 composition 事务；每个预编辑 update 经 MutationObserver→CM6 事务→快路径**逐个落 replace_text op（无缓冲）**，组合期间 view==core 恒成立（ime_cdp.py IC1-IC6 的 `docIsMd` 逐步断言）。取消 = compositionend('') 后 CM6 删除预编辑 → 快路径删除 op，与插入 op 在 1s 合并窗内收**同一个 undo 条目**（IC4「cancel burst still lands ops…undo of cancel-only entry is content-neutral」）。**结论：组合期间无分叉窗口**；代价是组合中间态进核进 undo 合并窗（设计取舍，ADR-B2）。

**其他已登记取舍**：undo/核驱动命令后 view 独有状态（核无法表示的末尾空行、选区）被全量回写吸收，选区仅钳制保活（main.ts:985-989；README §undo）。`installNewCore`（__inject 族）= 导入语义整核重建 + `mdc-core-reinstalled` 重基线脏位（1649-1674, 2681-2686）——防「导入即未保存」缺陷复活，处理正确。

### 2.3 测试覆盖

- W5（web_smoke.py:421-483）Enter/Backspace/列表续行；W6 undo/redo；W1-W13 行为级；G2 渐进；contract_gate inject/chunk 面。
- **缺口**：B-1 触发序列（「先打字后 Enter」）全仓无测试（W5 只测「Enter 后打字」，见 §4.3）；B-2（sync reject 注入）无测试；B-3 宿主通道多 chunk+编辑无测试（contract_gate 只测 2 chunk 内容正确）。

---

## 3. 问题 2：UTF-16↔UTF-8 坐标换算对覆盖面

### 3.1 实现与「单一换算对」声明

- 换算对 = `utf16ToCp`/`cpToUtf16`（main.ts:93-114），模块级唯一出口；类内委托 `cpLen`（699-701）；`__bridge.unit` 暴露给测试（516, 1323-1327）。注释明示「单位宪法（R17/ADR-V2）……禁止散落手工计数」（main.ts:86-90）。
- **例外**：`BlockMap.extend` 内有一处局部 `const cpLen = (s) => [...s].length`（main.ts:300）——`[...s].length` 正是宪法禁止的散落形态（语义等价：迭代器按码点）。同为局部直写的还有 rebuild 的 `nl()` 计行（174）。属**文档级违例、语义无害**（[...s].length 与 utf16ToCp 无位置参数形态同义），但「唯一出口」声称不严格成立。

### 3.2 覆盖面核对

| 语料 | web 侧 | 核侧语义锁 | e2e |
|---|---|---|---|
| 增补平面 emoji（🙂/🎉🚀） | unit 往返 | r17_utf16_mapping.rs（570 LOC，FIXTURE 含 🙂/𠮷） | W13 + IC5（真实 IME emoji，含快路径 op 偏移=码点断言 ime_cdp.py:337-340） |
| CJK 扩展 B（𠮷 U+20BB7） | 同上 | r17 fixture:44 | W13 |
| 变体选择符 U+FE0F（𠮷️） | 同上 | r17 fixture:44「2 码点」自证 | W13 |
| ZWJ 家族（👨‍👩‍👧‍👦 7 码点/11 码元） | 同上 | r17:38 | W13 + W1-W12 语义不变 |
| 组合字符（NFD 拉丁变音符） | —— | **未见显式用例**（chars() 与 codePointAt 均按码点逐个计，语义天然一致） | 无 |
| 孤立 surrogate | codePointAt 计 1 码点（main.ts:97-101） | Rust String 不可能持有 → 边界靠 U+FFFD 收敛（B-5） | 无 |

**评估**：换算对的非 BMP/多码元族覆盖是**三方锁**（核语义测试 + 浏览器 e2e + 真实 IME 通道），质量高。两个残留：①组合字符无显式用例（低风险，双单位制都按码点计数）；②孤立 surrogate 跨界静默替换（B-5，P3）。

### 3.3 r17 核侧测试强度

`crates/mdcore/tests/r17_utf16_mapping.rs`：基础事实自证（96-100 起）+ FIXTURE 混合长文档（43-57：标题/段落/3 层 list/quote/fence/表格/hr 全混非 BMP）+ 码点视角 cp_replace 工具（86-90）——锁定 op.start/end 的 `len_chars()` 语义。**这是换算对正确性的根锚**（JS 侧若错，W13 的 `cpEquals`/op span 断言会爆）。

---

## 4. B-1 详述（P1）：BlockMap 陈旧 textChars → Enter 错误切分

### 4.1 证据链

1. `byId` 的唯一写点 = `rebuild`（main.ts:210）与 `extend`（317）：`{ path, textChars: cpLen(b.text), kind, attrs }`。
2. 快路径 `tryFastPath` 成功后**只**平移同块后续行的 `textOffset`（877-885，多行体 code/table），**不更新 `byId.get(rec.blockId).textChars`**，也不重建 map（这正是「行记录只存行内相对量，doc 增长零维护」的性能设计，但漏掉了 textChars 这个**块级**缓存量）。
3. `handleEnter`（1122-1183）读 `const meta = this.blockMap.byId.get(rec.blockId)`（1134）并把 `meta.textChars` 当作 ReplaceText 的 `end`（1144：`{ t:'replace_text', path, start: charOff, end: meta.textChars, insert:'' }`），`charOff` 则按**当前**行文本计算（1136）。

### 4.2 失效推导（最简复现）

- 初始：list item 文本 `"ab"`（marker `- `，prefix=2，textOffset=0），`byId.textChars=2`。
- 用户在 `a|b` 处打 `X` → 快路径 op `{start:1,end:1,insert:"X"}` 成功 → 核文本 `"aXb"`（3 cp）。**`textChars` 仍=2**。
- 用户在 `aX|` 处按 Enter（行内 col=3）：`charOff = 0 + cpLen("aX") = 2`；`tailText = "b"`。
- 提交 ops：`ReplaceText{start:2, end:2, insert:""}`（**no-op！**）+ `InsertBlock{text:"b"}` → 核内 item1=`"aXb"` **原样保留**、item2=`"b"` → **"b" 重复**。pushRender 后用户看到 `- aXb\n- b`（正确应为 `- aX\n- b`）。
- 一般化：净插入 Δ 后在 `charOff ≤ 陈旧 end` 处 Enter → 删除区间 `[charOff, 旧end)` ≠ 应删的 `[charOff, 新end)` → 中段文本重复/错位；`charOff > 旧end` 时 start>end → 核 BadRange（op.rs:135）拒绝 → `threw` → 返 false 走原生换行+兜底（自愈）；净删除时 `end > len_chars`（op.rs:135）恒拒绝（自愈）。

### 4.3 触发面与测试缺口

- **触发面**：list_item 内打字（快路径）→ 同 item 行内任意位置 Enter（`isItem` 绕过行尾守卫 main.ts:1131-1132；段落/标题仅行尾走核驱动，行尾场景 `charOff=新长>旧end` 恒自愈——所以**只有 list_item 损坏**）。「在 bullet 里打几个字然后回车拆分」是高频编辑动作。
- **测试缺口**：W5（web_smoke.py:457-482）= Enter 在 `- first item` **行尾**且该 item 此前**无快路径编辑**（textChars 新鲜）→ 通过；随后打字发生在**新** item。editor_toolbar/i18n/tr1/hostile 无 Enter 键测。**全仓无「先打字后 item 内 Enter」用例**——P1 未被任何测试逮到。
- 置信：高（三处行号互证 + 最简复现推导）。未浏览器实测（纪律禁 npm/build）。建议验证脚本：`__inject("- ab\n")` → `keyboard.type` 中插 `X` → `keyboard.press("Enter")` → 断言 `__md() === "- aX\n- b"`。
- 修复方向（供参考，未改动）：快路径成功后同步维护 `byId.get(rec.blockId).textChars += delta`（与 876-885 的 textOffset 平移同点）；或 handleEnter 改用核返回/行文本推导 end。

---

## 5. 问题 3：IME 路径与「取消零残留」声称核实

### 5.1 处理路径（实现面）

桥**没有** compositionstart/update/end 的事件级处理（grep 证实 main.ts 仅头注释 9 行提及「IME 大改 → 兜底」）。实际路径：Chromium 原生 composition DOM 变更 → CM6 MutationObserver/事务 → `onDocChanged` → 快路径（单 change、行内、无 \n）→ 逐 update 落 op；含 \n 的提交（真实 IME 不产，测试用压测）bail='newline' → 兜底。**即 IME 完全复用普通编辑双层管道，无特判**——这是「取消零残留」可信的结构性原因：预编辑文本在核里就是普通 replace_text，取消 = CM6 删预编辑事务 = 又一个 replace_text，无独立状态可残留。

### 5.2 ime_cdp.py 断言强度核实

- 通道：Playwright `context.new_cdp_session` + CDP `Input.imeSetComposition`/`Input.insertText`（ime_cdp.py:112-117 等）——经 Chromium **InputMethodController**，产生真实 compositionstart/update/end + beforeinput(insertCompositionText) 事件链与原生预编辑 DOM 变更（文件头 5-10 行与 gates.md R23-a 一致）。**这强于合成事件（W3/W9 的 execCommand 路径），但不是 OS 级真 IME**（无候选窗 UI 交互、无重转换、无跨进程 IME 特有取消）。
- 28 checks（gates.md:360 记档；实测 check 站点 29 含 pageerror）覆盖：IC1 拼音主链（事件链计数、逐 update 单 op、合并窗 coalesced+revision 恒定、提交=单 replace_text、整段=1 undo 条目、一次 undo 全撤且 doc==md）；IC2 多字词（Chromium 提交差分 ins='世界'）；IC3 跨行提交（bail='newline'→兜底、结构 op 独立 undo 条目、两次 undo 回到预编辑态）；IC4 取消（compositionend(data='')、**doc 无 "xfor" 残留、composing 清零、docIsMd、取消爆发=单条目且 undo 内容中性**——「零残留」断言即此四点，264-286 行）；IC5 emoji 非 BMP（含快路径 op 偏移=码点的直接断言 337-340）；IC6 组合中点击（**诚实登记 CDP 限制**：虚拟组合下点击不触发 blur 强制提交——真 IME 强制提交保留为人工项，387-399 行）；DIFF 合成基线对照。
- 判定：**断言强度 = 事件链 + op 流形状 + undo 栈深 + 文档内容 + view==core 五维，属高可信自动化**；「取消零残留」在 CDP 语义内成立。剩余风险=真 IME 独有路径（强制提交/重转换/部分 IME 的 Esc 语义），R22 候选 4 已登记为人工验收项——声称边界诚实。

---

## 6. 问题 4：撤销桥

- **CM6 history 刻意不装**（main.ts:15-16 注释；extensions 列表 605-688 无 history）——`Mod-z/Mod-Shift-z/Mod-Shift-Z` 三键位绑定 `owner.undo/redo`（666-671，Shift 大小写双写是 R9 审查修复）。**单栈（核）无双栈**，不存在「undo 桥穿透双栈」问题；`undo()` 恒返 true = 键位消费契约（995-999「永远消费按键：没有 CM6 history 可退」），核侧 no-op 判据 = revision 不前进（gates.md R21 勘误，244-247 行）。
- undo 后 = `pushRender()` 全量回写（997；README §undo 取舍：view 独有状态被吸收）。undo 大文档成本 = to_markdown + 全 doc dispatch + rebuildMap（W4 记 169.4ms 全量回写）。
- **R21/ADR-B4「ingest 不入栈」已落地**：`apply_ops` 的 `Source::System` 分支 bump revision + log.record + push_event + `note_boundary`，**跳过 push_entry/coalesce**（mdcore lib.rs:316-331）；`note_boundary` 只推进合并窗边界（history.rs:108-116），System 后首个 View 击键必开新条目（should_coalesce 要求两端 View，history.rs:290-291 测试锁）。「桥穿透 0.3ms」= 开档后 undo 为 no-op：`core.undo()` 返 None → 不 pushRender → 仅键位消费路径（gates.md:66 表格「no-op（R21/ADR-B4……桥穿透 0.3ms）」）。实测 10MB 渐进开档 history.undo_entries 15→0、wasm 内存 -83.8MB（gates.md:252-258）。
- `sync_from_markdown` 保持 `Source::View`（mdcore lib.rs:804-809「source = 谁驱动了内容变化」）→ **兜底桥的编辑可撤销**，语义正确（undo 能撤回兜底差分事务，IC3 两次 undo 验证）。
- undo 失败回滚：条目压回 + 边界拨回 + 树由 backup 恢复（mdcore lib.rs:905-918，T1-fix F10）。
- IME/合并窗：组合+提交 = 单条目（ADR-B2 1s 窗，IC1/IC2/IC5 断言 undoLen==1）。
- **缺口**：桥层无 undo 后选区恢复（仅钳制保活）；跨「快路径多行体 textOffset 平移」与 undo 的交互未专项测（W10 结构 undo ID 还原已覆盖大结构面）。

---

## 7. 问题 5：mdcore-wasm panic 分布与可达性（62 unwrap/167 expect/17 panic 的真相）

**总量统计含 `#[cfg(test)]` 模块**。逐文件切分（mod tests 起始行：lib.rs:280 / xlsx_export:519 / xlsx_import:814 / docx_export:1516 / docx_import:837）：

| 模块 | 生产段（非 test）panic 站点 | 评估 |
|---|---|---|
| lib.rs（WasmCore 主面） | 0（全部 `unwrap_or_else` 兜底：114-117/146/250-251） | 干净 |
| docx_export.rs（1-1515） | **0**（expect/panic 全部 ≥1529） | 干净；导出面 `Result<Vec<u8>, JsValue>` typed 错误（1469/1487） |
| xlsx_export.rs（1-518） | **0**（全部 ≥532） | 同上（504） |
| docx_import.rs（1-836） | 1 × `unreachable!`（578「text boxes flattened in preprocessing」） | **安全**：490-510 预处理循环把 TextBox 展开至定点（`expanded` 标志控制），第二遍 match 不可达 |
| xlsx_import.rs（1-813） | 1 × `expect("ASCII")`（343，col_letters） | **安全**：`b'A'+col%26` 构造恒 ASCII |
| canvas.rs / collab.rs / workbench.rs / xlsx_grid.rs（无内嵌 test） | ≈35 × `serde_json::to_string(...).unwrap()`（canvas 239-321、collab 441-606、workbench 297-313、xlsx_grid 105-301）+ 2 × `.lock().unwrap()`（canvas:511、collab:644） | String/简单值序列化**实践不可失败**；lock 在 wasm 单线程下仅当先前 panic 持锁才毒化（见 B-4） |

**用户输入可达的硬 panic ≈ 0（mdcore-wasm 自身）**。跨 crate 可达面：`import_docx/import_xlsx`（自由函数，docx_import.rs:774-775 / xlsx_import.rs:727）直通 **office-io（loc-stats：616 unwrap / 27 expect / 74 panic）**——恶意文档的 panic 风险真实存在于该 crate（另案审计范围）；但 wasm 侧这两个入口是**无共享态的自由函数**（t16/t17 设计），panic 只炸本次导入调用（JS `docxImport()` 的 try/catch 捕获为 `{ok:false, code:'unknown'}`，main.ts:1874-1891），**编辑器 WasmCore 不受污染**——这是良好的故障隔离设计。

**B-4（P2）：无 `console_error_panic_hook`**——mdcore-wasm/Cargo.toml 依赖仅 wasm-bindgen+serde_json（无 hook；对比 pdf-wasm 有 hook，loc-stats:16）；build-wasm.sh 无 panic 策略覆盖 → wasm32 默认 abort。后果：①panic 消息丢失（RuntimeError: unreachable，无 Rust 位置）；②`apply_ops` 的 backup 回滚只在 `Err` 路径（mdcore lib.rs:297-313）——panic 若发生在 mutation 之后、收尾之前，树=半改而 revision/log/history 未推进，**后续调用在脏状态继续**（无形式毒化标志、无恢复路径；wasm 线性内存中的结构仍可被下次调用触及）。缓解：可达面≈0 + 导入隔离 + JS 层 catch。建议：接 hook（诊断）+ 考虑 panic 后核只读降级标志。

---

## 8. 问题 7：渐进加载与大文档内存

### 8.1 配合面（设计正确性）

- 核侧分片出口：`blocks_count`（mdcore debug.rs:49-61）/ `blocks_json_range(from,count)`（debug.rs:69+，越界截断）；wasm 面 lib.rs:130-163。
- web 侧：内置渐进路径每 chunk `extend_from_markdown` → `ingestExtend()`（main.ts:918-936）→ `blocks_count` 游标 + `blocks_json_range(prev, total-prev)` → `BlockMap.extend`（298-393，只解析新增先序区间——消除「每次追加全量 dump+parse 20 万块」双份大对象）；事件容量护栏：extend 的 insert_block 明细不进 JS 堆，按 `{absorbed:N}` 聚合（919-930）；`locked=true` 拒编辑（1586）；ingest 毕一次性 `pushRender({skipMap})`（1597）。
- 等价性护栏：W7 `debugMapVsFullRebuild`（943-955）增量建图 ≡ 强制全量重建逐项比对。
- 宿主通道（B-3）与 xlsx_grid 的 `gridOpenBytesWhenReady` 各自独立生命周期（main.ts:1710-1745 注释）。

### 8.2 内存驻留根因（10MB → wasm 425MB）

- **根因 = `apply_ops` 每事务 `self.tree.clone()` 整树备份**：mdcore lib.rs:297（apply_ops）、380（apply_raw_batch=undo/redo）、1015（replay）。10MB 渐进开档 16 段 × 全树 Rope/slots/id_index 克隆 = 分配器高水位主驱动（gates.md:282-287 R22-b 原文登记：「apply_ops 每事务 tree.clone() 整树备份是分配器高水位主驱动……候选路径：求逆回滚/受影响子树范围备份。回滚正确性需 ADR 先行」）。R22-a（Arc<str> op 文本共享）实测仅 Δ-0.6MB（噪音级，估算撤回，gates.md:309-310）。
- 量级链：R20 基线 509.2MB → R21（ingest 退出 undo 栈，双份 ops+inverses 消失）425.4MB（-83.8MB，gates.md:255）→ R22-a 424.8MB。逐键打字同样每键整树 clone（W4 p95 13.5ms 的主项之一；#205 增量 fence 缓存刚修的是渲染侧 O(doc)，非此备份）。
- JS 侧治理已到位（R16 三件：分片建图/批量化 pushRender/事件聚合；R19 `__wasmMemory`+`__heapTrace` 归因出口；R20 `__memStats` 组件级审计）。oversized_web.py A 组断言 10MB 加载 + 60s 内 ready + 编辑/undo 往返存活 + wasm <1.5GB（记录非门）。

---

## 9. 问题 8：web 桥测试可信度（「28+16」核实）

- **「28+16」= `ime_cdp.py` 28 checks（README.md:64、gates.md:360/R23-e）+ `contract_gate.py` 契约门 16 checks（README.md:65 记档；当前文件已长到 ~22-23 个 check 站点）**。
- **ime_cdp 28 checks 断言什么**：真实事件链计数 + op 流形状（逐 update 单 replace_text）+ 合并窗（coalesced/revision）+ undo 栈深 + 文档内容 + view==core——**行为级**（见 §5.2）。
- **contract_gate ~16-22 checks 断言什么**：约半数是**结构级**（`__ready`/`__contractVersion===1.4`/boot_surface typeof 全谱/bridge 清单 typeof/渐进专属成员在场性，contract_gate.py:57-95、224-234）；另半数**行为级**（inject 全量重建内容、事件族计数与排空时机、渐进 chunk 两段内容、undo 键位契约恒 true、单位换算往返、settings 读写/16 种拒绝/面板联动，103-222）。**结构+行为混合，非纯结构**。
- 整体矩阵（35 py）：行为级为主——web_smoke W1-W13（69 check 站点）、crash_recovery（16 checks，真实键盘 beforeunload/草稿四组）、integration_journey（真键盘+真 fixture 字节跨四表面）、oversized（typed 拒绝/有界行为/no-hang 三性质）、hostile（空文档/超大/emoji-BIDI/连击/localStorage 污染）、editor_toolbar/i18n_ui/各面板面。**共同弱点**：①结果 `results_*.json` 不入库（web/tests/results 不存在且被忽略）——gates.md 的「全绿」数字无法仓内独立复核；②W5 类 Enter 用例未覆盖「先打字后 Enter」（B-1 漏网）；③均跑在 dist/ 构建产物上（pkg 不入库，同因不可仓内复跑）。
- 对照：shell 侧 `contract_freeze.rs` = 词面锁（`src.contains("'mdc-inject'")` 等字符串包含断言，19-35 行）——冻结契约的合理形态，但非行为验证。

---

## 10. 汇总表（含测试覆盖与置信）

| 编号 | 级别 | 问题 | 证据链 | 测试覆盖 | 置信 |
|---|---|---|---|---|---|
| B-1 | **P1** | 快路径不维护 `byId.textChars`；list_item 行内 Enter 用陈旧 end → ReplaceText 错误区间 + InsertBlock 正确尾文本 → 中段重复/错位（最简例：`- ab` 中插 X 后 Enter → `- aXb`/`- b` 重复）。段落行尾与净删除场景因 BadRange 自愈 | main.ts:210/317（唯一写点）、872-886（快路径无 byId 更新）、1134/1144（消费点）；op.rs:135（reject 边界） | **零覆盖**（W5 只测 Enter-后-打字；无任何测试打字-后-Enter 于 item） | 高 |
| B-2 | P2 | fullResync 失败仍 rebuildMap（旧树对新文本）+ syncedRev=rev → pushRender 守卫永久跳过 → 分叉不自愈 | main.ts:890-900、970 | 无（无 sync reject 注入用例） | 高 |
| B-3 | P2 | 宿主 `__inject_chunk(first=false)` 无 ingestExtend/无 locked → BlockMap 落后、追加区每键 O(doc) 兜底；与内置渐进路径不对称 | main.ts:1679-1690 vs 1586-1599 | contract_gate 仅内容正确（120-126） | 高 |
| B-4 | P2 | 无 console_error_panic_hook；panic 后核可处于半改态（backup 回滚仅 Err 路径）且静默 | mdcore-wasm/Cargo.toml（无依赖）；mdcore lib.rs:297-313 | 无 | 高（后果低频：生产可达 panic≈0，导入隔离） |
| B-5 | P3 | 孤立 surrogate → U+FFFD 一字符差至下次全量重渲染 | main.ts:97-101（JS 计 1 码点）+ wasm-bindgen TextEncoder 语义 | 无 | 中 |
| B-6 | P3 | drainEvents/ingestExtend 静默吞坏 JSON（事件证据断流/增量建图停追，后续 fullResync 自愈） | main.ts:799-807、919-930 | 无 | 高 |
| B-7 | P3 | apply_ops 每事务整树 clone = 10MB wasm 425MB 高水位主驱动（逐键同源） | mdcore lib.rs:297/380/1015；gates.md:282-287 | oversized_web.py 仅记录非门 | 高（与 gates R22-b 一致） |
| B-8 | P3 | 无宿主时 `__save` 为假保存（仅派 mdc-save 事件+清 dirty）；TOC 重名跳首；extend 内 `[...s].length` 违反单位宪法「唯一出口」字面 | main.ts:2690-2701、2738-2741、300 | crash_recovery 测 dirty 语义（测的是口径非持久化） | 高 |

**正面确认**（无问题）：R21 System-undo 落地与测试（lib.rs:316-331 + history.rs:266-299）；IME「取消零残留」在 CDP 语义内五维断言成立（ime_cdp.py IC4）；导入 panic 故障隔离（自由函数 + JS catch）；单位宪法三方锁；wasm 单飞 init 门修复真实 SEGV（wasm_boot.ts:1-14）；mdapp:// 目录穿越防护（shell-native lib.rs:198-201 + 测试 432-440）；契约双侧锁纪律（CONTRACT v1.6 + contract_gate + contract_freeze）。

---

## 11. 建议行动（按优先级）

1. **[P1] 修 B-1**：快路径成功处同步 `byId.textChars += delta`（与 textOffset 平移同点）；补 e2e：item 内打字→Enter 断言 `__md()` 精确文本。验证成本低（一条 py 用例）。
2. **[P2] B-2**：fullResync 失败分支不推进 syncedRev/rebuildMap，置重试或用户可见错误。
3. **[P2] B-3**：宿主通道复用 `ingestExtend()` + `locked` 门（与内置路径对称）。
4. **[P2] B-4**：接 console_error_panic_hook（pdf-wasm 先例）；评估 panic 后核只读降级。
5. **[P3] B-7**：R22-b 已登记的 backup 克隆治理（求逆回滚/子树备份，需 ADR）——同时惠及逐键延迟与内存。
6. **[P3]** 单位宪法文档勘误（extend 内 `[...s].length`）；孤立 surrogate 专项用例；results_*.json 入库或 CI 归档使 gates 数字可复核。
