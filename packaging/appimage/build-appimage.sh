#!/usr/bin/env bash
# AppImage 构建（R24-c；实体验证留 R24-d——本脚本在开发环境执行）
#
# 输入：宿主壳 release 产物（shell-native，tao+wry+GTK3 链接）
# 输出：mdcore-<VERSION>-x86_64.AppImage
#
# 用法：packaging/appimage/build-appimage.sh <path-to-shell-binary>
# 前置：linuxdeploy + linuxdeploy-gtk-plugin + appimagetool（CI 缓存或
#       下载到 build/_tools/；版本锁定见下 *_VER）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${1:?usage: build-appimage.sh <path-to-shell-binary>}"
APPIMG_TOOL_VER="continuous"   # appimagetool 无版本号发布轨，按 SHA256 锁
LINUXDEPLOY_VER="1-alpha-20250106"  # R24-d 实测后锁具体版

# 版本单源：从产物内嵌版本读取（不信任手传参数——checklist B2）
# shell 二进制 --version 输出 = mdcore::VERSION（CONTRACT §1 版本面）
VERSION="$("$BIN" --version | awk '{print $2}')"
echo "[appimage] version(single-source)=${VERSION}"

APPDIR="$ROOT/build/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps"

install -m 0755 "$BIN" "$APPDIR/usr/bin/mdcore"

# 桌面入口 + 图标（{{}} 占位符在此注入；APP_NAME 待定名后启用，
# 当前以 mdcore 工作名出 CI 内测包——checklist A4 未勾选不出正式包）
sed -e 's/mang/mdcore/g' \
    -e 's/{{APP_EXEC}}/mdcore/g' \
    -e 's/{{BUNDLE_ICON}}/mdcore/g' \
    "$ROOT/packaging/shared/app.desktop.tpl" \
    > "$APPDIR/usr/share/applications/dev.mdcore.editor.desktop"
install -m 0644 "$ROOT/packaging/appimage/icon-placeholder.png" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps/mdcore.png" 2>/dev/null \
    || echo "[appimage] WARN: 图标资产未登记（ASSET_CHECK），跳过——正式发布前必须补齐（checklist C5）"

install -m 0755 "$ROOT/packaging/appimage/AppRun" "$APPDIR/AppRun"

# linuxdeploy 收敛运行时 + appimagetool 出包（--appdir 面向 GTK 插件）
"$ROOT/build/_tools/linuxdeploy-x86_64.AppImage" \
  --appdir "$APPDIR" \
  --desktop-file "$APPDIR/usr/share/applications/dev.mdcore.editor.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/mdcore.png" \
  --plugin gtk \
  --output appimage

mv -f "mdcore-x86_64.AppImage" "$ROOT/build/mdcore-${VERSION}-x86_64.AppImage"
echo "[appimage] OK: build/mdcore-${VERSION}-x86_64.AppImage"
