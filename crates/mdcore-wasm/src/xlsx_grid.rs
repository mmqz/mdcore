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

use wasm_bindgen::prelude::*;

use mdc_office_io::io::{read_book_from_bytes, Book, CellData, ReadError};

/// 单窗切片格数上限（读面总量门同纪律——防巨型窗口分配）。
pub const MAX_SLICE_CELLS: u32 = 100_000;

/// WasmBook 构造错误（typed 码——宿主可 JSON.parse 出结构化信息）。
#[derive(Debug)]
pub enum GridError {
    Read(ReadError),
    SheetIndex { index: usize, sheets: usize },
    SliceTooLarge { cells: u64, max: u32 },
}

impl GridError {
    fn code(&self) -> &'static str {
        match self {
            GridError::Read(_) => "grid_read_failed",
            GridError::SheetIndex { .. } => "grid_sheet_index",
            GridError::SliceTooLarge { .. } => "grid_slice_too_large",
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

pub fn sheets_core(book: &Book) -> String {
    let sheets: Vec<String> = book
        .sheets
        .iter()
        .map(|s| {
            let (rows, cols) = match (s.typed.start(), s.typed.end()) {
                (Some((r0, c0)), Some((r1, c1))) => (r1 - r0 + 1, c1 - c0 + 1),
                _ => (0, 0),
            };
            format!(
                "{{\"name\":{},\"rows\":{rows},\"cols\":{cols}}}",
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
}

#[wasm_bindgen]
impl WasmBook {
    /// 字节入口（read_book_from_bytes——同一套读面安全门）。
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<WasmBook, JsValue> {
        let book = read_book_from_bytes(bytes).map_err(|e| GridError::Read(e).to_js())?;
        Ok(WasmBook { book })
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
}
