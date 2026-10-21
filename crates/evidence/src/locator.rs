//! 分引擎定位器（§四.2 `Locator`——不统一底层文档模型的纪律落点）。
//!
//! 每个变体只携带**该引擎内可稳定重寻址**的最小信息 + 定位时刻的
//! 内容指纹（供"位置还在但内容已换"的二级失效判定——结构路径存活
//! 不代表定位仍指同一内容）。
//!
//! 刻意不做的事：
//! - 不发明跨引擎统一坐标（§二.1）；
//! - 不依赖 mdcore/office-io 类型（三域零依赖）——A1 记法校验为本
//!   地实现，接入轮由调用方负责"引擎真实结构 ↔ 定位器"的互译。

use serde::{Deserialize, Serialize};

/// XLSX 单元格坐标（A1 记法，如 `C7`；列字母不区分大小写，规范化
/// 为大写存储）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellCoord(pub String);

impl CellCoord {
    /// 校验并规范化 A1 记法（1-3 列字母 + 1-7 位行号；列域上界
    /// XFD=16384、行域上界 1048576——与 mdc-office-io 读面一致）。
    pub fn parse(s: &str) -> Result<Self, LocatorError> {
        let up = s.trim().to_ascii_uppercase();
        let bytes = up.as_bytes();
        let mut i = 0usize;
        let mut col: u32 = 0;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            col = col
                .checked_mul(26)
                .and_then(|c| c.checked_add((bytes[i] - b'A' + 1) as u32))
                .ok_or(LocatorError::BadCellCoord)?;
            i += 1;
            if i > 3 {
                return Err(LocatorError::BadCellCoord);
            }
        }
        if i == 0 {
            return Err(LocatorError::BadCellCoord);
        }
        let row: u64 = up[i..]
            .parse::<u64>()
            .map_err(|_| LocatorError::BadCellCoord)
            .and_then(|r| {
                if r == 0 || r > 1_048_576 {
                    Err(LocatorError::BadCellCoord)
                } else {
                    Ok(r)
                }
            })?;
        let _ = row; // 行域校验本身即目的（值域检查）；数值供 split_coord 使用
        if col == 0 || col > 16_384 {
            return Err(LocatorError::BadCellCoord);
        }
        Ok(CellCoord(up))
    }
}

/// 定位器构造/校验错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatorError {
    BadCellCoord,
    BadRegion,
    EmptyPath,
    BadIndex,
}

impl std::fmt::Display for LocatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = match self {
            LocatorError::BadCellCoord => "invalid A1 cell coordinate",
            LocatorError::BadRegion => "invalid region (start must precede end)",
            LocatorError::EmptyPath => "empty structural path",
            LocatorError::BadIndex => "index out of domain",
        };
        f.write_str(m)
    }
}
impl std::error::Error for LocatorError {}

/// 分引擎定位器。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Locator {
    /// XLSX 单元格：工作表 + 坐标 + 定位时刻的原始值指纹 + 公式指纹
    /// （公式缓存值与文本两通道——公式变更即使缓存值相同也应失效）。
    XlsxCell {
        sheet: String,
        cell: CellCoord,
        /// 定位时刻原始值的指纹（算法前缀编码）。
        value_fingerprint: String,
        /// 定位时刻公式文本的指纹（非公式格 = None——区分"无公式"
        /// 与"公式被删除"）。
        formula_fingerprint: Option<String>,
    },
    /// XLSX 区域：工作表 + 左上/右下（闭区间；start ≤ end 端点序）。
    XlsxRegion {
        sheet: String,
        start: CellCoord,
        end: CellCoord,
        /// 区域级内容指纹（拼接序确定——左上到右右下行优先）。
        region_fingerprint: String,
    },
    /// DOCX 段落：结构路径（段落序）+ 文本指纹（正文改写检测的主
    /// 判定面）。§四.5：润色后允许失效——人工重绑定路径在状态机。
    DocxParagraph {
        /// 0 起段落序（登记时刻）。
        paragraph_index: u32,
        /// 段落规范化文本指纹。
        text_fingerprint: String,
    },
    /// DOCX 表格单元格：表序 + 行 + 列 + 文本指纹。
    DocxTableCell {
        table_index: u32,
        row: u32,
        col: u32,
        text_fingerprint: String,
    },
    /// Markdown 块（后续扩展——块 ID 寻址是 mdcore 域的既有能力）。
    MarkdownBlock {
        block_id: String,
        text_fingerprint: String,
    },
    /// PDF 页文本（后续只读来源——页号 + 页文本指纹；区域待扩展）。
    PdfPageText {
        /// 1 起页号。
        page: u32,
        text_fingerprint: String,
    },
    /// Canvas 画布对象（画布域协作引擎 mdc-canvas——对象身份 = 画布
    /// 对象 ID[Map 键，删除撤销复原后同 ID]；指纹 = 对象语义指纹
    /// [几何/属性/边端点/组成员——位置漂移即语义变更，证据失效]）。
    CanvasObject {
        object_id: String,
        object_fingerprint: String,
    },
}

impl Locator {
    /// 该定位器所属引擎（分派面）。
    pub fn engine(&self) -> crate::refs::EngineKind {
        use crate::refs::EngineKind;
        match self {
            Locator::XlsxCell { .. } | Locator::XlsxRegion { .. } => EngineKind::Xlsx,
            Locator::DocxParagraph { .. } | Locator::DocxTableCell { .. } => EngineKind::Docx,
            Locator::MarkdownBlock { .. } => EngineKind::Markdown,
            Locator::PdfPageText { .. } => EngineKind::Pdf,
            Locator::CanvasObject { .. } => EngineKind::Canvas,
        }
    }

    /// 区域端点序校验（start ≤ end，先行后列）。
    pub fn validate_region(start: &CellCoord, end: &CellCoord) -> Result<(), LocatorError> {
        let (sc, sr) = split_coord(start)?;
        let (ec, er) = split_coord(end)?;
        if er < sr || (er == sr && ec < sc) {
            return Err(LocatorError::BadRegion);
        }
        Ok(())
    }
}

fn split_coord(c: &CellCoord) -> Result<(u32, u64), LocatorError> {
    let s = &c.0;
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let col: u32 = letters_to_col(&s[..i]).ok_or(LocatorError::BadCellCoord)?;
    let row: u64 = s[i..].parse().map_err(|_| LocatorError::BadCellCoord)?;
    Ok((col, row))
}

fn letters_to_col(letters: &str) -> Option<u32> {
    let mut col = 0u32;
    for ch in letters.bytes() {
        col = col.checked_mul(26)?.checked_add((ch - b'A' + 1) as u32)?;
    }
    (col > 0 && col <= 16_384).then_some(col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_coord_domain() {
        assert!(CellCoord::parse("A1").is_ok());
        assert!(CellCoord::parse("c7").is_ok()); // 大小写规范化
        assert_eq!(CellCoord::parse("c7").unwrap().0, "C7");
        assert!(CellCoord::parse("XFD1048576").is_ok(), "域上界恰过");
        assert!(CellCoord::parse("XFE1").is_err(), "列超域拒");
        assert!(CellCoord::parse("A1048577").is_err(), "行超域拒");
        assert!(CellCoord::parse("A0").is_err(), "0 行拒");
        assert!(CellCoord::parse("1A").is_err());
        assert!(CellCoord::parse("").is_err());
        assert!(CellCoord::parse("AAAA1").is_err(), ">3 列字母拒");
    }

    #[test]
    fn region_ordering() {
        let a1 = CellCoord::parse("A1").unwrap();
        let b2 = CellCoord::parse("B2").unwrap();
        let a3 = CellCoord::parse("A3").unwrap();
        assert!(Locator::validate_region(&a1, &b2).is_ok());
        assert!(Locator::validate_region(&a1, &a3).is_ok());
        assert!(Locator::validate_region(&b2, &a1).is_err(), "逆序拒");
        assert!(Locator::validate_region(&a3, &a1).is_err());
    }

    #[test]
    fn locator_engine_dispatch() {
        let l = Locator::XlsxCell {
            sheet: "Sheet1".into(),
            cell: CellCoord::parse("C7").unwrap(),
            value_fingerprint: "fnv64:aa".into(),
            formula_fingerprint: None,
        };
        assert_eq!(l.engine(), crate::refs::EngineKind::Xlsx);
        let l2 = Locator::DocxParagraph {
            paragraph_index: 4,
            text_fingerprint: "fnv64:bb".into(),
        };
        assert_eq!(l2.engine(), crate::refs::EngineKind::Docx);
    }
}
