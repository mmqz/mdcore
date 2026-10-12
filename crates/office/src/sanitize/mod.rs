//! 消毒五面（敌意输出面工具包）——各面职责表与职位见 crate 根文档。
//!
//! 五模块与 office-d §5.4 职责表逐面对应（编号一致）：
//! 1. [`csv_formula`]——CSV 出口首字符消毒（§5.1）；
//! 2. [`xml_control`]——XML 控制字符保字符档（§5.2 唯一达标档）；
//! 3. [`xml_entities`]——5 实体 + 控制字符一体化转义（§5.2 属性位
//!    兜底职责）；
//! 4. [`html_escape`]——HTML 出口转义（§5.1 裁决 3 href 面兜底）；
//! 5. [`field_whitelist`]——wordprocessing 域指令白名单（§5.3）。

pub mod csv_formula;
pub mod field_whitelist;
pub mod html_escape;
pub mod xml_control;
pub mod xml_entities;

pub use csv_formula::{guard_csv_cell, CsvFormulaMode, CsvGuardError};
pub use field_whitelist::{FieldGuard, FieldPolicy, FieldRejected};
pub use html_escape::{escape_html_attr, escape_html_text};
pub use xml_control::{escape_control_chars, unescape_control_chars};
pub use xml_entities::{
    escape_xml_attr, escape_xml_attr_full, escape_xml_text, escape_xml_text_full,
};
