#!/usr/bin/env python3
"""Generate web/tests/fixtures/sample.pdf — PDF 视图 e2e 夹具（阶段5 §八.4）。

最小合法 PDF 1.4（3 页，每页一段 Helvetica 24pt 文本），纯标准库字符串
模板生成，零 pip 依赖（mdc-pdf 无整档写面——lopdf 只做标注增量写，故
夹具走经典最小 PDF 手写路径：objects 1=Catalog 2=Pages 3-5=Page 6=Font
7-9=内容流 + xref 表逐字节精确偏移 + trailer）。

页文本约定（e2e 断言锚）：
  - 每页含 "Page N - mdcore pdf fixture"（搜索 "Page" ≥ 3 命中 /
    "Page 1" 选区锚 / 页码跟踪）；
  - 第二行追加差异文本（"alpha/beta/gamma line"）——搜索过滤与摘录面
    有区分度。

pdf_view.py 开头若缺夹具自动调用本脚本；直接运行亦可。
"""

import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent / "fixtures" / "sample.pdf"

PAGE_W, PAGE_H = 612, 792  # US Letter pt（PDF 用户空间）
PAGE_TEXTS = [
    ("Page 1 - mdcore pdf fixture", "alpha line"),
    ("Page 2 - mdcore pdf fixture", "beta line"),
    ("Page 3 - mdcore pdf fixture", "gamma line"),
]


def content_stream(title: str, extra: str) -> bytes:
    # 两条文本：24pt 标题（y=720）+ 12pt 次行（y=680）。
    # 文本内无 () 反斜杠——流内字符串免转义。
    return (
        f"BT /F1 24 Tf 72 720 Td ({title}) Tj ET\n"
        f"BT /F2 12 Tf 72 680 Td ({extra}) Tj ET\n"
    ).encode("ascii")


def build_pdf() -> bytes:
    out = bytearray()
    offsets: dict[int, int] = {}

    def obj(num: int, body: bytes) -> None:
        offsets[num] = len(out)
        out.extend(f"{num} 0 obj\n".encode("ascii"))
        out.extend(body)
        out.extend(b"\nendobj\n")

    # header（二进制标记注释行——规范建议，pdf.js/lopdf 均认可）
    out.extend(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")

    n_pages = len(PAGE_TEXTS)
    kids = " ".join(f"{3 + i} 0 R" for i in range(n_pages))
    obj(1, f"<< /Type /Catalog /Pages 2 0 R >>".encode("ascii"))
    obj(2, f"<< /Type /Pages /Kids [{kids}] /Count {n_pages} >>".encode("ascii"))
    for i, (title, extra) in enumerate(PAGE_TEXTS):
        obj(3 + i, (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] "
            f"/Resources << /Font << /F1 6 0 R /F2 7 0 R >> >> "
            f"/Contents {8 + i} 0 R >>"
        ).encode("ascii"))
    obj(6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    obj(7, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    for i, (title, extra) in enumerate(PAGE_TEXTS):
        stream = content_stream(title, extra)
        obj(8 + i, b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"endstream")

    # xref（0 哨兵 + 对象 1-10：1=Catalog 2=Pages 3-5=Page 6-7=Font 8-10=内容流）
    n_objects = 10
    xref_at = len(out)
    out.extend(f"xref\n0 {n_objects + 1}\n".encode("ascii"))
    out.extend(b"0000000000 65535 f \n")
    for num in range(1, n_objects + 1):
        out.extend(f"{offsets[num]:010d} 00000 n \n".encode("ascii"))
    out.extend(
        f"trailer << /Size {n_objects + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n".encode("ascii")
    )
    return bytes(out)


def verify(pdf: bytes) -> None:
    """自校验：header / 每对象偏移落点 / xref 表项与实际偏移一致。"""
    assert pdf.startswith(b"%PDF-1.4\n"), "header"
    xref_pos = int(pdf.rsplit(b"startxref\n", 1)[1].split(b"\n")[0])
    assert pdf[xref_pos:xref_pos + 4] == b"xref", "startxref -> xref"
    entries = pdf[xref_pos:].split(b"\n")[2:12]
    for num, entry in enumerate(entries[1:], start=1):
        off = int(entry.split(b" ")[0])
        assert pdf[off:off + len(f"{num} 0 obj")] == f"{num} 0 obj".encode(), \
            f"obj {num} offset {off}"
    assert pdf.rstrip().endswith(b"%%EOF"), "EOF"


def main() -> int:
    pdf = build_pdf()
    verify(pdf)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(pdf)
    print(f"ok -> {OUT} ({len(pdf)} bytes, {len(PAGE_TEXTS)} pages)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
