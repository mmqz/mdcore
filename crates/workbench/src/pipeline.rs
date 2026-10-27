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
    CalcStatus, CalculationRecord, CommandEnvelope, CommandOutcome, CurrentInput, Decimal,
    DocumentRef, EvalError, EvidenceRecord, Fingerprinter, Fnv64, IdempotencyKey, LinkageStatus,
    Locator, RecalcOutcome, RelationKind, ReviewStatus, Sidecar, SidecarError, SidecarStore,
    VersionStamp,
};
use mdc_office_io::docx::{read_docx_from_bytes, DocxError};
use mdc_office_io::io::ReadError;

use crate::ingest::{self, DocxBody, XlsxSource};
use crate::WORKBENCH_MARKERS;
use mdc_evidence::{CalcInput, PrecisionRule};
use std::collections::HashMap;

/// 来源注册项（doc_id ↔ 路径[文件会话] + 当前版本；内存会话 path =
/// None——字节在 [`Io::Memory::sources`]）。
#[derive(Debug, Clone)]
struct SourceRegistration {
    doc_id: String,
    path: Option<PathBuf>,
    version: u64,
}

/// 会话 IO 后端（p5-t3：文件 = 原子落盘；内存 = wasm/无文件系统）。
/// 两后端共享同一验证/级联/审计核心（[`Workbench`] 各步方法）——
/// 仅 IO 边界分派，不重写任何判定逻辑。
enum Io {
    Files {
        policy: crate::security::PathPolicy,
        store: SidecarStore,
        body_path: PathBuf,
    },
    Memory {
        body: Vec<u8>,
        sources: HashMap<String, Vec<u8>>,
    },
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
    /// 审核门槛拒绝（阶段 2——操作者不在允许列表 / 失效关联过审 /
    /// 高值主张未过复算；拒绝 = 零状态变化）。
    ReviewGate {
        evidence_id: String,
        reason: String,
    },
    /// 路径允许列表拒绝（S2——分发面映射 Rejected）。
    PathPolicy(crate::security::PolicyError),
    /// 规模限制拒绝（S2——条数/字节门）。
    Limit(String),
    /// 序列化失败。
    Serialize(serde_json::Error),
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
            WorkbenchError::ReviewGate {
                evidence_id,
                reason,
            } => {
                write!(f, "review gate rejected [{evidence_id}]: {reason}")
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
            WorkbenchError::PathPolicy(e) => write!(f, "path policy: {e}"),
            WorkbenchError::Limit(r) => write!(f, "limit: {r}"),
            WorkbenchError::Serialize(e) => write!(f, "serialize: {e}"),
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

/// 八步闭环会话（S2 起：全部文件面经 `policy` 允许列表；p5-t3 起内存
/// 会话 [wasm/无文件系统] 与文件会话共享同一验证核心）。
pub struct Workbench {
    sidecar: Sidecar,
    io: Io,
    body_doc_id: String,
    body_version: u64,
    sources: Vec<SourceRegistration>,
    /// 审核门槛（阶段 2——会话级策略，默认严格档；见 security::ReviewPolicy）。
    review_policy: crate::security::ReviewPolicy,
}

impl Workbench {
    /// ②登记 DOCX 正文（建 sidecar；正文指纹 = 登记引用）。
    /// S2：sidecar/正文/来源/导出全部路径必须落在 `policy` 允许根内。
    pub fn start(
        policy: crate::security::PathPolicy,
        sidecar_path: &Path,
        body_path: &Path,
        body_doc_id: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<Workbench, WorkbenchError> {
        policy
            .check_dest(sidecar_path)
            .map_err(WorkbenchError::PathPolicy)?;
        policy
            .check(body_path)
            .map_err(WorkbenchError::PathPolicy)?;
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
        let json = serde_json::to_string_pretty(&sidecar).map_err(WorkbenchError::Serialize)?;
        crate::security::check_sidecar_size(json.len()).map_err(WorkbenchError::Limit)?;
        crate::security::publish(store.path(), json.as_bytes())?;
        Ok(Workbench {
            sidecar,
            io: Io::Files {
                policy,
                store,
                body_path: body_path.to_path_buf(),
            },
            body_doc_id: body_doc_id.to_string(),
            body_version: 1,
            sources: vec![],
            review_policy: crate::security::ReviewPolicy::default(),
        })
    }

    /// ②登记 DOCX 正文（内存会话——p5-t3 wasm 面：零 std::fs，
    /// sidecar 恒驻内存，持久化由调用方经 [`Self::sidecar_json`] 自取）。
    pub fn start_memory(
        body_bytes: &[u8],
        body_doc_id: &str,
        at_ms: u64,
        operator: &str,
    ) -> Result<Workbench, WorkbenchError> {
        let body = ingest::ingest_docx_bytes(body_bytes)?;
        let body_ref = ingest::docx_body_ref(body_doc_id, &body, 1);
        let mut sidecar = Sidecar::new(body_ref);
        sidecar.push_audit_public(
            "workbench_start",
            serde_json::json!({
                "body_doc_id": body_doc_id,
                "io": "memory",
            }),
            at_ms,
            operator,
        );
        Ok(Workbench {
            sidecar,
            io: Io::Memory {
                body: body_bytes.to_vec(),
                sources: HashMap::new(),
            },
            body_doc_id: body_doc_id.to_string(),
            body_version: 1,
            sources: vec![],
            review_policy: crate::security::ReviewPolicy::default(),
        })
    }

    /// 重开会话（路径由调用方重供——路径不入 sidecar；policy 校验）。
    ///
    /// 恢复保真：`body_doc_id`/`body_version`/来源版本全部从 sidecar
    /// 登记（`sidecar.document` 与 `sidecar.source_docs`）恢复——绝不
    /// 硬编码重启值，否则重开后 `refresh_body`/`ingest_source` 会造成
    /// 版本回退（与历史登记碰撞）与正文 doc_id 置空。
    pub fn open(
        policy: crate::security::PathPolicy,
        sidecar_path: &Path,
        body_path: &Path,
        source_paths: &[(String, PathBuf)],
    ) -> Result<Workbench, WorkbenchError> {
        policy
            .check_dest(sidecar_path)
            .map_err(WorkbenchError::PathPolicy)?;
        policy
            .check(body_path)
            .map_err(WorkbenchError::PathPolicy)?;
        let store = SidecarStore::new(sidecar_path);
        let sidecar = store
            .load()?
            .ok_or(SidecarError::Corrupt("sidecar not found".into()))?;
        // 正文身份与版本从 sidecar 恢复（单一真源）。
        let body_doc_id = sidecar.document.doc_id.as_str().to_string();
        let body_version = sidecar.document.version.0;
        // 来源版本从 sidecar 注册表恢复：同 doc_id 未登记过的默认 1
        // （首次 ingest 才入册）。
        let sources = source_paths
            .iter()
            .map(|(id, p)| {
                let version = sidecar
                    .source_docs
                    .iter()
                    .find(|d| d.doc_id.as_str() == id.as_str())
                    .map(|d| d.version.0)
                    .unwrap_or(1);
                SourceRegistration {
                    doc_id: id.clone(),
                    path: Some(p.clone()),
                    version,
                }
            })
            .collect();
        Ok(Workbench {
            sidecar,
            io: Io::Files {
                policy,
                store,
                body_path: body_path.to_path_buf(),
            },
            body_doc_id,
            body_version,
            sources,
            review_policy: crate::security::ReviewPolicy::default(),
        })
    }

    /// 重开内存会话（p5-t3 wasm 面：sidecar JSON + 正文/来源字节由
    /// 调用方重供——路径不入 sidecar 纪律天然成立；恢复保真语义与
    /// [`Self::open`] 同源——身份/版本全部从 sidecar 登记）。
    pub fn open_memory(
        sidecar_json: &str,
        body_bytes: &[u8],
        source_bytes: &[(String, Vec<u8>)],
    ) -> Result<Workbench, WorkbenchError> {
        let sidecar: Sidecar = serde_json::from_str(sidecar_json).map_err(|_| {
            WorkbenchError::Sidecar(SidecarError::Corrupt("sidecar json parse failed".into()))
        })?;
        if sidecar.schema_version != 1 {
            return Err(WorkbenchError::Sidecar(SidecarError::Corrupt(format!(
                "schema version {} not supported",
                sidecar.schema_version
            ))));
        }
        let body_doc_id = sidecar.document.doc_id.as_str().to_string();
        let body_version = sidecar.document.version.0;
        // 正文指纹 fail-closed：重供字节必须与 sidecar 登记引用一致
        // （与 open 后首个 body 面调用同纪律——错配即拒，不静默）。
        let body = ingest::ingest_docx_bytes(body_bytes)?;
        if body.fingerprint != sidecar.document.fingerprint {
            return Err(WorkbenchError::BodyStale);
        }
        let mut map = HashMap::new();
        let sources = source_bytes
            .iter()
            .map(|(id, bytes)| {
                let version = sidecar
                    .source_docs
                    .iter()
                    .find(|d| d.doc_id.as_str() == id.as_str())
                    .map(|d| d.version.0)
                    .unwrap_or(1);
                map.insert(id.clone(), bytes.clone());
                SourceRegistration {
                    doc_id: id.clone(),
                    path: None,
                    version,
                }
            })
            .collect();
        Ok(Workbench {
            sidecar,
            io: Io::Memory {
                body: body_bytes.to_vec(),
                sources: map,
            },
            body_doc_id,
            body_version,
            sources,
            review_policy: crate::security::ReviewPolicy::default(),
        })
    }

    pub fn sidecar(&self) -> &Sidecar {
        &self.sidecar
    }

    /// 直接访问（高级面——变更后请自行 save；常规操作走各步方法）。
    pub fn sidecar_mut(&mut self) -> &mut Sidecar {
        &mut self.sidecar
    }

    // —— S2 命令层 crate 内辅助（command.rs 用）——

    pub(crate) fn lookup_command(
        &self,
        key: &mdc_evidence::IdempotencyKey,
    ) -> Option<CommandOutcome> {
        self.sidecar
            .idempotency
            .iter()
            .find(|e| e.key == *key)
            .map(|e| e.outcome.clone())
    }

    pub(crate) fn audit_replay(&mut self, envelope: &CommandEnvelope, at_ms: u64) {
        self.sidecar.push_audit_public(
            "command_replayed",
            serde_json::json!({
                "op_id": envelope.op_id,
                "idempotency_key": envelope.idempotency_key.0,
            }),
            at_ms,
            &envelope.operator,
        );
    }

    pub(crate) fn body_version_stamp(&self) -> VersionStamp {
        self.sidecar.document.version
    }

    pub(crate) fn has_any_records(&self) -> bool {
        !self.sidecar.evidence.is_empty() || !self.sidecar.calculations.is_empty()
    }

    pub(crate) fn store_command_outcome(
        &mut self,
        envelope: &CommandEnvelope,
        outcome: CommandOutcome,
    ) {
        let _ =
            self.sidecar
                .record_command(envelope.idempotency_key.clone(), &envelope.op_id, outcome);
    }

    /// 落盘（发布单点同 `save`——命令层入口用）。
    pub(crate) fn persist(&self) -> Result<(), WorkbenchError> {
        self.save()
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

    /// 落盘（S2 发布单点：序列化 → 字节门 → 临时区+rename 原子发布；
    /// 内存会话 = 字节门照跑 + 零落盘——sidecar 恒驻内存，持久化由
    /// 调用方经 [`Self::sidecar_json`] 自取）。
    fn save(&self) -> Result<(), WorkbenchError> {
        let json =
            serde_json::to_string_pretty(&self.sidecar).map_err(WorkbenchError::Serialize)?;
        crate::security::check_sidecar_size(json.len()).map_err(WorkbenchError::Limit)?;
        if let Io::Files { store, .. } = &self.io {
            crate::security::publish(store.path(), json.as_bytes())?;
        }
        Ok(())
    }

    /// 内存会话持久化面（单点投影——无隐藏中间态；与磁盘 sidecar
    /// 字节同构[serde 原生形]，原生端 [`Self::open`] 可直接消费）。
    pub fn sidecar_json(&self) -> Result<String, WorkbenchError> {
        serde_json::to_string_pretty(&self.sidecar).map_err(WorkbenchError::Serialize)
    }

    /// restore 专用附源（p5-t3 wasm 面：`open_memory` 后逐源补字节——
    /// 只填内存字节表与注册表，**不动 sidecar/不产审计/不 bump 版本**；
    /// 版本以 sidecar.source_docs 登记为准[未登记默认 1]——恢复语义
    /// 与 `open_memory` 的列表构造面同源）。幂等：同 doc_id 重复附 =
    /// 字节覆盖。仅内存会话。
    pub fn attach_source_bytes(
        &mut self,
        doc_id: &str,
        bytes: &[u8],
    ) -> Result<(), WorkbenchError> {
        let Io::Memory { sources, .. } = &mut self.io else {
            return Err(WorkbenchError::Sidecar(SidecarError::Corrupt(
                "attach_source_bytes on file session".into(),
            )));
        };
        sources.insert(doc_id.to_string(), bytes.to_vec());
        let version = self
            .sidecar
            .source_docs
            .iter()
            .find(|d| d.doc_id.as_str() == doc_id)
            .map(|d| d.version.0)
            .unwrap_or(1);
        match self.sources.iter_mut().find(|s| s.doc_id == doc_id) {
            Some(reg) => reg.version = version,
            None => self.sources.push(SourceRegistration {
                doc_id: doc_id.to_string(),
                path: None,
                version,
            }),
        }
        Ok(())
    }

    /// 当前正文段落面（p5-t3 wasm 面：证据侧栏 claim 锚跳转数据源）。
    pub fn current_paragraphs(&self) -> Result<Vec<ingest::ParagraphInfo>, WorkbenchError> {
        Ok(self.current_body()?.paragraphs)
    }

    fn current_body(&self) -> Result<DocxBody, WorkbenchError> {
        match &self.io {
            Io::Files { body_path, .. } => ingest::ingest_docx(body_path),
            Io::Memory { body, .. } => ingest::ingest_docx_bytes(body),
        }
    }

    /// 当前正文字节（导出面/内存刷新面共用）。
    fn body_bytes(&self) -> Result<Vec<u8>, WorkbenchError> {
        match &self.io {
            Io::Files { body_path, .. } => std::fs::read(body_path).map_err(WorkbenchError::Io),
            Io::Memory { body, .. } => Ok(body.clone()),
        }
    }

    /// 重读来源（两后端同语义：策略检查仅文件会话——内存会话无路径面）。
    fn load_source(&self, doc_id: &str) -> Result<XlsxSource, WorkbenchError> {
        let reg = self
            .sources
            .iter()
            .find(|s| s.doc_id == doc_id)
            .ok_or_else(|| WorkbenchError::SourceNotRegistered {
                doc_id: doc_id.to_string(),
            })?;
        match &self.io {
            Io::Files { policy, .. } => {
                let path = reg
                    .path
                    .as_ref()
                    .ok_or(WorkbenchError::SourceNotRegistered {
                        doc_id: doc_id.to_string(),
                    })?;
                policy.check(path).map_err(WorkbenchError::PathPolicy)?;
                ingest::ingest_xlsx(path, doc_id)
            }
            Io::Memory { sources, .. } => {
                let bytes =
                    sources
                        .get(doc_id)
                        .ok_or_else(|| WorkbenchError::SourceNotRegistered {
                            doc_id: doc_id.to_string(),
                        })?;
                ingest::ingest_xlsx_bytes(bytes, doc_id)
            }
        }
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
        let src = match &self.io {
            Io::Files { policy, .. } => {
                policy.check(path).map_err(WorkbenchError::PathPolicy)?;
                ingest::ingest_xlsx(path, doc_id)?
            }
            Io::Memory { .. } => {
                return Err(WorkbenchError::Sidecar(SidecarError::Corrupt(
                    "file ingest on memory session — use ingest_source_bytes".into(),
                )));
            }
        };
        let reg_path = Some(path.to_path_buf());
        self.ingest_source_inner(doc_id, src, reg_path, at_ms, operator)
    }

    /// ①/⑥ 导入或刷新 XLSX 来源（内存会话——字节替换同 doc_id，
    /// 语义与路径版同源：指纹变化自动触发证据失效，版本自增）。
    pub fn ingest_source_bytes(
        &mut self,
        doc_id: &str,
        bytes: &[u8],
        at_ms: u64,
        operator: &str,
    ) -> Result<DocumentRef, WorkbenchError> {
        if !matches!(self.io, Io::Memory { .. }) {
            return Err(WorkbenchError::Sidecar(SidecarError::Corrupt(
                "byte ingest on file session — use ingest_source".into(),
            )));
        }
        let src = ingest::ingest_xlsx_bytes(bytes, doc_id)?;
        if let Io::Memory { sources, .. } = &mut self.io {
            sources.insert(doc_id.to_string(), bytes.to_vec());
        }
        self.ingest_source_inner(doc_id, src, None, at_ms, operator)
    }

    /// 摄取核心（两入口共享：版本对账 + 注册 + sidecar 登记 + save）。
    fn ingest_source_inner(
        &mut self,
        doc_id: &str,
        src: XlsxSource,
        reg_path: Option<PathBuf>,
        at_ms: u64,
        operator: &str,
    ) -> Result<DocumentRef, WorkbenchError> {
        let version = match self.sources.iter().find(|s| s.doc_id == doc_id) {
            Some(reg) => reg.version + 1,
            None => 1,
        };
        let doc_ref = ingest::xlsx_source_ref(&src, version);
        self.sidecar
            .register_source_doc(doc_ref.clone(), at_ms, operator);
        match self.sources.iter_mut().find(|s| s.doc_id == doc_id) {
            Some(reg) => {
                reg.path = reg_path;
                reg.version = version;
            }
            None => self.sources.push(SourceRegistration {
                doc_id: doc_id.to_string(),
                path: reg_path,
                version,
            }),
        }
        self.save()?;
        Ok(doc_ref)
    }

    /// 来源 fail-closed 检查：当前源指纹 == 注册引用（返回摄取产物）。
    fn fresh_source(&self, doc_id: &str) -> Result<(XlsxSource, DocumentRef), WorkbenchError> {
        let reg = self
            .sources
            .iter()
            .find(|s| s.doc_id == doc_id)
            .ok_or_else(|| WorkbenchError::SourceNotRegistered {
                doc_id: doc_id.to_string(),
            })?;
        let src = self.load_source(doc_id)?;
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
        crate::security::check_counts(
            self.sidecar.evidence.len(),
            self.sidecar.calculations.len(),
            true,
            false,
        )
        .map_err(WorkbenchError::Limit)?;
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
        crate::security::check_counts(
            self.sidecar.evidence.len(),
            self.sidecar.calculations.len(),
            false,
            true,
        )
        .map_err(WorkbenchError::Limit)?;
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
                    // 重读现值（两后端同语义：注册源重读——内存会话
                    // 无路径复查面，重读失败 fail-closed 级联 Unavailable）。
                    let extracted = self
                        .load_source(doc_id)
                        .ok()
                        .and_then(|src| src.cell(sheet, cell.0.as_str()).and_then(|c| c.decimal));
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

    /// 主张侧显式重验（⑥恢复通道——claim 锚）：新段落位置 + 当前
    /// 正文（fail-closed：当前正文指纹必须 == sidecar 登记引用，
    /// 即 refresh_body 已先行）。
    pub fn reverify_claim(
        &mut self,
        evidence_id: &str,
        paragraph_index: u32,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        let body = self.current_body()?;
        self.check_body_fresh(&body)?;
        let para = body
            .paragraph(paragraph_index)
            .ok_or(WorkbenchError::ParagraphNotFound {
                index: paragraph_index,
            })?;
        self.sidecar.reverify_claim(
            evidence_id,
            self.sidecar.document.clone(),
            paragraph_index,
            para.fingerprint.0.clone(),
            at_ms,
            operator,
        )?;
        self.save()?;
        Ok(())
    }

    /// ⑥ 正文改写失效检测（claim 锚；版本自增）。
    pub fn refresh_body(&mut self, at_ms: u64, operator: &str) -> Result<usize, WorkbenchError> {
        self.refresh_body_inner(at_ms, operator)
    }

    /// ⑥ 正文改写失效检测（内存会话：宿主供新正文字节——「文件已
    /// 变」的内存等价物；指纹对照/级联/版本自增语义同源）。
    pub fn refresh_body_bytes(
        &mut self,
        new_body_bytes: &[u8],
        at_ms: u64,
        operator: &str,
    ) -> Result<usize, WorkbenchError> {
        if !matches!(self.io, Io::Memory { .. }) {
            return Err(WorkbenchError::Sidecar(SidecarError::Corrupt(
                "byte refresh on file session — use refresh_body".into(),
            )));
        }
        // 先验证新字节可解析（fail-closed：不可解析即拒，会话不动）。
        let _ = ingest::ingest_docx_bytes(new_body_bytes)?;
        if let Io::Memory { body, .. } = &mut self.io {
            *body = new_body_bytes.to_vec();
        }
        self.refresh_body_inner(at_ms, operator)
    }

    fn refresh_body_inner(&mut self, at_ms: u64, operator: &str) -> Result<usize, WorkbenchError> {
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

    /// ⑦ 人工审核动作（阶段 2 起经 [`Self::check_review_gate`] 门槛——
    /// 拒绝 = 零状态变化、sidecar 零落盘）。
    pub fn review(
        &mut self,
        evidence_id: &str,
        to: ReviewStatus,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        self.check_review_gate(evidence_id, to, operator)?;
        self.sidecar
            .review_evidence(evidence_id, to, at_ms, operator)?;
        self.save()?;
        Ok(())
    }

    /// 审核门槛检查（计划 §五「不同业务场景可配置的审核门槛」）。
    ///
    /// 三维 fail-closed（配置见 [`crate::security::ReviewPolicy`]）：
    /// 操作者前缀允许列表（默认 `human:`——AI 不可改审核态）；
    /// 过审要求关联 Valid（除非显式放开失效过审）；主张值达门槛的
    /// 计算型证据要求计算面 Passed。
    fn check_review_gate(
        &self,
        evidence_id: &str,
        to: ReviewStatus,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        let gate = |reason: String| WorkbenchError::ReviewGate {
            evidence_id: evidence_id.to_string(),
            reason,
        };
        if !self.review_policy.operator_allowed(operator) {
            return Err(gate(format!(
                "operator '{operator}' not in reviewer allowlist"
            )));
        }
        if to != ReviewStatus::Approved {
            return Ok(()); // 拒绝/审核面其余方向不受关联/计算门槛辖
        }
        let ev = self
            .sidecar
            .evidence
            .iter()
            .find(|e| e.evidence_id == evidence_id)
            .ok_or(WorkbenchError::Sidecar(SidecarError::EvidenceNotFound))?;
        if !self.review_policy.allow_approval_of_broken_linkage
            && ev.linkage != LinkageStatus::Valid
        {
            return Err(gate(format!(
                "linkage is {:?} (not Valid) — reverify before approval",
                ev.linkage
            )));
        }
        if let Some(thr) = self.review_policy.calc_required_at_or_above {
            let high_value = ev
                .claimed_value
                .as_ref()
                .map(|v| *v >= thr)
                .unwrap_or(false);
            if ev.calc_ref.is_some() && high_value && ev.calc != CalcStatus::Passed {
                return Err(gate(format!(
                    "claimed value at/above {thr} requires Passed calc (current: {:?})",
                    ev.calc
                )));
            }
        }
        Ok(())
    }

    /// 审核门槛策略注入（会话级——不同业务场景不同档；不入 sidecar）。
    pub fn set_review_policy(&mut self, policy: crate::security::ReviewPolicy) {
        self.review_policy = policy;
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
        // 次序（审查 M1 修复）：check_dest 前置 → 验证 → 产物落地 →
        // 审计——拒绝/写失败/验证失败路径零状态变化（审计零记录、
        // sidecar 零落盘，与 review 门拒绝同纪律）。
        if let Io::Files { policy, .. } = &self.io {
            policy
                .check_dest(dest)
                .map_err(WorkbenchError::PathPolicy)?;
            let bytes = self.export_clean_verified()?;
            // 先验证后产出（§九.11）：临时文件 + rename 原子发布。
            let tmp = dest.with_extension("export.tmp");
            std::fs::write(&tmp, &bytes).map_err(WorkbenchError::Io)?;
            std::fs::rename(&tmp, dest).map_err(WorkbenchError::Io)?;
            // 验证 2（文件会话）：字节同一（导出 = 正文原样，零变换）。
            let exported = std::fs::read(dest).map_err(WorkbenchError::Io)?;
            if exported != bytes {
                return Err(WorkbenchError::ExportByteMismatch);
            }
        }
        // 产物已落地才审计。
        self.record_clean_export(at_ms, operator)
    }

    /// ⑧-bis 暗标匿名导出（G3 / R-11 BLOCKER：元数据级清零在原子
    /// 发布字节边界内完成，无「先发布再补丁」窗口）。
    ///
    /// 次序与 [`Self::export_clean`] 同族：check_dest 前置 → 验证
    /// （源过零标记门 → `anonymize_docx_props` → 匿名产物可解析 +
    /// 正文部件逐字节同源）→ 临时区 + rename 原子发布 → 审计。
    /// 拒绝/写失败/验证失败路径零状态变化（与干净导出同纪律）。
    pub fn export_anonymous(
        &mut self,
        dest: &Path,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        if let Io::Files { policy, .. } = &self.io {
            policy
                .check_dest(dest)
                .map_err(WorkbenchError::PathPolicy)?;
            let bytes = self.export_anonymous_verified()?;
            let tmp = dest.with_extension("export-anon.tmp");
            std::fs::write(&tmp, &bytes).map_err(WorkbenchError::Io)?;
            std::fs::rename(&tmp, dest).map_err(WorkbenchError::Io)?;
            // 验证 2（文件会话）：落地产物读回可解析（匿名化非破坏性
            // 代理——字节同一验证不适用：docProps 面字节必然变化）。
            let exported = std::fs::read(dest).map_err(WorkbenchError::Io)?;
            read_docx_from_bytes(&exported).map_err(WorkbenchError::DocxRead)?;
        }
        self.record_anonymous_export(at_ms, operator)
    }

    /// ⑧-bis 匿名导出（字节返回版——内存会话/宿主自定落点共用）。
    pub fn export_anonymous_bytes(
        &mut self,
        at_ms: u64,
        operator: &str,
    ) -> Result<Vec<u8>, WorkbenchError> {
        let bytes = self.export_anonymous_verified()?;
        self.record_anonymous_export(at_ms, operator)?;
        Ok(bytes)
    }

    /// 匿名导出验证核心（零副作用）：源先过干净导出全门（可解析 +
    /// 零标记——复用 [`Self::export_clean_verified`]），再匿名化，
    /// 匿名产物双验证（可解析 + 正文部件与源逐字节同）。
    fn export_anonymous_verified(&self) -> Result<Vec<u8>, WorkbenchError> {
        let body_bytes = self.export_clean_verified()?;
        let anon = mdc_office_io::docx::anonymize_docx_props(&body_bytes)
            .map_err(WorkbenchError::DocxRead)?;
        read_docx_from_bytes(&anon).map_err(WorkbenchError::DocxRead)?;
        let src_doc = mdc_office_io::docx::docx_part_bytes(&body_bytes, "word/document.xml");
        let out_doc = mdc_office_io::docx::docx_part_bytes(&anon, "word/document.xml");
        if src_doc.is_none() || src_doc != out_doc {
            return Err(WorkbenchError::ExportByteMismatch);
        }
        Ok(anon)
    }

    /// ⑧ 干净导出（字节返回版——内存会话/宿主自定落点共用）。
    /// 返回字节即产物交付（先审计合理）；零标记验证照跑。
    pub fn export_clean_bytes(
        &mut self,
        at_ms: u64,
        operator: &str,
    ) -> Result<Vec<u8>, WorkbenchError> {
        let body_bytes = self.export_clean_verified()?;
        self.record_clean_export(at_ms, operator)?;
        Ok(body_bytes)
    }

    /// 验证核心（零副作用：读正文 + 解析 + 零标记扫描；不审计不
    /// 落盘——两入口共享，审查 M1 拆分）。
    fn export_clean_verified(&self) -> Result<Vec<u8>, WorkbenchError> {
        let body_bytes = self.body_bytes()?;
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
                // #18：文本框内层块递归入文本流（验证面——全文本收集）。
                mdc_office_io::docx::Block::TextBox(tb) => {
                    for inner in &tb.blocks {
                        match inner {
                            mdc_office_io::docx::Block::Paragraph(p) => texts.push(p.text()),
                            mdc_office_io::docx::Block::Table(t) => {
                                for row in &t.rows {
                                    for cell in row {
                                        texts.push(cell.clone());
                                    }
                                }
                            }
                            mdc_office_io::docx::Block::TextBox(_) => {}
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
        Ok(body_bytes)
    }

    /// 导出审计 + 落盘（产物落地后调用——拒绝路径零记录）。
    fn record_clean_export(&mut self, at_ms: u64, operator: &str) -> Result<(), WorkbenchError> {
        self.sidecar.push_audit_public(
            "clean_export",
            serde_json::json!({"markers_checked": WORKBENCH_MARKERS.len()}),
            at_ms,
            operator,
        );
        self.save()
    }

    fn record_anonymous_export(
        &mut self,
        at_ms: u64,
        operator: &str,
    ) -> Result<(), WorkbenchError> {
        self.sidecar.push_audit_public(
            "anonymous_export",
            serde_json::json!({
                "markers_checked": WORKBENCH_MARKERS.len(),
                "scrub_targets": [
                    "docProps/core.xml", "docProps/app.xml", "docProps/custom.xml",
                    "word/comments.xml", "word/people.xml"
                ],
            }),
            at_ms,
            operator,
        );
        self.save()
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
