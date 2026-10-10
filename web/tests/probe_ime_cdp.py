#!/usr/bin/env python3
"""R23-a probe (fixed): CDP real IME stream against the CM6 <-> mdcore-wasm page.

Param shape confirmed on chromium-1243 (both headless shell and new headless):
  Input.imeSetComposition {text, selectionStart, selectionEnd}
  Input.insertText {text}  -> acts as IME commit (compositionend fires)
  empty text               -> acts as cancel (compositionend with data='')
Observation only — feeds ime_cdp.py assertion design.
"""

import json
import os
import socketserver
import http.server
import sys
import threading
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def log_message(self, *a):
        pass


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


INSTALL_LOG = """() => {
  window.__imeLog = [];
  const log = (kind, ev) => window.__imeLog.push({
    kind, data: ev.data === undefined ? null : ev.data, inputType: ev.inputType || null });
  const content = document.querySelector('.cm-content');
  for (const type of ['compositionstart', 'compositionupdate', 'compositionend'])
    content.addEventListener(type, (e) => log(type, e));
  content.addEventListener('beforeinput', (e) => log('beforeinput', e));
}"""


def main():
    os.chdir(WEB / "dist")
    httpd = ReusableTCPServer(("127.0.0.1", 0), QuietServer)
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    out = {"steps": []}
    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=["--js-flags=--expose-gc", "--enable-precise-memory-info"])
        page = browser.new_page()
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(f"http://127.0.0.1:{port}/index.html")
        page.wait_for_function("window.__ready === true", timeout=60_000)
        page.evaluate(INSTALL_LOG)
        rev0 = page.evaluate("""() => {
          const v = window.__view;
          const pos = v.state.doc.toString().indexOf('for typing.');
          v.focus();
          v.dispatch({selection: {anchor: pos}});
          return Number(window.__core.revision());
        }""")
        cdp = page.context.new_cdp_session(page)

        def snap(step):
            time.sleep(0.12)
            s = page.evaluate("""() => {
              const doc = window.__view.state.doc.toString();
              const i = doc.indexOf('for typing.');
              const log = window.__imeLog.slice(); window.__imeLog.length = 0;
              return {log, docAround: doc.slice(Math.max(0, i - 8), i + 26),
                      rev: Number(window.__core.revision()),
                      events: window.__mdcoreEvents.length,
                      composing: window.__view.composing,
                      sel: window.__view.state.selection.main.anchor};
            }""")
            out["steps"].append({"step": step, **s})

        cdp.send("Input.imeSetComposition", {"text": "n", "selectionStart": 1, "selectionEnd": 1})
        snap("setComp 'n'")
        cdp.send("Input.imeSetComposition", {"text": "ni", "selectionStart": 2, "selectionEnd": 2})
        snap("setComp 'ni'")
        cdp.send("Input.imeSetComposition", {"text": "你好", "selectionStart": 2, "selectionEnd": 2})
        snap("setComp '你好' (multi-char preedit)")
        cdp.send("Input.insertText", {"text": "你"})
        snap("insertText '你' (commit)")
        cdp.send("Input.imeSetComposition", {"text": "x", "selectionStart": 1, "selectionEnd": 1})
        snap("setComp 'x' (pre-cancel)")
        cdp.send("Input.imeSetComposition", {"text": "", "selectionStart": 0, "selectionEnd": 0})
        snap("setComp '' (cancel)")

        out["rev0"] = rev0
        out["errors"] = errors
        out["eventsJsonTail"] = page.evaluate(
            "() => JSON.stringify(window.__mdcoreEvents.slice(-10), null, 1)")
        out["coreBlocksTail"] = page.evaluate(
            """() => window.__blocks().filter((b) => b.text && b.text.includes('typing')).map((b) => b.text)""")
        out["historyUndoEntries"] = page.evaluate(
            "() => window.__memStats ? window.__memStats().history.undo_entries : null")
        page.close()
        browser.close()

    outdir = WEB / "tests" / "results"
    outdir.mkdir(exist_ok=True)
    (outdir / "probe_ime.json").write_text(json.dumps(out, indent=2, ensure_ascii=False))
    for s in out["steps"]:
        print(json.dumps(s, ensure_ascii=False))
    print("events tail:", out["eventsJsonTail"][:1200])
    print("core blocks tail:", out["coreBlocksTail"])
    print("undo entries:", out["historyUndoEntries"], "errors:", out["errors"])


if __name__ == "__main__":
    sys.exit(main())
