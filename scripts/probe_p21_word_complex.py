# -*- coding: utf-8 -*-
"""P2-1 验收探针：word-complex.docx 导入 → 页眉/页脚接入输出实测。

用法（仓库根，需 8123 端口 serve web/dist）：python3 scripts/probe_p21_word_complex.py
前提：web/dist 已构建（本分支 wasm）。
输出：markdown 前 6 行 + 末 3 行 + stats 关键键——PR 自测证据用。
"""
import base64
import json
import pathlib
import sys

from playwright.sync_api import sync_playwright

REPO = pathlib.Path(__file__).resolve().parents[1]
FIXTURE = REPO / "test-artifacts/TR2/fixtures/word-complex.docx"
URL = "http://127.0.0.1:8123/index.html"

JS = """(b64) => {
  const bin = atob(b64); const arr = new Uint8Array(bin.length);
  for (let i=0;i<bin.length;i++) arr[i] = bin.charCodeAt(i);
  const r = window.__importDocx(arr);
  return { ok: r.ok, md: String(window.__md()), stats: r.stats };
}"""


def main() -> int:
    b64 = base64.b64encode(FIXTURE.read_bytes()).decode()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        page = browser.new_page()
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.goto(URL)
        page.wait_for_function("window.__ready === true", timeout=30000)
        r = page.evaluate(JS, b64)
        browser.close()
    print("ok:", r["ok"], "| pageerrors:", errors)
    lines = r["md"].split("\n")
    print("--- md head (first 6 lines) ---")
    for line in lines[:6]:
        print(repr(line))
    print("--- md tail (last 3 lines) ---")
    for line in lines[-3:]:
        print(repr(line))
    s = r["stats"]
    keys = ["blocks", "headings", "paragraphs", "tables", "headers_skipped",
            "footers_skipped", "headers_imported", "footers_imported",
            "comments", "footnotes", "fields_dropped", "warnings"]
    print("--- stats ---")
    print(json.dumps({k: s.get(k) for k in keys}, ensure_ascii=False))
    return 0 if r["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
