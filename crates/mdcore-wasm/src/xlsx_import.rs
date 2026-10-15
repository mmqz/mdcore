//! t17：XLSX → Markdown 导入切片（office-io 读面经 wasm 桥的 web 消费）。
//!
//! **定位（2026-10-14，t17 立项——t16 DOCX 切片的对称展开）**：把
//! [`mdc_office_io::io`] 的硬化读面（容器三门 + calamine 五面提取）经
//! 本 crate 暴露给 web 编辑器——「在编辑器里打开 .xlsx」的最小真实
//! 切片。消费边 = mdcore-wasm → office-io（t16 已落地的既有边，本
//! 模块为兄弟模块；**只读取消费**——XLSX 维持 C3 读—改—重建裁决，
//! 本轮不经 wasm 写 xlsx）。
//!
//! ## 暴露面（[`import_xlsx`]，wasm-bindgen 自由函数——不碰 WasmCore
//! 状态，既有类面零变化）
//!
//! - 入参：XLSX 字节（JS `Uint8Array` → `&[u8]`，经 office-io t17
//!   增量字节入口 [`mdc_office_io::io::read_book_from_bytes`]——wasm32
//!   无文件系统）。
//! - 成功：JSON 字符串 `{"markdown": "...", "stats": {...}}`——markdown
//!   直接喂 [`crate::WasmCore::new`] / 宿主注入契约（web 侧
//!   `window.__inject`）；stats = 紧凑摘要（见下）。
//! - 失败：抛 JSON 字符串 `{"code": "...", "message": "..."}`——
//!   typed 错误码（下表），宿主可 `JSON.parse` 出结构化错误。
//!
//! ## 错误码表（office-io `ReadError` → 码——与 t16 docx 族同构）
//!
//! | office-io 错误 | code | 含义 |
//! |---|---|---|
//! | [`ReadError::PrescanRejected`] | `container_prescan` | 门 1 拒（条目数/单尺寸/总量/路径逃逸——敌意输入零解析层接触） |
//! | `ContainerCorrupt` | `container_corrupt` | zip 容器损坏（EOCD/中央目录不可读——垃圾字节/截断档案） |
//! | `InvalidPart` | `container_invalid_part` | 门 3 部件良构性拒（严格模式；本模块恒兼容读，仅引擎自身仍可产此面的路径） |
//! | `ParseError` | `xlsx_parse` | 过门后解析层错误（calamine 引擎面——与 t16 `docx_parse` 命名同族） |
//!
//! 族差异登记：t16 的 `container_parse`（zip 读界）在本模块**不产生**
//! ——office-io XLSX 读面对过门后错误为单一 [`ReadError::ParseError`]
//! （calamine 内部 zip 读界错误同样归并其中），无 docx 式
//! Container/Parse 二分；引擎错误统一 `xlsx_parse` 保持族命名一致。
//!
//! 读模式 = **兼容**（[`ReadOptions::compat`]）：可恢复的畸形部件降级
//! 为结构化警告随 stats 返回（可见性），敌意输入防线（门 1/门 2 与
//! 全部安全上限）两模式恒等——垃圾/截断档案仍 typed 拒绝。
//!
//! ## 映射规则（Book → markdown；值面消费，非保真渲染）
//!
//! - **每表一节**：`## {表名}`（MD 上限 h6 内取 h2——工作簿无文档级
//!   标题，表名即章节；表名内 `\n`/`\r` 拍平为空格——行级 markdown
//!   不承载换行）+ GFM 管道表（与 mdcore `BlockKind::Table` 规范文本
//!   同构，t16 docx 表映射同形）；
//! - **使用区（used range）**：calamine 值矩阵的包围盒（含仅格式空格
//!   的扩张——Excel 使用区语义）；**首个使用行 = GFM 表头行**，其余
//!   为体行；
//! - **单元格值**：office-io 类型化模型 [`mdc_office_io::io::CellData`]
//!   的 `display()` 显示面（r4-t1 切换——与 calamine `Data` Display
//!   逐字等值，office-io tests/cell_types.rs 全格等值钉定；本模块
//!   自此零 calamine 面接触）：文本原样/数值 Rust 格式化（`4.5`、
//!   `9`）/布尔 `true`/`false`/错误码（`#DIV/0!` 等）/ISO 日期时长
//!   原串/Excel 序列数日期（`45943.5`——numFmt 显示形态不宣称，值面
//!   语义；**kind 不改显示面**：前导零文本 `00123` 逐字、日期格 =
//!   序列数）；清洗：`|` → `\|` 转义、`\n`/`\r` → 空格（GFM 单元格
//!   不承载换行）、首尾空白去；
//! - **公式格 = 缓存值**（Excel 显示语义——`values` 面即含缓存值的
//!   值矩阵）：**公式文本不导入**，计数入 stats（`formulas_flattened`）。
//!   依据：office-io 写面模块文档登记「cached = None 读回
//!   `Float(0.0)`（与真值 0 不可区分）」——「有缓存值用缓存值、否则
//!   公式文本」不可实现（两义性），值面消费取显示语义为诚实选择；
//! - **合并单元格**：值矩阵自然编码（左上值 + 其余空——Excel 语义），
//!   计数入 stats（`merged_ranges`）；
//! - **超链接**：单元格文本（显示文本）保留在表中，**目标 URL 丢弃**
//!   （GFM 表格单元格不承载链接元数据），计数入 stats
//!   （`hyperlinks_dropped`）；
//! - **空表**（值矩阵空）：`## {表名}` + `(empty sheet)` 段落
//!   （表清单可见性——不静默丢表），计数 `empty_sheets`。
//!
//! ## 规模上限（markdown 产出可预期；越限丢弃计数显式化）
//!
//! [`MAX_SHEETS`] = 16 表 / [`MAX_ROWS`] = 50 行/表 / [`MAX_COLS`] = 32
//! 列/表（使用区前缀截断）→ `sheets_dropped` / `rows_dropped` /
//! `cols_dropped` stats。零表工作簿 → 空 markdown（编辑器空文档，
//! `sheets` = 0）。
//!
//! **两层关系登记（r4-t2）**：本三常量是**渲染窗**（markdown 产出
//! 可预期）——读面全提取（office-io `io::read_book`：表数 1,000 /
//! 稠密包围盒格总面积 4,000,000 —— r4-t2 资源上限）之上的消费
//! 子集截断，**渲染截断非拒绝**（越限计数进 stats，丢弃可见）；
//! 读面上限是安全拒绝（typed `PrescanRejected`）。两层无重复定义
//! 不收敛：读面 = C1 大档能力宣称面（office-io SECURITY_POLICY
//! §3），渲染窗 = 本模块 GFM 产出可预期性——同一工作簿先过读面
//! 上限，再进渲染窗截断。
//!
//! ## stats 形状
//!
//! ```json
//! { "sheets": 3, "tables": 2, "empty_sheets": 1, "rows": 6, "cells": 14,
//!   "formulas_flattened": 1, "merged_ranges": 1, "hyperlinks_dropped": 1,
//!   "sheets_dropped": 0, "rows_dropped": 0, "cols_dropped": 0,
//!   "warnings": [{ "part": "...", "kind": "...", "detail": "..." }] }
//! ```
//!
//! - `sheets`/`tables`/`empty_sheets`：渲染表数 / GFM 表数 / 空表节数
//!   （sheets = tables + empty_sheets + 越限前已渲染的…——恒
//!   tables + empty_sheets）；
//! - `rows`：全部渲染表的数据行合计（含表头行）；`cells`：渲染窗口内
//!   非空文本单元格合计；
//! - `warnings`：兼容模式结构化警告（office-io [`Book::warnings`] 透传，
//!   kind 取 snake_case 字符串——t16 同形）。
//!
//! ## 已知边界（登记，非缺陷）
//!
//! - 单元格文本以 MD 结构字符开头（`#`/`|` 等）在表格单元格内安全
//!   （mdcore 表块按整行 `|` 前缀判定，单元格内容不重解释）；
//! - 数值/日期为值面（Rust f64 格式化/Excel 序列数），无 numFmt 显示
//!   形态（¥/百分比/日历显示不宣称——与 CAPABILITIES C1 口径一致）；
//! - 公式求值不发生（无引擎计算面）——缓存值即所见；
//! - 样式/图表/图片/数据验证等 C1 面外数据不进入 markdown（office-io
//!   读面本就不提取——FIDELITY 丢失矩阵为上层真源）。
//!
//! 纯内核（[`book_to_markdown`] 等）不触碰 JsValue → 原生 cargo test
//! 直测（构造路径 = office-io 写面产线 → 字节入口读 → 映射——写读
//! 全环同测，docx_import.rs `parse_source_str` 先例的测试面变体）；
//! wasm 壳由 wasm32 check + web 四件套门禁背书。

use mdc_office_io::io::{
    read_book_from_bytes_with_options, Book, ReadError, ReadOptions, SheetData,
};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// 规模上限（markdown 产出可预期——越限丢弃恒计数进 stats）
// ---------------------------------------------------------------------------

/// 渲染表数上限（工作簿表序前缀）。
pub const MAX_SHEETS: usize = 16;
/// 每表渲染行数上限（使用区行序前缀，含表头行）。
pub const MAX_ROWS: usize = 50;
/// 每表渲染列数上限（使用区列序前缀）。
pub const MAX_COLS: usize = 32;

// ---------------------------------------------------------------------------
// typed 错误面（t16 族同构）
// ---------------------------------------------------------------------------

/// typed 导入错误（错误码 + 人类可读消息——见模块文档错误码表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError {
    pub code: &'static str,
    pub message: String,
}

/// office-io `ReadError` → 错误码映射（容器门拒 ≠ 引擎解析错误——
/// 族命名与 t16 docx 面一致，`container_parse` 不产生的族差异见模块
/// 文档）。
fn map_error(e: &ReadError) -> ImportError {
    match e {
        ReadError::PrescanRejected { .. } => ImportError {
            code: "container_prescan",
            message: e.to_string(),
        },
        ReadError::ContainerCorrupt(_) => ImportError {
            code: "container_corrupt",
            message: e.to_string(),
        },
        ReadError::ParseError(_) => ImportError {
            code: "xlsx_parse",
            message: format!("calamine parse error: {e}"),
        },
        ReadError::InvalidPart { .. } => ImportError {
            code: "container_invalid_part",
            message: e.to_string(),
        },
    }
}

/// 兼容模式读选项（可恢复畸形部件降级警告；敌意防线两模式恒等）。
fn compat_options() -> ReadOptions {
    ReadOptions { compat: true }
}

// ---------------------------------------------------------------------------
// 纯映射内核（原生可测——r4-t1 起消费面只经 office-io 公开类型，
// calamine 类型零接触：单元格值经 CellData::display 显示面）
// ---------------------------------------------------------------------------

/// 单元格清洗：`|` 转义（GFM 列分隔符冲突）、`\n`/`\r` → 空格
/// （单元格不承载换行）、首尾空白去（GFM 解析器惯例——t16
/// docx_import::sanitize_cell 同形）。
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

/// 表名 → 标题行文本（`\n`/`\r` 拍平空格——行级 markdown 不承载
/// 换行；其余字符原样——表名在标题位不受表格清洗约束）。
fn heading_name(name: &str) -> String {
    if name.contains('\n') || name.contains('\r') {
        name.chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect()
    } else {
        name.to_string()
    }
}

/// 一行单元格 → GFM 管道表行（`| a | b |` 形——t16 render_row 同形）。
fn render_row(cells: &[String]) -> String {
    let mut line = String::from("|");
    for cell in cells {
        line.push(' ');
        line.push_str(cell);
        line.push_str(" |");
    }
    line
}

/// stats 的警告 kind → snake_case 字符串（t16 warning_kind 同形）。
fn warning_kind(kind: &mdc_office_io::io::WarningKind) -> &'static str {
    match kind {
        mdc_office_io::io::WarningKind::MalformedXml => "malformed_xml",
        mdc_office_io::io::WarningKind::ControlChars => "control_chars",
        mdc_office_io::io::WarningKind::BrokenReference => "broken_reference",
    }
}

/// 单表渲染产物（book_to_markdown 的聚合单元——纯数据）。
struct SheetOutcome {
    section: String,
    /// 渲染数据行数（含表头行）；0 = 空表。
    rows: usize,
    /// 越限丢弃行/列数（使用区 − 渲染前缀）。
    rows_dropped: usize,
    cols_dropped: usize,
    /// 渲染窗口内非空文本单元格数（Empty 与空串同视为空）。
    non_empty_cells: usize,
}

/// 单表 → SheetOutcome。空表（使用区空）→ 标题 + `(empty sheet)`
/// 段落。规模上限截断的使用区前缀；`formulas_flattened` 计全表公式
/// 文本格（越限行的公式文本同样丢弃，已由 rows/cols_dropped 登记其
/// 值面丢弃——公式计数不二次截断，语义 = 表内公式文本总数）。
fn sheet_to_section(sheet: &SheetData) -> SheetOutcome {
    let heading = format!("## {}", heading_name(&sheet.name));
    let height = sheet.values.height();
    let width = sheet.values.width();
    if height == 0 || width == 0 {
        return SheetOutcome {
            section: format!("{heading}\n\n(empty sheet)"),
            rows: 0,
            rows_dropped: 0,
            cols_dropped: 0,
            non_empty_cells: 0,
        };
    }
    let rows = height.min(MAX_ROWS);
    let cols = width.min(MAX_COLS);
    let mut lines: Vec<String> = Vec::with_capacity(rows + 1);
    let mut non_empty_cells = 0usize;
    for r in 0..rows {
        let mut cells: Vec<String> = Vec::with_capacity(cols);
        for c in 0..cols {
            // r4-t1：类型化模型显示面（CellData::display——与 calamine
            // Data Display 逐字等值，office-io tests/cell_types.rs 钉定；
            // Empty → ""；显示面兼容 = 既有断言即钉定）。
            let text = sheet
                .typed
                .get((r, c))
                .map(|d| d.display())
                .unwrap_or_default();
            if !text.is_empty() {
                non_empty_cells += 1;
            }
            cells.push(sanitize_cell(&text));
        }
        lines.push(render_row(&cells));
    }
    // 首个使用行 = 表头 → 表头行后插 `---` 分隔行（GFM 表语法）。
    let sep_cells: Vec<String> = std::iter::repeat_n(String::from("---"), cols).collect();
    lines.insert(1, render_row(&sep_cells));
    SheetOutcome {
        section: format!("{heading}\n\n{}", lines.join("\n")),
        rows,
        rows_dropped: height - rows,
        cols_dropped: width - cols,
        non_empty_cells,
    }
}

/// Book → (markdown, stats JSON)。纯函数（原生直测；宿主消费经
/// [`import_xlsx_core`] 字节入口）。
fn book_to_markdown(book: &Book) -> (String, serde_json::Value) {
    let mut sections: Vec<String> = Vec::new();
    let mut tables = 0usize;
    let mut empty_sheets = 0usize;
    let mut rows_total = 0usize;
    let mut cells_total = 0usize;
    let mut rows_dropped = 0usize;
    let mut cols_dropped = 0usize;

    let rendered = book.sheets.len().min(MAX_SHEETS);
    for sheet in &book.sheets[..rendered] {
        let out = sheet_to_section(sheet);
        if out.rows == 0 {
            empty_sheets += 1;
        } else {
            tables += 1;
            rows_total += out.rows;
            cells_total += out.non_empty_cells;
        }
        rows_dropped += out.rows_dropped;
        cols_dropped += out.cols_dropped;
        sections.push(out.section);
    }

    // 公式文本丢弃计数（全表——见 sheet_to_section 文档）。
    let formulas_flattened: usize = book.sheets[..rendered]
        .iter()
        .map(|s| {
            (0..s.formulas.height())
                .map(|r| {
                    (0..s.formulas.width())
                        .filter(|&c| {
                            s.formulas
                                .get((r, c))
                                .map(|f| !f.is_empty())
                                .unwrap_or(false)
                        })
                        .count()
                })
                .sum::<usize>()
        })
        .sum();
    let merged_ranges: usize = book.sheets[..rendered].iter().map(|s| s.merges.len()).sum();
    let hyperlinks_dropped: usize = book.sheets[..rendered]
        .iter()
        .map(|s| s.hyperlinks.len())
        .sum();

    let stats = serde_json::json!({
        "sheets": rendered,
        "tables": tables,
        "empty_sheets": empty_sheets,
        "rows": rows_total,
        "cells": cells_total,
        "formulas_flattened": formulas_flattened,
        "merged_ranges": merged_ranges,
        "hyperlinks_dropped": hyperlinks_dropped,
        "sheets_dropped": book.sheets.len() - rendered,
        "rows_dropped": rows_dropped,
        "cols_dropped": cols_dropped,
        "warnings": book.warnings.iter().map(|w| serde_json::json!({
            "part": w.part,
            "kind": warning_kind(&w.kind),
            "detail": w.detail,
        })).collect::<Vec<_>>(),
    });
    (sections.join("\n\n"), stats)
}

/// 导入内核（字节入口；纯函数——无 JsValue，原生 cargo test 直测）。
/// 成功返回与 wasm 面同形的 JSON 串 `{"markdown": ..., "stats": ...}`。
pub(crate) fn import_xlsx_core(bytes: &[u8]) -> Result<String, ImportError> {
    let book =
        read_book_from_bytes_with_options(bytes, &compat_options()).map_err(|e| map_error(&e))?;
    let (markdown, stats) = book_to_markdown(&book);
    Ok(serde_json::json!({ "markdown": markdown, "stats": stats }).to_string())
}

// ---------------------------------------------------------------------------
// wasm-bindgen 壳（JsValue 仅此层——lib.rs parse_source 先例同构）
// ---------------------------------------------------------------------------

/// 导入 .xlsx 字节 → `{"markdown": "...", "stats": {...}}`（JSON 串，
/// 形状见模块文档）；失败抛 JSON 串 `{"code": "...", "message": "..."}`
///（typed 错误码表见模块文档——宿主 `JSON.parse(e)` 即结构化错误）。
#[wasm_bindgen]
pub fn import_xlsx(bytes: &[u8]) -> Result<String, JsValue> {
    import_xlsx_core(bytes).map_err(|e| {
        JsValue::from_str(&serde_json::json!({ "code": e.code, "message": e.message }).to_string())
    })
}

// ---------------------------------------------------------------------------
// 样例工作簿（单一真源：examples/gen_xlsx_fixture.rs 生成 web e2e 夹具
// + 本模块测试消费——office-io 写面即夹具产线，零外部工具依赖；
// 同时写读全环 = C2→C1 栈复证）
// ---------------------------------------------------------------------------

/// 样例 XLSX 字节（夹具产线单一真源）：三表——"Data"（表头/文本/
/// 数值/公式+缓存值/超链接/合并范围）、"Notes"（布尔值）、"Empty"
///（空表——使用区为空）。消费方 = 本模块测试 + examples/
/// gen_xlsx_fixture.rs（web e2e 夹具生成器）。`#[doc(hidden)]`：非
/// 产品面（wasm 导出面无此项——无 #[wasm_bindgen]），仅为避免产线
/// 双份漂移而公开。
#[doc(hidden)]
pub fn build_sample_xlsx() -> Vec<u8> {
    use mdc_office_io::write::{CellValue, FormulaCache, HyperlinkSpec, WorkbookWriter};
    let mut w = WorkbookWriter::new();
    {
        let mut s = w.add_sheet("Data").expect("sheet Data");
        s.write_row(
            0,
            &[
                CellValue::Text("Item".into()),
                CellValue::Text("Qty".into()),
                CellValue::Text("Note".into()),
            ],
        )
        .expect("header row");
        s.write_row(
            1,
            &[
                CellValue::Text("Widget".into()),
                CellValue::Number(4.5),
                CellValue::Text("has | pipe".into()),
            ],
        )
        .expect("data row 1");
        s.write(2, 0, CellValue::Text("Gadget".into()), None)
            .expect("gadget name");
        s.write(
            2,
            1,
            CellValue::Formula {
                text: "SUM(B2:B2)*2".into(),
                cached: Some(FormulaCache::Number(9.0)),
            },
            None,
        )
        .expect("formula cell");
        s.write_hyperlink(
            2,
            2,
            HyperlinkSpec {
                url: "https://example.com/docs".into(),
                display_text: Some("docs".into()),
                tooltip: None,
            },
        )
        .expect("hyperlink cell");
        s.merge(3, 0, 3, 2, "merged top", None)
            .expect("merge range");
    }
    {
        let mut s = w.add_sheet("Notes").expect("sheet Notes");
        s.write_row(
            0,
            &[
                CellValue::Text("Key".into()),
                CellValue::Text("Value".into()),
            ],
        )
        .expect("notes header");
        s.write_row(1, &[CellValue::Text("k1".into()), CellValue::Boolean(true)])
            .expect("notes row");
    }
    {
        let _s = w.add_sheet("Empty").expect("sheet Empty");
    }
    w.save_to_vec().expect("save")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import_json(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_str(&import_xlsx_core(bytes).expect("import ok")).expect("payload json")
    }

    /// 写读映射全环精确等值（夹具 = 产线单一真源 build_sample_xlsx）。
    #[test]
    fn writer_roundtrip_import_maps_structure() {
        let payload = import_json(&build_sample_xlsx());
        let md = payload["markdown"].as_str().expect("markdown");
        assert_eq!(
            md,
            "## Data\n\
             \n\
             | Item | Qty | Note |\n\
             | --- | --- | --- |\n\
             | Widget | 4.5 | has \\| pipe |\n\
             | Gadget | 9 | docs |\n\
             | merged top |  |  |\n\
             \n\
             ## Notes\n\
             \n\
             | Key | Value |\n\
             | --- | --- |\n\
             | k1 | true |\n\
             \n\
             ## Empty\n\
             \n\
             (empty sheet)"
        );
        let stats = &payload["stats"];
        assert_eq!(stats["sheets"], 3);
        assert_eq!(stats["tables"], 2);
        assert_eq!(stats["empty_sheets"], 1);
        assert_eq!(stats["rows"], 6); // Data 4（含表头）+ Notes 2
        assert_eq!(stats["cells"], 14); // Data 10 + Notes 4
        assert_eq!(stats["formulas_flattened"], 1);
        assert_eq!(stats["merged_ranges"], 1);
        assert_eq!(stats["hyperlinks_dropped"], 1);
        assert_eq!(stats["sheets_dropped"], 0);
        assert_eq!(stats["rows_dropped"], 0);
        assert_eq!(stats["cols_dropped"], 0);
        assert_eq!(stats["warnings"].as_array().map(Vec::len), Some(0));
    }

    /// 值面渲染（Data Display 家族）：布尔/数值/错误码经写读全环落
    /// 到单元格文本。错误面与 ISO 串为 calamine 直读路径（写面不产
    /// 出），值面渲染由 Display 单一出口保证——此处钉写面可达面。
    #[test]
    fn cell_value_face_renders() {
        use mdc_office_io::write::{CellValue, WorkbookWriter};
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("V").expect("sheet");
            s.write_row(
                0,
                &[
                    CellValue::Text("T".into()),
                    CellValue::Text("N".into()),
                    CellValue::Text("B".into()),
                    CellValue::Text("P".into()),
                ],
            )
            .expect("header");
            s.write_row(
                1,
                &[
                    CellValue::Text("a\nb".into()), // 换行拍平
                    CellValue::Number(-0.25),       // 负小数
                    CellValue::Boolean(false),
                    CellValue::Text(" x ".into()), // 首尾空白去
                ],
            )
            .expect("row");
        }
        let payload = import_json(&w.save_to_vec().expect("save"));
        let md = payload["markdown"].as_str().expect("markdown");
        assert_eq!(
            md,
            "## V\n\n| T | N | B | P |\n| --- | --- | --- | --- |\n| a b | -0.25 | false | x |"
        );
    }

    /// r4-t1：类型化模型显示面（CellData::display 切换后）——kind 不改
    /// 显示面：前导零文本逐字（默认保留裁决）、日期格 = 序列数值面
    /// （numFmt 显示形态不宣称——C1 口径）、数值/布尔与 t17 语义同源。
    #[test]
    fn typed_display_face_kinds_preserve_text_semantics() {
        use mdc_office_io::write::{CellStyle, CellValue, NumFmt, WorkbookWriter};
        let mut w = WorkbookWriter::new();
        {
            let mut s = w.add_sheet("T").expect("sheet");
            let date_style = CellStyle {
                num_fmt: Some(NumFmt::Custom("yyyy-mm-dd".into())),
                ..Default::default()
            };
            s.write_row(
                0,
                &[
                    CellValue::Text("code".into()),
                    CellValue::Text("qty".into()),
                    CellValue::Text("ok".into()),
                    CellValue::Text("day".into()),
                ],
            )
            .expect("header");
            s.write(1, 0, CellValue::Text("00123".into()), None)
                .expect("leading-zero text");
            s.write(1, 1, CellValue::Number(4.5), None).expect("number");
            s.write(1, 2, CellValue::Boolean(true), None).expect("bool");
            s.write(1, 3, CellValue::Number(44927.0), Some(&date_style))
                .expect("date serial + numFmt");
        }
        let payload = import_json(&w.save_to_vec().expect("save"));
        let md = payload["markdown"].as_str().expect("markdown");
        assert_eq!(
            md,
            "## T\n\n| code | qty | ok | day |\n| --- | --- | --- | --- |\n| 00123 | 4.5 | true | 44927 |",
            "前导零逐字 + 日期格 = 序列数（kind 进 office-io 模型，显示面不变）"
        );
    }

    /// 公式格策略：缓存值渲染（Excel 显示语义），公式文本丢弃计数
    ///（缓存缺失两义性在档——模块文档映射规则）。
    #[test]
    fn formula_policy_renders_cached_value() {
        let payload = import_json(&build_sample_xlsx());
        let md = payload["markdown"].as_str().expect("markdown");
        assert!(
            md.contains("| Gadget | 9 | docs |"),
            "公式格应为缓存值 9: {md}"
        );
        assert!(!md.contains("SUM"), "公式文本不得进入 markdown: {md}");
        assert_eq!(payload["stats"]["formulas_flattened"], 1);
    }

    /// 合并范围：值矩阵自然编码（左上值 + 其余空）。
    #[test]
    fn merged_range_renders_top_left_plus_empties() {
        let payload = import_json(&build_sample_xlsx());
        let md = payload["markdown"].as_str().expect("markdown");
        assert!(
            md.contains("| merged top |  |  |"),
            "合并行应为左上值+空: {md}"
        );
        assert_eq!(payload["stats"]["merged_ranges"], 1);
    }

    /// 规模上限：表数/行/列前缀截断 + 丢弃计数显式化。
    #[test]
    fn caps_truncate_with_drop_stats() {
        use mdc_office_io::write::{CellValue, WorkbookWriter};
        let mut w = WorkbookWriter::new();
        // MAX_SHEETS + 3 表（前 16 渲染，3 丢弃）。
        for i in 0..(MAX_SHEETS + 3) {
            let mut s = w.add_sheet(&format!("S{i}")).expect("sheet");
            // 首 3 表造 55 行 × 35 列（50 行/32 列渲染，5 行/3 列丢弃）。
            let (rows, cols) = if i < 3 { (55, 35) } else { (2, 2) };
            for r in 0..rows {
                let cells: Vec<CellValue> = (0..cols)
                    .map(|c| CellValue::Number(((r * 100 + c) % 10) as f64))
                    .collect();
                s.write_row(r as u32, &cells).expect("row");
            }
        }
        let payload = import_json(&w.save_to_vec().expect("save"));
        let stats = &payload["stats"];
        assert_eq!(stats["sheets"], MAX_SHEETS);
        assert_eq!(stats["sheets_dropped"], 3);
        assert_eq!(stats["rows_dropped"], 15); // 3 表 × 5 行
        assert_eq!(stats["cols_dropped"], 9); // 3 表 × 3 列
        assert_eq!(stats["rows"], 3 * MAX_ROWS + 13 * 2);
        let md = payload["markdown"].as_str().expect("markdown");
        // 每表 50 数据行 + 表头 + 分隔行 = 52 行；表节 16 个。
        let sheet_sections = md.split("\n\n## ").count();
        assert_eq!(sheet_sections, MAX_SHEETS, "渲染表节数应为上限");
        let s0 = md.split("\n\n## ").next().expect("first section");
        // 标题 + 空行 +（表头 + 分隔行 + 50 数据行）= 53 行。
        assert_eq!(s0.lines().count(), 3 + MAX_ROWS, "首表行数 = 表头+分隔+50");
    }

    /// 空表：标题 + (empty sheet) 段落 + 计数（不静默丢表）。
    #[test]
    fn empty_sheet_renders_note() {
        let payload = import_json(&build_sample_xlsx());
        let md = payload["markdown"].as_str().expect("markdown");
        assert!(
            md.ends_with("## Empty\n\n(empty sheet)"),
            "尾节应为空表注记: {md}"
        );
        assert_eq!(payload["stats"]["empty_sheets"], 1);
        assert_eq!(payload["stats"]["tables"], 2);
    }

    /// 错误码映射全臂（typed 族——ReadError 构造直测，免 zip 构造）。
    #[test]
    fn error_codes_cover_all_read_error_arms() {
        let e = map_error(&ReadError::PrescanRejected {
            reason: "entry count over limit",
            entry: None,
        });
        assert_eq!(e.code, "container_prescan");
        let e = map_error(&ReadError::ContainerCorrupt("bad eocd".into()));
        assert_eq!(e.code, "container_corrupt");
        let e = map_error(&ReadError::ParseError("range[S1]: boom".into()));
        assert_eq!(e.code, "xlsx_parse");
        assert!(e.message.contains("calamine"));
        let e = map_error(&ReadError::InvalidPart {
            part: "xl/worksheets/sheet1.xml".into(),
            detail: "truncated".into(),
        });
        assert_eq!(e.code, "container_invalid_part");
    }

    /// 敌意面：垃圾字节 → container_corrupt typed 拒。
    #[test]
    fn garbage_bytes_typed_container_error() {
        for garbage in [
            vec![],
            vec![0u8, 1, 2, 99, 255],
            b"not a zip at all".to_vec(),
        ] {
            let err = import_xlsx_core(&garbage).expect_err("garbage must reject");
            assert_eq!(err.code, "container_corrupt", "bytes={garbage:?}");
            assert!(!err.message.is_empty());
        }
    }

    /// 敌意面：截断档案（EOCD 缺失）→ container_corrupt typed 拒。
    #[test]
    fn truncated_xlsx_typed_container_error() {
        let full = build_sample_xlsx();
        let cut = &full[..full.len() / 2];
        let err = import_xlsx_core(cut).expect_err("truncated zip must reject");
        assert_eq!(err.code, "container_corrupt");
    }

    /// 夹具漂移守卫：web e2e 夹具（examples/gen_xlsx_fixture.rs 产线，
    /// 提交于 web/tests/fixtures/import_sample.xlsx）必须仍产出期望
    /// markdown——夹具损坏/漂移在原生层先红，带清晰定位信息。
    #[test]
    fn web_e2e_fixture_imports_expected_markdown() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/tests/fixtures/import_sample.xlsx"
        );
        let bytes = std::fs::read(path)
            .unwrap_or_else(|e| panic!("read fixture {path}: {e} — regenerate: cargo run -p mdcore-wasm --example gen_xlsx_fixture -- ../../web/tests/fixtures/import_sample.xlsx"));
        let payload = import_json(&bytes);
        assert!(
            payload["markdown"]
                .as_str()
                .expect("markdown")
                .contains("## Data"),
            "fixture markdown drifted: {}",
            payload["markdown"]
        );
        assert_eq!(payload["stats"]["tables"], 2);
        assert_eq!(payload["stats"]["formulas_flattened"], 1);
    }
}
