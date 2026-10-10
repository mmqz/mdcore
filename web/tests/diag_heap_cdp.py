#!/usr/bin/env python3
"""R19 one-off: CDP allocation sampling during 10MB progressive ingest.
Attributes the ~500MB ingest-phase heap growth to allocation sites."""
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


def flatten(node, out, path):
    frame = node.get("callFrame", {})
    name = frame.get("functionName") or "(anon)"
    url = (frame.get("url") or "").split("/")[-1]
    key = f"{name}@{url}"
    out.setdefault(key, 0)
    out[key] += node.get("selfSize", 0) or 0
    for c in node.get("children", []):
        flatten(c, out, path + [key])


def main():
    os.chdir(WEB / "dist")
    with Server(("127.0.0.1", 8799), Quiet) as httpd:
        t = threading.Thread(target=httpd.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as p:
            browser = p.chromium.launch(args=["--js-flags=--expose-gc", "--enable-precise-memory-info"])
            page = browser.new_page()
            client = page.context.new_cdp_session(page)
            client.send("HeapProfiler.startSampling", {"samplingInterval": 131072})  # 128KB
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto("http://127.0.0.1:8799/index.html?doc=big&kb=10240&chunks=16")
            page.wait_for_function("window.__ready === true", timeout=180_000)
            prof = client.send("HeapProfiler.stopSampling")
            # GC 稳态
            page.evaluate("() => { for (let i=0;i<3;i++) window.gc(); }")
            steady = page.evaluate("() => Math.round(performance.memory.usedJSHeapSize / 1048576)")
            browser.close()
    agg = {}
    flatten(prof["profile"]["head"], agg, [])
    total = sum(agg.values())
    rows = sorted(agg.items(), key=lambda kv: -kv[1])[:22]
    print(f"total sampled allocation: {total / 1048576:.1f}MB; GC steady: {steady}MB")
    for k, v in rows:
        print(f"{v / 1048576:8.1f}MB  {k}")
    print("errors:", errors[:3])


if __name__ == "__main__":
    main()
