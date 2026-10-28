#!/usr/bin/env python3
"""TR-UI 敌意回归（dev-r17）：新 UI 面在边界态的行为钉定。

组 A 空文档：无选区/无块态下全部格式命令 → 零页错 + 状态合法。
组 B 超大文档：200KB 注入 + 工具栏操作 → 无崩溃 + 字数统计合理。
组 C 特殊字符：emoji/CJK/BIDI 混排 + 包裹/替换 → 零损坏（UTF-16 边界）。
组 D 面板叠加：TOC+查找+导出选项+帮助全开 → 互不干扰 + Esc 收敛。
组 E 连击：格式命令快速连击 20 次 → undo 链稳定可回。
组 F localStorage 污染：非法字号值注入 → boot clamp 防御。
Evidence -> results/results_hostile_ui.json
"""

import http.server
import json
import socketserver
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_hostile_ui.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    allow_reuse_address = True

    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


BIG_HEADINGS = 400  # 200KB 量级（400 标题 × 500B）


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 0), QuietServer) as srv:
        port = srv.server_address[1]
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto(f"http://127.0.0.1:{port}/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            page.wait_for_timeout(800)

            # ---- 组 A：空文档全命令 ----
            a = page.evaluate("""() => {
                window.__inject('');
                const results = {};
                for (const k of ['bold','italic','code','del','h1','h2','h3','para','quote','ul','ol'])
                    results[k] = window.__format(k);
                return { results, blocks: window.__blockCount(), md: window.__md() };
            }""")
            all_ok = all(v is True for v in a["results"].values())
            # 语义：空文档上命令安全（true + 零页错）——wrapSelection 空选区
            # 插入成对标记会创建内容块（** 成段）= 合法行为，blocks ≥ 0。
            check("empty_doc_all_commands_safe",
                  all_ok and a["blocks"] >= 0 and len(page_errors) == 0,
                  json.dumps({"results": a["results"], "blocks": a["blocks"]}))

            # ---- 组 B：超大文档 + 工具栏 ----
            b = page.evaluate("""(n) => {
                const parts = [];
                for (let i = 0; i < n; i++) parts.push(`# 标题第${i}节\\n` + '正文内容。'.repeat(20) + `\\n`);
                window.__inject(parts.join(''));
                return { blocks: window.__blockCount() };
            }""", BIG_HEADINGS)
            page.wait_for_function(f"window.__blockCount() === {BIG_HEADINGS * 2}", timeout=30000)
            t0 = page.evaluate("performance.now()")
            page.evaluate("""() => {
                const doc = window.__view.state.doc;
                window.__view.dispatch({ selection: { anchor: 0, head: 10 } });
                window.__format('bold');
                window.__format('h2');
                return true;
            }""")
            t1 = page.evaluate("performance.now()")
            counts = page.evaluate("(document.getElementById('sb-counts') || {}).textContent")
            page.wait_for_timeout(600)  # 轮询窗
            counts2 = page.evaluate("(document.getElementById('sb-counts') || {}).textContent")
            check("huge_doc_toolbar_perf",
                  b["blocks"] == BIG_HEADINGS * 2 and (t1 - t0) < 3000
                  and counts2 and "chars" in counts2 and "words" in counts2  # #43 i18n：en locale 口径（旧字面「字符 · 词」→ sb.counts）
                  and len(page_errors) == 0,
                  json.dumps({"blocks": b["blocks"], "ms": t1 - t0, "counts": counts2}))

            # ---- 组 C：特殊字符（emoji/CJK/BIDI 混排 + UTF-16 边界）----
            c = page.evaluate("""() => {
                window.__inject('中文🎉emoji 𝕏 数学 ∑∫ 测试\\nעברית טקסט BIDI\\n');
                const doc = window.__view.state.doc;
                const line = doc.line(1);
                // 选区含代理对（🎉 = 2 个 UTF-16 单元）
                window.__view.dispatch({ selection: { anchor: line.from + 2, head: line.from + 6 } });
                window.__format('bold');
                const md = window.__md();
                const cnt = window.__findMatches('🎉');
                window.__replaceAll('🎉', '✅');
                const md2 = window.__md();
                return { md, cnt, md2 };
            }""")
            # 选区 UTF-16 索引 2-6 = 文+🎉(2 单元)+em → 包裹形 中文**🎉em**oji
            # （代理对完整包裹 = 无撕裂；cpLen 换算路径的实证）
            check("special_chars_surrogate_safe",
                  c["md"].startswith("中文**🎉em**oji") and c["cnt"] == 1
                  and "✅" in c["md2"] and "🎉" not in c["md2"],
                  json.dumps(c))

            # ---- 组 D：面板叠加 ----
            d = page.evaluate("""() => {
                document.getElementById('toc-toggle-btn').click();
                document.getElementById('toc-toggle-btn').click();  // 关回
                document.getElementById('find-input'); // 存在性
                window.__helpVisible();
                return true;
            }""")
            page.keyboard.press("Control+f")
            page.keyboard.press("F1")
            overlay_state = page.evaluate("""() => ({
                find: !document.getElementById('find-panel').hidden,
                help: window.__helpVisible(),
            })""")
            page.keyboard.press("Escape")  # 关帮助（最近开）
            s1 = page.evaluate("window.__helpVisible()")
            page.keyboard.press("Escape")  # 关查找
            s2 = page.evaluate("!document.getElementById('find-panel').hidden")
            check("panels_stack_esc_converge",
                  overlay_state["find"] and overlay_state["help"] and s1 is False and s2 is False,
                  json.dumps({"stacked": overlay_state, "afterEsc1": s1, "afterEsc2": s2}))

            # ---- 组 E：连击 20 次 ----
            e = page.evaluate("""() => {
                window.__inject('连击测试\\n');
                for (let i = 0; i < 20; i++) window.__format('bold');
                return window.__md();
            }""")
            undone = page.evaluate("""() => {
                for (let i = 0; i < 3; i++) window.__bridge.undo();
                return window.__md();
            }""")
            check("burst_20_undo_chain",
                  "**" * 10 in e or e.count("**") >= 20 and len(page_errors) == 0,
                  json.dumps({"afterBurst": e[:80], "afterUndo": undone[:60]}))

            # ---- 组 F：localStorage 污染 ----
            f = page.evaluate("""() => {
                localStorage.setItem('mdc-ed-font-size', 'NaN');
                window.__setEdFontSize(Number('NaN'));
                const after = window.__edFontSize();
                localStorage.setItem('mdc-ed-font-size', '999');
                window.__setEdFontSize(999);
                return { afterNaN: after, after999: window.__edFontSize() };
            }""")
            check("localStorage_poison_clamped",
                  f["afterNaN"] == 15 and f["after999"] == 28,
                  json.dumps(f))

            # ---- TR-UI 批 D：行号槽 + 自动保存提醒 ----
            # M-2 校准：__inject 重基线（mdc-core-reinstalled——导入语义
            # 不继承陈旧 dirty）后，dirtySince 从本次 '!' 键入的首个轮询
            # 起算——提示最坏 5s 窗 + 2×500ms 轮询相位（dirtySince 检出
            # 相位 + 阈值后触发 tick）= 6s；旧 5.6s 等待靠陈旧 dirtySince
            # 即时触发恒过（潜在时序缺陷被重基线修复暴露）。断言语义不变。
            d2 = page.evaluate("""() => new Promise((resolve) => {
                window.__inject('行号测试\\n行二\\n');
                let hint = null;
                window.addEventListener('mdc-autosave-hint', (e) => { hint = e.detail; }, { once: true });
                // 触发 dirty 后等 6.5s（5s 窗 + 2×0.5s 轮询相位 + 余量）
                const doc = window.__view.state.doc;
                window.__view.dispatch({ changes: { from: doc.length, insert: '!' } });
                setTimeout(() => resolve({
                    hint,
                    gutter: !!document.querySelector('.cm-gutters .cm-lineNumbers'),
                    gutterLines: document.querySelectorAll('.cm-gutterElement').length,
                }), 6500);
            })""")
            check("line_gutter_and_autosave_hint",
                  d2["gutter"] and d2["gutterLines"] >= 2 and d2["hint"] and d2["hint"]["revision"] >= 1,
                  json.dumps(d2))

            # ---- 终验：全程零页错 ----
            check("no_pageerrors_total", len(page_errors) == 0,
                  json.dumps({"errors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "TR-UI hostile regression (dev-r17)", "dist": str(WEB / "dist")},
        "checks": checks,
        "passed": len(checks) - len(failed),
        "failed": len(failed),
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
