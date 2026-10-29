# NOTICES

本仓库使用了以下第三方开源组件。逐文件级来源与改动清单见
`third_party/zed/PROVENANCE.md`。

## Zed (https://github.com/zed-industries/zed)

- **使用范围**：`third_party/zed/crates/`（21 个 crate，ui 传递闭包）
- **上游快照**：commit `1663af412c625417759561c0d15ab5a1668aafa1`（2026-10-29）
- **许可**：GPL-3.0-or-later（gpui/ui/theme/component/icons/menu/syntax_theme/ui_macros）
  与 Apache-2.0（gpui_macros/sum_tree/scheduler/bench_metrics/zlog/gpui_util/
  refineable/derive_refineable/http_client/ztracing/collections/
  gpui_shared_string/ztracing_macro）——各 crate 目录内保留上游原版 LICENSE 文件
- **改动**：源码零修改；Cargo.toml 的 workspace 继承展开为显式版本（自动改写，
  规则与逐 crate 明细见 PROVENANCE.md）
- **本项目整体**因此采用 GPL-3.0-or-later（见仓根 LICENSE）

## crates.io 依赖

`third_party/zed/` 的编译按上游 Cargo.toml 引用 crates.io / git 依赖，
完整解析结果以 `third_party/zed/Cargo.lock` 为准（含上游 [patch.crates-io]
钉 rev 的 4 个 git 依赖：async-task / calloop / cpal / khronos-egl）。
