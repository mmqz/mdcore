# -*- coding: utf-8 -*-
"""生成 #304 变换验收夹具：rotated_cropped.pdf（四页旋转×裁剪矩阵）。

每页在用户空间 (100, 100) 处画一个 6×6pt 的实心黑方块（标记点），
页几何构成 2×4 矩阵：

  页 1  Rotate 90  + CropBox (30,40,582,752)   ⊂ MediaBox (0,0,612,792)
  页 2  Rotate 180 + 无 CropBox
  页 3  Rotate 270 + CropBox (100,20,400,700)  （非对称偏移）
  页 4  Rotate 0   + CropBox (0,0,612,792)     （零偏移全幅）

标记点中心 = (103, 103)（方块左下角 (100,100)，边长 6）。
渲染后定位黑方块像素中心 = 设备↔用户空间变换的渲染级 oracle
（pypdfium2 渲染 → 找黑像素质心 → 与 PageTransform::user_to_screen 对比）。

工具链：reportlab 画底稿 + pypdf 设 /Rotate /CropBox（真实生成器链，
与 gen_form_fixtures.py 同族）。
"""
import io
import os

from reportlab.pdfgen import canvas
from reportlab.lib.colors import black
from pypdf import PdfReader, PdfWriter
from pypdf.generic import NameObject, NumberObject, ArrayObject, FloatObject

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                   "rotated_cropped.pdf")

# 四页参数矩阵：(rotate, crop or None)
PAGES = [
    (90, (30.0, 40.0, 582.0, 752.0)),   # 页1：旋转 + 居中偏移裁剪
    (180, None),                         # 页2：旋转 + 无裁剪
    (270, (100.0, 20.0, 400.0, 700.0)), # 页3：旋转 + 非对称偏移裁剪
    (0, (0.0, 0.0, 612.0, 792.0)),      # 页4：零旋转 + 零偏移全幅裁剪
]

MARK_X, MARK_Y, MARK_SIZE = 100.0, 100.0, 6.0  # 标记方块左下角与边长


def base_pages():
    """reportlab 画 4 页信纸底稿，每页带 (100,100) 黑方块标记。

    页角标用 40×2pt 细长条（非方形——避免 oracle 连通域形状判据
    误匹配；文字字形可能偶然近方形，结构性排除比判据调优可靠）。
    """
    buf = io.BytesIO()
    c = canvas.Canvas(buf, pagesize=(612.0, 792.0))
    for i in range(len(PAGES)):
        c.setFillColor(black)
        c.rect(MARK_X, MARK_Y, MARK_SIZE, MARK_SIZE, stroke=0, fill=1)
        # 页角标：细长条（右上，非方形）
        c.rect(540.0, 750.0, 40.0, 2.0, stroke=0, fill=1)
        c.showPage()
    c.save()
    buf.seek(0)
    return buf


def main():
    reader = PdfReader(base_pages())
    writer = PdfWriter()
    for i, page in enumerate(reader.pages):
        rotate, crop = PAGES[i]
        page[NameObject("/Rotate")] = NumberObject(rotate)
        if crop is not None:
            arr = ArrayObject([FloatObject(v) for v in crop])
            page[NameObject("/CropBox")] = arr
        writer.add_page(page)
    with open(OUT, "wb") as f:
        writer.write(f)
    print("wrote", OUT, os.path.getsize(OUT), "bytes")
    # 自检：回读四页的几何矩阵
    r2 = PdfReader(OUT)
    for i, page in enumerate(r2.pages):
        mb = [float(x) for x in page.mediabox]
        cb = [float(x) for x in page.cropbox] if "/CropBox" in page else None
        print(f"  page {i + 1}: Rotate={page.get('/Rotate')} "
              f"MediaBox={mb} CropBox={cb}")


if __name__ == "__main__":
    main()
