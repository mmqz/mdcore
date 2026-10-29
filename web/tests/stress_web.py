#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""M-3（#29 任务 2）并发压力测试 e2e：真实压力负载四场景（新文件——
不动 concurrent_web.py 既有 16 项）。

Lead 场景表逐条（15:03 指令）：

  S1. 3-5 页同时编辑同文档（本套件取上限 5 页，cc=1..5 同 context
      同源 BroadcastChannel）：5 页各改独占块（快速连发不等待收敛）
      + 5 页同块尾部并发插入（CRDT 无丢失——所有插入存活）→
      5 页 doc JSON 全等 + 全部标记在场。
  S2. 高频编辑 ≥10 次/秒持续 60 秒：页内 JS 驱动器 90ms 间隔连发
      __collabExec replace_text（双块交替计数追加），对页 1Hz 伴跑
      编辑；驱动器自记逐 op 时间戳 → 实测平均频率、零错误、无丢失
      （计数块终态逐位核验：#0..#659 全在场）。
  S3. 断连/重连循环 10 次：每轮 p2 断连（冻结实证：p1 标记编辑
      不达 p2）→ 重连 → catch-up 收敛（doc 全等 + 标记在场）。
  S4. 全程内存监控：--enable-precise-memory-info 启动；页内采样器
      2s 周期记 {js 堆, 引擎记账字节}（树/日志/历史数值叶总和）；
      增长 < 50MB（js 堆与引擎面分别断言）。

结果 -> tests/results/results_stress_web.json（端口 8959），
含逐 op 时间戳与内存曲线（Lead 要求「真实数据：耗时/内存曲线」）。
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
DIST = WEB / "dist"
PORT = 8959
OUT = WEB / "tests" / "results" / "results_stress_web.json"

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
    page_errors: dict[str, list[str]] = {}
    curve: list[dict] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name
              + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return bool(ok)

    def wait_until(pred, timeout=15.0, interval=0.35):
        """轮询断言器：超时返回 None（区别于 False——谓词可能返回 falsy 值）。"""
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                r = pred()
            except Exception:  # noqa: BLE001 —— 探针竞态按未就绪处理
                r = None
            if r:
                return r
            time.sleep(interval)
        return None

    def collab_exec(page, actor, cmd):
        return page.evaluate("([a, c]) => window.__collabExec(a, c)",
                             [actor, json.dumps(cmd)])

    def doc_snap(page):
        return page.evaluate("() => JSON.stringify(window.__collabDoc())")

    def mem_sample(page):
        """{js: 字节, engine: 引擎记账数值叶总和}（树+日志+历史）。"""
        return page.evaluate("""() => {
            const js = performance.memory ? performance.memory.usedJSHeapSize : -1;
            let engine = 0;
            try {
                const walk = (v) => {
                    if (typeof v === 'number') engine += v;
                    else if (v && typeof v === 'object') for (const k of Object.keys(v)) walk(v[k]);
                };
                walk(window.__memStats());
            } catch (e) { engine = -1; }
            return { js, engine };
        }""")

    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as pw:
            # 精确内存：Chromium 默认 performance.memory 分桶 ~1MB——
            # 50MB 判据需要精确值（R-1 登记的口径差异教训）。
            browser = pw.chromium.launch(args=["--enable-precise-memory-info"])
            context = browser.new_context()
            pages = []
            for slot in range(1, 6):
                p = context.new_page()
                errs: list[str] = []
                page_errors[f"p{slot}"] = errs
                p.on("pageerror", lambda e, k=f"p{slot}": errs.append(str(e)))
                p.goto(f"http://127.0.0.1:{PORT}/index.html?cc={slot}")
                p.wait_for_function(
                    "window.__ready === true && window.__collabReady === true",
                    timeout=60_000)
                pages.append(p)
            p1, p2 = pages[0], pages[1]
            check("five_pages_booted_collab_ready",
                  all(len(page_errors[f"p{i}"]) == 0 for i in range(1, 6)),
                  json.dumps({k: v[:2] for k, v in page_errors.items() if v}))

            # 内存采样器（p1 = 压力主战场；2s 周期，终局收割曲线）。
            p1.evaluate("""() => {
                window.__stressMem = [];
                const t0 = Date.now();
                window.__stressMemTimer = setInterval(() => {
                    let engine = 0;
                    try {
                        const walk = (v) => {
                            if (typeof v === 'number') engine += v;
                            else if (v && typeof v === 'object')
                                for (const k of Object.keys(v)) walk(v[k]);
                        };
                        walk(window.__memStats());
                    } catch (e) { engine = -1; }
                    window.__stressMem.push({
                        t: Date.now() - t0,
                        js: performance.memory ? performance.memory.usedJSHeapSize : -1,
                        engine,
                    });
                }, 2000);
                return true;
            }""")
            mem_baseline = mem_sample(p1)

            # ── S1. 5 页同时编辑 ────────────────────────────────
            load_cmd = {"t": "load_blocks", "blocks": [
                {"kind": "p", "attrs": {}, "text": f"S1 base paragraph {i}."}
                for i in range(1, 7)  # 块 0-4 = 各页独占；块 5 = 同块并发
            ]}
            r_load = collab_exec(p1, "human", load_cmd)
            ids = (r_load.get("receipt") or {}).get("ids") if r_load.get("ok") else None
            s1_ok = r_load.get("ok") is True and ids is not None and len(ids) == 6
            if s1_ok:
                # 5 页各改独占块——快速连发（不等待收敛 = 真同时）。
                # actor ∈ {human, ai}（Rust 枚举白名单——页身份由 slot
                # client_id 在 CRDT 层承载，actor 只是命令来源标签）。
                for i in range(5):
                    collab_exec(pages[i], "human" if i % 2 == 0 else "ai", {
                        "t": "replace_text", "block_id": ids[i],
                        "start": 0, "end": 2, "insert": f"[P{i + 1}-EDIT]"})
                # 5 页同块尾部并发插入（各自当下视图定位——单 evaluate
                # 内读长+执行，窗口最小化；CRDT 无丢失判据 = 全部在场）。
                for i in range(5):
                    pages[i].evaluate(
                        """([actor, tag, bid]) => {
                          const walk = (bs) => { for (const b of bs) {
                            if (b.block_id === bid) return b;
                            const hit = walk(b.children || []); if (hit) return hit; }
                            return null; };
                          const b = walk(window.__collabDoc());
                          if (!b) return {ok: false};
                          return window.__collabExec(actor, JSON.stringify({
                            t: 'replace_text', block_id: bid,
                            start: b.text.length, end: b.text.length,
                            insert: ' <' + tag + '>'}));
                        }""", [("human" if i % 2 == 0 else "ai"), f"P{i + 1}", ids[5]])
            # 收敛：5 页 doc JSON 全等 + 全部标记在场。
            def all_converged():
                snaps = [doc_snap(p) for p in pages]
                if any(s != snaps[0] for s in snaps):
                    return None
                s = snaps[0]
                if not all(f"[P{i}-EDIT]" in s for i in range(1, 6)):
                    return None
                if not all(f"<P{i}>" in s for i in range(1, 6)):
                    return None
                return s
            conv1 = wait_until(all_converged, timeout=20) if s1_ok else None
            check("s1_five_pages_concurrent_edit_convergence",
                  s1_ok and conv1 is not None
                  and all(f"[P{i}-EDIT]" in (conv1 or "") for i in range(1, 6))
                  and all(f"<P{i}>" in (conv1 or "") for i in range(1, 6)),
                  json.dumps({"load_ok": s1_ok, "converged": conv1 is not None},
                             ensure_ascii=False))
            mem_after_s1 = mem_sample(p1)

            # ── S2. ≥10Hz × 60s 高频编辑 ────────────────────────
            # 时序：先起 p2 伴跑器（1Hz 读 + 每 10 次一记真实追加编辑——
            # 保持通道活跃且证对页活着），再进 p1 阻塞驱动器（60s Promise：
            # 双块交替计数追加；85ms×720 op ≈ 61s ≈ 11.8Hz——双判据裕度：
            # ≥60s 持续 且 ≥10Hz 同时满足）。
            p2.evaluate("""(bid) => {
                window.__s2Peer = {reads: 0, edits: 0, stop: false, marks: []};
                const tick = () => {
                    if (window.__s2Peer.stop) return;
                    window.__s2Peer.reads += JSON.stringify(window.__collabDoc()).length;
                    window.__s2Peer.edits += 1;
                    if (window.__s2Peer.edits % 10 === 0) {
                        const walk = (bs) => { for (const b of bs) {
                            if (b.block_id === bid) return b;
                            const hit = walk(b.children || []); if (hit) return hit; }
                            return null; };
                        const b = walk(window.__collabDoc());
                        if (b) {
                            const m = '[PEER' + window.__s2Peer.edits + ']';
                            window.__s2Peer.marks.push(m);
                            window.__collabExec('p2', JSON.stringify({
                                t: 'replace_text', block_id: bid,
                                start: b.text.length, end: b.text.length,
                                insert: m}));
                        }
                    }
                    setTimeout(tick, 1000);
                };
                tick();
                return true;
            }""", ids[4] if s1_ok else "")
            drv = p1.evaluate("""(ids) => new Promise((resolve) => {
                const ops = [];
                const errors = [];
                const t0 = performance.now();
                let i = 0;
                const walk = (bs, bid) => { for (const b of bs) {
                    if (b.block_id === bid) return b;
                    const hit = walk(b.children || [], bid); if (hit) return hit; }
                    return null; };
                const timer = setInterval(() => {
                    const bid = ids[i % 2];
                    const b = walk(window.__collabDoc(), bid);
                    if (!b) { errors.push({i, code: 'block_missing'}); }
                    else {
                        const r = window.__collabExec('human', JSON.stringify({
                            t: 'replace_text', block_id: bid,
                            start: b.text.length, end: b.text.length,
                            insert: ' #' + i + ';'}));
                        if (!r || r.ok !== true) errors.push({i, code: r && r.code});
                        else ops.push({i, at: Math.round(performance.now() - t0)});
                    }
                    i += 1;
                    if (i >= 720) {
                        clearInterval(timer);
                        const span = performance.now() - t0;
                        resolve({ops, errors, spanMs: Math.round(span),
                                 hz: ops.length / (span / 1000)});
                    }
                }, 85);
            })""", [ids[0], ids[1]] if s1_ok else ["", ""])
            p2.evaluate("window.__s2Peer.stop = true")
            peer_summary = p2.evaluate(
                "() => ({edits: window.__s2Peer.edits,"
                " marks: window.__s2Peer.marks.length})")
            s2_hz = (drv or {}).get("hz", 0)
            s2_errors = (drv or {}).get("errors", [])
            s2_ops = (drv or {}).get("ops", [])
            s2_span = (drv or {}).get("spanMs", 0)
            # 无丢失终验：双页收敛 + 计数块首尾逐位 + 计数标记总数。
            s2_final = wait_until(
                lambda: (doc_snap(p1) == doc_snap(p2)
                         and all(f" #{k};" in doc_snap(p1)
                                 for k in (0, 1, 718, 719))) or None, timeout=20)
            counts = None
            if s2_final:
                counts = p1.evaluate("""() => {
                    const s = JSON.stringify(window.__collabDoc());
                    return { marks: (s.match(/ #\\d+;/g) || []).length };
                }""")
            check("s2_high_freq_10hz_60s_no_loss",
                  drv is not None and s2_hz >= 10.0 and s2_span >= 60_000
                  and not s2_errors and len(s2_ops) >= 720
                  and s2_final is not None and counts is not None
                  and counts["marks"] >= 720
                  and peer_summary["marks"] >= 5,
                  json.dumps({"hz": round(s2_hz, 2),
                              "span_s": round(s2_span / 1000, 1),
                              "ops": len(s2_ops), "errors": s2_errors[:3],
                              "marks": counts and counts["marks"],
                              "peer": peer_summary,
                              "converged": s2_final is not None}))
            mem_after_s2 = mem_sample(p1)

            # ── S3. 断连/重连 ×10 ───────────────────────────────
            cycles_ok = 0
            cycle_detail = []
            for c in range(10 if s1_ok else 0):
                p2.evaluate("window.__collabDisconnect()")
                # 冻结实证：断连后 p1 编辑不达 p2。
                r_edit = collab_exec(p1, "human", {
                    "t": "replace_text", "block_id": ids[0],
                    "start": 0, "end": 0, "insert": f"[CY{c}]"})
                frozen = doc_snap(p2).find(f"[CY{c}]") < 0
                p2.evaluate("window.__collabReconnect()")
                conv = wait_until(
                    lambda c=c: (doc_snap(p1) == doc_snap(p2)
                                 and f"[CY{c}]" in doc_snap(p2)) or None,
                    timeout=15)
                ok_cycle = (r_edit.get("ok") is True and frozen and conv is not None)
                cycles_ok += 1 if ok_cycle else 0
                cycle_detail.append({"c": c, "edit": r_edit.get("ok"),
                                     "frozen": frozen, "conv": conv is not None})
            check("s3_disconnect_reconnect_10_cycles",
                  cycles_ok == 10,
                  json.dumps({"ok": cycles_ok, "cycles": cycle_detail[:3]}))
            mem_after_s3 = mem_sample(p1)

            # ── S4. 内存增长 < 50MB（曲线收割）──────────────────
            curve = p1.evaluate(
                "() => { clearInterval(window.__stressMemTimer);"
                " return window.__stressMem; }")
            js_growth_mb = ((mem_after_s3["js"] - mem_baseline["js"]) / 1048576)
            engine_growth_mb = ((mem_after_s3["engine"] - mem_baseline["engine"]) / 1048576)
            curve_js_max_delta = 0.0
            if curve and curve[0]["js"] > 0:
                js_vals = [c["js"] for c in curve if c["js"] > 0]
                curve_js_max_delta = (max(js_vals) - js_vals[0]) / 1048576
            check("s4_memory_growth_under_50mb",
                  js_growth_mb < 50 and engine_growth_mb < 50,
                  json.dumps({
                      "baseline": mem_baseline, "after_s1": mem_after_s1,
                      "after_s2": mem_after_s2, "after_s3": mem_after_s3,
                      "js_growth_mb": round(js_growth_mb, 1),
                      "engine_growth_mb": round(engine_growth_mb, 1),
                      "curve_js_max_delta_mb": round(curve_js_max_delta, 1),
                      "curve_samples": len(curve)}))

            # 收尾：全程零 pageerror（5 页）。
            check("zero_pageerror_all_five_pages",
                  all(not v for v in page_errors.values()),
                  json.dumps({k: v[:2] for k, v in page_errors.items() if v}))

            for p in pages:
                p.close()
            context.close()
            browser.close()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps(
        {"meta": {"browser": "chromium-headless",
                  "args": ["--enable-precise-memory-info"],
                  "runner": "playwright", "page": "web/dist/index.html",
                  "suite": "stress_web (M-3, #29 task 2)"},
         "tests": checks,
         "memory_curve": curve,
         "summary": {"total": len(checks), "failed": len(failed),
                     "pass": len(failed) == 0}},
        ensure_ascii=False, indent=1))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
