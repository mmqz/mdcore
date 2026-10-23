//! 真实文档摄取（office-io 引擎面 ↔ evidence 契约面的互译层）。
//!
//! 只做三件事：读文件 → 确定性语义指纹 → 提取可定位的值/段落。
//! **不验证、不判定**——验证归 pipeline（程序验证纪律），判定归
//! sidecar 状态机。

use std::path::Path;

use mdc_evidence::{
    ContentFingerprint, Decimal, DocId, DocumentRef, EngineKind, Fingerprinter, Fnv64, VersionStamp,
};
use mdc_office_io::docx::{read_docx, read_docx_from_bytes, Block};
use mdc_office_io::io::{read_book, read_book_from_bytes, Book, CellData};

use crate::pipeline::WorkbenchError;

// ---------------------------------------------------------------------------
// XLSX 来源摄取
// ---------------------------------------------------------------------------

/// 单个非空源格（A1 坐标 + 确定性值表示 + 十进制抽取 + 公式通道）。
#[derive(Debug, Clone, PartialEq)]
pub struct SourceCell {
    pub sheet: String,
    /// A1 记法（大写规范化，如 `C7`）。
    pub coord: String,
    /// 0 起绝对行。
    pub row: u32,
    /// 0 起绝对列。
    pub col: u32,
    /// 确定性值表示（指纹与对照的唯一真源；`n:`/`t:`/`b:`/… 前缀
    /// 区分同文本异类型）。
    pub repr: String,
    /// 数值面（Number 格可无损转十进制时；否则 None——使用点显式
    /// 报错，不猜）。
    pub decimal: Option<Decimal>,
    /// 原始文本面（Text 格）。
    pub text: Option<String>,
    /// 公式文本（无 `=` 前缀——office-io 读面口径；非公式格 None）。
    pub formula: Option<String>,
}

/// XLSX 来源摄取产物。
#[derive(Debug, Clone, PartialEq)]
pub struct XlsxSource {
    pub doc_id: String,
    /// 非空格（表序→行→列确定性序）。
    pub cells: Vec<SourceCell>,
    /// 语义指纹（非空格 repr 序——样式等非语义面不参与）。
    pub fingerprint: ContentFingerprint,
}

impl XlsxSource {
    /// 按 A1 坐标取格。
    pub fn cell(&self, sheet: &str, coord: &str) -> Option<&SourceCell> {
        self.cells
            .iter()
            .find(|c| c.sheet == sheet && c.coord == coord.to_ascii_uppercase())
    }
}

/// CellData 的确定性值表示（同 repr = 同语义值）。
fn cell_repr(cd: &CellData) -> String {
    match cd {
        CellData::Text(t) => format!("t:{t}"),
        CellData::Number(v) => format!("n:{v}"),
        CellData::Boolean(b) => format!("b:{b}"),
        CellData::DateTime { serial, is_1904 } => format!("d:{serial}:{is_1904}"),
        CellData::DateTimeIso(s) => format!("di:{s}"),
        CellData::Duration { serial, is_1904 } => format!("u:{serial}:{is_1904}"),
        CellData::DurationIso(s) => format!("ui:{s}"),
        CellData::Error(e) => format!("e:{e}"),
        CellData::Empty => "empty".to_string(),
    }
}

/// 0 起列号 → A1 列字母（0→A … 16383→XFD）。
pub fn col_to_a1(col: u32) -> String {
    let mut n = col + 1;
    let mut out = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

/// 摄取 XLSX（严格读门默认——office-io 四门全开）。
pub fn ingest_xlsx(path: &Path, doc_id: &str) -> Result<XlsxSource, WorkbenchError> {
    let book: Book = read_book(&path.to_string_lossy()).map_err(WorkbenchError::XlsxRead)?;
    xlsx_from_book(&book, doc_id)
}

/// 摄取 XLSX（字节入口——p5-t3 内存会话面：与路径版共享核心，
/// 严格读门同套[read_book_from_bytes 与 read_book 同门]）。
pub fn ingest_xlsx_bytes(bytes: &[u8], doc_id: &str) -> Result<XlsxSource, WorkbenchError> {
    let book: Book = read_book_from_bytes(bytes).map_err(WorkbenchError::XlsxRead)?;
    xlsx_from_book(&book, doc_id)
}

/// Book → XlsxSource 核心提取（路径/字节两入口共享——不重复实现）。
fn xlsx_from_book(book: &Book, doc_id: &str) -> Result<XlsxSource, WorkbenchError> {
    let mut cells = vec![];
    let mut fp_input = String::new();
    for sheet in &book.sheets {
        let (start, end) = match (sheet.typed.start(), sheet.typed.end()) {
            (Some(s), Some(e)) => (s, e),
            _ => continue, // 空表
        };
        for row in start.0..=end.0 {
            for col in start.1..=end.1 {
                let cd = match sheet.typed.get_abs(row, col) {
                    Some(cd) => cd,
                    None => continue,
                };
                if matches!(cd, CellData::Empty) {
                    continue;
                }
                let repr = cell_repr(cd);
                let coord = format!("{}{}", col_to_a1(col), row + 1);
                let decimal = match cd {
                    CellData::Number(v) => Decimal::parse(&v.to_string()).ok(),
                    _ => None,
                };
                let text = match cd {
                    CellData::Text(t) => Some(t.clone()),
                    _ => None,
                };
                let formula = sheet.formula_at(row, col).map(str::to_string);
                fp_input.push_str(&sheet.name);
                fp_input.push('|');
                fp_input.push_str(&coord);
                fp_input.push('|');
                fp_input.push_str(&repr);
                fp_input.push('\n');
                cells.push(SourceCell {
                    sheet: sheet.name.clone(),
                    coord,
                    row,
                    col,
                    repr,
                    decimal,
                    text,
                    formula,
                });
            }
        }
    }
    Ok(XlsxSource {
        doc_id: doc_id.to_string(),
        cells,
        fingerprint: Fnv64.fingerprint(fp_input.as_bytes()),
    })
}

/// 由摄取产物构造来源 DocumentRef（版本由调用方管理）。
pub fn xlsx_source_ref(src: &XlsxSource, version: u64) -> DocumentRef {
    DocumentRef::new(
        DocId::new(src.doc_id.clone()),
        EngineKind::Xlsx,
        VersionStamp(version),
        src.fingerprint.clone(),
    )
}

// ---------------------------------------------------------------------------
// DOCX 正文摄取
// ---------------------------------------------------------------------------

/// 段落信息（规范化文本 + 指纹）。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphInfo {
    /// 0 起段落序（仅计段落——表格块不占号，对应 DocxParagraph
    /// 定位器口径）。
    pub index: u32,
    /// 原始拼接文本（run 直连）。
    pub text: String,
    /// 规范化文本（trim——MVP 口径，lib.rs 文档登记）。
    pub normalized: String,
    pub fingerprint: ContentFingerprint,
}

/// DOCX 正文摄取产物。
#[derive(Debug, Clone, PartialEq)]
pub struct DocxBody {
    pub paragraphs: Vec<ParagraphInfo>,
    /// 全文指纹（规范化段落文本 `\n` 连接）。
    pub fingerprint: ContentFingerprint,
}

impl DocxBody {
    pub fn paragraph(&self, index: u32) -> Option<&ParagraphInfo> {
        self.paragraphs.get(index as usize)
    }
}

/// 摄取 DOCX 正文（严格读门默认）。
pub fn ingest_docx(path: &Path) -> Result<DocxBody, WorkbenchError> {
    let doc = read_docx(&path.to_string_lossy()).map_err(WorkbenchError::DocxRead)?;
    docx_from_blocks(&doc.blocks)
}

/// 摄取 DOCX 正文（字节入口——p5-t3 内存会话面：与路径版共享核心）。
pub fn ingest_docx_bytes(bytes: &[u8]) -> Result<DocxBody, WorkbenchError> {
    let doc = read_docx_from_bytes(bytes).map_err(WorkbenchError::DocxRead)?;
    docx_from_blocks(&doc.blocks)
}

/// Blocks → DocxBody 核心提取（路径/字节两入口共享）。
fn docx_from_blocks(blocks: &[Block]) -> Result<DocxBody, WorkbenchError> {
    let mut paragraphs = vec![];
    let mut fp_input = String::new();
    let mut idx = 0u32;
    for block in blocks {
        if let Block::Paragraph(p) = block {
            let text = p.text();
            let normalized = text.trim().to_string();
            let fingerprint = Fnv64.fingerprint(normalized.as_bytes());
            fp_input.push_str(&normalized);
            fp_input.push('\n');
            paragraphs.push(ParagraphInfo {
                index: idx,
                text,
                normalized,
                fingerprint,
            });
            idx += 1;
        }
    }
    Ok(DocxBody {
        paragraphs,
        fingerprint: Fnv64.fingerprint(fp_input.as_bytes()),
    })
}

/// 由摄取产物构造正文 DocumentRef。
pub fn docx_body_ref(doc_id: &str, body: &DocxBody, version: u64) -> DocumentRef {
    DocumentRef::new(
        DocId::new(doc_id),
        EngineKind::Docx,
        VersionStamp(version),
        body.fingerprint.clone(),
    )
}
