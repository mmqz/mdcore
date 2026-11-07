//! docx 可编辑桥（#115——Buffer ↔ DocxEditor 可逆双向映射）。
//!
//! ## 定位
//!
//! 编辑器**不重写**：Buffer（文本/rope）→ display_map → 渲染 → 光标 →
//! 选区 → 命令整条管线原样复用。本模块只解决一件事——
//! **「Buffer 里的一行」↔「docx 里的一个 w:p」的双向映射**。
//!
//! ## 可逆性（核心不变量）
//!
//! > **未编辑的段落必须回写「原文全等」，否则一律不调用 set。**
//!
//! 引擎的 set_paragraph_text 是**外科手术**不是「改文字」——即使新文与
//! 旧文逐字相同，它也会把多 run 段合并成首 run、给 w:t 加
//! xml:space="preserve"、把自闭合 <w:p/> 转成开形……即「一字不改也会
//! 动手术」。所以桥侧先做逐字节比对：Buffer 段文本 == 打开态投影 → 跳过
//! （零手术、零字节变化）；不等才调用。
//!
//! 由此得三条硬性质（均有测试钉定）：
//!
//! 1. **打开后不编辑 → 保存 → 字节级恒等**；
//! 2. **编辑一段 → 非目标部件逐字节不变**（引擎 raw_copy 透传）；
//! 3. **非目标段落逐字节不变**（引擎「目标段外零接触」）。
//!
//! ## 打开态投影 `proj[p]`（单一口径，编辑期恒不变）
//!
//! 段落 p 在行模型里呈现为若干行，`proj[p]` = 这些行**去掉段落内序号**后
//! 的文本（多行以 \n 连接）。段落内序号 = 「这一行是该段落的第几行」：
//!
//! - 行模型里同段的行永远**连号**（段落的行在行流里相邻）；
//! - 删除本段内某行 → 其后各段内序号减一；从**别的段**插入的行 →
//!   其后所有段内序号加一；段内拆分/合并 → 本段（及其后）序号重排。
//!
//! 于是：
//! - **打开后原样保存**：compute 逐段还原 = proj 逐段全等 → 零 set；
//! - **只改一段**：只有该段不等（其余段行数/文本都没动）→ 只 set 它；
//! - **一行的编辑不会误改相邻段**——序号是**相对本段**的，与其它段无关。
//!
//! ## 行归属（四态）
//!
//! | 归属 | 含义 | 回写 |
//! |------|------|------|
//! | `Para { p, idx, cont }` | 段落 p 的第 idx 行（cont = 承接续行） | 参与 p 的文本 |
//! | `Foreign { id, idx }` | 从**别的段**拆出来插进来的段内行 | 不写（原段少了这行 = 原文变更） |
//! | `User { join }` | 用户敲 Enter 造出的新行 | 并入本段文本（等价软换行；join = 该行删掉后是同段两行的交接口） |
//! | `Locked(kind)` | 锁定行（表格/分隔线/域/修订/辅助清单……） | 不写；编辑拒 |
//!
//! ## 第一版边界（显式登记，不假装支持）
//!
//! - **锁定行**：表格行/表头分隔线/尾空行、页眉页脚/批注/脚注/尾注/
//!   图片清单行——只读（在其中敲字 = 拒），原因见 [`ReadOnlyLine::reason`]；
//!   在锁定行上敲 Enter **退回**（不静默吞）；
//! - **域代码段 / TOC 段 / 修订段（Refuse 策略）/ mc:Fallback 段 /
//!   文本框内嵌段**：引擎侧 typed 拒改——桥侧标锁定，渲染成文本但不可改；
//! - 段内**多 run 格式**：改文字保留**首个含文本 run** 的 w:rPr（引擎口径，
//!   实测「改文字不坏格式」），其余 run 并入——登记；
//! - 页眉/页脚/批注/脚注/尾注：第一版**只读**（不入编辑面；原文逐字节
//!   透传）；
//! - 段落**增删**（整段删掉 / 新起一段）：文本对不上时该段落按原文写回，
//!   无归属的残行忽略——**不静默**：保存证据行报告 `dropped` 计数。
//!
//! ## 与引擎的边界
//!
//! 桥侧唯一依赖的引擎能力收在 [`DocxPort`] 窄面（段落数/文本/可编辑面/
//! 表格·文本框标记/软换行数/设文本/落盘/待落账判定）——单测以内存假实现
//! 驱动（零编译器成本、零磁盘依赖）；真实实现 =
//! [`crate::editor::docx_doc::DocxEnginePort`]（薄包装
//! `mdc_office_io::docx_edit::DocxEditor`）。

use std::collections::HashMap;

use super::buffer::{EditHook, EditIntent, EditSummary, UndoHook};

/// 行归属（与行严格对齐；1:1——每行一项）。
///
/// NB：`Foreign`（外段行插入）与 `Locked(TableRow/TableRule/TableGap)` 是
/// **完整设计面**的保留分支——第一版表格/文本框整体只读、外段行插入路径
/// 尚未接线，故当前构造点为 0（显式登记，不是遗漏）。
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineBlock {
    /// 打开文档自带的行（属于某段落的段内序号）。
    Para {
        /// 段落在 `DocxEditor::paragraphs()` 里的 0 基文档序索引。
        paragraph: usize,
        /// 段内序号（0 基）——该行是本段落的第几行。
        idx: usize,
        /// 承接续行（无列表标记的第二行起；清空它 ≠ 清空目标文本）。
        cont: bool,
    },
    /// 从别的段拆出来、插进本段位置的行（不参与本段回写）。
    Foreign {
        /// 该行的 `(段落, 段内序号)` 身份。
        id: (usize, usize),
        /// 它在本段落里的位置（序号）——本段后续行由它承接。
        idx: usize,
    },
    /// 用户敲 Enter 造出的新行。
    User {
        /// 该行删掉后，是否就是同段两行的交接口（回车撤销后仍可归位）。
        join: bool,
    },
    /// 表格单元格（#115 表格编辑——与 `Para` 平级）。
    ///
    /// 回写目标由**侧表**给（[DocxBridge::cell_paragraph]），不在本变体里
    /// 存段落号：表格在文档序里插行/删行时侧表整体平移，行归属不用重算
    /// （设计 §4「侧表与正文解耦」）。
    Cell {
        /// 表序号（文档序第几个最外层 `w:tbl`）。
        table: usize,
        /// 行号（0 基）。
        row: usize,
        /// 栅格列号（0 基；合并格的覆盖列指向首格）。
        col: usize,
    },
    /// 锁定行（不可编辑；原因见 [`ReadOnlyLine`]）。
    Locked(ReadOnlyLine),
}

/// 锁定行的类别（属性面板/证据行/拒改提示共用单一真源）。
#[allow(dead_code)] // 同上：表格三类保留分支（第一版表格整体只读）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnlyLine {
    /// 表格行（a | b 网格投影——表格单元格写入属后续版本）。
    TableRow,
    /// 表头分隔线（---+---）。
    TableRule,
    /// 表间呼吸空行（块间空白）。
    TableGap,
    /// 文本框内段落（嵌套编辑互斥——引擎 L1 登记）。
    Textbox,
    /// 域代码 / TOC 段落（引擎 typed 拒改：缓存结果与指令会失配）。
    FieldCode,
    /// 修订标记段落（RevisionPolicy::Refuse 下引擎拒改）。
    Revision,
    /// mc:Fallback 复制区段（双分支一致性防线——引擎拒改）。
    FallbackCopy,
    /// 页眉/页脚/批注/脚注/尾注/图片等「不入正文流」的清单行（只读）。
    AuxAppendix,
    /// 宽面有、引擎无对应 `w:p` 的段落（块模型/XML 扫描口径分歧——
    /// 此类文件整体降级只读，本行不会出现在正常装载里）。
    Unrepresentable,
}

impl ReadOnlyLine {
    /// 面向用户的拒改原因（拒改提示/属性面板/日志复用同一说法）。
    pub fn reason(self) -> &'static str {
        match self {
            ReadOnlyLine::TableRow => "表格内的文字（第一版只读——表格单元格写回属后续版本）",
            ReadOnlyLine::TableRule => "表格分隔线（装饰行，不可编辑）",
            ReadOnlyLine::TableGap => "表格块间空行（装饰行，不可编辑）",
            ReadOnlyLine::Textbox => "文本框内文字（第一版只读——文本框编辑需处理嵌套段互斥）",
            ReadOnlyLine::FieldCode => {
                "域代码段落（页码/目录/交叉引用——引擎拒改，避免缓存值与指令失配）"
            }
            ReadOnlyLine::Revision => "含修订标记的段落（默认策略拒改，避免吃掉修订痕迹）",
            ReadOnlyLine::FallbackCopy => "兼容回退副本段（引擎拒改，避免双分支不一致）",
            ReadOnlyLine::AuxAppendix => "页眉/页脚/批注/脚注等辅助内容（第一版只读）",
            ReadOnlyLine::Unrepresentable => {
                "该文件段落对账不一致（块模型与 XML 扫描口径分歧）——整篇降级只读"
            }
        }
    }
}

/// docx 引擎窄面（桥侧唯一依赖；单测用内存假实现）。
pub trait DocxPort {
    /// 段落总数（文档序扁平：body/表格内/文本框内统一计数）。
    fn paragraph_count(&self) -> usize;
    /// 段落当前文本（已 set 的取待落账值；否则原文装配值）。
    fn paragraph_text(&self, idx: usize) -> String;
    /// 引擎侧拒改面：None = 可编辑；Some = 拒改类别。
    fn blocked_reason(&self, idx: usize) -> Option<ReadOnlyLine>;
    /// 设置段落文本（待落账）。
    fn set_text(&mut self, idx: usize, text: &str) -> Result<(), String>;
    /// 落账并产出完整 docx 字节。
    fn save(&self) -> Result<Vec<u8>, String>;
    /// 表格栅格（**权威坐标**——引擎扫描 `document.xml` 得出，桥侧不推断）。
    ///
    /// 表 → 行 → 栅格列 → 段落号；合并格按 OOXML 语义占位（`gridSpan`
    /// 覆盖列 / `vMerge` 续格指向起始格）。缺省实现返回空表（"无表格
    /// 能力"的窄面替身——单测`FakePort` 不必重复实现）。
    fn table_cells(&self) -> Vec<Vec<Vec<usize>>> {
        Vec::new()
    }

    /// #156：sdt 容器内段落号清单（升序——引擎可见、宽面**结构性不可见**
    /// 的盲区；背景见 `mdc_office_io::docx_edit::ParagraphRef::in_sdt`）。
    ///
    /// 装载对账消费：宽面块模型丢弃 `w:sdt` 内容（docx-rs 读模型
    /// `StructuredDataTag` 面外项），这些段落必须从行模型顺序游标里
    /// **跳过**（空投影 + 不入行流），段落序才能与引擎原文坐标系对齐。
    /// 缺省空集（窄面替身）。
    fn sdt_paragraphs(&self) -> Vec<usize> {
        Vec::new()
    }

    /// #156：域清单（透传引擎 `DocxEditor::fields`——`--smoke-docx-bid`
    /// 四面貌编辑的域面）。缺省空集。
    fn fields(&self) -> Vec<mdc_office_io::docx_edit::FieldInfo> {
        Vec::new()
    }

    /// #156：更新域显示值（透传引擎——只改结果区 `w:t`，域五段结构
    /// 零接触）。缺省拒（窄面替身无域面）。
    fn set_field_value(
        &mut self,
        _para: usize,
        _field_idx: usize,
        _value: &str,
    ) -> Result<(), String> {
        Err("port 无域面".into())
    }

    /// #156：批注清单（透传引擎 `DocxEditor::comments`）。缺省空集。
    fn comments(&self) -> Vec<mdc_office_io::docx_edit::CommentInfo> {
        Vec::new()
    }

    /// #156：更新批注文本（透传引擎——部件级手术，只动 `word/comments.xml`
    /// 目标条目）。缺省拒（窄面替身无批注面）。
    fn set_comment_text(&mut self, _id: usize, _text: &str) -> Result<(), String> {
        Err("port 无批注面".into())
    }
}

// ---------------------------------------------------------------------------
// 纯函数面（单测锚点，不起 GPUI）
// ---------------------------------------------------------------------------

/// 在行列表里删除 `[start, count)` 行。
pub fn delete_rows(rows: &mut Vec<LineBlock>, start: usize, count: usize) {
    let end = (start + count).min(rows.len());
    if start < end {
        rows.drain(start..end);
    }
}

/// 插入行时的归属决策（[\`remap_after_edit\`] 的单行版——多行插入一次算定）。
#[allow(dead_code)]
pub fn insert_rows(rows: &mut Vec<LineBlock>, anchor: usize, new_rows: usize, source: LineBlock) {
    let at = anchor.min(rows.len());
    let fresh: Vec<LineBlock> = (0..new_rows)
        .map(|k| match source {
            LineBlock::Para {
                paragraph,
                idx,
                cont,
            } => LineBlock::Para {
                paragraph,
                idx: idx + k,
                cont,
            },
            LineBlock::User { join } => LineBlock::User { join },
            LineBlock::Foreign { id, idx } => LineBlock::Foreign { id, idx: idx + k },
            // 单元格行**不可复制**：单元格不是「插一行」的单位（增删
            // 行列属后续版本）——插入位置落在格子里时退化为用户行。
            LineBlock::Cell { .. } => LineBlock::User { join: false },
            LineBlock::Locked(_) => LineBlock::User { join: false },
        })
        .collect();
    for (k, row) in fresh.into_iter().enumerate() {
        rows.insert(at + k, row);
    }
}

/// 编辑后的行归属重排（纯函数——[EditHook] 的实现体）。
///
/// 三种形态：
/// - **删除**（`start..end` 非空、未插入）：逐行退位（同段且序号更大者
///   −1）；合并「甲乙」时首删行（段 0）的剩余正文转入下一删行（段 1）→
///   段 1 吃下合并文本，段 0 变为「空段落行」由段 1 行承接；
/// - **纯插入**（段内拆分/回车）：新行归**上一行**所属段（段内序号按
///   位置重排）；
/// - **原位替换**（如缩进只动行首）：归属不变。
pub fn remap_after_edit(
    rows: &mut Vec<LineBlock>,
    start: (usize, usize),
    end: (usize, usize),
    inserted_lines: usize,
    _newline_removed: bool,
) {
    let a = start.0.min(rows.len());
    let b = end.0.min(rows.len());
    let removed: Vec<LineBlock> = if a < b {
        rows[a..b].to_vec()
    } else {
        Vec::new()
    };
    // 插入锚点：删除 → 被删首行归属；纯插入 → 上一行归属。
    let anchor = if removed.is_empty() {
        rows.get(a.saturating_sub(1).min(rows.len().saturating_sub(1)))
            .copied()
            .and_then(|r| r.sum_anchor(&[]))
            .or_else(|| removed.first().copied())
    } else {
        a.sum_anchor(&removed)
    };
    delete_rows(rows, a, b - a);
    for _ in 0..inserted_lines {
        let row = match anchor {
            Some(LineBlock::Para {
                paragraph,
                idx,
                cont,
            }) => LineBlock::Para {
                paragraph,
                idx,
                cont,
            },
            // 单元格不是可复制的插入单位（同 insert_rows 口径）。
            Some(other) => other,
            None => LineBlock::User { join: false },
        };
        let at = a.min(rows.len());
        rows.insert(at, row);
    }
    // 删除的 Para/Foreign 行：同段更靠后的行逐行退位。
    for r in &removed {
        match *r {
            LineBlock::Para { paragraph, idx, .. }
            | LineBlock::Foreign {
                id: (paragraph, idx),
                ..
            } => {
                for row in rows.iter_mut() {
                    match row {
                        LineBlock::Para {
                            paragraph: p2,
                            idx: i2,
                            ..
                        } if *p2 == paragraph && *i2 > idx => *i2 -= 1,
                        LineBlock::Foreign { id: (p2, i2), .. }
                            if *p2 == paragraph && *i2 > idx =>
                        {
                            *i2 -= 1
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    // 纯插入落在段内：本段行按位置重排（新行的段内序号看**上一行**在哪）。
    if let Some(LineBlock::Para { paragraph, .. }) = anchor
        && inserted_lines > 0
    {
        let mut k = 0usize;
        for row in rows.iter_mut() {
            if let LineBlock::Para {
                paragraph: p2,
                idx,
                cont,
            } = row
                && *p2 == paragraph
            {
                *idx = k;
                *cont = k > 0;
                k += 1;
            }
        }
    }
}

/// 删除锚点（`removed` 的行实际属于哪个段——决定插入行的身份）。
trait SumAnchor {
    fn sum_anchor(self, removed: &[LineBlock]) -> Option<LineBlock>;
}

impl SumAnchor for LineBlock {
    fn sum_anchor(self, _removed: &[LineBlock]) -> Option<LineBlock> {
        match self {
            LineBlock::Para { .. } | LineBlock::User { .. } | LineBlock::Foreign { .. } => {
                Some(self)
            }
            // 单元格行是**可编辑**的，但它不是段落续接的锚（在格子里敲
            // Enter 不该把正文段落拉进来）：插入锚退化为用户行。
            LineBlock::Cell { .. } | LineBlock::Locked(_) => None,
        }
    }
}

impl SumAnchor for usize {
    fn sum_anchor(self, removed: &[LineBlock]) -> Option<LineBlock> {
        let _ = self;
        removed.iter().find_map(|b| match b {
            LineBlock::Para { .. } | LineBlock::User { .. } => Some(*b),
            // 删掉的是「外段行」：插入应仍在原段位置（本段后续行承接）。
            LineBlock::Foreign { id, idx } => Some(LineBlock::Para {
                paragraph: id.0,
                idx: *idx,
                cont: false,
            }),
            // 单元格行不做段落续接锚（同 SumAnchor for LineBlock 口径）。
            LineBlock::Cell { .. } | LineBlock::Locked(_) => None,
        })
    }
}

/// 单行正文（去掉列表标记前缀——回写用；纯函数）。
///
/// 标记判定：至多 2 个前导空格后的 `N. ` / `• ` / `· `（行模型标记集）；
/// 用户改过的行（标记被删/被换）按「整行即正文」处理。
///
/// NB：**不剥前导空格**——缩进近似是段落属性（`w:ind`）的投影，正文比对
/// 两侧（投影/兜底/回写）必须同口径，否则缩进段落会被判成「改过」。
pub fn body_of(line: &str) -> &str {
    for marker in ["\u{2022} ", "\u{00b7} "] {
        if let Some(pos) = line.find(marker)
            && pos <= 2
            && line[..pos].chars().all(|c| c == ' ')
        {
            return &line[pos + marker.len()..];
        }
    }
    let lead_spaces = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[lead_spaces..];
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if lead_spaces <= 2 && digits > 0 && digits <= 3 && rest[digits..].starts_with(". ") {
        return &rest[digits + 2..];
    }
    line
}

/// 段内行文本列表 → 回写正文（逐行取正文，\n 连接）。
fn body_join(texts: &[String]) -> String {
    join_trimmed(texts, body_of)
}

/// 由「段内行号 → 行文本」还原段落文本（`\n` 连接；缺号跳过）。
pub fn compose(texts: &[String]) -> String {
    join_trimmed(texts, |t| t)
}

/// 拼接并去掉**尾部**空单元格（承接头被清空 = 不改目标文本；行模型里
/// 承接续行本来就是「目标文本之外的行」）。
fn join_trimmed<F: Fn(&str) -> &str>(texts: &[String], f: F) -> String {
    let out: Vec<&str> = texts.iter().map(|t| f(t.as_str())).collect();
    out.join("\n")
}

/// 回写计划（[DocxBridge::compute] 产物——纯数据，可断言可日志）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Writeback {
    /// 需写入的段落（按段落序升序）。
    pub sets: Vec<(usize, String)>,
    /// 未变（与打开态投影一致）的段落数。
    pub unchanged: usize,
    /// 无归属的残行数（第一版不新增/不删除段落——这些行被忽略）。
    pub dropped: usize,
}

// ---------------------------------------------------------------------------
// 桥
// ---------------------------------------------------------------------------

/// 一次编辑的块级衔接参数（[DocxBridge::before_edit] 入参）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coalesce {
    /// 编辑起点所在行。
    pub start: (usize, usize),
    /// 编辑终点。
    pub end: (usize, usize),
    /// 本次编辑插入的行数（0 = 纯删除）。
    pub inserted_lines: usize,
}

/// Buffer ↔ DocxEditor 双向映射（`EditHook` 实现 = 编辑门禁 + 归属重排）。
#[derive(Clone)]
pub struct DocxBridge {
    /// 行归属（与行严格 1:1）。
    rows: Vec<LineBlock>,
    /// 打开态投影：段落 p → 段内行**整行**文本（含列表标记/缩进；多行
    /// 以 \n 连接）——「这一段动没动」的比对基准。
    proj: Vec<Option<String>>,
    /// 回写兜底：段落 p → 段内行**正文**文本（去掉标记/缩进；多行以 \n
    /// 连接）——用户改过标记行时据此回写（字号/格式面保留见引擎口径）。
    fallback: Vec<Option<String>>,
    /// 上次已 set 进引擎的文本（None = 未 set 过）。
    pending: Vec<Option<String>>,
    /// 段落原文装配文本（回归测试闸门）。
    orig: Vec<Option<String>>,
    /// 表格单元格段落号（扁平集合——「哪些段落是单元格」的快速查询；
    /// 回写目标以 `tables` 侧表为准）。
    cells: Vec<usize>,
    /// 表格侧表（#115 表格编辑）：表 → 行 → 栅格列 → 段落号。**权威
    /// 坐标来自引擎**（[`DocxPort::table_cells`]），桥侧不推断——见设计
    /// 文档 §2 的三类静默错。
    tables: Vec<TableRef>,
}

/// 表格侧表条目（与正文行模型解耦：表格增删行列时正文侧零影响）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableRef {
    /// 表序号（文档序第几个最外层 `w:tbl`）。
    pub index: usize,
    /// 栅格：行 → 列 → 单元格段落号（合并格按 OOXML 语义占位）。
    pub cells: Vec<Vec<usize>>,
}

impl TableRef {
    /// 行数。
    pub fn rows(&self) -> usize {
        self.cells.len()
    }

    /// 最大栅格列数。
    pub fn cols(&self) -> usize {
        self.cells.iter().map(|r| r.len()).max().unwrap_or(0)
    }

    /// 单元格段落号（越界 `None`）。
    pub fn paragraph(&self, row: usize, col: usize) -> Option<usize> {
        self.cells.get(row).and_then(|r| r.get(col)).copied()
    }
}

impl std::fmt::Debug for DocxBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocxBridge")
            .field("lines", &self.rows.len())
            .field("paragraphs", &self.proj.len())
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)] // 诊断/测试面（stats/snapshot/original/block/rows…）
impl DocxBridge {
    /// 由行模型产物的块表 + 段落原文构造（`blocks` 与行同长）。
    pub fn new(blocks: Vec<LineBlock>, orig: Vec<Option<String>>) -> Self {
        let n = orig.len();
        Self {
            rows: blocks,
            proj: vec![None; n],
            fallback: vec![None; n],
            pending: vec![None; n],
            orig,
            cells: Vec::new(),
            tables: Vec::new(),
        }
    }

    /// 登记「打开态投影」（`docx_doc` 转换时逐段落账：段落 p 的段内行
    /// 文本，多行以 \n 连接）。
    pub fn note_projection(&mut self, paragraph: usize, text: &str) {
        if paragraph < self.proj.len() {
            self.proj[paragraph] = Some(text.to_string());
        }
    }

    /// 登记表格单元格段落号（见字段文档）。
    pub fn note_cells(&mut self, cells: Vec<usize>) {
        self.cells = cells;
    }

    /// 登记表格侧表（**权威坐标**——由 [`DocxPort::table_cells`] 原样喂入）。
    pub fn note_tables(&mut self, tables: Vec<TableRef>) {
        self.tables = tables;
    }

    /// 侧表（只读）。
    pub fn tables(&self) -> &[TableRef] {
        &self.tables
    }

    /// 单元格 (table, row, col) 的段落号（越界 `None`）。
    pub fn cell_paragraph(&self, table: usize, row: usize, col: usize) -> Option<usize> {
        self.tables.get(table).and_then(|t| t.paragraph(row, col))
    }

    /// 段落号是否落在某个表格单元格里。
    pub fn is_cell_paragraph(&self, paragraph: usize) -> bool {
        self.cells.binary_search(&paragraph).is_ok()
            || self
                .tables
                .iter()
                .any(|t| t.cells.iter().any(|r| r.iter().any(|p| *p == paragraph)))
    }

    /// 登记「回写兜底投影」（段内行去掉列表标记/缩进后的正文文本）。
    pub fn note_fallback(&mut self, paragraph: usize, text: &str) {
        if paragraph < self.fallback.len() {
            self.fallback[paragraph] = Some(text.to_string());
        }
    }

    /// 行数。
    pub fn line_count(&self) -> usize {
        self.rows.len()
    }

    /// 行归属（越界 None）。
    pub fn block(&self, line: usize) -> Option<LineBlock> {
        self.rows.get(line).copied()
    }

    /// 行归属表（可变——钩子侧重排用）。
    pub fn rows_mut(&mut self) -> &mut Vec<LineBlock> {
        &mut self.rows
    }

    /// 行归属表（只读）。
    pub fn rows(&self) -> &[LineBlock] {
        &self.rows
    }

    /// 段落数。
    pub fn paragraph_count(&self) -> usize {
        self.proj.len()
    }

    /// 段落打开态投影。
    pub fn snapshot(&self, paragraph: usize) -> Option<&str> {
        self.proj.get(paragraph).and_then(|u| u.as_deref())
    }

    /// 段落原文装配文本（回归测试闸门）。
    pub fn original(&self, paragraph: usize) -> Option<&str> {
        self.orig.get(paragraph).and_then(|o| o.as_deref())
    }

    /// 段落是否动过（相对打开态投影）。
    pub fn touched(&self, paragraph: usize) -> bool {
        self.pending.get(paragraph).and_then(|p| p.as_deref())
            != self.proj.get(paragraph).and_then(|u| u.as_deref())
    }

    // -----------------------------------------------------------------------
    // 提交：Buffer 行文本 → 回写计划 → 引擎
    // -----------------------------------------------------------------------

    /// 由当前 Buffer 行文本计算回写计划（纯函数——不触碰引擎）。
    ///
    /// 三段判定（口径见模块文档「打开态投影」）：
    /// 1. 段落**整行投影**与打开态一致 → 未动，**零 set**（回写原文 =
    ///    连 `\t`/软换行在内的原文装配文本，引擎无需动手术）；
    /// 2. 整行投影变了（用户改了列表标记/行首缩进）但**正文**没变 →
    ///    仍算未动，零 set；
    /// 3. 正文变了 → 写正文（标记/缩进是行模型的装饰，不属于 docx 文本；
    ///    列表的编号定义/缩进属性本身不动——登记）。
    pub fn compute(&self, lines: &[String]) -> Writeback {
        let n = self.proj.len();
        let mut slots: Vec<Vec<(usize, String)>> = vec![Vec::new(); n];
        // 段落里「打开态单元格文本」的**回写口径槽位**（含空单元格）：
        let mut dropped = 0usize;
        for (i, block) in self.rows.iter().enumerate() {
            let text = lines.get(i).cloned().unwrap_or_default();
            // 越界段落号（映射漂移的防线）：只计数、**绝不**索引——否则
            // 超长语料会在这里越界 panic（#115 实测 bug65649：38596）。
            match *block {
                LineBlock::Para { paragraph, idx, .. } => {
                    if paragraph < n && paragraph < slots.len() {
                        push_slot(&mut slots[paragraph], idx, text);
                    } else {
                        dropped += 1;
                    }
                }
                LineBlock::Foreign {
                    id: (paragraph, idx),
                    ..
                } => {
                    if paragraph < n && paragraph < slots.len() {
                        push_slot(&mut slots[paragraph], idx, text);
                    } else {
                        dropped += 1;
                    }
                }
                LineBlock::User { .. } => match nearest_paragraph(&self.rows, i) {
                    Some((p, idx)) if p < n && p < slots.len() => {
                        push_slot(&mut slots[p], idx + 1, text)
                    }
                    _ => dropped += 1,
                },
                // 单元格行：回写目标 = 侧表里的权威段落号。纯删除时
                // 行仍在（占位空串），由下面「清空 ≠ 删段」的闸门拦下。
                LineBlock::Cell { table, row, col } => match self.cell_paragraph(table, row, col) {
                    Some(p) if p < n && p < slots.len() => push_slot(&mut slots[p], 0, text),
                    _ => dropped += 1,
                },
                LineBlock::Locked(_) => {}
            }
        }
        let mut sets = Vec::new();
        let mut unchanged = 0usize;
        for (p, slot) in slots.iter().enumerate() {
            if slot.is_empty() {
                // 没有任何行归属到本段（锁定段/空段/表格段）——原文不动。
                unchanged += 1;
                continue;
            }
            let mut rows = slot.clone();
            rows.sort_by_key(|(k, _)| *k);
            let texts: Vec<String> = rows.into_iter().map(|(_, t)| t).collect();
            let as_model = compose(&texts);
            let as_body = body_join(&texts);
            let model_same = self.proj[p].as_deref() == Some(as_model.as_str());
            let body_same = self.fallback[p].as_deref() == Some(as_body.as_str());
            // 表格单元格的**第 0 段**闸门：单元格行在行模型里显示的是
            // 原文（空格压成单空格、\t 展开等近似），用户没动它时
            // as_body 与原文不等——若直接写回就会把没碰过的格子改坏。
            // 口径与「正文段落投影近似」一致：只要与**原文装配文本**逐字
            // 相同就零 set（下面那条分支已覆盖），故此处只需保证单元格
            // 的投影/兜底登记**与行文本同口径**（由 docx_doc 负责）。
            if model_same || body_same {
                // 未动（或只动了标记/缩进这类行模型装饰）——不写。
                unchanged += 1;
            } else if as_body == self.orig[p].clone().unwrap_or_default() {
                // 与**原文装配文本**逐字相同（投影是行模型的近似、原文
                // 没变）——写回去也是零语义变化，不写（避免引擎把它当成
                // 一次手术：多 run 段合并、xml:space 改写）。
                unchanged += 1;
            } else {
                sets.push((p, as_body));
            }
        }
        sets.sort_by_key(|(p, _)| *p);
        Writeback {
            sets,
            unchanged,
            dropped,
        }
    }

    /// 计划落地：逐段 set（与上次同值跳过）。返回 (set 成功数, 跳过数, 失败)。
    pub fn apply<F>(&mut self, plan: Writeback, mut set: F) -> (usize, usize, Vec<String>)
    where
        F: FnMut(usize, &str) -> Result<(), String>,
    {
        let mut applied = 0;
        let mut skipped = 0;
        let mut failures = Vec::new();
        for (p, text) in plan.sets {
            if self.pending.get(p).and_then(|x| x.as_deref()) == Some(text.as_str()) {
                skipped += 1;
                continue;
            }
            match set(p, &text) {
                Ok(()) => {
                    if p < self.pending.len() {
                        self.pending[p] = Some(text);
                    }
                    applied += 1;
                }
                Err(e) => failures.push(e),
            }
        }
        (applied, skipped, failures)
    }

    /// 诊断快照。
    pub fn stats(&self, buffer_lines: usize) -> BridgeStats {
        let docx_lines = self
            .rows
            .iter()
            .filter(|b| {
                matches!(
                    b,
                    LineBlock::Para { .. } | LineBlock::User { .. } | LineBlock::Cell { .. }
                )
            })
            .count();
        let touched = (0..self.proj.len()).filter(|&p| self.touched(p)).count();
        let engine_pending = self.pending.iter().filter(|p| p.is_some()).count();
        BridgeStats {
            lines: buffer_lines,
            docx_lines,
            readonly_lines: self.rows.len().saturating_sub(docx_lines),
            touched_paragraphs: touched,
            engine_pending,
        }
    }
}

fn push_slot(slot: &mut Vec<(usize, String)>, idx: usize, text: String) {
    // 同一段内序号冲突（用户行插在同段中间）：退化为「接在该序号之后」。
    let mut k = idx;
    while slot.iter().any(|(j, _)| *j == k) {
        k += 1;
    }
    slot.push((k, text));
}

/// 第 i 行为用户新行时，归到哪个段落的哪个序号（最近的 `Para` 邻行）。
fn nearest_paragraph(rows: &[LineBlock], i: usize) -> Option<(usize, usize)> {
    for back in (0..i).rev() {
        match rows[back] {
            LineBlock::Para { paragraph, idx, .. }
            | LineBlock::Foreign {
                id: (paragraph, idx),
                ..
            } => return Some((paragraph, idx)),
            // 单元格行是**边界**：在表格里敲出的用户行不得把上方/下方
            // 正文段落拉进来改写（否则改表格会静默改正文）。
            LineBlock::Cell { .. } => return None,
            LineBlock::User { .. } | LineBlock::Locked(_) => {}
        }
    }
    None
}

/// before_edit 裁决结果（`Ok` 放行；`Err(原因)` 拒——Buffer 零变更）。
pub type Verdict = Result<(), String>;

impl DocxBridge {
    /// 编辑门禁：起点/终点行都必须是可编辑行（锁定行 → 拒）。
    pub fn before_edit(&self, c: Coalesce) -> Verdict {
        for line in [c.start.0, c.end.0] {
            if let Some(LineBlock::Locked(kind)) = self.rows.get(line).copied() {
                return Err(kind.reason().to_string());
            }
        }
        let _ = c.inserted_lines;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EditHook 实现（Buffer 侧唯一入口）
// ---------------------------------------------------------------------------

impl EditHook for DocxBridge {
    fn before_edit(&mut self, intent: &EditIntent) -> Result<(), String> {
        DocxBridge::before_edit(
            self,
            Coalesce {
                start: (intent.start.line, intent.start.col),
                end: (intent.end.line, intent.end.col),
                inserted_lines: intent.inserted_lines,
            },
        )
    }

    fn after_edit(&mut self, summary: &EditSummary) {
        remap_after_edit(
            &mut self.rows,
            (summary.start.line, summary.start.col),
            (summary.end.line, summary.end.col),
            summary.inserted_lines,
            summary.newline_removed,
        );
    }
}

// ---------------------------------------------------------------------------
// Editor 侧组合（桥本体 + 引擎窄面——`EditHook` 的实现体，供 Buffer 持有）
// ---------------------------------------------------------------------------

/// 编辑器内 docx 编辑面：映射桥 + 引擎窄面（`Box<dyn EditHook>` 的实体）。
///
/// Buffer 持有它的**句柄**（唯一可变通路），桥本体与引擎留在 `Editor`
/// 里——「Buffer 每次变更 → 桥改行归属 + **当场节流回写**」在
/// [`EditHook::after_edit`] 里完成；保存只需把已落账的引擎字节落盘。
pub struct DocxEditing {
    /// 映射桥本体（Editor 持有）。
    pub bridge: DocxBridge,
    /// 引擎窄面（Editor 持有）。
    pub engine: Box<dyn DocxPort>,
    /// 距上次回写的变更数（节流计数——本版：**每 1 次变更即回写**
    /// 待落账层；引擎落账是内存态、写盘由 Ctrl+S 触发，故无需时间节流，
    /// 见 #115 验证记录「节流」条）。
    pub edits_since_flush: usize,
    /// 累计回写统计（apply 的 (成功, 同值, 失败)——诊断/证据行用）。
    pub stats: (usize, usize, usize),
    /// 最近一次编辑起点（撤销后据此精确重排行归属）。
    pub last_edit: Option<(usize, usize)>,
}

impl UndoHook for std::rc::Rc<std::cell::RefCell<DocxEditing>> {
    fn before_history(&mut self, _redo: bool, delta: i64) -> Result<(), String> {
        // 行数先对齐（变化量来自撤销记录本身——精确），精确归属等
        // `after_history` 拿到 Buffer 实际行数后再从最近编辑点重排。
        let mut e = self.borrow_mut();
        let want = (e.bridge.line_count() as i64 + delta).max(1) as usize;
        let near = e.last_edit;
        reshape_rows(e.bridge.rows_mut(), want, near);
        Ok(())
    }

    fn after_history(&mut self, _redo: bool, lines: usize) {
        let mut e = self.borrow_mut();
        let near = e.last_edit;
        reshape_rows(e.bridge.rows_mut(), lines, near);
    }
}

impl DocxEditing {
    /// 由桥 + 引擎构造。
    pub fn new(bridge: DocxBridge, engine: Box<dyn DocxPort>) -> Self {
        Self {
            bridge,
            engine,
            edits_since_flush: 0,
            stats: (0, 0, 0),
            last_edit: None,
        }
    }

    /// 按当前 Buffer 行文本回写（Buffer 侧钩子自动喂——这里只做计划与
    /// 落账）。返回本次计划（无变更时 `sets` 为空）。
    pub fn flush(&mut self, lines: &[String]) -> Writeback {
        let plan = self.bridge.compute(lines);
        let engine = &mut self.engine;
        let (applied, skipped, failures) = self
            .bridge
            .apply(plan.clone(), |p, t| engine.set_text(p, t));
        self.stats.0 += applied;
        self.stats.1 += skipped;
        self.stats.2 += failures.len();
        for f in &failures {
            eprintln!("docx-edit: 段落写回被引擎拒绝——{f}");
        }
        if applied + failures.len() > 0 {
            eprintln!(
                "docx-edit: 回写 {} 段（成功 {applied} / 同值 {skipped} / 失败 {}）",
                plan.sets.len(),
                failures.len()
            );
        }
        self.edits_since_flush = 0;
        plan
    }
}

impl EditHook for DocxEditing {
    fn before_edit(&mut self, intent: &EditIntent) -> Result<(), String> {
        self.bridge.before_edit(Coalesce {
            start: (intent.start.line, intent.start.col),
            end: (intent.end.line, intent.end.col),
            inserted_lines: intent.inserted_lines,
        })
    }

    fn after_edit(&mut self, summary: &EditSummary) {
        // 行归属已由 `DocxBridge::after_edit` 系语义的 remap 在同一钩子里
        // 完成？——否：本类型自己是钩子，故在此显式重排。
        remap_after_edit(
            self.bridge.rows_mut(),
            (summary.start.line, summary.start.col),
            (summary.end.line, summary.end.col),
            summary.inserted_lines,
            summary.newline_removed,
        );
        self.last_edit = Some((summary.start.line, summary.start.col));
        self.edits_since_flush += 1;
    }
}

/// `Rc<RefCell<DocxEditing>>` 的钩子转发（Buffer 持句柄，Editor 持本体）。
impl EditHook for std::rc::Rc<std::cell::RefCell<DocxEditing>> {
    fn before_edit(&mut self, intent: &EditIntent) -> Result<(), String> {
        self.borrow_mut().before_edit(intent)
    }

    fn after_edit(&mut self, summary: &EditSummary) {
        self.borrow_mut().after_edit(summary)
    }
}

// ---------------------------------------------------------------------------
// 撤销/重做后的行归属兜底重排
// ---------------------------------------------------------------------------

/// 把行归属表调整到 `want` 行；从 `near`（最近编辑点）起按上下文的段
/// 身份逐行重排，收敛不了的行退化为 `User`。
///
/// 撤销/重做是物理回放，钩子看不到「哪一段」——但**精确归属**其实有
/// 替代真源：保存时逐段做「正文比对」，拿不对的段按原文写回（绝不误改）。
/// 这里只保证**行数守恒 + 段身份不越界**，避免行数漂移导致整篇错位。
fn reshape_rows(rows: &mut Vec<LineBlock>, want: usize, near: Option<(usize, usize)>) {
    while rows.len() < want {
        let idx = near.map(|(l, _)| l.min(rows.len())).unwrap_or(rows.len());
        let src = rows
            .get(idx.saturating_sub(1))
            .copied()
            .unwrap_or(LineBlock::User { join: false });
        let row = match src {
            LineBlock::Para {
                paragraph, idx: i, ..
            } => LineBlock::Para {
                paragraph,
                idx: i,
                cont: true,
            },
            LineBlock::Cell { .. } | LineBlock::Locked(_) => LineBlock::User { join: false },
            other => other,
        };
        rows.insert(idx.min(rows.len()), row);
    }
    while rows.len() > want {
        let idx = near
            .map(|(l, _)| l.min(rows.len().saturating_sub(1)))
            .unwrap_or(rows.len() - 1);
        rows.remove(idx);
    }
}

/// 映射状态快照（诊断/测试面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub struct BridgeStats {
    /// 文本行数（= Buffer 行数）。
    pub lines: usize,
    /// 可编辑行数（段落行 + 用户行）。
    pub docx_lines: usize,
    /// 锁定行数。
    pub readonly_lines: usize,
    /// 已动过的段落数。
    pub touched_paragraphs: usize,
    /// 已 set 进引擎、尚未落盘的段落数。
    pub engine_pending: usize,
}

/// 行模型投影拆解：把「列表标记 + w:ind 缩进近似 + 正文」拆成三段。
///
/// `has_marker`：本行带列表标记（首行且段有编号）。`rest` 为该行去掉
/// 标记之后的部分（前导空格即缩进近似）。返回 (正文, 缩进填充)。
#[allow(dead_code)]
pub fn split_projection(has_marker: bool, rest: &str) -> (String, String) {
    if !has_marker {
        return (rest.to_string(), String::new());
    }
    let indent_len = rest.len() - rest.trim_start_matches(' ').len();
    (
        rest[indent_len..].to_string(),
        rest[..indent_len].to_string(),
    )
}

/// 仅测试/诊断：段内序号表（段落 p 的已登记行序号集合）。
#[allow(dead_code)]
pub fn index_map(rows: &[LineBlock]) -> HashMap<usize, Vec<usize>> {
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for r in rows {
        if let LineBlock::Para { paragraph, idx, .. } = r {
            out.entry(*paragraph).or_default().push(*idx);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(p: usize, idx: usize) -> LineBlock {
        LineBlock::Para {
            paragraph: p,
            idx,
            cont: idx > 0,
        }
    }

    fn locked(k: ReadOnlyLine) -> LineBlock {
        LineBlock::Locked(k)
    }

    /// 由 (行归属, 行文本) 构造桥；orig = 各段**原文装配文本**（引擎口径），
    /// 投影/兜底按行文本派生（与 `docx_doc` 转换同口径）。
    fn bridge(rows: &[(LineBlock, &str)], orig: Vec<&str>) -> DocxBridge {
        let blocks: Vec<LineBlock> = rows.iter().map(|(b, _)| *b).collect();
        let mut b = DocxBridge::new(
            blocks,
            orig.iter().map(|s| Some((*s).to_string())).collect(),
        );
        let mut per: HashMap<usize, Vec<(usize, String)>> = HashMap::new();
        for (block, text) in rows {
            match block {
                LineBlock::Para { paragraph, idx, .. } => {
                    per.entry(*paragraph)
                        .or_default()
                        .push((*idx, (*text).to_string()));
                }
                LineBlock::Foreign { id: (p, i), .. } => {
                    per.entry(*p).or_default().push((*i, (*text).to_string()));
                }
                // 单元格行由测试自己 note_tables（见桥的 tables 侧表）。
                LineBlock::User { .. } | LineBlock::Cell { .. } | LineBlock::Locked(_) => {}
            }
        }
        for (p, mut cells) in per {
            cells.sort_by_key(|(i, _)| *i);
            let texts: Vec<String> = cells.into_iter().map(|(_, t)| t).collect();
            b.note_projection(p, &compose(&texts));
            b.note_fallback(p, &body_join(&texts));
        }
        b
    }

    fn texts(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn compute_skips_untouched_paragraphs() {
        let b = bridge(
            &[
                (para(0, 0), "第一段"),
                (locked(ReadOnlyLine::TableRule), "---"),
                (para(1, 0), "第二段"),
            ],
            vec!["第一段", "第二段"],
        );
        let wb = b.compute(&texts(&["第一段", "---", "第二段"]));
        assert!(wb.sets.is_empty(), "未动 = 零 set（引擎不动手术）");
        assert_eq!(wb.unchanged, 2);
    }

    #[test]
    fn compute_reports_only_changed_paragraph() {
        let b = bridge(
            &[(para(0, 0), "第一段"), (para(1, 0), "第二段")],
            vec!["第一段", "第二段"],
        );
        let wb = b.compute(&texts(&["第一段", "第二段改"]));
        assert_eq!(wb.sets, vec![(1, "第二段改".to_string())]);
        assert_eq!(wb.dropped, 0);
    }

    #[test]
    fn compute_strips_list_marker_on_writeback() {
        let b = bridge(
            &[(para(0, 0), "1. 第一项"), (para(1, 0), "第二项")],
            vec!["第一项", "第二项"],
        );
        // 用户改了列表项正文（列表标记在行模型里是装饰）。
        let wb = b.compute(&texts(&["1. 第一项改", "第二项"]));
        assert_eq!(wb.sets, vec![(0, "第一项改".to_string())]);
    }

    #[test]
    fn compute_cont_line_empty_skipped() {
        // 段 1 两行：首行带标记、续行承接。清空承接行 → 不把段清成空串。
        let b = bridge(
            &[
                (para(0, 0), "标题"),
                (para(1, 0), "1. 第一项"),
                (para(1, 1), "续行"),
            ],
            vec!["标题", "第一项\n续行"],
        );
        // 清空承接行 → 写回「首行正文 + 空续行」（段内换行位置**原样**：
        // 不偷偷吃掉目标文本，也不让承接行消失）。
        let wb = b.compute(&texts(&["标题", "1. 第一项", ""]));
        assert_eq!(wb.sets, vec![(1, "第一项\n".to_string())]);
    }

    #[test]
    fn marker_only_edit_is_not_a_change() {
        // 只把标记后的空格删掉/换一种标记、正文没动 → 不算变更（不触发
        // 引擎手术；列表编号定义/段落属性本来也不归行文本管）。
        let b = bridge(&[(para(0, 0), "1. 甲")], vec!["甲"]);
        for variant in ["2. 甲", "\u{2022} 甲"] {
            let wb = b.compute(&texts(&[variant]));
            assert!(
                wb.sets.is_empty(),
                "{variant:?} 正文相同 → 零 set，实得 {:?}",
                wb.sets
            );
        }
        // 正文真的改了 → 写正文（不带标记）。
        let wb = b.compute(&texts(&["2. 甲改"]));
        assert_eq!(wb.sets, vec![(0, "甲改".to_string())]);
        // 标记与分隔空格一起被删（`"1.甲"`）：正文口径变了 → 按整行写回——
        // 语义仍然是「用户改了这个词」，本轮算一次正常编辑（登记）。
        let wb = b.compute(&texts(&["1.甲"]));
        assert_eq!(wb.sets, vec![(0, "1.甲".to_string())]);
    }

    #[test]
    fn soft_break_lines_roundtrip() {
        let b = bridge(
            &[(para(0, 0), "上行"), (para(0, 1), "下行")],
            vec!["上行\n下行"],
        );
        assert!(b.compute(&texts(&["上行", "下行"])).sets.is_empty());
        assert_eq!(
            b.compute(&texts(&["上行", "下行改"])).sets,
            vec![(0, "上行\n下行改".to_string())]
        );
    }

    #[test]
    fn unrepresentable_projection_is_not_written() {
        // 行模型投影与原文不同（`\t` 等宽近似等）且用户没动它 → 绝不
        // 做引擎手术（否则多 run 段被合并）。
        let mut b = DocxBridge::new(vec![para(0, 0)], vec![Some("甲\t乙".to_string())]);
        b.note_projection(0, "甲   乙");
        b.note_fallback(0, "甲   乙");
        let wb = b.compute(&texts(&["甲   乙"]));
        assert!(wb.sets.is_empty(), "投影≠原文且未编辑 → 不写");
    }

    #[test]
    fn merged_lines_remap_preserves_row_count() {
        // 合并第 0 行与第 1 行：删 1 行、无插入 → 行数守恒，被删行退位。
        let mut rows = vec![para(0, 0), para(1, 0), para(2, 0), para(2, 1)];
        remap_after_edit(&mut rows, (0, 3), (1, 0), 0, true);
        assert_eq!(rows.len(), 3, "行数守恒：{:?}", rows);
        assert_eq!(rows[2], para(2, 1), "后续段行身份不变（序号不漂）");
    }

    #[test]
    fn split_line_inherits_paragraph() {
        let mut rows = vec![para(0, 0), para(1, 0), para(2, 0), para(2, 1)];
        // 在第 0 行中间敲回车：插入 1 行（同段）。
        remap_after_edit(&mut rows, (0, 1), (0, 1), 1, false);
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows[0..2],
            [para(0, 0), para(0, 1)],
            "段内拆分：新行归本段（序号 0/1）"
        );
        assert_eq!(
            rows[2..],
            [para(1, 0), para(2, 0), para(2, 1)],
            "后续行身份不变"
        );
    }

    #[test]
    fn edit_on_locked_line_is_refused() {
        let mut b = bridge(
            &[
                (para(0, 0), "正文"),
                (locked(ReadOnlyLine::TableRow), "a | b"),
            ],
            vec!["正文"],
        );
        let err = EditHook::before_edit(
            &mut b,
            &EditIntent {
                start: crate::editor::buffer::Point::new(1, 0),
                end: crate::editor::buffer::Point::new(1, 0),
                inserted_lines: 0,
                joins_next: false,
                joins_prev: false,
            },
        );
        assert!(err.is_err(), "锁定行拒改");
        assert!(err.unwrap_err().contains("表格"));
    }

    #[test]
    fn user_rows_join_nearest_paragraph() {
        // 在第 1 行（段 1 首行）之前插入用户行 → 本段行（承接续行）。
        let mut rows = vec![para(0, 0), para(1, 0)];
        remap_after_edit(&mut rows, (1, 0), (1, 0), 1, false);
        assert_eq!(rows.len(), 3, "行数 +1");
        // 行归属：新行由上一行（段 0）承接（正文比对负责最终归位）。
        assert!(
            matches!(rows[1], LineBlock::Para { paragraph: 0, .. })
                || matches!(rows[1], LineBlock::User { .. }),
            "新行承接上一行归属：{:?}",
            rows[1]
        );
        let mut b = DocxBridge::new(rows, vec![Some("甲".into()), Some("乙".into())]);
        b.note_projection(0, "甲");
        b.note_projection(1, "乙");
        b.note_fallback(0, "甲");
        b.note_fallback(1, "乙");
        let wb = b.compute(&texts(&["甲", "新行", "乙"]));
        // 新行由上一行（段 0）承接 → 段 0 变成两行；段 1 原样不动。
        assert_eq!(wb.sets, vec![(0, "甲\n新行".to_string())]);
    }

    #[test]
    fn stats_counts_locked_rows() {
        let b = bridge(
            &[
                (para(0, 0), "甲"),
                (locked(ReadOnlyLine::TableGap), ""),
                (para(1, 0), "乙"),
            ],
            vec!["甲", "乙"],
        );
        let s = b.stats(3);
        assert_eq!((s.lines, s.docx_lines, s.readonly_lines), (3, 2, 1));
    }

    #[test]
    fn body_of_strips_markers_and_indent() {
        assert_eq!(body_of("1. 正文"), "正文");
        assert_eq!(body_of("12. 正文"), "正文");
        assert_eq!(body_of("\u{2022} 正文"), "正文");
        assert_eq!(body_of("\u{00b7} 正文"), "正文");
        assert_eq!(body_of("  正文"), "  正文", "缩进不在正文剥离面内");
        // 非标记形态原样。
        assert_eq!(body_of("1.2 版本说明"), "1.2 版本说明");
    }

    #[test]
    fn split_projection_keeps_prefix_and_indent_out_of_body() {
        assert_eq!(
            split_projection(true, " 正文"),
            ("正文".to_string(), " ".to_string())
        );
        assert_eq!(
            split_projection(false, "  正文"),
            ("  正文".to_string(), String::new())
        );
    }

    // ------------------------------------------------------------------
    // #115 表格编辑：LineBlock::Cell + 侧表回写
    // ------------------------------------------------------------------

    /// 带侧表的桥：段落 0 是正文，段落 1..=4 是 2×2 表格的四个格。
    fn table_bridge() -> DocxBridge {
        let rows = [
            (para(0, 0), "正文"),
            (
                LineBlock::Cell {
                    table: 0,
                    row: 0,
                    col: 0,
                },
                "A1",
            ),
            (
                LineBlock::Cell {
                    table: 0,
                    row: 0,
                    col: 1,
                },
                "B1",
            ),
            (
                LineBlock::Cell {
                    table: 0,
                    row: 1,
                    col: 0,
                },
                "A2",
            ),
            (
                LineBlock::Cell {
                    table: 0,
                    row: 1,
                    col: 1,
                },
                "B2",
            ),
        ];
        let orig = vec!["正文", "A1", "B1", "A2", "B2"];
        let mut b = bridge(&rows, orig);
        b.note_tables(vec![TableRef {
            index: 0,
            cells: vec![vec![1, 2], vec![3, 4]],
        }]);
        b
    }

    #[test]
    fn cell_rows_are_editable_and_write_their_own_paragraph() {
        let b = table_bridge();
        // 未编辑 → 零 set（单元格投影 = 引擎装配文本）。
        let base = texts(&["正文", "A1", "B1", "A2", "B2"]);
        assert!(b.compute(&base).sets.is_empty(), "未编辑的格子零 set");
        // 改第 2 行第 1 列（段落 3）。
        let wb = b.compute(&texts(&["正文", "A1", "B1", "A2改", "B2"]));
        assert_eq!(wb.sets, vec![(3, "A2改".to_string())], "只写目标格");
    }

    #[test]
    fn empty_cell_can_be_filled() {
        let rows = [
            (para(0, 0), "正文"),
            (
                LineBlock::Cell {
                    table: 0,
                    row: 0,
                    col: 0,
                },
                "",
            ),
        ];
        let mut b = bridge(&rows, vec!["正文", ""]);
        b.note_tables(vec![TableRef {
            index: 0,
            cells: vec![vec![1]],
        }]);
        let wb = b.compute(&texts(&["正文", "填空"]));
        assert_eq!(wb.sets, vec![(1, "填空".to_string())]);
    }

    #[test]
    fn editing_a_cell_never_touches_neighbouring_body_paragraph() {
        let b = table_bridge();
        let wb = b.compute(&texts(&["正文", "A1", "B1改", "A2", "B2"]));
        assert_eq!(
            wb.sets,
            vec![(2, "B1改".to_string())],
            "只改格子，正文段零接触"
        );
        assert!(
            !wb.sets.iter().any(|(p, _)| *p == 0),
            "正文段不得被牵连：{:?}",
            wb.sets
        );
    }

    #[test]
    fn cell_rows_refuse_edits_via_locked_only_not_cell() {
        // 单元格行是**可编辑**的（不是 Locked）；只读表格形态用 TableRow。
        let mut b = bridge(
            &[
                (para(0, 0), "正文"),
                (locked(ReadOnlyLine::TableRow), "a | b"),
            ],
            vec!["正文"],
        );
        let err = b.before_edit(Coalesce {
            start: (1, 0),
            end: (1, 0),
            inserted_lines: 0,
        });
        assert!(err.is_err(), "只读表格行拒改");
        // 可编辑单元格行放行。
        let b2 = table_bridge();
        assert!(
            b2.before_edit(Coalesce {
                start: (1, 0),
                end: (1, 0),
                inserted_lines: 0,
            })
            .is_ok(),
            "可编辑单元格放行"
        );
    }

    #[test]
    fn cell_paragraph_is_authoritative_not_inferred() {
        // 侧表说格 (0,1) 是段落 2——即使行序上它排在第 2 行位置，也写 2。
        let b = table_bridge();
        assert_eq!(b.cell_paragraph(0, 0, 0), Some(1));
        assert_eq!(b.cell_paragraph(0, 0, 1), Some(2));
        assert_eq!(b.cell_paragraph(0, 1, 0), Some(3));
        assert_eq!(b.cell_paragraph(0, 1, 1), Some(4));
        assert_eq!(b.cell_paragraph(1, 0, 0), None, "越界表 = None（不 panic）");
    }

    #[test]
    fn cell_without_side_table_is_dropped_not_written() {
        // 侧表缺席（引擎没给栅格）→ 该 Cell 行无权威坐标，计入 dropped，
        // **绝不**猜一个段落号去写。
        let b = bridge(
            &[(
                LineBlock::Cell {
                    table: 0,
                    row: 0,
                    col: 0,
                },
                "X",
            )],
            vec!["X"],
        );
        let wb = b.compute(&texts(&["X改"]));
        assert!(wb.sets.is_empty(), "无侧表 → 不写：{:?}", wb.sets);
        assert_eq!(wb.dropped, 1);
    }

    #[test]
    fn cell_is_boundary_for_user_row_join() {
        // 在表格行之后敲出的用户行不得被并进上方正文段落。
        let mut rows = vec![
            para(0, 0),
            LineBlock::Cell {
                table: 0,
                row: 0,
                col: 0,
            },
        ];
        remap_after_edit(&mut rows, (2, 0), (2, 0), 1, false);
        assert_eq!(rows.len(), 3);
        assert!(
            !matches!(rows[2], LineBlock::Para { paragraph: 0, .. }),
            "用户行不得承接正文段：{:?}",
            rows[2]
        );
    }

    #[test]
    fn cell_rows_count_as_editable_in_stats() {
        let b = table_bridge();
        let s = b.stats(5);
        assert_eq!(s.docx_lines, 5, "正文 1 + 格 4 都是可编辑行");
        assert_eq!(s.readonly_lines, 0);
    }
}
