//! 证据记录与计算记录（§四.2 `EvidenceRecord` / `CalculationRecord`）。
//!
//! `EvidenceRecord` = "正文中的一个主张被什么支持"；`CalculationRecord`
//! = "一个数字是怎么算出来的"。两者经 `calc_ref` 可选关联（纯引用型
//! 主张无计算记录；每个计算记录可被多条证据引用）。

use serde::{Deserialize, Serialize};

use crate::calc::{Decimal, Rounding};
use crate::locator::Locator;
use crate::refs::DocumentRef;
use crate::status::{CalcStatus, LinkageStatus, ReviewStatus};

/// 来源关系类型（§四.2——显式四分，不发明隐式等级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// 直接引用（来源原文即主张内容）。
    DirectQuote,
    /// 计算（数值经 CalculationRecord 推导）。
    Calculated,
    /// 综合推断（多来源归纳——程序不可完全验证，审核默认必经）。
    Synthesized,
    /// 人工确认（无机器可验证来源，仅人工背书）。
    HumanAttested,
}

/// 计算输入项（值 + 来源定位 + 登记时刻快照——复算时对照）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalcInput {
    /// 逻辑名（表达式内引用名，如 "qty" / "price"）。
    pub name: String,
    /// 登记值（定点十进制规范形）。
    pub value: Decimal,
    /// 值的来源定位器（None = 人工给定常量——此时该输入的正当性
    /// 完全依赖人工审核面）。
    pub source: Option<Locator>,
    /// 单位（自由文本——单位换算一致性由表达式层校验，S1 后续轮）。
    pub unit: Option<String>,
}

/// 精度与舍入规则（复算必须同规则——§四.4 "明确精度和舍入规则"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrecisionRule {
    /// 结果目标标度（小数位）。
    pub target_scale: u8,
    /// 舍入模式。
    pub rounding: Rounding,
}

/// 计算记录：输入、公式、单位、精度、中间过程与复算（§四.2/§四.4）。
///
/// 表达式在本轮为**登记不执行**（白名单求值器 = S1 后续轮）——复算
/// 状态由求值轮写入，本轮记录形状与状态语义先行冻结。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalculationRecord {
    pub calc_id: String,
    /// 输入清单（含各自来源）。
    pub inputs: Vec<CalcInput>,
    /// 计算表达式（受白名单约束的文本——求值器定义文法）。
    pub expression: String,
    /// 结果单位。
    pub unit: Option<String>,
    /// 精度与舍入规则。
    pub precision: PrecisionRule,
    /// 中间值（表达式子步求值轨迹——求值器产出，登记面先行）。
    pub intermediates: Vec<String>,
    /// 最终值（按 precision 规范形）。
    pub final_value: Option<Decimal>,
    /// 复算状态（求值器写入；登记时 = NotApplicable）。
    pub calc_status: CalcStatus,
}

/// 证据记录（§四.2 核心）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub evidence_id: String,
    /// 被支持的正文主张（人读文本——"总报价 4210.00 万元"）。
    pub claim: String,
    /// 主张所在正文位置（DOCX 侧定位器）。
    pub claim_locator: Locator,
    /// 主张登记时的正文文档引用（版本 + 指纹——失效判定锚）。
    pub claim_doc: DocumentRef,
    /// 来源定位器（XLSX 侧等）。
    pub source_locator: Locator,
    /// 来源登记时的文档引用。
    pub source_doc: DocumentRef,
    /// 原文摘录（敏感数据——sidecar 本地纪律）。
    pub excerpt: String,
    /// 关系类型。
    pub relation: RelationKind,
    /// 关联计算记录（Calculated 型必有；其他型应 None——构造面校验）。
    pub calc_ref: Option<String>,
    /// 三态（正交——分别演化，见 status 模块）。
    pub linkage: LinkageStatus,
    pub calc: CalcStatus,
    pub review: ReviewStatus,
    /// 生成批次（AI 生成轮次标识——同一批产物可整体追溯）。
    pub generation_batch: String,
    /// 创建时间（Unix 毫秒——审计用）。
    pub created_at_ms: u64,
}

impl EvidenceRecord {
    /// 构造面校验（不变量——违反 = 调用方 bug，拒建）：
    /// - Calculated 必带 calc_ref；非 Calculated 不得带（引用型主张
    ///   绑计算记录 = 语义混淆）；
    /// - claim/source 定位器引擎与所属文档引擎一致（DOCX 主张 ↔
    ///   XLSX 来源的典型形状，但不允许引擎错配的悬空引用）。
    pub fn validate(&self) -> Result<(), EvidenceError> {
        match self.relation {
            RelationKind::Calculated => {
                if self.calc_ref.as_deref().is_none_or(|s| s.is_empty()) {
                    return Err(EvidenceError::CalculatedMissingRef);
                }
            }
            _ => {
                if self.calc_ref.is_some() {
                    return Err(EvidenceError::NonCalculatedWithRef);
                }
            }
        }
        if self.claim_locator.engine() != self.claim_doc.engine {
            return Err(EvidenceError::EngineMismatch);
        }
        if self.source_locator.engine() != self.source_doc.engine {
            return Err(EvidenceError::EngineMismatch);
        }
        if self.claim.is_empty() {
            return Err(EvidenceError::EmptyClaim);
        }
        Ok(())
    }

    /// 来源变更后的失效迁移（§四.5 铁律的定点）：**只降不升**——
    /// Valid 可降 Stale/Missing/Ambiguous；已失效状态不经显式重验
    /// 不得互转或回 Valid。返回是否发生变化（供审计）。
    pub fn degrade_linkage(&mut self, to: LinkageStatus) -> bool {
        use LinkageStatus::*;
        match (self.linkage, to) {
            (Valid, new) if new != Valid => {
                self.linkage = new;
                true
            }
            // 任何 → Valid 都被拒（静默重绑禁令）——经 SidecarStore
            // 的显式重验通道恢复。
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceError {
    CalculatedMissingRef,
    NonCalculatedWithRef,
    EngineMismatch,
    EmptyClaim,
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EvidenceError::CalculatedMissingRef => "calculated relation requires calc_ref",
            EvidenceError::NonCalculatedWithRef => {
                "non-calculated relation must not carry calc_ref"
            }
            EvidenceError::EngineMismatch => "locator engine disagrees with document engine",
            EvidenceError::EmptyClaim => "claim text is empty",
        })
    }
}
impl std::error::Error for EvidenceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locator::CellCoord;
    use crate::refs::{ContentFingerprint, DocId, VersionStamp};

    fn doc(engine: crate::refs::EngineKind) -> DocumentRef {
        DocumentRef::new(
            DocId::new("d1"),
            engine,
            VersionStamp(1),
            ContentFingerprint("fnv64:aa".into()),
        )
    }

    fn record(relation: RelationKind, calc_ref: Option<String>) -> EvidenceRecord {
        EvidenceRecord {
            evidence_id: "e1".into(),
            claim: "总报价 4210.00 万元".into(),
            claim_locator: Locator::DocxParagraph {
                paragraph_index: 3,
                text_fingerprint: "fnv64:cc".into(),
            },
            claim_doc: doc(crate::refs::EngineKind::Docx),
            source_locator: Locator::XlsxCell {
                sheet: "报价表".into(),
                cell: CellCoord::parse("C7").unwrap(),
                value_fingerprint: "fnv64:dd".into(),
                formula_fingerprint: None,
            },
            source_doc: doc(crate::refs::EngineKind::Xlsx),
            excerpt: "C7: 4210.00".into(),
            relation,
            calc_ref,
            linkage: LinkageStatus::Valid,
            calc: CalcStatus::NotApplicable,
            review: ReviewStatus::Pending,
            generation_batch: "batch-1".into(),
            created_at_ms: 1_000,
        }
    }

    #[test]
    fn relation_calc_ref_invariants() {
        assert!(record(RelationKind::Calculated, Some("c1".into()))
            .validate()
            .is_ok());
        assert_eq!(
            record(RelationKind::Calculated, None)
                .validate()
                .unwrap_err(),
            EvidenceError::CalculatedMissingRef
        );
        assert_eq!(
            record(RelationKind::DirectQuote, Some("c1".into()))
                .validate()
                .unwrap_err(),
            EvidenceError::NonCalculatedWithRef
        );
        assert!(record(RelationKind::DirectQuote, None).validate().is_ok());
    }

    #[test]
    fn linkage_only_degrades_never_recovers() {
        let mut r = record(RelationKind::DirectQuote, None);
        // Valid → Stale：允许（降级）
        assert!(r.degrade_linkage(LinkageStatus::Stale));
        assert_eq!(r.linkage, LinkageStatus::Stale);
        // Stale → Valid：拒（静默重绑禁令）
        assert!(!r.degrade_linkage(LinkageStatus::Valid));
        assert_eq!(r.linkage, LinkageStatus::Stale);
        // Stale → Missing：拒（失效态互转不经重验不允许——重验通道
        // 负责一切恢复/迁移）
        assert!(!r.degrade_linkage(LinkageStatus::Missing));
        // Valid → Valid：无变化
        let mut r2 = record(RelationKind::DirectQuote, None);
        assert!(!r2.degrade_linkage(LinkageStatus::Valid));
    }
}
