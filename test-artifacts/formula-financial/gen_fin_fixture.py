#!/usr/bin/env python3
"""formula-financial 语料生成器（S3 任务 1 真值对照）。

产物：
  fin_raw.xlsx —— 手写原始件（数据 + 公式**无缓存值**）
  fin_lo.xlsx  —— soffice 重算件（LibreOffice 计算并写入缓存值 = 真值 oracle）

纪律（对齐 pdf-organize-corpus 先例）：生成器入库；产物小（<20KB）入库；
重建 = python3 gen_fin_fixture.py（需 soffice 在位）。

模型：16 行 × 7 列季度财务表（收入/成本/费用三类别明细 + SUM 小计 +
SUMIF 类别汇总 + IF 除零防护比率 + 环比），~50 个公式覆盖
SUM/SUMIF/IF/算术/百分比——真实财务表形态（#110 验收「含公式的财务表」）。
"""
import zipfile
import subprocess
import sys
from pathlib import Path

def esc(s: str) -> str:
    return s.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;').replace('"', '&quot;')

DATA = [
    ["类别", "项目", "Q1", "Q2", "Q3", "Q4", "全年"],
    ["收入", "产品", "120000", "135000", "142500", "158000", "SUM(C2:F2)"],
    ["收入", "服务", "45000", "48000", "51000", "56000", "SUM(C3:F3)"],
    ["成本", "产品", "72000", "81000", "85500", "94800", "SUM(C4:F4)"],
    ["成本", "服务", "27000", "28800", "30600", "33600", "SUM(C5:F5)"],
    ["费用", "部门A", "12000", "13000", "14000", "15000", "SUM(C6:F6)"],
    ["费用", "部门B", "8000", "8500", "9000", "9500", "SUM(C7:F7)"],
    ["合计", "收入合计", 'SUMIF($A$2:$A$7,"收入",C2:C7)', 'SUMIF($A$2:$A$7,"收入",D2:D7)',
     'SUMIF($A$2:$A$7,"收入",E2:E7)', 'SUMIF($A$2:$A$7,"收入",F2:F7)', "SUM(C8:F8)"],
    ["合计", "成本合计", 'SUMIF($A$2:$A$7,"成本",C2:C7)', 'SUMIF($A$2:$A$7,"成本",D2:D7)',
     'SUMIF($A$2:$A$7,"成本",E2:E7)', 'SUMIF($A$2:$A$7,"成本",F2:F7)', "SUM(C9:F9)"],
    ["合计", "费用合计", 'SUMIF($A$2:$A$7,"费用",C2:C7)', 'SUMIF($A$2:$A$7,"费用",D2:D7)',
     'SUMIF($A$2:$A$7,"费用",E2:E7)', 'SUMIF($A$2:$A$7,"费用",F2:F7)', "SUM(C10:F10)"],
    ["合计", "毛利", "C8-C9", "D8-D9", "E8-E9", "F8-F9", "SUM(C11:F11)"],
    ["合计", "毛利率", 'IF(C8=0,0,C11/C8)', 'IF(D8=0,0,D11/D8)', 'IF(E8=0,0,E11/E8)',
     'IF(F8=0,0,F11/F8)', 'IF(G8=0,0,G11/G8)'],
    ["合计", "净利润", "C11-C10", "D11-D10", "E11-E10", "F11-F10", "SUM(C13:F13)"],
    ["合计", "净利率", 'IF(C8=0,0,C13/C8)', 'IF(D8=0,0,D13/D8)', 'IF(E8=0,0,E13/E8)',
     'IF(F8=0,0,F13/F8)', 'IF(G8=0,0,G13/G8)'],
    ["合计", "费用率", 'IF(C8=0,0,C10/C8)', 'IF(D8=0,0,D10/D8)', 'IF(E8=0,0,E10/E8)',
     'IF(F8=0,0,F10/F8)', 'IF(G8=0,0,G10/G8)'],
    ["合计", "Q4收入环比", "", "", "", "", "F8/E8-1"],
]

def col_letter(i: int) -> str:
    s = ""
    i += 1
    while i > 0:
        i, rem = divmod(i - 1, 26)
        s = chr(65 + rem) + s
    return s

def build_sheet_xml() -> str:
    parts = ["<row>"]
    for r, row in enumerate(DATA, 1):
        for c, val in enumerate(row):
            if val == "":
                continue
            ref = f"{col_letter(c)}{r}"
            if r == 1 or c < 2:
                parts.append(f'<c r="{ref}" t="inlineStr"><is><t>{esc(val)}</t></is></c>')
            elif c <= 5 and val.replace(".", "").isdigit():
                parts.append(f'<c r="{ref}"><v>{val}</v></c>')
            else:
                parts.append(f'<c r="{ref}"><f>{esc(val)}</f></c>')
        parts.append("</row><row>")
    return ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
            '<sheetData>' + "".join(parts) + '</row></sheetData></worksheet>')

def write_raw(path: Path) -> None:
    ct = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
          '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
          '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
          '<Default Extension="xml" ContentType="application/xml"/>'
          '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
          '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
          '</Types>')
    rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
            '</Relationships>')
    wb = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
          '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
          'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
          '<sheets><sheet name="Fin" sheetId="1" r:id="rId1"/></sheets></workbook>')
    wbrels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
              '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
              '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
              '</Relationships>')
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", ct)
        z.writestr("_rels/.rels", rels)
        z.writestr("xl/workbook.xml", wb)
        z.writestr("xl/_rels/workbook.xml.rels", wbrels)
        z.writestr("xl/worksheets/sheet1.xml", build_sheet_xml())

def main() -> int:
    here = Path(__file__).parent
    raw = here / "fin_raw.xlsx"
    write_raw(raw)
    print(f"written {raw}")
    out = here / "lo_out"
    subprocess.run(
        ["soffice", "--headless", "--convert-to", "xlsx", "--outdir", str(out), str(raw)],
        check=True, capture_output=True, timeout=120,
    )
    lo = here / "fin_lo.xlsx"
    (out / "fin_raw.xlsx").rename(lo)
    print(f"written {lo} (LibreOffice 真值)")
    return 0

if __name__ == "__main__":
    sys.exit(main())
