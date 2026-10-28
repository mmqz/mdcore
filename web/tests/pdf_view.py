#!/usr/bin/env python3
"""Phase 5 §8.4 PDF 表面视图 e2e：pdf.js 显示面（canvas 位图 + textLayer
文字选区 + 页缓存/缩放/滚动）+ WasmPdfSession（mdc-pdf 第二 wasm 包
web/pkg_pdf/）模型面（安全评估先行 / 搜索 / 标注四 kind / 增量保存）。

serve web/dist（8938），驱动 #tab-pdf / #pdf-panel 与 window.__pdf* 探针：
夹具 web/tests/fixtures/sample.pdf（3 页最小 PDF，tests/gen_pdf_fixture.py
纯标准库生成——缺失时自动生成）。

pkg_pdf 缺席（mdc-pdf-wasm crate 并行实施中）时：面板照常启动，打开走
display-only 降级（渲染/选区/缩放/缓存可用；会话面 typed bridge_absent）。
会话依赖用例（2-12）按 __pdfState().wasm SKIP 门跳过（照 evidence_panel.py
模式）；1（就绪）与 13（tab 共存 + 零页错）恒跑。加密档只读（11）无标准
库夹具生成路径——SKIP 登记（不强制，任务书明示可选）。

结果 -> tests/results/results_pdf_view.json
"""

import base64
import http.server
import json
import socketserver
import subprocess
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURE = WEB / "tests" / "fixtures" / "sample.pdf"
GEN = ["python3", "tests/gen_pdf_fixture.py"]
OUT = WEB / "tests" / "results" / "results_pdf_view.json"

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


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True


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

            # ---- 1. pdf_ready（面板就绪 + tab 存在 + 工具条齐全）----
            boot = page.evaluate("""() => ({
                ready: window.__pdfReady === true,
                tab: !!document.getElementById('tab-pdf'),
                panel: !!document.getElementById('pdf-panel'),
                toolbar: !!document.getElementById('pdf-toolbar'),
                viewport: !!document.getElementById('pdf-viewport'),
                status: !!document.getElementById('pdf-status'),
                fileInput: !!document.getElementById('pdf-file'),
                pageLabel: !!document.getElementById('pdf-page-label'),
                zoomOut: !!document.getElementById('pdf-zoom-out'),
                zoomIn: !!document.getElementById('pdf-zoom-in'),
                zoomFit: !!document.getElementById('pdf-zoom-fit'),
                zoomLabel: !!document.getElementById('pdf-zoom-label'),
                search: !!document.getElementById('pdf-search'),
                searchPrev: !!document.getElementById('pdf-search-prev'),
                searchNext: !!document.getElementById('pdf-search-next'),
                annotMode: !!document.getElementById('pdf-annot-mode'),
                saveBtn: !!document.getElementById('pdf-save-btn'),
                saveDisabled: document.getElementById('pdf-save-btn').disabled,
                panelHidden: getComputedStyle(document.getElementById('pdf-panel')).display === 'none',
                state: window.__pdfState(),
            })""")
            st = boot["state"]
            check("pdf_ready",
                  boot["ready"] and boot["tab"] and boot["panel"] and boot["toolbar"]
                  and boot["viewport"] and boot["status"] and boot["fileInput"]
                  and boot["pageLabel"] and boot["zoomOut"] and boot["zoomIn"]
                  and boot["zoomFit"] and boot["zoomLabel"] and boot["search"]
                  and boot["searchPrev"] and boot["searchNext"] and boot["annotMode"]
                  and boot["saveBtn"] and boot["saveDisabled"]
                  and boot["panelHidden"]
                  and st["open"] is False and st["pages"] == 0
                  and st["encrypted"] is False and st["warnings"] == []
                  and st["annots"] == 0,
                  json.dumps(boot, ensure_ascii=False))

            # wasm 面在场判定：pkg_pdf 缺席（crate 并行实施）→ 2-12 SKIP
            has_wasm = page.evaluate("window.__pdfState().wasm") is True

            def pending_wasm(name):
                check(name, True, "pending wasm (pkg_pdf absent — skip until mdc-pdf-wasm build)",
                      skip=True)

            # 打开 PDF 面（后续用例的公共前置；会话缺席时同样可开——
            # display-only 降级路径本身也是 1 号面板的运行面）
            page.click("#tab-pdf")
            page.wait_for_function(
                "getComputedStyle(document.getElementById('pdf-panel')).display !== 'none'")
            opened = page.evaluate("b64 => window.__pdfOpenBytes(b64)", fixture_b64)

            # ---- 2. pdf_open（打开 → 3 页 + 首页渲染 + 页码标签）----
            if not has_wasm:
                pending_wasm("pdf_open")
            else:
                page.wait_for_function("window.__pdfPage(1).rendered === true", timeout=20000)
                after = page.evaluate("""() => ({
                    open: window.__pdfState(),
                    label: document.getElementById('pdf-page-label').textContent,
                    pagesDom: document.querySelectorAll('.pdf-page').length,
                    page1: window.__pdfPage(1),
                    zoomLabel: document.getElementById('pdf-zoom-label').textContent,
                })""")
                check("pdf_open",
                      opened.get("ok") is True and opened.get("value", {}).get("pages") == 3
                      and after["open"]["open"] is True and after["open"]["pages"] == 3
                      and after["pagesDom"] == 3
                      and after["page1"]["rendered"] is True
                      and after["page1"]["cached"] is True
                      and after["label"] == "1 / 3"
                      and after["zoomLabel"].endswith("%"),
                      json.dumps({"opened": opened, "after": after}, ensure_ascii=False))

            # ---- 3. pdf_text_selection（textLayer + 原生选区）----
            if not has_wasm:
                pending_wasm("pdf_text_selection")
            else:
                page.wait_for_function(
                    "document.querySelectorAll('.pdf-page[data-page=\"0\"] .textLayer span').length > 0",
                    timeout=20000)
                sel = page.evaluate("""() => {
                    const span = document.querySelector('.pdf-page[data-page="0"] .textLayer span');
                    const range = document.createRange();
                    range.selectNodeContents(span);
                    const s = window.getSelection();
                    s.removeAllRanges(); s.addRange(range);
                    return { text: window.__pdfSelection(),
                             spans: document.querySelectorAll('.pdf-page[data-page="0"] .textLayer span').length };
                }""")
                check("pdf_text_selection",
                      sel["spans"] >= 1 and "Page 1" in sel["text"],
                      json.dumps(sel, ensure_ascii=False))

            # ---- 4. pdf_scale（缩放 + 重渲染 + 缓存失效重建）----
            if not has_wasm:
                pending_wasm("pdf_scale")
            else:
                s0 = page.evaluate("window.__pdfScale()")
                page.click("#pdf-zoom-in")
                page.wait_for_function(f"window.__pdfScale() > {s0}", timeout=10000)
                page.wait_for_function("window.__pdfPage(1).rendered === true", timeout=20000)
                after = page.evaluate("""() => ({
                    scale: window.__pdfScale(),
                    page1: window.__pdfPage(1),
                    zoomLabel: document.getElementById('pdf-zoom-label').textContent,
                })""")
                pct = int(after["zoomLabel"].rstrip("%"))
                check("pdf_scale",
                      after["scale"] > s0 and after["page1"]["rendered"] is True
                      and after["page1"]["cached"] is True
                      and after["page1"]["lastSource"] == "render"  # 缩放失效缓存 → 重渲染
                      and abs(pct - after["scale"] * 100) <= 0.5,
                      json.dumps({"s0": s0, "after": after}, ensure_ascii=False))

            # ---- 5. pdf_page_cache（滚到页 3 → 远页释放 → 回页 1 缓存命中）----
            if not has_wasm:
                pending_wasm("pdf_page_cache")
            else:
                page.evaluate("document.querySelectorAll('.pdf-page')[2].scrollIntoView()")
                page.wait_for_function("window.__pdfPage(3).rendered === true", timeout=20000)
                mid = page.evaluate("""() => ({
                    page3: window.__pdfPage(3),
                    label: document.getElementById('pdf-page-label').textContent,
                })""")
                try:
                    # 远页 canvas 释放（滚动 rAF 后；几何上页 1 距页 3 > 1.5 视口高）
                    page.wait_for_function("window.__pdfPage(1).rendered === false", timeout=5000)
                except Exception:
                    pass  # 释放窗口边界（视口几何差异）——回滚断言仍验证缓存路径
                page.evaluate("document.querySelectorAll('.pdf-page')[0].scrollIntoView()")
                page.wait_for_function("window.__pdfPage(1).rendered === true", timeout=20000)
                page.wait_for_function(
                    "document.getElementById('pdf-page-label').textContent === '1 / 3'",
                    timeout=10000)
                back = page.evaluate("""() => ({
                    page1: window.__pdfPage(1),
                    label: document.getElementById('pdf-page-label').textContent,
                })""")
                check("pdf_page_cache",
                      mid["page3"]["rendered"] is True and mid["label"] == "3 / 3"
                      and back["page1"]["rendered"] is True
                      and back["page1"]["cached"] is True
                      and back["page1"]["lastSource"] == "cache"  # 位图缓存命中回贴
                      and back["label"] == "1 / 3",
                      json.dumps({"mid": mid, "back": back}, ensure_ascii=False))

            # ---- 6. pdf_search（会话 Search 语义命中——3 页各一）----
            if not has_wasm:
                pending_wasm("pdf_search")
            else:
                r = page.evaluate("q => window.__pdfSearch(q)", "Page")
                page.wait_for_function(
                    "document.querySelectorAll('.pdf-search-hit').length >= 3", timeout=20000)
                hits_dom = page.evaluate("document.querySelectorAll('.pdf-search-hit').length")
                pages_of_hits = sorted({h["page"] for h in (r.get("value", {}) or {}).get("hits", [])})
                check("pdf_search",
                      r.get("ok") is True
                      and len(r.get("value", {}).get("hits", [])) >= 3
                      and pages_of_hits == [0, 1, 2]
                      and r.get("value", {}).get("truncated") is False
                      and hits_dom >= 3,  # pdf.js 几何显示层（双面诚实）
                      json.dumps({"r": r, "hits_dom": hits_dom}, ensure_ascii=False))

            # ---- 6b. pdf_extract_text（会话 ExtractText 全档回读——三命令实测面）----
            if not has_wasm:
                pending_wasm("pdf_extract_text")
            else:
                r = page.evaluate("window.__pdfExtractText()")
                pages = r.get("value", []) if r.get("ok") else []
                texts_ok = (
                    len(pages) == 3
                    and all(isinstance(p, list) and len(p) == 2 for p in pages)
                    and [p[0] for p in pages] == [0, 1, 2]
                    and all(f"Page {i + 1}" in pages[i][1] for i in range(3))
                )
                check("pdf_extract_text",
                      r.get("ok") is True and texts_ok,
                      json.dumps({"ok": r.get("ok"),
                                  "pages": [[p[0], p[1][:40]] for p in pages] if pages else r},
                                 ensure_ascii=False))

            # ---- 7. pdf_annot_highlight（程序化高亮 → 会话入库 + overlay DOM）----
            if not has_wasm:
                pending_wasm("pdf_annot_highlight")
            else:
                r = page.evaluate(
                    "([p, rect]) => window.__pdfAddHighlight(p, rect)",
                    [0, {"x": 72, "y": 700, "w": 200, "h": 24}])
                after = page.evaluate("""() => ({
                    annots: window.__pdfAnnots(),
                    stateAnnots: window.__pdfState().annots,
                    overlayDom: document.querySelectorAll('.pdf-annot-highlight').length,
                })""")
                annots = after["annots"] if isinstance(after["annots"], list) else []
                kind_ok = bool(annots) and isinstance(annots[0].get("kind"), dict) \
                    and "Highlight" in annots[0]["kind"]
                annot_id = r.get("value", {}).get("id", 0)
                check("pdf_annot_highlight",
                      r.get("ok") is True and annot_id >= 1
                      and len(annots) == 1 and kind_ok
                      and after["stateAnnots"] == 1 and after["overlayDom"] == 1,
                      json.dumps({"r": r, "after": after}, ensure_ascii=False))

            # ---- 8. pdf_annot_list_filter（AnnotateList 过滤面）----
            if not has_wasm:
                pending_wasm("pdf_annot_list_filter")
            else:
                r2 = page.evaluate(
                    "([p, rect]) => window.__pdfAddHighlight(p, rect)",
                    [1, {"x": 72, "y": 700, "w": 200, "h": 24}])
                all_annots = page.evaluate("window.__pdfAnnots()")
                f_highlight = page.evaluate(
                    "j => window.__pdfAnnots(j)", json.dumps({"kinds": ["Highlight"]}))
                f_note = page.evaluate(
                    "j => window.__pdfAnnots(j)", json.dumps({"kinds": ["Note"]}))
                f_page0 = page.evaluate(
                    "j => window.__pdfAnnots(j)", json.dumps({"page": 0}))
                f_page1 = page.evaluate(
                    "j => window.__pdfAnnots(j)", json.dumps({"page": 1}))
                f_origin = page.evaluate(
                    "j => window.__pdfAnnots(j)", json.dumps({"origin": "Human"}))
                ok = (r2.get("ok") is True
                      and isinstance(all_annots, list) and len(all_annots) == 2
                      and isinstance(f_highlight, list) and len(f_highlight) == 2
                      and isinstance(f_note, list) and len(f_note) == 0
                      and isinstance(f_page0, list) and len(f_page0) == 1
                      and isinstance(f_page1, list) and len(f_page1) == 1
                      and f_page0[0]["page"] == 0 and f_page1[0]["page"] == 1
                      and isinstance(f_origin, list) and len(f_origin) == 2)
                check("pdf_annot_list_filter", ok,
                      json.dumps({"r2": r2, "all": len(all_annots) if isinstance(all_annots, list) else all_annots,
                                  "highlight": len(f_highlight) if isinstance(f_highlight, list) else f_highlight,
                                  "note": len(f_note) if isinstance(f_note, list) else f_note,
                                  "page0": len(f_page0) if isinstance(f_page0, list) else f_page0,
                                  "page1": len(f_page1) if isinstance(f_page1, list) else f_page1,
                                  "origin": len(f_origin) if isinstance(f_origin, list) else f_origin},
                                 ensure_ascii=False))

            # ---- 9. pdf_flush（增量保存：appended > 0 + prefix == 原字节长）----
            if not has_wasm:
                pending_wasm("pdf_flush")
                r_flush = None
            else:
                r_flush = page.evaluate("window.__pdfFlush()")
                check("pdf_flush",
                      r_flush.get("ok") is True
                      and r_flush.get("bytes_appended", 0) > 0
                      and r_flush.get("prefix_bytes", -1) == len(fixture_bytes)
                      and r_flush.get("len", -1) == r_flush.get("prefix_bytes", -2) + r_flush.get("bytes_appended", -2)
                      and r_flush.get("bytes", "").startswith("JVBER"),  # %PDF magic（≥4 字节前缀的 b64 面）
                      json.dumps({k: r_flush.get(k) for k in ("ok", "bytes_appended", "prefix_bytes", "len")},
                                 ensure_ascii=False))

            # ---- 10. pdf_incremental_prefix（首存前缀逐位 == 原字节；零脏二存幂等 +0）----
            if not has_wasm:
                pending_wasm("pdf_incremental_prefix")
            else:
                product1 = base64.b64decode(r_flush["bytes"])
                prefix_ok = product1[: r_flush["prefix_bytes"]] == fixture_bytes
                # 二存：无新增标注 → 零脏幂等（真写器合同：bytes_appended = 0，产物不变）
                r2 = page.evaluate("window.__pdfFlush()")
                product2 = base64.b64decode(r2["bytes"]) if r2.get("ok") else b""
                idem_ok = (r2.get("ok") is True and r2.get("bytes_appended") == 0
                           and product2 == product1
                           and r2.get("prefix_bytes") == len(product1))
                check("pdf_incremental_prefix",
                      r_flush.get("ok") is True and prefix_ok and idem_ok,
                      json.dumps({"prefix_ok": prefix_ok,
                                  "first": {k: r_flush.get(k) for k in ("prefix_bytes", "bytes_appended")},
                                  "second": {k: r2.get(k) for k in ("ok", "prefix_bytes", "bytes_appended")},
                                  "idem_ok": idem_ok}, ensure_ascii=False))

                # 双修订链锚（case 10b 消费）：首存产物的 b64 与解码字节。
                product1 = r_flush.get("bytes", "") if r_flush.get("ok") else ""
                import base64 as _b64
                product1_bytes = _b64.b64decode(product1) if product1 else b""

            # ---- 10b. 双脏修订链（审查 S-3：wasm 会话 + TS 写器复用链的
            #      双修订锁——第二次 flush 的产物以第一次产物为前缀
            #      [链式增量]，且第二修订非零追加）----
            if not has_wasm:
                pending_wasm("pdf_double_revision_chain")
            else:
                r2a = page.evaluate("""() => window.__pdfAddHighlight(1, {x: 100, y: 500, w: 200, h: 20})""")
                f2 = page.evaluate("window.__pdfFlush()")
                # 字节级比对（b64 前缀 != 字节前缀——3 字节块对齐面）。
                import base64 as _b64mod
                p2_bytes = _b64mod.b64decode(f2["bytes"]) if f2.get("ok") and f2.get("bytes") else b""
                check("pdf_double_revision_chain",
                      r2a.get("ok") is True and f2.get("ok") is True
                      and f2.get("bytes_appended", 0) > 0
                      # 链式前缀：product2 以 product1（上一次 flush 产物）为前缀。
                      and p2_bytes[:len(product1_bytes)] == product1_bytes
                      and f2.get("prefix_bytes") == len(product1_bytes)
                      and len(p2_bytes) == len(product1_bytes) + f2.get("bytes_appended", 0),
                      json.dumps({"add": r2a, "flush2": f2,
                                  "prefixLen": len(product1_bytes),
                                  "p2Len": len(p2_bytes)}, ensure_ascii=False))

            # ---- 11. pdf_encrypted_readonly（加密档只读——SKIP 登记）----
            # 造加密夹具需 RC4/标准安全处理器实现（无 pip 依赖不可行）；
            # S5 写拒语义由 mdc-pdf 侧语义锁背书（tests/pdfd_session_flush.rs）。
            check("pdf_encrypted_readonly", True,
                  "SKIP 登记项：加密夹具需标准安全处理器（RC4）——无三方依赖不可造；"
                  "S5 写拒由 Rust 侧语义锁背书", skip=True)

            # ---- 10c. #45 墨迹颜色/线宽（UI 选择器 → 载荷 → 存储往返 →
            #      overlay 视觉态——红 4pt 全链；/C /BS 落盘面由引擎
            #      pdfd_writer 测试背书）。----
            if not has_wasm:
                pending_wasm("pdf_ink_color_width_ui_to_payload")
            else:
                page.select_option("#pdf-ink-color", "#ff0000")
                page.select_option("#pdf-ink-width", "4")
                ink_r = page.evaluate(
                    """() => window.__pdfAddInk(1, [
                        [{x: 80, y: 600}, {x: 200, y: 620}, {x: 300, y: 590}],
                    ])""")
                ink_stored = page.evaluate(
                    """() => {
                        const anns = window.__pdfAnnots();
                        const ink = (anns || []).find((a) => a.kind && a.kind.Ink);
                        const poly = document.querySelector('#pdf-viewport svg.pdf-annot-ink polyline');
                        return {
                            kind: ink ? ink.kind.Ink : null,
                            stroke: poly ? poly.getAttribute('stroke') : null,
                            width: poly ? poly.getAttribute('stroke-width') : null,
                        };
                    }""")
                check("pdf_ink_color_width_ui_to_payload",
                      ink_r.get("ok") is True
                      and ink_stored["kind"] is not None
                      and ink_stored["kind"]["color"] == {"r": 255, "g": 0, "b": 0}
                      and ink_stored["kind"]["width"] == 4
                      and ink_stored["stroke"] == "rgb(255, 0, 0)"
                      and ink_stored["width"] == "4",
                      json.dumps({"add": ink_r, "stored": ink_stored}, ensure_ascii=False))

            # ---- 12. pdf_scan_warnings（垃圾字节 → 扫描面 typed 拒）----
            if not has_wasm:
                pending_wasm("pdf_scan_warnings")
            else:
                bad = page.evaluate(
                    "b64 => window.__pdfOpenBytes(b64)",
                    base64.b64encode(b"not a pdf at all").decode())
                state = page.evaluate("window.__pdfState()")
                err_status = page.evaluate(
                    "!!document.querySelector('#pdf-status .pdf-error')")
                check("pdf_scan_warnings",
                      bad.get("ok") is False and bool(bad.get("code"))
                      and bool(bad.get("message"))
                      and state["open"] is False
                      and err_status,
                      json.dumps({"bad": bad, "state": state, "err_status": err_status},
                                 ensure_ascii=False))
                # 重新打开正常夹具恢复现场（后续共存断言用）
                page.evaluate("b64 => window.__pdfOpenBytes(b64)", fixture_b64)
                page.wait_for_function("window.__pdfPage(1).rendered === true", timeout=20000)

            # ---- 13. pdf_view_coexistence（grid/evidence tab 仍工作 + 零页错）----
            page.click("#tab-grid")
            grid_vis = page.evaluate("""() => ({
                panel: getComputedStyle(document.getElementById('grid-panel')).display,
                editor: getComputedStyle(document.getElementById('editor')).display,
                pdf: getComputedStyle(document.getElementById('pdf-panel')).display,
                evidence: getComputedStyle(document.getElementById('evidence-panel')).display,
            })""")
            page.click("#tab-pdf")
            pdf_vis = page.evaluate("""() => ({
                panel: getComputedStyle(document.getElementById('pdf-panel')).display,
                editor: getComputedStyle(document.getElementById('editor')).display,
                grid: getComputedStyle(document.getElementById('grid-panel')).display,
            })""")
            page.click("#tab-doc")
            doc_vis = page.evaluate("""() => ({
                editor: getComputedStyle(document.getElementById('editor')).display,
                grid: getComputedStyle(document.getElementById('grid-panel')).display,
                pdf: getComputedStyle(document.getElementById('pdf-panel')).display,
                gridReady: window.__gridReady === true,
                evReady: window.__evReady === true,
                pdfReady: window.__pdfReady === true,
                contract: window.__contractVersion,
                md: typeof window.__md,
            })""")
            check("pdf_view_coexistence",
                  grid_vis["panel"] != "none" and grid_vis["editor"] == "none"
                  and grid_vis["pdf"] == "none" and grid_vis["evidence"] != "none"
                  and pdf_vis["panel"] != "none" and pdf_vis["editor"] == "none"
                  and pdf_vis["grid"] == "none"
                  and doc_vis["editor"] != "none" and doc_vis["grid"] == "none"
                  and doc_vis["pdf"] == "none"
                  and doc_vis["gridReady"] and doc_vis["evReady"] and doc_vis["pdfReady"]
                  and doc_vis["contract"] == 1.4 and doc_vis["md"] == "function"
                  and len(page_errors) == 0,
                  json.dumps({"grid": grid_vis, "pdf": pdf_vis, "doc": doc_vis,
                              "pageErrors": page_errors[:3]}, ensure_ascii=False))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "phase 5 §8.4 pdf view e2e", "dist": str(WEB / "dist"),
                 "fixture": str(FIXTURE), "fixture_bytes": len(fixture_bytes),
                 "pkg_pdf_present": has_wasm},
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
