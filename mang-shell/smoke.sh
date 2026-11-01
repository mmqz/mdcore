#!/usr/bin/env bash
# mang-shell Xvfb 烟雾验证（循 mdshell-native examples/xvfb_smoke 先例）。
#
# 用法: ./smoke.sh [烟雾秒数，默认 5] [可选：打开的文件路径]
#
# 判定: exit 0 且 stdout 末行 "smoke: quit reached"。
# 带文件模式（#68 阶段 1）：编辑器打开真实 md 文档渲染——
#   ./smoke.sh 5 tests/fixtures/smoke.md
# 环境自适应：
#   - 宿主机有完整系统依赖（xkbcommon dev/vulkan ICD）→ 直接 Xvfb 跑
#   - 无 root 沙箱（libxkbcommon dev / lavapipe 缺失）→ 自动挂用户态
#     sysroot（$MANG_SYSROOT，默认 /home/z/.local/sysroot）的链接/运行/ICD 面
set -euo pipefail
cd "$(dirname "$0")"

SMOKE_SEC="${1:-5}"
SMOKE_FILE="${2:-}"
SYSROOT="${MANG_SYSROOT:-/home/z/.local/sysroot}"
# 截图/授权链（xwd/xauth/netpbm）与 cargo 均可能在 sysroot/用户目录。
export PATH="/home/z/.cargo/bin:$SYSROOT/usr/bin:/home/z/.local/bin:$PATH"

# 链接期（仅无 root 沙箱需要；宿主机自带 dev 面时该目录不存在，跳过）
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export RUSTFLAGS="${RUSTFLAGS:--L native=$SYSROOT/usr/lib/x86_64-linux-gnu}"
fi

CARGO_INCREMENTAL=0 cargo build

# 运行期：sysroot 运行库 + lavapipe ICD（仅当系统无 vulkan ICD 时挂）。
# 注意：用 export 直传——`env VAR=... cmd` 前缀形式在 xvfb-run 下会
# 静默 127（r39 实测），不用 RUN_ENV 数组。
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export LD_LIBRARY_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
if [ ! -d /usr/share/vulkan/icd.d ] && [ -f "$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json" ]; then
  export VK_DRIVER_FILES="$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json"
fi

if [ -n "$SMOKE_FILE" ]; then
  echo "[smoke] xvfb-run -a ./target/debug/mang-shell --smoke-sec $SMOKE_SEC $SMOKE_FILE"
  xvfb-run -a -s "-screen 0 1280x800x24" \
    ./target/debug/mang-shell --smoke-sec "$SMOKE_SEC" "$SMOKE_FILE"
else
  echo "[smoke] xvfb-run -a ./target/debug/mang-shell --smoke-sec $SMOKE_SEC"
  xvfb-run -a -s "-screen 0 1280x800x24" \
    ./target/debug/mang-shell --smoke-sec "$SMOKE_SEC"
fi
