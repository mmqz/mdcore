#!/usr/bin/env python3
"""t19 XLSX export e2e (editor document -> office-io WorkbookWriter write
face -> .xlsx bytes download, closing the XLSX round-trip with t17 and the
office consumption trilogy t16/t17/t18): serve web/dist, drive the real
export button + programmatic window.__exportXlsx, assert zip validity
([Content_Types].xml / xl/workbook.xml / xl/worksheets/sheet1.xml),
workbook.xml sheet-name policy evidence (Document + header-derived),
string-level content evidence (sharedStrings aggregate + styles.xml bold),
degraded-block tolerance, empty-document minimal workbook, editor state
untouched by export (read-only), and full circle export -> __importXlsx ->
canonical markdown compare.
Evidence -> results/results_xlsx_export.json
"""

import base64
import http.server
import io
import json
import socketserver
import sys
import threading
import zipfile
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_xlsx_export.json"

# 快乐路径文档：标题 + 段落 + 表（heading→Document 表加粗行 / 表块→
# 独立 sheet 全覆盖；表名 = 表头 A1 "alpha"）。
EXPORT_MD = (
    "# Export Test\n"
    "plain paragraph text\n"
    "| alpha | beta |\n"
    "| --- | --- |\n"
    "| 1 | 2 |"
)

# 降级面文档：code block / blockquote / list（嵌套）/ divider——线性块
# 全部落 Document 表平铺行（xlsx_export.rs 模块文档映射表登记）。
DEGRADED_MD = (
    "```\n"
    "let x = 1;\n"
    "```\n"
    "> quoted line\n"
    "- list item\n"
    "  - nested\n"
    "---"
)

# 全环文档：仅表块（无 Document 表、无文本混排）——导出→t17 导入在
# 表数据面上应逐格等值（all-Text 值面 Display 往返）。
FULLCIRCLE_MD = (
    "| alpha | beta |\n"
    "| --- | --- |\n"
    "| 1 | 2 |"
)

# 全环后期望的编辑器文档：核心树规范形（to_markdown 按行 join——空行
# 在 ingestion 时被跳过；t17 xlsx_import.py EXPECTED_MD 同口径）。
FULLCIRCLE_EXPECTED_MD = (
    "## alpha\n"
    "| alpha | beta |\n"
    "| --- | --- |\n"
    "| 1 | 2 |"
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

# 页内字节导入助手（base64 → Uint8Array → window.__importXlsx）——t17 面
IMPORT_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__importXlsx(bytes);
}"""


def as_bytes(v) -> bytes:
    """page.evaluate 返回的 Uint8Array → python bytes/list 归一。"""
    if isinstance(v, bytes):
        return v
    if isinstance(v, list):
        return bytes(v)
    raise TypeError(f"unexpected bytes shape: {type(v)}")


def zip_part(zf: zipfile.ZipFile, name: str) -> str:
    with zf.open(name) as f:
        return f.read().decode("utf-8")


def all_xml(zf: zipfile.ZipFile) -> str:
    """全部 .xml 部件聚合（sharedStrings 承载字符串——非流式表面）。"""
    return "\n".join(
        zf.read(n).decode("utf-8") for n in zf.namelist() if n.endswith(".xml")
    )


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8936), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8936/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- UI 面在场 + 契约（启动零回归：新模块不破坏 boot）----
            ui = page.evaluate("""() => ({
                btn: !!document.getElementById('xlsx-export-btn'),
                btnLabel: (document.getElementById('xlsx-export-btn') || {}).textContent,
                status: !!document.getElementById('xlsx-export-status'),
                exportFn: typeof window.__exportXlsx,
                last: window.__lastXlsxExport,
                contract: window.__contractVersion,
            })""")
            check("export_ui_present",
                  ui["btn"] and ui["btnLabel"] == "export .xlsx" and ui["status"]
                  and ui["exportFn"] == "function" and ui["last"] is None and ui["contract"] == 1.4,
                  json.dumps(ui))

            # ---- 快乐路径：真实按钮点击（UI 路径）+ 程序化钩子取字节 ----
            page.evaluate("(s) => window.__inject(s)", EXPORT_MD)
            page.wait_for_function("window.__blockCount() === 3", timeout=15000)
            page.click("#xlsx-export-btn")
            page.wait_for_function("window.__lastXlsxExport && window.__lastXlsxExport.ok === true",
                                   timeout=15000)
            r = page.evaluate("window.__exportXlsx()")
            ok_zip = False
            part_names = []
            zf = None
            try:
                raw = as_bytes(r["bytes"])
                zf = zipfile.ZipFile(io.BytesIO(raw))
                part_names = zf.namelist()
                ok_zip = (
                    raw[:2] == b"PK"
                    and "[Content_Types].xml" in part_names
                    and "xl/workbook.xml" in part_names
                    and "xl/worksheets/sheet1.xml" in part_names
                    and zf.testzip() is None
                )
            except Exception as e:  # noqa: BLE001
                ok_zip = False
                r = {"ok": r.get("ok"), "error": str(e)}
            status_text = page.evaluate("document.getElementById('xlsx-export-status').textContent")
            check("happy_export_valid_zip",
                  r.get("ok") is True and ok_zip and r["sizeBytes"] > 400
                  and status_text.startswith("exported:")
                  and "2 sheets" in status_text,
                  json.dumps({"r": {k: r.get(k) for k in ("ok", "sizeBytes")},
                              "parts": part_names, "status": status_text}))

            # ---- workbook.xml 表名策略证据（Document + 表头 A1 派生）----
            wb_xml = zip_part(zf, "xl/workbook.xml")
            check("workbook_xml_sheet_names",
                  'name="Document"' in wb_xml and 'name="alpha"' in wb_xml,
                  wb_xml[:300])

            # ---- 内容字符串级证据（sharedStrings 聚合 + styles.xml 加粗）----
            # 「1」/「2」落在 sharedStrings 字符串表（<t>1</t>）而非数值格——
            # all-Text 类型决策的容器级证据（类型断言主证在原生
            # cell_debug 面）。
            aggregate = all_xml(zf)
            styles_xml = zip_part(zf, "xl/styles.xml")
            shared_xml = zip_part(zf, "xl/sharedStrings.xml")
            check("sheet_content_strings",
                  "Export Test" in aggregate and "plain paragraph text" in aggregate
                  and "alpha" in aggregate and "beta" in aggregate
                  and "<t>1</t>" in shared_xml and "<t>2</t>" in shared_xml
                  and ("<b/>" in styles_xml or "<b " in styles_xml),
                  "sharedStrings/worksheets/styles 聚合串 + heading 加粗面")

            # ---- 降级块不破导出 + 内容在位（线性块 → Document 表平铺行）----
            page.evaluate("(s) => window.__inject(s)", DEGRADED_MD)
            page.wait_for_function("window.__blockCount() === 6", timeout=15000)
            md_before = page.evaluate("window.__md()")
            r2 = page.evaluate("window.__exportXlsx()")
            raw2 = as_bytes(r2["bytes"])
            zf2 = zipfile.ZipFile(io.BytesIO(raw2))
            agg2 = all_xml(zf2)
            degraded_status = page.evaluate("document.getElementById('xlsx-export-status').textContent")
            check("degraded_blocks_survive_export",
                  r2["ok"] is True and "let x = 1;" in agg2
                  and "quoted line" in agg2 and "list item" in agg2 and "nested" in agg2
                  and "1 sheet" in degraded_status,
                  json.dumps({"status": degraded_status, "head": agg2[:120]}))

            # ---- 只读证明：导出前后编辑器状态逐字不变（含块计数）----
            blocks_after = page.evaluate("window.__blockCount()")
            check("readonly_export_state_untouched",
                  page.evaluate("window.__md()") == md_before and blocks_after == 6,
                  "md 字节一致 + 块计数不变")

            # ---- 空文档 = 最小合法工作簿（单空表）----
            page.evaluate("window.__inject('')")
            page.wait_for_function("window.__blockCount() === 0", timeout=15000)
            r3 = page.evaluate("window.__exportXlsx()")
            raw3 = as_bytes(r3["bytes"])
            zf3 = zipfile.ZipFile(io.BytesIO(raw3))
            empty_status = page.evaluate("document.getElementById('xlsx-export-status').textContent")
            check("empty_document_minimal_workbook",
                  r3["ok"] is True and "xl/workbook.xml" in zf3.namelist()
                  and "xl/worksheets/sheet1.xml" in zf3.namelist()
                  and "1 sheet" in empty_status,
                  empty_status)

            # ---- 全环：export → __importXlsx → 规范形 markdown 比对 ----
            #（XLSX 往返闭环 + office 三部曲合拢：表数据逐格等值）。
            page.evaluate("(s) => window.__inject(s)", FULLCIRCLE_MD)
            page.wait_for_function("window.__blockCount() === 1", timeout=15000)
            r4 = page.evaluate("window.__exportXlsx()")
            raw4 = as_bytes(r4["bytes"])
            b64 = base64.b64encode(raw4).decode()
            reimport = page.evaluate(IMPORT_JS, b64)
            reimport_status = page.evaluate("document.getElementById('xlsx-status').textContent")
            md_roundtrip = page.evaluate("window.__md()")
            check("full_circle_reimport_content",
                  reimport.get("ok") is True and md_roundtrip == FULLCIRCLE_EXPECTED_MD
                  and "imported:" in reimport_status,
                  json.dumps({"reimport": {k: reimport.get(k) for k in ("ok", "code")},
                              "md": md_roundtrip[:200], "status": reimport_status}))

            # ---- 契约面 + 零页错（启动/导出/导入全程无 JS 异常逃逸）----
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                contract: window.__contractVersion === 1.4,
                inject: typeof window.__inject === 'function',
                exportXlsx: typeof window.__exportXlsx === 'function',
                exportDocx: typeof window.__exportDocx === 'function',
                importXlsx: typeof window.__importXlsx === 'function',
            })""")
            check("contract_intact_no_pageerror",
                  final["ready"] and final["contract"] and final["inject"]
                  and final["exportXlsx"] and final["exportDocx"] and final["importXlsx"]
                  and len(page_errors) == 0,
                  json.dumps({"final": final, "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "t19 xlsx export e2e", "dist": str(WEB / "dist")},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
