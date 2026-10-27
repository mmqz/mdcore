#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""TR-2B L2+L3 · 外部语料跨层执行（issue #11 §4.3）。

L2（pandoc 用例集 → 引擎导出 docx → LO 开验提取 → 文本对比）：
  输入 = jgm/pandoc test/ 四件（writer/tables/more/lhs——GPL 仓只取
  输入数据）；流程 = __inject(md) → __exportDocx() → LO 26.8 headless
  --convert-to txt → 与原文非空白字符集对比（召回率——markdown 语法
  字符的合法变换与 docx/txt 双向损耗已按宽松集计算并记录）。

L3（POI 真实复杂 docx → 引擎导入 → 结构对账 + LO 交叉提取）：
  输入 = apache/poi test-data/document 六件（Apache-2.0——含
  51921-Word-Crash067 崩溃夹具）；流程 = __importDocx(bytes) →
  typed 结果（成功/受控拒绝——零崩溃即过）+ __md() 结构计数
  （标题/表格行/列表项）+ LO 提取文本召回对照（结构召回率口径：
  计数在场性，非字符全等）。
"""

import base64
import http.server
import json
import os
import re
import socketserver
import subprocess
import sys
import tempfile
import threading
from collections import Counter
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent
CORPUS = WEB.parent / "test-artifacts" / "external-corpus"
OUT = WEB / "tests" / "results" / "tr2b_l23_external.json"
LO = Path("/tmp/my-project/lo268/opt/libreoffice26.8/program/soffice")
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript",
                               ".wasm": "application/wasm", ".mjs": "text/javascript"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True

PANDOC_INPUTS = ["writer.markdown", "tables.markdown",
                 "markdown-reader-more.txt", "lhs-test.markdown"]
POI_INPUTS = ["51921-Word-Crash067.docx", "52288.docx", "52449.docx",
              "55733.docx", "56392.docx", "59030.docx"]


def lo_extract(docx_path: Path, outdir: Path) -> str:
    """LO headless docx → txt（失败返回空串并记录 stderr 尾行）。"""
    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = f"{LO.parent}:{LO.parent}/../ure/lib:" + env.get("LD_LIBRARY_PATH", "")
    r = subprocess.run(
        [str(LO), "--headless", "--convert-to", "txt:Text (encoded):UTF8",
         "--outdir", str(outdir), str(docx_path)],
        capture_output=True, text=True, timeout=120, env=env)
    txt = outdir / (docx_path.stem + ".txt")
    if txt.exists():
        return txt.read_text(encoding="utf-8", errors="replace")
    return f"<LO_FAIL: {(r.stderr or r.stdout).strip()[-160:]}>"


def char_recall(src: str, dst: str) -> float:
    """非空白字符多重集召回（宽松——语法字符变换容忍）。"""
    sc, dc = Counter(c for c in src if not c.isspace()), Counter(c for c in dst if not c.isspace())
    kept = sum(min(n, dc[c]) for c, n in sc.items())
    return kept / max(1, sum(sc.values()))


def structure_counts(md: str) -> dict:
    return {
        "headings": len(re.findall(r"^#{1,6} ", md, re.M)),
        "table_rows": len(re.findall(r"^\|.*\|$", md, re.M)),
        "list_items": len(re.findall(r"^(\s*[-*+] |\s*\d+\. )", md, re.M)),
        "blocks": len([l for l in md.split("\n\n") if l.strip()]),
    }


def main() -> int:
    results = {"l2_pandoc": [], "l3_poi": [], "meta": {"lo": "26.8.1.1 headless"}}
    tmp = Path(tempfile.mkdtemp(prefix="tr2b_l23_"))

    with socketserver.TCPServer(("127.0.0.1", 8947), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.goto("http://127.0.0.1:8947/index.html")
            page.wait_for_function("window.__ready === true", timeout=60_000)

            # ---- L2：pandoc 输入 → 引擎导出 → LO 开验 ----
            for name in PANDOC_INPUTS:
                md = (CORPUS / "pandoc" / name).read_text(encoding="utf-8")
                r = page.evaluate("""(md) => {
                    window.__inject(md);
                    const e = window.__exportDocx();
                    return { ok: e.ok, bytes: e.bytes ? Array.from(e.bytes).length : 0,
                             raw: e.bytes ? Array.from(e.bytes) : null };
                }""", md)
                entry = {"input": name, "export_ok": r["ok"], "docx_bytes": r["bytes"]}
                if r["ok"] and r["raw"]:
                    f = tmp / (name.replace("/", "_") + ".docx")
                    f.write_bytes(bytes(r["raw"]))
                    lo_txt = lo_extract(f, tmp)
                    entry["lo_open_ok"] = not lo_txt.startswith("<LO_FAIL")
                    entry["lo_recall"] = round(char_recall(md, lo_txt), 4)
                results["l2_pandoc"].append(entry)
                print("L2", entry)

            # ---- L3：POI 样本 → 引擎导入（typed + 结构 + LO 交叉）----
            for name in POI_INPUTS:
                data = (CORPUS / "poi" / name).read_bytes()
                b64 = base64.b64encode(data).decode()
                r = page.evaluate("""(b64) => {
                    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
                    const res = window.__importDocx(bytes);
                    const md = window.__md();
                    return { res, md, alive: typeof window.__blockCount() === 'number' };
                }""", b64)
                entry = {"input": name,
                         "import_ok": r["res"].get("ok") is True,
                         "alive": r["alive"],
                         "result_shape": {k: r["res"].get(k) for k in ("ok", "code") if k in r["res"]}}
                if r["res"].get("ok"):
                    entry["structure"] = structure_counts(r["md"])
                    # LO 交叉提取（结构召回对照面）
                    lo_txt = lo_extract(CORPUS / "poi" / name, tmp)
                    entry["lo_open_ok"] = not lo_txt.startswith("<LO_FAIL")
                    if entry["lo_open_ok"]:
                        entry["lo_vs_engine_recall"] = round(
                            char_recall(lo_txt, r["md"]), 4)
                results["l3_poi"].append(entry)
                print("L3", entry)
            page.close()
            browser.close()

    # 汇总判定
    l2_ok = all(e["export_ok"] and e.get("lo_open_ok") and e.get("lo_recall", 0) >= 0.9
                for e in results["l2_pandoc"])
    l3_ok = all(e["alive"] for e in results["l3_poi"])
    results["verdict"] = {
        "l2_pass_rate": f"{sum(1 for e in results['l2_pandoc'] if e['export_ok'] and e.get('lo_open_ok'))}/{len(results['l2_pandoc'])}",
        "l2_lo_recall_min": min(e.get("lo_recall", 0) for e in results["l2_pandoc"]),
        "l3_alive_rate": f"{sum(1 for e in results['l3_poi'] if e['alive'])}/{len(results['l3_poi'])}",
        "l3_import_ok_rate": f"{sum(1 for e in results['l3_poi'] if e['import_ok'])}/{len(results['l3_poi'])}",
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(results, ensure_ascii=False, indent=1))
    print("\nverdict:", json.dumps(results["verdict"], ensure_ascii=False))
    print(f"-> {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
