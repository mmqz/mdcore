#!/usr/bin/env python3
"""#280 P1-19 击键路径内存/延迟测量（诊断工具——gates/280.md 证据采集口径）.

大档（10MB / 20 万块）打开后连击 50 键（真实 CM6 键盘路径 → wasm
apply_ops → ReplaceText = text-class 事务），采样：
- 每键延迟 ms（p50/p95/max——含 ~90ms Playwright harness 开销，
  **同 harness 对比看差值**：pre-fix p50 366ms vs post-fix 98ms）；
- 击键期 usedJSHeapSize 高水位与 GC 稳态差（整树 clone churn）。

参考值（2026-11-06，Chromium headless，10MB/200,576 块）：
- pre-fix（整树 clone）：p50 366.2 / p95 387.7 / max 765.4ms，
  墙钟 18.7s，稳态差 +14MB；
- post-fix（受影响面备份）：p50 98.2 / p95 104.8 / max 107.4ms，
  墙钟 5.7s，稳态差 +2MB——扣除 harness 开销核心路径 ≈34×。

用法：python3 tests/measure_typing_heap.py <label>
"""
import http.server
import json
import socketserver
import sys
import threading
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript",
                               ".wasm": "application/wasm", ".mjs": "text/javascript"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True


def main():
    label = sys.argv[1] if len(sys.argv) > 1 else "run"
    KEYS = 50
    with socketserver.TCPServer(("127.0.0.1", 8799), QuietServer) as httpd:
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        with sync_playwright() as p:
            browser = p.chromium.launch(
                args=["--js-flags=--expose-gc", "--enable-precise-memory-info"])
            page = browser.new_page()
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto("http://127.0.0.1:8799/index.html?doc=big&kb=10240&chunks=16")
            page.wait_for_function("window.__ready === true", timeout=180_000)
            # 基线 GC 稳态。
            base = page.evaluate("""async () => {
              for (let i = 0; i < 3; i++) window.gc();
              await new Promise(r => setTimeout(r, 200));
              return Math.round(performance.memory.usedJSHeapSize / 1048576);
            }""")
            # 聚焦编辑器（CM6 真键盘路径）。
            page.click("#editor .cm-content")
            page.keyboard.type("x")  # 热身 1 键（首击含 focus/选区建立）
            page.wait_for_timeout(300)
            samples = []
            heap_samples = []
            t0 = time.time()
            for i in range(KEYS):
                k0 = time.time()
                page.keyboard.type(f"键{i % 10}")
                page.wait_for_timeout(6)
                samples.append((time.time() - k0) * 1000)
                heap_samples.append(page.evaluate(
                    "Math.round(performance.memory.usedJSHeapSize / 1048576)"))
            wall = (time.time() - t0) * 1000
            peak = max(heap_samples)
            steady = page.evaluate("""async () => {
              for (let i = 0; i < 3; i++) window.gc();
              await new Promise(r => setTimeout(r, 300));
              return Math.round(performance.memory.usedJSHeapSize / 1048576);
            }""")
            samples.sort()
            out = {
                "label": label,
                "keys": KEYS,
                "latency_ms": {
                    "p50": round(samples[len(samples) // 2], 1),
                    "p95": round(samples[int(len(samples) * 0.95)], 1),
                    "max": round(samples[-1], 1),
                },
                "heap_mb": {"baseline_gc": base, "peak_during": peak,
                            "peak_delta": peak - base, "steady_after_gc": steady,
                            "steady_delta": steady - base},
                "wall_ms": round(wall),
                "errors": errors[:3],
            }
            print(json.dumps(out, ensure_ascii=False, indent=1))
            browser.close()


if __name__ == "__main__":
    main()
