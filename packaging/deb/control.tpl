# 注入：{{VERSION}}（构建脚本从 mdcore::VERSION 取值——checklist B2）
# 闭源 EULA = ADR-C1 挂起项（checklist F4）；正式发布前补齐并改写
# 描述段的许可句。（注：本行必须在字段区外——Q3-3 实测：缩进在
# Description 块内的 # 行是合法续行文本，grep -v '^#' 剥不掉，会裸进包。）
# #319（2026-11-07）：Depends 切 mang-shell（wgpu 自绘主线壳）直链集。
# 依据 = release 产物 readelf DT_NEEDED 实测：libxcb.so.1 / libxkbcommon.so.0 /
# libxkbcommon-x11.so.0 / libgcc_s.so.1 / libc.so.6（ldd 面的 libxcb-xkb / libXau /
# libXdmcp 均为传递依赖，Debian 由直接 NEEDED 声明覆盖）。旧 GTK3+WebKit2GTK
# 行是 tao+wry 旧壳（mdshell-native）残留——mang-shell 零 GTK/WebKit 引用。
# 渲染后端（Vulkan / GL 4.3+）与 Wayland 为运行时 dlopen：缺位时应用自带
# 降级弹窗（#270 验收 evidence/xvfb-launch-bare.log 实证）。是否声明
# `libvulkan1 | libgl1` 待 Lead 裁定（最小依赖派 vs 显式派），裁定前不声明。
Package: mdcore
Version: {{VERSION}}
Section: text
Priority: optional
Architecture: amd64
# 维护者字段待定名后随三件套登记（packaging/README 变量表）
Maintainer: mdcore project <noreply@mdcore.invalid>
Depends: libxcb1, libxkbcommon0, libxkbcommon-x11-0, libgcc-s1, libc6 (>= 2.34)
Description: Modular document editor core with native shell
 Document editor (Markdown / DOCX / PDF / XLSX) built on a headless editing
 core (block tree + op log + undo history) with a native wgpu-rendered
 shell (GPU-accelerated self-drawn UI, X11 and Wayland). Opens .md files,
 supports progressive loading for large documents, session restore,
 and per-document identity for external-change reconciliation.
Homepage: https://mdcore.invalid
Installed-Size: {{INSTALLED_SIZE_KB}}
