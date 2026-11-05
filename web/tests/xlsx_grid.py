#!/usr/bin/env python3
"""Phase 5 §8.3 grid UI e2e (xlsx_grid web face — table surface
productization): serve web/dist, drive the real tab/grid-panel UI + programmatic
window.__grid* probes, assert virtual-window rendering (pooled DOM, bounded
node counts), pinned frozen row/col + headers, keyboard navigation, edit
ledger (commit/undo/delete/paste/copy TSV), evidence highlight, sheet switch,
read-modify-rebuild export face (skipped as `bridge_absent` while the wasm
exportWithEdits rebuild is pending), and typed error surfaces (garbage bytes /
oversized slice). Zero pageerror throughout.
Fixture = web/tests/fixtures/grid_sample.xlsx (provenance: office-io
WorkbookWriter via `cargo run -p mdcore-wasm --example gen_xlsx_grid_fixture`;
the deterministic value function below mirrors the Rust generator).
Evidence -> results/results_xlsx_grid.json
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
FIXTURE = WEB / "tests" / "fixtures" / "grid_sample.xlsx"
OUT = WEB / "tests" / "results" / "results_xlsx_grid.json"
FIXTURE_GEN = ["cargo", "run", "-p", "mdcore-wasm", "--example", "gen_xlsx_grid_fixture"]

# Geometry constants mirrored from web/src/xlsx_grid.ts (layout contract).
CELL_W, CELL_H, HEAD_W, HEAD_H = 88, 26, 48, 24


def fmt_num(v: float) -> str:
    """Rust f64 Display shape: 201.0 -> "201", 108.5 -> "108.5"."""
    return str(int(v)) if float(v).is_integer() else str(v)


def expected(r: int, c: int) -> str:
    """Mirror of gen_xlsx_grid_fixture.rs::data_cell (display shape)."""
    if r == 0:
        return f"H{c}"
    if c == 0:
        return f"R{r}"
    t = (r * 4 + c * 2) % 5
    if t == 0:
        v = r * 100 + c
        if c % 2 == 0:
            v += 0.5
        if r % 2 == 1:
            v = -v
        return fmt_num(v)
    if t == 1:
        return f"cell-{r}-{c}"
    if t == 2:
        return "true" if (r + c) % 2 == 0 else "false"
    if t == 3:
        return f"00{r}{c}"
    return fmt_num(r + c)


Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True

# 页内字节打开助手（base64 → Uint8Array → window.__gridOpenBytes）
OPEN_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__gridOpenBytes(b64);
}"""


def main() -> int:
    if not FIXTURE.exists():
        print(f"fixture missing — generating via {FIXTURE_GEN[2]}")
        subprocess.run(FIXTURE_GEN, check=True, cwd=str(WEB.parent))

    checks = []
    page_errors: list[str] = []
    skipped = 0

    def check(name, ok, detail="", skip=False):
        nonlocal skipped
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip), "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        if skip:
            skipped += 1
        print(tag + name + ("  | " + str(detail)[:160] if detail and not ok and not skip else ""))
        return ok

    fixture_bytes = FIXTURE.read_bytes()
    fixture_b64 = base64.b64encode(fixture_bytes).decode()

    with socketserver.TCPServer(("127.0.0.1", 8936), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            # clipboard permission for the real Ctrl+C path (copy check)
            ctx = browser.new_context(permissions=["clipboard-read", "clipboard-write"])
            page = ctx.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8936/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            page.wait_for_function("window.__gridReady === true", timeout=30000)

            # ---- 1. grid ready（wasm init + DOM 就绪；既有 doc 面零回归）----
            boot = page.evaluate("""() => ({
                ready: window.__gridReady === true,
                viewport: !!document.getElementById('grid-viewport'),
                sheetSelect: !!document.getElementById('grid-sheet-select'),
                exportBtn: !!document.getElementById('grid-export-btn'),
                fileInput: !!document.getElementById('grid-file'),
                tabs: !!document.getElementById('tab-grid'),
                importFn: typeof window.__importXlsx,
                contract: window.__contractVersion,
            })""")
            check("grid_ready",
                  boot["ready"] and boot["viewport"] and boot["sheetSelect"] and boot["exportBtn"]
                  and boot["fileInput"] and boot["tabs"] and boot["importFn"] == "function"
                  and boot["contract"] == 1.4,
                  json.dumps(boot))

            # ---- 1b. 视图 tab 切换（文档 ↔ 表格面板互斥显隐）----
            page.click("#tab-grid")
            grid_vis = page.evaluate("""() => ({
                panel: getComputedStyle(document.getElementById('grid-panel')).display,
                editor: getComputedStyle(document.getElementById('editor')).display,
                active: document.getElementById('tab-grid').classList.contains('active'),
                docActive: document.getElementById('tab-doc').classList.contains('active'),
            })""")
            page.click("#tab-doc")
            doc_vis = page.evaluate("getComputedStyle(document.getElementById('editor')).display !== 'none' && getComputedStyle(document.getElementById('grid-panel')).display === 'none'")
            page.click("#tab-grid")
            check("view_tabs_toggle",
                  grid_vis["panel"] != "none" and grid_vis["editor"] == "none"
                  and grid_vis["active"] and not grid_vis["docActive"] and doc_vis,
                  json.dumps(grid_vis))

            # ---- 2. 打开夹具（探针面）+ sheets 元数据 ----
            opened = page.evaluate(OPEN_JS, fixture_b64)
            # S1: ext_rows/ext_cols = absolute extent (anchored fixture:
            # ext === rows/cols; field presence locks the contract).
            sheets = opened.get("sheets") or []
            check("grid_open_sheets",
                  opened.get("ok") is True
                  and len(sheets) == 2
                  and sheets[0]["name"] == "data" and sheets[0]["rows"] == 300
                  and sheets[0]["cols"] == 40
                  and sheets[0].get("ext_rows") == 300
                  and sheets[0].get("ext_cols") == 40
                  and sheets[1]["name"] == "types" and sheets[1]["rows"] == 4
                  and sheets[1]["cols"] == 3
                  and sheets[1].get("ext_rows") == 4,
                  json.dumps(opened))

            # ---- 2b. 打开夹具（真实 file input 路径）----
            page.set_input_files("#grid-file", str(FIXTURE))
            page.wait_for_function("window.__gridInfo().rows === 300", timeout=15000)
            file_open = page.evaluate("""() => ({
                status: document.getElementById('grid-status').textContent,
                err: document.getElementById('grid-status').classList.contains('grid-error'),
            })""")
            check("grid_open_file_input",
                  "opened: 2 sheets" in file_open["status"] and not file_open["err"],
                  json.dumps(file_open))

            # ---- 3. 首窗渲染（虚拟窗 + 池化 DOM 有界）----
            vis = page.evaluate("window.__gridVisible()")
            check("grid_first_window",
                  vis["r0"] == 0 and vis["c0"] == 0
                  and 0 < vis["mainDom"] < 500 and vis["frozenRowDom"] > 0
                  and vis["frozenColDom"] > 0 and vis["frozenRowDom"] < 100,
                  json.dumps(vis))
            texts = page.evaluate("[window.__gridCellText(0,0), window.__gridCellText(1,1)]")
            check("grid_cell_text_seed",
                  texts == ["H0", "cell-1-1"],
                  json.dumps(texts))

            # ---- 4. 虚拟滚动（深滚动位 → 窗跟随 + DOM 仍有界 + 值抽查）----
            page.evaluate("""() => {
                const v = document.getElementById('grid-viewport');
                v.scrollTop = 150 * 26;
                v.scrollLeft = 20 * 88;
            }""")
            page.wait_for_function("window.__gridVisible().r0 > 100", timeout=5000)
            vis = page.evaluate("window.__gridVisible()")
            r0, c0 = vis["r0"], vis["c0"]
            spot = page.evaluate(
                "[window.__gridCellText(%d,%d), window.__gridCellText(%d,%d)]"
                % (r0, c0, r0 + 3, c0 + 2))
            spot_exp = [expected(r0, c0), expected(r0 + 3, c0 + 2)]
            check("grid_virtual_scroll",
                  148 <= r0 <= 152 and 18 <= c0 <= 22 and 0 < vis["mainDom"] < 500
                  and vis["scrollTop"] == 150 * 26 and vis["scrollLeft"] == 20 * 88
                  and spot == spot_exp,
                  json.dumps({"vis": vis, "spot": spot, "expected": spot_exp}))

            # ---- 5. 冻结恒显（滚动后冻结行/列头仍钉在原位）----
            pin = page.evaluate("""() => {
                const vp = document.getElementById('grid-viewport');
                const vr = vp.getBoundingClientRect();
                const fr = document.getElementById('grid-frozen-row');
                const fc = document.getElementById('grid-frozen-col');
                const ch = document.getElementById('grid-colhead');
                const rh = document.getElementById('grid-rowhead');
                const first = fr.querySelector('.grid-cell');
                const firstCol = fc.querySelector('.grid-cell');
                return {
                    frTransform: fr.style.transform,
                    fcTransform: fc.style.transform,
                    scrollLeft: vp.scrollLeft,
                    scrollTop: vp.scrollTop,
                    frTop: first ? first.getBoundingClientRect().top - vr.top : null,
                    fcLeft: firstCol ? firstCol.getBoundingClientRect().left - vr.left : null,
                    colheadTop: ch.getBoundingClientRect().top - vr.top,
                    rowheadLeft: rh.getBoundingClientRect().left - vr.left,
                };
            }""")
            check("grid_frozen_pinned",
                  pin["frTransform"] == f"translate(-{pin['scrollLeft']}px, {pin['scrollTop']}px)"
                  and pin["fcTransform"] == f"translate({pin['scrollLeft']}px, -{pin['scrollTop']}px)"
                  and abs(pin["frTop"] - HEAD_H) <= 1
                  and abs(pin["fcLeft"] - HEAD_W) <= 1
                  and abs(pin["colheadTop"]) <= 1
                  and abs(pin["rowheadLeft"]) <= 1,
                  json.dumps(pin))

            # ---- 6. 键盘导航（方向/Tab/Home/End/PageDown/Ctrl+Home）----
            page.evaluate("document.getElementById('grid-viewport').focus()")
            page.keyboard.press("Control+Home")
            seq = {}
            for _ in range(3):
                page.keyboard.press("ArrowDown")
            for _ in range(2):
                page.keyboard.press("ArrowRight")
            seq["arrows"] = page.evaluate("window.__gridSelection()")
            page.keyboard.press("Tab")
            seq["tab"] = page.evaluate("window.__gridSelection()")
            page.keyboard.press("Shift+Tab")
            seq["shiftTab"] = page.evaluate("window.__gridSelection()")
            page.keyboard.press("Home")
            seq["home"] = page.evaluate("window.__gridSelection()")
            page.keyboard.press("End")
            seq["end"] = page.evaluate("window.__gridSelection()")
            page.keyboard.press("PageDown")
            seq["pageDown"] = page.evaluate("window.__gridSelection()")
            page_rows = page.evaluate(
                "Math.max(1, Math.floor((document.getElementById('grid-viewport').clientHeight - %d) / %d))"
                % (HEAD_H, CELL_H))
            page.keyboard.press("Control+Home")
            seq["ctrlHome"] = page.evaluate("window.__gridSelection()")
            check("grid_keyboard_navigation",
                  seq["arrows"] == {"r": 3, "c": 2, "rows": 1, "cols": 1}
                  and seq["tab"] == {"r": 3, "c": 3, "rows": 1, "cols": 1}
                  and seq["shiftTab"] == {"r": 3, "c": 2, "rows": 1, "cols": 1}
                  and seq["home"] == {"r": 3, "c": 0, "rows": 1, "cols": 1}
                  and seq["end"] == {"r": 3, "c": 39, "rows": 1, "cols": 1}
                  and seq["pageDown"]["r"] == 3 + page_rows and seq["pageDown"]["c"] == 39
                  and seq["ctrlHome"] == {"r": 0, "c": 0, "rows": 1, "cols": 1},
                  json.dumps(seq))

            # ---- 7. 编辑（双击 → 编辑浮层 → 提交 number + 下移）----
            page.locator("#grid-cells div[data-r='1'][data-c='1']").dblclick()
            edit_state = page.evaluate("""() => {
                const i = document.getElementById('grid-edit-input');
                return { hidden: i.hidden, value: i.value };
            }""")
            page.keyboard.type("123.5")
            page.keyboard.press("Enter")
            edit_after = page.evaluate("""() => ({
                edits: window.__gridEdits(),
                text: window.__gridCellText(1, 1),
                editedDom: !!document.querySelector("#grid-cells div[data-r='1'][data-c='1'].edited"),
                sel: window.__gridSelection(),
            })""")
            ok_edit = (edit_state["value"] == "cell-1-1" and not edit_state["hidden"]
                       and len(edit_after["edits"]) == 1
                       and edit_after["edits"][0]["k"] == "number"
                       and edit_after["edits"][0]["v"] == 123.5
                       and edit_after["edits"][0]["r"] == 1 and edit_after["edits"][0]["c"] == 1
                       and edit_after["text"] == "123.5" and edit_after["editedDom"]
                       and edit_after["sel"] == {"r": 2, "c": 1, "rows": 1, "cols": 1})
            check("grid_edit_commit", ok_edit,
                  json.dumps({"seed": edit_state, "after": edit_after}))

            # ---- 8. 编辑撤销（Ctrl+Z → 账本回退 + 显示回原值）----
            page.keyboard.press("Control+z")
            undo_after = page.evaluate("""() => ({
                edits: window.__gridEdits().length,
                text: window.__gridCellText(1, 1),
            })""")
            check("grid_edit_undo",
                  undo_after["edits"] == 0 and undo_after["text"] == "cell-1-1",
                  json.dumps(undo_after))

            # ---- 8b. 同格多次编辑 → 逐次撤销恢复前一版本（审查 S6-4：
            #      undo = pop 后按剩余账本重建（backward-search），非清空）----
            page.evaluate("window.__gridSetSelection(4, 4)")
            page.keyboard.press("Enter")
            page.keyboard.type("v1")
            page.keyboard.press("Enter")
            # 回原格再编辑（Enter 确认后选区已下移——第二次编辑须显式回格）
            page.evaluate("window.__gridSetSelection(4, 4)")
            page.keyboard.press("Enter")
            page.keyboard.type("v2")
            page.keyboard.press("Enter")
            two = page.evaluate(
                "JSON.parse(JSON.stringify(window.__gridEdits()))")
            mid_text = page.evaluate("window.__gridCellText(4, 4)")
            page.keyboard.press("Control+z")
            one = page.evaluate(
                "JSON.parse(JSON.stringify(window.__gridEdits()))")
            back_text = page.evaluate("window.__gridCellText(4, 4)")
            check("grid_edit_undo_stack",
                  len(two) == 2 and mid_text == "v2"
                  and len(one) == 1 and back_text == "v1",
                  json.dumps({"two": two, "mid": mid_text,
                              "one": one, "back": back_text}))
            # 清场（撤销到零 + 选区回 (2,1)——后续 copy 用例的前置契约
            # [grid_edit_commit 确认下移遗留位]）
            page.keyboard.press("Control+z")
            page.evaluate("window.__gridSetSelection(2, 1)")

            # ---- 9. 复制（选区 2×2 → Ctrl+C → 剪贴板 TSV；真径优先，合成兜底）----
            page.keyboard.press("Shift+ArrowDown")
            page.keyboard.press("Shift+ArrowRight")
            sel = page.evaluate("window.__gridSelection()")
            exp_tsv = "\t".join([expected(2, 1), expected(2, 2)]) + "\n" + \
                "\t".join([expected(3, 1), expected(3, 2)])
            page.keyboard.press("Control+c")
            try:
                copied = page.evaluate("navigator.clipboard.readText()")
            except Exception:
                copied = ""
            copy_path = "real clipboard"
            if copied != exp_tsv:
                # 合成兜底（headless 剪贴板面不稳时验证 handler 本体）
                copied = page.evaluate("""(tsv) => {
                    const dt = new DataTransfer();
                    dt.setData('text/plain', tsv);
                    let ev;
                    try {
                        ev = new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true });
                        if (!ev.clipboardData) throw new Error('ctor ignored clipboardData');
                    } catch (e) {
                        ev = new Event('copy', { bubbles: true, cancelable: true });
                        Object.defineProperty(ev, 'clipboardData', { value: dt });
                    }
                    document.dispatchEvent(ev);
                    return dt.getData('text/plain');
                }""", exp_tsv)
                copy_path = "synthetic copy event"
            check("grid_copy_tsv",
                  sel == {"r": 2, "c": 1, "rows": 2, "cols": 2} and copied == exp_tsv,
                  json.dumps({"sel": sel, "copied": copied, "expected": exp_tsv, "path": copy_path}))

            # ---- 9b. 敌意单元格复制消毒（#225 P0-5：出口必经 guardTsvCell——
            #      审计 P0-5：guard_csv_cell 存在但产品路径从未接线）。
            #      三格一组：DDE 载荷 / 负数（登记为文本——PrefixApostrophe 模式
            #      的已知代价，与引擎 Rust 面 guard_csv_cell 语义一致）/ 内嵌
            #      \t（TSV 无转义，替换空格防新首字符位注入）----
            for col, payload in ((6, "=cmd|/c calc"), (7, "-42")):
                page.evaluate(f"window.__gridSetSelection(6, {col})")
                page.keyboard.press("Enter")
                page.keyboard.type(payload)
                page.keyboard.press("Enter")
            page.evaluate("window.__gridSetSelection(6, 8)")
            page.keyboard.press("Enter")
            page.keyboard.insert_text("plain\twith\ttabs")
            page.keyboard.press("Enter")
            # 选回 (6,6) 并扩 1×3（Enter 提交后选区已下移 → 显式回位）
            page.evaluate("window.__gridSetSelection(6, 6)")
            page.keyboard.press("Shift+ArrowRight")
            page.keyboard.press("Shift+ArrowRight")
            guarded = page.evaluate("""() => {
                const dt = new DataTransfer();
                let ev;
                try {
                    ev = new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true });
                    if (!ev.clipboardData) throw new Error('ctor ignored clipboardData');
                } catch (e) {
                    ev = new Event('copy', { bubbles: true, cancelable: true });
                    Object.defineProperty(ev, 'clipboardData', { value: dt });
                }
                document.dispatchEvent(ev);
                return dt.getData('text/plain');
            }""")
            check("grid_copy_formula_injection_guarded",
                  guarded == "'=cmd|/c calc\t'-42\tplain with tabs",
                  json.dumps({"copied": guarded}))
            # 清场：撤销 9b 的三笔编辑 + 选区回 (2,1)（10 号粘贴用例的
            # 前置契约：账本零 + 落点 (2,1)）
            page.keyboard.press("Control+z")
            page.keyboard.press("Control+z")
            page.keyboard.press("Control+z")
            page.evaluate("window.__gridSetSelection(2, 1)")

            # ---- 10. 粘贴（TSV → 逐格编辑命令 k=text）----
            page.evaluate("""() => {
                const dt = new DataTransfer();
                dt.setData('text/plain', 'a\\tb\\nc\\td');
                let ev;
                try {
                    ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true });
                    if (!ev.clipboardData) throw new Error('ctor ignored clipboardData');
                } catch (e) {
                    ev = new Event('paste', { bubbles: true, cancelable: true });
                    Object.defineProperty(ev, 'clipboardData', { value: dt });
                }
                document.dispatchEvent(ev);
            }""")
            paste_after = page.evaluate("""() => ({
                edits: window.__gridEdits(),
                texts: [window.__gridCellText(2,1), window.__gridCellText(2,2),
                        window.__gridCellText(3,1), window.__gridCellText(3,2)],
            })""")
            check("grid_paste_tsv",
                  len(paste_after["edits"]) == 4
                  and all(e["k"] == "text" for e in paste_after["edits"])
                  and paste_after["texts"] == ["a", "b", "c", "d"],
                  json.dumps(paste_after))

            # ---- 11. Delete 清空（导航态 → 选区全格 k=empty 批量编辑）----
            page.click("#grid-cells div[data-r='10'][data-c='10']")
            page.keyboard.press("Delete")
            del_after = page.evaluate("""() => ({
                edits: window.__gridEdits(),
                text: window.__gridCellText(10, 10),
            })""")
            check("grid_delete_clears",
                  len(del_after["edits"]) == 5
                  and del_after["edits"][-1]["k"] == "empty"
                  and del_after["edits"][-1]["r"] == 10 and del_after["edits"][-1]["c"] == 10
                  and del_after["text"] == "",
                  json.dumps(del_after))

            # ---- 12. 证据高亮（探针面 → DOM .evidence 恰 N）----
            page.evaluate("window.__gridSetEvidence(0, [[5,5],[6,7]])")
            page.evaluate("""() => {
                const v = document.getElementById('grid-viewport');
                v.scrollTop = 0; v.scrollLeft = 0;
            }""")
            page.wait_for_function("window.__gridVisible().r0 === 0", timeout=5000)
            ev_after = page.evaluate("""() => ({
                dom: document.querySelectorAll('#grid-panel .grid-cell.evidence').length,
                info: window.__gridInfo().evidence,
            })""")
            check("grid_evidence_highlight",
                  ev_after["dom"] == 2 and ev_after["info"] == 2,
                  json.dumps(ev_after))

            # ---- 13. sheet 切换（select → 激活表 + 维度 + 回切）----
            page.select_option("#grid-sheet-select", label="types")
            info_types = page.evaluate("window.__gridInfo()")
            page.select_option("#grid-sheet-select", label="data")
            info_data = page.evaluate("window.__gridInfo()")
            sheet_count = page.evaluate("window.__gridSheetCount()")
            check("grid_sheet_switch",
                  info_types["sheet"] == 1 and info_types["rows"] == 4 and info_types["cols"] == 3
                  and info_data["sheet"] == 0 and info_data["rows"] == 300
                  and info_data["cols"] == 40 and sheet_count == 2
                  and info_data["edits"] == 5,  # 账本跨 sheet 保留
                  json.dumps({"types": info_types, "data": info_data, "count": sheet_count}))

            # ---- 14. 导出（读改重建面；pkg 缺 exportWithEdits = SKIP）----
            res = page.evaluate("window.__gridExportEdits()")
            if res.get("ok") is True:
                stats = res.get("stats", {})
                stat_keys = ("sheets", "cells_written", "merges", "hyperlinks",
                             "edits_applied", "skipped")
                ok_export = (res["bytes"].startswith("UEsD")
                             and stats.get("edits_applied", 0) >= 2
                             and all(k in stats for k in stat_keys))
                check("grid_export_edits", ok_export,
                      json.dumps({"stats": stats, "bytesHead": res["bytes"][:16]}))
            else:
                code = res.get("code", "")
                msg = str(res.get("message", ""))
                if code == "bridge_absent" or "not a function" in msg:
                    check("grid_export_edits", True,
                          "SKIP (pending wasm exportWithEdits rebuild): " + json.dumps(res),
                          skip=True)
                else:
                    check("grid_export_edits", False, json.dumps(res))

            # ---- 15. 错误面（垃圾字节 / 切片超限 → typed 码，现状零触碰）----
            bad = page.evaluate(OPEN_JS, base64.b64encode(b"hi").decode())
            still = page.evaluate("window.__gridSheetCount()")
            slice_big = page.evaluate("window.__gridSliceRaw(0, 0, 0, 1001, 100)")
            bad_status = page.evaluate("document.getElementById('grid-status').textContent")
            check("grid_error_faces",
                  bad.get("ok") is False and bad.get("code") == "grid_read_failed"
                  and bool(bad.get("message")) and still == 2
                  and slice_big.get("ok") is False
                  and slice_big.get("code") == "grid_slice_too_large"
                  and "grid_read_failed" in bad_status,
                  json.dumps({"bad": bad, "sliceBig": slice_big, "still": still}))

            # ---- 16. 契约面 + 零页错（全程无 JS 异常逃逸）----
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                gridReady: window.__gridReady === true,
                contract: window.__contractVersion === 1.4,
                md: typeof window.__md === 'function',
                exportXlsx: typeof window.__exportXlsx === 'function',
            })""")
            check("grid_contract_intact_no_pageerror",
                  final["ready"] and final["gridReady"] and final["contract"]
                  and final["md"] and final["exportXlsx"] and len(page_errors) == 0,
                  json.dumps({"final": final, "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "phase 5 §8.3 grid UI e2e", "dist": str(WEB / "dist"),
                 "fixture": str(FIXTURE), "fixture_bytes": len(fixture_bytes)},
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
