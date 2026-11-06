# -*- coding: utf-8 -*-
"""生成表格结构化提取夹具（issue #232 / R9·P1）。

产物：
  table_grid.pdf   合成评分表（4 列 × 6 行，含表头 + 4 数据行 + 1 合计
                   行[合并格] + 页首标题[单格行——不应入表] + 表后段落
                   [单格行——不应入表]）

金标准（#232 验收锚——单元格文本与列序对照本表）：
  表（1 张，6 行，列序 0..3）：
    行 1（表头）  : 序号 | 评分项 | 分值 | 评分标准            col=[0,1,2,3]
    行 2          : 1 | 技术方案 | 20 | 方案完整、技术路线合理
    行 3          : 2 | 商务响应 | 15 | 响应偏离度小
    行 4          : 3 | 实施计划 | 10 | 计划可行、里程碑清晰
    行 5          : 4 | 售后服务 | 5  | 服务承诺明确
    行 6（合计行）: 合计（技术+商务）→ col_index=0、col_span=2（跨
                    序号+评分项两列锚，文本右缘 ≈ 153 越过锚 140）|
                    35 → col_index=2（分值列）
  列左缘锚（drawString 左缘）：序号 60 / 评分项 140 / 分值 330 / 评分标准 400
  页首「评分表」（18pt，单格）与表后段落（11pt，单格）**不得**并入表
  ——单格行不入表的算法锚。

用法：python3 gen_table_fixture.py（在 fixtures/ 目录内执行）
"""
import os

from reportlab.lib.pagesizes import A4
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont
from reportlab.pdfgen import canvas as rl_canvas

OUT = os.path.dirname(os.path.abspath(__file__))

pdfmetrics.registerFont(UnicodeCIDFont("STSong-Light"))

c = rl_canvas.Canvas(os.path.join(OUT, "table_grid.pdf"), pagesize=A4)
W, H = A4

# 列 x 锚（金标准）：与模块 doc 同源。
COLS = [60, 140, 330, 400]

# 页首标题（单格行——不入表）。
c.setFont("STSong-Light", 18)
c.drawString(260, H - 70, "评分表")

# 表头行。
c.setFont("STSong-Light", 11)
y = H - 120
headers = ["序号", "评分项", "分值", "评分标准"]
for x, text in zip(COLS, headers):
    c.drawString(x, y, text)

# 数据行（5 行，行距 26pt）。
rows = [
    ["1", "技术方案", "20", "方案完整、技术路线合理"],
    ["2", "商务响应", "15", "响应偏离度小"],
    ["3", "实施计划", "10", "计划可行、里程碑清晰"],
    ["4", "售后服务", "5", "服务承诺明确"],
]
for row in rows:
    y -= 26
    for x, text in zip(COLS, row):
        c.drawString(x, y, text)

# 合计行：首格从锚 60（列 0）起笔，文本右缘 ≈ 153 越过锚 140（列 1）
# → col_index=0、col_span=2；分值列独立“35”（不与合并格重叠——真实
# 合并格语义：合并区域内无其他列内容）。
y -= 26
c.drawString(60, y, "合计（技术+商务）")
c.drawString(COLS[2], y, "35")

# 表后段落（单格行——不入表）。
y -= 40
c.setFont("STSong-Light", 11)
c.drawString(60, y, "注：本表为合成夹具，用于表格结构化提取的行列重建验收。")

c.save()
print("generated:", os.path.join(OUT, "table_grid.pdf"))
