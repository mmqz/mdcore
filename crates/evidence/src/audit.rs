//! 审计日志（追加式序号分配器——事件类型见 `command::AuditEvent`）。

use crate::command::AuditEvent;

/// 内存序号分配器（sidecar 持久层为唯一真源——本类型只做构造期
/// 编号，加载后从 sidecar 末序号续）。
#[derive(Debug, Clone, Copy, Default)]
pub struct AuditSeq {
    last: u64,
}

impl AuditSeq {
    pub fn continuing_after(last: u64) -> Self {
        AuditSeq { last }
    }
    pub fn allocate(&mut self) -> u64 {
        self.last += 1;
        self.last
    }
    pub fn last(&self) -> u64 {
        self.last
    }
}

/// 便捷构造（序号由调用方分配）。
pub fn event(
    seq: u64,
    at_ms: u64,
    operator: &str,
    resource: &str,
    kind: &str,
    detail: serde_json::Value,
) -> AuditEvent {
    AuditEvent {
        seq,
        at_ms,
        operator: operator.into(),
        resource: resource.into(),
        kind: kind.into(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_monotonic_and_continues() {
        let mut s = AuditSeq::continuing_after(41);
        assert_eq!(s.allocate(), 42);
        assert_eq!(s.allocate(), 43);
        assert_eq!(s.last(), 43);
    }
}
