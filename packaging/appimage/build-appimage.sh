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
# Q3-3 实测钉（2026-10-28）：linuxdeploy continuous = 1-alpha git 07333c6
#（build 369, 2026-10-01）；旧钉 1-alpha-20250106 的 release 资产已 404。
LINUXDEPLOY_VER="continuous"

# 版本单源：从产物内嵌版本读取（不信任手传参数——checklist B2）
# shell 二进制 --version 输出 = mdcore::VERSION（CONTRACT §1 版本面）
VERSION="$("$BIN" --version | awk '{print $2}')"
echo "[appimage] version(single-source)=${VERSION}"

APPDIR="$ROOT/build/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps"

install -m 0755 "$BIN" "$APPDIR/usr/bin/mdcore"

# #320（2026-11-07）：pdfium 引擎库捆绑（与 Windows NSIS 的 pdfium.dll
# 傍 exe 对称；与 deb 同取件位/同定位序层——见 build-deb.sh 注释）。
# 安装位 = $APPDIR/usr/bin/libpdfium.so：AppRun.wrapped exec 的是
# usr/bin/mdcore，定位序第 2 层「exe 同目录」即中——免 AppRun 改动。
PDFIUM_LIB="${PDFIUM_LIB:-$ROOT/bin/lib/libpdfium.so}"
if [ ! -f "$PDFIUM_LIB" ]; then
  echo "[appimage] FATAL: bin/lib/libpdfium.so 不存在（PDF 引擎库，装包必需）" >&2
  echo "  取件命令与 SHA256 钉扎见 packaging/deb/build-deb.sh 同段（chromium/8086 linux-x64）" >&2
  exit 1
fi
install -m 0755 "$PDFIUM_LIB" "$APPDIR/usr/bin/libpdfium.so"

# s5a-env 工作台 bundle（Q3-3 实测缺陷修复——原脚本完全不装 dist，壳的
# 三候选路径（--dist / MDCORE_DIST / /usr/share/mdcore/dist / ./web/dist）
# 在 AppImage 布局下全落空，启动即 exit 2；镜像 deb 布局 + AppRun 导出
# MDCORE_DIST 指向 AppDir 内路径）。
DIST_SRC="$ROOT/web/dist"
if [ ! -f "$DIST_SRC/index.html" ]; then
  echo "[appimage] FATAL: web/dist/index.html 不存在（先构建 wasm 与 web dist）" >&2
  exit 1
fi
mkdir -p "$APPDIR/usr/share/mdcore"
cp -a "$DIST_SRC" "$APPDIR/usr/share/mdcore/dist"

# 桌面入口 + 图标（{{}} 占位符在此注入；APP_NAME 待定名后启用，
# 当前以 mdcore 工作名出 CI 内测包——checklist A4 未勾选不出正式包）
sed -e 's/mang/mdcore/g' \
    -e 's/{{APP_EXEC}}/mdcore/g' \
    -e 's/{{BUNDLE_ICON}}/mdcore/g' \
    "$ROOT/packaging/shared/app.desktop.tpl" \
    | sed '/^#/d' > "$APPDIR/usr/share/applications/dev.mdcore.editor.desktop"
install -m 0644 "$ROOT/packaging/appimage/icon-placeholder.png" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps/mdcore.png" 2>/dev/null \
    || echo "[appimage] WARN: 图标资产未登记（ASSET_CHECK），跳过——正式发布前必须补齐（checklist C5）"

# AppRun：{{APP_EXEC}} 占位符注入（Q3-3 实测缺陷修复——原 install 直拷
# 裸进包，AppRun.wrapped 的 exec "${HERE}/usr/bin/{{APP_EXEC}}" 在任何
# 机器上都是 ENOENT；linuxdeploy 会把本文件裹成 AppRun.wrapped）。
sed -e 's/{{APP_EXEC}}/mdcore/g' \
    "$ROOT/packaging/appimage/AppRun" > "$APPDIR/AppRun"
chmod 0755 "$APPDIR/AppRun"

# linuxdeploy 收敛运行时 + appimagetool 出包（--appdir 面向 GTK 插件）
"$ROOT/build/_tools/linuxdeploy-x86_64.AppImage" \
  --appdir "$APPDIR" \
  --desktop-file "$APPDIR/usr/share/applications/dev.mdcore.editor.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/mdcore.png" \
  --plugin gtk \
  --output appimage

mv -f "mdcore-x86_64.AppImage" "$ROOT/build/mdcore-${VERSION}-x86_64.AppImage"
echo "[appimage] OK: build/mdcore-${VERSION}-x86_64.AppImage"
