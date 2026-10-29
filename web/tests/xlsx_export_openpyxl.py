#!/usr/bin/env python3
"""#22 Q3-2 XLSX export openpyxl independent verification (web e2e).

与 t19（xlsx_export.py，引擎自带 calamine 读回 + zip 字面断言）互补：
本套件用 **openpyxl 独立读取**（第三方消费者视角，非引擎读面）验证
导出结构，覆盖三面：

1. 公式写回（Q3-2 新面）：反引号公式格（P2-3 导入标记）→ 真公式格
   ——openpyxl 读出 `=SUM(1,2)` 公式文本而非字面反引号串；
2. **验收**：excel-complex.xlsx（真夹具）导入 → 再导出 → openpyxl
   读出结构一致（sheet 名 / 单元格文本 / 公式格 / 合并面）；
3. 对称性：import → export → import，markdown 表格内容逐格等价
   （公式反引号形保持）+ 降级面显式断言（合并不可重构[md 单流] /
   公式缓存值不复算[cached None] / 超链接以 GFM 文本形态落格）。

Evidence -> tests/results/results_xlsx_export_openpyxl.json
"""

import base64
import http.server
import json
import socketserver
import sys
import threading
from io import BytesIO
from pathlib import Path

from openpyxl import load_workbook
from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_xlsx_export_openpyxl.json"
FIXTURE = WEB.parent / "test-artifacts" / "TR2" / "fixtures" / "excel-complex.xlsx"

# 公式写回面：反引号公式格（单载/双载）+ 普通文本格 + 数值串（不推断）。
# 批 5 双载：`=SUM(3,4)`=7 → 公式 + 缓存值 7（openpyxl data_only 可读）。
FORMULA_MD = (
    "| a | b | c |\n"
    "| --- | --- | --- |\n"
    "| `=SUM(1,2)` | 10 | `=SUM(3,4)`=7 |"
)

IMPORT_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__importXlsx(bytes);
}"""

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


def export_bytes(page) -> bytes:
    """__exportXlsx → python bytes（失败即抛——套件级失败面）。"""
    r = page.evaluate("window.__exportXlsx()")
    assert r.get("ok") is True, f"export failed: {r}"
    arr = page.evaluate("Array.from(window.__exportXlsx().bytes)")
    return bytes(arr)


def md_tables(md: str) -> list[list[list[str]]]:
    """markdown → 表格块列表（GFM 管道行解析——单元格反斜杠竖线反转义）。"""
    tables: list[list[list[str]]] = []
    current: list[list[str]] = []
    for line in md.split("\n"):
        stripped = line.strip()
        if stripped.startswith("|") and stripped.endswith("|"):
            cells = [c.strip().replace("\\|", "|") for c in stripped[1:-1].split(" | ")]
            # 分隔行跳过（--- 形）。
            if all(set(c) <= {"-", ":", " "} and c for c in cells):
                continue
            current.append(cells)
        else:
            if current:
                tables.append(current)
                current = []
    if current:
        tables.append(current)
    return tables


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    fixture_b64 = base64.b64encode(FIXTURE.read_bytes()).decode()

    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("127.0.0.1", 8938), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8938/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- 1. 公式写回（openpyxl 独立读——非引擎 calamine 面）----
            page.evaluate("(s) => window.__inject(s)", FORMULA_MD)
            wb = load_workbook(BytesIO(export_bytes(page)))
            ws = wb["a"]
            a2 = ws["A2"].value
            b2 = ws["B2"].value
            c2 = ws["C2"].value
            check("openpyxl_formula_cells",
                  a2 == "=SUM(1,2)" and b2 == "10" and isinstance(b2, str)
                  and c2 == "=SUM(3,4)"
                  and list(ws.merged_cells.ranges) == [],
                  json.dumps({"A2": repr(a2), "B2": repr(b2), "C2": repr(c2),
                              "sheets": wb.sheetnames}))
            # 批 5 双载缓存写回：data_only 读出缓存值 7（非 0——缓存通道
            # 真实落盘，原 cached None → 0 降级面根治）。
            wb_v = load_workbook(BytesIO(export_bytes(page)), data_only=True)
            check("openpyxl_dualload_cached_value",
                  wb_v["a"]["C2"].value == 7,
                  json.dumps({"C2": repr(wb_v["a"]["C2"].value)}))

            # ---- 2. 验收：excel-complex 导入 → 再导出 → openpyxl 结构一致 ----
            imp = page.evaluate(IMPORT_JS, fixture_b64)
            assert imp.get("ok") is True, f"fixture import failed: {imp}"
            md1 = page.evaluate("window.__md()")
            stats1 = page.evaluate("window.__lastXlsxImport.stats")
            exported = export_bytes(page)
            wb2 = load_workbook(BytesIO(exported))

            # sheet 名（批 5 表名通道）：H2 标题紧邻表 → 标题认领为表名
            #（报价明细/评分表——导入面 `## {表名}` 节标题的对称回取），
            # 无 Document 表（标题不双落）。
            names_ok = wb2.sheetnames == ["报价明细", "评分表"]
            wsx, wsp = wb2["报价明细"], wb2["评分表"]
            # 报价明细 表：文本扁平（原数值 1 → str "1"——不推断在案）。
            seq_ok = wsx["A1"].value == "序号" and wsx["A2"].value == "1" and isinstance(wsx["A2"].value, str)
            # 评分表 表：公式格真公式（openpyxl 读出 = 形文本）+ 数据格逐字
            #（批 5：公式格带缓存写回——data_only 读出 100/1）。
            pf_ok = (wsp["B5"].value == "=SUM(B2:B4)" and wsp["C5"].value == "=SUM(C2:C4)"
                     and wsp["A2"].value == "技术" and wsp["B2"].value == "40")
            wb2_v = load_workbook(BytesIO(exported), data_only=True)
            cached_ok = (wb2_v["评分表"]["B5"].value == 100
                         and wb2_v["评分表"]["C5"].value == 1)
            # 合并面：md 单流无可重构合并 → 导出恒 0（降级显式断言；
            # 本夹具原文件合并也为 0——无损失）。
            merges_ok = all(list(ws.merged_cells.ranges) == [] for ws in wb2.worksheets)
            # 招标公告行：GFM 链接文本形态落格（rels 链接不可重构——降级）。
            link_row = " ".join(str(wsp.cell(row=r, column=c).value or "")
                                for r in (6, 7) for c in (1, 2, 3))
            link_ok = "招标公告" in link_row and "https://example.com/tender" in link_row
            check("excel_complex_openpyxl_consistency",
                  names_ok and seq_ok and pf_ok and cached_ok and merges_ok and link_ok
                  and len(page_errors) == 0,
                  json.dumps({"sheets": wb2.sheetnames, "names_ok": names_ok,
                              "seq": seq_ok, "pf": pf_ok, "cached": cached_ok,
                              "merges": merges_ok, "link": link_ok,
                              "link_row": link_row[:120]}, ensure_ascii=False))

            # ---- 3. 对称性：import → export → import 内容等价 ----
            b64_roundtrip = base64.b64encode(exported).decode()
            imp2 = page.evaluate(IMPORT_JS, b64_roundtrip)
            assert imp2.get("ok") is True, f"reimport failed: {imp2}"
            md2 = page.evaluate("window.__md()")
            stats2 = page.evaluate("window.__lastXlsxImport.stats")

            # 表格内容逐格等价（批 5 表名通道）：首环 2 表 vs 再导入 2 表
            # ——**精确对称**（表名从节标题回取，无 Document 表注入）。
            t1, t2 = md_tables(md1), md_tables(md2)
            tables_eq = (len(t1) == 2 and len(t2) == 2 and t1 == t2)
            # 公式通道保持（反引号双载形往返）+ 缓存保真（批 5：原夹具
            # 缓存值 100/1 经双载通道写回读回——不再降级为 0）。
            f2 = stats2.get("formulas") or []
            formulas_ok = (stats2.get("formulas_imported") == 2
                           and {x.get("text") for x in f2} == {"=SUM(B2:B4)", "=SUM(C2:C4)"}
                           and {x.get("cached") for x in f2} == {"100", "1"})
            # 降级面：merged_ranges 0（md 往返无合并面）。
            check("import_export_import_symmetry",
                  tables_eq and formulas_ok and stats2.get("merged_ranges") == 0
                  and stats2.get("sheets") == 2
                  and len(page_errors) == 0,
                  json.dumps({"tables_eq": tables_eq, "t1": len(t1), "t2": len(t2),
                              "formulas": f2, "sheets": stats2.get("sheets")},
                             ensure_ascii=False))

            browser.close()
        srv.shutdown()

    ok = all(c["ok"] for c in checks) and not page_errors
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps({
        "meta": {"suite": "xlsx_export_openpyxl", "fixture": str(FIXTURE.name),
                 "ok": ok, "checks": len(checks), "pageErrors": page_errors},
        "checks": checks,
    }, ensure_ascii=False, indent=1))
    print(f"\n{'ALL PASS' if ok else 'FAILURES'}: {sum(c['ok'] for c in checks)}/{len(checks)} checks"
          f" -> {OUT}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
