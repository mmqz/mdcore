# 审计笔记 27：研究仓 cleanroom-analysis 全量审计（mdcore 决策谱系验证）

> Task ID：R5-d ｜ 日期：2026-11-05 ｜ 性质：只读审计（源码零改动，本文件为唯一新增产出）。
> 审计对象：`/home/z/my-project/mang-research-archive/cleanroom-analysis`（归档快照；外层仓
> git log 头部 = `e625568`，其中 `88d1206`「archive: 并入 cleanroom-analysis —— mdcore 的
> 设计决策来源」即本子仓的归档动作——原独立仓 main 引用（HANDOFF 内 `141c482`/`b3d5237`）
> 在归档仓历史中不可直接验证，内容以快照为准）。
> mdcore 对照基线：本仓 `mdcore-audit` main@`96596d7`（2026-11-04），既有两轮审计
> `audit/notes/01-23-*.md` + `audit/final-report.md` 全部可作对照证据引用。
> 统计口径：find+wc 实测（2026-11-05）；决策文档四份 100% 全文通读；py 全部 py_compile 通过。

## 0. 一分钟总览

- cleanroom-analysis = mdcore 的**真实设计决策来源**【已确认】：DECISIONS.md（20 项三分类登记）
  → DECISION_GROUPS.md（17 项→5 依赖组四件套）→ DECISION_SHEET.md（4 批次批注，
  2026-10-28 用户全批）→ mdcore 提交链（批次 2 = `b15c0ab→c4856f7` 精确可对账）。
  README.md:12 的「设计决策来源」声明与内容物一致，非自封。
- 六线划分（md/office/pdf/canvas/collab/workbench）在 mdcore 落为 16 crate 家族，谱系成立；
  **但研究仓的记录终点 = 2026-10-27（mdcore `c57b6ce`）**，此后 mdcore 的主线转向
  （mang-shell GPUI + third_party/zed vendor）**零记录**——rg 全仓「mang-shell|GPUI」零命中
  （含 json 外全部文本文件）。
- 重大决策对账（20 条）：**已实现 12 / 部分实现 4 / 偏离 2 / 未实现 1 / 已放弃 1**。
  谱系内的裁决兑现率极高（collab/office 选型/锚点写面全兑现）；偏离全部发生在
  「裁决之后、产品化之前」的窗口（宿主、渲染管线、undo 接线）。
- 最尖锐冲突【已确认】：md-line/STATUS.md:24-26「ADR-C1 闭源最严姿态：GPL/AGPL 零容忍」
  vs mdcore HEAD `third_party/zed` 10 个 GPL crate ≈48.5K LOC 编入实际主线 GUI（final-report.md
  §1.1/§1.2）——研究仓立下的净室红线被产品仓的后续演进正面打破。

---

## 1. 全量清单与统计（find+wc 实测）

### 1.1 顶层结构

| 条目 | 性质 | 规模实测 |
|---|---|---|
| DECISIONS.md | 裁决包三分类单一登记表（20 项） | 89 行 |
| DECISION_GROUPS.md | 17 项→5 依赖组映射（每组默认/备选/后果/不可推断四件套） | 358 行 |
| DECISION_SHEET.md | 4 批次单页批注版（含 2026-10-28 用户批注原文） | 189 行 |
| HANDOFF.md | 新会话接手指南（§6 含 2026-10-08~10-27 全部速览，追加式） | 894 行 |
| README.md | 总索引 | 48 行 |
| worklog.md | 全程工作日志（追加式，Task ID 1…m2） | 7,203 行 |
| md-line/ | MD 线结论层（boundary-audit 八件套 + pdf-a ADR + reuse-impl） | 23 文件（md 16/2,383 行；rs 4/488 行） |
| office-line/ | 办公线（研究报告 + ADR-O1 + SECURITY_POLICY v1.2 + spike×2 + 三开发轮档案） | 47 文件（md 23/7,016 行；rs 11/3,242 行；py 7/2,636 行） |
| pdf-line/ | L2 PdfCraft 净室分析四篇 + 05 规划 + 06 R2 综合 | 6 文件（md 6/2,226 行） |
| canvas-line/ | 画布线七篇深读/pass | 7 文件（md 7/3,467 行） |
| collab-line/ | 协作线（collab-a/affine-a/yrs-a…g + S0 + 交付报告 + upstream 草案） | 47 文件（md 14/2,902 行；rs 26/7,861 行；json 2） |
| workbench-line/ | 工作台线（六阶段计划 + 宿主选型 + 基准/验收报告 + 双原型） | 17 文件（md 5/747 行；rs 4/1,133 行；py 2/205 行；json 4） |
| acrobat-audit/ | L1 Acrobat 安装器审计（6 分析 + 48 json 证据 + 8 py 工具 + source 哈希登记） | 70 文件（md 8/713 行；py 8/1,173 行） |
| agents-md-audit/ | L0 AGENTS.md 审计（8 分析 + 5 证据 json + 5 py 工具 + 源件） | 22 文件（md 12/1,571 行；py 5/607 行） |
| tools/ | push_to_github.sh（token 零落盘推送模板） | 1 文件 |

### 1.2 类型总计（全仓 247 文件）

| 类型 | 文件数 | LOC | 备注 |
|---|---|---|---|
| md | 97 | 29,806 | 与 README「约 97 md」一致 |
| json | 59 | —（数据证据） | 48 个在 acrobat-audit/evidence（msi-tables/pe-capability/外部研究），5 个 agents-md-audit/evidence，4 个 workbench proto-host/results，2 个 collab spike |
| rs | 45 | 12,724 | collab-line 26（7,861，p1-p20 探针 + adapter 原型 + 5 测试套）/ office-line 11（3,242，spike）/ workbench-line 4（1,133，proto-native）/ md-line 4（488，pdf-a spike） |
| py | 22 | 4,621 | acrobat 8（1,173）/ agents 5（607）/ office spike-ro2 7（2,636）/ workbench proto-host 2（205） |
| toml | 5 | — | 4 个 spike Cargo.toml + 1 根 workspace（collab spike） |
| 其他 | 17 | — | 3 sh / 3 Cargo.lock / 5 .gitignore / ps1 / mjs / patch / docx / ini×2 / txt / 无扩展名 1 |

Rust/Python 原型分布结论：**45 rs 的 58% 集中在 collab-line（适配层原型 + 20 个编号探针）**，
22 py 的 81% 在两个审计子仓（取证工具链）与 office spike-ro2（LO 转换回归）。
六线中 pdf-line / canvas-line **零代码**（纯研究报告线）。

---

## 2. 决策清单表（四份顶层文档 100% 通读）

### 2.1 决策体系结构（四件套的分工）

| 文档 | 职能 | 条目数 |
|---|---|---|
| DECISIONS.md | 全线剩余门控项单一登记表（类别 1 安全实现 / 类别 2 完备提案 / 类别 3 精确决策问题） | 20 项（类别 1 恰 1 项且已实施=STATUS §7-6 陈旧修正；类别 2 两件；类别 3 十七项，#4 含三子裁决） |
| DECISION_GROUPS.md | 类别 3 十七项 → G1 载体与边界 / G2 撤销与锚点身份 / G3 栈与资源上限 / G4 Office 锚点写 API / G5 渲染器部署治理 | 5 组 17 行映射（DECISION_GROUPS.md:19-38） |
| DECISION_SHEET.md | 批次 1（D1 解锁包）/批次 2（锚点，已执行）/批次 3（渲染）/批次 4（低优先八项） | 4 批次；批注落款均为 2026-10-28 用户裁决（DECISION_SHEET.md:36/105/149） |
| HANDOFF.md | 阶段状态 + §6 下一步（多版本速览追加式保留） | 速览 12+ 层，覆盖 2026-10-08~10-27 |

**总条目统计**：DECISIONS 20 项（其中 #4 拆三子裁决 = 22 个决策点）；DECISION_GROUPS 17 项
5 组；DECISION_SHEET 4 批次覆盖 13 个决策点 + 维持类 6 项；另线内裁决（ADR-P1、ADR-O1、
xlsx-r2 五项裁决回放、office-g 裁决③、S1 三项选型追认、六阶段计划 governing 指令、
宿主选型定谳）约 15 项——**全仓可数决策总量 ≈ 37 项 + 若干「维持现状」默认裁决**。

### 2.2 重大决策清单（已裁决定谳项，含裁决与证据锚点）

| ID | 主题 | 裁决（含备选） | 证据锚点 |
|---|---|---|---|
| A1 | PDF 引擎选型 | pdfium(pdfium-render)+lopdf 双引擎；mupdf AGPL 排除；non-V8 构建断言 | md-line/pdf-a/ADR-P1-pdf-engine-selection.md:10-23（已裁决 2026-10-10） |
| A2 | Office IO 选型 | calamine 读 / rust_xlsxwriter 写 / docx-rs 读写 / zip 走 zip2 线；umya 按需不预进 | office-line/ADR-O1-office-io-selection.md:11-17（建议稿，实施落地） |
| A3 | 消毒面 | mdc-office 消毒六面（纯 std 自锁）+ 双 crate 方案 A（独立 mdc-office-io） | office-line/xlsx-r1/REPORT.md §2（方案 A 建议）；SECURITY_POLICY.md v1.2 |
| A4 | D1① 协同载体 | **XmlFragment**（Array-of-Map 备选被 p11 锚点双保持失败否决） | DECISION_SHEET.md:36-38（2026-10-28 批准） |
| A5 | D1② B3.2 撤销栈 | **知情采纳**：会话本地 + per-origin 分栈 + 栈永不进 update | DECISION_SHEET.md:40 |
| A6 | D1③/G3① 栈上限 | **维持无限**，附三交付：undo/redo_depth 观测、栈>512MB 告警、clear_all 手动入口（**不设自动上限**） | DECISION_SHEET.md:42-52 |
| A7 | 批次 2 锚点写面 | **Option A**：RunSpec.anchor 加性扩 + anchor_link + bookmark_paragraph + InvalidInput 变体，Phase 1+2 全批（已合入 mdcore `b15c0ab→c4856f7`） | DECISION_SHEET.md:54-66 |
| A8 | 批次 3 渲染基线 | **C 方案**：钉 26.2.7 + 同步验证 26.8 + soffice --version 运行时检测 + 支持范围 >=25.2 && <27 + 季度复核 | DECISION_SHEET.md:105-134 |
| A9 | R-1 undo_coalesce_ms | **A 接线**（History 加字段 + Core setter + wasm 导出 + main.ts 活应用，默认零行为变化） | DECISION_SHEET.md:140/155 |
| A10 | R-2 本地撞号 | **c 维持现状**（协同域已被 R4.4 消解） | DECISION_SHEET.md:156 |
| A11 | D2 双态 | **暂不立项**（画布需求未激活） | DECISION_SHEET.md:157 |
| A12 | D3 生成态 | **A 主 B 辅**（A=进程内声明式 + 消毒三闸；B=模板保真触发） | DECISION_SHEET.md:157；pdf-line/06 §5.2 |
| A13 | ① code 高亮 | **接受永久降级**（docx-rs 库层不可达） | DECISION_SHEET.md:143/158 |
| A14 | ② MarkSpan URL | **维持现状**（mdcore 公开模型变更另批） | DECISION_SHEET.md:144/159 |
| A15 | ④ 自定义编号 / ⑦ 富文本 marks | **维持消费者触发 / 维持模型不扩**（载体即绪、模型另批） | DECISION_SHEET.md:158 |
| A16 | #19 上游协同 yrs | 维持跟踪：skip_gc 全编队不变量 + #676 open 不升生产内核 + 两项 API 请求候选 + 修复 PR #682 在途 | DECISION_SHEET.md:147；DECISIONS.md:36 |
| A17 | p20 捕获窗缺陷 | G2 裁 **A：捕获窗改 0**（硬编码非配置面，mdcore COALESCE_MS 零接触） | HANDOFF.md:695-696（s0-t2 已执行） |
| A18 | S1 sidecar | **JSON 非 SQLite**（用户追认）+ FNV-1a 指纹 + PDF 接入后置 S4 | workbench-line/S1-证据系统MVP验收报告.md §3 |
| A19 | 宿主选型 | **主宿主 A：web(CM6)+wry/tao 壳**；egui 原型 B 保留降级路径 | workbench-line/宿主选型记录.md:14-21（2026-10-22 定谳） |
| A20 | R-O2 渲染路线 | C 路线（LO headless→PDF→mdc-pdf）spike 全过进原型阶段；渲染产品化 = 批次 3 | office-line/office-f_docx渲染级读侧评估pass.md §0（用户裁 C 路线已发生） |

维持/延后类（非重大但登记）：R-5 基准延后（前置 profile 口径缺失）、R-6 真机排期、
fuzz 战役择机（S6 表）、LO 26.8 复跑环境就绪即跑、D2 双态、R-4 无事可做维持。

---

## 3. 六线逐区审计（结论/原型/质量/入档路径）

### 3.1 md-line（16 md + 4 rs/488 行）

- **结论**：mdcore 引擎线结论层——STATUS.md 登记 R24 编排/PDF 载体五轮全完成、双门合规
  终态（四路审计全 CLEAN，STATUS.md:22-27）；boundary-audit（P1-P6+REPORT）给出 12 项复用
  单元三分类（5 直接复用/5 最小适配/2 域外），核心结论「mdc-pdf 与 mdcore 零编译依赖，
  唯三耦合点」【已确认，与本仓 01-structure.md 一致】；reuse-impl R1-R6 六项零机械推进项
  （R1-R6逐项处理.md §0：R-1/R-2/R-3 停-产品语义、R-4 无事可做、R-5/R-6 维持延后）。
- **原型**：pdf-a/spike 4 rs/488 行（main/lib/fixtures/formfill_probe）——ADR-P1 三探针
  （打开/渲染/增量写+互通）实测数据落 ADR §spike 表；质量定位 = 证据探针而非产品。
- **入决策路径**：STATUS §7 → DECISIONS #1/#2/#3/#15/#16/#20（陈旧修正即类别 1 唯一项）。

### 3.2 office-line（23 md + 11 rs/3,242 行 + 7 py/2,636 行）

- **结论**：量最大的研究线。四深读（Univer「快照即模型」/ONLYOFFICE「内部二进制真相源」
  /HyperFormula「gridless 计算内核」/office-d JS 写侧四件套）+ office-e 五仓一手核验 →
  ADR-O1 六面选型 → 开发五轮（office-impl → xlsx-r1/r2 → docx-r1/r2 → 消费面 t16-t22 →
  Round4/5）全部 REPORT 落档；FIDELITY.md 24 项五态保真矩阵为「单一真源」纪律。
- **原型**：spike（rs 3,242 行：corpus/lie86/sparse_bomb/style_probe 等探针 + io/lib/main +
  url_policy）与 spike-ro2（py 7 件：lo_convert_spike×2 + regress.py 确定性 CI 套件 +
  render_guard.py 三合一守护器 + rss_wrapper + fixtures）。**regress.py 质量突出**：
  14→22 用例/204 断言、夹具字节级确定性门（sha256 台账）、超时杀进程组零残留、
  /usr/bin/time 不存在的替代 RSS 口径（REGRESSION.md §0-§5.2）——证据文化完整。
- **入决策路径**：ADR-O1 → SECURITY_POLICY v1.2（威胁 T1-T7×四层防线×33 项限额表）→
  DECISIONS #7/#8/#9-#12 → 批次 2/3/4。

### 3.3 pdf-line（6 md/2,226 行，零代码）

- **结论**：L2 PdfCraft 净室分析闭环（01 架构/02 生态缺口「哑铃形」/03 敌意输入边界/
  04 收官「净室开发成立」）+ 05 规划（16 候选/11 维方法论）+ 06 R2 综合（四条主线裁决建议：
  CRDT 统一/渲染光谱/双态/生成态）。03 篇的「限额五层纵深/三行为学」是 mdc-pdf 与
  office SECURITY_POLICY 的直接输入（md-line/STATUS.md:58-60 明示衔接）。
- **入决策路径**：06 §8.1 D1/D2/D3 终裁问题留用户 → yrs 线证据链 → 批次 1/4。

### 3.4 canvas-line（7 md/3,467 行，零代码）

- **结论**：Excalidraw（元素级 LWW 薄协同）/tldraw（source-available 熔断）/drawio-maxGraph
  （布局算法族纯算法可移植）/ComfyUI（GPL → 进程外协议集成唯一路线）/canvas-e 三渲染基座
  （三仓 MIT 定谳）/InvokeAI UX/Penpot（服务器定序）七篇；对 mdcore 的直接产出 =
  「画布独立领域模型 + 键级 LWW + 证据契约」设计输入【已确认：crates/canvas 落地形态一致】。
- **入决策路径**：D2 双态暂不立项（06 §8.3 触发条件表）→ 批次 4。

### 3.5 collab-line（14 md + 26 rs/7,861 行）

- **结论**：研究仓最重证据链——yrs-a 评估（MIT 定谳/双语言 wire 互证/两大内存发现）→
  yrs-b 四项边界（GC/UAF 隔离·事务粒度·撤销栈归属·稳定锚点）→ yrs-c/d 适配层原型与加固
  （matrix 25+hostile 13+ASan 零报告）→ yrs-e/f 映射矩阵就绪包（p15/p16/p17 ALL PASS）→
  S0 一页资源清单 → 协作与 Canvas 正式交付报告（mdc-collab/ mdc-canvas 落 mdcore）。
- **原型质量**【已确认】：adapter 原型 crate「公开 API 零 yrs 类型、crate 边界=可撤回边界」
  （adapter/src/lib.rs:1-23 模块文档）；20 个编号探针（p1_basics…p20_undo_window_repro）
  每个对应一个证据问题；上游缺陷定谳链（#676 ASan 复现/p20 四配置矩阵/PR #682 在途）
  全程「不宣称已修复」纪律。此原型后升格为产品 crates/collab（见 §4 对账）。
- **入决策路径**：D1 三裁决（批次 1）→ G2/G3 联动 → mdc-collab 实施。

### 3.6 workbench-line（5 md + 4 rs/1,133 行 + 2 py/205 行）

- **结论**：用户 governing 指令落档（六阶段计划）→ 阶段 1 四类性能基线 → 宿主选型
  双原型对照（13 场景双侧对称实测，帧预算 web 全落 60Hz vs 原生软渲染超预算）→
  S1 证据系统 MVP（12 门禁逐条映射测试）→ S2 写入安全 + S3-S5 决策记录。
- **原型**：proto-native（egui 四面板 + x11rb XTEST 真实输入注入，1,133 行）与
  proto-host 两个 bench py + results json 四件（实测数字在档）。质量 = 对照证据件，
  结果以「未测量面诚实登记」（宿主选型记录.md：wry 壳沙箱不可编译=阶段 5 前置项）。
- **入决策路径**：宿主定谳 A → 批次 4 前端裁决链闭环。

---

## 4. 决策→mdcore 对账表（本笔记核心）

对照基线：mdcore-audit main@96596d7（2026-11-04）。分布：**已实现 12 / 部分实现 4 /
偏离 2 / 未实现 1 / 已放弃 1（共 20 条）**。

| # | 决策ID | 决策内容（一行） | mdcore 现状 | mdcore 侧证据 |
|---|---|---|---|---|
| 1 | A1 | pdfium+lopdf 双引擎（pin 0.9.4/0.45.0） | **已实现** | crates/pdf/Cargo.toml:10-19（engine-pdfium pdfium-render=0.9.4 / engine-lopdf lopdf=0.45.0，版本 pin 注明 ADR-P1）；07-pdf.md 笔记核证 |
| 2 | A2 | calamine/rust_xlsxwriter/docx-rs/zip2 选型 | **已实现** | crates/office-io/Cargo.toml:32-33（calamine 0.36 + docx-rs 0.4.22 注记）；Cargo.lock 钉版；**zip 实钉 8.6 而非 ADR 的 ≥9.x**——偏差有 DEPS.md「8.6/9.0 并存登记」在档依据【设计偏好】 |
| 3 | A3 | 消毒六面 + 双 crate 方案 A | **部分实现**（crate 落地、**六面零产品接线**） | crates/office 905 LOC/78 test 存在；office-io Cargo.toml 无 mdc-office 依赖边（仅注释「写入前必须过消毒链」）；final-report.md §1.2「消费倒挂」+ P0-5（TSV 注入防护存在未接线）；05-docx.md §Q6 |
| 4 | A4 | 协同载体 XmlFragment | **已实现** | crates/collab/audit.rs（XmlFragmentRef 视图 + ForeignRootType 校验）、ops.rs/identity.rs 全 XmlFragment；Cargo.toml 注「载体：XmlFragment 根 doc（yrs-e 映射矩阵定谳形状）」；09-collab.md 笔记 §1.1 |
| 5 | A5 | B3.2 会话本地 + per-origin 分栈 + 栈不进 update | **已实现** | crates/collab/src/session.rs:200-204（双 UndoManager per Human/Ai）、:470（权威快照=update 编码）；09-collab.md 笔记 |
| 6 | A6 | 栈维持无限 + 三交付（观测/告警/手动 clear_all，**不设自动上限**） | **部分实现/语义偏离** | undo_depth/redo_depth ✓（session.rs:364/372）；但落地为**软上限自动触发 clear_all**（session.rs:379-397，undo_cleared 显式回执）——「自动逐出」恰是裁决文明确排除项（「不设自动上限——那会伤体验」），且未见「栈占用>512MB 告警」（软上限计量=栈深度非内存字节） |
| 7 | A7 | 锚点写面 Option A（Phase 1+2 全批） | **已实现** | crates/office-io/src/docx.rs:3877（RunSpec.anchor 字段）、:3981（anchor_link 构造器）、:4487（bookmark_paragraph）；读4/写4 对称（05-docx.md 笔记）；TOC 面消费 docx.rs:6801/7286 |
| 8 | A8 | LO 基线钉 26.2.7 + 运行时版本检测 + 支持范围声明 | **未实现** | mdcore 仓内零 LO 渲染管线：web PDF 面 = pdf.js + mdc-pdf-wasm（路线 B）；docx 消费走 office-io 读面；soffice 仅出现在 mang-shell 软门测试（mang-shell/tests/docx_bid_e2e.rs:316，缺件 ENV_MISSING 跳过）与调研脚本（docs/field-code-semantics.md:46）；无 --version 探测/无支持范围声明面 |
| 9 | A9 | R-1 undo_coalesce_ms A 接线 | **未实现**（已裁决未执行） | audit/final-report.md:214「undo_coalesce_ms 假旋钮（引擎恒用常量 1000）」；rg coalesce 在 crates/mdcore/src 仅命中注释与常量；11-quality.md:207（双轨合并窗分叉实证） |
| 10 | A10 | R-2 撞号维持现状 c | **维持现状**（裁决兑现）但风险兑现为 P0 | mdcore-audit P0-1「save→open 身份重编号 + undo 物化 ID 撞号」仍为最重 P0（final-report.md §1.3）——维持现状的代价在产品仓被实证 |
| 11 | A11 | D2 双态暂不立项 | **已实现**（维持） | 无双态投影；crates/canvas = 独立领域模型（Map 键身份/LWW），与 06 §3 双态路线一致地未走 |
| 12 | A12 | D3 生成态 A 主（四件套直拼 + 消毒三闸） | **部分实现/形态偏离** | 无 dsh 集成、无进程内声明式生成态 API；AI 调用面实际形态 = mang-shell MCP server（sdk/mcp/README.md：MCP 宿主⇄unix socket⇄mang-shell）+ mdcore-wasm 协同命令流（Source::Ai 溯源）；消毒三闸未见对应（消毒六面且未接线，见 #3） |
| 13 | A13 | ① code 高亮永久降级 | **部分实现**（降级面被部分消解） | mdcore-wasm/src/docx_export.rs:42：#15（2026-10-27 dev-A..J）行内 marks 消费落地——源文本层 parse_inline_marks 派生五位格式 run 化（`code`→Consolas 等宽 ✓）；fence 语言元数据仍降级（#15 Option B：模型/投影/同步协议零触碰） |
| 14 | A14 | ② MarkSpan URL 维持现状 | **已实现**（严格维持） | crates/mdcore/src/tree.rs:41-45（MarkSpan{start,end,kind} 无 URL）；docx_export.rs:14 自认「模型 MarkSpan 无 URL 载荷——mdcore MD 域上游缺口」，link kind→纯文本 |
| 15 | A15 | ④ 编号维持触发 / ⑦ marks 模型不扩 | **已实现**（维持且消费面演进） | ④：office-io numbering ListKind 两态维持（docx-r1/FIDELITY.md #14 写入降级可诊断）；⑦：MarkSpan 未扩 + 导出面派生（#13 证据）+ 协同侧 D-2 marks 归一化重建已落地（crates/collab/src/ops.rs:502-631）——「模型裁决落地日载体即绪」的预测被验证 |
| 16 | A16 | #19 上游协同（skip_gc/#676/钉 0.28） | **已实现** | session.rs:193（skip_gc 无条件 B1.1）、:105（SessionOptions 刻意不含该开关）；yrs 0.28 钉版（Cargo.lock）；**上游 issue 两份 ready-to-file 至今未提交**（collab-line/upstream/README.md:12-15 呈用户裁决）【高度怀疑：未提交状态为文档自述，无提交回执】 |
| 17 | A17 | p20 捕获窗改 0 | **已实现** | session.rs:256（capture_timeout_millis: 0 硬编码，注释引 p20）；HANDOFF.md:695-696（s0-t2 执行） |
| 18 | A18 | S1 sidecar JSON 非 SQLite | **已实现** | crates/evidence/src/lib.rs:34/62-73（sidecar v1：JSON、原子写、模式版本校验）；09-collab.md 笔记「evidence 被低估的资产」 |
| 19 | A19 | 宿主 A：web+wry/tao 壳 | **偏离**（重大） | shell-native 蓝图停滞（568 LOC，10-26 后零提交，01-structure.md §1.1）；实际主线 = mang-shell GPUI（33.8K LOC、106 commits/两周、**对 mdcore 核零依赖**，01-structure.md §1「GPUI 线（实际主线 GUI）」）；web CM6 桥为现役产品路径——两轨并存且互不相通（final-report.md §1.2 问题 1） |
| 20 | A20 | R-O2 C 路线（LO→PDF 渲染级读侧） | **已放弃/被替代** | HANDOFF.md:321-324（2026-10-23 p5-t4 定谳路线 B：pdf.js 渲染 + mdc-pdf lopdf wasm 写回，「渲染分离、真相不分离」；A pdfium-wasm 否决/C 壳侧渲染=升级位）；docx 渲染级读侧最终未走 LO 管线，R-O2 22 用例回归套件留在研究仓 spike-ro2（mdcore 仓内无对应资产） |

### 4.1 对账补充事实（重要）

- **研究仓时间盲区**【已确认】：HANDOFF 终态速览 = 2026-10-27 m2（mdcore `c57b6ce`、
  「六面复验完整收官…全绿无死角」）；mdcore-audit 基线 = 2026-11-04 `96596d7`。中间
  8 天 mdcore 的最大演进（mang-shell GPUI 主线 + third_party/zed vendor + docx_edit 字节
  手术面）**在研究仓零记录**（rg「mang-shell|GPUI」全仓 0 命中）。决策谱系对
  2026-10-27 后的 mdcore 不具解释力。
- **GPL 红线冲突**【已确认】：md-line/STATUS.md:24-26（ADR-C1「GPL/AGPL 零容忍、上游特有
  命名零容忍」）vs mdcore-audit final-report.md §1.2 问题 2（GPL 10 crate 48.5K LOC 编入
  mang-shell 主线 + 仓根 LICENSE=GPL-3.0）。研究仓视角的「净室纪律」在其记录终点之后
  被产品仓正面打破——这不是研究仓的失职（其记录诚实止于其所见），而是**决策谱系失去
  持续维护后的必然漂移**。
- **docx-r1 勘误的适用域被收窄**【已确认】：docx-r1/REPORT.md §1（「.rels 词汇表敌意面：
  初判 unwrap panic 经实证勘误——from_str catch-all 永不 Err」）仅对 rels 层成立；
  mdcore-audit P0-2 实测 docx-rs 0.4.22 主体读路径在 Word365 真实语料 4 处 panic +
  22/23 属性族解析失败（final-report.md §1.3，05-docx.md §0）。研究仓「引擎容忍非 panic」
  的 comforting 结论不可外推至整个读路径。
- **限额/安全策略基本兑现**【已确认】：SECURITY_POLICY v1.2 的 33 项限额表在 office-io
  落地（门 0 输入字节/门 1 谎言头/门 2 解压炸弹/门 3 稠密包围盒——05-docx.md、
  06-xlsx-formula.md 笔记核证）；稀疏包围盒炸弹闭环（1588B→550GB 探针→typed 拒）。

---

## 5. HANDOFF.md 下一步计划 vs mdcore 实际走向（逐条核对）

HANDOFF §6 终态（2026-10-27 m2）声明的「剩余」与走向：

| 计划/声明 | 实际走向（mdcore-audit 基线） | 判定 |
|---|---|---|
| 「剩余 = DECISIONS 类别 3 用户裁决面（19 项）+ 产品决策三项（商品名/平台矩阵/闭源）——工程侧无欠账」 | 裁决面已于 2026-10-28 全批（DECISION_SHEET 批注），但 R-1 接线（#A9）未执行；「无欠账」= 无未裁决项 ≠ 无缺陷——mdcore-audit 实出 5 个 P0（含 R-2 撞号 P0-1） | **语义成立、实质存疑**【高度怀疑】 |
| 批次 1 D1 实施轮「批注即开（预告 3-5 轮）」 | 实际路径绕行：S 计划后置（HANDOFF.md:700-702）→ 2026-10-21 协作转正式交付 → mdc-collab 落地 → 高质量但**用户编辑流零消费**（09-collab.md：collab_channel.ts 只作演示 harness） | 已实现（时序偏离 + 消费缺位） |
| 批次 3 渲染管线产品化（C 方案四项落地） | 未走 LO 管线；PDF 面 = 路线 B（pdf.js+lopdf）；docx 面 = GPUI 自绘 + docx_edit 字节手术；render_guard.py 守护器留在研究仓 | **已放弃**（#A8/#A20） |
| web 19 套件复验（待 npm 链重建） | 实际 29 个 py e2e 套件（01-structure.md §1.1 web 行「README 19 套件已漂移」） | 部分兑现（数量超预期、口径漂移） |
| R24-d 壳实体（开发环境轮） | wry 壳曾实机收口（s5b-wry-deb，HANDOFF.md:243-271，`c33bcab`）→ 后停滞（shell-native 568 LOC 蓝图态）→ 主线转 mang-shell GPUI（研究仓无记录） | **偏离** |
| 产品决策三项（商品名/平台矩阵/闭源） | 平台矩阵更新但数字三方互斥（README 850+ vs 静态 1220+ vs mang-shell 563，final-report §1.2 问题 3）；商品名未定；闭源决策因 GPL 栈陷入两难 | 未收口 |
| 磁盘/环境纪律（26 次重置恢复、门禁复验口径演进） | mdcore-audit 亲跑复现：cargo test 1,241 全过 ✓、fmt **977 行漂移**（门禁失效，final-report §0）、scan.sh exit 1 | 复验文化真实；门禁体系部分失效（历史第三次） |

---

## 6. 脚本与工具质量（tools/ + worklog + 两子仓）

- **tools/push_to_github.sh**：token 零落盘纪律实现完整（环境变量单次传递 +
  `http.extraheader` 内存 Basic 头 + unset，tools/push_to_github.sh:24-33）；内置网关
  quirk 注记（smart HTTP 仅 Basic）。质量合格。
- **py_compile 全量 22 件全部通过**（2026-11-05 实测；编译缓存 __pycache__ 已清理，
  研究仓快照保持只读原状）。
- **acrobat-audit/tools 8 件**：MSI 数据库重建/PE 能力矩阵/zip 范围请求取证/
  Authenticode 验签（worklog Task 1：signify 校准实验方法论扎实——「先下载已知有效签名的
  procexp64.exe 校准算法」）。证据 JSON 59 个结构完整（抽查 msi-tables/pe-capability/
  external-research 三族）。
- **agents-md-audit/tools 5 件**：字符级取证（11 类隐藏载荷）/结构提取/供应链扫描/
  指标量化——evidence json 与分析文档互为印证。
- **office-line/spike-ro2 7 件**：见 §3.2——regress.py 的确定性门与钉定纪律是全仓
  工具脚本最佳实践。
- **worklog.md（7,203 行）**：追加式纪律严格（Task ID/Agent/Task/Work Log/Stage Summary
  五段制贯穿 1→m2）；**26 次环境重置恢复全部如实登记**（含「并行线自报全门禁复验与
  实际不符被抓」（HANDOFF.md:193-196）的诚实记录）；「不伪造工作」停机条件在档
  （HANDOFF.md:748-750）。过程质量评级：**极高**。
- **交叉引用关系**：acrobat-audit 与 agents-md-audit 是 L0/L1 两条独立审计线（README.md:21-25），
  与 mdcore 六线的直接决策链弱——其价值在**方法论输出**（敌意输入多层防线 →
  office/pdf SECURITY_POLICY 输入，md-line/STATUS.md:58-60、office-line/SECURITY_POLICY.md
  头注互证）；二者各带独立 WORKLOG 与 GitHub 原始仓指针（README.md:45-48，链接口径
  已随归档过时）。

---

## 7. 结论与覆盖声明

### 7.1 总结论

1. **「mdcore 设计决策来源」声明成立**【已确认】：六线划分、四层决策文档体系、
   20 项裁决与 mdcore crate 家族的谱系可逐条对上（§4 对账表 20 条中 16 条落在
   已实现/维持兑现区间）；DECISION_SHEET 批次 2 的裁决→合入→门禁复验→联动回归扩展
   全链（DECISION_SHEET.md:56-66）是「证据先于裁决、裁决即实施」的完整示范。
2. **谱系的有效边界 = 2026-10-27**【已确认】：此后 mdcore 的两大转向（GPUI 主线、
   GPL vendor）零记录；决策文档体系（DECISIONS 类别 3 十九项「剩余门控」）在裁决
   批注后**停更**——R-1 已批未接、批次 3 已批未建管线、宿主 A 被 mang-shell 取代
   均无人回写对账。建议：把「决策→实现回写」作为 mdcore 治理门禁的一部分
   （本笔记即首个回写件）。
3. **兑现质量分层**：研究证据链质量最高的两域（collab 适配层、office 锚点写面）
   兑现最完整；工程化最弱的面（消毒六面接线、R-1 接线、LO 渲染管线）全部停滞在
   「已建成未消费」或「已裁决未执行」状态——与 mdcore-audit final-report 的
   「消费倒挂」结论（§1.2 问题 4）同构。
4. **最需行动的三件**：R-1 接线（一行常量→配置，裁决已在档）；G3① 三交付条件
   与实现的语义差（自动软上限 vs 不设自动上限——需用户重申）；mang-shell/GPL
   与 ADR-C1 的正面冲突（研究仓红线 vs 产品现实，最终裁决在商业路线）。

### 7.2 覆盖声明

| 对象 | 覆盖 |
|---|---|
| 四份顶层决策文档（DECISIONS/GROUPS/SHEET/HANDOFF，1,530 行） | **100% 全文通读** |
| README/worklog | 100% 通读（worklog 7,203 行：头部 60 + 尾部 120 + 结构抽样） |
| 六线 71 篇 md | 全清单 100% 检视（每篇至少头注+定性结论）；深度通读约 20 篇核心（md-line 4/office-line 9/pdf-line 6 头部+关键节/collab 12 头部/workbench 5 头部）≈ 40% |
| 45 rs 原型 | 结构 100% 清点；深读 4 件（adapter lib.rs 全读 + spike/main + proto-native + formfill_probe 头部） |
| 22 py | **100% py_compile 通过**；深读 3 件（push_to_github.sh/render_guard.py/regress 套件文档） |
| 59 json | 结构抽查（3 族）+ 数量口径核验，未逐件通读（证据数据件） |
| mdcore 侧对照 | crates/collab、office-io、pdf、mdcore(tree)、mdcore-wasm(docx_export)、evidence、mang-shell(tests)、sdk 定向 Grep 实测；引用 audit/notes/01/05/09/11 与 final-report 既有结论 |

### 7.3 风险登记（研究仓侧）

- HANDOFF.md:80-84 记录的「上游三锚点」（yrs #676/#682/0.28.0、TDF 26.2.7）为
  2026-10-27 快照——**当前真值需复核**【需真机验证】（本审计不做网络查询）。
- DECISION_SHEET.md:181 双仓推送态引用（cleanroom `141c482`/mdcore `b15c0ab`）为归档前
  口径；归档动作（外层仓 `88d1206`）后的等价性未验证【高度怀疑：归档完整性】。
