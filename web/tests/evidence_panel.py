#!/usr/bin/env python3
"""Phase 5 §8.1 evidence panel e2e (workbench web face — the consumption
side of the eight-step evidence loop): serve web/dist, drive the left
evidence sidebar + programmatic window.__ev* probes, assert the session
lifecycle (body start → source ingest → binding proposals → calculation →
recalc cascade → review gate → stale/reverify recovery → sidecar
roundtrip → clean export → claim jump) and typed error surfaces.

The WasmWorkbench wasm handle is being added in parallel (pkg rebuild
pending) — when the ctor is absent from the pkg every wasm-facing case
(2-15) is SKIP (detail: pending wasm), while panel boot (1) and grid
coexistence/zero-pageerror (16) must pass. Same SKIP pattern as
xlsx_grid.py's exportWithEdits bridge.

Fixture = web/tests/fixtures/{evidence_sample.xlsx, evidence_body.docx,
evidence_body_v2.docx} (provenance: office-io write faces via
`cargo run -p mdcore-wasm --example gen_evidence_fixture`).
Evidence -> results/results_evidence_panel.json
"""

import base64
import http.server
import json
import socketserver
import subprocess
import sys
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
FIXTURES = WEB / "tests" / "fixtures"
XLSX_FIXTURE = FIXTURES / "evidence_sample.xlsx"
BODY_FIXTURE = FIXTURES / "evidence_body.docx"
BODY_V2_FIXTURE = FIXTURES / "evidence_body_v2.docx"
OUT = WEB / "tests" / "results" / "results_evidence_panel.json"
FIXTURE_GEN = ["cargo", "run", "-p", "mdcore-wasm", "--example", "gen_evidence_fixture"]

ZERO_COUNTS = {"pending": 0, "approved": 0, "rejected": 0, "stale": 0}

# Proposal JSON faces (workbench contract — spec §八.1)
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


def main() -> int:
    missing = [p for p in (XLSX_FIXTURE, BODY_FIXTURE, BODY_V2_FIXTURE) if not p.exists()]
    if missing:
        print(f"fixture missing {[str(m) for m in missing]} — generating via {FIXTURE_GEN[2]}")
        subprocess.run(FIXTURE_GEN, check=True, cwd=str(WEB.parent))

    checks = []
    page_errors: list[str] = []
    skipped = 0

    def check(name, ok, detail="", skip=False):
        nonlocal skipped
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip), "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        if skip:
            skipped += 1
        print(tag + name + ("  | " + str(detail)[:160] if detail and not ok and not skip else ""))
        return ok

    xlsx_b64 = base64.b64encode(XLSX_FIXTURE.read_bytes()).decode()
    body_b64 = base64.b64encode(BODY_FIXTURE.read_bytes()).decode()
    body_v2_b64 = base64.b64encode(BODY_V2_FIXTURE.read_bytes()).decode()

    with socketserver.TCPServer(("127.0.0.1", 8937), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8937/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            page.wait_for_function("window.__evReady === true", timeout=30000)
            # #43 问题 2：证据面板默认折叠——本套件含面板内真实 UI 点击
            #（.ev-claim），就绪后先展开（探针面不受折叠影响，纯展开态）。
            page.click("#evidence-collapse-btn")

            # ---- 1. ev_ready（面板就绪 + 未激活态按钮禁用——不依赖 wasm 面）----
            boot = page.evaluate("""() => ({
                ready: window.__evReady === true,
                toolbar: !!document.getElementById('ev-toolbar'),
                list: !!document.getElementById('ev-list'),
                report: !!document.getElementById('ev-report'),
                status: !!document.getElementById('ev-status'),
                counts: !!document.getElementById('ev-counts'),
                loadBody: !!document.getElementById('ev-load-body'),
                bodyFile: !!document.getElementById('ev-body-file'),
                sourceFile: !!document.getElementById('ev-source-file'),
                sidecarFile: !!document.getElementById('ev-sidecar-file'),
                loadBodyDisabled: document.getElementById('ev-load-body').disabled,
                addSourceDisabled: document.getElementById('ev-add-source').disabled,
                exportCleanDisabled: document.getElementById('ev-export-clean').disabled,
                exportSidecarDisabled: document.getElementById('ev-export-sidecar').disabled,
                importSidecarDisabled: document.getElementById('ev-import-sidecar').disabled,
                state: window.__evState(),
                counts: window.__evCounts(),
            })""")
            st = boot["state"]
            check("ev_ready",
                  boot["ready"] and boot["toolbar"] and boot["list"] and boot["report"]
                  and boot["status"] and boot["counts"] and boot["loadBody"] and boot["bodyFile"]
                  and boot["sourceFile"] and boot["sidecarFile"]
                  and not boot["loadBodyDisabled"]
                  and boot["addSourceDisabled"] and boot["exportCleanDisabled"]
                  and boot["exportSidecarDisabled"] and boot["importSidecarDisabled"]
                  and st["active"] is False and st["evidence"] == 0
                  and st["sources"] == [] and st["counts"] == ZERO_COUNTS
                  and boot["counts"] == ZERO_COUNTS,
                  json.dumps(boot, ensure_ascii=False))

            # wasm 面在场判定：WasmWorkbench 缺席（pkg 重建未落）→ 2-15 全 SKIP
            has_wasm = page.evaluate("window.__evState().wasm") is True

            def pending_wasm(name):
                check(name, True, "pending wasm (WasmWorkbench absent from pkg — skip until rebuild)",
                      skip=True)

            # ---- 2. ev_start_body（正文导入 → 会话激活；counts 全 0）----
            if not has_wasm:
                pending_wasm("ev_start_body")
            else:
                r = page.evaluate("b64 => window.__evStartBody(b64)", body_b64)
                state = page.evaluate("""() => ({
                    state: window.__evState(),
                    addSourceDisabled: document.getElementById('ev-add-source').disabled,
                    cards: document.querySelectorAll('#ev-list .ev-card').length,
                })""")
                check("ev_start_body",
                      r.get("ok") is True and state["state"]["active"] is True
                      and state["state"]["evidence"] == 0 and state["state"]["sources"] == []
                      and state["state"]["counts"] == ZERO_COUNTS
                      and not state["addSourceDisabled"] and state["cards"] == 0,
                      json.dumps({"r": r, "state": state}, ensure_ascii=False))

            # ---- 3. ev_ingest_source（xlsx 来源注入 → src-1 自增注册）----
            if not has_wasm:
                pending_wasm("ev_ingest_source")
            else:
                r = page.evaluate("b64 => window.__evIngestSource(b64)", xlsx_b64)
                state = page.evaluate("window.__evState()")
                check("ev_ingest_source",
                      r.get("ok") is True and r.get("doc_id") == "src-1"
                      and state["sources"] == ["src-1"],
                      json.dumps({"r": r, "state": state}, ensure_ascii=False))

            # ---- 4. ev_propose_binding（e-quote：direct_quote → pending 卡片）----
            if not has_wasm:
                pending_wasm("ev_propose_binding")
            else:
                r = page.evaluate("j => window.__evProposeBinding(j)", E_QUOTE)
                after = page.evaluate("""() => ({
                    counts: window.__evCounts(),
                    cards: document.querySelectorAll('#ev-list .ev-card').length,
                    pendingBadge: !!document.querySelector('.ev-card[data-id=\"e-1\"] .review-pending'),
                    linkageBadge: !!document.querySelector('.ev-card[data-id=\"e-1\"] .linkage-valid'),
                    ev: window.__evEvidence().find(e => e.evidence_id === 'e-1'),
                })""")
                ev = after["ev"] or {}
                check("ev_propose_binding",
                      r.get("ok") is True and r.get("evidence_id") == "e-1"
                      and after["counts"]["pending"] == 1 and after["cards"] == 1
                      and after["pendingBadge"] and after["linkageBadge"]
                      and ev.get("review") == "pending" and ev.get("linkage") == "valid"
                      and ev.get("relation") == "direct_quote"
                      and ev.get("source_locator", {}).get("kind") == "xlsx_cell"
                      and ev.get("claim_locator", {}).get("kind") == "docx_paragraph",
                      json.dumps({"r": r, "after": after}, ensure_ascii=False))

            # ---- 5. ev_propose_binding_2（e-calc：calculated + calc_ref）----
            if not has_wasm:
                pending_wasm("ev_propose_binding_2")
            else:
                r = page.evaluate("j => window.__evProposeBinding(j)", E_CALC)
                after = page.evaluate("""() => ({
                    counts: window.__evCounts(),
                    cards: document.querySelectorAll('#ev-list .ev-card').length,
                    ev: window.__evEvidence().find(e => e.evidence_id === 'e-2'),
                })""")
                ev = after["ev"] or {}
                check("ev_propose_binding_2",
                      r.get("ok") is True and r.get("evidence_id") == "e-2"
                      and after["counts"]["pending"] == 2 and after["cards"] == 2
                      and ev.get("relation") == "calculated"
                      and ev.get("calc_ref") == "calc-1"
                      and ev.get("calc") == "not_applicable",
                      json.dumps({"r": r, "after": after}, ensure_ascii=False))

            # ---- 6. ev_propose_calc（qty * price → 4210.000）----
            if not has_wasm:
                pending_wasm("ev_propose_calc")
            else:
                r = page.evaluate("j => window.__evProposeCalc(j)", CALC_1)
                check("ev_propose_calc",
                      r.get("ok") is True and r.get("value") == "4210.000",
                      json.dumps(r, ensure_ascii=False))

            # ---- 7. ev_recalc（recalc_current → e-calc calc=passed 级联）----
            if not has_wasm:
                pending_wasm("ev_recalc")
            else:
                r = page.evaluate("c => window.__evRecalc(c)", "calc-1")
                after = page.evaluate("""() => ({
                    ev: window.__evEvidence().find(e => e.evidence_id === 'e-2'),
                    badge: !!document.querySelector('.ev-card[data-id=\"e-2\"] .calc-passed'),
                })""")
                ev = after["ev"] or {}
                check("ev_recalc",
                      r.get("ok") is True and ev.get("calc") == "passed"
                      and ev.get("linkage") == "valid" and after["badge"],
                      json.dumps({"r": r, "after": after}, ensure_ascii=False))

            # ---- 8. ev_review_gate（首次过审 ok；重复过审 → wb_* 拒绝）----
            if not has_wasm:
                pending_wasm("ev_review_gate")
            else:
                r1 = page.evaluate("([id, d]) => window.__evReview(id, d)", ["e-2", "approved"])
                r2 = page.evaluate("([id, d]) => window.__evReview(id, d)", ["e-2", "approved"])
                badge = page.evaluate(
                    "!!document.querySelector('.ev-card[data-id=\"e-2\"] .review-approved')")
                check("ev_review_gate",
                      r1.get("ok") is True and r2.get("ok") is False
                      and str(r2.get("code", "")).startswith("wb_") and badge,
                      json.dumps({"r1": r1, "r2": r2, "badge": badge}, ensure_ascii=False))

            # ---- 9. ev_review_reject（e-quote 拒绝 → rejected 徽标）----
            if not has_wasm:
                pending_wasm("ev_review_reject")
            else:
                r = page.evaluate("([id, d]) => window.__evReview(id, d)", ["e-1", "rejected"])
                after = page.evaluate("""() => ({
                    counts: window.__evCounts(),
                    badge: !!document.querySelector('.ev-card[data-id=\"e-1\"] .review-rejected'),
                    ev: window.__evEvidence().find(e => e.evidence_id === 'e-1'),
                })""")
                check("ev_review_reject",
                      r.get("ok") is True and after["badge"]
                      and after["counts"] == {"pending": 0, "approved": 1, "rejected": 1, "stale": 0}
                      and (after["ev"] or {}).get("review") == "rejected",
                      json.dumps({"r": r, "after": after}, ensure_ascii=False))

            # ---- 10. ev_reverify（refresh_body 制造 stale → 重验恢复 valid）----
            if not has_wasm:
                pending_wasm("ev_reverify")
            else:
                r = page.evaluate("b64 => window.__evRefreshBody(b64)", body_v2_b64)
                mid = page.evaluate("""() => ({
                    counts: window.__evCounts(),
                    staleBadge: !!document.querySelector('.ev-card[data-id=\"e-1\"] .linkage-stale'),
                    ev1: window.__evEvidence().find(e => e.evidence_id === 'e-1'),
                    ev2: window.__evEvidence().find(e => e.evidence_id === 'e-2'),
                })""")
                rv = page.evaluate("id => window.__evReverify(id)", "e-1")
                end = page.evaluate("""() => ({
                    counts: window.__evCounts(),
                    ev1: window.__evEvidence().find(e => e.evidence_id === 'e-1'),
                    validBadge: !!document.querySelector('.ev-card[data-id=\"e-1\"] .linkage-valid'),
                })""")
                check("ev_reverify",
                      r.get("ok") is True and r.get("degraded", 0) >= 1
                      and mid["counts"]["stale"] == 1 and mid["staleBadge"]
                      and (mid["ev1"] or {}).get("linkage") == "stale"
                      and (mid["ev2"] or {}).get("linkage") == "valid"
                      and rv.get("ok") is True
                      and (end["ev1"] or {}).get("linkage") == "valid"
                      and end["validBadge"] and end["counts"]["stale"] == 0,
                      json.dumps({"refresh": r, "mid": mid, "reverify": rv, "end": end},
                                 ensure_ascii=False))

            # ---- 11. ev_counts/report（counts 形态 + 报告面非空）----
            if not has_wasm:
                pending_wasm("ev_counts_report")
            else:
                counts = page.evaluate("window.__evCounts()")
                report = page.evaluate("window.__evReport()")
                has_marker = ("e-1" in report and ("e-2" in report)
                              and ("Evidence" in report or "证据" in report))
                check("ev_counts_report",
                      counts == {"pending": 0, "approved": 1, "rejected": 1, "stale": 0}
                      and isinstance(report, str) and len(report) > 0 and has_marker,
                      json.dumps({"counts": counts, "reportHead": report[:200]},
                                 ensure_ascii=False))

            # ---- 12. ev_sidecar_roundtrip（sidecar 结构 + 恢复会话）----
            if not has_wasm:
                pending_wasm("ev_sidecar_roundtrip")
            else:
                sc = page.evaluate("window.__evSidecar()")
                parsed = json.loads(sc)
                keys = {"schema_version", "document", "source_docs", "evidence",
                        "calculations", "idempotency", "audit"}
                audit = parsed.get("audit") or []
                seqs = [e["seq"] for e in audit]
                mono = all(a < b for a, b in zip(seqs, seqs[1:]))
                pre_review = {e.get("evidence_id"): e.get("review")
                              for e in (parsed.get("evidence") or [])}
                r = page.evaluate("j => window.__evRestoreSidecar(j)", sc)
                after = page.evaluate("""() => ({
                    evidence: window.__evEvidence().length,
                    state: window.__evState(),
                    review: Object.fromEntries(window.__evEvidence()
                        .map(e => [e.evidence_id, e.review])),
                })""")
                check("ev_sidecar_roundtrip",
                      keys.issubset(set(parsed.keys()))
                      and parsed["schema_version"] == 1
                      and len(parsed["evidence"]) == 2
                      and len(parsed["calculations"]) == 1
                      and len(parsed["source_docs"]) == 1
                      and len(audit) >= 1 and mono
                      and r.get("ok") is True
                      and after["evidence"] == 2
                      and after["state"]["active"] is True
                      and after["state"]["sources"] == ["src-1"]
                      # N1: review states survive the restore roundtrip.
                      and after["review"] == pre_review,
                      json.dumps({"keys": sorted(parsed.keys()),
                                  "evidence": len(parsed.get("evidence") or []),
                                  "audit": len(audit), "mono": mono,
                                  "restore": r, "after": after}, ensure_ascii=False))

            # ---- 13. ev_export_clean（干净正文导出 → PK zip 字节）----
            if not has_wasm:
                pending_wasm("ev_export_clean")
            else:
                r = page.evaluate("window.__evExportClean()")
                check("ev_export_clean",
                      r.get("ok") is True and r.get("bytes", "").startswith("UEsD")
                      and r.get("len", 0) > 0,
                      json.dumps({"ok": r.get("ok"), "len": r.get("len"),
                                  "head": str(r.get("bytes", ""))[:16]}, ensure_ascii=False))

            # ---- 14. ev_jump（claim 点击 → paragraph_index 跳转事件记录）----
            if not has_wasm:
                pending_wasm("ev_jump")
            else:
                page.click('.ev-card[data-id="e-1"] .ev-claim')
                log = page.evaluate("window.__evJumpLog()")
                check("ev_jump", log == [2], json.dumps(log))

            # ---- 15. ev_error_faces（垃圾字节/非 JSON → typed 码；会话保持）----
            if not has_wasm:
                pending_wasm("ev_error_faces")
            else:
                bad = page.evaluate("b64 => window.__evStartBody(b64)",
                                    base64.b64encode(b"hi").decode())
                still = page.evaluate("""() => ({
                    evidence: window.__evEvidence().length,
                    active: window.__evState().active,
                })""")
                bad_prop = page.evaluate("j => window.__evProposeBinding(j)", "not json")
                err_status = page.evaluate(
                    "document.getElementById('ev-status').classList.contains('ev-error')")
                check("ev_error_faces",
                      bad.get("ok") is False and str(bad.get("code", "")).startswith("wb_")
                      and bool(bad.get("message"))
                      and still["active"] is True and still["evidence"] == 2
                      and bad_prop.get("ok") is False
                      and str(bad_prop.get("code", "")).startswith("wb_")
                      and err_status,
                      json.dumps({"bad": bad, "still": still, "badProp": bad_prop},
                                 ensure_ascii=False))

            # ---- 16. 契约面 + grid 共存 + 零页错（全程无 JS 异常逃逸）----
            page.click("#tab-grid")
            grid_vis = page.evaluate("""() => ({
                panel: getComputedStyle(document.getElementById('grid-panel')).display,
                editor: getComputedStyle(document.getElementById('editor')).display,
                evidence: getComputedStyle(document.getElementById('evidence-panel')).display,
            })""")
            page.click("#tab-doc")
            doc_vis = page.evaluate("""() => ({
                editor: getComputedStyle(document.getElementById('editor')).display,
                panel: getComputedStyle(document.getElementById('grid-panel')).display,
                gridReady: window.__gridReady === true,
                evReady: window.__evReady === true,
                contract: window.__contractVersion,
                md: typeof window.__md,
            })""")
            check("ev_coexistence_no_pageerror",
                  grid_vis["panel"] != "none" and grid_vis["editor"] == "none"
                  and grid_vis["evidence"] != "none"
                  and doc_vis["editor"] != "none" and doc_vis["panel"] == "none"
                  and doc_vis["gridReady"] and doc_vis["evReady"]
                  and doc_vis["contract"] == 1.4 and doc_vis["md"] == "function"
                  and len(page_errors) == 0,
                  json.dumps({"grid": grid_vis, "doc": doc_vis,
                              "pageErrors": page_errors[:3]}, ensure_ascii=False))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "phase 5 §8.1 evidence panel e2e", "dist": str(WEB / "dist"),
                 "fixtures": [str(XLSX_FIXTURE), str(BODY_FIXTURE), str(BODY_V2_FIXTURE)],
                 "wasm_workbench_present": has_wasm},
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
