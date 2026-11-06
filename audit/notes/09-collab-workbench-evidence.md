# 09 · 协作 / 中继 / 工作台 / 证据域深度审计（collab + relay + workbench + evidence）

> 只读审计，基线 main@96596d7（2026-11-04）。证据口径：`文件:行号` 均为 HEAD 实测（Read/Grep/只读 bash）；
> LOC/测试计数引自 `audit-notes/loc-stats.md`（自动生成）+ 本轮 `rg '#\[test\]'` 复核（collab 60、relay 9、
> workbench 31、evidence 53——与 loc-stats 一致）。活跃度 = `git log --since=2026-10-20` 实测：
> workbench 11 > canvas 10 > collab 8 > evidence 4 > mdbridge 2 = relay 2。
> 前置参照：`01-structure.md`（结构审计已判 mdbridge/relay/office/mdshell 为「死 crate 群」——本轮独立复核实测成立，见 §3）。

---

## 0. 总览事实（一分钟版）

- **四 crate 全部零内部依赖入向**（evidence/relay 叶子；collab 仅 yrs+serde_json；workbench→evidence+office-io），
  Cargo 层无环、无反向污染——域独立纪律在编译层全部成立。
- **消费面实测**（与 01-structure §2.1 独立复核一致）：
  - evidence → canvas、workbench、mdcore-wasm（**唯一被三个域消费的契约层，真实产品路径**：web 证据侧栏 evidence_panel.ts → WasmWorkbench）。
  - workbench → mdcore-wasm → web（**有真实产品 UI 表面**）。
  - collab → mdbridge（死）、relay（死）、mdcore-wasm（**活但只喂 harness**：web/src/collab_channel.ts 的探针驱动演示会话，非用户编辑面）。
  - mdbridge / relay / office / mdshell：**零产品消费者**（逐一 rg 复核，见 §3）。
- **质量侧画像**：四 crate 合计 src 9.6K LOC / tests 9.7K LOC / 153 #[test]，测试:源 ≈ 1:1；src 生产代码
  panic 面极小（evidence 125 unwrap **全部**位于 `#[cfg(test)]` 内联测试——实测 `awk` 边界切割为零生产命中，
  修正 01-structure P3-12 的统计级怀疑；collab 生产面仅 3 处构造性安全 expect/panic）。
- **一句话总判**：evidence 是被低估的资产（契约扎实+三域消费）；workbench 是活的独立领域（有产品 UI）；
  collab 是高质量的「期货」（CRDT 引擎完整但产品编辑面零消费）；relay/mdbridge 是等真实宿主的死 crate。

---

## 1. 四 crate 分节裁决

### 1.1 crates/collab（mdc-collab，3,234 src / 3,275 tests / 60 test，yrs 0.28）

**裁决：工程质量第一梯队的 CRDT 适配层；但产品消费面 = 零编辑路径（仅跨页演示 harness）。定级：保持资产、冻结扩张。**

- **架构纪律完整落地且可验证**：
  - 公开 API 零 `yrs::` 类型（lib.rs:23-25 声明；实测 session.rs/identity.rs 等公开签名全部为自有类型——撤除 yrs 只拆本 crate 的隔离不变量成立）。
  - `skip_gc: true` 无条件（session.rs:193，SessionOptions 刻意不含该开关 session.rs:105）；client_id 53 位域 fail-closed（session.rs:186-191，yrs release 静默破坏的显式拒收面，hostile.rs:26-47 有锁）。
  - 双 UndoManager 按 origin 分栈（session.rs:200-204、:235-263），**捕获窗硬编码 0**（session.rs:256，p20 上游缺陷缓解——上游窗>0 + 交错 origin = undo 部分应用 + redo panic，登记为不可调参数）。
  - 全入向字节限额（默认 52MiB，session.rs:283-291 check_inbound 前置于一切入向面）+ apply_update/快照/GC 全 catch_unwind panic 隔离（session.rs:429-457、:475-483、:497-502）。
  - 撤销栈软上限 = 超限 clear_all（撤销历史全失的**显式回执** undo_cleared，session.rs:382-397 + command.rs:61-67——诚实登记不静默）。
- **语义差异登记制度是本 crate 最大亮点**：yrs 载体与块模型的三条已知差异（D-1 种类变更=删旧建新搬子树 ops.rs:361-462；D-2 marks=非重叠最小区间归一化重建 ops.rs:502-631；D-3 char↔byte 单点换算 session.rs:791-803）全部在模块文档+实现+测试三层钉定。
- **身份双轨**（lib.rs:34-36）：`block_id` 字符串（`{client_id}-{seq}`，session.rs:295-298）× `CrdtAnchor{client,clock}`（identity.rs:16-21，"证据锚本位币"——结构性编辑后仍可解算，删除后显式 AnchorDangling 不返回邻居 identity.rs:48-64；undo 复活=新 ID 族，旧锚悬空）。
- **敌意面（问题 8 详答见 §2.8）**：hostile.rs 3 用例 + audit.rs 外部 wire 审计 + recovery.rs 畸形输入，参数面/字节面/侧车面全 fail-closed 且断言零状态污染。
- **消费现实**：mdcore-wasm/src/collab.rs:484-663 出口完整（命令/双栈撤销/同步四件套/事件/快照恢复），**但刻意不出口 presence、锚点侧车、maintenance_gc**（collab.rs:28-30 边界登记）——web 侧唯一消费者 collab_channel.ts 只把它当演示/测试文档（§2.1 详证）。mdbridge（唯一桥接 mdcore 的路径）零产品消费者。

### 1.2 crates/relay（mdc-relay，226 src / 598 tests / 9 test）

**裁决：设计干净的中枢状态机（集成项⑥）；零传输层、零持久化、零认证、零消费者。定级：死 crate（等真实宿主），与 01-structure P2-8 一致。**

- 纯状态机自述诚实：lib.rs:20-21「零网络/线程/进程面；from 字段=传输层已认证身份（中继不重签——宿主责任，文档化边界）」。
- 状态机本身完整：Join/Leave/DocUpdate/PresenceUpdate/SyncRequest 五消息（lib.rs:81-92）；权威副本先应用再扇出、失败零扇出 fail-closed（lib.rs:177-186，relay.rs:197-214 有锁）；晚到端走差量不发快照（lib.rs:195-198）；间隔检查点 + 新中继恢复形态（relay.rs:125-159）。
- `SyncReplica` 门面（lib.rs:43-57）以孤儿规则实现域无关——文档域/画布域同构接入有测试证明（relay.rs:106-122，canvas_harness）。
- **测试 harness 的诚实标注**：doc_harness.rs:192-194 `pump() {}` 空实现注释「本 harness 投递即应用——pump 为异步宿主收敛循环的语义锚」——**没有真实的延迟/乱序/丢包注入**；9 个测试全部是即时应用的确定性单线程模拟。
- **latest_snapshot 仅内存缓存**（lib.rs:115），崩溃后需宿主持久化的快照字节才能恢复——relay 自身无任何落盘。
- 网络现实性详答见 §2.4。

### 1.3 crates/workbench（mdc-workbench，2,195 src / 2,845 tests / 31 test）

**裁决：成立的独立领域（非外围功能集合）；依赖方向正确；有真实产品 UI 表面（web 证据侧栏）。定级：继续投入的优先域。**

- **领域模型自成一体**：八步闭环（lib.rs:10-14：导入→登记→关联→计算→报告→失效→审核→导出）+ 三态正交消费 + 审核门槛 + 幂等命令 + 匿名导出——概念内聚，不是 office-io 的外围包装。
- **三条结构性纪律可验证**：
  1. 正文零写入：对 DOCX 只读；干净导出=字节拷贝+双验证（pipeline.rs:1144-1171：check_dest 前置→验证→临时区+rename→审计，拒绝路径零状态变化）；匿名导出复用全门后 anonymize_docx_props + 正文部件逐字节同源验证（pipeline.rs:1216-1227）。
  2. AI 提议/程序验证：数值一律由程序从真实文件提取（pipeline.rs:739-815 propose_binding：正文指纹 fail-closed :753-754、段落存在 :755-759、claimed_value 必须真实出现在段落文本 :760-766、来源指纹 fresh_source :712-736、摘录程序构造 :803）；常量是唯一例外且正当性归人工审核（InputSource::Constant，pipeline.rs:89-92）。
  3. 无任意路径/SQL/Shell：PathPolicy 允许根 + canonicalize 符号链接逃逸防护（security.rs:69-132）+ 条数/字节门（security.rs:28-32）+ 原子发布单点 publish（security.rs:161-166）。
- **审核门（阶段 2）三维 fail-closed**（security.rs:187-213 + pipeline.rs:1081-1127）：操作者前缀允许列表默认 `["human:"]`——**AI 不可改审核态**（架构原则门面化）；失效关联不得过审（默认拒绝）；高值主张达门槛须 calc Passed。会话级策略不入 sidecar（策略属调用方信任面，security.rs:172-174）。
- **命令层**（command.rs:63-125）：幂等键重放（Replayed 不重复执行）+ 乐观版本（Exactly/MustBeNew/Any——冲突拒绝不覆盖）+ 记录规则（Succeeded/Failed 入表，VersionConflict/Rejected 不入——重试不被旧失败毒化，command.rs:111-118）。
- **内存会话（p5-t3）与文件会话共享验证核心**（pipeline.rs:44-57 Io 枚举仅 IO 边界分派）——wasm 零 std::fs 路径成立。
- **依赖判定**：workbench→office-io 合理（读 XLSX/DOCX→指纹/定位器→证据链，ingest.rs:1-5「只做三件事：读文件→确定性语义指纹→提取可定位的值/段落，不验证不判定」——验证归 pipeline 的分层干净）；→evidence 合理（契约消费）。零 mdcore 依赖正确。
- **AI 共用权威状态的落实（问题 6 详答见 §2.6）**：产品内目前是「AI 可用的工作台」而非「AI 在用的工作台」——ai: 操作者只出现在测试（p2-r1 提交记录：command_layer/persistence_recovery 的 Review 命令 ai:glm→human 信封适配）。

### 1.4 crates/evidence（mdc-evidence，3,907 src（内联 53 test）/ 零内部依赖）

**裁决：四 crate 中设计密度最高、消费面最实的契约层；FNV-1a 与 Missing/Ambiguous 两处已知边界需登记。定级：最高优先保护的资产。**

- **三态正交模型**（status.rs:1-12）：LinkageStatus × CalcStatus × ReviewStatus 三维度互不蕴含、无单一 verified 汇总——「任何『全部通过』的汇总判断都必须在调用方显式组合三面」（status.rs:10-12）。注意：**LinkageStatus 实为 4 值**（Valid/Stale/Missing/Ambiguous，status.rs:19-30）——题目所称「三态」是三个正交维度；其中 **Missing/Ambiguous 是死状态**（§2.5 详证：mark_source_unavailable 零消费者）。
- **只降不升铁律**（evidence.rs:151-165 degrade_linkage：Valid 可降级、任何→Valid 拒绝、失效态互转拒绝）+ 显式重验唯一恢复通道（sidecar.rs:475-509 reverify_evidence / :437-471 reverify_claim，均带审计）——sidecar.rs:4-11 模块文档自述为「§四.5 铁律的执行点」并有测试锁（evidence.rs:299-313）。
- **级联诚实性**：linkage 非 Valid 的证据复算直接 Failed(InputUnavailable)（sidecar.rs:294-302）；复算通过后对照 claimed_value → ResultMismatch（sidecar.rs:303-321）。
- **FNV-1a 64 非防篡改**：fingerprint.rs:6-15 诚实登记用途边界（变更检测，非审计防篡改）+ sha256 升级路径（算法前缀辨识，跨算法恒 false 不误判一致）。
- **panic 面修正**：125 unwrap 全在 #[cfg(test)]（本轮 awk 实测：calc.rs 45 处全在 :323 后、sidecar.rs 42 处全在 :652 后、expr.rs 22 处全在 :659 后）——01-structure P3-12 对 evidence 的怀疑**不成立**，生产代码零 unwrap。
- 消费者：canvas（对象指纹/定位器 Locator::CanvasObject）、workbench（全契约）、mdcore-wasm（契约类型直依赖）——**三个域的真实底座**。

---

## 2. 八问详答（均引文件:行号）

### 2.1 【问题1】协作状态与本地权威的关系：平行宇宙，不是镜像；桥存在但无产品消费者

**关系判定：平行（非镜像、非主从）。** mdbridge/src/host.rs:3-7 自述真相源纪律：「CollabSession 是协同文档唯一真相源；shadow BlockTree 是纯投影」——**在 bridge 内部**，collab 赢（CRDT 权威，shadow 投影自愈）。但这只是设计意图：

- **两个独立块模型**：collab::BlockKind 是「协作域自有类型（领域模型独立纪律：不依赖 mdcore crate）」（collab/src/lib.rs:65-69）——与 mdcore tree.rs::BlockKind 名称语义一一对应但**代码零共享**；七操作面同样双份（mdcore op.rs × collab ops.rs，语义对齐、实现独立）。
- **mdbridge 的实际协议**（三段式，translate.rs:1-11 + host.rs:273-290）：
  1. `Plan::make`：预态解析——mdcore NodePath → 协作域 block_id（经 IdResolver：shadow u64 → family 编码或 foreign 反查），并镜像协作域约束预检（容器面/hr 文本面/保留键），失败零副作用；
  2. `apply_op`：shadow BlockTree 原子应用（mdcore 侧 id 分配/回写）；
  3. `into_command`：组 CollabCommand 进会话；**collab 命令失败 → shadow 全量重物化自愈 + 显式错误**（host.rs:284-288）。
  翻译路径之外的态变更（远端同步/undo/redo/发布）= **全量重物化**（host.rs:308-312 apply_remote、:323-393 materialize）——shadow 永远从会话重建，无增量反同步。
- **修改传播与分叉裁决**：单向（op 流 → 命令流 → wire）；wire 回来只经 apply_remote → 重物化。失败/分叉时 **CRDT 会话赢**（host.rs:285 注释「collab 是真相源，shadow 回投会话态」）。快照恢复换 client_id → family id 落 foreign 类（身份换新族，正确性不变——lib.rs:39-40 登记）。
- **publish 的身份语义 = import 重映射**（host.rs:160-175）：源树 u64 与会话 family u64 是两个独立分配器空间，「保身份直传必然撞号」——发布为每块分配新 family u64，映射表即回执 top_ids；「需要保 u64 的活会话交接面留显式 adopt 裁决——当前无消费方，不预制」。
- **产品现实（关键发现）**：上述协议**零产品执行**。mdcore-wasm/Cargo.toml（全文）依赖 mdc-collab（:26）但**不含 mdbridge**；rg `mdbridge|MdCollabHost` 于 mdcore-wasm/src 零命中。web 线的真实形态是：
  - 主编辑文档 = WasmCore（mdcore BlockTree，CM6 桥）；
  - 协作文档 = collab_channel.ts:443-445 `docSession = new ctor(BigInt(docClientId), UNDO_LIMIT)`——**自建独立会话**，与主编辑文档无任何数据通路；其内容只经 window.__collab* 探针驱动（collab_channel.ts:295-413），web/tests 的 stress_web.py/concurrent_web.py/lifecycle_web.py 经探针注入命令。
  - 即：**同一 web 应用内两份可变文档权威并存且互不相通**。它们不会「分叉」（因为从不合流——比分叉更彻底的隔离）。collab 会话不是用户正在编辑的文档的协作副本，而是一份平行的演示/测试文档。
- **结论**：「同一文档两份可变权威」在设计上是 bridge 内 shadow（投影，不可变权威）；在产品上是两个从未连接的权威（编辑器文档 × 协作演示文档）。失败时谁赢的问题只在 bridge 测试里被回答过（collab 赢），从未在产品里发生。

### 2.2 【问题2】身份与版本系统：≥6 套并存，语义冲突已被意识并登记，但维护成本真实存在

实测身份体系清单：

| # | 体系 | 形态 | 稳定性语义 | 位置 |
|---|---|---|---|---|
| 1 | mdcore BlockId | u64 树内单调分配 | 树内稳定；跨端无意义 | mdcore id.rs（host.rs:5 引述） |
| 2 | collab block_id | String `{client}-{seq}` | 会话全局唯一；**SetBlockType/MoveBlock 换新族后靠属性随迁保持语义连续**（ops.rs:361-362） | session.rs:295-298 |
| 3 | collab CrdtAnchor | `{client:u64, clock:u32}` | **跨结构性编辑稳定**（CRDT 首项 ID）；删除后悬空显式报错 | identity.rs:16-21 |
| 4 | mdbridge family/foreign | `md:{client}:{n}` + 双向映射表 | family 确定性编解码；foreign 跨重物化稳定；**publish=重映射** | mdbridge lib.rs:157-171、host.rs:29-31 |
| 5 | evidence 文档身份 | DocId + EngineKind + VersionStamp + ContentFingerprint 四元组 | 逻辑 ID 稳定 + 版本（保存代数）与指纹（内容）分离 | refs.rs:1-8 |
| 6 | evidence 定位器身份 | Locator 七变体（位置+指纹混合） | 位置稳定但内容敏感（§2.5） | locator.rs:80-136 |
| 7 | workbench 会话身份 | body_doc_id + body_version + 来源注册表（会话内存态） | 从 sidecar 单一真源恢复（pipeline.rs:337-392） | pipeline.rs:248-256 |
| 8 | EvidenceRecord.evidence_id | 调用方供串，查重拒建 | 无内建格式约束 | sidecar.rs:179-209 |

**语义冲突点（实测）**：
- **顺序 ID vs CRDT ID 的稳定性鸿沟**是全部复杂度的根源：mdcore u64 / paragraph_index / VersionStamp 均为顺序身份（重排/删插后漂移），CrdtAnchor 是结构稳定身份。mdbridge 用 family 编解码+水位观察（host.rs:433-463 assign_u64：family 复用 observe、foreign 查表、孤儿兜底）+ 同族伪造防御（lib.rs:35-38：wire 上伪造 `md:{本端}:{n}` → observe 抬水位永不复用——bridge_sync.rs:108-132 有锁）弥合鸿沟，代价是 publish=重映射、restore 换端=换族。
- **collab 内部双轨**（block_id 操作寻址 × CrdtAnchor 证据锚）是**有意识的裁决**而非事故（lib.rs:33-36「身份双轨」+ identity.rs:1-9），且锚点侧车（identity.rs:67-154）带 fail-closed 敌意载荷防御——这是六套体系里设计最完整的一套。
- **真正的冲突不在语义而在重复**：BlockKind + 七操作在 mdcore 与 collab 各写一份（语义对齐纪律靠注释与 mdbridge 语义对照表维护，mdbridge lib.rs:42-52）；锚点稳定性能力（CrdtAnchor）与 evidence 定位器（Locator::MarkdownBlock「后续扩展」locator.rs:118-122）**从未接线**——evidence 没有获得 collab 已经建好的最强稳定身份工具。
- **版本系统**：mdcore RevisionLog（revision.rs:5-8，append-only、undo/redo 入日志）与 collab CRDT 向量时钟、workbench ExpectedVersion（command.rs:87-103）、evidence VersionStamp 四套定序/版本机制互不感知——见 §2.3。

### 2.3 【问题3】Revision/操作顺序一致性：CRDT 合并域 + 拒绝域两套答案，无 LWW 毯子；断言强度=收敛性而非序

- **协作 op 定序**：collab 无 revision 号——定序权完全交给 CRDT 因果（state_vector/encode_diff，session.rs:409-419）。**undo 不进 wire**（session.rs:230-234：远端 update 不带 origin → 不进本地撤销栈，「撤销域=本地」B3.1；撤销产物作为普通 update 传播）。mdcore 的 RevisionLog 与 collab **零关联**——bridge 每 op 一命令一栈项，「mdcore 的命令层组合（split/merge 等多 op 序列）在协作域为多栈项，逐 op 撤销」（mdbridge lib.rs:64-65 已知差异登记）。
- **并发修改的裁决规则（实测）**：
  - 文本并发（不相交区间删）：双端合并、双双生效（concurrency.rs:113-141：Alice 删 [0,5) × Bob 删 [5,10) → 双端收敛空串）。
  - 结构并发（同位插入）：**序由 CRDT 决定，测试只断言集合与一致性、不断言具体序**（concurrency.rs:143-172 注释明言）——这是对 CRDT 的正确断言姿态，但也意味着序的确定性未被钉定（换 client_id 序可能变）。
  - **无后写赢毯子**；但存在两个已登记的「终态语义」点：
    1. **SetMark 归一化重建**：ops.rs:502-505 自认「同块并发文本编辑可能被本归一化终态取代——收敛性恒成立[CRDT]，字面叠加语义注册为已知面」——即 **SetMark × ReplaceText 并发时，归一化以命令执行方的读时快照重建 own-text 段，并发对端的字符级编辑存在被覆盖窗口**。concurrency.rs 只测 Replace×Replace 与 Insert×Insert，**无 mark×text 并发测试**（matrix/marks_readback 亦为单会话形状）。见 P1-2。
    2. SetAttrs 终态钦定（ops.rs:464-466「终态替换」）——并发 SetAttrs 后到者属性集胜出（集合级 LWW-by-merge，属性级合并）。
  - **撤销跨端语义**：Bob 只能撤自己的编辑、Alice 的保留；撤销产物同步在对端同样生效（concurrency.rs:224-276 undo_semantics_across_sessions 全链断言）——**这是本测试文件最强的断言**。
- **workbench 层的并发答案 = 拒绝**：ExpectedVersion::Exactly 不符 → VersionConflict（不覆盖，command.rs:89-103）；MustBeNew 违反 → Rejected。绑定/计算前的指纹 fail-closed（BodyStale/SourceStale，pipeline.rs:627-633、:712-736）= 「绑向过期视图一律拒绝」。
- **concurrency.rs 断言强度总评**：对收敛性（双端读模型全等）与撤销归属的断言是强的；对序确定性与 mark×text 交错是空的——与 CRDT 产品语义相称，但留下 P1-2 的未锁面。

### 2.4 【问题4】网络现实性：relay 承载不了真实多人（也无须它承载——没有网络产品面）

- **relay 传输层 = 无**。lib.rs:20-21「本 crate 是纯状态机（入向消息→出向投递表）；零网络/线程/进程面」；全仓 rg `websocket|WebSocket|tokio|axum` 产品代码零命中（仅 spike/probe-gpui 孤儿 Cargo.lock）。**没有 socket、没有 HTTP、没有任何字节离开进程的路径**。
- **真实存在的传输 = web BroadcastChannel**（collab_channel.ts:1-9）：同源浏览器内跨 tab 通道，消息协议 hello/hi/bye/doc_update/canvas_update/sync_req/sync_resp（:11-18）。这是**真实通道**（真跨 tab、真异步），但：同浏览器、同用户、同源——不是「多人」，是「多窗口」。
- **relay 能否承载真实多人**：状态机语义齐备（扇出/差量追平/检查点/恢复/fail-closed），但缺三样硬东西——①传输与认证（from 字段信任宿主，lib.rs:22-23）；②投递可靠性（投递表交给宿主「负责实际送达与重试语义」，lib.rs:94-95——重试/去重/排序全部外推）；③快照持久化（latest_snapshot 内存缓存，lib.rs:115）。**结论：relay 是给未来宿主准备的 226 LOC 协议内核，不是服务端**。
- **断线重连/重复/乱序的测试覆盖分级（实测）**：
  - **recovery.rs（Rust 层）测的是会话级故障注入**：权威快照重建+继续编辑+继续同步（:26-85）、快照与差量互通（:89-116）、维护压缩（:120-160）、**畸形 wire 输入**（:165-213：随机坏字节/截断真 update/坏 state vector → Decode 错误 + 会话零污染 + 后续合法同步照常）。级别 = **解码级 + 崩溃恢复级**，无网络级（无乱序/重复/延迟注入）。
  - **relay.rs（Rust 层）测中枢语义级**：三端扇出收敛、晚到端差量、离开停投、失败零扇出、新中继恢复——但 harness `pump(){}` 空实现、投递即应用（doc_harness.rs:192-194）——**无异步、无丢包**。
  - **web e2e（Python/Playwright 层）补了通道级**：lifecycle_web.py:591-716 多轮 `__collabDisconnect()/__collabReconnect()`（通道关闭/重开 + sync_req catch-up 补差）；stress_web.py:306-312 断连重连下伴跑；concurrent_web.py 双页并发命令。**乱序/重复**的容忍不靠测试而靠 CRDT 结构性（update 幂等/交换律，apply 失败「typed 记账不抛出（CRDT 幂等——重试/快照 catch-up 兜底）」collab_channel.ts:23）。
  - **总评**：断线重连有真实浏览器级三处覆盖；重复/乱序靠数学性质（CRDT 幂等）而非测试证明——这在工程上是可接受的取舍，但「网络级敌意注入（乱序到达序列）」面为零。
- relay 自身活跃度 2 commits（10-22 后停滞）+ 零消费者——与 01-structure P2-8 判定一致，本轮独立复核确认。

### 2.5 【问题5】Evidence Locator 长期稳定性：位置+指纹混合寻址；演化存活率结构性偏低（索引级联误伤）；Missing/Ambiguous 是死状态

- **寻址机制（locator.rs:80-136）= 位置寻址 + 定位时刻内容指纹的混合**——**无内容寻址回退**（不存在按指纹反查）：
  - XlsxCell：sheet + A1 坐标 + value_fingerprint + formula_fingerprint（:86-94，公式缓存值与文本双通道——公式变更即使缓存值相同也失效）；
  - XlsxRegion：闭区间端点序校验 + 区域级指纹（:95-102）；
  - DocxParagraph：paragraph_index（0 起位置）+ text_fingerprint（:105-110）；
  - DocxTableCell / MarkdownBlock（block_id，后续扩展）/ PdfPageText / CanvasObject（object_id + 对象语义指纹，:129-135）。
- **文档演化后的存活率（代码级推演，重点发现）**：
  - **插入/删除/重排 → 级联误伤**：refresh_document 的失效判定是 `paragraph_fps.get(index) != text_fingerprint`、索引越界=已删（sidecar.rs:400-417）。**在段首插入一段 → 后续所有段落的 index 集体右移 → 该正文上全部 DocxParagraph 定位器（index>插入点者）同时 Stale**——即使其文本一字未动（指纹本身仍能唯一标识该段，但系统不按指纹重新寻找）。存活率：仅「原地未动且前方无增删」的段落锚存活。一次段首插入 ≈ 全量证据降级 + 逐条人工 reverify_claim。
  - **XLSX 侧**：坐标本身结构稳定（增删行列不移坐标）→ 指纹不匹配才 Stale——存活率显著高于 DOCX 侧；但行/列插入导致数值位移时坐标指向新内容 → 指纹失配 → Stale（正确降级）。
  - **设计动机是诚实性**（§四.5 铁律：「润色后允许失效——人工重绑定路径在状态机」，locator.rs:104；只降不升 + 显式重验唯一恢复）——**误伤不是 bug 而是禁止静默重绑的代价**；但「按指纹唯一匹配 → 提示重绑候选」的辅助面不存在，高变动文档的用户成本是 O(证据数) 的手工重验。
  - **最强稳定身份工具未被复用**：collab CrdtAnchor 明确宣称「结构性编辑（邻居增删/搬移）后仍可解算到同一块」（identity.rs:5-7）——但 evidence 的 MarkdownBlock 定位器（block_id + text_fingerprint）标注「后续扩展」（locator.rs:118）且从未接到 mdcore 或 collab 锚点。**两套稳定性能力（CrdtAnchor ↔ Locator）互不相识**。
- **Stale/Missing/Ambiguous 三态的定义与传播路径完整性**：
  - 定义完整（status.rs:19-30：Stale=可达但指纹不匹配；Missing=来源缺失/打不开/坐标超界；Ambiguous=解析出多候选需人工消歧）。
  - **传播路径实测**：
    - Valid→Stale：**自动且双面**——来源侧重摄取（sidecar.rs:132-150 apply_linkage_invalidation）+ 正文刷新（:390-432 refresh_document）。✅
    - **Missing/Ambiguous：只有外部驱动接口 `mark_source_unavailable`（sidecar.rs:156-177，注释「调用方观测后显式登记」）——rg 全仓消费者 = 零**（除定义处自身）。workbench 在来源读取失败时返回 XlsxRead 错误而不标 Missing；Ambiguous 对当前定位器种类**结构上不可达**（按 index/sheet+coord 寻址天然唯一；「文本重复出现致路径歧义」是内容寻址才有的问题，而系统不做内容寻址）。→ **四值 LinkageStatus 里两值是死字母**：产品生命周期内只会出现 Valid/Stale。
    - 恢复：reverify_evidence（源侧）/reverify_claim（主张侧）双通道 + 审计（sidecar.rs:437-509）——唯一恢复通道声明与 degrade 单向性测试互证（evidence.rs:299-313）。✅
    - 消费方处理：workbench review gate `linkage != Valid` 拒过审（pipeline.rs:1105-1112，涵盖假想的 Missing/Ambiguous）；recalc 级联 InputUnavailable（sidecar.rs:294-302）；报告三态分列（report.rs）。传播路径对**活状态**完整，对死状态是「设计在、路径空」。

### 2.6 【问题6】Workbench 领域模型判定：成立的独立领域；office-io 依赖合理；「AI 共用权威」落实为门面与信封，产品内无 AI 实跑

- **独立领域判定成立**（非外围功能集合）：自有概念体系（八步闭环/三态消费/审核门槛/幂等命令信封/匿名导出）+ 自有不变量（正文零写入/程序验证/路径允许列表）+ 2.2K src / 2.8K tests 的内聚测试面（eight_step_loop/review_gate/anonymous_export/persistence_recovery/memory_session/command_layer 六文件 31 test 全部围绕领域语义而非 IO 管道）。
- **ingest/pipeline 对 office-io 的依赖合理**：ingest.rs:1-5「只做三件事：读文件→确定性语义指纹→提取可定位的值/段落。**不验证、不判定**——验证归 pipeline（程序验证纪律），判定归 sidecar 状态机」——依赖方向（workbench→office-io）与分层（摄取/验证/判定三层）都干净；匿名导出的 anonymize_docx_props/docx_part_bytes 也是 office-io 的正当消费（pipeline.rs:1216-1227）。无循环、无 mdcore 反向依赖。
- **「AI/人共用权威状态」（r12_ai_loop 在 mdcore 侧已验证）在 workbench 层的落实形态**：
  1. **命令信封单一入口**：WorkbenchCommand 十命令，operator 字符串承载身份（command.rs:25-59）；AI 与人走同一 execute/同一验证。
  2. **提议/验证分离**：AI 只能给映射与公式，「数值一律由程序从真实文件提取」（lib.rs:21-24 纪律 + propose_binding 全链 fail-closed）——模型置信度不构成验证。
  3. **审核门把 AI 挡在审核态外**：默认 reviewer_prefixes=["human:"]（security.rs:196-198），AI 不可改 review——「AI 只提关联与推导」的原则门面化。
  4. **但产品内没有 AI 驱动者**：web evidence_panel.ts 是人工操作面板（导入 docx/xlsx→人工提议/审核）；`ai:` 前缀 operator 只出现在测试信封（p2-r1 提交记录明言测试适配 ai:glm→human）。**workbench 是「AI 可安全接入的工作台」，AI 循环本身尚无产品执行者**——这与 mdcore 侧 r12（Source::Ai 入事件流）互不连通（workbench 零 mdcore 依赖，AI 溯源语义 Source::Ai 不进证据链；generation_batch 字段是 AI 批次的最弱追踪面，evidence.rs:109-110）。
- **与协作域的组合形态**：collab_composition.rs（integ-r4，dev-dep mdbridge/mdcore/collab）证明「协同文档作为受证正文」全链可行（AI 润色受证段→恰该主张 Stale→人工重绑恢复 Valid→撤销→再失效→再重绑）——**但这只活在测试里**：workbench 产品依赖面（Cargo.toml [dependencies]）只有 evidence+office-io，桥接组合零产品路径。

### 2.7 【问题7】「不要为提前协作引入无需求复杂抽象」检验：yrs 用法本身物有所值，但协作整域相对产品现实是超期投资

- **两处 yrs 用法对比（均物有所值的局部判定）**：
  - collab：XmlFragment 树载体 + 七操作翻译 + D-1/D-2/D-3 差异登记 + 双栈撤销 + 审计/修复面——**真实 CRDT 语义负载**（收敛/撤销归属/断连恢复），不是玩具封装；
  - canvas：Map 载体 + DeepObservable 事件（canvas/src/events.rs:28-29）+ 同一命令模式——对象图 CRDT，与文档树用法形状不同但纪律同构（skip_gc/捕获窗 0/限额三件套同款，session.rs:120-124 时钟注入「与 mdc-canvas SessionOptions::clock_ms 同款纪律」）。
  - 两处各自解决各自载体的真实收敛问题，无互相抄错；Cargo.toml 注释「mdc-collab 是产品面唯一 yrs 接触点」的声明与 canvas 并存说明该注释已过时（canvas 也直接用 yrs——01-structure 已记 dev 边 canvas⇢collab 为测试面）。**修正：yrs 接触点 = 2（collab/canvas），隔离不变量各自成立**。
- **「协作从未有真实第二用户」检验（实测成立）**：
  - 产品内唯一协作形态 = BroadcastChannel 同源跨 tab（同一用户的同一浏览器，collab_channel.ts:6-7「同源浏览器内真实多 tab 通道」）；
  - GPUI 主线（mang-shell，实际产品 GUI）对 mdc-collab **零依赖**（01-structure §1.3 复核成立）；
  - 无任何网络部署面（§2.4）；e2e「双端」= 同一 Playwright 实例开两个 page。
- **对照 cleanroom 决策**：boundary-audit（mang-research-archive，基线 03815b6）P4 §3 判「六项协同能力缺失（多端同步/OT/CRDT/广播/离线/自动冲突）」→ collab/canvas/relay 家族是该结论的响应（01-structure §5 表末行：「部分被 mdc-collab/mdc-canvas/mdc-relay 覆盖，但这些在 web 线仅经 mdcore-wasm 出口、GPUI 线完全未接」）。**cleanroom 要的是能力补齐，产品现实没要使用**——投资与消费的错配是结构审计（P1-1 编辑器双轨）在协作域的具体投影。
- **过早性裁决（平衡陈述）**：CRDT 引入了不可回退的复杂度红利——第二份块模型、第二套操作面、第二套身份、桥 crate、归一化终态语义（P1-2）；但 yrs 的引入被隔离不变量钉死在单 crate 内（公开 API 零 yrs 类型——撤除成本 = 1 crate），且 yrs 的选型研究链（collab-line 分析仓、p18/p20 上游缺陷登记、B1.1-B3.4 纪律编号）显示这是**知情的**而非跟风的决策。**结论：不是「无需求复杂抽象」的滥用，而是「有纪律的期货」；真正的债务不在 yrs 而在域模型双轨 + 桥无消费者。处置建议：保持 collab/canvas 现状（质量高、隔离好），冻结 relay/mdbridge 扩张，直到出现真实第二用户/真实网络宿主的产品决策。**

### 2.8 【问题8】hostile.rs 敌意输入面：参数/字节/侧车三面强防护；身份伪造在 mdbridge 层补防；presence 对端不设防（已文档化且无产品表面）

- **hostile.rs 覆盖面（全部带零污染断言）**：
  - 构造面：client_id 0 / ≥2^53 拒（:26-47——yrs release 静默破坏 ID 编码的显式防线）；
  - 参数面：start>end / end>len（char 域前置校验）/ mark 越界 / ghost 块六类操作全拒 / index 越界 / 物化 id 撞号 / hr 文本面 / 空 mark kind（:50-209）；
  - 限额面：装载/编辑/apply_update/侧车四路字节门 + 会话零污染（:212-260）；
  - 侧车面：坏 magic / 版本 / **敌意 count 预分配**（u32::MAX 物理上界拒绝 identity.rs:97-102——防 HashMap 预分配炸内存）/ 尾部冗余（:263-302）。
- **伪造 op（wire 层状态毒化）的纵深**：apply_update 解码失败/panic 隔离/限额三重（session.rs:425-457 + recovery.rs:165-213）；**audit_update 先审后用面**（audit.rs:152-174：隔离暂存 Doc 上应用+走查，零本端副作用）检出 Error 四类（缺文本节点/缺 id/重复 id/外域根）+ Warning 十类（未知标签强转/根级文本/mark 缺 kind 等——读模型静默丢弃面全部显式登记）；repair_text_nodes 只做唯一可机械修复项（origin=mdc:maintenance 不进撤销栈，audit.rs:199-238）。**注意：先审后用是集成纪律（is_clean 才 apply）而非强制门**——宿主跳过审计直接 apply_update 仍可摄入毒形状（之后靠 audit_self + 读模型降级兜底）。敌意形状测试在 audit.rs 用**绕过引擎直写 yrs wire** 的真实敌意载体（audit.rs:46-60 external_doc/full_update——dev-dep 直接用 yrs 构造，测试面诚实）。
- **身份伪造**：collab 层 block_id 是开放字符串域（对端可发任意 id）——**find_block 全树属性扫描天然容忍**（session.rs:528-533「远端插入的块无需侧车即可寻址」）；**同族伪造**（对端伪造 `md:{本端client}:{n}`）在 mdbridge 层防御：family 解码复用 n + observe 抬水位 → 本端永不复用该 n（mdbridge lib.rs:35-38 + bridge_sync.rs:108-132 断言后续分配 ≠999 + 伪造块结构保真可物化）；空 block_id / 敌意撞号 → 孤儿分配（身份跨轮不稳定登记，host.rs:320-322）。
- **origin 伪造：结构性不可能**——Yjs update 不携带 origin，撤销栈 tracked_origins 只匹配本地事务 origin（session.rs:230-234 远端应用不带 origin → 进不了撤销栈）——**远端无法伪造「Human 的撤销历史」**。这是设计里最漂亮的一条结构性安全性质（虽未见显式文档宣称，行为被 undo_semantics_across_sessions 间接锁定）。
- **presence：对端不校验**（presence.rs:22-23「恶意对端可注入任意字符串，本 crate 零解析即零 panic 面；states() 消费方解析须防御性——文档化边界」）：本端 set fail-closed（JSON 必须可解析，presence.rs:197-207）；对端 JSON 不验证（yjs 生态兼容面）+ 入向 1MiB 限额 + panic 隔离（:261-281）。**风险敞口被 wasm 出口决策关闭**：presence 不在 mdcore-wasm 出口（collab.rs:28-30「传输层/锚点身份层——宿主集成轮事项」）——当前产品无 presence 消费者，防御责任文档化且无人接。relay 测试对超限在场 update 有拒收锁（relay.rs:219-243）。
- **总评**：对一个 3.2K LOC 的 CRDT 适配层，敌意面强度属第一梯队（fail-closed 全路径 + 零污染断言 + 审计/修复纵深 + 物理上界防预分配）；弱点两处——audit_update 非强制、presence 对端不设防（均已被边界文档登记，且后者无产品表面）。

---

## 3. 死 crate 群独立核验（对 01-structure 结论的复核）

| crate | 01-structure 判定 | 本轮独立复核证据 | 结论 |
|---|---|---|---|
| mdbridge | 零生产消费者 | rg `mdbridge|MdCollabHost` 于 mdcore-wasm/src 零命中；mdcore-wasm/Cargo.toml 无 mdbridge 边；全仓命中 = 自身测试（bridge_*.rs）+ workbench dev-dep（Cargo.toml [dev-dependencies] :34-37）+ workbench/examples | **成立** |
| relay | 零生产消费者 | 全仓 mdc_relay 命中 = 自身 tests/examples + Cargo.lock + web/src/collab_channel.ts:3 **注释引用**（「mdc-relay RelayMessage 语义的客户端侧镜像」——TS 侧重实现了消息协议而非依赖 crate） | **成立**（且发现：web 用注释级「协议镜像」绕开了 crate 依赖——relay 的协议设计实际影响了产品，但代码资产未被消费） |
| office | 零生产消费者 | rg `mdc-office`（带引号精确依赖形）于 **/Cargo.toml 零命中；命中仅各处注释 | **成立**（消毒层未接线维持 01-structure P2-5） |
| mdshell | 仅 shell-native（蓝图链） | 本轮未重验（超范围）；01-structure §2.1 证据链充分（shell→shell-native 链尾是未发布 bin） | **维持** |

补充事实：collab 的产品消费仅经 mdcore-wasm（Cargo.toml:26）喂 web collab_channel harness——**collab 不在死 crate 群**（有编译边+运行面），但其消费形态是「探针驱动的演示文档」而非编辑路径（§2.1）。

---

## 4. 问题清单（P0-P3 + 置信级别 + 证据链）

> 置信口径沿用 01-structure：已确认 = 本轮代码/git 实证；高度怀疑 = 证据充分未运行验证；需外部验证 = 需构建运行或用户裁决；设计偏好 = 非缺陷。

**P0（数据丢失/安全/核心状态破坏）：本轮范围内未发现。**（以下 P1-1 是「结构性无消费」而非破坏；P1-2 是代码自认的并发窗口。）

### P1-1 协作域与编辑域平行宇宙：8.4K LOC 协作资产（collab+mdbridge+relay）无产品编辑路径
- **证据链**：mdcore-wasm/Cargo.toml:26（有 collab 无 mdbridge）；rg mdbridge 于 mdcore-wasm/src 零命中；web/src/collab_channel.ts:443-445（独立 docSession）+ :295-413（仅 __collab* 探针驱动）；main.ts:1745（setupCollabChannel 装配但无 UI 编辑面）；mang-shell 零 collab 依赖（01-structure §1.3）；§3 死 crate 核验表。
- **影响**：用户真实编辑的文档（WasmCore/CM6）永不进入协作；协作会话是平行演示文档——「人机共用可同步共享文档状态」（collab lib.rs:1-8 产品语义宣称）在产品内不成立（只在 mdbridge 测试与 workbench 组合测试内成立）。维护双份块模型/操作面/身份体系。
- **置信**：已确认。
- **处置建议**：三选一裁决——①把 mdbridge 接进 mdcore-wasm（编辑 op 流→collab，代价：双 undo 语义冲突需裁决）；②收缩 collab 为 canvas 同款「表面域」（承认演示定位）；③维持现状但冻结 relay/mdbridge。取决于「真实第二用户」何时成为产品需求。

### P1-2 SetMark × 并发文本编辑的覆盖窗口（代码自认、无测试锁）
- **证据链**：collab/src/ops.rs:502-505（「同块并发文本编辑可能被本归一化终态取代——收敛性恒成立[CRDT]，字面叠加语义注册为已知面」）；concurrency.rs 测试矩阵无 mark×text 交错项（全文件仅 Replace×Replace/Insert×Insert/三方链/undo）。
- **影响**：双端同块并发（一端 SetMark、一端 ReplaceText）时对端字符编辑存在丢失/漂移窗口——CRDT 收敛性保证双端一致，但一致到的内容可能不含被覆盖编辑。
- **置信**：代码自认已确认；**实际丢失形态需外部验证**（需构造双会话交错跑观察读模型）。
- **处置建议**：补 concurrency 测试（SetMark 并发 ReplaceText 双向）钉定实际行为；若确有覆盖，将 set_mark 的整段重建改为差量（仅重建受影响区间节点）。

### P2-1 evidence Missing/Ambiguous 死状态：三态（实四值）设计完整但两值零产生路径
- **证据链**：status.rs:19-30（四值定义）；sidecar.rs:156-177 mark_source_unavailable 全仓消费者=零（rg 实测）；workbench 来源读取失败走 XlsxRead 错误而非标 Missing（pipeline.rs:596-625）；Ambiguous 对位置寻址结构不可达（§2.5）。
- **影响**：正交状态模型的 2/4 linkage 值是死字母——报告/gate 代码为它们写的分支永不触发；「来源文档缺失」的真实产品场景（文件被删）不会在 sidecar 留痕。
- **置信**：已确认。
- **处置建议**：workbench 的 load_source 失败路径调 mark_source_unavailable（Missing）——一行接线让死状态活过来；或在文档中降级登记「预留态」。

### P2-2 DocxParagraph 定位器索引级联误伤：段首一次插入 ≈ 后续全量证据 Stale
- **证据链**：sidecar.rs:400-417（refresh_document 按同 index 比指纹、越界=删，**无指纹重寻**）；locator.rs:104-110（位置+指纹混合、无内容寻址回退）。
- **影响**：高变动正文的证据链维护成本 O(证据数)（逐条 reverify_claim 手工恢复）；「存活率」结构性偏低（仅原地未动段落存活）。
- **置信**：已确认（代码路径推演）；用户实际痛感需外部验证。
- **处置建议**：refresh 后新增「指纹唯一匹配重绑候选」建议面（不自动绑——保持 §四.5 铁律，只把候选喂给人工重验 UI）。

### P2-3 relay 无传输/无持久化/无认证 + 零消费者（维持 01-structure P2-8，新增网络面实证）
- **证据链**：relay lib.rs:20-23、:94-95、:115；全仓 rg websocket/tokio/axym 产品零命中（仅 spike 孤儿锁）；doc_harness.rs:192-194 pump(){} 空实现；活跃度 2 commits 停滞。
- **影响**：226 LOC 状态机 + 598 LOC 测试的维护面无回报；「服务端中继」的宣称（crate 描述）与现实（纯内存内核）有距离。
- **置信**：已确认。
- **处置建议**：冻结；文档改称「中继协议内核（宿主集成前置件）」。

### P2-4 协作「多人」宣称与产品现实的落差（BroadcastChannel=同源同用户跨 tab）
- **证据链**：collab_channel.ts:6-7；web/tests 三套 e2e 双 page 模拟；无网络部署面（§2.4）；collab lib.rs:5-8「一份结构化文档由多个会话（人类编辑器/AI 生成端）共享」——产品内第二会话=同用户第二 tab。
- **置信**：已确认（落差事实）；是否构成问题取决于产品路线（需外部验证）。

### P3-5 audit_update 先审后用为纪律非强制 + presence 对端不设防（已登记、后者无产品表面）
- **证据链**：audit.rs:148-151（「集成纪律：外部数据先审后用——is_clean() 才 apply_update()」——无强制机制）；presence.rs:22-23；mdcore-wasm collab.rs:28-30（presence 不出口）。
- **置信**：已确认。

### P3-6 修正项：evidence 125 unwrap 全部位于内联测试（01-structure P3-12 对 evidence 的怀疑不成立）
- **证据链**：awk 边界切割实测——calc.rs 45 处全在 #[cfg(test)]（:323）后、sidecar.rs 42 处全在 :652 后、expr.rs 22 处全在 :659 后；collab 生产面仅 3 处构造性安全（session.rs:660 expect push/pop 对称、ops.rs:609 unwrap 非空集合首元、events.rs:164 构造期 fail-fast panic）。
- **置信**：已确认（正向修正）。

### P3-7 collab Cargo 注释漂移：「本 crate 是产品面唯一 yrs 接触点」已不成立（canvas 亦直接用 yrs）
- **证据链**：collab/Cargo.toml 注释 vs canvas/src/session.rs:14-18 use yrs。
- **置信**：已确认（文档级）。

### P3-8 设计偏好登记（非缺陷）
- 撤销栈软上限 clear_all 全失语义（session.rs:379-397 显式回执——粗但诚实，上游无逐出 API）；
- SetAttrs 终态钦定 / SetBlockType 换族重建（D-1）——CRDT 载体限制的已知代价；
- FNV-1a 64 非防篡改（fingerprint.rs:6-15 登记升级路径）；
- 段落定位器按「仅段落计」的 index 口径（ingest.rs:174-177）与表格块不占号的一致性依赖约定。

---

## 5. 投入优先级裁决（供主审计员）

```
evidence（契约层，3.9K）      ★★★★★ 保护+接线（把 Missing 接活、把 CrdtAnchor 稳定性考虑接入 Locator 演化）
workbench（编排域，2.2K）     ★★★★  继续投入（有产品 UI、领域完整；下一步是把 AI 循环从测试接到产品信封）
collab（CRDT 引擎，3.2K）     ★★☆  维持现状（质量高、隔离好）；冻结扩张直到 P1-1 裁决
relay / mdbridge（0.6K+1.0K） ★    冻结（死 crate；relay 协议思想已被 web TS 镜像吸收）
```

**优先级一句话**：证据与工作台是「活在产品里的资产」应继续投入；协作引擎是「高质量期货」，其价值兑现有待编辑域与协作域的桥接裁决（P1-1）——在那之前，relay/mdbridge 的维护应停止。

—— 完 ——（本轮零写入仓库；产物仅本笔记文件）
