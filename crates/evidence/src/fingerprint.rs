//! 指纹算法契约 + 默认实现。
//!
//! # 算法前缀纪律
//!
//! 所有指纹字符串形如 `algorithm:value`（十六进制）。默认实现
//! FNV-1a 64：
//! - **用途边界**：变更检测（内容变 → 指纹变）。64 位空间在 MVP
//!   规模（千级记录）下意外碰撞概率可忽略（生日界 ~10^-11）。
//! - **非防篡改**：FNV 非密码学哈希——审计防篡改不是它的职责；
//!   sha256 升级路径：新实现 `Fingerprinter`（如 `Sha256`）产出
//!   `sha256:` 前缀指纹，旧记录经算法前缀辨识后走显式重登记，
//!   `DocumentRef::content_matches` 跨算法恒 false 保证不会把
//!   "不可比"误判成"一致"。
//! - S2（写安全轮）评估切换时机；切换属于 sidecar 模式版本内的
//!   兼容面（读旧写新，重验不静默）。

use std::fmt::Write as _;

use crate::refs::ContentFingerprint;

/// 指纹产出能力（依赖倒置：真实文档指纹由调用方在应用层用
/// mdc-office-io 解析结果计算——本 crate 不依赖引擎）。
pub trait Fingerprinter {
    /// 对任意字节序列产出带算法前缀的指纹。
    fn fingerprint(&self, content: &[u8]) -> ContentFingerprint;
    /// 算法名（前缀）。
    fn algorithm(&self) -> &'static str;
}

/// FNV-1a 64（默认；非密码学——见模块文档）。
#[derive(Debug, Clone, Copy, Default)]
pub struct Fnv64;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl Fnv64 {
    pub fn hash_bytes(bytes: &[u8]) -> u64 {
        let mut h = FNV_OFFSET;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
}

impl Fingerprinter for Fnv64 {
    fn fingerprint(&self, content: &[u8]) -> ContentFingerprint {
        let mut s = String::with_capacity(6 + 1 + 16);
        let _ = write!(s, "fnv64:{:016x}", Self::hash_bytes(content));
        ContentFingerprint(s)
    }
    fn algorithm(&self) -> &'static str {
        "fnv64"
    }
}

/// 测试/调用方便捷构造（非实现 trait——直接包装已有指纹串）。
pub fn fingerprint_of(algorithm: &str, hex_value: &str) -> ContentFingerprint {
    ContentFingerprint(format!("{algorithm}:{hex_value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv64_deterministic_and_sensitive() {
        let f = Fnv64;
        let a = f.fingerprint(b"hello world");
        let b = f.fingerprint(b"hello world");
        let c = f.fingerprint(b"hello world!");
        assert_eq!(a, b, "同内容同指纹（确定性）");
        assert_ne!(a, c, "一字之差指纹必变");
        assert_eq!(a.algorithm().unwrap(), "fnv64");
        assert_eq!(a.value().unwrap().len(), 16, "64 位 = 16 个十六进制字符");
    }

    #[test]
    fn known_vector() {
        // FNV-1a 64 公开测试向量（空串与 "a"）——防实现漂移。
        assert_eq!(Fnv64::hash_bytes(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(Fnv64::hash_bytes(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
