# Linux 桌面入口（deb / AppImage 共用模板；R24-c）
# 注入：mang {{APP_EXEC}} {{BUNDLE_ICON}}
# 方言：Desktop Entry Specification 1.5（GNOME/KDE/Cinnamon 通用）
[Desktop Entry]
Type=Application
Version=1.5
Name=mang
GenericName=Markdown Editor
Comment=Modular Markdown editing core with native shell
# 可执行名用自研命名域（TRADEMARK_RULES §1）；不等待定名。
Exec={{APP_EXEC}} %F
TryExec={{APP_EXEC}}
Icon={{BUNDLE_ICON}}
Terminal=false
Categories=TextEditor;Utility;
# .md 关联 + mdcore:// 自定义协议注册（CONTRACT §2：mdcore://app）
MimeType=text/markdown;x-scheme-handler/mdcore;
# 单实例语义（R24-b 一窗一档；宿主把新文件参数转交既有实例）
StartupWMClass={{APP_EXEC}}
SingleMainWindow=true
# #321（2026-11-07）：仅保留真实实现的动作。open-settings =
# `--settings` CLI 合同（ShellApp::open_settings 同链）；new-window
# 已移除——单实例一窗一档是 R24-b 既定设计（真开二窗与 RPC 单端点/
# 退出语义相悖；新档由宿主转交既有实例，非二窗）。
Actions=open-settings;

[Desktop Action open-settings]
Name=Settings
Exec={{APP_EXEC}} --settings
