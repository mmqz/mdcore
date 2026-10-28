#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""M-2（#29 任务 1）崩溃恢复与持久化 e2e：beforeunload 拦截 + localStorage
草稿 + 保存/恢复语义闭环。

Lead 验收四条（15:03 指令）逐条对齐 + 实现语义面（去抖落稿/pagehide 终刷/
注入直通/超限清稿/协同门控/恢复不脏）：

  A. beforeunload 四验收（1-4）：
     没编辑过刷新不弹 / 编辑后刷新弹（dismiss 留页实证）/ 编辑+Ctrl+S
     后刷新不弹 / 编辑后接受弹窗刷新 → 草稿恢复内容不丢。
  B. 草稿机制（5-8）：
     真实键入 1s 去抖落稿（localStorage 键 + 信封）/ 保存即刷（mdc-save
     权威锚点）/ __inject 直通置换 + 整核重建 rev 归零重基线（「导入即
     未保存」脏位复活缺陷回归锁）/ 超 1MiB 上限清稿不落。
  C. 门控与终刷（9-11）：
     协同槽位（?cc=1）三面全关（不恢复/不落稿/不拦 beforeunload）/
     小文档回归后草稿复活 / pagehide 终刷（去抖窗内立即刷新——pending
     开窗实证 + 恢复含刚键入内容）。
  D. 全程零 pageerror（12）。

键入全部走真实键盘（CM6 事务 → 核 op → rev——__inject 是整核重建导入
语义，不能充当编辑证据）。结果 -> tests/results/results_crash_recovery.json
（端口 8957）。
"""

import http.server
import json
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
DIST = WEB / "dist"
PORT = 8957
OUT = WEB / "tests" / "results" / "results_crash_recovery.json"
URL = f"http://127.0.0.1:{PORT}/index.html"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({
    ".html": "text/html", ".js": "text/javascript",
    ".wasm": "application/wasm", ".mjs": "text/javascript",
})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(DIST), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True


def main() -> int:
    if not (DIST / "main.js").exists():
        print("FAIL dist/main.js absent — run `npm run build` first")
        return 1

    checks: list[dict] = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name
              + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return bool(ok)

    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            context = browser.new_context()
            page = context.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))

            # 弹窗路由：单 handler 可切换 accept/dismiss（beforeunload 只有
            # 注册了 handler Playwright 才会上报——这正是验收机制）。
            dialog_state = {"action": "accept", "log": []}

            def on_dialog(d):
                dialog_state["log"].append(d.type)
                if dialog_state["action"] == "accept":
                    d.accept()
                else:
                    d.dismiss()

            page.on("dialog", on_dialog)

            # 恢复事件捕捉：init script 每次导航前注册（跨 reload 存活）。
            page.add_init_script(
                "window.__evDraft = null;"
                "window.addEventListener('mdc-draft-restored',"
                " (e) => { window.__evDraft = e.detail.ts; });")

            def ready(p=None, timeout=60_000):
                # 导航感知就绪：旧文档 __pre=1（reload 前设置）恒不满足，
                # 新文档 __pre 未定义且 __ready——消除「在旧页上即真」的
                # 导航竞态（typing 打在将死旧页上会丢）。
                (p or page).wait_for_function(
                    "window.__pre === undefined && window.__ready === true",
                    timeout=timeout)

            def type_at_end(text, p=None):
                """真实键盘路径：聚焦 + 光标移文档尾 + 逐键输入。"""
                (p or page).evaluate(
                    "window.__view.focus();"
                    "window.__view.dispatch({selection: "
                    "{anchor: window.__view.state.doc.length}})")
                (p or page).keyboard.type(text)

            def reload_js(p=None):
                # 导航标记（新文档无此键）+ 延迟触发（evaluate 返回后再导航，
                # 避免内联 location.reload 与 evaluate 的竞态）。
                (p or page).evaluate(
                    "window.__pre = 1;"
                    "() => setTimeout(() => location.reload(), 0)")

            def draft_raw(p=None):
                return (p or page).evaluate("localStorage.getItem('mdc-draft')")

            # ---- 1. 没编辑过刷新 -> 不弹 ----
            page.goto(URL)
            ready()
            check("boot_fresh_no_draft_restored",
                  page.evaluate("window.__draftInfo().restoredTs === null"
                                " && window.__draftInfo().active === true"),
                  json.dumps(page.evaluate("window.__draftInfo()")))
            dialog_state.update(action="accept", log=[])
            reload_js()
            ready()
            check("no_edit_reload_no_dialog",
                  dialog_state["log"] == [],
                  f"dialogs={dialog_state['log']}")

            # ---- 2. 编辑后刷新 -> 弹 + dismiss 留页 ----
            type_at_end(" MARKER_A")
            page.wait_for_function("window.__unsaved() === true", timeout=10_000)
            check("real_typing_marks_unsaved",
                  page.evaluate("window.__unsaved() === true"))
            page.evaluate("window.__stayed = 'yes'")
            dialog_state.update(action="dismiss", log=[])
            reload_js()
            page.wait_for_timeout(800)  # dismiss = 导航取消，页面留存
            check("edit_reload_beforeunload_dialog_blocks",
                  dialog_state["log"] == ["beforeunload"]
                  and page.evaluate("window.__stayed") == "yes",
                  f"dialogs={dialog_state['log']} "
                  f"stayed={page.evaluate('window.__stayed')}")

            # ---- 3. 去抖落稿（真实键入 1s 静默）----
            type_at_end(" MARKER_B")
            page.wait_for_function(
                "() => { const r = localStorage.getItem('mdc-draft');"
                " return !!r && r.includes('MARKER_A') && r.includes('MARKER_B'); }",
                timeout=15_000)
            raw = draft_raw()
            check("debounce_draft_written_with_markers",
                  raw is not None and "MARKER_A" in raw and "MARKER_B" in raw,
                  f"bytes={len(raw or '')}")

            # ---- 4. Ctrl+S 后刷新 -> 不弹 + 草稿恢复不丢 ----
            page.keyboard.press("Control+s")
            page.wait_for_function("window.__unsaved() === false", timeout=5_000)
            dialog_state.update(action="accept", log=[])
            reload_js()
            ready()
            md_after = page.evaluate("window.__md()")
            check("save_then_reload_no_dialog_and_draft_restored",
                  dialog_state["log"] == []
                  and "MARKER_A" in md_after and "MARKER_B" in md_after
                  and page.evaluate("window.__draftInfo().restoredTs !== null"),
                  f"dialogs={dialog_state['log']} restored="
                  f"{page.evaluate('window.__draftInfo().restoredTs')}")

            # ---- 5. 恢复后不脏（脏位复活缺陷回归锁）----
            check("restored_boot_is_not_dirty",
                  page.evaluate("window.__unsaved() === false"),
                  "restore path must re-baseline dirty (rev starts fresh)")

            # ---- 6. __inject 直通 + 重基线（导入语义）----
            type_at_end(" MARKER_C")
            page.wait_for_function("window.__unsaved() === true", timeout=10_000)
            page.evaluate("window.__inject('INJECT-X')")
            page.wait_for_timeout(600)  # 事件派发 + 状态栏重基线轮询
            raw = draft_raw()
            check("inject_replaces_draft_and_rebaselines_dirty",
                  page.evaluate("window.__unsaved() === false"
                                " && window.__md() === 'INJECT-X'")
                  and raw is not None and "INJECT-X" in raw,
                  f"unsaved={page.evaluate('window.__unsaved()')} raw_has_x="
                  f"{'INJECT-X' in (raw or '')}")

            # ---- 7. 超限清稿（>1MiB 不落；多行真实形态——单行超长
            #     是 CM6 行装饰的病态输入，非本套件度量对象）----
            page.evaluate(
                "window.__inject(('X'.repeat(99) + '\\n').repeat(12100))")
            page.wait_for_timeout(2500)  # 1.2MB 整核重建 + 渲染 + 直通落稿
            check("oversize_inject_clears_draft",
                  draft_raw() is None
                  and page.evaluate("window.__draftInfo().bytes === 0"),
                  f"raw={draft_raw() is not None}")

            # ---- 8. 小文档回归 → 草稿复活 + 再键入去抖 ----
            page.evaluate("window.__inject('small base')")
            page.wait_for_timeout(600)
            type_at_end(" MARKER_D")
            # 内容驱动等待（字节数断言会被 inject 写穿的旧稿瞬时满足）。
            page.wait_for_function(
                "() => { const r = localStorage.getItem('mdc-draft');"
                " return !!r && r.includes('small base') && r.includes('MARKER_D'); }",
                timeout=15_000)
            raw = draft_raw()
            check("draft_returns_after_small_doc",
                  raw is not None and "small base" in raw and "MARKER_D" in raw,
                  f"bytes={len(raw or '')}")

            # ---- 9. 协同槽位门控（?cc=1：不恢复/不落稿/不拦截）----
            p2 = context.new_page()
            p2.on("pageerror", lambda e: page_errors.append(f"cc:{e}"))
            p2.goto(f"{URL}?cc=1")
            ready(p2)
            p2.wait_for_function(
                "window.__collabReady === true || window.__ready === true",
                timeout=30_000)
            md_cc = p2.evaluate("window.__md()")
            ts_before = p2.evaluate("window.__draftInfo().ts")
            type_at_end(" MARKER_E", p2)
            p2.wait_for_timeout(2500)  # 越过去抖窗（若门控失效会落稿）
            ts_after = p2.evaluate("window.__draftInfo().ts")
            dialog_state.update(action="accept", log=[])
            reload_js(p2)
            ready(p2)
            check("collab_slot_gates_all_three_faces",
                  p2.evaluate("window.__draftInfo().active === false")
                  and "MARKER_D" not in md_cc
                  and ts_before == ts_after
                  and dialog_state["log"] == [],
                  f"active={p2.evaluate('window.__draftInfo().active')} "
                  f"ts_same={ts_before == ts_after} "
                  f"dialogs={dialog_state['log']}")
            p2.close()

            # ---- 10. pagehide 终刷（去抖窗内刷新——草稿仍被写出）----
            page.bring_to_front()
            type_at_end(" MARKER_F")
            page.wait_for_function("window.__unsaved() === true", timeout=10_000)
            # pending 开窗 = 去抖未落（此刻草稿不含 F）——刷新只能靠 pagehide
            page.wait_for_function("window.__draftInfo().pending === true",
                                   timeout=10_000)
            dialog_state.update(action="accept", log=[])
            reload_js()
            ready()
            md_after = page.evaluate("window.__md()")
            check("pagehide_flush_catches_debounce_window",
                  dialog_state["log"] == ["beforeunload"]
                  and "MARKER_D" in md_after and "MARKER_F" in md_after,
                  f"dialogs={dialog_state['log']} has_f={'MARKER_F' in md_after}")

            # ---- 11. 恢复事件通告（宿主可消费面；init script 监听跨 reload）----
            dialog_state.update(action="accept", log=[])
            reload_js()
            ready()
            check("restored_event_dispatched_on_boot",
                  page.evaluate("window.__evDraft !== null"
                                " && window.__draftInfo().restoredTs !== null"),
                  f"ev={page.evaluate('window.__evDraft')} "
                  f"restored={page.evaluate('window.__draftInfo().restoredTs')}")

            # ---- 12. 全程零 pageerror ----
            check("zero_pageerror", not page_errors,
                  "; ".join(page_errors[:3]))

            page.close()
            context.close()
            browser.close()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps(
        {"meta": {"browser": "chromium-headless", "runner": "playwright",
                  "page": "web/dist/index.html",
                  "suite": "crash_recovery (M-2, #29 task 1)"},
         "tests": checks,
         "summary": {"total": len(checks), "failed": len(failed),
                     "pass": len(failed) == 0}},
        ensure_ascii=False, indent=1))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
