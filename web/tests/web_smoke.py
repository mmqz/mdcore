#!/usr/bin/env python3
"""R9 web smoke: serve web/dist, drive one CM6<->mdcore-wasm page through
W1-W6 in headless Chromium (Playwright), record results.json as evidence.

W1 syntax hiding        W2 cursor negotiation     W3 synthetic IME composition
W4 big-doc perf (1MB)   W5 block ops              W6 core-driven undo/redo
R12: W11 Human+AI 闭环（AI op 流经 JSON 边界注入 + 事件同流 + 整批 undo）
R15: W12 装饰增量等价护栏（selection-only 增量 == 强制全量，deco+atomic 双集）
R17: W13 非 BMP 单位宪法（码点↔UTF-16 换算 + 事件流 op 偏移=码点 + undo 恒等）
"""

import time
import http.server
import json
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def log_message(self, *a):
        pass


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


def open_page(browser, port, query=""):
    page = browser.new_page()
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(f"http://127.0.0.1:{port}/index.html{query}")
    page.wait_for_function("window.__ready === true", timeout=60_000)
    return page, errors


def check(name, cond, detail=""):
    return {"check": name, "pass": bool(cond), "detail": detail}


def w1_syntax_hiding(page):
    r = page.evaluate("""() => {
      const ed = document.querySelector('.cm-editor');
      const lineOf = (sel) => document.querySelector(sel)?.closest('.cm-line')?.textContent ?? null;
      const h1 = document.querySelector('.cm-line.cm-h1');
      const bold = document.querySelector('.cm-bold');
      const code = document.querySelector('.cm-inline-code');
      const hr = document.querySelector('.cm-hr');
      return {
        h1Text: h1?.textContent ?? null,
        h1HasMarker: (h1?.textContent ?? '').includes('#'),
        boldText: bold?.textContent ?? null,
        boldLine: lineOf('.cm-bold'),
        codeText: code?.textContent ?? null,
        codeLine: lineOf('.cm-inline-code'),
        hrWidget: !!hr,
        hrLineHasDashes: lineOf('.cm-hr')?.includes('---') ?? null,
        listMarker: !!document.querySelector('.cm-list-marker'),
        quoteMarker: !!document.querySelector('.cm-quote-marker'),
      };
    }""")
    checks = [
        check("h1 rendered + '# ' hidden", r["h1Text"] == "Bridge Demo" and not r["h1HasMarker"], str(r["h1Text"])),
        check("bold styled, '**' hidden on its line", r["boldText"] == "bold" and "**" not in (r["boldLine"] or ""), str(r["boldLine"])),
        check("inline code styled, backticks hidden", r["codeText"] == "inline code" and "`" not in (r["codeLine"] or ""), str(r["codeLine"])),
        check("hr widget replaces '---'", r["hrWidget"] and not r["hrLineHasDashes"], f"widget={r['hrWidget']}"),
        check("list/quote markers decorated", r["listMarker"] and r["quoteMarker"]),
    ]
    # 光标进入标记 -> 揭示；移开 -> 再隐藏（严格重叠显隐）
    reveal = page.evaluate("""() => {
      const doc = window.__view.state.doc;
      const idx = doc.toString().indexOf('**');
      window.__view.focus();
      window.__view.dispatch({selection: {anchor: idx + 1}});
      const line = document.querySelector('.cm-bold')?.closest('.cm-line')?.textContent ?? '';
      const revealedNow = line.includes('**') && !!document.querySelector('.cm-marker-revealed');
      window.__view.dispatch({selection: {anchor: 0}}); // 移开（行首相邻也不揭示）
      const line2 = document.querySelector('.cm-bold')?.closest('.cm-line')?.textContent ?? '';
      return {revealedNow, hiddenAgain: !line2.includes('**')};
    }""")
    checks.append(check("cursor inside marker reveals, moving away hides again", reveal["revealedNow"] and reveal["hiddenAgain"], str(reveal)))
    return checks


def w2_cursor_negotiation(page):
    checks = []
    # 标题 marker [0,2) 原子跳
    a = page.evaluate("() => { window.__view.focus(); window.__view.dispatch({selection:{anchor:2}}); return 2; }")
    page.keyboard.press("ArrowLeft")
    left = page.evaluate("() => window.__view.state.selection.main.anchor")
    page.keyboard.press("ArrowRight")
    right = page.evaluate("() => window.__view.state.selection.main.anchor")
    checks.append(check("heading marker atomic jump (2 ->ArrowLeft-> 0 ->ArrowRight-> 2)", a == 2 and left == 0 and right == 2, f"left={left} right={right}"))
    # bold 开标记原子跳
    b = page.evaluate("""() => {
      const idx = window.__view.state.doc.toString().indexOf('**');
      window.__view.dispatch({selection:{anchor: idx + 2}});
      return idx;
    }""")
    page.keyboard.press("ArrowLeft")
    left = page.evaluate("() => window.__view.state.selection.main.anchor")
    page.keyboard.press("ArrowRight")
    right = page.evaluate("() => window.__view.state.selection.main.anchor")
    checks.append(check("bold marker atomic jump", b >= 0 and left == b and right == b + 2, f"idx={b} left={left} right={right}"))
    # 快路径打字后选区存活；undo 全量重渲染后选区仍有效
    alive = page.evaluate("""() => {
      const v = window.__view;
      const end = v.state.doc.length;
      // 模拟真实打字：插入 + 选区随插入前进（裸 dispatch 的 CM6 默认映射
      // 停在插入点之前，与键盘行为不同）
      v.dispatch({changes: {from: end, insert: 'x'}, selection: {anchor: end + 1}});
      return v.state.selection.main.anchor === v.state.doc.length;
    }""")
    page.keyboard.press("Control+z")
    post = page.evaluate("""() => {
      const v = window.__view;
      const s = v.state.selection.main;
      const ok = s.anchor >= 0 && s.anchor <= v.state.doc.length && !window.__view.state.doc.toString().includes('xx');
      window.__view.dispatch({selection:{anchor: Math.min(5, v.state.doc.length)}});
      return ok && window.__view.state.selection.main.anchor === Math.min(5, v.state.doc.length);
    }""")
    checks.append(check("selection survives fast-path edit + core-undo re-render", alive and post, f"alive={alive} post={post}"))
    return checks


def w3_synthetic_ime(page):
    # 合成 composition 事件流（真 IME = R11，结果注明 synthetic only）
    page.evaluate("""() => {
      const v = window.__view;
      const pos = v.state.doc.toString().indexOf('Final paragraph');
      window.__view.focus();
      v.dispatch({selection: {anchor: pos}});
    }""")
    page.evaluate("""() => {
      const content = document.querySelector('.cm-content');
      content.dispatchEvent(new CompositionEvent('compositionstart', {data: ''}));
      document.execCommand('insertText', false, 'IME测试');
      content.dispatchEvent(new CompositionEvent('compositionupdate', {data: 'IME测试'}));
      content.dispatchEvent(new CompositionEvent('compositionend'));
    }""")
    r = page.evaluate("""() => {
      const doc = window.__view.state.doc.toString();
      const blocks = window.__blocks();
      const para = blocks.find((b) => b.type === 'paragraph' && b.text.includes('IME'));
      return {
        inDoc: doc.includes('IME测试') && doc.includes('Final paragraph'),
        inCore: !!para,
        docStillIntact: doc.includes('Bridge Demo') && doc.includes('first item'),
        revAdvanced: Number(window.__core.revision()) > 0,
      };
    }""")
    checks = [
        check("composition text landed in view doc, doc not corrupted", r["inDoc"] and r["docStillIntact"], str(r)),
        check("core absorbed composition via fallback sync", r["inCore"] and r["revAdvanced"], str(r)),
    ]
    return checks


def w4_big_doc(browser, port):
    page, errors = open_page(browser, port, "?doc=big&kb=1024")
    try:
        perf = page.evaluate("""() => ({
          perf: window.__perf,
          docLen: window.__view.state.doc.length,
          blocks: window.__blocks().length,
          fastPathUsed: (window.__mdcoreEvents.length > 0),
        })""")
        ms = sorted(perf["perf"]["typeMs"])
        p95 = ms[max(0, int(len(ms) * 0.95) - 1)] if ms else None
        checks = [
            check("1MB doc loaded (doc length > 900k)", perf["docLen"] > 900_000, f"len={perf['docLen']}"),
            check("1MB parsed into block tree (>1000 blocks)", perf["blocks"] > 1000, f"blocks={perf['blocks']}"),
            check("10 fast-path typings recorded", len(ms) == 10 and all(m >= 0 for m in ms), str([round(m, 1) for m in ms])),
            check("fallback sync + undo timings recorded", perf["perf"]["fallbackSyncMs"] is not None and perf["perf"]["undoMs"] is not None),
        ]
        return {
            "checks": checks,
            "load_ms": round(perf["perf"]["loadMs"], 1),
            "typing_ms": [round(m, 1) for m in ms],
            "typing_p95_ms": round(p95, 1) if p95 is not None else None,
            "fallback_sync_ms": round(perf["perf"]["fallbackSyncMs"], 1),
            "undo_rerender_ms": round(perf["perf"]["undoMs"], 1),
            "doc_len": perf["docLen"],
            "blocks": perf["blocks"],
        }
    finally:
        page.close()


def w7_progressive_open(browser, port, baseline_load_ms):
    """R10 渐进打开：首段 TTI < 全量加载（同机同文档相对断言），终态 = 全文档。"""
    page, errors = open_page(browser, port, "?doc=big&kb=1024&chunks=8")
    try:
        perf = page.evaluate("""() => ({
          p: window.__perf.progressive,
          docLen: window.__view.state.doc.length,
          blocks: window.__blocks().length,
        })""")
        p = perf["p"]
        # 终态可编辑：尾部快路径打字进核（ingest 窗口已解锁）
        typed = page.evaluate("""() => {
          const v = window.__view;
          const end = v.state.doc.length;
          v.dispatch({changes: {from: end, insert: 'z'}, selection: {anchor: end + 1}});
          const blocks = window.__blocks();
          return blocks.some((b) => b.text && b.text.endsWith('z'));
        }""")
        checks = [
            check("progressive mode ran (>=2 chunks)", p is not None and p["chunks"] >= 2, str(p)),
            check("TTI < full-load baseline (same machine/doc)", p is not None and p["ttiMs"] < baseline_load_ms,
                  f"tti={p['ttiMs']}ms vs full={baseline_load_ms}ms"),
            check("final state = full document", perf["blocks"] > 1000 and perf["docLen"] > 900_000,
                  f"blocks={perf['blocks']} len={perf['docLen']}"),
            check("editable after ingest (fast path lands in core)", typed),
        ]
        return {"checks": checks, "tti_ms": round(p["ttiMs"], 1), "ingest_ms": round(p["ingestMs"], 1),
                "final_render_ms": round(p["finalRenderMs"], 1), "chunks": p["chunks"],
                "baseline_full_load_ms": baseline_load_ms, "doc_len": perf["docLen"], "blocks": perf["blocks"]}
    finally:
        page.close()


def w8_long_transaction_stress(browser, port):
    """R11 验证①：长事务桥压测（1MB）。批量 op 单事务 / 幂等 sync 收敛 /
    批量 undo / CM6 多 change 事务兜底单次 sync。"""
    page, errors = open_page(browser, port, "?doc=big&kb=1024")
    try:
        r = page.evaluate("""() => {
          const core = window.__core;
          const blocks = window.__blocks();
          const rev0 = Number(core.revision());
          // ① 200-op 批量事务（每第 100 块替换前缀）
          const ops = [];
          for (let i = 0; i < 200 && i * 100 < blocks.length; i++) {
            const b = blocks[i * 100];
            ops.push({t: 'replace_text', path: [i * 100], start: 0,
                      end: Math.min(9, b.text.length), insert: 'BULK' + i});
          }
          core.set_clock_ms(BigInt(Date.now()));
          const t0 = performance.now();
          core.apply_ops(JSON.stringify(ops), 'ai', 'bulk_edit');
          const batchMs = performance.now() - t0;
          const t1 = performance.now();
          window.__bridge.pushRender();
          const rerenderMs = performance.now() - t1;
          const md1 = core.to_markdown();
          // ② 幂等收敛：对当前 doc 再 sync = 零差分（revision 不动）
          const revBeforeSync = Number(core.revision());
          const t2 = performance.now();
          core.sync_from_markdown(window.__view.state.doc.toString());
          const idemMs = performance.now() - t2;
          const idemNoop = Number(core.revision()) === revBeforeSync;
          // ③ 批量 undo 一次全撤（走桥：undo 事件经 drainEvents 入出口）
          const evBefore = window.__mdcoreEvents.length;
          window.__core.set_clock_ms(BigInt(Date.now()));
          const t3 = performance.now();
          const undone = window.__bridge.undo() === true;
          const undoMs = performance.now() - t3;
          const mdAfterUndo = window.__core.to_markdown();
          return {batchMs, rerenderMs, idemMs, idemNoop, undoMs, undone,
                  absorbed: md1.includes('BULK0') && md1.includes('BULK199'),
                  undoClean: !mdAfterUndo.includes('BULK0'),
                  blocksSame: window.__blocks().length === blocks.length,
                  undoEvent: window.__mdcoreEvents.length > evBefore,
                  rev0};
        }""")
        # ④ CM6 多 change 单事务 → 兜底一次 sync
        multi = page.evaluate("""() => {
          const v = window.__view;
          const doc = v.state.doc;
          const mid = Math.floor(doc.length / 2);
          const rev0 = Number(window.__core.revision());
          const t0 = performance.now();
          v.dispatch({changes: [
            {from: mid, insert: 'ALPHA'},
            {from: Math.floor(doc.length / 4), insert: 'BETA'},
            {from: Math.floor(doc.length * 3 / 4), insert: 'GAMMA'},
          ]});
          const ms = performance.now() - t0;
          const md = window.__core.to_markdown();
          return {ms, ok: md.includes('ALPHA') && md.includes('BETA') && md.includes('GAMMA'),
                  revAdvanced: Number(window.__core.revision()) > rev0,
                  docLen: v.state.doc.length};
        }""")
        checks = [
            check("200-op batch = single transaction, absorbed", r["absorbed"] and r["blocksSame"], str({k: r[k] for k in ('batchMs','blocksSame')})),
            check("idempotent resync is a no-op (rev unchanged)", r["idemNoop"], f"idem={r['idemMs']}ms"),
            check("batch undo reverts all, event recorded", r["undone"] and r["undoClean"] and r["undoEvent"], f"undo={r['undoMs']}ms"),
            check("multi-change CM6 tx → fallback absorbs all", multi["ok"] and multi["revAdvanced"], f"multi={multi['ms']}ms len={multi['docLen']}"),
        ]
        return {"checks": checks, "batch_ms": round(r["batchMs"], 1), "rerender_ms": round(r["rerenderMs"], 1),
                "idempotent_sync_ms": round(r["idemMs"], 1), "batch_undo_ms": round(r["undoMs"], 1),
                "multi_change_fallback_ms": round(multi["ms"], 1)}
    finally:
        page.close()


def w9_deep_synthetic_ime(page):
    """R11 验证②：深合成 IME 序列（真 IME 人工协议见 README）。多步 composition /
    取消路径 / 连续三轮。"""
    r = page.evaluate("""async () => {
      const v = window.__view;
      const content = document.querySelector('.cm-content');
      v.focus();
      const rev0 = Number(window.__core.revision());
      const ev0 = window.__mdcoreEvents.length;
      const out = {rounds: []};
      const tick = (ms) => new Promise((res) => setTimeout(res, ms));
      // —— 三轮连续 composition（每轮重设选区；CM6 经 MutationObserver
      //    异步吸收 DOM 变更，同步检查会假阴性——R11 审查记录）——
      for (let r = 1; r <= 3; r++) {
        const mark = 'IME' + r;
        const pos = v.state.doc.toString().indexOf('Final paragraph');
        v.dispatch({selection: {anchor: pos}});
        content.dispatchEvent(new CompositionEvent('compositionstart', {data: ''}));
        document.execCommand('insertText', false, mark + '字');
        content.dispatchEvent(new CompositionEvent('compositionupdate', {data: mark + '字'}));
        document.execCommand('insertText', false, '词');
        content.dispatchEvent(new CompositionEvent('compositionend', {data: mark + '字词'}));
        await tick(30);
        out.rounds.push(v.state.doc.toString().includes(mark + '字词'));
      }
      // —— 取消路径：composition 中插入 tmp（CM6 dispatch 模拟），再删净，end ——
      const posC = v.state.doc.toString().indexOf('Final paragraph');
      v.dispatch({selection: {anchor: posC}});
      content.dispatchEvent(new CompositionEvent('compositionstart', {data: ''}));
      const insAt = v.state.selection.main.anchor;
      v.dispatch({changes: {from: insAt, to: insAt, insert: 'CANCELLED'}});
      const pos2 = v.state.doc.toString().indexOf('CANCELLED');
      const inserted = pos2 >= 0;
      if (inserted) v.dispatch({changes: {from: pos2, to: pos2 + 9, insert: ''}});
      content.dispatchEvent(new CompositionEvent('compositionend', {data: ''}));
      await tick(30);
      out.cancelInserted = inserted;
      out.cancelNoResidue = !v.state.doc.toString().includes('CANCELLED');
      out.docIntact = ['Bridge Demo', 'first item'].every((s) => v.state.doc.toString().includes(s));
      out.coreConverged = window.__blocks().some((b) => b.text && b.text.includes('IME3字词'))
        && !window.__blocks().some((b) => b.text && b.text.includes('CANCELLED'));
      out.revAdvanced = Number(window.__core.revision()) >= rev0; // 可能因 1s 合并窗与 W3 共享 revision（§11.1 预期）
      out.eventsGrew = window.__mdcoreEvents.length > ev0;
      out.coreHas3 = window.__md().includes('IME3字词');
      out.docHas3 = v.state.doc.toString().includes('IME3字词');
      return out;
    }""")
    checks = [
        check("3 rapid composition rounds all absorbed", all(r["rounds"]), str(r["rounds"])),
        check("cancelled composition leaves no residue (view+core)",
              r["cancelInserted"] and r["cancelNoResidue"] and r["coreConverged"], str(r)),
        check("doc intact + core/view converged + events recorded", r["docIntact"] and r["eventsGrew"] and r["coreHas3"] and r["docHas3"], str(r)),
    ]
    return checks


def w10_big_doc_block_ops(browser, port):
    """R11 验证③：大文档块操作（1MB）。核驱动 split / fallback merge /
    结构 undo，全部断言 + 计时。"""
    page, errors = open_page(browser, port, "?doc=big&kb=1024")
    try:
        blocks0 = page.evaluate("() => window.__blocks().length")
        items0 = page.evaluate("() => window.__blocks().filter((b) => b.type === 'list_item').length")
        # ① 核驱动 Enter 分段（list item 尾；真实键盘触发）
        page.evaluate("""() => {
          const v = window.__view;
          const doc = v.state.doc;
          const line = doc.lineAt(doc.toString().indexOf('- item alpha 500'));
          v.focus();
          v.dispatch({selection: {anchor: line.to}});
        }""")
        t0 = time.time()
        page.keyboard.press("Enter")
        split_ms = (time.time() - t0) * 1000
        split = page.evaluate("""() => {
          const blocks = window.__blocks();
          return {n: blocks.length, items: blocks.filter((b) => b.type === 'list_item').length};
        }""")
        # ② Backspace 行首合并（fallback）
        page.evaluate("""() => {
          const v = window.__view;
          const doc = v.state.doc;
          const line = doc.lineAt(v.state.selection.main.anchor);
          v.dispatch({selection: {anchor: line.from}});
        }""")
        t1 = time.time()
        page.keyboard.press("Backspace")
        merge_ms = (time.time() - t1) * 1000
        merged = page.evaluate("() => ({n: window.__blocks().length})")
        # ③ 结构 undo 两次 → 块数与首批 ID 还原
        ids0 = page.evaluate("() => window.__blocks().map((b) => b.id).slice(0, 50)")
        t2 = time.time()
        page.keyboard.press("Control+z")
        page.keyboard.press("Control+z")
        undo_ms = (time.time() - t2) * 1000
        restored = page.evaluate("() => ({n: window.__blocks().length, ids: window.__blocks().map((b) => b.id).slice(0, 50)})")
        return {
            "checks": [
                check("big-doc core-driven split (blocks +1, list item +1)",
                      split["n"] == blocks0 + 1 and split["items"] == items0 + 1,
                      f"{blocks0}->{split['n']} items={items0}->{split['items']}"),
                check("big-doc fallback merge (blocks back)", merged["n"] == blocks0, f"n={merged['n']}"),
                check("structural undo x2 restores block count + IDs",
                      restored["n"] == blocks0 and restored["ids"] == ids0,
                      f"n={restored['n']} ids_ok={restored['ids'] == ids0}"),
            ],
            "blocks": blocks0, "split_ms": round(split_ms, 1),
            "merge_ms": round(merge_ms, 1), "structural_undo_ms": round(undo_ms, 1),
        }
    finally:
        page.close()


def w5_block_ops(page):
    checks = []
    # Enter 分段（段落行中 -> 浏览器原生 \n -> 兜底 sync 拆块）
    before = page.evaluate("() => window.__blocks().length")
    page.evaluate("""() => {
      const v = window.__view;
      const pos = v.state.doc.toString().indexOf('Plain paragraph');
      window.__view.focus();
      v.dispatch({selection: {anchor: pos + 5}}); // 'Plain| paragraph'
    }""")
    page.keyboard.press("Enter")
    after = page.evaluate("""() => {
      const blocks = window.__blocks();
      const texts = blocks.map((b) => b.text);
      return {n: blocks.length, texts, rev: Number(window.__core.revision())};
    }""")
    ok = (after["n"] == before + 1 and "Plain" in after["texts"]
          and any(t.lstrip().startswith("paragraph") for t in after["texts"]))
    checks.append(check("Enter splits paragraph into a new block", ok, f"before={before} after={after['n']} rev={after['rev']}"))

    # Backspace 行首合并（删 \\n -> 兜底 sync 合并）
    merged = page.evaluate("""() => {
      const doc = window.__view.state.doc;
      const line = doc.lineAt(doc.toString().indexOf('paragraph with'));
      const v = window.__view;
      window.__view.focus();
      v.dispatch({selection: {anchor: line.from}}); // 行首
      return window.__blocks().length;
    }""")
    page.keyboard.press("Backspace")
    after2 = page.evaluate("""() => {
      const blocks = window.__blocks();
      return {n: blocks.length, hasMerged: blocks.some((b) => b.text === 'Plain paragraph with **bold** and `inline code` inside.')};
    }""")
    checks.append(check("Backspace at line start merges blocks", after2["n"] == before and after2["hasMerged"], f"n={after2['n']} merged={after2['hasMerged']}"))

    # 列表续行（Enter 在 list item 尾 -> 核驱动 split，marker 继承）
    lst = page.evaluate("""() => {
      const doc = window.__view.state.doc;
      const line = doc.lineAt(doc.toString().indexOf('- first item'));
      window.__view.focus();
      window.__view.dispatch({selection: {anchor: line.to}});
      const itemsBefore = window.__blocks().filter((b) => b.type === 'list_item').length;
      return itemsBefore;
    }""")
    page.keyboard.press("Enter")
    cont = page.evaluate("""() => {
      const blocks = window.__blocks();
      const items = blocks.filter((b) => b.type === 'list_item');
      const doc = window.__view.state.doc.toString();
      return {items: items.length, secondMarker: items[1]?.attrs?.marker ?? null,
              secondLineIsItem: /^- $/m.test(doc), selLine: window.__view.state.doc.lineAt(window.__view.state.selection.main.anchor).number};
    }""")
    # 在新 item body 打字 -> 快路径直接进核
    page.keyboard.type("continued")
    typed = page.evaluate("""() => {
      const items = window.__blocks().filter((b) => b.type === 'list_item');
      return items.some((b) => b.text === 'continued');
    }""")
    checks.append(check("Enter continues list (core split, marker inherited) + typing lands in new item",
                        cont["items"] == lst + 1 and cont["secondMarker"] == "-" and cont["secondLineIsItem"] and typed,
                        f"items={lst}->{cont['items']} marker={cont['secondMarker']} typed={typed}"))
    return checks


def w6_undo_redo(page):
    checks = []
    ids_before = page.evaluate("() => window.__blocks().map((b) => b.id)")
    n_before = page.evaluate("() => window.__blocks().length")
    # 打字 -> 一次 undo 全撤（1s 合并窗）
    page.evaluate("""() => {
      const v = window.__view;
      const pos = v.state.doc.toString().indexOf('for typing.');
      window.__view.focus();
      v.dispatch({selection: {anchor: v.state.doc.length}});
    }""")
    page.keyboard.type("abc")
    typed = page.evaluate("() => window.__view.state.doc.toString().includes('abc')")
    page.keyboard.press("Control+z")
    undone = page.evaluate("""() => ({
      doc: !window.__view.state.doc.toString().includes('abc'),
      block: !window.__blocks().some((b) => b.text.includes('abc')),
      undoEvent: window.__mdcoreEvents.some((e) => e.meta && e.meta.source === 'undo'),
    })""")
    checks.append(check("typing (coalesced) reverted by one core undo", typed and undone["doc"] and undone["block"] and undone["undoEvent"], str(undone)))
    # 结构 undo：行尾 Enter 分段（核驱动 split）-> undo 合回 -> ID 恢复
    page.evaluate("""() => {
      const doc = window.__view.state.doc;
      const line = doc.lineAt(doc.toString().indexOf('Plain paragraph'));
      window.__view.focus();
      window.__view.dispatch({selection: {anchor: line.to}});
    }""")
    page.keyboard.press("Enter")
    split = page.evaluate("() => window.__blocks().length")
    page.keyboard.press("Control+z")
    back = page.evaluate("""() => {
      const blocks = window.__blocks();
      return {n: blocks.length, ids: blocks.map((b) => b.id)};
    }""")
    ids_ok = back["ids"] == ids_before
    checks.append(check("structural split undone (block count + IDs restored)",
                        split == n_before + 1 and back["n"] == n_before and ids_ok,
                        f"{n_before}->{split}->{back['n']} ids_ok={ids_ok}"))
    # redo 重新分段
    # 大写 Z = 真实浏览器 Shift+z 的 key 属性（Playwright 小写 z + shiftKey
    # 是合成怪癖，会首查命中 Ctrl-z 的 undo 绑定——R9 审查战役记录）
    page.keyboard.press("Control+Shift+Z")
    redone = page.evaluate("() => window.__blocks().length")
    checks.append(check("redo re-applies the split", redone == n_before + 1, f"n={redone}"))
    return checks


def w11_ai_loop(browser, port):
    """R12 验证：Human+AI 闭环浏览器半边。AI 经 JSON 边界读 blocks_json →
    确定性 op 流注入核（apply_ops source="ai"）→ CM6 渲染 AI 改动；
    Human/AI 事件同流；undo 整批回退不吞 Human 编辑（R12 合并窗收紧的
    端到端实证：AI 条目后的击键 revision 独立递增）。"""
    page, errors = open_page(browser, port)
    try:
        r = page.evaluate("""() => {
          const core = window.__core;
          const v = window.__view;
          const ev0 = window.__mdcoreEvents.length;
          // ① Human：CM6 快路径打字（source=view，经桥 mutate→drainEvents）
          const end = v.state.doc.length;
          v.dispatch({changes: {from: end, insert: ' human'}, selection: {anchor: end + 6}});
          // ② AI：读状态（blocks_json）→ 确定性 op（文首插入 AI note）→ JSON 边界提交
          core.set_clock_ms(BigInt(Date.now()));
          const first = JSON.parse(core.blocks_json()).find((b) => b.text);
          const ops = [{t: 'insert_block', parent: [], index: 0, kind: 'paragraph',
                        attrs: {}, text: 'AI note: ' + first.text, id: null}];
          let aiOk = true;
          try { core.apply_ops(JSON.stringify(ops), 'ai', 'ai.note'); } catch (e) { aiOk = false; }
          const mdAfterAi = core.to_markdown();
          // ③ 宿主收到 AI 事件 → pushRender：CM6 渲染出 AI 改动
          window.__bridge.pushRender();
          const docAfterAi = v.state.doc.toString();
          // ④ Human 在 AI 改动后继续打字（快路径）→ drainEvents 把 AI 事件
          //    与 view 事件一并取走（Human/AI 同一条 op 流的证据）
          const end2 = v.state.doc.length;
          v.dispatch({changes: {from: end2, insert: ' again'}, selection: {anchor: end2 + 6}});
          const events = window.__mdcoreEvents.slice(ev0);
          // ⑤ undo 两次：先撤人手 again，再撤 AI 整批（单条历史条目）
          window.__bridge.undo();
          const docAfterUndo1 = v.state.doc.toString();
          window.__bridge.undo();
          const docAfterUndo2 = v.state.doc.toString();
          return {aiOk, docAfterAi, mdAfterAi, docAfterUndo1, docAfterUndo2, events};
        }""")
        seq = [(e.get("meta", {}).get("source"), e.get("meta", {}).get("command")) for e in r["events"]]
        revs = [e.get("revision") for e in r["events"]]
        ai_op = (r["events"][1].get("ops") or [{}])[0] if len(r["events"]) > 1 else {}
        checks = [
            check("AI ops accepted at JSON boundary (source=ai)",
                  r["aiOk"] and "AI note: " in r["mdAfterAi"]),
            check("CM6 renders the AI change (Human edit preserved)",
                  r["docAfterAi"].startswith("AI note: ") and r["docAfterAi"].rstrip().endswith(" human"),
                  r["docAfterAi"][:60]),
            check("Human/AI share one event stream (view→ai→view)",
                  [s for s, _ in seq[:3]] == ["view", "ai", "view"] and seq[1][1] == "ai.note", str(seq[:3])),
            check("AI op id materialized in event stream (§8.2)",
                  isinstance(ai_op.get("id"), (int, float)) and ai_op.get("id", 0) >= 1, str(ai_op)),
            check("revisions strictly increasing (AI never coalesces; R12 view-after-ai rule)",
                  len(revs) >= 3 and revs[:3] == sorted(revs[:3]) and len(set(revs[:3])) == 3, str(revs[:3])),
            check("undo #1 reverts trailing Human keystroke only",
                  r["docAfterUndo1"].rstrip().endswith(" human") and not r["docAfterUndo1"].rstrip().endswith(" again")),
            check("undo #2 reverts the AI batch (single entry)",
                  "AI note: " not in r["docAfterUndo2"] and r["docAfterUndo2"].rstrip().endswith(" human")),
        ]
        return checks
    finally:
        page.close()


def w12_incremental_equivalence(browser, port):
    """R15 等价性护栏：装饰增量窗口化（selection-only 事务）与强制全量
    重建逐项等价（deco + atomic 双集，dump 全序字符串化对比）。
    跳转序列覆盖：marker 内揭示/移开再隐藏、双光标远距（窗口真并集）、
    跨行选区、doc 末尾边界窗口、返回行首。每步：先取增量快照，再强制
    全量重算取基线快照，两者必须逐项相同。"""
    page, errors = open_page(browser, port)
    try:
        r = page.evaluate("""() => {
          const b = window.__bridge;
          const doc = window.__view.state.doc.toString();
          const boldAt = doc.indexOf('**');
          const codeAt = doc.indexOf('`');
          const lines = doc.split('\\n');
          const posOfLine = (n) => lines.slice(0, n).reduce((a, l) => a + l.length + 1, 0);
          const hrAt = posOfLine(5);       // '---' 行首
          const end = doc.length;
          const jumps = [
            [[boldAt + 1, boldAt + 1]],        // bold 标记内 → 揭示
            [[0, 0]],                          // 移开 → 再隐藏
            [[boldAt, boldAt], [codeAt, codeAt]], // 双光标：两个 marker 同时揭示
            [[hrAt, hrAt + 3]],                // 跨行选区（窗口含相邻行）
            [[end - 1, end - 1]],              // doc 末尾（窗口贴右边界）
            [[0, 0]],                          // 返回行首（长跳回）
          ];
          const snap = () => JSON.stringify(b.dumpDecorations());
          const eq = [];
          for (const sels of jumps) {
            b.setSelections(sels);
            const inc = snap();               // 增量路径产物
            b.computeDecorations();           // 强制全量重算（dirty 置位）
            const full = snap();              // 全量基线
            eq.push({ sels, same: inc === full,
                      nInc: JSON.parse(inc).deco.length, nFull: JSON.parse(full).deco.length });
          }
          return { eq };
        }""")
        checks = [
            check(f"incremental == full rebuild at {j['sels']}", j["same"],
                  f"step#{i} deco n={j['nInc']}/{j['nFull']}")
            for i, j in enumerate(r["eq"])
        ]
        return checks
    finally:
        page.close()


def w13_non_bmp_units(browser, port):
    """R17 P1-9 单位宪法端到端：非 BMP 语料（ZWJ 家族 / CJK 扩展 B / 变体
    选择符）下 a) __bridge.unit 双向换算自证；b) blocks_json 的 text 与
    CM6 doc 行内容逐字符一致（码点口径）；c) 快路径编辑 1 码点目标时事件
    流 op 的 end-start 恰为 1（码点）——UTF-16 侧 span 为 2，单位宪法的
    直接证据；d) undo 后核侧块文本恒等（doc 经全量重渲染规范化为
    to_markdown 正则式，属 README §undo 既有取舍，断言做内容级比较）。"""
    page, errors = open_page(browser, port)
    try:
        r = page.evaluate("""async () => {
          const b = window.__bridge;
          const v = window.__view;
          const core = window.__core;
          const out = {};
          // ① unit 自证（单位宪法基础事实）
          const fam = '👨‍👩‍👧‍👦';
          out.famCp = b.unit.utf16ToCp(fam);
          out.famUtf16 = b.unit.cpToUtf16(fam, out.famCp);
          // ② 注入非 BMP 内容（多行插入 → 兜底 sync → 核重新解析建块）
          const end = v.state.doc.length;
          v.dispatch({changes: {from: end, insert: '\\n# 标题𠮷️ 🙂\\n\\n含 👨‍👩‍👧‍👦 与 𠮷 字。\\n'}});
          const doc = v.state.doc.toString();
          const lines = doc.split('\\n');
          const headLine = lines.find((l) => l.startsWith('# 标题'));
          const paraLine = lines.find((l) => l.includes('𠮷 字'));
          const blocks = JSON.parse(core.blocks_json());
          const headBlock = blocks.find((bl) => bl.text && bl.text.includes('𠮷'));
          const paraBlock = blocks.find((bl) => bl.text && bl.text.includes('𠮷 字'));
          out.blockMatch = !!(headBlock && headBlock.text === headLine.replace(/^#\\s+/, ''))
            && !!(paraBlock && paraBlock.text === paraLine);
          out.cpEquals = b.unit.utf16ToCp(paraLine) === [...paraBlock.text].length;
          // ③ 快路径编辑：把段落里的 𠮷（1 码点 / 2 UTF-16）换成 吉（1 码元）
          //    —— 用 unit.cpToUtf16 定位（P1-9 换算出口的真实消费者）
          const target = paraLine.indexOf('𠮷');
          const lineStart = doc.indexOf(paraLine);
          const at = lineStart + b.unit.cpToUtf16(paraLine, b.unit.utf16ToCp(paraLine, target));
          v.dispatch({changes: {from: at, to: at + 2, insert: '吉'}});
          const ev = window.__mdcoreEvents[window.__mdcoreEvents.length - 1];
          const op = (ev && ev.ops && ev.ops[0]) || {};
          out.fastOp = {t: op.t, span: op.end - op.start, insert: op.insert,
                        ops: ev ? ev.ops.length : 0, bail: b.lastFastBail};
          const docAfter = v.state.doc.toString();
          out.editLanded = docAfter.includes('与 吉 字') && !docAfter.includes('𠮷 字');
          const blocksAfter = JSON.parse(core.blocks_json());
          const paraAfter = blocksAfter.find((bl) => bl.text && bl.text.includes('与 吉 字'));
          out.coreMatchesDoc = !!(paraAfter && paraAfter.text === docAfter.split('\\n').find((l) => l.includes('与 吉 字')));
          // ④ undo 恒等（核侧块文本；doc 经重渲染规范化，做内容级比较）
          b.undo();
          const docUndo = v.state.doc.toString();
          const blocksUndo = JSON.parse(core.blocks_json());
          const paraUndo = blocksUndo.find((bl) => bl.text && bl.text.includes('𠮷 字'));
          const content = (s2) => s2.split('\\n').filter((l) => l !== '').join('\\n');
          out.undoIdentity = content(docUndo) === content(doc)
            && !!(paraUndo && paraUndo.text === paraLine);
          out.roundTrip = out.famCp === 7 && out.famUtf16 === 11;
          return out;
        }""")
        checks = [
            check(f"unit constitution facts (ZWJ family {r['famCp']}cp/{r['famUtf16']}utf16)", r["roundTrip"],
                  f"cp={r['famCp']} utf16={r['famUtf16']}"),
            check("blocks_json text == doc line (code-point identity)", r["blockMatch"] and r["cpEquals"],
                  f"blockMatch={r['blockMatch']} cpEquals={r['cpEquals']}"),
            check("fast-path op offsets are code points (2utf16 span -> 1cp op)",
                  r["fastOp"]["t"] == "replace_text" and r["fastOp"]["span"] == 1 and r["fastOp"]["ops"] == 1,
                  str(r["fastOp"])),
            check("edit landed on the exact code point (core==doc)", r["editLanded"] and r["coreMatchesDoc"],
                  f"landed={r['editLanded']} core={r['coreMatchesDoc']}"),
            check("undo restores core identity (content-level doc compare)", r["undoIdentity"],
                  f"undoIdentity={r['undoIdentity']}"),
        ]
        return checks
    finally:
        page.close()


def g1_cursor_latency(browser, port):
    """R14 Gate 2（§15.3 击键→光标响应 <16ms）三路拆分：
    a) selection-only dispatch = 纯光标路径（判 Gate 2）；
    b) typing 全路径 dispatch（与 W4 typeMs 同语义，加大样本）；
    c) core-only op（WasmCore 旁路）= 核内份额归因。
    另记 1MB 终态 JS 堆（§15.1 非门预算 250MB，只记录）。"""
    page, errors = open_page(browser, port, "?doc=big&kb=1024")
    try:
        r = page.evaluate("""() => {
          const view = window.__view;
          const core = window.__core;
          const p95 = (a) => { const s = [...a].sort((x, y) => x - y);
                               return s[Math.min(s.length - 1, Math.floor(s.length * 0.95))]; };
          const docLen = view.state.doc.length;
          // a) 纯光标路径：selection-only dispatch，无文本变更
          const cur = [];
          for (let i = 0; i < 30; i++) {
            const t = performance.now();
            view.dispatch({selection: {anchor: Math.floor(docLen * (i % 10) / 10)}});
            cur.push(performance.now() - t);
          }
          // b) typing 全路径（与 W4 同语义，30 样本）
          const typ = [];
          for (let i = 0; i < 30; i++) {
            const t = performance.now();
            view.dispatch({changes: {from: view.state.doc.length, insert: 'x'}});
            typ.push(performance.now() - t);
          }
          // c) core-only 旁路：不进 CM6 的同形 replace_text（核内份额）
          const blocks = window.__blocks();
          core.set_clock_ms(BigInt(Date.now()));
          const coreMs = [];
          for (let i = 0; i < 30; i++) {
            const idx = (i * 97) % blocks.length;
            const b = blocks[idx];
            const op = {t: 'replace_text', path: [idx], start: 0,
                        end: Math.min(4, b.text.length), insert: 'C' + i};
            const t = performance.now();
            core.apply_ops(JSON.stringify([op]), 'view', 'g1-probe');
            coreMs.push(performance.now() - t);
          }
          const mem = performance.memory ? performance.memory.usedJSHeapSize : null;
          return {cursor_p95: p95(cur), cursor_max: Math.max(...cur),
                  type_p95: p95(typ), type_max: Math.max(...typ),
                  core_p95: p95(coreMs), core_p50: [...coreMs].sort((x,y)=>x-y)[15],
                  doc_len: docLen, blocks: blocks.length, heap_used_mb: mem ? mem / 1048576 : null};
        }""")
        checks = [
            check("1MB doc loaded for gate2", r["doc_len"] > 900_000, f"len={r['doc_len']}"),
            check("cursor-path dispatch works and timed (selection-only)",
                  r["cursor_p95"] > 0 and r["cursor_p95"] < 1000,
                  f"p95={r['cursor_p95']:.1f}ms max={r['cursor_max']:.1f}ms"),
            check("typing full path p95 recorded (attribution, non-gate)",
                  r["type_p95"] > 0, f"p95={r['type_p95']:.1f}ms"),
            check("core-only op p95 recorded (attribution, non-gate)",
                  r["core_p95"] > 0, f"p95={r['core_p95']:.1f}ms p50={r['core_p50']:.1f}ms"),
        ]
        # Gate 2 判定（§15.3 <16ms）：记录于 gate_verdict / gates.md，不翻全局
        # pass（回归套件与门判定分离；R15 增量窗口化后的归因见 gates.md）。
        verdict = ("PASS — cursor p95 %.1fms < 16ms（R15 装饰增量窗口化后）" % r["cursor_p95"]
                   if r["cursor_p95"] < 16 else
                   "GAP — cursor p95 %.1fms > 16ms（R15 增量窗口化后仍超预算，"
                   "需重归因：窗口比较/拼装残余成本或 CM6 选区自身开销）" % r["cursor_p95"])
        return {"checks": checks, "cursor_p95_ms": round(r["cursor_p95"], 1),
                "cursor_max_ms": round(r["cursor_max"], 1),
                "typing_p95_ms": round(r["type_p95"], 1),
                "core_only_p95_ms": round(r["core_p95"], 1),
                "core_only_p50_ms": round(r["core_p50"], 1),
                "heap_used_mb": round(r["heap_used_mb"], 1) if r["heap_used_mb"] else None,
                "doc_len": r["doc_len"], "blocks": r["blocks"], "gate_verdict": verdict}
    finally:
        page.close()


def g2_10mb_progressive_memory(browser, port):
    """R14 §28 P0-3 量化：10MB 渐进打开（出文本/TTI/终态一致性）+ 终态
    JS 堆（§15.1 非门预算 600MB，只记录）。
    R16 测量口径：块计数走 __blockCount()（零分配绑定）；堆测量在强制
    GC 后取开档稳态（判定依据）。
    R21/ADR-B4：渐进开档后 undo 栈必空（System ingest 不入 undo 栈）→
    undo 必为 no-op（行为锚点：开档不可撤）；原「10MB 级 undo 压测」
    随之失去主体，改为断言 no-op 语义 + undo_entries==0 + 计时透明记录。"""
    page, errors = open_page(browser, port, "?doc=big&kb=10240&chunks=16")
    try:
        r = page.evaluate("""async () => {
          const p = window.__perf.progressive;
          const blocks = window.__blockCount ? window.__blockCount() : window.__blocks().length;
          const docLen = window.__view.state.doc.length;
          // R16 测量卫生：undo/pushRender 的全量重建属瞬态垃圾，测稳态前
          // 强制回收（--js-flags=--expose-gc；两次调用覆盖跨代引用）。
          // R16 测量口径（§15.1「终态 JS 堆」= 开档稳态）：
          // ① 强制 GC 后取开档稳态堆（判定依据）；undo 属压测动作，
          //    其重建路径的瞬态峰值与 GC 后残留在测后透明记录（不入判定）。
          if (window.gc) { window.gc(); window.gc(); }
          const mem = performance.memory ? performance.memory.usedJSHeapSize : null;
          // R19 口径修正：新版 Chromium 的 usedJSHeapSize 包含 wasm 线性内存
          // （实测：GC 稳态 568MB 中 wasm 占 509MB，JS 分配采样仅 46.5MB）。
          // 归因必须分离：js_heap = used - wasm；wasm 单独透明记录（核侧）。
          const wasmBytes = window.__wasmMemory ? window.__wasmMemory.buffer.byteLength : null;
          // R21/ADR-B4：开档后（全部 System ingest）undo 栈必空、undo 必 no-op；
          // __memStats.undo_entries 锁定战果（R20 的 15 → 0）。注意：
          // __bridge.undo() 返回值是 CM6 键位契约（恒 true 消费按键），核侧
          // 是否真撤必须看 revision（成功 undo 必 bump）与 undo 事件流。
          const msHist = window.__memStats ? window.__memStats().history : null;
          const undoEntries = msHist ? msHist.undo_entries : null;
          const revBeforeUndo = window.__memStats ? window.__memStats().revision : null;
          const evBeforeUndo = window.__mdcoreEvents ? window.__mdcoreEvents.length : 0;
          const t0 = performance.now();
          const okUndo = window.__bridge.undo();
          const undoMs = performance.now() - t0;
          const revAfterUndo = window.__memStats ? window.__memStats().revision : null;
          const undoEventFired = window.__mdcoreEvents
            ? window.__mdcoreEvents.slice(evBeforeUndo).some((e) => e.meta && e.meta.source === 'undo')
            : null;
          const t1 = performance.now();
          window.__bridge.pushRender();
          const rerenderMs = performance.now() - t1;
          if (window.gc) { window.gc(); window.gc(); }
          await new Promise((r2) => setTimeout(r2, 200));
          if (window.gc) { window.gc(); window.gc(); }
          const memAfterUndo = performance.memory ? performance.memory.usedJSHeapSize : null;
          const wasmAfterUndo = window.__wasmMemory ? window.__wasmMemory.buffer.byteLength : null;
          return {tti: p ? p.ttiMs : null, ingest: p ? p.ingestMs : null,
                  heap_after_undo_mb: memAfterUndo ? memAfterUndo / 1048576 : null,
                  wasm_after_undo_mb: wasmAfterUndo ? wasmAfterUndo / 1048576 : null,
                  final_render: p ? p.finalRenderMs : null, chunks: p ? p.chunks : null,
                  blocks, doc_len: docLen, ok_undo: okUndo, undo_ms: undoMs,
                  undo_entries: undoEntries,
                  rev_before_undo: revBeforeUndo, rev_after_undo: revAfterUndo,
                  undo_event_fired: undoEventFired,
                  rerender_ms: rerenderMs,
                  heap_used_mb: mem ? mem / 1048576 : null,
                  wasm_memory_mb: wasmBytes ? wasmBytes / 1048576 : null,
                  js_heap_mb: (mem && wasmBytes) ? (mem - wasmBytes) / 1048576 : null,
                  heap_trace: window.__heapTrace || []};
        }""")
        checks = [
            check("10MB doc ingested progressively (len > 9MB)", r["doc_len"] > 9_000_000,
                  f"len={r['doc_len']}"),
            check("10MB block count sane (> 100k)", r["blocks"] > 100_000, f"blocks={r['blocks']}"),
            check("progressive TTI recorded", r["tti"] is not None, f"tti={r['tti']}"),
            check("ADR-B4: undo_entries == 0 after progressive open (System ingest off undo stack)",
                  r["undo_entries"] == 0, f"undo_entries={r['undo_entries']}"),
            check("ADR-B4: undo is no-op after pure ingest (revision unchanged, no undo event; ok_undo=true is keymap contract)",
                  r["rev_after_undo"] == r["rev_before_undo"] and not r["undo_event_fired"],
                  f"rev {r['rev_before_undo']} -> {r['rev_after_undo']}, undo_event={r['undo_event_fired']}, ok_undo={r['ok_undo']} ({r['undo_ms']:.1f}ms)"),
            check("10MB heap split recorded (R19: JS heap vs wasm linear memory)",
                  r["heap_used_mb"] is not None and r["wasm_memory_mb"] is not None,
                  f"used={round(r['heap_used_mb'], 1)}MB = JS {round(r['js_heap_mb'], 1)}MB + wasm {round(r['wasm_memory_mb'], 1)}MB"),
        ]
        js_flag = r["js_heap_mb"] and r["js_heap_mb"] >= 600
        wasm_note = r["wasm_memory_mb"] and r["wasm_memory_mb"] >= 100
        verdict = ("PASS（Gate1 口径：1MB TTI < 600ms 在 W4/W7 复验；10MB 出文本/TTI 实测入档；"
                   "R21/ADR-B4：undo no-op + undo_entries==0 锁定）"
                   + ("；R19 口径修正：10MB 开档稳态 JS 堆 %.0fMB 达标（非门 600MB）" % r["js_heap_mb"] if not js_flag else
                      "；FLAG：10MB JS 堆 %.0fMB > 600MB" % r["js_heap_mb"])
                   + ("；wasm 线性内存 %.0fMB（核侧 ropey+arena+revision，透明记录，优化登记 R20）" % r["wasm_memory_mb"] if wasm_note else "")
                   + ("；undo（no-op）后 used 残留 %.0fMB / wasm %.0fMB（透明记录）"
                      % (r["heap_after_undo_mb"], r["wasm_after_undo_mb"])
                      if r["heap_after_undo_mb"] else ""))
        return {"checks": checks, "tti_ms": round(r["tti"], 1) if r["tti"] else None,
                "undo_entries": r["undo_entries"],
                "rev_before_undo": r["rev_before_undo"], "rev_after_undo": r["rev_after_undo"],
                "undo_event_fired": r["undo_event_fired"],
                "heap_after_undo_mb": round(r["heap_after_undo_mb"], 1) if r["heap_after_undo_mb"] else None,
                "wasm_after_undo_mb": round(r["wasm_after_undo_mb"], 1) if r["wasm_after_undo_mb"] else None,
                "ingest_ms": round(r["ingest"], 1) if r["ingest"] else None,
                "final_render_ms": round(r["final_render"], 1) if r["final_render"] else None,
                "chunks": r["chunks"], "blocks": r["blocks"], "doc_len": r["doc_len"],
                "undo_ms": round(r["undo_ms"], 1), "rerender_ms": round(r["rerender_ms"], 1),
                "heap_used_mb": round(r["heap_used_mb"], 1) if r["heap_used_mb"] else None,
                "js_heap_mb": round(r["js_heap_mb"], 1) if r["js_heap_mb"] is not None else None,
                "wasm_memory_mb": round(r["wasm_memory_mb"], 1) if r["wasm_memory_mb"] is not None else None,
                "heap_trace": r["heap_trace"],
                "gate_verdict": verdict}
    finally:
        page.close()


def main():
    import os

    os.chdir(WEB / "dist")
    httpd = ReusableTCPServer(("127.0.0.1", 0), QuietServer)
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    results = {"meta": {
        "browser": "chromium-headless", "runner": "playwright",
        "page": "web/dist/index.html (CM6 <-> mdcore-wasm bridge)",
        "note_w3": "synthetic only, real IME = R11",
    }, "tests": {}}
    failures = []

    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=["--js-flags=--expose-gc", "--enable-precise-memory-info"])
        try:
            page, errors = open_page(browser, port)
            # W9 紧跟 W3 同页执行（合成 composition 对页面历史敏感，实测
            # 新页首插会静默失败——R11 审查记录）
            for name, fn in (("W1", w1_syntax_hiding), ("W2", w2_cursor_negotiation),
                             ("W3", w3_synthetic_ime), ("W9", w9_deep_synthetic_ime),
                             ("W5", w5_block_ops), ("W6", w6_undo_redo)):
                results["tests"][name] = fn(page)
            page.close()

            w4res = w4_big_doc(browser, port)
            results["tests"]["W4"] = w4res
            results["tests"]["W7"] = w7_progressive_open(browser, port, w4res["load_ms"])
            results["tests"]["W8"] = w8_long_transaction_stress(browser, port)
            results["tests"]["W10"] = w10_big_doc_block_ops(browser, port)
            results["tests"]["W11"] = w11_ai_loop(browser, port)
            results["tests"]["W12"] = w12_incremental_equivalence(browser, port)
            results["tests"]["W13"] = w13_non_bmp_units(browser, port)
            # R14 硬 Gate 收口轮（G 前缀）
            results["tests"]["G1"] = g1_cursor_latency(browser, port)
            results["tests"]["G2"] = g2_10mb_progressive_memory(browser, port)
        finally:
            browser.close()

    for name, checks in results["tests"].items():
        for c in checks if isinstance(checks, list) else checks["checks"]:
            if not c["pass"]:
                failures.append(f"{name}: {c['check']} ({c.get('detail', '')})")
    if errors:
        failures.append(f"pageerror: {errors}")

    results["summary"] = {"total": 15, "failed": len(failures), "pass": len(failures) == 0,
                          "failures": failures}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(results, indent=2, ensure_ascii=False))
    print(json.dumps(results["tests"]["W4"], indent=2, ensure_ascii=False))
    print(json.dumps(results["summary"], indent=2, ensure_ascii=False))
    print(f"\nfull results -> {OUT}")
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
