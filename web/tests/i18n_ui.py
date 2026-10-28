#!/usr/bin/env python3
"""#43 i18n + UI 中文化 e2e：zh-CN locale 全中文面 / en locale 零回归 /
appearance.language 设置覆盖与持久化 / 证据面板默认折叠 / PDF 入口强化 /
主题标签实际生效模式 / 成功状态绿显。

语言解析序（CONTRACT §4 v1.6 单一真源 initI18n）：
  setting appearance.language（'auto'|'en'|'zh-CN'）> 'auto' → navigator.language。
切换语义 = localStorage 镜像 mdc-language + location.reload()（全页重建）。
Evidence -> results/results_i18n_ui.json
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
FIXTURE = WEB / "tests" / "fixtures" / "import_sample.xlsx"
OUT = WEB / "tests" / "results" / "results_i18n_ui.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    fixture_b64 = base64.b64encode(FIXTURE.read_bytes()).decode()

    with socketserver.TCPServer(("127.0.0.1", 8952), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()

            # ---- A. zh-CN locale（auto → zh）：所有可见文本中文 ----
            ctx_zh = browser.new_context(locale="zh-CN", color_scheme="dark")
            page = ctx_zh.new_page()
            page.on("pageerror", lambda e: page_errors.append(f"zh:{e}"))
            page.goto("http://127.0.0.1:8952/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            # pdf/grid 面板异步启动（pdfjs + wasm 链）——引导卡就绪门。
            page.wait_for_function(
                "window.__pdfReady === true && window.__gridReady === true", timeout=30000)
            zh = page.evaluate("""() => ({
                contract: window.__contractVersion,
                lang: window.__settings().appearance.language,
                tabs: ['tab-doc','tab-grid','tab-pdf','tab-canvas'].map((id) =>
                    document.getElementById(id).textContent),
                importDocx: document.querySelector('label[for=docx-file] span').textContent,
                importXlsx: document.querySelector('label[for=xlsx-file] span').textContent,
                exportDocx: document.querySelector('#docx-export-btn span').textContent,
                openGrid: document.querySelector('label[for=grid-file] span').textContent,
                settings: document.querySelector('#settings-btn span').textContent,
                findPh: document.getElementById('find-input').placeholder,
                theme: document.getElementById('theme-label').textContent,
                gridGuide: document.querySelector('#grid-guide .sg-title').textContent,
                pdfGuide: document.querySelector('#pdf-guide .sg-title').textContent,
                pdfOpen: (document.querySelector('label[for=pdf-file]') || {}).textContent,
                pdfOpenProminent: !!(document.querySelector('label.pdf-open-btn')),
                booting: document.getElementById('debug').textContent,
                sbSaved: document.getElementById('sb-save').textContent,
            })""")
            check("zh_locale_all_visible_text_chinese",
                  zh["contract"] == 1.4 and zh["lang"] == "auto"
                  and zh["tabs"] == ["文档", "表格", "PDF", "画布"]
                  and zh["importDocx"] == "导入 Word" and zh["importXlsx"] == "导入 Excel"
                  and zh["exportDocx"] == "导出 Word" and zh["openGrid"] == "表格页打开 Excel"
                  and zh["settings"] == "设置"
                  and zh["findPh"] == "查找…" and zh["theme"] == "跟随系统（暗）"
                  and zh["gridGuide"] == "未打开工作簿" and zh["pdfGuide"] == "未打开 PDF"
                  and zh["pdfOpen"] == "打开 PDF" and zh["pdfOpenProminent"]
                  and zh["booting"].startswith("就绪") and zh["sbSaved"] == "已保存",
                  json.dumps(zh, ensure_ascii=False))

            # ---- A2. zh：导入状态行中文 + 成功绿显 + 联动后缀 ----
            page.evaluate("(b64) => window.__importXlsx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)))",
                          fixture_b64)
            page.wait_for_function(
                "window.__lastXlsxImport && window.__lastXlsxImport.grid"
                " && window.__lastXlsxImport.grid.ok === true", timeout=15000)
            zh_status = page.evaluate("""() => ({
                text: document.getElementById('xlsx-status').textContent,
                ok: document.getElementById('xlsx-status').classList.contains('status-ok'),
                err: document.getElementById('xlsx-status').classList.contains('xlsx-error'),
            })""")
            check("zh_import_status_chinese_and_green",
                  zh_status["ok"] and not zh_status["err"]
                  and zh_status["text"].startswith("已导入：")
                  and "3 个工作表" in zh_status["text"] and "2 个表格" in zh_status["text"]
                  and "工作簿已载入「表格」页（3 个工作表）" in zh_status["text"],
                  json.dumps(zh_status, ensure_ascii=False))

            # ---- A3. 证据面板默认折叠 + 展开可用（#43 问题 2）----
            ev = page.evaluate("""() => {
                const row = document.getElementById('main-row');
                const collapsed = row.classList.contains('ev-collapsed');
                const w = document.getElementById('evidence-panel').offsetWidth;
                return { collapsed, w, btn: !!document.getElementById('evidence-collapse-btn') };
            }""")
            check("evidence_panel_collapsed_by_default",
                  ev["collapsed"] and ev["w"] == 0 and ev["btn"],
                  json.dumps(ev))
            page.click("#evidence-collapse-btn")
            ev2 = page.evaluate("""() => ({
                collapsed: document.getElementById('main-row').classList.contains('ev-collapsed'),
                w: document.getElementById('evidence-panel').offsetWidth,
                counts: document.getElementById('ev-counts').textContent,
                status: document.getElementById('ev-status').textContent,
                title: (document.querySelector('#ev-toolbar') || {}).textContent ?? '',
            })""")
            check("evidence_panel_expand_works_zh",
                  not ev2["collapsed"] and ev2["w"] > 0
                  and ev2["counts"] == "无会话" and "与左侧编辑文档无关" in ev2["status"],
                  json.dumps(ev2, ensure_ascii=False))
            ctx_zh.close()

            # ---- B. en locale（默认）：既有英文字面零漂移（回归锚）----
            ctx_en = browser.new_context(locale="en-US", color_scheme="dark")
            page = ctx_en.new_page()
            page.on("pageerror", lambda e: page_errors.append(f"en:{e}"))
            page.goto("http://127.0.0.1:8952/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            page.wait_for_function(
                "window.__pdfReady === true && window.__gridReady === true", timeout=30000)
            en = page.evaluate("""() => ({
                tabs: ['tab-doc','tab-grid','tab-pdf','tab-canvas'].map((id) =>
                    document.getElementById(id).textContent),
                importDocx: document.querySelector('label[for=docx-file] span').textContent,
                findPh: document.getElementById('find-input').placeholder,
                theme: document.getElementById('theme-label').textContent,
                gridGuide: document.querySelector('#grid-guide .sg-title').textContent,
                lang: window.__settings().appearance.language,
            })""")
            check("en_locale_regression_anchor",
                  en["tabs"] == ["Document", "Grid", "PDF", "Canvas"]
                  and en["importDocx"] == "import .docx"
                  and en["findPh"] == "Find…" and en["theme"] == "system (dark)"
                  and en["gridGuide"] == "No workbook open" and en["lang"] == "auto",
                  json.dumps(en))

            # ---- B2. en：导入状态行英文字面逐字节（组合器回归锚）----
            page.evaluate("(b64) => window.__importXlsx(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)))",
                          fixture_b64)
            page.wait_for_function(
                "window.__lastXlsxImport && window.__lastXlsxImport.grid"
                " && window.__lastXlsxImport.grid.ok === true", timeout=15000)
            en_status = page.evaluate("document.getElementById('xlsx-status').textContent")
            check("en_import_status_byte_identical",
                  en_status.startswith("imported: 3 sheets · 2 tables · 6 rows · 1 empty")
                  and "1 formula kept" in en_status and "1 link kept" in en_status
                  and "1 merge" in en_status and "1 warning" in en_status
                  and "workbook in grid (3 sheets)" in en_status,
                  en_status)

            # ---- C. 设置覆盖：en locale 强制 zh-CN → reload 后中文（v1.6 消费链）----
            ok_set = page.evaluate("window.__setSetting('appearance.language', 'zh-CN')")
            page.wait_for_load_state("load", timeout=15000)
            page.wait_for_function("window.__ready === true", timeout=30000)
            override = page.evaluate("""() => ({
                tabDoc: document.getElementById('tab-doc').textContent,
                lang: window.__settings().appearance.language,
                ls: localStorage.getItem('mdc-language'),
            })""")
            check("setting_overrides_locale_zh",
                  ok_set is True and override["tabDoc"] == "文档"
                  and override["lang"] == "zh-CN" and override["ls"] == "zh-CN",
                  json.dumps(override, ensure_ascii=False))

            # ---- C2. 非法值拒（域 auto|en|zh-CN 外 = false 零事件）----
            rejected = page.evaluate("""() => ({
                fr: window.__setSetting('appearance.language', 'fr'),
                num: window.__setSetting('appearance.language', 1),
                zhTW: window.__setSetting('appearance.language', 'zh-TW'),
                stillZh: document.getElementById('tab-doc').textContent,
            })""")
            check("invalid_language_rejected",
                  rejected["fr"] is False and rejected["num"] is False
                  and rejected["zhTW"] is False and rejected["stillZh"] == "文档",
                  json.dumps(rejected, ensure_ascii=False))

            # ---- C3. 回 auto（en locale → 英文；清理镜像）----
            page.evaluate("window.__setSetting('appearance.language', 'auto')")
            page.wait_for_load_state("load", timeout=15000)
            page.wait_for_function("window.__ready === true", timeout=30000)
            back = page.evaluate("""() => ({
                tabDoc: document.getElementById('tab-doc').textContent,
                ls: localStorage.getItem('mdc-language'),
            })""")
            check("auto_restores_locale_en",
                  back["tabDoc"] == "Document" and back["ls"] == "auto",
                  json.dumps(back))
            ctx_en.close()
            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "#43 i18n + UI zh-CN e2e", "dist": str(WEB / "dist")},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
