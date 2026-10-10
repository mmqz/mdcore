# Prototype — Headless Core v0（R8）+ CM6 桥（R9）+ 渐进打开/宿主契约层（R10）

> 状态：✅ R8 九项清单全部落地（2026-10-08）｜ workspace `cargo test` 42/42 绿 ｜
> `cargo check --target wasm32-unknown-unknown` 通过 ｜ inspector 示例可运行。
> ✅ R9 桥落地：浏览器内 CM6 ↔ mdcore-wasm 全链路，W1-W6 冒烟 6/6 绿。
> ✅ R10 落地（2026-10-08）：渐进打开（核内 `extend_from_markdown` 追加式
> 分块 + 文件层 document_id/开档存档）+ 宿主契约层 `crates/shell`
> （headless 可验证）+ W7 冒烟（1MB TTI 153ms vs 全量 361ms）。
> ✅ R12 落地（2026-10-08）：Human+AI 闭环最小实现——`commit_transaction_as`
> （base gate + Source::Ai 溯源合一）+ 合并窗收紧为两端 View；
> `r12_ai_loop` 10 条 + 浏览器 W11。workspace 55/55 绿、W1-W11 11/11 绿。
> ✅ R13 落地（2026-10-08）：格式扩展第一环——HTML ↔ DM 边界编解码器
> （`html.rs`，块级子集 + 行内五 mark + 结构化 Loss Report）+
> `r13_html_roundtrip` 19 条。workspace 74/74 绿。
> ✅ R14 落地（2026-10-08）：第一阶段硬 Gate 量化收口（§15.3）——Gate 1/3/4
> PASS、Gate 2 GAP（光标 p95 23.9ms>16ms，归因=装饰随光标全量重建，登记
> R15 专项）；10MB 量化入档（堆 803MB FLAG）。判定表 `gates.md`。
> ✅ R15 落地（2026-10-09）：Gate 2 视图专项收口——装饰**增量窗口化**
> （纯 selection 事务只重算新旧选区覆盖行 ±1 真并集，共享单行生成函数；
> decoField 值改 {deco, atomic} 元组，atomicRanges facet O(1) 取缓存）+
> W12 等价性护栏（增量 ≡ 强制全量，六步选区序列）。光标 p95 23.9ms →
> **5.0ms**（<16ms 硬门 PASS，~4.8×）；typing p95 33.9ms 持平。
> 冒烟 W1-W12+G1/G2 14/14 绿。判定表 `gates.md`。
> ✅ R16 落地（2026-10-09）：10MB 量化收口——核侧分片出口
> `blocks_count`/`blocks_json_range`（先序区间查询，免全量 dump）+
> BlockMap 增量建图（`extend`，渐进 extend 只解析新增区间）+ ingest
> 批渲染（每段免全量重渲染，结束一次 pushRender + revision 守卫）。
> TTI 362.7 → **284.7ms**（<300ms）、ingest 11.9 → 4.2s、undo 2.27 →
> 1.90s；终态堆 803 → 631.3MB（-21%，测量口径修正为 precise+GC 开档
> 稳态；残余 ~500MB 未归因驻留登记 R17）。`r16_blocks_range` 3 条 +
> 冒烟 14/14 绿；workspace 82/82。
> ✅ R17 落地（2026-10-09）：主档 P1 队列两项收口——**P1-9 单位宪法**
> （ADR-V2：码点↔UTF-16 单一换算函数对 + 非 BMP 语义锁 11 条 + 浏览器
> W13 五断言）+ **P1-4 Block ID 边界规则**（ADR-B4：table 单块/嵌套
> list 继承/paste 按位对齐槽位继承，语义锁 15 条）。**顺带修复 R9 以来
> 潜伏的 BlockMap 行对齐漂移**（rebuild 不数块间空行 → fallback 后快
> 路径对空行之后的块恒失明——W13 抓出）。冒烟 W1-W13+G1/G2 15/15 绿；
> workspace 109/109。
> 计划基准：仓库根 `project_panorama_master_2026-10-07.md` §R8/§R9/§R10/§R11；
> View 层裁决：R7 Spike 报告（`../spike/report.md`）——范式 B 条件性采纳，
> Core 保持 op-first。

## 1. 布局

```text
projects/prototype/
  Cargo.toml               workspace（default-members = mdcore）
  crates/mdcore/           Headless Core v0（Rust，零运行时依赖）
    src/{lib,id,tree,op,command→lib,history,anchor,revision,snapshot,
         markdown,debug}.rs
    tests/spike_semantics.rs   Spike A-E 语义测试（14 条）
    tests/sync_resync.rs       R9 重同步兜底测试（10 条）
    tests/r11_stress.rs        R11 压测（3 条：批量事务/身份/重放）
    tests/r12_ai_loop.rs       R12 Human+AI 闭环（10 条：ScriptedAi 替身）
    tests/r13_html_roundtrip.rs R13 HTML round-trip fixture CI（19 条）
    tests/r14_gates.rs         R14 硬 Gate 核侧（5 条：snapshot/restore 恒等 + 计时打点）
    src/html.rs                R13 HTML 边界编解码器（子集/Loss Report/降级）
    examples/inspector.rs      对象级检视面演示
  crates/mdcore-wasm/      WASM 绑定（wasm-bindgen，JSON 边界）
  crates/shell/            R10 宿主契约层：open/progressive/save/document_id + Host trait（headless 可验证）
  crates/shell-native/     R10 原生壳蓝图（wry+Slint；随附零依赖可测，原生依赖需 webkit2gtk-4.1）
  web/                     R9/R10：CM6 View ↔ WasmCore 桥 + W1-W7 冒烟（见 web/README.md）
```

构建 / 测试 / 演示：

```bash
cargo test  -p mdcore                      # 74 tests（3 unit + 14 语义 + 10 重同步 + 3 压测 + 10 R12 + 19 R13 + 5 R14）
cargo check -p mdcore-wasm --target wasm32-unknown-unknown
cargo run   -p mdcore --example inspector  # 全流程演示（含 stale reject）
```

依赖策略（主档技术栈约束内）：`ropey`（文本存储）+ `serde/serde_json`
（序列化边界）+ `wasm-bindgen`（仅绑定层）。Core 语义本身不依赖任何框架。

## 2. R8 清单对照（主档 §阶段R8 九项）

| # | 主档项 | 实现 | 验证 |
|---|---|---|---|
| 1 | Document / Block Tree | 虚根 arena 树（`tree.rs`），§8.1 块型全集（含 table） | spike_a_* |
| 2 | Stable Block ID | 单调分配器 + BTreeMap 注册表，永不回收（`id.rs`/§8.2） | spike_b_identity_* |
| 3 | Rope | per-block `ropey::Rope`，char offset 语义 | 全部文本路径 |
| 4 | Command | §10.2 十原语（`lib.rs` 命令层） | spike_c_all_first_stage_primitives |
| 5 | Transaction | `apply_ops` 批 op 原子性（备份回滚）+ `commit_transaction` | 同上（单历史条目断言） |
| 6 | History | op 栈 + 1s 时间合并 + kind 语义破组 + 整栈 JSON 往返 | spike_d_* |
| 7 | Anchor | (BlockId, offset) 解析层 + TextTarget 双模（`anchor.rs`） | spike_e_anchor_mapping_and_rejects |
| 8 | snapshot / restore | 递归 SnapNode JSON + allocator 水位复原 | spike_a_snapshot_* / spike_d_snapshot_* |
| 9 | stale / conflict tests | 结构命令 base_revision 硬门禁 + 锚点三类 reject | spike_e_structural_stale_must_reject |

另有：Revision 日志（append-only JSONL + genesis 自含 + 确定性重放）、
事件队列（View/AI 共同 op 流）、debug 检视面（可诊断性内建）、
WASM 绑定（JSON 进出，`WasmCore`）。

## 3. 关键设计决策（含依据）

1. **双轨寻址 = ADR-B1 混合式的直接落地**：mutation op 走 `NodePath`
   （顺序重放安全，与 spike core 契约同构）；意图/延迟命令走 `BlockId`
   （注册表解析，结构变化后仍可达——`spike_e_anchor` 的"移动后锚点编辑
   成功"用例即其价值时刻）。
2. **per-block Rope，不做整篇 Rope**：主档 §9——Rope 只是文本存储实现，
   不得成为第二个语义真理；整篇 source buffer 由 View 层 CM6 承担
   （R7 裁决），Core 不重复持有。大块（code fence 多行）的局部编辑
   复杂度仍由 ropey 保证。
3. **7 个原子 op + 命令层展开**：split = ReplaceText+InsertBlock、
   merge = ReplaceText+RemoveBlock、commit_source = SetType+SetAttrs+
   ReplaceText（或 remove+重建）。每个原子 op 可精确求逆 → undo 栈
   只存原子序列（spike core.ts 同款技巧）。
4. **ID 物化（identity 硬保证的关键）**：`InsertBlock.id: Option<u64>`——
   正常路径 None（现场分配并回写 op），历史条目/事件/inverse 携带具体 ID；
   undo 复原 `RemoveBlock` 时按先序重建并**恢复原 ID**；redo/replay 同理。
   配合 allocator 水位只升不降，任何路径都不会复用旧 ID（§8.2/§8.4）。
5. **MoveBlock 的摘除后语义（post-removal indexing）**：先 detach 再按
   `to_index` 插入，同父跨位零偏移校正；其 inverse 可证明为
   `(from = to_parent+[to_index], to = 原位置)`，已在测试矩阵覆盖。
6. **Revision 语义（§11.1 无击键风暴）**：合并窗口内的连续 text op
   **共享同一个 revision**；结构命令 / AI 命令 / undo / redo 各自 bump。
   结构命令 gate（§11.2）= `base != current → Reject::Stale`，绝不 rebase。
7. **事件队列而非回调**：`take_events()` 轮询式取走 op 流——跨 WASM 边界
   友好（JS 侧轮询），也避免借用纠纷；View A/B 与 AI 消费同一条流
   （§22 不变量 1/10）。
8. **日志自含（genesis）**：`from_markdown` 把初始解析结果物化为首条
   日志记录，`Core::replay_jsonl` 从空核即可确定性复原状态与 revision
   （op 内 ID 已物化 → 重放身份不变）。
9. **单位宪法（ADR-V2，R17 / 主档 P1-9 收口）**：核侧（ropey / op.start/
   end / Anchor.offset）= **Unicode 码点**；CM6 / JS 串 = **UTF-16 码元**；
   桥以单一函数对 `utf16ToCp / cpToUtf16`（main.ts 模块级，`__bridge.unit`
   暴露）做全部跨单位换算，禁止散落手工计数。核侧语义锁
   `r17_utf16_mapping.rs`（11 条非 BMP 性质测试：ZWJ 家族/CJK 扩展 B/
   变体选择符，round-trip/undo 恒等/锚点解析/JSON 边界）；浏览器端
   `W13`（快路径 op end-start==1 码点，UTF-16 侧 span=2）。快路径的
   行内前缀/marker 均为 ASCII，是其 cp↔UTF-16 同值的前提不变量。
10. **Block ID 边界规则（ADR-B4，R17 / 主档 P1-4 收口）**：① table 为
   单块建模，表内行编辑=块内 replace_text，身份不变；② 嵌套 list
   split：新 item 现场分配新 ID，marker/depth 继承，子树随头块；
   merge：幸存者=前一个兄弟；③ 跨块 paste（sync_from_markdown 位置
   差分）：**按位对齐槽位继承**——对齐槽位的既有块保 ID（内容被
   改写也保），尾部新增/放不下尾段=新 ID，undo/redo 身份复原。
   语义锁 `r17_id_boundaries.rs`（15 条）。已知代价：对齐规则下中段
   改写块"名存实亡"（ID 在、内容换），可预测性优先。

## 4. v0 已登记的简化（R9 验证/升级清单）

| 简化 | 现状 | R9 动作 |
|---|---|---|
| mark 截断有损 | 跨越 mark 的删除截掉 span 后，undo 不复原被截部分（与 CM6 映射语义一致） | 若需精确复原，把 mark 状态并入 ReplaceText 的 inverse 元数据 |
| undo 分组粒度 | 时间窗 1s + kind 切换破组；无词边界/输入类型破组（实测项） | ADR-B2：真键序样本实测调参 |
| 锚点偏移映射 | 块存在即按现 offset 应用，不做周围文本变化的自动平移；`expect` 内容保护代替 | §11.3 完整映射（CM6 ChangeSet 式 position mapping） |
| markdown 边界 | fence 仅根层；quote 内列表/列表内 quote 的解析规则刻意最小；多行段落 round-trip 不保证 | 对齐 `analysis/deep_dive/grammar_and_serializer.md` 的 CJK/围栏规则 |
| document_id | 未开始（§8.3 workspace metadata / UUIDv7 / 重开匹配） | R9 文件层引入 |
| Spike F | 真 IME + 渐进打开 TTI 需要真实 WebView | 真 IME 留 R11；synthetic composition 已过（web W3） |
| mdcore-wasm | JSON 字符串边界 + wasm32 check 通过；未在浏览器内冒烟 | ✅ R9 已冒烟：`web/` 全链路 W1-W6 6/6 绿 |

## 5. 对 R9 的输入（已消费，留档）

- **桥代码起点**：`crates/mdcore-wasm` 的 `apply_ops/replace_text_at_anchor/
  take_events` 即 spike/b `main.ts` 桥的 Rust-WASM 化对端。
- **主档 R9 清单**：CM6 / WebView / IME / progressive open / Markdown
  rendering——Core 侧语义不变量已全部就位（本 README §2），进入
  语义不变量全绿后的宿主阶段。
- **性能基线**：Spike S4 数据（1MB/5MB，范式 B 15.5ms/33ms 初始渲染）
  是宿主层的对照基线；Core 自身开销在 S4 p95 中已被证明非瓶颈。

## 6. R9 产出（2026-10-08）

- **`web/` 全链路桥**：浏览器内 CM6 ↔ WasmCore，快路径（单 change
  ReplaceText op）+ 兜底（`sync_from_markdown` 核内位置差分，走 apply_ops
  管道保持 history/事件/revision 一致）；BlockMap 行模型与
  `markdown.rs::render` 逐字段对齐。
- **核侧新增**：`sync_from_markdown`（最小 op 差分重同步）、
  `blocks_json`（机器可读块树平铺）、`set_clock`/`set_clock_ms`
  （wasm 时钟注入，合并窗依赖）+ 10 条重同步单测（26/26 绿）。
- **W1-W6 冒烟 6/6 绿**（Playwright chromium headless，证据
  `web/tests/results/results.json`）：1MB 加载 384ms / 打字 p95 43.7ms /
  兜底 sync 304.5ms / undo 重渲染 165.1ms / 20448 块。
- **审查战役三缺陷修复**（详见 web/README.md §4）：装饰 facet 未入 DOM
  （潜伏自 spike/b 的断言面盲区）、redo 键位大小写失配（w3c-keyname +
  Playwright 合成怪癖双重因素）、快路径选区映射断言方式。
- **R10 对接点**：`web/dist/` 即 wry 宿主加载物；宿主最小面 =
  窗口/文件对话框/初始文本注入；渐进打开以 1MB 384ms 全量路径为基线。

## 7. R10 产出（2026-10-08）

- **渐进打开（核侧）**：`Core::extend_from_markdown` —— 追加式吃文档
  后缀（位置差分零参与，不碰已有块），InsertBlock op（id 物化）走
  apply_ops 管道（Source::System，永不进合并窗）；`is_complete_chunk`
  悬挂 fence 防线 + Reject 不污染树；**one-shot vs 分块 extend 的树/
  先序 ID/序列化三重相等为硬测试断言**（1MB 级）。
- **文件层（document.rs，R8 简化表清账）**：`DocumentId`（UUIDv7 形状，
  零外部依赖，时钟注入）、`open_document`/`save_document` 往返
  （树/revision/历史栈逐字节一致）、genesis 日志自含 document_id、
  重放恢复 + 冲突拒绝 + 旧格式向后兼容。
- **宿主契约层 `crates/shell`**：`Host` trait（surface_load_markdown）
  + `open_document`/`open_document_progressive`（OpenProgress 事件：
  FirstChunkInteractive/ChunkAppended/Completed）+ save；headless
  测试替身 4 条（原生 wry 后端的可验证契约）。
- **原生壳蓝图 `crates/shell-native`**：wry+Slint 胶水结构定稿
  （tao 事件循环/进度条/文本注入 `__inject`/保存路径五对接点）；
  随附零依赖可编译可测试，原生依赖（webkit2gtk-4.1-dev，沙箱无
  sudo 不可装）在原生机器启用——见 crate 文档。
- **W7 冒烟**（`?doc=big&kb=1024&chunks=8`）：TTI 153.2ms vs 全量
  360.7ms（**-57%**），ingest 690ms（含每段全量重渲染 O(K·n)，v0
  登记），终态 20448 块与一次性打开一致；ingest 窗口 `locked` 防护
  （半开状态拒绝编辑，防 fallback 误删未渲染块）。
- 测试：workspace 42/42 绿（mdcore 37 + shell 4 + shell-native 1）。

## 8. R11 验证轮产出（2026-10-08）

R8 遗留三项验证清单全部收口：

| 验证项 | 核侧（Rust） | 桥侧（浏览器 W8-W10） |
|---|---|---|
| 长事务桥压测 | `r11_stress`：20k 块上 200-op 批量事务单 revision、坏 op 全回滚、50×MoveBlock undo 身份还原、日志重放复现 | W8：200-op 批量 14.9ms、全量重渲染 111.8ms、**幂等 sync 零差分**（44.4ms）、批量 undo 127.3ms、CM6 多 change 单事务兜底吸收 127.2ms |
| 深 IME（synthetic） | — | W9：连续三轮 composition + 取消路径零残留 + 核/视图收敛；真 IME 人工验收协议见 web/README §6 |
| 大文档块操作 | 同上（20k 块 split/merge/move 级） | W10：20448 块上核驱动 split 156.1ms、fallback merge 275.2ms、结构 undo×2 322.7ms 且首批 ID 逐一还原 |

**审查战役两处真实缺陷修复（核级）：**

1. **MoveBlock 空 `from` 路径减法溢出 panic**（`op.rs` 求逆路径）——
   虚根不可移动的边界从未被防；现为 `Reject::Invalid(BadPath)`。
2. **undo/redo 不写 Revision 日志 → 重放与实时状态分叉**——违背
   "日志自含 + 确定性重放"承诺；修复后 undo/redo 以 `Source::Undo/Redo`
   记入日志，重放 = 实时状态（新增硬断言：moves+undo 的日志重放逐字节
   复原）。

**审查战役两处测试面缺陷（浏览器）：**

3. **CM6 MutationObserver 异步吸收**——composition/execCommand 后同步
   断言会假阴性，须 await tick；此前 W3 的通过属"另起 evaluate"侥幸。
4. **桥直调 vs 事件出口**——直调 `core.undo()` 不经桥的 drainEvents，
   事件留队列；测试必须走 `__bridge.undo()`。

测试终态：workspace Rust 45/45 绿（mdcore 40 含 r11_stress 3 条 +
shell 4 + shell-native 1）；浏览器 W1-W10 10/10 绿。

## 9. R12 产出：Human + AI 闭环最小实现（2026-10-08）

主档 §阶段 R10 闭环 `Human command → Document State ← AI command` 四条
验证点逐条落成 Rust 测试（`crates/mdcore/tests/r12_ai_loop.rs`，10 条，
headless 优先）。AI 替身 `ScriptedAi`：observe（只读 `blocks_json` +
`take_events`）→ plan（确定性纯函数）→ submit（Command Boundary）；
自然语言永不进核（§10.2 红线）。

| 主档验证点 | 测试 | 断言要点 |
|---|---|---|
| 1. AI read current state | `ai_reads_human_edited_state_as_decision_input` | Human 三连编辑后，AI 从 `blocks_json` 读到的块树平铺（内容/ID/层级/路径，stack-by-depth 复原 NodePath）与核内按路径解析逐一相等；事件流为第二读通道；`plan_summarize` 输出文本由读到的现场决定 |
| 2. AI 生成 deterministic commands | `scripted_ai_plan_is_deterministic_and_primitive_only`、`ai_fills_empty_paragraph_via_command_boundary` | 同一观察 → 同一 op 序列（跨核逐字节同 JSON）；计划只含 §10.2 原子 op（7 原语白名单断言）；换状态 → 不同计划（计划是状态的函数） |
| 3. Core execute + revision | `ai_fills_empty_paragraph_via_command_boundary`、`ai_text_command_bumps_own_revision_never_coalesces`、`ai_batch_is_atomic_on_bad_op` | AI 批走 `commit_transaction_as`（gate → apply_ops 既有管道，零旁路）；`Source::Ai` 入历史/事件/日志（`"source":"ai"`、`command:"ai.summarize"`）；AI 命令各自 bump revision、永不进 1s 合并窗；批量事务一个坏 op → 整批 reject + 树/历史/事件零污染 |
| 4. undo / replay / stale | a) `ai_batch_undo_reverts_whole_batch_not_human_edit`、`ai_entries_survive_save_open_and_undo_whole_batch`、`human_and_ai_alternate_on_one_document_state`；b) `ai_stale_transaction_rejected_then_retries_with_fresh_base`；c) `ai_inclusive_log_replays_to_identical_state` | a) 多 op AI 批 = 单条历史条目，一次 undo 整批回退且不吞 Human 编辑（save/open 往返后仍成立；三轮 Human/AI 交替后 undo 链逐条对账）；b) Human 抢先改状态后 AI 旧 base 提交 → `Reject::Stale`（树/revision/历史/事件零污染，绝不静默 rebase）→ AI 重读重规划、新 base 重试恰好生效一次；c) 含 ai/undo/redo 记录的日志 `replay_jsonl` 与实时状态 dump_tree/序列化/blocks_json/revision 逐一相等（R11 硬断言风格） |

**核心 src 改动（两文件，最小改；理由如下）：**

1. `lib.rs` 新增 `commit_transaction_as(base, ops, meta)`（+`commit_transaction`
   改为其 View 薄包装，签名不变）：此前带 base_revision 硬门禁的命令面
   硬编码 `Source::View`——AI 只能二选一：`apply_ops`（保住 Source::Ai
   溯源，但拿不到 §11.2 stale gate）或 `commit_transaction`（拿到 gate，
   却把 AI 命令误记成 View 源）。新方法让调用方（宿主/Planner）同时声明
   base 与 meta；执行仍走既有 gate → apply_ops 管道，无旁路
   （§10.1/§22 不变量 2/5/8）。这是闭环的"AI 提交协议"缺口，非它不可。
2. `history.rs` 合并窗收紧：`should_coalesce` 增加 `last_source == View`
   条件（合并要求**条目两端都是 View**），`push_entry`/`coalesce_into_last`
   随条目记 source。修复真实缺陷——旧规则只查 incoming 侧，AI/System
   事务后 1s 内的 Human 击键会被吸进 AI 条目，一次 undo 连人手字符一起
   吞掉，直接违背验证点 4a。`last_source` 走 serde default（旧档恢复后
   首键开新条目，保守安全，向后兼容）。

**验证终态**：workspace 55/55 绿（mdcore 50 = 3 unit + 14 spike +
10 sync + 3 r11 压测 + 10 r12；shell 4；shell-native 1）；
`cargo check -p mdcore-wasm --target wasm32-unknown-unknown` 通过；
浏览器 W1-W11 11/11 绿（W11 = 经 JSON 边界 `apply_ops source="ai"`
注入 AI op 流 → CM6 渲染 AI 改动 → view/ai/view 同一事件流 +
AI op id 物化 + 两段 undo 语义端到端，证据 `web/tests/results/results.json`）。

**登记简化项（后续轮次清单）：**

- `ScriptedAi` 为规则式确定性替身（空段落 → 填摘要+提级+文末追加），
  非 LLM；真集成时只替换 `plan_summarize` 实现，observe/submit 协议不变
  （§10.1 Planner/Translator 归宿）。
- stale 恢复 = 重读状态 + 重规划 + 新 base 重试（§11.2 禁 rebase 的直接
  推论）；未做 AI op 对 Human op 的自动 rebase/合并。
- §10.3 最小协议的 command_id / idempotency strategy 暂由 `command`
  署名字符串 + base gate 粗粒度承担；resulting revision 已由 Receipt 返回。
- 浏览器侧 AI 注入走 `apply_ops`（有溯源、无 base gate）；带 gate 的
  `commit_transaction_as` 尚未透传到 wasm 绑定（Rust 侧已验证，宿主需要
  时一行透传即可）。
- 合并窗"两端 View"规则对 AI 辅助连续润色（AI 改完人手立刻续写）会
  多产生一条 undo 条目——这是 4a 正确性的代价，调参依据 ADR-B2。

## 10. R13 产出：HTML ↔ Document Model ↔ MD round-trip（2026-10-08）

主档 §阶段 R11 第 1 项 + §18 同族转换策略。DM 仍是唯一语义真理（§9）：
`html.rs` 与 markdown.rs 同构的边界编解码器，`BlockKind`/`MarkSpan`/`Attrs`
零改动。模块子集/降级规则/有损点全文见 `src/html.rs` 头部文档。

| 面 | 内容 |
|---|---|
| 导入（块级） | p / h1-h6 / ul·ol·li（嵌套）/ blockquote（嵌套）/ pre>code（lang）/ table（thead·tbody·tr·th·td）/ hr |
| 导入（行内） | strong·b→strong，em·i→em，code→code，del·s→del，u→u（SetMark 自由 kind）；br→块内换行；未知行内标签透明（文字保留、样式丢弃） |
| 导出 | 语义 HTML fragment；全文本转义 `& < > " '`；属性最小化（code 的 language-class、ol 的 start）；mark 活跃集归一化保证标签合法嵌套 |
| 降级 | 未知/分节容器透明穿轴+留痕；script/style/iframe 等整体丢弃+留痕；空/退化表格原文落段+RawBlock；超深嵌套（>256）截断+RawBlock；不规范输入不 panic |
| Loss Report | `import_html` 返回 `Vec<Loss>`（offset / original / disposition / note），§18 结构化有损报告，AI 与 UI 可消费 |

**round-trip fixture CI**（`tests/r13_html_roundtrip.rs`，19 条）：
DM→HTML→DM 三重恒等（dump_tree / blocks_json / MD 序列化）覆盖标题·段落·
嵌套列表·有序列表 start·嵌套引用·fence·表格·hr·CJK；mark round-trip
（strong/em/code/del/u span 逐一还原 + br 换行还原）；MD→HTML 渲染契约；
网页粘贴样例 → MD；降级与 Loss 断言；转义安全（导出转义、实体伪装标签
不二次解析）；畸形输入不 panic。

**施工审核中修复的实现缺陷（三处，均补测试锁定）：**
1. `tokenize` 的 char-offset `coff` 不随正文文本推进 → flush 时
   `coff - run 字符数` 减法下溢 panic（任何含正文的输入即崩）。
   修复：文本分支按字符推进 coff，维持 `coff == chars(input[..i])` 不变量。
2. `depth` 属性 0 起始，与 markdown.rs（缩进/2+1，1 起始）不一致 →
   同一文档两条导入路径 dump_tree 不等。修复：`(list_depth + 1)` 对齐。
3. 表格分隔行规范化带内边距（`|  ---  |`），与 markdown.rs 常规形态
   （`|---|---|`）不一致 → Table 文本两条路径逐字节不等。修复：分隔行
   无内边距。

**登记简化（后续轮次清单）：**
- 浏览器侧 W12（粘贴 HTML → 桥 → 块树）留待宿主轮：桥侧无现成粘贴挂钩，
  本轮不为它改桥架构（子代理任务书既定处置）。
- 行内空白按 HTML 语义折叠：canonical 内容恒等，非规范文本是规范化而非保真。
- 表格单元格行内 mark 不可承载（Table 文本 = 管道行）；对齐信息（`:---:`）
  不保留。
- li 内多段落折叠进条目文本；同层混排 ul/ol 共享一个列表标签。
- 导出仅认识 strong/em/code/del/u 五种 mark；其余 kind 不导出（DM→MD 同样
  不写 mark，方向一致）。

## 11. R14 产出：第一阶段硬 Gate 量化收口（2026-10-08）

主档 §15.3 四门逐条判定 + §28 P0-3 拆分实测，**判定表与 GAP 归因全文见
[`gates.md`](gates.md)**；证据 `web/tests/results/results.json`（W1-W11 +
G1/G2）与 `crates/mdcore/tests/r14_gates.rs`（核侧，串行计时）。

| Gate | 判定 | 要点 |
|---|---|---|
| 1. 1MB 可输入 <600ms | **PASS** | 全量 349.4ms / 渐进 TTI 156.9ms |
| 2. 击键→光标 <16ms | **GAP** | 纯光标路径 p95 23.9ms；归因=decoField 对 selection 事务全量重建 O(doc) 装饰（revealed() 光标协商语义所致）；核内份额仅 8.7ms——修复（facet 拆分/窗口化重建）登记 R15 首选项 |
| 3. snapshot/restore 完整 | **PASS** | 20k 块三重恒等 + 跨档 undo 链；snapshot 125ms / restore 216ms |
| 4. 中日文 IME | **PASS（synthetic）** | W3/W9 零残留；真机人工协议成文（沙箱无真 IME，待办） |

10MB 量化（G2）：渐进 TTI 362.7ms、完整 ingest 11.85s、undo 2268ms、
终态堆 803MB（非门预算 600MB 超出 → FLAG）；1MB 堆 168.8MB（<250MB 达标）。
核侧发现：单 op 成本随文档规模增长（小文档 0.02ms → 20k 块 5.97ms，300×），
p95 仍在 10ms 增量解析预算内。测量卫生：核侧计时测试必须串行（并行互扰
使 p95 虚增 5 倍，教训入档）。R15 候选清单（Gate 2 视图专项 / 10MB 堆 /
10MB 首屏）见 gates.md 末节。
