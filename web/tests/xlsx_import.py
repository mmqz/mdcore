#!/usr/bin/env python3
"""t17 XLSX import e2e (office-io read face -> mdc-wasm -> web editor, the
xlsx consumption slice mirroring t16): serve web/dist, drive the real
file-input UI path + programmatic window.__importXlsx, assert happy-path
sheet->GFM-table mapping, typed error surfacing on hostile/truncated input,
live-editor editability of the imported doc, and contract surface intact.
Fixture = web/tests/fixtures/import_sample.xlsx (provenance: office-io
WorkbookWriter via `cargo run -p mdcore-wasm --example gen_xlsx_fixture`).
Evidence -> results/results_xlsx.json
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
FIXTURE = WEB / "tests" / "fixtures" / "import_sample.xlsx"
OUT = WEB / "tests" / "results" / "results_xlsx.json"

# 导入后期望的编辑器文档：核心树规范形（to_markdown 按行 join——空行
# 在 ingestion 时被跳过，块间无空行；导入串的 \n\n 分隔是宿主可读性面，
# 编辑器 canonical 化后即单行分隔）。
EXPECTED_MD = (
    "## Data\n"
    "| Item | Qty | Note |\n"
    "| --- | --- | --- |\n"
    "| Widget | 4.5 | has \\| pipe |\n"
    "| Gadget | `=SUM(B2:B2)*2`=9 | [docs](https://example.com/docs) |\n"
    "| merged top |  |  |\n"
    "## Notes\n"
    "| Key | Value |\n"
    "| --- | --- |\n"
    "| k1 | true |\n"
    "## Empty\n"
    "(empty sheet)"
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

# 页内字节导入助手（base64 → Uint8Array → window.__importXlsx）
IMPORT_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__importXlsx(bytes);
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

    with socketserver.TCPServer(("127.0.0.1", 8934), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8934/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- UI 面在场（启动零回归：新模块不破坏 boot；t16 docx 面并存）----
            ui = page.evaluate("""() => ({
                file: !!document.getElementById('xlsx-file'),
                status: !!document.getElementById('xlsx-status'),
                importFn: typeof window.__importXlsx,
                docxFn: typeof window.__importDocx,
                contract: window.__contractVersion,
            })""")
            check("import_ui_present",
                  ui["file"] and ui["status"] and ui["importFn"] == "function"
                  and ui["docxFn"] == "function" and ui["contract"] == 1.4,
                  json.dumps(ui))

            # ---- 快乐路径：真实 file input（Playwright set_input_files
            #      触发原生 change → arrayBuffer → wasm import → __inject 重建）----
            page.set_input_files("#xlsx-file", str(FIXTURE))
            page.wait_for_function("window.__lastXlsxImport && window.__lastXlsxImport.ok === true",
                                   timeout=15000)
            md = page.evaluate("window.__md()")
            check("happy_import_file_input_exact_markdown", md == EXPECTED_MD, repr(md[:200]))
            blocks = page.evaluate("window.__blocks().map((b) => b.type)")
            check("import_block_types",
                  blocks == ["heading", "table", "heading", "table", "heading", "paragraph"],
                  json.dumps(blocks))

            # ---- stats 面（typed 摘要 + 丢弃面计数显式化）----
            stats = page.evaluate("window.__lastXlsxImport.stats")
            status_text = page.evaluate("document.getElementById('xlsx-status').textContent")
            check("import_stats_and_status_line",
                  stats.get("sheets") == 3 and stats.get("tables") == 2
                  and stats.get("empty_sheets") == 1 and stats.get("rows") == 6
                  and stats.get("cells") == 14
                  # P2-3（#22）：双通道接入——imported 计数 + 越限丢弃恒 0
                  #（正常表）+ 状态行展示接入计数。
                  and stats.get("formulas_imported") == 1
                  and stats.get("hyperlinks_imported") == 1
                  and stats.get("formulas_flattened") == 0
                  and stats.get("merged_ranges") == 1
                  and stats.get("hyperlinks_dropped") == 0
                  and stats.get("sheets_dropped") == 0
                  and (stats.get("formulas") or [{}])[0].get("cached") == "9"
                  and (stats.get("hyperlinks") or [{}])[0].get("target") == "https://example.com/docs"
                  and status_text.startswith("imported:")
                  and "2 tables" in status_text and "1 formula kept" in status_text
                  and "1 link kept" in status_text and "1 merge" in status_text
                  # P2-4（#22）：merged_ranges 结构登记 → info 档可见警告。
                  and "1 warning" in status_text
                  and any(w.get("kind") == "merged_ranges" and w.get("severity") == "info"
                          and w.get("count") == 1 and w.get("part") == "xlsx"
                          for w in (stats.get("warnings") or [])),
                  json.dumps({"stats": stats, "status": status_text}))

            # ---- 导入文档是活编辑器文档（全桥：op/sync/事件管道）----
            page.evaluate("window.__view.dispatch({ changes: { from: window.__view.state.doc.length, insert: '\\ntyped tail' } })")
            page.wait_for_function("window.__blockCount() === 7", timeout=15000)
            md_live = page.evaluate("window.__md()")
            check("imported_doc_is_live_editor",
                  "typed tail" in md_live and md_live.startswith("## Data"),
                  repr(md_live[-80:]))

            # ---- undo 键位契约在导入文档上仍成立 ----
            undo_ret = page.evaluate("window.__bridge.undo()")
            page.wait_for_function("!window.__md().includes('typed tail')", timeout=15000)
            md_undo = page.evaluate("window.__md()")
            check("undo_works_after_import", undo_ret is True and "typed tail" not in md_undo,
                  json.dumps({"ret": undo_ret, "tail": md_undo[-60:]}))

            # ---- #42 方案 A：顶部导入联动 Grid 会话（同份 bytes 两面呈现）----
            page.wait_for_function(
                "window.__lastXlsxImport && window.__lastXlsxImport.grid"
                " && window.__lastXlsxImport.grid.ok === true", timeout=15000)
            grid_link = page.evaluate("""() => ({
                ready: window.__gridReady === true,
                sheets: (window.__gridSheets() || []).map((s) => s.name),
                status: document.getElementById('xlsx-status').textContent,
            })""")
            check("top_import_links_grid_session",
                  grid_link["ready"] and grid_link["sheets"] == ["Data", "Notes", "Empty"]
                  and "workbook in grid (3 sheets)" in grid_link["status"],
                  json.dumps(grid_link))

            # ---- #42 验收：切到 Grid 面板能看到该工作簿（真实 UI 点击路径）----
            page.click("#tab-grid")
            grid_view = page.evaluate("""() => ({
                panelVisible: document.getElementById('grid-panel').style.display === 'block',
                editorHidden: document.getElementById('editor').style.display === 'none',
                guideHidden: document.getElementById('grid-guide').hidden === true,
                status: document.getElementById('grid-status').textContent,
                sheetOptions: Array.from(document.querySelectorAll('#grid-sheet-select option'))
                    .map((o) => o.textContent),
            })""")
            check("grid_panel_shows_workbook_after_top_import",
                  grid_view["panelVisible"] and grid_view["editorHidden"]
                  and grid_view["guideHidden"]
                  and "opened: 3 sheets" in grid_view["status"]
                  and grid_view["sheetOptions"] == ["Data", "Notes", "Empty"],
                  json.dumps(grid_view))
            page.click("#tab-doc")

            # ---- #42 验收：两条路径导入同一文件 → 内容一致（sheets + 首表切片全等）----
            path_a = page.evaluate("""() => ({
                sheets: window.__gridSheets(),
                slice: window.__gridSliceRaw(0, 0, 0, 4, 3),
            })""")
            path_b_open = page.evaluate("(b64) => window.__gridOpenBytes(b64)", fixture_b64)
            path_b = page.evaluate("""() => ({
                sheets: window.__gridSheets(),
                slice: window.__gridSliceRaw(0, 0, 0, 4, 3),
            })""")
            check("both_paths_same_workbook_content",
                  path_b_open.get("ok") is True
                  and path_a["sheets"] == path_b["sheets"]
                  and path_a["slice"] == path_b["slice"],
                  json.dumps({"a": path_a["sheets"], "b": path_b["sheets"]})[:300])

            # ---- 敌意面：垃圾字节 → typed 错误码 + 编辑器现状零触碰 ----
            md_before_hostile = page.evaluate("window.__md()")
            grid_before_hostile = page.evaluate("window.__gridSheets()")
            garbage = base64.b64encode(bytes([0x00, 0x01, 0x02, 0x63, 0xFF])).decode()
            hostile = page.evaluate(IMPORT_JS, garbage)
            hostile_status = page.evaluate("document.getElementById('xlsx-status').textContent")
            check("hostile_garbage_typed_error",
                  hostile.get("ok") is False and hostile.get("code") == "container_corrupt"
                  and bool(hostile.get("message"))
                  and "container_corrupt" in hostile_status
                  and page.evaluate("window.__md()") == md_before_hostile
                  # #42：失败导入不联动（grid 会话原样保留——联动仅成功路径）
                  and page.evaluate("window.__gridSheets()") == grid_before_hostile,
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
                contract: window.__contractVersion === 1.4,
                inject: typeof window.__inject === 'function',
                docx: typeof window.__importDocx === 'function',
                md: window.__md(),
            })""")
            check("contract_intact_no_pageerror",
                  final["ready"] and final["contract"] and final["inject"] and final["docx"]
                  and final["md"] == EXPECTED_MD and len(page_errors) == 0,
                  json.dumps({"final": {k: final[k] for k in ("ready", "contract", "inject", "docx")},
                              "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "t17 xlsx import e2e", "dist": str(WEB / "dist"), "fixture": str(FIXTURE),
                 "fixture_bytes": len(fixture_bytes)},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
