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
//! - **公式格（P2-3 双通道 + 批 5 值面双载）**：单元格 = 反引号包裹
//!   公式原文（`` `=SUM(B2:B4)` ``——calamine 读侧无 `=` 前缀[office-io
//!   FIDELITY §1 在档]，本层补 `=`；写面对称接受带/不带前缀）；
//!   **缓存值面双载（批 5，#29 三大发现 #2/#3 修复）**：缓存值
//!   非空时追加 `` `=SUM(B2:B4)`=9 `` 形（`=` + 缓存值显示面——内容
//!   提取/引用场景值面可见；错误格缓存 = 错误码文本，显示面忠实
//!   原则 `#DIV/0!` 即 Excel 显示语义）；缓存空（calamine Empty）
//!   保持单载。缓存值同时进 stats `formulas` 清单（`{sheet, cell,
//!   text, cached}`）；渲染窗内计数 `formulas_imported`，越限（行/
//!   列超出渲染窗）计数 `formulas_flattened`（值与文本同弃——
//!   `rows_dropped`/`cols_dropped` 已登记其丢弃面）；
//! - **超链接格（P2-3 四字段保留）**：单元格 = `[text](target)`
//!   GFM 形（外链 `target` 优先；内链 `location` → `#锚` 形[docx
//!   导入内链同族]；`tooltip` → GFM title ` "tooltip"`；显示文本
//!   缺省回落单元格值，仍空回落目标串——零静默丢）；四字段
//!   （target/location/displayed_text/tooltip）+ 锚格文本进 stats
//!   `hyperlinks` 清单；锚格在渲染窗内计数 `hyperlinks_imported`，
//!   锚格越限计数 `hyperlinks_dropped`；range 形链接以锚格承载
//!   （覆盖区其余格为值面——不重复渲染）；公式格带链接 = 链接形
//!   包裹公式形（双通道叠加，零丢弃）；
//! - **合并单元格**：值矩阵自然编码（左上值 + 其余空——Excel 语义），
//!   计数入 stats（`merged_ranges`）；
//! - **空表**（值矩阵空）：`## {表名}` + `(empty sheet)` 段落
//!   （表清单可见性——不静默丢表），计数 `empty_sheets`。
//!
//! ## 规模上限（markdown 产出可预期；越限丢弃计数显式化）
//!
//! [`MAX_SHEETS`] = 64 表 / [`MAX_ROWS`] = 5,000 行/表 / [`MAX_COLS`] =
//! 128 列/表（使用区前缀截断）→ `sheets_dropped` / `rows_dropped` /
//! `cols_dropped` stats。零表工作簿 → 空 markdown（编辑器空文档，
//! `sheets` = 0）。
//!
//! **批 5 窗口提级（#29 三大发现 #1 修复）**：原 16/50/32 对真实数据
//! 簿过紧（体检实测：196 行逮捕登记簿丢 146 行召回 0.242；36 列簿
//! 丢 4 列；5 件窗口案）——真实报表/登记簿普遍 50-5,000 行、
//! ≤128 列，新窗覆盖该全域（R-1 系 50MB 件的 ~3,250 行亦全覆盖）。
//! 大档防线不依赖本窗：读面上限（office-io `io::read_book`：表数
//! 1,000 / 稠密包围盒格总面积 4,000,000——r4-t2 资源上限）+ 容器
//! 预扫门（门 1/门 2）仍是安全拒绝层；渲染窗仅是消费子集截断
//! （越限计数进 stats + P2-4 error 档可见警告——丢弃可见不可恢复）。
//!
//! **两层关系登记（r4-t2）**：本三常量是**渲染窗**（markdown 产出
//! 可预期）——读面全提取之上的消费
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
//!   "formulas_imported": 1, "hyperlinks_imported": 1,
//!   "formulas": [{"sheet": "Data", "cell": "B3", "text": "=SUM(B2:B2)*2", "cached": "9"}],
//!   "hyperlinks": [{"sheet": "Data", "cell": "C3", "text": "docs",
//!                    "target": "https://example.com/docs", "location": null, "tooltip": null}],
//!   "formulas_flattened": 0, "merged_ranges": 1, "hyperlinks_dropped": 0,
//!   "sheets_dropped": 0, "rows_dropped": 0, "cols_dropped": 0,
//!   "warnings": [{ "kind": "merged_ranges", "part": "xlsx", "count": 1, "severity": "info" },
//!                 { "part": "xl/..", "kind": "...", "detail": "...", "count": 1, "severity": "warn" }] }
//! ```
//!
//! - `sheets`/`tables`/`empty_sheets`：渲染表数 / GFM 表数 / 空表节数
//!   （sheets = tables + empty_sheets + 越限前已渲染的…——恒
//!   tables + empty_sheets）；
//! - `rows`：全部渲染表的数据行合计（含表头行）；`cells`：渲染窗口内
//!   非空内容单元格合计（P2-3：公式格计入——公式原文即内容）；
//! - P2-3 键：`formulas_imported`/`hyperlinks_imported`（渲染窗内
//!   接入数）+ `formulas`/`hyperlinks` 清单（≤200 条——超过部分计数
//!   仍全量在 `*_imported`，清单截断仅影响明细面）；
//!   `formulas_flattened`/`hyperlinks_dropped` 语义收窄为**渲染窗外**
//!   丢弃（行/列越限或锚格越位——正常表恒 0；P2-3 前为全量丢弃
//!   计数）；
//! - `warnings`（#22 P2-4 可见警告面）：引擎兼容模式结构化警告
//!   （office-io [`Book::warnings`] 透传，kind 取 snake_case 字符串，
//!   每条 +`count: 1` +`severity`）**叠加**导入链有损项聚合条目
//!   （`{kind, part: "xlsx", count, severity}`——非 0 计数才进，count =
//!   stats 同键镜像）；severity 三档：`error` = 截断（sheets/rows/
//!   cols_dropped——内容整体不可见，需关注）/ `warn` = 越限丢弃
//!   （formulas_flattened/hyperlinks_dropped）与引擎部件降级 /
//!   `info` = 可接受（merged_ranges 结构登记等）。
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
///
/// 批 5（#29 三大发现 #1）：16 → 64——真实多表簿（登记簿/月报表
/// 常见 2-10 表）+ 多年归档簿覆盖；安全层在读面（1,000 表）与容器
/// 预扫门，不依赖本窗。
pub const MAX_SHEETS: usize = 64;
/// 每表渲染行数上限（使用区行序前缀，含表头行）。
///
/// 批 5（#29 三大发现 #1）：50 → 5,000——体检实测 196 行真实簿丢
/// 146 行（召回 0.242）；R-1 系 50MB 件 ~3,250 行。5,000 覆盖真实
/// 报表全域；越限计数 + error 档警告（P2-4）可见。
pub const MAX_ROWS: usize = 5_000;
/// 每表渲染列数上限（使用区列序前缀）。
///
/// 批 5（#29 三大发现 #1）：32 → 128——体检实测 36 列真实簿丢 4 列；
/// Excel 实用簿列域普遍 ≤52（A-AZ），128 覆盖全域。
pub const MAX_COLS: usize = 128;

/// P2-3：stats `formulas`/`hyperlinks` 明细清单上限（200 条——计数键
/// `*_imported` 恒全量，清单截断仅影响明细面；渲染窗 64×5,000×128 内
/// 公式/链接数理论上限远大于此，截断防 stats 无界膨胀）。
const FORMULA_INVENTORY_CAP: usize = 200;

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
        // #234：防御性映射（xlsx 读入口未接 OLE2 嗅探——.xls 改名
        // .xlsx 同构问题守边界登记于 #234；变体可达性：仅 docx 入口产
        // 生。若未来 xlsx 接线，码面随之专化）。
        ReadError::LegacyOfficeFormat { .. } => ImportError {
            code: "container_legacy_format",
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
        // #17：修订删除文本登记（docx 面专用；xlsx 面无此类别使用面
        // ——完备性映射）。
        mdc_office_io::io::WarningKind::RevisionDelete => "revision_delete",
        // #39：面外元素跳过（docx 嵌套表深度防御——xlsx 面无触发面，
        // 完备性映射）。
        mdc_office_io::io::WarningKind::SkippedElement => "skipped_element",
        // #233（zip-exotica）：压缩比超观察门限（警告口径不拒——
        // 纵深可见性；见 mdc-office-io LIMIT_COMPRESS_RATIO 文档）。
        mdc_office_io::io::WarningKind::HighCompressionRatio => "high_compression_ratio",
        // #276（B-1）：超链接目标未过 URL 策略（敌意 scheme 消毒
        // 降级；xlsx 读面保持透传，出口由 rebuild scheme 门承载——
        // 此映射为完备性所需）。
        mdc_office_io::io::WarningKind::UnsafeLink => "unsafe_link",
    }
}

/// 引擎警告 kind → 严重度（#22 P2-4：docx 导入 `warning_severity` 同形
/// ——两模块各自内聚，不建跨模块耦合；xlsx 面 `error` 档在此启用：
/// sheets/rows/cols_dropped = **内容整体不可见**[渲染窗外截断]——
/// R-1 实测 101MB 档 rows_dropped=3200，量级面非段落级丢弃）。
fn warning_severity(kind: &mdc_office_io::io::WarningKind) -> &'static str {
    match kind {
        mdc_office_io::io::WarningKind::MalformedXml
        | mdc_office_io::io::WarningKind::BrokenReference => "warn",
        mdc_office_io::io::WarningKind::ControlChars
        | mdc_office_io::io::WarningKind::RevisionDelete => "info",
        // #39：嵌套表深度超限跳过（xlsx 面无触发面——完备性）。
        mdc_office_io::io::WarningKind::SkippedElement => "warn",
        // #233：压缩比观察（合法高压缩比件也触发——s7b 149:1 实证；
        // 内容无缺席，纯可见性）→ info。
        mdc_office_io::io::WarningKind::HighCompressionRatio => "info",
        // #276（B-1）：完备性映射（xlsx 面当前无触发点）→ warn。
        mdc_office_io::io::WarningKind::UnsafeLink => "warn",
    }
}

/// 导入链有损项 → warnings 条目（#22 P2-4，docx 面 `import_lossy_warnings`
/// 同族）：非 0 才进；`count` = stats 同键镜像（单一真源在 stats 键）；
/// `part` = `"xlsx"`（格式面标识）；severity 三档裁决：截断
///（sheets/rows/cols——内容整体不可见）= `error`，越限丢弃
///（formulas/hyperlinks——渲染窗外双通道丢失）= `warn`，结构登记
///（merged_ranges——GFM 降级显式化非损失）= `info`。
fn import_lossy_warnings(
    sheets_dropped: usize,
    rows_dropped: usize,
    cols_dropped: usize,
    formulas_flattened: usize,
    hyperlinks_dropped: usize,
    merged_ranges: usize,
) -> Vec<serde_json::Value> {
    [
        ("sheets_dropped", sheets_dropped, "error"),
        ("rows_dropped", rows_dropped, "error"),
        ("cols_dropped", cols_dropped, "error"),
        ("formulas_flattened", formulas_flattened, "warn"),
        ("hyperlinks_dropped", hyperlinks_dropped, "warn"),
        ("merged_ranges", merged_ranges, "info"),
    ]
    .into_iter()
    .filter(|&(_, count, _)| count > 0)
    .map(|(kind, count, severity)| {
        serde_json::json!({ "kind": kind, "part": "xlsx", "count": count, "severity": severity })
    })
    .collect()
}

/// 单表渲染产物（book_to_markdown 的聚合单元——纯数据）。
/// 列号（0 基）→ Excel 列字母（A..Z, AA..ZZ…——覆盖全列域 16,384
/// = XFD；渲染窗 32 列内恒 ≤ AI）。
fn col_letters(mut col: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

/// 绝对坐标（0 基）→ A1 形引用（stats 清单定位用）。
fn cell_ref(row: u32, col: u32) -> String {
    format!("{}{}", col_letters(col), row + 1)
}

/// GFM destination 形规整（空/空白/`)`/`<>` → 角括号包裹——docx
/// 导入 `angle_wrap` 同族镜像；本模块私有[两导入模块各自内聚，
/// 不建跨模块耦合]）。
fn gfm_angle_wrap(target: &str) -> String {
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

/// 超链接格 → GFM 链接形（P2-3：四字段消费——外链 `target` 优先 /
/// 内链 `location` → `#锚` 形[docx 导入内链同族] / `tooltip` → GFM
/// title ` "tooltip"`（引号→单引号防 GFM title 提前闭合）/ 显示文本
/// 缺省回落单元格值、仍空回落目标串——零静默丢）。`target`/
/// `location` 双缺 = 降级纯文本（calamine 面不可达——防御分支）。
fn render_xlsx_link(
    target: Option<&str>,
    location: Option<&str>,
    tooltip: Option<&str>,
    displayed_text: Option<&str>,
    fallback: &str,
) -> String {
    let dest = if let Some(t) = target {
        gfm_angle_wrap(t)
    } else if let Some(l) = location {
        format!("#{}", gfm_angle_wrap(l))
    } else {
        return fallback.to_string();
    };
    let text = displayed_text
        .filter(|s| !s.is_empty())
        .or_else(|| (!fallback.is_empty()).then_some(fallback))
        .unwrap_or(dest.as_str());
    match tooltip.filter(|t| !t.is_empty()) {
        Some(t) => format!("[{text}]({dest} \"{}\")", t.replace('"', "'")),
        None => format!("[{text}]({dest})"),
    }
}

/// P2-3：渲染窗内公式格清单条目（sheet 名在 book 级聚合时注入）。
struct FormulaEntry {
    row: u32,
    col: u32,
    /// 公式原文（含本层补的 `=` 前缀——calamine 读侧无 = 形）。
    text: String,
    /// 缓存值显示面（CellData::display；None = 无缓存[Empty]）。
    cached: Option<String>,
}

/// P2-3：渲染窗内超链接清单条目（四字段 + 锚格文本）。
struct HyperlinkEntry {
    row: u32,
    col: u32,
    /// 锚格基础文本（链接形包裹前的值面/公式面文本）。
    text: String,
    target: Option<String>,
    location: Option<String>,
    tooltip: Option<String>,
}

/// 单表渲染结果（含 P2-3 双通道清单与越限丢弃计数）。
struct SheetOutcome {
    section: String,
    /// 渲染数据行数（含表头行）；0 = 空表。
    rows: usize,
    /// 越限丢弃行/列数（使用区 − 渲染前缀）。
    rows_dropped: usize,
    cols_dropped: usize,
    /// 渲染窗口内非空内容单元格数（P2-3：公式格计入——公式原文即
    /// 内容；Empty 与空串同视为空）。
    non_empty_cells: usize,
    /// P2-3：渲染窗内公式格清单。
    formulas: Vec<FormulaEntry>,
    /// P2-3：渲染窗内超链接清单（锚格承载）。
    hyperlinks: Vec<HyperlinkEntry>,
    /// P2-3：渲染窗外公式格数（行/列越限或公式网格越出值网格——
    /// 值与文本同弃，`rows/cols_dropped` 已登记其丢弃面）。
    formulas_dropped: usize,
    /// P2-3：锚格越限（渲染窗外）超链接数。
    hyperlinks_dropped: usize,
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
        // 空使用区但公式/链接面在场：全部计越限丢弃（无表可渲染
        // ——计数可见，不静默）。
        return SheetOutcome {
            section: format!("{heading}\n\n(empty sheet)"),
            rows: 0,
            rows_dropped: 0,
            cols_dropped: 0,
            non_empty_cells: 0,
            formulas: Vec::new(),
            hyperlinks: Vec::new(),
            formulas_dropped: count_formulas_outside(sheet, 0, 0),
            hyperlinks_dropped: count_hyperlinks_outside(sheet, 0, 0),
        };
    }
    let rows = height.min(MAX_ROWS);
    let cols = width.min(MAX_COLS);
    // 值网格锚点（绝对坐标基准——P2-3 公式/链接定位用；非空网格
    // 恒有 start）。
    let (base_r, base_c) = sheet.values.start().unwrap_or((0, 0));
    let mut lines: Vec<String> = Vec::with_capacity(rows + 1);
    let mut non_empty_cells = 0usize;
    let mut formulas: Vec<FormulaEntry> = Vec::new();
    let mut hyperlinks: Vec<HyperlinkEntry> = Vec::new();
    for r in 0..rows {
        let mut cells: Vec<String> = Vec::with_capacity(cols);
        for c in 0..cols {
            let abs_r = base_r + r as u32;
            let abs_c = base_c + c as u32;
            // r4-t1：类型化模型显示面（CellData::display——与 calamine
            // Data Display 逐字等值，office-io tests/cell_types.rs 钉定；
            // Empty → ""；显示面兼容 = 既有断言即钉定）。公式格的
            // typed 值 = 缓存计算结果（值面语义——缓存通道进清单）。
            let value_text = sheet
                .typed
                .get((r, c))
                .map(|d| d.display())
                .unwrap_or_default();
            // P2-3 公式双通道：单元格 = 反引号包裹公式原文（补 = 前缀
            // ——calamine 读侧无 = 形，office-io FIDELITY §1）；缓存值
            // 进 stats 清单（结构化面——md 单元格保持 GFM 洁净）。
            let formula_text = sheet.formula_at(abs_r, abs_c).map(|f| format!("={f}"));
            let mut raw = match &formula_text {
                Some(f) => {
                    formulas.push(FormulaEntry {
                        row: abs_r,
                        col: abs_c,
                        text: f.clone(),
                        cached: if value_text.is_empty() {
                            None
                        } else {
                            Some(value_text.clone())
                        },
                    });
                    // 批 5 值面双载（#29 三大发现 #2/#3）：缓存值非空
                    // 追加 `=值` 后缀——值面可见（内容提取/引用场景
                    // 公式格读出数字而非公式文本；错误格缓存 = 错误码
                    // 文本，`#DIV/0!` 即 Excel 显示语义——显示面忠实
                    // 原则）；缓存空（calamine Empty）保持单载。
                    // 写面对称解析（xlsx_export::cell_value——同形判据）。
                    if value_text.is_empty() {
                        format!("`{f}`")
                    } else {
                        format!("`{f}`={value_text}")
                    }
                }
                None => value_text.clone(),
            };
            // P2-3 超链接四字段：锚格（range.start）承载——range 形
            // 链接覆盖区其余格为值面（不重复渲染）；公式格带链接 =
            // 链接形包裹公式形（双通道叠加，零丢弃）。
            if let Some(h) = sheet
                .hyperlinks
                .iter()
                .find(|h| h.range.start == (abs_r, abs_c))
            {
                hyperlinks.push(HyperlinkEntry {
                    row: abs_r,
                    col: abs_c,
                    text: raw.clone(),
                    target: h.target.clone(),
                    location: h.location.clone(),
                    tooltip: h.tooltip.clone(),
                });
                raw = render_xlsx_link(
                    h.target.as_deref(),
                    h.location.as_deref(),
                    h.tooltip.as_deref(),
                    h.displayed_text.as_deref(),
                    &raw,
                );
            }
            if !raw.is_empty() {
                non_empty_cells += 1;
            }
            cells.push(sanitize_cell(&raw));
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
        formulas,
        hyperlinks,
        formulas_dropped: count_formulas_outside(sheet, rows, cols),
        hyperlinks_dropped: count_hyperlinks_outside(sheet, rows, cols),
    }
}

/// P2-3：渲染窗外公式格计数（全公式网格扫描——行/列越限或公式
/// 网格越出值网格；`rows`/`cols` = 渲染窗尺寸，0×0 = 空表全量）。
fn count_formulas_outside(sheet: &SheetData, rows: usize, cols: usize) -> usize {
    let Some((fr, fc)) = sheet.formulas.start() else {
        return 0;
    };
    let (base_r, base_c) = sheet.values.start().unwrap_or((0, 0));
    let mut dropped = 0usize;
    for r in 0..sheet.formulas.height() {
        for c in 0..sheet.formulas.width() {
            if sheet.formulas.get((r, c)).is_some_and(|f| !f.is_empty()) {
                let rel_r = fr as i64 + r as i64 - base_r as i64;
                let rel_c = fc as i64 + c as i64 - base_c as i64;
                if rel_r < 0 || rel_c < 0 || rel_r >= rows as i64 || rel_c >= cols as i64 {
                    dropped += 1;
                }
            }
        }
    }
    dropped
}

/// P2-3：锚格越限超链接计数（range.start 为锚——覆盖区其余格不
/// 重复计；锚格在渲染窗外或先于值网格起点 = 丢弃）。
fn count_hyperlinks_outside(sheet: &SheetData, rows: usize, cols: usize) -> usize {
    let (base_r, base_c) = sheet.values.start().unwrap_or((0, 0));
    sheet
        .hyperlinks
        .iter()
        .filter(|h| {
            let rel_r = h.range.start.0 as i64 - base_r as i64;
            let rel_c = h.range.start.1 as i64 - base_c as i64;
            rel_r < 0 || rel_c < 0 || rel_r >= rows as i64 || rel_c >= cols as i64
        })
        .count()
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
    // P2-3：双通道聚合（接入计数 + 越限丢弃 + 明细清单 ≤200 条）。
    let mut formulas_imported = 0usize;
    let mut hyperlinks_imported = 0usize;
    let mut formulas_dropped = 0usize;
    let mut hyperlinks_dropped = 0usize;
    let mut formula_inventory: Vec<serde_json::Value> = Vec::new();
    let mut hyperlink_inventory: Vec<serde_json::Value> = Vec::new();

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
        formulas_imported += out.formulas.len();
        hyperlinks_imported += out.hyperlinks.len();
        formulas_dropped += out.formulas_dropped;
        hyperlinks_dropped += out.hyperlinks_dropped;
        for f in out.formulas {
            if formula_inventory.len() < FORMULA_INVENTORY_CAP {
                formula_inventory.push(serde_json::json!({
                    "sheet": sheet.name,
                    "cell": cell_ref(f.row, f.col),
                    "text": f.text,
                    "cached": f.cached,
                }));
            }
        }
        for h in out.hyperlinks {
            if hyperlink_inventory.len() < FORMULA_INVENTORY_CAP {
                hyperlink_inventory.push(serde_json::json!({
                    "sheet": sheet.name,
                    "cell": cell_ref(h.row, h.col),
                    "text": h.text,
                    "target": h.target,
                    "location": h.location,
                    "tooltip": h.tooltip,
                }));
            }
        }
        sections.push(out.section);
    }

    let merged_ranges: usize = book.sheets[..rendered].iter().map(|s| s.merges.len()).sum();

    // #22 P2-4：可见警告面——引擎警告（count=1 逐条 + severity）+
    // 导入链有损项聚合（非 0 才进；count = stats 同键镜像单一真源）。
    let mut warnings: Vec<serde_json::Value> = book
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
        book.sheets.len() - rendered,
        rows_dropped,
        cols_dropped,
        formulas_dropped,
        hyperlinks_dropped,
        merged_ranges,
    ));

    let stats = serde_json::json!({
        "sheets": rendered,
        "tables": tables,
        "empty_sheets": empty_sheets,
        "rows": rows_total,
        "cells": cells_total,
        // P2-3：双通道接入计数 + 明细清单（≤200 条——计数恒全量）。
        "formulas_imported": formulas_imported,
        "hyperlinks_imported": hyperlinks_imported,
        "formulas": formula_inventory,
        "hyperlinks": hyperlink_inventory,
        // P2-3 语义收窄：渲染窗外（越限）丢弃——正常表恒 0（P2-3 前
        // 为全量丢弃计数——键保留，消费方兼容）。
        "formulas_flattened": formulas_dropped,
        "merged_ranges": merged_ranges,
        "hyperlinks_dropped": hyperlinks_dropped,
        "sheets_dropped": book.sheets.len() - rendered,
        "rows_dropped": rows_dropped,
        "cols_dropped": cols_dropped,
        "warnings": warnings,
    });
    (sections.join("\n\n"), stats)
}

/// 导入内核（字节入口；纯函数——无 JsValue，原生 cargo test 直测）。
/// 成功返回与 wasm 面同形的 JSON 串 `{"markdown": ..., "stats": ...}`。
///
/// 可见性：pub（阶段 B 六维体检 runner 消费——集成测试面可见性加宽；
/// 纯函数语义零变化，xlsx_grid `*_core` 先例同构）。
pub fn import_xlsx_core(bytes: &[u8]) -> Result<String, ImportError> {
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
    /// P2-3：公式格 = 反引号包裹公式原文；批 5 值面双载：缓存值非空
    /// 追加 `=值` 后缀；超链接格 = [text](url) GFM 形。
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
             | Gadget | `=SUM(B2:B2)*2`=9 | [docs](https://example.com/docs) |\n\
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
        assert_eq!(stats["cells"], 14); // Data 10 + Notes 4（公式格计入）
                                        // P2-3：双通道接入——渲染窗内计数恒全量，越限丢弃恒 0。
        assert_eq!(stats["formulas_imported"], 1);
        assert_eq!(stats["hyperlinks_imported"], 1);
        assert_eq!(stats["formulas_flattened"], 0);
        assert_eq!(stats["hyperlinks_dropped"], 0);
        // P2-3：明细清单（公式含缓存值通道；链接四字段）。
        assert_eq!(
            stats["formulas"],
            serde_json::json!([{
                "sheet": "Data", "cell": "B3",
                "text": "=SUM(B2:B2)*2", "cached": "9"
            }])
        );
        assert_eq!(
            stats["hyperlinks"],
            serde_json::json!([{
                "sheet": "Data", "cell": "C3", "text": "docs",
                "target": "https://example.com/docs",
                "location": null, "tooltip": null
            }])
        );
        assert_eq!(stats["merged_ranges"], 1);
        assert_eq!(stats["sheets_dropped"], 0);
        assert_eq!(stats["rows_dropped"], 0);
        assert_eq!(stats["cols_dropped"], 0);
        // #22 P2-4：merged_ranges（结构登记非损失）→ info 档可见警告；
        // 截断/越限项全 0 → 不进（无事不噪声）；零损样例恰一条。
        assert_eq!(
            stats["warnings"],
            serde_json::json!([{ "kind": "merged_ranges", "part": "xlsx", "count": 1, "severity": "info" }])
        );
    }

    /// P2-3：公式双通道——单元格 = 反引号公式原文；缓存值进 stats
    /// 清单（结构化面；渲染窗内零丢弃）+ 批 5 值面双载：缓存值面
    /// 在 md 单元格可见（`=formula`=cached 形）。
    #[test]
    fn formula_policy_keeps_text_and_cached_channels() {
        let payload = import_json(&build_sample_xlsx());
        let md = payload["markdown"].as_str().expect("markdown");
        assert!(
            md.contains("| Gadget | `=SUM(B2:B2)*2`=9 | [docs](https://example.com/docs) |"),
            "公式格应为双载形：公式原文 + `=缓存值` + 链接格 GFM 形: {md}"
        );
        // 值面不再缺席（#29 三大发现 #2）：公式格的缓存值在 md 可见。
        assert!(md.contains("=9"), "缓存值面缺席: {md}");
        // 双通道清单面保持：文本与缓存在清单（"cached": "9"）。
        assert_eq!(payload["stats"]["formulas_imported"], 1);
        assert_eq!(
            payload["stats"]["formulas"][0]["cached"],
            serde_json::json!("9")
        );
        assert_eq!(payload["stats"]["formulas_flattened"], 0);
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
        // #22 P2-4：同上——merged_ranges info 档可见警告。
        assert_eq!(payload["stats"]["warnings"][0]["kind"], "merged_ranges");
        assert_eq!(payload["stats"]["warnings"][0]["severity"], "info");
    }

    /// 规模上限：表数/行/列前缀截断 + 丢弃计数显式化。批 5 窗口提级
    /// 后夹具稀疏化：窗口越限表仅点写边界格（used range 包围盒由非空
    /// 格撑开——高度/宽度越限不需稠密矩阵，测试不随窗口提级线性变重）。
    #[test]
    fn caps_truncate_with_drop_stats() {
        use mdc_office_io::write::{CellValue, WorkbookWriter};
        let mut w = WorkbookWriter::new();
        // MAX_SHEETS + 3 表（前 MAX_SHEETS 渲染，3 丢弃）。
        for i in 0..(MAX_SHEETS + 3) {
            let mut s = w.add_sheet(&format!("S{i}")).expect("sheet");
            // 表 0 行窗越限：(0,0) 与 (MAX_ROWS+4,0) 两点撑开高度
            // MAX_ROWS+5（渲染 MAX_ROWS，丢弃 5）。
            // 表 1 列窗越限：(0,0) 与 (0,MAX_COLS+2) 两点撑开宽度
            // MAX_COLS+3（渲染 MAX_COLS，丢弃 3）。
            // 其余表 2×2 常规。
            match i {
                0 => {
                    s.write(0, 0, CellValue::Number(1.0), None).expect("pt a");
                    s.write((MAX_ROWS + 4) as u32, 0, CellValue::Number(2.0), None)
                        .expect("pt b");
                }
                1 => {
                    s.write(0, 0, CellValue::Number(1.0), None).expect("pt a");
                    s.write(0, (MAX_COLS + 2) as u16, CellValue::Number(2.0), None)
                        .expect("pt b");
                }
                _ => {
                    for r in 0..2u32 {
                        let cells = [CellValue::Number(1.0), CellValue::Number(2.0)];
                        s.write_row(r, &cells).expect("row");
                    }
                }
            }
        }
        let payload = import_json(&w.save_to_vec().expect("save"));
        let stats = &payload["stats"];
        assert_eq!(stats["sheets"], MAX_SHEETS);
        assert_eq!(stats["sheets_dropped"], 3);
        assert_eq!(stats["rows_dropped"], 5); // 表 0：高度 MAX_ROWS+5 − MAX_ROWS
        assert_eq!(stats["cols_dropped"], 3); // 表 1：宽度 MAX_COLS+3 − MAX_COLS
                                              // #22 P2-4：截断面（内容整体不可见）→ error 档可见警告三连
                                              //（count = stats 同键镜像）。
        let warns = stats["warnings"].as_array().expect("warnings array");
        for (kind, count) in [
            ("sheets_dropped", 3),
            ("rows_dropped", 5),
            ("cols_dropped", 3),
        ] {
            let hit = warns
                .iter()
                .find(|w| w["kind"] == kind && w["severity"] == "error")
                .unwrap_or_else(|| panic!("{kind} error 条目缺席: {warns:?}"));
            assert_eq!(hit["count"], count, "{kind} 计数镜像");
            assert_eq!(hit["part"], "xlsx");
        }
        // 表 0 渲染 MAX_ROWS 行 + 表 1 1 行 + 62 常规表 × 2 行。
        assert_eq!(stats["rows"], MAX_ROWS + 1 + (MAX_SHEETS - 2) * 2);
        let md = payload["markdown"].as_str().expect("markdown");
        // 全部渲染表有内容 → 表节 = 上限数。
        let sheet_sections = md.split("\n\n## ").count();
        assert_eq!(sheet_sections, MAX_SHEETS, "渲染表节数应为上限");
        let s0 = md.split("\n\n## ").next().expect("first section");
        // 标题 + 空行 +（表头 + 分隔行 + MAX_ROWS 行）= 3 + MAX_ROWS 行。
        assert_eq!(
            s0.lines().count(),
            3 + MAX_ROWS,
            "首表行数 = 表头+分隔+MAX_ROWS"
        );
    }

    /// 批 5 窗口提级（#29 三大发现 #1）：真实数据簿规模（196 行 × 36 列
    /// ——体检实测 minimal_package 形态：原 50/32 窗丢 146 行/4 列召回
    /// 0.242）全量导入零丢弃、零截断警告。
    #[test]
    fn window_raise_covers_real_scale_sheets() {
        use mdc_office_io::write::{CellValue, WorkbookWriter};
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet("register").expect("sheet");
        for r in 0..196u32 {
            let cells: Vec<CellValue> = (0..36)
                .map(|c| CellValue::Text(format!("v{r}-{c}")))
                .collect();
            s.write_row(r, &cells).expect("row");
        }
        let payload = import_json(&w.save_to_vec().expect("save"));
        let stats = &payload["stats"];
        assert_eq!(stats["rows_dropped"], 0, "196 行簿不再截断");
        assert_eq!(stats["cols_dropped"], 0, "36 列簿不再截断");
        assert_eq!(stats["rows"], 196);
        assert_eq!(stats["sheets_dropped"], 0);
        assert!(
            stats["warnings"]
                .as_array()
                .expect("warnings")
                .iter()
                .all(|w| !matches!(
                    w["kind"].as_str(),
                    Some("rows_dropped") | Some("cols_dropped")
                )),
            "无截断警告: {}",
            stats["warnings"]
        );
        let md = payload["markdown"].as_str().expect("markdown");
        assert!(
            md.contains("v195-35"),
            "末行末列在场（原窗口丢 146 行）: {md}"
        );
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
        // #234：防御性臂（xlsx 入口不可达——见 map_error 注释）。
        let e = map_error(&ReadError::LegacyOfficeFormat { expect: "xlsx" });
        assert_eq!(e.code, "container_legacy_format");
        assert!(e.message.contains("OLE2"));
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
        // P2-3：夹具内公式/链接接入（原 flattened=1 → imported=1）。
        assert_eq!(payload["stats"]["formulas_imported"], 1);
        assert_eq!(payload["stats"]["hyperlinks_imported"], 1);
        assert_eq!(payload["stats"]["formulas_flattened"], 0);
        assert_eq!(payload["stats"]["hyperlinks_dropped"], 0);
    }

    /// P2-3：render_xlsx_link 纯函数矩阵——外链/内链/tooltip/显示
    ///文本回落/双缺降级五面（无 calamine 类型依赖——字段经 &str 传参）。
    #[test]
    fn render_xlsx_link_matrix() {
        // 外链 + 显示文本。
        assert_eq!(
            render_xlsx_link(
                Some("https://example.com/tender"),
                None,
                None,
                Some("招标公告"),
                "cell text"
            ),
            "[招标公告](https://example.com/tender)"
        );
        // 外链 + tooltip → GFM title（双引号防提前闭合 → 单引号）。
        assert_eq!(
            render_xlsx_link(
                Some("https://example.com"),
                None,
                Some("点我\"看\"详情"),
                None,
                "cell text"
            ),
            "[cell text](https://example.com \"点我'看'详情\")"
        );
        // 内链 location → #锚形（docx 导入内链同族；无空白/`)` 不需角括号）。
        assert_eq!(
            render_xlsx_link(None, Some("'Sheet2'!A1"), None, Some("去表二"), ""),
            "[去表二](#'Sheet2'!A1)"
        );
        // 显示文本缺省回落单元格值。
        assert_eq!(
            render_xlsx_link(Some("https://x.io"), None, None, None, "值面文本"),
            "[值面文本](https://x.io)"
        );
        // 双空显示 → 回落目标串（零静默丢）。
        assert_eq!(
            render_xlsx_link(Some("https://x.io"), None, None, None, ""),
            "[https://x.io](https://x.io)"
        );
        // target/location 双缺 → 降级纯文本（calamine 面不可达——防御）。
        assert_eq!(
            render_xlsx_link(None, None, None, Some("x"), "fallback"),
            "fallback"
        );
    }

    /// P2-3 验收钉定（#22）：真 Excel 夹具 excel-complex.xlsx——
    /// `=SUM(B2:B4)` 与超链接 URL 都在输出中。夹具：
    /// test-artifacts/TR2/fixtures/excel-complex.xlsx（真 Excel 生成，
    /// TR-2D 报告源——含 2 公式 + 1 外链[rels 形，openpyxl 漏读、
    /// calamine 面读到的那个]）。
    #[test]
    fn excel_complex_fixture_formula_link_acceptance() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test-artifacts/TR2/fixtures/excel-complex.xlsx"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
        let payload = import_json(&bytes);
        let md = payload["markdown"].as_str().expect("markdown");
        // 验收：公式文本（含 = 前缀）在输出中。
        assert!(md.contains("`=SUM(B2:B4)`"), "公式原文缺失: {md}");
        assert!(md.contains("`=SUM(C2:C4)`"), "第二公式缺失: {md}");
        // 验收：超链接 URL 在输出中（GFM 链接形；夹具自带 tooltip
        // = 显示文本 → GFM title 形）。
        assert!(
            md.contains("[招标公告](https://example.com/tender \"招标公告\")"),
            "超链接 GFM 形缺失: {md}"
        );
        let stats = &payload["stats"];
        assert_eq!(stats["formulas_imported"], 2);
        assert_eq!(stats["hyperlinks_imported"], 1);
        assert_eq!(stats["formulas_flattened"], 0);
        assert_eq!(stats["hyperlinks_dropped"], 0);
        // 双通道：缓存值进清单（B5 = 100；C5 = 1）。
        let formulas = stats["formulas"].as_array().expect("formulas");
        assert_eq!(formulas.len(), 2);
        assert_eq!(formulas[0]["cell"], "B5");
        assert_eq!(formulas[0]["text"], "=SUM(B2:B4)");
        assert_eq!(formulas[0]["cached"], "100");
        assert_eq!(formulas[1]["cell"], "C5");
        assert_eq!(formulas[1]["cached"], "1");
        // 链接四字段清单。
        let links = stats["hyperlinks"].as_array().expect("hyperlinks");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0]["cell"], "A7");
        assert_eq!(links[0]["target"], "https://example.com/tender");
    }

    /// P2-3：越限丢弃计数语义——渲染窗外（行 > MAX_ROWS）公式与
    /// 锚格越限链接计 `formulas_flattened`/`hyperlinks_dropped`，
    /// 窗内计数零漂移。
    #[test]
    fn over_limit_formulas_and_links_count_dropped() {
        use mdc_office_io::write::{CellValue, FormulaCache, HyperlinkSpec, WorkbookWriter};
        let mut w = WorkbookWriter::new();
        let mut s = w.add_sheet("Big").expect("sheet");
        s.write_row(0, &[CellValue::Text("h".into())]).expect("row");
        // MAX_ROWS + 5 行：首行 + 越限区公式（行索引 MAX_ROWS+2）。
        let far_row = (MAX_ROWS + 2) as u32;
        s.write(
            far_row,
            0,
            CellValue::Formula {
                text: "1+1".into(),
                cached: Some(FormulaCache::Number(2.0)),
            },
            None,
        )
        .expect("far formula");
        // 越限区链接（锚格同行）。
        s.write_hyperlink(
            far_row,
            1,
            HyperlinkSpec {
                url: "https://example.com/far".into(),
                display_text: None,
                tooltip: None,
            },
        )
        .expect("far link");
        let payload = import_json(&w.save_to_vec().expect("save"));
        let stats = &payload["stats"];
        // 窗内零公式零链接（唯一公式/链接都在越限行）。
        assert_eq!(stats["formulas_imported"], 0);
        assert_eq!(stats["hyperlinks_imported"], 0);
        assert_eq!(stats["formulas_flattened"], 1);
        assert_eq!(stats["hyperlinks_dropped"], 1);
        assert_eq!(stats["rows_dropped"], far_row as usize + 1 - MAX_ROWS);
        assert_eq!(stats["formulas"].as_array().map(Vec::len), Some(0));
        assert_eq!(stats["hyperlinks"].as_array().map(Vec::len), Some(0));
        let md = payload["markdown"].as_str().expect("markdown");
        assert!(!md.contains("1+1"), "越限公式不得进 md: {md}");
        assert!(!md.contains("far"), "越限链接不得进 md: {md}");
    }
}
