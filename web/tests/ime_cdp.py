#!/usr/bin/env python3
"""R23-a IME 自动化验收（gates.md R22 候选清单第 4 项的自动化先行）：
CDP 真实输入法模拟驱动 CM6<->mdcore-wasm 桥的组合流。

与 web_smoke.py W3/W9（synthetic：手动 dispatch CompositionEvent +
execCommand 模拟 DOM 变更）不同，本测试走 **真浏览器输入法通道**：
Chromium InputMethodController 经 CDP `Input.imeSetComposition` /
`Input.insertText` 产生真实的 compositionstart/update/end 与
beforeinput(inputType=insertCompositionText) 事件链和原生预编辑 DOM 变更，
覆盖 W3 合成路径无法到达的 OS 级事件语义。

参数形状（chromium-1243 实测，与旧协议不同）：imeSetComposition 取
`text`（非 compositionText）；提交 = 组合中 Input.insertText（自动
compositionend）；取消 = imeSetComposition text=""（空 data compositionend）。

证据：tests/results/results_ime.json（含逐事件日志、op 流、undo 栈深度）。
各 IC 的断言全部以实现实测语义为准（探针 = probe_ime_cdp.py）：
- 组合期间每个预编辑 update 经 CM6 MutationObserver → 桥快路径逐个落
  replace_text op（无缓冲），1s 合并窗把整段组合收进单个 undo 条目；
- 提交步本身 = 单个 replace_text op 事件；
- 跨行提交触发兜底 sync（结构性 op，不并条，undo 分次回退）。
"""

import http.server
import json
import os
import socketserver
import sys
import threading
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_ime.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def log_message(self, *a):
        pass


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


def check(name, cond, detail=""):
    return {"check": name, "pass": bool(cond), "detail": detail}


INSTALL_LOG = """() => {
  window.__imeLog = [];
  const log = (kind, ev) => window.__imeLog.push({
    kind, data: ev.data === undefined ? null : ev.data, inputType: ev.inputType || null });
  const content = document.querySelector('.cm-content');
  for (const type of ['compositionstart', 'compositionupdate', 'compositionend'])
    content.addEventListener(type, (e) => log(type, e));
  content.addEventListener('beforeinput', (e) => log('beforeinput', e));
}"""

SNAP = """() => {
  const doc = window.__view.state.doc.toString();
  const events = window.__mdcoreEvents;
  const fresh = events.slice(window.__evCursor || 0);
  window.__evCursor = events.length;
  const logOut = window.__imeLog.slice().map((x) => JSON.stringify(x));
  window.__imeLog.length = 0;
  const i = doc.indexOf('for typing.');
  return {
    log: logOut,
    newEvents: fresh.map((ev) => ({
      rev: ev.revision, coalesced: ev.meta && ev.meta.coalesced,
      src: ev.meta && ev.meta.source, cmd: ev.meta && ev.meta.command,
      ops: (ev.ops || []).map((o) => ({t: o.t, start: o.start, end: o.end, ins: o.insert})) })),
    docAround: doc.slice(Math.max(0, i - 10), i + 26),
    docIsMd: doc === window.__md(),
    rev: Number(window.__core.revision()),
    undoLen: JSON.parse(window.__core.history_json()).undo.length,
    composing: !!window.__view.composing,
    bail: window.__bridge.lastFastBail,
  };
}"""


def place_cursor(page, at_marker="for typing."):
    """聚焦编辑器并把光标放到 Final paragraph 的 'for typing.' 之前。"""
    return page.evaluate("""(marker) => {
      const v = window.__view;
      const pos = v.state.doc.toString().indexOf(marker);
      v.focus();
      v.dispatch({selection: {anchor: pos}});
      window.__evCursor = window.__mdcoreEvents.length;
      return pos;
    }""", at_marker)


def snap(page, step, wait=0.15):
    time.sleep(wait)
    s = page.evaluate(SNAP)
    s["step"] = step
    return s


def ic1_pinyin_commit(page, cdp):
    """主链：n → ni（预编辑）→ 提交 '你'。真实 DOM 事件链 + op 流 + 合并窗 undo。"""
    place_cursor(page)
    steps = []
    cdp.send("Input.imeSetComposition", {"text": "n", "selectionStart": 1, "selectionEnd": 1})
    steps.append(snap(page, "preedit 'n'"))
    cdp.send("Input.imeSetComposition", {"text": "ni", "selectionStart": 2, "selectionEnd": 2})
    steps.append(snap(page, "preedit 'ni'"))
    cdp.send("Input.insertText", {"text": "你"})
    steps.append(snap(page, "commit '你' (Input.insertText)"))

    undo_before = steps[-1]["undoLen"]
    doc_after_commit = page.evaluate("() => window.__view.state.doc.toString()")
    undone = page.evaluate("() => window.__bridge.undo()")
    post = page.evaluate("""() => ({
      doc: window.__view.state.doc.toString(),
      md: window.__md(),
      undoLen: JSON.parse(window.__core.history_json()).undo.length,
      docIsMd: window.__view.state.doc.toString() === window.__md() })""")
    undo_after = post["undoLen"]

    all_log = [x for s in steps for x in s["log"]]
    evs = [e for s in steps for e in s["newEvents"]]
    chain = {
        "comp_start": sum(1 for x in all_log if '"compositionstart"' in x),
        "comp_update": sum(1 for x in all_log if '"compositionupdate"' in x),
        "comp_end": sum(1 for x in all_log if '"compositionend"' in x),
        "beforeinput_composition": sum(1 for x in all_log if "insertCompositionText" in x),
    }
    commit_evs = evs[-1]
    doc_after = post["doc"]
    checks = [
        check("real DOM event chain: start/update×2/end + beforeinput(insertCompositionText)",
              chain["comp_start"] == 1 and chain["comp_update"] >= 2 and chain["comp_end"] == 1
              and chain["beforeinput_composition"] >= 3,
              f"chain={chain} log={all_log}"),
        check("preedit updates land per-update replace_text ops (implemented semantics: no buffering)",
              len(evs) == 3 and all(len(e["ops"]) == 1 and e["ops"][0]["t"] == "replace_text" for e in evs),
              json.dumps(evs, ensure_ascii=False)),
        check("merge window: first op opens entry (coalesced=false), later ops coalesce, revision holds",
              evs[0]["coalesced"] is False and all(e["coalesced"] for e in evs[1:])
              and evs[0]["rev"] == 1 and all(e["rev"] == 1 for e in evs[1:]),
              f"coalesced={[e['coalesced'] for e in evs]} rev={[e['rev'] for e in evs]}"),
        check("commit lands a single replace_text op (minimal-diff replace of the preedit)",
              len(commit_evs["ops"]) == 1 and commit_evs["ops"][0]["t"] == "replace_text"
              and "你for typing." in doc_after_commit,
              f"commitEv={json.dumps(commit_evs, ensure_ascii=False)} docAfterCommit={steps[-1]['docAround']}"),
        check("whole composition+commit = ONE undo entry (ADR-B2 merge window)",
              undo_before == 1,
              f"undoLen={undo_before}"),
        check("one undo reverts the whole commit (view+core converge, doc==md)",
              undone and undo_after == undo_before - 1 and "你" not in doc_after
              and "for typing." in doc_after and post["docIsMd"],
              f"undone={undone} undoLen {undo_before}->{undo_after} docIsMd={post['docIsMd']}"),
    ]
    return {"checks": checks, "steps": steps, "event_chain": chain,
            "post_undo_doc_around": doc_after[doc_after.find("Final paragraph"):][:60] if "Final paragraph" in doc_after else doc_after[-60:]}


def ic2_multichar_word(page, cdp):
    """多字词：预编辑 ni → nihao → 候选'你好' → 提交更长词 '你好世界'。"""
    place_cursor(page)
    steps = []
    cdp.send("Input.imeSetComposition", {"text": "ni", "selectionStart": 2, "selectionEnd": 2})
    steps.append(snap(page, "preedit 'ni'"))
    cdp.send("Input.imeSetComposition", {"text": "nihao", "selectionStart": 5, "selectionEnd": 5})
    steps.append(snap(page, "preedit 'nihao'"))
    cdp.send("Input.imeSetComposition", {"text": "你好", "selectionStart": 2, "selectionEnd": 2})
    steps.append(snap(page, "candidate '你好'"))
    cdp.send("Input.insertText", {"text": "你好世界"})
    steps.append(snap(page, "commit '你好世界'"))

    evs = [e for s in steps for e in s["newEvents"]]
    undo_len = steps[-1]["undoLen"]
    doc = page.evaluate("() => window.__view.state.doc.toString()")
    one_undo = page.evaluate("""() => {
      window.__bridge.undo();
      const d = window.__view.state.doc.toString();
      return {noWord: !d.includes('你好世界'), intact: d.includes('Bridge Demo') && d.includes('for typing.'),
              docIsMd: d === window.__md()};
    }""")
    commit_ev = evs[-1]
    checks = [
        check("multi-char preedit chain absorbed (nihao→你好 rewrites land as ops)",
              all(any(o["t"] == "replace_text" for o in e["ops"]) for e in evs) and len(evs) == 4,
              json.dumps(evs, ensure_ascii=False)),
        check("commit = single replace_text op (Chromium commits diff vs preedit: ins='世界')",
              len(commit_ev["ops"]) == 1 and commit_ev["ops"][0]["t"] == "replace_text"
              and commit_ev["ops"][0]["ins"] == "世界",
              f"commitEv={json.dumps(commit_ev, ensure_ascii=False)}"),
        check("final doc contains committed word, view==core", "你好世界" in doc and steps[-1]["docIsMd"],
              f"doc={steps[-1]['docAround']} docIsMd={steps[-1]['docIsMd']}"),
        check("one undo reverts the whole multi-char session (single merged entry)",
              undo_len == 1 and one_undo["noWord"] and one_undo["intact"] and one_undo["docIsMd"],
              f"undoLen={undo_len} post={json.dumps(one_undo, ensure_ascii=False)}"),
    ]
    return {"checks": checks, "steps": steps}


def ic3_crossline_commit(page, cdp):
    """跨块（跨行）提交：组合 '前' → 提交含 \\n 文本（真实 IME 不产 \\n，
    此处用提交通道压测兜底路径；观察到的核侧语义 = 解析器按行切块）。"""
    place_cursor(page)
    steps = []
    cdp.send("Input.imeSetComposition", {"text": "前", "selectionStart": 1, "selectionEnd": 1})
    steps.append(snap(page, "preedit '前'"))
    cdp.send("Input.insertText", {"text": "甲\n乙"})
    steps.append(snap(page, "commit '甲\\n乙' (cross-line)"))

    evs = [e for s in steps for e in s["newEvents"]]
    commit_evs = evs[-1]
    undo_len = steps[-1]["undoLen"]
    # undo①：应整撤兜底 sync 的事务（回到预编辑态 '前'）；undo②：撤预编辑。
    u1 = page.evaluate("""() => {
      window.__bridge.undo();
      const d = window.__view.state.doc.toString();
      return {backToPreedit: d.includes('前') && !d.includes('甲') && !d.includes('乙'),
              docIsMd: d === window.__md()};
    }""")
    u2 = page.evaluate("""() => {
      window.__bridge.undo();
      const d = window.__view.state.doc.toString();
      return {preeditGone: !d.includes('前'), docIsMd: d === window.__md()};
    }""")
    checks = [
        check("fast path bails on newline (lastFastBail='newline') → fallback sync absorbs",
              steps[-1]["bail"] == "newline" and "甲" in steps[-1]["docAround"]
              and "乙" in steps[-1]["docAround"],
              f"bail={steps[-1]['bail']} doc={steps[-1]['docAround']}"),
        check("cross-line commit event carries the newline (structural split by parser)",
              any("\\n" in (o["ins"] or "") or "\n" in (o["ins"] or "") for o in commit_evs["ops"])
              or commit_evs["cmd"] == "sync_from_markdown",
              f"commitEvs={json.dumps(commit_evs, ensure_ascii=False)}"),
        check("view==core after cross-line commit", steps[-1]["docIsMd"],
              f"docIsMd={steps[-1]['docIsMd']}"),
        check("structural commit opens its own entry (not coalesced into preedit entry)",
              undo_len == 2,
              f"undoLen={undo_len} evs={json.dumps([(e['cmd'], e['coalesced']) for e in evs], ensure_ascii=False)}"),
        check("undo① reverts the whole commit back to preedit state; undo② reverts preedit",
              u1["backToPreedit"] and u1["docIsMd"] and u2["preeditGone"] and u2["docIsMd"],
              f"u1={json.dumps(u1, ensure_ascii=False)} u2={json.dumps(u2, ensure_ascii=False)}"),
    ]
    return {"checks": checks, "steps": steps}


def ic4_cancel(page, cdp):
    """取消路径：组合中 imeSetComposition text=''（Chromium 取消语义 =
    compositionupdate('') + compositionend(data='')），无残留。"""
    place_cursor(page)
    steps = []
    cdp.send("Input.imeSetComposition", {"text": "x", "selectionStart": 1, "selectionEnd": 1})
    steps.append(snap(page, "preedit 'x'"))
    cdp.send("Input.imeSetComposition", {"text": "", "selectionStart": 0, "selectionEnd": 0})
    steps.append(snap(page, "cancel (text='')"))

    log = [x for s in steps for x in s["log"]]
    evs = [e for s in steps for e in s["newEvents"]]
    undo_len = steps[-1]["undoLen"]
    doc = page.evaluate("() => window.__view.state.doc.toString()")
    u = page.evaluate("""() => {
      window.__bridge.undo();
      const d = window.__view.state.doc.toString();
      return {docIsMd: d === window.__md(), noX: !d.includes('xfor')};
    }""")
    end_evt = [x for x in log if '"compositionend"' in x]
    checks = [
        check("cancel emits compositionupdate('') + compositionend(data='') — real cancel semantics",
              len(end_evt) == 1 and '"data":""' in end_evt[0],
              f"endEvt={end_evt}"),
        check("preedit removed cleanly: no residue in doc, composing cleared",
              "xfor" not in doc and not steps[-1]["composing"] and steps[-1]["docIsMd"],
              f"doc={steps[-1]['docAround']} composing={steps[-1]['composing']}"),
        check("cancel burst still lands ops (insert+delete coalesce into one entry)",
              len(evs) == 2 and undo_len == 1,
              f"evs={json.dumps(evs, ensure_ascii=False)} undoLen={undo_len}"),
        check("undo of a cancel-only entry is content-neutral",
              u["noX"] and u["docIsMd"],
              f"post={json.dumps(u, ensure_ascii=False)}"),
    ]
    return {"checks": checks, "steps": steps}


def ic5_emoji_nonbmp(page, cdp):
    """非 BMP 冒烟：Emoji 组合+提交（W13 单位宪法在真实事件流下不炸）。"""
    place_cursor(page)
    steps = []
    cdp.send("Input.imeSetComposition", {"text": "🎉", "selectionStart": 2, "selectionEnd": 2})
    steps.append(snap(page, "preedit '🎉' (non-BMP)"))
    cdp.send("Input.insertText", {"text": "🎉🚀"})
    steps.append(snap(page, "commit '🎉🚀'"))
    # 提交后紧接一次普通 insertText（快路径），验证紧贴非 BMP 字符的
    # 快路径 op 偏移仍是码点（W13 宪法在真实事件流下）。
    cdp.send("Input.insertText", {"text": "X"})
    steps.append(snap(page, "plain insertText 'X' after emoji (fast path)"))

    evs = [e for s in steps for e in s["newEvents"]]
    doc = page.evaluate("() => window.__view.state.doc.toString()")
    # 快路径 op 的偏移断言：'X' 落点 = 块文本中该位置的码点序号
    op_x = evs[-1]["ops"][0] if evs and evs[-1]["ops"] else None
    cp_check = page.evaluate("""() => {
      const doc = window.__view.state.doc.toString();
      const i = doc.indexOf('for typing.');
      const line = doc.slice(doc.lastIndexOf('\\n', i) + 1, i);
      return {cpBeforeX: [...line].length - 1, lineTail: line.slice(-6)};
    }""")
    block_match = page.evaluate("""() => {
      const doc = window.__view.state.doc.toString();
      const blocks = window.__blocks();
      const para = blocks.find((b) => b.type === 'paragraph' && b.text.includes('🎉'));
      const i = doc.indexOf('🎉');
      const nl = doc.indexOf('\\n', i);
      const line = doc.slice(doc.lastIndexOf('\\n', i) + 1, nl === -1 ? doc.length : nl);
      return {blockText: para ? para.text : null, docLine: line,
              cpEquals: para ? para.text === line : false};
    }""")
    u = page.evaluate("""() => {
      const undoLen = JSON.parse(window.__core.history_json()).undo.length;
      window.__bridge.undo();
      const d = window.__view.state.doc.toString();
      return {undoLen, gone: !d.includes('🎉'), docIsMd: d === window.__md(),
              intact: d.includes('Bridge Demo')};
    }""")
    checks = [
        check("non-BMP preedit+commit absorbed (🎉🚀 in doc, view==core at commit)",
              "🎉🚀" in doc and steps[-2]["docIsMd"],
              f"doc={steps[-2]['docAround']} docIsMd={steps[-2]['docIsMd']}"),
        check("W13 constitution: blocks_json text == doc line (code-point identity under real IME)",
              block_match["cpEquals"] and "🎉" in (block_match["blockText"] or ""),
              json.dumps(block_match, ensure_ascii=False)),
        check("fast-path op right after emoji uses code-point offsets (X at cp offset)",
              op_x is not None and op_x["t"] == "replace_text"
              and op_x["start"] == op_x["end"] == cp_check["cpBeforeX"],
              f"op={json.dumps(op_x, ensure_ascii=False)} cpBeforeX={cp_check['cpBeforeX']} lineTail={cp_check['lineTail']}"),
        check("emoji session + following keystroke = one merged undo entry",
              u["undoLen"] == 1,
              f"undoLen={u['undoLen']} evs={json.dumps([(e['cmd'], e['coalesced']) for e in evs], ensure_ascii=False)}"),
        check("one undo reverts emoji session entirely, view==core",
              u["gone"] and u["docIsMd"] and u["intact"],
              json.dumps(u, ensure_ascii=False)),
    ]
    return {"checks": checks, "steps": steps}


def ic6_click_force_commit(page, cdp):
    """组合中点击别处：真实 IME 下焦点转移会强制提交（人工协议第 4 项）。
    CDP 虚拟组合实测：CDP 鼠标事件不触发 blur/IME 线程事件，组合保持活跃
    ——本 IC 按实测行为断言（差异发现：CDP 路径无法复现强制提交，
    记为路径限制），并验证组合保持期间 view==core 一致 + 显式提交收尾。"""
    place_cursor(page)
    cdp.send("Input.imeSetComposition", {"text": "com", "selectionStart": 3, "selectionEnd": 3})
    time.sleep(0.15)
    pre = page.evaluate(SNAP)
    # 点击另一段（Bridge Demo 标题附近），产生真实 mousedown/mouseup
    box = page.evaluate("""() => {
      const el = document.querySelector('.cm-content');
      const r = el.getBoundingClientRect();
      return {x: r.left + 20, y: r.top + 4};
    }""")
    cdp.send("Input.dispatchMouseEvent", {"type": "mousePressed", "x": box["x"], "y": box["y"],
                                          "button": "left", "clickCount": 1})
    cdp.send("Input.dispatchMouseEvent", {"type": "mouseReleased", "x": box["x"], "y": box["y"],
                                          "button": "left", "clickCount": 1})
    post = snap(page, "click elsewhere during composition", wait=0.25)

    log = pre["log"] + post["log"]
    doc = page.evaluate("() => window.__view.state.doc.toString()")
    occurrences = doc.count("com")
    no_end = sum(1 for x in log if '"compositionend"' in x) == 0
    # 显式提交收尾（insertText while composing → compositionend）
    cdp.send("Input.insertText", {"text": "com"})
    fin = snap(page, "explicit commit after click", wait=0.2)
    doc2 = page.evaluate("() => window.__view.state.doc.toString()")
    fin_end = [x for x in fin["log"] if '"compositionend"' in x]
    u = page.evaluate("""() => {
      window.__bridge.undo();
      const d = window.__view.state.doc.toString();
      return {gone: !d.includes('com'), docIsMd: d === window.__md()};
    }""")
    checks = [
        check("CDP path limitation: mouse click during virtual composition does NOT force-commit "
              "(composing stays true, no compositionend; real-IME force-commit stays a manual item)",
              post["composing"] and no_end,
              f"composing={post['composing']} log={log}"),
        check("while composition persists after click: preedit retained exactly once, view==core",
              occurrences == 1 and post["docIsMd"] and post["docAround"].endswith("comfor typing."),
              f"occurrences={occurrences} docAround={post['docAround']} docIsMd={post['docIsMd']}"),
        check("explicit commit after click finalizes: compositionend fired, no duplication, composing cleared",
              len(fin_end) == 1 and doc2.count("com") == 1 and not fin["composing"] and fin["docIsMd"],
              f"endEvt={fin_end} occurrences={doc2.count('com')} composing={fin['composing']}"),
        check("undo reverts the committed text (single entry, view==core)",
              u["gone"] and u["docIsMd"],
              json.dumps(u, ensure_ascii=False)),
    ]
    return {"checks": checks, "steps": [pre, post, fin]}


def diff_synthetic_baseline(page):
    """差异对照的合成路径基线：独立页面上用 W3 式合成事件走一遍组合，
    记录事件链差异（证据入 results_ime.json，对照表落 gates.md）。"""
    r = page.evaluate("""async () => {
      const v = window.__view;
      const content = document.querySelector('.cm-content');
      const pos = v.state.doc.toString().indexOf('quoted line');
      v.focus();
      v.dispatch({selection: {anchor: pos}});
      window.__evCursor = window.__mdcoreEvents.length;
      content.dispatchEvent(new CompositionEvent('compositionstart', {data: ''}));
      document.execCommand('insertText', false, 'IME测试');
      content.dispatchEvent(new CompositionEvent('compositionupdate', {data: 'IME测试'}));
      content.dispatchEvent(new CompositionEvent('compositionend'));
      await new Promise((res) => setTimeout(res, 120));
      const doc = v.state.doc.toString();
      const events = window.__mdcoreEvents.slice(window.__evCursor);
      return {
        log: window.__imeLog.slice().map((x) => JSON.stringify(x)),
        docAround: doc.slice(Math.max(0, doc.indexOf('quoted') - 2), doc.indexOf('quoted') + 18),
        ops: events.map((ev) => ({cmd: ev.meta && ev.meta.command,
          ops: (ev.ops || []).map((o) => ({t: o.t, start: o.start, end: o.end, ins: o.insert}))})),
        docIsMd: doc === window.__md(),
      };
    }""")
    return r


def main():
    os.chdir(WEB / "dist")
    httpd = ReusableTCPServer(("127.0.0.1", 0), QuietServer)
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    results = {"meta": {
        "browser": "chromium-headless (CDP Input.imeSetComposition / Input.insertText)",
        "runner": "playwright + CDP session",
        "page": "web/dist/index.html (CM6 <-> mdcore-wasm bridge)",
        "protocol_note": "chromium-1243: imeSetComposition takes `text`; commit = Input.insertText "
                         "while composing (auto compositionend); cancel = empty text",
        "semantics_note": "assertions follow the measured implementation semantics "
                          "(probe_ime_cdp.py), not the idealized buffering model",
    }, "tests": {}}
    failures = []

    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=["--js-flags=--expose-gc", "--enable-precise-memory-info"])
        try:
            # IC1/IC2 共页（IC1 的 undo 已把页面复原到基线，IC2 可复用）；
            # IC3-IC6 各自新页隔离（组合流对页面历史敏感，沿用 W3/W9 惯例）。
            page, errors = open_page(browser, port)
            cdp = page.context.new_cdp_session(page)
            page.evaluate(INSTALL_LOG)
            results["tests"]["IC1"] = ic1_pinyin_commit(page, cdp)
            results["tests"]["IC2"] = ic2_multichar_word(page, cdp)
            page.close()

            for name, fn in (("IC3", ic3_crossline_commit), ("IC4", ic4_cancel),
                             ("IC5", ic5_emoji_nonbmp), ("IC6", ic6_click_force_commit)):
                page, errors2 = open_page(browser, port)
                errors.extend(errors2)
                cdp = page.context.new_cdp_session(page)
                page.evaluate(INSTALL_LOG)
                results["tests"][name] = fn(page, cdp)
                page.close()

            # 差异对照：合成路径基线（W3 式）在独立页面采集
            page, errors2 = open_page(browser, port)
            errors.extend(errors2)
            page.evaluate(INSTALL_LOG)
            results["tests"]["DIFF_baseline_synthetic"] = diff_synthetic_baseline(page)
            page.close()
        finally:
            browser.close()

    # 汇总判定
    for name, res in results["tests"].items():
        if "checks" not in res:
            continue
        for c in res["checks"]:
            if not c["pass"]:
                failures.append(f"{name}: {c['check']} ({c.get('detail', '')})")
    if errors:
        failures.append(f"pageerror: {errors}")

    results["summary"] = {"total": len([c for r in results["tests"].values()
                                        if "checks" in r for c in r["checks"]]),
                          "failed": len(failures), "pass": len(failures) == 0,
                          "failures": failures}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(results, indent=2, ensure_ascii=False))
    for name, res in results["tests"].items():
        if "checks" in res:
            for c in res["checks"]:
                print(f"{'PASS' if c['pass'] else 'FAIL'} {name}: {c['check']}"
                      + ("" if c["pass"] else f" -- {c.get('detail', '')[:220]}"))
    print(json.dumps(results["summary"], indent=2, ensure_ascii=False))
    print(f"\nfull results -> {OUT}")
    return 0 if not failures else 1


def open_page(browser, port):
    page = browser.new_page()
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(f"http://127.0.0.1:{port}/index.html")
    page.wait_for_function("window.__ready === true", timeout=60_000)
    return page, errors


if __name__ == "__main__":
    sys.exit(main())
