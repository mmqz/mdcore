//! 三态分列报告（§九验收门禁 8——"分别报告"纪律）。
//!
//! Markdown 形（人读面）；三态各一列 + 计算轨迹 + 来源摘录。**刻意
//! 不产生任何"全部通过"汇总列**——汇总判断必须由读者显式组合三面
//! （status 模块文档的纪律落点）。

use mdc_evidence::{CalcStatus, EvidenceRecord, LinkageStatus, ReviewStatus, Sidecar};

/// 渲染全量报告（每条证据一行三态 + 计算详情段）。
pub fn render_report(sidecar: &Sidecar) -> String {
    let mut out = String::new();
    out.push_str("# 证据报告\n\n");
    out.push_str(&format!(
        "- 正文：{}（v{}，指纹 {}）\n",
        sidecar.document.doc_id.as_str(),
        sidecar.document.version.0,
        sidecar.document.fingerprint.0,
    ));
    out.push_str(&format!("- 来源文档：{} 个\n", sidecar.source_docs.len()));
    out.push_str(&format!(
        "- 证据：{} 条；计算：{} 条；审计：{} 条\n\n",
        sidecar.evidence.len(),
        sidecar.calculations.len(),
        sidecar.audit.len(),
    ));
    out.push_str("| ID | 主张 | 关联(linkage) | 计算(calc) | 审核(review) | 来源 |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for ev in &sidecar.evidence {
        out.push_str(&format!(
            "| {} | {} | {:?} | {} | {:?} | {}!… |\n",
            ev.evidence_id,
            ev.claim,
            ev.linkage,
            calc_cell(ev),
            ev.review,
            ev.source_doc.doc_id.as_str(),
        ));
    }
    if !sidecar.calculations.is_empty() {
        out.push_str("\n## 计算详情\n\n");
        for c in &sidecar.calculations {
            out.push_str(&format!(
                "### {} `{}`\n\n- 输入：{}\n- 精度：scale {} / {:?}\n- 登记值：{}\n- 复算：{:?}\n",
                c.calc_id,
                c.expression,
                c.inputs
                    .iter()
                    .map(|i| format!("{}={}{}", i.name, i.value, i.unit.as_deref().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join(", "),
                c.precision.target_scale,
                c.precision.rounding,
                c.final_value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "（未求值）".into()),
                c.calc_status,
            ));
            if !c.intermediates.is_empty() {
                out.push_str("- 轨迹：\n");
                for step in &c.intermediates {
                    out.push_str(&format!("  - `{step}`\n"));
                }
            }
            out.push('\n');
        }
    }
    out
}

fn calc_cell(ev: &EvidenceRecord) -> String {
    match ev.calc {
        CalcStatus::NotApplicable => "不适用".to_string(),
        CalcStatus::Passed => "通过".to_string(),
        CalcStatus::Failed(f) => format!("失败({f:?})"),
    }
}

/// 待人工审核清单（§四.3——不可自动验证面的归宿）。
pub fn pending_reviews(sidecar: &Sidecar) -> Vec<&EvidenceRecord> {
    sidecar
        .evidence
        .iter()
        .filter(|e| e.review == ReviewStatus::Pending)
        .collect()
}

/// 失效或更差的证据清单（重验/处理工作面）。
pub fn stale_evidence(sidecar: &Sidecar) -> Vec<&EvidenceRecord> {
    sidecar
        .evidence
        .iter()
        .filter(|e| !matches!(e.linkage, LinkageStatus::Valid))
        .collect()
}
