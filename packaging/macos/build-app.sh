#!/usr/bin/env bash
# packaging/macos/build-app.sh —— macOS .app bundle 构建脚本
# #228 R4 任务 4：构建路径落地（此前 macOS 仅 Info.plist 模板，纸面平台）。
#
# 链路：cargo release 构建 mang-shell → 载荷落位（mdcore = mang-shell 重命名
#       + libpdfium.dylib 同束）→ Info.plist 渲染（{{VERSION}} 单源注入）→
#       ad-hoc 签名 → 部署自检（--check-pdfium，#224 复用）→ 分发 zip。
#
# 版本单源：packaging/version.py（根 [workspace.package].version =
#       mdcore::VERSION，r24 锁背书）——与 NSIS 链同源（#228）。
# pdfium：pin bblanchon chromium/8086 mac-univ（与 windows-test.yml 的
#       win-x64 同 release 同 pin 纪律；升级 = 两处同步改）。
#       dylib 放 Contents/MacOS/（exe 同目录 = 定位序 exe-dir 候选，
#       #223/#224——与 Windows 安装包的 pdfium.dll 同语义）。
#
# 用法（仓根执行）：
#   bash packaging/macos/build-app.sh                 # 全链（需 macOS）
#   bash packaging/macos/build-app.sh --skip-build    # 复用已构建产物
#   bash packaging/macos/build-app.sh --pdfium <dylib路径>   # 自带 pdfium
#   bash packaging/macos/build-app.sh --render-only   # 只渲染+校验 Info.plist
#                                                     # （任意平台可跑——
#                                                      本地模板面验证通道）
#
# 退出码：0 = 全链成功；非 0 = 失败（构建/校验/自检任一环）。
# 本地（Linux）只能验证模板面（--render-only）——macOS 实体构建按
# issue #228 验收口径由 Lead/macOS 环境复核，如实登记。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP_NAME="mang"          # 显示名（A4 定名 2026-10-27，packaging/README 定名记录）
APP_EXEC="mdcore"        # 可执行名（自研命名域，TRADEMARK_RULES §1；与 NSIS 同名域）
OUT_DIR="$ROOT/build/macos"
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8086/pdfium-mac-univ.tgz"
PDFIUM_SHA256="ceb8dff448db2a2b15ac52e1360427edc3461f2b51cb4579b1462f1a931087d5"

usage() {
  sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
}

SKIP_BUILD=0
RENDER_ONLY=0
PDFIUM_PATH=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1 ;;
    --render-only) RENDER_ONLY=1 ;;
    --pdfium)
      [[ $# -ge 2 ]] || { echo "--pdfium 需要路径参数" >&2; exit 2; }
      PDFIUM_PATH="$2"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "未知参数：$1（--help 看用法）" >&2; exit 2 ;;
  esac
  shift
done

# ── Info.plist 渲染（模板面——任意平台可验证）──────────────────────────
# {{VERSION}} ← packaging/version.py（mdcore::VERSION 单源）；
# CFBundleIconFile 键值对剥离（BUNDLE_ICON 资产未到位——checklist C5 资产
# 轮落地后恢复注入；plist 无图标键合法）。BSD/GNU sed 兼容（N;d 两行删）。
render_plist() {
  local ver="$1" dst="$2"
  sed -e "s/{{VERSION}}/$ver/g" \
      -e '/<key>CFBundleIconFile<\/key>/{N;d;}' \
      "$ROOT/packaging/macos/Info.plist.tpl" > "$dst"
  if grep -q '{{' "$dst"; then
    echo "FAIL: Info.plist 渲染残留占位符：$dst" >&2
    exit 1
  fi
  # 良构校验（plistlib 逐键解析——渲染产物可装载性证明）
  python3 - "$dst" <<'PY'
import plistlib, sys
with open(sys.argv[1], "rb") as f:
    data = plistlib.load(f)
assert data["CFBundleExecutable"] == "mdcore", "CFBundleExecutable 必须为 mdcore"
assert data["CFBundleName"] == "mang", "CFBundleName 必须为定名 mang"
print("plist well-formed + 命名域断言通过")
PY
}

VER="$(python3 "$ROOT/packaging/version.py")"
echo "[build-app] 版本（mdcore::VERSION 单源）：$VER"

if [[ "$RENDER_ONLY" == 1 ]]; then
  mkdir -p "$OUT_DIR"
  render_plist "$VER" "$OUT_DIR/Info.plist"
  echo "[build-app] --render-only 完成：$OUT_DIR/Info.plist（模板面验证通道，无需 macOS）"
  exit 0
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "FAIL: 本脚本实体构建需 macOS（cargo 构建 mang-shell + codesign + bundle）。" >&2
  echo "      模板面验证：bash packaging/macos/build-app.sh --render-only" >&2
  exit 2
fi

# ── 1) 构建主线壳（release --locked，与 BUILD-VERIFY 口径一致）─────────
if [[ "$SKIP_BUILD" == 0 ]]; then
  (cd "$ROOT/mang-shell" && cargo build --release --locked)
fi
EXE="$ROOT/mang-shell/target/release/mang-shell"
[[ -f "$EXE" ]] || { echo "FAIL: $EXE 不存在（--skip-build 复用前须先构建）" >&2; exit 1; }

# ── 2) pdfium dylib（univ pin 自动取件，或 --pdfium 显式指定）──────────
if [[ -z "$PDFIUM_PATH" ]]; then
  PDFIUM_PATH="$OUT_DIR/pdfium/libpdfium.dylib"
  if [[ ! -f "$PDFIUM_PATH" ]]; then
    mkdir -p "$OUT_DIR/pdfium"
    curl -fL "$PDFIUM_URL" -o "$OUT_DIR/pdfium/pdfium-mac-univ.tgz"
    echo "$PDFIUM_SHA256  $OUT_DIR/pdfium/pdfium-mac-univ.tgz" | shasum -a 256 -c -
    rm -rf "$OUT_DIR/pdfium/x" && mkdir -p "$OUT_DIR/pdfium/x"
    tar -xzf "$OUT_DIR/pdfium/pdfium-mac-univ.tgz" -C "$OUT_DIR/pdfium/x"
    cp "$OUT_DIR/pdfium/x/lib/libpdfium.dylib" "$PDFIUM_PATH"
  fi
fi
[[ -f "$PDFIUM_PATH" ]] || { echo "FAIL: pdfium dylib 缺失：$PDFIUM_PATH" >&2; exit 1; }

# ── 3) bundle 布局（mang.app：显示名目录 + mdcore 可执行 + 同束 dylib）──
APP="$OUT_DIR/$APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$EXE" "$APP/Contents/MacOS/$APP_EXEC"
# 同束定位（衔接 #224 定位序）：dylib 与可执行同目录 = exe-dir 候选
cp "$PDFIUM_PATH" "$APP/Contents/MacOS/libpdfium.dylib"
render_plist "$VER" "$APP/Contents/Info.plist"

# ── 4) ad-hoc 签名（无开发者账号可本机运行；正式发布签名/公证 = checklist F3）──
if command -v codesign >/dev/null 2>&1; then
  codesign --force --sign - "$APP"
else
  echo "WARN: codesign 不可用——跳过 ad-hoc 签名（Gatekeeper 引导文案见 checklist D3）"
fi

# ── 5) 部署自检（#224 复用；退出码 0=已绑定 / 3=未绑定——非零即脚本失败）──
"$APP/Contents/MacOS/$APP_EXEC" --check-pdfium
"$APP/Contents/MacOS/$APP_EXEC" --version

# ── 6) 分发 zip + 校验和（checklist F2：SHA-256 随发布公布）────────────
if command -v ditto >/dev/null 2>&1; then
  ZIP="$OUT_DIR/$APP_NAME-$VER-macos.zip"
  ditto -c -k --keepParent "$APP" "$ZIP"
  shasum -a 256 "$ZIP"
fi
echo "[build-app] 完成：$APP（$VER）"
