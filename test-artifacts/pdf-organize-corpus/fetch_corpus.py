#!/usr/bin/env python3
"""#262/#273 补充取件：pdf-organize 语料的「多页真实件」轻量子集。

Lead 评审（2026-11-06，PR #273）指出的缺口：主语料（MANIFEST.json
92 件，build_corpus.py 生成——依赖 soffice+poppler，重建约分钟级）
CI 上不可廉价重建，语料门因此空转。本脚本补一条**轻取件**通道：

  realworld/  8 件多页真实 PDF（共约 11MB）
  来源 = mozilla/pdf.js + apache/pdfbox 测试仓（Apache-2.0，与
  #264 pdf-corpus 同源同许可——逐件 sha256 见 realworld/PROVENANCE.json）。

选件口径（Lead 方案 A：「3-5 件 10 页的 PDF，CI 里能真跑」）：
  · 多页（4-352 页）——删页/抽取/重排/合并的操作面；
  · 真实世界件（学术论文/整书/真实注释导出/用户 bug 附件）——
    非 lopdf 自产合成件；
  · 未加密（加密拒绝面由 encrypted_rejected_s5 现场生成夹具覆盖）；
  · 与 #264 pdf-corpus 的 mixed/multicolumn 类**同 URL 复用**——
    上游漂移面单一，provenance 可交叉核对。

纪律（pdf-corpus 先例同构）：
  · 取件脚本 + PROVENANCE 入库；PDF 二进制不入库（根 .gitignore：
    test-artifacts/pdf-organize-corpus/*.pdf 递归生效）；
  · 幂等（已存在跳过）；依赖 curl；pdfinfo 缺席时 pages 记 null
    （不阻断——CI Windows runner 无 poppler）。
"""
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

# Windows CI：控制台/默认文件编码是 cp1252——中文注记（PROVENANCE
# note / 横幅）会 UnicodeEncodeError 炸取件步骤（首跑实测，位置
# 142-152 = 「整书 352 页」段）。stdout/stderr 重配 UTF-8 +
# 写文件显式 encoding="utf-8"（Python 3.7+；runner 3.12）。
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

OUT = Path(__file__).resolve().parent / "realworld"

PDFJS = "https://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/"
PDFBOX = "https://raw.githubusercontent.com/apache/pdfbox/trunk/pdfbox/src/test/resources/input/rendering/"

# (文件名, 源URL, 来源仓, 测试点一句话)
MANIFEST = [
    ("comments.pdf", PDFJS, "mozilla/pdf.js", "tracemonkey 注释版（真实评论导出，14 页 1MB）"),
    ("freeculture.pdf", PDFJS, "mozilla/pdf.js", "《Free Culture》整书 352 页（长文档操作面）"),
    ("highlights.pdf", PDFJS, "mozilla/pdf.js", "真实高亮集（14 页 946KB）"),
    ("issue19971.pdf", PDFJS, "mozilla/pdf.js", "真实用户文档（6 页 letter，1.7MB）"),
    ("issue6286.pdf", PDFJS, "mozilla/pdf.js", "真实用户文档（1.9MB 密集单页）"),
    ("issue6961.pdf", PDFJS, "mozilla/pdf.js", "真实用户文档（2 页 2.1MB）"),
    ("survey.pdf", PDFBOX, "apache/pdfbox", "学术调查问卷（双栏+表格，9 页）"),
    ("tracemonkey_with_editable_annotations.pdf", PDFJS, "mozilla/pdf.js", "学术论文双栏 14 页（pdf.js 渲染基准件）"),
]


def sha256(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def pdfinfo_pages(p: Path):
    """页数探针——pdfinfo 缺席（如 CI Windows runner）记 null 不阻断。"""
    try:
        r = subprocess.run(
            ["pdfinfo", str(p)], capture_output=True, text=True, timeout=60
        )
    except FileNotFoundError:
        return None
    if r.returncode != 0:
        return None
    for line in r.stdout.splitlines():
        if line.startswith("Pages:"):
            return int(line.split(":")[1].strip())
    return None


def main():
    t0 = time.time()
    OUT.mkdir(parents=True, exist_ok=True)
    prov = []
    for name, base, repo, note in MANIFEST:
        dest = OUT / name
        if dest.exists() and dest.stat().st_size > 0:
            status = "kept"
        else:
            r = subprocess.run(
                ["curl", "-sSLf", "--max-time", "120", "-o", str(dest), base + name],
                capture_output=True, text=True, timeout=180,
            )
            if r.returncode != 0 or not dest.exists() or dest.stat().st_size == 0:
                print(f"[MISS] {name}")
                dest.unlink(missing_ok=True)
                continue
            status = "fetched"
        prov.append({
            "name": name, "repo": repo, "note": note, "url": base + name,
            "bytes": dest.stat().st_size, "pages": pdfinfo_pages(dest),
            "sha256": sha256(dest),
        })
        print(f"[{status}] {name} {dest.stat().st_size}B pages={pdfinfo_pages(dest)}")
    (OUT / "PROVENANCE.json").write_text(
        json.dumps({"fetched_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                    "files": prov}, ensure_ascii=False, indent=1),
        encoding="utf-8")
    total = sum(f["bytes"] for f in prov)
    print(f"[done] {len(prov)} 件 / {total/1048576:.1f} MB / {time.time()-t0:.0f}s")
    if len(prov) < 5:
        print("[WARN] 取件不足 5 件——语料门 sweep 断言（processed >= 5）会红")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
