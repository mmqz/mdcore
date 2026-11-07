//! #344（#318 D1）：MCE（Markup Compatibility，ISO/IEC 29500-3）选择面
//! ——`mc:AlternateContent` 多分支全展开（**内容重复缺陷**）的读前修复。
//!
//! ## 缺陷机制（结构账本 #260 GT 对照实证）
//!
//! docx-rs 读面无 MCE 选择面：`document.rs`/`paragraph.rs`/`run.rs` 等
//! 读者对未知容器（`AlternateContent`/`Choice`/`Fallback`）`_ => {}`
//! **透明下降**——容器标签被忽略、内部 `w:p`/`w:r` 全部被收进块流。
//! 结果：同一内容以 N 份进正文（mc-fun_mc-block 单件 10 Choice +
//! 1 Fallback = 11 份段落；Word 2010 wps 文本框双写形态 = DrawingML
//! 主选 + VML 副本双份）。消费方（标书侧）把重复文本当真实条款。
//!
//! ## 修复策略（scan-and-rewrite——docx_panic_guard 同构先例）
//!
//! [`select_mce`] 在 `read_bytes_gated` 的门后、OPC 关系层之前对
//! ZIP 字节做一趟重写：每个**外层** `mc:AlternateContent` 只保留
//! **选中分支**的内容（unwrap 展开——AC/分支容器标签一并剥除，内容
//! 提升到宿主位置），其余分支整支剥除。选择语义（ISO 29500-3
//! §10.1.1「首个可理解 Choice，都不理解才 Fallback」的读面落地）：
//!
//! · **首个 `mc:Choice` 无条件选中**——docx-rs 按 local-name 解析，
//!   分支内容（WordML/DrawingML）不因 `Requires` 标签改变可读性；
//!   Word 写盘时首选分支（最新形态）恒在首位（newest-first 语料
//!   实锚）；
//! · **无 Choice → `mc:Fallback` 兜底**——典型为 VML 副本
//!   （`w:pict → v:textbox`，由 #18 第二趟提取，unwrap 后不再处于
//!   Fallback 上下文——`scan_vml_textbox_regions` 的防双取特判自然
//!   消解为正确提取）；
//! · **畸形/零分支 AC**（意外直子 / 未闭合 / 无 Choice 无 Fallback）
//!   → 整段**原样保留**（宁缺毋错——行为不劣于修复前；零内容 AC
//!   剥不剥除无语义差异，保留 = 零风险）。
//!
//! ## 单视图不变量（防双取与索引错位）
//!
//! 重写在 OPC 关系层块**之前**接线：引擎字节（docx-rs）、`document_xml`
//! 原始串（VML 文本框第二趟 / 脚注引用扫描）与 panic guard **消费同一
//! 过滤后视图**——修复前 `apply_vml_textboxes` 需要「Fallback 副本防
//! 双取」特判、`scan_note_references` 的 body 块索引与引擎块序在
//! 「body 级 AC 扁平段 + 后续段落含脚注引用」同场时错位（#318 附带
//! 发现）；过滤后 AC 扁平段成为 body 直接段，两个第二趟的 XML 侧
//! 索引与模型块序自然对齐。
//!
//! ## 覆盖面与成本
//!
//! · 全部 `.xml` 部件（document/headers/footers/footnotes/endnotes/
//!   comments——extraction-probe 族 header1/2.xml 实锚 AC 在场）；
//!   `.rels`/媒体件不涉及（子串预检直通）；
//! · 无命中部件 `Cow::Borrowed` 零重打包；有命中 → 重打包（非目标
//!   部件 `raw_copy_file` 逐字节透传）；
//! · 每处展开走 Warning 通道（可见性——不静默）。
//!
//! ## 不做什么（边界登记）
//!
//! · `mc:Ignorable` 属性处理——docx-rs 未知元素本就透明下降，无
//!   消费差异；
//! · `Requires` 依赖链版本仲裁——不做命名空间理解力判定（策略见上）；
//! · AC 直子层的嵌套 AC = 畸形（规范要求分支包裹）→ 原样保留；
//!   分支**内部**的嵌套 AC 由递归重写处理。

use std::borrow::Cow;
use std::io::{Read as _, Write as _};

use quick_xml::events::Event;

use crate::docx::DocxError;
use crate::io::{CompatibilityWarning, WarningKind};

/// `buffer_position()`（u64）→ 偏移切序（usize——部件尺寸 ≤ 门 0
/// 限额，无损断言见 io 限额面）。
fn pos(reader: &quick_xml::Reader<&[u8]>) -> usize {
    reader.buffer_position() as usize
}

/// local-name 匹配（命名空间前缀无关——`mc:`/`mce:`/strict purl 同构）。
const AC_NAME: &[u8] = b"AlternateContent";
const BRANCH_CHOICE: &[u8] = b"Choice";
const BRANCH_FALLBACK: &[u8] = b"Fallback";

/// 单部件重写产出（Warning 登记原料）。
#[derive(Default, Debug)]
struct PartStats {
    /// 展开的 AC 数（选中分支内容提升）。
    ac_expanded: usize,
    /// 剥除的非选中分支数（首个 Choice 后的重复 Choice + Fallback 副本）。
    branches_dropped: usize,
}

/// AC 子树处理结果。
enum AcOutcome {
    /// 选中分支内容区段 `[inner_start, inner_end)` + AC 结束偏移 +
    /// 剥除分支数。
    Selected {
        inner_start: usize,
        inner_end: usize,
        ac_end: usize,
        dropped: usize,
    },
    /// 畸形 / 零分支——整段原样保留（子树已消费至 AC 结束或 Eof）。
    Malformed,
}

/// 消费当前已开分支的剩余事件直至其 End（返回 End 起始偏移 = 分支
/// 内容右边界；嵌套 Start 深度计数）。Eof/扫描错（未闭合）→ None。
fn skip_branch_tail(reader: &mut quick_xml::Reader<&[u8]>) -> Option<usize> {
    let mut depth = 0usize;
    loop {
        let before = pos(reader);
        match reader.read_event() {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return Some(before);
                }
                depth -= 1;
            }
            Ok(Event::Eof) => return None,
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

/// 消费剩余事件直至 AC 自身的 End（或 Eof——畸形收尾兜底）。
///
/// 供 [`AcOutcome::Malformed`] 路径使用：调用后外层循环恰在 AC 之后
/// 恢复（后续事件不再被误当顶层内容）。对良构输入，此处遇到的下一个
/// `AlternateContent` End 即本 AC 的 End（嵌套 AC 只会在分支内部、
/// 已由分支消费完毕）。
fn consume_to_ac_end(reader: &mut quick_xml::Reader<&[u8]>) {
    loop {
        match reader.read_event() {
            Ok(Event::End(e)) if e.local_name().as_ref() == AC_NAME => return,
            Ok(Event::Eof) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// 处理一个外层 AC 子树（`AlternateContent` Start 已消费；返回时消费
/// 至匹配 End 或 Eof）。
///
/// 分支迭代：Choice/Fallback 直子按文档序处理——首个 Choice 选中
/// （记录内容区段），后续 Choice 与「Choice 已出现后的 Fallback」
/// 剥除；无 Choice 时 Fallback 选中。意外直子 / 非空白文本直子 /
/// 未闭合 → [`AcOutcome::Malformed`]（原样保留）。
fn handle_alternate_content(reader: &mut quick_xml::Reader<&[u8]>) -> AcOutcome {
    let mut selected: Option<(usize, usize)> = None;
    let mut saw_choice = false;
    let mut dropped = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                BRANCH_CHOICE => {
                    let inner_start = pos(reader);
                    let Some(inner_end) = skip_branch_tail(reader) else {
                        return AcOutcome::Malformed;
                    };
                    if !saw_choice {
                        selected = Some((inner_start, inner_end));
                        saw_choice = true;
                    } else {
                        dropped += 1;
                    }
                }
                BRANCH_FALLBACK => {
                    let inner_start = pos(reader);
                    let Some(inner_end) = skip_branch_tail(reader) else {
                        return AcOutcome::Malformed;
                    };
                    if saw_choice {
                        dropped += 1;
                    } else {
                        selected = Some((inner_start, inner_end));
                    }
                }
                // 意外直子（含直子层嵌套 AC——规范要求分支包裹）。
                _ => {
                    consume_to_ac_end(reader);
                    return AcOutcome::Malformed;
                }
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                BRANCH_CHOICE => {
                    // 空分支：零内容；首个空 Choice 仍消费选择权
                    //（MCE 语义——选中分支可为空）。
                    if !saw_choice {
                        let at = pos(reader);
                        selected = Some((at, at));
                        saw_choice = true;
                    }
                }
                BRANCH_FALLBACK => {
                    if !saw_choice {
                        let at = pos(reader);
                        selected = Some((at, at));
                    }
                }
                _ => {
                    consume_to_ac_end(reader);
                    return AcOutcome::Malformed;
                }
            },
            Ok(Event::Text(t)) => {
                // 分支间空白容差（格式化换行）；非空白文本直子 = 畸形。
                if !t.decode().map(|c| c.trim().is_empty()).unwrap_or(true) {
                    consume_to_ac_end(reader);
                    return AcOutcome::Malformed;
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == AC_NAME => {
                let ac_end = pos(reader);
                return match selected {
                    Some((inner_start, inner_end)) => AcOutcome::Selected {
                        inner_start,
                        inner_end,
                        ac_end,
                        dropped,
                    },
                    // 零分支 AC：零内容——原样保留（剥不剥除无语义差异）。
                    None => AcOutcome::Malformed,
                };
            }
            // AC 直子层的意外 End（失衡面——门 2+3 常态下不至）。
            Ok(Event::End(_)) => {
                consume_to_ac_end(reader);
                return AcOutcome::Malformed;
            }
            Ok(Event::Eof) => return AcOutcome::Malformed,
            // 注释/PI/声明等零内容噪音：容差直过。
            Ok(_) => {}
            Err(_) => return AcOutcome::Malformed,
        }
    }
}

/// 单部件 XML 重写（offset 切片拼接——未命中区段零成本直拷）。
///
/// 命中 → `(重写后 XML, 统计)`；无展开 / 扫描错 → `None`（调用面
/// 不修）。选中分支内容若含嵌套 AC（分支内部），递归重写（内层切片
/// 独立扫描——内层扫描错则内层原样）。
fn rewrite_part(xml: &str) -> Option<(String, PartStats)> {
    if !xml.contains("AlternateContent") {
        return None;
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().allow_unmatched_ends = false;
    let mut stats = PartStats::default();
    let mut out = String::with_capacity(xml.len());
    let mut copied = 0usize; // [0, copied) 已入 out
    loop {
        let tag_start = pos(&reader);
        match reader.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == AC_NAME => {
                match handle_alternate_content(&mut reader) {
                    AcOutcome::Selected {
                        inner_start,
                        inner_end,
                        ac_end,
                        dropped,
                    } => {
                        out.push_str(&xml[copied..tag_start]);
                        let inner = &xml[inner_start..inner_end];
                        if inner.contains("AlternateContent") {
                            match rewrite_part(inner) {
                                Some((rewritten, sub)) => {
                                    out.push_str(&rewritten);
                                    stats.ac_expanded += sub.ac_expanded;
                                    stats.branches_dropped += sub.branches_dropped;
                                }
                                None => out.push_str(inner),
                            }
                        } else {
                            out.push_str(inner);
                        }
                        copied = ac_end;
                        stats.ac_expanded += 1;
                        stats.branches_dropped += dropped;
                    }
                    // 畸形/零分支：不推进 copied——区段随后续直拷原样保留。
                    AcOutcome::Malformed => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return None, // 门 2+3 已过校验的件不应到这；宁缺毋错。
        }
    }
    if stats.ac_expanded == 0 {
        return None;
    }
    out.push_str(&xml[copied..]);
    Some((out, stats))
}

/// MCE 选择重写（`read_bytes_gated` 在 OPC 关系层之前调用）。
///
/// 无命中 → `Cow::Borrowed`（零重打包直通）；有命中 → 重打包（重写件
/// 替换内容、其余 `raw_copy_file` 逐字节透传），展开统计入 `warnings`
/// （不静默）。
pub(crate) fn select_mce<'a>(
    bytes: &'a [u8],
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Cow<'a, [u8]>, DocxError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| DocxError::Io(format!("mce select: open zip: {e}")))?;
    // 第一趟：扫全部 .xml 部件（子串预检直通——绝大多数部件零 AC）。
    let mut rewritten: Vec<(String, String, PartStats)> = Vec::new();
    for i in 0..archive.len() {
        let name = {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| DocxError::Io(format!("mce select: zip index: {e}")))?;
            entry.name().to_string()
        };
        if !name.ends_with(".xml") {
            continue;
        }
        let data = {
            let mut zf = archive
                .by_index(i)
                .map_err(|e| DocxError::Io(format!("mce select: zip read: {e}")))?;
            let mut buf = String::new();
            if zf.read_to_string(&mut buf).is_err() {
                continue; // 非 UTF-8 部件：不修（交由既有门处置）。
            }
            buf
        };
        if let Some((xml, stats)) = rewrite_part(&data) {
            rewritten.push((name, xml, stats));
        }
    }
    if rewritten.is_empty() {
        return Ok(Cow::Borrowed(bytes));
    }
    // 第二趟：重打包——重写件替换内容，其余逐字节透传。
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let name = {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| DocxError::Io(format!("mce select: zip index: {e}")))?;
            entry.name().to_string()
        };
        match rewritten.iter().find(|(n, _, _)| *n == name) {
            Some((_, xml, _)) => {
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| DocxError::Io(format!("mce select: zip start: {e}")))?;
                writer
                    .write_all(xml.as_bytes())
                    .map_err(|e| DocxError::Io(format!("mce select: zip write: {e}")))?;
            }
            None => {
                let entry = archive
                    .by_index_raw(i)
                    .map_err(|e| DocxError::Io(format!("mce select: zip index: {e}")))?;
                writer
                    .raw_copy_file(entry)
                    .map_err(|e| DocxError::Io(format!("mce select: zip copy: {e}")))?;
            }
        }
    }
    let out = writer
        .finish()
        .map_err(|e| DocxError::Io(format!("mce select: zip finish: {e}")))?;
    // Warning 登记（不静默）：每部件一条汇总。
    for (name, _, stats) in &rewritten {
        warnings.push(CompatibilityWarning {
            part: name.clone(),
            kind: WarningKind::SkippedElement,
            detail: format!(
                "MCE 选择：{} 处 AlternateContent 展开（首 Choice 选中 / {} 分支剥除；\
                 ISO 29500-3——原先全部 Choice+Fallback 重复进正文）",
                stats.ac_expanded, stats.branches_dropped
            ),
        });
    }
    Ok(Cow::Owned(out.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 夹具（docx_panic_guard 测试同构的最小 docx——容器门全过）──

    const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>";

    fn document_wrap(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">\
<w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:body></w:document>"
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

    fn choice(ns: &str, text: &str) -> String {
        format!("<mc:Choice Requires=\"{ns}\"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></mc:Choice>")
    }

    fn fallback(text: &str) -> String {
        format!("<mc:Fallback><w:p><w:r><w:t>{text}</w:t></w:r></w:p></mc:Fallback>")
    }

    fn top_paras(doc: &crate::docx::DocxDocument) -> Vec<String> {
        doc.blocks
            .iter()
            .filter_map(|b| match b {
                crate::docx::Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect()
    }

    fn read(bytes: &[u8]) -> crate::docx::DocxDocument {
        crate::docx::read_docx_from_bytes_with_options(
            bytes,
            &crate::io::ReadOptions { compat: true },
        )
        .expect("compat read")
    }

    #[test]
    fn no_ac_no_rewrite_borrowed() {
        let bytes = build_docx("<w:p><w:r><w:t>plain</w:t></w:r></w:p>");
        let mut warnings = Vec::new();
        let out = select_mce(&bytes, &mut warnings).unwrap();
        assert!(matches!(out, Cow::Borrowed(_)), "零 AC 直通借用");
        assert!(warnings.is_empty());
    }

    #[test]
    fn ten_choices_collapse_to_first() {
        // mc-fun_mc-block 实锚形态：10 Choice（各含一段）+ 1 Fallback。
        let mut ac = String::new();
        for ns in [
            "w14", "w15", "w16se", "w16cid", "w16", "w16cex", "w16sdtdh", "w16sdtfl", "w16du",
            "wp14",
        ] {
            ac.push_str(&choice(ns, ns));
        }
        ac.push_str(&fallback("fallback"));
        let bytes = build_docx(&format!(
            "<w:p><w:r><w:t>头</w:t></w:r></w:p><mc:AlternateContent>{ac}</mc:AlternateContent><w:p><w:r><w:t>尾</w:t></w:r></w:p>"
        ));
        let mut warnings = Vec::new();
        let out = select_mce(&bytes, &mut warnings).unwrap();
        assert!(matches!(out, Cow::Owned(_)), "有 AC 必重打包");
        let doc = read(&out);
        let paras = top_paras(&doc);
        assert_eq!(
            paras,
            vec!["头".to_string(), "w14".to_string(), "尾".to_string()],
            "11 份重复 → 首 Choice 单份：{paras:?}"
        );
        assert_eq!(warnings.len(), 1, "Warning 可见性（单部件汇总）");
        assert!(
            warnings[0].detail.contains("1 处"),
            "展开计数：{}",
            warnings[0].detail
        );
        assert!(
            warnings[0].detail.contains("10 分支"),
            "剥除计数：{}",
            warnings[0].detail
        );
    }

    #[test]
    fn fallback_only_selected_when_no_choice() {
        let bytes = build_docx(&format!(
            "<mc:AlternateContent>{}</mc:AlternateContent>",
            fallback("兜底")
        ));
        let out = select_mce(&bytes, &mut Vec::new()).unwrap();
        let doc = read(&out);
        assert_eq!(
            top_paras(&doc),
            vec!["兜底".to_string()],
            "无 Choice → Fallback 选中"
        );
    }

    #[test]
    fn no_fallback_keeps_first_choice() {
        // mc-fun_mc-block-nofallback 实锚形态。
        let ac = format!("{}{}", choice("w14", "一"), choice("w15", "二"));
        let bytes = build_docx(&format!("<mc:AlternateContent>{ac}</mc:AlternateContent>"));
        let doc = read(&select_mce(&bytes, &mut Vec::new()).unwrap());
        assert_eq!(
            top_paras(&doc),
            vec!["一".to_string()],
            "无 Fallback 首选不变"
        );
    }

    #[test]
    fn run_level_ac_text_single_copy() {
        // mc-fun_mc-text 实锚形态：AC 在 w:r 内（Choice 内容 = 裸 w:t——
        // 修复前全部 Choice+Fallback 文本串接进同一 run）。
        let bytes = build_docx(&format!(
            "<w:p><w:r><w:t>前</w:t></w:r><w:r><mc:AlternateContent>{}{}{}</mc:AlternateContent></w:r><w:r><w:t>后</w:t></w:r></w:p>",
            "<mc:Choice Requires=\"w14\"><w:t>选</w:t></mc:Choice>",
            "<mc:Choice Requires=\"w15\"><w:t>弃</w:t></mc:Choice>",
            "<mc:Fallback><w:t>兜</w:t></mc:Fallback>"
        ));
        let doc = read(&select_mce(&bytes, &mut Vec::new()).unwrap());
        assert_eq!(
            top_paras(&doc),
            vec!["前选后".to_string()],
            "run 级 AC 单份内联"
        );
    }

    #[test]
    fn nested_ac_inside_selected_branch() {
        let inner = format!(
            "<mc:AlternateContent>{}{}</mc:AlternateContent>",
            choice("w15", "内选"),
            fallback("内兜")
        );
        let bytes = build_docx(&format!(
            "<mc:AlternateContent><mc:Choice Requires=\"w14\"><w:p><w:r><w:t>外</w:t></w:r></w:p>{inner}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>",
            fallback("外兜")
        ));
        let doc = read(&select_mce(&bytes, &mut Vec::new()).unwrap());
        assert_eq!(
            top_paras(&doc),
            vec!["外".to_string(), "内选".to_string()],
            "分支内嵌套 AC 递归选择"
        );
    }

    #[test]
    fn malformed_ac_verbatim_preserved() {
        // 意外直子（规范外）：AC 整段原样保留——行为不劣于修复前。
        let bytes = build_docx(
            "<mc:AlternateContent><w:p><w:r><w:t>意外直子</w:t></w:r></w:p></mc:AlternateContent>",
        );
        let out = select_mce(&bytes, &mut Vec::new()).unwrap();
        assert!(
            matches!(out, Cow::Borrowed(_)),
            "畸形 AC 零重写（宁缺毋错）"
        );
        // 修复前行为（透明下降 → 段落进正文）保持。
        assert_eq!(top_paras(&read(&bytes)), vec!["意外直子".to_string()]);
    }

    #[test]
    fn empty_ac_verbatim_preserved() {
        // 零分支 AC（开闭/自闭合）：零内容——原样保留（无语义差异）。
        let bytes = build_docx(
            "<mc:AlternateContent></mc:AlternateContent><w:p><w:r><w:t>旁</w:t></w:r></w:p>",
        );
        let out = select_mce(&bytes, &mut Vec::new()).unwrap();
        assert!(matches!(out, Cow::Borrowed(_)), "零分支 AC 零重写");
        let doc = read(&out);
        assert_eq!(top_paras(&doc), vec!["旁".to_string()]);
    }

    #[test]
    fn table_in_body_level_ac_single_copy() {
        // 语料零样本形态（防御面钉定）：body 级 AC 内表格 Choice+Fallback
        // 双写 → 过滤后单表。
        let tbl = |t: &str| {
            format!("<w:tbl><w:tr><w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>")
        };
        let bytes = build_docx(&format!(
            "<mc:AlternateContent><mc:Choice Requires=\"w14\">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>",
            tbl("表选"),
            tbl("表兜")
        ));
        let doc = read(&select_mce(&bytes, &mut Vec::new()).unwrap());
        let tables: Vec<String> = doc
            .blocks
            .iter()
            .filter_map(|b| match b {
                crate::docx::Block::Table(t) => Some(t.rows.concat().join("|")),
                _ => None,
            })
            .collect();
        assert_eq!(tables, vec!["表选".to_string()], "AC 表格单份：{tables:?}");
    }

    #[test]
    fn real_corpus_mc_block_dedup() {
        // 真实语料回归锚（tracked——缺席即红，#262 纪律）。
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-artifacts/health-check/corpus/docx4j/mc-fun_mc-block.docx");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {e}"));
        // 缺陷面（原始 XML）：单 AC 10 Choice + 1 Fallback = 11 份段落。
        {
            use std::io::Read as _;
            let mut za = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            let mut xml = String::new();
            za.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            let ac = xml.match_indices("<mc:AlternateContent").count();
            assert_eq!(ac, 1, "单 AC 在场");
            let paras = xml.match_indices("<w:p>").count() + xml.match_indices("<w:p/>").count();
            assert_eq!(paras, 13, "原始 13 段（2 直 + 11 AC）");
        }
        // 读面（含本过滤）：11 份重复 → 3 段（2 直 + 首 Choice 单份）。
        let mut warnings = Vec::new();
        let doc = read(&select_mce(&bytes, &mut warnings).unwrap());
        let after = top_paras(&doc);
        assert_eq!(
            after.len(),
            3,
            "修复后 3 段（2 直段 + 首选 w14）: {after:?}"
        );
        assert!(after.contains(&"w14".to_string()), "首 Choice 选中");
        assert!(!after.contains(&"fallback".to_string()), "Fallback 剥除");
        assert!(!after.contains(&"w15".to_string()), "重复 Choice 剥除");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn real_corpus_txbx_double_write_unchanged_count() {
        // Word 2010 wps 双写形态：DrawingML 主选（模型面）+ VML Fallback
        // （第二趟）——修复后文本框计数不变（防双取从特判变天然）。
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-artifacts/health-check/corpus/docx4j/mc-fun_mc-txbx-block.docx");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture missing: {e}"));
        let before = read(&bytes);
        let before_tb = before
            .blocks
            .iter()
            .filter(|b| matches!(b, crate::docx::Block::TextBox(_)))
            .count();
        let after = read(&select_mce(&bytes, &mut Vec::new()).unwrap());
        let after_tb = after
            .blocks
            .iter()
            .filter(|b| matches!(b, crate::docx::Block::TextBox(_)))
            .count();
        assert_eq!(before_tb, after_tb, "文本框计数修复前后不变");
        assert!(after_tb >= 1, "DrawingML 主选仍在场");
    }
}
