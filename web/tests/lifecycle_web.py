#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""阶段 6 §九.8 web 组合 e2e：应用退出、重启、断连和恢复（lifecycle）.

web face 的生命周期语义映射（对等原生证据的客户端侧）：
「应用退出/重启」= 页面 reload（挥发性面清零）+ 宿主工件恢复
（__inject 契约 / __core.snapshot→constructor.restore / evidence
sidecar 重供）；「断连」= BroadcastChannel 协同通道关闭；「恢复」=
snapshot/sidecar 重放 + 状态向量 catch-up（sync_req/sync_resp 差量）。
原生中继崩溃恢复已在 crates/relay/examples/relay_demo.rs（13 检查点）
钉死——本套件只覆盖 web 客户端侧，不重复原生面。

单浏览器 ≤3 页（同 context：p1 ?cc=1 / p2 ?cc=2 / p3 ?cc=3 槽位互异），
15 项检查（全部走既有探针面，零产品代码）：

  A. 退出与重启（1-5）：doc 退出→__inject 工件恢复+复活编辑 /
     history_json 工件恢复→undo/redo 往返 / snapshot→constructor.restore
     静态恢复（wasm 出口） / evidence 8 步会话 sidecar 跨 reload 全量
     连续 / canvas 双 page-life 快照往返（裸快照字节不可导出——差异
     记录在 detail）。
  B. 重启对与恢复（6-7）：协同对双页重启=挥发性面归零+单侧重播收敛 /
     collab 会话 authoritative_snapshot→restore（同槽 client id、
     撤销栈不随快照走——B3.1）。
  C. 断连与恢复（8-12）：断连冻结对端视图（doc+canvas 双面缺席 +
     对端健康）/ 重连状态向量 catch-up 收敛（doc 全等 + canvas 对象
     计数全等）/ 断连期双侧分叉编辑→CRDT 无冲突合流（双端各自行
     disconnect/reconnect 周期后双页全等且含双方内容）/ 3 轮断连-
     重连循环仍收敛零错误 / canvas 断连-重连（bring_to_front 纪律）。
  D. 收尾（13-15）：晚加入第三页——被动 catch-up 不投递既有状态
     （实测行为记录）+ 显式 disconnect/reconnect 状态向量同步全量
     收敛 / 三页状态一致（connected+errors[]+成员互见+docBlocks 全等）/
     全程零 pageerror（含全部 reload 与断连）。

结果 -> tests/results/results_lifecycle_web.json（端口 8946）。
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
PORT = 8946
OUT = WEB / "tests" / "results" / "results_lifecycle_web.json"

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

# —— 探针组合 JS（测试内联助手——不触碰产品代码）—————————————
JS_DOC_HAS = "m => JSON.stringify(window.__collabDoc()).indexOf(m) >= 0"
JS_CV_HAS_LABEL = ("l => window.__canvasObjects('main')"
                   ".some(o => o.props && o.props.label === l) || false")
JS_CV_BY_ID = ("i => window.__canvasObjects('main').find(o => o.id === i) || null")

# evidence 面 proposal JSON（workbench 契约——形状取自通过的
# tests/sidecar_faults_web.py；e-3 为恢复后「会话仍活」探针）。
E_QUOTE = json.dumps({
    "evidence_id": "e-1", "claim": "单价 12.5 元", "claimed_value": "12.5",
    "paragraph_index": 2, "source_doc_id": "src-1", "sheet": "Sheet1",
    "coord": "B7", "relation": "direct_quote", "calc_ref": None,
    "generation_batch": "g1",
}, ensure_ascii=False)
E_CALC = json.dumps({
    "evidence_id": "e-2", "claim": "小计 4210.000 元", "claimed_value": "4210.000",
    "paragraph_index": 3, "source_doc_id": "src-1", "sheet": "Sheet1",
    "coord": "B7", "relation": "calculated", "calc_ref": "calc-1",
    "generation_batch": "g1",
}, ensure_ascii=False)
E_3_NEW = json.dumps({
    "evidence_id": "e-3", "claim": "单价 12.5 元", "claimed_value": "12.5",
    "paragraph_index": 2, "source_doc_id": "src-1", "sheet": "Sheet1",
    "coord": "B7", "relation": "direct_quote", "calc_ref": None,
    "generation_batch": "g2",
}, ensure_ascii=False)
CALC_1 = json.dumps({
    "calc_id": "calc-1", "expression": "qty * price", "unit": None,
    "precision": {"target_scale": 3, "rounding": "half_up"},
    "inputs": [
        {"name": "qty", "source": {"cell": {"doc_id": "src-1", "sheet": "Sheet1", "coord": "B7"}}, "unit": None},
        {"name": "price", "source": {"cell": {"doc_id": "src-1", "sheet": "Sheet1", "coord": "C7"}}, "unit": None},
    ],
}, ensure_ascii=False)

# doc 面确定性单 op 编辑（View 源——可撤销；wasm 类方法经实例可达）
JS_DOC_REPLACE = """(marker) => {
  const blocks = window.__blocks();
  const b = blocks.find(x => x.type === 'paragraph' && (x.text || '').length > 5)
    || blocks[0];
  if (!b) return {ok: false, code: 'no_block'};
  window.__core.set_clock_ms(BigInt(Date.now()));
  try {
    const ret = window.__core.replace_text_at_anchor(
      BigInt(b.id), 0, 0, marker, null);
    return {ok: true, ret: JSON.parse(ret), block: Number(b.id)};
  } catch (err) { return {ok: false, msg: String(err)}; }
}"""


def main() -> int:
    missing = [p for p in ("evidence_body.docx", "evidence_sample.xlsx")
               if not (FIXTURES / p).exists()]
    if missing:
        print(f"FATAL fixture missing {missing}")
        return 2

    checks: list[dict] = []
    page_errors: dict[str, list[str]] = {"p1": [], "p2": [], "p3": []}

    def check(name, ok, detail="", skip=False):
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip),
                       "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        print(tag + name + ("  | " + str(detail)[:160]
                            if detail and not ok and not skip else ""))
        return bool(ok)

    def wait_until(pred, timeout=15.0, interval=0.35):
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

    def stays_absent(pred, timeout=1.2, interval=0.25):
        """断言谓词在窗口内始终为假（断连冻结面的「缺席」证据）。"""
        deadline = time.time() + timeout
        while True:
            try:
                if pred():
                    return False
            except Exception:
                pass
            if time.time() >= deadline:
                return True
            time.sleep(interval)

    def collab_exec(page, actor, cmd):
        return page.evaluate("([a, c]) => window.__collabExec(a, c)",
                             [actor, json.dumps(cmd, ensure_ascii=False)])

    def doc_snap(page):
        return page.evaluate("JSON.stringify(window.__collabDoc())")

    def wait_converged(expect=None, timeout=15.0):
        def snap():
            a = doc_snap(p1)
            if a != doc_snap(p2):
                return None
            if expect is not None and not expect(a):
                return None
            return a
        return wait_until(snap, timeout=timeout)

    def reload_and_wait(page):
        """「应用退出与重启」= reload；就绪旗全量重等后再探针。"""
        page.reload()
        page.wait_for_function(
            "window.__ready === true && window.__collabReady === true"
            " && window.__canvasReady === true", timeout=60000)
        page.wait_for_function("window.__evReady === true", timeout=60000)
        page.wait_for_function("window.__collabState().canvasHooked === true",
                               timeout=30000)

    def canvas_drive(page):
        """canvas 驱动纪律：bring_to_front + 切 canvas tab。"""
        page.bring_to_front()
        page.click("#tab-canvas")

    xlsx_b64 = base64.b64encode((FIXTURES / "evidence_sample.xlsx").read_bytes()).decode()
    body_b64 = base64.b64encode((FIXTURES / "evidence_body.docx").read_bytes()).decode()

    browser = None
    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        try:
            with sync_playwright() as pw:
                browser = pw.chromium.launch()
                # 同 context 双页（BroadcastChannel 同源可达）；槽位互异
                p1 = browser.new_context().new_page()
                p2 = p1.context.new_page()
                p1.on("pageerror", lambda e: page_errors["p1"].append(str(e)))
                p2.on("pageerror", lambda e: page_errors["p2"].append(str(e)))

                p1.goto(f"http://127.0.0.1:{PORT}/index.html?cc=1")
                p1.wait_for_function(
                    "window.__ready === true && window.__collabReady === true"
                    " && window.__canvasReady === true", timeout=60000)
                p1.wait_for_function("window.__evReady === true", timeout=60000)
                p1.wait_for_function("window.__collabState().canvasHooked === true",
                                     timeout=30000)
                p2.goto(f"http://127.0.0.1:{PORT}/index.html?cc=2")
                p2.wait_for_function(
                    "window.__ready === true && window.__collabReady === true"
                    " && window.__canvasReady === true", timeout=60000)
                p2.wait_for_function("window.__evReady === true", timeout=60000)
                p2.wait_for_function("window.__collabState().canvasHooked === true",
                                     timeout=30000)
                boot_md0 = p1.evaluate("window.__md()")
                boot_n0 = p1.evaluate("window.__blockCount()")

                # ══ A. 应用退出与重启（doc/evidence/canvas 工件恢复）═════

                # 1. doc 退出→重启→__inject 工件恢复（宿主全量打开契约）
                p1.bring_to_front()
                p1.click("#tab-doc")
                p1.click("#editor .cm-content")
                p1.keyboard.type("LC-EXIT-1 typed marker ")
                p1.wait_for_timeout(400)
                r_op1 = p1.evaluate(JS_DOC_REPLACE, "[LC-A1-OP] ")
                md_exit = p1.evaluate("window.__md()")
                n_exit = p1.evaluate("window.__blockCount()")
                rev_exit = p1.evaluate("Number(window.__core.revision())")
                snap_exit = p1.evaluate("window.__core.snapshot()")
                reload_and_wait(p1)
                fresh = p1.evaluate("() => ({md: window.__md(), n: window.__blockCount()})")
                p1.evaluate("md => { window.__inject(md); return true; }", md_exit)
                restored = p1.evaluate("() => ({md: window.__md(), n: window.__blockCount()})")
                rev_inject = p1.evaluate("Number(window.__core.revision())")
                r_reborn = p1.evaluate(JS_DOC_REPLACE, "[LC-A1-REBORN] ")
                md_reborn = p1.evaluate("window.__md()")
                rev_reborn = p1.evaluate("Number(window.__core.revision())")
                check("doc_exit_restart_via_inject",
                      r_op1.get("ok") is True
                      and "LC-EXIT-1 typed marker" in md_exit
                      and "[LC-A1-OP] " in md_exit
                      and "LC-EXIT-1" not in fresh["md"]
                      and fresh["md"] == boot_md0            # 重启=工厂态
                      and fresh["n"] == boot_n0
                      and restored["md"] == md_exit           # 工件恢复：md 全等
                      and restored["n"] == n_exit             # 块数全等
                      and r_reborn.get("ok") is True          # 恢复后仍可编辑
                      and "[LC-A1-REBORN] " in md_reborn
                      and rev_reborn > rev_inject,
                      json.dumps({"typed": "LC-EXIT-1 typed marker" in md_exit,
                                  "op": r_op1, "fresh_is_boot": fresh["md"] == boot_md0,
                                  "marker_gone": "LC-EXIT-1" not in fresh["md"],
                                  "fresh_blocks_eq_boot": fresh["n"] == boot_n0,
                                  "restored_eq": restored["md"] == md_exit,
                                  "blocks": [n_exit, restored["n"]],
                                  "rev": [rev_inject, rev_reborn],
                                  "reborn": r_reborn}, ensure_ascii=False)[:400])

                # 2. history 工件恢复：capture → reload → inject →
                #    set_history_json → undo/redo 往返（原生镜像
                #    crates/mdcore/tests/document_progressive.rs ③'）
                p1.wait_for_timeout(1500)  # COALESCE_MS=1000 外——独立条目
                md_before_h = p1.evaluate("window.__md()")
                r_h2 = p1.evaluate(JS_DOC_REPLACE, "[LC-H2] ")
                md_after_h = p1.evaluate("window.__md()")
                hj = p1.evaluate("window.__core.history_json()")
                hj_undo_n = len(json.loads(hj).get("undo", []))
                reload_and_wait(p1)
                p1.evaluate("md => { window.__inject(md); return true; }", md_after_h)
                set_h = p1.evaluate(
                    "j => { try { window.__core.set_history_json(j);"
                    " return {ok: true}; } catch (e) { return {ok: false,"
                    " err: String(e)}; } }", hj)
                undo_r = p1.evaluate(
                    """() => { try { const r = window.__core.undo();
                      return r ? {ok: true, rev: JSON.parse(r).revision}
                               : {ok: false}; } catch (e) {
                      return {ok: false, err: String(e)}; } }""")
                md_undone = p1.evaluate("window.__md()")
                redo_r = p1.evaluate(
                    """() => { try { const r = window.__core.redo();
                      return r ? {ok: true, rev: JSON.parse(r).revision}
                               : {ok: false}; } catch (e) {
                      return {ok: false, err: String(e)}; } }""")
                md_redone = p1.evaluate("window.__md()")
                check("doc_history_recovery",
                      r_h2.get("ok") is True and hj_undo_n >= 2
                      and set_h.get("ok") is True
                      and undo_r.get("ok") is True
                      and md_undone == md_before_h
                      and redo_r.get("ok") is True
                      and md_redone == md_after_h,
                      json.dumps({"edit": r_h2, "undo_entries": hj_undo_n,
                                  "set": set_h, "undo": undo_r,
                                  "undone_eq": md_undone == md_before_h,
                                  "redo": redo_r,
                                  "redone_eq": md_redone == md_after_h},
                                 ensure_ascii=False)[:400])

                # 3. snapshot→constructor.restore 静态恢复（wasm 出口经
                #    实例 constructor 可达；独立实例上恢复+复活编辑）
                snap_r = p1.evaluate("""(snap) => {
                  const out = {};
                  try {
                    const R = window.__core.constructor.restore(snap);
                    out.md = R.to_markdown();
                    out.blocks = R.blocks_count();
                    out.rev = Number(R.revision());
                    const blocks = JSON.parse(R.blocks_json());
                    const b = blocks.find(x => x.type === 'paragraph'
                      && (x.text || '').length > 5) || blocks[0];
                    R.set_clock_ms(BigInt(Date.now()));
                    const ret = R.replace_text_at_anchor(
                      BigInt(b.id), 0, 0, '[LC-SNAP-REBORN]', null);
                    out.edit = JSON.parse(ret);
                    out.mdAfter = R.to_markdown();
                    out.revAfter = Number(R.revision());
                    out.ok = true;
                  } catch (e) { out.ok = false; out.err = String(e); }
                  return out;
                }""", snap_exit)
                check("doc_snapshot_static_restore_survives_reload",
                      snap_r.get("ok") is True
                      and snap_r.get("md") == md_exit
                      and snap_r.get("blocks") == n_exit
                      and snap_r.get("rev") == rev_exit
                      and isinstance(snap_r.get("edit"), dict)
                      and snap_r["edit"].get("revision", 0) > snap_r.get("rev", -1)
                      and "[LC-SNAP-REBORN]" in snap_r.get("mdAfter", ""),
                      json.dumps({"restore": snap_r, "md_eq": snap_r.get("md") == md_exit,
                                  "blocks": [n_exit, snap_r.get("blocks")],
                                  "rev": [rev_exit, snap_r.get("rev")]},
                                 ensure_ascii=False)[:400])

                # 4. evidence 8 步会话 → 异常退出（reload）→ 宿主重供
                #    （body+source）→ sidecar 恢复 → 全量连续 + 会话仍活
                steps = {
                    "start": p1.evaluate("b => window.__evStartBody(b)", body_b64),
                    "src": p1.evaluate("b => window.__evIngestSource(b)", xlsx_b64),
                    "bind1": p1.evaluate("j => window.__evProposeBinding(j)", E_QUOTE),
                    "bind2": p1.evaluate("j => window.__evProposeBinding(j)", E_CALC),
                    "calc": p1.evaluate("j => window.__evProposeCalc(j)", CALC_1),
                    "recalc": p1.evaluate("c => window.__evRecalc(c)", "calc-1"),
                    "rev2": p1.evaluate("([i, d]) => window.__evReview(i, d)", ["e-2", "approved"]),
                    "rev1": p1.evaluate("([i, d]) => window.__evReview(i, d)", ["e-1", "rejected"]),
                }
                sc = p1.evaluate("window.__evSidecar()")
                sc_parsed = json.loads(sc)
                audit_before = len(sc_parsed.get("audit", []))
                audit_kinds = [a["kind"] for a in sc_parsed.get("audit", [])]
                reload_and_wait(p1)
                ev_gone = p1.evaluate(
                    "() => ({active: window.__evState().active,"
                    " evidence: window.__evState().evidence,"
                    " sidecar: window.__evSidecar()})")
                re_supplied = {
                    "body": p1.evaluate("b => window.__evStartBody(b)", body_b64),
                    "src": p1.evaluate("b => window.__evIngestSource(b)", xlsx_b64),
                }
                rr = p1.evaluate("j => window.__evRestoreSidecar(j)", sc)
                after = p1.evaluate("""() => ({
                    state: window.__evState(),
                    evidence: window.__evEvidence(),
                    audit: window.__evAudit(),
                })""")
                ev_reviews = {e["evidence_id"]: e["review"] for e in after["evidence"]}
                ev_linkage = {e["evidence_id"]: e["linkage"] for e in after["evidence"]}
                aseq = [a["seq"] for a in after["audit"]]
                rc = p1.evaluate("c => window.__evRecalc(c)", "calc-1")
                pb3 = p1.evaluate("j => window.__evProposeBinding(j)", E_3_NEW)
                final_counts = p1.evaluate("window.__evCounts()")
                check("evidence_exit_restart_full_recovery",
                      all(r.get("ok") is True for r in steps.values())
                      and ev_gone["active"] is False and ev_gone["evidence"] == 0
                      and ev_gone["sidecar"] == ""
                      and re_supplied["body"].get("ok") is True
                      and re_supplied["src"].get("ok") is True
                      and rr.get("ok") is True
                      and len(after["evidence"]) == 2
                      and ev_reviews == {"e-1": "rejected", "e-2": "approved"}
                      and ev_linkage == {"e-1": "valid", "e-2": "valid"}
                      and len(after["audit"]) == audit_before
                      and [a["kind"] for a in after["audit"]] == audit_kinds
                      and all(a < b for a, b in zip(aseq, aseq[1:]))
                      and after["state"]["active"] is True
                      and after["state"]["sources"] == ["src-1"]
                      and rc.get("ok") is True
                      and pb3.get("ok") is True and pb3.get("evidence_id") == "e-3"
                      and final_counts == {"pending": 1, "approved": 1,
                                           "rejected": 1, "stale": 0},
                      json.dumps({"steps_ok": all(r.get("ok") is True for r in steps.values()),
                                  "wiped": [ev_gone["active"], ev_gone["evidence"]],
                                  "restore": rr.get("ok"), "evidence": len(after["evidence"]),
                                  "reviews": ev_reviews, "linkage": ev_linkage,
                                  "audit": [audit_before, len(after["audit"])],
                                  "kinds_kept": [a["kind"] for a in after["audit"]] == audit_kinds,
                                  "live": [rc.get("ok"), pb3.get("ok")],
                                  "counts": final_counts}, ensure_ascii=False)[:400])

                # 5. canvas 双 page-life 快照往返（authoritative_snapshot
                #    裸字节无探针导出面——__canvasSnapshotRoundtrip 自包；
                #    差异记录：跨 reload canvas 恢复走 B12 的 sync 差量面）
                canvas_drive(p1)
                cn1 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                  [60, 80, "LC-C1"])
                cn2 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                  [220, 80, "LC-C2"])
                cn3 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                  [140, 220, "LC-C3"])
                r_edge = p1.evaluate(
                    """([a, b]) => window.__canvasExecute('human',
                        JSON.stringify({t: 'add_edge', id: null,
                          from: a, to: b, props: {}}))""",
                    [cn1.get("id"), cn2.get("id")]) if cn1.get("ok") and cn2.get("ok") else None
                st_life1 = p1.evaluate("window.__canvasState()")
                rt1 = p1.evaluate("window.__canvasSnapshotRoundtrip()")
                reload_and_wait(p1)
                # p6-t6 语义修正后：reload 页经 joinerCatchUp 从在场对端
                # （p2 仍持有全量）差量复活——「退出清零」仅在孤页成立；
                # 房间持状态（对端复活）= 跨 reload canvas 恢复的真路径。
                cv_revive = wait_until(lambda: p1.evaluate(
                    "window.__canvasState().objects") == 4, timeout=15)
                fresh_cv = p1.evaluate("window.__canvasState()")
                canvas_drive(p1)
                cr1 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                  [80, 90, "LC-R1"])
                cr2 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                  [240, 90, "LC-R2"])
                r_edge2 = p1.evaluate(
                    """([a, b]) => window.__canvasExecute('human',
                        JSON.stringify({t: 'add_edge', id: null,
                          from: a, to: b, props: {}}))""",
                    [cr1.get("id"), cr2.get("id")]) if cr1.get("ok") and cr2.get("ok") else None
                st_life2 = p1.evaluate("window.__canvasState()")
                rt2 = p1.evaluate("window.__canvasSnapshotRoundtrip()")
                check("canvas_snapshot_restore_survives_reload",
                      cn1.get("ok") is True and cn2.get("ok") is True
                      and cn3.get("ok") is True
                      and (r_edge or {}).get("ok") is True
                      and (r_edge or {}).get("receipt", {}).get("outcome") == "edge_added"
                      and st_life1.get("objects") == 4 and st_life1.get("edges") == 1
                      and rt1.get("ok") is True and len(rt1.get("objects", [])) == 4
                      and fresh_cv.get("objects") == 4 and cv_revive is not None
                      and cr1.get("ok") is True and cr2.get("ok") is True
                      and (r_edge2 or {}).get("ok") is True
                      and st_life2.get("objects") == 7 and st_life2.get("edges") == 2
                      and rt2.get("ok") is True and len(rt2.get("objects", [])) == 7,
                      json.dumps({"life1": {"state": st_life1, "rt": rt1.get("ok"),
                                            "objs": len(rt1.get("objects", []))},
                                  "fresh_after_reload": fresh_cv,
                                  "life2": {"state": st_life2, "rt": rt2.get("ok"),
                                            "objs": len(rt2.get("objects", []))},
                                  "revive_after_reload": fresh_cv.get("objects"),
                                  "gap": "raw snapshot bytes are not probe-exportable"
                                         " across reload (roundtrip face is self-contained);"
                                         " cross-reload canvas restore rides the collab"
                                         " sync_req/sync_resp diff face (peer resurrection,"
                                         " p6-t6 joinerCatchUp fix)"},
                                 ensure_ascii=False)[:400])

                # ══ B. 协同对重启 + collab 会话快照恢复 ═════════════════

                # 6. 协同对双页重启：挥发性面（doc 会话 + canvas）归零 →
                #    单侧重播 → 对端收敛（“重启后重同步”）
                #    注：同槽 reload 后 canvas 会话 client id 不变而 op
                #    计数器归零——新 op 与对端已存 op id 撞号被 CRDT 静默
                #    去重（实测行为，记录于 meta）；双页同时重启后撞号面
                #    一并清零，重播成立。
                pre_seed = collab_exec(p1, "human", {"t": "load_blocks", "blocks": [
                    {"kind": "p", "attrs": {}, "text": "LC pre-restart A."},
                    {"kind": "p", "attrs": {}, "text": "LC pre-restart B."},
                ]})
                pre_conv = wait_converged(timeout=10)
                pre_cv_counts = {
                    "p1": p1.evaluate("window.__canvasObjects('main').length"),
                    "p2": p2.evaluate("window.__canvasObjects('main').length"),
                }
                reload_and_wait(p2)
                reload_and_wait(p1)
                # p6-t6 语义修正后：双页先后 reload，后页从先复活页差量拉
                # 回全量——房间状态在成员重启间存活（对端复活面）。
                revive_pair = wait_until(lambda: (
                    p1.evaluate("() => window.__canvasState().objects") == pre_cv_counts["p1"]
                    and p2.evaluate("() => window.__canvasState().objects") == pre_cv_counts["p2"]
                    and p1.evaluate("() => window.__collabState().docBlocks") == 2
                    and p2.evaluate("() => window.__collabState().docBlocks") == 2), timeout=15)
                wiped = {
                    "p1": p1.evaluate("() => ({doc: window.__collabState().docBlocks,"
                                      " cv: window.__canvasState().objects})"),
                    "p2": p2.evaluate("() => ({doc: window.__collabState().docBlocks,"
                                      " cv: window.__canvasState().objects})"),
                }
                reseed = collab_exec(p1, "human", {"t": "load_blocks", "blocks": [
                    {"kind": "p", "attrs": {}, "text": "LC alpha block."},
                    {"kind": "p", "attrs": {}, "text": "LC beta block."},
                    {"kind": "p", "attrs": {}, "text": "LC gamma block."},
                ]})
                base_ids = (reseed.get("receipt") or {}).get("ids") if reseed.get("ok") else None
                reseed_conv = wait_converged(
                    expect=lambda a: '"LC alpha block."' in a
                    and '"LC gamma block."' in a, timeout=15)
                canvas_drive(p1)
                r_seed_cv = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                        [50, 50, "LC-SEED-1"])
                seed_cv_id = r_seed_cv.get("id") if r_seed_cv.get("ok") else None
                seed_cv_seen = wait_until(lambda: p2.evaluate(
                    "([i]) => window.__canvasObjects('main')"
                    ".some(o => o.id === i) || false", [seed_cv_id]), timeout=10)
                check("collab_pair_restart_wipes_and_resyncs",
                      pre_seed.get("ok") is True and pre_conv is not None
                      and pre_cv_counts["p1"] > 0 and pre_cv_counts["p2"] > 0
                      and revive_pair is not None
                      and wiped["p1"]["doc"] == 2 and wiped["p1"]["cv"] == pre_cv_counts["p1"]
                      and wiped["p2"]["doc"] == 2 and wiped["p2"]["cv"] == pre_cv_counts["p2"]
                      and reseed.get("ok") is True and base_ids is not None
                      and len(base_ids) == 3
                      and reseed_conv is not None
                      and r_seed_cv.get("ok") is True and seed_cv_seen is not None,
                      json.dumps({"pre_conv": pre_conv is not None,
                                  "pre_cv_counts": pre_cv_counts,
                                  "wiped": wiped, "reseed_ids": len(base_ids or []),
                                  "reseed_conv": reseed_conv is not None,
                                  "seed_cv_p2": seed_cv_seen is not None,
                                  "revive_pair": revive_pair is not None,
                                  "note": "room state survives member restarts"
                                          " (peer resurrection via state-vector catch-up);"
                                          " same-slot reload op-id collisions are"
                                          " silently CRDT-deduped (see meta)"},
                                 ensure_ascii=False)[:400])

                # 7. collab 会话快照恢复：authoritative_snapshot →
                #    restore（同槽 client id；撤销栈不随快照走——B3.1）
                r_delta = collab_exec(p1, "human", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "LC delta block."}})
                delta_seen = wait_until(lambda: p2.evaluate(JS_DOC_HAS, "LC delta block."),
                                        timeout=10)
                doc_pre = doc_snap(p1)
                doc_pre_p2 = doc_snap(p2)
                snap_b64 = p1.evaluate("window.__collabSnapshot()")
                d_pre = p1.evaluate("window.__collabDepth('human')")
                r_restore = p1.evaluate("b => window.__collabRestoreDoc(b)", snap_b64)
                doc_post = doc_snap(p1)
                d_post = p1.evaluate("window.__collabDepth('human')")
                r_eps = collab_exec(p1, "human", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "LC epsilon block."}})
                d_eps = p1.evaluate("window.__collabDepth('human')")
                eps_conv = wait_converged(
                    expect=lambda a: '"LC epsilon block."' in a
                    and '"LC delta block."' in a, timeout=15)
                check("collab_doc_snapshot_restore",
                      r_delta.get("ok") is True and delta_seen is not None
                      and doc_pre == doc_pre_p2
                      and isinstance(snap_b64, str) and len(snap_b64) > 40
                      and d_pre.get("undo", 0) >= 2
                      and r_restore.get("ok") is True
                      and doc_post == doc_pre                    # 块 id/文本全等
                      and d_post.get("undo") == 0 and d_post.get("redo") == 0
                      and r_eps.get("ok") is True
                      and d_eps.get("undo") == 1                 # 会话仍可操作
                      and eps_conv is not None,
                      json.dumps({"delta": r_delta.get("ok"),
                                  "pre_converged": doc_pre == doc_pre_p2,
                                  "snap_len": len(snap_b64 or ""),
                                  "depth": [d_pre, d_post, d_eps],
                                  "restore": r_restore,
                                  "doc_identical": doc_post == doc_pre,
                                  "eps_conv": eps_conv is not None},
                                 ensure_ascii=False)[:400])

                # ══ C. 断连与恢复 ═════════════════════════════════════

                # 8. 断连冻结对端：p2 通道关闭 → p1 doc+canvas 双面操作
                #    p2 均不可见（缺席证据）且自身状态冻结；p1 健康
                p2.evaluate("window.__collabDisconnect()")
                p2.wait_for_timeout(300)
                frozen_doc = doc_snap(p2)
                st_disc = p2.evaluate("window.__collabState()")
                r_miss = collab_exec(p1, "human", {
                    "t": "replace_text", "block_id": base_ids[1],
                    "start": 0, "end": 0, "insert": "[LC-MISSED] "})
                canvas_drive(p1)
                r_cn8 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                    [300, 160, "LC-NODE-DISCONN"])
                cn8_id = r_cn8.get("id") if r_cn8.get("ok") else None
                absent_doc = stays_absent(lambda: p2.evaluate(JS_DOC_HAS, "[LC-MISSED]"),
                                          timeout=1.4)
                absent_cv = stays_absent(lambda: p2.evaluate(JS_CV_HAS_LABEL, "LC-NODE-DISCONN"),
                                         timeout=1.4)
                still_frozen = doc_snap(p2) == frozen_doc
                p1_health = p1.evaluate("""() => ({
                    docHas: JSON.stringify(window.__collabDoc()).indexOf('[LC-MISSED]') >= 0,
                    cvHas: window.__canvasObjects('main')
                      .some(o => o.props && o.props.label === 'LC-NODE-DISCONN'),
                    state: window.__collabState()})""")
                check("disconnect_freezes_peer_state",
                      st_disc.get("connected") is False
                      and r_miss.get("ok") is True and cn8_id is not None
                      and absent_doc and absent_cv
                      and still_frozen
                      and p1_health["docHas"] and p1_health["cvHas"]
                      and p1_health["state"].get("connected") is True
                      and p1_health["state"].get("errors") == [],
                      json.dumps({"p2_connected": st_disc.get("connected"),
                                  "miss": r_miss.get("ok"), "node": cn8_id is not None,
                                  "absent": [absent_doc, absent_cv],
                                  "frozen": still_frozen,
                                  "p1": {"doc": p1_health["docHas"],
                                         "cv": p1_health["cvHas"],
                                         "errors": p1_health["state"].get("errors")}},
                                 ensure_ascii=False)[:400])

                # 9. 重连 catch-up：p2 重连（自带 sync_req 状态向量）→
                #    doc 全等 + canvas 对象计数全等 + 节点级一致
                p2.evaluate("window.__collabReconnect()")
                conv9 = wait_until(lambda: (
                    doc_snap(p1) == doc_snap(p2)
                    and p2.evaluate(JS_CV_BY_ID, cn8_id) is not None
                    and p1.evaluate("window.__canvasObjects('main').length")
                    == p2.evaluate("window.__canvasObjects('main').length")), timeout=15)
                st9 = {
                    "p1": p1.evaluate("window.__collabState()"),
                    "p2": p2.evaluate("window.__collabState()"),
                }
                cv_sync_p2 = p2.evaluate("window.__canvasSyncState()")
                node9 = p2.evaluate(JS_CV_BY_ID, cn8_id) if cn8_id else None
                check("reconnect_catchup_converges",
                      conv9 is not None
                      and st9["p2"].get("connected") is True
                      and st9["p1"].get("errors") == [] and st9["p2"].get("errors") == []
                      and cv_sync_p2.get("converged") is True
                      and node9 is not None
                      and node9.get("props", {}).get("label") == "LC-NODE-DISCONN",
                      json.dumps({"converged": conv9 is not None,
                                  "doc_eq": doc_snap(p1) == doc_snap(p2),
                                  "cv_counts": [p1.evaluate("window.__canvasObjects('main').length"),
                                                p2.evaluate("window.__canvasObjects('main').length")],
                                  "node_on_p2": node9 and node9.get("props", {}).get("label"),
                                  "sync": cv_sync_p2,
                                  "errors": [st9["p1"].get("errors"), st9["p2"].get("errors")]},
                                 ensure_ascii=False)[:400])

                # 10. 断连期双侧分叉编辑 → 重连 CRDT 无冲突合流
                #     （p2 重连取回 p1 侧；p1 周期取回 p2 侧——双端收敛）
                p2.evaluate("window.__collabDisconnect()")
                r_div1 = collab_exec(p1, "human", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "LC p1 divergent edit."}})
                r_div2 = collab_exec(p2, "ai", {
                    "t": "insert_block", "parent": None, "index": 0, "id": None,
                    "spec": {"kind": "p", "attrs": {}, "text": "LC p2 divergent edit."}})
                div_observable = stays_absent(
                    lambda: p1.evaluate(JS_DOC_HAS, "LC p2 divergent edit."), timeout=1.2)
                p2.evaluate("window.__collabReconnect()")
                p2_merged = wait_until(lambda: (
                    p2.evaluate(JS_DOC_HAS, "LC p1 divergent edit.")
                    and p2.evaluate(JS_DOC_HAS, "LC p2 divergent edit.")), timeout=15)
                p1.evaluate("window.__collabDisconnect()")
                p1.evaluate("window.__collabReconnect()")
                conv10 = wait_until(lambda: (
                    doc_snap(p1) == doc_snap(p2)
                    and p1.evaluate(JS_DOC_HAS, "LC p2 divergent edit.")
                    and p2.evaluate(JS_DOC_HAS, "LC p1 divergent edit.")), timeout=15)
                conv10_doc = doc_snap(p1) if conv10 else ""
                both_md = ('"LC p1 divergent edit."' in conv10_doc
                           and '"LC p2 divergent edit."' in conv10_doc)
                st10 = {"p1": p1.evaluate("window.__collabState()"),
                        "p2": p2.evaluate("window.__collabState()")}
                check("edits_during_disconnect_no_conflict",
                      r_div1.get("ok") is True and r_div2.get("ok") is True
                      and div_observable
                      and p2_merged is not None
                      and conv10 is not None and both_md
                      and st10["p1"].get("errors") == [] and st10["p2"].get("errors") == [],
                      json.dumps({"p1_edit": r_div1.get("ok"),
                                  "p2_edit_while_disconnected": r_div2.get("ok"),
                                  "divergence_observable_p1": div_observable,
                                  "p2_merged_superset": p2_merged is not None,
                                  "both_converged": conv10 is not None,
                                  "both_contents_present": both_md,
                                  "doc_blocks": [st10["p1"].get("docBlocks"),
                                                 st10["p2"].get("docBlocks")],
                                  "errors": [st10["p1"].get("errors"),
                                             st10["p2"].get("errors")]},
                                 ensure_ascii=False)[:400])

                # 11. 断连-重连 ×3 循环：每轮双侧编辑 → 双端收敛 + 零错误
                cycle_notes = []
                cycles_ok = True
                for i in range(3):
                    p2.evaluate("window.__collabDisconnect()")
                    c_p1 = collab_exec(p1, "human", {
                        "t": "insert_block", "parent": None, "index": 0, "id": None,
                        "spec": {"kind": "p", "attrs": {},
                                 "text": f"LC cycle {i} p1 side."}})
                    c_p2 = collab_exec(p2, "ai", {
                        "t": "insert_block", "parent": None, "index": 0, "id": None,
                        "spec": {"kind": "p", "attrs": {},
                                 "text": f"LC cycle {i} p2 side."}})
                    p2.evaluate("window.__collabReconnect()")
                    m_p2 = wait_until(lambda i=i: (
                        p2.evaluate(JS_DOC_HAS, f"LC cycle {i} p1 side.")
                        and p2.evaluate(JS_DOC_HAS, f"LC cycle {i} p2 side.")), timeout=15)
                    p1.evaluate("window.__collabDisconnect()")
                    p1.evaluate("window.__collabReconnect()")
                    conv_c = wait_until(lambda i=i: (
                        doc_snap(p1) == doc_snap(p2)
                        and p1.evaluate(JS_DOC_HAS, f"LC cycle {i} p1 side.")
                        and p1.evaluate(JS_DOC_HAS, f"LC cycle {i} p2 side.")), timeout=15)
                    ok_c = (c_p1.get("ok") is True and c_p2.get("ok") is True
                            and m_p2 is not None and conv_c is not None)
                    cycles_ok = cycles_ok and ok_c
                    cycle_notes.append({"cycle": i, "edits": [c_p1.get("ok"), c_p2.get("ok")],
                                        "p2_merged": m_p2 is not None,
                                        "converged": conv_c is not None})
                st11 = {"p1": p1.evaluate("window.__collabState()"),
                        "p2": p2.evaluate("window.__collabState()")}
                all_markers = all(
                    p1.evaluate(JS_DOC_HAS, f"LC cycle {i} {s} side.")
                    for i in range(3) for s in ("p1", "p2"))
                check("reconnect_repeated_cycles",
                      cycles_ok and all_markers
                      and doc_snap(p1) == doc_snap(p2)
                      and st11["p1"].get("errors") == [] and st11["p2"].get("errors") == []
                      and st11["p1"].get("connected") is True
                      and st11["p2"].get("connected") is True,
                      json.dumps({"cycles": cycle_notes,
                                  "all_markers": all_markers,
                                  "doc_blocks": [st11["p1"].get("docBlocks"),
                                                 st11["p2"].get("docBlocks")],
                                  "errors": [st11["p1"].get("errors"),
                                             st11["p2"].get("errors")]},
                                 ensure_ascii=False)[:400])

                # 12. canvas 断连-重连（bring_to_front 驱动纪律）
                p2.evaluate("window.__collabDisconnect()")
                canvas_drive(p1)
                r_cv12 = p1.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                     [420, 260, "LC-CV10-REVIVE"])
                cv12_id = r_cv12.get("id") if r_cv12.get("ok") else None
                absent_cv12 = stays_absent(
                    lambda: p2.evaluate(JS_CV_HAS_LABEL, "LC-CV10-REVIVE"), timeout=1.4)
                p2.evaluate("window.__collabReconnect()")
                cv12_got = wait_until(lambda: (
                    p2.evaluate(JS_CV_BY_ID, cv12_id) is not None
                    and p1.evaluate("window.__canvasObjects('main').length")
                    == p2.evaluate("window.__canvasObjects('main').length")), timeout=15)
                node12 = p2.evaluate(JS_CV_BY_ID, cv12_id) if cv12_id else None
                node12_p1 = p1.evaluate(JS_CV_BY_ID, cv12_id) if cv12_id else None
                sync12 = p2.evaluate("window.__canvasSyncState()")
                check("canvas_disconnect_reconnect",
                      r_cv12.get("ok") is True and cv12_id is not None
                      and absent_cv12
                      and cv12_got is not None
                      and node12 is not None and node12_p1 is not None
                      and node12.get("x") == node12_p1.get("x")
                      and node12.get("y") == node12_p1.get("y")
                      and node12.get("props", {}).get("label") == "LC-CV10-REVIVE"
                      and sync12.get("converged") is True,
                      json.dumps({"add": r_cv12.get("ok"),
                                  "absent_while_disconnected": absent_cv12,
                                  "caught_up": cv12_got is not None,
                                  "node_p2": node12 and {"x": node12.get("x"),
                                                         "y": node12.get("y"),
                                                         "label": node12.get("props", {}).get("label")},
                                  "node_p1": node12_p1 and {"x": node12_p1.get("x"),
                                                            "y": node12_p1.get("y")},
                                  "sync_p2": sync12}, ensure_ascii=False)[:400])

                # ══ D. 收尾 ═══════════════════════════════════════════

                # 13. 晚加入第三页：被动 hello catch-up 不投递既有状态
                #     （实测）；显式 disconnect/reconnect（自带空状态向量
                #     sync_req）→ 全量 catch-up 收敛（doc + canvas）
                p3 = p1.context.new_page()
                p3.on("pageerror", lambda e: page_errors["p3"].append(str(e)))
                p3.goto(f"http://127.0.0.1:{PORT}/index.html?cc=3")
                p3.wait_for_function(
                    "window.__ready === true && window.__collabReady === true"
                    " && window.__canvasReady === true", timeout=60000)
                p3.wait_for_function("window.__collabState().canvasHooked === true",
                                     timeout=30000)
                p3.wait_for_timeout(2000)
                passive = p3.evaluate(
                    "() => ({doc: window.__collabDoc().length,"
                    " canvas: window.__canvasObjects('main').length})")
                p3.evaluate("window.__collabDisconnect()")
                p3.evaluate("window.__collabReconnect()")
                conv13 = wait_until(lambda: (
                    doc_snap(p3) == doc_snap(p1)
                    and p3.evaluate("window.__canvasObjects('main').length")
                    == p1.evaluate("window.__canvasObjects('main').length")
                    and p3.evaluate(JS_CV_BY_ID, cv12_id) is not None), timeout=15)
                st13 = p3.evaluate("window.__collabState()")
                check("late_joiner_third_page_catchup",
                      conv13 is not None
                      and st13.get("connected") is True and st13.get("errors") == [],
                      json.dumps({"passive_after_2s": passive,
                                  "note": "hello-path catchUpFrom posts the sender's own sv;"
                                          ' responders diff against it, so a fresh joiner'
                                          " (never publishes its own sv) receives nothing"
                                          " passive; explicit disconnect/reconnect"
                                          " (sync_req with the joiner's sv) delivers the"
                                          " full state-vector catch-up",
                                  "explicit_recovery_converged": conv13 is not None,
                                  "p3_doc_blocks": st13.get("docBlocks"),
                                  "p1_doc_blocks": p1.evaluate(
                                      "window.__collabState().docBlocks")},
                                 ensure_ascii=False)[:400])

                # 14. 三页状态一致：connected + errors[] + 成员互见 + docBlocks 全等
                st14 = {k: p.evaluate("window.__collabState()")
                        for k, p in (("p1", p1), ("p2", p2), ("p3", p3))}
                blocks14 = [st14[k].get("docBlocks") for k in ("p1", "p2", "p3")]
                check("state_consistency_final",
                      all(st14[k].get("connected") is True for k in st14)
                      and all(st14[k].get("errors") == [] for k in st14)
                      and 2 in st14["p1"].get("members", []) and 3 in st14["p1"].get("members", [])
                      and 1 in st14["p2"].get("members", []) and 3 in st14["p2"].get("members", [])
                      and 1 in st14["p3"].get("members", []) and 2 in st14["p3"].get("members", [])
                      and blocks14[0] == blocks14[1] == blocks14[2] and blocks14[0] >= 1,
                      json.dumps({k: {"connected": v.get("connected"),
                                      "members": v.get("members"),
                                      "errors": v.get("errors"),
                                      "docBlocks": v.get("docBlocks")}
                                  for k, v in st14.items()}, ensure_ascii=False)[:400])

                # 15. 全程零 pageerror（含全部 reload / 断连 / 重连 / 三页）
                check("zero_pageerror_all_pages",
                      all(len(v) == 0 for v in page_errors.values()),
                      json.dumps({k: v[:3] for k, v in page_errors.items()},
                                 ensure_ascii=False))

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
        "meta": {
            "gate": "stage 6 §九.8 lifecycle web composition e2e "
                    "(app exit / restart / disconnect / recovery)",
            "dist": str(DIST), "port": PORT, "total": len(checks),
            "failed": len(failed),
            "native_evidence": {
                "relay_crash_recovery": "crates/relay/examples/relay_demo.rs (13 checkpoints)",
                "collab_snapshot_recovery": "crates/collab/tests/recovery.rs",
                "doc_snapshot_history": "crates/mdcore/tests/document_progressive.rs (③')",
                "sidecar_persistence": "crates/workbench/tests/persistence_recovery.rs",
            },
            "observed_sync_semantics": {
                "reconnect_catchup": "reconnecting page posts sync_req with its own state"
                                     " vector; peers answer sync_resp diffs (doc+canvas)",
                "divergent_merge": "divergent edits made while disconnected reach the peer"
                                   " only after that peer also cycles disconnect/reconnect"
                                   " (its reconnect sync_req pulls the peer's diff)",
                "late_join_passive": "fresh joiner receives nothing passive (hello-path"
                                     " catchUpFrom diffs against the sender's own sv);"
                                     " explicit reconnect delivers full catch-up",
                "same_slot_reload_canvas_dedup": "a reloaded same-slot canvas session"
                                                " reuses its slot-derived client id with a"
                                                " restarted op counter; new ops collide with"
                                                " the peer's accumulated op ids and are"
                                                " silently CRDT-deduped — restarting BOTH"
                                                " pages clears the collision surface",
            },
        },
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed),
                    "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
