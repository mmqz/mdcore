#!/usr/bin/env bash
# mang-shell 鼠标烟雾验证（#68 阶段 1 批 2：点击定位/拖选/双击/三击/滚轮）。
#
# 用法: ./mouse_smoke.sh [输出目录，默认 /tmp/mouse-smoke]
#
# 判定（三通道）:
#   1. exit 0 且 stdout 末行 "smoke: quit reached"（进程存活全周期）
#   2. mouse 日志断言（stdout 捕获）——down/drag/up/selection 序列齐全
#   3. 定时截图（单击/拖选/双击/三击四阶段）——VLM 视觉裁决
#
# 环境自适应同 smoke.sh（sysroot 链接/运行/ICD 面）。
set -euo pipefail
cd "$(dirname "$0")"

OUT="${1:-/tmp/mouse-smoke}"
SMOKE_FILE="${2:-tests/fixtures/mouse_smoke.md}"
SYSROOT="${MANG_SYSROOT:-/home/z/.local/sysroot}"
export PATH="/home/z/.cargo/bin:$SYSROOT/usr/bin:/home/z/.local/bin:$PATH"
export LD_LIBRARY_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export RUSTFLAGS="${RUSTFLAGS:--L native=$SYSROOT/usr/lib/x86_64-linux-gnu}"
  export PKG_CONFIG_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu/pkgconfig"
  export CPATH="$SYSROOT/usr/include"
fi

mkdir -p "$OUT"
CARGO_INCREMENTAL=0 cargo build

# 运行期环境（export 直传——`env` 前缀形式在 xvfb-run 下静默 127）。
if [ ! -d /usr/share/vulkan/icd.d ] && [ -f "$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json" ]; then
  export VK_DRIVER_FILES="$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json"
fi

DISPLAY_ID=":97"
Xvfb "$DISPLAY_ID" -screen 0 1280x800x24 &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true' EXIT
sleep 1

# 鼠标脚本时间轴（run_mouse_script 脚本钟；smoke-sec 18 覆盖全序列）：
#   2.5s 单击 → 2.8s 抬起 → 4.8s 拖选按下 → 5.1/5.4s 拖移 → 5.8s 抬起
#   → 7.8s 双击 → 8.1s 抬起 → 10.1s 三击 → 10.4s 抬起 → 12.5s 滚轮
#   → 14.0s 出界拖拽按下 → 14.3s 拖出底缘(y=830) → 15.2s 抬起（自动滚动期）
# 截图时间从 launch 起算 = 脚本钟 + 启动滞后 L（实测 ~1.2s，容差 [0.5,2]）——
# 各阶段稳定窗中点取图，L 全容差范围内不串拍。
DISPLAY="$DISPLAY_ID" ./target/debug/mang-shell \
  --smoke-sec 18 --smoke-mouse "$SMOKE_FILE" > "$OUT/mouse.log" 2>&1 &
APP_PID=$!

# 定时截图（与脚本阶段对齐；xwd→pnm→png 走 sysroot 链）
shot() {
  local name="$1" at="$2"
  (
    sleep "$at"
    DISPLAY="$DISPLAY_ID" xwd -root -silent -out "$OUT/$name.xwd" 2>/dev/null || true
    xwdtopnm < "$OUT/$name.xwd" > "$OUT/$name.ppm" 2>/dev/null || true
    pnmtopng "$OUT/$name.ppm" > "$OUT/$name.png" 2>/dev/null || true
    rm -f "$OUT/$name.xwd" "$OUT/$name.ppm"
  ) &
}

shot stage1-click    4.8   # 单击定位后（光标第 4 行——脚本 2.8s 完成 + 启动滞后）
shot stage2-drag     7.8   # 拖选后（4:10..10:14 选区保留）
shot stage3-word    10.2   # 双击选词后（「。」高亮）
shot stage4-line    12.6   # 三击选行后（第 4 行整行高亮）
shot stage5-scroll  14.4   # 滚轮后（内容向下滚 3 行）

wait "$APP_PID"
echo "=== mouse.log ==="
cat "$OUT/mouse.log"
echo "=== 截图产物 ==="
ls -la "$OUT"/*.png 2>/dev/null || echo "(无截图——截图链异常)"

# 日志断言（通道 2）：五类事件齐全 + 选区非空
LOG="$OUT/mouse.log"
assert_has() { grep -q "$1" "$LOG" || { echo "ASSERT FAIL: 缺少 $1"; exit 1; }; }
assert_has "mouse: down count=1"
assert_has "mouse: down count=2"
assert_has "mouse: down count=3"
assert_has "mouse: drag"
assert_has "mouse: up -- selection"
assert_has "mouse: autoscroll"
grep -q "smoke: quit reached" "$LOG" || { echo "ASSERT FAIL: 未达 quit"; exit 1; }
echo "ASSERT PASS: 事件序列齐全 + quit reached"
