//! 命令信封与审计事件（§四.2 `Command`/`AuditEvent`；§二.4 命令层薄）。
//!
//! 命令信封只携带**元数据**（谁/对什么/幂等键/预期版本）——业务
//! 语义由各引擎自己的操作类型承载（调用方组合），本类型不发明
//! 万能操作枚举。执行分发 = S2（写安全轮）；本轮冻结信封形状与
//! 幂等/版本语义（sidecar 内重放表先行落地）。

use serde::{Deserialize, Serialize};

use crate::refs::VersionStamp;

/// 幂等键（调用方生成，全局唯一——同键重放返回首次结果，不重复
/// 写入）。形状建议 `uuid v4` 或 `<operator>:<monotonic>`，本类型
/// 只要求非空 ≤128 字符。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(pub String);

impl IdempotencyKey {
    pub fn new(s: impl Into<String>) -> Result<Self, CommandError> {
        let s = s.into();
        if s.is_empty() || s.len() > 128 {
            return Err(CommandError::BadIdempotencyKey);
        }
        Ok(IdempotencyKey(s))
    }
}

/// 预期文档版本（乐观并发——过期操作拒绝而非覆盖，§二.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpectedVersion {
    /// 必须恰为该版本（典型 CAS）。
    Exactly(VersionStamp),
    /// 首次操作（目标尚不存在——创建面）。
    MustBeNew,
    /// 不检查（读面/显式豁免——审计仍记录）。
    Any,
}

/// 命令信封（元数据层——业务载荷由调用方定义并序列化进 `payload`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    /// 操作 ID（调用方侧唯一标识——审计关联）。
    pub op_id: String,
    /// 操作者（人类用户名 / "ai:<model>" / "system"）。
    pub operator: String,
    /// 目标资源（文档 ID / sidecar ID 等）。
    pub resource: String,
    /// 幂等键。
    pub idempotency_key: IdempotencyKey,
    /// 预期版本。
    pub expected_version: ExpectedVersion,
    /// 业务载荷（调用方域 JSON——本层不解释）。
    pub payload: serde_json::Value,
}

/// 命令结果（明确失败语义——§二.4 "明确的失败语义"）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
    /// 执行成功（载荷 = 域结果 JSON）。
    Succeeded { result: serde_json::Value },
    /// 幂等重放（键命中首次结果——非错误；审计记 replayed）。
    Replayed { first_result: serde_json::Value },
    /// 版本冲突（预期版本不符——调用方刷新后重试，绝不覆盖）。
    VersionConflict {
        expected: VersionStamp,
        actual: VersionStamp,
    },
    /// 拒绝（校验/权限/路径允许列表——携带机器可读原因码）。
    Rejected { reason: String },
    /// 执行失败（已回滚——多步写入的补偿语义由 S2 事务层保证）。
    Failed { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    BadIdempotencyKey,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid idempotency key (empty or >128 chars)")
    }
}
impl std::error::Error for CommandError {}

/// 审计事件（追加式——只增不改，序号单调）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// 全局单调序号（sidecar 内分配）。
    pub seq: u64,
    /// 时间（Unix 毫秒）。
    pub at_ms: u64,
    /// 操作者。
    pub operator: String,
    /// 目标资源。
    pub resource: String,
    /// 事件类型（登记/重验/降级/审核/命令……调用方枚举字符串）。
    pub kind: String,
    /// 事件详情 JSON。
    pub detail: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_key_domain() {
        assert!(IdempotencyKey::new("abc").is_ok());
        assert!(IdempotencyKey::new("").is_err());
        assert!(IdempotencyKey::new("x".repeat(129)).is_err());
        assert!(IdempotencyKey::new("x".repeat(128)).is_ok(), "恰 128 过");
    }

    #[test]
    fn envelope_roundtrip_with_domain_payload() {
        let env = CommandEnvelope {
            op_id: "op-1".into(),
            operator: "ai:glm".into(),
            resource: "docx:bid-2026".into(),
            idempotency_key: IdempotencyKey::new("k-1").unwrap(),
            expected_version: ExpectedVersion::Exactly(VersionStamp(3)),
            payload: serde_json::json!({"action": "bind_evidence", "evidence_id": "e1"}),
        };
        let s = serde_json::to_string(&env).unwrap();
        let back: CommandEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back, env);
    }
}
