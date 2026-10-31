# -*- coding: utf-8 -*-
"""生成 PDF 表单测试夹具（issue #116 任务 1）。

产物：
  form_bid_letter.pdf   投标函模板（中文标签 + 文本/复选/单选/下拉域）
  form_signed.pdf       带数字签名（验证面夹具）
  stamp_logo.png        半透明红章（盖章面夹具）
"""
import io, os, sys

OUT = os.path.dirname(os.path.abspath(__file__))

# ---------------------------------------------------------------- 投标函模板
from reportlab.pdfgen import canvas as rl_canvas
from reportlab.lib.pagesizes import A4
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont

pdfmetrics.registerFont(UnicodeCIDFont("STSong-Light"))

c = rl_canvas.Canvas(os.path.join(OUT, "form_bid_letter.pdf"), pagesize=A4)
W, H = A4
c.setFont("STSong-Light", 18)
c.drawString(200, H - 80, "投 标 函")
c.setFont("STSong-Light", 11)
c.drawString(60, H - 130, "致：（招标人名称）")
c.drawString(60, H - 160, "1. 我方已仔细阅读招标文件全部内容，愿按招标文件要求承担本项目。")
c.drawString(60, H - 190, "2. 我方投标报价为（大写）：____ 元（小写）：____ 元。")
c.drawString(60, H - 220, "3. 项目名称：____________________")
c.drawString(60, H - 250, "4. 法定代表人：________________")
c.drawString(60, H - 280, "5. 工期承诺：本工程工期 ______ 日历天。")
c.drawString(60, H - 310, "6. 我方同意：同意 □   不同意 □")
c.drawString(60, H - 340, "7. 投标有效期：60 天 □   90 天 □")
c.drawString(60, H - 370, "8. 项目类别：____________________")
c.drawString(60, H - 400, "9. 我方承诺所提供的一切资料真实、有效。")
c.setFont("STSong-Light", 10)
c.drawString(330, H - 460, "投标人（盖章）：____________")
c.drawString(330, H - 490, "法定代表人或其委托代理人（签字）：____________")
c.drawString(330, H - 520, "日期：______ 年 ____ 月 ____ 日")

# ---- 表单域（reportlab 原生 AcroForm，标准 /Widget 结构）
form = c.acroForm
kw = dict(fontName="Helvetica", fontSize=10)
form.textfield(name="project_name", tooltip="项目名称", x=150, y=H - 232, width=250, height=20,
               borderStyle="inset", forceBorder=True, **kw)
form.textfield(name="legal_rep", tooltip="法定代表人", x=150, y=H - 262, width=200, height=20,
               borderStyle="inset", forceBorder=True, **kw)
form.textfield(name="duration_days", tooltip="工期（日历天）", x=150, y=H - 292, width=80, height=20,
               borderStyle="inset", forceBorder=True, maxlen=4, **kw)
form.textfield(name="bid_amount_upper", tooltip="投标报价大写", x=190, y=H - 202, width=150, height=20,
               borderStyle="inset", forceBorder=True, **kw)
form.checkbox(name="consent", tooltip="同意", x=120, y=H - 322, size=14, checked=False,
               borderStyle="solid", forceBorder=True)
form.checkbox(name="agree_valid_60", tooltip="有效期60天", x=150, y=H - 352, size=14, checked=False,
               borderStyle="solid", forceBorder=True)
form.checkbox(name="agree_valid_90", tooltip="有效期90天", x=230, y=H - 352, size=14, checked=False,
               borderStyle="solid", forceBorder=True)
form.choice(name="project_type", tooltip="项目类别", x=150, y=H - 382, width=200, height=20, value="construction",
             options=["construction", "supervision", "design", "goods"], borderStyle="inset", forceBorder=True, **kw)
c.showPage()
c.save()
print("wrote form_bid_letter.pdf")

# ---------------------------------------------------------------- 半透明公章
from PIL import Image, ImageDraw
img = Image.new("RGBA", (400, 400), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.ellipse([20, 20, 380, 380], outline=(200, 20, 20, 160), width=14)
d.ellipse([90, 90, 310, 310], outline=(200, 20, 20, 160), width=8)
d.polygon([(200, 150), (230, 240), (170, 240)], fill=(200, 20, 20, 170))
img.save(os.path.join(OUT, "stamp_seal.png"))
print("wrote stamp_seal.png")
