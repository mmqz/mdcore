//! 三态正交状态模型（§四.2 末段——"不要只用一个 verified 字段"）。
//!
//! # 为什么是三个独立枚举
//!
//! 三个正确性维度**互不蕴含**：
//! - 来源仍可打开（linkage=Valid）≠ 来源支持结论（需人工/计算判断）；
//! - 公式复算通过（calc=Passed）≠ 输入数据正确或适用于该结论；
//! - 人工通过（review=Approved）不改变前两者的机器事实。
//!
//! 因此不存在单一 `verified` 汇总——报告层分别呈现三面（§九验收
//! 门禁 12 的"分别报告"纪律）。任何"全部通过"的汇总判断都必须在
//! 调用方显式组合三面，无法在数据层静默发生。

use serde::{Deserialize, Serialize};

/// 来源关联状态（证据 ↔ 来源定位器的可达与一致面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkageStatus {
    /// 定位器可达 + 指纹一致。
    Valid,
    /// 定位器可达但指纹不匹配（内容已换）——**只能经显式重验恢复**，
    /// 不得静默重绑（§四.5 铁律）。
    Stale,
    /// 来源文档缺失/无法打开/定位坐标超界。
    Missing,
    /// 定位器解析出多个候选（如文本重复出现致路径歧义）——需人工
    /// 消歧后重绑。
    Ambiguous,
}

/// 计算状态（可复算面的机器判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalcStatus {
    /// 非计算型主张（纯引用/事实陈述）。
    NotApplicable,
    /// 复算通过（输入/公式/精度全链一致）。
    Passed,
    /// 复算失败（携带原因码——见变体）。
    Failed(CalcFailure),
}

/// 复算失败原因（显式分类——报告层可诊断，不笼统"failed"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalcFailure {
    /// 输入缺失或自身已失效（上游 Linkage 非 Valid 时复算不允许
    /// 通过——级联诚实性）。
    InputUnavailable,
    /// 输入值与登记值不符（来源被改）。
    InputMismatch,
    /// 公式求值错误（除零/域越界/语法）。
    EvaluationError,
    /// 精度/舍入规则与登记不符。
    PrecisionMismatch,
    /// 结果与正文主张值不符。
    ResultMismatch,
}

/// 人工审核状态（无法自动验证的文本主张的归宿——§四.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    /// 待审核（默认态；AI 提出的关联/推导一律落此态——程序只能
    /// 验证可程序验证的面，模型置信度不是验证）。
    Pending,
    /// 人工通过。
    Approved,
    /// 人工拒绝。
    Rejected,
}

impl LinkageStatus {
    /// 复算的前置门槛：仅 Valid 可进入复算（其余一律
    /// CalcStatus::Failed(InputUnavailable)——来源不实不许复算通过）。
    pub fn allows_recalc(self) -> bool {
        matches!(self, LinkageStatus::Valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_allows_recalc() {
        assert!(LinkageStatus::Valid.allows_recalc());
        assert!(!LinkageStatus::Stale.allows_recalc());
        assert!(!LinkageStatus::Missing.allows_recalc());
        assert!(!LinkageStatus::Ambiguous.allows_recalc());
    }

    #[test]
    fn three_states_are_orthogonal_in_serde() {
        // 三态可独立取值且序列化不互串（正交性的最小实证）。
        let l = serde_json::to_string(&LinkageStatus::Stale).unwrap();
        assert_eq!(l, "\"stale\"");
        let c = serde_json::to_string(&CalcStatus::Failed(CalcFailure::InputMismatch)).unwrap();
        assert!(c.contains("input_mismatch"));
        let r = serde_json::to_string(&ReviewStatus::Pending).unwrap();
        assert_eq!(r, "\"pending\"");
    }
}
