//! 面 4：HTML 出口转义（office-d §5.1 裁决 3 的 href 面兜底）。
//!
//! 生态数据骨架导出件的链接位 `<a href="...">` **未经 HTML 转义**
//! （一手代码锚点在档）——目标含引号/事件属性可直接注入 HTML
//! 属性。本模块提供文本位与属性位转义；数据骨架消费面（表格 /
//! 链接导出）在出口处调用。
//!
//! 文本位 = `&` `<` `>`；属性位 = 五实体（`&` `<` `>` `"` `'`——
//! 属性定界符无关的安全面，引号注入即被消除）。
//!
//! **边界注记**：控制字符在 HTML 文本/属性位语法上多可透传（敌意
//! 向量是属性注入而非控制字符），本模块不处理控制字符——需要时
//! 调用方先过 [`crate::sanitize::xml_control`]。
//!
//! ```
//! use mdc_office::sanitize::html_escape::{escape_html_attr, escape_html_text};
//!
//! // href 注入载荷：引号转义后属性封闭被消除
//! let evil = "\"><img src=x onerror=alert(1)>";
//! let esc = escape_html_attr(evil);
//! assert!(esc.contains("&quot;&gt;"));
//! assert!(!esc.contains("\">"));
//! assert_eq!(escape_html_text("<b>a&b</b>"), "&lt;b&gt;a&amp;b&lt;/b&gt;");
//! ```

/// HTML 文本位转义（`&` `<` `>`）。
pub fn escape_html_text(input: &str) -> String {
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

/// HTML 属性位转义（5 实体：`&` `<` `>` `"` `'`）——href 面兜底
/// 的完整形态。
pub fn escape_html_attr(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
