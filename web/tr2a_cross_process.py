#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""TR-2A · 跨进程确定性（issue #11 §3）。

单进程内双跑无法排除进程内缓存/全局态——本驱动以**独立浏览器进程**
（每次 chromium.launch() = 独立 OS 进程，wasm 实例随页面冷启）为
「进程」单元：

  Run A → Run B → Run C（三轮冷启动，语料序固定）
  Run D → Run E（两轮冷启动，语料序打乱——输出必须与序无关）

每轮逐语料取双 digest：
  · 解析面：__inject(md) → __md() 的 SHA-256（投影确定性）
  · 导出面：__exportDocx() 字节的 SHA-256（导出确定性）

验收：3 次独立进程 × 乱序 2 次的结果表（cross-process-determinism.md）。
覆盖注记：OCR 面属标书仓（bid-parse）——引擎仓无 OCR 面，诚实登记
不伪装；导出面即引擎 wasm 导出（与标书共享同一 wasm 包）。
"""

import hashlib
import http.server
import json
import random
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent
OUT = WEB / "tests" / "results" / "cross_process_determinism.json"
Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript",
                               ".wasm": "application/wasm", ".mjs": "text/javascript"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


socketserver.TCPServer.allow_reuse_address = True

# 语料（跨层形态覆盖：标题/列表/表格/代码/长文/Unicode/嵌套——
# 与 testround A/B 组互补的确定性面，非语义攻击面）。
CORPUS = {
    "headings": "# 一级\n\n## 二级\n\n### 三级\n\n正文段。",
    "lists_mixed": "- 甲项\n- 乙项\n\n1. 第一\n2. 第二\n\n- [ ] 待办\n- [x] 完成",
    "table": "| 甲 | 乙 | 丙 |\n|---|---|---|\n| 一 | 二 | 三 |\n| 四 | 五 | 六 |",
    "code_fence": "```rust\nfn main() {\n    println!(\"hello\");\n}\n```",
    "unicode": "# 中文标题 🎯\n\n段落含 emoji 👨‍👩‍👧‍👦、CJK 扩展 𠀀、零宽 \u200b、\n与 bidi 覆盖 &#x202e; 字符。",
    "long_doc": "\n\n".join(f"第 {i} 段：{'内容' * 12}" for i in range(400)),
    "nested_quotes": "> 外层引用\n> > 内层引用\n> > 更深一层\n> 回到外层",
    "inline_marks": "正常**粗体**斜体*强调*~~删除~~`行内码`混合文本",
}


def run_round(pw, port, order, label):
    """一轮 = 一个独立浏览器进程（冷启动 wasm 实例）。"""
    browser = pw.chromium.launch()
    page = browser.new_page()
    page.goto(f"http://127.0.0.1:{port}/index.html")
    page.wait_for_function("window.__ready === true", timeout=60_000)
    digests = {}
    for key in order:
        md = CORPUS[key]
        page.evaluate("(s) => window.__inject(s)", md)
        page.wait_for_timeout(30)
        proj = page.evaluate("window.__md()")
        r = page.evaluate("window.__exportDocx()")
        assert r.get("ok"), f"{label}/{key}: export failed {r.get('code')}"
        v = r["bytes"]
        raw = bytes(v) if isinstance(v, list) else v
        digests[key] = {
            "parse_sha": hashlib.sha256(proj.encode("utf-8")).hexdigest(),
            "export_sha": hashlib.sha256(raw).hexdigest(),
            "export_bytes": len(raw),
        }
    page.close()
    browser.close()
    return digests


def main() -> int:
    keys = list(CORPUS)
    rounds = {}
    with socketserver.TCPServer(("127.0.0.1", 8945), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            # Run A/B/C：固定序，三轮独立进程
            for label in ("A", "B", "C"):
                rounds[label] = run_round(pw, 8945, keys, label)
                print(f"run {label} (fixed order) done")
            # Run D/E：乱序（随机打乱，seed 记录可复现）
            for label, seed in (("D", 20261027), ("E", 99887766)):
                order = list(keys)
                random.Random(seed).shuffle(order)
                rounds[label] = run_round(pw, 8945, order, label)
                print(f"run {label} (shuffled seed={seed}) done")

    # 比对：A=B=C=D=E（全语料双 digest 逐位一致）
    base = rounds["A"]
    mismatches = []
    table = []
    for key in keys:
        row = {"corpus": key, "parse_sha": base[key]["parse_sha"][:12],
               "export_sha": base[key]["export_sha"][:12],
               "export_bytes": base[key]["export_bytes"]}
        for label in ("B", "C", "D", "E"):
            for face in ("parse_sha", "export_sha", "export_bytes"):
                if rounds[label][key][face] != base[key][face]:
                    mismatches.append(f"{label}/{key}/{face}")
        row["stable"] = not any(m.startswith(tuple("BCDE")) and f"/{key}/" in m
                                for m in mismatches)
        table.append(row)

    result = {
        "meta": {
            "runs": "A/B/C fixed-order + D/E shuffled (seeds 20261027/99887766)",
            "process_model": "each run = fresh chromium OS process (cold wasm)",
            "faces": "parse=__md() sha256 · export=docx bytes sha256",
            "corpus_size": len(keys),
        },
        "rounds_digest_table": table,
        "mismatches": mismatches,
        "verdict": "DETERMINISTIC" if not mismatches else "NON-DETERMINISTIC",
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(result, ensure_ascii=False, indent=1))
    print(f"\nverdict: {result['verdict']} "
          f"({len(keys)} corpus × 5 rounds × 2 faces, mismatches={len(mismatches)})")
    print(f"-> {OUT}")
    return 0 if not mismatches else 1


if __name__ == "__main__":
    sys.exit(main())
