#!/usr/bin/env python3
"""One-off probe: attribute Gate 2 selection-only dispatch cost (R15).
Serves web/dist, loads the 1MB big doc, runs 30 selection-only dispatches,
reports per-dispatch times + accumulated buildDecorations/filterAtomic probe."""
import http.server
import socketserver
import threading
import sys
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
    import os
    os.chdir(WEB / "dist")
    with Server(("127.0.0.1", 8791), Quiet) as httpd:
        t = threading.Thread(target=httpd.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as p:
            browser = p.chromium.launch()
            page = browser.new_page()
            page.goto("http://127.0.0.1:8791/index.html?doc=big&kb=1024")
            page.wait_for_function("window.__ready === true", timeout=60_000)
            r = page.evaluate("""() => {
              const view = window.__view;
              const p95 = (a) => { const s = [...a].sort((x, y) => x - y);
                                   return s[Math.min(s.length - 1, Math.floor(s.length * 0.95))]; };
              const docLen = view.state.doc.length;
              const times = [];
              for (let i = 0; i < 30; i++) {
                const t = performance.now();
                view.dispatch({selection: {anchor: Math.floor(docLen * (i % 10) / 10)}});
                times.push(performance.now() - t);
              }
              return {times, p95: p95(times), max: Math.max(...times),
                      deco: window.__decoProbe ?? null, atomic: window.__atomicProbe ?? null,
                      lines: view.state.doc.lines};
            }""")
            # R19：视口外原子区补算验证——程序化跳到 1MB 文档中部 heading
            # marker（无滚动，DECO_PAD 窗口外），ArrowLeft 应原子跳过整段标记。
            gap = page.evaluate("""() => {
              const view = window.__view;
              const doc = view.state.doc;
              // 找视口外 heading：从视口起点行 +300 行处向下扫（无滚动 dispatch，
              // viewport 保持文档开头——目标必在 DECO_PAD 窗口外）。
              const vpLine = doc.lineAt(view.viewport.from).number;
              const start = Math.min(doc.lines, vpLine + 300);
              let anchor = null, ln = 0;
              for (let i = start; i <= doc.lines; i++) {
                const m = /^#{1,6} /.exec(doc.line(i).text);
                if (m) { anchor = doc.line(i).from + m[0].length; ln = i; break; }
              }
              if (anchor === null) return {skipped: 'no heading found below viewport'};
              view.dispatch({selection: {anchor}});
              const before = view.state.selection.main.anchor;
              view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowLeft', bubbles: true, cancelable: true}));
              const after = view.state.selection.main.anchor;
              const lineStart = doc.line(ln).from;
              return {line: ln, vpLine, before, after, lineStart, skippedAtomic: after === lineStart};
            }""")
            browser.close()
    times = r.pop("times")
    print("doc lines:", r["lines"])
    print("probe:", r)
    print("per-dispatch ms:", [round(x, 1) for x in times])
    print("gap-check (R19 out-of-viewport atomic):", gap)


if __name__ == "__main__":
    sys.exit(main())
