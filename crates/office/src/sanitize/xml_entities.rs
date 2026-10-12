//! 面 3：XML 实体转义（office-d §5.2 属性位兜底职责）。
//!
//! 生态写库的实体面基本达标（5 实体），但存在**属性位缺口**（个别
//! 选项位未过转义器——属性注入缺口）与**控制字符三档缺位**（见
//! [`crate::sanitize::xml_control`]）。本模块提供实体转义与
//! 实体 + 控制字符**一体化**变体（`*_full`）——属性位兜底应使用
//! `escape_xml_attr_full`（实体 + 保字符档一次到位，喂入前职位）。
//!
//! 文本位最小合法集 = `&` `<` `>`（`>` 转义非 XML 1.0 强制但消除
//! `]]>` 序列面，惯例全转）；属性位全集 = 五实体（双引号属性定界
//! 下 `'` 非必需，但属性定界符策略属调用方，五实体全转是定界符
//! 无关的安全面）。
//!
//! ```
//! use mdc_office::sanitize::xml_entities::{escape_xml_attr_full, escape_xml_text};
//!
//! assert_eq!(escape_xml_text("a<b & c>d"), "a&lt;b &amp; c&gt;d");
//! assert_eq!(escape_xml_attr_full("q\"v\" & <tag>"), "q&quot;v&quot; &amp; &lt;tag&gt;");
//! // 控制字符一体化（保字符档，无静默丢失）
//! assert_eq!(escape_xml_attr_full("k\u{000B}v"), "k_x000B_v");
//! ```

use super::xml_control::escape_control_chars;

/// XML 文本位实体转义（`&` `<` `>`；控制字符不处理——文本位控制
/// 字符是 [`crate::sanitize::xml_control`] 的职责）。
pub fn escape_xml_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// XML 属性位实体转义（5 实体：`&` `<` `>` `"` `'`）。
pub fn escape_xml_attr(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// 文本位一体化（实体 + 保字符档控制字符转义）。
///
/// 顺序：先控制字符后实体——`_xHHHH_` 产物不含实体字符，两序皆可；
/// 先控制字符保持与"喂入前字符过滤"职位的直觉一致。
pub fn escape_xml_text_full(input: &str) -> String {
    escape_xml_text(&escape_control_chars(input))
}

/// 属性位一体化（5 实体 + 保字符档控制字符转义）——**属性位兜底
/// 的完整形态**（office-d §5.2 属性缺口的对策面）。
pub fn escape_xml_attr_full(input: &str) -> String {
    escape_xml_attr(&escape_control_chars(input))
}
