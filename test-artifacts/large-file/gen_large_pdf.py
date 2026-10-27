#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""R-1 大文件夹具：500 页 PDF（reportlab——引擎外独立产线，R-1 度量
对象是 pdf.js 视图打开/首渲/内存面，非引擎写路径）。

用法（仓库根）：python3 test-artifacts/large-file/gen_large_pdf.py [out.pdf] [pages]
默认 out = test-artifacts/large-file/large-500p.pdf，pages = 500。
每页：标题 + 三段变体文本 + 一张 4×4 表（模拟标书页密度）。
产物不入库（.gitignore）。
"""
import sys
from pathlib import Path

from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.platypus import (PageBreak, Paragraph, SimpleDocTemplate,
                                   Spacer, Table, TableStyle)

POOL = [
    "本项目为智能文档平台建设，涵盖需求解析、协同编辑与格式保真三大子系统，"
    "投标人应具备同类项目实施经验并提供近三年内不少于两个案例证明材料。",
    "系统须支持国产化环境部署，包括信创操作系统与主流数据库适配能力，"
    "服务响应时间为工作日九点至十八点电话支持，两小时内远程响应。",
    "验收标准以本技术要求书逐项核对为准，任何偏离项须在投标文件中明确示出，"
    "评标委员会保留对技术方案进行澄清与质询的权利。",
]


def para_text(page: int, idx: int) -> str:
    return f"第{page}页第{idx}节：{POOL[(page + idx) % len(POOL)]}"


def main() -> int:
    out = Path(sys.argv[1] if len(sys.argv) > 1 else
               Path(__file__).parent / "large-500p.pdf")
    pages = int(sys.argv[2]) if len(sys.argv) > 2 else 500
    styles = getSampleStyleSheet()
    doc = SimpleDocTemplate(str(out), pagesize=A4,
                            title=f"R-1 large fixture ({pages} pages)")
    story = []
    for p in range(1, pages + 1):
        story.append(Paragraph(f"大文件边界测试文档 · 第 {p} 页", styles["Heading1"]))
        for i in range(3):
            story.append(Paragraph(para_text(p, i), styles["BodyText"]))
            story.append(Spacer(1, 6))
        table = Table(
            [[f"R{p}-{r}C{c}" for c in range(4)] for r in range(4)],
            colWidths=[110] * 4,
        )
        table.setStyle(TableStyle([("GRID", (0, 0), (-1, -1), 0.5, "black")]))
        story.append(table)
        story.append(PageBreak())  # 每单元一页——pages 参数即物理页数
    doc.build(story)
    print(f"pdf path={out} pages={pages} bytes={out.stat().st_size}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
