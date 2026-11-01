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
# 截图链（xwd/xwdtopnm/pnmtopng）与 cargo 均可能在 sysroot/用户目录
# （sysroot bin 对齐 smoke.sh——r40 实施发现：宿主无系统级 x11-apps 时
# 本脚本曾因缺 $SYSROOT/usr/bin 而 xwd not found）。
export PATH="/home/z/.cargo/bin:$SYSROOT/usr/bin:/home/z/.local/bin:$PATH"
export LD_LIBRARY_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
# 与 smoke.sh 同一链接面（sysroot dev 库）——RUSTFLAGS 变化会触发全量重编。
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export RUSTFLAGS="${RUSTFLAGS:--L native=$SYSROOT/usr/lib/x86_64-linux-gnu}"
fi

CARGO_INCREMENTAL=0 cargo build

# 运行期 ICD（export 直传——`env` 前缀形式在 xvfb-run/Xvfb 下静默 127，
# r39 实测）。
if [ ! -d /usr/share/vulkan/icd.d ] && [ -f "$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json" ]; then
  export VK_DRIVER_FILES="$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json"
fi

DISPLAY_ID=":98"
# 屏面 1600×1000 > 窗面 1280×800@（96,64）——整窗入画（r40 实施发现：
# 1280×800 屏下窗底 64px 被裁，状态栏（页码验收面）恰在屏外）。
Xvfb "$DISPLAY_ID" -screen 0 1600x1000x24 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true' EXIT
sleep 1

if [ -n "$SMOKE_FILE" ]; then
  DISPLAY="$DISPLAY_ID" ./target/debug/mang-shell --smoke-sec "$((WAIT + 10))" "$SMOKE_FILE" &
else
  DISPLAY="$DISPLAY_ID" ./target/debug/mang-shell --smoke-sec "$((WAIT + 10))" &
fi
APP_PID=$!

sleep "$WAIT"
DISPLAY="$DISPLAY_ID" xwd -root -silent -out /tmp/mang-shot.xwd
kill "$APP_PID" 2>/dev/null || true

xwdtopnm < /tmp/mang-shot.xwd > /tmp/mang-shot.ppm 2>/dev/null
pnmtopng /tmp/mang-shot.ppm > "$OUT"
echo "[screenshot] 已输出 $OUT（$PWD 绝对路径）"
