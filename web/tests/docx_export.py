#!/usr/bin/env python3
"""t18 DOCX export e2e (editor document -> office-io write face -> .docx
bytes download, closing the DOCX round-trip with t16): serve web/dist,
drive the real export button + programmatic window.__exportDocx, assert
zip validity ([Content_Types].xml / word/document.xml), string-level
document.xml mapping evidence (style_probe precedent), degraded-block
tolerance, empty-document minimal docx, editor state untouched by export
(read-only), and full circle export -> __importDocx -> content compare.

t20 extension (de-degradation XML faces): list numbering (numPr +
numbering.xml abstract + rels wiring), quote (w:ind + pBdr left), hr
(pBdr bottom), block code dual-channel monospace (rFonts Courier New);
status line simplified count is code_block only now (lang/fence remains
registered; blockquote/list_item/thematic_break fully expressed).

t22 extension: multiline list item continuation indent (single paragraph
+ w:br soft breaks + paragraph-level hanging w:ind = the numbering
level geometry -> continuation lines align with first-line text column;
multiline item text constructed via op face replace_text, markdown parse
is single-line only).
Evidence -> results/results_docx_export.json
"""

import base64
import http.server
import io
import json
import socketserver
import sys
import threading
import zipfile
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parent.parent
OUT = WEB / "tests" / "results" / "results_docx_export.json"

# 快乐路径文档：标题 + 段落 + 表（映射全覆盖的三块型——与 native
# roundtrip 测试同款面）。
EXPORT_MD = (
    "# Export Test\n"
    "plain paragraph text\n"
    "| alpha | beta |\n"
    "| --- | --- |\n"
    "| 1 | 2 |"
)

# 降级面文档：code block / blockquote / list（嵌套）/ divider——映射表中
# 无 docx 表达面的四块型（docx_export.rs 模块文档登记）。
DEGRADED_MD = (
    "```\n"
    "let x = 1;\n"
    "```\n"
    "> quoted line\n"
    "- list item\n"
    "  - nested\n"
    "---"
)

# 全环文档：只用非降级块型（heading/paragraph/table）——导出→t16 导入
# 在该子集上应逐字还原（降级块型不可全环——模块文档登记，本测试
# 显式取可表达子集）。
FULLCIRCLE_MD = (
    "# Round Trip\n"
    "plain text\n"
    "| a | b |\n"
    "| --- | --- |\n"
    "| 1 | 2 |"
)

# TR-15 行内 marks 文档（issue #15 A3/P0：TR-2D W4 复现面——星号
# 原样进 Word 的 P0）：标题 + 混合格式段 + 五位谱段 + 加粗列表项。
MARKS_MD = (
    "# 报价说明\n"
    "总计人民币 **壹佰贰拾万元整**，*非最终报价*。\n"
    "等宽 `f(x)`、删除 ~~旧条款~~、下划线 <u>新条款</u>。\n"
    "- **重点**：加粗列表项"
)

Handler = http.server.SimpleHTTPRequestHandler
Handler.extensions_map.update({".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"})


class QuietServer(Handler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(WEB / "dist"), **kw)

    def log_message(self, *a):
        pass


# 快速重跑不踩 TIME_WAIT（contract_gate 同款口径）
socketserver.TCPServer.allow_reuse_address = True

# 页内字节导入助手（base64 → Uint8Array → window.__importDocx）——t16 面
IMPORT_JS = """(b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return window.__importDocx(bytes);
}"""


def as_bytes(v) -> bytes:
    """page.evaluate 返回的 Uint8Array → python bytes/list 归一。"""
    if isinstance(v, bytes):
        return v
    if isinstance(v, list):
        return bytes(v)
    raise TypeError(f"unexpected bytes shape: {type(v)}")


def zip_part(zf: zipfile.ZipFile, name: str) -> str:
    with zf.open(name) as f:
        return f.read().decode("utf-8")


def main() -> int:
    checks = []
    page_errors: list[str] = []

    def check(name, ok, detail=""):
        checks.append({"name": name, "ok": bool(ok), "detail": str(detail)[:400]})
        print(("PASS " if ok else "FAIL ") + name + ("  | " + str(detail)[:160] if detail and not ok else ""))
        return ok

    with socketserver.TCPServer(("127.0.0.1", 8935), QuietServer) as srv:
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            page = browser.new_page()
            page.on("pageerror", lambda e: page_errors.append(str(e)))
            page.goto("http://127.0.0.1:8935/index.html")
            page.wait_for_function("window.__ready === true", timeout=30000)

            # ---- UI 面在场 + 契约（启动零回归：新模块不破坏 boot）----
            ui = page.evaluate("""() => ({
                btn: !!document.getElementById('docx-export-btn'),
                btnLabel: (document.getElementById('docx-export-btn') || {}).textContent,
                status: !!document.getElementById('docx-export-status'),
                exportFn: typeof window.__exportDocx,
                last: window.__lastDocxExport,
                contract: window.__contractVersion,
            })""")
            check("export_ui_present",
                  ui["btn"] and ui["btnLabel"] == "export .docx" and ui["status"]
                  and ui["exportFn"] == "function" and ui["last"] is None and ui["contract"] == 1.1,
                  json.dumps(ui))

            # ---- 快乐路径：真实按钮点击（UI 路径）+ 程序化钩子取字节 ----
            page.evaluate("(s) => window.__inject(s)", EXPORT_MD)
            page.wait_for_function("window.__blockCount() === 3", timeout=15000)
            page.click("#docx-export-btn")
            page.wait_for_function("window.__lastDocxExport && window.__lastDocxExport.ok === true",
                                   timeout=15000)
            r = page.evaluate("window.__exportDocx()")
            r_plain_keep = {"ok": r.get("ok"), "bytes": as_bytes(r["bytes"])}  # TR-4 检查4对照：默认态字节基线
            ok_zip = False
            part_names = []
            try:
                raw = as_bytes(r["bytes"])
                zf = zipfile.ZipFile(io.BytesIO(raw))
                part_names = zf.namelist()
                ok_zip = (
                    raw[:2] == b"PK"
                    and "[Content_Types].xml" in part_names
                    and "word/document.xml" in part_names
                    and zipfile.ZipFile(io.BytesIO(raw)).testzip() is None
                )
                if ok_zip:
                    # 后续检查复用同一 zip（doc_xml 一次性取出）
                    doc_xml = zip_part(zf, "word/document.xml")
                    page.evaluate("(s) => { window.__docXml = s; }", doc_xml)
            except Exception as e:  # noqa: BLE001
                ok_zip = False
                r = {"ok": r.get("ok"), "error": str(e)}
            status_text = page.evaluate("document.getElementById('docx-export-status').textContent")
            check("happy_export_valid_zip",
                  r.get("ok") is True and ok_zip and r["sizeBytes"] > 400
                  and status_text.startswith("exported:")
                  and "blocks" in status_text and "simplified" not in status_text,
                  json.dumps({"r": {k: r.get(k) for k in ("ok", "sizeBytes")},
                              "parts": part_names, "status": status_text}))

            # ---- document.xml 字符串级映射证据（style_probe 先例）----
            doc_xml = page.evaluate("window.__docXml")
            check("document_xml_mapping_strings",
                  "Export Test" in doc_xml and "plain paragraph text" in doc_xml
                  and "Heading1" in doc_xml and "<w:outlineLvl" in doc_xml,
                  doc_xml[:200] if "Export Test" not in doc_xml else "ok")

            # ---- 表导出（w:tbl + 单元格文本）----
            check("table_export_xml",
                  "<w:tbl>" in doc_xml and "alpha" in doc_xml and "beta" in doc_xml
                  and ">1<" in doc_xml and ">2<" in doc_xml,
                  "tbl marker + cells")

            # ---- 降级块不破导出 + 编辑器现状零触碰（导出只读）----
            # t20：quote/list/hr 已全表达（不再是 simplified 计数面）；
            # code_block 等宽已表达但 lang/fence 仍降级 → 1 simplified。
            page.evaluate("(s) => window.__inject(s)", DEGRADED_MD)
            page.wait_for_function("window.__blockCount() === 6", timeout=15000)
            md_before = page.evaluate("window.__md()")
            r2 = page.evaluate("window.__exportDocx()")
            raw2 = as_bytes(r2["bytes"])
            zf2 = zipfile.ZipFile(io.BytesIO(raw2))
            doc_xml2 = zip_part(zf2, "word/document.xml")
            degraded_status = page.evaluate("document.getElementById('docx-export-status').textContent")
            check("degraded_blocks_survive_export",
                  r2["ok"] is True and "let x = 1;" in doc_xml2
                  and "quoted line" in doc_xml2 and "list item" in doc_xml2 and "nested" in doc_xml2
                  and "1 simplified" in degraded_status
                  and page.evaluate("window.__md()") == md_before,
                  json.dumps({"status": degraded_status, "md_same": True}))

            # ---- t20 消降级 XML 面 ①：列表 numbering（numPr 配对 +
            #      numbering.xml bullet 抽象 + rels 接线——悬空 numPr
            #      陷阱的反证）----
            num_xml2 = zip_part(zf2, "word/numbering.xml")
            rels2 = zip_part(zf2, "word/_rels/document.xml.rels")
            check("t20_list_numbering_xml",
                  '<w:numId w:val="101" /><w:ilvl w:val="0" />' in doc_xml2
                  and '<w:numId w:val="101" /><w:ilvl w:val="1" />' in doc_xml2
                  and 'w:abstractNumId="101"' in num_xml2
                  and 'w:numFmt w:val="bullet"' in num_xml2
                  and "relationships/numbering" in rels2,
                  "bullet numPr ilvl 0/1 + 抽象 + rels")

            # ---- t20 消降级 XML 面 ②：引用（ind 左缩进 + pBdr 左边框）----
            check("t20_quote_indent_border_xml",
                  'w:ind w:left="720"' in doc_xml2
                  and '<w:pBdr><w:left w:val="single" w:space="4" w:sz="12" w:color="auto" /></w:pBdr>' in doc_xml2,
                  "quote ind 720 + left border")

            # ---- t20 消降级 XML 面 ③：水平线（空段下边框）----
            check("t20_hr_bottom_border_xml",
                  '<w:pBdr><w:bottom w:val="single" w:space="1" w:sz="6" w:color="auto" /></w:pBdr>' in doc_xml2,
                  "hr pBdr bottom")

            # ---- t20 消降级 XML 面 ④：块级 code 双通道等宽（段落级
            #      pPr>rPr>rFonts + run 级 rFonts，Courier New）----
            check("t20_code_block_mono_rfonts_xml",
                  '<w:pPr><w:rPr><w:rFonts w:ascii="Courier New"' in doc_xml2
                  and '<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" />' in doc_xml2,
                  "code dual-channel rFonts")

            # ---- t22 消降级 XML 面 ⑤：多行 item 续行缩进（单段 + w:br +
            #      段落级 hanging ind——与编号抽象层级同值悬挂几何，
            #      续行与首行文字列对齐）。多行 item 文本 = op 面构造
            #      （markdown 解析单行：注入单行 item 后 replace_text
            #      插 \n——op.rs 无换行校验，防御面登记）----
            page.evaluate("(s) => window.__inject(s)", "- item text")
            page.wait_for_function("window.__blockCount() === 1", timeout=15000)
            op_json = json.dumps([
                {"t": "replace_text", "path": [0], "start": 9, "end": 9,
                 "insert": "\nsecond line"}
            ])
            page.evaluate("(o) => window.__core.apply_ops(o, 'view', 't22-multiline')",
                          op_json)
            r5 = page.evaluate("window.__exportDocx()")
            raw5 = as_bytes(r5["bytes"])
            zf5 = zipfile.ZipFile(io.BytesIO(raw5))
            doc_xml5 = zip_part(zf5, "word/document.xml")
            check("t22_multiline_item_br_hanging_xml",
                  r5["ok"] is True
                  and page.evaluate("window.__blockCount()") == 1
                  and '<w:br w:type="textWrapping" />' in doc_xml5
                  and '<w:numPr><w:numId w:val="101" /><w:ilvl w:val="0" /></w:numPr>'
                      '<w:ind w:left="720" w:right="0" w:hanging="360" />' in doc_xml5
                  and doc_xml5.count("<w:numPr>") == 1
                  and "item text" in doc_xml5 and "second line" in doc_xml5,
                  "multiline item: single-paragraph numPr + br + hanging ind")

            # ---- TR-15（issue #15 A3/P0）：行内 marks 消费——markdown
            #      源文本层在导出面派生（TR-2D W4 复现：星号原样进
            #      Word 的 P0）。五位谱 XML 发射 + 零星号残留 + 标题
            #      定界符剥除 + 导入全环闭环。----
            page.evaluate("(s) => window.__inject(s)", MARKS_MD)
            page.wait_for_function("window.__blockCount() === 4", timeout=15000)
            r6 = page.evaluate("window.__exportDocx()")
            raw6 = as_bytes(r6["bytes"])
            zf6 = zipfile.ZipFile(io.BytesIO(raw6))
            doc_xml6 = zip_part(zf6, "word/document.xml")
            check("tr15_inline_marks_five_bit_emission",
                  r6["ok"] is True
                  and "<w:b />" in doc_xml6
                  and "<w:i />" in doc_xml6
                  and "<w:strike />" in doc_xml6
                  and '<w:u w:val="single" />' in doc_xml6
                  and 'w:ascii="Consolas"' in doc_xml6,
                  "bold/italic/strike/underline/mono 五位 XML 发射")
            check("tr15_zero_delimiter_residue",
                  "壹佰贰拾万元整" in doc_xml6 and "非最终报价" in doc_xml6
                  and "**" not in doc_xml6 and "~~" not in doc_xml6
                  and "<u>" not in doc_xml6 and "报价说明" in doc_xml6
                  and "加粗列表项" in doc_xml6,
                  "净文本零定界符残留（星号/波浪线/标签全剥；列表项"
                  " bold/普通两 run 切分——尾段子串断言）")
            # 全环闭环：导出 bold run → t16 导入 → md 重新含 `**`——
            # TR-2D 断链（导入产 `**`、导出不消费）修复钉定。
            b64_6 = base64.b64encode(raw6).decode()
            reimport6 = page.evaluate(IMPORT_JS, b64_6)
            md_rt6 = page.evaluate("window.__md()")
            check("tr15_full_circle_marks_roundtrip",
                  reimport6.get("ok") is True
                  and "**壹佰贰拾万元整**" in md_rt6
                  and "*非最终报价*" in md_rt6
                  and "**重点**：加粗列表项" in md_rt6,
                  json.dumps({"reimport_ok": reimport6.get("ok"),
                              "md": md_rt6[:200]}))
            # 未闭合定界符 → 字面量（不猜测——issue 验收要求 4）。
            page.evaluate("(s) => window.__inject(s)", "总计人民币 **壹佰贰拾万元整。")
            page.wait_for_function("window.__blockCount() === 1", timeout=15000)
            r7 = page.evaluate("window.__exportDocx()")
            doc_xml7 = zip_part(zipfile.ZipFile(io.BytesIO(as_bytes(r7["bytes"]))),
                                "word/document.xml")
            check("tr15_unclosed_marker_literal",
                  r7["ok"] is True and "**壹佰贰拾万元整。" in doc_xml7
                  and "<w:b />" not in doc_xml7,
                  "未闭合星号原样保留、零格式派生")

            # ---- 空文档 = 最小合法 docx ----
            page.evaluate("window.__inject('')")
            page.wait_for_function("window.__blockCount() === 0", timeout=15000)
            r3 = page.evaluate("window.__exportDocx()")
            raw3 = as_bytes(r3["bytes"])
            zf3 = zipfile.ZipFile(io.BytesIO(raw3))
            empty_status = page.evaluate("document.getElementById('docx-export-status').textContent")
            check("empty_document_minimal_docx",
                  r3["ok"] is True and "word/document.xml" in zf3.namelist()
                  and "0 blocks" in empty_status,
                  empty_status)

            # ---- 全环：export → __importDocx → 内容比对（非降级子集逐字）----
            page.evaluate("(s) => window.__inject(s)", FULLCIRCLE_MD)
            page.wait_for_function("window.__blockCount() === 3", timeout=15000)
            r4 = page.evaluate("window.__exportDocx()")
            raw4 = as_bytes(r4["bytes"])
            b64 = base64.b64encode(raw4).decode()
            reimport = page.evaluate(IMPORT_JS, b64)
            reimport_status = page.evaluate("document.getElementById('docx-status').textContent")
            md_roundtrip = page.evaluate("window.__md()")
            check("full_circle_reimport_content",
                  reimport.get("ok") is True and md_roundtrip == FULLCIRCLE_MD
                  and "imported:" in reimport_status,
                  json.dumps({"reimport": {k: reimport.get(k) for k in ("ok", "code")},
                              "md": md_roundtrip[:160], "status": reimport_status}))

            # ---- TR-4（issue #10）：导出格式选项——G2 格式面 web 消费 ----
            # 1) UI + 测试桥在场，默认态 = 引擎基态（options JSON = "{}"）
            tr4_ui = page.evaluate("""() => ({
                optBtn: !!document.getElementById('docx-export-options-btn'),
                panel: !!document.getElementById('docx-export-options'),
                presetSel: !!document.getElementById('deo-preset'),
                getFn: typeof window.__getDocxExportOptions,
                setFn: typeof window.__setDocxExportOptions,
                styledFace: typeof (window.__core || {}).export_docx_styled,
                cur: window.__getDocxExportOptions ? window.__getDocxExportOptions() : null,
            })""")
            check("tr4_options_ui_and_bridge_present",
                  tr4_ui["optBtn"] and tr4_ui["panel"] and tr4_ui["presetSel"]
                  and tr4_ui["getFn"] == "function" and tr4_ui["setFn"] == "function"
                  and tr4_ui["styledFace"] == "function"
                  and tr4_ui["cur"] and tr4_ui["cur"]["preset"] == "default"
                  and tr4_ui["cur"]["json"] == "{}",
                  json.dumps(tr4_ui))

            # 2) 公文 A4 预设：排版 + 几何全量发射（styles.xml docDefaults +
            #    document.xml sectPr 双部件断言——Rust 侧 E1 的 web 镜像）
            set_ok = page.evaluate("window.__setDocxExportOptions('official-a4')")
            r_preset = page.evaluate("window.__exportDocx()")
            st_preset = ""
            doc_preset = ""
            try:
                zf = zipfile.ZipFile(io.BytesIO(as_bytes(r_preset["bytes"])))
                st_preset = zip_part(zf, "word/styles.xml")
                doc_preset = zip_part(zf, "word/document.xml")
            except Exception as e:  # noqa: BLE001
                set_ok = f"zip error: {e}"
            check("tr4_official_a4_preset_emission",
                  set_ok is True
                  and 'w:val="28"' in st_preset and "宋体" in st_preset
                  and 'w:line="360"' in st_preset
                  and 'w:left="1800"' in doc_preset and 'w:right="1800"' in doc_preset,
                  json.dumps({"setOk": set_ok, "sz28": 'w:val="28"' in st_preset,
                              "songti": "宋体" in st_preset, "line360": 'w:line="360"' in st_preset,
                              "m1800": 'w:left="1800"' in doc_preset}))

            # 3) 自定义三选值：21 半磅 + 单倍 240 + 2cm 1134 边距
            page.evaluate("window.__setDocxExportOptions('custom', '21', '240', '1134')")
            r_custom = page.evaluate("window.__exportDocx()")
            st_custom = ""
            doc_custom = ""
            try:
                zf = zipfile.ZipFile(io.BytesIO(as_bytes(r_custom["bytes"])))
                st_custom = zip_part(zf, "word/styles.xml")
                doc_custom = zip_part(zf, "word/document.xml")
            except Exception:  # noqa: BLE001
                pass
            check("tr4_custom_values_emission",
                  'w:val="21"' in st_custom and 'w:line="240"' in st_custom
                  and 'w:left="1134"' in doc_custom,
                  json.dumps({"sz21": 'w:val="21"' in st_custom,
                              "line240": 'w:line="240"' in st_custom,
                              "m1134": 'w:left="1134"' in doc_custom}))

            # 4) 回默认态：语义等价（zip 容器 DOS 时间戳随导出时刻漂移
            #    [2 秒粒度]——字节级断言不可用；document.xml/styles.xml
            #    逐部件内容相等 = 映射面等价，且与预设产物部件可区分）。
            #    复位文档 = 基线保存时的 EXPORT_MD（全环检查已改注入内容）。
            page.evaluate("(s) => window.__inject(s)", EXPORT_MD)
            page.wait_for_function("window.__blockCount() === 3", timeout=15000)
            page.evaluate("window.__setDocxExportOptions('default')")
            r_default = page.evaluate("window.__exportDocx()")
            try:
                zf_d = zipfile.ZipFile(io.BytesIO(as_bytes(r_default["bytes"])))
                zf_k = zipfile.ZipFile(io.BytesIO(r_plain_keep["bytes"]))
                doc_eq = (zip_part(zf_d, "word/document.xml") == zip_part(zf_k, "word/document.xml")
                          and zip_part(zf_d, "word/styles.xml") == zip_part(zf_k, "word/styles.xml"))
                doc_differs_preset = (zip_part(zf_d, "word/styles.xml")
                                      != zip_part(zipfile.ZipFile(io.BytesIO(as_bytes(r_preset["bytes"]))),
                                                  "word/styles.xml"))
            except Exception as e:  # noqa: BLE001
                doc_eq = f"zip error: {e}"
                doc_differs_preset = False
            check("tr4_default_restores_plain_parts",
                  r_default.get("ok") is True and doc_eq is True and doc_differs_preset,
                  json.dumps({"partsEqualPlain": doc_eq,
                              "differsPreset": doc_differs_preset,
                              "defaultBytes": len(as_bytes(r_default["bytes"])),
                              "presetBytes": len(as_bytes(r_preset["bytes"]))}))

            # 5) 非法预设名拒绝：状态零触碰
            bad_set = page.evaluate("window.__setDocxExportOptions('nonexistent')")
            cur_after_bad = page.evaluate("window.__getDocxExportOptions()")
            check("tr4_invalid_preset_rejected",
                  bad_set is False and cur_after_bad["preset"] == "default"
                  and cur_after_bad["json"] == "{}",
                  json.dumps({"badSet": bad_set, "cur": cur_after_bad}))

            # ---- 契约面 + 零页错（启动/导出/导入全程无 JS 异常逃逸）----
            final = page.evaluate("""() => ({
                ready: window.__ready === true,
                contract: window.__contractVersion === 1.1,
                inject: typeof window.__inject === 'function',
                exportFn: typeof window.__exportDocx === 'function',
                importFn: typeof window.__importDocx === 'function',
            })""")
            check("contract_intact_no_pageerror",
                  final["ready"] and final["contract"] and final["inject"]
                  and final["exportFn"] and final["importFn"]
                  and len(page_errors) == 0,
                  json.dumps({"final": final, "pageErrors": page_errors[:3]}))

            browser.close()
        srv.shutdown()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    failed = [c for c in checks if not c["ok"]]
    OUT.write_text(json.dumps({
        "meta": {"gate": "t18 docx export e2e", "dist": str(WEB / "dist")},
        "tests": checks,
        "summary": {"total": len(checks), "failed": len(failed), "pass": not failed,
                    "failures": [c["name"] for c in failed]},
    }, ensure_ascii=False, indent=2))
    print(f"\n{len(checks) - len(failed)}/{len(checks)} checks passed -> {OUT}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
