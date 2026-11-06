//! docx-rs 读层 panic 预检防御（#226 R2 —— 审计 P0-2；真实语料 4 处
//! panic 实锤见 `test-artifacts/health-check/REPORT.md` #E1）。
//!
//! ## 为什么存在（根因 = 依赖层）
//!
//! docx-rs 0.4.22 读层对「OOXML 规范允许、真实世界存在」的形态不容忍：
//!
//! | 触发形态（语料实锤） | docx-rs 位点 | 表现 |
//! |---|---|---|
//! | `<w:u/>` 零属性（runprops_word365_props-{b,u}-styles.docx） | `run_property.rs:155` `attributes[0]` | index OOB panic |
//! | `<w:highlight/>` 零属性（runprops_word365_props-enum-styles.docx） | `run_property.rs:110` | 同上 |
//! | `w:w="178.05pt"` 通用度量（strict_strict-sample-docx.docx） | `width.rs:18` `f64::…expect` | ParseFloat panic |
//!
//! `w:u` 的 `w:val` 可省略（ECMA-376 17.3.2.42）；通用度量（pt/pc/in/
//! cm/mm 后缀）是 ST_MeasurementOrPercent 的合法成员——**不是输入
//! 畸形，是读层缺陷**。panic 在 wasm 面 = trap、GUI 面 = 进程崩溃
//!（用户未保存编辑随之丢失——审计 P0-2）。
//!
//! ## 修复策略（scan-and-repair——#E3 vml_textbox 同构先例）
//!
//! [`guard_docx_bytes`] 在 `docx_rs::read_docx` **之前**对全部
//! `.xml`/`.rels` 部件做两趟字符串预检：
//!
//! 1. **裸 run 属性元素**（`w:u`/`w:highlight`/`w:vertAlign`/`w:sz`
//!    零属性 start tag——run_property.rs 四个 `attributes[0]` 位点）：
//!    注入规范缺省 `w:val`（结构不变，只加属性）；
//! 2. **度量属性**（`w:w` 值剥 `%` 后非数值——width.rs 同判定）：通用
//!    度量换算 twips（pt×20 / pc×240 / in×1440 / cm×566.93 /
//!    mm×56.69），其余垃圾值归零（read_width 同语义路径）。
//!
//! 无命中 → 借用直通（`Cow::Borrowed`，零重打包成本）；有命中 →
//! 重打包（非目标部件 `raw_copy_file` 逐字节透传——docx_edit
//! repack_parts 同构）。两模式（严格/兼容）恒等启用——panic 防线不
//! 随模式放松（裸元素是合法 OOXML，非「兼容模式才放宽」的形态）；
//! 每处修复走既有 Warning 通道（裸元素 = [`WarningKind::SkippedElement`]
//! / 度量归一化 = [`WarningKind::MalformedXml`]），不静默。
//!
//! ## 已知边界
//!
//! - 字符串级扫描（不建 XML 树）：属性值/文本中字面出现 `<w:u/>`、
//!   `w:w="0pt"` 字样会被误修——合法 XML 属性值中 `<` 必须转义，现实
//!   面仅剩「正文文本恰好含该字样」的病理形态（E3 扫描同类边界，
//!   代价 = 一处文本值变化 + 警告，不崩溃）；
//! - 非 UTF-8 部件不修（交由既有门处置）。
//!
//! ## 不修什么（宁缺毋错）
//!
//! docx-rs 同类 `attributes[0]` 位点（`paragraph_property.rs:45/51/65`、
//! `table.rs:38`、`table_property.rs:35/40/45`、`style.rs:33`、
//! `numbering_property.rs:22/26`、`numberings.rs:41/45/82`、
//! `level.rs:39-75`、`level_override.rs:22`）**语料未实证、注入缺省有
//! 语义风险**（如 `w:pStyle` 引用）——不在本守卫面；上游跟踪项与完整
//! 位点清单见 REUSE.md docx-rs 节。未知形态由消费方入口 catch_unwind
//! 兜底（mang-shell #226 任务 1）。

use std::borrow::Cow;
use std::io::{Read as _, Write as _};

use crate::docx::DocxError;
use crate::io::{CompatibilityWarning, WarningKind};

/// 裸 run 属性元素 → 注入的规范缺省 `w:val`（docx-rs
/// `run_property.rs` 四个 `attributes[0]` 直接索引位点）。
///
/// 缺省值依据：`w:u` = `single`（ECMA-376 17.3.2.42 缺省）；
/// `w:vertAlign` = `baseline`（无上下标）；`w:highlight` 无规范缺省，
/// Word 365 裸形态语义 ≈ 无高亮（`none`）；`w:sz` 无缺省，`0` 半点 =
/// 值面无意义但读层可容（引擎行模型不消费字号）。
const BARE_RUN_PROPS: &[(&str, &str)] = &[
    ("u", "single"),
    ("highlight", "none"),
    ("vertAlign", "baseline"),
    ("sz", "0"),
];

/// 通用度量 → twips 换算表（ST_MeasurementOrPercent；dxa = 1/20 pt）。
const MEASURE_TO_TWIPS: &[(&str, f64)] = &[
    ("pt", 20.0),
    ("pc", 240.0),
    ("in", 1440.0),
    ("cm", 1440.0 / 2.54),
    ("mm", 1440.0 / 25.4),
];

/// 单部件修复清单（Warning 登记原料）。
#[derive(Default, Debug)]
struct PartFixes {
    /// (元素名, 注入次数)。
    bare: Vec<(&'static str, usize)>,
    /// (原值, 归一化值)。
    widths: Vec<(String, String)>,
}

/// `<w:` 之后是否为「目标裸元素 + 零属性闭合」形态。
///
/// 返回 `(元素名, 缺省值, 自 `<` 起的整标签长度)`；`after_prefix` 从
/// `<w:` 之后起始。前缀近邻（`<w:updateFields`、`<w:szCs` 等）因后续
/// 首字符非 `>`/`/>` 不匹配。
fn match_bare_run_prop(after_prefix: &str) -> Option<(&'static str, &'static str, usize)> {
    for &(name, default) in BARE_RUN_PROPS {
        if let Some(rest) = after_prefix.strip_prefix(name) {
            let ws_len = rest.len() - rest.trim_start().len();
            let tail = &rest[ws_len..];
            if tail.starts_with('>') {
                return Some((name, default, 3 + name.len() + ws_len + 1));
            }
            if tail.starts_with("/>") {
                return Some((name, default, 3 + name.len() + ws_len + 2));
            }
        }
    }
    None
}

/// 裸 run 属性元素注入（字符串级——结构不变，只加属性）。
///
/// `<w:u/>` → `<w:u w:val="single"/>`；`<w:u>` → `<w:u w:val="single">`
///（开闭标签形同样处理——docx-rs 在 StartEvent 即索引 `attributes[0]`）。
fn repair_bare_elements(xml: &str) -> Option<(String, Vec<(&'static str, usize)>)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    let mut out = String::with_capacity(xml.len() + 32);
    let mut cursor = 0usize;
    let mut hit = false;
    while let Some(rel) = xml[cursor..].find('<') {
        let lt = cursor + rel;
        out.push_str(&xml[cursor..lt]);
        let matched = if xml[lt..].starts_with("<w:") {
            match_bare_run_prop(&xml[lt + 3..])
        } else {
            None
        };
        match matched {
            Some((name, default, tag_len)) => {
                out.push_str("<w:");
                out.push_str(name);
                out.push_str(" w:val=\"");
                out.push_str(default);
                if xml[lt..lt + tag_len].ends_with("/>") {
                    out.push_str("\"/>");
                } else {
                    out.push_str("\">");
                }
                match counts.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, c)) => *c += 1,
                    None => counts.push((name, 1)),
                }
                hit = true;
                cursor = lt + tag_len;
            }
            None => {
                out.push('<');
                cursor = lt + 1;
            }
        }
    }
    out.push_str(&xml[cursor..]);
    if hit {
        Some((out, counts))
    } else {
        None
    }
}

/// `w:w` 值是否落入 docx-rs `read_width` 的 panic 判定（剥 `%` 后
/// `f64::from_str` 失败——width.rs:18 `.expect` 的前置条件）。
fn width_needs_repair(value: &str) -> bool {
    let stripped = value.strip_suffix('%').unwrap_or(value);
    stripped.parse::<f64>().is_err()
}

/// 通用度量 → twips；无可识别单位/数值 → `0`（与 read_width 归零后
/// 的读取语义一致——宽度信息缺席而非崩溃）。
fn normalize_measure(value: &str) -> String {
    let stripped = value.strip_suffix('%').unwrap_or(value);
    for &(unit, factor) in MEASURE_TO_TWIPS {
        if let Some(num) = stripped
            .strip_suffix(unit)
            .and_then(|n| n.parse::<f64>().ok())
        {
            return ((num * factor).round() as i64).to_string();
        }
    }
    "0".to_string()
}

/// `w:w` 度量归一化（仅标签内部——正文文本含该字样不误伤）。
///
/// 记账不变量：**已扫描区段逐字节入 `out` 再前进**（无需修复的值/
/// 非属性近邻原样吐过）——修复值之间的「无修复 w:w」（如 pgSz 数值
/// 宽）不丢中间段（回归锁：`no_repair_values_between_fixes_preserve_bytes`）。
fn repair_width_values(xml: &str) -> Option<(String, Vec<(String, String)>)> {
    let bytes = xml.as_bytes();
    let mut fixes: Vec<(String, String)> = Vec::new();
    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0usize;
    let ws = |p: &mut usize| {
        while matches!(
            bytes.get(*p),
            Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
        ) {
            *p += 1;
        }
    };
    while let Some(rel) = xml[cursor..].find("w:w") {
        let key = cursor + rel;
        // 必须位于标签内部：key 之前最近的 '<'/'>' 是 '<'（文本内容跳过）。
        let in_tag = xml[..key]
            .rfind(['<', '>'])
            .is_some_and(|b| bytes[b] == b'<');
        // key 之后：可选空白 + '=' + 可选空白 + '"' 值 '"'
        let mut p = key + 3;
        ws(&mut p);
        if !in_tag || bytes.get(p) != Some(&b'=') {
            let advance = key + 3;
            out.push_str(&xml[cursor..advance]);
            cursor = advance;
            continue;
        }
        p += 1;
        ws(&mut p);
        if bytes.get(p) != Some(&b'"') {
            let advance = key + 3;
            out.push_str(&xml[cursor..advance]);
            cursor = advance;
            continue;
        }
        p += 1;
        let val_start = p;
        let Some(end_rel) = xml[val_start..].find('"') else {
            // 值未闭合——畸形面（门 3 会拒/警告）；剩余交给 final push。
            break;
        };
        let val_end = val_start + end_rel;
        let value = &xml[val_start..val_end];
        if width_needs_repair(value) {
            let replacement = normalize_measure(value);
            out.push_str(&xml[cursor..val_start]);
            out.push_str(&replacement);
            fixes.push((value.to_string(), replacement));
        } else {
            // 无需修复：原样吐过（含开引号与值；闭引号由后续推入）。
            out.push_str(&xml[cursor..val_end]);
        }
        cursor = val_end;
    }
    out.push_str(&xml[cursor..]);
    if fixes.is_empty() {
        None
    } else {
        Some((out, fixes))
    }
}

/// 单部件两趟修复（裸元素 → 度量，顺序串接）。
fn repair_xml_part(xml: &str) -> Option<(String, PartFixes)> {
    let mut fixes = PartFixes::default();
    let mut current: Option<String> = None;
    if let Some((out, bare)) = repair_bare_elements(xml) {
        fixes.bare = bare;
        current = Some(out);
    }
    let src: &str = current.as_deref().unwrap_or(xml);
    if let Some((out, widths)) = repair_width_values(src) {
        fixes.widths = widths;
        current = Some(out);
    }
    current.map(|c| (c, fixes))
}

/// docx-rs 读层 panic 预检（`read_bytes_gated` 在 `docx_rs::read_docx`
/// 之前调用）。
///
/// 无命中 → `Cow::Borrowed`（零重打包直通）；有命中 → 重打包（修复件
/// 替换内容、其余 `raw_copy_file` 逐字节透传），修复清单入 `warnings`。
pub(crate) fn guard_docx_bytes<'a>(
    bytes: &'a [u8],
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Cow<'a, [u8]>, DocxError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| DocxError::Io(format!("panic guard: open zip: {e}")))?;
    // 第一趟：扫全部 .xml/.rels 部件（UTF-8 可解码者），收集修复件。
    let mut repaired: Vec<(String, String, PartFixes)> = Vec::new();
    for i in 0..archive.len() {
        let name = {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| DocxError::Io(format!("panic guard: zip index: {e}")))?;
            entry.name().to_string()
        };
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        let data = {
            let mut zf = archive
                .by_index(i)
                .map_err(|e| DocxError::Io(format!("panic guard: zip read: {e}")))?;
            let mut buf = String::new();
            if zf.read_to_string(&mut buf).is_err() {
                continue; // 非 UTF-8 部件：不修，交由既有门处置。
            }
            buf
        };
        if let Some((xml, fixes)) = repair_xml_part(&data) {
            repaired.push((name, xml, fixes));
        }
    }
    if repaired.is_empty() {
        return Ok(Cow::Borrowed(bytes));
    }
    // 第二趟：重打包——修复件替换内容，其余逐字节透传。
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let name = {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| DocxError::Io(format!("panic guard: zip index: {e}")))?;
            entry.name().to_string()
        };
        match repaired.iter().find(|(n, _, _)| *n == name) {
            Some((_, xml, _)) => {
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| DocxError::Io(format!("panic guard: zip start: {e}")))?;
                writer
                    .write_all(xml.as_bytes())
                    .map_err(|e| DocxError::Io(format!("panic guard: zip write: {e}")))?;
            }
            None => {
                let entry = archive
                    .by_index_raw(i)
                    .map_err(|e| DocxError::Io(format!("panic guard: zip index: {e}")))?;
                writer
                    .raw_copy_file(entry)
                    .map_err(|e| DocxError::Io(format!("panic guard: zip copy: {e}")))?;
            }
        }
    }
    let out = writer
        .finish()
        .map_err(|e| DocxError::Io(format!("panic guard: zip finish: {e}")))?;
    // Warning 登记（不静默）：裸元素注入与度量归一化各走各的 kind。
    for (name, _, fixes) in &repaired {
        if !fixes.bare.is_empty() {
            let list: Vec<String> = fixes
                .bare
                .iter()
                .map(|(n, c)| format!("<w:{n}/> ×{c}"))
                .collect();
            warnings.push(CompatibilityWarning {
                part: name.clone(),
                kind: WarningKind::SkippedElement,
                detail: format!(
                    "docx-rs panic 防御：零属性 run 属性 {} 注入规范缺省 w:val\
                     （run_property.rs:110/126/132/155 index OOB）",
                    list.join("、")
                ),
            });
        }
        if !fixes.widths.is_empty() {
            let list: Vec<String> = fixes
                .widths
                .iter()
                .map(|(a, b)| format!("\"{a}\"→{b}"))
                .collect();
            warnings.push(CompatibilityWarning {
                part: name.clone(),
                kind: WarningKind::MalformedXml,
                detail: format!(
                    "docx-rs panic 防御：w:w 通用度量归一化 {}（width.rs:18 f64 expect）",
                    list.join("、")
                ),
            });
        }
    }
    Ok(Cow::Owned(out.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 夹具（docx_edit 测试同构的最小 docx——容器门全过）──────────

    const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId7\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.com/\" TargetMode=\"External\"/></Relationships>";

    fn document_wrap(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{body}\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:body></w:document>"
        )
    }

    fn build_docx(body: &str) -> Vec<u8> {
        let document = document_wrap(body);
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in [
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", DOC_RELS),
            ("word/document.xml", &document),
        ] {
            w.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            use std::io::Write as _;
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    // ── 裸元素注入 ─────────────────────────────────────────────

    /// 语料实锤形态：自闭合与开闭标签两形均注入缺省；计数齐全。
    #[test]
    fn bare_run_props_inject_defaults() {
        let src = r#"<w:rPr><w:u/><w:u></w:u><w:highlight/><w:vertAlign/><w:sz/></w:rPr>"#;
        let (out, counts) = repair_bare_elements(src).expect("五处全命中");
        assert!(out.contains(r#"<w:u w:val="single"/>"#), "{out}");
        assert!(out.contains(r#"<w:u w:val="single"></w:u>"#), "{out}");
        assert!(out.contains(r#"<w:highlight w:val="none"/>"#), "{out}");
        assert!(out.contains(r#"<w:vertAlign w:val="baseline"/>"#), "{out}");
        assert!(out.contains(r#"<w:sz w:val="0"/>"#), "{out}");
        assert!(counts.contains(&("u", 2)));
        assert!(counts.contains(&("highlight", 1)));
        assert!(counts.contains(&("vertAlign", 1)));
        assert!(counts.contains(&("sz", 1)));
    }

    /// 带属性形态与前缀近邻（updateFields/szCs）不误伤。
    #[test]
    fn attributed_and_prefix_neighbors_untouched() {
        let src = r#"<w:u w:val="single"/><w:sz w:val="24"/><w:updateFields w:val="true"/><w:szCs w:val="24"/><w:settings><w:u/></w:settings>"#;
        let (out, counts) = repair_bare_elements(src).expect("仅 settings 内裸 u 命中");
        assert!(out.contains(r#"<w:u w:val="single"/>"#));
        assert!(
            out.contains(r#"<w:sz w:val="24"/>"#),
            "带属性 sz 不动：{out}"
        );
        assert!(out.contains("<w:updateFields w:val=\"true\"/>"), "{out}");
        assert!(out.contains("<w:szCs w:val=\"24\"/>"), "{out}");
        assert!(out.contains(r#"<w:u w:val="single"/>"#));
        assert_eq!(counts, vec![("u", 1)]);
    }

    // ── 度量归一化 ─────────────────────────────────────────────

    #[test]
    fn width_measures_convert_and_garbage_zeroes() {
        assert_eq!(normalize_measure("178.05pt"), "3561");
        assert_eq!(normalize_measure("0pt"), "0");
        assert_eq!(normalize_measure("595.35pt"), "11907");
        assert_eq!(normalize_measure("1pc"), "240");
        assert_eq!(normalize_measure("1in"), "1440");
        assert_eq!(normalize_measure("2.54cm"), "1440");
        assert_eq!(normalize_measure("10mm"), "567");
        assert_eq!(normalize_measure("bogus"), "0");
        assert_eq!(normalize_measure(""), "0");
    }

    #[test]
    fn width_needs_repair_boundary() {
        for ok in ["5000", "50%", "-3", "1.5", "0"] {
            assert!(!width_needs_repair(ok), "{ok} 不应修");
        }
        for bad in ["0pt", "178.05pt", "bogus", ""] {
            assert!(width_needs_repair(bad), "{bad} 应修");
        }
    }

    /// 标签内 `w:w` 逐值归一化；正文文本含同字样不误伤。
    #[test]
    fn width_values_in_tags_only() {
        let src = r#"<w:tblW w:w="0pt" w:type="auto"/><w:tcW w:w="178.05pt" w:type="dxa"/>"#;
        let (out, fixes) = repair_width_values(src).expect("两处命中");
        assert!(out.contains(r#"w:w="0""#), "{out}");
        assert!(out.contains(r#"w:w="3561""#), "{out}");
        assert_eq!(fixes.len(), 2);

        let text = r#"<w:t>参见 w:w="0pt" 的说明（正文文本不修）</w:t>"#;
        assert!(repair_width_values(text).is_none(), "文本内容不误伤");
    }

    /// 回归锁（记账 bug——首版实现实证）：修复值后跟「无修复 w:w」
    ///（pgSz 数值宽）时，中间段逐字节保留。首版只在无需修复路径推进
    /// cursor 不吐字节，吞掉了 `w:type="auto"/>…<w:pgSz w:w="11906"`
    /// 整段（strict 语料即此形态——文档直接解析失败）。
    #[test]
    fn no_repair_values_between_fixes_preserve_bytes() {
        let src = concat!(
            r#"<w:tbl><w:tblPr><w:tblW w:w="0pt" w:type="auto"/></w:tblPr>"#,
            r#"<w:tr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
            r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
        );
        let (out, fixes) = repair_width_values(src).expect("tblW 一处命中");
        assert_eq!(fixes, vec![("0pt".to_string(), "0".to_string())]);
        assert!(out.contains(r#"<w:tblW w:w="0" w:type="auto"/>"#), "{out}");
        assert!(
            out.contains(r#"<w:pgSz w:w="11906" w:h="16838"/>"#),
            "无修复 w:w 原样保留：{out}"
        );
        assert!(out.contains("<w:t>c</w:t>"), "中间段不丢：{out}");
        assert_eq!(
            out.replace("w:w=\"0\"", "w:w=\"0pt\""),
            src,
            "除修复值外逐字节等价"
        );
    }

    // ── 部件级（clean 直通 / 语料形态端到端）──────────────────

    /// 干净包零成本直通（借用指针相等）+ 零警告。
    #[test]
    fn clean_package_passes_through_borrowed() {
        let bytes = build_docx(
            r#"<w:p><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>ok</w:t></w:r></w:p>"#,
        );
        let mut warnings = Vec::new();
        let out = guard_docx_bytes(&bytes, &mut warnings).expect("guard ok");
        assert!(
            matches!(&out, Cow::Borrowed(b) if b.as_ptr() == bytes.as_ptr()),
            "无命中必须借用直通"
        );
        assert!(warnings.is_empty());
    }

    /// 语料实锤形态端到端：无防御 = docx-rs panic（catch_unwind 实证）；
    /// 有防御 = 引擎读面 Ok + 结构化警告（裸元素 + 度量两 kind 齐全）。
    #[test]
    fn corpus_panic_forms_read_ok_with_warnings() {
        let body = concat!(
            r#"<w:p><w:r><w:rPr><w:rStyle w:val="X"/><w:u/></w:rPr><w:t>S11</w:t></w:r></w:p>"#,
            r#"<w:p><w:r><w:rPr><w:highlight/></w:rPr><w:t>M4</w:t></w:r></w:p>"#,
            r#"<w:p><w:r><w:rPr><w:vertAlign/><w:sz/></w:rPr><w:t>M8</w:t></w:r></w:p>"#,
            r#"<w:tbl><w:tblPr><w:tblW w:w="0pt" w:type="auto"/></w:tblPr>#,
            r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="178.05pt" w:type="dxa"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>"#,
        );
        let bytes = build_docx(body);

        // 依赖层实锤（run_property.rs 位点）：无防御直接 panic。
        let raw =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| docx_rs::read_docx(&bytes)));
        assert!(
            raw.is_err(),
            "无防御基线应 panic（run_property.rs:110/155）"
        );

        // 引擎读面：预检防御 → Ok + 警告不静默。
        let doc = crate::docx::read_docx_from_bytes_with_options(
            &bytes,
            &crate::io::ReadOptions { compat: true },
        )
        .expect("防御后可读");
        assert!(
            doc.warnings
                .iter()
                .any(|w| w.part == "word/document.xml"
                    && matches!(w.kind, WarningKind::SkippedElement)),
            "裸元素警告缺席：{:?}",
            doc.warnings
        );
        assert!(
            doc.warnings
                .iter()
                .any(|w| w.part == "word/document.xml"
                    && matches!(w.kind, WarningKind::MalformedXml)),
            "度量归一化警告缺席：{:?}",
            doc.warnings
        );
    }

    /// width.rs 位点独立实证：纯度量形态（无裸元素）无防御时同样 panic。
    #[test]
    fn width_panic_site_proven_independently() {
        let bytes = build_docx(
            r#"<w:tbl><w:tblPr><w:tblW w:w="0pt" w:type="auto"/></w:tblPr><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#,
        );
        let raw =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| docx_rs::read_docx(&bytes)));
        assert!(raw.is_err(), "无防御基线应 panic（width.rs:18）");
        let doc = crate::docx::read_docx_from_bytes_with_options(
            &bytes,
            &crate::io::ReadOptions { compat: true },
        )
        .expect("防御后可读");
        assert!(doc.warnings.iter().any(|w| w.part == "word/document.xml"));
    }
}
