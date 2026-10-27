#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""#8 K4/K6 重核补遗（一次性别脚本）：表格空段落 + canvas 形态面。"""

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

WEB = Path(__file__).resolve().parent
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript",
                               ".wasm": "application/wasm", ".mjs": "text/javascript"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True

MD_TABLE = """# 表格空段重核

| 列甲 | 列乙 | 列丙 |
|---|---|---|
| 一行一 | 一行二 | 一行三 |
| 二行一 | 二行二 | 二行三 |
| 三行一 | 三行二 | 三行三 |

表格后正文段。"""


def main() -> int:
    out = {}
    with socketserver.TCPServer(("127.0.0.1", 8943), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.goto("http://127.0.0.1:8943/index.html")
            page.wait_for_function("window.__ready === true", timeout=60_000)

            # ---- K6：表格导出空段落 ----
            page.evaluate(
                "([md]) => window.__inject(md)", [MD_TABLE])
            page.wait_for_timeout(200)
            r = page.evaluate("window.__exportDocx()")
            assert r.get("ok"), f"export failed: {r}"
            v = r["bytes"]
            docx = bytes(v) if isinstance(v, list) else v
            zf = zipfile.ZipFile(io.BytesIO(docx))
            xml = zf.read("word/document.xml").decode("utf-8")

            # 表内空段：w:tbl 内、w:tr 之间的空 w:p（K6 形态）；
            # 表后空段：w:tbl 结束与下一段之间的空 w:p。
            import re
            tbls = re.findall(r"<w:tbl>.*?</w:tbl>", xml, re.S)
            out["tables"] = len(tbls)
            empty_p_in_tbl = 0
            trs = re.findall(r"<w:tr[ >].*?</w:tr>", tbls[0], re.S) if tbls else []
            out["rows"] = len(trs)
            for tr in trs:
                # tr 直接子层的空段（非单元格内）
                cells = re.findall(r"<w:tc>.*?</w:tc>", tr, re.S)
                body = re.sub(r"<w:tc>.*?</w:tc>", "", tr, flags=re.S)
                empty_p_in_tbl += len(re.findall(r"<w:p\s*/>|<w:p></w:p>", body))
            out["empty_p_between_rows_in_tbl"] = empty_p_in_tbl
            # 表后空段（tbl 结束到下一个非空段之间）
            tail = re.search(r"</w:tbl>(.*?)<w:p[ >]", xml, re.S)
            out["between_tbl_and_next_p"] = (
                len(re.findall(r"<w:p\s*/>|<w:p></w:p>", tail.group(1)))
                if tail else None)
            # 全文档空段计数（信息面）
            out["total_empty_p"] = len(re.findall(r"<w:p\s*/>|<w:p></w:p>", xml))
            out["docx_bytes"] = len(docx)

            # ---- K4：canvas 形态面（模型对象种类 + 渲染元素）----
            page.click("#tab-canvas")
            page.wait_for_function("window.__canvasReady === true", timeout=30_000)
            out["canvas_state_kinds"] = page.evaluate(
                "window.__canvasState()").get("objects_by_kind",
                page.evaluate("JSON.stringify(window.__canvasState())")[:200])
            # 形态面探查：add node 后 SVG 元素形态
            page.evaluate("window.__canvasAddNode(60, 60, 'k4')")
            page.wait_for_timeout(100)
            svg_shape = page.evaluate("""() => {
              const layer = document.getElementById('cv-l-nodes');
              const el = layer ? layer.firstElementChild : null;
              return el ? el.tagName : null;
            }""")
            out["canvas_node_svg_element"] = svg_shape
            page.close()
            browser.close()

    print(json.dumps(out, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
