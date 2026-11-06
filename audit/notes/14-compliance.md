# 14 · 合规体系深审（compliance）——许可拓扑 / scan.sh / SBOM / NOTICES / 闭源阻塞

> 审计对象：mdcore 仓合规体系全组件（compliance/ + 结论文档 + 记账文件 + 四层依赖锁）。
> 方法：绝对只读——零文件修改、零 git 状态变更；scan.sh 以只读实跑复现；许可普查基于 Cargo.lock/Cargo.toml/pyproject/uv.lock 一手数据 + 已知 crate 许可知识（离线环境无 registry 元数据缓存，无法 cargo metadata 取证——见 §4.3 置信声明）。
> 基线：audit 快照（git 单提交 b476c1a，源=main）；既有终报告 P1-15 概要结论全部复核并深化。
> 产出定位：此前缺失的 14 号笔记（audit/notes/ 有 01-13/16，缺 14/15）。

置信四档：**实证**（本地复跑/逐字节验证）｜**高**（一手文件交叉≥2 处对上）｜**中**（单源或需联网复核）｜**低**（推测）。

---

## 1. 合规资产清单（全景）

### 1.1 工具与门禁（compliance/，随码走）

| 文件 | 角色 | 实况 |
|---|---|---|
| `compliance/scan.sh`（67 行） | 词法级常驻门：denylist 词 × 产品面路径，命中 exit 1 | **实跑 exit 1（5 命中，全部 packaging/ 假阳性）**；工程缺陷见 §3 |
| `compliance/denylist/sources.txt`（43 行） | 20 词四源字集（Typora 6/Delta 4/Zed 8/Obsidian 3…实际 20 词） | 第 29 行自述「Zed 源（GPL-3.0 传染对象，ADR-C1 全面禁入）」——与 19 天后的 32 crate 搬运直接矛盾 |
| `compliance/README.md` | 运行说明 + 豁免登记表 | 豁免表**空置**（L46-51「暂无」）；scan.sh **无豁免机制可消费该表**——有账无闸 |

### 1.2 结论性文档（仓根 + compliance_release/）

| 文件 | 声称 | 与现实对账结论 |
|---|---|---|
| `COMPLIANCE.md`（146 行） | 闭源最严姿态（L8「GPL/AGPL/未知名零容忍」）；四路审计全 CLEAN（L15-22，2026-10-10）；常驻门禁双绿（L27-38）；SBOM 366 全白名单（L78-80） | 姿态与仓根 LICENSE 互斥；审计终态底稿在私有分析仓**不可复核**；「双绿」现况为假（scan.sh 红）；367/366 数字与今日 lock 对不上 |
| `NOTICES.md`（30 行，根） | Zed 32 crate 记账 + GPL 10 件闭源替换清单 + 「本项目整体 GPL-3.0-or-later」（L23） | Zed crate 面记账**属实且精确**（§4.2 逐项复核）；但**零 SVG 资产条目**、零 mang-shell lock 面记账 |
| `LICENSE`（210 行，根） | GPL-3.0-or-later（L11 GPL v3 全文）；头部含 Zed attribution 句（L6-7） | 与「私有产品仓」（README:4）、「GPL 零容忍」（COMPLIANCE.md:8）三方互斥；SDK 两份 LICENSE 逐字节相同（md5 `ecb394…` 三处一致） |
| `gates.md`（1446 行） | 全轮次门禁注册表 | **零 mang-shell/Zed 搬运/GPUI 门禁条目**（grep 全文 0 命中）——仓史最大许可面变更（2026-10-29，230,756 LOC 进仓）无任何门禁注册；末轮 PDF-i（2026-10-31）为 PDF 修复轮 |
| `compliance_release/THIRD-PARTY-NOTICES.draft.md`（654 行） | v1.6（2026-10-21）：118 项 lock 条目全量核验零 GPL（L652）；四面复跑 75/88/108/121（L27-29）；npm 12 ship 项 | **停在 Zed 搬运前 8 天**；全文零 Zed/third_party 提及；其「GPL/AGPL 零容忍」条款与根 LICENSE 直接冲突 |
| `compliance_release/ASSET_CHECK.md`（104 行） | 「资产全登记」纪律（L61-68：引入图标/字体须逐项登记） | 末次增补 2026-10-14（v1.5 era）；**15 件 Zed SVG（~2026-10-30 引入）零登记**——自家纪律未执行 |
| `compliance_release/TRADEMARK_RULES.md` | 命名域/四对象禁用/定名检索门 | 结构完好；A5 归属句与「闭源 EULA 待起草」挂起（checklist.md:24-25, 68） |
| `compliance_release/trademark_search_2026-10-27.md` | mang/芒 定名检索 | CNIPA/WIPO/common-law 未查（自declared 非商用口径）；「npm `mang` 已占用」在档 |
| `packaging/checklist.md` | 分发合规核对单（A-F 六节） | C1/C3/C5 未勾；**A3 行（L20）即 scan.sh 假阳性来源之一**；A4 已执行（mang 替换） |
| `packaging/`（deb/nsis/appimage/macos 模板） | 发布面 | 四平台模板 + BUILD-VERIFY（deb 2.4MiB 实构建过）；**打包脚本零 license/NOTICES/EULA 装载**（grep 四脚本 0 命中）——GPL 分发义务（随附许可证全文 + 源码提供）未接 |
| `CONTRACT.md`（冻结 v1.6） | 契约冻结文档 | 合规面无直接角色；README:39 引「CONTRACT.md v1.2」已落后两版（文档漂移佐证） |

### 1.3 锁文件（依赖记账真源）

| 锁 | 规模 | 构成 |
|---|---|---|
| `Cargo.lock`（主 workspace，16 成员） | 424 `[[package]]` 块 / 371 唯一名 | **408 个三方条目**（424−16，含同名多版本）；与终报告「408 包」口径一致 |
| `mang-shell/Cargo.lock` | 741 块 / 673 唯一名 | 681 registry + 23 git 钉（wgpu rev ×13 + zed-industries fork ×10：xim×3/async-task/calloop/khronos-egl/font-kit/scap/oo7/wasm_thread）+ 37 path（31 vendored Zed crate + mang-shell + 5 主仓 crate：mdc-office-io/mdc-pdf/mdc-canvas/mdc-evidence/mdc-formula） |
| `third_party/zed/Cargo.lock` | 766 块 | 独立 workspace 构建面（被 mang-shell lock 功能性取代） |
| `web/package-lock.json` | 53 项 | 全 MIT/Apache-2.0（§4.5） |
| `sdk/python/uv.lock` + `sdk/mcp/uv.lock` | 52 / 162 项 | 运行时：python 侧**零三方**（纯 stdlib）；mcp 侧 `mcp>=1.2,<2`（lock 实锁 1.30.0）+ mang-r |

---

## 2. 许可拓扑（分层文字图 + 冲突矩阵）

### 2.1 五层许可现实

```text
L5 sdk（Python）        sdk/python（mang-rpc）：运行时零三方依赖（pyproject L11）
                        sdk/mcp（mang-mcp）：mcp 1.30.0（MIT，已知知识）+ mang-rpc
                        └ 两件自声明 GPL-3.0-or-later（pyproject:7 各一行）
                          LICENSE 与根逐字节同（含「incorporates code from Zed」句——
                          SDK 无任何 Zed 代码，attribution 超claim，中置信无害但失真）
L4 web（npm）           53 lock 项：CM6 族（@codemirror/*+@lezer/*+crelt+style-mod
                        +w3c-keyname）全 MIT；pdfjs-dist 6.4.299 Apache-2.0；
                        esbuild 0.25（devDep）MIT；typescript（devDep）Apache-2.0；
                        @napi-rs/canvas 平台件 13 项 MIT（pdfjs optionalDependencies）
                        └ 零 GPL 零冲突（实证：lock license 字段逐项读取）
L3 third_party/zed     32 crate / 230,756 LOC（PROVENANCE.md:83）
                        GPL-3.0-or-later 10 件 48,562 LOC ｜ Apache-2.0 22 件 182,194 LOC
                        每 crate 目录留上游原版 LICENSE-*（33 份；syntax_theme 双文件但
                        manifest 声明单选 GPL-3.0-or-later，按声明口径计 GPL）
L2 mang-shell（GPUI）  自研 ~36,850 LOC src（+6,305 tests）；manifest license =
                        "GPL-3.0-or-later"（mang-shell/Cargo.toml:20）
                        静态链接 L3 的 10 件 GPL crate（直接 3 + 传递 7，§4.2）
                        + include_bytes! 内嵌 15 件 Zed SVG（§4.4）
                        + 拉入主仓 5 crate（mdc-office-io/mdc-pdf/mdc-canvas/
                          mdc-evidence/mdc-formula——引擎代码进 GPL 二进制）
L1 主 workspace（引擎）16 自研 crate；**16 个 Cargo.toml 全部无 license 字段**
                        （workspace.package 亦无，Cargo.toml:10-11 仅 version）
                        三方 408 条目：**0 GPL**（§4.3 普查）
                        产物：mdshell-native（deb 面）+ wasm + web bundle——均不含 GPL 代码
```

### 2.2 「整体 GPL 声明」vs「主仓 0 GPL」的精确边界

- **受 GPL 传染的分发物（仅一件）**：`mang-shell` 二进制。静态链接 10 件 GPL crate（ui/text/theme/rope/component/icons/menu/syntax_theme/ui_macros/clock）+ 内嵌 15 件 GPL 口径 SVG（assets.rs:1-3 自述「随仓 GPL-3.0-or-later 口径」）。分发即触发 GPL-3.0 完整义务：随附许可证全文、提供对应源码（含 mang-shell 自研 36.8K src 与链上全部物）、不设额外限制。**闭源分发法律性阻断**（NOTICES.md:16-18「Lead 规则」记账准确）。
- **不受传染的分发物**：①mdshell-native deb（主 workspace 构建，三方 408 条目 0 GPL；BUILD-VERIFY.md §1 实构建记录）；②mdcore-wasm + web dist（wasm 面 + npm 53 项全白名单）；③sdk 两件（无 GPL 代码）。这些面的「GPL」只是**标签**：作者作为全量版权人可随时改标（无第三方 GPL 代码约束）——但当前状态下，接收方按 GPL 取得主仓引擎源码的权利是**已生效的赠与**，且 SDK 的假 Zed attribution 会误导下游。
- **边界成因**：mang-shell 是独立 cargo workspace（mang-shell/Cargo.toml:3-5「主仓门禁不触达本目录」；PROVENANCE.md:158-162），许可传染止步于该 workspace 的产物；主 workspace 编译产物零 GPL 是**结构性成立**的，非侥幸。
- **互斥三边形**（实证）：根 LICENSE=GPL-3.0-or-later ↔ COMPLIANCE.md:8「GPL/AGPL/未知名许可零容忍」↔ README.md:4「私有产品仓」。三方各自为真话的场景（GPL 壳 + 白名单引擎 + 未定分发）从未被同一文档调和——闭源/商用决策（COMPLIANCE.md:125-127 issue #7）悬空未决。

---

## 3. scan.sh 深读与误报分析（B 题全答）

### 3.1 逐行逻辑（compliance/scan.sh）

- L18-20：`ROOT=脚本目录/..`、`DENYLIST=$ROOT/compliance/denylist/sources.txt`、`MODE=$1`。
- L24 **PRODUCT_PATHS**（7 项）：`crates/*/src web/src web/index.html Cargo.toml crates/*/Cargo.toml web/package.json packaging`——packaging 为 R24-c 增补（注释 L23「发布面=产品面」）。
- L26 **EXTRA_PATHS**（--all 附加 4 项）：`crates/*/tests web/tests web/build.mjs web/build-wasm.sh`。
- L41-58 主循环：跳空行/注释（L42）；`*_` 结尾词走 `-E -e "\b<word>"` 前缀匹配（L44-46，d1_/d2_）；其余走 `grep -rIn -w -e "<word>"` 词边界匹配（L47）。
- L51：`grep … "$p" 2>/dev/null || true`——**错误吞噬点**（见 3.3-B）。
- L52-56：命中打印（`head -20` 截断）并 `fail++`（按 词×路径 计数，非命中行数）。
- L62-66：`fail>0 → exit 1`。

### 3.2 实跑结果（只读复现，实证）

- 从仓根运行 `bash compliance/scan.sh`：**exit 1，`hit-paths=5`，denylist terms=20**。五命中（词全在 `packaging/`）：

| # | 词 | 命中位置 |
|---|---|---|
| 1 | `typora` | packaging/checklist.md:20 + packaging/README.md:44 |
| 2 | `muya` | 同上两行 |
| 3 | `zed` | 同上两行 |
| 4 | `gpui` | 同上两行 |
| 5 | `obsidian` | 同上两行 |

两行的原文均为「A3 denylist 近似判定……（`mang` 与 typora/muya/delta/zed/gpui/obsidian 均不近似）」——**自证清单引用词**：打包核对单与 README 在解释「定名与四源品牌词不近似」时把 denylist 原词写进了被扫描路径。`Delta`（大写词形）与 `delta`（小写泛词，设计性不入集，sources.txt:26-27）均未命中——子串/大小写边界行为与设计一致。`d1_/d2_`（前缀模式）零命中。`--all` 模式同 5 命中（测试面无新增）。
- **结论**：五命中 100% 假阳性，零真实参考对象痕迹泄漏；但门禁因此**红了 ≥8 天无人分诊**（A3 勾选日 2026-10-27 起；trademark_search_2026-10-27.md 自述「CI 上 scan.sh --all 仍需复跑」从未执行）。

### 3.3 工程质量缺陷清单

- **A · 无豁免机制（根因）**：compliance/README.md:21-23 规定「合法独立用词 → 登记书面豁免后放行」，但 scan.sh **没有任何 allowlist 消费逻辑**——豁免表（README:46-51）是纯文档，登记了也照样红。有账无闸。
- **B · 静默假 PASS（最重缺陷，本地实证）**：`PATHS` 全为**相对路径**（相对调用方 cwd，非 ROOT）；从非仓根目录调用（如 CI checkout 变体、`bash /path/compliance/scan.sh` 于任意子目录）时 glob 不展开、grep 报「No such file or directory」被 `2>/dev/null || true`（L51）吞噬 → **全部词零命中 → PASS**。本地复现：从 `/home/z/my-project` 调用得 `PASS: product surface clean`，从仓根调用得 exit 1——同一脚本两个相反结论。安全门单向失效（只可能放过，不可能误拦）。
- **C · 覆盖面缺口**：PRODUCT_PATHS 不含 `mang-shell/src`（**当前真正的产品面**——GPUI 壳编译进发布产物）、不含 `sdk/`、不含 `scripts/`（zed_port.py 含 zed ×7 词）；`--all` 不含 mang-shell/tests。而 mang-shell/src 实测含 `gpui` 词（assets.rs:11 等）与 `zed` 词（title_bar.rs、git_panel.rs、canvas_view.rs 等注释）——若纳入将大面积命中，故该门对 Zed 搬运后的架构**实际上已无解释力**（PROVENANCE.md:161 自述「scan.sh 的 PRODUCT_PATHS 不含 third_party」）。
- **D · 模式匹配残留面**：`-w` 词边界对 `sum_tree/SumTree/language_tree`（denylist:35-37）这类下划线词有效，但对 Zed 命名域的变体（`Zed` 命中 `zEditor`？——`-w` 下 zEditor 不算 Zed 词，反向：denylist 词形仅 8 个 Zed 域词，上游 ~195 STRICT 特征针在分析仓，产品仓只剩词法影子）；子串误报已由 `-w` 抑制（「自证清单」命中是**整词**出现——词边界救不了文档引用词，需豁免/路径排除）。
- **E · 小项**：命中输出 `head -20` 截断（超限不警示）；`fail` 计「词×路径」非命中总数；`--all` 语义（product+tests）与 gates.md:347「strict 0 / ns 0」双口径术语已无对应实现（strict/ns 分类在产品仓不存在）。

### 3.4 修复建议（最小→根治）

1. **最小止血（当日可做）**：packaging/checklist.md:20 与 packaging/README.md:44 的原词列举改写为「四源品牌词」占位描述（不落原词）→ scan.sh 立即回绿，零代码改动。
2. **豁免机制**：新增 `compliance/denylist/allowlist.txt`（格式 `word<TAB>path-regex<TAB>理由`），scan.sh 在 grep 后按 (word, file) 过滤豁免命中并在输出单独计 `exempt=N`；compliance/README.md 的豁免表自动从该文件渲染——账闸合一。
3. **假 PASS 修复**：脚本头 `cd "$ROOT"`（或全部路径改 `"$ROOT/…"` 绝对拼接）；grep 前 `[[ -e $p ]] || { echo "MISSING-PATH $p"; fail++ ; continue; }`——路径缺失按 FAIL 处理，杜绝静默放过。
4. **域扩展（分阶段）**：产品面加 `mang-shell/src mang-shell/Cargo.toml sdk`——前提是 Zed 搬运线先完成豁免登记（`gpui/zed/theme/ui/text` 等词在 mang-shell 属**合法 vendored 消费**，逐词豁免+理由+日期），否则改为结构性门禁：vendored 面以 PROVENANCE+lock 哈希为准（词法门对 GPL 面本就无意义——GPL 问题在许可而非词汇）。
5. **接入 CI**：新增 `.github/workflows/compliance.yml`（当前 workflows 仅 windows-check/windows-test，grep 零 scan.sh 引用——README:61「每次提交前 PASS 才可合入」无任何机器强制，.git/hooks 仅 sample）。

---

## 4. 许可普查与 GPL 面重建（A/D 题数据底座）

### 4.1 主 workspace 408 三方条目普查（Cargo.lock）

- **GPL 家族：0 件**；AGPL：0 件（mupdf AGPL 在 SECURITY_POLICY/ADR-P1 设计性排除，gates.md:692）。LGPL 字样仅 `r-efi@6.0.0`（OR 三选一组合、UEFI 目标面不进分发——COMPLIANCE.md:79-80 与 NOTICES 草案 L612 双源一致）。
- 高风险/重点包人工判读（离线知识 + 仓内交叉）：`yrs 0.28`（MIT，mdc-collab/mdc-canvas，NOTICES v1.6 在账）；`ironcalc_base =0.9.1`（mdc-formula 唯一三方，crates/formula/Cargo.toml:12 注释自证「MIT OR Apache-2.0」——**需联网 registry 复核，中置信**）；`pdfium-render`（MIT OR Apache-2.0）；`lopdf =0.45.0`（MIT）；`calamine/zip/docx-rs/rust_xlsxwriter/statrs`（MIT 族）；`wry/tao`（Apache-2.0 OR MIT）；gtk-rs 系（MIT 绑定 + LGPL 系统库动态链接政策，COMPLIANCE.md:37-38/82-85）；servo 族 cssparser/servo_arc/html5ever/kuchikiki（MPL-2.0）；RustCrypto 族（MIT OR Apache-2.0）；`encoding_rs`（AND 复合许可，NOTICES 草案 L646-647 已记义务）；windows-* 族（MIT OR Apache-2.0）。
- **结论（高置信）**：「主仓 0 GPL」成立；与终报告口径一致。全量 100% 逐包取证离线不可达（registry 元数据不可用），408 项中仅 `ironcalc_base` 一项标记待联网复核。

### 4.2 mang-shell 链 GPL 面重建（mang-shell/Cargo.toml + Cargo.lock 双验，实证）

直达路径（mang-shell/Cargo.toml:59-80）：`gpui`（Apache）、`gpui_platform`（Apache）、**theme（GPL）**、**ui（GPL）**、`zlog`（Apache）、**text（GPL）**、mdc-office-io/mdc-pdf/mdc-canvas（自研 path）。
10 件 GPL crate 全部可达：ui→component/icons/menu/ui_macros；theme→syntax_theme；text→rope/clock（依赖链实测自 vendored Cargo.toml [dependencies] 段）。

| # | crate | 版本（mang-shell lock） | 许可（vendored Cargo.toml） | LOC（.rs 实测 wc -l = PROVENANCE 记账） | 入径 |
|---|---|---|---|---|---|
| 1 | ui | 0.1.0（path） | GPL-3.0-or-later | **29,844** | 直接 |
| 2 | text | 0.1.0（path） | GPL-3.0-or-later | **6,746** | 直接 |
| 3 | theme | 0.1.0（path） | GPL-3.0-or-later | **5,957** | 直接 |
| 4 | rope | 0.1.0（path） | GPL-3.0-or-later | **4,166** | text 传递 |
| 5 | component | 0.1.0（path） | GPL-3.0-or-later | 530 | ui 传递 |
| 6 | clock | 0.1.0（path） | GPL-3.0-or-later | 338 | text 传递 |
| 7 | icons | 0.1.0（path） | GPL-3.0-or-later | 360 | ui 传递 |
| 8 | syntax_theme | 0.1.0（path） | GPL-3.0-or-later | 345 | theme 传递 |
| 9 | ui_macros | 0.1.0（path） | GPL-3.0-or-later | 239 | ui 传递 |
| 10 | menu | 0.1.0（path） | GPL-3.0-or-later | 37 | ui 传递 |
| | **合计** | | | **48,562** | |

- 10 件 LOC 与 NOTICES.md:17-18、PROVENANCE.md:83「GPL 系 48,562」**逐位吻合**（wc -l 实测）；版本号 0.1.0 为搬运改写（gpui 保留上游 0.2.2）。
- mang-shell lock 其余 710 项：vendored Apache 21 件 + crates.io 681 + git 23——**git 钉 23 项的许可未在 NOTICES 任何层记账**（xim-rs×3/wasm_thread/scap/oo7/font-kit 为 zed-industries fork，PROVENANCE.md:53 仅一句「照抄」；wgpu rev ×13；许可需逐 fork 复核——**中置信待联网**）。
- bench_metrics（vendored 第 32 件）**不在 mang-shell lock**（bench-only）——NOTICES.md:8「32 个 crate」记账为搬运面，非编译面，口径无误但易误读。

### 4.3 置信声明

Cargo.lock 不含 license 字段；离线无 registry 元数据（cargo metadata --offline 报 ropey 解析失败，本仓 registry 缓存仅 wasm-bindgen-cli 一个 .crate）。§4.1/4.2 的「0 GPL」结论依据：①已知许可知识覆盖高频 crate；②NOTICES 草案 v1.6 的 118 项 registry 直读账目（其流程为 cargo metadata --locked 逐包取证）；③终报告同口径普查。对低频包（vecmath/zmij/cpubits/bit Field 族等长尾）为**中置信**，SBOM 复跑后应逐项转实证。

### 4.4 SVG 资产内嵌记账面（实证）

- `mang-shell/src/assets.rs:17-75`：`ICONS` 常量 15 条 `include_bytes!("../assets/icons/*.svg")`——行号 20/24/28/30/33/37/41/45/49/53/57/61/65/69/73。文件实测 15 件（9,406 字节），16×16 viewBox + stroke-width 1.2——与 vendored icons crate README 设计规范吻合（Zed 改造版 Lucide/Phosphor 系）。
- 头注释（assets.rs:1-3）自证来源 Zed 上游 `assets/icons/`、「随仓 GPL-3.0-or-later 口径」。
- **记账面缺口**：NOTICES.md 全文零 SVG 条目（仅 icons 作为 crate 名出现）；PROVENANCE.md 零 SVG 提及（L43「assets 未搬运」仅指 crate 目录树）；ASSET_CHECK.md 末次增补 2026-10-14（v1.5）早于 SVG 引入，「资产全登记」纪律（L67-68「引入图标/字体逐项登记」）未执行。**15 件 GPL 口径分发资产在三层记账文件中均无账**——终报告「超账」判定实证成立。

### 4.5 web / sdk 许可面（实证）

- web/package.json:16-25：deps 4（CM6 三包 + pdfjs-dist）+ devDeps 2（esbuild/typescript）。package-lock 53 项 license 字段逐项读取：**全 MIT/Apache-2.0**（pdfjs-dist Apache-2.0；@napi-rs/canvas 族 13 项 optional 平台件 MIT）。零冲突；`package.json` 无 license 字段（private:true，小缺口）。
- sdk：见 §2.1 L5——运行时零三方（python）+ mcp 1.30.0（MIT，中置信待联网）；pyproject license 字段两件均 `GPL-3.0-or-later`；LICENSE 三份逐字节相同（含 Zed attribution 句——SDK 无 Zed 代码，**attribution 失真但方向为过度声明**，无害下游却损害自家闭源弹性）。

---

## 5. SBOM / NOTICES 缺口（C/D 题全答）

### 5.1 SBOM 现实

- **仓内不存在任何 SBOM 文件**（全树 find `*sbom*|*spdx*|*cyclonedx*` 零命中；终报告同结论复核成立）。声称链：README.md:79「SBOM 367 项全白名单」← NOTICES 草案 L633「沿 R23 367 项口径」← COMPLIANCE.md:78「366 三方包去重」。
- **不可复核性三证**：①生成器在分析仓（COMPLIANCE.md:81-82「脚本：分析侧 scripts/sbom_check.py…可复跑」——路径不在本仓，无离线复跑可能）；②数字互相漂移（367 vs 366 vs 今日实况 424 块/371 唯一名/408 三方条目/355 唯一三方名——**四种计数都对不上声称值**）；③声称时点（R23/s5a-env）早于 ironcalc_base/yrs/docx-rs/rust_xlsxwriter/canvas 等后入依赖——SBOM 若存在也已结构性过期。
- **mang-shell 741-lock 无任何 SBOM/NOTICES 义务执行**（COMPLIANCE.md §3 门禁 3「依赖变更重跑 SBOM」对该 workspace 从未触发——gates.md 零 mang-shell 条目佐证）。

### 5.2 应有形状（四源 → 按发布物出具）

| SBOM 文档（按发布物） | 源 | 规模基线（今日实测） |
|---|---|---|
| `mdcore deb`（mdshell-native + dist + pdfium） | 主 Cargo.lock（408 条）+ web/package-lock（ship 面 ~12 项）+ pdfium 二进制（pin chromium/8086 + SHA256，ASSET_CHECK A-1）+ deb Depends 系统库（webkit2gtk/gtk3，LGPL 动态链接声明位） | ~420 组件 |
| `mang-shell` bin | mang-shell/Cargo.lock 全 741（含 31 vendored + 23 git 钉 + 引擎 5 path）+ 15 SVG 资产（GPL 口径，作为 file-level 组件入账）+ xkbcommon/wayland/X11/vulkan 系统库位 | ~760 组件 |
| wasm bundle | 主 lock 的 wasm 子面 + npm ship 12 | ~50 组件 |
| sdk 两件 wheel | uv.lock（python 运行时 0/1 项 + mcp 面） | <10 组件 |

建议形态：CycloneDX 1.6（`bom-ref` 唯一化多版本；`evidence` 字段挂 license 取证来源）或 SPDX-2.3（`hasExtractedLicensingInfos` 承载 AND/OR 复合与 r-efi 三选一）。生成脚本**入产品仓**（compliance/sbom.py），以 `cargo metadata --locked --format-version 1` 的 license 字段为基（离线 registry 快照或 vendors SPDX 词典缓存随脚本提交），npm 侧读 package-lock license 字段、uv 侧 `uv export --frozen`。**四锁同跑 + NOTICES 增量 diff 检测**进 CI（依赖变更 → SBOM diff 非空 → NOTICES 必须同轮更新，兑现 COMPLIANCE.md:58「随依赖变更必须再生成」的机器化）。

### 5.3 NOTICES 记账缺口清单（D 题全答）

| # | 缺口 | 证据 | 严重度 |
|---|---|---|---|
| 1 | **15 件 Zed SVG 零账**（NOTICES/PROVENANCE/ASSET_CHECK 三层皆无） | assets.rs:20-73；ASSET_CHECK 末次 2026-10-14 | 高 |
| 2 | **NOTICES 草案 v1.6 停在搬运前**：2026-10-21 < Zed 搬运 2026-10-29；全文零 Zed 面 | 草案 L1 沿革表 | 高 |
| 3 | mang-shell 741-lock 的 23 个 git 钉依赖零账（10 个 zed fork + 13 wgpu rev） | mang-shell/Cargo.lock source 字段统计 | 中高 |
| 4 | mang-shell workspace 整体无任何 NOTICES/SBOM 面（NOTICES.md:25-29 仅覆盖 third_party 闭包视角） | mang-shell/ 目录树 | 高 |
| 5 | SDK LICENSE 的 Zed attribution 失真（无 Zed 代码却声明 incorporates） | 三 LICENSE md5 相同 | 中（方向：过度声明） |
| 6 | 主仓 16 crate 无 license 元数据（workspace.package 无 license 键） | Cargo.toml:10-11 + 16 个 crate manifest | 中（SBOM 工具化障碍） |
| 7 | 搬运记账本身质量良好：NOTICES.md:8-23 的 10 件清单/LOC/上游 commit `1663af4`（2026-10-29）/唯一补丁 P1 与 PROVENANCE.md:108-115、`scripts/p1_gpui_windows_zoom_toggle.patch` 在档**全部对上** | 逐项复核 | ——（正面结论） |

---

## 6. 闭源商业化阻塞清单（E 题：替换什么、改什么）

### 6.1 必须替换的代码/资产（mang-shell 产物面）

| 项 | 规模 | 建议路径 |
|---|---|---|
| `ui` crate | 29,844 LOC | 最大件：①在 gpui（Apache，可留）之上自研控件层；或②整体迁移 egui/iced（MIT/Apache）——架构级重写 |
| `text` + `rope` + `clock` | 6,746+4,166+338 LOC | rope→**ropey**（MIT OR Apache-2.0，主仓 mdcore 已在用——现成且熟悉）；text::Buffer→自研（buffer.rs 已有 908 行自研前史，PROVENANCE.md:66-68 记「换 Zed text」的动因是锚点+O(log n)，可按同等接口自写或包装 ropey+自研锚点）；clock→338 行平替微不足道 |
| `theme` + `syntax_theme` | 5,957+345 LOC | 自研主题模型（theme 依赖 palette/parking_lot 等全白名单传递面不受影响）；语法主题转 syntect 主题格式（MIT）或自定义 JSON |
| `component`/`icons`(crate)/`menu`/`ui_macros` | 530+360+37+239 LOC | 均为 ui 的伴随件，随 ui 一并消失/重写；ui_macros 用 syn 自写宏 |
| **15 件 Zed SVG** | 9,406 B | 从 **Lucide（ISC）/Phosphor（MIT）原版重新取件**（上游原版许可宽松；注意 Zed 版为改造版，视觉有差，需微调）或付费图标集/自绘 |
| `gpui` Apache 系 22 件 | 182,194 LOC | **可保留**（Apache-2.0 无 copyleft）——NOTICES.md:16「Lead 规则」的记账正是此分层；但需完成 Apache attribution（NOTICES + LICENSE 文件随发行物） |

### 6.2 必须修改的文件/声明

1. 仓根 `LICENSE`（GPL→目标许可；头部 Zed attribution 句随替换完成而删除/改写）。
2. `mang-shell/Cargo.toml:20` license 字段；`mang-shell/Cargo.toml:59-80` 依赖面（拆 GPL path 依赖）。
3. `sdk/python/pyproject.toml:7` + `sdk/mcp/pyproject.toml:7` + 两份 `sdk/*/LICENSE`（GPL 标签与假 attribution 一并清理——SDK 本可即刻转任意许可）。
4. `NOTICES.md`（重写为双段式：vendored Apache 资产 + 自研声明）；`compliance_release/THIRD-PARTY-NOTICES.draft.md`（v1.7 起以四源 SBOM 为基）；`ASSET_CHECK.md`（SVG/替换图标登记）。
5. `COMPLIANCE.md` §1 姿态段与 §2 审计终态（补 mang-shell 面结论或改口径）；`README.md:4`「私有产品仓」与许可声明一致性。
6. `packaging/` 四模板/脚本：装 LICENSE 全文 + THIRD-PARTY-NOTICES + Apache NOTICE 义务（当前打包链**零许可装载**，即便 GPL 路线也缺 GPL 义务件——双重缺口）；`packaging/checklist.md` F4「闭源 EULA 起草」。
7. `[patch.crates-io]` 三条（mang-shell/Cargo.toml:107-110）与 10 个 zed fork git 钉依赖的许可逐项复核转正（scap 等为闭源前的清理项）。
8. 治理文件：gates.md 增补 mang-shell 门禁位（当前 0 条目）；scan.sh 域扩展（mang-shell/src 纳入 + 豁免登记）。
9. 商标面：A5 归属句、定名重检索（trademark_search_2026-10-27.md §4 未竟项——商用前 CNIPA/WIPO 必补，checklist.md:16-19 自declared）。

### 6.3 可即刻闭源的部分（不受 GPL 约束）

引擎全套（mdcore 16 crate + mdshell-native deb + wasm + web dist + sdk 两件）——主仓 0 GPL 实证成立；只要 mang-shell 线剥离或隔离为独立 GPL 开源产品，主仓即可按任意许可商业化。**最小商业路径**：先商业化 mdshell-native（wry/tao 技术栈，产品功能面已完整：BUILD-VERIFY deb 实构建 + GUI 启动验证在档），mang-shell 作为开源 GPL 旁线并行。

---

## 7. 问题清单（P0-P3 + 置信）

| 级 | 编号 | 问题 | 证据 | 置信 |
|---|---|---|---|---|
| P0 | C-1 | **合规门禁体系当前失效**：scan.sh 实跑 exit 1（5 假阳性）≥8 天无人分诊；无 CI 集成无钩子，「每次提交前 PASS 才可合入」（README.md:61）为纯口头纪律 | §3.2 复现 + workflows grep | 实证 |
| P0 | C-2 | **SBOM 声称不可复核且结构性过期**：仓内无 SBOM；367/366 数字与今日 424/371/408/355 全对不上；生成器路径在仓外不存在 | §5.1 | 实证 |
| P0 | C-3 | **NOTICES/记账停在 Zed 搬运前**：草案 v1.6（2026-10-21）早搬运 8 天零 Zed 面；mang-shell 741-lock 零 SBOM 零 NOTICES；15 SVG 三层零账 | §5.3 表 | 实证 |
| P0 | C-4 | **许可声明三方互斥悬空**：LICENSE=GPL-3.0 ↔ COMPLIANCE.md:8 零容忍 ↔ README.md:4 私有产品仓；商业化/闭源决策（issue #7）无可靠底账支撑 | §2.2 | 实证 |
| P1 | C-5 | scan.sh **静默假 PASS** 缺陷（相对路径 + `2>/dev/null \|\| true` 吞错；本地复现两向结果相反）——单向失效的安全门 | §3.3-B | 实证 |
| P1 | C-6 | scan.sh 无豁免机制（README 豁免表纯文档无法消费）+ 产品面缺 mang-shell/sdk/scripts 域——门禁覆盖面与架构脱锚 | §3.3-A/C | 实证 |
| P1 | C-7 | gates.md **零 mang-shell/Zed 搬运门禁条目**：230,756 LOC 许可面变更无任何门禁注册（门禁注册表自身脱锚） | gates.md grep | 实证 |
| P1 | C-8 | 打包链零许可装载：deb/NSIS/AppImage 模板与脚本 grep 零 license/NOTICES——GPL 路线的随附义务与未来任何路线的第三方声明均未接 | packaging 四脚本 | 实证 |
| P1 | C-9 | mang-shell 23 个 git 钉依赖（10 zed fork + 13 wgpu rev）许可未记账未复核 | §4.2 | 高 |
| P2 | C-10 | 主仓 16 crate + workspace.package 无 license 元数据（SBOM/发布工具化障碍） | §4.1 | 实证 |
| P2 | C-11 | SDK 假 Zed attribution（无 Zed 代码而声明 incorporates）；web/package.json 无 license 字段 | §2.1/§4.5 | 实证 |
| P2 | C-12 | 数字漂移生态：SBOM 366↔367↔实况；测试数 267（REUSE.md:56）/361（COMPLIANCE.md:30）/850+（README.md:10）；README:39 引 CONTRACT v1.2 实为 v1.6——结论文档无人维护 | 多文件 | 实证 |
| P2 | C-13 | COMPLIANCE.md §2 审计终态（四路 CLEAN）底稿在私有分析仓，产品仓侧不可复核——「可复核性」为零，只有声明 | COMPLIANCE.md:15,105-106 | 高 |
| P3 | C-14 | scan.sh 小项：head -20 截断、fail 按 词×路径 计数、`--all` 不含 mang-shell tests；`ironcalc_base` 许可需联网复核；分支拓扑（main vs compliance-baseline，COMPLIANCE.md:99-104）在本快照不可见 | §3.3-E/§4.1 | 中 |

---

## 8. 修复路线建议（对齐终报告 T5/T10）

1. **T+0（当日）**：packaging 两行原词改写（§3.4-1）→ scan.sh 回绿；同步登记豁免表（词+理由+日期，即使机制未上先记账）。
2. **T+1 周（合规止血）**：scan.sh 修假 PASS（cd ROOT + 缺路径计 FAIL）+ allowlist 机制（§3.4-2/3）；compliance CI job（scan.sh + 四锁 SBOM diff + NOTICES 增量检测）；NOTICES v1.7 增量：Zed 32 crate 面移植（NOTICES.md 根已有，草案补）+ 15 SVG 登记（ASSET_CHECK 增补节）+ 23 git 钉复核 + sdk 声明清理。
3. **T+2-4 周（底账重建）**：SBOM 生成脚本入仓（§5.2 四源按发布物出四件 CycloneDX/SPDX，随 tag 归档）；COMPLIANCE.md 姿态段重写（三态声明：引擎=待定/闭源可选、mang-shell=GPL 事实、sdk=待定——消除互斥）；gates.md 补 mang-shell 门禁位（构建绿 + SVG/资产登记锁 + PROVENANCE 哈希锁）。
4. **商业化裁决门（issue #7 落地）**：若闭源 → 执行 §6 全清单（优先序：mang-shell 剥离为独立 GPL 开源仓 + 引擎即刻转目标许可 = 最小路径；或全量替换 ui 29.8K = 4-6 周重写路线）；若开源 → 仅需文档调和（LICENSE 维持 + COMPLIANCE 姿态改「GPL 壳+白名单引擎」双态描述 + 打包装 GPL 义务件）。
5. **长期治理**：豁免/资产/NOTICES/SBOM 四本账统一由 CI 比对（账闸合一）；每轮收口双绿定义扩展为「scan.sh + cargo test + SBOM diff=0 或 NOTICES 同步」。

---

## 附 · 证据索引（核心引用速查）

- scan.sh 实跑：exit 1 / hit-paths=5 / terms=20（本审计本地复现，输出存证 /tmp/scan_result.txt——审计环境临时件，不入仓）
- 五假阳性定位：packaging/checklist.md:20、packaging/README.md:44（同词列举行）
- GPL 10 件：NOTICES.md:16-18 ↔ PROVENANCE.md:76-83 ↔ mang-shell/Cargo.toml:59-80 ↔ 逐 crate Cargo.toml license 字段 + wc -l 实测 48,562
- SVG 15 件：mang-shell/src/assets.rs:20,24,28,30,33,37,41,45,49,53,57,61,65,69,73
- SDK 同源 LICENSE：md5 `ecb39498a738729db92fab5ed2ce71dd` ×3（LICENSE/sdk/python/LICENSE/sdk/mcp/LICENSE）
- SBOM 缺位：全树 find 零命中；声称链 README.md:79 → NOTICES 草案:633 → COMPLIANCE.md:78
- 打包零许可装载：packaging/deb/build-deb.sh、deb/control.tpl、appimage/build-appimage.sh、nsis/installer.nsi.tpl grep 零命中
- CI 无合规 job：.github/workflows/ 仅 windows-check.yml/windows-test.yml（grep scan.sh 零命中）
- 上游快照一致性：NOTICES.md:9 = PROVENANCE.md:11 = `1663af412c625417759561c0d15ab5a1668aafa1`（2026-10-29）
- 唯一源码补丁：PROVENANCE.md:108-115（gpui_windows zoom toggle）+ scripts/p1_gpui_windows_zoom_toggle.patch 在档
