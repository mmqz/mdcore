# 注入：{{VERSION}}（构建脚本从 mdcore::VERSION 取值——checklist B2）
# 闭源 EULA = ADR-C1 挂起项（checklist F4）；正式发布前补齐并改写
# 描述段的许可句。（注：本行必须在字段区外——Q3-3 实测：缩进在
# Description 块内的 # 行是合法续行文本，grep -v '^#' 剥不掉，会裸进包。）
Package: mdcore
Version: {{VERSION}}
Section: text
Priority: optional
Architecture: amd64
# 维护者字段待定名后随三件套登记（packaging/README 变量表）
Maintainer: mdcore project <noreply@mdcore.invalid>
# Depends 依据（#330 修复，2026-11-07 验收实测锚定）：
#   ELF NEEDED（readelf -d /usr/bin/mdcore）：libxcb.so.1 / libxkbcommon.so.0 /
#     libxkbcommon-x11.so.0（+libc/libm/libgcc_s——libc6 由基础面承载）
#   运行期 dlopen（缺失即对应功能坏）：
#     libvulkan.so.1（wgpu 渲染必需——GPUI/blade 面，缺=开窗失败）
#     libwayland-client.so.0（Wayland 会话；wayland-backend dlopen 态）
#   已删除：libgtk-3-0 / libwebkit2gtk-4.1-0（旧壳 mdshell-native 遗留——
#     GPUI 壳零 GTK/WebKit 链接；实测 apt --simulate 旧声明拖 43 包无关
#     依赖且实际所需 3 库 0 命中，干净系统装完即 cannot open shared
#     object file。证据链见 packaging/ACCEPTANCE-270-LINUX.md §1① 与 #330）
Depends: libc6 (>= 2.34), libxcb1, libxkbcommon0, libxkbcommon-x11-0, libvulkan1, libwayland-client0
Description: Modular Markdown editing core with native shell
 Markdown editor built on a headless editing core (block tree + op log +
 undo history) with a native shell (tao+wry+GTK3). Opens .md files,
 supports progressive loading for large documents, session restore,
 and per-document identity for external-change reconciliation.
Homepage: https://mdcore.invalid
Installed-Size: {{INSTALLED_SIZE_KB}}
