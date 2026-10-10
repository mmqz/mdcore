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
| 终态 JS 堆 | 168.8MB（G1，R14 旧口径） | **R22-a 口径：used=483.5MB = JS 58.7MB + wasm 424.8MB**（R21 基线 484.1 = 58.7 + 425.4；R22-a Arc<str> 实测 Δ-0.6MB ≈ 噪音，估算撤回见 R22-a 段） | 1MB <250MB；10MB <600MB（均非门） | 1MB 达标；10MB JS 堆 58.7MB 达标；wasm 509→425→424.8MB，高水位主驱动 = backup 克隆（R22-b 登记治理） |
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
System 事务（ADR-B4）失败走 ADR-B5 求逆回滚。

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
产品仓 R22-b 合流后复测构成）；take-external 全量重建的 log 内存杠杆（10MB 高频
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
② **库定位序**：env `MDC_PDFIUM_LIB` → 编译期候选清单（产品仓
`bin/lib`（fetch-pdfium.sh 取件位，PDF-e 接线）→ 分析仓 spike
`bin/lib`（门禁环境）→ crate 本地）→ 系统（bind_to_system_library）；
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
化）/ perf_random_flip 正名为 perf_sequential_flip（两阶段顺序取页，
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
