#!/usr/bin/env python3
"""t16 DOCX import e2e (office-io -> mdc-wasm -> web editor, first office
consumption path through the wasm bridge): serve web/dist, drive the real
file-input UI path + programmatic window.__importDocx, assert happy-path
structure mapping, typed error surfacing on hostile/truncated input,
live-editor editability of the imported doc, and contract surface intact.
Fixture = web/tests/fixtures/import_sample.docx (provenance: office-io
DocxWriter via `cargo run -p mdcore-wasm --example gen_docx_fixture`).
Evidence -> results/results_docx.json
"""

import base64
import http.server
import json
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURE = WEB / "tests" / "fixtures" / "import_sample.docx"
OUT = WEB / "tests" / "results" / "results_docx.json"

# 导入后期望的编辑器文档：核心树规范形（to_markdown 按行 join——空行
# 在 ingestion 时被跳过，块间无空行；导入串的 \n\n 分隔是宿主可读性面，
# 编辑器 canonical 化后即单行分隔）。
EXPECTED_MD = (
    "# Imported Title\n"
    "plain **bold** and *italic* and [docs](https://example.com/docs)\n"
    "| Name | Value |\n"
    "| --- | --- |\n"
    "| alpha | 1 |"
)

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True

# 页内字节导入助手（base64 → Uint8Array → window.__importDocx）
IMPORT_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__importDocx(bytes);
}"""


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    fixture_bytes = FIXTURE.read_bytes()
    fixture_b64 = base64.b64encode(fixture_bytes).decode()

    with socketserver.TCPServer(("127.0.0.1", 8933), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8933/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- UI 面在场（启动零回归：新模块不破坏 boot）----
            ui = page.evaluate("""() => ({
                file: !!document.getElementById('docx-file'),
                status: !!document.getElementById('docx-status'),
                importFn: typeof window.__importDocx,
                contract: window.__contractVersion,
            })""")
            check("import_ui_present",
                  ui["file"] and ui["status"] and ui["importFn"] == "function" and ui["contract"] == 1.1,
                  json.dumps(ui))

            # ---- 快乐路径：真实 file input（Playwright set_input_files
            #      触发原生 change → arrayBuffer → wasm import → __inject 重建）----
            page.set_input_files("#docx-file", str(FIXTURE))
            page.wait_for_function("window.__lastDocxImport && window.__lastDocxImport.ok === true",
                                   timeout=15000)
            md = page.evaluate("window.__md()")
            check("happy_import_file_input_exact_markdown", md == EXPECTED_MD, repr(md[:160]))
            blocks = page.evaluate("window.__blocks().map((b) => b.type)")
            check("import_block_types",
                  blocks == ["heading", "paragraph", "table"], json.dumps(blocks))

            # ---- stats 面（typed 摘要 + 不导入面计数显式化）----
            stats = page.evaluate("window.__lastDocxImport.stats")
            status_text = page.evaluate("document.getElementById('docx-status').textContent")
            check("import_stats_and_status_line",
                  stats.get("headings") == 1 and stats.get("paragraphs") == 1
                  and stats.get("tables") == 1 and stats.get("hyperlinks") == 1
                  and stats.get("images_skipped") == 1 and stats.get("headers_skipped") == 1
                  and stats.get("footers_skipped") == 1
                  and status_text.startswith("imported:")
                  and "1 table" in status_text and "image skipped" in status_text,
                  json.dumps({"stats": stats, "status": status_text}))

            # ---- 导入文档是活编辑器文档（全桥：op/sync/事件管道）----
            page.evaluate("window.__view.dispatch({ changes: { from: window.__view.state.doc.length, insert: '\\ntyped tail' } })")
            page.wait_for_function("window.__blockCount() === 4", timeout=15000)
            md_live = page.evaluate("window.__md()")
            check("imported_doc_is_live_editor",
                  "typed tail" in md_live and md_live.startswith("# Imported Title"),
                  repr(md_live[-80:]))

            # ---- undo 键位契约在导入文档上仍成立 ----
            undo_ret = page.evaluate("window.__bridge.undo()")
            page.wait_for_function("!window.__md().includes('typed tail')", timeout=15000)
            md_undo = page.evaluate("window.__md()")
            check("undo_works_after_import", undo_ret is True and "typed tail" not in md_undo,
                  json.dumps({"ret": undo_ret, "tail": md_undo[-60:]}))

            # ---- 敌意面：垃圾字节 → typed 错误码 + 编辑器现状零触碰 ----
            md_before_hostile = page.evaluate("window.__md()")
            garbage = base64.b64encode(bytes([0x00, 0x01, 0x02, 0x63, 0xFF])).decode()
            hostile = page.evaluate(IMPORT_JS, garbage)
            hostile_status = page.evaluate("document.getElementById('docx-status').textContent")
            check("hostile_garbage_typed_error",
                  hostile.get("ok") is False and hostile.get("code") == "container_corrupt"
                  and bool(hostile.get("message"))
                  and "container_corrupt" in hostile_status
                  and page.evaluate("window.__md()") == md_before_hostile,
                  json.dumps({"r": hostile, "status": hostile_status}))

            # ---- 敌意面：截断档案（EOCD 缺失）→ 同 typed 面 ----
            truncated = base64.b64encode(fixture_bytes[: len(fixture_bytes) // 2]).decode()
            trunc = page.evaluate(IMPORT_JS, truncated)
            check("truncated_zip_typed_error",
                  trunc.get("ok") is False and trunc.get("code") == "container_corrupt"
                  and page.evaluate("window.__ready === true")
                  and page.evaluate("window.__md()") == md_before_hostile,
                  json.dumps({k: trunc.get(k) for k in ("ok", "code")}))

            # ---- 契约面 + 零页错（启动/导入全程无 JS 异常逃逸）----
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                contract: window.__contractVersion === 1.1,
                inject: typeof window.__inject === 'function',
                md: window.__md(),
            })""")
            check("contract_intact_no_pageerror",
                  final["ready"] and final["contract"] and final["inject"]
                  and final["md"] == EXPECTED_MD and len(page_errors) == 0,
                  json.dumps({"final": {k: final[k] for k in ("ready", "contract", "inject")},
                              "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "t16 docx import e2e", "dist": str(WEB / "dist"), "fixture": str(FIXTURE),
                 "fixture_bytes": len(fixture_bytes)},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
