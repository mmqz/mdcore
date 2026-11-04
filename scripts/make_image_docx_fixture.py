#!/usr/bin/env python3
"""#204 带图 docx 黄金例生成器（mang-shell/tests/fixtures/smoke_image.docx）。

产物为仓内夹具（缺失即测试 FAIL——render_170_docx.rs 恒读不 SKIP）。
补 #170 图片正向对账缺口（issue #204）：tests/fixtures 两个既有 docx
夹具均无图（a:blip = 0 / word/media/ = 0 部件），「图片不丢」正向面
（props.images ↔ a:blip ↔ media 部件三方对账）无素材可测。

手构纪律 = make_fields/make_notes 先例（python 标准库 zipfile 手拼
OOXML 骨架）。图片 XML 形状 = docx-rs 写面发射形（w:drawing →
wp:inline → wp:extent → a:graphic → a:graphicData → pic:pic →
pic:blipFill → a:blip r:embed 全链——office-io 读面实测可读：
tests/docx_roundtrip.rs image_write_read_roundtrip 同构往返在档）。

覆盖矩阵（2 图，尺寸不同、分置两段）：
  段4  纯图片段     —— 单 run 单 drawing、零 w:t → 行模型空行 +
                       empty_paragraphs 计数（图片不入行流，计数入
                       props.images——docx_doc 读面实装语义）
  段6  文本+图混排段 —— 前文本 run + 图片 run → 文本保留、图只计数
  media           —— word/media/image1.png（96×64 三横带拓扑示意）、
                     word/media/image2.png（160×80 竖纹机房示意）；
                     一图一部件一引用（三方对账恒等：各部件恰一
                     a:blip、各引用恰一关系条目）
  其余            —— H1/H2 标题 + 正文段 + 2×2 表（smoke 家族混合语义：
                     图片读面不破坏邻接内容）

纯标准库（struct + zlib 手工打包最小合法 PNG——无 PIL 依赖，沙箱
无图库环境可跑）。
"""
import pathlib
import struct
import sys
import zipfile
import zlib

OUT = (
    pathlib.Path(__file__).resolve().parent.parent
    / "mang-shell"
    / "tests"
    / "fixtures"
    / "smoke_image.docx"
)

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
PIC = "http://schemas.openxmlformats.org/drawingml/2006/picture"
# 96dpi 像素 → EMU（docx-rs from_px 同值）。
EMU_PER_PX = 9525

CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Default Extension="png" ContentType="image/png"/>'
    '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>'
    '<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>'
    "</Types>"
)

ROOT_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>'
    '<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>'
    "</Relationships>"
)

# 正文关系表：恰两条 image 关系（一图一部件一引用——三方对账前提）。
DOC_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>'
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image2.png"/>'
    "</Relationships>"
)

CORE = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/">'
    "<dc:title>smoke image golden</dc:title>"
    "</cp:coreProperties>"
)

APP = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">'
    "<Application>make_image_docx_fixture.py</Application>"
    "</Properties>"
)


# ---------------------------------------------------------------------------
# 最小合法 PNG（zlib + struct 纯标准库；8-bit truecolor，逐行 filter 0）
# ---------------------------------------------------------------------------


def png_chunk(tag: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def make_png(width: int, height: int, rows: list[bytes]) -> bytes:
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    raw = b"".join(b"\x00" + row for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", zlib.compress(raw, 9))
        + png_chunk(b"IEND", b"")
    )


def image1_topology() -> bytes:
    """image1：96×64 三横带（接入/核心/出口三层——拓扑分层示意）。"""
    w, h = 96, 64
    third = h // 3
    rows = (
        [bytes((0x44, 0x72, 0xC4)) * w] * third  # 接入层（蓝）
        + [bytes((0x70, 0xAD, 0x47)) * w] * third  # 核心层（绿）
        + [bytes((0xED, 0x7D, 0x31)) * w] * (h - 2 * third)  # 出口层（橙）
    )
    return make_png(w, h, rows)


def image2_server_room() -> bytes:
    """image2：160×80 竖纹（机柜列——机房广角示意；与 image1 尺寸不同）。"""
    w, h = 160, 80
    stripe = bytes((0x5B, 0x9B, 0xD5)) * 8 + bytes((0xDE, 0xEB, 0xF7)) * 8
    row = (stripe * (w // 16 + 1))[: w * 3]
    return make_png(w, h, [row] * h)


# ---------------------------------------------------------------------------
# OOXML 片段（图片 XML = docx-rs 写面发射形——读面实测可读）
# ---------------------------------------------------------------------------


def drawing(rid: str, doc_pr_id: int, name: str, w_px: int, h_px: int) -> str:
    """内联图片 run 内容（w:drawing 全链；px → EMU 换算同 docx-rs）。"""
    cx, cy = w_px * EMU_PER_PX, h_px * EMU_PER_PX
    return (
        "<w:drawing>"
        '<wp:inline distT="0" distB="0" distL="0" distR="0">'
        f'<wp:extent cx="{cx}" cy="{cy}"/>'
        '<wp:effectExtent l="0" t="0" r="0" b="0"/>'
        f'<wp:docPr id="{doc_pr_id}" name="{name}"/>'
        "<wp:cNvGraphicFramePr>"
        f'<a:graphicFrameLocks xmlns:a="{A}" noChangeAspect="1"/>'
        "</wp:cNvGraphicFramePr>"
        f'<a:graphic xmlns:a="{A}">'
        f'<a:graphicData uri="{PIC}">'
        f'<pic:pic xmlns:pic="{PIC}">'
        "<pic:nvPicPr>"
        f'<pic:cNvPr id="0" name="{name}"/>'
        "<pic:cNvPicPr/>"
        "</pic:nvPicPr>"
        "<pic:blipFill>"
        f'<a:blip r:embed="{rid}"/>'
        "<a:stretch><a:fillRect/></a:stretch>"
        "</pic:blipFill>"
        "<pic:spPr>"
        f'<a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>'
        "</pic:spPr>"
        "</pic:pic>"
        "</a:graphicData>"
        "</a:graphic>"
        "</wp:inline>"
        "</w:drawing>"
    )


def heading(level: int, text: str) -> str:
    return (
        "<w:p><w:pPr>"
        f'<w:pStyle w:val="Heading{level}"/>'
        f'<w:outlineLvl w:val="{level - 1}"/>'
        "</w:pPr>"
        f"<w:r><w:t>{text}</w:t></w:r></w:p>"
    )


def para(text: str) -> str:
    return f"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"


def image_para(rid: str, doc_pr_id: int, name: str, w_px: int, h_px: int) -> str:
    """纯图片段（单 run 单 drawing、零 w:t——行模型空行承载）。"""
    return (
        "<w:p><w:r>"
        + drawing(rid, doc_pr_id, name, w_px, h_px)
        + "</w:r></w:p>"
    )


def cell(text: str) -> str:
    return f"<w:tc><w:tcPr/><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"


DOCUMENT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    f'<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:wp="{WP}">'
    "<w:body>"
    + heading(1, "智慧园区综合管理平台图示样张")
    + para("本样张验证 docx 图片读面（#204）：两张内联图片，尺寸不同、分置两段。")
    + heading(2, "一、网络拓扑")
    + para("下图为平台网络拓扑示意（纯图片段——图片不入正文行流）。")
    + image_para("rId1", 1, "拓扑示意图", 96, 64)
    + heading(2, "二、机房实拍")
    + "<w:p><w:r><w:t>机房实拍（广角）：</w:t></w:r><w:r>"
    + drawing("rId2", 2, "机房实拍图", 160, 80)
    + "</w:r></w:p>"
    + heading(2, "三、报价明细")
    + "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid/>"
    + "<w:tr>"
    + cell("设备")
    + cell("数量")
    + "</w:tr>"
    + "<w:tr>"
    + cell("机柜")
    + cell("42")
    + "</w:tr>"
    + "</w:tbl>"
    + para("全文完——图片清单见属性面板（props.images = 2）。")
    + '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>'
    "</w:body></w:document>"
)


def main() -> int:
    img1 = image1_topology()
    img2 = image2_server_room()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CONTENT_TYPES)
        z.writestr("_rels/.rels", ROOT_RELS)
        z.writestr("docProps/core.xml", CORE)
        z.writestr("docProps/app.xml", APP)
        z.writestr("word/_rels/document.xml.rels", DOC_RELS)
        z.writestr("word/document.xml", DOCUMENT)
        z.writestr("word/media/image1.png", img1)
        z.writestr("word/media/image2.png", img2)
    print(
        f"written: {OUT} ({OUT.stat().st_size} bytes; "
        f"image1.png {len(img1)} B, image2.png {len(img2)} B)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
