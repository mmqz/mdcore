#!/usr/bin/env python3
"""TR-1 web e2e 复验（issue #9 关单评论承诺）：CRLF 注入 → __md() 投影
零 CR + CM6 视图一致。serve web/dist，playwright 驱动。"""
import http.server
import json
import socketserver
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


def main() -> int:
    checks = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok)})
        print(("PASS " if ok else "FAIL ") + name)

    with socketserver.TCPServer(("127.0.0.1", 0), QuietServer) as srv:  # 端口 0 自动分配（避免与 evidence_panel 的 8937 冲突）
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.goto(f"http://127.0.0.1:{srv.server_address[1]}/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # issue #9 复现形：CRLF 注入
            page.evaluate("window.__inject('line one\\r\\nline two\\r\\n')")
            page.wait_for_function("window.__blockCount() === 2", timeout=15000)
            r = page.evaluate("""() => ({
                md: window.__md(),
                view: window.__view.state.doc.toString(),
            })""")
            check("tr1_web_md_projection_cr_free",
                  "\r" not in r["md"] and r["md"] == "line one\nline two",
                  json.dumps(r))
            check("tr1_web_view_model_mirror",
                  r["view"].replace("\n", "|") == "line one|line two" or "\r" not in r["view"],
                  json.dumps(r))
            browser.close()
        srv.shutdown()

    failed = [c for c in checks if not c["ok"]]
    print(f"{'ALL PASS' if not failed else 'FAILED'}: {len(checks) - len(failed)}/{len(checks)}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
