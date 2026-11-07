#!/usr/bin/env python3
"""#268 分栏语料取件：LibreOffice 测试仓真实多栏 docx（13 件）。

来源 = LibreOffice/core `sw/qa/extras/ooxmlexport/data/`（MPL-2.0，
Word/LO 互操作回归样本——tdf 编号 = LibreOffice bugzilla 回归件）。
选件口径（issue #268 验收「≥5 件真实多栏 docx」）：
  · sectPr/w:cols num>=2 或 w:col 子元素在场（真实多栏形态）；
  · 覆盖面：num=2/3/4、equalWidth=false+w:col 子表（tdf121665）、
    栏分隔线 sep（multi-column-separator-with-line / tdf153592）、
    显式分栏符 w:br type=column（columnbreak / tdf121665 /
    tdf153592）、多节分栏（tdf135343/tdf121670/unbalanced 等）。

与 #264 pdf-corpus / #265 xlsx 语料同族纪律：**docx 二进制小件
（<21KB/件，共 ~172KB）直接入库**（与 health-check/corpus 的 642 件
docx 同例——CI 无网可跑）；本脚本供 provenance 复核/补件（幂等：
已存在跳过）；逐件 sha256 见 PROVENANCE.json。

Windows CI 控制台 cp1252 注记：stdout 重配 UTF-8（#262 取件脚本
同族教训）。
"""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

OUT = Path(__file__).resolve().parent
BASE = "https://raw.githubusercontent.com/LibreOffice/core/master/sw/qa/extras/ooxmlexport/data/"

# (文件名, 测试点一句话)
MANIFEST = [
    ("2col-header.docx", "双栏节 + 页眉（num=2 space=720）"),
    ("alphabeticalIndex_MultipleColumns.docx", "多节索引分栏（含 num=4 节）"),
    ("columnbreak.docx", "双栏 + 显式分栏符（num=2 space=708）"),
    ("default-sect-break-cols.docx", "多节分栏（2 栏节 + 缺省节交替）"),
    ("multi-column-line-separator-SAVED.docx", "equalWidth/sep 显式属性面"),
    ("multi-column-separator-with-line.docx", "栏分隔线 sep=1 + 多节"),
    ("tdf121665_back2backColumnBreaks.docx", "三栏 equalWidth=false + w:col 子表 + 背靠背分栏符（全形态件）"),
    ("tdf121669_equalColumns.docx", "三栏等宽（num=3）"),
    ("tdf121670_columnsInSectionsOnly.docx", "分栏只属节面（3 节 3 栏→3 栏→缺省）"),
    ("tdf135343_columnSectionBreak_c12v3.docx", "多节异构（num=4 → num=2）"),
    ("tdf153592_columnBreaks.docx", "三栏全属性（space=850 equalWidth sep）+ 分栏符"),
    ("tdf46940_dontEquallyDistributeColumns.docx", "双栏 space=290 窄间距"),
    ("unbalanced-columns.docx", "三节双栏（平衡/非平衡栏面）"),
]

provenance = []
for name, note in MANIFEST:
    target = OUT / name
    if not target.exists():
        r = subprocess.run(
            ["curl", "-sSfL", "--connect-timeout", "10", "--max-time", "30",
             BASE + name, "-o", str(target)],
            capture_output=True,
        )
        if r.returncode != 0:
            print(f"[fetch] FAIL {name}: {r.stderr.decode(errors='replace')}")
            continue
        print(f"[fetch] downloaded {name}")
    data = target.read_bytes()
    provenance.append({
        "file": name,
        "source_repo": "LibreOffice/core",
        "source_path": f"sw/qa/extras/ooxmlexport/data/{name}",
        "url": BASE + name,
        "license": "MPL-2.0",
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "note": note,
    })

(OUT / "PROVENANCE.json").write_text(
    json.dumps(provenance, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
)
print(f"[fetch] PROVENANCE.json updated: {len(provenance)} entries")
