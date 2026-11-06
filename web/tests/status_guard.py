#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""#280（审计 P1-25）：状态栏字数/块数 revision 守卫 e2e.

更新模型背景（TR-UI 批 A-2）：状态栏 500ms 轮询。修复前 refresh
无条件调 `core.word_count_json()` + `core.blocks_count()`——前者每块
Rope→String 物化（全文档扫描 + 分配 churn），空闲态每秒 2 次全扫
（#280 现象二）。修复后以 `core.revision()` 为守卫：未变 → 沿用上次
文本零重算。

锁清单（4 项检查）：
  A. 空闲守卫（1）：注入文档后静置 2.2s（≥4 个轮询 tick）——
     word_count_json 恰调 1 次（首次 tick 开闸重算，此后零重算；
     修复前此口径 ≈5 次）。
  B. 编辑开闸（2）：单 op 编辑（revision +1）→ 1.2s 内恰再调 1 次
     （守卫开闸），再静置 1.2s 计数不增（回到守卫态）。
  C. 文本一致性（3）：状态栏字数文本与引擎口径（word_count_json
     七字段）在编辑后一致——守卫不得以陈旧值冒充（cache 正确性）。
  D. 收尾（4）：全程零 pageerror。

仪器化：core 方法包装计数（contract_gate __spy 同款口径——不触碰
产品代码）；__inject 重建核 → 包装在注入后施于新核实例。
"""

import http.server
import json
import socketserver
import sys
import threading
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_status_guard.json"

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


socketserver.TCPServer.allow_reuse_address = True

DOC_MD = "\n".join(
    f"第{i}章 投标方案概述：本方案覆盖数据中心机房建设与冷热通道隔离设计。" for i in range(1, 9)
)

# 仪器化（注入后施于新核；refresh 每 tick 重读 window.__core——方法
# 包装对轮询路径生效）。
JS_INSTRUMENT = """() => {
  const core = window.__core;
  const origWc = core.word_count_json.bind(core);
  const origBc = core.blocks_count.bind(core);
  window.__guardSpy = { wc: 0, blocks: 0 };
  core.word_count_json = (...a) => { window.__guardSpy.wc++; return origWc(...a); };
  core.blocks_count = (...a) => { window.__guardSpy.blocks++; return origBc(...a); };
  return true;
}"""

JS_EDIT = """(marker) => {
  const blocks = window.__blocks();
  const b = blocks.find(x => x.type === 'paragraph' && (x.text || '').length > 5) || blocks[0];
  if (!b) return { ok: false, code: 'no_block' };
  window.__core.set_clock_ms(BigInt(Date.now()));
  try {
    const ret = window.__core.replace_text_at_anchor(
      BigInt(b.id), 0, 0, marker, null);
    return { ok: true, block: Number(b.id) };
  } catch (err) { return { ok: false, msg: String(err) }; }
}"""


def main() -> int:
    if not (WEB / "dist" / "index.html").exists():
        print("FATAL dist/ missing — run `npm run build:wasm && npm run build` first")
        return 2

    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name
              + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8948), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8948/index.html")
            page.wait_for_function("window.__ready === true", timeout=60_000)

            # 注入文档（重建核）→ 仪器化新核。
            page.evaluate("md => window.__inject(md)", DOC_MD)
            page.evaluate(JS_INSTRUMENT)

            # ---- A. 空闲守卫：2.2s ≥ 4 tick，恰 1 次全文档扫描 ----
            time.sleep(2.2)
            spy_idle = page.evaluate("window.__guardSpy")
            # 注入后首个 tick：revision 与守卫初值（-2→0 路径或注入前旧值）
            # 必不相等 → 开闸恰 1 次；此后 revision 恒定 → 零重算。
            check(
                "idle_word_count_guard",
                spy_idle.get("wc") == 1 and spy_idle.get("blocks") == 1,
                f"2.2s 空闲后 wc={spy_idle.get('wc')} blocks={spy_idle.get('blocks')}"
                "（期望恰 1——修复前 ≈5 次/tick 全扫）",
            )

            # ---- B. 编辑开闸：revision +1 → 恰再 1 次，再守卫 ----
            edit = page.evaluate(JS_EDIT, "#280-marker")
            if not edit.get("ok"):
                check("edit_applied", False, f"编辑失败：{edit}")
            else:
                time.sleep(1.2)  # ≥2 tick：开闸重算 1 次
                spy_after_edit = page.evaluate("window.__guardSpy")
                wc_after_edit = spy_after_edit.get("wc", 99)
                time.sleep(1.2)  # 再静置：计数不得再增
                spy_settled = page.evaluate("window.__guardSpy")
                check(
                    "edit_reopens_guard_once",
                    wc_after_edit == 2 and spy_settled.get("wc") == 2,
                    f"编辑后 wc={wc_after_edit}，再静置后 wc={spy_settled.get('wc')}"
                    "（期望 2 → 2：开闸恰一次后回守卫态）",
                )

                # ---- C. 文本一致性：守卫缓存 = 引擎现值 ----
                texts = page.evaluate("""() => ({
                  counts: document.getElementById('sb-counts')?.textContent ?? '',
                  blocks: document.getElementById('sb-blocks')?.textContent ?? '',
                  engine: JSON.parse(window.__core.word_count_json()),
                  blocksCount: window.__core.blocks_count(),
                })""")
                engine_counts = (
                    f"{texts['engine']['total_chars']} 字 · {texts['engine']['total_words']} 词"
                    if texts["engine"]["total_chars"] or texts["engine"]["total_words"]
                    else None
                )
                # i18n 文案形如「N 字 / N 词」或「N 字 · N 词」——一致性判据 =
                # 状态栏文本含引擎两数值（数字为锚，分隔符随文案变体）。
                ok_text = (
                    str(texts["engine"]["total_chars"]) in (texts["counts"] or "")
                    and str(texts["engine"]["total_words"]) in (texts["counts"] or "")
                    and str(texts["blocksCount"]) in (texts["blocks"] or "")
                )
                check(
                    "statusbar_matches_engine",
                    ok_text,
                    f"statusbar={texts['counts']!r}/{texts['blocks']!r}"
                    f" engine(chars={texts['engine']['total_chars']},"
                    f" words={texts['engine']['total_words']},"
                    f" blocks={texts['blocksCount']}) ref={engine_counts!r}",
                )

            # ---- D. 零 pageerror ----
            check("zero_pageerror", not page_errors, "; ".join(page_errors[:3]))

            browser.close()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(
        {"suite": "status_guard", "issue": 280, "checks": checks,
         "ok": all(c["ok"] for c in checks)},
        ensure_ascii=False, indent=1))
    print(f"-> {OUT}")
    return 0 if all(c["ok"] for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
