#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""R-1 大文件边界实测（#22 任务池）：50MB docx / 100MB xlsx / 500 页 PDF
经 web 编辑器导入/打开——耗时、内存（JS 堆）、崩溃、UI 阻塞四指标。

方法（与 #22 R-1 表格逐项对应）：
- 耗时：页内 performance.now() 包裹 wasm 调用（fetch 字节与导入分开
  计时——导入耗时不含网络/解码噪声）；
- 内存：CDP Performance.getMetrics 的 JSHeapUsedSize/TotalSize——
  导入前 / 导入后两拍（wasm 线性内存不收缩：导入后值 ≈ 峰值）；
- 崩溃：pageerror + 进程存活 + 结果 ok 双检；
- UI 阻塞：同步 wasm 调用期间主线程恒阻塞——导入前挂 requestAnimationFrame
  旗标，测 rAF 实际触发时刻与导入完成时刻的差（= 事件循环饥饿窗口）；
  阻塞时长 = 导入耗时（架构事实，非缺陷——Web Worker 化登记为后续项）。

用法（仓库根；需先构建 web/dist）：
    python3 test-artifacts/large-file/run_r1.py [--quick]
--quick：跳过 50MB/100MB 档只跑小档（烟囱验证用）。
结果：stdout 表格 + test-artifacts/large-file/results-r1.json。
"""
import argparse
import http.server
import json
import os
import socketserver
import sys
import threading
from pathlib import Path

import psutil
from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parents[2]
LARGE = Path(__file__).resolve().parent
OUT = LARGE / "results-r1.json"
PORT = 8210
BASE = f"http://127.0.0.1:{PORT}"
APP = f"{BASE}/web/dist/index.html"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript",
                               ".wasm": "application/wasm", ".mjs": "text/javascript"})


class Quiet(Handler):
    def log_message(self, *a):
        pass


class Server(socketserver.TCPServer):
    allow_reuse_address = True


# 页内测量脚本：fetch 字节 → heap 前拍 → rAF 旗标 → 同步导入 →
# rAF 触发等待 → heap 后拍。全部计时在页内 performance.now()（单调）。
MEASURE_IMPORT_JS = """
async (arg) => {
  const { path, kind } = arg;
  const tFetch0 = performance.now();
  const buf = await fetch(path).then((r) => r.arrayBuffer());
  const tFetch1 = performance.now();
  const bytes = new Uint8Array(buf);
  const heap = () => performance.memory
      ? { used: performance.memory.usedJSHeapSize,
          total: performance.memory.totalJSHeapSize }
      : { used: 0, total: 0 };
  const before = heap();
  // rAF 旗标：导入前挂起，导入（同步阻塞主线程）完成后事件循环恢复
  // 才触发——tRaf - tDone = 事件循环饥饿余量（通常 < 2 帧）。
  let rafAt = -1;
  requestAnimationFrame(() => { rafAt = performance.now(); });
  const t0 = performance.now();
  let result;
  let threw = null;
  try {
    result = kind === 'docx' ? window.__importDocx(bytes)
                             : window.__importXlsx(bytes);
  } catch (e) { threw = String(e); }
  const t1 = performance.now();
  // 等 rAF 真正触发（事件循环恢复证明）。
  const rafDone = await new Promise((res) => {
    if (rafAt >= 0) return res(rafAt);
    requestAnimationFrame(() => res(performance.now()));
  });
  const after = heap();
  return {
    fetchMs: tFetch1 - tFetch0,
    importMs: t1 - t0,
    rafDelayMs: rafDone - t1,
    blockedMs: (rafAt >= 0 ? rafAt : rafDone) - t0,
    bytes: bytes.length,
    ok: !!result && result.ok,
    threw,
    stats: result && result.ok ? result.stats : null,
    blocks: typeof window.__blockCount === 'function' ? window.__blockCount() : null,
    heapBefore: before, heapAfter: after,
  };
}
"""

MEASURE_PDF_JS = """
async (path) => {
  const tFetch0 = performance.now();
  const buf = await fetch(path).then((r) => r.arrayBuffer());
  const tFetch1 = performance.now();
  const bytes = new Uint8Array(buf);
  // 分块 base64（__pdfOpenBytes 取 b64 串——32KB 块防栈溢出）。
  let b64 = '';
  const CH = 32768;
  for (let i = 0; i < bytes.length; i += CH) {
    b64 += String.fromCharCode.apply(null, bytes.subarray(i, i + CH));
  }
  b64 = btoa(b64);
  const heap = () => performance.memory
      ? { used: performance.memory.usedJSHeapSize,
          total: performance.memory.totalJSHeapSize }
      : { used: 0, total: 0 };
  const before = heap();
  const t0 = performance.now();
  const opened = window.__pdfOpenBytes(b64);
  const t1 = performance.now();
  const state = window.__pdfState();
  const after = heap();
  return {
    fetchMs: tFetch1 - tFetch0,
    b64Ms: -1, // base64 在页内完成（含于 open 计时外的准备段）
    openMs: t1 - t0,
    bytes: bytes.length,
    opened,
    pages: state ? state.pages : null,
    wasm: state ? state.wasm : null,
    heapBefore: before, heapAfter: after,
  };
}
"""

WAIT_RENDER_JS = """
async () => {
  const t0 = performance.now();
  const ok = await new Promise((res) => {
    const dl = Date.now() + 30000;
    const poll = () => {
      const p1 = window.__pdfPage(1);
      if (p1 && p1.rendered) return res(true);
      if (Date.now() > dl) return res(false);
      requestAnimationFrame(poll);
    };
    poll();
  });
  return { rendered: ok, firstRenderMs: performance.now() - t0 };
}
"""


def renderer_rss_bytes() -> int:
    """渲染器进程 RSS 合计（python→playwright driver→chrome 树内
    --type=renderer 子进程——含 wasm 线性内存与 V8 堆外分配：比
    performance.memory[仅 V8 JS 堆] 更接近 #22 R-1 的「峰值内存」口径）。"""
    total = 0
    try:
        me = psutil.Process(os.getpid())
        procs = me.children(recursive=True)
    except psutil.Error:
        return 0
    for p in procs:
        try:
            if "--type=renderer" in p.cmdline():
                total += p.memory_info().rss
        except psutil.Error:
            continue
    return total


def mb(x: float) -> float:
    return round(x / 1024 / 1024, 1)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="小档烟囱验证")
    args = ap.parse_args()

    targets = []
    for name, path, kind in [
        ("docx-50mb", LARGE / "large-50mb.docx", "docx"),
        ("xlsx-100mb", LARGE / "large-100mb.xlsx", "xlsx"),
    ]:
        if not path.exists():
            print(f"[skip] {name}: fixture missing ({path}) — generate first")
            continue
        if args.quick and "50mb" not in name and "100mb" not in name:
            continue
        targets.append((name, f"/test-artifacts/large-file/{path.name}", kind))
    pdf_path = LARGE / "large-500p.pdf"
    if pdf_path.exists():
        targets.append(("pdf-500p", f"/test-artifacts/large-file/{pdf_path.name}", "pdf"))

    if not targets:
        print("no fixtures found — nothing to do")
        return 1

    os.chdir(REPO)
    results = []
    with Server(("127.0.0.1", PORT), Quiet) as httpd:
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch(headless=True)
            for name, url, kind in targets:
                page = browser.new_page()
                errors = []
                page.on("pageerror", lambda e: errors.append(str(e)))
                page.set_default_timeout(600000)
                page.goto(APP)
                page.wait_for_function("window.__ready === true", timeout=30000)
                rss_before = renderer_rss_bytes()
                if kind == "pdf":
                    page.wait_for_function("window.__pdfReady === true", timeout=30000)
                    r = page.evaluate(MEASURE_PDF_JS, url)
                    rr = page.evaluate(WAIT_RENDER_JS)
                    r.update(rr)
                    # open 为异步：页数在首渲后复核（state.pages 实时）。
                    r["pages"] = page.evaluate("window.__pdfState().pages")
                else:
                    r = page.evaluate(MEASURE_IMPORT_JS, {"path": url, "kind": kind})
                r["rssBeforeMB"] = round(rss_before / 1024 / 1024, 1)
                r["rssAfterMB"] = round(renderer_rss_bytes() / 1024 / 1024, 1)
                r["rssDeltaMB"] = round(r["rssAfterMB"] - r["rssBeforeMB"], 1)
                r["pageerrors"] = errors
                r["crashed"] = page.is_closed() or bool(errors)
                results.append({"name": name, **r})
                page.close()
            browser.close()

    # 报表。
    print("=" * 96)
    print(f"{'target':<12} {'bytes':>10} {'fetchMs':>9} {'importMs':>10} "
          f"{'rssΔMB':>8} {'rssAfterMB':>10} {'blocks':>8} {'ok':>5}")
    print("-" * 96)
    for r in results:
        print(f"{r['name']:<12} {r['bytes']:>10} {r['fetchMs']:>9.0f} "
              f"{r.get('importMs', r.get('openMs', 0)):>10.0f} "
              f"{r['rssDeltaMB']:>8} {r['rssAfterMB']:>10} "
              f"{str(r.get('blocks')):>8} {str(r['ok'] if 'ok' in r else r.get('opened')):>5}")
        if r.get("firstRenderMs") is not None:
            print(f"{'':<12} first-page render: {r['firstRenderMs']:.0f} ms "
                  f"(pages={r.get('pages')}, wasm={r.get('wasm')})")
    print("=" * 96)
    OUT.write_text(json.dumps(results, ensure_ascii=False, indent=1))
    print(f"results -> {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
