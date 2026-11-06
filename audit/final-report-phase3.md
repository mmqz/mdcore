# mdcore 全仓覆盖审计补充报告（Phase 3 · 研究仓卷）

- **对象**：mdcore main@`6322f86` + **研究仓 mang-research-archive@`e625568`**（双仓；研究仓含 typora-analysis / typora-re-analysis / acrobat-pro-audit / cleanroom-analysis / _deleted-repos-backup 五区，146MB、473 个代码文档文件）
- **日期**：2026-11-05
- **触发**：审计方指令——「全仓代码审计：两仓全部代码含研究仓均须覆盖，过程含代码测试」
- **方法**：6 并行子代理（R5-a~R5-e 研究仓五路 + R6-a mdcore 残余盲区）+ 主审计员第三轮测试实证（笔记 30）
- **纪律**：双仓全程只读；mdcore `git status` 全程干净（构建产物仅落 gitignore 目录；删除过一次 7.2G gitignored `target/` 缓存，零跟踪文件影响）；研究仓零写入（Python 语法检查字节码定向 /tmp；git 仅只读命令）；唯一写动作 = mdcore 审计分支新增笔记 24-30 与本报告

---

## 0. Executive Summary

**一句话总评：研究仓全量打开后，「mdcore = cleanroom 决策谱系的忠实执行者」这一自我叙事大体成立——20 条重大决策 12 全落地、全部选型 pin 可溯源到 ADR；但偏差面集中在「软承诺」：ADR 声称已落地而代码未动（ADR-B5 求逆回滚）、已裁决未接线（R-1 undo 合并窗假旋钮）、语义走样（G3① 自动 clear_all）、整案零实现（批次3 C 方案）。研究仓自身的最大问题不在研究质量（过程纪律极高）而在归档治理——公开仓携带 21.97MB Adobe 商业安装器与 Typora 闭源反混淆/解密树，属尚未爆发的合规暴露。**

最重要的八件事：

1. **双仓 100% 覆盖达成**：mdcore 残余盲区（spike/packaging/scripts/compliance_release/根草稿/web 位置之谜）补全（笔记 29）；研究仓五区全量（笔记 24-28：决策文档 100% 通读、脚本 100% 语法检查、二进制 100% 指纹化零执行）。
2. **新 P1-26 研究仓合规暴露**：mang-research-archive 为 public 仓；acrobat-pro-audit/source 携带 11 个 Adobe 安装器共 21,976,736 B（占该子仓 84%），`push_to_github.sh:18` 以 `public:true` 建仓；叠加 Typora 56MB beautified 渲染层 + 0.11.18 解密闭源树（AES-256-CBC 钥匙硬编码在仓内脚本）。
3. **新 P1-27 决策档案可信度问题（ADR-B5）**：typora-re 的 ADR-B5 自述「求逆回滚已落地」，但原型与 mdcore main 的 undo 路径至今仍是 `tree.clone()` 备份 + 扁平 `.reverse()`（笔记 25；笔记 04/11 佐证）——决策说的与代码干的对不上。
4. **决策对账 20 条重大决策：12 全落地 / 4 部分 / 2 偏离 / 1 未实现 / 1 放弃**（笔记 27）：D1 消毒三裁决全兑现（docx.rs:3877/3981/4487 可溯）；选型 pin 全落地；偏差集中在 G3①、R-1、批次3 C 方案、宿主 A→GPUI 换轨（研究仓零记载）。
5. **原型吸收链证实**：31/31 个 Rust 原型文件级 100% 被吸收进 mdcore（逐字节相同起步、主仓为超集）——但只走 CM6/web 线，mang-shell GPUI 主线零吸收（双轨格局再添一层实证）。
6. **第三轮测试全绿**（笔记 30）：workspace 1,311/0/7、pdf-lopdf 182、clippy 5 警告、wasm32 双 crate、**mang-shell lib 456 首次 Linux 实跑**、web_smoke 15/15、tr2b_l1 652/652、**tr2b_l23 经桥接 4/4+6/6**——推翻 phase2 两项「环境门不可验证」结论（C-4/C-7）。
7. **mdcore 残余面新 P2-37**：NSIS 打包三断链（`{{VERSION}}` 无注入机制、WebView2 引导 URL 定义未用 + ExecWait 不存在文件、`File "..\.."` 与 BUILD-VERIFY 示例命令矛盾）+ macOS 纸面平台；另 spike 评估用 crates.io gpui 0.2.2 而生产 vendor zed@1663af4（评估≠落地）。
8. **fmt 漂移 1896 行独立复核**与开放 PR #216 精确吻合——该 PR「机械复位」前提成立，可放行（建议合并时附 hunk 数上限门防再漂）。

---

## 1. 双仓覆盖声明

**mdcore 仓（`6328f86` 基线）**：

| 区域 | 覆盖笔记 |
|---|---|
| 16 crates（mdcore/formula/office/office-io/pdf/pdf-wasm/mdcore-wasm/canvas/collab/evidence/relay/settings/mdbridge/shell/shell-native/workbench） | 01-09、17、18 + 30（workspace 测试） |
| mang-shell（GPUI 编辑器，独立 workspace） | 10、22、23 + 30（lib 456 实跑；bin/14 集成目标 = xkbcommon 环境门） |
| web/（TS 前端 + e2e 套件） | 20、21 + 30（三套件复跑） |
| sdk/（mang-rpc + mang-mcp + python） | 17 |
| spike / packaging / scripts / compliance / compliance_release / 根部 9 个草稿文件 / gates.md | 29（补全） |
| .github/workflows / CI 门禁 | 13 + 30 复核 |
| docs/ 与 mang-shell/docs/ | 29（未引用文档登记） |

**研究仓（`e625568` 基线）**：

| 子仓 | 覆盖笔记 | 抽样口径 |
|---|---|---|
| typora-analysis（174 文件 / 56MB） | 24 | 清点 100%；README+报告 100%；scripts 12/12 全读+py_compile 9/9；versions 逐行精读 21/146 ≈14% + 35 处锚点验证 |
| typora-re-analysis（259 文件 / 文本 LOC ≈402.5K） | 25 | decisions/plans/compliance/projects 100% 通读；rs 31/31 枚举 + 16 全文 diff；py 24/24 编译；sources 性质确认 + 3 抽样（闭源料 32 万行不逐行） |
| acrobat-pro-audit（82 文件 / 26MB） | 26 | md 8/8 全读；py 8/8 全读+编译；二进制 11/11 指纹零执行；json 100% 枚举 + 深读 15/48；20+ 定量结论抽检零漂移 |
| cleanroom-analysis（决策谱系源） | 27 | 四份决策文档 100% 通读；97 md 清单 100% 检视（深读 ~40%）；rs 45 结构全清点 + 4 深读；py 22/22 编译；20 条重大决策对账 |
| _deleted-repos-backup + 仓库元面 | 28 | 孤儿 5 文件全审（含 2 个 Rust 解析器与 mdcore 同源命名链对照）；subtree/--follow/移出自洽性核验；全仓 py 63/63、js 118/142 语法测试 |

## 2. 新问题登记（续 phase2 编号：P1-26 起 / P2-35 起）

### 新 P1（2 项）

| ID | 模块 | 位置 | 问题 | 影响 | 置信 | 笔记 |
|---|---|---|---|---|---|---|
| P1-26 | 研究仓合规 | acrobat-pro-audit/source（11 个 exe/msi，21,976,736B）+ push_to_github.sh:18（public:true）+ typora-analysis versions/（56MB beautified 渲染层）+ typora-re sources/（0.11.18 解密树，钥匙在 decrypt_01118.py:25） | **公开仓分发商业闭源产物**：Adobe 安装器（含完整 AcroPro.msi，sha256 089dc9a2…60f4b2d）+ Typora 闭源代码树（subtree 合并使 mang-research-archive 双份携带） | 法律/合规面高风险（工程面低——对 mdcore 零传染，rg 验证零引用）；远端可见性【需真机验证】 | 已确认 | 26、28 |
| P1-27 | 决策档案可信度 | typora-re decisions/ADR-B5 vs 原型 lib.rs:229/246/285 与 mdcore main lib.rs:297/314 | ADR 自述「求逆回滚已落地」，实际两份快照 undo 路径均仍 `tree.clone()` 备份 + 扁平 `.reverse()`——ADR 要修的「嵌套 undo 硬失败」缺陷形状仍在 | 决策档案存在「说了没做」条目；为 P0-1（undo 链）修复优先级新增证据角度；「决策已落地」叙述不成立 | 已确认 | 25（对照 04:196、11:110） |

### 新 P2（3 项）

| ID | 模块 | 位置 | 问题 | 笔记 |
|---|---|---|---|---|
| P2-35 | 研究仓元面 | README.md（合并方式/体积/commit 数叙述） | 四处口径失真：subtree 实际 5 次（README 列 4）；「`git log --follow` 可追溯」实测不成立（三文件输出全空，正确口径 `--full-history`+原始路径）；desktop-gecko「189 commit 全在」vs 本地可达 125【高度怀疑】；「3-8MB」vs typora-analysis 实际 56MB | 28 |
| P2-36 | 决策执行偏差 | cleanroom 批次裁决 → mdcore 实现 | 四件套：G3① 裁决「手动 clear_all + 512MB 告警」→ 实现为软上限**自动** clear_all（session.rs:379-397，计量=栈深非内存）；R-1 undo_coalesce_ms 已裁决接线但未执行（引擎恒 1000ms 假旋钮）；批次3 C 方案（LO 双轨+运行时检测）零实现（R-O2 22 用例滞留研究仓）；宿主 A（web+wry）被 GPUI 取代而研究仓零记载 | 27 |
| P2-37 | packaging | packaging/nsis/installer.nsi.tpl:9,15,40 + :12,54-60 + BUILD-VERIFY.md:105 | 三断链：`{{VERSION}}` 占位符无注入机制（示例 `-DVERSION` 与语法不匹配→字面进 exe 名/注册表）；WebView2「引导下载」URL 定义后零使用且 ExecWait 不存在的 `$TEMP\wv2setup.exe`；`File "..\.."` 相对路径与示例命令（仓根执行）矛盾必 not-found；另 macOS 仅 Info.plist 无构建路径（三平台表不含 macOS=纸面平台）；deb/AppImage 现捆占位图标 | 29 |

### 新 P3（精选）

研究仓侧：remote URL 内嵌 GitHub 令牌（本地 .git/config 面，操作泄露风险）；1 个 .pyc 已入库；6 个 typora-analysis 脚本硬编码 `/home/z/...` 绝对路径（「可独立复跑」不成立）；race_capture.py:68 恒 False 死分支（链式比较）；extract_endpoints.py 域名正则把 `e.top` 等当域名（假阳性入 endpoints_old.json）；zip_cab_index.py:48 MSCF 头格式串错位（versionMinor/Major 读成 u16=259，全部 CAB indexed=1、gc_hits 全空——WORKLOG R6 叙述与证据不符）；acrobat 两 JSON 为早期脚本代次产物与最终 schema 不同名（README「复现一致性」需加范围限定）；update_mdcore_compliance.py 为写入型卷宗注入件（未执行）。mdcore/web 侧：tr2b_l23 硬编码 /tmp soffice 路径（测试不可移植，T-e）；CI wasm32 无 mdc-pdf-wasm（T-b）；compliance/README 分支叙事与主树现实不符；zed_port.py 三起事故同根因（正则 TOML 解析单点脆弱、上游 commit 钉值无机器校验）；`sed 's/mang/mdcore/g'` 子串反转机制脆弱；9 个根部草稿文件零信息损失可删（仓库卫生）；L3 语料 Word-Crash067 引擎全空 vs LO 可提取（T-f，需 office 线复核）。

## 3. 决策谱系对账摘要（cleanroom → mdcore，笔记 27）

- 可数决策 ≈37 项（DECISIONS 20 + GROUPS 17→5 组 + SHEET 4 批次 + 线内 ADR ~15）；本轮对账重大决策 20 条：**已实现 12 / 部分实现 4 / 偏离 2 / 未实现 1 / 已放弃 1**。
- 「设计决策来源」声明成立：批次 2 裁决 → `c4856f7` 合入 → 联动回归全链可精确对账（docx.rs:3877/3981/4487）。
- 选型全落地：pdfium-render 0.9.4 / lopdf 0.45 / calamine 0.36 / docx-rs 0.4.22（zip 8.6 偏 ADR ≥9.x 有 DEPS.md 在档依据）；排除 mupdf（AGPL）的决策与 crates/pdf 主干一致。
- 四项偏差见 P2-36；**GPL 红线被正面打破**（md-line/STATUS.md:24-26「ADR-C1 GPL 零容忍」vs third_party/zed 10 crate 48.5K LOC——final-report §1.2 已登记，本轮补谱系学定位）。
- **研究仓时间盲区**：cleanroom 记录终点 2026-10-27（c57b6ce）→ 审计基线 11-04（96596d7），其间 mang-shell 106 commits 零研究记录——「决策-执行」链路在 GUI 线存在 8 天无 ADR 真空期。
- 上游欠账：yrs 两份 ready-to-file issue 至今未提交（upstream/README.md:12-15）。

## 4. mdcore 残余面结论（笔记 29 摘要）

- **spike**：probe-gpui 依赖 crates.io gpui 0.2.2（709 包锁）——评估路线 ≠ 落地路线（生产 vendor zed@1663af4）；无第二框架痕迹。
- **packaging**：NSIS 三断链 + macOS 纸面平台（P2-37）；deb/AppImage 单源版本闭环成立；TRADEMARK_RULES「不在仓库」的挂起理由失效（文件实在档）。
- **scripts**：zed_port.py 与 mang-shell Zed 集成的关系补档（正则 TOML 解析三起事故同根因）；四个 fixture/probe 脚本 py_compile 全过。
- **根部 9 个草稿文件**：全部完成「集成锚点对账」（display_map.rs:160/165/173 等）——零信息损失，属可删开发残渣；.t.log 为 Windows 开发机 cargo 警告日志。
- **web 位置之谜澄清**：web/ 就在仓根（sdk/ 只有 mcp/python）；test-runs/web-e2e.log 的 4 条 "No such file" 是运行器把 6 个**有意放在 web/ 根**的脚本按 `web/tests/` 路径调起所致；web-e2e.log 实为 12 行尾片段而非全量日志；k5/k45 的 ✅ 无工件佐证——上轮记分板证据等级修正（C-6）。

## 5. 第三轮测试实证摘要（笔记 30）

全绿矩阵：workspace（15 crate，含 wasm 双 crate 原生面首跑）**1,311/0/7**；pdf-lopdf 182/0/1；clippy 5 unused/0 err；fmt 1896 行/134 hunk（=PR #216）；wasm32 双 crate check ok；**mang-shell lib 456/0/5（首次 Linux 实跑）**；web_smoke 15/15；tr2b_l1 652/652；tr2b_l23 桥接后 4/4+6/6。环境门：pdfium（ignored=1）、xkbcommon（mang-shell bin+14 集成目标）、GTK（shell-native）、无 sudo。口径：CARGO_INCREMENTAL=0 + CARGO_PROFILE_DEV_DEBUG=0（磁盘限制，不影响结果语义）。

## 6. 对上轮结论的修正（3 项 + 1 项归并）

| # | 上轮结论 | 本轮修正 | 证据 |
|---|---|---|---|
| C-4 | phase2 §7「mang-shell 编译与测试不可验证（GPUI 系统依赖，沙箱不可装）」 | **前半句推翻**：lib 面（含 GPUI/Zed vendored 树编译）在本 Linux 沙箱可构建且 456 测试全绿；不可验证面收窄为「bin 链接 + 14 个集成测试目标」（-lxkbcommon，无 sudo 不可装） | 笔记 30 §1.6/§1.7 |
| C-5 | 笔记 15 称 typora-re「26 ADR」 | **勘误：10 ADR + 1 plan** | 笔记 25 |
| C-6 | 笔记 21 记分板（web-e2e.log 为全量日志、k5/k45 ✅、根目录 4 个脚本） | **证据等级修正**：web-e2e.log 实为 12 行尾片段；k5/k45 ✅ 无工件佐证；根目录实为 6 个脚本 | 笔记 29 |
| C-7 | phase2 §7「LibreOffice 差分（tr2b_l23_external 环境门）」 | **推翻**：系统 LO 25.2 存在，真实根因是测试硬编码已回收的 /tmp 安装路径；仓外软链接桥接后 4/4+6/6 通过（LO 26.8→25.2 替换不影响判定） | 笔记 30 §1.11/T-e |

## 7. 模块裁决更新（仅增量）

| 模块 | 裁决 | 变化原因 |
|---|---|---|
| mang-research-archive（研究仓整体） | **D-档（治理动作，非代码裁决）** | 非生产代码资产；研究过程质量 A-（worklog 7,203 行五段制、26 次重置全登记、三次自我修正留痕、哈希链三层自洽）；治理动作见 T-R | 笔记 24-28 |
| mdcore 各模块 | **维持 phase1/2 裁决** | 本轮无新推翻；P2-37 打包链并入 T5 修复批次；P1-27 为 undo 链（P0-1）新增证据角度 | — |

## 8. 路线图增量

- **T-R（研究仓治理批次，新增）**：① Adobe 二进制迁出公开仓（artifact-hashes.txt 哈希清单保留在档，可复现性不损失）② Typora beautified/解密树加法律声明或转私有 ③ README 四处口径修正 ④ remote token URL 清理 ⑤ .pyc 删除 ⑥ 已知缺陷脚本（race_capture/zip_cab_index）修复或标注废弃。
- **T5 增补**：NSIS 三断链 + macOS 构建路径（P2-37）随 fmt 复位批次处理；TRADEMARK 复查解挂。
- **P0-1/P0-3 维持开放**：ADR-B5 矛盾（P1-27）为 undo 修复优先级新增证据——「求逆回滚已落地」叙述不成立。
- **PR #216 放行建议**：fmt 复位前提经本轮独立复核成立（1896 行精确吻合）；合并时建议附 hunk 数上限门防口径再漂。
- **CI 增补建议**：mdc-pdf-wasm 纳入 wasm32 check（T-b）；mang-shell lib 面可考虑 Linux job（C-4 证明可跑）。

## 9. 本轮不可验证项（如实登记）

mang-shell bin 与 14 个集成测试目标（xkbcommon + 无 sudo）；mdshell-native（GTK dev）；pdfium 动态库真渲染面；真 IME；MCP 组合逃逸端到端串测；Adobe/Typora 法律面正式法律意见（本审计仅为工程风险评级，非法律结论）。与上轮相比，**mang-shell lib 面与 tr2b_l23 已从不可验证转为已验证**。

---

## 附录：本轮笔记索引（audit/notes/）

| # | 文件 | 内容 | 来源 |
|---|---|---|---|
| 24 | research-typora-analysis.md | Typora 三版本审计仓全量：性质/报告 11/11 复核/脚本缺陷/合规高风险/对 mdcore 零传染 | 子代理 R5-a |
| 25 | research-typora-re.md | Typora 深逆仓全量：10 ADR 登记/**ADR-B5 矛盾**/rs 原型吸收 31/31/compliance 四目录 CLEAN/PDF carrier plan 谱系 | 子代理 R5-b |
| 26 | research-acrobat.md | Acrobat 审计仓全量：哈希链自洽/36 PE 能力矩阵/SKIP_GC 确证/MSCF 工具 bug/Adobe 安装器公开分发风险 | 子代理 R5-c |
| 27 | research-cleanroom.md | **决策谱系源全量**：决策清单（≈37 项）/六线审计/20 条对账表（12/4/2/1/1）/HANDOFF 对照 | 子代理 R5-d |
| 28 | research-meta.md | 研究仓元面：subtree/--follow/gecko 移出核验/孤儿文件（bid-parse 与 mdcore 同源命名链）/全仓语法测试（py 63/63、js 118/142） | 子代理 R5-e |
| 29 | residual-coverage.md | mdcore 残余盲区补全：已覆盖判定表/NSIS 断链/spike/scripts/草稿对账/web 位置之谜 | 子代理 R6-a |
| 30 | test-runs-round3.md | 第三轮测试实证：执行矩阵 11 项/对账表/环境门/T-a~T-f | 主审计员 |
| — | test-runs/round3/ | 原始日志（workspace/pdf-lopdf/clippy/fmt-check/wasm-check/mang-shell）+ web-results 三 JSON | 自动化产出 |

*本报告由只读审计产生；所有修复建议均未实施。审计分支：`audit/architecture-review-2026-11`（仅新增 audit/ 文档；未触碰 main、未建 PR）。*
