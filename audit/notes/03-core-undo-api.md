# mdcore 命令/事务/撤销体系与 API 质量深度审计（03-core-undo-api）

- 审计对象：`mdcore-audit` main@96596d7，`crates/mdcore/src`（op.rs / history.rs / document.rs / revision.rs / snapshot.rs / lib.rs / tree.rs / external.rs / anchor.rs / id.rs）
- 测试对照：r12_ai_loop、r11_stress、t1_quality_fixes、r24_quality_fixes、t3_reuse_locks、tr1_line_endings、spike_semantics、r14_gates、document_progressive、sync_resync
- 文档对照：docs/api-reference.md、gates.md（R21/ADR-B4、R24-a2、R11）、REUSE.md §5、README
- 方法：只读静态走查 + 沿调用链手工求值（用户操作 → Document 公开方法 → Op 构造 → invert/apply → 状态/事件/日志 → undo/redo）。未运行 cargo。
- 置信级别标注：【已确认】= 代码逐行核实或测试锁定；【高度怀疑】= 推理充分但无运行验证；【需外部验证】= 依赖宿主/跨 crate 行为；【设计偏好】= 非缺陷但值得裁决。

---

## 0. 调用链与总体结论

调用链实测形状（lib.rs:283 `apply_ops` 为唯一事务入口）：

```
用户/AI → 公开命令面（insert_block/delete_block/...带 gate；replace_text/set_mark 无 gate；
          commit_transaction_as 带 gate+source；wasm apply_ops(opsJson, source) 直达）
        → Op 构造（命令层展开，如 split = ReplaceText+InsertBlock，lib.rs:538-553）
        → Core::apply_ops：backup=tree.clone() → 逐 op「invert(基于 state-before) + apply」
          → 任一失败：tree=backup 整体回滚，revision/history/log/events 零触碰
        → System 分支（不入栈）/ coalesce（1s 双端 View）/ push_entry（清 redo）
        → bump_revision → log.record → push_event → dirty=true
undo/redo → pop 栈 → apply_raw_batch（backup 回滚）→ 失败则条目回栈+边界拨回（T1-F10）
```

**总体结论**：事务原子性、三源（View/Ai/System）语义、合并窗、revision/log/事件一致性、失败路径完备性均实现正确且测试锁定扎实，是本 crate 最可信的部分。但存在两条 **P1 级静默数据保真缺陷**（结构操作全链路丢行内 marks；save→open 后 undo 物化 ID 撞号致锚点错块），以及一批 API 质量问题（Option 吞错、无守卫的 ID 注入面、String 错误）。gates.md R24 审计轮结论「lib.rs 事务管线/回滚/ADR 引用完备」在控制流层面成立，但漏掉了 marks 保真与 ID 注册表守卫两个数据完整性面。

---

## 1.【P1】结构操作全链路丢失行内 marks（delete→undo / merge / split）

**结论**：`RemoveBlock` 的逆（先序重建子树）**不恢复 marks**；前向 `merge_block`/`split_block` 也单向丢 marks。违反模块自我承诺的「undo 精确恢复前态」不变量（op.rs:5「保证每个原子 op 都可精确求逆」）。

**证据**：
- `Op::InsertBlock` 没有 marks 字段（op.rs:25-36，七个原子 op 中唯一携带富文本状态的插入通道只传 kind/attrs/text/id）；
- `build_rebuild_ops`（op.rs:346-364）只发射 kind/attrs/text/id，`NodeData.marks`（tree.rs:88-95，`collect_subtree` 在 tree.rs:252-266 已完整收集）被静默丢弃；
- external.rs:174-175 注释自认：「marks 不入 InsertBlock——外部文件本无 marks，**与 undo 复原路径的 v0 口径一致**」——即开发方知道 undo 复原路径丢 marks，但该边界**未登记** gates.md / README / REUSE.md 任何一处（spike_semantics.rs:10-11 只登记了「编辑区间部分重叠截断」这一更窄的有损面）；
- 前向 merge：lib.rs:581-590 的 op 组 = `ReplaceText(prev 尾接) + RemoveBlock`，右块的 marks 既不随文本迁移也不保留；split：lib.rs:538-553 的 `ReplaceText(截尾) + InsertBlock(tail_text)`，尾区 marks 被 `adjust_marks` 截掉且新块零 marks（op.rs:196-239 的第 4/5 分支）。

**可复现构型**（纯推理，未运行）：`set_mark(bold) → delete_block → undo`：文本回来、加粗消失；`split_block`：光标后已加粗文本瞬间裸化。

**测试覆盖评估**：**零覆盖**。r24_quality_fixes 只测 SetMark 自身的求逆；spike_e_marks_edit_semantics 测 adjust_marks 三情形但止步于截断（未 undo 截断删除）；无任何「delete→undo 恢复 marks」或「merge/split marks 迁移」断言。

**风险**：会话内静默格式丢失（用户删块再撤销 = 加粗/斜体全丢）；AI 源（html.rs inline 解析产 marks，r13 链路）导入的富文本经一次结构编辑即降级。因 marks 不写入 markdown（markdown.rs:8），损失被 save 行为掩盖，用户可能在导出 HTML（html.rs 渲染 marks）时才察觉。分级 **P1**，置信【已确认】（代码逐行核实，逻辑无歧义）。

**修复方向**：(a) `InsertBlock` 增加 `#[serde(default)] marks: Vec<MarkSpan>`（serde 向后兼容），`build_rebuild_ops`/`emit_rebuild`/`push_subtree_ops` 透传；(b) merge/split 在命令层补 SetMark op 迁移区间。

---

## 2.【P1】save→open 后 undo 物化 ID 与重解析存活块撞号（注册表覆盖无守卫）

**结论**：`open_document` 按文档先序重解析重编号（markdown 不含 ID），而 meta 里恢复的历史栈条目携带**旧物化 ID**；undo 触发重插时 `register()` 静默覆盖 `id_index`，树内出现**重复 BlockId**，先注册块从 ID 寻址面失联；持有该 BlockId 的锚点（AI/宿主）随后**静默编辑错误块**。

**证据**：
- tree.rs:154-156 `register` = `id_index.insert(id, slot)`，无重复检测；
- document.rs:220-263 `save_document`（md 不含 ID + meta 嵌整栈历史）/ `open_document`（from_markdown 重解析 + set_history_json）；
- t3_reuse_locks.rs:145-186 `save_open_undo_rematerialization_id_collision_boundary` **以现状锁固化**：重开后 undo → `preorder_ids == [1,2,3,3,4]`，`lookup_id(3)` 指向最后注册者；
- REUSE.md §5-2 已把「消费方不得依赖重开后的 ID 唯一性」登记为消费契约，但树内不变量（§8.2 身份唯一）实际被破坏，且无守卫、无修复路径、无产品裁决跟进（t3 注释：「未来若刻意改变该行为……请同步更新」，G4a 注明「ID 持久化进 meta（未来裁决项）」）。

**测试覆盖评估**：覆盖良好（G4a 漂移 + G4b 撞号双锁）——但锁的是缺陷现状，不是正确语义。

**风险**：锚点错块是**静默数据损坏**（replace_text_at_anchor 命中错块，无任何报错）；碰撞链在正常操作序列（编辑→保存→重开→undo）即可触发，无需敌意输入。分级 **P1**，置信【已确认】（测试在库、行为已锁）。

**修复方向**：(a) `register` 撞号守卫（debug_assert 或 typed reject）；(b) `save_document` 把 ID 映射持久化进 meta；(c) `open_document` 重开时对历史物化 ID 做重映射。三者取一即可消除 P1，(a) 最小。

---

## 3.【P2】`undo()/redo() -> Option<Receipt>` 吞掉失败语义

**结论**：空栈与「历史与树不一致导致 apply 失败」返回同一个 `None`，调用方无法区分「无可撤销」与「撤销失败」。

**证据**：lib.rs:905-935（undo）、937-962（redo）——失败路径（lib.rs:909-918）`return None`；成功路径 `Some(Receipt)`。t1_quality_fixes.rs:393/439 将失败断言为 `is_none()`，即行为已被契约化。wasm 面 `undo()` 同样折叠为 `Option<string>`（mdcore-wasm/src/lib.rs:101-105）。gates.md R21 段记载宿主侧判据只能绕道「revision 不前进 + 无 source=undo 事件」。

**测试覆盖**：失败路径行为本身锁定良好（F10 三断言：条目回栈/边界拨回/History JSON 逐字节一致）；缺的是**可观测错误面**。

**风险**：用户 Ctrl+Z 无反应且无诊断信息；宿主无法向用户报告「历史损坏」。分级 **P2**，置信【已确认】。

**修复方向**：`Result<Receipt, UndoError>`（`Empty` / `ApplyFailed(Reject)`），wasm 面映射为既有 `{ok:false,...}` 形状；旧语义可用 `Ok(None)` 兼容。

---

## 4.【P2】公开面可注入物化 `InsertBlock.id`，无撞号守卫

**结论**：`Op::InsertBlock.id` 的 `Some(x)` 分支（op.rs:115-119 → tree.rs:178-183 `create_node(Some(x))` → `register` 覆盖）在**公开 API 上无任何守卫**：`commit_transaction` / `apply_ops` / wasm `apply_ops(opsJson)` 都能直接传入与活树冲突的 ID，复现问题 2 的注册表覆盖，无需 save→open。

**证据**：op.rs:33 注释把 `Some` 定义为「undo/redo/replay 物化」专用，但类型系统与运行时都不强制；serde `#[serde(default)]` 使历史 JSON/wasm JSON 面天然可达。问题 2 的 G4b 路径只是该无守卫面的一个自然触发例。

**测试覆盖**：无（t3 G4b 走的是 save→open 链，未测直接注入）。

**风险**：宿主/AI 侧 op 序列化 bug（如复用旧计划重放）会静默打穿 ID 寻址。分级 **P2**，置信【已确认】。

**修复方向**：`apply_op` 对 `Some(id)` 先查 `lookup_id` 冲突即拒（`OpError::DuplicateId`）；与问题 2(a) 同一守卫可双杀。

---

## 5.【P2】SetMark 的 span 代数是「精确匹配」：子区间 remove 静默 no-op、重叠 add 冗余堆积

**结论**：`add=false` 只删除**完全相等**的 span（op.rs:174-176 `retain(|s| s != &span)`），对既有 span 的子区间取消标记是**静默无效果**（但仍产生历史条目、bump revision、发事件——「命令成功但状态没变」）；`add=true` 对已有 span 的子/交集区间追加**重叠冗余 span**（op.rs:170-173 `contains` 精确匹配）。

**证据**：op.rs:153-177；spike_e_marks_edit_semantics 只测整区间加/删；r24 锁定 no-op 求逆（该部分正确，见 §9）。

**风险**：真实编辑器语义（选中粗体中段按取消粗体 → 应拆分 span）在此实现为静默失败；冗余重叠 span 累积后导出面（html.rs 折叠 marks 的 `fold_inline`，html.rs:889-921）行为未定义验证。分级 **P2**，置信【已确认】（语义面）；实际产品影响取决于 web 工具条是否已接 SetMark——【需外部验证】。

**修复方向**：remove 走区间减法（拆分 span），add 走区间并/去重；或至少对「remove 不存在的精确 span 但与现存 span 相交」返回 typed reject 而非静默成功。

---

## 6.【P2】wasm32 默认时钟恒 0：漏注入 `set_clock_ms` → undo 粒度坍塌为「一次 undo 撤销全部输入」

**结论**：`Core::new()`/`from_markdown` 在 wasm32 上 `system_ms()==0`（lib.rs:127-131）；`WasmCore::new` 不注入时钟（mdcore-wasm/src/lib.rs:54-60）。若宿主漏调 `set_clock_ms`，`should_coalesce` 的 `now - last_at = 0 < 1000` 恒真 → **整个会话所有 View 文本操作并成一条历史条目**。

**证据**：history.rs:91-98（saturating_sub 恒 0）；REUSE.md §5-3 已登记「时钟注入必须」；web/src/main.ts:790/1157/1590/1685 四处注入（实际宿主守约）；`set_clock_ms` 是**定值闭包**（mdcore-wasm/src/lib.rs:207-209），宿主须在**每次变异前**重注入——「必须按特殊顺序调用才正确」的 API 形态。

**测试覆盖**：wasm e2e 全部显式注入时钟（web/tests/web_smoke.py:249 等）；**无「漏注入」负路径测试**。

**风险**：新宿主接入（或某个新命令路径漏一处注入）即触发，且症状（undo 行为怪异）远离成因（时钟），极难排查。分级 **P2**，置信【已确认】（机制），触发条件【需外部验证】（取决于各宿主纪律）。

**修复方向**：wasm 构造器里注入 `js_sys::Date::now()` 兜底，或 `Performance::now`；或 coalesce 判据加「时钟恒 0 → 拒绝合并」防御。

---

## 7.【P3】API 质量清单（错误面 / 封装 / 契约）

1. **String 错误**：`Core::replay_jsonl -> Result<Self, String>`（lib.rs:996）；`From<OpError> for Reject` 把结构化错误降级为 `Invalid(format!("{e:?}"))` Debug 字符串（lib.rs:111-115）——`Tree(BadPath)` 类信息可读但不可编程匹配。`Reject`/`OpError`/`TreeError`/`AnchorError` 均无 `Display`/`std::error::Error`（对照 snapshot.rs:82-112 的 `SnapshotError` 与 document.rs:110-116 的 `DocumentIdError` 已做对）。置信【已确认】。
2. **`pub tree / pub history / pub log` 公有字段**（lib.rs:138-140）：与 lib.rs 模块头「apply_ops（唯一变异入口）」的承诺直接矛盾；宿主可 `core.log.records.push(...)` 或绕过 apply_ops 直改树，破坏重放/撤销一致性。api-reference.md §4 甚至把「`core.tree` 块树直访」列为特性。属封装取舍，但「唯一入口」承诺应改口或收口（`pub(crate)` + 显式检视面）。置信【已确认】（设计取舍类）。
3. **`Entry.selection` 写而不读**（lib.rs:354 写入；history.rs:47 公有+序列化；全 crate 无消费点）：undo 不恢复选区是 G2 裁决（View 域，t3 注释），但字段存在且随历史 JSON 持久化，误导复用方以为核会恢复选区。置信【已确认】。
4. **根路径 op 无守卫**：`ReplaceText/SetAttrs/SetMark/SetBlockType` 在 `path=[]`（虚根）上静默成功（tree.rs:202-207 空路径=根；op.rs 只拦 `kind==Document` 的 Insert/SetBlockType）——写入根的隐形 text/attrs/marks、可改根 kind；序列化/`blocks_json`/`dump_tree` 均不可见（markdown.rs:32-38 只走 top_level），快照却持久化它们。同类：对**有子块的 Blockquote 容器**做 ReplaceText 也是隐形写（markdown.rs:90-97 有子块时忽略 text）。可达面 = wasm apply_ops JSON / commit_transaction。影响低但违反最小意外原则。置信【已确认】。
5. **`SetBlockType` 无 kind×children 相容校验**：Blockquote（含子块）→ Paragraph 后子块从 markdown 序列化面消失（render 不递归非容器 kind 的 children）；Heading↔ListItem 等转换不补 attrs（level/marker/depth）。置信【已确认】。
6. **`MoveBlock.to_index` 摘除后语义 sharp edge**（op.rs:8-9 有文档）：同父「移到末尾」须传 `len-1` 而非 `len`（传 `len` → `BadIndex` 拒绝，tree.rs:235-237）；跨父则是 `len`。同一名词两种边界，命令层调用易错。置信【已确认】。
7. **save/open 往返语义不一致**：`save_document` 的 md 不含 marks（markdown.rs:8），meta 历史却含 SetMark op → 重开后 mark 相关 undo 全成 no-op（逆的 exists 判定恒 false）、格式状态静默清零。与问题 1 同根但独立可观测。置信【已确认】。
8. **api-reference.md 过时**：口径 main@f4328eb（早于本审计 96596d7）；§4「mdcore 核心面」只列 4 个 API，`apply_ops`/`commit_transaction_as`/`Reject`/`ApplyMeta`/`set_history_json`/undo 语义（G6 revision 非唯一序号等消费陷阱）均未入册——「新增/变更公开 API 的 PR 须同步本文」的维护纪律未执行。置信【已确认】。
9. **`sync_from_markdown` 记为 View 源**（lib.rs:795-811 + gates.md R21 边界规则「source = 谁驱动了内容变化」）：全量差分作为一条 View 条目进 undo 栈、无 gate。ADR 已裁决、有依据，但意味着一次 resync 的 undo 粒度 = 整个文档差分。【设计偏好】。
10. **ByPath 文本命令零防护**（anchor.rs:32 注释自认「View 保证 path 新鲜」；lib.rs:728-735 无 expect 无 gate）：View 持过期 path（AI 结构编辑插入后）会静默改错块。wasm 面主通道 `apply_ops` 亦无 gate。属 v0 已知取舍。【设计偏好】+【需外部验证】（web 桥事件驱动重建是否完全杜绝过期 path）。

---

## 8. 事务原子性核查（结论：正确，置信【已确认】）

- `apply_ops`（lib.rs:283-376）：`backup = self.tree.clone()`（lib.rs:297）先于任何变异；`invert_op` 失败（lib.rs:301-307）与 `apply_op` 失败（lib.rs:308-311）都整体 `self.tree = backup`；此时尚未触碰 revision/history/log/events/dirty——失败 = 调用前状态完全一致。**部分变更不可能外泄**。
- 失败面覆盖：非法 op（BadPath/BadRange/BadIndex/Cycle/RootForbidden/InvalidKind/IncompleteChunk）全部在 op 层或入口被 typed 拒；UTF-8 边界不构成风险（offset 全部为 char 域，ropey 按 char 索引，`start>end||end>len_chars` 双查，op.rs:135-136/162-163）。
- `apply_raw_batch`（undo/redo 路径，lib.rs:379-388）同款 backup 回滚；T1-F10 补齐了栈/边界回拨（lib.rs:909-918/941-947 + history.rs:154-158），`History` 整栈 JSON 与调用前逐字节一致（t1_quality_fixes.rs:411-414）。
- 测试锁定：r12 `ai_batch_is_atomic_on_bad_op`（尾部坏 op 拖垮整批、零污染零事件）、r11 `bulk_transactions_are_atomic_and_undoable_at_scale`（20k 块 49+1 坏 op 全回滚）、spike_e stale reject 零副作用。**覆盖优秀**。
- 微小注：`apply_ops` 对空 ops 早退返回当前 revision（lib.rs:284-288），与「空差分不 bump」文档一致。

## 9. 逐 op 求逆正确性矩阵（R24-a2 修复完备性专项）

| Op | 逆构造 | 核验结论 |
|---|---|---|
| InsertBlock | RemoveBlock{join(parent,index)}（op.rs:245-247） | 正确（含 ID 物化回写 op.rs:119，entry.ops 携带 Some(id)，redo 复原身份——r12:520 实测） |
| RemoveBlock | 先序重建（op.rs:248-256 + 346-364） | **结构/身份/attrs 正确，marks 丢失**（问题 1） |
| ReplaceText | 区间逆（op.rs:257-274） | 正确；marks 部分重叠截断有损（已登记语义边界，spike 头注） |
| SetBlockType | 前态 kind 回读（op.rs:276-282） | 正确（同 kind no-op 逆亦 no-op，对称） |
| SetAttrs | 前态 attrs 回读（op.rs:283-289） | 正确（同 attrs 同理）；**无 undo 专属测试**（见 §11 缺口） |
| SetMark | **R24-a2 状态决定方向**（op.rs:290-324） | 正确且**完备**：`inv_add = if exists == add { add } else { !add }` 穷尽 (exists, add) 四组合——no-op→同向幂等、有效变更→反向。非仅预设输入：子区间 add（exists=false 但重叠）逆为 remove，精确消除新加 span（推理核验）。r24_quality_fixes 四锁 + 区间校验加固（op.rs:162-163） |
| MoveBlock | (from=join(to_parent,to_index), to_parent=原父, to_index=原下标)（op.rs:325-342） | 手工穷举四情形（同父上移/下移/跨父双向/原位 no-op）均正确；环与根守卫（tree.rs:296-309 + op.rs:330-333 R11 修复）在位 |

**同类不对称残留扫描**（任务专项）：数值/结构面无残留——ReplaceText 幂等替换、SetBlockType 同 kind、SetAttrs 同值、MoveBlock 原位均为对称 no-op；SetMark 的不对称已被 R24-a2 根治。**残留的同类问题只在 marks 维度**：问题 1（重建丢 marks）与问题 5（span 精确匹配代数）——即 R24-a2 修的是「求逆方向」，没修「求逆的信息完备性」。

## 10. System/AI/Human 三源与合并窗（结论：与 ADR-B4/R12 一致，置信【已确认】）

- **AI 同路径**：`commit_transaction_as`（lib.rs:640-648）= gate + apply_ops，无旁路；AI 批 = 单条目、整批 undo、不吞 Human（r12:465-522/694-780 实测五连 undo 边界正确）；AI 永不 coalesce（source 双端 View 约束，history.rs:91-98）；stale 硬门禁零污染（r12:563-640）。AI 多步撤销粒度 = 每次 apply_ops 一条目（宿主组合责任，r12 注释明确）。
- **System 绕过撤销栈**：lib.rs:316-331 专分支——bump revision + log.record + push_event + note_boundary，唯独跳过 push_entry/coalesce；`extend_from_markdown`（lib.rs:879-899，含 IncompleteChunk 零污染前置防线 op 入口前拒绝）与 `adopt_external`（external.rs:101-171，成功后 `clear_interaction` 清栈+重置合并窗，ADR-B7 §4）实现与文档一致。边界破组（System 后首个 View 击键必开新条目）由 last_source=System 保证（lib.rs 单测 1083-1158 + document_progressive.rs）。
- **合并窗**：双端 View + Kind::Text + 1s + redo 空（lib.rs:333-334 补 redo 闸）+ undo 栈非空；`coalesce_into_last` inverses 前插保持逆序不变量（history.rs:119-134）；无内容相邻性/同块约束（跨块击键同窗合并——粒度取舍，v0 无词边界破组，history.rs:10 注明留档）。合并后单次 undo 撤整组语义正确（r24 对照锁）。**G6 消费契约**（coalesced 事件与组首共享 revision，t3:194-218）已锁。
- 边界失败完备性（T1-F10）与 System 边界元数据序列化（history.rs:269-299 单测）均在。

## 11. Revision/事件一致性、重放、并发（结论：可信，置信【已确认】）

- undo/redo 各 bump revision + `log.record(Source::Undo/Redo)`（R11 修正，lib.rs:920-924/949-951）+ 事件携带新 revision——外部观察者（web 桥轮询 take_events）可正确刷新；含 undo/redo 的日志重放 = 实时状态逐字节一致（r12:787-841、r14 gate1）。
- revision 单调性豁口均为**已登记契约**：coalesced 不 bump（G6）；`open_document`/`restore`/`replay_jsonl` 直接覆写 revision（REUSE.md §5-1「跨核不可比较」）。
- 并发/重入：**无 RefCell/BorrowMut**（全仓 grep 零命中）；单写者 `&mut self`（REUSE.md §5-5）；历史 panic 点（MoveBlock 空路径减法下溢）已由 R11 修复并加守卫（op.rs:330-333）；现存 `expect` 均为内部不变量（node slot occupied / path_of root / 序列化不可失败）。r11_stress 20k 块 × 百 op 无 panic。`ClockFn` Send+Sync 但 Core 本身不做线程共享承诺——一致。
- 性能注记：undo 双克隆 `entry.inverses.clone()`（lib.rs:908-909）；`apply_ops` 每事务整树 clone backup（R22-b 已登记为高水位主驱动，gates.md）。

## 12. 测试覆盖矩阵与缺口

| 面 | 覆盖 | 缺口 |
|---|---|---|
| 事务原子性 | r12/r11/t1/spike_e | 无 |
| ReplaceText/SetBlockType/SetMark/Insert/Remove/Move 求逆 | spike_b/spike_d/r24/r11/r12 | SetAttrs undo；MoveBlock 同父上移/下移小例矩阵（r11 只测「首块→尾部」单向 50 次）；**delete→undo marks** |
| System 语义 | lib 单测/document_progressive/gates R21 实测 | 无 |
| AI 闭环 | r12 全套 | 无 |
| 合并窗 | spike_d/t3 G1/G6/r12 | 跨块合并粒度无锁（行为性无断言） |
| 失败路径 | t1 F10 | 无 |
| 快照防线 | t1 O4 | root-path op 无锁 |
| 行尾/解析 | tr1 | 无（与撤销体系正交，实测三入口单点覆盖） |

---

## 13. 行动建议（按优先序）

1. **P1-修复**：`InsertBlock` 增加 marks 字段并贯通三条发射路径（op.rs build_rebuild_ops / external.rs emit_rebuild / lib.rs push_subtree_ops）+ merge/split marks 迁移（问题 1）。
2. **P1-修复**：`register` 撞号守卫（tree.rs:154）——同时消除问题 2 与问题 4 的注入面。
3. **P2**：`undo()/redo()` 改 `Result`（问题 3）；wasm 构造器时钟兜底（问题 6）；SetMark 子区间 remove 的 typed reject（问题 5 最小改）。
4. **P3**：错误类型 Display/Error 化 + replay_jsonl 去 String 化；api-reference.md §4 补命令/事务/撤销面；`Entry.selection` 删除或实现；root-path 与容器 op 守卫。
5. **测试**：补 delete→undo marks、SetAttrs undo、MoveBlock 小例矩阵、漏注入时钟负路径、id=Some 注入拒绝。

## 14. 一句话裁决

**可长期依赖的前提是先修两个 P1**：控制流层（原子性/求逆方向/三源/合并窗/失败回滚）工程质量高、锁测试密度大，可放心依赖；数据保真层（marks、ID 注册表）存在两条已确认的静默丢失路径，在修复前，任何依赖「undo 后格式与锚点精确复原」的宿主行为都建立在未被兑现的承诺上。
