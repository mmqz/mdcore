# 审计笔记 30：第三轮代码测试实证（全仓覆盖轮）

- **对象**：mdcore main@`6322f86`（= 本审计分支基线；audit 分支 HEAD 仅新增审计文档）+ web e2e 面
- **日期**：2026-11-05
- **方法**：主审计员本地实跑——Rust 全量测试 + mang-shell + wasm32 门禁 + clippy/fmt 门禁 + web e2e 三套件复跑
- **口径声明**：沙箱磁盘 9.9G 限制，全部 Rust 构建带 `CARGO_INCREMENTAL=0` + `CARGO_PROFILE_DEV_DEBUG=0`（关增量/关 debuginfo；不影响测试语义与通过性，仅影响 backtrace 细节）。两次 ENOSPC 与删除 7.2G gitignored `target/` 旧缓存的过程保留在 workspace.log（`git status` 全程干净，`target/` 在 .gitignore 第 1 行）。

## 1. 执行矩阵

| # | 命令 | 范围 | 结果 | 原始工件 |
|---|---|---|---|---|
| 1 | `cargo test --workspace --exclude mdshell-native --no-fail-fast --locked` | 15 crate **一次性全量**（含 mdcore-wasm / mdc-pdf-wasm 原生面——前两轮均未实跑的面） | **142 套件 / 1,311 passed / 0 failed / 7 ignored** | round3/workspace.log |
| 2 | `cargo test -p mdc-pdf --features engine-lopdf --no-fail-fast --locked` | PDF lopdf 引擎特性面（复刻上轮口径） | 18 套件 / **182 passed / 0 failed / 1 ignored**（与上轮逐数一致；ignored = pdfium 环境门） | round3/pdf-lopdf.log |
| 3 | `cargo clippy --workspace --exclude mdshell-native --locked` | 15 crate | **5 warning（全部 unused）/ 0 error** | round3/clippy.log |
| 4 | `cargo fmt --all -- --check` | 全仓 | **1,896 行 / 134 Diff hunk**——与开放 PR #216 自述「1896 行」精确吻合（独立复核 phase2 C-3 口径成立） | round3/fmt-check.log |
| 5 | `cargo check -p mdcore-wasm -p mdc-pdf-wasm --target wasm32-unknown-unknown --locked` | 双 wasm crate（CI windows-check.yml:52 只 check mdcore-wasm；mdc-pdf-wasm 为本轮新增覆盖） | Finished ok / 5 unused-var warning | round3/wasm-check.log |
| 6 | `cargo test --manifest-path mang-shell/Cargo.toml --lib --locked -- --test-threads=1` | mang-shell lib 面（CI test-shell 同口径） | **456 passed / 0 failed / 5 ignored——三轮审计中首次实跑**（GPUI/Zed vendored 树在 Linux 沙箱编译通过） | round3/mang-shell.log:1742 |
| 7 | `cargo test --tests` 及逐 `--test <name>` ×14 | mang-shell 14 个集成测试目标 | **链接门**：bin `mang-shell` 链 `-lxkbcommon -lxkbcommon-x11` 缺失（无 sudo 不可装）→ Linux 全断；集成目标全部依赖 bin 构建 | round3/mang-shell.log:2439-2458 |
| 8 | `cargo test --workspace`（含 mdshell-native 的首试） | crates/shell-native | gdk-sys v0.18.2 构建失败（GTK dev 库缺失）——上轮排除口径复现 | round3/workspace.log 首段 |
| 9 | `npm ci` + `node build.mjs` + `python3 tests/web_smoke.py` | web e2e 冒烟（Playwright headless Chromium，serve dist/）：W1-W6 + W11 Human/AI 闭环 + W12 装饰增量等价 + W13 非 BMP 宪法 | **total 15 / failed 0 / pass true**；1MB 文档（20,448 块）typing p95 15.5ms、fallback_sync 328.6ms、undo 重渲染 146.1ms | round3/web-results/results.json |
| 10 | `python3 tr2b_l1_commonmark.py` | CommonMark L1 双向 652 例 | **652/652 idempotent & alive**（preserve_avg 0.9922；低保持段仅 Thematic breaks 0.111——与上轮一致） | round3/web-results/tr2b_l1_external.json |
| 11 | `python3 tr2b_l23_external.py`（仓外软链接桥接系统 LibreOffice 25.2） | L2 md→docx→LO 召回 4/4（min 0.9204）；L3 退化语料 6/6 alive/import_ok | 上轮「环境门不可验证」判定**被推翻**；真实根因 = 测试硬编码 `/tmp/my-project/lo268/opt/libreoffice26.8/program/soffice`（lo_extract subprocess 调用点，见 traceback :63）——见 §5 T-e | round3/web-results/tr2b_l23_external.json |

web 侧先行步骤补充：`node build.mjs` 重建 dist 后 `dist/pkg/snippets/` 为 0 文件——**P1-21（build.mjs 不拷 snippets）在 HEAD 独立复现**，手工补拷 `pkg/snippets` 后 web_smoke 方可跑通；与笔记 21 §2 根因实验互证，修复状态：未修。

## 2. 与前两轮对账

| 指标 | 上轮（笔记 16/21） | 本轮 | 口径说明 |
|---|---|---|---|
| workspace Rust 测试 | 1,241（分四组跑） | 1,311（一次性 --workspace） | 差额 = 本轮纳入 mdcore-wasm/mdc-pdf-wasm 原生 doc-tests 与全部集成面 |
| pdf-lopdf | 182/18 套件 | 182/18/1 ignored | 逐数一致 |
| fmt 漂移 | ~1,900 行 / 134 hunk（C-3） | 1,896 行 / 134 hunk | 精确一致 |
| clippy | （笔记 13 引 CI 门禁事实） | 5 unused / 0 err | 本地首跑全量 |
| mang-shell | 静态计数 563，未实跑 | lib 面 456 实跑全绿 | phase2 §7「沙箱不可装」前半句推翻（C-4） |
| web_smoke | 15 组全 pass | 15/15 | 一致；typing p95 20.3→15.5ms 属环境波动 |
| tr2b_l1 | 652/652 | 652/652 | 一致 |
| tr2b_l23 | 环境门跳过 | 4/4 + 6/6（桥接后） | 推翻（C-7） |

## 3. 环境门登记（本轮实测边界）

- **pdfium 动态库**：缺（ignored=1 维持，与上轮同口径）。
- **libxkbcommon / xkbcommon-x11**：缺 + 无 sudo → mang-shell bin 与 14 个集成测试目标 Linux 不可链接；lib 面（456）可跑。CI 注释「本地开发验证就是 Windows」（windows-test.yml test-shell job）与 README「Linux 全量基线」的可信度问题再添一层实证。
- **GTK dev（gdk-sys）**：缺 → mdshell-native 构建门（两轮一致）。
- **磁盘**：9.9G 沙箱——全仓 debug 构建两度 ENOSPC（workspace.log 留痕）；收敛于关增量/关 debuginfo 口径后 target 1.5G。

## 4. 本轮新增覆盖面（相对前两轮）

mdcore-wasm + mdc-pdf-wasm 原生测试；mang-shell lib 测试（456）；clippy 全量本地首跑；wasm32 双 crate check；web_smoke + tr2b_l1 + tr2b_l23 三套件复跑；fmt 门禁独立复核。

## 5. 测试视角新发现

- **T-a（→C-4）**：phase2 §7「mang-shell 编译与测试不可验证」前半句推翻——lib 456 绿；bin/集成面维持门（xkbcommon）。不可验证项清单应改写为「bin+集成面」。
- **T-b（新 P3）**：CI wasm32 check 只含 mdcore-wasm（windows-check.yml:52），**mdc-pdf-wasm 的 wasm32 编译面无任何 CI 门**——本轮本地补跑为绿。
- **T-c**：P1-21（build.mjs 不拷 snippets）在 HEAD 独立复现——修复状态：未修（开放中）。
- **T-d**：mang-shell 14 个集成测试目标硬依赖 bin 构建（逐 `--test` 单独跑也触发 bin 链接）——测试架构耦合点。
- **T-e（新 P3）**：tr2b_l23_external.py 硬编码 `/tmp/my-project/lo268/opt/libreoffice26.8/program/soffice`——测试不可移植；系统 LO 存在也因路径而死。仓外软链接桥接（`/tmp/my-project/lo268/opt/libreoffice26.8` → `/usr/lib/libreoffice`，零仓内改动）证明逻辑本身可跑（LO 26.8→25.2 替换未影响判定）。
- **T-f（观察，需复核）**：L3 语料 `51921-Word-Crash067.docx` 引擎导入结果为全空结构（blocks 0）而 LO 可提取内容（lo_vs_engine_recall 0.0）——退化语料的「静默空文档」行为是否属设计防御，需与 office 线复核（对照笔记 05 的 Word365 语料家族与 P0-2）。

## 6. 覆盖声明

本轮测试覆盖：mdcore workspace 15/16 crate（mdshell-native 环境门，两轮一致）+ mang-shell lib 面 + 双 wasm crate 双目标 + web e2e 三套件复跑。全部原始日志与结果 JSON 落 `audit/notes/test-runs/round3/`。
