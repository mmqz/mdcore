#!/usr/bin/env python3
"""Phase 5 §8.5 Canvas 表面视图 e2e：SVG 保留模式显示面（分层渲染/拖拽
预览/选中/删除/行内编辑/分组/工具条）+ WasmCanvas（mdc-canvas 会话，
web/pkg/mdcore_wasm.js）模型面（八命令/双撤销栈/事件轮询/快照）+
同页双实例直连协同（事件 updates → apply_update 互同步 + encode_diff
追赶——AI 端只读镜像面板）。

serve web/dist（8939），驱动 #tab-canvas / #canvas-panel 与
window.__canvas* 探针。拖拽用 CDP 真实鼠标事件（page.mouse——
pointerdown O(n) 命中 → DOM 预览 → pointerup 单次 move_node 提交）。

pkg 未含 WasmCanvas（mdc-canvas wasm 模块并行实施中）时：面板照常启动
（工具条禁用 + typed bridge_absent），会话依赖用例（2-13、15）按
__canvasState().wasm SKIP 门跳过（照 evidence_panel.py 模式）；1（就绪）
与 14（四 tab 共存 + 零页错）恒跑。wasm 在场前的全路径验证 = 契约 mock
干跑（临时路由注入——验证后删除，照 pdf_view 先例）。

结果 -> tests/results/results_canvas_view.json
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
OUT = WEB / "tests" / "results" / "results_canvas_view.json"
PORT = 8939

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update(
    {".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm",
     ".mjs": "text/javascript"}
)


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True


def main() -> int:
    checks = []
    page_errors: list[str] = []
    skipped = 0

    def check(name, ok, detail="", skip=False):
        nonlocal skipped
        checks.append({"name": name, "ok": bool(ok), "skip": bool(skip),
                       "detail": str(detail)[:400]})
        tag = "SKIP " if skip else ("PASS " if ok else "FAIL ")
        if skip:
            skipped += 1
        print(tag + name + ("  | " + str(detail)[:160] if detail and not ok and not skip else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", PORT), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto(f"http://127.0.0.1:{PORT}/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)
            page.wait_for_function("window.__canvasReady === true", timeout=30000)

            # ---- 1. canvas_ready（tab + 面板 + 工具条 + SVG 容器 + 状态行）----
            boot = page.evaluate("""() => ({
                ready: window.__canvasReady === true,
                tab: !!document.getElementById('tab-canvas'),
                panel: !!document.getElementById('canvas-panel'),
                toolbar: !!document.getElementById('canvas-toolbar'),
                addNode: !!document.getElementById('canvas-add-node-btn'),
                edgeMode: !!document.getElementById('canvas-edge-mode-btn'),
                group: !!document.getElementById('canvas-group-btn'),
                ungroup: !!document.getElementById('canvas-ungroup-btn'),
                undo: !!document.getElementById('canvas-undo-btn'),
                redo: !!document.getElementById('canvas-redo-btn'),
                aiAdd: !!document.getElementById('canvas-ai-add-btn'),
                save: !!document.getElementById('canvas-save-btn'),
                restore: !!document.getElementById('canvas-restore-btn'),
                clear: !!document.getElementById('canvas-clear-btn'),
                scroll: !!document.getElementById('canvas-scroll'),
                svg: !!document.getElementById('canvas-svg'),
                aiPanel: !!document.getElementById('canvas-ai-panel'),
                aiSvg: !!document.getElementById('canvas-ai-svg'),
                aiAdd2: !!document.getElementById('canvas-ai-add-btn2'),
                status: !!document.getElementById('canvas-status'),
                panelHidden: getComputedStyle(document.getElementById('canvas-panel')).display === 'none',
                state: window.__canvasState(),
            })""")
            st = boot["state"]
            check("canvas_ready",
                  boot["ready"] and boot["tab"] and boot["panel"] and boot["toolbar"]
                  and boot["addNode"] and boot["edgeMode"] and boot["group"] and boot["ungroup"]
                  and boot["undo"] and boot["redo"] and boot["aiAdd"] and boot["save"]
                  and boot["restore"] and boot["clear"] and boot["scroll"] and boot["svg"]
                  and boot["aiPanel"] and boot["aiSvg"] and boot["aiAdd2"] and boot["status"]
                  and boot["panelHidden"]
                  and st["objects"] == 0 and st["nodes"] == 0 and st["edges"] == 0
                  and st["groups"] == 0 and st["undoDepth"] == 0 and st["redoDepth"] == 0,
                  json.dumps(boot, ensure_ascii=False))

            # 切到 canvas 面（后续用例的公共前置）
            page.click("#tab-canvas")
            page.wait_for_function(
                "getComputedStyle(document.getElementById('canvas-panel')).display !== 'none'")

            # wasm 面在场判定：pkg 未含 WasmCanvas（并行实施）→ 2-13、15 SKIP
            has_wasm = page.evaluate("window.__canvasState().wasm") is True

            def pending_wasm(name):
                check(name, True, "pending wasm (WasmCanvas absent — skip until mdc-canvas wasm rebuild)",
                      skip=True)

            def obj(page, oid, side=None):
                args = [oid] if side is None else [oid, side]
                return page.evaluate(
                    """([id, s]) => (s ? window.__canvasObjects(s) : window.__canvasObjects())
                       .find(o => o.id === id)""", args)

            if not has_wasm:
                for n in ["canvas_add_node", "canvas_add_edge", "canvas_drag_commit",
                          "canvas_undo_redo", "canvas_delete", "canvas_group",
                          "canvas_props_edit", "canvas_fingerprint_stale",
                          "canvas_snapshot_roundtrip", "canvas_sync_convergence",
                          "canvas_remote_not_in_undo", "canvas_error_faces",
                          "canvas_perf_smoke"]:
                    pending_wasm(n)
            else:
                # ---- 2. canvas_add_node（程序化加节点 → 模型 + SVG DOM + label）----
                r1 = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                   [40, 80, "Quote"])
                after = page.evaluate("""() => ({
                    state: window.__canvasState(),
                    objects: window.__canvasObjects(),
                    domNodes: document.querySelectorAll('#cv-l-nodes > g.node').length,
                    gTransform: document.querySelector('#cv-l-nodes > g.node')?.getAttribute('transform'),
                    gText: document.querySelector('#cv-l-nodes > g.node text')?.textContent,
                })""")
                objs = after["objects"]
                check("canvas_add_node",
                      r1.get("ok") is True and bool(r1.get("id"))
                      and after["state"]["objects"] == 1 and after["state"]["nodes"] == 1
                      and after["state"]["undoDepth"] == 1
                      and len(objs) == 1 and objs[0]["kind"] == "node"
                      and objs[0]["x"] == 40 and objs[0]["y"] == 80
                      and objs[0]["w"] == 120 and objs[0]["h"] == 36
                      and objs[0]["props"].get("label") == "Quote"
                      and after["domNodes"] == 1
                      and after["gTransform"] == "translate(40,80)"
                      and after["gText"] == "Quote",
                      json.dumps({"r1": r1, "after": after}, ensure_ascii=False))
                n1 = r1["id"]

                # ---- 3. canvas_add_edge（两节点 + add_edge → line + from/to）----
                r2 = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                   [300, 80, "Bid"])
                r3 = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                   [520, 160, "Win"])
                n2, n3 = r2["id"], r3["id"]
                re_ = page.evaluate(
                    """([a, b]) => window.__canvasExecute('human',
                        JSON.stringify({t: 'add_edge', id: null, from: a, to: b, props: {}}))""",
                    [n1, n2])
                after = page.evaluate("""() => ({
                    state: window.__canvasState(),
                    edges: window.__canvasObjects().filter(o => o.kind === 'edge'),
                    lines: [...document.querySelectorAll('#cv-l-edges > line.edge')].map(l => ({
                        id: l.dataset.id, x1: l.getAttribute('x1'), y1: l.getAttribute('y1'),
                        x2: l.getAttribute('x2'), y2: l.getAttribute('y2'),
                        dangling: l.classList.contains('dangling'),
                    })),
                })""")
                ok3 = (re_.get("ok") is True and re_.get("receipt", {}).get("outcome") == "edge_added"
                       and after["state"]["edges"] == 1
                       and len(after["edges"]) == 1
                       and after["edges"][0]["from"] == n1 and after["edges"][0]["to"] == n2
                       and len(after["lines"]) == 1
                       and after["lines"][0]["id"] == re_["receipt"]["id"]
                       # bench 口径：端点 = 节点左上 +60/+18（120×36 = 中心）
                       and after["lines"][0]["x1"] == "100" and after["lines"][0]["y1"] == "98"
                       and after["lines"][0]["x2"] == "360" and after["lines"][0]["y2"] == "98"
                       and not after["lines"][0]["dangling"])
                check("canvas_add_edge", ok3,
                      json.dumps({"r": re_, "after": after}, ensure_ascii=False))
                e1 = re_["receipt"]["id"]

                # ---- 4. canvas_drag_commit（CDP 真实拖拽 → move_node 单次提交）----
                before = obj(page, n1)
                depth_before = page.evaluate("window.__canvasState().undoDepth")
                g = page.locator(f'#cv-l-nodes > g.node[data-id="{n1}"]')
                box = g.bounding_box()
                cx, cy = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
                page.mouse.move(cx, cy)
                page.mouse.down()
                page.mouse.move(cx + 60, cy + 40, steps=4)
                page.mouse.up()
                moved = obj(page, n1)
                depth_after = page.evaluate("window.__canvasState().undoDepth")
                # 拖拽 = 一次 move_node（LWW 提交粒度）；delta 精确（取整提交）
                ok4 = (moved is not None
                       and abs(moved["x"] - (before["x"] + 60)) <= 1
                       and abs(moved["y"] - (before["y"] + 40)) <= 1
                       and depth_after == depth_before + 1)
                # 相连边跟随（端点 = 新位 +60/+18）
                line = page.evaluate(
                    """id => { const l = document.querySelector(`#cv-l-edges > line[data-id="${id}"]`);
                              return l ? {x1: l.getAttribute('x1'), y1: l.getAttribute('y1')} : null; }""",
                    e1)
                ok4 = ok4 and line is not None and abs(float(line["x1"]) - (moved["x"] + 60)) <= 1
                check("canvas_drag_commit", ok4,
                      json.dumps({"before": before, "moved": moved,
                                  "depth": [depth_before, depth_after], "line": line},
                                 ensure_ascii=False))

                # ---- 5. canvas_undo_redo（撤销回原位 + 重做到新位）----
                u = page.evaluate("window.__canvasUndo()")
                undone = obj(page, n1)
                dom_t = page.evaluate(
                    """id => document.querySelector(`#cv-l-nodes > g.node[data-id="${id}"]`)
                             ?.getAttribute('transform')""", n1)
                page.evaluate("window.__canvasRedo()")
                redone = obj(page, n1)
                check("canvas_undo_redo",
                      u is True and undone["x"] == before["x"] and undone["y"] == before["y"]
                      and dom_t == f"translate({before['x']},{before['y']})"
                      and abs(redone["x"] - moved["x"]) <= 1
                      and abs(redone["y"] - moved["y"]) <= 1,
                      json.dumps({"u": u, "undone": undone, "dom": dom_t, "redone": redone},
                                 ensure_ascii=False))

                # ---- 6. canvas_delete（选中 + Delete → 删除；相连边悬空暴露）----
                g = page.locator(f'#cv-l-nodes > g.node[data-id="{n1}"]')
                box = g.bounding_box()
                page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2)
                sel = page.evaluate(
                    """id => document.querySelector(`#cv-l-nodes > g.node[data-id="${id}"]`)
                             ?.classList.contains('selected')""", n1)
                count_before = page.evaluate("window.__canvasState().objects")
                page.keyboard.press("Delete")
                after = page.evaluate(
                    """([n1, e1]) => ({
                        state: window.__canvasState(),
                        gone: !document.querySelector(`#cv-l-nodes > g.node[data-id="${n1}"]`),
                        edge: window.__canvasObjects().find(o => o.kind === 'edge'),
                        lineDangling: document.querySelector(`#cv-l-edges > line[data-id="${e1}"]`)
                                       ?.classList.contains('dangling'),
                    })""", [n1, e1])
                # canvas 语义：删除节点不级联删边——读模型暴露悬空（消费侧检测面）
                check("canvas_delete",
                      sel is True and after["state"]["objects"] == count_before - 1
                      and after["state"]["nodes"] == 2
                      and after["gone"] is True
                      and after["edge"] is not None
                      and after["edge"]["from"] == n1 and after["edge"]["to"] == n2
                      and after["lineDangling"] is True,
                      json.dumps({"sel": sel, "count_before": count_before, "after": after},
                                 ensure_ascii=False))

                # ---- 7. canvas_group（双选 → group → 成员入组；ungroup → 成员回）----
                ra = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)", [80, 300, "GA"])
                rb = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)", [320, 300, "GB"])
                na, nb = ra["id"], rb["id"]
                ga = page.locator(f'#cv-l-nodes > g.node[data-id="{na}"]')
                gb = page.locator(f'#cv-l-nodes > g.node[data-id="{nb}"]')
                ba, bb = ga.bounding_box(), gb.bounding_box()
                page.mouse.click(ba["x"] + ba["width"] / 2, ba["y"] + ba["height"] / 2)
                # shift+click = 多选（page.mouse 无 modifiers 形参——keyboard 压键）
                page.keyboard.down("Shift")
                page.mouse.click(bb["x"] + bb["width"] / 2, bb["y"] + bb["height"] / 2)
                page.keyboard.up("Shift")
                selboth = page.evaluate(
                    """([a, b]) => ({
                        sa: document.querySelector(`#cv-l-nodes > g.node[data-id="${a}"]`)
                               ?.classList.contains('selected'),
                        sb: document.querySelector(`#cv-l-nodes > g.node[data-id="${b}"]`)
                               ?.classList.contains('selected'),
                    })""", [na, nb])
                page.click("#canvas-group-btn")
                after = page.evaluate("""() => ({
                    state: window.__canvasState(),
                    groups: window.__canvasObjects().filter(o => o.kind === 'group'),
                    domGroup: !!document.querySelector('#cv-l-groups > g.group rect'),
                })""")
                ok7 = (selboth["sa"] is True and selboth["sb"] is True
                       and after["state"]["groups"] == 1
                       and len(after["groups"]) == 1
                       and sorted(after["groups"][0]["members"]) == sorted([na, nb])
                       and after["domGroup"] is True)
                gid = after["groups"][0]["id"] if after["groups"] else None
                # 组内空区点击 → 选组 → ungroup
                pt = page.evaluate("""([x, y]) => {
                    const sc = document.getElementById('canvas-scroll');
                    const r = sc.getBoundingClientRect();
                    return { x: r.left + x - sc.scrollLeft, y: r.top + y - sc.scrollTop };
                }""", [100, 294])  # bbox(68,288) 内、节点上方
                page.mouse.click(pt["x"], pt["y"])
                gsel = page.evaluate(
                    """id => document.querySelector(`#cv-l-groups > g.group[data-id="${id}"]`)
                             ?.classList.contains('selected')""", gid) if gid else None
                page.click("#canvas-ungroup-btn")
                after2 = page.evaluate("""() => ({
                    groups: window.__canvasObjects().filter(o => o.kind === 'group').length,
                    nodes: window.__canvasObjects().filter(o => o.kind === 'node').length,
                })""")
                ok7 = ok7 and gsel is True and after2["groups"] == 0 and after2["nodes"] == 4
                check("canvas_group", ok7,
                      json.dumps({"selboth": selboth, "after": after, "gid": gid,
                                  "gsel": gsel, "after2": after2}, ensure_ascii=False))

                # ---- 8. canvas_props_edit（set_props 终态替换 + 行内编辑）----
                rc = page.evaluate("([x, y, l]) => window.__canvasAddNode(x, y, l)",
                                   [80, 420, "Editable"])
                nc = rc["id"]
                rp = page.evaluate(
                    """([id, props]) => window.__canvasExecute('human',
                        JSON.stringify({t: 'set_props', id, props}))""",
                    [nc, {"label": "Renamed", "color": "#112233"}])
                t1 = page.evaluate(
                    """id => document.querySelector(`#cv-l-nodes > g.node[data-id="${id}"] text`)
                             ?.textContent""", nc)
                o1 = obj(page, nc)
                rp2 = page.evaluate(
                    """([id, props]) => window.__canvasExecute('human',
                        JSON.stringify({t: 'set_props', id, props}))""",
                    [nc, {"label": "Final"}])
                t2 = page.evaluate(
                    """id => document.querySelector(`#cv-l-nodes > g.node[data-id="${id}"] text`)
                             ?.textContent""", nc)
                o2 = obj(page, nc)
                # 终态替换：旧键清除（color 不在第二set中即消失）
                # 行内编辑（双击 → input → Enter → set_props 提交）
                gc = page.locator(f'#cv-l-nodes > g.node[data-id="{nc}"]')
                bc = gc.bounding_box()
                page.mouse.dblclick(bc["x"] + bc["width"] / 2, bc["y"] + bc["height"] / 2)
                page.wait_for_selector("#canvas-edit-input", timeout=5000)
                page.fill("#canvas-edit-input", "Edited")
                page.press("#canvas-edit-input", "Enter")
                o3 = obj(page, nc)
                input_gone = page.evaluate(
                    "!document.getElementById('canvas-edit-input')")
                check("canvas_props_edit",
                      rp.get("ok") is True and rp2.get("ok") is True
                      and t1 == "Renamed" and o1["props"].get("color") == "#112233"
                      and t2 == "Final" and o2["props"] == {"label": "Final"}
                      and o3 is not None and o3["props"].get("label") == "Edited"
                      and input_gone,
                      json.dumps({"rp": rp, "t1": t1, "o1p": o1 and o1["props"],
                                  "t2": t2, "o2p": o2 and o2["props"],
                                  "o3p": o3 and o3["props"], "input_gone": input_gone},
                                 ensure_ascii=False))

                # ---- 9. canvas_fingerprint_stale（move 后指纹变化——证据关联钩子）----
                f1 = page.evaluate("id => window.__canvasFingerprint(id)", nc)
                page.evaluate("([id, x, y]) => window.__canvasMoveNode(id, x, y)",
                              [nc, o3["x"] + 150, o3["y"] + 50])
                f2 = page.evaluate("id => window.__canvasFingerprint(id)", nc)
                page.evaluate("([id, x, y]) => window.__canvasMoveNode(id, x, y)",
                              [nc, o3["x"], o3["y"]])
                f3 = page.evaluate("id => window.__canvasFingerprint(id)", nc)
                check("canvas_fingerprint_stale",
                      f1.get("ok") is True and f2.get("ok") is True and f3.get("ok") is True
                      and f1["fp"].startswith("fnv64:") and f2["fp"].startswith("fnv64:")
                      and f1["fp"] != f2["fp"] and f3["fp"] == f1["fp"],
                      json.dumps({"f1": f1, "f2": f2, "f3": f3}, ensure_ascii=False))

                # ---- 10. canvas_snapshot_roundtrip（快照 → restore → objects 相等）----
                count10 = page.evaluate("window.__canvasState().objects")
                rr = page.evaluate("window.__canvasSnapshotRoundtrip()")
                check("canvas_snapshot_roundtrip",
                      rr.get("ok") is True and len(rr.get("objects", [])) == count10,
                      json.dumps({"rr": {k: rr.get(k) for k in ("ok", "code", "message")},
                                  "objects": len(rr.get("objects", [])), "count": count10},
                                 ensure_ascii=False))

                # ---- 11. canvas_sync_convergence（双实例直连协同核心验证）----
                s0 = page.evaluate("window.__canvasSyncState()")
                rai = page.evaluate("window.__canvasAiAddNode()")
                ai_id = rai.get("id")
                main_has = page.evaluate(
                    "id => window.__canvasObjects().some(o => o.id === id)", ai_id)
                ai_has = page.evaluate(
                    "id => window.__canvasObjects('ai').some(o => o.id === id)", ai_id)
                s1 = page.evaluate("window.__canvasSyncState()")
                # 主端事件轮询发现 remote 更新（AI 端命令经 apply_update 入主端）
                ev_log = page.evaluate("window.__canvasEvents(100)")
                remote_on_main = any(
                    e["side"] == "main" and e["source"] == "remote" for e in ev_log)
                # 主端拖动 AI 节点 → AI 端跟随（位置同步）
                page.evaluate("([id, x, y]) => window.__canvasMoveNode(id, x, y)",
                              [ai_id, 700, 500])
                ai_view = page.evaluate(
                    "id => window.__canvasObjects('ai').find(o => o.id === id)", ai_id)
                s2 = page.evaluate("window.__canvasSyncState()")
                check("canvas_sync_convergence",
                      s0["converged"] is True and rai.get("ok") is True and bool(ai_id)
                      and main_has is True and ai_has is True
                      and s1["converged"] is True and s1["main"] == s1["ai"]
                      and remote_on_main is True
                      and ai_view is not None and ai_view["x"] == 700 and ai_view["y"] == 500
                      and s2["converged"] is True,
                      json.dumps({"s0": s0, "rai": rai, "main_has": main_has,
                                  "ai_has": ai_has, "s1": s1, "remote_on_main": remote_on_main,
                                  "ai_view": ai_view, "s2": s2}, ensure_ascii=False))

                # ---- 12. canvas_remote_not_in_undo（远端应用不入本地撤销栈）----
                d0 = page.evaluate("window.__canvasState().undoDepth")
                c0 = page.evaluate("window.__canvasState().objects")
                rai2 = page.evaluate("window.__canvasAiAddNode()")
                d1 = page.evaluate("window.__canvasState().undoDepth")
                c1 = page.evaluate("window.__canvasState().objects")
                s3 = page.evaluate("window.__canvasSyncState()")
                check("canvas_remote_not_in_undo",
                      rai2.get("ok") is True and d1 == d0 and c1 == c0 + 1
                      and s3["converged"] is True,
                      json.dumps({"d0": d0, "d1": d1, "c0": c0, "c1": c1, "s3": s3},
                                 ensure_ascii=False))

                # ---- 13. canvas_error_faces（NaN 坐标/不存在 id/重复 id）----
                cnt = page.evaluate("window.__canvasState().objects")
                e1 = page.evaluate(
                    "() => window.__canvasExecute('human', "
                    "'{\"t\":\"add_node\",\"id\":null,\"x\":1e999,\"y\":10,\"w\":50,\"h\":50,\"props\":{}}')")
                cnt1 = page.evaluate("window.__canvasState().objects")
                e2 = page.evaluate(
                    "() => window.__canvasExecute('human', "
                    "'{\"t\":\"move_node\",\"id\":\"nope-404\",\"x\":1,\"y\":1}')")
                cnt2 = page.evaluate("window.__canvasState().objects")
                ea = page.evaluate(
                    "() => window.__canvasExecute('human', "
                    "'{\"t\":\"add_node\",\"id\":\"dup-test-1\",\"x\":10,\"y\":10,\"w\":50,\"h\":50,\"props\":{}}')")
                cnta = page.evaluate("window.__canvasState().objects")
                eb = page.evaluate(
                    "() => window.__canvasExecute('human', "
                    "'{\"t\":\"add_node\",\"id\":\"dup-test-1\",\"x\":20,\"y\":10,\"w\":50,\"h\":50,\"props\":{}}')")
                cntb = page.evaluate("window.__canvasState().objects")
                check("canvas_error_faces",
                      # NaN 在 JSON 层即非法（serde 先拒）——JSON 边界的
                      # 非有限值防线 = cv_bad_json（cv_non_finite 是 Rust
                      # 直构 API 的语义层防线，无法经 JSON 触达——诚实分层）。
                      e1.get("ok") is False and e1.get("code") == "cv_bad_json"
                      and e2.get("ok") is False and e2.get("code") == "cv_object_not_found"
                      and ea.get("ok") is True
                      and eb.get("ok") is False and eb.get("code") == "cv_duplicate_id"
                      and cnt1 == cnt and cnt2 == cnt and cnta == cnt + 1 and cntb == cnta,
                      json.dumps({"e1": e1, "e2": e2, "ea": ea, "eb": eb,
                                  "counts": [cnt, cnt1, cnt2, cnta, cntb]},
                                 ensure_ascii=False))

            # ---- 14. canvas_coexistence（四 tab 切换 + 就绪旗标 + 零页错）----
            vis = {}
            page.click("#tab-doc")
            vis["doc"] = page.evaluate("""() => ({
                editor: getComputedStyle(document.getElementById('editor')).display,
                grid: getComputedStyle(document.getElementById('grid-panel')).display,
                pdf: getComputedStyle(document.getElementById('pdf-panel')).display,
                canvas: getComputedStyle(document.getElementById('canvas-panel')).display,
            })""")
            page.click("#tab-grid")
            vis["grid"] = page.evaluate("""() => ({
                editor: getComputedStyle(document.getElementById('editor')).display,
                grid: getComputedStyle(document.getElementById('grid-panel')).display,
                pdf: getComputedStyle(document.getElementById('pdf-panel')).display,
                canvas: getComputedStyle(document.getElementById('canvas-panel')).display,
            })""")
            page.click("#tab-pdf")
            vis["pdf"] = page.evaluate("""() => ({
                editor: getComputedStyle(document.getElementById('editor')).display,
                grid: getComputedStyle(document.getElementById('grid-panel')).display,
                pdf: getComputedStyle(document.getElementById('pdf-panel')).display,
                canvas: getComputedStyle(document.getElementById('canvas-panel')).display,
            })""")
            page.click("#tab-canvas")
            vis["canvas"] = page.evaluate("""() => ({
                editor: getComputedStyle(document.getElementById('editor')).display,
                grid: getComputedStyle(document.getElementById('grid-panel')).display,
                pdf: getComputedStyle(document.getElementById('pdf-panel')).display,
                canvas: getComputedStyle(document.getElementById('canvas-panel')).display,
                activeTab: document.getElementById('tab-canvas').classList.contains('active'),
                flags: {
                    ready: window.__ready === true,
                    grid: window.__gridReady === true,
                    ev: window.__evReady === true,
                    pdf: window.__pdfReady === true,
                    canvas: window.__canvasReady === true,
                },
                contract: window.__contractVersion,
                md: typeof window.__md,
            })""")
            check("canvas_coexistence",
                  vis["doc"]["editor"] != "none" and vis["doc"]["grid"] == "none"
                  and vis["doc"]["pdf"] == "none" and vis["doc"]["canvas"] == "none"
                  and vis["grid"]["grid"] != "none" and vis["grid"]["editor"] == "none"
                  and vis["grid"]["pdf"] == "none" and vis["grid"]["canvas"] == "none"
                  and vis["pdf"]["pdf"] != "none" and vis["pdf"]["editor"] == "none"
                  and vis["pdf"]["grid"] == "none" and vis["pdf"]["canvas"] == "none"
                  and vis["canvas"]["canvas"] != "none" and vis["canvas"]["editor"] == "none"
                  and vis["canvas"]["grid"] == "none" and vis["canvas"]["pdf"] == "none"
                  and vis["canvas"]["activeTab"] is True
                  and vis["canvas"]["flags"]["ready"] and vis["canvas"]["flags"]["grid"]
                  and vis["canvas"]["flags"]["ev"] and vis["canvas"]["flags"]["pdf"]
                  and vis["canvas"]["flags"]["canvas"]
                  and vis["canvas"]["contract"] == 1.2 and vis["canvas"]["md"] == "function"
                  and len(page_errors) == 0,
                  json.dumps({"vis": vis, "pageErrors": page_errors[:3]}, ensure_ascii=False))

            # ---- 15. canvas_perf_smoke（100 节点批量添加 < 2s——粗粒度烟测）----
            if not has_wasm:
                pass  # already SKIP-registered above
            else:
                base_nodes = page.evaluate(
                    "window.__canvasObjects().filter(o => o.kind === 'node').length")
                t0 = time.time()
                page.evaluate("""() => {
                    for (let i = 0; i < 100; i++) {
                        window.__canvasExecute('human', JSON.stringify({
                            t: 'add_node', id: null,
                            x: 40 + (i % 10) * 150, y: 620 + Math.floor(i / 10) * 60,
                            w: 120, h: 36, props: { label: 'Bulk ' + i },
                        }));
                    }
                }""")
                page.wait_for_function(
                    f"document.querySelectorAll('#cv-l-nodes > g.node').length === {base_nodes + 100}",
                    timeout=5000)
                elapsed = time.time() - t0
                st15 = page.evaluate("window.__canvasSyncState()")
                check("canvas_perf_smoke",
                      elapsed < 2.0 and st15["converged"] is True,
                      json.dumps({"elapsed_s": round(elapsed, 3), "sync": st15},
                                 ensure_ascii=False))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "phase 5 §8.5 canvas view e2e", "dist": str(WEB / "dist"),
                 "port": PORT, "wasm_canvas_present": has_wasm},
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
