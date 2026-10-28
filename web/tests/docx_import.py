#!/usr/bin/env python3
"""t16 DOCX import e2e (office-io -> mdc-wasm -> web editor, first office
consumption path through the wasm bridge): serve web/dist, drive the real
file-input UI path + programmatic window.__importDocx, assert happy-path
structure mapping, typed error surfacing on hostile/truncated input,
live-editor editability of the imported doc, and contract surface intact.
Fixture = web/tests/fixtures/import_sample.docx (provenance: office-io
DocxWriter via `cargo run -p mdcore-wasm --example gen_docx_fixture`).
Evidence -> results/results_docx.json
"""

import base64
import http.server
import json
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURE = WEB / "tests" / "fixtures" / "import_sample.docx"
OUT = WEB / "tests" / "results" / "results_docx.json"

# 导入后期望的编辑器文档：核心树规范形（to_markdown 按行 join——空行
# 在 ingestion 时被跳过，块间无空行；导入串的 \n\n 分隔是宿主可读性面，
# 编辑器 canonical 化后即单行分隔）。r5-t1：列表节（有序×2 + 嵌套
# ilvl1 → 2 空格缩进 + 无序×2）进期望串——numbering 读面 → markdown
# 列表映射的消费面证据。P2-1（#22）：页眉前导（来源标记段 +
# Header Slot）+ 页脚尾接（标记段 + Footer Slot）——页眉/页脚内容
# 接入导入输出（headers/footers_imported = 1/1，skipped 恒 0）。
EXPECTED_MD = (
    "<!-- mdcore-import: header:default -->\n"
    "Header Slot\n"
    "# Imported Title\n"
    "plain **bold** and *italic* and [docs](https://example.com/docs)\n"
    "| Name | Value |\n"
    "| --- | --- |\n"
    "| alpha | 1 |\n"
    "1. First step\n"
    "2. Second step\n"
    "  1. Nested step\n"
    "- Bullet one\n"
    "- Bullet two\n"
    "<!-- mdcore-import: footer:default -->\n"
    "Footer Slot"
)

# 导入后先序块序列（blocks_json：type + 嵌套 depth 字段，0 = 顶层；
# Nested step 为 Second step 的子块 → depth 1）。P2-1：页眉/页脚四段
# （标记段 + 内容段 ×2 端）进块序。
EXPECTED_BLOCKS = [
    {"type": "paragraph", "depth": 0},  # P2-1 页眉来源标记段
    {"type": "paragraph", "depth": 0},  # P2-1 Header Slot
    {"type": "heading", "depth": 0},
    {"type": "paragraph", "depth": 0},
    {"type": "table", "depth": 0},
    {"type": "list_item", "depth": 0},
    {"type": "list_item", "depth": 0},
    {"type": "list_item", "depth": 1},
    {"type": "list_item", "depth": 0},
    {"type": "list_item", "depth": 0},
    {"type": "paragraph", "depth": 0},  # P2-1 页脚来源标记段
    {"type": "paragraph", "depth": 0},  # P2-1 Footer Slot
]

# 列表项 attrs（mdcore 解析保留 marker 文本 + markdown 深度 1 基）。
EXPECTED_LIST_ATTRS = [
    {"text": "First step", "marker": "1.", "depth": "1"},
    {"text": "Second step", "marker": "2.", "depth": "1"},
    {"text": "Nested step", "marker": "1.", "depth": "2"},
    {"text": "Bullet one", "marker": "-", "depth": "1"},
    {"text": "Bullet two", "marker": "-", "depth": "1"},
]

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True

# 页内字节导入助手（base64 → Uint8Array → window.__importDocx）
IMPORT_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__importDocx(bytes);
}"""


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    fixture_bytes = FIXTURE.read_bytes()
    fixture_b64 = base64.b64encode(fixture_bytes).decode()

    with socketserver.TCPServer(("127.0.0.1", 8933), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8933/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- UI 面在场（启动零回归：新模块不破坏 boot）----
            ui = page.evaluate("""() => ({
                file: !!document.getElementById('docx-file'),
                status: !!document.getElementById('docx-status'),
                importFn: typeof window.__importDocx,
                contract: window.__contractVersion,
            })""")
            check("import_ui_present",
                  ui["file"] and ui["status"] and ui["importFn"] == "function" and ui["contract"] == 1.4,
                  json.dumps(ui))

            # ---- 快乐路径：真实 file input（Playwright set_input_files
            #      触发原生 change → arrayBuffer → wasm import → __inject 重建）----
            page.set_input_files("#docx-file", str(FIXTURE))
            page.wait_for_function("window.__lastDocxImport && window.__lastDocxImport.ok === true",
                                   timeout=15000)
            md = page.evaluate("window.__md()")
            check("happy_import_file_input_exact_markdown", md == EXPECTED_MD, repr(md[:160]))
            blocks = page.evaluate("window.__blocks().map((b) => b.type)")
            check("import_block_types",
                  blocks == [b["type"] for b in EXPECTED_BLOCKS], json.dumps(blocks))

            # ---- 列表结构面（r5-t1：嵌套 + 先序深度——BlockTree 终端消费面）----
            blocks_full = page.evaluate(
                "window.__blocks().map((b) => ({type: b.type, depth: b.depth, text: b.text, "
                "marker: b.attrs ? (b.attrs.marker || null) : null, "
                "md_depth: b.attrs ? (b.attrs.depth || null) : null}))")
            list_blocks = [b for b in blocks_full if b["type"] == "list_item"]
            nesting_ok = (
                len(list_blocks) == 5
                and blocks_full[7]["depth"] == 1  # Nested step = Second step 子块（P2-1 前导 2 块后移）
                and blocks_full[6]["depth"] == 0
                and all(b["depth"] == 0 for i, b in enumerate(blocks_full) if b["type"] == "list_item" and i != 7)
            )
            check("import_list_nesting_depth",
                  nesting_ok,
                  json.dumps([{"type": b["type"], "depth": b["depth"]} for b in blocks_full]))
            markers_ok = all(
                lb["text"] == want["text"] and lb["marker"] == want["marker"]
                and lb["md_depth"] == want["depth"]
                for lb, want in zip(list_blocks, EXPECTED_LIST_ATTRS)
            )
            check("import_list_markers_and_md_depth",
                  markers_ok,
                  json.dumps(list_blocks))

            # ---- stats 面（typed 摘要 + 不导入面计数显式化；r5-t1 列表
            #      计数 + 零降级断言——夹具列表节全部走映射非降级路径）----
            stats = page.evaluate("window.__lastDocxImport.stats")
            status_text = page.evaluate("document.getElementById('docx-status').textContent")
            check("import_stats_and_status_line",
                  stats.get("headings") == 1 and stats.get("paragraphs") == 3
                  and stats.get("tables") == 1 and stats.get("hyperlinks") == 1
                  and stats.get("list_items") == 5
                  and stats.get("list_items_degraded") == 0
                  and stats.get("list_numbering_dropped") == 0
                  and stats.get("list_depth_clamped") == 0
                  and stats.get("list_markers_normalized") == 0
                  and stats.get("images_skipped") == 1
                  # P2-1（#22）：页眉/页脚全量接入——skipped 恒 0 +
                  # imported 计数（页眉/页脚各 1 部件）；状态行展示接入计数。
                  and stats.get("headers_skipped") == 0 and stats.get("footers_skipped") == 0
                  and stats.get("headers_imported") == 1 and stats.get("footers_imported") == 1
                  and status_text.startswith("imported:")
                  and "1 table" in status_text and "image skipped" in status_text
                  and "5 list items" in status_text
                  and "2 header/footer parts imported" in status_text,
                  json.dumps({"stats": stats, "status": status_text}))

            # ---- 导入文档是活编辑器文档（全桥：op/sync/事件管道）；
            #      列表块在编辑后存活（r5-t1）----
            page.evaluate("window.__view.dispatch({ changes: { from: window.__view.state.doc.length, insert: '\\ntyped tail' } })")
            page.wait_for_function("window.__blockCount() === 13", timeout=15000)
            md_live = page.evaluate("window.__md()")
            check("imported_doc_is_live_editor",
                  "typed tail" in md_live
                  and md_live.startswith("<!-- mdcore-import: header:default -->")
                  and "# Imported Title" in md_live
                  and "Header Slot" in md_live and "Footer Slot" in md_live
                  and "1. First step" in md_live and "  1. Nested step" in md_live
                  and "- Bullet two" in md_live,
                  repr(md_live[-80:]))

            # ---- undo 键位契约在导入文档上仍成立（回到导入态含列表节）----
            undo_ret = page.evaluate("window.__bridge.undo()")
            page.wait_for_function("!window.__md().includes('typed tail')", timeout=15000)
            md_undo = page.evaluate("window.__md()")
            check("undo_works_after_import", undo_ret is True and md_undo == EXPECTED_MD,
                  json.dumps({"ret": undo_ret, "tail": md_undo[-60:]}))

            # ---- 敌意面：垃圾字节 → typed 错误码 + 编辑器现状零触碰 ----
            md_before_hostile = page.evaluate("window.__md()")
            garbage = base64.b64encode(bytes([0x00, 0x01, 0x02, 0x63, 0xFF])).decode()
            hostile = page.evaluate(IMPORT_JS, garbage)
            hostile_status = page.evaluate("document.getElementById('docx-status').textContent")
            check("hostile_garbage_typed_error",
                  hostile.get("ok") is False and hostile.get("code") == "container_corrupt"
                  and bool(hostile.get("message"))
                  and "container_corrupt" in hostile_status
                  and page.evaluate("window.__md()") == md_before_hostile,
                  json.dumps({"r": hostile, "status": hostile_status}))

            # ---- 敌意面：截断档案（EOCD 缺失）→ 同 typed 面 ----
            truncated = base64.b64encode(fixture_bytes[: len(fixture_bytes) // 2]).decode()
            trunc = page.evaluate(IMPORT_JS, truncated)
            check("truncated_zip_typed_error",
                  trunc.get("ok") is False and trunc.get("code") == "container_corrupt"
                  and page.evaluate("window.__ready === true")
                  and page.evaluate("window.__md()") == md_before_hostile,
                  json.dumps({k: trunc.get(k) for k in ("ok", "code")}))

            # ---- 契约面 + 零页错（启动/导入全程无 JS 异常逃逸）----
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                contract: window.__contractVersion === 1.4,
                inject: typeof window.__inject === 'function',
                md: window.__md(),
            })""")
            check("contract_intact_no_pageerror",
                  final["ready"] and final["contract"] and final["inject"]
                  and final["md"] == EXPECTED_MD and len(page_errors) == 0,
                  json.dumps({"final": {k: final[k] for k in ("ready", "contract", "inject")},
                              "pageErrors": page_errors[:3]}))

            # ---- TR-13（issue #13 A1）：真 Word 夹具标题识别全链
            #      （中文版 styleId="1"-"9" → 样式名链 → headings: 0 → 4；
            #      夹具 = test-artifacts/TR2/fixtures/word-complex.docx，
            #      真 Word COM 生成——TR-2D batch2 P1-2 复现件）。放契约
            #      检之后（本节替换编辑器内容，不影响 EXPECTED_MD 末检）。----
            real_fixture = WEB.parent / "test-artifacts" / "TR2" / "fixtures" / "word-complex.docx"
            real_bytes = real_fixture.read_bytes()
            real_b64 = base64.b64encode(real_bytes).decode()
            tr13 = page.evaluate(IMPORT_JS, real_b64)
            tr13_stats = page.evaluate("window.__lastDocxImport.stats")
            tr13_md = page.evaluate("window.__md()")
            tr13_levels = [len(l) - len(l.lstrip('#'))
                           for l in tr13_md.split("\n") if l.startswith("#")]
            check("tr13_real_word_heading_chain",
                  tr13.get("ok") is True
                  and tr13_stats.get("headings") == 4
                  and "# 项目技术方案书" in tr13_md
                  and tr13_levels == [1, 2, 3, 2]
                  and len(page_errors) == 0,
                  json.dumps({"ok": tr13.get("ok"), "stats": tr13_stats,
                              "levels": tr13_levels, "bytes": len(real_bytes)}))

            # ---- TR-14（issue #14 A2）：PAGE 域缓存结果剥离（真根因：
            #      h3 段内嵌 PAGE 域缓存 "1" + 正文 "1.1 表格" → 此前
            #      "11.1 表格"）——域状态机分派（表现型剥离/内容型保留）----
            check("tr14_page_field_result_stripped",
                  "### 1.1 表格" in tr13_md
                  and "11.1" not in tr13_md
                  and tr13_stats.get("fields_dropped") == 1
                  and "## REV-INSERT-第一章 概述" in tr13_md,  # #17: ins=当前文本（前缀在场）
                  json.dumps({"fields_dropped": tr13_stats.get("fields_dropped"),
                              "h3_ok": "### 1.1 表格" in tr13_md}))

            # ---- #22 P2-4：可见警告面（真 Word 夹具验收）——
            #      word-complex fields_dropped=1 → warnings 数组 warn 条目
            #      + 状态行警告数 + 面板默认收起/点击展开/再点收起。----
            p24_status = page.evaluate("document.getElementById('docx-status').textContent")
            p24_warns = tr13_stats.get("warnings") or []
            p24_expected = f"{len(p24_warns)} warning" + ("s" if len(p24_warns) != 1 else "")
            p24_hidden0 = page.evaluate("document.getElementById('docx-warning-panel')?.hidden")
            page.locator("#docx-status").click()
            p24_open = page.evaluate("!document.getElementById('docx-warning-panel')?.hidden")
            p24_panel = page.evaluate("document.getElementById('docx-warning-panel')?.textContent ?? ''")
            page.locator("#docx-status").click()
            p24_hidden1 = page.evaluate("document.getElementById('docx-warning-panel')?.hidden")
            check("p2_4_visible_warnings_panel",
                  any(w.get("kind") == "fields_dropped" and w.get("severity") == "warn"
                      and w.get("count") == 1 and w.get("part") == "docx"
                      for w in p24_warns)
                  and p24_expected in p24_status
                  and p24_hidden0 is True and p24_open is True and p24_hidden1 is True
                  and "fields_dropped" in p24_panel and "warn" in p24_panel,
                  json.dumps({"status": p24_status, "panel": p24_panel[:160],
                              "warns": p24_warns}))

            # ---- TR-17（issue #17 P0）：修订插入当前文本提取——真
            #      澄清样本（ai-bid-research corpus-R D2a）：w:ins 文本
            #      进 md 流 + delText 零提取 + revision_delete 登记。----
            d2a = WEB.parent / "test-artifacts" / "corpusR" / "D2a-nonbody.docx"
            d2a_bytes = d2a.read_bytes()
            d2a_b64 = base64.b64encode(d2a_bytes).decode()
            tr17 = page.evaluate(IMPORT_JS, d2a_b64)
            tr17_stats = page.evaluate("window.__lastDocxImport.stats")
            tr17_md = page.evaluate("window.__md()")
            tr17_warns = tr17_stats.get("warnings") or []
            check("tr17_revision_insert_current_text",
                  tr17.get("ok") is True
                  and "（修订插入：响应截止时间顺延至 2026-12-01 09:30）" in tr17_md
                  and "原表述：响应截止时间 2026-11-20 09:30" not in tr17_md
                  and any(w.get("kind") == "revision_delete" for w in tr17_warns)
                  and len(page_errors) == 0,
                  json.dumps({"ok": tr17.get("ok"),
                              "ins_ok": "修订插入" in tr17_md,
                              "del_leak": "原表述" in tr17_md,
                              "warns": tr17_warns[:2]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "t16 docx import e2e", "dist": str(WEB / "dist"), "fixture": str(FIXTURE),
                 "fixture_bytes": len(fixture_bytes)},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
