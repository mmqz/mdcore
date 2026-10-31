#!/usr/bin/env python3
"""S2-4 OCR 扫描件夹具生成（真实扫描语义：图像页 + 零文本层）。

管线（真实扫描面模拟）：
  PIL 画布（NotoSansSC 真字体）渲染中英混排文本 → JPEG（扫描件有损
  压缩面）→ lopdf 嵌入 PDF（/Filter /DCTDecode——图像页零文本层，
  pdftotext 提取为空 = 「扫描件」的字面验证）。

产物：crates/pdf/fixtures/scanned_mixed.pdf（小体积随包入库）
"""
import io
import sys

from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

FIXTURES = Path(__file__).resolve().parent
FONT = "/usr/share/fonts/truetype/chinese/NotoSansSC-Regular.ttf"


def build() -> None:
    W, H = 1700, 2200  # 200 DPI Letter 等效
    img = Image.new("RGB", (W, H), "white")
    d = ImageDraw.Draw(img)
    title = ImageFont.truetype(FONT, 88)
    body = ImageFont.truetype(FONT, 56)
    lines = [
        ("招标文件技术规格书", title),
        ("Bid Document Technical Specification", body),
        ("", body),
        ("第一章 总体要求", body),
        ("Chapter 1 General Requirements", body),
        ("本项目为办公文档处理系统采购。", body),
        ("The project is a procurement of an office", body),
        ("document processing system with OCR support.", body),
        ("第二章 验收标准", body),
        ("Chapter 2 Acceptance Criteria", body),
        ("文字识别准确率不低于百分之九十五。", body),
        ("OCR accuracy shall not be lower than 95 percent.", body),
    ]
    y = 140
    for text, font in lines:
        if text:
            d.text((140, y), text, fill="black", font=font)
        y += font.size + 44
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=88)
    jpeg = buf.getvalue()

    # lopdf 嵌入（DCTDecode）。
    sys.path.insert(0, str(FIXTURES.parent / "src"))
    # 独立构造（不依赖被测 crate——纯夹具生成用 lopdf 直构）：
    # 用最小手写 xref 走 lopdf？此处直接用 pip lopdf 不可得——改用
    # 生成脚本内嵌最小 PDF 写出器（DCTDecode 单页——38 行）。
    out = build_pdf_with_jpeg(jpeg, W, H)
    (FIXTURES / "scanned_mixed.pdf").write_bytes(out)
    print(f"scanned_mixed.pdf: {len(out)} 字节（JPEG {len(jpeg)}）")


def build_pdf_with_jpeg(jpeg: bytes, w: int, h: int) -> bytes:
    """最小单页 PDF：/DCTDecode 图像 XObject + 全幅内容流（手写 xref——
    独立于被测写路径的夹具生成纪律）。"""
    # PDF 用户空间尺寸 = 像素/200DPI*72pt。
    pw, ph = w * 72.0 / 200.0, h * 72.0 / 200.0
    objs = {}
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".encode()
    objs[3] = (
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.0f} {ph:.0f}] "
        f"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
    ).encode()
    content = f"q {pw:.0f} 0 0 {ph:.0f} 0 0 cm /Im0 Do Q".encode()
    objs[4] = f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"\nendstream"
    objs[5] = (
        f"<< /Type /XObject /Subtype /Image /Width {w} /Height {h} "
        f"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode "
        f"/Length {len(jpeg)} >>\nstream\n"
    ).encode() + jpeg + b"\nendstream"

    out = io.BytesIO()
    out.write(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for num in sorted(objs):
        offsets[num] = out.tell()
        out.write(f"{num} 0 obj\n".encode() + objs[num] + b"\nendobj\n")
    xref_pos = out.tell()
    max_obj = max(objs)
    out.write(f"xref\n0 {max_obj + 1}\n".encode())
    out.write(b"0000000000 65535 f \n")
    for num in range(1, max_obj + 1):
        out.write(f"{offsets[num]:010d} 00000 n \n".encode())
    out.write(
        f"trailer\n<< /Size {max_obj + 1} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF".encode()
    )
    return out.getvalue()


if __name__ == "__main__":
    build()
