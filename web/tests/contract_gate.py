#!/usr/bin/env python3
"""R23-d contract gate: serve web/dist, assert the frozen host<->bridge<->core
contract (CONTRACT.md v1) on a live page — boot surface, host inject path
(__inject / __inject_chunk / mdc-inject* event family), __bridge inventory,
undo key-contract, unit-constitution roundtrip. Evidence -> results_contract.json
"""

import http.server
import json
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_contract.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（web_smoke 同款口径）
socketserver.TCPServer.allow_reuse_address = True


def main() -> int:
    checks = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8931), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: check("pageerror", False, str(e)))
            page.goto("http://127.0.0.1:8931/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- §3.1 启动面 ----
            check("ready", page.evaluate("window.__ready === true"))
            check("contract_version", page.evaluate("window.__contractVersion === 1"))
            surf = page.evaluate("""() => ({
                inject: typeof window.__inject,
                injectChunk: typeof window.__inject_chunk,
                md: typeof window.__md,
                blocks: typeof window.__blocks,
                blockCount: typeof window.__blockCount,
                memStats: typeof window.__memStats,
                events: Array.isArray(window.__mdcoreEvents),
                perf: typeof window.__perf,
                core: typeof window.__core,
            })""")
            check("boot_surface_types",
                  all(v == "function" for k, v in surf.items() if k not in ("events", "core", "perf"))
                  and surf["events"] and surf["core"] == "object" and surf["perf"] == "object", json.dumps(surf))

            # ---- §3.3 __bridge 清单 ----
            br = page.evaluate("""() => ({
                pushRender: typeof window.__bridge.pushRender,
                locked: typeof window.__bridge.locked,
                undo: typeof window.__bridge.undo,
                redo: typeof window.__bridge.redo,
                computeDecorations: typeof window.__bridge.computeDecorations,
                dumpDecorations: typeof window.__bridge.dumpDecorations,
                setSelections: typeof window.__bridge.setSelections,
                debugMapVsFullRebuild: typeof window.__bridge.debugMapVsFullRebuild,
                utf16ToCp: typeof window.__bridge.unit.utf16ToCp,
                cpToUtf16: typeof window.__bridge.unit.cpToUtf16,
            })""")
            check("bridge_inventory", all(v == "function" for k, v in br.items() if k != "locked")
                  and br["locked"] == "boolean", json.dumps(br))

            # ---- §3.2 注入面：事件族 + 全量重建 ----
            page.evaluate("""() => {
                window.__spy = { inject: 0, chunk: 0 };
                window.addEventListener('mdc-inject', () => window.__spy.inject++);
                window.addEventListener('mdc-inject-chunk', () => window.__spy.chunk++);
            }""")
            page.evaluate("window.__inject('# Host Doc\\n\\nHost body paragraph.\\n\\n')")
            page.wait_for_function("window.__blockCount() === 2", timeout=15000)
            md1 = page.evaluate("window.__md()")
            check("inject_rebuild", "Host body paragraph." in md1 and "# Host Doc" in md1, md1[:80])
            check("inject_event_fired", page.evaluate("window.__spy.inject === 1"))
            # 排空时机契约：事件在 pushRender 同步时 drain（R11 桥语义），
            # 重建后先验起点清零，再验渲染后 ingest 事件可见
            check("inject_event_stream_fresh", page.evaluate("window.__mdcoreEvents.length === 0"))
            # 重建核的初始 ingest 事件由构造器首渲消费（与 boot 同语义）；
            # 编辑产生的 op 事件在渲染同步时到达宿主数组（R11 排空契约正测面）
            page.evaluate("window.__view.dispatch({ changes: { from: 0, insert: 'Z' } })")
            page.evaluate("window.__bridge.pushRender(undefined, { force: true })")
            check("inject_events_drain_on_render", page.evaluate("window.__mdcoreEvents.length > 0"))
            check("inject_keeps_surface",
                  page.evaluate("typeof window.__memStats === 'function' && window.__contractVersion === 1"))

            # ---- §3.2 渐进通道 ----
            page.evaluate("window.__inject_chunk('first chunk\\n\\n', true)")
            page.wait_for_function("window.__blockCount() === 1", timeout=15000)
            page.evaluate("window.__inject_chunk('second chunk tail\\n\\n', false)")
            page.wait_for_function("window.__blockCount() === 2", timeout=15000)
            md2 = page.evaluate("window.__md()")
            check("chunk_progressive", "first chunk" in md2 and "second chunk tail" in md2, md2[:120])
            check("chunk_event_fired", page.evaluate("window.__spy.chunk === 2"))

            # ---- §3.3 undo 键位契约（R21 勘误口径）----
            undo_ret = page.evaluate("window.__bridge.undo()")
            check("undo_key_contract", undo_ret is True, repr(undo_ret))

            # ---- §3.3 单位宪法换算出口 ----
            unit = page.evaluate("""() => {
                const s = 'a\\u{1F389}b';
                return { u2c: window.__bridge.unit.utf16ToCp(s, 3), c2u: window.__bridge.unit.cpToUtf16(s, 2) };
            }""")
            check("unit_constitution", unit["u2c"] == 2 and unit["c2u"] == 3, json.dumps(unit))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "R23-d contract v1", "dist": str(WEB / "dist")},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed, "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
