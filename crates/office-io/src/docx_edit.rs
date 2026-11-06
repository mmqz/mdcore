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
//! | 域代码段（w:fldChar/w:instrText/w:fldSimple） | 整段替换拒改 typed 错（读不受影响——缓存结果文本可读）；#140 加性：显示值可编辑（只改结果区 w:t，五段结构零接触） | [`ParagraphRef::field_code`] / [`DocxEditor::set_field_value`] |
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
//! 保证），对未编辑区域零接触。#140 域编辑同模型（`field_edits` 平行
//! 数组）：结果区字节区间手术（首 w:t Replace 新值、余 w:t Remove；
//! fldChar/instrText/separate/end 零 op——域在 Word/LibreOffice 打开
//! 仍是活域，见 tests/docx_fields.rs 域活证伪）。跨段域（TOC 型）/嵌套
//! 域/无 separate 退化域第一版只读（fields() 可列——display_value
//! 恒可读）。
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

/// 域类型（#140 任务 1：域码段编辑——指令首词切分，与读面
/// [`crate::docx`] 模块 FieldMachine 的 is_volatile 同一口径：
/// 首词 trim 后按空格/反斜杠/tab/nbsp 切分，大小写不敏感）。
///
/// 未映射指令（PAGEREF/PRINTDATE/IF/DOCPROPERTY…）→ [`FieldKind::Custom`]，
/// 装原始指令串（诊断面——不发明映射，宁缺毋错）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// `PAGE`（当前页码域）。
    Page,
    /// `NUMPAGES`（总页数域）。
    NumPages,
    /// `DATE`（日期域）。
    Date,
    /// `TIME`（时间域）。
    Time,
    /// `TOC \o "1-3" …`（目录域——跨段大域，第一版只读）。
    Toc,
    /// `REF <bookmark>`（书签引用域——第二词为书签名）。
    Ref(String),
    /// 其他指令（原始串透传）。
    Custom(String),
}

impl FieldKind {
    /// 指令串 → 类型（首词切分；`REF` 取第二词为书签名）。
    pub fn from_instr(instr: &str) -> FieldKind {
        field_kind_from_instr(instr)
    }

    /// 该域的指令串（`Ref`/`Custom` 装载原值；纯变体给规范名）。
    pub fn instr(&self) -> String {
        match self {
            FieldKind::Page => "PAGE".into(),
            FieldKind::NumPages => "NUMPAGES".into(),
            FieldKind::Date => "DATE".into(),
            FieldKind::Time => "TIME".into(),
            FieldKind::Toc => "TOC".into(),
            FieldKind::Ref(b) => format!("REF {b}"),
            FieldKind::Custom(s) => s.clone(),
        }
    }
}

/// 域条目（[`DocxEditor::fields`] 返回——段落号 + 域类型 + 当前显示值，
/// 用户坐标面 `(para, field_idx)` 双键即 [`DocxEditor::set_field_value`] 入参）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// begin 所在段落号（文档序扁平坐标系——与 `paragraphs()` 同源；
    /// 跨段域挂 begin 段，end 可能在后段）。
    pub para: usize,
    /// 段内域序（0 基，按 begin 在段内的文档序）。
    pub field_idx: usize,
    /// 域类型。
    pub kind: FieldKind,
    /// 原始指令串（完整——含开关参数；fldSimple 取 `w:instr` 属性）。
    pub instr: String,
    /// 当前缓存显示值（separate 与 end 之间的 `w:t` 拼接；
    /// 无 separate 的退化域 = 空串——指令文本恒不入显示值）。
    pub display_value: String,
    /// begin/end 同段（跨段域——TOC 型——第一版只读）。
    pub same_para: bool,
    /// 本版可编辑（= same_para && 非嵌套 && 有结果区；
    /// TOC 域/嵌套域/无缓存区退化域均为 false）。
    pub editable: bool,
}

/// 脚注条目（[`DocxEditor::footnotes`] 返回——id + 内容 + 引用位置）。
///
/// 分隔符条目（`w:type=separator`/`continuationSeparator`——惯例
/// id -1/0）不入列（与读面 [`crate::docx`] `FootnoteData` 同口径：
/// 呈现辅助件非内容）。`anchor_para` = 正文 `w:footnoteReference`
/// 所在段落号（文档序扁平坐标系；None = 孤儿脚注：footnotes.xml
/// 在场而正文无引用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteInfo {
    /// 脚注 id（`w:footnote/@w:id`）。
    pub id: usize,
    /// 脚注内容（条目内全部 `w:t` 拼接——跨段/跨表；`w:footnoteRef`
    /// 引用编号等非文本节点不入）。
    pub text: String,
    /// 引用位置（正文段落号；孤儿 = None）。
    pub anchor_para: Option<usize>,
    /// 内容段落数（诊断面——多段脚注编辑后段落壳保留，见
    /// [`DocxEditor::set_footnote_text`] 行为登记）。
    pub paras: usize,
}

/// 批注条目（[`DocxEditor::comments`] 返回——id + 作者 + 内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentInfo {
    /// 批注 id（`w:comment/@w:id`——与正文锚 `commentRangeStart`/
    /// `commentReference` 同源）。
    pub id: usize,
    /// 作者（`w:comment/@w:author`——缺省空串）。
    pub author: String,
    /// 日期（`w:comment/@w:date`——缺省 None）。
    pub date: Option<String>,
    /// 批注内容（条目内全部 `w:t` 拼接）。
    pub text: String,
    /// 锚定段落号（正文 `commentRangeStart` 首现段；孤儿 = None）。
    pub anchor_para: Option<usize>,
    /// 内容段落数（诊断面）。
    pub paras: usize,
}

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
    /// 结构化文档标签（`w:sdt` 容器）内段落（#156：含 `w:sdtContent`
    /// 开启态中的段/表格内段）。
    ///
    /// 背景：标书导出链（ai-bid-research → docx-rs）把 TOC 域包在
    /// `w:sdt` 里 emitted；宽面读门（docx-rs 读模型 `StructuredDataTag`
    /// 节点）**结构性丢弃**整个 sdt 容器内容（登记的模型级切片面外项），
    /// 而引擎按原文 `w:p` 逐段计数——两侧段落序因此发散（mang-shell
    /// 装载对账降级只读）。本旗标让消费方能精确对账：**sdt 容器内的
    /// 段落 = 引擎可见、宽面不可见的结构性盲区**（sdt 包裹表格的形态
    /// 消费方应整体拒改——栅格块序与宽面块序无法对齐，见 mang-shell
    /// 装载门）。
    pub in_sdt: bool,
    /// 原始装配文本（私有——经 [`DocxEditor::paragraph_text`] 读）。
    text: String,
}

/// 表格栅格（#115 表格编辑加性扩展——**权威坐标**，非调用方推断）。
///
/// 只为 `document.xml` 里**最外层**的 `w:tbl` 建表；嵌套表的单元格
/// 段落不进栅格（见 [`DocxEditor::table_cells`] 的收面说明）。
#[derive(Debug, Clone, Default)]
struct TableGrid {
    /// 行 → 栅格列 → 单元格段落号（原文坐标系；重复号 = `gridSpan`
    /// 覆盖列 / `vMerge` 续格指向起始格）。
    rows: Vec<Vec<usize>>,
    /// 该表文档序首段落号（诊断/对账面）。
    first_paragraph: usize,
    /// `tblGrid` 声明的列数（0 = 无 tblGrid，按行宽兜底）。
    grid_width: usize,
}

impl TableGrid {
    fn cells(&self) -> Vec<Vec<usize>> {
        self.rows.clone()
    }
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
    /// 表格栅格（#115-表格编辑 加性扩展：表 → 行 → 栅格列 → 段落号）。
    /// 原文坐标系，与 `paragraphs` 同源同序；open 后不变。
    tables: Vec<TableGrid>,
    /// 域索引（#140-域码段编辑：open 时与段落同趟扫描落账；
    /// 原文坐标系，open 后不变——`fields()` 的数据源）。
    fields_idx: Vec<FieldScan>,
    /// 待落账域编辑（与 fields_idx 等长平行数组；None = 未编辑）。
    field_edits: Vec<Option<String>>,
    /// #141 批注脚注：两部件（footnotes.xml / comments.xml——在场
    /// 则扫描，缺席 = None = 该面空集恒保真透传）。
    notes: NoteParts,
    /// 正文锚（`w:footnoteReference`/`commentRangeStart` → 段落号）。
    note_anchors: NoteAnchors,
    /// 待落账脚注编辑（与 `notes.footnotes.entries` 平行；None = 未编辑）。
    pending_footnotes: Vec<Option<String>>,
    /// 待落账批注编辑（与 `notes.comments.entries` 平行）。
    pending_comments: Vec<Option<String>>,
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
        // #234：OLE2 魔数前置（与读面 read_bytes_gated 同款——编辑面
        // 同陷阱：异源 .doc 改名 .docx 不再误报 EOCD 损坏）。
        if io::is_ole2(bytes) {
            return Err(DocxError::Container(io::ReadError::LegacyOfficeFormat {
                expect: "docx",
            }));
        }
        let cursor = std::io::Cursor::new(bytes);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
        // #233：EOCD 守恒门（与读面 read_bytes_gated 同款）。
        io::eocd_conservation(bytes, &za)?;
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
        let (paragraphs, tables, fields_idx, note_anchors) = scan_paragraphs(&xml)?;
        let notes = NoteParts::scan(&mut za)?;
        let n = paragraphs.len();
        let pending_footnotes = notes.pending_len(NotePartKind::Footnotes);
        let pending_comments = notes.pending_len(NotePartKind::Comments);
        Ok(Self {
            bytes: bytes.to_vec(),
            doc_xml: xml,
            paragraphs,
            pending: vec![None; n],
            policy,
            tables,
            field_edits: vec![None; fields_idx.len()],
            fields_idx,
            note_anchors,
            pending_footnotes,
            pending_comments,
            notes,
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

    // -----------------------------------------------------------------------
    // #140 域码段编辑（加性扩展——不改既有语义；拒改面见模块文档）
    // -----------------------------------------------------------------------

    /// 列出文档里的域（段落号 + 类型 + 当前显示值）。
    ///
    /// 坐标系：`para` 与 `paragraphs()` 同源（文档序扁平，含表格/文本框
    /// 内段落）；`field_idx` 为段内序（0 基，按 begin 在段内文档序）。
    /// `(para, field_idx)` 即 [`Self::set_field_value`] 入参。
    pub fn fields(&self) -> Vec<FieldInfo> {
        self.fields_idx
            .iter()
            .map(|f| FieldInfo {
                para: f.begin_para,
                field_idx: f.seq_in_para,
                kind: f.kind.clone(),
                instr: f.instr.clone(),
                display_value: f.display.clone(),
                same_para: f.same_para,
                editable: f.editable,
            })
            .collect()
    }

    // #141 批注脚注（加性扩展——部件级编辑面）
    // -----------------------------------------------------------------------

    /// 列出脚注（id + 内容 + 引用位置）。
    ///
    /// 分隔符条目（separator/continuationSeparator 惯例 -1/0）不入列
    /// （与读面 [`crate::docx`] `FootnoteData` 同口径）。部件缺席
    /// （无 word/footnotes.xml）= 空集——该面保真透传。
    pub fn footnotes(&self) -> Vec<FootnoteInfo> {
        let Some(part) = &self.notes.footnotes else {
            return Vec::new();
        };
        part.entries
            .iter()
            .map(|e| FootnoteInfo {
                id: e.id,
                text: e.text.clone(),
                anchor_para: self.note_anchors.footnote.get(&e.id).copied(),
                paras: e.paras,
            })
            .collect()
    }

    /// 更新域的显示值（只改结果区 `w:t`；fldChar/instrText/separate/end
    /// 五段结构零接触——域在 Word/LibreOffice 里仍是活域）。
    ///
    /// 结果区规则：首个 `w:t` 换新值（`xml:space` + 实体转义 +
    /// `\t`→tab/`\n`→br，与 `set_paragraph_text` 同口径）；其余
    /// 结果区 `w:t` 节点移除（run 壳保留——最小侵入）。
    ///
    /// 拒改面（typed `DocxError::InvalidInput`，fail-closed）：
    /// 坐标不存在 / 跨段域（TOC 型）/ 嵌套域 / 无结果区
    /// （无 `separate` 的退化域，或 separate 后无 `w:t`）/ 新值含
    /// XML 1.0 非法控制字符或非字符。
    ///
    /// 注：域编辑不反映在 [`Self::paragraph_text`]（该文本坐标系
    /// 只随 `set_paragraph_text` 落账；域显示值读面 = `fields()`
    /// `display_value`——save 后 reopen 即新值）。
    pub fn set_field_value(
        &mut self,
        para: usize,
        field_idx: usize,
        value: &str,
    ) -> Result<(), DocxError> {
        let pos = self
            .fields_idx
            .iter()
            .position(|f| f.begin_para == para && f.seq_in_para == field_idx)
            .ok_or_else(|| {
                DocxError::InvalidInput(format!(
                    "field ({para}, {field_idx}) not found — use fields() for actual coordinates"
                ))
            })?;
        let f = &self.fields_idx[pos];
        if !f.same_para {
            return Err(DocxError::InvalidInput(format!(
                "field ({para}, {field_idx}): cross-paragraph field (TOC-type) is read-only in v1"
            )));
        }
        if f.nested {
            return Err(DocxError::InvalidInput(format!(
                "field ({para}, {field_idx}): nested field structure is read-only in v1"
            )));
        }
        if !f.has_separate {
            return Err(DocxError::InvalidInput(format!(
                "field ({para}, {field_idx}): no w:fldChar separate (degenerate field) \
                 refuses edit — refusing to invent field structure"
            )));
        }
        if f.result_spans.is_empty() {
            return Err(DocxError::InvalidInput(format!(
                "field ({para}, {field_idx}): empty result region (no w:t between \
                 separate and end) refuses edit"
            )));
        }
        check_field_text(para, field_idx, value)?;
        self.field_edits[pos] = Some(value.to_string());
        Ok(())
    }

    /// 列出批注（id + 作者 + 内容）。
    ///
    /// `anchor_para` = 正文 `commentRangeStart`/`commentReference` 首现
    /// 段落号；孤儿批注（comments.xml 在场而正文无锚）= None。
    pub fn comments(&self) -> Vec<CommentInfo> {
        let Some(part) = &self.notes.comments else {
            return Vec::new();
        };
        part.entries
            .iter()
            .map(|e| CommentInfo {
                id: e.id,
                author: e.author.clone(),
                date: e.date.clone(),
                text: e.text.clone(),
                anchor_para: self.note_anchors.comment.get(&e.id).copied(),
                paras: e.paras,
            })
            .collect()
    }

    /// 更新脚注文本（`id` = `footnotes()` 给出的 id）。
    ///
    /// 手术规则：条目内**首个** `w:t` 换新值（`xml:space`/实体转义/
    /// `\t`→tab/`\n`→br——与 `set_paragraph_text` 同口径），其余
    /// `w:t` 节点移除（run 壳与段落壳保留——最少侵入；多段脚注
    /// 编辑后残余空段占行，Word 显示 = 新文本单段）。
    /// `w:footnoteRef`（页底编号）零接触。
    ///
    /// 拒改面（typed `DocxError::InvalidInput`）：id 不存在 / 条目
    /// 无文本节点（空内容无法置值——不发明内容区）/ 新值含 XML 1.0
    /// 非法控制字符或非字符。
    pub fn set_footnote_text(&mut self, id: usize, text: &str) -> Result<(), DocxError> {
        self.set_note_text(NotePartKind::Footnotes, id, text)
    }

    /// 更新批注文本（`id` = `comments()` 给出的 id；作者/日期不动）。
    ///
    /// 手术规则与拒改面同 [`DocxEditor::set_footnote_text`]；
    /// `w:annotationRef` 零接触，正文锚（commentRangeStart/
    /// commentReference）在 document.xml 不动。
    pub fn set_comment_text(&mut self, id: usize, text: &str) -> Result<(), DocxError> {
        self.set_note_text(NotePartKind::Comments, id, text)
    }

    /// 两部件共用置值内派。
    fn set_note_text(
        &mut self,
        kind: NotePartKind,
        id: usize,
        text: &str,
    ) -> Result<(), DocxError> {
        let (part, pending) = self.note_slot_mut(kind)?;
        let Some(part) = part else {
            return Err(DocxError::InvalidInput(format!(
                "note id {id}: part absent (no word/{part_name} — nothing to edit)",
                part_name = match kind {
                    NotePartKind::Footnotes => "footnotes.xml",
                    NotePartKind::Comments => "comments.xml",
                }
            )));
        };
        let pos = part
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or_else(|| {
                DocxError::InvalidInput(format!(
                    "note id {id} not found — use footnotes()/comments() for actual ids"
                ))
            })?;
        if part.entries[pos].wt_spans.is_empty() {
            return Err(DocxError::InvalidInput(format!(
                "note id {id}: empty content region (no w:t) refuses edit \
                 — refusing to invent content"
            )));
        }
        check_note_text(kind, id, text)?;
        pending[pos] = Some(text.to_string());
        Ok(())
    }

    /// 部件+待落账数组双槽借用。
    fn note_slot_mut(
        &mut self,
        kind: NotePartKind,
    ) -> Result<(Option<&mut NotePart>, &mut Vec<Option<String>>), DocxError> {
        Ok(match kind {
            NotePartKind::Footnotes => (self.notes.footnotes.as_mut(), &mut self.pending_footnotes),
            NotePartKind::Comments => (self.notes.comments.as_mut(), &mut self.pending_comments),
        })
    }

    // -----------------------------------------------------------------------
    // #115 表格编辑（加性扩展——只读查表，不改既有语义）
    // -----------------------------------------------------------------------

    /// 表格栅格：表序号 → 行 → 单元格段落号（权威坐标，来自本文档
    /// `document.xml` 扫描，非调用方推断）。
    ///
    /// 合并格按 OOXML 语义**占位**（与读面 `TableData::rows` 的栅格展开
    /// 同口径）：`gridSpan` 展开的覆盖列重复给出首格段落号；`vMerge`
    /// 的 `continue` 格给出**起始格**段落号。故每行长度 = 该行栅格宽度，
    /// 消费方可以直接按 `cells[row][col]` 取段落号。
    ///
    /// 只收 `document.xml` 里**最外层**（正文直系）的 `w:tbl`：
    /// 嵌套表（`w:tbl` 套 `w:tbl`）与其单元格段落不进任何表的栅格——
    /// 它们在段落序里仍是段落（`in_table` 为真），但不出现在
    /// `table_cells()` 里。理由：嵌套表单元格段落没有"文档序平面"上
    /// 稳定的行列归属，猜坐标就会静默改错格（设计 §2 的三类静默错）。
    ///
    /// 本方法**只读**：不写盘、不改 `paragraphs`、不影晌 `save_to_vec`。
    pub fn table_cells(&self) -> Vec<Vec<Vec<usize>>> {
        self.tables.iter().map(TableGrid::cells).collect()
    }

    /// 表格数（`table_cells().len()` 的廉价别名）。
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// 表 `table` 的文档序首段落号（诊断/对账锚）；越界 = `None`。
    pub fn table_first_paragraph(&self, table: usize) -> Option<usize> {
        self.tables.get(table).map(|t| t.first_paragraph)
    }

    /// 表 `table` 的 `tblGrid` 声明列数（0 = 无 `tblGrid`）；越界 = `None`。
    pub fn table_grid_width(&self, table: usize) -> Option<usize> {
        self.tables.get(table).map(|t| t.grid_width)
    }

    /// 表 `table` 的行列形状 `(rows, max_cols)`；越界 = `None`。
    pub fn table_shape(&self, table: usize) -> Option<(usize, usize)> {
        self.tables.get(table).map(|t| {
            (
                t.rows.len(),
                t.rows.iter().map(|r| r.len()).max().unwrap_or(0),
            )
        })
    }

    /// 保存（一次性落账全部待编辑——段落文本、域显示值、脚注/批注三类；
    /// 幂等——同状态重复调用产出相同字节）。
    pub fn save_to_vec(&self) -> Result<Vec<u8>, DocxError> {
        let new_xml = apply_all_edits(
            &self.doc_xml,
            &self.paragraphs,
            &self.pending,
            &self.fields_idx,
            &self.field_edits,
        )?;
        // #141 脚注/批注部件：有编辑则手术，无编辑透传 None。
        let new_footnotes =
            apply_note_edits(self.notes.footnotes.as_ref(), &self.pending_footnotes);
        let new_comments = apply_note_edits(self.notes.comments.as_ref(), &self.pending_comments);
        repack_parts(
            &self.bytes,
            new_xml.as_bytes(),
            new_footnotes.as_deref().map(str::as_bytes),
            new_comments.as_deref().map(str::as_bytes),
        )
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

// ---------------------------------------------------------------------------
// #140 域码段：域扫描（与 scan_paragraphs 同趟喂点——域事件/段落
// 坐标系天然同源；fldChar 五段结构与 fldSimple 简单形两形态）
// ---------------------------------------------------------------------------

/// 域扫描落账条目（原文坐标系——open 后不变，`fields()` 数据源）。
#[derive(Debug)]
struct FieldScan {
    /// begin（或 fldSimple）所在段落号。
    begin_para: usize,
    /// 段内域序（0 基，按 begin 在段内文档序——finish 统一分配）。
    seq_in_para: usize,
    /// begin 节点 '<' 字节位（同段多域的段内排序锚）。
    begin_byte: usize,
    kind: FieldKind,
    /// 原始指令串（instrText 拼接 / fldSimple `w:instr` 属性）。
    instr: String,
    /// 缓存显示值（结果区 `w:t` 拼接；退化域 = 空串）。
    display: String,
    /// 结果区各 `w:t` **节点域**（含开闭标签——编辑锚点：首 Replace /
    /// 余 Remove；跨段域照收全量但只读面不用）。
    result_spans: Vec<(usize, usize)>,
    /// begin/end 同段（fldSimple 恒 true——不跨段元素）。
    same_para: bool,
    /// 嵌套参与（结果区内出现子 begin / 本域内出现 fldChar）。
    nested: bool,
    /// 有 separate（fldSimple = true——简单形无 separate 概念，
    /// 结果区即子 `w:t`）。
    has_separate: bool,
    /// 可编辑（= same_para && !nested && has_separate && !result_spans.empty()）。
    editable: bool,
}

/// 开放域累积态（fldChar 栈项 / fldSimple 槽共用形）。
#[derive(Debug, Default)]
struct OpenField {
    begin_para: Option<usize>,
    begin_byte: usize,
    instr: String,
    /// separate 已见（结果区开放——complex 域专用）。
    in_result: bool,
    has_separate: bool,
    display: String,
    result_spans: Vec<(usize, usize)>,
    nested: bool,
    /// fldSimple 形。
    is_simple: bool,
}

/// 域扫描状态机（scan_paragraphs 同趟喂点；开放域 EOF 未闭合 = 丢弃
/// 不落账——未闭合域非完整结构，列出来会误导「有显示值」）。
#[derive(Debug, Default)]
struct FieldBuilders {
    /// fldChar 域嵌套栈（内层最尾）。
    complex: Vec<OpenField>,
    /// fldSimple 开放槽（fldSimple 内遇 fldChar → 本槽标 nested，
    /// complex 机照常独立跑——畸形态只读保全）。
    simple: Option<OpenField>,
    /// 开放中 instrText 的 (node_start, content_open)。
    instr_open: Option<(usize, usize)>,
    /// 已落账域（finish 前序——begin 文档序大体但非严格，终期统一排序）。
    done: Vec<FieldScan>,
}

impl FieldBuilders {
    /// fldChar 事件（Start/Empty 同形——`w:fldCharType` 驱动）。
    fn on_fldchar(&mut self, ty: Option<&str>, para: Option<usize>, node_start: usize) {
        match ty {
            Some("begin") => {
                if let Some(s) = self.simple.as_mut() {
                    s.nested = true;
                }
                if let Some(top) = self.complex.last_mut() {
                    top.nested = true;
                }
                if let Some(p) = para {
                    self.complex.push(OpenField {
                        begin_para: Some(p),
                        begin_byte: node_start,
                        ..OpenField::default()
                    });
                }
                // begin 无段落上下文（段落外 run——畸形）：不 push；
                // 后续 separate/end 因栈空自然忽略。
            }
            Some("separate") => {
                if let Some(top) = self.complex.last_mut() {
                    top.has_separate = true;
                    top.in_result = true;
                }
                // fldSimple 内的 fldChar separate：simple 已在 begin 喂点
                // 标 nested——这里无需再动（simple 无结果态概念）。
            }
            Some("end") => {
                if let Some(open) = self.complex.pop() {
                    if open.begin_para.is_some() {
                        let scan = finalize_scan(open, para);
                        self.done.push(scan);
                    }
                }
            }
            _ => {}
        }
    }

    /// instrText 内容闭（喂开放 complex 栈顶；无栈顶时喂 simple——
    /// fldSimple 内 instrText 罕见面）。
    fn on_instr_close(&mut self, content: &str) {
        if let Some(top) = self.complex.last_mut() {
            top.instr.push_str(content);
        } else if let Some(s) = self.simple.as_mut() {
            s.instr.push_str(content);
        }
    }

    /// fldSimple 开（Start 形——子内容随后；Empty 形走 on_fldsimple_empty）。
    fn on_fldsimple_start(
        &mut self,
        instr: Option<String>,
        para: Option<usize>,
        node_start: usize,
    ) {
        // 简单形无 separate 概念：结果区 = 子层 w:t（恒开放收集）。
        let open = OpenField {
            begin_para: para,
            begin_byte: node_start,
            instr: instr.unwrap_or_default(),
            is_simple: true,
            has_separate: true,
            ..OpenField::default()
        };
        // 已有开放 simple（w:p 层 fldSimple 套 fldSimple 畸形）：旧槽
        // 丢弃不落账，新槽顶替（登记面——罕畸形态）。
        self.simple = Some(open);
    }

    /// fldSimple 自闭合（无子内容——空结果区，只读面即刻落账）。
    fn on_fldsimple_empty(&mut self, instr: Option<String>, para: Option<usize>) {
        if para.is_none() {
            return;
        }
        let open = OpenField {
            begin_para: para,
            instr: instr.unwrap_or_default(),
            is_simple: true,
            has_separate: true,
            ..OpenField::default()
        };
        let scan = finalize_scan(open, para);
        self.done.push(scan);
    }

    /// fldSimple 闭（End 事件——落账）。
    fn on_fldsimple_end(&mut self, para: Option<usize>) {
        if let Some(open) = self.simple.take() {
            if open.begin_para.is_some() {
                let scan = finalize_scan(open, para);
                self.done.push(scan);
            }
        }
    }

    /// 收尾：残开放域丢弃 + 排序 + 段内序分配。
    fn finish(mut self) -> Vec<FieldScan> {
        let mut items = std::mem::take(&mut self.done);
        items.sort_by_key(|f| (f.begin_para, f.begin_byte));
        let mut cur_para = usize::MAX;
        let mut seq = 0usize;
        for f in items.iter_mut() {
            if f.begin_para != cur_para {
                cur_para = f.begin_para;
                seq = 0;
            }
            f.seq_in_para = seq;
            seq += 1;
        }
        items
    }
}

/// 开放域 → 落账条目（kind 解析 + editable 判定）。
fn finalize_scan(open: OpenField, end_para: Option<usize>) -> FieldScan {
    let same_para = if open.is_simple {
        true // fldSimple 是段落内元素，不跨段。
    } else {
        end_para.is_some() && open.begin_para.is_some() && open.begin_para == end_para
    };
    let kind = field_kind_from_instr(&open.instr);
    let editable = same_para && !open.nested && open.has_separate && !open.result_spans.is_empty();
    FieldScan {
        begin_para: open.begin_para.unwrap_or(usize::MAX),
        seq_in_para: 0,
        begin_byte: open.begin_byte,
        kind,
        instr: open.instr,
        display: open.display,
        result_spans: open.result_spans,
        same_para,
        nested: open.nested,
        has_separate: open.has_separate,
        editable,
    }
}

/// 指令串 → 域类型（首词切分与读面 FieldMachine::is_volatile 同口径；
/// `REF` 取第二词为书签名；未映射 → Custom 装原串）。
fn field_kind_from_instr(instr: &str) -> FieldKind {
    let t = instr.trim();
    let head = t.split([' ', '\\', '\t', '\u{a0}']).next().unwrap_or("");
    let up = head.to_ascii_uppercase();
    match up.as_str() {
        "PAGE" => FieldKind::Page,
        "NUMPAGES" => FieldKind::NumPages,
        "DATE" => FieldKind::Date,
        "TIME" => FieldKind::Time,
        "TOC" => FieldKind::Toc,
        "REF" => {
            let rest = t.get(head.len()..).unwrap_or("").trim();
            let bm = rest.split([' ', '\\', '\t', '\u{a0}']).next().unwrap_or("");
            FieldKind::Ref(bm.to_string())
        }
        _ => FieldKind::Custom(t.to_string()),
    }
}

/// 域新值防线（与 check_edit_text 同口径——坐标面换域）。
fn check_field_text(para: usize, field_idx: usize, text: &str) -> Result<(), DocxError> {
    for c in text.chars() {
        let cp = c as u32;
        if matches!(c, '\u{FFFE}' | '\u{FFFF}') {
            return Err(DocxError::InvalidInput(format!(
                "field ({para}, {field_idx}): new value contains XML noncharacter U+{cp:04X}"
            )));
        }
        if cp < 0x20 && !matches!(c, '\t' | '\n' | '\r') {
            return Err(DocxError::InvalidInput(format!(
                "field ({para}, {field_idx}): new value contains control character U+{cp:04X} \
                 (XML 1.0 illegal; allowed: \\t \\n \\r as tab/br)"
            )));
        }
    }
    Ok(())
}

/// 取任意命名属性（无前缀变体同名同认——`id`/`type`/`author`/`date`/
/// `fldCharType`/`instr`；与 attr_val 同构——#140 域事件与 #141 note
/// 部件/正文锚共用）。
fn attr_named(e: &quick_xml::events::BytesStart<'_>, want: &str) -> Option<String> {
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned();
        if key == want {
            return Some(String::from_utf8_lossy(a.value.as_ref()).into_owned());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// #141 批注脚注：footnotes.xml / comments.xml 部件扫描与锚
// ---------------------------------------------------------------------------

/// 部件判别（footnotes / comments）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotePartKind {
    Footnotes,
    Comments,
}

/// 条目（`w:footnote` / `w:comment`）——原文坐标系（部件 xml 内）。
#[derive(Debug)]
struct NoteEntry {
    id: usize,
    /// `w:comment/@w:author`（footnotes 恒空串）。
    author: String,
    /// `w:comment/@w:date`（footnotes 恒 None）。
    date: Option<String>,
    /// 条目内全部 `w:t` 拼接（跨段/跨表；`w:footnoteRef`/
    /// `w:annotationRef` 引用编号非文本节点不入）。
    text: String,
    /// 文本节点域（部件 xml 坐标系——编辑锚：首 Replace / 余 Remove）。
    wt_spans: Vec<(usize, usize)>,
    /// 段落数（诊断面）。
    paras: usize,
}

/// 单部件（原文 + 条目索引）。
#[derive(Debug)]
struct NotePart {
    xml: String,
    entries: Vec<NoteEntry>,
}

/// 两部件组（缺席 = None——该面空集，save 时恒保真透传）。
#[derive(Debug, Default)]
struct NoteParts {
    footnotes: Option<NotePart>,
    comments: Option<NotePart>,
}

/// 正文锚（document.xml 坐标系——id → 段落号）。
#[derive(Debug, Default)]
struct NoteAnchors {
    footnote: std::collections::HashMap<usize, usize>,
    comment: std::collections::HashMap<usize, usize>,
}

impl NoteParts {
    /// 待落账数组长度（平行数组分配用）。
    fn pending_len(&self, kind: NotePartKind) -> Vec<Option<String>> {
        let n = match (kind, &self.footnotes, &self.comments) {
            (NotePartKind::Footnotes, Some(p), _) => p.entries.len(),
            (NotePartKind::Comments, _, Some(p)) => p.entries.len(),
            _ => 0,
        };
        vec![None; n]
    }

    /// 读部件（在场则扫描条目；缺席 = None）。
    fn scan(za: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> Result<Self, DocxError> {
        Ok(Self {
            footnotes: Self::scan_part(za, "word/footnotes.xml", NotePartKind::Footnotes)?,
            comments: Self::scan_part(za, "word/comments.xml", NotePartKind::Comments)?,
        })
    }

    fn scan_part(
        za: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
        name: &'static str,
        kind: NotePartKind,
    ) -> Result<Option<NotePart>, DocxError> {
        let mut zf = match za.by_name(name) {
            Ok(zf) => zf,
            Err(_) => return Ok(None),
        };
        let mut xml = String::new();
        zf.read_to_string(&mut xml).map_err(|e| {
            DocxError::Container(io::ReadError::InvalidPart {
                part: name.to_string(),
                detail: format!("read/UTF-8 failed: {e}"),
            })
        })?;
        let entries = scan_note_entries(&xml, kind)?;
        Ok(Some(NotePart { xml, entries }))
    }
}

/// 部件单趟扫描：`w:footnote`/`w:comment` 条目开闭 + 条目内 `w:t`
/// 收集（节点域 + 内容拼接；同 scan_paragraphs 的 wt_open 模式）。
///
/// 分隔符条目（`@w:type` 在场——separator/continuationSeparator 惯例
/// id -1/0）与 `@w:id` 非 usize 条目不入列（呈现辅助件非内容——与
/// 读面 `FootnoteData` 同口径）；其内容不收（不开条目上下文）。
fn scan_note_entries(xml: &str, kind: NotePartKind) -> Result<Vec<NoteEntry>, DocxError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let bytes = xml.as_bytes();
    let mut out: Vec<NoteEntry> = Vec::new();
    // 开放条目索引（out 下标）；同一时刻至多一个（条目不嵌套）。
    let mut open_entry: Option<usize> = None;
    let mut wt_open: Option<(usize, usize)> = None;
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "footnote" | "comment" => {
                        // 条目开：@w:type 在场 = 分隔符惯例；@w:id 非
                        // usize（含 -1）= 畸形惯例——均不入列。
                        let has_type = attr_named(&e, "type").is_some();
                        let id = attr_named(&e, "id").and_then(|v| v.trim().parse::<usize>().ok());
                        if has_type || id.is_none() {
                            open_entry = None;
                        } else {
                            out.push(NoteEntry {
                                id: id.unwrap_or(usize::MAX),
                                author: attr_named(&e, "author").unwrap_or_default(),
                                date: attr_named(&e, "date"),
                                text: String::new(),
                                wt_spans: Vec::new(),
                                paras: 0,
                            });
                            open_entry = Some(out.len() - 1);
                        }
                    }
                    "p" => {
                        if let Some(i) = open_entry {
                            out[i].paras += 1;
                        }
                    }
                    "t" => {
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        wt_open = Some((node_start, content_open));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "p" => {
                        // 自闭合空段：计段不收文本（无子事件）。
                        if let Some(i) = open_entry {
                            out[i].paras += 1;
                        }
                    }
                    "t" => {} // 自闭合空 w:t：零内容不收。
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name_end(&e);
                match name.as_str() {
                    "footnote" | "comment" => open_entry = None,
                    "t" => {
                        if let (Some(i), Some((node_start, open))) = (open_entry, wt_open.take()) {
                            let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                            let close_start = back_to_lt(bytes, end);
                            if close_start >= open {
                                let raw = &xml[open..close_start];
                                let decoded = quick_xml::escape::unescape(raw).map_err(|e| {
                                    DocxError::Container(io::ReadError::InvalidPart {
                                        part: "notes part".to_string(),
                                        detail: format!("text entity decode failed: {e}"),
                                    })
                                })?;
                                if !decoded.is_empty() {
                                    out[i].text.push_str(decoded.as_ref());
                                    out[i].wt_spans.push((node_start, end));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(DocxError::Container(io::ReadError::InvalidPart {
                    part: "notes part".to_string(),
                    detail: format!("note scan failed: {e}"),
                }));
            }
        }
    }
    let _ = kind; // 判别仅用于诊断预留（两部件条目模型同构）。
    Ok(out)
}

/// scan_paragraphs 返回形（段落表 / 表格栅格 / 域索引 / 正文锚——
/// #140 与 #141 各加一元）。
type ScanTuple = (
    Vec<ParagraphRef>,
    Vec<TableGrid>,
    Vec<FieldScan>,
    NoteAnchors,
);

fn scan_paragraphs(xml: &str) -> Result<ScanTuple, DocxError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let bytes = xml.as_bytes();
    let mut out: Vec<ParagraphRef> = Vec::new();
    // #141 正文锚：note 引用/批注锦 id → 段落号（首现 or_insert）。
    let mut note_anchors = NoteAnchors::default();
    // 开启中的段落（内层最尾——文本/旗标只记 innermost；嵌套文本框段落
    // 各自独立索引，外层不吃内层文本）。
    let mut para_stack: Vec<usize> = Vec::new();
    // 开启中 w:t 的 (节点 '<' 位, 内容区 open 端)（w:t 恒为叶——单槽；
    // #140：节点位供域结果区收集，内容端供段落文本装配）。
    let mut wt_open: Option<(usize, usize)> = None;
    // #140 域状态机（与段落同趟喂点）。
    let mut fbuild = FieldBuilders::default();
    let mut fallback = 0usize;
    let mut tbl = 0usize;
    let mut txbx = 0usize;
    // #156：`w:sdt` 容器深度（段落级 in_sdt 旗标的数据源——背景见
    // [`ParagraphRef::in_sdt`]）。sdt 可嵌套（sdtContent 内再套 sdt）。
    let mut sdt = 0usize;

    // ── #115 表格编辑（加性扩展）栅格扫描状态 ──────────────────────
    // 开启中的 `w:tbl`（只为**最外层**建栅格——见 DocxEditor::table_cells）。
    let mut tbl_stack: Vec<TableBuild> = Vec::new();
    // 表内 `w:tr` 深度（非 0 = 在表行内）。
    let mut tr = 0usize;
    // 开启中的 `w:tc`（每项 = 一个原生单元格；仅在表行内压栈）。
    let mut tc_stack: Vec<CellBuild> = Vec::new();
    // 落成的表栅格（文档序）。
    let mut tables: Vec<TableGrid> = Vec::new();

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "Fallback" => fallback += 1,
                    "sdt" => sdt += 1,
                    "tbl" => {
                        // 深度 0 → 1 才是外层表；嵌套表（深度 ≥2）只加
                        // 深度，其单元格段落**不进**任何栅格。
                        if tbl == 0 {
                            tbl_stack.push(TableBuild::default());
                        }
                        tbl += 1;
                    }
                    "tblGrid" if tbl == 1 => {
                        if let Some(t) = tbl_stack.last_mut() {
                            t.in_grid = true;
                        }
                    }
                    "tr" if tbl > 0 => {
                        tr += 1;
                        // 只有外层表（tbl==1）才推进本表行号计数器；
                        // 嵌套表的行数不参与外层栅格。
                        if tbl == 1 {
                            if let Some(t) = tbl_stack.last_mut() {
                                t.tr_seen += 1;
                            }
                        }
                    }
                    "tc" if tbl > 0 && tr > 0 => {
                        // 行号 = 本表内已开启行数 - 1（"tr" 分支先自增）。
                        let row_abs = tbl_stack
                            .last()
                            .map(|t| t.tr_seen.saturating_sub(1))
                            .unwrap_or(0);
                        tc_stack.push(CellBuild {
                            row_abs,
                            ..CellBuild::default()
                        });
                    }
                    "gridSpan" if !tc_stack.is_empty() => {
                        if let Some(v) = attr_span(&e) {
                            if let Some(c) = tc_stack.last_mut() {
                                c.grid_span = v;
                            }
                        }
                    }
                    "vMerge" if !tc_stack.is_empty() => {
                        // 无 `w:val` = continue（OOXML §17.7.41 语义）。
                        let v = attr_val(&e);
                        if let Some(c) = tc_stack.last_mut() {
                            c.v_merge = Some(match v.as_deref() {
                                Some("restart") => VMergeVal::Restart,
                                _ => VMergeVal::Continue,
                            });
                        }
                    }
                    "txbxContent" => txbx += 1,
                    "p" => {
                        let start = fwd_to_lt(bytes, pos_before);
                        let idx = out.len();
                        out.push(ParagraphRef {
                            span: (start, usize::MAX),
                            field_code: false,
                            revision: false,
                            fallback_copy: fallback > 0,
                            in_table: tbl > 0,
                            in_textbox: txbx > 0,
                            in_sdt: sdt > 0,
                            text: String::new(),
                        });
                        para_stack.push(idx);
                        // 栅格：单元格的**首个**段落进栅格（续段仍计
                        // 段落序，但不再占一格——与读面 rows 栅格「每格
                        // 一条文本」同口径）。嵌套表（tbl>1）内的段落
                        // 不进外层栅格。
                        if tbl <= 1 {
                            if let Some(c) = tc_stack.last_mut() {
                                if c.paragraph.is_none() {
                                    c.paragraph = Some(idx);
                                }
                            }
                        }
                    }
                    "t" if !para_stack.is_empty() => {
                        // 文本节点开：记 (节点 '<' 位, 内容区 open 端)
                        // （内容由配对 End 补齐——需闭标签起点定位内容区）。
                        let node_start = fwd_to_lt(bytes, pos_before);
                        let content_open = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        wt_open = Some((node_start, content_open));
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
                        // #140 域状态机喂点（开形——Empty 形在 Empty 分支同款）。
                        let node_start = fwd_to_lt(bytes, pos_before);
                        match name.as_str() {
                            "fldChar" => {
                                let ty = attr_named(&e, "fldCharType");
                                fbuild.on_fldchar(
                                    ty.as_deref(),
                                    para_stack.last().copied(),
                                    node_start,
                                );
                            }
                            "instrText" => {
                                let content_open =
                                    fwd_to_gt(bytes, reader.buffer_position() as usize);
                                fbuild.instr_open = Some((node_start, content_open));
                            }
                            "fldSimple" => {
                                let instr = attr_named(&e, "instr");
                                fbuild.on_fldsimple_start(
                                    instr,
                                    para_stack.last().copied(),
                                    node_start,
                                );
                            }
                            _ => unreachable!(),
                        }
                    }
                    _ => {}
                }
            }

            Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "gridCol" if tbl == 1 => {
                        // 声明列数（只在表自己的 tblGrid 内计——单元格
                        // 内嵌表的 tblGrid 由 tbl 深度挡在外面）。
                        if let Some(t) = tbl_stack.last_mut() {
                            if t.in_grid {
                                t.grid_width += 1;
                            }
                        }
                    }
                    "p" => {
                        // 自闭合空段：完整段（无子事件），即刻落账。
                        let start = fwd_to_lt(bytes, pos_before);
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        let idx = out.len();
                        out.push(ParagraphRef {
                            span: (start, end),
                            field_code: false,
                            revision: false,
                            fallback_copy: fallback > 0,
                            in_table: tbl > 0,
                            in_textbox: txbx > 0,
                            in_sdt: sdt > 0,
                            text: String::new(),
                        });
                        if tbl <= 1 {
                            if let Some(c) = tc_stack.last_mut() {
                                if c.paragraph.is_none() {
                                    c.paragraph = Some(idx);
                                }
                            }
                        }
                    }
                    "gridSpan" if !tc_stack.is_empty() => {
                        if let Some(v) = attr_span(&e) {
                            if let Some(c) = tc_stack.last_mut() {
                                c.grid_span = v;
                            }
                        }
                    }
                    "vMerge" if !tc_stack.is_empty() => {
                        let v = attr_val(&e);
                        if let Some(c) = tc_stack.last_mut() {
                            c.v_merge = Some(match v.as_deref() {
                                Some("restart") => VMergeVal::Restart,
                                _ => VMergeVal::Continue,
                            });
                        }
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
                        // #140 域状态机喂点（自闭合形）。
                        let node_start = fwd_to_lt(bytes, pos_before);
                        match name.as_str() {
                            "fldChar" => {
                                let ty = attr_named(&e, "fldCharType");
                                fbuild.on_fldchar(
                                    ty.as_deref(),
                                    para_stack.last().copied(),
                                    node_start,
                                );
                            }
                            "instrText" => {} // 自闭合空指令——零内容。
                            "fldSimple" => {
                                let instr = attr_named(&e, "instr");
                                fbuild.on_fldsimple_empty(instr, para_stack.last().copied());
                            }
                            _ => unreachable!(),
                        }
                    }
                    "footnoteReference" | "endnoteReference" | "commentRangeStart"
                    | "commentReference"
                        if !para_stack.is_empty() =>
                    {
                        // #141 锚：正文引用/批注锚 id → 段落号（首现
                        // or_insert；id 非 usize 畸形忽略。endnote 锚
                        // 不收集——尾注部件不在编辑面，原样透传）。
                        if let (Some(&p), Some(id)) = (
                            para_stack.last(),
                            attr_named(&e, "id").and_then(|v| v.trim().parse::<usize>().ok()),
                        ) {
                            match name.as_str() {
                                "footnoteReference" => {
                                    note_anchors.footnote.entry(id).or_insert(p);
                                }
                                "commentRangeStart" | "commentReference" => {
                                    note_anchors.comment.entry(id).or_insert(p);
                                }
                                _ => {}
                            }
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
                    "sdt" => sdt = sdt.saturating_sub(1),
                    "tbl" => {
                        // 外层表收尾：栅格落表。
                        if tbl == 1 {
                            if let Some(t) = tbl_stack.pop() {
                                tables.push(t.finish());
                            }
                        }
                        tbl = tbl.saturating_sub(1);
                    }
                    "tblGrid" => {
                        if let Some(t) = tbl_stack.last_mut() {
                            t.in_grid = false;
                        }
                    }
                    "tr" => tr = tr.saturating_sub(1),
                    "tc" => {
                        if tbl == 1 {
                            if let Some(c) = tc_stack.pop() {
                                if let Some(t) = tbl_stack.last_mut() {
                                    t.cells.push(c);
                                }
                            }
                        }
                    }
                    "txbxContent" => txbx = txbx.saturating_sub(1),
                    "p" => {
                        let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                        if let Some(i) = para_stack.pop() {
                            out[i].span.1 = end;
                        }
                    }
                    "t" => {
                        // 文本节点闭：内容区 = [content_open, 闭标签起点)。
                        if let (Some(&i), Some((node_start, open))) =
                            (para_stack.last(), wt_open.take())
                        {
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
                                // #140 域结果区收集：嵌套期 w:t 归属
                                // **所有** in_result 开放层（外层 display
                                // = 整域渲染文本——含嵌套子域显示值；
                                // fldSimple 与 complex 栈各层独立喂）。
                                // 空 w:t 不收（display 与 spans 一致）。
                                if !decoded.is_empty() {
                                    let node_span = (node_start, end);
                                    if let Some(s) = fbuild.simple.as_mut() {
                                        s.display.push_str(decoded.as_ref());
                                        s.result_spans.push(node_span);
                                    }
                                    for open in fbuild.complex.iter_mut() {
                                        if open.in_result {
                                            open.display.push_str(decoded.as_ref());
                                            open.result_spans.push(node_span);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "instrText" => {
                        // #140 指令文本闭：内容区 → 开放域 instr 累积
                        //（simple 开放优先喂 simple，否则 complex 栈顶）。
                        if let Some((_node_start, open)) = fbuild.instr_open.take() {
                            let end = fwd_to_gt(bytes, reader.buffer_position() as usize);
                            let close_start = back_to_lt(bytes, end);
                            if close_start >= open {
                                let raw = &xml[open..close_start];
                                let decoded = quick_xml::escape::unescape(raw).map_err(|e| {
                                    DocxError::Container(io::ReadError::InvalidPart {
                                        part: "word/document.xml".to_string(),
                                        detail: format!("instrText entity decode failed: {e}"),
                                    })
                                })?;
                                fbuild.on_instr_close(decoded.as_ref());
                            }
                        }
                    }
                    "fldSimple" => {
                        // #140 fldSimple 闭：simple 槽落账。
                        fbuild.on_fldsimple_end(para_stack.last().copied());
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
    Ok((out, tables, fbuild.finish(), note_anchors))
}

/// 表栅格扫描累积态（扫描期）。
#[derive(Debug, Default)]
struct TableBuild {
    /// 原生单元格（文档序）。
    cells: Vec<CellBuild>,
    /// `tblGrid` 声明列数。
    grid_width: usize,
    /// 是否处于本表自己的 `tblGrid` 内。
    in_grid: bool,
    /// 本表内已开启的 `w:tr` 数（0 基行号来源）。
    tr_seen: usize,
}

impl TableBuild {
    /// 栅格展开（与读面 `TableData::rows` 同口径：gridSpan 覆盖列重复
    /// 首格段落号；vMerge 续格指向起始格段落号）。
    fn finish(self) -> TableGrid {
        let first_paragraph = self.cells.iter().find_map(|c| c.paragraph).unwrap_or(0);
        let width = self.grid_width.max(1);
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        // vMerge 开放区：列区间 → 起始格段落号（跨行存续，换行不清）。
        let mut open_v: Vec<((usize, usize), usize)> = Vec::new();
        let mut cur_row: Option<usize> = None;
        for c in &self.cells {
            // 表行切换 = 换栅格行（vMerge 开放区**跨行**保持）。
            if cur_row != Some(c.row_abs) {
                if cur_row.is_some() {
                    rows.push(std::mem::take(&mut current));
                }
                cur_row = Some(c.row_abs);
            }
            let start_col = current.len();
            let span = c.grid_span.max(1);
            let end_col = start_col + span - 1;
            // 本格的落栅格段落号（无 `w:p` 的畸形格 = 该表首段，占位不
            // 丢列——把'少一格'变成'最后一格'会静默错位）。
            let own = c.paragraph.unwrap_or(first_paragraph);
            let anchor = match c.v_merge {
                Some(VMergeVal::Continue) => open_v
                    .iter()
                    .find(|((s, e2), _)| *s == start_col && *e2 == end_col)
                    .map(|(_, p)| p)
                    .copied()
                    .unwrap_or(own),
                Some(VMergeVal::Restart) => {
                    open_v.retain(|((s, e2), _)| !(*s == start_col && *e2 == end_col));
                    open_v.push(((start_col, end_col), own));
                    own
                }
                None => {
                    open_v.retain(|((s, e2), _)| !(*s == start_col && *e2 == end_col));
                    own
                }
            };
            for k in 0..span {
                let _ = k;
                current.push(anchor);
            }
        }
        if !current.is_empty() {
            rows.push(current);
        }
        // 宽兜底：tblGrid 声明比实际宽（缺格表）时补该行末格，保持矩形
        // （消费方按栅格坐标取格，行长不一致 = 越界风险）。
        for r in rows.iter_mut() {
            if r.is_empty() {
                r.push(first_paragraph);
            }
            while r.len() < width && self.grid_width > 0 {
                let last = *r.last().unwrap_or(&first_paragraph);
                r.push(last);
            }
        }
        TableGrid {
            rows,
            first_paragraph,
            grid_width: self.grid_width,
        }
    }
}

/// 单元格扫描累积态。
#[derive(Debug, Default)]
struct CellBuild {
    /// 首段落号（None = 该格无 `w:p`——畸形格，落栅格时占位）。
    paragraph: Option<usize>,
    /// `w:gridSpan/@w:val`（缺省 1）。
    grid_span: usize,
    /// `w:vMerge` 在场值。
    v_merge: Option<VMergeVal>,
    /// 所属表行的绝对行序（扫描期 `w:tr` 深度）——栅格分行依据。
    row_abs: usize,
}

/// `w:vMerge` 取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VMergeVal {
    Restart,
    Continue,
}

/// 取 `@w:val`（无命名空间前缀变体也认——`val`）。
fn attr_val(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned();
        if key == "val" {
            return Some(String::from_utf8_lossy(a.value.as_ref()).into_owned());
        }
    }
    None
}

/// 取 `w:gridSpan/@w:val`（解析失败/缺省 = 1）。
fn attr_span(e: &quick_xml::events::BytesStart<'_>) -> Option<usize> {
    attr_val(e)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.max(1))
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

/// #141 脚注/批注新值防线（与 check_edit_text 同口径——坐标面换 note id）。
fn check_note_text(kind: NotePartKind, id: usize, text: &str) -> Result<(), DocxError> {
    let what = match kind {
        NotePartKind::Footnotes => "footnote",
        NotePartKind::Comments => "comment",
    };
    for c in text.chars() {
        let cp = c as u32;
        if matches!(c, '\u{FFFE}' | '\u{FFFF}') {
            return Err(DocxError::InvalidInput(format!(
                "{what} id {id}: new text contains XML noncharacter U+{cp:04X}"
            )));
        }
        if cp < 0x20 && !matches!(c, '\t' | '\n' | '\r') {
            return Err(DocxError::InvalidInput(format!(
                "{what} id {id}: new text contains control character U+{cp:04X} (XML 1.0 illegal; allowed: \\t \\n \\r as tab/br)"
            )));
        }
    }
    Ok(())
}

/// #141 脚注/批注落账：条目内首 `w:t` Replace 新值（`emit_text_payload`
/// 同口径）、余 `w:t` Remove；`w:footnoteRef`/`w:annotationRef`/段落壳
/// 零接触。无任何编辑 = None（透传）。ops 坐标 = 部件 xml 坐标系
/// （条目互不嵌套、不同条目 `w:t` 域互斥——splice fail-closed 兜底）。
fn apply_note_edits(part: Option<&NotePart>, pending: &[Option<String>]) -> Option<String> {
    let part = part?;
    if pending.iter().all(|p| p.is_none()) {
        return None;
    }
    let mut ops: Vec<OpRec> = Vec::new();
    for (entry, val) in part.entries.iter().zip(pending.iter()) {
        let Some(value) = val.as_deref() else {
            continue;
        };
        if let Some((&first, rest)) = entry.wt_spans.split_first() {
            ops.push(OpRec {
                start: first.0,
                end: first.1,
                op: Op::Replace(emit_text_payload(value)),
            });
            for &(s, e) in rest {
                ops.push(OpRec {
                    start: s,
                    end: e,
                    op: Op::Remove,
                });
            }
        }
    }
    Some(splice(&part.xml, 0, ops).expect("note surgery ops disjoint"))
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
///
/// 两类编辑统一升序归并（段落 span 互不重叠——XML 良构保证；
/// 嵌套段冲突由 cursor 单调不变量 fail-closed 拒绝）：
/// · 文本编辑（既有 `set_paragraph_text`——段落树手术）；
/// · 域编辑（#140 `set_field_value`——结果区字节区间手术：
///   首 `w:t` Replace 新值、余 `w:t` Remove；fldChar/instrText/
///   separate/end 零 op——域结构零接触，Word/LibreOffice 打开
///   仍是活域）。
///
/// 同段互斥不变量：域段（`field_code`）拒文本编辑（既有防线），
/// 双 Some 同段 = 内部不变量破坏，fail-closed。
fn apply_all_edits(
    doc_xml: &str,
    paragraphs: &[ParagraphRef],
    text_pending: &[Option<String>],
    fields_idx: &[FieldScan],
    field_edits: &[Option<String>],
) -> Result<String, DocxError> {
    // 域编辑按段聚合（BTreeMap——begin_para 升序；同段多域保
    // fields_idx 序，段内 ops 无重叠[同段域结果区互斥，嵌套域已拒编辑]）。
    let mut field_by_para: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (pos, val) in field_edits.iter().enumerate() {
        if val.is_some() {
            field_by_para
                .entry(fields_idx[pos].begin_para)
                .or_default()
                .push(pos);
        }
    }
    let mut out = String::with_capacity(doc_xml.len() + 64);
    let mut cursor = 0usize;
    let mut idx = 0usize;
    while idx < paragraphs.len() || !field_by_para.is_empty() {
        // 双源归并取下一段（升序；文本源游标 idx 跳过 None 段）。
        let text_at = {
            let mut j = idx;
            while j < paragraphs.len() && text_pending[j].is_none() {
                j += 1;
            }
            if j < paragraphs.len() {
                Some(j)
            } else {
                None
            }
        };
        let field_at = field_by_para.keys().next().copied();
        // 取较小者（文本段与域段不可能同号——互斥不变量；此处防御
        // fail-closed）。无下一段即退出。
        let Some(target) = [text_at, field_at].into_iter().flatten().min() else {
            break;
        };
        let is_text = text_at == Some(target);
        let is_field = field_at == Some(target);
        if is_text && is_field {
            return Err(DocxError::Container(io::ReadError::InvalidPart {
                part: "word/document.xml".to_string(),
                detail: format!(
                    "paragraph {target}: text edit and field edit co-present \
                     (invariant broken — field paragraph refuses text edit)"
                ),
            }));
        }
        if is_text {
            idx = target + 1;
        } else {
            field_by_para.remove(&target);
        }
        let p = &paragraphs[target];
        if p.span.1 <= p.span.0 || p.span.1 == usize::MAX {
            return Err(DocxError::Container(io::ReadError::InvalidPart {
                part: "word/document.xml".to_string(),
                detail: format!("paragraph {target} span invalid ({:?})", p.span),
            }));
        }
        if p.span.0 < cursor {
            return Err(DocxError::Container(io::ReadError::InvalidPart {
                part: "word/document.xml".to_string(),
                detail: format!(
                    "paragraph {target} span overlaps prior edit (ordering invariant broken)"
                ),
            }));
        }
        let mut ops: Vec<OpRec> = Vec::new();
        if is_text {
            // 既有路径：段落树手术（多 run 合并/特殊节点保留）。
            let new_text = text_pending[target]
                .as_deref()
                .expect("is_text implies Some");
            let node = parse_element(&doc_xml[p.span.0..p.span.1], p.span.0)?;
            let mut ctx = SurgeryCtx {
                first_done: false,
                new_text,
            };
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
        } else {
            // #140 域路径：结果区字节区间手术（无树重解析——open 时
            // 建好的 result_spans 坐标直用；域结构节点零 op）。
            field_surgery_ops(fields_idx, field_edits, target, &mut ops);
        }
        out.push_str(&doc_xml[cursor..p.span.0]);
        out.push_str(&splice(&doc_xml[p.span.0..p.span.1], p.span.0, ops)?);
        cursor = p.span.1;
    }
    out.push_str(&doc_xml[cursor..]);
    Ok(out)
}

/// #140 域手术 ops：段 `para` 的全部待编辑域（fields_idx 文档序）——
/// 各域结果区**首** `w:t` Replace 新值（`emit_text_payload`：xml:space
/// 恒带/实体转义/`\t`→tab/`\n`→br——与段落文本编辑同口径），其余
/// 结果区 `w:t` Remove。fldChar/instrText/separate/end 零 op（域结构
/// 零接触——域在 Word/LibreOffice 打开仍是活域）。
/// `!editable` 防御跳过（set 面已拒——双保险）。
fn field_surgery_ops(
    fields_idx: &[FieldScan],
    field_edits: &[Option<String>],
    para: usize,
    ops: &mut Vec<OpRec>,
) {
    for (f, val) in fields_idx.iter().zip(field_edits.iter()) {
        let Some(value) = val.as_deref() else {
            continue;
        };
        if f.begin_para != para || !f.editable {
            continue;
        }
        if let Some((&first, rest)) = f.result_spans.split_first() {
            ops.push(OpRec {
                start: first.0,
                end: first.1,
                op: Op::Replace(emit_text_payload(value)),
            });
            for &(s, e) in rest {
                ops.push(OpRec {
                    start: s,
                    end: e,
                    op: Op::Remove,
                });
            }
        }
    }
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

/// #141 多部件重打包：document.xml 恒替换（在场检查既有）；footnotes/
/// comments 有编辑则替换，否则与全部非目标部件一起逐字节透传。
fn repack_parts(
    bytes: &[u8],
    document_xml: &[u8],
    footnotes_xml: Option<&[u8]>,
    comments_xml: Option<&[u8]>,
) -> Result<Vec<u8>, DocxError> {
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
        let replacement: Option<&[u8]> = if name == "word/document.xml" {
            has_document = true;
            Some(document_xml)
        } else if name == "word/footnotes.xml" {
            footnotes_xml
        } else if name == "word/comments.xml" {
            comments_xml
        } else {
            None
        };
        match replacement {
            Some(data) => {
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| DocxError::Io(format!("zip start: {e}")))?;
                writer
                    .write_all(data)
                    .map_err(|e| DocxError::Io(format!("zip write: {e}")))?;
            }
            None => {
                let entry = archive
                    .by_index_raw(i)
                    .map_err(|e| DocxError::Io(format!("zip index: {e}")))?;
                writer
                    .raw_copy_file(entry)
                    .map_err(|e| DocxError::Io(format!("zip copy: {e}")))?;
            }
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

    // ── #115 表格编辑：table_cells() 权威栅格 ────────────────────

    #[test]
    fn table_cells_plain_grid() {
        let body = "<w:p><w:r><w:t>Intro</w:t></w:r></w:p>\
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:p><w:r><w:t>Outro</w:t></w:r></w:p>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        // 文档序：0=Intro, 1..4=四格, 5=Outro。
        assert_eq!(ed.paragraphs().len(), 6);
        assert_eq!(ed.table_count(), 1);
        let cells = ed.table_cells();
        assert_eq!(cells, vec![vec![vec![1, 2], vec![3, 4]]], "权威行列坐标");
        assert_eq!(ed.table_shape(0), Some((2, 2)));
        assert_eq!(ed.table_first_paragraph(0), Some(1));
        // 栅格坐标里的段落号确实指向预期文本（不会静默改错格）。
        for (r, row) in cells[0].iter().enumerate() {
            for (c, para) in row.iter().enumerate() {
                let want = ["A1", "B1", "A2", "B2"][r * 2 + c];
                assert_eq!(ed.paragraph_text(*para), want, "[{r}][{c}]");
            }
        }
    }

    #[test]
    fn table_cells_gridspan_repeats_anchor() {
        // 第二行三格：A（1 列）+ B（gridSpan=2）。栅格宽 3。
        let body = "<w:tbl><w:tr>\
<w:tc><w:p><w:r><w:t>H1</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>H2</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>H3</w:t></w:r></w:p></w:tc></w:tr><w:tr>\
<w:tc><w:p><w:r><w:t>X</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr>\
<w:p><w:r><w:t>Y</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraphs().len(), 5);
        let cells = ed.table_cells();
        assert_eq!(
            cells,
            vec![vec![vec![0, 1, 2], vec![3, 4, 4]]],
            "gridSpan 覆盖列重复首格段落号（OOXML 占位语义）"
        );
        assert_eq!(ed.paragraph_text(cells[0][1][1]), "Y");
        assert_eq!(ed.paragraph_text(cells[0][1][2]), "Y", "覆盖列指回首格");
    }

    #[test]
    fn table_cells_vmerge_continue_points_to_restart() {
        // 2×2：左列 vMerge restart/continue，右列独立两格。
        let body = "<w:tbl><w:tr>\
<w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr>\
<w:p><w:r><w:t>R</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>P</w:t></w:r></w:p></w:tc></w:tr><w:tr>\
<w:tc><w:tcPr><w:vMerge/></w:tcPr>\
<w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>Q</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraphs().len(), 4);
        let cells = ed.table_cells();
        assert_eq!(
            cells,
            vec![vec![vec![0, 1], vec![0, 3]]],
            "vMerge continue 格指向起始格段落号（0），自身段落(2)不进栅格"
        );
        // 续格自己的段落仍是合法段落（可被 set 但不该被栅格驱动）。
        assert_eq!(ed.paragraph_text(2), "C");
    }

    #[test]
    fn table_cells_self_closing_cell_paragraph() {
        // 单元格里是自闭合 <w:p/>（空段落）——仍要有权威坐标。
        let body = "<w:tbl><w:tr><w:tc><w:p/></w:tc>\
<w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.table_cells(), vec![vec![vec![0, 1]]]);
        assert_eq!(ed.paragraph_text(0), "");
    }

    #[test]
    fn table_cells_multiple_tables_in_document_order() {
        let body = "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>T1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:p><w:r><w:t>Between</w:t></w:r></w:p>\
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>T2a</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>T2b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.table_count(), 2);
        assert_eq!(
            ed.table_cells(),
            vec![vec![vec![0]], vec![vec![2, 3]]],
            "表序号按文档序；中间正文段不进栅格"
        );
    }

    #[test]
    fn table_cells_nested_table_excluded_from_outer_grid() {
        // 外层表一格内嵌一张内层表：内层表的段落**不进**外层栅格
        // （猜坐标 = 静默改错格的防线）。
        let body = "<w:tbl><w:tr><w:tc>\
<w:p><w:r><w:t>Outer</w:t></w:r></w:p>\
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Inner</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
</w:tc></w:tr></w:tbl>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        assert_eq!(ed.paragraphs().len(), 2, "外层格段 + 内层格段");
        assert_eq!(
            ed.table_cells(),
            vec![vec![vec![0]]],
            "外层栅格只收外层格的首段（0）；内层段(1)不进栅格"
        );
        assert_eq!(ed.paragraph_text(1), "Inner");
    }

    #[test]
    fn table_cells_is_read_only_and_save_stays_identical() {
        // 查表不得动手术：open → table_cells() → save 字节恒等。
        let body = "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
        let bytes = build_docx(body);
        let ed = DocxEditor::open(&bytes).unwrap();
        let _ = ed.table_cells();
        let _ = ed.table_shape(0);
        let saved = ed.save_to_vec().unwrap();
        assert_eq!(saved_document_xml(&saved), saved_document_xml(&bytes));
    }

    #[test]
    fn table_cells_set_cell_text_keeps_tcpr_and_surrounding_bytes() {
        // 改一格文字：w:tcPr 原样、另一格字节不变、结构不变（第一版边界）。
        let body = "<w:p><w:r><w:t>Head</w:t></w:r></w:p>\
<w:tbl><w:tr>\
<w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr>\
<w:p><w:pPr><w:pStyle w:val=\"Default\"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>Old</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>Keep</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
        let bytes = build_docx(body);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let cells = ed.table_cells();
        let target = cells[0][0][0];
        assert_eq!(ed.paragraph_text(target), "Old");
        ed.set_paragraph_text(target, "New").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains("<w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr>"),
            "w:tcPr 原样保留：{xml}"
        );
        assert!(
            xml.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">New</w:t>"),
            "首 run 的 w:rPr 保留：{xml}"
        );
        // 未被编辑的另一格：**原文逐字节**保留（引擎不做任何手术——
        // 不加 xml:space、不拆 run）。
        assert!(
            xml.contains("<w:tc><w:p><w:r><w:t>Keep</w:t></w:r></w:p></w:tc>"),
            "另一格原文逐字节不变：{xml}"
        );
        // 结构不变：表/行/格数一致，重开仍可栅格化。
        let re = DocxEditor::open(&saved).unwrap();
        assert_eq!(re.table_cells(), cells);
        assert_eq!(re.paragraph_text(target), "New");
    }

    // ── #140 域码段编辑（fields/set_field_value）─────────────────

    fn fld_begin() -> &'static str {
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#
    }
    fn fld_sep() -> &'static str {
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#
    }
    fn fld_end() -> &'static str {
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#
    }
    fn instr(s: &str) -> String {
        format!(r#"<w:r><w:instrText xml:space="preserve">{s}</w:instrText></w:r>"#)
    }
    fn wt(s: &str) -> String {
        format!(r#"<w:r><w:t>{s}</w:t></w:r>"#)
    }

    /// 单段 PAGE 域：fields 列出 → set 显示值 → save →
    /// 域五段结构字节原样（Word 打开仍是活域）→ reopen 新值。
    #[test]
    fn field_page_edit_keeps_five_part_structure() {
        let body = format!(
            "{bb}{ii}{ss}{rr}{ee}",
            bb = fld_begin(),
            ii = instr(" PAGE \\* MERGEFORMAT "),
            ss = fld_sep(),
            rr = wt("3"),
            ee = fld_end()
        );
        let bytes = build_docx(&format!("<w:p>{body}</w:p>"));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        // 列出：段落号 + 类型 + 显示值。
        let fs = ed.fields();
        assert_eq!(fs.len(), 1, "一个 PAGE 域: {fs:?}");
        assert_eq!(fs[0].para, 0);
        assert_eq!(fs[0].field_idx, 0);
        assert_eq!(fs[0].kind, FieldKind::Page);
        assert_eq!(fs[0].instr, " PAGE \\* MERGEFORMAT ");
        assert_eq!(fs[0].display_value, "3");
        assert!(fs[0].editable && fs[0].same_para);
        // 编辑显示值（不碰结构）。
        ed.set_field_value(0, 0, "7").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        // 五段结构零接触（活域——非死文本）。
        assert!(
            xml.contains(r#"<w:fldChar w:fldCharType="begin"/>"#),
            "begin 原样: {xml}"
        );
        assert!(
            xml.contains("PAGE \\* MERGEFORMAT"),
            "instrText 原样: {xml}"
        );
        assert!(
            xml.contains(r#"<w:fldChar w:fldCharType="separate"/>"#),
            "separate 原样: {xml}"
        );
        assert!(
            xml.contains(r#"<w:fldChar w:fldCharType="end"/>"#),
            "end 原样: {xml}"
        );
        // 新值在场（xml:space 恒带）；旧值消失。
        assert!(
            xml.contains(r#"<w:t xml:space="preserve">7</w:t>"#),
            "新显示值: {xml}"
        );
        assert!(!xml.contains(">3</w:t>"), "旧显示值已换: {xml}");
        // reopen：域仍被识别为活域 + 新值。
        let ed2 = DocxEditor::open(&saved).unwrap();
        let fs2 = ed2.fields();
        assert_eq!(fs2.len(), 1);
        assert_eq!(fs2[0].kind, FieldKind::Page);
        assert_eq!(fs2[0].display_value, "7");
        assert!(fs2[0].editable, "保存后域仍可编辑（幂等链）");
    }

    /// 多 span 结果区（DATE 域跨 3 个 w:t run）：首 Replace、余 Remove。
    #[test]
    fn field_multi_span_result_first_replaced_rest_removed() {
        let body = format!(
            "{bb}{ii}{ss}{r1}{r2}{r3}{ee}",
            bb = fld_begin(),
            ii = instr(" DATE \\@ \"yyyy-MM-dd\" "),
            ss = fld_sep(),
            r1 = wt("2020"),
            r2 = wt("-06"),
            r3 = wt("-30"),
            ee = fld_end()
        );
        let bytes = build_docx(&format!("<w:p>{body}</w:p>"));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let fs = ed.fields();
        assert_eq!(fs[0].kind, FieldKind::Date);
        assert_eq!(fs[0].display_value, "2020-06-30", "三 span 拼接: {fs:?}");
        ed.set_field_value(0, 0, "2026-11-01").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains(r#"<w:t xml:space="preserve">2026-11-01</w:t>"#),
            "首 span 换新值: {xml}"
        );
        assert!(!xml.contains(">2020</w:t>"), "span1 旧值消失");
        assert!(!xml.contains(">-06</w:t>"), "span2 已移除");
        assert!(!xml.contains(">-30</w:t>"), "span3 已移除");
        // 域结构原样。
        assert!(xml.contains(r#"fldCharType="separate""#) && xml.contains(r#"fldCharType="end""#));
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.fields()[0].display_value, "2026-11-01");
    }

    /// TOC 跨段域 + 嵌套 PAGEREF：TOC 只读（跨段+嵌套双拒因）、
    /// 嵌套子域 PAGEREF 按 same_para 规则可编辑（规则一致性钉定——
    /// TOC 刷新覆盖是 Word 语义，结构保持是本面承诺）。
    #[test]
    fn field_toc_crosspara_readonly_pageref_nested_editable() {
        // 段2: TOC begin+instr+separate；段3: 条目文本 + PAGEREF 域；
        // 段4: TOC end。
        let toc_open = format!(
            "{bb}{ii}{ss}",
            bb = fld_begin(),
            ii = instr(" TOC \\o \"1-3\" \\h \\z \\u "),
            ss = fld_sep()
        );
        let pageref = format!(
            "{t}{bb}{ii}{ss}{one}{ee}",
            t = wt("第一章 概述 "),
            bb = fld_begin(),
            ii = instr(" PAGEREF _Toc1 \\h "),
            ss = fld_sep(),
            one = wt("1"),
            ee = fld_end()
        );
        let toc_close = format!("{t}{ee}", t = wt("第二章 方案 "), ee = fld_end());
        let bytes = build_docx(&format!(
            "<w:p>{toc_open}</w:p><w:p>{pageref}</w:p><w:p>{toc_close}</w:p>\
             <w:p><w:r><w:t>普通段</w:t></w:r></w:p>"
        ));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let fs = ed.fields();
        assert_eq!(fs.len(), 2, "TOC + PAGEREF: {fs:?}");
        // TOC：跨段只读。
        let toc = &fs[0];
        assert_eq!(toc.kind, FieldKind::Toc);
        assert_eq!(toc.para, 0);
        assert!(!toc.same_para, "begin 段0 / end 段2——跨段");
        assert!(!toc.editable);
        assert!(
            toc.display_value.contains("第一章 概述 ")
                && toc.display_value.contains("第二章 方案 "),
            "跨段结果区拼接（含条目文本）: {}",
            toc.display_value
        );
        // PAGEREF：嵌套子域，begin/end 同段3（数组序 1）——可编辑。
        let pr = &fs[1];
        assert_eq!(pr.kind, FieldKind::Custom("PAGEREF _Toc1 \\h".into()));
        assert_eq!(pr.para, 1);
        assert!(pr.same_para);
        assert!(pr.editable, "子域未标 nested（nested 只记在栈顶 TOC）");
        // TOC 拒编辑（跨段 typed 错）。
        let err = ed.set_field_value(0, 0, "x").unwrap_err();
        assert!(err.to_string().contains("cross-paragraph"), "跨段拒: {err}");
        // TOC 域段（0/2）整段文本替换也拒（field_code 既有防线）。
        for i in [0usize, 2] {
            assert!(ed.set_paragraph_text(i, "x").is_err(), "段{i} 拒整段替换");
        }
        // 条目段1：既拒整段替换（field_code——PAGEREF 在场），子域可编辑。
        assert!(ed.set_paragraph_text(1, "x").is_err());
        ed.set_field_value(1, 0, "12").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        // TOC 结构整体原样（begin/separate/end 计数 3+1）。
        assert_eq!(xml.matches(r#"fldCharType="begin""#).count(), 2);
        assert_eq!(xml.matches(r#"fldCharType="separate""#).count(), 2);
        assert_eq!(xml.matches(r#"fldCharType="end""#).count(), 2);
        assert!(
            xml.contains(" TOC \\o \"1-3\" \\h \\z \\u "),
            "TOC 指令原样"
        );
        assert!(xml.contains(" PAGEREF _Toc1 \\h "), "PAGEREF 指令原样");
        // PAGEREF 显示值 "1"→"12"；TOC 缓存条目文本原样。
        assert!(xml.contains(r#"<w:t xml:space="preserve">12</w:t>"#));
        assert!(!xml.contains(">1</w:t>"), "旧页码换新");
        assert!(xml.contains("第一章 概述 "), "TOC 缓存文本未动");
        assert!(xml.contains("第二章 方案 "));
    }

    /// 退化域（无 separate——WPS 形）：列出 + 拒编辑（不发明结构）。
    #[test]
    fn field_degenerate_no_separate_refuses_edit() {
        let body = format!(
            "{w}{bb}{ii}{ee}{t}",
            w = wt("字面前 "),
            bb = fld_begin(),
            ii = instr(" REF _Ref123456789 \\h "),
            ee = fld_end(),
            t = wt(" 字面后")
        );
        let bytes = build_docx(&format!("<w:p>{body}</w:p>"));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let fs = ed.fields();
        assert_eq!(fs.len(), 1);
        assert_eq!(fs[0].kind, FieldKind::Ref("_Ref123456789".into()));
        assert_eq!(fs[0].display_value, "", "无结果区——空显示值");
        assert!(fs[0].same_para);
        assert!(!fs[0].editable, "无 separate → 只读");
        let err = ed.set_field_value(0, 0, "x").unwrap_err();
        assert!(err.to_string().contains("separate"), "退化域拒: {err}");
        // 段落文本：字面 run 与指令区并存（指令不入正文——读面同口径）。
        assert_eq!(ed.paragraph_text(0), "字面前  字面后");
    }

    /// fldSimple 简单形：instr 属性 + 子层 w:t 结果区。
    #[test]
    fn field_fldsimple_instr_attr_and_editable() {
        let body = format!(
            r#"<w:fldSimple w:instr=" TIME \* MERGEFORMAT ">{t}</w:fldSimple>{tail}"#,
            t = wt("09:30"),
            tail = wt("（更新于）")
        );
        let bytes = build_docx(&format!("<w:p>{body}</w:p>"));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let fs = ed.fields();
        assert_eq!(fs.len(), 1);
        assert_eq!(fs[0].kind, FieldKind::Time);
        assert_eq!(fs[0].instr, " TIME \\* MERGEFORMAT ");
        assert_eq!(fs[0].display_value, "09:30");
        assert!(fs[0].editable);
        // fldSimple 属性原样约束下的显示值编辑。
        ed.set_field_value(0, 0, "23:59").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains(r#"<w:fldSimple w:instr=" TIME \* MERGEFORMAT "><w:r><w:t xml:space="preserve">23:59</w:t></w:r></w:fldSimple>"#),
            "fldSimple 属性/壳原样 + 新值: {xml}"
        );
        assert!(xml.contains("（更新于）"), "后续字面 run 未动");
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.fields()[0].display_value, "23:59");
        // 落款段文本面：fldSimple 结果 + 字面 tail 都入段落文本（既有口径）。
        assert_eq!(ed2.paragraph_text(0), "23:59（更新于）");
    }

    /// FieldKind 解析矩阵（首词切分 + REF 书签名 + 未映射 Custom）。
    #[test]
    fn field_kind_parsing_matrix() {
        use FieldKind::*;
        assert_eq!(field_kind_from_instr(" PAGE \\* MERGEFORMAT "), Page);
        assert_eq!(field_kind_from_instr("page"), Page, "大小写不敏感");
        assert_eq!(field_kind_from_instr("  NUMPAGES "), NumPages);
        assert_eq!(field_kind_from_instr("DATE \\@ \"yyyy-MM-dd\""), Date);
        assert_eq!(field_kind_from_instr("TOC \\o \"1-3\""), Toc);
        assert_eq!(
            field_kind_from_instr(" REF _Ref123 \\h "),
            Ref("_Ref123".into())
        );
        // 未映射：Custom 装原串（PAGEREF/PRINTDATE/IF 不发明映射）。
        assert_eq!(
            field_kind_from_instr(" PAGEREF _Toc9 \\h "),
            Custom(" PAGEREF _Toc9 \\h ".trim().into())
        );
        assert_eq!(
            field_kind_from_instr("PRINTDATE"),
            Custom("PRINTDATE".into())
        );
        assert_eq!(field_kind_from_instr(""), Custom("".into()));
        // 公开构造器透传。
        assert_eq!(FieldKind::from_instr("TIME"), Time);
        assert_eq!(FieldKind::from_instr("REF bm").instr(), "REF bm");
    }

    /// 域值防线：控制字符/非字符 typed 拒；合法 \t\n 放行（tab/br 发射）。
    #[test]
    fn field_value_defense_and_tab_br_emission() {
        let body = format!(
            "{bb}{ii}{ss}{rr}{ee}",
            bb = fld_begin(),
            ii = instr(" PAGE "),
            ss = fld_sep(),
            rr = wt("9"),
            ee = fld_end()
        );
        let bytes = build_docx(&format!("<w:p>{body}</w:p>"));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert!(
            ed.set_field_value(0, 0, "a\u{7}b").is_err(),
            "U+0007 控制字符拒"
        );
        assert!(
            ed.set_field_value(0, 0, "a\u{FFFE}b").is_err(),
            "U+FFFE 非字符拒"
        );
        // 合法：\t → <w:tab/>、\n → <w:br/>；实体转义。
        ed.set_field_value(0, 0, "A\tB\nC&D<E").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains(">A</w:t><w:tab/><w:t xml:space=\"preserve\">B</w:t><w:br/><w:t xml:space=\"preserve\">C&amp;D&lt;E</w:t>"),
            "tab/br/实体转义发射: {xml}"
        );
    }

    /// 混合保存：域编辑段 + 普通段文本编辑同 save（归并升序）。
    #[test]
    fn field_and_text_edits_merged_save() {
        let page = format!(
            "{bb}{ii}{ss}{rr}{ee}",
            bb = fld_begin(),
            ii = instr(" PAGE "),
            ss = fld_sep(),
            rr = wt("9"),
            ee = fld_end()
        );
        let bytes = build_docx(&format!(
            "<w:p>{page}</w:p><w:p><w:r><w:t>旧文本</w:t></w:r></w:p><w:p><w:r><w:t>尾段</w:t></w:r></w:p>"
        ));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        ed.set_field_value(0, 0, "5").unwrap();
        ed.set_paragraph_text(1, "新文本").unwrap();
        ed.set_paragraph_text(2, "改尾").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(
            xml.contains(r#"<w:t xml:space="preserve">5</w:t>"#),
            "域值: {xml}"
        );
        assert!(xml.contains(r#"<w:t xml:space="preserve">新文本</w:t>"#));
        assert!(xml.contains(r#"<w:t xml:space="preserve">改尾</w:t>"#));
        assert!(!xml.contains(">9</w:t>") && !xml.contains("旧文本"));
        // 未编辑区域零接触 + 幂等。
        let again = ed.save_to_vec().unwrap();
        assert_eq!(saved, again, "幂等——同状态重复 save 同字节");
    }

    /// 坐标防线：未知 (para, field_idx) typed 拒。
    #[test]
    fn field_unknown_coords_rejected() {
        let body = format!(
            "{bb}{ii}{ss}{rr}{ee}",
            bb = fld_begin(),
            ii = instr(" PAGE "),
            ss = fld_sep(),
            rr = wt("9"),
            ee = fld_end()
        );
        let bytes = build_docx(&format!("<w:p>{body}</w:p>"));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        assert!(ed.set_field_value(0, 1, "x").is_err(), "field_idx 越界拒");
        assert!(ed.set_field_value(9, 0, "x").is_err(), "para 越界拒");
        assert!(ed.set_field_value(1, 0, "x").is_err(), "无域段拒");
    }

    /// 空结果区（有 separate 但无 w:t）：拒编辑。
    #[test]
    fn field_empty_result_region_refuses() {
        let body = format!(
            "{bb}{ii}{ss}{ee}",
            bb = fld_begin(),
            ii = instr(" PAGE "),
            ss = fld_sep(),
            ee = fld_end()
        );
        let bytes = build_docx(&format!("<w:p>{body}</w:p>"));
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let fs = ed.fields();
        assert_eq!(fs[0].display_value, "");
        assert!(!fs[0].editable);
        let err = ed.set_field_value(0, 0, "x").unwrap_err();
        assert!(err.to_string().contains("empty result region"), "{err}");
    }

    // ── #141 批注脚注（footnotes/comments/set_*_text）─────────────

    /// notes 部件包 CONTENT_TYPES（footnotes/comments 两 Override）。
    const CONTENT_TYPES_NOTES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/><Override PartName=\"/word/comments.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/></Types>";

    /// notes 部件关系（footnotes/comments 两 Relationship——与黄金例
    /// 生成器同构）。
    const DOC_RELS_NOTES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments\" Target=\"comments.xml\"/></Relationships>";

    /// build_docx 的 notes 加形：footnotes.xml / comments.xml 两部件在场。
    fn build_docx_notes(body: &str, footnotes: &str, comments: &str) -> Vec<u8> {
        use std::io::Write as _;
        let document = document_wrap(body);
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in [
            ("[Content_Types].xml", CONTENT_TYPES_NOTES),
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", DOC_RELS_NOTES),
            ("word/document.xml", &document),
            ("word/footnotes.xml", footnotes),
            ("word/comments.xml", comments),
        ] {
            w.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// notes 部件原文（三实体脚注 + 孤儿 + 双批注——测试共用）。
    fn fixture_footnotes_xml() -> String {
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
            // 分隔符惯例（-1/0）——不入列。
            "<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>",
            "<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>",
            // id=1：单段双 w:t（多 span 形）。
            "<w:footnote w:id=\"1\"><w:p><w:r><w:footnoteRef/></w:r>",
            "<w:r><w:t>脚注一：机房</w:t></w:r><w:r><w:t>建设标准</w:t></w:r></w:p></w:footnote>",
            // id=2：双段。
            "<w:footnote w:id=\"2\"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>脚注二第一段</w:t></w:r></w:p>",
            "<w:p><w:r><w:t>脚注二第二段</w:t></w:r></w:p></w:footnote>",
            // id=3：孤儿（正文无引用）。
            "<w:footnote w:id=\"3\"><w:p><w:r><w:t>孤儿脚注</w:t></w:r></w:p></w:footnote>",
            "</w:footnotes>"
        ).into()
    }

    fn fixture_comments_xml() -> String {
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<w:comments xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
            "<w:comment w:id=\"701\" w:author=\"作者甲\" w:date=\"2026-11-01T09:00:00Z\">",
            "<w:p><w:r><w:annotationRef/></w:r><w:r><w:t>批注甲</w:t></w:r></w:p></w:comment>",
            "<w:comment w:id=\"702\" w:author=\"作者乙\">",
            "<w:p><w:r><w:t>批注乙第一段</w:t></w:r></w:p>",
            "<w:p><w:r><w:t>批注乙第二段</w:t></w:r></w:p></w:comment>",
            "</w:comments>"
        )
        .into()
    }

    /// 正文（段0：脚注1+批注701 锚；段1：脚注2 锚 + 尾注引用不入锚；
    /// 段2：普通段）。
    fn notes_body() -> String {
        concat!(
            "<w:p><w:r><w:t>正文一</w:t></w:r><w:commentRangeStart w:id=\"701\"/>",
            "<w:r><w:footnoteReference w:id=\"1\"/></w:r><w:commentRangeEnd w:id=\"701\"/>",
            "<w:r><w:commentReference w:id=\"701\"/></w:r></w:p>",
            "<w:p><w:r><w:footnoteReference w:id=\"2\"/></w:r>",
            "<w:r><w:endnoteReference w:id=\"9\"/></w:r></w:p>",
            "<w:p><w:r><w:t>普通段</w:t></w:r></w:p>"
        )
        .into()
    }

    fn part_text(bytes: &[u8], name: &str) -> String {
        String::from_utf8(part_bytes(bytes, name)).unwrap()
    }

    /// 列出面：实体入列 / 分隔符不入 / 锚坐标系 / 孤儿 None。
    #[test]
    fn notes_list_shapes_anchors_separator_exclusion() {
        let bytes = build_docx_notes(
            &notes_body(),
            &fixture_footnotes_xml(),
            &fixture_comments_xml(),
        );
        let ed = DocxEditor::open(&bytes).unwrap();
        // 正文段序只含 document.xml 段落（notes 部件内容段不进——部件级）。
        assert_eq!(ed.paragraphs().len(), 3);
        // 脚注：三实体（id 1/2/3）；分隔符 -1/0 不入列。
        let fns = ed.footnotes();
        assert_eq!(fns.len(), 3, "分隔符不入列: {fns:?}");
        assert_eq!(
            (
                fns[0].id,
                fns[0].text.as_str(),
                fns[0].anchor_para,
                fns[0].paras
            ),
            (1, "脚注一：机房建设标准", Some(0), 1),
            "多 span 拼接 + 锚段0: {fns:?}"
        );
        assert_eq!(
            (
                fns[1].id,
                fns[1].text.as_str(),
                fns[1].anchor_para,
                fns[1].paras
            ),
            (2, "脚注二第一段脚注二第二段", Some(1), 2),
            "双段拼接 + 锚段1: {fns:?}"
        );
        assert_eq!(
            (
                fns[2].id,
                fns[2].text.as_str(),
                fns[2].anchor_para,
                fns[2].paras
            ),
            (3, "孤儿脚注", None, 1),
            "孤儿脚注 anchor None（正文无引用）: {fns:?}"
        );
        // 批注：701 锚段0（commentRangeStart 首现）；702 孤儿 None。
        let cms = ed.comments();
        assert_eq!(cms.len(), 2, "{cms:?}");
        assert_eq!(
            (
                cms[0].id,
                cms[0].author.as_str(),
                cms[0].date.as_deref(),
                cms[0].text.as_str(),
                cms[0].anchor_para
            ),
            (
                701,
                "作者甲",
                Some("2026-11-01T09:00:00Z"),
                "批注甲",
                Some(0)
            ),
            "{cms:?}"
        );
        assert_eq!(
            (
                cms[1].id,
                cms[1].author.as_str(),
                cms[1].date.as_deref(),
                cms[1].text.as_str(),
                cms[1].anchor_para,
                cms[1].paras
            ),
            (702, "作者乙", None, "批注乙第一段批注乙第二段", None, 2),
            "孤儿批注 + 无日期属性: {cms:?}"
        );
        // endnoteReference（id 9）不入任何锚——尾注部件不在编辑面。
        // （可观察面：footnotes 列表无 9 号条目——上方已断言 3 实体。）
    }

    /// 编辑面：首 w:t Replace（xml:space/转义同口径）、余 w:t Remove、
    /// run/段落壳保留、footnoteRef/annotationRef/属性零接触。
    #[test]
    fn notes_edit_first_replace_rest_removed_shells_kept() {
        let bytes = build_docx_notes(
            &notes_body(),
            &fixture_footnotes_xml(),
            &fixture_comments_xml(),
        );
        let mut ed = DocxEditor::open(&bytes).unwrap();
        ed.set_footnote_text(2, "新脚注二 & <续>").unwrap();
        ed.set_comment_text(702, "新批注乙").unwrap();
        let saved = ed.save_to_vec().unwrap();
        // footnotes.xml：首 w:t 换新值（xml:space 恒带 + 实体转义）。
        let fx = part_text(&saved, "word/footnotes.xml");
        assert!(
            fx.contains("<w:t xml:space=\"preserve\">新脚注二 &amp; &lt;续&gt;</w:t>"),
            "首 span 换新值（转义同口径）: {fx}"
        );
        // 余 w:t 移除；run 壳/段落壳保留（最小侵入——空段占行）。
        assert!(!fx.contains("脚注二第一段"), "span1 旧值消失");
        assert!(!fx.contains("脚注二第二段"), "span2 已移除");
        assert!(
            fx.contains("<w:p><w:r></w:r></w:p>"),
            "残余空段壳保留: {fx}"
        );
        // footnoteRef / 分隔符 / 未编辑条目零接触。
        assert!(fx.contains("<w:footnoteRef/>"), "页底编号零接触: {fx}");
        assert!(
            fx.contains("<w:footnote w:type=\"separator\" w:id=\"-1\">"),
            "分隔符条目原样: {fx}"
        );
        assert!(fx.contains("脚注一：机房"), "未编辑条目原文零接触: {fx}");
        // comments.xml：作者属性不动 + 新值在场 + 余 span 移除。
        let cm = part_text(&saved, "word/comments.xml");
        assert!(cm.contains("w:author=\"作者乙\""), "作者零接触: {cm}");
        assert!(
            cm.contains("<w:t xml:space=\"preserve\">新批注乙</w:t>"),
            "{cm}"
        );
        assert!(
            !cm.contains("批注乙第一段") && !cm.contains("批注乙第二段"),
            "{cm}"
        );
        assert!(
            cm.contains("<w:p><w:r></w:r></w:p>"),
            "批注残余空段壳: {cm}"
        );
        // reopen：新值可读（显示面）+ 段数壳不变 + 幂等链。
        let ed2 = DocxEditor::open(&saved).unwrap();
        let fns2 = ed2.footnotes();
        assert_eq!(fns2[1].text, "新脚注二 & <续>", "reopen 解转义: {fns2:?}");
        assert_eq!(fns2[1].paras, 2, "段落壳保留——段数不变");
        let cms2 = ed2.comments();
        assert_eq!(cms2[1].text, "新批注乙");
        assert_eq!(cms2[1].paras, 2);
        assert_eq!(
            ed2.save_to_vec().unwrap(),
            saved,
            "reopen 再 save 同字节（幂等链）"
        );
    }

    /// 拒改面：部件缺席 / 未知 id / 空内容区 / 控制字符与非字符防线；
    /// 合法 \t\n 走 tab/br 发射（emit_text_payload 同口径）。
    #[test]
    fn notes_rejections_and_tab_br_emission() {
        // 部件缺席：空集 + typed 拒（不发明部件/内容）。
        let plain = build_docx("<w:p><w:r><w:t>正文</w:t></w:r></w:p>");
        let mut ed0 = DocxEditor::open(&plain).unwrap();
        assert!(ed0.footnotes().is_empty() && ed0.comments().is_empty());
        let e1 = ed0.set_footnote_text(1, "x").unwrap_err();
        assert!(e1.to_string().contains("part absent"), "{e1}");
        let e2 = ed0.set_comment_text(1, "x").unwrap_err();
        assert!(e2.to_string().contains("part absent"), "{e2}");
        // save 不发明部件（namelist 不变）。
        let names = |b: &[u8]| {
            zip::ZipArchive::new(std::io::Cursor::new(b))
                .unwrap()
                .file_names()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&ed0.save_to_vec().unwrap()), names(&plain));
        // 在场部件：空内容区（只有 footnoteRef 无 w:t）拒编辑。
        let footnotes = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
            "<w:footnote w:id=\"1\"><w:p><w:r><w:footnoteRef/></w:r></w:p></w:footnote>",
            "</w:footnotes>"
        );
        let comments = fixture_comments_xml();
        let bytes = build_docx_notes(&notes_body(), footnotes, &comments);
        let mut ed = DocxEditor::open(&bytes).unwrap();
        let fns = ed.footnotes();
        assert_eq!(fns.len(), 1, "{fns:?}");
        assert_eq!(fns[0].text, "");
        let e3 = ed.set_footnote_text(1, "x").unwrap_err();
        assert!(e3.to_string().contains("empty content region"), "{e3}");
        // 未知 id。
        let e4 = ed.set_footnote_text(9, "x").unwrap_err();
        assert!(e4.to_string().contains("not found"), "{e4}");
        let e5 = ed.set_comment_text(999, "x").unwrap_err();
        assert!(e5.to_string().contains("not found"), "{e5}");
        // 控制字符 / 非字符。
        let e6 = ed.set_comment_text(701, "a\u{0001}b").unwrap_err();
        assert!(e6.to_string().contains("control character"), "{e6}");
        let e7 = ed.set_comment_text(701, "\u{FFFF}").unwrap_err();
        assert!(e7.to_string().contains("noncharacter"), "{e7}");
        // 合法 \t\n → tab/br 发射（与 set_paragraph_text 同口径）。
        ed.set_comment_text(701, "甲\t乙\n丙").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let cm = part_text(&saved, "word/comments.xml");
        assert!(
            cm.contains("<w:t xml:space=\"preserve\">甲</w:t><w:tab/><w:t xml:space=\"preserve\">乙</w:t><w:br/><w:t xml:space=\"preserve\">丙</w:t>"),
            "tab/br 发射: {cm}"
        );
        // reopen：显示面 = w:t 拼接（tab/br 非 w:t 不入）。
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.comments()[0].text, "甲乙丙");
    }

    /// 透传面：无 notes 编辑 → 两部件逐字节透传；无文本编辑 →
    /// document.xml 同字节。
    #[test]
    fn notes_transparency_without_edits() {
        let bytes = build_docx_notes(
            &notes_body(),
            &fixture_footnotes_xml(),
            &fixture_comments_xml(),
        );
        let ed = DocxEditor::open(&bytes).unwrap();
        let saved = ed.save_to_vec().unwrap();
        assert_eq!(
            part_text(&saved, "word/footnotes.xml"),
            fixture_footnotes_xml()
        );
        assert_eq!(
            part_text(&saved, "word/comments.xml"),
            fixture_comments_xml()
        );
        assert_eq!(
            part_text(&saved, "word/document.xml"),
            String::from_utf8(part_bytes(&bytes, "word/document.xml")).unwrap()
        );
    }

    /// 混合落账：域编辑 + 脚注编辑 + 批注编辑 + 普通段文本，一次 save
    /// 四面全中（#140 与 #141 共用一条流水线——合并后的核心不变量）。
    #[test]
    fn notes_fields_text_mixed_one_save() {
        let body = format!(
            "<w:p>{bb}{ii}{ss}{rr}{ee}<w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
             <w:p><w:r><w:t>旧正文</w:t></w:r></w:p>",
            bb = fld_begin(),
            ii = instr(" PAGE \\* MERGEFORMAT "),
            ss = fld_sep(),
            rr = wt("3"),
            ee = fld_end()
        );
        let bytes = build_docx_notes(&body, &fixture_footnotes_xml(), &fixture_comments_xml());
        let mut ed = DocxEditor::open(&bytes).unwrap();
        // 域段拒整段替换（既有防线在 notes 在场时同样生效）。
        assert!(ed.set_paragraph_text(0, "x").is_err());
        ed.set_field_value(0, 0, "7").unwrap();
        ed.set_footnote_text(1, "新脚注一").unwrap();
        ed.set_comment_text(701, "新批注甲").unwrap();
        ed.set_paragraph_text(1, "新正文").unwrap();
        let saved = ed.save_to_vec().unwrap();
        // document.xml：域五段原样 + 域新值 + 文本新值。
        let xml = saved_document_xml(&saved);
        assert!(xml.contains(r#"<w:fldChar w:fldCharType="begin"/>"#));
        assert!(xml.contains("PAGE \\* MERGEFORMAT"));
        assert!(xml.contains(r#"<w:t xml:space="preserve">7</w:t>"#));
        assert!(xml.contains(r#"<w:t xml:space="preserve">新正文</w:t>"#));
        assert!(!xml.contains(">3</w:t>") && !xml.contains("旧正文"));
        // footnotes.xml：脚注新值；comments.xml：批注新值。
        assert!(part_text(&saved, "word/footnotes.xml").contains("新脚注一"));
        assert!(part_text(&saved, "word/comments.xml").contains("新批注甲"));
        // reopen：四面全中。
        let ed2 = DocxEditor::open(&saved).unwrap();
        assert_eq!(ed2.fields()[0].display_value, "7");
        assert_eq!(ed2.footnotes()[0].text, "新脚注一");
        assert_eq!(ed2.comments()[0].text, "新批注甲");
        assert_eq!(ed2.comments()[0].author, "作者甲");
        assert_eq!(ed2.paragraph_text(1), "新正文");
        // 幂等。
        assert_eq!(ed2.save_to_vec().unwrap(), saved);
    }
    // ── #156：sdt 容器段落旗标（标书 e2e 对账的地基）────────────

    /// 标书导出链真形：docx-rs 的 TOC 域 = `<w:sdt><w:sdtContent>`
    /// 包裹两个纯域脚手段（begin+instrText+separate 一段 / end 一段）。
    /// 引擎必须逐段计数且打 `in_sdt`（宽面读门结构性丢弃 sdt 内容——
    /// 消费方据此对账，见 [`ParagraphRef::in_sdt`] 文档）。
    #[test]
    fn sdt_wrapped_toc_paragraphs_flagged_and_counted() {
        let body = concat!(
            r#"<w:p><w:r><w:t>标题</w:t></w:r></w:p>"#,
            r#"<w:sdt><w:sdtPr><w:rPr /></w:sdtPr><w:sdtContent>"#,
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin" w:dirty="true"/>"#,
            r#"<w:instrText>TOC \o "1-3" \h \u</w:instrText>"#,
            r#"<w:fldChar w:fldCharType="separate" w:dirty="false"/></w:r></w:p>"#,
            r#"<w:p><w:r><w:fldChar w:fldCharType="end" w:dirty="false"/></w:r></w:p>"#,
            r#"</w:sdtContent></w:sdt>"#,
            r#"<w:p><w:r><w:t>第一章 正文</w:t></w:r></w:p>"#,
        );
        let mut ed = DocxEditor::open(&build_docx(body)).unwrap();
        let paras = ed.paragraphs();
        assert_eq!(paras.len(), 4, "sdt 内两段必须计数（宽面丢弃 ≠ 引擎丢弃）");
        assert!(!paras[0].in_sdt && !paras[0].field_code);
        // sdt 内两段：in_sdt + 域脚手（跨段 TOC——域面只读保护对象）。
        assert!(paras[1].in_sdt && paras[1].field_code);
        assert!(paras[2].in_sdt && paras[2].field_code);
        assert!(!paras[3].in_sdt);
        // 域面：TOC 跨段只读（editable=false——既有语义不因 sdt 改变）。
        let f = &ed.fields()[0];
        assert!(matches!(f.kind, FieldKind::Toc));
        assert!(!f.editable && !f.same_para);
        assert!(ed.set_field_value(f.para, f.field_idx, "X").is_err());
        // sdt 段照常拒整段替换（域段防线）；落账零接触（字节恒等往返）。
        assert!(ed.set_paragraph_text(1, "x").is_err());
        assert!(ed.set_paragraph_text(2, "x").is_err());
        ed.set_paragraph_text(3, "改后正文").unwrap();
        let saved = ed.save_to_vec().unwrap();
        let xml = saved_document_xml(&saved);
        assert!(xml.contains("<w:sdt>"), "sdt 容器字节保真");
        assert!(xml.contains(r#"<w:instrText>TOC"#), "TOC 域指令保真");
        assert!(xml.contains("改后正文"));
    }

    /// sdt 包裹**表格**：单元格段 in_sdt && in_table（消费方整体拒改的
    /// 信号——栅格块序与宽面块序无法对齐）。
    #[test]
    fn sdt_wrapped_table_cells_flagged_in_table() {
        let cell =
            |t: &str| format!(r#"<w:tc><w:tcPr/><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>"#);
        let body = format!(
            concat!(
                r#"<w:tbl><w:tr>{}{}</w:tr></w:tbl>"#,
                r#"<w:sdt><w:sdtPr/><w:sdtContent>"#,
                r#"<w:tbl><w:tr>{}{}</w:tr></w:tbl>"#,
                r#"</w:sdtContent></w:sdt>"#,
            ),
            cell("A1"),
            cell("B1"),
            cell("S1"),
            cell("S2"),
        );
        let ed = DocxEditor::open(&build_docx(&body)).unwrap();
        let paras = ed.paragraphs();
        // 外层表 2 格 + sdt 表 2 格 = 4 段。
        assert_eq!(paras.len(), 4);
        assert!(paras[0].in_table && !paras[0].in_sdt);
        assert!(paras[1].in_table && !paras[1].in_sdt);
        assert!(paras[2].in_table && paras[2].in_sdt, "sdt 表内段双旗标");
        assert!(paras[3].in_table && paras[3].in_sdt);
        // 两表都进引擎栅格（原文坐标系不区分 sdt——消费方负责拒改面）。
        assert_eq!(ed.table_count(), 2);
    }
}
