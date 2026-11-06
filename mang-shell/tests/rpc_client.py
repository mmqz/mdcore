#!/usr/bin/env python3
"""mang-shell RPC 最小客户端（#104 任务 2「RPC 桥接准备」）。

协议：unix socket + JSON Lines（一行一 JSON 对象，UTF-8，响应恒一行）。
端点：`$XDG_RUNTIME_DIR/mang-shell.sock`（缺省 `/tmp/mang-shell-$UID.sock`）
或启动时 `--rpc-sock PATH` 指定的路径。

两种用法：

  1. 单命令（AI 客户端 / 人工调试的最小示例）::

       ./rpc_client.py /run/user/1000/mang-shell.sock get_state
       ./rpc_client.py SOCK insert_text 0 4 "hello"
       ./rpc_client.py SOCK replace_selection "X"
       ./rpc_client.py SOCK open_file /tmp/a.md
       ./rpc_client.py SOCK close_file /tmp/a.md
       ./rpc_client.py SOCK switch_tab 1

     响应 JSON 打到 stdout；exit 0 = ok:true，exit 1 = ok:false，
     exit 2 = 用法错，连接失败抛异常（exit 3）。

  2. 烟雾序列（rpc_smoke.sh 消费——六命令全链断言 + 双客户端并发）::

       ./rpc_client.py SOCK --smoke FILE1 FILE2

     #275 起 FILE1/FILE2 必须在 --smoke-workspace 工作区内（open_file/
     save_as 沙箱收口——rpc_smoke.sh 已将 fixtures 复制进 $WS）。

  3. #111 扩展序列（rpc_smoke.sh 消费——15 命令面 + 订阅推送 +
     双向回显 + 多客户端订阅互不干扰；应用需以 --smoke-user-type
     启动——本序列断言 source=user 的用户键入事件。FILE2 为干净
     战场——--smoke 对 FILE1 的编辑会污染 undo 栈/版本）::

       ./rpc_client.py SOCK --smoke2 FILE1 FILE2

  4. #134 FS 序列（rpc_smoke.sh 消费——7 个文件系统命令全链 +
     沙箱边界拒绝 + 写已开文件 reload 同步 + rename 标签迁移 +
     delete 自动关标签；应用需以 --smoke-workspace WS 启动）::

       ./rpc_client.py SOCK --smoke-fs WORKSPACE

  5. #134 编辑器命令序列（rpc_smoke.sh 消费——run_command 真实
     分派 + list_commands 清单 + get_outline 标题树 + apply_format
     md 包裹与 undo 回滚；战场为 $WS/cmd.md）::

       ./rpc_client.py SOCK --smoke-cmd FILE

  6. #134 批量替换序列（rpc_smoke.sh 消费——跨文件双通道 + 正则 $1
     + per-file 计数 + undo 全批回滚）::

       ./rpc_client.py SOCK --smoke-batch WORKSPACE
"""

import json
import os
import socket
import sys
import time
import threading

TIMEOUT = 5.0


def send(sock_path, payload, timeout=TIMEOUT):
    """发一条命令，收一条响应（新连接/命令——多客户端安全）。"""
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    try:
        s.connect(sock_path)
        s.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        buf = b""
        while b"\n" not in buf:
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
    except (OSError, socket.timeout) as e:
        raise RuntimeError(f"连接/通信失败（{sock_path}）：{e}") from e
    finally:
        s.close()
    if not buf:
        raise RuntimeError("连接建立但无响应（应用退出中？）")
    return json.loads(buf.decode("utf-8").strip())


def build_payload(parts):
    """单命令模式：CLI 参数 → 请求载荷。"""
    cmd, args = parts[0], parts[1:]
    if cmd == "get_state" and not args:
        return {"cmd": "get_state"}
    if cmd == "insert_text" and len(args) == 3:
        return {
            "cmd": "insert_text",
            "line": int(args[0]),
            "col": int(args[1]),
            "text": args[2],
        }
    if cmd == "replace_selection" and len(args) == 1:
        return {"cmd": "replace_selection", "text": args[0]}
    if cmd == "open_file" and len(args) == 1:
        return {"cmd": "open_file", "path": os.path.abspath(args[0])}
    if cmd == "close_file" and len(args) == 1:
        return {"cmd": "close_file", "path": os.path.abspath(args[0])}
    if cmd == "switch_tab" and len(args) == 1:
        return {"cmd": "switch_tab", "index": int(args[0])}
    # ---- #134 任务 1：文件系统命令（路径相对工作区根——不 abspath，
    # 沙箱解析在服务端；绝对路径也照传）----
    if cmd == "list_dir" and len(args) == 1:
        return {"cmd": "list_dir", "path": args[0]}
    if cmd == "read_file" and len(args) == 1:
        return {"cmd": "read_file", "path": args[0]}
    if cmd == "write_file" and len(args) == 2:
        return {"cmd": "write_file", "path": args[0], "content": args[1]}
    if cmd == "create_file" and len(args) in (1, 2):
        p = {"cmd": "create_file", "path": args[0]}
        if len(args) == 2:
            p["content"] = args[1]
        return p
    if cmd == "create_dir" and len(args) == 1:
        return {"cmd": "create_dir", "path": args[0]}
    if cmd == "delete" and len(args) == 1:
        return {"cmd": "delete", "path": args[0]}
    if cmd == "rename" and len(args) == 2:
        return {"cmd": "rename", "from": args[0], "to": args[1]}
    raise SystemExit(f"未知命令或参数不匹配：{parts}（见文件头用法）")


def run_smoke(sock, file1, file2):
    """六命令全链 + 双客户端并发断言（rpc_smoke.sh 消费）。"""
    file1 = os.path.abspath(file1)
    file2 = os.path.abspath(file2)
    n = 0

    def cmd(payload):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is True, f"命令失败：{payload} -> {resp}"
        assert resp.get("id") == payload.get("id"), f"id 未回显：{payload} -> {resp}"
        return resp["result"]

    def cmd_err(payload, needle):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is False, f"应失败却成功：{payload} -> {resp}"
        assert needle in resp.get("error", ""), f"错误消息应含 {needle!r}：{resp}"
        return resp

    # ---- 1. 初始状态：单标签 FILE1，光标 (0,0)，干净 ----
    r = cmd({"id": 1, "cmd": "get_state"})
    assert len(r["tabs"]) == 1, r
    assert r["active"]["path"] == file1, r
    assert r["active"]["cursor"] == {"line": 0, "col": 0}, r
    assert r["active"]["modified"] is False, r
    assert r["active"]["readonly"] is False, r
    base_chars = r["active"]["char_count"]

    # ---- 2. insert_text：文首插入 "RPC "——字数+4、光标落插入尾 (0,4)、置脏 ----
    r = cmd({"id": 2, "cmd": "insert_text", "line": 0, "col": 0, "text": "RPC "})
    assert r["inserted_chars"] == 4, r
    assert r["cursor"] == {"line": 0, "col": 4}, r
    r = cmd({"id": 3, "cmd": "get_state"})
    assert r["active"]["char_count"] == base_chars + 4, r
    assert r["active"]["modified"] is True, "插入必须置脏（undo 链真实生效）"

    # ---- 3. replace_selection：无选区 = 光标处插入 ----
    r = cmd({"id": 4, "cmd": "replace_selection", "text": "桥"})
    assert r["had_selection"] is False, r
    r = cmd({"id": 5, "cmd": "get_state"})
    assert r["active"]["char_count"] == base_chars + 5, r

    # ---- 4. open_file：第二文件开新标签并激活 ----
    r = cmd({"id": 6, "cmd": "open_file", "path": file2})
    assert r["already_open"] is False and r["tab_index"] == 1, r
    r = cmd({"id": 7, "cmd": "get_state"})
    assert len(r["tabs"]) == 2 and r["active_index"] == 1, r
    assert r["active"]["path"] == file2, r

    # ---- 5. switch_tab：切回 0 ----
    r = cmd({"id": 8, "cmd": "switch_tab", "index": 0})
    assert r["active_index"] == 0, r

    # ---- 6. close_file：脏标签拒绝（先保存语义）----
    cmd_err({"id": 9, "cmd": "close_file", "path": file1}, "未保存")

    # ---- 7. open_file 已开 → 激活既有标签（already_open）----
    r = cmd({"id": 10, "cmd": "open_file", "path": file2})
    assert r["already_open"] is True and r["tab_index"] == 1, r

    # ---- 8. close_file：干净标签真关 ----
    r = cmd({"id": 11, "cmd": "close_file", "path": file2})
    assert r["closed_index"] == 1 and r["tabs_remaining"] == 1, r
    r = cmd({"id": 12, "cmd": "get_state"})
    assert len(r["tabs"]) == 1 and r["active_index"] == 0, r

    # ---- 9. switch_tab 越界拒绝 ----
    cmd_err({"id": 13, "cmd": "switch_tab", "index": 5}, "越界")

    # ---- 10. 双客户端并发：A 连发查询 / B 插入——互不阻塞 ----
    results = {}

    def client_a():
        try:
            for i in range(3):
                send(sock, {"id": 100 + i, "cmd": "get_state"})
            results["a"] = "ok"
        except Exception as e:  # noqa: BLE001——烟雾面要全错误形态
            results["a"] = f"fail: {e}"

    def client_b():
        try:
            send(sock, {"id": 200, "cmd": "insert_text", "line": 0, "col": 0, "text": "并发"})
            send(sock, {"id": 201, "cmd": "get_state"})
            results["b"] = "ok"
        except Exception as e:  # noqa: BLE001
            results["b"] = f"fail: {e}"

    ta = threading.Thread(target=client_a)
    tb = threading.Thread(target=client_b)
    ta.start()
    tb.start()
    ta.join()
    tb.join()
    assert results.get("a") == "ok" and results.get("b") == "ok", results

    total = n + 5  # 主线 13 + 并发 5
    print(f"rpc-smoke: client sequence passed（{total} 命令全链，双客户端并发互不阻塞）")



# ---------------------------------------------------------------------------
# #111 任务 2「RPC 扩展」：长连接订阅客户端 + 15 命令全链 + 事件断言
# ---------------------------------------------------------------------------


class SubClient:
    """长连接订阅客户端（响应与事件同 socket 混流——reader 线程分派）。

    响应（含 "id"/"ok"）入 pending 队列供 `send` 消费；
    事件（"type" == "doc_changed"）入 events 列表供断言消费。
    """

    def __init__(self, sock_path, timeout=30.0):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        # recv 超时只是 reader 周期性醒来的节拍（超时 continue）——
        # 不能用命令面的 5s：订阅静默期可能远长于此。
        self.sock.settimeout(timeout)
        try:
            self.sock.connect(sock_path)
        except OSError as e:
            raise RuntimeError(f"订阅连接失败（{sock_path}）：{e}") from e
        self.buf = b""
        self.events = []
        self.responses = []
        self.lock = threading.Lock()
        self.alive = True
        self.reader = threading.Thread(target=self._read_loop, daemon=True)
        self.reader.start()

    def _read_loop(self):
        while self.alive:
            try:
                chunk = self.sock.recv(65536)
            except socket.timeout:
                continue  # 订阅长连接：静默期正常——继续等（不是死）
            except OSError:
                break  # 真错（对端断/本端关）才退出
            self.buf += chunk
            while b"\n" in self.buf:
                line, _, self.buf = self.buf.partition(b"\n")
                if not line.strip():
                    continue
                try:
                    msg = json.loads(line.decode("utf-8"))
                except ValueError:
                    continue
                with self.lock:
                    if msg.get("type") == "doc_changed":
                        self.events.append(msg)
                    else:
                        self.responses.append(msg)

    def _recv_response(self, want_id, timeout=TIMEOUT):
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                for i, resp in enumerate(self.responses):
                    if resp.get("id") == want_id:
                        del self.responses[i]
                        return resp
            time.sleep(0.02)
        raise RuntimeError(f"响应超时（id={want_id}）——可能与应用退出竞态")

    def send(self, payload):
        """发命令并等该 id 的响应（事件被 reader 旁路到 events）。"""
        self.sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        return self._recv_response(payload.get("id"))

    def wait_event(self, pred, timeout=TIMEOUT):
        """等首条满足 pred 的事件（doc_changed）；超时返回 None。"""
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                for ev in self.events:
                    if pred(ev):
                        return ev
            time.sleep(0.02)
        return None

    def event_count(self):
        with self.lock:
            return len(self.events)

    def close(self):
        self.alive = False
        try:
            self.sock.close()
        except OSError:
            pass


def run_smoke2(sock, file1, file2):
    """#111 扩展序列：8 新命令全链 + 订阅推送 + 双向回显 + 多客户端订阅。

    rpc_smoke.sh 消费（应用以 --smoke-user-type 启动——6s 后合成用户键入，
    本序列在 6s 前完成 subscribe、6-7.3s 收 user 事件）。

    主战场取 FILE2：--smoke 序列对 FILE1 的编辑（insert/replace/并发）
    会污染 undo 栈与版本号——FILE2 在 --smoke 末尾被 close，重开即
    干净态（rev 0 / undo 栈空 / 未置脏），全部断言基于它。
    """
    file1 = os.path.abspath(file1)
    file2 = os.path.abspath(file2)
    n = 0

    # #134 修复：FILE2 快照/恢复——本序列的 save 步骤（id 11）会把带
    # “RPC”插入的内容真实写盘，多轮烟雾会叠加污染夹具（#111 遗留坑，
    # 修复前已实测 4 轮叠加出 "RPCRPCRPCRPC" 前缀）。序列尾原样写回。
    file2_snapshot = open(file2, encoding="utf-8").read()

    def cmd(payload):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is True, f"命令失败：{payload} -> {resp}"
        return resp["result"]

    def cmd_err(payload, needle):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is False, f"应失败却成功：{payload} -> {resp}"
        assert needle in resp.get("error", ""), f"错误应含 {needle!r}：{resp}"
        return resp

    # ---- 0. 订阅客户端 A 先上线（后续所有编辑它都要收到）----
    sub_a = SubClient(sock)
    r = sub_a.send({"id": 1000, "cmd": "subscribe"})
    assert r.get("ok") is True and r["result"]["subscribed"] is True, r
    n += 1

    # ---- 0.5 干净战场：FILE2 重开（--smoke 已 close 它——rev 0 栈空）----
    r = cmd({"id": 900, "cmd": "open_file", "path": file2})
    assert r["already_open"] is False, r
    r = cmd({"id": 901, "cmd": "get_state"})
    assert r["active"]["path"] == file2, r
    assert r["active"]["modified"] is False, "干净战场前提"
    assert r["active"]["char_count"] > 10, r  # mouse_smoke.md 有内容

    # ---- 1. set_cursor：跳到 (1,0)；越界 clamp 到末行 ----
    r = cmd({"id": 1, "cmd": "set_cursor", "line": 1, "col": 0})
    assert r["cursor"]["line"] == 1, r
    r = cmd({"id": 2, "cmd": "set_cursor", "line": 9999, "col": 0})
    state = send(sock, {"id": 3, "cmd": "get_state"})["result"]
    n += 1
    assert r["cursor"]["line"] == state["active"]["line_count"] - 1, (
        f"越界应 clamp 末行：{r} vs {state}"
    )

    # ---- 2. set_selection + get_selection：选区排序 + 文本回带 ----
    r = cmd({"id": 4, "cmd": "set_selection",
             "start": {"line": 0, "col": 4}, "end": {"line": 0, "col": 0}})
    assert r["selection"]["start"] == {"line": 0, "col": 0}, "逆序两点应排序"
    r = cmd({"id": 5, "cmd": "get_selection"})
    assert r["selection"]["start"] == {"line": 0, "col": 0}, r
    assert r["selection"]["chars"] == 4, r

    # ---- 3. undo/redo：无编辑时栈空不是错误 ----
    r = cmd({"id": 6, "cmd": "undo"})
    undo_empty_rev = r["revision"]

    # ---- 4. insert → 订阅事件（source=rpc，自己改的也回显）----
    r = cmd({"id": 7, "cmd": "insert_text", "line": 0, "col": 0, "text": "RPC"})
    insert_rev = send(sock, {"id": 8, "cmd": "get_selection"})["result"]["revision"]
    n += 1
    ev = sub_a.wait_event(
        lambda e: e.get("source") == "rpc"
        and any(c.get("text") == "RPC" for c in e.get("changes", []))
    )
    assert ev is not None, "订阅端应收 insert_text 的 doc_changed（source=rpc）"
    assert ev["revision"] == insert_rev, f"事件版本应达当前：{ev}"
    assert ev["path"].endswith(file2.split("/")[-1]), ev
    assert ev["revision_before"] == insert_rev - 1, (
        f"revision_before 应为变更前版本：{ev}"
    )

    # ---- 5. undo：事件 kind=undo + 版本继续递增（按 revision 精确匹配
    #      本次 undo 的事件——队列里可能有历史 undo 事件）----
    r = cmd({"id": 9, "cmd": "undo"})
    assert r["undid"] is True, r
    undo_rev = r["revision"]
    ev = sub_a.wait_event(
        lambda e: e.get("revision") == undo_rev
        and e.get("changes")
        and e["changes"][0].get("kind") == "undo"
    )
    assert ev is not None, "订阅端应收本次 undo 的事件"
    assert ev["revision_before"] == insert_rev, (
        f"undo 事件基线应为撤销前版本 {insert_rev}：{ev}"
    )
    assert undo_rev > insert_rev, "undo 也是变更——版本递增"

    # ---- 6. redo：事件 kind=redo（同按 revision 匹配）----
    r = cmd({"id": 10, "cmd": "redo"})
    assert r["redid"] is True, r
    redo_rev = r["revision"]
    ev = sub_a.wait_event(
        lambda e: e.get("revision") == redo_rev
        and e.get("changes")
        and e["changes"][0].get("kind") == "redo"
    )
    assert ev is not None, "订阅端应收 redo 事件"
    assert ev["revision_before"] == undo_rev, f"redo 基线：{ev}"

    # ---- 7. save：脏 → 保存 → 干净（modified false）----
    r = cmd({"id": 11, "cmd": "save"})
    assert r["saved"] is True and r["modified"] is False, r

    # ---- 8. save_as：另存新名 → tab 身份迁移（#275：目标须在工作区内
    #      ——沙箱收口后 /tmp 根外路径会被拒，改用 file2 同目录）----
    saved_as = os.path.join(os.path.dirname(file2), "smoke-saveas.md")
    r = cmd({"id": 12, "cmd": "save_as", "path": saved_as})
    assert r["saved"] is True and r["tab_index"] is not None, r
    # 身份迁移实测：按新路径能找到（close 不再报「标签未找到」）
    r = cmd({"id": 902, "cmd": "close_file", "path": saved_as})
    assert r["tabs_remaining"] >= 1, r  # file1 还开着

    # ---- 9. scroll_to：越界 clamp 末行 ----
    r = cmd({"id": 13, "cmd": "scroll_to", "line": 9999})
    assert r["scrolled_to"] == r["line_count"] - 1, r

    # ---- 10. 错误面：坏命令 / save 后干净态 close 走正常 ----
    cmd_err({"id": 14, "cmd": "bogus_cmd"}, "bogus_cmd")

    # ---- 11. 多客户端订阅互不干扰：B 上线同订 → 一处编辑两路都收 ----
    sub_b = SubClient(sock)
    r = sub_b.send({"id": 2000, "cmd": "subscribe"})
    assert r.get("ok") is True, r
    n += 1
    r = cmd({"id": 16, "cmd": "insert_text", "line": 0, "col": 0, "text": "双路"})
    ev_a = sub_a.wait_event(
        lambda e: e.get("source") == "rpc"
        and any(c.get("text") == "双路" for c in e.get("changes", []))
    )
    ev_b = sub_b.wait_event(
        lambda e: e.get("source") == "rpc"
        and any(c.get("text") == "双路" for c in e.get("changes", []))
    )
    assert ev_a is not None and ev_b is not None, "两路订阅均应收到同一事件"
    assert ev_a["revision"] == ev_b["revision"], "两路事件版本一致"

    # ---- 12. 双向回显·用户侧：等 --smoke-user-type 的 6s 合成键入 ----
    #（应用侧 6.0s 首键、每键 60ms、15 字 + enter ≈ 6.95s 收尾；从 subscribe
    # 起等 8s 稳定覆盖。用户事件逐键一条——断言「至少一条 source=user 且
    # 文本为键入片段」。）
    # 逐字符键入：每键一条事件（text = 单字符）——匹配方向是
    # 「事件 text 是键入串的字符」，不是「键入串含事件 text」。
    typed = "user typed this"
    user_ev = sub_a.wait_event(
        lambda e: e.get("source") == "user"
        and any(c.get("text") and c["text"] in typed for c in e.get("changes", [])),
        timeout=9.0,
    )
    if user_ev is None:
        # 调试面：dump 事件源分布（失败诊断——不应出现在 PASS 输出）
        with sub_a.lock:
            sources = [e.get("source") for e in sub_a.events]
            sample = sub_a.events[-3:]
        raise AssertionError(
            f"订阅端应收用户键入的 doc_changed（source=user）——"
            f"双向回显的用户侧；实际事件源分布 {sources}；末 3 条 {sample}"
        )
    assert user_ev["revision_before"] < user_ev["revision"], user_ev

    # ---- 13. 断连自动注销：关掉 A 后事件泵广播不炸（B 照常收）----
    sub_a.close()
    r = cmd({"id": 17, "cmd": "insert_text", "line": 0, "col": 0, "text": "afterA"})
    ev = sub_b.wait_event(
        lambda e: any(c.get("text") == "afterA" for c in e.get("changes", []))
    )
    assert ev is not None, "A 断连后 B 应照常收到（互不干扰 + 自动注销不炸泵）"
    sub_b.close()

    # #134：FILE2 复原——save 步骤的写盘回滚（夹具零污染收口）。
    with open(file2, "w", encoding="utf-8") as f:
        f.write(file2_snapshot)

    print(
        f"rpc-smoke2: client sequence passed"
        f"（{n} 命令 + 15 命令面全链 + 订阅/双向回显/多客户端/断连注销）"
    )


# ---------------------------------------------------------------------------
# #134 任务 1「RPC 文件系统命令」：沙箱实测序列（真实 unix socket）
# ---------------------------------------------------------------------------


def run_smoke_fs(sock, ws):
    """#134 FS 序列：7 命令全链 + 沙箱边界 + 写已开文件同步 + 标签迁移。

    ws = 烟雾工作区根（rpc_smoke.sh 预置夹具：readme.md / src/{a,b}.rs /
    docs/deep/note.md）。应用需以 --smoke-workspace $WS 启动。
    """
    ws = os.path.abspath(ws)
    n = 0

    def cmd(payload):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is True, f"命令失败：{payload} -> {resp}"
        return resp["result"]

    def cmd_err(payload, needle):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is False, f"应失败却成功：{payload} -> {resp}"
        assert needle in resp.get("error", ""), f"错误应含 {needle!r}：{resp}"
        return resp

    # ---- 1. list_dir 根（单层）：夹具可见 + rel 相对根 ----
    r = cmd({"id": 1, "cmd": "list_dir", "path": "."})
    names = {e["name"] for e in r["entries"]}
    assert {"readme.md", "src", "docs"} <= names, r
    assert r["truncated"] is False
    readme = next(e for e in r["entries"] if e["name"] == "readme.md")
    assert readme["is_dir"] is False and readme["size"] > 0, readme
    assert readme["rel"] == "readme.md", readme

    # ---- 2. list_dir recursive：嵌套文件可见 ----
    r = cmd({"id": 2, "cmd": "list_dir", "path": ".", "recursive": True})
    rels = {e["rel"] for e in r["entries"]}
    assert {"src/a.rs", "src/b.md", "docs/deep/note.md"} <= rels, r

    # ---- 3. read_file：相对路径 + 内容回带 ----
    r = cmd({"id": 3, "cmd": "read_file", "path": "src/a.rs"})
    assert "fn main" in r["text"], r
    assert r["truncated"] is False and r["size"] == len(r["text"].encode()), r

    # ---- 4. 沙箱边界：根外绝对路径 / .. 逃逸 全拒（安全验收核心）----
    cmd_err({"id": 4, "cmd": "read_file", "path": "/etc/passwd"}, "工作区")
    cmd_err({"id": 5, "cmd": "list_dir", "path": "../"}, "工作区")
    cmd_err(
        {"id": 6, "cmd": "write_file", "path": "/tmp/mang-escape.md", "content": "x"},
        "工作区",
    )
    cmd_err({"id": 7, "cmd": "delete", "path": "../../etc"}, "工作区")
    assert not os.path.exists("/tmp/mang-escape.md"), "逃逸写入绝不能落盘"
    # ---- 4b. #275 沙箱收口：open_file / save_as 同门（此前是
    #      ControlCommand 族旁路——S-1 读链 / S-2 写链源头）。
    #      open_file 根外拒：读链断（open→select_all→get_state 泄不出去）。
    cmd_err({"id": 41, "cmd": "open_file", "path": "/etc/passwd"}, "工作区")
    #      save_as 根外拒：写链断（活动文档是启动参数 FILE——可编辑
    #      md；若放行将把内容写到根外目标）。落盘不得发生。
    cmd_err(
        {"id": 42, "cmd": "save_as", "path": "/tmp/mang-escape-saveas.md"},
        "工作区",
    )
    assert not os.path.exists("/tmp/mang-escape-saveas.md"), "逃逸另存绝不能落盘"
    #      open_file 相对路径仍在根内放行（AI 主用法不变）。
    r = cmd({"id": 43, "cmd": "open_file", "path": "readme.md"})
    assert r["already_open"] is False, r
    r = cmd({"id": 44, "cmd": "close_file", "path": os.path.join(ws, "readme.md")})
    assert r["tabs_remaining"] >= 1, r

    # ---- 5. create_dir + create_file（含内容）+ 已存在/父目录拒绝 ----
    cmd({"id": 8, "cmd": "create_dir", "path": "gen"})
    r = cmd({"id": 9, "cmd": "create_file", "path": "gen/new.md", "content": "// generated\n"})
    assert r["bytes"] == 13, r
    cmd_err({"id": 10, "cmd": "create_file", "path": "gen/new.md"}, "已存在")
    cmd_err({"id": 11, "cmd": "create_file", "path": "nope/x.rs"}, "父目录")
    cmd_err({"id": 12, "cmd": "create_dir", "path": "gen"}, "已存在")

    # ---- 6. write_file 未打开文件：直接落盘（不动 UI）----
    r = cmd({"id": 13, "cmd": "write_file", "path": "gen/new.md", "content": "// rewritten\n"})
    assert r["synced_open_tab"] is False, r
    r = cmd({"id": 14, "cmd": "read_file", "path": "gen/new.md"})
    assert r["text"] == "// rewritten\n", r

    # ---- 7. write_file 已打开的干净标签 → reload 同步 + 订阅事件 ----
    sub = SubClient(sock)
    r = sub.send({"id": 3000, "cmd": "subscribe"})
    assert r.get("ok") is True and r["result"]["subscribed"] is True, r
    n += 1
    r = cmd({"id": 15, "cmd": "open_file", "path": os.path.join(ws, "src/b.md")})
    assert r["already_open"] is False, r
    r = cmd({"id": 16, "cmd": "write_file", "path": "src/b.md", "content": "// b rewritten\n"})
    assert r["synced_open_tab"] is True, f"已打开干净标签应 reload 同步：{r}"
    # Buffer 同步实测：活动文档干净（无未保存差异）且后续读取即新内容
    r = cmd({"id": 17, "cmd": "get_state"})
    assert r["active"]["path"].endswith("b.md"), r
    assert r["active"]["modified"] is False, f"reload 后应干净：{r}"
    # 双向回显不破：订阅端收到 reload 触发的 doc_changed（source=rpc）
    ev = sub.wait_event(
        lambda e: e.get("source") == "rpc"
        and (e.get("path") or "").endswith("b.md")
        and any("b rewritten" in (c.get("text") or "") for c in e.get("changes", []))
    )
    assert ev is not None, "write_file → reload → doc_changed：订阅端应收到"
    sub.close()

    # ---- 8. write_file 已打开的脏标签 → 拒（防丢用户修改）----
    cmd({"id": 18, "cmd": "insert_text", "line": 0, "col": 0, "text": "dirty"})
    cmd_err({"id": 19, "cmd": "write_file", "path": "src/b.md", "content": "x"}, "未保存")
    # 收尾：撤销脏化 + 保存（幂等）+ 关闭
    cmd({"id": 20, "cmd": "undo"})
    cmd({"id": 21, "cmd": "save"})
    cmd({"id": 22, "cmd": "close_file", "path": os.path.join(ws, "src/b.md")})

    # ---- 9. rename：文件 + 已打开标签身份迁移 ----
    cmd({"id": 23, "cmd": "open_file", "path": os.path.join(ws, "gen/new.md")})
    r = cmd({"id": 24, "cmd": "rename", "from": "gen/new.md", "to": "gen/renamed.md"})
    assert r["migrated_tabs"] == 1, r
    r = cmd({"id": 25, "cmd": "get_state"})
    paths = [t["path"] for t in r["tabs"]]
    assert any(p.endswith("gen/renamed.md") for p in paths), f"标签应迁到新名：{paths}"
    assert not any(p.endswith("gen/new.md") for p in paths), f"旧名标签应消失：{paths}"
    # rename 不覆盖：目标已存在拒绝
    cmd_err({"id": 26, "cmd": "rename", "from": "gen/renamed.md", "to": "readme.md"}, "已存在")
    cmd({"id": 27, "cmd": "close_file", "path": os.path.join(ws, "gen/renamed.md")})

    # ---- 10. delete：文件 + 目录（含自动关标签）+ 根自身拒绝 ----
    r = cmd({"id": 28, "cmd": "delete", "path": "gen/renamed.md"})
    assert r["deleted"] is True and r["was_dir"] is False, r
    cmd_err({"id": 29, "cmd": "read_file", "path": "gen/renamed.md"}, "读取失败")
    # 删目录：目录里有已打开的干净标签 → 删后自动关
    cmd({"id": 30, "cmd": "open_file", "path": os.path.join(ws, "docs/deep/note.md")})
    r = cmd({"id": 31, "cmd": "delete", "path": "docs"})
    assert r["was_dir"] is True and r["closed_tabs"] == 1, r
    r = cmd({"id": 32, "cmd": "get_state"})
    assert not any("note.md" in (t["path"] or "") for t in r["tabs"]), r
    cmd_err({"id": 33, "cmd": "delete", "path": "."}, "根目录")

    print(
        f"rpc-smoke-fs: client sequence passed（{n} 命令：7 FS 命令全链 + "
        f"沙箱拒绝 + reload 同步/脏拒 + rename 迁移 + delete 关标签）"
    )


def run_smoke_cmd(sock, file):
    """#134 任务 2：编辑器命令面——run_command / list_commands /
    get_outline / apply_format（真实 socket 断言）。

    file = 独立干净战场（rpc_smoke.sh 的 $WS/cmd.md——首行标题供
    get_outline；避开 FILE2 的合成键入残留；本序列的 apply_format
    只在 Buffer 内不改盘，收尾 undo 回滚）。
    """
    file = os.path.abspath(file)
    n = 0

    def cmd(payload):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is True, f"命令失败：{payload} -> {resp}"
        return resp["result"]

    def cmd_err(payload, needle):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is False, f"应失败却成功：{payload} -> {resp}"
        assert needle in resp.get("error", ""), f"错误应含 {needle!r}：{resp}"
        return resp

    # ---- 1. list_commands：全部命令可见（机器名/标题/默认键位）----
    r = cmd({"id": 1, "cmd": "list_commands"})
    assert r["count"] >= 18, r
    ids = {c["id"] for c in r["commands"]}
    assert {"move_left", "undo", "save", "goto_line", "toggle_fold"} <= ids, r
    undo_item = next(c for c in r["commands"] if c["id"] == "undo")
    assert undo_item["default_key"] == "Ctrl+Z", undo_item
    assert undo_item["title"], undo_item

    # ---- 2. run_command：doc_end 真移动光标（get_state 对照）----
    r = cmd({"id": 2, "cmd": "open_file", "path": file})
    assert r["already_open"] is False, r
    r = cmd({"id": 3, "cmd": "run_command", "name": "doc_end"})
    assert r["ran"] is True and r["command"] == "doc_end", r
    r = cmd({"id": 4, "cmd": "get_state"})
    assert r["active"]["cursor"]["line"] == r["active"]["line_count"] - 1, (
        f"doc_end 后光标应在末行：{r}"
    )
    # 未知名拒绝（引导 list_commands）
    cmd_err({"id": 5, "cmd": "run_command", "name": "bogus_cmd"}, "list_commands")

    # ---- 3. get_outline：cmd.md 首行是 "# cmd battlefield" ----
    r = cmd({"id": 6, "cmd": "get_outline"})
    assert r["count"] >= 1, r
    first = r["entries"][0]
    assert first["level"] == 1, first
    assert first["line"] == 0, first
    assert "battlefield" in first["text"], first

    # ---- 4. apply_format：加粗/斜体 + undo 回滚（char 计数链）----
    r = cmd({"id": 7, "cmd": "get_state"})
    base_chars = r["active"]["char_count"]
    r = cmd(
        {
            "id": 8,
            "cmd": "apply_format",
            "range": [{"line": 0, "col": 0}, {"line": 0, "col": 4}],
            "format": "bold",
        }
    )
    assert r["applied"] is True and r["format"] == "bold", r
    r = cmd({"id": 9, "cmd": "get_state"})
    assert r["active"]["char_count"] == base_chars + 4, (
        f"bold 包裹应 +4 字符（** + **）：{r}"
    )
    assert r["active"]["modified"] is True, r
    # 逆序两点（内部排序——同范围再包 italic）
    r = cmd(
        {
            "id": 10,
            "cmd": "apply_format",
            "range": [{"line": 0, "col": 4}, {"line": 0, "col": 0}],
            "format": "italic",
        }
    )
    assert r["applied"] is True, r
    r = cmd({"id": 11, "cmd": "get_state"})
    assert r["active"]["char_count"] == base_chars + 6, f"italic 再 +2：{r}"
    # 未知格式拒绝
    cmd_err(
        {"id": 12, "cmd": "apply_format", "range": [[0, 0], [0, 4]], "format": "underline"},
        "",  # serde 错误——只断言失败
    )
    # undo×2 回滚两个格式（一条 undo 一个格式动作）
    cmd({"id": 13, "cmd": "undo"})
    cmd({"id": 14, "cmd": "undo"})
    r = cmd({"id": 15, "cmd": "get_state"})
    assert r["active"]["char_count"] == base_chars, f"两次 undo 应回滚全部格式：{r}"

    print(
        f"rpc-smoke-cmd: client sequence passed（{n} 命令：run_command "
        f"真实移动 + list_commands 清单 + outline 首标题 + apply_format "
        f"char 链 + undo 回滚）"
    )


def run_smoke_batch(sock, ws):
    """#134 任务 3：batch_replace——跨文件双通道 + 正则 + per-file
    计数 + undo 语义（真实 socket 断言）。

    ws = 烟雾工作区根（rpc_smoke.sh 预置 batch/ 夹具：b1.md 两处 foo /
    b2.md 一处 foo / b3.md 无命中）。
    """
    ws = os.path.abspath(ws)
    n = 0

    def cmd(payload):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is True, f"命令失败：{payload} -> {resp}"
        return resp["result"]

    def cmd_err(payload, needle):
        nonlocal n
        n += 1
        resp = send(sock, payload)
        assert resp.get("ok") is False, f"应失败却成功：{payload} -> {resp}"
        assert needle in resp.get("error", ""), f"错误应含 {needle!r}：{resp}"
        return resp

    def file_result(r, rel):
        return next(f for f in r["files"] if f["path"].endswith(rel))

    # ---- 1. 打开 b1（buffer 通道战场；b2/b3 保持关闭走 disk 通道）----
    r = cmd({"id": 1, "cmd": "open_file", "path": os.path.join(ws, "batch/b1.md")})
    assert r["already_open"] is False, r
    r = cmd({"id": 2, "cmd": "get_state"})
    b1_chars = r["active"]["char_count"]

    # ---- 2. 字面替换：跨三文件，双通道各自计数 ----
    r = cmd(
        {
            "id": 3,
            "cmd": "batch_replace",
            "files": ["batch/b1.md", "batch/b2.md", "batch/b3.md"],
            "pattern": "foo",
            "replacement": "bar",
        }
    )
    assert r["total_replacements"] == 3, r
    assert r["files_changed"] == 2, r
    b1 = file_result(r, "batch/b1.md")
    b2 = file_result(r, "batch/b2.md")
    b3 = file_result(r, "batch/b3.md")
    assert b1["via"] == "buffer" and b1["count"] == 2, b1
    assert b2["via"] == "disk" and b2["count"] == 1, b2
    assert b3["count"] == 0 and b3.get("note") == "无命中", b3

    # ---- 3. disk 通道真落盘 / buffer 通道不落盘（双通道语义实测）----
    r = cmd({"id": 4, "cmd": "read_file", "path": "batch/b2.md"})
    assert r["text"] == "bar single\n", r
    r = cmd({"id": 5, "cmd": "read_file", "path": "batch/b1.md"})
    assert r["text"] == "foo one\nfoo two\n", "buffer 通道只改 Buffer 不落盘"
    r = cmd({"id": 6, "cmd": "get_state"})
    assert r["active"]["modified"] is True, "b1 的 Buffer 已改（未保存）"

    # ---- 4. undo 一次回滚 b1 全批（一条 undo = 该文件全部替换）----
    r = cmd({"id": 7, "cmd": "undo"})
    assert r["undid"] is True, r
    r = cmd({"id": 8, "cmd": "get_state"})
    assert r["active"]["char_count"] == b1_chars, f"undo 应回滚全批：{r}"
    cmd({"id": 9, "cmd": "save"})  # 幂等归位（盘=buffer）
    cmd({"id": 10, "cmd": "close_file", "path": os.path.join(ws, "batch/b1.md")})

    # ---- 5. 正则替换（$1 捕获展开，disk 通道）----
    r = cmd(
        {
            "id": 11,
            "cmd": "batch_replace",
            "files": ["batch/b2.md"],
            "pattern": r"bar (\w+)",
            "replacement": "BAR_$1",
            "is_regex": True,
        }
    )
    b2 = file_result(r, "batch/b2.md")
    assert b2["via"] == "disk" and b2["count"] == 1, r
    r = cmd({"id": 12, "cmd": "read_file", "path": "batch/b2.md"})
    assert r["text"] == "BAR_single\n", f"$1 展开实测：{r}"

    # ---- 6. 错误面：沙箱外 skipped / 坏正则顶层拒 / 空清单拒 ----
    r = cmd(
        {
            "id": 13,
            "cmd": "batch_replace",
            "files": ["/etc/passwd"],
            "pattern": "root",
            "replacement": "x",
        }
    )
    f0 = r["files"][0]
    assert f0["via"] == "skipped" and "工作区" in f0.get("error", ""), r
    cmd_err(
        {
            "id": 14,
            "cmd": "batch_replace",
            "files": ["batch/b3.md"],
            "pattern": "(",
            "replacement": "x",
            "is_regex": True,
        },
        "正则编译失败",
    )
    cmd_err(
        {"id": 15, "cmd": "batch_replace", "files": [], "pattern": "x", "replacement": "y"},
        "清单为空",
    )

    print(
        f"rpc-smoke-batch: client sequence passed（{n} 命令：跨文件双通道 "
        f"+ per-file 计数 + buffer/disk 落盘语义 + undo 全批回滚 + 正则 $1 + 错误面）"
    )


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        print(__doc__)
        sys.exit(2)
    sock, rest = args[0], args[1:]
    if rest[0] == "--smoke":
        if len(rest) != 3:
            print("用法：rpc_client.py SOCK --smoke FILE1 FILE2", file=sys.stderr)
            sys.exit(2)
        try:
            run_smoke(sock, rest[1], rest[2])
        except AssertionError as e:
            print(f"rpc-smoke: FAIL —— {e}", file=sys.stderr)
            sys.exit(1)
        except RuntimeError as e:
            print(f"rpc-smoke: FAIL —— {e}", file=sys.stderr)
            sys.exit(3)
        return
    if rest[0] == "--smoke2":
        if len(rest) != 3:
            print("用法：rpc_client.py SOCK --smoke2 FILE1 FILE2", file=sys.stderr)
            sys.exit(2)
        try:
            run_smoke2(sock, rest[1], rest[2])
        except AssertionError as e:
            print(f"rpc-smoke2: FAIL —— {e}", file=sys.stderr)
            sys.exit(1)
        except RuntimeError as e:
            print(f"rpc-smoke2: FAIL —— {e}", file=sys.stderr)
            sys.exit(3)
        return
    if rest[0] == "--smoke-fs":
        if len(rest) != 2:
            print("用法：rpc_client.py SOCK --smoke-fs WORKSPACE", file=sys.stderr)
            sys.exit(2)
        try:
            run_smoke_fs(sock, rest[1])
        except AssertionError as e:
            print(f"rpc-smoke-fs: FAIL —— {e}", file=sys.stderr)
            sys.exit(1)
        except RuntimeError as e:
            print(f"rpc-smoke-fs: FAIL —— {e}", file=sys.stderr)
            sys.exit(3)
        return
    if rest[0] == "--smoke-cmd":
        if len(rest) != 2:
            print("用法：rpc_client.py SOCK --smoke-cmd FILE2", file=sys.stderr)
            sys.exit(2)
        try:
            run_smoke_cmd(sock, rest[1])
        except AssertionError as e:
            print(f"rpc-smoke-cmd: FAIL —— {e}", file=sys.stderr)
            sys.exit(1)
        except RuntimeError as e:
            print(f"rpc-smoke-cmd: FAIL —— {e}", file=sys.stderr)
            sys.exit(3)
        return
    if rest[0] == "--smoke-batch":
        if len(rest) != 2:
            print("用法：rpc_client.py SOCK --smoke-batch WORKSPACE", file=sys.stderr)
            sys.exit(2)
        try:
            run_smoke_batch(sock, rest[1])
        except AssertionError as e:
            print(f"rpc-smoke-batch: FAIL —— {e}", file=sys.stderr)
            sys.exit(1)
        except RuntimeError as e:
            print(f"rpc-smoke-batch: FAIL —— {e}", file=sys.stderr)
            sys.exit(3)
        return
    payload = build_payload(rest)
    payload.setdefault("id", 1)
    try:
        resp = send(sock, payload)
    except RuntimeError as e:
        print(f"rpc-client: {e}", file=sys.stderr)
        sys.exit(3)
    print(json.dumps(resp, ensure_ascii=False))
    sys.exit(0 if resp.get("ok") else 1)


if __name__ == "__main__":
    main()
