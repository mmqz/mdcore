#!/usr/bin/env python3
"""#141 任务 2（批注脚注编辑）黄金例 2 生成器。

产物 test-artifacts/F2/notes-golden.docx（仓内夹具——缺失即 FAIL）。
手构纪律 = tr19 先例（#18 同款 corpus_harness 手构 zip：引擎写面无
TextBox/脚注件生成面）。真实语料脚注面实测为分隔符空壳（零实体
脚注——扫描 27 件 docx：word-complex/word-merged/51921 均仅
separator/continuationSeparator），实体脚注验证以本黄金例 +
soffice 往返承载（C 组）；批注真实件 = word-complex（真 Word
comments.xml）+ D2a-nonbody（tr19 批注 701）。

覆盖矩阵：
  footnotes.xml   separator(-1) / continuationSeparator(0)  —— 分隔符（不入列）
                  id=1 单段脚注 / id=2 双段脚注          —— 实体（入列）
  comments.xml    id=701 示例作者 / id=702 评审           —— 批注（入列）
  document.xml    正文引用 w:footnoteReference{1,2}
                  批注锚 commentRangeStart/Reference{701,702}
"""
import zipfile
import pathlib
import sys

OUT = pathlib.Path(__file__).resolve().parent.parent / "test-artifacts" / "F2" / "notes-golden.docx"

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"

CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    '<Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/>'
    '<Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/>'
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

DOC_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>'
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/>'
    "</Relationships>"
)

CORE = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/">'
    "<dc:title>notes golden</dc:title>"
    "</cp:coreProperties>"
)

APP = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">'
    "<Application>make_notes_fixture.py</Application>"
    "</Properties>"
)

FOOTNOTES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    f'<w:footnotes xmlns:w="{W}">'
    # 分隔符惯例（-1/0）——不入列（与读面 FootnoteData 同口径）
    '<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>'
    '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>'
    # 实体脚注 id=1（单段，结果区含两个 w:t——多 span 形）
    '<w:footnote w:id="1"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>脚注一：机房</w:t></w:r><w:r><w:t>建设标准</w:t></w:r></w:p></w:footnote>'
    # 实体脚注 id=2（双段）
    '<w:footnote w:id="2">'
    '<w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>脚注二第一段</w:t></w:r></w:p>'
    '<w:p><w:r><w:t>脚注二第二段</w:t></w:r></w:p>'
    "</w:footnote>"
    "</w:footnotes>"
)

COMMENTS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    f'<w:comments xmlns:w="{W}">'
    '<w:comment w:id="701" w:author="示例作者" w:date="2026-11-01T09:00:00Z" w:initials="例">'
    '<w:p><w:r><w:annotationRef/></w:r><w:r><w:t>批注甲：口径差异待澄清</w:t></w:r></w:p>'
    "</w:comment>"
    '<w:comment w:id="702" w:author="评审专家" w:date="2026-11-01T10:00:00Z" w:initials="评">'
    '<w:p><w:r><w:annotationRef/></w:r><w:r><w:t>批注乙第一段</w:t></w:r></w:p>'
    '<w:p><w:r><w:t>批注乙第二段</w:t></w:r></w:p>'
    "</w:comment>"
    "</w:comments>"
)

DOCUMENT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    f'<w:document xmlns:w="{W}">'
    "<w:body>"
    # 段0：正文 + 脚注1 引用 + 批注701 锚
    '<w:p><w:r><w:t>正文第一段</w:t></w:r>'
    '<w:commentRangeStart w:id="701"/>'
    '<w:r><w:footnoteReference w:id="1"/></w:r>'
    '<w:r><w:t>。</w:t></w:r>'
    '<w:commentRangeEnd w:id="701"/>'
    '<w:r><w:commentReference w:id="701"/></w:r></w:p>'
    # 段1：正文 + 脚注2 引用（双段脚注）+ 批注702 锚
    '<w:p><w:r><w:t>正文第二段</w:t></w:r>'
    '<w:commentRangeStart w:id="702"/>'
    '<w:r><w:footnoteReference w:id="2"/></w:r>'
    '<w:r><w:t>，继续。</w:t></w:r>'
    '<w:commentRangeEnd w:id="702"/>'
    '<w:r><w:commentReference w:id="702"/></w:r></w:p>'
    # 段2：普通段（回归对照）
    "<w:p><w:r><w:t>普通段落</w:t></w:r></w:p>"
    '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>'
    "</w:body></w:document>"
)


def main() -> int:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CONTENT_TYPES)
        z.writestr("_rels/.rels", ROOT_RELS)
        z.writestr("docProps/core.xml", CORE)
        z.writestr("docProps/app.xml", APP)
        z.writestr("word/_rels/document.xml.rels", DOC_RELS)
        z.writestr("word/document.xml", DOCUMENT)
        z.writestr("word/footnotes.xml", FOOTNOTES)
        z.writestr("word/comments.xml", COMMENTS)
    print(f"written: {OUT} ({OUT.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
