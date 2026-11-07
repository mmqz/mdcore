# -*- coding: utf-8 -*-
"""#304：旋转 + 偏移 CropBox 夹具生成器。

一页 PDF：MediaBox [0,0,612,792] / CropBox [50,40,562,752]（⊂ MediaBox
非零偏移）/ /Rotate 90。内容流画一个红方块（用户空间 [100,132,160,192]，
中心 (130,162)）——page_transform 引擎测试的落点锚（公式用户→像素↔
真实渲染互证见 scripts/verify_304_transform.py 的同款构造）。
"""
import os

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "rotated_crop.pdf")

# 手写最小 PDF（零依赖、字节确定——xref 偏移精确计算）。
def pdf_string(n: int) -> str:
    return f"{n} 0 obj\n"

objs = {}

# 1：Catalog
objs[1] = "<< /Type /Catalog /Pages 2 0 R >>"
# 2：Pages
objs[2] = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
# 3：Page（MediaBox + 偏移 CropBox + /Rotate 90 + 内容引用）
objs[3] = (
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    "/CropBox [50 40 562 752] /Rotate 90 /Resources << >> /Contents 4 0 R >>"
)
# 4：内容流（红方块 @ 用户空间 [100,132,160,192]）
content = b"q 1 0 0 RG 1 0 0 rg 100 132 60 60 re f Q"
objs[4] = f"<< /Length {len(content)} >>\nstream\n__CONTENT__\nendstream"

out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
offsets = {}
for n in sorted(objs):
    offsets[n] = len(out)
    body = objs[n].replace("__CONTENT__", "")
    out += pdf_string(n).encode() + body.encode() + b"\nendobj\n"
# 内容字节原位嵌入（4 号对象 body 里的占位长度一致——流长度已按内容计）。
# 重写：直接以字节拼接构造（上面的 __CONTENT__ 技巧对多行不严谨，改为显式拼）。
out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
offsets = {}
for n in sorted(objs):
    offsets[n] = len(out)
    if n == 4:
        head = b"4 0 obj\n<< /Length " + str(len(content)).encode() + b" >>\nstream\n"
        out += head + content + b"\nendstream\nendobj\n"
    else:
        out += pdf_string(n).encode() + objs[n].encode() + b"\nendobj\n"

xref_pos = len(out)
n_objs = max(objs) + 1  # +1：freet 0 条目
out += b"xref\n0 " + str(n_objs).encode() + b"\n"
out += b"0000000000 65535 f \n"
for n in range(1, n_objs):
    out += f"{offsets[n]:010d} 00000 n \n".encode()
out += (
    b"trailer\n<< /Size " + str(n_objs).encode()
    + b" /Root 1 0 R >>\nstartxref\n" + str(xref_pos).encode() + b"\n%%EOF\n"
)

with open(OUT, "wb") as f:
    f.write(out)
print("wrote", OUT, len(out), "bytes")
