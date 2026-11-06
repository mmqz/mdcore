#!/usr/bin/env python3
"""#265 XLSX 结构面语料取件脚本。

来源：Apache POI test-data/spreadsheet（Apache-2.0——与 GPL-3.0
主仓分发兼容；真实 bug 附件/真实文档，逐件见 PROVENANCE.md）。

四类（issue #265 盘点缺口）：
  hidden_rows/     隐藏行列（POI bug 附件——真实隐藏形态）
  external_links/  外部链接（externalLinks 部件 + TargetMode=External）
  num_formats/     自定义数字格式（numFmts + builtin 日期段）
  style_inherit/   样式继承链（cellXfs/cellStyleXfs——含巨型 3088）

纪律（pdf-corpus 先例同构）：取件脚本 + PROVENANCE + README 入库；
xlsx 二进制不入库（.gitignore）；单件 ≤5MB；幂等（已存在跳过）。
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
POI = "https://raw.githubusercontent.com/apache/poi/trunk/test-data/spreadsheet/"

# (分类, 文件名, 测试点)
MANIFEST = [
    # ---- hidden_rows/ 隐藏行列（8）----
    ("hidden_rows", "55640.xlsx", "sheet1 四行隐藏（r=1..4）——POI bug 附件"),
    ("hidden_rows", "AverageTaxRates.xlsx", "真实税率表：三表 19 行次隐藏 + B 列隐藏×3"),
    ("hidden_rows", "ConditionalFormattingSamples.xlsx", "条件格式样本：14 行隐藏 + 0%/$/mm/dd 格式"),
    ("hidden_rows", "FillWithoutColor.xlsx", "47 行 + AA..宽列段隐藏（大范围形态）"),
    ("hidden_rows", "GroupTest.xlsx", "分组形态：13 行隐藏（组折叠）"),
    ("hidden_rows", "WidthsAndHeights.xlsx", "宽高设置件：2 行 + 1 列隐藏"),
    ("hidden_rows", "bug65464.xlsx", "POI bug 附件：5 行 + 3 列隐藏"),
    ("hidden_rows", "clone_sheet.xlsx", "克隆表形态：2 列隐藏"),
    # ---- external_links/ 外部链接（8）----
    ("external_links", "54399.xlsx", "双外部链接（Top 150 + Craft venue 两个外部源）"),
    ("external_links", "56502.xlsx", "外部链接（POI bug 附件）"),
    ("external_links", "63934.xlsx", "外部链接 + #,##0 builtin 格式"),
    ("external_links", "HsGetVal.xlsx", "三表外部链接（HsGetVal 跨簿引用）"),
    ("external_links", "bug67784.xlsx", "外部链接（POI bug 附件）"),
    ("external_links", "link-external-workbook-b.xlsx", "外部链接对之 b（a↔b 链接形态）"),
    ("external_links", "ref-56737.xlsx", "跨簿引用（POI bug 附件）"),
    ("external_links", "ref2-56737.xlsx", "跨簿引用 2（双链接）"),
    # ---- num_formats/ 数字格式（6）----
    ("num_formats", "45540_classic_Header.xlsx", "5 自定义格式（classic 页眉表）"),
    ("num_formats", "45544.xlsx", "5 自定义格式（7 表）"),
    ("num_formats", "47804.xlsx", "4 自定义格式（POI bug 附件）"),
    ("num_formats", "48962.xlsx", "4 自定义 + builtin 日期段（m/d/yyyy/d-mmm）"),
    ("num_formats", "52348.xlsx", "3 自定义格式（POI bug 附件）"),
    ("num_formats", "56274.xlsx", "自定义格式（POI bug 附件）"),
    # ---- style_inherit/ 样式继承链（6）----
    ("style_inherit", "49156.xlsx", "xf 继承（POI bug 附件）"),
    ("style_inherit", "49273.xlsx", "466 cellXfs（样式洪水——结构面吞吐锚点）"),
    ("style_inherit", "53568.xlsx", "3088 cellStyleXfs（巨型样式表）"),
    ("style_inherit", "54288-ref.xlsx", "外部引用样式（6 自定义格式 + xf 链）"),
    ("style_inherit", "54288.xlsx", "9 cellXfs + 19 cellStyleXfs（xfId 继承链）"),
    ("style_inherit", "55406_Conditional_formatting_sample.xlsx", "641 cellStyleXfs（条件格式样式族）"),
]


def sha256(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    t0 = time.time()
    prov = []
    for cat, name, note in MANIFEST:
        d = OUT / cat
        d.mkdir(parents=True, exist_ok=True)
        dest = d / name
        if dest.exists() and dest.stat().st_size > 0:
            status = "kept"
        else:
            r = subprocess.run(
                ["curl", "-sSLf", "--max-time", "60", "-o", str(dest), POI + name],
                capture_output=True, text=True, timeout=120,
            )
            if r.returncode != 0 or not dest.exists() or dest.stat().st_size == 0:
                print(f"[MISS] {cat}/{name}")
                dest.unlink(missing_ok=True)
                continue
            status = "fetched"
        prov.append({
            "category": cat, "name": name, "repo": "apache/poi",
            "url": POI + name, "note": note,
            "bytes": dest.stat().st_size, "sha256": sha256(dest),
        })
        print(f"[{status}] {cat}/{name} {dest.stat().st_size}B")
    (OUT / "PROVENANCE.json").write_text(
        json.dumps({"fetched_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                    "files": prov}, ensure_ascii=False, indent=1),
        encoding="utf-8")
    print(f"[done] {len(prov)} 件 / {sum(f['bytes'] for f in prov)/1024:.0f}KB / {time.time()-t0:.0f}s")
    # 语料门断言 files.len() >= 26——取件不足在此显式退出（CI 快速
    # 失败且信息直指上游漂移，而不是让测试断言兜底报「shrank」）。
    if len(prov) < 26:
        print(f"[WARN] 取件 {len(prov)} 件 < 26 下限——上游源漂移（[MISS] 见上）")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
