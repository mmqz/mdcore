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
Depends: libgtk-3-0 (>= 3.24), libwebkit2gtk-4.1-0, libc6 (>= 2.34)
Description: Modular Markdown editing core with native shell
 Markdown editor built on a headless editing core (block tree + op log +
 undo history) with a native shell (tao+wry+GTK3). Opens .md files,
 supports progressive loading for large documents, session restore,
 and per-document identity for external-change reconciliation.
Homepage: https://mdcore.invalid
Installed-Size: {{INSTALLED_SIZE_KB}}
