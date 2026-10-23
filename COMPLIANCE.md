# COMPLIANCE.md — 合规基线与门禁（mdcore 产品仓）

> 本文件是产品仓的合规基线声明。依据：ADR-C1（闭源最严姿态，存分析仓
> `decisions/ADR-C1-license-posture.md`）。适用范围：本仓全部内容与全部分发形态。

## 1. 姿态（ADR-C1 摘要）

- **闭源最严**：GPL/AGPL/未知名许可依赖零容忍；上游特有命名（类型名、函数名、
  模块名、事件名、字符串造词）零容忍；解密专有源与未发布竞品二进制零参照。
- **可复用面**：行为锚点（思想/功能层，如撤销合并窗、渐进打开、块身份）+
  白名单许可（MIT/Apache-2.0/BSD/ISC/MPL-2.0 动态链接例外）依赖。
- **表达独立**：结构、控制流、模块分解、数据结构选择必须自研——改名式合规不构成
  免责（同功能异结构原则）。

## 2. 审计终态（2026-10-10，证据存分析仓 compliance_fullcross/ + compliance_behavior/）

| 路 | 对象 | 结论 |
|---|---|---|
| GPL 上游 | Zed 系全量源（STRICT ~195 + NS 25 特征针） | 0 命中 0 提及 |
| 解密专有源 | Typora 全量解密源（298 针） | 0 violation |
| 未发布竞品 | delta 逆向 81 万行（302 针） | 0 violation |
| SBOM+资产+专有 d.ts | Obsidian d.ts（136 针）+ SBOM | 全白名单、零资产、0 命中 |

编译产物反编译自检（RC-S/R22-f）：审计态/发行态双 profile，品牌词 0、
发行态特有符号 0 行。产品面品牌词零命中（`cid` 按 AUTO-SHORT 通用缩写判留，处置登记在案）。

## 3. 常驻门禁（每轮收口必须双绿）

1. `bash compliance/scan.sh`（denylist 门禁：品牌词/上游特有词扫描，strict 0 / ns 0）
2. `cargo test --workspace`（语义锁全绿，**361 用例**——2026-10-13
   xlsx-r1 阶段 B 实跑口径：PDF-e/T2-fix 259 + T3 复用 8 = 267 +
   office-impl T1 23 [18 集成 + 5 文档] = 290 + 阶段 B 71 [office
   url_policy 16 单测 + 消毒五面靶向安全 44 + office-io io 单测 9 +
   17 件语料 harness 2]；46 test result 行全 ok / 31 非空套件；
   290 基线只增不改；feature 面计数见 gates.md PDF-e 节门禁——
   lopdf 79 / pdfium 81+6i / dual 109+6i 本轮复验逐位一致零波及）
3. 启用新系统依赖（wry/tao/GTK 系）后：重跑 SBOM 并更新
   `compliance_release/THIRD-PARTY-NOTICES`（LGPL 系统库动态链接政策）。

### feature 面前置 bootstrap（T1-F12，2026-10-10 成文）

`cargo test -p mdc-pdf --features engine-pdfium[,engine-lopdf]` 的测试
载荷（libpdfium.so + fixtures）**非净克隆自带**，需先在分析仓 spike
（`cleanroom-analysis/md-line/pdf-a/spike/`）三步生成：
① `./fetch-pdfium.sh`（版本 pin chromium/8086 non-V8 + SHA256 验证
取库）→ ② `cargo run --release`（small / large / small_annotated
fixtures）→ ③ `cargo run --bin formfill_probe --release`
（form_probe.pdf 表单探针）。载荷定位序（libpdfium.so：env
`MDC_PDFIUM_LIB` → 编译期候选清单 → 系统；fixtures：env
`MDC_PDF_TEST_FIXTURES` → 编译期候选，crate 本地
`crates/pdf/fixtures/` 优先、仓外 spike 位兜底）与完整纪律见
gates.md「T1 卫生修复轮 · feature 面前置 bootstrap」。默认面常驻
门禁（上述第 2 条）不受影响——零引擎依赖，裸环境全绿。

## 4. 发布形态套件（compliance-baseline 分支 compliance_release/）

- `THIRD-PARTY-NOTICES.draft.md`：34 第三方组件按锁定版本逐包取证（crates.io/npm），
  全白名单族、零 GPL 传染、零 UNKNOWN，与 R22-c SBOM 交叉一致。**随依赖变更必须再生成**
  （生成器脚本存分析仓 scripts/r2_gen_notices.py，可复跑）。
- **p5-t4 增量登记（pdfjs-dist）**：`pdfjs-dist@6.4.299`（Apache-2.0，
  web/package.json 首个 PDF 面三方运行时件——CM6 三包为既有正文编辑
  面依赖）。用途 = PDF 表面显示面（§八.4 成熟组件复用；路线定谳 B：
  pdf.js 渲染 + mdc-pdf lopdf wasm 写回——渲染分离、真相不分离）。
  **传递面**（审查 M-1 修正）：`optionalDependencies` 含
  `@napi-rs/canvas@1.0.13`（MIT，Node 服务端渲染可选面——浏览器
  bundle 不消费，dist 零分发二进制）+ 11 个平台二进制可选包
  （android/darwin/linux/win32 家族）——npm lock 本次 +280 行共
  **13 包**，全 MIT 白名单族；NOTICES 再生成时须全量收录（非仅
  pdfjs-dist 一项）。同轮 wasm 面：`console_error_panic_hook`
  （MIT，已在 mdcore-wasm 闭包）；lopdf/getrandom 零新版本
  （lock +11 行为 workspace 内部边与 getrandom 特性统一）。
  legacy 构建取舍：modern 路径用 TC39 upsert（Map.getOrInsertComputed
  ——webview 宿主 JSC 不支持），legacy 构建为宿主兼容面（pdf_view.ts
  注释在档）。
- `ASSET_CHECK.md`：零内嵌二进制资产核查记录。
- `TRADEMARK_RULES.md`：命名域（mdcore/mdshell/mdc-*）、四对象标记禁止事项、
  兼容表述边界、**定名前检索门**。

## 5. 遗留登记（非本仓代码缺陷）

1. 真机 IME / 真机视觉与对话框人工验收（需物理输入法环境；自动化先行部分已由
   R23-a 闭环：web/tests/ime_cdp.py 28/28，CDP 限制 1 条与人工协议并存）。
2. 正式商品名定名后商标检索（TRADEMARK_RULES §4 流程）。
3. ~~`compliance/scan.sh` + `compliance/denylist/` 随产品代码首推入 main~~
   ✅ 2026-10-10 随 main 首推落位（由分析仓审计结论重建；开发环境 R22-c 原版
   合入时以 diff 为准替换，见 compliance/README.md 重建声明）。

## 6. 分支政策

- `main`：产品代码主体（crates/mdcore、mdcore-wasm、shell、shell-native + web/ +
  CONTRACT.md + compliance/）。**禁 force-push**。
- `compliance-baseline`：合规基线套件——COMPLIANCE.md + 发布形态三件套。
  main 首推后转为对账基线（结论性声明以 main 版 COMPLIANCE.md 为权威）。
- 审计底稿（解密源对照针集、命中明细、四源材料）**只存分析仓**（私有允许域），
  本仓仅存结论与门禁。

## 7. main 首推记录（2026-10-10）

- **来源**：`mother/r23-snapshot`（24268ca）= 分析仓 e88dbdb（R23-e 阶段收口终态）
  的核侧源面，单提交 squash、历史自含（不引入分析仓历史，产品仓历史自含政策不变）。
- **提交链**：① 快照基线（24268ca 树）→ ② compliance 常驻门禁重建
  （scan.sh + denylist/sources.txt，20 词四源；product 与 product+tests 双口径
  PASS）→ ③ 本文件增注 + 产品级 README。
- **政策微调声明**：原政策"main 由持有完整 R22-a..f 实装的开发环境首推"调整为
  **"main 基线 = R23 终态核侧母本快照（shell-native 蓝图态）；R22 宿主壳实体由
  开发环境按 CONTRACT.md v1 以后续提交合入"**。理由：R23-d 契约冻结已消除
  母本↔产品仓契约漂移，main 空仓的对接/审计成本高于基线先行；开发环境合入时
  diff 即增量清单。禁 force-push 不变。
- **首推验证**：product 面 denylist 0 命中；Cargo.lock/package-lock 与 R23-e
  终态一致（依赖零增）；无 token/凭据落盘（凭据卫生纪律）。
