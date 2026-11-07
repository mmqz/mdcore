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
//! - **嵌套表格（读）**：`w:tbl` 嵌于外层表 `w:tc`（depth≥2）——内层
//!   表沿文档序以 `Block::Table` 独立发射（`TableData::nested = true`
//!   标记；外层在前、内层行主序随后——表数守恒；深度上限 16 防敌意
//!   栈溢出，超限整块跳过 + `SkippedElement` warning——#39 闭项，
//!   此前静默丢弃 = 标书侧费率表整块缺失）；
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
//! **t20 增量（2026-10-15）**：主题 = **写面五面扩——列表/等宽/
//! 水平线/引用/行内删除线下画线**（底稿 = 分析仓 `office-line/docx-r1/
//! WRITE_FACE_PROBE.md`——docx-rs 0.4.22 一手源码核验 + spike 探针
//! 16/16 双通道实证；t18 四件登记降级 + 块级 code 等宽 + 行内 del·u
//! 整批消解）：
//! - **numbering（列表）**：[`DocxWriter::list_item`]——两个 abstractNum
//!   （ordered decimal 9 级 / bullet `•` 9 级）+ num 桥 + 段落
//!   `Paragraph::numbering`（numId 100/101，避开引擎内建 numId=1）；
//!   **必须走 `Paragraph::numbering` 方法**——同置 `has_numbering`
//!   → document.xml.rels 接线；`numbering_property` 路径不置位 →
//!   rels 缺关系 → 悬空 numPr（WRITE_FACE_PROBE §2.1 陷阱）；
//! - **等宽 run**：[`RunSpec`] 扩 `mono`/`strike`/`underline` 三位
//!   （`Run::fonts`[rFonts 三槽]/`strike`/`underline`）；
//! - **块级 code**：[`DocxWriter::code_line`] 双通道等宽（段落级
//!   `pPr>rPr>rFonts` + run 级 rFonts——run 级保真 + 段落级扈段标记）；
//! - **水平线**：[`DocxWriter::horizontal_rule`]——空段下边框
//!   （**`ParagraphBorders::with_empty().set` 单边——`new()` 四边全
//!   Single 陷阱**，WRITE_FACE_PROBE §2.3）；
//! - **引用**：[`DocxWriter::quote_paragraph`]——左缩进 720×depth
//!   （钳 8）+ 左边框（无内建 Quote 样式可引——直接格式）。
//!   读面 numPr/pBdr/ind/rFonts 的模型承载未核（读模型无对应字段
//!   ——XML 级断言见 tests/docx_roundtrip.rs，读面登记维持）。
//!   **r4-t3 追记**：其中 numPr/ind 两面已落地读面承载（见下方
//!   r4-t3 增量节）；pBdr/rFonts 维持登记（引擎值私有，见增量节）。
//!
//! **t22 增量（2026-10-16）**：[`DocxWriter::list_item_lines`]——多行
//! 列表项续行缩进面（t20 登记残余翻转为已表达）：一个列表项 = 一个
//! OOXML 段落（行间 `w:br w:type="textWrapping"` 软换行）+ 段落级
//! `w:ind`（left = 720×(ilvl+1)、hanging [`LIST_HANGING_INDENT`]——与
//! 编号抽象层级同值的悬挂几何）→ 续行与首行文字列对齐；单行项
//! [`DocxWriter::list_item`] 字节级零变化（只增不改）。
//!
//! **r4-t3 增量（2026-10-18，DOCX 读写保真）**：读面**段落结构双承载**
//! ——t20-a 遗留④「numPr/pBdr/ind/rFonts 读面往返承载」中可承载
//! 两面落地（完整矩阵与方案论证 = 分析仓 `office-line/docx-r1/
//! FIDELITY.md`）：
//! - **numPr 承载**（[`ParagraphData::numbering`] → [`ParagraphNumbering`]）：
//!   num_id/ilvl 透传 + **语义解析**（numId → num 桥 → abstractNum →
//!   该 ilvl 级 numFmt——引擎模型全链 pub，见 [`NumberingIndex`]；
//!   lvlOverride 并入）→ kind（bullet → [`ListKind::Bullet`]，其余
//!   → Ordered）+ format（原生 numFmt 值）。**写回降级可诊断**：
//!   写面经自有抽象 100/101 重定义（非 decimal/bullet 的原生格式
//!   降级——format 字段使消费方可比对诊断，登记）；num 桥悬空 =
//!   严格拒 / 兼容 BrokenReference 警告 + kind=None 结构承载；
//!   numId=0（OOXML 去编号惯例）/无 numId → 非列表段；
//! - **w:ind 承载**（[`ParagraphData::indent`] → [`ParagraphIndent`]）：
//!   left/right/firstLine/hanging twips 透传（Chars 字符单位变体登记
//!   不承载）；写面无任意 ind 面（仅经列表/引用抽象间接表达——
//!   写入降级登记）。
//!
//! 同轮**维持登记不承载**（引擎值私有——presence 对显式关闭形态会
//! 误报，不做不诚实承载）：行内 rFonts/strike/underline
//! （RunFonts/Strike.val/Underline.val 无公开取值面）、段落边框 pBdr
//! （ParagraphBorders 六槽私有）、bold/italic 的 `w:val="false"`
//! 显式关闭变体（Bold.val/Italic.val 私有——presence 语义为既有行为
//! 维持，登记）。详见 FIDELITY.md 矩阵行锚点。
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
//!   （文本/加粗/斜体/**超链接目标**）+ **编号/缩进承载**（r4-t3：
//!   [`ParagraphData::numbering`]/[`ParagraphData::indent`]）；表 =
//!   行 × 单元格拼接文本（单元格内多段落以 `\n` 连接）；
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
//! - 修订追踪：**`w:ins` = 当前生效文本，已提取**（run 级
//!   `revision_inserted` 标记——#17；跳过整容器会静默丢失生效条款）；
//!   `w:del`/`delText` = 已删除内容不提取（逐段计数登记
//!   `WarningKind::RevisionDelete`）；MoveFrom/MoveTo（移动修订）v0
//!   不提取（幽灵文本/移动目标——登记，#17 范围外）；
//! - 页码/总页数域（PageNum/NumPages）、书签、批注、
//!   StructuredDataTag（内容控件）；
//! - 脚注/尾注（**读面引擎受限**：docx-rs 从不读 footnotes.xml 且
//!   正文 `w:footnoteReference` 不入模型——CAPABILITY_NOTES §5；
//!   写面已暴露 [`RunSpec::footnote`]，读回文本不含脚注内容——
//!   XML 级断言见 tests/docx_roundtrip.rs）；
//! - 节属性（page size/margin 等——页眉页脚引用面已暴露，其余
//!   节语义面外）；first/even 页眉页脚槽位的**激活设置语义**
//!   （titlePg/evenAndOddHeaders——变体透传，激活判定归消费方）；
//! - **编号语义已承载（r4-t3 翻转）**：numPr → [`ParagraphNumbering`]
//!   （num_id/ilvl/kind/format——解析链悬空处置见 [`NumberingIndex`]）；
//!   **编号定义原始抽象不承载**（自定义 numFmt/lvlText/glyphs——
//!   写回经自有抽象 100/101 降级，format 字段可诊断）；
//! - **行内 rFonts/strike/underline 与段落边框 pBdr 读面不承载**
//!   （r4-t3 维持登记——引擎解析入模型但值字段私有无取值面；
//!   presence-only 对 `w:u w:val="none"`/`w:val="false"` 显式关闭
//!   形态会误报，不做不诚实承载——完整论证 = docx-r1/FIDELITY.md）；
//!   bold/italic 为 presence 语义（`w:val="false"` 关闭变体不可区分
//!   ——引擎 Bold.val/Italic.val 私有，既有行为维持登记）；
//! - **缩进已承载（r4-t3 翻转）**：w:ind → [`ParagraphIndent`]（twips；
//!   Chars 字符单位变体登记不承载；写面无任意 ind 面——写入降级）；
//!   **段落边框 pBdr 不承载**（ParagraphBorders 六槽字段私有）；
//! - 域指令文本（instrText 引擎入模型字符串，提取面外——登记；
//!   写面简单域 PAGE/NUMPAGES 已暴露，域结果文本不入读回文本——
//!   域值是渲染期计算面；域**缓存结果文本以普通文本呈现**）；
//!   **退化域（无 separate 指令域，含 WPS `""` 前缀形态）指令恒不
//!   入正文**——四实现面共识（LO 渲染/LO 导入/pandoc/python-docx）
//!   裁决 = docs/field-code-semantics.md（#40）；
//!
//! **#257 增量（2026-11-06，多节分节符读写面——审计 P1-1 修复）**：
//! 主题 = **多节文档全量承载**（此前读面只取末节——中段节页眉/页脚/
//! 几何/节边界静默丢，P1-1；写面单节不可表达）：
//! - **节（读）**：[`DocxDocument::sections`]（恒 ≥1 条——单节文档 =
//!   1 条末节）逐节承载几何（pgSz/pgMar twips——[`PageGeometry`]
//!   同域）/titlePg/分栏数/节类型/起始页码/六槽位页眉页脚/**块区间**
//!   `[start, end)`（连续切分 `blocks` 全序）。中段节页眉/页脚部件
//!   **引擎第二趟自解析**（docx-rs `read_docx` 只把内容联接到末节
//!   ——`read_docx.rs:232-290`，引用在而内容不联接 = P1-1 静默丢的
//!   根因）；末节槽位 = 文档级 [`DocxDocument::headers`]/[`footers`]
//!   同源（既有语义零变化——health-check 基线零漂移）；
//! - **节（写）**：[`DocxWriter::section_break`]（[`SectionBreakSpec`]
//!   ——几何/titlePg/起始页码/本节六槽位）在已发射内容后闭合一节
//!   （OOXML 段落级 sectPr 语义——docx-rs `DocumentChild::Section`
//!   落地）；末节起始页码 [`DocxWriter::page_number_start`]（封面
//!   不计数 + 正文从 1 起算的典型位）；几何正值域校验同 V9（构造期
//!   粘性拒绝）；
//! - **编辑面**：sectPr 属编辑辖外（不动 = 保真）——多节文档手术
//!   编辑节结构不变，`tests/docx_sections.rs::edit_preserves_sections`
//!   回归钉住；
//! - **证据链**：`tests/docx_sections.rs`（写→读回环 + 单节基线 +
//!   docx4j/POI 真实语料 P1-1 钉面 + 编辑幸存 + 校验门）。
//!
//! **#345 增量（2026-11-07，#318 D2 修复——表内 DrawingML 文本框）**：
//! 主题 = **`w:tc` 容器内 `w:drawing > wps:txbx > w:txbxContent` 补提取**
//! （此前单元格路径只取 `TableCellContent::Paragraph` 文本，DrawingML
//! 文本框子树静默双丢——结构与内容均不可见，费率表等条款区常见形态）：
//! - **第二趟**（[`scan_tc_textbox_regions`]/[`apply_tc_textboxes`]——
//!   与 #18 VML 趟同构 quick-xml local-name 通道）：捕获 tc 内 drawing
//!   形态 txbxContent（裸 drawing 或 mc:AlternateContent Choice 主选；
//!   Fallback 副本 / body 级 sdt 包裹 / 嵌套非最外层 / VML 形态四类
//!   跳过——与 GT 账本 `txbx_in_tc` 计数条件互补成对）；
//! - **发射位**：宿主表块**完整跨度后**（外层表块 + 全部内层表块——
//!   lockstep 重走对齐 body 子层序到最终数组坐标；表内框归属整表）；
//! - **边界**：与 D4（wgp 组内文本框——#346）边界清晰：本修复只做
//!   tc 容器内形态；页眉/页脚部件内的表内框维持登记（第二趟仅扫
//!   document.xml，与 VML 趟同口径）；
//! - **证据链**：tests/tr32_tc_textbox.rs（10 件真实语料锚——
//!   extraction-probe 族 8 + mc-fun AC 包裹 2：条款文本在场 + 恰一次
//!   [零双提取] + 宿主表完整 + 发射位钉定）+ 结构账本
//!   `txbx_in_tc` 扣除项移除（tests/docx_structural_ledger.rs——平价
//!   公式重钉）。
//!
//! 写侧**不支持**（引擎能力在[docx-rs 写面远大于本切片]，未实测
//! 不暴露）：图片自动转码（`Pic::new` 需 `image` 特性 = +23 crate
//! 依赖树——登记，PNG 显式尺寸面已暴露）、非 PNG 图片、浮动定位/
//! 旋转图片、表格样式、TOC/PAGEREF/HYPERLINK 域、目录、
//! 页眉页脚内表格、锚点写面（引擎均支持——CAPABILITY_NOTES）。
//! 列表写面 t20 已暴露（[`DocxWriter::list_item`]——ordered/bullet
//! 两抽象 × 9 级；多列族混排/自定义编号格式/级别覆盖未暴露，引擎
//! 能力在——按需增量）。
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

/// DOCX 读/写统一错误（四分：容器安全拒 / 解析 / IO / 写面资源限
/// [r4-t2]）。
#[derive(Debug)]
pub enum DocxError {
    /// 容器门/关系层拒绝（门 1/2/3 + .rels 结构校验——与 XLSX 同一
    /// 门结构；内含具体拒绝面）。
    Container(io::ReadError),
    /// docx-rs 模型解析错误。
    Parse(String),
    /// 文件 IO 错误。
    Io(String),
    /// 写面资源上限拒绝（r4-t2：[`MAX_PARAGRAPHS`] / [`MAX_RUNS`] ——
    /// 安全类，与解析[格式]错误可区分；save 面统一漏斗 typed 拒，
    /// 超限调用无产物返回）。
    Limit(String),
    /// 写面输入校验拒绝（office-g Phase 1/2——V1-V7 校验矩阵：
    /// 书签名域/保留名/重名/空锚点；fail-closed 宁缺毋错）。
    /// **加性枚举变体**（office-g §11 e-review 勘误后口径——本方案
    /// 唯一枚举新增面；workspace 内 exhaustive match 机械补齐）。
    InvalidInput(String),
}

impl std::fmt::Display for DocxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocxError::Container(e) => write!(f, "container: {e}"),
            DocxError::Parse(m) => write!(f, "parse error: {m}"),
            DocxError::Io(m) => write!(f, "io error: {m}"),
            DocxError::Limit(m) => write!(f, "resource limit: {m}"),
            DocxError::InvalidInput(m) => write!(f, "invalid input: {m}"),
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

/// 写面段落数上限（r4-t2 资源面：引擎 docx-rs 段落 Vec 无内建上限
/// ——一手核验；计段等价物 = 发射调用累计[含表格单元格与页眉页脚槽
/// 位，替换槽位按每次调用计——上界语义]，save 面统一漏斗 typed 拒）。
/// 值与 XLSX 写面 [`crate::write::MAX_CELLS`] 同量级（百万级——
/// 基准见 examples/bench_limits.rs）。
pub const MAX_PARAGRAPHS: u64 = 1_000_000;
/// 写面 run 数上限（r4-t2；计 run = 发射调用累计，同上界语义；
/// 脚注内容段计 +1 段 +1 run）。
pub const MAX_RUNS: u64 = 4_000_000;

/// 写面规模检查（r4-t2——消费方 preflight 与 save 面共用的单点比较；
/// `MAX_PARAGRAPHS`/`MAX_RUNS` 的公开执行面）。
pub fn check_document_scale(paragraphs: u64, runs: u64) -> Result<(), DocxError> {
    if paragraphs > MAX_PARAGRAPHS {
        return Err(DocxError::Limit(format!(
            "paragraph count over limit {MAX_PARAGRAPHS}"
        )));
    }
    if runs > MAX_RUNS {
        return Err(DocxError::Limit(format!("run count over limit {MAX_RUNS}")));
    }
    Ok(())
}

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
    /// S1 样式面：pub(crate)（style.rs 主题/样式部件定位消费）。
    /// S1 编号面：pub(crate)（numbering.rs 部件定位消费）。
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Relationship> {
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
/// S1 样式面：pub(crate)（style.rs 关系 target 归一复用）。
/// S1 编号面：pub(crate)（numbering.rs 关系 target 归一复用）。
pub(crate) fn normalize_part_path(path: &str) -> Option<String> {
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
/// S1 样式面：pub(crate)（style.rs 关系层复用）。
/// S1 编号面：pub(crate)（numbering.rs 关系层复用）。
pub(crate) fn build_rels_bundle<R: std::io::Read + std::io::Seek>(
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
    za: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
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
    /// #17：run 来自修订插入容器（`w:ins` 内 = 当前生效文本，正常
    /// 提取；标记面供消费方识别修订通道——如标书仓的澄清条款抽取）。
    pub revision_inserted: bool,
}

/// 段落编号承载（r4-t3 读面：numPr 透传 + 语义解析）。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphNumbering {
    /// 原 numId 透传（标识列表成员归属；写面回放**不使用原 numId**——
    /// 经自有抽象 [`ORDERED_NUM_ID`]/[`BULLET_NUM_ID`] 重定义，见
    /// [`ListKind`]）。
    pub num_id: usize,
    /// 原 ilvl 透传（0 基；OOXML 缺省 0——`w:ilvl` 缺席时按 0 承载；
    /// 读面不钳位，钳位属写面策略〔[`DocxWriter::list_item`] 0-8〕）。
    pub ilvl: usize,
    /// 语义解析结果（numId → num 桥 → abstractNum → 该 ilvl 级
    /// numFmt）：`"bullet"` → [`ListKind::Bullet`]；其余（decimal/
    /// lowerLetter/upperRoman/…）→ [`ListKind::Ordered`]。**None =
    /// 解析链不可达**（num 桥悬空〔严格拒/兼容 BrokenReference 警告〕
    /// 或抽象层级缺定义〔numStyleLink / ilvl 超出定义域——兼容
    /// BrokenReference 警告〕）——结构面（num_id/ilvl）仍承载。
    pub kind: Option<ListKind>,
    /// 原生 numFmt 值（解析成功时，如 `"decimal"`/`"bullet"`/
    /// `"lowerLetter"`/`"none"`）。写面回放经自有抽象时非
    /// `"decimal"`/`"bullet"` 的格式**降级**（有序恒 decimal outline、
    /// 无序恒 `•`）——此字段使降级**可诊断**（消费方可比对）。
    pub format: Option<String>,
}

/// 段落缩进承载（r4-t3 读面：`w:ind` 透传，单位 twips；Chars 字符
/// 单位变体登记不承载）。写面无任意 ind 面——仅经列表抽象
/// （[`DocxWriter::list_item_lines`] 悬挂几何）与引用抽象
/// （[`DocxWriter::quote_paragraph`] 步进缩进）间接表达（写入降级，
/// 登记见模块文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParagraphIndent {
    /// 左缩进（`w:ind/@w:left` 或 `@w:start`——引擎读面归一为 start）。
    pub left: Option<i32>,
    /// 右缩进（`w:ind/@w:right` 或 `@w:end`——引擎读面归一为 end）。
    pub right: Option<i32>,
    /// 首行特殊缩进（`@w:firstLine` / `@w:hanging`）。
    pub special: Option<ParagraphIndentSpecial>,
}

/// 首行特殊缩进变体（`w:ind` 的 `w:firstLine`/`w:hanging` 两形态，
/// twips 值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphIndentSpecial {
    /// 首行额外缩进（`w:firstLine`）。
    FirstLine(i32),
    /// 悬挂缩进（`w:hanging`——首行回退，续行起于 left）。
    Hanging(i32),
}

/// 段落：样式 ID 透传 + 标题层级解析 + run 序 + 编号/缩进承载（r4-t3）。
///
/// #316：`Default` 派生（BlockSource::Body 缺省——测试/直构面经
/// `..Default::default()` 构造，新增字段不再逐处同步）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParagraphData {
    /// 样式 ID（如 `Heading1`/`Normal`）——语义判定归消费方。
    pub style: Option<String>,
    /// #13（A1）：样式语义解析的标题层级（1-9 域，None = 非标题段）。
    ///
    /// 读面单一真源解析（仿 r4-t3 [`NumberingIndex`] 先例——样式表是
    /// docx 格式语义的一部分，语义解析归读面而非各消费方自猜 styleId）：
    /// 解析链 = ①段落级 `w:outlineLvl` 直接格式（0-8 → +1，直接格式
    /// 优先——Word 直接格式压样式）→ ②pStyle `Heading1`..`Heading9`
    /// 前缀（引擎写面/生成器惯用 styleId）→ ③样式名 heading 形
    /// （`heading N` 引擎助手 / `HeadingN` 紧凑形 / `标题 N`·`标题N`
    /// 中文形——WPS 等生成器；真 Word 中文版 styleId="1"-"9" +
    /// name="heading N" 双双覆盖）→ ④样式级 `w:outlineLvl`（0-8 → +1）。
    /// 越域值（outlineLvl 9+ / 层级 10+）防御忽略；钳 1-9。
    pub heading_level: Option<usize>,
    /// #14（A2）：本段被剥离的**表现型域**缓存结果计数（0 = 无剥离）。
    /// OOXML 域 = `fldChar begin` → 指令 → [`separate` → 缓存结果 runs]
    /// → `end`：表现型域（PAGE/NUMPAGES/DATE/…——值随渲染上下文漂移
    /// 的陈旧呈现数据，见 [`FieldMachine`] 域表）的缓存结果不入正文；
    /// 内容型域（REF/HYPERLINK/TOC/…——结果承载作者可见内容）保留。
    /// 诊断计数：导入面聚合进 `stats.fields_dropped`（降级显式化）。
    pub fields_dropped: usize,
    pub runs: Vec<RunData>,
    /// 段落编号（r4-t3 读面承载——None = 非列表段；`w:numId w:val="0"`
    /// 的 OOXML 去编号惯例同样 → None）。
    pub numbering: Option<ParagraphNumbering>,
    /// #266：解析序号（Word 语义计算——start/lvlRestart/numFmt/lvlText
    /// 全链渲染，如「一、」/（%2）→（一）/「1.»）。
    ///
    /// 读面后置遍历计算（[`resolve_block_numbers`]——文档序喂入
    /// [`crate::numbering::NumberingTracker`]）：有序列表段且定义链
    /// 可达时 `Some`；bullet/悬空 numId/numFmt=none = `None`（消费方
    /// 沿用 ListKind 既有降级面）。`list_level`/`list_id` 即
    /// [`Self::numbering`] 的 `ilvl`/`num_id`（透传面不重复承载）。
    pub resolved_number: Option<String>,
    /// 段落缩进（r4-t3 读面承载——None = 无 `w:ind`）。
    pub indent: Option<ParagraphIndent>,
    /// #19：本段引用的脚注 id 序（文档序——`w:footnoteReference`）。
    ///
    /// 引擎 run 模型不承载引用位（CAPABILITY_NOTES §5），读面文档级
    /// 第二趟标注（段落粒度——run 粒度需引擎模型承载，待上游）；表内
    /// 段引用位不标注（body 段落标记面）。尾注引用位不标注（内容面
    /// [`DocxDocument::endnotes`] 承载）。
    pub footnote_refs: Vec<i64>,
    /// 块来源（#22 P2-1）：本段所在文档位置——正文（含脚注/尾注/
    /// 批注等文档内容面）恒 `Body`；页眉/页脚部件内的段
    /// `Header`/`Footer`（读面提取时按部件定值）。
    pub source: BlockSource,
}

impl ParagraphData {
    /// 段落拼接文本（run 文本直连——不加空格，保真 run 边界）。
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// 块来源面（#22 P2-1）：块在文档中的位置标记——页眉/页脚内容
/// 接入导入输出后，消费方需要区分块源（正文流 vs 页面装置）。
///
/// 语义二分 = **文档内容** vs **页面装置**：`Body` 覆盖正文及文档
/// 附属内容面（脚注/尾注/批注——其承载通道独立于正文块序，语义
/// 上仍是文档内容而非页面装置）；`Header`/`Footer` 仅对应六槽位
/// 挂接部件（`HeaderFooterData::blocks` 内的块）。
///
/// #316：`Default`（Body 缺省）——[`ParagraphData`] 的 `Default`
/// 派生依赖项（加字段后旧字面量经 `..Default::default()` 免逐个
/// 跟随——#285② Anchor 加字段连环编译事故的同款预防）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockSource {
    #[default]
    Body,
    Header,
    Footer,
}

/// 文档块（有序）。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(ParagraphData),
    Table(TableData),
    /// #18：文本框（`w:txbxContent`——VML `v:textbox` 与 DrawingML
    /// `wps:txbx` 双形态）。内层块与正文同构（段落/表——嵌套承载）。
    TextBox(TextBoxData),
}

/// #18：文本框内容（内层块序——宿主段之后的文档序位置发射）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextBoxData {
    /// 文本框内块序（段落/表，经与正文同一提取机制——样式/域/修订
    /// 通道全承载）。
    pub blocks: Vec<Block>,
    /// 块来源（#22 P2-1）：与宿主块同源（文本框当前仅正文段承载——
    /// 页眉/页脚内浮动文本框读面未提取，登记面外）。
    pub source: BlockSource,
}

/// #19：批注（word/comments.xml + 正文锚关联）。
#[derive(Debug, Clone, PartialEq)]
pub struct CommentData {
    /// 批注 id（`w:comment/@w:id`——与正文锚 `commentRangeStart` 同源）。
    pub id: usize,
    /// 作者（`w:comment/@w:author`——缺省空串）。
    pub author: String,
    /// 日期（`w:comment/@w:date`——缺省空串）。
    pub date: String,
    /// 锚定正文块索引（`commentRangeStart` 首现块——最终块序数组
    /// 索引；跨块区间归起始块；None = 孤儿批注：comments.xml 在场而
    /// 正文无锚）。
    pub anchor_block_idx: Option<usize>,
    /// 批注内容块（段落/表——与正文同构提取）。
    pub blocks: Vec<Block>,
}

/// #19：脚注/尾注条目（word/footnotes.xml / word/endnotes.xml——引擎
/// 读面从不读，CAPABILITY_NOTES §5；读面文档级第二趟补提取）。
///
/// 分隔符条目（`w:type=separator`/`continuationSeparator`）与 `id < 1`
/// 条目不入流（呈现辅助件非内容）；id 与正文 `w:footnoteReference`/
/// `w:endnoteReference` 对偶。
#[derive(Debug, Clone, PartialEq)]
pub struct FootnoteData {
    /// 脚注/尾注 id（`w:footnote/@w:id`——分隔符惯例 -1/0 已滤）。
    pub id: i64,
    /// 脚注/尾注内容块（段落/表——与正文同构提取）。
    pub blocks: Vec<Block>,
}

/// 文本表格：**栅格展开形**（#22 P2-2）——行 × 栅格列；gridSpan 覆盖
/// 列与 vMerge 续格置空（内容只出现于原点格——Word 显示语义），
/// 丢失的合并结构由 [`TableData::merges`] 显式承载（与 xlsx 侧
/// `SheetData`「值栅格 + merges 清单」同构——引擎侧两容器面统一）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableData {
    pub rows: Vec<Vec<String>>,
    /// 合并区域清单（#22 P2-2）：栅格坐标 0 基、含端点——每区域
    /// 一条矩形对角记录。`rows` 已展开为栅格（覆盖格空串），消费方
    /// 渲染零重建；GFM 无合并面的降级可诊断性由本清单承载。
    pub merges: Vec<MergeRange>,
    /// 块来源（#22 P2-1）：表所在文档位置——正文 `Body` / 页眉
    /// `Header` / 页脚 `Footer`（表内段落继承表来源）。
    pub source: BlockSource,
    /// #39：内嵌表标记（`w:tbl` 嵌于外层表 `w:tc`，depth≥2 时本层
    /// 读回的内层表）。false = 文档直系表（既有全部表）；true = 内层
    /// 表——消费方可分流（标书侧费率表等实质性条款区识别面）。
    /// 缺省语义不可依赖 serde（本类型无 serde 面——构造点显式赋值）。
    pub nested: bool,
}

/// 表格合并区域（#22 P2-2）：`start`/`end` 为栅格矩形对角（0 基、
/// 含端点）。三个面：纵向合并（vMerge restart-continue 链）、横向
/// 合并（gridSpan > 1）、块合并（两者叠加——单条矩形，不拆两条）。
/// 单行单列的 vMerge restart（标了续格但无人续）**不是合并**（不发
/// 区域）；单行横向 gridSpan 仍发（跨列即结构）。与 xlsx 侧
/// `SheetData::merges`（calamine `Dimensions`，绝对坐标 u32）对齐的
/// 是**语义面**（矩形清单 + 值栅格）；docx 侧为文档内相对坐标
/// `usize`（无绝对锚定概念）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
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

/// 分节符行为类型（`w:sectPr/w:type`——**本节末分节符**的换页语义；
/// 缺省 = NextPage。读面透传，激活/渲染语义归消费方）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    NextPage,
    NextColumn,
    Continuous,
    EvenPage,
    OddPage,
}

/// 节属性来源（#257）：中段节（`w:p/w:pPr/w:sectPr`——承载段落为
/// 本节末段）/ 末节（body 级 `w:sectPr`——文档收尾，无承载段落）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionSource {
    Mid,
    Final,
}

/// 文档节（#257 多节分节符读写面）：每节的几何/槽位/块区间。
///
/// 读面单一真源：`DocxDocument.sections` **恒 ≥1 条**（单节文档 =
/// 1 条末节）——节序 = 文档序，`block_range` 连续切分 `blocks`
/// 全序（节 i 的 end = 节 i+1 的 start；末节 end = `blocks.len()`）。
///
/// 中段节页眉/页脚部件由引擎第二趟自解析（docx-rs `read_docx` 只把
/// 内容联接到末节——`read_docx.rs:232-290`；中段节引用在、内容不
/// 联接，P1-1 静默丢的根因）；末节槽位与 [`DocxDocument::headers`]/
/// [`DocxDocument::footers`] 同源（既有单一真源，本表复制一份供
/// 逐节统一访问）。
#[derive(Debug, Clone, PartialEq)]
pub struct SectionData {
    /// 节序（0 起，文档序）。
    pub index: usize,
    /// 属性来源（中段节 / 末节）。
    pub source: SectionSource,
    /// 页面几何（pgSz/pgMar，twips——与写面 [`PageGeometry`] 同域，
    /// 读模板 → 写导出零换算）。
    pub geometry: PageGeometry,
    /// 首页不同（`w:titlePg`——first 槽位的激活位，语义解释归消费方）。
    pub title_page: bool,
    /// 分栏数（`w:cols/@w:num`；缺省 1）。#268 修正：docx-rs 0.4.22
    /// 读面**不解析 w:cols**（`SectionProperty::read` 无 cols 分支，
    /// `columns` 恒缺省 1——#257 遗留静默面）；本字段改由 office-io
    /// 自有 XML 第二趟（[`parse_columns_specs`]）真源派生。
    pub columns: usize,
    /// #268：分栏规格全字段（`w:cols`——num/space/equalWidth/sep/
    /// `w:col` 子表；None = 本节 `w:cols` 元素缺席[单栏缺省形态]。
    /// docx-rs 0.4.22 只携 num/space 两字段（equalWidth/sep/col 子表
    /// 缺失）——本面由 office-io 自有 quick-xml 第二趟补齐）。
    pub columns_spec: Option<ColumnsSpec>,
    /// #268：显式分栏符块（`w:br w:type="column"`——本节内含该符的
    /// 段落块全局索引，升序；含符块内符前文本在原栏、符后文本入
    /// 下栏[块级归属取块起始栏——粗粒度登记]；空 = 无显式分栏
    /// [栏平衡是排版期行为，静态读面不可预知]）。
    pub column_break_blocks: Vec<usize>,
    /// 分节符类型（`w:type`；缺省 None = NextPage 语义）。
    pub section_type: Option<SectionKind>,
    /// 起始页码（`w:pgNumType/@w:start`；None = 续前节）。
    pub page_number_start: Option<u32>,
    /// 本节页眉（六槽位中实际挂接的——变体 + 块序，同文档级口径）。
    pub headers: Vec<HeaderFooterData>,
    /// 本节页脚（同上）。
    pub footers: Vec<HeaderFooterData>,
    /// 本节覆盖的块区间 `[start, end)`（`DocxDocument::blocks` 索引；
    /// 中段节 end = 承载 sectPr 的段落块之后的边界）。
    pub block_range: (usize, usize),
}

impl SectionData {
    /// 块的栏号（0 起，#268 文本流分栏归属）：基于**显式分栏符**的
    /// 确定性归属——块前显式分栏符计数（含符块自身取其起始栏）；
    /// 钉制到 `columns-1`（多于栏数的分栏符由排版期流到下页——静态
    /// 读面钳制登记）。无显式分栏 = 恒 0（自动栏平衡是排版期行为，
    /// 静态读面不可预知——诚实降级，不猜）。`block_idx` 不在本节
    /// `block_range` 内 → None。
    pub fn column_of(&self, block_idx: usize) -> Option<usize> {
        let (start, end) = self.block_range;
        if block_idx < start || block_idx >= end {
            return None;
        }
        let num = self.columns.max(1);
        let prior = self
            .column_break_blocks
            .iter()
            .filter(|&&b| b < block_idx)
            .count();
        Some(prior.min(num - 1))
    }
}

/// 单栏宽（#268：`w:cols/w:col` 子元素——`@w:w`/`@w:space`，twips；
/// `equalWidth=0` 形态下承载各栏宽度/间距，缺省属性 → None 不代填）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColumnWidth {
    /// 栏宽（`@w:w`，twips）。
    pub width: Option<u32>,
    /// 与右邻栏间距（`@w:space`，twips）。
    pub space: Option<u32>,
}

/// 分栏规格（#268：`sectPr/w:cols` 全字段）。
///
/// 缺省语义（ECMA-376 §17.6.4）：num 缺省 1；space 缺省 720（twips，
/// 不代填——消费方按需取缺省）；equalWidth 缺省 true；sep 缺省
/// false。`w:col` 子表仅在 equalWidth=false 形态有意义（等宽形态下
/// 子表在场也透传——读面不解释）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnsSpec {
    /// 栏数（`@w:num`）。
    pub num: Option<u32>,
    /// 栏间距（`@w:space`，twips）。
    pub space: Option<u32>,
    /// 等宽栏（`@w:equalWidth`——缺席 = true 语义，此处 None 区分）。
    pub equal_width: Option<bool>,
    /// 栏间分隔线（`@w:sep`——缺席 = false 语义）。
    pub separator: Option<bool>,
    /// 各栏宽子表（`w:col` 子元素，文档序）。
    pub columns: Vec<ColumnWidth>,
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
    /// #19：批注（word/comments.xml——锚关联最终块序；孤儿批注
    /// anchor_block_idx = None）。
    pub comments: Vec<CommentData>,
    /// #19：脚注（word/footnotes.xml——分隔符条目已滤）。
    pub footnotes: Vec<FootnoteData>,
    /// #19：尾注（word/endnotes.xml——分隔符条目已滤）。
    pub endnotes: Vec<FootnoteData>,
    /// #257：文档节（多节分节符读写面——恒 ≥1 条，节序 = 文档序；
    /// 单节文档 = 1 条末节。中段节页眉页脚此前静默丢（审计 P1-1），
    /// 本面补齐）。
    pub sections: Vec<SectionData>,
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
///
/// r4-t2：**输入字节上限前置**——`fs::read` 全量入内存之前先查元数据
/// 尺寸（此前无前置检查 = 真实内存面：垃圾前缀大文件全量读入）；
/// docx 族声明总量限 = 50MB（经 [`io::prescan_docx`]，此前误用 xlsx
/// 族 300MB 常量）。
pub fn read_docx_with_options(path: &str, opts: &ReadOptions) -> Result<DocxDocument, DocxError> {
    let meta = std::fs::metadata(path).map_err(|e| DocxError::Io(format!("stat {path}: {e}")))?;
    if meta.len() > io::LIMIT_INPUT_BYTES_DOCX {
        return Err(DocxError::Container(io::ReadError::PrescanRejected {
            reason: "input size over limit",
            entry: None,
        }));
    }
    let bytes = std::fs::read(path).map_err(|e| DocxError::Io(format!("read {path}: {e}")))?;
    read_bytes_gated(&bytes, opts)
}

/// 读 DOCX 字节（容器三门同样适用——字节入口不绕过输入防线）。
pub fn read_docx_from_bytes(bytes: &[u8]) -> Result<DocxDocument, DocxError> {
    read_docx_from_bytes_with_options(bytes, &ReadOptions::default())
}

/// 读 DOCX 字节（带选项）。r4-t2：入参字节上限 + **全借用化**
///（此前 `bytes.to_vec()` 复制一份——大档双倍峰值，去除）。
pub fn read_docx_from_bytes_with_options(
    bytes: &[u8],
    opts: &ReadOptions,
) -> Result<DocxDocument, DocxError> {
    if bytes.len() as u64 > io::LIMIT_INPUT_BYTES_DOCX {
        return Err(DocxError::Container(io::ReadError::PrescanRejected {
            reason: "input size over limit",
            entry: None,
        }));
    }
    read_bytes_gated(bytes, opts)
}

/// 门结构：门 0（输入字节上限——r4-t2 前置于任何全量读/分配）→
/// 门 1（prescan——**docx 族限额** [`io::prescan_docx`]，r4-t2 族分化）
/// → 门 2+3 融合趟（尺寸 + 部件良构——复用 XLSX 读路径同一实现）
/// → **自有 OPC 关系层**（先于引擎——上限两模式恒拒；词汇表/schema
/// 严格拒兼容降级）→ [Content_Types].xml 解析 + 媒体清单 → docx-rs
/// 模型解析 → 关系联接提取 + run 级图片引用计数。
fn read_bytes_gated(bytes: &[u8], opts: &ReadOptions) -> Result<DocxDocument, DocxError> {
    let strict = !opts.compat;
    // #234：OLE2 魔数前置（异源 .doc 改名 .docx 的真实形态）——先于
    // zip 装载识别，不再误报 EOCD 容器损坏（可操作指引随错误变体）。
    if io::is_ole2(bytes) {
        return Err(DocxError::Container(io::ReadError::LegacyOfficeFormat {
            expect: "docx",
        }));
    }
    let cursor = std::io::Cursor::new(bytes);
    let mut za = zip::ZipArchive::new(cursor)
        .map_err(|e| DocxError::Container(io::ReadError::ContainerCorrupt(format!("{e}"))))?;
    // #233：EOCD 守恒门（zip-exotica 防线）——crate 吸收物理同名条目
    // 使 by_index 视野失明，守恒比对在 prescan 之前。
    io::eocd_conservation(bytes, &za)?;
    io::prescan_docx(&mut za)?;
    let mut warnings = Vec::new();
    // 门 2+3 融合趟（io 私有实现经公开管道函数复用——见 io::read_book）。
    io::verify_gates(&mut za, strict, &mut warnings)?;
    drop(za);

    // OPC 关系层 + 内容类型 + 媒体清单（同一门后字节——零门绕过）。
    let (bundle, images, document_xml, footnotes_xml, endnotes_xml, numbering_xml) = {
        let cursor = std::io::Cursor::new(bytes);
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
        // #18：document.xml 原始字节（VML 文本框第二趟——引擎读面
        // 不承载 v:pict→v:textbox 形态，文档级补提取）。
        let mut document_xml = String::new();
        if let Ok(mut zf) = za.by_name("word/document.xml") {
            let _ = zf.read_to_string(&mut document_xml);
        }
        // #19：脚注/尾注部件原始字节（引擎读面从不读——CAPABILITY_NOTES
        // §5；文档级第二趟补提取。部件缺席 = 空串快路径）。
        let mut footnotes_xml = String::new();
        if let Ok(mut zf) = za.by_name("word/footnotes.xml") {
            let _ = zf.read_to_string(&mut footnotes_xml);
        }
        let mut endnotes_xml = String::new();
        if let Ok(mut zf) = za.by_name("word/endnotes.xml") {
            let _ = zf.read_to_string(&mut endnotes_xml);
        }
        // #266：numbering.xml 原始字节（定义面全承载解析——引擎
        // docx-rs 的 Start/LevelText/LevelRestart 字段私有不可达，
        // 见 numbering.rs 模块文档；无部件 = 空串快路径）。
        let mut numbering_xml = String::new();
        if let Ok(mut zf) = za.by_name("word/numbering.xml") {
            let _ = zf.read_to_string(&mut numbering_xml);
        }
        (
            bundle,
            images,
            document_xml,
            footnotes_xml,
            endnotes_xml,
            numbering_xml,
        )
    };

    // #226 R2：docx-rs 读层 panic 预检防御——真实语料 4 处实锤（裸 run
    // 属性零属性 start tag + w:w 通用度量）在依赖层 panic（wasm = trap /
    // GUI = 进程崩溃）；引擎侧先修后读（scan-and-repair，修复走 Warning
    // 通道——详见 docx_panic_guard 模块文档）。
    let engine_bytes = crate::docx_panic_guard::guard_docx_bytes(bytes, &mut warnings)?;
    let docx =
        docx_rs::read_docx(engine_bytes.as_ref()).map_err(|e| DocxError::Parse(format!("{e}")))?;
    // r4-t3：numbering.xml 解析索引（numId → ilvl 级 numFmt——引擎
    // 读面 numberings 模型全链 pub，见 NumberingIndex 文档）。
    let numbering_index = NumberingIndex::build(&docx.numberings);
    // #13（A1）：styles.xml 语义索引（styleId → 标题语义——引擎读面
    // styles 模型 pub，见 StyleIndex 文档）。
    let style_index = StyleIndex::build(&docx.styles);
    // #19：脚注引用位扫描（w:footnoteReference——引擎 run 模型不
    // 承载，文档级第二趟；body 块索引 → id 序）。
    let note_refs = scan_note_references(&document_xml);
    let (blocks, comment_anchors, section_marks, column_break_blocks) = extract_blocks(
        &docx,
        &bundle,
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
        &note_refs,
    )?;
    // #257：节边界分离（apply_vml_textboxes 插入位移同步重映射）。
    let mut section_boundaries: Vec<usize> = section_marks.iter().map(|(b, _)| *b).collect();
    // #18：VML 文本框插入（宿主块后——DrawingML 形态已在
    // extract_blocks 模型面发射，mc:Fallback 副本防双取）。
    let blocks = apply_vml_textboxes(
        blocks,
        &document_xml,
        &bundle,
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
        &mut section_boundaries,
    )?;
    // #345（#318 D2）：表内 DrawingML 文本框插入（宿主表块跨度后——
    // tc 容器内 wps:txbx 形态：模型面单元格路径只取段落文本，该子树
    // 静默丢，此趟补提取；VML 表内形态仍由上一步既有通道承载，两趟
    // 捕获条件互补不重叠）。
    let mut blocks = apply_tc_textboxes(
        blocks,
        &document_xml,
        &bundle,
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
        &mut section_boundaries,
    )?;
    let (mut headers, mut footers) = extract_headers_footers(
        &docx,
        &bundle,
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
    )?;
    // #257：文档节全量构建（中段节标记 + 末节；中段节页眉页脚部件
    // 引擎第二趟自解析——docx-rs read_docx 只联末节）。
    let sections = build_sections(
        &docx,
        &section_marks,
        &section_boundaries,
        blocks.len(),
        bytes,
        &bundle,
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
        &headers,
        &footers,
        &document_xml,
        &column_break_blocks,
    )?;
    // #19：批注面（模型面 comments + 锚关联——VML 插入后的最终块序
    // lockstep 重 walk 解析数组索引）。
    let comments = extract_comments(
        &docx,
        &comment_anchors,
        &blocks,
        bundle.document.as_ref(),
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
    )?;
    // #19：脚注/尾注部件第二趟（分隔符条目已滤；部件缺席零成本）。
    let footnotes = parse_notes_part(
        &footnotes_xml,
        "footnote",
        "word/footnotes.xml",
        bundle.document.as_ref(),
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
    )?;
    let endnotes = parse_notes_part(
        &endnotes_xml,
        "endnote",
        "word/endnotes.xml",
        bundle.document.as_ref(),
        &numbering_index,
        &style_index,
        strict,
        &mut warnings,
    )?;
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
    // #266：多级列表自动编号——定义面全承载解析（自有 quick-xml 层）
    // + 文档序序号计算（正文 + 页眉/页脚各自独立计数——Word 每个
    // story 独立语义）。脚注/尾注的编号段维持 None（v1 边界，登记）。
    {
        let mut table = crate::numbering::NumberingTable::default();
        if !numbering_xml.is_empty() {
            table = crate::numbering::parse_numbering_part(
                numbering_xml.as_bytes(),
                "word/numbering.xml",
                strict,
                &mut warnings,
            )?;
        }
        let mut tracker = crate::numbering::NumberingTracker::new(&table);
        resolve_block_numbers(&mut blocks, &mut tracker);
        for hf in headers.iter_mut().chain(footers.iter_mut()) {
            let mut part_tracker = crate::numbering::NumberingTracker::new(&table);
            resolve_block_numbers(&mut hf.blocks, &mut part_tracker);
        }
    }

    Ok(DocxDocument {
        blocks,
        headers,
        footers,
        images,
        comments,
        footnotes,
        endnotes,
        sections,
        warnings,
    })
}

/// #266：块序编号解析（后置遍历——文档序喂入 [`NumberingTracker`]，
/// 有序段计算 resolved_number）。
///
/// - 递归面：正文块序 + 文本框内块序（TextBox 发射在宿主段之后的
///   文档序位置——顺序正确）；表格单元是纯文本承载（r4-t3 面），
///   无编号段不入遍历。
/// - 计算面：仅 `kind == Ordered` 段（bullet 走 ListKind 既有圆点面；
///   悬空 numId/超域 ilvl → feed 返回 None 维持降级）。
/// - 空渲染（numFmt=none）不落值——`Some("")` 无消费语义。
fn resolve_block_numbers(blocks: &mut [Block], tracker: &mut crate::numbering::NumberingTracker) {
    for block in blocks.iter_mut() {
        match block {
            Block::Paragraph(p) => {
                let Some(num) = &p.numbering else { continue };
                if num.kind != Some(ListKind::Ordered) {
                    continue;
                }
                if let Some(item) = tracker.feed(num.num_id, num.ilvl) {
                    if !item.text.is_empty() {
                        p.resolved_number = Some(item.text);
                    }
                }
            }
            Block::TextBox(tb) => resolve_block_numbers(&mut tb.blocks, tracker),
            Block::Table(_) => {}
        }
    }
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
            // #276（审计 B-1）：外部超链接读面消毒——scheme 白名单
            // （mdc-office url_policy，#276 首个生产接线）。敌意
            // `javascript:`/`vbscript:`/`data:` scheme 此前直转
            // `[text](javascript:...)` GFM 进编辑器（docx_import
            // link_destination 消费面）；拒绝降级 Unresolved（下游
            // 链接渲染降级纯文本）+ UnsafeLink 警告，不废整个文档。
            // 白名单语义 = http/https/mailto + 无 scheme 相对路径
            // （file:/ftp: 等罕见 scheme 一并降级——警告文案可诊断，
            // 放行清单调整属 Lead 裁定面）。
            match mdc_office::sanitize::check_url(&rel.target) {
                Ok(()) => HyperlinkTarget::External {
                    target: rel.target.clone(),
                },
                Err(e) => {
                    warnings.push(CompatibilityWarning {
                        part: part.to_string(),
                        kind: WarningKind::UnsafeLink,
                        detail: format!(
                            "hyperlink target rejected by url policy ({e}): {}",
                            rel.target
                        ),
                    });
                    HyperlinkTarget::Unresolved {
                        rid: rid.to_string(),
                    }
                }
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
/// #13（A1）：样式表索引（styles.xml 语义解析——styleId → 标题语义）。
/// 仿 [`NumberingIndex`] 先例：格式语义（样式名/outlineLvl）在读面
/// 解析为单一真源，消费方（docx_import）不再各自猜 styleId。位于
/// [`extract_blocks`] 前定义以就近服务提取层。
#[derive(Debug, Default)]
struct StyleIndex {
    /// styleId → (样式名 heading 层级 1-9, 样式级 outlineLvl 0-8)。
    /// 仅收有标题语义的样式（两链皆无的不入表——查询 miss = 非标题，
    /// 不误判非标题样式）。
    by_id: HashMap<String, (Option<usize>, Option<usize>)>,
}

impl StyleIndex {
    fn build(styles: &docx_rs::Styles) -> Self {
        let mut by_id = HashMap::new();
        for s in &styles.styles {
            let name_level = heading_level_from_name(&s.name);
            let outline = s.paragraph_property.outline_lvl.as_ref().map(|o| o.v);
            if name_level.is_some() || outline.is_some() {
                by_id.insert(s.style_id.clone(), (name_level, outline));
            }
        }
        StyleIndex { by_id }
    }
}

/// 样式名 → 标题层级（1-9）：引擎 [`docx_rs::Name`] 助手（大小写不
/// 敏感 `heading` 前缀 + `get_heading_number` 空格形数字）为主，
/// 紧凑形（`HeadingN`——无空格，`get_heading_number` 的 replace
/// 不命中面）与中文形（`标题 N`/`标题N`——WPS 等生成器）走
/// `starts_with` 探针（`Name` 原始串私有，公开面仅此——9 级恒定
/// 前缀集，实际 O(1)）。
fn heading_level_from_name(name: &docx_rs::Name) -> Option<usize> {
    if name.is_heading() {
        if let Some(n) = name.get_heading_number().filter(|&n| (1..=9).contains(&n)) {
            return Some(n);
        }
        // 紧凑形（"Heading1"）：大小写两规范形各探一次。
        for n in 1..=9 {
            if name.starts_with(&format!("Heading{n}")) || name.starts_with(&format!("heading{n}"))
            {
                return Some(n);
            }
        }
        return None; // `heading` 前缀但无数字（如 "Heading"）→ 非层级标题
    }
    // 中文形：标题 N / 标题N。
    for n in 1..=9 {
        if name.starts_with(&format!("标题 {n}")) || name.starts_with(&format!("标题{n}")) {
            return Some(n);
        }
    }
    None
}

/// pStyle 值 → 标题层级（`Heading1`..`Heading9` 前缀——引擎写面发射
/// 形；1-9 域外 None）。原 docx_import 面的同名逻辑收归读面单一真源。
fn heading_level_from_style_id(style_id: &str) -> Option<usize> {
    let rest = style_id.strip_prefix("Heading")?;
    let n: usize = rest.parse().ok()?;
    (1..=9).contains(&n).then_some(n)
}

/// #14（A2）：域机制 run 级状态机。
///
/// OOXML 域 = `fldChar begin` → `instrText`（指令）→ [`fldChar
/// separate` → 缓存结果 runs] → `fldChar end`。指令文本恒不入正文
///（既有行为——非文本源）；缓存结果按域类型分派：
///
/// | 分派 | 域 | 理由 |
///|---|---|---|
///| **剥离**（表现型） | PAGE / NUMPAGES / SECTIONPAGES / PAGEREF /
/// DATE / TIME / PRINTDATE / CREATEDATE / SAVEDATE / EDITTIME /
/// REVNUM / FILENAME / FILESIZE / TEMPLATE / APPVERSION / SECTION |
/// 值随渲染上下文漂移（页码/日期/时长/文件元数据）——缓存结果是
/// 保存时快照，转换后必陈旧（真 Word 夹具的「11.1 表格」即 PAGE
/// 域缓存结果 "1" 前缀拼接——TR-2D batch2 P1-1 真根因）；PAGEREF
/// 同理：markdown 无分页，页码恒失真 |
///| **保留**（内容型） | REF / HYPERLINK / MERGEFIELD / TOC / SEQ /
/// DOCVARIABLE / QUOTE / STYLEREF / AUTHOR / TITLE / 未知域 |
/// 结果承载作者可见内容（交叉引用文本/链接显示文本/目录内容/图表
///序号/文档属性）——宁多勿丢，保守保留 |
///
/// 嵌套域（域中域——OOXML 允许，罕见）：Begin 重置活动域状态
///（外层域的 separate/end 由内层 End 后的首个匹配态收束——v0
/// 近似，登记）。
///
/// 退化域（begin → instrText → end，无 separate 无缓存结果——WPS
/// 12.1 常见发射形，指令可为 `""` 前缀的任意不可解析串）：指令文本
/// 恒不入正文且**不计 dropped**（指令从未可见，非损失面）——四实现
/// 面共识裁决与证据链 = docs/field-code-semantics.md（#40）；钉定
/// 测试 = tests/tr31_degenerate_field.rs。字面 `w:t` 文本永不因引号/
/// 内容形态剥离（剥离按域结果区**位置**判定）。
#[derive(Default)]
struct FieldMachine {
    /// 活动域指令累积（begin 后收集，separate 时定格判定）。
    instr: String,
    /// separate 已见（缓存结果区）。
    in_result: bool,
    /// 活动域为表现型（缓存结果将剥离）。
    volatile: bool,
    /// 已剥离的表现型域计数（段落级透出 → stats.fields_dropped）。
    dropped: usize,
}

impl FieldMachine {
    /// 域指令首词 → 表现型判定（指令含开关参数如
    /// ` PAGE \* MERGEFORMAT `——首词切分；大小写不敏感：Word 发射
    /// 大写、手写文件常见小写）。
    fn is_volatile(instr: &str) -> bool {
        let head = instr
            .trim_start()
            .split([' ', '\\', '\t', '\u{a0}'])
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        matches!(
            head.as_str(),
            "PAGE"
                | "NUMPAGES"
                | "SECTIONPAGES"
                | "PAGEREF"
                | "DATE"
                | "TIME"
                | "PRINTDATE"
                | "CREATEDATE"
                | "SAVEDATE"
                | "EDITTIME"
                | "REVNUM"
                | "FILENAME"
                | "FILESIZE"
                | "TEMPLATE"
                | "APPVERSION"
                | "SECTION"
        )
    }

    /// fldChar 状态转移。Begin 重置（嵌套近似）；Separate 定格
    /// 表现型判定并进入结果区；End 收束（表现型且含结果区 → 计数）。
    fn transition(&mut self, ty: &docx_rs::FieldCharType) {
        use docx_rs::FieldCharType;
        match ty {
            FieldCharType::Begin => {
                self.instr.clear();
                self.in_result = false;
                self.volatile = false;
            }
            FieldCharType::Separate => {
                self.in_result = true;
                self.volatile = Self::is_volatile(&self.instr);
            }
            FieldCharType::End => {
                if self.in_result && self.volatile {
                    self.dropped += 1;
                }
                self.instr.clear();
                self.in_result = false;
                self.volatile = false;
            }
            FieldCharType::Unsupported => {}
        }
    }

    /// 当前文本是否剥离（表现型域的缓存结果区）。
    fn suppress(&self) -> bool {
        self.in_result && self.volatile
    }
}

/// 段落标题层级全链解析（见 [`ParagraphData::heading_level`] 文档——
/// 直接 outlineLvl > pStyle 前缀 > 样式名 > 样式 outlineLvl）。
/// outlineLvl 0-8 域（+1 → 1-9）；9+ 越域防御忽略。
fn resolve_heading_level(
    direct_outline: Option<usize>,
    style_id: Option<&str>,
    styles: &StyleIndex,
) -> Option<usize> {
    if let Some(o) = direct_outline {
        if o <= 8 {
            return Some((o + 1).min(9));
        }
    }
    if let Some(level) = style_id.and_then(heading_level_from_style_id) {
        return Some(level);
    }
    if let Some(id) = style_id {
        if let Some((name_level, outline)) = styles.by_id.get(id) {
            if let Some(n) = name_level {
                return Some(*n);
            }
            if let Some(o) = outline {
                if *o <= 8 {
                    return Some((*o + 1).min(9));
                }
            }
        }
    }
    None
}

/// #18：段落内 DrawingML 文本框内容子序（w:drawing → DrawingData::
/// TextBox——run 级 + 超链接内 run 级两路收集）。
fn paragraph_text_box_children(p: &docx_rs::Paragraph) -> Vec<&Vec<docx_rs::TextBoxContentChild>> {
    fn walk_run<'a>(r: &'a docx_rs::Run, out: &mut Vec<&'a Vec<docx_rs::TextBoxContentChild>>) {
        for c in &r.children {
            if let docx_rs::RunChild::Drawing(d) = c {
                if let Some(docx_rs::DrawingData::TextBox(tb)) = &d.data {
                    out.push(&tb.children);
                }
            }
        }
    }
    let mut out = Vec::new();
    for child in &p.children {
        match child {
            docx_rs::ParagraphChild::Run(r) => walk_run(r, &mut out),
            docx_rs::ParagraphChild::Hyperlink(h) => {
                for c in &h.children {
                    if let docx_rs::ParagraphChild::Run(r) = c {
                        walk_run(r, &mut out);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// #18：文本框内层块提取（与正文同一机制——样式索引/域状态机/修订
/// 通道/超链接联接全承载；TextBoxContentChild 段落/表两型）。
#[allow(clippy::too_many_arguments)]
fn text_box_blocks(
    children: &[docx_rs::TextBoxContentChild],
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    part: &str,
    source: BlockSource,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<Block>, io::ReadError> {
    let mut out = Vec::new();
    for c in children {
        match c {
            docx_rs::TextBoxContentChild::Paragraph(p) => out.push(Block::Paragraph(
                extract_paragraph(p, table, index, styles, part, source, strict, warnings)?,
            )),
            docx_rs::TextBoxContentChild::Table(t) => {
                // #39：嵌套表沿文档序独立成块（extract_table 返回
                // [本表, 内层表…]）。
                for td in extract_table(t, table, index, styles, part, source, strict, warnings)? {
                    out.push(Block::Table(td));
                }
            }
        }
    }
    Ok(out)
}

/// #18：VML 文本框第二趟扫描（`w:pict → v:shape → v:textbox →
/// w:txbxContent`——引擎读面不承载该形态，文档级补提取）。
///
/// 返回 (宿主块索引, txbxContent 原始 XML 区段) 序列（文档序）。
/// 宿主块索引 = 区段所在顶层块（w:p/w:tbl 的 body 直接子层序——
/// 与 extract_blocks 的块序一一对应；捕获态下顶层计数冻结——
/// txbxContent 内层 w:p 非顶层）。两类区域跳过：
/// · **`w:drawing` 子树内**（DrawingML `wps:txbx` 形态——模型面
///   extract_blocks 已发射，第二趟再取 = 双提取）；
/// · **mc:Fallback 内**（AlternateContent 双写形态：DrawingML 主选
///   已由模型面提取，Fallback 的 VML 副本再取 = 双提取）。
fn scan_vml_textbox_regions(xml: &str) -> Vec<(usize, String)> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth: usize = 0;
    let mut body_child_depth: Option<usize> = None;
    let mut block_idx: usize = 0;
    let mut in_fallback: usize = 0;
    // DrawingML 子树深度（`w:drawing → wps:txbx → w:txbxContent`——
    // 模型面领地，第二趟不碰）。
    let mut in_drawing: usize = 0;
    // 外层 txbxContent 捕获态（嵌套文本框并入外层区段——内层经
    // TextBoxContent::read 的段落解析自然展开，边界登记）。
    let mut capture: Option<(usize, usize)> = None; // (host, txbx 嵌套深度)
    let mut span_start = 0usize;
    let mut out: Vec<(usize, String)> = Vec::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "body" => body_child_depth = Some(depth + 1),
                    "Fallback" => in_fallback += 1,
                    "drawing" => in_drawing += 1,
                    "txbxContent" => {
                        if in_fallback == 0 && in_drawing == 0 && capture.is_none() {
                            // 宿主 = 最近一个已计数的顶层块（承载 pict 的段）。
                            capture = Some((block_idx.saturating_sub(1), 1));
                            span_start = pos_before;
                        } else if let Some((_, d)) = capture.as_mut() {
                            *d += 1;
                        }
                    }
                    "p" | "tbl"
                        // 顶层块计数：Start 时 depth = 该元素自身层（body
                        // 直接子层 = body_child_depth）——depth 在 match 后
                        // 才自增，此处比 body_child_depth 本值（非 +1）。
                        if capture.is_none() && in_fallback == 0 && body_child_depth == Some(depth)
                        => {
                            block_idx += 1;
                        }
                    _ => {}
                }
                depth += 1;
            }
            // 自闭合 `<w:p/>`（空段——真 Word 形态）：与 Start+End 等
            // 价计一个顶层块（depth 不变）；自闭合 txbxContent 无内容
            // 不捕获。
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if (name == "p" || name == "tbl")
                    && capture.is_none()
                    && in_fallback == 0
                    && body_child_depth == Some(depth)
                {
                    block_idx += 1;
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "Fallback" => in_fallback = in_fallback.saturating_sub(1),
                    "drawing" => in_drawing = in_drawing.saturating_sub(1),
                    "txbxContent" => {
                        if let Some((host, d)) = capture.as_mut() {
                            *d -= 1;
                            if *d == 0 {
                                let host = *host;
                                capture = None;
                                let span_end = reader.buffer_position() as usize;
                                // quick-xml 0.41 buffer_position 边界计量偏移（批 4
                                // 体检实测：区段起点可含前一事件尾字节/前导空白，
                                // 终点可截断闭合标签中段——残缺闭合标签会令
                                // docx-rs TextBoxContent::read 在 EOF 上死循环）。
                                // 双端前向修复：起点走到下一个 '<'（= txbxContent
                                // 自身开标签——XML 属性值不可含裸 '<'）；终点若
                                // 不在标签边界则补到下一个 '>' 含端点（欠报修复；
                                // 过报多带尾部字节对解析无害——read 于正确闭合
                                // 事件返回）。
                                let bytes = xml.as_bytes();
                                let mut s = span_start.min(xml.len());
                                while s < xml.len() && bytes[s] != b'<' {
                                    s += 1;
                                }
                                let mut e = span_end.min(xml.len());
                                if e == 0 || bytes[e - 1] != b'>' {
                                    while e < xml.len() && bytes[e] != b'>' {
                                        e += 1;
                                    }
                                    if e < xml.len() {
                                        e += 1;
                                    }
                                }
                                if s < e {
                                    out.push((host, xml[s..e].to_string()));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break, // 门 2+3 已过结构校验（严格）；兼容面防御终止
        }
    }
    out
}

/// #345（#318 D2）：表内 DrawingML 文本框第二趟扫描——`w:tc` 容器内
/// `w:drawing > wps:txbx > w:txbxContent`（裸 drawing 或
/// mc:AlternateContent 的 Choice 主选形态）。
///
/// 机制：单元格内容走 `TableCellContent::Paragraph` 文本扁平化
/// （`extract_table_inner`），docx-rs 段落模型的 DrawingML 文本框子树
/// 在该路径无承载 → 表内框的结构与内容双丢（费率表等条款区常见形
/// 态）。本趟与 `scan_vml_textbox_regions` 同构（quick-xml local-name
/// 通道，命名空间无关——strict OOXML 天然兼容），捕获条件互补不重叠。
///
/// 返回 (宿主块索引, txbxContent 原始 XML 区段) 序列（文档序）。
/// 宿主块索引 = 区段所在顶层块（承载 w:tbl 的 body 直接子层序——
/// 与既有第二趟宿主语义同构；发射位由 `apply_tc_textboxes` 对齐到
/// 宿主表块跨度之后，tc 归属沿文档序对齐单元格序）。跳过面（与 GT
/// 账本 `txbx_in_tc` 计数条件互补成对——见 docx_structural_ledger）：
/// · **mc:Fallback 副本**（AlternateContent 双写——Choice 主选已由
///   本趟取，Fallback 的 VML 副本再取 = 双提取）；
/// · **body 级 sdt 包裹**（GT 归 `txbx_in_sdt` 损失族——sdt 内容丢
///   是独立缺陷面，本趟不越界）；
/// · **非最外层**（嵌套文本框并入外层区段——内层经
///   `TextBoxContent::read` 的段落解析自然展开，边界登记）；
/// · **非 drawing 形态的表内框**（VML 形态由 `scan_vml_textbox_regions`
///   既有通道承载——本趟不双取）。
fn scan_tc_textbox_regions(xml: &str) -> Vec<(usize, String)> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth: usize = 0;
    let mut body_child_depth: Option<usize> = None;
    let mut block_idx: usize = 0;
    let mut in_fallback: usize = 0;
    let mut in_drawing: usize = 0;
    // w:tc 容器深度（表内上下文——捕获的必要条件）。
    let mut in_tc: usize = 0;
    // body 级 sdt 包裹深度标记（进入位 depth；离开清空——该子树整体
    // 归 sdt 损失族，捕获面排除。近似口径：仅 body 直接子层的 sdt
    // 置标记；tc 内层 sdt 不置（与 GT `in_bsdt` 置位条件一致——
    // 语料实测零命中，防御性排除）。
    let mut in_body_sdt: Option<usize> = None;
    // 外层 txbxContent 捕获态（嵌套文本框并入外层区段）。
    let mut capture: Option<(usize, usize)> = None; // (host, txbx 嵌套深度)
    let mut span_start = 0usize;
    let mut out: Vec<(usize, String)> = Vec::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "body" => body_child_depth = Some(depth + 1),
                    "Fallback" => in_fallback += 1,
                    "drawing" => in_drawing += 1,
                    "tc" => in_tc += 1,
                    "sdt" | "sdtContent" => {
                        if in_body_sdt.is_none()
                            && body_child_depth == Some(depth)
                            && in_tc == 0
                        {
                            in_body_sdt = Some(depth);
                        }
                    }
                    "txbxContent" => {
                        if in_fallback == 0
                            && in_drawing > 0
                            && in_tc > 0
                            && in_body_sdt.is_none()
                            && capture.is_none()
                        {
                            // 宿主 = 最近一个已计数的顶层块（承载本 w:tbl
                            // 的 body 直接子层序）。
                            capture = Some((block_idx.saturating_sub(1), 1));
                            span_start = pos_before;
                        } else if let Some((_, d)) = capture.as_mut() {
                            *d += 1;
                        }
                    }
                    "p" | "tbl"
                        // 顶层块计数：Start 时 depth = 该元素自身层（body
                        // 直接子层 = body_child_depth）——depth 在 match 后
                        // 才自增，此处比 body_child_depth 本值（非 +1）。
                        // 捕获态冻结（txbxContent 内层 w:p 非顶层——与
                        // scan_vml_textbox_regions 同防御）。
                        if capture.is_none() && in_fallback == 0 && body_child_depth == Some(depth)
                        => {
                            block_idx += 1;
                        }
                    _ => {}
                }
                depth += 1;
            }
            // 自闭合 `<w:p/>` / `<w:tbl/>`（空形态）：与 Start+End 等价计
            // 一个顶层块；自闭合 txbxContent 无内容不捕获。
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if (name == "p" || name == "tbl")
                    && capture.is_none()
                    && in_fallback == 0
                    && body_child_depth == Some(depth)
                {
                    block_idx += 1;
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "Fallback" => in_fallback = in_fallback.saturating_sub(1),
                    "drawing" => in_drawing = in_drawing.saturating_sub(1),
                    "tc" => in_tc = in_tc.saturating_sub(1),
                    "sdt" | "sdtContent" => {
                        if let Some(m) = in_body_sdt {
                            // 离开标记层（本元素的 End 在 depth = m + 1——
                            // Start 置位时 depth = m，随后自增）。
                            if depth == m + 1 {
                                in_body_sdt = None;
                            }
                        }
                    }
                    "txbxContent" => {
                        if let Some((host, d)) = capture.as_mut() {
                            *d -= 1;
                            if *d == 0 {
                                let host = *host;
                                capture = None;
                                let span_end = reader.buffer_position() as usize;
                                // quick-xml 0.41 buffer_position 边界计量偏移
                                // （与 scan_vml_textbox_regions 同修——区段
                                // 起点前导空白 / 终点闭合标签截断的双端
                                // 前向修复，详见该函数批 4 注释）。
                                let bytes = xml.as_bytes();
                                let mut s = span_start.min(xml.len());
                                while s < xml.len() && bytes[s] != b'<' {
                                    s += 1;
                                }
                                let mut e = span_end.min(xml.len());
                                if e == 0 || bytes[e - 1] != b'>' {
                                    while e < xml.len() && bytes[e] != b'>' {
                                        e += 1;
                                    }
                                    if e < xml.len() {
                                        e += 1;
                                    }
                                }
                                if s < e {
                                    out.push((host, xml[s..e].to_string()));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break, // 门 2+3 已过结构校验（严格）；兼容面防御终止
        }
    }
    out
}

/// #18：VML 区段 → 内层块（docx-rs 读面复用：EventReader 直喂
/// `TextBoxContent::read`——零第二解析器，约束 #1/#2 纪律）。
#[allow(clippy::too_many_arguments)]
fn parse_textbox_region(
    region: &str,
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<Block>, io::ReadError> {
    // 读面项经 crate 根重导出（lib.rs `pub use reader::*`——reader
    // 模块本体私有）。
    use docx_rs::{ElementReader, EventReader, XmlEvent};
    let mut er = EventReader::new(region.as_bytes());
    // 首事件 = txbxContent Start（区段起点恒是；批 4 加固：前导空白
    // 事件容忍——quick-xml 位置偏移的防御纵深）——属性透传。
    let attrs = loop {
        match er.next_event() {
            Ok(XmlEvent::StartElement { attributes, .. }) => break attributes,
            Ok(XmlEvent::Whitespace(_)) => continue,
            _ => {
                return Err(io::ReadError::InvalidPart {
                    part: part.to_string(),
                    detail: "textbox region malformed (missing txbxContent open)".into(),
                });
            }
        }
    };
    let content =
        docx_rs::TextBoxContent::read(&mut er, &attrs).map_err(|_| io::ReadError::InvalidPart {
            part: part.to_string(),
            detail: "textbox region parse failed".into(),
        })?;
    text_box_blocks(
        &content.children,
        table,
        index,
        styles,
        part,
        // VML 第二趟仅扫描 document.xml（apply_vml_textboxes）——文本
        // 框区域恒正文源；页眉/页脚内 VML 文本框读面未提取（登记面外）。
        BlockSource::Body,
        strict,
        warnings,
    )
}

/// #19：正文脚注引用位扫描（`w:footnoteReference`——引擎 run 模型
/// 不承载，CAPABILITY_NOTES §5；文档级第二趟）。
///
/// 返回 body 块索引 → 脚注 id 序（文档序）。body 块索引 = `w:p`/
/// `w:tbl` 的 body 直接子层计数（与 extract_blocks/scan_vml_textbox_regions
/// 一致，含自闭合形态）；文本框内容（`w:txbxContent`）与 mc:Fallback
/// 复制区内引用不标注（内层块非 body 块 / Fallback 为复制区）；表内
/// 段引用归表块（标注面为 body 段落——表块不消费，静默）。
fn scan_note_references(xml: &str) -> HashMap<usize, Vec<i64>> {
    use quick_xml::events::Event;

    /// 条目 id 属性读取（w:id——ASCII 数位，损失解码防御）。
    fn note_id(e: &quick_xml::events::BytesStart<'_>) -> Option<i64> {
        for attr in e.attributes() {
            let Ok(a) = attr else { continue };
            let key = a.key.local_name();
            if key.as_ref() == b"id" {
                if let Ok(v) = std::str::from_utf8(a.value.as_ref()) {
                    if let Ok(id) = v.trim().parse::<i64>() {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth: usize = 0;
    let mut body_child_depth: Option<usize> = None;
    let mut block_idx: usize = 0;
    let mut in_fallback: usize = 0;
    let mut in_txbx: usize = 0;
    let mut out: HashMap<usize, Vec<i64>> = HashMap::new();
    // 引用位记录（自闭合/双标签同形）。
    let record = |e: &quick_xml::events::BytesStart<'_>,
                  block_idx: usize,
                  in_fallback: usize,
                  in_txbx: usize,
                  out: &mut HashMap<usize, Vec<i64>>| {
        if in_fallback == 0 && in_txbx == 0 {
            if let Some(id) = note_id(e) {
                out.entry(block_idx.saturating_sub(1)).or_default().push(id);
            }
        }
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "body" => body_child_depth = Some(depth + 1),
                    "Fallback" => in_fallback += 1,
                    "txbxContent" => in_txbx += 1,
                    "p" | "tbl" => {
                        if in_fallback == 0 && body_child_depth == Some(depth) {
                            block_idx += 1;
                        }
                    }
                    "footnoteReference" => {
                        record(&e, block_idx, in_fallback, in_txbx, &mut out);
                    }
                    _ => {}
                }
                depth += 1;
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "p" | "tbl" => {
                        if in_fallback == 0 && body_child_depth == Some(depth) {
                            block_idx += 1;
                        }
                    }
                    "footnoteReference" => {
                        record(&e, block_idx, in_fallback, in_txbx, &mut out);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "Fallback" => in_fallback = in_fallback.saturating_sub(1),
                    "txbxContent" => in_txbx = in_txbx.saturating_sub(1),
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break, // 门 2+3 已过结构校验；兼容面防御终止
        }
    }
    out
}

/// #19：脚注/尾注部件条目区段扫描（`<w:footnote …>…</w:footnote>` /
/// `<w:endnote …>`——不嵌套，防御深度计数）。返回 (id, type, 区段
/// XML) 序列（部件序）。
fn scan_note_regions(xml: &str, element: &str) -> Vec<(i64, Option<String>, String)> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth: usize = 0;
    // 条目捕获态 = 条目**内部**层深（Start 时 depth+1——End 事件到达时
    // depth 回落到该值；直接记元素层会在 End 比较时差一）。
    let mut entry_depth: Option<usize> = None;
    let mut span_start = 0usize;
    let mut cur_id: i64 = 0;
    let mut cur_type: Option<String> = None;
    let mut out: Vec<(i64, Option<String>, String)> = Vec::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name == element && entry_depth.is_none() {
                    let mut id: i64 = 0;
                    let mut ty: Option<String> = None;
                    for attr in e.attributes() {
                        let Ok(a) = attr else { continue };
                        let key = a.key.local_name();
                        if key.as_ref() == b"id" {
                            if let Ok(v) = std::str::from_utf8(a.value.as_ref()) {
                                id = v.trim().parse::<i64>().unwrap_or(0);
                            }
                        } else if key.as_ref() == b"type" {
                            ty = Some(String::from_utf8_lossy(a.value.as_ref()).into_owned());
                        }
                    }
                    cur_id = id;
                    cur_type = ty;
                    span_start = pos_before;
                    entry_depth = Some(depth + 1);
                }
                depth += 1;
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name == element && entry_depth == Some(depth) {
                    let span_end = reader.buffer_position() as usize;
                    out.push((
                        cur_id,
                        cur_type.take(),
                        xml[span_start..span_end].to_string(),
                    ));
                    entry_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    out
}

/// #19：脚注/尾注部件第二趟解析（word/footnotes.xml / word/endnotes.xml
/// ——引擎读面从不读，CAPABILITY_NOTES §5）。
///
/// 分隔符条目（`w:type=separator`/`continuationSeparator`——呈现辅助
/// 件）与 `id < 1` 条目不入流；区段解析复用 docx-rs 读面
/// （Paragraph::read/Table::read——零第二解析器，#18 同款纪律）。
#[allow(clippy::too_many_arguments)]
fn parse_notes_part(
    xml: &str,
    element: &str,
    part: &str,
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<FootnoteData>, io::ReadError> {
    // 快路径：部件缺席（空串）或无条目标签（损失解码下 contains 防御）。
    if !xml.contains(element) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (id, note_type, region) in scan_note_regions(xml, element) {
        if matches!(
            note_type.as_deref(),
            Some("separator") | Some("continuationSeparator")
        ) || id < 1
        {
            continue;
        }
        let blocks = parse_note_region(
            &region, element, part, table, index, styles, strict, warnings,
        )?;
        out.push(FootnoteData { id, blocks });
    }
    Ok(out)
}

/// #19：脚注/尾注条目区段 → 内层块（docx-rs 读面复用：首事件 = 条目
/// Start，子层 w:p/w:tbl 经 Paragraph::read/Table::read——#18 文本框
/// 区段同款纪律）。
#[allow(clippy::too_many_arguments)]
fn parse_note_region(
    region: &str,
    element: &str,
    part: &str,
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<Block>, io::ReadError> {
    use docx_rs::{ElementReader, EventReader, XmlEvent};
    let parse_err = |detail: &str| io::ReadError::InvalidPart {
        part: part.to_string(),
        detail: detail.to_string(),
    };
    let mut er = EventReader::new(region.as_bytes());
    // 首事件 = 条目 Start（区段起点恒是）。
    match er.next_event() {
        Ok(XmlEvent::StartElement { .. }) => {}
        _ => return Err(parse_err("note region malformed (missing entry open)")),
    }
    let mut blocks = Vec::new();
    loop {
        match er.next_event() {
            Ok(XmlEvent::StartElement {
                name, attributes, ..
            }) => {
                let local = name.local_name.clone();
                if local == "p" {
                    let p = docx_rs::Paragraph::read(&mut er, &attributes)
                        .map_err(|_| parse_err("note paragraph parse failed"))?;
                    blocks.push(Block::Paragraph(extract_paragraph(
                        &p,
                        table,
                        index,
                        styles,
                        part,
                        BlockSource::Body,
                        strict,
                        warnings,
                    )?));
                } else if local == "tbl" {
                    let t = docx_rs::Table::read(&mut er, &attributes)
                        .map_err(|_| parse_err("note table parse failed"))?;
                    // #39：嵌套表沿文档序独立成块（外层在前、内层
                    // 行主序随后——表数守恒）。
                    for td in extract_table(
                        &t,
                        table,
                        index,
                        styles,
                        part,
                        BlockSource::Body,
                        strict,
                        warnings,
                    )? {
                        blocks.push(Block::Table(td));
                    }
                }
            }
            Ok(XmlEvent::EndElement { name, .. }) => {
                if name.local_name == element {
                    return Ok(blocks);
                }
            }
            Err(_) => return Err(parse_err("note region parse failed")),
            _ => {}
        }
    }
}

/// #19：批注面装配（模型面 docx.comments + 锚关联最终块序）。
///
/// anchor_block_idx 语义：`commentRangeStart` 首现块在**最终块序**中的
/// 数组索引（VML/DrawingML 文本框插入后——lockstep 重 walk：body 段/表
/// 计数与 extract_blocks 一致，文本框块跳过）。孤儿批注
/// （comments.xml 在场、正文无锚——含表内锚）→ None。表内锚的块级
/// 承载待需要时扩展（当前双夹具均为段落锚）。
#[allow(clippy::too_many_arguments)]
fn extract_comments(
    docx: &docx_rs::Docx,
    comment_anchors: &[(usize, usize)],
    blocks: &[Block],
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<CommentData>, io::ReadError> {
    // 模型面批注（docx-rs 仅经 document.xml.rels 读 comments.xml——
    // 部件在场而 rels 未声明 = 空集，诚实语义）。
    let model_comments = docx.comments.clone().into_inner();
    if model_comments.is_empty() {
        return Ok(Vec::new());
    }
    // body 块索引 → 最终数组索引（lockstep：段/表计数，文本框跳过）。
    let mut body_to_final: HashMap<usize, usize> = HashMap::new();
    let mut body_idx = 0usize;
    for (i, b) in blocks.iter().enumerate() {
        match b {
            Block::Paragraph(_) | Block::Table(_) => {
                body_to_final.insert(body_idx, i);
                body_idx += 1;
            }
            Block::TextBox(_) => {}
        }
    }
    // 锚首现映射（批注 id → body 块索引）。
    let mut anchor_map: HashMap<usize, usize> = HashMap::new();
    for &(body_i, id) in comment_anchors {
        anchor_map.entry(id).or_insert(body_i);
    }
    let mut out = Vec::new();
    for c in &model_comments {
        let mut cblocks = Vec::new();
        for child in &c.children {
            match child {
                docx_rs::CommentChild::Paragraph(p) => {
                    cblocks.push(Block::Paragraph(extract_paragraph(
                        p,
                        table,
                        index,
                        styles,
                        "word/comments.xml",
                        // 批注 = 文档内容面（BlockSource 语义二分：内容
                        // vs 页面装置——批注非页面槽位）。
                        BlockSource::Body,
                        strict,
                        warnings,
                    )?))
                }
                docx_rs::CommentChild::Table(t) => {
                    // #39：嵌套表同构独立成块。
                    for td in extract_table(
                        t,
                        table,
                        index,
                        styles,
                        "word/comments.xml",
                        BlockSource::Body,
                        strict,
                        warnings,
                    )? {
                        cblocks.push(Block::Table(td));
                    }
                }
            }
        }
        let anchor_block_idx = anchor_map
            .get(&c.id)
            .and_then(|&b| body_to_final.get(&b))
            .copied();
        out.push(CommentData {
            id: c.id,
            author: c.author.clone(),
            date: c.date.clone(),
            anchor_block_idx,
            blocks: cblocks,
        });
    }
    Ok(out)
}

/// #18：VML 文本框块序插入（宿主块后——降序插入保索引稳定）。
#[allow(clippy::too_many_arguments)]
fn apply_vml_textboxes(
    mut blocks: Vec<Block>,
    document_xml: &str,
    bundle: &RelsBundle,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
    // #257：节边界同步重映射（VML 插入点及之后的边界随插入位移——
    // 插入的文本框块是其宿主段的内容，归属宿主段所在节）。
    section_boundaries: &mut [usize],
) -> Result<Vec<Block>, io::ReadError> {
    if !document_xml.contains("txbxContent") {
        return Ok(blocks); // 快路径零成本
    }
    let table = bundle.document.as_ref();
    let regions = scan_vml_textbox_regions(document_xml);
    for (host, region) in regions.iter().rev() {
        let tb_blocks = parse_textbox_region(
            region,
            table,
            index,
            styles,
            "word/document.xml",
            strict,
            warnings,
        )?;
        if tb_blocks.is_empty() {
            continue;
        }
        let insert_at = (*host + 1).min(blocks.len());
        // #257：插入点及之后的节边界同步 +1（逆序插入下当前
        // insert_at 即最终坐标系——后续插入点更小，不受本次位移）。
        for b in section_boundaries.iter_mut() {
            if *b >= insert_at {
                *b += 1;
            }
        }
        blocks.insert(
            insert_at,
            Block::TextBox(TextBoxData {
                blocks: tb_blocks,
                source: BlockSource::Body,
            }),
        );
    }
    Ok(blocks)
}

/// #345（#318 D2）：表内 DrawingML 文本框块序插入（宿主表块跨度后）。
///
/// 与 `apply_vml_textboxes` 同构的第二趟发射面，差异在**发射位**：
/// 表内框的宿主是表——插入位对齐到宿主表在块流中的**完整跨度之后**
/// （外层表块 + 其全部内层表块——`extract_table` 沿文档序独立成块的
/// 既有发射面），而非 ordinal+1 近似（该近似会把表内框插进外层表与
/// 内层表块之间——语义上表内框归属整表，须整表块后）。块流 lockstep
/// 重走（段/非嵌套表计 body 序——TextBox 块与嵌套表块跳过）把 body
/// 子层序映射到最终数组坐标。同宿主多框沿文档序追加（span_end 随插
/// 自增）；不同宿主升序处理（前方插入不位移后方宿主跨度）。
#[allow(clippy::too_many_arguments)]
fn apply_tc_textboxes(
    mut blocks: Vec<Block>,
    document_xml: &str,
    bundle: &RelsBundle,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
    // #257：节边界同步重映射（与 apply_vml_textboxes 同语义——插入位
    // 及之后的边界随插入位移；节边界 = 段落级 sectPr 闭合，恒不在表
    // 跨度内，升序插入下逐次重映射为累积正确坐标）。
    section_boundaries: &mut [usize],
) -> Result<Vec<Block>, io::ReadError> {
    if !document_xml.contains("txbxContent") {
        return Ok(blocks); // 快路径零成本（与 VML 趟同口径）
    }
    let regions = scan_tc_textbox_regions(document_xml);
    if regions.is_empty() {
        return Ok(blocks);
    }
    // 宿主升序分组（同宿主保持文档序——扫描输出已文档序）。
    let mut hosts: Vec<usize> = regions.iter().map(|(h, _)| *h).collect();
    hosts.sort_unstable();
    hosts.dedup();
    for host in hosts {
        // lockstep 重走：body 子层序 → 宿主表跨度终点（当前数组坐标）。
        // 每次插入后数组已变——每宿主重走一次（升序下前方插入只位移
        // 后方坐标，重走天然吸收）。
        let mut body_child = 0usize;
        let mut span_end = blocks.len();
        for (i, b) in blocks.iter().enumerate() {
            match b {
                Block::Paragraph(_) => body_child += 1,
                Block::Table(t) => {
                    if !t.nested {
                        body_child += 1;
                        if body_child == host + 1 {
                            // 宿主表首块——跨度 = 本块 + 后续连续内层表块。
                            let mut j = i + 1;
                            while j < blocks.len() {
                                match &blocks[j] {
                                    Block::Table(nt) if nt.nested => j += 1,
                                    _ => break,
                                }
                            }
                            span_end = j;
                            break;
                        }
                    }
                    // nested 表块：属前方表跨度，不计 body 序。
                }
                Block::TextBox(_) => {} // 非顶层 body 子层（模型面/第二趟发射块）
            }
        }
        // 该宿主的全部区段（文档序）沿 span_end 逐个追加。
        for (h, region) in regions.iter().filter(|(h, _)| *h == host) {
            debug_assert_eq!(*h, host);
            let tb_blocks = parse_textbox_region(
                region,
                bundle.document.as_ref(),
                index,
                styles,
                "word/document.xml",
                strict,
                warnings,
            )?;
            if tb_blocks.is_empty() {
                continue;
            }
            let insert_at = span_end.min(blocks.len());
            // #257：插入点及之后的节边界同步 +1（升序逐次重映射）。
            for b in section_boundaries.iter_mut() {
                if *b >= insert_at {
                    *b += 1;
                }
            }
            blocks.insert(
                insert_at,
                Block::TextBox(TextBoxData {
                    blocks: tb_blocks,
                    source: BlockSource::Body,
                }),
            );
            span_end += 1; // 同宿主后续框追加在本框之后（文档序）。
        }
    }
    Ok(blocks)
}

/// #19：批注锚采集面（body 块索引, 批注 id）——extract_blocks 副产
///（首现语义在 extract_comments 消费面归并）。
type CommentAnchors = Vec<(usize, usize)>;

/// #257：中段节标记——extract_blocks 副产（文档序）：承载段落级
/// sectPr 的段落闭合一节，`boundary` = 该段（含其 DrawingML 文本框
/// 块）入序后的 blocks 长度。`sp` 克隆自 docx-rs 模型（引用/几何/
/// titlePg/pgNumType 全量——中段节页眉页脚内容 docx-rs 不联接，
/// 引擎第二趟自解析）。
/// 边界为 **VML 插入前**坐标系——apply_vml_textboxes 插入时同步重映射。
type SectionMarks = Vec<(usize, docx_rs::SectionProperty)>;

fn extract_blocks(
    docx: &docx_rs::Docx,
    bundle: &RelsBundle,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
    note_refs: &HashMap<usize, Vec<i64>>,
) -> Result<(Vec<Block>, CommentAnchors, SectionMarks, Vec<usize>), io::ReadError> {
    let table = bundle.document.as_ref();
    let mut blocks = Vec::new();
    // #19：批注锚采集（body 块索引, 批注 id）+ body 块计数器
    //（w:p/w:tbl 序——与 scan_note_references 的 XML 计数一致）。
    let mut comment_anchors: CommentAnchors = Vec::new();
    // #257：中段节标记（文档序）——承载 sectPr 的段落闭合一节。
    let mut section_marks: SectionMarks = Vec::new();
    // #268：显式分栏符块（含 `w:br w:type="column"` 的段落块索引，
    // 全局块序——build_sections 按节 block_range 切分归属）。
    let mut column_break_blocks: Vec<usize> = Vec::new();
    let mut body_idx: usize = 0;
    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => {
                let k = body_idx;
                body_idx += 1;
                let mut pd = extract_paragraph(
                    p,
                    table,
                    index,
                    styles,
                    "word/document.xml",
                    BlockSource::Body,
                    strict,
                    warnings,
                )?;
                // #19：脚注引用位标注（文档级扫描 → 段落标记面）。
                if let Some(refs) = note_refs.get(&k) {
                    pd.footnote_refs = refs.clone();
                }
                // #19：段落级批注锚（ParagraphChild::CommentStart——
                // 引擎读面已承载；跨块区间归首现块）。
                for c in &p.children {
                    if let docx_rs::ParagraphChild::CommentStart(cs) = c {
                        comment_anchors.push((k, cs.id));
                    }
                }
                blocks.push(Block::Paragraph(pd));
                // #268：显式分栏符标注（含符段落的自身块索引——符前
                // 文本在原栏、符后入下栏，块级归属见
                // `SectionData::column_of`）。
                if paragraph_has_column_break(p) {
                    column_break_blocks.push(blocks.len() - 1);
                }
                // #18：DrawingML 文本框（w:drawing → wps:txbx——引擎
                // 读面已承载）宿主段后发射；VML 形态走文档级第二趟
                // （apply_vml_textboxes——读面不承载，登记）。
                for tb in paragraph_text_box_children(p) {
                    let tb_blocks = text_box_blocks(
                        tb,
                        table,
                        index,
                        styles,
                        "word/document.xml",
                        BlockSource::Body,
                        strict,
                        warnings,
                    )?;
                    if !tb_blocks.is_empty() {
                        blocks.push(Block::TextBox(TextBoxData {
                            blocks: tb_blocks,
                            source: BlockSource::Body,
                        }));
                    }
                }
                // #257：段落级 sectPr = 本节闭合（该段为本节末段——
                // 边界含本段及其文本框块；末节属性在 body 级，不在
                // 段落级，此处只记中段节）。
                if let Some(sp) = &p.property.section_property {
                    section_marks.push((blocks.len(), sp.clone()));
                }
            }
            docx_rs::DocumentChild::Table(t) => {
                let k = body_idx;
                body_idx += 1;
                // #39：嵌套表同构独立成块。
                for td in extract_table(
                    t,
                    table,
                    index,
                    styles,
                    "word/document.xml",
                    BlockSource::Body,
                    strict,
                    warnings,
                )? {
                    // #19：表内脚注引用位不标注（body 段落标记面——表内
                    // 段落非 body 块；文档级面待需要时扩展）。
                    let _ = k;
                    blocks.push(Block::Table(td));
                }
            }
            // #19：body 级批注锚（段间锚——归属前一块；首块前锚归
            // 块 0 语义上不成立，saturating 归零防御）。
            docx_rs::DocumentChild::CommentStart(cs) => {
                comment_anchors.push((body_idx.saturating_sub(1), cs.id));
            }
            // 书签/批注结束锄/内容控件/TOC/节——模型级切片面外，跳过（登记）。
            _ => {}
        }
    }
    Ok((blocks, comment_anchors, section_marks, column_break_blocks))
}

/// #268：段落含显式分栏符（`w:br w:type="column"`）？——run 直挂 /
/// 超链接内嵌 / 修订插入三通道；修订删除面不计（非当前生效文本）。
fn paragraph_has_column_break(p: &docx_rs::Paragraph) -> bool {
    fn run_has(r: &docx_rs::Run) -> bool {
        r.children.iter().any(|c| match c {
            docx_rs::RunChild::Break(b) => break_is_column(b),
            _ => false,
        })
    }
    p.children.iter().any(|c| match c {
        docx_rs::ParagraphChild::Run(r) => run_has(r),
        docx_rs::ParagraphChild::Hyperlink(h) => h
            .children
            .iter()
            .any(|hc| matches!(hc, docx_rs::ParagraphChild::Run(r) if run_has(r))),
        docx_rs::ParagraphChild::Insert(ins) => ins.children.iter().any(|ic| match ic {
            docx_rs::InsertChild::Run(r) => run_has(r),
            _ => false,
        }),
        _ => false,
    })
}

/// #268：`w:br` 是否 column 型？——docx-rs 0.4.22 `Break.break_type`
/// 私有无 getter（builder 面独占），`Serialize` derive 是模型面唯一
/// 公开读通道（#22 P2-2 `TableCellProperty` 同族先例；serde_json
/// 已在产品闭包）。
fn break_is_column(b: &docx_rs::Break) -> bool {
    serde_json::to_value(b)
        .ok()
        .and_then(|v| {
            v.get("breakType")
                .and_then(|t| t.as_str())
                .map(|s| s == "column")
        })
        .unwrap_or(false)
}

/// numId → 各 ilvl 级 numFmt 解析索引（r4-t3——numbering.xml 链的
/// 读面消费：num 桥 → abstractNum 层级 → lvlOverride 并入；引擎模型
/// 全链 pub——Numbering.id/abstract_num_id、AbstractNumbering.levels、
/// Level.level/format.val、LevelOverride.override_level）。位于
/// [`extract_blocks`] 前定义以就近服务提取层。
///
/// ilvl 键仅收**已定义级**（HashMap 形态——敌意 `w:ilvl` 巨值不触发
/// 分配，越定义域查询 → None → 软警告路径）。
#[derive(Debug, Default)]
struct NumberingIndex {
    /// numId → (ilvl → 原生 numFmt 值)。
    by_num_id: HashMap<usize, HashMap<usize, String>>,
}

impl NumberingIndex {
    fn build(numberings: &docx_rs::Numberings) -> Self {
        let mut index = NumberingIndex::default();
        for num in &numberings.numberings {
            let mut levels: HashMap<usize, String> = HashMap::new();
            if let Some(a) = numberings
                .abstract_nums
                .iter()
                .find(|a| a.id == num.abstract_num_id)
            {
                for l in &a.levels {
                    levels.insert(l.level, l.format.val.clone());
                }
            }
            // lvlOverride 并入（override_level 形态——startOnly 覆盖无
            // format 面不并入）。
            for o in &num.level_overrides {
                if let Some(ol) = &o.override_level {
                    levels.insert(o.level, ol.format.val.clone());
                }
            }
            index.by_num_id.insert(num.id, levels);
        }
        index
    }
}

/// numPr → 承载（r4-t3）：
/// - numId 缺席（仅 ilvl 或空 numPr）→ None（OOXML 语义：非列表段）；
/// - `w:numId w:val="0"` → None（OOXML 去编号惯例——样式继承编号的
///   显式去除，非列表段）；
/// - num 桥悬空（numbering.xml 无该 numId）→ **严格拒** / 兼容
///   BrokenReference 警告 + kind=None 结构承载（num_id/ilvl 不丢——
///   与超链接/页眉页脚悬空引用同族纪律）；
/// - 抽象层级缺定义（numStyleLink 无层级 / ilvl 超出定义域）→
///   BrokenReference 警告（**两模式非致命**——合法但解析深度外）+
///   kind=None 结构承载；
/// - 解析成功 → kind（`bullet` → Bullet，其余 → Ordered）+ format
///   （原生 numFmt 值——写面降级可诊断）。
fn carry_numbering(
    np: &docx_rs::NumberingProperty,
    index: &NumberingIndex,
    part: &str,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Option<ParagraphNumbering>, io::ReadError> {
    let Some(num_id) = np.id.as_ref().map(|n| n.id) else {
        return Ok(None);
    };
    if num_id == 0 {
        return Ok(None);
    }
    let ilvl = np.level.as_ref().map(|l| l.val).unwrap_or(0);
    let Some(levels) = index.by_num_id.get(&num_id) else {
        let detail =
            format!("numbering reference numId '{num_id}' has no numbering definition (dangling)");
        if strict {
            return Err(io::ReadError::InvalidPart {
                part: part.to_string(),
                detail,
            });
        }
        warnings.push(CompatibilityWarning {
            part: part.to_string(),
            kind: WarningKind::BrokenReference,
            detail: format!("{detail} (kind unresolved; num_id/ilvl carried)"),
        });
        return Ok(Some(ParagraphNumbering {
            num_id,
            ilvl,
            kind: None,
            format: None,
        }));
    };
    let Some(format) = levels.get(&ilvl) else {
        let detail = format!(
            "numbering numId '{num_id}' has no level definition at ilvl {ilvl} (numStyleLink or level beyond definitions)"
        );
        warnings.push(CompatibilityWarning {
            part: part.to_string(),
            kind: WarningKind::BrokenReference,
            detail: format!("{detail} (kind unresolved; num_id/ilvl carried)"),
        });
        return Ok(Some(ParagraphNumbering {
            num_id,
            ilvl,
            kind: None,
            format: None,
        }));
    };
    let kind = if format == "bullet" {
        ListKind::Bullet
    } else {
        ListKind::Ordered
    };
    Ok(Some(ParagraphNumbering {
        num_id,
        ilvl,
        kind: Some(kind),
        format: Some(format.clone()),
    }))
}

/// `w:ind` → 承载（r4-t3：twips 透传——引擎 Indent 字段全 pub；
/// Chars 字符单位变体〔hangingChars/firstLineChars〕登记不承载）。
fn carry_indent(ind: &docx_rs::Indent) -> ParagraphIndent {
    ParagraphIndent {
        left: ind.start,
        right: ind.end,
        special: ind.special_indent.as_ref().map(|s| match s {
            docx_rs::SpecialIndentType::FirstLine(v) => ParagraphIndentSpecial::FirstLine(*v),
            docx_rs::SpecialIndentType::Hanging(v) => ParagraphIndentSpecial::Hanging(*v),
        }),
    }
}

/// 段落提取：样式透传 + run 序（Run + Hyperlink 内 run[目标联接]）+
/// 编号/缩进承载（r4-t3）+ 块源定值（P2-1 `source` 参数）。
#[allow(clippy::too_many_arguments)]
fn extract_paragraph(
    p: &docx_rs::Paragraph,
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    part: &str,
    source: BlockSource,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<ParagraphData, io::ReadError> {
    let style = p.property.style.as_ref().map(|s| s.val.clone());
    // #13（A1）：标题层级全链解析（读面单一真源——四链见
    // ParagraphData::heading_level 文档；真 Word 中文版 styleId="1"-"9"
    // 经样式名链命中）。
    let heading_level = resolve_heading_level(
        p.property.outline_lvl.as_ref().map(|o| o.v),
        style.as_deref(),
        styles,
    );
    let numbering = match &p.property.numbering_property {
        Some(np) => carry_numbering(np, index, part, strict, warnings)?,
        None => None,
    };
    let indent = p.property.indent.as_ref().map(carry_indent);
    let mut runs = Vec::new();
    // #14（A2）：域状态机——段落级单实例（域可跨 run：begin 与 end
    // 分属不同 run，状态必须段落作用域）。dropped 计数随段落透出。
    let mut fm = FieldMachine::default();
    // #17：修订删除文本（w:delText）计数——OOXML 语义 = 已删除内容
    // 不提取；逐段登记可见性（WarningKind::RevisionDelete）。
    let mut del_text_runs = 0usize;
    for child in &p.children {
        match child {
            docx_rs::ParagraphChild::Run(r) => {
                del_text_runs += count_del_text(r);
                runs.push(extract_run(r, &mut fm, false))
            }
            // 超链接：内文本保留（run 边界保持）+ 目标联接（docx-r1）。
            docx_rs::ParagraphChild::Hyperlink(h) => {
                let target = resolve_hyperlink(h, table, part, strict, warnings)?;
                for c in &h.children {
                    if let docx_rs::ParagraphChild::Run(r) = c {
                        let mut rd = extract_run(r, &mut fm, false);
                        rd.hyperlink = Some(target.clone());
                        runs.push(rd);
                    }
                }
            }
            // #17：修订插入（w:ins）= 当前生效文本——正常提取 + run 级
            // revision_inserted 标记（消费方可辨修订通道）。域机器同段
            // 作用域延续（域可跨 ins 边界）。
            docx_rs::ParagraphChild::Insert(ins) => {
                for c in &ins.children {
                    if let docx_rs::InsertChild::Run(r) = c {
                        del_text_runs += count_del_text(r);
                        runs.push(extract_run(r, &mut fm, true));
                    }
                    // ins 内嵌套 w:del（删除容器在插入容器内——罕见）：
                    // delText 计数、内容不提取（同一语义）。
                    if let docx_rs::InsertChild::Delete(del) = c {
                        for dc in &del.children {
                            if let docx_rs::DeleteChild::Run(r) = dc {
                                del_text_runs += count_del_text(r);
                            }
                        }
                    }
                }
            }
            // #17：修订删除容器（w:del）——内容整体不提取（已删除）；
            // delText 计数入告警。
            docx_rs::ParagraphChild::Delete(del) => {
                for dc in &del.children {
                    if let docx_rs::DeleteChild::Run(r) = dc {
                        del_text_runs += count_del_text(r);
                    }
                }
            }
            // 书签/批注/MoveFrom/MoveTo/内容控件——面外跳过（登记：
            // MoveFrom=幽灵文本/MoveTo=移动目标——v0 均不提取，修订
            // 面仅 w:ins 通道，#17 范围）。域机制在 run 级状态机消费
            //（FieldMachine——#14）。
            _ => {}
        }
    }
    if del_text_runs > 0 {
        warnings.push(CompatibilityWarning {
            part: part.to_string(),
            kind: WarningKind::RevisionDelete,
            detail: format!(
                "{del_text_runs} revision-deleted text run(s) skipped                  (w:delText = removed content, not extracted)"
            ),
        });
    }
    Ok(ParagraphData {
        resolved_number: None,
        style,
        heading_level,
        fields_dropped: fm.dropped,
        runs,
        numbering,
        indent,
        footnote_refs: Vec::new(),
        source,
    })
}

/// run 提取：文本三源（Text/Tab/Break+CarriageReturn）+ 加粗斜体
///（超链接目标由调用面挂接——extract_paragraph 的 Hyperlink 分支）。
/// #14（A2）：域机器先行——FieldChar 状态转移 + 指令累积；表现型
/// 域缓存结果区文本剥离（suppress）；空文本 run 保留（run 边界
/// 语义零变化——合并面归导入侧 merge_runs）。
fn extract_run(r: &docx_rs::Run, fm: &mut FieldMachine, revision_inserted: bool) -> RunData {
    let mut text = String::new();
    for child in &r.children {
        match child {
            docx_rs::RunChild::Text(t) => {
                if !fm.suppress() {
                    text.push_str(&t.text);
                }
            }
            docx_rs::RunChild::Tab(_) => {
                if !fm.suppress() {
                    text.push('\t');
                }
            }
            docx_rs::RunChild::Break(_) => {
                if !fm.suppress() {
                    text.push('\n');
                }
            }
            docx_rs::RunChild::CarriageReturn(_) => {
                if !fm.suppress() {
                    text.push('\r');
                }
            }
            // #14：域机器——状态转移 / 指令累积（指令恒不入正文）。
            docx_rs::RunChild::FieldChar(f) => fm.transition(&f.field_char_type),
            docx_rs::RunChild::InstrTextString(s) => fm.instr.push_str(s),
            // Sym/DeleteText/Drawing/Shape/批注锚——面外跳过（登记）。
            _ => {}
        }
    }
    RunData {
        text,
        bold: r.run_property.bold.is_some(),
        italic: r.run_property.italic.is_some(),
        hyperlink: None,
        revision_inserted,
    }
}

/// #17：run 内修订删除文本（`w:delText`）计数——不提取（已删除
/// 内容），计数供段落级可见性告警。
fn count_del_text(r: &docx_rs::Run) -> usize {
    r.children
        .iter()
        .filter(|c| matches!(c, docx_rs::RunChild::DeleteText(_)))
        .count()
}

/// 表提取：**栅格展开 + 合并区域清单**（#22 P2-2）——单元格内多
/// 段落 `\n` 连接；嵌套表/内容控件跳过（登记）。单元格内超链接目标
/// 丢弃（TableData 扁平文本面——登记），但悬空引用仍按严格/兼容纪律
/// 处置（包完整性）；单元格段落的 numPr/ind 同为扁平面外承载（提取
/// 后弃置——登记）。块源定值（P2-1 `source` 参数——表内段落继承表
/// 来源）。
///
/// 合并语义（OOXML §17.4.22/§17.4.57）：
/// - `gridSpan > 1` = 横向合并（覆盖列置空，内容居原点格）；
/// - `vMerge restart` 起、`continue` 续 = 纵向合并（**续格内容置空**——
///   Word 显示语义：续格文本不可见，原点格承载全部内容；部分生成器
///   在续格放副本，本层丢弃）；
/// - 两者叠加 = 块合并（单条矩形）；
/// - 孤儿续格（`continue` 无前置 restart，OOXML 未定义）：不发明
///   区域，按普通格落栅格（内容可见）；
/// - 续格 span 与 restart 不一致：不匹配开放区，同孤儿处理（登记）。
#[allow(clippy::too_many_arguments)]
fn extract_table(
    t: &docx_rs::Table,
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    part: &str,
    source: BlockSource,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<TableData>, io::ReadError> {
    extract_table_inner(t, table, index, styles, part, source, strict, warnings, 0)
}

/// #39：嵌套表深度上限（敌意深嵌 XML 防栈溢出——起域内递归；超限
/// 表整块跳过 + warning 登记，诚实降级而非崩溃）。Word 实务嵌套
/// 深度普遍 ≤ 3（layout 表套数据表）；16 = 宽裕域。
const MAX_NESTED_TABLE_DEPTH: usize = 16;

#[allow(clippy::too_many_arguments)] // 9 参 = 既有 extract_table 8 参 + depth（同豁免模式）
fn extract_table_inner(
    t: &docx_rs::Table,
    table: Option<&RelationshipTable>,
    index: &NumberingIndex,
    styles: &StyleIndex,
    part: &str,
    source: BlockSource,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
    depth: usize,
) -> Result<Vec<TableData>, io::ReadError> {
    // 注：docx-rs 0.4.22 模型中 TableChild/TableRowChild 均为单变体
    //（TableRow/TableCell）——直接解构；模型升级增变体时此函数
    // 编译期报非穷尽，而面外项处理策略不变（跳过+登记）。
    //
    // gridSpan/vMerge 读取通道：docx-rs 0.4.22 `TableCellProperty` 字段
    // 私有且无 getter（builder 面独占），`Serialize` derive（camelCase）
    // 是模型面唯一公开读通道——serde_json 按值取出（`gridSpan`: u32 /
    // `verticalMerge`: "restart"|"continue"|"unsupported"）。版本钉
    // 0.4.22（Cargo.lock）；模型升级时此通道无编译期保护，须复验
    //（序列化字段名/形变化 = 面外合并丢失，仅退化不崩溃）。
    let cell_props = |c: &docx_rs::TableCell| -> (usize, Option<String>) {
        let v = serde_json::to_value(&c.property).unwrap_or(serde_json::Value::Null);
        let span = v
            .get("gridSpan")
            .and_then(|s| s.as_u64())
            .map(|n| n.max(1) as usize) // 敌意 0/负数形（serde u64 域钳 0）→ 钉 1
            .unwrap_or(1);
        let vmerge = v
            .get("verticalMerge")
            .and_then(|s| s.as_str())
            .map(str::to_string);
        (span, vmerge)
    };

    // 开放纵向合并区（restart 起、continue 续；文档序确定性——Vec 线性查，
    // 表内合并数有限）。`last_row` = 最近一次 continue 所在行（行末关账
    // 用：本行未续即封）。
    struct OpenVmerge {
        start_row: usize,
        start_col: usize,
        end_col: usize,
        last_row: usize,
    }
    let close_region = |o: &OpenVmerge, merges: &mut Vec<MergeRange>| {
        if o.last_row > o.start_row {
            // 真纵向/块合并（跨行）。
            merges.push(MergeRange {
                start_row: o.start_row,
                start_col: o.start_col,
                end_row: o.last_row,
                end_col: o.end_col,
            });
        } else if o.end_col > o.start_col {
            // 单行 restart 未续 + 跨列 = 横向合并（gridSpan 语义仍然在）。
            merges.push(MergeRange {
                start_row: o.start_row,
                start_col: o.start_col,
                end_row: o.start_row,
                end_col: o.end_col,
            });
        }
        // 单行单列 restart 未续：无覆盖非原点格——非合并，不发。
    };

    let mut grid_rows: Vec<Vec<String>> = Vec::new();
    let mut merges: Vec<MergeRange> = Vec::new();
    let mut open_v: Vec<OpenVmerge> = Vec::new();
    // #39：内嵌表块流（沿文档序——行主序单元序、递归深度优先；
    // 返回序 = [外层表, 内层表…]）。内层表不进单元格文本（独立块
    // 发射——方案 A：表数守恒 + 消费方 nested 标记分流）。
    let mut nested_tables: Vec<TableData> = Vec::new();
    // 栅格宽度：tblGrid 权威列数（空则 0），行和兜底（畸形表——无 tblGrid
    // 而 gridSpan 撑宽；防御 max）。
    let mut grid_width = t.grid.len();
    for (ri, row) in t.rows.iter().enumerate() {
        let docx_rs::TableChild::TableRow(r) = row;
        let mut grid_row: Vec<String> = Vec::new();
        let mut col = 0usize;
        for cell in &r.cells {
            let docx_rs::TableRowChild::TableCell(c) = cell;
            let (span, vmerge) = cell_props(c);
            let mut parts = Vec::new();
            for content in &c.children {
                match content {
                    docx_rs::TableCellContent::Paragraph(p) => {
                        // 表内段落继承表来源（P2-1——单元格段与表同位置）。
                        let pd = extract_paragraph(
                            p, table, index, styles, part, source, strict, warnings,
                        )?;
                        parts.push(pd.text());
                    }
                    // #39：内嵌表递归提取（w:tbl@w:tc depth≥2——此前
                    // 静默丢弃 = 标书侧费率表整块缺失）；深度上限防
                    // 敌意栈溢出（超限跳过 + warning，诚实降级）。
                    docx_rs::TableCellContent::Table(nt) => {
                        if depth >= MAX_NESTED_TABLE_DEPTH {
                            warnings.push(CompatibilityWarning {
                                part: part.to_string(),
                                kind: WarningKind::SkippedElement,
                                detail: format!(
                                    "nested table at depth {} (max {}) skipped — \
                                     hostile/degenerate nesting",
                                    depth + 1,
                                    MAX_NESTED_TABLE_DEPTH
                                ),
                            });
                        } else {
                            for mut td in extract_table_inner(
                                nt,
                                table,
                                index,
                                styles,
                                part,
                                source,
                                strict,
                                warnings,
                                depth + 1,
                            )? {
                                td.nested = true;
                                nested_tables.push(td);
                            }
                        }
                    }
                    // StructuredDataTag / TableOfContents：面外项（既有
                    // 策略不变——跳过；单元格文本面无承载，登记于模块
                    // 文档「丢弃面」）。注：单元格段落内的 DrawingML 文本
                    // 框子树（wps:txbx）同样不入本路径的段落文本——由
                    // #345（#318 D2）文档级第二趟补提取
                    // （apply_tc_textboxes，发射宿主表块跨度后）。
                    _ => {}
                }
            }
            let text = parts.join("\n");
            // 落栅格：原点格承载内容，gridSpan 覆盖列置空。
            for k in 0..span {
                grid_row.push(if k == 0 { text.clone() } else { String::new() });
            }
            match vmerge.as_deref() {
                Some("restart") => {
                    // 同列区间既有开放区先封（restart 接续 = 新区域链）。
                    if let Some(pos) = open_v
                        .iter()
                        .position(|o| o.start_col == col && o.end_col == col + span - 1)
                    {
                        let o = open_v.remove(pos);
                        close_region(&o, &mut merges);
                    }
                    open_v.push(OpenVmerge {
                        start_row: ri,
                        start_col: col,
                        end_col: col + span - 1,
                        last_row: ri,
                    });
                }
                Some("continue") => {
                    // 同列区间匹配才续（Word 语义：续格内容不可见——
                    // 覆盖置空，原点格已承载）；不匹配 = 孤儿续格（未定义
                    // 行为）→ 普通格处理（内容保留，不发明区域）。
                    if let Some(region) = open_v
                        .iter_mut()
                        .find(|o| o.start_col == col && o.end_col == col + span - 1)
                    {
                        region.last_row = ri;
                        for k in 0..span {
                            grid_row[col + k] = String::new();
                        }
                    }
                }
                _ => {
                    // 普通格（无 vMerge / "unsupported"）：跨列即横向合并。
                    if span > 1 {
                        merges.push(MergeRange {
                            start_row: ri,
                            start_col: col,
                            end_row: ri,
                            end_col: col + span - 1,
                        });
                    }
                }
            }
            debug_assert_eq!(grid_row.len(), col + span);
            col += span;
            // 注：vmerge 格的横向面由区域矩形承载（close_region 单行跨列
            // 分支），不双发两条。
        }
        grid_width = grid_width.max(grid_row.len());
        grid_rows.push(grid_row);
        // 行末关账：本行未续的开放区封盘（区域终止于 last_row）。
        let mut i = 0;
        while i < open_v.len() {
            if open_v[i].last_row < ri {
                let o = open_v.remove(i);
                close_region(&o, &mut merges);
            } else {
                i += 1;
            }
        }
    }
    // 表末关账：仍开放的区域封盘。
    for o in open_v.drain(..) {
        close_region(&o, &mut merges);
    }
    // 行宽对齐：全行补齐到栅格宽（短行尾置空——ragged 行栅格完整形）。
    for row in &mut grid_rows {
        row.resize(grid_width, String::new());
    }
    Ok(vec![TableData {
        rows: grid_rows,
        merges,
        source,
        nested: false,
    }]
    .into_iter()
    .chain(nested_tables)
    .collect())
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
/// 页眉/页脚部件自身的 rels 联接其内部超链接目标（CAPABILITY_NOTES §2）；
/// numPr/ind 承载与正文同路径（r4-t3——页眉/页脚内列表段同样可见）。
fn extract_headers_footers(
    docx: &docx_rs::Docx,
    bundle: &RelsBundle,
    index: &NumberingIndex,
    styles: &StyleIndex,
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
                docx_rs::HeaderChild::Paragraph(p) => {
                    blocks.push(Block::Paragraph(extract_paragraph(
                        p,
                        table,
                        index,
                        styles,
                        part_name,
                        BlockSource::Header,
                        strict,
                        warnings,
                    )?))
                }
                docx_rs::HeaderChild::Table(t) => {
                    // #39：嵌套表同构独立成块。
                    for td in extract_table(
                        t,
                        table,
                        index,
                        styles,
                        part_name,
                        BlockSource::Header,
                        strict,
                        warnings,
                    )? {
                        blocks.push(Block::Table(td));
                    }
                }
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
                docx_rs::FooterChild::Paragraph(p) => {
                    blocks.push(Block::Paragraph(extract_paragraph(
                        p,
                        table,
                        index,
                        styles,
                        part_name,
                        BlockSource::Footer,
                        strict,
                        warnings,
                    )?))
                }
                docx_rs::FooterChild::Table(t) => {
                    // #39：嵌套表同构独立成块。
                    for td in extract_table(
                        t,
                        table,
                        index,
                        styles,
                        part_name,
                        BlockSource::Footer,
                        strict,
                        warnings,
                    )? {
                        blocks.push(Block::Table(td));
                    }
                }
                // StructuredDataTag——面外跳过（登记）。
                _ => {}
            }
        }
        footers.push(HeaderFooterData { variant, blocks });
    }
    Ok((headers, footers))
}

// ---------------------------------------------------------------------------
// #257：多节分节符读面（中段节——docx-rs read_docx 只联末节的补齐）
// ---------------------------------------------------------------------------

/// 节几何提取（pgSz/pgMar → [`PageGeometry`]，twips 同域）。
///
/// `PageSize.w/h` 私有无 getter（docx-rs 0.4.22）——经 serde_json 序列化
/// 通道读取（同 gridSpan/verticalMerge 合并面先例 P2-1：版本钉死
/// 0.4.22，升版即静默退化的已知风险面，代码内登记）。`PageMargin`
///（types 版）字段全 pub——直接访问。
fn section_geometry(sp: &docx_rs::SectionProperty) -> PageGeometry {
    // camelCase 对单字母字段 w/h 无改写；解析失败防御回落默认 A4
    //（敌意畸形值已被容器门/panic 预检拦在前——此处仅类型对齐防御）。
    let size = serde_json::to_value(&sp.page_size).ok();
    let (width, height) = match size
        .as_ref()
        .and_then(|v| v.get("w"))
        .and_then(|v| v.as_u64())
        .zip(
            size.as_ref()
                .and_then(|v| v.get("h"))
                .and_then(|v| v.as_u64()),
        ) {
        Some((w, h)) => (w as u32, h as u32),
        None => (11906, 16838), // docx-rs PageSize::default（A4）
    };
    PageGeometry {
        width,
        height,
        margin_top: sp.page_margin.top,
        margin_right: sp.page_margin.right,
        margin_bottom: sp.page_margin.bottom,
        margin_left: sp.page_margin.left,
        margin_header: sp.page_margin.header,
        margin_footer: sp.page_margin.footer,
        gutter: sp.page_margin.gutter,
    }
}

/// 节类型映射（docx-rs `SectionType` → 引擎 [`SectionKind`]——透传，
/// 一一对应）。
fn section_kind(sp: &docx_rs::SectionProperty) -> Option<SectionKind> {
    sp.section_type.map(|t| match t {
        docx_rs::SectionType::NextPage => SectionKind::NextPage,
        docx_rs::SectionType::NextColumn => SectionKind::NextColumn,
        docx_rs::SectionType::Continuous => SectionKind::Continuous,
        docx_rs::SectionType::EvenPage => SectionKind::EvenPage,
        docx_rs::SectionType::OddPage => SectionKind::OddPage,
    })
}

/// 页眉部件子块提取（`docx_rs::Header` → 块序——与末节提取同构：
/// 段落/表 + StructuredDataTag 面外跳过登记；numPr/ind 承载同路径）。
#[allow(clippy::too_many_arguments)]
fn header_children_blocks(
    header: &docx_rs::Header,
    bundle: &RelsBundle,
    part: &str,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<Block>, io::ReadError> {
    let table = bundle.by_part.get(part);
    let mut blocks = Vec::new();
    for child in &header.children {
        match child {
            docx_rs::HeaderChild::Paragraph(p) => blocks.push(Block::Paragraph(extract_paragraph(
                p,
                table,
                index,
                styles,
                part,
                BlockSource::Header,
                strict,
                warnings,
            )?)),
            docx_rs::HeaderChild::Table(t) => {
                for td in extract_table(
                    t,
                    table,
                    index,
                    styles,
                    part,
                    BlockSource::Header,
                    strict,
                    warnings,
                )? {
                    blocks.push(Block::Table(td));
                }
            }
            // StructuredDataTag——面外跳过（登记；同末节提取口径）。
            _ => {}
        }
    }
    Ok(blocks)
}

/// 页脚部件子块提取（同 [`header_children_blocks`]）。
#[allow(clippy::too_many_arguments)]
fn footer_children_blocks(
    footer: &docx_rs::Footer,
    bundle: &RelsBundle,
    part: &str,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<Vec<Block>, io::ReadError> {
    let table = bundle.by_part.get(part);
    let mut blocks = Vec::new();
    for child in &footer.children {
        match child {
            docx_rs::FooterChild::Paragraph(p) => blocks.push(Block::Paragraph(extract_paragraph(
                p,
                table,
                index,
                styles,
                part,
                BlockSource::Footer,
                strict,
                warnings,
            )?)),
            docx_rs::FooterChild::Table(t) => {
                for td in extract_table(
                    t,
                    table,
                    index,
                    styles,
                    part,
                    BlockSource::Footer,
                    strict,
                    warnings,
                )? {
                    blocks.push(Block::Table(td));
                }
            }
            _ => {}
        }
    }
    Ok(blocks)
}

/// 中段节页眉/页脚部件装载（zip 单趟收集——部件集先全量解析再复用）。
///
/// 返回 part path → 部件 XML。读取失败的部件**不入表**（调用面按
/// 缺件处理——严格拒 / 兼容警告）；门 2 已验部件良构性，此处失败 =
/// 损失解码等边缘态，走降级通道不崩。
fn load_section_part_xml(bytes: &[u8], paths: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if paths.is_empty() {
        return out;
    }
    let Ok(mut za) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return out;
    };
    for path in paths {
        if out.contains_key(path) {
            continue;
        }
        if let Ok(mut zf) = za.by_name(path) {
            let mut xml = String::new();
            if zf.read_to_string(&mut xml).is_ok() {
                out.insert(path.clone(), xml);
            }
        }
    }
    out
}

/// 中段节六槽位提取（引用 rid → 部件 → 引擎侧解析——docx-rs
/// `read_docx` 只把内容联接到末节，中段节引用在、内容不联接，本函数
/// 补齐 P1-1 的静默丢）。
///
/// 槽位语义与末节提取一致：引用悬空 = 严格拒 / 兼容 BrokenReference
/// 警告 + 跳过（slot_part_path 复用）；部件在而解析失败 = 严格拒 /
/// 兼容 MalformedXml 警告 + 跳过（与末节「内容挂接失败」同语义）。
#[allow(clippy::too_many_arguments)]
fn mid_section_slots(
    sp: &docx_rs::SectionProperty,
    part_xml: &HashMap<String, String>,
    bundle: &RelsBundle,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
) -> Result<(Vec<HeaderFooterData>, Vec<HeaderFooterData>), io::ReadError> {
    use docx_rs::FromXML;
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    // 页眉三槽位（default/first/even——引用在而部件缺 = 同末节降级语义）。
    for (variant, reference) in [
        (HeaderFooterVariant::Default, &sp.header_reference),
        (HeaderFooterVariant::First, &sp.first_header_reference),
        (HeaderFooterVariant::Even, &sp.even_header_reference),
    ] {
        let Some(r) = reference else { continue };
        let Some(part) = slot_part_path(&r.id, bundle, strict, warnings)? else {
            continue;
        };
        let Some(xml) = part_xml.get(&part) else {
            let detail = format!("header part '{part}' unreadable (lossy decode or io)");
            if strict {
                return Err(io::ReadError::InvalidPart {
                    part: part.clone(),
                    detail,
                });
            }
            warnings.push(CompatibilityWarning {
                part: part.clone(),
                kind: WarningKind::MalformedXml,
                detail: format!("compat mode: {detail} (slot skipped)"),
            });
            continue;
        };
        match docx_rs::Header::from_xml(xml.as_bytes()) {
            Ok(h) => {
                let blocks =
                    header_children_blocks(&h, bundle, &part, index, styles, strict, warnings)?;
                headers.push(HeaderFooterData { variant, blocks });
            }
            Err(_) => {
                let detail = format!("header part '{part}' parse failed");
                if strict {
                    return Err(io::ReadError::InvalidPart {
                        part: part.clone(),
                        detail,
                    });
                }
                warnings.push(CompatibilityWarning {
                    part: part.clone(),
                    kind: WarningKind::MalformedXml,
                    detail: format!("compat mode: {detail} (slot skipped)"),
                });
            }
        }
    }
    // 页脚三槽位。
    for (variant, reference) in [
        (HeaderFooterVariant::Default, &sp.footer_reference),
        (HeaderFooterVariant::First, &sp.first_footer_reference),
        (HeaderFooterVariant::Even, &sp.even_footer_reference),
    ] {
        let Some(r) = reference else { continue };
        let Some(part) = slot_part_path(&r.id, bundle, strict, warnings)? else {
            continue;
        };
        let Some(xml) = part_xml.get(&part) else {
            let detail = format!("footer part '{part}' unreadable (lossy decode or io)");
            if strict {
                return Err(io::ReadError::InvalidPart {
                    part: part.clone(),
                    detail,
                });
            }
            warnings.push(CompatibilityWarning {
                part: part.clone(),
                kind: WarningKind::MalformedXml,
                detail: format!("compat mode: {detail} (slot skipped)"),
            });
            continue;
        };
        match docx_rs::Footer::from_xml(xml.as_bytes()) {
            Ok(f) => {
                let blocks =
                    footer_children_blocks(&f, bundle, &part, index, styles, strict, warnings)?;
                footers.push(HeaderFooterData { variant, blocks });
            }
            Err(_) => {
                let detail = format!("footer part '{part}' parse failed");
                if strict {
                    return Err(io::ReadError::InvalidPart {
                        part: part.clone(),
                        detail,
                    });
                }
                warnings.push(CompatibilityWarning {
                    part: part.clone(),
                    kind: WarningKind::MalformedXml,
                    detail: format!("compat mode: {detail} (slot skipped)"),
                });
            }
        }
    }
    Ok((headers, footers))
}

/// #268：解析 document.xml 全部 `sectPr/w:cols`（文档序）。
///
/// docx-rs 0.4.22 `SectionProperty` 只携 num/space（equalWidth/sep/
/// `w:col` 子表缺失）——本函数为 office-io 自有 quick-xml 第二趟
/// （与 style.rs styles/theme 自有层同族技术；sectPr 序 = 节序，
/// 与 build_sections 的 marks+末节序列对齐）。`w:sectPrChange` 内
/// 嵌套的旧节属性不计（修订面——非当前生效）。
fn parse_columns_specs(xml: &str) -> Vec<Option<ColumnsSpec>> {
    use quick_xml::events::Event;

    let mut out: Vec<Option<ColumnsSpec>> = Vec::new();
    let mut in_sect_pr = false; // sectPr 开（配对形态——自闭形态即时闭）
    let mut in_sect_pr_change = false; // 修订旧属性区——嵌套 sectPr 不计
    let mut cur: Option<ColumnsSpec> = None; // 当前 sectPr 的 cols（未遇 = None）
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    // 元素处理（Start/Empty 共用——is_empty 区分自闭 sectPr 即时闭）。
    let handle = |e: &quick_xml::events::BytesStart<'_>,
                  is_empty: bool,
                  out: &mut Vec<Option<ColumnsSpec>>,
                  in_sect_pr: &mut bool,
                  in_change: &mut bool,
                  cur: &mut Option<ColumnsSpec>| {
        match e.local_name().as_ref() {
            b"sectPrChange" => *in_change = true,
            b"sectPr" if !*in_change => {
                if is_empty {
                    // 自闭 `<w:sectPr/>`：空属性区——即时闭（无后续
                    // End 事件）。
                    out.push(None);
                } else {
                    *in_sect_pr = true;
                    *cur = None;
                }
            }
            b"cols" if *in_sect_pr && !*in_change => {
                let mut spec = ColumnsSpec::default();
                for a in e.attributes() {
                    let Ok(a) = a else { continue };
                    match attr_local(&a) {
                        b"num" => spec.num = parse_u32_attr(&a.value),
                        b"space" => spec.space = parse_u32_attr(&a.value),
                        b"equalWidth" => spec.equal_width = parse_on_off(&a.value),
                        b"sep" => spec.separator = parse_on_off(&a.value),
                        _ => {}
                    }
                }
                // 首个 cols 记，后见覆盖（sectPr 内重复 cols 非法定
                // 形态——Word 容错同向）。
                *cur = Some(spec);
            }
            b"col" if *in_sect_pr && !*in_change => {
                let mut cw = ColumnWidth::default();
                for a in e.attributes() {
                    let Ok(a) = a else { continue };
                    match attr_local(&a) {
                        b"w" => cw.width = parse_u32_attr(&a.value),
                        b"space" => cw.space = parse_u32_attr(&a.value),
                        _ => {}
                    }
                }
                if let Some(spec) = cur.as_mut() {
                    spec.columns.push(cw);
                }
            }
            _ => {}
        }
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => handle(
                &e,
                false,
                &mut out,
                &mut in_sect_pr,
                &mut in_sect_pr_change,
                &mut cur,
            ),
            Ok(Event::Empty(e)) => handle(
                &e,
                true,
                &mut out,
                &mut in_sect_pr,
                &mut in_sect_pr_change,
                &mut cur,
            ),
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"sectPrChange" => in_sect_pr_change = false,
                b"sectPr" if !in_sect_pr_change => {
                    in_sect_pr = false;
                    out.push(cur.take());
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            // XML 流错误：已收集面透传（门 3 良构校验先行——此处不会
            // 到达，防御姿态）。
            Err(_) => break,
            Ok(_) => {}
        }
        buf.clear();
    }
    out
}

/// #268：ST_OnOff 属性解析（"true"/"1"/"false"/"0"；其他 → None
/// 不猜——与主题色系「登记不猜值」同族）。
fn parse_on_off(v: &[u8]) -> Option<bool> {
    match v {
        b"true" | b"1" => Some(true),
        b"false" | b"0" => Some(false),
        _ => None,
    }
}

/// #268：属性字节 → u32（非数 → None）。
fn parse_u32_attr(v: &[u8]) -> Option<u32> {
    std::str::from_utf8(v).ok()?.trim().parse().ok()
}

/// #268：属性限定名 → local name（剥 `w:` 前缀——style.rs `key_is`
/// 同构；无前缀原样）。
fn attr_local<'a>(a: &'a quick_xml::events::attributes::Attribute<'a>) -> &'a [u8] {
    let k = a.key.as_ref();
    match k.iter().position(|&b| b == b':') {
        Some(i) => &k[i + 1..],
        None => k,
    }
}

/// 文档节全量构建（#257 读面收口）：中段节标记（文档序）+ 末节 →
/// `Vec<SectionData>`（恒 ≥1 条；块区间连续切分 blocks 全序）。
///
/// `boundaries` 与 `marks` 等长（VML 插入后重映射的最终坐标系）；
/// 末节 `headers`/`footers` 复用文档级提取结果（单一真源复制——
/// 逐节统一访问面；中段节经 [`mid_section_slots`] 第二趟自解析）。
#[allow(clippy::too_many_arguments)]
fn build_sections(
    docx: &docx_rs::Docx,
    marks: &[(usize, docx_rs::SectionProperty)],
    boundaries: &[usize],
    block_count: usize,
    bytes: &[u8],
    bundle: &RelsBundle,
    index: &NumberingIndex,
    styles: &StyleIndex,
    strict: bool,
    warnings: &mut Vec<CompatibilityWarning>,
    final_headers: &[HeaderFooterData],
    final_footers: &[HeaderFooterData],
    document_xml: &str,
    column_break_blocks: &[usize],
) -> Result<Vec<SectionData>, io::ReadError> {
    debug_assert_eq!(
        marks.len(),
        boundaries.len(),
        "section marks/boundaries lockstep"
    );
    // 中段节部件集（六槽位引用 → part path 全量解析，zip 单趟）。
    let mut part_paths: Vec<String> = Vec::new();
    for (_, sp) in marks {
        // rid 统一收集（header/footer 引用类型不同——id 同域 String）。
        let mut rids: Vec<&str> = Vec::new();
        for r in [
            sp.header_reference.as_ref(),
            sp.first_header_reference.as_ref(),
            sp.even_header_reference.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            rids.push(r.id.as_str());
        }
        for r in [
            sp.footer_reference.as_ref(),
            sp.first_footer_reference.as_ref(),
            sp.even_footer_reference.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            rids.push(r.id.as_str());
        }
        for rid in rids {
            if let Some(path) = slot_part_path(rid, bundle, strict, warnings)? {
                if !part_paths.contains(&path) {
                    part_paths.push(path);
                }
            }
        }
    }
    let part_xml = load_section_part_xml(bytes, &part_paths);

    // #268：cols 全字段（自有 quick-xml 第二趟——文档序与 marks+末节
    // 对齐；计数分歧 = 解析面分歧，兼容警告 + 全 None 降级[docx-rs
    // 的 num 基础面仍有效]，严格模式同降级[非容器损坏是内部一致性
    // 面向——typed 拒会误伤可读档]）。
    let cols_specs = parse_columns_specs(document_xml);
    let section_count = marks.len() + 1;
    let cols_specs = if cols_specs.len() == section_count {
        cols_specs
    } else {
        warnings.push(crate::io::CompatibilityWarning {
            part: "word/document.xml".to_string(),
            kind: WarningKind::MalformedXml,
            detail: format!(
                "compat note: sectPr count mismatch (xml walk {} vs model {}); \
                 columns_spec degraded to None",
                cols_specs.len(),
                section_count
            ),
        });
        vec![None; section_count]
    };
    let col_spec = |i: usize| cols_specs.get(i).cloned().flatten();
    // 分栏数：XML 真源（cols/@w:num；0 = 敌意值防御钳 1；元素缺席/
    // 计数分歧降级 = 缺省 1）。docx-rs `sp.columns` 读面恒 1（
    // SectionProperty::read 无 cols 分支）——不可用作真源。
    let col_num = |i: usize| {
        cols_specs
            .get(i)
            .and_then(|s| s.as_ref())
            .and_then(|s| s.num)
            .map(|n| n.max(1) as usize)
            .unwrap_or(1)
    };
    let section_breaks = |start: usize, end: usize| {
        column_break_blocks
            .iter()
            .copied()
            .filter(|&b| b >= start && b < end)
            .collect()
    };

    let mut sections = Vec::with_capacity(marks.len() + 1);
    let mut start = 0usize;
    for (i, ((_, sp), end)) in marks.iter().zip(boundaries.iter()).enumerate() {
        let (headers, footers) =
            mid_section_slots(sp, &part_xml, bundle, index, styles, strict, warnings)?;
        sections.push(SectionData {
            index: i,
            source: SectionSource::Mid,
            geometry: section_geometry(sp),
            title_page: sp.title_pg,
            columns: col_num(i),
            columns_spec: col_spec(i),
            column_break_blocks: section_breaks(start, *end),
            section_type: section_kind(sp),
            page_number_start: sp.page_num_type.as_ref().and_then(|t| t.start),
            headers,
            footers,
            block_range: (start, *end),
        });
        start = *end;
    }
    // 末节（body 级 sectPr——docx-rs 模型恒在）。
    let sp = &docx.document.section_property;
    let final_index = sections.len();
    let final_start = start;
    sections.push(SectionData {
        index: final_index,
        source: SectionSource::Final,
        geometry: section_geometry(sp),
        title_page: sp.title_pg,
        columns: col_num(final_index),
        columns_spec: col_spec(final_index),
        column_break_blocks: section_breaks(final_start, block_count),
        section_type: section_kind(sp),
        page_number_start: sp.page_num_type.as_ref().and_then(|t| t.start),
        headers: final_headers.to_vec(),
        footers: final_footers.to_vec(),
        block_range: (final_start, block_count),
    });
    Ok(sections)
}

// ---------------------------------------------------------------------------
// 写 API（新建语义——DocxWriter）
// ---------------------------------------------------------------------------

/// 简单域代码（docx-r2 写面）：PAGE / NUMPAGES 两面（引擎元素族
/// 其余域 = TOC[文档级]/PAGEREF[书签面]/HYPERLINK[`todo!()` 崩溃面
/// instr_text.rs:32]——登记不暴露）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldSpec {
    /// 当前页码域（`<w:instrText>PAGE</w:instrText>`）。
    Page,
    /// 总页数域（`<w:instrText>NUMPAGES</w:instrText>`）。
    NumPages,
    /// 目录域（TOC——issue #6 G1；#309③ 域缓存写入）：fldChar
    /// begin[dirty=false——#309③ 前为 true：Word/LO 打开弹「是否更新域」
    /// 提示，但缓存内容本就在场，弹窗纯属打扰；现改打开即显缓存（无
    /// 弹窗），域结构完整保留（F9 重算生成真目录）] + instrText
    /// `TOC \o "lo-hi"` + separate + 占位提示文本 + end 五连（比
    /// PAGE/NUMPAGES 三连多域结果占位两 run——无占位的裸域合法但
    /// 打开即空白）。`heading_range` = `\o` 开关（None =
    /// 不发射开关——Word 默认收录全部标题层级）。
    TableOfContents { heading_range: Option<(u8, u8)> },
    /// 书签页码引用域（PAGEREF——#196 任务 2）：instrText
    /// `PAGEREF {bookmark} \h` 五连（begin + instr + separate +
    /// 页码占位 "1" + end）；`\h` = 超链接行为（点击跳转书签处）。
    /// 占位页码 "1" 是保存时快照——Word/LO 打开后 F9 重算（TOC 域
    /// #309③ 后同样不置 dirty：全族打开即显缓存零弹窗）。
    /// 引用的书签名：非空粘性校验（V11——count_specs 漏斗）；悬空
    /// 引用（书签不在本文档）合法——与 anchor_link 悬空锚点同语义
    /// （T3 先例：跨文档书签引用是 Word 合法形态，读面 PAGEREF 是
    /// volatile 域——域结果剥离，读面无语义损失）。
    PageRef { bookmark: String },
}

/// #196 任务 2：TOC 静态目录项（[`DocxWriter::toc_entries`] 元素——
/// 目录项文本 + 层级；书签名由写器生成，不经本结构）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// 标题层级（1-9——V8 同域；驱动目录项缩进 (level-1)×240 twips）。
    pub level: u8,
    /// 目录项文本（标题段净文本——调用方负责剥行内定界符）。
    pub text: String,
}

impl TocEntry {
    /// 便捷构造。
    pub fn new(level: u8, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// G2 文档级格式面（issue #6：页面设置 / 正文默认样式 / 行距——
// 引擎三 API 已在场[Docx::default_size·default_fonts·default_line_spacing
// ·section_property]，office-io 此前未接线 → 全文 10pt Times New Roman
// + 页边距写死的实测根因）
// ---------------------------------------------------------------------------

/// 页面几何（sectPr 面——单位 **twips**（1/20 pt，Word 域口径：
/// A4 = 11906×16838；引擎默认边距 = Word 2019 值 1985/1701/1701/1701）。
#[derive(Debug, Clone, PartialEq)]
pub struct PageGeometry {
    /// 页宽（pgSz w）。
    pub width: u32,
    /// 页高（pgSz h）。
    pub height: u32,
    /// 上边距（pgMar top）。
    pub margin_top: i32,
    /// 右边距（pgMar right）。
    pub margin_right: i32,
    /// 下边距（pgMar bottom）。
    pub margin_bottom: i32,
    /// 左边距（pgMar left）。
    pub margin_left: i32,
    /// 页眉距边（pgMar header）。
    pub margin_header: i32,
    /// 页脚距边（pgMar footer）。
    pub margin_footer: i32,
    /// 装订线（pgMar gutter）。
    pub gutter: i32,
}

impl PageGeometry {
    /// A4 + 引擎默认边距（= 不调用 `page_geometry` 时的产物基线）。
    pub fn a4_default() -> Self {
        Self {
            width: 11906,
            height: 16838,
            margin_top: 1985,
            margin_right: 1701,
            margin_bottom: 1701,
            margin_left: 1701,
            margin_header: 851,
            margin_footer: 992,
            gutter: 0,
        }
    }
}

/// 节槽位组（#257 写面）：[`SectionBreakSpec`] 的六槽位——`None` =
/// 不挂接（OOXML 缺省 = 继承前节同位槽）。单段语义同文档级
/// [`DocxWriter::header`]/[`DocxWriter::footer`] 六方法。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionSlots {
    /// 默认页眉（default 槽位）。
    pub default_header: Option<Vec<RunSpec>>,
    /// 首页页眉（first 槽位——`title_page = true` 时激活）。
    pub first_header: Option<Vec<RunSpec>>,
    /// 偶数页页眉（even 槽位——settings evenAndOddHeaders 激活语义
    /// 归消费方，与文档级六方法同口径）。
    pub even_header: Option<Vec<RunSpec>>,
    /// 默认页脚。
    pub default_footer: Option<Vec<RunSpec>>,
    /// 首页页脚。
    pub first_footer: Option<Vec<RunSpec>>,
    /// 偶数页页脚。
    pub even_footer: Option<Vec<RunSpec>>,
}

/// 节断点规格（#257 写面）：[`DocxWriter::section_break`] 入参——
/// 闭合一节的全部节属性。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionBreakSpec {
    /// 页面几何（None = docx-rs 默认 A4 + 默认边距；正值域校验同
    /// [`PageGeometry`] V9）。
    pub geometry: Option<PageGeometry>,
    /// 首页不同（`w:titlePg`）。
    pub title_page: bool,
    /// 起始页码（`w:pgNumType/@w:start`；None = 续前节）。
    pub page_number_start: Option<u32>,
    /// 本节页眉槽位组。
    pub headers: SectionSlots,
    /// 本节页脚槽位组。
    pub footers: SectionSlots,
}

/// 行距规则（w:spacing w:lineRule——`line` 值语义随规则变化）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineRuleSpec {
    /// `auto`：`line` = 行倍数 × 240（240 = 单倍；1.5 倍 = 360）。
    Auto,
    /// `atLeast`：`line` = twips 最小值（不足行高自动撑开）。
    AtLeast,
    /// `exact`：`line` = twips 精确值（超出裁剪）。
    Exact,
}

/// 行距与段距（docDefaults 段级 pPr——全文默认；段级覆盖面在
/// `RunSpec` 之外属段落属性，本面只提供文档级默认）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineSpacingSpec {
    /// 行距主值（语义随 [`LineRuleSpec`]——twips 或 倍数×240）。
    pub line: Option<i32>,
    /// 行距规则。
    pub line_rule: Option<LineRuleSpec>,
    /// 段前距（twips）。
    pub before: Option<u32>,
    /// 段后距（twips）。
    pub after: Option<u32>,
}

impl LineSpacingSpec {
    /// 倍数行距（auto 规则——`multiple` 为行倍数：1.0 单倍 / 1.5 / 2.0）。
    pub fn multiple(multiple: f64) -> Self {
        Self {
            line: Some((multiple * 240.0).round() as i32),
            line_rule: Some(LineRuleSpec::Auto),
            before: None,
            after: None,
        }
    }

    /// 精确行距（exact 规则——twips：240 twips = 12pt）。
    pub fn exact_twips(twips: i32) -> Self {
        Self {
            line: Some(twips),
            line_rule: Some(LineRuleSpec::Exact),
            before: None,
            after: None,
        }
    }
}

/// 正文默认排版（docDefaults rPr 面——**公开 API 单位换算**：
/// `size_half_points` 半磅[10pt=20；#8 实测「全文 10pt」= 引擎缺省
/// 面的修复入口]；字体三槽 ascii/east_asia/cs——中文标书场景
/// east_asia 必设）。
#[derive(Debug, Clone, PartialEq)]
pub struct DefaultTypography {
    /// 西文字体（rFonts ascii）。
    pub ascii_font: String,
    /// 中文字体（rFonts eastAsia）。
    pub east_asia_font: String,
    /// 复杂文种字体（rFonts cs——可与 ascii 同值）。
    pub complex_script_font: String,
    /// 默认字号（w:sz——**半磅**：10.5pt = 21，四号 = 28）。
    pub size_half_points: u16,
    /// 默认行距/段距（w:spacing——None = 引擎缺省[单倍无段距]）。
    pub line_spacing: Option<LineSpacingSpec>,
}

// ---------------------------------------------------------------------------
// t20 写面常量（公开 = 写面契约的一部分——消费方 XML 级断言可依赖）
// ---------------------------------------------------------------------------

/// 有序列表 numId（= abstractNumId；**避开引擎内建 numId=1**——
/// numberings.rs:49-176 无条件内置 decimal 9 级抽象）。
pub const ORDERED_NUM_ID: usize = 100;
/// 无序列表 numId（= abstractNumId；同上避开内建）。
pub const BULLET_NUM_ID: usize = 101;
/// 列表抽象层级数（ilvl 0-8——与引擎内建抽象同为 9 级）。
pub const MAX_LIST_LEVEL: usize = 8;
/// 行内等宽字体（[`RunSpec::mono`] 位 = true 时 rFonts 三槽值）。
pub const MONO_FONT: &str = "Consolas";
/// 块级代码等宽字体（[`DocxWriter::code_line`] 双通道；与行内分名
/// ——XML 断言可区分两条通道）。
pub const CODE_FONT: &str = "Courier New";
/// 引用左缩进步进（twips：720 = 0.5 英寸/深度）。
pub const QUOTE_INDENT_STEP: i32 = 720;
/// 引用嵌套深度上限（左缩进钳制——8×720=5760 twips=4 英寸，
/// 防溢出版心；超限静默钳位，见 [`DocxWriter::quote_paragraph`]）。
pub const MAX_QUOTE_DEPTH: usize = 8;
/// 列表悬挂缩进（twips：360 = 0.25 英寸——编号列宽）。t20 编号抽象
/// 层级与 [`DocxWriter::list_item_lines`] 段落级 ind 的单一真源
///（两侧同值：marker 列 = left−360、文字列 = left——悬挂几何见
/// [`DocxWriter::list_item_lines`] 模块文档）。
pub const LIST_HANGING_INDENT: i32 = 360;

/// 列表类型（t20 列表面——numbering 定义面二选一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// 有序（decimal `1.` / `1.1.` … outline 式 9 级）。
    Ordered,
    /// 无序（`•` 项目符号 9 级）。
    Bullet,
}

/// 富文本 run 规格（写侧输入模型）。
#[derive(Debug, Clone, PartialEq)]
pub struct RunSpec {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// 等宽字体位（t20——true = `w:rFonts` 三槽 [`MONO_FONT`]：
    /// ascii/hAnsi/cs）。行内 code mark 的 docx 表达面。
    pub mono: bool,
    /// 删除线位（t20——true = `w:strike` 单删除线）。行内 del mark。
    pub strike: bool,
    /// 下画线位（t20——true = `w:u w:val="single"`）。行内 u mark。
    pub underline: bool,
    /// 外部超链接目标（None = 普通 run；写侧经引擎关系面发射
    /// TargetMode="External"——引擎写面 "Now support external only"
    /// [documents/mod.rs:1209]，锚点写面不暴露）。
    pub link: Option<String>,
    /// **内部锚点链接目标**（office-g Phase 1——书签名载荷；Some = 本
    /// run 以 `w:hyperlink w:anchor` 属性形发射[零 rels 面——引擎
    /// HyperlinkType::Anchor]，与 `link` 同 Some 时 **anchor 赢、link
    /// 忽略**（V8 优先序——读写镜像：读面 r5-t2 探针② 钉定 w:anchor
    /// 优先于 r:id）。悬空锚点（目标书签不存在）= **合法形**（T3——
    /// 存在性检查是消费侧责任，读面不暴露书签定义）。经
    /// [`RunSpec::anchor_link`] 构造；空串 = V7 违规（save 漏斗经写器
    /// 粘性校验错误拒绝——见 `DocxWriter::validation_error`）。
    pub anchor: Option<String>,
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
    /// **字符样式引用**（#196 任务 3——`w:rStyle`）：Some = run 发射
    /// `Run::style(id)`（与直接格式位 bold/italic 并存——Word 中
    /// 直接格式压样式，共存合法）；引用合法性 = 预置字符样式集
    /// （Strong/Emphasis——Word 内置名）∪ `define_style` 已定义
    /// Character 型 id（V12 粘性：悬空引用 save 漏斗拒——Word 静默
    /// 忽略悬空 rStyle = 视觉丢失，写自洽不产出）。经
    /// [`RunSpec::char_styled`] 构造。
    pub char_style: Option<String>,
}

impl RunSpec {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            mono: false,
            strike: false,
            underline: false,
            link: None,
            anchor: None,
            footnote: None,
            field: None,
            char_style: None,
        }
    }

    pub fn styled(text: impl Into<String>, bold: bool, italic: bool) -> Self {
        Self {
            text: text.into(),
            bold,
            italic,
            mono: false,
            strike: false,
            underline: false,
            link: None,
            anchor: None,
            footnote: None,
            field: None,
            char_style: None,
        }
    }

    /// 全样式位构造（t20——mono/strike/underline 三位扩面；
    /// 调用方按 mark 语义置位）。
    pub fn styled_full(
        text: impl Into<String>,
        bold: bool,
        italic: bool,
        mono: bool,
        strike: bool,
        underline: bool,
    ) -> Self {
        Self {
            text: text.into(),
            bold,
            italic,
            mono,
            strike,
            underline,
            link: None,
            anchor: None,
            footnote: None,
            field: None,
            char_style: None,
        }
    }

    /// 外部链接 run（`text` = 链接显示文本，`url` = 目标 URL——
    /// 读回为 [`HyperlinkTarget::External`]）。
    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            mono: false,
            strike: false,
            underline: false,
            link: Some(url.into()),
            anchor: None,
            footnote: None,
            field: None,
            char_style: None,
        }
    }

    /// **内部锚点链接 run**（office-g Phase 1——目录/交叉引用形态：
    /// 链接文本 + 目标书签名；`w:hyperlink w:anchor` 属性形，零 rels
    /// 面，悬空合法[T3]）。与 [`RunSpec::link`] 并存；同 Some 时
    /// anchor 赢（V8 优先序——读写镜像，读面探针② 钉定）。
    /// 空锚点名 = V7 违规：构造不失败（人体工学签名），save 漏斗经
    /// 写器粘性校验错误拒绝（见 `DocxWriter::validation_error`）。
    pub fn anchor_link(text: impl Into<String>, anchor: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            mono: false,
            strike: false,
            underline: false,
            link: None,
            anchor: Some(anchor.into()),
            footnote: None,
            field: None,
            char_style: None,
        }
    }

    /// 脚注引用 marker run（`note` = 脚注内容单段纯文本；样式面忽略
    /// ——见字段文档）。
    pub fn footnote(note: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            bold: false,
            italic: false,
            mono: false,
            strike: false,
            underline: false,
            link: None,
            anchor: None,
            footnote: Some(note.into()),
            field: None,
            char_style: None,
        }
    }

    /// 当前页码域 run（`PAGE`）。
    pub fn field_page() -> Self {
        Self {
            text: String::new(),
            bold: false,
            italic: false,
            mono: false,
            strike: false,
            underline: false,
            link: None,
            anchor: None,
            footnote: None,
            field: Some(FieldSpec::Page),
            char_style: None,
        }
    }

    /// 总页数域 run（`NUMPAGES`）。
    pub fn field_num_pages() -> Self {
        Self {
            text: String::new(),
            bold: false,
            italic: false,
            mono: false,
            strike: false,
            underline: false,
            link: None,
            anchor: None,
            footnote: None,
            field: Some(FieldSpec::NumPages),
            char_style: None,
        }
    }

    /// 目录域 run（`TOC`——全标题层级，Word 默认收录；#309③ 后
    /// begin 不置 dirty——打开即显占位缓存无弹窗，F9 生成真目录）。
    pub fn field_toc() -> Self {
        Self {
            field: Some(FieldSpec::TableOfContents {
                heading_range: None,
            }),
            ..Self::field_page()
        }
    }

    /// 目录域 run（`TOC \o "lo-hi"`——收录标题层级 lo..=hi）。
    /// 域校验 V8（1-9 + lo ≤ hi）在 save 漏斗粘性拒绝（与 V7 同
    /// 语义——构造器保持人体工学签名）。
    pub fn field_toc_heading_range(lo: u8, hi: u8) -> Self {
        Self {
            field: Some(FieldSpec::TableOfContents {
                heading_range: Some((lo, hi)),
            }),
            ..Self::field_page()
        }
    }

    /// 书签页码引用域 run（`PAGEREF {bookmark} \h`——#196 任务 2；
    /// \h 超链接行为恒开：无 \h 的 PAGEREF 只显示页码不可点击，
    /// 导出面无「仅显页码不要跳转」用例，不暴露开关——最小面）。
    /// 空书签名 = V11 违规：构造不失败（人体工学签名），save 漏斗
    /// 经写器粘性校验拒绝（与 V7/V8 同语义）。
    pub fn field_pageref(bookmark: impl Into<String>) -> Self {
        Self {
            field: Some(FieldSpec::PageRef {
                bookmark: bookmark.into(),
            }),
            ..Self::field_page()
        }
    }

    /// 字符样式引用 run（#196 任务 3——`w:rStyle`；与直接格式位
    /// 并存：Word 中直接格式压样式）。样式 id 合法性 = 预置字符集
    /// （Strong/Emphasis）∪ 已 define 的 Character 型 id（V12 粘性
    /// save 漏斗拒）；空串拒（与 V7/V11 同语义）。
    pub fn char_styled(text: impl Into<String>, style_id: impl Into<String>) -> Self {
        Self {
            char_style: Some(style_id.into()),
            ..Self::text(text)
        }
    }
}

// ---------------------------------------------------------------------------
// #133 任务 2：高级表格写面（合并/底纹/边框/列宽——标书表格要件）
// ---------------------------------------------------------------------------

/// 单元格纵向合并状态（`w:vMerge`——#133 任务 2）。
///
/// 语义与读面 [`MergeRange`] 的 restart-continue 链对齐：`Restart` =
/// 合并原点（内容承载格）；`Continue` = 续格（Word 渲染不可见——
/// 读面栅格展开置空）。**续格禁止携带文本**（V10e——见
/// [`TableCellSpec`]）：读面把续格文本丢弃（Word 显示语义），写面
/// 放行 = 内容静默丢失，宁拒勿吞。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellVMerge {
    /// 不参与纵向合并（缺省——不发 `w:vMerge`）。
    #[default]
    None,
    /// 合并原点（`w:vMerge w:val="restart"`——内容承载格）。
    Restart,
    /// 续格（`w:vMerge w:val="continue"`）——文本须为空（V10e）。
    Continue,
}

/// 边框线型（`w:val` 跨引擎稳妥子集——#133 任务 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellBorderStyle {
    /// 无边框（`nil`——显式压掉表级继承边框）。
    Nil,
    /// 单实线（缺省——Word 表格最常见形态）。
    #[default]
    Single,
    /// 双线。
    Double,
    /// 虚线。
    Dashed,
    /// 点线。
    Dotted,
    /// 粗线。
    Thick,
}

/// 单边边框（#133 任务 2）。
///
/// `size_eighths` = 1/8 磅（`w:sz` 语义：4 = 1/2 磅；域 1..=96，
/// V10c）；`color` = `#RRGGBB` 或 `auto`（V10b）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellBorderEdge {
    pub style: CellBorderStyle,
    pub size_eighths: u8,
    pub color: String,
}

impl CellBorderEdge {
    /// 便捷构造：单实线 1/2 磅 + `auto` 色（Word「所有框线」默认档）。
    pub fn single() -> Self {
        CellBorderEdge {
            style: CellBorderStyle::Single,
            size_eighths: 4,
            color: "auto".to_string(),
        }
    }
}

/// 单元格边框四边（#133 任务 2；`None` = 该边不设——继承表级样式）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellBorders {
    pub top: Option<CellBorderEdge>,
    pub left: Option<CellBorderEdge>,
    pub bottom: Option<CellBorderEdge>,
    pub right: Option<CellBorderEdge>,
}

/// 高级单元格规格（#133 任务 2——合并/底纹/边框/格宽）。
///
/// V10 域校验（save 漏斗粘性拒绝——与 V7/V9 同纪律，发射保持人体
/// 工学签名 [`DocxWriter::table_advanced`]）：
///
/// - **V10a** `grid_span` ≥ 1（0 非法；1 = 不合并——不发 `w:gridSpan`）；
/// - **V10b** `background` = `#RRGGBB`（6 位十六进制，前缀 `#` 必带）；
///   边框 `color` = `#RRGGBB` 或 `auto`；
/// - **V10c** `size_eighths` 1..=96（`w:sz` 1/8 磅域）；
/// - **V10d** `width` ≥ 1 twips（`w:tcW` dxa 域）；
/// - **V10e** `v_merge = Continue` 且 `text` 非空 → 拒（续格文本
///   Word 不可见且读面不回读——内容请置于 `Restart` 原点格）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCellSpec {
    pub text: String,
    /// 横向合并跨列数（`w:gridSpan`；1 = 不合并）。**缺省 = 1**
    ///（手写 Default——`u32` 零值 0 是 V10a 非法域，derive 会踩坑）。
    pub grid_span: u32,
    pub v_merge: CellVMerge,
    /// 底纹填充色（`w:shd w:val="clear" w:fill`；`None` = 无底纹）。
    pub background: Option<String>,
    pub borders: Option<CellBorders>,
    /// 格宽（twips——`w:tcW` dxa；`None` = 自适应）。
    pub width: Option<u32>,
}

impl Default for TableCellSpec {
    /// 缺省 = 纯文本不合并格（`grid_span: 1`——非 u32 零值）。
    fn default() -> Self {
        TableCellSpec {
            text: String::new(),
            grid_span: 1,
            v_merge: CellVMerge::None,
            background: None,
            borders: None,
            width: None,
        }
    }
}

impl TableCellSpec {
    /// 纯文本格便捷构造（与 [`DocxWriter::table`] 的 `Vec<String>` 格
    /// 等价——零合并/零装饰）。
    pub fn text(text: impl Into<String>) -> Self {
        TableCellSpec {
            text: text.into(),
            ..Default::default()
        }
    }
}

/// 高级表规格（#133 任务 2）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableSpec {
    /// 行 × 格（**物理格序**——合并由格内 `grid_span`/`v_merge` 表达，
    /// 不是栅格展开形；读回经 [`TableData`] 得展开栅格 + `merges`
    /// 矩形清单——roundtrip 对账面）。
    pub rows: Vec<Vec<TableCellSpec>>,
    /// 列宽（twips——`w:tblGrid/w:gridCol @w:w`，按栅格列序）。
    /// 空 = 不发 tblGrid（引擎行和兜底——读面同语义，
    /// `missing_tbl_grid_falls_back_to_row_sums` 钉定）。
    pub column_widths: Vec<u32>,
}

/// `#RRGGBB` 校验 + 引擎形（大写无前缀——OOXML `ST_HexColor`）。
/// 非法输入返回 `None`（调用方落 V10b 粘性错误）。
fn hex_color_6(s: &str) -> Option<String> {
    let body = s.strip_prefix('#')?;
    if body.len() != 6 || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(body.to_ascii_uppercase())
}

/// 边框线型映射（公开子集 → 引擎枚举）。
fn map_border_style(s: CellBorderStyle) -> docx_rs::BorderType {
    match s {
        CellBorderStyle::Nil => docx_rs::BorderType::Nil,
        CellBorderStyle::Single => docx_rs::BorderType::Single,
        CellBorderStyle::Double => docx_rs::BorderType::Double,
        CellBorderStyle::Dashed => docx_rs::BorderType::Dashed,
        CellBorderStyle::Dotted => docx_rs::BorderType::Dotted,
        CellBorderStyle::Thick => docx_rs::BorderType::Thick,
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
    /// t20：numbering 定义面惰性注册位（首个 [`Self::list_item`] 时
    /// 注入 ordered/bullet 两抽象 + num 桥——无列表文档的 numbering.xml
    /// 保持引擎内建形态；`save_to_vec` 后随基态重置）。
    numbering_registered: bool,
    /// G2：文档级格式面登记（V9 域校验数据源——几何/排版越界
    /// save 漏斗粘性拒绝）。
    page_geometry: Option<PageGeometry>,
    typography: Option<DefaultTypography>,
    /// r4-t2：段等价物累计（含表单元格/页眉页脚槽位/脚注内容段；
    /// 替换槽位按每次调用计——上界语义）。
    paragraphs: u64,
    /// r4-t2：run 累计（发射 run + 图片 run + 脚注内容 run）。
    runs: u64,
    /// office-g Phase 1/2：**粘性校验错误消息**（V7 空锚点等构造期
    /// 不可 Result 化的违规——构造器保持 §7.1 人体工学签名，首个
    /// 违规在此落账，save 漏斗首查以 `DocxError::InvalidInput` 拒绝
    /// [零产物语义维持]；fail-closed：不产出半合法文档。存消息而非
    /// 错误值——`DocxError` 无 Clone（公开面不加 derive），且粘性面
    /// 恒为 InvalidInput 族）。
    validation_error: Option<String>,
    /// office-g Phase 2：书签名注册表（V6 重名拒绝 + id 唯一化的
    /// 名字面账本）。
    bookmark_names: std::collections::HashSet<String>,
    /// office-g Phase 2：bookmarkStart/End 的 w:id 计数器（引擎内
    /// 部 id——§8：仅单次导出会话内稳定，**不作为身份**；公开身份
    /// = 书签名）。
    bookmark_next_id: usize,
    /// #196 任务 2：toc_entries 书签名生成计数器（`toc_N` 递增基准
    /// ——与用户书签避让：生成时查 bookmark_names，撞名则递增跳过；
    /// save_to_vec 重建基态时随 Self::new() 重置）。
    toc_bookmark_counter: usize,
    /// N-4（#29）：已发射表格计数（文档序 0 基——与 document.xml
    /// `<w:tbl>` 出现序对齐；save 后随基态重置）。
    tables_emitted: usize,
    /// N-4：需首行打 `tblHeader`（跨页表头重复）的表序号集合——
    /// [`Self::table_header_repeat`] 记账，save 漏斗内 zip 后处理
    /// 注入（docx-rs 0.4.22 `TableRowProperty` 无 tblHeader 面——
    /// 版本钉死不可升，见 `mark_table_header_rows` 文档）。
    header_tables: std::collections::HashSet<usize>,
    /// S1 样式面：自定义样式 id 注册表（define_style 重名拒绝 +
    /// styled_paragraph 引用校验的账本——`save_to_vec` 重建基态时
    /// 随 Self::new() 重置为空集[新档无自定义样式，语义同
    /// numbering_registered]）。
    defined_styles: std::collections::HashSet<String>,
    /// S1 编号面：自定义编号定义账本（id → 最大 ilvl——重名拒绝 +
    /// numbered_list_item 引用/级域校验；save_to_vec 重建基态时随
    /// Self::new() 重置[新档无自定义编号]）。
    defined_numberings: std::collections::HashMap<usize, usize>,
}

impl DocxWriter {
    pub fn new() -> Self {
        let mut docx = docx_rs::Docx::new();
        // Heading1-9 样式定义（outline 级；based_on Normal；qFormat
        // ——样式面板可见）。#196 任务 3：视觉面补齐——此前 rPr 空 =
        // 样式面板可见但视觉平（标题与正文无差），「改样式批量生效」
        // 仅形式满足；现补字号梯度（Word 内置默认 16/13/12/11pt×6
        // ——半点 32/26/24/22）+ 加粗（H1-H4，H5-H9 常规——层级递弱）
        // ，标题有了可见初始态，改样式（如全局换标题字体）批量生效
        // 有实际视觉意义。颜色不补（主题面复杂，出版惯例黑字最稳
        // ——登记裁决点）。
        const HEADING_SIZES: [usize; 9] = [32, 26, 24, 22, 22, 22, 22, 22, 22];
        for level in 1u8..=9 {
            let mut style =
                docx_rs::Style::new(format!("Heading{level}"), docx_rs::StyleType::Paragraph)
                    .name(format!("heading {level}"))
                    .based_on("Normal")
                    .next("Normal")
                    .q_format(true)
                    .size(HEADING_SIZES[(level - 1) as usize]);
            if level <= 4 {
                style = style.bold();
            }
            style.paragraph_property = style
                .paragraph_property
                .clone()
                .outline_lvl((level - 1) as usize);
            docx = docx.add_style(style);
        }
        // #196 任务 3：预置字符样式（Word 内置名）——Strong = 加粗
        // 强调 / Emphasis = 斜体强调；rStyle 引用面（RunSpec::
        // char_styled）零定义即用，V12 校验的合法集（另有 define_style
        // 的 Character 型自定义——写自洽先定义后引用）。
        docx = docx.add_style(
            docx_rs::Style::new("Strong", docx_rs::StyleType::Character)
                .name("Strong")
                .q_format(true)
                .bold(),
        );
        docx = docx.add_style(
            docx_rs::Style::new("Emphasis", docx_rs::StyleType::Character)
                .name("Emphasis")
                .q_format(true)
                .italic(),
        );
        Self {
            docx,
            numbering_registered: false,
            page_geometry: None,
            typography: None,
            paragraphs: 0,
            runs: 0,
            toc_bookmark_counter: 0,
            validation_error: None,
            bookmark_names: std::collections::HashSet::new(),
            bookmark_next_id: 0,
            tables_emitted: 0,
            header_tables: std::collections::HashSet::new(),
            defined_styles: std::collections::HashSet::new(),
            defined_numberings: std::collections::HashMap::new(),
        }
    }

    // -------------------------------------------------------------------
    // S1 样式面/编号面访问器（style.rs·numbering.rs 分文件 impl 桥
    // ——pub(crate) 单点）
    // -------------------------------------------------------------------

    /// 引擎文档可变引用（style.rs `define_style`/`styled_paragraph`、
    /// numbering.rs 写面构造的 take/写回模式用）。
    pub(crate) fn docx_mut(&mut self) -> &mut docx_rs::Docx {
        &mut self.docx
    }

    /// 自定义样式 id 账本（只读）。
    pub(crate) fn defined_styles(&self) -> &std::collections::HashSet<String> {
        &self.defined_styles
    }

    /// 自定义样式 id 账本（可变——define_style 落账）。
    pub(crate) fn defined_styles_mut(&mut self) -> &mut std::collections::HashSet<String> {
        &mut self.defined_styles
    }

    /// 自定义编号定义账本（只读——id → 最大 ilvl）。
    pub(crate) fn defined_numberings(&self) -> &std::collections::HashMap<usize, usize> {
        &self.defined_numberings
    }

    /// 自定义编号定义账本（可变——define_numbering 落账）。
    pub(crate) fn defined_numberings_mut(
        &mut self,
    ) -> &mut std::collections::HashMap<usize, usize> {
        &mut self.defined_numberings
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
        self.paragraphs += 1;
        self.runs += 1;
        Ok(())
    }

    /// **带书签的标题段**（#196 任务 2——TOC 跳转目标侧）：与
    /// [`Self::heading`] 同构（样式引用 + outline 级双通道）+
    /// `bookmarkStart/End` 对包标题文本（书签定义点——与
    /// [`Self::bookmark_paragraph`] 同 id 唯一化/名字注册纪律，复用
    /// V1-V6 校验矩阵；`toc_N` 名由 [`Self::toc_entries`] 生成，
    /// 过全部校验）。**跳转闭环的另一侧**：[`Self::toc_entries`] 发射
    /// 的目录项 PAGEREF/hyperlink 引用本方法注册的书签名。
    pub fn heading_bookmarked(
        &mut self,
        level: u8,
        text: &str,
        bookmark: &str,
    ) -> Result<(), DocxError> {
        if !(1..=9).contains(&level) {
            return Err(DocxError::Parse(format!(
                "heading level must be 1-9, got {level}"
            )));
        }
        validate_bookmark_name(bookmark, &self.bookmark_names)?;
        let id = self.bookmark_next_id;
        self.bookmark_next_id += 1;
        self.bookmark_names.insert(bookmark.to_string());
        let mut p = docx_rs::Paragraph::new()
            .style(&format!("Heading{level}"))
            .add_run(docx_rs::Run::new().add_text(text));
        p.property = p.property.clone().outline_lvl((level - 1) as usize);
        p = p.add_bookmark_start(id, bookmark.to_string());
        p = p.add_bookmark_end(id);
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.paragraphs += 1;
        self.runs += 1;
        Ok(())
    }

    /// 单文本段落（便捷面）。
    pub fn paragraph_text(&mut self, text: &str) {
        self.paragraph(&[RunSpec::text(text)]);
    }

    /// 富文本段落（run 级加粗/斜体 + 外部超链接）。r4-t2：段/run 计数
    /// （发射调用累计——脚注规格额外 +1 段 +1 run[内容段]）。
    pub fn paragraph(&mut self, runs: &[RunSpec]) {
        let p = specs_to_paragraph(runs);
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.count_specs(1, runs);
    }

    /// **书签定义段**（office-g Phase 2——生产侧：被链接的目标）。
    /// 发射 `bookmarkStart(w:id, w:name)` … runs … `bookmarkEnd(w:id)`
    /// 对；`w:id` 由写器内部计数器唯一化（§8：仅单次导出会话内稳定，
    /// **不作为身份**——公开身份 = 书签名，与协同侧车
    /// OfficeBookmark.name 同源）。
    ///
    /// 校验矩阵 V1-V6（office-g §11，全部 typed `InvalidInput`——
    /// 宁缺毋错）：
    /// V1 非空 / V2 ≤ 40 字符（Word 上限）/ V3 首字符字母 /
    /// V4 仅 `[A-Za-z0-9_]`（保守 ASCII 域 = 跨引擎最稳交集——放宽
    /// 为后续裁决点）/ V5 `_Toc` 保留名拒绝（TOC 内部机制不踩踏）/
    /// V6 同文档重名拒绝（Word 容忍重名但跳转不可预期——显式拒绝）。
    pub fn bookmark_paragraph(&mut self, runs: &[RunSpec], name: &str) -> Result<(), DocxError> {
        validate_bookmark_name(name, &self.bookmark_names)?;
        let id = self.bookmark_next_id;
        self.bookmark_next_id += 1;
        self.bookmark_names.insert(name.to_string());
        let mut p = specs_to_paragraph(runs);
        p = p.add_bookmark_start(id, name.to_string());
        p = p.add_bookmark_end(id);
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.count_specs(1, runs);
        Ok(())
    }

    /// **TOC 静态目录项块**（#196 任务 2——目录项可点击跳转；#309③
    /// 域缓存写入）：跨段域三段式 = 域头段（begin[dirty=false——#309③：
    /// 打开即显缓存无弹窗] + instrText `TOC \o "lo-hi"`
    /// 与 separate——与旧 [`RunSpec::field_toc_heading_range`] 域段前三
    /// run 同构）、目录项段 × N、域尾段（end）。目录项段 =
    /// `hyperlink(w:anchor=书签名)`{ 文本 run + tab run + PAGEREF 五连 }
    /// 附右对齐点线 tab stop（pos 随 [`Self::page_geometry`] 动态
    /// 计算——缺省引擎基线 A4 文本宽 8504 twips）与层级缩进
    /// （(level-1)×240 twips）。
    ///
    /// **书签名**：`toc_N`（`toc_bookmark_counter` 递增，撞用户书签
    /// 则跳号避让）；**返回值** = entries 序的书签名列表——调用方在
    /// 正文对应标题处经 [`Self::heading_bookmarked`] 以同名注册书签
    /// （定义点），与本方法发射的引用侧（hyperlink anchor + PAGEREF
    /// 指令）构成跳转闭环。名字走 V4 字符域（`toc_1` 形态恒过；V5
    /// `_Toc` 保留名不撞——Word 的 `_TocNNN` 内部惯例非协议要求，
    /// 包装层自有命名空间避开它，用户书签纪律 V1-V6 零例外维持）。
    ///
    /// **entries 空 = 旧形态退化**：发射占位提示五连域段（与
    /// `field_toc_heading_range` 路径逐字节同构——无标题文档既有行为
    /// 零漂移）。
    ///
    /// 校验（save 漏斗粘性——V8 同纪律）：entry level 1-9；\
    /// `heading_range` 的 lo/hi 1-9 且 lo ≤ hi。
    pub fn toc_entries(
        &mut self,
        entries: &[TocEntry],
        heading_range: Option<(u8, u8)>,
    ) -> Vec<String> {
        // V8 域校验（heading_range——与 count_specs 的 RunSpec 路径
        // 同判据；本方法不经 RunSpec，粘性落账在此单点）。
        if let Some((lo, hi)) = heading_range {
            if self.validation_error.is_none()
                && (lo == 0 || hi == 0 || lo > 9 || hi > 9 || lo > hi)
            {
                self.validation_error = Some(format!(
                    "V8: TOC 标题层级域违规（{lo}-{hi}——须 1-9 且 lo ≤ hi）"
                ));
            }
        }
        // V8'（entry level 域——heading 同域 1-9）。
        if self.validation_error.is_none() {
            if let Some(bad) = entries.iter().find(|e| !(1..=9).contains(&e.level)) {
                self.validation_error =
                    Some(format!("V8: TOC 目录项层级越界（{}——须 1-9）", bad.level));
            }
        }
        // 空退化：占位五连域段（#309③：begin 不置 dirty——与非空路径
        // 同「打开即显缓存」语义，旧形态仅域头 dirty 位差异）。
        if entries.is_empty() {
            let mut toc = docx_rs::InstrToC::new();
            if let Some((lo, hi)) = heading_range {
                toc = toc.heading_styles_range(lo as usize, hi as usize);
            }
            let p = docx_rs::Paragraph::new()
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false))
                .add_run(docx_rs::Run::new().add_instr_text(docx_rs::InstrText::TOC(toc)))
                .add_run(
                    docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Separate, false),
                )
                .add_run(docx_rs::Run::new().add_text("（目录域——打开后全选再按 F9 更新生成）"))
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false));
            self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            self.paragraphs += 1;
            self.runs += 5;
            return Vec::new();
        }
        // tab stop 位 = 文本宽（页宽 - 左右边距 - 装订线；几何未设置
        // = 引擎基线 A4 11906 + Word 2019 边距 1701/1701——g2_t4 钉定
        // 的字节面缺省）。
        let text_width = match &self.page_geometry {
            Some(g) => {
                let w = g.width as i64
                    - g.margin_left.max(0) as i64
                    - g.margin_right.max(0) as i64
                    - g.gutter.max(0) as i64;
                w.max(0) as usize
            }
            None => 11906 - 1701 - 1701,
        };
        // 域头段（#309③：begin 不置 dirty——打开即显缓存目录项，
        // Word/LO 零弹窗；F9 重算精确页码）。
        let mut toc = docx_rs::InstrToC::new();
        if let Some((lo, hi)) = heading_range {
            toc = toc.heading_styles_range(lo as usize, hi as usize);
        }
        let head = docx_rs::Paragraph::new()
            .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false))
            .add_run(docx_rs::Run::new().add_instr_text(docx_rs::InstrText::TOC(toc)))
            .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Separate, false));
        self.docx = std::mem::take(&mut self.docx).add_paragraph(head);
        self.paragraphs += 1;
        self.runs += 3;
        // 目录项段 × N（hyperlink(anchor){ 文本 + tab + PAGEREF 五连 }）。
        let mut names = Vec::with_capacity(entries.len());
        for e in entries {
            // 书签名生成：toc_N 递增，撞用户已注册名则跳号（先到先得
            // ——用户已注册 toc_3 则生成跳到 toc_4）。**生成侧不注册**
            // （名字只预留不占用——注册/定义发生在调用方
            // `heading_bookmarked` 携同名注册时；counter 单调不回退，
            // 同会话零重复生成；用户抢注同名后到 → 定义点 V6 诚实拒）。
            let mut n = self.toc_bookmark_counter;
            loop {
                let cand = format!("toc_{n}");
                if !self.bookmark_names.contains(&cand) {
                    self.toc_bookmark_counter = n + 1;
                    names.push(cand);
                    break;
                }
                n += 1;
            }
            let name = names.last().unwrap().clone();
            let mut p = docx_rs::Paragraph::new().indent(
                Some((e.level as i32 - 1) * 240),
                None,
                None,
                None,
            );
            p = p.add_tab(
                docx_rs::Tab::new()
                    .val(docx_rs::TabValueType::Right)
                    .leader(docx_rs::TabLeaderType::Dot)
                    .pos(text_width),
            );
            let hl = docx_rs::Hyperlink::new(name.as_str(), docx_rs::HyperlinkType::Anchor)
                .add_run(docx_rs::Run::new().add_text(e.text.as_str()))
                .add_run(docx_rs::Run::new().add_tab())
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false))
                .add_run(
                    docx_rs::Run::new().add_instr_text(docx_rs::InstrText::PAGEREF(
                        docx_rs::InstrPAGEREF::new(name.as_str()).hyperlink(),
                    )),
                )
                .add_run(
                    docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Separate, false),
                )
                .add_run(docx_rs::Run::new().add_text("1"))
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false));
            p = p.add_hyperlink(hl);
            self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
            self.paragraphs += 1;
            // 文本 + tab + PAGEREF 五连 = 7 run（hyperlink 内 run 计
            // 入文档 run 面——与 link run 同纪律）。
            self.runs += 7;
        }
        // 域尾段（end——独立空段，引擎 TableOfContentsItem 同形）。
        let tail = docx_rs::Paragraph::new()
            .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false));
        self.docx = std::mem::take(&mut self.docx).add_paragraph(tail);
        self.paragraphs += 1;
        self.runs += 1;
        names
    }

    /// r4-t2：段/run 计数单点（paras 含本段；脚注规格 +1 段 +1 run）。
    /// S1 样式面：pub(crate)（style.rs `styled_paragraph` 复用）。
    /// S1 编号面：pub(crate)（numbering.rs `numbered_list_item` 复用）。
    pub(crate) fn count_specs(&mut self, paras: u64, runs: &[RunSpec]) {
        self.paragraphs += paras;
        self.runs += runs.len() as u64;
        for r in runs {
            // office-g V7（空锚点）粘性检查——构造期不可 Result 化的
            // 违规在此落账，save 漏斗拒绝（零产物语义维持）。
            if self.validation_error.is_none() {
                if let Some(a) = &r.anchor {
                    if a.is_empty() {
                        self.validation_error = Some("V7: anchor_link 目标书签名为空串".into());
                    }
                }
            }
            // G1 V8（TOC 标题层级域）粘性检查——1-9 域与 lo ≤ hi
            // （与 heading 级域同纪律；违规 save 漏斗拒绝零产物）。
            if self.validation_error.is_none() {
                if let Some(FieldSpec::TableOfContents {
                    heading_range: Some((lo, hi)),
                }) = &r.field
                {
                    let bad = *lo == 0 || *hi == 0 || *lo > 9 || *hi > 9 || lo > hi;
                    if bad {
                        self.validation_error = Some(format!(
                            "V8: TOC 标题层级域违规（{lo}-{hi}——须 1-9 且 lo ≤ hi）"
                        ));
                    }
                }
            }
            // #196 任务 2 V11（PAGEREF 空书签）粘性检查——与 V7 同
            // 语义（构造器人体工学签名，save 漏斗拒绝零产物）。
            if self.validation_error.is_none() {
                if let Some(FieldSpec::PageRef { bookmark }) = &r.field {
                    if bookmark.is_empty() {
                        self.validation_error = Some("V11: PAGEREF 目标书签名为空串".into());
                    }
                }
            }
            // #196 任务 3 V12（悬空 rStyle 引用）粘性检查——字符样式
            // id 须在预置字符集（Strong/Emphasis）∪ 已定义 Character
            // 型集合内；空串同拒（写自洽：Word 静默忽略悬空 rStyle =
            // 视觉丢失，不产出半合法文档）。
            if self.validation_error.is_none() {
                if let Some(cs) = &r.char_style {
                    if cs.is_empty() {
                        self.validation_error = Some("V12: 字符样式引用为空串".into());
                    } else {
                        let known = crate::style::PRESET_CHAR_STYLE_IDS.contains(&cs.as_str())
                            || self.defined_styles.contains(cs);
                        if !known {
                            self.validation_error = Some(format!(
                                "V12: 字符样式 '{cs}' 未定义（预置 Strong/Emphasis 或先 define_style）"
                            ));
                        }
                    }
                }
            }
            if r.footnote.is_some() {
                self.paragraphs += 1;
                self.runs += 1;
            }
        }
    }

    /// 默认页眉（单段 run 序——run 级加粗/斜体/链接/脚注/域）。
    /// **default 槽位，重复调用 = 原位替换**（引擎公开 `Docx::header` 按调用
    /// 次数递增 rels 计数——重复调用会发射 rIdHeader{n+1} 关系而无
    /// 对应部件[自产悬空引用，documents/mod.rs:576-589 计数语义]；
    /// 本包装层改为原位替换保持包自洽——同 XLSX 流式纪律的包装层
    /// 防御先例）。
    pub fn header(&mut self, runs: &[RunSpec]) {
        let h = docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs));
        self.count_specs(1, runs);
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
        self.count_specs(1, runs);
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
        self.count_specs(1, runs);
        if self.docx.document.section_property.even_header.is_some() {
            replace_slot_header(&mut self.docx, SlotKind::Even, h);
        } else {
            self.docx = std::mem::take(&mut self.docx).even_header(h);
        }
    }

    /// 默认页脚（单段 run 序——同 [`Self::header`]，重复调用 = 原位替换）。
    pub fn footer(&mut self, runs: &[RunSpec]) {
        let f = docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs));
        self.count_specs(1, runs);
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
        self.count_specs(1, runs);
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
        self.count_specs(1, runs);
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
        self.paragraphs += 1;
        self.runs += 1;
        Ok(())
    }

    /// 文本表格（行 × 单元格文本；首行不特殊化——表头样式属渲染面）。
    /// r4-t2：单元格逐格计数（每格 = 引擎侧 1 段 + 1 run）。
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
        let cells: u64 = rows.iter().map(|r| r.len() as u64).sum();
        self.paragraphs += cells;
        self.runs += cells;
        // N-4：文档序表计数（与 document.xml <w:tbl> 出现序对齐——
        // table_header_repeat 的记账坐标系）。
        self.tables_emitted += 1;
    }

    /// N-4（#29）：表格 + 首行跨页表头重复（`w:trPr/w:tblHeader`）。
    ///
    /// 语义：首行 = 表头行（GFM 首行即表头同构）；跨页时 Word/LO
    /// 在后续页顶重复该行。仅 ≥ 2 行的表打标（单行表无跨页面，
    /// 防御性不打——Word 对孤立 tblHeader 行的渲染语义未定义）。
    /// 字节面：`[`Self::table`]` 全同 + 首行 `<w:trPr />` →
    /// `<w:trPr><w:tblHeader /></w:trPr>`（save 漏斗内 zip 后处理
    /// 注入——docx-rs 0.4.22 `TableRowProperty` 无 tblHeader 面，
    /// 版本钉死[P2-2 serde 通道锁 0.4.22]不可升；见
    /// [`mark_table_header_rows`]）。**读面不回读**（docx-rs 反序列
    /// 化丢 trPr 扩展——round-trip 不保 tblHeader，模块文档登记）。
    pub fn table_header_repeat(&mut self, rows: &[Vec<String>]) {
        let idx = self.tables_emitted;
        self.table(rows);
        if rows.len() >= 2 {
            self.header_tables.insert(idx);
        }
    }

    /// 高级表格（#133 任务 2）：合并单元格（`w:gridSpan`/`w:vMerge`）+
    /// 单元格底纹（`w:shd`）+ 边框（`w:tcBorders`）+ 列宽（`w:tblGrid`
    /// + `w:tcW`）——标书表格要件。
    ///
    /// 与 [`Self::table`] 并存（`table` 纯文本极简签名零变——既有 API
    /// 契约）。语义镜像读面：`grid_span`/`v_merge` 写入 → 读回经
    /// [`TableData`] 展开为栅格 + [`MergeRange`] 矩形清单（roundtrip
    /// 对账见 tests/docx_table_advanced.rs）；底纹/边框/列宽为纯写面
    /// 装饰（docx-rs 反序列化不回读——与 tblHeader 同登记口径，字节
    /// 断言 + LibreOffice 开档代验）。
    ///
    /// V10 域校验（save 漏斗粘性拒绝——见 [`TableCellSpec`]；发射
    /// 保持人体工学签名）。计数同 [`Self::table`] 口径：物理格数
    /// （每格 = 引擎侧 1 段 + 1 run）+ 表计数。
    pub fn table_advanced(&mut self, spec: TableSpec) {
        // V10 前置：空表/空行防御（无意义产物，宁拒勿吞）。
        if spec.rows.is_empty() || spec.rows.iter().any(|r| r.is_empty()) {
            self.validation_error.get_or_insert_with(|| {
                "V10: table_advanced 空表/空行（rows 须非空且每行至少一格）".to_string()
            });
        }
        // V10d（列宽域）——发射前登记（set_grid 仍发合法形）。
        if spec.column_widths.contains(&0) {
            self.validation_error
                .get_or_insert_with(|| "V10: table_advanced 列宽须为正 twips".to_string());
        }
        let mut cells: u64 = 0;
        let rows: Vec<docx_rs::TableRow> = spec
            .rows
            .iter()
            .map(|row| {
                let out: Vec<docx_rs::TableCell> =
                    row.iter().map(|c| self.advanced_cell(c)).collect();
                cells += row.len() as u64;
                docx_rs::TableRow::new(out)
            })
            .collect();
        let mut table = docx_rs::Table::new(rows);
        if !spec.column_widths.is_empty() {
            table = table.set_grid(spec.column_widths.iter().map(|&w| w as usize).collect());
        }
        self.docx = std::mem::take(&mut self.docx).add_table(table);
        self.paragraphs += cells;
        self.runs += cells;
        self.tables_emitted += 1;
    }

    /// #133 任务 2：格规格 → 引擎格（V10 粘性校验 + XML 发射）。
    /// 违规格仍发射合法降级形（与 V7「无效 run 已进引擎文档」同语义
    /// ——save 漏斗零产物，发射形仅保持引擎构造不炸）。
    fn advanced_cell(&mut self, c: &TableCellSpec) -> docx_rs::TableCell {
        // V10a：grid_span 域。
        if c.grid_span == 0 {
            self.validation_error.get_or_insert_with(|| {
                "V10: table_advanced grid_span 须 ≥ 1（0 非法；1 = 不合并）".to_string()
            });
        }
        // V10b：底纹色域（#RRGGBB——前缀必带）。
        let mut bg_ok: Option<String> = None;
        if let Some(bg) = &c.background {
            match hex_color_6(bg) {
                Some(hex) => bg_ok = Some(hex),
                None => {
                    self.validation_error.get_or_insert_with(|| {
                        format!("V10: table_advanced 底纹色 {bg:?} 非法（须 #RRGGBB 6 位十六进制）")
                    });
                }
            }
        }
        // V10b/V10c：边框色域 + 尺寸域。
        let mut borders_ok: Option<(docx_rs::TableCellBorders, usize)> = None;
        if let Some(bs) = &c.borders {
            let mut engine = docx_rs::TableCellBorders::new();
            let mut count = 0usize;
            let sides = [
                (docx_rs::TableCellBorderPosition::Top, &bs.top),
                (docx_rs::TableCellBorderPosition::Left, &bs.left),
                (docx_rs::TableCellBorderPosition::Bottom, &bs.bottom),
                (docx_rs::TableCellBorderPosition::Right, &bs.right),
            ];
            for (pos, edge) in sides {
                let Some(e) = edge else { continue };
                if e.size_eighths == 0 || e.size_eighths > 96 {
                    self.validation_error.get_or_insert_with(|| {
                        format!(
                            "V10: table_advanced 边框尺寸 {} 越域（w:sz 1/8 磅须 1..=96）",
                            e.size_eighths
                        )
                    });
                }
                let color = if e.color == "auto" {
                    "auto".to_string()
                } else {
                    match hex_color_6(&e.color) {
                        Some(hex) => hex,
                        None => {
                            self.validation_error.get_or_insert_with(|| {
                                format!(
                                    "V10: table_advanced 边框色 {:?} 非法（须 #RRGGBB 或 auto）",
                                    e.color
                                )
                            });
                            "auto".to_string()
                        }
                    }
                };
                engine = engine.set(
                    docx_rs::TableCellBorder::new(pos)
                        .size(e.size_eighths.max(1) as usize)
                        .color(color)
                        .border_type(map_border_style(e.style)),
                );
                count += 1;
            }
            if count > 0 {
                borders_ok = Some((engine, count));
            }
        }
        // V10d：格宽域。
        if c.width == Some(0) {
            self.validation_error
                .get_or_insert_with(|| "V10: table_advanced 格宽须为正 twips".to_string());
        }
        // V10e：续格文本（读面丢弃——放行 = 内容静默丢失）。
        if c.v_merge == CellVMerge::Continue && !c.text.is_empty() {
            self.validation_error.get_or_insert_with(|| {
                "V10: table_advanced 续格（v_merge=Continue）禁止携带文本——Word 不可见且读面不回读；内容置于 Restart 原点格".to_string()
            });
        }

        let mut cell = docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(c.text.as_str())),
        );
        if c.grid_span > 1 {
            cell = cell.grid_span(c.grid_span as usize);
        }
        match c.v_merge {
            CellVMerge::Restart => {
                cell = cell.vertical_merge(docx_rs::VMergeType::Restart);
            }
            CellVMerge::Continue => {
                cell = cell.vertical_merge(docx_rs::VMergeType::Continue);
            }
            CellVMerge::None => {}
        }
        if let Some(hex) = bg_ok {
            cell = cell.shading(
                docx_rs::Shading::new()
                    .shd_type(docx_rs::ShdType::Clear)
                    .fill(hex),
            );
        }
        if let Some(w) = c.width {
            if w > 0 {
                cell = cell.width(w as usize, docx_rs::WidthType::Dxa);
            }
        }
        if let Some((engine, _)) = borders_ok {
            cell = cell.set_borders(engine);
        }
        cell
    }

    /// 保存到文件路径。
    pub fn save_to_path(&mut self, path: &str) -> Result<(), DocxError> {
        let mut buf = self.save_to_vec()?;
        std::fs::write(path, &mut buf).map_err(|e| DocxError::Io(format!("write {path}: {e}")))
    }

    /// #257：多节分节符——在**已发射内容之后**闭合一节。
    ///
    /// OOXML 语义：分节符 = 承载 `sectPr` 的段落（本节末段）。本方法
    /// 追加一个承载 `spec` 属性的空段（docx-rs `DocumentChild::Section`
    /// 落地——zip 序列化时该段发射为 `<w:p><w:pPr><w:sectPr>…`），
    /// 之后的段落属于下一节（末节属性由 [`Self::page_geometry`] +
    /// [`Self::header`]/[`Self::footer`] 六方法治理——文档级单值语义
    /// 不变）。
    ///
    /// 投标文件典型用法（封面/正文异构格式）：
    /// ```text
    /// 封面段落… → section_break(封面节：无页眉 + titlePg + 起始页码)
    /// 正文段落… → page_geometry(正文边距) + header/footer(页眉页脚页码)
    /// ```
    ///
    /// - `geometry`：本节 pgSz/pgMar（None = docx-rs 默认 A4+默认边距）；
    ///   正值域校验同 V9（粘性拒绝——本方法签名不可 Result 化，首个
    ///   违规落 `validation_error`，save 漏斗拒）。
    /// - `title_page`：`w:titlePg`（本节首页不同——first 槽位激活位）。
    /// - `page_number_start`：`w:pgNumType/@w:start`（本节起始页码——
    ///   封面不计数 = 正文节 start=1 的前置位）。
    /// - `headers`/`footers`：本节六槽位（`None` 槽位不挂接——OOXML
    ///   缺省 = 继承前节同位槽；`Vec<RunSpec>` 单段语义同文档级六方法）。
    ///
    /// r4-t2 计数：节断点段（承载 sectPr 的空段）+1 段。
    pub fn section_break(&mut self, spec: SectionBreakSpec) {
        // V9 同域校验（几何正值域）——粘性拒绝，与 page_geometry 的
        // save 漏斗校验同判据（本处提前到构造期，失败更快）。
        if let Some(g) = &spec.geometry {
            let bad = g.width == 0
                || g.height == 0
                || g.width > 40000
                || g.height > 40000
                || g.margin_top < 0
                || g.margin_right < 0
                || g.margin_bottom < 0
                || g.margin_left < 0
                || g.margin_header < 0
                || g.margin_footer < 0
                || g.gutter < 0;
            if bad {
                self.validation_error = Some(format!(
                    "section_break geometry out of domain: {g:?} (twips; pgSz 1-40000, margins >= 0)"
                ));
                return;
            }
        }
        let mut section = docx_rs::Section::new();
        if let Some(g) = &spec.geometry {
            section = section
                .page_size(docx_rs::PageSize::new().width(g.width).height(g.height))
                .page_margin(
                    docx_rs::PageMargin::new()
                        .top(g.margin_top)
                        .right(g.margin_right)
                        .bottom(g.margin_bottom)
                        .left(g.margin_left)
                        .header(g.margin_header)
                        .footer(g.margin_footer)
                        .gutter(g.gutter),
                );
        }
        if spec.title_page {
            section = section.title_pg();
        }
        if let Some(start) = spec.page_number_start {
            section = section.page_num_type(docx_rs::PageNumType::new().start(start));
        }
        // 六槽位（temp_header 通道——add_section 分配 rid + 计数，
        // zipper 收集段落级节页眉部件：zipper/mod.rs:197-201）。
        let mut spec_runs: Vec<&[RunSpec]> = Vec::new();
        if let Some(runs) = &spec.headers.default_header {
            section =
                section.header(docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs)));
            spec_runs.push(runs);
        }
        if let Some(runs) = &spec.headers.first_header {
            section = section
                .first_header(docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs)));
            spec_runs.push(runs);
        }
        if let Some(runs) = &spec.headers.even_header {
            section =
                section.even_header(docx_rs::Header::new().add_paragraph(specs_to_paragraph(runs)));
            spec_runs.push(runs);
        }
        if let Some(runs) = &spec.footers.default_footer {
            section =
                section.footer(docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs)));
            spec_runs.push(runs);
        }
        if let Some(runs) = &spec.footers.first_footer {
            section = section
                .first_footer(docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs)));
            spec_runs.push(runs);
        }
        if let Some(runs) = &spec.footers.even_footer {
            section =
                section.even_footer(docx_rs::Footer::new().add_paragraph(specs_to_paragraph(runs)));
            spec_runs.push(runs);
        }
        for runs in spec_runs {
            self.count_specs(1, runs);
        }
        self.docx = std::mem::take(&mut self.docx).add_section(section);
        // r4-t2：节断点段（承载 sectPr 的空段）计数。
        self.paragraphs += 1;
    }

    /// #257：末节起始页码（`w:pgNumType/@w:start`——文档级单值语义，
    /// 多次调用后值覆盖；中段节起始页码经 [`Self::section_break`] 的
    /// [`SectionBreakSpec::page_number_start`] 治理）。
    ///
    /// 投标文件典型位：封面节不计页码（无页脚页码域）+ 正文节
    /// `page_number_start(1)`——正文从 1 起算。
    pub fn page_number_start(&mut self, start: u32) {
        self.docx.document.section_property.page_num_type =
            Some(docx_rs::PageNumType::new().start(start));
    }

    /// 保存到内存缓冲（zip 容器字节）。r4-t2：save 面统一漏斗——
    /// [`MAX_PARAGRAPHS`]/[`MAX_RUNS`] 资源限在此 typed 拒（超限调用
    /// **无产物返回**；计数为发射调用累计的上界语义，见
    /// [`Self::paragraph`]）。发射方法的不可变签名（段落/表格/页眉
    /// 页脚等返回 ()）是既有 API 契约——逐调用 typed 拒结构性不可用，
    /// 资源限统一在本漏斗执行（消费面预检经
    /// [`check_document_scale`] 共用同一单点比较）。
    /// G2 页面几何（sectPr 面——pgSz/pgMar；单位 twips，见
    /// [`PageGeometry`]）。多次调用 = 后值覆盖（文档级单值语义）。
    /// V9 域校验（几何正值域）save 漏斗粘性拒绝。
    pub fn page_geometry(&mut self, g: PageGeometry) {
        let sp = &mut self.docx.document.section_property;
        sp.page_size = docx_rs::PageSize::new().width(g.width).height(g.height);
        sp.page_margin = docx_rs::PageMargin::new()
            .top(g.margin_top)
            .right(g.margin_right)
            .bottom(g.margin_bottom)
            .left(g.margin_left)
            .header(g.margin_header)
            .footer(g.margin_footer)
            .gutter(g.gutter);
        self.page_geometry = Some(g);
    }

    /// G2 正文默认排版（docDefaults 面——默认字体三槽 + 字号 +
    /// 行距/段距）。多次调用 = 后值覆盖。V9 域校验（字号 1-32767
    /// 半磅、行距正值）save 漏斗粘性拒绝。
    pub fn default_typography(&mut self, t: DefaultTypography) {
        let fonts = docx_rs::RunFonts::new()
            .ascii(t.ascii_font.as_str())
            .east_asia(t.east_asia_font.as_str())
            .cs(t.complex_script_font.as_str());
        self.docx = std::mem::take(&mut self.docx)
            .default_fonts(fonts)
            .default_size(t.size_half_points as usize);
        if let Some(ls) = &t.line_spacing {
            let mut spacing = docx_rs::LineSpacing::new();
            if let Some(line) = ls.line {
                spacing = spacing.line(line);
            }
            if let Some(rule) = ls.line_rule {
                spacing = spacing.line_rule(match rule {
                    LineRuleSpec::Auto => docx_rs::LineSpacingType::Auto,
                    LineRuleSpec::AtLeast => docx_rs::LineSpacingType::AtLeast,
                    LineRuleSpec::Exact => docx_rs::LineSpacingType::Exact,
                });
            }
            if let Some(b) = ls.before {
                spacing = spacing.before(b);
            }
            if let Some(a) = ls.after {
                spacing = spacing.after(a);
            }
            self.docx = std::mem::take(&mut self.docx).default_line_spacing(spacing);
        }
        self.typography = Some(t);
    }

    pub fn save_to_vec(&mut self) -> Result<Vec<u8>, DocxError> {
        // office-g V7 粘性校验首查（fail-closed：半合法文档零产出——
        // 与超限零产物同语义）。**不 take 不重置**：错误路径与 Limit
        // 路径同语义（写器保持原状）；无效 run 已发射进引擎文档，
        // 粘性错误持续存在——后续 save 恒拒，唯一解 = 去除违规规格
        // 重建写器。
        if let Some(msg) = &self.validation_error {
            return Err(DocxError::InvalidInput(msg.clone()));
        }
        // G2 V9（文档级格式域）——几何正值域 + 字号半磅域 + 行距
        // 正值域（构造期不可 Result 化，与 V7/V8 同漏斗纪律）。
        if let Some(g) = &self.page_geometry {
            let bad_geom = g.width == 0
                || g.height == 0
                || g.width > 40000
                || g.height > 40000
                || g.margin_top < 0
                || g.margin_right < 0
                || g.margin_bottom < 0
                || g.margin_left < 0
                || g.margin_header < 0
                || g.margin_footer < 0
                || g.gutter < 0;
            if bad_geom {
                return Err(DocxError::InvalidInput(format!(
                    "V9: 页面几何域违规（w={} h={} m={}/{}/{}/{}——twips 须 1-40000 且边距非负）",
                    g.width, g.height, g.margin_top, g.margin_right, g.margin_bottom, g.margin_left
                )));
            }
        }
        if let Some(t) = &self.typography {
            if t.size_half_points == 0 || t.size_half_points > 32767 {
                return Err(DocxError::InvalidInput(format!(
                    "V9: 默认字号域违规（{} 半磅——须 1-32767）",
                    t.size_half_points
                )));
            }
            if let Some(ls) = &t.line_spacing {
                let bad_ls = ls.line.map(|l| l <= 0).unwrap_or(false)
                    || ls.before.map(|b| b > 31680).unwrap_or(false)
                    || ls.after.map(|a| a > 31680).unwrap_or(false);
                if bad_ls {
                    return Err(DocxError::InvalidInput(
                        "V9: 行距/段距域违规（line 须正值；before/after ≤ 31680 twips）".into(),
                    ));
                }
            }
        }
        check_document_scale(self.paragraphs, self.runs)?;
        let mut cursor = std::io::Cursor::new(Vec::new());
        let built = std::mem::take(&mut self.docx).build();
        built
            .pack(&mut cursor)
            .map_err(|e| DocxError::Io(format!("pack: {e}")))?;
        // N-4：首行 tblHeader 注入（仅 table_header_repeat 记账的表——
        // 纯 table() 消费方字节级零变化）。
        let bytes = cursor.into_inner();
        let out = if self.header_tables.is_empty() {
            bytes
        } else {
            let tables = std::mem::take(&mut self.header_tables);
            mark_table_header_rows(&bytes, &tables)?
        };
        // build() 消费 self.docx —— 重建基态（写器可续写：幂等语义，
        // 已写内容已在 built 内落盘，续写 = 新档；numbering 惰性位
        // 随之重置——新档首个 list_item 重新注册）。
        *self = Self::new();
        Ok(out)
    }

    /// 列表段（t20 列表面）：`kind` 选 ordered/bullet 抽象，`ilvl` 选
    /// 级（**0-8**，typed 验证——抽象 9 级）；run 序同
    /// [`Self::paragraph`]（加粗/斜体/等宽/删除线/下画线/链接全位）。
    ///
    /// **接线陷阱（WRITE_FACE_PROBE §2.1）**：必须经引擎
    /// `Paragraph::numbering`（同置 `has_numbering` → 打包时
    /// document.xml.rels 发射 numbering 关系）；`ParagraphProperty::
    /// numbering_property` 直挂路径不置位 → rels 缺关系 → **悬空
    /// numPr**（合法包但 Word 无列表渲染）。
    pub fn list_item(
        &mut self,
        runs: &[RunSpec],
        kind: ListKind,
        ilvl: usize,
    ) -> Result<(), DocxError> {
        if ilvl > MAX_LIST_LEVEL {
            return Err(DocxError::Parse(format!(
                "list ilvl must be 0-{MAX_LIST_LEVEL}, got {ilvl}"
            )));
        }
        self.ensure_numberings();
        let num_id = match kind {
            ListKind::Ordered => ORDERED_NUM_ID,
            ListKind::Bullet => BULLET_NUM_ID,
        };
        let p = specs_to_paragraph(runs).numbering(
            docx_rs::NumberingId::new(num_id),
            docx_rs::IndentLevel::new(ilvl),
        );
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.count_specs(1, runs);
        Ok(())
    }

    /// 多行列表项段（t22 续行缩进面）：**一个列表项 = 一个 OOXML 段落**
    /// ——`line_runs` 每元素 = 一行的 run 序（全位同 [`Self::paragraph`]），
    /// 行间 `w:br w:type="textWrapping"` 软换行（`Run::add_break`
    /// run.rs:247 + `BreakType::TextWrapping`——一手源码核验）；
    /// **续行与首行文字列对齐** = 段落级 `w:ind`（left =
    /// [`QUOTE_INDENT_STEP`]×(ilvl+1)、hanging [`LIST_HANGING_INDENT`]）
    /// 与编号抽象层级同值（numbering.xml 层级本有同几何——marker 列 =
    /// left−360、首行文字列 = left；直接 pPr 优先但同值 = 同几何，
    /// Word 自产文档惯例形态；显式直挂使 document.xml 自含几何，
    /// 对 numbering 层级 pPr 应用不全的渲染器更鲁棒）。
    /// br 续行/自动换行行起于段落 left → 与首行文字列（tab 后到位）
    /// 精确同列。单行项应走 [`Self::list_item`]（无显式 ind——字节级
    /// 稳定面）。ilvl typed 验证与 numbering 接线陷阱同 [`Self::list_item`]。
    pub fn list_item_lines(
        &mut self,
        line_runs: &[Vec<RunSpec>],
        kind: ListKind,
        ilvl: usize,
    ) -> Result<(), DocxError> {
        if ilvl > MAX_LIST_LEVEL {
            return Err(DocxError::Parse(format!(
                "list ilvl must be 0-{MAX_LIST_LEVEL}, got {ilvl}"
            )));
        }
        self.ensure_numberings();
        let num_id = match kind {
            ListKind::Ordered => ORDERED_NUM_ID,
            ListKind::Bullet => BULLET_NUM_ID,
        };
        let p = spec_lines_to_paragraph(line_runs)
            .numbering(
                docx_rs::NumberingId::new(num_id),
                docx_rs::IndentLevel::new(ilvl),
            )
            .indent(
                Some(QUOTE_INDENT_STEP * (ilvl as i32 + 1)),
                Some(docx_rs::SpecialIndentType::Hanging(LIST_HANGING_INDENT)),
                None,
                None,
            );
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.paragraphs += 1;
        for line in line_runs {
            self.runs += line.len() as u64;
            for r in line {
                if r.footnote.is_some() {
                    self.paragraphs += 1;
                    self.runs += 1;
                }
            }
        }
        Ok(())
    }

    /// 代码行段（t20 等宽面·块级）：**双通道等宽**——段落级
    /// `pPr>rPr>rFonts`（段落标记 + 段内缺省面）+ run 级 rFonts
    /// [`CODE_FONT`]（文本保真主通道）。调用方负责行切分（本方法
    /// 单行语义——每行一段，模型行级哲学）。
    pub fn code_line(&mut self, text: &str) {
        let mut p = specs_to_paragraph(&[RunSpec {
            text: text.to_string(),
            mono: true,
            ..RunSpec::text("")
        }]);
        p = p.fonts(mono_fonts(CODE_FONT));
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.paragraphs += 1;
        self.runs += 1;
    }

    /// 水平线段（t20 hr 面）：空段 + 下边框（OOXML 无原生 hr 的惯用
    /// 表达；`size=6` = 0.75pt、`space=1`——WRITE_FACE_PROBE §2.3
    /// 探针参数）。
    ///
    /// **陷阱**：`ParagraphBorders::new()`/`Default` = 四边全 Single
    /// （paragraph_borders.rs:117-127）——会画整框而非水平线；本面
    /// 恒 `with_empty().set(单边)`。
    pub fn horizontal_rule(&mut self) {
        let mut p = docx_rs::Paragraph::new();
        p.property = p.property.clone().set_borders(
            docx_rs::ParagraphBorders::with_empty().set(
                docx_rs::ParagraphBorder::new(docx_rs::ParagraphBorderPosition::Bottom)
                    .size(6)
                    .space(1)
                    .color("auto"),
            ),
        );
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.paragraphs += 1;
    }

    /// 引用段（t20 quote 面）：左缩进 `QUOTE_INDENT_STEP × depth`
    ///（**depth 钳 [`MAX_QUOTE_DEPTH`]——防溢出版心，超限静默钳位**）
    /// 与左边框（`size=12` = 1.5pt、`space=4`）；run 序全位同
    /// [`Self::paragraph`]。无内建 `Quote` 段落样式可引（引擎
    /// preset_styles 仅 toc——`Paragraph::style("Quote")` 只产悬空
    /// pStyle 引用），故为直接格式（与 t18 REPORT §8 预判一致）。
    pub fn quote_paragraph(&mut self, runs: &[RunSpec], depth: usize) {
        let left = QUOTE_INDENT_STEP * depth.min(MAX_QUOTE_DEPTH) as i32;
        let mut p = specs_to_paragraph(runs).indent(Some(left), None, None, None);
        p.property = p.property.clone().set_borders(
            docx_rs::ParagraphBorders::with_empty().set(
                docx_rs::ParagraphBorder::new(docx_rs::ParagraphBorderPosition::Left)
                    .size(12)
                    .space(4)
                    .color("auto"),
            ),
        );
        self.docx = std::mem::take(&mut self.docx).add_paragraph(p);
        self.count_specs(1, runs);
    }

    /// numbering 定义面惰性注册（幂等）：ordered（decimal outline 式
    /// 9 级——lvlText `%1.` / `%1.%2.` / …；left = 720×(i+1)、悬挂 360）
    /// 与 bullet（`•` 9 级同构）两抽象 + num 桥（numId = abstractNumId
    /// = 100/101，避开内建 1）。
    fn ensure_numberings(&mut self) {
        if self.numbering_registered {
            return;
        }
        let ordered = ordered_abstract();
        let bullet = bullet_abstract();
        let docx = std::mem::take(&mut self.docx);
        self.docx = docx
            .add_abstract_numbering(ordered)
            .add_abstract_numbering(bullet)
            .add_numbering(docx_rs::Numbering::new(ORDERED_NUM_ID, ORDERED_NUM_ID))
            .add_numbering(docx_rs::Numbering::new(BULLET_NUM_ID, BULLET_NUM_ID));
        self.numbering_registered = true;
    }
}

/// run 规格序列 → 引擎段落（链接 run → 引擎超链接元素[rels 由引擎
/// 打包收集]；脚注 marker run → 引擎脚注引用[footnotes.xml 由引擎
/// 打包收集]；域 run → fldChar begin/instrText/end 三连；普通 run
/// 直挂）。t20：mono/strike/underline 三位 → rFonts 三槽 / w:strike /
/// w:u single。
/// office-g §11 V1-V6 书签名校验（fail-closed——宁缺毋错对齐：
/// 静默双书签/截断/踩踏 TOC 机制均显式拒绝）。
fn validate_bookmark_name(
    name: &str,
    existing: &std::collections::HashSet<String>,
) -> Result<(), DocxError> {
    if name.is_empty() {
        return Err(DocxError::InvalidInput("V1: 书签名为空".into()));
    }
    if name.chars().count() > 40 {
        return Err(DocxError::InvalidInput(format!(
            "V2: 书签名超 40 字符上限（实得 {}）",
            name.chars().count()
        )));
    }
    // 校验序 = specific-before-general：V5（_Toc 保留名）先于 V3（首
    // 字母）——'_' 非 ASCII 字母，若 V3 先行则 V5 恒不可达（office-g
    // §11 矩阵为校验面清单，求值序由实现定；保留名是更具体的拒绝面）。
    if name.starts_with("_Toc") {
        return Err(DocxError::InvalidInput(
            "V5: _Toc 前缀保留名拒绝（TOC 内部机制不踩踏）".into(),
        ));
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() {
        return Err(DocxError::InvalidInput(format!(
            "V3: 书签名首字符须为字母（实得 {:?}）",
            first
        )));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(DocxError::InvalidInput(
            "V4: 书签名仅允许 [A-Za-z0-9_]（保守 ASCII 域——跨引擎最稳交集；放宽为后续裁决点）"
                .into(),
        ));
    }
    if existing.contains(name) {
        return Err(DocxError::InvalidInput(format!(
            "V6: 同文档书签名重名（{:?}）——Word 容忍重名但跳转不可预期，包装层显式拒绝",
            name
        )));
    }
    Ok(())
}

/// S1 样式面：pub(crate)（style.rs `styled_paragraph` 复用——同构纯函数）。
/// S1 编号面：pub(crate)（numbering.rs `numbered_list_item` 复用）。
pub(crate) fn specs_to_paragraph(runs: &[RunSpec]) -> docx_rs::Paragraph {
    let mut p = docx_rs::Paragraph::new();
    for spec in runs {
        p = add_spec_to_paragraph(p, spec);
    }
    p
}

/// 单个 [`RunSpec`] 的 run 构造挂接（[`specs_to_paragraph`] 的逐 spec
/// 步进——t22 抽出共享：脚注 marker / 简单域三连 / 普通 run + 链接
/// 包裹三分支原样搬移，行为零变化）。
fn add_spec_to_paragraph(p: docx_rs::Paragraph, spec: &RunSpec) -> docx_rs::Paragraph {
    let mut p = p;
    // 脚注 marker（样式面忽略——引擎 add_footnote_reference 重置
    // run_property，见 RunSpec::footnote 文档）。
    if let Some(note) = &spec.footnote {
        let mut footnote = docx_rs::Footnote::new();
        footnote.add_content(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(note.as_str())),
        );
        p = p.add_run(docx_rs::Run::new().add_footnote_reference(footnote));
        return p;
    }
    // 简单域（begin/instrText/end 三连——引擎 add_tc 同构先例
    // run.rs:202-208）。
    if let Some(field) = &spec.field {
        // G1 TOC：五连（begin[非 dirty——#309③ 域缓存写入] + instrText
        // + separate + 占位 + end）——域结果占位 = 打开即显的缓存
        //（无弹窗）；F9 重算生成真目录。#309③ 前为 dirty=true：靠
        // Word/LO 打开弹「是否更新域」驱动更新，缓存反被弹窗遮蔽
        // ——语义倒置，改缓存直显。
        if let FieldSpec::TableOfContents { heading_range } = field {
            let mut toc = docx_rs::InstrToC::new();
            if let Some((lo, hi)) = *heading_range {
                toc = toc.heading_styles_range(lo as usize, hi as usize);
            }
            return p
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false))
                .add_run(docx_rs::Run::new().add_instr_text(docx_rs::InstrText::TOC(toc)))
                .add_run(
                    docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Separate, false),
                )
                .add_run(docx_rs::Run::new().add_text("（目录域——打开后全选再按 F9 更新生成）"))
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false));
        }
        // #196 任务 2 PAGEREF：五连（begin 非dirty + instr
        // `PAGEREF {bm} \h` + separate + 页码占位 + end）——占位 "1"
        // 是保存时快照，F9 重算；与 TOC 五连同构（域结果占位两 run）
        // 但 begin 不置 dirty（嵌入 TOC 静态项时随整块刷新，单独发射
        // 时无「提示更新」必要）。
        if let FieldSpec::PageRef { bookmark } = field {
            return p
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false))
                .add_run(
                    docx_rs::Run::new().add_instr_text(docx_rs::InstrText::PAGEREF(
                        docx_rs::InstrPAGEREF::new(bookmark).hyperlink(),
                    )),
                )
                .add_run(
                    docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Separate, false),
                )
                .add_run(docx_rs::Run::new().add_text("1"))
                .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false));
        }
        let instr = match field {
            FieldSpec::Page => docx_rs::InstrText::PAGE(docx_rs::InstrPAGE::new()),
            FieldSpec::NumPages => docx_rs::InstrText::NUMPAGES(docx_rs::InstrNUMPAGES::new()),
            FieldSpec::TableOfContents { .. } => unreachable!("TOC 已在上方分支返回"),
            FieldSpec::PageRef { .. } => unreachable!("PAGEREF 已在上方分支返回"),
        };
        return p
            .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false))
            .add_run(docx_rs::Run::new().add_instr_text(instr))
            .add_run(docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false));
    }
    let mut run = docx_rs::Run::new().add_text(spec.text.as_str());
    let mut rp = docx_rs::RunProperty::new();
    // #196 任务 3：字符样式引用（w:rStyle）——引擎 RunProperty::style
    //（设置在 rp 内；先于此处构造后续格式位后赋值，避免覆盖）。
    if let Some(cs) = &spec.char_style {
        rp = rp.style(cs);
    }
    if spec.bold {
        rp = rp.bold();
    }
    if spec.italic {
        rp = rp.italic();
    }
    if spec.strike {
        rp = rp.strike();
    }
    if spec.underline {
        rp = rp.underline("single");
    }
    if spec.mono {
        rp = rp.fonts(mono_fonts(MONO_FONT));
    }
    run.run_property = rp;
    // office-g Phase 1 发射分支：anchor 赢、link 忽略（V8 优先序——
    // 读面 HyperlinkData 解析顺序镜像[r5-t2 探针②]）；anchor 形 =
    // w:anchor 属性（零 rels 面）；悬空锚点发射不拒（T3 合法形）。
    if let Some(anchor) = &spec.anchor {
        p.add_hyperlink(
            docx_rs::Hyperlink::new(anchor.as_str(), docx_rs::HyperlinkType::Anchor).add_run(run),
        )
    } else if let Some(url) = &spec.link {
        p.add_hyperlink(
            docx_rs::Hyperlink::new(url.as_str(), docx_rs::HyperlinkType::External).add_run(run),
        )
    } else {
        p.add_run(run)
    }
}

/// 多行 run 序列（每元素 = 一行的 run 组）→ 单引擎段落：行间
/// `w:br w:type="textWrapping"` 软换行 run（t22 续行缩进面——一个
/// 列表项 = 一个段落语义）。空行 = 空 run 组（仅 br 分隔，无文本 run）。
fn spec_lines_to_paragraph(line_runs: &[Vec<RunSpec>]) -> docx_rs::Paragraph {
    let mut p = docx_rs::Paragraph::new();
    for (i, line) in line_runs.iter().enumerate() {
        if i > 0 {
            p = p.add_run(docx_rs::Run::new().add_break(docx_rs::BreakType::TextWrapping));
        }
        for spec in line {
            p = add_spec_to_paragraph(p, spec);
        }
    }
    p
}

/// 等宽 rFonts 三槽（ascii/hAnsi/cs——`w:ascii` 覆盖首 128 码位、
/// `w:hAnsi` 高 ANSI、`w:cs` 复杂文种；run_fonts.rs:39-87）。
fn mono_fonts(font: &str) -> docx_rs::RunFonts {
    docx_rs::RunFonts::new().ascii(font).hi_ansi(font).cs(font)
}

/// 有序（decimal outline 式）9 级抽象——lvlText 逐级拼号
///（`%1.` / `%1.%2.` / … `%1.%2.%3.%4.%5.%6.%7.%8.%9.`）；
/// left = 720×(i+1) + 悬挂 360（WRITE_FACE_PROBE §2.1 探针参数
/// 同构）。
fn ordered_abstract() -> docx_rs::AbstractNumbering {
    let mut a = docx_rs::AbstractNumbering::new(ORDERED_NUM_ID);
    for i in 0..=MAX_LIST_LEVEL {
        let lvl_text: String = (1..=i + 1)
            .map(|k| format!("%{k}"))
            .collect::<Vec<_>>()
            .join(".");
        a = a.add_level(
            docx_rs::Level::new(
                i,
                docx_rs::Start::new(1),
                docx_rs::NumberFormat::new("decimal"),
                docx_rs::LevelText::new(format!("{lvl_text}.")),
                docx_rs::LevelJc::new("left"),
            )
            .indent(
                Some(QUOTE_INDENT_STEP * (i as i32 + 1)),
                Some(docx_rs::SpecialIndentType::Hanging(LIST_HANGING_INDENT)),
                None,
                None,
            ),
        );
    }
    a
}

/// 无序（`•`）9 级抽象——各级同字形（Word 原生逐级变字形惯例不
/// 采用——均匀 • 即消解降级的最小诚实面）；缩进同 ordered。
fn bullet_abstract() -> docx_rs::AbstractNumbering {
    let mut a = docx_rs::AbstractNumbering::new(BULLET_NUM_ID);
    for i in 0..=MAX_LIST_LEVEL {
        a = a.add_level(
            docx_rs::Level::new(
                i,
                docx_rs::Start::new(1),
                docx_rs::NumberFormat::new("bullet"),
                docx_rs::LevelText::new("\u{2022}"),
                docx_rs::LevelJc::new("left"),
            )
            .indent(
                Some(QUOTE_INDENT_STEP * (i as i32 + 1)),
                Some(docx_rs::SpecialIndentType::Hanging(LIST_HANGING_INDENT)),
                None,
                None,
            ),
        );
    }
    a
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

// ---------------------------------------------------------------------------
// G3 暗标元数据匿名化（issue #6 / 标书 R-11 BLOCKER——2026-10-26）
// ---------------------------------------------------------------------------

/// 匿名 core.xml（与引擎新建面 `DocxWriter::save_to_vec` 产物**逐字节
/// 同构**——探针 g3_docx_docprops_inventory 实测锚点。「新建」与
/// 「匿名导出」元数据面一致 = 单一真源语义：unknown 作者 + epoch
/// 零时 + revision=1）。
const ANON_CORE_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<cp:coreProperties"#,
    r#" xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties""#,
    r#" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
    r#" xmlns:dcterms="http://purl.org/dc/terms/""#,
    r#" xmlns:dcmitype="http://purl.org/dc/dcmitype/""#,
    r#" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#,
    r#"<dcterms:created xsi:type="dcterms:W3CDTF">1970-01-01T00:00:00Z</dcterms:created>"#,
    r#"<dc:creator>unknown</dc:creator>"#,
    r#"<cp:lastModifiedBy>unknown</cp:lastModifiedBy>"#,
    r#"<dcterms:modified xsi:type="dcterms:W3CDTF">1970-01-01T00:00:00Z</dcterms:modified>"#,
    r#"<cp:revision>1</cp:revision>"#,
    r#"</cp:coreProperties>"#,
);

/// 匿名 app.xml / custom.xml（空骨架——与引擎新建面同构；custom 的
/// Company/Sender 等 VT 自定义属性整面清空）。
const ANON_APP_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties""#,
    r#" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"/>"#,
);
const ANON_CUSTOM_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties""#,
    r#" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"/>"#,
);

/// 匿名 people.xml（批注人员名单部件——存在即清空）。
const ANON_PEOPLE_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:people xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#,
);

/// comments.xml 批注作者面匿名化（w:author/w:initials → 空串、
/// w:date → epoch——quick-xml 事件流属性级改写，文本内容零触碰）。
fn anonymize_comments_xml(xml: &str) -> Result<String, DocxError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = quick_xml::Writer::new(Vec::new());
    loop {
        match reader.read_event() {
            Ok(Event::Start(mut e)) => {
                anonymize_comment_attrs(&mut e);
                writer
                    .write_event(Event::Start(e))
                    .map_err(|e| DocxError::Io(format!("comments rewrite: {e}")))?;
            }
            Ok(Event::Empty(mut e)) => {
                anonymize_comment_attrs(&mut e);
                writer
                    .write_event(Event::Empty(e))
                    .map_err(|e| DocxError::Io(format!("comments rewrite: {e}")))?;
            }
            Ok(Event::Eof) => break,
            Ok(other) => {
                writer
                    .write_event(other)
                    .map_err(|e| DocxError::Io(format!("comments rewrite: {e}")))?;
            }
            Err(e) => return Err(DocxError::Io(format!("comments parse: {e}"))),
        }
    }
    let out = String::from_utf8(writer.into_inner())
        .map_err(|e| DocxError::Io(format!("comments utf8: {e}")))?;
    Ok(out)
}

/// 批注属性清零（author/initials → 空串、date → epoch）。
/// quick-xml 0.41 `Attributes` 无原位改写——收集后重建 BytesStart
/// （保留原始 key 字节含前缀，未匹配属性原值回写）。
fn anonymize_comment_attrs(e: &mut quick_xml::events::BytesStart<'_>) {
    let owned: Vec<(Vec<u8>, Vec<u8>)> = e
        .attributes()
        .flatten()
        .map(|a| (a.key.as_ref().to_vec(), a.value.as_ref().to_vec()))
        .collect();
    if owned.is_empty() {
        return;
    }
    let name = e.name().as_ref().to_vec();
    let mut new_start = quick_xml::events::BytesStart::new(std::borrow::Cow::Owned(
        String::from_utf8_lossy(&name).into_owned(),
    ));
    for (key, value) in owned {
        let local = key.rsplit(|&b| b == b':').next().unwrap_or(&key);
        let replacement: &[u8] = match local {
            b"author" | b"initials" => b"",
            b"date" => b"1970-01-01T00:00:00Z",
            _ => &value,
        };
        new_start.push_attribute(quick_xml::events::attributes::Attribute {
            key: quick_xml::name::QName(&key),
            value: replacement.into(),
        });
    }
    *e = new_start;
}

/// docx 暗标元数据匿名化（G3 / R-11：既有文档读改写导出面的
/// 元数据级清零——原子发布字节边界内完成，无「先发布再补丁」窗口）。
///
/// **变换面（五部件）**：
/// - `docProps/core.xml` → 匿名常量（unknown 作者 + epoch 零时 +
///   revision=1，与引擎新建面逐字节同构）；
/// - `docProps/app.xml` / `docProps/custom.xml` → 空骨架（Company/
///   Manager/Sender 等扩展与自定义属性整面清空）；
/// - `word/comments.xml` → w:author/w:initials 置空 + w:date → epoch
///   （批注**文本内容零触碰**——只清归属面）；
/// - `word/people.xml` → 空骨架（存在即清空）。
///
/// **不变量（正文零触碰）**：`word/document.xml` 及其余全部部件
/// **原压缩流原样拷贝**（`raw_copy`——字节级不变）。容忍缺部件
/// （无 docProps 的极简 zip 跳过对应面）；`word/document.xml`
/// 不在场 = 拒（非 docx 伪产物 fail-closed）。
///
/// **边界**：`document.xml` 内的修订记录（w:ins/w:del 的 w:author
/// 属性）**不在变换面**——清作者需动正文部件字节，违反正文零触碰
/// 不变量；带修订文档应先接受/拒绝修订（内容面操作，非元数据面）。
pub fn anonymize_docx_props(bytes: &[u8]) -> Result<Vec<u8>, DocxError> {
    use std::io::Write as _;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| DocxError::Io(format!("open zip: {e}")))?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut has_document = false;
    for i in 0..archive.len() {
        // 两段借用：by_index_raw 仅用于名字发现与 raw_copy（未解压流）；
        // comments.xml 需解压读 → 独立 by_name（解压模式）。
        let name = {
            let entry = archive
                .by_index_raw(i)
                .map_err(|e| DocxError::Io(format!("zip index: {e}")))?;
            entry.name().to_string()
        };
        match name.as_str() {
            "docProps/core.xml" => {
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| DocxError::Io(format!("zip start: {e}")))?;
                writer
                    .write_all(ANON_CORE_XML.as_bytes())
                    .map_err(|e| DocxError::Io(format!("zip write: {e}")))?;
            }
            "docProps/app.xml" | "docProps/custom.xml" => {
                let skeleton = if name == "docProps/app.xml" {
                    ANON_APP_XML
                } else {
                    ANON_CUSTOM_XML
                };
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| DocxError::Io(format!("zip start: {e}")))?;
                writer
                    .write_all(skeleton.as_bytes())
                    .map_err(|e| DocxError::Io(format!("zip write: {e}")))?;
            }
            "word/people.xml" => {
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| DocxError::Io(format!("zip start: {e}")))?;
                writer
                    .write_all(ANON_PEOPLE_XML.as_bytes())
                    .map_err(|e| DocxError::Io(format!("zip write: {e}")))?;
            }
            "word/comments.xml" => {
                let mut xml = String::new();
                archive
                    .by_name(&name)
                    .map_err(|e| DocxError::Io(format!("comments open: {e}")))?
                    .read_to_string(&mut xml)
                    .map_err(|e| DocxError::Io(format!("comments read: {e}")))?;
                let anon = anonymize_comments_xml(&xml)?;
                writer
                    .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                    .map_err(|e| DocxError::Io(format!("zip start: {e}")))?;
                writer
                    .write_all(anon.as_bytes())
                    .map_err(|e| DocxError::Io(format!("zip write: {e}")))?;
            }
            _ => {
                if name == "word/document.xml" {
                    has_document = true;
                }
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
            "anonymize: word/document.xml 不在场（非 docx 伪产物——fail-closed 拒绝）".into(),
        ));
    }
    let out = writer
        .finish()
        .map_err(|e| DocxError::Io(format!("zip finish: {e}")))?;
    Ok(out.into_inner())
}

/// N-4（#29）：docx 字节 → 指定文档序表集的首行注入 `w:tblHeader`
/// （跨页表头重复）。
///
/// 为什么 zip 后处理而非 docx-rs 面发射：docx-rs 0.4.22
/// `TableRowProperty` 无 tblHeader 字段（仅 gridAfter/Before/行高/
/// cantSplit 族）；版本钉死不可升（P2-2 gridSpan/vMerge 读取通道 =
/// serde derive 锁 0.4.22）。注入锚点 = 引擎写面发射形态实证钉定
/// （`<w:tbl>`/`<w:tr>` 恒无属性、首子元素恒 `<w:trPr />` 自闭合），
/// 三形态防御（自闭合两拼写 / 开形含子 / 无 trPr 兜底插入）；
/// `<w:trPr>` 内 `<w:tblHeader />` 恒居尾（ECMA CT_TrPr 序——
/// cantSplit/trHeight 之后合法位）。非目标部件 raw_copy 原样保留
/// （匿名化后处理同构）。`word/document.xml` 不在场 = fail-closed 拒。
pub fn mark_table_header_rows(
    bytes: &[u8],
    tables: &std::collections::HashSet<usize>,
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
        if name == "word/document.xml" {
            has_document = true;
            let mut xml = String::new();
            archive
                .by_name(&name)
                .map_err(|e| DocxError::Io(format!("document open: {e}")))?
                .read_to_string(&mut xml)
                .map_err(|e| DocxError::Io(format!("document read: {e}")))?;
            let marked = inject_tbl_header(&xml, tables)?;
            writer
                .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                .map_err(|e| DocxError::Io(format!("zip start: {e}")))?;
            writer
                .write_all(marked.as_bytes())
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
            "tblHeader 注入：word/document.xml 不在场（非 docx 伪产物——fail-closed 拒绝）".into(),
        ));
    }
    let out = writer
        .finish()
        .map_err(|e| DocxError::Io(format!("zip finish: {e}")))?;
    Ok(out.into_inner())
}

/// document.xml 锚定注入：第 `tables` 集合内序号的 `<w:tbl>` 的首个
/// `<w:tr>` 打 `tblHeader`。表序 = `<w:tbl` 出现序（含嵌套表——内层
/// 表首行在自身 `<w:tbl>` 之后，外层首行恒先于内层出现，文档序计数
/// 对两种结构均正确）。
fn inject_tbl_header(
    xml: &str,
    tables: &std::collections::HashSet<usize>,
) -> Result<String, DocxError> {
    let mut out = String::with_capacity(xml.len() + tables.len() * 32);
    let mut rest = xml;
    let mut table_idx = 0usize;
    while let Some(pos) = rest.find("<w:tbl") {
        let after_tag = pos + "<w:tbl".len();
        // `<w:tbl>` 或 `<w:tbl 属性…>`（引擎发射恒无属性——属性形
        // 防御面）；排除 `<w:tblPr`/`<w:tblGrid` 前缀误配（后随字符
        // 必须为 `>` 或空白）。
        let tag_close = match rest[after_tag..].find('>') {
            Some(off) => after_tag + off,
            None => {
                return Err(DocxError::Io(
                    "tblHeader 注入：<w:tbl 开标签无闭合（畸形 XML——fail-closed 拒绝）".into(),
                ));
            }
        };
        let next = rest.as_bytes().get(after_tag).copied();
        if next != Some(b'>') && next != Some(b' ') {
            // <w:tblPr / <w:tblGrid 等前缀——原样透传继续扫描。
            out.push_str(&rest[..tag_close + 1]);
            rest = &rest[tag_close + 1..];
            continue;
        }
        // 本表打标 → 找首个 <w:tr 行开标签（`<w:tr>`/`<w:tr …>`——
        // `<w:trPr`/`<w:trHeight` 前缀由后随字符区分）。
        let mark = tables.contains(&table_idx);
        if !mark {
            out.push_str(&rest[..tag_close + 1]);
            rest = &rest[tag_close + 1..];
            table_idx += 1;
            continue;
        }
        let row_open = match rest[tag_close + 1..].find("<w:tr") {
            Some(off) => tag_close + 1 + off,
            None => {
                return Err(DocxError::Io(
                    "tblHeader 注入：表内无 <w:tr 行（畸形——fail-closed 拒绝）".into(),
                ));
            }
        };
        let after_row = row_open + "<w:tr".len();
        let next = rest.as_bytes().get(after_row).copied();
        if next != Some(b'>') && next != Some(b' ') {
            return Err(DocxError::Io(
                "tblHeader 注入：<w:tr 后随既非 > 也非空白（非行开标签——畸形拒）".into(),
            ));
        }
        let row_close = match rest[after_row..].find('>') {
            Some(off) => after_row + off,
            None => {
                return Err(DocxError::Io(
                    "tblHeader 注入：<w:tr 开标签无闭合（畸形——fail-closed 拒绝）".into(),
                ));
            }
        };
        out.push_str(&rest[..row_close + 1]);
        rest = &rest[row_close + 1..];
        // 三形态：自闭合 `<w:trPr />` / `<w:trPr/>`；开形
        // `<w:trPr>…</w:trPr>`；无 trPr（兜底直插行首）。
        if let Some(stripped) = rest.strip_prefix("<w:trPr />") {
            out.push_str("<w:trPr><w:tblHeader /></w:trPr>");
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix("<w:trPr/>") {
            out.push_str("<w:trPr><w:tblHeader /></w:trPr>");
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix("<w:trPr></w:trPr>") {
            // 空开闭形（非引擎发射——防御面；替换而非追加，杜绝双
            // trPr 产出[CT_TrPr maxOccurs=1]）。
            out.push_str("<w:trPr><w:tblHeader /></w:trPr>");
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix("<w:trPr>") {
            // 开形：找闭合 </w:trPr>，tblHeader 插其前（CT_TrPr 序
            // 尾位——cantSplit/trHeight 之后合法）。
            match stripped.find("</w:trPr>") {
                Some(off) => {
                    out.push_str("<w:trPr>");
                    out.push_str(&stripped[..off]);
                    out.push_str("<w:tblHeader /></w:trPr>");
                    rest = &stripped[off + "</w:trPr>".len()..];
                }
                None => {
                    return Err(DocxError::Io(
                        "tblHeader 注入：<w:trPr> 无闭合（畸形——fail-closed 拒绝）".into(),
                    ));
                }
            }
        } else {
            // 无 trPr：行开标签后直插（首个子元素位——schema 合法）。
            out.push_str("<w:trPr><w:tblHeader /></w:trPr>");
        }
        table_idx += 1;
    }
    out.push_str(rest);
    Ok(out)
}

/// 读 docx 指定部件原始字节（匿名化配套验证面——workbench 导出验证
/// 与测试共用；部件不在场返回 None）。
pub fn docx_part_bytes(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut e = ar.by_name(name).ok()?;
    let mut v = vec![];
    std::io::Read::read_to_end(&mut e, &mut v).ok()?;
    Some(v)
}

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

    /// #13（A1）：样式标题语义解析矩阵（StyleIndex + 四链优先序）。
    /// 夹具面（真 Word 中文版 styleId="1"-"9"）见
    /// tests/tr13_heading_styles.rs；本测试钉定合成全谱。
    #[test]
    fn style_index_resolution_matrix() {
        use docx_rs::{Name, ParagraphProperty, Style, Styles};
        let styled = |id: &str, name: &str, outline: Option<usize>| Style {
            style_id: id.to_string(),
            name: Name::new(name),
            paragraph_property: match outline {
                Some(o) => ParagraphProperty::new().outline_lvl(o),
                None => ParagraphProperty::new(),
            },
            ..Default::default()
        };
        let styles = Styles::new()
            .add_style(styled("1", "heading 1", None)) // 真 Word 中文版形
            .add_style(styled("H3", "heading 3", None)) // 引擎空格形
            .add_style(styled("c4", "Heading4", None)) // 紧凑形
            .add_style(styled("w1", "标题 5", None)) // 中文空格形
            .add_style(styled("w2", "标题6", None)) // 中文紧凑形
            .add_style(styled("o5", "自定义大纲", Some(4))) // 样式 outlineLvl 链
            .add_style(styled("n", "Normal", None)) // 非标题（不入表）
            .add_style(styled("hn", "Heading", None)) // heading 前缀无数字
            .add_style(styled("h10", "heading 10", None)) // 越域数字
            .add_style(styled("o9", "越域大纲", Some(9))); // outlineLvl 9+
        let idx = StyleIndex::build(&styles);
        // 样式名三形 + 真中文版 styleId 形。
        assert_eq!(
            resolve_heading_level(None, Some("1"), &idx),
            Some(1),
            "真 Word 中文版 styleId=1 → 样式名链命中"
        );
        assert_eq!(resolve_heading_level(None, Some("H3"), &idx), Some(3));
        assert_eq!(resolve_heading_level(None, Some("c4"), &idx), Some(4));
        assert_eq!(resolve_heading_level(None, Some("w1"), &idx), Some(5));
        assert_eq!(resolve_heading_level(None, Some("w2"), &idx), Some(6));
        // 样式 outlineLvl 链（0-8 → +1）。
        assert_eq!(resolve_heading_level(None, Some("o5"), &idx), Some(5));
        // 非标题 / 无数字 / 越域 → None（不误判）。
        assert_eq!(resolve_heading_level(None, Some("n"), &idx), None);
        assert_eq!(resolve_heading_level(None, Some("hn"), &idx), None);
        assert_eq!(resolve_heading_level(None, Some("h10"), &idx), None);
        assert_eq!(resolve_heading_level(None, Some("o9"), &idx), None);
        assert_eq!(resolve_heading_level(None, Some("absent"), &idx), None);
        assert_eq!(resolve_heading_level(None, None, &idx), None);
        // pStyle 前缀链（无样式表条目也命中——引擎写面发射形）。
        assert_eq!(resolve_heading_level(None, Some("Heading7"), &idx), Some(7));
        assert_eq!(resolve_heading_level(None, Some("Heading0"), &idx), None);
        assert_eq!(resolve_heading_level(None, Some("Heading10"), &idx), None);
        // 段落级 outlineLvl 直接格式最优先（压样式链）。
        assert_eq!(
            resolve_heading_level(Some(0), Some("1"), &idx),
            Some(1),
            "直接格式 0 → 1"
        );
        assert_eq!(
            resolve_heading_level(Some(2), Some("H3"), &idx),
            Some(3),
            "直接格式压样式名（同值形）"
        );
        assert_eq!(resolve_heading_level(Some(8), None, &idx), Some(9), "钳 9");
        assert_eq!(
            resolve_heading_level(Some(9), None, &idx),
            None,
            "outlineLvl 9 越域防御忽略"
        );
    }

    /// #14（A2）：域状态机单元矩阵（真夹具/写读环见
    /// tests/tr14_field_machine.rs）。
    #[test]
    fn field_machine_volatile_matrix_and_lifecycle() {
        use docx_rs::FieldCharType;
        // 指令首词分派（含开关参数形 + 大小写混合）。
        for instr in [
            " PAGE \\* MERGEFORMAT ",
            "page",
            "NUMPAGES",
            "PAGEREF _Toc1 \\h",
            "DATE \\@ \"yyyy\"",
            "TIME",
            "PRINTDATE",
            "CREATEDATE",
            "SAVEDATE",
            "EDITTIME",
            "REVNUM",
            "FILENAME",
            "SECTIONPAGES",
        ] {
            assert!(FieldMachine::is_volatile(instr), "volatile: {instr:?}");
        }
        for instr in [
            " REF _Toc1 \\h ",
            " HYPERLINK \"http://x\" ",
            " MERGEFIELD a ",
            " TOC \\o \"1-3\" ",
            " SEQ Figure ",
            " DOCPROPERTY x ",
            "",
            "未知域指令",
        ] {
            assert!(!FieldMachine::is_volatile(instr), "content: {instr:?}");
        }
        // 生命周期：begin → instr → separate（判定定格）→ 结果区剥离
        // → end（计数 + 复位）。
        let mut fm = FieldMachine::default();
        fm.transition(&FieldCharType::Begin);
        assert!(!fm.suppress(), "begin 后未入结果区");
        fm.instr.push_str(" PAGE \\* MERGEFORMAT ");
        fm.transition(&FieldCharType::Separate);
        assert!(fm.suppress(), "PAGE 域结果区剥离");
        fm.transition(&FieldCharType::End);
        assert_eq!(fm.dropped, 1, "表现型域含结果区 → 计数");
        assert!(!fm.suppress(), "end 后复位");
        // 内容型域：结果区不剥离、零计数。
        let mut fm2 = FieldMachine::default();
        fm2.transition(&FieldCharType::Begin);
        fm2.instr.push_str(" REF _Toc1 \\h ");
        fm2.transition(&FieldCharType::Separate);
        assert!(!fm2.suppress(), "REF 域结果保留");
        fm2.transition(&FieldCharType::End);
        assert_eq!(fm2.dropped, 0);
        // 无结果区（begin → instr → end：写面 PAGE 形）：零剥离零计数。
        let mut fm3 = FieldMachine::default();
        fm3.transition(&FieldCharType::Begin);
        fm3.instr.push_str("PAGE");
        fm3.transition(&FieldCharType::End);
        assert_eq!(fm3.dropped, 0, "无 separate = 无缓存结果面");
        // 静默 begin（无指令）→ separate：空指令非表现型 → 保守保留。
        let mut fm4 = FieldMachine::default();
        fm4.transition(&FieldCharType::Begin);
        fm4.transition(&FieldCharType::Separate);
        assert!(!fm4.suppress(), "未知/空指令保守保留");
    }

    #[test]
    fn run_spec_constructors() {
        assert_eq!(
            RunSpec::text("a"),
            RunSpec {
                text: "a".into(),
                bold: false,
                italic: false,
                mono: false,
                strike: false,
                underline: false,
                link: None,
                anchor: None,
                footnote: None,
                field: None,
                char_style: None,
            }
        );
        assert!(RunSpec::styled("a", true, false).bold);
        assert_eq!(
            RunSpec::link("a", "https://x.example/"),
            RunSpec {
                text: "a".into(),
                bold: false,
                italic: false,
                mono: false,
                strike: false,
                underline: false,
                link: Some("https://x.example/".into()),
                anchor: None,
                footnote: None,
                field: None,
                char_style: None,
            }
        );
        // docx-r2 构造器：脚注 marker / 两个简单域。
        assert_eq!(
            RunSpec::footnote("注"),
            RunSpec {
                text: String::new(),
                bold: false,
                italic: false,
                mono: false,
                strike: false,
                underline: false,
                link: None,
                anchor: None,
                footnote: Some("注".into()),
                field: None,
                char_style: None,
            }
        );
        assert_eq!(RunSpec::field_page().field, Some(FieldSpec::Page));
        assert_eq!(RunSpec::field_num_pages().field, Some(FieldSpec::NumPages));
        assert_eq!(RunSpec::field_page().text, "");
        // t20 三布尔位：既有构造器恒 false（零漂移）；styled_full 全位。
        assert!(
            !RunSpec::text("a").mono && !RunSpec::text("a").strike && !RunSpec::text("a").underline
        );
        assert!(!RunSpec::styled("a", true, true).mono);
        assert!(
            RunSpec::styled_full("a", false, false, true, true, true).mono
                && RunSpec::styled_full("a", false, false, true, true, true).strike
                && RunSpec::styled_full("a", false, false, true, true, true).underline
        );
        assert!(!RunSpec::styled_full("a", false, false, true, true, true).bold);
    }

    /// t20：list_item ilvl typed 验证（0-8 过 / 9 拒——与 heading
    /// 级验证同族）。
    #[test]
    fn list_item_ilvl_validation() {
        let mut w = DocxWriter::new();
        assert!(
            w.list_item(&[RunSpec::text("a")], ListKind::Ordered, MAX_LIST_LEVEL)
                .is_ok()
        );
        assert!(
            w.list_item(&[RunSpec::text("a")], ListKind::Bullet, 0)
                .is_ok()
        );
        assert!(matches!(
            w.list_item(&[RunSpec::text("a")], ListKind::Bullet, MAX_LIST_LEVEL + 1),
            Err(DocxError::Parse(m)) if m.contains("ilvl")
        ));
    }

    /// t22：list_item_lines 验证面与模型级（单段落、br run 行边界
    /// 分隔、pPr numbering/ind 双挂）。XML 面（`w:br w:type` / ind
    /// 值直断）见 tests/docx_roundtrip.rs `t22_list_item_multiline_xml_level`。
    #[test]
    fn list_item_lines_model_level() {
        let mut w = DocxWriter::new();
        // ilvl 越界拒（与 list_item 同族 typed 验证）。
        assert!(matches!(
            w.list_item_lines(&[vec![RunSpec::text("a")]], ListKind::Bullet, MAX_LIST_LEVEL + 1),
            Err(DocxError::Parse(m)) if m.contains("ilvl")
        ));
        w.list_item_lines(
            &[
                vec![
                    RunSpec::text("head"),
                    RunSpec::styled_full("b", true, false, false, false, false),
                ],
                vec![],
                vec![RunSpec::text("tail")],
            ],
            ListKind::Bullet,
            1,
        )
        .unwrap();
        // 单段：一个列表项 = 一个 OOXML 段落。
        assert_eq!(w.docx.document.children.len(), 1);
        let docx_rs::DocumentChild::Paragraph(p) = &w.docx.document.children[0] else {
            panic!("应为段落");
        };
        // pPr：numbering（numId 101 / ilvl 1）+ ind（left = 720×2、
        // hanging 360——与抽象层级同值悬挂几何）。
        let np = p.property.numbering_property.as_ref().expect("numPr");
        assert_eq!(
            np.id.as_ref().map(|i| i.id),
            Some(BULLET_NUM_ID),
            "bullet numId"
        );
        assert_eq!(np.level.as_ref().map(|l| l.val), Some(1), "ilvl 1");
        let ind = p.property.indent.as_ref().expect("段落级 ind");
        assert_eq!(
            ind.start,
            Some(QUOTE_INDENT_STEP * 2),
            "left = 720×(ilvl+1)"
        );
        assert_eq!(
            ind.special_indent,
            Some(docx_rs::SpecialIndentType::Hanging(LIST_HANGING_INDENT)),
            "hanging = 编号列宽"
        );
        // run 子序：head / b / br /（空行无 run）/ br / tail——br 恒在
        // 行边界（空行仅贡献 br）。
        let mut seq = Vec::new();
        for child in &p.children {
            let docx_rs::ParagraphChild::Run(r) = child else {
                panic!("应为 run");
            };
            for c in &r.children {
                seq.push(match c {
                    docx_rs::RunChild::Text(t) => format!("t:{}", t.text),
                    docx_rs::RunChild::Break(_) => "br".to_string(),
                    other => format!("other:{other:?}"),
                });
            }
        }
        assert_eq!(seq, vec!["t:head", "t:b", "br", "br", "t:tail"]);
    }

    /// t20：specs_to_paragraph 三新位引擎接线（模型级——rFonts/strike/
    /// underline 落 RunProperty）。
    #[test]
    fn specs_expand_mono_strike_underline() {
        let p = specs_to_paragraph(&[
            RunSpec::styled_full("code", false, false, true, false, false),
            RunSpec::styled_full("gone", false, false, false, true, false),
            RunSpec::styled_full("ul", false, false, false, false, true),
            RunSpec::text("plain"),
        ]);
        let mut runs = Vec::new();
        for child in &p.children {
            let docx_rs::ParagraphChild::Run(r) = child else {
                panic!("应为 run");
            };
            runs.push(r);
        }
        assert_eq!(runs.len(), 4);
        // mono → rFonts 三槽 Consolas（ascii/hAnsi/cs）。
        assert_eq!(runs[0].run_property.fonts, Some(mono_fonts(MONO_FONT)));
        assert!(runs[0].run_property.strike.is_none() && runs[0].run_property.underline.is_none());
        // strike → w:strike（val=true）。
        assert_eq!(runs[1].run_property.strike, Some(docx_rs::Strike::new()));
        assert!(runs[1].run_property.fonts.is_none() && runs[1].run_property.underline.is_none());
        // underline → w:u val="single"。
        assert_eq!(
            runs[2].run_property.underline,
            Some(docx_rs::Underline::new("single"))
        );
        assert!(runs[2].run_property.strike.is_none() && runs[2].run_property.fonts.is_none());
        // plain run 零新位（既有面零漂移）。
        assert!(
            runs[3].run_property.fonts.is_none()
                && runs[3].run_property.strike.is_none()
                && runs[3].run_property.underline.is_none()
        );
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
        assert!(
            w.iter().any(
                |x| x.kind == WarningKind::MalformedXml && x.detail.contains("unknown element")
            )
        );
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
        assert!(
            w.iter().any(
                |x| x.kind == WarningKind::MalformedXml && x.detail.contains("unknown element")
            )
        );
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

    // -----------------------------------------------------------------
    // r4-t2 资源上限（段落/run 计数漏斗 + 输入字节 + docx 族门）
    // -----------------------------------------------------------------

    /// W9/W10 边界（check_document_scale 单点比较——save 漏斗与消费
    /// 方预检共用）：恰 = 限过；段落限 + 1 / run 限 + 1 → typed Limit
    /// 且消息可诊断。
    #[test]
    fn document_scale_limit_boundaries() {
        assert!(check_document_scale(MAX_PARAGRAPHS, MAX_RUNS).is_ok());
        assert!(check_document_scale(0, 0).is_ok());
        match check_document_scale(MAX_PARAGRAPHS + 1, 0) {
            Err(DocxError::Limit(m)) => {
                assert!(m.contains("paragraph count over limit"), "{m}");
            }
            other => panic!("expected paragraph Limit, got {other:?}"),
        }
        match check_document_scale(0, MAX_RUNS + 1) {
            Err(DocxError::Limit(m)) => {
                assert!(m.contains("run count over limit"), "{m}");
            }
            other => panic!("expected run Limit, got {other:?}"),
        }
    }

    /// W9/W10 save 漏斗（发射方法不可变签名 → 资源限统一在 save 面
    /// typed 拒）：私有计数直置超限（避免真实 1M 段构建——基准面在
    /// examples/bench_limits.rs）→ save_to_vec Limit；**save_to_path
    /// 错误路径零产物文件**（fs::write 只在成功 save 后执行——导出
    /// 失败不留完整文件，API 形态核实）。
    #[test]
    fn docx_save_funnel_rejects_over_scale_no_artifact() {
        let mut w = DocxWriter::new();
        w.heading(1, "ok").unwrap();
        w.paragraphs = MAX_PARAGRAPHS + 1;
        match w.save_to_vec() {
            Err(DocxError::Limit(m)) => {
                assert!(m.contains("paragraph count over limit"), "{m}");
            }
            other => panic!("expected save Limit, got {other:?}"),
        }
        let p = std::env::temp_dir().join(format!(
            "mdc_docx_test_noartifact_{}.docx",
            std::process::id()
        ));
        let ps = p.to_string_lossy().into_owned();
        assert!(w.save_to_path(&ps).is_err());
        assert!(!p.exists(), "超限导出不得留产物文件");
        let _ = std::fs::remove_file(&p);
    }

    /// W9/W10 计数接线准确性（全部发射面）：heading/paragraph（含
    /// 脚注 +1段+1run）/table（每格 1段+1run）/header/footer/code_line/
    /// horizontal_rule（零 run）/quote/list_item/list_item_lines/image
    /// ——限内正常保存。
    #[test]
    fn docx_paragraph_run_counting_wiring() {
        let mut w = DocxWriter::new();
        w.heading(1, "h").unwrap(); // 1p + 1r
        w.paragraph(&[
            // 1p + 3r + 脚注内容段 1p + 1r
            RunSpec::text("a"),
            RunSpec::text("b"),
            RunSpec::footnote("n"),
        ]);
        w.table(&[
            // 3 格 → 3p + 3r
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string()],
        ]);
        w.header(&[RunSpec::text("H")]); // 1p + 1r
        w.footer(&[RunSpec::text("F")]); // 1p + 1r
        w.code_line("let x = 1;"); // 1p + 1r
        w.horizontal_rule(); // 1p + 0r
        w.quote_paragraph(&[RunSpec::text("q")], 1); // 1p + 1r
        w.list_item(&[RunSpec::text("i")], ListKind::Bullet, 0)
            .unwrap(); // 1p + 1r
        w.list_item_lines(
            &[
                vec![RunSpec::text("l1"), RunSpec::text("l2")],
                vec![RunSpec::text("l3")],
            ],
            ListKind::Ordered,
            0,
        )
        .unwrap(); // 1p + 3r
        w.image(
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4],
            320,
            240,
        )
        .unwrap(); // 1p + 1r
        // 14 段：heading 1 + paragraph 2（含脚注内容段）+ table 3 +
        // header/footer/code/hr/quote/list_item/list_item_lines/image 各 1。
        assert_eq!(w.paragraphs, 14);
        // 17 run：heading 1 + paragraph 4（3 run + 脚注 1）+ table 3 +
        // header 1 + footer 1 + code 1 + hr 0 + quote 1 + list_item 1 +
        // list_item_lines 3 + image 1。
        assert_eq!(w.runs, 17);
        let bytes = w.save_to_vec().unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    /// D1 输入字节上限（path 入口）：fs::read 全量入内存**之前**元
    /// 数据检查（原无前置 = 真实内存面）——恰 = 限过门 0（败于 zip
    /// 结构 Io 面）；限 + 1 / 极端 4× → typed 安全拒绝；compat 同限。
    /// 稀疏文件构造零实际磁盘写。
    #[test]
    fn docx_input_size_limit_path_entry() {
        let p = std::env::temp_dir().join(format!(
            "mdc_docx_test_inputsize_{}.docx",
            std::process::id()
        ));
        let ps = p.to_string_lossy().into_owned();
        let set_len = |n: u64| {
            let f = std::fs::File::create(&p).unwrap();
            f.set_len(n).unwrap();
        };
        set_len(io::LIMIT_INPUT_BYTES_DOCX);
        match read_docx(&ps) {
            // 过门 0（元数据 ≤ 限），fs::read 后败于 zip 结构（容器面）。
            Err(DocxError::Container(io::ReadError::ContainerCorrupt(_))) => {}
            other => panic!("expected container corrupt at exact limit, got {other:?}"),
        }
        set_len(io::LIMIT_INPUT_BYTES_DOCX + 1);
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_docx_with_options(&ps, &opts) {
                Err(DocxError::Container(io::ReadError::PrescanRejected { reason, .. })) => {
                    assert_eq!(reason, "input size over limit");
                }
                other => panic!("expected input size reject, got {other:?}"),
            }
        }
        set_len(io::LIMIT_INPUT_BYTES_DOCX * 4);
        match read_docx(&ps) {
            Err(DocxError::Container(io::ReadError::PrescanRejected { reason, .. })) => {
                assert_eq!(reason, "input size over limit")
            }
            other => panic!("expected input size reject, got {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// D2 输入字节上限（bytes 入口）+ 借用化（r4-t2 去 to_vec 复制）
    ///——与 path 同限（四入口统一）。
    #[test]
    fn docx_input_size_limit_bytes_entry() {
        let at = vec![0u8; io::LIMIT_INPUT_BYTES_DOCX as usize];
        match read_docx_from_bytes(&at) {
            Err(DocxError::Container(io::ReadError::ContainerCorrupt(_))) => {}
            other => panic!("expected container corrupt at exact limit, got {other:?}"),
        }
        drop(at);
        let over = vec![0u8; io::LIMIT_INPUT_BYTES_DOCX as usize + 1];
        for opts in [ReadOptions::default(), ReadOptions { compat: true }] {
            match read_docx_from_bytes_with_options(&over, &opts) {
                Err(DocxError::Container(io::ReadError::PrescanRejected { reason, .. })) => {
                    assert_eq!(reason, "input size over limit");
                }
                other => panic!("expected input size reject, got {other:?}"),
            }
        }
    }

    /// D3 docx 族声明限端到端（read 管线经 prescan_docx——r4-t2 族
    /// 分化，此前误用 xlsx 族 300MB 常量）：单条目 > 50MB → 容器门
    /// typed 拒（DocxError::Container(PrescanRejected)）。
    #[test]
    fn docx_family_declared_limit_end_to_end() {
        // 伪造声明尺寸原始 zip（STORED 零数据——prescan 只读中央目录）。
        let mut z = Vec::new();
        let declared = (io::LIMIT_TOTAL_DECLARED_DOCX + 1) as u32;
        let name = b"word/document.xml";
        z.extend_from_slice(b"PK\x03\x04");
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes()); // STORED
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes()); // crc
        z.extend_from_slice(&declared.to_le_bytes());
        z.extend_from_slice(&declared.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(name);
        let cd_off = z.len() as u32;
        z.extend_from_slice(b"PK\x01\x02");
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&20u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&declared.to_le_bytes());
        z.extend_from_slice(&declared.to_le_bytes());
        z.extend_from_slice(&(name.len() as u16).to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(name);
        let cd_size = z.len() as u32 - cd_off;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&cd_size.to_le_bytes());
        z.extend_from_slice(&cd_off.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());

        match read_docx_from_bytes(&z) {
            Err(DocxError::Container(io::ReadError::PrescanRejected { reason, entry })) => {
                assert_eq!(reason, "single entry declared size over limit");
                assert_eq!(entry.as_deref(), Some("word/document.xml"));
            }
            other => panic!("expected docx family prescan reject, got {other:?}"),
        }
    }

    /// r4-t3：NumberingIndex 构建 + carry_numbering 五路径
    ///（解析有序/无序、numId=0、无 numId、num 桥悬空、层级缺定义）。
    /// 索引级断言——端到端（写读全环/悬空包注入）见
    /// tests/docx_roundtrip.rs 本轮新测试。
    #[test]
    fn numbering_index_and_carry_paths() {
        use docx_rs::{
            AbstractNumbering, IndentLevel, Level, LevelJc, LevelOverride, LevelText, NumberFormat,
            Numbering, NumberingId, NumberingProperty, Start,
        };
        // 抽象 5：0 级 decimal；抽象 6：0-1 级 bullet。
        let a5 = AbstractNumbering::new(5).add_level(Level::new(
            0,
            Start::new(1),
            NumberFormat::new("decimal"),
            LevelText::new("%1."),
            LevelJc::new("left"),
        ));
        let a6 = AbstractNumbering::new(6)
            .add_level(Level::new(
                0,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new("\u{2022}"),
                LevelJc::new("left"),
            ))
            .add_level(Level::new(
                1,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new("\u{25E6}"),
                LevelJc::new("left"),
            ));
        // num 桥 50→5（lvlOverride 覆盖 0 级为 lowerLetter）；51→6。
        let mut o0 = LevelOverride::new(0);
        o0.override_level = Some(Level::new(
            0,
            Start::new(1),
            NumberFormat::new("lowerLetter"),
            LevelText::new("%1."),
            LevelJc::new("left"),
        ));
        let n50 = Numbering::new(50, 5).add_override(o0);
        let n51 = Numbering::new(51, 6);
        let engine_numberings = docx_rs::Numberings::new()
            .add_abstract_numbering(a5)
            .add_abstract_numbering(a6)
            .add_numbering(n50)
            .add_numbering(n51);
        let index = NumberingIndex::build(&engine_numberings);
        // lvlOverride 并入：50 的 0 级 = lowerLetter（非 decimal）。
        assert!(index.by_num_id[&50].get(&0).map(String::as_str) == Some("lowerLetter"));
        assert!(index.by_num_id[&51].get(&1).map(String::as_str) == Some("bullet"));

        let part = "word/document.xml";
        let mut warnings = Vec::new();
        let carry = |np: NumberingProperty, warnings: &mut Vec<CompatibilityWarning>| {
            let mut w = std::mem::take(warnings);
            let r = carry_numbering(&np, &index, part, false, &mut w);
            *warnings = w;
            r.unwrap()
        };

        // ① 解析有序（numId 50 ilvl 0——override 后 lowerLetter → Ordered
        //    + format 可诊断降级）。
        let np = NumberingProperty::new().add_num(NumberingId::new(50), IndentLevel::new(0));
        let c = carry(np, &mut warnings);
        assert_eq!(
            c,
            Some(ParagraphNumbering {
                num_id: 50,
                ilvl: 0,
                kind: Some(ListKind::Ordered),
                format: Some("lowerLetter".to_string()),
            })
        );
        // ② 解析无序（numId 51 ilvl 1）。
        let np = NumberingProperty::new().add_num(NumberingId::new(51), IndentLevel::new(1));
        let c = carry(np, &mut warnings).unwrap();
        assert_eq!(c.kind, Some(ListKind::Bullet));
        assert_eq!(c.format.as_deref(), Some("bullet"));
        // ③ numId=0（OOXML 去编号惯例）→ None。
        let np = NumberingProperty::new().add_num(NumberingId::new(0), IndentLevel::new(0));
        assert_eq!(carry(np, &mut warnings), None);
        // ④ 无 numId（仅 ilvl——引擎构造器 level 为私有，直接字段置入）。
        let mut np = NumberingProperty::new();
        np.level = Some(IndentLevel::new(3));
        assert_eq!(carry(np, &mut warnings), None);
        // ⑤ num 桥悬空（兼容）：警告 + kind=None 结构承载。
        let np = NumberingProperty::new().add_num(NumberingId::new(777), IndentLevel::new(2));
        let c = carry(np, &mut warnings);
        assert_eq!(
            c,
            Some(ParagraphNumbering {
                num_id: 777,
                ilvl: 2,
                kind: None,
                format: None,
            })
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::BrokenReference && w.detail.contains("dangling")),
            "{warnings:?}"
        );
        warnings.clear();
        // ⑥ 层级缺定义（numId 51 ilvl 9 超定义域）：软警告 + 结构承载。
        let np = NumberingProperty::new().add_num(NumberingId::new(51), IndentLevel::new(9));
        let c = carry(np, &mut warnings).unwrap();
        assert_eq!(c.num_id, 51);
        assert_eq!(c.ilvl, 9);
        assert_eq!(c.kind, None);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::BrokenReference
                    && w.detail.contains("no level definition")),
            "{warnings:?}"
        );
        // ⑦ num 桥悬空（严格）：typed 拒。
        let mut sink = Vec::new();
        let np = NumberingProperty::new().add_num(NumberingId::new(777), IndentLevel::new(0));
        match carry_numbering(&np, &index, part, true, &mut sink) {
            Err(io::ReadError::InvalidPart { detail, .. }) => {
                assert!(detail.contains("dangling"), "{detail}")
            }
            other => panic!("strict should reject dangling numId, got {other:?}"),
        }
    }

    /// r4-t3：w:ind → ParagraphIndent 映射（left/right/Hanging/FirstLine）。
    #[test]
    fn indent_carry_variants() {
        let hanging = carry_indent(&docx_rs::Indent::new(
            Some(720),
            Some(docx_rs::SpecialIndentType::Hanging(360)),
            Some(120),
            None,
        ));
        assert_eq!(
            hanging,
            ParagraphIndent {
                left: Some(720),
                right: Some(120),
                special: Some(ParagraphIndentSpecial::Hanging(360)),
            }
        );
        let first_line = carry_indent(&docx_rs::Indent::new(
            Some(360),
            Some(docx_rs::SpecialIndentType::FirstLine(480)),
            None,
            None,
        ));
        assert_eq!(
            first_line,
            ParagraphIndent {
                left: Some(360),
                right: None,
                special: Some(ParagraphIndentSpecial::FirstLine(480)),
            }
        );
        let bare = carry_indent(&docx_rs::Indent::new(Some(720), None, None, None));
        assert_eq!(
            bare,
            ParagraphIndent {
                left: Some(720),
                right: None,
                special: None,
            }
        );
        // 引擎 BuildXML 形态交叉验证：Start 空时引擎发射 w:right="0"
        //（indent.rs 测试 test_left）——读面模型承载的是解析值非发射形。
        assert_eq!(bare.right, None);
    }

    // ── office-g Phase 1/2 锚点写面（实施批 T1-T7——分支待裁决）────────

    /// 测试辅助：读 zip 部件文本（缺失 = None）。
    fn zip_part(bytes: &[u8], name: &str) -> Option<String> {
        let cursor = std::io::Cursor::new(bytes);
        let mut za = match zip::ZipArchive::new(cursor) {
            Ok(z) => z,
            Err(_) => return None,
        };
        let entry = za.by_name(name);
        match entry {
            Ok(mut zf) => {
                let mut data = String::new();
                use std::io::Read;
                zf.read_to_string(&mut data).ok()?;
                Some(data)
            }
            Err(_) => None,
        }
    }

    /// 从读回文档中收集全部 hyperlink 目标（run 级）。
    fn collect_hyperlinks(doc: &DocxDocument) -> Vec<HyperlinkTarget> {
        let mut out = Vec::new();
        for block in &doc.blocks {
            if let Block::Paragraph(pd) = block {
                for r in &pd.runs {
                    if let Some(h) = &r.hyperlink {
                        out.push(h.clone());
                    }
                }
            }
        }
        out
    }

    /// T1：anchor_link 写→读全环——w:anchor 属性形 + 零 rels 面 +
    /// 自有读 Anchor 变体复原。
    #[test]
    fn anchor_t1_roundtrip_attribute_form_zero_rels() {
        let mut w = DocxWriter::new();
        w.paragraph(&[
            RunSpec::text("参见 "),
            RunSpec::anchor_link("第 3 节 方法论", "secMethods"),
            RunSpec::text("。"),
        ]);
        let bytes = w.save_to_vec().expect("悬空锚点 = 合法形（T3 同批钉定）");
        // XML 级：w:anchor 属性形在场；零 r:id 超链接形
        let doc_xml = zip_part(&bytes, "word/document.xml").expect("document.xml");
        assert!(
            doc_xml.contains(r#"w:anchor="secMethods""#),
            "w:anchor 属性形在场"
        );
        assert!(
            !doc_xml.contains("<w:hyperlink r:id="),
            "零 r:id 面（锚点形不经关系层）"
        );
        // rels 级：无 hyperlink 关系条目（部件可能整缺席——两者皆零面）
        if let Some(rels) = zip_part(&bytes, "word/_rels/document.xml.rels") {
            assert!(!rels.contains("hyperlink"), "零 hyperlink 关系：{rels}");
        }
        // 读回：Anchor 变体复原（r5-t2 读面既有变体——写读镜像）
        let doc = read_docx_from_bytes(&bytes).expect("read back");
        let links = collect_hyperlinks(&doc);
        assert_eq!(
            links,
            vec![HyperlinkTarget::Anchor {
                anchor: "secMethods".into()
            }],
            "自有读 Anchor 变体复原"
        );
    }

    /// T2：anchor 与 link 同 Some → anchor 赢、link 忽略（V8 优先序——
    /// 读写镜像：写侧发射 w:anchor 无 r:id；读回 Anchor 变体）。
    #[test]
    fn anchor_t2_priority_over_external_link() {
        let mut w = DocxWriter::new();
        let mut spec = RunSpec::anchor_link("内部跳转", "secMethods");
        spec.link = Some("https://example.com/ext".into()); // 同 Some 对抗位
        w.paragraph(&[spec]);
        let bytes = w.save_to_vec().unwrap();
        let doc_xml = zip_part(&bytes, "word/document.xml").unwrap();
        assert!(doc_xml.contains(r#"w:anchor="secMethods""#));
        assert!(!doc_xml.contains("<w:hyperlink r:id="), "link 被忽略");
        let doc = read_docx_from_bytes(&bytes).unwrap();
        assert_eq!(
            collect_hyperlinks(&doc),
            vec![HyperlinkTarget::Anchor {
                anchor: "secMethods".into()
            }],
            "读回 anchor 赢（V8）"
        );
    }

    /// T3：悬空锚点（目标书签不存在）= 合法形——发射不拒、读回不拒。
    #[test]
    fn anchor_t3_dangling_is_legal_form() {
        let mut w = DocxWriter::new();
        w.paragraph(&[RunSpec::anchor_link("指向不存在书签", "noSuchBookmark")]);
        let bytes = w
            .save_to_vec()
            .expect("悬空 = 合法形（存在性检查是消费侧责任）");
        let doc = read_docx_from_bytes(&bytes).expect("读回不拒");
        assert_eq!(collect_hyperlinks(&doc).len(), 1);
    }

    /// T4：bookmark_paragraph XML 面——bookmarkStart/End 对 + 两书签段。
    #[test]
    fn anchor_t4_bookmark_paragraph_xml_face() {
        let mut w = DocxWriter::new();
        w.bookmark_paragraph(&[RunSpec::text("3. 方法论")], "secMethods")
            .expect("V1-V6 合法名");
        w.bookmark_paragraph(&[RunSpec::text("附录 A")], "appendixA")
            .expect("第二书签 id 唯一");
        let bytes = w.save_to_vec().unwrap();
        let doc_xml = zip_part(&bytes, "word/document.xml").unwrap();
        assert!(doc_xml.contains(r#"w:name="secMethods""#), "书签名 1");
        assert!(doc_xml.contains(r#"w:name="appendixA""#), "书签名 2");
        assert!(doc_xml.contains("bookmarkStart"), "Start 对");
        assert!(doc_xml.contains("bookmarkEnd"), "End 对");
        let starts: Vec<&str> = doc_xml
            .match_indices("bookmarkStart")
            .map(|(i, _)| &doc_xml[i..i + 40])
            .collect();
        assert_eq!(starts.len(), 2, "恰两 bookmarkStart");
    }

    /// T5 + V1-V6 校验矩阵：空名/超长/首字符/字符域/保留名/重名——
    /// 全部 typed InvalidInput 且消息可诊断。
    #[test]
    fn anchor_t5_validation_matrix_v1_to_v6() {
        let mut w = DocxWriter::new();
        let cases: &[(&str, &str)] = &[
            ("", "V1"),
            ("a", "OK"),
            ("a2345678901234567890123456789012345678901", "V2"),
            ("1abc", "V3"),
            ("bad name", "V4"),
            ("bad-name", "V4"),
            ("中文书签", "V3"), // CJK 首字符 → V3 先拒（校验序：首字符先于字符域）
            ("a中文", "V4"),    // 首字符过 V3、域内含 CJK → V4
            ("_Toc123", "V5"),
            ("Toc_ok", "OK"),
        ];
        for (name, expect) in cases {
            let mut w2 = DocxWriter::new();
            let r = w2.bookmark_paragraph(&[RunSpec::text("x")], name);
            if *expect == "OK" {
                assert!(r.is_ok(), "{name:?} 应合法");
            } else {
                match r {
                    Err(DocxError::InvalidInput(m)) => {
                        assert!(m.contains(expect), "{name:?} 消息含 {expect:?}: {m}");
                    }
                    other => panic!("{name:?} 期望 InvalidInput({expect}), 实得 {other:?}"),
                }
            }
        }
        // V6 重名：同名第二段拒绝
        w.bookmark_paragraph(&[RunSpec::text("x")], "dupName")
            .unwrap();
        match w.bookmark_paragraph(&[RunSpec::text("y")], "dupName") {
            Err(DocxError::InvalidInput(m)) => assert!(m.contains("V6"), "{m}"),
            other => panic!("期望 V6 重名拒绝, 实得 {other:?}"),
        }
    }

    /// V7：anchor_link 空锚点——构造不失败（人体工学签名），save 漏斗
    /// 经粘性校验错误拒绝（零产物语义维持）。
    #[test]
    fn anchor_v7_empty_anchor_sticky_reject_at_save() {
        let mut w = DocxWriter::new();
        w.paragraph(&[RunSpec::anchor_link("坏锚点", "")]);
        match w.save_to_vec() {
            Err(DocxError::InvalidInput(m)) => assert!(m.contains("V7"), "{m}"),
            other => panic!("期望 V7 粘性拒绝, 实得 {other:?}"),
        }
        // save_to_path 同漏斗（委托 save_to_vec）——零产物文件
        let p = std::env::temp_dir().join(format!("mdc_anchor_v7_{}.docx", std::process::id()));
        let ps = p.to_string_lossy().into_owned();
        assert!(w.save_to_path(&ps).is_err());
        assert!(!p.exists(), "V7 拒绝零产物文件");
        let _ = std::fs::remove_file(&p);
    }

    /// T7：构造器零漂移——anchor_link 恒等断言（既有构造器 anchor: None
    /// 由 run_spec_constructors 结构字面锁定）。
    #[test]
    fn anchor_t7_constructor_identity() {
        assert_eq!(
            RunSpec::anchor_link("a", "sec1"),
            RunSpec {
                text: "a".into(),
                bold: false,
                italic: false,
                mono: false,
                strike: false,
                underline: false,
                link: None,
                anchor: Some("sec1".into()),
                footnote: None,
                field: None,
                char_style: None,
            }
        );
    }

    // ── G3 暗标匿名化测试组（issue #6 / R-11）──────────────────────

    /// zip 部件字节读取 helper（G3 测试面——既有 `zip_part` 返回
    /// String，此处需要原始字节比较）。
    fn zip_part_bytes(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
        let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
        let mut e = ar.by_name(name).ok()?;
        let mut v = vec![];
        std::io::Read::read_to_end(&mut e, &mut v).ok()?;
        Some(v)
    }

    /// 构造带暗标的 docx（helper：DocxWriter 基底 → 注入暗标
    /// core.xml / custom.xml / comments.xml——模拟用户既有文档）。
    fn build_darkmarked_docx() -> Vec<u8> {
        let mut w = DocxWriter::new();
        w.heading(1, "既有文档").unwrap();
        w.paragraph_text("用户既有正文——暗标随导出泄出的威胁面。");
        let base = w.save_to_vec().unwrap();
        let mut reader = zip::ZipArchive::new(std::io::Cursor::new(&base)).unwrap();
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        use std::io::Write as _;
        for i in 0..reader.len() {
            let mut entry = reader.by_index(i).unwrap();
            let name = entry.name().to_string();
            let replacement: &[u8] = match name.as_str() {
                "docProps/core.xml" => r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:creator>张三</dc:creator><cp:lastModifiedBy>李四</cp:lastModifiedBy><dcterms:created xsi:type="dcterms:W3CDTF" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">2026-10-26T09:00:00Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">2026-10-26T10:30:00Z</dcterms:modified><cp:revision>42</cp:revision></cp:coreProperties>"#.as_bytes(),
                "docProps/custom.xml" => r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Company"><vt:lpstr>某某科技有限公司</vt:lpstr></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="Sender"><vt:lpstr>王五</vt:lpstr></property></Properties>"#.as_bytes(),
                "word/comments.xml" => r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="0" w:author="赵六" w:date="2026-10-20T08:00:00Z" w:initials="ZL"><w:p><w:r><w:t>批注内容文本</w:t></w:r></w:p></w:comment></w:comments>"#.as_bytes(),
                _ => {
                    use std::io::Read as _;
                    let mut buf = vec![];
                    entry.read_to_end(&mut buf).unwrap();
                    writer
                        .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                        .unwrap();
                    writer.write_all(&buf).unwrap();
                    continue;
                }
            };
            writer
                .start_file(name.clone(), zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(replacement).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// T1：引擎新建面已匿名 + anonymize 幂等（core 逐字节 = 匿名常量
    /// ——新建与匿名导出元数据面同构 = 单一真源）。
    #[test]
    fn g3_t1_new_face_already_anonymous_and_idempotent() {
        let mut w = DocxWriter::new();
        w.heading(1, "新建面").unwrap();
        let fresh = w.save_to_vec().unwrap();
        let core_fresh =
            String::from_utf8(zip_part_bytes(&fresh, "docProps/core.xml").unwrap()).unwrap();
        assert_eq!(core_fresh, ANON_CORE_XML, "引擎新建面 core.xml = 匿名常量");

        let once = anonymize_docx_props(&fresh).unwrap();
        assert_eq!(
            String::from_utf8(zip_part_bytes(&once, "docProps/core.xml").unwrap()).unwrap(),
            ANON_CORE_XML,
            "匿名化后 core.xml 逐字节 = 匿名常量（幂等）"
        );
        // 正文部件字节不变
        assert_eq!(
            zip_part_bytes(&fresh, "word/document.xml"),
            zip_part_bytes(&once, "word/document.xml"),
            "正文零触碰"
        );
        // 匿名产物仍可解析（Word 可开的最小代理）
        assert!(read_docx_from_bytes(&once).is_ok());
    }

    /// T2：暗标全清——creator/lastModifiedBy/Company/Sender/批注作者
    /// 全部清零，正文与批注文本保留。
    #[test]
    fn g3_t2_darkmark_full_scrub() {
        let dark = build_darkmarked_docx();
        let doc_before = zip_part_bytes(&dark, "word/document.xml").unwrap();
        let anon = anonymize_docx_props(&dark).unwrap();
        let core = String::from_utf8(zip_part_bytes(&anon, "docProps/core.xml").unwrap()).unwrap();
        assert_eq!(core, ANON_CORE_XML);
        assert!(!core.contains("张三") && !core.contains("李四") && !core.contains("42"));
        let custom =
            String::from_utf8(zip_part_bytes(&anon, "docProps/custom.xml").unwrap()).unwrap();
        assert_eq!(custom, ANON_CUSTOM_XML);
        assert!(!custom.contains("某某科技") && !custom.contains("王五"));
        let comments =
            String::from_utf8(zip_part_bytes(&anon, "word/comments.xml").unwrap()).unwrap();
        assert!(
            !comments.contains("赵六") && !comments.contains("ZL"),
            "批注作者/缩写清零"
        );
        assert!(comments.contains("批注内容文本"), "批注文本内容保留");
        assert!(
            comments.contains(r#"w:author="""#) && comments.contains("1970-01-01T00:00:00Z"),
            "author 置空 + date → epoch"
        );
        assert_eq!(
            zip_part_bytes(&anon, "word/document.xml").unwrap(),
            doc_before,
            "正文部件原压缩流字节不变"
        );
        assert!(read_docx_from_bytes(&anon).is_ok(), "匿名产物可解析");
    }

    /// T3：缺部件容忍（无 docProps 的极简 zip——docProps 面跳过，
    /// 正文照常原样拷贝）。
    #[test]
    fn g3_t3_missing_docprops_tolerated() {
        use std::io::Write as _;
        let mut w = DocxWriter::new();
        w.paragraph_text("极简档");
        let base = w.save_to_vec().unwrap();
        let doc = zip_part_bytes(&base, "word/document.xml").unwrap();
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(&doc).unwrap();
        let minimal = writer.finish().unwrap().into_inner();
        let anon = anonymize_docx_props(&minimal).unwrap();
        assert_eq!(zip_part_bytes(&anon, "word/document.xml").unwrap(), doc);
        assert!(
            zip_part_bytes(&anon, "docProps/core.xml").is_none(),
            "未注入新部件（最小干预）"
        );
    }

    /// G2-T1：页面几何发射（pgSz/pgMar 自定义值 XML 断言）。
    #[test]
    fn g2_t1_page_geometry_emission() {
        let mut w = DocxWriter::new();
        w.page_geometry(PageGeometry {
            width: 11906,
            height: 16838,
            margin_top: 1440,
            margin_right: 1800,
            margin_bottom: 1440,
            margin_left: 1800,
            margin_header: 851,
            margin_footer: 992,
            gutter: 0,
        });
        w.paragraph_text("页面几何探针");
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(
            doc.contains(r#"<w:pgSz w:w="11906" w:h="16838""#),
            "pgSz 在场"
        );
        assert!(
            doc.contains(r#"w:top="1445""#) || doc.contains(r#"w:top="1440""#),
            "pgMar top 自定义值在场"
        );
        assert!(
            doc.contains(r#"w:left="1800""#) && doc.contains(r#"w:right="1800""#),
            "pgMar 左右自定义值"
        );
        assert!(read_docx_from_bytes(&bytes).is_ok());
    }

    /// G2-T2：正文默认排版发射（docDefaults rFonts/sz/spacing——
    /// styles.xml 断言）。
    #[test]
    fn g2_t2_default_typography_emission() {
        let mut w = DocxWriter::new();
        w.default_typography(DefaultTypography {
            ascii_font: "Times New Roman".into(),
            east_asia_font: "宋体".into(),
            complex_script_font: "Times New Roman".into(),
            size_half_points: 28, // 四号 14pt
            line_spacing: Some(LineSpacingSpec {
                line: Some(360),
                line_rule: Some(LineRuleSpec::Auto), // 1.5 倍
                before: Some(120),
                after: Some(120),
            }),
        });
        w.paragraph_text("排版探针");
        let bytes = w.save_to_vec().unwrap();
        let st = String::from_utf8(zip_part_bytes(&bytes, "word/styles.xml").unwrap()).unwrap();
        assert!(st.contains("Times New Roman"), "ascii 字体在场");
        assert!(st.contains("宋体"), "eastAsia 字体在场");
        assert!(
            st.contains(r#"<w:sz w:val="28""#) || st.contains(r#"w:val="28""#),
            "字号 28 半磅在场"
        );
        assert!(
            st.contains(r#"w:line="360""#),
            "行距 360（1.5 倍 auto）在场"
        );
        assert!(
            st.contains("w:lineRule") || st.contains("auto"),
            "lineRule 在场"
        );
        assert!(read_docx_from_bytes(&bytes).is_ok());
    }

    /// G2-T3：V9 域校验——几何零值/超域、字号零/超域、行距非正值
    /// 全部粘性拒绝零产物。
    #[test]
    fn g2_t3_v9_domain_reject() {
        let mut w = DocxWriter::new();
        w.page_geometry(PageGeometry {
            width: 0,
            ..PageGeometry::a4_default()
        });
        let e = w.save_to_vec().unwrap_err();
        assert!(matches!(e, DocxError::InvalidInput(ref m) if m.starts_with("V9")));
        let mut w = DocxWriter::new();
        w.page_geometry(PageGeometry {
            margin_top: -5,
            ..PageGeometry::a4_default()
        });
        assert!(matches!(
            w.save_to_vec().unwrap_err(),
            DocxError::InvalidInput(_)
        ));
        let mut w = DocxWriter::new();
        w.default_typography(DefaultTypography {
            ascii_font: "x".into(),
            east_asia_font: "y".into(),
            complex_script_font: "x".into(),
            size_half_points: 0,
            line_spacing: None,
        });
        assert!(matches!(
            w.save_to_vec().unwrap_err(),
            DocxError::InvalidInput(_)
        ));
        let mut w = DocxWriter::new();
        w.default_typography(DefaultTypography {
            ascii_font: "x".into(),
            east_asia_font: "y".into(),
            complex_script_font: "x".into(),
            size_half_points: 21,
            line_spacing: Some(LineSpacingSpec {
                line: Some(-240),
                line_rule: Some(LineRuleSpec::Auto),
                before: None,
                after: None,
            }),
        });
        assert!(matches!(
            w.save_to_vec().unwrap_err(),
            DocxError::InvalidInput(_)
        ));
    }

    /// G2-T4：基线不变量——不调用两设置面时产物与引擎默认逐项一致
    /// （A4 + Word 2019 边距 + 空 docDefaults——既有消费者零漂移）。
    #[test]
    fn g2_t4_baseline_unchanged() {
        let mut w = DocxWriter::new();
        w.paragraph_text("基线");
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(
            doc.contains(r#"w:top="1985""#) && doc.contains(r#"w:left="1701""#),
            "引擎默认边距恒"
        );
        let st = String::from_utf8(zip_part_bytes(&bytes, "word/styles.xml").unwrap()).unwrap();
        assert!(
            st.contains("<w:rPrDefault><w:rPr /></w:rPrDefault>"),
            "docDefaults 基线恒（空 rPr）"
        );
    }

    /// G1-T1：TOC 域五连发射（begin dirty + instrText + separate +
    /// 占位 + end）——XML 面断言。
    #[test]
    fn g1_t1_toc_five_part_emission() {
        let mut w = DocxWriter::new();
        w.heading(1, "章一").unwrap();
        w.heading(2, "节 1.1").unwrap();
        w.paragraph(&[RunSpec::field_toc_heading_range(1, 3)]);
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(doc.contains("TOC"), "instrText TOC 在场: 无");
        assert!(
            doc.contains(r#"\o "1-3""#) || doc.contains("1-3"),
            "heading range 开关在场"
        );
        assert!(doc.contains("separate"), "separate 域结果分界在场");
        assert!(
            doc.contains("（目录域——打开后全选再按 F9 更新生成）"),
            "占位提示在场"
        );
        // #309③ 域缓存写入：begin 不置 dirty——打开即显缓存占位
        //（无「是否更新域」弹窗）；域结构五连完整保留（F9 重算）。
        // 注：docx-rs 恒显式写 w:dirty="false"（打开不更新域——正是
        // #309③ 语义），断言口径 = dirty="true" 零出现。
        assert_eq!(
            doc.matches(r#"w:dirty="true""#).count(),
            0,
            "dirty=true 须零出现（#309③ 缓存直显——在场即回退）"
        );
        // 可解析往返（读面域结构提取面外为既有登记——这里只证容器完好）
        assert!(read_docx_from_bytes(&bytes).is_ok());
    }

    /// G1-T2：全层级构造器（无 \o 开关——Word 默认收录）。
    #[test]
    fn g1_t2_toc_full_range_no_switch() {
        let mut w = DocxWriter::new();
        w.paragraph(&[RunSpec::field_toc()]);
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(doc.contains("TOC"));
        assert!(!doc.contains(r"\o "), r"无 \o 开关（默认全层级）");
    }

    /// G1-T3：V8 域校验——越界/倒序粘性拒绝（save 零产物）。
    #[test]
    fn g1_t3_v8_domain_reject() {
        for (lo, hi) in [(0, 3), (1, 10), (5, 2), (0, 0), (10, 12)] {
            let mut w = DocxWriter::new();
            w.paragraph(&[RunSpec::field_toc_heading_range(lo, hi)]);
            let err = w.save_to_vec().unwrap_err();
            assert!(
                matches!(err, DocxError::InvalidInput(ref m) if m.starts_with("V8")),
                "({lo},{hi}) 期望 V8 拒绝，实得 {err:?}"
            );
        }
        // 合法边界恰过：1-9 全域
        let mut w = DocxWriter::new();
        w.paragraph(&[RunSpec::field_toc_heading_range(1, 9)]);
        assert!(w.save_to_vec().is_ok());
    }

    /// G1-T4：TOC + PAGE/NUMPAGES 混排共存（页脚页码 + 正文目录）。
    #[test]
    fn g1_t4_toc_mixed_with_page_fields() {
        let mut w = DocxWriter::new();
        w.heading(1, "章").unwrap();
        w.paragraph(&[RunSpec::field_toc()]);
        w.footer(&[
            RunSpec::text("第 "),
            RunSpec::field_page(),
            RunSpec::text(" / 共 "),
            RunSpec::field_num_pages(),
            RunSpec::text(" 页"),
        ]);
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(doc.contains("TOC"));
        let footer = zip_part_bytes(&bytes, "word/footer1.xml")
            .or_else(|| zip_part_bytes(&bytes, "word/footer2.xml"));
        let footer = footer.expect("footer 部件在场");
        let fs = String::from_utf8(footer).unwrap();
        assert!(
            fs.contains("PAGE") && fs.contains("NUMPAGES"),
            "页脚双域在场"
        );
        assert!(read_docx_from_bytes(&bytes).is_ok());
    }

    /// T4：非 docx 伪产物 fail-closed（无 word/document.xml 的 zip 拒）。
    #[test]
    fn g3_t4_not_docx_rejected() {
        use std::io::Write as _;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("foo.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"not a docx").unwrap();
        let bogus = writer.finish().unwrap().into_inner();
        let err = anonymize_docx_props(&bogus).unwrap_err();
        assert!(matches!(err, DocxError::Io(_)), "fail-closed 拒绝: {err:?}");
    }

    // ── #196 任务 2：PAGEREF + TOC 静态目录项测试组（T8）──────────

    /// T8a：field_pageref 五连 XML 面（begin 非 dirty + instr
    /// `PAGEREF {bm} \h` + separate + 页码占位 "1" + end）。
    #[test]
    fn t8a_field_pageref_five_part_xml_face() {
        let mut w = DocxWriter::new();
        w.paragraph(&[RunSpec::text("页码："), RunSpec::field_pageref("secIntro")]);
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        // 五连逐件在场（begin 非单独 dirty——TOC 域才有 dirty 提示位；
        // 此处断言 begin 在场 + 不多不少一个 PAGEREF 指令）。
        assert!(
            doc.contains(r#"<w:instrText>PAGEREF secIntro \h</w:instrText>"#),
            "PAGEREF 指令（\\h 超链接开关）恰在场"
        );
        assert!(
            doc.matches(r#"w:fldCharType="begin""#).count() == 1,
            "单域 begin 恰一次"
        );
        assert!(
            doc.matches(r#"w:fldCharType="separate""#).count() == 1,
            "separate 恰一次"
        );
        assert!(
            doc.matches(r#"w:fldCharType="end""#).count() == 1,
            "end 恰一次"
        );
        // 页码占位 "1"（域结果区——separate 与 end 之间）。
        let sep = doc.find(r#"w:fldCharType="separate""#).unwrap();
        let end = doc.find(r#"w:fldCharType="end""#).unwrap();
        assert!(doc[sep..end].contains(">1<"), "域结果区页码占位 \"1\" 在场");
        // 可解析往返（读面 PAGEREF = volatile 域，域结果剥离不 panic）。
        assert!(read_docx_from_bytes(&bytes).is_ok());
    }

    /// T8b：toc_entries 三段式跨段域结构 + 跳转闭环配平。
    #[test]
    fn t8b_toc_entries_block_structure_and_jump_closure() {
        let mut w = DocxWriter::new();
        let names = w.toc_entries(
            &[
                TocEntry::new(1, "第一章 引言"),
                TocEntry::new(2, "1.1 背景"),
                TocEntry::new(3, "1.1.1 细节"),
            ],
            Some((1, 3)),
        );
        assert_eq!(names.len(), 3, "书签名按 entries 序返回");
        assert_eq!(names[0], "toc_0", "首名 toc_0（counter 从 0 起）");
        // 正文侧书签定义（跳转闭环另一侧）。
        w.heading_bookmarked(1, "第一章 引言", &names[0]).unwrap();
        w.heading_bookmarked(2, "1.1 背景", &names[1]).unwrap();
        w.heading_bookmarked(3, "1.1.1 细节", &names[2]).unwrap();
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        // 域头：TOC 指令在场 + #309③ 后全文档 dirty 零出现（打开即显
        // 缓存目录项——entries 文本 + 占位页码 "1"，无弹窗）。
        assert!(
            doc.contains(r#"<w:instrText>TOC \o &quot;1-3&quot;</w:instrText>"#),
            "TOC \\o 指令在场"
        );
        assert_eq!(
            doc.matches(r#"w:dirty="true""#).count(),
            0,
            "#309③ 后 TOC 族全非 dirty=true（缓存直显，无打开弹窗）"
        );
        // 跨段域配平：begin/separate/end 各 = TOC(1) + PAGEREF(3)。
        assert_eq!(
            doc.matches(r#"w:fldCharType="begin""#).count(),
            4,
            "begin = TOC + 3×PAGEREF"
        );
        assert_eq!(
            doc.matches(r#"w:fldCharType="separate""#).count(),
            4,
            "separate 同配平"
        );
        assert_eq!(
            doc.matches(r#"w:fldCharType="end""#).count(),
            4,
            "end 同配平"
        );
        // 目录项：hyperlink anchor 引用侧 ×3 + 指令引用侧 ×3。
        for n in &names {
            assert!(
                doc.contains(&format!(r#"<w:hyperlink w:anchor="{n}""#)),
                "hyperlink 引用 {n}"
            );
            assert!(
                doc.contains(&format!("<w:instrText>PAGEREF {n} \\h</w:instrText>")),
                "PAGEREF 指令引用 {n}"
            );
        }
        // 定义侧：bookmarkStart/End 对 ×3（标题段内）。
        assert_eq!(doc.matches("bookmarkStart").count(), 3, "标题书签定义侧 ×3");
        assert_eq!(doc.matches("bookmarkEnd").count(), 3, "配对 End ×3");
        // tab stop：右对齐点线（A4 缺省文本宽 8504）。
        assert!(
            doc.contains(r#"w:leader="dot""#) && doc.contains(r#"w:val="right""#),
            "右对齐点线 tab stop 在场"
        );
        assert!(doc.contains(r#"w:pos="8504""#), "tab 位 = A4 缺省文本宽");
        // 层级缩进：level 2/3 项缩进 240/480。
        assert!(
            doc.contains(r#"w:left="240""#) && doc.contains(r#"w:left="480""#),
            "层级缩进 (level-1)×240"
        );
        // 标题段样式双通道保持（heading_bookmarked 与 heading 同构）。
        assert!(
            doc.matches(r#"w:val="Heading1""#).count() >= 1
                && doc.matches(r#"w:val="Heading3""#).count() >= 1,
            "Heading 样式引用 + outline 双通道保持"
        );
    }

    /// T8c：heading_bookmarked 校验矩阵（heading 域 + V1-V6 书签域
    /// 透传——与 bookmark_paragraph 同纪律）。
    #[test]
    fn t8c_heading_bookmarked_validation() {
        // heading 层级域。
        let mut w = DocxWriter::new();
        assert!(w.heading_bookmarked(0, "x", "a").is_err(), "level 0 拒");
        assert!(w.heading_bookmarked(10, "x", "a").is_err(), "level 10 拒");
        // V5 保留名（toc 通道自有命名 toc_N 不撞；用户直接传 _Toc 拒）。
        let mut w2 = DocxWriter::new();
        assert!(
            w2.heading_bookmarked(1, "x", "_Toc1").is_err(),
            "_Toc 保留名拒（V5）"
        );
        // V6 重名（toc_entries 生成名已被 heading_bookmarked 注册——
        // 再注册同名拒）。
        let mut w3 = DocxWriter::new();
        let names = w3.toc_entries(&[TocEntry::new(1, "章")], None);
        let dup = w3.heading_bookmarked(1, "章", &names[0]);
        assert!(dup.is_ok(), "首注册过（闭环合法路径）");
        let mut w4 = DocxWriter::new();
        let names4 = w4.toc_entries(&[TocEntry::new(1, "章")], None);
        w4.heading_bookmarked(1, "章", &names4[0]).unwrap();
        assert!(
            w4.heading_bookmarked(1, "again", &names4[0]).is_err(),
            "同书签名二次定义拒（V6）"
        );
    }

    /// T8d：写读全环——读面语义镜像（TOC 域内容型保留 + PAGEREF
    /// volatile 域结果剥离 + 标题文本在场）。
    #[test]
    fn t8d_write_read_roundtrip_semantics() {
        let mut w = DocxWriter::new();
        let names = w.toc_entries(
            &[TocEntry::new(1, "章甲"), TocEntry::new(2, "节乙")],
            Some((1, 2)),
        );
        w.heading_bookmarked(1, "章甲", &names[0]).unwrap();
        w.heading_bookmarked(2, "节乙", &names[1]).unwrap();
        w.paragraph_text("正文。");
        let bytes = w.save_to_vec().unwrap();
        let doc = read_docx_from_bytes(&bytes).unwrap();
        // 目录项文本保留（TOC = 内容型域——域结果承载作者可见内容；
        // 段落文本面 = Block 文本聚合）。
        let all_text: String = doc
            .blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(p) => p.text(),
                Block::Table(_) | Block::TextBox(_) => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_text.contains("章甲"), "目录项文本 1 保留");
        assert!(all_text.contains("节乙"), "目录项文本 2 保留");
        // PAGEREF 页码占位剥离（volatile——markdown 无分页语义）。
        assert!(
            !all_text.contains("章甲1") && !all_text.contains("节乙1"),
            "页码占位不拼接（volatile 剥离）"
        );
        // 正文标题文本在场（写读主体面不受影响）。
        assert!(all_text.contains("正文。"), "正文在场");
    }

    /// T8e：粘性校验——V11 空书签 / V8 entry 层级 / V8 heading_range
    /// （save 漏斗拒绝零产物）。
    #[test]
    fn t8e_sticky_validation_v11_v8() {
        // V11：field_pageref 空书签名。
        let mut w = DocxWriter::new();
        w.paragraph(&[RunSpec::field_pageref("")]);
        let err = w.save_to_vec().unwrap_err();
        assert!(
            matches!(err, DocxError::InvalidInput(ref m) if m.starts_with("V11")),
            "V11 粘性拒：{err:?}"
        );
        // V8'：entry 层级越界。
        let mut w2 = DocxWriter::new();
        w2.toc_entries(&[TocEntry::new(0, "x")], None);
        let err2 = w2.save_to_vec().unwrap_err();
        assert!(
            matches!(err2, DocxError::InvalidInput(ref m) if m.contains("V8")),
            "entry 层级粘性拒：{err2:?}"
        );
        // V8：heading_range 倒序（toc_entries 直传路径）。
        let mut w3 = DocxWriter::new();
        w3.toc_entries(&[TocEntry::new(1, "x")], Some((3, 1)));
        let err3 = w3.save_to_vec().unwrap_err();
        assert!(
            matches!(err3, DocxError::InvalidInput(ref m) if m.starts_with("V8")),
            "heading_range 粘性拒：{err3:?}"
        );
    }

    /// T8f：书签生成避让——用户已注册 toc_0/toc_1 时生成跳号（先到
    /// 先得，零重名）。
    #[test]
    fn t8f_bookmark_generation_avoids_user_collisions() {
        let mut w = DocxWriter::new();
        w.bookmark_paragraph(&[RunSpec::text("用户书签")], "toc_0")
            .unwrap();
        w.bookmark_paragraph(&[RunSpec::text("用户书签 2")], "toc_1")
            .unwrap();
        let names = w.toc_entries(&[TocEntry::new(1, "甲"), TocEntry::new(1, "乙")], None);
        assert_eq!(
            names,
            vec!["toc_2".to_string(), "toc_3".to_string()],
            "撞名跳号"
        );
        // 生成后注册定义侧（闭环）且不再撞用户名。
        w.heading_bookmarked(1, "甲", &names[0]).unwrap();
        w.heading_bookmarked(1, "乙", &names[1]).unwrap();
        assert!(w.save_to_vec().is_ok(), "全环合法");
    }

    /// T8g：空 entries 退化——与旧 RunSpec TOC 路径同构（占位提示
    /// 文本在场，无目录项/书签/PAGEREF 面）。
    #[test]
    fn t8g_empty_entries_degrades_to_legacy_shape() {
        let mut w = DocxWriter::new();
        let names = w.toc_entries(&[], Some((1, 3)));
        assert!(names.is_empty(), "空 entries 零书签");
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(
            doc.contains("（目录域——打开后全选再按 F9 更新生成）"),
            "占位提示在场（旧形态）"
        );
        assert!(!doc.contains("PAGEREF"), "零 PAGEREF 面");
        assert!(!doc.contains("bookmarkStart"), "零书签面");
        // 与 RunSpec 路径字节同构（同参数同产物——既有消费者零漂移）。
        let mut w2 = DocxWriter::new();
        w2.paragraph(&[RunSpec::field_toc_heading_range(1, 3)]);
        let bytes2 = w2.save_to_vec().unwrap();
        let doc2 =
            String::from_utf8(zip_part_bytes(&bytes2, "word/document.xml").unwrap()).unwrap();
        // 域段部分逐字节同构（w14:paraId 是引擎自增序——两文档段序
        // 相同则 paraId 相同；比对去 paraId 后的域段内容）。
        let strip = |s: &str| s.replace("w14:paraId=\"00000001\"", "P");
        assert_eq!(
            strip(&doc).replace("w14:paraId=\"00000002\"", "P"),
            strip(&doc2).replace("w14:paraId=\"00000002\"", "P"),
            "空 entries 与 RunSpec TOC 路径字节同构"
        );
    }
    // ── #196 任务 3：pStyle 视觉面 + rStyle 测试组（T9）───────────

    /// T9a：Heading1-9 视觉面——字号梯度（16/13/12/11pt×6）+ 加粗
    /// （H1-H4）；pStyle 引用与 outlineLvl 双通道不受影响（document.xml
    /// 面零变化——纯 styles.xml 增强）。
    #[test]
    fn t9a_heading_visual_face() {
        let mut w = DocxWriter::new();
        w.heading(1, "甲").unwrap();
        w.heading(5, "乙").unwrap();
        let bytes = w.save_to_vec().unwrap();
        let styles = String::from_utf8(zip_part_bytes(&bytes, "word/styles.xml").unwrap()).unwrap();
        // 字号梯度：H1=32 半点（16pt）/ H2=26 / H3=24 / H4=22 /
        // H5-H9=22。
        for (id, sz) in [
            ("Heading1", "32"),
            ("Heading2", "26"),
            ("Heading3", "24"),
            ("Heading4", "22"),
            ("Heading9", "22"),
        ] {
            assert!(
                styles.contains(&format!(r#"<w:style w:type="paragraph" w:styleId="{id}">"#)),
                "{id} 定义在场"
            );
            // sz 断言：Heading 样式段内 <w:sz w:val="N" />（半点）。
            let seg_start = styles
                .find(&format!(r#"w:styleId="{id}""#))
                .expect("{id} 段");
            let seg_end = styles[seg_start + 10..]
                .find("<w:style ")
                .map(|k| seg_start + 10 + k)
                .unwrap_or(styles.len());
            let seg = &styles[seg_start..seg_end];
            assert!(
                seg.contains(&format!(r#"<w:sz w:val="{sz}" />"#)),
                "{id} 字号 {sz} 半点在场"
            );
        }
        // 加粗面：H1-H4 有 <w:b />，H5-H9 无。
        for (id, expect_b) in [
            ("Heading1", true),
            ("Heading4", true),
            ("Heading5", false),
            ("Heading9", false),
        ] {
            let seg_start = styles.find(&format!(r#"w:styleId="{id}""#)).unwrap();
            let seg_end = styles[seg_start + 10..]
                .find("<w:style ")
                .map(|k| seg_start + 10 + k)
                .unwrap_or(styles.len());
            let seg = &styles[seg_start..seg_end];
            assert_eq!(
                seg.contains("<w:b />"),
                expect_b,
                "{id} 加粗位 = {expect_b}"
            );
        }
        // document.xml 面零变化：pStyle + outlineLvl 双通道照旧。
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(
            doc.contains(r#"<w:pStyle w:val="Heading1" />"#)
                && doc.contains(r#"<w:outlineLvl w:val="0" />"#),
            "pStyle/outline 双通道保持"
        );
    }

    /// T9b：rStyle 发射——char_styled run 的 `w:rStyle`（与直接格式位
    /// 并存：Word 直接格式压样式）。
    #[test]
    fn t9b_char_style_emission() {
        let mut w = DocxWriter::new();
        w.paragraph(&[
            RunSpec::char_styled("重点", "Strong"),
            RunSpec::char_styled("斜引", "Emphasis"),
            RunSpec::text("普通"),
        ]);
        let bytes = w.save_to_vec().unwrap();
        let doc = String::from_utf8(zip_part_bytes(&bytes, "word/document.xml").unwrap()).unwrap();
        assert!(
            doc.contains(r#"<w:rStyle w:val="Strong" />"#),
            "Strong rStyle 发射"
        );
        assert!(
            doc.contains(r#"<w:rStyle w:val="Emphasis" />"#),
            "Emphasis rStyle 发射"
        );
        // 无 char_style 的 run 零 rStyle 面（普通 run 不受影响）。
        assert_eq!(
            doc.matches("rStyle").count(),
            2,
            "恰两个 rStyle（普通 run 零面）"
        );
        // 与直接格式并存（bold + rStyle 同 run 合法）。
        let mut w2 = DocxWriter::new();
        w2.paragraph(&[RunSpec {
            bold: true,
            char_style: Some("Strong".into()),
            ..RunSpec::text("叠加")
        }]);
        let bytes2 = w2.save_to_vec().unwrap();
        let doc2 =
            String::from_utf8(zip_part_bytes(&bytes2, "word/document.xml").unwrap()).unwrap();
        assert!(
            doc2.contains(r#"<w:rStyle w:val="Strong" />"#) && doc2.contains("<w:b />"),
            "rStyle 与直接格式共存"
        );
    }

    /// T9c：V12 矩阵——空串拒 / 悬空名拒 / 预置 Strong·Emphasis 过 /
    /// define_style 的 Character 型过（写自洽先定义后引用）。
    #[test]
    fn t9c_v12_reference_matrix() {
        // 空串。
        let mut w = DocxWriter::new();
        w.paragraph(&[RunSpec::char_styled("x", "")]);
        let err = w.save_to_vec().unwrap_err();
        assert!(
            matches!(err, DocxError::InvalidInput(ref m) if m.starts_with("V12")),
            "空串拒：{err:?}"
        );
        // 悬空名（未定义未预置）。
        let mut w2 = DocxWriter::new();
        w2.paragraph(&[RunSpec::char_styled("x", "NoSuchStyle")]);
        let err2 = w2.save_to_vec().unwrap_err();
        assert!(
            matches!(err2, DocxError::InvalidInput(ref m) if m.starts_with("V12")),
            "悬空拒：{err2:?}"
        );
        // 预置字符样式两枚过。
        let mut w3 = DocxWriter::new();
        w3.paragraph(&[RunSpec::char_styled("x", "Strong")]);
        w3.paragraph(&[RunSpec::char_styled("y", "Emphasis")]);
        assert!(w3.save_to_vec().is_ok(), "预置 Strong/Emphasis 过");
        // define_style 的 Character 型先定义后引用过。
        let mut w4 = DocxWriter::new();
        w4.define_style(crate::style::StyleDef {
            run: crate::style::StyleRunProps {
                bold: Some(true),
                ..Default::default()
            },
            ..crate::style::StyleDef::new("MyChar", crate::style::StyleType::Character)
        })
        .unwrap();
        w4.paragraph(&[RunSpec::char_styled("z", "MyChar")]);
        assert!(w4.save_to_vec().is_ok(), "自定义 Character 型过");
        // define_style 重名拒（Strong/Emphasis 已预置）。
        let mut w5 = DocxWriter::new();
        let dup = w5.define_style(crate::style::StyleDef::new(
            "Strong",
            crate::style::StyleType::Character,
        ));
        assert!(dup.is_err(), "define Strong 重名拒");
    }

    /// T9d：预置注入——styles.xml 恒含 Strong/Emphasis 字符样式定义
    /// （Word 内置名，qFormat 面板可见）；写读全环读回 bold/italic。
    #[test]
    fn t9d_preset_char_styles_injected_and_roundtrip() {
        let mut w = DocxWriter::new();
        w.paragraph_text("占位");
        let bytes = w.save_to_vec().unwrap();
        let styles = String::from_utf8(zip_part_bytes(&bytes, "word/styles.xml").unwrap()).unwrap();
        assert!(
            styles.contains(r#"<w:style w:type="character" w:styleId="Strong">"#),
            "Strong 字符样式定义在场"
        );
        assert!(
            styles.contains(r#"<w:style w:type="character" w:styleId="Emphasis">"#),
            "Emphasis 字符样式定义在场"
        );
        // 写读全环：读面逐字段还原（S1 对偶纪律）。
        let b = crate::style::read_docx_styles_from_bytes(&bytes).unwrap();
        let strong = b.tree.get("Strong").expect("读回 Strong");
        assert_eq!(strong.run.bold, Some(true), "Strong.bold 读回");
        let emph = b.tree.get("Emphasis").expect("读回 Emphasis");
        assert_eq!(emph.run.italic, Some(true), "Emphasis.italic 读回");
        // Heading 视觉面读回（H1 = 32 半点 + bold）。
        let h1 = b.tree.get("Heading1").expect("读回 Heading1");
        assert_eq!(h1.run.size, Some(32), "Heading1.size 读回");
        assert_eq!(h1.run.bold, Some(true), "Heading1.bold 读回");
    }
}
