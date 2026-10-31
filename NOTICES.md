# NOTICES

本仓库使用了以下第三方开源组件。逐文件级来源与改动清单见
`third_party/zed/PROVENANCE.md`。

## Zed (https://github.com/zed-industries/zed)

- **使用范围**：`third_party/zed/crates/`（32 个 crate = ui 传递闭包 21 + gpui_platform 窗口层 r39 增补 8 + text 编辑器基座 r42 增补 3；闭包定义与重放见 `scripts/zed_port.py`）
- **上游快照**：commit `1663af412c625417759561c0d15ab5a1668aafa1`（2026-10-29）
- **许可**：GPL-3.0-or-later（gpui/ui/theme/component/icons/menu/syntax_theme/ui_macros
  + r42 新增 text/rope/clock）与 Apache-2.0（gpui_macros/sum_tree/scheduler/bench_metrics/
  zlog/gpui_util/refineable/derive_refineable/http_client/ztracing/collections/
  gpui_shared_string/ztracing_macro + r39 新增 gpui_platform/gpui_linux/gpui_wgpu/
  gpui_macos/gpui_windows/gpui_ios/gpui_web/gpui_apple）——各 crate 目录内保留上游
  原版 LICENSE 文件
- **GPL 部件记账（Lead 规则：用着不阻断；开源时整体 GPL 无冲突；闭源时需替换）**：
  闭源时需替换清单 = GPL-3.0 系 10 件（ui/theme/component/icons/menu/syntax_theme/
  ui_macros/text/rope/clock）。PROVENANCE.md r42 节有逐件 LOC 与决策记录。
- **改动**：源码零修改；Cargo.toml 的 workspace 继承展开为显式版本（自动改写，
  规则与逐 crate 明细见 PROVENANCE.md）；唯一源码补丁 = P1（gpui_windows zoom
  toggle 化，登记于 PROVENANCE「源码补丁登记」，重放文件
  `scripts/p1_gpui_windows_zoom_toggle.patch`）
- **本项目整体**因此采用 GPL-3.0-or-later（见仓根 LICENSE）

## crates.io 依赖

`third_party/zed/` 的编译按上游 Cargo.toml 引用 crates.io / git 依赖，
完整解析结果以 `third_party/zed/Cargo.lock` 为准（含上游 [patch.crates-io]
钉 rev 的 4 个 git 依赖：async-task / calloop / cpal / khronos-egl）。
