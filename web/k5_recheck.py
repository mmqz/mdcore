#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""#8 K5 重核：canvas 连续 add×20 的 undo 深度与可回退性。"""

import http.server
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript",
                               ".wasm": "application/wasm", ".mjs": "text/javascript"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True


def main() -> int:
    with socketserver.TCPServer(("127.0.0.1", 8944), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.goto("http://127.0.0.1:8944/index.html")
            page.wait_for_function("window.__ready === true", timeout=60_000)
            page.click("#tab-canvas")
            page.wait_for_function("window.__canvasReady === true", timeout=30_000)
            for i in range(20):
                page.evaluate(
                    "([x, y, l]) => window.__canvasAddNode(x, y, l)",
                    [20 + i * 30, 40 + i * 20, f"k5-{i}"])
            page.wait_for_timeout(150)
            st = page.evaluate("window.__canvasState()")
            # 逐次 undo 到第 1 个（可回退性——不只看深度数字）
            undone = 0
            while page.evaluate("window.__canvasState()")["undoDepth"] > 0 and undone < 30:
                page.click("#canvas-undo-btn")
                undone += 1
                page.wait_for_timeout(30)
            final = page.evaluate("window.__canvasState()")
            print(f"after 20 adds: undoDepth={st['undoDepth']} objects={st['objects']}")
            print(f"undo clicks possible: {undone}; final objects={final['objects']} "
                  f"undoDepth={final['undoDepth']}")
            # 判定：能撤到 0 对象 = 无 8 步截断
            verdict = "NO-8-CAP" if final["objects"] == 0 and undone > 8 else (
                "CAP-8" if undone <= 8 else "PARTIAL")
            print("K5 VERDICT:", verdict)
            page.close()
            browser.close()
            return 0


if __name__ == "__main__":
    sys.exit(main())
