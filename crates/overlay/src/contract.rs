//! Native Projection 契约（issue #239 §六）。
//!
//! 两阶段：**plan 产出计划 + 报告（不写文件）→ 用户确认 → execute
//! 核对源文件指纹后才写**。产物是新文件字节（落盘由宿主决定——
//! 「写原生文件是显式动作、产物是新文件」）。
//!
//! ```text
//! user 确认报告后，源文件被修改了
//!   → execute 发现 source_fingerprint 不一致
//!   → 拒绝执行（StaleSource），要求重新 plan
//!   → 不允许把旧计划写入新文件
//! ```
//!
//! 适配器在应用层注册（如 `mdc-overlay-pdf` 的 `PdfNativeAdapter`），
//! 本层不认识它们——trait 是唯一契约面。

use mdc_evidence::DocumentRef;
use serde::{Deserialize, Serialize};

use crate::anchor::SemanticAnchor;
use crate::geometry::Quad;
use crate::mark::{AnnotationId, OverlayEntry, OverlayMark};
use crate::Format;

/// 适配器执行错误（typed——不静默）。
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterError {
    /// 源文件指纹与计划不符（源在 plan 后被修改——旧计划拒绝执行，
    /// 重新 plan）。携带期望/实得指纹供报告层诊断。
    StaleSource { expected: String, found: String },
    /// 调用方登记的 `DocumentRef` 指纹与源文件实际内容不符
    ///（DocumentRef 陈旧——重新登记，不得静默沿用）。
    FingerprintMismatch { registered: String, actual: String },
    /// 计划内不含任何可执行操作（空投影——调用方应先看 report）。
    EmptyPlan,
    /// 源字节无法被目标格式解析（引擎面错误描述）。
    Engine(String),
    /// 输入形状非法（如 plan 收到与适配器格式不符的标注集）。
    InvalidInput(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::StaleSource { expected, found } => write!(
                f,
                "源文件已变更（期望指纹 {expected}，实得 {found}）——旧计划拒绝执行，请重新 plan"
            ),
            AdapterError::FingerprintMismatch { registered, actual } => write!(
                f,
                "登记指纹 {registered} 与源文件实际 {actual} 不符——重新登记文档身份"
            ),
            AdapterError::EmptyPlan => f.write_str("计划不含可执行操作"),
            AdapterError::Engine(e) => write!(f, "引擎面：{e}"),
            AdapterError::InvalidInput(e) => write!(f, "输入非法：{e}"),
        }
    }
}
impl std::error::Error for AdapterError {}

/// 适配器能力声明（Phase 5 能力矩阵的最小接口面——issue §八：本轮
/// 只定义接口，不实现完整矩阵）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterCapabilities {
    /// Emphasis → 强调类原生标注（PDF /Highlight）。
    pub emphasis: bool,
    /// Comment → 评论类原生标注（PDF /Text Note）。
    pub comment: bool,
    /// Freehand → 墨迹类原生标注（PDF /Ink）。
    pub freehand: bool,
    /// Region → 形状类原生标注（PDF /Square）。
    pub region: bool,
}

impl AdapterCapabilities {
    pub fn none() -> Self {
        AdapterCapabilities {
            emphasis: false,
            comment: false,
            freehand: false,
            region: false,
        }
    }
}

/// 投影目标（**格式域契约**，非引擎依赖——issue §四映射表的目标侧
/// 形状；v0 唯一实装面 = PdfHighlight）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum ProjectionTarget {
    /// PDF /Highlight：页号（1 起）+ QuadPoints。
    PdfHighlight { page: u32, quads: Vec<Quad> },
    /// PDF /Text Note（形状占位——Comment 的目标面，本轮投影
    /// unsupported，枚举形状完整以固化 wire 面）。
    PdfTextNote { page: u32 },
}

/// 原生标注适配器契约（两阶段）。
pub trait NativeAnnotationAdapter {
    /// 适配器身份（计划回执关联 + 注册表键；稳定字符串，如
    /// `"mdc-overlay-pdf"`）。
    fn id(&self) -> &str;

    /// 目标格式（分派面——标注按 `SemanticAnchor.format` 寻适配器）。
    fn format(&self) -> Format;

    /// 能力声明（plan 前的快速过滤面）。
    fn capabilities(&self) -> AdapterCapabilities;

    /// 阶段 1：生成计划 + 报告（不写文件）。绑定源文件身份与指纹；
    /// 不能表达的标注进 `report.unsupported`/`degraded`——**不静默
    /// 丢弃**。
    ///
    /// 输入面 = `&[OverlayEntry]`（带 id 的存储单元）：报告三态全部
    /// 以 `AnnotationId` 关联，id 在存储单元而非 `OverlayMark` 上
    ///（issue #239 §六草案笔误的忠实修正——见 mdc-overlay-pdf
    /// lib 文档「显式偏差」节）。
    fn plan(
        &self,
        source_doc: &DocumentRef,
        source_bytes: &[u8],
        entries: &[OverlayEntry],
    ) -> Result<ProjectionPlan, AdapterError>;

    /// 阶段 2：执行（核对源文件指纹后才写）。产物 = 新文件字节
    ///（原字节前缀语义由具体实装的写器决定）——本契约不落盘。
    fn execute(
        &self,
        source_bytes: &[u8],
        plan: &ProjectionPlan,
    ) -> Result<ProjectionResult, AdapterError>;
}

/// 投影计划（plan 产物——用户确认的对象）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionPlan {
    /// 源文件身份（plan 时刻的登记）。
    pub source_doc: DocumentRef,
    /// 源文件指纹（执行前核对——plan 后源被改则 execute 拒绝）。
    pub source_fingerprint: String,
    /// 产出本计划的适配器（回执关联）。
    pub adapter_id: String,
    pub operations: Vec<ProjectionOperation>,
    pub report: ProjectionReport,
}

/// 单条投影操作（计划的最小执行单元——id 关联，不是字符串）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionOperation {
    pub annotation_id: AnnotationId,
    /// 原始标注载荷（回放面——执行侧不需要重算语义）。
    pub mark: OverlayMark,
    pub target: ProjectionTarget,
}

/// 投影报告（plan 阶段产出——三态分列，全部用 id 关联）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProjectionReport {
    /// 成功映射的操作（id 集）。
    pub exported: Vec<AnnotationId>,
    /// 不能表达的标注（id + 原因——如 PDF 适配器收到 Freehand 而
    /// 能力面未实装）。
    pub unsupported: Vec<(AnnotationId, String)>,
    /// 可表达但有损的标注（id + 降级说明）。
    pub degraded: Vec<(AnnotationId, String)>,
    /// 非致命警告（不影响 exported 判定）。
    pub warnings: Vec<String>,
}

impl ProjectionReport {
    /// 报告是否可安全全量执行（无 unsupported/degraded——调用方的
    /// 确认门）。
    pub fn is_clean(&self) -> bool {
        self.unsupported.is_empty() && self.degraded.is_empty()
    }
}

/// 执行结果（新文件字节 + 终报）。
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionResult {
    /// 新文件字节（**不是**原文件的就地修改——落盘由宿主决定）。
    pub bytes: Vec<u8>,
    pub report: ProjectionReport,
}

// ---------------------------------------------------------------------------
// 计划构造的共用校验（适配器实装面的复用面——不是契约的一部分，
// 但各适配器的 plan 都要过同一套不变量）
// ---------------------------------------------------------------------------

/// 计划不变量校验（适配器实装在组装完 plan 后调用）：
/// - source_fingerprint 非空；
/// - operations 的 id 与 report.exported 一一对应（计划内每条
///   exported 都有操作，反之亦然）；
/// - unsupported/degraded 的 id 不出现在 operations（三态互斥）。
pub fn validate_plan(plan: &ProjectionPlan) -> Result<(), AdapterError> {
    if plan.source_fingerprint.trim().is_empty() {
        return Err(AdapterError::InvalidInput(
            "计划缺源指纹（plan 必须绑定源文件版本）".into(),
        ));
    }
    let op_ids: std::collections::BTreeSet<&str> = plan
        .operations
        .iter()
        .map(|op| op.annotation_id.as_str())
        .collect();
    let exported: std::collections::BTreeSet<&str> =
        plan.report.exported.iter().map(|id| id.as_str()).collect();
    if op_ids != exported {
        return Err(AdapterError::InvalidInput(format!(
            "operations 与 report.exported 不一致（ops={op_ids:?} exported={exported:?}）"
        )));
    }
    for (id, _) in plan
        .report
        .unsupported
        .iter()
        .chain(plan.report.degraded.iter())
    {
        if op_ids.contains(id.as_str()) {
            return Err(AdapterError::InvalidInput(format!(
                "标注 {} 同时进 operations 与 unsupported/degraded（三态互斥破坏）",
                id.as_str()
            )));
        }
    }
    Ok(())
}

/// 标注语义锚点提取（plan 实装面的便捷面：锚点缺失或格式不符 =
/// 显式错误，不是静默跳过）。
pub fn require_anchor(mark: &OverlayMark, format: Format) -> Result<&SemanticAnchor, AdapterError> {
    let anchor = mark.anchor().ok_or_else(|| {
        AdapterError::InvalidInput(format!(
            "{} 无语义锚点（几何标注不走语义投影面）",
            mark.kind_name()
        ))
    })?;
    if anchor.format != format {
        return Err(AdapterError::InvalidInput(format!(
            "锚点格式 {:?} 与适配器目标 {:?} 不符",
            anchor.format, format
        )));
    }
    Ok(anchor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_validation_catches_triple_state_violation() {
        let id = AnnotationId::new("m1").unwrap();
        let mark = OverlayMark::Emphasis {
            anchor: SemanticAnchor::new(
                Format::Pdf,
                crate::anchor::PrimaryLocator::PdfSpan {
                    page: 1,
                    line: 0,
                    char_start: 0,
                    char_end: 2,
                },
                vec![],
                "fnv64:s",
            )
            .unwrap(),
            note: None,
        };
        let doc = DocumentRef::new(
            mdc_evidence::DocId::new("d"),
            mdc_evidence::EngineKind::Pdf,
            mdc_evidence::VersionStamp(1),
            mdc_evidence::ContentFingerprint("fnv64:a".into()),
        );
        let mk_plan = |report: ProjectionReport, ops: Vec<ProjectionOperation>| ProjectionPlan {
            source_doc: doc.clone(),
            source_fingerprint: "fnv64:src".into(),
            adapter_id: "test".into(),
            operations: ops,
            report,
        };
        let op = |id: &str| ProjectionOperation {
            annotation_id: AnnotationId::new(id).unwrap(),
            mark: mark.clone(),
            target: ProjectionTarget::PdfHighlight {
                page: 1,
                quads: vec![],
            },
        };

        // 一致：过。
        let plan = mk_plan(
            ProjectionReport {
                exported: vec![id.clone()],
                ..Default::default()
            },
            vec![op("m1")],
        );
        assert!(validate_plan(&plan).is_ok());

        // exported 缺席对应 op：拒。
        let plan = mk_plan(ProjectionReport::default(), vec![op("m1")]);
        assert!(matches!(
            validate_plan(&plan),
            Err(AdapterError::InvalidInput(_))
        ));

        // 同 id 三态互斥破坏：拒。
        let plan = mk_plan(
            ProjectionReport {
                unsupported: vec![(id.clone(), "不支持".into())],
                ..Default::default()
            },
            vec![op("m1")],
        );
        assert!(validate_plan(&plan).is_err());

        // 空指纹：拒。
        let mut plan = mk_plan(ProjectionReport::default(), vec![]);
        plan.source_fingerprint = " ".into();
        assert!(matches!(
            validate_plan(&plan),
            Err(AdapterError::InvalidInput(_))
        ));
    }
}
