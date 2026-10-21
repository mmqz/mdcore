//! 八步闭环编排（会话 = sidecar + 正文/来源路径注册表）。
//!
//! # 会话形态
//!
//! [`Workbench`] 持有一个 [`Sidecar`]（经 [`SidecarStore`] 原子落盘）
//! 与路径注册表（正文一个 + 来源多个）。**每次变更即存盘**——不存
//! 在"已改内存未落盘"的可见中间态（§九.11 发布面纪律）。
//!
//! # 验证纪律（每方法的程序验证面）
//!
//! - 提议只含**映射与公式**（段落号/表/坐标/表达式/精度/关系）；
//!   数值、指纹、摘录全部由程序从真实文件提取构造。
//! - 绑定与计算前 fail-closed 检查：正文/来源文件当前指纹必须等于
//!   sidecar 登记引用的指纹——文件已变而未刷新 = 拒绝提议（先
//!   `refresh_body`/重新 `ingest_source`，绝不绑向过期视图）。
//! - `claimed_value` 必须真实出现在所指段落文本中（数字格式容差 =
//!   原串/去尾零两形——千分位等变体不猜，容差外显式报错）。

use std::path::{Path, PathBuf};

use mdc_evidence::{
    CalculationRecord, CurrentInput, Decimal, DocumentRef, EvalError, EvidenceRecord,
    Fingerprinter, Fnv64, IdempotencyKey, Locator, RecalcOutcome, RelationKind, ReviewStatus,
    Sidecar, SidecarError, SidecarStore,
};
use mdc_office_io::docx::{read_docx_from_bytes, DocxError};
use mdc_office_io::io::ReadError;

use crate::ingest::{self, DocxBody, XlsxSource};
use crate::WORKBENCH_MARKERS;
use mdc_evidence::{CalcInput, PrecisionRule};

/// 来源注册项（doc_id ↔ 路径 + 当前版本）。
#[derive(Debug, Clone)]
struct SourceRegistration {
    doc_id: String,
    path: PathBuf,
    version: u64,
}

/// 关联提议（AI/人给出映射，程序验证并构造记录）。
#[derive(Debug, Clone)]
pub struct BindingProposal {
    pub evidence_id: String,
    /// 主张文本（AI 对正文断言的概括——人读面）。
    pub claim: String,
    /// 主张数值（Calculated 必填；DirectQuote 可选——供对照）。
    pub claimed_value: Option<Decimal>,
    /// 正文段落号（0 起，仅段落计）。
    pub paragraph_index: u32,
    /// 来源文档 ID（须已注册）。
    pub source_doc_id: String,
    pub sheet: String,
    /// A1 记法。
    pub coord: String,
    pub relation: RelationKind,
    /// Calculated 型必填（前向引用——计算步骤稍后落）。
    pub calc_ref: Option<String>,
    /// 生成批次（AI 轮次标识）。
    pub generation_batch: String,
}

/// 计算输入来源（单元格映射或常量声明）。
#[derive(Debug, Clone)]
pub enum InputSource {
    Cell {
        doc_id: String,
        sheet: String,
        coord: String,
    },
    /// 人工常量（提议方供值的**唯一例外**——正当性归人工审核面，
    /// 提议即声明）。
    Constant { value: Decimal },
}

/// 计算输入规格（名字 + 来源 + 单位）。
#[derive(Debug, Clone)]
pub struct CalcInputSpec {
    pub name: String,
    pub source: InputSource,
    pub unit: Option<String>,
}

/// 计算提议（表达式 + 精度 + 输入映射——数值由程序提取）。
#[derive(Debug, Clone)]
pub struct CalcProposal {
    pub calc_id: String,
    pub expression: String,
    pub unit: Option<String>,
    pub precision: PrecisionRule,
    pub inputs: Vec<CalcInputSpec>,
}

/// 编排错误（全部显式分类）。
#[derive(Debug)]
pub enum WorkbenchError {
    Sidecar(SidecarError),
    Io(std::io::Error),
    XlsxRead(ReadError),
    DocxRead(DocxError),
    Eval(EvalError),
    /// 正文文件指纹 ≠ sidecar 登记引用（先 refresh_body）。
    BodyStale,
    /// 来源文件指纹 ≠ 注册引用（先重新 ingest_source）。
    SourceStale {
        doc_id: String,
    },
    SourceNotRegistered {
        doc_id: String,
    },
    CellNotFound {
        doc_id: String,
        sheet: String,
        coord: String,
    },
    CellNotNumeric {
        doc_id: String,
        sheet: String,
        coord: String,
    },
    ParagraphNotFound {
        index: u32,
    },
    /// 主张数值未出现在所指段落文本（两格式容差外）。
    ClaimedValueNotInParagraph {
        value: String,
    },
    DuplicateCalcId {
        calc_id: String,
    },
    CalcNotFound {
        calc_id: String,
    },
    /// 干净导出验证失败（正文含工作台标记）。
    MarkerFound {
        marker: String,
    },
    ExportByteMismatch,
}

impl std::fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkbenchError::Sidecar(e) => write!(f, "sidecar: {e}"),
            WorkbenchError::Io(e) => write!(f, "io: {e}"),
            WorkbenchError::XlsxRead(e) => write!(f, "xlsx read: {e}"),
            WorkbenchError::DocxRead(e) => write!(f, "docx read: {e}"),
            WorkbenchError::Eval(e) => write!(f, "eval: {e}"),
            WorkbenchError::BodyStale => {
                f.write_str("body file changed since registration — refresh_body before proposing")
            }
            WorkbenchError::SourceStale { doc_id } => write!(
                f,
                "source '{doc_id}' changed since registration — re-ingest before proposing"
            ),
            WorkbenchError::SourceNotRegistered { doc_id } => {
                write!(f, "source not registered: {doc_id}")
            }
            WorkbenchError::CellNotFound {
                doc_id,
                sheet,
                coord,
            } => write!(f, "cell not found: {doc_id}!{sheet}!{coord}"),
            WorkbenchError::CellNotNumeric {
                doc_id,
                sheet,
                coord,
            } => write!(f, "cell not numeric: {doc_id}!{sheet}!{coord}"),
            WorkbenchError::ParagraphNotFound { index } => {
                write!(f, "paragraph not found: {index}")
            }
            WorkbenchError::ClaimedValueNotInParagraph { value } => write!(
                f,
                "claimed value '{value}' does not appear in the claim paragraph"
            ),
            WorkbenchError::DuplicateCalcId { calc_id } => {
                write!(f, "duplicate calc id: {calc_id}")
            }
            WorkbenchError::CalcNotFound { calc_id } => {
                write!(f, "calc not found: {calc_id}")
            }
            WorkbenchError::MarkerFound { marker } => {
                write!(
                    f,
                    "clean export verification failed: marker '{marker}' found in body"
                )
            }
            WorkbenchError::ExportByteMismatch => {
                f.write_str("clean export verification failed: bytes differ from body")
            }
        }
    }
}
impl std::error::Error for WorkbenchError {}

impl From<SidecarError> for WorkbenchError {
    fn from(e: SidecarError) -> Self {
        WorkbenchError::Sidecar(e)
    }
}
impl From<std::io::Error> for WorkbenchError {
    fn from(e: std::io::Error) -> Self {
        WorkbenchError::Io(e)
    }
}

/// 八步闭环会话。
pub struct Workbench {
    sidecar: Sidecar,
    store: SidecarStore,
    body_path: PathBuf,
    body_doc_id: String,
    body_version: u64,
    sources: Vec<SourceRegistration>,
}

impl Workbench {
    /// ②登记 DOCX 正文（建 sidecar；正文指纹 = 登记引用）。
    pub fn start(
        sidecar_path: &Path,
        body_path: &Path,
        body_doc_id: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<Workbench, WorkbenchError> {
        let body = ingest::ingest_docx(body_path)?;
        let body_ref = ingest::docx_body_ref(body_doc_id, &body, 1);
        let mut sidecar = Sidecar::new(body_ref);
        sidecar.push_audit_public(
            "workbench_start",
            serde_json::json!({
                "body_doc_id": body_doc_id,
            }),
            at_ms,
            operator,
        );
        let store = SidecarStore::new(sidecar_path);
        store.save(&sidecar)?;
        Ok(Workbench {
            sidecar,
            store,
            body_path: body_path.to_path_buf(),
            body_doc_id: body_doc_id.to_string(),
            body_version: 1,
            sources: vec![],
        })
    }

    /// 重开会话（路径由调用方重供——路径不入 sidecar）。
    pub fn open(
        sidecar_path: &Path,
        body_path: &Path,
        source_paths: &[(String, PathBuf)],
    ) -> Result<Workbench, WorkbenchError> {
        let store = SidecarStore::new(sidecar_path);
        let sidecar = store
            .load()?
            .ok_or(SidecarError::Corrupt("sidecar not found".into()))?;
        let sources = source_paths
            .iter()
            .map(|(id, p)| SourceRegistration {
                doc_id: id.clone(),
                path: p.clone(),
                version: 1,
            })
            .collect();
        Ok(Workbench {
            sidecar,
            store,
            body_path: body_path.to_path_buf(),
            body_doc_id: String::new(),
            body_version: 1,
            sources,
        })
    }

    pub fn sidecar(&self) -> &Sidecar {
        &self.sidecar
    }

    /// 直接访问（高级面——变更后请自行 save；常规操作走各步方法）。
    pub fn sidecar_mut(&mut self) -> &mut Sidecar {
        &mut self.sidecar
    }

    /// ⑤ 报告（三态分列——见 report 模块）。
    pub fn report(&self) -> String {
        crate::report::render_report(&self.sidecar)
    }

    /// ⑦ 待人工审核清单。
    pub fn pending_reviews(&self) -> Vec<&EvidenceRecord> {
        crate::report::pending_reviews(&self.sidecar)
    }

    /// 失效/更差证据清单（重验工作面）。
    pub fn stale_evidence(&self) -> Vec<&EvidenceRecord> {
        crate::report::stale_evidence(&self.sidecar)
    }

    fn save(&self) -> Result<(), WorkbenchError> {
        self.store.save(&self.sidecar)?;
        Ok(())
    }

    fn current_body(&self) -> Result<DocxBody, WorkbenchError> {
        ingest::ingest_docx(&self.body_path)
    }

    /// 正文 fail-closed 检查：当前文件指纹 == sidecar 登记引用。
    fn check_body_fresh(&self, body: &DocxBody) -> Result<(), WorkbenchError> {
        if body.fingerprint != self.sidecar.document.fingerprint {
            return Err(WorkbenchError::BodyStale);
        }
        Ok(())
    }

    /// ①/⑥ 导入或刷新 XLSX 来源（同 doc_id 重复调用 = 重读文件重
    /// 登记——指纹变化自动触发证据失效，版本自增）。
    pub fn ingest_source(
        &mut self,
        doc_id: &str,
        path: &Path,
        at_ms: u64,
        operator: &str,
    ) -> Result<DocumentRef, WorkbenchError> {
        let src = ingest::ingest_xlsx(path, doc_id)?;
        let version = match self.sources.iter().find(|s| s.doc_id == doc_id) {
            Some(reg) => reg.version + 1,
            None => 1,
        };
        let doc_ref = ingest::xlsx_source_ref(&src, version);
        self.sidecar
            .register_source_doc(doc_ref.clone(), at_ms, operator);
        match self.sources.iter_mut().find(|s| s.doc_id == doc_id) {
            Some(reg) => {
                reg.path = path.to_path_buf();
                reg.version = version;
            }
            None => self.sources.push(SourceRegistration {
                doc_id: doc_id.to_string(),
                path: path.to_path_buf(),
                version,
            }),
        }
        self.save()?;
        Ok(doc_ref)
    }

    /// 来源 fail-closed 检查：当前文件指纹 == 注册引用（返回摄取产物）。
    fn fresh_source(&self, doc_id: &str) -> Result<(XlsxSource, DocumentRef), WorkbenchError> {
        let reg = self
            .sources
            .iter()
            .find(|s| s.doc_id == doc_id)
            .ok_or_else(|| WorkbenchError::SourceNotRegistered {
                doc_id: doc_id.to_string(),
            })?;
        let src = ingest::ingest_xlsx(&reg.path, doc_id)?;
        let doc_ref = ingest::xlsx_source_ref(&src, reg.version);
        let registered = self
            .sidecar
            .source_docs
            .iter()
            .find(|d| d.doc_id.as_str() == doc_id)
            .ok_or_else(|| WorkbenchError::SourceNotRegistered {
                doc_id: doc_id.to_string(),
            })?;
        if src.fingerprint != registered.fingerprint {
            return Err(WorkbenchError::SourceStale {
                doc_id: doc_id.to_string(),
            });
        }
        Ok((src, doc_ref))
    }

    /// ③ 关联（AI 提议 / 程序验证 + 构造，review 落 Pending）。
    pub fn propose_binding(
        &mut self,
        p: BindingProposal,
        at_ms: u64,
        operator: &str,
    ) -> Result<String, WorkbenchError> {
        // 正文 fail-closed + 段落存在 + 主张数值在场
        let body = self.current_body()?;
        self.check_body_fresh(&body)?;
        let para = body
            .paragraph(p.paragraph_index)
            .ok_or(WorkbenchError::ParagraphNotFound {
                index: p.paragraph_index,
            })?;
        if let Some(v) = &p.claimed_value {
            let s = v.to_string();
            let trimmed = trim_trailing_zeros(&s);
            if !para.normalized.contains(&s) && !para.normalized.contains(trimmed) {
                return Err(WorkbenchError::ClaimedValueNotInParagraph { value: s });
            }
        }
        // 来源 fail-closed + 单元格存在
        let (src, source_ref) = self.fresh_source(&p.source_doc_id)?;
        let cell = src
            .cell(&p.sheet, &p.coord)
            .ok_or(WorkbenchError::CellNotFound {
                doc_id: p.source_doc_id.clone(),
                sheet: p.sheet.clone(),
                coord: p.coord.clone(),
            })?;
        let source_locator = Locator::XlsxCell {
            sheet: cell.sheet.clone(),
            cell: mdc_evidence::CellCoord::parse(&cell.coord).map_err(|_| {
                WorkbenchError::CellNotFound {
                    doc_id: p.source_doc_id.clone(),
                    sheet: p.sheet.clone(),
                    coord: p.coord.clone(),
                }
            })?,
            value_fingerprint: Fnv64.fingerprint(cell.repr.as_bytes()).0,
            formula_fingerprint: cell
                .formula
                .as_deref()
                .map(|f| Fnv64.fingerprint(f.as_bytes()).0),
        };
        let record = EvidenceRecord {
            evidence_id: p.evidence_id.clone(),
            claim: p.claim,
            claimed_value: p.claimed_value,
            claim_locator: Locator::DocxParagraph {
                paragraph_index: p.paragraph_index,
                text_fingerprint: para.fingerprint.0.clone(),
            },
            claim_doc: self.sidecar.document.clone(),
            source_locator,
            source_doc: source_ref,
            // 摘录由程序从真实格构造（提议方不供文）。
            excerpt: format!("{}!{}: {}", cell.sheet, cell.coord, cell.repr),
            relation: p.relation,
            calc_ref: p.calc_ref,
            linkage: mdc_evidence::LinkageStatus::Valid,
            calc: mdc_evidence::CalcStatus::NotApplicable,
            review: ReviewStatus::Pending,
            generation_batch: p.generation_batch,
            created_at_ms: at_ms,
        };
        self.sidecar.insert_evidence(record, at_ms, operator)?;
        self.save()?;
        Ok(p.evidence_id)
    }

    /// ④ 计算（程序提取数值 + 求值 + 落值）。
    pub fn propose_calculation(
        &mut self,
        p: CalcProposal,
        at_ms: u64,
        operator: &str,
    ) -> Result<Decimal, WorkbenchError> {
        let mut inputs = vec![];
        for spec in &p.inputs {
            match &spec.source {
                InputSource::Cell {
                    doc_id,
                    sheet,
                    coord,
                } => {
                    let (src, _) = self.fresh_source(doc_id)?;
                    let cell = src.cell(sheet, coord).ok_or(WorkbenchError::CellNotFound {
                        doc_id: doc_id.clone(),
                        sheet: sheet.clone(),
                        coord: coord.clone(),
                    })?;
                    let value = cell.decimal.ok_or(WorkbenchError::CellNotNumeric {
                        doc_id: doc_id.clone(),
                        sheet: sheet.clone(),
                        coord: coord.clone(),
                    })?;
                    inputs.push(CalcInput {
                        name: spec.name.clone(),
                        value,
                        source: Some(Locator::XlsxCell {
                            sheet: cell.sheet.clone(),
                            cell: mdc_evidence::CellCoord::parse(&cell.coord).map_err(|_| {
                                WorkbenchError::CellNotFound {
                                    doc_id: doc_id.clone(),
                                    sheet: sheet.clone(),
                                    coord: coord.clone(),
                                }
                            })?,
                            value_fingerprint: Fnv64.fingerprint(cell.repr.as_bytes()).0,
                            formula_fingerprint: cell
                                .formula
                                .as_deref()
                                .map(|f| Fnv64.fingerprint(f.as_bytes()).0),
                        }),
                        source_doc_id: Some(doc_id.clone()),
                        unit: spec.unit.clone(),
                    });
                }
                InputSource::Constant { value } => inputs.push(CalcInput {
                    name: spec.name.clone(),
                    value: *value,
                    source: None,
                    source_doc_id: None,
                    unit: spec.unit.clone(),
                }),
            }
        }
        let rec = CalculationRecord {
            calc_id: p.calc_id.clone(),
            inputs,
            expression: p.expression.clone(),
            unit: p.unit.clone(),
            precision: p.precision,
            intermediates: vec![],
            final_value: None,
            calc_status: mdc_evidence::CalcStatus::NotApplicable,
        };
        if !self.sidecar.add_calculation(rec, at_ms, operator) {
            return Err(WorkbenchError::DuplicateCalcId { calc_id: p.calc_id });
        }
        let record = self
            .sidecar
            .calculations
            .iter()
            .find(|c| c.calc_id == p.calc_id)
            .expect("上方刚插入");
        let outcome = mdc_evidence::evaluate(&record.expression, &record.inputs, &record.precision)
            .map_err(WorkbenchError::Eval)?;
        self.sidecar
            .apply_evaluation(&p.calc_id, outcome.clone(), at_ms, operator)?;
        self.save()?;
        Ok(outcome.value)
    }

    /// 复算（现值由程序从当前来源文件提取——Cell/常量两路）。
    pub fn recalc_current(
        &mut self,
        calc_id: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<RecalcOutcome, WorkbenchError> {
        let record = self
            .sidecar
            .calculations
            .iter()
            .find(|c| c.calc_id == calc_id)
            .ok_or_else(|| WorkbenchError::CalcNotFound {
                calc_id: calc_id.to_string(),
            })?
            .clone();
        let mut current = vec![];
        for input in &record.inputs {
            match (&input.source, &input.source_doc_id) {
                (Some(Locator::XlsxCell { sheet, cell, .. }), Some(doc_id)) => {
                    let reg = self.sources.iter().find(|s| s.doc_id == *doc_id);
                    let extracted = match reg {
                        Some(reg) => ingest::ingest_xlsx(&reg.path, doc_id).ok().and_then(|src| {
                            src.cell(sheet, cell.0.as_str()).and_then(|c| c.decimal)
                        }),
                        None => None,
                    };
                    current.push(match extracted {
                        Some(v) => CurrentInput::Available { value: v },
                        None => CurrentInput::Unavailable,
                    });
                }
                // 常量：回填登记值（无独立来源可验——正当性在审核面）
                _ => current.push(CurrentInput::Available { value: input.value }),
            }
        }
        let outcome = self
            .sidecar
            .recalculate_calc(calc_id, &current, at_ms, operator)?;
        self.save()?;
        Ok(outcome)
    }

    /// 来源侧显式重验（⑥恢复通道——§四.5 唯一路径）：重读当前来源
    /// 文件，以现值构造新定位器重绑。
    pub fn reverify_source(
        &mut self,
        evidence_id: &str,
        doc_id: &str,
        sheet: &str,
        coord: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        let (src, doc_ref) = self.fresh_source(doc_id)?;
        let cell = src.cell(sheet, coord).ok_or(WorkbenchError::CellNotFound {
            doc_id: doc_id.to_string(),
            sheet: sheet.to_string(),
            coord: coord.to_string(),
        })?;
        let locator = Locator::XlsxCell {
            sheet: cell.sheet.clone(),
            cell: mdc_evidence::CellCoord::parse(&cell.coord).map_err(|_| {
                WorkbenchError::CellNotFound {
                    doc_id: doc_id.to_string(),
                    sheet: sheet.to_string(),
                    coord: coord.to_string(),
                }
            })?,
            value_fingerprint: Fnv64.fingerprint(cell.repr.as_bytes()).0,
            formula_fingerprint: cell
                .formula
                .as_deref()
                .map(|f| Fnv64.fingerprint(f.as_bytes()).0),
        };
        self.sidecar
            .reverify_evidence(evidence_id, doc_ref, locator, at_ms, operator)?;
        self.save()?;
        Ok(())
    }

    /// ⑥ 正文改写失效检测（claim 锚；版本自增）。
    pub fn refresh_body(&mut self, at_ms: u64, operator: &str) -> Result<usize, WorkbenchError> {
        let body = self.current_body()?;
        self.body_version += 1;
        let new_ref = ingest::docx_body_ref(&self.body_doc_id, &body, self.body_version);
        let fps: Vec<String> = body
            .paragraphs
            .iter()
            .map(|p| p.fingerprint.0.clone())
            .collect();
        let degraded = self
            .sidecar
            .refresh_document(new_ref, &fps, at_ms, operator);
        self.save()?;
        Ok(degraded)
    }

    /// ⑦ 人工审核动作。
    pub fn review(
        &mut self,
        evidence_id: &str,
        to: ReviewStatus,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        self.sidecar
            .review_evidence(evidence_id, to, at_ms, operator)?;
        self.save()?;
        Ok(())
    }

    /// 幂等记录（S2 命令层预埋——会话内直用）。
    pub fn record_command(
        &mut self,
        key: IdempotencyKey,
        op_id: &str,
        outcome: mdc_evidence::CommandOutcome,
    ) -> Option<mdc_evidence::CommandOutcome> {
        self.sidecar.record_command(key, op_id, outcome)
    }

    /// ⑧ 干净导出：字节拷贝 + 双验证（字节同一 + 零标记）+ 审计。
    pub fn export_clean(
        &mut self,
        dest: &Path,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        let body_bytes = std::fs::read(&self.body_path)?;
        // 先验证后产出（§九.11——验证失败不产出已发布结果）：
        // 验证 1：模型可解析（Word/WPS 可开的最小代理）+ 零工作台标记。
        let doc = read_docx_from_bytes(&body_bytes).map_err(WorkbenchError::DocxRead)?;
        let mut texts: Vec<String> = vec![];
        for block in &doc.blocks {
            match block {
                mdc_office_io::docx::Block::Paragraph(p) => texts.push(p.text()),
                mdc_office_io::docx::Block::Table(t) => {
                    for row in &t.rows {
                        for cell in row {
                            texts.push(cell.clone());
                        }
                    }
                }
            }
        }
        for hf in doc.headers.iter().chain(doc.footers.iter()) {
            for block in &hf.blocks {
                if let mdc_office_io::docx::Block::Paragraph(p) = block {
                    texts.push(p.text());
                }
            }
        }
        for text in &texts {
            for marker in WORKBENCH_MARKERS {
                if text.contains(marker) {
                    return Err(WorkbenchError::MarkerFound {
                        marker: marker.to_string(),
                    });
                }
            }
        }
        // 验证全过后才落产物（临时文件 + rename 原子发布）。
        let tmp = dest.with_extension("export.tmp");
        std::fs::write(&tmp, &body_bytes)?;
        std::fs::rename(&tmp, dest)?;
        // 验证 2：字节同一（导出 = 正文原样，零变换）。
        let exported = std::fs::read(dest)?;
        if exported != body_bytes {
            return Err(WorkbenchError::ExportByteMismatch);
        }
        self.sidecar.push_audit_public(
            "clean_export",
            serde_json::json!({"bytes": body_bytes.len(), "markers_checked": WORKBENCH_MARKERS.len()}),
            at_ms,
            operator,
        );
        self.save()?;
        Ok(())
    }
}

/// 十进制串去尾零形（`4210.00` → `4210`；`4210.500` → `4210.5`）。
fn trim_trailing_zeros(s: &str) -> &str {
    if let Some(dot) = s.find('.') {
        let t = s.trim_end_matches('0');
        let t = t.strip_suffix('.').unwrap_or(t);
        // 保持至少 "0.x" 形
        if t.len() < dot + 1 {
            return &s[..dot + 2.min(s.len())];
        }
        t
    } else {
        s
    }
}
