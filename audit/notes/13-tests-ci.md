# 13 — 测试体系可信度与 CI 门禁审计（体系级）

- 审计对象：`mdcore-audit` main@96596d7（仓库只读；未运行 cargo，运行结果引用主审计员口径/占位）
- 范围：`.github/workflows` 全量、17 个 Rust crate 的 tests/ 与内联 `#[cfg(test)]`、`web/tests` 29 个 py 套件、`test-artifacts` 语料与台账、git 历史与开放 PR #216
- 方法：全 workflow 精读 + 8 个代表性测试文件深读 + 只读 git 考古（fmt 失效链）+ rg 交叉验证软门/环境门/feature 门。**分域细节不重复**（见 01-06 笔记）；本文只做体系级分类、门禁事实核对与缺口清单。
- 基线：`#[test]` 静态计数 ≈ **1,936**（主 workspace 16 crate ≈1,373 + mang-shell 独立 workspace 563；default-members ≈1,193）。web py 35 文件 11,649 LOC（loc-stats.md）。

---

## 0. 结论速览

| 级别 | 编号 | 发现 | 置信 |
|---|---|---|---|
| **P0** | T-1 | **绿测试零验证实证**：`style_corpus.rs` / `numbering_corpus.rs` 的真实语料 sweep（声称 204 件 docx）在**任何环境（含本地）都静默空转**——`corpus_files()` 只读 `test-artifacts/health-check/corpus/` 顶层，而语料已重构进 `calamine/ docx4j/ poi-docx/` 三个子目录（实测顶层 `.docx` 计数 = 0）；`files.is_empty() → eprintln + return`，测试恒绿但断言零执行 | 高（三层实测：目录列举/代码路径/跳过分支） |
| **P0** | T-2 | **fmt 门禁在 main 上事实失效且 CI 无拦截**：main@96596d7 携带 2026-11-01 四提交（4ca8668/d5728876/4415a9b/d75a430，均已在 main）引入的 **1896 行 fmt 漂移**；复位 PR #216（分支 `origin/style/fmt-gate-restore`，tip f93f41f，2026-11-04）**未合并**（main 对其 0 ahead/1 behind）。两个 workflow 均无 fmt 步骤——漂移只能在人工复审（PR #206/#207，#161 留言）中被发现 | 高（git 考古完整） |
| **P1** | T-3 | **CI 测试覆盖 9/17 crate**：windows-test 实跑 8 个引擎 crate + mang-shell；**mdcore-wasm（148 #[test]）不在 CI**——它是唯一现役产品路径（web 桥）的 wasm 绑定层；office(78)/shell(44)/mdbridge(21)/relay(9)/settings(2)/pdf-wasm(5) 亦只编译不运行（windows-check `--all-targets`）；shell-native 连编译都不覆盖 | 高 |
| **P1** | T-4 | **web/tests 24 个行为套件（≈389 checks，含 W1-W13/G1/G2、ime_cdp 28、contract_gate 21）零 CI**——workflows 只有 cargo 命令；`web/package.json` 的 `test` 脚本也只挂 web_smoke.py 单套件且不被任何 CI 调用。测试证据 `web/tests/results/*.json` 被 `web/.gitignore:5` 排除，**仓内不可复核** | 高 |
| **P1** | T-5 | **外部软件互通零硬门**：LibreOffice 9 个测试文件全部软门（缺 soffice → SKIP 不红）或 `#[ignore]`；CI Windows runner 无 soffice → **CI 中 LO 验证 = 0**。PDFium 面 feature+动态库双缺席 → CI 零覆盖。Word/WPS/Acrobat：零自动化（详见 §3.7——TR-2D 一次性人工 Word COM 探针是唯一例外） | 高 |
| **P2** | T-6 | WASM **只编译不运行**：全仓（除 third_party）无一处 `wasm_bindgen_test`；`cargo check -p mdcore-wasm --target wasm32` 是 wasm 面的唯一 CI 动作。148 个 wasm-crate 测试只在 native 宿主跑且不在 CI | 高 |
| **P2** | T-7 | 性能门禁断言分裂：mang-shell 有硬阈值断言（buffer.rs:2316 `elapsed < 100ms`——CI 需单线程防抖动）；mdcore r14_gates/r11_stress 为 **println 计量无阈值断言**；web G1/G2 的 p95 断言只有宽松护栏（web_smoke.py:767 `cursor_p95 < 1000ms`），gates.md 的「<16ms PASS」是**人工判读**非自动门 | 高 |
| **P2** | T-8 | 安全测试高质但测的是**未接线代码**：sanitize_security.rs 44 例教科书级敌意断言，但 mdc-office 零生产消费者（01 笔记 P2-7）——安全防线在 doc 语气里，负向断言锁的是死代码 | 高（引用 01） |
| **P3** | T-9 | 声称数字三口径漂移：README「850+」（:10）/「361」（:55）/ COMPLIANCE「782+」（:131）全部低于静态实测 1,373；「19 web e2e 套件」实为 29 个 py 文件；contract_gate「16 checks」实为 21 | 高 |

**一句话判断**：CI 绿灯证明的是「9 个 crate 的内部模型一致性」；用户可见正确性所依赖的四类证据——真实文件互操作、外部软件读回、渲染像素、wasm 运行时行为——全部位于环境门 / `#[ignore]` / 本地人工流程之后，**绿 ≠ 用户可见正确**（TR-2D 已实证：引擎全绿时代，真 Word COM 探针抓出 P0 级导出缺陷，见 §3.7）。

---

## 1. 测试分类台账（用户指定 9 类）

口径说明：「存在」= 有可执行断言且默认环境会跑；「环境门控 SKIP」= 存在但缺环境即静默/显式跳过；「不存在」= 全仓无对应测试。

### 1.1 单元测试 —— 存在（密度高）
- 形态：src 内联 `#[cfg(test)]`（mdcore 240 例中约半数在 src；office-io docx.rs 62/xlsx_edit.rs 15/write.rs 15；mang-shell 563 例中 src 占绝大多数：buffer.rs 75、rpc.rs 39、highlight.rs 36、find_bar.rs 27…）。
- 覆盖范围：纯函数/数据结构语义（op 求逆、UTF-16 映射、样式链、编号计算器、路径正规化、find/替换、括号匹配、围栏解析、菜单/命令注册表）。
- 评价：断言对象多为**内部数据结构**（Block/Tree/RunData/CellScan），密度业界偏高；mang-shell 侧含部分「源码文本断言」类（见 §4.8）。

### 1.2 模块集成测试 —— 存在（主力形态）
- office-io：29 个 tests 文件（docx_roundtrip 2673 行、docx_roundtrip_matrix、xlsx_edit_roundtrip、docx_fields、tr13-tr31 系、rebuild/rebuild_recalc、xlsx_chart/condfmt）。
- pdf：15 个 tests 文件（pdfb 骨架→pdfi 签名→pdfj 高级表单全链；engine-lopdf 面默认可测，pdfium 面 feature 门）。
- mdcore：19 个 tests 文件（r 系/t 系/tr1/spike）。
- 断言对象：写→读全环的自有模型对账 + 容器门敌意归类（含 `#[should_panic]` 崩溃面直探针，docx_roundtrip.rs:16）。

### 1.3 跨 crate 组合测试 —— 存在但消费者多为测试自身
- workbench（evidence+office-io，7 文件）；mdbridge（mdcore↔collab 互译，3 文件——**零生产消费者**）；relay（collab+canvas harness）；mang-shell（office-io+pdf+canvas 三引擎消费面）；mdcore-wasm（聚合 6 crate 的 wasm 出口，8 文件）。
- **CI 现状**：workbench/collab/canvas 在 CI；mdbridge/relay/mdcore-wasm 不在（T-3）。

### 1.4 端到端测试 —— 存在但零 CI（T-4）
- **web/tests：24 个行为套件 ≈389 checks**（Playwright 真浏览器 + 真键盘/真 IME CDP + 真 localStorage）：web_smoke（W1-W13+G1/G2，56）、docx_export（30）、editor_toolbar（26）、ime_cdp（28）、contract_gate（21，README 称 16）、oversized_web（22）、xlsx_grid（22）、canvas_view/evidence_panel/concurrent_web/pdf_view/lifecycle_web（15-16）…另有 5 个探针/生成器（probe_gate2/probe_ime_cdp/diag_heap×2/gen_pdf_fixture，0 checks）。
- mang-shell：docx_bid_e2e（LO 开档软门）、docx_edit_e2e。
- workbench：eight_step_loop（真文件八步闭环）。
- **结论（事实）**：以上 py 套件**不存在于任何 CI**——`.github/workflows` 仅 cargo 命令；需本地 `npm run build:full` 产 dist+wasm pkg（均 gitignore）。README:10 的「19 web e2e 套件全绿」是本地人工口径且计数漂移（实 29 文件/24 套件）。

### 1.5 真实文件测试 —— 存在（分层）
- 入库二进制夹具：pdf fixtures 9 件（small/bookmarked/signed_sample/scanned_mixed/form_*）+ mang-shell smoke 系列（smoke.docx/xlsx/pdf/md/canvas/image_docx + smoke_font.ttf）+ formula-financial（fin_lo.xlsx=**LO 真值 oracle**）+ TR2/F1/F2 golden docx。
- 真实语料（入库）：external-corpus（CommonMark spec 655 + markdown-it 652 + pandoc 4 + POI 6，`mdcore/tests/external_corpus.rs` 以「下降即红」基线门常驻——**CI 实跑**）；health-check/corpus（calamine 76 + poi-docx 130 + docx4j 502）由 mdcore-wasm health_check 三维门消费（**不在 CI**）+ docx_edit_corpus（`#[ignore]` 手动跑）。
- **环境门控/静默失效**：style_corpus/numbering_corpus 204 件语料 sweep **静默空转**（T-1）；mdcore/office-io external_corpus（POI 6 件）入库且 CI 实跑（l3 门）。
- 大文件（R-1 1GB 级）：生成器+度量脚本入库、二进制不入库——**不存在于任何默认测试面**。

### 1.6 外部软件互通测试 —— 存在但全部软门/一次性（T-5）
| 外部软件 | 实现位置 | 门形态 | CI 中 |
|---|---|---|---|
| LibreOffice | docx_fields/docx_notes/docx_table_advanced/numbering_corpus/style_corpus(C组)/xlsx_chart/xlsx_condfmt/xlsx_edit_roundtrip/rebuild_recalc/mang-shell docx_bid_e2e | `Command::new("soffice")` 探测失败→SKIP 不红（docx_fields.rs:53-61 含 `/usr/bin/soffice` Unix 候选；docx_bid_e2e.rs:315-320 ENV_MISSING 证据行） | **0**（Windows runner 无 soffice） |
| LibreOffice（探针级） | g1/g2_lo_probe.rs | `#[ignore]` 且**测试体只产夹具不调 LO**（g1_lo_probe.rs:7-19；LO 验证为仓外手动轮） | 0 |
| LibreOffice（真差分） | docx_roundtrip_matrix.rs:473（`#[ignore]` soffice+pdftotext）、formula/real_xlsx_lo.rs（恒跑层=入库 fin_lo.xlsx 缓存真值 + soffice 活性层环境门） | 显式手动 | 0 |
| PDFium | pdfc_engine（14 锁，`#![cfg(feature="engine-pdfium")]` + 「不静默 skip」panic 合同）、pdff3 交叉面、pdff4_ocr（tesseract 双门） | feature+动态库双门 | **0**（CI 只开 engine-lopdf） |
| openpyxl（Python 独立读回） | web/tests/xlsx_export_openpyxl.py（第三方消费者视角） | 本地 | 0 |
| CommonMark spec/markdown-it | mdcore/tests/external_corpus.rs（输入面注入+幂等+保留率基线门） | 无门（语料入库） | **实跑**（唯一进 CI 的外部语料面） |
| 真实 IME | web/tests/ime_cdp.py（CDP 级 28 checks） | 本地 | 0 |
| **Word/WPS/Acrobat** | **不存在于任何测试套件**（rg 全仓除 third_party 零调用） | — | 0 |

### 1.7 性能测试 —— 存在但三种互不一致的纪律（T-7）
- 硬阈值断言：mang-shell buffer.rs:2315-2317（千处批量替换 <100ms + 千处撤销验收线）、CI 以 `--test-threads=1` 防抖动（windows-test.yml:132-135 注释自认「4 核 runner 会抖动」）。
- 计量留痕无断言：mdcore r14_gates.rs（头注明示「不做脆弱的时间硬断言」）、r11_stress；pdfc_perf.rs `#[ignore]`（「阈值=实测×3」人工口径）。
- py 侧宽松护栏：web_smoke.py G1 `cursor_p95 < 1000ms`（:767）——gates.md 的「<16ms 硬门 PASS」是人工判读。
- R6 B-1..B-10 MD 面基准大部分空位（01 笔记已登记）。

### 1.8 安全测试 —— 存在（高质量）但防线未接线（T-8）
- office/tests/sanitize_security.rs（44 例，逐攻击向量精确断言，深读见 §6）+ sanitize_gates.rs（18 性质测试）。
- office-io 容器三门敌意归类（corpus_harness 三分 OK/REJECT/ERR；POI fuzzer 17 件损坏样本门拒绝）。
- collab/tests/hostile.rs、canvas 并发不变量、web hostile_ui.py（空态/超大/特殊字符/连击/localStorage 污染六组）。
- pdf pdfe_security（加密态拒写先于零脏短路等次序锁）。
- 敌意测试轮台账：test-artifacts/TEST-ROUND-REPORT.md（87 用例 0 crash 结论）+ EXTERNAL-TEST-REPORT.md（1317 扒取用例）——一次性人工轮。

### 1.9 跨平台测试 —— 事实上「Windows 单平台」
- 存在的跨平台意识件：tr1_line_endings.rs（三入口行尾）、n3_word_count 行尾归一化（5c37151）、brackets_corpus 显式 `\n`（7ecbc8d）、`.gitattributes` binary 全量标记、CI 的 small.pdf=1099 字节 CRLF 守卫（windows-test.yml:36-48，**仅此一处**）。
- 环境门控 SKIP：canvas `golden_svg_matches_fixture` 被 CI `--skip`（windows-test.yml:83-88 自认「Windows 与 Linux 浮点格式化末位有差异……跨平台应改容差比较（待后续任务）」）。
- 不存在：Linux CI、macOS CI（零验证——packaging/Info.plist 声称 macOS 目标）、字体名/路径大小写守护（mang-shell pdf 引擎面 `#[ignore]` 需 libpdfium+smoke_font.ttf，pdf_doc.rs:603-657）、shell-native Linux 构建守护（CI 排除）。

---

## 2. CI 门禁事实表（逐门核实）

| # | 门 | 声称/预期 | 事实（HEAD 实测） | 证据 | 判定 |
|---|---|---|---|---|---|
| 1 | **cargo fmt** | gates.md 多轮「fmt 0 diff」「门禁复位」 | **无 CI 步骤**；main 携带 1896 行漂移；PR #216 开放未合；历史上已三次事故（§3.1） | windows-check.yml / windows-test.yml 全文无 fmt；git 考古见 §3.1 | **失效（P0/T-2）** |
| 2 | **clippy** | gates.md 各轮「clippy 零新增」；rust-toolchain.toml:13 声明 clippy 组件 | **无 CI 步骤**；mang-shell/src/lib.rs:8 `#![allow(clippy::all)]` 整体豁免；mang-shell/Cargo.toml:4 自认「主仓 clippy/fmt 不触达本目录」 | 两 workflow 无 clippy；rg 验证 | **不存在（人工纪律）** |
| 3 | **Linux 基线** | README:10「Linux ✅ 全量测试基线 cargo test --workspace 850+」；COMPLIANCE.md:131「全门禁 782+ 口径」 | **无 Linux job**（全部 CI=windows-latest ×2）；「基线」= 开发者本机复现 | 两 workflow runs-on | **本地口径冒充平台门（P1）** |
| 4 | **Windows 编译** | — | cargo check --workspace --all-targets --exclude mdshell-native（覆盖全部 16 crate 测试目标编译）+ wasm32 check | windows-check.yml:49,52 | 存在（唯一健全门） |
| 5 | **Windows 测试** | — | 实跑 **8 引擎 crate**：mdcore / office-io / pdf(--features engine-lopdf) / formula / canvas(--skip golden) / evidence / collab / workbench + mang-shell job（check + test --lib 单线程 + test --tests --skip real_rust_corpus_full_sweep 单线程） | windows-test.yml:68-97,128-145 | 存在但 9/17 crate（T-3）；两个显式 skip + 单线程妥协 |
| 6 | **WASM 运行** | （README 未声称） | `cargo check -p mdcore-wasm --target wasm32`（**只编译**）；全仓无 wasm_bindgen_test；148 个 wasm-crate 测试 native-only 且不在 CI | rg `wasm_bindgen_test` 零命中 | **不存在（T-6）** |
| 7 | **PDFium 矩阵** | README:56-58 列出 pdfium feature 测试命令 | **无矩阵**：CI 不装 pdfium.dll、不设 MDC_PDFIUM_LIB、不开 engine-pdfium feature；engine_pdfium.rs:147 编译期候选含仓外 `cleanroom-analysis/...` 取件位 | 两 workflow 全文 | **不存在（T-5）** |
| 8 | **LibreOffice 硬门** | 多个测试文档「本机 soffice 25.2/26.2」 | 全部软门（缺件 SKIP 不红）或 `#[ignore]`；CI Windows 无 soffice | §1.6 表 | **软门（P1/T-5）** |
| 9 | **Word/WPS/Acrobat** | docx_table_advanced.rs:13-14「真 Word 验收登记 Lead 真机」欠条 | **零自动化**；唯一真机证据=TR-2D 一次性人工 Word COM 探针（2026-10-26，@51da472，已归档 test-artifacts/TR2/） | rg 零命中 + §3.7 | **不存在（P1）** |
| 10 | **scan.sh 合规门** | README:61-62「每次提交前 PASS 才可合入」 | bash 脚本 + **无 CI 调用**（Windows runner 未挂 git-bash 步骤）；「每轮收口双绿」纯人工 | 两 workflow 无 scan.sh | **人工纪律（P2，01 笔记 D-4 同源）** |
| 11 | **web e2e / py 套件** | README:10「19 web e2e 套件全绿」 | **零 CI**（T-4）；npm test 仅 web_smoke.py 也不被调用 | workflows + package.json | **不存在于 CI（P1）** |
| 12 | **二进制夹具完整性** | .gitattributes binary 全标记 | CI 守卫仅 small.pdf 一处（1099 字节）；其余夹具裸奔依赖 .gitattributes 正确性 | windows-test.yml:36-48 | **单点守卫（P3）** |

### 2.1 fmt 门禁失效证据链（完整时间线，git 实测）

1. **事故一（工具链漂移假红）**：无钉住时代 rustfmt 1.10.0 对旧工具链格式报全仓红（459 diff/32 文件）→ 根治 = rust-toolchain.toml 钉 1.99.0 + fmt/clippy 组件声明（rust-toolchain.toml:1-13 头注原文）。
2. **事故二（一次性清扫）**：gates.md:1116-1122 记录 `cargo fmt --all` 清扫 492 diff/34 文件，「清扫后 --check 0 diff——fmt 门禁自此跨环境可复现」。
3. **事故三（当前，未收口）**：2026-11-01 四提交入 main（4ca8668 盖章 / d5728876 表单校验 / 4415a9b canvas 渲染测试 / d75a430 高级表格写面，`git merge-base --is-ancestor` 四者均 IN main）引入 **1896 行漂移** → `cargo fmt --all -- --check` 在 main 事实失效。PR #206/#207 人工复审发现（#161 2026-11-04 留言登记）→ 复位 PR **#216**（`origin/style/fmt-gate-restore`，tip f93f41f「style: fmt 门禁复位——pdf/canvas 侧既有漂移归口清零」，18 文件 +700/−283）**截至审计时点未合并**（main 与该分支 0/1 关系）。
4. PR 自述根因原文（f93f41f 提交信息）：「**CI 未拦原因：windows-check/windows-test 均不含 fmt 步骤**——建议后续在 CI 补 fmt 门禁步骤（另行 issue/PR，本 PR 不扩面）」。
5. 附注：main 分支漂移的**直接受害者**是 fmt 可复现性承诺；rust-toolchain.toml 的组件声明（为门禁自动补装 fmt/clippy 而设计）在 CI 中完全未被利用。

### 2.7 Word/WPS/Acrobat 互通现状（重要更正性事实）

- **自动化：零**。全仓（除 third_party）rg `winword|acrobat|COM 调用` 无任何测试代码命中。
- **一次性真机证据存在**：`test-artifacts/TR2/TR-2D-word-deep.md`（2026-10-26，Windows + Office 16 COM，被测 @51da472 本地 checkout）——`word_probe.ps1` 用 `New-Object -ComObject Word.Application` 逐段取证样式/字号/TOC/页边距，`word-com-raw-output.txt` 为原始输出；第二批用真 Word/Excel COM 生成 word-complex/word-merged/excel-complex 三件做导入对账。**发现 P0×1（行内粗体星号原样输出）+ P1×3（全部 10pt Times New Roman、TOC=0、页边距写死）**。
- 该报告对云端的修正原话：「『零 P0』(Linux+LO) vs **P0 至少 1 条**（Windows+Word）」「『API 存在』和『用户能用』是两回事」——**这是「绿测试≠用户可见正确性」的仓内自证**。
- WPS：零真机。仅语料生成器指纹分析（field-code-semantics.md:14 `WPS Office_12.1.0.29121`）+ 退化域读面语义钉定（tr31_degenerate_field.rs）+ 勘误预留条款（field-code-semantics.md:86「若 Word/WPS 实机证据推翻共识则重开裁决」）。
- Acrobat：仅纯函数约定（pdfd_session_flush.rs:1「Acrobat 互通纯函数约定」——/T origin 编码 + /M 日期串的可逆性），零读回验证。

---

## 4. 代表性测试深读（8 文件，断言对象判定）

| # | 文件 | 断言对象 | 用户可见性 | 深读发现 |
|---|---|---|---|---|
| 1 | `crates/mdcore/tests/r14_gates.rs`（r 系） | `debug::dump_tree` / `blocks_json` / MD 序列化 / revision——**内部结构 + 序列化面** | 中（MD 序列化=文件面） | 性能全部 `println!` 无阈值（头注自认策略）；**fake clock** `set_clock(Arc::new(\|\| 1_000))`（:90,:110）驱动 undo 合并窗测试；串行锁防 p95 污染（:17-24 注释实测并行 33ms/串行 6ms） |
| 2 | `crates/office-io/tests/docx_roundtrip.rs`（roundtrip 系，2673 行） | 自有读模型（Block/RunData/Table）——**自产自读互证** | 低-中 | 全环 = DocxWriter 写 → **同一 crate 的 read_docx** 读回逐面断言（:53-119）；含敌意容器门归类 + `#[should_panic]` 引擎崩溃面直探针（:16）；生成器与解析器同源——**内部一致性≠Word 能读**（Word 侧证据仅 TR-2D 一次性） |
| 3 | `crates/pdf/tests/pdfd_session_flush.rs`（pdfc 系） | **MockWriter 统计模型**（追加=8+32×n）+ 纯函数（origin/日期编码） | 低（无真实字节） | 写路径契约用 mock 验证；「Acrobat 互通」= 纯函数约定（§2.7）；**fake clock** `set_clock_ms(1_700_000_000_000)`（:76）；失败路径覆盖好（S5 加密先拒/无写器 typed 错/保存失败重试安全/幂等） |
| 4 | `mang-shell/tests/render_170_xlsx.rs`（render_170 系） | 渲染行模型 × **独立 zip/XML 解析对账**（自实现 EOCD/中央目录扫描器 :81-142，不信任被测引擎） | **高**（渲染行=用户所见） | 质量标杆：坐标级值锚点（非子串 contains）+ 公式/合并/超链接计数对账源 XML；布尔小写 `true` 钉定为口径（:26-27——与 Excel 显示 `TRUE` 偏差，06 笔记 X-6）；夹具由 rust_xlsxwriter 生成（写面同源——夹具漂移以 python3+zipfile 预检补偿 :33-36） |
| 5 | `crates/workbench/tests/eight_step_loop.rs` | sidecar 内部状态（evidence/linkage/calc/review 三态 + 审计链 seq）为主 | 中 | **fake clock** `t=&mut 1000u64; tick()+=100`（:65-69）——全部时间戳合成；含两个**用户可见级**断言：导出=正文原样字节（:259-260 `body_bytes==export_bytes`）+ read_docx 读回段数（:265-269，注释自认「Word/WPS 可开的**最小代理**」——代理≠真实可开）；失败路径 fail-closed 矩阵完整（:476-647：越界/数值不在段/格不存在/正文陈旧/来源陈旧/验证失败不产出 :647） |
| 6 | `crates/collab/tests/concurrency.rs` | yrs 文档模型（block 文本序列/双端收敛） | 低（模型级） | 双端全量+增量收敛、并发编辑收敛、undo 后同步——**模型行为**测试，无任何 UI/网络/真实多进程面（relay/canvas harness 另测） |
| 7 | `crates/office/tests/sanitize_security.rs`（安全，44 例） | 消毒函数的**精确输入→精确输出** | 低（纯函数） | 见 §6——负向断言质量标杆，但被测 crate 零生产消费者 |
| 8 | `crates/formula/tests/real_xlsx_lo.rs` | 引擎重算值 × **LibreOffice 缓存真值**（外部 oracle） | **高**（数值正确性） | 全仓唯一「真外部 oracle 恒跑」设计：fin_lo.xlsx（LO 预重算缓存值入库）逐公式格对照（≥45 格下限护栏防跑空 :188）+ 手算锚点 6 处 + 编辑级联重算；soffice 活性层环境门（:263-268 缺件 SKIP） |

补充（第 8+ 个，源码断言类）：`mang-shell/tests/perf_wiring.rs`——**断言对象是源码文本本身**（`main_rs.contains("mod perf;")`、`boot_pos - main_pos < 400` :44-56）。同族：module_wiring / wiring_integrity / command_coverage / ui_wiring_faces。这是「grep-as-test」接线完整性锚点（#168/#183/#169 谱系），诚实自洽，但**与用户行为零关系**——它能保证测量点在源码里存在，不能保证运行时真的被走到/数据正确。

### 4.9 「测试与真实用户行为不一致」具体例子（抽查实证）

1. **eight_step_loop.rs:269**——断言注释「导出文档结构完整（Word/WPS 可开的最小代理）」：实际断言的是 `read_docx`（自家解析器）能读回 5 个块。自家可读≠Word/WPS 可开（TR-2D 曾实证两者背离：引擎自报全绿、真 Word 打开后粗体丢失/字号全错）。
2. **pdfd_session_flush.rs:1**——「Acrobat 互通」测试不打开 Acrobat：只测 MockWriter 统计与 /T /M 编码纯函数；增量写产物的 Acrobat 读回零验证。
3. **docx_roundtrip.rs 全环**——「写读全环」的读者是自身的 `read_docx`；LO 差分（roundtrip_matrix `#[ignore]`）与 Word COM（TR-2D 一次性）才是外部读者，均非自动门。
4. **web_smoke.py G1**——`cursor_p95 < 1000ms` 的宽松护栏 vs gates.md 宣称的「<16ms 硬门 PASS」：门是人工判读，不是测试断言。
5. **mang-shell CI skip real_rust_corpus_full_sweep**——全仓 .rs 真实语料扫描（11 分钟）被 CI 排除：语法高亮面对真实代码语料的行为回归只在本地跑。

---

## 5. 固定输入绕过真实边界（实锤例）

| # | 形态 | 实例 | 绕过了什么 |
|---|---|---|---|
| 1 | **自产自读循环**（生成器与解析器同源互证） | docx_roundtrip / write_roundtrip / xlsx_edit_roundtrip（xlsx 侧仅对本 crate 自产 zip 文件级恒等，rust_xlsxwriter/LO 产物仅部件级——06 笔记 X-9 注记 xlsx_edit_roundtrip.rs:284-293） | 双方共享同一套模型假设与同一作者的错误——Word/Excel 生态的真实方言（pretty-print、strict OOXML、WPS 退化域）只能靠语料与 LO 软门撞出 |
| 2 | **fake 时钟** | r14_gates `set_clock(Arc::new(\|\|1_000))`；pdfd_session_flush `set_clock_ms(1.7e12)`；eight_step_loop `tick()+100`；mdcore-wasm 桥全靠宿主注入时钟（wasm32 无 SystemTime——架构性约束，REUSE.md:95） | undo 合并窗（1s）/标注时间戳的**真实墙钟行为**从未被验证；测试时钟单调完美递增，掩盖了系统时钟抖动/回拨下的分组语义 |
| 3 | **被 skip 的 golden 比较** | canvas `golden_svg_matches_fixture` 被 CI `--skip`（windows-test.yml:88，浮点末位平台差异，容差比较「待后续任务」自认欠账） | 跨平台渲染 golden 的字节级守护=零；同平台防回归仍在 |
| 4 | **静默空转语料**（最严重） | style_corpus/numbering_corpus（T-1）：语料迁移子目录后顶层扫描恒空，`assert!(files.len() >= 200)` 的「防跑空」护栏**永不触发**（is_empty 提前 return）；测试头注仍声称「204 件真实 docx」 | 204 件真实生态样本的全部断言（POI 130+LO 73）——绿但不验证 |
| 5 | **双门恒空的外部面** | pdfium（feature+库）/OCR（tesseract+语言包）/TR2 docx_edit_corpus（`#[ignore]` + MDCORE_DOCX_CORPUS 目录指定） | 真渲染/真 OCR/130 件 POI 手术编辑回归在 CI 恒空，且无「被跳过数量」的汇总断言 |

---

## 6. 负向用例覆盖

### 6.1 sanitize_security.rs 深读（44 例，敌意样本负向断言——有，且质量高）

- **形态**：44 例全部是「精确敌意输入 → 精确期望输出」的 `assert_eq!`/`matches!(Err)`（头注明示「非包含/不包含弱断言」）。五面：CSV 公式注入（C-01..C-10 + 模式对照 11 例：DDE/`+`/`-`/`@` 触发集、前置空格中和、幂等防重复编码、全角＝同形**不动**的边界钉定）、XML 控制字符（X-01..X-10：`_x0000_` 字面形态走私消歧 `_x005F_x0000_`、逐级再编码可解码性不变量、错误长度/非十六进制/大小写不构成形态的**负向拒识别**）、XML 实体（E-01..E-07：CDATA 终结符、数字实体走私 `&#x00;`→`&amp;#x00;`）、HTML（H-01..H-06：`javascript:`/`data:` 对字符转义**无效**的边界自认——钉为 url_policy 必要性证据 H-03/H-04）、Word 域白名单（F-01..F-10：嵌套注入 F-03/F-04、大小写逃逸 F-05、引号包裹放行的实测钉定 F-06、超集前缀拦截 F-10、保守误杀登记 F-09）。
- **设计-实现出入的诚实处理**：三处按实测钉定并在设计文档回写（头注 X-03/F-06/F-10）。
- **缺口**：①敌意样本全部净室自构造（头注声明「零外部恶意样本引入」——无 EICAR/真实 CVE 样本对照）；②**被测 crate（mdc-office）零生产消费者**——44 例锁住的是一条没有接进产品路径的防线（office-io 只在 doc 注释里说「必须过」，01 笔记 P2-7）；③URL scheme 面（H-03 自认无效）的真正防线 url_policy 同样无消费者。
- 结论：**负向断言有且强，但保护面是空的**——安全可信度的瓶颈不在测试而在接线。

### 6.2 失败路径测试盘点（其他域）

- **undo 失败**：mdcore t1 F10 失败回滚（03 笔记覆盖）；undo/redo 仍返回 bool 非 Result（03 笔记问题 3——失败路径可见性弱）。
- **IO 失败**：容器门三分归类（corpus_harness OK/REJECT/ERR）；字节入口不绕门（docx_roundtrip.rs:11）；`#[should_panic]` 引擎崩溃面直探针（docx-rs 依赖层 4 panic 已锁基线——05 笔记 P0 表）。
- **损坏文件**：POI fuzzer 17 件损坏样本门拒绝（style_corpus 实况记载）；health_check catch_unwind「panic=测试失败」最硬门 + Crash 夹具零 panic；pdf 引擎层装载垃圾字节→typed Engine 错误（pdfc_engine.rs:433-442）。
- **加密/权限**：pdfd S5 加密态 flush 先拒（次序锁）；pdfe_security。
- **网络/并发故障**：web sidecar_faults_web.py（sidecar 故障注入）；concurrent_web.py；collab hostile.rs。
- **缺口**：导出验证失败不产出的守卫只在 workbench（`!export.exists()` :647）一处；保存路径磁盘满/权限拒绝等真实 IO 故障注入零（全部合成内存路径）。

---

## 7. 声称数字 vs 实测（对账表）

| 声称 | 出处 | 实测 | 差异解释 |
|---|---|---|---|
| 「cargo test --workspace 850+ 用例」 | README:10 | 静态 `#[test]` 全 workspace ≈**1,373** | 850+ 是历史口径未更新；且 `#[test]`≠执行数（feature 门空目标/env skip/静默空转/`#[ignore]` 均扣减，实际有效面更小） |
| 「语义锁全绿 361 用例」 | README:55 | 同上 1,373 | 361=2026-10-13 xlsx-r1 阶段 B 时代；COMPLIANCE.md:34 同残留 |
| 「全门禁 782+ 口径」 | COMPLIANCE.md:131 | — | 第三口径并存（361/782/850+ 同页不同时期） |
| 「19 web e2e 套件全绿」 | README:10 | **29 个 py 文件（24 行为套件 + 5 工具）≈389 checks** | 01 笔记 D-5 已登记；且「全绿」证据 results_*.json 不入库 |
| 「contract_gate 16 checks（13+3）」 | README:65 | **21** 个 `check(` | R24 后扩面未同步文档 |
| 「ime_cdp 28 checks」 | README:64 | 28 ✓ | 唯一准确项（但零 CI） |
| gates.md 性能数字（TTI 156.9ms / selection p95 0.8ms / snapshot 125.2ms…） | gates.md:13-70 | 证据文件 `web/tests/results/*.json` 被 `web/.gitignore:5` 排除；r14_gates 计时=println 无断言 | **仓内不可复核**——数字只能信「曾经人工跑过」；<16ms/<600ms「硬门」无对应自动断言（§1.7） |
| 「204 件真实 docx 语料」 | style_corpus.rs:4 / numbering_corpus.rs:3 | 顶层 `.docx` 计数=**0**（语料在 calamine/docx4j/poi-docx 子目录） | T-1：sweep 静默空转，声称与行为背离 |

---

## 8. 跨平台缺口（Windows-only CI 盲区）

1. **行尾/CRLF**：CI 守卫仅 `small.pdf`=1099 字节一处（windows-test.yml:36-48）；其余全部二进制夹具（form_*.pdf、signed_sample.pdf、smoke.* 系列、fin_lo.xlsx、golden.docx）依赖 `.gitattributes` 的正确性——历史上 CRLF 事故三次（#89 xref 漂移事故→.gitattributes 建立；7b80465 标注 xref 三守卫；7ecbc8d/5c37151 行尾归一化修复）。守卫无覆盖面扩散机制。
2. **golden float**：canvas `golden_svg_matches_fixture` CI skip（平台浮点格式化末位差异）——跨平台容差比较自认欠账。
3. **字体大小写**：零守护。mang-shell pdf 引擎面（含 smoke_font.ttf q31 子集夹具）整体 `#[ignore]` 烟雾通道显式跑（pdf_doc.rs:603-657）；Linux 大小写敏感文件系统/macOS 字体名解析从未验证。
4. **macOS：零验证**（CI 无 macOS job；packaging/Info.plist 声称目标平台；third_party zed gpui_platform 的 3 个 macOS-only `#[ignore]` 测试与产品无关）。
5. **Linux：零 CI**（README 却把 Linux 列为「正式开发与验证面」）；soffice 软门的 `/usr/bin/soffice` 硬编码候选（docx_fields.rs:55）在 Windows 上无意义——即使 runner 装了 LO 也可能探测失败。
6. **mdshell-native**（GTK/WebKitGTK 面）：被 windows-check `--exclude`——Linux 构建面无任何 CI 编译守护，README「Linux ✅ 构建可运行」纯本地口径。
7. **路径分隔符**：Windows-only CI 意味着 `/` 硬编码路径在 Linux 上的行为只在本地被撞（engine_pdfium.rs:147 仓外相对路径取件位即为实例）。

---

## 9. 测试可信度表（每域一行）

| 域 | 主要测试形态 | CI 在跑 | 覆盖核心风险 | 可信度 | 最大缺口 |
|---|---|---|---|---|---|
| **mdcore（MD 核）** | 240 例：op/undo/anchor/外部重载/语料门（CommonMark 655+markdown-it 652 入库常驻） | ✅ | 内部语义/幂等/崩溃 | **高** | 性能=println 无门；undo 失败面弱（03） |
| **office-io DOCX** | 350 例：roundtrip 全环+敌意容器+域状态机+手术编辑 | ✅ | 容器安全/内部一致性 | **中高** | 自产自读循环；LO 软门；Word 侧仅一次性 TR-2D；style/numbering 语料空转 |
| **office-io XLSX** | 三门+XlsxEditor 字节恒等+重算 | ✅ | 保真/重算 | **中高** | 双路径分叉消费倒挂（06 X-2）；LO 真值 oracle 只在 formula 侧恒跑 |
| **mdc-pdf（lopdf 面）** | 四件套+forms/stamp/signature/annotate | ✅（engine-lopdf） | 模型/写链 | **中** | **pdfium 渲染面 CI=0**；Acrobat 读回零；渲染像素证据非 CI |
| **mdc-formula** | 27 例+**LO 缓存真值恒跑 oracle** | ✅ | 数值正确性 | **高**（oracle 设计佳） | soffice 活性层软门；与 Excel 行为三偏差在档（06 X-5） |
| **mdc-canvas** | 59 例+golden | ✅（skip golden） | 协同不变量/结构 | **中** | golden CI skip；渲染像素跨平台零容差 |
| **mdc-collab** | 60 例（收敛/敌意/undo） | ✅ | yrs 隔离不变量 | **中高** | 模型级——无真实网络/多进程面（relay harness 亦非 CI） |
| **mdc-evidence/workbench** | 53+31 例（八步闭环真文件） | ✅ | 三态契约/审计链 | **中高** | fake tick 时钟；断言偏 sidecar 内部态 |
| **mdcore-wasm + web 桥** | 148 例 native + **24 套 Playwright 行为级（≈389 checks）+ IME CDP** | ❌ **全零** | 唯一现役产品路径 | **测试质量高/强制力零** | T-3/T-4：最高价值面完全靠人工；results 不入库 |
| **mang-shell（GPUI）** | 563 例+render_170 对账+wiring 锚点+perf 硬阈值 | ✅（单线程+2 skip） | 编辑器逻辑/渲染对账 | **中高** | wiring/perf_wiring=源码文本断言（grep-as-test）；pdf 引擎面 #[ignore]；docx_edit_corpus 不自动跑 |
| **mdc-office（消毒）** | 62 例（44 靶向敌意 + 18 性质） | ❌（不在 CI） | 攻击向量 | **断言质量高/防线未接线** | T-8：零生产消费者 |
| **shell/mdbridge/relay/settings/pdf-wasm** | 81 例合计 | ❌（只编译） | 契约/互译 | **低（未执行）** | 无 CI 运行；mdbridge 零生产消费者 |
| **mdshell-native** | 4 例 | ❌（连编译都排除） | — | **零** | Linux 构建无守护 |

---

## 10. 必须真机/外部软件验证的问题清单（绿 CI 无法回答）

1. **Word 实开/实编辑往返**（导出面）：docx-rs 发射面在真 Word 的样式/字号/TOC/粗体保真——TR-2D 2026-10-26 发现的 P0/P1 是否已全部修复且未回归，**无任何自动面能回答**（欠条 docx_table_advanced.rs:13）。建议把 word_probe.ps1 从 test-artifacts 提升为可重跑 harness（哪怕每月一次的 scheduled workflow + self-hosted Office 机）。
2. **WPS 退化域显示裁决**：field-code-semantics.md:86 预留「Word/WPS 实机证据推翻共识则重开」——该裁决目前建立在零 WPS 实机上。
3. **Acrobat 读回增量写产物**：/T /M /AP 形态与 /Prev 链在 Acrobat 的真实呈现（pdfd 的纯函数约定≠互通）。
4. **PDFium 真渲染**（含 OCR）：CI 零覆盖；建议至少一个带 pdfium.dll 的 job 跑 `cargo test -p mdc-pdf --features engine-pdfium`（其「不静默 skip」panic 合同已为此设计好）。
5. **LibreOffice 硬门化**：9 个软门测试至少在一个 Linux CI 装 soffice 跑（docx_fields/docx_bid_e2e/real_xlsx_lo 的活性层）——同时补上 Linux 基线 job。
6. **wasm32 运行时**：引入 wasm-bindgen-test（哪怕最小冒烟集）或 headless Chrome 加载 dist 跑 `__ready`——当前 wasm 面唯一的 CI 动作是编译。
7. **真实墙钟行为**：undo 合并窗/标注时间戳在系统时钟抖动下的表现（fake clock 全覆盖的盲区）。
8. **macOS**：零验证（目标平台声明在 packaging）。
9. **真实 IME/输入法矩阵**：ime_cdp.py 仅 Chromium CDP；原生 IME（Windows IME/macOS 输入法）零。

---

## 11. P0-P3 分级汇总（含置信）

| 级别 | 发现 | 建议动作 | 置信 |
|---|---|---|---|
| **P0** | T-1 语料静默空转（style_corpus/numbering_corpus 顶层扫描 bug；「防跑空」护栏被 is_empty 短路） | 修 corpus_files 递归子目录或指认子目录；加「扫描数=0 即红」护栏；任何 env-skip 汇总断言 | 高 |
| **P0** | T-2 fmt 门禁失效在 main（1896 行漂移 + PR #216 未合 + CI 无 fmt） | 合并 #216；windows-check 加 `cargo fmt --all -- --check`（工具链已钉 1.99.0，可复现性已具备）；顺带加 clippy `-- -D warnings` 基线化 | 高 |
| **P1** | T-3 mdcore-wasm 148 测试 + 6 个小 crate 不在 CI | test-engine job 增 `cargo test -p mdcore-wasm -p mdc-office -p mdshell -p mdc-mdbridge -p mdc-relay -p mdc-settings`（全 native 可跑） | 高 |
| **P1** | T-4 web 24 套 e2e 零 CI + results 不入库 | 新增 web job（build:full + 至少 web_smoke/contract_gate/ime_cdp 三件套）；关键 results JSON 入库或作为 artifact 上传 | 高 |
| **P1** | T-5 外部软件零硬门（LO/PDFium/Word/WPS/Acrobat） | 见 §10；最低限度：一个 Linux+soffice job 把 LO 软门转硬门 | 高 |
| **P1** | Linux 基线为本地口径（README 声称 vs 零 Linux CI） | 增 ubuntu job 跑 `cargo test --workspace`（顺带覆盖 mdshell-native 编译）+ scan.sh | 高 |
| **P2** | T-6 WASM 只编译不运行 | wasm-bindgen-test 最小集或 headless 冒烟 | 高 |
| **P2** | T-7 性能门分裂（println 无断言/py 宽护栏/人工判读） | r14_gates 加阈值断言（三倍实测线）；G1 的 16ms 判定进 py 断言 | 高 |
| **P2** | T-8 消毒 crate 零接线（安全防线=文档语气） | office-io 写路径接 mdc-office（Cargo 边）或在 README 明示防线未激活 | 高（01 已立） |
| **P2** | CRLF 守卫单点 + golden float skip + 字体大小写零守护 | 守卫循环化（全部 fixtures 哈希清单）；golden 容差比较；跨平台 job 自然收敛后解开 skip | 高 |
| **P3** | 数字三口径漂移（850+/361/782+/19 套件/16 checks）+ 204 件语料声称 | README/COMPLIANCE 数字以 loc-stats 自动生成；claim-测试对齐 | 高 |
| **P3** | docx_edit_corpus #[ignore] 不自动跑；shell-native 零 CI；macOS 零验证；`.t.log`/根目录测试残片（01 卫生项） | 分批收口 | 高 |

---

## 12. 一句话裁决

**这套测试体系的单点工程质量（敌意容器门、LO 真值 oracle、独立 XML 对账、44 例消毒靶向、CommonMark 常驻语料门）在业界平均之上，但体系级的强制力几乎为零**：CI 只守护 9/17 个 crate 的 native 内部一致性，fmt 门禁已实际失效待复位（PR #216），而全部「用户可见正确性」证据（外部软件读回、渲染像素、wasm 运行、真实 IME、真机 Word）都住在环境门、`#[ignore]`、本地人工流程和不入库的 results.json 里——**绿 CI 证明的是模型自洽，不是用户所见**；仓库自己的 TR-2D（真 Word 探针在引擎全绿时代抓出 P0）已经把这个差距演示过一次。
