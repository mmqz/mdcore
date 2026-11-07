#!/usr/bin/env bash
# AppImage 构建（R24-c；实体验证留 R24-d——本脚本在开发环境执行）
#
# 输入：宿主壳 release 产物（主线壳 mang-shell，wgpu 自绘链）
# 输出：mdcore-<VERSION>-x86_64.AppImage
#
# 用法：packaging/appimage/build-appimage.sh <path-to-shell-binary>
# 前置：linuxdeploy + appimagetool（CI 缓存或下载到 build/_tools/；
#       版本锁定见下 *_VER）。
# #319（2026-11-07）：去 --plugin gtk。旧壳 mdshell-native（tao+wry+GTK3）
# 时代引入；mang-shell 零 GTK 引用 → 插件无法自动检测版本首跑必红
# （DEPLOY_GTK_VERSION 强制才能出包，且捆绑的 GTK 运行时对自绘壳
# 全为死重）。去掉后 linuxdeploy 只收敛 DT_NEEDED 直链集（xcb/xkbcommon
# 系），#270 验收实测出包 36.9MB 全链可跑。
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

# #331（2026-11-07）：出包 strip——staged 副本剥 debug section 与符号表
# （mang-shell [profile.release] debug="limited" → 裸产物 306MB+）。
# --strip-all（非 --strip-debug：符号表 +13MB 会冲破 50MB 验收门，且
# 无符号化基础设施时保留函数名收益有限）；构建产物原件不动。
# 与 build-deb.sh 同段同款（#331 验收门：deb/AppImage 安装面 < 50MB）。
strip --strip-all "$APPDIR/usr/bin/mdcore"

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

# #331（2026-11-07）：web/dist bundle 移除。s5a-env 时代的该段是旧壳
# mdshell-native（tao+wry+webview）的 UI 载荷面（三候选解析序
# --dist / MDCORE_DIST / /usr/share/mdcore/dist / ./web/dist 的消费方
# 是旧壳）——现主线壳 mang-shell（wgpu 自绘链，#228 换代）全树零引用
# web/dist / MDCORE_DIST / mdapp://（mang-shell/src 全量 grep 无命中，
# #331 在档）。出包不再前置 wasm×2 + npm 构建，AppDir 亦不再携带死重
# （AppRun 的 MDCORE_DIST 导出同步移除）。

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

# linuxdeploy 收敛运行时 + appimagetool 出包（#319：mang-shell 为 wgpu
# 自绘链，无 GTK 运行时依赖——不挂插件，只收敛直链集）
"$ROOT/build/_tools/linuxdeploy-x86_64.AppImage" \
  --appdir "$APPDIR" \
  --desktop-file "$APPDIR/usr/share/applications/dev.mdcore.editor.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/mdcore.png" \
  --output appimage

mv -f "mdcore-x86_64.AppImage" "$ROOT/build/mdcore-${VERSION}-x86_64.AppImage"
echo "[appimage] OK: build/mdcore-${VERSION}-x86_64.AppImage"
