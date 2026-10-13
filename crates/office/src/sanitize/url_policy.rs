//! 面 6：URL 策略——scheme 白名单（xlsx-r1 t10-e §6 缺口补面）。
//!
//! xlsx-r1 阶段 B 落地（2026-10-13）：分析仓 spike `src/url_policy.rs`
//! 直移（16 测试验证态同构随迁）。纯 std 零依赖，与消毒五面同构。
//!
//! **缺口背景**（t10-e 复核发现）：html_escape 承 href 注入位、
//! field_whitelist 承域外链位，但 **scheme 级策略**（`javascript:` /
//! `data:` / `vbscript:` / `file:` 协议白名单）此前无承载面——字符
//! 转义对 scheme 攻击无效（SANITIZE_TESTS.md H-03/H-04 用例实证）。
//!
//! 职责边界（与 html_escape 联防，管线顺序 = 本面先、转义后）：
//! - 本面判 **结构位**：scheme 白名单（默认 http/https/mailto +
//!   xlsx 内部链接 internal）、UNC 拒绝、NUL 拒绝、RFC 3986 非法
//!   原生字符拒绝（`<>"`——属性注入相关位）。
//! - html_escape 判 **字符位**（属性与文本转义）。
//!
//! 规范化语义（与浏览器对齐，防规范化逃逸）：剥首尾 C0 控制符与
//! 空格；移除全串 tab/LF/CR（WHATWG URL 行为）；scheme 大小写
//! 不敏感。**读侧不过滤**（t10-d mal7c 实证 NUL 可入值）——本面
//! 是 NUL 截断逃逸（U-13）的必经拦截位。

use std::collections::BTreeSet;
use std::fmt;

/// 默认 scheme 白名单：http / https / mailto + internal（xlsx 内部
/// 链接，rust_xlsxwriter write_url 语义）。
pub const DEFAULT_SCHEMES: [&str; 4] = ["http", "https", "mailto", "internal"];

/// URL 策略拒绝（typed reject，带原因）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlPolicyError {
    /// 含 NUL 字节（URL 全串非法；解析器截断差异面）。
    NullByte,
    /// scheme 不在白名单（携带实际 scheme 小写形态）。
    SchemeNotWhitelisted { scheme: String },
    /// UNC 路径（`\\` 前缀，本地网络面）。
    UncPath,
    /// RFC 3986 非法原生字符（`<` `>` `"`——须由调用方百分号编码）。
    InvalidRawChar { ch: char },
}

impl fmt::Display for UrlPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UrlPolicyError::NullByte => write!(f, "url contains NUL byte"),
            UrlPolicyError::SchemeNotWhitelisted { scheme } => {
                write!(f, "url scheme not whitelisted: {scheme}")
            }
            UrlPolicyError::UncPath => write!(f, "unc path rejected"),
            UrlPolicyError::InvalidRawChar { ch } => {
                write!(f, "invalid raw char in url (percent-encode it): {ch:?}")
            }
        }
    }
}

impl std::error::Error for UrlPolicyError {}

/// scheme 白名单策略（可配；默认 [`DEFAULT_SCHEMES`]）。
#[derive(Debug, Clone)]
pub struct UrlPolicy {
    schemes: BTreeSet<String>,
}

impl Default for UrlPolicy {
    fn default() -> Self {
        Self::new(&DEFAULT_SCHEMES)
    }
}

impl UrlPolicy {
    /// 自定义白名单（成员一律小写化；空白名单 = 全拒 scheme 型）。
    pub fn new(schemes: &[&str]) -> Self {
        Self {
            schemes: schemes.iter().map(|s| s.to_ascii_lowercase()).collect(),
        }
    }

    /// 策略检查。放行 = Ok(())；拒绝 = Err（typed，带原因）。
    pub fn check(&self, url: &str) -> Result<(), UrlPolicyError> {
        // 1. NUL：全串任何位置（截断差异面，先于一切规范化）。
        if url.contains('\0') {
            return Err(UrlPolicyError::NullByte);
        }

        // 2. RFC 3986 非法原生字符（属性注入相关位；合法 URL 中
        //    这些字符必须已百分号编码）。
        for ch in url.chars() {
            if matches!(ch, '<' | '>' | '"') {
                return Err(UrlPolicyError::InvalidRawChar { ch });
            }
        }

        // 3. 规范化（与浏览器对齐）：剥首尾 C0+空格；移除全串 \t\n\r。
        let trimmed = url.trim_matches(|c: char| c <= ' ');
        let normalized: String = trimmed
            .chars()
            .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
            .collect();

        // 4. UNC 路径（反斜杠前缀）。
        if normalized.starts_with('\\') {
            return Err(UrlPolicyError::UncPath);
        }

        // 5. scheme 判定：` ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`
        //    前缀不匹配该文法（或无冒号）→ 相对 URL，放行
        //    （协议相对 `//host/x` 也在此列——继承页面 scheme，合法）。
        if let Some(colon) = normalized.find(':') {
            let scheme_raw = &normalized[..colon];
            if is_valid_scheme(scheme_raw) {
                let scheme = scheme_raw.to_ascii_lowercase();
                if !self.schemes.contains(&scheme) {
                    return Err(UrlPolicyError::SchemeNotWhitelisted { scheme });
                }
            }
            // 文法不匹配（如 `java script:` 带内空格）→ 浏览器按相对
            // 路径处理，无 scheme 语义，放行与浏览器对齐。
        }

        Ok(())
    }
}

fn is_valid_scheme(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// 便捷入口：默认白名单检查。
pub fn check(url: &str) -> Result<(), UrlPolicyError> {
    UrlPolicy::default().check(url)
}

// ---------------------------------------------------------------------------
// 针对性安全测试（t10-e §6 全 15 载荷表逐条落地 + 边界）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(url: &str) {
        check(url).unwrap_or_else(|e| panic!("应放行 {url:?}: {e}"));
    }
    fn reject(url: &str) -> UrlPolicyError {
        match check(url) {
            Err(e) => e,
            Ok(()) => panic!("应拒绝 {url:?}"),
        }
    }

    #[test]
    fn u01_javascript_rejected() {
        assert_eq!(
            reject("javascript:alert(1)"),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "javascript".into()
            }
        );
    }

    #[test]
    fn u02_case_evasion_rejected() {
        assert_eq!(
            reject("JaVaScRiPt:alert(1)"),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "javascript".into()
            }
        );
    }

    #[test]
    fn u03_whitespace_normalization_rejected() {
        // 前导空白 + 内嵌制表符：规范化后 = javascript:
        assert_eq!(
            reject(" java\tscript:alert(1)"),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "javascript".into()
            }
        );
        // C0 控制符前缀同路径
        assert_eq!(
            reject("\u{1}javascript:alert(1)"),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "javascript".into()
            }
        );
    }

    #[test]
    fn u04_data_uri_rejected() {
        assert_eq!(
            reject("data:text/html;base64,PHNjcmlwdD4="),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "data".into()
            }
        );
    }

    #[test]
    fn u05_vbscript_rejected() {
        assert_eq!(
            reject("vbscript:msgbox(1)"),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "vbscript".into()
            }
        );
    }

    #[test]
    fn u06_file_rejected() {
        assert_eq!(
            reject("file:///etc/passwd"),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "file".into()
            }
        );
    }

    #[test]
    fn u07_unc_rejected() {
        assert_eq!(reject("\\\\evil\\share"), UrlPolicyError::UncPath);
        assert_eq!(reject("\\evil"), UrlPolicyError::UncPath);
    }

    #[test]
    fn u08_protocol_relative_allowed() {
        ok("//evil.com/x"); // 继承 scheme——合法形态，误拒即损坏合法链接
    }

    #[test]
    fn u09_https_allowed() {
        ok("https://example.com/docs?q=1");
    }

    #[test]
    fn u10_mailto_allowed() {
        ok("mailto:a@b.c");
    }

    #[test]
    fn u11_internal_link_allowed() {
        ok("internal:Target!A1"); // xlsx 内部链接（rust_xlsxwriter 语义）
    }

    #[test]
    fn u12_attribute_glue_rejected() {
        // 引号粘连（属性注入形态）→ RFC 3986 非法原生字符
        assert_eq!(
            reject("internal:Sheet1!A1\" style=\"x"),
            UrlPolicyError::InvalidRawChar { ch: '"' }
        );
        assert_eq!(
            reject("https://x/<script>"),
            UrlPolicyError::InvalidRawChar { ch: '<' }
        );
    }

    #[test]
    fn u13_nul_truncation_rejected() {
        // t10-d mal7c 联动：读侧容忍 NUL 入值 → 本面必拦
        assert_eq!(reject("java\u{0}script:alert(1)"), UrlPolicyError::NullByte);
        assert_eq!(reject("https://a\u{0}b"), UrlPolicyError::NullByte);
    }

    #[test]
    fn u14_whitelist_case_insensitive() {
        ok("HTTPS://EXAMPLE.COM"); // 大小写不误杀白名单
    }

    #[test]
    fn u15_empty_mailto_and_edges() {
        ok("mailto:"); // scheme 合法性独立于余部
        ok(""); // 空串：无 scheme 无非法字符——放行（无意义但无害）
        ok("relative/path.xlsx"); // 相对路径
        ok("java script:alert(1)"); // 内空格 → 文法不匹配 → 相对路径（浏览器同判）
        ok("1http:x"); // scheme 首字符非 ALPHA → 相对路径
    }

    #[test]
    fn custom_whitelist_and_full_reject() {
        let ftp_only = UrlPolicy::new(&["ftp"]);
        assert!(ftp_only.check("ftp://h/f").is_ok());
        assert_eq!(
            ftp_only.check("https://h/").unwrap_err(),
            UrlPolicyError::SchemeNotWhitelisted {
                scheme: "https".into()
            }
        );
        // 空白名单 = scheme 型全拒；相对路径仍放行
        let none = UrlPolicy::new(&[]);
        assert!(none.check("relative").is_ok());
        assert!(none.check("https://h/").is_err());
    }
}
