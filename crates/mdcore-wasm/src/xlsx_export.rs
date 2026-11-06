//! t19：编辑器当前文档 → XLSX 导出切片（office-io 写面经 wasm 桥，
//! 与 t17 合流 = XLSX 往返闭环；office 消费三部曲 t16/t17/t18 收口）。
//!
//! **定位（2026-10-14，t19 立项）**：编辑器当前文档（mdcore
//! [`BlockTree`]——唯一语义真理，主档 §9）→ [`mdc_office_io::write`]
//! 的 [`WorkbookWriter`] 新建工作簿写面 → zip 容器字节 → web 侧
//! `Blob + a.download` 下载 `document.xlsx`。与 t17（XLSX 导入）合流
//! = XLSX 往返闭环；三域纪律唯一新边沿用 t16-t18 的 mdcore-wasm →
//! office-io（消费方边界；office-io 对 mdcore 零依赖不变）。
//!
//! ## 暴露面（[`WasmCore::export_xlsx`]——既有类增量方法，状态面零变化）
//!
//! - 入参：无（导出 `self` 当前块树——宿主编辑器文档本体）。
//! - 成功：XLSX 字节（wasm-bindgen `Vec<u8>` → JS `Uint8Array`）。
//! - 失败：抛 JSON 字符串 `{"code": "...", "message": "..."}`
//!   （t16-t18 错误面约定，宿主 `JSON.parse` 出结构化错误）。
//!
//! ## 错误码表
//!
//! | code | 含义 |
//! |---|---|
//! | `xlsx_build` | 引擎写/打包失败（[`WriteError::Engine`] = `save_to_vec` 打包失败为唯一现实路径；[`WriteError::InvalidInput`] = 表名/坐标/流式纪律/文本格式限 typed 面——本映射经命名清洗 + 防御性截断**构造不可达**，完备性仍映射，t18 `docx_build` 单族同构）。**映射层是全函数——降级永不报错**。 |
//! | `xlsx_limit` | **r4-t2 写面资源限**（office-io `MAX_CELLS`/`MAX_SHEETS`——逐写调用 typed 拒[write 面执行点]，超限导出**无产物字节**——错误路径即无返回值；常量单一真源在 office-io write.rs，本层零自立数字；`MAX_CELLS` = 1M < 2^20 保证资源限先于本模块 MAX_ROW 防御截断[静默]触发）。 |
//!
//! ## 栅格映射（XLSX = 栅格；线性 markdown 文档映射固有有损——诚实定界）
//!
//! | mdcore 块 | XLSX 落面 | 降级（不可表达面，显式登记） |
//! |---|---|---|
//! | `Table` | **每表一独立 sheet**（文档序）：首行 = 表头行原样、体行按序；每格 [`CellValue::Text`]，**反引号公式格例外 → [`CellValue::Formula`]**（#22 Q3-2：整体反引号包裹 + 内层以 `=` 开头——P2-3 导入面的对称标记，非类型推断） | 分隔行剔除 + `\|` 反转义 + 单元格 `\n`/`\r` → 空格（t18 `table_rows` 同一 GFM 语义真源共享）；**非公式格恒 Text——数值/布尔类型不推断**（前导零 `007`/日期式字符串会因推断失真；Text 面保证 t17 读回 Display 逐字等值 = 全环断言基础）；公式格缓存值不伪造（无计算引擎——读回 values 面 = 0.0，FIDELITY §1 在档；公式原文经公式通道往返）；不整齐行原样落栅格（无补齐——栅格天然容纳 ragged）；空文本格不落格（栅格等价，行索引照常）；0 行表跳过（防御——markdown 解析不产空表）；行 > [`MAX_ROW`]/列 > [`MAX_COL`] 防御截断（病理文档 2^20 行/2^14 列外不可达——映射恒全函数） |
//! | `Heading` | Document sheet 行：A 列单格 + 加粗（[`CellStyle::bold`]，style_probe 验证面） | **level 丢失**（h1-h6 同为加粗单格——无分级样式面）；**marks 丢弃**（单元格无富文本承载） |
//! | `Paragraph` | Document sheet 行：A 列单格纯文本 | **marks 全降级纯文本**（bold/em/code/del/u/link——单元格无富文本承载；文本逐字保留） |
//! | `CodeBlock` | Document sheet 行：每行单格逐字 | lang/fence/等宽面丢失（无单元格字体面） |
//! | `Blockquote` | 子块平铺递归（空引用按 text 发射——镜像 markdown.rs 序列化语义 L90-93，t18 同款） | 引用容器/缩进丢失 |
//! | `ListItem` | item 文本行 + 子块递归 | **marker/depth（列表结构）丢弃**（t18 同款登记；不注入 `- ` 伪 markdown） |
//! | `ThematicBreak` | 空行（不落格——栅格空行 = 分隔视觉提示） | 无水平线面 |
//!
//! ## Sheet 命名策略（write.rs 表名规则 = 非空/≤31 chars/禁 `[]:*?/\`/
//! 禁首尾单引号/禁保留名 History/禁重复——映射层清洗保证恒过 typed 验证）
//!
//! - **批 5 表名通道（#29 三大发现 #4/#5 修复）**：表块的**紧邻前驱
//!   Heading**（同一兄弟序列，未被他表认领，清洗后非空非 History）
//!   → 标题文本成为该表 sheet 名（去重），标题**不再落 Document 表**
//!   （单落）——导入面 `## {表名}` 节标题的对称回取：`## Sheet1` + 表
//!   → sheet `Sheet1` → 再导入 `## Sheet1` + 表（**表身份保真 + 往返
//!   收敛**——原 A1 派生形 `Sheet1`→`45306` 漂移与 Document 表逐轮
//!   注入膨胀[31→87 行 8 轮]根治）。不可认领（无前驱标题/空/保留名/
//!   已认领）→ 回退 A1 派生（下行）。
//! - **文本块存在 → 首表固定 `Document`**（文档线性内容的栅格落位，
//!   先建先占位；认领标题已剔除——仅未认领文本块）；纯表文档无
//!   Document 表。
//! - **表块名 = 表头 A1 格**清洗：`\n`/`\r`/`\t` → 空格；剥非法
//!   `[]:*?/\`；trim 空白与单引号；截 31 chars；重 trim（截断可留尾
//!   空白/引号）。清洗后空 / `History`（忽略大小写）→ 回退
//!   `Table {n}`（n = 本表发射序，1 起）。
//! - **去重**：与已建表精确冲突 → 后缀 ` 2`/` 3`…（基底重截保总长
//!   ≤31）；与 `Document` 冲突同规则（文本块在先，表退 `Document 2`）。
//! - **表序**：Document（若有）→ 表块文档序（先序遍历——嵌套于
//!   quote/list 的表同样收表；`Table` 自身 children 不下钻，t18
//!   `emit_block` 同款防御，op 面理论可达的表子块不导出）。
//! - **空文档 → 单空表 `Sheet1`**（最小合法工作簿——Excel 工作簿
//!   至少一表；t18「空树 = 最小合法 docx」同位面）。
//!
//! ## 已知边界（登记，非缺陷）
//!
//! - heading 加粗样式经写面落盘（styles.xml `<b/>`），读面（calamine）
//!   无样式可观测面——原生全环断言值面，样式面由 web e2e XML 级断言
//!   （style_probe 先例）；
//! - 富文本 marks 全降级纯文本（xlsx 单元格无内联富文本面——
//!   rust_xlsxwriter 0.99.1 富文本 API 不在 office-io 包装面，未实测
//!   不宣称）；
//! - 数值/布尔不推断（全 Text）——Excel 侧无数字类型语义（求值/排序
//!   按文本），全环文本零损与类型语义二选一，取文本零损（登记）。
//!   **r4-t1 裁决定谳（2026-10-17）**：本面源 = mdcore BlockTree（markdown
//!   文本模型，无单元格类型载荷）——「数字文本不擅自转数字」的用户
//!   裁决使从文本推断类型不可行（前导零 `007` 会因推断失真），全
//!   Text 维持为正确语义而非妥协。**类型保持写面在 office-io 层**：
//!   有类型源（[`mdc_office_io::io::CellData`]）的读-改-重建路径经
//!   `SheetData::replay_cell`（kind 分派 + numFmt 样式配对——office-io
//!   tests/cell_types.rs kind 往返钉定）；本面（BlockTree 文本源）无
//!   类型可派发，lang 语义边界在此登记（模型扩类型载荷 = mdcore 域
//!   上游裁决，未立项不宣称）；
//! - 合并/超链接/公式/样式（除 heading 加粗）不在映射面——模型无源
//!   （BlockTree 无此载荷，非降级而是无源可映射）。
//!
//! 纯内核（[`export_xlsx_core`]）不触碰 JsValue → 原生 cargo test
//! 直测（写→`read_book_from_bytes`→值面断言 + `export → t17
//! import_xlsx_core` 全环，t16-t18 同款写读全环纪律）；wasm 壳由
//! wasm32 check + web 套件门禁背书。

use crate::docx_export::{table_rows, text_lines};
use crate::WasmCore;
use mdc_office_io::write::{
    CellStyle, CellValue, FormulaCache, WorkbookWriter, WriteError, MAX_COL, MAX_ROW,
};
use mdcore::tree::{BlockKind, BlockTree, NodeId};
use std::collections::HashSet;
use wasm_bindgen::prelude::*;

/// Document 表名（文本块的栅格落位——固定名，模块文档命名策略）。
const DOC_SHEET: &str = "Document";

/// typed 导出错误（错误码 + 人类可读消息——见模块文档错误码表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
}

/// 引擎写面错误 → 错误码（r4-t2：资源限 `xlsx_limit` 与格式/引擎面
/// 可区分——见模块文档错误码表）。
fn map_xlsx(e: WriteError) -> ExportError {
    match e {
        WriteError::Limit(m) => ExportError {
            code: "xlsx_limit",
            message: format!("xlsx writer resource limit: {m}"),
        },
        other => ExportError {
            code: "xlsx_build",
            message: format!("xlsx writer engine error: {other}"),
        },
    }
}

// ---------------------------------------------------------------------------
// 纯映射内核（原生可测——BlockTree 直构，零 zip 依赖直到 save_to_vec）
// ---------------------------------------------------------------------------

/// 表头 A1 → 合法表名基底：控制/换行 → 空格；剥非法 `[]:*?/\`；
/// trim 空白 + 首尾单引号；截 31 chars；重 trim（截断可留尾空白/引号）。
fn sanitize_sheet_name(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            _ => c,
        })
        .collect();
    let stripped: String = mapped
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .collect();
    let taken: String = stripped
        .trim()
        .trim_matches('\'')
        .chars()
        .take(31)
        .collect();
    taken.trim().trim_matches('\'').trim().to_string()
}

/// 基底去重：精确冲突 → ` {n}` 后缀（n 从 2），基底重截保总长 ≤31
/// （31 = write.rs 表名上限，char 计）。
fn unique_sheet_name(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|n| n == base) {
        return base.to_string();
    }
    let mut n = 2usize;
    loop {
        let suffix = format!(" {n}");
        let keep = 31usize.saturating_sub(suffix.chars().count());
        let head: String = base.chars().take(keep).collect();
        let candidate = format!("{head}{suffix}");
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// 表头 A1 → 表名：清洗 → 空 / `History`（忽略大小写，保留名）→
/// `Table {ordinal}` 回退（ordinal = 本表发射序）；与 existing 去重。
fn table_sheet_name(header: &str, ordinal: usize, existing: &[String]) -> String {
    let base = sanitize_sheet_name(header);
    let base = if base.is_empty() || base.eq_ignore_ascii_case("history") {
        format!("Table {ordinal}")
    } else {
        base
    };
    unique_sheet_name(&base, existing)
}

/// Document 表行（A 列单格行模型）：text + 加粗标记（heading 行）。
struct TextRow {
    text: String,
    bold: bool,
}

/// 非表块 → Document 表行（先序递归；`Table` 不入——独立表通道；
/// `Table` children 同样不下钻，t18 emit_block 同款防御；**认领标题
/// 跳过**——批 5 表名通道：紧邻表的标题已进 sheet 名，再落 Document
/// 行 = 双落，正是往返膨胀根因[#29 三大发现 #4]）。
fn collect_text_rows(
    tree: &BlockTree,
    id: NodeId,
    rows: &mut Vec<TextRow>,
    skip: &HashSet<NodeId>,
) {
    if skip.contains(&id) {
        return; // 已被表认领为表名（不双落）
    }
    let node = tree.node(id);
    let text = node.text.to_string();
    match node.kind {
        BlockKind::Document => {
            // 虚根透明递归（top_level 已绕过；op 面禁止 Document 子块——防御）
            for &c in &node.children {
                collect_text_rows(tree, c, rows, skip);
            }
        }
        // 独立表通道（collect_tables），不混入 Document 表
        BlockKind::Table => {}
        BlockKind::Heading => {
            for line in text_lines(&text) {
                rows.push(TextRow {
                    text: line.to_string(),
                    bold: true,
                });
            }
        }
        BlockKind::Paragraph | BlockKind::CodeBlock | BlockKind::Math => {
            // #261：数学块同构 CodeBlock——LaTeX 源逐字行入表
            // （非 bold 通道；源码零丢失，公式语义降级登记）。
            for line in text_lines(&text) {
                rows.push(TextRow {
                    text: line.to_string(),
                    bold: false,
                });
            }
        }
        BlockKind::Blockquote => {
            // 空引用（无子）按 text 发射——镜像 markdown.rs 序列化语义
            if node.children.is_empty() {
                for line in text_lines(&text) {
                    rows.push(TextRow {
                        text: line.to_string(),
                        bold: false,
                    });
                }
            }
            for &c in &node.children {
                collect_text_rows(tree, c, rows, skip);
            }
        }
        BlockKind::ListItem => {
            // marker/depth 丢弃（模块文档登记）
            for line in text_lines(&text) {
                rows.push(TextRow {
                    text: line.to_string(),
                    bold: false,
                });
            }
            for &c in &node.children {
                collect_text_rows(tree, c, rows, skip);
            }
        }
        BlockKind::ThematicBreak => {
            // 空行（不落格）——无水平线面，分隔视觉提示
            rows.push(TextRow {
                text: String::new(),
                bold: false,
            });
        }
    }
}

/// 反引号公式格检测（#22 Q3-2 + 批 5 双载）：单元格为反引号包裹
/// 的公式原文（`` `=SUM(B2:B4)` `` 单载形——P2-3 导入无缓存面）或
/// **双载形**（`` `=SUM(B2:B4)`=9 ``——批 5 导入缓存值面追加）→
/// [`CellValue::Formula`]（真公式格——openpyxl 独立读出公式文本
/// 而非字面反引号串）。裁决链：
/// - 首/末反引号括号语义（`rfind`——内层可含反引号，嵌套不双解）；
/// - 内层 trim 后以 `=` 开头且长于 1（退化 `=` 单独不算）→ Formula
///   （写面规范化 = 前缀——io.rs FIDELITY §1 往返一致）；
/// - 双载后缀（末反引号后）：`=值` 形 → **缓存重建**（
///   [`parse_cache`] 显示面恒等重建——导入面追加的缓存值面写回
///   xlsx 缓存通道，非伪造）；后缀空 → 单载（cached None）；后缀
///   非 `=` 开头 → 整体字面 Text（防御）；
/// - 非公式反引号串（内联代码 `` `code` `` 等）保持 Text——
///   **非类型推断**：反引号 = 导入面显式标记，非猜测（"007"/
///   日期串恒 Text 的既有裁决不变）；
/// - 单层不双解（`` ``=x`` `` 内层不以 `=` 开头 → Text）。
fn cell_value(text: &str) -> CellValue {
    let Some(rest) = text.strip_prefix('`') else {
        return CellValue::Text(text.to_string());
    };
    // 末反引号 = 闭合界（单载形末字符；双载形值面前分隔界）。
    let Some(close) = rest.rfind('`') else {
        return CellValue::Text(text.to_string());
    };
    let inner = rest[..close].trim();
    if !(inner.starts_with('=') && inner.len() > 1) {
        return CellValue::Text(text.to_string());
    }
    let suffix = &rest[close + 1..];
    let cached = if suffix.is_empty() {
        None // 单载：无缓存面（不伪造——读回 values 面 = 0.0 在档）
    } else if let Some(v) = suffix.strip_prefix('=') {
        if v.is_empty() {
            // 裸 `=` 空值后缀 → 整体字面（防御——导入面不产此形）
            return CellValue::Text(text.to_string());
        }
        parse_cache(v) // 双载：缓存值面重建（显示面恒等）
    } else {
        // 非法后缀（非 `=` 开头非空）→ 整体字面（防御——导入面不产此形）
        return CellValue::Text(text.to_string());
    };
    CellValue::Formula {
        text: inner.to_string(),
        cached,
    }
}

/// 双载缓存值重建（批 5）：[`FormulaCache`] 重建链——导入面后缀 =
/// `CellData::display()` 显示面，重建经写面引擎序列化后读回的显示
/// 面必须逐字相同（往返精确的基础）：
/// - `true`/`false` → Boolean（引擎 t="b" 写 1/0，读回 Bool 显示
///   "true"/"false" 恒等）；
/// - f64 可解析（有限）→ Number——**引擎序列化裁决**：
///   `write_formula_cell` 对可解析数值串恒写数值面（无 t 属性），
///   规范形（`45306`/`4.5`/`-3`）读回显示逐字恒等；**非规范形**
///   （前导零 `007`/科学形 `1e5`）读回规范化（`7`/`100000`）——
///   引擎面降级登记（md 后缀 `=007` 往返后 `=7`），测试钉定；
/// - 错误码（`#DIV/0!` 等）→ Text——引擎 t="e" 写出，读回
///   `Data::Error` → 显示面逐字保留（错误缓存面完整往返，非降级）；
/// - 其余 → Text（t="str" 读回 String 逐字）；
/// - 空串 → None（防御——导入面不产空后缀）。
fn parse_cache(v: &str) -> Option<FormulaCache> {
    if v.is_empty() {
        return None;
    }
    if v == "true" {
        return Some(FormulaCache::Boolean(true));
    }
    if v == "false" {
        return Some(FormulaCache::Boolean(false));
    }
    if let Ok(n) = v.parse::<f64>() {
        if n.is_finite() {
            return Some(FormulaCache::Number(n));
        }
    }
    Some(FormulaCache::Text(v.to_string()))
}

/// 紧邻前驱 Heading 认领判定（批 5，#29 三大发现 #4/#5）：标题文本
/// 清洗后非空且非保留名 History → 可认领表名（导入面 `## {表名}` 节
/// 标题的对称回取——**表身份往返保真**：`## Sheet1` 节导出表名
/// `Sheet1`，再导入回 `## Sheet1`，不再漂移 `Sheet1`→`45306`）。
/// 空/保留名 → None（标题保持 Document 行，表名回退 A1 派生——
/// 保留名面 [`table_sheet_name`] 同款裁决）。
fn heading_claim_name(tree: &BlockTree, heading: NodeId) -> Option<String> {
    let text = tree.node(heading).text.to_string();
    let base = sanitize_sheet_name(&text);
    if base.is_empty() || base.eq_ignore_ascii_case("history") {
        return None;
    }
    Some(base)
}

/// 表块收集（先序文档序——嵌套于 quote/list 的表同样收；`Table`
/// 自身 children 不下钻，t18 同款防御）+ **紧邻前驱 Heading 认领**
/// （批 5 表名通道）：同一兄弟序列内 Table 的前一块为 Heading 且
/// 未被他表认领 → 标题文本成为该表 sheet 名（标题从 Document 行
/// 收集中剔除——单落）。输出 (表 NodeId, 认领名) 对 + 被认领标题集。
fn collect_tables_claiming(
    tree: &BlockTree,
    children: &[NodeId],
    out: &mut Vec<(NodeId, Option<String>)>,
    claimed: &mut Vec<NodeId>,
) {
    for (i, &c) in children.iter().enumerate() {
        let node = tree.node(c);
        if node.kind == BlockKind::Table {
            let prev = if i > 0 { Some(children[i - 1]) } else { None };
            let claim = prev
                .filter(|&p| tree.node(p).kind == BlockKind::Heading && !claimed.contains(&p))
                .and_then(|p| heading_claim_name(tree, p).map(|n| (p, n)));
            match claim {
                Some((heading, name)) => {
                    claimed.push(heading);
                    out.push((c, Some(name)));
                }
                None => out.push((c, None)),
            }
            continue;
        }
        collect_tables_claiming(tree, &node.children, out, claimed);
    }
}

/// 写 Document 表：行 0 起顺序、A 列单格；heading 行加粗（值面读回
/// 无样式观测——XML 级由 web e2e 断言）；空文本行不落格（行索引照常
/// 推进——栅格等价）；防御性 [`MAX_ROW`] 截断（登记）。
fn write_document_sheet(w: &mut WorkbookWriter, rows: &[TextRow]) -> Result<(), ExportError> {
    let bold = CellStyle {
        bold: true,
        ..Default::default()
    };
    {
        let mut s = w.add_sheet(DOC_SHEET).map_err(map_xlsx)?;
        for (r, row) in rows.iter().enumerate() {
            if r as u64 > MAX_ROW as u64 {
                break; // 防御截断（病理文档 2^20 行外——映射恒全函数）
            }
            if row.text.is_empty() {
                continue; // 空行不落格
            }
            let style = if row.bold { Some(&bold) } else { None };
            s.write(r as u32, 0, cell_value(&row.text), style)
                .map_err(map_xlsx)?;
        }
    }
    Ok(())
}

/// 写表块 sheet：首行 = 表头行原样、体行按序；全部格 Text（模块文档
/// 映射表——类型不推断）；空文本格不落格；防御性 MAX_ROW/MAX_COL
/// 截断（登记）。
fn write_table_sheet(
    w: &mut WorkbookWriter,
    name: &str,
    rows: &[Vec<String>],
) -> Result<(), ExportError> {
    {
        let mut s = w.add_sheet(name).map_err(map_xlsx)?;
        for (r, cells) in rows.iter().enumerate() {
            if r as u64 > MAX_ROW as u64 {
                break; // 防御截断（登记）
            }
            for (c, cell) in cells.iter().enumerate() {
                if c > MAX_COL as usize {
                    break; // 防御截断：列 2^14 外（16384 格/行内）
                }
                if cell.is_empty() {
                    continue; // 空格不落格
                }
                s.write(r as u32, c as u16, cell_value(cell), None)
                    .map_err(map_xlsx)?;
            }
        }
    }
    Ok(())
}

/// 编辑器核心块树 → XLSX 字节（zip 容器）。映射全函数：不可表达面
/// 显式降级（模块文档映射表），映射层永不报错；错误面 = 引擎写/打包。
pub fn export_xlsx_core(tree: &BlockTree) -> Result<Vec<u8>, ExportError> {
    let mut w = WorkbookWriter::new();
    let mut existing: Vec<String> = Vec::new();

    // 1) 表块收集（文档序）+ 紧邻前驱 Heading 认领（批 5 表名通道
    //    ——表身份保真 + Document 膨胀根因剔除）。
    let mut tables: Vec<(NodeId, Option<String>)> = Vec::new();
    let mut claimed: Vec<NodeId> = Vec::new();
    collect_tables_claiming(tree, tree.top_level(), &mut tables, &mut claimed);
    let claimed_set: HashSet<NodeId> = claimed.into_iter().collect();

    // 2) 非表块（认领标题除外）→ 前置 Document 表（存在才建，先建先占名）。
    let mut text_rows: Vec<TextRow> = Vec::new();
    for &child in tree.top_level() {
        collect_text_rows(tree, child, &mut text_rows, &claimed_set);
    }
    if !text_rows.is_empty() {
        write_document_sheet(&mut w, &text_rows)?;
        existing.push(DOC_SHEET.to_string());
    }

    // 3) 表块 → 独立表（文档序）；认领名（去重）优先，回退表头 A1
    //    派生；0 行表跳过（防御，登记）。
    for (i, (id, claim)) in tables.iter().enumerate() {
        let text = tree.node(*id).text.to_string();
        let rows = table_rows(&text);
        if rows.is_empty() {
            continue;
        }
        let ordinal = i + 1;
        let name = match claim {
            Some(base) => unique_sheet_name(base, &existing),
            None => {
                let header = rows[0].first().map(String::as_str).unwrap_or("");
                table_sheet_name(header, ordinal, &existing)
            }
        };
        existing.push(name.clone());
        write_table_sheet(&mut w, &name, &rows)?;
    }

    // 4) 全零 → 最小合法工作簿（单空表 Sheet1——Excel 工作簿至少一表）。
    if existing.is_empty() {
        let _ = w.add_sheet("Sheet1").map_err(map_xlsx)?;
    }

    w.save_to_vec().map_err(map_xlsx)
}

// ---------------------------------------------------------------------------
// wasm 壳（WasmCore 增量方法——既有类面零变化，t18 export_docx 同款；
// 本 crate 内 impl 分文件合法）
// ---------------------------------------------------------------------------

#[wasm_bindgen]
impl WasmCore {
    /// 当前文档 → XLSX 字节（JS `Uint8Array`）。失败 = 抛 JSON 串
    /// `{"code":"xlsx_build","message":"..."}`（t16-t18 错误面约定）。
    #[wasm_bindgen]
    pub fn export_xlsx(&self) -> Result<Vec<u8>, JsValue> {
        export_xlsx_core(&self.inner.tree).map_err(|e| {
            JsValue::from_str(
                &serde_json::json!({ "code": e.code, "message": e.message }).to_string(),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// 原生测试（写 → read_book_from_bytes → 值面断言 + export → t17
// import_xlsx_core 全环；t16-t18 写读全环纪律）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xlsx_import::import_xlsx_core;
    use mdc_office_io::io::{read_book_from_bytes, SheetData};
    use mdcore::markdown::markdown_to_tree;
    use mdcore::tree::{Attrs, MarkSpan};

    /// 直构单块树（t18 测试助手同款——attrs/marks 精确控制）。
    fn single(kind: BlockKind, text: &str, attrs: Attrs, marks: Vec<MarkSpan>) -> BlockTree {
        let mut tree = BlockTree::new();
        let id = tree.create_node(kind, text, attrs, marks);
        let root = tree.root();
        let len = tree.node(root).children.len();
        tree.insert_child(root, len, id).expect("test attach");
        tree
    }

    /// 写→读全环：字节必须过 office-io 读面（容器门 + 提取）。
    /// 返回 Book（sheets 值面断言用）。
    fn roundtrip(tree: &BlockTree) -> mdc_office_io::io::Book {
        let bytes = export_xlsx_core(tree).expect("export");
        assert!(
            bytes.len() > 4 && &bytes[..2] == b"PK",
            "zip local header magic, {} bytes",
            bytes.len()
        );
        read_book_from_bytes(&bytes).expect("read back")
    }

    /// 值面单格访问（Display 值面——不具名 calamine 类型，读面
    /// Data::Empty/缺格 → ""，与 t17 Display 消费同面）。
    /// 坐标 = 绝对（0 基）——按 values.start() 锚点换算相对。
    fn cell(sheet: &SheetData, r: usize, c: usize) -> String {
        let s = sheet.values.start().unwrap_or((0, 0));
        sheet
            .values
            .get((r - s.0 as usize, c - s.1 as usize))
            .map(|d| d.to_string())
            .unwrap_or_default()
    }

    /// 值面单格 Debug 形态（类型判别断言——all-Text 证实用，同样不
    /// 具名 calamine 类型）。
    fn cell_debug(sheet: &SheetData, r: usize, c: usize) -> String {
        let s = sheet.values.start().unwrap_or((0, 0));
        format!(
            "{:?}",
            sheet.values.get((r - s.0 as usize, c - s.1 as usize))
        )
    }

    #[test]
    fn table_block_maps_to_sheet_grid_exact() {
        let tree = markdown_to_tree("| alpha | beta |\n| --- | --- |\n| 1 | 2 |");
        assert_eq!(tree.top_level().len(), 1, "单表块");
        let book = roundtrip(&tree);
        assert_eq!(book.sheet_names(), vec!["alpha"], "表名 = 表头 A1");
        let s = &book.sheets[0];
        assert_eq!((s.values.height(), s.values.width()), (2, 2), "2 行 × 2 列");
        assert_eq!(cell(s, 0, 0), "alpha");
        assert_eq!(cell(s, 0, 1), "beta");
        assert_eq!(cell(s, 1, 0), "1");
        assert_eq!(cell(s, 1, 1), "2", "分隔行剔除（GFM 语义位）");
    }

    /// #22 Q3-2：反引号公式格 → 真公式格（P2-3 导入标记的对称写面
    /// ——公式通道往返；读回不带 `=` 前缀 = calamine 行为 FIDELITY §1）。
    #[test]
    fn backtick_formula_maps_to_formula_channel() {
        let tree = markdown_to_tree("| a | b |\n| --- | --- |\n| `=SUM(B2:B4)` | `=1+1` |");
        let book = roundtrip(&tree);
        let s = &book.sheets[0];
        assert_eq!(s.formula_at(1, 0), Some("SUM(B2:B4)"), "公式原文经公式通道");
        assert_eq!(s.formula_at(1, 1), Some("1+1"));
        // 值面：cached None → Float(0.0)（不伪造缓存——写面在档行为）。
        assert_eq!(cell_debug(s, 1, 0), "Some(Float(0.0))");
        // 表头恒 Text（反引号才触发，非位置性推断）。
        assert_eq!(cell(s, 0, 0), "a");
        assert!(s.formula_at(0, 0).is_none());
    }

    /// #22 Q3-2：检测矩阵——非公式反引号（内联代码）/单层不双解/
    /// 退化 `=`/空内层/裸 `=` 串/前导零 全部恒 Text（非类型推断裁决）。
    #[test]
    fn backtick_formula_detection_matrix() {
        use mdc_office_io::write::CellValue;
        // 公式形（含内层空白 trim）。
        for text in ["`=SUM(B2:B4)`", "` =SUM(B2:B4) `", "`=1+1`"] {
            assert!(
                matches!(cell_value(text), CellValue::Formula { .. }),
                "{text} 应为公式格"
            );
        }
        match cell_value("`=SUM(B2:B4)`") {
            CellValue::Formula { text, cached } => {
                assert_eq!(text, "=SUM(B2:B4)");
                assert!(cached.is_none(), "缓存值不伪造（无计算引擎）");
            }
            _ => panic!("公式形"),
        }
        // Text 恒等面。
        for text in ["plain", "`code`", "``=x``", "`=`", "``", "=SUM(x)", "007"] {
            assert!(
                matches!(cell_value(text), CellValue::Text(t) if t == text),
                "{text} 应保持 Text 逐字"
            );
        }
    }

    /// 批 5 双载检测矩阵：`` `=公式`=缓存 `` 形 → Formula + 缓存重建
    /// （显示面恒等——[#parse_cache] 裁决链）；非法后缀整体字面（防御）。
    #[test]
    fn dual_load_formula_detection_matrix() {
        use mdc_office_io::write::{CellValue, FormulaCache};
        // 数值重建（整数/小数/负数——重格式化逐字相同）。
        for (text, want) in [
            ("`=SUM(B2:B4)`=9", 9.0),
            ("`=SUM(B2:B4)`=45306.5", 45306.5),
            ("`=A1-B1`=-3", -3.0),
        ] {
            match cell_value(text) {
                CellValue::Formula {
                    text: f,
                    cached: Some(FormulaCache::Number(n)),
                } => {
                    assert_eq!(f, text.split('`').nth(1).unwrap(), "{text} 公式原文");
                    assert_eq!(n, want, "{text} 缓存数值");
                }
                other => panic!("{text} 应为双载 Formula(Number): {other:?}"),
            }
        }
        // 布尔重建。
        for (text, want) in [("`=A1>B1`=true", true), ("`=A1>B1`=false", false)] {
            match cell_value(text) {
                CellValue::Formula {
                    cached: Some(FormulaCache::Boolean(b)),
                    ..
                } => assert_eq!(b, want, "{text}"),
                other => panic!("{text} 应为双载 Formula(Boolean): {other:?}"),
            }
        }
        // 前导零 → Number 缓存（引擎序列化裁决：数值可解析串恒写数值
        // 面——Text 形也会被引擎写为数值，Number 为诚实模型；非规范形
        // 读回规范化 `=7` 为已登记引擎面降级，见 [#parse_cache]）。
        match cell_value("`=LEN(A1)`=007") {
            CellValue::Formula {
                cached: Some(FormulaCache::Number(n)),
                ..
            } => assert_eq!(n, 7.0, "前导零 → Number（引擎面恒数值化）"),
            other => panic!("`=LEN(A1)`=007 应为 Number 缓存: {other:?}"),
        }
        // 错误码 → Text 缓存（FormulaCache 无 Error 变体——显示面逐字
        // 保留，类型面降级登记）。
        match cell_value("`=5/0`=#DIV/0!") {
            CellValue::Formula {
                cached: Some(FormulaCache::Text(s)),
                ..
            } => assert_eq!(s, "#DIV/0!", "错误码逐字 Text 缓存"),
            other => panic!("错误码双载应为 Text 缓存: {other:?}"),
        }
        // 文本缓存（任意非数值布尔串）。
        match cell_value("`=A1&B1`=hello") {
            CellValue::Formula {
                cached: Some(FormulaCache::Text(s)),
                ..
            } => assert_eq!(s, "hello"),
            other => panic!("文本缓存: {other:?}"),
        }
        // 非法后缀（非 `=` 开头非空 / 裸 `=` 空值）→ 整体字面 Text。
        for text in ["`=SUM(x)`abc", "`=SUM(x)`=", "`code`=5"] {
            assert!(
                matches!(cell_value(text), CellValue::Text(t) if t == text),
                "{text} 非法后缀应整体 Text 逐字"
            );
        }
        // 内层含反引号的公式：首/末反引号括号语义（rfind）不双解。
        match cell_value("`=CONCAT(\"`\",1)`=a") {
            CellValue::Formula { text, cached } => {
                assert_eq!(text, "=CONCAT(\"`\",1)");
                assert!(matches!(cached, Some(FormulaCache::Text(_))));
            }
            other => panic!("嵌套反引号双载: {other:?}"),
        }
    }

    /// 批 5 双载写读全环：公式格 + 缓存值 → xlsx 缓存通道 → t17 读回
    /// 双载形逐字还原（缓存面不伪造也不丢失——原 `cached None →
    /// Float(0.0)` 降级面由双载通道根治）。非规范数值串（`007`）经
    /// 引擎序列化规范化（`=7`——引擎面降级登记，[#parse_cache]）。
    #[test]
    fn dual_load_formula_roundtrip_via_t17() {
        let md =
            "| a | b | c |\n| --- | --- | --- |\n| `=SUM(B2:B4)`=9 | `=X`=hello | `=5/0`=#DIV/0! |";
        let tree = markdown_to_tree(md);
        let bytes = export_xlsx_core(&tree).expect("export");
        let payload = import_xlsx_core(&bytes).expect("t17 reimport");
        let json: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(
            json["markdown"].as_str().unwrap(),
            "## a\n\n| a | b | c |\n| --- | --- | --- |\n| `=SUM(B2:B4)`=9 | `=X`=hello | `=5/0`=#DIV/0! |",
            "双载形全环逐字（数值/文本/错误码三缓存面：Number+Text+t=e 错误链完整往返）"
        );

        // 引擎面降级钉定：前导零数值串读回规范化（007 → 7）。
        let md = "| a |\n| --- |\n| `=LEN(A1)`=007 |";
        let tree = markdown_to_tree(md);
        let bytes = export_xlsx_core(&tree).expect("export");
        let payload = import_xlsx_core(&bytes).expect("t17 reimport");
        let json: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(
            json["markdown"].as_str().unwrap(),
            "## a\n\n| a |\n| --- |\n| `=LEN(A1)`=7 |",
            "非规范数值串引擎面降级（登记非缺陷——write_formula_cell 数值化）"
        );
    }

    /// 批 5 表名通道（#29 三大发现 #4/#5）：紧邻前驱 Heading 认领为
    /// sheet 名 + 标题不双落 Document；认领排除面（隔块/保留名/已被
    /// 认领）回退 A1 派生。
    #[test]
    fn heading_claims_adjacent_table_sheet_name() {
        // 紧邻标题+表 → 表名 = 标题文本，无 Document 表（单落）。
        let book = roundtrip(&markdown_to_tree(
            "## 报价明细\n\n| 序号 | 金额 |\n| --- | --- |\n| 1 | 2 |",
        ));
        assert_eq!(
            book.sheet_names(),
            vec!["报价明细"],
            "紧邻标题认领表名（非 A1 派生），无 Document 表"
        );
        assert_eq!(cell(&book.sheets[0], 0, 0), "序号", "表内容不受认领影响");

        // 标题+段落+表（隔块）→ 不认领：Document 表 + A1 派生。
        let book = roundtrip(&markdown_to_tree(
            "## Title\n\nparagraph between\n\n| Data | b |\n| --- | --- |",
        ));
        assert_eq!(
            book.sheet_names(),
            vec!["Document", "Data"],
            "隔块不认领——Document + A1"
        );

        // 保留名 History → 不可认领（标题落 Document，表名回退 A1）。
        let book = roundtrip(&markdown_to_tree(
            "## History\n\n| Data | b |\n| --- | --- |",
        ));
        assert_eq!(
            book.sheet_names(),
            vec!["Document", "Data"],
            "History 保留名不认领"
        );

        // 一标题两表：首表认领，次表 A1（前驱兄弟是 Table 非 Heading）。
        let book = roundtrip(&markdown_to_tree(
            "## Solo\n\n| a | b |\n| --- | --- |\n\n| c | d |\n| --- | --- |",
        ));
        assert_eq!(
            book.sheet_names(),
            vec!["Solo", "c"],
            "认领一次性（不共享）"
        );

        // 同名两标题两表 → 去重后缀。
        let book = roundtrip(&markdown_to_tree(
            "## Data\n\n| a | b |\n| --- | --- |\n\n## Data\n\n| c | d |\n| --- | --- |",
        ));
        assert_eq!(book.sheet_names(), vec!["Data", "Data 2"], "认领名去重");
    }

    /// 批 5 往返收敛（#29 三大发现 #4：8 轮 31→87 行膨胀根治）：
    /// 纯表文档首轮即精确；混排文档二轮定点；多轮行数单调不增。
    #[test]
    fn roundtrip_converges_with_heading_named_tables() {
        // 情形 A：标题 + 表（导入面产出的标准形）→ 首轮精确。
        let md1 = "## Data\n\n| a | b |\n| --- | --- |\n| 1 | 2 |";
        let mut md = md1.to_string();
        for round in 1..=8 {
            let tree = markdown_to_tree(&md);
            let bytes = export_xlsx_core(&tree).expect("export");
            let payload = import_xlsx_core(&bytes).expect("reimport");
            let next: String = serde_json::from_str::<serde_json::Value>(&payload).unwrap()
                ["markdown"]
                .as_str()
                .unwrap()
                .to_string();
            if round == 1 {
                assert_eq!(next, md1, "标题+表 首轮精确（表名通道对称）");
            }
            assert_eq!(next, md, "第 {round} 轮定点");
            md = next;
        }

        // 情形 B：段落 + 标题 + 表（混排）→ 首轮 Document 前置重排
        //（有界增长——表头/分隔行入表代价），二轮起定点，后续轮行数
        // 恒等（无界膨胀[31→87 行 8 轮]根治）。
        let md1 = "intro para\n\n## Data\n\n| a | b |\n| --- | --- |\n| 1 | 2 |";
        let mut md = md1.to_string();
        let mut lines = md.lines().count();
        let mut peak = lines;
        for round in 1..=8 {
            let tree = markdown_to_tree(&md);
            let bytes = export_xlsx_core(&tree).expect("export");
            let payload = import_xlsx_core(&bytes).expect("reimport");
            let next: String = serde_json::from_str::<serde_json::Value>(&payload).unwrap()
                ["markdown"]
                .as_str()
                .unwrap()
                .to_string();
            let next_lines = next.lines().count();
            if round >= 2 {
                assert_eq!(
                    next_lines, lines,
                    "第 {round} 轮行数漂移（不收敛）: {lines} → {next_lines}"
                );
                assert_eq!(next, md, "第 {round} 轮定点（二轮收敛）");
            }
            peak = peak.max(next_lines);
            lines = next_lines;
            md = next;
        }
        assert_ne!(md, md1, "混排首轮重排（Document 前置——已知结构差异）");
        assert_eq!(
            md.lines().count(),
            10,
            "定点行数 = Document 节(标题+空+表头+分隔) + 节间空行 + Data 节(标题+空+表头+分隔+数据)"
        );
        assert_eq!(peak, 10, "峰值有界（首轮重排即峰值，后续恒等）");
    }

    #[test]
    fn sheet_name_policy_branches() {
        // 长表头 > 31 chars → 截 31。
        let long = "L".repeat(40);
        let md_long = format!("| {long} | b |\n| --- | --- |\n| 1 | 2 |");
        let book = roundtrip(&markdown_to_tree(&md_long));
        assert_eq!(book.sheet_names(), vec![long[..31].to_string()]);

        // 非法字符剥除 + History 保留名回退。
        let book = roundtrip(&markdown_to_tree("| bad[name:*?/\\x | b |\n| --- | --- |"));
        assert_eq!(book.sheet_names(), vec!["badnamex"], "非法 `[]:*?/\\` 剥除");

        let book = roundtrip(&markdown_to_tree("| History | b |\n| --- | --- |"));
        assert_eq!(book.sheet_names(), vec!["Table 1"], "保留名 → 回退");

        // 空表头 → 回退。
        let book = roundtrip(&markdown_to_tree("|  | b |\n| --- | --- |"));
        assert_eq!(book.sheet_names(), vec!["Table 1"], "空 A1 → 回退");

        // 重复表头 → 去重后缀。
        let book = roundtrip(&markdown_to_tree(
            "| Data | b |\n| --- | --- |\n\n| Data | c |\n| --- | --- |",
        ));
        assert_eq!(book.sheet_names(), vec!["Data", "Data 2"], "精确冲突去重");

        // 与 Document 冲突：文本块在先占名，表退 Document 2。
        let book = roundtrip(&markdown_to_tree(
            "para text\n\n| Document | b |\n| --- | --- |",
        ));
        assert_eq!(
            book.sheet_names(),
            vec!["Document", "Document 2"],
            "Document 先占位，表块同名去重"
        );
    }

    #[test]
    fn text_blocks_map_to_document_sheet_in_order() {
        let tree =
            markdown_to_tree("# Title\n\nparagraph\n\n> quoted\n\n- item\n  - nested\n\n---");
        let book = roundtrip(&tree);
        assert_eq!(book.sheet_names(), vec!["Document"], "文本块 → 单一前置表");
        let s = &book.sheets[0];
        // 行序 = 文档序平铺；divider 行空（不落格）→ 使用区 6 行实体。
        let rows: Vec<String> = (0..6).map(|r| cell(s, r, 0)).collect();
        assert_eq!(
            rows,
            vec![
                "Title".to_string(),
                "paragraph".to_string(),
                "quoted".to_string(),
                "item".to_string(),
                "nested".to_string(),
                String::new(),
            ],
            "heading/paragraph/quote/list(嵌套)/divider 平铺保序，divider = 空行"
        );
    }

    #[test]
    fn code_block_lines_verbatim_in_document_sheet() {
        let tree = markdown_to_tree("```rust\nfn main() {}\nlet x = 1;\n```");
        let book = roundtrip(&tree);
        let s = &book.sheets[0];
        assert_eq!(cell(s, 0, 0), "fn main() {}");
        assert_eq!(cell(s, 1, 0), "let x = 1;", "逐字行（lang/fence 丢弃）");
    }

    #[test]
    fn all_cells_text_typed_roundtrip() {
        // 数值形/布尔形/前导零文本全部 Text（类型不推断——模块文档登记）。
        let tree =
            markdown_to_tree("| name | qty | code |\n| --- | --- | --- |\n| Widget | 4.5 | 007 |");
        let book = roundtrip(&tree);
        let s = &book.sheets[0];
        assert_eq!(cell(s, 1, 1), "4.5");
        assert_eq!(
            cell_debug(s, 1, 1),
            "Some(String(\"4.5\"))",
            "4.5 = 文本格非数字格"
        );
        assert_eq!(cell_debug(s, 1, 2), "Some(String(\"007\"))", "前导零零损");
    }

    #[test]
    fn escaped_pipe_cell_roundtrip() {
        let tree = markdown_to_tree("| a | b \\| c |\n| --- | --- |\n| 1 | 2 |");
        let book = roundtrip(&tree);
        let s = &book.sheets[0];
        assert_eq!(cell(s, 0, 1), "b | c", "\\| 反转义 + 文本面往返");
    }

    #[test]
    fn ragged_rows_no_padding() {
        let tree = markdown_to_tree("| a | b | c |\n| --- | --- | --- |\n| 1 |");
        let book = roundtrip(&tree);
        let s = &book.sheets[0];
        assert_eq!(cell(s, 0, 2), "c");
        assert_eq!(cell(s, 1, 0), "1");
        assert_eq!(cell(s, 1, 1), "", "缺格 = 空（无补齐）");
        assert_eq!(cell(s, 1, 2), "");
    }

    #[test]
    fn empty_tree_yields_minimal_workbook() {
        let tree = BlockTree::new();
        let bytes = export_xlsx_core(&tree).expect("export empty");
        assert!(bytes.len() > 4 && &bytes[..2] == b"PK");
        let book = read_book_from_bytes(&bytes).expect("read back");
        assert_eq!(book.sheet_names(), vec!["Sheet1"], "空文档 = 单空表");
        assert_eq!(book.sheets[0].values.height(), 0, "零值面");
    }

    #[test]
    fn only_tables_no_document_sheet() {
        let tree = markdown_to_tree("| one | x |\n| --- | --- |\n\n| two | y |\n| --- | --- |");
        let book = roundtrip(&tree);
        assert_eq!(
            book.sheet_names(),
            vec!["one", "two"],
            "纯表文档无 Document 表"
        );
    }

    #[test]
    fn wide_row_defensive_col_truncation() {
        // 16386 实体格单行（> MAX_COL+1=16384）→ 防御截断为前 16384 格
        //（映射恒全函数——越界格丢弃，模块文档登记）。
        let wide: Vec<&str> = vec!["a"; 16_386];
        let line = format!("| {} |", wide.join(" | "));
        let tree = single(BlockKind::Table, &line, Attrs::new(), vec![]);
        let book = roundtrip(&tree);
        let s = &book.sheets[0];
        assert_eq!(s.values.width(), 16_384, "列截断至 2^14 格");
        assert_eq!(cell(s, 0, 0), "a");
        assert_eq!(cell(s, 0, 16_383), "a", "截断保留前 16384 格");
    }

    #[test]
    fn full_circle_via_t17_import_table_only() {
        // 全环：mdcore 表 → XLSX 字节 → t17 import_xlsx_core → markdown。
        // 表数据 GFM 形态逐字还原（t17 表名节 + 表格；三步曲合拢证据）。
        let md = "| alpha | beta |\n| --- | --- |\n| 1 | 2 |";
        let tree = markdown_to_tree(md);
        let bytes = export_xlsx_core(&tree).expect("export");
        let payload = import_xlsx_core(&bytes).expect("t17 reimport");
        let json: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(
            json["markdown"].as_str().unwrap(),
            "## alpha\n\n| alpha | beta |\n| --- | --- |\n| 1 | 2 |",
            "表名节 + GFM 表逐字等值（all-Text 值面 Display 往返）"
        );
    }

    #[test]
    fn full_circle_via_t17_import_document_sheet() {
        // 文本块全环：Document 表 → t17 导入 → 表名节 + 单列表。
        let md = "# Title\n\npara text";
        let tree = markdown_to_tree(md);
        let bytes = export_xlsx_core(&tree).expect("export");
        let payload = import_xlsx_core(&bytes).expect("t17 reimport");
        let json: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(
            json["markdown"].as_str().unwrap(),
            "## Document\n\n| Title |\n| --- |\n| para text |",
            "heading level 降级登记下文本内容 GFM 形态等值"
        );
    }

    /// r4-t2：资源限错误码可区分——`xlsx_limit`（表数/格预算，office-io
    /// `MAX_SHEETS`/`MAX_CELLS`）≠ `xlsx_build`（InvalidInput/Engine
    /// 既有单族——映射层全函数降级永不报错，完备性映射不变）。
    /// 常量单一真源在 office-io write.rs，本层零自立数字。
    #[test]
    fn limit_error_code_mapping() {
        let e = map_xlsx(mdc_office_io::write::WriteError::Limit(
            "sheet count over limit 256".into(),
        ));
        assert_eq!(e.code, "xlsx_limit");
        assert!(
            e.message.starts_with("xlsx writer resource limit"),
            "{:?}",
            e.message
        );
        // 既有面不动：InvalidInput/Engine 仍归 xlsx_build 单族。
        assert_eq!(
            map_xlsx(mdc_office_io::write::WriteError::InvalidInput("x".into())).code,
            "xlsx_build"
        );
        assert_eq!(
            map_xlsx(mdc_office_io::write::WriteError::Engine("x".into())).code,
            "xlsx_build"
        );
    }
}
