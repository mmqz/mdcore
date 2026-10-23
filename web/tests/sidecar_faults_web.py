#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Stage 6 §九.6 — sidecar 故障路径 e2e（evidence workbench web face）.

Web-face twin of the native fail-closed family pinned in
crates/workbench/tests/persistence_recovery.rs — driven over a REAL
browser page against the built dist/ (no product code touched):

  A. 损坏 (corruption): truncated JSON / garbage bytes / empty / null /
     future schema_version / tampered body fingerprint → restore is
     REJECTED with a typed {ok:false, code:'wb_sidecar'|'wb_body_stale'}
     result — never a throw, page stays alive, session untouched.
  B. 丢失 (loss): page reload with NO restore = the session is simply
     gone (fresh panel, zero evidence, no sidecar export) — honest
     "lost sidecar = lost session" semantics, nothing crashes.
  C. 版本不匹配 (body mismatch): sidecar captured against body v1,
     restored into a session started with body v2 → typed wb_body_stale
     rejection, no partial application, session still usable. Plus the
     designed degrade path: refresh_body → stale → reverify recovery.
  D. 异常退出 (abnormal exit): reload as session death — the sidecar
     captured before death + host re-supply (body + source bytes)
     revives the session with full continuity (evidence / reviews /
     audit chain / linkage), the session stays live (recalc + new
     binding + review) and the panel's audit clock re-seats monotononic
     over the restored chain (N2).

Fault-path calls all run inside try/catch evaluate wrappers: the panel
contract says typed failures never throw — pinned as an aggregate check.
Fixtures = tests/fixtures/{evidence_body.docx, evidence_body_v2.docx,
evidence_sample.xlsx}. Results -> tests/results/results_sidecar_faults.json.
"""

import base64
import http.server
import json
import socketserver
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURES = WEB / "tests" / "fixtures"
XLSX_FIXTURE = FIXTURES / "evidence_sample.xlsx"
BODY_FIXTURE = FIXTURES / "evidence_body.docx"
BODY_V2_FIXTURE = FIXTURES / "evidence_body_v2.docx"
OUT = WEB / "tests" / "results" / "results_sidecar_faults.json"
PORT = 8944

ZERO_COUNTS = {"pending": 0, "approved": 0, "rejected": 0, "stale": 0}

# Proposal JSON faces (workbench contract — exact shapes from the passing
# tests/evidence_panel.py suite; e-3 is the post-restore "session is live"
# probe with a fresh evidence_id + generation_batch).
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

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True

# wasm 面缺席时的整组 SKIP 名单（防御性——dist 现含 WasmWorkbench）
WASM_CASES = [
    "session_build_and_live_sidecar_structure",
    "restore_truncated_sidecar_fails_closed",
    "restore_garbage_sidecar_fails_closed",
    "restore_empty_and_null_sidecar_fails_closed",
    "restore_future_schema_version_fails_closed",
    "restore_tampered_body_fingerprint_fails_closed",
    "session_without_sidecar_is_fresh_after_reload",
    "restore_after_reload_requires_body_resupply",
    "abnormal_exit_recovery_full_continuity",
    "post_recovery_operations_work",
    "recovery_clock_monotonicity",
    "stale_then_refresh_body_recovery",
    "restore_against_changed_body_fails_closed",
    "typed_failures_never_throw",
]


def main() -> int:
    missing = [p for p in (XLSX_FIXTURE, BODY_FIXTURE, BODY_V2_FIXTURE) if not p.exists()]
    if missing:
        print(f"FATAL fixture missing {[str(p) for p in missing]}")
        return 2

    checks: list[dict] = []
    skipped = 0
    page_errors: list[str] = []
    fault_calls: list[tuple] = []  # (label, {threw, r}) — aggregate never-throw check
    semantics: dict[str, str] = {}  # observed typed code per fault face (meta)

    def check(name, ok, detail="", skip=False):
        nonlocal skipped
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip), "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        if skip:
            skipped += 1
        print(tag + name + ("  | " + str(detail)[:200] if detail and not ok and not skip else ""))
        return ok

    def safe_call(page, fn_js, arg, label):
        """Fault-path wrapper: the panel contract must return typed results,
        never throw — a throw inside the page surfaces as {threw:true}."""
        res = page.evaluate(
            "a => { try { return { threw: false, r: (" + fn_js + ")(a) }; }"
            " catch (e) { return { threw: true, err: String(e) }; } }",
            arg,
        )
        fault_calls.append((label, res))
        return res

    def alive(page, expect_active=True):
        """Trivial working op after each fault injection — page must answer."""
        try:
            st = page.evaluate("window.__evState()")
        except Exception as e:  # noqa: BLE001
            return False, {"eval_error": str(e)}
        ok = isinstance(st, dict) and st.get("active") is expect_active
        return ok, st

    xlsx_b64 = base64.b64encode(XLSX_FIXTURE.read_bytes()).decode()
    body_b64 = base64.b64encode(BODY_FIXTURE.read_bytes()).decode()
    body_v2_b64 = base64.b64encode(BODY_V2_FIXTURE.read_bytes()).decode()

    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()  # RAM tight: one browser, one page
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto(f"http://127.0.0.1:{PORT}/index.html")
            page.wait_for_function("window.__ready === true", timeout=60000)
            page.wait_for_function("window.__evReady === true", timeout=60000)

            has_wasm = page.evaluate("window.__evState().wasm") is True

            if not has_wasm:
                for name in WASM_CASES:
                    check(name, True, "pending wasm (WasmWorkbench absent from pkg — skip until rebuild)",
                          skip=True)
            else:
                # ---- 1. 建满 8 步会话 + 现场 sidecar 结构面 ----
                steps = {
                    "start": page.evaluate("b => window.__evStartBody(b)", body_b64),
                    "src": page.evaluate("b => window.__evIngestSource(b)", xlsx_b64),
                    "bind1": page.evaluate("j => window.__evProposeBinding(j)", E_QUOTE),
                    "bind2": page.evaluate("j => window.__evProposeBinding(j)", E_CALC),
                    "calc": page.evaluate("j => window.__evProposeCalc(j)", CALC_1),
                    "recalc": page.evaluate("c => window.__evRecalc(c)", "calc-1"),
                    "rev2": page.evaluate("([i, d]) => window.__evReview(i, d)", ["e-2", "approved"]),
                    "rev1": page.evaluate("([i, d]) => window.__evReview(i, d)", ["e-1", "rejected"]),
                }
                sc = page.evaluate("window.__evSidecar()")
                parsed = json.loads(sc)
                audit_kinds = [a["kind"] for a in parsed.get("audit", [])]
                reviews = {e["evidence_id"]: e["review"] for e in parsed.get("evidence", [])}
                fp = (parsed.get("document") or {}).get("fingerprint", "")
                audit_before = len(parsed.get("audit", []))
                check(
                    "session_build_and_live_sidecar_structure",
                    all(r.get("ok") is True for r in steps.values())
                    and steps["src"].get("doc_id") == "src-1"
                    and steps["calc"].get("value") == "4210.000"
                    and parsed.get("schema_version") == 1
                    and {"schema_version", "document", "source_docs", "evidence",
                         "calculations", "idempotency", "audit"}.issubset(set(parsed.keys()))
                    and len(parsed.get("evidence", [])) == 2
                    and reviews == {"e-1": "rejected", "e-2": "approved"}
                    and len(parsed.get("calculations", [])) == 1
                    and len(parsed.get("source_docs", [])) == 1
                    and audit_before >= 6
                    and all(a < b for a, b in zip(
                        [a["seq"] for a in parsed["audit"]],
                        [a["seq"] for a in parsed["audit"]][1:]))
                    and fp.startswith("fnv64:") and len(fp) > len("fnv64:"),
                    json.dumps({"steps": {k: v.get("ok") for k, v in steps.items()},
                                "calc": steps["calc"].get("value"),
                                "schema_version": parsed.get("schema_version"),
                                "keys": sorted(parsed.keys()),
                                "document": parsed.get("document"),
                                "audit_len": audit_before, "audit_kinds": audit_kinds,
                                "sc_len": len(sc)}, ensure_ascii=False),
                )

                # ---- 2. 截断 sidecar → wb_sidecar fail-closed ----
                truncated = sc[: int(len(sc) * 0.6)]
                r = safe_call(page, "window.__evRestoreSidecar", truncated, "truncated")
                live, st = alive(page)
                semantics["truncated"] = str(r["r"].get("code"))
                check("restore_truncated_sidecar_fails_closed",
                      r["threw"] is False and r["r"].get("ok") is False
                      and r["r"].get("code") == "wb_sidecar"
                      and bool(r["r"].get("message"))
                      and live and st.get("evidence") == 2,
                      json.dumps({"r": r, "state": st}, ensure_ascii=False))

                # ---- 3. 垃圾字节（非 JSON / emoji 乱码）→ wb_sidecar ----
                g1 = safe_call(page, "window.__evRestoreSidecar",
                               '{"this is": not json at all', "garbage_json")
                g2 = safe_call(page, "window.__evRestoreSidecar",
                               "💥garbage💥", "garbage_emoji")
                live, st = alive(page)
                semantics["garbage_json"] = str(g1["r"].get("code"))
                semantics["garbage_emoji"] = str(g2["r"].get("code"))
                check("restore_garbage_sidecar_fails_closed",
                      g1["threw"] is False and g1["r"].get("ok") is False
                      and g1["r"].get("code") == "wb_sidecar" and bool(g1["r"].get("message"))
                      and g2["threw"] is False and g2["r"].get("ok") is False
                      and g2["r"].get("code") == "wb_sidecar" and bool(g2["r"].get("message"))
                      and live and st.get("evidence") == 2,
                      json.dumps({"g1": g1, "g2": g2, "state": st}, ensure_ascii=False))

                # ---- 4. 空串 / null → wb_sidecar ----
                e1 = safe_call(page, "window.__evRestoreSidecar", "", "empty")
                e2 = safe_call(page, "window.__evRestoreSidecar", "null", "null")
                live, st = alive(page)
                semantics["empty"] = str(e1["r"].get("code"))
                semantics["null"] = str(e2["r"].get("code"))
                check("restore_empty_and_null_sidecar_fails_closed",
                      e1["threw"] is False and e1["r"].get("ok") is False
                      and e1["r"].get("code") == "wb_sidecar" and bool(e1["r"].get("message"))
                      and e2["threw"] is False and e2["r"].get("ok") is False
                      and e2["r"].get("code") == "wb_sidecar" and bool(e2["r"].get("message"))
                      and live and st.get("evidence") == 2,
                      json.dumps({"empty": e1, "null": e2, "state": st}, ensure_ascii=False))

                # ---- 5. 未来 schema_version=99 → wb_sidecar（未知版本 fail-closed）----
                future = json.loads(sc)
                future["schema_version"] = 99
                f = safe_call(page, "window.__evRestoreSidecar",
                              json.dumps(future, ensure_ascii=False), "future_schema")
                live, st = alive(page)
                semantics["future_schema_version"] = str(f["r"].get("code"))
                check("restore_future_schema_version_fails_closed",
                      f["threw"] is False and f["r"].get("ok") is False
                      and f["r"].get("code") == "wb_sidecar"
                      and "schema version" in str(f["r"].get("message", ""))
                      and live and st.get("evidence") == 2,
                      json.dumps({"r": f, "state": st}, ensure_ascii=False))

                # ---- 6. 篡改正文指纹（document.fingerprint 一字节）→ wb_body_stale ----
                # 实际语义：restore 被拒（fail-closed）而非降级——重供正文字节
                # 与 sidecar 登记指纹不一致 = BodyStale typed 拒绝。
                tampered = json.loads(sc)
                fp_old = tampered["document"]["fingerprint"]
                tampered["document"]["fingerprint"] = (
                    fp_old[:6] + ("0" if fp_old[6] != "0" else "1") + fp_old[7:])
                assert tampered["document"]["fingerprint"] != fp_old
                tm = safe_call(page, "window.__evRestoreSidecar",
                               json.dumps(tampered, ensure_ascii=False), "tampered_fingerprint")
                live, st = alive(page)
                semantics["tampered_body_fingerprint"] = str(tm["r"].get("code"))
                check("restore_tampered_body_fingerprint_fails_closed",
                      tm["threw"] is False and tm["r"].get("ok") is False
                      and tm["r"].get("code") == "wb_body_stale"
                      and bool(tm["r"].get("message"))
                      and live and st.get("active") is True and st.get("evidence") == 2,
                      json.dumps({"r": tm, "state": st,
                                  "note": "fail-closed restore rejection (wb_body_stale), "
                                          "not a stale-degrade — session intact"},
                                 ensure_ascii=False))

                # ---- 7. 丢失：reload 不恢复 → 全新面板（丢 sidecar = 丢会话）----
                page.reload()
                page.wait_for_function("window.__ready === true", timeout=60000)
                page.wait_for_function("window.__evReady === true", timeout=60000)
                fresh = page.evaluate("""() => ({
                    state: window.__evState(),
                    evidence: window.__evEvidence(),
                    counts: window.__evCounts(),
                    sidecar: window.__evSidecar(),
                })""")
                check("session_without_sidecar_is_fresh_after_reload",
                      fresh["state"]["active"] is False
                      and fresh["state"]["evidence"] == 0
                      and fresh["state"]["sources"] == []
                      and fresh["state"]["counts"] == ZERO_COUNTS
                      and fresh["evidence"] == [] and fresh["counts"] == ZERO_COUNTS
                      and fresh["sidecar"] == "",
                      json.dumps({"state": fresh["state"], "sidecar": fresh["sidecar"]},
                                 ensure_ascii=False))

                # ---- 8. 异常退出后裸恢复：宿主必须先重供正文字节 ----
                nb = safe_call(page, "window.__evRestoreSidecar", sc, "no_body_guard")
                live, st = alive(page, expect_active=False)
                semantics["restore_without_body_resupply"] = str(nb["r"].get("code"))
                check("restore_after_reload_requires_body_resupply",
                      nb["threw"] is False and nb["r"].get("ok") is False
                      and nb["r"].get("code") == "no_body"
                      and bool(nb["r"].get("message"))
                      and live,
                      json.dumps({"r": nb, "state": st}, ensure_ascii=False))

                # ---- 9. 异常退出恢复：reload 前 sidecar + 宿主重供 → 全量连续 ----
                re_supplied = {
                    "body": page.evaluate("b => window.__evStartBody(b)", body_b64),
                    "src": page.evaluate("b => window.__evIngestSource(b)", xlsx_b64),
                }
                rr = page.evaluate("j => window.__evRestoreSidecar(j)", sc)
                after = page.evaluate("""() => ({
                    state: window.__evState(),
                    evidence: window.__evEvidence(),
                    audit: window.__evAudit(),
                })""")
                ev_reviews = {e["evidence_id"]: e["review"] for e in after["evidence"]}
                ev_linkage = {e["evidence_id"]: e["linkage"] for e in after["evidence"]}
                aseq = [a["seq"] for a in after["audit"]]
                check("abnormal_exit_recovery_full_continuity",
                      re_supplied["body"].get("ok") is True
                      and re_supplied["src"].get("ok") is True
                      and rr.get("ok") is True
                      and len(after["evidence"]) == 2
                      and ev_reviews == {"e-1": "rejected", "e-2": "approved"}
                      and ev_linkage == {"e-1": "valid", "e-2": "valid"}
                      and len(after["audit"]) == audit_before  # exact preservation
                      and [a["kind"] for a in after["audit"]] == audit_kinds
                      and all(a < b for a, b in zip(aseq, aseq[1:]))
                      and after["state"]["active"] is True
                      and after["state"]["sources"] == ["src-1"],
                      json.dumps({"restore": rr, "evidence": len(after["evidence"]),
                                  "reviews": ev_reviews, "linkage": ev_linkage,
                                  "audit_len": len(after["audit"]),
                                  "audit_before": audit_before,
                                  "sources": after["state"]["sources"]}, ensure_ascii=False))

                # ---- 10. 恢复后会话仍活：复算 + 新关联（不是冻结快照）----
                rc = page.evaluate("c => window.__evRecalc(c)", "calc-1")
                mid = page.evaluate("""() => ({
                    e2: window.__evEvidence().find(e => e.evidence_id === 'e-2'),
                    counts: window.__evCounts(),
                })""")
                pb = page.evaluate("j => window.__evProposeBinding(j)", E_3_NEW)
                end = page.evaluate("""() => ({
                    e3: window.__evEvidence().find(e => e.evidence_id === 'e-3'),
                    counts: window.__evCounts(),
                })""")
                check("post_recovery_operations_work",
                      rc.get("ok") is True
                      and (mid["e2"] or {}).get("calc") == "passed"
                      and (mid["e2"] or {}).get("linkage") == "valid"
                      and pb.get("ok") is True and pb.get("evidence_id") == "e-3"
                      and (end["e3"] or {}).get("review") == "pending"
                      and (end["e3"] or {}).get("linkage") == "valid"
                      and end["counts"] == {"pending": 1, "approved": 1, "rejected": 1, "stale": 0},
                      json.dumps({"recalc": rc.get("ok"), "e2_calc": (mid["e2"] or {}).get("calc"),
                                  "propose": pb, "e3": end["e3"], "counts": end["counts"],
                                  "audit_len": len(page.evaluate('window.__evAudit()'))},
                                 ensure_ascii=False))

                # ---- 11. 恢复时钟单调：恢复链上续操作 at_ms 严格越过重seat水位 ----
                rv = page.evaluate("([i, d]) => window.__evReview(i, d)", ["e-3", "approved"])
                audit = page.evaluate("window.__evAudit()")
                restored_chain = audit[:audit_before]
                new_entries = audit[audit_before:]
                seqs = [a["seq"] for a in audit]
                ats = [a["at_ms"] for a in audit]
                max_restored = max(a["at_ms"] for a in restored_chain)
                check("recovery_clock_monotonicity",
                      rv.get("ok") is True
                      and len(audit) >= audit_before + 3
                      and len(new_entries) >= 3
                      and all(a["at_ms"] > max_restored for a in new_entries)
                      and all(a <= b for a, b in zip(ats, ats[1:]))
                      and all(a < b for a, b in zip(seqs, seqs[1:]))
                      and seqs[-1] == len(audit),
                      json.dumps({"audit_len": len(audit), "audit_before": audit_before,
                                  "max_restored_at": max_restored,
                                  "new_at_ms": [a["at_ms"] for a in new_entries],
                                  "new_kinds": [a["kind"] for a in new_entries],
                                  "last_seq": seqs[-1],
                                  "note": "at_ms non-decreasing overall (one op may stamp "
                                          "multiple entries at the same at_ms); every "
                                          "post-restore entry strictly exceeds the restored "
                                          "chain's max (panel clockMs re-seat, N2)"},
                             ensure_ascii=False))

                # ---- 12. 正文刷新降级 → reverify 恢复（设计的降级通道）----
                rf = page.evaluate("b => window.__evRefreshBody(b)", body_v2_b64)
                mid2 = page.evaluate("""() => ({
                    counts: window.__evCounts(),
                    e1: window.__evEvidence().find(e => e.evidence_id === 'e-1'),
                    e2: window.__evEvidence().find(e => e.evidence_id === 'e-2'),
                })""")
                rvy = page.evaluate("id => window.__evReverify(id)", "e-1")
                end2 = page.evaluate("""() => ({
                    counts: window.__evCounts(),
                    e1: window.__evEvidence().find(e => e.evidence_id === 'e-1'),
                })""")
                check("stale_then_refresh_body_recovery",
                      rf.get("ok") is True and rf.get("degraded", 0) >= 1
                      and mid2["counts"]["stale"] >= 1
                      and (mid2["e1"] or {}).get("linkage") == "stale"
                      and (mid2["e2"] or {}).get("linkage") == "valid"
                      and rvy.get("ok") is True
                      and (end2["e1"] or {}).get("linkage") == "valid",
                      json.dumps({"refresh": rf, "mid": {"counts": mid2["counts"],
                                  "e1": (mid2["e1"] or {}).get("linkage"),
                                  "e2": (mid2["e2"] or {}).get("linkage")},
                                  "reverify": rvy, "end_e1": (end2["e1"] or {}).get("linkage"),
                                  "end_counts": end2["counts"]}, ensure_ascii=False))

                # ---- 13. 版本不匹配：v2 正文会话恢复 v1 sidecar → wb_body_stale ----
                fresh2 = {
                    "body": page.evaluate("b => window.__evStartBody(b)", body_v2_b64),
                    "src": page.evaluate("b => window.__evIngestSource(b)", xlsx_b64),
                }
                pre_state = page.evaluate("window.__evState()")
                cb = safe_call(page, "window.__evRestoreSidecar", sc, "changed_body")
                post_state = page.evaluate("window.__evState()")
                pb2 = page.evaluate("j => window.__evProposeBinding(j)", E_QUOTE)
                final_counts = page.evaluate("window.__evCounts()")
                semantics["restore_against_changed_body"] = str(cb["r"].get("code"))
                check("restore_against_changed_body_fails_closed",
                      fresh2["body"].get("ok") is True and fresh2["src"].get("ok") is True
                      and pre_state["evidence"] == 0
                      and cb["threw"] is False and cb["r"].get("ok") is False
                      and cb["r"].get("code") == "wb_body_stale"
                      and bool(cb["r"].get("message"))
                      # 无半途应用：失败后仍是干净的 v2 会话
                      and post_state["active"] is True
                      and post_state["evidence"] == 0
                      and post_state["sources"] == ["src-1"]
                      # 会话在失败后仍可用（v2 正文含 12.5 → 提议通过）
                      and pb2.get("ok") is True and pb2.get("evidence_id") == "e-1"
                      and final_counts["pending"] == 1,
                      json.dumps({"restore": cb, "pre": pre_state, "post": post_state,
                                  "propose": pb2, "counts": final_counts,
                                  "note": "typed wb_body_stale rejection, no partial "
                                          "application, session usable after failure"},
                                 ensure_ascii=False))

                # ---- 14. typed 失败永不抛出（面板契约聚合面）----
                bad = [(lbl, r) for lbl, r in fault_calls
                       if r.get("threw") or not isinstance(r.get("r"), dict)
                       or r["r"].get("ok") is not False
                       or not isinstance(r["r"].get("code"), str) or not r["r"].get("code")
                       or not isinstance(r["r"].get("message"), str) or not r["r"].get("message")]
                wb_codes = [r["r"].get("code") for _, r in fault_calls
                            if str(r["r"].get("code", "")).startswith("wb_")]
                check("typed_failures_never_throw",
                      len(fault_calls) >= 9 and not bad and len(wb_codes) >= 8,
                      json.dumps({"calls": [{"label": lbl, "threw": r.get("threw"),
                                             "code": (r.get("r") or {}).get("code")}
                                            for lbl, r in fault_calls],
                                  "bad": [lbl for lbl, _ in bad],
                                  "wb_codes": wb_codes}, ensure_ascii=False))

            # ---- 15. 零 pageerror（全程无 JS 异常逃逸，含 reload）----
            check("zero_pageerror", len(page_errors) == 0,
                  json.dumps({"pageErrors": page_errors[:5]}, ensure_ascii=False))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"] and not c["skip"]]
    OUT.write_text(json.dumps({
        "meta": {
            "gate": "stage 6 §九.6 sidecar fault paths (web face)",
            "native_twin": "crates/workbench/tests/persistence_recovery.rs",
            "port": PORT,
            "dist": str(WEB / "dist"),
            "fixtures": [str(XLSX_FIXTURE), str(BODY_FIXTURE), str(BODY_V2_FIXTURE)],
            "wasm_workbench_present": has_wasm,
            "observed_fault_codes": semantics,
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
