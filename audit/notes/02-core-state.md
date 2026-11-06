# mdcore 文档状态模型深度审计（02-core-state）

- 审计对象：`/home/z/my-project/mdcore-audit`（main@96596d7）`crates/mdcore`
- 审计方式：只读静态审计，沿真实调用链验证，引用 `文件:行号`
- 规模基线（loc-stats.md）：mdcore src 18 文件 / 8786 LOC；tests 19 文件 / 7322 LOC；240 `#[test]`；0 unsafe；依赖仅 ropey / serde / sha2
- 分级：P0=可能数据丢失/核心状态破坏；P1=核心正确性重大问题；P2=明显技术债；P3=建议
- 置信：已确认 / 高度怀疑 / 需外部验证 / 设计偏好

## 总发现清单

| # | 级别 | 发现 | 置信 |
|---|---|---|---|
| F-1 | **P0** | save→open 块身份按位置重编号 + undo 物化 ID 回放撞号 → **活树出现重复 BlockId**（§8.2 唯一性不变量破坏；快照往返被毒化；锚点/协作身份错配） | 已确认（仓内已有现状锁测试） |
| F-2 | **P1** | Anchor 无偏移平移映射（v0 静态偏移）+ `sync_from_markdown` 按位对齐使 **ID 跟位置不跟内容** → 无 `expect` 的 ByAnchor 可静默解析到并改写**错误内容** | 已确认 |
| F-3 | **P1** | 行内格式双轨表示：`node.marks`（op 层存储态）与源文本字面定界符（`**` 等）并存；**marks 不持久化**（save/open 静默丢失）、导出层优先级规则不一致 → 状态漂移与静默格式丢失 | 已确认 |
| F-4 | **P2** | `Core.tree / history / log` 为 `pub` 可变字段，绕过「apply_ops 唯一变异入口」承诺，revision/log/事件可整体失同步 | 已确认（API 形状） |
| F-5 | **P2** | 渐进加载 chunk 完整性只防未闭合 fence；quote 组 / list 组 / table 行跨 chunk 切分**静默**破坏 one-shot 等价性 | 已确认 |
| F-6 | **P2** | `save_document` 不保存 revision log；重开后 log 只含 genesis(rev 0) 而 `revision=N` → 「日志自含 + 重放==实时」承诺跨会话不成立 | 已确认 |
| F-7 | **P2** | UTF-16 换算边界测试缺口：组合字符/字素簇零覆盖、代理对中间位换算语义未锁、桥函数在 crate 外（web/src/main.ts） | 已确认（覆盖缺口） |
| F-8 | P3 | undo 不回放 `Entry.selection`；`Core.selection` 不入 snapshot/meta（存而不用的半状态） | 已确认 |
| F-9 | P3 | mark 区间与编辑部分重叠 → 有损截断且 undo 不复原 span（v0 已登记语义） | 已确认（设计登记） |
| F-10 | P3 | 空操作事务（如空 insert 的 ReplaceText）仍 bump revision + 置脏 + 入历史（等效变换多推进） | 已确认 |
| F-11 | P3 | `FileStamp` 同毫秒 + 同尺寸的外部变更漏检（mtime 毫秒精度残余风险，注释自认） | 已确认（文档自认） |
| F-12 | P3 | `path_of` 对「已注册但未挂载」节点断言 panic（`pub tree` 误用可达）；递归遍历深度无界 | 高度怀疑（需构造验证） |
| F-13 | P3 | fingerprint「同型同文双胞胎」最小序位分配无专属语义锁测试；`replay_jsonl` 无条件信任 log 的 revision 字段 | 已确认（覆盖缺口） |

一句话判断（详见结语）：状态模型骨架（BlockTree + 单一 apply_ops 管线 + 事务备份回滚 + typed reject）质量与测试密度显著高于同类 v0；但**身份层在持久化边界被系统性破坏（F-1），锚点语义只是乐观并发而非稳定寻址（F-2）**——修复这两项与 marks 单一真源（F-3）之前，不宜作为长期产品的身份/协作基座。

---

## Q1 权威状态（source of truth）在哪里？Document/Tree/Ropey 关系？是否存在第二份可变状态？

### 结论

- **唯一语义权威 = `Core.tree: BlockTree`**（lib.rs:137）。`BlockTree` = arena 槽位树（`Vec<Option<Node>>`，NodeId=槽位号永不压缩，tree.rs:98-103）+ `IdAllocator` + `id_index: BTreeMap<BlockId, NodeId>` 注册表（tree.rs:150-160）。
- **Ropey 不是第二真理**：每个 `Node.text` 是 per-block `Rope`（tree.rs:51），整篇 source rope 归 View 层 CM6（tree.rs:7-8，R7 裁决）。Rope 只是文本存储实现，块语义（kind/attrs/marks/父子）全在 Node 上。
- `Document`（DocumentId）只是**文件层身份标签**（UUIDv7，document.rs:39-89），不是状态容器；`Document` BlockKind 是虚根保留（tree.rs:108）。
- `Core` 的伴随状态：`history`（undo/redo 栈，含物化 op/inverses）、`log`（RevisionLog，append-only）、`revision`、`selection`、`events`（轮询取走）、`dirty`（R24-b 粘性脏标记，lib.rs:147-153）。这些是**账本/派生面**，不是内容真源。

### 第二份可变状态与漂移面（逐个核查）

1. **`id_index` ↔ 槽位**：由 register/unregister 维护（tree.rs:154-160）；`remove_subtree` 逐节点注销（tree.rs:275-281）。**无守卫**：`BTreeMap::insert` 静默覆盖 → 重复 BlockId 可进树（见 F-1，P0）。`detach` 不注销（tree.rs:244-249）——卸载节点仍在注册表，`path_of` 会撞 `assert!(cur == self.root)`（tree.rs:220）panic（F-12）。
2. **`node.marks` ↔ 源文本字面定界符**：**真正的双轨可变状态**。inline.rs:5-10 声明「行内格式是消费时派生，不是存储态」，但 `SetMark` op 确实把 `MarkSpan` 写进存储（op.rs:153-177）；docx 导出 `effective_inline` 又规定「node.marks 非空时 op 层覆盖赢（不二次解析）」（mdcore-wasm/src/docx_export.rs:42、1856）。markdown 序列化明确「marks 不写入 MD」（markdown.rs:8）→ **save→open 丢失 marks**（见 F-3，P1）。word_count/pagination 的 `net_text(&text, &node.marks)`（pagination.rs:230-256）也消费该双轨。
3. **`history` inverses ↔ 树**：apply 管线先整树 `backup = self.tree.clone()`，逐 op 求逆+应用，任一失败回滚（lib.rs:296-313）；undo/redo 失败路径有 T1-fix F10 的条目回栈+边界拨回（lib.rs:909-918、940-947，测试 t1_quality_fixes.rs:371-439）。**会话内一致**；跨 save/open 会失配（F-1/F-6）。
4. **`log` ↔ `revision`**：会话内一致（apply/undo/redo 全记）；save/open 后 log 归零而 revision 保留（F-6）。
5. **无缓存类派生态**：`word_count`/`estimate_pages`/`chunk_document`/`blocks_json` 全是按需纯函数（word_count.rs 模块文档「纯检视」、chunk.rs:189-193「不碰 revision/历史/事件」、fingerprint.rs:15「全部 API 为只读检视」；测试 r23_fingerprint.rs:478-495 `matching_is_readonly` 钉死）。**无 memo 化缓存 → 无缓存漂移**，这是本 crate 的显著优点。
6. **`selection`**：`set_selection` 自由写入、无对树的校验（lib.rs:259-261），存进 Entry（lib.rs:354）但 undo 从不回放（F-8）；不随 snapshot/meta 持久化。
7. **`dirty`**：粘性语义清晰（View/Ai/Undo/Redo 置位、System 装载不置位、adopt 成功清位，lib.rs:371-372、933、960、external.rs:163），方向性安全（假阳性取向）。

### 封装缺口（F-4，P2）

lib.rs:9 声明「apply_ops（唯一变异入口）」，但 lib.rs:137-139 `pub tree / pub history / pub log` 三个可变公有字段允许宿主直接改树（或调用 `tree.deep_clone` 留悬挂注册项），完全绕过 revision/log/事件/历史 → **整套账本静默失同步**。crate 内无一处这么做（已核查全部 18 个 src 文件），风险全部来自 crate 外。

### 测试覆盖评估

- 权威面：spike_semantics.rs:107-118（snapshot 往返保 ID）、125-197（§8.2 身份矩阵）、t3_reuse_locks.rs:92-186（save/open 身份漂移现状锁）。
- 只读面：r23_fingerprint.rs:478-495；chunk.rs:600-607（空文档零 chunk）。
- **未覆盖**：`pub tree` 直改后的账本失同步（无从测起，属 API 契约缺口）；marks 跨 save/open 的丢失（无任何测试，见 F-3）。

---

## Q2 Block ID（id.rs）稳定性与身份匹配

### 结论

ID = 单调分配器 `IdAllocator`（id.rs:32-46），**只增不减、永不回收**（delete 即失效，§8.2）；`observe` 抬升水位防重放撞车（id.rs:38-42）。明示否决内容哈希 ID（id.rs:4-5）——内容变化不传播身份。

### 各操作身份行为（沿调用链验证）

| 操作 | 身份行为 | 证据 |
|---|---|---|
| 文本/属性/类型变更 | 保持 | op.rs:127-152 原位改；spike_semantics.rs:125-197 |
| move | 保持（ID 解析到新路径） | tree.rs:290-312（环检查）；spike_semantics.rs:165-174 |
| split | 左保原 ID、右新 ID；code/table/heading 尾降级 paragraph，list/quote 继承 marker/depth | lib.rs:494-555；r17_id_boundaries.rs:244-377 |
| merge | survivor=前一兄弟（ID 保持），被并块失效 | lib.rs:557-591；r17_id_boundaries.rs:288-309 |
| delete / copy | 失效 / 全新 ID | tree.rs:269-286、314-329；spike_semantics.rs:199-231 |
| undo/redo | inverses 物化原 ID 精确复原 | op.rs:346-364（`id: Some(data.id.0)`）；r17_id_boundaries.rs:206-236、r17_utf16_mapping.rs:494-569 |
| `sync_from_markdown`（CM6 桥 fallback） | **按位置朴素对齐：对齐位保 ID、错位槽被原位改写、净变尾部新 ID/删除** | lib.rs:814-859 `diff_forest`；r17_id_boundaries.rs:380-488 |
| `adopt_external` | 三级指纹匹配携带身份（preserved/moved/edited 保 ID；manifest 可信 added 带声明 ID；其余 fresh） | external.rs:101-171；r23_external.rs:116-303 |

### 身份错配路径（核心发现）

1. **sync 的槽位继承（F-2 的一半）**：`diff_forest` 同层按 index 配对（lib.rs:822-848）——中段删除时后续块整体「顶位」，**c 的内容落在 b 的 ID 下**（r17_id_boundaries.rs:419-447，测试注释原文「ID 跟位置不跟内容」）；中段插入时原位槽被改写为新内容（r17_id_boundaries.rs:385-416 注释「按位对齐规则的代价」）。**结构性身份错配是 sync 的既定规则而非 bug**，但对持有 BlockId 的外部观察者（AI 锚点/协作端/证据锚）等于身份偷换。已确认。
2. **save→open 重编号 + undo 撞号（F-1，P0）**：
   - md 不含 ID（markdown.rs:8）；`open_document = from_markdown`（document.rs:253）→ 重解析按文档先序重新分配 1..n。活树 `[1,4,2,3]`（结构性插入物化 ID 4）重开变为 `[1,2,3,4]`——**跨档持有 BlockId 的消费者全部失联**（t3_reuse_locks.rs:92-133 G4a 现状锁）。
   - 更糟：undo 条目的 inverses 携带**旧会话物化 ID**；重开后回放 `InsertBlock(id=Some(3))` 与重解析存活块的 ID 3 撞号——`register` 的 `BTreeMap::insert` 无守卫覆盖（tree.rs:154-156），`apply_op` 对物化 ID 无冲突检查（op.rs:115-121）→ **活树出现两个 BlockId 3**（t3_reuse_locks.rs:145-186 G4b 现状锁，断言 `vec![1,2,3,3,4]`）。后果链（snapshot.rs:57-66 自述）：先注册者从 id_index 失联（anchor 面 Missing 假阴性）、一方 remove 时注销殃及同号另一方、**该状态的 snapshot 可 capture 但 from_json 拒收**（validate 的 DuplicateBlockId，snapshot.rs:148-159）→ 快照持久化被毒化。文本内容本身不丢，但 §8.2 唯一性不变量破坏 = 核心状态破坏级。对比：`snapshot()+set_history_json` 路径无此问题（快照保 ID，r12_ai_loop.rs:520-558 验证）——问题专属于 `save_document/open_document` markdown 路径。
   - 同一无守卫面还允许宿主经公开 `apply_ops`/`commit_transaction` 直接构造 `InsertBlock{id:Some(已存活ID)}` 注入重复 ID（Op 是公有枚举，op.rs:24-66）。
3. **重复内容（两段完全相同文本）**：活动树内无歧义——两段是不同 BlockId，ID 唯一。歧义只在**匹配面**：fingerprint 精确指纹 bucket 配对取 `list.remove(0)`（最小旧序位，fingerprint.rs:552-562），文档声明「同型同文双胞胎按最小序位确定性分配」（fingerprint.rs:18）——确定但**不保真**（双胞胎互换时身份可能交叉）；sync 面按位置对齐，双胞胎天然同位不触发。无 P0/P1 风险，但**无专属锁测**（F-13）。

### 测试覆盖评估

- 优秀面：r17_id_boundaries.rs 全 15 例（表内编辑/嵌套 split/merge/跨块粘贴/undo/redo 身份恒等）；r17_utf16_mapping.rs:494-569（80 次随机编辑后全量 undo/redo 身份恒等）；r11_stress.rs:129-160（20k 块 50 次 MoveBlock + undo + log 重放逐字节复原）。
- 缺口：**reopen→undo 撞号已被 G4b 锁为「现状」而非修复**；双胞胎匹配无锁测；`apply_ops` 手工物化 ID 注入重复无防御无测试。

---

## Q3 Anchor 失效/歧义/静默漂移路径

### 结论

Anchor = `(BlockId, offset)`，offset 单位 = **码点**（anchor.rs:17-21）。v0 明确**没有偏移平移映射**：anchor.rs:11-12「偏移平移映射（周围文本变化自动跟移）属 R9 增强，v0 以 expect 保护」。即 anchor 是**冻结快照坐标**，不是活定位。没有 bias（左右黏附）语义——插入发生在 offset 处时锚点不移动也不分裂。

### 逐路径核查（lib.rs:702-776 `resolve_anchor`/`resolve_text_target`）

| 场景 | 行为 | 评价 |
|---|---|---|
| 块被删 | `lookup_id` miss → `AnchorError::Missing` → reject（绝不静默猜测，§8.4） | ✅ 显式；spike_semantics.rs:666-677 |
| offset 超长（块文本缩短后） | `OutOfRange{len}`（码点口径）→ reject | ✅ 显式；r17_utf16_mapping.rs:399-445 |
| 区间反转 start>end | 入口 `Inverted` 拒绝（T1-fix F2，修复了 ropey slice panic） | ✅；t1_quality_fixes.rs:48-112 |
| `expect` 不匹配 | `ExpectMismatch` → reject | ✅ 乐观并发保护 |
| **相邻/同偏移插入** | 锚点**不动**（静态 offset），插入文本出现在锚点前 → 锚点指向的码点内容已变 | ⚠️ **静默语义漂移**（唯一防线是可选 expect） |
| 块内前部编辑 | 同上，offset 不平移 | ⚠️ 同上 |
| split（锚点在右半） | 右半新 ID → Missing；锚点在左半但 offset>新长 → OutOfRange | ✅ 显式 |
| merge（锚点在被并块） | Missing；锚点在 survivor：offset 落入并入文本区 → **指向新并入内容**（静默） | ⚠️ 漂移 |
| move | ID 解析到新路径 ✅（ADR-B1 的价值时刻，spike_semantics.rs:617-632） | ✅ |
| **sync_from_markdown 后** | ID 跟位置不跟内容（Q2-1）→ 锚点解析到**另一个内容**的块 | ⚠️ **最危险的静默漂移**（F-2，P1）：无 expect 的 ByAnchor 把 AI 文本写进错误内容 |
| 跨块粘贴/commit_source 重建 | 重建块全新 ID → 旧锚点 Missing | ✅ 显式（保守） |

### 风险分级

- **F-2（P1，已确认）**：组合面 = 静态 offset + sync 位对齐 ID 重指派 + expect 可选。触发链：AI 持 `(block=2, offset=5)`（无 expect）→ 用户 CM6 fullResync（中段删一行）→ ID2 槽内容已换 → `replace_text(ByAnchor)` 静默改写错误段落。代码与测试都如实记录该语义（r17_id_boundaries.rs:427-441「c 内容落在 b 的槽 ID 下」），但**没有一个测试演示「锚点跨 sync 后指向错误内容」的端到端后果**。
- chunk locator（chunk.rs:121-127 `Anchor{block, 0}`）同样受 F-1/F-2 影响：重开/重同步后 locator 失联或错指。
- `Core.selection` 是同构静态锚，无任何维护 → F-8（P3）。

### 测试覆盖评估

覆盖好的：Missing/OutOfRange/Inverted/ExpectMismatch 四类 typed reject（spike_semantics.rs:612-678、r17_utf16_mapping.rs:381-446、t1_quality_fixes.rs:48-112）；move 后解析（spike_semantics.rs:617-632）。**无覆盖**：相邻插入的 bias 行为（因为根本没有实现——设计空白而非 bug）；跨 sync 的锚点漂移端到端；merge 后锚点指向并入内容。

---

## Q4 Revision 语义：什么推进、什么不推进，漏推进/多推进

### 结论

revision 是**事务序号**（coalesced 批共享组首号），不是内容哈希或全局唯一事件序号（t3_reuse_locks.rs:188-218 G6 契约锁：合并事务的事件与组首同 revision，消费方不得以 revision 做去重序号）。

### 推进矩阵（沿 apply 管线逐分支验证）

| 路径 | bump? | 证据 |
|---|---|---|
| apply_ops 新条目（View/Ai） | ✅ | lib.rs:350-358 |
| apply_ops 合并窗内 coalesced | ❌（共享组首 revision，log 多记录同号） | lib.rs:333-348；revision.rs:1-5 |
| apply_ops Source::System（extend/adopt） | ✅（log/事件照常，历史栈跳过，ADR-B4） | lib.rs:316-331；document_progressive.rs:151-257 |
| undo / redo | ✅（记 Undo/Redo 源日志，R11 修正） | lib.rs:920-924、949-951 |
| undo/redo **失败** | ❌（条目回栈+边界拨回，树/revision 不动） | lib.rs:909-918、940-947；t1_quality_fixes.rs:371-439 |
| 空 ops / 空差分 sync / Noop adopt | ❌（早退） | lib.rs:284-288、external.rs:109-117；sync_resync.rs:51-54、r23_external.rs:223-232 |
| 匹配/指纹/字数/分页/chunk（只读检视） | ❌ | r23_fingerprint.rs:478-495 |
| set_selection / set_clock / mark_saved | ❌（非文档状态/非变更） | lib.rs:196-231 |

### 漏推进核查

- 会话内未发现绕过 apply 管线的树变更（除 F-4 的 `pub` 字段旁路——那是唯一漏推进通道，P2）。
- `from_markdown` 初始态 rev 0 + genesis 记录（lib.rs:182-191）；`restore`/`open_document`/`replay_jsonl` 显式赋值（lib.rs:985-991、document.rs:257、lib.rs:1022）——恢复路径赋值而非推进，正确。
- 外部观察者拿到陈旧状态的可能面：**F-6**——`open_document` 后 `core.revision = meta.revision`（如 N），但 `core.log` 只有 genesis(0)（save 不保存 log，document.rs:220-230）→ 依赖 log 重放的观察者得到 rev 0 的陈旧树；`replay_jsonl` 无条件信任每条记录的 `revision` 字段（lib.rs:1022-1023），被篡改/乱序的 log 可使 revision 倒退（log 属可信输入，P3）。

### 多推进核查

- **F-10（P3，已确认）**：等效空变换（如 `ReplaceText{start==end, insert:""}`）走完整管线 → bump + 置脏 + 一条历史 + 一条日志。op 层无「无变化短路」（op.rs:127-141 对空区间照常 remove(空)+insert(空)）。
- adopt 的 Noop 早退与匹配只读已正确避免无效推进（r23_external.rs:223-232）。
- undo 置脏的方向性假阳性是有意的安全取向（lib.rs:147-153）。

### 测试覆盖评估

优秀：spike_semantics.rs:364-413（合并窗/语义破组/revision 计数）、document_progressive.rs:151-333（System 边界全套 + mem_stats 零条目）、t3_reuse_locks.rs:194-218（G6 契约）、sync_resync.rs:44-55（幂等不推进）。缺口：空操作事务的多推进无测试无登记；重开后 log/revision 失配无测试（F-6）。

---

## Q5 Rope 码点 / UTF-8 字节 / UTF-16（CM6）三套坐标换算边界

### 结论

**单位宪法**：核侧一切偏移（op.start/end、Anchor.offset、MarkSpan）= **Unicode 码点**（ropey `len_chars`/`slice` 的 char 语义）；CM6/JS = UTF-16 码元；换算桥 = web/src/main.ts 的 `utf16ToCp`/`cpToUtf16`（main.ts:93-114），**在 mdcore crate 之外**。UTF-8 字节只出现在 fingerprint 哈希写入与 chunk 的 byte 区间（全部由 `char_indices` 派生，恒为字符边界）。

### r17_utf16_mapping.rs 实际覆盖（逐测试盘点）

已锁定的核侧码点语义（非 BMP 语料 U+20BB7/U+1F642/ZWJ 家族/U+FE0F）：
1. 基础事实自证（:96-113）；2. parse→serialize 逐字符恒等 + 二次幂等（:119-158）；3. heading/marker 前缀偏移=码点、三套单位数值互异自证（:160-186）；4. ReplaceText 偏移=码点、跨非 BMP 边界删除不产生孤代理、**越界以码点判定**（若按码元判 end=11 反而「合法」——锁死单位不许是码元，:192-249）；5. JSON 边界 op 往返不变（:251-273）；6. split/merge 非 BMP 偏移 + undo/redo 恒等（:275-341）；7. 多行 fence 块内跨行偏移（:343-375）；8. Anchor expect/OutOfRange 码点口径（:381-446）；9. sync 非 BMP ID 保持（:452-480）；10. 60 段×4 字母系×80 次伪随机码点编辑 + 全量 undo/redo 身份恒等（:493-569）。

### 漏掉的 case（F-7，P2）

1. **换算函数本体不在 mdcore 测试内**（桥属 web 层，由 web/tests/web_smoke.py、contract_gate.py 驱动——本审计范围外，需外部验证）。对 `utf16ToCp`（main.ts:93-103）的静态推演：`utf16Pos` 落在代理对**中间位**时（如 "🙂" 的 pos=1），`codePointAt(i)` 在高位代理处返回整字符 → i 跳 2 → 返回 cp=1（=该字符**之后**）——「右倾」语义：确定、不 panic，但**未在任何测试锁死**；`cpToUtf16` 越界截断到串长、负 cpPos 归 0（main.ts:106-114），同样未锁。
2. **组合字符/字素簇零覆盖**：全仓 grep 无 U+0301/combining 测试。核侧码点切分允许把 base+combining mark、ZWJ 家族（7 码点）从中间切开——码点级正确、字素级可能破坏渲染/撤销粒度（设计偏好级问题，但完全无测试无登记）。
3. BMP 外字符**切分点**（split_block 落在增补平面字符上）由码点语义天然安全（split_block 校验 offset ≤ len_chars，lib.rs:506-509），r17 已覆盖切在 ZWJ 家族首码点；但「切在家族中间」无显式断言。
4. Rust 侧无 UTF-16 出口（`encode_utf16` 只在测试断言里出现）——三套体系换算压力实际全部落在 TS 桥，**mdcore 侧测试名为 utf16_mapping 实为「码点语义锁」**，命名与覆盖面有偏差（不影响正确性，影响审计预期）。

### 风险分级

核侧：无发现（码点语义实现干净，ropey 承担边界安全）。桥侧：P2 覆盖缺口（中间位语义、字素簇）+ 需外部验证（web 层测试不在本 crate）。

---

## Q6 External reload：并发策略、未保存改动、指纹失败行为

### 结论（external.rs + fingerprint.rs + 测试链）

**决策面是纯函数三态**（external.rs:61-69）：外部无变化 → `Noop`（即使本地脏——自存回声不扰民，r23_external.rs:85-97）；有变化+本地净 → `SilentAdopt`；有变化+本地脏 → `OfferChoice`（v0 明确**不做区域级安全合并**——核无逐块本地脏标记，「贸然合并等于替用户做决定」，external.rs:58-60）。

**采纳面 = take-external 全量重建**（external.rs:101-171）：
- op 合成 = `RemoveBlock[0]×n`（路径恒 valid）+ 先序重建；preserved/moved/edited 携带旧 ID、manifest 可信 added 带声明 ID、其余 fresh（external.rs:121-150、176-208）。
- System 事务（不入 undo 栈）+ **交互史整体作废**（`clear_interaction`，ADR-B7 §4：被放弃文档态的 undo/redo 对重建树会 BadPath，external.rs:159-160；r23_external.rs:196-219）。
- 原子性：apply_ops 备份回滚（失败时 history 也不清——清空在 apply 成功之后，external.rs:152-163 顺序正确）。
- 成功后 `mark_saved`（缓冲==磁盘）；Noop 早退**不**清脏不推进（r23_external.rs:221-232）。

**并发**：核无并发原语（单线程模型，wasm 边界串行化），文件监视归宿主（external.rs:3-5）。真正的「并发」= 宿主在用户编辑途中调用 adopt——核侧行为是**丢弃本地未保存改动**（take-external 字面语义），由 `plan_reload` 的 OfferChoice UX 挡在前面。若宿主绕过 plan_reload 在 local_dirty 时直接 SilentAdopt → 本地改动静默丢失（宿主侧违约，核无防护参数——adopt_external 不接收 dirty 状态，设计把责任完全交给宿主，P3 记录）。

**指纹匹配失败的行为**：三级策略降级（fingerprint.rs:9-12）——manifest 快路径全有全无核验（:439-485，一位不符整体弃用，r23_fingerprint.rs:276-308）→ 结构锚 front/back 走查（:510-530）→ 精确指纹全局配对捕移动（:543-562）→ 窗内同型相似兜底 0.7 阈值（:564-595，阈值双向锁 r23_fingerprint.rs:311-370）→ 拆/并归组（:597-726）→ 残渣 reported 为 added/removed。失败 ≠ 报错，而是**降级为 remove+add（身份丢失、内容=外部版）**——保守且可观测（stats 分桶）。

**FileStamp 残余风险**（F-11，P3，注释自认）：mtime 毫秒 + size 双同值判 fresh——同毫秒同尺寸的内容变更漏检（external.rs:36-40）。

**与 marks 的交互**：本地 SetMark 格式 + 外部同文本 → `payload_changed` 含 marks 比较（fingerprint.rs:487-492）→ edited → OfferChoice → adopt 重建后 marks 丢失（fingerprint 重建 op 不携带 marks——external.rs:174-175「marks 不入 InsertBlock，与 undo 复原路径 v0 口径一致」）→ **外部重载是 marks 丢失的第二个通道**（与 F-3 同根）。

### 测试覆盖评估（r23_external 14 例 + r23_fingerprint 15 例 + external_corpus）

覆盖优秀：三态决策、四象限身份保持、嵌套子树携带、fresh ID 单调、Noop 幂等、空文档边界、manifest 可信 added、跨运行确定性（r23_external.rs:52-325）；匹配只读、1MB 性能相对判据（r23_fingerprint.rs:402-474）；CommonMark 655 + markdown-it 652 例幂等/保留率基线门（external_corpus.rs:202-315，环境缺件=SKIP 纪律）。缺口：**本地脏 + SilentAdopt 被宿主误用的丢数据路径**无测试（核侧无防御面可言）；同毫秒同尺寸 FileStamp 漏检无测试；marks 跨 adopt 丢失无测试。

---

## Q7 边界条件核查（逐条找测试；找不到则推演）

| 边界 | 测试 | 状态与推演 |
|---|---|---|
| **空文档** | lib.rs:1068-1075（empty_core_invariants）；chunk.rs:600-607；r23_external.rs:236-249（adopt 全删/全增）；sync_resync.rs:274-284（空核 sync 建块再清空 + undo）；n5_pagination.rs:56-59（零块 1 页） | ✅ 充分覆盖 |
| **单字符文档** | 无（grep 无果） | **无测试覆盖**。推演：单字符段落 → replace/split(offset 0/1)/undo 均由码点路径覆盖（r17 的 "𠮷a🙂b" 最短 4 码点；单字符是同一代码路径的退化输入，风险低但 split 后 tail 为空块的行为（空文本 ListItem 序列化）值得一个冒烟） |
| **超长行** | chunk 硬切（chunk.rs:477-496 按 child_max 字符切，char_indices 安全）；r11_stress.rs 20k 块但每行短 | **无单行 MB 级测试**。推演：(a) 解析按行 split 无行长限制，内存 = 行长本身，无放大；(b) **pagination 的 code_lines = 字面行数**（n5_pagination.rs:79-88，"等宽不折行近似"）→ 无换行的 MB 级代码行被记 1 行，页数**严重低估**——模型已知近似，误差带 10% 档吸收不了数量级偏差（P3，设计登记不足）；(c) chunk 硬切保证有界 ✅ |
| **嵌套列表深层** | r17_id_boundaries.rs 3 层（:244-339）；r17_utf16_mapping.rs 3 层 | **3 层即测试上限，无深层/病态深度测试**。推演：深度栈建树无上限（markdown.rs:256-260），序列化/collect_subtree/to_snap/validate_node/walk_entries 全递归**无深度限制** → 病态输入（数千层缩进）理论栈溢出（F-12，高度怀疑，需构造验证）；render 的 list_indent 字符串每层 +2 字节，千层 = O(n²) 字符串复制（性能债） |
| **跨块粘贴含围栏代码** | r17_id_boundaries.rs:206-236（表格+fence 尾贴，undo/redo 身份恒等）；sync_resync.rs:261-270（fence 吞行差分） | ✅ 覆盖 |
| **连续 undo/redo 交叉** | r17_utf16_mapping.rs:294-305（undo→undo→redo→redo）；t3_reuse_locks.rs:59-82（新编辑清 redo 栈 + redo() 空栈 None）；r12_ai_loop.rs:508-525（undo AI 批→redo 全链身份复原）；t1_quality_fixes.rs:371-439（失败路径栈/边界完全复原） | ✅ 基本充分；缺「undo→新编辑→redo 为空→再 undo」更长交叉序列的属性级测试（低风险） |
| **文档关闭重开后 anchor/ID 复原** | **snapshot 路径**：r12_ai_loop.rs:520-558（snapshot+history_json 重开 → undo/redo 全可用）✅；**save_document 路径**：t3_reuse_locks.rs:92-186（G4a/G4b：ID 重编号 + undo 撞号，**现状锁=已知未修复**）❌；anchor 跨 save/open 复原：**无测试**（推演：ID 全部换号 → 一切 anchor Missing；chunk locator 失联） | ❌ save/open 路径即 F-1（P0）本体；snapshot 路径健康 |

补充：渐进打开 + 结构 op 混合的 20k 块路径（r11_stress.rs:128-172 one-shot/渐进收敛）覆盖良好。

---

## Q8 chunk.rs 分块与渐进加载：边界切在多字节字符/行中/块中

### 结论（两条「chunk」要分开）

**A. 检索切块（chunk.rs，只读派生）**：父块=章节内连续段、子块=句组。
- **多字节安全**：`split_units` 用 `char_indices`+`len_utf8` 切字节区间（chunk.rs:464-496），`build_children` 的 `parent.text[byte_start..byte_end]`（:443）恒为字符边界 → **不可能切出半个多字节字符**（Rust 切片保证 + 索引来源保证，双重安全）。CJK 按字硬切 ✅。
- 允许切在：句中（无句界长文按 child_max 硬切）、**字素簇中间**（ZWJ 家族/组合字符可被从中间切开——码点级正确，检索面容忍，模块文档裁决 2 自认「不做缩写/小数防御」）。
- 长度不变量：「父块全部子块拼接 == 父块文本逐字节」有测试钉定（模块文档裁决 2 + chunk.rs 测试区）；内容完备不变量（每非空块恰入一父块）有设计论证。
- 确定性：无 HashMap 迭代序、chunk_id=sha256 内容寻址、fingerprint 与 fingerprint.rs 同域同口径（chunk.rs:60-65、524-536）。

**B. 渐进加载切块（extend_from_markdown + is_complete_chunk）**：
- **只防未闭合 fence**（markdown.rs:20-28：任何 ``` 开头行翻转开合态，与解析器同规）→ `Reject::IncompleteChunk` 在任何 op 构造前拒绝，树零污染（lib.rs:879-884；document_progressive.rs:414-442 四种坏 chunk 全验证）。
- **切在行中**：半行成为独立 Paragraph 块 → 与 one-shot 解析（整行一块）结构不同 → **静默歧义**，无检测无拒绝。
- **切在块组中（F-5，P2）**：quote 组跨 chunk（容器合并语义 markdown.rs:185-210 的栈状态不跨 chunk）→ 两个 Blockquote 容器；list 组中段切（子项失去父栈，`attach_under_stack` 挂根，markdown.rs:244-254）→ 扁平化假深度；table 行跨 chunk → 两个 Table。document_progressive.rs:17-20 测试纪律注释**自认**「中途切 quote 组会破坏 one-shot 等价性」——即防御深度不一致：fence 有核内硬门，quote/list/table 只靠调用方纪律。渐进等价性的唯一保证是 1MB 三重相等测试用的**规整分块器**（section 边界，:107-144）。
- ID 一致性硬保证成立：one-shot 与 K 段 extend 的先序 ID 序列完全一致（两边都是文档序先序分配，lib.rs:869-877 论证 + :107-144 实测 1MB + r11_stress.rs:163-172 20k 块收敛）。

### 测试覆盖评估

chunk.rs 内嵌测试（边界矩阵 + 确定性 + 内容寻址）+ r16_blocks_range.rs（先序区间分片 ≡ 全量、越界截断、渐进增量拼接）覆盖良好。缺口：**mid-line/mid-group chunk 的静默歧义无测试**（F-5）；多字节边界「不可能切坏」由类型系统保证、无显式对抗测试（低风险）；chunk locator 的长期有效性依赖 F-1/F-2 修复。

---

## 结语：能否作为长期产品基础

**骨架可以，身份层不行（当前态）。**

优点是实打实的：单一变异入口 + 事务备份回滚 + typed reject 的管线纪律；0 unsafe、src 内 0 panic 宏；匹配/统计/分页/切块全部只读纯函数（无缓存漂移）；240 个测试且含 1MB/20k 块规模门与外部语料基线门；每条已知边界都有「现状锁」式的诚实登记。

但三个已确认问题挡在「长期产品基础」前面：
1. **F-1（P0）**：`save_document/open_document` 路径系统性破坏 §8.2 身份唯一性（重开重编号 + undo 物化 ID 撞号进活树，快照往返被毒化）——仓内 G4b 已锁现状未修，任何依赖 BlockId 跨会话存活的消费者（协作、证据锚、AI 长任务）都会踩中。
2. **F-2（P1）**：锚点是冻结坐标而非活定位，叠加 sync 的「ID 跟位置不跟内容」，无 expect 的锚点可静默改写错误内容——R9 的偏移平移映射必须落地，或强制 AI 锚点携带 expect。
3. **F-3（P1）**：行内格式双轨（node.marks vs 源文本字面量）无单一真源，save/open 与 adopt 两条通道都静默丢 marks。

修复路径清晰且不动骨架：开档时重建 BlockId↔历史物化 ID 映射或对物化插入做冲突检测（修 F-1）；锚点偏移映射（修 F-2）；marks 并入 markdown 序列化或声明弃用 op 层 marks（修 F-3）。在此之前，mdcore 可作为**单会话、以 markdown 文本为交换格式**的可靠引擎，不能作为跨会话身份/协作的基座。

（审计只读，未修改任何仓库文件；本报告写入 /home/z/my-project/audit-notes/02-core-state.md）
