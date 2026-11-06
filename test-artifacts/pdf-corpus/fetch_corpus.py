#!/usr/bin/env python3
"""#264 真实 PDF 语料取件脚本。

来源（全部 Apache-2.0 仓的测试语料，与 GPL 主仓分发兼容——
语料文件本身按原仓许可再分发，逐件见 PROVENANCE.md）：
  1. mozilla/pdf.js   test/pdfs/        （真实世界捐赠件：bug 附件/
     书籍/学术论文/注释/表单——Apache-2.0）
  2. apache/pdfbox    pdfbox/src/test/resources/（加密全家族/
     渲染真实件——Apache-2.0）
  3. qpdf/qpdf        test/ 边缘件       （Apache-2.0）

纪律（对齐 pdf-organize-corpus 先例）：
  · 取件脚本 + PROVENANCE + README 入库；PDF 二进制不入库
    （.gitignore：test-artifacts/pdf-corpus/**/*.pdf）
  · 单件 ≤5MB；重建 = python3 fetch_corpus.py（幂等：已存在跳过）
"""
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

# Windows CI：控制台/默认文件编码是 cp1252——中文注记（PROVENANCE
# note / [WARN] 横幅）会 UnicodeEncodeError 炸取件步骤（CI 首跑实测）。
# stdout/stderr 重配 UTF-8 + 写文件显式 encoding="utf-8"。
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

OUT = Path(__file__).resolve().parent

PDFJS = "https://raw.githubusercontent.com/mozilla/pdf.js/master/test/pdfs/"
PDFBOX = "https://raw.githubusercontent.com/apache/pdfbox/trunk/pdfbox/src/test/resources/"

# (分类, 文件名, 源URL前缀, 来源仓, 测试点一句话)
MANIFEST = [
    # ---- encrypted/ 加密全家族（PDFBox 10 + pdf.js 1）----
    ("encrypted", "AES128ExposedMeta.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "AES-128 加密、元数据可读"),
    ("encrypted", "AES256ExposedMeta.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "AES-256 加密、元数据可读"),
    ("encrypted", "AESkeylength128.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "AES-128 密钥长度探针"),
    ("encrypted", "AESkeylength256.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "AES-256 密钥长度探针"),
    ("encrypted", "PasswordSample-128bit.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "128bit 密码加密样张"),
    ("encrypted", "PasswordSample-256bit.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "256bit 密码加密样张"),
    ("encrypted", "PasswordSample-40bit.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "RC4-40bit 密码加密（老式）"),
    ("encrypted", "preEnc_20141025_105451.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "真实预加密件（扫描产物）"),
    ("encrypted", "Acroform-PDFBOX-2333.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "加密 + AcroForm 混合形态"),
    ("encrypted", "test.pdf", PDFBOX + "org/apache/pdfbox/encryption/", "apache/pdfbox", "加密基线件"),
    ("encrypted", "encrypted-attachment.pdf", PDFJS, "mozilla/pdf.js", "加密容器内嵌附件"),
    # ---- forms/ 表单（AcroForm + XFA）----
    ("forms", "annotation-button-widget.pdf", PDFJS, "mozilla/pdf.js", "按钮控件 AcroForm"),
    ("forms", "annotation-choice-widget.pdf", PDFJS, "mozilla/pdf.js", "选择控件 AcroForm"),
    ("forms", "annotation-text-widget.pdf", PDFJS, "mozilla/pdf.js", "文本控件 AcroForm"),
    ("forms", "acroform_calculation_order.pdf", PDFJS, "mozilla/pdf.js", "字段计算顺序 CO 数组"),
    ("forms", "file_pdfjs_form.pdf", PDFJS, "mozilla/pdf.js", "pdf.js 自产基础表单"),
    ("forms", "form_two_pages.pdf", PDFJS, "mozilla/pdf.js", "跨两页表单字段"),
    ("forms", "checkbox-bad-appearance.pdf", PDFJS, "mozilla/pdf.js", "坏 appearance 流的复选框"),
    ("forms", "annotation-tx.pdf", PDFJS, "mozilla/pdf.js", "文本字段 appearance"),
    ("forms", "annotation-tx2.pdf", PDFJS, "mozilla/pdf.js", "文本字段 appearance 变体"),
    ("forms", "xfa_issue14315.pdf", PDFJS, "mozilla/pdf.js", "XFA 动态表单"),
    ("forms", "xfa_filled_imm1344e.pdf", PDFJS, "mozilla/pdf.js", "真实加拿大移民表单（已填写 XFA，2.9MB）"),
    # ---- annotations/ 注释 13 面 ----
    ("annotations", "annotation-border-styles.pdf", PDFJS, "mozilla/pdf.js", "边框样式族"),
    ("annotations", "annotation-caret-ink.pdf", PDFJS, "mozilla/pdf.js", "插入符 + 墨迹"),
    ("annotations", "annotation-fileattachment.pdf", PDFJS, "mozilla/pdf.js", "文件附件注释"),
    ("annotations", "annotation-freetext.pdf", PDFJS, "mozilla/pdf.js", "自由文本"),
    ("annotations", "annotation-highlight.pdf", PDFJS, "mozilla/pdf.js", "高亮"),
    ("annotations", "annotation-line.pdf", PDFJS, "mozilla/pdf.js", "线条"),
    ("annotations", "annotation-link-text-popup.pdf", PDFJS, "mozilla/pdf.js", "链接 + 弹窗"),
    ("annotations", "annotation-polyline-polygon.pdf", PDFJS, "mozilla/pdf.js", "折线/多边形"),
    ("annotations", "annotation-square-circle.pdf", PDFJS, "mozilla/pdf.js", "矩形/椭圆"),
    ("annotations", "annotation-squiggly.pdf", PDFJS, "mozilla/pdf.js", "波浪线"),
    ("annotations", "annotation-stamp.pdf", PDFJS, "mozilla/pdf.js", "图章"),
    ("annotations", "annotation-strikeout.pdf", PDFJS, "mozilla/pdf.js", "删除线"),
    ("annotations", "annotation-underline.pdf", PDFJS, "mozilla/pdf.js", "下划线"),
    # ---- bookmarks/ 书签 ----
    ("bookmarks", "outline_goto_action.pdf", PDFJS, "mozilla/pdf.js", "大纲 + GoTo 动作"),
    ("bookmarks", "outlines_for_editor.pdf", PDFJS, "mozilla/pdf.js", "编辑器大纲"),
    ("bookmarks", "nested_outline.pdf", PDFJS, "mozilla/pdf.js", "嵌套大纲"),
    ("bookmarks", "outlines_se.pdf", PDFJS, "mozilla/pdf.js", "瑞典语大纲（非 ASCII 标题）"),
    # ---- scanned/ 扫描 ----
    ("scanned", "scan-bad.pdf", PDFJS, "mozilla/pdf.js", "坏扫描件"),
    ("scanned", "perf_images_scan_bilevel.pdf", PDFJS, "mozilla/pdf.js", "双级扫描图像"),
    ("scanned", "perf_images_scan_gray.pdf", PDFJS, "mozilla/pdf.js", "灰度扫描图像（1.7MB）"),
    # ---- multicolumn/ 多栏 ----
    ("multicolumn", "tracemonkey_with_editable_annotations.pdf", PDFJS, "mozilla/pdf.js", "学术论文双栏 + 可编辑注释（pdf.js 渲染基准件，1.2MB）"),
    ("multicolumn", "freeculture.pdf", PDFJS, "mozilla/pdf.js", "《Free Culture》整书（Lawrence Lessig，书籍排版，2.5MB）"),
    ("multicolumn", "survey.pdf", PDFBOX + "input/rendering/", "apache/pdfbox", "学术调查问卷（双栏 + 表格）"),
    # ---- mixed/ 真实世界混合 ----
    ("mixed", "comments.pdf", PDFJS, "mozilla/pdf.js", "tracemonkey 论文注释版（真实评论导出，1MB）"),
    ("mixed", "highlights.pdf", PDFJS, "mozilla/pdf.js", "真实高亮集（946KB）"),
    ("mixed", "issue5567.pdf", PDFJS, "mozilla/pdf.js", "用户 bug 附件（2.1MB 真实文档）"),
    ("mixed", "issue6961.pdf", PDFJS, "mozilla/pdf.js", "用户 bug 附件（2.1MB）"),
    ("mixed", "issue6286.pdf", PDFJS, "mozilla/pdf.js", "用户 bug 附件（1.9MB）"),
    ("mixed", "issue19971.pdf", PDFJS, "mozilla/pdf.js", "用户 bug 附件（1.7MB）"),
    ("mixed", "issue8795_reduced.pdf", PDFJS, "mozilla/pdf.js", "裁剪后的真实件（4.1MB）"),
    ("mixed", "issue10529.pdf", PDFJS, "mozilla/pdf.js", "隐藏文本编码件（真实用户场景）"),
    ("mixed", "issue10301.pdf", PDFJS, "mozilla/pdf.js", "希伯来文 RTL 文本件"),
    ("mixed", "issue1045.pdf", PDFJS, "mozilla/pdf.js", "用户件（裁剪字体形）"),
    ("mixed", "issue10402.pdf", PDFJS, "mozilla/pdf.js", "用户件（裁剪字体形）"),
    ("mixed", "issue11144_reduced.pdf", PDFJS, "mozilla/pdf.js", "裁剪后的真实件"),
    ("mixed", "issue11150_reduced.pdf", PDFJS, "mozilla/pdf.js", "裁剪后的真实件"),
    ("mixed", "issue10084_reduced.pdf", PDFJS, "mozilla/pdf.js", "裁剪后的真实件"),
    ("mixed", "4PP-Highlighting.pdf", PDFBOX + "input/rendering/", "apache/pdfbox", "双栏高亮渲染 4 页件"),
    ("mixed", "tiger-as-form-xobject.pdf", PDFBOX + "input/rendering/", "apache/pdfbox", "Form XObject 嵌套绘制"),
    ("mixed", "PDFJS-9279-reduced.pdf", PDFJS, "mozilla/pdf.js", "裁剪后的真实件（3MB）"),
    # ---- broken/ 损坏与结构异常 ----
    ("broken", "bad-PageLabels.pdf", PDFJS, "mozilla/pdf.js", "坏 PageLabels 数组"),
    ("broken", "boundingBox_invalid.pdf", PDFJS, "mozilla/pdf.js", "无效 boundingBox"),
    ("broken", "helloworld-bad.pdf", PDFJS, "mozilla/pdf.js", "坏 HelloWorld"),
    ("broken", "devicen-none-zero-domain.pdf", PDFJS, "mozilla/pdf.js", "DeviceN 零域"),
    ("broken", "zerowidthline.pdf", PDFJS, "mozilla/pdf.js", "零宽线"),
    ("broken", "multiple-filters-length-zero.pdf", PDFJS, "mozilla/pdf.js", "多过滤器零长度流"),
    ("broken", "zero_descent.pdf", PDFJS, "mozilla/pdf.js", "零下降字体度量"),
    ("broken", "annotation-line-without-appearance-empty-Rect.pdf", PDFJS, "mozilla/pdf.js", "空 Rect 注释"),
    ("broken", "issue10438_reduced.pdf", PDFJS, "mozilla/pdf.js", "结构异常裁剪件"),
    ("broken", "issue10542_reduced.pdf", PDFJS, "mozilla/pdf.js", "结构异常裁剪件"),
    ("broken", "issue1055r.pdf", PDFJS, "mozilla/pdf.js", "结构异常件"),
    ("broken", "issue11124.pdf", PDFJS, "mozilla/pdf.js", "零页结构异常"),
    # ---- captions/ 图注（图文混排）----
    ("captions", "issue11045.pdf", PDFJS, "mozilla/pdf.js", "机翼图 Figure 1 图注（X_NP/X_CG 标注）"),
    ("captions", "issue10640.pdf", PDFJS, "mozilla/pdf.js", "Open Sans LATEX 文档（figure 引用排版）"),
    # ---- tables/ 真实表格 ----
    ("tables", "issue10900.pdf", PDFJS, "mozilla/pdf.js", "四列数值对齐表格（真实数据）"),
    ("tables", "source.pdf", PDFBOX + "input/rendering/", "apache/pdfbox", "捷克语报纸表格布局（mBank 框架表格）"),
]


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    h.update(p.read_bytes())
    return h.hexdigest()


def pdfinfo_pages(p: Path):
    """页数探针——pdfinfo 缺席（CI Windows runner 无 poppler）记
    None 不阻断（FileNotFoundError 会直接炸取件步骤）。"""
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


def fetch(url: str, dest: Path) -> bool:
    r = subprocess.run(
        ["curl", "-sSLf", "--max-time", "120", "-o", str(dest), url],
        capture_output=True, text=True, timeout=180,
    )
    return r.returncode == 0 and dest.exists() and dest.stat().st_size > 0


def main():
    t0 = time.time()
    prov = []
    for cat, name, base, repo, note in MANIFEST:
        d = OUT / cat
        d.mkdir(parents=True, exist_ok=True)
        dest = d / name
        url = base + name
        if dest.exists() and dest.stat().st_size > 0:
            status = "kept"
        else:
            ok = fetch(url, dest)
            if not ok:
                print(f"[MISS] {cat}/{name}")
                dest.unlink(missing_ok=True)
                continue
            status = "fetched"
        pages = pdfinfo_pages(dest)
        prov.append({
            "category": cat, "name": name, "repo": repo, "note": note,
            "url": url, "bytes": dest.stat().st_size, "pages": pages,
            "sha256": sha256(dest),
        })
        print(f"[{status}] {cat}/{name} {dest.stat().st_size}B pages={pages}")
    (OUT / "PROVENANCE.json").write_text(
        json.dumps({"fetched_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                    "files": prov}, ensure_ascii=False, indent=1),
        encoding="utf-8")
    total = sum(f["bytes"] for f in prov)
    print(f"[done] {len(prov)} 件 / {total/1048576:.1f} MB / {time.time()-t0:.0f}s")
    # 语料门断言 files.len() >= 70——取件不足在此显式退出（CI 快速失败
    # 且信息直指上游漂移，而不是让测试断言兜底报「shrank」）。
    if len(prov) < 70:
        print(f"[WARN] 取件 {len(prov)} 件 < 70 下限——上游源漂移（[MISS] 见上）")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
