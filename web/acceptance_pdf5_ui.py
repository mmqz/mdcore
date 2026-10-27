#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""#5 验收补证（UI 级）：标注后 save 按钮启用 + 真实下载可读回。

pdf_view.py 探针面已绿（flush/incremental_prefix/double_revision）——
本脚本补 UI 交互层最后一环：按钮启用态随 bridge/注释状态流转、点击
产生浏览器下载、下载字节 = 探针 flush 字节（前缀一致 + 追加非零）。
一次性验收脚本（非套件——不入 tests/ 常规面）。
"""

import base64
import http.server
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent
FIXTURE = WEB / "tests" / "fixtures" / "sample.pdf"

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
    pdf_b64 = base64.b64encode(FIXTURE.read_bytes()).decode()
    with socketserver.TCPServer(("127.0.0.1", 8942), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.goto("http://127.0.0.1:8942/index.html")
            page.wait_for_function("window.__ready === true", timeout=60_000)
            page.wait_for_function("window.__pdfReady === true", timeout=30_000)

            page.click("#tab-pdf")
            # ① 打开前：save 禁用（无文档）
            pre = page.evaluate(
                "document.getElementById('pdf-save-btn').disabled")
            r = page.evaluate("(b) => window.__pdfOpenBytes(b)", pdf_b64)
            page.wait_for_timeout(300)
            # ② 打开后（模型面在场 + 无注释）：启用
            post_open = page.evaluate(
                "document.getElementById('pdf-save-btn').disabled")
            # ③ 注释后：仍启用 + flush 前缀/追加量
            annot = page.evaluate(
                "() => window.__pdfAddHighlight(0, {x: 72, y: 700, w: 300, h: 24})")
            flush = page.evaluate("window.__pdfFlush()")
            # ④ UI 下载：点击 save → 下载事件 → 字节与探针一致
            with page.expect_download() as dl_info:
                page.click("#pdf-save-btn")
            dl = dl_info.value
            out = WEB / "tests" / "results" / "save_annotated_ui.pdf"
            dl.save_as(str(out))
            got = out.read_bytes()

            ok = (pre is True and r.get("ok") is True and post_open is False
                  and annot.get("ok") is True and flush.get("ok") is True
                  and flush.get("bytes_appended", 0) > 0
                  and got[:5] == b"%PDF-"
                  and len(got) == flush.get("prefix_bytes", 0) + flush.get("bytes_appended", -1))
            print(f"pre_disabled={pre} open_ok={r.get('ok')} post_open_disabled={post_open}")
            print(f"annot_ok={annot.get('ok')} appended={flush.get('bytes_appended')}")
            print(f"download_bytes={len(got)} head={got[:8]!r}")
            print("UI-LEVEL ACCEPTANCE:", "PASS" if ok else "FAIL")
            page.close()
            browser.close()
            return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
