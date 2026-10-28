#!/usr/bin/env python3
"""R23-d contract gate (v1) + R24-a settings face (v1.1): serve web/dist, assert
the frozen host<->bridge<->core contract (CONTRACT.md v1.1) on a live page —
boot surface, host inject path (__inject / __inject_chunk / mdc-inject* event
family), __bridge inventory, undo key-contract, unit-constitution roundtrip,
settings read/write face (__settings / __setSetting / mdc-settings-changed).
Evidence -> results_contract.json
"""

import http.server
import json
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_contract.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（web_smoke 同款口径）
socketserver.TCPServer.allow_reuse_address = True


def main() -> int:
    checks = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8931), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: check("pageerror", False, str(e)))
            page.goto("http://127.0.0.1:8931/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- §3.1 启动面 ----
            check("ready", page.evaluate("window.__ready === true"))
            check("contract_version", page.evaluate("window.__contractVersion === 1.4"))
            surf = page.evaluate("""() => ({
                inject: typeof window.__inject,
                injectChunk: typeof window.__inject_chunk,
                md: typeof window.__md,
                blocks: typeof window.__blocks,
                blockCount: typeof window.__blockCount,
                memStats: typeof window.__memStats,
                events: Array.isArray(window.__mdcoreEvents),
                perf: typeof window.__perf,
                core: typeof window.__core,
                view: typeof window.__view,
            })""")
            check("boot_surface_types",
                  all(v == "function" for k, v in surf.items() if k not in ("events", "core", "perf", "view"))
                  and surf["events"] and surf["core"] == "object" and surf["perf"] == "object"
                  and surf["view"] == "object", json.dumps(surf))
            # T3 复用地基轮 F-1 现状锁：__wasmMemory/__heapTrace 仅渐进路径
            # 安装——默认一次性启动后不在场（此前契约误记"恒在"，已勘误）。
            check("boot_progressive_only_members_absent_by_default",
                  page.evaluate("window.__wasmMemory === undefined && window.__heapTrace === undefined"),
                  "默认启动后 __wasmMemory/__heapTrace 应不在场（渐进路径专属）")

            # ---- §3.3 __bridge 清单 ----
            br = page.evaluate("""() => ({
                pushRender: typeof window.__bridge.pushRender,
                locked: typeof window.__bridge.locked,
                undo: typeof window.__bridge.undo,
                redo: typeof window.__bridge.redo,
                computeDecorations: typeof window.__bridge.computeDecorations,
                dumpDecorations: typeof window.__bridge.dumpDecorations,
                setSelections: typeof window.__bridge.setSelections,
                debugMapVsFullRebuild: typeof window.__bridge.debugMapVsFullRebuild,
                utf16ToCp: typeof window.__bridge.unit.utf16ToCp,
                cpToUtf16: typeof window.__bridge.unit.cpToUtf16,
            })""")
            check("bridge_inventory", all(v == "function" for k, v in br.items() if k != "locked")
                  and br["locked"] == "boolean", json.dumps(br))

            # ---- §3.2 注入面：事件族 + 全量重建 ----
            page.evaluate("""() => {
                window.__spy = { inject: 0, chunk: 0 };
                window.addEventListener('mdc-inject', () => window.__spy.inject++);
                window.addEventListener('mdc-inject-chunk', () => window.__spy.chunk++);
            }""")
            page.evaluate("window.__inject('# Host Doc\\n\\nHost body paragraph.\\n\\n')")
            page.wait_for_function("window.__blockCount() === 2", timeout=15000)
            md1 = page.evaluate("window.__md()")
            check("inject_rebuild", "Host body paragraph." in md1 and "# Host Doc" in md1, md1[:80])
            check("inject_event_fired", page.evaluate("window.__spy.inject === 1"))
            # 排空时机契约：事件在 pushRender 同步时 drain（R11 桥语义），
            # 重建后先验起点清零，再验渲染后 ingest 事件可见
            check("inject_event_stream_fresh", page.evaluate("window.__mdcoreEvents.length === 0"))
            # 重建核的初始 ingest 事件由构造器首渲消费（与 boot 同语义）；
            # 编辑产生的 op 事件在渲染同步时到达宿主数组（R11 排空契约正测面）
            page.evaluate("window.__view.dispatch({ changes: { from: 0, insert: 'Z' } })")
            page.evaluate("window.__bridge.pushRender(undefined, { force: true })")
            check("inject_events_drain_on_render", page.evaluate("window.__mdcoreEvents.length > 0"))
            check("inject_keeps_surface",
                  page.evaluate("typeof window.__memStats === 'function' && window.__contractVersion === 1.4"))

            # ---- §3.2 渐进通道 ----
            page.evaluate("window.__inject_chunk('first chunk\\n\\n', true)")
            page.wait_for_function("window.__blockCount() === 1", timeout=15000)
            page.evaluate("window.__inject_chunk('second chunk tail\\n\\n', false)")
            page.wait_for_function("window.__blockCount() === 2", timeout=15000)
            md2 = page.evaluate("window.__md()")
            check("chunk_progressive", "first chunk" in md2 and "second chunk tail" in md2, md2[:120])
            check("chunk_event_fired", page.evaluate("window.__spy.chunk === 2"))

            # ---- §3.3 undo 键位契约（R21 勘误口径）----
            undo_ret = page.evaluate("window.__bridge.undo()")
            check("undo_key_contract", undo_ret is True, repr(undo_ret))

            # ---- §3.3 单位宪法换算出口 ----
            unit = page.evaluate("""() => {
                const s = 'a\\u{1F389}b';
                return { u2c: window.__bridge.unit.utf16ToCp(s, 3), c2u: window.__bridge.unit.cpToUtf16(s, 2) };
            }""")
            check("unit_constitution", unit["u2c"] == 2 and unit["c2u"] == 3, json.dumps(unit))

            # ---- §4 设置面（v1.1，R24-a；#16 增补 docx_export 层 v1.2）----
            st = page.evaluate("window.__settings()")
            check("settings_read_surface",
                  st.get("schema_version") == 1
                  and all(k in st for k in ("editor", "ai", "storage", "appearance", "docx_export"))
                  and st["editor"]["undo_coalesce_ms"] == 1000
                  and st["appearance"]["theme"] == "system"
                  # #43（CONTRACT v1.6）：语言键缺省 auto（跟随 navigator）。
                  and st["appearance"]["language"] == "auto"
                  and st["docx_export"]["preset"] == "default"
                  and st["docx_export"]["size_half_points"] == 28, json.dumps(st))

            page.evaluate("""() => {
                window.__settingsSpy = [];
                window.addEventListener('mdc-settings-changed',
                    (ev) => window.__settingsSpy.push(ev.detail));
            }""")
            ok_write = page.evaluate("window.__setSetting('appearance.theme', 'dark')")
            st2 = page.evaluate("window.__settings()")
            spy = page.evaluate("window.__settingsSpy")
            check("settings_write_and_event",
                  ok_write is True and st2["appearance"]["theme"] == "dark"
                  and len(spy) == 1 and spy[0]["path"] == "appearance.theme"
                  and spy[0]["value"] == "dark", json.dumps({"ok": ok_write, "st": st2, "spy": spy}))

            rejected = page.evaluate("""() => ({
                unknownPath: window.__setSetting('editor.nonexistent', 1),
                reservedKey: window.__setSetting('schema_version', 2),
                wrongType: window.__setSetting('appearance.theme', 123),
                outOfRange: window.__setSetting('appearance.font_size_px', 99),
                floatNotInt: window.__setSetting('editor.undo_coalesce_ms', 1.5),
                docxPresetEnum: window.__setSetting('docx_export.preset', 'a4'),
                docxPresetType: window.__setSetting('docx_export.preset', 1),
                docxSizeDomain: window.__setSetting('docx_export.size_half_points', 22),
                docxSpacingDomain: window.__setSetting('docx_export.line_spacing', 300),
                docxMarginDomain: window.__setSetting('docx_export.margin_twips', 2000),
                docxTocType: window.__setSetting('docx_export.toc', 'yes'),
                docxTocNum: window.__setSetting('docx_export.toc', 1),
                docxAnonymizeType: window.__setSetting('docx_export.anonymize', 'true'),
                langType: window.__setSetting('appearance.language', 1),
                langDomain: window.__setSetting('appearance.language', 'fr'),
                langLocaleVariant: window.__setSetting('appearance.language', 'zh-TW'),
            })""")
            spy2 = page.evaluate("window.__settingsSpy")
            st3 = page.evaluate("window.__settings()")
            check("settings_write_rejects",
                  all(v is False for v in rejected.values())
                  and len(spy2) == 1  # 拒绝写入不派事件
                  and st3["schema_version"] == 1 and st3["appearance"]["theme"] == "dark"
                  and st3["appearance"]["font_size_px"] == 15
                  and st3["docx_export"]["preset"] == "default", json.dumps(rejected))

            # #16：docx_export 层写入 + 事件 + 面板联动（mdc-settings-changed
            # → docxExportOptions 同步——设置面三源归一）。
            ok_docx_write = page.evaluate(
                "window.__setSetting('docx_export.preset', 'official-a4')")
            st4 = page.evaluate("window.__settings()")
            docx_opts = page.evaluate("window.__getDocxExportOptions()")
            check("tr16_settings_write_syncs_panel",
                  ok_docx_write is True
                  and st4["docx_export"]["preset"] == "official-a4"
                  and docx_opts["preset"] == "official-a4"
                  and docx_opts["json"].find('宋体') != -1, json.dumps(docx_opts))

            # W-1/W-2（issue #29，§4 v1.5）：toc/anonymize 两布尔键写入
            # → 快照 + 面板状态 + options JSON 组装（导出链消费面）。
            ok_w2a_toc = page.evaluate("window.__setSetting('docx_export.toc', true)")
            ok_w2a_anon = page.evaluate("window.__setSetting('docx_export.anonymize', true)")
            w2a = page.evaluate("""() => ({
                snap: window.__settings().docx_export,
                panel: window.__getDocxExportOptions(),
                tocChk: (document.getElementById('deo-toc') || {}).checked,
                anonChk: (document.getElementById('deo-anonymize') || {}).checked,
            })""")
            check("w2a_toc_anonymize_settings_sync_panel",
                  ok_w2a_toc is True and ok_w2a_anon is True
                  and w2a["snap"]["toc"] is True and w2a["snap"]["anonymize"] is True
                  and w2a["panel"]["json"].find('"toc":{"enabled":true,"level_from":1,"level_to":3}') != -1
                  and w2a["panel"]["json"].find('"anonymize":true') != -1
                  and w2a["tocChk"] is True and w2a["anonChk"] is True,
                  json.dumps(w2a))
            # 复位（零污染后续检查/下次运行）。
            page.evaluate("window.__setSetting('docx_export.toc', false)")
            page.evaluate("window.__setSetting('docx_export.anonymize', false)")

            # ---- §3.1 F-1 对侧现状锁：渐进路径专属成员在场 ----
            # 默认启动缺席已由 boot_progressive_only_members_absent_by_default
            # 锁定；此处开 ?chunks=2 渐进页验在场（CONTRACT §3 勘误后双侧
            # 与实现一致——T3 复用地基轮）。
            ppage = browser.new_page()
            ppage.goto("http://127.0.0.1:8931/index.html?chunks=2")
            ppage.wait_for_function("window.__ready === true", timeout=30000)
            check("boot_progressive_only_members_present_on_progressive_boot",
                  ppage.evaluate("typeof window.__wasmMemory === 'object' && window.__wasmMemory !== null"
                                 " && Array.isArray(window.__heapTrace)"),
                  "渐进启动应安装 __wasmMemory(Memory)/__heapTrace(数组)")
            ppage.close()

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "R23-d contract v1 + R24-a settings v1.1", "dist": str(WEB / "dist")},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed, "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
