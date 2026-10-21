//! 文档身份契约（§四.2 `DocumentRef`）。
//!
//! 设计要点：身份 = 逻辑 ID + 引擎类型 + 版本 + 内容指纹 四元组。
//! 指纹是**内容派生**的（同内容同指纹），版本是**保存代数**（每次
//! 保存递增）——两者分离：编辑器另存不改逻辑 ID 但版本与指纹都变；
//! 内容回滚到旧态则指纹复原而版本仍前进。失效判定用指纹（内容面），
//! 审计用版本（顺序面），两者都不可省。

use serde::{Deserialize, Serialize};

/// 引擎类型（领域模型独立纪律的契约面——Locator 按此分派）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    /// Word 文档（S1 正文载体）。
    Docx,
    /// Excel 工作簿（S1 数据源载体）。
    Xlsx,
    /// PDF（后续只读来源——§四.1 明示后置）。
    #[serde(rename = "pdf")]
    Pdf,
    /// Markdown（mdcore 域——后续扩展）。
    #[serde(rename = "md")]
    Markdown,
    /// Canvas（画布域——collab-r1 后续目标③：证据系统对画布对象的
    /// 关联能力；对象身份 = 画布对象 ID，定位器见 Locator::CanvasObject）。
    #[serde(rename = "canvas")]
    Canvas,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Docx => "docx",
            EngineKind::Xlsx => "xlsx",
            EngineKind::Pdf => "pdf",
            EngineKind::Markdown => "md",
            EngineKind::Canvas => "canvas",
        }
    }
}

/// 文档逻辑 ID（调用方命名空间的稳定标识；与存储路径解耦——
/// 路径会变，ID 不变）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocId(pub String);

impl DocId {
    pub fn new(s: impl Into<String>) -> Self {
        DocId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 保存代数（单调递增；由登记方在每次保存后递增——S2 接入写命令时
/// 由命令层强制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VersionStamp(pub u64);

impl VersionStamp {
    pub const INITIAL: VersionStamp = VersionStamp(0);
    pub fn next(self) -> Self {
        VersionStamp(self.0 + 1)
    }
}

/// 内容指纹（算法前缀 + 十六进制值，如 `fnv64:9ae16a3b2f90ac64`）。
///
/// 语义 = **变更检测**（内容变 → 指纹变），非防篡改（默认 FNV-1a
/// 非密码学——`fingerprint` 模块文档的升级路径）。算法前缀使未来
/// 切换 sha256 时旧 sidecar 可辨识、可显式重验而非静默比较失败。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentFingerprint(pub String);

impl ContentFingerprint {
    /// 解析算法前缀（无前缀 = 拒绝——裸值不可辨识算法，失效判定
    /// 不可靠）。
    pub fn algorithm(&self) -> Result<&str, FingerprintFormatError> {
        let (alg, _) = self.0.split_once(':').ok_or(FingerprintFormatError)?;
        if alg.is_empty() || alg.len() > 16 {
            return Err(FingerprintFormatError);
        }
        Ok(alg)
    }
    pub fn value(&self) -> Result<&str, FingerprintFormatError> {
        let (_, v) = self.0.split_once(':').ok_or(FingerprintFormatError)?;
        if v.is_empty() {
            return Err(FingerprintFormatError);
        }
        Ok(v)
    }
}

/// 指纹格式错误（应为 algorithm:value）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FingerprintFormatError;

impl std::fmt::Display for FingerprintFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fingerprint format error (expected algorithm:value)")
    }
}
impl std::error::Error for FingerprintFormatError {}

/// 文档引用（证据记录关联正文的规范形状）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRef {
    pub doc_id: DocId,
    pub engine: EngineKind,
    pub version: VersionStamp,
    pub fingerprint: ContentFingerprint,
}

impl DocumentRef {
    pub fn new(
        doc_id: DocId,
        engine: EngineKind,
        version: VersionStamp,
        fingerprint: ContentFingerprint,
    ) -> Self {
        DocumentRef {
            doc_id,
            engine,
            version,
            fingerprint,
        }
    }

    /// 内容一致性判定（失效检测的输入）：同算法前缀才可比——跨算法
    /// 比较恒 false（调用方须重新登记指纹，而非误判"变更"）。
    pub fn content_matches(&self, other: &DocumentRef) -> bool {
        self.doc_id == other.doc_id
            && self.fingerprint.algorithm() == other.fingerprint.algorithm()
            && self.fingerprint == other.fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_requires_algorithm_prefix() {
        assert!(ContentFingerprint("9ae16a3b2f90ac64".into())
            .algorithm()
            .is_err());
        assert_eq!(
            ContentFingerprint("fnv64:9ae16".into())
                .algorithm()
                .unwrap(),
            "fnv64"
        );
        assert!(ContentFingerprint("fnv64:".into()).value().is_err());
    }

    #[test]
    fn content_matches_requires_same_algorithm() {
        let a = DocumentRef::new(
            DocId::new("bid-2026"),
            EngineKind::Docx,
            VersionStamp(3),
            ContentFingerprint("fnv64:aaaa".into()),
        );
        let b_same = DocumentRef::new(
            DocId::new("bid-2026"),
            EngineKind::Docx,
            VersionStamp(9),
            ContentFingerprint("fnv64:aaaa".into()),
        );
        let b_alg = DocumentRef::new(
            DocId::new("bid-2026"),
            EngineKind::Docx,
            VersionStamp(9),
            ContentFingerprint("sha256:aaaa".into()),
        );
        // 版本不同、指纹同 = 内容一致（版本不参与内容判定）
        assert!(a.content_matches(&b_same));
        // 算法不同 = 不可比（即使值同）→ false，触发重登记而非误判
        assert!(!a.content_matches(&b_alg));
    }
}
