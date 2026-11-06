#!/usr/bin/env python3
"""S2-1 页面组织任务的真实 PDF 语料构建脚本。

来源（真实文档纪律——与 office-io 语料同源哲学）：
1. test-artifacts/health-check/corpus/{poi-docx,docx4j}/ 的真实 docx
   → LibreOffice soffice 转 PDF（真实 LO PDF 写出器结构：页树/资源/
   字体子集/书签[标题样式驱动的文档有]）。
2. 大文档（300 页级）：n2-toc-1200 的 1200 条 TOC docx 或拼接生成
   → soffice 转 PDF，满足 issue #103「300 页技术手册」验收位。

产物：test-artifacts/pdf-organize-corpus/
  poi__*.pdf / docx4j__*.pdf   — LO 转换的真实结构 PDF
  large__*.pdf                 — 300 页级
  MANIFEST.json                — 来源/页数/书签数/加密态登记（provenance）

用法：python3 build_corpus.py [--sample N] [--max-pages P]
"""
import json
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]  # pdf-organize-corpus → test-artifacts → 仓根（#262 顺带修复：脚本自 scripts/ 迁入后 parents[1] 指向不存在的 test-artifacts/mdcore，语料无法重建）
CORPUS_SRC = REPO / "test-artifacts" / "health-check" / "corpus"
OUT_DIR = REPO / "test-artifacts" / "pdf-organize-corpus"
SOFFICE = "soffice"
PDFINFO = "pdfinfo"
PDFTOTEXT = "pdftotext"


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=180, **kw)


def pdf_pages(path: Path):
    r = sh([PDFINFO, str(path)])
    for line in r.stdout.splitlines():
        if line.startswith("Pages:"):
            return int(line.split(":", 1)[1].strip())
    return None


def pdf_encrypted(path: Path) -> bool:
    r = sh([PDFINFO, str(path)])
    return "Encrypted:       yes" in r.stdout


def convert_one(docx: Path, out_dir: Path, prefix: str) -> dict | None:
    out_pdf = out_dir / f"{prefix}__{docx.stem}.pdf"
    if out_pdf.exists():
        pass  # 幂等：已转换的直接登记
    else:
        r = sh([SOFFICE, "--headless", "--convert-to", "pdf",
                "--outdir", str(out_dir), str(docx)])
        produced = out_dir / (docx.stem + ".pdf")
        if not produced.exists():
            return None
        produced.rename(out_pdf)
    pages = pdf_pages(out_pdf)
    if not pages:
        return None
    return {"name": out_pdf.name, "pages": pages,
            "encrypted": pdf_encrypted(out_pdf), "source": str(docx.relative_to(REPO))}


def main():
    sample = int(sys.argv[sys.argv.index("--sample") + 1]) if "--sample" in sys.argv else 40
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    manifest = []
    t0 = time.time()

    # --- 真实 docx 语料抽样（POI + docx4j 各半）---
    for sub, prefix in (("poi-docx", "poi"), ("docx4j", "docx4j")):
        src = CORPUS_SRC / sub
        if not src.is_dir():
            print(f"[warn] 语料源缺失：{src}")
            continue
        files = sorted(p for p in src.glob("*.docx") if not p.name.startswith("~"))
        # 稳定抽样（确定性：排序后等距取 sample 个）
        step = max(1, len(files) // sample)
        picked = files[::step][:sample]
        for i, docx in enumerate(picked):
            info = convert_one(docx, OUT_DIR, prefix)
            if info:
                manifest.append(info)
            if (i + 1) % 10 == 0:
                print(f"[{sub}] {i+1}/{len(picked)} …")

    # --- 大文档（300 页级）---
    big_candidates = sorted(CORPUS_SRC.glob("docx4j/*.docx"),
                            key=lambda p: p.stat().st_size, reverse=True)[:5]
    for docx in big_candidates:
        info = convert_one(docx, OUT_DIR, "large")
        if info and info["pages"] >= 200:
            manifest.append(info)
            break
    else:
        print("[warn] 未找到 ≥200 页的大文档")

    (OUT_DIR / "MANIFEST.json").write_text(
        json.dumps({"built_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                    "tool": "soffice headless convert-to pdf",
                    "provenance": "POI/docx4j 真实 docx 语料 → LibreOffice PDF 写出器",
                    "files": manifest}, ensure_ascii=False, indent=1))
    pages_total = sum(m["pages"] for m in manifest)
    print(f"[done] {len(manifest)} 件 / 共 {pages_total} 页 / {time.time()-t0:.0f}s → {OUT_DIR}")


if __name__ == "__main__":
    main()
