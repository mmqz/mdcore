# REUSE.md — MD 模块复用指引

> 产出：T3 复用地基轮（2026-10-12）。依据 = boundary-audit 审计报告的
> 12 复用单元三分类（锚点证据链与独立复审记录在分析仓
> md-line/boundary-audit/，本文为产品内消费视角转录，零新研究）。
>
> **三域纪律（用户裁决 2026-10-12）**：MD / PDF / Office 保持独立领域
> 模型与引擎实现；只复用有明确共性的基础能力；不建通用引擎、不做
> 统一 CRDT、不做双向投影；跨引擎统一留后续阶段评估。

## 0. 快速指引（按消费者类型）

| 你要做什么 | 去哪里 | 第一步 |
|-----------|--------|--------|
| 只要设置面基础设施（任意域） | `crates/settings`（mdc-settings） | 依赖该 crate——对块核**零依赖**，四层 schema + TOML sidecar + 六态降级开箱即用 |
| 接入第二文档模型（并列引擎） | 参考 `crates/pdf` 先例 | `DocumentEngine` trait（engine.rs:17-37，object-safe 四方法）+ 引擎/写器独立装配 + 装配期页数对账 |
| 在 MD 文档域内消费（AI/工具/宿主） | §1 R1-R5 直取 | Rust 直调或经 wasm 桥，无需适配层 |
| 订阅变更 / 按块取数 / 块级监听 | §2 A1-A3 | 消费方侧包装（核不改） |
| 跨域（画布等异构模型）要 undo | §3 X1-X2 | 代码不可移植，架构模式可移植 |

## 1. 可直接复用（R1-R5，不改核即可调用）

| # | 单元 | 接口（锚点） | 必跑验证 | 关键坑位 | PDF 质量 |
|---|------|-------------|----------|----------|----------|
| R1 | 命令调用协议 | `Core::apply_ops`（lib.rs:253，唯一变异入口）+ 十原语（:387-618）+ `commit_transaction_as`（:610，gate+溯源）；wasm 面 `apply_ops/replace_text_at_anchor/undo/redo` | 默认面全量 + 契约门 18/18（web 面） | ①结构性命令带 `base_revision` 硬门禁（Stale 必拒绝不 rebase）；②文本命令无 base 门禁，靠锚点/expect 保护；③`Core` 字段 `pub tree/history/log` 直访旁路在类型层敞开——复用方自律或包装 | 零波及 |
| R2 | 序列化五面 | `tree_to_markdown`（markdown.rs:32）/ `blocks_json(_range)`（debug.rs:39/69）/ `Snapshot`（snapshot.rs:44）/ `history_json` / revision JSONL（revision.rs:60） | 默认面全量（含 r13 往返 19 条 + r23_fingerprint 15 条） | **marks 不写入 MD、ID 不写入 MD**（markdown.rs:8）——持久身份走 meta/snapshot/log 面；`blocks_json` 不含 marks | 零波及 |
| R3 | undo/redo 闭环 | `invert_op`（op.rs:243）× 1s 合并窗（COALESCE_MS）× 栈管理 + 失败全回滚（lib.rs:920-962，整树备份恢复） | t3_reuse_locks + r24_quality_fixes + r11_stress + r25_inverse_group_order | ①合并窗五条件（R12 双端 View：AI/System 边界必开新条目）；②undo 不恢复选区（View 域）；③栈无上限（长会话内存形状，产品裁决项）；④`entry.inverses` 的序 = **组间逆序组内保序**（#220：`invert_op` 的逆组多为长度 1，RemoveBlock 逆 = 先序重建子树）——直接消费 inverses 的方面（重放/审计工具）不得假设逐 op 扁平逆序 | 零波及 |
| R4 | 渐进打开 + 分片 dump | `extend_from_markdown`（lib.rs:849）+ `blocks_json_range`（debug.rs:69） | document_progressive + r16_blocks_range | ingest 窗口 `locked` 拒绝编辑；悬挂 fence 在 op 构造前拒绝（树零污染） | 零波及 |
| R5 | 设置面基础设施 | **`crates/settings`（mdc-settings）**——本轮抽取件 | r24_settings 全套 + closure_lock | 域无关（对块核零依赖）；`undo_coalesce_ms` 为**登记保留键**（存储链齐备、当前不驱动引擎行为——接线属产品裁决） | 零波及（PDF 面未消费） |

## 2. 需要最小适配（A1-A5，消费方侧完成，零改核）

| # | 能力 | 现状 | 适配内容（落点=消费方） |
|---|------|------|------------------------|
| A1 | 变更订阅 | `take_events` 拉取式单消费者 drain（lib.rs:237） | 自建轮询泵 + 分发层（单队列语义：第二消费者不得与既有桥争抢——先取先得）；推送/多订阅先例 = 设置面 CustomEvent |
| A2 | 按 ID 取块 | 无 `get_block` 单步 API | Rust：`tree.lookup_id(id) → tree.node(nid)` 两步组合（全 pub）；JS：`blocks_json` dump 后自维护 byId 索引（**注意 dump 不含 marks**） |
| A3 | 块级监听 | 事件为事务 op 粒度；InsertBlock 物化带 id、其余六类 op 只带 path | 自建 path→id 换算（Rust `tree.resolve(path)`；JS 自建索引）+ 按块过滤层 |
| A4 | 会话管理 | `SessionManager` 一窗一档上限 16（session.rs:68），`DocumentSession` 类型绑定 MD 核 | **每域一个管理器**模式（类型独立，零改代码）；泛化 = 改核，留裁决 |
| A5 | 外部重载决策链 | `FileStamp` 探针 + `plan_reload` 三态纯函数（external.rs:30-69） | "变更检测+三态决策"可直取纯函数；身份保持采纳（adopt_external）仅域内可用 |

## 3. 跨域不可复用（X1-X2：代码不可移植，模式可移植）

- **X1 op 集 + 求逆矩阵**：`Op` 七变体封闭于 MD 块语义；异构域需自有
  op 集 + 自有求逆。可移植的是**架构模式**：原子 op + state-before 求逆
  + 合并窗 + 事务回滚 + no-op 幂等逆（t3_reuse_locks / r24_quality_fixes
  的断言集即移植验收清单模板）。
- **X2 undo 栈容器**：`History/Entry` 单态绑定 `mdcore::Op`。可移植机制 =
  合并窗 / 边界快照 / 交互史作废（clear_interaction 先例）。
- 现状印证：PDF 面六命令**无 undo 系统**、`Source::Undo/Redo` 在 pdf src
  零消费——两域互不依赖的并存本身就是"跨域不复用 op/undo 代码"的已
  实现先例。

## 4. 质量守护基线（复用后必须保持全绿）

```
cargo test --workspace                     # 默认面（当前 267）
cargo test -p mdc-pdf --features engine-lopdf          # 79
cargo test -p mdc-pdf --features engine-pdfium         # 81 + 6 ignored
cargo test -p mdc-pdf --features engine-pdfium,engine-lopdf  # 109 + 6i
cargo test -p mdc-pdf --features engine-pdfium,engine-lopdf --test pdfc_perf -- --ignored  # 6/6
bash compliance/scan.sh                    # 0 hits
cargo check --workspace --target wasm32-unknown-unknown
# t21 起收严：wasm32 测试面（--all-targets）进门禁——t20-review 登记
# 观察项（write_roundtrip streaming 测试未同步 cfg → E0599）已修：
# 测试面 cfg 化（wasm32 结构性缺席，native 全跑）；此后 wasm32 结构性
# 缺席面（如 add_sheet_streaming，t19 tempfile 背书）API 与测试 cfg 须同步
cargo check --workspace --target wasm32-unknown-unknown --all-targets
cargo clippy --workspace --target wasm32-unknown-unknown --all-targets
# clippy 四面 --all-targets（gates.md PDF-e 起口径；t8 曾收窄为 --workspace
# 漏检测试面 1 条，已回退修复）——2 条 wasm 在档基线，四面零新增
cargo clippy --workspace --all-targets
cargo clippy -p mdc-pdf --features engine-lopdf --all-targets
cargo clippy -p mdc-pdf --features engine-pdfium --all-targets
cargo clippy -p mdc-pdf --features engine-pdfium,engine-lopdf --all-targets
cargo fmt --all -- --check
# web 面：cd web && python3 tests/contract_gate.py（18）+ web_smoke.py（15）+ ime_cdp.py（28）
```

PDF 布局/分页/格式保真由 mdc-pdf 自有代码 + pdfium（固定版式引擎）决定；
**复用 MD 模块不在任何 PDF 质量守护测试的被测对象里**（审计 P6 Q1-Q8
逐项锚点）。唯三文件级耦合点：①`Source` 五值语义镜像（双向锁
`source_mirror_bidirectional_exact_count` 把守，加值须双侧同步）；
②`mdcore://` scheme 命名；③workspace 版本单源。

## 5. 消费契约注记（已知边界，复用方须知）

1. **revision 非全局唯一序号**：合并窗内 coalesced 事务与组首共享同一
   revision（t3_reuse_locks 锁）；open/replay 直接覆写 revision——跨核/
   跨档不可比较，只能取"单核会话内单调"语义。
2. **Block ID 条件保真**：snapshot / replay 路径全保真；**save→open 重解析
   按先序重编号**（结构性编辑后必漂移，t3_reuse_locks 现状锁）；重开
   undo 物化 ID 可能与重解析存活块撞号（现状 = 树内重复 BlockId，注册表
   覆盖无守卫——消费方不得依赖重开后的 ID 唯一性）。跨档块引用需
   ID 持久化进 meta（未来裁决项）。
3. **时钟注入必须**：wasm32 无 SystemTime——`set_clock_ms`（宿主注入）；
   合并窗与 at_ms 全依赖此时钟。
4. **Source 封闭枚举**：五值（view/undo/redo/ai/system）；wasm 面非法串
   typed reject（T3 轮修复，此前静默兜底 View）；加值 = serde 破坏兼容
   + 双侧镜像锁红。
5. **单写者模型**：全部变异面 `&mut self`；跨线程共享需消费方自带同步。
6. **事件面无背压**：VecDeque 无界，`events_pending` 仅可观测——轮询泵
   必须及时排空。
