# 资产合规检查（Round 2）

- 日期：2026-10-10；扫描根：projects/prototype 全树（排除 node_modules/target）。
- 目标：确认零未授权第三方资产（字体/图标/图片/音视频），对齐 ADR-C1 §1（专有零容忍：不接触四对象的任何图标、字体、主题文件）。

## 结果

- 二进制资产命中：**0 项**（原型面）。
- 原型面**零内嵌二进制资产**：UI 全部由系统控件/文本与代码生成，无字体文件、无图标集、无主题资源包；测试 fixture 均为纯文本 Markdown。
- 结论：资产维度 CLEAN；发行前在产品仓 mdcore/ 复跑本脚本同口径扫描（宿主壳 tao+wry+GTK3 不引入静态资产，若后续引入图标/字体须逐项登记许可证）。

---

## 增补（PDF-e 收口，2026-10-11）：首个分发资产登记 —— pdfium 预编译二进制

R24-c 打包与 PDF-c 引擎引入首个随产物分发的第三方二进制资产。此前「零二进制资产」
口径限定原型面；本节起分发资产按条目登记（取件脚本 + 版本 pin + SHA256 + 许可 +
licenses/ 全量 + 三平台状态），口径从「零资产」演进为「资产全登记」。

### A-1 libpdfium（linux-x64）—— 实取件 ✅

- **来源**：`https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8086/pdfium-linux-x64.tgz`
  （bblanchon/pdfium-binaries——Chromium PDFium 官方树的社区构建轨；ADR-P1 选型定谳，
  mupdf AGPL 排除、pdfium non-V8 选定）。
- **版本 pin**：`chromium/8086` = PDFium **157.0.8086.0**（包内 VERSION 文件实测
  MAJOR=157 / MINOR=0 / BUILD=8086 / PATCH=0）。
- **构建形态证据**（包内 args.gn 实测，安全策略 §JS 面合规）：`pdf_enable_v8=false`
  （**non-V8**）、`pdf_enable_xfa=false`、`pdf_use_partition_alloc=false`、
  `is_debug=false`、`pdf_is_standalone=true`、`target_cpu="x64"`、`target_os="linux"`。
- **SHA256（取件脚本闭环）**：
  - tgz `pdfium-8086-linux-x64.tgz`（3,788,766 B）：
    `588577cf52dabc1a444988bac841920df54cc2f141801424de97ab04f4fbb935`
    （与 fetch-pdfium.sh 内 pin 值逐位一致；2026-10-11 本轮复验重算一致）
  - 解包制品 `lib/libpdfium.so`（7,942,208 B）：
    `e995a89f11d48605a79a0e44f101ca3a621e5c3ef9ee311a1405e03b44dc5b19`
- **许可**：包内 `LICENSE` = BSD-3-Clause（Copyright 2014 The PDFium Authors）
  + Apache-2.0 全文；`licenses/` 目录 17 项内嵌第三方声明（abseil/agg23/dragonbox×2/
  fast_float/freetype/harfbuzz/icu/lcms/libjpeg_turbo×2/libopenjpeg/libpng/llvm-libc/
  pdfium/simdutf/zlib——逐项许可类型见 THIRD-PARTY-NOTICES.draft v1.2 §pdfium）。
  **分发义务**：licenses/ 全量随包（或随 NOTICES 附录全文引用），不得裁剪。
- **取件脚本指针**：分析仓 `md-line/pdf-a/spike/fetch-pdfium.sh`（版本 pin +
  SHA256 验证 + NOTICES 流程三合一纪律；升级流程 = 改 pin + 重跑 + 复验三探针 +
  NOTICES 更新，脚本头注释在档）。
- **当前存放**：分析仓 spike `bin/`（测试定位序：MDC_PDFIUM_LIB env → 产品仓 bin/
  → spike bin/ → 系统）；产品仓随 PDF-e 后按 gates.md PDF-c 前瞻入 `bin/` 取件位。
- **取件状态**：2026-10-05 取件（mtime 实证）/ 2026-10-11 SBOM 收口轮复验
  （SHA256 重算 + VERSION/args.gn/licenses 逐项读取）。

### A-2 libpdfium（macos-x64）—— 占位：分发时取件声明 ⏳

- 同源同 pin（`chromium/8086` non-V8），制品名 `pdfium-macos-x64.tgz`（fetch-pdfium.sh
  的 ARTIFACT 切换 macos-x64 后重跑）。
- **状态**：未取件（本环境 linux 容器）。**分发 macos 产物前**：取件 + SHA256 实测
  回填本节 + pin 回填脚本 + licenses/ 复核 + NOTICES v1.2 §pdfium 占位行转正。

### A-3 libpdfium（win-x64）—— 占位：分发时取件声明 ⏳

- 同源同 pin（`chromium/8086` non-V8），制品名 `pdfium-win-x64.tgz`（同上流程）。
- **状态**：未取件。分发 win 产物（packaging/nsis）前完成 A-2 同款闭环。

### 登记纪律（后续资产增量沿用）

1. 每个分发资产必须登记：来源 URL、版本 pin、SHA256（tgz 与解包制品双值）、许可、
   licenses/ 内嵌声明清单、取件脚本指针、平台状态。
2. 占位条目（未取件平台）分发前必须转正（取件 + 回填 + 复验），不得带占位发布。
3. 升级 = 改 pin + 重跑取件脚本 + 复验三探针（spike）+ NOTICES/ASSET_CHECK 同步更新。
4. 若后续引入图标/字体等静态资产（宿主壳/品牌资产），逐项登记许可证并回到
   「零未授权资产」核验流程。

---

## 增补（v1.3 xlsx-r1 阶段 B 对账，2026-10-13）：office 线零新资产确认

- 本轮（mdc-office-io 落地）引入的首批 Office 三方依赖**全部为源码 crate**
  （calamine/zip 族 16 项，NOTICES v1.3 正文 + 分类账），**零二进制资产**、
  零图标/字体/静态资源——资产登记面无增量。
- A-1/A-2/A-3 条目状态不变（pdfium 三平台维持：linux 实取件在档 /
  macos、win 占位待分发时转正）。
- 结论：资产维度维持 CLEAN；「资产全登记」口径下无未登记新面。

---

## 增补（v1.4 xlsx-r2 Phase 1 对账，2026-10-14）：写面产品化零新资产确认

- 本轮（rust_xlsxwriter 升产品依赖 + tempfile 链进 normal 面，NOTICES
  v1.4 正文 101→106）新增的 5 项正文组件**全部为源码 crate**，零二进制、
  零图标/字体/静态资源——资产登记面无增量。
- tempfile 的运行时临时文件为用户态会话工件（非常驻分发物），不属
  资产登记域。
- A-1/A-2/A-3 条目状态不变（pdfium 三平台维持）。
- 结论：资产维度维持 CLEAN；「资产全登记」口径下无未登记新面。

---

## 增补（v1.5 xlsx-r2 Phase 3 对账，2026-10-14）：DOCX 切片零新资产确认

- 本轮（docx-rs 0.4.22 + base64 0.22.1 入账，NOTICES v1.5 正文
  106→108）新增 2 项正文组件**全部为源码 crate**，零二进制、零图标/
  字体/静态资源——资产登记面无增量。
- docx-rs 的 image 特性已禁用（default-features = false）——图片
  编解码巨树不入闭包，亦无媒体工件。
- A-1/A-2/A-3 条目状态不变（pdfium 三平台维持）。
- 结论：资产维度维持 CLEAN。
