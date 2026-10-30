#!/usr/bin/env bash
# mang-shell 截图取证（#68 阶段 1 编辑器视觉验证）。
#
# 用法: ./screenshot.sh <输出PNG> [秒数-后-截图，默认 6] [可选：文件路径]
#
# Xvfb 常驻开窗 → 等渲染稳定 → xwd 抓根窗 → netpbm 转 PNG。
# 依赖：xvfb-run、xwd、xwdtopnm、pnmtopng（sysroot 自愈见 smoke.sh）。
set -euo pipefail
cd "$(dirname "$0")"

OUT="${1:-screenshot.png}"
WAIT="${2:-6}"
SMOKE_FILE="${3:-}"
SYSROOT="${MANG_SYSROOT:-/home/z/.local/sysroot}"
export PATH="/home/z/.cargo/bin:/home/z/.local/bin:$PATH"
export LD_LIBRARY_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
# 与 smoke.sh 同一链接面（sysroot dev 库）——RUSTFLAGS 变化会触发全量重编。
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export RUSTFLAGS="-L $SYSROOT/usr/lib/x86_64-linux-gnu${RUSTFLAGS:+ $RUSTFLAGS}"
fi

CARGO_INCREMENTAL=0 cargo build

RUN_ENV=(env)
if [ ! -d /usr/share/vulkan/icd.d ] && [ -f "$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json" ]; then
  RUN_ENV+=("VK_DRIVER_FILES=$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json")
fi

DISPLAY_ID=":98"
Xvfb "$DISPLAY_ID" -screen 0 1280x800x24 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true' EXIT
sleep 1

if [ -n "$SMOKE_FILE" ]; then
  "${RUN_ENV[@]}" DISPLAY="$DISPLAY_ID" ./target/debug/mang-shell --smoke-sec "$((WAIT + 10))" "$SMOKE_FILE" &
else
  "${RUN_ENV[@]}" DISPLAY="$DISPLAY_ID" ./target/debug/mang-shell --smoke-sec "$((WAIT + 10))" &
fi
APP_PID=$!

sleep "$WAIT"
DISPLAY="$DISPLAY_ID" xwd -root -silent -out /tmp/mang-shot.xwd
kill "$APP_PID" 2>/dev/null || true

xwdtopnm < /tmp/mang-shot.xwd > /tmp/mang-shot.ppm 2>/dev/null
pnmtopng /tmp/mang-shot.ppm > "$OUT"
echo "[screenshot] 已输出 $OUT（$PWD 绝对路径）"
