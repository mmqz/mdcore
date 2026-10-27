#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""TR-2B L1 · CommonMark spec 全量 + markdown-it 自有套件注入（issue #11 §4.3）。

扒取来源（provenance 见 external-corpus/provenance.json）：
  · commonmark/commonmark-spec spec.txt（CC-BY-SA 4.0——输入用例）：
    32 反引号围栏 example 块（markdown + '.' + html）全量解析注入；
  · markdown-it/markdown-it test/fixtures/commonmark/good.txt（MIT）：
    40 波浪线分隔块注入。

判定面（L1 注入三问）：
  1. 幂等到达：inject(md) → md1 = __md() → inject(md1) → md2 → md1 == md2
     （投影在一步内到定点——第二轮投影稳定）；
  2. 零崩溃：页面存活（__blockCount() 可调）；
  3. 内容面（启发式——非判定）：原文非空白字符在投影中的保留率
     （markdown 语法字符的合法变换会使该值 < 1，按 section 统计
     供人工判读「规范化差异 vs 内容丢失」）。
"""

import http.server
import json
import re
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent
CORPUS = WEB.parent / "test-artifacts" / "external-corpus"
OUT = WEB / "tests" / "results" / "tr2b_l1_external.json"
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript",
                               ".wasm": "application/wasm", ".mjs": "text/javascript"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True


def parse_spec_examples(text):
    """commonmark spec.txt example 块解析（32 反引号围栏）。"""
    examples = []
    section = ""
    lines = text.split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        m = re.match(r"^#{1,6} (.+)$", line)
        if m:
            section = m.group(1).strip()
        if re.match(r"^`{32} example(\s+\S.*)?$", line):
            # 收集至闭合围栏
            body = []
            i += 1
            while i < len(lines) and not re.match(r"^`{32}$", lines[i]):
                body.append(lines[i])
                i += 1
            # body = markdown ... '.' ... html
            try:
                dot = body.index(".")
                md = "\n".join(body[:dot])
                html = "\n".join(body[dot + 1:])
                disabled = "disabled" in line
                examples.append(
                    {"section": section, "md": md, "html": html, "disabled": disabled})
            except ValueError:
                pass
        i += 1
    return examples


def parse_good_txt(text):
    """markdown-it good.txt 解析（40 波浪线分隔）。"""
    tests = []
    blocks = re.split(r"^~{42}$", text, flags=re.M)
    for b in blocks:
        lines = [ln for ln in b.split("\n")]
        # 块内形态：['', 'src line: NNN', '', '.', input..., '.', expected..., '.', '']
        dots = [i for i, ln in enumerate(lines) if ln == "."]
        if len(dots) >= 2:
            md = "\n".join(lines[dots[0] + 1:dots[1]])
            tests.append({"section": "markdown-it", "md": md})
    return tests


def content_chars(s):
    """非空白字符集（内容面启发式）。"""
    return [c for c in s if not c.isspace()]


def main() -> int:
    spec_examples = parse_spec_examples(
        (CORPUS / "commonmark" / "spec.txt").read_text(encoding="utf-8"))
    good_tests = parse_good_txt(
        (CORPUS / "markdown-it" / "good.txt").read_text(encoding="utf-8"))
    # disabled 例（规范自身排除项）跳过但计数
    enabled = [e for e in spec_examples if not e["disabled"]]
    print(f"spec examples: {len(spec_examples)} (enabled {len(enabled)}, "
          f"disabled {len(spec_examples) - len(enabled)}); markdown-it: {len(good_tests)}")

    results = {"spec": [], "markdown_it": []}
    stats = {"spec": {"total": 0, "idempotent": 0, "alive": 0, "preserve_sum": 0.0},
             "markdown_it": {"total": 0, "idempotent": 0, "alive": 0, "preserve_sum": 0.0}}

    with socketserver.TCPServer(("127.0.0.1", 8946), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.goto("http://127.0.0.1:8946/index.html")
            page.wait_for_function("window.__ready === true", timeout=60_000)

            def run_case(face, case):
                try:
                    r = page.evaluate("""(md) => {
                        window.__inject(md);
                        const md1 = window.__md();
                        window.__inject(md1);
                        const md2 = window.__md();
                        return { ok: true, md1, md2,
                                 alive: typeof window.__blockCount() === 'number' };
                    }""", case["md"])
                except Exception as e:  # 页面崩溃/求值异常
                    return {"section": case["section"], "idempotent": False,
                            "alive": False, "preserve": 0.0, "error": str(e)[:120]}
                src_chars = content_chars(case["md"])
                proj_chars = content_chars(r.get("md1", ""))
                from collections import Counter
                sc, pc = Counter(src_chars), Counter(proj_chars)
                kept = sum(min(sc[c], pc[c]) for c in sc)
                preserve = kept / max(1, sum(sc.values()))
                return {"section": case["section"],
                        "idempotent": r["md1"] == r["md2"],
                        "alive": r["alive"],
                        "preserve": round(preserve, 4),
                        "error": None}

            for case in enabled:
                res = run_case("spec", case)
                results["spec"].append(res)
                s = stats["spec"]
                s["total"] += 1
                s["idempotent"] += res["idempotent"]
                s["alive"] += res["alive"]
                s["preserve_sum"] += res["preserve"]
            for case in good_tests:
                res = run_case("markdown_it", case)
                results["markdown_it"].append(res)
                s = stats["markdown_it"]
                s["total"] += 1
                s["idempotent"] += res["idempotent"]
                s["alive"] += res["alive"]
                s["preserve_sum"] += res["preserve"]
            page.close()
            browser.close()

    for face, s in stats.items():
        s["idempotent_rate"] = round(s["idempotent"] / max(1, s["total"]), 4)
        s["alive_rate"] = round(s["alive"] / max(1, s["total"]), 4)
        s["preserve_avg"] = round(s["preserve_sum"] / max(1, s["total"]), 4)
    # 低保留率 section 汇总（人工判读清单）
    lows = {}
    for res in results["spec"]:
        if res["preserve"] < 0.5:
            lows.setdefault(res["section"], []).append(res["preserve"])
    results["low_preserve_sections"] = {
        k: {"n": len(v), "avg": round(sum(v) / len(v), 3)} for k, v in lows.items()}
    results["stats"] = stats

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(results, ensure_ascii=False, indent=1))
    print(json.dumps(stats, ensure_ascii=False, indent=1))
    print("low-preserve sections:", json.dumps(results["low_preserve_sections"], ensure_ascii=False))
    print(f"-> {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
