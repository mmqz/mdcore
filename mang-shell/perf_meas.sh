#!/usr/bin/env bash
# mang-shell 性能实测驱动（#164 1c——MANG_PERF=1 证据行采集与汇总）。
#
# 用法: ./perf_meas.sh [输出目录，默认 /tmp/perf-meas] [样本MB列表，默认 "1.5 7.5"]
#
# 测量面与红线（#68 设计口径）：
#   首帧 <2s / 滚动 ≥60fps 不低于 30 / 内存峰值 VmHWM <1GB /
#   输入延迟 <50ms / 全文查找 <1s
#
# 采集方式（与 #164 归档口径对齐）：
#   启动链  MANG_PERF=1 开窗 → perf: main-enter / doc-open / editor-built /
#          first-frame / VmHWM（退出期 /proc/self/status）
#   逐键    --smoke-perf-keys 200（30ms 间隔，真实键处理链全管线）
#          → perf: key 行逐键延迟（p50/p95/max 由汇总段计算）
#   滚动    --smoke-perf-scroll 500（16ms 间隔 ≈ 62.5/s，上下交替）
#          → 吞吐/存活证据（逐帧 fps 需实机——沙箱无 vsync 帧）
#   查找    --smoke-find-replace（G1 既有烟雾：真实查找条键入链）
#          → perf: find 行（查询长/行数/命中数/耗时）
#
# 环境自适应：宿主有 Vulkan/OpenGL 直接跑；沙箱 lavapipe 用户态
#（env_gpu.sh——LD_LIBRARY_PATH + VK_DRIVER_FILES，见 sysroot/）。
set -euo pipefail
cd "$(dirname "$0")"

OUT="${1:-/tmp/perf-meas}"
shift || true
SIZES=("$@")
[ ${#SIZES[@]} -eq 0 ] && SIZES=(1.5 7.5)

# 工具链 + GPU 用户态（存在才 source——宿主机不需要）
for envfile in /home/z/my-project/env_rust.sh /home/z/my-project/env_gpu.sh; do
    # shellcheck disable=SC1090
    [ -f "$envfile" ] && source "$envfile"
done

mkdir -p "$OUT"

# ---------------------------------------------------------------------------
# ① 样本生成：真实形态 markdown（标题 / CJK+拉丁正文 / 列表 / 围栏代码）
# ---------------------------------------------------------------------------
gen_sample() { # $1 = 目标 MB
    python3 - "$1" << 'PYEOF'
import sys
target = int(float(sys.argv[1]) * 1024 * 1024)
out = []
n = 0
while sum(len(l) + 1 for l in out) < target:
    n += 1
    out.append(f"# 章节 {n}：性能样本标题（measurement sample）")
    for _ in range(6):
        out.append(
            "正文段落：mang-shell 性能验证样本 GPUI 渲染引擎 workspace "
            "buffer display_map 折叠 围栏 markdown 编辑器。"
            "Latin mixed content for realistic measurement abc def ghi."
        )
        out.append("")
    out.append("- 列表项一（性能样本）")
    out.append("- 列表项二（measurement）")
    out.append("")
    out.append("```rust")
    out.append(f"fn sample_{n}() -> u32 {{ let x = {n}; x * 2 + 1 }}")
    out.append("```")
    out.append("")
sys.stdout.write("\n".join(out)[:target])
PYEOF
}

# ---------------------------------------------------------------------------
# ② 单轮运行：$1=样本 $2=案名 $3=smoke-sec 其后=附加 smoke 旗标
# ---------------------------------------------------------------------------
run_case() {
    local sample="$1" name="$2" secs="$3"
    shift 3
    echo "--- run: $name (secs=$secs $*) ---"
    MANG_PERF=1 timeout $((secs + 25)) \
        ./target/debug/mang-shell --smoke-sec "$secs" "$@" "$sample" \
        > "$OUT/$name.log" 2>&1 || true
    if ! grep -q "smoke: quit reached" "$OUT/$name.log"; then
        echo "!! $name: quit reached 缺失（进程可能异常退出）"
        tail -5 "$OUT/$name.log"
    fi
}

# ---------------------------------------------------------------------------
# ③ 执行矩阵
# ---------------------------------------------------------------------------
CARGO_INCREMENTAL=0 cargo build --bin mang-shell

DISPLAY_ID=":98"
Xvfb "$DISPLAY_ID" -screen 0 1280x800x24 2>/dev/null &
XVFB_PID=$!
trap 'kill $XVFB_PID 2>/dev/null || true' EXIT
sleep 1
export DISPLAY="$DISPLAY_ID"

KEYS=200
SCROLLS=500
for mb in "${SIZES[@]}"; do
    sample="$OUT/sample_${mb}mb.md"
    gen_sample "$mb" > "$sample"
    lines=$(wc -l < "$sample")
    echo "=== 样本 ${mb}MB：$lines 行 ==="

    # 逐键量与时长按规模配置（实测标定：软件渲染 debug 构建下逐键处理
    # ≈ 0.33s@32K 行——处理成本高于 30ms 发送间隔时事件排队，时长必须
    # 给足到 script done 必现，否则样本被 smoke-sec 截断）。
    if python3 -c "import sys; sys.exit(0 if float('$mb') > 3 else 1)"; then
        KEYS_THIS=60    # 大样本：键量收敛（分位数足够）时长放宽
        KEYS_SECS=120
        SCROLL_SECS=90
    else
        KEYS_THIS=150
        KEYS_SECS=70
        SCROLL_SECS=50
    fi

    # 启动链（首帧/doc-open/editor-built/VmHWM）
    run_case "$sample" "start_${mb}" 8
    # 逐键延迟（30ms 发送间隔；延迟口径 = dispatch_key 进入到处理完毕，
    # 不含排队等待）
    run_case "$sample" "keys_${mb}" "$KEYS_SECS" --smoke-perf-keys "$KEYS_THIS"
    # 滚动吞吐（16ms 发送间隔 ≈ 62.5/s；沙箱软件渲染下逐事件壁钟 >16ms
    # 会排队——script done 总耗时为实际吞吐证据）
    run_case "$sample" "scroll_${mb}" "$SCROLL_SECS" --smoke-perf-scroll "$SCROLLS"
done

# 全文查找（最大样本上跑 G1 查找替换烟雾——真实查找条键入链）
BIGGEST="${SIZES[-1]}"
run_case "$OUT/sample_${BIGGEST}mb.md" "find_${BIGGEST}" 14 --smoke-find-replace

# ---------------------------------------------------------------------------
# ④ 汇总（红线对照 + 逐键分位数）
# ---------------------------------------------------------------------------
python3 - "$OUT" "$KEYS" "$SCROLLS" "${SIZES[@]}" << 'PYEOF'
import re, sys, os, statistics as st

out, keys_n, scrolls_n = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
sizes = sys.argv[4:]

def lines_of(name):
    p = os.path.join(out, f"{name}.log")
    if not os.path.exists(p):
        return []
    return open(p, encoding="utf-8", errors="replace").read().splitlines()

def grab(name, pat):
    for l in lines_of(name):
        m = re.search(pat, l)
        if m:
            return m.group(1) if m.groups() else m.group(0)
    return "-"

def key_latencies(name):
    vals = []
    for l in lines_of(name):
        m = re.match(r"perf: key x ([\d.]+)ms", l)
        if m:
            vals.append(float(m.group(1)))
    return vals

print("\n## 性能实测汇总（MANG_PERF=1，Xvfb + lavapipe 软件渲染）\n")
print("| 样本 | doc-open | editor-built | first-frame（红线 <2s） | VmHWM（红线 <1GB） |")
print("|---|---|---|---|---|")
for mb in sizes:
    d = grab(f"start_{mb}", r"perf: doc-open md \d+B \d+L ([\d.]+)ms")
    e = grab(f"start_{mb}", r"perf: editor-built ([\d.]+)ms")
    f = grab(f"start_{mb}", r"perf: first-frame ([\d.]+)ms")
    v = grab(f"start_{mb}", r"VmHWM:\s+(\d+) kB")
    if v != "-":
        v = f"{int(v)/1024/1024:.2f}GB"
    print(f"| {mb}MB | {d}ms | {e}ms | {f}ms | {v} |")

print(f"\n### 逐键延迟（真实键处理链，30ms 发送间隔——处理成本高于间隔时排队，\n延迟口径 = dispatch_key 进入到处理完毕，不含排队等待）\n")
print("| 样本 | 实测键数 | p50 | p95 | max（红线 <50ms） |")
print("|---|---|---|---|---|")
for mb in sizes:
    v = key_latencies(f"keys_{mb}")
    if v:
        v.sort()
        p50 = v[len(v)//2]
        p95 = v[int(len(v)*0.95)]
        print(f"| {mb}MB | {len(v)} | {p50:.2f}ms | {p95:.2f}ms | {max(v):.2f}ms |")
    else:
        print(f"| {mb}MB | 0 | （无 key 行） | | |")

print(f"\n### 滚动吞吐（{scrolls_n} events，16ms 发送间隔 ≈ 62.5/s）\n")
for mb in sizes:
    done = grab(f"scroll_{mb}", r"perf-scroll: script done \(\d+ events, ([\d.]+)s\)")
    alive = "是" if any("smoke: quit reached" in l for l in lines_of(f"scroll_{mb}")) else "否"
    print(f"- {mb}MB：总耗时 {done}s（全程存活 = {alive}）")

print("\n### 全文查找（最大样本，真实查找条键入链）\n")
for l in lines_of(f"find_{sizes[-1]}"):
    if l.startswith("perf: find "):
        print(f"- {l[6:]}")

print("\n注：滚动逐帧 fps 与真实 vsync 需实机验证（沙箱 Xvfb 无帧时钟）；")
print("    逐键/查找为真实派发管线端到端计时（含 doc_events/渲染失效）。")
PYEOF

echo
echo "完整日志：$OUT/*.log"
