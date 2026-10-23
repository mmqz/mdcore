#!/usr/bin/env python3
"""阶段 6 §九.5 web 组合 e2e：并发修改、过期命令、撤销重做、删除与复原.

双页同源（?cc=1 / ?cc=2 槽位互异——BroadcastChannel 跨页通道 +
WasmCollabSession CRDT 文档会话 + canvas main 传输钩）真实浏览器组
合路径，16 项检查（全部走既有探针面，零产品代码）：

  A. 并发修改（1-4）：双页 doc 双向收敛（同 block_id 同文本）/交错
     并发不同块/同块双端并发插入（首尾无冲突——CRDT 无丢失）/
     canvas 跨页传播（bring_to_front——后台页 rAF 节流纪律）。
  B. 过期命令（5-7）：doc 面 expect 内容保护（错 expect = typed
     ExpectMismatch 拒绝且页面不崩）/正确 expect 放行/workbench 面
     refresh_body(v2) 后旧视图提议 fail-closed（旧值 336.8 已不在
     新正文 P2——typed wb_claimed_value_not_in_paragraph）。
  C. 撤销重做（8-11）：origin 分栈本地性（p1 撤销不累 p2 的 ai 栈）/
     单页双 actor 装载→撤销→撤销→重做→重做全往返/撤销更新跨页
     传播/canvas 深度往返。
  D. 删除与复原（12-14）：doc 删后撤销同 block_id 复活（证据锚身份
     连续，双页一致）/canvas 删除撤销同 ID 同指纹/跨页删除不被创建
     端撤销复活（对端 remove 站住，双页收敛）。
  E. 收尾（15-16）：双页全程零 pageerror/collab 状态一致（errors
     空 + connected + 成员互见）。

结果 -> tests/results/results_concurrent_web.json（端口 8943）。
"""

import base64
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
FIXTURES = WEB / "tests" / "fixtures"
PORT = 8943
OUT = WEB / "tests" / "results" / "results_concurrent_web.json"

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({
    ".html": "text/html",
    ".js": "text/javascript",
    ".wasm": "application/wasm",
    ".mjs": "text/javascript",
})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(DIST), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True

# —— 探针组合 JS（测试内联助手——不触碰产品代码）—————————
JS_FIND_BLOCK = """(id) => {
  const walk = (bs) => {
    for (const b of bs) {
      if (b.block_id === id) return b;
      const hit = walk(b.children || []);
      if (hit) return hit;
    }
    return null;
  };
  return walk(window.__collabDoc());
}"""
JS_DOC_HAS_TEXT = ("(t) => JSON.stringify(window.__collabDoc())"
                   ".indexOf(JSON.stringify(t)) >= 0")


def collab_exec(page, actor, cmd):
    """__collabExec 包装：命令 dict -> flat JSON -> typed 回执。"""
    return page.evaluate("([a, c]) => window.__collabExec(a, c)",
                         [actor, json.dumps(cmd, ensure_ascii=False)])


def doc_snap(page):
    return page.evaluate("JSON.stringify(window.__collabDoc())")


def block_gone(page, bid):
    return page.evaluate(JS_FIND_BLOCK, bid) is None


def main() -> int:
    checks: list[dict] = []
    page_errors_1: list[str] = []
    page_errors_2: list[str] = []

    def check(name, ok, detail="", skip=False):
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip),
                       "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        print(tag + name + ("  | " + str(detail)[:160]
                            if detail and not ok and not skip else ""))
        return bool(ok)

    xlsx_b64 = base64.b64encode((FIXTURES / "evidence_sample.xlsx").read_bytes()).decode()
    body_b64 = base64.b64encode((FIXTURES / "evidence_body.docx").read_bytes()).decode()
    body_v2_b64 = base64.b64encode((FIXTURES / "evidence_body_v2.docx").read_bytes()).decode()

    browser = None
    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        try:
            with sync_playwright() as pw:
                browser = pw.chromium.launch()
                # 同 context 双页（同源 = BroadcastChannel 可达）；槽位互异
                # （撞号 = CRDT client_id 冲突）。
                p1 = browser.new_context().new_page()
                p2 = p1.context.new_page()
                p1.on("pageerror", lambda e: page_errors_1.append(str(e)))
                p2.on("pageerror", lambda e: page_errors_2.append(str(e)))

                p1.goto(f"http://127.0.0.1:{PORT}/index.html?cc=1")
                p1.wait_for_function(
                    "window.__ready === true && window.__collabReady === true"
                    " && window.__canvasReady === true", timeout=60000)
                p2.goto(f"http://127.0.0.1:{PORT}/index.html?cc=2")
                p2.wait_for_function(
                    "window.__ready === true && window.__collabReady === true"
                    " && window.__canvasReady === true", timeout=60000)
                # canvas 传输钩（跨页 canvas_update 外送前提——测试 4 前置）
                p1.wait_for_function("window.__collabState().canvasHooked === true",
                                     timeout=30000)
                p2.wait_for_function("window.__collabState().canvasHooked === true",
                                     timeout=30000)

                def wait_until(pred, timeout=15.0, interval=0.4):
                    deadline = time.time() + timeout
                    while True:
                        try:
                            out = pred()
                        except Exception:
                            out = None
                        if out:
                            return out
                        if time.time() >= deadline:
                            return None
                        time.sleep(interval)

                def wait_converged(timeout=15.0, expect=None):
                    """双页 doc JSON 全等（block_id/kind/attrs/text/marks/children）。"""
                    def snap():
                        a = doc_snap(p1)
                        if a != doc_snap(p2):
                            return None
                        if expect is not None and not expect(a):
                            return None
                        return a
                    return wait_until(snap, timeout=timeout)

                # ── A. 并发修改 ─────────────────────────────────────
                # 1. 双页 doc 双向收敛：p1 human 装载 + p2 ai 改写
                load_cmd = {"t": "load_blocks", "blocks": [
                    {"kind": "p", "attrs": {}, "text": "Alpha base paragraph."},
                    {"kind": "p", "attrs": {}, "text": "Beta base paragraph."},
                    {"kind": "p", "attrs": {}, "text": "Gamma base paragraph."},
                ]}
                r_load = collab_exec(p1, "human", load_cmd)
                ids = (r_load.get("receipt") or {}).get("ids") if r_load.get("ok") else None
                p2_seen = wait_until(lambda: p2.evaluate(
                    "() => window.__collabDoc().length === 3"
                    " && window.__collabDoc().every(b => b.text.endsWith('base paragraph.'))"
                ) or None)
                r_edit = (collab_exec(p2, "ai", {"t": "replace_text", "block_id": ids[1],
                                                 "start": 0, "end": 4, "insert": "BETA"})
                          if ids else None)
                conv1 = wait_converged(
                    expect=lambda a: '"BETA base paragraph."' in a
                    and '"Alpha base paragraph."' in a) if r_edit else None
                texts1 = [b["text"] for b in json.loads(conv1)] if conv1 else []
                check("dual_tab_doc_bidirectional_convergence",
                      r_load.get("ok") is True and ids is not None and len(ids) == 3
                      and p2_seen is not None and (r_edit or {}).get("ok") is True
                      and conv1 is not None
                      and texts1 == ["Alpha base paragraph.", "BETA base paragraph.",
                                     "Gamma base paragraph."]
                      and all(f'"{i}"' in conv1 for i in ids),
                      json.dumps({"load": r_load.get("ok"), "ids": ids,
                                  "p2_seen": p2_seen is not None,
                                  "edit": r_edit, "texts": texts1},
                                 ensure_ascii=False)[:400])

                # 2. 交错并发（不同块，无同步间隔连发——p1/p2 轮转）
                if ids:
                    collab_exec(p1, "human", {"t": "replace_text", "block_id": ids[0],
                                              "start": 0, "end": 5, "insert": "alphaX"})
                    collab_exec(p2, "ai", {"t": "replace_text", "block_id": ids[2],
                                           "start": 0, "end": 5, "insert": "gammaX"})
                    collab_exec(p1, "human", {"t": "replace_text", "block_id": ids[0],
                                              "start": 0, "end": 6, "insert": "alphaY"})
                    collab_exec(p2, "ai", {"t": "replace_text", "block_id": ids[2],
                                           "start": 0, "end": 6, "insert": "gammaY"})
                conv2 = wait_converged(
                    expect=lambda a: '"alphaY base paragraph."' in a
                    and '"gammaY base paragraph."' in a) if ids else None
                texts2 = [b["text"] for b in json.loads(conv2)] if conv2 else []
                check("concurrent_interleaved_edits",
                      conv2 is not None
                      and texts2 == ["alphaY base paragraph.", "BETA base paragraph.",
                                     "gammaY base paragraph."],
                      json.dumps({"texts": texts2}, ensure_ascii=False))

                # 3. 同块并发双插：p1 首部 + p2 尾部（各自当下视图定位——
                #    单 evaluate 内读长+执行，窗口最小化）
                r_p1_ins = collab_exec(p1, "human", {
                    "t": "replace_text", "block_id": ids[1],
                    "start": 0, "end": 0, "insert": "[P1]"}) if ids else None
                r_p2_ins = p2.evaluate(
                    """(bid) => {
                      const walk = (bs) => { for (const b of bs) {
                        if (b.block_id === bid) return b;
                        const hit = walk(b.children || []); if (hit) return hit; }
                        return null; };
                      const b = walk(window.__collabDoc());
                      if (!b) return {ok: false, code: 'block_missing'};
                      return window.__collabExec('ai', JSON.stringify({
                        t: 'replace_text', block_id: bid,
                        start: b.text.length, end: b.text.length, insert: '[P2]'}));
                    }""", ids[1]) if ids else None
                conv3 = wait_converged(
                    expect=lambda a: "[P1]BETA base paragraph.[P2]" in a) if ids else None
                texts3 = [b["text"] for b in json.loads(conv3)] if conv3 else []
                check("concurrent_same_block_edits",
                      (r_p1_ins or {}).get("ok") is True and (r_p2_ins or {}).get("ok") is True
                      and conv3 is not None
                      and texts3 == ["alphaY base paragraph.",
                                     "[P1]BETA base paragraph.[P2]",
                                     "gammaY base paragraph."],
                      json.dumps({"p1": r_p1_ins, "p2": r_p2_ins, "texts": texts3},
                                 ensure_ascii=False))

                # 4. canvas 跨页传播（后台页 rAF 节流——先 bring_to_front）
                p1.bring_to_front()
                p1.click("#tab-canvas")
                r_cn = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                   [60, 80, "CW-Node"])
                cn_id = r_cn.get("id") if (r_cn or {}).get("ok") else None
                r_mv = p1.evaluate("([i, x, y]) => window.__canvasMoveNode(i, x, y)",
                                   [cn_id, 160, 140]) if cn_id else None
                p2_node = wait_until(lambda: (lambda o: o if (o and o.get("x") == 160
                                                              and o.get("y") == 140)
                                              else None)(
                    p2.evaluate("([i]) => window.__canvasObjects('main')"
                                ".find(o => o.id === i) || null", [cn_id]))) if cn_id else None
                p2_sync = p2.evaluate("window.__canvasSyncState()")
                check("canvas_cross_page_propagation",
                      (r_cn or {}).get("ok") is True and cn_id
                      and (r_mv or {}).get("ok") is True
                      and p2_node is not None
                      and p2_node.get("props", {}).get("label") == "CW-Node"
                      and p2_sync.get("converged") is True,
                      json.dumps({"add": r_cn, "move": r_mv and r_mv.get("ok"),
                                  "p2_node": p2_node, "p2_sync": p2_sync},
                                 ensure_ascii=False)[:400])

                # ── B. 过期命令 ─────────────────────────────────────
                # 5. doc 面 expect 内容保护：错 expect = typed 拒绝 + 页面不崩
                p1.click("#tab-doc")
                p1.click("#editor .cm-content")
                p1.keyboard.type(" cw-stale-guard-3368")
                p1.wait_for_timeout(300)
                blk = p1.evaluate("""() => {
                    const b = window.__blocks().find(x => (x.text || '')
                        .includes('cw-stale-guard-3368'));
                    return b ? {id: b.id, text: b.text} : null;
                }""")
                if blk:
                    stale = p1.evaluate(
                        """(bid) => {
                          let out;
                          try {
                            window.__core.replace_text_at_anchor(
                              BigInt(bid), 0, 4, 'STALE', 'definitely-wrong-expect');
                            out = {threw: false};
                          } catch (err) {
                            out = {threw: true, msg: String(err)};
                          }
                          let parsed = null;
                          try { parsed = JSON.parse(out.msg || ''); } catch (e) {}
                          out.parsed = parsed;
                          out.rev = Number(window.__core.revision());
                          out.mdKept = window.__md().includes('cw-stale-guard-3368');
                          return out;
                        }""", blk["id"])
                else:
                    stale = None
                parsed_var = (stale or {}).get("parsed") or {}
                check("doc_expect_guard_rejects_stale_write",
                      blk is not None and stale is not None and stale.get("threw") is True
                      and isinstance(parsed_var, dict) and "Anchor" in parsed_var
                      and "ExpectMismatch" in json.dumps(parsed_var)
                      and stale.get("mdKept") is True and stale.get("rev", 0) >= 0,
                      json.dumps({"blk": blk and {"id": blk["id"],
                                                  "head": blk["text"][:24]},
                                  "stale": stale}, ensure_ascii=False)[:400])

                # 6. 正确 expect 放行（同一块同区间——新鲜视图写入成功）
                if blk:
                    fresh = p1.evaluate(
                        """(bid) => {
                          const b = window.__blocks().find(x => x.id === bid);
                          if (!b) return {ok: false, code: 'block_gone'};
                          const expect = b.text.slice(0, 4);
                          let out;
                          try {
                            const ret = window.__core.replace_text_at_anchor(
                              BigInt(bid), 0, 4, 'FRESH', expect);
                            out = {ok: true, ret: JSON.parse(ret), expect};
                          } catch (err) { out = {ok: false, msg: String(err)}; }
                          const after = window.__blocks().find(x => x.id === bid);
                          out.afterHead = after ? after.text.slice(0, 8) : null;
                          out.markerKept = after ? after.text.includes('cw-stale-guard-3368')
                                                 : false;
                          return out;
                        }""", blk["id"])
                else:
                    fresh = None
                check("doc_expect_guard_accepts_fresh_write",
                      (fresh or {}).get("ok") is True
                      and (fresh or {}).get("afterHead", "").startswith("FRESH")
                      and (fresh or {}).get("markerKept") is True,
                      json.dumps({"fresh": fresh}, ensure_ascii=False)[:400])

                # 7. workbench 面：refresh_body(v2) 后旧视图提议 fail-closed
                #    （v1 P2 含 336.8——新鲜提议过门；v2 P2 改为 337.0——
                #     旧视图命令 typed 拒绝）
                cw_e = lambda eid, batch: json.dumps({
                    "evidence_id": eid, "claim": "数量 336.8。", "claimed_value": "336.8",
                    "paragraph_index": 2, "source_doc_id": "src-1", "sheet": "Sheet1",
                    "coord": "C7", "relation": "direct_quote", "calc_ref": None,
                    "generation_batch": batch}, ensure_ascii=False)
                ev_start = p1.evaluate("b => window.__evStartBody(b)", body_b64)
                ev_ing = p1.evaluate("b => window.__evIngestSource(b)", xlsx_b64)
                ev_fresh = p1.evaluate("j => window.__evProposeBinding(j)",
                                       cw_e("cw-e1", "cw1"))
                ev_refresh = p1.evaluate("b => window.__evRefreshBody(b)", body_v2_b64)
                ev_stale = p1.evaluate("j => window.__evProposeBinding(j)",
                                       cw_e("cw-e2", "cw2"))
                ev_after = p1.evaluate("""() => ({
                    active: window.__evState().active,
                    evidence: window.__evEvidence().length,
                    cw1: (window.__evEvidence().find(e => e.evidence_id === 'cw-e1')
                           || {}).linkage,
                })""")
                check("workbench_stale_view_rejects_proposal",
                      ev_start.get("ok") is True and ev_ing.get("ok") is True
                      and ev_fresh.get("ok") is True
                      and ev_refresh.get("ok") is True
                      and ev_refresh.get("degraded", 0) >= 1
                      and ev_stale.get("ok") is False
                      and ev_stale.get("code") == "wb_claimed_value_not_in_paragraph"
                      and bool(ev_stale.get("message"))
                      and ev_after.get("active") is True
                      and ev_after.get("evidence") == 1
                      and ev_after.get("cw1") == "stale",
                      json.dumps({"start": ev_start.get("ok"), "ing": ev_ing.get("ok"),
                                  "fresh": ev_fresh.get("ok"),
                                  "refresh": ev_refresh, "stale": ev_stale,
                                  "after": ev_after}, ensure_ascii=False)[:400])

                # ── C. 撤销重做 ─────────────────────────────────────
                # 8. origin 分栈本地性：p1 撤销自己的 human 操作，p2 的
                #    ai 栈深（undo/redo）不受累
                d_ai_before = p2.evaluate("window.__collabDepth('ai')")
                r_loc = collab_exec(p1, "human", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "Locality probe block."}})
                loc_id = (r_loc.get("receipt") or {}).get("block_id") if r_loc.get("ok") else None
                loc_seen = wait_until(lambda: p2.evaluate(JS_FIND_BLOCK, loc_id)) if loc_id else None
                u8 = p1.evaluate("window.__collabUndo('human')")
                conv8 = wait_converged(
                    expect=lambda a: '"Locality probe block."' not in a) if loc_id else None
                d_ai_after = p2.evaluate("window.__collabDepth('ai')")
                check("undo_origin_locality",
                      r_loc.get("ok") is True and loc_id is not None
                      and loc_seen is not None and u8 is True
                      and conv8 is not None and block_gone(p1, loc_id)
                      and block_gone(p2, loc_id)
                      and d_ai_before.get("undo", 0) >= 1  # p2 确有 ai 历史（断言有牙）
                      and d_ai_after == d_ai_before,
                      json.dumps({"loc": r_loc.get("ok"), "seen": loc_seen is not None,
                                  "undo": u8, "conv": conv8 is not None,
                                  "ai_before": d_ai_before, "ai_after": d_ai_after}))

                # 9. 单页双 actor 往返：human 装载 + ai 装载 → 撤销 human
                #    （只撤 human 块）→ 撤销 ai → 双重做复原
                dh0 = p1.evaluate("window.__collabDepth('human')")
                da0 = p1.evaluate("window.__collabDepth('ai')")
                r_h9 = collab_exec(p1, "human", {"t": "load_blocks", "blocks": [
                    {"kind": "p", "attrs": {}, "text": "Human actor block nine."}]})
                r_a9 = collab_exec(p1, "ai", {"t": "load_blocks", "blocks": [
                    {"kind": "p", "attrs": {}, "text": "Ai actor block nine."}]})
                dh1 = p1.evaluate("window.__collabDepth('human')")
                da1 = p1.evaluate("window.__collabDepth('ai')")
                seen9 = wait_until(lambda: (p1.evaluate(JS_DOC_HAS_TEXT, "Human actor block nine.")
                                            and p1.evaluate(JS_DOC_HAS_TEXT,
                                                            "Ai actor block nine.")) or None)
                u_h = p1.evaluate("window.__collabUndo('human')")
                mid_h_gone = not p1.evaluate(JS_DOC_HAS_TEXT, "Human actor block nine.")
                mid_a_kept = p1.evaluate(JS_DOC_HAS_TEXT, "Ai actor block nine.")
                u_a = p1.evaluate("window.__collabUndo('ai')")
                mid_all_gone = (not p1.evaluate(JS_DOC_HAS_TEXT, "Ai actor block nine."))
                dh2 = p1.evaluate("window.__collabDepth('human')")
                da2 = p1.evaluate("window.__collabDepth('ai')")
                rd_h = p1.evaluate("window.__collabRedo('human')")
                back_h = p1.evaluate(JS_DOC_HAS_TEXT, "Human actor block nine.")
                rd_a = p1.evaluate("window.__collabRedo('ai')")
                back_a = p1.evaluate(JS_DOC_HAS_TEXT, "Ai actor block nine.")
                conv9 = wait_converged(
                    expect=lambda a: '"Human actor block nine."' in a
                    and '"Ai actor block nine."' in a)
                check("undo_redo_roundtrip_both_actors",
                      r_h9.get("ok") is True and r_a9.get("ok") is True
                      and seen9 is not None
                      and dh1.get("undo") == dh0.get("undo", -2) + 1
                      and da1.get("undo") == da0.get("undo", -2) + 1
                      and u_h is True and mid_h_gone and mid_a_kept
                      and u_a is True and mid_all_gone
                      and dh2.get("undo") == dh0.get("undo", -2)
                      and da2.get("undo") == da0.get("undo", -2)
                      and rd_h is True and back_h
                      and rd_a is True and back_a
                      and conv9 is not None,
                      json.dumps({"loads": [r_h9.get("ok"), r_a9.get("ok")],
                                  "dh": [dh0.get("undo"), dh1.get("undo"), dh2.get("undo")],
                                  "da": [da0.get("undo"), da1.get("undo"), da2.get("undo")],
                                  "u": [u_h, u_a], "rd": [rd_h, rd_a],
                                  "mid": [mid_h_gone, mid_a_kept, mid_all_gone],
                                  "back": [back_h, back_a],
                                  "conv": conv9 is not None}))

                # 10. 撤销跨页传播：p1 插入 → p2 看见 → p1 撤销 → p2 见移除
                r_p10 = collab_exec(p1, "human", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "Propagate probe block."}})
                pid = (r_p10.get("receipt") or {}).get("block_id") if r_p10.get("ok") else None
                p2_saw = wait_until(lambda: p2.evaluate(JS_FIND_BLOCK, pid)) if pid else None
                u10 = p1.evaluate("window.__collabUndo('human')")
                conv10 = wait_converged(
                    expect=lambda a: '"Propagate probe block."' not in a) if pid else None
                check("undo_propagates_cross_page",
                      r_p10.get("ok") is True and pid is not None
                      and p2_saw is not None and u10 is True
                      and conv10 is not None
                      and block_gone(p1, pid) and block_gone(p2, pid),
                      json.dumps({"ins": r_p10.get("ok"), "p2_saw": p2_saw is not None,
                                  "undo": u10, "conv": conv10 is not None,
                                  "gone": [block_gone(p1, pid), block_gone(p2, pid)]
                                  if pid else None}))

                # 11. canvas 深度往返：加节点 → 撤销（深度/对象数回退）→ 重做
                p1.bring_to_front()
                st0 = p1.evaluate("window.__canvasState()")
                r_c11 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                    [540, 320, "CW-Depth"])
                c11_id = r_c11.get("id") if (r_c11 or {}).get("ok") else None
                st1 = p1.evaluate("window.__canvasState()")
                u11 = p1.evaluate("window.__canvasUndo()")
                st2 = p1.evaluate("window.__canvasState()")
                rd11 = p1.evaluate("window.__canvasRedo()")
                st3 = p1.evaluate("window.__canvasState()")
                check("canvas_undo_redo_depths",
                      (r_c11 or {}).get("ok") is True and c11_id
                      and st1.get("objects") == st0.get("objects", -9) + 1
                      and st1.get("undoDepth") == st0.get("undoDepth", -9) + 1
                      and u11 is True
                      and st2.get("objects") == st0.get("objects", -9)
                      and st2.get("undoDepth") == st0.get("undoDepth", -9)
                      and st2.get("redoDepth") == st0.get("redoDepth", -9) + 1
                      and rd11 is True
                      and st3.get("objects") == st0.get("objects", -9) + 1
                      and st3.get("undoDepth") == st0.get("undoDepth", -9) + 1
                      and st3.get("redoDepth") == st0.get("redoDepth", -9),
                      json.dumps({"st0": st0, "st1": st1, "st2": st2, "st3": st3,
                                  "u": u11, "rd": rd11}))

                # ── D. 删除与复原 ───────────────────────────────────
                # 12. doc 删后撤销同身份复活：同 block_id 同文本，双页一致
                r_i12 = collab_exec(p1, "human", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "Identity keep 42"}})
                bid12 = (r_i12.get("receipt") or {}).get("block_id") if r_i12.get("ok") else None
                seen12 = wait_until(lambda: p2.evaluate(JS_FIND_BLOCK, bid12)) if bid12 else None
                r_r12 = collab_exec(p1, "human", {"t": "remove_block", "block_id": bid12})
                gone12 = wait_until(lambda: (True if block_gone(p1, bid12)
                                             and block_gone(p2, bid12) else None)) if bid12 else None
                u12 = p1.evaluate("window.__collabUndo('human')")
                back12 = wait_until(lambda: (lambda a, b: a and b
                                             and a.get("text") == "Identity keep 42"
                                             and b.get("text") == "Identity keep 42"
                                             and {"p1": a, "p2": b} or None)(
                    p1.evaluate(JS_FIND_BLOCK, bid12),
                    p2.evaluate(JS_FIND_BLOCK, bid12))) if bid12 else None
                check("doc_delete_then_undo_same_identity",
                      r_i12.get("ok") is True and bid12 is not None
                      and seen12 is not None and (r_r12 or {}).get("ok") is True
                      and gone12 is not None and u12 is True
                      and back12 is not None
                      and back12.get("p1", {}).get("block_id") == bid12
                      and back12.get("p2", {}).get("block_id") == bid12,
                      json.dumps({"ins": r_i12.get("ok"), "bid": bid12,
                                  "seen_p2": seen12 is not None,
                                  "rm": r_r12, "gone": gone12 is not None,
                                  "undo": u12, "back": back12}, ensure_ascii=False)[:400])

                # 13. canvas 删除撤销：同 ID 回归 + 指纹不变（证据锚连续性）
                p1.bring_to_front()
                r_c13 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                   [640, 200, "CW-Fingerprint"])
                c13_id = r_c13.get("id") if (r_c13 or {}).get("ok") else None
                fp1 = p1.evaluate("id => window.__canvasFingerprint(id)", c13_id) if c13_id else None
                r_rm13 = p1.evaluate(
                    "([i]) => window.__canvasExecute('human',"
                    " JSON.stringify({t: 'remove_object', id: i}))", [c13_id]) if c13_id else None
                gone13 = p1.evaluate(
                    "([i]) => !window.__canvasObjects('main').some(o => o.id === i)",
                    [c13_id]) if c13_id else None
                u13 = p1.evaluate("window.__canvasUndo()")
                back13 = p1.evaluate(
                    "([i]) => window.__canvasObjects('main').find(o => o.id === i) || null",
                    [c13_id]) if c13_id else None
                fp2 = p1.evaluate("id => window.__canvasFingerprint(id)", c13_id) if c13_id else None
                check("canvas_delete_restore_fingerprint",
                      (r_c13 or {}).get("ok") is True and c13_id
                      and (fp1 or {}).get("ok") is True
                      and str((fp1 or {}).get("fp", "")).startswith("fnv64:")
                      and (r_rm13 or {}).get("ok") is True
                      and (r_rm13 or {}).get("receipt", {}).get("outcome") == "object_removed"
                      and gone13 is True and u13 is True
                      and back13 is not None and back13.get("id") == c13_id
                      and (fp2 or {}).get("fp") == (fp1 or {}).get("fp"),
                      json.dumps({"add": r_c13, "fp1": fp1, "rm": r_rm13,
                                  "gone": gone13, "undo": u13,
                                  "back": back13 and {"id": back13.get("id"),
                                                      "label": back13.get("props", {}).get("label")},
                                  "fp2": fp2}, ensure_ascii=False)[:400])

                # 14. 跨页删除：p2 删 p1 的块 → p1 撤销自己的插入不复活
                #     （对端 remove 站住；双页收敛）
                r_i14 = collab_exec(p1, "human", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "Cross page del probe"}})
                bid14 = (r_i14.get("receipt") or {}).get("block_id") if r_i14.get("ok") else None
                seen14 = wait_until(lambda: p2.evaluate(JS_FIND_BLOCK, bid14)) if bid14 else None
                r_rm14 = collab_exec(p2, "ai", {"t": "remove_block", "block_id": bid14})
                gone_p1_14 = wait_until(lambda: (True if block_gone(p1, bid14)
                                                 else None)) if bid14 else None
                u14 = p1.evaluate("window.__collabUndo('human')")
                conv14 = wait_converged(
                    expect=lambda a: '"Cross page del probe"' not in a) if bid14 else None
                still_gone = block_gone(p1, bid14) and block_gone(p2, bid14) if bid14 else None
                check("cross_page_delete_restore",
                      r_i14.get("ok") is True and bid14 is not None
                      and seen14 is not None and (r_rm14 or {}).get("ok") is True
                      and gone_p1_14 is not None and u14 is True
                      and conv14 is not None and still_gone is True,
                      json.dumps({"ins": r_i14.get("ok"), "bid": bid14,
                                  "p2_saw": seen14 is not None,
                                  "rm": r_rm14, "p1_saw_delete": gone_p1_14 is not None,
                                  "p1_undo": u14, "conv": conv14 is not None,
                                  "still_gone": still_gone}))

                # ── E. 收尾 ─────────────────────────────────────────
                # 15. 双页全程零 pageerror
                check("zero_pageerror_all_pages",
                      len(page_errors_1) == 0 and len(page_errors_2) == 0,
                      json.dumps({"p1": page_errors_1[:3], "p2": page_errors_2[:3]},
                                 ensure_ascii=False))

                # 16. collab 状态一致：零错误 + connected + 成员互见
                s1 = p1.evaluate("window.__collabState()")
                s2 = p2.evaluate("window.__collabState()")
                check("collab_state_consistency",
                      s1.get("connected") is True and s2.get("connected") is True
                      and s1.get("errors") == [] and s2.get("errors") == []
                      and 2 in (s1.get("members") or [])   # p1 见 p2（slot 2）
                      and 1 in (s2.get("members") or [])   # p2 见 p1（slot 1）
                      and s1.get("docBlocks", -1) >= 1 and s2.get("docBlocks", -1) >= 1,
                      json.dumps({"p1": s1, "p2": s2}, ensure_ascii=False)[:400])

                browser.close()
                browser = None
        finally:
            if browser is not None:
                try:
                    browser.close()
                except Exception:
                    pass
        srv.shutdown()

    failed = [c for c in checks if not c["ok"] and not c["skip"]]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps({
        "meta": {"gate": "stage 6 §九.5 concurrent web composition e2e",
                 "dist": str(DIST), "port": PORT, "total": len(checks),
                 "failed": len(failed)},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed),
                    "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
