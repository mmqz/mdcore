#!/usr/bin/env python3
"""#240 层③ e2e：web 打印态不再藏 PDF 面板（PDF 页参与 @media print）。

前置（pdf_view.py 同款）：serve web/dist（8938），#tab-pdf 激活 + 打开
sample.pdf（3 页夹具，缺失时自动生成）。打印面 = 显示面（pdf.js canvas
位图）——不依赖 pkg_pdf（display-only 降级同样可打印，wasm 缺席不 SKIP）。

锁清单（每锁讲清**为什么**）：
1. print_button_present —— #pdf-print-btn 在场 + 打开后可用（打印入口）；
2. print_state_flag —— body[data-pdf-active] 随 tab 激活/切走维护
   （打印 CSS 的单一判据——探针 __pdfPrintInfo 同源）；
3. print_css_pdf_active —— print 媒体态：PDF 面可见 + 编辑器隐藏 +
   工具栏/状态行不进纸面 + 滚动容器解禁（overflow:auto 会裁打印内容）+
   一页一纸（break-after: page，末页不续纸）+ 文字层/交互叠加层不进
   纸面（canvas 位图即内容）——**issue 任务③ 的 CSS 面**；
4. print_css_editor_regression —— 编辑器 tab 激活时 print 态维持旧行为
   （#pdf-panel 隐藏、#editor 可见——回归锁：只加 PDF 面打印，不改
   既有「Ctrl+P = 编辑器纸面输出」语义）；
5. print_media_restore —— print 媒体态切回 screen 后屏幕样式复原
   （打印 CSS 只影响 print 态，不污染屏幕面）；
6. print_prepare_all_pages —— __pdfPrintPrepare() = 打印按钮同一路径：
   懒渲染（视口 ±1）→ 全 3 页渲染完成（打印需要每页位图在场——远页
   canvas 释放的逆操作）；
7. print_pdf_output_pages —— **端到端主锁**：emulate print 媒体 +
   page.pdf() 真实走 Chromium 打印管线 → 产物 PDF 页数 ≥ 3（一页一纸
   落到真实分页；改前 #pdf-panel 被 @media print 藏死——产物 1 页）。

结果 -> tests/results/results_pdf_print.json
"""

import base64
import http.server
import json
import re
import socketserver
import subprocess
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURE = WEB / "tests" / "fixtures" / "sample.pdf"
GEN = ["python3", "tests/gen_pdf_fixture.py"]
OUT = WEB / "tests" / "results" / "results_pdf_print.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update(
    {".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm",
     ".mjs": "text/javascript"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True


def pdf_page_count(data: bytes) -> int:
    """Chromium page.pdf() 产物的页数（/Type /Pages 的 /Count N——最大值
    口径：嵌套 Pages 树时取所有 Count 的最大值，容忍结构差异）。"""
    counts = [int(m) for m in re.findall(rb"/Type\s*/Pages[^>]*?/Count\s+(\d+)", data)]
    # /Count 与 /Type 顺序不定——反着也扫一遍
    counts += [int(m) for m in re.findall(rb"/Count\s+(\d+)[^>]*?/Type\s*/Pages", data)]
    return max(counts) if counts else 0


def main() -> int:
    if not FIXTURE.exists():
        print(f"fixture missing {FIXTURE} — generating via {GEN[1]}")
        subprocess.run(GEN, check=True, cwd=str(WEB))
    fixture_bytes = FIXTURE.read_bytes()
    fixture_b64 = base64.b64encode(fixture_bytes).decode()

    checks = []
    page_errors: list[str] = []
    skipped = 0

    def check(name, ok, detail="", skip=False):
        nonlocal skipped
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip),
                       "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        if skip:
            skipped += 1
        print(tag + name + ("  | " + str(detail)[:160] if detail and not ok and not skip else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8938), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8938/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            page.wait_for_function("window.__pdfReady === true", timeout=30000)

            # 激活 PDF tab + 打开夹具（打印面 = 显示面，不依赖 wasm——
            # display-only 降级同样走 pdf.js 渲染）。
            page.click("#tab-pdf")
            page.wait_for_function(
                "getComputedStyle(document.getElementById('pdf-panel')).display !== 'none'")
            opened = page.evaluate("b64 => window.__pdfOpenBytes(b64)", fixture_b64)
            page.wait_for_function("window.__pdfPage(1).rendered === true", timeout=20000)

            # ---- 1. print_button_present（打印入口在位 + 打开后可用）----
            btn = page.evaluate("""() => ({
                present: !!document.getElementById('pdf-print-btn'),
                disabled: document.getElementById('pdf-print-btn').disabled,
                label: document.getElementById('pdf-print-btn').textContent.trim(),
                openOk: window.__pdfState().open === true,
                pages: window.__pdfState().pages,
            })""")
            check("print_button_present",
                  btn["present"] and btn["disabled"] is False and btn["openOk"]
                  and btn["pages"] == 3,
                  json.dumps({"btn": btn, "opened": opened}, ensure_ascii=False))

            # ---- 2. print_state_flag（data-pdf-active 判据维护）----
            flag = page.evaluate("""() => ({
                active: document.body.dataset.pdfActive !== undefined,
                probe: window.__pdfPrintInfo(),
            })""")
            check("print_state_flag",
                  flag["active"] is True and flag["probe"]["pdfActive"] is True
                  and flag["probe"]["pages"] == 3,
                  json.dumps(flag, ensure_ascii=False))

            # ---- 3. print_css_pdf_active（打印态 CSS 面——任务③核心）----
            page.emulate_media(media="print")
            css = page.evaluate("""() => {
                const cs = (el) => el ? getComputedStyle(el) : null;
                const panel = cs(document.getElementById('pdf-panel'));
                const editor = cs(document.getElementById('editor'));
                const toolbar = cs(document.getElementById('pdf-toolbar'));
                const status = cs(document.getElementById('pdf-status'));
                const viewport = cs(document.getElementById('pdf-viewport'));
                const content = cs(document.getElementById('pdf-content'));
                const page1 = cs(document.querySelector('.pdf-page[data-page="0"]'));
                const last = cs(document.querySelector('.pdf-page:last-child'));
                const tl = cs(document.querySelector('.pdf-page[data-page="0"] .textLayer'));
                const ov = cs(document.querySelector('.pdf-page[data-page="0"] .pdf-overlay'));
                return {
                    panelDisplay: panel.display, panelMaxWidth: panel.maxWidth,
                    editorDisplay: editor.display,
                    toolbarDisplay: toolbar.display, statusDisplay: status.display,
                    viewportOverflow: viewport.overflow, viewportHeight: viewport.height,
                    contentWidth: content.width,
                    pageBreakAfter: page1.breakAfter, pageMargin: page1.margin,
                    pageShadow: page1.boxShadow, pageBorder: page1.borderTopWidth,
                    lastBreakAfter: last.breakAfter,
                    textLayerDisplay: tl.display, overlayDisplay: ov.display,
                    toolbarApp: cs(document.getElementById('toolbar')).display,
                    statusbar: cs(document.getElementById('status-bar')).display,
                };
            }""")
            # 注：height/width 的 print 态覆盖声明为 auto——computed 返回
            # 解析后的**使用值**（px），故断言用数值口径：viewport 高由
            # 内容驱动（3 页全展开 > 1000px——旧 calc(100% - 64px) 会钳在
            # 视口高量级 ≈ 700px，裁剪打印内容）。
            vh = float(re.sub(r"[^0-9.]", "", css["viewportHeight"]) or 0)
            check("print_css_pdf_active",
                  css["panelDisplay"] == "block" and css["panelMaxWidth"] == "none"
                  and css["editorDisplay"] == "none"
                  and css["toolbarDisplay"] == "none" and css["statusDisplay"] == "none"
                  and css["viewportOverflow"] == "visible" and vh > 1000.0
                  and css["pageBreakAfter"] == "page"
                  and css["pageMargin"] == "0px"
                  and css["pageShadow"] == "none"
                  and css["pageBorder"] == "0px"
                  and css["lastBreakAfter"] == "auto"
                  and css["textLayerDisplay"] == "none" and css["overlayDisplay"] == "none"
                  and css["toolbarApp"] == "none" and css["statusbar"] == "none",
                  json.dumps(css, ensure_ascii=False))

            # ---- 7. print_pdf_output_pages（端到端主锁——真实打印管线）----
            # emulate print 媒态下 page.pdf() 走 Chromium 打印分页：3 页
            # 夹具 + break-after: page → 产物 ≥ 3 页（改前面板被藏 → 1 页）。
            pdf_bytes = page.pdf(scale=1.0, print_background=True)
            got_pages = pdf_page_count(pdf_bytes)
            check("print_pdf_output_pages",
                  got_pages >= 3,
                  f"page.pdf() 产物页数 = {got_pages}（期望 ≥ 3——一页一纸主锁；"
                  f"产物头 {pdf_bytes[:16]!r}）")

            # ---- 5. print_media_restore（切回 screen 复原——打印 CSS 不
            #      污染屏幕面）----
            page.emulate_media(media="screen")
            restore = page.evaluate("""() => ({
                panelDisplay: getComputedStyle(document.getElementById('pdf-panel')).display,
                toolbarDisplay: getComputedStyle(document.getElementById('pdf-toolbar')).display,
                viewportOverflow: getComputedStyle(document.getElementById('pdf-viewport')).overflow,
                pageBreakAfter: getComputedStyle(
                    document.querySelector('.pdf-page[data-page="0"]')).breakAfter,
            })""")
            check("print_media_restore",
                  restore["panelDisplay"] != "none" and restore["toolbarDisplay"] != "none"
                  and restore["viewportOverflow"] != "visible",
                  json.dumps(restore, ensure_ascii=False))

            # ---- 6. print_prepare_all_pages（打印按钮同一路径——全页渲染）----
            # 懒渲染只保视口附近——先滚到页 1 制造远页释放态，再走
            # __pdfPrintPrepare（= #pdf-print-btn 点击的渲染前置）。
            page.evaluate(
                "document.getElementById('pdf-viewport').scrollTop = 100000")
            prepared = page.evaluate("() => window.__pdfPrintPrepare()")
            info = page.evaluate("() => window.__pdfPrintInfo()")
            check("print_prepare_all_pages",
                  prepared == 3 and info["rendered"] == 3 and info["pages"] == 3,
                  json.dumps({"prepared": prepared, "info": info}, ensure_ascii=False))

            # ---- 4. print_css_editor_regression（编辑器 tab 打印态回归锁）----
            page.click("#tab-doc")
            page.wait_for_function(
                "getComputedStyle(document.getElementById('pdf-panel')).display === 'none'")
            page.emulate_media(media="print")
            doc_print = page.evaluate("""() => ({
                flag: document.body.dataset.pdfActive !== undefined,
                panelDisplay: getComputedStyle(document.getElementById('pdf-panel')).display,
                editorDisplay: getComputedStyle(document.getElementById('editor')).display,
                toolbarApp: getComputedStyle(document.getElementById('toolbar')).display,
                statusbar: getComputedStyle(document.getElementById('status-bar')).display,
            })""")
            check("print_css_editor_regression",
                  doc_print["flag"] is False and doc_print["panelDisplay"] == "none"
                  and doc_print["editorDisplay"] == "block"
                  and doc_print["toolbarApp"] == "none"
                  and doc_print["statusbar"] == "none",
                  json.dumps(doc_print, ensure_ascii=False))
            page.emulate_media(media="screen")

            # 页面零 JS 错误（全程——含打印态切换）
            check("no_page_errors", len(page_errors) == 0, str(page_errors[:3]))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "#240 层③ web print e2e（打印态不再藏 PDF 面板）",
                 "dist": str(WEB / "dist"), "fixture": str(FIXTURE),
                 "fixture_bytes": len(fixture_bytes)},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "skipped": skipped,
                    "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed"
          + (f" ({skipped} skipped)" if skipped else "")
          + f" -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
