//! 表格表面数据面（阶段 5 §八.3 前置件）——XLSX 类型化网格的 wasm 出口。
//!
//! ## 定位
//! [`WasmBook`] = 一次解析常驻句柄（`Book` 生命周期挂 JS 侧——与
//! [`crate::WasmCore`] 同模式）；可视窗切片（`slice_json`）= 表格
//! 虚拟滚动的数据源。**单一真相**：typed 值与 display 串均出自
//! mdc-office-io 读面（与 xlsx_import 渲染面/t17 显示面同源）——
//! 宿主不建第二份表格数据。
//!
//! ## 出口形状
//! - [`WasmBook::from_bytes`]：字节入口（`read_book_from_bytes`——
//!   同一套读面安全门：字节数/表数/包围盒总量三前置检查）。
//! - [`WasmBook::sheets_json`]：表清单（名字 + 使用区行列 + 非空格数）。
//! - [`WasmBook::slice_json`]：可视窗切片——typed kind + 原值 + display
//!   串三件套；越界钳制（窗超使用区 = 空格填充语义，非错误）。
//! - [`WasmBook::cell_display`] / [`WasmBook::cell_kind`]：单格面
//!   （编辑定位/证据高亮钩子）。
//!
//! ## 安全
//! 切片窗规模有硬上限（`MAX_SLICE_CELLS`——与读面总量门同纪律，
//! 防单窗巨型分配）；sheet 索引越界 = typed 错误。
//! 纯内核（`*_core`）不触碰 JsValue → 原生 cargo test 可测。
//!
//! ## 读-改-写出口（p5-t2 表格 UI 产品化；#283 接线升级）
//! [`WasmBook::export_with_edits`]：编辑账本 JSON（命令形态
//! `[{sheet,r,c,k,v}]`）→ `mdc_office_io::rebuild::export_edited`
//! （#283：XlsxEditor 原位手术优先 + rebuild 兜底）→ 新簿字节
//! （Uint8Array）；[`WasmBook::last_rebuild_stats_json`] = 最近一次
//! 导出统计（六键恒在：`path`/`sheets`/`cells_written`/`merges`/
//! `hyperlinks`/`edits_applied`/`recalc_cells`/`skipped`/
//! `fallback_reasons`——`skipped` 非空时宿主须警示，`path`
//! `in_place` = 结构性面保真、`rebuild` = 诚实账本警示面）。
//! 错误码：`rebuild_edits_json`（账本解析）/ `rebuild_invalid`
//!（含 FFFE/FFFF 编辑拒）/ `rebuild_limit` / `rebuild_engine`。

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use mdc_office_io::io::{read_book_from_bytes, Book, CellData, ReadError};
use mdc_office_io::rebuild::{export_edited, CellEdit, EditValue, ExportPath, ExportStats};

/// 单窗切片格数上限（读面总量门同纪律——防巨型窗口分配）。
pub const MAX_SLICE_CELLS: u32 = 100_000;

/// 编辑账本条数上限（防巨粘贴/巨账本 DoS——宿主侧 10000 限的硬底）。
pub const MAX_EDIT_CELLS: u32 = 100_000;

/// WasmBook 构造错误（typed 码——宿主可 JSON.parse 出结构化信息）。
#[derive(Debug)]
pub enum GridError {
    Read(ReadError),
    SheetIndex {
        index: usize,
        sheets: usize,
    },
    SliceTooLarge {
        cells: u64,
        max: u32,
    },
    /// 编辑账本 JSON 解析失败（宿主侧序列化 bug——形态面非数据面）。
    EditsJson(String),
    /// 读-改-重建失败（InvalidInput/Limit/Engine 三分透传）。
    Rebuild {
        code: &'static str,
        message: String,
    },
}

impl GridError {
    fn code(&self) -> &'static str {
        match self {
            GridError::Read(_) => "grid_read_failed",
            GridError::SheetIndex { .. } => "grid_sheet_index",
            GridError::SliceTooLarge { .. } => "grid_slice_too_large",
            GridError::EditsJson(_) => "rebuild_edits_json",
            GridError::Rebuild { code, .. } => code,
        }
    }
    fn detail(&self) -> String {
        match self {
            GridError::Read(e) => format!("{e}"),
            GridError::SheetIndex { index, sheets } => {
                format!("sheet index {index} out of range ({sheets} sheets)")
            }
            GridError::SliceTooLarge { cells, max } => {
                format!("slice window {cells} cells exceeds max {max}")
            }
            GridError::EditsJson(m) => format!("edits json parse failed: {m}"),
            GridError::Rebuild { message, .. } => message.clone(),
        }
    }
    fn to_js(&self) -> JsValue {
        JsValue::from_str(&format!(
            "{{\"code\":\"{}\",\"message\":\"{}\"}}",
            self.code(),
            self.detail().replace('"', "'")
        ))
    }
}

/// 单格 JSON 三件套（kind / 原值 / display）。
fn cell_json(cd: &CellData) -> String {
    let d = cd.display();
    match cd {
        CellData::Text(s) => {
            format!(
                "{{\"k\":\"text\",\"v\":{},\"d\":{}}}",
                serde_json::to_string(s).unwrap(),
                serde_json::to_string(&d).unwrap()
            )
        }
        CellData::Number(v) => format!(
            "{{\"k\":\"number\",\"v\":{v},\"d\":{}}}",
            serde_json::to_string(&d).unwrap()
        ),
        CellData::Boolean(b) => format!(
            "{{\"k\":\"bool\",\"v\":{b},\"d\":{}}}",
            serde_json::to_string(&d).unwrap()
        ),
        CellData::DateTime { serial, .. } | CellData::Duration { serial, .. } => {
            format!(
                "{{\"k\":\"datetime\",\"v\":{serial},\"d\":{}}}",
                serde_json::to_string(&d).unwrap()
            )
        }
        CellData::DateTimeIso(s) | CellData::DurationIso(s) => format!(
            "{{\"k\":\"datetime_iso\",\"v\":{},\"d\":{}}}",
            serde_json::to_string(s).unwrap(),
            serde_json::to_string(&d).unwrap()
        ),
        CellData::Error(code) => format!(
            "{{\"k\":\"error\",\"v\":{},\"d\":{}}}",
            serde_json::to_string(code).unwrap(),
            serde_json::to_string(&d).unwrap()
        ),
        CellData::Empty => "{\"k\":\"empty\"}".to_string(),
    }
}

// ── 纯内核（JsValue-free，原生 cargo test 面）──────────────────────

/// 导出统计 → JSON（两路径统一形状——#283：`path` 面板 + 六键恒在
/// + `recalc_cells` 增量暴露；web 宿主按 `skipped` 长度亮警）。
pub fn export_stats_json(stats: &ExportStats) -> String {
    let skipped: Vec<String> = stats
        .skipped
        .iter()
        .map(|s| serde_json::to_string(s).unwrap())
        .collect();
    let fallback: Vec<String> = stats
        .fallback_reasons
        .iter()
        .map(|s| serde_json::to_string(s).unwrap())
        .collect();
    let path = match stats.path {
        ExportPath::InPlace => "in_place",
        ExportPath::Rebuild => "rebuild",
    };
    format!(
        "{{\"path\":\"{path}\",\"sheets\":{},\"cells_written\":{},\"merges\":{},\"hyperlinks\":{},\"edits_applied\":{},\"recalc_cells\":{},\"skipped\":[{}],\"fallback_reasons\":[{}]}}",
        stats.sheets,
        stats.cells_written,
        stats.merges,
        stats.hyperlinks,
        stats.edits_applied,
        stats.recalc_cells,
        skipped.join(","),
        fallback.join(","),
    )
}

/// 编辑账本 JSON 解析（`[{sheet,r,c,k,v}]`；k ∈ text/number/bool/
/// empty——empty 无 v）。同格后写胜由编辑账本分组承载。
pub fn parse_edits_core(edits_json: &str) -> Result<Vec<CellEdit>, GridError> {
    let v: serde_json::Value =
        serde_json::from_str(edits_json).map_err(|e| GridError::EditsJson(e.to_string()))?;
    let arr = v
        .as_array()
        .ok_or_else(|| GridError::EditsJson("expected top-level JSON array".to_string()))?;
    if arr.len() > MAX_EDIT_CELLS as usize {
        return Err(GridError::EditsJson(format!(
            "edits array {} entries exceeds max {}",
            arr.len(),
            MAX_EDIT_CELLS
        )));
    }
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let sheet = item
            .get("sheet")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| GridError::EditsJson(format!("item {i}: missing sheet")))?;
        let r = item
            .get("r")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| GridError::EditsJson(format!("item {i}: missing r")))?;
        let c = item
            .get("c")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| GridError::EditsJson(format!("item {i}: missing c")))?;
        let k = item
            .get("k")
            .and_then(|x| x.as_str())
            .ok_or_else(|| GridError::EditsJson(format!("item {i}: missing k")))?;
        let value =
            match k {
                "text" => EditValue::Text(
                    item.get("v")
                        .and_then(|x| x.as_str())
                        .ok_or_else(|| {
                            GridError::EditsJson(format!("item {i}: k=text missing v string"))
                        })?
                        .to_string(),
                ),
                "number" => {
                    EditValue::Number(item.get("v").and_then(|x| x.as_f64()).ok_or_else(|| {
                        GridError::EditsJson(format!("item {i}: k=number missing v f64"))
                    })?)
                }
                "bool" => EditValue::Boolean(item.get("v").and_then(|x| x.as_bool()).ok_or_else(
                    || GridError::EditsJson(format!("item {i}: k=bool missing v bool")),
                )?),
                "empty" => EditValue::Empty,
                other => return Err(GridError::EditsJson(format!("item {i}: unknown k {other}"))),
            };
        let (r, c) = (
            u32::try_from(r).map_err(|_| GridError::EditsJson(format!("item {i}: r overflow")))?,
            u32::try_from(c).map_err(|_| GridError::EditsJson(format!("item {i}: c overflow")))?,
        );
        let sheet = usize::try_from(sheet)
            .map_err(|_| GridError::EditsJson(format!("item {i}: sheet overflow")))?;
        out.push(CellEdit {
            sheet,
            row: r,
            col: c,
            value,
        });
    }
    Ok(out)
}

/// 读-改-写纯内核（原生测试面）：`(新簿字节, 统计 JSON)`。
/// #283：`export_edited` 编排（in-place 优先 + rebuild 兜底）。
pub fn export_with_edits_core(
    book: &Book,
    bytes: &[u8],
    edits_json: &str,
) -> Result<(Vec<u8>, String), GridError> {
    let edits = parse_edits_core(edits_json)?;
    let (bytes, stats) = export_edited(book, bytes, &edits).map_err(|e| GridError::Rebuild {
        code: match &e {
            mdc_office_io::write::WriteError::Limit(_) => "rebuild_limit",
            mdc_office_io::write::WriteError::InvalidInput(_) => "rebuild_invalid",
            mdc_office_io::write::WriteError::Engine(_) => "rebuild_engine",
        },
        message: format!("{e}"),
    })?;
    Ok((bytes, export_stats_json(&stats)))
}

pub fn sheets_core(book: &Book) -> String {
    let sheets: Vec<String> = book
        .sheets
        .iter()
        .map(|s| {
            let (rows, cols, ext_rows, ext_cols) = match (s.typed.start(), s.typed.end()) {
                (Some((r0, c0)), Some((r1, c1))) => {
                    // 高宽（r1-r0+1）与绝对 extent（end+1）双暴露——
                    // 使用区非原点锚定的表（dimension 声明从 B2 起等）
                    // 两者不同，宿主网格须按 extent 建域（审查 S1）。
                    (r1 - r0 + 1, c1 - c0 + 1, r1 + 1, c1 + 1)
                }
                _ => (0, 0, 0, 0),
            };
            format!(
                "{{\"name\":{},\"rows\":{rows},\"cols\":{cols},\"ext_rows\":{ext_rows},\"ext_cols\":{ext_cols}}}",
                serde_json::to_string(&s.name).unwrap()
            )
        })
        .collect();
    format!("[{}]", sheets.join(","))
}

pub fn slice_core(
    book: &Book,
    sheet: usize,
    r0: u32,
    c0: u32,
    rows: u32,
    cols: u32,
) -> Result<String, GridError> {
    let sh = book.sheets.get(sheet).ok_or(GridError::SheetIndex {
        index: sheet,
        sheets: book.sheets.len(),
    })?;
    let cells = rows as u64 * cols as u64;
    if cells > MAX_SLICE_CELLS as u64 {
        return Err(GridError::SliceTooLarge {
            cells,
            max: MAX_SLICE_CELLS,
        });
    }
    // 使用区外钳制语义：窗外/空表 = empty 填充（虚拟滚动的空白区）
    let mut out_rows = Vec::with_capacity(rows as usize);
    for r in r0..r0.saturating_add(rows) {
        let mut row_cells = Vec::with_capacity(cols as usize);
        for c in c0..c0.saturating_add(cols) {
            let cell = match sh.typed.get_abs(r, c) {
                Some(cd) => cell_json(cd),
                None => "{\"k\":\"empty\"}".to_string(),
            };
            row_cells.push(cell);
        }
        out_rows.push(format!("[{}]", row_cells.join(",")));
    }
    Ok(format!(
        "{{\"sheet\":{},\"r0\":{r0},\"c0\":{c0},\"rows\":{rows},\"cols\":{cols},\"cells\":[{}]}}",
        serde_json::to_string(&sh.name).unwrap(),
        out_rows.join(",")
    ))
}

// ── wasm-bindgen 面 ────────────────────────────────────────────────

/// 一次解析常驻的类型化网格句柄（表格表面数据源）。
#[wasm_bindgen]
pub struct WasmBook {
    book: Book,
    /// 源档字节（#283——in-place 原位手术原料：导出时编辑面需要
    /// 原始部件透传，非读面抽象）。内存代价 = 源档一倍，与编辑功能
    /// 一起在场（只读消费方仍用 book 面）。
    bytes: Vec<u8>,
    /// 最近一次导出统计（`export_with_edits` 写入 / 宿主
    /// `last_rebuild_stats_json` 读取——`&self` 下的 interior
    /// mutability）。
    last_stats: RefCell<String>,
}

#[wasm_bindgen]
impl WasmBook {
    /// 字节入口（read_book_from_bytes——同一套读面安全门）。
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<WasmBook, JsValue> {
        let book = read_book_from_bytes(bytes).map_err(|e| GridError::Read(e).to_js())?;
        Ok(WasmBook {
            book,
            bytes: bytes.to_vec(),
            last_stats: RefCell::new(String::new()),
        })
    }

    /// 表清单 JSON：[{name, rows, cols}]。
    pub fn sheets_json(&self) -> String {
        sheets_core(&self.book)
    }

    /// 可视窗切片 JSON（typed kind/原值/display 三件套；窗外 = empty）。
    pub fn slice_json(
        &self,
        sheet: usize,
        r0: u32,
        c0: u32,
        rows: u32,
        cols: u32,
    ) -> Result<String, JsValue> {
        slice_core(&self.book, sheet, r0, c0, rows, cols).map_err(|e| e.to_js())
    }

    /// 单格 display 串（编辑定位/证据高亮钩子）。
    pub fn cell_display(&self, sheet: usize, r: u32, c: u32) -> Result<String, JsValue> {
        let sh = self.book.sheets.get(sheet).ok_or(
            GridError::SheetIndex {
                index: sheet,
                sheets: self.book.sheets.len(),
            }
            .to_js(),
        )?;
        Ok(sh
            .typed
            .get_abs(r, c)
            .map(|cd| cd.display())
            .unwrap_or_default())
    }

    /// 单格 kind（text/number/bool/datetime/error/empty——编辑面语义）。
    pub fn cell_kind(&self, sheet: usize, r: u32, c: u32) -> Result<String, JsValue> {
        let sh = self.book.sheets.get(sheet).ok_or(
            GridError::SheetIndex {
                index: sheet,
                sheets: self.book.sheets.len(),
            }
            .to_js(),
        )?;
        let kind = match sh.typed.get_abs(r, c) {
            Some(CellData::Text(_)) => "text",
            Some(CellData::Number(_)) => "number",
            Some(CellData::Boolean(_)) => "bool",
            Some(CellData::DateTime { .. } | CellData::Duration { .. }) => "datetime",
            Some(CellData::DateTimeIso(_) | CellData::DurationIso(_)) => "datetime_iso",
            Some(CellData::Error(_)) => "error",
            Some(CellData::Empty) | None => "empty",
        };
        Ok(kind.to_string())
    }

    /// 读-改-写导出（#283：in-place 优先 + rebuild 兜底）：编辑账本
    /// JSON → 新簿字节（Uint8Array）。统计面见
    /// [`Self::last_rebuild_stats_json`]。
    pub fn export_with_edits(&self, edits_json: &str) -> Result<Vec<u8>, JsValue> {
        let (bytes, stats) =
            export_with_edits_core(&self.book, &self.bytes, edits_json).map_err(|e| e.to_js())?;
        *self.last_stats.borrow_mut() = stats;
        Ok(bytes)
    }

    /// 最近一次 `export_with_edits` 统计 JSON（未导出 = 空串）。
    pub fn last_rebuild_stats_json(&self) -> String {
        self.last_stats.borrow().clone()
    }
}
