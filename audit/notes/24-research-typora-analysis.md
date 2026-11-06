# 24 · 研究仓 typora-analysis 全量审计（R5-a，2026-11-05）

> 审计对象：`mang-research-archive/typora-analysis/`（Typora 三版本逆向分析工作区：1.14.10-stable / 1.14.1-dev-beta / 0.11.18）。
> 方法：只读静态审计——目录级全量清点（find/wc/du 实测）；report/ PDF 13/13 页全读；scripts 12 文件全文通读，Python 仅 `python3 -m py_compile`（9/9 通过，未运行脚本本体）；versions/ JS 一律静态阅读 + 锚点级检索，未用 node 执行任何仓内文件；零网络、零 git 写操作。
> 定位：这是「研究仓全量覆盖」补洞轮——`audit/notes/15-benchmark.md:19-21` 的三仓表（cleanroom / typora-re-analysis / acrobat）**未列 typora-analysis**，本笔记为该子仓首次入档审计（编号接续 notes 01-23）。
> 兄弟仓区分：typora-analysis（本仓，2026-09-30，隐私/性能技术栈审计 + 原始 beautified 树）≠ typora-re-analysis（2026-10-01~10-10，架构深读 + ADR + 合规审计）。两者源料有重叠但角色不同。

## 0. 总览事实（一分钟版）

- **规模**：174 文件 / 56 MB / 文本行 1,141,443（js 1,028,865 + css 91,305 + html 4,527 + json 9,973 + txt 5,520 + py 1,194 + md 38 + log 21）。【已确认，find+wc 实测】
- **性质**：versions/ 三棵 js-beautify 可读化代码树（41/41/64 文件，19M/19M/16M），其中 `appsrc/window/frame.js`（60,476 / 60,159 / 61,869 行）是 **Typora 渲染层完整业务源码**（编辑器内核 + Sentry + AWS Pinpoint + 窗口管理）；1.14.x 主进程逻辑**不在仓内**（jsc 二进制不入库，只有字符串常量产物）；0.11.18 asar_app 12 个模块是**密文**。artifacts/ 13 个证据文件；report/ 一份 13 页 PDF；scripts/ 9 py + 3 js。【已确认】
- **报告可信度**：核心结论（Sentry DSN 三版同源、1.14.x 发送回调恒 false、0.11.18 采样衰减阶梯、体积/依赖数字、SHA256）**全部在仓内证据中复核通过**；但发现 1 处数值口径错误（"差集 927 行"）、1 处路径漂移（附录 A 的 beautified/ 层不存在）、1 个实质性覆盖缺口（**0.11.18 AWS Pinpoint 遥测模块在代码树中完整在档而报告隐私章只字未提**）。见 §2.5。【已确认/高度怀疑，逐条带锚点】
- **合规**：在 git 仓持有商业闭源软件的完整 beautified 渲染层源码 = **高风险**（README:36-38 已自我声明"必须保持私有，禁止再分发"）；对 mdcore 无代码复制路径（15-benchmark:33 的行为级 CLEAN 审计属于 typora-re-analysis，**本子仓无同等合规审计文件**）。
- **与 mdcore 关系**：本仓报告的两大可用结论（1.14.x 默认零遥测、性能臃肿来自 Chromium 而非解析器）**已经由 typora-re-analysis 同结论路径被 15-benchmark 消化**（间接消化/双源互证）；Sentry 采样+脱敏设计模式、启动链路懒加载、许可体系模式三项**未被消费**（终报告零 telemetry 字样，rg 实测）。

---

## 1. 全量清单与统计

### 1.1 目录树与计数（实测，2026-11-05）

| 区 | 文件数 | 体积 | 内容 |
|---|---|---|---|
| `versions/` | 146 | 54 MB | 三版本 beautified 树：1.14.10-stable 41、1.14.1-dev-beta 41、0.11.18 64 |
| `artifacts/` | 13 | 448 KB | endpoints×3、jsc_strings×2、netlog、perf_statics、sha256、charts×2、0.11.18_protect×3 |
| `report/` | 2 | 712 KB | 13 页 PDF（717,318 B）+ cover.html（封面源） |
| `scripts/` | 12 | 216 KB | 9 py（1,194 行）+ 3 js（80 行） |
| 根 | 1 | — | README.md（38 行） |
| **合计** | **174** | **56 MB** | |

### 1.2 按扩展名（全局 / versions 内）

- js **94**（versions 90 + scripts 3 + artifacts 1[decrypted_loader_module.js]）；css **26**（全在 versions）；html **22**（versions 21 + report/cover.html）；json **14**（versions 9 = package.json×3 + asset-manifest×3 + bootstrape/config.json×3；artifacts 5）；py **9**；txt **3**；png **2**；log **2**；pdf **1**；md **1**。【已确认，实测】
- 与任务书参考量级（~94 js、26 css、22 html、14 json、9 py）**精确一致**。

### 1.3 versions/ 逐版本结构（每版本 5 个子区）

| 子区 | 1.14.10 / beta | 0.11.18 | 性质 |
|---|---|---|---|
| `asar_app/` | 2（launch.dist.js + package.json） | 13（12 个密文模块 + package.json） | 1.14.x=bytenode 引导器（明文可读）；0.11.18=自定义编码密文 |
| `asar_lib/` | 15 | 19 | 第三方库：MathJax4(2 文件)/MathJax3(9 文件)、codemirror/mode.min.js、diagram(mermaid×2)、aws-sdk、jimp、pdf-lib、pdf-designer、image-worker、bootstrape |
| `appsrc/` | 3 | 3 | frame.js（窗口层完整业务）+ finder-worker.js（搜索 worker）+ EmojiSearchMap.js（emoji 关键词数据表，1077 行三版全同） |
| `page-dist/` | 19 | 27 | React 单页骨架：5 chunk 家族 + service-worker(Workbox) + asset-manifest + runtime |
| `html/` | 2 | 2 | preview.html / content.html 窗口模板（引用未入库的 ../style/，属提取片段） |

- 各版本 JS LOC：1.14.10 **366,770** / beta **366,442** / 0.11.18 **295,573**。最大单体：mermaid.min.js 120,972 行（双 1.14.x）、frame.js 61,869 行（0.11.18）。（wc -l 实测）

---

## 2. 逐区审计

### 2.1 README.md（38 行）

- 自我定位准确：「私有研究用途，勿分发」（README.md:3）；样本三版本 + 分析日期 2026-09-30（:5-6）；核心结论速览表（:10-16）与 PDF 报告一致（复核见 §2.4）。
- 复现提示（:30-34）与仓内证据吻合：innoextract/@electron/asar 工具链、version_hash `112c8476`、0.11.18 反调试报错文案「Arguments not allowed debug in production main thread」——后者在 `artifacts/0.11.18_protect/antidebug_evidence.log:6` 有运行时实录。【已确认】
- 合规声明（:36-38）：「包含逆向衍生代码，必须保持私有，禁止再分发」——姿态正确（评级见 §4）。

### 2.2 report/（PDF 13 页 + cover.html）

- **研究目标**：三版本隐私（外联/遥测/许可）+ 性能（体积/依赖/启动链）双维审计；两阶段流水线（解包→反混淆→静态情报→动态观测→交叉验证）。
- **证据方法**（可信）：静态（beautified 树检索 + jsc 字符串常量池）+ 动态（Linux 1.14.9 + Xvfb + netlog/TCP 中继/连接轮询三通道）交叉取证；三处残余不确定性在 §7.3 明示（字符串完整性假设 / 0.11.18 止步加载器级 / Windows 路径未运行时复现）——边界诚实度高于平均水平。
- **关键结论**（我方逐条复核，见 §2.4-2.5）。
- cover.html：纯封面样式源（794×1123 设计稿），非证据文件，结构检视即止。

### 2.3 versions/ JS 性质逐目录裁定

| 目录 | 裁定 | 证据（路径:行号，引用≤3行） |
|---|---|---|
| `appsrc/window/frame.js`×3 | **Typora 渲染层完整业务源码**（非片段）：编辑器内核（contenteditable+块 cid+CodeMirror fence 懒挂载+fullValidate）、Sentry 装配、0.11.18 另含 AWS Pinpoint 模块 | CodeMirror fence 集成 `1.14.10-stable/appsrc/window/frame.js:1305`；fullValidate 17 处命中/版本；Sentry DSN `frame.js:16666`（stable）/`:16401`（beta）/`:19219`（0.11.18）同一 DSN `https://41af...@sentry.typora.io/2` |
| `appsrc/editor/EmojiSearchMap.js`×3 | 数据表（emoji→关键词），非逻辑 | `1.14.10-stable/appsrc/editor/EmojiSearchMap.js:1-3` `window.EmojiSearchMap = { 0: ["zero"]...` |
| `appsrc/finder-worker.js`×3 | 文件搜索 worker（明文小件） | `finder-worker.js:5-9`（supportTextBundle/_MAX_COUNT=600 等常量） |
| `page-dist/static/js/*`×3 | React 应用 webpack chunk（beautified）：Preferences 12,580 行、main 50 行（懒加载壳） | `1.14.10-stable/page-dist/static/js/main.fb2bf03d.js:1-13`（webpack runtime）；PicGo/piclist 端点在 Preferences chunk（32 处命中） |
| `asar_app/`（1.14.x） | bytenode 引导器（明文），**主进程业务逻辑不在仓** | `1.14.10-stable/asar_app/launch.dist.js:1-40`：`Module._extensions[".jsc"]... vm.Script(o,{cachedData:a})`；package.json `"main": "launch.dist.js"`（`asar_app/package.json:4`） |
| `asar_app/`（0.11.18） | **密文**（自定义编码），无信息增量 | `0.11.18/asar_app/License.js:1`（`PAsps9R2ALkYHXZXs8 + VU + QFi3...` base64 族密文）；package.json `"main": "main.node"`（`asar_app/package.json:4`） |
| `asar_lib/`×3 | 第三方 OSS 库打包（MathJax/CodeMirror/mermaid/aws-sdk/jimp/pdf-lib/bootstrap）；pdf-designer 归属不明 | jimp MIT 头在档 `asar_lib/jimp/browser/lib/jimp.min.js:1-4`；pdf-designer 无许可头（`pdf/pdf-designer.min.js:1` 起 `function PDF_SeekPos`） |
| `html/`×3 | 窗口/导出模板片段 | `1.14.10-stable/html/content.html:2-10`（解析 `--tyopt=` argv 注入 window._options） |

- **主进程情报缺口**：1.14.x 的 `atom.compiled.dist.jsc`（388KB 字节码）按报告附录 A「二进制不入库」原则未提交，主进程逻辑仅有 `artifacts/jsc_strings_*.txt` 常量池快照——即本仓对主进程是**字符串情报级**而非源码级。【已确认】

### 2.4 报告结论逐条复核（证据锚定）

| # | 报告结论 | 仓内复核 | 判定 |
|---|---|---|---|
| 1 | 三版硬编码同一自建 Sentry DSN（sentry.typora.io/2） | frame.js:16666/16401/19219 三处同串 | 【已确认】 |
| 2 | 1.14.x 发送回调恒 false（断流） | `1.14.10-stable/.../frame.js:16668-16669` `setShouldSendCallback(function(e){ return File.isMac, !1 })`；beta `:16403-16405` 同款 | 【已确认】 |
| 3 | 0.11.18 采样阶梯 10%→3%→2%→1%→0.1%、45 天 tooOldToReport | `0.11.18/.../frame.js:19207`（3456e5/6048e5/1296e6/2592e6/3888e6 五阈值链）、`:19204`（tooOldToReport 读取）、`:19222-19226`（`Math.random() < i` 发送门） | 【已确认】 |
| 4 | 上报前路径脱敏 {filepath} | `0.11.18/.../frame.js:19192-19194`（`replace(...(md\|mmd\|markdown...)/g, "{filepath}")`） | 【已确认】 |
| 5 | 许可体系：store.typora.io activate/deactivate + node-machine-id + 12h 续约 + 超额注销 | `artifacts/jsc_strings_stable_1.14.10.txt:1458-1459`（api/client/activate\|deactivate）、`:2152`（node-machine-id）、`:1361`（license renewed in 12h）、`:1328`（exceeded the max devices numbers） | 【已确认】 |
| 6 | 更新通道模板 /releases/{dev_}windows_ | jsc_strings_stable:478-479 | 【已确认】 |
| 7 | 安装包 SHA256 三条 | `artifacts/sha256.txt:1-3` 全文三串，前 16 位与报告表 2-1 一致 | 【已确认】 |
| 8 | 体积/依赖：405.3/413.6/228.1 MB、411/413/375 文件、asar 三元组、nm 22/23/20 | `artifacts/perf_statics.json` 逐值一致（python json 解析实测） | 【已确认】 |
| 9 | jsc 去重字符串 2,749/2,768 条 | 两 txt 行数=去重数=2749/2768（sort -u 复核） | 【已确认】 |
| 10 | 图片上传端点 PicGo-cli/piclist.cn（Preferences 包） | Preferences chunk 32 处命中（grep 实测） | 【已确认】 |
| 11 | 空闲 9 分钟 0 外联（仅 gvt1.com 拼写词典） | `artifacts/netlog_1149_requests.json` 仅 2 条记录且均为 redirector.gvt1.com/.../en-us-10-1.bdic | 【已确认，但样本仅 1.14.9 Linux，见 §2.5-D】 |

### 2.5 报告质量问题（本审计新增发现）

- **A｜"差集 927 行"口径错误**【高度怀疑→已确认为口径漂移】：报告第 4 章（build_report_body.py:342）称两版本字符串"差集 927 行"。实测：`diff a b | wc -l` = 927（含 98 个 hunk 头 + 46 个 `---` 分隔行）；**真正内容差集 = 783 行**（beta 独有 401 + stable 独有 382，comm 实测）。927 是 diff 输出行数被误当差集行数，虚高 18.4%。
- **B｜`File.toggleReadonlyMode` 方向反了**【已确认】：报告将其列为差集中"可辨识的业务增量"（暗示 beta 新接口）；实测该串在 stable（jsc_strings_stable_1.14.10.txt:131）而 **beta 为 0 命中**——它是 stable 侧增量（1.14.10 > 1.14.1-dev 版本序），报告未标方向。
- **C｜附录 A 路径漂移**【已确认】：报告（build_report_body.py:492-494）写 `versions/1.14.10-stable/beautified/`，实际仓内无 `beautified/` 层（直接是 `versions/1.14.10-stable/asar_app` 等）——报告索引不可按图索骥。
- **D｜实质性覆盖缺口：0.11.18 AWS Pinpoint 遥测模块未入报告**【已确认存在，活跃性需真机验证】：`0.11.18/appsrc/window/frame.js:19333-19455` 完整在档——硬编码 Cognito IdentityPoolId（:19344 `us-west-2:146e5a95-...`）、`new AWS.Pinpoint`（:19349）、`updateEndpoint`/`putEvents`（:19437/:19455）、门控 `File.option.sendAnonymousUsage`（:19137 `a = () => !window.debugMode && ... sendAnonymousUsage`）、macOS 侧 `Analytics.record` bridge（:19353）。报告第五章只覆盖 Sentry，**对 Pinpoint 零提及**；该通道由 typora-re-analysis/analysis/privacy_findings.md 补位（15-benchmark:158 引证「0.11.18 AWS Pinpoint 默认开」）。本子仓报告的"0.11.18 隐私画像"因此不完整；其 9 分钟动态观测跑在 1.14.9 Linux 上，对 0.11.18 无动态发言权。
- **E｜"脚本可独立复跑"不成立**【已确认】：beautify_all.py:5-6、extract_endpoints.py:5、perf_statics.py:5、make_charts.py:11、cdp_fast.py:7、race_capture.py:7 均硬编码 `/home/z/my-project/typora_analysis`（原始工作区绝对路径），且依赖不入库的 `extracted/` 树——在本仓（mang-research-archive）内不可原位复跑。
- **F｜报告数据链是手抄而非管道**【已确认】：build_report_body.py 全部表格/叙事为字面量内联（:263-265、:431）；make_charts.py:22-23 图表数据同样手工内联（71.4/116.5/113.6 与 228.1/413.6/405.3）。与 perf_statics.json 的一致性靠人肉对齐（本次抽验全对，但无再生成保障）。
- **G｜次要**：报告称 appsrc 下 frame.js 为 1.7MB（原始压缩态），beautified 后实为约 2.4MB/60K 行——口径未注明。【设计偏好级瑕疵】

### 2.6 artifacts/ 审计

| 文件 | 内容 | 质量备注 |
|---|---|---|
| endpoints_stable/beta/old.json | 各 400 URL + 120 域名 + url_file_sources | **含噪声**：DOMAIN_RE（extract_endpoints.py:10）把 minified DOM 属性当域名——endpoints_old.json top_domains 含 `n.top`/`t.top`/`e.top`（`x=e.top` 类代码）；apache.org/crates.io 等为捆绑 OSS 库许可证 URL 混入；报告表 5-1 是人工过滤后的子集【已确认】 |
| jsc_strings_*.txt | 2749/2768 条去重串 | 与报告数字精确一致；beta 独有行多为二进制噪声串（` Pf*...` 族） |
| netlog_1149_requests.json | 2 条请求（gvt1 词典 ×2） | 与"0 外联"结论一致；但仅覆盖 1.14.9 Linux 会话 |
| perf_statics.json | 三版本体积/asar/nm 清单 | 与报告表 6-1 逐值一致 |
| sha256.txt | 三安装包全哈希 | 与报告表 2-1 一致 |
| 0.11.18_protect/ | decrypted_loader_module.js（16,427 字符单行，CDP 竞速抢到的 require 链引导模块）+ antidebug_evidence.log（17 行真实栈：`Arguments not allowed debug...` at atom.js:1:586）+ bootstrap_trace.log（4 行） | 加载器含反调试检查与 `Raven=require("raven")`、`app.setting.addAnalyticsEvent("openFolder")` 等主进程线索——报告未深挖 addAnalyticsEvent（本地分析事件 vs 外发的区分未做）【高度怀疑有未消费情报】 |
| chart_sizes.png / chart_sentry.png | 报告图 6-1 / 5-1 | 源数据手工内联（§2.5-F） |

---

## 3. scripts/ 审计（9 py + 3 js）

**py_compile 结果：9/9 通过**（`python3 -m py_compile`，2026-11-05 实测；race_capture.py 产生 1 条 SyntaxWarning，见下）。未运行任何脚本本体。

| 脚本 | 行数 | 用途 | 质量/正确性问题（文件:行号） |
|---|---|---|---|
| beautify_all.py | 82 | js-beautify 批量美化三版本 | :57-58 `os.path.join(res, root)` 双拼——因 root 为绝对路径而侥幸无副作用（os.path.join 绝对参数弃前缀），脆弱写法；:69 >25MB 跳过美化直接拷贝（合理降级） |
| extract_endpoints.py | 68 | URL/域名/关键词情报提取 | :10 DOMAIN_RE 无上下文过滤→`e.top` 类假阳性（§2.6）；:56-58 top 400/120 截断（情报有上限损失） |
| build_report_body.py | 503 | ReportLab 生成 PDF 正文 | 数据全字面量内联（§2.5-F）；:17-28 字体路径硬编码；:492 附录路径与实仓不符（§2.5-C） |
| make_charts.py | 69 | matplotlib 双图 | :22-23 数据手工内联，未读 perf_statics.json（双源漂移风险） |
| cdp_fast.py | 72 | CDP 快速附加+argv 清洗+scriptParsed 抓源 | :44 `synchronousStartup` 非 CDP Runtime.evaluate 合法参数（无害被忽略）；:66 `maxScriptsCacheSize: 1e8` 浮点传参（协议要整数） |
| race_capture.py | 130 | 0.11.18 反调试竞速（spawn+pause+patch+采集） | **:68 SyntaxWarning**：`url in ("", "fs.js", ...) is False`——Python 链式比较使其恒为 False，过滤实际退化为 `"app.asar" in url`，后半分支为死代码（不影响主目标但意图未实现）；:42 采集≥40 即停/主循环≥10 即断（阈值魔法数） |
| relay_logger.py | 125 | 透明 TCP 中继记录 CONNECT/明文请求行 | :10-15 `sniff_clienthello` 死代码且 :13 `readexact` 拼写错（应为 `readexactly`）——函数从未被调用，SNI 提取名存实亡；:106 请求行重写逻辑对绝对 URI 的 path 重组较粗糙 |
| perf_statics.py | 64 | 体积/依赖静态指标 | 基本正确；依赖 GNU `du -sb`（:11）；:50-53 Linux 版本文件探测仅在原工作区有效（§2.5-E） |
| parse_netlog.py | 81 | netlog 事件→URL_REQUEST 汇总 | :14-24 逐行 JSON 解析假设 netlog 为 pretty-print 格式（若单行压缩则全漏）；:39/:42/:50 事件类型常量（54/1/90）硬编码，随 Chromium 版本漂移即失准 |

JS 三件（静态阅读，未执行）：runner_0118.js（21 行，require 原始 asar 触发密文模块加载的探针）；probe_sourcehash.js（19 行，实证 V8 源哈希=纯长度语义：任意 237,670 码元同哈希）；debug_load.js（40 行，cachedDataRejected 三变体诊断）——与报告 §4.2 的"源哈希长度语义/反序列化层拒绝"叙述**互相印证**。【已确认】

---

## 4. 合规风险评级（仅风险陈述，非法律意见）

| 持有物 | 风险 | 理由 |
|---|---|---|
| versions/appsrc/frame.js×3 + page-dist chunks（Typora 私有渲染层完整可读源码） | **高** | Typora 商业闭源，EULA 限制逆向（报告 §2.2 自认）；beautified 形态=受版权表达的全量还原，在 git 仓（含远端）持有即有再分发暴露面；README:36-38「必须保持私有」是对的姿态但不是许可 |
| 0.11.18 asar_app 12 个密文模块 | 中 | 不可读、无信息增量，但仍是受版权材料本体 |
| asar_lib 树 | 低-中 | 多为 MIT/Apache OSS（jimp MIT 头在档 jimp.min.js:1-4）；但仓内无任何第三方许可声明文件（NOTICES 缺位），再分发时连 OSS 合规也不完整；pdf-designer.min.js 无许可头、归属不明【高度怀疑 Typora 自有代码，按高对待直至查明】 |
| artifacts（端点/字符串/日志） | 低-中 | 事实性情报（URL/域名/常量）保护弱，但字符串字面量仍源自受版权代码 |
| 对 mdcore 的传染 | **零（现状）** | mdcore 为 Rust/TS 独立实现，audit/ 全系 notes 无一处引用本子仓代码（rg "typora-analysis" 于 mdcore-audit/audit 零命中）；行为级洁净审计（AUDIT_24_BEHAVIORAL 四层 CLEAN）在 typora-re-analysis 侧在档（15-benchmark:33），**本子仓无同等合规审计文件——建议补做** |
| 远端暴露面 | 【需真机验证】 | 报告附录 A 称产物已推送 `mmqz/typora-analysis` 私有仓；mang-research-archive README:44 以 subtree 并入、:54-59 称原仓已 Archive。GitHub 上 Archived≠Private——远端可见性未经本审计验证（无网络纪律），若任一环节为公开仓即触发自设红线 |

**总评**：私有研究持有 + 结论层消费的用法风险可控；风险集中在①远端可见性未证实、②本子仓缺行为级合规审计、③pdf-designer 归属不明三点。

---

## 5. 与 mdcore 的关系（对照 15-benchmark.md）

> 通道验证：archive README:34「Typora 分析 → Markdown 编辑器的设计参考」。本审计以 `audit/notes/15-benchmark.md`（D2 竞品对标证据档案）为消化判据；mdcore-audit/audit 全系 rg「sentry/遥测/隐私」仅命中 15-benchmark 自身（08-canvas 一处无关命中）——终报告零 telemetry 字样。

| 本仓报告结论 | 对 mdcore 的价值 | 消化状态（判据） |
|---|---|---|
| 1.14.x 渲染层崩溃上报断流（发送回调恒 false）＝商业编辑器默认零遥测终态 | 产品化遥测姿态先例 | **间接消化**：同结论经 typora-re-analysis/privacy_findings.md 路径入 15-benchmark:158/:245（「商业对手把默认零遥测当终态」）；本仓为**独立第二源**（frame.js:16668 一手代码证据），15-benchmark 未标双源 |
| 性能臃肿来自 Chromium/运行时代际（Electron 13→42，228→405/414MB）而非 Markdown 解析 | 「视图层自研负 ROI」佐证 | **间接消化**：同结论经 typora-re-analysis 主档 §4.2 入 15-benchmark ④-3/2.1；本仓的量化分解（asar 三元组/nm 20→22/主进程反缩至 388KB）**未被引用**——是 15-benchmark 弱点 2（性能量级多为【推断】）本可补的实测数字 |
| Sentry 采样衰减+{filepath} 脱敏+面包屑全关的装配设计 | 若 mdcore 未来引入 opt-in 崩溃上报，这是小型化/隐私优先的成熟模式（采样率随龄期衰减、macOS 除外、上报前脱敏） | **未消化**（mdcore 零遥测设计下暂不需要；登记为 P3+ 备用参照，对应 15-benchmark ⑧-2「若引入遥测时的合规底线参照」族的补充件） |
| 启动链路：主进程反序列化→IPC/窗口→渲染层；MathJax 启动后约 1s 装载、aws-sdk 按需 getScript、重型组件懒初始化 | 重型引擎懒加载/按需装载的设计参考 | **未显式消化**：mdcore web 侧 pkg_pdf 可选构建（01-structure.md:42）与该结论同向，但无文档链路；属【设计偏好】级互证 |
| 许可体系模式（machine-id 哈希派生/activate-deactivate/12h 续约/超额注销最旧设备/事件驱动外联） | mdcore 若商业化收费的许可面参照 | **未消化、暂不相关**（mdcore 无许可系统） |
| bytenode/V8 字节码、0.11.18 原生加密反逆向、版本指纹 | 与 mdcore 产品无关 | 不适用（对「是否给 mdcore 上代码保护」的负案例价值未登记） |
| AWS Pinpoint 门控 sendAnonymousUsage（本审计新发现的仓内一手材料） | 「遥测必须用户开关门控」的又一先例（与 Acrobat SKIP_GC 开关同族） | **未消化**（本审计首次披露于仓内；建议随 ⑧-2 遥测先例链一并登记） |

**判定汇总**：本子仓对 mdcore 的实际贡献通道 = 经 typora-re-analysis 的结论层转发（2 项间接消化）+ 独立双源互证价值（未标注）；量化性能数据、Sentry 设计模式、Pinpoint 门控三项为**未回收材料**。15-benchmark §1.1 的三仓表应补第四行（typora-analysis：隐私/性能一手双源 + 量化数据源）。

---

## 6. 结论与覆盖声明

### 6.1 结论

1. 该子仓是**结构完整、方法可信的隐私/性能逆向审计工作区**：报告核心结论 11/11 在仓内证据复核通过（§2.4），证据链（sha256→perf_statics→frame.js 锚点→jsc 字符串）闭环。
2. **四项报告质量缺陷**：差集 927 口径错（实 783）、toggleReadonlyMode 方向反、附录 A 路径漂移、「可独立复跑」不成立（§2.5 A-C/E）。
3. **一项实质覆盖缺口**：0.11.18 AWS Pinpoint 遥测模块完整在档而报告零提及（§2.5-D）——该版本的隐私画像不完整，幸有兄弟仓补位。
4. 脚本层 9/9 可编译，但普遍是**一次性研究脚本质量**：硬编码绝对路径×6、死代码×2、链式比较 bug×1、域名正则噪声、netlog 常量漂移风险（§3）。
5. 合规姿态正确但留三个开口：远端可见性未证实、本子仓无行为级合规审计、pdf-designer 归属不明（§4）。
6. 对 mdcore：结论层已通过 typora-re-analysis 通道消化 2 项，本仓的独有增量（量化性能数据、Sentry/Pinpoint 设计模式）尚未回收（§5）。

### 6.2 覆盖声明

| 覆盖面 | 比例 | 说明 |
|---|---|---|
| 文件清点/分类/统计 | **174/174 = 100%** | find/wc/du/du -sh 实测，含每目录性质裁定 |
| README + report PDF | **100%**（38 行 + 13/13 页） | PDF 全页解析；cover.html 结构检视（封面源非证据件） |
| scripts/ | **12/12 = 100%** 全文通读 | 9 py 逐行 + py_compile 9/9；3 js 逐行静态阅读 |
| artifacts/ | **13/13 结构级**；其中 7 个全量读取（sha256/netlog/perf/2 log/loader/cover 级） | endpoints×3 键结构+定向检索；jsc_strings×2 头部+定向 grep+差集复算 |
| versions/ | 目录级 100%（146/146 分类）；**逐行精读 21/146 ≈ 14% 文件**（frame.js×3 关键窗口、launch.dist.js、package.json×3、finder-worker、EmojiSearchMap、main chunk、content.html 等） | 其余 125 个为第三方 OSS 库/React chunk/密文——按性质裁定抽样（asar_lib 头部抽 5 件验许可）；关键证据锚点验证约 35 处 |
| 动态行为 | 0%（纪律：不执行仓内二进制/脚本） | 动态结论仅采信仓内既有实录（netlog/antidebug log） |

### 6.3 建议后续动作（不属本轮执行）

- 15-benchmark §1.1 补登 typora-analysis 第四行；⑧ 未用材料表补「量化性能数据 + Sentry/Pinpoint 设计模式」两项。
- 对本子仓补一轮 typora-re-analysis 式行为级合规审计（名字/字面量/结构/常量/反编译产物五维），并核实 mmqz/typora-analysis 远端可见性【需真机验证】。
- 报告若再版：修 §2.5 A-C 三处数字/路径；补 0.11.18 Pinpoint 章节并标注 sendAnonymousUsage 门控的默认值验证需求。

---
*证据口径：所有 路径:行号 为 2026-11-05 工作区实测；统计命令 find/wc/du/sort -u/comm/diff；PDF 经页级解析。本笔记为零写入审计的唯一产出（写入本文件本身）。*
