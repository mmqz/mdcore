#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""N-2 TOC 1200 条目验收（#29）——LO Basic 宏等价 F9 更新目录后 PDF 验收。

方法（Word F9 的沙箱等价——环境无 Word，诚实登记）：
1. 1200 标题（400×H1/H2/H3 混合）markdown → 引擎导出（toc 选项
   level_from/to=1-3——W-1 面）；
2. LO Basic 宏（~/.config/libreoffice Standard 库 UpdateTocExport）：
   隐藏打开 → 两轮 refresh + DocumentIndexes.update()（目录自身占页
   改分页——第二轮收敛页码）→ PDF 导出；
3. PyMuPDF 验收：①TOC 条目恰 1200 无截断 ②层级序（H1→H2→H3 循环
   不乱序）③页码引用真实（抽查 N 条：标题文本确在引用页上）。

用法（仓库根；需 harness release 构建 + soffice + LO Standard 库宏已装）：
    python3 test-artifacts/n2-toc-1200/run_n2.py
产物：stdout 报告 + test-artifacts/n2-toc-1200/result.json
"""
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HARNESS = ROOT / "target/release/examples/n1_n5_harness"
WORK = Path("/tmp/n1n5")
ARGS_FILE = WORK / "n2-args.txt"

def build_fixture() -> Path:
    """1200 标题夹具：400 单元 × (H1 + H2 + H3)。"""
    lines = []
    h = 0
    for _ in range(400):
        h += 3
        lines.append(f"# 第{h}章 大标题")
        lines.append(f"## {h}.1 节 中标题")
        lines.append("正文内容" * 24)
        lines.append(f"### {h}.1.1 小标题")
        lines.append("正文内容" * 24)
    md = WORK / "n2-toc.md"
    md.write_text("\n\n".join(lines) + "\n")
    return md

def main():
    WORK.mkdir(parents=True, exist_ok=True)
    md = build_fixture()

    # ① 引擎导出（toc level 1-3）
    docx = WORK / "n2-toc.docx"
    subprocess.run(
        [str(HARNESS), "export", str(md), str(docx),
         '{"toc":{"enabled":true,"level_from":1,"level_to":3}}'],
        capture_output=True, text=True, check=True)

    # ② LO 宏更新目录 + 导出
    pdf = WORK / "n2-toc-updated.pdf"
    ARGS_FILE.write_text(f"{docx}\n{pdf}\n")
    subprocess.run(
        ["soffice", "--headless", "--norestore",
         "vnd.sun.star.script:Standard.Module1.UpdateTocExport?language=Basic&location=application"],
        capture_output=True, timeout=600, check=True)

    # ③ PyMuPDF 验收
    import fitz
    d = fitz.open(pdf)
    pages = [p.get_text() for p in d]
    total = len(d)

    # TOC 条目形："标题.....页码"（leader 点线）——全文收集
    entry_re = re.compile(r"^(.+?)\.{4,}\s*(\d+)\s*$", re.M)
    entries = []
    for t in pages:
        entries.extend(entry_re.findall(t))

    def kind(title):
        title = title.strip()
        # PyMuPDF 在数字/CJK 边界可插空格（「第3 章」）——\s* 容差。
        if re.match(r"^第\s*\d+\s*章\s*大标题$", title):
            return 1
        if re.match(r"^\d+\s*\.\s*1\s*节\s*中标题$", title):
            return 2
        if re.match(r"^\d+\s*\.\s*1\s*\.\s*1\s*小标题$", title):
            return 3
        return 0

    toc_entries = [(t.strip(), int(p)) for t, p in entries if kind(t)]
    n = len(toc_entries)
    h1 = sum(1 for t, _ in toc_entries if kind(t) == 1)
    h2 = sum(1 for t, _ in toc_entries if kind(t) == 2)
    h3 = sum(1 for t, _ in toc_entries if kind(t) == 3)

    # 层级序抽查：前 12 条应为 1→2→3 循环
    seq = [kind(t) for t, _ in toc_entries[:12]]
    level_ok = seq == [1, 2, 3] * 4

    # 页码引用真实性：抽查 20 条（均匀步进）——标题文本确在引用页
    #（LO TOC 页码 = 文档页计数 1 基，与 PDF 物理页直对应）。
    sample = toc_entries[::max(1, n // 20)][:20]
    ref_ok = 0
    for title, page in sample:
        if 0 < page <= total and title in pages[page - 1]:
            ref_ok += 1
    ref_pct = round(ref_ok / len(sample) * 100, 1) if sample else 0.0

    verdict = "PASS" if (n == 1200 and h1 == 400 and h2 == 400 and h3 == 400
                         and level_ok and ref_ok == len(sample)) else "FAIL"
    print(f"[N-2] 页数 {total}")
    print(f"      TOC 条目 {n} = H1 {h1} + H2 {h2} + H3 {h3}（要求 400/400/400，无截断）")
    print(f"      层级序前 12 条 = {seq}（要求 1→2→3 循环）")
    print(f"      页码引用抽查 {ref_ok}/{len(sample)} 命中（{ref_pct}%）")
    print(f"判定: {verdict}")

    # ④ 范围控制（规格点名：W-1 必须能配 level_from/to）。
    # **验收面 = XML 指令层**（`TOC \o "2-3"`）——LO 导入 ContentIndex
    # 不保层级范围属性（宏 dump 定谳：CreateFromOutline=True 无 level
    # 面，update() 恒全层级——LO writerfilter 对 \o 开关的导入局限，
    # 登记非引擎缺陷）；Word 消费方遵循 \o 范围，实机验证待 Windows
    # 窗口（#28 同口径）。
    docx23 = WORK / "n2-toc-23.docx"
    subprocess.run(
        [str(HARNESS), "export", str(md), str(docx23),
         '{"toc":{"enabled":true,"level_from":2,"level_to":3}}'],
        capture_output=True, text=True, check=True)
    import zipfile
    xml23 = zipfile.ZipFile(docx23).read("word/document.xml").decode("utf-8")
    range_ok = 'TOC \\o &quot;2-3&quot;' in xml23
    # 反向：1-3 档指令面（对照防串档）
    xml13 = zipfile.ZipFile(docx).read("word/document.xml").decode("utf-8")
    range_ok = range_ok and 'TOC \\o &quot;1-3&quot;' in xml13
    print(f"[范围控制] XML 指令面：2-3 档 {'✓' if 'TOC \\o &quot;2-3&quot;' in xml23 else '✗'}"
          f" / 1-3 档 {'✓' if 'TOC \\o &quot;1-3&quot;' in xml13 else '✗'}"
          f"（LO 导入丢 \\o 范围开关 = 渲染甲骨文局限已登记——Word 实机待验证）"
          f" → {'PASS' if range_ok else 'FAIL'}")
    d.close()
    if not range_ok:
        verdict = "FAIL"

    out = ROOT / "test-artifacts/n2-toc-1200/result.json"
    out.write_text(json.dumps({
        "pages": total, "toc_entries": n, "h1": h1, "h2": h2, "h3": h3,
        "level_seq_head": seq, "page_ref_sample_hits": ref_ok,
        "page_ref_sample_total": len(sample),
        "range_control": {"level": "2-3", "xml_assert": "TOC \\o \"2-3\"", "ok": range_ok,
                          "lo_import_limitation": "LO ContentIndex 不保 \\o 范围——update 恒全层级（登记，Word 实机待验证）"},
        "verdict": verdict,
    }, ensure_ascii=False, indent=2))
    print(f"result → {out}")
    sys.exit(0 if verdict == "PASS" else 1)

if __name__ == "__main__":
    main()
