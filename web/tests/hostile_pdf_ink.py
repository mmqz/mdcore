#!/usr/bin/env python3
"""#282（P2-25）敌意 Ink 标注注入负向锁：renderAnnotOverlay 的
polyline 构造改 createElementNS + setAttribute 后，敌意标注字段
（color.r 断 stroke=" 注属性 / width 断 stroke-width=" 注子元素）
必须以惰性属性值落位——零新属性、零新子元素。

数据源论证：this.annotations 两个写入点（AnnotListed 列表刷新 /
AnnotAdded 回显）均来自 wasm 会话 execute() 的 JSON.parse 产物，
v0 标注全本地构造故不可达；「PDF 既有标注解析」落地后敌意面活化
（审计 P2-25 原文）。本测试用 Playwright route 拦截把
pkg_pdf/pdf_wasm.js 换成敌意 mock 模块（临时路由注入——canvas_view
契约 mock 同款先例），AnnotListed 直接回敌意 Ink 标注——等价于
敌意解析面的注入，无需等真解析落地。

判别力（旧 innerHTML 代码上必红）：
- color.r = '0" onload="window.__inkPwned=1" data-pwn="x' 旧代码
  → polyline 多出 onload/data-pwn 属性（getAttributeNames 不符）
- width = '2"><img src=x onerror=...' 旧代码 → polyline 多出
  <img> 子元素（childElementCount > 0）

夹具 web/tests/fixtures/sample.pdf（缺失时 gen_pdf_fixture.py 自动
生成）。serve web/dist（8945）。
结果 -> tests/results/results_hostile_pdf_ink.json
"""

import base64
import http.server
import json
import socketserver
import subprocess
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURE = WEB / "tests" / "fixtures" / "sample.pdf"
GEN = ["python3", "tests/gen_pdf_fixture.py"]
OUT = WEB / "tests" / "results" / "results_hostile_pdf_ink.json"
PORT = 8945

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

# 敌意 mock 模块（pkg_pdf/pdf_wasm.js 的替换面——最小结构面：
# default 单飞门 + WasmPdfSession ctor/set_clock_ms/execute）。
# AnnotateList 回 window.__pdfHostileAnnots（测试运行前注入的敌意
# 标注）；AnnotateAdd 回显 draft（kind 原样——正常回归用例走它）。
MOCK_MODULE_JS = """
export default async function () {}
export class WasmPdfSession {
  constructor(metaJson, assessmentJson, pagesJson, outlineJson) {
    this._nextId = 1;
  }
  set_clock_ms(now) {}
  execute(cmdJson) {
    const cmd = JSON.parse(cmdJson);
    if (cmd && cmd.AnnotateList) {
      return JSON.stringify({ AnnotListed: (window.__pdfHostileAnnots || []) });
    }
    if (cmd && cmd.AnnotateAdd) {
      const d = cmd.AnnotateAdd.draft;
      const annot = {
        id: this._nextId++, page: d.page, kind: d.kind, author: d.author,
        origin: d.origin, created_ms: 0, modified_ms: 0,
      };
      return JSON.stringify({ AnnotAdded: annot });
    }
    return JSON.stringify({ ExtractedText: { pages: [] } });
  }
}
"""

# 敌意 Ink 标注：strokes 正常（保 polyline 真渲染——断言先证明渲染
# 路径跑通再谈注入）；color.r / width 载荷断属性双面（审计 P2-25 原文
# 的 `stroke="` 断注 + 子元素注入二合一）。
HOSTILE_ANNOT = {
    "id": 1,
    "page": 1,
    "kind": {
        "Ink": {
            "strokes": [[{"x": 80, "y": 600}, {"x": 200, "y": 620}, {"x": 300, "y": 590}]],
            "color": {"r": '0" onload="window.__inkPwned=1" data-pwn="x', "g": 0, "b": 0},
            "width": '2"><img src=x onerror="window.__inkPwned2=1',
        }
    },
    "author": "e2e-hostile",
    "origin": "Human",
    "created_ms": 0,
    "modified_ms": 0,
}

#: createElementNS 构造的属性面全集（旧 innerHTML 渲染面同集——
#: renderAnnotOverlay Ink 分支的六个 setAttribute 名）。
EXPECTED_ATTRS = ["fill", "points", "stroke", "stroke-linecap", "stroke-linejoin", "stroke-width"]


def main() -> int:
    if not FIXTURE.exists():
        print(f"fixture missing {FIXTURE} — generating via {GEN[1]}")
        subprocess.run(GEN, check=True, cwd=str(WEB))
    fixture_b64 = base64.b64encode(FIXTURE.read_bytes()).decode()

    if not (WEB / "dist" / "main.js").exists():
        print("SKIP: web/dist/main.js absent — run `npm run build` (build-wasm.sh first)")
        return 0

    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail="", skip=False):
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip),
                       "detail": str(detail)[:400]})
        print(("SKIP " if skip else "PASS " if ok else "FAIL ") + name
              + (("  | " + str(detail)[:200]) if detail and not ok and not skip else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))

            # 路由拦截在 goto 前就位——setupPdfView 的懒加载 import 一发即中
            page.route(
                "**/pkg_pdf/pdf_wasm.js",
                lambda route: route.fulfill(
                    status=200,
                    content_type="text/javascript",
                    body=MOCK_MODULE_JS,
                ),
            )
            page.goto(f"http://127.0.0.1:{PORT}/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            page.wait_for_function("window.__pdfReady === true", timeout=30000)

            # 敌意标注先于打开注入（openBytes → refreshAnnotations →
            # AnnotListed 即取到）
            page.evaluate(
                "annots => { window.__pdfHostileAnnots = annots; }", [HOSTILE_ANNOT]
            )

            page.click("#tab-pdf")
            page.wait_for_function(
                "getComputedStyle(document.getElementById('pdf-panel')).display !== 'none'")
            page.wait_for_function("window.__pdfState().wasm === true", timeout=10000)
            opened = page.evaluate("b64 => window.__pdfOpenBytes(b64)", fixture_b64)
            page.wait_for_function("window.__pdfPage(1).rendered === true", timeout=20000)

            # ---- 1. hostile_ink_open_with_mock_session ----
            # 敌意 mock 会话打开成功 = 注入链路跑通的前置证明
            #（打开 → 3 页 + wasm 面在场）
            state = page.evaluate("window.__pdfState()")
            check("hostile_ink_open_with_mock_session",
                  opened.get("ok") is True and state.get("open") is True
                  and state.get("pages") == 3 and state.get("wasm") is True,
                  json.dumps({"opened": opened, "state": state}, ensure_ascii=False))

            # ---- 2. hostile_ink_payload_inert ----
            # 核心：载荷以惰性属性值落位——零新属性、零新子元素、
            # 属性面全集恰合、敌意文本在 stroke/width 值内完整可见。
            got = page.evaluate("""() => {
                const svg = document.querySelector('#pdf-viewport svg.pdf-annot-ink');
                const poly = svg ? svg.querySelector('polyline') : null;
                return {
                    polyExists: !!poly,
                    attrNames: poly ? poly.getAttributeNames().sort() : null,
                    stroke: poly ? poly.getAttribute('stroke') : null,
                    width: poly ? poly.getAttribute('stroke-width') : null,
                    polyChildren: poly ? poly.childElementCount : null,
                    svgChildren: svg ? svg.childElementCount : null,
                    extraEls: document.querySelectorAll('#pdf-viewport svg.pdf-annot-ink *:not(polyline)').length,
                    pwned1: window.__inkPwned || null,
                    pwned2: window.__inkPwned2 || null,
                };
            }""")
            kind = HOSTILE_ANNOT["kind"]["Ink"]
            stroke_expected = f"rgb({kind['color']['r']}, {kind['color']['g']}, {kind['color']['b']})"
            width_expected = str(kind["width"])
            check("hostile_ink_payload_inert",
                  got["polyExists"] is True
                  and got["attrNames"] == sorted(EXPECTED_ATTRS)
                  and got["stroke"] == stroke_expected
                  and got["width"] == width_expected
                  and got["polyChildren"] == 0
                  and got["svgChildren"] == 1
                  and got["extraEls"] == 0
                  and got["pwned1"] is None
                  and got["pwned2"] is None,
                  json.dumps(got, ensure_ascii=False))

            # ---- 3. hostile_ink_no_attribute_escape ----
            # 拆面：断属性逃逸的关键观测——getNamedItem 按敌意注入名
            # 查必为 null（onload/data-pwn 在旧代码上是真实属性）。
            no_esc = page.evaluate("""() => {
                const poly = document.querySelector('#pdf-viewport svg.pdf-annot-ink polyline');
                if (!poly) return { ok: false };
                return {
                    ok: !poly.getAttributeNode('onload') && !poly.getAttributeNode('data-pwn')
                        && !poly.getAttributeNode('src') && !poly.getAttributeNode('onerror'),
                    onload: poly.getAttribute('onload'),
                };
            }""")
            check("hostile_ink_no_attribute_escape", no_esc.get("ok") is True,
                  json.dumps(no_esc, ensure_ascii=False))

            # ---- 4. hostile_ink_normal_add_regression ----
            # 正常路径回归锁：mock 会话回显（inkPayload 缺省蓝/2pt）→
            # 第二条 polyline 属性面与存储往返一致（createELementNS
            # 改写不改变可观测 DOM 语义——与 pdf_view.py 10c 同口径）。
            add_r = page.evaluate(
                """() => window.__pdfAddInk(1, [
                    [{x: 100, y: 610}, {x: 220, y: 630}],
                ])""")
            norm = page.evaluate("""() => {
                const polys = document.querySelectorAll('#pdf-viewport svg.pdf-annot-ink polyline');
                const poly = polys[polys.length - 1];
                return {
                    count: polys.length,
                    stroke: poly ? poly.getAttribute('stroke') : null,
                    width: poly ? poly.getAttribute('stroke-width') : null,
                    attrs: poly ? poly.getAttributeNames().sort() : null,
                    children: poly ? poly.childElementCount : null,
                };
            }""")
            check("hostile_ink_normal_add_regression",
                  add_r.get("ok") is True
                  and norm["count"] == 2
                  and norm["stroke"] == "rgb(26, 115, 232)"
                  and norm["width"] == "2"
                  and norm["attrs"] == sorted(EXPECTED_ATTRS)
                  and norm["children"] == 0,
                  json.dumps({"add": add_r, "norm": norm}, ensure_ascii=False))

            browser.close()

    ok_all = all(c["ok"] for c in checks if not c["skip"])
    if page_errors:
        ok_all = False
        for e in page_errors:
            print("PAGE ERROR:", e)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps({"checks": checks, "page_errors": page_errors},
                              ensure_ascii=False, indent=1))
    print(f"\n{len(checks)}/{len(checks)} checks {'passed' if ok_all else 'FAILED'}"
          f" -> {OUT}")
    return 0 if ok_all else 1


if __name__ == "__main__":
    raise SystemExit(main())
