//! XLSX 结构面读取——隐藏行列 / 外部链接 / 样式继承链（#265）。
//!
//! 动机：calamine 只提取**值**（worksheet_range）；隐藏行列
//!（`<row hidden>`/`<col hidden>`）、外部链接（externalLinks 部件）、
//! styles.xml 的 xf 继承链（cellXfs→cellStyleXfs→numFmts）三个结构
//! 面在读取能力上为零（#260 盘点：三类语料 0-1 件 = 等于没测）。
//!
//! 设计：
//! - **独立 zip 解析**（不嵌 calamine、不动 [`crate::io`] 主流程——
//!   零回归面）；字节入口 [`read_structural_from_bytes`]。
//! - 解析面全部**容错降级**：单部件坏 → 该面空 + 继续（结构面是
//!   增强信息，不是门——值面门在 [`crate::io`] 承担）。
//! - 列隐藏**范围展开**（`<col min="3" max="5" hidden>` → C/D/E）。
//! - xf 继承：`cellXfs[i]` 的 `applyNumberFormat=0` → 沿 `xfId` 落
//!   `cellStyleXfs`（ECMA-376 §18.8.10 语义）；numFmtId 落 builtin
//!   表（§18.8.30）或自定义 numFmts。
//!
//! 测试面 = `tests/xlsx_structural_corpus.rs`（真实语料四类断言）。

use crate::io::ReadError;
use quick_xml::events::Event;
use quick_xml::Reader;

/// 每表结构（隐藏行列）。
#[derive(Debug, Clone, PartialEq)]
pub struct SheetStructure {
    /// sheet 名（workbook.xml `<sheet name>`）。
    pub name: String,
    /// 部件路径（xl/worksheets/sheet1.xml）。
    pub part: String,
    /// 隐藏行号（升序去重）。
    pub hidden_rows: Vec<u32>,
    /// 隐藏列字母（范围展开，升序去重——A/B/…/Z/AA/…）。
    pub hidden_cols: Vec<String>,
}

/// 外部链接（workbook 级——externalLinks 关系目标）。
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalLink {
    /// 关系 id（rIdN）。
    pub r_id: String,
    /// 外部目标（相对路径 / UNC / URL——原样保留）。
    pub target: String,
}

/// cellXfs 条目（numFmt 面——#265 断言面）。
#[derive(Debug, Clone, PartialEq)]
pub struct XfDef {
    /// numFmtId（builtin 或自定义 id）。
    pub num_fmt_id: u32,
    /// applyNumberFormat 位（false → 沿 xfId 继承 cellStyleXfs）。
    pub apply_number_format: bool,
    /// cellStyleXfs 索引（xfId——继承链父）。
    pub xf_id: Option<u32>,
}

/// 样式表（styles.xml 的 numFmt/xf 面）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleTable {
    /// 自定义 numFmt（id, formatCode——id ≥ 164 惯例但按实际解析）。
    pub custom_fmts: Vec<(u32, String)>,
    /// cellXfs（单元格样式索引——sheet `<c s="i">` 指此处）。
    pub cell_xfs: Vec<XfDef>,
    /// cellStyleXfs（命名样式层——xfId 继承父）。
    pub cell_style_xfs: Vec<XfDef>,
}

impl StyleTable {
    /// 沿 xf 继承链解析最终生效的数字格式串。
    ///
    /// 语义（ECMA-376 §18.8.10/§18.8.30）：`cellXfs[i]` 的
    /// `applyNumberFormat=1` → 用自身 numFmtId；否则沿 `xfId` 落
    /// `cellStyleXfs` 同号字段；两侧都无 → General（id 0）。
    /// numFmtId 落 builtin 表或 [`StyleTable::custom_fmts`]。
    pub fn effective_num_fmt(&self, xf_index: usize) -> Option<String> {
        let xf = self.cell_xfs.get(xf_index)?;
        let resolved: &XfDef = if xf.apply_number_format {
            xf
        } else {
            // 继承：xfId → cellStyleXfs；缺父 → 自身 numFmtId 兜底
            //（缺引用降级——#265「缺失指向优雅降级」）。
            xf.xf_id
                .and_then(|i| self.cell_style_xfs.get(i as usize))
                .unwrap_or(xf)
        };
        let id = resolved.num_fmt_id;
        if let Some((_, code)) = self.custom_fmts.iter().find(|(cid, _)| *cid == id) {
            return Some(code.clone());
        }
        builtin_num_fmt(id).map(str::to_string)
    }
}

/// 结构面总报告。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructuralReport {
    pub sheets: Vec<SheetStructure>,
    pub external_links: Vec<ExternalLink>,
    pub styles: StyleTable,
}

impl StructuralReport {
    /// 按表名取结构。
    pub fn sheet(&self, name: &str) -> Option<&SheetStructure> {
        self.sheets.iter().find(|s| s.name == name)
    }
}

/// ECMA-376 §18.8.30 builtin numFmt 表（断言面只覆盖常用段——
/// 完整显示语义引擎是 #161 显示面任务，此处只做格式串承载）。
pub fn builtin_num_fmt(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// 列号（1 基）→ 字母（1→A … 27→AA）。
fn col_letter(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn attr<'a>(e: &'a quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| {
            // XML 实体解码（&quot;/&amp; 等——formatCode 常含引号实体）。
            let raw = String::from_utf8_lossy(&a.value).into_owned();
            match quick_xml::escape::unescape(&raw) {
                Ok(s) => s.into_owned(),
                Err(_) => raw,
            }
        })
}

/// 读 xlsx 结构面（字节入口）。容器坏 → typed 错误；部件面容错。
pub fn read_structural_from_bytes(bytes: &[u8]) -> Result<StructuralReport, ReadError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut za =
        zip::ZipArchive::new(cursor).map_err(|e| ReadError::ContainerCorrupt(format!("{e}")))?;
    let mut report = StructuralReport::default();

    // ① workbook.xml：sheet 名 + r:id（部件映射在 rels）。
    let mut sheet_rids: Vec<(String, String)> = Vec::new(); // (name, rId)
    if let Some(xml) = read_part(&mut za, "xl/workbook.xml") {
        sheet_rids = parse_workbook_sheets(&xml);
    }
    // ② workbook rels：rId → Target（worksheet 部件；外部链接在 ④
    //    按同一 rels 的 TargetMode="External" 直取）。
    let mut worksheet_target: Vec<(String, String)> = Vec::new(); // (rId, part)
    if let Some(xml) = read_part(&mut za, "xl/_rels/workbook.xml.rels") {
        worksheet_target = parse_workbook_rels(&xml);
    }
    // ③ 逐 worksheet 部件：隐藏行列（名字按 rels 映射回 sheet 名）。
    let part_to_name: Vec<(String, String)> = worksheet_target
        .iter()
        .filter_map(|(rid, part)| {
            let name = sheet_rids
                .iter()
                .find(|(_, r)| r == rid)
                .map(|(n, _)| n.clone())?;
            Some((part.clone(), name))
        })
        .collect();
    for (part, name) in &part_to_name {
        let (hr, hc) = read_part(&mut za, part)
            .map(|xml| parse_hidden(&xml))
            .unwrap_or_default();
        report.sheets.push(SheetStructure {
            name: name.clone(),
            part: part.clone(),
            hidden_rows: hr,
            hidden_cols: hc,
        });
    }
    // ④ 外部链接（ECMA-376 §15.2 两级形态）：workbook rels 的
    //    externalLink 关系 → 部件（xl/externalLinks/externalLinkN.xml，
    //    Target 无 Mode = Internal）；真正的外部目标在**部件自己的
    //    rels**（xl/externalLinks/_rels/externalLinkN.xml.rels 的
    //    TargetMode="External" Target=file:///…/http://…）。
    let mut ext_parts: Vec<String> = Vec::new();
    if let Some(xml) = read_part(&mut za, "xl/_rels/workbook.xml.rels") {
        ext_parts = parse_external_link_parts(&xml);
    }
    for part in &ext_parts {
        // part = xl/externalLinks/externalLink1.xml → rels 路径换算。
        let base = part.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let file = part.rsplit_once('/').map(|(_, f)| f).unwrap_or("");
        let rels_path = format!("{base}/_rels/{file}.rels");
        if let Some(xml) = read_part(&mut za, &rels_path) {
            for link in parse_external_targets(&xml) {
                report.external_links.push(link);
            }
        }
    }
    // ⑤ styles.xml：numFmts + cellXfs + cellStyleXfs。
    if let Some(xml) = read_part(&mut za, "xl/styles.xml") {
        report.styles = parse_styles(&xml);
    }
    Ok(report)
}

fn read_part(za: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = za.by_name(name).ok()?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).ok()?;
    Some(s)
}

/// workbook.xml `<sheet name="X" r:id="rIdN"/>`。
fn parse_workbook_sheets(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"sheet" {
                    let name = attr(&e, b"name").unwrap_or_default();
                    // r:id 命名空间前缀不定（r:/ns0:）——按局部名匹配。
                    let rid = e
                        .attributes()
                        .flatten()
                        .find(|a| {
                            a.key
                                .as_ref()
                                .rsplit(|c| *c == b':')
                                .next()
                                .map(|k| k == b"id")
                                .unwrap_or(false)
                        })
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                        .unwrap_or_default();
                    if !name.is_empty() {
                        out.push((name, rid));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    out
}

/// workbook rels：worksheet 关系（rId → Target 部件路径）。
fn parse_workbook_rels(xml: &str) -> Vec<(String, String)> {
    let mut ws = Vec::new();
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"Relationship" {
                    let id = attr(&e, b"Id").unwrap_or_default();
                    let target = attr(&e, b"Target").unwrap_or_default();
                    let rtype = attr(&e, b"Type").unwrap_or_default();
                    if rtype.ends_with("/worksheet") {
                        // Target 相对 xl/（worksheets/sheet1.xml）。
                        let part = if target.starts_with('/') {
                            target.trim_start_matches('/').to_string()
                        } else {
                            format!("xl/{target}")
                        };
                        ws.push((id, part));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    ws
}

/// workbook rels：externalLink 关系的部件路径（xl/externalLinks/*.xml）。
fn parse_external_link_parts(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"Relationship" {
                    let rtype = attr(&e, b"Type").unwrap_or_default();
                    if rtype.ends_with("/externalLink") {
                        let target = attr(&e, b"Target").unwrap_or_default();
                        let part = if target.starts_with('/') {
                            target.trim_start_matches('/').to_string()
                        } else {
                            format!("xl/{target}")
                        };
                        out.push(part);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    out
}

/// rels 的外部目标（TargetMode="External"——externalLinkPath 关系）。
fn parse_external_targets(xml: &str) -> Vec<ExternalLink> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"Relationship" {
                    let mode = attr(&e, b"TargetMode").unwrap_or_default();
                    if mode == "External" {
                        let id = attr(&e, b"Id").unwrap_or_default();
                        let target = attr(&e, b"Target").unwrap_or_default();
                        if !target.is_empty() {
                            out.push(ExternalLink { r_id: id, target });
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    out
}

/// worksheet XML：隐藏行（row hidden）+ 隐藏列（col hidden，范围展开）。
fn parse_hidden(xml: &str) -> (Vec<u32>, Vec<String>) {
    let mut rows = Vec::new();
    let mut cols = Vec::new();
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let hidden = attr(&e, b"hidden")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
                if hidden {
                    match e.name().as_ref() {
                        b"row" => {
                            if let Some(rn) = attr(&e, b"r").and_then(|v| v.parse().ok()) {
                                rows.push(rn);
                            }
                        }
                        b"col" => {
                            let min: u32 =
                                attr(&e, b"min").and_then(|v| v.parse().ok()).unwrap_or(1);
                            let max: u32 =
                                attr(&e, b"max").and_then(|v| v.parse().ok()).unwrap_or(min);
                            // 范围展开 + 钳制（防 min/max 反转洪水——
                            // max-min > 16384 展开面直接钳到 Excel 列上限）。
                            let hi = max.min(min.saturating_add(16384));
                            for n in min..=hi {
                                cols.push(col_letter(n));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    rows.sort_unstable();
    rows.dedup();
    cols.sort();
    cols.dedup();
    (rows, cols)
}

/// styles.xml：numFmts + cellXfs + cellStyleXfs（xf 三元组）。
///
/// `<xf>`/`<numFmt>` 双形态承载：自闭合（`<xf …/>`，Empty 事件）与
/// 带子元素（`<xf …><alignment/></xf>`，Start 事件——POI 真实件
/// 主流形态，49273 全部 466 个 xf 皆此形态）。属性都在开标签上，
/// 两形态同解析。
fn parse_styles(xml: &str) -> StyleTable {
    let mut st = StyleTable::default();
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    // 当前所在容器（numFmts / cellXfs / cellStyleXfs）。
    let mut in_numfmts = false;
    let mut in_cell_xfs = false;
    let mut in_style_xfs = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"numFmts" => in_numfmts = true,
                b"cellXfs" => in_cell_xfs = true,
                b"cellStyleXfs" => in_style_xfs = true,
                b"numFmt" if in_numfmts => {
                    let id = attr(&e, b"numFmtId").and_then(|v| v.parse().ok());
                    let code = attr(&e, b"formatCode");
                    if let (Some(id), Some(code)) = (id, code) {
                        st.custom_fmts.push((id, code));
                    }
                }
                b"xf" if in_cell_xfs || in_style_xfs => {
                    let xf = parse_xf(&e);
                    if in_cell_xfs {
                        st.cell_xfs.push(xf);
                    } else {
                        st.cell_style_xfs.push(xf);
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                // 自闭合形态（无子元素）。
                if in_numfmts && e.name().as_ref() == b"numFmt" {
                    let id = attr(&e, b"numFmtId").and_then(|v| v.parse().ok());
                    let code = attr(&e, b"formatCode");
                    if let (Some(id), Some(code)) = (id, code) {
                        st.custom_fmts.push((id, code));
                    }
                } else if (in_cell_xfs || in_style_xfs) && e.name().as_ref() == b"xf" {
                    let xf = parse_xf(&e);
                    if in_cell_xfs {
                        st.cell_xfs.push(xf);
                    } else {
                        st.cell_style_xfs.push(xf);
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"numFmts" => in_numfmts = false,
                b"cellXfs" => in_cell_xfs = false,
                b"cellStyleXfs" => in_style_xfs = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    st
}

/// `<xf numFmtId applyNumberFormat xfId/>` 三元组。
fn parse_xf(e: &quick_xml::events::BytesStart<'_>) -> XfDef {
    XfDef {
        num_fmt_id: attr(e, b"numFmtId")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        apply_number_format: attr(e, b"applyNumberFormat")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false),
        xf_id: attr(e, b"xfId").and_then(|v| v.parse().ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小容器构造（真实 xlsx 生成器面 = rust_xlsxwriter——测试
    /// 夹具构造用写面，语料面用真实件）。
    fn min_xlsx(sheet_xml: &str, styles_xml: &str, rels_extra: &str) -> Vec<u8> {
        let workbook = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let rels = format!(
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>{rels_extra}</Relationships>"#
        );
        let types = br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opt = zip::write::SimpleFileOptions::default();
            use std::io::Write as _;
            zw.start_file("[Content_Types].xml", opt).unwrap();
            zw.write_all(types).unwrap();
            zw.start_file("_rels/.rels", opt).unwrap();
            zw.write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#).unwrap();
            zw.start_file("xl/workbook.xml", opt).unwrap();
            zw.write_all(workbook.as_bytes()).unwrap();
            zw.start_file("xl/_rels/workbook.xml.rels", opt).unwrap();
            zw.write_all(rels.as_bytes()).unwrap();
            zw.start_file("xl/worksheets/sheet1.xml", opt).unwrap();
            zw.write_all(sheet_xml.as_bytes()).unwrap();
            zw.start_file("xl/styles.xml", opt).unwrap();
            zw.write_all(styles_xml.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        buf
    }

    #[test]
    fn hidden_rows_cols_parsed() {
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="2" hidden="1"/><row r="5" hidden="true"/><row r="7"/></sheetData><cols><col min="3" max="5" hidden="1" width="0"/></cols></worksheet>"#;
        let bytes = min_xlsx(sheet, "<styles/>", "");
        let rep = read_structural_from_bytes(&bytes).unwrap();
        let s = rep.sheet("S1").expect("sheet S1");
        assert_eq!(s.hidden_rows, vec![2, 5]);
        assert_eq!(
            s.hidden_cols,
            vec!["C".to_string(), "D".to_string(), "E".to_string()]
        );
    }

    #[test]
    fn external_link_target_parsed() {
        // ECMA-376 §15.2 两级：workbook rels → externalLink 部件
        //（Internal）→ 部件自己的 rels → TargetMode=External 外部目标。
        let rels_extra = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink" Target="externalLinks/externalLink1.xml"/>"#;
        let bytes = min_xlsx("<worksheet/>", "<styles/>", rels_extra);
        // 重写 zip：原部件 + externalLink 部件 + 其 rels（min_xlsx
        // 构造器不感知 externalLink——zip 不可追加，全量重写）。
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opt = zip::write::SimpleFileOptions::default();
            use std::io::Write as _;
            let mut cur = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            for i in 0..cur.len() {
                let mut f = cur.by_index(i).unwrap();
                zw.start_file(f.name().to_string(), opt).unwrap();
                std::io::copy(&mut f, &mut zw).unwrap();
            }
            zw.start_file("xl/externalLinks/externalLink1.xml", opt)
                .unwrap();
            zw.write_all(br#"<?xml version="1.0"?><externalLink xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#).unwrap();
            zw.start_file("xl/externalLinks/_rels/externalLink1.xml.rels", opt)
                .unwrap();
            zw.write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLinkPath" Target="file:///C:/quotes/source.xlsx" TargetMode="External"/></Relationships>"#).unwrap();
            zw.finish().unwrap();
        }
        let rep = read_structural_from_bytes(&buf).unwrap();
        assert_eq!(rep.external_links.len(), 1);
        assert_eq!(
            rep.external_links[0].target,
            "file:///C:/quotes/source.xlsx"
        );
        assert_eq!(rep.external_links[0].r_id, "rId1");
    }

    #[test]
    fn xf_inheritance_chain_resolved() {
        // cellXfs[2]: applyNumberFormat=0, xfId=1 → cellStyleXfs[1] 的
        // numFmtId=164（自定义 ¥#,##0.00）；cellXfs[0]: apply=1 id=9
        //（builtin 0%）；cellXfs[1]: apply=0 无 xfId → 兜底自身 id 0。
        let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="&quot;¥&quot;#,##0.00"/></numFmts><cellStyleXfs count="2"><xf numFmtId="0" applyNumberFormat="0"/><xf numFmtId="164" applyNumberFormat="1"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="9" applyNumberFormat="1" xfId="0"/><xf numFmtId="0" applyNumberFormat="0"/><xf numFmtId="0" applyNumberFormat="0" xfId="1"/></cellXfs></styleSheet>"#;
        let bytes = min_xlsx("<worksheet/>", styles, "");
        let rep = read_structural_from_bytes(&bytes).unwrap();
        let st = &rep.styles;
        assert_eq!(st.custom_fmts.len(), 1);
        assert_eq!(st.cell_xfs.len(), 3);
        assert_eq!(st.cell_style_xfs.len(), 2);
        assert_eq!(st.effective_num_fmt(0).as_deref(), Some("0%"));
        assert_eq!(st.effective_num_fmt(1).as_deref(), Some("General"));
        // 引号是格式码语法的一部分（字面包裹符）——原样承载，
        // 显示渲染时才剥（#161 显示面）。
        assert_eq!(st.effective_num_fmt(2).as_deref(), Some("\"¥\"#,##0.00"));
        // 越界 → None（优雅降级）。
        assert_eq!(st.effective_num_fmt(99), None);
    }

    #[test]
    fn builtin_table_covers_common_formats() {
        assert_eq!(builtin_num_fmt(14), Some("m/d/yyyy"));
        assert_eq!(builtin_num_fmt(4), Some("#,##0.00"));
        assert_eq!(builtin_num_fmt(49), Some("@"));
        assert_eq!(builtin_num_fmt(200), None);
        assert_eq!(col_letter(1), "A");
        assert_eq!(col_letter(27), "AA");
        assert_eq!(col_letter(52), "AZ");
    }
}
