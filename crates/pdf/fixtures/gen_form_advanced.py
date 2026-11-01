# -*- coding: utf-8 -*-
"""生成高级表单测试夹具（issue #116 后续任务）。

form_advanced.pdf —— 2 页，覆盖：
  · 必填域（/Ff bit1 = 1）
  · 超长限制（/MaxLen）
  · 选项域（/Ch /Opt）
  · **跨页同名字段**（/Kids，同一 /T 挂在多页 Widget 上）
"""
import os, sys
sys.stdout.reconfigure(encoding="utf-8")
from reportlab.pdfgen import canvas as rl_canvas
from reportlab.lib.pagesizes import A4
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont

OUT = os.path.dirname(os.path.abspath(__file__))
pdfmetrics.registerFont(UnicodeCIDFont('STSong-Light'))

c = rl_canvas.Canvas(os.path.join(OUT, 'form_advanced.pdf'), pagesize=A4)
W, H = A4
form = c.acroForm
kw = dict(fontName='Helvetica', fontSize=10, borderStyle='inset', forceBorder=True)

# ===================== 第 1 页 =====================
c.setFont('STSong-Light', 16)
c.drawString(60, H - 70, '投标函（第一页）')
c.setFont('STSong-Light', 11)
c.drawString(60, H - 120, '1. 项目名称（必填）：')
c.drawString(60, H - 160, '2. 法定代表人（必填）：')
c.drawString(60, H - 200, '3. 工期日历天（最长 4 位）：')
c.drawString(60, H - 240, '4. 项目类别：')
c.drawString(60, H - 280, '5. 投标人名称（跨页字段）：')

form.textfield(name='project_name', tooltip='项目名称',
               x=200, y=H - 133, width=280, height=20,
               fieldFlags='required', maxlen=60, **kw)
form.textfield(name='legal_rep', tooltip='法定代表人',
               x=200, y=H - 173, width=280, height=20,
               fieldFlags='required', maxlen=30, **kw)
form.textfield(name='duration_days', tooltip='工期',
               x=200, y=H - 213, width=80, height=20, maxlen=4, **kw)
form.choice(name='project_type', tooltip='项目类别',
             value='construction',
             options=['construction', 'supervision', 'design', 'goods'],
             x=200, y=H - 253, width=200, height=20, **kw)
# 跨页字段：第 1 页先建一次（同名，reportlab 会各自建独立域，
# 这里先建，第 2 页再建一个同名域，形成多页同名并存）
form.textfield(name='bidder_name', tooltip='投标人名称',
               x=200, y=H - 293, width=280, height=20, maxlen=60, **kw)

c.setFont('STSong-Light', 10)
c.drawString(60, H - 340, '注：带 * 为必填项。')
c.showPage()

# ===================== 第 2 页 =====================
c.setFont('STSong-Light', 16)
c.drawString(60, H - 70, '投标函（第二页）')
c.setFont('STSong-Light', 11)
c.drawString(60, H - 120, '6. 投标人名称（跨页字段，续）：')
c.drawString(60, H - 160, '7. 报价大写（必填）：')
c.drawString(60, H - 200, '8. 同意条款：')

form.textfield(name='bidder_name', tooltip='投标人名称',
               x=200, y=H - 133, width=280, height=20, maxlen=60, **kw)
form.textfield(name='amount_upper', tooltip='报价大写',
               x=200, y=H - 173, width=280, height=20,
               fieldFlags='required', maxlen=50, **kw)
form.checkbox(name='consent', tooltip='同意',
              x=200, y=H - 203, size=14, checked=False,
              borderStyle='solid', forceBorder=True)
c.showPage()
c.save()
print('wrote form_advanced.pdf')