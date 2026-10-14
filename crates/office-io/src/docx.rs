//! DOCX 读/写切片（xlsx-r2 Phase 3 落地 → docx-r1 增量第一轮）。
//!
//! **定位（2026-10-14，用户三阶段指令 Phase 3）**：用既有研究结论
//! （分析仓 ADR-O1 O1-3：docx-rs = docx 写 + 模型级读，bokuweb/docx-rs
//! 0.4.22 MIT）落地**小而可用**的 DOCX 切片。**模型级处理，与渲染
//! 保真分离**：本模块产出/消费结构化文本模型（块/段落/run/表），
//! 不做布局、分页、字体度量或任何渲染语义。
//!
//! **docx-r1 增量（2026-10-15，遗留①）**：主题 = **OPC 关系解析层 +
//! 该层解锁的面**。一手核验底稿 = 分析仓 `office-line/docx-r1/
//! CAPABILITY_NOTES.md`（源码锚点在档；未实测不暴露纪律）：
//! - **OPC 关系层（自有——「OPC 关系解析层」节）**：quick-xml 解析全部 `.rels`
//!   部件（门 3 同族纪律：typed 错误 + 上限恒等两模式）；docx-rs
//!   引擎只透传超链接 r:id 不做联接（CAPABILITY_NOTES §1），
//!   联接由本层承担；
//! - **超链接目标（读）**：run 级 [`HyperlinkTarget`]（外部 URL/
//!   包内目标/文档内锚点/兼容模式未解析四变体）——Phase 3 的
//!   「目标丢弃」数据损失登记**闭项**（表格单元格内目标仍丢——
//!   TableData 扁平文本面，登记见下）；
//! - **超链接（写）**：[`RunSpec::link`] 外部 URL run + 写读全环；
//! - **页眉/页脚（读）**：六槽位（default/first/even × 页眉/页脚，
//!   变体透传——first/even 激活需 titlePg/evenAndOddHeaders 节设置，
//!   语义解释面外）× 块序提取（与文档体同构）；
//! - **页眉/页脚（写）**：[`DocxWriter::header`]/[`DocxWriter::footer`]
//!   （default 槽位单段 run 序）；
//! - **敌意面显式化**：docx-rs 读侧对 .rels 词汇表外元素静默忽略
//!   （`XMLElement::from_str` 全名映射 `Ok(Unsupported)` 永不 Err
//!   ——reader/xml_element.rs:447，CAPABILITY_NOTES §8 实证勘误）;
//!   本层对同一面给显式信号：严格拒（OPC 关系部件模式 ECMA-376
//!   Part 2 词汇表）/ 兼容警告 + 跳过（引擎同等容忍 + 可见性）。
//!
//! **docx-r2 增量（2026-10-16，遗留① 收尾轮）**：主题 = **脚注/图片/
//! 域/first-even 槽位四面收口**（底稿 = 分析仓 docx-r1
//! CAPABILITY_NOTES.md §5/§6/§7 增补 + r2 勘误链）：
//! - **脚注（写）**：[`RunSpec::footnote`] run 级脚注引用（引擎
//!   `add_footnote_reference` + 打包自动收集 footnotes.xml）；**读面
//!   引擎受限维持登记**（reader 从不读 footnotes.xml——CAPABILITY_NOTES
//!   §5，本轮锚点精确化）；
//! - **图片（读）**：**容器级媒体清单** [`DocxDocument::images`]（自有
//!   实现：[Content_Types].xml 解析 + `word/media/*` 枚举 + .rels 联接
//!   + run 级 `a:blip r:embed` 引用计数——引擎面三个静默怪癖在档，
//!     本清单不受其影响）；
//! - **图片（写）**：[`DocxWriter::image`] **PNG-only 内联图**——勘误：
//!   引擎 `Pic::new_with_dimensions` **不受** `image` 特性门控（r1 §6
//!   初判修正：仅 `Pic::new` 自动转码面被门控）；`image` 特性代价
//!   = +23 crate（登记不启用）；
//! - **域（写·简单域）**：[`RunSpec::field_page`]/
//!   [`RunSpec::field_num_pages`]（fldChar begin/instrText/end 三连，
//!   引擎 `add_tc` 同构先例）；读面维持登记（引擎区分结构但提取面
//!   外）；TOC/PAGEREF/HYPERLINK 域登记不暴露（引擎 `todo!()` 崩溃面
//!   在 instr_text.rs:32）；
//! - **first/even 页眉页脚（写）**：[`DocxWriter::first_header`] 等
//!   四方法（引擎激活语义完整：first 自动 titlePg、even 自动
//!   evenAndOddHeaders）；同槽重复调用 = 原位替换（**保持原槽 rid**
//!   ——r1 的无条件 rIdHeader1 在多槽组合下会踩踏，本轮修复）。
//!
//! ## 模块边界（CAPABILITIES.md §2 裁决——DOCX 独立模块）
//!
//! - 与 XLSX 模块（[`crate::io`]/[`crate::write`]）**不共用领域模型、
//!   不建统一 Office 接口抽象**：本模块自有模型（[`DocxDocument`]/
//!   [`Block`]/[`RunData`]），错误面自有（[`DocxError`]），按格式独立
//!   演进。
//! - 共享面仅限**容器级基础设施**：DOCX 与 XLSX 同为 zip 容器与
//!   OPC 部件约定，复用同一门结构（门 1 声明值预扫 [`crate::io::prescan`]
//!   与门 2 尺寸诚实性、门 3 部件良构性融合趟 [`crate::io`] 内部
//!   机制）以及结构化警告/兼容模式（[`crate::io::ReadOptions`]）——
//!   这是引擎侧输入防线（SECURITY_POLICY v1.1 §2.1），非领域抽象。
//! - 共享消毒面 = mdc-office sanitize 六面（上下文级纯函数，跨格式
//!   复用正当——消费方责任边界同 [`crate::write`] 模块文档）。
//!
//! ## 读面（[`read_docx`] / [`read_docx_from_bytes`]）
//!
//! 容器三门（与 XLSX 读路径同构：严格默认 + 显式兼容）→ **自有 OPC
//! 关系层**（先于引擎——词汇表/上限两模式拒）→ docx-rs `read_docx`
//! 模型解析 → 自有模型提取（关系联接）：
//! - **块序**（段落/表，文档体顺序）；段落 = 样式 ID 透传（如
//!   `Heading1`——语义判定归消费方，模型级不做标题解释）+ run 序
//!   （文本/加粗/斜体/**超链接目标**）；表 = 行 × 单元格拼接文本
//!   （单元格内多段落以 `\n` 连接）；
//! - **页眉/页脚**：六槽位中实际挂接的（引用悬空时：严格拒 /
//!   兼容警告 + 槽位跳过）；
//! - Tab → `\t`、Break → `\n`、CarriageReturn → `\r`（文本模型
//!   保真）；结构化警告面与 XLSX 一致（畸形部件/控制字符）+
//!   [`WarningKind::BrokenReference`]（关系引用悬空，兼容模式降级）。
//!
//! ## 写面（[`DocxWriter`]）
//!
//! 新建文档（**仅新建语义**——与 [`crate::write`] 的 XLSX 边界
//! 同构，不存在已有文档编辑往返）：标题（Heading1-9 样式定义 +
//! outline 级——无字体面[模型级]）/富文本段落（run 级加粗斜体 +
//! 外部超链接）/文本表格/**默认页眉与页脚**（单段 run 序）。写产物
//! 过读面全环往返测试（tests/docx_roundtrip.rs）。
//!
//! ## 不支持面与数据损失登记（诚实边界——模型级切片的现实）
//!
//! 读侧**跳过并登记**（文本提取不受损面外的可见损失）：
//! - 图形/图片/形状（**媒体面已暴露**：容器级清单 [`DocxDocument::
//!   images`]——字节/内容类型/引用关系/run 级引用计数；**块序内图片
//!   位置不入块模型**[Block 无图片块]——模型级文本提取边界，登记）；
//! - **表格单元格内超链接目标**（单元格文本保留，目标丢弃——
//!   TableData 扁平文本面；引擎解析能力在档
//!   [docx-r1/CAPABILITY_NOTES §2，分析仓]）；
//! - 修订追踪（Insert/Delete/MoveFrom/MoveTo 容器——内容不入提取）；
//! - 页码/总页数域（PageNum/NumPages）、书签、批注、
//!   StructuredDataTag（内容控件）；
//! - 脚注/尾注（**读面引擎受限**：docx-rs 从不读 footnotes.xml 且
//!   正文 `w:footnoteReference` 不入模型——CAPABILITY_NOTES §5；
//!   写面已暴露 [`RunSpec::footnote`]，读回文本不含脚注内容——
//!   XML 级断言见 tests/docx_roundtrip.rs）；
//! - 节属性（page size/margin 等——页眉页脚引用面已暴露，其余
//!   节语义面外）；first/even 页眉页脚槽位的**激活设置语义**
//!   （titlePg/evenAndOddHeaders——变体透传，激活判定归消费方）；
//! - 编号/列表语义（列表段落文本保留，编号结构丢弃）；
//! - 域指令文本（instrText 引擎入模型字符串，提取面外——登记；
//!   写面简单域 PAGE/NUMPAGES 已暴露，域结果文本不入读回文本——
//!   域值是渲染期计算面）。
//!
//! 写侧**不支持**（引擎能力在[docx-rs 写面远大于本切片]，未实测
//! 不暴露）：图片自动转码（`Pic::new` 需 `image` 特性 = +23 crate
//! 依赖树——登记，PNG 显式尺寸面已暴露）、非 PNG 图片、浮动定位/
//! 旋转图片、表格样式、TOC/PAGEREF/HYPERLINK 域、目录、编号列表、
//! 页眉页脚内表格、锚点写面（引擎均支持——CAPABILITY_NOTES）。
//!
//! 证据链：tests/docx_roundtrip.rs（写→读全环 + 门结构复用 + 敌意
//! 输入钉定 + .rels 敌意面 + 引擎崩溃面直探针）+ 本模块内联单测。
//!

use crate::io::{self, CompatibilityWarning, ReadOptions, WarningKind};
use std::collections::{HashMap, HashSet};
use std::io::Read as _;

// ---------------------------------------------------------------------------
// 错误面（DOCX 自有——按格式独立演进纪律）
// ---------------------------------------------------------------------------

/// DOCX 读/写统一错误。
#[derive(Debug)]
pub enum DocxError {
    /// 容器门/关系层拒绝（门 1/2/3 + .rels 结构校验——与 XLSX 同一
    /// 门结构；内含具体拒绝面）。
    Container(io::ReadError),
    /// docx-rs 模型解析错误。
    Parse(String),
    /// 文件 IO 错误。
    Io(String),
}

impl std::fmt::Display for DocxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocxError::Container(e) => write!(f, "container: {e}"),
            DocxError::Parse(m) => write!(f, "parse error: {m}"),
            DocxError::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for DocxError {}

impl From<io::ReadError> for DocxError {
    fn from(e: io::ReadError) -> Self {
        DocxError::Container(e)
    }
}

// ---------------------------------------------------------------------------
// OPC 关系解析层（docx-r1：自有——quick-xml 直依赖，零新依赖）
// ---------------------------------------------------------------------------
//
// 引擎缺口（CAPABILITY_NOTES §1 一手锚定）：docx-rs 读路径只把
// 超链接 r:id 透传进模型（reader/hyperlink.rs:21-26，path 置空
// "not used"），包级 `Docx.hyperlinks` 仅收 TargetMode 存在的关系
//（read_docx.rs:386-398）——无 r:id → target 联接。本层补面。
//
// 纪律（门 3 同族）：
// - **上限恒等两模式**（[`LIMIT_RELATIONSHIPS`]）；
// - **词汇表校验**：OPC 关系部件模式（ECMA-376 Part 2）只允许
//   Relationships/Relationship 元素——严格拒 / 兼容警告 + 跳过
//   （引擎同等容忍：`XMLElement::from_str` 全名映射 `Ok(Unsupported)`
//   永不 Err[reader/xml_element.rs:447]，未知元素静默忽略——
//   CAPABILITY_NOTES §8 实证勘误）；
// - 部件级 schema 违反（缺 Id/Type/Target、重复 Id）：严格拒 /
//   兼容警告降级（MalformedXml 类）；
// - 跨部件引用悬空（r:id 无条目/目标部件缺失/越出包根）：严格拒 /
//   兼容警告（BrokenReference 类）。

/// 单个 `.rels` 部件关系条目数上限（与门 1 条目上限同量级——
/// 安全上限恒等两模式）。
pub const LIMIT_RELATIONSHIPS: usize = 10_000;

/// 单条 OPC 关系（`.rels` 部件 `<Relationship>` 条目）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Relationship {
    pub id: String,
    pub rel_type: String,
    /// Target 原样透传（外部 = URL，永不解引用——消费方责任边界；
    /// 内部 = 包内相对路径）。
    pub target: String,
    /// TargetMode（如 `External`；None = 包内目标）。
    pub target_mode: Option<String>,
}

/// 单个 `.rels` 部件的关系表（rId → 关系；重复 Id 兼容首胜）。
#[derive(Debug, Clone, Default)]
pub(crate) struct RelationshipTable {
    by_id: HashMap<String, Relationship>,
}

impl RelationshipTable {
    fn get(&self, rid: &str) -> Option<&Relationship> {
        self.by_id.get(rid)
    }

    fn len(&self) -> usize {
        self.by_id.len()
    }

    /// 条目迭代（媒体清单联接用——遍历序非确定，消费面须自排序）。
    fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.by_id.values()
    }
}

/// 全包 `.rels` 部件解析结果：document 主关系表 + 按属主部件路径
/// 索引的其余关系表（页眉/页脚部件自身 rels——页眉内超链接联接）。
#[derive(Debug, Default)]
pub(crate) struct RelsBundle {
    /// `word/_rels/document.xml.rels` 的表（部件缺失/兼容降级 = None）。
    pub document: Option<RelationshipTable>,
    /// 属主部件路径（如 `word/header1.xml`）→ 该部件的关系表。
    by_part: HashMap<String, RelationshipTable>,
    /// 全包 zip 条目名（悬空目标部件判定用）。
    entries: HashSet<String>,
}

/// 解析单个 `.rels` 部件。
///
/// 返回 `Ok(Some(table))` = 正常；`Ok(None)` = 兼容模式降级（空表 +
/// 警告已记——严格模式下该部件已被门 3 先拒，不可达）；`Err` =
/// 两模式拒（条目数上限）或严格模式 schema 拒（词汇表外元素/缺
/// Id/Type/Target/重复 Id——兼容模式警告 + 跳过/首胜，引擎同等
/// 容忍面见 CAPABILITY_NOTES §8）。
fn parse_rels_part(
    data: &[u8],
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Option<RelationshipTable>, io::ReadError> {
    let malformed = |detail: String| io::ReadError::InvalidPart {
        part: part.to_string(),
        detail,
    };
    // 兼容降级（quick-xml 失败/深度超限/截断）：门 3 已对该部件发过
    // MalformedXml 警告时不重复登记。
    fn degrade(
        part: &str,
        detail: String,
        warnings: &mut Vec<CompatibilityWarning>,
    ) -> Result<Option<RelationshipTable>, io::ReadError> {
        let already = warnings
            .iter()
            .any(|w| w.part == part && w.kind == WarningKind::MalformedXml);
        if !already {
            warnings.push(CompatibilityWarning {
                part: part.to_string(),
                kind: WarningKind::MalformedXml,
                detail: format!("compat mode: rels parse degraded: {detail}"),
            });
        }
        Ok(None)
    }

    let mut reader = quick_xml::Reader::from_reader(data);
    let mut buf = Vec::with_capacity(8 * 1024);
    let mut table = RelationshipTable::default();
    let mut depth: usize = 0;
    use quick_xml::events::Event;
    // 词汇表外元素（含根元素名错误 / 超深嵌套的 Relationship）：
    // 严格拒（OPC 关系部件模式 ECMA-376 Part 2——Relationships/
    // Relationship only）；兼容警告 + 跳过（引擎同等容忍——
    // XMLElement::from_str 全名映射 Ok(Unsupported) 永不 Err
    // [reader/xml_element.rs:447]，未知元素被静默忽略；CAPABILITY_NOTES §8）。
    let unknown_element = |local: &[u8],
                           depth: usize,
                           warnings: &mut Vec<CompatibilityWarning>|
     -> Result<(), io::ReadError> {
        let detail = format!(
            "unknown element '{}' at depth {depth} (rels vocabulary: Relationships/Relationship only)",
            String::from_utf8_lossy(local)
        );
        if strict {
            Err(malformed(detail))
        } else {
            skip_entry(part, &format!("{detail} (tolerated, skipped)"), warnings);
            Ok(())
        }
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = name.local_name();
                let local = local.as_ref();
                if depth == 0 {
                    if local != b"Relationships" {
                        unknown_element(local, depth, warnings)?;
                    }
                } else if local != b"Relationship" || depth > 1 {
                    unknown_element(local, depth, warnings)?;
                } else if let Some(rel) = parse_relationship(&e, &reader, part, strict, warnings)? {
                    insert_relationship(&mut table, rel, part, strict, warnings)?;
                }
                depth += 1;
                if depth > io::LIMIT_XML_DEPTH {
                    return if strict {
                        Err(malformed(format!(
                            "element depth over limit {}",
                            io::LIMIT_XML_DEPTH
                        )))
                    } else {
                        degrade(
                            part,
                            format!("element depth over limit {}", io::LIMIT_XML_DEPTH),
                            warnings,
                        )
                    };
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = name.local_name();
                let local = local.as_ref();
                if depth == 0 {
                    if local != b"Relationships" {
                        unknown_element(local, depth, warnings)?;
                    }
                } else if local != b"Relationship" || depth > 1 {
                    unknown_element(local, depth, warnings)?;
                } else if let Some(rel) = parse_relationship(&e, &reader, part, strict, warnings)? {
                    insert_relationship(&mut table, rel, part, strict, warnings)?;
                }
            }
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return if strict {
                    // 严格模式下门 3 已先拒（.rels 属门 3 校验面）——
                    // 不可达路径，保守同拒。
                    Err(malformed(format!("rels XML parse error: {e}")))
                } else {
                    degrade(part, format!("rels XML parse error: {e}"), warnings)
                };
            }
        }
        buf.clear();
    }
    if depth > 0 {
        // EOF 截断（严格模式下门 3 已先拒——不可达路径，保守同拒）。
        return if strict {
            Err(malformed(format!(
                "truncated: {depth} unclosed element(s) at EOF"
            )))
        } else {
            degrade(
                part,
                format!("truncated: {depth} unclosed element(s) at EOF"),
                warnings,
            )
        };
    }
    Ok(Some(table))
}

/// 解析单个 `<Relationship>` 条目属性（缺任一必需属性 = schema 违反：
/// 严格拒 / 兼容警告 + 跳过该条目）。返回 None = 兼容跳过。
fn parse_relationship(
    e: &quick_xml::events::BytesStart<'_>,
    reader: &quick_xml::Reader<&[u8]>,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Option<Relationship>, io::ReadError> {
    let mut id = String::new();
    let mut rel_type = String::new();
    let mut target = String::new();
    let mut target_mode = None;
    let mut missing: Vec<&str> = Vec::new();
    for attr in e.attributes() {
        let attr = match attr {
            Ok(a) => a,
            Err(e) => {
                let detail = format!("bad attribute: {e}");
                if strict {
                    return Err(io::ReadError::InvalidPart {
                        part: part.to_string(),
                        detail,
                    });
                }
                skip_entry(part, &detail, warnings);
                return Ok(None);
            }
        };
        let key = attr.key.local_name();
        let key = key.as_ref();
        if key == b"Id" {
            // 实体补偿（写侧引擎转义 &amp; 等——读回还原逻辑值；
            // XLSX v2 IO 边界补偿同构）。
            id = attr
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|e| io::ReadError::InvalidPart {
                    part: part.to_string(),
                    detail: format!("bad Id attribute: {e}"),
                })?
                .into_owned();
        } else if key == b"Type" {
            rel_type = attr
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|e| io::ReadError::InvalidPart {
                    part: part.to_string(),
                    detail: format!("bad Type attribute: {e}"),
                })?
                .into_owned();
        } else if key == b"Target" {
            target = attr
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|e| io::ReadError::InvalidPart {
                    part: part.to_string(),
                    detail: format!("bad Target attribute: {e}"),
                })?
                .into_owned();
        } else if key == b"TargetMode" {
            target_mode = Some(
                attr.decoded_and_normalized_value(
                    quick_xml::XmlVersion::Implicit1_0,
                    reader.decoder(),
                )
                .map_err(|e| io::ReadError::InvalidPart {
                    part: part.to_string(),
                    detail: format!("bad TargetMode attribute: {e}"),
                })?
                .into_owned(),
            );
        }
    }
    if id.is_empty() {
        missing.push("Id");
    }
    if rel_type.is_empty() {
        missing.push("Type");
    }
    if target.is_empty() {
        missing.push("Target");
    }
    if !missing.is_empty() {
        let detail = format!("Relationship missing {} attribute(s)", missing.join("/"));
        return if strict {
            Err(io::ReadError::InvalidPart {
                part: part.to_string(),
                detail,
            })
        } else {
            skip_entry(part, &detail, warnings);
            Ok(None)
        };
    }
    Ok(Some(Relationship {
        id,
        rel_type,
        target,
        target_mode,
    }))
}

/// 兼容模式跳条目（警告登记）。
fn skip_entry(part: &str, detail: &str, warnings: &mut Vec<CompatibilityWarning>) {
    warnings.push(CompatibilityWarning {
        part: part.to_string(),
        kind: WarningKind::MalformedXml,
        detail: format!("compat mode: relationship entry skipped: {detail}"),
    });
}

/// 插入关系条目（重复 Id：严格拒 / 兼容警告 + 首胜）。
fn insert_relationship(
    table: &mut RelationshipTable,
    rel: Relationship,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<(), io::ReadError> {
    if table.by_id.contains_key(&rel.id) {
        let detail = format!("duplicate relationship Id '{}'", rel.id);
        return if strict {
            Err(io::ReadError::InvalidPart {
                part: part.to_string(),
                detail,
            })
        } else {
            warnings.push(CompatibilityWarning {
                part: part.to_string(),
                kind: WarningKind::MalformedXml,
                detail: format!("compat mode: {detail} (first entry wins)"),
            });
            Ok(())
        };
    }
    table.by_id.insert(rel.id.clone(), rel);
    if table.len() > LIMIT_RELATIONSHIPS {
        // 上限恒等两模式（在插入路径即时检出，避免超限包全解析）。
        return Err(io::ReadError::InvalidPart {
            part: part.to_string(),
            detail: format!("relationship count over limit {LIMIT_RELATIONSHIPS}"),
        });
    }
    Ok(())
}

/// `.rels` 部件路径 → 属主部件路径（`word/_rels/header1.xml.rels` →
/// `word/header1.xml`；非 `<dir>/_rels/<base>.rels` 形态返回 None——
/// 敌意命名按无表处理）。
fn rels_owner_part(rels_path: &str) -> Option<String> {
    let (dir, file) = rels_path.rsplit_once('/')?;
    let owner_dir = dir.strip_suffix("/_rels")?;
    let base = file.strip_suffix(".rels")?;
    if base.is_empty() {
        return None;
    }
    Some(format!("{owner_dir}/{base}"))
}

// ---------------------------------------------------------------------------
// [Content_Types].xml 解析（docx-r2：媒体清单的部件类型面——自有，
// quick-xml 直依赖，零新依赖；纪律与 rels 层同族）
// ---------------------------------------------------------------------------

/// 包内容类型声明（`[Content_Types].xml` 的 Default/Override 两面）。
#[derive(Debug, Default, Clone)]
pub(crate) struct ContentTypes {
    /// 扩展名（小写归一）→ 内容类型（`<Default Extension="png" .../>`）。
    defaults: HashMap<String, String>,
    /// 部件名（去前导 `/`）→ 内容类型（`<Override PartName=... .../>`）。
    overrides: HashMap<String, String>,
}

impl ContentTypes {
    /// 部件的内容类型（Override 优先于 Default——OPC 语义）；无声明
    /// = None（严格模式拒 / 兼容警告面）。
    fn content_type_of(&self, part_name: &str) -> Option<String> {
        if let Some(ct) = self.overrides.get(part_name) {
            return Some(ct.clone());
        }
        let ext = part_name.rsplit_once('.')?.1.to_ascii_lowercase();
        self.defaults.get(&ext).cloned()
    }
}

/// 解析 `[Content_Types].xml`（词汇表：Types/Default/Override only——
/// 严格拒 / 兼容警告 + 跳过；缺属性 = schema 违反同 rels 层分派；
/// 重复 Default 扩展名 / Override PartName：严格拒 / 兼容首胜）。
fn parse_content_types(
    data: &[u8],
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<ContentTypes, io::ReadError> {
    const PART: &str = "[Content_Types].xml";
    let malformed = |detail: String| io::ReadError::InvalidPart {
        part: PART.to_string(),
        detail,
    };
    // 兼容模式跳条目/降级（警告登记；严格模式同面 typed reject）。
    fn warn_skip(detail: String, warnings: &mut Vec<CompatibilityWarning>) {
        warnings.push(CompatibilityWarning {
            part: "[Content_Types].xml".to_string(),
            kind: WarningKind::MalformedXml,
            detail: format!("compat mode: content-types entry skipped: {detail}"),
        });
    }
    let mut reader = quick_xml::Reader::from_reader(data);
    let mut buf = Vec::with_capacity(4 * 1024);
    let mut out = ContentTypes::default();
    let mut depth: usize = 0;
    use quick_xml::events::Event;
    // 词汇表外元素（含根元素名错误）：严格拒 / 兼容警告 + 跳过
    // （rels 层同族——引擎对 [Content_Types].xml 的 FromXML 是
    // depth==1 全收宽容面，本层给显式信号）。
    let unknown = |local: &[u8], warnings: &mut Vec<CompatibilityWarning>| {
        let detail = format!(
            "unknown element '{}' (content-types vocabulary: Types/Default/Override only)",
            String::from_utf8_lossy(local)
        );
        if strict {
            Err(malformed(detail))
        } else {
            warn_skip(format!("{detail} (tolerated, skipped)"), warnings);
            Ok(())
        }
    };
    loop {
        // Start 与 Empty 合并处理（Empty 无 End 事件——深度仅 Start 递增）。
        let (e, is_start) = match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => (e, true),
            Ok(Event::Empty(e)) => (e, false),
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
                buf.clear();
                continue;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {
                buf.clear();
                continue;
            }
            Err(e) => {
                return if strict {
                    // 严格模式下门 3 已先拒（.xml 部件）——不可达路径，
                    // 保守同拒。
                    Err(malformed(format!("content-types XML parse error: {e}")))
                } else {
                    warnings.push(CompatibilityWarning {
                        part: PART.to_string(),
                        kind: WarningKind::MalformedXml,
                        detail: format!("compat mode: content-types parse degraded: {e}"),
                    });
                    Ok(ContentTypes::default())
                };
            }
        };
        let local = e.name().local_name();
        let local = local.as_ref();
        if depth == 0 {
            if local != b"Types" {
                unknown(local, warnings)?;
            }
        } else if depth == 1 && local == b"Default" {
            match parse_ct_entry(&e, &reader, b"Extension")? {
                Ok((ext, ct)) => {
                    let ext = ext.to_ascii_lowercase();
                    match out.defaults.entry(ext.clone()) {
                        std::collections::hash_map::Entry::Occupied(_) => {
                            let detail = format!("duplicate Default Extension '{ext}'");
                            if strict {
                                return Err(malformed(detail));
                            }
                            warn_skip(format!("{detail} (first wins)"), warnings);
                        }
                        std::collections::hash_map::Entry::Vacant(v) => {
                            v.insert(ct);
                        }
                    }
                }
                Err(detail) => {
                    if strict {
                        return Err(malformed(detail));
                    }
                    warn_skip(detail, warnings);
                }
            }
        } else if depth == 1 && local == b"Override" {
            match parse_ct_entry(&e, &reader, b"PartName")? {
                Ok((part, ct)) => {
                    let part = part.trim_start_matches('/').to_string();
                    match out.overrides.entry(part.clone()) {
                        std::collections::hash_map::Entry::Occupied(_) => {
                            let detail = format!("duplicate Override PartName '{part}'");
                            if strict {
                                return Err(malformed(detail));
                            }
                            warn_skip(format!("{detail} (first wins)"), warnings);
                        }
                        std::collections::hash_map::Entry::Vacant(v) => {
                            v.insert(ct);
                        }
                    }
                }
                Err(detail) => {
                    if strict {
                        return Err(malformed(detail));
                    }
                    warn_skip(detail, warnings);
                }
            }
        } else {
            unknown(local, warnings)?;
        }
        if is_start {
            depth += 1;
            if depth > io::LIMIT_XML_DEPTH {
                return Err(malformed(format!(
                    "element depth over limit {}",
                    io::LIMIT_XML_DEPTH
                )));
            }
        }
        buf.clear();
    }
    if depth > 0 {
        // EOF 截断（严格模式下门 3 已先拒——不可达路径，保守同拒）。
        return if strict {
            Err(malformed(format!(
                "truncated: {depth} unclosed element(s) at EOF"
            )))
        } else {
            warnings.push(CompatibilityWarning {
                part: PART.to_string(),
                kind: WarningKind::MalformedXml,
                detail: format!("compat mode: content-types truncated ({depth} unclosed)"),
            });
            Ok(ContentTypes::default())
        };
    }
    Ok(out)
}

/// `<Default Extension=… ContentType=…/>` / `<Override PartName=…
/// ContentType=…/>` 属性对解析。`Ok(Ok((key, ct)))` = 正常；
/// `Ok(Err(detail))` = schema 违反（缺必需属性/坏属性——调用面分派
/// 严格拒或兼容跳条目）；`Err` = 坏属性值（两模式同拒——quick-xml
/// 层错误非 schema 面）。
fn parse_ct_entry(
    e: &quick_xml::events::BytesStart<'_>,
    reader: &quick_xml::Reader<&[u8]>,
    key_attr: &[u8],
) -> Result<Result<(String, String), String>, io::ReadError> {
    let mut key = String::new();
    let mut ct = String::new();
    for attr in e.attributes() {
        let attr = match attr {
            Ok(a) => a,
            Err(err) => return Ok(Err(format!("bad attribute: {err}"))),
        };
        let k = attr.key.local_name();
        let k = k.as_ref();
        let v = attr
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|err| io::ReadError::InvalidPart {
                part: "[Content_Types].xml".to_string(),
                detail: format!("bad attribute value: {err}"),
            })?
            .into_owned();
        if k == key_attr {
            key = v;
        } else if k == b"ContentType" {
            ct = v;
        }
    }
    let el = if key_attr == b"Extension" {
        "Default"
    } else {
        "Override"
    };
    let key_name = String::from_utf8_lossy(key_attr);
    if key.is_empty() {
        return Ok(Err(format!("{el} missing {key_name} attribute")));
    }
    if ct.is_empty() {
        return Ok(Err(format!("{el} missing ContentType attribute")));
    }
    Ok(Ok((key, ct)))
}

/// 包内部件路径词法归一化（`word/../x/y.xml` → `x/y.xml`）；越出
/// 包根（`../` 弹空）返回 None——敌意路径逃逸面。
fn normalize_part_path(path: &str) -> Option<String> {
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            s => stack.push(s),
        }
    }
    Some(stack.join("/"))
}

/// 构建全包 `.rels` 捆绑（门后字节；条目读取有界——门 2 已验
/// 实际 ≤ 声明 ≤ 上限）。
fn build_rels_bundle<R: std::io::Read + std::io::Seek>(
    za: &mut zip::ZipArchive<R>,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<RelsBundle, io::ReadError> {
    let mut bundle = RelsBundle::default();
    let names: Vec<String> = za.file_names().map(str::to_string).collect();
    bundle.entries = names.iter().cloned().collect();
    for name in &names {
        if !name.ends_with(".rels") {
            continue;
        }
        let mut data = Vec::new();
        let ok = za
            .by_name(name)
            .and_then(|mut zf| zf.read_to_end(&mut data).map(|_| ()).map_err(Into::into));
        if let Err(e) = ok {
            return Err(io::ReadError::ContainerCorrupt(format!(
                "read rels part {name}: {e}"
            )));
        }
        if let Some(table) = parse_rels_part(&data, name, strict, warnings)? {
            if name == "word/_rels/document.xml.rels" {
                bundle.document = Some(table);
            } else if let Some(owner) = rels_owner_part(name) {
                bundle.by_part.insert(owner, table);
            }
        }
        // 兼容降级（None）——表缺席，警告已记。
    }
    Ok(bundle)
}

// ---------------------------------------------------------------------------
// 媒体清单（docx-r2 读面——容器级：word/media/* 枚举 + .rels 联接 +
// 引擎 Drawing r:embed 引用计数；模型级无渲染语义）
// ---------------------------------------------------------------------------
//
// 引擎读面三个静默怪癖在档（本清单不受影响——自有实现直读 zip 条目）：
// ① 无 `image` 特性时非 PNG/非 EMF 媒体在 generate_image_previews=true
//   默认路径静默丢弃（documents/mod.rs:291-327）；
// ② 悬空 image 关系 read_zip Err 静默跳过（read_docx.rs:404-423）；
// ③ 引擎 `Docx.images` 仅收 document rels 的 IMAGE_TYPE 关系（页眉图片
//   字节不入）。
// zip 8.6 重复条目名：by_name 最后条目胜（read/zip_archive.rs:49-51
// IndexMap::insert 后胜）——本清单与引擎读路径同语义。

/// 图像类型关系词尾（OPC officeDocument 关系命名空间）。
const IMAGE_REL_TYPE_SUFFIX: &str = "/image";

/// 媒体部件目录前缀（Word 约定——清单面固定 `word/media/`）。
const MEDIA_DIR: &str = "word/media/";

/// 单条图像引用关系（rid + 属主部件）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageReference {
    /// 关系 Id（如 `rIdImage1`——属主部件 .rels 内局部）。
    pub rid: String,
    /// 属主部件路径（如 `word/document.xml` / `word/header1.xml`）。
    pub owner_part: String,
}

/// 单个媒体资产（容器级清单项）。
#[derive(Debug, Clone, PartialEq)]
pub struct ImageAsset {
    /// 包内部件名（如 `word/media/rIdImage1.png`）。
    pub part_name: String,
    /// 内容类型（[Content_Types].xml Default/Override 解析；无声明
    /// = None——严格模式整包拒，兼容模式保留 + 警告）。
    pub content_type: Option<String>,
    /// 原始字节（门 2 已验实际 ≤ 声明 ≤ 300MB/条）。
    pub bytes: Vec<u8>,
    /// 引用该媒体的关系集合（去重后按 (owner_part, rid) 排序——
    /// 同媒体可被多部件引用；零引用 = 包内孤儿媒体，保留不警告）。
    pub references: Vec<ImageReference>,
    /// 正文/页眉/页脚 run 级 `a:blip r:embed` 解析到本资产的次数
    /// （引擎读侧 Pic.id 透传 rid——reader/pic.rs:24-28；悬空 r:embed
    /// 严格拒 / 兼容 BrokenReference 警告且不计入）。
    pub body_uses: usize,
}

/// 构建媒体清单（门后字节；条目读取有界——门 2 已验证）。
///
/// 悬空 image 关系（r:id 无条目对侧不存在——目标部件缺失/越根）：
/// 严格拒 / 兼容 BrokenReference 警告 + 资产不增；TargetMode=External
/// 型 image 关系（链接图——无包内部件）不入清单不警告（登记）；
/// 目标存在但非 `word/media/*` 部件：不联接不警告（登记观察）。
fn build_image_inventory(
    za: &mut zip::ZipArchive<std::io::Cursor<&std::vec::Vec<u8>>>,
    bundle: &RelsBundle,
    ct: &ContentTypes,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<ImageAsset>, io::ReadError> {
    let reject = |detail: String| io::ReadError::InvalidPart {
        part: "word/document.xml".to_string(),
        detail,
    };
    let names: Vec<String> = za
        .file_names()
        .filter(|n| n.starts_with(MEDIA_DIR) && !n.ends_with('/'))
        .map(str::to_string)
        .collect();
    let mut assets: Vec<ImageAsset> = Vec::with_capacity(names.len());
    let mut by_part_name: HashMap<String, usize> = HashMap::new();
    for name in &names {
        let mut bytes = Vec::new();
        let ok = za
            .by_name(name)
            .and_then(|mut zf| zf.read_to_end(&mut bytes).map(|_| ()).map_err(Into::into));
        if let Err(e) = ok {
            return Err(io::ReadError::ContainerCorrupt(format!(
                "read media part {name}: {e}"
            )));
        }
        // 内容类型（Override 优先于 Default）；无声明 = 严格拒 /
        // 兼容 MalformedXml 警告 + content_type=None 保留（OPC：每部件
        // 必须有内容类型——包结构违反，兼容容忍可见化）。
        let content_type = ct.content_type_of(name);
        let content_type = match content_type {
            Some(t) => Some(t),
            None => {
                let detail = format!("media part '{name}' has no content type declaration");
                if strict {
                    return Err(reject(detail));
                }
                warnings.push(CompatibilityWarning {
                    part: name.clone(),
                    kind: WarningKind::MalformedXml,
                    detail: format!("compat mode: {detail} (kept with content_type=None)"),
                });
                None
            }
        };
        by_part_name.insert(name.clone(), assets.len());
        assets.push(ImageAsset {
            part_name: name.clone(),
            content_type,
            bytes,
            references: Vec::new(),
            body_uses: 0,
        });
    }

    // 关系联接：全部 .rels 表（document 主表 + per-part 表）中 image 型
    // 关系 → 目标媒体资产。迭代序非确定 → 收集后按 (owner, rid) 排序。
    let mut owner_tables: Vec<(String, &RelationshipTable)> = Vec::new();
    if let Some(t) = bundle.document.as_ref() {
        owner_tables.push(("word/document.xml".to_string(), t));
    }
    for (owner, t) in &bundle.by_part {
        owner_tables.push((owner.clone(), t));
    }
    for (owner, table) in owner_tables {
        let owner_dir = owner.rsplit_once('/').map(|(d, _)| d.to_string());
        for rel in table.iter() {
            if !rel.rel_type.ends_with(IMAGE_REL_TYPE_SUFFIX) {
                continue;
            }
            if rel.target_mode.is_some() {
                // External 型 image 关系（链接图）——无包内部件，不入清单
                // 不警告（与超链接 External 透传同族，登记）。
                continue;
            }
            let Some(owner_dir) = &owner_dir else {
                continue;
            };
            let Some(path) = normalize_part_path(&format!("{owner_dir}/{}", rel.target)) else {
                let detail = format!(
                    "image relationship target '{}' (owner {owner}) escapes package root",
                    rel.target
                );
                if strict {
                    return Err(reject(detail));
                }
                warnings.push(CompatibilityWarning {
                    part: owner.clone(),
                    kind: WarningKind::BrokenReference,
                    detail,
                });
                continue;
            };
            let Some(&idx) = by_part_name.get(&path) else {
                // 目标不在包内（悬空）或非 word/media/* 部件（在包内
                // 但非清单面——登记观察，不警告不联接）。
                if bundle.entries.contains(&path) {
                    continue;
                }
                let detail = format!(
                    "image relationship target part '{path}' (owner {owner}) missing from package"
                );
                if strict {
                    return Err(reject(detail));
                }
                warnings.push(CompatibilityWarning {
                    part: owner.clone(),
                    kind: WarningKind::BrokenReference,
                    detail,
                });
                continue;
            };
            assets[idx].references.push(ImageReference {
                rid: rel.id.clone(),
                owner_part: owner.clone(),
            });
        }
    }
    for a in &mut assets {
        a.references.sort();
        a.references.dedup();
    }
    Ok(assets)
}

/// 引擎模型 run 级 `a:blip r:embed` 引用收集（正文 + 六槽位页眉/
/// 页脚；悬空 r:embed 严格拒 / 兼容 BrokenReference 警告且不计入）。
fn count_image_uses(
    docx: &docx_rs::Docx,
    bundle: &RelsBundle,
    media_parts: &HashMap<String, usize>,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<HashMap<String, usize>, io::ReadError> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut sink: Vec<CompatibilityWarning> = Vec::new(); // 丢弃面：槽位引用悬空警告已在提取层记

    // 单个 rid 经属主表解析 → 目标媒体部件（悬空/越根/External：
    // 严格拒 / 兼容警告跳过；非媒体目标：跳过）。
    let mut resolve = |rid: &str,
                       table: Option<&RelationshipTable>,
                       owner: &str,
                       owner_dir: &str,
                       counts: &mut HashMap<String, usize>|
     -> Result<(), io::ReadError> {
        let Some(table) = table else {
            return Ok(()); // 无表可验（降级警告已在解析层记）——静默
        };
        let Some(rel) = table.get(rid) else {
            let detail = format!("image r:embed '{rid}' (owner {owner}) has no relationship entry");
            if strict {
                return Err(io::ReadError::InvalidPart {
                    part: owner.to_string(),
                    detail,
                });
            }
            warnings.push(CompatibilityWarning {
                part: owner.to_string(),
                kind: WarningKind::BrokenReference,
                detail,
            });
            return Ok(());
        };
        if rel.target_mode.is_some() {
            return Ok(()); // External 链接图（登记）
        }
        let Some(path) = normalize_part_path(&format!("{owner_dir}/{}", rel.target)) else {
            let detail = format!(
                "image r:embed target '{}' (owner {owner}) escapes package root",
                rel.target
            );
            if strict {
                return Err(io::ReadError::InvalidPart {
                    part: owner.to_string(),
                    detail,
                });
            }
            warnings.push(CompatibilityWarning {
                part: owner.to_string(),
                kind: WarningKind::BrokenReference,
                detail,
            });
            return Ok(());
        };
        if media_parts.contains_key(&path) {
            *counts.entry(path).or_insert(0) += 1;
        }
        // 目标在包内但非 word/media/* 部件：跳过（登记观察）。
        Ok(())
    };

    // 正文（word/document.xml）。
    let doc_table = bundle.document.as_ref();
    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => {
                collect_paragraph_image_rids(
                    p,
                    doc_table,
                    "word/document.xml",
                    "word",
                    &mut resolve,
                    &mut counts,
                )?;
            }
            docx_rs::DocumentChild::Table(t) => {
                collect_table_image_rids(
                    t,
                    doc_table,
                    "word/document.xml",
                    "word",
                    &mut resolve,
                    &mut counts,
                )?;
            }
            _ => {}
        }
    }

    // 六槽位页眉/页脚（各自 rels 联接——槽位路径经 slot_part_path
    // 复算，悬空槽位静默跳过：提取层已警告）。
    let sp = &docx.document.section_property;
    enum SlotContent<'a> {
        Header(&'a docx_rs::Header),
        Footer(&'a docx_rs::Footer),
    }
    /// 页眉/页脚槽位对（default/first/even 三组——rid + 内容）。
    type SlotPair<'a> = (
        Option<&'a (String, docx_rs::Header)>,
        Option<&'a (String, docx_rs::Footer)>,
    );
    let slots: [SlotPair<'_>; 3] = [
        (sp.header.as_ref(), sp.footer.as_ref()),
        (sp.first_header.as_ref(), sp.first_footer.as_ref()),
        (sp.even_header.as_ref(), sp.even_footer.as_ref()),
    ];
    for (header, footer) in slots {
        for (rid, content) in [
            header.map(|(rid, h)| (rid, SlotContent::Header(h))),
            footer.map(|(rid, f)| (rid, SlotContent::Footer(f))),
        ]
        .into_iter()
        .flatten()
        {
            sink.clear();
            let Some(part) = slot_part_path(rid, bundle, false, &mut sink)? else {
                continue;
            };
            let table = bundle.by_part.get(&part);
            let Some(owner_dir) = part.rsplit_once('/').map(|(d, _)| d) else {
                continue;
            };
            match content {
                SlotContent::Header(h) => {
                    for child in &h.children {
                        match child {
                            docx_rs::HeaderChild::Paragraph(p) => {
                                collect_paragraph_image_rids(
                                    p,
                                    table,
                                    &part,
                                    owner_dir,
                                    &mut resolve,
                                    &mut counts,
                                )?;
                            }
                            docx_rs::HeaderChild::Table(t) => {
                                collect_table_image_rids(
                                    t,
                                    table,
                                    &part,
                                    owner_dir,
                                    &mut resolve,
                                    &mut counts,
                                )?;
                            }
                            // StructuredDataTag 等——面外。
                            _ => {}
                        }
                    }
                }
                SlotContent::Footer(f) => {
                    for child in &f.children {
                        match child {
                            docx_rs::FooterChild::Paragraph(p) => {
                                collect_paragraph_image_rids(
                                    p,
                                    table,
                                    &part,
                                    owner_dir,
                                    &mut resolve,
                                    &mut counts,
                                )?;
                            }
                            docx_rs::FooterChild::Table(t) => {
                                collect_table_image_rids(
                                    t,
                                    table,
                                    &part,
                                    owner_dir,
                                    &mut resolve,
                                    &mut counts,
                                )?;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(counts)
}

/// 段落内 run 级 Drawing r:embed 收集（含超链接内 run；表格递归）。
#[allow(clippy::too_many_arguments)]
fn collect_paragraph_image_rids(
    p: &docx_rs::Paragraph,
    table: Option<&RelationshipTable>,
    owner: &str,
    owner_dir: &str,
    resolve: &mut impl FnMut(
        &str,
        Option<&RelationshipTable>,
        &str,
        &str,
        &mut HashMap<String, usize>,
    ) -> Result<(), io::ReadError>,
    counts: &mut HashMap<String, usize>,
) -> Result<(), io::ReadError> {
    for child in &p.children {
        match child {
            docx_rs::ParagraphChild::Run(r) => {
                collect_run_image_rids(r, table, owner, owner_dir, resolve, counts)?;
            }
            docx_rs::ParagraphChild::Hyperlink(h) => {
                for c in &h.children {
                    if let docx_rs::ParagraphChild::Run(r) = c {
                        collect_run_image_rids(r, table, owner, owner_dir, resolve, counts)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// 表内段落级 Drawing r:embed 收集（单元格段落递归）。
#[allow(clippy::too_many_arguments)]
fn collect_table_image_rids(
    t: &docx_rs::Table,
    table: Option<&RelationshipTable>,
    owner: &str,
    owner_dir: &str,
    resolve: &mut impl FnMut(
        &str,
        Option<&RelationshipTable>,
        &str,
        &str,
        &mut HashMap<String, usize>,
    ) -> Result<(), io::ReadError>,
    counts: &mut HashMap<String, usize>,
) -> Result<(), io::ReadError> {
    for row in &t.rows {
        let docx_rs::TableChild::TableRow(r) = row;
        for cell in &r.cells {
            let docx_rs::TableRowChild::TableCell(c) = cell;
            for content in &c.children {
                if let docx_rs::TableCellContent::Paragraph(p) = content {
                    collect_paragraph_image_rids(p, table, owner, owner_dir, resolve, counts)?;
                }
            }
        }
    }
    Ok(())
}

/// run 内 Drawing r:embed 收集（引擎读侧 Pic.id 透传 rid）。
#[allow(clippy::too_many_arguments)]
fn collect_run_image_rids(
    r: &docx_rs::Run,
    table: Option<&RelationshipTable>,
    owner: &str,
    owner_dir: &str,
    resolve: &mut impl FnMut(
        &str,
        Option<&RelationshipTable>,
        &str,
        &str,
        &mut HashMap<String, usize>,
    ) -> Result<(), io::ReadError>,
    counts: &mut HashMap<String, usize>,
) -> Result<(), io::ReadError> {
    for child in &r.children {
        if let docx_rs::RunChild::Drawing(d) = child {
            if let Some(docx_rs::DrawingData::Pic(pic)) = &d.data {
                if !pic.id.is_empty() {
                    resolve(&pic.id, table, owner, owner_dir, counts)?;
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 读侧模型（自有类型——docx-rs 类型零泄漏）
// ---------------------------------------------------------------------------

/// 超链接目标（docx-r1：OPC 关系层联接产出）。
#[derive(Debug, Clone, PartialEq)]
pub enum HyperlinkTarget {
    /// 外部目标（关系 TargetMode="External"——URL 原样透传，**永不
    /// 解引用**；消费方自证安全[mdc-office 消毒链责任边界]）。
    External { target: String },
    /// 包内目标（关系无 TargetMode——相对路径原样透传，未解析为包
    /// 绝对路径；罕见面：Word 常规外链均带 External）。
    Internal { target: String },
    /// 文档内锚点（`w:anchor`——无关系面）。
    Anchor { anchor: String },
    /// r:id 无对应关系条目（**仅兼容模式**：严格模式先拒；链接文本
    /// 保留，目标不可用——BrokenReference 警告随文档返回）。
    Unresolved { rid: String },
}

/// 单个文本运行（模型级四面：文本 + 加粗 + 斜体 + 超链接目标）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunData {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// run 所在超链接的解析结果（None = 普通文本 run；同一超链接内
    /// 的各 run 共享同一目标）。
    pub hyperlink: Option<HyperlinkTarget>,
}

/// 段落：样式 ID 透传 + run 序。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphData {
    /// 样式 ID（如 `Heading1`/`Normal`）——语义判定归消费方。
    pub style: Option<String>,
    pub runs: Vec<RunData>,
}

impl ParagraphData {
    /// 段落拼接文本（run 文本直连——不加空格，保真 run 边界）。
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// 文档块（有序）。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(ParagraphData),
    Table(TableData),
}

/// 文本表格：行 × 单元格拼接文本。
#[derive(Debug, Clone, PartialEq)]
pub struct TableData {
    pub rows: Vec<Vec<String>>,
}

/// 页眉/页脚槽位变体（透传 `headerReference/@w:type`——first/even
/// 槽位**激活**需 titlePg/evenAndOddHeaders 节设置，语义解释面外）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderFooterVariant {
    Default,
    First,
    Even,
}

/// 页眉/页脚提取（六槽位中实际挂接的）：变体 + 块序（与文档体
/// 同构的段落/表提取；页眉部件自身 rels 联接超链接目标）。
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFooterData {
    pub variant: HeaderFooterVariant,
    pub blocks: Vec<Block>,
}

/// 整档提取结果：块序 + 页眉页脚 + 媒体清单 + 结构化警告（容器门面，
/// 与 XLSX 一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct DocxDocument {
    pub blocks: Vec<Block>,
    /// 页眉（default/first/even 中实际挂接的——引用悬空时严格拒 /
    /// 兼容警告 + 槽位跳过）。
    pub headers: Vec<HeaderFooterData>,
    /// 页脚（同上）。
    pub footers: Vec<HeaderFooterData>,
    /// 媒体清单（docx-r2：`word/media/*` 容器级枚举——字节/内容类型/
    /// 引用关系/run 级引用计数；块序内图片位置不入块模型，登记）。
    pub images: Vec<ImageAsset>,
    pub warnings: Vec<CompatibilityWarning>,
}

// ---------------------------------------------------------------------------
// 读 API（容器三门 → 自有关系层 → docx-rs 模型解析 → 联接提取）
// ---------------------------------------------------------------------------

/// 读 DOCX 文件（严格模式默认——行为见模块文档）。
pub fn read_docx(path: &str) -> Result<DocxDocument, DocxError> {
    read_docx_with_options(path, &ReadOptions::default())
}

/// 读 DOCX 文件（带选项）。`opts.compat = true` 时畸形部件降级为
/// 结构化警告（门 1/门 2/安全上限恒不放松——语义同 [`io::read_book_with_options`]）。
pub fn read_docx_with_options(path: &str, opts: &ReadOptions) -> Result<DocxDocument, DocxError> {
    let bytes = std::fs::read(path).map_err(|e| DocxError::Io(format!("read {path}: {e}")))?;
    read_bytes_gated(bytes, opts)
}

/// 读 DOCX 字节（容器三门同样适用——字节入口不绕过输入防线）。
pub fn read_docx_from_bytes(bytes: &[u8]) -> Result<DocxDocument, DocxError> {
    read_docx_from_bytes_with_options(bytes, &ReadOptions::default())
}

/// 读 DOCX 字节（带选项）。
pub fn read_docx_from_bytes_with_options(
    bytes: &[u8],
    opts: &ReadOptions,
) -> Result<DocxDocument, DocxError> {
    read_bytes_gated(bytes.to_vec(), opts)
}

/// 门结构：门 1（prescan）→ 门 2+3 融合趟（尺寸 + 部件良构——
/// 复用 XLSX 读路径同一实现）→ **自有 OPC 关系层**（先于引擎——
/// 上限两模式恒拒；词汇表/schema 严格拒兼容降级）→ [Content_Types].xml
/// 解析 + 媒体清单 → docx-rs 模型解析 → 关系联接提取 + run 级图片
/// 引用计数。
fn read_bytes_gated(bytes: Vec<u8>, opts: &ReadOptions) -> Result<DocxDocument, DocxError> {
    let strict = !opts.compat;
    let cursor = std::io::Cursor::new(&bytes);
    let mut za = zip::ZipArchive::new(cursor)
        .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
    io::prescan(&mut za)?;
    let mut warnings = Vec::new();
    // 门 2+3 融合趟（io 私有实现经公开管道函数复用——见 io::read_book）。
    io::verify_gates(&mut za, strict, &mut warnings)?;
    drop(za);

    // OPC 关系层 + 内容类型 + 媒体清单（同一门后字节——零门绕过）。
    let (bundle, images) = {
        let cursor = std::io::Cursor::new(&bytes);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
        let bundle = build_rels_bundle(&mut za, strict, &mut warnings)?;
        // [Content_Types].xml：部件缺失 = 严格拒 / 兼容降级空表（引擎
        // 随后将拒——read_docx.rs:101 硬依赖，两模式最终同拒，诚实警告）。
        let content_types = match za.by_name("[Content_Types].xml") {
            Ok(mut zf) => {
                let mut data = Vec::new();
                let r = zf
                    .read_to_end(&mut data)
                    .map_err(|e| io::ReadError::ContainerCorrupt(format!("{e}")))?;
                let _ = r;
                parse_content_types(&data, strict, &mut warnings)?
            }
            Err(e) => {
                let detail = format!("[Content_Types].xml part missing ({e})");
                if strict {
                    return Err(DocxError::Container(io::ReadError::InvalidPart {
                        part: "[Content_Types].xml".to_string(),
                        detail,
                    }));
                }
                warnings.push(CompatibilityWarning {
                    part: "[Content_Types].xml".to_string(),
                    kind: WarningKind::MalformedXml,
                    detail: format!("compat mode: {detail} (inventory empty; engine will reject)"),
                });
                ContentTypes::default()
            }
        };
        let images =
            build_image_inventory(&mut za, &bundle, &content_types, strict, &mut warnings)?;
        (bundle, images)
    };

    let docx = docx_rs::read_docx(&bytes).map_err(|e| DocxError::Parse(format!("{e}")))?;
    let blocks = extract_blocks(&docx, &bundle, strict, &mut warnings)?;
    let (headers, footers) = extract_headers_footers(&docx, &bundle, strict, &mut warnings)?;
    // run 级图片引用计数（悬空 r:embed 严格拒 / 兼容警告）。
    let media_parts: HashMap<String, usize> = images
        .iter()
        .enumerate()
        .map(|(i, a)| (a.part_name.clone(), i))
        .collect();
    let mut images = images;
    let uses = count_image_uses(&docx, &bundle, &media_parts, strict, &mut warnings)?;
    for a in &mut images {
        a.body_uses = uses.get(&a.part_name).copied().unwrap_or(0);
    }
    Ok(DocxDocument {
        blocks,
        headers,
        footers,
        images,
        warnings,
    })
}

/// r:id → 关系表条目 + 模式分派（External/Internal；无条目 = 严格拒 /
/// 兼容 Unresolved + BrokenReference 警告）。
fn resolve_link(
    rid: &str,
    table: Option<&RelationshipTable>,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<HyperlinkTarget, io::ReadError> {
    match table.and_then(|t| t.get(rid)) {
        Some(rel) => Ok(if rel.target_mode.as_deref() == Some("External") {
            HyperlinkTarget::External {
                target: rel.target.clone(),
            }
        } else {
            HyperlinkTarget::Internal {
                target: rel.target.clone(),
            }
        }),
        None => {
            let detail = if table.is_none() {
                format!("hyperlink r:id '{rid}' unresolvable (document rels degraded/unavailable)")
            } else {
                format!("hyperlink r:id '{rid}' has no relationship entry (dangling reference)")
            };
            if strict {
                Err(io::ReadError::InvalidPart {
                    part: part.to_string(),
                    detail,
                })
            } else {
                warnings.push(CompatibilityWarning {
                    part: part.to_string(),
                    kind: WarningKind::BrokenReference,
                    detail,
                });
                Ok(HyperlinkTarget::Unresolved {
                    rid: rid.to_string(),
                })
            }
        }
    }
}

/// 超链接元素 → 目标（w:anchor 优先[引擎行为镜像]；r:id 联接）。
fn resolve_hyperlink(
    h: &docx_rs::Hyperlink,
    table: Option<&RelationshipTable>,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<HyperlinkTarget, io::ReadError> {
    match &h.link {
        docx_rs::HyperlinkData::Anchor { anchor } => Ok(HyperlinkTarget::Anchor {
            anchor: anchor.clone(),
        }),
        // 引擎对无 r:id 无 w:anchor 的畸形超链接给空 rid
        //（reader/hyperlink.rs:23 unwrap_or_default）——走同一条
        // 悬空引用拒绝/降级路径。
        docx_rs::HyperlinkData::External { rid, .. } => {
            resolve_link(rid, table, part, strict, warnings)
        }
    }
}

/// docx-rs 模型 → 自有模型提取（读面跳过项见模块文档登记表）。
fn extract_blocks(
    docx: &docx_rs::Docx,
    bundle: &RelsBundle,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<Block>, io::ReadError> {
    let table = bundle.document.as_ref();
    let mut blocks = Vec::new();
    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => blocks.push(Block::Paragraph(
                extract_paragraph(p, table, "word/document.xml", strict, warnings)?,
            )),
            docx_rs::DocumentChild::Table(t) => blocks.push(Block::Table(extract_table(
                t,
                table,
                "word/document.xml",
                strict,
                warnings,
            )?)),
            // 书签/批注/内容控件/TOC/节——模型级切片面外，跳过（登记）。
            _ => {}
        }
    }
    Ok(blocks)
}

/// 段落提取：样式透传 + run 序（Run + Hyperlink 内 run[目标联接]）。
fn extract_paragraph(
    p: &docx_rs::Paragraph,
    table: Option<&RelationshipTable>,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<ParagraphData, io::ReadError> {
    let style = p.property.style.as_ref().map(|s| s.val.clone());
    let mut runs = Vec::new();
    for child in &p.children {
        match child {
            docx_rs::ParagraphChild::Run(r) => runs.push(extract_run(r)),
            // 超链接：内文本保留（run 边界保持）+ 目标联接（docx-r1）。
            docx_rs::ParagraphChild::Hyperlink(h) => {
                let target = resolve_hyperlink(h, table, part, strict, warnings)?;
                for c in &h.children {
                    if let docx_rs::ParagraphChild::Run(r) = c {
                        let mut rd = extract_run(r);
                        rd.hyperlink = Some(target.clone());
                        runs.push(rd);
                    }
                }
            }
            // 修订追踪/书签/批注/域/内容控件——面外跳过（登记）。
            _ => {}
        }
    }
    Ok(ParagraphData { style, runs })
}

/// run 提取：文本三源（Text/Tab/Break+CarriageReturn）+ 加粗斜体
///（超链接目标由调用面挂接——extract_paragraph 的 Hyperlink 分支）。
fn extract_run(r: &docx_rs::Run) -> RunData {
    let mut text = String::new();
    for child in &r.children {
        match child {
            docx_rs::RunChild::Text(t) => text.push_str(&t.text),
            docx_rs::RunChild::Tab(_) => text.push('\t'),
            docx_rs::RunChild::Break(_) => text.push('\n'),
            docx_rs::RunChild::CarriageReturn(_) => text.push('\r'),
            // Sym/DeleteText/Drawing/Shape/域/批注锚——面外跳过（登记）。
            _ => {}
        }
    }
    RunData {
        text,
        bold: r.run_property.bold.is_some(),
        italic: r.run_property.italic.is_some(),
        hyperlink: None,
    }
}

/// 表提取：行 × 单元格拼接文本（单元格内多段落 `\n` 连接；嵌套表/
/// 内容控件跳过——登记）。单元格内超链接目标丢弃（TableData 扁平
/// 文本面——登记），但悬空引用仍按严格/兼容纪律处置（包完整性）。
fn extract_table(
    t: &docx_rs::Table,
    table: Option<&RelationshipTable>,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<TableData, io::ReadError> {
    // 注：docx-rs 0.4.22 模型中 TableChild/TableRowChild 均为单变体
    //（TableRow/TableCell）——直接解构；模型升级增变体时此函数
    // 编译期报非穷尽，而面外项处理策略不变（跳过+登记）。
    let mut rows = Vec::new();
    for row in &t.rows {
        let docx_rs::TableChild::TableRow(r) = row;
        let mut cells = Vec::new();
        for cell in &r.cells {
            let docx_rs::TableRowChild::TableCell(c) = cell;
            let mut parts = Vec::new();
            for content in &c.children {
                if let docx_rs::TableCellContent::Paragraph(p) = content {
                    let pd = extract_paragraph(p, table, part, strict, warnings)?;
                    parts.push(pd.text());
                }
            }
            cells.push(parts.join("\n"));
        }
        rows.push(cells);
    }
    Ok(TableData { rows })
}

/// 页眉/页脚槽位引用校验：r:id 在文档关系表 + 目标部件在包内 +
/// 路径不越出包根。返回属主部件路径（内容联接用）。悬空 = 严格拒 /
/// 兼容警告 + None（槽位跳过）；文档关系表降级/缺席 = 无表可验
/// （降级警告已在解析层记），静默 None（内容照常提取，链接 Unresolved）。
fn slot_part_path(
    rid: &str,
    bundle: &RelsBundle,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Option<String>, io::ReadError> {
    let dangling = |detail: String| io::ReadError::InvalidPart {
        part: "word/document.xml".to_string(),
        detail,
    };
    let Some(table) = bundle.document.as_ref() else {
        return Ok(None);
    };
    let Some(rel) = table.get(rid) else {
        let detail = format!("header/footer reference r:id '{rid}' has no relationship entry");
        return if strict {
            Err(dangling(detail))
        } else {
            warnings.push(CompatibilityWarning {
                part: "word/document.xml".to_string(),
                kind: WarningKind::BrokenReference,
                detail,
            });
            Ok(None)
        };
    };
    // 目标 = 相对于 word/ 的包内路径（词法归一化；越出包根 = 敌意）。
    let Some(path) = normalize_part_path(&format!("word/{}", rel.target)) else {
        let detail = format!(
            "header/footer relationship target '{}' escapes package root",
            rel.target
        );
        return if strict {
            Err(dangling(detail))
        } else {
            warnings.push(CompatibilityWarning {
                part: "word/document.xml".to_string(),
                kind: WarningKind::BrokenReference,
                detail,
            });
            Ok(None)
        };
    };
    if !bundle.entries.contains(&path) {
        let detail =
            format!("header/footer relationship target part '{path}' missing from package");
        return if strict {
            Err(dangling(detail))
        } else {
            warnings.push(CompatibilityWarning {
                part: "word/document.xml".to_string(),
                kind: WarningKind::BrokenReference,
                detail,
            });
            Ok(None)
        };
    }
    Ok(Some(path))
}

/// 页眉/页脚提取（六槽位：default/first/even × header/footer——
/// 仅实际挂接的；引用在而内容挂接失败 = 严格拒 / 兼容警告 + 跳过）。
/// 页眉/页脚部件自身的 rels 联接其内部超链接目标（CAPABILITY_NOTES §2）。
fn extract_headers_footers(
    docx: &docx_rs::Docx,
    bundle: &RelsBundle,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<(Vec<HeaderFooterData>, Vec<HeaderFooterData>), io::ReadError> {
    let sp = &docx.document.section_property;
    let mut headers = Vec::new();
    let mut footers = Vec::new();

    // 页眉三槽位（default/first/even）。`let-else` 解构：内容槽位
    //（引擎挂接的）与引用槽位（sectPr 声明的）分离——引擎在引用在
    // 而挂接失败时静默跳过（CAPABILITY_NOTES §2），本层改为严格拒 /
    // 兼容警告。无引用 = 无槽位（不产生空槽）。
    for (variant, reference, content) in [
        (
            HeaderFooterVariant::Default,
            &sp.header_reference,
            &sp.header,
        ),
        (
            HeaderFooterVariant::First,
            &sp.first_header_reference,
            &sp.first_header,
        ),
        (
            HeaderFooterVariant::Even,
            &sp.even_header_reference,
            &sp.even_header,
        ),
    ] {
        let Some((rid, header)) = content else {
            if let Some(r) = reference {
                slot_part_path(&r.id, bundle, strict, warnings)?;
            }
            continue;
        };
        let part = slot_part_path(rid, bundle, strict, warnings)?;
        let table = part.as_deref().and_then(|p| bundle.by_part.get(p));
        let part_name = part.as_deref().unwrap_or("");
        let mut blocks = Vec::new();
        for child in &header.children {
            match child {
                docx_rs::HeaderChild::Paragraph(p) => blocks.push(Block::Paragraph(
                    extract_paragraph(p, table, part_name, strict, warnings)?,
                )),
                docx_rs::HeaderChild::Table(t) => blocks.push(Block::Table(extract_table(
                    t, table, part_name, strict, warnings,
                )?)),
                // StructuredDataTag——面外跳过（登记）。
                _ => {}
            }
        }
        headers.push(HeaderFooterData { variant, blocks });
    }

    // 页脚三槽位。
    for (variant, reference, content) in [
        (
            HeaderFooterVariant::Default,
            &sp.footer_reference,
            &sp.footer,
        ),
        (
            HeaderFooterVariant::First,
            &sp.first_footer_reference,
            &sp.first_footer,
        ),
        (
            HeaderFooterVariant::Even,
            &sp.even_footer_reference,
            &sp.even_footer,
        ),
    ] {
        let Some((rid, footer)) = content else {
            if let Some(r) = reference {
                slot_part_path(&r.id, bundle, strict, warnings)?;
            }
            continue;
        };
        let part = slot_part_path(rid, bundle, strict, warnings)?;
        let table = part.as_deref().and_then(|p| bundle.by_part.get(p));
        let part_name = part.as_deref().unwrap_or("");
        let mut blocks = Vec::new();
        for child in &footer.children {
            match child {
                docx_rs::FooterChild::Paragraph(p) => blocks.push(Block::Paragraph(
                    extract_paragraph(p, table, part_name, strict, warnings)?,
                )),
                docx_rs::FooterChild::Table(t) => blocks.push(Block::Table(extract_table(
                    t, table, part_name, strict, warnings,
                )?)),
                // StructuredDataTag——面外跳过（登记）。
                _ => {}
            }
        }
        footers.push(HeaderFooterData { variant, blocks });
    }
    Ok((headers, footers))
}

// ---------------------------------------------------------------------------
// 写 API（新建语义——DocxWriter）
// ---------------------------------------------------------------------------

/// 简单域代码（docx-r2 写面）：PAGE / NUMPAGES 两面（引擎元素族
/// 其余域 = TOC[文档级]/PAGEREF[书签面]/HYPERLINK[`todo!()` 崩溃面
/// instr_text.rs:32]——登记不暴露）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldSpec {
    /// 当前页码域（`<w:instrText>PAGE</w:instrText>`）。
    Page,
    /// 总页数域（`<w:instrText>NUMPAGES</w:instrText>`）。
    NumPages,
}

/// 富文本 run 规格（写侧输入模型）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunSpec {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// 外部超链接目标（None = 普通 run；写侧经引擎关系面发射
    /// TargetMode="External"——引擎写面 "Now support external only"
    /// [documents/mod.rs:1209]，锚点写面不暴露）。
    pub link: Option<String>,
    /// 脚注内容（docx-r2；Some = 本 run 为脚注引用 marker——`text`/
    /// `bold`/`italic`/`link` 域值**忽略**：引擎 `add_footnote_reference`
    /// 重置 run_property 为 FootnoteReference 字符样式
    /// [documents/elements/run.rs:363-369]，marker 无用户样式面；
    /// 脚注内容 = 单段纯文本，发射进 word/footnotes.xml）。读面引擎
    /// 受限（CAPABILITY_NOTES §5）——XML 级断言见 tests/docx_roundtrip.rs。
    pub footnote: Option<String>,
    /// 简单域（docx-r2；Some = 本 run 展开为 fldChar begin/instrText/
    /// fldChar end 三连——无缓存结果文本：域值是渲染期计算面，
    /// 读回文本不含域值）。
    pub field: Option<FieldSpec>,
}

impl RunSpec {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            link: None,
            footnote: None,
            field: None,
        }
    }

    pub fn styled(text: impl Into<String>, bold: bool, italic: bool) -> Self {
        Self {
            text: text.into(),
            bold,
            italic,
            link: None,
            footnote: None,
            field: None,
        }
    }

    /// 外部链接 run（`text` = 链接显示文本，`url` = 目标 URL——
    /// 读回为 [`HyperlinkTarget::External`]）。
    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            link: Some(url.into()),
            footnote: None,
            field: None,
        }
    }

    /// 脚注引用 marker run（`note` = 脚注内容单段纯文本；样式面忽略
    /// ——见字段文档）。
    pub fn footnote(note: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            bold: false,
            italic: false,
            link: None,
            footnote: Some(note.into()),
            field: None,
        }
    }

    /// 当前页码域 run（`PAGE`）。
    pub fn field_page() -> Self {
        Self {
            text: String::new(),
            bold: false,
            italic: false,
            link: None,
            footnote: None,
            field: Some(FieldSpec::Page),
        }
    }

    /// 总页数域 run（`NUMPAGES`）。
    pub fn field_num_pages() -> Self {
        Self {
            text: String::new(),
            bold: false,
            italic: false,
            link: None,
            footnote: None,
            field: Some(FieldSpec::NumPages),
        }
    }
}

/// DOCX 新建文档写器（**仅新建语义**——不存在已有文档编辑往返，
/// 与 XLSX [`crate::write::WorkbookWriter`] 边界同构）。
///
/// 预置 Heading1-9 段落样式（outline 级——模型级结构语义，无字体/
/// 字号面）——标题在 Word 大纲视图/目录中可用；正文段落样式
/// `Normal`（引擎默认）。
#[derive(Debug, Default)]
pub struct DocxWriter {
    docx: docx_rs::Docx,
}

impl DocxWriter {
    pub fn new() -> Self {
        let mut docx = docx_rs::Docx::new();
        // Heading1-9 样式定义（outline 级；based_on Normal；qFormat
        // ——样式面板可见）。无字体面 = 模型级边界（渲染语义面外）。
        for level in 1u8..=9 {
            let mut style =
                docx_rs::Style::new(format!("Heading{level}"), docx_rs::StyleType::Paragraph)
                    .name(format!("heading {level}"))
                    .based_on("Normal")
                    .next("Normal")
                    .q_format(true);
            style.paragraph_property = style
                .paragraph_property
                .clone()
                .outline_lvl((level - 1) as usize);
            docx = docx.add_style(style);
        }
        Self { docx }
    }

    /// 标题段（level 1-9；样式引用 + outline 级双通道——样式被剥除
    /// 时大纲结构仍可恢复）。
    pub fn heading(&mut self, level: u8, text: &str) -> Result<(), DocxError> {
        if !(1..=9).contains(&level) {
            return Err(DocxError::Parse(format!(
                "heading level must be 1-9, got {level}"
            )));
        }
        let mut p = docx_rs::Paragraph::new()
            .style(&format!("Heading{level}"))
            .add_run(docx_rs::Run::new().add_text(text));
        p.property = p.property.clone().outline_lvl((level - 1) as usize);
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        Ok(())
    }

    /// 单文本段落（便捷面）。
    pub fn paragraph_text(&mut self, text: &str) {
        self.paragraph(&[RunSpec::text(text)]);
    }

    /// 富文本段落（run 级加粗/斜体 + 外部超链接）。
    pub fn paragraph(&mut self, runs: &[RunSpec]) {
        let p = specs_to_paragraph(runs);
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
    }

    /// 默认页眉（单段 run 序——run 级加粗/斜体/链接/脚注/域）。
    /// **default 槽位，重复调用 = 原位替换**（引擎公开 `Docx::header` 按调用
    /// 次数递增 rels 计数——重复调用会发射 rIdHeader{n+1} 关系而无
    /// 对应部件[自产悬空引用，documents/mod.rs:576-589 计数语义]；
    /// 本包装层改为原位替换保持包自洽——同 XLSX 流式纪律的包装层
    /// 防御先例）。
    pub fn header(&mut self, runs: &[RunSpec]) {
        let h = docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.header.is_some() {
            replace_slot_header(&mut self.docx, SlotKind::Default, h);
        } else {
            self.docx = std::mem::take(&mut self.docx).header(h);
        }
    }

    /// 首页页眉（docx-r2；引擎自动置 titlePg 激活——
    /// section_property.rs:95；重复调用 = 原位替换保持原槽 rid）。
    pub fn first_header(&mut self, runs: &[RunSpec]) {
        let h = docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.first_header.is_some() {
            replace_slot_header(&mut self.docx, SlotKind::First, h);
        } else {
            self.docx = std::mem::take(&mut self.docx).first_header(h);
        }
    }

    /// 偶数页页眉（docx-r2；引擎自动置 settings evenAndOddHeaders
    /// 激活——documents/mod.rs:613-616；重复调用 = 原位替换保持原槽
    /// rid）。
    pub fn even_header(&mut self, runs: &[RunSpec]) {
        let h = docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.even_header.is_some() {
            replace_slot_header(&mut self.docx, SlotKind::Even, h);
        } else {
            self.docx = std::mem::take(&mut self.docx).even_header(h);
        }
    }

    /// 默认页脚（单段 run 序——同 [`Self::header`]，重复调用 = 原位替换）。
    pub fn footer(&mut self, runs: &[RunSpec]) {
        let f = docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.footer.is_some() {
            replace_slot_footer(&mut self.docx, SlotKind::Default, f);
        } else {
            self.docx = std::mem::take(&mut self.docx).footer(f);
        }
    }

    /// 首页页脚（docx-r2；同 [`Self::first_header`]——titlePg 激活 +
    /// 原位替换）。
    pub fn first_footer(&mut self, runs: &[RunSpec]) {
        let f = docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.first_footer.is_some() {
            replace_slot_footer(&mut self.docx, SlotKind::First, f);
        } else {
            self.docx = std::mem::take(&mut self.docx).first_footer(f);
        }
    }

    /// 偶数页页脚（docx-r2；同 [`Self::even_header`]——evenAndOddHeaders
    /// 激活 + 原位替换）。
    pub fn even_footer(&mut self, runs: &[RunSpec]) {
        let f = docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.even_footer.is_some() {
            replace_slot_footer(&mut self.docx, SlotKind::Even, f);
        } else {
            self.docx = std::mem::take(&mut self.docx).even_footer(f);
        }
    }

    /// 内联 PNG 图片段（docx-r2；单图单段，包级媒体部件 + 正文关系
    /// + `<a:blip r:embed>` 引用——读回经 [`DocxDocument::images`] 清单全环）。
    ///
    /// **PNG-only 面**：引擎媒体部件恒 `*.png` 命名
    /// （image_collector.rs:137 `media/{id}.png`；zipper/mod.rs:118 注释
    /// 「For now only png supported」）——非 PNG 签名字节 typed 拒；
    /// 自动转码/尺寸推断面 = `Pic::new` 需 `image` 特性（+23 crate
    /// 依赖树——登记不启用），本面要求显式像素尺寸（EMU 换算引擎
    /// 内建 `from_px` = px × 9525 @96dpi）。
    pub fn image(&mut self, png: &[u8], width_px: u32, height_px: u32) -> Result<(), DocxError> {
        const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        if png.len() < 8 || png[..8] != PNG_SIGNATURE {
            return Err(DocxError::Parse(format!(
                "image must be PNG (signature mismatch, {} bytes)",
                png.len()
            )));
        }
        if width_px == 0 || height_px == 0 {
            return Err(DocxError::Parse(format!(
                "image dimensions must be positive, got {width_px}x{height_px}"
            )));
        }
        let pic = docx_rs::Pic::new_with_dimensions(png.to_vec(), width_px, height_px);
        let p = docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_image(pic));
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        Ok(())
    }

    /// 文本表格（行 × 单元格文本；首行不特殊化——表头样式属渲染面）。
    pub fn table(&mut self, rows: &[Vec<String>]) {
        let table_rows = rows
            .iter()
            .map(|row| {
                let cells = row
                    .iter()
                    .map(|cell| {
                        docx_rs::TableCell::new().add_paragraph(
                            docx_rs::Paragraph::new()
                                .add_run(docx_rs::Run::new().add_text(cell.as_str())),
                        )
                    })
                    .collect();
                docx_rs::TableRow::new(cells)
            })
            .collect();
        let table = docx_rs::Table::new(table_rows);
        self.docx = std::mem::take(&mut self.docx).add_table(table);
    }

    /// 保存到文件路径。
    pub fn save_to_path(&mut self, path: &str) -> Result<(), DocxError> {
        let mut buf = self.save_to_vec()?;
        std::fs::write(path, &mut buf).map_err(|e| DocxError::Io(format!("write {path}: {e}")))
    }

    /// 保存到内存缓冲（zip 容器字节）。
    pub fn save_to_vec(&mut self) -> Result<Vec<u8>, DocxError> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        let built = std::mem::take(&mut self.docx).build();
        built
            .pack(&mut cursor)
            .map_err(|e| DocxError::Io(format!("pack: {e}")))?;
        // build() 消费 self.docx —— 重建基态（写器可续写：幂等语义，
        // 已写内容已在 built 内落盘，续写 = 新档）。
        self.docx = Self::new().docx;
        Ok(cursor.into_inner())
    }
}

/// run 规格序列 → 引擎段落（链接 run → 引擎超链接元素[rels 由引擎
/// 打包收集]；脚注 marker run → 引擎脚注引用[footnotes.xml 由引擎
/// 打包收集]；域 run → fldChar begin/instrText/end 三连；普通 run
/// 直挂）。
fn specs_to_paragraph(runs: &[RunSpec]) -> docx_rs::Paragraph {
    let mut p = docx_rs::Paragraph::new();
    for spec in runs {
        // 脚注 marker（样式面忽略——引擎 add_footnote_reference 重置
        // run_property，见 RunSpec::footnote 文档）。
        if let Some(note) = &spec.footnote {
            let mut footnote = docx_rs::Footnote::new();
            footnote.add_content(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(note.as_str())),
            );
            p = p.add_run(docx_rs::Run::new().add_footnote_reference(footnote));
            continue;
        }
        // 简单域（begin/instrText/end 三连——引擎 add_tc 同构先例
        // run.rs:202-208）。
        if let Some(field) = spec.field {
            let instr = match field {
                FieldSpec::Page => docx_rs::InstrText::PAGE(docx_rs::InstrPAGE::new()),
                FieldSpec::NumPages => docx_rs::InstrText::NUMPAGES(docx_rs::InstrNUMPAGES::new()),
            };
            p = p
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false))
                .add_run(docx_rs::Run::new().add_instr_text(instr))
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false));
            continue;
        }
        let mut run = docx_rs::Run::new().add_text(spec.text.as_str());
        let mut rp = docx_rs::RunProperty::new();
        if spec.bold {
            rp = rp.bold();
        }
        if spec.italic {
            rp = rp.italic();
        }
        run.run_property = rp;
        if let Some(url) = &spec.link {
            p = p.add_hyperlink(
                docx_rs::Hyperlink::new(url.as_str(), docx_rs::HyperlinkType::External)
                    .add_run(run),
            );
        } else {
            p = p.add_run(run);
        }
    }
    p
}

/// 页眉槽位标记（原位替换目标）。
#[derive(Debug, Clone, Copy)]
enum SlotKind {
    Default,
    First,
    Even,
}

impl SlotKind {
    fn type_str(self) -> &'static str {
        match self {
            SlotKind::Default => "default",
            SlotKind::First => "first",
            SlotKind::Even => "even",
        }
    }
}

/// 原位替换页眉槽位（**保持原槽 rid**——docx-r2 修复：r1 无条件
/// rIdHeader1 在多槽组合下会踩踏[先 first 后 default 时 default 槽
/// 实为 rIdHeader2]；rid 保持 = rels 计数不变 + 无跨槽碰撞；
/// titlePg 激活位随槽保持——first 槽置位时引擎已设，防御性重申）。
fn replace_slot_header(docx: &mut docx_rs::Docx, slot: SlotKind, h: docx_rs::Header) {
    let sp = &mut docx.document.section_property;
    let existing = match slot {
        SlotKind::Default => sp.header.as_ref().map(|(r, _)| r.clone()),
        SlotKind::First => sp.first_header.as_ref().map(|(r, _)| r.clone()),
        SlotKind::Even => sp.even_header.as_ref().map(|(r, _)| r.clone()),
    };
    let rid = existing.unwrap_or_else(|| docx_rs::create_header_rid(1));
    match slot {
        SlotKind::Default => sp.header = Some((rid.clone(), h)),
        SlotKind::First => {
            sp.first_header = Some((rid.clone(), h));
            sp.title_pg = true;
        }
        SlotKind::Even => sp.even_header = Some((rid.clone(), h)),
    }
    let reference = docx_rs::HeaderReference::new(slot.type_str(), rid);
    match slot {
        SlotKind::Default => sp.header_reference = Some(reference),
        SlotKind::First => sp.first_header_reference = Some(reference),
        SlotKind::Even => sp.even_header_reference = Some(reference),
    }
}

/// 原位替换页脚槽位（同 [`replace_slot_header`]——footer 侧对称）。
fn replace_slot_footer(docx: &mut docx_rs::Docx, slot: SlotKind, f: docx_rs::Footer) {
    let sp = &mut docx.document.section_property;
    let existing = match slot {
        SlotKind::Default => sp.footer.as_ref().map(|(r, _)| r.clone()),
        SlotKind::First => sp.first_footer.as_ref().map(|(r, _)| r.clone()),
        SlotKind::Even => sp.even_footer.as_ref().map(|(r, _)| r.clone()),
    };
    let rid = existing.unwrap_or_else(|| docx_rs::create_footer_rid(1));
    match slot {
        SlotKind::Default => sp.footer = Some((rid.clone(), f)),
        SlotKind::First => {
            sp.first_footer = Some((rid.clone(), f));
            sp.title_pg = true;
        }
        SlotKind::Even => sp.even_footer = Some((rid.clone(), f)),
    }
    let reference = docx_rs::FooterReference::new(slot.type_str(), rid);
    match slot {
        SlotKind::Default => sp.footer_reference = Some(reference),
        SlotKind::First => sp.first_footer_reference = Some(reference),
        SlotKind::Even => sp.even_footer_reference = Some(reference),
    }
}

// ---------------------------------------------------------------------------
// 单测（全环往返与敌意输入见 tests/docx_roundtrip.rs）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_level_validation() {
        let mut w = DocxWriter::new();
        assert!(w.heading(1, "ok").is_ok());
        assert!(w.heading(9, "ok").is_ok());
        assert!(w.heading(0, "bad").is_err());
        assert!(w.heading(10, "bad").is_err());
    }

    #[test]
    fn run_spec_constructors() {
        assert_eq!(
            RunSpec::text("a"),
            RunSpec {
                text: "a".into(),
                bold: false,
                italic: false,
                link: None,
                footnote: None,
                field: None,
            }
        );
        assert!(RunSpec::styled("a", true, false).bold);
        assert_eq!(
            RunSpec::link("a", "https://x.example/"),
            RunSpec {
                text: "a".into(),
                bold: false,
                italic: false,
                link: Some("https://x.example/".into()),
                footnote: None,
                field: None,
            }
        );
        // docx-r2 构造器：脚注 marker / 两个简单域。
        assert_eq!(
            RunSpec::footnote("注"),
            RunSpec {
                text: String::new(),
                bold: false,
                italic: false,
                link: None,
                footnote: Some("注".into()),
                field: None,
            }
        );
        assert_eq!(RunSpec::field_page().field, Some(FieldSpec::Page));
        assert_eq!(RunSpec::field_num_pages().field, Some(FieldSpec::NumPages));
        assert_eq!(RunSpec::field_page().text, "");
    }

    /// specs_to_paragraph 展开：脚注 marker（引擎 FootnoteReference
    /// 变体 + 内容在脚注内 + marker 无文本）。
    #[test]
    fn specs_expand_footnote_marker() {
        let p = specs_to_paragraph(&[
            RunSpec::text("前文"),
            RunSpec::footnote("脚注内容"),
            RunSpec::text("后文"),
        ]);
        let mut texts = Vec::new();
        let mut footnote_refs = 0;
        for child in &p.children {
            if let docx_rs::ParagraphChild::Run(r) = child {
                for c in &r.children {
                    match c {
                        docx_rs::RunChild::Text(t) => texts.push(t.text.clone()),
                        docx_rs::RunChild::FootnoteReference(f) => {
                            footnote_refs += 1;
                            assert_eq!(f.content.len(), 1, "脚注内容单段");
                            assert!(matches!(
                                &f.content[0].children[0],
                                docx_rs::ParagraphChild::Run(rr)
                                    if matches!(&rr.children[0],
                                        docx_rs::RunChild::Text(t) if t.text == "脚注内容")
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(texts, vec!["前文", "后文"], "marker 无文本 run");
        assert_eq!(footnote_refs, 1);
    }

    /// specs_to_paragraph 展开：简单域三连（begin/instrText/end——
    /// 引擎模型级断言）。
    #[test]
    fn specs_expand_simple_fields() {
        let p = specs_to_paragraph(&[RunSpec::field_page(), RunSpec::field_num_pages()]);
        let mut fld_chars = Vec::new();
        let mut instr = Vec::new();
        for child in &p.children {
            if let docx_rs::ParagraphChild::Run(r) = child {
                for c in &r.children {
                    match c {
                        docx_rs::RunChild::FieldChar(f) => {
                            fld_chars.push(format!("{:?}", f.field_char_type))
                        }
                        docx_rs::RunChild::InstrText(i) => instr.push(match i.as_ref() {
                            docx_rs::InstrText::PAGE(_) => "PAGE".to_string(),
                            docx_rs::InstrText::NUMPAGES(_) => "NUMPAGES".to_string(),
                            other => format!("{other:?}"),
                        }),
                        other => panic!("域 run 意外子元素: {other:?}"),
                    }
                }
            }
        }
        assert_eq!(fld_chars, vec!["Begin", "End", "Begin", "End"]);
        assert_eq!(instr, vec!["PAGE", "NUMPAGES"]);
    }

    /// 图片写面验证：非 PNG 签名 / 零尺寸 typed 拒；正常 PNG 过
    /// （引擎 Pic 尺寸换算 from_px = px × 9525）。
    #[test]
    fn image_write_validation() {
        let mut w = DocxWriter::new();
        assert!(matches!(
            w.image(b"JFIF-looks-like-jpeg", 10, 10),
            Err(DocxError::Parse(m)) if m.contains("PNG")
        ));
        assert!(matches!(
            w.image(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0], 0, 10),
            Err(DocxError::Parse(m)) if m.contains("positive")
        ));
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
        w.image(&png, 320, 240).unwrap();
        // 引擎模型级：单段单 run 单 Drawing（Pic 尺寸 EMU 换算）。
        let children = &w.docx.document.children;
        let docx_rs::DocumentChild::Paragraph(p) = &children[0] else {
            panic!("首块应为段落");
        };
        let docx_rs::ParagraphChild::Run(r) = &p.children[0] else {
            panic!("首子元素应为 run");
        };
        let docx_rs::RunChild::Drawing(d) = &r.children[0] else {
            panic!("run 应含 Drawing");
        };
        let Some(docx_rs::DrawingData::Pic(pic)) = &d.data else {
            panic!("Drawing 应含 Pic");
        };
        assert_eq!(pic.size, (320 * 9525, 240 * 9525));
        assert_eq!(pic.image, png.to_vec());
    }

    /// [Content_Types].xml 解析：Default/Override + 查表优先级
    /// （Override > Default）+ 扩展名小写归一。
    #[test]
    fn content_types_parse_and_lookup() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="PNG" ContentType="image/png"/>
<Default Extension="jpeg" ContentType="image/jpeg"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/media/special.bin" ContentType="application/x-custom"/>
</Types>"#;
        let mut w = Vec::new();
        let ct = parse_content_types(xml, true, &mut w).unwrap();
        assert_eq!(w.len(), 0);
        assert_eq!(
            ct.content_type_of("word/media/a.PnG").as_deref(),
            Some("image/png"),
            "扩展名小写归一查 Default"
        );
        assert_eq!(
            ct.content_type_of("word/media/b.jpeg").as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            ct.content_type_of("word/document.xml").as_deref(),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
            ),
            "Override 命中"
        );
        assert_eq!(
            ct.content_type_of("word/media/special.bin").as_deref(),
            Some("application/x-custom"),
            "Override 优先于扩展名 Default"
        );
        assert_eq!(ct.content_type_of("word/media/c.tiff"), None, "无声明");
        assert_eq!(ct.content_type_of("word/media/noext"), None, "无扩展名");
    }

    /// [Content_Types].xml 词汇表外元素：严格拒 / 兼容警告 + 跳过
    /// （合法条目保留）。
    #[test]
    fn content_types_unknown_element_strict_compat() {
        let xml = br#"<Types><Default Extension="png" ContentType="image/png"/><zzz/></Types>"#;
        let mut w = Vec::new();
        match parse_content_types(xml, true, &mut w) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("unknown element"), "{detail}")
            }
            other => panic!("strict 应拒, got {other:?}"),
        }
        let mut w = Vec::new();
        let ct = parse_content_types(xml, false, &mut w).unwrap();
        assert_eq!(ct.content_type_of("a.png").as_deref(), Some("image/png"));
        assert!(w
            .iter()
            .any(|x| x.kind == WarningKind::MalformedXml && x.detail.contains("unknown element")));
    }

    /// [Content_Types].xml 缺属性条目 + 重复条目：严格拒 / 兼容跳过
    /// 或首胜。
    #[test]
    fn content_types_missing_and_duplicate_entries() {
        let missing = br#"<Types><Default Extension="png"/><Default Extension="gif" ContentType="image/gif"/></Types>"#;
        let mut w = Vec::new();
        match parse_content_types(missing, true, &mut w) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("missing ContentType"), "{detail}")
            }
            other => panic!("strict 应拒, got {other:?}"),
        }
        let mut w = Vec::new();
        let ct = parse_content_types(missing, false, &mut w).unwrap();
        assert_eq!(ct.content_type_of("a.gif").as_deref(), Some("image/gif"));
        assert_eq!(ct.content_type_of("a.png"), None, "缺属性条目跳过");
        assert_eq!(
            w.iter()
                .filter(|x| x.kind == WarningKind::MalformedXml)
                .count(),
            1
        );
        let dup = br#"<Types><Default Extension="png" ContentType="image/png"/><Default Extension="png" ContentType="image/x-other"/></Types>"#;
        let mut w = Vec::new();
        assert!(parse_content_types(dup, true, &mut w).is_err());
        let mut w = Vec::new();
        let ct = parse_content_types(dup, false, &mut w).unwrap();
        assert_eq!(
            ct.content_type_of("a.png").as_deref(),
            Some("image/png"),
            "首胜"
        );
    }

    /// [Content_Types].xml 垃圾/截断：严格拒 / 兼容降级空表。
    #[test]
    fn content_types_garbage_strict_rejects_compat_degrades() {
        let truncated = b"<Types><Default Extensio"; // 标签内截断
        let mut w = Vec::new();
        assert!(parse_content_types(truncated, true, &mut w).is_err());
        let mut w = Vec::new();
        let ct = parse_content_types(truncated, false, &mut w).unwrap();
        assert_eq!(ct.content_type_of("a.png"), None, "降级空表");
        assert!(!w.is_empty());
    }

    /// .rels 解析：正常四属性（实体补偿：&amp; → &）。
    #[test]
    fn rels_parse_basic_with_entity_compensation() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://x.example/?a=1&amp;b=2" TargetMode="External"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
</Relationships>"#;
        let mut w = Vec::new();
        let t = parse_rels_part(xml, "word/_rels/document.xml.rels", true, &mut w)
            .unwrap()
            .expect("strict ok");
        assert_eq!(w.len(), 0);
        let r1 = t.get("rId1").unwrap();
        assert_eq!(r1.target, "https://x.example/?a=1&b=2");
        assert_eq!(r1.target_mode.as_deref(), Some("External"));
        let r2 = t.get("rId2").unwrap();
        assert_eq!(r2.target, "header1.xml");
        assert_eq!(r2.target_mode, None);
    }

    /// .rels 词汇表外元素：严格拒 / 兼容警告 + 跳过（引擎同等容忍
    /// [CAPABILITY_NOTES §8]——警告面 = 可见性提升，合法条目保留）。
    #[test]
    fn rels_unknown_element_strict_rejects_compat_skips() {
        let xml = br#"<Relationships><Relationship Id="rId1" Type="t" Target="x.xml"/><zzz/></Relationships>"#;
        let mut w = Vec::new();
        match parse_rels_part(xml, "p", true, &mut w) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("unknown element"), "{detail}")
            }
            other => panic!("strict 应拒, got {other:?}"),
        }
        let mut w = Vec::new();
        let t = parse_rels_part(xml, "p", false, &mut w)
            .unwrap()
            .expect("兼容容忍");
        assert!(t.get("rId1").is_some(), "合法条目保留");
        assert!(w
            .iter()
            .any(|x| x.kind == WarningKind::MalformedXml && x.detail.contains("unknown element")));
    }

    /// .rels 根元素名错误：严格拒 / 兼容警告 + 条目照常解析
    /// （引擎同等容忍——任意根下任意深度 Relationship 均收集）。
    #[test]
    fn rels_wrong_root_strict_rejects_compat_tolerates() {
        let xml = br#"<NotRelationships><Relationship Id="rId1" Type="t" Target="x.xml"/></NotRelationships>"#;
        let mut w = Vec::new();
        assert!(parse_rels_part(xml, "p", true, &mut w).is_err());
        let mut w = Vec::new();
        let t = parse_rels_part(xml, "p", false, &mut w)
            .unwrap()
            .expect("兼容容忍");
        assert!(t.get("rId1").is_some(), "条目照常解析");
        assert!(!w.is_empty());
    }

    /// 缺属性条目：严格拒 / 兼容警告 + 跳过（其余条目保留）。
    #[test]
    fn rels_missing_attributes_strict_rejects_compat_skips() {
        let xml = br#"<Relationships>
<Relationship Id="rId1" Type="t" Target="x.xml"/>
<Relationship Type="t" Target="y.xml"/>
</Relationships>"#;
        let mut w = Vec::new();
        match parse_rels_part(xml, "p", true, &mut w) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("missing Id"), "{detail}")
            }
            other => panic!("strict 应拒, got {other:?}"),
        }
        let mut w = Vec::new();
        let t = parse_rels_part(xml, "p", false, &mut w)
            .unwrap()
            .expect("compat ok");
        assert!(t.get("rId1").is_some(), "合法条目保留");
        assert_eq!(
            w.iter()
                .filter(|x| x.kind == WarningKind::MalformedXml)
                .count(),
            1,
            "跳条目警告恰一条"
        );
    }

    /// 重复 Id：严格拒 / 兼容警告 + 首胜。
    #[test]
    fn rels_duplicate_id_strict_rejects_compat_first_wins() {
        let xml = br#"<Relationships>
<Relationship Id="rId1" Type="t" Target="first.xml"/>
<Relationship Id="rId1" Type="t" Target="second.xml"/>
</Relationships>"#;
        let mut w = Vec::new();
        match parse_rels_part(xml, "p", true, &mut w) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("duplicate"), "{detail}")
            }
            other => panic!("strict 应拒, got {other:?}"),
        }
        let mut w = Vec::new();
        let t = parse_rels_part(xml, "p", false, &mut w)
            .unwrap()
            .expect("compat ok");
        assert_eq!(t.get("rId1").unwrap().target, "first.xml", "首胜");
        assert!(w.iter().any(|x| x.kind == WarningKind::MalformedXml));
    }

    /// 垃圾 .rels（quick-xml 解析失败面——截断）：严格拒（门 3 同族
    /// 不可达路径的保守同拒）/ 兼容降级（None + 警告）。
    #[test]
    fn rels_garbage_strict_rejects_compat_degrades() {
        let truncated = b"<Relationships><Relatio"; // 标签内截断 → 解析错误
        let mut w = Vec::new();
        assert!(
            parse_rels_part(truncated, "p", true, &mut w).is_err(),
            "严格拒"
        );
        let mut w = Vec::new();
        let t = parse_rels_part(truncated, "p", false, &mut w).unwrap();
        assert!(t.is_none(), "降级 = 无表");
        assert!(!w.is_empty(), "降级警告在档");
    }

    /// 垃圾 .rels（quick-xml 宽名容忍面——0-255 循环字节被解析为
    /// 怪名元素）：严格拒（怪名元素 = 词汇表外）；兼容容忍面不
    /// panic（怪名警告 + 后续解析错误则降级——两路径均警告在档）。
    #[test]
    fn rels_garbage_weird_names_strict_rejects_compat_tolerates() {
        let garbage: Vec<u8> = (0..256u16).map(|i| i as u8).cycle().take(512).collect();
        let mut w = Vec::new();
        match parse_rels_part(&garbage, "p", true, &mut w) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("unknown element"), "{detail}")
            }
            other => panic!("strict 应词汇表拒, got {other:?}"),
        }
        let mut w = Vec::new();
        // 兼容：不 panic（警告容忍或降级——两者皆合法兼容行为）。
        let _ = parse_rels_part(&garbage, "p", false, &mut w).unwrap();
        assert!(!w.is_empty(), "警告在档");
    }

    /// 属主部件路径推导。
    #[test]
    fn rels_owner_part_derivation() {
        assert_eq!(
            rels_owner_part("word/_rels/document.xml.rels").as_deref(),
            Some("word/document.xml")
        );
        assert_eq!(
            rels_owner_part("word/_rels/header1.xml.rels").as_deref(),
            Some("word/header1.xml")
        );
        assert_eq!(rels_owner_part("_rels/.rels"), None, "包根 rels 无属主");
        assert_eq!(
            rels_owner_part("word/document.xml.rels"),
            None,
            "非 _rels 目录"
        );
    }

    /// 包内路径词法归一化 + 越出包根检出。
    #[test]
    fn part_path_normalization() {
        assert_eq!(
            normalize_part_path("word/../evil.xml").as_deref(),
            Some("evil.xml")
        );
        assert_eq!(
            normalize_part_path("word/./a/b.xml").as_deref(),
            Some("word/a/b.xml")
        );
        assert_eq!(
            normalize_part_path("word/../../etc/passwd"),
            None,
            "越出包根 = 敌意"
        );
    }

    /// 联接分派：External / Internal / Anchor / Unresolved。
    #[test]
    fn hyperlink_resolution_dispatch() {
        let mut table = RelationshipTable::default();
        table.by_id.insert(
            "rId1".into(),
            Relationship {
                id: "rId1".into(),
                rel_type: "hyperlink".into(),
                target: "https://x.example/".into(),
                target_mode: Some("External".into()),
            },
        );
        table.by_id.insert(
            "rId2".into(),
            Relationship {
                id: "rId2".into(),
                rel_type: "hyperlink".into(),
                target: "other.docx".into(),
                target_mode: None,
            },
        );
        let mut w = Vec::new();
        assert_eq!(
            resolve_link("rId1", Some(&table), "p", true, &mut w).unwrap(),
            HyperlinkTarget::External {
                target: "https://x.example/".into()
            }
        );
        assert_eq!(
            resolve_link("rId2", Some(&table), "p", true, &mut w).unwrap(),
            HyperlinkTarget::Internal {
                target: "other.docx".into()
            }
        );
        // 锚点（无关系面）。
        let h = docx_rs::Hyperlink::new("toc1", docx_rs::HyperlinkType::Anchor);
        assert_eq!(
            resolve_hyperlink(&h, None, "p", true, &mut w).unwrap(),
            HyperlinkTarget::Anchor {
                anchor: "toc1".into()
            }
        );
        // 悬空：严格拒。
        match resolve_link("rId9", Some(&table), "p", true, &mut w) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("rId9"), "{detail}")
            }
            other => panic!("strict 应拒, got {other:?}"),
        }
        // 悬空：兼容 Unresolved + BrokenReference 警告。
        let mut w = Vec::new();
        assert_eq!(
            resolve_link("rId9", Some(&table), "p", false, &mut w).unwrap(),
            HyperlinkTarget::Unresolved { rid: "rId9".into() }
        );
        assert!(w.iter().any(|x| x.kind == WarningKind::BrokenReference));
    }
}
