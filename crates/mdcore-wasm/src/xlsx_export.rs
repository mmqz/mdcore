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
//! | `Table` | **每表一独立 sheet**（文档序）：首行 = 表头行原样、体行按序；每格 [`CellValue::Text`] | 分隔行剔除 + `\|` 反转义 + 单元格 `\n`/`\r` → 空格（t18 `table_rows` 同一 GFM 语义真源共享）；**全部格 = Text——数值/布尔类型不推断**（前导零 `007`/日期式字符串会因推断失真；Text 面保证 t17 读回 Display 逐字等值 = 全环断言基础）；不整齐行原样落栅格（无补齐——栅格天然容纳 ragged）；空文本格不落格（栅格等价，行索引照常）；0 行表跳过（防御——markdown 解析不产空表）；行 > [`MAX_ROW`]/列 > [`MAX_COL`] 防御截断（病理文档 2^20 行/2^14 列外不可达——映射恒全函数） |
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
//! - **文本块存在 → 首表固定 `Document`**（文档线性内容的栅格落位，
//!   先建先占位）；纯表文档无 Document 表。
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
use mdc_office_io::write::{CellStyle, CellValue, WorkbookWriter, WriteError, MAX_COL, MAX_ROW};
use mdcore::tree::{BlockKind, BlockTree, NodeId};
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
/// `Table` children 同样不下钻，t18 emit_block 同款防御）。
fn collect_text_rows(tree: &BlockTree, id: NodeId, rows: &mut Vec<TextRow>) {
    let node = tree.node(id);
    let text = node.text.to_string();
    match node.kind {
        BlockKind::Document => {
            // 虚根透明递归（top_level 已绕过；op 面禁止 Document 子块——防御）
            for &c in &node.children {
                collect_text_rows(tree, c, rows);
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
        BlockKind::Paragraph | BlockKind::CodeBlock => {
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
                collect_text_rows(tree, c, rows);
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
                collect_text_rows(tree, c, rows);
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

/// 表块收集（先序文档序——嵌套于 quote/list 的表同样收；`Table`
/// 自身 children 不下钻，t18 同款防御）。
fn collect_tables(tree: &BlockTree, id: NodeId, out: &mut Vec<NodeId>) {
    let node = tree.node(id);
    if node.kind == BlockKind::Table {
        out.push(id);
        return;
    }
    for &c in &node.children {
        collect_tables(tree, c, out);
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
            s.write(r as u32, 0, CellValue::Text(row.text.clone()), style)
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
                s.write(r as u32, c as u16, CellValue::Text(cell.clone()), None)
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

    // 1) 非表块 → 前置 Document 表（存在才建，先建先占名）。
    let mut text_rows: Vec<TextRow> = Vec::new();
    for &child in tree.top_level() {
        collect_text_rows(tree, child, &mut text_rows);
    }
    if !text_rows.is_empty() {
        write_document_sheet(&mut w, &text_rows)?;
        existing.push(DOC_SHEET.to_string());
    }

    // 2) 表块 → 独立表（文档序）；0 行表跳过（防御，登记）。
    let mut tables: Vec<NodeId> = Vec::new();
    for &child in tree.top_level() {
        collect_tables(tree, child, &mut tables);
    }
    for (i, &id) in tables.iter().enumerate() {
        let text = tree.node(id).text.to_string();
        let rows = table_rows(&text);
        if rows.is_empty() {
            continue;
        }
        let ordinal = i + 1;
        let header = rows[0].first().map(String::as_str).unwrap_or("");
        let name = table_sheet_name(header, ordinal, &existing);
        existing.push(name.clone());
        write_table_sheet(&mut w, &name, &rows)?;
    }

    // 3) 全零 → 最小合法工作簿（单空表 Sheet1——Excel 工作簿至少一表）。
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
        let id = tree.create_node(kind, text, attrs, marks, None);
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
