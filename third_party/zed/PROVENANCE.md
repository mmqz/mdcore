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

## Cargo.toml 改写规则（全部由 `scripts/zed_port.py` 自动完成）

1. `[package]`：`version/edition/publish/rust-version` 的 `.workspace = true` → 显式值（`0.1.0` / `2024` / `false` / `1.86`）；元数据继承行（description/authors 等）删除
2. `[lints] workspace = true` 段整体删除（第三方代码不受 mdcore 项目 lint 门；Zed workspace lints 与本仓 clippy 基线口径不同）
3. 内部依赖（闭包内 21 crate）→ `{ path = "../<crate>", ... }`（features 保留）
4. 外部依赖 → 从 Zed 根 `workspace.dependencies` 展开为显式定义（版本/features/default-features/optional 与上游解析结果一致；crate 处附加属性如 `optional = true` 与根表定义合并，features 拼接去重）
5. workspace 根（third_party/zed/Cargo.toml）：members + dev profile（上游 dev/build-override/proc-macro opt-level=3）+ release profile + `[patch.crates-io]` 中处于本闭包依赖图内的 4 条（async-task/calloop/cpal/khronos-egl——上游为修上游 bug 的钉 rev，照抄保证行为一致；scap/wasm_thread 等由外部依赖的 git 引用自带）

**源码（.rs/build.rs）零修改**——与上游同 commit diff 仅在 Cargo.toml 与目录布局，上游升级可重放。

## 决策依据（#63 决策规则对表）

| 判据 | 情况 A 要求 | 实测 | 判定 |
|---|---|---|---|
| 内部 crate 数 | ≤ 8 | **21**（gpui 系基础设施 14 个 + GPL 组件系 7 个） | 字面超标 |
| 单 crate workspace 继承改动 | ≤ 30 行 | 最大 gpui 48 处（脚本自动展开，人工改动 0） | 实质达标 |
| 能编译通过 | — | **21/21 一次通过**（`cargo build` 全绿，见下） | 达标 |
| 牵连 settings/workspace/project 重型模块 | 无则非情况 B | 生产代码零耦合（settings 仅 dev-deps，`theme::theme_settings()` 是 trait provider 模式非 settings crate） | 无 |

**结论：情况 A 变体**——crate 数超字面判据（21 > 8），但「依赖链收敛 + 零源码修改 + 一次编译通过」三核心判据全部满足，且数量超标主因是 gpui 基础设施（Apache 系 120K LOC，非 GPL 传染面）。GPL 系实际仅 7 crate / 37K LOC。按 #63「完整可直接抄 > 改造后融入 > 自写」优先序取最高档。

**为何不用 crates.io gpui 0.2.2**：crates.io 版停留在 2025-10-22 快照，Zed 仓内 gpui 同号 0.2.2 但持续开发至今（2026-10-29 当日仍有提交）——ui/theme 针对仓内版编写，对一年前的发布版编译必然大面积漂移。物理搬运锁定自洽快照是唯一可行路径。

## 编译证据（2026-10-29 沙箱实测）

```
$ cd third_party/zed && cargo build          # 独立 workspace
    Finished `dev` profile target(s) in 47.07s   # 21/21 crates 零 error
$ cd third_party/zed && cargo build -p gpui  # 最重一环单独复验
    Finished `dev` profile target(s) in 1m 55s
```

系统依赖（Linux）：libX11/libxcb/libxkbcommon/libwayland-client/libvulkan 运行时库（链接面）+ Vulkan 走 loader 动态加载。工具链：rustc 1.99.0（与主仓 rust-toolchain.toml 同源）。

## 与主仓的关系

- `third_party/zed/` 是**独立 cargo workspace**（自带 [workspace] 根），不在主仓 members 显式列表内——主仓 `cargo build/test/clippy/fmt` 门禁均不触达
- compliance/scan.sh 的 PRODUCT_PATHS 不含 third_party——上游代码词条不进词法门
- 主仓含本目录后整体许可面变为 **GPL-3.0-or-later**（见仓根 LICENSE 与 NOTICES）

## 上游升级流程

1. `git -C <zed-clone> fetch && git checkout <new-rev>`
2. 修改 `scripts/zed_port.py` 顶部 `ZED` 指向新 clone（或保持路径约定）
3. `python3 scripts/zed_port.py --clean`（在实验区重建后比对）
4. `cd third_party/zed && cargo build` 全绿后，更新本文件「来源快照」的 commit 与日期
