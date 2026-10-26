//! 证据工作台 wasm 出口（阶段 5 §八.1 壳与证据界面——p5-t3）。
//!
//! ## 定位
//! [`WasmWorkbench`] = mdc-workbench 内存会话的 wasm 句柄（与
//! [`crate::WasmCore`]/[`crate::xlsx_grid::WasmBook`] 同模式：一次
//! 构造常驻，生命周期挂 JS 侧）。**单一真值**：全部验证/级联/审计
//! 逻辑在 mdc-workbench（`Io::Memory` 后端）——本模块零验证逻辑，
//! 只做边界互译（JSON ↔ 提议结构 + typed 错误串）。
//!
//! ## 会话生命周期
//! - 构造：`new(body_bytes, doc_id, at_ms, operator)`（`start_memory`）
//!   / `restore(sidecar_json, body_bytes)`（`open_memory` 空源 +
//!   `attach_source` 逐源补字节——恢复语义：身份/版本从 sidecar 登记）。
//! - 持久化：`sidecar_json()` 单点投影（serde 原生形，与磁盘 sidecar
//!   字节同构）——何时存/存哪由宿主决定（wasm 零 std::fs 纪律）。
//! - 导出：`export_clean(at_ms, operator)` 返回字节（零标记验证照跑）。
//!
//! ## JSON 边界形态
//! - 提议（入）：扁平人读形——Decimal 一律字符串（命令层先例）；
//!   `relation` ∈ direct_quote/calculated/synthesized/human_attested；
//!   `rounding` ∈ half_up/half_even/truncate。
//! - 查询（出）：serde 原生形（与 sidecar 同构——宿主可在原生端
//!   `Workbench::open` 消费同一 JSON）。
//! - 错误：typed JSON 串 `{"code":"wb_*","message":"..."}`——
//!   WorkbenchError 全变体一码一义（见 [`WbError::code`]）。

use wasm_bindgen::prelude::*;

use mdc_evidence::{Decimal, PrecisionRule, RecalcOutcome, RelationKind, ReviewStatus, Rounding};
use mdc_workbench::{
    BindingProposal, CalcInputSpec, CalcProposal, InputSource, ReviewPolicy, Workbench,
    WorkbenchError,
};

// ---------------------------------------------------------------------------
// 错误面（typed 码——宿主可 JSON.parse）
// ---------------------------------------------------------------------------

/// wasm 边界错误（WorkbenchError 全变体 + 边界 JSON 解析错）。
#[derive(Debug)]
pub struct WbError {
    pub code: &'static str,
    pub message: String,
}

impl WbError {
    fn bad_json(what: &str, detail: String) -> Self {
        WbError {
            code: "wb_bad_json",
            message: format!("{what}: {detail}"),
        }
    }
    fn to_js(&self) -> JsValue {
        JsValue::from_str(
            &serde_json::json!({"code": self.code, "message": self.message}).to_string(),
        )
    }
}

impl From<WorkbenchError> for WbError {
    fn from(e: WorkbenchError) -> Self {
        let (code, message) = match &e {
            WorkbenchError::Sidecar(_) => ("wb_sidecar", format!("{e}")),
            WorkbenchError::Io(_) => ("wb_io", format!("{e}")),
            WorkbenchError::XlsxRead(_) => ("wb_xlsx_read", format!("{e}")),
            WorkbenchError::DocxRead(_) => ("wb_docx_read", format!("{e}")),
            WorkbenchError::Eval(_) => ("wb_eval", format!("{e}")),
            WorkbenchError::BodyStale => ("wb_body_stale", format!("{e}")),
            WorkbenchError::SourceStale { .. } => ("wb_source_stale", format!("{e}")),
            WorkbenchError::SourceNotRegistered { .. } => {
                ("wb_source_not_registered", format!("{e}"))
            }
            WorkbenchError::CellNotFound { .. } => ("wb_cell_not_found", format!("{e}")),
            WorkbenchError::CellNotNumeric { .. } => ("wb_cell_not_numeric", format!("{e}")),
            WorkbenchError::ParagraphNotFound { .. } => ("wb_paragraph_not_found", format!("{e}")),
            WorkbenchError::ClaimedValueNotInParagraph { .. } => {
                ("wb_claimed_value_not_in_paragraph", format!("{e}"))
            }
            WorkbenchError::DuplicateCalcId { .. } => ("wb_duplicate_calc_id", format!("{e}")),
            WorkbenchError::CalcNotFound { .. } => ("wb_calc_not_found", format!("{e}")),
            WorkbenchError::MarkerFound { .. } => ("wb_marker_found", format!("{e}")),
            WorkbenchError::ExportByteMismatch => ("wb_export_mismatch", format!("{e}")),
            WorkbenchError::ReviewGate { .. } => ("wb_review_gate", format!("{e}")),
            WorkbenchError::PathPolicy(_) => ("wb_path_policy", format!("{e}")),
            WorkbenchError::Limit(_) => ("wb_limit", format!("{e}")),
            WorkbenchError::Serialize(_) => ("wb_serialize", format!("{e}")),
        };
        WbError { code, message }
    }
}

type WbResult<T> = Result<T, WbError>;

// ---------------------------------------------------------------------------
// 纯内核（JsValue-free——原生 cargo test 面）
// ---------------------------------------------------------------------------

fn s<'a>(v: &'a serde_json::Value, field: &str) -> WbResult<&'a str> {
    v.get(field)
        .and_then(|x| x.as_str())
        .ok_or_else(|| WbError::bad_json("proposal", format!("missing/invalid `{field}`")))
}

fn u32f(v: &serde_json::Value, field: &str) -> WbResult<u32> {
    v.get(field)
        .and_then(|x| x.as_u64())
        .and_then(|x| u32::try_from(x).ok())
        .ok_or_else(|| WbError::bad_json("proposal", format!("missing/invalid `{field}`")))
}

/// 关联提议 JSON → BindingProposal（Decimal 字符串形；relation 四枚）。
pub fn parse_binding_core(json: &str) -> WbResult<BindingProposal> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| WbError::bad_json("binding proposal", e.to_string()))?;
    let relation = match s(&v, "relation")? {
        "direct_quote" => RelationKind::DirectQuote,
        "calculated" => RelationKind::Calculated,
        "synthesized" => RelationKind::Synthesized,
        "human_attested" => RelationKind::HumanAttested,
        other => {
            return Err(WbError::bad_json(
                "binding proposal",
                format!("unknown relation `{other}`"),
            ))
        }
    };
    let claimed_value = match v.get("claimed_value") {
        Some(serde_json::Value::String(t)) => {
            Some(Decimal::parse(t).map_err(|_| WbError::bad_json("claimed_value", t.to_string()))?)
        }
        _ => None,
    };
    let calc_ref = match v.get("calc_ref") {
        Some(serde_json::Value::String(t)) => Some(t.to_string()),
        _ => None,
    };
    Ok(BindingProposal {
        evidence_id: s(&v, "evidence_id")?.to_string(),
        claim: s(&v, "claim")?.to_string(),
        claimed_value,
        paragraph_index: u32f(&v, "paragraph_index")?,
        source_doc_id: s(&v, "source_doc_id")?.to_string(),
        sheet: s(&v, "sheet")?.to_string(),
        coord: s(&v, "coord")?.to_string(),
        relation,
        calc_ref,
        generation_batch: s(&v, "generation_batch")?.to_string(),
    })
}

/// 计算提议 JSON → CalcProposal（inputs 的 source 两形态：cell/constant）。
pub fn parse_calc_core(json: &str) -> WbResult<CalcProposal> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| WbError::bad_json("calc proposal", e.to_string()))?;
    let precision = v
        .get("precision")
        .ok_or_else(|| WbError::bad_json("calc proposal", "missing `precision`".into()))?;
    let rounding = match precision
        .get("rounding")
        .and_then(|x| x.as_str())
        .unwrap_or("half_up")
    {
        "half_up" => Rounding::HalfUp,
        "half_even" => Rounding::HalfEven,
        "truncate" => Rounding::Truncate,
        other => {
            return Err(WbError::bad_json(
                "calc proposal",
                format!("unknown rounding `{other}`"),
            ))
        }
    };
    let inputs_v = v
        .get("inputs")
        .and_then(|x| x.as_array())
        .ok_or_else(|| WbError::bad_json("calc proposal", "missing `inputs`".into()))?;
    let mut inputs = Vec::with_capacity(inputs_v.len());
    for item in inputs_v {
        let name = s(item, "name")?.to_string();
        let unit = match item.get("unit") {
            Some(serde_json::Value::String(t)) => Some(t.to_string()),
            _ => None,
        };
        let source = if let Some(cell) = item.get("source").and_then(|x| x.get("cell")) {
            InputSource::Cell {
                doc_id: s(cell, "doc_id")?.to_string(),
                sheet: s(cell, "sheet")?.to_string(),
                coord: s(cell, "coord")?.to_string(),
            }
        } else if let Some(c) = item
            .get("source")
            .and_then(|x| x.get("constant"))
            .and_then(|x| x.as_str())
        {
            InputSource::Constant {
                value: Decimal::parse(c)
                    .map_err(|_| WbError::bad_json("constant input", c.to_string()))?,
            }
        } else {
            return Err(WbError::bad_json(
                "calc proposal",
                format!("input `{name}`: source must be cell or constant"),
            ));
        };
        inputs.push(CalcInputSpec { name, source, unit });
    }
    let unit = match v.get("unit") {
        Some(serde_json::Value::String(t)) => Some(t.to_string()),
        _ => None,
    };
    Ok(CalcProposal {
        calc_id: s(&v, "calc_id")?.to_string(),
        expression: s(&v, "expression")?.to_string(),
        unit,
        precision: PrecisionRule {
            target_scale: u8::try_from(u32f(precision, "target_scale")?)
                .map_err(|_| WbError::bad_json("calc proposal", "target_scale over u8".into()))?,
            rounding,
        },
        inputs,
    })
}

/// 审核决策串 → ReviewStatus。
pub fn parse_decision_core(d: &str) -> WbResult<ReviewStatus> {
    match d {
        "approved" => Ok(ReviewStatus::Approved),
        "rejected" => Ok(ReviewStatus::Rejected),
        "pending" => Ok(ReviewStatus::Pending),
        other => Err(WbError::bad_json(
            "review decision",
            format!("unknown `{other}`"),
        )),
    }
}

/// 审核门槛 JSON → ReviewPolicy（会话级——不入 sidecar 纪律保持）。
pub fn parse_review_policy_core(json: &str) -> WbResult<ReviewPolicy> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| WbError::bad_json("review policy", e.to_string()))?;
    let prefixes: Vec<String> = match v.get("reviewer_prefixes") {
        None => vec!["human:".to_string()],
        Some(serde_json::Value::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for item in a {
                let p = item.as_str().ok_or_else(|| {
                    WbError::bad_json(
                        "review policy",
                        "reviewer_prefixes entries must be strings".to_string(),
                    )
                })?;
                out.push(p.to_string());
            }
            // 空列表 = operator_allowed 恒真（全员放行）——收紧意图
            // 的调用方拿到放宽，fail-open，typed 拒（审查 S3）。
            if out.is_empty() {
                return Err(WbError::bad_json(
                    "review policy",
                    "reviewer_prefixes must not be empty".to_string(),
                ));
            }
            out
        }
        Some(other) => {
            return Err(WbError::bad_json(
                "review policy",
                format!("reviewer_prefixes must be an array, got {other}"),
            ))
        }
    };
    Ok(ReviewPolicy {
        reviewer_prefixes: prefixes,
        allow_approval_of_broken_linkage: v
            .get("allow_approval_of_broken_linkage")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        calc_required_at_or_above: match v.get("calc_required_at_or_above") {
            Some(serde_json::Value::String(t)) => Some(
                Decimal::parse(t)
                    .map_err(|_| WbError::bad_json("calc_required_at_or_above", t.clone()))?,
            ),
            _ => None,
        },
    })
}

/// RecalcOutcome → JSON（手拼——status snake_case + computed 字符串形）。
fn recalc_outcome_json(outcome: &RecalcOutcome) -> String {
    let status = match outcome.status {
        mdc_evidence::CalcStatus::NotApplicable => "not_applicable",
        mdc_evidence::CalcStatus::Passed => "passed",
        mdc_evidence::CalcStatus::Failed(_) => "failed",
    };
    match &outcome.computed {
        Some(v) => format!(
            "{{\"status\":\"{status}\",\"computed\":{}}}",
            serde_json::to_string(&v.to_string()).unwrap()
        ),
        None => format!("{{\"status\":\"{status}\",\"computed\":null}}"),
    }
}

/// 段落清单 JSON（claim 锚跳转数据源）。
pub fn paragraphs_json(paras: &[mdc_workbench::ParagraphInfo]) -> String {
    let items: Vec<String> = paras
        .iter()
        .map(|p| {
            format!(
                "{{\"index\":{},\"text\":{},\"normalized\":{},\"fingerprint\":{}}}",
                p.index,
                serde_json::to_string(&p.text).unwrap(),
                serde_json::to_string(&p.normalized).unwrap(),
                serde_json::to_string(&p.fingerprint.0).unwrap()
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// DocumentRef → JSON（serde 原生形——与 sidecar 同构）。
fn doc_ref_json(r: &mdc_evidence::DocumentRef) -> String {
    serde_json::to_string(r).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// wasm-bindgen 面
// ---------------------------------------------------------------------------

/// 证据工作台内存会话句柄（表格/正文/证据三表面共享的权威侧栏数据源）。
#[wasm_bindgen]
pub struct WasmWorkbench {
    inner: Workbench,
}

#[wasm_bindgen]
impl WasmWorkbench {
    /// 新会话：登记 DOCX 正文（字节入口——`start_memory`）。
    #[wasm_bindgen(constructor)]
    pub fn new(
        body_bytes: &[u8],
        body_doc_id: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<WasmWorkbench, JsValue> {
        let inner =
            Workbench::start_memory(body_bytes, body_doc_id, at_ms, operator).map_err(wb_js)?;
        Ok(WasmWorkbench { inner })
    }

    /// 恢复会话：sidecar JSON + 正文字节（来源字节经 `attach_source`
    /// 逐源补——恢复语义：身份/版本/证据/审计从 sidecar 登记）。
    #[wasm_bindgen(static_method_of = WasmWorkbench)]
    pub fn restore(sidecar_json: &str, body_bytes: &[u8]) -> Result<WasmWorkbench, JsValue> {
        let inner = Workbench::open_memory(sidecar_json, body_bytes, &[]).map_err(wb_js)?;
        Ok(WasmWorkbench { inner })
    }

    /// restore 后逐源补字节（幂等；不动 sidecar/审计/版本）。
    pub fn attach_source(&mut self, doc_id: &str, bytes: &[u8]) -> Result<(), JsValue> {
        self.inner.attach_source_bytes(doc_id, bytes).map_err(wb_js)
    }

    /// ①/⑥ 来源摄取（字节入口；同 doc_id 重复 = 刷新——指纹变化自动失效）。
    pub fn ingest_source(
        &mut self,
        doc_id: &str,
        bytes: &[u8],
        at_ms: u64,
        operator: &str,
    ) -> Result<String, JsValue> {
        let r = self
            .inner
            .ingest_source_bytes(doc_id, bytes, at_ms, operator)
            .map_err(wb_js)?;
        Ok(doc_ref_json(&r))
    }

    /// ③ 关联提议（程序验证 + 构造；review 落 Pending）。返回 evidence_id。
    pub fn propose_binding(
        &mut self,
        proposal_json: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<String, JsValue> {
        let p = parse_binding_core(proposal_json).map_err(wb_js)?;
        self.inner
            .propose_binding(p, at_ms, operator)
            .map_err(wb_js)
    }

    /// ④ 计算提议（表达式求值器）。返回计算值字符串（定点十进制）。
    pub fn propose_calculation(
        &mut self,
        calc_json: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<String, JsValue> {
        let p = parse_calc_core(calc_json).map_err(wb_js)?;
        let v = self
            .inner
            .propose_calculation(p, at_ms, operator)
            .map_err(wb_js)?;
        Ok(v.to_string())
    }

    /// ④ 复算（现值从当前内存源提取）。返回 RecalcOutcome JSON。
    pub fn recalc_current(
        &mut self,
        calc_id: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<String, JsValue> {
        let o = self
            .inner
            .recalc_current(calc_id, at_ms, operator)
            .map_err(wb_js)?;
        Ok(recalc_outcome_json(&o))
    }

    /// ⑥ 来源侧显式重验（唯一恢复通道·源侧）。
    pub fn reverify_source(
        &mut self,
        evidence_id: &str,
        doc_id: &str,
        sheet: &str,
        coord: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), JsValue> {
        self.inner
            .reverify_source(evidence_id, doc_id, sheet, coord, at_ms, operator)
            .map_err(wb_js)
    }

    /// ⑥ 主张侧显式重验（claim 锚）。
    pub fn reverify_claim(
        &mut self,
        evidence_id: &str,
        paragraph_index: u32,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), JsValue> {
        self.inner
            .reverify_claim(evidence_id, paragraph_index, at_ms, operator)
            .map_err(wb_js)
    }

    /// ⑥ 正文变更失效检测（新正文字节——「文件已变」的内存等价物）。
    /// 返回本次降级条数。
    pub fn refresh_body(
        &mut self,
        new_body_bytes: &[u8],
        at_ms: u64,
        operator: &str,
    ) -> Result<usize, JsValue> {
        self.inner
            .refresh_body_bytes(new_body_bytes, at_ms, operator)
            .map_err(wb_js)
    }

    /// ⑦ 人工审核（决策 ∈ approved/rejected/pending；门槛拒 = typed 错）。
    pub fn review(
        &mut self,
        evidence_id: &str,
        decision: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), JsValue> {
        let to = parse_decision_core(decision).map_err(wb_js)?;
        self.inner
            .review(evidence_id, to, at_ms, operator)
            .map_err(wb_js)
    }

    /// 审核门槛策略注入（会话级；不入 sidecar）。
    pub fn set_review_policy(&mut self, policy_json: &str) -> Result<(), JsValue> {
        let p = parse_review_policy_core(policy_json).map_err(wb_js)?;
        self.inner.set_review_policy(p);
        Ok(())
    }

    /// sidecar 持久化面（单点投影——serde 原生形，与磁盘字节同构）。
    pub fn sidecar_json(&self) -> Result<String, JsValue> {
        self.inner.sidecar_json().map_err(wb_js)
    }

    /// 全量证据清单（serde 原生形）。
    pub fn evidence_json(&self) -> String {
        serde_json::to_string(&self.inner.sidecar().evidence).unwrap_or_else(|_| "[]".into())
    }

    /// 待审清单（serde 原生形）。
    pub fn pending_reviews_json(&self) -> String {
        serde_json::to_string(&self.inner.pending_reviews()).unwrap_or_else(|_| "[]".into())
    }

    /// 失效/更差证据清单（serde 原生形）。
    pub fn stale_evidence_json(&self) -> String {
        serde_json::to_string(&self.inner.stale_evidence()).unwrap_or_else(|_| "[]".into())
    }

    /// 审计链（serde 原生形）。
    pub fn audit_json(&self) -> String {
        serde_json::to_string(&self.inner.sidecar().audit).unwrap_or_else(|_| "[]".into())
    }

    /// 三态报告（Markdown 原样）。
    pub fn report(&self) -> String {
        self.inner.report()
    }

    /// 当前正文段落清单（claim 锚跳转数据源）。
    pub fn paragraphs_json(&self) -> Result<String, JsValue> {
        let paras = self.inner.current_paragraphs().map_err(wb_js)?;
        Ok(paragraphs_json(&paras))
    }

    /// ⑧ 干净导出（字节返回——零标记验证照跑；落点由宿主决定）。
    pub fn export_clean(&mut self, at_ms: u64, operator: &str) -> Result<Vec<u8>, JsValue> {
        self.inner
            .export_clean_bytes(at_ms, operator)
            .map_err(wb_js)
    }

    /// ⑧-bis 暗标匿名导出（G3 / R-11：字节返回版——宿主自定落点；
    /// 元数据级清零在返回字节内完成）。
    pub fn export_anonymous(&mut self, at_ms: u64, operator: &str) -> Result<Vec<u8>, JsValue> {
        self.inner
            .export_anonymous_bytes(at_ms, operator)
            .map_err(wb_js)
    }
}

fn wb_js(e: impl Into<WbError>) -> JsValue {
    e.into().to_js()
}
