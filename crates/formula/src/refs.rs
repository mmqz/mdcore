//! A1 记法辅助（公开面最小依赖——纯 std，无引擎类型）。
//!
//! 语义对齐 Excel A1：列 `A`..`XFD`（1..=16384），行 `1`..=1048576。
//! R1C1 记法不支持（xlsx 存储面恒 A1——登记限制，见 lib.rs 能力边界）。

use crate::FormulaError;

/// Excel 列上限（XFD；与 ironcalc `LAST_COLUMN` 一致——公开常量自持，
/// 不透传引擎私有常量）。
pub const MAX_COLUMN: u32 = 16_384;
/// Excel 行上限（与 ironcalc `LAST_ROW` 一致）。
pub const MAX_ROW: u32 = 1_048_576;

/// A1 文本 → `(row, col)`（均 1-based）。
///
/// 接受形态：`A1` / `$A$1` / `a1`（大小写不敏感；`$` 前缀容忍——
/// 绝对标记在引用解析面无位置语义，格子定位只看坐标）。
/// 拒绝：越界（行 > [`MAX_ROW`] 或列 > [`MAX_COLUMN`]）、空组件、
/// 非法字符、前导零（`A01` 拒——Excel 自身拒绝的形态）。
pub fn parse_a1(text: &str) -> Result<(u32, u32), FormulaError> {
    let t = text.trim();
    let core = t.strip_prefix('$').unwrap_or(t);
    if core.is_empty() {
        return Err(FormulaError::InvalidCell {
            text: text.to_string(),
        });
    }
    // 列段：字母（大小写归一），行段：数字；两段均可带 `$` 前缀。
    let bytes = core.as_bytes();
    let mut i = 0;
    let mut col: u64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        let c = bytes[i].to_ascii_uppercase();
        col = col * 26 + u64::from(c - b'A' + 1);
        i += 1;
    }
    if i == 0 {
        return Err(FormulaError::InvalidCell {
            text: text.to_string(),
        });
    }
    let rest = &core[i..];
    let row_str = rest.strip_prefix('$').unwrap_or(rest);
    if row_str.is_empty() || !row_str.bytes().all(|b| b.is_ascii_digit()) {
        return Err(FormulaError::InvalidCell {
            text: text.to_string(),
        });
    }
    if row_str.len() > 1 && row_str.starts_with('0') {
        return Err(FormulaError::InvalidCell {
            text: text.to_string(),
        });
    }
    let row: u64 = row_str.parse().map_err(|_| FormulaError::InvalidCell {
        text: text.to_string(),
    })?;
    let col_u32 = u32::try_from(col).map_err(|_| FormulaError::InvalidCell {
        text: text.to_string(),
    })?;
    if row == 0 || row > u64::from(MAX_ROW) || col == 0 || col > u64::from(MAX_COLUMN) {
        return Err(FormulaError::InvalidCell {
            text: text.to_string(),
        });
    }
    Ok((row as u32, col_u32))
}

/// `(row, col)` → A1 文本（`B3` 形态，无 `$`）。
///
/// 越界输入 → [`FormulaError::InvalidCell`]（公开面不静默钳制）。
pub fn format_a1(row: u32, col: u32) -> Result<String, FormulaError> {
    if row == 0 || row > MAX_ROW || col == 0 || col > MAX_COLUMN {
        return Err(FormulaError::InvalidCell {
            text: format!("({row},{col})"),
        });
    }
    let mut c = col;
    let mut letters = Vec::new();
    while c > 0 {
        // 1-based 26 进制：A=1..Z=26, AA=27——余 0 时借位成 Z。
        let rem = (c - 1) % 26;
        letters.push(b'A' + rem as u8);
        c = (c - 1) / 26;
    }
    letters.reverse();
    Ok(format!("{}{}", String::from_utf8_lossy(&letters), row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1_roundtrip_basic() {
        assert_eq!(parse_a1("A1").unwrap(), (1, 1));
        assert_eq!(parse_a1("B3").unwrap(), (3, 2));
        assert_eq!(parse_a1("Z1").unwrap(), (1, 26));
        assert_eq!(parse_a1("AA10").unwrap(), (10, 27));
        assert_eq!(parse_a1("XFD1048576").unwrap(), (1_048_576, 16_384));
        assert_eq!(parse_a1("a1").unwrap(), (1, 1));
        assert_eq!(parse_a1("$A$1").unwrap(), (1, 1));
        assert_eq!(parse_a1("$B$3").unwrap(), (3, 2));
        // 前导空白容忍（用户输入面）。
        assert_eq!(parse_a1(" A1 ").unwrap(), (1, 1));
    }

    #[test]
    fn a1_rejects_invalid() {
        assert!(matches!(
            parse_a1(""),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            parse_a1("A"),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            parse_a1("1A"),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            parse_a1("A0"),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            parse_a1("A01"),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            parse_a1("A-1"),
            Err(FormulaError::InvalidCell { .. })
        ));
        // 越界（行/列上限各一 + 超 26^3）。
        assert!(matches!(
            parse_a1("A1048577"),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            parse_a1("XFE1"),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            parse_a1("XFD1A"),
            Err(FormulaError::InvalidCell { .. })
        ));
    }

    #[test]
    fn a1_format_roundtrip() {
        for (r, c) in [
            (1u32, 1u32),
            (3, 2),
            (1, 26),
            (10, 27),
            (1_048_576, 16_384),
            (5, 703),
            (99, 731),
        ] {
            let text = format_a1(r, c).unwrap();
            assert_eq!(parse_a1(&text).unwrap(), (r, c), "{text} 往返失败");
        }
        assert_eq!(format_a1(1, 703).unwrap(), "AAA1");
        assert_eq!(format_a1(1, 16_384).unwrap(), "XFD1");
        assert!(matches!(
            format_a1(0, 1),
            Err(FormulaError::InvalidCell { .. })
        ));
        assert!(matches!(
            format_a1(1, 16_385),
            Err(FormulaError::InvalidCell { .. })
        ));
    }
}
