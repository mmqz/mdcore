# 审计笔记 25：研究仓 typora-re-analysis 全量审计（R5-b，2026-11-05）

> Task ID：**R5-b**（mdcore 全仓代码审计项目 · 研究仓全量覆盖 · typora-re-analysis 子仓）。
> 审计对象：`/home/z/my-project/mang-research-archive/typora-re-analysis`（只读）。
> 铁律执行：两仓全程零写入、零 git 操作、零二进制执行；Python 仅 `py_compile`（24/24 通过）；
> JS/TS/Rust 全静态阅读。本文件为唯一写动作产出。
> 证据口径：`路径:行号` 均为本次实测；LOC/文件数为 `find`+`wc` 实测。
> 风格与参照：`audit/notes/01-structure.md`；交叉引用笔记 02/04/07/11/15。

## 0. 总览事实（一分钟版）

- **性质**：Typora 闭源深度逆向研究仓 + 后续产品（mdcore）的决策/原型母本仓。三层身份：①Typora 三版本（0.11.18 / 1.14.5-dev / 1.14.10-stable）解密逆向产物与 26 篇深钻文档；②四对象（Typora/Delta/Obsidian/MarkText-muya）研究与 A/B Spike；③mdcore 的 ADR 体系、Rust 原型 workspace（`projects/prototype`，31 个 .rs）与合规审计家族（compliance_* 四目录）。
- **规模**：259 文件；文本 LOC 合计 ≈402.5K，其中 **sources/ 闭源提取 JS 占 319,841 行（79%）**——研究仓的主体质量是 Typora 专有代码，产品价值在 md（15,262 行）与 rs（10,552 行）。
- **原型吸收率：31/31 = 100%**（文件级）。`projects/prototype` 四 crate（mdcore/mdcore-wasm/shell/shell-native）是 mdcore-audit md 线的**直接前身**：13 个 src 模块全部在主仓存在，`document.rs`/`id.rs`/`tests/r16_blocks_range.rs` 三文件逐字节相同（diff=0）；主仓在其上新增 4 个 src 模块 + 9 个测试文件继续演化。
- **最重要发现（F1）**：ADR-B5 声称「已裁决并落地（2026-10-09，R22-b）」的求逆回滚（`apply_batch_invert_rollback`，三处 `tree.clone()` 全消、逆组保序修复），在**原型母本与 mdcore-audit main@96596d7 两份静态快照中均不存在**——两处 lib.rs 仍是「整树 clone 备份 + 扁平 `.reverse()`」的 ADR-B5 §背景 所述**待修形状**（详见 §7）。主仓既有笔记 04-bridge-host.md:196 / 11-quality.md:110 亦将其列为未治理项，与本发现一致。
- **PDF 载体计划 → mdcore pdf 线**：`plans/pdf_carrier_plan_2026-10-10.md` 的选型（pdfium 读 + lopdf 写）与「第二 Document Model 不互嵌」在主仓 `crates/pdf` 主干落实（笔记 07-pdf.md 佐证：lopdf 0.45.0 / pdfium-render 0.9.4 pin、零 path 依赖）；但计划承诺的「lopdf 增量更新保留原字节——签名友好」被笔记 07 的 P0（`save_to` 全量重写摧毁签名）证伪，安全扫描接线（S2/S6）在桌面消费面未接。
- **合规四目录结论一致（全 CLEAN）**，且方法学有实质演进：22-x denylist（覆盖 <1%）→ RC-R 全量正向对照（产品 token 为针 × 四源全量，8954 命中逐级甄别）→ RC-S 行为/结构/常量/产物五维 → rounds 独立复核 + 原型母本镜像清洗。证据链闭合度高。
- **mang-shell（GPUI 主线）对原型零吸收**：`mang-shell/src/` 中 grep `extend_from_markdown|sync_from_markdown|adopt_external` 零命中——原型只被 web 线（CM6 桥）吸收，与笔记 01 §0 的双线格局互证。

## 1. 全量清单与统计（find+wc 实测，2026-11-05）

### 1.1 类型统计（259 文件）

| 类型 | 文件数 | LOC | 主体 |
|---|---|---|---|
| .js | 48 | 319,841 | sources/ 三版本闭源提取料（解密/美化/webpack chunk） |
| .tsv | 13 | 23,908 | compliance_fullcross/hits（12）+ round1 候选池（1），机械对照命中明细 |
| .ts | 9 | 14,455 | spike A/B + core mock + prototype/web 桥 + harness |
| .md | 71 | 15,262 | 分析 29 + 决策/计划 11 + 合规 13 + projects 16 + README/主档 2 |
| .rs | 31 | 10,552 | projects/prototype 四 crate（§3） |
| .txt | 23 | 9,568 | 符号/IPC/端点 dump（delta d1-d4 数据 14 + recon 6 + ipc_surface + LICENSE.txt 2） |
| .json | 19 | 6,641 | endpoints_raw（3019 行）/ jsc_analysis / results*（4）/ package-lock×2 / 其余配置 |
| .py | 24 | 4,311 | scripts 3 + fc 脚本 6 + delta 数据 7 + web 测试 7 + spike runner 1 |
| 其他 21 | — | — | Cargo.toml×5 / Cargo.lock×2 / index.html×4 / build.mjs×2 / .gitignore×3 / sql / sh / **needle.pkl / atom.compiled.dist.jsc（388,344B 二进制）/ __pycache__/*.pyc** |

### 1.2 目录树与定位

```
typora-re-analysis/
├── README.md                        声明：Typora 分析阶段 COMPLETE（26 文档 ~4050 行）
├── project_panorama_master_2026-10-07.md  权威主档（1579 行：愿景/ADR/路线/R1-R24+RC/§32 PDF 裁决）
├── analysis/        29 md + 2 json   3 顶层（privacy/version_diff/decrypt）+ deep_dive 26 篇 + ipc_surface.txt
├── sources/         48 js            0.11.18（解密 13 + appsrc 3 + page-dist 13）/ 1.14.10（6+5+1 jsc）/ 1.14.5-dev（2+5）
├── decisions/       10 ADR           B1-B7 / C1 / H1 / View-01
├── plans/           1                pdf_carrier_plan_2026-10-10.md（PDF 五轮计划）
├── compliance_behavior/   1 md       RC-S：行为/结构级五维审计 + 产物反编译自检
├── compliance_fullcross/  报告+12 tsv+6 py+1 pkl   RC-R：全量正向对照（hits 22,673 行机器证据）
├── compliance_release/   3 md        NOTICES 草案（34 组件）/ ASSET_CHECK / TRADEMARK_RULES
├── compliance_rounds/    8 md        独立复核会话：round1 三层特征清单 + round2 和解与镜像清洗
├── scripts/         3 py             decrypt_01118 / jsc_analyzer / classify_dev_stable
└── projects/        delta（收口）+ marktext + obsidian（recon）+ spike（A/B 裁决）+ prototype（Rust 母本）
```

## 2. 逐区审计

### 2.1 decisions/ 全量登记（10 ADR，全部通读）

| ADR | 标题/结论 | 证据文件 | 落点验证 |
|---|---|---|---|
| B1 | 块身份=混合式：mutation 走 NodePath、意图/选区走 BlockId 锚点（解析失败显式 reject，绝不静默猜测）；ID 不写入 MD | ADR-B1-block-identity.md:13-27 | 原型 anchor.rs(40)/id.rs(69)；主仓 anchor.rs(67)。**注意**：笔记 02 F-2 指出主仓 anchor 无偏移平移映射，v0 语义弱于 ADR 叙述 |
| B2 | undo 分组=1s 时间窗 + kind 语义破组（v0）；AI 变更永远独立成条可 undo；整栈 JSON 往返；undo/redo 不入 RevisionLog | ADR-B2-undo-grouping.md:12-25 | 原型 history.rs(258)；主仓 history.rs(300)。词边界破组留 R9 实测（遗留） |
| B3 | 行内 tokenizer 归 Core（MarkSpan 最小集）+ tree-sitter 只管块级派生 | ADR-B3-inline-tokenizer.md:11-22 | 原型 op.rs SetMark；主仓 inline.rs 为新增演进 |
| B4 | System 源事务不入 undo 栈（undo 栈=交互编辑史）但推进合并窗边界；边界规则=「谁驱动内容变化」 | ADR-B4-system-undo.md:13-30 | **两仓 lib.rs 均见 `if meta.source == Source::System` 独立分支**（原型 lib.rs:248 / 主仓 lib.rs:316）——吸收确证 |
| B5 | 求逆回滚：废除 tree.clone 备份、逆组「组间逆序组内保序」修复嵌套 undo 硬失败/静默错挂 | ADR-B5-inverse-rollback.md:13-40 | **两仓均未落地**（§7 F1）：原型 lib.rs:229/246/285/765 与主仓 lib.rs:297/314 仍是旧形状 |
| B6 | 块指纹：两层（结构 0xB7 / 内容 0xC1 域标签 FNV-1a 64）+ 三级匹配（manifest/结构锚/中段 Exact+相似度 0.7·8）+ 五类报告 | ADR-B6-block-fingerprint.md | 原型 fingerprint.rs(840)→主仓(879)；语义锁 r23_fingerprint.rs 451→522 行 |
| B7 | 外部重载三原语：FileStamp 探针 / plan_reload 三态决策（Noop/SilentAdopt/OfferChoice）/ adopt_external 全量重建+清交互史 | ADR-B7-external-reload.md | 原型 external.rs(197)→主仓(208)；r23_external.rs 244→325。mtime 毫秒+size 漏检残余=主仓笔记 02 F-11 |
| C1 | 许可姿态=闭源最严：GPL/专有/特异命名零容忍；**物理隔离**（产品代码迁出本仓→独立 mdcore 仓）；允许域=行为锚点+白名单依赖 | ADR-C1-license-posture.md:26-48 | 本仓 sources/ 仍留 26.7MB 专有料（合规位）；主仓 compliance/ 存在 |
| H1 | 宿主壳=tao+wry+GTK3 原生控件，Slint 排除（GPL-3.0 默认证+窗口栈不相容）；LGPL 系统库动态链接精确豁免 | ADR-H1-host-shell.md:13-31 | 原型 shell-native 63 行蓝图→主仓 447 行实体（笔记 01：10-26 后停滞） |
| View-01 | View 范式=**B（单 CM6+Decoration）条件性采纳**，0.758:0.715；代价=op 提取桥（快路径+全量重同步兜底）；Core 保持 op-first | ADR-View-01-paradigm-b.md:11-33 | spike/report.md（S1-S6 实测表）；主仓 web/ CM6 线全部承袭 |

### 2.2 plans/：PDF carrier engineering plan 深审（pdf_carrier_plan_2026-10-10.md，95 行）

- **计划内容**：新建 mdc-pdf「第二 Document Model」（与 mdcore 块核**并列不互嵌**），五轮 PDF-a 侦察选型 / PDF-b 骨架 / PDF-c 阅读面 / PDF-d 标注面+Acrobat 互通 / PDF-e 收口（§5 表）。
- **技术路线**：渲染/文本/搜索=**pdfium**（BSD-3，经 pdfium-render）；对象级写入=**lopdf**（MIT，增量更新保留原字节）；排除 mupdf（AGPL 违 ADR-C1）（§2 表）。风险表登记 pdfium 二进制分发链与 Acrobat 互通（§7）。
- **合规判断**：选型全部落在 ADR-C1 白名单族；安全策略「直接转化 Acrobat 审计三仓成果」——JS 引擎不接（V8/XFA 不启用）、/OpenAction//Launch//EmbeddedFiles//RichMedia 全禁、pdfium 独立线程零网络沙箱（§3 表）。v0 明确不做：内容编辑/生成/OCR/表单/AcroForm（§6）。
- **与 mdcore pdf 线的关系**（对照笔记 07-pdf.md）：
  - 【已确认】主干落实：lopdf 0.45.0 + pdfium-render 0.9.4 双 pin；三层分工（Document Model 零依赖 / pdfium 读引擎 / lopdf 写面）；crate 间零 path 依赖=「不互嵌」兑现。
  - 【已确认】**关键违背**：计划 §2「lopdf 增量更新（incremental update）保留原字节——签名文档友好」在主仓被笔记 07 定为 P0——`Document::save_to` 实为全量重写，恰好会在其目标场景（已签名文档）静默摧毁签名。
  - 【已确认】安全策略半接线：desktop 读路径（mang-shell）不接 S2/S6 危险对象扫描（笔记 07 §0），与计划 §3「打开时扫描危险对象族」的目标存在落差；CI 零 pdfium 全链覆盖。
  - 执行体 ADR-P1 落位于 cleanroom-analysis（笔记 07 头部参照），本仓持有的是计划与裁决链源头。
- **来源**：主档 §32（2026-10-10 用户裁决 PDF 提前为第二载体）+ 主档 §28 P2-13。

### 2.3 analysis/ 与 sources/

- **analysis/ 性质与规模**：3 顶层 + deep_dive 26 篇（4,033 行），README:19 称「~4,050 行蒸馏发现」。深钻覆盖：架构/许可协议/jsc/渲染引擎/安全面/主题/图片导出/数学图/草稿恢复/dev-stable delta/输入热路径/语法序列化/编辑语义/混合渲染/性能预算/选区模型/表引擎/加载管线/不变量与可观测/搜索替换/XSS 面/列表编辑/宿主桥与命令/外围系统/主进程辨识/平台文本服务——即 Typora 编辑器内核的全子系统拆解。
- **版本 diff 方法**：三版本结构对照（version_diff.md：打包/入口链/体积/Electron 13.6.1→42.2.0）；dev↔stable 用 `classify_dev_stable.py` 把 315 hunk 分类为 282 cosmetic（压缩器标识符置换）/33 semantic（analysis/deep_dive/dev_stable_delta.md）。
- **telemetry 行为证据**（privacy_findings.md，静态重建 + 2026-10-01 活体验证）：0.11.18 默认**开启**匿名遥测（`sendAnonymousUsage==null→true`，opt-OUT），AWS Pinpoint AppId `4eeda6df…` + Cognito pool `us-west-2:146e5a95-…` + 持久 UUID；1.14.x 全部入口 stub 为 `return void 0`、默认翻转 false；`typora.io/api/endpoint` GeoIP（实测 HTTP 200 返回 base64 城市串）。raw 数据：endpoints_raw.json（3019 行）+ ipc_surface.txt（194 行 IPC 频次表）。
- **sources/ 里 JS 是什么**：【已确认】**Typora 闭源代码的提取/复原产物**，证据：
  - `sources/0.11.18/app-decrypted/`：12 个 AES-256-CBC 解密后的主进程模块（License.js 444 行 / Setting.js 884 行 / atom.js 532 行 / menu.js 1593 行…，License.js:1-10 可见 `require("Env.js")`+electron API——完整可读的专有源码）；解密钥匙硬编码于 `scripts/decrypt_01118.py:25`（`DEFAULT_KEY="4ee1b382…"`），方案文档 decrypt_0.11.18.md（IV 前置 + main.node .rodata 0xc880 偏移取钥）。
  - `sources/1.14.10-stable/appsrc/frame.beautified.js`（60,468 行）/`main.beautified.js`（65,104 行）：美化后的渲染器 bundle；`main-process/atom.compiled.dist.jsc`（388,344 字节 V8 字节码二进制，未反编译，经字符串池交叉解读）。
  - `page-dist/js/*.chunk.js`：webpack 产物（含 .LICENSE.txt 第三方许可标注）。
  - 法律姿态：README.md:3-8「仅限个人安全研究，禁止再分发/商用」；ADR-C1 §1 定性「零接触净化室：仅行为锚点可复用」。

### 2.4 compliance_* 四目录（全部通读）

| 目录 | 轮次/维度 | 方法 | 结论 | 方法学评价 |
|---|---|---|---|---|
| compliance_fullcross（RC-R，2026-10-09） | 标识符+字面量，**正向全量** | 产品仓全量 token 为针（RAW 2804+复合 1051+gram ~5.1 万）×四源全量文本流式对照；命中 8954（typora 1480/delta 1786/obsidian 788/zed 4900）逐级分桶甄别；5 个 fc_* 脚本可复跑 | 零特征残留/零特有造词；15 处设计出处名提及中性化 | **高**：正视了 denylist 时代「0 命中只证明精选词条零命中」的覆盖 <1% 缺陷，方向反转补全覆盖 |
| compliance_behavior（RC-S，AUDIT_24_BEHAVIORAL.md 234 行） | **结构/行为/常量/产物**（补齐 ③④⑤ 维） | 模块分解三侧正交对照；关键行为常量三值互异（undo 合并窗 3000/300/1000ms）；编译产物反编译自检 | 四层证据链 CLEAN；并明确「免责的是独立创作，不是改名」 | **高**：直接回应「改名不免责」的法律精确化（思想/表达二分） |
| compliance_rounds（独立复核会话） | round1 三层特征清单（337 标识符/2,121 字面量/结构图 + 392 靶）；round2 与 RC-R/RC-S **基线和解** + 发现原型母本 15 处名字中性化未同步的缺口并镜像清洗复验（392 靶复跑：可疑 8→1、确凿 0） | 双方法学交叉印证 | CLEAN 双门闭合 | **高**：独立会话 + fetch 和解 + 母本漂移（「复活风险」）的真实发现，见 §7 F2 |
| compliance_release | 发布形态：THIRD-PARTY-NOTICES 草案（34 组件逐包按锁定版本取证，UNKNOWN=0，与 R22-c SBOM 367 项交叉一致）；ASSET_CHECK（零内嵌资产）；TRADEMARK_RULES（命名域+定名检索门） | 可复跑脚本（r2_gen_notices.py） | CLEAN | 中-高：草案态，发行前须以发行时点元数据复核（自认） |

**一致性**：四目录 + 22-x 前置审计全部 CLEAN，无互相矛盾；分歧点仅在方法覆盖面（已被 RC-R 补齐）。`cid` 一词按 AUTO-SHORT 通用缩写判留并登记处置（round2/self_grep_rerun_POST_CLEAN.md）——处置透明。

### 2.5 projects/（delta/marktext/obsidian/spike/prototype）

- **delta**：D1-D4 深钻（历史 DAG/副本并发/SQLite schema/线程-worktree 绑定）+ recon 数据（符号 27.2 万 demangled、DDL×4）→ **2026-10-08 按主档边界收口**（CLOSURE.md：产品层参考定位，D5-D7 不再执行）；定位「验证 operation-level AI-native work state，不因此引入 CRDT」。
- **obsidian**：recon round 0——Live Preview=单 CM6 实例+Decoration 管线（149,432 行美化 bundle 精读，key_regions.txt 24 区锚点）。
- **marktext/muya**：recon round 0——TS 重写架构（权威 JSON 状态 + ot-json1 op + DOM 镜像 + rebuild 兜底）。
- **spike**：范式 A/B 实验（a/ 334 行 vs b/ 365 行 + core mock 300 行 + runner），report.md 给出 S1-S6 实测与加权判决 0.758:0.715——**ADR-View-01 的数据底座**；Spike 中实踩 3 处 desync 缺陷的记录（report.md §2）是范式 B 风险的一手证据。TS spike 未入主仓（mdcore-audit/spike/ 仅有 gui-framework-eval）。
- **prototype**：R8-R11/R14-R24 递进（README:46-74 记载 W1-W10 全绿、45/45 cargo 绿）， gates.md 为门禁台账（四硬 Gate PASS + GAP 归因），CONTRACT.md v1 双侧契约冻结（含 `__contractVersion` 与双侧锁测试）。

## 3. Rust 原型与 mdcore 吸收对照（31/31）

### 3.1 原型是什么

`projects/prototype` = 自含 workspace（Cargo.toml: members 4 crate，default 3），**md 线原型**（非 pdf/office 线——pdf 线产物在 cleanroom-analysis + 主仓 crates/pdf，原型期计划仅 plans/ 文档）。31 个 .rs = mdcore src 14（含 lib.rs）+ examples 1 + tests 12 + mdcore-wasm 1 + shell 2（src+tests）+ shell-native 1。依赖策略：ropey/serde/wasm-bindgen 三件（Cargo.toml 注释「Core 语义不依赖任何一项」）。

### 3.2 文件级对照表（原型 → mdcore-audit main@96596d7，diff 行数实测）

| 原型文件（行数） | 主仓对应（行数） | diff | 判定 |
|---|---|---|---|
| mdcore/src/document.rs (284) | 284 | **0** | 逐字节相同 |
| mdcore/src/id.rs (69) | 69 | **0** | 逐字节相同 |
| mdcore/tests/r16_blocks_range.rs (151) | 151 | **0** | 逐字节相同 |
| src/debug.rs (156) / tree.rs (381) / external.rs (197) / revision.rs (136) / history.rs (258) / anchor.rs (40) / markdown.rs (301) | 160/387/208/146/300/67/330 | 8-60 | 吸收后小幅演化 |
| src/fingerprint.rs (840) / op.rs (285) / snapshot.rs (90) / html.rs (1427) / lib.rs (934) | 879/370/242/1682/1216 | 98-591 | 吸收后大幅增长（R23+/word_count/pagination 轮） |
| tests 11 件（138-843 行） | 同名全在 + **新增 9 件**（external_corpus/n3/n5/r24×3/t1/t3/tr1） | — | 全部承袭并扩容 |
| examples/inspector.rs (88) | 115 | 55 | 承袭 |
| mdcore-wasm/src/lib.rs (206) | 303 | 142 | 承袭（后长成主仓 10,296 行的应用层） |
| shell/src/lib.rs (167) + tests/contract_freeze.rs (70) | 217 + 178（+r24 三件新测试） | 105/134 | 承袭扩容 |
| shell-native/src/lib.rs (63) | 447 | 412 | 蓝图→tao+wry+GTK3 实体 |
| —（无） | **chunk.rs / inline.rs / pagination.rs / word_count.rs** | — | 主仓新模块（吸收后新增） |

**结论**：吸收率 **31/31 = 100%（文件级）**，其中 3 件逐字节相同、其余全部同名同构演化；主仓 mdcore src 18 文件 8,786 LOC = 原型 13 模块的严格超集。web 桥同样承袭：prototype/web/src/main.ts 1,377 → 主仓 web/src/main.ts 3,347 行（diff 2,010）。**mang-shell（GPUI 线）零吸收**（grep 零命中，见 §0）。

### 3.3 静态质量评估（原型代码）

- 文档密度极高：lib.rs:1-14 模块级 doc 声明职责边界与十条不变量映射；op.rs:1-11 声明求逆可证明性；op.rs:23 `OpText = Arc<str>`（R22-a 共享优化）。
- 契约纪律：CONTRACT.md §1 把 pub API 定为编译级冻结面 + 语义锁测试清单（r11-r23 逐面）。
- 缺陷自证：gates.md 记录 R11 压力测出两核缺陷（MoveBlock 空 path 求逆溢出、undo 缺 revision log）并修复；R15 视口惰性把 selection p95 23.9→0.8ms 的根因分析（decoField.update O(doc) 全量重建）是高质量的代码级归因。
- 同样继承的问题：tree.clone 备份（§7 F1）、`pub tree/history/log` 可变字段（主仓笔记 02 F-4，原型 lib.rs 同形状）。

## 4. 脚本质量（24 个 .py，py_compile 24/24 通过，4,311 行）

| 脚本 | 评价 |
|---|---|
| scripts/decrypt_01118.py | 质量高：argparse CLI、pycryptodome 缺失时纯 Python 回退、12/12 文件验证记载于 docstring；含硬编码 AES 钥（研究性质使然） |
| scripts/jsc_analyzer.py | 质量高：V8 code-cache 头部解析（magic/version-hash/source-hash 被 bytenode 劫持为源长度的发现）+ 字符串分类器，--json/--md 双输出 |
| scripts/classify_dev_stable.py | **P3**：`DIFF = "/home/z/my-project/typora_work/dev_stable.diff"`（:7）硬编码一次性路径，仓外不可复跑（属一次性研究脚本自认范畴） |
| compliance_fullcross/scripts/fc_*.py ×5 | 质量高：针集合构建/流式扫描/自动分桶/分层/反查定位五件套，可复跑，命中落 出处:行号 tsv |
| compliance_fullcross/scripts/update_mdcore_compliance.py | **写入型脚本**（向 /home/z/my-project/mdcore compliance/ 注入 evidence/COMPLIANCE 增补节）——合规卷宗工作流的自动化件；本次审计**未执行**，仅 py_compile |
| projects/delta/deep_dive/data/*.py ×7 | 数据分析件（符号盘点/DDL 提取与校验）；d4_binding_inventory 有 __pycache__/*.pyc 执行痕迹 |
| projects/prototype/web/tests/*.py ×7 | 浏览器门禁（playwright/CDP：contract_gate/web_smoke/ime_cdp/diag_heap 等），results/*.json 为运行证据（475 行 W1-W12+G1/G2） |
| projects/spike/tests/runner.py | Spike 场景 runner（results.json 284 行为 A/B 裁决原始数据） |

## 5. 合规视角

- 【已确认】分区铁律被执行：sources/（专有料）与 compliance 结论层分离；round1 README 声明「全程未读取、未列举 sources/（净化室纪律）」。
- 【已确认】主仓（mdcore-audit）现状：compliance/ + compliance_release/ + NOTICES.md 在位（§1.2 目录树），denylist 域纪律延续（TRADEMARK_RULES §1 命名域 mdcore/mdshell/mdc-*）。
- 【已确认】**残留风险**（round2 已自认）：原型母本 `projects/prototype` 与产品仓的**代码级漂移**——15 处名字中性化镜像清洗只覆盖命名面；ADR-B5 等后续代码级改动未回流母本（§7 F1/F2 同根）。
- 【高度怀疑→已排除】sources/ 是否泄漏进产品：RC-R 全量对照 + 笔记 14 主仓合规面双重 CLEAN；本审计 grep 主仓 crates 未发现 sources 路径引用。
- 本仓自身合规姿态：README 法律声明在位；逆向产物不商用不复制的纪律与 ADR-C1 一致。

## 6. 与 mdcore 的关系（对照既有笔记）

| 研究仓结论 | mdcore 承袭证据 | 笔记对照 |
|---|---|---|
| ADR-View-01 范式 B | web/ 全 CM6 桥架构（main.ts 3,347 行） | 笔记 01 §0「web 线（CM6）」、笔记 20 |
| 主档 §8 ID 不变量/§10 十原语 | mdcore lib.rs apply_ops 十命令面 + 语义锁 | 笔记 02（并揭 F-1 持久化边界身份破坏、F-2 锚点弱化——研究仓设计承诺 vs 产品实现的落差） |
| ADR-B4 System 不入 undo 栈 | 主仓 lib.rs:316-331 同构分支 | 笔记 03 undo API |
| Typora 性能预算（perf_budgets.md：brush 200ms/undo 3s/35K-60K/2MB） | gates.md 四硬 Gate + W 系实测（1MB TTI 156.9ms 等） | 笔记 15-benchmark §「研究仓一手材料」 |
| PDF carrier plan（pdfium+lopdf/第二 DM/安全策略） | crates/pdf 25 模块 + pdf-wasm | 笔记 07（P0 签名摧毁=计划承诺违背；S2/S6 未接线） |
| ADR-C1/H1 合规与宿主壳 | 主仓 compliance 体系 + shell-native 447 行 | 笔记 01/14 |
| ADR-B6/B7 指纹与外部重载 | fingerprint.rs/external.rs + r23 语义锁 | 笔记 02 F-11/F-13（mtime 漏检、双胞胎缺口） |
| **笔误勘正**：笔记 15-benchmark.md §1.1 称本仓「26 ADR」——实为 **10 个 ADR**（decisions/）+ 1 个 plan；「26」应为 deep_dive 文档数 | — | 建议后续勘正 |

## 7. 发现清单（分级）

| # | 级别 | 发现 | 置信 |
|---|---|---|---|
| F1 | **P0（对账级）** | ADR-B5「求逆回滚已落地（R22-b，三处 tree.clone 全消）」与静态证据矛盾：原型 lib.rs:229/285/765 与主仓 main@96596d7 lib.rs:297 均保留 `backup = self.tree.clone()`；两仓 lib.rs:246/:314 均保留扁平 `inverses_seq.reverse()`——即 ADR-B5 §背景 描述的「嵌套子树删除 undo 硬失败 / 多 op 事务静默错挂」缺陷形状在两份快照中**仍在**。主仓笔记 04:196 / 11:110 亦按「未治理」处理。解释二选一：R22-b 实体仅存在于开发环境仓未回流 main；或主档增注与 main 实态脱锚 | 已确认（静态比对） |
| F2 | P1 | 原型母本漂移：round2 只镜像了 15 处**命名**清洗；代码级演进（含 ADR-B5、word_count/pagination 等新模块）未回流 `projects/prototype`——母本与产品仓分叉持续扩大，作为「历史母本」的证据价值逐步衰减 | 已确认 |
| F3 | P1 | PDF 计划承诺 vs 产品落差：增量写保留签名（计划 §2）→ 主仓 save 全量重写毁签名（笔记 07 P0）；危险对象扫描（计划 §3）→ 桌面读路径未接线（笔记 07）。计划的合规判断本身成立，产品化执行存在缺口 | 已确认（转引主仓笔记 07 实测） |
| F4 | P2 | classify_dev_stable.py 硬编码仓外绝对路径，不可复现；update_mdcore_compliance.py 为写入型脚本且目标路径为产品仓（工作流件，需受控使用） | 已确认 |
| F5 | P2 | jsc 二进制（388KB）与 needle.pkl/pyc 为二进制/执行痕迹存档，仓内无来源哈希登记（对照 acrobat 仓有哈希登记惯例）——来源完整性依赖 README 口径（sha256 仅记安装器层） | 已确认 |
| F6 | P3 | 笔记 15-benchmark「26 ADR」计数错误（实为 10+1）；研究仓 gates.md 台账止于 R24 编排（2026-10-10），此后演进只在主仓，研究仓定位为冻结参考（README:21-23 自认） | 已确认 |

## 8. 结论与覆盖声明

**一句话判断**：typora-re-analysis 是 mdcore md 线的**完整决策与原型母本仓**——Typora 逆向（26 篇深钻 + 三版本源料）产出行为锚点与性能预算，经主档 → 10 ADR → A/B Spike → Rust 原型（31 rs，100% 被主仓吸收）→ 合规四目录（全 CLEAN，方法学从 denylist 演进到全量正向对照）→ 产品仓物理隔离，链条完整可追溯；但其「已落地」声明与 mdcore-audit main@96596d7 之间存在 **ADR-B5 级实现落差（F1）** 与原型母本代码级漂移（F2），且 PDF 计划的两项关键承诺在产品面未兑现（F3）——研究仓结论不能默认等于主仓实态，须逐项对账（与笔记 15 §1.2「时点漂移」结论一致并加重）。

**覆盖声明**：
- 全量清单/统计：**100%**（259 文件逐一路径枚举 + 8 类型 LOC 实测）。
- decisions/ + plans/ + 主档 + README：**100% 通读**（11 件：2 件全文精读 + 8 件全文/头部+关键节；主档 1,579 行全文）。
- compliance_* 四目录：**100% 通读**（13 md 主体全文；hits 12 tsv 抽样验证格式与出处口径）。
- projects/ 五子项目：README/CLOSURE/CONTRACT/gates/report **100% 通读**（16 md）。
- analysis/：3 顶层全文 + deep_dive **4/26 深读**（license_system/hybrid_rendering/jsc/main_process 相关）+ 22/26 清单级确认（README 逐文件摘要 + 行数核对）——文档层约 60% 深读 + 100% 清单覆盖。
- sources/：48 文件**100% 清单/行数/性质确认**，正文抽样 3 件（License.js/frame.beautified.js/launch.dist.js 头部）——不逐行（319,841 行闭源料，审计目标为性质与规模）。
- .rs：**100% 枚举** + 16/31 与主仓逐文件 diff + 其余 15 件行数对照；3 件核心（lib/op/tree 相关）深读。
- .py：**100% py_compile（24/24 过）** + 4 件精读。
- tsv/json/ts/txt/pkl/pyc：100% 枚举 + 性质抽样确认。

（审计执行：R5-b 子代理，2026-11-05；零 git、零执行、零既有文件修改。）
