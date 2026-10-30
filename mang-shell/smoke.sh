#!/usr/bin/env bash
# mang-shell Xvfb 烟雾验证（循 mdshell-native examples/xvfb_smoke 先例）。
#
# 用法: ./smoke.sh [烟雾秒数，默认 5]
#
# 判定: exit 0 且 stdout 末行 "smoke: quit reached"。
# 环境自适应：
#   - 宿主机有完整系统依赖（xkbcommon/vulkan ICD）→ 直接 Xvfb 跑
#   - 无 root 沙箱（libxkbcommon-x11 / lavapipe 缺失）→ 自动挂用户态
#     sysroot（$MANG_SYSROOT，默认 /home/z/.local/sysroot）的链接/运行/ICD 面
set -euo pipefail
cd "$(dirname "$0")"

SMOKE_SEC="${1:-5}"
SYSROOT="${MANG_SYSROOT:-/home/z/.local/sysroot}"

# 链接期（仅无 root 沙箱需要；宿主机自带 dev 面时该目录不存在，跳过）
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export LIBRARY_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu${LIBRARY_PATH:+:$LIBRARY_PATH}"
  export PKG_CONFIG_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi

cargo build

# 运行期：sysroot 运行库 + lavapipe ICD（仅当系统无 vulkan ICD 时挂）
RUN_ENV=(env)
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  RUN_ENV+=("LD_LIBRARY_PATH=$SYSROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}")
fi
if [ ! -d /usr/share/vulkan/icd.d ] && [ -f "$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json" ]; then
  RUN_ENV+=("VK_DRIVER_FILES=$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json")
fi

echo "[smoke] xvfb-run -a ./target/debug/mang-shell --smoke-sec $SMOKE_SEC"
xvfb-run -a -s "-screen 0 1280x800x24" "${RUN_ENV[@]}" \
  ./target/debug/mang-shell --smoke-sec "$SMOKE_SEC"
