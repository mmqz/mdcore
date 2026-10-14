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
//!   ——reader/xml_element.rs:447，CAPABILITY_NOTES §8 实证勘误）；
//!   本层对同一面给显式信号：严格拒（OPC 关系部件模式 ECMA-376
//!   Part 2 词汇表）/ 兼容警告 + 跳过（引擎同等容忍 + 可见性）。
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
//! - 图形/图片/形状（Drawing/Shape——模型级切片不含媒体面；
//!   引擎包级字节面在 docx-rs 读模型 `Docx::images`，未暴露）；
//! - **表格单元格内超链接目标**（单元格文本保留，目标丢弃——
//!   TableData 扁平文本面；引擎解析能力在档
//!   [docx-r1/CAPABILITY_NOTES §2，分析仓]）；
//! - 修订追踪（Insert/Delete/MoveFrom/MoveTo 容器——内容不入提取）；
//! - 页码/总页数域（PageNum/NumPages）、书签、批注、
//!   StructuredDataTag（内容控件）；
//! - 脚注/尾注（**读面引擎受限**：docx-rs 从不读 footnotes.xml 且
//!   正文 `w:footnoteReference` 不入模型——CAPABILITY_NOTES §5；
//!   写面引擎在，未实测不暴露）；
//! - 节属性（page size/margin 等——页眉页脚引用面已暴露，其余
//!   节语义面外）；first/even 页眉页脚槽位的**激活设置语义**
//!   （titlePg/evenAndOddHeaders——变体透传，激活判定归消费方）；
//! - 编号/列表语义（列表段落文本保留，编号结构丢弃）；
//! - 域指令文本（instrText 引擎入模型字符串，提取面外——登记）。
//!
//! 写侧**不支持**（引擎能力在[docx-rs 写面远大于本切片]，未实测
//! 不暴露）：图片、表格样式、域、目录、编号列表、first/even 页眉
//! 页脚槽位、页眉页脚内表格（引擎均支持——CAPABILITY_NOTES）。
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

/// 整档提取结果：块序 + 页眉页脚 + 结构化警告（容器门面，与 XLSX 一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct DocxDocument {
    pub blocks: Vec<Block>,
    /// 页眉（default/first/even 中实际挂接的——引用悬空时严格拒 /
    /// 兼容警告 + 槽位跳过）。
    pub headers: Vec<HeaderFooterData>,
    /// 页脚（同上）。
    pub footers: Vec<HeaderFooterData>,
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
/// 上限两模式恒拒；词汇表/schema 严格拒兼容降级）→ docx-rs 模型
/// 解析 → 关系联接提取。
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

    // OPC 关系层（同一门后字节——零门绕过）。
    let bundle = {
        let cursor = std::io::Cursor::new(&bytes);
        let mut za = zip::ZipArchive::new(cursor)
            .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
        build_rels_bundle(&mut za, strict, &mut warnings)?
    };

    let docx = docx_rs::read_docx(&bytes).map_err(|e| DocxError::Parse(format!("{e}")))?;
    let blocks = extract_blocks(&docx, &bundle, strict, &mut warnings)?;
    let (headers, footers) = extract_headers_footers(&docx, &bundle, strict, &mut warnings)?;
    Ok(DocxDocument {
        blocks,
        headers,
        footers,
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
}

impl RunSpec {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            link: None,
        }
    }

    pub fn styled(text: impl Into<String>, bold: bool, italic: bool) -> Self {
        Self {
            text: text.into(),
            bold,
            italic,
            link: None,
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

    /// 默认页眉（单段 run 序——run 级加粗/斜体/链接）。**default
    /// 槽位，重复调用 = 原位替换**（引擎公开 `Docx::header` 按调用
    /// 次数递增 rels 计数——重复调用会发射 rIdHeader{n+1} 关系而无
    /// 对应部件[自产悬空引用，documents/mod.rs:576-589 计数语义]；
    /// 本包装层改为原位替换保持包自洽——同 XLSX 流式纪律的包装层
    /// 防御先例）；first/even 槽位引擎支持、本面未暴露（登记——
    /// 未实测不暴露）。
    pub fn header(&mut self, runs: &[RunSpec]) {
        let h = docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.header.is_some() {
            replace_default_header(&mut self.docx, h);
        } else {
            self.docx = std::mem::take(&mut self.docx).header(h);
        }
    }

    /// 默认页脚（单段 run 序——同 [`Self::header`]，重复调用 = 原位替换）。
    pub fn footer(&mut self, runs: &[RunSpec]) {
        let f = docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs));
        if self.docx.document.section_property.footer.is_some() {
            replace_default_footer(&mut self.docx, f);
        } else {
            self.docx = std::mem::take(&mut self.docx).footer(f);
        }
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
/// 打包收集]；普通 run 直挂）。
fn specs_to_paragraph(runs: &[RunSpec]) -> docx_rs::Paragraph {
    let mut p = docx_rs::Paragraph::new();
    for spec in runs {
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

/// 原位替换 default 页眉槽位（保持 `rIdHeader1` + rels 计数不变——
/// 替代引擎重复调用计数递增的自产悬空引用行为；rId 格式锚 =
/// documents/header_id.rs `create_header_rid`）。
fn replace_default_header(docx: &mut docx_rs::Docx, h: docx_rs::Header) {
    let sp = &mut docx.document.section_property;
    let rid = docx_rs::create_header_rid(1);
    sp.header = Some((rid.clone(), h));
    sp.header_reference = Some(docx_rs::HeaderReference::new("default", rid));
}

/// 原位替换 default 页脚槽位（同 [`replace_default_header`]——
/// documents/footer_id.rs `create_footer_rid`）。
fn replace_default_footer(docx: &mut docx_rs::Docx, f: docx_rs::Footer) {
    let sp = &mut docx.document.section_property;
    let rid = docx_rs::create_footer_rid(1);
    sp.footer = Some((rid.clone(), f));
    sp.footer_reference = Some(docx_rs::FooterReference::new("default", rid));
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
            }
        );
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
