#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""N-4 跨页表头重复 LO 渲染验收（#29）。

方法：80 行长表 markdown → 引擎导出（table_header_repeat 首行 tblHeader）
→ LO 渲染 PDF → 逐页提取文本：第 2 页顶部应含表头行文本（跨页重复）。
对照：注入前（纯 table()）第 2 页顶部无表头。

用法（仓库根；需 harness release 构建与 soffice）：
    python3 scripts/verify_n4_lo.py
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HARNESS = ROOT / "target/release/examples/n1_n5_harness"
WORK = Path("/tmp/n1n5")

# 80 行 × 3 列（长表——引擎缺省档每页 ~44 行，必跨页）
ROWS = 80
md = ["| 序号 | 项目名称 | 金额 |", "|---|---|---|"]
for i in range(ROWS):
    md.append(f"| {i+1} | 跨页测试项目第{i+1}项名称 | {i+1}.00 |")
(WORK / "n4-long-table.md").write_text("\n".join(md) + "\n")

def render_and_check(docx: Path, tag: str) -> dict:
    subprocess.run(
        ["soffice", "--headless", "--convert-to", "pdf", "--outdir", str(WORK), str(docx)],
        capture_output=True, timeout=300, check=True)
    import fitz
    pdf = fitz.open(WORK / (docx.stem + ".pdf"))
    pages = [p.get_text() for p in pdf]
    n = len(pdf)
    pdf.close()
    # 第 2 页文本：表头三格文本是否在场
    header_in_p2 = ("序号" in pages[1]) and ("项目名称" in pages[1])
    # 第 2 页头部 60 字符（页顶内容检视）
    head_p2 = pages[1][:60].replace(chr(10), "/")
    print(f"[{tag}] {n} 页 | 第 2 页含表头: {header_in_p2} | 页顶: {head_p2}")
    return {"pages": n, "header_in_page2": header_in_p2}

# ① 引擎导出（table_header_repeat——wasm 链 Table 分支已接线）
proc = subprocess.run(
    [str(HARNESS), "export", str(WORK / "n4-long-table.md"),
     str(WORK / "n4-marked.docx"), "default"],
    capture_output=True, text=True, check=True)
marked = render_and_check(WORK / "n4-marked.docx", "注入后")

# ② 对照：手造无 tblHeader 版（office-io 纯 table——Python 直改 zip：
# 把首个 <w:trPr><w:tblHeader /></w:trPr> 还原为 <w:trPr />）
import zipfile, shutil, re
src = WORK / "n4-marked.docx"
dst = WORK / "n4-plain.docx"
zin = zipfile.ZipFile(src)
zout = zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED)
for item in zin.infolist():
    data = zin.read(item.filename)
    if item.filename == "word/document.xml":
        data = data.replace(
            b"<w:trPr><w:tblHeader /></w:trPr>", b"<w:trPr />", 1)
    zout.writestr(item, data)
zout.close()
plain = render_and_check(WORK / "n4-plain.docx", "对照组")

ok = marked["header_in_page2"] and not plain["header_in_page2"]
print(f"\n验收判定: {'PASS' if ok else 'FAIL'}"
      f"（注入后第 2 页有表头 + 对照组无——差异归因 tblHeader）")
json.dump({"marked": marked, "plain": plain, "verdict": "PASS" if ok else "FAIL"},
          open(WORK / "n4-result.json", "w"), ensure_ascii=False, indent=2)
sys.exit(0 if ok else 1)
