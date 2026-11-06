# 审计笔记 26：研究仓 acrobat-pro-audit 全量审计

> Task ID：R5-c（mdcore 全仓代码审计 · 研究仓全覆盖系列）。日期：2026-11-05。
> 审计对象：`/home/z/my-project/mang-research-archive/acrobat-pro-audit`（Adobe Acrobat
> Pro DC 26.002.21931 安装包架构级审计：数据库取证 / 安装行为 / 网络遥测 / 二进制
> 能力面 / AGS 取证）。只读审计：未执行任何 .exe/.msi/二进制（仅 ls/sha256sum 指纹）；
> Python 仅 py_compile；未使用网络；未做任何写入型 git 操作。
> 交叉参照：`audit/notes/07-pdf.md`（mdcore pdf 线审计）、
> `mang-research-archive/cleanroom-analysis/md-line/pdf-a/SECURITY_POLICY.md`（映射链）。

## 0. 总览事实（一分钟版）

- 仓库 82 文件 / 26MB：evidence 4.3MB（48 json）+ source 22MB（**11 个 Adobe 二进制
  共 21,976,736B，占全仓 84%**）+ analysis 68KB（6 md）+ tools 80KB（8 py + 1 ps1 + 1 sh）。
- 全部核心数字声明经实测**逐项吻合**：11 张表行数（313/336/252/7/6/806/2308/3306/
  5280/39/78）、138 流 / 13,050,541B、36 PE 零网络零注入、x64=15/x86=21/签名 19、
  节熵最大 6.62 < 7.2、55 EXE-DLL（5 exe + 50 dll）、自启动命中 30、SKIP_GC 条件。
- 全部 14 个 sha256 实测与仓内登记（`source/artifact-hashes.txt` + 证据 JSON 内嵌
  哈希）一致——**证据链哈希自洽闭合**。
- 发现两处实质质量问题：① `tools/zip_cab_index.py` 的 MSCF 头解析有字段错位 bug，
  其产物 `cab_index.json` 显示 6 个 CAB 全部 `indexed=1`、`gc_hits` 全空——WORKLOG
  R6「CAB 的 CFFILE 索引扫描（定位 AGS）」的叙述与所提交证据不符（§4.4）；
  ② `_behavior_summary.json` / `msi_manifest.json` 是早期会话脚本代次产物，与仓内
  最终版 `msi_audit.py` 的输出 schema 不一致——README「复现一致性」声明仅对 11 张
  表成立，对汇总/清单文件不成立（§4.2）。
- 与 mdcore 的映射链完整可溯：`SECURITY_POLICY.md` 明文引用 acrobat-audit R1-R6
  作为 S1.1/S1.4/S4.1/S4.5 的输入资产；mdcore `crates/pdf` 已实装对应策略
  （07-pdf.md §5）。未采纳面：企业策略治理层（FeatureLockdown 等价物）与签名
  密码级验证（§7）。

## 1. 全量清单与统计

### 1.1 目录树与文件类型计数（find 实测）

```
acrobat-pro-audit/          82 文件 / 26MB (du)
├── README.md  WORKLOG.md  .gitignore        3 文件
├── analysis/  6 md（01 数据库逆向 ~ 06 AGS取证）        68KB
├── evidence/  48 json / 4.3MB
│   ├── _behavior_summary.json                58,729B
│   ├── msi-tables/          16 json / 2.8MB（11 表 + manifest/meta/streams/summary/mining）
│   ├── pe-capability/       10 json / 252KB（36PE 能力 + AGS/ARM 载荷审计）
│   ├── ags-forensics/       2 json（ZIP 中央目录清单 + CAB 索引）
│   └── external-research/   19 json / 1.3MB（检索快照，2026-10-09）
├── source/                 15 文件 / 22MB（11 二进制 + 2 INI + 哈希清单 + dispatchtable）
└── tools/                   10 文件 / 80KB（8 py + Collect-AdobeAutostart.ps1 + push_to_github.sh）
```

类型计数：json 48（16+10+19+2+1）、py 8、md 8（README+WORKLOG+6 analysis）、
二进制 11（exe 6 + msi 5）、其他 5（.gitignore / ABCPY.INI / setup.ini /
artifact-hashes.txt / gc_dispatchtable XML）。任务书给定基线「48 json + 8 py + 8 md
+ 6 exe + 5 msi」与实测完全一致。`.gitignore` 排除 `msi_streams/` 与 `__pycache__/`
——13MB 流转储未入库（正确取舍）。

### 1.2 二进制体积（du / ls 实测，未执行）

| 文件 | 大小 (B) | sha256（实测前 16 位） |
|---|---|---|
| source/AcroPro.msi | 13,197,312 | `089dc9a23f5eb868…` |
| source/setup.exe | 678,872 | `8cd5ae216bb155ac…` |
| ags/AdobeARM_1824214663.msi | 875,520 | `22831a2a10c7357a…` |
| ags/gc/AGSService.exe | 3,103,824 | `1d61549a6e76adb4…` |
| ags/gc/gccustomhook.exe | 2,406,888 | `f514f27f1608236d…` |
| ags/arm_payload/adobearm.exe | 1,160,408 | `232400b5017dab58…` |
| ags/arm_payload/adobearmhelper.exe | 421,608 | `ef93c3ce36b58f02…` |
| ags/AcrobatDCManifest.msi | 20,480 | `a6ef625c83bcfcf4…` |
| ags/ArmManifest.msi | 14,848 | `c50dbe7996899d8f…` |
| ags/AcroManifest.msi | 14,336 | `a476e824c2d64b37…` |
| ags/arm_payload/armsvc.exe | 82,640 | `a36cc49a0c829a5c…` |

合计 21,976,736B（≈21.0MiB）。另有 gc_dispatchtable（XML，`e79325adef1e06f2…`，
含 DTSignature 签名保护的 AGS 调度表）。文本件：ABCPY.INI 644B、setup.ini 1264B
（与 `acro_zip_manifest.json` 中 ZIP 条目 usize 逐项一致）。

## 2. README / WORKLOG 方法学审计

**审计目标**（README.md:3）：对官方 Acrobat Pro DC 26.002.21931 安装包做数据库级
逆向 + 安装行为 + 网络遥测 + 二进制能力面 + AGS 运行期取证，回答「签名合法的官方
安装器，会对系统做什么、向谁报告」。六轮时间线（WORKLOG.md:5-62）：R1 MSI 数据库
解码（Task 4）→ R2 安装行为面（Task 5）→ R3 网络遥测面（Task 6）→ R4 仓库整合
（Task 7）→ R5 二进制能力面（Task 8）→ R6 AGS 取证（Task 9-10，提交 ded0987）。

**方法学评价**：
- 证据纪律声明明确且大体落实：「每条结论可回溯到具体流/表/字符串/检索记录」
  （README.md:60）。本轮抽样核对 20+ 条定量结论，除 §4.4 一处叙述-证据不符外全部
  可回溯。【已确认】
- 纯 Python 逆向（无 msitools/7-zip/sudo 环境约束促成格式级取证）：字符串池格式由
  唯一闭合数值检验锁定（Σ长度 781,290B == StringData 字节数，msi_audit.py:114 断言
  在位）；表身份由内容特征判定绕过 InstallShield 流名混淆——方法正当且诚实声明了
  未破解混淆算法本身（analysis/01:13, §57）。【已确认】
- 诚实记录三次自我修正：R5 修正 R1 的两处 PE 身份误判（analysis/05:43-49）；R6 修正
  R2 的「AGS 不在静态载荷」为「在包内 CAB、经条件组件安装」（analysis/06:42）；
  自研验签脚本对 32 位 PE 误报由 signify 复验并记录（analysis/06:19）。这种自我纠错
  是研究仓可信度的加分项。【已确认】
- 局限：WORKLOG 无失败/空转轮记录（六轮全部「成功」叙事）；检索快照无检索工具与
  查询串元数据（见 §4.5）。

## 3. analysis/ 六篇结论登记与证据强度

| # | 结论（analysis 篇） | 证据锚 | 强度评级 |
|---|---|---|---|
| 01 | 138 流/13,050,541B；字符串池 29,429 条×4B；列块 column-major 布局；11 表全量重建 | msi_manifest.json 求和实测=13,050,541 ✓；_summary.json 行×列=细胞数全部闭合 ✓ | 【已确认】 |
| 01 | CustomAction/File 存在变长列异质段，逐行对齐不完美，聚合可信 | Component.json:15911（KeyPath/Condition 互换）、CustomAction.json:283（Source/Target 错位）实测可见错位痕迹 | 【已确认】且局限声明诚实；但错位实际也出现在 Component 表（声明未覆盖，见 §4.3） |
| 02 | 默认安装常驻清单：AdobeARMservice（SYSTEM）+ Update Task + Run 键 Acrotray + CRClient WER 挂接 + ElevationPolicy(AdobeCollabSync) | ServiceControl.json:AdobeARMservice Event2/128；_behavior_summary.registry_autostart 30 行含 Registry1065（Run/Acrotray）、Registry29（ElevationPolicy）、`_312C81860F04…`（RuntimeExceptionHelperModules→CRClient.dll, 组件 Ex_Bin_CR_CRClient） | 【已确认】表级；计划任务仅字符串证据（ArmInst AddTask/RemoveTask 导出 + 任务名），实机形态属【需真机验证】 |
| 02 | 3,306 注册表写入 / 5,280 文件 / 55 EXE-DLL | Registry.json rows=3306 ✓；File.json rows=5280 ✓；扩展名精确计数 exe=5+dll=50=55 ✓ | 【已确认】 |
| 03 | 安装器静态载荷唯一行为端点族 = armmf.adobe.com（4 清单 + 3 自更新 MSI），72h 周期 | Property.json 实测：ARMManifestLocation×3+OnDemand=4、ARM_Update×3、ARMCheckFrequency=72 ✓；msi_strings_mining 全部非证书 URL 仅 clients2.google.com(s023)+flexerasoftware(s035/s059) ✓ | 【已确认】 |
| 03 | AGS 仍活跃、社区高 CPU 投诉持续、卸载链不完整 | external-research 19 json 检索快照（ags_eol/gcclient/update_task 等抽样内容与叙述吻合） | 【已确认】为检索时点（2026-10-09）快照，非持久结论 |
| 04 | 风险定级：安装器低 / ARM 中 / AGS 中 / 运行时遥测中 | 定级推理链引用 01-03+05-06 全部本地证据 | 【已确认】（推理自洽）；处置矩阵（FeatureLockdown/SKIP_GC/防火墙收敛）属【设计偏好】 |
| 05 | 36 PE 零网络导入、零注入 API、节熵全部 <7.2、时间戳 2012/2021 两分群 | capability_matrix.json 实测：proc_inject=0、net_*=0、最大熵 6.62、x64=15/x86=21/签名 19、能力分布 12/3/12/6/6/4/1/19 与 analysis/05 §3 表逐项一致 | 【已确认】 |
| 05 | Chrome 扩展 + 原生消息宿主注册（com.adobe.acrobat.chrome_webcapture） | strings_hits s023 clients2.google.com/service/update2/crx + Registry 表交叉 | 【已确认】静态注册面；实际运行形态【需真机验证】 |
| 06 | SKIP_GC=1 官方 AGS 阻断开关 | Component.json:15907-15919（GC_AGSService/GC_CustomHook 条件 "Not SKIP_GC"）；IES.json:543-544（ExecuteGCHook 条件含 Not (SKIP_GC)，序 6130） | 【已确认】静态条件证据；实机阻断效果【需真机验证】 |
| 06 | AGS 上报面：genuine.adobe.com/server/services/{check,machineevents,…} + na1e.services.adobe.com；MACAddress 指纹 | gc_dispatchtable XML 实读：PR/GM 两 scope 各 9 端点与 analysis/06 §4 表逐字一致、DTSignature 在位；ags_AGSService.exe_strings.json keyword_w 含 "MACAddress" ✓ | 【已确认】 |
| 06 | ARM→AGS 全链验签（authentihash 匹配） | 仓内无验签输出 JSON——验证结论依赖 WORKLOG 记录与 signify 运行（工具在位可复跑） | 【高度怀疑→待复跑】静态哈希自洽但验签过程证据未落盘 |

**总体**：六篇分析的定量结论与 evidence 的吻合度异常高（本轮 20+ 项抽检零数字
漂移），叙述克制（区分静态证据/外部情报/机制假设三层，如 CPU 异常明确标注
「静态证据不能证明卡顿……待实机验证」，analysis/06:87）。证据强度评级：
安装器面结论 = 强；AGS 运行期行为 = 中（静态+检索）；处置建议 = 指南性。

## 4. evidence/ 质量核对（48 json）

### 4.1 Schema 总描述

- **msi-tables/（16 件）**：11 张表 = 对象数组，字段名即 MSI 列名（如 Registry 表
  {Registry,Root,Key,Name,Value,Component_}），整数列已数值化（Root=2、Event=128），
  NULL 保留为 null——schema 干净统一。Binary.json 为 39 字符串数组（旧代次导出，
  见 4.2）。辅证 5 件：msi_manifest（138 流清单：idx/ole_name/size/class/sha256_16）、
  msi_pe_meta（36 PE 版本身份）、msi_streams（138 流魔数/头部 hex）、
  msi_strings_mining（36 PE 的 urls/domains/svc 分类命中）、_summary（表规格）。
- **pe-capability/（10 件）**：pe_capabilities（36 条全量：sha256/机器/时间戳/节熵/
  导入/导出/版本块）；capability_matrix（36 条轻量矩阵+敏感组合判定）；
  strings_hits（ASCII+UTF16LE 双通道挖掘）；ags_/arm_ 系 6 件复用同一 PE 审计核心
  （arm_payload_audit.py 直接 import msi_pe_audit.audit_one）。
- **external-research/（19 件）**：搜索引擎结果快照数组（{url,name,snippet,rank,
  date}），adobe_endpoints.json 863KB / pdt.json 330KB 为官方文档整页抓取。
- **ags-forensics/（2 件）**：acro_zip_manifest（1.58GB ZIP 中央目录 39 条目，
  total=1,696,202,212B 与 README「1.58GB」一致；条目 usize 与本地 source/ 文件
  逐项吻合）+ cab_index（**质量缺陷件**，见 4.4）。

### 4.2 内部逻辑自洽性核对（深读 15 件，远超 ≥8 要求）

核对结果：msi-tables 16 件全部行数核对通过；pe-capability 深读 7 件；ags-forensics
2 件全读；external-research 深读 3 件+2 大件 schema 抽样；_behavior_summary 全读。
哈希交叉：AGSService.exe 实测 sha256 `1d61549a…` == ags_binaries_audit.json 内嵌
值 ✓；adobearm.exe 实测 `232400b5…` == arm_payload_audit.json 内嵌值 ✓；
AcroPro.msi 实测 == analysis/01 头部声明 `089dc9a2…60f4b2d` ✓。

**发现 A（代次混杂）**：`_behavior_summary.json` 的键为 binary_keys /
serviceinstall_cells / ca_by_binary_source（**空对象**）/ ies_binary_cas（**空数组**）/
registry_autostart / file_ext_dist——而仓内最终版 `msi_audit.py:185-197` 生成的是
servicecontrol / custom_action_count / ca_type_distribution / ca_source_counts /
ies_action_count / registry_rows / registry_autostart_hits / binary_keys。两套 schema
不同名、两个空字段残留（疑似旧版工具在此处未能填充）。同理 `msi_manifest.json`
的流分类含 `POOL?`/`TABLE?` 标记（138 = bin49+POOL?19+TABLE?27+PE36+JPEG1+BMP3+
ASCII3），而现行 msi_audit.py:42-54 只产 bin/PE/BMP/JPEG/CAB/ASCII。结论：
**README.md:56「11 张表与首次导出 10 张字节级一致」的复现声明范围仅限表 JSON；
汇总/清单类证据是早期代次产物，不能由仓内最终工具按原样再生**。对结论可信度
影响有限（表数据本身被两边一致确认），但「可复现」表述应以范围限定。【已确认】

**发现 B（列错位的直接样本）**：serviceinstall_cells 16 细胞中第 4-8 位为
"The Signature_ represents…"（MSI 文档串污染）、"Table"、0、0、0——ServiceInstall
流的手工解码同样存在 StringPool 串扰；分析篇只取语义正确的字段（服务名/显示名/
描述/组件），未声明此表也有噪声。Component 表 GC 行 KeyPath/Condition 互换、
CustomAction ExecuteGCHook 行 Source/Target 错位（§3 表内登记）。这些错位与
01 篇声明的「CustomAction/File 两表对齐不完美」相符但范围被低估——Component/
ServiceInstall 实际也在内。聚合级结论不受影响。【已确认】

### 4.3 质量评估

evidence 包整体质量**高于典型研究仓**：定量自洽（行数/字节数/哈希三层闭合）、
schema 统一（表件与 PE 件各自规整）、敏感字段明示（suspicious_level/flags）。
扣分项：①代次混杂未标注版本；②检索快照无查询串/引擎元数据，19 件文件名即唯一
上下文（ags_dl_4/5/6 与 ags_download_1/2/3 两组命名并存，关系需靠内容推断）；
③cab_index.json 为失败扫描产物却作为「CAB 索引扫描」证据入库（4.4）。

### 4.4 关键缺陷：cab_index.json 与 R6 叙述不符【高度怀疑→定性为工具 bug】

`evidence/ags-forensics/cab_index.json` 实测：六个 CAB（Core/Intermediate/Extras/
Optional/AlfSdPack/Languages）全部 `cFiles=1, cFolders=259, indexed=1`，`gc_hits`
全部空数组，`all_names_sample` 各只有 1 个文件名。根因在 `tools/zip_cab_index.py:48-49`
的 MSCF 头解析：格式串 `"<4sIIIIIHHHH"` 把 versionMinor/versionMajor（各 1 字节，
03 01）读成一个 u16 = 0x0103 = **259（六个 CAB 的恒定 "cFolders" 值即此伪值）**，
后续字段整体错位——脚本的 "cFiles" 实为真 cFolders、"flags" 实为真 cFiles
（Core.cab 真文件数约 723）、"setID" 实为真 flags；枚举循环 `while n < cFiles`
因此只走 1 步。后果：**WORKLOG.md:56「6个CAB的CFFILE索引扫描→Intermediate.cab
128MB」的「索引扫描定位」叙述与所提交证据矛盾**——索引扫描实际未能枚举任何 CAB
内容（gc_hits 全空），AGS 的真实定位路径只能是整拉 Intermediate.cab（128MB）后经
libarchive 解包（仓内工具链支持此路径，分析/06:4 的「4MB+128MB 范围取证」总量
声明仍诚实）。该缺陷不影响 AGS 本体证据（二进制哈希/验签/dispatchtable 均独立
成立），但 R6 方法论叙述存在一处美化——**「不经索引定位、直接范围拉取 CAB」
才是与证据一致的表述**。zip_cab_index.py 的 names 解析起点 coffFiles 读取正确，
修正头部格式串（`"<4sIIIIIHHHHHH"` 或逐字段）即可修复。

### 4.5 外部证据（检索快照）质量

19 件快照 schema 一致（rank/url/name/snippet/date），抽样 ags_eol/update_task/
gcclient 内容与 analysis/03 §2 的引述逐句吻合（如 update_task.json 收录社区原话
"Updater self-updated itself… Moving to Task Scheduler makes…"）。缺陷：无检索引擎
标识、无原始查询串、无检索时间戳入 JSON（仅 README/分析篇文字声明 2026-10-09）——
快照的再验证性依赖文件名猜测。属可接受的次要问题。【已确认】

## 5. tools/ 脚本审计（8 py 全读 + py_compile 全过 + ps1/sh 全读）

| 脚本 | 用途 | py_compile | 质量评注 |
|---|---|---|---|
| msi_audit.py（279 行） | 核心管线：OLE 转储→池解码→11 表重建→行为汇总→PE 字符串挖掘 | OK | 结构清晰、含池长度断言（:114）；**硬编码绑定**：TABLE_SPEC 写死本 MSI 的流文件名（"069_156.bin" 等 11 项）与 StringPool 尺寸 117716B（:102-104）——单工件取证脚本而非通用 MSI 工具；phase_strings 依赖 binutils strings |
| msi_pe_audit.py（370 行） | 36 PE 导入/导出/节熵/时间戳/签名 + 20 类能力规则 + 双通道字符串挖掘 | OK | 能力规则集（:30-80）与敏感组合判定（:83-92）设计良好；瑕疵：rich_header() 内一次无效 parse_data_directories 调用（:230-234 死代码）、suspicious_flags() 残留未用变量 w（:287）；**autoexec 正则过宽**——`(?:Run|Startup…)[!-~]{0,40}` 使 "Runtime"/"run" 类 CRT 串大量误报（strings_hits.json 中 17 个 _IsIcoRes 全部带 autoexec 噪声命中）——分析篇未依赖该字段做自启动结论（自启动以 Registry 表为准），误报未污染结论但降低了该证据件信噪比 |
| arm_payload_audit.py（82 行） | R6：ARM 载荷 + ARM MSI 内 CA PE 审计 | OK | 复用 msi_pe_audit.audit_one（:14），薄壳正确 |
| verify_msi_sig.py（124 行） | signify MSI/MSP 验签（authentihash vs messageDigest） | OK | 依赖 signify；:32-34 占位死代码；签名者证书按 issuer+serial 匹配（:61-67）+ 回退启发式；对 32 位 PE 的摘要误报已在 WORKLOG R6 如实记录（诚实） |
| zip_range_probe.py（76 行） | 远程 ZIP 中央目录解析（EOCD 定位） | OK | 干净；parse_central_dir 有未用变量 cd_start（:38）；假设中央目录紧邻 EOCD（对 Adobe 分发 ZIP 成立） |
| zip_range_fetch.py（59 行） | 单条目分块范围下载 + raw-deflate 流式解压 | OK | **无 CRC32 校验**（打印期望 usize 但不核对，:54）——完整性靠 ETag/MD5 交叉（WORKLOG R6 记录了此补偿手段） |
| zip_cab_index.py（109 行） | CAB 前缀流式解压 + CFFILE 索引 | OK（能编译） | **功能 bug**：MSCF 头字段错位（§4.4）——脚本可用性与其产物证据双双失效 |
| libarchive_cab_extract.py（81 行） | ctypes 调 libarchive.so.13 解 LZX CAB | OK | 硬编码 soname（环境绑定）；输出仅 basename（防路径穿越，:66-67）；流式读取正确 |

非 Python：`Collect-AdobeAutostart.ps1`（136 行）——Windows 只读收集器，内容与
「只读」声明一致（仅 Get-* / Get-AuthenticodeSignature / Out-File 本地 JSON），
全局 SilentlyContinue 可能掩盖采集失败（次要）；`push_to_github.sh`——token 不落盘
推送（Basic 头内存传递，:27-29），属其自身仓的写操作脚本（本轮未执行）。

**工具链→证据可信度总评**：证据链的主干（表重建/PE 能力/AGS 哈希）由质量中上
的脚本产出且带断言与交叉验证，可信度**高**；分支（CAB 索引扫描、字符串 autoexec
命中）存在已知缺陷，好在分析篇未把结论建立在这些缺陷分支上。结论：**evidence
整体可信，但引用时应避开 cab_index.json 与 strings_hits 的 autoexec 字段**。

## 6. source/ 二进制指纹与合规风险

指纹表见 §1.2（全部 sha256 实测，未执行任何二进制）。性质判断（静态）：
AcroPro.msi = Acrobat Pro DC 26.002.21931 基座 MSI（InstallShield 2012 骨架，
2021-02 构建）；setup.exe = 引导器；ags/ 四 MSI = ARM 活体清单/更新包（2015-2024
时间戳跨度）；arm_payload 三 exe = ARM 1.824.21.4663；gc/ 三件 = AGS 6.6.0.204 /
6.0.0.168 本体。

**合规风险（Adobe 商业安装包入 git 公开仓）**：
- `push_to_github.sh:18` 建仓参数 `"public":true`；cleanroom-analysis/README.md:48
  亦将 mmqz/acrobat-pro-audit 列为公开仓。即**21.97MB Adobe 版权再分发件处于
  公开分发状态**。Adobe EULA 通常禁止安装介质再分发；以「审计可复现性」为目的的
  收录不构成当然豁免，且 6 个 exe + 5 个 msi 均为完整可安装/可执行工件（非片段）。
- 体积代价：全仓 26MB 中 84% 为二进制，git 历史永久携带（subtree 合并进一步把
  22MB 复制进 mang-research-archive 的历史——合并仓 = 双份存储）。
- 缓解建议（供 mdcore-audit 汇总处置参考）：保留 artifact-hashes.txt + 证据 JSON，
  将二进制迁出公开仓（本地冷存/私有 release 附件），或整体转私有。属【设计偏好】
  但法律暴露面真实存在。
- 正面记录：`.gitignore` 正确排除 13MB 流转储目录；source/ 收录范围最小必要
  （未入库 6 个 CAB / 953MB msp / 1.58GB ZIP 本体）。

## 7. 与 mdcore pdf 线的映射（对照 07-pdf.md）

映射链实测：`mang-research-archive/README.md`「Acrobat 审计 → PDF 能力边界与安全
策略」→ `cleanroom-analysis/md-line/pdf-a/SECURITY_POLICY.md:5`（「输入资产：
acrobat-audit R1-R6」）+ `pdf_carrier_plan.md:52`（「安全策略（Acrobat 审计成果
直接转化）」）→ mdcore `crates/pdf` 实装（07-pdf.md §5）。

**已被 mdcore 采纳的 Acrobat 审计结论**（出处可溯）：

| Acrobat 结论 | 转化路径 | mdcore 现状（07-pdf.md 证据） |
|---|---|---|
| 36 PE 零网络导入 = 干净安装链判定法；AGS 上报面 = 反面教材 | SECURITY_POLICY S1.4（明文引用 R5/R6） | mdc-pdf 零 HTTP client / 零遥测（07 §5.1「引擎层无网络面」）——**采纳且更严** |
| JS/动作解释器是最大攻击面（Acrobat 16 年沙箱演进 + AGS 教训） | S1.1（引用 Acrobat R5/R6）+ pdf_carrier_plan「JS 不接」 | pdfium non-V8 构建、全 crate 无 JS 执行面（07 §1.2）——**采纳** |
| 危险对象族枚举 = 审计第一层（R1/R3 字符串挖掘法） | S2.3 出处栏明文引用 | security_scan.rs 八族对象图全扫 + OpenAssessment（07 §5.1）——**采纳** |
| 崩溃数据不外发（CRClient/AGS 反面教材） | S4.5（引用 R6） | 崩溃转储/日志本地化（无任何外发面）——**采纳** |
| 能力面方法论（导入表判能力、worker 隔离） | S4.1（引用 Acrobat R5 能力面方法论） | worker.rs 单工人 + 看门狗换代 + panic 隔离（07 §4.2）——**方法论采纳** |
| 「签名合法 ≠ 内容透明」 | 隐性原则（R6 验签实践） | signature.rs 诚实限界「结构验证 ≠ 签名有效」三次声明（07 §6）——**原则采纳** |

**未被采纳 / 无对应物的 Acrobat 结论**：
- **企业策略治理层**（analysis/04 处置矩阵：FeatureLockdown 注册表键位、
  PolicyDisableUpdater、SKIP_GC=1 类官方开关、端点白名单治理）：mdcore 的 S2/S5/S6
  是代码内常量策略，**无宿主可配置策略面、无企业策略键等价物**——mdcore 若进入
  企业部署叙事，这是 Acrobat 审计给出的现成模式而 mdcore 未接。属【设计偏好】
  （库形态 v0 的合理省略）+ 潜在演进项。
- **签名密码级验证能力**：Acrobat 审计对 Adobe 二进制做了 signify authentihash
  逐字节验签；mdcore signature.rs 止步结构级（P2-1：真实签名档往返测试缺）。
  Acrobat 的验签方法论未转化为 mdcore 的密码验证实现（07-pdf.md §9 P2-1 已登记）。
- **浏览器集成面**（Chrome 扩展 + 原生消息宿主）：mdcore pdf-wasm 走 pdf.js 纯
  渲染、零浏览器注册面——以「不做」方式划定更窄边界，可视为边界结论的采纳。
- **更新通道治理**（armmf 单域收敛）：mdcore 无自更新面——不适用（库形态）。
- Acrobat 审计的「常驻面最小化」画像（1 服务 + 1 任务 + 1 Run 键即被视为需治理的
  面成本）与 mdcore「零常驻、零网络」姿态同向——属研究输入而非逐条采纳。

## 8. 结论与覆盖声明

**总评**：acrobat-pro-audit 是一个证据纪律罕见严格的研究仓——定量结论 20+ 项抽检
零漂移、哈希三层自洽（文件级/JSON 内嵌/叙述声明）、自我修正留痕。审计价值经由
cleanroom-analysis SECURITY_POLICY 直接注入 mdcore pdf 线（S1.1/S1.4/S4.1/S4.5
四条策略明文引用），是六条研究线中映射链最短、最可溯的一条。

**遗留问题清单**（按重要性）：
1. zip_cab_index.py MSCF 头错位 bug + cab_index.json 失效证据 + WORKLOG R6
   「索引扫描定位」叙述与证据不符【已确认，工具 bug + 叙述修正项】；
2. 二进制合规：21.97MB Adobe 安装件公开分发状态（push 脚本 public:true）【高度
   怀疑（法律暴露面）→建议迁出/转私有】；
3. evidence 代次混杂（_behavior_summary/msi_manifest 为旧代次 schema，含两个空
   字段残留）与 README 复现声明的范围限定【已确认，文档修正项】；
4. Component/ServiceInstall 列错位超出 01 篇声明的两表范围（聚合结论不受影响）
   【已确认，声明修正项】；
5. R6 验签结论无落盘输出件（依赖 WORKLOG 叙述；工具在位可复跑）【待复跑可闭合】；
6. AGS 高 CPU 机制假设、SKIP_GC 实机阻断效果、计划任务实机形态——均属
   【需真机验证】（Collect-AdobeAutostart.ps1 已备）。

**覆盖声明**：
- 100% 检视：README.md、WORKLOG.md、analysis/ 6 篇全文；tools/ 10 件全文
  （8 py 逐行 + py_compile 8/8 通过 + ps1/sh 全文）；.gitignore、artifact-hashes.txt、
  ABCPY.INI/setup.ini（哈希+尺寸）、gc_dispatchtable（静态读全文头部）；
  source/ 11 个二进制 100% 指纹化（ls + sha256，零执行）。
- evidence/：48 件 100% 枚举 + schema/行数级全量校验；深读 15 件（31%）——
  msi-tables 16 件行数全核对（其中 Property/ServiceControl/CustomAction/IES/
  Component/File/Registry/_summary/msi_manifest/msi_pe_meta 10 件字段级深读）、
  pe-capability 7 件、ags-forensics 2 件、external-research 3 件深读 + 2 大件
  schema 抽样、_behavior_summary 全读。所有分析篇关键定量结论的支撑证据 100%
  核对（含 armmf 端点、SKIP_GC、CRClient、55 EXE-DLL、熵上限、能力分布）。
- 外部对照：mdcore-audit/audit/notes/07-pdf.md 全文 + cleanroom-analysis 的
  SECURITY_POLICY.md / pdf_carrier_plan.md / README.md 相关段（映射链取证）。
- 未覆盖：msi-tables 内 2 件大 JSON（File.json 1.3MB / Registry.json 750KB）的
  逐行内容（行数与抽样行核对代替）；external-research 16 件未逐件全文读（3 件
  深读 + 全件 schema/尺寸登记）；WORKLOG 引用的「主 worklog / Task 1 / R0」等
  仓外会话记录（不在本子仓内，无法检证）。

（完）
