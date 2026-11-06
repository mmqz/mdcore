# 15-benchmark · 竞品对标笔记（D2）——用研究仓一手材料为终报告 §5 建立证据档案

> 审计对象：mdcore-audit（audit 分支，main@96596d7 口径）＋ mang-research-archive（cleanroom-analysis / typora-re-analysis / acrobat-pro-audit）。
> 本笔记性质：**证据档案 + 增量深化**，不是新的架构裁决。既有终报告 §5（`audit/final-report.md:250-286`）的结论级判断（六个领域共享「视图/语义分界线」；自研负 ROI 在视图层、差异化在语义层）在本笔记中获得一手出处锚定，并做逐项复核。
> 证据纪律（三档位）：
> - **【研】**＝研究仓一手结论（引用给 `mang-research-archive/` 相对路径 + §/行号）；
> - **【主】**＝主仓现实（引用给 `文件:行号`）；
> - **【我】**＝本笔记者的知识判断（2026-11 视角，显式标注）。
> 铁律遵守：两仓全程只读、零 git 操作；产出仅落本文件。

---

## ① 研究仓全景与可信度评估

### 1.1 三个研究仓的构成与分工

| 仓库 | 规模/结构 | 角色 | 关键入口 |
|---|---|---|---|
| cleanroom-analysis | 六线（md/office/pdf/canvas/collab/workbench）+ DECISIONS.md（20 项裁决登记）+ DECISION_SHEET.md（四批次用户批注）+ DECISION_GROUPS.md（17 项→5 组）+ HANDOFF.md（895 行）+ worklog.md（7203 行追加式） | **mdcore 设计决策源**：全部选型/裁决的证据链与用户批注 | `cleanroom-analysis/README.md`、`DECISIONS.md`、`HANDOFF.md` |
| typora-re-analysis | 26 分析文档 ~4050 行 + 三版本源料（0.11.18 解密 / 1.14.10 / 1.14.5-dev）+ 26 ADR（B1-B7/C1/H1/View-01）+ 主档 `project_panorama_master_2026-10-07.md`（1578 行）+ muya/Obsidian/Delta recon | **MD 线审计底稿仓**（分区铁律：底稿不出该仓，cleanroom 只存结论层） | `typora-re-analysis/README.md`、主档 §4/§9/§14 |
| acrobat-pro-audit（并档于 cleanroom `acrobat-audit/`） | 4 分析 + 25 证据 JSON + 10 工具 + 哈希登记 | **PDF 域敌意输入与行为面参照**（Acrobat Pro DC 26.002.21931 安装器黑盒取证） | `acrobat-audit/README.md`、`analysis/04-风险定级与处置建议.md` |

### 1.2 可信度评估（分维度）

| 维度 | 评估 | 依据 |
|---|---|---|
| 证据可回溯性 | **高** | 净室纪律：关键结论带「报告 §」或 `路径:行号`；【推断】/【建议】字面机械计数（`pdf-line/06` 附录 A：19+19 处零漏标） |
| 独立复核 | **高** | 14 篇深读全部经独立复审+修补（`cleanroom-analysis/README.md:35`）；md-line boundary-audit 八件套 500+ 锚点经复审零错锚 |
| 决策可追认 | **高** | DECISION_SHEET 四批次均有用户批注原文与日期（如「批次 1 = 批准（2026-10-28 用户裁决）」，`DECISION_SHEET.md:36`） |
| 弱点 1：综合层间接引用 | 中 | `pdf-line/06` 综合层**零上游仓锚点**（设计纪律，附录 B:451-452）——上游事实须经各深读报告二次跳转才能落到一手源 |
| 弱点 2：性能量级多为【推断】 | 中 | 渲染光谱八路线的「对象量级」列 7 处【推断】随行标注（`pdf-line/06` §2.1）；非实测数字，只可作数量级参考 |
| 弱点 3：时点漂移 | 低-中 | 研究仓基准日集中在 2026-10-08~10-28；主仓 HEAD 为 2026-11-04 口径（`audit/notes/07-pdf.md:1`）——中间一周的漂移须逐项对账（见 ③） |
| Typora 仓法律姿态 | 特殊 | 逆向产物**仅限个人安全研究、禁商用禁再分发**（`typora-re-analysis/README.md:3-8`）；其价值是**架构事实与决策清单**，代码级零复制（Task 24 行为级审计四层 CLEAN，`compliance_behavior/AUDIT_24_BEHAVIORAL.md`） |
| Acrobat 仓 | 高 | 四重验签（TLS+MD5+Authenticode）官方安装器取证；11 张 MSI 表复现字节级一致（`acrobat-audit/README.md:56`） |

**总评**：研究仓是「决策之前」的证据档案，主仓是「决策之后」的实现现实；两者之间的差值（见 ③ 对账表）本身就是最重要的审计发现源。

---

## ② 六线决策档案（A）：每线「对标了什么 × 结论 × 证据 × 主仓验证」

### 2.0 六线总览——对标谱系与「视图/语义分界线」的出处

六线对标合计覆盖 **30+ 竞品对象**【研】。分界线结论的一手谱系：
- Typora/Delta/Obsidian/muya 四源 → MD 线范式裁决（`typora-re-analysis/project_panorama_master_2026-10-07.md` §9 State Ownership：Canonical Model=唯一语义真理、CM6/View=临时视图状态；§14 范式 A/B spike：B（CM6 Decoration）0.758:0.715 胜出、「core stays op-first」）【研】
- 办公/画布 14 篇深读 → 渲染光谱八路线 + 吞吐三招 + 「store 自持 + 渲染后端可换」（`pdf-line/06` §2.2/§2.4）【研】
- PdfCraft/开源 PDF 生态 → 「哑铃形」成熟度图（两端结实、中间层塌陷；`pdf-line/02` §0-§1）【研】
- 终报告 §5.1 的一句话判断（`audit/final-report.md:254`）是上述三支的汇流——**本笔记确认其每个分句都有一手出处**。

### 2.1 md 线（mdcore 块核 + CM6 桥）

| 竞品 | 能力缺口（对标发现） | 决策 | 主仓验证 |
|---|---|---|---|
| Typora 1.14.10 | 块状态机可行但 DOM↔AST 一致性是核心风险（fullValidate 七不变量）；性能臃肿主因是 Chromium/富媒体而非 parser | 范式 B（CM6 单实例 + decoration）为视图基线，core 保持 op-first（`projects/spike/report.md`；ADR-View-01）【研】 | ✅ `web/src/main.ts` CM6 桥为唯一现役产品路径；`web/package.json:17-19` @codemirror/* ^6.x【主】 |
| Obsidian 1.14.4 | Live Preview=单 CM6+手写增量 decoration 扫描器 | 与 B 范式互证 | ✅ 同上【主】 |
| muya（MarkText） | 开源块状态机参照：JSON 权威态+ot-json1 op+rebuild 兜底 | 「同题不同解」登记：路径身份 vs cid 身份二选一 | ⚠️ mdcore 用 cid 族（`crates/mdcore/src/id.rs`）；但 P0-1 撞号证明身份层未收口【主】 |
| Zed/Delta | operation-level history / AI-native 工作状态 | 「操作是一等历史对象」；**v0 不上 CRDT**（主档 §5.2C）【研】 | ⚠️ 现实分裂：mdcore op log 在（语义层✅），协作平行宇宙（见 ③-11）【主】 |
| **能力缺口总账** | 六项协同能力全缺失 + 推送订阅/按 ID 取块/块级监听三缺口 | 复用审计三分类（12 单元：5 直复用/5 适配/2 跨域不可复用，`md-line/boundary-audit/REPORT.md` §2）【研】 | REUSE.md 转录入仓【主】 |

**已验证/落空**：块核+CM6 桥+渐进打开（R9-R11，1MB TTI 153ms）已被主仓实现验证；「撤销栈无上限」「save→open ID 漂移」两个登记缺口在 HEAD **仍未修**且升级为终报告 P0-1/P1（`audit/final-report.md:44,324`）【主】。

### 2.2 office 线（office-io + formula）

| 竞品 | 能力缺口 | 决策 | 主仓验证 |
|---|---|---|---|
| Univer | xlsx 引擎级读写全在 Pro（OSS/Pro 边界机器可读）；「快照即模型是反面教材」 | 净室参考（主）+弱依赖候选（`pdf-line/06` §7#1）【研】 | 未引入；作参照【主】 |
| ONLYOFFICE | AGPL+服务器全序 OT 深绑私有真相源；输入限额三件套是正面资产 | 净室参考；直接依赖排除（ADR-C1）（`pdf-line/06` §7#2）【研】 | 未引入【主】 |
| HyperFormula | GPLv3 双许可+函数级行为锁闭源 | 净室参考；排除（`office-c` 全篇）【研】 | ✅ 后续以 **ironcalc_base 0.9.1（MIT OR Apache-2.0）**替代（`crates/formula/Cargo.toml:30`）——**此选型发生在研究仓之外**（issue #110 调研，2026-10-31）【主】 |
| 写侧四件套（docx/exceljs/pptxgenjs/sheetjs） | CSV 公式注入四库零实现、XML 控制字符三档实况、域指令直通 | 消毒三闸=管线层责任（`pdf-line/06` §6.1）【研】 | ✅ mdc-office 消毒六面落地（`crates/office/src/sanitize/` 六文件）但**零产品接线**（终报告 §1.2-4 消费倒挂）【主】 |
| Rust 生态五件（office-e 一手核验） | docx 渲染级读侧 Rust 空位（「蓝海不在模型解析，在渲染保真级读侧」，`office-e` §5）【研】 | ADR-O1：calamine 读/rust_xlsxwriter 写/docx-rs 读写/zip≥9.x（`office-line/ADR-O1-office-io-selection.md`）【研】 | ✅ calamine 0.36.1/rust_xlsxwriter 0.99.1/docx-rs 0.4.22 落地；⚠️ **zip 钉 8.6.0 非 ADR 建议的 ≥9.x**（`Cargo.lock` zip=8.6.0），以策略层补面（谎言头门）补偿【主】 |
| LibreOffice | Office→PDF 分页不一致风险 | C 路线（LO headless→PDF→mdc-pdf）+ R-O2 22 用例回归套件 + 批次 3 裁决双轨（基线 26.2.7+同步验证 26.8+支持范围 `>=25.2 && <27`，`DECISION_SHEET.md:105-134`）【研】 | ⚠️ 主仓 LO 测试全为**软门**（缺 soffice 即 SKIP，终报告 §6.2）；运行时版本检测/范围声明**零落地**（grep 全仓无 `soffice --version`/`26.2.7` 命中）【主】 |

### 2.3 pdf 线（mdc-pdf）

| 竞品 | 能力缺口 | 决策 | 主仓验证 |
|---|---|---|---|
| MuPDF | AGPL ❌ | ADR-C1 全面禁入（`md-line/pdf-a/ADR-P1-pdf-engine-selection.md` 排除记录）【研】 | ✅ 未引入【主】 |
| pdfium（BSD-3 式） | 增量写接口弱 | **读渲染走 pdfium**（经 pdfium-render 0.9.4，non-V8 构建断言 S1.1）【研】 | ✅ `crates/pdf/Cargo.toml`：pdfium-render "=0.9.4"、default 零引擎依赖【主】 |
| lopdf（MIT） | — | **标注增量写走 lopdf**（IncrementalDocument 原字节前缀保持、签名友好；探针 3：+471B 追加、pdfium 交叉读回 type=Highlight ✓）【研】 | ✅ writer_lopdf.rs:130/326 落实且为本域质量最高组件；⚠️ **lopdf 面扩张为十模块**，其中 forms「声称增量、实为全量重写」= 终报告 P0-3（`audit/notes/07-pdf.md:19-22,69-73`）【主】 |
| qpdf | 无 Rust binding 主轨 | 排除（写路径候选落选）【研】 | 未引入【主】 |
| PDF.js | 浏览器查看器≠文档平台（内存模型/XFA/JS 沙箱三墙，`pdf-line/02` §4.2）【研】 | 非竞品定位 | ⚠️ **web 视图侧实际用了 pdfjs-dist ^6.4.299**（`web/package.json:20`）——引擎侧 pdfium+视图侧 pdf.js 双栈并存，属「视图层借力生态」的正确分界，但双渲染栈成本未在终报告登记【主】【我】 |
| PdfCraft/开源生态 | 中间层（编辑/修复/表单/XFA/色彩/签名链/符合性）全生态真空 | 「分析别人怎么做」的 17 维方法论+敌意输入边界直接转为 PDF-a 安全策略表输入（`md-line/STATUS.md` §5）【研】 | ✅ SECURITY_POLICY S1-S5 落地（security.rs/security_scan.rs/worker.rs 看门狗 16-17）【主】 |
| Acrobat（互通目标） | 见 ⑤ | v0 只用标准 Annot 子型零私有扩展；PDF-d 实测互通 | ⚠️ 仅引擎层预证（lopdf 写→pdfium 读），GUI 人工验证未做（终报告 §6.2）【主】 |

### 2.4 canvas 线（mdc-canvas）

| 竞品 | 能力缺口 | 决策 | 主仓验证 |
|---|---|---|---|
| Excalidraw/tldraw/Penpot/AFFiNE/InvokeAI/drawio 六路深读 | 三基座（Konva/Fabric/PixiJS）皆**非协同编辑内核**；Excalidraw/tldraw 引擎不可拆；tldraw source-available 带遥测 | **自研 store + shape util 注册表 + 渲染后端可换**；矢量创作 Fabric；动画 PixiJS 仅回放基座（`pdf-line/06` §2.3 四口径矩阵）【研】 | ✅ mdc-canvas 自有模型（Node/Edge/Group+几何+端点，`crates/canvas/src/lib.rs`）+ yrs 根 Map 载体落地（2026-10-21 转正式交付，`collab-line/协作与Canvas正式交付报告.md` §1.2）【主】 |
| AFFiNE 双态 | 全 DOM 单层 2000 图元掉帧；两视图投影=数据模型先行六条 | D2 建议「两视图投影」但**暂不立项**（`DECISION_SHEET.md:142` 批次 4）【研】 | ⚠️ canvas 引擎已交付但**双态投影未实现**（终报告 §2.3 canvas 域无文档投影消费）【主】 |
| 吞吐三招（离屏位图/脏区增量/相机合成器） | — | 渲染层工程内核登记 | ⚠️ canvas render 面「文本无字形」等五课缺口（终报告 §5.2 暂缓类+P2 五课）【主】 |

### 2.5 collab 线（mdc-collab）

| 竞品 | 能力缺口 | 决策 | 主仓验证 |
|---|---|---|---|
| Yjs（协议） | v13/v14 断代、map 并发退化 LWW、删除零元数据 | **采纳 Yjs 同步协议子集**（SyncStep1/2/Update+SV+awareness，协议十年冻结 MIT）【研】 | ✅ yrs features=["sync"]（协议常量同源，`yrs-a` §3 源码锚点 protocol.rs:347-433）【研→主】 |
| yrs（实现） | #676 UAF（undo+并发 GC）/批量装载内存放大 3.6×/编码字节非确定 | D1② 裁 **(a) yrs 起步**；MIT 定谳+双语言 wire 互证实证（yrs 0.28↔yjs 13.6.33 双向精确重建）；skip_gc 全编队不变量【研】 | ✅ `crates/collab/Cargo.toml`：yrs 0.28、skip_gc 无条件注释在档、公开 API 零 yrs 类型；载体=XmlFragment 根（G1① 用户裁决 2026-10-28）【主】 |
| Univer OT/tldraw tlsync/Penpot 行锁/ONLYOFFICE 全序/Excalidraw LWW | 五方光谱：OT 族深绑私有真相源、离线需自建协议 | 「中心化权威快照+客户端 CRDT 离线自愈」双层（`pdf-line/06` §1.2 问③）【研】 | ⚠️ relay（权威副本扇出/差量追平）已写但属死 crate 群（终报告 §2.1）【主】 |
| **落空项** | — | S2-S5 决策记录（2026-10-21 早）：协作=可选子系统等真实多端需求（`workbench-line/S2-S5收口与阶段决策记录.md` §2） | ⚠️ 同日用户指令翻转「转正式交付」→ integ-r1..r5 落地；但**用户编辑流从未接入**（平行宇宙，终报告 §1.2-1/§8-Q6）【主】 |

### 2.6 workbench 线（证据系统+宿主）

| 竞品 | 能力缺口 | 决策 | 主仓验证 |
|---|---|---|---|
| web（CM6+wasm）vs egui 双原型 | egui 无对等 Markdown 编辑组件、IME 未验证；web 四类负载全落 60Hz 预算、IME CDP 28/28 | **主宿主=web+wry/tao，egui 备用**（2026-10-22，`workbench-line/宿主选型记录.md` §1：四维 A 两维 B）【研】 | ❌ **重大漂移**：HEAD 最活跃 GUI 是 mang-shell（GPUI+Zed text，33.8K LOC、不依赖 mdcore）；README 架构图仍画 tao+wry（`README.md:19`）——选型记录的 A/B 两案**均未被采纳**，第三案 GPUI vendor 未经过该基准对照【主】 |
| tldraw MCP 四件套/Penpot MCP/Univer Pro DirectWrite | agent 调用面「roadmap 已列、交付未发生」窗口 | MCP 工具面进内核仓的差异化窗口登记（`pdf-line/06` §4.2）【研】 | ⚠️ mang-shell 布局明示「无 AI 面板（MCP 暴露）」预留位（`mang-shell/Cargo.toml:19`）；引擎侧 RPC 桥在（mang-shell src/rpc.rs）【主】 |
| S1-S5 工作台计划 | — | S1 证据 MVP+S2 写入安全收口；S3/S4/S5 决策记录（协作可选/渲染后置/Canvas 后置）【研】 | ✅ workbench/evidence 双 crate 落地且被三域消费（终报告 §2.1 读图要点③）【主】 |

---

## ③ DECISIONS.md 决策清单对账（B）——12 条抽查

> 口径：「研究仓决策」→「主仓 HEAD 现实」，判定四档：**一致 / 部分一致 / 漂移 / 已裁决未落地**。

| # | 决策（出处） | 主仓 HEAD 现实（锚点） | 判定 |
|---|---|---|---|
| 1 | **ADR-P1：pdfium+lopdf 双引擎**（`md-line/pdf-a/ADR-P1`）【研】 | `crates/pdf/Cargo.toml`：pdfium-render "=0.9.4" + lopdf "=0.45.0"，default=[] 零引擎依赖；writer_lopdf 增量写主干落实（audit notes 07 §1.2 四项全✅）【主】 | **一致**；但 lopdf 面越界扩张为十模块、forms 增量写声称与实现相反（P0-3）——「ADR 只裁决标注增量写」的范围纪律失守【主】 |
| 2 | **D1②：yrs 选型 (a) 路线**（`collab-line/yrs-a`+DECISION_SHEET 批次 1）【研】 | `crates/collab`+`crates/canvas` yrs 0.28（lock=0.28.0）、skip_gc 无条件、XmlFragment 载体、隔离纪律（公开 API 零 yrs 类型）全部在档【主】 | **一致**；增量：终报告称「0.28.1 已合并 GC UAF 根因修复」（`final-report.md:269`）而 lock 仍 0.28.0、研究仓 maint-r1（2026-10-25）登记 0.28.0 为最新+PR #682 在途【研+主】——升级动作待执行 |
| 3 | **宿主：ADR-H1 tao+wry+GTK3**（`md-line/STATUS.md:14`）【研】 | mang-shell=GPUI 主线（`mang-shell/Cargo.toml:59` path 依赖 third_party/zed）；shell-native 568 LOC 蓝图停滞；README:3/19 仍声称 tao+wry【主】 | **漂移（三段式）**：tao+wry（10-10）→ web+wry 主/egui 备（10-22 基准）→ GPUI vendor（10-29，issue #63/#64/#68）。第三案无基准对照记录，且引入 GPL 48,562 LOC 商业阻塞（终报告 §1.2-2） |
| 4 | **CM6 桥（范式 B，ADR-View-01）**【研】 | `web/package.json:17-19`+`web/src/main.ts`：CM6 ^6.x 现役唯一产品桥；IME CDP 28/28 在案【主】 | **一致**（双轨问题在另一轨，不在本决策） |
| 5 | **ADR-O1：office-io 五件套**（`office-line/ADR-O1`）【研】 | calamine 0.36.1/rust_xlsxwriter 0.99.1/docx-rs 0.4.22/quick-xml 0.41 全按 ADR 落地【主】 | **部分一致**：zip 钉 **8.6.0**（ADR 建议 ≥9.x zip2 线），以「尺寸诚实性门」策略层补面补偿（office-io Cargo.toml description 自述）——版本漂移已知情补偿【主】 |
| 6 | **批次 3：LO 双轨 C 方案**（基线 26.2.7+验证 26.8+运行时检测+范围声明 `>=25.2 && <27`+季度复核，`DECISION_SHEET.md:105-134` 用户裁决）【研】 | 主仓 LO 面：9 个测试全软门（终报告 §6.2）；grep 全仓零 `soffice --version`/版本范围声明【主】 | **已裁决未落地**：四项落地动作（检测/声明/双轨验证/季度复核）无一在主仓出现 |
| 7 | **R-1 undo_coalesce_ms 接线**（批次 4 用户批准「A 接线」，`DECISION_SHEET.md:140,155`）【研】 | `crates/mdcore/src/history.rs:20` `pub const COALESCE_MS: u64 = 1000` 常量直读（:96 消费）；web 侧设置存储+UI 暴露（`web/src/main.ts:1451,3196`）但 mdcore-wasm **无任何 coalesce 参数**——存储链齐备、引擎面断链【主】 | **已裁决未落地**：UI 先行、引擎未接线（REUSE.md R5 坑位描述仍准确） |
| 8 | **R-3/B3.4：撤销栈维持无限+三项可观测交付**（批次 1 用户裁决：undo_depth()/512MB 告警/clear_all 入口，`DECISION_SHEET.md:42-52`）【研】 | 协同域✅（collab 双栈+软上限 clear_all 显式回执，交付报告 §1.1）；MD 本地域❌（History 无上限、无深度指标，REUSE.md R3 坑位③）【主】 | **部分一致**：协同域兑现、本地域欠账——恰是终报告 P1「身份层收口」同族问题 |
| 9 | **锚点写面 Phase 1+2**（批次 2 已裁决已执行 2026-10-17，`DECISION_SHEET.md:54-66`）【研】 | `crates/office-io/src/docx.rs:4487` `bookmark_paragraph` + 测试（docx_roundtrip.rs:7326-7330 XML 面锚定）【主】 | **一致**（4 文件 +398/-3 合入记录与主仓在位吻合） |
| 10 | **Source 镜像锁双向化**（T0-T5 实施轮，`md-line/STATUS.md:113-124`）【研】 | `crates/pdf/tests/pdfb_skeleton.rs:1128-1148`：五值词面镜像+mdcore 源面文件级对齐锁【主】 | **一致** |
| 11 | **S3「协作=可选子系统」**（2026-10-21 s2-r1 决策记录）→ 同日用户指令「转正式交付」【研】 | collab/canvas/mdbridge/relay/presence 五轮 integ 落地（交付报告 §3 状态更新）；但**用户编辑流从未进协作**（终报告 §8-Q6「平行宇宙」）【主】 | **漂移（向上翻转后又半落空）**：工程面全交付、产品面零消费——「正式交付」的产品语义未兑现 |
| 12 | **D2 双态暂不立项 / D3 生成态 A 主 B 辅**（批次 4 默认批准，`DECISION_SHEET.md:142`）【研】 | canvas 引擎（独立域模型）已交付；双态投影（xywh+displayMode+fractional index 入块模型）零实现；生成态路线 A 四件套未见于主仓（JS 侧无 exceljs/pptxgenjs 依赖，`web/package.json`）【主】 | **一致**（决策=不立项，现实=未立项）；风险登记：affine 六条设计输入随时间贬值 |

**对账总账**：12 条中 5 一致、3 部分一致、2 已裁决未落地、2 漂移。漂移集中面＝**宿主**与**协作产品化**——与终报告「编辑器双轨」「平行宇宙」两大结构病精确对应：研究仓材料早有预警（宿主选型记录的四维判据、S2-S5 的重启条件），但决策链在 10-21~10-29 一周内被连续翻转且未回写治理文档。

---

## ④ Typora 逆向的架构启示（C）

### 4.1 内核结构澄清：Typora ≠ muya

- **Typora 1.14.x 内核**＝自研闭源引擎：单 contenteditable 宿主 + 块内懒挂载 CodeMirror（viewportMargin:∞）+ 200ms dirty-block brush 队列 + quickParse 最小 DOM patch + cid/nodeMap 块身份 + AST 节点 JSON 快照式 undo（3s 合并/160 权重栈）+ fullValidate 七不变量（`typora-re-analysis/analysis/deep_dive/hybrid_rendering.md`、`renderer_engine.md`、`input_hotpath.md`）【研】。**不是 muya**——muya 是 MarkText 的 TS 重写引擎（JSON 权威态+ot-json1+块树 DOM 镜像+rebuild 兜底），作为「开源块状态机参照」单独 recon（`projects/marktext/recon/README.md`）【研】。
- **mdcore 块核的同构/异构点**（对照表，`projects/marktext/recon/README.md` §5 七行互证表）【研】：
  - 同构①：权威语义态与视图派生物分离（Typora AST↔DOM、muya JSONState↔ScrollPage、mdcore BlockTree↔CM6 doc）——三方独立互证；
  - 同构②：undo 栈可序列化（Typora 命令 JSON/muya getHistory/mdcore history_json）；
  - 异构①：块身份三解——Typora cid（稳但需回收）、muya 树路径（免注册表但怕移动失效）、mdcore 物化 ID+指纹（`crates/mdcore/src/id.rs`/`fingerprint.rs`）——「同题不同解」的自研分叉点；
  - 异构②：Typora/muya 的增量 DOM walker 均承认 desync 风险（muya history.ts rebuild 注释为**一手反面证据**）→ mdcore 以「核 op 流为唯一变异入口+快路径投影」替代 HTML diff，属架构代差改进。

### 4.2 「Typora 逆向佐证视图/语义分工」的证据链（终报告 §5.1 该分句的档案）

1. **State Ownership 五层表**（主档 §9，`project_panorama_master_2026-10-07.md:462-503`）：Canonical Model=唯一语义真理；Rope=存储实现；Tree-sitter=派生；**CM6/View 只许持有 cursor/selection/IME composition/短时 pending typing**，「不能成为 Document State 的最终所有者」【研】——这是分界线最早的一手表述（2026-10-07，先于六线研究）。
2. **范式 A/B spike**（主档 §14+`projects/spike/report.md`）：同 core-mock 双视图实测，B（CM6 装饰）0.758:0.715 胜——「视图借成熟件、core 自持 op-first」的实证判据【研】。
3. **Typora 性能归因**（主档 §4.2）：臃肿来自 Chromium 运行时/富文本 DOM/富媒体即时渲染/常驻服务，「不是 Markdown 解析本身」→ 视图层自研负 ROI 的直接佐证【研】。
4. **IME/剪贴板/Enter 决策清单**（`editing_semantics.md` §8）：按键只分派字符只放行/Enter 前强制 flush/IME 三段式封印/剪贴板三格式/光标=文本偏移+cursorDiff——语义层命令语义的成熟参照【研】。
5. **muya 互证**：块状态机范式的开源级独立重实现证明该分工非 Typora 特例【研】。
6. 主仓现实回证：CM6 桥为现役产品路径（语义核经 wasm 驱动视图）✅；反面例证＝mang-shell 在 Zed GPL 栈上重建第二视图层（终报告 §1.2-1）——恰好是违反此分界线的成本展示【主】。

### 4.3 增量启示（终报告未用的部分）

- Typora 大文档**四档降级梯**（35K/60K 字符、10K 节点、2MB 硬顶）+「核心思路不是无条件虚拟化，而是逐级牺牲功能换流畅」（主档 §4.1）——对 mang-shell buffer.rs（2,583 行自研）的规模策略有直接参照价值【研】。
- Typora 遥测收缩史（0.11.18 AWS Pinpoint 默认开→1.14.x 全部 stub+默认 false，`analysis/privacy_findings.md`）【研】：商业编辑器把「默认零遥测」当终态——mdcore 产品化遥测姿态的先例（见 ⑧-2）。

---

## ⑤ Acrobat 审计对 PDF 域的边界启示（D）

### 5.1 Acrobat 的能力面（作为标尺的一手登记）

- 审计覆盖面：安装器数据库级逆向（11 表/336 CustomAction/3,306 注册表写入/5,280 文件）、36 PE 能力矩阵（零网络导入/零注入 API）、ARM/AGS 更新与正版验证链（`acrobat-audit/README.md` 核心结论 1-6）【研】。
- PDF 能力面标尺（`pdf-line/02` §4 差异矩阵 19 行 × 9 引擎）【研】：Acrobat 商业独占域＝结构修复/xref 重建、增量保存保签名字节、preflight/fixup、Tagged 读取/编辑/autotag、XFA 动态表单、Acrobat JS 沙箱、PAdES LTV/吊销、OCR、文本编辑/语义重建、文档比较、PDF→Office、PDF/A 转换修复、AI 文档助手（2026 零对标区）。

### 5.2 mdc-pdf 现状 × Acrobat 差距谱系（分界线裁剪后）

| 能力域 | Acrobat | mdc-pdf 现状（主仓锚点） | 差距定性 |
|---|---|---|---|
| 渲染阅读 | ● | pdfium 引擎+LRU+渲染工人+看门狗（worker.rs:16-17）【主】 | **已闭合**（借力 BSD 引擎——视图/语义线的正确应用） |
| 标注增量写（保签名） | ● | writer_lopdf 前缀逐位保持+四层测试（audit notes 07 §2.1）【主】 | 引擎层已闭合；DocMDP/已签名档**检测缺失**（from_bytes 不看 /Sig，notes 07 §2.2）——P2 级差距 |
| 表单（AcroForm） | ● | forms.rs 读写面存在但**增量写为假**（P0-3 全量重写摧毁签名）【主】 | **声称-实现倒挂**（招标函填值目标场景直接踩雷） |
| 安全姿态 | ●（JS 沙箱等） | JS 不接/XFA 不启用/OpenAction 禁/OpenAction 族扫描（S1-S5）【主】 | 比 Acrobat 更保守（免沙箱负担）——**差异化而非差距** |
| 页组织/盖章/链接/图层/OCR | ● | organize/stamp/links/layers/ocr 模块在（旁路全量保存）【主】 | 功能壳在、保真与增量纪律未收口 |
| Preflight/PAdES LTV/XFA 动态/AI 助手 | ● | 零实现 | **明确不做**（v0 边界，`md-line/pdf_carrier_plan.md` §6）——与研究仓「十大无人区」评级一致（`pdf-line/02` §5：fixup ★★★/渲染 ★★/结构重建 ★★★ 等，**机会≠义务**）【研】 |
| 遥测面 | 重（AGS 指纹+多域上报，SKIP_GC=1 官方阻断开关）【研】 | 零遥测设计 | 差异化卖点（见 ⑧-2） |

**边界启示**：Acrobat 审计的贡献不是「抄 Acrobat 功能」，而是①安全策略转化（危险动作默认禁/对象族扫描/JS 不接——`pdf_carrier_plan.md` §3 明示来源 R1-R6）②**能力面的负空间清单**（哪些真空是「补不上/养不起」的护城河，`pdf-line/02` §0 四条结构性原因）③互通验证目标（v0 标准 Annot 子型+Acrobat 可读）。主仓越界点＝forms/organize/stamp 三个旁路写面把「不自研中间层」的纪律打了洞（audit notes 07 §1.2 偏差点）【主】。

---

## ⑥ 竞品生态现况速查（E，2026-11 视角）

> 格式：成熟度 / 许可 / 与 mdcore 的分界线判定。【我】= 笔记者知识；【研】/【主】= 仓内一手。

| 对象 | 成熟度 | 许可 | 与 mdcore 的分界线判定 |
|---|---|---|---|
| CodeMirror 6 | 成熟，6.x 持续维护（`web/package.json` ^6.36.5 锁定）【主】 | MIT | **视图层沿用**（MD 面唯一现役桥）——分界线正确侧 |
| ProseMirror | 成熟（CM6 同作者生态）【我】 | MIT | 参照件（step/op 模型与 mdcore op log 同族）；不引入 |
| Yjs | 成熟（协议十年冻结+RFC2119 文档化）【研】 | MIT | 协议规格净室参考；不依赖 JS 实现本体 |
| yrs | 活跃（月度发版/138 提交每年/413 万下载）但开放问题面在（#676 族）【研】 | MIT | **直接依赖**（collab/canvas 唯一接触点+隔离纪律）；上游已迁 y-crdt/y-crdt（maint-r1）【研】 |
| Automerge | automerge JS 2.0 长期 alpha（终报告 §5.2 反证）【主+我】 | MIT | 反证 yrs 为唯一生产级选择；不引入 |
| ONLYOFFICE | 成熟全家桶 | AGPL-3.0+商业 | 净室参考（输入限额三件套/页位图缓存）；直接依赖排除（ADR-C1）【研】 |
| PDF.js | 成熟查看器 | Apache-2.0 | **非编辑器/非平台**（三墙论，`pdf-line/02` §4.2）【研】；主仓 web PDF 视图实际消费中（pdfjs-dist ^6.4.299）【主】 |
| pdfium / lopdf | pdfium=Chrome 同款；lopdf 0.45 pin【主】 | BSD-3 式 / MIT | 引擎侧直接依赖（读写分离双引擎，ADR-P1）【研+主】 |
| tldraw | 活跃但 source-available（ECDSA 验签+熔断+水印+遥测回传部署 URL）【研】 | 商业+source-available | 架构净室参考（shape util/exec 范式）；依赖排除【研】 |
| Excalidraw | 成熟白板 | MIT | 净室参考（版本三元组/双修改通道）；React 绑定 headless 不可行【研】 |
| Zed / GPUI | app=GPL；**gpui crate=Apache-2.0；crates.io 0.2.2 停在 2025-10-22 快照、仓内同号持续开发**（`third_party/zed/PROVENANCE.md:128`）【主】 | 混合 | 视图基座 vendor 快照（32 crate/230,756 LOC，GPL 48,562）；迁移 crates.io 的可行性见 ⑦-F1【主】 |
| docx-rs | 0.4.22 后 4 个月未发版、单人仓、FORK_CHANGES 在档【研+主】 | MIT | 直接依赖（唯一成熟 Rust 件）+**局部重构**（panic 兜底 P0-2/上游跟踪） |
| docx4j / POI | 成熟（Java 生态）【我】 | Apache-2.0 | **oracle 用途**（终报告 §5.2：仅可作 oracle）——不进产品依赖 |
| calamine | 活跃（2026-09 仍有依赖维护提交）【研】 | MIT | 直接依赖（xlsx 读四面） |
| ironcalc_base | 0.9.1 pre-1.0、~470 函数、纯 Rust 零 C【主】 | MIT OR Apache-2.0 | 直接依赖（formula crate 包装；**研究仓未覆盖此选型**——见 ⑧-6） |
| rust_xlsxwriter | 活跃（constant_memory 在役修复 2026-10-01）【研】 | MIT/Apache 双许可 | 直接依赖（流式写） |
| LibreOffice | 成熟；26.2.7（still 终版）/26.8.1（fresh）双轨已裁【研】 | MPL-2.0 | 子进程+宿主预装（不 bundle，授权边界维持）；渲染管线外部件【研】 |
| Tauri 2.x | 活跃（终报告 §5.2 建议评估）【主+我】 | MIT/Apache | 宿主候选（web 资产复用路线）；**评估未启动**【主】 |

---

## ⑦ 终报告 §5 五分类的增量复核（F）

**复核结论：无材料支持「类别间移动」；有两条材料支持「类内细化/附加条件」，一条时间性勘误。**

### F1 「换成熟件：GPUI 基座 → crates.io gpui 0.2.2（Apache-2.0）」（`final-report.md:279`）——**建议附加可行性约束**

- PROVENANCE.md:126-130【主】的明确反论：crates.io 0.2.2 停在 2025-10-22 快照，ui/theme/text 针对仓内持续开发版编写，「对一年前的发布版编译必然大面积漂移；物理搬运锁定自洽快照是唯一可行路径」。
- 结构事实【主】：mang-shell 依赖闭包=gpui（Apache）+**ui/theme/text/rope 等 GPL 系 10 crate/48,562 LOC**——后者的 crates.io 化**不存在**（GPL 组件从未发布）。
- 【我】判定：终报告的「vendored→crates.io 迁移」只在「抛弃 GPL 组件层、纯 gpui 重写 UI」的前提下才成立——那实质是 §5.2 第四类的「视图层退回成熟件」路径，而非同路径换源。建议改写为：「**GPL 若裁闭源**：gpui 基座可保留 vendor（Apache 系 182,194 LOC 无传染），替换面收敛到 ui/theme/text/rope 48.5K；**GPL 若裁开源**：维持 vendor 快照+上游升级流程（PROVENANCE §流程 5 步），crates.io 0.2.2 不构成升级目标」。分类不变（仍在「应考虑成熟方案」），但迁移建议的可行性口径须修正。

### F2 「继续沿用：yrs」——**附升级触发勘误**

- 终报告称「0.28.1 已合并 GC UAF 根因修复」（:269）【主】；研究仓 maint-r1（2026-10-25）登记 crates.io 最新仍 0.28.0、修复 PR #682 open 未合并、上游仓已迁 y-crdt/y-crdt（`DECISIONS.md:36`）【研】；主仓 lock=0.28.0【主】。
- 【我】判定：0.28.1 的存在晚于研究仓登记窗口、无法在仓内验证——升级动作前必须核对 crates.io 实际版本与 #682 合入史；「跟踪升级」建议本身正确且被三方材料共同支持，分类不变。

### F3 「应暂缓：relay/mdbridge（冻结）」——**新增反向注脚**

- 【主】协作集成五轮（integ-r1..r5）已把 mdbridge/relay 建成「工程完成、产品零消费」状态（终报告 §8-Q6 平行宇宙）。研究仓 S2-S5 §2 的「重启条件」（真实多端需求/#676 修复）从未被触发——**暂缓分类与研究仓原始决策一致，且材料证明「冻结」是正确裁决而非损失**。

### F4 其余三类——无修改材料

- 「必须自研」六项：muya/Typora 互证（④）+ PDF 中间层真空（⑤）+ canvas 生态不可拆（2.4）全部**强化**该分类；
- 「继续沿用」其余件：office-e 一手核验+主仓 lock 对账（③-5）强化；
- 「局部重构」docx-rs：office-e §4 FORK_CHANGES 观察项+终报告 P0-2 一致强化；
- 附注（非分类修改）：web PDF 视图用 pdfjs-dist【主】与 mdc-pdf 引擎侧 pdfium 构成双渲染栈——属「视图层借力」的正确分界，但**双栈维护成本**应入 P2 登记面【我】。

---

## ⑧ 未用材料清单与影响（G）

| # | 未用材料（研究仓一手） | 对终报告/后续路线的影响 |
|---|---|---|
| 1 | **`workbench-line/宿主选型记录.md`**（13 场景双原型基准+七维评估+未测量面五条诚实边界）【研】 | 终报告 T7「双轨收敛+宿主裁决」缺这份**唯一有实测数字的宿主对照件**：web 路线 60Hz 预算全过+IME 28/28 证据 vs egui 表格帧 31.6ms 超预算。双轨裁决会（T7）应以其为 A 案（web/Tauri 化）性能底账；mang-shell GPUI 案从未跑过同口径基准——**裁决输入不对称**是当前最大的证据缺口 |
| 2 | **Typora 遥测/隐私分析**（`typora-re-analysis/analysis/privacy_findings.md`：0.11.18 默认开→1.x 全 stub）+ **Acrobat 遥测定级**（`acrobat-audit/analysis/04`：ARM/AGS 中危+SKIP_GC 开关）【研】 | 产品化遥测姿态的一手先例链：商业对手（Typora）把默认零遥测当终态、 heavyweight（Acrobat）靠 FeatureLockdown 治理。mdcore 若坚持零遥测=差异化卖点；若引入任何遥测，Acrobat AGS 的「用户可阻断开关」形态是合规底线参照 |
| 3 | **`DECISION_SHEET.md` 批次 3 LO 双轨 C 方案四项落地件**（版本检测入日志/支持范围声明/双轨验证/季度复核）【研】 | 终报告 §6.2「LO 硬门」建议缺用户已裁的版本治理框架（③-6 证明零落地）——CI 化时应一并补 `soffice --version` 探测与 `>=25.2 && <27` 范围警告，而非仅装 soffice 转硬门 |
| 4 | **`pdf-line/02` 十大无人区评级**（fixup ★★★/结构重建 ★★★/CJK 全链 ★★★ 等）【研】 | mdc-pdf 路线图的**差异化机会清单**未被终报告消费：若「PDF 产物字节级非确定」与「office→PDF 依赖 LO」长期成立，「结构语义重建（PDF→Office）」与「只修不判的 PDF/A fixup（veraPDF 当 oracle）」是两条已评级、错开 pdfcraft 峰值的蓝海——属 P3+ 排期面 |
| 5 | **Typora 决策清单与大文档降级梯**（`editing_semantics.md` §8 五条+`hybrid_rendering.md` 四档降级）【研】 | mang-shell 视图层（buffer/display_map/multi_cursor 全自研 4,004 行）的成熟参照件：IME 三段式封印/Enter 前 flush/光标=偏移+cursorDiff/规模降级——直接映射终报告「Canvas 五课」同族的「编辑器视图课」 |
| 6 | **ironcalc 选型证据缺口（反向登记）** | formula crate 选 ironcalc_base 0.9.1（issue #110 调研在主仓内）【主】——研究仓 office-c 只深读了 HyperFormula（GPL 排除）与 office-e 五件，**从未核验 ironcalc**（许可/闭包/函数覆盖证据链不在研究仓）。建议补一轮净室核验入档（闭包许可+470 函数对照面+pre-1.0 升级风险），否则该直接依赖缺研究级证据档案 |
| 7 | **`compliance_behavior/AUDIT_24_BEHAVIORAL.md` 五维审计矩阵**（名字/字面量/结构/常量/反编译产物）【研】 | GPL 商业化裁决（T7 三选一）的合规工具箱：若选「闭源+替换 GPL 面」，替换后应复跑同矩阵证明表达层独立；若选开源，矩阵转 SBOM 对账工具。另含法律框架精确定义（「免责的从来不是改名，是独立创作」） |
| 8 | **yrs-g 上游跟踪+两项 API 请求候选**（Update 内容类枚举/ClientID release 校验）【研】 | yrs 升级窗口（#682 合并后）的工程预备：升级=跳过 skip_gc 不变量解除评估+两项 API 请求呈递时机——研究仓已备好跟踪纪律三条，主仓升级执行时应循此清单 |
| 9 | **`pdf-line/06` §4 AI 调用面五方水位表**（tldraw MCP 四件套/Penpot 插件同门写路径）【研】 | mang-shell「无 AI 面板（MCP 暴露）」预留位的设计输入已在档：宽容输入 proxy/代码即操作+轻量沙箱/结构化状态回推/checkpoint 寻址四件套——若 mdcore 启 MCP 面，此为最短路径蓝图，且窗口期判定（roadmap 已列交付未发生）需 2026-11 复核刷新 |
| 10 | **`md-line/boundary-audit/` p4 C/K 约束清单**（revision 非唯一序号/单写者模型/事件单消费者）【研】 | 协作桥 PoC（终报告 P2 项）的前置事实基座：六项能力缺失+三相邻机制判定已 100% 证据化——PoC 设计应从这份约束清单出发而非从零调研 |

---

## ⑨ 结论

1. **终报告 §5 的两条结论级判断全部获得一手档案支撑**：「视图/语义分界线」的最早表述在 Typora 主档 §9（2026-10-07），经范式 spike 实证、六线 30+ 竞品对标、PDF 哑铃形生态图三支汇流；「自研负 ROI 在视图层」有 Typora 性能归因+宿主选型基准双证，「差异化在语义层」有 muya/Typora/Delta 互证+PDF 中间层真空+canvas 生态不可拆三证。
2. **对账发现的主漂移**：宿主（tao+wry→web 主案→GPUI vendor，无基准对照）与协作产品化（可选→正式交付→平行宇宙）；已裁决未落地两项（LO 双轨治理四件、R-1 接线）；版本漂移一项（zip 8.6 vs ADR ≥9.x，已知情补偿）。
3. **五分类复核**：无类别间移动；GPUI crates.io 迁移建议需附 PROVENANCE 反论的可行性约束；yrs 升级触发需勘误核对。
4. **最值得回收的未用材料**：宿主选型记录（T7 裁决输入）、十大无人区评级（PDF 差异化路线）、Typora 决策清单（mang-shell 视图课）、遥测先例链（产品化姿态）。
5. **补档建议**：ironcalc 净室核验、GPUI 案补跑同口径宿主基准——两项是「研究仓证据链未覆盖主仓现实」的仅存缺口。

---

## 附：本笔记引用锚点索引（抽样）

- 研究仓：`cleanroom-analysis/{README.md, DECISIONS.md, DECISION_SHEET.md, DECISION_GROUPS.md, HANDOFF.md, worklog.md}`；`md-line/{STATUS.md, pdf_carrier_plan.md, pdf-a/ADR-P1-pdf-engine-selection.md, boundary-audit/REPORT.md, reuse-impl/}`；`office-line/{ADR-O1-office-io-selection.md, office-e_Rust生态选型pass.md, office-c_HyperFormula单件深读报告.md}`；`pdf-line/{02_开源PDF生态缺口与差异项深度分析.md, 06_办公与画布线R2综合与裁决建议.md}`；`collab-line/{yrs-a_yrs评估pass.md, 协作与Canvas正式交付报告.md}`；`workbench-line/{宿主选型记录.md, S2-S5收口与阶段决策记录.md}`；`acrobat-audit/{README.md, analysis/04-风险定级与处置建议.md}`。
- Typora 仓：`project_panorama_master_2026-10-07.md`（§4/§5/§9/§10/§14）；`analysis/deep_dive/{editing_semantics.md, hybrid_rendering.md, input_hotpath.md, privacy_findings.md}`；`projects/marktext/recon/README.md`；`projects/spike/report.md`；`compliance_behavior/AUDIT_24_BEHAVIORAL.md`。
- 主仓：`README.md`；`third_party/zed/PROVENANCE.md`；`audit/final-report.md`；`audit/notes/{07-pdf.md, 09-collab-workbench-evidence.md, 10-mangshell.md}`；`crates/pdf/Cargo.toml`；`crates/collab/Cargo.toml`；`crates/office-io/src/docx.rs`；`crates/mdcore/src/history.rs`；`crates/pdf/tests/pdfb_skeleton.rs`；`crates/formula/Cargo.toml`；`web/package.json`；`web/src/main.ts`；`mang-shell/Cargo.toml`；`Cargo.lock`。
