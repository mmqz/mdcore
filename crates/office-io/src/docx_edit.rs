//! DOCX 外科手术式文本编辑（#79 T1——[`DocxEditor`]）。
//!
//! ## 定位（与既有两面的边界）
//!
//! - **不做块模型重建**：[`crate::docx`] 的模型级读（docx-rs 引擎）+
//!   写回会丢样式（xlsx FIDELITY v2 §4 定谳：组合语义 = 重建非修改，
//!   样式/元数据必丢）。本模块走**字节级外科手术**：解压 → 定位
//!   `word/document.xml` 段落（`w:p`）→ 只改目标段落的文本节点
//!   （`w:t`）→ 其余 XML 原样保留 → 重打包（非目标部件
//!   `raw_copy_file` 逐字节透传——anonymize_docx_props /
//!   mark_table_header_rows 同构先例）。
//! - **编辑范围 = document.xml 段落**（body/表格内/文本框内，文档序
//!   扁平索引）；页眉页脚/脚注/批注等其余部件不进编辑面（原样保留
//!   = 该面保真，见 docs/docx-fidelity-matrix.md）。
//!
//! ## 七坑映射（#90 任务书，逐坑锚点）
//!
//! | 坑 | 处理 | 锚点 |
//! |----|------|------|
//! | 一段多 run（格式不同） | 保留首个**含文本** run 的 `w:rPr` 原文；其余文本 run 合并（其 `w:t` 移除） | [`process_run`] |
//! | run 内特殊节点（w:br/w:tab/w:drawing） | 不删：非文本子节点原样保留（整 run 清空后才连 run 一起移除） | [`process_run`] 空壳判定 |
//! | 域代码段（w:fldChar/w:instrText/w:fldSimple） | 拒改 typed 错（读不受影响——缓存结果文本可读） | [`ParagraphRef::field_code`] |
//! | 修订标记（w:ins/w:del） | 按配置：[`RevisionPolicy::Refuse`]（默认，拒改）/ [`RevisionPolicy::Edit`]（ins 参与、del 原样） | [`DocxEditor::set_paragraph_text`] |
//! | 超链接内文字（w:hyperlink 包 w:r） | 首个文本位若在 hyperlink 内 → 新文字落在 hyperlink 里（属性原样）；后位 hyperlink 清空则整壳移除 | [`process_container`] |
//! | 空白保留 | 重写 `w:t` 恒带 `xml:space="preserve"`（永不吃首尾空白） | [`emit_text_payload`] |
//! | XML 转义 | `&`/`<`/`>` 文本实体转义（`&amp;`/`&lt;`/`&gt;`）；原文实体读侧反转义 | [`escape_text`] |
//!
//! ## 文本约定（与 [`crate::docx`] 模型级读面同构）
//!
//! [`DocxEditor::paragraph_text`]：`w:t` 按文档序拼接；`w:tab`→`\t`、
//! `w:br`→`\n`、`w:cr`→`\r`；`w:delText`（已删除内容）不计。
//! [`DocxEditor::set_paragraph_text`] 反向：新文本中 `\t`→`<w:tab/>`、
//! `\r\n`/`\r`/`\n`→`<w:br/>`（CRLF 合一为一次换行）；其余控制字符
//! （XML 1.0 非法面）与 U+FFFE/U+FFFF 非字符 typed 拒。
//!
//! ## 落账模型（save 时一次性落账）
//!
//! 编辑存**待落账态**（`pending`），`doc_xml`/`paragraphs` 恒为 open()
//! 时的原文坐标系——`paragraphs()` 的 span 不随编辑漂移；同一 paragraph
//! 后写胜。`save_to_vec` 单趟拼接（编辑段升序、互不重叠——XML 良构
//! 保证），对未编辑区域零接触。
//!
//! ## 容器防线（与读路径同门，恒严格）
//!
//! 门 0（`LIMIT_INPUT_BYTES_DOCX` 输入上限）→ 门 1（`prescan_docx`
//! 声明值预扫）→ 门 2+3（`verify_gates` 尺寸诚实 + 部件良构——**编辑
//! 面不放松**：结构畸形部件拒绝编辑，不做兼容降级）→ `word/document.xml`
//! 在场 + UTF-8（fail-closed）。扫描/树解析沿用 quick-xml 0.41
//! `buffer_position` 补偿口径（起点前向修 `'<'`、终点前向修 `'>'`
//! ——scan_vml_textbox_regions 同款，XML 属性值不含裸 `'<'` 假设同源）。

use std::io::Read as _;

use crate::docx::DocxError;
use crate::io;
use quick_xml::events::Event;

// ---------------------------------------------------------------------------
// 公开类型
// ---------------------------------------------------------------------------

/// 修订策略（坑④「按配置处理」的配置面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionPolicy {
    /// 段落含修订标记（`w:ins`/`w:del`/`moveFrom`/`moveTo`）→
    /// `set_paragraph_text` typed 拒（默认——宁缺毋错）。
    Refuse,
    /// 放行编辑：`w:ins`/`moveTo` 内容（当前生效文本）参与首个文本位
    /// 竞争；`w:del`/`moveFrom`（`w:delText`，已删除内容）原样保留。
    Edit,
}

/// 段落索引条目（原文坐标系——span 指向 open() 时的 document.xml 原文，
/// 不随编辑漂移；编辑为待落账态，经 [`DocxEditor::paragraph_text`] 读取）。
#[derive(Debug, Clone)]
pub struct ParagraphRef {
    /// `<w:p …>`…`</w:p>` 字节域（含开闭标签；自闭合空段为 `<w:p/>` 全域）。
    pub span: (usize, usize),
    /// 域代码在场（`w:fldChar`/`w:instrText`/`w:fldSimple`）——set 拒改。
    pub field_code: bool,
    /// 修订标记在场（`w:ins`/`w:del`/`moveFrom`/`moveTo`）——Refuse 拒改。
    pub revision: bool,
    /// mc:Fallback 复制区（旧版消费者拷贝；单独编辑会造成双分支不一致）
    /// ——set 拒改（编辑对应 mc:Choice 分支条目）。
    pub fallback_copy: bool,
    /// 表格内段落（`w:tbl` 开启态中——含嵌套表）。
    pub in_table: bool,
    /// 文本框内段落（`w:txbxContent` 开启态中——DrawingML/VML 同形）。
    pub in_textbox: bool,
    /// 原始装配文本（私有——经 [`DocxEditor::paragraph_text`] 读）。
    text: String,
}

/// 外科手术式 docx 编辑器（#79 T1）。
///
/// ```ignore
/// let bytes = std::fs::read("a.docx")?;
/// let mut ed = DocxEditor::open(&bytes)?;
/// ed.set_paragraph_text(3, "新文字")?;
/// std::fs::write("a.docx", ed.save_to_vec()?)?;
/// ```
#[derive(Debug)]
pub struct DocxEditor {
    /// 原始字节（open 时的输入——save 重打包的原料，恒不变）。
    bytes: Vec<u8>,
    /// document.xml 原文（UTF-8；原文坐标系锚）。
    doc_xml: String,
    /// 段落索引（文档序扁平——body/表格内/文本框内统一计数）。
    paragraphs: Vec<ParagraphRef>,
    /// 待落账编辑（与 paragraphs 等长；None = 未编辑）。
    pending: Vec<Option<String>>,
    /// 修订策略。
    policy: RevisionPolicy,
}

impl DocxEditor {
    /// 打开 docx（默认 [`RevisionPolicy::Refuse`]）。
    pub fn open(bytes: &[u8]) -> Result<Self, DocxError> {
        Self::open_with_policy(bytes, RevisionPolicy::Refuse)
    }

    /// 打开 docx（指定修订策略）。
    pub fn open_with_policy(bytes: &[u8], policy: RevisionPolicy) -> Result<Self, DocxError> {
        if bytes.len() as u64 > io::LIMIT_INPUT_BYTES_DOCX {
            return Err(DocxError::Container(io::ReadError::PrescanRejected {
                reason: "input size over limit",
                entry: None,
            }));
        }
        let cursor = std::io::Cursor::new(bytes);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
        io::prescan_docx(&mut za)?;
        let mut warnings = Vec::new();
        io::verify_gates(&mut za, true, &mut warnings)?;
        drop(za);

        let cursor = std::io::Cursor::new(bytes);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
        let mut xml = String::new();
        match za.by_name("word/document.xml") {
            Ok(mut zf) => {
                zf.read_to_string(&mut xml).map_err(|e| {
                    DocxError::Container(io::ReadError::InvalidPart {
                        part: "word/document.xml".to_string(),
                        detail: format!("read/UTF-8 failed: {e}"),
                    })
                })?;
            }
            Err(_) => {
                return Err(DocxError::Container(io::ReadError::InvalidPart {
                    part: "word/document.xml".to_string(),
                    detail: "part missing (fail-closed)".to_string(),
                }));
            }
        }
        let paragraphs = scan_paragraphs(&xml)?;
        let n = paragraphs.len();
        Ok(Self {
            bytes: bytes.to_vec(),
            doc_xml: xml,
            paragraphs,
            pending: vec![None; n],
            policy,
        })
    }

    /// 段落索引（原文坐标系）。
    pub fn paragraphs(&self) -> &[ParagraphRef] {
        &self.paragraphs
    }

    /// 段落当前文本（编辑待落账值优先；原文装配值兜底）。
    ///
    /// 越界 = panic（切片索引语义）。
    pub fn paragraph_text(&self, idx: usize) -> &str {
        assert!(
            idx < self.paragraphs.len(),
            "paragraph_text: index {idx} out of range"
        );
        if let Some(t) = self.pending[idx].as_deref() {
            t
        } else {
            &self.paragraphs[idx].text
        }
    }

    /// 设置段落文本（待落账；同段后写胜）。
    ///
    /// 拒改面（typed `DocxError::InvalidInput`，fail-closed）：
    /// 索引越界 / 域代码段 /（Refuse 策略）修订段 / Fallback 复制区段 /
    /// 新文本含 XML 1.0 非法控制字符或非字符。
    pub fn set_paragraph_text(&mut self, idx: usize, text: &str) -> Result<(), DocxError> {
        let n = self.paragraphs.len();
        let Some(p) = self.paragraphs.get(idx) else {
            return Err(DocxError::InvalidInput(format!(
                "paragraph {idx} out of range ({n} paragraphs)"
            )));
        };
        if p.fallback_copy {
            return Err(DocxError::InvalidInput(format!(
                "paragraph {idx}: mc:Fallback copy region refuses edit (edit the mc:Choice branch entry instead)"
            )));
        }
        if p.field_code {
            return Err(DocxError::InvalidInput(format!(
                "paragraph {idx}: field code present (w:fldChar/w:instrText/w:fldSimple) refuses edit"
            )));
        }
        if p.revision && self.policy == RevisionPolicy::Refuse {
            return Err(DocxError::InvalidInput(format!(
                "paragraph {idx}: revision markup present (w:ins/w:del) refuses edit under RevisionPolicy::Refuse (open_with_policy(Edit) allows: ins participates, del kept verbatim)"
            )));
        }
        // 嵌套段互斥：外层段与文本框内层段同时待编辑 = 嵌套 span，
        // 单趟拼接模型不支持（v1 显式拒绝——宁缺毋错）。
        for (j, pj) in self.paragraphs.iter().enumerate() {
            if j != idx && self.pending[j].is_some() && p.span.0 < pj.span.1 && pj.span.0 < p.span.1
            {
                return Err(DocxError::InvalidInput(format!(
                    "paragraph {idx}: nested edit with paragraph {j} (outer/textbox-inner chain) not supported — one per nesting chain"
                )));
            }
        }
        check_edit_text(idx, text)?;
        self.pending[idx] = Some(text.to_string());
        Ok(())
    }

    /// 当前修订策略。
    pub fn revision_policy(&self) -> RevisionPolicy {
        self.policy
    }

    /// 保存（一次性落账全部待编辑；幂等——同状态重复调用产出相同字节）。
    pub fn save_to_vec(&self) -> Result<Vec<u8>, DocxError> {
        let new_xml = apply_pending_edits(&self.doc_xml, &self.paragraphs, &self.pending)?;
        repack_document_part(&self.bytes, new_xml.as_bytes())
    }
}

// ---------------------------------------------------------------------------
// 位置补偿（quick-xml 0.41 buffer_position 边界口径——
// scan_vml_textbox_regions 同款：起点前向修 '<'，终点前向修 '>'；
// 闭标签起点由终点回扫（闭标签与前一事件尾之间只有空白）。
// ---------------------------------------------------------------------------

fn fwd_to_lt(bytes: &[u8], from: usize) -> usize {
    let mut i = from.min(bytes.len());
    while i < bytes.len() && bytes[i] != b'<' {
        i += 1;
    }
    i
}

fn fwd_to_gt(bytes: &[u8], from: usize) -> usize {
    let mut i = from.min(bytes.len());
    if i > 0 && bytes[i - 1] == b'>' {
        return i;
    }
    while i < bytes.len() && bytes[i] != b'>' {
        i += 1;
    }
    if i < bytes.len() {
        i += 1;
    }
    i
}

/// 闭标签 `'<'` 位置：由（含端点）终点回扫。
fn back_to_lt(bytes: &[u8], end_after: usize) -> usize {
    let mut i = end_after.min(bytes.len());
    while i > 0 {
        if bytes[i - 1] == b'<' {
            return i - 1;
        }
        i -= 1;
    }
    0
}

// ---------------------------------------------------------------------------
// 段落索引扫描（document.xml 单趟；门 2+3 已保证良构）
// ---------------------------------------------------------------------------

fn local_name(e: &quick_xml::events::BytesStart<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn scan_paragraphs(xml: &str) -> Result<Vec<ParagraphRef>, DocxError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let bytes = xml.as_bytes();
    let mut out: Vec<ParagraphRef> = Vec::new();
    // 开启中的段落（内层最尾——文本/旗标只记 innermost；嵌套文本框段落
    // 各自独立索引，外层不吃内层文本）。
    let mut para_stack: Vec<usize> = Vec::new();
    // 开启中 w:t 的 open 端（内层段落的待装配文本；w:t 恒为叶——单槽）。
    let mut wt_open: Option<usize> = None;
    let mut fallback = 0usize;
    let mut tbl = 0usize;
    let mut txbx = 0usize;
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "Fallback" => fallback += 1,
                    "tbl" => tbl += 1,
                    "txbxContent" => txbx += 1,
                    "p" => {
                        let start = fwd_to_lt(bytes, pos_before);
                        out.push(ParagraphRef {
                            span: (start, usize::MAX),
                            field_code: false,
                            revision: false,
                            fallback_copy: fallback > 0,
                            in_table: tbl > 0,
                            in_textbox: txbx > 0,
                            text: String::new(),
                        });
                        para_stack.push(out.len() - 1);
                    }
                    "t" if !para_stack.is_empty() => {
                        // 文本节点开：记 open 端（内容由配对 End 补齐——
                        // 需要闭标签起点定位内容区）。
                        wt_open = Some(fwd_to_gt(bytes, reader.buffer_position() as usize));
                    }
                    "ins" | "del" | "moveFrom" | "moveTo" if !para_stack.is_empty() => {
                        if let Some(&i) = para_stack.last() {
                            out[i].revision = true;
                        }
                    }
                    "fldChar" | "instrText" | "fldSimple" if !para_stack.is_empty() => {
                        if let Some(&i) = para_stack.last() {
                            out[i].field_code = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "p" => {
                        // 自闭合空段：完整段（无子事件），即刻落账。
                        let start = fwd_to_lt(bytes, pos_before);
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        out.push(ParagraphRef {
                            span: (start, end),
                            field_code: false,
                            revision: false,
                            fallback_copy: fallback > 0,
                            in_table: tbl > 0,
                            in_textbox: txbx > 0,
                            text: String::new(),
                        });
                    }
                    "tab" if !para_stack.is_empty() => {
                        if let Some(&i) = para_stack.last() {
                            out[i].text.push('\t');
                        }
                    }
                    "br" if !para_stack.is_empty() => {
                        if let Some(&i) = para_stack.last() {
                            out[i].text.push('\n');
                        }
                    }
                    "cr" if !para_stack.is_empty() => {
                        if let Some(&i) = para_stack.last() {
                            out[i].text.push('\r');
                        }
                    }
                    "t" if !para_stack.is_empty() => {
                        // 空文本节点（自闭合 <w:t/>）：零内容，无待配对。
                    }
                    "fldChar" | "instrText" | "fldSimple" if !para_stack.is_empty() => {
                        if let Some(&i) = para_stack.last() {
                            out[i].field_code = true;
                        }
                    }
                    "ins" | "del" | "moveFrom" | "moveTo" if !para_stack.is_empty() => {
                        if let Some(&i) = para_stack.last() {
                            out[i].revision = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name_end(&e);
                match name.as_str() {
                    "Fallback" => fallback = fallback.saturating_sub(1),
                    "tbl" => tbl = tbl.saturating_sub(1),
                    "txbxContent" => txbx = txbx.saturating_sub(1),
                    "p" => {
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        if let Some(i) = para_stack.pop() {
                            out[i].span.1 = end;
                        }
                    }
                    "t" => {
                        // 文本节点闭：内容区 = [wt_open, 闭标签起点)。
                        if let (Some(&i), Some(open)) = (para_stack.last(), wt_open.take()) {
                            let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                            let close_start = back_to_lt(bytes, end);
                            if close_start >= open {
                                let raw = &xml[open..close_start];
                                let decoded = quick_xml::escape::unescape(raw).map_err(|e| {
                                    DocxError::Container(io::ReadError::InvalidPart {
                                        part: "word/document.xml".to_string(),
                                        detail: format!("text entity decode failed: {e}"),
                                    })
                                })?;
                                out[i].text.push_str(decoded.as_ref());
                            }
                        }
                    }
                    "br" if !para_stack.is_empty() => {
                        // 开形 <w:br …></w:br>：与自闭合同语义。
                        if let Some(&i) = para_stack.last() {
                            out[i].text.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(DocxError::Container(io::ReadError::InvalidPart {
                    part: "word/document.xml".to_string(),
                    detail: format!("paragraph scan failed: {e}"),
                }));
            }
        }
    }
    Ok(out)
}

fn local_name_end(e: &quick_xml::events::BytesEnd<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

// ---------------------------------------------------------------------------
// 编辑文本防线（控制字符/非字符——XML 1.0 面；\t\n\r 合法放行）
// ---------------------------------------------------------------------------

fn check_edit_text(idx: usize, text: &str) -> Result<(), DocxError> {
    for c in text.chars() {
        let cp = c as u32;
        if matches!(c, '\u{FFFE}' | '\u{FFFF}') {
            return Err(DocxError::InvalidInput(format!(
                "paragraph {idx}: new text contains XML noncharacter U+{cp:04X}"
            )));
        }
        if cp < 0x20 && !matches!(c, '\t' | '\n' | '\r') {
            return Err(DocxError::InvalidInput(format!(
                "paragraph {idx}: new text contains control character U+{cp:04X} (XML 1.0 illegal; allowed: \\t \\n \\r as tab/br)"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 元素树（save 时对单个编辑段落重解析——小域重扫，不做全文树）
// ---------------------------------------------------------------------------

struct ElNode {
    name: String,
    /// '<' of open tag.
    start: usize,
    /// after '>' of open tag（自闭合 = 元素终点）。
    open_end: usize,
    /// '<' of close tag（自闭合 = 元素终点）。
    close_start: usize,
    /// after '>'（自闭合或闭标签）。
    end: usize,
    self_closing: bool,
    children: Vec<ElNode>,
}

/// 解析 `xml`（段落切片）首个元素为树（位置 = 切片内坐标 + `base`）。
fn parse_element(xml: &str, base: usize) -> Result<ElNode, DocxError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let bytes = xml.as_bytes();
    let mut stack: Vec<ElNode> = Vec::new();
    let mut root: Option<ElNode> = None;
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if stack.len() > io::LIMIT_XML_DEPTH {
                    return Err(DocxError::Container(io::ReadError::InvalidPart {
                        part: "word/document.xml".to_string(),
                        detail: "paragraph tree depth over limit".to_string(),
                    }));
                }
                stack.push(ElNode {
                    name: local_name(&e),
                    start: base + fwd_to_lt(bytes, pos_before),
                    open_end: base + fwd_to_gt(bytes, reader.buffer_position() as usize),
                    close_start: 0,
                    end: 0,
                    self_closing: false,
                    children: Vec::new(),
                });
            }
            Ok(Event::Empty(e)) => {
                let start = base + fwd_to_lt(bytes, pos_before);
                let end = base + fwd_to_gt(bytes, reader.buffer_position() as usize);
                let node = ElNode {
                    name: local_name(&e),
                    start,
                    open_end: end,
                    close_start: end,
                    end,
                    self_closing: true,
                    children: Vec::new(),
                };
                attach(node, &mut stack, &mut root);
            }
            Ok(Event::End(_)) => {
                let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                let Some(mut node) = stack.pop() else {
                    return Err(DocxError::Container(io::ReadError::InvalidPart {
                        part: "word/document.xml".to_string(),
                        detail: "paragraph tree: unbalanced end".to_string(),
                    }));
                };
                node.end = base + end;
                node.close_start = base + back_to_lt(bytes, end);
                attach(node, &mut stack, &mut root);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(DocxError::Container(io::ReadError::InvalidPart {
                    part: "word/document.xml".to_string(),
                    detail: format!("paragraph tree parse failed: {e}"),
                }));
            }
        }
        if root.is_some() {
            break;
        }
    }
    root.ok_or_else(|| {
        DocxError::Container(io::ReadError::InvalidPart {
            part: "word/document.xml".to_string(),
            detail: "paragraph tree: no root element".to_string(),
        })
    })
}

fn attach(node: ElNode, stack: &mut [ElNode], root: &mut Option<ElNode>) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        *root = Some(node);
    }
}

// ---------------------------------------------------------------------------
// 手术（ops 列表 + 单趟拼接）
// ---------------------------------------------------------------------------

enum Op {
    Remove,
    Replace(String),
    /// 零宽插入（同位多插按提交序拼接）。
    Insert(String),
}

struct OpRec {
    start: usize,
    end: usize,
    op: Op,
}

struct SurgeryCtx<'a> {
    first_done: bool,
    new_text: &'a str,
}

/// 容器（hyperlink/ins/moveTo/sdt/smartTag/dir）子层递归；Top 层 = w:p 子层。
fn process_children(children: &[ElNode], ctx: &mut SurgeryCtx<'_>, ops: &mut Vec<OpRec>) {
    for child in children {
        match child.name.as_str() {
            "r" => process_run(child, ctx, ops),
            "hyperlink" | "ins" | "moveTo" | "sdt" | "smartTag" | "dir" => {
                let before = ops.len();
                process_children(&child.children, ctx, ops);
                // 壳清空判定：全部子元素整域移除（首文本 run 所在壳不可能
                // 全空——Replace 使其保有子层）→ 整壳一并移除。
                if !child.children.is_empty()
                    && child.children.iter().all(|g| {
                        ops[before..].iter().any(|o| {
                            matches!(o.op, Op::Remove) && o.start == g.start && o.end == g.end
                        })
                    })
                {
                    ops.truncate(before);
                    ops.push(OpRec {
                        start: child.start,
                        end: child.end,
                        op: Op::Remove,
                    });
                }
            }
            // pPr/书签/proofErr/del(moveFrom) 等其余面：原样（零 op）。
            _ => {}
        }
    }
}

fn process_run(run: &ElNode, ctx: &mut SurgeryCtx<'_>, ops: &mut Vec<OpRec>) {
    // run 子层直接子节点（run 不嵌套容器）。
    let ts: Vec<&ElNode> = run.children.iter().filter(|c| c.name == "t").collect();
    if ts.is_empty() {
        return; // 非文本 run（br/tab/drawing-only/rPr-only）——原样。
    }
    if !ctx.first_done {
        let first = ts[0];
        ops.push(OpRec {
            start: first.start,
            end: first.end,
            op: Op::Replace(emit_text_payload(ctx.new_text)),
        });
        for extra in &ts[1..] {
            ops.push(OpRec {
                start: extra.start,
                end: extra.end,
                op: Op::Remove,
            });
        }
        ctx.first_done = true;
    } else {
        // 后位文本 run：w:t 全移；run 壳仅在残留非 rPr 子节点时保留。
        let remains = run
            .children
            .iter()
            .any(|c| c.name != "rPr" && c.name != "t");
        if remains {
            for t in &ts {
                ops.push(OpRec {
                    start: t.start,
                    end: t.end,
                    op: Op::Remove,
                });
            }
        } else {
            ops.push(OpRec {
                start: run.start,
                end: run.end,
                op: Op::Remove,
            });
        }
    }
}

/// 新文本发射（坑⑥/⑦：恒 `xml:space="preserve"`；实体转义；
/// `\t`→`<w:tab/>`、`\r\n`/`\r`/`\n`→`<w:br/>`；末尾断行后不再补空段）。
fn emit_text_payload(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    loop {
        let hit = rest
            .char_indices()
            .find(|(_, c)| matches!(c, '\t' | '\n' | '\r'));
        let (seg_len, brk) = match hit {
            Some((i, c)) => (i, Some(c)),
            None => (rest.len(), None),
        };
        out.push_str("<w:t xml:space=\"preserve\">");
        out.push_str(&escape_text(&rest[..seg_len]));
        out.push_str("</w:t>");
        match brk {
            None => break,
            Some('\t') => {
                out.push_str("<w:tab/>");
                rest = &rest[seg_len + 1..];
            }
            Some('\r') => {
                out.push_str("<w:br/>");
                let adv = if rest[seg_len + 1..].starts_with('\n') {
                    2
                } else {
                    1
                };
                rest = &rest[seg_len + adv..];
            }
            Some('\n') => {
                out.push_str("<w:br/>");
                rest = &rest[seg_len + 1..];
            }
            Some(_) => unreachable!(),
        }
        if rest.is_empty() {
            break; // 断行收尾：不再补空 <w:t>
        }
    }
    out
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 全部待落账编辑 → 新 document.xml（单趟；未编辑区域零接触）。
fn apply_pending_edits(
    doc_xml: &str,
    paragraphs: &[ParagraphRef],
    pending: &[Option<String>],
) -> Result<String, DocxError> {
    let mut out = String::with_capacity(doc_xml.len() + 64);
    let mut cursor = 0usize;
    for (idx, new_text) in pending.iter().enumerate() {
        let Some(new_text) = new_text.as_deref() else {
            continue;
        };
        let p = &paragraphs[idx];
        if p.span.1 <= p.span.0 || p.span.1 == usize::MAX {
            return Err(DocxError::Container(io::ReadError::InvalidPart {
                part: "word/document.xml".to_string(),
                detail: format!("paragraph {idx} span invalid ({:?})", p.span),
            }));
        }
        if p.span.0 < cursor {
            return Err(DocxError::Container(io::ReadError::InvalidPart {
                part: "word/document.xml".to_string(),
                detail: format!(
                    "paragraph {idx} span overlaps prior edit (ordering invariant broken)"
                ),
            }));
        }
        let node = parse_element(&doc_xml[p.span.0..p.span.1], p.span.0)?;
        let mut ctx = SurgeryCtx {
            first_done: false,
            new_text,
        };
        let mut ops: Vec<OpRec> = Vec::new();
        process_children(&node.children, &mut ctx, &mut ops);
        if !ctx.first_done {
            // 无文本段落：新 run 插在 pPr 之后（无 pPr = children 区首）。
            let insert_at = node
                .children
                .iter()
                .find(|c| c.name == "pPr")
                .map(|ppr| ppr.end)
                .unwrap_or(node.open_end);
            ops.push(OpRec {
                start: insert_at,
                end: insert_at,
                op: Op::Insert(format!("<w:r>{}</w:r>", emit_text_payload(new_text))),
            });
        }
        if node.self_closing {
            // `<w:p …/>` → 开形：`/>` 换 `>`（换行段需承载子层）。
            ops.push(OpRec {
                start: node.end - 2,
                end: node.end,
                op: Op::Replace(">".to_string()),
            });
            ops.push(OpRec {
                start: node.end,
                end: node.end,
                op: Op::Insert("</w:p>".to_string()),
            });
        }
        out.push_str(&doc_xml[cursor..p.span.0]);
        out.push_str(&splice(&doc_xml[p.span.0..p.span.1], p.span.0, ops)?);
        cursor = p.span.1;
    }
    out.push_str(&doc_xml[cursor..]);
    Ok(out)
}

/// ops 单趟拼接（排序稳定——同位保提交序；重叠 = 内部不变量破坏，
/// fail-closed）。
fn splice(slice: &str, base: usize, mut ops: Vec<OpRec>) -> Result<String, DocxError> {
    ops.sort_by_key(|o| (o.start, o.end)); // 稳定排序
    let mut out = String::with_capacity(slice.len() + 64);
    let mut cursor = 0usize; // slice 内坐标
    let limit = slice.len();
    let mut i = 0usize;
    while i < ops.len() {
        let op = &ops[i];
        let s = op.start.saturating_sub(base).min(limit);
        let e = op.end.saturating_sub(base).min(limit);
        if s < cursor || e < s {
            return Err(DocxError::Container(io::ReadError::InvalidPart {
                part: "word/document.xml".to_string(),
                detail: "surgery ops overlap (internal invariant broken)".to_string(),
            }));
        }
        out.push_str(&slice[cursor..s]);
        match &op.op {
            Op::Remove => {}
            Op::Replace(t) => out.push_str(t),
            Op::Insert(t) => out.push_str(t),
        }
        cursor = e.max(cursor);
        i += 1;
    }
    out.push_str(&slice[cursor..]);
    Ok(out)
}

// ---------------------------------------------------------------------------
// 重打包（非目标部件 raw_copy 逐字节透传——anonymize/mark_table_header
// 同构先例；word/document.xml 在场 fail-closed）
// ---------------------------------------------------------------------------

fn repack_document_part(bytes: &[u8], document_xml: &[u8]) -> Result<Vec<u8>, DocxError> {
    use std::io::Write as _;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| DocxError::Io(format!("open zip: {e}")))?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut has_document = false;
    for i in 0..archive.len() {
        let name = {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| DocxError::Io(format!("zip index: {e}")))?;
            entry.name().to_string()
        };
        if name == "word/document.xml" {
            has_document = true;
            writer
                .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                .map_err(|e| DocxError::Io(format!("zip start: {e}")))?;
            writer
                .write_all(document_xml)
                .map_err(|e| DocxError::Io(format!("zip write: {e}")))?;
        } else {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| DocxError::Io(format!("zip index: {e}")))?;
            writer
                .raw_copy_file(entry)
                .map_err(|e| DocxError::Io(format!("zip copy: {e}")))?;
        }
    }
    if !has_document {
        return Err(DocxError::Io(
            "docx_edit: word/document.xml 不在场（非 docx 伪产物——fail-closed 拒绝）".into(),
        ));
    }
    let out = writer
        .finish()
        .map_err(|e| DocxError::Io(format!("zip finish: {e}")))?;
    Ok(out.into_inner())
}

// ---------------------------------------------------------------------------
// 测试（7 坑每坑 ≥1 + 核心往返/保真/防线）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── 夹具基建 ───────────────────────────────────────────────

    const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    const DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId7\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.com/\" TargetMode=\"External\"/></Relationships>";

    fn document_wrap(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><w:body>{body}\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:body></w:document>"
        )
    }

    fn build_docx(body: &str) -> Vec<u8> {
        use std::io::Write as _;
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
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
        let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut e = ar.by_name(name).unwrap();
        let mut v = vec![];
        std::io::Read::read_to_end(&mut e, &mut v).unwrap();
        v
    }

    fn saved_document_xml(bytes: &[u8]) -> String {
        String::from_utf8(part_bytes(bytes, "word/document.xml")).unwrap()
    }

    // ── 坑①：一段多 run（格式不同）────────────────────────────

    #[test]
    fn pit1_multi_run_first_text_run_rpr_kept_rest_merged() {
        let body = "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>\
<w:r><w:rPr><w:b/></w:rPr><w:t>Hello </w:t></w:r>\
<w:r><w:rPr><w:i/></w:rPr><w:t>world</w:t></w:r>\
<w:r><w:rPr><w:u/></w:rPr><w:t>!</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraph_text(0), "Hello world!");
        ed.set_paragraph_text(0, "New caption").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        // 首个含文本 run：rPr(b) + pPr 原样，新文字单 w:t。
        assert!(
            xml.contains("<w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">New caption</w:t></w:r></w:p>"),
            "首 run 属性保留 + 其余合并（实际：{xml}）"
        );
        // 后位文本 run（仅 rPr + w:t，清空后无残留子节点）整 run 移除。
        assert!(!xml.contains("<w:i/>"), "斜体 run 已合并移除");
        assert!(!xml.contains("<w:u/>"), "下画线 run 已合并移除");
        assert!(!xml.contains("world"), "旧文本已移除");
        // 重开校验。
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.paragraph_text(0), "New caption");
    }

    // ── 坑②：run 内特殊节点（br/tab/drawing）──────────────────

    #[test]
    fn pit2_special_nodes_preserved_not_deleted() {
        let drawing = "<w:r><w:drawing><wp:inline><a:graphic/></wp:inline></w:drawing></w:r>";
        let body = format!(
            "<w:p><w:r><w:t>See </w:t></w:r>{drawing}<w:r><w:t>above</w:t></w:r></w:p>\
<w:p><w:r><w:t>A</w:t></w:r><w:r><w:tab/></w:r><w:r><w:br/></w:r><w:r><w:t>B</w:t></w:r></w:p>\
<w:p><w:r><w:t>X</w:t><w:br/><w:t>Y</w:t></w:r></w:p>"
        );
        let bytes = build_docx(&body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraph_text(0), "See above");
        assert_eq!(ed.paragraph_text(1), "A\t\nB");
        assert_eq!(ed.paragraph_text(2), "X\nY");
        ed.set_paragraph_text(0, "Caption").unwrap();
        ed.set_paragraph_text(1, "Plain").unwrap();
        ed.set_paragraph_text(2, "Mixed").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        // drawing run 原样保留（非文本 run——不删）。
        assert!(xml.contains(drawing), "drawing run 原样保留");
        // tab/br 独立 run 原样保留。
        assert!(xml.contains("<w:r><w:tab/></w:r>"), "tab run 保留");
        assert!(xml.contains("<w:r><w:br/></w:r>"), "br run 保留");
        // 同 run 混合子节点：首 w:t 换新文，次 w:t 移除，br 保留。
        assert!(
            xml.contains("<w:r><w:t xml:space=\"preserve\">Mixed</w:t><w:br/></w:r>"),
            "混合 run：新文 + br 保留、旧 Y 移除（实际：{xml}）"
        );
        assert!(!xml.contains(">X<"), "旧 X 文本已移除");
        let ed2 = DocxEditor::open(&saved).unwrap();
        // 坑②行为后果：保留的 tab/br 节点读回计入文本（新文字后随）。
        assert_eq!(ed2.paragraph_text(0), "Caption");
        assert_eq!(
            ed2.paragraph_text(1),
            "Plain\t\n",
            "tab/br run 保留 → 读回随新文字"
        );
        assert_eq!(
            ed2.paragraph_text(2),
            "Mixed\n",
            "同 run 内 br 保留 → 读回随新文字"
        );
    }

    // ── 坑③：域代码段拒改────────────────────────────────────

    #[test]
    fn pit3_field_code_refuses_edit() {
        let body = "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
<w:r><w:instrText> PAGE </w:instrText></w:r>\
<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>\
<w:r><w:t>3</w:t></w:r>\
<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>\
<w:p><w:fldSimple w:instr=\" DATE \"><w:r><w:t>2026</w:t></w:r></w:fldSimple></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert!(ed.paragraphs()[0].field_code, "fldChar 段标记");
        assert!(ed.paragraphs()[1].field_code, "fldSimple 段标记");
        // 读不受影响（缓存结果文本可读；指令文本不入正文）。
        assert_eq!(ed.paragraph_text(0), "3");
        assert_eq!(ed.paragraph_text(1), "2026");
        let e0 = ed.set_paragraph_text(0, "x").unwrap_err();
        assert!(matches!(e0, DocxError::InvalidInput(_)), "拒改 typed 错");
        assert!(
            format!("{e0}").contains("field code"),
            "消息含拒改原因：{e0}"
        );
        let e1 = ed.set_paragraph_text(1, "x").unwrap_err();
        assert!(format!("{e1}").contains("field code"));
        // 拒改后零产物副作用（save 与原文 document.xml 一致）。
        let saved = ed.save_to_vec().unwrap();
        assert_eq!(
            part_bytes(&saved, "word/document.xml"),
            part_bytes(&bytes, "word/document.xml"),
            "零编辑落账 = document.xml 原样"
        );
    }

    // ── 坑④：修订标记按配置──────────────────────────────────

    #[test]
    fn pit4_revision_policy_refuse_and_edit() {
        let body = "<w:p><w:ins w:id=\"1\"><w:r><w:t>Inserted</w:t></w:r></w:ins>\
<w:del w:id=\"2\"><w:r><w:delText>old</w:delText></w:r></w:del></w:p>";
        let bytes = build_docx(body);
        // 默认策略：拒改。
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert!(ed.paragraphs()[0].revision);
        assert!(!ed.paragraphs()[0].field_code);
        assert_eq!(
            ed.paragraph_text(0),
            "Inserted",
            "delText 不计（已删除内容）"
        );
        let err = ed.set_paragraph_text(0, "x").unwrap_err();
        assert!(
            format!("{err}").contains("revision markup"),
            "Refuse 拒改：{err}"
        );
        // Edit 策略：ins 参与首文本位，del 原样保留。
        let mut ed2 = DocxEditor::open_with_policy(&bytes, RevisionPolicy::Edit).unwrap();
        assert_eq!(ed2.revision_policy(), RevisionPolicy::Edit);
        ed2.set_paragraph_text(0, "Rewritten").unwrap();
        let saved = ed2.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains(
                "<w:ins w:id=\"1\"><w:r><w:t xml:space=\"preserve\">Rewritten</w:t></w:r></w:ins>"
            ),
            "新文字落在 ins 内（当前生效文本位——实际：{xml}）"
        );
        assert!(
            xml.contains("<w:del w:id=\"2\"><w:r><w:delText>old</w:delText></w:r></w:del>"),
            "del 块原样保留（已删除内容不动）"
        );
        let ed3 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed3.paragraph_text(0), "Rewritten");
    }

    // ── 坑⑤：超链接内文字────────────────────────────────────

    #[test]
    fn pit5_hyperlink_text_stays_inside_link() {
        let body = "<w:p><w:hyperlink r:id=\"rId7\"><w:r><w:rPr><w:rStyle w:val=\"Link\"/></w:rPr><w:t>Example</w:t></w:r><w:r><w:t> Site</w:t></w:r></w:hyperlink><w:r><w:t> tail</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraph_text(0), "Example Site tail");
        ed.set_paragraph_text(0, "New label").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        // 首文本位在 hyperlink 内 → 新文字留在 hyperlink 里（属性原样）。
        assert!(
            xml.contains("<w:hyperlink r:id=\"rId7\"><w:r><w:rPr><w:rStyle w:val=\"Link\"/></w:rPr><w:t xml:space=\"preserve\">New label</w:t></w:r></w:hyperlink>"),
            "hyperlink 属性保留 + 新文字入链（实际：{xml}）"
        );
        assert!(
            !xml.contains("> Site<") && !xml.contains("> tail<"),
            "后位文本已合并移除"
        );
        // rels 部件逐字节原样（raw_copy）。
        assert_eq!(
            part_bytes(&saved, "word/_rels/document.xml.rels"),
            part_bytes(&bytes, "word/_rels/document.xml.rels")
        );
    }

    #[test]
    fn pit5_hyperlink_emptied_shell_removed() {
        let body = "<w:p><w:r><w:t>Plain first</w:t></w:r><w:hyperlink r:id=\"rId7\"><w:r><w:t>Linked later</w:t></w:r></w:hyperlink></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        ed.set_paragraph_text(0, "X").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains("<w:p><w:r><w:t xml:space=\"preserve\">X</w:t></w:r></w:p>"),
            "首 run 承载新文；清空 hyperlink 整壳移除（实际：{xml}）"
        );
        assert!(!xml.contains("w:hyperlink"), "空壳 hyperlink 已移除");
        // 未用关系条目保留在 rels（raw_copy 原样——合法空置关系）。
        assert_eq!(
            part_bytes(&saved, "word/_rels/document.xml.rels"),
            part_bytes(&bytes, "word/_rels/document.xml.rels")
        );
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.paragraph_text(0), "X");
    }

    // ── 坑⑥：空白保留────────────────────────────────────────

    #[test]
    fn pit6_whitespace_preserved_with_xml_space() {
        let body = "<w:p><w:r><w:t>old</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        ed.set_paragraph_text(0, "  padded  ").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains("<w:t xml:space=\"preserve\">  padded  </w:t>"),
            "首尾空白配 preserve（实际：{xml}）"
        );
        // 无空白文本同样恒带 preserve（永不吃空白策略）。
        let mut ed2 = DocxEditor::open(&bytes).unwrap();
        ed2.set_paragraph_text(0, "plain").unwrap();
        let xml2 = saved_document_xml(&ed2.save_to_vec().unwrap());
        assert!(xml2.contains("<w:t xml:space=\"preserve\">plain</w:t>"));
        // 重开往返：首尾空白逐字还原（无 trim）。
        let ed3 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed3.paragraph_text(0), "  padded  ");
    }

    // ── 坑⑦：XML 转义────────────────────────────────────────

    #[test]
    fn pit7_xml_escaping_roundtrip() {
        let body = "<w:p><w:r><w:t>A &amp; B &lt;tag&gt;</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraph_text(0), "A & B <tag>", "原文实体读侧反转义");
        let mut ed2 = DocxEditor::open(&bytes).unwrap();
        ed2.set_paragraph_text(0, "a & b < c > d").unwrap();
        let saved = ed2.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains("<w:t xml:space=\"preserve\">a &amp; b &lt; c &gt; d</w:t>"),
            "写侧实体转义（实际：{xml}）"
        );
        let ed3 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed3.paragraph_text(0), "a & b < c > d", "重开反转义还原");
    }

    // ── 核心：往返 ×10 不膨胀 ────────────────────────────────

    #[test]
    fn core_roundtrip_ten_cycles_no_bloat() {
        let body = "<w:p><w:r><w:t>Intro</w:t></w:r></w:p>\
<w:p><w:r><w:t>Second</w:t></w:r></w:p>\
<w:p><w:r><w:t>Third</w:t></w:r></w:p>";
        let mut cur = build_docx(body);
        let mut sizes = vec![];
        for _round in 1..=10 {
            let mut ed = DocxEditor::open(&cur).unwrap();
            ed.set_paragraph_text(1, "第 1 轮编辑后的第二段文本")
                .unwrap();
            let saved = ed.save_to_vec().unwrap();
            sizes.push(saved.len());
            cur = saved;
        }
        let first = sizes[0] as f64;
        let last = *sizes.last().unwrap() as f64;
        assert!(
            last <= first * 1.01,
            "10 轮膨胀 <1%（实测 {first} → {last} B）"
        );
        assert_eq!(sizes[0], *sizes.last().unwrap(), "稳定态逐字节等长");
        let ed = DocxEditor::open(&cur).unwrap();
        assert_eq!(ed.paragraph_text(1), "第 1 轮编辑后的第二段文本");
        assert_eq!(ed.paragraph_text(0), "Intro");
        assert_eq!(ed.paragraph_text(2), "Third");
    }

    // ── 核心：未触及面字节级保真 ─────────────────────────────

    #[test]
    fn core_untouched_bytes_and_regions_identical() {
        let body = "<w:p><w:r><w:t>One</w:t></w:r></w:p>\
<w:p><w:r><w:t>Two</w:t></w:r></w:p>\
<w:p><w:r><w:t>Three</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let orig_doc = saved_document_xml(&bytes);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let spans: Vec<(usize, usize)> = ed.paragraphs().iter().map(|p| p.span).collect();
        ed.set_paragraph_text(1, "EDITED").unwrap();
        let saved = ed.save_to_vec().unwrap();
        // 未编辑区域 = 原文前缀 + 原文后缀（拼接恒等）；编辑段仅在中间。
        let new_doc = saved_document_xml(&saved);
        assert!(
            new_doc.starts_with(&orig_doc[..spans[1].0]),
            "编辑段前 document.xml 零接触"
        );
        assert!(
            new_doc.ends_with(&orig_doc[spans[1].1..]),
            "编辑段后 document.xml 零接触"
        );
        assert!(
            new_doc.contains("EDITED") && !new_doc.contains(">Two<"),
            "编辑段在位、旧文移除"
        );
        // 全部非 document 部件逐字节原样。
        let mut ar0 = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut ar1 = zip::ZipArchive::new(std::io::Cursor::new(&saved)).unwrap();
        assert_eq!(ar0.len(), ar1.len(), "部件数不变");
        for i in 0..ar0.len() {
            let n0 = ar0.by_index_raw(i).unwrap().name().to_string();
            let n1 = ar1.by_index_raw(i).unwrap().name().to_string();
            assert_eq!(n0, n1, "部件序不变（{i}）");
            if n0 == "word/document.xml" {
                continue;
            }
            assert_eq!(
                part_bytes(&bytes, &n0),
                part_bytes(&saved, &n1),
                "非目标部件 {n0} 逐字节原样（raw_copy）"
            );
        }
    }

    // ── 核心：无文本段插入 + 自闭合空段形态转换 ───────────────

    #[test]
    fn core_textless_paragraph_insert_and_selfclosing_form() {
        let body = "<w:p><w:pPr><w:jc w:val=\"left\"/></w:pPr><w:r><w:drawing/></w:r></w:p><w:p/>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraph_text(0), "", "图片段无文本");
        assert_eq!(ed.paragraph_text(1), "", "自闭合空段无文本");
        ed.set_paragraph_text(0, "Caption").unwrap();
        ed.set_paragraph_text(1, "Fill").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains("</w:pPr><w:r><w:t xml:space=\"preserve\">Caption</w:t></w:r><w:r><w:drawing/></w:r>"),
            "新 run 插在 pPr 后、drawing 前（实际：{xml}）"
        );
        assert!(
            xml.contains("<w:p><w:r><w:t xml:space=\"preserve\">Fill</w:t></w:r></w:p>"),
            "自闭合 `<w:p/>` 转开形承载子层（实际：{xml}）"
        );
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.paragraph_text(0), "Caption");
        assert_eq!(ed2.paragraph_text(1), "Fill");
    }

    // ── 核心：控制字符/换行/制表策略 ─────────────────────────

    #[test]
    fn core_control_chars_and_break_conventions() {
        let body = "<w:p><w:r><w:t>base</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let e1 = ed.set_paragraph_text(0, "a\u{000B}b").unwrap_err();
        assert!(format!("{e1}").contains("control character"), "{e1}");
        let e2 = ed.set_paragraph_text(0, "a\u{FFFE}b").unwrap_err();
        assert!(format!("{e2}").contains("noncharacter"), "{e2}");
        // 越界索引。
        let e3 = ed.set_paragraph_text(9, "x").unwrap_err();
        assert!(format!("{e3}").contains("out of range"), "{e3}");
        // \t → w:tab；\r\n/\r/\n → w:br（CRLF 合一）。
        ed.set_paragraph_text(0, "a\tb").unwrap();
        let xml = saved_document_xml(&ed.save_to_vec().unwrap());
        assert!(
            xml.contains(
                "<w:t xml:space=\"preserve\">a</w:t><w:tab/><w:t xml:space=\"preserve\">b</w:t>"
            ),
            "tab 转元素（实际：{xml}）"
        );
        let mut ed2 = DocxEditor::open(&bytes).unwrap();
        ed2.set_paragraph_text(0, "x\r\ny").unwrap();
        let xml2 = saved_document_xml(&ed2.save_to_vec().unwrap());
        assert!(
            xml2.contains(
                "<w:t xml:space=\"preserve\">x</w:t><w:br/><w:t xml:space=\"preserve\">y</w:t>"
            ),
            "CRLF 合一为一次 br（实际：{xml2}）"
        );
        let ed3 = DocxEditor::open(&ed2.save_to_vec().unwrap()).unwrap();
        assert_eq!(
            ed3.paragraph_text(0),
            "x\ny",
            "br 读回 \\n（模型级同构约定）"
        );
    }

    // ── 核心：表格内/文本框内段落 + 嵌套编辑互斥 ──────────────

    #[test]
    fn core_table_textbox_paragraphs_and_nesting_guard() {
        let body = "<w:p><w:r><w:t>Intro</w:t></w:r></w:p>\
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell A</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>Cell B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:p><w:r><w:drawing><wps:txbx><w:txbxContent><w:p><w:r><w:t>Box text</w:t></w:r></w:p></w:txbxContent></wps:txbx></w:drawing></w:r></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let ps = ed.paragraphs();
        assert_eq!(
            ps.len(),
            5,
            "文档序扁平索引：intro/两格/外层 drawing 段/文本框内段"
        );
        assert_eq!(ed.paragraph_text(0), "Intro");
        assert!(ps[1].in_table && ps[2].in_table, "表格内标记");
        assert_eq!(ed.paragraph_text(1), "Cell A");
        assert_eq!(ed.paragraph_text(2), "Cell B");
        assert_eq!(ed.paragraph_text(3), "", "外层段自身无文本（drawing-only）");
        assert!(ps[4].in_textbox, "文本框内标记");
        assert_eq!(ed.paragraph_text(4), "Box text");
        // 外层段文本不吃内层文本框文本（innermost 规则）。
        assert!(!ed.paragraph_text(3).contains("Box text"));
        // 嵌套链互斥：外层(3) 与文本框内层(4) 同时待编辑 = 拒绝。
        ed.set_paragraph_text(4, "New box").unwrap();
        let err = ed.set_paragraph_text(3, "outer").unwrap_err();
        assert!(
            format!("{err}").contains("nested edit"),
            "嵌套编辑互斥：{err}"
        );
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains("<w:txbxContent><w:p><w:r><w:t xml:space=\"preserve\">New box</w:t></w:r></w:p></w:txbxContent>"),
            "文本框内段编辑（实际：{xml}）"
        );
        // 外层段保持原样（编辑内层不动外层字节）。
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.paragraph_text(3), "");
        assert_eq!(ed2.paragraph_text(4), "New box");
    }

    // ── 核心：mc:Fallback 复制区拒改 ─────────────────────────

    #[test]
    fn core_fallback_copy_refuses() {
        let body = "<mc:AlternateContent><mc:Choice><w:p><w:r><w:t>Choice branch</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:t>Fallback copy</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraphs().len(), 2);
        assert!(!ed.paragraphs()[0].fallback_copy, "Choice 分支可编辑");
        assert!(ed.paragraphs()[1].fallback_copy, "Fallback 复制区标记");
        let err = ed.set_paragraph_text(1, "x").unwrap_err();
        assert!(format!("{err}").contains("mc:Fallback"), "{err}");
        ed.set_paragraph_text(0, "Edited choice").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.paragraph_text(0), "Edited choice");
        assert_eq!(ed2.paragraph_text(1), "Fallback copy", "Fallback 原样未动");
    }

    // ── 核心：容器防线（敌意输入 fail-closed） ────────────────

    #[test]
    fn core_container_gates_hostile_input() {
        // 非 zip。
        let e = DocxEditor::open(b"not a zip at all").unwrap_err();
        assert!(matches!(e, DocxError::Container(_)), "{e}");
        // 缺 document.xml。
        use std::io::Write as _;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file("x.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"<a/>").unwrap();
        let z = w.finish().unwrap().into_inner();
        let e2 = DocxEditor::open(&z).unwrap_err();
        assert!(
            format!("{e2}").contains("word/document.xml"),
            "缺件 fail-closed：{e2}"
        );
        // document.xml 非 UTF-8。
        let mut w2 = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w2.start_file(
            "[Content_Types].xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        w2.write_all(CONTENT_TYPES.as_bytes()).unwrap();
        w2.start_file("_rels/.rels", zip::write::SimpleFileOptions::default())
            .unwrap();
        w2.write_all(ROOT_RELS.as_bytes()).unwrap();
        w2.start_file(
            "word/_rels/document.xml.rels",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        w2.write_all(DOC_RELS.as_bytes()).unwrap();
        w2.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        w2.write_all(&[0x3C, 0x77, 0xFF, 0xFE, 0x3E]).unwrap(); // "<w\xff\xfe>" 非法 UTF-8
        let z2 = w2.finish().unwrap().into_inner();
        let e3 = DocxEditor::open(&z2).unwrap_err();
        // 非 UTF-8 字节在门 3（结构校验）先行拒绝（同字节面）——
        // open 的 read_to_string 是纵深防御；两路均为 Container 拒。
        assert!(
            matches!(e3, DocxError::Container(_)),
            "非 UTF-8 fail-closed：{e3}"
        );
    }

    // ── 核心：索引/文本约定 + 后写胜 ─────────────────────────

    #[test]
    fn core_index_text_conventions_and_last_write_wins() {
        let body = "<w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t></w:r></w:p>\
<w:p><w:r><w:t>line1</w:t><w:br/><w:t>line2</w:t></w:r></w:p>\
<w:p><w:r><w:t>c</w:t><w:cr/><w:t>d</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraph_text(0), "A\tB");
        assert_eq!(ed.paragraph_text(1), "line1\nline2");
        assert_eq!(ed.paragraph_text(2), "c\rd");
        let doc_len = ed.doc_xml.len();
        for p in ed.paragraphs() {
            assert!(p.span.0 < p.span.1, "span 非空");
            assert!(p.span.1 <= doc_len, "span 在 document.xml 域内");
        }
        // 同段后写胜。
        ed.set_paragraph_text(0, "first").unwrap();
        ed.set_paragraph_text(0, "second").unwrap();
        assert_eq!(ed.paragraph_text(0), "second");
        let saved = ed.save_to_vec().unwrap();
        // 同 run 内 tab 保留 → 读回随新文字（坑②行为后果）。
        assert_eq!(
            DocxEditor::open(&saved).unwrap().paragraph_text(0),
            "second\t"
        );
        // save 确定性（同状态两存逐字节相同）。
        let again = {
            let mut e2 = DocxEditor::open(&bytes).unwrap();
            e2.set_paragraph_text(0, "second").unwrap();
            e2.save_to_vec().unwrap()
        };
        assert_eq!(saved, again, "save 幂等");
    }
}
