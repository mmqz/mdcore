//! t18：编辑器核心文档 → DOCX 导出切片（office-io 写面经 wasm 桥）。
//!
//! **定位（2026-10-14，t18 立项）**：编辑器当前文档（mdcore
//! [`BlockTree`]——唯一语义真理，主档 §9）→ [`mdc_office_io::docx`]
//! 的 [`DocxWriter`] 写面 → zip 容器字节 → web 侧 `Blob + a.download`
//! 下载 `document.docx`。与 t16（DOCX 导入）合流 = DOCX 往返闭环；
//! 三域纪律唯一新边沿用 t16/t17 的 mdcore-wasm → office-io（消费方
//! 边界；office-io 对 mdcore 零依赖不变）。
//!
//! **t20 增量（2026-10-15）**：office-io 写面五面扩（底稿 = 分析仓
//! `office-line/docx-r1/WRITE_FACE_PROBE.md` §4 映射草案）后，t18
//! 登记的四件降级 + 块级 code 等宽 + 行内 del·u **整批消解**；
//! 残余降级 = code lang/fence（库无语法高亮/元数据面）与 link
//! mark（模型 [`MarkSpan`] 无 URL 载荷——mdcore MD 域上游缺口）。
//!
//! **t22 增量（2026-10-16）**：多行 item 续行缩进（t20 登记残余
//! 翻转为已表达）——office-io 新面 [`DocxWriter::list_item_lines`]：
//! 一个列表项 = 一个 OOXML 段落（行间 `w:br w:type="textWrapping"`
//! 软换行）+ 段落级 hanging `w:ind`（left = 720×(ilvl+1)、hanging
//! 360——与编号抽象层级同值）→ **续行与首行文字列对齐**；单行项
//! 既有路径字节级零变化（纯增量路径）。
//!
//! ## 暴露面（[`WasmCore::export_docx`]——既有类增量方法，状态面零变化）
//!
//! - 入参：无（导出 `self` 当前块树——宿主编辑器文档本体）。
//! - 成功：DOCX 字节（wasm-bindgen `Vec<u8>` → JS `Uint8Array`）。
//! - 失败：抛 JSON 字符串 `{"code": "...", "message": "..."}`
//!   （t16/t17 错误面约定，宿主 `JSON.parse` 出结构化错误）。
//!
//! ## 错误码表
//!
//! | code | 含义 |
//! |---|---|
//! | `docx_build` | 引擎写/打包错误（[`DocxError::Io`] 打包失败为唯一现实路径；[`DocxError::Parse`] = heading 级越界 / list ilvl 越界，映射层恒钳位故不可达，完备性仍映射）。**映射层是全函数——降级永不报错**；`Container` 变体只在读面产生，写侧不可达（完备性登记）。 |
//! | `docx_limit` | **r4-t2 写面资源限**（office-io `mdc_office_io::docx::MAX_PARAGRAPHS`/`MAX_RUNS`——save 面 typed 拒；超限导出**无产物字节**——错误路径即无返回值，无半成品 Vec；常量单一真源在 office-io，本层零自立数字）。 |
//!
//! ## 映射表（BlockKind → DocxWriter；模型级，非保真渲染）
//!
//! | mdcore 块 | DocxWriter 调用 | 降级（不可表达面，显式登记） |
//! |---|---|---|
//! | `Heading`（attrs.level，缺省/不可解析 → 1；钳 1-9） | [`DocxWriter::heading`]（Heading1-9 样式 + outline 级） | 模型 level 可为任意 usize（op 面），>9 钳 9；**标题 marks 丢弃**（[`DocxWriter::heading`] 纯文本面）——#15：行内定界符仍剥除（`# **报价**` → 净文本“报价”，格式不承载），未闭合定界符按字面量保留 |
//! | `Paragraph`（text + marks） | [`DocxWriter::paragraph`] / 引用上下文 [`DocxWriter::quote_paragraph`] run 序 | mark kind 自由字符串：`strong`/`bold` → 加粗、`em`/`italic` → 斜体、`code` → 等宽（Consolas rFonts）、`del` → 删除线、`u` → 下画线（t20 消解）；**link kind → 纯文本**——模型 [`MarkSpan`] 无 URL 载荷，目标不可表达（html.rs 亦无 `<a>` 处理面——模型级无链接表示）；未知 kind → 纯文本。**#15（A3/P0）：行内 marks 消费**——模型文本是 markdown 源文本层，发射前经 [`mdcore::inline::parse_inline_marks`] 派生（`**粗**`/`*斜*`/`_斜_`/`` `码` ``/`~~删~~`/`<u>下</u>` → 净文本 + marks；未闭合 → 字面量）；node.marks 非空时 op 层覆盖赢（不二次解析——混合语义唯一化） |
//! | `CodeBlock`（attrs.lang + text） | 每行 [`DocxWriter::code_line`] 双通道等宽（段落级+run 级 Courier New） | **等宽面已表达（t20 消解）；lang、fence 结构仍丢弃**（docx-rs 无语法高亮/元数据面——库层不可达，登记维持）；文本逐字保留 |
//! | `Math`（text = LaTeX 源） | #261：同构 CodeBlock 降级——每行 [`DocxWriter::code_line`] 双通道等宽 | **公式渲染语义降级**（docx-rs 无 OMML/Office 数学发射面——库层不可达，登记）；LaTeX 源逐字保留（零丢失）；行内 `math` mark → 等宽 run（`mark_style` mono 位，code 同构） |
//! | `Table`（text = 规范管道行） | [`DocxWriter::table_header_repeat`]（N-4：首行 tblHeader 跨页重复） | 第 2 行分隔行剔除（GFM 语义位）；`\|` 反转义 → `\|`；单元格内 `\n`/`\r` → 空格（GFM 单元格单行面，t17 同向）；孤立 `\` 原样透传（模型无内联转义层——t16 同边界登记）；单元格内 marks 本就不可承载（Table = 扁平文本面，html.rs 同边界）；0 行表跳过（空 `w:tbl` 非 OOXML 合法形——防御，markdown 解析不产空表）；首行 = GFM 表头 → `tblHeader` 跨页重复（读面不回读——docx-rs 反序列化丢 trPr 扩展，登记） |
//! | `Blockquote` | 引用面 [`DocxWriter::quote_paragraph`]（ind 720×depth 钳 8 + 左边框，嵌套步进）；子块递归（段落/嵌套引用承载引用面——markdown 解析器产出的唯一形态） | 空引用（无子且 text 非空）按 text 发射（markdown.rs 序列化语义镜像）；**引用上下文中的 heading/table/code/list/hr 子块不挂 ind/pBdr**（markdown 解析器不产此形态——attach 恒挂 root；op 面可构造，防御递归保持 t18 平铺行为，登记） |
//! | `ListItem`（attrs.marker/depth） | 单行：[`DocxWriter::list_item`]：marker 判序（数字型 → Ordered，`-`/`*`/`+`/缺失 → Bullet）+ depth→ilvl（钳 0-8；ilvl 越界防御：>8 钳 8——映射层全函数）；多行（t22 消解）：[`DocxWriter::list_item_lines`]——单段落 + 行间 `w:br` + 段落级 hanging `w:ind`（left = 720×(ilvl+1)、hanging 360，与编号抽象层级同值）→ 续行与首行文字列对齐 | **列表结构已表达（t20 消解）；多行续行缩进已表达（t22 消解——行内 marks 经 rich_lines 逐行平移保持）**；子块递归保持 |
//! | `ThematicBreak` | [`DocxWriter::horizontal_rule`]（空段下边框） | 无降级（t20 消解；OOXML 无原生 hr——下边框为惯用表达） |
//!
//! 文本行处理（全局规则）：块文本按 `\n` 分行、每行剥一个行尾 `\r`
//! （CRLF 容忍——markdown.rs `split('\n')` 保留 `\r`），多行文本 →
//! 每行一个 docx 段落（mdcore 行级哲学，t16 w:br→`\n` 同向登记；
//! **例外**：ListItem 多行 = 单段 + 行间 `w:br`——t22 续行缩进面，
//! 项内软换行语义，读面 br→`\n` 约定对称）；
//! 行内孤立 `\r` 原样透传。mark 偏移单位 = char（op.rs SetMark 契约），
//! 多行切分时按行区间裁剪平移。
//!
//! ## 已知边界（登记，非缺陷）
//!
//! - run 相邻同 (bold,italic,mono,strike,underline) 合并（docx 段落
//!   run 边界不承载模型语义，t16 导入侧同款合并登记的反向；t20
//!   合并键扩至五位）；空 run 不发射；
//! - **导出→t16 重导入的读面承载边界**：numPr/pBdr/ind/rFonts 不入
//!   office-io 读模型（[`RunData`] 无字体/删除线/下画线字段，段落无
//!   缩进/边框/编号字段）——**不宣称 list/quote/hr/等宽的往返等值**；
//!   本模块测试 = 导出 XML 面直断言 + 既有文本面回归（文本零损）；
//! - 导出的 heading 样式读回 = `Heading1`..`Heading9`（office-io 读面
//!   样式 ID 透传）——全环断言见本模块测试；
//! - 不暴露面（DocxWriter 能力中本切片未接线）：脚注——**例外**：
//!   W-1 目录域（TOC）经 options 开关接线（见下）；页眉/页脚槽位、
//!   页码域（PAGE/NUMPAGES）与 PNG 图片经 P1 #190 options 接线
//!   （见下节）；正文域仍不暴露（无页面装置语义面）。
//!
//! ## W-1/W-2 增量（2026-10-28，issue #29 阶段 A0）：options 两新键
//!
//! [`export_docx_styled`] 的 options JSON 在既有 `typography`/
//! `page_geometry` 两键外新增两可选键（缺省 = 与既有导出字节级一致
//! ——纯加性，既有测试零改动锁背书）：
//!
//! - `"toc"`: `{ "enabled": bool, "level_from": 1..=9?, "level_to": 1..=9? }`
//!   ——目录域开关（W-1 / issue #6 G1 消费）。`enabled` 必填；
//!   层级缺省 1/3。enabled=true 时在**文档内容块发射之前**发射
//!   TOC 域段（Word 惯例——目录居文档开头）：fldChar begin
//!   [dirty=true——打开提示更新域] + instrText `TOC \o "lo-hi"` +
//!   separate + 占位提示文本 + end 五连（office-io
//!   [`RunSpec::field_toc_heading_range`]）。层级域校验 1-9 且
//!   from ≤ to：本层前置 typed 拒（`docx_options`——与既有形状错误
//!   同码），引擎 V8 save 漏斗兜底（本层拒后不可达——完备性登记）。
//!   `"enabled": false` ≡ 缺省（零发射，字节稳定）。
//! - `"anonymize"`: bool——暗标元数据匿名化开关（W-2 / G3 / R-11
//!   BLOCKER）。true 时 `save_to_vec()` 产物过
//!   [`anonymize_docx_props`] 后处理（docProps/core.xml → 匿名常量
//!   [unknown 作者 + epoch 零时 + revision=1]、app.xml/custom.xml →
//!   空骨架、people.xml 清空、comments.xml 作者面匿名化；正文部件
//!   原压缩流原样拷贝——零触碰）。缺省 false = 直接返回引擎产物
//!   （引擎新建面 docx-rs 默认 core.xml 本身已与匿名常量同构——
//!   office-io g3_t1 单一真源钉定；本开关 = 显式防御性再洗[幂等] +
//!   产品语义面[暗标红线开关]，引擎/库默认漂移时仍保有保证）。
//!   错误映射：后处理失败（非 docx 伪产物/zip 面）经 [`map_error`]
//!   归 `docx_build` 既有通道。
//!
//! ## P1 增量（2026-11-02，issue #190）：options 三新键——页眉/页脚/图片
//!
//! office-io 写面六槽位（`DocxWriter::header`/`first_header`/
//! `even_header`/`footer`/`first_footer`/`even_footer`——重复调用原位
//! 替换）与 `DocxWriter::image`（PNG-only，显式像素尺寸）能力已在，
//! 本切片经 [`export_docx_styled`] options 接线（投标刚需：页眉 =
//! 投标人/项目名/正副本标识，页脚 = 页码，图片 = 资质证书/业绩合同）。
//! 三键缺省 = 零发射（与既有导出字节级一致——纯加性）。
//!
//! - `"header"` / `"footer"`：`{ "runs": [run...], "first_page":
//!   {"runs": [...]}?, "even_page": {"runs": [...]}? }`——default 槽
//!   必填；`first_page`/`even_page` 独立槽可选（缺省/null/false =
//!   不设置；引擎自动置 titlePg / evenAndOddHeaders 激活位）。
//!   run = `{"text", "bold"?, "italic"?}` 或 `{"field":
//!   "page"|"numpages"}`（页码域——页眉页脚刚需面，text/field 互斥）。
//!   形状错误 → `docx_options`（既有码）。
//! - `"images"`：`{ <src>: {"png_base64", "width_px", "height_px"} }`
//!   资源表——Markdown 图片行 `![alt](src)` 的 src 命中表键时发射
//!   [`DocxWriter::image`]（单行 Paragraph 独立块——v0 行级解析器
//!   图片行即独立段落块，零树改动；`![alt](src)` 字面量在树里）。
//!   未命中 src / 非 PNG 签名 / base64 坏 → 见模块文档降级与拒收面。
//!   识别在块发射层（`effective_inline` 之前——图片是块级面，marks
//!   不适用）；多行块内图片行不识别（登记）。
//!
//! 纯内核（[`export_docx_core`]）不触碰 JsValue → 原生 cargo test
//! 直测（写→`read_docx_from_bytes`→模型断言 + XML 面 zip 本地头遍历
//! 直断言，t16/t17/t18 同款写读全环纪律）；wasm 壳由 wasm32 check +
//! web 三件套门禁背书。

use crate::WasmCore;
#[cfg(test)]
use mdc_office_io::docx::read_docx_from_bytes;
use mdc_office_io::docx::{
    anonymize_docx_props, DefaultTypography, DocxError, DocxWriter, FieldSpec, LineRuleSpec,
    LineSpacingSpec, ListKind, PageGeometry, RunSpec, TocEntry,
};
use mdcore::tree::{Attrs, BlockKind, BlockTree, MarkSpan, NodeId};
use wasm_bindgen::prelude::*;

/// typed 导出错误（错误码 + 人类可读消息——见模块文档错误码表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
}

/// 引擎写面错误 → 错误码（r4-t2：资源限 `docx_limit` 与引擎面
/// `docx_build` 可区分——见模块文档错误码表）。
fn map_error(e: &DocxError) -> ExportError {
    match e {
        DocxError::Limit(m) => ExportError {
            code: "docx_limit",
            message: format!("docx writer resource limit: {m}"),
        },
        // office-g Phase 1/2：输入校验拒绝（V1-V7）——与引擎面可区分。
        // 当前 wasm 面不可达（BlockTree 无锚点载荷——DECISIONS ②⑦
        // 门控；RunSpec 构造不经 wasm）——完备性映射，公开面暴露
        // 后即时生效。
        DocxError::InvalidInput(m) => ExportError {
            code: "docx_invalid_input",
            message: format!("docx writer invalid input: {m}"),
        },
        other => ExportError {
            code: "docx_build",
            message: format!("docx writer engine error: {other}"),
        },
    }
}

// ---------------------------------------------------------------------------
// 纯映射内核（原生可测——BlockTree 直构，零 zip 依赖直到 save_to_vec）
// ---------------------------------------------------------------------------

/// Heading attrs["level"] → 1-9（不可解析/缺失 → 1；>9 钳 9）。
/// markdown.rs 解析只产 1-6；op 面可设任意值——钳位登记见模块文档。
fn heading_level(attrs: &Attrs) -> u8 {
    attrs
        .get("level")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 9) as u8
}

/// mark kind → (bold, italic, mono, strike, underline) 五位。模型 kind
/// 为自由字符串（op.rs SetMark）：`strong`/`bold`（html.rs 规范 /
/// spike 命名）→ 加粗；`em`/`italic` → 斜体；`code` → 等宽（t20
/// 消解）；`math` → 等宽（#261——LaTeX 源码逐字降级，docx-rs 无
/// OMML 发射面，登记）；`del` → 删除线（t20 消解）；`u` → 下画线
/// （t20 消解）；其余（link/未知）→ 无面可表达 → 纯文本
/// （link = 模型无 URL 载荷，上游缺口登记）。
fn mark_style(kind: &str) -> (bool, bool, bool, bool, bool) {
    match kind {
        "strong" | "bold" => (true, false, false, false, false),
        "em" | "italic" => (false, true, false, false, false),
        "code" => (false, false, true, false, false),
        "math" => (false, false, true, false, false),
        "del" => (false, false, false, true, false),
        "u" => (false, false, false, false, true),
        _ => (false, false, false, false, false),
    }
}

/// 单行文本 + char 偏移 marks → run 序（边界集分段 + 相邻同五位
/// (bold,italic,mono,strike,underline) 合并 + 空段跳过）。空文本 →
/// 空 run 序。
fn runs_from(text: &str, marks: &[MarkSpan]) -> Vec<RunSpec> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    // 五位覆盖区间集（mark 交叠 = 位叠加——t18 bold/italic 交叠语义
    // 的 t20 扩面）。
    let mut bold: Vec<(usize, usize)> = Vec::new();
    let mut italic: Vec<(usize, usize)> = Vec::new();
    let mut mono: Vec<(usize, usize)> = Vec::new();
    let mut strike: Vec<(usize, usize)> = Vec::new();
    let mut underline: Vec<(usize, usize)> = Vec::new();
    for m in marks {
        // 防御钳位（op 面写入时已校验 ≤ len_chars；adjust_marks 维护不变量）
        let s = m.start.min(len);
        let e = m.end.min(len).max(s);
        if s == e {
            continue;
        }
        let (b, i, mo, st, u) = mark_style(&m.kind);
        if b {
            bold.push((s, e));
        }
        if i {
            italic.push((s, e));
        }
        if mo {
            mono.push((s, e));
        }
        if st {
            strike.push((s, e));
        }
        if u {
            underline.push((s, e));
        }
    }
    let mut points: Vec<usize> = vec![0, len];
    points.extend(
        bold.iter()
            .chain(italic.iter())
            .chain(mono.iter())
            .chain(strike.iter())
            .chain(underline.iter())
            .flat_map(|&(s, e)| [s, e]),
    );
    points.sort_unstable();
    points.dedup();
    let covers = |spans: &[(usize, usize)], x: usize| spans.iter().any(|&(s, e)| s <= x && x < e);
    let mut runs: Vec<RunSpec> = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a == b {
            continue;
        }
        let (bold_seg, italic_seg) = (covers(&bold, a), covers(&italic, a));
        let (mono_seg, strike_seg, underline_seg) =
            (covers(&mono, a), covers(&strike, a), covers(&underline, a));
        let seg: String = chars[a..b].iter().collect();
        // 相邻同样式合并（模型无链接 run——link 恒 None，样式同即并）
        if let Some(last) = runs.last_mut() {
            if last.bold == bold_seg
                && last.italic == italic_seg
                && last.mono == mono_seg
                && last.strike == strike_seg
                && last.underline == underline_seg
            {
                last.text.push_str(&seg);
                continue;
            }
        }
        runs.push(RunSpec::styled_full(
            seg,
            bold_seg,
            italic_seg,
            mono_seg,
            strike_seg,
            underline_seg,
        ));
    }
    runs
}

/// 块文本 → docx 段落行序列（'\n' 分行 + 每行剥一个行尾 '\r'——
/// CRLF 容忍；孤立 '\r' 原样透传，登记见模块文档）。
/// t19：GFM 行切分语义单一真源 —— [`crate::xlsx_export`] 共用
///（pub(crate)，无行为变化）。
pub(crate) fn text_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// 富文本（可多行）→ (行, 行内 marks) 序列：'\n' 分行 + 行尾 '\r'
/// 仅在发射面剥除 + mark char 偏移按行区间裁剪平移（多行切分不丢
/// 格式）。行切分 + mark 平移的单一真源（ListItem 首行/余行分道与
/// 引用段落共用）。
fn rich_lines(text: &str, marks: &[MarkSpan]) -> Vec<(String, Vec<MarkSpan>)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    loop {
        let nl = chars[start..]
            .iter()
            .position(|&c| c == '\n')
            .map(|i| start + i)
            .unwrap_or(chars.len());
        // 行尾 '\r' 只在发射面剥除；start/nl 坐标保持原文本系（偏移恒等）
        let mut emit_end = nl;
        if emit_end > start && chars[emit_end - 1] == '\r' {
            emit_end -= 1;
        }
        let line: String = chars[start..emit_end].iter().collect();
        let line_marks: Vec<MarkSpan> = marks
            .iter()
            .filter_map(|m| {
                let s = m.start.clamp(start, emit_end);
                let e = m.end.clamp(start, emit_end).max(s);
                if s == e {
                    None
                } else {
                    Some(MarkSpan {
                        start: s - start,
                        end: e - start,
                        kind: m.kind.clone(),
                    })
                }
            })
            .collect();
        out.push((line, line_marks));
        if nl == chars.len() {
            break;
        }
        start = nl + 1;
    }
    out
}

/// 富文本（可多行）→ 每行一个 docx 段落；`quote_depth > 0` 时走引用面
///（ind + 左边框，嵌套步进语义由调用方传深度）。
fn emit_rich_paragraphs(w: &mut DocxWriter, text: &str, marks: &[MarkSpan], quote_depth: usize) {
    for (line, line_marks) in rich_lines(text, marks) {
        let runs = runs_from(&line, &line_marks);
        if quote_depth > 0 {
            w.quote_paragraph(&runs, quote_depth);
        } else {
            w.paragraph(&runs);
        }
    }
}

/// #15（A3/P0）：发射前有效 (text, marks) 派生。
///
/// 模型文本 = markdown 源文本层（CM6 同步协议基石——快路径
/// `replace_text` 直写原始行文本，块级解析不剥行内定界符）。
/// 本函数在发射面消费该层：
/// - `node.marks` 非空（op 面程序化覆盖——html.rs 导入/op 测试构造）
///   → 原样透传（偏移指向原始文本，op 契约一致），**不二次解析**
///   （混合语义唯一化——覆盖与派生不叠加）；
/// - 空（产品路径恒空——markdown 解析与 CM6 同步都不产 marks）→
///   [`mdcore::inline::parse_inline_marks`] 派生：剥定界符净文本 +
///   行内 marks；未闭合定界符按字面量保留（不猜测）。
///
/// 单一漏斗：Paragraph/Blockquote 空引用/ListItem 共用；Heading 仅取
/// 净文本（格式丢弃——`DocxWriter::heading` 纯文本面，映射表登记）。
fn effective_inline(text: &str, marks: &[MarkSpan]) -> (String, Vec<MarkSpan>) {
    if marks.is_empty() {
        mdcore::inline::parse_inline_marks(text)
    } else {
        (text.to_string(), marks.to_vec())
    }
}

/// ListItem attrs → (列表类型, ilvl)：marker 首字符为 ASCII 数字 →
/// Ordered（markdown.rs `1.`/`1)` 两形态）；`-`/`*`/`+`/缺失/其他 →
/// Bullet（缺失 = op 面防御缺省，登记）。depth（1 基，markdown.rs
/// indent/2+1）→ ilvl = depth-1 钳 0-8（九级抽象域；越界防御钳位——
/// 映射全函数）。
fn list_shape(attrs: &Attrs) -> (ListKind, usize) {
    let marker = attrs.get("marker").map(|s| s.as_str()).unwrap_or("");
    let kind = if marker.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        ListKind::Ordered
    } else {
        ListKind::Bullet
    };
    let depth = attrs
        .get("depth")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    (kind, (depth - 1).min(8))
}

/// 单管道行 → 单元格序列：前导/尾随 `|` 剥离；未转义 `|` 分隔；
/// `\|` → `|` 反转义；单元格 trim；孤立 `\` 原样透传（无内联转义层）。
fn split_pipe_row(line: &str) -> Vec<String> {
    let s = line.strip_prefix('|').unwrap_or(line);
    let s = s.strip_suffix('|').unwrap_or(s);
    let mut cells: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c == '|' {
                cur.push('|');
            } else {
                cur.push('\\');
                cur.push(c);
            }
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur = String::new();
        } else {
            cur.push(c);
        }
    }
    if esc {
        cur.push('\\');
    }
    cells.push(cur.trim().to_string());
    cells
}

/// GFM 分隔行判定（每个 trim 后单元格 `:?-+:?` 形）。
fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            let core = t.strip_prefix(':').unwrap_or(t);
            let core = core.strip_suffix(':').unwrap_or(core);
            !core.is_empty() && core.chars().all(|ch| ch == '-')
        })
}

/// Table 块文本 → 表行序列（首个发射行后紧跟的分隔形行剔除；
/// 单元格 `\n`/`\r` → 空格；空白行跳过）。0 行 = 空表 → 调用方跳过。
/// t19：GFM 管道行语义单一真源 —— [`crate::xlsx_export`] 共用
///（pub(crate)，无行为变化）。
pub(crate) fn table_rows(text: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for line in text_lines(text) {
        if line.trim().is_empty() {
            continue;
        }
        let cells = split_pipe_row(line);
        if out.len() == 1 && is_separator_row(&cells) {
            continue;
        }
        out.push(
            cells
                .into_iter()
                .map(|c| c.replace(['\n', '\r'], " "))
                .collect(),
        );
    }
    out
}

/// #196 任务 2：TOC 标题书签配对表——(块 id, 行序) → 书签名
/// （[`collect_toc_entries`] 收集位置序 = [`DocxWriter::toc_entries`]
/// 返回名序，zip 配对后供 [`emit_block`] 查表；空 = 无 TOC 或无收录
/// 标题，Heading 走裸 [`DocxWriter::heading`] 路径）。
type TocMarkMap = std::collections::HashMap<(NodeId, usize), String>;

/// #196 任务 2：收集 TOC 目录项（镜像 [`emit_block`] 遍历形状：
/// Document/Blockquote/ListItem 递归 + Heading 收录；层级域
/// lo..=hi 过滤 = TOC `\o` 开关镜像——F9 后 Word 重排仅收录范围内，
/// 静态项与之一致）。多行标题逐行逐项（每行一目录项一书签——与
/// emit_block 逐行发射镜像，行间序号配对不错位）。
fn collect_toc_entries(
    tree: &BlockTree,
    id: NodeId,
    lo: u8,
    hi: u8,
    entries: &mut Vec<TocEntry>,
    positions: &mut Vec<(NodeId, usize)>,
) {
    let node = tree.node(id);
    match node.kind {
        // 递归容器面（与 emit_block 同形：虚根透明 / 引用嵌套 / 列表
        // 嵌套 item；Blockquote 空引用无标题面自然零递归）。
        BlockKind::Document | BlockKind::Blockquote | BlockKind::ListItem => {
            for &c in &node.children {
                collect_toc_entries(tree, c, lo, hi, entries, positions);
            }
        }
        BlockKind::Heading => {
            let level = heading_level(&node.attrs);
            if (lo..=hi).contains(&level) {
                // 净文本口径与 emit_block Heading 分支同源
                // （effective_inline 剥定界符 + text_lines 展开）。
                let (clean, _) = effective_inline(&node.text.to_string(), &node.marks);
                for (idx, line) in text_lines(&clean).iter().enumerate() {
                    entries.push(TocEntry::new(level, *line));
                    positions.push((id, idx));
                }
            }
        }
        _ => {}
    }
}

/// 单块发射（先序递归；降级语义见模块文档映射表）。`quote_depth` =
/// 引用上下文深度（0 = 无引用面；Blockquote 子块递归传 depth+1 嵌套
/// 步进——仅段落/嵌套引用承载 ind/pBdr，其余子块登记见模块文档）。
fn emit_block(
    tree: &BlockTree,
    id: NodeId,
    w: &mut DocxWriter,
    quote_depth: usize,
    images: &ImageTable,
    toc_marks: &TocMarkMap,
) -> Result<(), ExportError> {
    let node = tree.node(id);
    let text = node.text.to_string();
    match node.kind {
        // 虚根透明递归（top_level 已绕过；op 面禁止 Document 子块——防御）
        BlockKind::Document => {
            for &c in &node.children {
                emit_block(tree, c, w, quote_depth, images, toc_marks)?;
            }
        }
        BlockKind::Heading => {
            let level = heading_level(&node.attrs);
            // #15：行内定界符剥除（净文本），格式丢弃——映射表登记；
            // 未闭合定界符字面量保留。
            let (clean, _) = effective_inline(&text, &node.marks);
            for (idx, line) in text_lines(&clean).iter().enumerate() {
                // #196 任务 2：TOC 收录标题行携书签发射（目录项跳转
                // 目标侧）；未收录（无 TOC / 层级域外）走裸 heading
                // （零漂移）。
                match toc_marks.get(&(id, idx)) {
                    Some(bm) => {
                        w.heading_bookmarked(level, line, bm)
                            .map_err(|e| map_error(&e))?;
                    }
                    None => {
                        w.heading(level, line).map_err(|e| map_error(&e))?;
                    }
                }
            }
        }
        BlockKind::Paragraph => {
            // P1 #190：图片行直通——整块单行 + `![alt](src)` 形 + src
            // 命中资源表 → [`DocxWriter::image`]（内嵌 PNG，显式尺寸）。
            // 识别在 `effective_inline` 之前（原始字面量——图片是块级
            // 面，行内 marks 不适用）；alt 不发射（OOXML 图注/alt-text
            // 是独立面，office-io image() 不暴露——登记）。未命中 src /
            // 多行块 / 嵌套括号形 → 原文本路径（字面量原样，降级登记
            // 见模块文档）。
            let mut emitted_image = false;
            if !text.contains('\n') {
                if let Some((_, src)) = image_line(&text) {
                    if let Some(img) = images.get(src) {
                        w.image(&img.png, img.width_px, img.height_px)
                            .map_err(|e| map_error(&e))?;
                        emitted_image = true;
                    }
                }
            }
            if !emitted_image {
                let (clean, marks) = effective_inline(&text, &node.marks);
                emit_rich_paragraphs(w, &clean, &marks, quote_depth);
            }
        }
        BlockKind::CodeBlock => {
            // 双通道等宽（t20 消解）；lang/fence 降级登记见模块文档。
            for line in text_lines(&text) {
                w.code_line(line);
            }
        }
        // #261：数学块同构 CodeBlock 降级——LaTeX 源逐字等宽逐行
        // （docx-rs 无 OMML 发射面；源码零丢失，渲染语义降级登记）。
        BlockKind::Math => {
            for line in text_lines(&text) {
                w.code_line(line);
            }
        }
        BlockKind::Table => {
            let rows = table_rows(&text);
            if !rows.is_empty() {
                // N-4：首行 tblHeader（GFM 首行即表头同构——跨页表头
                // 重复；≥2 行才打标，见 DocxWriter::table_header_repeat）。
                w.table_header_repeat(&rows);
            }
        }
        BlockKind::Blockquote => {
            // 引用面（t20 消解）：嵌套步进 depth+1；空引用（无子）按
            // text 发射——镜像 markdown.rs 序列化语义。
            let d = quote_depth + 1;
            if node.children.is_empty() {
                let (clean, marks) = effective_inline(&text, &node.marks);
                emit_rich_paragraphs(w, &clean, &marks, d);
            }
            for &c in &node.children {
                emit_block(tree, c, w, d, images, toc_marks)?;
            }
        }
        BlockKind::ListItem => {
            // 列表结构（t20 消解）：marker 判序 + depth→ilvl。多行 item
            // 文本（t22 消解）：单行 → 编号段（既有面，字节级零变化）；
            // 多行 → [`DocxWriter::list_item_lines`]——一个列表项 = 一个
            // OOXML 段落（行间 w:br 软换行 + 段落级 hanging ind），
            // 续行与首行文字列对齐（每行 run 序由 rich_lines 平移行内
            // marks）。子块递归（嵌套 item 各自编号段，次序保持）。
            let (kind, ilvl) = list_shape(&node.attrs);
            let (clean, eff_marks) = effective_inline(&text, &node.marks);
            let lines = rich_lines(&clean, &eff_marks);
            let line_runs: Vec<Vec<RunSpec>> = lines
                .iter()
                .map(|(line, line_marks)| runs_from(line, line_marks))
                .collect();
            if line_runs.len() <= 1 {
                // 单行：既有 list_item 路径不动（无显式 ind——稳定面）。
                if let Some(first) = line_runs.first() {
                    w.list_item(first, kind, ilvl).map_err(|e| map_error(&e))?;
                }
            } else {
                w.list_item_lines(&line_runs, kind, ilvl)
                    .map_err(|e| map_error(&e))?;
            }
            for &c in &node.children {
                emit_block(tree, c, w, quote_depth, images, toc_marks)?;
            }
        }
        BlockKind::ThematicBreak => {
            // 水平线（t20 消解）：空段下边框。
            w.horizontal_rule();
        }
    }
    Ok(())
}

/// 编辑器核心块树 → DOCX 字节（zip 容器）。映射全函数：不可表达面
/// 显式降级（模块文档映射表），映射层永不报错；错误面 = 引擎写/打包。
pub fn export_docx_core(tree: &BlockTree) -> Result<Vec<u8>, ExportError> {
    let mut w = DocxWriter::new();
    // P1 #190：无 options 面 = 空图片表（图片行降级为字面量文本——
    // 与既有行为逐字节一致，图片只在 styled 面资源表供给）。
    let empty: ImageTable = ImageTable::new();
    // #196 任务 2：无 options 面无 TOC——空配对表（Heading 裸路径）。
    let no_toc: TocMarkMap = TocMarkMap::new();
    for &child in tree.top_level() {
        emit_block(tree, child, &mut w, 0, &empty, &no_toc)?;
    }
    w.save_to_vec().map_err(|e| map_error(&e))
}

// ---------------------------------------------------------------------------
// issue #10（TR-4）：G2 格式面消费——export_docx_styled 加性导出面。
//
// 现状勘误（立项依据）：G2（office-g `3f16779`）已供
// [`DocxWriter::default_typography`]（字体三槽 + 半磅字号 + 行距）与
// [`DocxWriter::page_geometry`]（twips 全八字段）+ V9 save 漏斗域校验，
// 但本模块 [`WasmCore::export_docx`] 的 BlockTree→DocxWriter 映射未
// 消费——web 默认导出恒为引擎默认态（实测 10.0pt / 85.2pt 边距）。
//
// 设计（加性面，零破坏既有契约）：
// - 新方法 [`WasmCore::export_docx_styled`]（options JSON 参数）；
//   既有 [`WasmCore::export_docx`] 字节级零变化（空 options = 引擎
//   默认态等价，测试钉定）。
// - options 字段**镜像 office-io 公开 API**（snake_case 1:1）：
//   `{"typography": {...}, "page_geometry": {...}}`——两者皆可选；
//   显式全量状态语义（子字段必填，不做局部补丁——避免本层发明
//   缺省值；调用方 UI 面负责呈现完整期望状态）。
// - 分层校验：**形状**（JSON 合法/对象形/未知顶层键/类型/缺字段/
//   line_rule 枚举）= 本层 `docx_options` 立拒；**值域语义**（twips
//   1-40000、边距非负、字号 1-32767、行距正/段距 ≤31680）= 经
//   writer 落 office-io V9 save 漏斗粘性拒绝（`docx_invalid_input`）
//   ——本层零自立数字，单一真源维持。
// ---------------------------------------------------------------------------

/// `docx_options`：options JSON 形状错误（与 V9 值域拒绝可区分）。
fn options_err(path: &str, detail: String) -> ExportError {
    ExportError {
        code: "docx_options",
        message: format!("export options `{path}`: {detail}"),
    }
}

/// 必填字符串字段提取（路径化错误——调用方可定位到字段）。
fn req_str(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    field: &str,
) -> Result<String, ExportError> {
    match obj.get(field) {
        Some(serde_json::Value::String(s)) => Ok(s.clone()),
        Some(other) => Err(options_err(
            path,
            format!("`{field}` must be a string, got {other}"),
        )),
        None => Err(options_err(
            path,
            format!("missing required field `{field}`"),
        )),
    }
}

/// 必填整数字段提取 + Rust 表示域 cast 检查（语义值域 = V9 单一真源，
/// 本处只保证 cast 不截断：`lo..=hi` 传目标类型的全域）。
fn req_int(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    field: &str,
    lo: i64,
    hi: i64,
) -> Result<i64, ExportError> {
    match obj.get(field) {
        Some(v) => {
            let n = v.as_i64().ok_or_else(|| {
                options_err(path, format!("`{field}` must be an integer, got {v}"))
            })?;
            if n < lo || n > hi {
                return Err(options_err(
                    path,
                    format!("`{field}` = {n} outside representable {lo}..={hi}"),
                ));
            }
            Ok(n)
        }
        None => Err(options_err(
            path,
            format!("missing required field `{field}`"),
        )),
    }
}

/// 可选整数字段（`None`/`null` → `None`；有值走 [`req_int`] 同款域检）。
fn opt_int(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    field: &str,
    lo: i64,
    hi: i64,
) -> Result<Option<i64>, ExportError> {
    match obj.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(_) => req_int(obj, path, field, lo, hi).map(Some),
    }
}

/// 必填布尔字段提取（W-1 `toc.enabled`——路径化错误与 [`req_str`] 同构）。
fn req_bool(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    field: &str,
) -> Result<bool, ExportError> {
    match obj.get(field) {
        Some(serde_json::Value::Bool(b)) => Ok(*b),
        Some(other) => Err(options_err(
            path,
            format!("`{field}` must be a boolean, got {other}"),
        )),
        None => Err(options_err(
            path,
            format!("missing required field `{field}`"),
        )),
    }
}

/// 可选 TOC 标题层级字段（W-1：语义域 1-9——与引擎 V8 同域；本层
/// 前置 typed 拒 [`options_err`]，引擎 save 漏斗兜底不可达——完备性）。
fn opt_toc_level(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<Option<u8>, ExportError> {
    match obj.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_i64().ok_or_else(|| {
                options_err("toc", format!("`{field}` must be an integer, got {v}"))
            })?;
            if !(1..=9).contains(&n) {
                return Err(options_err(
                    "toc",
                    format!("`{field}` = {n} outside 1..=9 (TOC heading levels)"),
                ));
            }
            Ok(Some(n as u8))
        }
    }
}

/// W-1：TOC 目录域选项（已启用形态——level_from ≤ level_to 已校验）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocOptions {
    pub level_from: u8,
    pub level_to: u8,
}

// ---------------------------------------------------------------- P1 #190：页眉/页脚槽位

/// P1 #190：页眉/页脚槽位选项（六槽位 = default/first/even × 页眉/页脚）。
///
/// `default` 必填（空 runs 数组合法——空槽语义，如「首页页眉留空、
/// 其余页有默认页眉」的 Word 惯用法）；`first`/`even` = None（缺省/
/// null/false）时不发射对应槽，Some 时发射（引擎自动置 titlePg /
/// evenAndOddHeaders 激活位——office-io `first_header`/`even_header`
/// 文档口径）。
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFooterSpec {
    pub default: Vec<RunSpec>,
    pub first: Option<Vec<RunSpec>>,
    pub even: Option<Vec<RunSpec>>,
}

/// 可选布尔字段（P1 #190——run 的 bold/italic；缺省/null = false，
/// 与 W-1 `toc.enabled` 的必填布尔 [`req_bool`] 相对）。
fn opt_bool(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    field: &str,
) -> Result<bool, ExportError> {
    match obj.get(field) {
        None | Some(serde_json::Value::Null) => Ok(false),
        Some(serde_json::Value::Bool(b)) => Ok(*b),
        Some(other) => Err(options_err(
            path,
            format!("`{field}` must be a boolean, got {other}"),
        )),
    }
}

/// P1 #190：页眉/页脚 run 解析（`{"text", "bold"?, "italic"?}` 文本 run
/// 或 `{"field": "page"|"numpages"}` 页码域 run——text/field 互斥）。
///
/// 域面只暴露 PAGE/NUMPAGES（页眉页脚刚需：页码定位）；TOC 域不在此
/// 面（目录是文档体语义，经 W-1 `toc` 键——避免两通道同义）。run 内
/// 未知键拒收（显式全量状态语义——与顶层白名单同纪律）。
fn parse_hf_run(v: &serde_json::Value, path: &str) -> Result<RunSpec, ExportError> {
    let obj = v
        .as_object()
        .ok_or_else(|| options_err(path, "run must be an object".into()))?;
    for key in obj.keys() {
        if !matches!(
            key.as_str(),
            "text" | "bold" | "italic" | "field" | "charStyle"
        ) {
            return Err(options_err(
                path,
                format!(
                    "unknown run field `{key}` (allowed: text, bold, italic, field, charStyle)"
                ),
            ));
        }
    }
    // 域 run：{"field": "page"|"numpages"}——text/field 互斥（域 run 的
    // 显示文本是渲染期计算面，混入 text 语义矛盾）。charStyle 与域
    // run 互斥（域 run 无用户样式面——页码字符形态归样式定义侧）。
    if let Some(fv) = obj.get("field") {
        let s = fv
            .as_str()
            .ok_or_else(|| options_err(path, format!("`field` must be a string, got {fv}")))?;
        let field = match s {
            "page" => FieldSpec::Page,
            "numpages" => FieldSpec::NumPages,
            other => {
                return Err(options_err(
                    path,
                    format!("`field` must be page|numpages, got {other:?}"),
                ))
            }
        };
        if obj.get("text").is_some() {
            return Err(options_err(
                path,
                "`field` and `text` are mutually exclusive".into(),
            ));
        }
        if obj.get("charStyle").is_some() {
            return Err(options_err(
                path,
                "`field` and `charStyle` are mutually exclusive".into(),
            ));
        }
        return Ok(RunSpec {
            field: Some(field),
            ..RunSpec::text("")
        });
    }
    // 文本 run：text 必填，bold/italic/charStyle 可选（#196 任务 3：
    // charStyle = 字符样式引用——合法集 = office-io 预置字符样式
    // （Strong/Emphasis）∪ 引擎 define 的 Character 型（本 options 面
    // 无 styles 定义键——自定义面经 office-io API 消费方）；引用合法
    // 性双层：桥面解析 typed + 引擎 V12 save 漏斗粘性兜底）。
    let text = req_str(obj, path, "text")?;
    let char_style = match obj.get("charStyle") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .ok_or_else(|| options_err(path, format!("`charStyle` must be a string, got {v}")))?
                .to_string(),
        ),
    };
    Ok(RunSpec {
        bold: opt_bool(obj, path, "bold")?,
        italic: opt_bool(obj, path, "italic")?,
        char_style,
        ..RunSpec::text(text)
    })
}

/// P1 #190：runs 数组解析（`path` 定位到槽位——错误消息可指到
/// `header.first_page.runs[2]` 级）。
fn parse_hf_runs(v: Option<&serde_json::Value>, path: &str) -> Result<Vec<RunSpec>, ExportError> {
    let arr = v
        .ok_or_else(|| options_err(path, "missing required field `runs`".into()))?
        .as_array()
        .ok_or_else(|| options_err(path, "`runs` must be an array".into()))?;
    arr.iter()
        .enumerate()
        .map(|(i, r)| parse_hf_run(r, &format!("{path}.runs[{i}]")))
        .collect()
}

/// P1 #190：独立槽（first_page/even_page）解析——缺省/null/false = 不
/// 设置；`{"runs": [...]}` = 设置；裸 true 拒（runs 无从谈起——形状
/// 严格面）。
fn parse_hf_variant_slot(
    v: Option<&serde_json::Value>,
    path: &str,
) -> Result<Option<Vec<RunSpec>>, ExportError> {
    match v {
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => Ok(None),
        Some(serde_json::Value::Bool(true)) => Err(options_err(
            path,
            "bare `true` is not a valid shape (use {\"runs\": [...]} or false)".into(),
        )),
        Some(obj) => {
            let path = format!("{path}.runs");
            let o = obj
                .as_object()
                .ok_or_else(|| options_err(&path, "must be an object or false".into()))?;
            for key in o.keys() {
                if key != "runs" {
                    return Err(options_err(
                        &path,
                        format!("unknown field `{key}` (allowed: runs)"),
                    ));
                }
            }
            parse_hf_runs(o.get("runs"), &path).map(Some)
        }
    }
}

/// P1 #190：`header`/`footer` 顶层键 → [`HeaderFooterSpec`]（缺省/null
/// = None——零发射，字节稳定）。
fn parse_hf_slot(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<HeaderFooterSpec>, ExportError> {
    match obj.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => {
            let hf = v
                .as_object()
                .ok_or_else(|| options_err(key, "must be an object".into()))?;
            for k in hf.keys() {
                if !matches!(k.as_str(), "runs" | "first_page" | "even_page") {
                    return Err(options_err(
                        key,
                        format!("unknown field `{k}` (allowed: runs, first_page, even_page)"),
                    ));
                }
            }
            let default = parse_hf_runs(hf.get("runs"), &format!("{key}.runs"))?;
            let first = parse_hf_variant_slot(hf.get("first_page"), &format!("{key}.first_page"))?;
            let even = parse_hf_variant_slot(hf.get("even_page"), &format!("{key}.even_page"))?;
            Ok(Some(HeaderFooterSpec {
                default,
                first,
                even,
            }))
        }
    }
}

// ---------------------------------------------------------------- P1 #190：图片资源表

/// PNG 标准签名（RFC 2083——与 office-io `DocxWriter::image` 同源常量；
/// 本层前置验保证拒绝路径零副作用：资源表全部校验完才开始发射，
/// 引擎侧签名校验保留为双保险）。
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// P1 #190：图片资源（解码后字节 + 显式像素尺寸——office-io `image()`
/// 无尺寸推断面，要求显式 px；EMU 换算引擎内建 `from_px` = px × 9525
/// @96dpi）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSpec {
    pub png: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
}

/// P1 #190：图片资源表（src 键 → 资源；Markdown 图片行 `![alt](src)`
/// 的 src 命中表键即发射内嵌图片）。查表面（只 get 不遍历——序无关）。
pub type ImageTable = std::collections::HashMap<String, ImageSpec>;

/// 标准 base64 解码（RFC 4648 字母表 + padding——零三方依赖：
/// Cargo.lock 零 diff 纪律下的最小实现）。非法字符 / 缺 padding /
/// padding 后跟数据 → 错误消息（含字节偏移——调用方可定位）。URL-safe
/// 变体（`-`/`_`）不支持（JS 侧 `btoa` 产标准字母表）。
fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let s = s.trim();
    if !s.len().is_multiple_of(4) {
        return Err(format!(
            "length {} not a multiple of 4 (missing padding?)",
            s.len()
        ));
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut padding_seen = false;
    for (i, ch) in s.bytes().enumerate() {
        if ch == b'=' {
            padding_seen = true;
            continue;
        }
        if padding_seen {
            return Err(format!("data after padding at byte {i}"));
        }
        let v = ALPHABET
            .iter()
            .position(|&a| a == ch)
            .ok_or_else(|| format!("invalid character {:?} at byte {i}", ch as char))?
            as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// P1 #190：`images` 顶层键 → [`ImageTable`]（缺省/null = 空表——零
/// 命中，图片行全部降级为字面量文本，字节稳定）。每项 =
/// `{"png_base64": str, "width_px": 1.., "height_px": 1..}`：
/// base64 解码 / PNG 签名 / 尺寸正数全部**前置验**（`docx_options`
/// ——拒绝路径零副作用纪律：发射中途拒会留半成品状态面）。
fn parse_images(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<ImageTable, ExportError> {
    let mut table = ImageTable::new();
    let v = match obj.get("images") {
        None | Some(serde_json::Value::Null) => return Ok(table),
        Some(v) => v
            .as_object()
            .ok_or_else(|| options_err("images", "must be an object".into()))?,
    };
    for (src, spec) in v {
        let path = format!("images.{src}");
        let o = spec
            .as_object()
            .ok_or_else(|| options_err(&path, "must be an object".into()))?;
        for k in o.keys() {
            if !matches!(k.as_str(), "png_base64" | "width_px" | "height_px") {
                return Err(options_err(
                    &path,
                    format!("unknown field `{k}` (allowed: png_base64, width_px, height_px)"),
                ));
            }
        }
        let b64 = req_str(o, &path, "png_base64")?;
        let png = base64_decode(&b64)
            .map_err(|e| options_err(&path, format!("`png_base64` decode failed: {e}")))?;
        if png.len() < 8 || png[..8] != PNG_SIGNATURE {
            return Err(options_err(
                &path,
                format!(
                    "`png_base64` is not a PNG (signature mismatch, {} bytes)",
                    png.len()
                ),
            ));
        }
        let width_px = req_int(o, &path, "width_px", 1, u32::MAX as i64)? as u32;
        let height_px = req_int(o, &path, "height_px", 1, u32::MAX as i64)? as u32;
        table.insert(
            src.clone(),
            ImageSpec {
                png,
                width_px,
                height_px,
            },
        );
    }
    Ok(table)
}

/// P1 #190：Markdown 图片行解析（`![alt](src)` 整行形）。
///
/// v0 行级解析器（markdown.rs）把图片行落为**独立单行 Paragraph**，
/// text 原文保留字面量——图片信息已在树里，本函数在导出层识别
/// （零树改动的接线路径；BlockKind 无 image 变体的替代面）。
/// 严格形：行首 `![` + 恰一个 `](` + 行尾 `)`；alt 可空、src 非空
/// 且不含 `()`（CommonMark 内联图片语法的最小无歧义子集——嵌套
/// 括号形不识别，降级为文本行）。
fn image_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("![")?;
    let close = rest.find("](")?;
    let alt = &rest[..close];
    let src = rest[close + 2..].strip_suffix(')')?;
    if src.is_empty() || src.contains('(') || src.contains(')') || alt.contains(']') {
        return None;
    }
    Some((alt, src))
}

/// 解析后的导出格式选项（已构造为 office-io 公开类型——值域校验
/// 延迟到 save 漏斗 V9）。
pub struct StyleOptions {
    pub typography: Option<DefaultTypography>,
    pub page_geometry: Option<PageGeometry>,
    /// W-1：`Some` = 发射 TOC 域段（文档内容块之前）；`None` = 缺省/
    /// enabled=false（零发射，字节稳定）。
    pub toc: Option<TocOptions>,
    /// W-2：true = 产物过 [`anonymize_docx_props`]（暗标 G3/R-11）。
    pub anonymize: bool,
    /// P1 #190：页眉六槽位（None = 缺省——零发射，字节稳定）。
    pub header: Option<HeaderFooterSpec>,
    /// P1 #190：页脚六槽位（同上）。
    pub footer: Option<HeaderFooterSpec>,
    /// P1 #190：图片资源表（空表 = 零命中——图片行降级为字面量
    /// 文本，字节稳定）。
    pub images: ImageTable,
}

/// options JSON → [`StyleOptions`]（形状校验层；JsValue-free 原生可测）。
pub fn parse_style_options(json: &str) -> Result<StyleOptions, ExportError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| options_err("$", format!("invalid JSON ({e})")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| options_err("$", "must be an object".into()))?;
    for key in obj.keys() {
        if !matches!(
            key.as_str(),
            "typography" | "page_geometry" | "toc" | "anonymize" | "header" | "footer" | "images"
        ) {
            return Err(options_err(
                "$",
                format!(
                    "unknown field `{key}` (allowed: typography, page_geometry, toc, anonymize, header, footer, images)"
                ),
            ));
        }
    }

    let typography = match obj.get("typography") {
        None | Some(serde_json::Value::Null) => None,
        Some(t) => {
            let t = t
                .as_object()
                .ok_or_else(|| options_err("typography", "must be an object".into()))?;
            let ascii_font = req_str(t, "typography", "ascii_font")?;
            let east_asia_font = req_str(t, "typography", "east_asia_font")?;
            let complex_script_font = req_str(t, "typography", "complex_script_font")?;
            let size_half_points =
                req_int(t, "typography", "size_half_points", 0, u16::MAX as i64)? as u16;
            let line_spacing = match t.get("line_spacing") {
                None | Some(serde_json::Value::Null) => None,
                Some(ls) => {
                    let p = "typography.line_spacing";
                    let ls = ls
                        .as_object()
                        .ok_or_else(|| options_err(p, "must be an object".into()))?;
                    let line =
                        opt_int(ls, p, "line", i32::MIN as i64, i32::MAX as i64)?.map(|n| n as i32);
                    let line_rule = match ls.get("line_rule") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(s)) => match s.as_str() {
                            // 镜像 w:spacing w:lineRule 属性值（office-io
                            // LineRuleSpec 文档口径）。
                            "auto" => Some(LineRuleSpec::Auto),
                            "atLeast" => Some(LineRuleSpec::AtLeast),
                            "exact" => Some(LineRuleSpec::Exact),
                            other => {
                                return Err(options_err(
                                    p,
                                    format!(
                                        "`line_rule` must be auto|atLeast|exact, got {other:?}"
                                    ),
                                ))
                            }
                        },
                        Some(other) => {
                            return Err(options_err(
                                p,
                                format!("`line_rule` must be a string, got {other}"),
                            ))
                        }
                    };
                    let before = opt_int(ls, p, "before", 0, u32::MAX as i64)?.map(|n| n as u32);
                    let after = opt_int(ls, p, "after", 0, u32::MAX as i64)?.map(|n| n as u32);
                    Some(LineSpacingSpec {
                        line,
                        line_rule,
                        before,
                        after,
                    })
                }
            };
            Some(DefaultTypography {
                ascii_font,
                east_asia_font,
                complex_script_font,
                size_half_points,
                line_spacing,
            })
        }
    };

    let page_geometry = match obj.get("page_geometry") {
        None | Some(serde_json::Value::Null) => None,
        Some(g) => {
            let p = "page_geometry";
            let g = g
                .as_object()
                .ok_or_else(|| options_err(p, "must be an object".into()))?;
            Some(PageGeometry {
                width: req_int(g, p, "width", 0, u32::MAX as i64)? as u32,
                height: req_int(g, p, "height", 0, u32::MAX as i64)? as u32,
                margin_top: req_int(g, p, "margin_top", i32::MIN as i64, i32::MAX as i64)? as i32,
                margin_right: req_int(g, p, "margin_right", i32::MIN as i64, i32::MAX as i64)?
                    as i32,
                margin_bottom: req_int(g, p, "margin_bottom", i32::MIN as i64, i32::MAX as i64)?
                    as i32,
                margin_left: req_int(g, p, "margin_left", i32::MIN as i64, i32::MAX as i64)? as i32,
                margin_header: req_int(g, p, "margin_header", i32::MIN as i64, i32::MAX as i64)?
                    as i32,
                margin_footer: req_int(g, p, "margin_footer", i32::MIN as i64, i32::MAX as i64)?
                    as i32,
                gutter: req_int(g, p, "gutter", i32::MIN as i64, i32::MAX as i64)? as i32,
            })
        }
    };

    // W-1：TOC 目录域开关（enabled 必填；层级缺省 1/3——层级域 1-9
    // + from ≤ to 本层前置 typed 拒，引擎 V8 兑底不可达）。
    let toc = match obj.get("toc") {
        None | Some(serde_json::Value::Null) => None,
        Some(t) => {
            let t = t
                .as_object()
                .ok_or_else(|| options_err("toc", "must be an object".into()))?;
            if !req_bool(t, "toc", "enabled")? {
                None
            } else {
                let level_from = opt_toc_level(t, "level_from")?.unwrap_or(1);
                let level_to = opt_toc_level(t, "level_to")?.unwrap_or(3);
                if level_from > level_to {
                    return Err(options_err(
                        "toc",
                        format!(
                            "`level_from` ({level_from}) > `level_to` ({level_to}) \
                             (TOC heading levels must satisfy from ≤ to)"
                        ),
                    ));
                }
                Some(TocOptions {
                    level_from,
                    level_to,
                })
            }
        }
    };

    // W-2：匿名化开关（缺省 false = 引擎产物直通——字节稳定）。
    let anonymize = match obj.get("anonymize") {
        None | Some(serde_json::Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| {
            options_err(
                "anonymize",
                format!("`anonymize` must be a boolean, got {v}"),
            )
        })?,
    };

    // P1 #190：页眉/页脚六槽位（缺省 = 零发射——字节稳定）。
    let header = parse_hf_slot(obj, "header")?;
    let footer = parse_hf_slot(obj, "footer")?;

    // P1 #190：图片资源表（缺省 = 空表——零命中，字节稳定）。
    let images = parse_images(obj)?;

    Ok(StyleOptions {
        typography,
        page_geometry,
        toc,
        anonymize,
        header,
        footer,
        images,
    })
}

/// 编辑器核心块树 + 格式选项 → DOCX 字节。选项消费 = 写器两设置面
/// （几何/排版）+ P1 #190 页眉/页脚六槽位 + W-1 TOC 域段（内容块
/// 之前）+ W-2 匿名化后处理；值域违规经 save 漏斗 V9 拒（错误码
/// `docx_invalid_input`）——形状违规 [`parse_style_options`] 已拦。
pub fn export_docx_styled_core(
    tree: &BlockTree,
    options_json: &str,
) -> Result<Vec<u8>, ExportError> {
    let opts = parse_style_options(options_json)?;
    let mut w = DocxWriter::new();
    if let Some(g) = opts.page_geometry {
        w.page_geometry(g);
    }
    if let Some(t) = opts.typography {
        w.default_typography(t);
    }
    // P1 #190：页眉/页脚六槽位（office-io builder 面——section_property
    // 级设置，与段落序无关；固定发射序 = 字节稳定）。发射先于 TOC 与
    // 内容块（页眉页脚是页面装置面，非文档体序面）。
    if let Some(hf) = &opts.header {
        w.header(&hf.default);
        if let Some(first) = &hf.first {
            w.first_header(first);
        }
        if let Some(even) = &hf.even {
            w.even_header(even);
        }
    }
    if let Some(hf) = &opts.footer {
        w.footer(&hf.default);
        if let Some(first) = &hf.first {
            w.first_footer(first);
        }
        if let Some(even) = &hf.even {
            w.even_footer(even);
        }
    }
    // W-1：TOC 域段居文档开头（内容块发射之前——Word 惯例：目录在
    // 正文前）。#196 任务 2：静态目录项升级——每个收录标题一项
    // （hyperlink 跳转 + PAGEREF 页码域），正文标题挂对应书签
    // （配对表查表发射）；无收录标题（空文档/全域外）退化为旧占位
    // 形态（office-io toc_entries 空退化）。既有消费者（toc 未启用）
    // 字节零变化。
    let mut toc_marks: TocMarkMap = TocMarkMap::new();
    if let Some(toc) = opts.toc {
        let mut entries: Vec<TocEntry> = Vec::new();
        let mut positions: Vec<(NodeId, usize)> = Vec::new();
        for &child in tree.top_level() {
            collect_toc_entries(
                tree,
                child,
                toc.level_from,
                toc.level_to,
                &mut entries,
                &mut positions,
            );
        }
        let names = w.toc_entries(&entries, Some((toc.level_from, toc.level_to)));
        for (pos, name) in positions.into_iter().zip(names) {
            toc_marks.insert(pos, name);
        }
    }
    for &child in tree.top_level() {
        emit_block(tree, child, &mut w, 0, &opts.images, &toc_marks)?;
    }
    let bytes = w.save_to_vec().map_err(|e| map_error(&e))?;
    // W-2：匿名化后处理（暗标 G3/R-11——docProps 元数据清零，正文
    // 部件原样拷贝；引擎新建面已匿名[单一真源]，本面 = 显式再洗
    // [幂等] + 产品开关语义）。
    if opts.anonymize {
        return anonymize_docx_props(&bytes).map_err(|e| map_error(&e));
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// N-5（issue #29）：分页预估——与导出同形 options 的预估面。
//
// 设计（与 [`export_docx_styled_core`] 同一 options 语法 = 单一形状
// 真源）：`typography`/`page_geometry` 映射 [`mdcore::pagination`]
// 平面参数（twips/倍数）；`toc`/`anonymize` 键合法但不参与估算
// （目录域 = 渲染期生成面[打开者 F9]，页数贡献不可预估——文档登记；
// 匿名化 = 元数据面零页数影响）。缺省 options ≡ 引擎默认档
// （A4 + 边距 1985/1701 + 10pt 单倍——与不传选项导出基线一致）。
// ---------------------------------------------------------------------------

/// office-io 排版/几何类型 → [`PaginationParams`]（twips/倍数平面——
/// 三域纪律：核心零 office-io 依赖，映射在桥）。
fn pagination_params_from_style(
    typography: Option<&DefaultTypography>,
    geometry: Option<&PageGeometry>,
) -> mdcore::pagination::PaginationParams {
    // 缺省 ≡ 引擎导出基线（不传选项的产物几何/排版）。
    let g = geometry.cloned().unwrap_or_else(PageGeometry::a4_default);
    let text_width = g
        .width
        .saturating_sub(g.margin_left.max(0) as u32)
        .saturating_sub(g.margin_right.max(0) as u32)
        .saturating_sub(g.gutter.max(0) as u32);
    let text_height = g
        .height
        .saturating_sub(g.margin_top.max(0) as u32)
        .saturating_sub(g.margin_bottom.max(0) as u32);
    let (body_size_twips, line_multiple, spacing_before, spacing_after) = match typography {
        None => (200, 1.0, 0, 0),
        Some(t) => {
            let size = t.size_half_points as u32 * 10;
            let (multiple, before, after) = match &t.line_spacing {
                None => (1.0, 0, 0),
                Some(spec) => {
                    let before = spec.before.unwrap_or(0);
                    let after = spec.after.unwrap_or(0);
                    let multiple = match (spec.line, spec.line_rule) {
                        // auto：line = 倍数 × 240。
                        (Some(line), Some(LineRuleSpec::Auto)) | (Some(line), None) => {
                            line as f64 / 240.0
                        }
                        // atLeast/exact：line = twips——折算为等效倍数
                        //（行高 = 字号 × (A + B×倍数) = line，单倍基
                        // = A + B）。
                        (Some(line), Some(rule)) => {
                            let single_base =
                                mdcore::pagination::BODY_LINE_A + mdcore::pagination::BODY_LINE_B;
                            let equiv = line as f64 / (size as f64 * single_base);
                            let _ = rule;
                            equiv
                        }
                        (None, _) => 1.0,
                    };
                    (multiple, before, after)
                }
            };
            (size, multiple, before, after)
        }
    };
    mdcore::pagination::PaginationParams {
        text_width,
        text_height,
        body_size_twips,
        line_multiple,
        spacing_before,
        spacing_after,
    }
}

/// 编辑器核心块树 + 格式选项 JSON → 分页预估 JSON（N-5 预估面——
/// options 语法与 [`export_docx_styled_core`] 同一：调用方以「将要
/// 导出的同一 options」预估，口径天然对齐）。失败 = options 形状
/// 错（复用 [`parse_style_options`]——单一形状真源）。
pub fn estimate_pages_core(tree: &BlockTree, options_json: &str) -> Result<String, ExportError> {
    let opts = parse_style_options(options_json)?;
    let params =
        pagination_params_from_style(opts.typography.as_ref(), opts.page_geometry.as_ref());
    let est = mdcore::pagination::estimate(tree, &params);
    serde_json::to_string(&est).map_err(|e| ExportError {
        code: "docx_build",
        message: format!("estimate serialize: {e}"),
    })
}

// ---------------------------------------------------------------------------
// wasm 壳（WasmCore 增量方法——既有类面零变化，t16/t17 自由函数模式的
// 差异 = 导出读 self 状态，故为方法；本 crate 内 impl 分文件合法）
// ---------------------------------------------------------------------------

#[wasm_bindgen]
impl WasmCore {
    /// 当前文档 → DOCX 字节（JS `Uint8Array`）。失败 = 抛 JSON 串
    /// `{"code":"docx_build"|"docx_limit","message":"..."}`
    /// （t16/t17 错误面约定 + r4-t2 资源限码）。
    #[wasm_bindgen]
    pub fn export_docx(&self) -> Result<Vec<u8>, JsValue> {
        export_docx_core(&self.inner.tree).map_err(|e| {
            JsValue::from_str(
                &serde_json::json!({ "code": e.code, "message": e.message }).to_string(),
            )
        })
    }

    /// 当前文档 + 格式选项 JSON → DOCX 字节（issue #10 / TR-4：G2 格式
    /// 面消费——排版[字体三槽/半磅字号/行距]与页面几何[twips 全八字段]；
    /// W-1/W-2 增量：`toc` 目录域开关 + `anonymize` 匿名化开关）。
    /// 失败 = 抛 JSON 串
    /// `"code": "docx_options"|"docx_invalid_input"|"docx_limit"|"docx_build"`。
    /// 选项字段镜像 office-io 公开 API（snake_case 1:1）；`typography`/
    /// `page_geometry` 子字段必填（显式全量状态语义）；`toc`/
    /// `anonymize` 可选（缺省 ≡ 零发射/零后处理）。空对象 `{}` ≡
    /// [`Self::export_docx`]（字节级）。
    #[wasm_bindgen]
    pub fn export_docx_styled(&self, options_json: &str) -> Result<Vec<u8>, JsValue> {
        export_docx_styled_core(&self.inner.tree, options_json).map_err(|e| {
            JsValue::from_str(
                &serde_json::json!({ "code": e.code, "message": e.message }).to_string(),
            )
        })
    }

    /// 当前文档 + 格式选项 JSON → 分页预估 JSON（N-5：导出前超页
    /// 判断——「技术标不超过 N 页」硬限制的事前检查）。options 语法
    /// 与 [`Self::export_docx_styled`] 同一（以将要导出的同一 options
    /// 预估，口径天然对齐）。返回字段：`pages`/`chars_per_line`/
    /// `lines_per_page`/分块行消耗/`error_margin_pct`（模型自报
    /// 误差带）。失败 = 抛 JSON 串（code = `docx_options`）。
    #[wasm_bindgen]
    pub fn estimate_pages_json(&self, options_json: &str) -> Result<String, JsValue> {
        estimate_pages_core(&self.inner.tree, options_json).map_err(|e| {
            JsValue::from_str(
                &serde_json::json!({ "code": e.code, "message": e.message }).to_string(),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// 原生测试（写 → read_docx_from_bytes → 模型断言；t16/t17 写读全环纪律）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_office_io::docx::{
        Block, HeaderFooterData, HeaderFooterVariant, ParagraphData, TableData,
    };
    use mdcore::markdown::markdown_to_tree;

    /// 直构单块树（marks/attrs 精确控制——op/html 导入面的模型等价物）。
    fn single(kind: BlockKind, text: &str, attrs: Attrs, marks: Vec<MarkSpan>) -> BlockTree {
        let mut tree = BlockTree::new();
        let id = tree.create_node(kind, text, attrs, marks);
        let root = tree.root();
        let len = tree.node(root).children.len();
        tree.insert_child(root, len, id).expect("test attach");
        tree
    }

    fn span(start: usize, end: usize, kind: &str) -> MarkSpan {
        MarkSpan {
            start,
            end,
            kind: kind.into(),
        }
    }

    fn attr(k: &str, v: &str) -> Attrs {
        let mut a = Attrs::new();
        a.insert(k.into(), v.into());
        a
    }

    /// 写→读全环：字节必须过 office-io 读面（容器门 + 模型解析）。
    fn roundtrip(tree: &BlockTree) -> Vec<Block> {
        let bytes = export_docx_core(tree).expect("export");
        assert!(
            bytes.len() > 4 && &bytes[..2] == b"PK",
            "zip local header magic, {} bytes",
            bytes.len()
        );
        read_docx_from_bytes(&bytes).expect("read back").blocks
    }

    fn paras(blocks: &[Block]) -> Vec<ParagraphData> {
        blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.clone()),
                Block::Table(_) | Block::TextBox(_) => None,
            })
            .collect()
    }

    /// (style, text, bold, italic) 四元组平铺——run 级断言便捷面。
    fn para_tuples(p: &ParagraphData) -> (Option<String>, Vec<(String, bool, bool)>) {
        (
            p.style.clone(),
            p.runs
                .iter()
                .map(|r| (r.text.clone(), r.bold, r.italic))
                .collect(),
        )
    }

    /// zip 本地头遍历取部件（t20 XML 面断言辅助）：docx-rs 打包恒
    /// `CompressionMethod::Stored` 无压缩（zipper/mod.rs:80-81）→ 本地
    /// 头记录的 size 即数据长度，直接切片零解压。office-io zip 依赖
    /// 不进 mdc-wasm（Cargo.lock 零 diff 纪律）——本遍历为测试面专用
    /// 最小实现（PK\x03\x04 签名步进：偏移 18/22 尺寸、26/28 名长）。
    fn zip_stored_part(bytes: &[u8], name: &str) -> String {
        let mut i = 0usize;
        while i + 30 <= bytes.len() {
            if &bytes[i..i + 4] != b"PK\x03\x04" {
                break; // 中央目录/EOCD 起点或非 zip——终止
            }
            let size =
                u32::from_le_bytes([bytes[i + 18], bytes[i + 19], bytes[i + 20], bytes[i + 21]])
                    as usize;
            let name_len = u16::from_le_bytes([bytes[i + 26], bytes[i + 27]]) as usize;
            let extra_len = u16::from_le_bytes([bytes[i + 28], bytes[i + 29]]) as usize;
            let entry_name =
                String::from_utf8_lossy(&bytes[i + 30..i + 30 + name_len]).into_owned();
            let data_start = i + 30 + name_len + extra_len;
            if entry_name == name {
                return String::from_utf8_lossy(&bytes[data_start..data_start + size]).into_owned();
            }
            i = data_start + size;
        }
        panic!("part {name} not found");
    }

    /// 导出字节 → document.xml（XML 面断言便捷面）。
    fn doc_xml(tree: &BlockTree) -> String {
        let bytes = export_docx_core(tree).expect("export");
        zip_stored_part(&bytes, "word/document.xml")
    }

    #[test]
    fn headings_map_to_styles_with_levels() {
        let tree = markdown_to_tree("# A\n## B\n###### F\nplain");
        let blocks = roundtrip(&tree);
        let ps = paras(&blocks);
        assert_eq!(ps.len(), 4, "3 headings + 1 paragraph");
        assert_eq!(ps[0].style.as_deref(), Some("Heading1"));
        assert_eq!(ps[1].style.as_deref(), Some("Heading2"));
        assert_eq!(ps[2].style.as_deref(), Some("Heading6"));
        assert_eq!(ps[3].style, None, "plain paragraph = no style");
        for (p, t) in ps.iter().zip(["A", "B", "F", "plain"]) {
            assert_eq!(p.text(), t);
        }
    }

    #[test]
    fn heading_level_default_and_clamp() {
        let over = single(BlockKind::Heading, "C", attr("level", "42"), vec![]);
        let bad = single(BlockKind::Heading, "D", attr("level", "abc"), vec![]);
        let none = single(BlockKind::Heading, "E", Attrs::new(), vec![]);
        let blocks = roundtrip(&over);
        assert_eq!(
            paras(&blocks)[0].style.as_deref(),
            Some("Heading9"),
            ">9 钳 9"
        );
        for tree in [&bad, &none] {
            let blocks = roundtrip(tree);
            assert_eq!(
                paras(&blocks)[0].style.as_deref(),
                Some("Heading1"),
                "不可解析/缺失 level → 1"
            );
        }
    }

    #[test]
    fn bold_italic_marks_roundtrip() {
        let tree = single(
            BlockKind::Paragraph,
            "hello bold world",
            Attrs::new(),
            vec![span(6, 10, "strong"), span(11, 16, "em")],
        );
        let ps = paras(&roundtrip(&tree));
        let (style, runs) = para_tuples(&ps[0]);
        assert_eq!(style, None);
        assert_eq!(
            runs,
            vec![
                ("hello ".into(), false, false),
                ("bold".into(), true, false),
                (" ".into(), false, false),
                ("world".into(), false, true),
            ],
            "char 偏移分段：plain / bold / 间隔空格 / italic"
        );
    }

    #[test]
    fn mark_adjacent_merge_and_overlap() {
        // strong[0,3]+strong[3,6] 邻接同式合并；strong[0,6] 与 em[3,9] 交叠三分
        let merged = single(
            BlockKind::Paragraph,
            "abcdef",
            Attrs::new(),
            vec![span(0, 3, "strong"), span(3, 6, "strong")],
        );
        let (_, runs) = para_tuples(&paras(&roundtrip(&merged))[0]);
        assert_eq!(
            runs,
            vec![("abcdef".into(), true, false)],
            "邻接同样式并 1 run"
        );

        let overlap = single(
            BlockKind::Paragraph,
            "abcdefghij",
            Attrs::new(),
            vec![span(0, 6, "strong"), span(3, 9, "em")],
        );
        let (_, runs) = para_tuples(&paras(&roundtrip(&overlap))[0]);
        assert_eq!(
            runs,
            vec![
                ("abc".into(), true, false),
                ("def".into(), true, true),
                ("ghi".into(), false, true),
                ("j".into(), false, false),
            ],
            "交叠 = 四段（bold / bold+italic / italic / 越界尾 plain）"
        );
    }

    /// t20 翻转（原 t18 `unsupported_inline_marks_degrade_to_plain`）：
    /// code/del/u 三 mark 降级已消解——XML 面直接断言（rFonts/strike/u）
    /// 与文本面回归（拼接零损；读模型 [`RunData`] 无字体/删除线/下画线
    /// 载荷——读面 run 分段不宣称往返等值，登记见模块文档）；
    /// link 维持纯文本（模型无 URL 载荷——上游缺口非降级）。
    #[test]
    fn inline_mono_del_u_expressed_link_plain() {
        let tree = single(
            BlockKind::Paragraph,
            "mono del u link",
            Attrs::new(),
            vec![
                span(0, 4, "code"),
                span(5, 8, "del"),
                span(0, 15, "u"),
                span(5, 8, "link"),
            ],
        );
        // XML 面：code → rFonts 三槽 Consolas；del → w:strike；u → w:u single。
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas" />"#)
                .count(),
            1,
            "code 段等宽面"
        );
        assert_eq!(xml.matches("<w:strike />").count(), 1, "del 段删除线");
        assert_eq!(
            xml.matches(r#"<w:u w:val="single" />"#).count(),
            4,
            "u 全段下画线（四 run 逐段）"
        );
        // 文本面：拼接零损；读模型不分段承载新位（四 run 均无 bold/italic）。
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps[0].text(), "mono del u link", "文本零损");
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![
                ("mono".into(), false, false),
                (" ".into(), false, false),
                ("del".into(), false, false),
                (" u link".into(), false, false),
            ],
            "读模型无新位载荷——分段可见但不宣称往返等值（登记）"
        );
    }

    // -----------------------------------------------------------------
    // #15（A3/P0）：行内 marks 消费——markdown 源文本层在发射面派生。
    // 产品路径（markdown_to_tree → export）此前把 `**` 原样输出进
    // docx（TR-2D P0 复现：星号原样进 Word）。以下经 effective_inline
    // 漏斗钉定：定界符剥除 + run 级格式 + 零星号残留。
    // -----------------------------------------------------------------

    /// #15 验收主用例（TR-2D W4 复现形）：加粗 + 斜体同行，零星号。
    #[test]
    fn tr15_bold_italic_from_markdown_source() {
        let tree = markdown_to_tree("总计人民币 **壹佰贰拾万元整**，*非最终报价*。");
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps.len(), 1);
        let (style, runs) = para_tuples(&ps[0]);
        assert_eq!(style, None);
        assert_eq!(
            runs,
            vec![
                ("总计人民币 ".into(), false, false),
                ("壹佰贰拾万元整".into(), true, false),
                ("，".into(), false, false),
                ("非最终报价".into(), false, true),
                ("。".into(), false, false),
            ],
            "加粗/斜体 run 化；零星号残留（净文本断言）"
        );
    }

    /// #15 五位 marks 全谱（XML 面——读模型仅 bold/italic 两位承载）：
    /// code（Consolas run 通道）/del（w:strike）/u（w:u single）。
    #[test]
    fn tr15_code_del_underline_xml_emission() {
        let tree = markdown_to_tree("等宽 `f(x)`、删除 ~~旧条款~~、下划线 <u>新条款</u>。");
        let xml = doc_xml(&tree);
        // run 级 Consolas（code mark → mono——runs_from 五位映射）。
        assert!(
            xml.contains(r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas" />"#),
            "code mark → run 级等宽 rFonts"
        );
        assert_eq!(xml.matches("<w:strike />").count(), 1, "del → 删除线");
        assert_eq!(
            xml.matches(r#"<w:u w:val="single" />"#).count(),
            1,
            "u → 下画线"
        );
        // 净文本零定界符残留（反引号/波浪线/标签全剥）。
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps[0].text(), "等宽 f(x)、删除 旧条款、下划线 新条款。");
    }

    /// #15 未闭合定界符按字面量（不猜测）——issue 验收要求 4。
    #[test]
    fn tr15_unclosed_markers_stay_literal() {
        let tree = markdown_to_tree("总计人民币 **壹佰贰拾万元整。");
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps[0].text(), "总计人民币 **壹佰贰拾万元整。");
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(runs.len(), 1, "单 run 纯文本——无格式派生");
        assert!(!runs[0].1, "未闭合 → 无加粗");
        // 转义形：`\*` 字面量星号不触发配对。
        let tree2 = markdown_to_tree("转义 \\*形\\* 不加粗");
        let ps2 = paras(&roundtrip(&tree2));
        assert_eq!(ps2[0].text(), "转义 *形* 不加粗");
        assert!(!ps2[0].runs.iter().any(|r| r.bold || r.italic));
    }

    /// #15 ListItem / Blockquote / Heading 三面消费：
    /// 列表项与引用段 marks 表达；标题定界符剥除（格式丢弃——映射表）。
    #[test]
    fn tr15_list_quote_heading_consume_inline() {
        let tree = markdown_to_tree("- **重点**：说明\n> 引用 **强调** 文本\n# **报价** 章节");
        let ps = paras(&roundtrip(&tree));
        // 三段：列表项 / 引用段 / 标题段。
        assert_eq!(ps.len(), 3, "列表项 + 引用段 + 标题段");
        // 列表项（numPr 段 + 加粗 run）。
        let (s0, r0) = para_tuples(&ps[0]);
        assert!(s0.is_none(), "列表段走 numPr 非样式通道");
        assert_eq!(
            r0,
            vec![
                ("重点".into(), true, false),
                ("：说明".into(), false, false),
            ],
            "列表项行内加粗表达"
        );
        // 引用段。
        let (_, r1) = para_tuples(&ps[1]);
        assert_eq!(
            r1,
            vec![
                ("引用 ".into(), false, false),
                ("强调".into(), true, false),
                (" 文本".into(), false, false),
            ],
            "引用段行内加粗表达"
        );
        // 标题：净文本（定界符剥除、格式丢弃——DocxWriter::heading
        // 纯文本面，映射表登记）。
        assert_eq!(ps[2].style.as_deref(), Some("Heading1"));
        assert_eq!(ps[2].text(), "报价 章节", "标题定界符剥除、零星号");
    }

    /// #15 优先序钉定：node.marks 非空（op/html 程序化覆盖）→ 覆盖赢，
    /// 原始文本（含字面定界符）原样透传，不二次解析。
    #[test]
    fn tr15_op_layer_marks_precedence_over_derivation() {
        let tree = single(
            BlockKind::Paragraph,
            "**保持原样**",
            Attrs::new(),
            vec![span(0, 10, "strong")],
        );
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps[0].text(), "**保持原样**", "覆盖路径文本原样（含定界符）");
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![("**保持原样**".into(), true, false)],
            "op 层 marks 生效；无二次解析（星号保留在文本中）"
        );
    }

    /// #15 嵌套叠加（runs_from 五位覆盖集）：`**a *b* c**` → 全程加粗
    /// + 内层斜体。
    #[test]
    fn tr15_nesting_overlay_bold_plus_italic() {
        let tree = markdown_to_tree("**总则 *细则* 附则**");
        let ps = paras(&roundtrip(&tree));
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![
                ("总则 ".into(), true, false),
                ("细则".into(), true, true),
                (" 附则".into(), true, false),
            ],
            "嵌套经 mark 交叠叠加：内层双格式 run"
        );
    }

    /// #15 全环闭环（TR-2D 断链修复）：t16 导入 bold run → md `**` →
    /// 模型源文本 → 导出 bold run——docx→md→docx 格式往返成立。
    #[test]
    fn tr15_docx_md_docx_bold_roundtrip() {
        // 直接以导入面产物形态（md 含 `**`）构造——与 __importDocx 后
        // __inject 重建核的产品链等价（docx_import 已钉定 bold→`**`）。
        let tree = markdown_to_tree("本项目含**智能文档平台**与*需求解析*两项内容。");
        let ps = paras(&roundtrip(&tree));
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![
                ("本项目含".into(), false, false),
                ("智能文档平台".into(), true, false),
                ("与".into(), false, false),
                ("需求解析".into(), false, true),
                ("两项内容。".into(), false, false),
            ],
            "TR-2D W4 复现文本：星号零残留 + 双格式 run"
        );
        assert!(!ps[0].text().contains('*'), "净文本零星号");
    }

    #[test]
    fn table_roundtrip_escaped_pipe_and_separator_drop() {
        let tree = markdown_to_tree("| a | b \\| c |\n| --- | --- |\n| 1 | 2 |");
        let blocks = roundtrip(&tree);
        let tables: Vec<TableData> = blocks
            .into_iter()
            .filter_map(|b| match b {
                Block::Table(t) => Some(t),
                Block::Paragraph(_) | Block::TextBox(_) => None,
            })
            .collect();
        assert_eq!(tables.len(), 1);
        assert_eq!(
            tables[0].rows,
            vec![
                vec!["a".to_string(), "b | c".to_string()],
                vec!["1".to_string(), "2".to_string()],
            ],
            "分隔行剔除 + \\| 反转义"
        );
    }

    #[test]
    fn code_block_degrades_to_plain_paragraphs() {
        let tree = markdown_to_tree("```rust\nfn main() {}\nlet x = 1;\n```");
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps.len(), 2);
        for (p, t) in ps.iter().zip(["fn main() {}", "let x = 1;"]) {
            assert_eq!(
                p.style, None,
                "lang 丢弃 → 无样式段（等宽面已表达但非样式通道）"
            );
            assert_eq!(p.text(), t);
        }
    }

    /// t20：块级 code 双通道等宽——XML 面直断言（段落级 pPr>rPr>rFonts
    /// 与每 run rFonts，Courier New）；文本面回归 = 既有
    /// `code_block_degrades_to_plain_paragraphs`（读模型无字体面）。
    #[test]
    fn code_block_mono_dual_channel_xml() {
        let tree = markdown_to_tree("```rust\nfn main() {}\nlet x = 1;\n```");
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(
                r#"<w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" />"#
            )
            .count(),
            2,
            "段落级通道 ×2 行"
        );
        assert_eq!(
            xml.matches(
                r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" />"#
            )
            .count(),
            2,
            "run 级通道 ×2 行（每行单 run）"
        );
        // 空行 code 块：空段也走双通道（行级哲学零例外）。
        let empty = markdown_to_tree("```\n\n```");
        let xml = doc_xml(&empty);
        assert!(xml.contains(r#"<w:pPr><w:rPr><w:rFonts w:ascii="Courier New""#));
    }

    /// t20：列表结构消解——marker 判序（数字型→Ordered，`-`→Bullet）
    /// + depth→ilvl；numPr 精确配对 + numbering.xml 双抽象（XML 面）
    /// + 文本面回归（次序保持）。
    #[test]
    fn list_mapping_numbering_xml() {
        let tree = markdown_to_tree("1. a\n2. b\n- c\n  - nested");
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="100" /><w:ilvl w:val="0" />"#)
                .count(),
            2,
            "有序 ilvl=0 ×2"
        );
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="0" />"#)
                .count(),
            1,
            "无序 ilvl=0 ×1"
        );
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="1" />"#)
                .count(),
            1,
            "嵌套无序 ilvl=1 ×1（depth 2 → ilvl 1）"
        );
        // 文本面：次序保持。
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["a", "b", "c", "nested"]);
    }

    /// t20：marker 形态面 + depth 钳位（直构 attrs——`1)` 形 / 深度越界
    /// / 缺失防御缺省）。
    #[test]
    fn list_marker_forms_and_depth_clamp() {
        // `1)` 形 → Ordered。
        let mut attrs = Attrs::new();
        attrs.insert("marker".into(), "1)".into());
        attrs.insert("depth".into(), "1".into());
        let paren = single(BlockKind::ListItem, "p", attrs, vec![]);
        let xml = doc_xml(&paren);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="100" /><w:ilvl w:val="0" />"#)
                .count(),
            1,
            "`1)` 形数字 marker → Ordered"
        );
        // depth 99 → ilvl 钳 8（映射层全函数，不报错）。
        let mut attrs = Attrs::new();
        attrs.insert("marker".into(), "-".into());
        attrs.insert("depth".into(), "99".into());
        let deep = single(BlockKind::ListItem, "d", attrs, vec![]);
        let xml = doc_xml(&deep);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="8" />"#)
                .count(),
            1,
            "depth 99 → ilvl 钳 8"
        );
        // attrs 全缺失 → Bullet + ilvl 0（防御缺省）。
        let bare = single(BlockKind::ListItem, "x", Attrs::new(), vec![]);
        let xml = doc_xml(&bare);
        assert_eq!(
            xml.matches(r#"<w:numId w:val="101" /><w:ilvl w:val="0" />"#)
                .count(),
            1,
            "缺失 attrs → Bullet ilvl=0"
        );
    }

    /// t22 翻转（原 t20 `list_multiline_item_first_numbered_rest_plain`
    /// 平铺断言）：多行 item 续行缩进已表达——一个列表项 = 一个 OOXML
    /// 段落（numPr 单点 + 行间 w:br + 段落级 hanging ind，续行与首行
    /// 文字列对齐）。读面 br → `\n`（t16 约定）→ 单段文本零损；列表
    /// kind 往返等值仍不宣称（读模型无 numPr——登记维持）。
    /// CRLF：行尾 `\r` 在发射面剥除（rich_lines），br 替位 `\n`。
    #[test]
    fn list_multiline_item_single_paragraph_br_hanging() {
        let tree = single(
            BlockKind::ListItem,
            "head line\ntail line",
            attr("marker", "-"),
            vec![],
        );
        // XML 面：单段 numPr（邻接串 = 同段性）+ br 软换行 + 段落级
        // hanging ind（left=720、hanging=360——与编号抽象层级同值）。
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(r#"<w:numPr><w:numId w:val="101" /><w:ilvl w:val="0" /></w:numPr><w:ind w:left="720" w:right="0" w:hanging="360" />"#)
                .count(),
            1,
            "多行项 = 单段（numPr→ind 邻接）"
        );
        assert_eq!(
            xml.matches(r#"<w:br w:type="textWrapping" />"#).count(),
            1,
            "两行一 br"
        );
        // 文本面：读回单段含 `\n`（br→\n），非平铺两段。
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["head line\ntail line"]);

        // CRLF 容忍："x\r\ny" → br 单段（\r 剥除）。
        let crlf = single(BlockKind::ListItem, "x\r\ny", attr("marker", "-"), vec![]);
        let ps = paras(&roundtrip(&crlf));
        assert_eq!(ps.len(), 1, "CRLF 多行同为单段");
        assert_eq!(ps[0].text(), "x\ny");

        // 单行项零漂移对照：无段落级 ind（既有 list_item 面）。
        let single_line = single(BlockKind::ListItem, "solo", attr("marker", "-"), vec![]);
        let xml = doc_xml(&single_line);
        assert_eq!(
            xml.matches(
                r#"<w:numPr><w:numId w:val="101" /><w:ilvl w:val="0" /></w:numPr></w:pPr>"#
            )
            .count(),
            1,
            "单行项 pPr 止于 numPr（无 ind）"
        );
        assert_eq!(xml.matches("<w:br").count(), 0, "单行无 br");
    }

    /// t22：嵌套多行 item（depth 2 → ilvl 1）——ind 步进 1440、
    /// hanging 同值 360；三行（含空行）→ br ×2；行内 marks 跨行
    /// 平移保持（rich_lines 单一真源——多行列表路径接线验证）。
    #[test]
    fn list_multiline_item_nested_depth_and_marks() {
        // "top\n\nbottom"：中空行；strong 覆盖第二行前 4 char。
        let mut attrs = Attrs::new();
        attrs.insert("marker".into(), "1.".into());
        attrs.insert("depth".into(), "2".into());
        let tree = single(
            BlockKind::ListItem,
            "top\n\nbottom",
            attrs,
            vec![span(5, 9, "strong")],
        );
        let xml = doc_xml(&tree);
        assert_eq!(
            xml.matches(
                r#"<w:numPr><w:numId w:val="100" /><w:ilvl w:val="1" /></w:numPr><w:ind w:left="1440" w:right="0" w:hanging="360" />"#
            )
            .count(),
            1,
            "depth 2 → ilvl 1：ind left = 720×(1+1) = 1440"
        );
        assert_eq!(
            xml.matches(r#"<w:br w:type="textWrapping" />"#).count(),
            2,
            "三行（空行居中）→ br ×2"
        );
        // 文本面：单段 "top\n\nbottom"；第二行 strong 平移（bold run）。
        let ps = paras(&roundtrip(&tree));
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text(), "top\n\nbottom");
        let (_, runs) = para_tuples(&ps[0]);
        assert_eq!(
            runs,
            vec![
                ("top".into(), false, false),
                ("\n".into(), false, false),
                ("\n".into(), false, false),
                ("bott".into(), true, false),
                ("om".into(), false, false),
            ],
            "strong[5,9] → 第二行 [0,4]（char 平移），br run 读为独立 \\n"
        );
    }

    /// t20：引用面 + hr 消解——ind 720×depth 嵌套步进 + 左边框 +
    /// 空段下边框（XML 面）+ 文本面回归（次序保持）。
    #[test]
    fn quote_and_hr_mapping_xml() {
        let tree = markdown_to_tree("> q\n>> deep\n---");
        let xml = doc_xml(&tree);
        assert!(xml.contains(r#"w:ind w:left="720""#), "引用 depth 1");
        assert!(
            xml.contains(r#"w:ind w:left="1440""#),
            "嵌套引用 depth 2 步进"
        );
        assert_eq!(
            xml.matches(
                r#"<w:pBdr><w:left w:val="single" w:space="4" w:sz="12" w:color="auto" /></w:pBdr>"#
            )
            .count(),
            2,
            "两引用段各带左边框"
        );
        assert!(
            xml.contains(
                r#"<w:pBdr><w:bottom w:val="single" w:space="1" w:sz="6" w:color="auto" /></w:pBdr>"#
            ),
            "hr 空段下边框"
        );
        // 文本面：次序保持（hr = 空段）。
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["q", "deep", ""]);
    }

    #[test]
    fn quote_list_divider_flatten_in_order() {
        let tree = markdown_to_tree("> quoted\n- item\n  - nested\n---");
        let ps = paras(&roundtrip(&tree));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(
            texts,
            vec![
                "quoted".to_string(),
                "item".to_string(),
                "nested".to_string(),
                String::new(),
            ],
            "引用/列表平铺（次序保持）+ 分隔线 → 空段"
        );
        assert!(ps.iter().all(|p| p.style.is_none()));
    }

    #[test]
    fn multiline_paragraph_splits_with_marks_and_crlf() {
        // 多行文本：每行一段，mark 按行区间平移（第一行 bold）
        let tree = single(
            BlockKind::Paragraph,
            "bold a\nplain b",
            Attrs::new(),
            vec![span(0, 6, "strong")],
        );
        let ps = paras(&roundtrip(&tree));
        let (_, r0) = para_tuples(&ps[0]);
        let (_, r1) = para_tuples(&ps[1]);
        assert_eq!(r0, vec![("bold a".into(), true, false)]);
        assert_eq!(r1, vec![("plain b".into(), false, false)]);

        // CRLF：行尾 \r 剥离（markdown.rs split('\n') 保留 \r 的容忍面）
        let crlf = single(BlockKind::Paragraph, "x\r\ny", Attrs::new(), vec![]);
        let ps = paras(&roundtrip(&crlf));
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(texts, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn empty_tree_yields_valid_minimal_docx() {
        let tree = BlockTree::new();
        let bytes = export_docx_core(&tree).expect("export empty");
        assert!(bytes.len() > 4 && &bytes[..2] == b"PK");
        let doc = read_docx_from_bytes(&bytes).expect("read back");
        assert!(doc.blocks.is_empty(), "空文档 = 0 块（最小合法 docx）");
        assert!(doc.headers.is_empty() && doc.footers.is_empty());
    }

    /// r4-t2：资源限错误码可区分——`docx_limit`（office-io
    /// `MAX_PARAGRAPHS`/`MAX_RUNS`，save 面 typed 拒、超限导出无产物
    /// 字节）≠ `docx_build`（引擎面既有单族）。常量单一真源在
    /// office-io docx.rs，本层零自立数字。
    #[test]
    fn limit_error_code_mapping() {
        let e = map_error(&mdc_office_io::docx::DocxError::Limit(
            "paragraph count over limit 1000000".into(),
        ));
        assert_eq!(e.code, "docx_limit");
        assert!(
            e.message.starts_with("docx writer resource limit"),
            "{:?}",
            e.message
        );
        // 既有面不动：Container/Parse/Io 仍归 container_prescan/
        // docx_parse/io（读面）与 docx_build（写面引擎）。
        for (err, code) in [
            (
                mdc_office_io::docx::DocxError::Parse("x".into()),
                "docx_build",
            ),
            (mdc_office_io::docx::DocxError::Io("x".into()), "docx_build"),
            (
                mdc_office_io::docx::DocxError::InvalidInput("V7: x".into()),
                "docx_invalid_input",
            ),
        ] {
            assert_eq!(map_error(&err).code, code);
        }
    }

    /// r5-t2 探针④：**链接形态文本的导出面**——mdcore 行级 markdown
    /// 解析不产行内 marks（markdown.rs 模块文档「marks 不写入 MD」；
    /// `[t](url)` 行入树 = 段落字面文本）→ 导出 DOCX：
    /// (a) XML 面**零 `w:hyperlink`**（无 RunSpec.link 源——mark 面
    ///     `inline_mono_del_u_expressed_link_plain` 已钉 link mark 降
    ///     纯文本；本探针钉 markdown 字面文本路径）；
    /// (b) 字面 `[t](url)` 文本逐字保留（URL 以文本形态存活——
    ///     非结构化）；
    /// (c) 重导入（office-io 读面）文本零损——链接形态文本的
    ///     「文本层往返」成立，「超链接层往返」不成立（MarkSpan 无
    ///     URL 载荷 = mdcore MD 域上游缺口，t20 登记 + 本探针留证）。
    #[test]
    fn r5t2_markdown_link_literal_text_not_hyperlink() {
        let tree = markdown_to_tree("See [docs](https://x.example/) and [top](#sec1).");
        // 前置：树面 = 单段字面文本（无 marks——行级解析边界）。
        let bytes = export_docx_core(&tree).expect("export");
        let xml = zip_stored_part(&bytes, "word/document.xml");
        assert_eq!(
            xml.matches("<w:hyperlink").count(),
            0,
            "无链接载荷源 → 零 w:hyperlink 元素"
        );
        // 引擎对 `( )` 括号不转义——字面串原样在 XML。
        assert!(
            xml.contains("See [docs](https://x.example/) and [top](#sec1)."),
            "链接形态字面文本逐字保留: {xml}"
        );
        // 文本层往返：读回文本零损（无 hyperlink 挂接）。
        let doc = read_docx_from_bytes(&bytes).expect("read back");
        let ps = paras(&doc.blocks);
        assert_eq!(ps.len(), 1);
        assert_eq!(
            ps[0].text(),
            "See [docs](https://x.example/) and [top](#sec1)."
        );
        assert_eq!(ps[0].runs[0].hyperlink, None, "纯文本 run——无目标");
    }

    // -----------------------------------------------------------------
    // issue #10（TR-4）：export_docx_styled——G2 格式面消费
    // -----------------------------------------------------------------

    /// TR-4 选项 JSON（issue #10 场景：14pt 四号 + 1800 twips 边距——
    /// G2 LO 实测口径：90.1pt = 1800 twips、14pt = 四号）。
    fn tr4_full_options() -> String {
        r#"{
            "typography": {
                "ascii_font": "Times New Roman",
                "east_asia_font": "宋体",
                "complex_script_font": "Times New Roman",
                "size_half_points": 28,
                "line_spacing": {
                    "line": 360,
                    "line_rule": "auto",
                    "before": 120,
                    "after": 120
                }
            },
            "page_geometry": {
                "width": 11906,
                "height": 16838,
                "margin_top": 1800,
                "margin_right": 1800,
                "margin_bottom": 1800,
                "margin_left": 1800,
                "margin_header": 851,
                "margin_footer": 992,
                "gutter": 0
            }
        }"#
        .to_string()
    }

    /// TR-4-E1：全选项发射——styles.xml docDefaults（rFonts 三槽/字号/
    /// 行距）+ document.xml sectPr（pgSz/pgMar）双部件 XML 断言。
    #[test]
    fn tr4_e1_styled_emission_full_options() {
        let tree = markdown_to_tree("# 标题\n\n正文段落。\n");
        let bytes = export_docx_styled_core(&tree, &tr4_full_options()).expect("styled export");
        // styles.xml：docDefaults 排版三面
        let st = zip_stored_part(&bytes, "word/styles.xml");
        assert!(st.contains("Times New Roman"), "ascii 字体在场");
        assert!(st.contains("宋体"), "eastAsia 字体在场");
        assert!(
            st.contains(r#"w:val="28""#),
            "字号 28 半磅（14pt 四号）在场"
        );
        assert!(
            st.contains(r#"w:line="360""#),
            "行距 360（1.5 倍 auto）在场"
        );
        assert!(st.contains("auto"), "lineRule=auto 在场");
        assert!(
            st.contains(r#"w:before="120""#) && st.contains(r#"w:after="120""#),
            "段前/段后 120 twips 在场"
        );
        // document.xml：sectPr 几何
        let doc = zip_stored_part(&bytes, "word/document.xml");
        assert!(
            doc.contains(r#"<w:pgSz w:w="11906" w:h="16838""#),
            "pgSz A4 在场"
        );
        assert!(
            doc.contains(r#"w:left="1800""#) && doc.contains(r#"w:right="1800""#),
            "pgMar 左右 1800 twips（= G2 LO 实测 90.1pt）在场"
        );
        // 读回全环（容器门 + 模型解析）
        let back = read_docx_from_bytes(&bytes).expect("read back");
        assert!(paras(&back.blocks).iter().any(|p| p.text() == "正文段落。"));
    }

    /// TR-4-E2：空选项 ≡ 既有导出（字节级零变化——既有契约保护）；
    /// `null` 字段与缺省同义。
    #[test]
    fn tr4_e2_empty_options_byte_equivalence() {
        let tree = markdown_to_tree("para one\n\npara two\n");
        let plain = export_docx_core(&tree).expect("plain export");
        let empty = export_docx_styled_core(&tree, "{}").expect("empty options");
        assert_eq!(plain, empty, "空对象选项必须与无选项导出逐字节相等");
        let nulls = export_docx_styled_core(&tree, r#"{"typography":null,"page_geometry":null}"#)
            .expect("null options");
        assert_eq!(plain, nulls, "null 字段与缺省同义");
    }

    /// TR-4-E3：形状错误面 `docx_options`（六面：坏 JSON/非对象/未知
    /// 顶层键/缺必填字段/类型错/line_rule 枚举外值）。
    #[test]
    fn tr4_e3_shape_errors_docx_options() {
        let tree = markdown_to_tree("x\n");
        let cases: Vec<(&str, &str)> = vec![
            ("bad json", "{not json"),
            ("non object", "[1,2]"),
            ("unknown key", r#"{"margins": 1}"#),
            ("missing field", r#"{"typography": {"ascii_font": "A"}}"#),
            (
                "wrong type",
                r#"{"typography": {"ascii_font": 5, "east_asia_font": "S", "complex_script_font": "C", "size_half_points": 21}}"#,
            ),
            (
                "bad line_rule",
                r#"{"typography": {"ascii_font": "A", "east_asia_font": "S", "complex_script_font": "C", "size_half_points": 21, "line_spacing": {"line": 240, "line_rule": "double"}}}"#,
            ),
        ];
        for (name, json) in cases {
            let e = export_docx_styled_core(&tree, json).expect_err("shape error must reject");
            assert_eq!(e.code, "docx_options", "{name}: {e:?}");
        }
    }

    /// TR-4-E4：V9 值域拒绝经 save 漏斗——`docx_invalid_input`（与
    /// 形状面 `docx_options` 可区分；本层零自立数字的单一真源验证）。
    #[test]
    fn tr4_e4_v9_domain_reject_docx_invalid_input() {
        let tree = markdown_to_tree("x\n");
        // 几何零宽
        let e = export_docx_styled_core(
            &tree,
            r#"{"page_geometry": {"width": 0, "height": 16838, "margin_top": 1800, "margin_right": 1800, "margin_bottom": 1800, "margin_left": 1800, "margin_header": 851, "margin_footer": 992, "gutter": 0}}"#,
        )
        .expect_err("zero width must reject");
        assert_eq!(e.code, "docx_invalid_input");
        assert!(e.message.contains("V9"), "V9 单一真源消息: {e:?}");
        // 负边距
        let e = export_docx_styled_core(
            &tree,
            r#"{"page_geometry": {"width": 11906, "height": 16838, "margin_top": -5, "margin_right": 1800, "margin_bottom": 1800, "margin_left": 1800, "margin_header": 851, "margin_footer": 992, "gutter": 0}}"#,
        )
        .expect_err("negative margin must reject");
        assert_eq!(e.code, "docx_invalid_input");
        // 字号零
        let e = export_docx_styled_core(
            &tree,
            r#"{"typography": {"ascii_font": "A", "east_asia_font": "S", "complex_script_font": "C", "size_half_points": 0}}"#,
        )
        .expect_err("zero size must reject");
        assert_eq!(e.code, "docx_invalid_input");
        // 行距非正
        let e = export_docx_styled_core(
            &tree,
            r#"{"typography": {"ascii_font": "A", "east_asia_font": "S", "complex_script_font": "C", "size_half_points": 21, "line_spacing": {"line": 0, "line_rule": "auto"}}}"#,
        )
        .expect_err("zero line must reject");
        assert_eq!(e.code, "docx_invalid_input");
    }

    /// TR-4-E5：带样式导出的文本层往返零损（格式面不碰内容面）。
    #[test]
    fn tr4_e5_styled_roundtrip_text_preserved() {
        let md = "# 标题一\n\n段落甲。\n\n- 列表项 1\n- 列表项 2\n";
        let tree = markdown_to_tree(md);
        let bytes = export_docx_styled_core(&tree, &tr4_full_options()).expect("export");
        let back = read_docx_from_bytes(&bytes).expect("read back");
        let ps = paras(&back.blocks);
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert!(texts.contains(&"标题一".to_string()));
        assert!(texts.contains(&"段落甲。".to_string()));
        assert!(texts.contains(&"列表项 1".to_string()));
        assert!(texts.contains(&"列表项 2".to_string()));
    }

    /// TR-4-E6：部分选项面——仅排版（几何缺省 = 引擎默认 sectPr）、
    /// 仅几何（排版缺省 = 无 docDefaults 覆盖）各自独立生效。
    #[test]
    fn tr4_e6_partial_options_independent() {
        let tree = markdown_to_tree("x\n");
        // 仅排版
        let bytes = export_docx_styled_core(
            &tree,
            r#"{"typography": {"ascii_font": "Fira Code", "east_asia_font": "楷体", "complex_script_font": "Fira Code", "size_half_points": 21}}"#,
        )
        .expect("typo only");
        let st = zip_stored_part(&bytes, "word/styles.xml");
        assert!(st.contains("Fira Code") && st.contains("楷体"));
        assert!(st.contains(r#"w:val="21""#), "10.5pt = 21 半磅在场");
        // 仅几何：默认 styles.xml 无自定义字号覆盖（docx-rs 引擎基态
        // 不含 21——上一断言的值在本面不得复现）
        let bytes2 = export_docx_styled_core(
            &tree,
            r#"{"page_geometry": {"width": 11906, "height": 16838, "margin_top": 1440, "margin_right": 1440, "margin_bottom": 1440, "margin_left": 1440, "margin_header": 851, "margin_footer": 992, "gutter": 0}}"#,
        )
        .expect("geom only");
        let doc = zip_stored_part(&bytes2, "word/document.xml");
        assert!(
            doc.contains(r#"w:left="1440""#) && doc.contains(r#"w:right="1440""#),
            "仅几何面：pgMar 生效"
        );
    }

    // -----------------------------------------------------------------
    // W-1/W-2（issue #29 阶段 A0）：`toc` 目录域 + `anonymize` 匿名化
    // 两 options 键（office-io API 在场、wasm 导出链此前未接——本批
    // 接线；缺省行为与既有导出字节级一致 = 纯加性）。
    // -----------------------------------------------------------------

    /// 匿名 core.xml 期望字节（office-io `ANON_CORE_XML` 同文——引擎
    /// 单一真源常量的 wasm 侧镜像；office-io 测试 g3_t1 双向钉定，
    /// 漂移即本断言红）。unknown 作者 + epoch 零时 + revision=1。
    const W2_ANON_CORE_XML: &str = concat!(
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

    /// 匿名 app.xml 期望字节（空骨架——office-io `ANON_APP_XML` 同文）。
    const W2_ANON_APP_XML: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties""#,
        r#" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"/>"#,
    );

    /// W-1 验收主用例：`toc` enabled → document.xml **首段** = TOC 域
    /// 五连（fldChar begin dirty=true + instrText `TOC \o "lo-hi"` +
    /// separate + 提示文本 + end——缺省层级 1/3），且居全部内容块
    /// 之前（Word 惯例——目录在正文前）；正文内容零损（加性首段）。
    #[test]
    fn w1_toc_field_five_part_emission_and_position() {
        let tree = markdown_to_tree("# 章一\n\n正文段。\n");
        let bytes = export_docx_styled_core(&tree, r#"{"toc":{"enabled":true}}"#)
            .expect("toc enabled, default levels");
        let doc = zip_stored_part(&bytes, "word/document.xml");
        // 五连结构（instrText 引号在 XML 面 = &quot; 转义——字符串面
        // 直断言与既有 XML 面测试同纪律）。
        assert!(
            doc.contains(r#"<w:fldChar w:fldCharType="begin" w:dirty="true" />"#),
            "fldChar begin dirty=true 在场（打开提示更新域）"
        );
        assert_eq!(
            doc.matches(r#"<w:instrText>TOC \o &quot;1-3&quot;</w:instrText>"#)
                .count(),
            1,
            "instrText TOC \\o \"1-3\"（层级缺省 1/3）恰一次"
        );
        assert!(
            doc.contains(r#"<w:fldChar w:fldCharType="separate""#),
            "separate 域结果分界在场"
        );
        // #196 任务 2：域结果区 = 静态目录项（占位提示升级——有收录
        // 标题时目录可见且可点击跳转；零收录仍退化占位形，见下文
        // 2-4 层级断言）。
        assert!(
            doc.contains(r#"<w:hyperlink w:anchor="toc_0""#),
            "目录项 hyperlink（w:anchor 引用侧）在场"
        );
        assert!(
            doc.contains("<w:instrText>PAGEREF toc_0 \\h</w:instrText>"),
            "目录项 PAGEREF 域（\\h 超链接行为）在场"
        );
        assert!(
            doc.contains(r#"<w:bookmarkStart w:id="0" w:name="toc_0""#),
            "标题书签（定义侧）在场"
        );
        assert!(
            doc.contains(r#"<w:tab w:val="right" w:leader="dot" w:pos="8504""#),
            "右对齐点线 tab stop（A4 缺省文本宽）在场"
        );
        assert!(
            doc.contains(r#"<w:fldChar w:fldCharType="end""#),
            "fldChar end 在场（跨段域收尾）"
        );
        // 位置：TOC 域居文档开头（首内容块之前）。
        let toc_pos = doc.find("TOC \\o").expect("TOC instr 在场");
        let first_heading = doc.find("章一").expect("首标题在场");
        assert!(
            toc_pos < first_heading,
            "TOC 域段居首内容块之前（Word 惯例：目录在正文前）"
        );
        // 引用侧（目录项 hyperlink）在定义侧（标题书签）之前——目录
        // 居正文前的文档序。
        let link_pos = doc.find(r#"w:anchor="toc_0""#).expect("引用侧");
        let bm_pos = doc.find(r#"w:name="toc_0""#).expect("定义侧");
        assert!(link_pos < bm_pos, "目录项引用侧先于标题书签定义侧");
        // 文本面：读回 = TOC 域块（域头空段 + 目录项段 + 域尾空段）
        // + 原内容零损（目录项文本 = 标题文本 + tab 字符；PAGEREF
        // 页码占位 volatile 剥离不拼接）。
        let back = read_docx_from_bytes(&bytes).expect("read back");
        let ps = paras(&back.blocks);
        let texts: Vec<String> = ps.iter().map(|p| p.text()).collect();
        assert_eq!(
            texts,
            vec![
                "".to_string(),
                "章一\t".to_string(),
                "".to_string(),
                "章一".to_string(),
                "正文段。".to_string(),
            ],
            "TOC 块 = 域头空段 + 目录项段（tab 读回 \t）+ 域尾空段；正文零损"
        );

        // 自定义层级 2-4 → `TOC \o "2-4"`。
        let bytes_24 = export_docx_styled_core(
            &tree,
            r#"{"toc":{"enabled":true,"level_from":2,"level_to":4}}"#,
        )
        .expect("toc 2-4");
        let doc_24 = zip_stored_part(&bytes_24, "word/document.xml");
        assert!(
            doc_24.contains(r#"<w:instrText>TOC \o &quot;2-4&quot;</w:instrText>"#),
            "自定义层级 2-4 进 \\o 开关"
        );
        // #196 任务 2：层级域外（章一 = H1 不在 2-4）→ 零收录 → 空
        // 退化占位形（提示 F9；零目录项/零 PAGEREF/零书签）。
        assert!(
            doc_24.contains("（目录域——打开后全选再按 F9 更新生成）"),
            "零收录 = 占位退化形（提示 F9）"
        );
        assert!(!doc_24.contains("PAGEREF"), "零收录零 PAGEREF");
        assert!(!doc_24.contains("bookmarkStart"), "零收录零书签");
        // 合法边界 1-9 全域恰过（引擎 V8 边界镜像）。
        let bytes_lo = export_docx_styled_core(
            &tree,
            r#"{"toc":{"enabled":true,"level_from":1,"level_to":9}}"#,
        )
        .expect("toc 1-9");
        let doc_lo = zip_stored_part(&bytes_lo, "word/document.xml");
        assert!(
            doc_lo.contains(r#"<w:instrText>TOC \o &quot;1-9&quot;</w:instrText>"#),
            "合法边界 1-9 全域恰过"
        );

        // 与 typography 组合互不干扰（G2 面仍生效）。
        let bytes_combo = export_docx_styled_core(
            &tree,
            r#"{"toc":{"enabled":true},"typography":{"ascii_font":"Fira Code","east_asia_font":"楷体","complex_script_font":"Fira Code","size_half_points":21}}"#,
        )
        .expect("toc + typography");
        let st = zip_stored_part(&bytes_combo, "word/styles.xml");
        assert!(
            st.contains("Fira Code") && st.contains("楷体"),
            "排版面不受 toc 影响"
        );
        let doc_combo = zip_stored_part(&bytes_combo, "word/document.xml");
        assert!(
            doc_combo.contains(r#"<w:instrText>TOC \o &quot;1-3&quot;</w:instrText>"#),
            "toc 面不受 typography 影响"
        );
    }

    /// #196 任务 2 w9a：多级静态目录项结构——目录项数/缩进梯度/
    /// hyperlink↔bookmarkStart 逐项配平（引用侧与定义侧名字闭环——
    /// 配对错位即红）。
    #[test]
    fn w9_toc_static_entries_multilevel_structure() {
        let tree = markdown_to_tree("# 章一\n\n## 节甲\n\n### 目乙\n\n正文。\n");
        let bytes =
            export_docx_styled_core(&tree, r#"{"toc":{"enabled":true}}"#).expect("toc 1-3 默认");
        let doc = zip_stored_part(&bytes, "word/document.xml");
        // 目录项数 = 收录标题数（H1+H2+H3 = 3；正文段不入目录）。
        assert_eq!(
            doc.matches("PAGEREF toc_").count(),
            3,
            "恰 3 目录项 PAGEREF"
        );
        // 缩进梯度：(level-1)×240 —— H1=0（无 ind）、H2=240、H3=480。
        assert!(
            doc.contains(r#"<w:ind w:left="240""#) && doc.contains(r#"<w:ind w:left="480""#),
            "层级缩进梯度 240/480 在场"
        );
        // 逐项配平：每个目录项 hyperlink anchor（引用侧）都有同名
        // bookmarkStart（定义侧）——跳转闭环零悬空（若桥面配对错位
        // /书签漏挂，此处即红）。
        for n in ["toc_0", "toc_1", "toc_2"] {
            assert!(
                doc.contains(&format!(r#"<w:hyperlink w:anchor="{n}""#)),
                "引用侧 {n}"
            );
            let bm = format!(r#"w:name="{n}""#);
            assert!(doc.contains(&bm), "定义侧 {n}（配平——零悬空引用）");
        }
        assert_eq!(
            doc.matches("bookmarkStart").count(),
            3,
            "定义侧恰 3（收录标题各一）"
        );
        // 域配平：begin/separate/end = TOC(1) + PAGEREF(3) = 4。
        assert_eq!(
            doc.matches(r#"w:fldCharType="begin""#).count(),
            4,
            "begin 配平"
        );
        assert_eq!(doc.matches(r#"w:fldCharType="end""#).count(), 4, "end 配平");
        // 标题样式双通道保持（书签挂接不改变 Heading 面）。
        for h in ["Heading1", "Heading2", "Heading3"] {
            assert!(doc.contains(&format!(r#"w:val="{h}""#)), "{h} 样式在场");
        }
    }

    /// #196 任务 2 w9b：层级过滤——`\o` 域外标题不入目录不挂书签
    ///（H4 在 1-3 域外：bookmarkStart 恰 = 收录数；H4 段无书签
    /// 面——裸 heading 路径零漂移）。
    #[test]
    fn w9_toc_level_filter_excludes_out_of_range_headings() {
        let tree = markdown_to_tree("# 章一\n\n#### 深层四\n\n正文。\n");
        let bytes = export_docx_styled_core(&tree, r#"{"toc":{"enabled":true}}"#).expect("toc 1-3");
        let doc = zip_stored_part(&bytes, "word/document.xml");
        // H4（层级 4）在 1-3 域外 → 不入目录。
        assert_eq!(
            doc.matches("PAGEREF toc_").count(),
            1,
            "仅 H1 入目录（H4 域外）"
        );
        // H4 文本恰现一次（正文标题段）——目录项无它（hyperlink 面
        // 恰 1 = H1 项）。
        assert_eq!(
            doc.matches("深层四").count(),
            1,
            "H4 恰现一次（仅正文标题段，不入目录项）"
        );
        assert_eq!(
            doc.matches("<w:hyperlink").count(),
            1,
            "hyperlink 面 = 仅 H1 目录项"
        );
        // H4 段在场（正文标题零损）且无书签（bookmarkStart 恰 1 = H1）。
        assert!(doc.contains("深层四"), "H4 正文标题在场（零损）");
        assert_eq!(
            doc.matches("bookmarkStart").count(),
            1,
            "书签恰挂收录标题（H4 域外裸 heading）"
        );
        // H4 样式仍发射（样式面与 TOC 收录面正交）。
        assert!(doc.contains(r#"w:val="Heading4""#), "H4 样式在场");
    }

    /// W-1/W-2 回归锚：两键缺省/null/enabled:false/anonymize:false →
    /// 与既有导出**逐字节一致**（纯加性 options——既有契约保护，
    /// tr4_e2 空对象面的两键扩展）。
    #[test]
    fn w1_w2_absent_options_byte_equivalence() {
        let tree = markdown_to_tree("para one\n\npara two\n");
        let plain = export_docx_core(&tree).expect("plain export");
        for json in [
            "{}",
            r#"{"toc":null,"anonymize":null}"#,
            r#"{"toc":{"enabled":false}}"#,
            // enabled=false 时层级字段不消费（禁用态零发射，字节稳定）。
            r#"{"toc":{"enabled":false,"level_from":2,"level_to":5}}"#,
            r#"{"anonymize":false}"#,
        ] {
            let out = export_docx_styled_core(&tree, json).expect("equivalent options");
            assert_eq!(plain, out, "options {json} 必须与无选项导出逐字节相等");
        }
    }

    /// W-1 层级域 + W-2 形状 typed 拒（`docx_options`——与既有形状错误
    /// 同码；层级域 0/10/倒序本层前置拒，引擎 V8 兜底不可达——完备性）。
    #[test]
    fn w1_toc_level_domain_and_shape_reject() {
        let tree = markdown_to_tree("x\n");
        let cases: Vec<(&str, &str)> = vec![
            (
                "level_from 0",
                r#"{"toc":{"enabled":true,"level_from":0,"level_to":3}}"#,
            ),
            (
                "level_to 10",
                r#"{"toc":{"enabled":true,"level_from":1,"level_to":10}}"#,
            ),
            (
                "from > to",
                r#"{"toc":{"enabled":true,"level_from":5,"level_to":2}}"#,
            ),
            (
                "both 0",
                r#"{"toc":{"enabled":true,"level_from":0,"level_to":0}}"#,
            ),
            (
                "level float",
                r#"{"toc":{"enabled":true,"level_from":1.5}}"#,
            ),
            ("level string", r#"{"toc":{"enabled":true,"level_to":"3"}}"#),
            ("enabled missing", r#"{"toc":{}}"#),
            ("enabled wrong type", r#"{"toc":{"enabled":"yes"}}"#),
            ("toc not object", r#"{"toc":true}"#),
            ("anonymize wrong type", r#"{"anonymize":"yes"}"#),
            ("anonymize not bool num", r#"{"anonymize":1}"#),
        ];
        for (name, json) in cases {
            let e = export_docx_styled_core(&tree, json).expect_err("must reject");
            assert_eq!(e.code, "docx_options", "{name}: {e:?}");
        }
    }

    /// W-2 验收主用例：`anonymize:true` → docProps/core.xml 逐字节 =
    /// 匿名常量 + app.xml 空骨架 + settings.xml 无作者/公司/机器名
    /// 残留 + 正文部件原压缩流零触碰 + 匿名产物可解析（Word 可开的
    /// 最小代理）。引擎新建面已匿名（docx-rs 默认 = 匿名常量，
    /// office-io g3_t1 单一真源）——本面 = 幂等再洗钉定 + 显式开关。
    #[test]
    fn w2_anonymize_props_scrub() {
        let tree = markdown_to_tree("# 章一\n\n正文段。\n");
        let plain = export_docx_core(&tree).expect("plain export");
        // 前置登记：引擎新建面已匿名（docx-rs 0.4.22 默认 core.xml =
        // 匿名常量——office-io ANON_CORE_XML 逐字节同构）。
        let core_plain = String::from_utf8(
            mdc_office_io::docx::docx_part_bytes(&plain, "docProps/core.xml").unwrap(),
        )
        .unwrap();
        assert_eq!(
            core_plain, W2_ANON_CORE_XML,
            "引擎新建面 core.xml 已匿名（单一真源）"
        );

        let anon =
            export_docx_styled_core(&tree, r#"{"anonymize":true}"#).expect("anonymize export");
        // ① core.xml 逐字节 = 匿名常量（dc:creator=unknown /
        //    cp:lastModifiedBy=unknown / epoch 零时 / revision=1）。
        let core = String::from_utf8(
            mdc_office_io::docx::docx_part_bytes(&anon, "docProps/core.xml").unwrap(),
        )
        .unwrap();
        assert_eq!(
            core, W2_ANON_CORE_XML,
            "匿名导出 core.xml 逐字节 = 匿名常量"
        );
        // ② app.xml = 空骨架（Company/Manager/Sender 面零残留）。
        let app = String::from_utf8(
            mdc_office_io::docx::docx_part_bytes(&anon, "docProps/app.xml").unwrap(),
        )
        .unwrap();
        assert_eq!(app, W2_ANON_APP_XML, "app.xml = 空骨架");
        // ③ settings.xml 无作者/公司/机器名残留。
        let settings = String::from_utf8(
            mdc_office_io::docx::docx_part_bytes(&anon, "word/settings.xml").unwrap(),
        )
        .unwrap();
        for token in [
            "Company", "Author", "Manager", "Sender", "creator", "Machine", "UserName",
        ] {
            assert!(
                !settings.contains(token),
                "settings.xml 不得含 {token} 残留: {settings}"
            );
        }
        // ④ 正文部件原压缩流零触碰（anonymize 不变量）。
        assert_eq!(
            mdc_office_io::docx::docx_part_bytes(&anon, "word/document.xml"),
            mdc_office_io::docx::docx_part_bytes(&plain, "word/document.xml"),
            "正文部件字节不变"
        );
        // ⑤ 匿名产物可解析。
        assert!(read_docx_from_bytes(&anon).is_ok(), "匿名产物可解析");

        // toc + anonymize 组合（两开关独立）。
        let both = export_docx_styled_core(&tree, r#"{"toc":{"enabled":true},"anonymize":true}"#)
            .expect("toc + anonymize");
        assert_eq!(
            mdc_office_io::docx::docx_part_bytes(&both, "docProps/core.xml"),
            mdc_office_io::docx::docx_part_bytes(&anon, "docProps/core.xml"),
            "组合面 core.xml 与单独 anonymize 一致"
        );
        let doc_both = String::from_utf8(
            mdc_office_io::docx::docx_part_bytes(&both, "word/document.xml").unwrap(),
        )
        .unwrap();
        assert!(
            doc_both.contains(r#"<w:instrText>TOC \o &quot;1-3&quot;</w:instrText>"#),
            "组合面 TOC 域在场"
        );
    }

    // -------------------------------------------------------------------
    // N-5：estimate_pages_core——options 同形语法 + 参数映射 + 错误面。
    // 校准证据 = test-artifacts/pagination/CALIBRATION.md（40 万字双档
    // +0.61%/-2.46%）；此处钉定桥层映射的确定性。
    // -------------------------------------------------------------------

    #[test]
    fn estimate_default_options_match_engine_baseline() {
        let tree = markdown_to_tree(&format!("{}\n", "汉".repeat(40)).repeat(200));
        // 空 options ≡ 引擎缺省档（与不传选项导出基线一致）。
        let est = serde_json::from_str::<serde_json::Value>(
            &estimate_pages_core(&tree, "{}").expect("estimate"),
        )
        .expect("json");
        assert_eq!(est["pages"].as_u64().unwrap() > 0, true);
        assert_eq!(est["chars_per_line"], 42, "8504/200 = 42");
        assert_eq!(est["error_margin_pct"], 5.0, "纯段落 5% 档");
    }

    #[test]
    fn estimate_tender_options_mapping() {
        let tree = markdown_to_tree(&format!("{}\n", "汉".repeat(40)).repeat(200));
        // tender 档（四号 1.5 倍 / Word 中文边距 1440/1800）：
        // 版心 8306×13958、cpl = 29、每页行数 = floor(13958/570.22) = 24。
        let opts = r#"{
            "typography": {
                "ascii_font": "Times New Roman",
                "east_asia_font": "FangSong",
                "complex_script_font": "Times New Roman",
                "size_half_points": 28,
                "line_spacing": { "line": 360, "line_rule": "auto" }
            },
            "page_geometry": {
                "width": 11906, "height": 16838,
                "margin_top": 1440, "margin_right": 1800,
                "margin_bottom": 1440, "margin_left": 1800,
                "margin_header": 851, "margin_footer": 992, "gutter": 0
            }
        }"#;
        let est = serde_json::from_str::<serde_json::Value>(
            &estimate_pages_core(&tree, opts).expect("estimate"),
        )
        .expect("json");
        assert_eq!(est["chars_per_line"], 29, "8306/280 = 29.66 → 29");
        assert_eq!(est["lines_per_page"], 24, "floor(13958/570.22) = 24");
        // 页数 > 同文档缺省档（1.5 倍行距 + 大字号放大）。
        let base = serde_json::from_str::<serde_json::Value>(
            &estimate_pages_core(&tree, "{}").expect("estimate"),
        )
        .expect("json");
        assert!(
            est["pages"].as_u64().unwrap() > base["pages"].as_u64().unwrap(),
            "tender 档页数 > 缺省档"
        );
    }

    #[test]
    fn estimate_tolerates_toc_anonymize_and_rejects_bad_options() {
        let tree = markdown_to_tree("单段");
        // toc/anonymize 键合法（不参与估算——目录 = 渲染期生成面）。
        assert!(estimate_pages_core(&tree, r#"{"toc":{"enabled":true},"anonymize":true}"#).is_ok());
        // 形状错误 = docx_options（复用导出 options 单一形状真源）。
        let err = estimate_pages_core(&tree, "not json").expect_err("bad json");
        assert_eq!(err.code, "docx_options");
        let err2 = estimate_pages_core(&tree, r#"{"unknown_key": 1}"#).expect_err("unknown key");
        assert_eq!(err2.code, "docx_options");
    }

    /// N-4（#29）：GFM 表首行即表头 → 导出面首行 `tblHeader`（跨页
    /// 表头重复）；读面不回读（docx-rs 反序列化丢 trPr 扩展——映射表
    /// 登记的 round-trip 缺口，此处钉定结构完整性不受注入影响）。
    #[test]
    fn gfm_table_first_row_gets_tblheader() {
        let tree = markdown_to_tree("| 甲 | 乙 |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |");
        let bytes = export_docx_core(&tree).expect("export");
        let xml = String::from_utf8(
            mdc_office_io::docx::docx_part_bytes(&bytes, "word/document.xml").unwrap(),
        )
        .unwrap();
        assert_eq!(
            xml.matches("<w:trPr><w:tblHeader /></w:trPr>").count(),
            1,
            "恰首行打标"
        );
        // 分隔行已剔除（GFM 语义）→ 3 数据行（表头 + 2 数据）中仅表头打标。
        assert_eq!(xml.matches("<w:trPr />").count(), 2, "其余行保持原形");
        // 注入产物结构完整：引擎读面可解析。
        assert!(read_docx_from_bytes(&bytes).is_ok(), "注入产物可读回");
    }

    // ------------------------------------------------------------ P1 #190：页眉/页脚

    /// P1 #190 辅助：zip 部件 XML 文本（`docx_part_bytes` 咖啡因包装
    /// ——office-io 公开面，容器门内建）。
    fn part_xml(bytes: &[u8], name: &str) -> String {
        String::from_utf8(
            mdc_office_io::docx::docx_part_bytes(bytes, name)
                .unwrap_or_else(|| panic!("part {name} not found")),
        )
        .unwrap()
    }

    /// P1 #190-E1：页眉/页脚默认槽发射 + 页码域（issue #190 验收①②：
    /// header/footer 含页码域；读回可见——office-io 读面 headers/
    /// footers 槽位挂接 + 文本面）。
    #[test]
    fn p1_hf_default_slots_with_page_fields() {
        let tree = markdown_to_tree("正文段落。\n");
        let options = r#"{
            "header": {
                "runs": [
                    {"text": "XX公司投标文件", "bold": true},
                    {"text": "  第 "},
                    {"field": "page"},
                    {"text": " 页"}
                ]
            },
            "footer": {
                "runs": [
                    {"text": "第 "}, {"field": "page"},
                    {"text": " 页 / 共 "}, {"field": "numpages"}, {"text": " 页"}
                ]
            }
        }"#;
        let bytes = export_docx_styled_core(&tree, options).expect("styled export");
        // 部件面：header1/footer1 在场 + 域指令 + 加粗位
        let h1 = part_xml(&bytes, "word/header1.xml");
        assert!(h1.contains("XX公司投标文件"), "页眉文本在场: {h1}");
        assert!(h1.contains("<w:b />"), "页眉加粗位在场");
        assert!(h1.contains("PAGE"), "页眉 PAGE 域在场");
        let f1 = part_xml(&bytes, "word/footer1.xml");
        assert!(f1.contains("页 / 共"), "页脚文本在场");
        assert!(
            f1.contains("PAGE") && f1.contains("NUMPAGES"),
            "页脚双域在场"
        );
        // document.xml：槽位引用挂接（default 型）
        let doc = part_xml(&bytes, "word/document.xml");
        assert!(
            doc.contains(r#"w:type="default""#) && doc.contains("headerReference"),
            "默认页眉引用挂接"
        );
        assert!(doc.contains("footerReference"), "页脚引用挂接");
        // 读回全环（验收⑦：office-io 读导出的 docx 能看到页眉页脚）
        let back = read_docx_from_bytes(&bytes).expect("read back");
        assert_eq!(back.headers.len(), 1, "默认页眉恰一槽");
        assert_eq!(back.headers[0].variant, HeaderFooterVariant::Default);
        let h_text: String = back.headers[0]
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect();
        assert!(h_text.contains("XX公司投标文件"), "读回页眉文本: {h_text}");
        assert_eq!(back.footers.len(), 1, "默认页脚恰一槽");
        assert_eq!(back.footers[0].variant, HeaderFooterVariant::Default);
        // 正文不受影响（页面装置面与文档体面正交）
        assert!(paras(&back.blocks).iter().any(|p| p.text() == "正文段落。"));
    }

    /// #196 任务 3 w10：hf run `charStyle` 键——预置字符样式引用
    ///（rStyle 发射 + styles.xml 定义恒在 + 互斥/悬空双层拒）。
    #[test]
    fn w10_hf_char_style_key() {
        let tree = markdown_to_tree("正文。\n");
        // 合法路径：Strong/Emphasis 引用发射 rStyle。
        let options = r#"{
            "header": {
                "runs": [
                    {"text": "重点强调", "charStyle": "Strong"},
                    {"text": " 普通对照"},
                    {"text": " 斜体引", "charStyle": "Emphasis"}
                ]
            }
        }"#;
        let bytes = export_docx_styled_core(&tree, options).expect("styled export");
        let h1 = part_xml(&bytes, "word/header1.xml");
        assert!(
            h1.contains(r#"<w:rStyle w:val="Strong" />"#),
            "Strong rStyle 发射: {h1}"
        );
        assert!(
            h1.contains(r#"<w:rStyle w:val="Emphasis" />"#),
            "Emphasis rStyle 发射"
        );
        // 普通 run 零 rStyle 面。
        assert_eq!(h1.matches("rStyle").count(), 2, "恰两个 rStyle");
        // styles.xml：预置字符样式定义恒在（hf 槽位独立部件也共享
        // 文档级 styles.xml）。
        let st = part_xml(&bytes, "word/styles.xml");
        assert!(
            st.contains(r#"<w:style w:type="character" w:styleId="Strong">"#),
            "Strong 定义在场"
        );
        // 互斥：field + charStyle 同 run 拒。
        let bad_mutex = r#"{
            "header": {"runs": [{"field": "page", "charStyle": "Strong"}]}
        }"#;
        let err = export_docx_styled_core(&tree, bad_mutex).unwrap_err();
        assert!(
            err.message.contains("mutually exclusive"),
            "field/charStyle 互斥拒: {}",
            err.message
        );
        // 悬空：未定义样式名经 V12 save 漏斗拒（桥面解析过——typed
        // 双层第二层）。
        let bad_dangling = r#"{
            "header": {"runs": [{"text": "x", "charStyle": "NoSuch"}]}
        }"#;
        let err2 = export_docx_styled_core(&tree, bad_dangling).unwrap_err();
        assert!(
            err2.message.contains("V12") || err2.message.contains("未定义"),
            "悬空样式 V12 拒: {}",
            err2.message
        );
        // null = 缺省（不引用——与既有 options 语义一致）。
        let null_ok = r#"{
            "header": {"runs": [{"text": "x", "charStyle": null}]}
        }"#;
        let bytes2 = export_docx_styled_core(&tree, null_ok).expect("null 合法");
        let h2 = part_xml(&bytes2, "word/header1.xml");
        assert!(!h2.contains("rStyle"), "null 缺省零 rStyle 面");
    }

    /// P1 #190-E2：首页/偶数页独立槽（验收③）——titlePg +
    /// evenAndOddHeaders 激活位由引擎自动置位；读回三槽 variant 齐。
    #[test]
    fn p1_hf_first_and_even_slots() {
        let tree = markdown_to_tree("x\n");
        let options = r#"{
            "header": {
                "runs": [{"text": "默认页眉"}],
                "first_page": {"runs": [{"text": "首页页眉"}]},
                "even_page": {"runs": [{"text": "偶数页页眉"}]}
            },
            "footer": {
                "runs": [{"text": "默认页脚"}],
                "first_page": {"runs": [{"text": "首页页脚"}]},
                "even_page": {"runs": [{"text": "偶数页页脚"}]}
            }
        }"#;
        let bytes = export_docx_styled_core(&tree, options).expect("styled export");
        // 六部件在场（header1-3 + footer1-3——docx-rs 槽位递增命名）
        for name in [
            "word/header1.xml",
            "word/header2.xml",
            "word/header3.xml",
            "word/footer1.xml",
            "word/footer2.xml",
            "word/footer3.xml",
        ] {
            assert!(
                mdc_office_io::docx::docx_part_bytes(&bytes, name).is_some(),
                "{name} 在场"
            );
        }
        let all_hf = [
            part_xml(&bytes, "word/header1.xml"),
            part_xml(&bytes, "word/header2.xml"),
            part_xml(&bytes, "word/header3.xml"),
            part_xml(&bytes, "word/footer1.xml"),
            part_xml(&bytes, "word/footer2.xml"),
            part_xml(&bytes, "word/footer3.xml"),
        ]
        .concat();
        for text in [
            "默认页眉",
            "首页页眉",
            "偶数页页眉",
            "默认页脚",
            "首页页脚",
            "偶数页页脚",
        ] {
            assert!(all_hf.contains(text), "{text} 在场");
        }
        // 激活位：document.xml titlePg + settings.xml evenAndOddHeaders
        let doc = part_xml(&bytes, "word/document.xml");
        assert!(doc.contains("titlePg"), "首页槽激活位 titlePg");
        assert!(
            doc.contains(r#"w:type="first""#) && doc.contains(r#"w:type="even""#),
            "first/even 型引用挂接"
        );
        let settings = part_xml(&bytes, "word/settings.xml");
        assert!(
            settings.contains("evenAndOddHeaders"),
            "偶数页槽激活位 evenAndOddHeaders"
        );
        // 读回：三槽 × 页眉/页脚，variant 齐
        let back = read_docx_from_bytes(&bytes).expect("read back");
        assert_eq!(back.headers.len(), 3, "页眉三槽");
        assert_eq!(back.footers.len(), 3, "页脚三槽");
        let variants = |v: &[HeaderFooterData]| v.iter().map(|d| d.variant).collect::<Vec<_>>();
        assert!(variants(&back.headers).contains(&HeaderFooterVariant::Default));
        assert!(variants(&back.headers).contains(&HeaderFooterVariant::First));
        assert!(variants(&back.headers).contains(&HeaderFooterVariant::Even));
        assert!(variants(&back.footers).contains(&HeaderFooterVariant::First));
    }

    /// P1 #190-E3：缺省字节级一致（验收⑥——null 形态与缺省同义；
    /// 空对象基线由 tr4_e2 钉定，此处补 header/footer 两键的 null 面）。
    #[test]
    fn p1_hf_null_keys_byte_equivalence() {
        let tree = markdown_to_tree("para\n");
        let plain = export_docx_core(&tree).expect("plain");
        let nulls =
            export_docx_styled_core(&tree, r#"{"header": null, "footer": null}"#).expect("nulls");
        assert_eq!(plain, nulls, "null 键 ≡ 缺省（零发射，字节稳定）");
        // false 形态的独立槽同义（first_page: false = 不设置）
        let options = r#"{"header": {"runs": [], "first_page": false, "even_page": false}}"#;
        let only_default = export_docx_styled_core(&tree, options).expect("only default slot");
        let bare = export_docx_styled_core(&tree, r#"{"header": {"runs": []}}"#).expect("bare");
        assert_eq!(only_default, bare, "false 独立槽 ≡ 缺省独立槽");
    }

    /// P1 #190-E4：形状错误面 `docx_options`（九面：非对象/缺 runs/
    /// runs 非数组/run 非对象/run 缺 text/field 枚举外/text+field 互斥
    /// /裸 true 独立槽/未知字段）——与既有形状错误同码可区分。
    #[test]
    fn p1_hf_shape_errors_docx_options() {
        let tree = markdown_to_tree("x\n");
        let cases: Vec<(&str, &str)> = vec![
            ("non object", r#"{"header": 5}"#),
            ("missing runs", r#"{"header": {}}"#),
            ("runs not array", r#"{"header": {"runs": 5}}"#),
            ("run not object", r#"{"footer": {"runs": ["str"]}}"#),
            (
                "run missing text",
                r#"{"header": {"runs": [{"bold": true}]}}"#,
            ),
            (
                "bad field value",
                r#"{"footer": {"runs": [{"field": "date"}]}}"#,
            ),
            (
                "text and field both",
                r#"{"header": {"runs": [{"text": "a", "field": "page"}]}}"#,
            ),
            (
                "bare true first_page",
                r#"{"header": {"runs": [], "first_page": true}}"#,
            ),
            (
                "unknown hf field",
                r#"{"header": {"runs": [], "margins": 1}}"#,
            ),
            (
                "unknown run field",
                r#"{"header": {"runs": [{"text": "a", "mono": true}]}}"#,
            ),
        ];
        for (name, json) in cases {
            let e =
                export_docx_styled_core(&tree, json).expect_err(&format!("{name}: must reject"));
            assert_eq!(e.code, "docx_options", "{name}: {e:?}");
        }
    }

    /// P1 #190-E5：空 runs 默认槽 = 空槽语义（发射空段落部件——
    /// 「首页留空、其余页有默认页眉」的 Word 惯用法合法面）；与
    /// 首页独立槽组合的真实用法。
    #[test]
    fn p1_hf_empty_default_slot_is_valid() {
        let tree = markdown_to_tree("x\n");
        // 首页页眉留空 + 其余页页眉有内容（Word 惯用法：封面页不显示页眉）
        let options = r#"{
            "header": {
                "runs": [{"text": "正文页眉"}],
                "first_page": {"runs": []}
            }
        }"#;
        let bytes = export_docx_styled_core(&tree, options).expect("empty first slot ok");
        let back = read_docx_from_bytes(&bytes).expect("read back");
        assert_eq!(back.headers.len(), 2, "默认 + 首页两槽");
        // 纯空默认槽（整档无页眉面——防御合法形）
        let bare_empty = export_docx_styled_core(&tree, r#"{"header": {"runs": []}}"#)
            .expect("bare empty slot ok");
        let back2 = read_docx_from_bytes(&bare_empty).expect("read back 2");
        assert_eq!(back2.headers.len(), 1, "空默认槽挂接");
    }

    // ------------------------------------------------------------ P1 #190：图片

    /// P1 #190 测试夹具：1×1 PNG（与 docx_import.rs `PNG_1X1` 同源字节
    /// ——office-io image() 已验证合法的最小 PNG）的 base64 形。
    const PNG_1X1_B64: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGIAAQAABQABDQottAAAAABJRU5ErkJggg==";

    /// P1 #190 测试夹具：images 资源表 JSON（单资源 cert.png @ 800×600）。
    fn images_options(extra: &str) -> String {
        format!(
            r#"{{
                "images": {{
                    "cert.png": {{
                        "png_base64": "{PNG_1X1_B64}",
                        "width_px": 800,
                        "height_px": 600
                    }}
                }}
                {extra}
            }}"#
        )
    }

    /// P1 #190-E6：Markdown 图片行 → docx 内嵌图片（验收④：`![alt](src)`
    /// 命中资源表 → word/media 部件 + 正文 a:blip 引用 + 读回 images
    /// 资产清单 body_uses ≥1）。
    #[test]
    fn p1_img_markdown_line_embeds_png() {
        let tree = markdown_to_tree("前文段落\n\n![资质证书](cert.png)\n\n后文段落\n");
        let bytes = export_docx_styled_core(&tree, &images_options("")).expect("image export");
        // 正文 document.xml：a:blip 引用 + extent 在场
        let doc = part_xml(&bytes, "word/document.xml");
        assert!(doc.contains("a:blip"), "内嵌图片引用在场");
        assert!(doc.contains("drawing"), "drawing 容器在场");
        // 前后文不受影响（图片行替换为图片，其余段落原样）
        let back = read_docx_from_bytes(&bytes).expect("read back");
        let texts: Vec<String> = paras(&back.blocks).iter().map(|p| p.text()).collect();
        assert!(texts.contains(&"前文段落".to_string()));
        assert!(texts.contains(&"后文段落".to_string()));
        // 图片行字面量不再以文本形态出现（被图片替换）
        assert!(
            !texts.iter().any(|t| t.contains("cert.png")),
            "图片行不入文本面"
        );
        // 读回资产清单（验收④全环：office-io 读面 images）——部件名
        // 经读回资产取（docx-rs 媒体 id 全局计数，跨测试并行序不钉死）
        assert_eq!(back.images.len(), 1, "媒体资产恰一");
        let asset = &back.images[0];
        assert!(
            asset.part_name.starts_with("word/media/") && asset.part_name.ends_with(".png"),
            "media 部件命名形: {}",
            asset.part_name
        );
        assert!(asset.body_uses >= 1, "正文引用计数 ≥1");
        assert_eq!(asset.bytes.len(), 67, "字节原样（1×1 PNG）");
        // 部件字节与读回资产一致（容器面双源核验）
        let part = mdc_office_io::docx::docx_part_bytes(&bytes, &asset.part_name)
            .expect("media part present");
        assert_eq!(part, asset.bytes, "zip 部件字节 = 读回资产字节");
    }

    /// P1 #190-E7：尺寸按 width_px/height_px 换算 EMU（验收⑤——引擎
    /// from_px = px × 9525 @96dpi：800→7620000、600→5715000）。
    #[test]
    fn p1_img_dimensions_emu_scaling() {
        let tree = markdown_to_tree("![](cert.png)\n");
        let bytes = export_docx_styled_core(&tree, &images_options("")).expect("export");
        let doc = part_xml(&bytes, "word/document.xml");
        assert!(
            doc.contains(r#"cx="7620000""#) && doc.contains(r#"cy="5715000""#),
            "EMU 尺寸在场（800×600 px → 7620000×5715000 EMU）: {doc}"
        );
    }

    /// P1 #190-E8：未命中 src 降级为字面量文本行（与无 images 键
    /// 逐字节一致——资源表供给与消费解耦：不命中零影响，字节稳定）。
    #[test]
    fn p1_img_miss_degrades_to_literal_text() {
        let tree = markdown_to_tree("![资质证书](other.png)\n");
        let with_table = export_docx_styled_core(&tree, &images_options(""))
            .expect("table present but src missed");
        let without = export_docx_styled_core(&tree, "{}").expect("no table");
        assert_eq!(
            with_table, without,
            "src 未命中 ≡ 无资源表（降级字面量，字节一致）"
        );
        // 降级文本可读回（字面量原样）
        let back = read_docx_from_bytes(&with_table).expect("read back");
        assert!(
            paras(&back.blocks)
                .iter()
                .any(|p| p.text() == "![资质证书](other.png)"),
            "字面量原样读回"
        );
        // 图片行识别的严格形：嵌套括号 / 缺 src / 非 `![` 前缀 → 文本行
        for md in ["![a](b(c).png)", "![]()", "文本 ![a](cert.png) 行中"] {
            let t = markdown_to_tree(md);
            let b = export_docx_styled_core(&t, &images_options("")).expect("strict form");
            assert!(read_docx_from_bytes(&b).is_ok(), "{md}: 降级产物结构完整");
        }
    }

    /// P1 #190-E9：资源表拒收面（验收⑥完备性——base64 坏 / 非 PNG
    /// 签名 / 零尺寸 / 未知字段 → `docx_options` 前置拒，发射零副作用）。
    #[test]
    fn p1_img_resource_table_rejects() {
        let tree = markdown_to_tree("![a](cert.png)\n");
        let cases: Vec<(&str, String)> = vec![
            (
                "bad base64 char",
                r#"{"images": {"cert.png": {"png_base64": "!!!not-base64!!!", "width_px": 1, "height_px": 1}}}"#.into(),
            ),
            (
                "bad base64 length",
                r#"{"images": {"cert.png": {"png_base64": "aGVsbG8", "width_px": 1, "height_px": 1}}}"#.into(),
            ),
            (
                "not png signature",
                r#"{"images": {"cert.png": {"png_base64": "aGVxsG8gd29ybGQh", "width_px": 1, "height_px": 1}}}"#.into(),
            ),
            (
                "zero width",
                format!(
                    r#"{{"images": {{"cert.png": {{"png_base64": "{PNG_1X1_B64}", "width_px": 0, "height_px": 1}}}}}}"#
                ),
            ),
            (
                "zero height",
                format!(
                    r#"{{"images": {{"cert.png": {{"png_base64": "{PNG_1X1_B64}", "width_px": 1, "height_px": 0}}}}}}"#
                ),
            ),
            (
                "unknown field",
                format!(
                    r#"{{"images": {{"cert.png": {{"png_base64": "{PNG_1X1_B64}", "width_px": 1, "height_px": 1, "alt": "x"}}}}}}"#
                ),
            ),
            (
                "missing base64",
                r#"{"images": {"cert.png": {"width_px": 1, "height_px": 1}}}"#.into(),
            ),
            (
                "images not object",
                r#"{"images": [1]}"#.into(),
            ),
        ];
        for (name, json) in cases {
            let e =
                export_docx_styled_core(&tree, &json).expect_err(&format!("{name}: must reject"));
            assert_eq!(e.code, "docx_options", "{name}: {e:?}");
        }
    }

    /// P1 #190-E10：estimate 面容忍 images 键（分页预估不参与——图片
    /// 高度面无通用估算口径，登记；options 形状与导出面单一真源共享）。
    #[test]
    fn p1_img_estimate_tolerates_images_key() {
        let tree = markdown_to_tree("![a](cert.png)\n段落。\n");
        let est = estimate_pages_core(&tree, &images_options("")).expect("estimate ok");
        assert!(est.contains("pages"), "预估出参结构完整: {est}");
        // 顶层白名单拒收面不受影响（images 必须是对象）
        let e = estimate_pages_core(&tree, r#"{"images": 5}"#).expect_err("must reject");
        assert_eq!(e.code, "docx_options");
    }
}
