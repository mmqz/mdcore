//! t16：DOCX → Markdown 导入切片（office-io 首个消费边，经 wasm 桥）。
//!
//! **定位（2026-10-17，t16 立项）**：把 [`mdc_office_io::docx`] 的硬化
//! 读面（容器三门 + 自有 OPC 关系层 + docx-rs 模型解析）经本 crate 暴露
//! 给 web 编辑器——「在编辑器里打开 .docx」的最小真实切片。三域纪律
//! 的唯一新边 = mdcore-wasm → office-io（消费方边界；office-io 对
//! mdcore 零依赖不变）。
//!
//! ## 暴露面（[`import_docx`]，wasm-bindgen 自由函数——不碰 WasmCore
//! 状态，既有类面零变化）
//!
//! - 入参：DOCX 字节（JS `Uint8Array` → `&[u8]`）。
//! - 成功：JSON 字符串 `{"markdown": "...", "stats": {...}}`——
//!   markdown 直接喂 [`crate::WasmCore::new`] / 宿主注入契约
//!   （web 侧 `window.__inject`）；stats = 紧凑摘要（块计数 + 兼容
//!   模式容器警告 + 不导入面计数）。
//! - 失败：抛 JSON 字符串 `{"code": "...", "message": "..."}`——
//!   typed 错误码（下表），宿主可 `JSON.parse` 出结构化错误。
//!
//! ## 错误码表（office-io 错误面 → 码——容器门拒与引擎解析错误分离）
//!
//! | office-io 错误 | code | 含义 |
//! |---|---|---|
//! | [`DocxError::Container`]([`mdc_office_io::io::ReadError::PrescanRejected`]) | `container_prescan` | 门 1 拒（条目数/单尺寸/总量/路径逃逸——敌意输入零解析层接触） |
//! | `Container(ContainerCorrupt)` | `container_corrupt` | zip 容器损坏（EOCD/中央目录不可读——垃圾字节/截断档案） |
//! | `Container(ParseError)` | `container_parse` | 过门后解析层错误（zip 读界） |
//! | `Container(InvalidPart)` | `container_invalid_part` | 门 3 部件良构性拒（畸形 XML——兼容模式下仅特定面仍可到这） |
//! | `Container(LegacyOfficeFormat)` | `docx_legacy_format` | 旧版 .doc（OLE2 复合文档）改名 .docx——专用码 + 转换指引（#234，不再误报容器损坏） |
//! | [`DocxError::Parse`] | `docx_parse` | docx-rs 模型解析错误（引擎面） |
//! | [`DocxError::Io`] | `io` | 文件 IO（字节入口实际不可达，完备性映射） |
//!
//! 读模式 = **兼容**（[`mdc_office_io::io::ReadOptions::compat`]）：
//! 可恢复的畸形部件降级为结构化警告随 stats 返回（可见性），敌意
//! 输入防线（门 1/门 2 与全部安全上限）两模式恒等——垃圾/截断档案
//! 仍 typed 拒绝。
//!
//! ## 映射规则（DocxDocument → markdown；模型级，非保真渲染）
//!
//! - 段落标题层级 `heading_level`（#13：office-io 读面四链解析单一
//!   真源——段落 outlineLvl 直接格式 > pStyle `Heading1`..`Heading9`
//!   前缀 > 样式名 heading/紧凑/中文形[真 Word 中文版 styleId="1"-"9"
//!   经此链命中] > 样式 outlineLvl）→ `#`×level 标题（**7-9 钳到 6**，
//!   MD 上限 h6）；None（Title/Subtitle/自定义等非标题语义）→ 普通
//!   段落；**标题段带 numPr → 标题赢**（Word 编号标题仍显示为标题；
//!   mdcore 标题非列表项），编号丢弃（`list_numbering_dropped` 计数，
//!   r5-t1）；
//! - **列表段（r5-t1）**：`numbering.kind` 解析成功 → markdown 列表项
//!   ——Bullet → `- `；Ordered → `{n}. `（n = 连续列表 run 内计数器，
//!   起始 1 **假定**〔读模型不承载 `w:start`〕）；嵌套缩进 =
//!   2×min(ilvl, 8) 空格（mdcore `list_item_of` depth = indent/2+1；
//!   钳 8 对称写面九级抽象域，`list_depth_clamped` 计数）；计数器 key
//!   = (num_id, ilvl)——不同 numId = 独立实例（Word 语义），同 numId
//!   更浅级推进 → 深级清零（Word 多级：父项推进子级重起）；**任何非
//!   列表映射块（普通段/标题/表/降级段）中断 → 计数器清零重启**
//!   （markdown 无持久列表身份；Word 对同 numId 非连续段续号 → 登记
//!   降级）；原生 numFmt ∉ {decimal}（lowerLetter/upperRoman/…）→ 序号
//!   标准化为 decimal（`list_markers_normalized` 计数——降级可诊断）；
//!   numFmt `"none"`（Word 该级不显示 marker）→ **降级普通段落**，
//!   不发明数字（`list_items_degraded` 计数）；kind=None（悬空 numId /
//!   numStyleLink / ilvl 超定义域——兼容模式读面已给 BrokenReference
//!   警告随 stats）→ 同样降级普通段落；空列表项 → `- `/`{n}. ` marker
//!   加空内容行（mdcore 解析为空 ListItem——结构保真）；numId=0 在读面
//!   已归 None（OOXML 去编号惯例，office-io 契约）→ 普通段落；
//! - run：加粗 → `**`、斜体 → `*`、超链接 → `[text](target)`（外链
//!   URL 含空白/`)`/空 → `<...>` 角括号形；锚点 → `#anchor`；包内
//!   目标原样透传；r:id 无对应条目 [`HyperlinkTarget::Unresolved`] →
//!   降级纯文本 + BrokenReference 警告随 stats）；
//!   **相邻同格式 run 合并**（MD 不承载 run 边界；`*a*`+`*b*` 直连
//!   会产出 CommonMark 歧义串 `*a**b*`）；
//! - 表 → GFM 管道表（首行表头 + `| --- |` 分隔行 + 体行——与
//!   mdcore `BlockKind::Table` 规范文本同构，html.rs
//!   `canonical_table_text` 同形）；单元格清洗：`|` → `\|` 转义、
//!   `\n`/`\r` → 空格（GFM 单元格不承载换行）、短行补空格对齐表头宽；
//!   **合并单元格**（#22 P2-2）：office-io 读面栅格展开形（gridSpan
//!   覆盖列/vMerge 续格置空，内容只居原点格——Word 显示语义）与
//!   `merges` 矩形清单；GFM 无合并面 → 降级为栅格对齐表（覆盖格空），
//!   计数进 `merged_ranges`（结构丢失显式化——与 xlsx 导入同键）；
//!   短行补齐在引擎栅格面已天然对齐（render 补齐 = 防御位）；
//! - 空段落跳过（mdcore 空行不建块）；段落内软换行（w:br → `\n`）
//!   原样输出——mdcore 行级解析按行建块（一段拆多块，登记）。
//!
//! ## 页眉/页脚接入（#22 P2-1——原「不导入面」项移交导入链）
//!
//! - 页眉内容 = **文档前导**（正文前发射）、页脚 = **文档尾接**：
//!   每部件 = 来源标记行（HTML 注释 `<!-- mdcore-import:
//!   header:default -->` 形——GFM 渲染不可见、mdcore 行级解析承载
//!   为普通段、`__md()` 投影原样往返）+ 内容行（与正文同一映射机制
//!   ——标题/列表/表/富文本全同构）；空部件（挂接但无可见内容）无
//!   输出；stats：`headers_imported`/`footers_imported`（有可见内容
//!   的部件数）+ `headers_skipped`/`footers_skipped` 恒 0（P2-1 前
//!   语义 = 未消费槽位数——键保留，消费方兼容）；
//! - 位置语义依据（页眉前导/页脚尾接的裁决理由）见
//!   [`docx_to_markdown`] 文档（原位不可回放 + markdown 线性单流 +
//!   语义保真三链）；
//! - 块源结构面：office-io `Block` 携带 `source: Body | Header |
//!   Footer`（读面按部件定值）——消费方可不经标记行直接从模型区分
//!   块源。
//!
//! ## 不导入面（模型级数据在 MD 中不可表示——计数进 stats，诚实边界）
//!
//! - 图片（`images_skipped` = 正文/页眉页脚 run 级引用数 >0 的资产；
//!   孤儿媒体不计——非用户可见内容）；
//! - 脚注（引擎读面受限，office-io 侧登记面原样继承）。
//!
//! ## 已知边界（登记，非缺陷）
//!
//! - 段落文本以 MD 结构字符开头（`#`/`|`/`>`/`- `/数字`.` 等）会被
//!   mdcore 行级解析解释为对应块型——导入是结构映射而非显示保真；
//! - 列表降级面（r5-t1 登记）：有序起始值假定 1（`w:start` 读模型不
//!   承载）；同 numId 非连续段重起（Word 续号）；自定义 numFmt 序号
//!   标准化 decimal（原生形态经 stats 计数可诊断）；多行列表项
//!   （run 内 `w:br` → `\n`）首行带 marker、续行脱离列表项——mdcore
//!   行级解析拆独立段落块（既有软换行族降级，同普通段）；
//! - 表格单元格内超链接目标（office-io TableData 扁平文本面）随
//!   单元格文本合并，目标丢弃；
//! - 内联 `*`/`_`/`[` 等字符不转义（mdcore 无内联转义层，转义会
//!   把反斜杠写进文本）。
//!
//! 纯内核（[`import_docx_core`] 等）不触碰 JsValue → 原生 cargo test
//! 直测（JsValue 非 wasm 目标构造即 panic，见 lib.rs `parse_source_str`
//! 先例）；wasm 壳由 wasm32 check + web 三件套门禁背书。

use mdc_office_io::docx::{
    read_docx_from_bytes_with_options, Block, DocxDocument, DocxError, HeaderFooterData,
    HeaderFooterVariant, HyperlinkTarget, ListKind, ParagraphNumbering, RunData, TableData,
};
use mdc_office_io::io::{ReadError, ReadOptions};
use wasm_bindgen::prelude::*;

/// typed 导入错误（错误码 + 人类可读消息——见模块文档错误码表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError {
    pub code: &'static str,
    pub message: String,
}

/// office-io 错误面 → 错误码映射（容器门拒与引擎解析错误分离；
/// r4-t2：写面资源限 `docx_limit` 与格式/引擎类可区分）。
fn map_error(e: &DocxError) -> ImportError {
    match e {
        DocxError::Container(read) => ImportError {
            code: match read {
                ReadError::PrescanRejected { .. } => "container_prescan",
                ReadError::ContainerCorrupt(_) => "container_corrupt",
                ReadError::ParseError(_) => "container_parse",
                ReadError::InvalidPart { .. } => "container_invalid_part",
                // #234：旧版 .doc（OLE2）改名 .docx——专用码（含可操作
                // 指引：另存为 .docx / soffice 转换），不再误报容器损坏。
                ReadError::LegacyOfficeFormat { .. } => "docx_legacy_format",
            },
            message: read.to_string(),
        },
        DocxError::Parse(m) => ImportError {
            code: "docx_parse",
            message: format!("docx-rs model parse error: {m}"),
        },
        DocxError::Io(m) => ImportError {
            code: "io",
            message: format!("io error: {m}"),
        },
        DocxError::Limit(m) => ImportError {
            code: "docx_limit",
            message: format!("docx writer resource limit: {m}"),
        },
        // office-g Phase 1/2：读面不可达（InvalidInput 仅写面校验产生
        // ——V1-V7；完备性映射防未来漂移）。
        DocxError::InvalidInput(m) => ImportError {
            code: "docx_invalid_input",
            message: format!("docx writer invalid input: {m}"),
        },
    }
}

/// 兼容模式读选项（可恢复畸形部件降级警告；敌意防线两模式恒等）。
fn compat_options() -> ReadOptions {
    ReadOptions { compat: true }
}

// ---------------------------------------------------------------------------
// 纯映射内核（原生可测——构造 DocxDocument 直测，零 zip 依赖）
// ---------------------------------------------------------------------------

// #13（A1）：标题层级识别已收归 office-io 读面单一真源
//（ParagraphData::heading_level——四链解析：段落 outlineLvl 直接格式 >
// pStyle `Heading1`..`Heading9` 前缀 > 样式名 heading/紧凑/中文形 >
// 样式 outlineLvl）。本层原 `heading_level(style)` 前缀函数删除（逻辑
// 完整迁移；真 Word 中文版 styleId="1"-"9" 由样式名链命中——导入
// headings: 0 修复）。

/// 相邻同格式 run 合并（bold/italic/超链接目标三元组相等——MD 不承载
/// run 边界，直连产出 CommonMark 歧义串）。
fn merge_runs(runs: &[RunData]) -> Vec<RunData> {
    let mut merged: Vec<RunData> = Vec::with_capacity(runs.len());
    for r in runs {
        if let Some(last) = merged.last_mut() {
            if last.bold == r.bold && last.italic == r.italic && last.hyperlink == r.hyperlink {
                last.text.push_str(&r.text);
                continue;
            }
        }
        merged.push(r.clone());
    }
    merged
}

/// 链接目标 → GFM destination（空/含空白/含 `)` → 角括号形；锚点加
/// `#` 前缀；Unresolved → None = 降级纯文本）。
fn link_destination(target: &HyperlinkTarget) -> Option<String> {
    match target {
        HyperlinkTarget::External { target } | HyperlinkTarget::Internal { target } => {
            Some(angle_wrap(target))
        }
        HyperlinkTarget::Anchor { anchor } => Some(format!("#{}", angle_wrap(anchor))),
        HyperlinkTarget::Unresolved { .. } => None,
    }
}

/// URL 形式规整（空/空白/`)` 需 `<...>` 包裹——GFM destination 语法）。
fn angle_wrap(target: &str) -> String {
    if target.is_empty()
        || target
            .chars()
            .any(|c| c.is_whitespace() || c == ')' || c == '<' || c == '>')
    {
        format!("<{target}>")
    } else {
        target.to_string()
    }
}

/// 单个（已合并）run → 内联 markdown。强调标记只裹住非空白核（`** x **`
/// 不构成 CommonMark 强调）；链接在外、强调在内（`[**b**](url)`）。
fn render_inline(run: &RunData) -> String {
    let text = &run.text;
    let core = text.trim();
    if core.is_empty() {
        // 纯空白 run：原样直出（标记裹空白 = 非法强调，无信息量丢弃）。
        return text.clone();
    }
    let lead_ws = &text[..text.len() - text.trim_start().len()];
    let trail_ws = &text[text.trim_end().len()..];
    let mut inner = core.to_string();
    if run.bold {
        inner = format!("**{inner}**");
    }
    if run.italic {
        inner = format!("*{inner}*");
    }
    let body = match run.hyperlink.as_ref().and_then(link_destination) {
        Some(dest) => format!("[{inner}]({dest})"),
        None => inner,
    };
    format!("{lead_ws}{body}{trail_ws}")
}

/// run 序 → 段落内联文本（返回文本 + 解析成功的链接计数）。
fn render_runs(runs: &[RunData]) -> (String, usize) {
    let mut out = String::new();
    let mut links = 0usize;
    for run in merge_runs(runs) {
        if run.hyperlink.as_ref().and_then(link_destination).is_some()
            && !run.text.trim().is_empty()
        {
            links += 1;
        }
        out.push_str(&render_inline(&run));
    }
    (out, links)
}

/// 单元格清洗：`|` 转义（GFM 列分隔符冲突）、`\n`/`\r` → 空格（单元格
/// 不承载换行）、首尾空白去（GFM 解析器惯例）。
fn sanitize_cell(cell: &str) -> String {
    let flat = if cell.contains('\n') || cell.contains('\r') {
        cell.chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect::<String>()
    } else {
        cell.to_string()
    };
    flat.trim().replace('|', "\\|")
}

/// 表 → GFM 管道表（表头 + `---` 分隔行 + 体行；与 mdcore Table 块
/// 规范文本同构）。空表 → None（跳过）；短行补空格到全表最大列宽。
fn render_table(table: &TableData) -> Option<String> {
    if table.rows.is_empty() {
        return None;
    }
    let width = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return None;
    }
    let render_row = |cells: &[String]| -> String {
        let padded = (0..width).map(|i| cells.get(i).map(|c| sanitize_cell(c)).unwrap_or_default());
        let mut line = String::from("|");
        for cell in padded {
            line.push(' ');
            line.push_str(&cell);
            line.push_str(" |");
        }
        line
    };
    let mut lines = vec![render_row(&table.rows[0])];
    let sep_cells: Vec<String> = std::iter::repeat_n(String::from("---"), width).collect();
    lines.push(render_row(&sep_cells));
    for row in &table.rows[1..] {
        lines.push(render_row(row));
    }
    Some(lines.join("\n"))
}

/// stats 的警告 kind → snake_case 字符串。
fn warning_kind(kind: &mdc_office_io::io::WarningKind) -> &'static str {
    match kind {
        mdc_office_io::io::WarningKind::MalformedXml => "malformed_xml",
        mdc_office_io::io::WarningKind::ControlChars => "control_chars",
        mdc_office_io::io::WarningKind::BrokenReference => "broken_reference",
        // #17：修订删除文本登记（docx 面专用；xlsx 面无此类别使用面
        // ——完备性映射）。
        mdc_office_io::io::WarningKind::RevisionDelete => "revision_delete",
        // #39：面外元素跳过（嵌套表深度超限——敌意防御降级）。
        mdc_office_io::io::WarningKind::SkippedElement => "skipped_element",
        // #233（zip-exotica）：压缩比超观察门限（警告口径不拒——
        // 纵深可见性；见 mdc-office-io LIMIT_COMPRESS_RATIO 文档）。
        mdc_office_io::io::WarningKind::HighCompressionRatio => "high_compression_ratio",
        // #276（B-1）：超链接目标未过 URL 策略（敌意 scheme 消毒
        // 降级——目标缺席但内容保留）。
        mdc_office_io::io::WarningKind::UnsafeLink => "unsafe_link",
    }
}

/// 引擎警告 kind → 严重度（#22 P2-4 三档：`info` 可接受 / `warn` 有损 /
/// `error` 需关注）。
///
/// 裁决链：`malformed_xml`/`broken_reference` = 部件降级或槽位跳过
/// （兼容模式读面——**有损**，内容可能缺席）→ `warn`；`control_chars`
/// = 控制字符清洗（可接受规范化）→ `info`；`revision_delete` =
/// 修订删除文本登记（当前文本语义，非损失——#17 delText 本就不入
/// 流）→ `info`。`error` 档预留给「内容整体不可见」面（见
/// xlsx 导入的 sheets/rows/cols_dropped）——docx 面当前无此量级面
///（丢弃均为段落/域/图片级）。
fn warning_severity(kind: &mdc_office_io::io::WarningKind) -> &'static str {
    match kind {
        mdc_office_io::io::WarningKind::MalformedXml
        | mdc_office_io::io::WarningKind::BrokenReference => "warn",
        mdc_office_io::io::WarningKind::ControlChars
        | mdc_office_io::io::WarningKind::RevisionDelete => "info",
        // #39：嵌套表深度超限跳过（敌意/退化嵌套防御——诚实降级
        // 登记；正常文档不触发）→ warn（内容缺席面）。
        mdc_office_io::io::WarningKind::SkippedElement => "warn",
        // #233：压缩比观察（合法高压缩比件也触发——s7b 149:1 实证；
        // 内容无缺席，纯可见性）→ info。
        mdc_office_io::io::WarningKind::HighCompressionRatio => "info",
        // #276（B-1）：敌意 scheme 超链接消毒降级——目标缺席但文本
        // 内容保留，且属安全防御面（用户须知悉）→ warn。
        mdc_office_io::io::WarningKind::UnsafeLink => "warn",
    }
}

/// 导入链有损项 → warnings 条目（#22 P2-4）：非 0 计数才进（0 = 无事
/// 不噪声）；`count` = 项数（stats 同键值的镜像，聚合面单一真源在
/// stats——本清单是**可见性通道**非第二计数器）；`part` = 格式面
///（`"docx"`——引擎警告的 part = 具体包内部件路径，本层聚合项的
/// part = 容器格式标识，语义差异在档）；severity 裁决同
/// [`warning_severity`] 模块文档：丢弃/降级/越限 = `warn`，规范化/
/// 钦位/结构登记 = `info`。
fn import_lossy_warnings(
    fields_dropped: usize,
    images_skipped: usize,
    total: &BlockMapper,
) -> Vec<serde_json::Value> {
    [
        ("fields_dropped", fields_dropped, "warn"),
        ("images_skipped", images_skipped, "warn"),
        ("list_items_degraded", total.list_items_degraded, "warn"),
        ("list_numbering_dropped", total.list_numbering_dropped, "warn"),
        ("list_markers_normalized", total.list_markers_normalized, "info"),
        ("list_depth_clamped", total.list_depth_clamped, "info"),
    ]
    .into_iter()
    .filter(|&(_, count, _)| count > 0)
    .map(|(kind, count, severity)| {
        serde_json::json!({ "kind": kind, "part": "docx", "count": count, "severity": severity })
    })
    .collect()
}

// ---------------------------------------------------------------------------
// 列表映射（r5-t1：numbering 读面承载 → markdown 列表行）
// ---------------------------------------------------------------------------

/// 导入侧嵌套钳位（= 写面九级抽象域：docx_export `list_shape`
/// depth→ilvl 钳 0-8 与 DocxWriter `list_item` 0-8 typed 域对称）。
/// 读面不钳（office-io 契约——敌意巨值走解析不可达路径），钳位在本层
/// 发生并计数（stats `list_depth_clamped`——降级可诊断）。
const IMPORT_MAX_ILVL: usize = 8;

/// 连续列表 run 内的有序计数器（`(num_id, ilvl)` → 已发项数）：
/// - 任何非列表映射块（普通段/标题/表/降级段）→ [`Self::reset`]
///   （**中断重启**：markdown 无持久列表身份；Word 对同 numId 非连续
///   段续号 → 模块文档登记降级）；
/// - 同 numId 更浅 ilvl 推进 → 清该 numId 更深计数（Word 多级语义：
///   父项推进子级重起）；不同 numId 互不影响（不同实例独立计数）。
#[derive(Default)]
struct ListCounters {
    ordered: std::collections::HashMap<(usize, usize), usize>,
}

impl ListCounters {
    fn reset(&mut self) {
        self.ordered.clear();
    }

    /// 有序项取号（清同 numId 更深级后自增）。
    fn next_ordered(&mut self, num_id: usize, ilvl: usize) -> usize {
        self.ordered
            .retain(|&(n, l), &mut _| n != num_id || l <= ilvl);
        let c = self.ordered.entry((num_id, ilvl)).or_insert(0);
        *c += 1;
        *c
    }
}

/// 单个列表段的映射结果（[`list_marker_line`] 出口）。
enum ListMapping {
    /// 列表行 + 降级旗标（ilvl 钳位 / 有序 marker 标准化——stats 计数面）。
    Line {
        line: String,
        depth_clamped: bool,
        marker_normalized: bool,
    },
    /// 降级普通段落：`kind=None`（悬空 numId / numStyleLink / ilvl 超
    /// 定义域——兼容模式读面已给 BrokenReference 警告随 stats）或
    /// numFmt `"none"`（Word 该级不显示 marker——不发明数字）。
    Degraded,
}

/// 列表段 → markdown 列表行（纯函数；计数器状态经 `&mut` 注入）。
/// 嵌套缩进 = 2×ilvl 空格（mdcore `list_item_of` depth = indent/2+1
/// ——ilvl 0 = 顶级）；有序 marker `{n}.`（起始 1 假定：读模型不承载
/// `w:start`）；无序恒 `- `；空文本项 → marker + 尾空格行（mdcore
/// 解析为空 ListItem——结构保真）。
fn list_marker_line(
    numbering: &ParagraphNumbering,
    resolved_number: Option<&str>,
    text: &str,
    counters: &mut ListCounters,
) -> ListMapping {
    let Some(kind) = numbering.kind else {
        return ListMapping::Degraded;
    };
    if numbering.format.as_deref() == Some("none") {
        return ListMapping::Degraded;
    }
    let depth_clamped = numbering.ilvl > IMPORT_MAX_ILVL;
    let ilvl = numbering.ilvl.min(IMPORT_MAX_ILVL);
    let (marker, marker_normalized) = match kind {
        ListKind::Bullet => ("-".to_string(), false),
        ListKind::Ordered => {
            // #266：引擎解析序号可用且为 decimal 形态（`N.`——mdcore
            // markdown 行模型的有序 marker 约束）时直接采用——修正
            // start≠1/lvlRestart 语义（本地简单计数的两处失真）。
            // 非 decimal 形态（一、/（一）/i.）无法入 marker 位——保持
            // decimal 标准化（`list_markers_normalized` 计数可诊断；
            // 原生序号经 mang-shell 渲染面消费，本面标记形态受限）。
            let normalized = numbering.format.as_deref().is_some_and(|f| f != "decimal");
            let local = counters.next_ordered(numbering.num_id, ilvl);
            let marker = match resolved_number.and_then(parse_decimal_marker) {
                Some(n) => format!("{n}."),
                None => format!("{local}."),
            };
            (marker, normalized)
        }
    };
    ListMapping::Line {
        line: format!("{}{} {}", " ".repeat(2 * ilvl), marker, text),
        depth_clamped,
        marker_normalized,
    }
}

/// #266：解析 decimal 形态序号（`"3."` → `Some(3)`；非 decimal 形态
/// 如「一、」/「i.」→ None）。mdcore markdown 行模型有序 marker 位
/// 只收 `{n}.`——引擎解析序号的 decimal 子集直通，其余走本地计数。
fn parse_decimal_marker(resolved: &str) -> Option<u64> {
    let digits = resolved.strip_suffix('.')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// 块映射器（#22 P2-1：正文/页眉/页脚同一映射机制——单一真源，
/// 正文映射逻辑自原 docx_to_markdown 主循环原样迁入，零行为漂移）。
///
/// 每个内容部件（正文块序 / 每个页眉槽位 / 每个页脚槽位）独立实例：
/// 列表计数器不跨部件续（页眉里的列表与正文列表 = 不同列表——Word
/// 语义同构：不同部件独立编号流）；stats 计数经 [`Self::absorb`] 在
/// 文档级聚合。
#[derive(Default)]
struct BlockMapper {
    headings: usize,
    paragraphs: usize,
    tables: usize,
    hyperlinks: usize,
    // r5-t1 列表映射面（stats additive——既有 key 零变化）。
    list_items: usize,
    list_items_degraded: usize,
    list_numbering_dropped: usize,
    list_depth_clamped: usize,
    list_markers_normalized: usize,
    // #22 P2-2：表格合并区域计数（GFM 无合并面的降级显式化）。
    merged_ranges: usize,
    // #18：文本框计数（承载可见）。
    text_boxes: usize,
    counters: ListCounters,
}

impl BlockMapper {
    /// 块序 → markdown 行序。#18：文本框（Block::TextBox）——md 无
    /// 文本框概念，内层块扁平化进主映射（段落/表与正文同构；嵌套
    /// 文本框多趟展开至定点）。返回行恒非空（空段跳过 / 空表跳过）。
    fn map_blocks(&mut self, blocks: &[Block]) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut flat_blocks: Vec<&Block> = blocks.iter().collect();
        loop {
            let mut next: Vec<&Block> = Vec::with_capacity(flat_blocks.len());
            let mut expanded = false;
            for b in flat_blocks.into_iter() {
                match b {
                    Block::TextBox(tb) => {
                        self.text_boxes += 1;
                        expanded = true;
                        next.extend(tb.blocks.iter());
                    }
                    _ => next.push(b),
                }
            }
            flat_blocks = next;
            if !expanded {
                break;
            }
        }
        for block in &flat_blocks {
            match block {
                Block::Paragraph(p) => {
                    let (text, links) = render_runs(&p.runs);
                    self.hyperlinks += links;
                    if let Some(level) = p.heading_level {
                        // 标题优先（r5-t1）：编号标题段（Word 多级编号标题）
                        // → 标题赢，编号丢弃（mdcore 标题非列表项；计数
                        // `list_numbering_dropped`）。标题中断列表 run。
                        // #13：层级来源 = 读面四链解析字段（样式表语义
                        // 单一真源），非本层 styleId 猜测。
                        if p.numbering.is_some() {
                            self.list_numbering_dropped += 1;
                        }
                        self.counters.reset();
                        // MD 上限 h6（mdcore heading_of 1-6）——7-9 钳 6，登记。
                        let level = level.min(6);
                        lines.push(format!("{} {}", "#".repeat(level), text));
                        self.headings += 1;
                        continue;
                    }
                    if let Some(numbering) = &p.numbering {
                        match list_marker_line(
                            numbering,
                            p.resolved_number.as_deref(),
                            &text,
                            &mut self.counters,
                        ) {
                            ListMapping::Line {
                                line,
                                depth_clamped,
                                marker_normalized,
                            } => {
                                if depth_clamped {
                                    self.list_depth_clamped += 1;
                                }
                                if marker_normalized {
                                    self.list_markers_normalized += 1;
                                }
                                lines.push(line);
                                self.list_items += 1;
                                continue;
                            }
                            // 降级普通段落（非崩溃非静默列表）：可诊断性 =
                            // 兼容模式读面 BrokenReference 警告随 stats.warnings
                            // + `list_items_degraded` 计数。降级段中断列表 run。
                            ListMapping::Degraded => {
                                self.list_items_degraded += 1;
                            }
                        }
                    }
                    // 普通段（含降级列表段/空段）——空段跳过不计数（mdcore 空行
                    // 不建块）；软换行段落原样多行——mdcore 行级解析拆多块，
                    // 模块文档登记。非列表块 → 列表 run 中断（计数器清零）。
                    self.counters.reset();
                    if !text.is_empty() {
                        lines.push(text);
                        self.paragraphs += 1;
                    }
                }
                Block::Table(t) => {
                    // 表块中断列表 run（markdown 表非列表成员）。
                    self.counters.reset();
                    // P2-2：合并区域计数（表未渲染[空表]时 merges 恒空
                    // ——栅格展开形下两者同源，无口径差）。
                    self.merged_ranges += t.merges.len();
                    if let Some(md) = render_table(t) {
                        lines.push(md);
                        self.tables += 1;
                    }
                }
                // #18：预处理定点已展开全部文本框——此臂类型穷尽性专用。
                Block::TextBox(_) => unreachable!("text boxes flattened in preprocessing"),
            }
        }
        lines
    }

    /// 聚合另一映射器的计数（部件级独立映射 → 文档级单一 stats 面）。
    fn absorb(&mut self, other: BlockMapper) {
        self.headings += other.headings;
        self.paragraphs += other.paragraphs;
        self.tables += other.tables;
        self.hyperlinks += other.hyperlinks;
        self.list_items += other.list_items;
        self.list_items_degraded += other.list_items_degraded;
        self.merged_ranges += other.merged_ranges;
        self.list_numbering_dropped += other.list_numbering_dropped;
        self.list_depth_clamped += other.list_depth_clamped;
        self.list_markers_normalized += other.list_markers_normalized;
        self.text_boxes += other.text_boxes;
    }
}

/// 页眉/页脚槽位变体 → 标记标签（default/first/even——读面槽位提取
/// 序的确定性命名）。
fn variant_label(v: &HeaderFooterVariant) -> &'static str {
    match v {
        HeaderFooterVariant::Default => "default",
        HeaderFooterVariant::First => "first",
        HeaderFooterVariant::Even => "even",
    }
}

/// 页眉/页脚部件 → markdown 行（#22 P2-1：内容接入导入输出）。
///
/// 输出形 = **来源标记行 + 内容行**：标记 = HTML 注释（`<!--
/// mdcore-import: header:default -->` 形——GFM 渲染面不可见；mdcore
/// 行级解析承载为普通段，`__md()` 投影原样往返——机器可 grep、
/// 人读不噪）。内容行与正文同一映射机制（[`BlockMapper`]）。空部件
/// （挂接但无可见内容——仅空段/仅剥离域）→ 空返回（无内容可标记；
/// `*_imported` 不计——挂接空槽非丢失）。
fn header_footer_section(
    part: &HeaderFooterData,
    kind: &str,
    mapper: &mut BlockMapper,
) -> Vec<String> {
    let content = mapper.map_blocks(&part.blocks);
    if content.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(content.len() + 1);
    out.push(format!(
        "<!-- mdcore-import: {kind}:{} -->",
        variant_label(&part.variant)
    ));
    out.extend(content);
    out
}

/// DocxDocument → (markdown, stats JSON)。纯函数（原生直测；宿主消费
/// 经 [`import_docx_core`] 字节入口）。
///
/// #22 P2-1：页眉内容 = **文档前导**（正文前发射）、页脚 = **文档
/// 尾接**（正文后发射）。位置语义依据（三链裁决）：
/// 1. **原位不可回放**：OOXML 页眉/页脚是节级页面装置
///    （`sectPr`/`headerReference` 挂接），读面收集为六槽平表
///    （docx-rs 文档级单节模型，节位置不承载）→「原位」无数据基础；
/// 2. **markdown 线性单流**：无页/节结构，页面装置无同构宿主；
/// 3. **语义保真**：页眉 = 每页重复的文档级信息（标书场景：项目
///    编号——评审项）→ 前导位保留「读者第一眼」语义；页脚 = 尾随
///    页信息（页码/落款）→ 尾接位。变体序 = 槽位提取序（default →
///    first → even，确定性——Word 节设置决定激活哪个槽，读面透传
///    不解释语义）。
fn docx_to_markdown(doc: &DocxDocument) -> (String, serde_json::Value) {
    let mut lines: Vec<String> = Vec::new();
    let mut total = BlockMapper::default();
    // P2-1：有可见内容的页眉/页脚部件数。
    let mut headers_imported = 0usize;
    let mut footers_imported = 0usize;
    // 页眉 → 文档前导。
    for part in &doc.headers {
        let mut m = BlockMapper::default();
        let section = header_footer_section(part, "header", &mut m);
        if !section.is_empty() {
            headers_imported += 1;
            lines.extend(section);
        }
        total.absorb(m);
    }
    // 正文。
    lines.extend(total.map_blocks(&doc.blocks));
    // 页脚 → 文档尾接。
    for part in &doc.footers {
        let mut m = BlockMapper::default();
        let section = header_footer_section(part, "footer", &mut m);
        if !section.is_empty() {
            footers_imported += 1;
            lines.extend(section);
        }
        total.absorb(m);
    }
    let images_skipped = doc.images.iter().filter(|a| a.body_uses > 0).count();
    // #14（A2）：表现型域缓存结果剥离计数（段落级聚合——正文 +
    // 页眉/页脚部件段[内容接入后同计]，降级显式化）。
    let fields_dropped: usize = doc
        .blocks
        .iter()
        .chain(doc.headers.iter().flat_map(|h| h.blocks.iter()))
        .chain(doc.footers.iter().flat_map(|f| f.blocks.iter()))
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.fields_dropped),
            _ => None,
        })
        .sum();
    // blocks = 内容行数（来源标记行不计——stats 语义 = mdcore 建块
    // 面；标记行虽经解析成为段落块，但为导入元数据而非文档内容，
    // 口径与 P2-1 前的 body 行数可比）。
    let blocks_stat = lines.len() - headers_imported - footers_imported;
    // #22 P2-4：可见警告面——引擎结构化警告（count=1 逐条 + severity
    // 三档）+ 导入链有损项聚合（非 0 才进——count = stats 同键镜像，
    // 单一真源在 stats 键）；既有消费方（web 状态行 / e2e tr17 kind
    // 断言）读 kind/part 零破坏，新增字段为加性面。
    let mut warnings: Vec<serde_json::Value> = doc
        .warnings
        .iter()
        .map(|w| {
            serde_json::json!({
                "part": w.part,
                "kind": warning_kind(&w.kind),
                "detail": w.detail,
                "count": 1,
                "severity": warning_severity(&w.kind),
            })
        })
        .collect();
    warnings.extend(import_lossy_warnings(
        fields_dropped,
        images_skipped,
        &total,
    ));
    let stats = serde_json::json!({
        "blocks": blocks_stat,
        "headings": total.headings,
        "paragraphs": total.paragraphs,
        "tables": total.tables,
        "hyperlinks": total.hyperlinks,
        "list_items": total.list_items,
        "list_items_degraded": total.list_items_degraded,
        "list_numbering_dropped": total.list_numbering_dropped,
        "list_depth_clamped": total.list_depth_clamped,
        "list_markers_normalized": total.list_markers_normalized,
        // P2-2：表格合并区域数（GFM 降级显式化——与 xlsx 导入同键；
        // 正文 + 页眉/页脚部件表同计）。
        "merged_ranges": total.merged_ranges,
        "images_skipped": images_skipped,
        "text_boxes": total.text_boxes,
        // #19：批注/脚注/尾注读面承载量（内容不入 md 投影——结构面
        // 经 DocxDocument 字段透出；计数 = 承载可见）。
        "comments": doc.comments.len(),
        "footnotes": doc.footnotes.len(),
        "endnotes": doc.endnotes.len(),
        "fields_dropped": fields_dropped,
        // P2-1 前语义 = 未消费槽位数；页眉/页脚全量接入后恒 0——键
        // 保留（stats 消费方兼容：web UI / e2e / 下游标书侧），可见
        // 性移交 headers_imported/footers_imported。
        "headers_skipped": 0,
        "footers_skipped": 0,
        // P2-1：接入计数（有可见内容的部件数；挂接但空内容部件不
        // 计——无输出非丢失）。
        "headers_imported": headers_imported,
        "footers_imported": footers_imported,
        // #22 P2-4：可见警告面（引擎警告 + 导入链有损项——组装在
        // 上方 `warnings` 变量，此处引用）。
        "warnings": warnings,
    });
    (lines.join("\n\n"), stats)
}

/// 导入内核（字节入口；纯函数——无 JsValue，原生 cargo test 直测）。
/// 成功返回与 wasm 面同形的 JSON 串 `{"markdown": ..., "stats": ...}`。
///
/// 可见性：pub（阶段 B 六维体检 runner 消费——集成测试面可见性加宽；
/// 纯函数语义零变化，import_xlsx_core 先例同构）。
pub fn import_docx_core(bytes: &[u8]) -> Result<String, ImportError> {
    let doc: DocxDocument =
        read_docx_from_bytes_with_options(bytes, &compat_options()).map_err(|e| map_error(&e))?;
    let (markdown, stats) = docx_to_markdown(&doc);
    Ok(serde_json::json!({ "markdown": markdown, "stats": stats }).to_string())
}

// ---------------------------------------------------------------------------
// wasm-bindgen 壳（JsValue 仅此层——lib.rs parse_source 先例同构）
// ---------------------------------------------------------------------------

/// 导入 .docx 字节 → `{"markdown": "...", "stats": {...}}`（JSON 串，
/// 形状见模块文档）；失败抛 JSON 串 `{"code": "...", "message": "..."}`
///（typed 错误码表见模块文档——宿主 `JSON.parse(e)` 即结构化错误）。
#[wasm_bindgen]
pub fn import_docx(bytes: &[u8]) -> Result<String, JsValue> {
    import_docx_core(bytes).map_err(|e| {
        JsValue::from_str(&serde_json::json!({ "code": e.code, "message": e.message }).to_string())
    })
}

// ---------------------------------------------------------------------------
// 样例文档（单一真源：examples/gen_docx_fixture.rs 生成 web e2e 夹具
// + 本模块测试消费——office-io 写面即夹具产线，零外部工具依赖）
// ---------------------------------------------------------------------------

/// 经典 1×1 透明 PNG（70 字节）——图片清单计数的 body_uses 面。
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// 样例 DOCX 字节（夹具产线单一真源）：标题 + 富文本段（加粗/斜体/
/// 外链）+ 两列表 + 列表节（有序×2 + 嵌套 ilvl1 + 无序×2——r5-t1
/// 列表映射 e2e 面）+ 图片 + 页眉 + 页脚。消费方 = 本模块测试 +
/// examples/gen_docx_fixture.rs（web e2e 夹具生成器）。
/// `#[doc(hidden)]`：非产品面（wasm 导出面无此项——无
/// #[wasm_bindgen]），仅为避免产线双份漂移而公开。
#[doc(hidden)]
pub fn build_sample_docx() -> Vec<u8> {
    use mdc_office_io::docx::{DocxWriter, ListKind, RunSpec};
    let mut w = DocxWriter::new();
    w.heading(1, "Imported Title").expect("heading");
    w.paragraph(&[
        RunSpec::text("plain "),
        RunSpec::styled("bold", true, false),
        RunSpec::text(" and "),
        RunSpec::styled("italic", false, true),
        RunSpec::text(" and "),
        RunSpec::link("docs", "https://example.com/docs"),
    ]);
    w.table(&[
        vec!["Name".to_string(), "Value".to_string()],
        vec!["alpha".to_string(), "1".to_string()],
    ]);
    // 列表节（r5-t1）：读面经自有抽象 100（全级 decimal）/101（bullet）
    // 读回 kind/format 双解析 → markdown 列表行（嵌套 = ilvl 1）。
    w.list_item(&[RunSpec::text("First step")], ListKind::Ordered, 0)
        .expect("list ordered");
    w.list_item(&[RunSpec::text("Second step")], ListKind::Ordered, 0)
        .expect("list ordered");
    w.list_item(&[RunSpec::text("Nested step")], ListKind::Ordered, 1)
        .expect("list nested");
    w.list_item(&[RunSpec::text("Bullet one")], ListKind::Bullet, 0)
        .expect("list bullet");
    w.list_item(&[RunSpec::text("Bullet two")], ListKind::Bullet, 0)
        .expect("list bullet");
    w.image(PNG_1X1, 1, 1).expect("image");
    w.header(&[RunSpec::text("Header Slot")]);
    w.footer(&[RunSpec::text("Footer Slot")]);
    w.save_to_vec().expect("save")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_office_io::docx::{
        BlockSource, HeaderFooterData, HeaderFooterVariant, MergeRange, ParagraphData,
    };

    fn para(style: Option<&str>, runs: Vec<RunData>) -> Block {
        Block::Paragraph(ParagraphData {
            style: style.map(str::to_string),
            // #13：读面已解析字段——直构测试镜像 pStyle 前缀链（引擎
            // 写面发射形；样式表链的直构等价在 office-io 侧测试）。
            heading_level: style.and_then(|s| {
                s.strip_prefix("Heading")
                    .and_then(|r| r.parse::<usize>().ok())
                    .filter(|&n| (1..=9).contains(&n))
            }),
            fields_dropped: 0,
            runs,
            // r4-t3 模型扩面构造点机械补齐（新增两字段默认 None——
            // 语义零变化：该 helper 的既有断言全部面向 style/runs）。
            // #19：footnote_refs 默认空（helper 断言不涉脚注位）。
            // P2-1：source 默认 Body（helper 构造正文形态块）。
            // #316：未列字段经 `..Default::default()` 承接（新增
            // resolved_number 等——加字段不再逐处同步）。
            numbering: None,
            indent: None,
            footnote_refs: Vec::new(),
            source: BlockSource::Body,
            ..Default::default()
        })
    }

    fn run(text: &str) -> RunData {
        RunData {
            text: text.to_string(),
            bold: false,
            italic: false,
            hyperlink: None,
            revision_inserted: false,
        }
    }

    fn style_run(text: &str, bold: bool, italic: bool) -> RunData {
        RunData {
            bold,
            italic,
            ..run(text)
        }
    }

    /// 带编号段构造（r5-t1 列表映射测试 helper——`para` 面 +numbering；
    /// kind=None = 读面悬空/不可达形态，format=None 同构）。
    fn numbered_para(
        num_id: usize,
        ilvl: usize,
        kind: Option<ListKind>,
        format: Option<&str>,
        runs: Vec<RunData>,
    ) -> Block {
        Block::Paragraph(ParagraphData {
            style: None,
            heading_level: None,
            fields_dropped: 0,
            runs,
            source: BlockSource::Body,
            numbering: Some(ParagraphNumbering {
                num_id,
                ilvl,
                kind,
                format: format.map(str::to_string),
            }),
            indent: None,
            footnote_refs: Vec::new(),
            // #316：未列字段（resolved_number 等）经缺省承接。
            ..Default::default()
        })
    }

    /// 列表映射矩阵样例（r5-t1 两个测试共用）：多级有序 + 中断重启 +
    /// 独立 numId + 无序混合。
    fn list_matrix_doc() -> DocxDocument {
        doc_of(vec![
            numbered_para(
                7,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Top one")],
            ),
            numbered_para(
                7,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Top two")],
            ),
            numbered_para(
                7,
                1,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Sub of two")],
            ),
            numbered_para(
                7,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Top three")],
            ),
            numbered_para(
                7,
                1,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Sub of three")],
            ),
            para(None, vec![run("Break")]),
            numbered_para(
                7,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Restarted")],
            ),
            numbered_para(
                8,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Other list")],
            ),
            numbered_para(
                9,
                0,
                Some(ListKind::Bullet),
                Some("bullet"),
                vec![run("Dot one")],
            ),
            numbered_para(
                9,
                0,
                Some(ListKind::Bullet),
                Some("bullet"),
                vec![run("Dot two")],
            ),
        ])
    }

    fn doc_of(blocks: Vec<Block>) -> DocxDocument {
        DocxDocument {
            blocks,
            headers: Vec::new(),
            footers: Vec::new(),
            images: Vec::new(),
            // #19：扩面默认空（helper 断言不涉批注/脚注/尾注）。
            comments: Vec::new(),
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            // #257：节面默认空（helper 断言不涉节）。
            sections: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn import_json(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_str(&import_docx_core(bytes).expect("import ok")).expect("payload json")
    }

    #[test]
    fn writer_roundtrip_import_maps_structure() {
        let payload = import_json(&build_sample_docx());
        let md = payload["markdown"].as_str().expect("markdown");
        // P2-1：页眉前导 + 页脚尾接（标记行 + 内容行；来源标记见
        // header_footer_section 文档——GFM 渲染不可见、mdcore 解析承载）。
        assert_eq!(
            md,
            "<!-- mdcore-import: header:default -->\n\
             \n\
             Header Slot\n\
             \n\
             # Imported Title\n\
             \n\
             plain **bold** and *italic* and [docs](https://example.com/docs)\n\
             \n\
             | Name | Value |\n\
             | --- | --- |\n\
             | alpha | 1 |\n\
             \n\
             1. First step\n\
             \n\
             2. Second step\n\
             \n\
             \x20 1. Nested step\n\
             \n\
             - Bullet one\n\
             \n\
             - Bullet two\n\
             \n\
             <!-- mdcore-import: footer:default -->\n\
             \n\
             Footer Slot"
        );
        let stats = &payload["stats"];
        // blocks = 内容行（标记行不计）：原 8 + 页眉/页脚各 1。
        assert_eq!(stats["blocks"], 10);
        assert_eq!(stats["headings"], 1);
        // 富文本段 1（正文）+ 页眉段 1 + 页脚段 1（内容接入后同计）。
        assert_eq!(stats["paragraphs"], 3);
        assert_eq!(stats["tables"], 1);
        assert_eq!(stats["hyperlinks"], 1);
        // r5-t1：列表节（有序×2 + 嵌套 ilvl1 + 无序×2）。
        assert_eq!(stats["list_items"], 5);
        assert_eq!(stats["list_items_degraded"], 0);
        assert_eq!(stats["list_numbering_dropped"], 0);
        assert_eq!(stats["list_depth_clamped"], 0);
        assert_eq!(stats["list_markers_normalized"], 0);
        assert_eq!(stats["images_skipped"], 1);
        // P2-1：全量接入——skipped 恒 0（兼容键）+ imported 计数。
        assert_eq!(stats["headers_skipped"], 0);
        assert_eq!(stats["footers_skipped"], 0);
        assert_eq!(stats["headers_imported"], 1);
        assert_eq!(stats["footers_imported"], 1);
        // #22 P2-4：有损项 images_skipped=1 → 恰一条 warn 可见警告
        //（引擎警告 0——写面产物无悬空引用；其余零损项不进）。
        assert_eq!(
            stats["warnings"],
            serde_json::json!([{ "kind": "images_skipped", "part": "docx", "count": 1, "severity": "warn" }])
        );
    }

    /// r5-t1：列表映射矩阵——多级有序计数（父项推进子级重起）+ 中断
    /// 重启 + 独立 numId + 无序混合，逐行 exact 断言 + stats 计数。
    #[test]
    fn list_numbering_maps_to_markdown_lines() {
        let (md, stats) = docx_to_markdown(&list_matrix_doc());
        let expected = [
            "1. Top one",
            "2. Top two",
            "  1. Sub of two",
            "3. Top three",
            "  1. Sub of three", // 父项推进后子级重起（Word 多级语义）
            "Break",
            "1. Restarted",  // 中断重启（同 numId 非连续段——登记降级）
            "1. Other list", // 不同 numId = 独立实例（Word 语义）
            "- Dot one",
            "- Dot two",
        ]
        .join("\n\n");
        assert_eq!(md, expected);
        assert_eq!(stats["list_items"], 9);
        assert_eq!(stats["paragraphs"], 1);
        assert_eq!(stats["list_items_degraded"], 0);
        assert_eq!(stats["list_numbering_dropped"], 0);
        assert_eq!(stats["list_depth_clamped"], 0);
        assert_eq!(stats["list_markers_normalized"], 0);
    }

    /// r5-t1：导入 markdown → mdcore 解析 → 列表树结构闭环（docx →
    /// markdown → BlockTree：深度/marker/父子关系——导入→编辑器消费链
    /// 的原生层证明；`\n\n` 连接与紧凑列表在 mdcore 行级解析下同构）。
    #[test]
    fn list_markdown_parses_back_in_mdcore() {
        let (md, _) = docx_to_markdown(&list_matrix_doc());
        let tree = mdcore::markdown::markdown_to_tree(&md);
        use mdcore::tree::BlockKind;
        let top: Vec<_> = tree.top_level().to_vec();
        // 顶级块序：7 个 ListItem + 1 个中断普通段（嵌套项挂在父项下）。
        let top_summary: Vec<(BlockKind, String)> = top
            .iter()
            .map(|&id| {
                let n = tree.node(id);
                (n.kind, n.text.to_string())
            })
            .collect();
        // 中断普通段在 Top three 之后落根层（文档序）——顶级序 =
        // 3 个有序项 + Break + 重启/独立/无序 4 项。
        let expected_top: Vec<(BlockKind, String)> = [
            (BlockKind::ListItem, "Top one"),
            (BlockKind::ListItem, "Top two"),
            (BlockKind::ListItem, "Top three"),
            (BlockKind::Paragraph, "Break"),
            (BlockKind::ListItem, "Restarted"),
            (BlockKind::ListItem, "Other list"),
            (BlockKind::ListItem, "Dot one"),
            (BlockKind::ListItem, "Dot two"),
        ]
        .into_iter()
        .map(|(k, t)| (k, t.to_string()))
        .collect();
        assert_eq!(top_summary, expected_top);
        assert_eq!(top.len(), 8);
        // 嵌套：Sub of two = Top two 的子项（depth 2、marker 重起 1.）。
        let top_two = top[1];
        let children: Vec<_> = tree.node(top_two).children.clone();
        assert_eq!(children.len(), 1);
        let sub = tree.node(children[0]);
        assert_eq!(sub.text.to_string(), "Sub of two");
        assert_eq!(sub.attrs.get("marker").map(String::as_str), Some("1."));
        assert_eq!(sub.attrs.get("depth").map(String::as_str), Some("2"));
        assert_eq!(sub.parent, Some(top_two));
        // 顶级项 attrs：marker/depth 逐项（Break = Paragraph 无 marker 面）。
        let expected_markers = [
            ("Top one", "1."),
            ("Top two", "2."),
            ("Top three", "3."),
            ("Break", ""),
            ("Restarted", "1."),
            ("Other list", "1."),
            ("Dot one", "-"),
            ("Dot two", "-"),
        ];
        for (id, (text, marker)) in top.iter().zip(expected_markers) {
            let n = tree.node(*id);
            assert_eq!(n.text.to_string(), text);
            if marker.is_empty() {
                continue; // 中断段落：非列表项无 marker/depth
            }
            assert_eq!(n.attrs.get("marker").map(String::as_str), Some(marker));
            assert_eq!(n.attrs.get("depth").map(String::as_str), Some("1"));
        }
    }

    /// r5-t1：降级面——kind=None（悬空 numId / numStyleLink / ilvl 超
    /// 定义域）与 numFmt "none" → 普通段落（非崩溃非静默列表）+ 降级
    /// 计数 + 读面警告随 stats；降级段中断有序计数（重起）。
    /// 严格模式悬空 numId 拒绝面 = office-io r4t3 zip 实测锚点
    /// （r4t3_numbering_dangling_custom_format_injected_ind）；导入面恒
    /// 兼容模式 → 警告 + 降级路径（本测试直构同形态模型）。
    #[test]
    fn list_degraded_cases_count_and_warn() {
        let mut doc = doc_of(vec![
            numbered_para(
                5,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Before")],
            ),
            numbered_para(777, 2, None, None, vec![run("Dangling")]),
            numbered_para(
                6,
                0,
                Some(ListKind::Ordered),
                Some("none"),
                vec![run("NoMarker")],
            ),
            numbered_para(
                5,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("After")],
            ),
        ]);
        doc.warnings.push(mdc_office_io::io::CompatibilityWarning {
            part: "word/document.xml".into(),
            kind: mdc_office_io::io::WarningKind::BrokenReference,
            detail: "numbering reference numId '777' has no numbering definition (dangling) \
                     (kind unresolved; num_id/ilvl carried)"
                .into(),
        });
        let (md, stats) = docx_to_markdown(&doc);
        let expected = ["1. Before", "Dangling", "NoMarker", "1. After"].join("\n\n");
        assert_eq!(md, expected, "降级段中断 → After 重起 1.");
        assert_eq!(stats["list_items"], 2);
        assert_eq!(stats["list_items_degraded"], 2);
        assert_eq!(stats["paragraphs"], 2);
        assert_eq!(stats["warnings"][0]["kind"], "broken_reference");
        // 空文本降级段：行跳过（同空段惯例）但降级计数可见。
        let (md2, stats2) =
            docx_to_markdown(&doc_of(vec![numbered_para(9, 0, None, None, vec![])]));
        assert_eq!(md2, "");
        assert_eq!(stats2["list_items"], 0);
        assert_eq!(stats2["list_items_degraded"], 1);
        assert_eq!(stats2["paragraphs"], 0);
    }

    /// r5-t1：自定义 numFmt（lowerLetter/upperRoman/…）→ 序号标准化
    /// decimal（kind 仍 Ordered），`list_markers_normalized` 计数可诊断。
    #[test]
    fn list_custom_numfmt_normalizes_to_decimal() {
        let doc = doc_of(vec![
            numbered_para(
                3,
                0,
                Some(ListKind::Ordered),
                Some("lowerLetter"),
                vec![run("Alpha")],
            ),
            numbered_para(
                3,
                0,
                Some(ListKind::Ordered),
                Some("upperRoman"),
                vec![run("Roman")],
            ),
            numbered_para(
                3,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Dec")],
            ),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(md, ["1. Alpha", "2. Roman", "3. Dec"].join("\n\n"));
        assert_eq!(stats["list_items"], 3);
        assert_eq!(
            stats["list_markers_normalized"], 2,
            "lowerLetter + upperRoman"
        );
    }

    /// r5-t1：嵌套深度钳位——ilvl>8 钳 8（写面对称九级域）+ 计数；
    /// mdcore 解析深度无上限（16 空格 → depth 9），导入→导出往返稳定
    /// 在 8 级（docx_export list_shape 钳 0-8）。
    #[test]
    fn list_depth_clamp_at_ilvl8() {
        let doc = doc_of(vec![
            numbered_para(
                4,
                8,
                Some(ListKind::Bullet),
                Some("bullet"),
                vec![run("At eight")],
            ),
            numbered_para(
                4,
                9,
                Some(ListKind::Bullet),
                Some("bullet"),
                vec![run("Nine")],
            ),
            numbered_para(
                4,
                99,
                Some(ListKind::Bullet),
                Some("bullet"),
                vec![run("Deep")],
            ),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        let indent = " ".repeat(16);
        assert_eq!(
            md,
            [
                format!("{indent}- At eight"),
                format!("{indent}- Nine"),
                format!("{indent}- Deep"),
            ]
            .join("\n\n")
        );
        assert_eq!(
            stats["list_depth_clamped"], 2,
            "ilvl 9 与 99 钳 8；恰 8 不计"
        );
        assert_eq!(stats["list_items"], 3);
        // mdcore 解析：16 空格 → depth 9（解析无上限；导出面 list_shape
        // 再钳 0-8——往返稳定在九级抽象域）。
        let tree = mdcore::markdown::markdown_to_tree(&md);
        let top: Vec<_> = tree.top_level().to_vec();
        assert_eq!(top.len(), 3);
        for id in top {
            let n = tree.node(id);
            assert_eq!(n.kind, mdcore::tree::BlockKind::ListItem);
            assert_eq!(n.attrs.get("depth").map(String::as_str), Some("9"));
        }
    }

    /// r5-t1：标题优先——Heading 样式段带 numPr → 标题赢（Word 编号标题
    /// 仍显示为标题；mdcore 标题非列表项），编号丢弃计数 + 标题中断
    /// 有序计数（后续同 numId 项重起）。
    #[test]
    fn list_heading_precedence_drops_numbering() {
        let doc = doc_of(vec![
            numbered_para(
                2,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("One")],
            ),
            Block::Paragraph(ParagraphData {
                style: Some("Heading2".to_string()),
                heading_level: Some(2),
                fields_dropped: 0,
                runs: vec![run("Numbered heading")],
                source: BlockSource::Body,
                numbering: Some(ParagraphNumbering {
                    num_id: 2,
                    ilvl: 0,
                    kind: Some(ListKind::Ordered),
                    format: Some("decimal".to_string()),
                }),
                indent: None,
                footnote_refs: Vec::new(),
                // #316：未列字段（resolved_number 等）经缺省承接。
                ..Default::default()
            }),
            numbered_para(
                2,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("Two")],
            ),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(md, ["1. One", "## Numbered heading", "1. Two"].join("\n\n"));
        assert_eq!(stats["list_numbering_dropped"], 1);
        assert_eq!(stats["list_items"], 2);
        assert_eq!(stats["headings"], 1);
        assert_eq!(stats["paragraphs"], 0);
    }

    /// r5-t1：空列表项 → marker + 尾空格行（mdcore 解析为空 ListItem——
    /// 结构保真；Word 空编号段同视觉）。
    #[test]
    fn list_empty_item_emits_marker_only() {
        let doc = doc_of(vec![
            numbered_para(1, 0, Some(ListKind::Bullet), Some("bullet"), vec![]),
            numbered_para(1, 0, Some(ListKind::Ordered), Some("decimal"), vec![]),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(md, "- \n\n1. ");
        assert_eq!(stats["list_items"], 2);
        let tree = mdcore::markdown::markdown_to_tree(&md);
        let top: Vec<_> = tree.top_level().to_vec();
        assert_eq!(top.len(), 2);
        for (id, marker) in top.iter().zip(["-", "1."]) {
            let n = tree.node(*id);
            assert_eq!(n.kind, mdcore::tree::BlockKind::ListItem);
            assert_eq!(n.text.to_string(), "");
            assert_eq!(n.attrs.get("marker").map(String::as_str), Some(marker));
        }
    }

    /// r5-t1：夹具全链——import markdown → mdcore 解析 → 列表树结构
    ///（嵌套父子 + marker 逐项 + 与标题/表共存；e2e docx_import.py 的
    /// 原生层镜像）。
    #[test]
    fn fixture_markdown_parses_to_list_tree() {
        let payload = import_json(&build_sample_docx());
        let md = payload["markdown"].as_str().expect("markdown");
        let tree = mdcore::markdown::markdown_to_tree(md);
        use mdcore::tree::BlockKind;
        let top: Vec<_> = tree.top_level().to_vec();
        let top_kinds: Vec<BlockKind> = top.iter().map(|&id| tree.node(id).kind).collect();
        // P2-1：页眉前导（标记段 + 内容段）在先 + 页脚尾接（标记段 +
        // 内容段）在后——块序 = 2 + 7 + 2。
        assert_eq!(
            top_kinds,
            vec![
                BlockKind::Paragraph, // 页眉来源标记（HTML 注释承载为普通段）
                BlockKind::Paragraph, // 页眉内容（Header Slot）
                BlockKind::Heading,
                BlockKind::Paragraph,
                BlockKind::Table,
                BlockKind::ListItem,
                BlockKind::ListItem,
                BlockKind::ListItem,
                BlockKind::ListItem,
                BlockKind::Paragraph, // 页脚来源标记
                BlockKind::Paragraph, // 页脚内容（Footer Slot）
            ]
        );
        // 嵌套：Second step 的子项 = Nested step（2 空格缩进 → depth 2）。
        let second = top[6];
        assert_eq!(tree.node(second).text.to_string(), "Second step");
        let children = tree.node(second).children.clone();
        assert_eq!(children.len(), 1);
        let nested = tree.node(children[0]);
        assert_eq!(nested.text.to_string(), "Nested step");
        assert_eq!(nested.attrs.get("depth").map(String::as_str), Some("2"));
        assert_eq!(nested.parent, Some(second));
        // 顶级列表项 marker 逐项（有序 1./2. + 无序 -/-）。
        for (id, marker) in top[5..9].iter().zip(["1.", "2.", "-", "-"]) {
            assert_eq!(
                tree.node(*id).attrs.get("marker").map(String::as_str),
                Some(marker)
            );
        }
    }

    #[test]
    fn heading_styles_map_with_clamp() {
        let doc = doc_of(vec![
            para(Some("Heading1"), vec![run("一")]),
            para(Some("Heading3"), vec![run("三")]),
            para(Some("Heading9"), vec![run("九")]),
            para(Some("Title"), vec![run("题")]),
            para(Some("Normal"), vec![run("常")]),
            para(None, vec![run("无")]),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        let lines: Vec<&str> = md.split("\n\n").collect();
        assert_eq!(lines[0], "# 一");
        assert_eq!(lines[1], "### 三");
        // Heading7-9 钳 6（MD 上限 h6——mdcore heading_of 1-6）
        assert_eq!(lines[2], "###### 九");
        assert_eq!(lines[3], "题");
        assert_eq!(lines[4], "常");
        assert_eq!(lines[5], "无");
        assert_eq!(stats["headings"], 3);
        assert_eq!(stats["paragraphs"], 3);
    }

    /// #13（A1）真 Word 夹具全链：中文版 styleId="1"-"9"（样式名
    /// `heading N` 链命中）→ md 标题映射 + stats.headings。夹具实含
    /// 4 标题段（1/2/3/2——COM 原始输出 [1][2][6][13]，issue 验收行的
    /// 「3 段 1,2,3」为报告记忆性表述，超集覆盖）。#14 面（PAGE 域
    /// 缓存结果前缀 "11.1"）不在本测试锚定范围。
    #[test]
    fn tr13_real_word_fixture_heading_chain() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-artifacts/TR2/fixtures/word-complex.docx");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
        let out = import_docx_core(&bytes).expect("compat import of real-Word fixture");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("stats json");
        let stats = &parsed["stats"];
        let md = parsed["markdown"].as_str().expect("md");
        // headings: 0 → 4（修复钉定）。
        assert_eq!(stats["headings"], 4, "夹具 4 标题段全识别: {stats}");
        // md 层级映射（TR-2D 断链修复的产品面证据）。
        assert!(md.contains("# 项目技术方案书"), "h1: {md}");
        assert!(md.contains("\n## "), "h2 在场（第一章/第二章）");
        assert!(md.contains("\n### "), "h3 在场（1.1 表格）");
        // 层级序列精确：# / ## / ### / ##（四段）。
        let levels: Vec<usize> = md
            .split('\n')
            .filter(|l| l.starts_with('#'))
            .map(|l| l.chars().take_while(|&c| c == '#').count())
            .collect();
        assert_eq!(levels, vec![1, 2, 3, 2], "层级序列（夹具真实结构）");
    }

    /// #14（A2）真 Word 夹具全链：PAGE 域缓存结果剥离 → h3 md 行
    /// `### 1.1 表格`（非 `### 11.1 表格`）+ stats.fields_dropped。
    #[test]
    fn tr14_real_word_page_field_stripped_chain() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-artifacts/TR2/fixtures/word-complex.docx");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
        let out = import_docx_core(&bytes).expect("compat import");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("stats json");
        let stats = &parsed["stats"];
        let md = parsed["markdown"].as_str().expect("md");
        // TR-2D P1-1 真根因收口：域缓存结果 "1" 剥离。
        assert!(md.contains("### 1.1 表格"), "h3 行非 11.1: {md}");
        assert!(!md.contains("11.1"), "域缓存残留零容忍");
        assert_eq!(stats["fields_dropped"], 1, "剥离计数: {stats}");
        // 回归（不过度修正）：中文序数标题逐字。
        // #17：ins 文本=当前生效——REV-INSERT- 前缀在场（逐字不变仍成立）。
        assert!(
            md.contains("## REV-INSERT-第一章 概述"),
            "无编号标题不变: {md}"
        );
    }

    #[test]
    fn adjacent_same_format_runs_merge() {
        let doc = doc_of(vec![para(
            None,
            vec![
                style_run("a", true, false),
                style_run("b", true, false),
                style_run("c", false, false),
                style_run("  ", true, true),
            ],
        )]);
        let (md, _) = docx_to_markdown(&doc);
        // 合并消除 `**a**` + `**b**` 直连歧义；纯空白 run 原样直出无标记。
        assert_eq!(md, "**ab**c  ");
    }

    #[test]
    fn link_variants_render() {
        let doc = doc_of(vec![
            para(
                None,
                vec![RunData {
                    hyperlink: Some(HyperlinkTarget::External {
                        target: "https://ex.com/a b".into(),
                    }),
                    ..run("ext")
                }],
            ),
            para(
                None,
                vec![RunData {
                    hyperlink: Some(HyperlinkTarget::Anchor {
                        anchor: "toc".into(),
                    }),
                    ..run("anc")
                }],
            ),
            para(
                None,
                vec![RunData {
                    hyperlink: Some(HyperlinkTarget::Unresolved { rid: "rId9".into() }),
                    ..style_run("unres", true, false)
                }],
            ),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        let lines: Vec<&str> = md.split("\n\n").collect();
        assert_eq!(lines[0], "[ext](<https://ex.com/a b>)");
        assert_eq!(lines[1], "[anc](#toc)");
        // Unresolved：目标不可用 → 降级纯文本（强调保留），不计链接数。
        assert_eq!(lines[2], "**unres**");
        assert_eq!(stats["hyperlinks"], 2);
    }

    /// #276（审计 B-1）：敌意 docx javascript: 超链接全链负向——
    /// office-io 读面 url_policy 消毒（降级 Unresolved + UnsafeLink
    /// 警告）之后，本层 markdown 出口不再发射
    /// `[text](javascript:...)`（旧行为：GFM destination 直转，敌意
    /// 链接进编辑器）。真实 docx 字节全链（写面构造敌意件 →
    /// import_docx_core 读面消毒 → markdown 断言）。
    #[test]
    fn unsafe_link_scheme_not_exported_to_markdown() {
        use mdc_office_io::docx::RunSpec;
        let mut w = mdc_office_io::docx::DocxWriter::new();
        w.paragraph(&[
            RunSpec::text("敌意 "),
            RunSpec::link("点我", "javascript:alert(1)"),
            RunSpec::text(" 合法 "),
            RunSpec::link("正常", "https://safe.example/x"),
            RunSpec::text("。"),
        ]);
        let bytes = w.save_to_vec().unwrap();

        // import_docx_core 返回 {markdown, stats} JSON 包装——断言按
        // 面拆分：markdown 面（输出内容）与 stats.warnings 面（消毒
        // 登记可见性）分开钉。
        let out = import_docx_core(&bytes).unwrap();
        // ① markdown 面：敌意链接降级纯文本（点我无链接形态）、
        //    合法链接原样保留。
        assert!(
            out.contains(r#""markdown":"敌意 点我 合法 [正常](https://safe.example/x)。""#),
            "敌意降级纯文本 + 合法链接保留：{out}"
        );
        assert!(
            !out.contains("](javascript"),
            "敌意 scheme 链接形态不得进 markdown：{out}"
        );
        // ② 消毒登记面：unsafe_link 警告恒可见（detail 含拒绝原因——
        //    警告文案里的 javascript: 是登记面，非 markdown 输出面）。
        assert!(
            out.contains(r#""kind":"unsafe_link""#),
            "消毒警告可见性：{out}"
        );
    }

    #[test]
    fn table_cell_sanitization_and_padding() {
        let doc = doc_of(vec![Block::Table(TableData {
            rows: vec![
                vec!["a|b".to_string(), "x".to_string()],
                vec!["two\nlines".to_string()],
            ],
            merges: vec![],
            source: BlockSource::Body,
            nested: false,
        })]);
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(md, "| a\\|b | x |\n| --- | --- |\n| two lines |  |");
        assert_eq!(stats["tables"], 1);
        // 空表跳过。
        let (md2, stats2) = docx_to_markdown(&doc_of(vec![Block::Table(TableData {
            rows: vec![],
            merges: vec![],
            source: BlockSource::Body,
            nested: false,
        })]));
        assert_eq!(md2, "");
        assert_eq!(stats2["tables"], 0);
    }

    /// #22 P2-2 单元级：merges 清单 → stats `merged_ranges` 计数 +
    /// GFM 栅格对齐降级（覆盖格空——引擎读面已栅格展开，render 补齐
    /// = 防御位）。
    #[test]
    fn merged_ranges_stat_and_gfm_grid() {
        let doc = doc_of(vec![Block::Table(TableData {
            rows: vec![
                vec!["合并A".to_string(), String::new(), "C".to_string()],
                vec![String::new(), String::new(), String::new()],
            ],
            merges: vec![
                MergeRange {
                    start_row: 0,
                    start_col: 0,
                    end_row: 1,
                    end_col: 1,
                },
                MergeRange {
                    start_row: 0,
                    start_col: 2,
                    end_row: 0,
                    end_col: 2,
                },
            ],
            source: BlockSource::Body,
            nested: false,
        })]);
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(stats["merged_ranges"], 2, "两处合并计数: {stats}");
        assert_eq!(stats["tables"], 1);
        // GFM 降级：栅格对齐（覆盖格空）——无合并语法可表达，登记即可。
        assert_eq!(md, "| 合并A |  | C |\n| --- | --- | --- |\n|  |  |  |");
    }

    /// #39：嵌套表 wasm 面——外层 + nested=true 内层两块 → GFM 双表
    /// 沿文档序 + stats.tables = 2（表数守恒）；nested 标记不改渲染
    ///（分流面归消费方）。
    #[test]
    fn nested_tables_flow_to_markdown_and_stats() {
        let doc = doc_of(vec![
            Block::Table(TableData {
                rows: vec![vec!["外层A".to_string(), "详见下表".to_string()]],
                merges: vec![],
                source: BlockSource::Body,
                nested: false,
            }),
            Block::Table(TableData {
                rows: vec![
                    vec![
                        "费率".to_string(),
                        "中标金额".to_string(),
                        "服务招标".to_string(),
                    ],
                    vec![
                        "100万元以下".to_string(),
                        "1.5%".to_string(),
                        "1.0%".to_string(),
                    ],
                ],
                merges: vec![],
                source: BlockSource::Body,
                nested: true,
            }),
        ]);
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(stats["tables"], 2, "表数守恒: {stats}");
        let outer_pos = md.find("| 外层A | 详见下表 |").expect("外层 GFM");
        let inner_pos = md.find("| 费率 | 中标金额 | 服务招标 |").expect("内层 GFM");
        assert!(outer_pos < inner_pos, "文档序（外层在前）: {md}");
        assert!(md.contains("| 100万元以下 | 1.5% | 1.0% |"), "内层行: {md}");
    }

    /// #22 P2-2 真夹具全链：word-merged.docx → `merged_ranges: 2` +
    /// 内容不重复（每标记恰一次）+ 栅格对齐行形状。
    #[test]
    fn p2_2_word_merged_fixture_two_merges() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-artifacts/TR2/fixtures/word-merged.docx");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
        let out = import_docx_core(&bytes).expect("compat import of merged fixture");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("stats json");
        let stats = &parsed["stats"];
        let md = parsed["markdown"].as_str().expect("md");
        // 验收：2 处合并（表 1 纵 1 + 横 1；表 0 无合并）。
        assert_eq!(stats["merged_ranges"], 2, "验收 2 处合并: {stats}");
        assert_eq!(stats["tables"], 2, "夹具两表均入投影: {stats}");
        // 栅格对齐行形状：内容居原点格（多段 \n → 空格清洗），覆盖/续格空。
        assert!(
            md.contains("| R1C1 R2C1 | R1C2 R1C3 |  |"),
            "表 1 首行栅格形: {md}"
        );
        assert!(md.contains("|  | R2C2 | R2C3 |"), "续格置空行: {md}");
        // 内容不重复：每标记在 md 中恰一次。
        for marker in ["R1C1", "R2C1", "R1C2", "R1C3", "R2C2", "R2C3", "R4C3"] {
            assert_eq!(md.matches(marker).count(), 1, "标记 {marker} 恰一次: {md}");
        }
    }

    #[test]
    fn skip_counts_cover_headers_footers_images() {
        let doc = DocxDocument {
            blocks: vec![para(None, vec![run("body")])],
            headers: vec![
                HeaderFooterData {
                    variant: HeaderFooterVariant::Default,
                    blocks: vec![para(None, vec![run("h")])],
                },
                // 挂接但无可见内容（空段——Word 空槽常态）→ 无输出、
                // imported 不计（无内容可标记非丢失）。
                HeaderFooterData {
                    variant: HeaderFooterVariant::First,
                    blocks: vec![para(None, vec![])],
                },
            ],
            footers: vec![HeaderFooterData {
                variant: HeaderFooterVariant::Even,
                blocks: vec![para(None, vec![run("f")])],
            }],
            images: vec![
                mdc_office_io::docx::ImageAsset {
                    part_name: "word/media/a.png".into(),
                    content_type: Some("image/png".into()),
                    bytes: vec![1],
                    references: vec![],
                    body_uses: 2,
                },
                mdc_office_io::docx::ImageAsset {
                    part_name: "word/media/orphan.png".into(),
                    content_type: Some("image/png".into()),
                    bytes: vec![1],
                    references: vec![],
                    body_uses: 0,
                },
            ],
            // #19：扩面默认空（本测断言面向 headers/footers/images）。
            comments: Vec::new(),
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            // #257：节面默认空（helper 断言不涉节）。
            sections: Vec::new(),
            warnings: vec![mdc_office_io::io::CompatibilityWarning {
                part: "word/document.xml".into(),
                kind: mdc_office_io::io::WarningKind::BrokenReference,
                detail: "dangling r:id rId7".into(),
            }],
        };
        let (md, stats) = docx_to_markdown(&doc);
        // P2-1：全量接入——skipped 恒 0（兼容键）；imported = 有可见
        // 内容的部件数（Default 页眉 + Even 页脚；First 空槽不计）。
        assert_eq!(stats["headers_skipped"], 0);
        assert_eq!(stats["footers_skipped"], 0);
        assert_eq!(stats["headers_imported"], 1);
        assert_eq!(stats["footers_imported"], 1);
        // 布局：页眉（前导）→ 正文 → 页脚（尾接）；标记行带变体标签。
        let lines: Vec<&str> = md.split("\n\n").collect();
        assert_eq!(lines[0], "<!-- mdcore-import: header:default -->");
        assert_eq!(lines[1], "h");
        assert_eq!(lines[2], "body");
        assert_eq!(lines[3], "<!-- mdcore-import: footer:even -->");
        assert_eq!(lines[4], "f");
        // 孤儿媒体（body_uses=0）不计——非用户可见内容。
        assert_eq!(stats["images_skipped"], 1);
        assert_eq!(stats["warnings"][0]["kind"], "broken_reference");
        // #22 P2-4：可见警告面——引擎条目 +count/severity（加性字段）；
        // 有损项（images_skipped=1）聚合进 warnings（非 0 才进、
        // count = stats 同键镜像）。
        assert_eq!(stats["warnings"][0]["count"], 1);
        assert_eq!(stats["warnings"][0]["severity"], "warn");
        assert_eq!(stats["warnings"][1]["kind"], "images_skipped");
        assert_eq!(stats["warnings"][1]["part"], "docx");
        assert_eq!(stats["warnings"][1]["count"], 1);
        assert_eq!(stats["warnings"][1]["severity"], "warn");
    }

    /// P2-1：变体标记矩阵 + 空部件无输出（挂接但无内容 → 不发射、
    /// 不计数——与 skip_counts 的单空槽互补：全空部件族）。
    #[test]
    fn header_footer_variant_markers_and_empty_parts() {
        let empty_part = |variant| HeaderFooterData {
            variant,
            blocks: vec![para(None, vec![run("")])],
        };
        let doc = DocxDocument {
            blocks: vec![para(None, vec![run("mid")])],
            // 槽位提取序：default → first → even（确定性——Word 节设置
            // 决定激活哪个槽，读面透传；发射序跟随）。
            headers: vec![
                HeaderFooterData {
                    variant: HeaderFooterVariant::Default,
                    blocks: vec![para(None, vec![run("H-default")])],
                },
                empty_part(HeaderFooterVariant::First),
                HeaderFooterData {
                    variant: HeaderFooterVariant::Even,
                    blocks: vec![para(None, vec![run("H-even")])],
                },
            ],
            footers: vec![empty_part(HeaderFooterVariant::Default)],
            images: Vec::new(),
            comments: Vec::new(),
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            // #257：节面默认空（helper 断言不涉节）。
            sections: Vec::new(),
            warnings: Vec::new(),
        };
        let (md, stats) = docx_to_markdown(&doc);
        assert_eq!(
            md,
            "<!-- mdcore-import: header:default -->\n\nH-default\n\n\
             <!-- mdcore-import: header:even -->\n\nH-even\n\n\
             mid"
        );
        assert_eq!(stats["headers_imported"], 2);
        assert_eq!(stats["footers_imported"], 0); // 空页脚族 → 无输出
        assert_eq!(stats["blocks"], 3); // 标记行不计入内容行
    }

    /// P2-1：页眉/页脚块计入 stats（标题/表/富文本与正文同构映射）
    /// + fields_dropped 聚合含页眉/页脚段。
    #[test]
    fn header_footer_blocks_count_into_stats() {
        let mut header_para = para(Some("Heading3"), vec![run("hdr title")]);
        if let Block::Paragraph(p) = &mut header_para {
            p.source = BlockSource::Header;
            p.fields_dropped = 1;
        }
        let doc = DocxDocument {
            blocks: vec![para(None, vec![run("body")])],
            headers: vec![HeaderFooterData {
                variant: HeaderFooterVariant::Default,
                blocks: vec![
                    header_para,
                    Block::Table(TableData {
                        rows: vec![vec!["a".to_string(), "b".to_string()]],
                        merges: vec![],
                        source: BlockSource::Header,
                        nested: false,
                    }),
                ],
            }],
            footers: Vec::new(),
            images: Vec::new(),
            comments: Vec::new(),
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            // #257：节面默认空（helper 断言不涉节）。
            sections: Vec::new(),
            warnings: Vec::new(),
        };
        let (md, stats) = docx_to_markdown(&doc);
        // 页眉标题 + 页眉表（GFM 同构）+ 正文。
        assert_eq!(
            md,
            "<!-- mdcore-import: header:default -->\n\n### hdr title\n\n\
             | a | b |\n| --- | --- |\n\n\
             body"
        );
        assert_eq!(stats["headings"], 1); // 页眉标题计入
        assert_eq!(stats["tables"], 1); // 页眉表计入
        assert_eq!(stats["fields_dropped"], 1); // 页眉段剥离域聚合
    }

    /// P2-1：列表计数器部件独立（页眉列表与正文列表 = 不同列表——
    /// Word 语义同构：不同部件独立编号流）。
    #[test]
    fn header_footer_list_counters_independent() {
        let doc = DocxDocument {
            blocks: vec![numbered_para(
                1,
                0,
                Some(ListKind::Ordered),
                Some("decimal"),
                vec![run("body item")],
            )],
            headers: vec![HeaderFooterData {
                variant: HeaderFooterVariant::Default,
                blocks: vec![
                    numbered_para(
                        1,
                        0,
                        Some(ListKind::Ordered),
                        Some("decimal"),
                        vec![run("hdr one")],
                    ),
                    numbered_para(
                        1,
                        0,
                        Some(ListKind::Ordered),
                        Some("decimal"),
                        vec![run("hdr two")],
                    ),
                ],
            }],
            footers: Vec::new(),
            images: Vec::new(),
            comments: Vec::new(),
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            // #257：节面默认空（helper 断言不涉节）。
            sections: Vec::new(),
            warnings: Vec::new(),
        };
        let (md, stats) = docx_to_markdown(&doc);
        // 页眉列表自 1 起数；正文列表同样自 1 起数（不延续页眉计数）。
        assert_eq!(
            md,
            "<!-- mdcore-import: header:default -->\n\n1. hdr one\n\n2. hdr two\n\n\
             1. body item"
        );
        assert_eq!(stats["list_items"], 3);
    }

    /// P2-1 验收钉定（#22）：真 Word 夹具 word-complex.docx——页眉
    /// `HEADER-TENDER-2026` 出现在输出中 + `headers_skipped: 0`（六
    /// 槽位三挂接：default 有内容，even/first 空 → imported = 1）。
    /// 夹具：test-artifacts/TR2/fixtures/word-complex.docx（真 Word 16
    /// 生成，TR-2D 报告源）。
    #[test]
    fn word_complex_fixture_header_footer_acceptance() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test-artifacts/TR2/fixtures/word-complex.docx"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
        let payload = import_json(&bytes);
        let md = payload["markdown"].as_str().expect("markdown");
        // 验收：页眉内容在输出中 + 标记行可辨。
        assert!(
            md.contains("HEADER-TENDER-2026"),
            "header content missing: {md}"
        );
        assert!(
            md.contains("<!-- mdcore-import: header:default -->"),
            "header marker missing: {md}"
        );
        // 页脚（default 槽 = FOOTER-PAGE- 文本面）。
        assert!(md.contains("FOOTER-PAGE-"), "footer content missing: {md}");
        // 验收：headers_skipped == 0（P2-1 前为 3——六槽位三挂接全跳）。
        let stats = &payload["stats"];
        assert_eq!(stats["headers_skipped"], 0);
        assert_eq!(stats["footers_skipped"], 0);
        // 三挂接槽中仅 default 有可见内容（even/first 空）。
        assert_eq!(stats["headers_imported"], 1);
        assert_eq!(stats["footers_imported"], 1);
        // 布局：页眉前导（首行区）+ 页脚尾接（末行区）。
        assert!(md.starts_with("<!-- mdcore-import: header:default -->"));
        assert!(md.ends_with("FOOTER-PAGE-") || md.contains("FOOTER-PAGE-"));
    }

    /// P2-1：标记行经 mdcore 行级解析承载为普通段（`<!-- ... -->` 无
    /// HTML 块特殊化——文本面原样往返，编辑器消费链证明）。
    #[test]
    fn header_footer_markers_parse_as_paragraphs() {
        let doc = DocxDocument {
            blocks: vec![para(None, vec![run("body")])],
            headers: vec![HeaderFooterData {
                variant: HeaderFooterVariant::Default,
                blocks: vec![para(None, vec![run("hdr")])],
            }],
            footers: Vec::new(),
            images: Vec::new(),
            comments: Vec::new(),
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            // #257：节面默认空（helper 断言不涉节）。
            sections: Vec::new(),
            warnings: Vec::new(),
        };
        let (md, _) = docx_to_markdown(&doc);
        let tree = mdcore::markdown::markdown_to_tree(&md);
        use mdcore::tree::BlockKind;
        let texts: Vec<String> = tree
            .top_level()
            .iter()
            .map(|&id| tree.node(id).text.to_string())
            .collect();
        // 标记行 = 普通段（文本原样）——首段。
        assert_eq!(texts[0], "<!-- mdcore-import: header:default -->");
        assert_eq!(texts[1], "hdr");
        assert_eq!(texts[2], "body");
        assert_eq!(tree.top_level().len(), 3);
        assert_eq!(tree.node(tree.top_level()[0]).kind, BlockKind::Paragraph);
    }

    /// #19：批注/脚注/尾注承载量计数透出（内容不入 md 投影——结构面
    /// 经 DocxDocument 字段，计数 = 承载可见）。
    #[test]
    fn note_and_comment_counts_surface() {
        use mdc_office_io::docx::{CommentData, FootnoteData};
        let doc = DocxDocument {
            blocks: vec![para(None, vec![run("body")])],
            headers: Vec::new(),
            footers: Vec::new(),
            images: Vec::new(),
            comments: vec![
                CommentData {
                    id: 701,
                    author: "示例作者".into(),
                    date: "2026-10-20T09:00:00Z".into(),
                    anchor_block_idx: Some(0),
                    blocks: Vec::new(),
                },
                CommentData {
                    id: 702,
                    author: "另一作者".into(),
                    date: String::new(),
                    anchor_block_idx: None,
                    blocks: Vec::new(),
                },
            ],
            footnotes: vec![FootnoteData {
                id: 1,
                blocks: Vec::new(),
            }],
            endnotes: vec![
                FootnoteData {
                    id: 1,
                    blocks: Vec::new(),
                },
                FootnoteData {
                    id: 2,
                    blocks: Vec::new(),
                },
            ],
            // #257：节面默认空（本断言不涉节）。
            sections: Vec::new(),
            warnings: Vec::new(),
        };
        let (_, stats) = docx_to_markdown(&doc);
        assert_eq!(stats["comments"], 2);
        assert_eq!(stats["footnotes"], 1);
        assert_eq!(stats["endnotes"], 2);
    }

    #[test]
    fn garbage_bytes_typed_container_error() {
        for garbage in [
            vec![],
            vec![0u8, 1, 2, 99, 255],
            b"not a zip at all".to_vec(),
        ] {
            let err = import_docx_core(&garbage).expect_err("garbage must reject");
            assert_eq!(err.code, "container_corrupt", "bytes={garbage:?}");
            assert!(!err.message.is_empty());
        }
    }

    #[test]
    fn truncated_docx_typed_container_error() {
        let full = build_sample_docx();
        let cut = &full[..full.len() / 2];
        let err = import_docx_core(cut).expect_err("truncated zip must reject");
        assert_eq!(err.code, "container_corrupt");
    }

    /// 夹具漂移守卫：web e2e 夹具（examples/gen_docx_fixture.rs 产线，
    /// 提交于 web/tests/fixtures/import_sample.docx）必须仍产出期望
    /// markdown——夹具损坏/漂移在原生层先红，带清晰定位信息。
    #[test]
    fn web_e2e_fixture_imports_expected_markdown() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/tests/fixtures/import_sample.docx"
        );
        let bytes = std::fs::read(path)
            .unwrap_or_else(|e| panic!("read fixture {path}: {e} — regenerate: cargo run -p mdcore-wasm --example gen_docx_fixture -- ../../web/tests/fixtures/import_sample.docx"));
        let payload = import_json(&bytes);
        assert!(
            payload["markdown"]
                .as_str()
                .expect("markdown")
                .contains("# Imported Title"),
            "fixture markdown drifted: {}",
            payload["markdown"]
        );
        assert_eq!(payload["stats"]["tables"], 1);
        assert_eq!(payload["stats"]["images_skipped"], 1);
        // r5-t1：列表节在真夹具上可达（5 项——与 build_sample_docx 同源）。
        assert_eq!(payload["stats"]["list_items"], 5);
        assert!(
            payload["markdown"]
                .as_str()
                .expect("markdown")
                .contains("2. Second step\n\n  1. Nested step"),
            "fixture list section drifted: {}",
            payload["markdown"]
        );
    }

    /// r4-t2：错误面完备性——`DocxError::Limit`（写面资源限，读路径
    /// 不可达——office-io `MAX_PARAGRAPHS`/`MAX_RUNS`）在导入面映射为
    /// `docx_limit`（与写导出面同码；完备性映射，模块文档错误码表）。
    /// 读面既有码不动：Container(PrescanRejected) → container_prescan。
    #[test]
    fn limit_variant_maps_to_docx_limit_code() {
        let e = map_error(&mdc_office_io::docx::DocxError::Limit(
            "run count over limit 4000000".into(),
        ));
        assert_eq!(e.code, "docx_limit");
        assert!(
            e.message.starts_with("docx writer resource limit"),
            "{:?}",
            e.message
        );
        let prescan = map_error(&mdc_office_io::docx::DocxError::Container(
            mdc_office_io::io::ReadError::PrescanRejected {
                reason: "input size over limit",
                entry: None,
            },
        ));
        assert_eq!(prescan.code, "container_prescan", "读面既有码零漂移");
    }
}
