//! 面 5：wordprocessing 域指令白名单（office-d §5.3）。
//!
//! **威胁面**：生态 docx 构建库的 `SimpleField` 构造把指令串**原样
//! 写入、零校验零过滤**（一手代码锚点在档）——INCLUDETEXT（外链
//! 文件注入读取）/ DDE（执行面）等危险域可由生成侧（LLM 面）注入，
//! 目标应用打开后按域语义执行。内建结构化域（TOC 开关拼接/页码
//! 固定模板）面受限，问题面在自由指令串。
//!
//! **默认口径**（office-d §5.3 建议白名单 + 黑名单首列）：
//! 白名单 = DATE / PAGE / NUMPAGES / TOC / PAGEREF / MERGEFIELD / SEQ；
//! 黑名单首列 = INCLUDETEXT / INCLUDEPICTURE（外部资源面）+
//! DDE / DDEAUTO（执行面）+ LINK / RD（安全补全：OLE 外链 /
//! 参考文档面，拒绝向扩集——过拒安全方向，误伤可由调用方调策略）。
//!
//! **全 token 扫描**：黑名单不只看首 token（域族），而是扫描指令
//! 全部 token 的**前缀匹配**——`MERGEFIELD x INCLUDETEXT "f"` 与
//! `MERGEFIELD x INCLUDETEXT"f"`（引号粘连）两种嵌套注入形态均
//! 拦截（防嵌套域注入）。
//!
//! ```
//! use mdc_office::sanitize::field_whitelist::{FieldGuard, FieldPolicy};
//!
//! let guard = FieldGuard::default();
//! assert!(guard.check("DATE \\@ \"yyyy\"").is_ok());       // 白名单 + 开关
//! assert!(guard.check("PAGEREF _Toc1 \\h").is_ok());
//! assert!(guard.check("INCLUDETEXT \"c:/x.docx\"").is_err()); // 外部资源面
//! assert!(guard.check("MERGEFIELD a INCLUDETEXT b").is_err()); // 嵌套注入
//! assert!(guard.check("DDEEXCEL|system!calc").is_err());    // 执行面
//! ```

use std::collections::BTreeSet;

/// 默认白名单（office-d §5.3 建议）。
pub const DEFAULT_ALLOW: &[&str] = &[
    "DATE",
    "PAGE",
    "NUMPAGES",
    "TOC",
    "PAGEREF",
    "MERGEFIELD",
    "SEQ",
];

/// 默认黑名单（office-d §5.3 首列 + 安全补全；拒绝向扩集）。
pub const DEFAULT_DENY: &[&str] = &[
    "INCLUDETEXT",
    "INCLUDEPICTURE",
    "DDE",
    "DDEAUTO",
    "LINK",
    "RD",
];

/// 域指令策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPolicy {
    /// 白名单制（默认）：首 token 域族必须在白名单内，且全 token
    /// 无黑名单前缀命中。
    Whitelist,
    /// 黑名单制：仅全 token 黑名单前缀拦截（域族免白名单）——
    /// 信任面较宽的调用方选用。
    Blacklist,
    /// 全拒：LLM 高敏面（完全不接受自由域指令的生成任务）。
    DenyAll,
}

/// 域指令拒绝原因（typed reject 面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldRejected {
    /// 首 token 域族不在白名单（携带域族名）。
    NotInWhitelist { family: String },
    /// 全 token 扫描命中黑名单前缀（携带命中 token 与黑名单族名）。
    DeniedFamily { token: String, family: String },
    /// 全拒策略。
    AllDenied,
}

impl std::fmt::Display for FieldRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldRejected::NotInWhitelist { family } => {
                write!(f, "域指令白名单：域族 {family:?} 不在白名单内")
            }
            FieldRejected::DeniedFamily { token, family } => {
                write!(f, "域指令黑名单：token {token:?} 命中拒绝族 {family:?}")
            }
            FieldRejected::AllDenied => write!(f, "域指令全拒策略：不接受自由域指令"),
        }
    }
}

impl std::error::Error for FieldRejected {}

/// 域指令守卫（策略 + 名单集；`check` 纯函数，零内部可变状态）。
#[derive(Clone, Debug)]
pub struct FieldGuard {
    policy: FieldPolicy,
    allow: BTreeSet<String>,
    deny: BTreeSet<String>,
}

impl Default for FieldGuard {
    /// 默认 = 白名单制 + 默认双名单（office-d §5.3 口径）。
    fn default() -> Self {
        Self::new(FieldPolicy::Whitelist)
    }
}

impl FieldGuard {
    /// 按策略构建（默认双名单）。
    pub fn new(policy: FieldPolicy) -> Self {
        Self {
            policy,
            allow: DEFAULT_ALLOW.iter().map(|s| (*s).to_string()).collect(),
            deny: DEFAULT_DENY.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// 自定义白名单（替换式）。
    pub fn with_allow(mut self, allow: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.allow = allow.into_iter().map(Into::into).collect();
        self
    }

    /// 自定义黑名单（替换式）。
    pub fn with_deny(mut self, deny: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.deny = deny.into_iter().map(Into::into).collect();
        self
    }

    /// 校验域指令串。
    ///
    /// 解析：首 token = 域族（大小写不敏感）；全 token 前缀匹配扫
    /// 黑名单（含引号粘连形态）。空白分隔 token 化；空指令 = 拒
    /// （NotInWhitelist 空族——无域语义的自由串不应进入指令位）。
    pub fn check(&self, instruction: &str) -> Result<(), FieldRejected> {
        if self.policy == FieldPolicy::DenyAll {
            return Err(FieldRejected::AllDenied);
        }
        let tokens: Vec<&str> = instruction.split_whitespace().collect();
        // 黑名单全 token 前缀扫描（两种策略下都执行——拒绝族优先）
        for tok in &tokens {
            let upper = tok.to_ascii_uppercase();
            for fam in &self.deny {
                if upper.starts_with(fam.as_str()) {
                    return Err(FieldRejected::DeniedFamily {
                        token: (*tok).to_string(),
                        family: fam.clone(),
                    });
                }
            }
        }
        if self.policy == FieldPolicy::Blacklist {
            return Ok(());
        }
        // 白名单制：首 token 域族判定
        let family = tokens
            .first()
            .map(|t| t.to_ascii_uppercase())
            .unwrap_or_default();
        if family.is_empty() || !self.allow.contains(&family) {
            return Err(FieldRejected::NotInWhitelist { family });
        }
        Ok(())
    }
}
