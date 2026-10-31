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
"""

import json
import os
import socket
import sys
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
