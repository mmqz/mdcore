# 本地测试运行证据（2026-11-04，HEAD=96596d7）

审计纪律：全部命令以 `--locked` 运行（保证 Cargo.lock 零改动）；CARGO_TARGET_DIR 指向仓库外；每批次后 `git status --porcelain` 复核仓库干净。

## 工具链

- 仓库 `rust-toolchain.toml` 钉住 **1.99.0 + wasm32-unknown-unknown + rustfmt + clippy**（注释记载：rustfmt 行为随工具链漂移曾致 459 diff 全仓门禁红事故——钉住后跨环境可复现）。
- 本地按钉住版本以 1.99.x（1.99.1 shim 实际生效）运行。
- MSRV 实测注记（审计发现）：依赖链要求 rustc ≥1.88（calamine 0.36.1 / zip 8.6.0 / rust_xlsxwriter 0.99.1 / statrs 0.19.1 / zopfli / lopdf 0.45 / aes / encoding_rs）；yrs 0.28.0 使用 match `if let` guard（1.93 仍 E0658，1.99 稳定）。**全仓 Cargo.toml 无 rust-version 声明**——建议补声明，否则旧工具链用户会在依赖解析期得到晦涩报错（本轮 1.85/1.89/1.93 实测均无法编译全仓）。

## 测试结果汇总

| 批次 | 命令 | 结果 |
|---|---|---|
| mdcore | `cargo test -p mdcore --locked` | 240 通过 / 21 套件 / 0 失败 |
| office 链 | `cargo test -p mdc-office-io -p mdc-office -p mdc-formula --locked` | 457 通过 / 40 套件 / 0 失败（含环境门 ignored 若干） |
| pdf | `cargo test -p mdc-pdf --features engine-lopdf --locked` | 182 通过 / 18 套件 / 0 失败（1 ignored=pdfium 环境门） |
| 其余 | `cargo test -p mdc-canvas -p mdc-evidence -p mdc-collab -p mdc-workbench -p mdc-office -p mdshell -p mdc-mdbridge -p mdc-relay -p mdc-settings --locked` | 362 通过 / 54 套件 / 0 失败 |
| **合计（本地可跑面）** | | **1,241 通过 / 133 套件 / 0 失败** |

未运行与原因：mdcore-wasm / pdf-wasm（需 wasm32 目标运行时环境）、shell-native（GTK sysroot 系统库缺失）、mang-shell（GPUI Linux 系统依赖 + 392 文件编译量超出沙箱预算）——与 CI 现状互证：CI 也仅在 Windows 跑其中 9 个 crate。

## fmt 门禁实测

`cargo fmt --all -- --check` → **FAIL，977 行漂移**（`^[+-]` 口径），集中于 crates/canvas（render.rs 多处）与 pdf 侧；与开放 PR #216「1896 行归口清零」（其口径含上下文行）方向一致、数量级吻合。结论：**fmt 门禁在 main 事实失效，且两份 CI workflow 均无 fmt 步骤**（PR #216 自述「CI 未拦原因：均不含 fmt 步骤」）。

## 结论

1. README「cargo test --workspace 850+ 用例（Linux 全量基线）」为**本地口径**且数字过时（静态 #[test] 计数 workspace ≈1220 + mang-shell ≈563；本轮本地实跑 1,241）——CI 无 Linux job，该基线不可在仓内复现。
2. 本地全绿佐证「9 个引擎 crate 内部模型自洽」；但结合 13 号笔记：用户可见正确性（外部软件读回、渲染像素、wasm 运行时、真实文件互操作）证据全部住在环境门/#[ignore]/本地人工流程之后。
