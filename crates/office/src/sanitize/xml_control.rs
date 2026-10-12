//! 面 2：XML 控制字符保字符档转义（office-d §5.2 唯一达标档）。
//!
//! **三档实况**（一手代码级裁决）：损坏档——非法控制字符原样透传，
//! 产物即非法 XML，目标应用直接拒开（可靠性故障面）；剥除档——剥除
//! 非法集（连带合法的 0x7F），产物合法 XML 但**静默丢字符**（保真
//! 故障面）；唯保字符档——以 `_xHHHH_` 约定转义，读写闭环。本模块
//! 实现保字符档。
//!
//! **字符集**（office-d §5.2 参照库 charegex 口径）：U+0000-U+0008 /
//! U+000B / U+000C / U+000E-U+001F（XML 1.0 非法控制字符集）+
//! U+FFFE / U+FFFF（非字符）。**放行**：\t \n \r（XML 1.0 合法）与
//! 0x7F（XML 1.0 合法——剥除档误伤面，本档不剥）。
//!
//! **字面量消歧**（本 crate 超出生态参照库的登记项——office-d §5.2
//! 裁决 3 的【推断】歧义面在此解决）：正文恰含 `_xHHHH_` 形态时，
//! 若不处置则解码会被反向展开（如正文 `_x0041_` 被解码为 `A`）。
//! 处置规则：**形态检测含逃逸边界扩展**——检测到 `_x` + 4 位十六
//! 进制 +（`_` 或转义集字符）时，前置转义其下划线（`_` → `_x005F_`）。
//! "转义集字符作闭合位"的扩展是必要的：该字符转义后的前导 `_` 会
//! 闭合一个跨逃逸边界的假形态（例：正文 `_x00AB` + U+000B 编码后
//! 为 `_x00AB_x000B_`，无扩展检测则 `_x00AB_` 成为可解码假形态）。
//! **任意输入编码-解码往返零失真**由性质测试锁死（tests/
//! sanitize_gates.rs：边界码位全扫 + 确定性随机字母表 roundtrip）。
//!
//! **解码口径**（读取遵循本约定的外部文件）：任意 `_x` + 4 位十六
//! 进制 + `_`（大小写十六进制均受）且码位为有效标量值（代理区
//! U+D800-U+DFFF 除外）即解码；非形态文本原样透传。解码不区分本
//! 编码器产物与外部产物——外部无消歧写入的字面量歧义是约定固有
//! 边界（登记，不展开）。
//!
//! ```
//! use mdc_office::sanitize::xml_control::{escape_control_chars, unescape_control_chars};
//!
//! // 非法控制字符 → _xHHHH_（产物保持合法 XML 且零字符丢失）
//! let esc = escape_control_chars("a\u{000B}b");
//! assert_eq!(esc, "a_x000B_b");
//! assert_eq!(unescape_control_chars(&esc), "a\u{000B}b");
//!
//! // 合法控制字符（\t \n \r）与 0x7F 放行
//! assert_eq!(escape_control_chars("a\tb\nc\rd\u{7F}e"), "a\tb\nc\rd\u{7F}e");
//!
//! // 字面量消歧：正文恰含形态 → 下划线前置转义，往返零失真
//! let lit = "_x0041_";
//! let esc2 = escape_control_chars(lit);
//! assert_eq!(esc2, "_x005F_x0041_");
//! assert_eq!(unescape_control_chars(&esc2), "_x0041_");
//! ```

/// 判定码位是否属于转义集（XML 1.0 非法控制字符 + 非字符 U+FFFE/U+FFFF）。
///
/// 放行 \t (U+0009) / \n (U+000A) / \r (U+000D) / 0x7F——XML 1.0 合法
/// （参照库 charegex 同口径；剥除档对 0x7F 的连带剥除是登记误伤面）。
pub fn is_escaped_code_point(c: char) -> bool {
    matches!(c, '\u{0000}'..='\u{0008}' | '\u{000B}' | '\u{000C}' | '\u{000E}'..='\u{001F}')
        || matches!(c, '\u{FFFE}' | '\u{FFFF}')
}

/// 检测原文本从字节偏移 `i` 起是否构成**待消歧形态**：
/// `_` `x` + 恰 4 位 ASCII 十六进制 + 闭合位，闭合位 ∈ {`_`（真形态）,
/// 转义集字符（逃逸边界扩展——该字符转义后的前导 `_` 会闭合假形态，
/// 含多字节成员 U+FFFE/U+FFFF）}。
///
/// 前置条件：`i` 是字符边界且 `s[i] == '_'`。返回 true 时调用方应把
/// 当前 `_` 前置转义为 `_x005F_` 并仅消费该下划线（后续 `x`/十六进制
/// 位/闭合位按常规扫描继续——闭合位为转义集字符时到达即正常转义；
/// 闭合位为 `_` 时其自身再做形态检测，链式形态逐个消歧）。
fn is_ambiguous_form_at(s: &str, i: usize) -> bool {
    let b = s.as_bytes();
    // 形态全长 7 字节：下标 i..=i+6 全部在位
    if i + 6 >= b.len() {
        return false;
    }
    if b[i + 1] != b'x' {
        return false;
    }
    for k in 2..=5 {
        if !b[i + k].is_ascii_hexdigit() {
            return false;
        }
    }
    // i+6 必为字符边界（前 6 字节均为 ASCII 单字节字符）
    let closer = s[i + 6..].chars().next().expect("i+6 为字符边界");
    closer == '_' || is_escaped_code_point(closer)
}

/// 保字符档转义：非法控制字符与非字符以 `_xHHHH_` 约定转义（大写
/// 十六进制），字面量形态按下划线前置转义消歧。
pub fn escape_control_chars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = input[i..].chars().next().expect("i 在字符边界上");
        if c == '_' && is_ambiguous_form_at(input, i) {
            out.push_str("_x005F_");
            i += 1; // 仅消费下划线；x/十六进制位/闭合位常规扫描
            continue;
        }
        if is_escaped_code_point(c) {
            out.push_str(&format!("_x{:04X}_", u32::from(c)));
        } else {
            out.push(c);
        }
        i += c.len_utf8();
    }
    out
}

/// 按约定解码 `_xHHHH_` 形态（大小写十六进制均受；代理区码位与
/// 非形态文本原样透传）。
pub fn unescape_control_chars(input: &str) -> String {
    let b = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'_' && i + 6 < b.len() && b[i + 1] == b'x' && b[i + 6] == b'_' {
            // 先按字节验证 4 位 ASCII 十六进制（同时保证切片边界安全）
            let hex_ok = (2..=5).all(|k| b[i + k].is_ascii_hexdigit());
            if hex_ok {
                let cp = u32::from_str_radix(&input[i + 2..i + 6], 16).expect("已验证十六进制");
                if let Some(ch) = char::from_u32(cp) {
                    // 代理区（U+D800-U+DFFF）返回 None → 原样透传
                    out.push(ch);
                    i += 7;
                    continue;
                }
            }
        }
        let c = input[i..].chars().next().expect("i 在字符边界上");
        out.push(c);
        i += c.len_utf8();
    }
    out
}
