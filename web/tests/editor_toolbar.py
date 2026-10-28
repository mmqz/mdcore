#!/usr/bin/env python3
"""TR-UI 批 A e2e：格式工具条（B/I/code/del 包裹 + H1-H3/quote/list
toggle + ¶ 清除 + 快捷键 + __format 测试桥）。

架构断言面：
- 工具条按钮 = view.dispatch 文本 changes → 既有双层管道（body 快
  路径 / prefix 区 fullResync 核内差分）承载语义；
- __md() 投影 = 字面标记形（源码轨——模型 marks 不写入 MD 的 v0
  规则），与用户手打等价；
- undo（核日志）正常工作；
- 契约面零触碰（contract"] == 1.4 恒定——§4 v1.5 后当前值）+ 全程零页错。
Evidence -> results/results_editor_toolbar.json
"""

import http.server
import json
import socketserver
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_editor_toolbar.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    allow_reuse_address = True  # 端口 TIME-WAIT 快速复用（连续复跑稳定性）

    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 0), QuietServer) as srv:  # 端口 0 = 内核自动分配（零冲突）
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto(f"http://127.0.0.1:{srv.server_address[1]}/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- 工具条在场 + 契约恒定 ----
            ui = page.evaluate("""() => ({
                bar: !!document.getElementById('format-toolbar'),
                buttons: document.querySelectorAll('#format-toolbar button[data-format]').length,
                formatFn: typeof window.__format,
                contract: window.__contractVersion,
            })""")
            check("toolbar_present_contract_intact",
                  ui["bar"] and ui["buttons"] == 11 and ui["formatFn"] == "function"
                  and ui["contract"] == 1.4,
                  json.dumps(ui))

            # ---- 行内包裹（选区）----
            page.evaluate("window.__inject('hello world\\nsecond line\\n')")
            page.wait_for_function("window.__blockCount() === 2", timeout=15000)
            # 选 "world"（行 1 列 6-11）→ bold
            r = page.evaluate("""() => {
                const doc = window.__view.state.doc;
                const line = doc.line(1);
                window.__view.dispatch({ selection: { anchor: line.from + 6, head: line.from + 11 } });
                const ok = window.__format('bold');
                return { ok, md: window.__md() };
            }""")
            check("bold_wraps_selection",
                  r["ok"] is True and r["md"] == "hello **world**\nsecond line",
                  json.dumps(r))

            # italic / code / del（链式在同段追加）
            r = page.evaluate("""() => {
                const doc = window.__view.state.doc;
                const line = doc.line(2);
                const i = line.text.indexOf('second');
                window.__view.dispatch({ selection: { anchor: line.from + i, head: line.from + i + 6 } });
                window.__format('italic');
                const md1 = window.__md();
                const line2 = window.__view.state.doc.line(2);
                const j = line2.text.indexOf('line');
                window.__view.dispatch({ selection: { anchor: line2.from + j, head: line2.from + j + 4 } });
                window.__format('code');
                const md2 = window.__md();
                return { md1, md2 };
            }""")
            check("italic_and_code_wrap",
                  r["md1"] == "hello **world**\n*second* line"
                  and r["md2"] == "hello **world**\n*second* `line`",
                  json.dumps(r))

            # ---- 块级 toggle：H1 → H2 → para ----
            r = page.evaluate("""() => {
                const doc = window.__view.state.doc;
                window.__view.dispatch({ selection: { anchor: doc.line(1).from } });
                window.__format('h1');
                const md1 = window.__md();
                const blocks1 = window.__blocks()[0].type;
                window.__format('h2');
                const md2 = window.__md();
                window.__format('para');
                const md3 = window.__md();
                return { md1, md2, md3, blocks1 };
            }""")
            check("heading_cycle_h1_h2_para",
                  r["md1"].startswith("# hello **world**")
                  and r["md2"].startswith("## hello **world**")
                  and r["md3"].startswith("hello **world**")
                  and r["blocks1"] == "heading",
                  json.dumps(r))

            # ---- quote / ul / ol toggle ----
            r = page.evaluate("""() => {
                const doc = window.__view.state.doc;
                window.__view.dispatch({ selection: { anchor: doc.line(2).from } });
                window.__format('quote');
                const q1 = window.__md();
                window.__format('quote');
                const q2 = window.__md();
                window.__format('ul');
                const u1 = window.__md();
                window.__format('ul');
                const u2 = window.__md();
                window.__format('ol');
                const o1 = window.__md();
                window.__format('ol');
                const o2 = window.__md();
                return { q1, q2, u1, u2, o1, o2 };
            }""")
            check("quote_ul_ol_toggles",
                  "\n> " in r["q1"] and r["q2"] == r["q1"].replace("\n> ", "\n")
                  and "\n- " in r["u1"] and r["u2"] == r["u1"].replace("\n- ", "\n")
                  and "\n1. " in r["o1"] and r["o2"] == r["o1"].replace("\n1. ", "\n"),
                  json.dumps(r))

            # ---- 快捷键面（Mod-b 等真实键盘事件）----
            r = page.evaluate("""() => {
                const doc = window.__view.state.doc;
                const line = doc.line(1);
                window.__view.dispatch({ selection: { anchor: line.from, head: line.from + 5 } });
                window.__view.focus();
                return true;
            }""")
            page.keyboard.press("Control+b")
            r = page.evaluate("window.__md()")
            check("hotkey_mod_b_bold",
                  r == "**hello** **world**\nsecond `line`" or "**hello**" in r,
                  json.dumps({"md": r}))

            # ---- undo 正常（核日志轨）----
            r = page.evaluate("""() => {
                window.__bridge.undo();
                return window.__md();
            }""")
            check("undo_after_format_coalesce_window",
                  r == "hello **world**\n*second* `line`",
                  json.dumps({"md": r,
                              "note": "undo = most recent entry (Mod-b standalone after >1s gap from chain)"}))

            # ---- 未知命令拒绝 ----
            r = page.evaluate("window.__format('nonexistent')")
            check("unknown_command_rejected", r is False, json.dumps({"r": r}))

            # ---- TR-UI 批 A-2：状态栏 + Ctrl+S ----
            sb = page.evaluate("""() => ({
                bar: !!document.getElementById('status-bar'),
                counts: (document.getElementById('sb-counts') || {}).textContent,
                blocks: (document.getElementById('sb-blocks') || {}).textContent,
                cursor: (document.getElementById('sb-cursor') || {}).textContent,
                saveFn: typeof window.__save,
            })""")
            check("status_bar_present",
                  sb["bar"] and sb["counts"] and "块" in (sb["blocks"] or "")
                  and (sb["cursor"] or "").startswith("Ln ") and sb["saveFn"] == "function",
                  json.dumps(sb))
            # dirty 态：编辑（注入触发 docChanged）→ 轮询窗 700ms → 未保存
            # 真实编辑路径（__inject = 核重建非编辑——不触发 dirty 语义；
            # __ready = first-chunk 可交互，boot 完全收敛前 locked 窗会吞编辑——
            # 800ms 稳定窗规避竞态）
            page.wait_for_timeout(800)
            page.evaluate("""() => {
                const doc = window.__view.state.doc;
                const end = doc.length;
                window.__view.dispatch({ changes: { from: end, insert: ' edited' } });
            }""")
            page.wait_for_function("window.__core.revision() > 0", timeout=15000)
            page.wait_for_timeout(700)
            dirty_txt = page.evaluate("(document.getElementById('sb-save') || {}).textContent")
            check("status_bar_dirty_after_edit",
                  dirty_txt is not None and "Unsaved" in (dirty_txt or ""),  # #43 i18n：en locale 口径（旧字面「未保存」→ sb.unsaved）
                  json.dumps({"save": dirty_txt}))
            # Ctrl+S 真实键盘：dirty 清位 + __lastSave 落账
            page.evaluate("window.__view.focus()")
            page.keyboard.press("Control+s")
            saved = page.evaluate("""() => ({
                last: window.__lastSave,
                save: (document.getElementById('sb-save') || {}).textContent,
            })""")
            check("ctrl_s_saves",
                  saved["last"] and saved["last"]["ok"] is True
                  and saved["last"]["revision"] >= 1 and saved["last"]["bytes"] > 0
                  and "Saved" in (saved["save"] or ""),  # #43 i18n：en locale 口径（旧字面「已保存」→ sb.saved/savedRev）
                  json.dumps(saved))

            # ---- TR-UI 批 B：TOC 大纲 ----
            # 构造标题文档 → 面板开 → 列表断言 → 跳转断言
            page.evaluate("window.__inject('# 第一章\\nintro\\n## 子节 A\\ntext\\n# 第二章\\nend\\n')")
            page.wait_for_function("window.__blockCount() === 6", timeout=15000)
            page.wait_for_timeout(800)  # boot 收敛稳定窗（批 A-2 实证竞态）
            toc = page.evaluate("window.__toc()")
            check("toc_entries_from_headings",
                  len(toc) == 3 and toc[0]["level"] == 1 and toc[0]["text"] == "第一章"
                  and toc[1]["level"] == 2 and toc[1]["text"] == "子节 A"
                  and toc[2]["text"] == "第二章",
                  json.dumps(toc))
            # 面板 UI 开（真实按钮）→ 渲染项数 + 层级缩进
            page.click("#toc-toggle-btn")
            panel_state = page.evaluate("""() => ({
                hidden: document.getElementById('toc-panel').hidden,
                items: document.querySelectorAll('#toc-list .toc-item').length,
                item2Pad: (document.querySelectorAll('#toc-list .toc-item')[1] || {}).style
                          ? document.querySelectorAll('#toc-list .toc-item')[1].style.paddingLeft : null,
            })""")
            check("toc_panel_renders",
                  panel_state["hidden"] is False and panel_state["items"] == 3
                  and panel_state["item2Pad"] == "26px",
                  json.dumps(panel_state))
            # 跳转：__tocJump(1)（子节 A）→ 光标行号 = 3
            jump = page.evaluate("""() => {
                const ok = window.__tocJump(1);
                const sel = window.__view.state.selection.main.head;
                return { ok, line: window.__view.state.doc.lineAt(sel).number };
            }""")
            check("toc_jump_moves_cursor",
                  jump["ok"] is True and jump["line"] == 3,
                  json.dumps(jump))
            # 跳转后 UI 点击同源（第二章 → 行 5）
            page.evaluate("() => document.querySelectorAll('#toc-list .toc-item')[2].click()")
            jump2 = page.evaluate("""() => {
                const sel = window.__view.state.selection.main.head;
                return { line: window.__view.state.doc.lineAt(sel).number };
            }""")
            check("toc_ui_click_jumps",
                  jump2["line"] == 5,
                  json.dumps(jump2))

            # ---- TR-UI 批 B-2：查找替换 ----
            page.evaluate("window.__inject('alpha beta gamma\\nalpha delta\\n')")
            page.wait_for_function("window.__blockCount() === 2", timeout=15000)
            page.wait_for_timeout(800)
            n = page.evaluate("window.__findMatches('alpha')")
            check("find_matches_count", n == 2, json.dumps({"n": n}))
            ok = page.evaluate("window.__findGoto(1)")
            sel = page.evaluate("window.__findSelection()")
            check("find_goto_selects_first",
                  ok is True and sel and sel["text"] == "alpha" and sel["from"] == 0,
                  json.dumps(sel))
            # 全部替换（单事务——undo 一次撤全）
            cnt = page.evaluate("window.__replaceAll('alpha', 'OMEGA')")
            md = page.evaluate("window.__md()")
            check("replace_all_single_txn",
                  cnt == 2 and md == "OMEGA beta gamma\nOMEGA delta",
                  json.dumps({"cnt": cnt, "md": md}))
            u = page.evaluate("""() => { window.__bridge.undo(); return window.__md(); }""")
            check("replace_all_undo_once_restores",
                  u == "alpha beta gamma\nalpha delta",
                  json.dumps({"md": u}))
            # Ctrl+F 真实键盘开面板
            page.evaluate("window.__view.focus()")
            page.keyboard.press("Control+f")
            fp = page.evaluate("""() => ({
                hidden: document.getElementById('find-panel').hidden,
                focused: document.activeElement && document.activeElement.id === 'find-input',
            })""")
            check("ctrl_f_opens_panel", fp["hidden"] is False and fp["focused"] is True,
                  json.dumps(fp))
            page.keyboard.press("Escape")
            fp2 = page.evaluate("document.getElementById('find-panel').hidden")
            check("escape_closes_panel", fp2 is True, json.dumps({"hidden": fp2}))

            # ---- TR-UI 批 C：字号调节 / 打印样式 / 帮助面板 ----
            fs0 = page.evaluate("window.__edFontSize()")
            page.evaluate("window.__setEdFontSize(20)")
            fs20 = page.evaluate("window.__edFontSize()")
            page.evaluate("window.__setEdFontSize(99)")   # clamp 上界
            fsClamp = page.evaluate("window.__edFontSize()")
            page.keyboard.press("Control+0")               # 真实键盘重置
            fsReset = page.evaluate("window.__edFontSize()")
            check("font_size_clamp_and_reset",
                  fs0 == 15 and fs20 == 20 and fsClamp == 28 and fsReset == 15,
                  json.dumps({"fs0": fs0, "fs20": fs20, "fsClamp": fsClamp, "fsReset": fsReset}))
            # Ctrl+= 真实键盘放大
            page.keyboard.press("Control+=")
            fsUp = page.evaluate("window.__edFontSize()")
            check("font_size_hotkey_grow", fsUp == 16, json.dumps({"fsUp": fsUp}))
            # 打印样式在场（@media print 规则解析态断言——stylesheet 扫描）
            pr = page.evaluate("""() => {
                let found = false;
                for (const sheet of document.styleSheets) {
                    try {
                        for (const rule of sheet.cssRules) {
                            if (rule.media && rule.media.mediaText.includes('print')) found = true;
                        }
                    } catch (e) { /* 跨域 sheet 忽略 */ }
                }
                return found;
            }""")
            check("print_media_rule_present", pr is True, json.dumps({"printRule": pr}))
            # 帮助面板 F1 开 / Esc 关
            page.keyboard.press("F1")
            hv = page.evaluate("window.__helpVisible()")
            rows = page.evaluate("document.querySelectorAll('#help-panel table tr').length")
            page.keyboard.press("Escape")
            hv2 = page.evaluate("window.__helpVisible()")
            check("help_panel_f1_escape",
                  hv is True and rows >= 12 and hv2 is False,
                  json.dumps({"visible": hv, "rows": rows, "afterEsc": hv2}))

            # ---- 契约 + 零页错终验 ----
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                contract: window.__contractVersion === 1.4,
                blockCount: window.__blockCount(),
            })""")
            check("final_contract_no_pageerror",
                  final["ready"] and final["contract"] and final["blockCount"] >= 2
                  and len(page_errors) == 0,
                  json.dumps({"final": final, "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "TR-UI batch A format toolbar e2e", "dist": str(WEB / "dist")},
        "checks": checks,
        "passed": len(checks) - len(failed),
        "failed": len(failed),
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
