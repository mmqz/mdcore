#!/usr/bin/env python3
"""R19 one-off: 10MB progressive open heap attribution.
Reads window.__heapTrace (phase-by-phase) + structure inventory + GC steady
state + post-main() re-sample (chunks array should be freed after main returns)."""
import http.server
import json
import os
import socketserver
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class Quiet(Handler):
    def log_message(self, *a):
        pass


class Server(socketserver.TCPServer):
    allow_reuse_address = True


def main():
    os.chdir(WEB / "dist")
    with Server(("127.0.0.1", 8798), Quiet) as httpd:
        t = threading.Thread(target=httpd.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as p:
            browser = p.chromium.launch(args=["--js-flags=--expose-gc", "--enable-precise-memory-info"])
            page = browser.new_page()
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto("http://127.0.0.1:8798/index.html?doc=big&kb=10240&chunks=16")
            page.wait_for_function("window.__ready === true", timeout=180_000)
            r = page.evaluate("""async () => {
              const mem = () => {
                const m = performance.memory;
                return m ? Math.round(m.usedJSHeapSize / 1048576) : -1;
              };
              const out = {};
              out.heapTrace = window.__heapTrace;
              // 阶段外补采：main() 已返回（chunks 数组应已可回收）
              out.used_before_gc = mem();
              for (let i = 0; i < 3; i++) { window.gc(); }
              await new Promise(res => setTimeout(res, 200));
              out.used_after_gc = mem();
              // 结构清单
              const doc = window.__view.state.doc;
              const bm = window.__bridge.blockMap;
              out.inv = {
                doc_len: doc.length,
                doc_lines: doc.lines,
                blocks: window.__blockCount(),
                blockMap_lines: bm.lines ? bm.lines.length : null,
                blockMap_byId: bm.byId ? bm.byId.size : null,
                events: window.__mdcoreEvents.length,
                // CM6 doc 结构尺寸旁证：序列化成本
                md_len: window.__md().length,
              };
              // 二次 GC 稳态（confirm）
              for (let i = 0; i < 2; i++) { window.gc(); }
              await new Promise(res => setTimeout(res, 200));
              out.steady_after_gc2 = mem();
              return out;
            }""")
            browser.close()
    print(json.dumps(r, indent=1, ensure_ascii=False))
    print("errors:", errors[:5])


if __name__ == "__main__":
    main()
