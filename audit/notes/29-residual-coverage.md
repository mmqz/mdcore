# 审计笔记 29：mdcore 残余盲区补全（Task ID: R6-a，2026-11-05）

> 全仓覆盖补漏轮。基线：main@6322f86（同深化轮，工作树只读克隆，2026-11-04 19:02 落盘）。
> 任务：对第一轮（notes 01-16 + final-report.md）与深化轮（notes 17-23 + final-report-phase2.md）
> 未触达或仅一笔带过的区域做全量补审。纪律：只读；零 git 命令；零源码修改；
> 唯一动作为创建本文件。脚本验证仅 `bash -n` / `py_compile`（字节码重定向 /tmp，仓内零落盘）。
> 风格与证据口径同笔记 01：`路径:行号` 均为本轮 HEAD 实测。

## 0. 执行摘要（一分钟版）

- **web 位置之谜已解**【已确认】：web 前端**就在本仓根 `web/`**（非 sdk/）。web-e2e.log 的 4 条
  "No such file or directory" 是深化轮沙箱运行器把 6 个位于 `web/` **根**的测试脚本（tr2a/tr2b_l1/
  k5/k45/acceptance_pdf5/tr2b_l23）按 `web/tests/` 路径调起所致——文件全部在档，且其中 3 个的
  成功运行产物（results JSON/PDF）就在 `web/tests/results/`（§7）。
- **NSIS 安装器模板有 3 处实缺陷**【已确认（静态）】：`{{VERSION}}` 占位符无注入机制且与
  BUILD-VERIFY 建议的 `-DVERSION` 语法不匹配；WebView2「引导下载」缺下载步骤（ExecWait 一个
  不存在的 `$TEMP\wv2setup.exe`）；相对路径假设与 BUILD-VERIFY 给出的命令自相矛盾（§3）。
- **macOS 打包 = 仅模板、零构建路径**【已确认】：`packaging/macos/` 只有 Info.plist.tpl，
  BUILD-VERIFY.md 四平台表里 macOS 根本不在验证范围（§3）。
- **spike 补审**【已确认】：probe-gpui 的锁文件显示该探针依赖的是 **crates.io 注册表的
  gpui 0.2.2**，而生产走的是 vendored third_party/zed @1663af4——评估路线与落地路线不同源（§2）。
- **9 个根目录草稿文件逐一对账完毕**【已确认】：全部为 #132-3 折叠轮「待粘贴片段」，内容已
  全部集成进 mang-shell（逐文件行号锚点见 §6），维持「删除」裁决并升级证据等级。
- **脚本语法检查全绿**：11 个 .sh `bash -n` 全过；`scripts/` 5 个 .py + `web/` 根 6 个 .py +
  `web/tests/` 29 个 .py `py_compile` 全过（40/40，§9）。
- 补全后全仓无产品代码死角；残余未审面仅剩三类既定排除项（§11）。

---

## 1. 已覆盖判定表（第一步：关键词 × 既有笔记检索）

检索范围：`audit/notes/*.md`（01-23 + loc-stats/hot-files）、`audit/final-report.md`、
`audit/final-report-phase2.md`（rg 全文，含 test-runs/ 日志）。

| # | 关键词/区域 | 判定 | 已有覆盖（笔记:行） | 缺口（本轮补） |
|---|---|---|---|---|
| 1 | spike | **一笔带过** | 01-structure.md:200（孤儿 Cargo.lock，删）、final-report.md:219/346（清理建议）、09:140（仅锁内 tokio 残影） | 锁文件内容挖掘：依赖什么、版本、与最终 GPUI 选型关系（§2） |
| 2 | packaging/ 总体 | 部分覆盖 | 14-compliance.md:34-35（四模板+BUILD-VERIFY 记账）、:236（零许可装载 C-8）、13:87/203/239（macOS 零验证） | **脚本/模板正确性**（路径假设、占位符、版本一致性）——零覆盖（§3） |
| 3 | BUILD-VERIFY.md | 一笔带过 | 14:35（deb 2.4MiB 实构建引用） | 文中命令与模板占位符/相对路径的**自洽性核对**（§3.1-3.3） |
| 4 | appimage | 部分覆盖 | 14:286（零 license 装载 grep）、BUILD-VERIFY §2（五缺陷修复史） | build-appimage.sh/AppRun 逐行（§3.4） |
| 5 | nsis | 一笔带过 | 14:286（零 license 装载）、BUILD-VERIFY:101-107（❌ 阻塞登记） | **模板本体缺陷**——3 处新发现（§3.1-3.3） |
| 6 | zed_port | 部分覆盖 | 10-mangshell.md:76（升级四步+默认路径 zed_port.py:16）、14:121（含 zed×7 词不入 PRODUCT_PATHS）、01:202（关键基建判定） | **脚本本体代码质量审计**（431 行逐读）（§4） |
| 7 | gates.md | 已覆盖 | 作为证据源被 01/03/04/07/11/13/14 六笔记引用；14:29 零 mang-shell 条目（C-7）；01:188（D-8 脱锚） | 无新缺口；仅登记文件性质（§6.11） |
| 8 | compliance/（scan.sh+denylist） | **已覆盖** | 14 §1.1/§3 全量（实跑 exit 1、5 假阳性、工程缺陷 A-E、PRODUCT_PATHS 逐行） | 仅补 compliance/README.md 两处过时声明（§5.1） |
| 9 | compliance_release/ | **已覆盖** | 14:30-33 四件逐项（NOTICES 草案停在搬运前 8 天、ASSET_CHECK 15 SVG 零登记、TRADEMARK_RULES 挂起项、检索未竟项）+ :239/:258/:270-272 路线图 | 无实质缺口（§5.2） |
| 10 | _dm_add/_foldtests/_foldtests2/_helpers/_mod_fold_api/_persist/_purefns/_settings_add/_settingstests | **一笔带过** | 01:198（一行裁决「删除 P3」）、loc-stats.md:59-67（尺寸+首行摘录）、final-report:219（游离文件列举） | **逐文件内容+集成锚点对账+分级**（§6） |
| 11 | run_web_tests | 一笔带过 | 21:179（「沙箱路径，仅记录」） | 运行器行为与 4 条报错的因果链（§7） |
| 12 | web/ | **已覆盖** | 04/13/15/18/20/21 六笔记（桥/测试/许可/内部/静态/实跑） | 位置之谜的证据链补齐（§7） |
| 13 | .github | **已覆盖** | 13-tests-ci.md §1 全量两 yml（63+145 行逐条）+ 14:131/287、17:202、19:151、21:170 交叉引用 | 无缺口（§8） |
| 14 | make_fields_fixture | **未覆盖** | 仅 01:61 一句「夹具生成」混装描述 | 逐个审计（§4.1-4.3） |
| 15 | make_image_docx_fixture / make_notes_fixture | **未覆盖** | 同上 | 逐个审计（§4.1-4.3） |
| 16 | probe_p21 | **未覆盖** | 零命中（21:179 的 probe_p04 族是**沙箱外**复现脚本，非本仓 scripts/probe_p21） | 逐个审计（§4.5） |
| 17 | docs/ 与 mang-shell/docs/ | 部分覆盖 | api-reference/field-code-semantics/docx-fidelity-matrix/pdf-annotate/rpc-protocol 五件被引 | **9 件零引用文档**登记（§10） |

结论：真正的全空白区 = packaging 脚本正确性、NSIS 模板、scripts/ 四个 py 本体、spike 内容、
9 草稿逐文件对账、docs 零引用清单、web 报错因果链。以下逐区补审。

---

## 2. a) spike/gui-framework-eval/ 补审

**现状**【已确认】：目录树 `spike/gui-framework-eval/probe-gpui/` 下**唯一文件** =
`Cargo.lock`（178,392 B，lock 格式 version 4，共 **709 个 package 条目**）。探针源码
（main.rs 等）从未提交——与 01:200「锁文件成孤儿」判定一致，本轮维持。

**锁文件能证明的事实**（新增证据）：

- 存在自引用包：`name = "probe-gpui"`, `version = "0.1.0"`, `dependencies = ["gpui"]`
  ——即探针是**只依赖 gpui 一个 crate 的最小骨架**（spike/.../Cargo.lock 内 probe-gpui 条目）。
- `gpui` 版本 = **0.2.2，source = registry+https://github.com/rust-lang/crates.io-index**
  ——评估走的是 **crates.io 发布版**，不是 git 依赖 zed-industries/zed。
- 传递闭包 709 包里出现 `gpui_macros / gpui_collections / gpui_derive_refineable /
  gpui_http_client / gpui_media / gpui_perf / gpui_refineable / gpui_semantic_version /
  gpui_sum_tree / gpui_util / gpui_util_macros`（crates.io 拆分发布族）与
  `zed-async-tar / zed-font-kit / zed-reqwest / zed-scap / zed-xim`、`quinn`（HTTP/3 栈）、
  `tao`/`cosmic`（tao 1 条、cosmic-text 1 条，均系 gpui 平台面传递依赖）。
  `winit/gtk/fltk/iced/egui/eframe/wry/slint/xilem/dioxus/vello` 等其他 GUI 框架名 **0 命中**。

**判定**：

1. 「gui-framework-eval」目录名暗示多框架横向评估，但仓内残迹只支持「**单框架（GPUI）
   可行性探针**」的结论——没有任何第二框架的痕迹在档【已确认】。
2. **评估路线 ≠ 落地路线**：spike 验证的是 crates.io `gpui 0.2.2`（registry 依赖，可随时
   解除/升级，Apache-2.0 口径与 crates.io 发布一致），而生产最终 vendor 了
   `third_party/zed` @`1663af4`（2026-10-29 快照，32 crate 含 GPL 面）——差异动因合理
   （mang-shell 需要 crates.io 上没有的 `ui`/`theme` 组件层，见 10-mangshell 搬运记账），
   但**决策记录里没有这份 spike 结论**：目录只剩锁文件，评估数字/结论未入 docs/，
   治理链断裂点与 01:200 判定一致【已确认】。
3. 卫生裁决（登记，不代执行）：`spike/` 整树删除；若 GPUI 选型依据需留档，把
   「probe-gpui@crates.io gpui 0.2.2 可编译」这一句话 + 本节证据移入 docs/ 或
   third_party/zed/PROVENANCE.md 即可【设计偏好】。

与 10-mangshell.md 的关系：该笔记审的是 vendored 树本身（NOTICES 对账/升级四步/风险面），
本轮只补「为什么有这个 spike、它验的是什么」——不重复其结论。

---

## 3. b) packaging/ 补审（脚本与模板正确性）

既有覆盖（14-compliance）只审了**许可装载面**（grep 四脚本零 license/NOTICES 命中，C-8）。
本轮补**构建正确性面**。逐文件：

### 3.1 nsis/installer.nsi.tpl —— 三处缺陷【已确认（静态）；运行验证需 makensis，归入需真机验证】

1. **`{{VERSION}}` 无注入机制**：`!define APPVERSION "{{VERSION}}"`（installer.nsi.tpl:9），
   OutFile `mdcore-setup-${APPVERSION}.exe`（:15）与注册表 DisplayVersion（:40）全部吃这个
   占位符。但 packaging/nsis/ 下**没有任何构建脚本**；BUILD-VERIFY.md:105-106 给的解法是
   `makensis -DVERSION=0.1.0 packaging/nsis/installer.nsi.tpl`——`-D` 定义的是 NSIS 符号
   `VERSION`，而模板里没有任何 `${VERSION}` 引用，只有字面 `{{VERSION}}`。照该命令执行，
   产物名与注册表将出现**字面 `{{VERSION}}`**。BUILD-VERIFY.md:106-107 自己也预警「模板的
   {{}} 注入面届时需先核对一遍——与 AppRun 同类占位符风险」，本轮把风险坐实为缺陷。
2. **WebView2「引导下载」缺下载步骤**：`WEBVIEW2_BOOTSTRAPPER` URL 定义后**零使用**
   （installer.nsi.tpl:12 定义，全文无 inetc/NSISdl/Download 引用）；`EnsureWebView2`
   （:54-60）在注册表探测失败路径上直接 `ExecWait '"$TEMP\wv2setup.exe" /install /silent'`
   （:58）——**从网上下下来的文件从未被下载**，ExecWait 一个不存在路径。packaging/README.md:129
   「NSIS 模板含检测 + 引导下载」的「引导下载」半句不成立（检测有、下载无、安装必然空跑）。
3. **相对路径假设自相矛盾**：`File "..\..\target\release\mdcore.exe"`（:29）与
   `File /r "..\..\web\dist"`（:30）假定 makensis 的 CWD = `packaging/nsis/`；而
   BUILD-VERIFY.md:106 的示例命令从仓根执行——两条 `..\..\` 会落到**仓外**，makensis
   直接报 file not found。两份文件至少有一份的口径是错的。

附带（设计风险，非阻断）：`WriteRegStr HKCR ".md" "" "MarkdownFile"`（:45）无条件抢占
全机 .md 关联且卸载不还原前值（:68 只删 MarkdownFile 键）——安装器常识缺陷【设计偏好】。

### 3.2 macos/Info.plist.tpl —— 仅模板、零构建路径【已确认】

`packaging/macos/` 目录下**只有** Info.plist.tpl（版本占位符 {{VERSION}} 在 :19/:21 两处、
BUNDLE_ICON :25、LSMinimumSystemVersion 12.0 :27）。无 build-app.sh/dmg 脚本、无 codesign/
notarization 面；BUILD-VERIFY.md §4 汇总表只列 deb/AppImage/NSIS 三平台——**macOS 打包链
从未存在构建路径**，只是「声明了目标平台」。与 13-tests-ci「macOS 零验证」（13:203/239）
互为印证：零测试 + 零打包 = macOS 是纯纸面平台【已确认】。checklist D3 的勾选框（:53-54）
在可预见的未来无执行载体。

### 3.3 版本一致性横评【已确认】

| 面 | 版本来源 | 判定 |
|---|---|---|
| deb | build-deb.sh:14 `"$BIN" --version \| awk '{print $2}'` → control.tpl:6 | 单源闭环 ✅（r24_packaging.rs:149-160 锁「禁止手抄」） |
| AppImage | build-appimage.sh:21 同法 → 产物文件名 :68 | 单源闭环 ✅ |
| NSIS | installer.nsi.tpl:9 字面 `{{VERSION}}`，无脚本 | **断链**（§3.1-1） |
| macOS | Info.plist.tpl:19/21 字面 `{{VERSION}}`，无脚本 | **断链**（§3.2） |
| desktop 入口 | app.desktop.tpl `Version=1.5`（:6）= Desktop Entry 规范版本，非应用版本 | 语义正确 ✅ |

### 3.4 deb / AppImage 脚本本体（bash -n 过，逻辑复读）

- build-deb.sh：布局/校验面完整——dist 缺失硬失败（:28-31）、Installed-Size 实测注入
  （:47-50）、control 注释剥离 `grep -v '^#'`（:50，即 BUILD-VERIFY.md:39-41 修复项）、
  dpkg-deb 输出路径修正有自注（:56-57）。质量**良好**。
- build-appimage.sh：Q3-3 三个阻断级缺陷的修复均在位（AppRun 占位符注入 :56-58、dist
  镜像 deb 布局 :34-40、图标缺失降级 WARN :49-51）；linuxdeploy 连续版钉注释更新过
  （:14-17，404 事故记录）。质量**良好**。
- AppRun：`set -euo pipefail`（:5）、`MDCORE_DIST` 导出（:21）、exec 占位符由构建期注入
  （:24）。质量**良好**。
- 遗留小刺：两脚本共用 `sed -e 's/mang/mdcore/g'`（build-deb.sh:35、build-appimage.sh:44）
  做显示名反转（A4 已替换为 mang、CI 内测包按 mdcore 工作名出，BUILD-VERIFY.md:3-4）。
  当前模板正文仅 `Name=mang`（app.desktop.tpl:7）一处会被改写，**无 collateral**；但这是
  全局子串替换，任何未来含 "mang" 子串的词（manga/mangle…）都会被误伤——低风险登记
  【已确认（当前安全）/设计偏好（机制脆弱）】。
- packaging/README.md:9-10 声称「TRADEMARK_RULES §1/§2.5/§4 **在仓库中不存在**」——
  该文件现在就在 compliance_release/TRADEMARK_RULES.md（3,745 B）。README 的「流程缺口」
  标注是**过时声明**（写作时点早于文件入仓，A5 因此被错误挂起：checklist.md:24-25 仍说
  「该文件当前不在仓库中」）【已确认——两处治理文档与现实矛盾，本轮新发现】。

### 3.5 与既有结论的合并

14-compliance 的 C-8（零许可装载）不变；新增：**打包链的平台完整性 = deb/AppImage 可用、
NSIS 三缺陷、macOS 无链**。BUILD-VERIFY.md 作为验证报告本身可信（它对 NSIS 如实标 ❌），
但它给出的解除命令会踩 §3.1-1/3.1-3 的坑。

---

## 4. c) scripts/ 补审（6 文件逐个 + py_compile）

既有覆盖仅 01:61/202 一句「夹具生成 + zed_port 搬运器 + p1 补丁」。逐文件：

### 4.1 make_fields_fixture.py（133 行）【质量：好】

- 用途：#140 域码黄金例生成器——手拼 zip 产 `test-artifacts/F1/fields-golden.docx`
  （make_fields_fixture.py:25），覆盖 fldChar 五段/多 span 结果区/TOC 跨段/嵌套 PAGEREF/
  fldSimple/无 separate 退化域九段矩阵（:6-16 docstring 与 PARAS :77-107 逐段对上）。
- 设计纪律值得肯定：纯标准库（zipfile，无 python-docx 依赖），「缺失即测试 FAIL 不 SKIP」
  （:4）——夹具可再生、测试恒读。
- 无风险点：只写 OUT 一个路径；文件名/目录由 `__file__` 相对推导（:25），无仓外路径假设。

### 4.2 make_image_docx_fixture.py（约 300 行）【质量：好】

- 用途：#204 带图 docx 黄金例 → `mang-shell/tests/fixtures/smoke_image.docx`（:30-35），
  补「图片不丢」正向对账缺口；2 图分置两段 + 手工打包最小合法 PNG（struct+zlib，:37 docstring
  「无 PIL 依赖，沙箱无图库环境可跑」）。
- 值得注意的跨 workspace 事实：**主仓 scripts/ 生成 mang-shell 子 workspace 的测试夹具**——
  两 workspace 在测试资产层的耦合方向与 01-structure「mang-shell 自含」的代码面结论不同，
  属资产供应链耦合（无风险，登记）【已确认】。

### 4.3 make_notes_fixture.py（约 180 行）【质量：好】

- 用途：#141 批注/脚注黄金例 → `test-artifacts/F2/notes-golden.docx`（:25）；
  docstring 记录了选型依据（真实语料脚注面为分隔符空壳，故手构——与 05-docx 的语料观察一致）。
- 与 4.1/4.2 同构：纯标准库、单输出路径、缺夹具即 FAIL 纪律。

### 4.4 zed_port.py（431 行）【质量：可用但脆弱——关键基建单点】

10-mangshell.md:76 已覆盖「升级四步 + 默认 `/home/z/my-project/zed-upstream`」（zed_port.py:16，
可 `ZED_UPSTREAM` env 覆盖）。本轮补**本体代码质量**（笔记 10/22 均未读此文件）：

- **正则解析 TOML 而非真解析器**：全链靠手写正则 + 括号配平聚合（:24-63、:116-231）。
  文件内自述的 r39 系列事故——主段正则截断漏 `[workspace.dependencies.<name>]` 子段致
  windows 依赖被注释（:46-49）、`[[bench]]` 双括号头漏配致 benches 段照抄（:126-127）、
  根表字段丢弃致闭包内依赖静默吃默认 features（:280-282）——**三处历史 bug 全部源于同一
  根因**。上游 Cargo.toml 排版稍变即再触发，且发生在「闭包计算/依赖展开」这类静默面。
  【已确认（代码级）；升级重放风险需真机验证】
- **docstring 过时**：:5 说「产出: /home/z/my-project/zed-port-lab/」，实际产出 =
  本仓 `third_party/zed`（:17-18 注释已改，docstring 未同步）【已确认】。
- `os.popen(f"cd {ZED} && git rev-parse HEAD")`（:375）取上游 commit 写入生成注释——
  shell 注入面可控（ZED 来自 env），但 os.popen 已是弃用风格；上游钉值只落在生成物注释 +
  PROVENANCE 手工维护，**脚本本身不校验**上游是否还是 1663af4（升级时漂移只能靠人眼）【已确认】。
- 闭包根与例外全部硬编码（:112 `['ui','gpui_platform','text','zlog','bench_metrics']`；
  :317 EXCLUDE 集；:364 keep_patches 四项）——这些是 r38-r42 轮试错沉淀，正确但**无任何
  测试/断言保护**，上游一变即需人工重推【已确认】。
- 正面：失败方向安全——真闭包缺口会 `raise SystemExit`（:300）而非静默漏搬；`--clean`
  只删 crates/ 与生成 Cargo.toml、明示不动 PROVENANCE/NOTICES（:410-416 注释）。

### 4.5 probe_p21_word_complex.py（57 行）【质量：好；一次性探针】

- 用途：P2-1 验收探针——`test-artifacts/TR2/fixtures/word-complex.docx` 经
  `http://127.0.0.1:8123`（需先 serve web/dist，:4/:17）导入，打印 md 头尾 + stats
  关键键（页眉页脚接入计数），「PR 自测证据用」（:6）。
- 定性：**一次性开发探针**而非常驻测试（无断言、依赖外部 serve 进程、exit code 只反映
  import ok）。留在 scripts/ 合理（与其余五件不同类：其余是可再生夹具/基建，本件是验收
  证据脚本）【设计偏好】。

### 4.6 p1_gpui_windows_zoom_toggle.patch（23 行）

- 内容：对 `third_party/zed/crates/gpui_windows/src/window.rs` zoom() 的 toggle 化补丁
  （IsZoomed → SW_RESTORE/SW_MAXIMIZE 分支，:3-19），注释内嵌登记链（PROVENANCE「源码
  补丁登记」P1、#68 真机验证缘由、三后端 toggle 语义对齐依据）。
- 与 10-mangshell.md:49/14:212/289 的「唯一登记补丁、在档一致」结论**复核一致**，无新发现；
  补充一点：补丁文件与 vendored 源的**重放一致性无机器校验**（PROVENANCE 靠人眼 diff），
  与 4.4 的上游钉值同属「人工对账」族【已确认】。

py_compile 结果：5 个 .py 全部通过（§9）。

---

## 5. d) compliance/ 与 compliance_release/ 补漏

### 5.1 compliance/（对照 14-compliance §1.1/§3 已覆盖部分）

已覆盖：scan.sh 67 行逐行（PRODUCT_PATHS/EXTRA_PATHS/词边界/前缀模式/exit 语义）、
denylist/sources.txt 20 词四源分节（本轮复核计数：15-42 行恰 20 词，与 14 的「实际 20 词」
一致）、实跑 exit 1 + 5 假阳性定位、工程缺陷 A-E。**本轮仅新增两点**：

1. compliance/README.md:4-5 声称「结论性文档与发布形态套件在 **compliance-baseline 分支**
   （COMPLIANCE.md + compliance_release/ 三件套）」——当前树里 compliance_release/ 四件
   与 COMPLIANCE.md 就在**主工作树**（根目录实测在档）。该分支叙事与现实脱节（或为早期
   分支规划残留）【已确认——文档过时】。
2. compliance/README.md:11「CI 与每次提交前运行」——与根 README.md:61 同款口头纪律声明，
   14:131/287 已证 workflows 零引用；此为**第二处**同声明位（补登）【已确认】。
3. 豁免表（compliance/README.md:46-51）「暂无」——与 14 §3.4-1 止血建议（先记账后机制）
   一致，未被采纳，登记状态不变。

### 5.2 compliance_release/（对照 14 §1.2 已覆盖部分）

四件（ASSET_CHECK.md 104 行 / THIRD-PARTY-NOTICES.draft.md 654 行 / TRADEMARK_RULES.md
/ trademark_search_2026-10-27.md）在 14-compliance.md:30-33 已逐项定性并给出缺口
（草案停在 Zed 搬运前 8 天、15 SVG 零登记、A5 归属句挂起、CNIPA/WIPO 未查）。本轮复读
无新增缺陷；唯一交叉发现已并入 §3.4：**TRADEMARK_RULES 实际在仓**，packaging 两文件的
「不存在」声明过时——这同时意味着 checklist A5 的挂起理由（「该文件当前不在仓库中」
checklist.md:24-25）已经失效，A5 变为**可执行未执行**【已确认——本轮新发现】。

---

## 6. e) 根目录 9 个下划线草稿 + gates.md 逐个对账

性质总判【已确认】：全部为 mang-shell **#132-3 折叠轮**的开发过程「待粘贴片段」（把准备
写进源码的 API/纯函数/测试体先存成根目录 txt），内容**已全部集成**。逐文件锚点对账
（本轮新增证据，笔记 01:198 只给了 display_map.rs:160 一个锚点）：

| 文件 | 内容 | 集成锚点（实测行号） | 分级裁决 |
|---|---|---|---|
| _dm_add.txt（833B） | DisplayMap 折叠 API：fold_heads/fold_ranges/unfold_all/unfold 包含行 | mang-shell/src/editor/display_map.rs:160/165/173（+ mod.rs:1713 Editor::unfold_all 转发） | 删 |
| _helpers.txt（894B） | 纯函数：heading_level_of（ATX 1-6 判定）+ fold_persist_key | mod.rs:4714、:4811 | 删 |
| _purefns.txt（1963B） | 纯函数 heading_span_in（标题块区间，无 GPUI 上下文单测用） | mod.rs:4735（pub fn 在位 + :5220 测试） | 删 |
| _mod_fold_api.txt（4914B） | Editor 侧围栏区间 fence_span_at 等 | mod.rs:1550（:1542 消费点、:1582 foldable_span_at） | 删 |
| _persist.txt（1793B） | persist_folds/restore_folds（折叠落盘/恢复，头行号语义） | mod.rs:1663/:1681（:1333-1345 五处调用） | 删 |
| _settings_add.txt（2859B） | settings 侧 save_folds/load_folds + FOLD_PERSIST_CAP/FOLD_FILE_CAP | settings.rs:219/:225/:215/:217 | 删 |
| _settingstests.txt（2145B） | folds_roundtrip_per_file 等设置测试 | settings.rs:535-610（folds_roundtrip_per_file :541-546 原文一致） | 删 |
| _foldtests.txt（4938B，121 行） | 编辑器侧折叠测试体（md_editor 构造 + heading_level 断言组） | mod.rs 内嵌测试面（heading_level_parses_atx_forms 等已并入） | 删 |
| _foldtests2.txt（3530B，86 行） | 同上的纯函数面第二版（测试驱动拆层产物，与 _foldtests 大量重复） | 同上（两版取并集后进 mod.rs） | 删 |

- 证据口径：九件内容与上列锚点**逐段文字一致**（docstring/常量值/断言串全同），
  集成完成度 100%，无一文件含「未落地」内容——即**零信息损失删除**【已确认】。
- 升级 01:198 的裁决理由：原判「删除（P3 卫生）」依据是 display_map.rs:160 单锚点 +
  「代码粘贴残片」目测；本轮 9/9 文件全部完成锚点对账，证据等级从「高度怀疑已集成」
  升为「已确认已集成」。均**不**建议移 docs/（内容是源码片段非文档；且真源在
  mang-shell，留档无增量信息）。
- 附带游离物：`.t.log`（13.9KB/251 行）= 开发者 **Windows 机器**的 cargo 警告日志
  （:1-3 即 `E:\Code\ai\md\audit\_wt\find\third_party\zed\crates\ztracing\src\lib.rs` 路径）
  ——内容为 unused-mut 类警告（尾段 multi_cursor.rs:617），即 mang-shell 的 clippy 级问题
  以日志形式被 commit 进仓。final-report.md:219 已列名，本轮补内容定性：**删**，且佐证
  13-tests-ci「mang-shell 不受 fmt/clippy 门约束」的又一实例【已确认】。

### 6.11 gates.md（性质登记，不重复深审）

1,445 行 / 106KB，R14→R24 各轮硬 Gate 台账（mdcore 线）。已被 6 份笔记按需引用
（03/04/07/11/13/14）；已知缺口（14:29 C-7 零 mang-shell 条目、01:188 D-8 零 GPUI 提及、
13 §1.7 性能数字仓内不可复核）维持原判。作为「根目录游离物」的定性：**非游离**——它是
被治理体系依赖的注册表，不应删/移；其问题是内容滞后而非文件卫生【已确认】。

---

## 7. g) web 位置之谜——证据链与答案

**问题**：web-e2e.log（audit/notes/test-runs/web-e2e.log，全文仅 12 行）记录
`python3: can't open file '/home/z/my-project/mdcore-audit/web/tests/tr2b_l1_commonmark.py': No such file or directory`（同型共 4 条：tr2b_l1_commonmark / tr2a_cross_process / k5_recheck / k45_recheck，均 exit=2，时戳 20:02:32）。

**答案（逐问）**：

1. **web 代码到底在不在本仓？** 在【已确认】。`web/` 就在仓根：src/ 13 个 .ts（含 i18n 3 件）、
   tests/ 29 个 .py + fixtures/ 7 件 + results/、根级 build.mjs / build-wasm.sh /
   build-pdf-wasm.sh / package.json / package-lock.json / index.html / bench.html /
   tsconfig.json / README.md / .gitignore。sdk/ 下只有 mcp/ 与 python/ 两个 Python SDK
   （mang_mcp/mang_rpc），**无任何 web 前端**。notes 20（静态全量）/21（实跑）审的就是这个
   真实目录——非幻影。
2. **报错根因**：那 4 个文件**位于 `web/` 根而非 `web/tests/`**【已确认】。深化轮沙箱运行器
   `/home/z/my-project/scripts/run_web_tests.sh`（仓外；:4 `cd .../web`，:9 默认集
   `ls tests/*.py` 且排除表漏了 tr2b_l1_commonmark）以 `tests/<name>.py` 相对路径调起了
   这批根级脚本 → ENOENT。web/ 根的 6 个脚本（tr2a_cross_process / tr2b_l1_commonmark /
   tr2b_l23_external / k5_recheck / k45_recheck / acceptance_pdf5_ui）中，acceptance_pdf5_ui.py
   自述「一次性验收脚本（**非套件——不入 tests/ 常规面**）」（:14-15 docstring），tr2 族为
   TR-2 轮驱动器——**有意放根目录**，是分层设计不是错放。
3. **e2e 测试文件现在何处**：全部在档（web/ 根 6 + web/tests/ 29）。其中 3 个的成功运行产物
   就在 gitignore 的 `web/tests/results/`：tr2b_l1_external.json（154,956 B，tr2b_l1_commonmark.py:32
   的 OUT 路径）、cross_process_determinism.json（tr2a_cross_process.py:33 的 OUT）、
   save_annotated_ui.pdf（acceptance_pdf5_ui.py:67 的 OUT）——与笔记 21 记分板 25/26/29 行的
   ✅ 互证。k5/k45 只打 stdout 不落 JSON（k45_recheck.py:106、k5_recheck.py 全文无 OUT），
   其 ✅ 仅有记分板声明、无仓内工件佐证【已确认（文件在档）/记分板该两行证据等级=声明】。
4. **results.json 套件清单对账**：audit/notes/test-runs/web-results/results.json = **web_smoke.py
   单套件**输出（tests 键 W1-W13 + G1/G2 共 15 组、56 check 全 pass，summary total=15）——
   与 web_smoke.py 的发射点（web_smoke.py:920-933 results["tests"] 逐键）和笔记 21 记分板
   第 1 行「✅ 15/15」**逐键对上**。results_docx_export.json summary = total 30 / failed 1 /
   failures=["w2_toc_field_emission_and_position"]——与笔记 21 §4 的 29/30 红项**逐字对上**。
   记分板 30 行 vs 实际文件：tests/ 29 py − 5 非套件（diag×2/probe×2/gen_pdf_fixture）= 24，
   + 根级 6 = **30，对上**。
5. **两处审计工件自身的小瑕疵**（登记给 R6 汇总轮，非仓缺陷）：
   - 笔记 21:56「含 web/ 根目录 4 个」实为 **6 个**（记分板 25-30 行自己列了 6）——笔记内
     计数不一致【已确认】。
   - 笔记 21 §9（:177）称 web-e2e.log 为「全量运行日志」——该文件实为**只有 4 条失败记录的
     尾片段**（759B/12 行；完整日志在仓外 audit-out/web-test-runs.log，同为 12 行，说明批跑
     器只留下了这段）。26 个 tests/ 套件的逐套件日志**未归档**【已确认——档案完整性缺口】。

---

## 8. f) 根目录隐藏条目与 .github 复核

`ls -la` 仓根全量隐藏项：`.git/`、`.gitattributes`、`.github/`、`.gitignore`（+ 非隐藏游离项
已在 §6 处理；`build/`、`target/`、`web/pkg*/dist` 均 gitignore 产物面）。`.github/` 下
**只有 workflows/windows-check.yml（63 行）与 workflows/windows-test.yml（145 行）**——
无 CODEOWNERS / dependabot.yml / ISSUE_TEMPLATE / PULL_REQUEST_TEMPLATE / FUNDING
【已确认】。两 workflow 已被 13-tests-ci.md §1 全量逐行覆盖（含 fmt/clippy 缺位、9/17
crate、--skip golden、单线程防抖动等），并被 14:131/287、17:202、19:151、21:170 交叉引用
——**f) 无需补审**。新增登记仅一条卫生观察：无 CODEOWNERS/dependabot = 依赖更新无机器
通道（与 14-compliance「SBOM 重跑从未触发」互证）【设计偏好级】。

---

## 9. 脚本语法检查结果（本轮全部实测）

| 检查 | 对象 | 结果 |
|---|---|---|
| `bash -n` | packaging/appimage/build-appimage.sh、packaging/appimage/AppRun、packaging/deb/build-deb.sh、compliance/scan.sh、mang-shell/{smoke,mouse_smoke,rpc_smoke,perf_meas,screenshot}.sh、web/{build-wasm,build-pdf-wasm}.sh | **11/11 通过**，0 语法错 |
| `py_compile`（cfile→/tmp，仓内零落盘） | scripts/ 5 个 .py | **5/5 通过** |
| 同上 | web/ 根 6 个 .py（tr2a/tr2b_l1/tr2b_l23/k5/k45/acceptance_pdf5_ui） | **6/6 通过** |
| 同上 | web/tests/ 29 个 .py | **29/29 通过** |
| `file` 只读探测 | packaging/appimage/icon-placeholder.png | PNG 256×256 RGBA 1,388 B——即 BUILD-VERIFY.md:86-87 自认的「确定性生成占位图」，deb/AppImage 当前**捆的是占位图**（checklist C5 未勾的实态）【已确认】 |

合计：11 shell + 40 python，**全绿**。无任何脚本因语法问题被阻断；§3/§4 的问题全部是
逻辑/接线级，不是语法级。

---

## 10. h) docs/ 与 mang-shell/docs/ 未引用文档清单

对账方法：rg 各文件名（含去扩展名的宽松式）于 notes/*.md + 两份 final-report。
**已被引用（5 件）**：api-reference.md（01/final-report——且已登记其核心面仅 4 API 的缺口）、
docx-fidelity-matrix.md、field-code-semantics.md（05 域）、pdf-annotate.md（07）、
mang-shell/docs/rpc-protocol.md（22/23）。**零引用（9 件，本轮登记）**：

| 文档 | 一句话内容判断（本轮首读头 20-25 行） |
|---|---|
| docs/INSTALL.md | PDFium 安装指引（MDC_PDFIUM_LIB 环境变量三平台口径）——与 07-pdf 的「pdfium feature 测试需 env」结论互补，内容自洽 |
| docs/quickstart.md | D-2 从零到全绿的最短路径（Linux 主环境 + Windows 增补）——注意它教的正是本地全量门禁流程，反衬 CI 缺位（13 的旁证，此前未被引用） |
| docs/async-import-design.md | P2-5 同步导入阻塞评估（50MB docx 4.9s 冻结实测）+ Worker 化设计——**评估轮只设计未实现**，与 18-wasm 的现状一致 |
| docs/pdf-ocr.md | OCR 能力矩阵（pdfium 光栅→PGM→tesseract 管线、环境旋钮）——与 07 的「非 V8 零 JS 面」结论同源 |
| docs/pdf-organize.md | PdfOrganizer 九 API 能力矩阵（删页/旋转/重排/合并）——与 07 的 P0-3 增量写问题同域，但文档侧未涉签名保真 |
| docs/pdf-render-ext.md | links/layers 读面 + 渲染注释钉定（真实语料 661 链接实测）——07 未深挖的补充面 |
| mang-shell/docs/docx-degrade-analysis.md | POI 129 件降级面归因（18 损坏 + 14 口径不一致，仅 ~2 纯表格问题）——实测结论，质量高 |
| mang-shell/docs/docx-table-edit-design.md | 表格编辑设计草案（宽面/窄面段落序口径不一致诊断）——**设计定稿未实现**的任务书 |
| mang-shell/docs/docx-edit.md | #115 docx 可编辑化五文件结构 + 管线——与 22-editor-kernel 的 docx_edit_bridge 审计同对象（22 只引了源码未引此文档） |

判定：九件均**非游离**（有明确任务锚点/内容质量普遍良好），无需处置；登记目的是覆盖
完备性——其中 quickstart.md/async-import-design.md 两件与既有审计结论有互证价值，
建议 R6 汇总时把它们挂进相应证据链【设计偏好】。

---

## 11. 结论与最终覆盖声明

**本轮补审产出汇总**（新发现，按严重度）：

| # | 发现 | 级 | 位置 | 状态 |
|---|---|---|---|---|
| N-1 | NSIS `{{VERSION}}` 无注入机制，BUILD-VERIFY 建议命令（-DVERSION）与模板语法不匹配，照跑产字面占位符进产物名/注册表 | P2 | packaging/nsis/installer.nsi.tpl:9,15,40 + BUILD-VERIFY.md:105-107 | 已确认（静态）；需 makensis 真机复验 |
| N-2 | NSIS WebView2「引导下载」缺下载步骤：BOOTSTRAPPER URL 零使用，ExecWait 不存在的 $TEMP 文件；packaging/README.md:129 声称与实现不符 | P2 | installer.nsi.tpl:12,54-60 | 已确认（静态）；需真机验证 |
| N-3 | NSIS File 相对路径（..\..\）与 BUILD-VERIFY 示例命令（仓根执行）矛盾，必然 file-not-found | P2 | installer.nsi.tpl:29-30 vs BUILD-VERIFY.md:106 | 已确认（静态） |
| N-4 | macOS 打包零构建路径（仅 Info.plist.tpl），BUILD-VERIFY 三平台表不含 macOS——「目标平台」纯纸面 | P2 | packaging/macos/ 全目录；BUILD-VERIFY.md:111-117 | 已确认 |
| N-5 | packaging/README.md:9-10 与 checklist.md:24-25 声称 TRADEMARK_RULES「不在仓库中」——文件实在档，A5 挂起理由失效（可执行未执行） | P3（治理） | compliance_release/TRADEMARK_RULES.md 实测在档 | 已确认 |
| N-6 | compliance/README.md:4-5 的 compliance-baseline 分支叙事与当前树不符（compliance_release/ 四件在主树） | P3（治理） | compliance/README.md:4-5 | 已确认 |
| N-7 | zed_port.py 正则 TOML 解析器三起历史事故同根因，上游升级重放为单点脆弱基建；docstring 产出路径过时；上游 commit 钉值无机器校验 | P2 | scripts/zed_port.py:5,24-63,375 | 已确认（代码级）；升级重放需真机 |
| N-8 | 根目录 9 草稿 + .t.log 卫生项证据升级：9/9 完成集成锚点对账（零信息损失可删）；.t.log 定性为 Windows 开发机 cargo 警告日志（E:\ 路径泄漏 + unused-mut 实录） | P3（卫生） | §6 表 + mod.rs/settings.rs 锚点 | 已确认 |
| N-9 | web-e2e.log 非「全量运行日志」而是 12 行尾片段；k5/k45 记分板 ✅ 无仓内工件佐证；笔记 21:56「根目录 4 个」实为 6 个 | P3（审计工件） | audit/notes/test-runs/web-e2e.log；web/tests/results/ | 已确认 |
| N-10 | deb/AppImage 现捆占位图标（icon-placeholder.png 256×256 确定性生成图） | P3 | packaging/deb/build-deb.sh:40-43；§9 file 探测 | 已确认 |
| N-11 | `sed 's/mang/mdcore/g'` 全局子串反转显示名——当前无 collateral、机制脆弱 | P3 | build-deb.sh:35、build-appimage.sh:44 | 已确认（当前安全） |

**最终覆盖声明**：经本轮 a-h 补审，结合既有 24 份笔记 + 2 份报告，mdcore 仓的**产品代码
与脚本面已无未审计角落**。仍明确排除在逐行审计外的只有三类（均为既定排除，非遗漏）：

1. **third_party/zed 32 crate 内部**：作为 vendored 第三方代码，仅按消费点下钻（10/22 已
   覆盖 text/gpui 关键语义），不做逐行——审计边界决策。
2. **二进制夹具字节内容**（mang-shell/tests/fixtures/*、crates/pdf/fixtures/*、
   test-artifacts 语料）：治理面已由 13（.gitattributes/CRLF 史）覆盖，字节内容不可静态审计。
3. **gates.md 1,445 行台账全文**：作为历史注册表按需引用（6 份笔记已引），无逐行必要。

以上三类 + 「需真机验证」清单（N-1/N-2 makensis、N-7 升级重放、GUI 启动面）即交还主
审计线（R6 汇总）的全部残余。

**纪律合规**：本轮零 git 命令、零源码/文档修改、零二进制执行；py_compile 字节码全部
重定向 /tmp 并清理；唯一写动作 = 本文件。
