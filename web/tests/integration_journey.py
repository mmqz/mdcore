#!/usr/bin/env python3
"""Four-surface integration journey e2e（阶段 6 §九 启动件——实际使用路径）.

单页用户旅程串联四表面（正文/表格/证据/PDF/Canvas），验证跨表面
共存与真实互操作（§九.1-4 骨架项）：
  1. 文档面：CM6 桥就绪 + 编辑往返（revision 递增）。
  2. 表格面：打开 xlsx → 虚拟滚动 → 编辑 → 读改重建导出。
  3. 证据面：正文+来源导入 → 双关联 → 计算 → 审核 → sidecar 往返。
  4. PDF 面：打开 → 渲染 → 文本选区 → 高亮 → 增量保存。
  5. Canvas 面：节点/边 → 拖拽 → 双实例协同收敛。
  6. 跨表面共存：四 tab 轮转 + 状态互不干扰 + 零 pageerror。

全部走既有面板的真实探针面（不新增产品代码）——这是验收测试，
不是功能测试。
"""

import base64
import http.server
import json
import os
import socketserver
import sys
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT / "dist"
FIXTURES = ROOT / "tests" / "fixtures"
PORT = 8941
RESULTS = ROOT / "tests" / "results" / "results_integration_journey.json"


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
    }


def serve() -> socketserver.TCPServer:
    os.chdir(DIST)  # 服务 root = dist/（产物自包含）
    socketserver.TCPServer.allow_reuse_address = True
    srv = socketserver.TCPServer(("127.0.0.1", PORT), Quiet)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


def main() -> int:
    from playwright.sync_api import sync_playwright

    checks: list[dict] = []

    def check(name: str, ok: bool, detail: str = "", skip: bool = False) -> None:
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip), "detail": detail[:400]})
        print(("SKIP " if skip else ("PASS " if ok else "FAIL ")) + name)

    srv = serve()
    page_errors: list[str] = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch()
        page = browser.new_page()
        page.on("pageerror", lambda e: page_errors.append(str(e)))

        page.goto(f"http://127.0.0.1:{PORT}/index.html")
        page.wait_for_function("window.__ready === true", timeout=60000)
        # 四表面懒加载异步完成（pdf/canvas 的 wasm 桥在 setup 异步尾置 ready）
        page.wait_for_function(
            "[window.__gridReady, window.__evReady, window.__pdfReady, window.__canvasReady]"
            ".every(x => x === true)",
            timeout=60000,
        )

        # ── 0. 全表面就绪 ──────────────────────────────────────────
        flags = page.evaluate(
            "[window.__gridReady, window.__evReady, window.__pdfReady, window.__canvasReady]"
        )
        check("journey_all_surfaces_ready", all(flags), json.dumps(flags))

        # ── 1. 文档面：编辑往返 ────────────────────────────────────
        rev0 = page.evaluate("window.__core.revision()")
        page.click("#editor .cm-content")
        page.keyboard.type("integration journey ")
        page.wait_for_timeout(300)
        rev1 = page.evaluate("window.__core.revision()")
        md = page.evaluate("window.__core.to_markdown()")
        check(
            "journey_doc_edit_roundtrip",
            int(rev1) > int(rev0) and "integration journey" in md,
            json.dumps({"rev0": str(rev0), "rev1": str(rev1)}),
        )

        # ── 2. 表格面：打开→滚动→编辑→导出 ────────────────────────
        page.click("#tab-grid")
        page.click("#grid-viewport")  # 键盘面 focus（导航/编辑前提）
        xlsx_b64 = base64.b64encode((FIXTURES / "grid_sample.xlsx").read_bytes()).decode()
        opened = page.evaluate("b => window.__gridOpenBytes(b)", xlsx_b64)
        page.evaluate(
            "() => { document.getElementById('grid-viewport').scrollTop = 2000; }"
        )
        page.wait_for_timeout(400)
        vis = page.evaluate("window.__gridVisible()")
        page.evaluate("window.__gridSetSelection(5, 5)")
        page.keyboard.press("Enter")
        page.keyboard.type("journey-edit")
        page.keyboard.press("Enter")
        edits = page.evaluate("window.__gridEdits()")
        exported = page.evaluate("window.__gridExportEdits()")
        check(
            "journey_grid_open_scroll_edit_export",
            opened.get("ok") is True
            and vis.get("r0", 0) > 20
            and len(edits) == 1
            and edits[0].get("v") == "journey-edit"
            and exported.get("ok") is True
            and exported.get("bytes", "").startswith("UEs"),
            json.dumps({"opened": opened.get("ok"), "r0": vis.get("r0"), "edits": len(edits)}),
        )

        # ── 3. 证据面：八步骨架（导入→关联→计算→审核→sidecar）───
        page.click("#tab-doc")
        body_b64 = base64.b64encode((FIXTURES / "evidence_body.docx").read_bytes()).decode()
        src_b64 = base64.b64encode((FIXTURES / "evidence_sample.xlsx").read_bytes()).decode()
        started = page.evaluate("b => window.__evStartBody(b)", body_b64)
        ingested = page.evaluate("b => window.__evIngestSource(b)", src_b64)
        b1 = page.evaluate(
            """() => window.__evProposeBinding(JSON.stringify({
                evidence_id: 'j-e1', claim: '单价 12.5 元，数量 336.8。',
                claimed_value: '12.5', paragraph_index: 2,
                source_doc_id: 'src-1', sheet: 'Sheet1', coord: 'B7',
                relation: 'direct_quote', calc_ref: null, generation_batch: 'j1'}))"""
        )
        c1 = page.evaluate(
            """() => window.__evProposeCalc(JSON.stringify({
                calc_id: 'j-c1', expression: 'qty * price', unit: null,
                precision: {target_scale: 3, rounding: 'half_up'},
                inputs: [
                  {name: 'qty', source: {cell: {doc_id: 'src-1', sheet: 'Sheet1', coord: 'B7'}}},
                  {name: 'price', source: {cell: {doc_id: 'src-1', sheet: 'Sheet1', coord: 'C7'}}}]}))"""
        )
        reviewed = page.evaluate("window.__evReview('j-e1', 'approved')")
        sc = page.evaluate("window.__evSidecar()")
        sc_parsed = json.loads(sc)
        check(
            "journey_evidence_eight_step_skeleton",
            started.get("ok") is True
            and ingested.get("ok") is True
            and b1.get("ok") is True
            and c1.get("ok") is True
            and c1.get("value") == "4210.000"
            and reviewed.get("ok") is True
            and len(sc_parsed.get("evidence", [])) == 1
            and sc_parsed.get("schema_version") == 1,
            json.dumps(
                {
                    "start": started.get("ok"),
                    "ingest": ingested.get("ok"),
                    "bind": b1.get("ok"),
                    "calc": c1.get("value"),
                    "review": reviewed.get("ok"),
                }
            ),
        )

        # ── 4. PDF 面：打开→选区→高亮→增量保存 ────────────────────
        # pkg_pdf 缺席（issue #5——display-only）时注释/增量保存面
        # bridge_absent：pdf_view.py 同款 SKIP 纪律（模型面就绪后恒跑）。
        page.click("#tab-pdf")
        pdf_b64 = base64.b64encode((FIXTURES / "sample.pdf").read_bytes()).decode()
        pdf_open = page.evaluate("b => window.__pdfOpenBytes(b)", pdf_b64)
        page.wait_for_timeout(1200)
        p_state = page.evaluate("window.__pdfState()")
        if p_state.get("wasm") is not True:
            check("journey_pdf_annotate_incremental_save", True,
                  "pending wasm (pkg_pdf absent — issue #5 display-only; "
                  "annotate/flush bridge_absent by design)", skip=True)
        else:
            annot = page.evaluate(
                "() => window.__pdfAddHighlight(0, {x: 72, y: 700, w: 300, h: 24})"
            )
            flush = page.evaluate("window.__pdfFlush()")
            check(
                "journey_pdf_annotate_incremental_save",
                pdf_open.get("ok") is True
                and p_state.get("pages") == 3
                and annot.get("ok") is True
                and flush.get("ok") is True
                and flush.get("bytes_appended", 0) > 0
                and flush.get("prefix_bytes") == len(base64.b64decode(pdf_b64)),
                json.dumps(
                    {
                        "open": pdf_open.get("ok"),
                        "pages": p_state.get("pages"),
                        "annot": annot.get("ok"),
                        "appended": flush.get("bytes_appended"),
                    }
                ),
            )

        # ── 5. Canvas 面：建→拖→协同收敛 ──────────────────────────
        page.click("#tab-canvas")
        n1 = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)", [40, 60, "J1"])
        n2 = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)", [200, 60, "J2"])
        moved = page.evaluate("([id, x, y]) => window.__canvasMoveNode(id, x, y)", [n1["id"], 80, 120])
        ai_added = page.evaluate("window.__canvasAiAddNode()")
        sync = page.evaluate("window.__canvasSyncState()")
        objs = page.evaluate("window.__canvasObjects()")
        check(
            "journey_canvas_edit_and_sync",
            n1.get("ok") is True
            and n2.get("ok") is True
            and moved.get("ok") is True
            and ai_added.get("ok") is True
            and sync.get("converged") is True
            and len(objs) >= 3,
            json.dumps(
                {
                    "n1": n1.get("ok"),
                    "n2": n2.get("ok"),
                    "move": moved.get("ok"),
                    "ai": ai_added.get("ok"),
                    "converged": sync.get("converged"),
                    "objects": len(objs),
                }
            ),
        )

        # ── 6. 跨表面共存：四 tab 轮转 + 互不干扰 + 零页错 ─────────
        page.click("#tab-doc")
        page.click("#tab-grid")
        page.click("#tab-pdf")
        page.click("#tab-canvas")
        page.click("#tab-doc")
        final_grid = page.evaluate("window.__gridInfo ? window.__gridInfo() : null")
        final_ev = page.evaluate("window.__evState()")
        final_pdf = page.evaluate("window.__pdfState()")
        final_canvas = page.evaluate("window.__canvasState()")
        check(
            "journey_cross_surface_coexistence",
            final_grid is not None
            and final_ev.get("evidence") == 1
            and final_pdf.get("pages") == 3
            and final_canvas.get("objects") >= 3
            and len(page_errors) == 0,
            json.dumps(
                {
                    "grid": final_grid is not None,
                    "ev": final_ev.get("evidence"),
                    "pdf": final_pdf.get("pages"),
                    "canvas": final_canvas.get("objects"),
                    "pageErrors": page_errors[:2],
                }
            ),
        )

        browser.close()
    srv.shutdown()

    failed = sum(1 for c in checks if not c["ok"] and not c["skip"])
    RESULTS.parent.mkdir(parents=True, exist_ok=True)
    RESULTS.write_text(
        json.dumps(
            {
                "meta": {"port": PORT, "total": len(checks), "failed": failed},
                "tests": checks,
                "summary": {"total": len(checks), "failed": failed, "pass": failed == 0},
            },
            ensure_ascii=False,
            indent=2,
        )
    )
    print(f"\n{len(checks) - failed}/{len(checks)} checks passed -> {RESULTS}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
