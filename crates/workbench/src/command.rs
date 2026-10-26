//! 命令分发（S2——§二.4 命令层薄：幂等键 + 预期版本 + 审计）。
//!
//! # 语义
//!
//! - **幂等**：同键重放返回首次结果（`CommandOutcome::Replayed`），
//!   不重复执行；重放记审计 `command_replayed`。
//! - **记录规则**：`Succeeded` / `Failed` 入幂等表（确定性结果）；
//!   `VersionConflict` / `Rejected` 不入（客户端可修正态——重试
//!   不应被旧失败毒化）。
//! - **预期版本**：`Exactly(n)` 对照 sidecar 登记正文版本
//!   （refresh_body 自增）；`MustBeNew` 要求证据与计算全空；
//!   `Any` 豁免（审计仍逐命令留痕）。
//! - **错误映射**：路径允许列表/规模限制 → `Rejected`（校验拒绝）；
//!   其余执行错误 → `Failed`（原因串）。

use std::path::PathBuf;

use mdc_evidence::{CommandEnvelope, CommandOutcome, ExpectedVersion, ReviewStatus, VersionStamp};

use crate::pipeline::{BindingProposal, CalcProposal, Workbench, WorkbenchError};

/// 八步闭环的受控命令集（无任意路径/SQL/Shell——路径仅指已注册
/// 资源或经允许列表校验的显式入参）。
#[derive(Debug, Clone)]
pub enum WorkbenchCommand {
    /// ①/⑥ 导入或刷新来源（同 doc_id = 重读重登记）。
    IngestSource { doc_id: String, path: PathBuf },
    /// ③ 关联提议。
    ProposeBinding(BindingProposal),
    /// ④ 计算提议。
    ProposeCalculation(CalcProposal),
    /// 复算（现值重提取）。
    Recalc { calc_id: String },
    /// ⑥ 正文刷新（claim 锚失效检测）。
    RefreshBody,
    /// ⑥ 来源侧重验。
    ReverifySource {
        evidence_id: String,
        doc_id: String,
        sheet: String,
        coord: String,
    },
    /// ⑥ 主张侧重验。
    ReverifyClaim {
        evidence_id: String,
        paragraph_index: u32,
    },
    /// ⑦ 人工审核。
    Review {
        evidence_id: String,
        decision: ReviewStatus,
    },
    /// ⑧ 干净导出。
    ExportClean { dest: PathBuf },
    /// ⑧-bis 暗标匿名导出（G3 / R-11：元数据级清零在原子发布边界内
    /// 完成——干净导出同门零标记验证 + docProps/批注作者面匿名化；
    /// 正文部件字节不变）。
    ExportAnonymous { dest: PathBuf },
}

impl Workbench {
    /// 受控命令执行（幂等 + 预期版本 + 审计——S2 命令层主入口）。
    pub fn execute(
        &mut self,
        envelope: CommandEnvelope,
        command: WorkbenchCommand,
        at_ms: u64,
    ) -> CommandOutcome {
        // 1. 幂等重放（键命中 = 不重复执行）。
        if let Some(first) = self.lookup_command(&envelope.idempotency_key) {
            let result = match first {
                CommandOutcome::Succeeded { result } => result,
                CommandOutcome::Replayed { first_result } => first_result,
                other => serde_json::json!({"stored": format!("{other:?}")}),
            };
            self.audit_replay(&envelope, at_ms);
            if self.persist().is_err() {
                return CommandOutcome::Failed {
                    reason: "sidecar save failed after replay audit".into(),
                };
            }
            return CommandOutcome::Replayed {
                first_result: result,
            };
        }

        // 2. 预期版本（乐观并发——过期拒绝而非覆盖）。
        let actual = self.body_version_stamp();
        match envelope.expected_version {
            ExpectedVersion::Exactly(expected) => {
                if expected != actual {
                    return CommandOutcome::VersionConflict { expected, actual };
                }
            }
            ExpectedVersion::MustBeNew => {
                if self.has_any_records() {
                    return CommandOutcome::Rejected {
                        reason: "MustBeNew violated: sidecar already carries records".into(),
                    };
                }
            }
            ExpectedVersion::Any => {}
        }

        // 3. 执行 + 结果映射（错误已在 inner 映射为 Rejected/Failed）。
        let outcome = match self.execute_inner(&envelope, command, at_ms) {
            Ok(result) => CommandOutcome::Succeeded { result },
            Err(outcome) => outcome,
        };

        // 4. 记录规则：Succeeded/Failed 入表；Rejected/VersionConflict 不入。
        let recordable = matches!(
            outcome,
            CommandOutcome::Succeeded { .. } | CommandOutcome::Failed { .. }
        );
        if recordable {
            self.store_command_outcome(&envelope, outcome.clone());
        }
        if self.persist().is_err() {
            return CommandOutcome::Failed {
                reason: "sidecar save failed".into(),
            };
        }
        outcome
    }

    fn execute_inner(
        &mut self,
        envelope: &CommandEnvelope,
        command: WorkbenchCommand,
        at_ms: u64,
    ) -> Result<serde_json::Value, CommandOutcome> {
        let op = &envelope.operator;
        match command {
            WorkbenchCommand::IngestSource { doc_id, path } => {
                let r = self
                    .ingest_source(&doc_id, &path, at_ms, op)
                    .map_err(map_error)?;
                Ok(serde_json::json!({
                    "doc_id": doc_id,
                    "version": r.version.0,
                    "fingerprint": r.fingerprint.0,
                }))
            }
            WorkbenchCommand::ProposeBinding(p) => {
                let id = self.propose_binding(p, at_ms, op).map_err(map_error)?;
                Ok(serde_json::json!({"evidence_id": id}))
            }
            WorkbenchCommand::ProposeCalculation(p) => {
                let value = self.propose_calculation(p, at_ms, op).map_err(map_error)?;
                Ok(serde_json::json!({"value": value.to_string()}))
            }
            WorkbenchCommand::Recalc { calc_id } => {
                let out = self
                    .recalc_current(&calc_id, at_ms, op)
                    .map_err(map_error)?;
                Ok(serde_json::json!({
                    "status": format!("{:?}", out.status),
                    "computed": out.computed.map(|v| v.to_string()),
                }))
            }
            WorkbenchCommand::RefreshBody => {
                let degraded = self.refresh_body(at_ms, op).map_err(map_error)?;
                Ok(serde_json::json!({"degraded": degraded}))
            }
            WorkbenchCommand::ReverifySource {
                evidence_id,
                doc_id,
                sheet,
                coord,
            } => {
                self.reverify_source(&evidence_id, &doc_id, &sheet, &coord, at_ms, op)
                    .map_err(map_error)?;
                Ok(serde_json::json!({"evidence_id": evidence_id}))
            }
            WorkbenchCommand::ReverifyClaim {
                evidence_id,
                paragraph_index,
            } => {
                self.reverify_claim(&evidence_id, paragraph_index, at_ms, op)
                    .map_err(map_error)?;
                Ok(serde_json::json!({
                    "evidence_id": evidence_id,
                    "paragraph_index": paragraph_index,
                }))
            }
            WorkbenchCommand::Review {
                evidence_id,
                decision,
            } => {
                self.review(&evidence_id, decision, at_ms, op)
                    .map_err(map_error)?;
                Ok(serde_json::json!({
                    "evidence_id": evidence_id,
                    "review": format!("{decision:?}"),
                }))
            }
            WorkbenchCommand::ExportClean { dest } => {
                self.export_clean(&dest, at_ms, op).map_err(map_error)?;
                Ok(serde_json::json!({"dest": dest.to_string_lossy()}))
            }
            WorkbenchCommand::ExportAnonymous { dest } => {
                self.export_anonymous(&dest, at_ms, op).map_err(map_error)?;
                Ok(serde_json::json!({"dest": dest.to_string_lossy()}))
            }
        }
    }
}

/// 错误映射：路径/规模/审核门槛 → Rejected（校验拒绝——客户端可
/// 修正态）；其余 → Failed。
fn map_error(e: WorkbenchError) -> CommandOutcome {
    match e {
        WorkbenchError::PathPolicy(p) => CommandOutcome::Rejected {
            reason: format!("path policy: {p}"),
        },
        WorkbenchError::Limit(r) => CommandOutcome::Rejected {
            reason: format!("limit: {r}"),
        },
        WorkbenchError::ReviewGate {
            evidence_id,
            reason,
        } => CommandOutcome::Rejected {
            reason: format!("review gate [{evidence_id}]: {reason}"),
        },
        other => CommandOutcome::Failed {
            reason: other.to_string(),
        },
    }
}

/// 当前正文版本（`Exactly` 构造辅助）。
pub fn expect_exactly(n: u64) -> ExpectedVersion {
    ExpectedVersion::Exactly(VersionStamp(n))
}
