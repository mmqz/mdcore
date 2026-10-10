#!/usr/bin/env bash
# deb 构建（R24-c；实体验证留 R24-d）
#
# 路线：手工布局 + dpkg-deb（不引入 cargo-deb 依赖——零新增依赖纪律；
# cargo-deb 的自动布局收益 < 依赖面成本，产品面保持最小工具链）。
#
# 输入：宿主壳 release 产物 + packaging/deb/ 模板
# 输出：build/mdcore_<VERSION>_amd64.deb
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${1:?usage: build-deb.sh <path-to-shell-binary>}"

VERSION="$("$BIN" --version | awk '{print $2}')"
echo "[deb] version(single-source)=${VERSION}"

STAGE="$ROOT/build/deb-stage/mdcore_${VERSION}_amd64"
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" "$STAGE/usr/bin" \
         "$STAGE/usr/share/applications" "$STAGE/usr/share/icons/hicolor/256x256/apps"

install -m 0755 "$BIN" "$STAGE/usr/bin/mdcore"

sed -e 's/{{APP_NAME}}/mdcore/g' \
    -e 's/{{APP_EXEC}}/mdcore/g' \
    -e 's/{{BUNDLE_ICON}}/mdcore/g' \
    "$ROOT/packaging/shared/app.desktop.tpl" \
    > "$STAGE/usr/share/applications/dev.mdcore.editor.desktop"
if [ -f "$ROOT/packaging/appimage/icon-placeholder.png" ]; then
  install -m 0644 "$ROOT/packaging/appimage/icon-placeholder.png" \
    "$STAGE/usr/share/icons/hicolor/256x256/apps/mdcore.png"
fi

# 控制面：版本注入 + Installed-Size 实测（不手抄——checklist B2）
SIZE_KB="$(( $(du -sk "$STAGE/usr" | cut -f1) ))"
sed -e "s/{{VERSION}}/${VERSION}/g" \
    -e "s/{{INSTALLED_SIZE_KB}}/${SIZE_KB}/g" \
    "$ROOT/packaging/deb/control.tpl" > "$STAGE/DEBIAN/control"
sed 's/{{APP_EXEC}}/mdcore/g' "$ROOT/packaging/deb/postinst.tpl" > "$STAGE/DEBIAN/postinst"
chmod 0755 "$STAGE/DEBIAN/postinst"

dpkg-deb --build --root-owner-group "$STAGE"
mv -f "mdcore_${VERSION}_amd64.deb" "$ROOT/build/"
echo "[deb] OK: build/mdcore_${VERSION}_amd64.deb"
