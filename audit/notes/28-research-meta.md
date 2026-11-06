# 审计笔记 28：研究仓元面审计 + 备份目录 + 全仓语法级代码测试（R5-e，2026-11-05）

> 只读审计。对象：`/home/z/my-project/mang-research-archive`（mmqz/mang-research-archive，HEAD=e625568，=origin/main，工作树审计起点干净）。
> 证据口径：所有 `文件:行号` 与 git 命令输出均为 HEAD 实测（沙箱时钟 2026-11-04 21:0x-21:1x UTC）；git 仅用 log/show/ls-tree/rev-list/remote/status 查询型命令，mdcore-audit 目录内零 git 命令。
> 分工：本笔记覆盖 **仓库元面 + _deleted-repos-backup + 全仓语法级代码测试**；typora-analysis / typora-re-analysis / acrobat-pro-audit / cleanroom-analysis 四子仓内容审计由其他子代理（R5-a..d 等）负责，本笔记不重复其内容面。

## 0. 一分钟总览

- 仓库 = 4 个存档子树（typora-analysis 56M / typora-re 33M / acrobat-pro 26M / cleanroom 8.5M）+ 备份目录 56K + .git 25M，共 **147M**；自建提交 **9 个**（first-parent，全部 2026-11-01 单日），全图 **377 提交**（继承源仓历史：1+58+4+125+180）。
- README 三处说法与实测不符：`git log --follow` 可追溯【假】、「4 个源仓」实为 5 次 subtree add、「3-8 MB」体积失真（typora-analysis 56M）。历史本体在对象库中完整【真】，正确追溯口径是 `--full-history` + 原始未加前缀路径。
- desktop-gecko-runtime 移出自洽（1187 文件、目录已无）；但「189 commit 全在」与本地可达 125 提交不符【高度怀疑】。
- remote URL **内嵌 GitHub 令牌**（明文凭据暴露面）。
- 孤儿 Rust 双件与 mdcore office-io **同源命名防御链**（LIMIT_INPUT_BYTES_DOCX/LIMIT_XML_DEPTH），方向判定为 bid-parse 参照引擎（docx.rs:49 自证「引擎 LIMIT_XML_DEPTH 同构」），**未被 mdcore 吸收**（mdcore 无 Block/ParsedFile/open_guarded 任何对应物）。
- 语法测试：**py 63/63 全过（100%）**；**js 118/142 过（83.1%），24 失败全部为非代码缺陷**（Typora 密文 12 + 解密尾料 11 + JSON 数据文件 1），0 个 ESM 待人工；mjs 3/3 过；ts 9 个不做（见 §5.4）。

---

## 1. 仓库元面核验

### 1.1 README.md 说法逐条核验

| # | README 说法（行号） | 实测 | 判定 |
|---|---|---|---|
| 1 | L3「4 个独立仓库经 git subtree 并入」 | git log 实测 **5 次 subtree add**：c8f3e45(+typora-analysis)、1143f9a(+typora-re-analysis)、8c0173f(+acrobat-pro-audit)、701cb43(+desktop-gecko-runtime)、88d1206(+cleanroom-analysis)。README L43-48「合并方式」只列 4 条命令，**漏 cleanroom-analysis 一行** | 【已确认】说法不全 |
| 2 | L50「每个 subtree 提交都带着原仓库的完整 commit 历史（`git log --follow <file>` 可追溯）」 | 三文件实测 `git log --oneline --follow -- <file>` 输出**全空**：cleanroom-analysis/DECISIONS.md、typora-analysis/README.md、typora-re-analysis/README.md（exit=0 零输出）。机制：subtree add 是 merge 提交，--follow 的改名检测跨不过 subtree 前缀边界，且默认历史简化剪掉二父链 | 【已确认】该口径不成立 |
| 3 | 同上「各自历史完整保留」 | 全图 rev-list = **377** = 9 自建 + 197976d(1) + 8f999af(58) + 246fc0f(4) + 100bc4e(125) + cc9405a(180)，逐 subtree 二父算术自洽。正确追溯命令：`git log --full-history -- <原始未加前缀路径>`（实测 DECISION_SHEET.md 回溯出 cc9405a/1de2326/13b6396… 整条 cleanroom 史）或 `git log <add>^2` | 【已确认】历史本体完整，仅追溯口径错 |
| 4 | L16-17「单个仓库都很小（3-8 MB）」 | du 实测：typora-analysis **56M**、typora-re **33M**、acrobat-pro **26M**、cleanroom 8.5M——仅 cleanroom 落在区间 | 【已确认】体积说法失真（typora 口径差 ~7x） |
| 5 | L54「原 5 个仓库归档」但 L55-59 只列 4 名 | 第 5 个应为 desktop-gecko-runtime——但它 L27 声称已恢复为独立仓（非归档），归档名单与「5 个」对不上 | 【已确认】计数与名单不一致 |
| 6 | L27「1187 文件 / 189 commit 历史全在」 | 文件数自洽：`git ls-tree -r 701cb43 | grep -c '^desktop-gecko-runtime/'` = **1187**；e625568 stat = 1188 files changed（1187 gecko + README.md 自身）。提交数**不自洽**：`git rev-list --count 8c0173f..100bc4e3` = **125**，本地无法复现 189 | 【高度怀疑】189 可能是恢复独立仓时 subtree split 的合成提交数或 GitHub 端口径——【需真机验证】网络核对 mmqz/desktop-gecko-runtime 实际提交数 |
| 7 | L50 subtree 提交带完整历史（对 typora-analysis） | typora-analysis 二父 197976d 是**无父根提交**（单提交源仓，2026-09-30）；其源仓「历史」仅 1 条 | 【已确认】typora-analysis 源仓本就单提交 |

### 1.2 desktop-gecko-runtime 移出核验

- `git show --stat e625568`：**1188 files changed, +7/−394,021**（desktop-gecko-runtime/.github/workflows、scripts/verify_*.py、tests/media/*.webm/opus/wav 等全量删除 + README.md 9 行修改）。
- 当前 HEAD `git ls-tree --name-only HEAD` = `.gitignore / README.md / _deleted-repos-backup / acrobat-pro-audit / cleanroom-analysis / typora-analysis / typora-re-analysis`——**无 desktop-gecko-runtime 目录**，与工作树 `ls -A` 一致。
- 结论：移出动作与目录现状**自洽**【已确认】；但 189 vs 125 提交数差异悬置（§1.1 #6）。gecko 历史（125 提交，末条 100bc4e "docs: Runtime v1.0 FREEZE archive (phase Q)"，2026-09-23）仍在本仓对象库可达——「移出」是删除工作树文件而非丢弃历史，README 未说明这一点【设计偏好】可接受。

### 1.3 .gitignore 与 .local/、分支状态

- `.gitignore` 全文：`*.tmp / *.log / .DS_Store / Thumbs.db / .local/`，末段注释「git subtree 元数据（记录已并入的子树）」。
- 实测 `.local/` **不存在**于工作树（ls 报 No such file or directory）。git subtree 工具本身不产生 .local/ 目录——「并入记录」真源是各 merge 提交的二父链，该注释口径不成立【已确认】（该行属预防性条目，无害）。
- 分支：仅 `main`（HEAD=origin/main=e625568），无 tag，无本地领先/落后；`git status` 起点干净（后续出现的 __pycache__ 变更为沙箱并行进程所为，见 §5.5）。
- 时间线（9 自建提交，全部 2026-11-01）：b4666f0 init → c8f3e45 +typora-analysis → 1d4b653 gitignore 加 .local/ → 1143f9a +typora-re → 8c0173f +acrobat → 701cb43 +gecko → 88d1206 +cleanroom → 08bd985 +备份目录/README 索引 → e625568 −gecko。注：任务简报「已知 10 个提交」应为 9 自建 + 1 继承史首个可见提交（cc9405a）的 head -20 口径。
- **凭据暴露**：`git remote -v` 显示 origin URL 形如 `https://[REDACTED:github_token]@github.com/mmqz/mang-research-archive.git`——remote URL 内嵌 GitHub 令牌，`.git/config` 明文持有（`grep -c 'https://.*@github' .git/config` = 1）【已确认】。任何读取配置的进程、目录打包、屏幕共享即泄漏；建议改用 credential helper / SSH。与公开仓再分发 Adobe 二进制叠加，构成 §4 合规面。

---

## 2. 孤儿文件审计（_deleted-repos-backup/，5 文件）

索引：`_deleted-repos-backup/README.md` 自述 2026-11-01 删除 3 个废弃仓（mmqz/mang-shell、mmqz/bid-agent-research、mmqz/adobe-acrobat-dc-security-audit）时提取的唯一/独特内容，「不是所有删除都是无损的」。

### 2.1 bid-agent-research 的 docx.rs / xlsx.rs（静态审计）

对象：`bid-agent-research__bidcore__crates__bid-parse__src__docx.rs`（236 行 / 9147B）与 `...__xlsx.rs`（158 行 / 8623B）。backup README 自述：bid-agent-research 是 ai-bid-research 的前身，241 文件中 239 个已迁移，仅这 2 个废弃旧实现被留档（新仓由 extract.rs + pdf.rs 承担）。

**质量评估（研究原型口径，中上）**：

| 面 | docx.rs | xlsx.rs |
|---|---|---|
| 防御链 | 输入字节门(LIMIT_INPUT_BYTES_DOCX)→prescan→受限读(LIMIT_ACTUAL_READ_DOCX)→XML 深度门(LIMIT_XML_DEPTH, :49-58)，guard 模块集中 | 输入门+prescan→calamine 全量载→**双轴后置门**（有效行/非空单元格，「超量显式拒绝不留部分结果」:56-65）；:3-8 **诚实登记** calamine 黑盒内存尖峰残余面 + 根治路径（zip 流式自读） |
| 正确性设计 | 单趟流式事件解析；章节上下文冻结（表格挂章节 :35-36）；深度递减 saturating 防漏计(:102-103) | 整体失败语义（无部分结果）；行/格双轴独立限额 |
| 瑕疵 | parse_summary 先构造再在 .map() 内重写(:167-181 冗余)；ParsedFile.name/sha256 恒空(:168-169 职责分置不明) | 无实质瑕疵；format 位置/命名参数混用(:91)合法但风格杂 |
| 测试 | 8 断言纯模式测试（:224-235），无文件级 parse 测试 | ** exemplary**：字节级构造最小合法 xlsx（:105-132，xorshift 高熵防压缩比门误伤）+ 正负对照双测（:134-157） |

**与 mdcore crates/office、office-io 的关系（Grep 全仓验证）**：

- 同源命名命中：`LIMIT_INPUT_BYTES_DOCX`（office-io/src/io.rs:111）、`LIMIT_XML_DEPTH`（io.rs:117，=1_000）——**同名常量双仓并存**；office-io 侧由 r4-t2（2026-10-17）落档，口径 = docx 族 50MB+2MB 边距。
- 方向判定：备份 docx.rs **第 49 行注释自证**「XML 嵌套深度门（**引擎 LIMIT_XML_DEPTH 同构**…）」——写作时存在一个被称作「引擎」的先行实现；结合 office-io r4-t2 时间线，判定为 **bid-parse 参照 mdcore office-io 的限额命名/门结构**（同谱系消费者），而非 mdcore 吸收 bid-parse【高度怀疑】（定谳需核对 ai-bid-research 仓提交时间，本沙箱无此仓）。
- 未吸收证据：mdcore 全仓（crates/office、crates/office-io、mang-shell）**零命中** `open_guarded / read_entry_limited / LIMIT_ACTUAL_READ_DOCX / LIMIT_XLSX_FILLED_ROWS / LIMIT_XLSX_TOTAL_CELLS / ParsedFile / BlockKind`；office-io docx.rs 是 Paragraph/Table 完整模型（:21 起，#39 嵌套表、tblHeader 注入等），与 bid 的 Block/locator 块模型无结构对应；mang-shell/src/editor/docx_doc.rs:193 仅复用 `io::LIMIT_INPUT_BYTES_DOCX` 的 4 倍可编辑装载上限。
- 谱系互证：xlsx.rs:6-8 登记的「calamine 内存尖峰，根治需流式自读」与 office-io io.rs:67-77 的 GridScan 流式根治（sparse_bomb 探针 1588B→~550GB 分配，2026-10-17）指向同一威胁模型的两个独立解法【已确认】同谱系；mdcore 的依据链是 cleanroom-analysis/office-line/spike（sparse_bomb.rs），不是 bid-parse。
- 结论：**这两个文件是 mdcore office-io 安全设计的旁系应用遗物，对 mdcore 无代码血缘贡献，留档价值 = 防御链设计传播的证据 + 「为何重构」的考古件**。

### 2.2 mang-shell__README.md（WebView2 版桌面壳诊断记录）

- 内容：tao 0.37.2 + wry 0.57.1 + WebView2（Windows 11）桌面壳的**自绘标题栏拖拽失效**完整求助式诊断——WebView2 铺满客户区 → 父窗口 WM_NCHITTEST 拿不到标题带点击（:23-35），三种方案失败表（:37-44），5 个怀疑方向与验证法（:151-160，含「SendMessageW 跨线程」与「WebView HWND 挂子类返回 HTTRANSPARENT」两个最可疑项）。
- 为何孤儿：技术路线被 **GPUI 取代**（backup README L9：GPUI 版在 mdcore/mang-shell）。与 mdcore 现状呼应：mdcore 主线 GUI = mang-shell（GPUI/Zed text::Buffer，见 01-structure.md §0），而 crates/shell-native（tao+wry+GTK3，蓝图态）是另一条 web 宿主线——本文件是 **tao+wry 路线在 Windows 上的死路证据**，与根 README L36「Gecko 运行时→桌面壳选型否决路径（最终选 GPUI）」同属选型档案【已确认】孤儿价值 = 决策证据，非代码资产。
- 附带信息：环境 Rust 1.99 / WebView2 Runtime 156.0.4314.50（:173-177）；License: MIT。

### 2.3 adobe-acrobat-dc-security-audit__README.md（Acrobat 审计早期版）

- 内容：170 行完整安全报告——Acrobat DC 26.002.21931 安装包（Win zip 1.58GB/mac dmg 1.59GB）审计：五级 Authenticode 签名链逐文件验证「无篡改」、MSI 数据库解码（服务/自启动/可疑特征全良性）、端点归属表（全 Adobe/已知遥测商）、后门十项检查清单「**无后门**」结论、「后台占 CPU」三类公开行为 + 本版桌面堆泄漏缺陷（1 万+ USER 对象/1.2 万+ 窗口句柄）的解释。
- 为何孤儿：**被 acrobat-pro-audit 顶层仓取代**（backup README L11：内容已基本覆盖，保留早期结论）。差异价值：含最终版可能未保留的早期口径（如 dmg 侧「上轮会话验证，本轮引用」的引用链式取证）【已确认】定位 = 结论快照，与顶层 acrobat-pro-audit 的 6 篇 analysis/ + evidence/ 构成前后两代。
- 备注：cleanroom-analysis/acrobat-audit/ 是第三个 Acrobat 审计副本（tools/ 与 evidence/msi-tables/File.json 与顶层仓同名重复 >1M 大文件）——三份 Acrobat 取证材料并存，含根 README 未声明的重复面【已确认】（内容归 acrobat 代理，此处仅记元面重复事实）。

### 2.4 _deleted-repos-backup/README.md

自索引件：3 仓删除理由表（L7-12）+ 保留理由（L15-18）+ 恢复路径（L22-26，「GitHub 删除的仓库无法恢复」+ 各自替代源）。与 08bd985 提交（「加备份目录 + 更新 README 索引」）对应【已确认】。
命名约定：`仓__路径__文件` 双下划线扁平化【设计偏好】可读性好，但 `__` 与原路径中的合法 `_` 不可逆区分（bid-agent-research__bidcore 串无法机械还原原路径）。

---

## 3. 体积与合规

### 3.1 体积量化（du 实测）

| 区域 | 体积 | 说明 |
|---|---|---|
| 全仓（含 .git） | **147M** | .git 25M + 工作树 ~122M |
| typora-analysis | 56M | 三版本 beautified/asar 树为主体 |
| typora-re-analysis | 33M | sources/ 四版本 + compliance_fullcross |
| acrobat-pro-audit | 26M | source/ Adobe 二进制 ~21M + evidence JSON |
| cleanroom-analysis | 8.5M | 文档 + spike + acrobat-audit 副本 |
| _deleted-repos-backup | 56K | 5 孤儿文件 |

### 3.2 大文件与二进制

- **>1M 文件 27 个**，三类：Adobe 二进制（adobearm.exe / AGSService.exe / gccustomhook.exe / AcroPro.msi 13.2MB / arm_payload 等）、Typora 大 JS（main.beautified.js / frame.js / frame.beautified.js / mermaid.min.js / tex-svg-full.js / MathJax en.js 等）、取证数据（acrobat 与 cleanroom 两份 File.json、needle.pkl 1.13MB）。
- **可执行/安装器 11 个**（exe/msi/msp，全部位于 acrobat-pro-audit/source/，合计 ~21MB）；**V8 字节码 1 个**（typora-re sources/1.14.10-stable/main-process/atom.compiled.dist.jsc，388KB）。本审计未执行任何二进制（纪律遵守）。
- 卫生瑕疵：**1 个 .pyc 被提交入库**（typora-re-analysis/projects/delta/deep_dive/data/__pycache__/d4_binding_inventory.cpython-312.pyc，git ls-files 实测）【已确认】。

### 3.3 合规风险汇总评级

| 风险 | 证据 | 评级 |
|---|---|---|
| Adobe 专有二进制公开再分发 | source/AcroPro.msi 等 11 件 ~21MB，公开仓 mmqz/mang-research-archive；无 Adobe 再分发授权 | **高**（法律面） |
| Typora 闭源代码 beautified/解密树公开托管 | typora-analysis 三版本 asar_app/beautified、typora-re app-decrypted/——商业闭源 EULA + 逆向工程 ToS 风险 | **高**（法律面） |
| 凭据暴露 | remote URL 内嵌 GitHub token（§1.3） | **高**（安全面，独立于内容） |
| 仓库无 LICENSE 声明 | find LICENSE* 仅命中 Typora 自带 license.html/LicenseIndex.*.js（属被审计对象），本仓自身零许可声明 | 中 |
| 工程面 | 纯只读存档、不参与构建/分发链 | 低 |

综合：**法律/合规面高风险**（三高叠加：Adobe 二进制 + Typora 闭源代码 + 内嵌令牌，均已在公开仓）；工程面低。若仓为私有则降为中低，但 remote 指向 github.com/mmqz/* 公开命名空间且 README 以公开链接口径写作（L28/L32）——【需真机验证】仓的实际可见性（本沙箱无网络）。缓解建议（不在本次执行）：剥离 source/ 二进制与 beautified 树至私有存储、轮换令牌、补 LICENSE 与第三方材料清单。

---

## 4. 全仓语法级代码测试结果

### 4.1 Python（63 个 .py，py_compile，100% 通过）

方法：逐文件 `py_compile.compile(f, cfile='/tmp/r5e_pyc.pyc', doraise=True)`（字节码全写 /tmp，仓内零写入）。

| 子仓 | 文件数 | 通过 | 失败 |
|---|---|---|---|
| acrobat-pro-audit/tools | 8 | 8 | 0 |
| cleanroom-analysis/acrobat-audit/tools | 8 | 8 | 0 |
| cleanroom-analysis/agents-md-audit/tools | 5 | 5 | 0 |
| cleanroom-analysis/office-line/spike-ro2 | 7 | 7 | 0 |
| cleanroom-analysis/workbench-line/proto-host | 2 | 2 | 0 |
| typora-analysis/scripts | 9 | 9 | 0 |
| typora-re-analysis（scripts/compliance_fullcross/delta/prototype 测试） | 24 | 24 | 0 |
| **合计** | **63** | **63** | **0（通过率 100%）** |

### 4.2 JavaScript（142 个 .js，node --check v24.22.2）

| 分类 | 数量 | 明细 |
|---|---|---|
| 通过 | **118（83.1%）** | 含全部 beautified 大文件（main.beautified.js 等）与 minified vendor 件 |
| 语法失败—**非代码缺陷** | 24（16.9%） | 见下表三分类，无一属于「仓库自有代码写错」 |
| ESM 待人工 | **0** | 无 .js 触发 import/export 类报错（ESM 构建件均为 .mjs 且通过） |

24 个失败三分类（node 报错原样记录）：

| 类 | 文件 | 报错样例 | 定性 |
|---|---|---|---|
| Typora 0.11.18 加密源（密文误当 .js） | typora-analysis/versions/0.11.18/asar_app/{Desktop,Dict,DocumentController,Downloader,Env,FilesOp,License,Setting,WindowController,atom,menu,updater}.js（12 件） | `SyntaxError: Unexpected identifier 'icMv'`（首行即高熵乱串） | 密文载荷，非源码；失败属预期，反而佐证 0.11.18 asar_app 加密事实【已确认】 |
| 解密产物尾部残留二进制 | typora-re-analysis/sources/0.11.18/app-decrypted/ 同名 11 件 | `SyntaxError: Invalid or unexpected token`（Desktop.js:319：`app.getPath(t));\x03\x03\x03`；各文件尾字节 03/05/08/0e/0f/10 或 `70 3b 01` 型） | JS 本体有效，尾 1-3 字节解密填充未剥；同目录 License.js（尾 0x0c=FF 合法空白）与 frame.js（尾干净）通过，反证定性【已确认】 |
| JSON 数据误挂 .js 扩展 | typora-analysis/versions/0.11.18/asar_lib/MathJax3/es5/sre/mathmaps/en.js | `SyntaxError: Unexpected token ':'`（首字符 `{` + 键名冒号） | MathJax mathmaps 数据文件，非代码【已确认】 |

### 4.3 其他脚本件

- **.mjs 3 个**（超任务范围补测）：build.mjs ×2、interop_js.mjs——**3/3 通过**。
- **.ts 9 个**：obsidian.d.ts、prototype/web/src/main.ts、spike a/b/core 各 main.ts、core/src/{index,types,markdown,core}.ts、tests/client/harness.ts——**不做** node --check：node --check 仅按 JS 语法解析，TS 的类型注解/接口/泛型必然报语法错（结果无意义）；类型正确性需 tsc 编译器（沙箱纪律禁装依赖/禁编译产物落盘），故按任务口径声明不做。
- **.rs 78 个**：不在本任务语法测试范围（rustc 检查需 emit 落盘，违反只写一件纪律）；其中 2 件的静态人工审计见 §2.1，其余归各子仓代理。

### 4.4 环境噪声记录（非本代理写入的证据链）

审计窗口内（沙箱 21:12 前后）仓内出现 __pycache__ 目录波动（10 → 6 个目录）与 tracked .pyc 转脏（`M ...d4_binding_inventory.cpython-312.pyc`）。**本代理零写入实证**：(a) 全部 py_compile 字节码定向 cfile=/tmp；(b) 对照实验——/tmp/r5e_t/a.py 经同款命令编译后目录内仅 a.py + out.pyc，**无 __pycache__ 产生**；(c) node --check / git 查询 / du / find 均无写语义。判定为沙箱内**并行进程**（其他子代理会话）运行 Python 所致；按只读纪律未做任何清理或还原。该 .pyc 的入库事实本身已记入 §3.2 卫生瑕疵。

---

## 5. 结论与覆盖声明

**核心结论**：
1. 【已确认】研究仓元面与 README 有四处口径漂移：`--follow` 追溯说法不成立（正确口径 `--full-history` + 原路径）、5 次 subtree 实并入（README 列 4）、体积「3-8MB」失真（最大 56M）、「原 5 仓归档」名单 4 个。**历史本体完整保留为真**（377 提交算术自洽，二父链全在）。
2. 【已确认】desktop-gecko-runtime 移出自洽（1187 文件 + 目录归零）；【高度怀疑】「189 commit 全在」与本地 125 不符——【需真机验证】独立恢复仓实况。
3. 【已确认】孤儿双 Rust 件 = mdcore office-io 安全设计语言的**旁系应用遗物**（同名限额常量 + 「引擎…同构」自证），无代码血缘被吸收；mang-shell README = tao+wry/WebView2 死路的决策档案；adobe README = Acrobat 审计早期结论快照。
4. 【已确认】语法级测试全绿口径成立：py 63/63、js 失败 24 件全数定性为密文/解密尾料/数据文件，**仓库自有代码语法缺陷 = 0**。
5. 【已确认】合规面高风险三叠加（Adobe 二进制 21MB + Typora 闭源 beautified/解密树 + remote 内嵌令牌），且 1 个 .pyc 入库；缓解动作超出本次只读范围。
6. 【设计偏好】_deleted-repos-backup 的「独特内容抢救」模式与扁平 `__` 命名值得沿用，但建议附原始路径清单以可逆。

**覆盖声明**：本代理（R5-e）负责区域——**仓库元面（README/git 元数据/时间线/remote）、_deleted-repos-backup 全部 5 文件、全仓 63 py + 142 js + 3 mjs 语法级测试——100% 覆盖**。四个子仓（typora-analysis / typora-re-analysis / acrobat-pro-audit / cleanroom-analysis）的内容级审计由其他子代理负责，本笔记仅涉其元面与统计口径。

**建议下一步**（不在本次执行）：核对 mmqz/desktop-gecko-runtime 提交数（189 vs 125）；轮换 remote 令牌并改 credential helper；评估 source/ 二进制与 beautified 树迁私有；对 app-decrypted 11 件做尾字节剥离后复测 node --check（预期 129/142 → 全自有件通过）。
