#!/usr/bin/env python3
"""t18 DOCX export e2e (editor document -> office-io write face -> .docx
bytes download, closing the DOCX round-trip with t16): serve web/dist,
drive the real export button + programmatic window.__exportDocx, assert
zip validity ([Content_Types].xml / word/document.xml), string-level
document.xml mapping evidence (style_probe precedent), degraded-block
tolerance, empty-document minimal docx, editor state untouched by export
(read-only), and full circle export -> __importDocx -> content compare.
Evidence -> results/results_docx_export.json
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
OUT = WEB / "tests" / "results" / "results_docx_export.json"

# 快乐路径文档：标题 + 段落 + 表（映射全覆盖的三块型——与 native
# roundtrip 测试同款面）。
EXPORT_MD = (
    "# Export Test\n"
    "plain paragraph text\n"
    "| alpha | beta |\n"
    "| --- | --- |\n"
    "| 1 | 2 |"
)

# 降级面文档：code block / blockquote / list（嵌套）/ divider——映射表中
# 无 docx 表达面的四块型（docx_export.rs 模块文档登记）。
DEGRADED_MD = (
    "```\n"
    "let x = 1;\n"
    "```\n"
    "> quoted line\n"
    "- list item\n"
    "  - nested\n"
    "---"
)

# 全环文档：只用非降级块型（heading/paragraph/table）——导出→t16 导入
# 在该子集上应逐字还原（降级块型不可全环——模块文档登记，本测试
# 显式取可表达子集）。
FULLCIRCLE_MD = (
    "# Round Trip\n"
    "plain text\n"
    "| a | b |\n"
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

# 页内字节导入助手（base64 → Uint8Array → window.__importDocx）——t16 面
IMPORT_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__importDocx(bytes);
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


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8935), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8935/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- UI 面在场 + 契约（启动零回归：新模块不破坏 boot）----
            ui = page.evaluate("""() => ({
                btn: !!document.getElementById('docx-export-btn'),
                btnLabel: (document.getElementById('docx-export-btn') || {}).textContent,
                status: !!document.getElementById('docx-export-status'),
                exportFn: typeof window.__exportDocx,
                last: window.__lastDocxExport,
                contract: window.__contractVersion,
            })""")
            check("export_ui_present",
                  ui["btn"] and ui["btnLabel"] == "export .docx" and ui["status"]
                  and ui["exportFn"] == "function" and ui["last"] is None and ui["contract"] == 1.1,
                  json.dumps(ui))

            # ---- 快乐路径：真实按钮点击（UI 路径）+ 程序化钩子取字节 ----
            page.evaluate("(s) => window.__inject(s)", EXPORT_MD)
            page.wait_for_function("window.__blockCount() === 3", timeout=15000)
            page.click("#docx-export-btn")
            page.wait_for_function("window.__lastDocxExport && window.__lastDocxExport.ok === true",
                                   timeout=15000)
            r = page.evaluate("window.__exportDocx()")
            ok_zip = False
            part_names = []
            try:
                raw = as_bytes(r["bytes"])
                zf = zipfile.ZipFile(io.BytesIO(raw))
                part_names = zf.namelist()
                ok_zip = (
                    raw[:2] == b"PK"
                    and "[Content_Types].xml" in part_names
                    and "word/document.xml" in part_names
                    and zipfile.ZipFile(io.BytesIO(raw)).testzip() is None
                )
                if ok_zip:
                    # 后续检查复用同一 zip（doc_xml 一次性取出）
                    doc_xml = zip_part(zf, "word/document.xml")
                    page.evaluate("(s) => { window.__docXml = s; }", doc_xml)
            except Exception as e:  # noqa: BLE001
                ok_zip = False
                r = {"ok": r.get("ok"), "error": str(e)}
            status_text = page.evaluate("document.getElementById('docx-export-status').textContent")
            check("happy_export_valid_zip",
                  r.get("ok") is True and ok_zip and r["sizeBytes"] > 400
                  and status_text.startswith("exported:")
                  and "blocks" in status_text and "simplified" not in status_text,
                  json.dumps({"r": {k: r.get(k) for k in ("ok", "sizeBytes")},
                              "parts": part_names, "status": status_text}))

            # ---- document.xml 字符串级映射证据（style_probe 先例）----
            doc_xml = page.evaluate("window.__docXml")
            check("document_xml_mapping_strings",
                  "Export Test" in doc_xml and "plain paragraph text" in doc_xml
                  and "Heading1" in doc_xml and "<w:outlineLvl" in doc_xml,
                  doc_xml[:200] if "Export Test" not in doc_xml else "ok")

            # ---- 表导出（w:tbl + 单元格文本）----
            check("table_export_xml",
                  "<w:tbl>" in doc_xml and "alpha" in doc_xml and "beta" in doc_xml
                  and ">1<" in doc_xml and ">2<" in doc_xml,
                  "tbl marker + cells")

            # ---- 降级块不破导出 + 编辑器现状零触碰（导出只读）----
            page.evaluate("(s) => window.__inject(s)", DEGRADED_MD)
            page.wait_for_function("window.__blockCount() === 6", timeout=15000)
            md_before = page.evaluate("window.__md()")
            r2 = page.evaluate("window.__exportDocx()")
            raw2 = as_bytes(r2["bytes"])
            zf2 = zipfile.ZipFile(io.BytesIO(raw2))
            doc_xml2 = zip_part(zf2, "word/document.xml")
            degraded_status = page.evaluate("document.getElementById('docx-export-status').textContent")
            check("degraded_blocks_survive_export",
                  r2["ok"] is True and "let x = 1;" in doc_xml2
                  and "quoted line" in doc_xml2 and "list item" in doc_xml2 and "nested" in doc_xml2
                  and "5 simplified" in degraded_status
                  and page.evaluate("window.__md()") == md_before,
                  json.dumps({"status": degraded_status, "md_same": True}))

            # ---- 空文档 = 最小合法 docx ----
            page.evaluate("window.__inject('')")
            page.wait_for_function("window.__blockCount() === 0", timeout=15000)
            r3 = page.evaluate("window.__exportDocx()")
            raw3 = as_bytes(r3["bytes"])
            zf3 = zipfile.ZipFile(io.BytesIO(raw3))
            empty_status = page.evaluate("document.getElementById('docx-export-status').textContent")
            check("empty_document_minimal_docx",
                  r3["ok"] is True and "word/document.xml" in zf3.namelist()
                  and "0 blocks" in empty_status,
                  empty_status)

            # ---- 全环：export → __importDocx → 内容比对（非降级子集逐字）----
            page.evaluate("(s) => window.__inject(s)", FULLCIRCLE_MD)
            page.wait_for_function("window.__blockCount() === 3", timeout=15000)
            r4 = page.evaluate("window.__exportDocx()")
            raw4 = as_bytes(r4["bytes"])
            b64 = base64.b64encode(raw4).decode()
            reimport = page.evaluate(IMPORT_JS, b64)
            reimport_status = page.evaluate("document.getElementById('docx-status').textContent")
            md_roundtrip = page.evaluate("window.__md()")
            check("full_circle_reimport_content",
                  reimport.get("ok") is True and md_roundtrip == FULLCIRCLE_MD
                  and "imported:" in reimport_status,
                  json.dumps({"reimport": {k: reimport.get(k) for k in ("ok", "code")},
                              "md": md_roundtrip[:160], "status": reimport_status}))

            # ---- 契约面 + 零页错（启动/导出/导入全程无 JS 异常逃逸）----
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                contract: window.__contractVersion === 1.1,
                inject: typeof window.__inject === 'function',
                exportFn: typeof window.__exportDocx === 'function',
                importFn: typeof window.__importDocx === 'function',
            })""")
            check("contract_intact_no_pageerror",
                  final["ready"] and final["contract"] and final["inject"]
                  and final["exportFn"] and final["importFn"]
                  and len(page_errors) == 0,
                  json.dumps({"final": final, "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "t18 docx export e2e", "dist": str(WEB / "dist")},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
