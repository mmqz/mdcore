#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""N-1 40万字导出实测（#29 任务池 N 组）+ N-5 分页预估验收对照。

度量对象：mdcore 导出链（markdown → BlockTree → export_docx_styled_core →
office-io G2 → docx 字节），双排版档（default = 引擎缺省 / tender = 中文
标书四号 1.5 倍行距）。

指标（#29 N-1 要求表）：
- 导出耗时（harness 自报 parse_ms/export_ms + 墙钟）；
- 峰值内存（getrusage(RUSAGE_CHILDREN) ru_maxrss——子进程峰值 RSS）；
- 结构完整性（Word「不弹修复」的沙箱等价证据链：python-docx 独立开
  + PyMuPDF 渲染开 + LO 转换成功；Word COM 实机探针 = TR-2D
  word_probe.ps1 模式，待 Windows 窗口补验——诚实登记）；
- 页数（LO 渲染计数）与字数换算对照（N-1「约 500 页」口径按排版档
  分列）。

N-5 验收（#29）：40 万字文档估算页数 vs LO 实际页数偏差 ≤5%。

用法（仓库根；需先 cargo build --release -p mdcore-wasm --example n1_n5_harness
与 soffice 在 PATH）：
    python3 test-artifacts/large-doc/run_n1.py
产物：stdout 报告 + test-artifacts/large-doc/results-n1.json
"""
import json
import resource
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HARNESS = ROOT / "target/release/examples/n1_n5_harness"
FIXTURE = ROOT / "test-artifacts/word-count/fixture_400k.md"
WORKDIR = Path("/tmp/n1n5")

def run_harness(args) -> tuple[dict, float]:
    """独立包装进程跑 harness：RUSAGE_CHILDREN 隔离（不与 soffice 混计
    ——getrusage 为累计峰值口径，LO 子进程 ~300MB 会污染引擎面数值）。"""
    wrapper = (
        "import subprocess,resource,sys,json\n"
        "p = subprocess.run(sys.argv[1:], capture_output=True, text=True, check=True)\n"
        "rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss\n"
        "print(json.dumps({'stdout': p.stdout, 'rss_kb': rss}))\n"
    )
    proc = subprocess.run(
        [sys.executable, "-c", wrapper, str(HARNESS), *args],
        capture_output=True, text=True, check=True,
    )
    w = json.loads(proc.stdout)
    return json.loads(w["stdout"]), w["rss_kb"] / 1024.0

def run(profile: str) -> dict:
    WORKDIR.mkdir(parents=True, exist_ok=True)
    docx = WORKDIR / f"f400k-{profile}.docx"
    t0 = time.time()
    summary, peak_rss_mb = run_harness(
        ["export", str(FIXTURE), str(docx), profile])
    wall_ms = (time.time() - t0) * 1000
    summary["wall_ms"] = round(wall_ms, 1)
    summary["peak_rss_mb"] = round(peak_rss_mb, 1)

    # LO 渲染页数（N-5 真值）
    pdf = WORKDIR / f"f400k-{profile}.pdf"
    subprocess.run(
        ["soffice", "--headless", "--convert-to", "pdf", "--outdir",
         str(WORKDIR), str(docx)],
        capture_output=True, timeout=600, check=True,
    )
    import fitz
    d = fitz.open(pdf)
    summary["lo_pages"] = len(d)
    d.close()

    # 结构完整性（独立解析器 + LO 转换成功本身即结构合法性旁证）
    import docx as pydocx
    pd = pydocx.Document(str(docx))
    summary["python_docx_open"] = "ok"
    summary["python_docx_paragraphs"] = len(pd.paragraphs)
    summary["python_docx_tables"] = len(pd.tables)

    # N-5 估算对照（同一 options 的 estimate 面）
    est = subprocess.run(
        [str(HARNESS), "estimate", str(FIXTURE), profile],
        capture_output=True, text=True, check=True,
    )
    e = json.loads(est.stdout)["estimate"]
    summary["estimate_pages"] = e["pages"]
    summary["estimate_dev_pct"] = round((e["pages"] - summary["lo_pages"])
                                        / summary["lo_pages"] * 100, 2)
    summary["estimate_detail"] = e
    return summary

def main():
    results = {"fixture": str(FIXTURE), "profiles": {}}
    ok = True
    for profile in ("default", "tender"):
        r = run(profile)
        results["profiles"][profile] = r
        verdict = "PASS" if abs(r["estimate_dev_pct"]) <= 5.0 else "FAIL"
        if verdict == "FAIL":
            ok = False
        print(f"[{profile}] export {r['export_ms']}ms / parse {r['parse_ms']}ms"
              f" / peak RSS {r['peak_rss_mb']}MB / docx {r['docx_bytes']}B")
        print(f"         LO 实际 {r['lo_pages']} 页 vs 估算 {r['estimate_pages']} 页"
              f" → 偏差 {r['estimate_dev_pct']:+.2f}% [{verdict}]（N-5 ≤5%）")
        print(f"         结构完整性：python-docx 开 OK（{r['python_docx_paragraphs']} 段 /"
              f" {r['python_docx_tables']} 表）+ LO 转换 OK")
    out = ROOT / "test-artifacts/large-doc/results-n1.json"
    out.write_text(json.dumps(results, ensure_ascii=False, indent=2))
    print(f"\nresults → {out}")
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    main()
