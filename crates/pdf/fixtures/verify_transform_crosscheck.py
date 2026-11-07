# -*- coding: utf-8 -*-
"""#304 变换公式 × 真实 pdfium 渲染交叉验证（第三意见）。

方法：构造已知用户空间内容的 PDF（红方块 @ 已知用户坐标 + /Rotate +
偏移 CropBox）→ pypdfium2 真渲染 → 位图里找红方块像素位置 →
用 Rust transform 的同款公式（Python 移植）反算用户坐标 → 应回到
已知坐标。四旋转 × 偏移 CropBox 全验证。
"""
import fitz  # PyMuPDF（构造）
import pypdfium2 as pdfium  # 真 pdfium（渲染）

SQ = (100.0, 132.0, 160.0, 192.0)  # 用户空间红方块 [x0, y0, x1, y1]
# （PyMuPDF 的 draw_rect(100,600,160,660) 是顶左 y 向下坐标；PDF 用户
# 空间 y = 792-660..792-600 = 132..192——内容流实测 `100 132 60 60 re`）
CENTER = ((SQ[0] + SQ[2]) / 2, (SQ[1] + SQ[3]) / 2)  # (130, 162)
CROP = (50.0, 40.0, 562.0, 752.0)  # 偏移 CropBox（⊂ MediaBox 非零偏移）


def make_pdf(rotate_deg: int, path: str):
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)  # MediaBox [0,0,612,792]
    # fitz 顶左坐标系里的方块（y_down 600..660 → 用户空间 y 132..192）。
    page.draw_rect(fitz.Rect(100, 600, 160, 660), color=(1, 0, 0), fill=(1, 0, 0))
    page.set_cropbox(fitz.Rect(50, 40, 562, 752))  # → 用户空间 [50,40,562,752]
    page.set_rotation(rotate_deg)
    doc.save(path)
    doc.close()


def find_red_pixels(pil):
    w, h = pil.size
    px = pil.load()
    xs, ys = [], []
    for y in range(0, h, 2):  # 步长 2 加速（坐标仍为真实像素坐标）
        for x in range(0, w, 2):
            r, g, b = px[x, y][:3]
            if r > 200 and g < 80 and b < 80:
                xs.append(x)
                ys.append(y)
    if not xs:
        return None
    return (min(xs), max(xs), min(ys), max(ys))


def u2d(ux, uy, c, rot):
    cx0, cy0, cx1, cy1 = c
    if rot == 0:
        return (ux - cx0, cy1 - uy)
    if rot == 90:
        return (uy - cy0, ux - cx0)
    if rot == 180:
        return (cx1 - ux, uy - cy0)
    if rot == 270:
        return (cy1 - uy, cx1 - ux)


def d2u(dx, dy, c, rot):
    cx0, cy0, cx1, cy1 = c
    if rot == 0:
        return (dx + cx0, cy1 - dy)
    if rot == 90:
        return (dy + cx0, dx + cy0)
    if rot == 180:
        return (cx1 - dx, dy + cy0)
    if rot == 270:
        return (cx1 - dy, cy1 - dx)


for rot in (0, 90, 180, 270):
    path = f"/tmp/t304_rot{rot}.pdf"
    make_pdf(rot, path)

    doc = pdfium.PdfDocument(path)
    page = doc[0]
    # ① 显示域尺寸 vs 我的 display_width/height（CropBox 基 + 旋转互换）
    disp_w, disp_h = page.get_size()  # pdfium 显示尺寸（点）
    cw, ch = CROP[2] - CROP[0], CROP[3] - CROP[1]
    my_w, my_h = (ch, cw) if rot in (90, 270) else (cw, ch)
    ok_size = abs(disp_w - my_w) < 0.01 and abs(disp_h - my_h) < 0.01
    print(f"rot={rot}: pdfium 显示尺寸 ({disp_w:.2f},{disp_h:.2f}) vs 公式 ({my_w},{my_h}) → {'✓' if ok_size else '✗'}")

    # ② 渲染 @ 96 DPI → 位图找红方块
    dpi = 96.0
    scale = dpi / 72.0
    pil = page.render(scale=scale).to_pil()
    box = find_red_pixels(pil)
    assert box, f"rot={rot}: 渲染位图中未找到红方块"
    # 像素中心（步长采样已 ×2 还原；方块像素中心 ≈ 几何中心）
    px_c = (box[0] + box[1]) / 2
    py_c = (box[2] + box[3]) / 2
    # ③ 我的公式反算用户坐标（像素 → 显示点 → 用户点）
    dx, dy = px_c * 72.0 / dpi, py_c * 72.0 / dpi
    ux, uy = d2u(dx, dy, CROP, rot)
    err = max(abs(ux - CENTER[0]), abs(uy - CENTER[1]))
    ok_pt = err < 1.5  # < 1.5pt（采样步长 2px @ 96DPI ≈ 1.5pt 量化误差）
    print(f"rot={rot}: 像素中心 ({px_c:.1f},{py_c:.1f}) → 公式用户坐标 ({ux:.2f},{uy:.2f}) vs 真值 ({CENTER[0]},{CENTER[1]}) → 误差 {err:.2f}pt {'✓' if ok_pt else '✗'}")

    # ④ 正向验证：已知用户中心 → 公式 → 像素 ≈ 实测像素中心
    dx2, dy2 = u2d(CENTER[0], CENTER[1], CROP, rot)
    sx, sy = dx2 * dpi / 72.0, dy2 * dpi / 72.0
    err2 = max(abs(sx - px_c), abs(sy - py_c))
    ok_fwd = err2 < 2.5  # 像素级
    print(f"rot={rot}: 用户真值 → 公式像素 ({sx:.1f},{sy:.1f}) vs 实测 ({px_c:.1f},{py_c:.1f}) → 误差 {err2:.1f}px {'✓' if ok_fwd else '✗'}")
    doc.close()

print("\n交叉验证完成：四旋转 × 偏移 CropBox × 96DPI")
