#!/usr/bin/env python3
"""#140 域码段编辑黄金例生成器（test-artifacts/F1/fields-golden.docx）。

产物为仓内夹具（缺失即测试 FAIL——tests/docx_fields.rs 恒读不 SKIP）。
覆盖矩阵（fldChar 五段 / 多 span 结果区 / TOC 跨段 / 嵌套 PAGEREF /
fldSimple 简单形 / 无 separate 退化域）：

  段0  PAGE 域            —— begin/instr/separate/"3"/end 单 span
  段1  DATE 域            —— 结果区跨 3 个 w:t run（"2020"+"-06"+"-30"）
  段2  TOC begin          —— begin/instr/separate（TOC 域开放，跨段）
  段3  TOC 条目 + PAGEREF —— 字面"第一章 概述 " + 嵌套 PAGEREF 域（"1"）
  段4  TOC end            —— 字面"第二章 方案 " + end
  段5  普通段             —— 无域（回归对照）
  段6  fldSimple TIME     —— w:instr 属性形 + 子 w:t"09:30" + 字面尾
  段7  退化 REF 域        —— begin/instr/end 无 separate（WPS 形）
  段8  普通段             —— 无域（回归对照）

soffice 验证（tests/docx_fields.rs C 组）：本夹具域活证伪——PAGE 显示值
改写不可能值后 LibreOffice 重算域（PAGE→当前页码），死文本则照显改写值。
"""
import zipfile
import pathlib
import sys

OUT = pathlib.Path(__file__).resolve().parent.parent / "test-artifacts" / "F1" / "fields-golden.docx"

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"

CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
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

CORE = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/">'
    '<dc:title>fields golden</dc:title>'
    "</cp:coreProperties>"
)

APP = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">'
    "<Application>make_fields_fixture.py</Application>"
    "</Properties>"
)

FB = '<w:r><w:fldChar w:fldCharType="begin"/></w:r>'
FS = '<w:r><w:fldChar w:fldCharType="separate"/></w:r>'
FE = '<w:r><w:fldChar w:fldCharType="end"/></w:r>'


def instr(s: str) -> str:
    return f'<w:r><w:instrText xml:space="preserve">{s}</w:instrText></w:r>'


def wt(s: str) -> str:
    return f"<w:r><w:t>{s}</w:t></w:r>"


PARAS = [
    # 段0 PAGE
    "<w:p>" + FB + instr(" PAGE \\* MERGEFORMAT ") + FS + wt("3") + FE + "</w:p>",
    # 段1 DATE（多 span）
    "<w:p>" + FB + instr(' DATE \\@ "yyyy-MM-dd" ') + FS + wt("2020") + wt("-06") + wt("-30") + FE + "</w:p>",
    # 段2 TOC 开
    "<w:p>" + FB + instr(' TOC \\o "1-3" \\h \\z \\u ') + FS + "</w:p>",
    # 段3 TOC 条目 + 嵌套 PAGEREF
    "<w:p>"
    + wt("第一章 概述 ")
    + FB
    + instr(" PAGEREF _Toc1 \\h ")
    + FS
    + wt("1")
    + FE
    + "</w:p>",
    # 段4 TOC 闭
    "<w:p>" + wt("第二章 方案 ") + FE + "</w:p>",
    # 段5 普通
    "<w:p>" + wt("普通段落甲") + "</w:p>",
    # 段6 fldSimple TIME + 字面尾
    '<w:p><w:fldSimple w:instr=" TIME \\* MERGEFORMAT ">'
    + wt("09:30")
    + '</w:fldSimple>'
    + wt("（更新于）")
    + "</w:p>",
    # 段7 退化 REF（无 separate）
    "<w:p>" + wt("字面前 ") + FB + instr(" REF _Ref123456789 \\h ") + FE + wt(" 字面后") + "</w:p>",
    # 段8 普通
    "<w:p>" + wt("普通段落乙") + "</w:p>",
]

DOCUMENT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    f'<w:document xmlns:w="{W}">'
    '<w:body>'
    + "".join(PARAS)
    + '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>'
    "</w:body></w:document>"
)


def main() -> int:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CONTENT_TYPES)
        z.writestr("_rels/.rels", ROOT_RELS)
        z.writestr("docProps/core.xml", CORE)
        z.writestr("docProps/app.xml", APP)
        z.writestr("word/document.xml", DOCUMENT)
    print(f"written: {OUT} ({OUT.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
