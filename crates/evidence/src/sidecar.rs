//! Sidecar 存储 v1（§四.1/§四.5——本地 JSON、原子写、模式版本、
//! 幂等重放表、显式重验唯一恢复通道）。
//!
//! # 安全语义（本模块是 §四.5 铁律的执行点）
//!
//! - **失效**：`refresh_document` 传入文档新引用——指纹不匹配的
//!   关联证据**只降不升**（Valid → Stale），绝不静默重绑。
//! - **恢复**：`reverify_evidence` 是唯一恢复通道（显式新定位器 +
//!   操作者 + 审计事件）——绕过它没有任何 API 能把 Stale 变回
//!   Valid（`EvidenceRecord::degrade_linkage` 单向 + 本函数走
//!   直赋前必经审计）。
//! - **原子写**：临时文件 + rename——失败不留半成品（§九.11
//!   "不留下正文和 sidecar 不匹配的已发布结果"的存储面）。
//! - **敏感面**：本模块零网络零上传（结构性保证——§二.3）。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::{AuditEvent, CommandOutcome, IdempotencyKey};
use crate::evidence::{CalculationRecord, EvidenceRecord};
use crate::expr::{self, CurrentInput, EvalOutcome, RecalcOutcome};
use crate::locator::Locator;
use crate::refs::DocumentRef;
use crate::status::{CalcStatus, LinkageStatus};

/// 当前模式版本（破坏性变更 = 递增 + 迁移函数——读旧拒新是错误，
/// 正确形状 = load 时按版本分派迁移）。
pub const SCHEMA_VERSION: u32 = 1;

/// 幂等重放表项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyEntry {
    pub key: IdempotencyKey,
    pub op_id: String,
    pub outcome: CommandOutcome,
}

/// Sidecar 数据体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub schema_version: u32,
    /// 本 sidecar 所属正文文档（登记时引用——refresh 时更新）。
    pub document: DocumentRef,
    /// 已登记的来源文档引用表（doc_id → 登记时引用；失效检测的
    /// 对照真源）。
    pub source_docs: Vec<DocumentRef>,
    pub evidence: Vec<EvidenceRecord>,
    pub calculations: Vec<CalculationRecord>,
    pub idempotency: Vec<IdempotencyEntry>,
    pub audit: Vec<AuditEvent>,
}

impl Sidecar {
    pub fn new(document: DocumentRef) -> Self {
        Sidecar {
            schema_version: SCHEMA_VERSION,
            document,
            source_docs: vec![],
            evidence: vec![],
            calculations: vec![],
            idempotency: vec![],
            audit: vec![],
        }
    }

    /// 会话级审计事件直录（编排层用——资源名恒为本 sidecar 所属文档）。
    pub fn push_audit_public(
        &mut self,
        kind: &str,
        detail: serde_json::Value,
        at_ms: u64,
        operator: &str,
    ) {
        self.push_audit(at_ms, operator, kind, detail);
    }

    fn next_seq(&mut self) -> u64 {
        self.audit.last().map(|e| e.seq).unwrap_or(0) + 1
    }

    fn push_audit(&mut self, at_ms: u64, operator: &str, kind: &str, detail: serde_json::Value) {
        let seq = self.next_seq();
        self.audit.push(AuditEvent {
            seq,
            at_ms,
            operator: operator.into(),
            resource: self.document.doc_id.as_str().to_string(),
            kind: kind.into(),
            detail,
        });
    }

    /// 登记来源文档（幂等：同 doc_id 重复登记 = 更新引用 + 审计）。
    pub fn register_source_doc(&mut self, doc: DocumentRef, at_ms: u64, operator: &str) {
        let existed = self
            .source_docs
            .iter()
            .position(|d| d.doc_id == doc.doc_id && d.engine == doc.engine);
        match existed {
            Some(i) => {
                let changed = self.source_docs[i].fingerprint != doc.fingerprint;
                self.source_docs[i] = doc.clone();
                self.push_audit(
                    at_ms,
                    operator,
                    "source_doc_refresh",
                    serde_json::json!({"doc_id": doc.doc_id.as_str(), "changed": changed}),
                );
                // 引用更新后立即执行失效检测（登记与检测不分离——
                // 不存在"已换引用但证据还没降级"的中间可见态）。
                if changed {
                    self.apply_linkage_invalidation(&doc, at_ms, operator);
                }
            }
            None => {
                self.source_docs.push(doc.clone());
                self.push_audit(
                    at_ms,
                    operator,
                    "source_doc_register",
                    serde_json::json!({"doc_id": doc.doc_id.as_str()}),
                );
            }
        }
    }

    /// 失效检测：给定（新）来源文档引用，对关联该文档的证据执行
    /// 指纹对照——不匹配 → Stale。**只降不升**（degrade_linkage 语义）。
    fn apply_linkage_invalidation(&mut self, doc: &DocumentRef, at_ms: u64, operator: &str) {
        let mut degraded: Vec<String> = vec![];
        for ev in self.evidence.iter_mut() {
            if ev.source_doc.doc_id == doc.doc_id
                && ev.source_doc.fingerprint != doc.fingerprint
                && ev.degrade_linkage(LinkageStatus::Stale)
            {
                degraded.push(ev.evidence_id.clone());
            }
        }
        if !degraded.is_empty() {
            self.push_audit(
                at_ms,
                operator,
                "linkage_degraded",
                serde_json::json!({"doc_id": doc.doc_id.as_str(), "evidence": degraded}),
            );
        }
    }

    /// 外部驱动的失效（来源文档打不开/定位歧义——调用方观测后
    /// 显式登记；审计同链）。语义域 = Missing/Ambiguous；传入其他
    /// 状态（如 Stale）经 degrade_linkage 单向规则自然 no-op——
    /// 失效态互转不经重验一律拒绝，不靠断言靠状态机。
    pub fn mark_source_unavailable(
        &mut self,
        doc_id: &str,
        status: LinkageStatus,
        at_ms: u64,
        operator: &str,
    ) {
        let mut degraded: Vec<String> = vec![];
        for ev in self.evidence.iter_mut() {
            if ev.source_doc.doc_id.as_str() == doc_id && ev.degrade_linkage(status) {
                degraded.push(ev.evidence_id.clone());
            }
        }
        if !degraded.is_empty() {
            self.push_audit(
                at_ms,
                operator,
                "linkage_degraded",
                serde_json::json!({"doc_id": doc_id, "status": format!("{status:?}"), "evidence": degraded}),
            );
        }
    }

    /// 登记证据（查重 → 校验 → 入列 → 审计）。重复 ID =
    /// `SidecarError::DuplicateEvidenceId`（更新走显式重验/审核通道，
    /// 不静默覆写）。
    pub fn insert_evidence(
        &mut self,
        record: EvidenceRecord,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), SidecarError> {
        if self
            .evidence
            .iter()
            .any(|e| e.evidence_id == record.evidence_id)
        {
            return Err(SidecarError::DuplicateEvidenceId(
                record.evidence_id.clone(),
            ));
        }
        record
            .validate()
            .map_err(|e| SidecarError::Corrupt(e.to_string()))?;
        let id = record.evidence_id.clone();
        self.evidence.push(record);
        self.push_audit(
            at_ms,
            operator,
            "evidence_add",
            serde_json::json!({"evidence_id": id}),
        );
        Ok(())
    }

    /// 登记计算记录（同 ID 拒绝）。
    pub fn add_calculation(&mut self, rec: CalculationRecord, at_ms: u64, operator: &str) -> bool {
        if self.calculations.iter().any(|c| c.calc_id == rec.calc_id) {
            return false;
        }
        let id = rec.calc_id.clone();
        self.calculations.push(rec);
        self.push_audit(
            at_ms,
            operator,
            "calculation_add",
            serde_json::json!({"calc_id": id}),
        );
        true
    }

    /// 首次求值落值（s1-r2）：`evaluate` 产物写回——final_value +
    /// intermediates + calc_status=Passed（值刚由求值器产出，链路自洽）。
    /// 已含登记值的记录拒绝（不静默覆写——更新走 `recalculate_calc`
    /// 或显式重登记）。
    pub fn apply_evaluation(
        &mut self,
        calc_id: &str,
        outcome: EvalOutcome,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), SidecarError> {
        let rec = self
            .calculations
            .iter_mut()
            .find(|c| c.calc_id == calc_id)
            .ok_or_else(|| SidecarError::CalculationNotFound(calc_id.to_string()))?;
        if rec.final_value.is_some() {
            return Err(SidecarError::AlreadyEvaluated(calc_id.to_string()));
        }
        rec.final_value = Some(outcome.value);
        rec.intermediates = outcome.intermediates;
        rec.calc_status = CalcStatus::Passed;
        self.push_audit(
            at_ms,
            operator,
            "calc_evaluated",
            serde_json::json!({
                "calc_id": calc_id,
                "value": outcome.value.to_string(),
                "unit": outcome.computed_unit,
            }),
        );
        Ok(())
    }

    /// 复算 + 级联（s1-r2——§四.4 复算闭环的执行点）：
    ///
    /// 1. `expr::recalc` 四步链（可用性/现值对照/求值/登记值对照）
    ///    → 更新 `CalculationRecord.calc_status`；
    /// 2. 对引用该计算的每条证据：失败**级联同因**（级联诚实性）；
    ///    通过时对照 `evidence.claimed_value`（第五步：正文主张值
    ///    对照）→ Passed / Failed(ResultMismatch)。
    ///
    /// 输入现值由调用方从真实来源提取（依赖倒置：本层不做引擎 IO）。
    pub fn recalculate_calc(
        &mut self,
        calc_id: &str,
        current: &[CurrentInput],
        at_ms: u64,
        operator: &str,
    ) -> Result<RecalcOutcome, SidecarError> {
        let rec = self
            .calculations
            .iter()
            .find(|c| c.calc_id == calc_id)
            .ok_or_else(|| SidecarError::CalculationNotFound(calc_id.to_string()))?;
        let outcome = expr::recalc(rec, current);
        let rec = self
            .calculations
            .iter_mut()
            .find(|c| c.calc_id == calc_id)
            .expect("上方已验证存在");
        rec.calc_status = outcome.status;
        // 级联：引用该计算的证据逐条同步 calc 面。
        // 级联诚实性门槛（status.rs）：linkage 非 Valid 的证据不经
        // 复算直接 Failed(InputUnavailable)——来源不实不许复算通过。
        let mut cascaded = vec![];
        for ev in self.evidence.iter_mut() {
            if ev.calc_ref.as_deref() != Some(calc_id) {
                continue;
            }
            if !ev.linkage.allows_recalc() {
                ev.calc = CalcStatus::Failed(crate::status::CalcFailure::InputUnavailable);
                cascaded.push(ev.evidence_id.clone());
                continue;
            }
            ev.calc = match outcome.status {
                CalcStatus::Failed(f) => CalcStatus::Failed(f),
                CalcStatus::NotApplicable => CalcStatus::NotApplicable,
                CalcStatus::Passed => {
                    let computed = outcome.computed.expect("Passed 必携带计算值");
                    match &ev.claimed_value {
                        Some(claimed) => {
                            if claimed.numeric_eq(computed).unwrap_or(false) {
                                CalcStatus::Passed
                            } else {
                                CalcStatus::Failed(crate::status::CalcFailure::ResultMismatch)
                            }
                        }
                        // 无可比对数值不可通过（诚实纪律；新数据经
                        // validate 已排除，此为旧数据防御面）
                        None => CalcStatus::Failed(crate::status::CalcFailure::ResultMismatch),
                    }
                }
            };
            cascaded.push(ev.evidence_id.clone());
        }
        self.push_audit(
            at_ms,
            operator,
            "calc_recalc",
            serde_json::json!({
                "calc_id": calc_id,
                "status": format!("{:?}", outcome.status),
                "cascaded_evidence": cascaded,
            }),
        );
        Ok(outcome)
    }

    /// 人工审核动作（s1-r3——§四.3 待人工面的归宿）：
    ///
    /// 允许：Pending → Approved/Rejected；Approved ↔ Rejected
    /// （人工改判——审核是人的判断，与机器事实不同，可被新的判断
    /// 取代，但每次都留审计）。拒绝：任何 → Pending（人工决策不
    /// 静默复位——复位 = 撤销决策，应经审计的新决策显式表达）。
    pub fn review_evidence(
        &mut self,
        evidence_id: &str,
        to: crate::status::ReviewStatus,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), SidecarError> {
        use crate::status::ReviewStatus;
        let ev = self
            .evidence
            .iter_mut()
            .find(|e| e.evidence_id == evidence_id)
            .ok_or(SidecarError::EvidenceNotFound)?;
        let from = ev.review;
        let allowed = matches!(
            (from, to),
            (ReviewStatus::Pending, ReviewStatus::Approved)
                | (ReviewStatus::Pending, ReviewStatus::Rejected)
                | (ReviewStatus::Approved, ReviewStatus::Rejected)
                | (ReviewStatus::Rejected, ReviewStatus::Approved)
        );
        if !allowed {
            return Err(SidecarError::InvalidReviewTransition { from, to });
        }
        ev.review = to;
        self.push_audit(
            at_ms,
            operator,
            "evidence_review",
            serde_json::json!({
                "evidence_id": evidence_id,
                "from": format!("{from:?}"),
                "to": format!("{to:?}"),
            }),
        );
        Ok(())
    }

    /// 正文刷新（s1-r3——§四.5 正文变更失效判定的执行点）：新正文
    /// 引用 + 新段落指纹表（索引 → 规范化文本指纹）→ **claim 锚断**
    /// 的证据 linkage → Stale（只降不升；LinkageStatus 语义 = 锚定
    /// 链状态，claim 锚与 source 锚共享同一状态面，审计 detail
    /// 区分 side）。
    ///
    /// 仅处理 `DocxParagraph` 型 claim 定位器（表格/其他引擎定位器
    /// 不猜——后续轮扩）；正文指纹未变 = 仅更新引用零降级。返回
    /// 降级条数。
    pub fn refresh_document(
        &mut self,
        new_doc: DocumentRef,
        paragraph_fps: &[String],
        at_ms: u64,
        operator: &str,
    ) -> usize {
        let body_changed = self.document.fingerprint != new_doc.fingerprint;
        let mut degraded = vec![];
        if body_changed {
            for ev in self.evidence.iter_mut() {
                let anchor_broken = match &ev.claim_locator {
                    Locator::DocxParagraph {
                        paragraph_index,
                        text_fingerprint,
                    } => {
                        paragraph_fps
                            .get(*paragraph_index as usize)
                            .map(|fp| fp != text_fingerprint)
                            .unwrap_or(true) // 索引越界 = 段落已删
                    }
                    _ => continue, // 非 DOCX 段落主张：不猜，后续轮
                };
                if anchor_broken && ev.degrade_linkage(LinkageStatus::Stale) {
                    degraded.push(ev.evidence_id.clone());
                }
            }
        }
        self.document = new_doc;
        if body_changed {
            self.push_audit(
                at_ms,
                operator,
                "body_refresh",
                serde_json::json!({
                    "side": "claim",
                    "degraded": degraded.len(),
                    "evidence": degraded,
                }),
            );
        }
        degraded.len()
    }

    /// 主张侧重验（s1-r3——claim 锚唯一恢复通道，与
    /// [`Sidecar::reverify_evidence`]（source 侧）对称）：新段落位置 +
    /// 文本指纹 + 新正文引用 → linkage 置 Valid + 审计。
    pub fn reverify_claim(
        &mut self,
        evidence_id: &str,
        new_claim_doc: DocumentRef,
        paragraph_index: u32,
        text_fingerprint: String,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), SidecarError> {
        if new_claim_doc.engine != crate::refs::EngineKind::Docx {
            return Err(SidecarError::EngineMismatch);
        }
        let ev = self
            .evidence
            .iter_mut()
            .find(|e| e.evidence_id == evidence_id)
            .ok_or(SidecarError::EvidenceNotFound)?;
        ev.claim_doc = new_claim_doc.clone();
        ev.claim_locator = Locator::DocxParagraph {
            paragraph_index,
            text_fingerprint,
        };
        ev.linkage = LinkageStatus::Valid;
        self.document = new_claim_doc;
        self.push_audit(
            at_ms,
            operator,
            "claim_reverified",
            serde_json::json!({
                "evidence_id": evidence_id,
                "paragraph_index": paragraph_index,
            }),
        );
        Ok(())
    }

    /// **显式重验**（唯一恢复通道）：新定位器 + 新文档引用 + 操作者
    /// ——审计记录后 linkage 置 Valid。定位器与文档引擎一致性校验。
    pub fn reverify_evidence(
        &mut self,
        evidence_id: &str,
        new_source_doc: DocumentRef,
        new_source_locator: Locator,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), SidecarError> {
        let ev = self
            .evidence
            .iter_mut()
            .find(|e| e.evidence_id == evidence_id)
            .ok_or(SidecarError::EvidenceNotFound)?;
        if new_source_locator.engine() != new_source_doc.engine {
            return Err(SidecarError::EngineMismatch);
        }
        ev.source_doc = new_source_doc.clone();
        ev.source_locator = new_source_locator;
        ev.linkage = LinkageStatus::Valid;
        // 登记新来源引用（若新 doc 不在表内）
        if !self
            .source_docs
            .iter()
            .any(|d| d.doc_id == new_source_doc.doc_id && d.engine == new_source_doc.engine)
        {
            self.source_docs.push(new_source_doc.clone());
        }
        self.push_audit(
            at_ms,
            operator,
            "linkage_reverified",
            serde_json::json!({"evidence_id": evidence_id, "doc_id": new_source_doc.doc_id.as_str()}),
        );
        Ok(())
    }

    /// 幂等登记：键已存在 → 返回首次结果（Replayed）；否则记录。
    pub fn record_command(
        &mut self,
        key: IdempotencyKey,
        op_id: &str,
        outcome: CommandOutcome,
    ) -> Option<CommandOutcome> {
        if let Some(first) = self
            .idempotency
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.outcome.clone())
        {
            return Some(first);
        }
        self.idempotency.push(IdempotencyEntry {
            key,
            op_id: op_id.into(),
            outcome,
        });
        None
    }
}

/// 存储错误。
#[derive(Debug)]
pub enum SidecarError {
    Io(io::Error),
    Serialize(serde_json::Error),
    /// 模式版本不支持（携带读到/支持值——显式拒绝不猜测）。
    SchemaUnsupported {
        found: u32,
        supported: u32,
    },
    /// JSON 损坏（不完整/非预期形状——拒绝加载，保留原文件待人工）。
    Corrupt(String),
    EvidenceNotFound,
    EngineMismatch,
    DuplicateEvidenceId(String),
    /// 计算记录不存在（s1-r2）。
    CalculationNotFound(String),
    /// 记录已含登记值——拒绝覆写（s1-r2 不静默覆写纪律）。
    AlreadyEvaluated(String),
    /// 审核状态迁移非法（s1-r3——如人工决策回 Pending 复位）。
    InvalidReviewTransition {
        from: crate::status::ReviewStatus,
        to: crate::status::ReviewStatus,
    },
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SidecarError::Io(e) => write!(f, "sidecar io error: {e}"),
            SidecarError::Serialize(e) => write!(f, "sidecar serialize error: {e}"),
            SidecarError::SchemaUnsupported { found, supported } => write!(
                f,
                "sidecar schema v{found} unsupported (this build supports v{supported})"
            ),
            SidecarError::Corrupt(m) => write!(f, "sidecar corrupt: {m}"),
            SidecarError::EvidenceNotFound => f.write_str("evidence record not found"),
            SidecarError::EngineMismatch => f.write_str("locator/document engine mismatch"),
            SidecarError::DuplicateEvidenceId(id) => {
                write!(f, "duplicate evidence id: {id}")
            }
            SidecarError::CalculationNotFound(id) => {
                write!(f, "calculation record not found: {id}")
            }
            SidecarError::AlreadyEvaluated(id) => {
                write!(f, "calculation already evaluated (refuse overwrite): {id}")
            }
            SidecarError::InvalidReviewTransition { from, to } => {
                write!(f, "invalid review transition: {from:?} -> {to:?}")
            }
        }
    }
}
impl std::error::Error for SidecarError {}

impl From<io::Error> for SidecarError {
    fn from(e: io::Error) -> Self {
        SidecarError::Io(e)
    }
}
impl From<serde_json::Error> for SidecarError {
    fn from(e: serde_json::Error) -> Self {
        SidecarError::Serialize(e)
    }
}

/// 文件存储（一个 sidecar 一个文件；路径由调用方管理——允许列表
/// 校验在 S2 命令层，本层不做路径决策）。
#[derive(Debug, Clone)]
pub struct SidecarStore {
    path: PathBuf,
}

impl SidecarStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        SidecarStore {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// 加载（不存在 → None；存在 → 版本校验 + 解析）。
    pub fn load(&self) -> Result<Option<Sidecar>, SidecarError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.path)?;
        let parsed: Sidecar = serde_json::from_str(&raw).map_err(|e| {
            SidecarError::Corrupt(format!("{:?} at line {}", e.classify(), e.line()))
        })?;
        if parsed.schema_version != SCHEMA_VERSION {
            return Err(SidecarError::SchemaUnsupported {
                found: parsed.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        Ok(Some(parsed))
    }

    /// 原子保存（同目录临时文件 + rename——同文件系统保证原子性；
    /// 写失败时原文件不动）。
    pub fn save(&self, sidecar: &Sidecar) -> Result<(), SidecarError> {
        let json = serde_json::to_string_pretty(sidecar)?;
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, json.as_bytes())?;
        // 同目录 rename（POSIX 原子）。
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// —— add_evidence 旧形状已并入 insert_evidence（查重→校验→入列→审计
// 单一入口；重复 ID = DuplicateEvidenceId 显式错误）。——

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{PrecisionRule, RelationKind, RelationKind::*};
    use crate::fingerprint::{Fingerprinter, Fnv64};
    use crate::locator::CellCoord;
    use crate::refs::{DocId, EngineKind, VersionStamp};
    use crate::status::{CalcStatus, ReviewStatus};

    fn doc(engine: EngineKind, content: &str, ver: u64) -> DocumentRef {
        let fp = Fnv64.fingerprint(content.as_bytes());
        DocumentRef::new(DocId::new("src-1"), engine, VersionStamp(ver), fp)
    }

    fn evidence(id: &str, relation: RelationKind) -> EvidenceRecord {
        EvidenceRecord {
            evidence_id: id.into(),
            claim: "C7 合计 4210.00 万元".into(),
            claimed_value: Some(crate::calc::Decimal::parse("4210.00").unwrap()),
            claim_locator: Locator::DocxParagraph {
                paragraph_index: 3,
                text_fingerprint: "fnv64:cc".into(),
            },
            claim_doc: doc(EngineKind::Docx, "body-v1", 1),
            source_locator: Locator::XlsxCell {
                sheet: "报价表".into(),
                cell: CellCoord::parse("C7").unwrap(),
                value_fingerprint: Fnv64.fingerprint(b"4210.00").0,
                formula_fingerprint: None,
            },
            source_doc: doc(EngineKind::Xlsx, "source-v1", 1),
            excerpt: "C7 = 4210.00".into(),
            relation,
            calc_ref: if relation == Calculated {
                Some("calc-1".into())
            } else {
                None
            },
            linkage: LinkageStatus::Valid,
            calc: CalcStatus::NotApplicable,
            review: ReviewStatus::Pending,
            generation_batch: "b1".into(),
            created_at_ms: 1_000,
        }
    }

    #[test]
    fn source_change_degrades_to_stale_and_never_silently_rebinds() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        sc.register_source_doc(doc(EngineKind::Xlsx, "source-v1", 1), 1_100, "ai:glm");
        sc.insert_evidence(evidence("e1", DirectQuote), 1_200, "ai:glm")
            .unwrap();
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Valid);

        // 来源内容变更（指纹变）→ 同 doc_id 重新登记 → 证据降级 Stale
        sc.register_source_doc(doc(EngineKind::Xlsx, "source-v2", 2), 2_000, "human:wang");
        assert_eq!(
            sc.evidence[0].linkage,
            LinkageStatus::Stale,
            "内容变 → Stale"
        );

        // 无任何路径直接回 Valid：再登记同内容（指纹同 source-v2）
        sc.register_source_doc(doc(EngineKind::Xlsx, "source-v2", 3), 2_100, "human:wang");
        assert_eq!(
            sc.evidence[0].linkage,
            LinkageStatus::Stale,
            "引用表更新 ≠ 证据恢复（单向降级）"
        );

        // 唯一恢复通道 = 显式重验（带审计）
        sc.reverify_evidence(
            "e1",
            doc(EngineKind::Xlsx, "source-v2", 3),
            Locator::XlsxCell {
                sheet: "报价表".into(),
                cell: CellCoord::parse("C8").unwrap(),
                value_fingerprint: Fnv64.fingerprint(b"4210.00").0,
                formula_fingerprint: None,
            },
            3_000,
            "human:wang",
        )
        .unwrap();
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Valid, "显式重验恢复");
        // 审计链：register ×2 + degraded + reverify + evidence_add 事件序
        let kinds: Vec<&str> = sc.audit.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"linkage_degraded"));
        assert!(kinds.contains(&"linkage_reverified"));
        // 序号单调无空洞
        let seqs: Vec<u64> = sc.audit.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());
    }

    #[test]
    fn source_unavailable_marks_missing() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        sc.register_source_doc(doc(EngineKind::Xlsx, "source-v1", 1), 1_100, "ai:glm");
        sc.insert_evidence(evidence("e1", DirectQuote), 1_200, "ai:glm")
            .unwrap();
        sc.mark_source_unavailable("src-1", LinkageStatus::Missing, 5_000, "system");
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Missing);
        // Missing → Stale 拒（失效态互转不经重验不允许）
        sc.mark_source_unavailable("src-1", LinkageStatus::Stale, 5_100, "system");
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Missing);
    }

    #[test]
    fn duplicate_evidence_id_rejected() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "b", 1));
        sc.insert_evidence(evidence("e1", DirectQuote), 1, "ai")
            .unwrap();
        assert!(matches!(
            sc.insert_evidence(evidence("e1", DirectQuote), 2, "ai"),
            Err(SidecarError::DuplicateEvidenceId(_))
        ));
    }

    #[test]
    fn idempotent_command_replay() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "b", 1));
        let key = IdempotencyKey::new("k-1").unwrap();
        let out = CommandOutcome::Succeeded {
            result: serde_json::json!({"bound": true}),
        };
        assert!(
            sc.record_command(key.clone(), "op-1", out.clone())
                .is_none(),
            "首次记录"
        );
        let replayed = sc.record_command(key, "op-1", out).unwrap();
        assert_eq!(
            replayed,
            CommandOutcome::Succeeded {
                result: serde_json::json!({"bound": true})
            },
            "重放返回首次结果"
        );
    }

    #[test]
    fn save_load_roundtrip_and_atomicity() {
        let dir = std::env::temp_dir().join(format!("mdc-ev-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = SidecarStore::new(dir.join("sc.json"));

        let mut sc = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        sc.register_source_doc(doc(EngineKind::Xlsx, "source-v1", 1), 1_100, "ai:glm");
        sc.insert_evidence(evidence("e1", Calculated), 1_200, "ai:glm")
            .unwrap();
        sc.add_calculation(
            CalculationRecord {
                calc_id: "calc-1".into(),
                inputs: vec![],
                expression: "qty * price".into(),
                unit: Some("万元".into()),
                precision: PrecisionRule {
                    target_scale: 2,
                    rounding: crate::calc::Rounding::HalfUp,
                },
                intermediates: vec![],
                final_value: Some(crate::calc::Decimal::parse("4210.00").unwrap()),
                calc_status: CalcStatus::Passed,
            },
            1_300,
            "ai:glm",
        );
        assert!(
            !sc.add_calculation(
                CalculationRecord {
                    calc_id: "calc-1".into(),
                    inputs: vec![],
                    expression: "x".into(),
                    unit: None,
                    precision: PrecisionRule {
                        target_scale: 2,
                        rounding: crate::calc::Rounding::HalfUp
                    },
                    intermediates: vec![],
                    final_value: None,
                    calc_status: CalcStatus::NotApplicable,
                },
                1_400,
                "ai:glm"
            ),
            "同 calc_id 拒"
        );

        store.save(&sc).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded, sc, "保存/加载往返全等");
        assert!(!dir.join("sc.tmp").exists(), "临时文件已 rename 清除");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_version_gate() {
        let dir = std::env::temp_dir().join(format!("mdc-ev-ver-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = SidecarStore::new(dir.join("sc.json"));
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "b", 1));
        sc.schema_version = 99; // 模拟未来版本
        store.save(&sc).unwrap();
        match store.load() {
            Err(SidecarError::SchemaUnsupported {
                found: 99,
                supported: 1,
            }) => {}
            other => panic!("期望版本拒绝，得到 {other:?}"),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_rejected_not_guessed() {
        let dir = std::env::temp_dir().join(format!("mdc-ev-corrupt-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = SidecarStore::new(dir.join("sc.json"));
        fs::write(dir.join("sc.json"), b"{ not valid").unwrap();
        assert!(matches!(store.load(), Err(SidecarError::Corrupt(_))));
        fs::remove_dir_all(&dir).ok();
    }

    // —— s1-r2：求值落值 + 复算级联 ——

    fn calc_inputs() -> Vec<crate::evidence::CalcInput> {
        use crate::evidence::CalcInput;
        vec![
            CalcInput {
                name: "qty".into(),
                value: crate::calc::Decimal::parse("12.5").unwrap(),
                source: Some(Locator::XlsxCell {
                    sheet: "报价表".into(),
                    cell: CellCoord::parse("B7").unwrap(),
                    value_fingerprint: Fnv64.fingerprint(b"12.5").0,
                    formula_fingerprint: None,
                }),
                source_doc_id: Some("src-1".into()),
                unit: Some("件".into()),
            },
            CalcInput {
                name: "price".into(),
                value: crate::calc::Decimal::parse("336.80").unwrap(),
                source: None,
                source_doc_id: None,
                unit: Some("万元".into()),
            },
        ]
    }

    fn unevaluated_calc() -> CalculationRecord {
        CalculationRecord {
            calc_id: "calc-1".into(),
            inputs: calc_inputs(),
            expression: "qty * price".into(),
            unit: Some("万元".into()),
            precision: PrecisionRule {
                target_scale: 2,
                rounding: crate::calc::Rounding::HalfUp,
            },
            intermediates: vec![],
            final_value: None,
            calc_status: CalcStatus::NotApplicable,
        }
    }

    fn avail(v: &str) -> CurrentInput {
        CurrentInput::Available {
            value: crate::calc::Decimal::parse(v).unwrap(),
        }
    }

    #[test]
    fn apply_evaluation_lands_and_refuses_overwrite() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        assert!(sc.add_calculation(unevaluated_calc(), 1_000, "ai:glm"));
        let outcome = crate::expr::evaluate(
            "qty * price",
            &calc_inputs(),
            &PrecisionRule {
                target_scale: 2,
                rounding: crate::calc::Rounding::HalfUp,
            },
        )
        .unwrap();
        assert_eq!(outcome.value.to_string(), "4210.00");
        sc.apply_evaluation("calc-1", outcome, 1_100, "ai:glm")
            .unwrap();
        let rec = &sc.calculations[0];
        assert_eq!(rec.final_value.unwrap().to_string(), "4210.00");
        assert_eq!(rec.calc_status, CalcStatus::Passed);
        assert_eq!(rec.intermediates.len(), 2, "乘法 + rescale 两行");
        // 不静默覆写
        let again = crate::expr::evaluate(
            "qty * price",
            &calc_inputs(),
            &PrecisionRule {
                target_scale: 2,
                rounding: crate::calc::Rounding::HalfUp,
            },
        )
        .unwrap();
        assert!(matches!(
            sc.apply_evaluation("calc-1", again.clone(), 1_200, "ai:glm"),
            Err(SidecarError::AlreadyEvaluated(_))
        ));
        assert!(matches!(
            sc.apply_evaluation("no-such", again, 1_200, "ai:glm"),
            Err(SidecarError::CalculationNotFound(_))
        ));
    }

    #[test]
    fn recalculate_cascades_to_referencing_evidence() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        sc.register_source_doc(doc(EngineKind::Xlsx, "source-v1", 1), 1_000, "ai:glm");
        assert!(sc.add_calculation(unevaluated_calc(), 1_050, "ai:glm"));
        // 证据引用 calc-1（Calculated 型，claimed_value = 4210.00）
        let mut ev = evidence("e1", Calculated);
        ev.claimed_value = Some(crate::calc::Decimal::parse("4210.00").unwrap());
        sc.insert_evidence(ev, 1_100, "ai:glm").unwrap();
        // 另一条主张值错的证据（复算应判 ResultMismatch）
        let mut ev_bad = evidence("e2", Calculated);
        ev_bad.claimed_value = Some(crate::calc::Decimal::parse("9999.99").unwrap());
        sc.insert_evidence(ev_bad, 1_150, "ai:glm").unwrap();
        let outcome = crate::expr::evaluate(
            "qty * price",
            &calc_inputs(),
            &PrecisionRule {
                target_scale: 2,
                rounding: crate::calc::Rounding::HalfUp,
            },
        )
        .unwrap();
        sc.apply_evaluation("calc-1", outcome, 1_200, "ai:glm")
            .unwrap();

        // 现值一致 → 复算通过；级联：e1 通过（主张值吻合）、e2
        // ResultMismatch（主张值不吻合）
        let out = sc
            .recalculate_calc(
                "calc-1",
                &[avail("12.5"), avail("336.80")],
                2_000,
                "human:wang",
            )
            .unwrap();
        assert_eq!(out.status, CalcStatus::Passed);
        assert_eq!(sc.evidence[0].calc, CalcStatus::Passed);
        assert_eq!(
            sc.evidence[1].calc,
            CalcStatus::Failed(crate::status::CalcFailure::ResultMismatch)
        );
        // 审计事件在场
        let kinds: Vec<&str> = sc.audit.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"calc_evaluated"));
        assert!(kinds.contains(&"calc_recalc"));

        // 现值变更 → InputMismatch 级联（两条证据同因）
        let out2 = sc
            .recalculate_calc(
                "calc-1",
                &[avail("13.0"), avail("336.80")],
                3_000,
                "human:wang",
            )
            .unwrap();
        assert_eq!(
            out2.status,
            CalcStatus::Failed(crate::status::CalcFailure::InputMismatch)
        );
        for ev in &sc.evidence {
            assert_eq!(
                ev.calc,
                CalcStatus::Failed(crate::status::CalcFailure::InputMismatch),
                "失败级联同因"
            );
        }
        assert!(matches!(
            sc.recalculate_calc("no-such", &[], 4_000, "x"),
            Err(SidecarError::CalculationNotFound(_))
        ));
    }

    #[test]
    fn review_transitions_and_reset_denied() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "b", 1));
        sc.insert_evidence(evidence("e1", DirectQuote), 1, "ai:glm")
            .unwrap();
        // Pending → Approved（人工通过）
        sc.review_evidence("e1", ReviewStatus::Approved, 2_000, "human:wang")
            .unwrap();
        assert_eq!(sc.evidence[0].review, ReviewStatus::Approved);
        // Approved → Rejected（人工改判——留审计）
        sc.review_evidence("e1", ReviewStatus::Rejected, 2_100, "human:wang")
            .unwrap();
        assert_eq!(sc.evidence[0].review, ReviewStatus::Rejected);
        // → Pending 拒（人工决策不静默复位）
        assert!(matches!(
            sc.review_evidence("e1", ReviewStatus::Pending, 2_200, "human:wang"),
            Err(SidecarError::InvalidReviewTransition { .. })
        ));
        assert!(matches!(
            sc.review_evidence("no-such", ReviewStatus::Approved, 2_300, "x"),
            Err(SidecarError::EvidenceNotFound)
        ));
        let kinds: Vec<&str> = sc.audit.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"evidence_review"));
    }

    #[test]
    fn body_refresh_breaks_claim_anchor_only() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        sc.insert_evidence(evidence("e1", DirectQuote), 1, "ai:glm")
            .unwrap();
        // 正文未变（同指纹）→ 零降级
        let n = sc.refresh_document(
            doc(EngineKind::Docx, "body-v1", 2),
            &["fnv64:x".into()],
            2_000,
            "system",
        );
        assert_eq!(n, 0);
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Valid);
        // 正文改写：段落 3 指纹变 → claim 锚断 → Stale
        let new_fps: Vec<String> = (0..5).map(|i| format!("fnv64:new-{i}")).collect();
        let n2 = sc.refresh_document(
            doc(EngineKind::Docx, "body-v2", 3),
            &new_fps,
            2_100,
            "system",
        );
        assert_eq!(n2, 1, "e1 段落 3 指纹不匹配");
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Stale);
        // 段落被删（索引越界）= 锚断
        let mut sc2 = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        sc2.insert_evidence(evidence("e9", DirectQuote), 1, "ai:glm")
            .unwrap();
        let short: Vec<String> = vec!["fnv64:only-0".into()];
        let n3 = sc2.refresh_document(doc(EngineKind::Docx, "body-v2", 2), &short, 2_200, "system");
        assert_eq!(n3, 1, "段落 3 已删（正文缩短）");
        // 审计在场
        let kinds: Vec<&str> = sc.audit.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"body_refresh"));
    }

    #[test]
    fn claim_reverify_is_only_claim_side_recovery() {
        let mut sc = Sidecar::new(doc(EngineKind::Docx, "body-v1", 1));
        sc.insert_evidence(evidence("e1", DirectQuote), 1, "ai:glm")
            .unwrap();
        let new_fps: Vec<String> = (0..5).map(|i| format!("fnv64:new-{i}")).collect();
        sc.refresh_document(
            doc(EngineKind::Docx, "body-v2", 3),
            &new_fps,
            2_000,
            "system",
        );
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Stale);
        // 主张侧重验：新段落位置 + 指纹 → 恢复 Valid
        sc.reverify_claim(
            "e1",
            doc(EngineKind::Docx, "body-v2", 3),
            4,
            "fnv64:new-4".into(),
            3_000,
            "human:wang",
        )
        .unwrap();
        assert_eq!(sc.evidence[0].linkage, LinkageStatus::Valid);
        let loc = &sc.evidence[0].claim_locator;
        match loc {
            Locator::DocxParagraph {
                paragraph_index,
                text_fingerprint,
            } => {
                assert_eq!(*paragraph_index, 4);
                assert_eq!(text_fingerprint, "fnv64:new-4");
            }
            other => panic!("期望 DocxParagraph，得到 {other:?}"),
        }
        let kinds: Vec<&str> = sc.audit.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"claim_reverified"));
        // 非 DOCX 引用拒
        assert!(matches!(
            sc.reverify_claim("e1", doc(EngineKind::Xlsx, "x", 1), 1, "f".into(), 4, "x"),
            Err(SidecarError::EngineMismatch)
        ));
    }
}
