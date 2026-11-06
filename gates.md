# 第一阶段硬 Gate 清单（R14 收口，2026-10-08；R15/R16/R17 主档收口；R18 视口惰性终局；R19 堆口径归因；R20 wasm 内存审计出口；R21 System-undo 语义收口 ADR-B4；R22-a Arc<str> op 文本共享 + 估算撤回；RC 全仓合规化前置节点完成，2026-10-09）

> 主档 §15.3 四条硬 Gate 的量化判定 + §28 P0-3 拆分实测。
> 证据：`web/tests/results/results.json`（W1-W12 + G1/G2）、
> `crates/mdcore/tests/r14_gates.rs`（核侧，串行计时）。
> 原则：**回归套件与门判定分离**——冒烟 checks 保持绿（功能正确性），
> 预算判定在本文档与各 G 项 `gate_verdict` 字段，GAP 不回避。

## §15.3 四门判定表

| # | Gate（预算） | 实测 | 判定 | 证据 |
|---|---|---|---|---|
| 1 | 1MB fixture 可输入 **< 600ms** | 全量打开 349.4ms（W4）；渐进 TTI **156.9ms**（W7，-55%）；10MB 渐进 TTI 362.7ms（G2 参考） | **PASS** | W4/W7/G2 |
| 2 | 击键→光标响应 **< 16ms** | R15：selection p95 **5.0ms**（PASS）。R18 视口惰性重构后：selection p95 **0.8ms** / max 2.6ms（probe 三连 0.8/2.2/1.1ms）；typing 全路径 p95 **12.9ms**（R14 基线 34.2ms）；核内份额 p95 8.7ms（不变，归因基线） | **PASS（R18）** | G1 |
| 3 | Document State 完整 **snapshot / restore** | 20k 块（1.2MB）三重恒等（dump_tree / blocks_json / MD 序列化）+ revision 复原 + 跨档 undo 链可用；snapshot 125.2ms / restore 216.2ms | **PASS** | r14_gates |
| 4 | 中文 / 日文 **IME Gate** | synthetic composition 三轮 + 取消路径零残留（W3/W9，核/视图收敛）；真 IME 需真实输入法环境，人工验收协议已成文 | **PASS（synthetic）**；真机验收 = web/README §6 人工协议（沙箱无真 IME，登记待办） | W3/W9 |

## Gate 2 GAP 归因与 R15 修复（2026-10-09 收口）

R14 三路拆分（G1，1MB / 20448 块）：

```text
纯光标路径（selection-only dispatch）   p95 23.9ms   ← R14 判门口径，超 16ms
typing 全路径（changes dispatch）       p95 34.2ms   ← 含核 op + CM6 更新 + 装饰重建
core-only 旁路（WasmCore 同形 op）      p95  8.7ms / p50 8.5ms  ← 核内份额
```

**R14 根因（代码级）**：`decoField.update` 对任何带 `selection` 的事务触发
`buildDecorations(tr.state)` 全量重建（O(doc)）。这是语义必需——
`revealed(from,to)` 使 mark/hr 的「显示原文 vs 替换为部件」取决于
**光标是否悬停其上**（Spike S1 光标协商语义），因此纯 selection 事务
合法地改变装饰集，无法简单跳过。

**R15 修复（已落地，视图层专项）——增量窗口化重算**：

1. decoField 值改为 `{deco, atomic}` 元组：主装饰集与其原子子集
   （isMarker 且非零长）成对维护，`atomicRanges` facet 改为 O(1) 取缓存
   （原实现每次 selection 事务对全 doc 重过滤一遍，是第二热点）。
2. 纯 selection 事务只对「新旧选区覆盖行 ±1 的真并集」（按行区间合并）
   重算装饰，与全量路径**共享同一单行生成函数**（正则只此一份）；
   窗口重算结果与旧集逐项（位置 + 装饰值身份）全同时原样返回旧值
   （CM6 装饰 diff 为空，只剩 CM6 自身选区开销）；有变化才按
   「旧集 gap 段 + 窗口重算块」用 RangeSetBuilder 有序拼装。
3. 等价性护栏 W12（冒烟新增）：六步选区序列（marker 内揭示/移开/
   双光标远距/跨行选区/doc 末尾边界/返回行首）逐步断言
   **增量产物 ≡ 强制全量重算产物**（deco + atomic 双集逐项对比）。
   该护栏在收口审核中实际抓出并修复了一处迭代器双 advance 缺陷
   （`RangeSet.iter()` 经 goto() 已定位在首项，循环初始化再 next()
   会静默跳过旧集首项——doc-end 场景丢失标题行类点装饰）。

**修复后（G1 复测，1MB / 20448 块）**：光标 p95 **5.0ms** / max 11.3ms
（预算 16ms，**PASS**；较 R14 的 23.9ms 改善 ~4.8 倍）；typing p95 33.9ms
（持平，docChanged 仍走全量重建，符合预期）；核内份额 8.7ms 不变
（selection-only 事务不触核，残余成本为 CM6 选区自身开销，低于核内单 op
份额）。W1-W12 冒烟 14/14 绿。

## §28 P0-3 量化表（1MB / 10MB）

| 项 | 1MB（20448 块） | 10MB（200576 块） | 预算（§15.1） | 判定 |
|---|---|---|---|---|
| 打开 TTI | 全量 349.4ms / 渐进 156.9ms（R14）；R18：渐进 **99.9ms**（同环境全量对照 231.5ms） | 渐进 284.7ms（R16）→ **201.6ms**（R18，16 段） | 1MB <600ms（硬门）；10MB 首屏 <300ms（非门） | 1MB **PASS**；10MB **PASS（R16，R18 再优化）** |
| 完整 ingest | 657.4ms（8 段） | **4240.7ms**（16 段，R16 后） | 非门 | 记录（-64%） |
| 击键（全路径 p95） | 34.2ms（R14）→ **12.9ms**（R18） | —（未测，10MB 打字属滚动后场景） | 击键→增量解析 <10ms（非门） | 记录（视口化后剩 hr 索引全扫 ~2-4ms + 核内 ~7ms） |
| 光标（selection p95） | **5.0ms**（R15）→ **0.8ms**（R18） | — | <16ms（硬门） | **PASS（R18）** |
| IME（synthetic） | 三轮 + 取消零残留 | — | 硬门 4 | PASS(synthetic) |
| undo | 全量回写 169.4ms（W4）；批量 127.3ms（W8） | **no-op（R21/ADR-B4：开档后 ingest 不在 undo 栈；桥穿透 0.3ms）**；R16 时代的 1898.5ms 撤段主体已不存在 | 非门 | 记录（语义变更，见 R21 段） |
| 终态 JS 堆 | 168.8MB（G1，R14 旧口径） | **R22-a 口径：used=483.5MB = JS 58.7MB + wasm 424.8MB**（R21 基线 484.1 = 58.7 + 425.4；R22-a Arc<str> 实测 Δ-0.6MB ≈ 噪音，估算撤回见 R22-a 段） | 1MB <250MB；10MB <600MB（均非门） | 1MB 达标；10MB JS 堆 58.7MB 达标；wasm 509→425→424.8MB，高水位主驱动 = backup 克隆（R22 候选 #2 在册；「R22-b 已落地」系研究仓单方声称、未回流 mdcore——#220 勘误：序修正半边已落地、备份未换） |
| 核侧单 op（旁路） | p50 5.97ms / p95 6.31ms；小文档对照 0.02ms | — | 增量解析 <10ms（非门） | 记录：成本随文档规模增长（300×），p95 在 10ms 内 |
| 核侧重放 | 256.6ms（1.2MB 日志） | — | 非门 | 记录 |
| 核侧 genesis | 217.5ms（1.2MB from_markdown） | — | 非门 | 记录 |

## 测量卫生备注

- 核侧计时测试必须**串行**（`gate_lock`）：并行 build_big 互相污染内存带宽时，
  单 op p95 从 6ms 虚增至 33ms（本轮实测对照，教训入档）。
- 浏览器数字为 headless chromium 沙箱值，绝对量随机器变化，趋势与比值更可信；
  结果快照 `web/tests/results/results.json`。
- **R16 堆测量口径修正**：chromium 须以 `--js-flags=--expose-gc
  --enable-precise-memory-info` 启动（后者缺省时 usedJSHeapSize 为量化陈旧值，
  GC 前后多位小数完全不变）；「终态堆」= 开档完成并强制 GC 后的稳态；
  undo 压测属非常态动作，其重建残留单独透明记录（heap_after_undo_mb），
  不入判定。10MB 开档稳态实测在 595-655MB 间波动（运行间方差 ~±40MB）。

## 非门登记（§15.3 明示暂不判定）

内存上限（1MB 169MB 达标 / 10MB 803MB 超预算 FLAG）、完整解析时间
（10MB ingest 11.9s）、60fps 滚动（未测）——均留优化轮，不阻塞第一阶段。

## R15/R16 候选清单（由 R14 数据产生；2/3 已落地）

1. ✅ **Gate 2 视图专项**（R15 落地）：装饰增量窗口化 + 原子集缓存，
   光标 p95 23.9ms → **5.0ms**（<16ms PASS），W12 等价性护栏入冒烟。
2. ⚠️→大部分 **10MB 堆优化**（R16）：核侧新增 `blocks_count`/
   `blocks_json_range` 分片出口 + JS 侧 BlockMap 增量建图 + 渐进批渲染，
   终态堆 803 → **631.3MB**（-21%，测量口径同步修正）；仍超 600 非门
   预算 ~31MB，且开档稳态存在 **~500MB 未归因驻留**（JS 侧清单仅解释
   ~100MB，CDP 采样显示分配大户为 BlockMap.extend ~32MB）——归因调查
   登记 R17。
3. ✅ **10MB 渐进首屏**（R16）：渐进 extend 分片建图 + ingest 后一次
   pushRender（rev 守卫免重复同步），TTI 362.7 → **284.7ms**（<300ms），
   ingest 11.9s → 4.2s，undo 2.27s → 1.90s。

## R18 视口惰性终局优化（2026-10-09，Gate 2 从 5.0ms → 0.8ms）

**背景与和解**：本会话在未知远端推进的情况下独立完成了另一套 Gate 2 修复
（视口惰性装饰），rebase 时与远端 R15/R16/R17 三提交链冲突。按 worklog
Task 13 固化的和解协议处置：**远端三提交为最新基线，本地方案（数据更优）
在 R17 基础上以 R18 轮次重新实施**，W12 等价性护栏恰好充当重构验证器。
教训再次强化：开工前必须 `git fetch`（本轮因违反此条造成一次重复施工）。

**方案对比（同机同口径，1MB / 20448 块）**：

| 方案 | 机制 | selection p95 | typing p95 | 10MB 渐进 TTI |
|---|---|---|---|---|
| R14 全量 O(doc) | 每次 dispatch 全文档重建 | 23.9ms | 34.2ms | 362.7ms |
| R15 窗口增量（远端） | selection 窗口 ±1 行增量拼装，无变化零 diff | 5.0ms | 33.9ms | 284.7ms（R16 后） |
| **R18 视口惰性（本轮）** | ViewPlugin 只构建视口 ±100 行；StateField 收窄为 hr block 专用通道 | **0.8ms** | **12.9ms** | **201.6ms** |

R18 相对 R15 的增量收益来源：① doc 变更路径同样视口化（R15 的 docChanged
仍是全量重建，1MB 上 ~8ms；R18 为 O(viewport) + hr 索引单扫 2-4ms）；
② 无增量拼装的窗口比对逻辑，代码面更小（删 incrementalDecorations/
windowUnchanged/windowIntervals/emitWindow 四方法）；③ 10MB 渐进期间
装饰构建从全量降到视口（TTI 284.7 → 201.6ms）。

**架构（双源）**：
- `decoPlugin`（ViewPlugin + PluginSpec.decorations 自动供值）：视口
  ±DECO_PAD(100) 行的全部非 block 装饰（复用共享单行生成
  genLineDecorations，语义与全量逐位一致）+ 同窗原子缓存（isHiddenMarker
  过滤），doc/selection/viewport 任一变化触发重建，每次用事务新状态全算
  窗口内行（不复用旧集，spike 战役②越界教训天然规避）。
- `decoField`（StateField）收窄为 **hr block widget 专用通道**：CM6 禁止
  block decoration 经 plugin 供应（view/dist L2777 throw）；hr 行索引
  单遍扫描（docChanged / 首次），未 revealed 行整行替换，集合内条目全部
  即原子装饰 → atomicRanges 对本集 O(1) 直取。打字路径剩余成本 ~2-4ms
  （hr 全扫），增量维护登记 R19。
- `atomicRanges` 双源：from(decoField) + facet.of(view.plugin)（勘误
  记档：Facet.from 只接受 StateField/Facet 源，plugin 源必须
  facet.of((view) => view.plugin(p)...)，传错运行时崩）。
- `dumpDecorations`（W12 护栏）适配双源：两源区间不相交（hr 行 hidden
  只有 field、revealed 只有 plugin），拼接排序即确定性全序。

**验证**：W1-W13 + G1/G2 全绿（15 组，含 W2 双向原子跳过、W12 增量/全量
等价、W13 UTF-16 单位宪法）；G1 selection p95 冒烟口径 0.8ms、probe 三连
0.8/2.2/1.1ms（max 2.6ms），预算 16ms 的 5-16%；R16/R17 的 10MB 能力
（分片建图/增量 BlockMap/单位宪法）零回归。

**已知权衡**（登记 R19）：DECO_PAD=100 行外的程序化跳转（无滚动）处
marker 暂无原子协商区——键盘命令自带 scrollIntoView，滚动即重建恢复；
影响面待真机评估，必要时 filterAtomic 光标邻近补算。

## R19 候选清单（R18 收口后登记）

1. **10MB 堆 FLAG 收口**：631.3MB → <600MB（R16 留下，~500MB 未归因
   驻留调查在 R17 未闭环——继续 CDP 归因）。
2. **hr 索引增量维护**：docChanged 全量单遍扫描（~2-4ms@1MB，typing
   12.9ms 中占比最大视图项）按 changes 受影响行区间增量重判。
3. **视口外原子区缺口**：见 R18 权衡——真机评估影响面，必要时补算。
4. **真 IME 人工验收**：协议已成文（web/README §6），待真实输入法环境。

## R19 堆口径归因修正 + 视口外原子区补算（2026-10-09）

**堆归因三重测量**（10MB 渐进开档，precise-memory-info + expose-gc）：

| 测量 | 方法 | 结果 |
|---|---|---|
| usedJSHeapSize（GC 稳态） | performance.memory（precise） | **568.0MB** |
| wasm 线性内存 | `__wasmMemory.buffer.byteLength`（Memory 对象实时引用） | **509.2MB** |
| JS 分配采样（全程） | CDP HeapProfiler.startSampling 128KB | **46.5MB**（BlockMap.extend 32.1MB 为最大户；split/set/rebuild 合计 ~11MB） |

**结论**：① 新版 Chromium 的 usedJSHeapSize **包含 wasm 线性内存**（568 = JS 58.7 + wasm 509.2 自洽）；② **真实 JS 活数据 ~59MB，10MB「堆 FLAG」解除**——R14/R16 的 803/631MB 是口径假象（把核侧 wasm 内存计成 JS 堆）；③ **堆增长的绝对大头在 ingest 阶段**（heapTrace 分阶段采样：ingest 每段 +32MB 稳定增长，final-render 仅 +8MB），即 wasm 侧 ropey/arena/revision 日志随文档规模线性增长——核侧问题，透明记录，优化登记 R20。

**R16 遗留项处置**：R16 注记的「~500MB 未归因驻留」归因闭环——即 wasm 线性内存；R16 的 smoke launch 参数漏加 `--enable-precise-memory-info`（注记说了但代码没加，导致非 precise 模式 usedJSHeapSize 为量化陈旧值），本轮补上。

**产品化观测点**：`window.__heapTrace`（渐进开档分阶段 JS 堆采样：chunks-built/core-created/first-chunk-rendered/ingest-N/final-render）与 `window.__wasmMemory`（Memory 对象实时引用）——wry 宿主内存面板可直接消费。

**视口外原子区补算**（R18 已知缺口修补）：atomicRanges plugin 源增加按需补算——selection 端点行落在 DECO_PAD 窗口外时，补算端点行 ±2 行的原子集（与视口集不重叠，拼接排序合并）；修复 seen 集合初始化 bug（端点行被自身去重排除，目标 marker 永不生成——gap-check 探针抓出）。probe_gate2.py 增加 gap-check 子用例：1MB 文档程序化跳至视口外 heading marker（行 301），ArrowLeft 原子跳过整段标记 ✓；selection p95 0.9ms 无回归。

**验证**：W1-W13 + G1/G2 全绿；G1 selection p95 0.9ms；G2 分离口径 568 = 58.7 + 509.2。

## R20 候选清单（R19 收口后登记）

1. **wasm 线性内存优化**（核侧，509MB@10MB 文档）：arena 块树/ropey 表示/
   revision 日志的内存布局审计（需核侧 profiler；块均 ~2.5KB 明显偏胖）；
   大输出（blocks_json 25MB 串）的临时分配峰值治理。
2. **hr 索引增量维护**：docChanged 全量单遍扫描（typing p95 16.4ms 中视图
   侧大头）按 changes 受影响行区间增量重判。
3. **真 IME 人工验收**：协议已成文（web/README §6），待真实输入法环境。

## R20 wasm 内存审计出口与 509MB 定调（2026-10-09）

**审计出口（产品化）**：核侧新增 `Core::mem_stats()`（tree/revision/history 三模块
各自新增 mem_stats 方法）→ wasm 导出 `core_mem_stats()` → JS 侧
`window.__memStats()`。零新依赖，host 内存面板与后续优化轮共用。

**10MB 开档的 wasm 509MB 构成定调**（core_mem_stats 实测）：

| 组件 | 实测 | 说明 |
|---|---|---|
| tree.slots_capacity | 16.8MB | 20 万 Node × 84B（Option<Node> 槽位数组）|
| tree.rope_est | 10.6MB | **per-block ropey::Rope**（结构+内容，9.8MB 文本）|
| tree.id_index | ~13MB | BTreeMap<BlockId,NodeId> 20 万条 |
| tree.attrs/children/marks | ~3MB | 25 万 attrs 条目 1.6MB 内容 + 头 |
| log（op 流克隆） | **~47.6MB** | 200576 ops：headers 估 36.1MB + content 11.4MB |
| history（undo 双份） | **~23.9MB** | **undo_entries=15 = 全部 ingest 段在栈**（undo_ops 375528 双份）|
| events_pending | 0 | 已出队 |
| **core 活数据合计** | **~110MB** | |
| **差值 = 分配器高水位/碎片** | **~400MB** | per-block 微分配（20 万次 Rope/Vec/String）与每段 ingest 临时（chunk 拷贝/parse 中间/op Vec/分片 JSON）交错，释放后不归还 |

**优化路径登记**（每项独立轮次量级，按收益/约束排序）：
1. **System-undo 语义审查（兼语义疑点）**：undo 栈持有全部 ingest System 事务
  （用户 undo 可撤 ingest 段）——语义上 System ops 是否应入 undo 栈需 ADR 裁决；
   若过滤可省 ~24MB 活数据。**审查项，未动**。
2. **op 文本共享（Arc<str>）**：InsertBlock.text/ReplaceText.insert 的 String →
   Arc<str>（rope/log/history-inverses 三份合一），估省 20-30MB；serde arc
   feature + 全构造点改造，中型工程。
3. **per-block Rope → String（ADR 修订级）**：短文本 Rope 的结构开销
   （结构 48B + 内部节点）约为 String 两倍；20 万块估省 20-40MB 并显著减少
   微分配碎片——但违反 R8「per-block Rope」选型（tree.rs 注释），需 ADR 修订。
4. **分配器治理**：碎片主因，零依赖约束下无可选 allocator（talck/lol_alloc 均
   为新依赖）——主档红线不动；可评估 ingest 前预分配 hint（slots/log.reserve）
   降低 realloc 碎片。

**验证**：cargo test 16 套件全绿（mem_stats 为纯只读审计，无行为变更）；
W1-W13 + G1/G2 全绿；used=567MB 与 R19 基线一致（本轮无回归无恶化）。

## R21 候选清单（R20 收口后登记）

1. **System-undo 语义 ADR**：undo 栈过滤 System source（含 10MB undo 撤段
   语义疑点审查）+ 若通过省 ~24MB。
2. **Arc<str> op 文本共享**（中型工程，省 20-30MB）。
3. **ingest 预分配 hint**（slots/log.reserve，降低碎片）。
4. **真 IME 人工验收**（协议已成文，待真实输入法环境）。

## R21 System-undo 语义收口（ADR-B4 落地，2026-10-09）

**裁决（decisions/ADR-B4-system-undo.md）**：undo 栈 = 交互编辑史——
`Source::System` 事务（extend_from_markdown 等核自驱装载）不入 undo 栈；
合并窗边界破组保留（`note_boundary` 推进 last_at_ms/last_kind/last_source
= System，System 后第一个 View 击键必开新条目，杜绝跨 ingest 边界错误
合并）；revision log 与事件流完全不变（replay == live state 保持）；
sync_from_markdown 保持 View（**边界规则：source = 谁驱动了内容变化，
而非谁调用了 API**——sync 的 op 内容源于用户 CM6 编辑，必须可 undo）。
行为锚点：打开文档不可撤销。

**实现**：history.rs `push_entry` 拆分出 `note_boundary`（边界推进与
入栈解耦，serde 序列化兼容）；lib.rs `apply_ops` 新增 System 分支
（bump revision + log.record + push_event + note_boundary，仅跳过
push_entry/coalesce_into_last）。零新依赖，op-first 契约未触碰。

**实测（10MB 渐进开档，G2）**：

| 指标 | R20 基线 | R21 实测 | 变化 |
|---|---|---|---|
| history.undo_entries | 15 | **0** | ingest 全部退出 undo 栈 |
| wasm 线性内存 | 509.2MB | **425.4MB** | **-83.8MB** |
| usedJSHeapSize（precise） | 567MB | 484.1MB | -83MB |
| JS 活数据（used - wasm） | 58.7MB | 58.7MB | 不变 |

省出 83MB **远超** R20 估计的 ~24MB 活数据：undo 双份（ops+inverses）
消失之外，ingest 期不再逐段克隆 history，分配器高水位/碎片（R20 的
~400MB 分量）同步回落——R20 优化路径 1 的连带收益。

**桥契约勘误（G2 适配时发现）**：`__bridge.undo()` 返回值是 CM6 键位
契约（恒 true 消费按键——"没有 CM6 history 可退"），**不是撤销是否
发生**；核侧 no-op 判据 = revision 不前进 + 无 source=undo 事件。
G2 断言已按此改写（undo_entries==0 + revision 恒常 + 无 undo 事件，
ok_undo 作键位契约透明记录）。

**验证**：cargo test 16 套件 112 用例全绿（新增
`system_apply_ops_filtered_from_undo_stack` /
`extend_not_undoable_and_system_boundary_breaks_group` /
`history_json_roundtrip_with_system_boundary_state`，重写
`extend_undo_reverts_only_last_chunk` → ADR-B4 新语义）；
W1-W13+G1/G2 15 组全绿；W4 typing p95 13.5ms、G2 渐进 TTI 198ms
（与 R18/R20 基线一致，零回归）。

## R22 候选清单（R21 收口后登记；R22-a 执行后重排）

1. ~~Arc<str> op 文本共享~~ → **已完成（R22-a）**：实现 + 回归证明落地，
   但 10MB 实测收益仅 Δ-0.6MB（噪音级）——R20 的 20-30MB 估算撤回，
   根因见 R22-a 段。
2. **apply 事务 backup 克隆治理**（新登记，升为内存优化主项）：
   apply_ops 每事务 `tree.clone()` 整树备份是分配器高水位主驱动
   （10MB 渐进开档 16 次 × 全树 Rope/slots/id_index 克隆）；候选路径：
   求逆回滚（inverses 已算出）/ 受影响子树范围备份。回滚正确性
   （部分应用失败时的逆序回放）需 ADR 先行。
   【#220 注记（2026-11-05，审计 P1-27 对账）】研究仓 ADR-B5 自述
   「已裁决并落地（R22-b）」，经静态比对（原型母本 + mdcore main）
   核实**未回流**——`tree.clone` 备份与旧逆序构造两仓均在。#220 先
   落地其**正确性半边**：逆组改为组间逆序组内保序（修复非叶删除
   undo 硬失败/静默错挂，r25 复现测试锁定）；**内存半边（废备份）
   本候选仍在册，未动**——#220 验收红线明确不把「用 tree.clone」
   当失败判据、不做 undo 性能重写。
3. **ingest 预分配 hint**（R20 路径 3；降级——高水位主驱动是 2 不是碎片，
   待 2 落地后复测构成再定）。
4. **真 IME 人工验收**（协议已成文，待真实输入法环境；能自动化部分
   先行——CDP Input.imeSetComposition 可试）。
5. **Slint/wry 宿主壳**（主档 R10 蓝图已在 shell/shell-native crate；
   ~~合规化改造完成后启动~~ → **✅ 合规化已完成（2026-10-09），宿主壳解除阻塞**。
   阶段 RC 记录：ADR-C1 闭源最严姿态；四路审计全 CLEAN（GPL 上游/解密
   专有源/未发布竞品二进制/SBOM+专有 d.ts）；产品代码已迁入独立合规仓库 `mdcore/`
   （denylist 门禁 `compliance/scan.sh` 与 cargo test 同级双绿，
   shell-native 事件名 md-inject* → mdc-inject*，113 cargo 用例全绿）。
   **宿主壳开工时新增门禁：启用 wry/tao/slint 后重跑 SBOM 并更新
   THIRD-PARTY-NOTICES**（compliance/COMPLIANCE.md §4）。）。

## R22-a：Arc<str> op 文本共享（完成 + 估算撤回）

**变更**：`Op::InsertBlock.text` / `Op::ReplaceText.insert` String → `Arc<str>`
（`op::OpText` 别名）；serde 增 "rc" feature，JSON 形状不变（旧 JSONL /
历史 JSON 兼容）；构造点 src 8 处 + tests 10 处；新增回归测试
`op_text_shared_across_log_events_and_clones`（Arc::ptr_eq 三路共享 +
JSON 往返）。树层（Node.text = Rope）与 NodeData 不变；SetMark.kind
（微小字符串）保持 String。

**测量（10MB 渐进开档，G2）**：used=483.5 = JS 58.7 + wasm 424.8
（R21 基线 484.1 = 58.7 + 425.4，**Δ-0.6MB ≈ 噪音**）；TTI 190ms（基线
198）；typing p95 14.7ms（基线 13.5，抖动内）；cargo 16 套件 113 用例
全绿（+1）；W1-W13+G1/G2 15/15。

**估算撤回与根因**：R20 估"省 20-30MB（rope/log/history-inverses 三份
合一）"的两个前提已失效——① R21/ADR-B4 后 System 事务不入 undo 栈，
inverses 份消失；② 宿主取走事件后即弃，静止态 log 是文本唯一持有者，
单份无可共享（共享只在 apply 窗口内瞬时成立：ops + log 克隆 + 事件同
行）；③ 高水位主驱动是 apply_ops 每事务整树 backup 克隆，与 op 文本
无关。R20 路径 2 定谳：**架构正确、收益证伪**。

**保留理由（不回退）**：a) 多消费者事件流（多视图/多 Agent，主档 R10
蓝图）将延长事件持有期，共享语义为未来买单；b) Op 枚举缩小 8B，37 万
op 的 Vec 与克隆同步受益；c) 零新依赖、113 测试全绿、15/15 无回归，
回退徒增 churn。

## RC：全仓合规化前置节点（2026-10-09 完成，宿主壳前置门禁清空）

**姿态（ADR-C1，外部设计档案）**：闭源最严——GPL/专有/上游特有命名
零容忍；行为锚点 + 白名单许可依赖可复用；产品代码与本仓自含 git 历史
（3312873 审计前原样 import → c1bd5e9 合规清洗+基建，diff 即清洗清单）。

**四路审计全 CLEAN**（denylist 本仓 compliance/denylist/，底稿仓外
mdcore_audit/）：

| 路 | denylist | 结论 |
|---|---|---|
| GPL 上游（实抓上游源建表） | zed.txt（STRICT ~195 + NS 25） | 0 命中 0 提及 |
| 解密专有源 | typora.txt（298：CSS/标识符/IPC/UI） | 0 violation |
| 未发布竞品二进制 | delta.txt（302：DDL/符号/crate/串） | 0 violation |
| SBOM+资产+专有 d.ts | obsidian.txt（136）+ sbom.json | 62 包全白名单、零资产、0 命中 |

**清洗**：shell-native 事件 `md-inject*` → `mdc-inject*`（唯一动码项，
零行为变更）；denylist 泛词提纯（Workspace/Plugin/Menu/Modal/Notice/
Setting/base_revision → WATCH 段，分类存档）。

**验证**：cargo 16 套件 113 用例全绿；wasm32 check 通过；
`bash compliance/scan.sh` strict 0 / ns 0（evidence/scan-latest.txt）。
**持续门禁**：scan.sh 与 cargo test 每轮收口双绿；宿主壳启用
wry/tao/slint 后重跑 SBOM + 更新 THIRD-PARTY-NOTICES。

## RC 收口增注（2026-10-10，成果推送）

mdcore 产品仓 remote 已建（github.com/mmqz/mdcore，私有）：compliance-baseline
分支承载 COMPLIANCE.md + 发布形态三件套；main 待开发环境首推产品代码主体。
独立复核轮次产出归档 compliance_rounds/（round1+round2）已入分析仓推送。
真机 IME 人工验收的自动化先行部分转 R23-a（CDP Input.imeSetComposition）。

## R23-a：IME 自动化验收（CDP 真实输入法模拟）（2026-10-10，R22 候选清单第 4 项自动化先行）

**测试面**：`web/tests/ime_cdp.py`（新，28 checks，证据
`tests/results/results_ime.json`，两次复跑稳定全绿）——Playwright +
`context.new_cdp_session`，以 CDP `Input.imeSetComposition` /
`Input.insertText` 经 Chromium InputMethodController 驱动**真浏览器组合流**
（真实 compositionstart/update/end + beforeinput 事件链 + 原生预编辑 DOM
变更），覆盖 W3/W9 合成路径不可达的 OS 级事件语义。方法探针
`tests/probe_ime_cdp.py`（证据 `results/probe_ime.json`，先探后写，
断言全以实测语义为准）。

**协议要点（chromium-1243 实测）**：`imeSetComposition` 形参为 `text`
（非旧协议 `compositionText`；playwright protocol.d.ts 证实）；
`Input.imeCommitComposition` 在本 build 不存在；**提交** = 组合中
`Input.insertText`（自动 compositionend，data=提交串）；**取消** =
`imeSetComposition text=""`（compositionupdate('') + compositionend('')，
零残留）。默认 headless shell 即支持，无需 new headless。

**三阶段语义实测结论**（我方实现 = CM6 原生组合通道 + 桥快路径/兜底；
main.ts 无组合专用代码，语义由 CM6 MutationObserver 逐次吸收决定）：

| 阶段 | 实测 |
|---|---|
| 组合期 | 每个预编辑 update 经 CM6 逐次 dispatch → 桥快路径落 **1 个 replace_text op（无缓冲）**；首 op 开新 undo 条目（coalesced=false），后续 op 1s 合并窗并条（coalesced=true），**合并条目不 bump revision**（revision 持平，apply_ops 合并分支只 log.record(self.revision)） |
| 提交 | Input.insertText 单次 compositionend，落**单个** replace_text op = 相对预编辑的最小 diff（IC1 "ni"→"你" 全量替换 op；IC2 提交词比预编辑长时仅补差量 ins="世界"） |
| undo | 整段组合+提交 = **单个 undo 条目**（ADR-B2 合并窗），一次 undo 全回退（view+core 收敛 doc==md）。例外：跨行提交（提交通道带 \n）触发兜底 sync（lastFastBail='newline'），核内解析器按行切块产生结构性 op → 不并条、undo 分次回退（先撤提交回预编辑态、再撤预编辑）——符合 Kind::Structural 破并条语义 |
| 取消 | 组合中清空 = insert+delete 两 op 并条入同一条目，undo 内容中性；预编辑零残留、composing 清位 |
| 非 BMP | 🎉 组合 + 🎉🚀 提交 + 紧贴快路径打字：op 偏移全为码点、blocks_json text == doc 行逐码点相等（**W13 单位宪法在真实事件流下成立**）；单条目一次 undo 全回退 |

**与合成路径（W3/W9）差异对照**（证据 = results_ime.json
`DIFF_baseline_synthetic`（W3 式同页采集）vs IC1 事件链）：

| 维度 | synthetic（手动 dispatch CompositionEvent + execCommand） | CDP 真实路径 |
|---|---|---|
| compositionstart | 手动 dispatch，浏览器组合状态由 CM6 侧独立记账 | imeSetComposition 自动触发，先于一切 DOM 变更 |
| compositionupdate | 手动 dispatch，1 次、data=最终串 | 逐 update 自动触发，data=全量预编辑串（n/ni/你好…） |
| beforeinput | 实测 0 条（execCommand 不产生） | 每次组合变更 1 条 `inputType=insertCompositionText`；提交步亦然（**非** insertText） |
| 提交语义 | compositionend 手动 dispatch data=''，无替换语义，DOM 靠 execCommand 先插入 | insertText 自动 compositionend(data=committed)，Chromium 原子替换组合范围，CM6 收最小 diff |
| 取消 | 手动构造插入+删除再 end（W9 路径） | text='' 原生取消，零残留 |
| op 流 | 整个合成组合 = 单个 insert op | 每 update 一 op + 提交一 op（1s 合并窗并成单 undo 条目） |
| 组合中点击/焦点转移 | 未测 | CDP 虚拟组合不受 CDP 鼠标影响（见下行限制） |

**CDP 路径限制（1 条）**：CDP 鼠标/选区事件不触发 blur、不驱动 IME
线程——组合中点击别处**不会**强制提交（composing 保持、无
compositionend；显式 Input.insertText 仍可正常收尾无重复）。真实 IME 的
焦点转移强制提交无法自动化复现，保留人工项。

**门禁（收口全复跑）**：`cargo test --workspace` 16 套件 **113 用例**
全绿；`web_smoke.py` **15/15**（W1-W13+G1/G2）；`ime_cdp.py` **28/28**
（两次复跑）。零回归、零代码语义变更（本轮纯测试与文档）。

**剩余人工项（web/README §6 协议不变）**：真机物理输入法（macOS 拼音/
日文 ROMAJI、Windows 微软拼音、Linux iBus）、候选词翻页期间预编辑下划线
区、物理 Esc 键取消、方向键/点击别处强制提交、code fence/表格/quote 前
缀内组词、>30s 长句连打的合并窗体验。真机验收保持登记待办。

## R23-b：块指纹 / 结构匹配（P1-7 定谳，2026-10-10）

**交付**：`crates/mdcore/src/fingerprint.rs`（826 行）+ `tests/r23_fingerprint.rs`（15 条
语义锁）+ `decisions/ADR-B6-block-fingerprint.md`。核心 = 两层指纹（结构 FNV-1a 0xB7 域：
kind+attrs+子数+子结构递归；内容 0xC1 域：空白折叠文本逐码点，非 BMP 安全）+ 三级匹配
（manifest 快路径全有全无核验 → 结构锚 front/back 走查 → 中段 Exact 配对/窗内相似 0.7·8/
拆并归组 join 核验）+ 五类报告（preserved/moved/edited/added/removed + tier stats，
R23-c 消费契约：祖先搬移子内 preserved、未配对只报子树根）。

**实现期修正**（子代理初版 4 处测试红 → 根因分诊）：①真 bug：preorder_with_paths 顶层
路径给 `[]` 与匹配器 `[i]` 约定不一致 → manifest 快路径 moved/edited 归类全部失准（修复
后快路径重写块 = edited 身份保住）；②测试助手 ids() 只收顶层 → 嵌套场景身份全保断言
误报（改先序全树）；③窗宽锁测试期望 1/1 违报告语义（未配对根各报 1 条 = 9/9 才对，
且两端配对是 anchor 层）——期望修正；④perf 测试 `str::replace` 误用（不可变方法未回赋，
perturbed==doc 导致 edited=0 假失败——回赋 + 双断言防回归）；⑤性能门改 match-only
（1MB 解析 183ms dev 归 G2 门，匹配 197ms dev ≈13ms release，阈值 400ms 抓 O(n²) 退化）。

**门禁**：cargo --workspace **131 用例全绿**（113 基线 + 15 语义锁 + 3 单元）；wasm32
check 过；产品面品牌词扫描 0（fingerprint.rs/r23_fingerprint.rs 零 delta 等专名）；警告
面零新增（2 条既有 wasm-bindgen 宏告警）。确定性契约：无 HashMap 迭代序、平局取最小序位。

**移交 R23-c**：`Core::match_external_markdown(md, manifest)` / `fingerprint_manifest()`
（存侧车）/ `BlockMatchReport` 五类清单 → 重载冲突状态机（kept/edited/added/removed
四象限 UX）；局限四条（ADR-B6 §4）决定 UX 文案口径。

## R23-c：外部文件重载（P1-8 核侧，2026-10-10，ADR-B7）

**交付**：`crates/mdcore/src/external.rs`（FileStamp 探针 + plan_reload 三态决策 +
Core::adopt_external 身份保持采纳）+ `tests/r23_external.rs`（14 条语义锁）+
`decisions/ADR-B7-external-reload.md` + 两处核面小扩展（`History::clear_interaction`——
take-external 交互史作废 + 合并窗状态重置；`BlockMatchReport::has_changes`——决策口径
区分 preserved 与真变化）。

**op 合成定谳**（ADR-B7 §3）：全量重建而非增量 diff-to-ops——RemoveBlock[0]×n + 先序
InsertBlock 携带映射 id（preserved/moved/edited 旧 id / manifest 可信 added 声明 id /
其余现场分配），与 undo 复原 build_rebuild_ops 同款物化形状，log 重放不变量保持；
System 事务（ADR-B4）失败回滚 = 整树备份恢复（`apply_ops` 事务入口
`backup = tree.clone()`、任一 op 失败 `self.tree = backup`）。#220 勘误
（审计 P1-27）：本行原文「失败走 ADR-B5 求逆回滚」不实——研究仓
ADR-B5（R22-b，2026-10-09）声称的 `apply_batch_invert_rollback`/
「三处 tree.clone 全消」从未回流 mdcore（原型与 main 静态比对均无
此实现）；其可落地的**序修正半边**（逆组组间逆序组内保序）已由
#220 落地（r25 复现锁定），但**备份机制未换**——backup 克隆的内存面
仍按 R22 候选 #2 在册治理。

**实现期修正**：①manifest 快路径可信 added 计入 stats.manifest 桶（原只计 resolved
配对，快路径用量被低估）；②重排/嵌套测试的身份断言改集合比较（文档序本就变，先序
比较误报）；③drains 测试两次击键落 1s 合并窗并条——断言口径改 can_undo。

**宿主流程**（R22-d/e 宿主壳接线用）：stamp 捕获（开档/存档）→ watcher/聚焦比对 →
stale 读盘 → match_external_markdown 预览 → plan_reload → Noop 忽略 / SilentAdopt
adopt / OfferChoice 弹冲突（keep-mine 不 adopt；take-external adopt；merge=v1）。
弹窗文案必须写明 take-external 会放弃未存改动且不可撤销（ADR-B7 §4.2）。

**门禁**：cargo --workspace **145 用例全绿**（131 + 14）；wasm32 check 过（external.rs
零 std::fs，核不碰文件系统）；产品面品牌词扫描 0。

**登记（不阻塞）**：全量重建 log 内存 ~+1× 文档字节（R20 路径 3 同族杠杆，10MB 高频
重载场景再议）；merge 与区域级静默合并留 v1。

## R23-d：宿主↔桥↔核契约冻结（2026-10-10，CONTRACT.md v1）

**交付**：`projects/prototype/CONTRACT.md`（冻结 v1：§1 核/mdshell 编译面 + 语义锚点表、
§2 宿主壳接线（mdcore://app / mdc-inject* / 文件生命周期 / R23-c 外部重载流程）、
§3 web 桥 window 面逐签名）+ 母本侧契约补齐（main.ts 新增 `__inject`/`__inject_chunk`
入口 + `mdc-inject*` 监听器 + `__contractVersion=1` + 直启流程补装 `__memStats`——
契约门逮到的真实面缺口；重建 = 旧 EditorView.destroy() + 全量 window 面重装）+
shell-native 蓝图事件名 md-inject* → mdc-inject*（对齐产品仓 RC 轮命名域，漂移清零）。

**双侧契约锁**：Rust `crates/shell/tests/contract_freeze.rs` 3 条（蓝图词面锁：mdc-*
在、md-inject 禁复活；CONTRACT.md 存在性/版本/关键面词面；mdshell OpenProgress 三变体
进度序列 + 身份复原语义）；web `web/tests/contract_gate.py` 13 checks（启动面类型全谱、
__bridge 清单、注入面事件族+全量重建+渐进通道、undo 键位契约恒 true（R21 勘误口径）、
单位宪法换算往返、事件排空时机（构造器首渲消费 ingest 事件=boot 同语义；编辑 op 事件
渲染同步时到达宿主））。证据 results_contract.json。

**实现期修正**：①渐进通道测试文档自身 2 块误断 1 块；②TCPServer 需 allow_reuse_address
（web_smoke 同款）；③直启 __memStats 缺失=门禁逮到后补装；④__perf 是 object 非 function
（类型检查排除表）；⑤事件排空断言改编辑驱动（初始 ingest 事件被构造器消费属正确语义）。

**门禁**：cargo --workspace **148 全绿**（145+3 契约锁）；web smoke **15/15**（main.ts
变更零回归）；契约门 **13/13**；wasm32 check 过；产品面品牌词 0。

**升版纪律**：契约面任何破坏性变更必须 __contractVersion 升版 + CONTRACT.md 更新 +
双侧锁测试同步改——三处不同步 = 漂移复活。

## R23-e：阶段收口（2026-10-10）

**门禁终态（全复跑）**：cargo --workspace **148/148**；wasm32 check 过；web smoke
**15/15**；契约门 **13/13**；IME CDP **28/28**；产品面品牌词 **0**；R23 五轮依赖零增
（Cargo.lock/package-lock.json 在 e4ed77a..HEAD 零 diff → THIRD-PARTY-NOTICES 与
R22-c SBOM **无需再生成**，口径交叉一致）。

**销项**：主档 §28 P0 1-3 ✅（obsidian recon / ADR-View-01 / W1-W13+G1/G2 实测）；
P1-7 ✅ R23-b（ADR-B6）；P1-8 ✅ R23-c 核侧（ADR-B7）。R22 候选 4 自动化先行 ✅ R23-a。

**遗留登记（非本阶段可闭环）**：真机物理输入法/视觉/对话框人工验收（协议已成文，
待物理环境）；定名后商标检索（TRADEMARK_RULES §4）；R20 路径 3 ingest 预分配（待
backup 克隆治理（R22 候选 #2）落地后复测构成；#220 勘误：原文「产品仓
R22-b 合流」不成立——R22-b 系研究仓单方声称「已落地」、经比对未回流
mdcore）；take-external 全量重建的 log 内存杠杆（10MB 高频
重载场景再议，ADR-B7 §4）；merge 与区域级静默合并（ADR-B7 v1）。

**R23 阶段结论**：宿主壳后的产品闭环第一段完成——IME 真实语义背书、外部重载身份
保持、P1-7/8 定谳、契约面冻结。产品仓（mmqz/mdcore）合流 R23-b/c/d 母本增量后即可
进入 R24（打包分发 / 多窗口 / 设置面，按主档远期路线）。

## R24-a：设置面核侧（2026-10-10）

**交付**（主档 §31 R24-a 四条机制合同全落地）：
- `mdshell::settings` 模块：四层 schema（editor/ai/storage/appearance）+
  TOML sidecar 持久化（原子写 tmp+rename，与 md+meta sidecar 纪律一致）。
- 机制：默认值合并 / 未知键+注释往返保留（toml_edit 格式保持编辑）/
  schema 版本迁移框架（v0 无版本键 → v1 补键）/ 损坏降级
  （文件级 FileDegraded 可修复 / 键级 KeyDegraded 该键回默认 /
  FutureSchema 拒写保护防降写毁数据）。
- 契约 v1 → v1.1 minor 升版（三处同步完成）：window 面增
  `__settings` / `__setSetting` / `mdc-settings-changed` 事件；
  `__contractVersion` 1 → 1.1；CONTRACT.md §4（默认值表双侧一致锚）；
  contract_gate.py 13 → 16 检；contract_freeze.rs 增 v1.1 词面锁。
- 语义锁 `crates/shell/tests/r24_settings.rs` 14 条：默认合并 / 未知键+
  注释往返 / 损坏降级修复 / 键级降级（类型+值域）/ v0→v1 迁移幂等 /
  未来 schema 拒写 / set 封闭键集+校验+doc↔快照↔重载三方一致 / 文件面
  （Created/原子写无 .tmp 残留）/ **跨 crate 常量锁**
  （settings 默认 undo_coalesce_ms == mdcore::history::COALESCE_MS）/
  JSON 形状往返（web 面锚）。

**新增依赖**（全 MIT/Apache-2.0 双许可白名单族）：toml_edit 0.25 + 传递
7 项（indexmap/equivalent/hashbrown/toml_datetime/toml_parser/toml_writer/
winnow）；serde/serde_json 复用既有（mdcore 已在 SBOM）。NOTICES 草案随
本轮在 compliance-baseline 分支增补。

**门禁**：cargo --workspace **164/164**（148 R23 终态 + 16 新增：14 语义锁
+ settings 单元 1 + 契约 v1.1 锁 1）；wasm32 check 过；scan.sh 产品面
0 命中；web 构建 OK；契约门 **16/16**；web smoke **15/15**；IME CDP
**28/28**（main.ts 改动零回归）。

**决策记录**：① schema_version 为保留键，set 面不可写（迁移框架独占）；
② 未来 schema 文件 load 全默认 + save 拒写（防降写毁新版本数据）；
③ 值域校验双侧同表（Rust set()/load 降级与 web 校验器共用 CONTRACT §4
默认值表）；④ ai 层 v0 仅 commands_enabled（宿主 feature flag），键集
扩充走 schema 版本迁移；⑤ web 快照 R24-d 接线后由宿主注入权威值。

## R24 质量审计轮（2026-10-10，用户裁决开工序第一项）

**方法**：双绿门禁复跑（基线确认）→ 逐模块代码勘察（lib/tree/op/history/
settings/external/fingerprint/wasm 桥/mdshell）→ 边界探针（先探后写，
复现后修复，探针升级为语义锁）。门禁基线：cargo 164/164 + scan 0
（与 R24-a 收口一致）；审计后：**cargo 171/171**（+7 锁）+ scan 0 +
wasm32 check 过 + clippy 零新增告警（2 条既有 wasm-bindgen 宏告警）。

**R24-a1 修复（settings 写读形态不对称——P1 级静默丢设置）**：
`set()` 直写 `doc[section][key]` 在节整段缺失时（缺键=默认是设计行为），
toml_edit 的 IndexMut 把 `Item::None` 自动升格为 **InlineTable**
（toml_edit index.rs `if let Item::None = *v` 分支），产出
`editor = { undo_coalesce_ms = 500 }`；而读侧 `as_table()` 只认标准表
→ **值落盘但下次 load 静默回默认**（用户设置丢失，运行时内存值与
磁盘态分叉直到重启）。修复双面：写侧 `ensure_section`（缺失→标准表；
inline→值保留升格；异形→空表重建）+ 读侧 `item_of` 改 `as_table_like`
（TableLike 覆盖 Table+InlineTable，手写 inline 节容错可读）。
锁 ×3（r24_settings.rs §10）：节缺失 set 产标准表+重载存活 / inline
升格保值（含未知键）/ 读侧 inline 容错。

**R24-a2 修复（SetMark 求逆不对称——undo 破坏「精确恢复前态」不变量）**：
`invert_op` 对 SetMark 无条件翻转 `add`，与注释承诺（"no-op 的逆 =
自身 no-op"）不符：独立条目中的冗余 add（apply no-op）undo 会删掉既有
mark；移除不存在的 span（apply no-op）undo 会凭空造出 mark。修复：逆的
方向由 **state-before 的 span 存在性**决定（no-op → 同向幂等；有效变更
→ 反向）。合并窗内连击 add+add 并条后的单次 undo 撤整组（组前状态）
是正确语义，作为对照锁固化。附带加固：SetMark 区间校验（start>end /
end>len → Reject，与 ReplaceText 同款），Reject 后树零污染。
锁 ×4（r24_quality_fixes.rs，新套件）。

**审计结论（未动码项）**：lib.rs 事务管线/回滚/ADR 引用完备；tree.rs
arena+环检查纪律清晰；history.rs 合并窗边界语义自洽（R12 双端 View +
R21 System 边界）；fingerprint.rs 确定性契约落实（无 HashMap 迭代序、
最小序位破平局）；external.rs adopt 失败回滚 + clear_interaction 时序
正确；wasm 桥零宏魔法纪律保持。mdshell::settings 的 save() 无 fsync
（rename 原子性足够、设置文件非文档数据，与 md+meta sidecar 同款纪律，
不升级）。

## R24-b：多窗口核侧（2026-10-10）

**交付**（主档 §31 R24-b 机制合同全落地）：
- `mdshell::session` 模块：`SessionManager`（窗口→会话映射 BTreeMap +
  MRU 焦点序 + 资源上限 `DEFAULT_MAX_SESSIONS=16`）+ `SessionRecord`
  （会话持久化纯数据 JSON，schema v1）+ `WindowId`（宿主分配透传）。
- 机制：一窗一档（同 document_id 二开 → `DocumentAlreadyOpen` 携带持有
  窗口，宿主 reveal-existing；匿名文档多开不受限）/ 超限显式 Err 不静默
  驱逐（驱逐 = 宿主 UX，核供 `dirty_windows`/`windows_in_focus_order`
  决策输入）/ 焦点序确定性（记录按焦点序导出，恢复末位获最终焦点）/
  未来 schema 记录拒绝（宿主空会话冷启动——可再生缓存，与 settings 的
  FutureSchema 拒写不同级）/ 无路径会话不入恢复集（v0 无草稿暂存，
  诚实语义）。
- **Core 脏标记**（置于核：单一变异入口挂钩）：`is_dirty`/`mark_saved`；
  粘性语义（View/Ai/Undo/Redo 置位；System 装载不置位；adopt_external
  成功清位——缓冲==外部磁盘内容；undo 回已存内容仍报脏=安全向假阳性）。
  `plan_reload(local_dirty)` 自此有权威取值；`mem_stats()` 增 dirty 字段。
- `DocumentSession` 增 `path: Option<String>`（宿主域透传）+
  `new().with_path()` 构造器。
- 契约：CONTRACT.md 新增 **§5 会话面**（纯宿主面 Rust↔Rust——window 面
  零新增，`__contractVersion` 保持 1.1；这一"不升版"裁决显式在档并由
  词面锁锁死，防误升版/漏升版两向漂移）；§1 核面增脏标记行；§4
  `storage.session_restore` 消费联动。contract_freeze.rs 增会话面词面锁
  （§5/SessionManager/DocumentAlreadyOpen/DEFAULT_MAX_SESSIONS/
  snapshot_record/mark_saved + 编译期常量双锚）。

**语义锁**：`crates/shell/tests/r24_session.rs` **10 条**——生命周期
MRU 序（含窗口 id 复用）/ 上限执行与释放 / 重复+未知窗口拒绝 / 一窗一档
（同 id 二开拒绝→close 后重开→匿名多开）/ 反向映射 / Core 脏标记全路径
（View/System/undo/adopt 四象限）/ dirty_windows BTreeMap 键序 /
记录往返+焦点序+pathless 跳过 / 未来 schema+畸形拒绝 / 同态逐字节确定
性 / open_document→with_path→open 恢复链路集成（含 meta 双源同档拒绝）
+ session.rs 单元 1 条（默认上限常量）。

**门禁**：cargo --workspace **183/183**（171 质量审计终态 + 12 R24-b）；
wasm32 check 过；scan.sh 0 命中；clippy 零新增（2 条既有基线）。零新依赖
（serde 复用既有——SBOM/NOTICES 无需再生成）。

**决策记录**：① 一窗一档 v0（放宽为多窗共享属契约级变更走升版纪律）；
② 会话记录 = 可再生缓存（非用户数据）：未来 schema 拒绝即弃、冷启动，
不设拒写保护；③ 脏标记粘性语义（假阳性安全向：宁可多问一次存盘）；
④ adopt_external 清位（缓冲==外部内容），Noop 早退路径不清（无变更
无采纳，脏态原样交宿主 UX）；⑤ 资源上限不静默驱逐——驱逐策略宿主域。

## R24-c：打包分发基建（2026-10-10）

**交付**（主档 §31 R24-c；产物实体验证按计划留 R24-d 开发环境轮）：
- **CARGO_PKG_VERSION 单源**：根 Cargo.toml 新增 `[workspace.package].version`
  （唯一手写点），四 crate 全部改 `version.workspace = true`（禁止独立
  声明）；运行时唯一出口 = `mdcore::VERSION`（env!("CARGO_PKG_VERSION")）。
  发布产物四处注入（deb control / NSIS / Info.plist / AppImage 元数据）
  全部从产物 `--version` 读取，禁止手抄。
- **packaging/ 基建**（名称全参数化，正式定名 = 用户决策 + 商标检索
  强制门未决，不阻塞基建）：
  - `README.md`：定名阻塞纪律（TRADEMARK_RULES §4 五步检索 + 复跑品牌
    词门禁 + 变量表登记三件套）+ 变量表（{{APP_NAME}} 待定名 /
    {{APP_EXEC}}=mdcore 自研命名域）+ 版本单源机制图。
  - `checklist.md`：分发合规核对单六节（A 命名商标 / B 版本产物 /
    C 依赖合规 / D 平台注册面 / E 隐私安全 / F 发布物料），发布前
    逐项打勾，A4 定名门与 B2 版本单源为硬门。
  - 四平台模板：`shared/app.desktop.tpl`（.md mime +
    x-scheme-handler/mdcore scheme 注册 + SingleMainWindow 单实例语义）/
    `appimage/`（AppRun + build 脚本，WEBKIT 合成模式兼容开关）/
    `deb/`（control 模板含 webkit2gtk-4.1 依赖声明 + postinst 注册
    缓存刷新 + 手工布局 dpkg-deb 构建——不引入 cargo-deb，零新增依赖）/
    `nsis/installer.nsi.tpl`（注册表面 + WebView2 Evergreen 检测引导 +
    卸载器）/ `macos/Info.plist.tpl`（文档类型 + URL scheme + 无网络面
    声明）。
- **合规门禁扩展**：scan.sh PRODUCT_PATHS 纳入 packaging/（发布面 =
  产品面，品牌词门禁恒覆盖分发模板）。

**语义锁**：`crates/shell/tests/r24_packaging.rs` **4 条**——版本单源
（workspace.package 与 mdcore::VERSION 一致 + 四 crate 继承声明面 +
禁独立声明）/ 工件在位与命名域合规（mime/scheme/APPID/webkit2gtk
依赖四锚）/ 构建脚本版本注入纪律（--version 读取 + 无内嵌版本串）/
定名阻塞门在档（占位符纪律 + 核对单硬门，定名提交三处一体改写）。

**门禁**：cargo --workspace **187/187**（183 + 4）；wasm32 check 过；
scan.sh 0 命中（scope 扩展后）；clippy 零新增。零新依赖（Cargo.lock
零 diff——版本继承不改变解析结果，SBOM/NOTICES 无需再生成）。

**决策记录**：① 版本手写点唯一化于 workspace.package（crate 独立声明
= 漂移源，锁死）；② 正式名未定不阻塞基建——{{APP_NAME}} 参数化，
可执行名/包名/scheme 用 mdcore 命名域先行；③ deb 走手工布局 dpkg-deb
（不引入 cargo-deb 依赖）；④ NSIS 的 WebView2 引导用微软官方
Evergreen bootstrapper；⑤ AppImage 的 WEBKIT_DISABLE_COMPOSITING_MODE
保守开启，R24-d 实测后按需收敛。

## PDF-a：侦察收口 + 三探针 spike + ADR-P1 选型定谳（2026-10-10）

**背景**：PDF 载体提前为第二载体（用户裁决，主档 §25）；五轮编排
PDF-a..e（md-line/pdf_carrier_plan.md）。本轮 = 侦察收口，无产品仓代码
变更（mdc-pdf crate 属 PDF-b）；分析侧交付存 cleanroom-analysis
md-line/pdf-a/（分区纪律：spike 与策略表 = 分析域）。

**三探针实测**（spike 工程：md-line/pdf-a/spike/；x64 linux 沙箱；
pdfium=chromium/8086 non-V8 构建 + pdfium-render 0.9.4；lopdf 0.45.0）：

| 探针 | 场景 | 实测 | 判定 |
|---|---|---|---|
| 1 打开 | 3 页/500 页 load + 首页文本抽取 | load 0ms（两档）；文本抽取首次 19ms（预热）后 0ms | ✅ 渲染引擎能力面成立 |
| 2 光栅化 | 1200px 宽 Letter 位图 | 单页 4ms；连渲 5 页 17ms（~3.4ms/页） | ✅（PDF-c 性能门前的强正向信号） |
| 3 标注写 | lopdf IncrementalDocument 增量写 Highlight | 原 1026B → 1497B（+471B 追加）；**原字节前缀逐位保持**；lopdf 重载 ✓；**pdfium 交叉读回 type=Highlight ✓** | ✅ 增量语义 + 双引擎互通坐实 |

**ADR-P1 定谳**（md-line/pdf-a/ADR-P1-pdf-engine-selection.md）：
pdfium（渲染/文本/搜索）+ lopdf（标注增量写）双引擎分工；排除 mupdf
（AGPL 禁入）/ pdfium-v8 构建（安全策略 S1.1：JS 引擎不接——non-V8
构建 args.gn 实证 pdf_enable_v8=false + pdf_enable_xfa=false）/ 造轮子
解析器（pdf-line/02 缺口分析）。版本 pin：chromium/8086（SHA256 回填
闭环）+ pdfium-render 0.9.4 + lopdf 0.45.0（crates.io 最新稳定）。
取件脚本 fetch-pdfium.sh（版本 pin + SHA256 验证 + non-V8 断言）随
spike 交付，PDF-b 起并入 mdc-pdf 基建。

**安全策略表 v1**（md-line/pdf-a/SECURITY_POLICY.md）：Acrobat 审计
R1-R6 + pdf-line/03 方法论（限额五层纵深 L0/L2/L3/L4/L7 + 三行为学
❶硬拒绝/❷钳制/❸弃置 + JS 沙箱四件套）直接转化为 mdc-pdf 安全规格：
S1 引擎能力面（non-V8/XFA 不启用/零网络）/ S2 危险对象族打开时检测
（/OpenAction //AA //Launch //EmbeddedFiles //RichMedia——只读告警
不执行）/ S3 限额五层 / S4 沙箱与进程边界（渲染工人池 + panic 隔离 +
增量写/渲染分线程快照隔离）/ S5 加密签名面（仍加密态拒绝增量写——
lopdf 原生 guard）/ S6 已知缺口诚实登记（AcroForm formfill 模块在
non-V8 构建中的存在性 = PDF-b 补探针）。

**门禁**：本仓零代码变更（cargo 187/187 基线不动 + scan 0 不变）；
spike 工程独立于产品 workspace（分析域依赖，不入 SBOM——PDF-b 起
mdc-pdf 才引入产品面依赖并触发 NOTICES 增量义务）。

## PDF-b：mdc-pdf crate 骨架（2026-10-10）

**背景**：pdf_carrier_plan §5 五轮编排第二轮——mdc-pdf 骨架 = 纯 Rust
类型 + 命令边界 + 安全策略 + 协议草案 + 语义锁；引擎（pdfium/lopdf）
PDF-c/d 引入。输入资产：ADR-P1 选型定谳 + SECURITY_POLICY.md v1（数值
权威）+ pdf_carrier_plan 交付定义。

**交付**（crate `mdc-pdf`，crates/pdf；workspace 第 5 成员）：

- **第二 Document Model**：与 mdcore 块核并列、互不嵌套、**crate 间零
  path 依赖**——Source 等共享语义 = **镜像**而非复用（serde 名与
  `mdcore::history::Source` 逐词面一致，`source_mirrors_mdcore_five_values`
  以文件级读锁对齐 mdcore 源面，手法同 contract_freeze 蓝图锁——零依赖
  下跨 crate 镜像的可证路径）。
- 模块八件：`page`（PageIndex/PageCount/PageGeometry/Rotation 归一化
  effective_*——90/270 交换宽高、0/180 不交换；PageSpan 构造即校验
  start<=end，页数越界归命令层）/ `outline`（树 + 深度优先前序迭代器，
  单一排序出口）/ `annotation`（PDF Annot 语义四变体
  Highlight/Note/FreeText/Ink + Origin Human/Ai + patch 面 kind 不可变）
  / `command`（六命令全带 source 溯源 + 五类 typed 错误 +
  SearchOptions/Filter/TextTarget 页粒度）/ `security`（S2 五危险族 +
  S3 Limits 数值表 + 三行为学 enforce 族 + S5 判定 + 稳定理由码）/
  `document`（PdfSession 打开后单形态 + execute 单一调度 + 值时钟注入
  + set_page_text 宿主文本层）/ `protocol`（mdcore://pdf 草案 +
  mdc-pdf-cmd/event 事件族 + CommandEnvelope 信封）/ `engine`
  （DocumentEngine trait，object-safe——PDF-c 绑 pdfium non-V8）。
- 契约：CONTRACT.md 升版 **v1.2** 新增 §6 PDF 面（草案）——window 面
  零变化，`__contractVersion` 保持 1.1（仿 R24-b §5 先例，防两向漂移）。
- README：架构图 + mdc-pdf 行 + 测试计数更新。

**决策记录**：① **零引擎依赖延至 PDF-c**（骨架轮不耦合原生库）：本轮
deps 仅 serde+serde_json（版本对齐既有——Cargo.lock 零解析变更，仅
新增 package 条目，SBOM/NOTICES 无增量义务）；workspace 在无 pdfium
二进制的环境保持全绿，引擎面（渲染/文本/增量写）与骨架面解耦演进；
② v0 引擎数据由宿主注入（open 参数 + set_page_text），PDF-c 换
DocumentEngine 数据源——会话/命令面结构不变；③ 标注 kind 不可变落
类型层（AnnotationPatch 无 kind 字段）+ serde 默认忽略未知字段 = 改
kind 请求退化 no-op（「忽略」语义锁死；PDF /Subtype 语义上不可换装，
改型 = 删旧加新）；④ 会话**值时钟**注入（核不取系统时间——确定性/
可测性/wasm32 兼容，对齐 mdcore 时钟纪律）；⑤ S5 写拒**先于存在性
检查**（策略门最前——不给「库里有没有这条标注」的探测面）；⑥ S3
限额 ❶ 拒绝以稳定理由码进 `SecurityDeny`（错误面收窄五类，无 per-field
变体——L7 作者名/文件大小同面）；⑦ 搜索 v0 = 宿主注入文本的朴素扫描
（Unicode 简单折叠 char 窗对齐——折叠变长字符不致索引错位；snippet
char 边界安全），PDF-c 换 pdfium 文本引擎，命令/载荷形状不变；⑧ 搜索
截断语义 = 收满即停 + 见溢出置 truncated（❷收敛继续 + 告知，不静默
丢弃、不全量扫）；⑨ no-op patch 不推进 modified_ms（时间戳反映内容
非命令次数）；⑩ **S6 AcroForm 探针**（non-V8 构建是否含 formfill
模块）登记为 spike 侧任务（分析域 md-line，不阻塞产品面——PDF-c
渲染轮前完成即可）。

**语义锁**：`crates/pdf/tests/pdfb_skeleton.rs` **32 条**——Rotation
归一化（90/270 交换 + 0/180 不交换 + from_degrees 四值/非法拒绝）/
PageSpan 反转构造拒绝 + 命令层越界拒绝 / 大纲深度优先序（3 层
(depth,title)）+ 空树/单根边界 / Rect 反序拒绝（x/y 两轴 + 退化合法）/
Quad/Point serde 往返 / Annotation serde 往返（Origin::Ai + 词面冻结）/
AnnotId 1 起单调 / patch 不改 kind（JSON 恶意改型 = 未知字段忽略）+
三可变面正例（含 modified_ms 推进）+ 不适用面 no-op（mtime 不动）/
三维过滤正反例 + id 升序确定性 / max_hits 钳制 → truncated（❶空 query
硬拒绝）/ 大小写 Unicode 折叠 + snippet 200 字符钳制与 CJK 边界 /
S5 加密双拒（先于存在性 + 读放行 + 零落库）/ 五危险族 serde 名冻结
（含斜杠形拒绝）/ Source 五值镜像（serde 名 + mdcore 源面文件级对齐）/
协议常量词面（SCHEME/FACE/URL 前缀/事件族）/ 版本单源（workspace.package
+ 继承声明，与 r24_packaging 的 mdcore::VERSION 锁合取 = 跨 crate 同值）/
open 三校验 / 作者名 200 上限（含 CJK 字符计）/ Limits 默认数值 = 策略
表 + enforce 族行为学 / ExtractText 页集形状（All/区间 + v0 空文本 +
注入越界）/ 信封往返 + Command JSON tag 面 / DocumentEngine
object-safe（&dyn 见证 + 三方法签名锚）；以及 pdf-b-1r 审计回归三锁：
serde 面反转 PageSpan 拒收 / serde 面反序 Rect 拒收（含反序 patch
JSON 全链路封死）/ 搜索两模式非重叠语义一致。

**审计轮（pdf-b-1r，独立审查代理）**：1 阻断（PageSpan 派生
Deserialize 绕过构造校验——宿主畸形信封 {"start":3,"end":1} 经
len() 下溢 panic（debug）/ capacity overflow abort（release，实测复现）；
修复 = 字段私有 + 自定义 Deserialize + resolve_span 改走 new()）+
1 重要（Rect 同类绕过：反序矩形可经 patch 静默落库，PDF-d 将产出非法
/Rect；同法修复）+ 3 轻微（搜索两模式重叠语义不一致→统一非重叠并
修 while 重写引入的 continue 标签死循环隐患/Source 镜像锁增严：
rename_all 属性前置断言/enforce_snippet_chars 提出循环）——全部修复，
每项配回归锁（上述 +3）。

**门禁**：cargo --workspace **219/219**（187 + 29 骨架锁 + 3 审计回归锁）；wasm32 check 过；
scan.sh 0 命中（产品面新源 crates/pdf/src + 清单已入扫描范围）；clippy
零新增（mdcore-wasm 2 条 = 既有基线）；cargo fmt 过（mdc-pdf 范围；
基线注记：本沙箱此前未装 rustfmt，既有源非 rustfmt 1.10 形——`cargo
fmt --all` 会重排 30+ 既有文件，本轮按 crate 范围执行以保持 diff 可审，
全仓格式化归 PDF-e 收口或独立格式化提交裁决）。零新依赖（serde 系复
用既有版本——Cargo.lock 仅增 mdc-pdf package 条目，无解析变更）。

## PDF-c：阅读面引擎（2026-10-10）

**背景**：pdf_carrier_plan §5 第三轮——阅读面 = 引擎实装（pdfium 绑定
+ 渲染/文本/几何实数据）+ LRU 页缓存 + 渲染工人（panic 隔离）+ 会话
引擎接线 + 性能门。输入资产：PDF-a spike 绑定范本（bind_to_library）
+ SECURITY_POLICY.md S3-L2/S4 + ADR-P1（pdfium-render =0.9.4 pin）。

**交付**（crates/pdf，feature 纪律 = 默认零引擎依赖不变）：

- `cache.rs`（常开，零依赖）：`PageCache<K>` 字节预算 LRU——HashMap
  键索引 + arena 槽位双向链表（u32 索引，槽 0 哨兵，free 栈复用），
  **O(1) get/put**、命中提权、超预算自尾驱逐返回被驱逐键集、重复 put
  原位更新不重复计费、超大条目自逐（不变量 bytes ≤ 预算恒成立）、
  零 unsafe；
- `render.rs`（常开）：`RenderRequest{page, width_px}` /
  `RenderedPage{width_px, height_px, bgra}`（BGRA 非预乘，缓冲长恒
  w×h×4）+ `Rasterizer` trait（object-safe，S4 工人以对象形态持有）；
- `worker.rs`（常开）：`RenderWorker<R>` 专用渲染线程 + mpsc 双通道 +
  停机令牌（drop 发信号 + join 不悬挂）+ 取消令牌（❸弃置语义：仅作用
  于未开跑的排队作业，不假装能杀线程）+ **panic 隔离**（S4.2：作业体
  catch_unwind(AssertUnwindSafe) → `CommandError::RenderPanic`，一坏页
  不毒全档，工人存活后续作业可用）；回执合同 = 提交序 FIFO + id 关联；
- `engine_pdfium.rs`（feature `engine-pdfium`）：`PdfiumEngine` 实现
  `DocumentEngine` + `Rasterizer`——`load_pdf_from_byte_vec` 全内存
  加载、page_geometry（MediaBox + /Rotate）、extract_text（逐页
  PdfPageText）、outline（书签树，越界目标丢弃）、rasterize（宽驱动
  等比 + S3-L2 ❷钳制：单边 8192 / 总像素 64M，超限等比收缩，生效尺寸
  以 RenderedPage 报告为准）；
- `document.rs` 增量：`attach_engine(Box<dyn DocumentEngine>)` /
  `detach_engine`——引擎在场则 ExtractText/Search/Outline/几何查询走
  实数据（search 引擎路径复用注入路径的收集器 = 限额/摘录/截断语义
  单一出口）；无引擎 v0 注入路径原样；结构面变更：`Clone` 移除（pdfium
  文档句柄不可语义复制）+ 手写 `Debug`（在档 minor 增量）；
- `CommandError` 增 `RenderPanic` / `Engine(String)`（**minor 增量不
  升版**——错误面仍只序列化不反序列化；`Engine` 受限构造：crate 内经
  `CommandError::engine` 规整——空串回退 + 512 字符 char 边界钳制）；
- Cargo.toml：`pdfium-render = { version = "=0.9.4", optional = true,
  default-features = false, features = ["pdfium_latest", "thread_safe"] }`
  + `engine-pdfium = ["dep:pdfium-render"]`，default = []。

**实测性能表**（x64 linux 沙箱，debug 测试 profile——pdfium 侧为原生
C++（release 品质），Rust 侧热路径无逐像素处理，数字对渲染面有代表性；
large.pdf = 500 页 Letter；pdfium = chromium/8086 non-V8）：

| 指标 | 载荷 | 实测 | 断言阈值（=实测×3 对齐 1/2/5 序列）|
|---|---|---|---|
| 打开 TTI（5 次取中位）| 500 页全内存加载 | **0.18ms**（max 3.41ms = 首次装载抖动；断言面 = 中位）| 1ms |
| 首页渲染（冷工人）| 1000px | **8.2ms** | 25ms |
| 顺序渲染 50 页 | 800px（Letter 等比 1035px 高）| **p50 0.9ms / p95 1.1ms** | p95 ≤ 5ms |
| 随机翻页：未命中 | 800px 工人渲染+入缓 | **中位 2.1ms** | —（对照面）|
| 随机翻页：LRU 命中 | 20 页整预算缓存 | **中位 5.1µs**（≈400× 快于未命中；二次取页 rasterize 调用 = 0）| ≤ 20µs 且 < 未命中/10 |
| 逐页文本抽取吞吐 | 500 页全量 | **30-37ms（≈14-16k 页/s）** | 100ms |
| 会话端到端冒烟 | 500 页全档 ExtractText / 定点 Search | 16-43ms / 26-37ms | 无阈值（记录面）|

**决策记录**：
① **image 取舍——零 image crate**：pdfium-render `default-features =
false`（默认特性含 `image_latest`，会拖入 image 全家）；渲染走低层
`PdfBitmap::from_bytes`（调用方缓冲，`FPDFBitmap_CreateEx`）+
`render_into_bitmap_with_config`（`FPDF_RenderPageBitmap`）+
`as_raw_bytes` 拷出——BGRA 四字节恒 4 对齐（stride = w×4，无行尾填充），
`set_reverse_byte_order(false)` 保原生 BGRA（默认 true 会翻成 RGBA）。
收益 = 依赖面不进 image 系 + Rust 侧无逐像素转换（debug profile 的
性能担忧面直接消失）；
② **库定位序**：env `MDC_PDFIUM_LIB` → 候选清单（**exe 同目录部署
位**（运行期 `current_exe()` 推导，#210 真实环境验证收口——INSTALL.md
「放 exe 旁」的显式候选，此前仅能靠系统绑定的 LoadLibrary 隐式搜索序
兜底、从未验证且定位详情不可诊断）→ 产品仓 `bin/lib`
（fetch-pdfium.sh 取件位，PDF-e 接线）→ 分析仓 spike `bin/lib`
（门禁环境）→ crate 本地）→ 系统（bind_to_system_library）；
绑定失败 = `Engine` 错误**携带全部已尝试路径**（门禁可诊断，不静默
降级）；进程级单例（pdfium-render 绑定表为全局 OnceCell——本 crate
`PDFIUM: OnceLock<&'static Pdfium>` + Box::leak；绑定失败不缓存，下次
重试；并发首个 open 经互斥锁仲裁）；
③ **线程纪律（S4.1 两层）**：主纪律 = 引擎以**所有权转移**进入专职
  渲染工人线程（不跨线程共享引用）；兜底 = pdfium-render `thread_safe`
  特性把每次 FFI 调用经全局互斥串行化（文档线程与渲染线程并存时进程
  级不并发进 C 库）——互斥是兜底不是许可，`PdfiumEngine: Send` 由该
  特性背书（语义锁见证）；
④ **panic 隔离的诚实边界（S4.2）**：catch_unwind 捕获的是 Rust 侧
  panic（&mut R 跨 unwind 经 AssertUnwindSafe——pdfium 句柄在 C 堆，
  Rust unwind 不触碰，后续作业继续尝试）；pdfium C++ 侧段错误/abort
  无法在本层捕获——进程隔离属 PDF-e 后强化面（S6 已知缺口登记同款
  纪律）；
⑤ **阈值依据**：阈值 = 实测 × 3 后向上对齐 1/2/5 稳定序列（防沙箱
  负载抖动误报；×3 下限仍拦数量级回归）；打开面用**中位**断言（首次
  装载抖动 max 3.4ms 已在档）；性能测试 `#[ignore]` 常驻（手动
  `--ignored --nocapture` 运行，不进常规门禁的抖动面）。**例外在档**
  （pdf-c-1r 审计增补）：hit_vs_miss_divisor = 10 而非 ×3——命中/未命中
  是比值门而非绝对值门（实测 415-549×，阈值 10× 为极宽松下限，
  意义是"缓存失效即报警"而非精确回归门）；另：命中侧计时不含出缓存
  后的位图消费成本（get 返回引用可零拷贝消费，设计上成立，已在测试
  注释明示）；
⑥ **CommandError minor 增量不升版**：`RenderPanic`/`Engine` 两变体
  为错误面内部增量（CONTRACT §6 在档；错误面仍 Serialize-only 单向）。

**语义锁**：`tests/pdfc_cache_worker.rs` **17 条**（常开，mock 可测）：
LRU 提权（get A 后逐 B 非最热页）/ 驱逐序 = LRU→MRU / 字节预算精确
（bytes ≤ 预算且 = 保留和；非均匀成本按字节逐）/ 重复 put 原位更新
（len 不变 + 旧成本退出 + 提权）/ miss 与 contains 不提权 / 超大条目
自逐（自身键在驱逐集末尾）+ 零成本条目不受驱逐 / clear 全量复位 /
entries 序 = LRU→MRU / 工人顺序回执（id 关联）/ panic 隔离 + 后续作业
存活 / 取消令牌（零 rasterize 调用）/ 停机不悬挂（含 submit 后立即
drop 在途路径）/ mock 端到端缓存命中（二次取页零调用）/ Box<dyn
Rasterizer + Send> 装箱形态 / RenderRequest/RenderedPage serde 往返；
以及 pdf-c-1r 审计回归两锁：Engine 错误受限构造 512 字符钳制（CJK
char 边界）/ 重复 put 更新路径超大值自逐（与 put-miss 路径同构）。
`tests/pdfc_engine.rs` **13 条**（feature，真实 pdfium + spike
fixtures——定位失败 = panic 打印候选，不静默 skip）：绑定打开（3 页
基线）/ Letter 几何 612×792 + Deg0 / 越界 → InvalidPage（几何与抽取
双面）/ 文本抽取非空 + 形状 + 逐页标签 / 无书签 → 空大纲 Ok / 会话
引擎端到端（占位几何被旁路 + 搜索真实命中）/ 限额语义不变（max_hits
钳制 → truncated + 空 query 硬拒）/ 位图缓冲长契约 + Letter 等比
（300→388）/ 渲染落笔（非全零 + 白底主导）/ S3-L2 钳制（0→1px；
999,999→8000×8000 收敛值锁定）/ 绑定失败与垃圾装载 → Engine（消息
含路径）/ Send 见证 + 工人真实往返 / 全链路缓存命中零 rasterize（计数
包裹真实引擎）。`tests/pdfc_perf.rs` **6 项**（feature + `#[ignore]`，
性能门——上表数字即本套件输出）。

**NOTICES 前瞻（PDF-e SBOM 增量义务）**：`engine-pdfium` 特性引入
pdfium-render =0.9.4（**MIT OR Apache-2.0**，pin 锁定）+ 其解析依赖
42 包（chrono/futures/libloading/web-sys 等——**默认构建零编译**，
仅 lock 解析在档）；pdfium 二进制本体 = chromium/8086 non-V8 构建
（**BSD-3-Clause**，fetch-pdfium.sh 版本 pin + SHA256 已闭环于 spike，
PDF-e 入产品仓 bin/ 取件 + licenses/ 全量进 NOTICES）。渲染路径零
image crate（决策①），依赖面不进 image 系。

**审计轮（pdf-c-1r，独立审查代理）：通过**（0 阻断 0 重要，8 轻微）。
修复四项：幽灵取消令牌防御（absorb_cancel 增 job_watermark 双侧剪枝
——id 超过提交水位 = 调用方 bug 令牌直接丢弃，集合上界 = 在飞作业数）/
attach_engine 页数一致性调用方合同入档（双权威不一致的失败模式显式
化；**T1 审计 O3 修复后已升级为装配期硬校验**——`attach_engine`
返回 `Result<(), AttachEngineError>`，引擎页数 ≠ `meta.page_count`
即 typed 拒绝 `PageCountMismatch{expected, engine}`（双侧数值携带，
拒绝时引擎 drop、会话保持 v0 注入路径），调用方合同退役为第二防线
——见文末 T1 卫生修复轮）/ perf_random_flip 正名为 perf_sequential_flip（两阶段顺序取页，
名实相符）/ Engine 钳制与 put-更新超大自逐两回归锁（+2）。在档不修
两项：命中侧计时口径（测试注释已明示）与绑定失败重试路径无直接锁
（进程单例语义限制，实现读审确认真实）。

**门禁**：cargo --workspace **236/236**（219 + 17 常开缓存/工人锁；
默认构建零引擎依赖不变，pdfc_engine/perf 退化空目标）；feature 面
`cargo test -p mdc-pdf --features engine-pdfium` **62/62 + 6 ignored**
（32 骨架 + 17 常开 + 13 引擎；ignored = 性能 6 项）；性能门
`--test pdfc_perf -- --ignored` 6/6（上表实测即本轮输出）；wasm32
check 过；scan.sh 0 命中；clippy 零新增（workspace 面 mdc-wasm 2 条 =
既有基线；feature 面全绿）；cargo fmt 过（mdc-pdf 范围）。既有 219
锁断言语义零改动（PDF-b 锁原样全绿）。

## PDF-d：标注面（2026-10-10）

**背景**：pdf_carrier_plan §5 第四轮——标注面 = v0 四种标注（Highlight/
Note/FreeText/Ink）的 lopdf 增量写落盘 + 往返语义锁 + Acrobat 互通的
引擎层预证。输入资产：PDF-a spike 探针 3（IncrementalDocument 增量写
技术范本——原字节前缀保持 + lopdf 重载 + pdfium 交叉读回）、PDF-b
annotation.rs 四 kind 模型（含 pdf-b-1r 审计在档的「落盘时由引擎侧
校验」承诺与 Rect 类型层不变量）、ADR-P1（lopdf 承担标注写）。

**交付**（crates/pdf，feature 纪律 = 默认零引擎依赖不变，default=[]
不动）：

- `writer.rs`（常开，零依赖）：`IncrementalSave{bytes_appended,
  prefix_bytes}`（增量语义的证据载荷——幂等 flush 的可断言面）+
  `trait AnnotationWriter{write_annotation, save_incremental}`（文档级
  会话面，**非命令面**——六命令零变化）+ `MockWriter`（常开契约可测）+
  三条 Acrobat 互通纯函数：`encode_author`/`decode_author`（/T origin
  约定，`ORIGIN_SUFFIX_AI = " [ai]"` 词面冻结）+ `pdf_date_string`
  （/M 日期串，自实现 civil-from-days——零 chrono，值时钟语义）；
- `writer_lopdf.rs`（feature `engine-lopdf`）：`LopdfWriter` =
  `from_bytes`（Document::load_mem；**加密档拒 = S5 引擎侧独立防线**：
  仍加密（口令不可得，lopdf 以未解密形态装载）与曾加密（空口令可解，
  文件本体仍是加密档）两形态均 `SecurityDeny{S5-encrypted-write}`）+
  四 kind → 标准 Annot 对象（**零私有扩展**）：Highlight `/Subtype
  /Highlight`+`/QuadPoints`（quad 环序 8 数直通）+`/C`（0-1 归一）；
  Note `/Subtype /Text`+`/Rect`；FreeText `/Subtype /FreeText`+`/Rect`+
  `/Contents`+`/DA`=`/Helv 12 Tf 0 g`（Acrobat 免 AP 渲染——v0 不生成
  /AP）；Ink `/Subtype /Ink`+`/InkList`（逐划点序）+`/BS`（`/W 1 /S /S`）。
  公共字段：`/T` origin 编码（Human 原文 / Ai `"{author} [ai]"`——可逆
  且 Acrobat 作者栏可读）+ `/M` 日期串 + `/Rect`（Highlight=quad 包络 /
  Ink=stroke 包络 / Note+FreeText=模型直通；Y 轴用户空间直通无翻转）+
  `/F 4`（打印位）；文本串编码：ASCII 字面量 / 非 ASCII UTF-16BE+BOM
  （PDF 32000 7.9.2——Acrobat 系阅读器稳妥形态）。挂载：页对象
  `opt_clone_object_to_new_document` 入新修订 + `/Annots` 三形态全覆盖
  （缺省建 / 直接数组追加 / 间接引用先克隆数组对象）+ 引用查重。增量
  保存：`save_to` 产物**前缀逐位保持断言**（违约 = typed Engine 错误，
  不产出损坏文件）+ 保存后从产物重建 IncrementalDocument（产物 = 新
  基线）+ `product_bytes`/`into_bytes` 产物取回；载荷健全性（空
  quads/空笔划/空笔划点/非有限坐标）写前校验（pdf-b 在档承诺兑现）；
  `InvalidPage` 页界防御（u32::MAX+1 防御含内）；
- `document.rs` 增量：`attach_writer(Box<dyn AnnotationWriter>)`（与
  引擎装配**独立**——attach_writer 不要求 attach_engine）+
  `flush_annotations()`（会话 API 非命令）：① S5 加密态拒（先于一切
  ——含零脏短路）→ ② 零脏幂等（写器在位 = 委托 save_incremental 零
  追加；无写器 = 双 0 诚实占位）→ ③ 逐条写（脏集升序 = BTreeSet 键序
  确定性）→ ④ save_incremental + **成功才清脏**（失败保留 = 重试经
  写器幂等合同安全）。脏跟踪：AnnotateAdd 入脏 / AnnotateEdit 仅实际
  内容变更重标脏（no-op edit 不脏——与 modified_ms 不推进同语义）；
  有脏 + 无写器 → typed `Engine` 错（错误面零增长——不新增变体）；
- Cargo.toml：`lopdf = { version = "=0.45.0", optional = true,
  default-features = false }` + `engine-lopdf = ["dep:lopdf"]`。lopdf
  0.45 默认特性 = `chrono-clock` + `rayon`——**全关**；解析必需依赖
  （nom/flate2/md-5/sha2/aes/stringprep 等非可选面）随包引入；image 系
  （embed_image）保持关闭（与 pdfium-render 面同一零 image 纪律）。

**Acrobat 互通验证记录**（两层，诚实登记不冒认）：

- **引擎层预证（本轮闭环，语义锁 ③/⑮）**：lopdf 写 → pdfium 读——
  四种 subtype 全对上（Highlight/Text/FreeText/Ink）+ `/T` 经
  FPDFAnnot（pdfium-render `creator()`）可读（含 Ai `"[ai]"` 后缀与非
  ASCII UTF-16BE 作者）+ FreeText `/Contents` 可读 + 既有标注（AcroForm
  widget）共存不破坏。spike 探针 3 同款路径的产品化实证；
- **GUI Acrobat 人工验证 = 开发环境轮事项（未做，不冒认）**：真
  Acrobat Reader 打开产物目视四 kind 渲染（/DA 免 AP 路径的实际效果/
  高亮叠加/墨迹笔宽/注释弹窗）与作者栏显示——属开发环境轮（沙箱无
  GUI Acrobat），登记为 PDF-e 前置人工验证项。引擎层证据不等于 GUI
  层证明，两层分开在档。

**决策记录**：

① **/T origin 编码约定**：Human = 原文；Ai = `"{author} [ai]"`——可逆
（后缀剥离）且 Acrobat 作者栏可读（对比：私有 XMP/隐藏字段的不可读与
不可逆两侧风险）。已知歧义在档不修：Human 名字面恰以 `" [ai]"` 结尾
时解码侧误判 Ai——解码仅用于自家回读对账，写侧权威在模型 origin 字段
（锁 8/④ 钉为文档化行为）；
② **/M 日期串纯函数 + UTC**：`pdf_date_string(epoch_ms)` 自实现
civil-from-days（零 chrono——lopdf 特性面 default-features=false 不带
时钟库；纯函数 = 同值时钟任何环境同一字符串，值时钟语义对齐会话时钟
注入纪律；UTC 固定偏移 `+00'00'`——无系统时区依赖）。闰日/世纪非闰年
（2100）均有锁；
③ **每次 flush = 恰好一个修订（保存后重建）**：save_incremental 后从
产物重建 IncrementalDocument——修订内容恒为「本轮脏对象」，不重复写
已落盘对象；代价 = 每次保存一次重解析（**保存路径，非渲染热路径**
——阅读面性能零影响：pdfium/缓存/工人全不动）。备选「单文档多轮
追加」被否：lopdf save 会把 new_document 全量对象重复写入第二修订；
④ **编辑重写 = 同对象号重定义（增量更新标准语义）**：AnnotId→ObjectId
映射跨 flush 持久——编辑重写在新修订里重定义同号对象（后修订覆盖前
修订），不产生重复对象/重复挂载（锁 8：重载计数不变 + 新值可见）；
⑤ **幂等语义双面**：会话零脏短路（flush 不触写器）+ 写器零待保存
短路（save 不落空修订）——bytes_appended 最小化两级成立；无写器 +
零脏 = 双 0 诚实占位（不伪造产物长度）；
⑥ **S5 双防线**：会话层（assessment.encrypted 判定，先于一切）+
  引擎层（from_bytes 加密档拒）——两层独立可测（锁 ⑥ 各自锁死）；
  曾加密（空口令可解）也拒：文件本体是加密档，v0 S5 = 加密档零增量写
  （lopdf 0.45 虽支持解密态重加密追加，策略面不做——复杂度与安全面
  收益不匹配，PDF-e 后强化面再议）；
⑦ **错误面零增长**：flush 的无写器/载荷不健全/引擎失败全部走既有
  `Engine(String)`（PDF-c 受限构造先例——512 字符钳制）；不新增
  CommandError 变体（错误面 Serialize-only 单向，协议面冻结）；
⑧ **lopdf/pdfium-render 版本钉住再导出**（feature 门内 `pub use`）：
  测试/宿主经 mdc-pdf 引用与实装同版本引擎 crate——避免 dev-dependency
  把引擎带进默认测试构建面（默认零引擎依赖纪律：默认构建不编译
  lopdf/pdfium-render，cargo tree 实证）。

**语义锁**：`tests/pdfd_session_flush.rs` **9 条**（常开，零依赖）：
写抽象契约 + mock 统计模型 / flush 逐条写 id 升序 + 幂等 / 脏集精确性
（新增/实际编辑/未动/no-op 四类） / S5 加密态 flush 拒先于零脏短路 +
写路径零接触 / 无写器错误面（有脏 Engine + 零脏双 0）/ 保存失败重试
安全（dirty 保留 + 重写）/ 写器无引擎独立装配 / /T origin 编码往返
（含空作者 + 文档化歧义）/ /M 日期串已知值（epoch 0/闰日/2100 世纪
非闰/形状恒定/毫秒截断）。`tests/pdfd_writer.rs` **17 条**（双引擎
feature 面，spike fixtures + libpdfium 定位序同 pdfc_engine）：①四
kind 全字段逐位往返（QuadPoints 16 数/InkList 逐划/Rect/Contents/C/F）
②原字节前缀逐位保持（两轮修订传递性）③pdfium 交叉读回（四 subtype +
/T + /Contents）④Origin 往返（含非 ASCII = UTF-16BE 路径）⑤/M 日期串
字面锁定 ⑥加密双防线（from_bytes 拒 + 会话 flush 拒且产物零变化）⑦
幂等 flush（零追加 + 长度不变）⑧编辑重脏 reflow（同对象号重定义——
无重复 + 新值可见 + /M 推进）⑨包络 Rect（quad/stroke min-max，含乱序
输入）⑩垃圾字节四形态 typed 拒 ⑪写器无引擎会话端到端 ⑫二轮修订
only-dirty（追加量 < 首轮 + 计数精确）⑬FreeText /DA 存在性 + 无 /AP
⑭百条冒烟（100 混合 kind 单轮 flush + 重载全数对回：页计数/subtype/
/T 逐槽）⑮既有 /Annots 挂载面（form_probe.pdf AcroForm widget 共存
不破坏 + pdfium 双条可读）。

**NOTICES 增量（PDF-e SBOM 义务前瞻，与 pdfium-render 面同纪律）**：
`engine-lopdf` 特性引入 lopdf =0.45.0（**MIT**，pin 锁定）+ 其解析/
加密依赖 46 包（aes/md-5/sha2/nom/flate2/encoding_rs/chrono 系**不含**
——default-features=false 后 chrono/rayon/image 均不进图；默认构建
零编译，仅 lock 解析在档）。`engine-pdfium` 面增量：pdfium-render 再
导出（`pub use`，零版本变化）。

**审计轮（pdf-d-1r，独立审查代理）：有条件通过**（0 阻断 1 重要 5 轻微）。
声明 10 条全复跑实证（含叠加一致性——本轮改动由两段代理工作叠加，
首段超时遗留 + 次段收尾，审查确认无死代码/半成品残留，仅 1 处零消费
再导出已删）。修复/补齐：**间接 /Annots 形态回归锁**（页 /Annots 5 0 R
独立数组对象——第三合法形态零覆盖，审计员仓外实证功能正确后补锁 ⑯：
既有标注存活 + 新增追加 + 前缀逐位保持 + 宿主 /T 零触碰）/「曾加密」
空口令形态回归锁 ⑰（was_encrypted 分支——装载自动解密仍拒增量写，
S5 双防线全形态闭环）/ 零消费 pdfium_render 再导出删除（叠加遗留死码）/
测试 helper 解引用间接 /Annots（结构性覆盖缺口根因）。在档不修：/DA
字体未挂 /DR（GUI Acrobat 验证轮处理，已在互通记录登记）；年份 >9999
日期串形状（宿主时钟不可达，文档注记）。
**门禁**：cargo --workspace **245/245**（236 + 9 常开标注面锁；默认
构建零引擎依赖不变——cargo tree 实证不编译 lopdf/pdfium-render）；
feature 面 `cargo test -p mdc-pdf --features engine-pdfium` **71/71 +
6 ignored**（62 + 9 常开标注面锁）；双引擎面 `--features
engine-pdfium,engine-lopdf` **88/88 + 6 ignored**（+17 pdfd_writer 锁，含审计回归两锁）；
wasm32 check 过；scan.sh 0 命中（product + --all 扩展面均 0）；clippy
零新增（workspace 面 mdc-wasm 2 条 = 既有基线；lopdf 单特性面与双引擎
面全绿）；cargo fmt 过（mdc-pdf 范围）。既有 236 锁断言语义零改动
（PDF-b/c 锁原样全绿）。

**T1 修复轮门禁更新（2026-10-10，实跑复核）**：workspace 默认面
**252/252**（245 + T1-fix-a 5 mdcore 质量锁 + T1-fix-b 2 pdf 锁——
pdfb_skeleton 32→33 / pdfc_cache_worker 17→18）；feature 面
engine-pdfium **73/73 + 6 ignored**、双引擎面 **90/90 + 6 ignored**
（fix-b 时点缺 form_probe.pdf 的 1 失败已自愈——探针 fixture 复原
就位，mount_preserves_existing_page_annots 复跑通过）。
详见下节「T1 卫生修复轮」。

## T1 卫生修复轮（2026-10-10，产品仓卫生批量 fix-c）

> 范围 = T1 勘察/复审 12 项清单中的仓卫生项（F5/F7/F8/F11/F12/O2）+
> fix-a/fix-b 代码修复的在档收口。代码缺陷修复本体（F1/F2/O3/O4/F10/
> F9）由并行代理 A/B 落地，本轮全部在其工作区改动之上**追加**（未回退/
> 覆盖任何既有改动；fmt 清扫对其代码仅重排格式，语义零变化——
> workspace 252/252 全绿复核）。

- **F5 工具链钉住 + 全仓 fmt 清扫**：新增 `rust-toolchain.toml`
  （`channel = "1.99.0"`——rustfmt 行为随工具链版本漂移的根治：此前
  无钉住，rustfmt 1.10.0 对旧工具链格式的 mdcore/shell/wasm 一次性
  报出全仓 fmt 门禁红；升级工具链 = 显式改该文件 + 全仓复跑门禁）。
  本轮 `cargo fmt --all` 一次性清扫 **492 diff / 34 文件**（mdcore 26
  + shell 7 + mdcore-wasm 1；T1-survey 时点 459/32，fix-a/fix-b 新增
  代码带来 +33 diff/+2 文件）；crates/pdf 维持干净（四轮手工对齐）。
  清扫后 `cargo fmt --all -- --check` **0 diff**——fmt 门禁自此
  跨环境可复现。
- **F7 版本单源清单补 pdf**：r24_packaging.rs 版本一致性锁清单
  `["mdcore","mdcore-wasm","shell","shell-native"]` → 增 `"pdf"`
  （PDF-b 起 workspace 第五成员此前漏列，仅由 pdfb_skeleton 版本自锁
  补位成闭环）——根 Cargo.toml「跨 crate 一致性锁背书」注释重新
  成立；模块文档「四 crate」同步改「五 crate」。
- **F8 shell-native 文档纠偏**：模块文档「未列入 workspace members、
  `cargo test` 不会触碰」与根 Cargo.toml members/default-members 均含
  其矛盾——改为事实（**在列** + 零依赖蓝图测试随 workspace 运行，
  冒烟锁 `blueprint_constants_present`；「依赖注释态」= 实体面
  wry/tao/gtk 不进编译面），保留设计意图注记（蓝图常驻 workspace =
  结构漂移由门禁即时可见）；mdshell Cargo.toml description 同类失真
  （「不在本 workspace 编译面内」）一并纠正。
- **F11 依赖卫生单源**：根 Cargo.toml 增 `[workspace.dependencies]`
  （serde = "1.0.229" / serde_json = "1.0.151"——与 Cargo.lock 现解析
  精确一致，**锁文件零变化**[git diff 实证]）；mdcore / mdcore-wasm /
  mdshell / mdc-pdf 四 crate 改 `workspace = true` 继承（mdcore 的
  宽松 `"1"` 要求同步收紧到锁内现值）；feature 声明（derive/rc）留
  crate 本地面；引擎 pin（pdfium-render/lopdf 的 optional 面）保持
  各 crate 现状不动（feature 决策记录在案）。
- **O2 门禁口径对齐（实跑为准）**：`cargo test --workspace` 默认面
  **252/252 全绿、25 个语义锁套件**（34 个 test result 行——含
  feature 面空套件与 doc-tests；1 ignored = shell-native 文档的
  `ignore` 代码块非测试）＝ 245 旧基线 + T1-fix-a 5（mdcore
  t1_quality_fixes）+ T1-fix-b 2（pdfb_skeleton 32→33 /
  pdfc_cache_worker 17→18）。文档修正：COMPLIANCE.md §3「16 套件」
  → 25 套件/252 用例；README 构建与门禁「24 套件/245 用例」→
  25/252、收口纪律「18 套件」→ 25；本文件 PDF-d 门禁段增 T1 更新
  口径块（上文）。T1-survey 在分析仓 worklog 的「223+/16 套件」口径
  失实由主代理登记勘误（非本轮范围）。
  〔T1-verify 终审脚注，PDF-e 轮补记：「25 套件」= **24 个非空测试
  目标套件 + shell-native 的 1-ignored doc-test 行**——「非空语义
  锁套件」的措辞把仅含 ignored 行的 shell-native doc-test 计入了
  非空面，计数本身不错（252 用例主口径精确），拆解在此；PDF-e 起
  默认面 35 个 test result 行（+pdfe_security feature 空套件）、
  257 用例，见 PDF-e 节门禁。〕
- **F12 feature 面可复现 bootstrap 成文 + fixtures 候选序调整**：
  bootstrap 三步清单成文（下小节，COMPLIANCE.md §3 后同款指引）；
  三测试文件（pdfc_engine / pdfc_perf / pdfd_writer）的
  fixture_candidates 候选序改为 **crate 本地 `crates/pdf/fixtures/`
  优先、仓外 spike 位兜底**（产品仓自持素材位在前——fixtures 入库后
  fresh clone 即可复现、不受仓外目录状态影响；当前 crate 本地位不
  存在，环境行为零变化）。libpdfium.so 定位序不变（env → 候选清单
  → 系统）。
- **fix-b 移交项收口**：① pdf-c-1r 在档行「attach_engine 调用方
  合同」更新为装配期硬校验语义（`PageCountMismatch{expected, engine}`
  typed 拒绝、双侧数值携带、拒绝时引擎 drop 会话保持 v0 注入路径；
  回归锁 33 `attach_engine_validates_page_count_consistency`——上文
  PDF-c 审计轮段内更新）；② worker 取消合同「作用于尚未开跑的排队
  作业」经 F1 修复后为真（`last_replied` 改 `Option<u64>`，首作业
  id 0 的排队取消令牌不再被哨兵撞值丢弃；回归锁 18
  `worker_cancel_first_job_id_zero_is_honored`）——措辞无需改动；
  ③ CONTRACT.md §6 核查：仅时间线语「会话引擎接线」提及 attach，
  **无 attach 行为合同措辞在档——无需升版**（§6 错误面/命令面零
  变化，AttachEngineError 为 Rust 侧装配期错误不进 serde 协议面）。
- **门禁（本轮验收）**：cargo test --workspace 252/252 全绿；clippy
  --workspace --all-targets 零新警告（mdc-wasm 2 条 static_method_of =
  既有基线）；cargo fmt --all -- --check 0 diff；wasm32 check
  （`-p mdcore-wasm --target wasm32-unknown-unknown`）过；Cargo.lock
  零变化；scan.sh 0 命中；feature 面复验（F12 候选序调序后实跑）：
  engine-pdfium 73/73 + 6 ignored / engine-lopdf 60/60 / 双引擎
  90/90 + 6 ignored（form_probe.pdf 探针 fixture 已在 spike 位就位，
  fix-b 时点的 1 失败自愈）。

### feature 面前置 bootstrap（T1-F12 成文——净克隆复现引擎门的唯一路径）

> feature 面测试载荷（libpdfium.so + fixtures）**非净克隆自带**：
> 默认面门禁零引擎依赖、裸环境全绿；`engine-pdfium` /
> `engine-lopdf` 面需先在分析仓 spike 完成 bootstrap。三步均在
> `cleanroom-analysis/md-line/pdf-a/spike/` 目录下执行：

1. `./fetch-pdfium.sh` —— 取 libpdfium.so（版本 pin `chromium/8086`
   non-V8 + SHA256 验证——升级 = 改脚本内 pin + 复验三探针 + NOTICES
   更新）；
2. `cargo run --release` —— 生成 fixtures：small.pdf（3 页）/
   large.pdf（500 页）/ small_annotated.pdf（lopdf 现场构造，无外部
   素材依赖）；
3. `cargo run --bin formfill_probe --release` —— 生成 form_probe.pdf
   （AcroForm 表单探针——PDF-d 锁 ⑮ 挂载面载荷；探针源由并行代理
   复原入库中）。

载荷定位序（测试运行期，任一缺失 = panic 打印全部已尝试候选——不
静默 skip，门禁环境必须可诊断）：

- **libpdfium.so**：env `MDC_PDFIUM_LIB`（文件路径）→ 编译期候选清单
  （产品仓 `bin/lib/` → 分析仓 spike `bin/lib/` → crate 本地
  `bin/lib/`）→ 系统（`bind_to_system_library`）；绑定失败 =
  `CommandError::Engine` 携带全部已尝试路径。
- **fixtures**：env `MDC_PDF_TEST_FIXTURES`（目录）→ 编译期候选
  （**crate 本地 `crates/pdf/fixtures/` 优先**——产品仓自持素材位，
  T1-F12 调序；仓外 spike `fixtures/` 兜底）。

## PDF-e：安全姿态对齐（2026-10-11）

**背景**：T1 审计定谳的「文档先于实现」漂移收口轮。权威策略 =
SECURITY_POLICY.md v1（分析仓，只读参照），产品侧权威 = CONTRACT.md
§6 + crates/pdf/src/security.rs。T1 清单的漂移点：`DangerousHit` /
`OpenAssessment.warnings` 零生产构造点（检测面不存在）、危险族 5 族
vs 策略 S2.2/S2.3 全清单 8 族、`render_watchdog_ms` 注释称「PDF-c
启用」实未接线、`open_budget_ms` / `decode_output_max_bytes` /
`object_depth_max` / `xref_objects_max` 零消费点、
`enforce_open_file_bytes` 零调用、S2.4 `open_scan()` 承诺未兑现、
S4.1「渲染工人池」实为单 `RenderWorker`、S4.2「panic 后工人重建」实为
同工人续跑、S6 表单枚举进 SecurityReport 未实装。

**交付**（crates/pdf + CONTRACT.md；feature 纪律不变——默认零引擎
依赖，`cargo tree` 面 default=[] 不动）：

- `security_scan.rs`（新模块，feature `engine-lopdf`）：S2.4 承诺的
  `open_scan(pdf, limits) -> OpenAssessment`（L7 输入字节 ❶ → lopdf
  装载 → S2 八族 + S6 表单探针 + S5 加密旗标 → L4 打开预算 ❶——
  产物直接喂 `PdfSession::open`，告警不阻断打开）+ `scan_document`
  （纯对象图扫描，`LopdfWriter::from_bytes` 装载后复用同一实现）。
  **对象图全扫**（`Document::objects` 逐对象 + 内联嵌套递归，引用不
  追——间接对象都在顶层枚举）：不可达敌意对象同样可见（检测面不
  依赖解析器可达性判定）；一 dict 每族至多一 hit（同族多触发条件
  合并——如 `/S /URI` 与 `/URI` 键双探测）；报告确定性（族声明序 ×
  详情字典序 + 精确去重——同文档任何环境同一报告）；/AcroForm
  /Fields 字段树走查带引用链 seen-set 环防 + 深度护栏
  （`object_depth_max` 复用）；
- `security.rs`：`DangerousFamily` 5 族 → **8 族**（增 `UriAction` /
  `JavaScript` / `NamesJavaScript`，声明序 = 报告排序序；serde 名
  冻结锁同步扩——协议面锚）+ S6 `FormProbe{field_count, fields}` /
  `FormField{name, value}` + `OpenAssessment.form_probe`
  （`serde(default, skip_serializing_if)`——旧 JSON 缺省 = None，
  None 序列化省略，旧消费者零感知）+ 理由码三枚
  （`S3-L7-open-file-bytes` / `S3-L4-open-budget` /
  `S3-L0-decode-output`）+ `Limits::check_open_input_bytes` /
  `check_open_budget`（Duration 全精度比较——0 预算确定性触发）/
  `enforce_decode_output_bytes`；字段注释逐项改为真实接线状态
  （已接线/登记待办分列）；
- `worker.rs`：**看门狗换代实装**（S3-L2 ❸弃置 + S4.2 工人重建）。
  `RenderWorker::watchdog(factory, budget)` 工厂构造：每代 = 渲染线程
  + 看门狗线程（共享 `WatchState` 短锁观测态；轮询 10ms）；在跑作业
  自**开跑时刻**起算超 `render_watchdog_ms` → ① `WorkerReply::Timeout
  {id}` 回执（❸弃置——迟到的旧代 `Done` 由包装器按「在飞作业首回执」
  验收面静默丢弃，「结果永不采用」的机械保证）→ ② 换代事件 →
  包装器在下一公共调用（submit/recv/cancel/drop）：drain 旧代回执 →
  旧代通道/join 句柄弃置（**断连弃置不 join**：敌意永挂 C 调用不得
  悬挂停机；旧线程 C 调用返回后见断连即退、自然回收）→ 工厂闭包起
  替代工人（**字节快照重开**——S4.4 纪律：闭包持字节快照，首代与
  替代代引擎都在渲染线程归属内构造）→ 在飞作业 + 未决取消令牌按
  提交序重放（保序：令牌在作业后——新代 FIFO 吸收路径同首播）。
  `generation()` 观测面（0 起，换代 +1）。`RenderWorker::new` 保持
  **无看门狗裸工人**（能力面拆分——无引擎重建能力即无换代面）；
  单工人常态 + 看门狗触发的工人重建 = v0 形态（非常驻工人池，
  CONTRACT §6 S4.1 措辞同步）；
- `engine_pdfium.rs`：打开面限额接线（`open` / `open_with_library`：
  L7 输入字节先于装载 + L4 打开预算）+ `rasterize` 内 S3-L0 渲染
  目标硬闸（L2 钳制后、分配前：宽×高×**4**（BGRA 实缓冲——任务书
  写 ×3，实装按真实分配口径）vs `decode_output_max_bytes`，超限
  `SecurityDeny{S3-L0-decode-output}`）；
- `writer_lopdf.rs`：`from_bytes` → `from_bytes_with_limits`（L7 →
  装载 → S5 加密拒（既有）→ S2/S6 扫描 → L4 预算）+ `assessment()`
  产品面出口（宿主取出喂 `PdfSession::open`——与 `open_scan` 同一
  扫描单一实现）；
- `document.rs`：attach_engine 拒绝语义措辞精度（T1-verify ③：二次
  注入拒绝场景保持的是**旧引擎**而非 v0——行为本正确，仅措辞）；
- `CONTRACT.md`：§6 v1.2 → **v1.3**（八族 + form_probe serde 形状 +
  `Timeout` 变体 + 三理由码——全部纯增量；`__contractVersion` 保持
  1.1，window 面零变化，仿 v1.2 先例显式在档）。

**决策记录**：

① **检测面 = 对象图全扫而非可达性走查**：不可达敌意对象（不挂任何
页/catalog）同样检出——敌意载荷可藏于未引用对象，检测不应依赖解析器
可达性判定（pdfium 视角与对象图视角的可达集本就不同）；
② **看门狗 = 独立线程 + 开跑时刻锚点**：在跑作业的 C 调用不可中断
（不假装能杀线程——与取消令牌「只作用于排队作业」同一诚实纪律），
看门狗只观测不注入；超时处置 = 回执 + 换代（弃置语义），不杀线程；
换代后旧代 join 句柄弃置 = 敌意永挂 C 调用下停机不悬挂（资源滞留
上限 = 恶意触发换代的次数，接受并在此在档）；
③ **换代重放保序**：作业先于令牌重放——新代 `job_watermark` 先覆盖
再吸收取消，FIFO 语义与首播一致（锁 20 的零执行断言背书）；
④ **加密档检测边界诚实性**：仍加密（口令不可得）态 lopdf 不解析对象
图——S2 检测面不可用（无假阳/假阴），`encrypted` 旗标仍置位（S5 据此
拒写）；曾加密（空口令自动解密）态对象图明文可见——检测照常。两形态
有锁；
⑤ **深度护栏面区分（短路验证发现）**：lopdf 0.45 解析器自身嵌套预算
`reader::MAX_NESTING_DEPTH = 100` 会静默丢弃超深对象（探针实证：100
层链序列化后 `/AA` 仍在字节里，load 后对象易号消失——99 层完好）。
文件面的深嵌套攻击被解析器先行截断（100 < 128 护栏）；扫描护栏只对
内存构造面（`scan_document` 直扫）可观测——判别锁 10 因此重构为内存
面直扫（原文件面写法恒真 = 锁空转，SC-J 实证后修复）；
⑥ **L0 渲染目标硬闸的可达性**：冻结数值下不可达（64M px × 4B =
256 MiB < 1 GiB，L2 钳制先行）——纵深防线（未来限额调整的收口闸），
接线在 `rasterize` 分配前；引擎级端到端锁不可构造
（`PdfiumEngine::rasterize` 用 `Limits::default()`，自定义限额接线属
PDF-d+），helper 面锁在 pdfb_skeleton 35 号 + 理由码入档。

**语义锁**（+16：11 feature 面专属 + 5 默认面常开[pdfb 2 + cache_worker 3]；
T2-fix 终修 +2 默认面常开[cache_worker 3→5]）：
`tests/pdfe_security.rs` **11 条**（feature `engine-lopdf`，敌意
fixtures 全部 lopdf 现场构造——零仓外依赖）：①良性零告警 + 报告
确定性 ②八族全检出（多族混合 + 不可达对象可见）③详情对象号定位 +
同 dict 同族合并 ④S6 表单探针（层级全名 `.` 连接 + /Tx 现值可读 +
表单非危险族——warnings 恒空）⑤加密双形态检测边界 ⑥写器
assessment 出口（与 open_scan 同结果）⑦L7 超限双入口拒 ⑧L4 预算
双入口拒（0 预算确定性触发）⑨会话端到端（告警不阻断打开 + 非加密态
写命令正常）⑩深度护栏判别式（内存面直扫——见决策⑤）⑪字段树环防
（/Kids 自引用终止 + 计数不重复）。`pdfb_skeleton.rs` +2（34
form_probe serde 形状：往返保真 + None 省略 + 旧 JSON 缺省兼容；35
打开入口限额：L7 边界值 + L4 零预算 + 理由码）。`pdfc_cache_worker.rs`
+3（19 看门狗超时：Timeout 回执 + 换代代次证据 + 超时作业不重放 +
旧工人自然退出；20 换代重放：排队作业保序 + 取消令牌随作业重放——
被取消作业零执行；21 能力面拆分对照：`new` 裸工人慢作业 Done、代次
恒 0）。T2-fix +2（22 工厂 panic 断连不悬挂：先置看门狗终态再传播 +
看门狗线程退出[Linux /proc 复核] + 代次恒 0[死亡≠换代]；23 替代代
工厂 panic：换代处理无 panic 逃逸 + 新代断连 + 替代代零服务）。
**全部新锁经短路验证**（临时拿掉实装 → 锁红 → 恢复 → 绿，
10 组机制级实验；SC-J 发现锁 10 空转并重构，见决策⑤；T2-fix 两锁
另经 4 组实验：SC-K1 catch 拿掉红 / SC-K2 Retired 先置拿掉红 /
SC-K2b 耗时断言禁用后 /proc 探针独判红 / SC-K3 修复前形态双红）。

**已知边界（诚实登记）**：

- `xref_objects_max`（8,388,607）与 lopdf 解析器内逐对象深度：lopdf
  内部无挂点——v0 边界项，升级 lopdf / 自研解析层时接线
  （security.rs 字段注释在档）；S3 对象流 seen-set 同理（/AcroForm
  字段树游走面已有 seen-set 环防——lopdf 解析面对象流去重无挂点）；
- L0 渲染目标硬闸冻结数值下不可达（决策⑥）；引擎级端到端锁随自定义
  限额接线（PDF-d+）补；
- `RenderWorker::watchdog` 生产消费点 = **零**（仅测试调用——语义锁
  19/20/21/22/23）：看门狗换代是已实装能力面，宿主接线
  （`Limits.render_watchdog_ms` → 工厂构造预算）属后续轮（PDF-f+）——
  与 L0 引擎级端到端锁同款登记口径（T2-review ❹，T2-fix 补登）；
- 仍加密档 S2 检测面不可用（决策④）——检测需明文对象图，非能力缺口
  的隐瞒（S5.1 只读支持加密档打开不变）；
- 旧代工人线程 detach（决策②）：敌意永挂 C 调用下旧线程不退出，
  资源滞留上限 = 恶意触发次数；权衡 = 停机不悬挂优先；
- S4.1/S4.2 措辞已与实装对齐（CONTRACT v1.3）：单工人常态 + 看门狗
  换代；panic = 同工人续跑（RenderPanic 回执）、超时 = 换代——两
  机制分立，不再合称「工人池 / panic 重建」。

**门禁**（2026-10-11 实跑）：`cargo test --workspace` 默认面
**257/257**（252 T1 基线 + PDF-e 默认面 5 新锁：pdfb_skeleton
33→35 / pdfc_cache_worker 18→21；35 个 test result 行——pdfe_security
为 feature 空套件；「25 套件」口径拆解见 T1 节 O2 脚注）；feature
面 engine-lopdf **76/76**（65 常开 + 11 pdfe_security）、
engine-pdfium **78/78 + 6 ignored**（73 + 5 常开新锁）、双引擎
**106/106 + 6 ignored**（90 + 5 + 11）；clippy 四面
（default/lopdf/pdfium/dual --all-targets）零新警告；`cargo fmt
-p mdc-pdf -- --check` 0 diff（本轮新增代码 fmt 清扫后入档）；
compliance/scan.sh 0 命中；既有 252 锁断言语义零改动（PDF-b/c/d/T1
锁原样全绿）。CONTRACT.md v1.3 升版在档（纯增量，window 面零变化）。

**T2-fix 终修（2026-10-11，修复代理H——T2-review 定谳 1 重要 + 3 轻微
+ 1 移交收口）**：❶（重要）`spawn_generation` 工厂 panic 语义——工厂
移入渲染线程闭包内执行 + `catch_unwind`（T2-review 处方：**先**置
`WatchState::Retired`——看门狗即退、其回执克隆 drop、通道断连、宿主
收报得 `None` 不悬挂——**再** `resume_unwind` 传播；:417-430 内层注释
与模块文档统一为真实行为）。短路验证 4 组：SC-K1 catch 拿掉 → 锁
22/23 双红（recv_timeout 实测 5.000s 吃满预算——耗时断言判别断连
vs 超时）/ SC-K2 Retired 先置拿掉 → 双红 / SC-K2b 耗时断言临时禁用
→ /proc 看门狗退出探针独判红（10s 超时——两判别面各自非摆设）/
SC-K3 修复前形态（工厂在构造线程块内执行）→ 双红（panic 逃逸：锁
22 于构造器、锁 23 于宿主公共调用帧——替代代重开 panic 炸穿 recv/
generation 的真实修复前面）。另以线程身份探针实证：修复前工厂实在
**构造线程**执行（`spawn({...})` 块参数在调用点求值——T2-review 的
「现状」方向描述反了，但其处方目标态与本修一致：代码向模块文档
「引擎都在渲染线程内构造」统一，替代代引擎构造不再落在宿主线程）。
❸（轻微）`probe_form` 注释补畸形 /AcroForm 归 None（与良性缺省
同判——诚实登记）。❹（轻微）看门狗零生产消费点入已知边界（上文）。
门禁复跑：默认面 **259/259**（257 + T2-fix 2：cache_worker 21→23）、
engine-lopdf **78/78**、engine-pdfium **80/80 + 6 ignored**、双引擎
**108/108 + 6 ignored**；clippy 四面 + workspace 零新警告（mdc-wasm
2 条 = 在档基线）；`cargo fmt -p mdc-pdf -- --check` 0 diff；
cache_worker 套件连跑 3 次零抖动。README/COMPLIANCE 计数同步 259
（E 移交项 252→257 为 T2-fix 锁增量落定前口径）。❷（轻微）licenses/
18→17 计数勘误在 compliance-baseline 分支（F 工作树两处），与本仓
收口分开提交。

## PDF-i：xref 链完整性防线（2026-10-31，行尾污染事故修复）

**事故**（issue #113 评论链，用户 Windows 实测 main 5 红：pdff2_annotate
的 eleven_kinds/session_flush/remove_added/remove_external/acrobat_interop，
征兆 `ReadReport{parsed:0}`——「写入成功但读不回来」）：

- 根因不在 #106 代码（该面自合入起零改动、Linux 全绿）：仓内
  `fixtures/small.pdf` 是**零 NUL 字节**未压缩 PDF，git 文本启发式判为
  文本；Windows 默认 `core.autocrlf=true` 检出 LF→CRLF → 全部字节偏移
  漂移、xref 条目 EOL 形态破坏。仓库无 `.gitattributes`（防线缺位）。
- 因果链（探针钉定，pdfi_xref_chain 锁 4 可执行复现）：lopdf 装载退化
  为全文件扫描重建（`xref_start=0`，页仍可读——写路径「一切正常」的
  隐蔽面）→ 增量写 `new_from_prev` 无 /Prev 可指（lopdf 对重建档刻意
  省略）→ 产物 xref 只覆盖本轮新对象、目录/页树经链不可达 →
  `read_annotations` 装载成功但 `get_pages()=0` → 静默 parsed:0。

**交付**（crates/pdf + 仓根，feature `engine-lopdf`）：

- `.gitattributes`（仓根新建）：pdf/png/docx/xlsx/pptx/zip/wasm 等
  binary 显式标记——任何平台/任何 autocrlf 配置下检出与入库逐位保持
  （ZIP/PNG 本含 NUL 已被启发式保护，显式标记属纵深防御）。
- 写入口守卫（Guard A，writer_lopdf `from_bytes_with_limits`）：
  `doc.xref_start == 0`（扫描重建装载态）→ `Engine` 响亮拒绝。增量
  语义要求前序 xref 链可信；重建档上增量写必产 /Prev 缺失的链断裂
  产物。能力边界登记：重建装载档不支持增量标注（全量重写路径未排期）。
- 读面零页守卫（Guard B，annotate `read_annotations`）：装载成功但
  页表空 → `Engine`（PDF 规范页数 ≥ 1，零页必为结构异常；「读到 0
  条」与「没有标注」不可区分正是事故隐身所在）。
- 产物自检（Guard C，writer_lopdf `save_incremental`）：重载后页树
  空 → 拒交付（on-error 写器保持保存前形态可重试）。已知触发路径已
  被 Guard A 入口封死，此为纵深防御（无直接触发测试，登记）。
- `tests/pdfi_xref_chain.rs`（4 锁）：夹具金丝雀（small.pdf 零 CR +
  xref_start≠0 + .gitattributes 在位）/ Guard A 拒重建档 + 原件无误伤 /
  Guard B 拒链断裂档 + 合法零标注档对照 / 事故形态钉定（绕过入口以
  lopdf 原语复刻旧版行为，产物=无 /Prev + 页树不可达——入口拒绝的
  正当性证明）。

**门禁**：engine-lopdf **110/110+1 环境项**（+4 新锁；links_corpus_
real_sweep 红 = test-artifacts/pdf-organize-corpus 语料本机缺失，
干净 main 同红在档）；双引擎 28 红全数环境性（pypdfium2_raw 库
`dlsym failed` ≠ pdfium_7881 绑定 ABI——engine_binds 失败于双侧，
其下游 pdfium 依赖测试任何分支必红；干净 main 失败名单 diff 完全
一致）；clippy -p mdc-pdf --all-targets 0 警告；本轮文件 fmt 0 diff。
**预存红账登记（非本轮引入，建议另行收口）**：① #116 的 forms.rs/
stamp.rs/generate.rs/lib.rs:96/pdfg_forms/pdfh_stamp fmt 未过门（80+
diff，main 上已红）；② links_corpus 语料缺失时不跳过而是 panic
（环境合同与 pdff2 的「缺失跳过」惯例不一致）。

**用户机修复指引**（Windows 检出污染工作树）：合并本 PR 后
`git checkout -f -- crates/pdf/fixtures/`（.gitattributes 生效后不再
复发）；small.pdf 期望 1099 字节。

## #220：ADR-B5 与 undo 实态对账（2026-11-05，审计 P1-27 复核 + 序修正落地）

**触发**：审计分支 `audit/architecture-review-2026-11` final-report-phase3
P1-27 / 研究仓审计笔记 25 F1（P0 对账级）：研究仓 ADR-B5
（decisions/ADR-B5-inverse-rollback.md:13-40，2026-10-09 R22-b）自述
「已裁决并落地」——`apply_batch_invert_rollback`、三处 `tree.clone()` 全消、
逆组「组间逆序组内保序」修复「嵌套子树删除 undo 硬失败 / 多 op 事务静默
错挂」。静态比对（原型母本 + mdcore main）两处均无此实现，mdcore 侧
档案另有三处不实声称（本文件 :450「失败走 ADR-B5 求逆回滚」、:67/:505
「R22-b」措辞、external.rs:99 doc 注释）——全部已于本条勘误。

**复现核实（先写测试、不预设结论）**：`crates/mdcore/tests/
r25_inverse_group_order.rs`（11 用例 = 9 复现 + 2 控制组）。缺陷链
实证与预判**逐项吻合**：

- 根因：`apply_ops` 把各 op 的逆组（`invert_op` 返回 `Vec<Op>`；多数
  长度 1，`RemoveBlock` 的逆 = `build_rebuild_ops` **先序**重建子树，
  组内序 = 父先子后的重建序）摊平后整体 `reverse()`——组内序被倒转。
  单 op 逆组（长度 1）摊平反转 == 组反转，故既有测试全绿（漏网机制）；
  **非叶块删除**（`delete_block` 对非叶无限制，子树删除公开可达）与
  含非叶 RemoveBlock 的多 op 事务产生长度 ≥2 逆组，undo 时先插子后
  插父，`InsertBlock` 的 parent 路径在移位后的树里：
  - 解析失败（BadPath/BadIndex）→ `apply_raw_batch` 失败 → T1-F10
    条目压回 → **undo 硬失败 None、整栈卡死**（复现 6 例：三根/两根
    基座、混组事务双序、双子树删除、链式第 2 步）；
  - 恰好命中顶替被删块位置的节点 → **静默错挂**（undo 返回 Some 但树
    损坏；复现 1 例：独子 P 删除后子块错挂到顶位者名下、P 复原为空壳，
    redo 重放进一步放大）。
- 既有覆盖检索结论：r11/r17/spike/t1/t3/r12 全部只覆盖长度 1 逆组
  （叶删除/MoveBlock/InsertBlock 族），无「非叶删除→undo→结构断言」。

**修复（最小面）**：`apply_ops`（lib.rs）逆序构造改**组感知**——收集
`Vec<Vec<Op>>` 后按组逆序摊平（组间逆序、组内保序）；`Entry.inverses`
仍为扁平 `Vec<Op>`，**serde/历史 JSON 形状不变，只改序**（旧档可读）；
`coalesce_into_last` 前插语义不变（合并面只收文本类事务 ⇒ 逆组全长度 1，
r25 控制组锁定）。

**登记边界（不追溯/不动项）**：
- ~~旧版本保存的历史（含子树删除条目）其存储序已是错的，修复不追溯
  （载入后按其存储原样应用），只有新产生的条目按组语义存储~~
  【已被同 PR 补充修复取代（见下节）：owner 审查指出「旧档照原样应用」
  等于放任错序 inverses 执行，持久化面改为版本闸门降级——旧栈丢弃
  不可执行，只有 v2 栈随档恢复】；
- `tree.clone()` 备份机制**未换**——#220 验收红线：不把「用了
  tree.clone」当失败判据、不做 undo 性能重写，内存面留给 R22 候选 #2；
- T1-F10 失败回退路径与公开 API 签名零改动；
- RemoveBlock 逆不恢复 marks 是**另一缺陷**（审计 03 §1，P1 级），
  与序缺陷正交，r25 断言面刻意避开，另行治理。

**验证面**：cargo test --workspace（除 shell-native，本沙箱缺 GTK
  dev 面）**1322 ok / 0 failed / 7 ignored**（基线 1311 + 新增 11），
r25 RED→GREEN 留证；fmt 触及文件 0 diff；clippy 对拍无新警告；
wasm32 check 过；既有测试无一需要改序（无钉死旧扁平序者）。

**补充修复（owner 对 PR #221 审查意见，同分支追加）——持久化
History 格式版本闸门（2026-11-05）**：

- **审查发现**：#220 只修了新建条目，但 undo 历史随文档持久化
  （`save_document` 写 meta.history、`open_document` 经
  `set_history_json` 恢复）——旧版本保存的档携带整体反转形态的
  错序 inverses，升级后打开照常恢复栈，一次 undo 即触发上述缺陷
  链（BadPath 硬失败/静默错挂）。「旧档可读」不够，还须「旧栈
  不可执行」。
- **实现**：`history.rs` 新增 `HISTORY_FORMAT_VERSION = 2`，
  `History` 增 `format_version` 字段（serde 无条件序列化、缺失
  default 0；`Default` 手写生而 v2）；`History::from_json` 版本
  闸门——解析成功但版本 ≠ 2（缺字段旧档/显式旧版本/未知未来
  版本）→ 丢弃 undo/redo 栈与合并窗边界，回落全新 History；
  JSON 畸形仍显式报错（「格式旧」降级、「格式坏」拒绝）。
  `open_document`/`set_history_json`（含 wasm 透传面）自动获得
  该行为：正文/document_id/revision 照常复原，旧 undo/redo 不
  执行；降级档继续编辑正常、再存即升 v2。
- **回归测试**：`r26_history_format_version.rs` 7 用例——旧档材料
  用当前代码产出后降格（删版本字段 + 单事务单 RemoveBlock 逆组
  整体反转，与修复前代码产出逐位一致），锁：①缺字段/v1/未来版
  三途降级（正文/身份/revision 完整、栈空、undo/redo 不执行、
  revision 不动）；②v2 往返「文本→非叶删除→文本」链 undo×3/
  redo×3 精确复原（含重建子树物化原 ID 随 JSON 往返）；③新档
  必带版本字段、降级档再存升 v2；④畸形 JSON 仍 Reject。
- **附带修复**：`pdfb_skeleton.rs` `source_mirrors_mdcore_five_values`
  的源文件镜像锁对 `history.rs` 做裸字节切片（`idx-200`/`idx+400`），
  本修复新增的中文 doc 注释使切点落进多字节字符内 panic——两处
  切界改 char boundary 对齐（窗只缩/只胀，contains 断言语义不变）；
  第二处系首次暴露（此前被第一处 panic 掩盖）。
- **验证面**：cargo test --workspace（除 shell-native，沙箱缺 GTK
dev 面；shell 契约层传导面已测，shell-native 仅编译性盲区由
  Windows CI 覆盖）**1329 ok / 0 failed**（= 1322 + r26 新增 7）；
  CI 口径 check（--workspace --all-targets --exclude mdshell-native）
  过；wasm32 check 过；fmt 触及文件 0 diff（r26 已格式化；
  document.rs/pdfb_skeleton.rs 各自 1/2 处既有漂移系 #216 复位
  领地，未触碰）；clippy 双面零新警告（mdcore-wasm 5+1 条既有
  基线不变）。

## #210：pdfium 部署链真实环境验证（2026-11-05，#222 Lead 指派方向一）

**背景**：owner 真机报错（#210，Windows：`pdfium 库定位失败（定位
序：…）`，PDF 功能完全不可用）→ #211 修复报错文案 + 补
INSTALL.md → #222 Lead 复核指派收口：「用户实测仍报 pdfium 库定位
失败，需要真实环境验证，不只是文档」。方向判断（方向一 PDF 能力面
vs 方向二 1.0 硬骨头）与证据链见 #222 方向评论。

**侦察实证（实施前）**：
① Linux 侧真实环境验证：按分析仓 `fetch-pdfium.sh` 同款 pin 取件
（chromium/8086 linux-x64 non-V8，SHA256 `588577cf…` 与 spike pin
逐位一致）放仓根 `bin/lib`（取件位）→ `cargo test -p mdc-pdf
--features engine-lopdf,engine-pdfium` **229 ok / 0 failed**（含
pdfc_engine 14 引擎语义锁）——装载与渲染链 Linux 侧实证可用，
**代码无病，病在部署验证链**；
② 文档与代码不对齐的实体：INSTALL.md 主推「放 exe 同目录」，但
定位序候选清单**没有该位置**——只能靠 `bind_to_system_library()`
（= `libloading::Library::new(platform_name)`，Windows 上依赖
LoadLibrary 隐式搜索序，exe 目录第一位）兜底：理论可行、从未验证、
且绑定失败时定位详情不含此位置（用户按文档操作失败后不可诊断）；
③ CI 缺口是结构性的：windows-test.yml 既有步骤注释自认「pdfium
面需要动态库，CI 上没有，只开 lopdf」——engine-pdfium 全测试面
（229）在 CI **零覆盖**。

**实现**：
- `library_candidates()` 增 **exe 同目录显式候选**（运行期
  `current_exe().parent()` 推导，候选 0——部署语义跟着二进制走，
  与 1-3 的编译期 `CARGO_MANIFEST_DIR` 推导不同源；`current_exe`
  不可得的极端环境静默跳过，后续候选与系统绑定兜底）。纯加性：
  既有取件位序不变；
- 形状锁 `exe_dir_is_first_candidate`（候选 0 = exe 同目录 + 平台
  库名；取件位仍在清单）；
- **`tests/pdfk_deploy.rs` 部署位 e2e**（进程级首绑定验证）：cargo
  每个集成测试文件 = 独立二进制 = 独立进程，而 pdfium 绑定是进程级
  单例——「exe 同目录候选被定位序真实选中」只能在进程首个绑定里
  验证，故独立文件单测试：从取件位复制库到 `current_exe()` 同目录
  → `remove_var(MDC_PDFIUM_LIB)`（防 CI env 短路；仅作用本进程）→
  `PdfiumEngine::open` 必须成功 + small.pdf 3 页基线——结合形状锁，
  绑定来源唯一可能是候选 0（INSTALL.md 方式 B 的自动化背书）。
  环境合同沿用 pdfc_engine 同款（库源/fixtures 缺失 = panic 打印
  全部已尝试位置，不静默 skip）；
- **CI 化**（windows-test.yml test-engine job）：新增 Fetch pdfium
  步骤（win-x64 chromium/8086 pin + SHA256 `1fd8af95…` 校验，与
  spike 脚本同轨；non-V8 与安全策略 §JS 面一致），`Test mdc-pdf`
  步骤升级为 `--features engine-lopdf,engine-pdfium` + env
  `MDC_PDFIUM_LIB` 显式提供（定位序第 1 优先）——pdfium 全测试面
  231（229 既有 + 本轮新增 2）CI 化，真实 Windows 上每 PR 验证「按文档操作
  PDF 能打开」；exe 同目录部署位由 pdfk_deploy 在同步骤进程级覆盖；
- INSTALL.md 补「How the application locates PDFium」节（四层定位
  序 + 排障指引：env 直指消除全部搜索歧义）；`.gitignore` 增
  `bin/`（取件产物不入库）。

**验证面**：本地（Linux，库在位）`cargo test -p mdc-pdf --features
engine-lopdf,engine-pdfium` **231 ok / 0 failed**（= 229 既有 + 形状
锁 1 + 部署位 e2e 1；既有口径核对：6 perf 显式 ignore + 1 doctest =
7 ignored 不变）；default 构建（无 feature）`pdfk_deploy.rs` 整文件
退化空目标（`#![cfg(feature)]` 门）——workspace 构建下 mdc-pdf 的
feature 面（pdf-wasm 联动 engine-lopdf，无 engine-pdfium）不含
本轮改动面；fmt 全仓 0 diff；clippy
`-p mdc-pdf --features engine-lopdf,engine-pdfium --all-targets`
零警告；`cargo check --workspace --all-targets --exclude
mdshell-native` Finished（51s，mdcore-wasm 既有 5 警告恒等）；
Windows 真实环境面由 CI 三门禁覆盖（check / engine
tests / mang-shell GPUI）。

## #226：DOCX 读路径 panic 兜底（2026-11-06，审计 P0-2 / health-check 批 4 #E1）

**背景**：全仓审计 P0-2「打开即崩」——docx-rs 0.4.22 读路径在真实语料 4 处
panic（docx4j 501 件批次实锤，REPORT #E1）：GUI 入口只 `map_err` 无
`catch_unwind`，用户打开常见 Word 文档即进程崩溃、未保存编辑丢失；wasm 面
= trap。排期 #230 R2（issue #226）。

**侦察实证（实施前）**：① 4 件 panic 源逐一取 XML 原文定位——
`runprops_word365_props-{b,enum,u}-styles.docx` 的 document.xml 内
`<w:rPr>` 裸元素（S11 `<w:u/>` / M4 `<w:highlight/>`，A2/B2 `<w:u/>`）+
`strict_strict-sample-docx.docx` 的 `w:w="178.05pt"` 族通用度量（tblW/tcW/
tblInd，pgSz 的 `595.35pt` 走 value_to_dax 不炸）；② docx-rs 0.4.22 源码
（crates.io .crate 解包）核对位点：`run_property.rs:110/155` `attributes[0]`
直接索引（VertAlign:126 / Size:132 同类）+ `width.rs:18` 只剥 `%` 的
`f64::expect`；③ 根因定性：**OOXML 规范允许而读层不容忍**（ECMA-376
17.3.2.42 `w:u` 的 `w:val` 可省；ST_MeasurementOrPercent 通用度量合法）
——非输入畸形；④ office-io 读管线 `docx_rs::read_docx(bytes)` 吃原始字节
——预检防御必须在主读之前（scan-and-repair + 重打包）。

**实现**：
- `crates/office-io/src/docx_panic_guard.rs`（新私有模块）：全部
  `.xml`/`.rels` 部件两趟字符串预检——裸 `w:u`/`w:highlight`/
  `w:vertAlign`/`w:sz` start tag 注入规范缺省 `w:val`（结构不变只加属性；
  前缀近邻 `<w:updateFields>`/`<w:szCs>` 不误伤）；`w:w` 值剥 `%` 后非
  数值时通用度量换算 twips（pt×20 / pc×240 / in×1440 / cm×566.93 /
  mm×56.69）、垃圾值归零（仅标签内部——正文文本含 `w:w="0pt"` 字样
  不误伤）；
- 无命中 `Cow::Borrowed` 直通（指针相等测试）；有命中重打包 zip
 （`raw_copy_file` 逐字节透传，docx_edit repack_parts 同构）；修复走
  既有 Warning 通道（裸元素=SkippedElement / 度量归一化=MalformedXml，
  不静默）；两模式恒等启用；
- `docx.rs::read_bytes_gated` 在 `docx_rs::read_docx` 前接线；
- mang-shell `docx_doc.rs`：`panic_guard`（catch_unwind → typed Err，
  String/&'static str/未知三类载荷）包住 `load`/`load_editable` 宽面读 +
  `DocxEditor::open`（窄面 panic 走既有降级只读分支——宽面已可看不丢
  查看）；`tests/docx_open_panic_corpus.rs` 四件 panic 源双装载回归。

**docx-rs 上游跟踪项（0.4.22 读层 panic 面——升版评估锚点）**：

| 位点 | 触发形态 | 语料实证 | 处置 |
|---|---|---|---|
| run_property.rs:110（Highlight） | `<w:highlight/>` 零属性 | props-enum-styles | ✅ 已修（注入 none） |
| run_property.rs:126（VertAlign） | `<w:vertAlign/>` | 同族防御面 | ✅ 已修（baseline） |
| run_property.rs:132（Size） | `<w:sz/>` | 防御面 | ✅ 已修（0） |
| run_property.rs:155（Underline） | `<w:u/>` | props-{b,u}-styles | ✅ 已修（single=ECMA-376 缺省） |
| width.rs:18（read_width） | `w:w="178.05pt"` 通用度量 | strict-sample-docx | ✅ 已修（twips 换算/归零） |
| paragraph_property.rs:45/51/65 | 裸 `<w:jc/>`/`<w:textAlignment/>`/`<w:pStyle>` | 未实证 | ⏸ 不修（pStyle 注入有语义风险）——入口 catch_unwind 兜底 |
| table.rs:38 / table_property.rs:35/40/45 | 裸对齐/宽度族 | 未实证 | ⏸ 同上 |
| style.rs:33 / numbering_property.rs:22/26 / numberings.rs:41/45/82 / level.rs:39-75 / level_override.rs:22 | 裸 name/level 族 | 未实证 | ⏸ 同上 |

（⏸ 位点 = 同类 `attributes[0]` 索引缺陷，语料未实证、注入缺省有语义风险
——宁缺毋错不进守卫面；未知形态由消费方入口 catch_unwind 兜底。docx-rs
升版时以本表 + err 20 件属性族（REPORT #E2）为验收锚点全量复跑。）

**验证面**：本地（Linux，1.99.0 工具链）——office-io 293 测试全绿零回归
（含守卫模块 9 测：裸元素注入/前缀近邻不误伤/度量换算边界/记账回归锁
（首版实现曾吞中间段——`no_repair_values_between_fixes_preserve_bytes`
钉定）/干净包借用直通/语料形态端到端（无防御 catch_unwind 实证 panic +
防御后 Ok + 双 kind 警告））；health_check_docx 双门绿（修复后
`HC_REGEN_BASELINE=1` 再生：docx4j panic 4→0 / ok 477→481 / roundtrip
383→386 / fixed_point 477→481，POI 零漂移——前后对照入 REPORT 批 6）；
mang-shell `cargo test --lib` 457 全绿 + 集成全套绿（CI 同口径）+
`docx_open_panic_corpus` 1/1（四件双装载 Ok）+ docx_edit_corpus POI 129
件实扫零回归（可编辑 97/降级 32/不可读 0）+ `cargo check --all-targets`
过（Linux 缺 xkbcommon-x11，经 deb 解包链接农场补齐）；fmt 我方文件
0 diff（mang-shell 既有全仓漂移不动——CI 不门禁该 workspace 的 fmt，
main 仓 fmt 门恒等）；Windows 真实环境面由 CI 三门禁覆盖。

## #227：PDF forms 保存真增量修订——已签名文档保护（2026-11-05，审计 P0-3）

**背景**：全仓审计 P0-3——`forms.rs` 模块文档与 `save()` 均声称
「增量修订、前缀逐位不变、既有签名仍有效」，实现却是 lopdf
`save_to` **全量重写**（save/flatten 两处），且无任何前缀测试。目标
场景「填已签名投标函」：填表保存 → 既有签名被**静默摧毁**（PAdES
硬约束违反；结构验签面 `read_signatures` 对此不可见——/Contents DER
原样可解析，ByteRange 已指向无关字节，损坏是静默的）。

**方案定案**：A（真增量）成体系落地，B（拒绝写）只用在**天生销毁性**
的 flatten 路径。理由：lopdf 0.45 `IncrementalDocument` 增量写机制
正是仓内 `writer_lopdf.rs`（PDF-d 标注面）已产品化并带语义锁的同款
机械——xref 追加段、/Prev 链、空档跳段（`xref.rs` 分段写）逐一读源
核实；夹具 `signed_sample.pdf` 本身就是 pyhanko 增量签名档（双 %%EOF
+ /Prev 链），装载/追加/重读全链路在既有测试面已覆盖。A 能做扎实
（前缀逐位不变可自动化断言），不必降级 B。

**实现**：
- `save()` 重写为 `IncrementalDocument` 追加修订：脏域对象**同对象号
  在新修订重定义**（后修订覆盖前修订）；会话新增对象（外观流/
  CIDFont/新建 AcroForm）随修订写出（`base_ids` 基线集区分既有/新增
  ——既有对象一字不重写）；三层守卫与 `writer_lopdf` 同款：①xref 链
  守卫（扫描重建装载档拒绝写，2026-10-31 事故防线）②前缀逐位不变
  断言（违约 = typed 错误不产出）③产物重载自检（页树可达）；
- flatten（保持全量重写——销毁性操作语义如此）增**已签名拒绝**：
  `has_signed_signature_fields()` 探针（/FT /Sig 且 /V 挂签名字典，
  直挂/间接引用两形态；未签署占位域不算）→ `FlattenRefused` +
  可操作指引（保签名走 save() 增量 / 定稿走副本 flatten 或重签）；
  已签名档打开即 `report()` 告警（保护可见化，不静默）；
- `tests/pdfl_forms_incremental.rs` 语义锁 7 条：前缀逐位 + 恰一修订
  段（%%EOF 计数）+ 重载新值生效；两代增量链传递；新定义落追加段；
  xref 守卫拒绝；**已签名投标函往返验收**（前缀不变 + /ByteRange
  覆盖段逐位相同 + 签名结构完整 + 填值生效 + 签名域保留）；签名探针
  与打开告警（未签名档对照无误伤）；flatten 拒签与指引。

**登记边界**：xref **流**（CrossReferenceStream）与混合引用档的追加
  写走 lopdf 同型路径但仓内夹具均为经典 xref 表（reportlab/pyhanko），
  未实测——与 `writer_lopdf` 面同边界；DocMDP/FieldMDP 变换约束未
  解析（夹具无 /Reference 限制；带限制签名档的追加编辑策略留后续）；
  `read_signatures` 为结构验签（无摘要复算——ByteRange 段逐位不变
  即摘要输入不变，密码学校验留 signature 面后续任务）。

**验证面**：本地（Linux）`cargo test -p mdc-pdf --features
engine-lopdf` **189 ok / 0 failed**（既有 182 + 新增 7）；fmt 0 diff；
clippy 零新警告；`cargo test --workspace`（默认面）与 wasm32/Windows
三 CI 门禁见 PR 记录。

## #228：打包链修复——NSIS 三断链 + Windows 链路就位（2026-11-06，审计 P2-37 / #68 方向二）

**背景**：分发链三断链（审计 P2-37，排期 R4·P0）：① NSIS 模板
`{{VERSION}}` 占位符零注入机制（BUILD-VERIFY 示例 `-DVERSION` 与 NSIS
方言不匹配且模板从不消费 define → 字面进 exe 名/注册表）；② WebView2
引导链 URL 定义未用 + `ExecWait` 幽灵 `$TEMP\wv2setup.exe`；③ `File
"..\.."` 相对路径与仓根执行示例矛盾必 not-found。macOS 仅 Info.plist
模板无构建路径（第二分支 feat/packaging-macos 收口，stacked on 本件）。

**侦察实证**：NSIS `File`/`OutFile` 解析语义探针钉死——按**脚本目录**
而非调用 CWD 解析（脚本在 X 目录、cwd=Y：`File "a/f"` 找 X/a/f、
OutFile 落 X/）——相对路径在「脚本目录 vs CWD」两套语义下天然歧义，
旧模板与仓根示例任一单侧假设都必 not-found（断链 ③ 的深层根因，旧
`..\..\` 反跳设计与「仓根执行」文档约定互斥）；主线壳 mang-shell 为
GPU 自绘渲染（DirectX 后端），无 WebView 运行时面（windows-check.yml
对旧壳 mdshell-native 的 GTK 排除注释同源——断链 ② 的面归属证据）。

**实现**（分支 fix/packaging-nsis-chain）：
- `packaging/version.py`：根 `[workspace.package].version` 读取器（=
  `mdcore::VERSION` 单源，r24 锁背书）——NSIS/Info.plist 注入链共用；
- `packaging/nsis/build-installer.py`：staging（mang-shell.exe →
  mdcore.exe 重命名 + pdfium.dll）→ 渲染（占位符集 VERSION/OUT_FILE/
  PAYLOAD_EXE/PAYLOAD_DLL；路径一律绝对 as_posix——NSIS 解析语义
  歧义的根治面）→ 断言（占位符零残留 + APPVERSION==版本单源 + 产物名
  含版本，违约非零退出）→ makensis（cwd=仓根统一口径）→ 产物校验 +
  SHA256；
- `installer.nsi.tpl` 重构：载荷 = mdcore.exe + pdfium.dll（exe 同目录
  = 定位序候选，#223/#224）；WebView2 面移除（断链 ② 处置 = 面移除，
  非链修复——面属旧壳）；安装后 `DeploySmoke` 跑 `--check-pdfium`
  （#224 复用，退出码 0=已绑定 / 3=未绑定；失败弹窗给修复指引不回滚
  ——Markdown/Word 面不依赖 pdfium）；卸载段补 pdfium.dll；APPID 恒
  dev.mdcore.editor（r24 门锁锚零破坏）；
- `.github/workflows/windows-installer.yml`：windows-latest 产 installer
  artifact（debug 内测口径——restore 复用 test-shell 缓存面控分钟数；
  choco nsis + pdfium pin chromium/8086 与 windows-test.yml 同源同
  SHA）；
- **顺带修复（本 PR 领域内既有门禁回归）**：packaging/README.md 与
  checklist.md 的 A3 记录行字面枚举 denylist 参考对象词 → 产品面扫描
  在 main 基线即红（干净 worktree @4ac0788 实证命中）——改写为「与
  denylist 全部参考对象词均不近似（判定依据 = sources.txt 四源字集）」，
  扫描复位 PASS。

**验证面**：本地（Linux，1.99.0 工具链 + Debian nsis 3.11 + NSISDIR
指路）——stub 载荷全链绿（版本 0.1.0 单源提取 → staging → 渲染断言 →
makensis 编译 → 产物 `mdcore-setup-0.1.0.exe` 109,192 B + SHA256
确定性复现）；`cargo test -p mdshell --test r24_packaging` **4/4**（门锁
零破坏）；`cargo fmt --all -- --check` 0 diff；`bash compliance/scan.sh`
PASS（产品面 0 命中）；`cargo test --workspace --exclude mdshell-native`
**147 test-binaries ok / 0 failed（exit 0）**——mdshell-native 因本沙箱
optroot GTK 提取树缺件不可编译（Linux GUI 面；CI windows-check 同排除
口径，deb/AppImage 构建面不受本 PR 影响）；wasm32 `cargo check
-p mdcore-wasm` 过；Windows 真实环境面 = CI 四门禁（check / engine
tests / mang-shell / nsis installer）。

**登记边界**：CI 内测包 = debug 构建（正式出包 = BUILD-VERIFY §3 的
release 命令，Lead 实机）；mang-shell 为独立 workspace，版本号未接根
单源机制（结构上无法 version.workspace 继承，当前两值一致 0.1.0；
漂移时安装包版本以 mdcore::VERSION 为准，对齐留后续任务）；NSIS 方言
= Debian makensis 3.11（本地）+ choco nsis 3.x（CI）双实证；安装行为
面（注册表/快捷方式/.md 关联/自检弹窗）CI 不装包——待 Lead Windows
实机复核清单（BUILD-VERIFY §3）；图标仍占位（checklist C5 资产轮，
非本件范围）。

