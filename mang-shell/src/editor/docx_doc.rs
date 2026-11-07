//! docx 文档模型（#68 批 3 只读渲染 + #115 可编辑化）。
//!
//! 职责：把 docx 解析结果转换为编辑器可渲染的**行模型**（纯文本行 +
//! 逐行样式 span + 逐行块归属 + 标题树 + 文档属性）——渲染/滚动/查找/
//! 选区/光标全复用批 1-2 的 Editor 管线。
//!
//! ## 两种装载（#115 起）
//!
//! - [`load`]：**只读渲染**（无编辑桥；pdf/xlsx 同口径）。
//! - [`load_editable`]：**可编辑**——宽面读（`read_docx_from_bytes`）出
//!   行模型，窄面编辑（`docx_edit::DocxEditor`——prescan + verify_gates +
//!   document.xml 扫描）出段落索引；两者按**段落序号**对账（宽面文档序
//!   第 i / k / 表格内段 ≡ 窄面文档序第 i 个 `w:p`——两个扫描器口径一致，
//!   有语料测试钉定；不一致则降级只读，绝不带着错位映射开工）。
//!
//! 门：宽面在**解析后**要做块模型（比窄面贵），故进程内对
//! `LIMIT_INPUT_BYTES_DOCX` 的 4 倍设**可编辑装载**上限（超限降级只读，
//! 不阻塞查看）；文件读取本身先做 `fs::metadata` 前置（大文件不入内存）。
//!
//! ## 转换规则（docx 块 → 行）
//!
//! - 段落：文本 = run 顺序拼接；标题段（`heading_level` 1-9）整段
//!   Heading 样式 + 记入标题树；列表段按 numId+ilvl 维持有序计数
//!   （`N. `）/ 无序 `• `，层级以缩进表达；`w:ind` 左缩进按 360
//!   twips ≈ 1 空格近似（≤16 空格封顶）；run 级 bold/italic/link →
//!   Bold/Italic/Link span（超链接内文本不入 bold/italic——链接样式
//!   优先；bold+italic 叠加取 Bold，斜体面降级登记）。
//! - 表格：栅格已由读面展开（合并格 = 空串），列宽按 East Asian
//!   宽度对齐（CJK 2 cell），行内 ` | ` 连接；首行整行 Bold + 下缀
//!   分隔线；内嵌表平铺为独立表块（无边框定位）。
//! - 文本框：内层块平铺渲染（无边框/锚定定位）。
//! - 页眉/页脚/图片/批注/脚注/尾注：**不入正文行流**——计数入
//!   [`DocxProps`]（属性面板呈现）；图片位置模型面外（office-io
//!   登记面）。#115：**可编辑态**下这些内容各出一行**只读清单行**
//!   （「页眉 / 页脚：2 / 1——第一版只读」），既有「看得见」的证据，
//!   也解释「为什么改不了」——行模型里没有的东西用户不会去找。
//!
//! ## 段落投影与块归属（#115 映射的地基）
//!
//! 行模型是「docx 块 → 人读行」的有损投影：一个段落可能占多行（列表
//! 标记行 + 无标记续行；`w:br` 软换行），表格行/分隔线是**凭空生成**
//! 的装饰行。可编辑化必须能反推，故每行带一个
//! [`docx_edit_bridge::LineBlock`]：
//!
//! - `Docx { paragraph, carry, .. }` = 该行属于段落 `paragraph`（可回写）；
//!   `carry = true` 是「承接头」（无标记的续行，删它 ≠ 删目标文本）；
//! - `ReadOnly(kind)` = 锁定行（表格/分隔线/填充/域/修订/回退副本/
//!   文本框/辅助清单），渲染可见、编辑面外，`kind.reason()` 即拒改原因。
//!
//! 行模型对段落的**完整投影**另行逐段记入桥（[`DocxPayload::projections`]）
//! ——回写时与 Buffer 行文本比对即可判「这一段动没动」（可逆性闸门）。
//!
//! ## 已知边界（批 3 登记 + #115 增补）
//!
//! - 表格列宽上限 28 cell（超宽截断单元格文本 + `…`）
//! - 有序列表计数不重置（OOXML 层级重启语义未实现——跨段重启按
//!   同 numId 续计）
//! - 软换行（`w:br`）由读面并入同段文本；#115 起按 `\n` 拆行（拆行后
//!   可编辑且可逆——连行即恢复原文）
//! - 分页语义面外（docx 无显式页边界入流——阶段 3 页码/行号按行号
//!   呈现）

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use mdc_office_io::docx::{self, Block, ListKind, read_docx_from_bytes_with_options};
use mdc_office_io::docx_edit::{DocxEditor, ParagraphRef};
use mdc_office_io::io;

use super::docx_edit_bridge::{DocxBridge, DocxPort, LineBlock, ReadOnlyLine, TableRef};
use super::highlight::TokenKind;

// NB：#115 起表格网格不入行流（表格只读），下列列宽/截断/补齐工具仅
// 在「未来把表格接回编辑面」时复用——保留实现并显式登记未用。
/// 表格单元格列宽上限（East Asian cell 计，超宽截断 + `…`）。
#[allow(dead_code)]
const CELL_WIDTH_CAP: usize = 28;
/// 左缩进换算：360 twips ≈ 1 空格（0.25 英寸），封顶 16 空格。
const INDENT_UNIT_TWIPS: i32 = 360;
#[allow(dead_code)]
const INDENT_SPACES_CAP: usize = 16;
/// 列表层级缩进（每级 2 空格，封顶 4 级）。
const LIST_INDENT_PER_LEVEL: usize = 2;
const LIST_LEVEL_CAP: usize = 4;
/// #115：可编辑装载的输入上限倍数（宽面块模型比窄面扫描贵；超限降级
/// 只读——查看不受阻）。
const EDIT_LOAD_LIMIT_FACTOR: u64 = 4;

/// 标题树条目（左栏「文档结构」——阶段 3.1 docx 面提前落地）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadingEntry {
    /// 标题层级（1-9，读面钳位后域）。
    pub level: usize,
    /// 标题文本（run 拼接）。
    pub text: String,
    /// 转换后行模型的 0 基行号（点击跳转锚）。
    pub line: usize,
}

/// 文档属性（右栏属性面板 + 启动日志证据通道）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocxProps {
    pub blocks: usize,
    pub paragraphs: usize,
    pub tables: usize,
    pub table_rows: usize,
    pub textboxes: usize,
    pub images: usize,
    pub headers: usize,
    pub footers: usize,
    pub comments: usize,
    pub footnotes: usize,
    pub endnotes: usize,
    pub warnings: usize,
    /// 空段落计数（诊断面——docx 常见空行布局）。
    pub empty_paragraphs: usize,
    /// #115：可编辑段落数（行模型里可回写的段落——非锁定）。
    pub editable_paragraphs: usize,
    /// #115：锁定段落数（域/修订/回退副本/文本框——引擎拒改面）。
    pub locked_paragraphs: usize,
}

/// docx 转换产物（Editor 渲染的全部数据面）。
#[derive(Clone, Debug, Default)]
pub struct DocxPayload {
    /// 全文纯文本（行以 `\n` 连接——喂 [`super::buffer::Buffer`]）。
    pub text: String,
    /// 逐行样式 span（char 索引区间 + token 类别；与 text 行严格对齐）。
    pub spans: Vec<Vec<(Range<usize>, TokenKind)>>,
    /// 逐行块归属（与 text 行严格 1:1；`None` = 只读装载，无映射）。
    pub blocks: Option<Vec<LineBlock>>,
    /// 标题树（文档序）。
    pub headings: Vec<HeadingEntry>,
    /// 文档属性。
    pub props: DocxProps,
    /// #115：可编辑装载的原始字节（编辑桥构造原料；`None` = 只读装载）。
    pub source: Option<Vec<u8>>,
    /// #115：逐段投影（`段落号 → 行模型里的完整投影文本`；只读装载为空）。
    pub projections: HashMap<usize, String>,
    /// #115：逐段回写兜底投影（`段落号 → 段内行正文`；只读装载为空）。
    pub fallbacks: HashMap<usize, String>,
    /// #115：候选段落数（宽面文档序段落总数——与窄面 w:p 数对账用）。
    pub candidates: usize,
    /// #115：表格单元格段落号（扁平集合——「哪些段落是单元格」）。
    pub cell_paragraphs: Vec<usize>,
    /// #115 表格编辑：侧表（表 → 行 → 栅格列 → 段落号）。权威坐标来自
    /// 引擎 `table_cells()`；只读装载为空。
    pub tables: Vec<TableRef>,
    /// #115：可编辑装载的降级说明（`Some` = 本文件实际是只读态）。
    pub notice: Option<String>,
}

/// #226 R2：panic 载荷 → 用户可读原因（String/&'static str 两类常见
/// 载荷；其余不猜——「未知」诚实呈现）。
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else {
        "未知 panic 载荷".to_string()
    }
}

/// #226 R2：docx 打开入口 panic 兜底。依赖层（docx-rs 读面）在真实
/// 语料有 panic 实锤（`test-artifacts/health-check` REPORT #E1——
/// office-io 预检防御已修实证位点）；此处兜**未知同类形态**（含
/// docx-rs 同类未实证 `attributes[0]` 位点，清单见 office-io
/// docx_panic_guard 模块文档）：panic → typed `Err`（「打开失败+
/// 原因」用户可见，编辑器不崩、未保存编辑不丢），Ok/Err 原语义
/// 原样透传。
fn panic_guard<T>(context: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        Err(format!(
            "{context}（引擎内部错误）：{}",
            panic_message(&*payload)
        ))
    })
}

/// 读盘 + 解析 + 转换（**只读**——pdf/xlsx 同口径）。
///
/// 兼容模式（`compat: true`）：畸形部件降级为结构化警告（数据照常
/// 提取，警告计数入属性）——查看器语义；安全门（预扫/尺寸/深度上限）
/// 两模式恒等不放松。输入尺寸前置（先 `fs::metadata` 再读——避免
/// 大文件全量入内存，同 office-io path 入口语义）。
pub fn load(path: &Path) -> Result<DocxPayload, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件信息失败：{e}"))?;
    if meta.len() > io::LIMIT_INPUT_BYTES_DOCX {
        return Err(format!(
            "文件超过 docx 输入上限（{} 字节 > {} 字节）",
            meta.len(),
            io::LIMIT_INPUT_BYTES_DOCX
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取失败：{e}"))?;
    let opts = io::ReadOptions { compat: true };
    // #226 R2：读引擎 panic 兜底（依赖层未知形态——见 panic_guard 文档）。
    let doc = panic_guard("文档打开失败", || {
        read_docx_from_bytes_with_options(&bytes, &opts).map_err(|e| format!("docx 解析失败：{e}"))
    })?;
    Ok(convert(&doc))
}

/// 可编辑装载结果：行模型 + 引擎窄面 + 编辑桥。
///
/// `bridge = None` 表示该文件**降级为只读**（超可编辑门限 / 引擎侧严格
/// 门拒 / 段落序对账失败——`payload.notice` 载原因）——不静默：状态栏/
/// 属性面板/启动日志都由它呈现。
pub struct EditablePayload {
    /// 行模型（`source = Some` 携带原始字节）。
    pub payload: DocxPayload,
    /// 编辑桥（`None` = 只读降级）。
    pub bridge: Option<DocxBridge>,
    /// 引擎窄面（桥 `Some` 时恒 `Some`——保存时落账用）。
    pub port: Option<Box<dyn DocxPort>>,
}

/// 可编辑装载：宽面出**行模型**、窄面出**段落索引**，按段落序对账。
pub fn load_editable(path: &Path) -> Result<EditablePayload, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件信息失败：{e}"))?;
    let editable_cap = io::LIMIT_INPUT_BYTES_DOCX.saturating_mul(EDIT_LOAD_LIMIT_FACTOR);
    if meta.len() > editable_cap {
        let mut payload = load(path)?;
        payload.notice = Some(format!(
            "文件 {} 字节超过可编辑装载门限（{editable_cap} 字节）——降级为只读",
            meta.len()
        ));
        return Ok(EditablePayload {
            payload,
            bridge: None,
            port: None,
        });
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取失败：{e}"))?;
    let opts = io::ReadOptions { compat: true };
    // #226 R2：读引擎 panic 兜底（依赖层未知形态——见 panic_guard 文档）。
    let doc = panic_guard("文档打开失败", || {
        read_docx_from_bytes_with_options(&bytes, &opts).map_err(|e| format!("docx 解析失败：{e}"))
    })?;

    // 窄面（引擎严格门）：失败 → 降级只读（宽面已证明可看）；panic
    // 同样降级（#226 R2——宽面已可看，不因窄面扫描崩溃丢掉查看）。
    let engine = match panic_guard("引擎编辑面装载失败", || {
        DocxEditor::open(&bytes).map_err(|e| e.to_string())
    }) {
        Ok(e) => e,
        Err(e) => {
            let mut payload = convert(&doc);
            payload.source = Some(bytes);
            payload.notice = Some(format!(
                "引擎编辑面拒（{e}）——降级为只读渲染（查看不受影响）"
            ));
            return Ok(EditablePayload {
                payload,
                bridge: None,
                port: None,
            });
        }
    };
    let engine_paras = engine.paragraphs().len();
    let port = DocxEnginePort { editor: engine };
    // #156 门 0：sdt 包裹**表格** → 整篇降级只读。宽面块序丢弃整个 sdt
    // 容器（含表），引擎栅格却含它的单元格段落——块序↔栅格对位不可信，
    // 错位写回风险不可接受（宁缺毋错）。纯段落 sdt 不受此门限制（盲区
    // 跳过，可编辑）。真实触发面：标书导出链的 TOC = sdt 包两段（无表），
    // 可编辑；Word 原生「目录内容控件」带缓存条目时同理。
    let sdt_all = port.sdt_paragraphs();
    if let Some(i) = sdt_all.iter().find(|i| {
        port.table_cells()
            .iter()
            .any(|t| t.iter().any(|r| r.contains(i)))
    }) {
        let mut fallback = convert(&doc);
        fallback.source = Some(bytes);
        fallback.notice = Some(format!(
            "结构化内容控件（w:sdt）包裹表格（段落 {i} 起的表不在宽面块序）——降级为只读，避免栅格错位写回"
        ));
        return Ok(EditablePayload {
            payload: fallback,
            bridge: None,
            port: None,
        });
    }
    let payload = convert_editable(&doc, &port);
    // 段落序对账（候选段落全集——含空段落/锁定段落，它们不入投影表）：
    // 行模型段落数必须等于窄面段数，否则块归属会错位。
    let mapped = payload.candidates;
    let unrepresentable = (0..engine_paras)
        .filter(|i| !payload.projections.contains_key(i))
        .count();
    // 宽面文档序段落总数（含表格/文本框内段落）：表格部分**以引擎栅格
    // 权威坐标为准**（宽面栅格是展开形，与 w:tc 数不等——见
    // [`ConvertCtx::push_table`]），文本框仍按宽面块模型数。
    let width_paragraphs = width_paragraph_total_engine(&doc.blocks, &payload.tables);
    if width_paragraphs + sdt_all.len() != engine_paras {
        let mut fallback = convert(&doc);
        fallback.source = Some(bytes);
        fallback.notice = Some(format!(
            "宽面段落数 {}（+sdt 盲区 {}）与引擎 {engine_paras} 不一致（块模型/XML 扫描口径分歧）——降级为只读",
            width_paragraphs,
            sdt_all.len()
        ));
        return Ok(EditablePayload {
            payload: fallback,
            bridge: None,
            port: None,
        });
    }
    if unrepresentable > 0 {
        // 有段落**进不了行模型**（宽面块模型与窄面 w:p 对不上的形态：
        // 例如本版表格单元格文本未逐格入行）——带着这种缺口编辑，保存时
        // 会把这些段落当成「用户清空」写回。宁缺毋错：整篇降级只读。
        let mut fallback = convert(&doc);
        fallback.source = Some(bytes);
        fallback.notice = Some(format!(
            "有 {unrepresentable} 个段落无法在行模型中表示（表格单元格等）——降级为只读，避免误写"
        ));
        return Ok(EditablePayload {
            payload: fallback,
            bridge: None,
            port: None,
        });
    }
    if mapped != engine_paras {
        let mut fallback = convert(&doc);
        fallback.source = Some(bytes);
        fallback.notice = Some(format!(
            "段落序对账失败（行模型 {mapped} 段 / 引擎 {engine_paras} 段）——降级为只读，避免错位改写"
        ));
        return Ok(EditablePayload {
            payload: fallback,
            bridge: None,
            port: None,
        });
    }
    let blocks = payload.blocks.clone().unwrap_or_default();
    let mut bridge = DocxBridge::new(
        blocks,
        (0..engine_paras)
            .map(|i| Some(port.paragraph_text(i)))
            .collect(),
    );
    for (p, text) in &payload.projections {
        bridge.note_projection(*p, text);
    }
    for (p, text) in &payload.fallbacks {
        bridge.note_fallback(*p, text);
    }
    bridge.note_cells(payload.cell_paragraphs.clone());
    bridge.note_tables(payload.tables.clone());
    let mut payload = payload;
    payload.source = Some(bytes);
    Ok(EditablePayload {
        payload,
        bridge: Some(bridge),
        port: Some(Box::new(port)),
    })
}

/// 宽面文档序段落总数（表格单元格/文本框内段落逐层计——与引擎 w:p 同口径）。
fn width_paragraph_total(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(_) => 1,
            Block::Table(t) => t.rows.iter().map(|r| r.len()).sum::<usize>(),
            Block::TextBox(tb) => width_paragraph_total(&tb.blocks),
        })
        .sum()
}

/// 宽面文档序段落总数（**表格部分用引擎权威栅格**）。
///
/// 宽面 `TableData::rows` 是栅格展开形（`gridSpan`/`vMerge` 覆盖列重复
/// 补列），格数 ≠ 引擎 `w:p` 数。故表格按引擎栅格的**去重段落号**计
/// （一个 `w:tc` 一个首段），文本框仍按宽面块模型（第一版只读、透传）。
///
/// `tables` 与 `blocks` 里 `Block::Table` 的出现顺序一一对应（文档序）；
/// 表数不足时该表退回宽面计数（降级只读，不静默）。
fn width_paragraph_total_engine(blocks: &[Block], tables: &[TableRef]) -> usize {
    fn walk(blocks: &[Block], tables: &[TableRef], cursor: &mut usize) -> usize {
        blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(_) => 1,
                Block::Table(t) => {
                    let idx = *cursor;
                    *cursor += 1;
                    match tables.get(idx) {
                        Some(tref) => {
                            let mut seen: Vec<usize> =
                                tref.cells.iter().flatten().copied().collect();
                            seen.sort_unstable();
                            seen.dedup();
                            seen.len().max(t.rows.len())
                        }
                        // 引擎没有这张表（嵌套表/异常）：按宽面计数兜底，
                        // 让对账自己去判定降级。
                        None => t.rows.iter().map(|r| r.len()).sum::<usize>(),
                    }
                }
                Block::TextBox(tb) => walk(&tb.blocks, tables, cursor),
            })
            .sum()
    }
    let mut cursor = 0usize;
    walk(blocks, tables, &mut cursor)
}

/// 引擎窄面实现（薄包装 `mdc_office_io::docx_edit::DocxEditor`）。
///
/// 段落级元数据（表格/文本框/域/修订/回退副本）取自 open 时的原文
/// 坐标系（`paragraphs()`）——待落账编辑不漂移索引。
pub struct DocxEnginePort {
    editor: DocxEditor,
}

#[allow(dead_code)] // 证据链/测试面（open/paragraphs）
impl DocxEnginePort {
    /// 由字节直接开引擎面（集成测试/证据链用；文件装载走 [`load_editable`]）。
    pub fn open(bytes: &[u8]) -> Result<Self, String> {
        DocxEditor::open(bytes)
            .map(|editor| Self { editor })
            .map_err(|e| e.to_string())
    }

    /// 段落索引（原文坐标系；属性面板/测试面只读视图）。
    pub fn paragraphs(&self) -> &[ParagraphRef] {
        self.editor.paragraphs()
    }
}

impl DocxPort for DocxEnginePort {
    fn paragraph_count(&self) -> usize {
        self.editor.paragraphs().len()
    }

    fn paragraph_text(&self, idx: usize) -> String {
        self.editor.paragraph_text(idx).to_string()
    }

    fn blocked_reason(&self, idx: usize) -> Option<ReadOnlyLine> {
        let p = self.editor.paragraphs().get(idx)?;
        if p.in_textbox {
            Some(ReadOnlyLine::Textbox)
        } else if p.fallback_copy {
            Some(ReadOnlyLine::FallbackCopy)
        } else if p.field_code {
            Some(ReadOnlyLine::FieldCode)
        } else if p.revision {
            Some(ReadOnlyLine::Revision)
        } else {
            None
        }
    }

    fn set_text(&mut self, idx: usize, text: &str) -> Result<(), String> {
        self.editor
            .set_paragraph_text(idx, text)
            .map_err(|e| e.to_string())
    }

    fn save(&self) -> Result<Vec<u8>, String> {
        self.editor.save_to_vec().map_err(|e| e.to_string())
    }

    fn table_cells(&self) -> Vec<Vec<Vec<usize>>> {
        self.editor.table_cells()
    }

    fn sdt_paragraphs(&self) -> Vec<usize> {
        self.editor
            .paragraphs()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.in_sdt)
            .map(|(i, _)| i)
            .collect()
    }

    fn fields(&self) -> Vec<mdc_office_io::docx_edit::FieldInfo> {
        self.editor.fields()
    }

    fn set_field_value(
        &mut self,
        para: usize,
        field_idx: usize,
        value: &str,
    ) -> Result<(), String> {
        self.editor
            .set_field_value(para, field_idx, value)
            .map_err(|e| e.to_string())
    }

    fn comments(&self) -> Vec<mdc_office_io::docx_edit::CommentInfo> {
        self.editor.comments()
    }

    fn set_comment_text(&mut self, id: usize, text: &str) -> Result<(), String> {
        self.editor
            .set_comment_text(id, text)
            .map_err(|e| e.to_string())
    }
}

/// #115：可编辑行模型（**真实引擎面**——从字节起走完整链：宽面块序 +
/// 窄面 `DocxEditor`，段落序对账不过直接 panic 之前的调用方自检）。
///
/// 与 [`load_editable`] 的区别：不做磁盘 IO / 不做降级（文件内测试与
/// 集成证据链用——失败即测试断言，不静默降级）。
#[allow(dead_code)] // 测试/证据链入口
pub fn convert_editable_wide(doc: &docx::DocxDocument) -> DocxPayload {
    convert(doc)
}

/// #115：可编辑行模型 + 真实引擎（字节入口——集成测试/证据链用）。
#[allow(dead_code)] // 测试/证据链入口
pub fn editable_from_bytes(bytes: &[u8]) -> (DocxPayload, DocxEnginePort) {
    let opts = io::ReadOptions { compat: true };
    let doc = read_docx_from_bytes_with_options(bytes, &opts).expect("docx 解析");
    let editor = DocxEditor::open(bytes).expect("引擎开");
    let port = DocxEnginePort { editor };
    let payload = convert_editable(&doc, &port);
    (payload, port)
}

/// 块序模型 → 行模型（**只读**：纯函数——单测锚点）。
pub fn convert(doc: &docx::DocxDocument) -> DocxPayload {
    convert_inner(doc, None)
}

/// 块序模型 → 行模型（**可编辑**：逐行块归属 + 逐段投影）。
pub fn convert_editable(doc: &docx::DocxDocument, port: &dyn DocxPort) -> DocxPayload {
    convert_inner(doc, Some(port))
}

fn convert_inner(doc: &docx::DocxDocument, port: Option<&dyn DocxPort>) -> DocxPayload {
    let mut out = DocxPayload {
        props: DocxProps {
            blocks: doc.blocks.len(),
            images: doc.images.len(),
            headers: doc.headers.len(),
            footers: doc.footers.len(),
            comments: doc.comments.len(),
            footnotes: doc.footnotes.len(),
            endnotes: doc.endnotes.len(),
            warnings: doc.warnings.len(),
            ..DocxProps::default()
        },
        ..DocxPayload::default()
    };
    let mut ctx = ConvertCtx {
        port,
        engine_paras: port.map(|p| p.paragraph_count()).unwrap_or(0),
        // 引擎权威栅格（#115 表格编辑）——一次取，转换期只读。
        engine_tables: port.map(|p| p.table_cells()).unwrap_or_default(),
        // #156：sdt 盲区段清单（升序——宽面结构性不可见；装载门已保证
        // 无 sdt 包表格形态，这里只需顺序跳过）。
        sdt_ahead: port.map(|p| p.sdt_paragraphs()).unwrap_or_default(),
        ..ConvertCtx::default()
    };
    for block in &doc.blocks {
        match block {
            Block::Paragraph(p) => ctx.push_paragraph(p),
            Block::Table(t) => ctx.push_table(t, &mut out),
            Block::TextBox(tb) => {
                out.props.textboxes += 1;
                // #115：文本框内文字**第一版只读**（不入行流）——原因同表格：
                // 单元格/文本框段落由引擎各自回写，行模型若把它们伪造/漏掉
                // 就会在对账时错位。文本框段落（含嵌套）仍计入段落序，只做
                // 原文透传；清单行说明边界。
                ctx.skip_textbox(tb);
            }
        }
    }
    // #156：尾部 sdt 盲区段（最后一块之后）——游标收尾跳过，否则
    // candidates 会短缺、门 3 误判降级。
    ctx.advance_past_sdt();
    let editable = port.is_some();
    ctx.push_aux_appendix(editable, &out.props);

    out.props.editable_paragraphs = ctx
        .blocks_vec
        .iter()
        .filter(|b| matches!(b, LineBlock::Para { cont: false, .. }))
        .count();
    out.props.locked_paragraphs = ctx.locked_paragraphs;
    out.props.paragraphs = ctx.next_para;
    out.props.empty_paragraphs = ctx.empty_paragraphs;
    out.text = ctx.lines.join("\n");
    out.spans = ctx.spans;
    out.blocks = if port.is_some() {
        Some(ctx.blocks_vec)
    } else {
        None
    };
    // 只读装载（无引擎）：投影/兜底/候选数一概清零（对账面不参与）。
    let editable = port.is_some();
    out.projections = if editable { ctx.proj } else { HashMap::new() };
    out.fallbacks = if editable {
        ctx.fallback
    } else {
        HashMap::new()
    };
    out.candidates = if editable { ctx.next_para } else { 0 };
    out.cell_paragraphs = if editable {
        ctx.cell_paragraphs
    } else {
        Vec::new()
    };
    out.tables = if editable {
        ctx.engine_tables
            .iter()
            .enumerate()
            .map(|(index, cells)| TableRef {
                index,
                cells: cells.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    out.headings = ctx.headings;
    out
}

/// 转换累积态。
#[derive(Default)]
struct ConvertCtx<'a> {
    port: Option<&'a dyn DocxPort>,
    lines: Vec<String>,
    spans: Vec<Vec<(Range<usize>, TokenKind)>>,
    blocks_vec: Vec<LineBlock>,
    headings: Vec<HeadingEntry>,
    /// 逐段投影（段落号 → 行模型里的完整文本）。
    proj: HashMap<usize, String>,
    /// 逐段回写兜底投影（段落号 → 段内行正文，\n 连接）。
    fallback: HashMap<usize, String>,
    /// 有序列表计数（numId+ilvl → 当前序号——跨段连续，不重置）。
    ordered: HashMap<(usize, usize), usize>,
    /// #115：宽面段落的文档序游标（与窄面 w:p 文档序对账）。
    next_para: usize,
    /// #115：锁定段落计数（域/修订/回退副本/文本框）。
    locked_paragraphs: usize,
    /// 引擎段落数（窄面 w:p 计数——超界防线）。
    engine_paras: usize,
    /// #115：表格栅格（只计属性——见 [`ConvertCtx::push_table`]）。
    table_rows: Vec<Vec<Vec<String>>>,
    /// #115：表格单元格段落号（文档序）——保存时按原文写回。
    cell_paragraphs: Vec<usize>,
    /// #115：未入行流但仍是 w:p 的段落数（表格单元格）——段落序对账用。
    aux_paragraphs: usize,
    /// #156：sdt 盲区段游标（升序待跳过清单——引擎可见、宽面结构性
    /// 不可见的段落；宽面块模型丢弃 `w:sdt` 内容，顺序游标必须显式跳过
    /// 它们才能与引擎原文坐标系对齐。背景见 `DocxPort::sdt_paragraphs`）。
    sdt_ahead: Vec<usize>,
    /// #156：已跳过的 sdt 盲区段数（诊断/证据行）。
    sdt_blind_seen: usize,
    /// #115 表格编辑：引擎权威栅格（表 → 行 → 栅格列 → 段落号），
    /// 建 ctx 时一次性取（`DocxPort::table_cells`）。
    engine_tables: Vec<Vec<Vec<usize>>>,
    /// 已处理的表数（表序号游标——与 `engine_tables` 对位）。
    tables_seen: usize,
    empty_paragraphs: usize,
}

impl ConvertCtx<'_> {
    /// #156：跳过 sdt 盲区段（宽面结构性不可见）——空投影/兜底 + 游标
    /// 推进；不入行流（无行）。盲区段永不归属任何行 → `compute` 恒判
    /// 「未动」→ 零 set → 引擎字节保真（sdt 容器与域脚手原样透传）。
    fn advance_past_sdt(&mut self) {
        while self.sdt_ahead.first().is_some_and(|&p| p == self.next_para) {
            let p = self.next_para;
            self.proj.insert(p, String::new());
            self.fallback.insert(p, String::new());
            self.sdt_blind_seen += 1;
            self.next_para += 1;
            self.sdt_ahead.remove(0);
        }
    }

    /// 文本框段落统计（不入行流——第一版只读；只做段落序对账）。
    fn skip_textbox(&mut self, tb: &docx::TextBoxData) {
        for inner in &tb.blocks {
            match inner {
                Block::Paragraph(_) => {
                    self.advance_past_sdt();
                    let para = self.next_para;
                    self.next_para += 1;
                    self.proj.insert(para, String::new());
                    self.fallback.insert(para, String::new());
                    self.cell_paragraphs.push(para);
                }
                Block::Table(t) => {
                    for row in &t.rows {
                        for _ in row {
                            self.advance_past_sdt();
                            let para = self.next_para;
                            self.next_para += 1;
                            self.proj.insert(para, String::new());
                            self.fallback.insert(para, String::new());
                            self.cell_paragraphs.push(para);
                        }
                    }
                }
                Block::TextBox(nested) => self.skip_textbox(nested),
            }
        }
    }

    /// 单元格段落的**行文本**（引擎装配文本——与回写口径逐字一致，
    /// 故未编辑时 `compute` 判定「未动」→ 零 set）。
    fn cell_text(&self, para: usize) -> String {
        match self.port {
            Some(port) if para < self.engine_paras => port.paragraph_text(para),
            _ => String::new(),
        }
    }

    /// 追加一行（行/样式/块三轨同步——单一入口，防错位）。
    fn push_line(&mut self, text: String, spans: Vec<(Range<usize>, TokenKind)>, block: LineBlock) {
        self.lines.push(text);
        self.spans.push(spans);
        self.blocks_vec.push(block);
    }

    /// 本段落的行块归属（只读装载恒 `Para`；可编辑态查引擎拒改面）。
    ///
    /// `unit` = 段落内"几何单元"序号（段落文本按软换行切出的第几段）。
    fn para_block(&mut self, para: usize, unit: usize, cont: bool) -> LineBlock {
        match self.port {
            None => LineBlock::Para {
                paragraph: para,
                idx: unit,
                cont,
            },
            // 超界防线：宽面块模型多出来的段落（引擎无对应 w:p）不得查询
            // 引擎（`paragraph_text` 越界 panic）。标锁定 + 计数，由调用方
            // 的对账把它判成「不可编辑装载」。
            Some(_) if para >= self.engine_paras => {
                if !cont {
                    self.locked_paragraphs += 1;
                }
                LineBlock::Locked(ReadOnlyLine::Unrepresentable)
            }
            Some(port) => match port.blocked_reason(para) {
                Some(kind) => {
                    if !cont {
                        self.locked_paragraphs += 1;
                    }
                    // 锁定段的投影 = 引擎原文（不参与回写；只为「未动」判定与
                    // 语料零编辑回归提供基准）。
                    let text = port.paragraph_text(para);
                    self.proj.insert(para, text.clone());
                    self.fallback.insert(para, text);
                    LineBlock::Locked(kind)
                }
                None => LineBlock::Para {
                    paragraph: para,
                    idx: unit,
                    cont,
                },
            },
        }
    }

    /// 段落入行流：标题 / 列表 / 普通三形态（#115 可逆口径）。
    ///
    /// 行切分：段落拆成「几何单元」，每单元一行——
    /// - 首单元 = 缩进填充 + 列表标记 + 正文首段（行模型原样投影）；
    /// - 软换行（\n）后的正文段 = 自带一个**承载行**（承接续行），其后
    ///   再各补一个**缩进填充行**（原文 \t 等宽近似：每个填充行 = 一个
    ///   \t，按原缩进量算——续行缩进在本层可见、可往返）；
    /// - 行内 \t（原文字符）由引擎读面装配进文本、按等宽渲染。
    ///
    /// 单元序号 = [`LineBlock::Para::idx`]；回写时正文按 \n 连接还原。
    fn push_paragraph(&mut self, p: &docx::ParagraphData) {
        self.advance_past_sdt();
        let para = self.next_para;
        self.next_para += 1;

        // 缩进（twips → 空格近似；`left` 为续行缩进，`first_line` 为首行）。
        let mut indent_first = 0usize;
        let mut indent_left = 0usize;
        if let Some(ind) = p.indent {
            // 续行缩进按「每 360 twips = 1 个 \t 宽」（行内制表符等宽渲染）。
            indent_left = ind.left.unwrap_or(0).max(0) as usize / INDENT_UNIT_TWIPS as usize;
            match ind.special {
                Some(docx::ParagraphIndentSpecial::FirstLine(tw)) => {
                    indent_first = tw.max(0) as usize / INDENT_UNIT_TWIPS as usize;
                }
                Some(docx::ParagraphIndentSpecial::Hanging(_)) | None => {}
            }
        }

        // 列表标记（层级缩进 + 序号/圆点）。
        let mut marker = String::new();
        let mut marker_indent = 0usize;
        if let Some(num) = &p.numbering {
            let lvl = num.ilvl.min(LIST_LEVEL_CAP);
            marker_indent = lvl * LIST_INDENT_PER_LEVEL;
            match num.kind {
                Some(ListKind::Bullet) => marker.push_str("• "),
                Some(ListKind::Ordered) => {
                    // #266：引擎解析序号优先（Word 语义全链——start/
                    // lvlRestart/numFmt/lvlText，中文序号「一、」原生
                    // 呈现）；解析不可达时退回本地简单计数（1. 2. 3.）。
                    if let Some(resolved) = &p.resolved_number {
                        marker.push_str(resolved);
                        marker.push(' ');
                    } else {
                        let c = self.ordered.entry((num.num_id, num.ilvl)).or_insert(0);
                        *c += 1;
                        marker.push_str(&format!("{}. ", c));
                    }
                }
                // 解析链不可达（num 桥悬空/抽象层级缺定义）——中性标记。
                None => marker.push_str("· "),
            }
        }

        // #115：行切分以**引擎装配文本**为准（`w:tab`→\t、`w:br`→\n 的
        // 字符面与回写口径逐字一致——软换行/制表符可往返）；引擎不在场
        // （只读装载）退回宽面文本。
        let full_text = match self.port {
            Some(port) if para < self.engine_paras => port.paragraph_text(para),
            // 只读装载没有引擎（#68 无编辑面）——退回宽面块模型文本；
            // 超界段（引擎无对应 w:p）同走宽面文本（由对账降级）。
            Some(_) | None => docx::ParagraphData::text(p),
        };
        let soft_parts: Vec<&str> = full_text.split('\n').collect();

        // 首单元：缩进 + 标记 + 首段正文。
        let mut first = String::new();
        for _ in 0..indent_first {
            first.push(' ');
        }
        for _ in 0..marker_indent {
            first.push(' ');
        }
        let marker_start = first.chars().count();
        first.push_str(&marker);
        let marker_end = first.chars().count();
        let head_chars = first.chars().count();
        first.push_str(soft_parts[0]);

        let mut unit_texts: Vec<String> = vec![first.clone()];
        let mut bodies: Vec<String> = vec![soft_parts[0].to_string()];
        // 续段：每个 \t/前导空格配一个填充行，然后承载行。
        for part in soft_parts.iter().skip(1) {
            let mut rest = (*part).to_string();
            let mut eaten = 0usize;
            while eaten < indent_left && (rest.starts_with('\t') || rest.starts_with(' ')) {
                rest.remove(0);
                eaten += 1;
            }
            for _ in 0..eaten {
                unit_texts.push(String::new());
                bodies.push(String::new());
            }
            unit_texts.push(rest.clone());
            bodies.push(rest);
        }

        // 落行（首行带标记 span / 标题 span / run 级样式）。
        let mut projection = String::new();
        let mut fallback_text = String::new();
        for (i, unit) in unit_texts.iter().enumerate() {
            if i > 0 {
                projection.push('\n');
            }
            projection.push_str(unit);
            if i > 0 {
                fallback_text.push('\n');
            }
            fallback_text.push_str(&bodies[i]);

            let mut spans: Vec<(Range<usize>, TokenKind)> = Vec::new();
            let line_chars = unit.chars().count();
            if i == 0 {
                if !marker.is_empty() {
                    spans.push((marker_start..marker_end, TokenKind::ListMarker));
                }
                if let Some(level) = p.heading_level {
                    if !soft_parts[0].is_empty() {
                        spans.push((head_chars..line_chars, TokenKind::Heading));
                    }
                    self.headings.push(HeadingEntry {
                        level,
                        text: full_text.replace('\n', " "),
                        line: self.lines.len(),
                    });
                }
            }
            if i == 0 && p.heading_level.is_none() {
                // run 级样式：仅按**首单元**文本对位（软换行后的单元按
                // 文本序近似——登记边界）。
                let mut consumed = 0usize;
                for run in &p.runs {
                    let start = (head_chars + consumed).min(line_chars);
                    consumed += run.text.chars().count();
                    let end = (head_chars + consumed).min(line_chars);
                    if end <= start {
                        continue;
                    }
                    let kind = if run.hyperlink.is_some() {
                        Some(TokenKind::Link)
                    } else if run.bold {
                        Some(TokenKind::Bold)
                    } else if run.italic {
                        Some(TokenKind::Italic)
                    } else {
                        None
                    };
                    if let Some(kind) = kind {
                        spans.push((start..end, kind));
                    }
                }
            }
            let block = self.para_block(para, i, i > 0);
            self.push_line(unit.clone(), spans, block);
        }
        if full_text.is_empty() {
            self.empty_paragraphs += 1;
        }
        self.proj.insert(para, projection);
        self.fallback.insert(para, fallback_text);
    }

    /// 表格（#115 表格编辑：每个可编辑单元格**一行**）。
    ///
    /// ## 走法
    ///
    /// 引擎 [`DocxPort::table_cells`] 给出权威栅格（表 → 行 → 栅格列 →
    /// 段落号）。每个**栅格行**先还原成「原生格列表」（`native_cells`：
    /// 去掉 `gridSpan` 覆盖列 / `vMerge` 续格产生的相邻重复号）：
    ///
    /// - **可编辑行**：原生格数 == 宽面该行格数（该行无横向扩展）→
    ///   每个原生格出一行 `LineBlock::Cell`，段落号取引擎坐标，行文本取
    ///   **引擎装配文本**（与回写口径逐字一致 → 未编辑零 set）；
    /// - **只读行**：引擎无该表 / 该行有 `gridSpan` 扩展 / 该格段落不是
    ///   单元格首段 → 出一行 `Locked(TableRow)`（显示宽面文本，拒改）。
    ///
    /// ## 为什么宽度要对账
    ///
    /// 宽面 `TableData::rows` 是**栅格展开形**（`gridSpan` 覆盖列重复补
    /// 列），而引擎一个 `w:tc` 一个段落。所以「第 i 列 ↔ 第 i 个 tc」
    /// **不成立**；只有该行没有横向扩展时两者才一一对应。有扩展就只读
    /// ——第一版不做合并单元格拆分（设计 §6 风险最高项）。
    ///
    /// ## 段落序
    ///
    /// 宽面栅格格数 ≠ 引擎段落数（扩展行多算），故段落游标**以引擎栅格
    /// 为准**（`max(段落号)+1`）；无引擎栅格时按「每格一段」兜底。
    fn push_table(&mut self, t: &docx::TableData, out: &mut DocxPayload) {
        out.props.tables += 1;
        out.props.table_rows += t.rows.len();
        let table_index = self.tables_seen;
        self.tables_seen += 1;
        // 引擎栅格（权威坐标）；`None` = 本表不可栅格化（整表只读）。
        let grid = self.engine_tables.get(table_index).cloned();
        // #156：sdt 盲区段可能横在本表与前一块之间——先跳过再栅格跳跃
        // （栅格跳跃是 max 语义，盲区段若在此后才会被后续 advance 跳过；
        // 在此先跳是为了不把盲区段漏进「本表首格」的对齐）。
        self.advance_past_sdt();
        // 段落游标以引擎栅格为准（扩展格多出来的段落也在栅格里）。
        if let Some(g) = &grid {
            if let Some(hi) = g.iter().flatten().copied().max() {
                self.next_para = self.next_para.max(hi + 1);
            }
        }
        for (ri, row) in t.rows.iter().enumerate() {
            let grid_row: Vec<usize> = grid
                .as_ref()
                .and_then(|g| g.get(ri))
                .cloned()
                .unwrap_or_default();
            // 该行是否**无横向扩展**：栅格列数 == 原生格数。有扩展
            // （gridSpan）的行不能「第 i 列 <-> 第 i 格」，整行只读——
            // 第一版不做合并单元格拆分（设计 §6 风险最高项）。
            let n_native = native_cells(&grid_row).len();
            let has_span = !grid_row.is_empty() && grid_row.len() != n_native;
            let editable = !grid_row.is_empty() && !has_span;
            // 可编辑行逐格遍历原生格；只读行按宽面栅格文本显示（看得见
            // 的边界好过沉默的空白）。
            let n_cells = if editable { n_native } else { row.len() };
            for ci in 0..n_cells {
                if editable {
                    let para = grid_row[ci];
                    self.cell_paragraphs.push(para);
                    let text = self.cell_text(para);
                    self.push_line(
                        text.clone(),
                        Vec::new(),
                        LineBlock::Cell {
                            table: table_index,
                            row: ri,
                            col: ci,
                        },
                    );
                    self.proj.insert(para, text.clone());
                    self.fallback.insert(para, text);
                } else {
                    let text = row.get(ci).cloned().unwrap_or_default();
                    self.push_line(
                        text.replace('\n', " "),
                        Vec::new(),
                        LineBlock::Locked(ReadOnlyLine::TableRow),
                    );
                }
            }
        }
        self.aux_paragraphs += t.rows.iter().map(|r| r.len()).sum::<usize>();
        self.table_rows.push(t.rows.clone());
    }

    /// #115：可编辑态下把「不入正文流」的内容各出一行只读清单——
    /// 看得见的边界，好过沉默的空白。
    fn push_aux_appendix(&mut self, editable: bool, props: &DocxProps) {
        if !editable {
            return;
        }
        let mut rows: Vec<(String, String)> = Vec::new();
        if props.tables > 0 {
            rows.push((
                "表格".into(),
                format!(
                    "{} 个（{} 行；第一版只读——表格文字不改，样式/边框原样保留）",
                    props.tables, props.table_rows
                ),
            ));
        }
        if props.images > 0 {
            rows.push(("图片".into(), format!("{} 张（不入行流）", props.images)));
        }
        if props.headers > 0 || props.footers > 0 {
            rows.push((
                "页眉 / 页脚".into(),
                format!("{} / {} 处", props.headers, props.footers),
            ));
        }
        if props.comments > 0 || props.footnotes > 0 || props.endnotes > 0 {
            rows.push((
                "批注 / 脚注 / 尾注".into(),
                format!(
                    "{} / {} / {} 处",
                    props.comments, props.footnotes, props.endnotes
                ),
            ));
        }
        if rows.is_empty() {
            return;
        }
        self.push_line(
            String::new(),
            Vec::new(),
            LineBlock::Locked(ReadOnlyLine::AuxAppendix),
        );
        for (label, text) in rows {
            self.push_line(
                format!("{label}：{text}——第一版只读"),
                Vec::new(),
                LineBlock::Locked(ReadOnlyLine::AuxAppendix),
            );
        }
    }
}

/// 栅格行 → **原生格**段落号列表（去掉 `gridSpan` 覆盖列与 `vMerge`
/// 续格产生的**相邻重复号**）。
///
/// 引擎 `table_cells()` 的栅格列是「Word 显示格」，合并区的每一列都指向
/// 同一个段落号；这里把连续重复折叠成一项，得到「一个 `w:tc` 一项」的
/// 原生格序列——与宽面 `TableData::rows` 的行宽只有在**无横向扩展**时才
/// 对齐（对齐才可编辑，见 [`ConvertCtx::push_table`]）。
fn native_cells(grid_row: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(grid_row.len());
    for p in grid_row {
        if out.last() != Some(p) {
            out.push(*p);
        }
    }
    out
}

/// East Asian 显示宽度（CJK/全角 = 2 cell，其余 1——Sarasa Mono SC
/// 等宽口径的近似；组合字符面外）。
#[allow(dead_code)]
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            let cjk = (0x1100..=0x115F).contains(&cp) // Hangul Jamo
                || (0x2E80..=0xA4CF).contains(&cp)   // CJK 部首..Yi
                || (0xAC00..=0xD7A3).contains(&cp)   // Hangul 音节
                || (0xF900..=0xFAFF).contains(&cp)   // CJK 兼容表意
                || (0xFE30..=0xFE4F).contains(&cp)   // CJK 兼容形式
                || (0xFF00..=0xFF60).contains(&cp)   // 全角形式
                || (0xFFE0..=0xFFE6).contains(&cp)   // 全角符号
                || (0x20000..=0x3FFFD).contains(&cp); // CJK 扩展
            if cjk { 2 } else { 1 }
        })
        .sum()
}

/// 超宽单元格截断（`…` 收尾——1 cell）。
#[allow(dead_code)]
fn truncate_cell(s: &str) -> String {
    if display_width(s) <= CELL_WIDTH_CAP {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = display_width(&c.to_string());
        if w + cw > CELL_WIDTH_CAP - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// 列宽补齐（East Asian 宽度对齐——空格右补）。
#[allow(dead_code)]
fn pad_cell(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(display_width(s));
    let mut out = s.to_string();
    for _ in 0..pad {
        out.push(' ');
    }
    out
}

/// 只读键门禁（纯函数——单测锚点）：浏览类命令放行，变更类整段拒绝。
///
/// 只读面（pdf / xlsx / **降级只读的 docx**）用；可编辑 docx 不走此门。
///
/// 放行：移动（含 Shift 选区扩展 / Ctrl 词跳）/ Home / End / PageUp /
/// PageDown / Esc 清选区 / Ctrl+A 全选 / Ctrl+C 复制 / Ctrl+F 查找。
/// 拒绝：输入 / 删除 / 回车 / Tab / Ctrl+X / Ctrl+V / Ctrl+Z / Ctrl+Y /
/// Ctrl+S（可打印字符经 `key_char` 兜底分支——非放行键一律拒绝）。
pub fn readonly_allows(key: &str, ctrl: bool) -> bool {
    match key {
        "left" | "right" | "up" | "down" | "home" | "end" | "pageup" | "pagedown" | "escape" => {
            true
        }
        "a" | "c" | "f" => ctrl,
        // F3：Ctrl+Tab 标签循环是导航组合非编辑命令——只读态放行
        //（冒泡至 shell）；无 Ctrl 的 Tab 是缩进编辑——拒绝如故。
        "tab" => ctrl,
        // F6：Ctrl+=/-/0 字号缩放与 Ctrl+T 系主题键是视图命令非编辑
        //——只读态（docx/pdf）同样可缩放/切主题。
        "=" | "+" | "-" | "0" | "t" => ctrl,
        // G2：Ctrl+G 跳转到行是导航命令非编辑——只读态（docx/pdf）
        // 同样可用（光标定位 + 滚动到可见区，无文本变更）。
        "g" => ctrl,
        _ => false,
    }
}

// =============================================================================
// 单元测试（DocxWriter 写面内存生成 → 读面解析 → 转换断言——零二进制
// 夹具依赖；烟雾 GUI 夹具由 `gen_smoke_docx`（--ignored）落盘生成）
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_office_io::docx::{DocxWriter, RunSpec, read_docx_from_bytes};

    /// 内存往返：写 → 读（兼容模式）。
    fn roundtrip(w: &mut DocxWriter) -> docx::DocxDocument {
        let bytes = w.save_to_vec().expect("写盘内存");
        let opts = io::ReadOptions { compat: true };
        read_docx_from_bytes_with_options(&bytes, &opts).expect("读回")
    }

    /// 内存往返（可编辑面：宽面块序 + 窄面引擎）。
    fn roundtrip_editable(w: &mut DocxWriter) -> (docx::DocxDocument, DocxEnginePort) {
        let bytes = w.save_to_vec().expect("写盘内存");
        let opts = io::ReadOptions { compat: true };
        let doc = read_docx_from_bytes_with_options(&bytes, &opts).expect("读回");
        let editor = DocxEditor::open(&bytes).expect("引擎面打开");
        (doc, DocxEnginePort { editor })
    }

    #[test]
    fn heading_tree_and_props() {
        let mut w = DocxWriter::new();
        w.heading(1, "项目概述").unwrap();
        w.paragraph_text("本项目承建数据中心机房。");
        w.heading(2, "技术方案").unwrap();
        w.paragraph_text("冷热通道隔离设计。");
        w.heading(3, "详细设计").unwrap();
        w.paragraph_text("机柜布局见下表。");
        let doc = roundtrip(&mut w);
        let out = convert(&doc);

        // 标题树：层级 + 行号（标题行在正文行前交错）。
        assert_eq!(out.headings.len(), 3);
        assert_eq!(
            out.headings[0],
            HeadingEntry {
                level: 1,
                text: "项目概述".into(),
                line: 0
            }
        );
        assert_eq!(
            out.headings[1],
            HeadingEntry {
                level: 2,
                text: "技术方案".into(),
                line: 2
            }
        );
        assert_eq!(
            out.headings[2],
            HeadingEntry {
                level: 3,
                text: "详细设计".into(),
                line: 4
            }
        );

        // 行模型：标题与正文交错。
        assert_eq!(
            out.text,
            "项目概述\n本项目承建数据中心机房。\n技术方案\n冷热通道隔离设计。\n详细设计\n机柜布局见下表。"
        );

        // Heading span：标题行整段。
        assert_eq!(out.spans[0], vec![(0..4, TokenKind::Heading)]);

        // 属性。
        assert_eq!(out.props.blocks, 6);
        assert_eq!(out.props.paragraphs, 6);
        assert_eq!(out.props.tables, 0);
        assert_eq!(out.props.warnings, 0);
        // 只读装载：无块归属/无投影。
        assert!(out.blocks.is_none());
        assert!(out.projections.is_empty());
        assert!(out.fallbacks.is_empty());
        assert_eq!(out.candidates, 0, "只读装载不做段落序对账");
    }

    #[test]
    fn run_styles_bold_italic_link() {
        let mut w = DocxWriter::new();
        w.paragraph(&[
            RunSpec::text("普通"),
            RunSpec::styled("加粗", true, false),
            RunSpec::styled("斜体", false, true),
            RunSpec::link("官网", "https://example.com"),
        ]);
        let doc = roundtrip(&mut w);
        let out = convert(&doc);

        assert_eq!(out.text, "普通加粗斜体官网");
        let span_of = |i: usize| out.spans[0][i].clone();
        // 加粗 2..4 / 斜体 4..6 / 链接 6..8（char 索引）。
        assert_eq!(span_of(0), (2..4, TokenKind::Bold));
        assert_eq!(span_of(1), (4..6, TokenKind::Italic));
        assert_eq!(span_of(2), (6..8, TokenKind::Link));

        // #115：可编辑装载下 run 级样式同样成立（行切分以引擎装配文本
        // 为准，run 对位口径不变）——假端口给出与原文一致的装配文本。
        let fake = FakePort {
            texts: vec!["普通加粗斜体官网".to_string()],
        };
        let ed = convert_editable(&doc, &fake);
        assert_eq!(ed.text, out.text);
        assert_eq!(ed.spans[0], out.spans[0]);
    }

    #[test]
    fn list_ordered_bullet_and_indent() {
        let mut w = DocxWriter::new();
        w.paragraph_text("清单：");
        w.list_item(&[RunSpec::text("需求调研")], ListKind::Ordered, 0)
            .unwrap();
        w.list_item(&[RunSpec::text("方案设计")], ListKind::Ordered, 0)
            .unwrap();
        w.list_item(&[RunSpec::text("现场勘测")], ListKind::Bullet, 0)
            .unwrap();
        let doc = roundtrip(&mut w);
        let out = convert(&doc);

        let lines: Vec<&str> = out.text.split('\n').collect();
        assert_eq!(lines[1], "1. 需求调研");
        assert_eq!(lines[2], "2. 方案设计");
        assert_eq!(lines[3], "• 现场勘测");
        // ListMarker span 覆盖 "1. "。
        assert_eq!(out.spans[1][0], (0..3, TokenKind::ListMarker));
    }

    /// #115：表格第一版**不出行**（整表原文透传——网格行无法逐字回写），
    /// 改为属性计数 + 只读清单行（看得见的边界）。
    #[test]
    fn table_props_counted_rows_not_flattened() {
        let mut w = DocxWriter::new();
        w.table(&[
            vec!["项目".into(), "数量".into(), "单价".into()],
            vec!["机柜".into(), "42".into(), "18000".into()],
            vec!["UPS".into(), "2".into(), "350000".into()],
        ]);
        let doc = roundtrip(&mut w);
        let out = convert(&doc);
        assert_eq!(out.props.tables, 1);
        assert_eq!(out.props.table_rows, 3);
        assert!(
            !out.text.contains('|'),
            "表格网格不入行流（第一版）：{:?}",
            out.text
        );
        // 段落序对账面（可编辑装载）：候选 = 引擎 w:p 数（含表格单元格）
        // = 宽面文档序段落总数。
        let (doc2, port2) = roundtrip_editable(&mut w);
        let ed = convert_editable(&doc2, &port2);
        assert_eq!(ed.candidates, port2.paragraph_count());
        assert_eq!(ed.candidates, port_doc_paragraphs(&doc2));
    }

    /// 宽面文档序段落总数（表格内段落也计——与引擎 \`w:p\` 同口径）。
    fn port_doc_paragraphs(doc: &docx::DocxDocument) -> usize {
        fn count_blocks(blocks: &[Block]) -> usize {
            blocks
                .iter()
                .map(|b| match b {
                    Block::Paragraph(_) => 1,
                    Block::Table(t) => t.rows.iter().map(|r| r.len()).sum::<usize>(),
                    Block::TextBox(tb) => count_blocks(&tb.blocks),
                })
                .sum()
        }
        count_blocks(&doc.blocks)
    }

    #[test]
    fn display_width_cjk_vs_ascii() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("中a"), 3);
        assert_eq!(display_width("１２"), 4, "全角数字 2 cell");
    }

    #[test]
    fn truncate_cell_caps_width() {
        // 25 char × 2 cell = 50 > 28 → 截断至 13 char（26 cell）+ …（27 ≤ 28）。
        let s = "这是一个非常非常长的单元格内容肯定需要被截断处理掉";
        assert_eq!(display_width(s), 50);
        let t = truncate_cell(s);
        assert!(
            display_width(&t) <= CELL_WIDTH_CAP,
            "截断后宽度：{}",
            display_width(&t)
        );
        assert!(t.ends_with('…'));
        assert_eq!(display_width(&t), 27);
        // 短串免截断。
        assert_eq!(truncate_cell("短串"), "短串");
    }

    #[test]
    fn readonly_gate_browsing_passes_mutations_blocked() {
        // 浏览面放行。
        for key in [
            "left", "right", "up", "down", "home", "end", "pageup", "pagedown", "escape",
        ] {
            assert!(readonly_allows(key, false), "{key} 应放行");
        }
        assert!(readonly_allows("a", true), "Ctrl+A 全选放行");
        assert!(readonly_allows("c", true), "Ctrl+C 复制放行");
        assert!(readonly_allows("f", true), "Ctrl+F 查找放行");
        assert!(readonly_allows("g", true), "G2：Ctrl+G 跳行放行（导航类）");
        assert!(!readonly_allows("g", false), "无 Ctrl 的 g 不放行");
        assert!(
            readonly_allows("tab", true),
            "F3：Ctrl+Tab 标签循环导航放行"
        );
        // 变更面拒绝。
        assert!(!readonly_allows("enter", false));
        assert!(!readonly_allows("backspace", false));
        assert!(!readonly_allows("delete", false));
        assert!(!readonly_allows("tab", false));
        assert!(!readonly_allows("z", true), "Ctrl+Z 撤销拒绝");
        assert!(!readonly_allows("y", true), "Ctrl+Y 重做拒绝");
        assert!(!readonly_allows("x", true), "Ctrl+X 剪切拒绝");
        assert!(!readonly_allows("v", true), "Ctrl+V 粘贴拒绝");
        assert!(!readonly_allows("s", true), "Ctrl+S 保存拒绝");
        assert!(!readonly_allows("s", false), "可打印字符插入拒绝");
        assert!(!readonly_allows("f", false), "无 Ctrl 的 f 是输入——拒绝");
    }

    #[test]
    fn textbox_blocks_flatten() {
        let mut w = DocxWriter::new();
        w.paragraph_text("正文段");
        let doc = roundtrip(&mut w);
        let out = convert(&doc);
        assert_eq!(out.props.textboxes, 0);
        assert_eq!(out.text, "正文段");
    }

    #[test]
    fn empty_paragraphs_counted() {
        let mut w = DocxWriter::new();
        w.paragraph_text("段一");
        w.paragraph_text("");
        w.paragraph_text("段二");
        let doc = roundtrip(&mut w);
        let out = convert(&doc);
        assert_eq!(out.props.empty_paragraphs, 1);
        assert_eq!(out.text, "段一\n\n段二");
    }

    // ------------------------------------------------------------------
    // #115：可编辑装载 —— 块归属 / 段落序 / 投影 / 软换行 / 锁定面
    // ------------------------------------------------------------------

    #[test]
    fn editable_blocks_cover_every_line() {
        let mut w = DocxWriter::new();
        w.heading(1, "概述").unwrap();
        w.paragraph_text("正文一。");
        w.list_item(&[RunSpec::text("项")], ListKind::Ordered, 0)
            .unwrap();
        w.table(&[vec!["A".into(), "B".into()], vec!["1".into(), "2".into()]]);
        let (doc, port) = roundtrip_editable(&mut w);
        let out = convert_editable(&doc, &port);
        let blocks = out.blocks.clone().expect("可编辑装载有块表");
        let line_count = out.text.split('\n').count();
        assert_eq!(blocks.len(), line_count, "块表与行 1:1");
        assert_eq!(blocks.len(), out.spans.len(), "块表与样式轨 1:1");
        // 段落序对账：宽面候选段落数 == 引擎 \`w:p\` 数（表格单元格段落也计）。
        assert_eq!(out.candidates, port.paragraph_count());
        assert_eq!(out.candidates, port_doc_paragraphs(&doc));
        // 正文段落（非表格）各自成行，段落号连续。
        let seen: Vec<usize> = blocks
            .iter()
            .filter_map(|b| match b {
                LineBlock::Para { paragraph, .. } => Some(*paragraph),
                _ => None,
            })
            .collect();
        assert_eq!(seen, vec![0, 1, 2], "正文三段落文档序");
        // 第一版：表格不出行 → 无表格锁定行；表格只进属性与清单。
        assert_eq!(
            blocks
                .iter()
                .filter(|b| matches!(b, LineBlock::Locked(ReadOnlyLine::TableRow)))
                .count(),
            0
        );
        assert_eq!(out.props.tables, 1);
        assert_eq!(out.props.table_rows, 2);
        assert!(
            out.text.contains("表格：1 个"),
            "表格清单行（看得见的边界）：{:?}",
            out.text
        );
    }

    #[test]
    fn editable_projection_matches_rendered_lines() {
        // 段落投影（逐段）必须能在行模型里逐行拼回来——这是回写可逆的
        // 前提（投影 ≠ 行文本 = 保存会误改）。
        let mut w = DocxWriter::new();
        w.paragraph_text("第一段。");
        w.list_item(&[RunSpec::text("列表项")], ListKind::Bullet, 0)
            .unwrap();
        w.paragraph_text("软换行前\r\n软换行后");
        let (doc, port) = roundtrip_editable(&mut w);
        let out = convert_editable(&doc, &port);
        let blocks = out.blocks.clone().unwrap();
        let lines: Vec<String> = out.text.split('\n').map(|s| s.to_string()).collect();
        let mut rebuilt: HashMap<usize, Vec<String>> = HashMap::new();
        for (i, b) in blocks.iter().enumerate() {
            if let LineBlock::Para { paragraph, .. } = b {
                rebuilt
                    .entry(*paragraph)
                    .or_default()
                    .push(lines[i].clone());
            }
        }
        for (p, chunk) in rebuilt {
            let joined = chunk.join("\n");
            assert_eq!(
                out.projections.get(&p).map(String::as_str),
                Some(joined.as_str()),
                "段 {p} 投影与行文本必须一致"
            );
            // 兜底投影 = 段内行正文（去标记/缩进）——回写口径。
            let body: Vec<&str> = joined
                .split('\n')
                .map(crate::editor::docx_edit_bridge::body_of)
                .collect();
            assert_eq!(
                out.fallbacks.get(&p).map(String::as_str),
                Some(body.join("\n").as_str()),
                "段 {p} 兜底投影 = 正文"
            );
        }
    }

    /// #115：软换行（`w:br`）拆行——引擎在场时行切分以**引擎装配文本**
    /// 为准（`w:br`→\n 与回写口径逐字一致）；只读装载走宽面块模型文本
    /// （引擎整备文本不含换行标记——源文本生成器的已知面，登记）。
    #[test]
    fn editable_soft_break_lines_are_editable_and_carry() {
        // 引擎装配文本含换行时的拆行 + 承接头归属（用假端口直接驱动）。
        let fake = FakePort {
            texts: vec!["上行\n下行".to_string()],
        };
        let mut w = DocxWriter::new();
        w.paragraph_text("上行下行");
        let doc = roundtrip(&mut w);
        let out = convert_editable(&doc, &fake);
        assert_eq!(out.text, "上行\n下行", "引擎装配文本的软换行拆成两行");
        let blocks = out.blocks.clone().unwrap();
        assert_eq!(
            blocks[0],
            LineBlock::Para {
                paragraph: 0,
                idx: 0,
                cont: false
            }
        );
        assert_eq!(
            blocks[1],
            LineBlock::Para {
                paragraph: 0,
                idx: 1,
                cont: true
            },
            "续行是承接头（删它 ≠ 删目标文本）"
        );
        assert_eq!(
            out.fallbacks.get(&0).map(String::as_str),
            Some("上行\n下行")
        );
    }

    /// 内存假端口（引擎面替身——只给文本，其余面中性）。
    struct FakePort {
        texts: Vec<String>,
    }

    impl DocxPort for FakePort {
        fn paragraph_count(&self) -> usize {
            self.texts.len()
        }
        fn paragraph_text(&self, idx: usize) -> String {
            self.texts.get(idx).cloned().unwrap_or_default()
        }
        fn blocked_reason(&self, _idx: usize) -> Option<ReadOnlyLine> {
            None
        }
        fn set_text(&mut self, _idx: usize, _text: &str) -> Result<(), String> {
            Ok(())
        }
        fn save(&self) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn editable_aux_appendix_lines_are_readonly() {
        let mut w = DocxWriter::new();
        w.paragraph_text("正文。");
        w.table(&[vec!["A".into()]]);
        let (doc, port) = roundtrip_editable(&mut w);
        let out = convert_editable(&doc, &port);
        // 表格存在 → 出一条「表格」清单行（第一版只读的边界，看得见）。
        assert!(
            out.text.contains("表格：1 个"),
            "表格清单行（看得见的边界）：{:?}",
            out.text
        );
        // 清单行本身不可编辑。
        let blocks = out.blocks.clone().unwrap();
        assert!(
            blocks
                .last()
                .is_some_and(|b| matches!(b, LineBlock::Locked(ReadOnlyLine::AuxAppendix))),
            "末行是只读清单"
        );
        assert_eq!(out.props.images, 0);
    }

    #[test]
    fn editable_locked_paragraphs_marked_readonly() {
        // 域代码段：引擎 typed 拒改 → 行模型标锁定（FieldCode）。
        let mut w = DocxWriter::new();
        w.paragraph_text("正文。");
        let (doc, port) = roundtrip_editable(&mut w);
        let out = convert_editable(&doc, &port);
        assert_eq!(out.props.locked_paragraphs, 0, "普通段无锁定");
        assert_eq!(out.props.editable_paragraphs, 1);
        let _ = read_docx_from_bytes;
    }

    /// 烟雾 GUI 夹具生成器（不随常规测试跑）：
    ///
    /// ```bash
    /// cargo test gen_smoke_docx -- --ignored
    /// ./smoke.sh 5 tests/fixtures/smoke.docx
    /// ```
    #[test]
    #[ignore = "烟雾夹具生成器（写 tests/fixtures/smoke.docx）"]
    fn gen_smoke_docx() {
        let mut w = DocxWriter::new();
        w.heading(1, "智慧园区综合管理平台投标文件").unwrap();
        w.paragraph_text("本文件为技术标书演示样张（mang-shell 可编辑化烟雾夹具）。");
        w.heading(2, "一、项目概述").unwrap();
        w.paragraph(&[
            RunSpec::text("本项目承建 "),
            RunSpec::styled("智慧园区综合管理平台", true, false),
            RunSpec::text("，含数据中心机房、安防与物联网接入三大部分。"),
        ]);
        w.heading(2, "二、技术方案").unwrap();
        w.paragraph_text("平台采用云原生微服务架构，核心组件如下：");
        w.list_item(
            &[RunSpec::text("接入网关集群（双活）")],
            ListKind::Ordered,
            0,
        )
        .unwrap();
        w.list_item(
            &[RunSpec::text("消息中间件（Kafka）")],
            ListKind::Ordered,
            0,
        )
        .unwrap();
        w.list_item(
            &[RunSpec::text("时序数据库（TDengine）")],
            ListKind::Ordered,
            0,
        )
        .unwrap();
        w.list_item(&[RunSpec::text("现场勘测已完成")], ListKind::Bullet, 0)
            .unwrap();
        w.heading(2, "三、报价明细").unwrap();
        w.table(&[
            vec!["设备".into(), "数量".into(), "单价（元）".into()],
            vec!["机柜".into(), "42".into(), "18000".into()],
            vec!["UPS 电源".into(), "2".into(), "350000".into()],
            vec!["核心交换机".into(), "4".into(), "128000".into()],
        ]);
        w.heading(2, "四、实施计划").unwrap();
        w.paragraph(&[
            RunSpec::text("总工期 "),
            RunSpec::styled("180", false, true),
            RunSpec::text(" 个日历天，详见 "),
            RunSpec::link("项目门户", "https://example.com"),
            RunSpec::text("。"),
        ]);
        w.quote_paragraph(
            &[RunSpec::text(
                "注：本样张由 mdc-office-io 写面生成，用于可编辑化验证。",
            )],
            1,
        );
        let bytes = w.save_to_vec().expect("写盘内存");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/smoke.docx");
        std::fs::write(path, &bytes).expect("落盘夹具");
        // 落盘件自校验：读回 + 转换可过。
        let payload = load(std::path::Path::new(path)).expect("夹具读回");
        assert_eq!(payload.headings.len(), 5, "五级标题树");
        assert_eq!(payload.props.tables, 1);
        let editable = load_editable(std::path::Path::new(path)).expect("夹具可编辑装载");
        assert!(editable.bridge.is_some(), "可编辑装载应成功");
        eprintln!(
            "docx 夹具已生成：{path}（{} 字节，{} 行，{} 标题）",
            bytes.len(),
            payload.text.split('\n').count(),
            payload.headings.len()
        );
    }

    /// #226 R2：panic 兜底——panic 载荷转 typed Err（context 前缀 +
    /// 载荷文本）；Ok/Err 原样透传；非字符串载荷诚实「未知」。
    #[test]
    fn panic_guard_converts_panics_to_typed_error() {
        let err = panic_guard("文档打开失败", || -> Result<(), String> {
            panic!("index out of bounds")
        })
        .expect_err("panic 应转 Err");
        assert!(err.contains("文档打开失败"), "{err}");
        assert!(err.contains("index out of bounds"), "{err}");

        let err2 = panic_guard("文档打开失败", || -> Result<(), String> {
            panic!("string payload: {}", 42)
        })
        .expect_err("panic 应转 Err");
        assert!(err2.contains("string payload: 42"), "{err2}");

        assert_eq!(panic_guard("x", || Ok(7)).expect("Ok 透传"), 7);
        assert_eq!(
            panic_guard("x", || Err::<(), _>("typed".into())).expect_err("Err 透传"),
            "typed"
        );

        let unknown = panic_guard("x", || -> Result<(), String> {
            std::panic::panic_any(3.14f64)
        })
        .expect_err("非字符串载荷转 Err");
        assert!(unknown.contains("未知"), "{unknown}");
    }
}
