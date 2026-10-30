# Zed 源码搬运清单（PROVENANCE）

> 对应 issue #63（GPUI 主线 UI）· 调研结论与决策详见 issue #64
> 搬运器：`scripts/zed_port.py`（可复现：从上游 Zed 仓重建本目录）

## 来源快照

| 项 | 值 |
|---|---|
| 上游仓库 | https://github.com/zed-industries/zed |
| Zed commit | `1663af412c625417759561c0d15ab5a1668aafa1` |
| 搬运日期 | 2026-10-29 |
| 搬运方式 | 物理复制（源码零修改）+ Cargo.toml workspace 继承展开 |
| 决策 | 情况 A 变体（直接抄全闭包，见「决策依据」） |

## 搬运清单（ui 传递闭包 · 21 crate）

| 来源路径 | 本地路径 | 许可 | LOC | 改动说明 |
|---|---|---|---:|---|
| crates/gpui/ | third_party/zed/crates/gpui/ | Apache-2.0 | 102,096 | 仅 Cargo.toml：workspace 继承展开为显式版本 |
| crates/ui/ | third_party/zed/crates/ui/ | GPL-3.0-or-later | 29,844 | 仅 Cargo.toml 同上；dev-deps settings/theme_settings（闭包外）删除 |
| crates/theme/ | third_party/zed/crates/theme/ | GPL-3.0-or-later | 5,957 | 仅 Cargo.toml 同上 |
| crates/gpui_macros/ | third_party/zed/crates/gpui_macros/ | Apache-2.0 | 3,631 | 仅 Cargo.toml 同上 |
| crates/sum_tree/ | third_party/zed/crates/sum_tree/ | Apache-2.0 | 3,327 | 仅 Cargo.toml 同上 |
| crates/scheduler/ | third_party/zed/crates/scheduler/ | Apache-2.0 | 3,094 | 仅 Cargo.toml 同上 |
| crates/bench_metrics/ | third_party/zed/crates/bench_metrics/ | Apache-2.0 | 2,234 | 仅 Cargo.toml 同上 |
| crates/zlog/ | third_party/zed/crates/zlog/ | Apache-2.0 | 1,710 | 仅 Cargo.toml 同上 |
| crates/gpui_util/ | third_party/zed/crates/gpui_util/ | Apache-2.0 | 834 | 仅 Cargo.toml 同上 |
| crates/refineable/ | third_party/zed/crates/refineable/ | Apache-2.0 | 680 | 仅 Cargo.toml 同上 |
| crates/http_client/ | third_party/zed/crates/http_client/ | Apache-2.0 | 669 | 仅 Cargo.toml 同上 |
| crates/ztracing/ | third_party/zed/crates/ztracing/ | Apache-2.0 | 572 | 仅 Cargo.toml 同上 |
| crates/refineable/derive_refineable/ | third_party/zed/crates/derive_refineable/ | Apache-2.0 | 548 | 仅 Cargo.toml 同上（内嵌子 crate 提为平级） |
| crates/component/ | third_party/zed/crates/component/ | GPL-3.0-or-later | 530 | 仅 Cargo.toml 同上 |
| crates/collections/ | third_party/zed/crates/collections/ | Apache-2.0 | 418 | 仅 Cargo.toml 同上 |
| crates/icons/ | third_party/zed/crates/icons/ | GPL-3.0-or-later | 360 | 仅 Cargo.toml 同上 |
| crates/syntax_theme/ | third_party/zed/crates/syntax_theme/ | GPL-3.0-or-later | 345 | 仅 Cargo.toml 同上 |
| crates/ui_macros/ | third_party/zed/crates/ui_macros/ | GPL-3.0-or-later | 239 | 仅 Cargo.toml 同上 |
| crates/gpui_shared_string/ | third_party/zed/crates/gpui_shared_string/ | Apache-2.0 | 210 | 仅 Cargo.toml 同上（根目录平铺 lib 文件保持原位） |
| crates/menu/ | third_party/zed/crates/menu/ | GPL-3.0-or-later | 37 | 仅 Cargo.toml 同上 |
| crates/ztracing_macro/ | third_party/zed/crates/ztracing_macro/ | Apache-2.0 | 7 | 仅 Cargo.toml 同上 |

合计 **157,342 LOC**（GPL 系 37,312 / Apache 系 120,030）。
每个 crate 目录内保留上游原版 `LICENSE-*` 文件；`tests/`、`examples/`、`benches/`、`docs/` 目录未搬运（构建面外，dev-deps 闭包外依赖已从 Cargo.toml 删除并留 `# [port]` 注释）。

## 增补搬运（r39 · gpui_platform 窗口层闭包 · 8 crate）

> 背景：上游已把 OS 后端从 gpui 拆出为独立 crate 链（gpui_platform → gpui_linux/gpui_wgpu/…），
> ui 闭包天然不含它们——仅有组件库而开不了真实窗口。增补闭包根 = `gpui_platform`（与 ui 并集）。

| 来源路径 | 本地路径 | 许可 | LOC | 改动说明 |
|---|---|---|---:|---|
| crates/gpui_platform/ | third_party/zed/crates/gpui_platform/ | Apache-2.0 | 259 | 仅 Cargo.toml：按下方改写规则（各 OS 门控的后端选择器门面） |
| crates/gpui_linux/ | third_party/zed/crates/gpui_linux/ | Apache-2.0 | 17,357 | 仅 Cargo.toml 同上（wayland+x11 双后端；xim 为上游 git 钉 rev 依赖照抄） |
| crates/gpui_wgpu/ | third_party/zed/crates/gpui_wgpu/ | Apache-2.0 | 6,213 | 仅 Cargo.toml 同上（wgpu 渲染器 + cosmic-text/swash 文本面 + font-kit） |
| crates/gpui_macos/ | third_party/zed/crates/gpui_macos/ | Apache-2.0 | 9,802 | 仅 Cargo.toml 同上（Linux 主机上 cfg 门控不参与编译，仅为闭包完整性与后续跨平台验证面） |
| crates/gpui_windows/ | third_party/zed/crates/gpui_windows/ | Apache-2.0 | 13,450 | 仅 Cargo.toml 同上（同上 cfg 门控） |
| crates/gpui_ios/ | third_party/zed/crates/gpui_ios/ | Apache-2.0 | 3,583 | 仅 Cargo.toml 同上 |
| crates/gpui_web/ | third_party/zed/crates/gpui_web/ | Apache-2.0 | 7,017 | 仅 Cargo.toml 同上（wasm32 面） |
| crates/gpui_apple/ | third_party/zed/crates/gpui_apple/ | Apache-2.0 | 4,483 | 仅 Cargo.toml 同上（gpui_macos/gpui_ios 共享底座，build.rs 随行搬运） |

增补合计 **62,164 LOC**（全部 Apache-2.0，零 GPL 增量）。

## 增补搬运（r42 · text 编辑器基座闭包 · 3 crate）

> 背景：#68 阶段 A——Lead 定调「抄 Zed 的设计思路，自己写干净的实现」+
> 「已有的独立 crate：text / rope（这两个是干净的）」；自研 buffer（908 行
> Vec<String>，O(n) 编辑无锚点）换 text::Buffer（rope + 锚点，O(log n)）。
> 闭包根 = `text`（与 ui/gpui_platform 并集）；text 闭包新增仅 3 crate
> （collections/gpui_util/sum_tree/ztracing/ztracing_macro 已在闭包）。
> 同批根固定：`zlog`/`bench_metrics` 升为闭包根（前者 ztracing 的 optional
> prod 依赖 + mang-shell 日志面直接引用；后者 gpui bench-support feature
> 面）——optional 跳过规则会把它们甩出闭包致 [features] `dep:` 引用悬空解析红。

| 来源路径 | 本地路径 | 许可 | LOC | 改动说明 |
|---|---|---|---:|---|
| crates/text/ | third_party/zed/crates/text/ | GPL-3.0-or-later | 6,746 | 仅 Cargo.toml 同上；feature-gated optional `util`（test-support 专用）不搬，其 [features] 引用由改写规则 10 剥离 |
| crates/rope/ | third_party/zed/crates/rope/ | GPL-3.0-or-later | 4,166 | 仅 Cargo.toml 同上；dev `util`/`zlog` 删除 |
| crates/clock/ | third_party/zed/crates/clock/ | GPL-3.0-or-later | 338 | 仅 Cargo.toml 同上 |

增补合计 **11,250 LOC**（全部 GPL-3.0-or-later；GPL 记账按 Lead 规则——
开源时整体 GPL 无冲突，闭源时需替换：ui/theme/component/icons/menu/
syntax_theme/ui_macros 之外新增 text/rope/clock 三件）。
当前总闭包 **32 crate / 230,756 LOC**（GPL 系 48,562 / Apache 系 182,194）。
编译证据：`cargo check`（独立 workspace）32/32 零 error（2026-10-30，1m52s）。

## Cargo.toml 改写规则（全部由 `scripts/zed_port.py` 自动完成）

1. `[package]`：`version/edition/publish/rust-version` 的 `.workspace = true` → 显式值（`0.1.0` / `2024` / `false` / `1.86`）；元数据继承行（description/authors 等）删除
2. `[lints] workspace = true` 段整体删除（第三方代码不受 mdcore 项目 lint 门；Zed workspace lints 与本仓 clippy 基线口径不同）
3. 内部依赖（闭包内 21 crate）→ `{ path = "../<crate>", ... }`（features 保留）
4. 外部依赖 → 从 Zed 根 `workspace.dependencies` 展开为显式定义（版本/features/default-features/optional 与上游解析结果一致；crate 处附加属性如 `optional = true` 与根表定义合并，features 拼接去重）
5. workspace 根（third_party/zed/Cargo.toml）：members + dev profile（上游 dev/build-override/proc-macro opt-level=3）+ release profile + `[patch.crates-io]` 中处于本闭包依赖图内的 4 条（async-task/calloop/cpal/khronos-egl——上游为修上游 bug 的钉 rev，照抄保证行为一致；scap/wasm_thread 等由外部依赖的 git 引用自带）
6. （r39）`[[bench]]`/`[[example]]`/`[[test]]` target 段整体删除——对应目录本就不在搬运面，且无显式 path 的 target 声明在 manifest 解析期即报错（gpui_platform/gpui_wgpu 实测）；带显式 path 的（gpui 24 个 example）一并删除保持口径一致
7. （r39）依赖子段形式 `[<section>.<name>]`（body 为 `workspace = true` + 可选字段，如 gpui_windows 的 scap）→ 转写为父段内 inline 条目
8. （r39）闭包内 path 依赖保留 Zed 根表附加字段（如 `gpui = { path, default-features = false }`）——此前根表字段被丢弃导致依赖静默吃默认 features，与上游语义不一致
9. （r39）Zed 根表子段形式 `[workspace.dependencies.windows]`（TOML 子表而非 inline）解析并入展开源——此前该形态全部漏解析（windows 依赖被注释）
10. （r42）闭包计算跳过含 `optional` 的依赖行——feature-gated 依赖（text 的 `dep:util` 仅 test-support 激活）不搬则不雪球（util → path/util_macros/async_zip 等）；若未来激活相应 feature，Cargo 显式报缺（可发现非静默）。安全方向：误跳导致解析期/编译期红，不导致静默行为漂移
11. （r42）被删 optional prod 依赖若被 [features] 引用（`dep:X` / `"X/..."` / `"X"` 三形式），同步从 feature 列表剥离（text 的 `test-support = ["rand"]`）——否则 Cargo 解析期对悬空 dep: 引用报错

**源码（.rs/build.rs）零修改，唯一例外见下方「源码补丁登记」**——与上游同 commit diff 仅在 Cargo.toml、目录布局与已登记补丁，上游升级可重放。

## 源码补丁登记（.rs 修改 · 逐条列出）

> 「源码零修改」口径的例外清单。每条须含：动机（上游平台语义偏差）、
> 对齐依据（其余后端同 trait 方法的既有行为）、重放指引。除下列条目外
> 源码与上游同 commit 零 diff；上游修复对应问题后删除条目恢复原样。

### P1 · gpui_windows `zoom()` toggle 化（2026-10-30，#68）

- **位置**：`crates/gpui_windows/src/window.rs` — `impl PlatformWindow for WindowsWindow` 的 `fn zoom`（~L947）
- **动机**：#68 Lead Windows 11 真机报告「点最大化 → 最大化；再点不还原」。上游无条件 `ShowWindowAsync(hwnd, SW_MAXIMIZE)`，对已最大化窗口是 no-op。
- **对齐依据**：`PlatformWindow::zoom`（crates/gpui/src/platform.rs:1304）在其余三后端均为 toggle 语义——macOS `NSWindow zoom:`（gpui_macos/window.rs:1924）、X11 `_NET_WM_STATE` Toggle hint（gpui_linux/x11/window.rs:1678）、Wayland set/unset_maximized（gpui_linux/wayland/window.rs:1942）。属后端间语义偏差修复，非功能新增。
- **内容**：可见时先查 `IsZoomed`：已最大化 → `SW_RESTORE`；否则 `SW_MAXIMIZE`。两符号均已在本文件既有使用（`is_maximized` ~L202 / `activate()` ~L826），零新导入。
- **重放指引**：上游升级时若 zed 已修为 toggle（关注 zed-industries/zed gpui_windows `fn zoom`），删除本条恢复上游原样；否则按动机重放 4 行分支。
- **关联修复**：mang-shell 侧无改动——`Window::zoom_window()` 调用面不变（macOS/X11/Wayland 本就 toggle，Windows 补齐后四端语义一致）。

## 决策依据（#63 决策规则对表）

| 判据 | 情况 A 要求 | 实测 | 判定 |
|---|---|---|---|
| 内部 crate 数 | ≤ 8 | **21**（gpui 系基础设施 14 个 + GPL 组件系 7 个） | 字面超标 |
| 单 crate workspace 继承改动 | ≤ 30 行 | 最大 gpui 48 处（脚本自动展开，人工改动 0） | 实质达标 |
| 能编译通过 | — | **21/21 一次通过**（`cargo build` 全绿，见下） | 达标 |
| 牵连 settings/workspace/project 重型模块 | 无则非情况 B | 生产代码零耦合（settings 仅 dev-deps，`theme::theme_settings()` 是 trait provider 模式非 settings crate） | 无 |

**结论：情况 A 变体**——crate 数超字面判据（21 > 8），但「依赖链收敛 + 零源码修改 + 一次编译通过」三核心判据全部满足，且数量超标主因是 gpui 基础设施（Apache 系 120K LOC，非 GPL 传染面）。GPL 系实际仅 7 crate / 37K LOC。按 #63「完整可直接抄 > 改造后融入 > 自写」优先序取最高档。

**为何不用 crates.io gpui 0.2.2**：crates.io 版停留在 2025-10-22 快照，Zed 仓内 gpui 同号 0.2.2 但持续开发至今（2026-10-29 当日仍有提交）——ui/theme 针对仓内版编写，对一年前的发布版编译必然大面积漂移。物理搬运锁定自洽快照是唯一可行路径。

**为何 r39 增补平台层**：上游 2025 下半年把 OS 后端（窗口/事件循环/渲染器/文本系统）从 gpui crate 拆出为 gpui_platform → gpui_linux/gpui_wgpu/… 独立 crate 链。ui 闭包（PR #65）因此只有组件库无窗口能力——`Application::new()` 在上游现属 `gpui_platform::application()`。mang-shell 外壳（#63 Lead 布局要求：自绘标题栏/图标条/三栏/状态栏）需要真实窗口，必须补搬平台层。闭包计算自动收敛：gpui_platform 根新增 8 crate（含非 Linux 后端，保持全闭包口径与后续跨平台验证面），零源码修改口径不变。

## 编译证据

### 2026-10-29（PR #65 · ui 闭包 21 crate）

```
$ cd third_party/zed && cargo build          # 独立 workspace
    Finished `dev` profile target(s) in 47.07s   # 21/21 crates 零 error
$ cd third_party/zed && cargo build -p gpui  # 最重一环单独复验
    Finished `dev` profile target(s) in 1m 55s
```

### 2026-10-30（r39 · 增补 gpui_platform 闭包后 29 crate）

```
$ cd third_party/zed && cargo check --workspace
    Finished `dev` profile target(s) in 1m 56s   # 29/29 crates 零 error
$ cd third_party/zed && cargo check -p gpui_platform --features wayland,x11,font-kit
    Finished `dev` profile target(s) in 34.02s   # Linux 真实后端全量编译
                                                   # （x11rb/xkbcommon/wayland 栈/accesskit/
                                                   #  gpui_wgpu 渲染器 wgpu+cosmic-text+font-kit）
                                                   # 仅 gpui_linux 2 条 check-cfg 噪声警告
                                                   # （上游 cfg(nightly_coverage) 词条，非错误）
```

系统依赖（Linux）：libX11/libxcb/libxkbcommon/libxkbcommon-x11/libwayland-client/libvulkan 运行时库 + fontconfig/freetype 开发面；Vulkan 走 loader 动态加载（wgpu/ash 自带头文件）。工具链：rustc 1.99.0（与主仓同源）。沙箱无 root 环境 libxkbcommon 开发面缺失时，可用 deb 解包用户态 sysroot + `PKG_CONFIG_PATH`/`RUSTFLAGS="-L …"` 过链接门（不影响仓内产物）。

## 与主仓的关系

- `third_party/zed/` 是**独立 cargo workspace**（自带 [workspace] 根），不在主仓 members 显式列表内——主仓 `cargo build/test/clippy/fmt` 门禁均不触达
- compliance/scan.sh 的 PRODUCT_PATHS 不含 third_party——上游代码词条不进词法门
- 主仓含本目录后整体许可面变为 **GPL-3.0-or-later**（见仓根 LICENSE 与 NOTICES）

## 上游升级流程

1. `git -C <zed-clone> fetch && git checkout <new-rev>`
2. 修改 `scripts/zed_port.py` 顶部 `ZED` 指向新 clone（或保持路径约定）
3. `python3 scripts/zed_port.py --clean`（在实验区重建后比对）
4. `cd third_party/zed && cargo build` 全绿后，更新本文件「来源快照」的 commit 与日期
