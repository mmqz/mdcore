#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""#3 UI 批 E：表面空态引导卡片（empty_state）.

issue #3 残差收口验收：PDF / Grid / Canvas 三表面空态从「整块空白
（视觉像加载失败）」改为居中引导卡片（图标 + 标题 + 引导行）。本套
件锁三面：

  A. 就绪面（1-4）：三卡片在场（#pdf-guide/#grid-guide/#canvas-guide
     各含 .sg-card + 标题 + 引导行 + SVG 图标）· 空态可见（无 hidden
     属性）· DOM 契约零触碰（既有面板/视口 id 全在场——21 套件约束）
     · 卡片真实几何（active tab 内非零尺寸 + 面板边界内居中 + 主题
     变量消费非透明底盘）。
  B. 到达隐藏（5-8）：grid 开簿（__gridOpenBytes 成功）→ #grid-guide
     hidden · pdf 打开（__pdfOpenBytes 成功）→ #pdf-guide hidden ·
     canvas 加节点（__canvasAddNode）→ #canvas-guide hidden。
  C. 归零回显（9-10）：canvas clear（对象清零）→ 卡片回显 ·
     pdf 换开失败（垃圾字节 → teardown 后 doc=null）→ 卡片回显。
  D. 收尾（11）：全程零 pageerror。

夹具：grid_sample.xlsx / sample.pdf（tests/fixtures，缺件自动生成——
既有套件同源）。语言注记：卡片文案英文（UI 既有文案语言一致性——
top bar/status/hints 全英文面）。
"""

import base64
import http.server
import json
import socketserver
import subprocess
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_empty_state.json"
GRID_FIXTURE = WEB / "tests" / "fixtures" / "grid_sample.xlsx"
GRID_GEN = ["cargo", "run", "-p", "mdcore-wasm", "--example", "gen_xlsx_grid_fixture"]
PDF_FIXTURE = WEB / "tests" / "fixtures" / "sample.pdf"
PDF_GEN = ["python3", "tests/gen_pdf_fixture.py"]

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update(
    {".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm",
     ".mjs": "text/javascript"}
)


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True


def main() -> int:
    if not GRID_FIXTURE.exists():
        subprocess.run(GRID_GEN, check=True, cwd=str(WEB))
    if not PDF_FIXTURE.exists():
        subprocess.run(PDF_GEN, check=True, cwd=str(WEB))
    grid_b64 = base64.b64encode(GRID_FIXTURE.read_bytes()).decode()
    pdf_b64 = base64.b64encode(PDF_FIXTURE.read_bytes()).decode()

    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name
              + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8941), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8941/index.html")
            page.wait_for_function("window.__ready === true", timeout=60_000)
            # PDF 面异步惰性引导（setupPdfView 自举——__ready 后仍需
            # __pdfReady 收敛，否则 guide 挂载竞态）。
            page.wait_for_function("window.__pdfReady === true", timeout=30_000)

            # ---- A. 就绪面 ----
            boot = page.evaluate("""() => {
              const guides = ['pdf-guide', 'grid-guide', 'canvas-guide'].map((id) => {
                const g = document.getElementById(id);
                const card = g?.querySelector('.sg-card');
                return {
                  id, present: !!g,
                  hidden: g?.hidden ?? null,
                  card: !!card,
                  title: card?.querySelector('.sg-title')?.textContent ?? null,
                  hints: card ? card.querySelectorAll('.sg-hint').length : 0,
                  icon: !!card?.querySelector('svg.sg-icon path'),
                };
              });
              return {
                guides,
                contract: {
                  editor: !!document.getElementById('editor'),
                  viewTabs: !!document.getElementById('view-tabs'),
                  evidence: !!document.getElementById('evidence-panel'),
                  pdfPanel: !!document.getElementById('pdf-panel'),
                  gridPanel: !!document.getElementById('grid-panel'),
                  canvasPanel: !!document.getElementById('canvas-panel'),
                  pdfViewport: !!document.getElementById('pdf-viewport'),
                  gridViewport: !!document.getElementById('grid-viewport'),
                  canvasScroll: !!document.getElementById('canvas-scroll'),
                },
              };
            }""")
            gs = {g["id"]: g for g in boot["guides"]}
            # canvas 面板惰性创建（首点 #tab-canvas 才挂——既有架构）
            #：boot 面断 pdf/grid 两卡，canvas 卡归 A5（首激活后、
            # 加节点前仍是空态）。
            check("A1 pdf/grid guides mounted at boot with card/title/hints/icon",
                  all(gs[i]["present"] and gs[i]["card"] and gs[i]["title"]
                      and gs[i]["hints"] >= 1 and gs[i]["icon"]
                      for i in ("pdf-guide", "grid-guide")),
                  json.dumps(boot["guides"], ensure_ascii=False))
            check("A2 pdf/grid guides visible while surfaces empty (no hidden attr)",
                  all(gs[i]["hidden"] is False for i in ("pdf-guide", "grid-guide")),
                  json.dumps([(g["id"], g["hidden"]) for g in boot["guides"]]))
            check("A3 DOM contract intact at boot (canvas 面惰性——scroll 归 A5)",
                  all(v for k, v in boot["contract"].items() if k != "canvasScroll"),
                  json.dumps(boot["contract"]))

            # 卡片真实几何 + 主题消费（grid tab 激活态下量测）
            page.click("#tab-grid")
            page.wait_for_timeout(120)
            geo = page.evaluate("""() => {
              const g = document.getElementById('grid-guide');
              const card = g.querySelector('.sg-card');
              const panel = document.getElementById('grid-panel');
              const cr = card.getBoundingClientRect();
              const pr = panel.getBoundingClientRect();
              const cs = getComputedStyle(card);
              return {
                cardW: cr.width, cardH: cr.height,
                inPanel: cr.left >= pr.left && cr.right <= pr.right
                      && cr.top >= pr.top && cr.bottom <= pr.bottom,
                roughlyCentered: Math.abs((cr.left + cr.right) / 2 - (pr.left + pr.right) / 2) < 80,
                bg: cs.backgroundColor, borderW: cs.borderTopWidth,
                pointerEvents: getComputedStyle(g).pointerEvents,
              };
            }""")
            check("A4 card geometry: sized, inside panel, centered, themed, non-blocking",
                  geo["cardW"] > 120 and geo["cardH"] > 60 and geo["inPanel"]
                  and geo["roughlyCentered"] and geo["bg"] not in ("rgba(0, 0, 0, 0)", "")
                  and geo["borderW"] != "0px" and geo["pointerEvents"] == "none",
                  json.dumps(geo))

            # ---- B. 到达隐藏 ----
            page.click("#tab-canvas")
            page.wait_for_function("window.__canvasReady === true", timeout=30_000)
            lazy = page.evaluate("""() => ({
              scroll: !!document.getElementById('canvas-scroll'),
              guide: !!document.getElementById('canvas-guide'),
              hidden: document.getElementById('canvas-guide')?.hidden ?? null,
            })""")
            check("A5 canvas lazy panel: guide mounted + visible on first activation (empty)",
                  lazy["scroll"] and lazy["guide"] and lazy["hidden"] is False,
                  json.dumps(lazy))
            page.evaluate("window.__canvasAddNode(120, 100, 'seed node')")
            page.wait_for_timeout(80)
            ch = page.evaluate("document.getElementById('canvas-guide').hidden")
            st = page.evaluate("window.__canvasState()")
            check("B3 canvas add node hides guide",
                  ch is True and st.get("objects", 1) >= 1,
                  json.dumps({"hidden": ch, "state": st}))

            r1 = page.evaluate("(b64) => window.__gridOpenBytes(b64)", grid_b64)
            gh1 = page.evaluate(
                "document.getElementById('grid-guide').hidden")
            check("B1 grid open hides guide", r1.get("ok") is True and gh1 is True,
                  json.dumps({"open": r1, "hidden": gh1}))

            page.click("#tab-pdf")
            page.wait_for_function("window.__pdfReady === true", timeout=30_000)
            r2 = page.evaluate("(b64) => window.__pdfOpenBytes(b64)", pdf_b64)
            ph = page.evaluate("document.getElementById('pdf-guide').hidden")
            check("B2 pdf open hides guide",
                  r2.get("ok") is True and ph is True,
                  json.dumps({"open": r2, "hidden": ph}))

            # ---- C. 归零回显 ----
            page.click("#tab-canvas")
            page.wait_for_timeout(80)
            page.click("#canvas-clear-btn")
            page.wait_for_timeout(120)
            ch2 = page.evaluate("document.getElementById('canvas-guide').hidden")
            st2 = page.evaluate("window.__canvasState()")
            check("C1 canvas clear shows guide again",
                  ch2 is False and st2.get("objects", -1) == 0,
                  json.dumps({"hidden": ch2, "state": st2}))

            page.click("#tab-pdf")
            bad = base64.b64encode(b"not a pdf at all").decode()
            r5 = page.evaluate("(b64) => window.__pdfOpenBytes(b64)", bad)
            ph5 = page.evaluate("document.getElementById('pdf-guide').hidden")
            check("C2 pdf failed re-open shows guide again (teardown → doc null)",
                  r5.get("ok") is False and ph5 is False,
                  json.dumps({"open": r5, "hidden": ph5}))

            # ---- D. 收尾 ----
            check("D1 zero page errors", not page_errors,
                  "; ".join(page_errors[:3]))
            page.close()
            browser.close()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps(
        {"meta": {"browser": "chromium-headless", "runner": "playwright",
                  "page": "web/dist/index.html", "suite": "empty_state (#3 UI batch E)"},
         "tests": checks,
         "summary": {"total": len(checks), "failed": len(failed),
                     "pass": len(failed) == 0}},
        ensure_ascii=False, indent=1))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
