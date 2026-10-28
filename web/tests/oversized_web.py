#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Stage 6 §九.7 — oversized/abnormal input, timeout & resource-exhaustion
boundary suite (workbench web face).

Web-face twin of the stage-6 plan theme 超大或异常输入、超时和资源耗尽:
every oversized/abnormal input must produce a TYPED rejection or a BOUNDED
behavior — never a crash, never a hang, never unbounded memory. Driven over
a REAL browser page against the built dist/ (no product code touched):

  A. 超大输入 (oversized):
     1. 10MB progressive big-doc (?doc=big&kb=10240&chunks=16) loads,
        __ready within 60s, >1000 blocks, page still responsive (live edit
        + core-driven undo round-trip); loadMs / heapTrace recorded.
     2. wasm linear memory sanity (< 1.5GB) + __memStats() parses —
        recorded as baseline evidence, not a perf gate.
     3. grid slice beyond MAX_SLICE_CELLS (100k) → typed grid_slice_too_large.
     4. grid paste > MAX_PASTE_CELLS (10k) → clamped at exactly 10k with a
        truncation notice; ledger hammered to the MAX_EDIT_LOG (100k) cap
        and saturates (final paste applies 0).
  B. 异常输入 (abnormal) — every hostile call wrapped in an in-page
     Promise.race bound (20s default): the call must RETURN, i.e. the
     no-hang property is asserted, not assumed:
     5. 10KB of control-char garbage → core.sync_from_markdown (bounded
        ingest or typed reject — actual recorded, page alive either way).
     6. replace_text_at_anchor(start=10^9) → typed Anchor/OutOfRange reject,
        revision/blockCount/md unchanged.
     7. apply_ops('[{"t":"bogus"}]') → JSON-boundary rejection; plus a
        well-formed but out-of-range op → typed Reject JSON; tree unpolluted.
     8. extend_from_markdown with an unclosed fence → typed IncompleteChunk,
        zero tree pollution.
     9. __gridOpenBytes with 100KB random garbage → typed grid_read_failed.
     10. evidence proposals: nonexistent paragraph / nonexistent source
         doc_id / nonexistent + malformed cell ref → typed wb_* rejections.
     11. __evRestoreSidecar('garbage') + truncated JSON → typed wb_sidecar.
     12. __importDocx with the first 40% of import_sample.docx → typed
         container_corrupt, editor untouched.
     13. __importXlsx with 100KB random → typed container_corrupt.
     14. __pdfOpenBytes with 50KB random → typed pdf_* reject; the panel
         stays display-capable (valid fixture reopens to 3 pages).
  C. 超时/资源面 (timeouts / resource exhaustion):
     15. hostile_calls_all_return — aggregate: every hostile call made
         returned within its bound (N made == N returned).
     16. 200 rapid collab insert_block commands → all receipts ok, undo
         depth bounded by UNDO_LIMIT (200), session still undoes.
     17. 300 canvas add_node commands → all ok, objects == 300, undo depth
         bounded (soft-cap clear_all semantics recorded), session undoes.
     18. zero_pageerror throughout (both page loads).

RAM discipline: ONE browser, ONE page reused across two goto()s (the 10MB
load first on a fresh page, then a plain reload for the rest). Garbage bytes
are generated from a FIXED seed (42) for reproducibility. Measured numbers
(loadMs, heap MB, wasm bytes, elapsed ms) are recorded in check details and
meta.measurements as baseline evidence — not gates.
Results -> tests/results/results_oversized_web.json
"""

import base64
import http.server
import json
import random
import socketserver
import sys
import threading
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURES = WEB / "tests" / "fixtures"
OUT = WEB / "tests" / "results" / "results_oversized_web.json"
PORT = 8945
BASE = f"http://127.0.0.1:{PORT}"
BIG_DOC_URL = f"{BASE}/index.html?doc=big&kb=10240&chunks=16"
PLAIN_URL = f"{BASE}/index.html"

# wasm 线性内存合理性界（非 perf 门——1.5GB sanity bound per mission）
WASM_MEMORY_SANITY = int(1.5 * 1024 * 1024 * 1024)
# 敌意调用默认 in-page 上限（no-hang 断言的硬界）
HOSTILE_DEFAULT_MS = 20_000
BATCH_BOUND_MS = 60_000

# ---------------------------------------------------------------------------
# deterministic garbage (fixed seed — reproducible)
# ---------------------------------------------------------------------------
RNG = random.Random(42)
GARBAGE_100K_B64 = base64.b64encode(RNG.randbytes(100 * 1024)).decode()
GARBAGE_50K_B64 = base64.b64encode(RNG.randbytes(50 * 1024)).decode()
# 10KB of control characters (0x00-0x1F — nulls, newlines, everything hostile)
CTRL_GARBAGE_10K = "".join(chr(RNG.randrange(0, 0x20)) for _ in range(10 * 1024))
# 101x101 = 10_201 cells TSV — one cell over the MAX_PASTE_CELLS overshoot
PASTE_TSV = "\n".join("\t".join(f"c{i}-{j}" for j in range(101)) for i in range(101))

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True

# ---------------------------------------------------------------------------
# in-page hostile-call dispatcher — Promise.race hard bound = no-hang proof.
# Every hostile input goes through this one face: the call either returns
# {returned:true, result|errRaw/errJson} or the race resolves
# {returned:false, timeoutMs} — the no-hang property is ASSERTED per call.
# ---------------------------------------------------------------------------
JS_HOSTILE = """(spec) => Promise.race([
  (async () => {
    const out = { kind: spec.kind, returned: true, threw: false };
    const paste = (tsv) => {
      const dt = new DataTransfer();
      dt.setData('text/plain', tsv);
      let ev;
      try {
        ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true });
        if (!ev.clipboardData) throw new Error('ctor ignored clipboardData');
      } catch (e) {
        ev = new Event('paste', { bubbles: true, cancelable: true });
        Object.defineProperty(ev, 'clipboardData', { value: dt });
      }
      document.dispatchEvent(ev);
    };
    try {
      let r;
      switch (spec.kind) {
        case 'core_sync_garbage':
          r = window.__core.sync_from_markdown(spec.md); break;
        case 'core_extend_fence':
          r = window.__core.extend_from_markdown(spec.md); break;
        case 'core_bad_range':
          r = window.__core.replace_text_at_anchor(
            BigInt(spec.blockId), spec.start, spec.end, 'x', null); break;
        case 'core_ops_json':
          r = window.__core.apply_ops(spec.ops, 'ai', 'hostile-probe'); break;
        case 'grid_open_garbage':
          r = window.__gridOpenBytes(spec.b64); break;
        case 'grid_slice_big':
          r = window.__gridSliceRaw(spec.sheet, spec.r0, spec.c0, spec.rows, spec.cols); break;
        case 'grid_paste':
          paste(spec.tsv);
          r = { edits: window.__gridInfo().edits,
                status: document.getElementById('grid-status').textContent }; break;
        case 'grid_paste_hammer': {
          const counts = [];
          for (let i = 0; i < spec.n; i++) {
            paste(spec.tsv);
            counts.push(window.__gridInfo().edits);
          }
          r = { counts, finalEdits: window.__gridInfo().edits,
                finalStatus: document.getElementById('grid-status').textContent }; break;
        }
        case 'ev_propose':
          r = window.__evProposeBinding(spec.json); break;
        case 'ev_sidecar':
          r = window.__evRestoreSidecar(spec.json); break;
        case 'docx_import':
          r = window.__importDocx(Uint8Array.from(atob(spec.b64), (c) => c.charCodeAt(0))); break;
        case 'xlsx_import':
          r = window.__importXlsx(Uint8Array.from(atob(spec.b64), (c) => c.charCodeAt(0))); break;
        case 'pdf_open':
          r = await window.__pdfOpenBytes(spec.b64); break;
        default:
          throw new Error('unknown hostile kind: ' + spec.kind);
      }
      out.result = r;
    } catch (e) {
      out.threw = true;
      out.errRaw = String(e);
      try { out.errJson = JSON.parse(String(e)); } catch (_) { out.errJson = null; }
    }
    return out;
  })(),
  new Promise((res) => setTimeout(
    () => res({ kind: spec.kind, returned: false, timeoutMs: spec.timeoutMs || HOSTILE_DEFAULT_MS }),
    spec.timeoutMs || HOSTILE_DEFAULT_MS)),
])"""

# 200 rapid collab insert_block commands (rapid-fire resource face)
JS_RAPID_COLLAB = """() => Promise.race([
  (async () => {
    const t0 = performance.now();
    let ok = 0, fail = 0; const failSample = [];
    for (let i = 0; i < 200; i++) {
      const r = window.__collabExec('human', JSON.stringify({
        t: 'insert_block', parent: null, index: 0, id: null,
        spec: { kind: 'p', attrs: {}, text: 'rapid-fire ' + i },
      }));
      if (r && r.ok === true) ok++;
      else { fail++; if (failSample.length < 3) failSample.push(r); }
    }
    const depth = window.__collabDepth('human');
    const docBlocks = window.__collabState().docBlocks;
    const undoOk = window.__collabUndo('human');
    const depthAfter = window.__collabDepth('human');
    const docBlocksAfter = window.__collabState().docBlocks;
    return { returned: true, ok, fail, failSample,
             elapsedMs: Math.round(performance.now() - t0),
             depth, undoOk, depthAfter, docBlocks, docBlocksAfter };
  })(),
  new Promise((res) => setTimeout(() => res({ returned: false }), BATCH_BOUND_MS)),
])"""

# 300 canvas add_node commands (big batch resource face)
JS_CANVAS_BATCH = """() => Promise.race([
  (async () => {
    const t0 = performance.now();
    let ok = 0, fail = 0, cleared = 0; const failSample = [];
    for (let i = 0; i < 300; i++) {
      const r = window.__canvasExecute('human', JSON.stringify({
        t: 'add_node', id: null,
        x: 20 + (i % 15) * 140, y: 20 + Math.floor(i / 15) * 60,
        w: 120, h: 36, props: { label: 'bulk-' + i },
      }));
      if (r && r.ok === true) {
        ok++;
        if (r.receipt && r.receipt.undo_cleared) cleared++;
      } else { fail++; if (failSample.length < 3) failSample.push(r); }
    }
    const st = window.__canvasState();
    const undoOk = window.__canvasUndo();
    const stAfter = window.__canvasState();
    return { returned: true, ok, fail, failSample, clearedCount: cleared,
             elapsedMs: Math.round(performance.now() - t0),
             objects: st.objects, undoDepth: st.undoDepth, redoDepth: st.redoDepth,
             undoOk, objectsAfterUndo: stAfter.objects, undoDepthAfter: stAfter.undoDepth };
  })(),
  new Promise((res) => setTimeout(() => res({ returned: false }), BATCH_BOUND_MS)),
])"""

# 10MB 终态活性探针：真实 CM6 编辑（快路径入核）+ 核驱动 undo 全量回写
JS_BIG_LIVENESS = """() => {
  const v = window.__view;
  const end = v.state.doc.length;
  const md0 = window.__md();
  const t0 = performance.now();
  v.dispatch({ changes: { from: end, insert: 'z' }, selection: { anchor: end + 1 } });
  const md1 = window.__md();
  const t1 = performance.now();
  const undoOk = window.__bridge.undo();
  const md2 = window.__md();
  return { editMs: Math.round(performance.now() - t0),
           undoMs: Math.round(performance.now() - t1),
           undoOk,
           landed: md1.length === md0.length + 1 && md1.endsWith('z') && !md0.endsWith('z'),
           reverted: md2 === md0,
           mdLens: [md0.length, md1.length, md2.length] };
}"""


def main() -> int:
    fixtures = {
        "grid": FIXTURES / "grid_sample.xlsx",
        "ev_body": FIXTURES / "evidence_body.docx",
        "ev_src": FIXTURES / "evidence_sample.xlsx",
        "docx": FIXTURES / "import_sample.docx",
        "pdf": FIXTURES / "sample.pdf",
    }
    missing = [str(p) for p in fixtures.values() if not p.exists()]
    if missing:
        print(f"FATAL fixture(s) missing: {missing}")
        return 2

    grid_b64 = base64.b64encode(fixtures["grid"].read_bytes()).decode()
    ev_body_b64 = base64.b64encode(fixtures["ev_body"].read_bytes()).decode()
    ev_src_b64 = base64.b64encode(fixtures["ev_src"].read_bytes()).decode()
    docx_bytes = fixtures["docx"].read_bytes()
    docx_trunc_b64 = base64.b64encode(docx_bytes[: int(len(docx_bytes) * 0.4)]).decode()
    pdf_b64 = base64.b64encode(fixtures["pdf"].read_bytes()).decode()

    checks: list[dict] = []
    page_errors: list[str] = []
    hostile_log: list[dict] = []
    skipped = 0
    measurements: dict = {}

    def check(name, ok, detail="", skip=False):
        nonlocal skipped
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip), "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        if skip:
            skipped += 1
        print(tag + name + ("  | " + str(detail)[:160] if detail and not ok and not skip else ""))
        return ok

    def hostile(page, name, spec):
        """One hostile/oversized call through the in-page bounded dispatcher.
        Records (name, returned, wall ms) — the aggregate no-hang evidence."""
        t0 = time.time()
        try:
            r = page.evaluate(JS_HOSTILE, spec)
            returned = bool(r) and r.get("returned") is True
        except Exception as e:  # page-level failure (crash / navigation) — counts as no-return
            r = {"kind": spec.get("kind"), "returned": False, "pyError": str(e)[:200]}
            returned = False
        hostile_log.append({"name": name, "returned": returned,
                            "ms": round((time.time() - t0) * 1000, 1)})
        return r

    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            # 与 web_smoke g2 同口径：expose-gc + precise-memory-info（堆测量）
            browser = pw.chromium.launch(
                args=["--js-flags=--expose-gc", "--enable-precise-memory-info"])
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))

            # ================================================================
            # A1/A2. 10MB 渐进大文档（最重用例先行——干净页）：
            #        有界加载 + 终态内存 + 活性（编辑/撤销往返）
            # ================================================================
            t0 = time.time()
            page.goto(BIG_DOC_URL)
            page.wait_for_function("window.__ready === true", timeout=60000)
            load_wall_ms = round((time.time() - t0) * 1000, 1)

            big = page.evaluate("""() => {
                const p = window.__perf.progressive || {};
                return { ttiMs: p.ttiMs ?? null, ingestMs: p.ingestMs ?? null,
                         finalRenderMs: p.finalRenderMs ?? null, chunks: p.chunks ?? null,
                         loadMs: window.__perf.loadMs,
                         blocks: window.__blockCount(),
                         docLen: window.__view.state.doc.length,
                         heapTraceLast: (window.__heapTrace || [])
                             [window.__heapTrace ? window.__heapTrace.length - 1 : 0] ?? null };
            }""")
            resp = page.evaluate(JS_BIG_LIVENESS)
            measurements["big_doc"] = {"load_wall_ms": load_wall_ms, **big, "liveness": resp}
            check("big_doc_10mb_progressive_load_alive",
                  big["blocks"] > 1000 and big["docLen"] > 9_000_000
                  and big["chunks"] == 16 and load_wall_ms < 60_000
                  and resp["landed"] and resp["reverted"] and resp["undoOk"]
                  and resp["editMs"] < 10_000 and resp["undoMs"] < 30_000,
                  json.dumps({"load_wall_ms": load_wall_ms,
                              "ttiMs": big["ttiMs"], "ingestMs": big["ingestMs"],
                              "blocks": big["blocks"], "docLen": big["docLen"],
                              "heapTraceLast": big["heapTraceLast"], "liveness": resp}))

            mem = page.evaluate("""async () => {
                const wasmBytes = window.__wasmMemory
                    ? window.__wasmMemory.buffer.byteLength : null;
                const stats = window.__memStats ? window.__memStats() : null;
                if (window.gc) { window.gc(); window.gc();
                    await new Promise((r) => setTimeout(r, 150)); }
                const m = performance.memory ? performance.memory.usedJSHeapSize : null;
                return { wasmBytes, stats,
                         heapTraceLen: (window.__heapTrace || []).length,
                         steadyUsedMb: m ? Math.round(m / 1048576) : null };
            }""")
            stats = mem.get("stats") or {}
            measurements["big_doc_memory"] = {k: mem.get(k) for k in ("wasmBytes", "steadyUsedMb")}
            check("big_doc_memory_bounded",
                  mem.get("wasmBytes") is not None
                  and mem["wasmBytes"] < WASM_MEMORY_SANITY
                  and isinstance(stats, dict) and isinstance(stats.get("revision"), int)
                  and mem.get("heapTraceLen", 0) > 0,
                  json.dumps({"wasmBytes": mem.get("wasmBytes"),
                              "wasmMb": round((mem.get("wasmBytes") or 0) / 1048576, 1),
                              "steadyUsedMb": mem.get("steadyUsedMb"),
                              "statsKeys": sorted(stats.keys())[:8]}))

            # ================================================================
            # 干净重载（10MB 页释放——RAM 纪律：同页复用，双 goto）
            # ================================================================
            page.goto(PLAIN_URL)
            page.wait_for_function(
                "[window.__ready, window.__gridReady, window.__evReady, window.__pdfReady,"
                " window.__canvasReady, window.__collabReady].every(x => x === true)",
                timeout=60000)

            # ---------------------------------------------------------------
            # A3. grid 切片超限（> MAX_SLICE_CELLS = 100k）→ typed 拒
            # ---------------------------------------------------------------
            page.evaluate("document.getElementById('tab-grid').click()")
            opened = page.evaluate("b => window.__gridOpenBytes(b)", grid_b64)
            slice_a = hostile(page, "grid_slice_1001x100",
                              {"kind": "grid_slice_big", "sheet": 0, "r0": 0, "c0": 0,
                               "rows": 1001, "cols": 100})
            slice_b = hostile(page, "grid_slice_2000x100",
                              {"kind": "grid_slice_big", "sheet": 0, "r0": 0, "c0": 0,
                               "rows": 2000, "cols": 100})
            grid_after = page.evaluate("""() => ({
                info: window.__gridInfo(), sheets: window.__gridSheetCount(),
                ready: window.__ready === true })""")
            # __gridSliceRaw 永不抛（probe 内 catch → wasmFail typed 面）
            ok_slices = all([
                opened.get("ok") is True,
                slice_a["returned"] and not slice_a.get("threw")
                and (slice_a.get("result") or {}).get("ok") is False
                and (slice_a.get("result") or {}).get("code") == "grid_slice_too_large",
                slice_b["returned"] and not slice_b.get("threw")
                and (slice_b.get("result") or {}).get("ok") is False
                and (slice_b.get("result") or {}).get("code") == "grid_slice_too_large",
                grid_after["info"]["rows"] == 300 and grid_after["info"]["cols"] == 40,
                grid_after["sheets"] == 2 and grid_after["ready"],
            ])
            check("grid_slice_oversized_rejects", ok_slices,
                  json.dumps({"opened": opened.get("ok"),
                              "sliceA": slice_a.get("result"),
                              "sliceB": slice_b.get("result"),
                              "still": grid_after}))

            # ---------------------------------------------------------------
            # B9. grid 垃圾字节（100KB 随机）→ typed grid_read_failed
            # ---------------------------------------------------------------
            g_garbage = hostile(page, "grid_open_garbage_100k",
                                {"kind": "grid_open_garbage", "b64": GARBAGE_100K_B64})
            g_still = page.evaluate("""() => ({
                sheets: window.__gridSheetCount(), info: window.__gridInfo(),
                status: document.getElementById('grid-status').textContent,
                errClass: document.getElementById('grid-status')
                    .classList.contains('grid-error') })""")
            g_res = g_garbage.get("result") or {}
            check("grid_garbage_bytes",
                  g_garbage["returned"] and not g_garbage.get("threw")
                  and g_res.get("ok") is False and g_res.get("code") == "grid_read_failed"
                  and bool(g_res.get("message"))
                  and g_still["sheets"] == 2 and g_still["info"]["rows"] == 300
                  and "grid_read_failed" in g_still["status"] and g_still["errClass"],
                  json.dumps({"r": g_res, "still": g_still}))

            # ---------------------------------------------------------------
            # A4. 超大粘贴 → MAX_PASTE_CELLS 钳制 + MAX_EDIT_LOG 饱和
            # ---------------------------------------------------------------
            page.evaluate("""() => {
                document.getElementById('grid-viewport').focus();
                window.__gridSetSelection(0, 0);
            }""")
            paste1 = hostile(page, "grid_paste_10201_cells",
                             {"kind": "grid_paste", "tsv": PASTE_TSV})
            p1 = paste1.get("result") or {}
            hammer = hostile(page, "grid_paste_ledger_saturation",
                             {"kind": "grid_paste_hammer", "tsv": PASTE_TSV, "n": 9})
            hm = hammer.get("result") or {}
            paste_final = hostile(page, "grid_paste_at_ledger_cap",
                                  {"kind": "grid_paste", "tsv": PASTE_TSV})
            pf = paste_final.get("result") or {}
            bounded_dom = page.evaluate("window.__gridVisible()")
            counts = hm.get("counts") or []
            monotonic = all(b - a == 10_000 for a, b in zip(counts, counts[1:]))
            check("oversized_paste_clamped",
                  paste1["returned"]
                  and p1.get("edits") == 10_000
                  and "truncated at 10000-cell limit" in str(p1.get("status"))
                  and hammer["returned"] and counts
                  and counts[0] == 20_000 and counts[-1] == 100_000 and monotonic
                  and hm.get("finalEdits") == 100_000          # MAX_EDIT_LOG 硬界
                  and paste_final["returned"]
                  and pf.get("edits") == 100_000               # 饱和后零增长
                  and 0 < bounded_dom["mainDom"] < 500,        # 虚拟窗 DOM 仍有界
                  json.dumps({"paste1Edits": p1.get("edits"),
                              "paste1Status": str(p1.get("status"))[:80],
                              "hammerCounts": counts,
                              "finalEdits": pf.get("edits"),
                              "finalStatus": str(pf.get("status"))[:80],
                              "visible": bounded_dom}))

            page.evaluate("document.getElementById('tab-doc').click()")

            # ---------------------------------------------------------------
            # B5. 10KB 控制字符垃圾 → sync_from_markdown（实测分支记录）
            # ---------------------------------------------------------------
            pre = page.evaluate("""() => ({ blocks: window.__blockCount(),
                                            rev: Number(window.__core.revision()) })""")
            sync = hostile(page, "core_sync_garbage_10k",
                           {"kind": "core_sync_garbage", "md": CTRL_GARBAGE_10K})
            post = page.evaluate("""() => ({ blocks: window.__blockCount(),
                                             rev: Number(window.__core.revision()),
                                             ready: window.__ready === true,
                                             mdLen: window.__md().length })""")
            sync_ok = (not sync.get("threw")) and isinstance(sync.get("result"), str)
            sync_reject = sync.get("threw") and bool(sync.get("errRaw"))
            measurements["doc_garbage_sync"] = {
                "outcome": "ingest" if sync_ok else ("typed_reject" if sync_reject else "other"),
                "blocks": [pre["blocks"], post["blocks"]],
                "revision": [pre["rev"], post["rev"]]}
            check("doc_garbage_markdown_sync",
                  sync["returned"] and (sync_ok or sync_reject)
                  and post["blocks"] >= 1 and post["ready"]
                  and (post["rev"] >= pre["rev"]),
                  json.dumps({"threw": sync.get("threw"),
                              "result": str(sync.get("result"))[:60],
                              "errJson": sync.get("errJson"),
                              "errRaw": str(sync.get("errRaw"))[:120],
                              "pre": pre, "post": post}))

            # ---------------------------------------------------------------
            # B6. 越界区间替换（start=10^9）→ typed Anchor/OutOfRange
            # ---------------------------------------------------------------
            blk = page.evaluate("window.__blocks()[0].id")
            before6 = page.evaluate("""() => ({ blocks: window.__blockCount(),
                                                rev: Number(window.__core.revision()),
                                                md: window.__md() })""")
            bad_range = hostile(page, "core_replace_start_1e9",
                                {"kind": "core_bad_range", "blockId": blk,
                                 "start": 1_000_000_000, "end": 1_000_000_004})
            after6 = page.evaluate("""() => ({ blocks: window.__blockCount(),
                                               rev: Number(window.__core.revision()),
                                               md: window.__md(),
                                               ready: window.__ready === true })""")
            err6 = bad_range.get("errJson")
            check("doc_bad_range_replace",
                  bad_range["returned"] and bad_range.get("threw")
                  and isinstance(err6, dict) and "Anchor" in err6
                  and "OutOfRange" in json.dumps(err6)
                  and after6["blocks"] == before6["blocks"]
                  and after6["rev"] == before6["rev"]
                  and after6["md"] == before6["md"] and after6["ready"],
                  json.dumps({"errJson": err6, "errRaw": str(bad_range.get("errRaw"))[:120],
                              "before": {k: before6[k] for k in ("blocks", "rev")},
                              "after": {k: after6[k] for k in ("blocks", "rev", "ready")}}))

            # ---------------------------------------------------------------
            # B7. apply_ops 敌意 JSON（未知变体 + 越界 op）→ typed 拒、树零污染
            # ---------------------------------------------------------------
            before7 = page.evaluate("window.__blockCount()")
            ops_a = hostile(page, "core_ops_unknown_variant",
                            {"kind": "core_ops_json", "ops": '[{"t":"bogus"}]'})
            mid7 = page.evaluate("window.__blockCount()")
            ops_b = hostile(page, "core_ops_out_of_range",
                            {"kind": "core_ops_json",
                             "ops": '[{"t":"replace_text","path":[0],"start":99999999,'
                                    '"end":999999999,"insert":"x"}]'})
            after7 = page.evaluate("""() => ({ blocks: window.__blockCount(),
                                               rev: Number(window.__core.revision()),
                                               ready: window.__ready === true })""")
            check("doc_invalid_ops_json",
                  ops_a["returned"] and ops_a.get("threw")
                  and str(ops_a.get("errRaw", "")).startswith("bad ops json")
                  and mid7 == before7
                  and ops_b["returned"] and ops_b.get("threw")
                  and isinstance(ops_b.get("errJson"), dict)
                  and after7["blocks"] == before7 and after7["ready"]
                  and after7["rev"] == before6["rev"],
                  json.dumps({"a": {"errRaw": str(ops_a.get("errRaw"))[:100]},
                              "b": {"errJson": ops_b.get("errJson"),
                                    "errRaw": str(ops_b.get("errRaw"))[:100]},
                              "blocks": [before7, mid7, after7["blocks"]]}))

            # ---------------------------------------------------------------
            # B8. 悬挂 chunk（未闭合 fence）→ typed IncompleteChunk、零污染
            # ---------------------------------------------------------------
            before8 = page.evaluate("window.__blockCount()")
            fence = hostile(page, "core_extend_unclosed_fence",
                            {"kind": "core_extend_fence",
                             "md": "```text\nunclosed fence — chunk hangs on EOF"})
            after8 = page.evaluate("""() => ({ blocks: window.__blockCount(),
                                               ready: window.__ready === true })""")
            f_json = fence.get("errJson")
            check("doc_incomplete_chunk_rejects",
                  fence["returned"] and fence.get("threw")
                  and isinstance(f_json, dict) and "IncompleteChunk" in f_json
                  and after8["blocks"] == before8 and after8["ready"],
                  json.dumps({"errJson": f_json,
                              "errRaw": str(fence.get("errRaw"))[:120],
                              "blocks": [before8, after8["blocks"]]}))

            # ---------------------------------------------------------------
            # B10. 证据面敌意提议（坏段落 / 坏来源 / 坏单元格引用）→ wb_*
            # ---------------------------------------------------------------
            has_ev = page.evaluate("window.__evState().wasm") is True

            def proposal(**over):
                p = {"evidence_id": "ov-1", "claim": "单价 12.5 元",
                     "claimed_value": "12.5", "paragraph_index": 2,
                     "source_doc_id": "src-1", "sheet": "Sheet1", "coord": "B7",
                     "relation": "direct_quote", "calc_ref": None,
                     "generation_batch": "ov1"}
                p.update(over)
                return json.dumps(p, ensure_ascii=False)

            if not has_ev:
                check("evidence_bad_proposals", True,
                      "pending wasm (WasmWorkbench absent from pkg)", skip=True)
            else:
                started = page.evaluate("b => window.__evStartBody(b)", ev_body_b64)
                ingested = page.evaluate("b => window.__evIngestSource(b)", ev_src_b64)
                v_para = hostile(page, "ev_propose_bad_paragraph",
                                 {"kind": "ev_propose",
                                  "json": proposal(paragraph_index=9999)})
                v_src = hostile(page, "ev_propose_bad_source",
                                {"kind": "ev_propose",
                                 "json": proposal(source_doc_id="no-such-src")})
                v_coord = hostile(page, "ev_propose_nonexistent_cell",
                                  {"kind": "ev_propose", "json": proposal(coord="ZZZZ9999")})
                v_malformed = hostile(page, "ev_propose_malformed_cell",
                                      {"kind": "ev_propose",
                                       "json": proposal(coord="not-a-cell")})
                ev_after = page.evaluate("window.__evState()")

                def code_of(h):
                    return ((h.get("result") or {}).get("code")
                            if not h.get("threw") else None)

                check("evidence_bad_proposals",
                      started.get("ok") is True and ingested.get("ok") is True
                      and ingested.get("doc_id") == "src-1"
                      and v_para["returned"] and code_of(v_para) == "wb_paragraph_not_found"
                      and v_src["returned"] and code_of(v_src) == "wb_source_not_registered"
                      and v_coord["returned"] and code_of(v_coord) == "wb_cell_not_found"
                      and v_malformed["returned"]
                      and code_of(v_malformed) == "wb_cell_not_found"
                      and all(bool(((h.get("result") or {}).get("message")))
                              for h in (v_para, v_src, v_coord, v_malformed))
                      and ev_after["active"] is True and ev_after["evidence"] == 0,
                      json.dumps({"para": v_para.get("result"),
                                  "src": v_src.get("result"),
                                  "coord": v_coord.get("result"),
                                  "malformed": v_malformed.get("result"),
                                  "after": {"active": ev_after["active"],
                                            "evidence": ev_after["evidence"]}}))

            # ---------------------------------------------------------------
            # B11. sidecar 垃圾/截断 → typed wb_sidecar（fail-closed）
            # ---------------------------------------------------------------
            if not has_ev:
                check("sidecar_corrupt_garbage", True,
                      "pending wasm (WasmWorkbench absent from pkg)", skip=True)
            else:
                sc_g = hostile(page, "ev_sidecar_garbage",
                               {"kind": "ev_sidecar", "json": "garbage"})
                sc_t = hostile(page, "ev_sidecar_truncated_json",
                               {"kind": "ev_sidecar",
                                "json": '{"schema_version":1,"evidence":['})
                sc_state = page.evaluate("""() => ({ active: window.__evState().active,
                                                     evidence: window.__evState().evidence })""")
                check("sidecar_corrupt_garbage",
                      sc_g["returned"] and not sc_g.get("threw")
                      and (sc_g.get("result") or {}).get("ok") is False
                      and (sc_g.get("result") or {}).get("code") == "wb_sidecar"
                      and bool((sc_g.get("result") or {}).get("message"))
                      and sc_t["returned"]
                      and (sc_t.get("result") or {}).get("code") == "wb_sidecar"
                      and sc_state["active"] is True and sc_state["evidence"] == 0,
                      json.dumps({"garbage": sc_g.get("result"),
                                  "truncated": sc_t.get("result"),
                                  "state": sc_state}))

            # ---------------------------------------------------------------
            # B12. DOCX 截断字节（前 40%）→ typed container_corrupt
            # ---------------------------------------------------------------
            md12 = page.evaluate("window.__md()")
            docx_tr = hostile(page, "docx_import_truncated_40pct",
                              {"kind": "docx_import", "b64": docx_trunc_b64})
            after12 = page.evaluate("""() => ({ md: window.__md(),
                                                ready: window.__ready === true,
                                                status: document.getElementById('docx-status')
                                                    .textContent })""")
            d_res = docx_tr.get("result") or {}
            check("docx_truncated_bytes",
                  docx_tr["returned"] and not docx_tr.get("threw")
                  and d_res.get("ok") is False and d_res.get("code") == "container_corrupt"
                  and bool(d_res.get("message"))
                  and after12["md"] == md12 and after12["ready"]
                  and "container_corrupt" in after12["status"],
                  json.dumps({"r": d_res, "status": str(after12["status"])[:100],
                              "mdUntouched": after12["md"] == md12}))

            # ---------------------------------------------------------------
            # B13. XLSX 垃圾字节（100KB 随机）→ typed container_corrupt
            # ---------------------------------------------------------------
            md13 = page.evaluate("window.__md()")
            xlsx_g = hostile(page, "xlsx_import_garbage_100k",
                             {"kind": "xlsx_import", "b64": GARBAGE_100K_B64})
            after13 = page.evaluate("""() => ({ md: window.__md(),
                                                ready: window.__ready === true,
                                                status: document.getElementById('xlsx-status')
                                                    .textContent })""")
            x_res = xlsx_g.get("result") or {}
            check("xlsx_garbage_bytes",
                  xlsx_g["returned"] and not xlsx_g.get("threw")
                  and x_res.get("ok") is False and x_res.get("code") == "container_corrupt"
                  and bool(x_res.get("message"))
                  and after13["md"] == md13 and after13["ready"]
                  and "container_corrupt" in after13["status"],
                  json.dumps({"r": x_res, "status": str(after13["status"])[:100],
                              "mdUntouched": after13["md"] == md13}))

            # ---------------------------------------------------------------
            # B14. PDF 垃圾字节（50KB 随机）→ typed pdf_*；面板仍可显示
            # ---------------------------------------------------------------
            pdf_g = hostile(page, "pdf_open_garbage_50k",
                            {"kind": "pdf_open", "b64": GARBAGE_50K_B64,
                             "timeoutMs": HOSTILE_DEFAULT_MS})
            pdf_state = page.evaluate("window.__pdfState()")
            pdf_err_ui = page.evaluate(
                "!!document.querySelector('#pdf-status .pdf-error')")
            reopen = page.evaluate("b => window.__pdfOpenBytes(b)", pdf_b64)
            page.wait_for_function("window.__pdfState().pages === 3", timeout=20000)
            reopened = page.evaluate("""() => ({ open: window.__pdfState().open,
                                                 pages: window.__pdfState().pages })""")
            p_res = pdf_g.get("result") or {}
            check("pdf_garbage_bytes",
                  pdf_g["returned"] and not pdf_g.get("threw")
                  and p_res.get("ok") is False
                  and str(p_res.get("code", "")).startswith("pdf_")
                  and bool(p_res.get("message"))
                  and pdf_state["open"] is False and pdf_err_ui
                  and reopen.get("ok") is True and reopened["open"] is True
                  and reopened["pages"] == 3,
                  json.dumps({"r": p_res, "stateAfterGarbage": pdf_state,
                              "errUi": pdf_err_ui, "reopen": reopen.get("ok"),
                              "reopened": reopened}))

            # ---------------------------------------------------------------
            # C16. 200 连发协同命令 → 全回执 + 撤销栈 ≤ UNDO_LIMIT(200)
            # ---------------------------------------------------------------
            has_collab = page.evaluate(
                "window.__collabDepth('human').undo >= 0") is True
            if not has_collab:
                check("rapid_fire_commands_bounded", True,
                      "pending wasm (WasmCollabSession absent from pkg)", skip=True)
            else:
                rapid = page.evaluate(JS_RAPID_COLLAB)
                hostile_log.append({"name": "rapid_fire_collab_200",
                                    "returned": bool(rapid.get("returned")),
                                    "ms": rapid.get("elapsedMs")})
                depth = (rapid.get("depth") or {}).get("undo")
                depth_after = (rapid.get("depthAfter") or {}).get("undo")
                measurements["rapid_fire_collab"] = {
                    k: rapid.get(k) for k in ("ok", "fail", "elapsedMs", "docBlocks")}
                check("rapid_fire_commands_bounded",
                      rapid.get("returned") is True
                      and rapid.get("ok") == 200 and rapid.get("fail") == 0
                      and isinstance(depth, int) and 1 <= depth <= 200
                      and rapid.get("docBlocks", 0) >= 200
                      and rapid.get("undoOk") is True
                      and depth_after == depth - 1
                      and rapid.get("docBlocksAfter") == rapid.get("docBlocks", -1) - 1,
                      json.dumps({"ok": rapid.get("ok"), "fail": rapid.get("fail"),
                                  "elapsedMs": rapid.get("elapsedMs"),
                                  "depth": rapid.get("depth"),
                                  "undoOk": rapid.get("undoOk"),
                                  "depthAfter": depth_after,
                                  "docBlocks": rapid.get("docBlocks"),
                                  "docBlocksAfter": rapid.get("docBlocksAfter")}))

            # ---------------------------------------------------------------
            # C17. 300 连发 canvas 命令 → 对象数精确 + 撤销栈有界
            # ---------------------------------------------------------------
            has_canvas = page.evaluate(
                "window.__canvasState().wasm") is True
            if not has_canvas:
                check("big_canvas_batch", True,
                      "pending wasm (WasmCanvas absent from pkg)", skip=True)
            else:
                batch = page.evaluate(JS_CANVAS_BATCH)
                hostile_log.append({"name": "canvas_batch_300",
                                    "returned": bool(batch.get("returned")),
                                    "ms": batch.get("elapsedMs")})
                measurements["canvas_batch"] = {
                    k: batch.get(k) for k in ("ok", "fail", "elapsedMs", "objects",
                                              "undoDepth", "clearedCount")}
                check("big_canvas_batch",
                      batch.get("returned") is True
                      and batch.get("ok") == 300 and batch.get("fail") == 0
                      and batch.get("objects") == 300
                      and isinstance(batch.get("undoDepth"), int)
                      and 0 <= batch["undoDepth"] <= 200
                      and batch.get("undoOk") is True
                      and batch.get("objectsAfterUndo") == 299,
                      json.dumps({"ok": batch.get("ok"), "fail": batch.get("fail"),
                                  "elapsedMs": batch.get("elapsedMs"),
                                  "objects": batch.get("objects"),
                                  "undoDepth": batch.get("undoDepth"),
                                  "clearedCount": batch.get("clearedCount"),
                                  "undoOk": batch.get("undoOk"),
                                  "objectsAfterUndo": batch.get("objectsAfterUndo"),
                                  "undoDepthAfter": batch.get("undoDepthAfter")}))

            # ---------------------------------------------------------------
            # C15. 聚合：N 次敌意调用 = N 次返回（no-hang 总账）
            # ---------------------------------------------------------------
            made = len(hostile_log)
            returned = sum(1 for e in hostile_log if e["returned"])
            slowest = max((e["ms"] for e in hostile_log), default=0)
            measurements["hostile_calls"] = hostile_log
            check("hostile_calls_all_return",
                  made >= 14 and made == returned and slowest < BATCH_BOUND_MS,
                  json.dumps({"made": made, "returned": returned,
                              "slowestMs": slowest,
                              "bigDocLoadWallMs": load_wall_ms,
                              "log": [(e["name"], e["ms"]) for e in hostile_log]}))

            # ---------------------------------------------------------------
            # C18. 契约面 + 零页错（两次加载全程无 JS 异常逃逸）
            # ---------------------------------------------------------------
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                contract: window.__contractVersion,
                md: typeof window.__md,
                gridReady: window.__gridReady === true,
                evReady: window.__evReady === true,
                pdfReady: window.__pdfReady === true,
                canvasReady: window.__canvasReady === true,
                collabReady: window.__collabReady === true,
            })""")
            check("zero_pageerror_throughout",
                  final["ready"] and final["contract"] == 1.4
                  and final["md"] == "function" and final["gridReady"]
                  and final["evReady"] and final["pdfReady"]
                  and final["canvasReady"] and final["collabReady"]
                  and len(page_errors) == 0,
                  json.dumps({"final": final, "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {
            "gate": "stage 6 §九.7 oversized/abnormal input + timeout/resource suite (web face)",
            "dist": str(WEB / "dist"), "port": PORT, "seed": 42,
            "big_doc_url": BIG_DOC_URL,
            "fixtures": {k: str(v) for k, v in fixtures.items()},
            "garbage": {"grid_xlsx_100k": 100 * 1024, "pdf_50k": 50 * 1024,
                        "ctrl_chars_10k": len(CTRL_GARBAGE_10K),
                        "paste_cells": 101 * 101, "docx_truncated_bytes": int(len(docx_bytes) * 0.4)},
            "bounds": {"wasm_memory_sanity": WASM_MEMORY_SANITY,
                       "hostile_call_ms": HOSTILE_DEFAULT_MS, "batch_ms": BATCH_BOUND_MS},
            "measurements": measurements,
        },
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "skipped": skipped,
                    "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed"
          + (f" ({skipped} skipped)" if skipped else "")
          + f" -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
