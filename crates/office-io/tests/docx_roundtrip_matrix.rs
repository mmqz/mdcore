//! docx 编辑保真矩阵（#79 T2——#90 任务书；对照 xlsx FIDELITY v2 方法论）。
//!
//! # 定位
//!
//! 建立显式矩阵：「哪些面在外科手术编辑往返中保真、哪些拒改、哪些按
//! 配置」。单一真源 = `docs/docx-fidelity-matrix.md`（本文件为可执行
//! 锚点）。方法论对照 xlsx-r1/FIDELITY.md v2：**实测数字 + 条目级变化
//! 全解释 + 无未解释差异**，不根据引擎理论能力认定。
//!
//! # 夹具
//!
//! `build_matrix_fixture()`：单文件 13 面全覆盖（样式/行内格式/编号/
//! 表格合并边框/图片/页眉三变体+页脚/域/批注/脚注/修订/文本框/TOC/
//! 纯文本编辑目标），部件级真实形态（header1-3、footer1、comments、
//! footnotes、settings、rels 接线）。
//!
//! # 断言口径
//!
//! - ✅ 面 = 编辑目标段外**逐字节恒等**（XML slice 在产物中原样在场 +
//!   非目标部件 zip 逐字节恒等）；
//! - ⚠️ 拒改面（域代码/TOC）= typed 错 + 段 slice 逐字节恒等；
//! - ⚠️ 按配置面（修订）= Refuse 拒 + Edit 放行（del 原样）；
//! - LO 探针 = `#[ignore]` 显式跑（`soffice` + `pdftotext`——
//!   g1/g2/g3 同款纪律）；编辑后真实开档由 PR 证据链
//!   （scripts/t1_realdoc_verify.py，18/18）背书。

use std::io::Read as _;
use std::io::Write as _;

use mdc_office_io::docx_edit::{DocxEditor, RevisionPolicy};

// ---------------------------------------------------------------------------
// 夹具基建
// ---------------------------------------------------------------------------

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>\
<Override PartName=\"/word/header2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>\
<Override PartName=\"/word/header3.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>\
<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>\
<Override PartName=\"/word/comments.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/>\
<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/>\
<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>\
</Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

const DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId7\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.com/\" TargetMode=\"External\"/>\
<Relationship Id=\"rId8\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/>\
<Relationship Id=\"rId9\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header2.xml\"/>\
<Relationship Id=\"rId10\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header3.xml\"/>\
<Relationship Id=\"rId11\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>\
<Relationship Id=\"rId12\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image1.png\"/>\
<Relationship Id=\"rId13\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments\" Target=\"comments.xml\"/>\
<Relationship Id=\"rId14\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/>\
<Relationship Id=\"rId15\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>\
</Relationships>";

fn hdr(tag: &str, text: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:hdr xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:p><w:r><w:t>{text}{tag}</w:t></w:r></w:p></w:hdr>",
        tag = tag,
        text = text
    )
}

fn header1() -> String {
    hdr("", "默认页眉")
}
fn header2() -> String {
    hdr("", "首页页眉")
}
fn header3() -> String {
    hdr("", "偶数页眉")
}
fn footer1() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:ftr xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:p><w:r><w:t>页脚文本</w:t></w:r></w:p></w:ftr>"
        .to_string()
}

const COMMENTS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:comments xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:comment w:id=\"0\" w:author=\"矩阵作者\" w:date=\"2026-10-31T08:00:00Z\" w:initials=\"JZ\">\
<w:p><w:r><w:t>批注内容文本</w:t></w:r></w:p></w:comment></w:comments>";

const FOOTNOTES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:footnote w:id=\"1\"><w:p><w:r><w:t>脚注内容文本</w:t></w:r></w:p></w:footnote></w:footnotes>";

const SETTINGS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:evenAndOddHeaders/><w:titlePg/></w:settings>";

/// 13 面全覆盖正文（每面带唯一文本锚，便于 slice 定位与断言）。
fn matrix_body() -> String {
    let mut b = String::new();
    // 面 2：段落样式（Heading1）。
    b.push_str("<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/><w:jc w:val=\"center\"/></w:pPr><w:r><w:t>锚-标题段</w:t></w:r></w:p>");
    // 面 3：行内格式（粗/斜/下画线/色/字号）+ 自定义样式。
    b.push_str("<w:p><w:pPr><w:pStyle w:val=\"MyStyle\"/></w:pPr><w:r><w:rPr><w:b/><w:i/><w:u w:val=\"single\"/><w:color w:val=\"FF0000\"/><w:sz w:val=\"28\"/></w:rPr><w:t>锚-富格式段</w:t></w:r><w:r><w:t>续</w:t></w:r></w:p>");
    // 面 4：编号列表（有序 + 多级 ilvl）。
    b.push_str("<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"5\"/></w:numPr></w:pPr><w:r><w:t>锚-列表项</w:t></w:r></w:p>");
    b.push_str("<w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"5\"/></w:numPr></w:pPr><w:r><w:t>锚-子级列表项</w:t></w:r></w:p>");
    // 面 8：域代码（PAGE）。
    b.push_str("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>锚-页码域缓存3</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>");
    // 面 13：TOC 域。
    b.push_str("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> TOC \\o \"1-2\" </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>锚-目录缓存条目</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>");
    // 面 9：批注（锚 + 引用）。
    b.push_str("<w:p><w:commentRangeStart w:id=\"0\"/><w:r><w:t>锚-被批注文本</w:t></w:r><w:commentRangeEnd w:id=\"0\"/><w:r><w:commentReference w:id=\"0\"/></w:r></w:p>");
    // 面 10：脚注引用。
    b.push_str(
        "<w:p><w:r><w:t>锚-脚注前文</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>",
    );
    // 面 11：修订（ins + del）。
    b.push_str("<w:p><w:ins w:id=\"10\"><w:r><w:t>锚-插入文本</w:t></w:r></w:ins><w:del w:id=\"11\"><w:r><w:delText>锚-删除文本</w:delText></w:r></w:del></w:p>");
    // 面 6：图片（真实 1×1 PNG + 完整 wp:inline/blip 接线）。
    b.push_str("<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"9525\" cy=\"9525\"/><wp:docPr id=\"1\" name=\"img1\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"1\" name=\"img1\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rId12\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"9525\" cy=\"9525\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>");
    // 面 12：文本框（正规 wps:wsp 形态——DrawingML 文本框）。
    b.push_str("<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"1905000\" cy=\"190500\"/><wp:docPr id=\"2\" name=\"box1\"/><a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\"><wps:wsp><wps:cNvSpPr txBox=\"1\"/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1905000\" cy=\"190500\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></wps:spPr><wps:txbx><w:txbxContent><w:p><w:r><w:t>锚-文本框内文字</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>");
    // 面 5：表格（合并 gridSpan/vMerge + 边框）。
    b.push_str("<w:tbl><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\"/><w:left w:val=\"single\" w:sz=\"4\"/></w:tblBorders></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p><w:r><w:t>锚-合并单元格</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p><w:r><w:t>锚-纵合并</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>锚-普通格</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:t>锚-纵合并续</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>锚-右下格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>");
    // 编辑目标段（面 1：段落文字）+ 超链接面（坑⑤复验）。
    b.push_str(
        "<w:p><w:hyperlink r:id=\"rId7\"><w:r><w:t>锚-链接文字</w:t></w:r></w:hyperlink></w:p>",
    );
    b.push_str("<w:p><w:r><w:t>锚-编辑目标段旧文本</w:t></w:r></w:p>");
    b
}

/// 1×1 PNG（69B，python struct/zlib 生成自校——真实媒体件，非骨架）。
const PNG_1X1: [u8; 69] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn build_matrix_fixture() -> Vec<u8> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">\
<w:body>{}\
<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rId8\"/><w:headerReference w:type=\"first\" r:id=\"rId9\"/>\
<w:headerReference w:type=\"even\" r:id=\"rId10\"/><w:footerReference w:type=\"default\" r:id=\"rId11\"/>\
<w:titlePg/><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:body></w:document>",
        matrix_body()
    );
    let parts: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", CONTENT_TYPES.to_string()),
        ("_rels/.rels", ROOT_RELS.to_string()),
        ("word/_rels/document.xml.rels", DOC_RELS.to_string()),
        ("word/document.xml", document),
        ("word/header1.xml", header1()),
        ("word/header2.xml", header2()),
        ("word/header3.xml", header3()),
        ("word/footer1.xml", footer1()),
        ("word/comments.xml", COMMENTS.to_string()),
        ("word/footnotes.xml", FOOTNOTES.to_string()),
        ("word/settings.xml", SETTINGS.to_string()),
    ];
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        w.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.start_file(
        "word/media/image1.png",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    w.write_all(&PNG_1X1).unwrap();
    w.finish().unwrap().into_inner()
}

fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut e = ar.by_name(name).unwrap();
    let mut v = vec![];
    e.read_to_end(&mut v).unwrap();
    v
}

fn doc_xml(bytes: &[u8]) -> String {
    String::from_utf8(part_bytes(bytes, "word/document.xml")).unwrap()
}

/// 从 document.xml 提取含 `anchor` 文本的整个 <w:p …>…</w:p> slice。
fn para_slice(xml: &str, anchor: &str) -> String {
    let t = xml
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor {anchor} 不在 document.xml"));
    let s = xml[..t].rfind("<w:p").expect("段落起点");
    let e = xml[t..].find("</w:p>").expect("段落终点") + t + "</w:p>".len();
    xml[s..e].to_string()
}

fn idx_by_anchor(ed: &DocxEditor, anchor: &str) -> usize {
    ed.paragraphs()
        .iter()
        .enumerate()
        .find(|(i, _)| ed.paragraph_text(*i).contains(anchor))
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("段落 {anchor} 不在索引"))
}

// ---------------------------------------------------------------------------
// 矩阵主断言（面 1-13 单编辑全保真）
// ---------------------------------------------------------------------------

#[test]
fn matrix_thirteen_faces_single_edit_perfect() {
    let bytes = build_matrix_fixture();
    let orig = doc_xml(&bytes);

    let mut ed = DocxEditor::open(&bytes).unwrap();
    // 索引健全性：13 面夹具应产出 20 段（含表格 4 格段 + 文本框内层段）。
    let paras = ed.paragraphs().len();
    assert!(paras >= 18, "段落数下限（实测 {paras}）");
    // 旗标面：域段/修订段标记。
    let field_idx = idx_by_anchor(&ed, "锚-页码域缓存3");
    assert!(ed.paragraphs()[field_idx].field_code);
    let toc_idx = idx_by_anchor(&ed, "锚-目录缓存条目");
    assert!(
        ed.paragraphs()[toc_idx].field_code,
        "TOC 域段标记（域族拒改面）"
    );
    let rev_idx = idx_by_anchor(&ed, "锚-插入文本");
    assert!(ed.paragraphs()[rev_idx].revision);
    let tbl_idx = idx_by_anchor(&ed, "锚-合并单元格");
    assert!(ed.paragraphs()[tbl_idx].in_table);
    let box_idx = idx_by_anchor(&ed, "锚-文本框内文字");
    assert!(ed.paragraphs()[box_idx].in_textbox);
    // 文本约定：修订段读面 = ins 计入、delText 不计。
    assert_eq!(ed.paragraph_text(rev_idx), "锚-插入文本");

    // 面 1：编辑目标段（纯文本面）。
    let target = idx_by_anchor(&ed, "锚-编辑目标段旧文本");
    assert_eq!(ed.paragraph_text(target), "锚-编辑目标段旧文本");
    ed.set_paragraph_text(target, "矩阵验证的新文本  & <转义>")
        .unwrap();
    let saved = ed.save_to_vec().unwrap();
    let out = doc_xml(&saved);

    // —— ✅ 面逐字节恒等断言（编辑段外零接触）——
    // 面 2 段落样式（Heading1 段 + 自定义样式段整 slice）。
    assert!(
        out.contains(&para_slice(&orig, "锚-标题段")),
        "面2 样式段逐字节恒等"
    );
    assert!(
        out.contains(&para_slice(&orig, "锚-富格式段")),
        "面3 行内格式/自定义样式段恒等"
    );
    // 面 4 编号列表（多级）。
    assert!(
        out.contains(&para_slice(&orig, "锚-列表项")),
        "面4 编号列表恒等"
    );
    assert!(
        out.contains(&para_slice(&orig, "锚-子级列表项")),
        "面4 多级恒等"
    );
    // 面 5 表格（合并 + 边框）整 <w:tbl> 恒等。
    let tbl = {
        let s = orig.find("<w:tbl").unwrap();
        let e = orig.find("</w:tbl>").unwrap() + "</w:tbl>".len();
        orig[s..e].to_string()
    };
    assert!(out.contains(&tbl), "面5 表格（合并/边框）整表逐字节恒等");
    // 面 6：图片段（无文本锚）——首个 <w:drawing> 元素整 slice 恒等。
    let img_drawing = {
        let s = orig.find("<w:drawing").unwrap();
        let e = orig.find("</w:drawing>").unwrap() + "</w:drawing>".len();
        orig[s..e].to_string()
    };
    assert!(
        out.contains(&img_drawing),
        "面6 图片 drawing 元素恒等（位置/大小描述原样）"
    );
    // 面 12 文本框：外层承载段 + txbxContent 内层段整 slice 恒等。
    assert!(
        out.contains(&para_slice(&orig, "锚-文本框内文字")),
        "面12 文本框内层段恒等"
    );
    let txbx = {
        let s = orig.find("<w:txbxContent>").unwrap();
        let e = orig.find("</w:txbxContent>").unwrap() + "</w:txbxContent>".len();
        orig[s..e].to_string()
    };
    assert!(out.contains(&txbx), "面12 txbxContent 整元素恒等");
    // 面 7 页眉页脚（三变体 + 页脚）部件级恒等。
    for p in [
        "word/header1.xml",
        "word/header2.xml",
        "word/header3.xml",
        "word/footer1.xml",
    ] {
        assert_eq!(
            part_bytes(&saved, p),
            part_bytes(&bytes, p),
            "面7 {p} 逐字节恒等"
        );
    }
    // 面 9 批注：comments.xml 部件 + 正文锚/引用 slice。
    assert_eq!(
        part_bytes(&saved, "word/comments.xml"),
        part_bytes(&bytes, "word/comments.xml"),
        "面9 comments.xml 恒等"
    );
    assert!(
        out.contains(&para_slice(&orig, "锚-被批注文本")),
        "面9 批注锚段恒等（commentRangeStart/End/Reference 原样）"
    );
    // 面 10 脚注：footnotes.xml 部件 + 引用段 slice。
    assert_eq!(
        part_bytes(&saved, "word/footnotes.xml"),
        part_bytes(&bytes, "word/footnotes.xml"),
        "面10 footnotes.xml 恒等"
    );
    assert!(
        out.contains(&para_slice(&orig, "锚-脚注前文")),
        "面10 脚注引用段恒等（footnoteReference 原样）"
    );
    assert_eq!(
        part_bytes(&saved, "word/settings.xml"),
        part_bytes(&bytes, "word/settings.xml"),
        "settings.xml 恒等"
    );
    // 面 8 域代码段 + 面 13 TOC 段：slice 恒等（未触碰）。
    assert!(
        out.contains(&para_slice(&orig, "锚-页码域缓存3")),
        "面8 域代码段恒等"
    );
    assert!(
        out.contains(&para_slice(&orig, "锚-目录缓存条目")),
        "面13 TOC 段恒等（缓存条目原样=需手动更新）"
    );
    // 面 11 修订段：本策略下未触碰（编辑目标在别处）→ slice 恒等。
    assert!(
        out.contains(&para_slice(&orig, "锚-插入文本")),
        "面11 修订段恒等"
    );
    // 坑⑤复验：超链接段（未编辑）恒等。
    assert!(
        out.contains(&para_slice(&orig, "锚-链接文字")),
        "超链接段恒等"
    );
    assert_eq!(
        part_bytes(&saved, "word/_rels/document.xml.rels"),
        part_bytes(&bytes, "word/_rels/document.xml.rels"),
        "rels 逐字节恒等"
    );

    // —— 面 reopening（往返态）——
    let mut ed2 = DocxEditor::open(&saved).unwrap();
    assert_eq!(
        ed2.paragraph_text(target),
        "矩阵验证的新文本  & <转义>",
        "面1 编辑段落账+转义往返"
    );
    assert_eq!(
        ed2.paragraph_text(idx_by_anchor(&ed2, "锚-标题段")),
        "锚-标题段",
        "面2 未编辑段读回原文"
    );
    assert_eq!(
        ed2.paragraph_text(field_idx),
        "锚-页码域缓存3",
        "面8 域缓存文本读回（拒改不拒读）"
    );
    assert_eq!(
        ed2.paragraph_text(rev_idx),
        "锚-插入文本",
        "面11 ins 读回（delText 不计）"
    );

    // —— 面域/TOC/修订拒改（⚠️ 面）——
    let e1 = ed2.set_paragraph_text(field_idx, "x").unwrap_err();
    assert!(format!("{e1}").contains("field code"));
    let e2 = ed2.set_paragraph_text(toc_idx, "x").unwrap_err();
    assert!(
        format!("{e2}").contains("field code"),
        "TOC 域族拒改（需手动更新语义）"
    );
    let e3 = ed2.set_paragraph_text(rev_idx, "x").unwrap_err();
    assert!(
        format!("{e3}").contains("revision markup"),
        "修订 Refuse 拒改"
    );
}

// ---------------------------------------------------------------------------
// 面 11（修订）按配置 + 面 12（文本框）可编辑
// ---------------------------------------------------------------------------

#[test]
fn matrix_revision_policy_faces() {
    let bytes = build_matrix_fixture();
    // Refuse：拒改 + 拒改后零产物副作用。
    let mut ed = DocxEditor::open(&bytes).unwrap();
    let rev = idx_by_anchor(&ed, "锚-插入文本");
    assert!(ed.set_paragraph_text(rev, "x").is_err());
    let untouched = ed.save_to_vec().unwrap();
    assert_eq!(
        part_bytes(&untouched, "word/document.xml"),
        part_bytes(&bytes, "word/document.xml"),
        "零编辑落账 = document.xml 原样"
    );
    // Edit：ins 参与首文本位、del 原样保留。
    let mut ed2 = DocxEditor::open_with_policy(&bytes, RevisionPolicy::Edit).unwrap();
    ed2.set_paragraph_text(rev, "修订放行后的新文本").unwrap();
    let saved = ed2.save_to_vec().unwrap();
    let out = doc_xml(&saved);
    assert!(
        out.contains("<w:ins w:id=\"10\"><w:r><w:t xml:space=\"preserve\">修订放行后的新文本</w:t></w:r></w:ins>"),
        "Edit 策略：新文字落在 ins 内"
    );
    assert!(
        out.contains("<w:del w:id=\"11\"><w:r><w:delText>锚-删除文本</w:delText></w:r></w:del>"),
        "del 块（delText）原样保留"
    );
    let ed3 = DocxEditor::open(&saved).unwrap();
    assert_eq!(ed3.paragraph_text(rev), "修订放行后的新文本");
}

#[test]
fn matrix_textbox_editable_face() {
    let bytes = build_matrix_fixture();
    let mut ed = DocxEditor::open(&bytes).unwrap();
    let box_idx = idx_by_anchor(&ed, "锚-文本框内文字");
    assert!(ed.paragraphs()[box_idx].in_textbox);
    ed.set_paragraph_text(box_idx, "文本框内新文字").unwrap();
    let saved = ed.save_to_vec().unwrap();
    let out = doc_xml(&saved);
    assert!(
        out.contains("<w:txbxContent><w:p><w:r><w:t xml:space=\"preserve\">文本框内新文字</w:t></w:r></w:p></w:txbxContent>"),
        "面12：文本框内段编辑（drawing/txbx 壳原样）"
    );
    assert!(out.contains("<wps:txbx>"), "txbx 壳在场");
    let ed2 = DocxEditor::open(&saved).unwrap();
    assert_eq!(ed2.paragraph_text(box_idx), "文本框内新文字");
}

// ---------------------------------------------------------------------------
// LO 探针（显式跑：cargo test -p mdc-office-io --test docx_roundtrip_matrix --
//           --ignored --nocapture；需 soffice + pdftotext）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "矩阵探针：需 LO 环境显式跑"]
fn lo_probe_matrix_fixture_edited() {
    let bytes = build_matrix_fixture();
    let mut ed = DocxEditor::open(&bytes).unwrap();
    let target = idx_by_anchor(&ed, "锚-编辑目标段旧文本");
    ed.set_paragraph_text(target, "LOPROBE-新文本在场").unwrap();
    let saved = ed.save_to_vec().unwrap();
    let dir = std::env::temp_dir().join("t2_matrix_probe");
    std::fs::create_dir_all(&dir).unwrap();
    let docx = dir.join("edited.docx");
    std::fs::write(&docx, &saved).unwrap();
    let st = std::process::Command::new("soffice")
        .args([
            "--headless",
            "--convert-to",
            "pdf",
            "--outdir",
            dir.to_str().unwrap(),
            docx.to_str().unwrap(),
        ])
        .status()
        .expect("soffice 启动");
    assert!(st.success(), "LO 转换成功");
    let pdf = dir.join("edited.pdf");
    assert!(pdf.exists(), "PDF 产物在场");
    let out = std::process::Command::new("pdftotext")
        .arg(&pdf)
        .arg("-")
        .output()
        .expect("pdftotext 启动");
    let txt = String::from_utf8_lossy(&out.stdout);
    assert!(
        txt.contains("LOPROBE-新文本在场"),
        "编辑文本在 LO 渲染产物中"
    );
    // titlePg 首页语义：1 页档首页显示 first 变体页眉（默认/偶数变体
    // 仅后续页出现——三变体激活设置语义归消费方，探针只钉在场面）。
    assert!(txt.contains("首页页眉"), "first 页眉渲染在场");
    assert!(txt.contains("锚-标题段"), "标题样式段渲染在场");
    println!("LO probe ok -> {}", pdf.display());
}
