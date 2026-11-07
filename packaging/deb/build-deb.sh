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

# #320（2026-11-07）：pdfium 引擎库捆绑（与 Windows NSIS 的 pdfium.dll
# 傍 exe 对称）。取件位 = 仓根 bin/lib/libpdfium.so（#224 定位序第 3 层
# 「产品仓 bin/lib 开发取件位」——nsis build-installer.py --pdfium /
# windows-test.yml 同源纪律）；安装位 = /usr/bin/libpdfium.so（定位序
# 第 2 层「exe 同目录（产品部署位）」——零应用代码改动，干净系统上
# --check-pdfium 免环境变量即绑定）。缺件硬失败：不捆 = 装机 PDF 面
# 静默降级（#270 验收 F2 实证）。
PDFIUM_LIB="${PDFIUM_LIB:-$ROOT/bin/lib/libpdfium.so}"
if [ ! -f "$PDFIUM_LIB" ]; then
  echo "[deb] FATAL: bin/lib/libpdfium.so 不存在（PDF 引擎库，装包必需）" >&2
  echo "  取件（与 CI chromium/8086 钉扎同轨）：" >&2
  echo "  curl -sSfL -o /tmp/pdfium-linux-x64.tgz \\\\" >&2
  echo "    https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8086/pdfium-linux-x64.tgz" >&2
  echo "  # SHA256 588577cf52dabc1a444988bac841920df54cc2f141801424de97ab04f4fbb935" >&2
  echo "  mkdir -p bin/lib && tar xzf /tmp/pdfium-linux-x64.tgz -C bin/lib --strip-components=1 lib/libpdfium.so" >&2
  exit 1
fi
install -m 0755 "$PDFIUM_LIB" "$STAGE/usr/bin/libpdfium.so"

# s5a-env：工作台 bundle 捆绑（壳的 mdapp:// 服务根 = usr/share/mdcore/dist，
# bin 的 dist 解析序第三位消费）。无 bundle = 坏包，硬失败（先
# build-wasm.sh + build-pdf-wasm.sh + npm run build）。
DIST_SRC="$ROOT/web/dist"
if [ ! -f "$DIST_SRC/index.html" ]; then
  echo "[deb] FATAL: web/dist/index.html 不存在（先构建 wasm 与 web dist）" >&2
  exit 1
fi
mkdir -p "$STAGE/usr/share/mdcore"
cp -a "$DIST_SRC" "$STAGE/usr/share/mdcore/dist"

sed -e 's/mang/mdcore/g' \
    -e 's/{{APP_EXEC}}/mdcore/g' \
    -e 's/{{BUNDLE_ICON}}/mdcore/g' \
    "$ROOT/packaging/shared/app.desktop.tpl" \
    | sed '/^#/d' > "$STAGE/usr/share/applications/dev.mdcore.editor.desktop"
if [ -f "$ROOT/packaging/appimage/icon-placeholder.png" ]; then
  install -m 0644 "$ROOT/packaging/appimage/icon-placeholder.png" \
    "$STAGE/usr/share/icons/hicolor/256x256/apps/mdcore.png"
fi

# 控制面：版本注入 + Installed-Size 实测（不手抄——checklist B2）。
# grep -v '^#'：tpl 的读者注释行剥离（dpkg-deb 严格解析 control，不接受 # 行）。
SIZE_KB="$(( $(du -sk "$STAGE/usr" | cut -f1) ))"
sed -e "s/{{VERSION}}/${VERSION}/g" \
    -e "s/{{INSTALLED_SIZE_KB}}/${SIZE_KB}/g" \
    "$ROOT/packaging/deb/control.tpl" | grep -v '^#' > "$STAGE/DEBIAN/control"
sed 's/{{APP_EXEC}}/mdcore/g' "$ROOT/packaging/deb/postinst.tpl" > "$STAGE/DEBIAN/postinst"
chmod 0755 "$STAGE/DEBIAN/postinst"

dpkg-deb --build --root-owner-group "$STAGE"
mkdir -p "$ROOT/build"
# dpkg-deb 输出 = <STAGE>.deb（与 STAGE 同目录同名；R24-c 未实跑故此路径
# 从未生效——s5a-env 实跑修正）
mv -f "${STAGE}.deb" "$ROOT/build/"
echo "[deb] OK: build/mdcore_${VERSION}_amd64.deb"
