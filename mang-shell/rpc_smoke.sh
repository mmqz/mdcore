#!/usr/bin/env bash
# mang-shell RPC 桥烟雾验证（#104 任务 2「RPC 桥接准备」）。
#
# 用法: ./rpc_smoke.sh [输出目录，默认 /tmp/rpc-smoke]
#
# 判定（三通道）:
#   1. 外部 python3 客户端（tests/rpc_client.py --smoke）——六命令全链
#      断言（get_state/insert_text/replace_selection/open_file/
#      switch_tab/close_file + 越界/脏拒绝错误面）+ 双客户端并发
#      互不阻塞。真实 unix socket 通信（非进程内自测）。
#   2. 应用侧证据行（rpc.log）——"rpc: listening at" + "rpc: exec
#      <cmd> ok=<bool>"（主线程执行日志）+ "rpc: client connected"。
#   3. exit 0 且 stdout 末行 "smoke: quit reached"（进程存活全周期）。
#
# 环境自适应同 smoke.sh（sysroot 链接/运行/ICD 面）。
set -euo pipefail
cd "$(dirname "$0")"

OUT="${1:-/tmp/rpc-smoke}"
SYSROOT="${MANG_SYSROOT:-/home/z/.local/sysroot}"
export PATH="/home/z/.cargo/bin:$SYSROOT/usr/bin:/home/z/.local/bin:$PATH"

# 链接期（仅无 root 沙箱需要；宿主机自带 dev 面时该目录不存在，跳过）
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export RUSTFLAGS="-L $SYSROOT/usr/lib/x86_64-linux-gnu${RUSTFLAGS:+ $RUSTFLAGS}"
fi

CARGO_INCREMENTAL=0 cargo build

# 运行期：sysroot 运行库（仅当 sysroot 存在时挂）+ lavapipe ICD
#（仅当系统无 vulkan ICD 时挂）。
if [ -d "$SYSROOT/usr/lib/x86_64-linux-gnu" ]; then
  export LD_LIBRARY_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
if [ ! -d /usr/share/vulkan/icd.d ] && [ -f "$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json" ]; then
  export VK_DRIVER_FILES="$SYSROOT/usr/share/vulkan/icd.d/lvp_icd.json"
fi

# 已知坑 #3（#104 提醒）：清配置目录——XDG 隔离到本轮输出目录，
# settings.json/recent.json 不跨轮残留。
export XDG_CONFIG_HOME="$OUT/config"
rm -rf "$OUT"
mkdir -p "$OUT/config"

SOCK="$OUT/rpc.sock"
rm -f "$SOCK"
FILE1="$PWD/tests/fixtures/smoke.md"
FILE2="$PWD/tests/fixtures/mouse_smoke.md"

# 选空闲显示号（PID 基准避开残留 Xvfb 锁——锁存在即换号；带锁但
# 服务器已死的场景由锁清理责任方处理，这里不碰）。
DISPLAY_ID=""
for CAND in $((100 + $$ % 700)) $((200 + $$ % 600)) 97 98 99; do
  if [ -e "/tmp/.X${CAND}-lock" ]; then
    continue
  fi
  DISPLAY_ID=":$CAND"
  break
done
if [ -z "$DISPLAY_ID" ]; then
  echo "FAIL: 无空闲 X 显示号（97-99 区被占）"
  exit 1
fi
Xvfb "$DISPLAY_ID" -screen 0 1280x800x24 -nolisten tcp &
XVFB_PID=$!
cleanup() {
  kill "$XVFB_PID" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1
if ! kill -0 "$XVFB_PID" 2>/dev/null; then
  echo "FAIL: Xvfb 未能启动（$DISPLAY_ID）"
  exit 1
fi
echo "[rpc-smoke] Xvfb 就绪（$DISPLAY_ID）"

echo "[rpc-smoke] 启动 mang-shell（--rpc-sock $SOCK，16s 后自动退出）"
DISPLAY="$DISPLAY_ID" ./target/debug/mang-shell \
  --smoke-sec 16 --rpc-sock "$SOCK" "$FILE1" > "$OUT/rpc.log" 2>&1 &
APP_PID=$!

# 通道 1 前置：等 "rpc: listening"（最多 15s；进程先死即失败）
LISTENING=""
for _ in $(seq 1 150); do
  if grep -q "rpc: listening" "$OUT/rpc.log" 2>/dev/null; then
    LISTENING=1
    break
  fi
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
if [ -z "$LISTENING" ]; then
  echo "FAIL: 未出现 'rpc: listening' 行（应用日志如下）"
  cat "$OUT/rpc.log" || true
  exit 1
fi
echo "[rpc-smoke] 'rpc: listening' 已就绪"

# 通道 2：外部客户端六命令全链 + 双客户端并发（真实 socket 断言）
python3 tests/rpc_client.py "$SOCK" --smoke "$FILE1" "$FILE2"

# 等应用自然退出（--smoke-sec 16；客户端序列 ~2s，余量充足）
wait "$APP_PID" || {
  echo "FAIL: 应用退出码非零"
  tail -20 "$OUT/rpc.log" || true
  exit 1
}

# 通道 3：应用侧证据行 + quit reached
EXEC_N=$(grep -c "rpc: exec" "$OUT/rpc.log" || true)
if [ "$EXEC_N" -lt 12 ]; then
  echo "FAIL: 'rpc: exec' 证据行不足（$EXEC_N < 12）"
  exit 1
fi
grep -q "rpc: client connected" "$OUT/rpc.log" || {
  echo "FAIL: 无 'rpc: client connected' 行"
  exit 1
}
grep -q "smoke: quit reached" "$OUT/rpc.log" || {
  echo "FAIL: 无 'smoke: quit reached'（进程未活到退出钩子）"
  exit 1
}

echo "rpc-smoke: PASS（exec 证据行 ${EXEC_N} 条 + 客户端全链断言 + quit reached）"
