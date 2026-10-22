//! 写入安全（S2——用户修订计划：路径允许列表 / 规模限制 / 发布纪律）。
//!
//! # 路径允许列表（`PathPolicy`）
//!
//! 全部文件操作（读来源/正文、写 sidecar/导出）必须落在**允许根
//! 目录**内。canonicalize 解析符号链接——链接指向根外 = 拒绝
//!（逃逸防护）。`allow_all` 仅供测试/嵌入式宿主自担信任面（产品
//! 面应始终构造受限 policy）。
//!
//! # 规模限制（fail-closed，与 SECURITY_POLICY 限额表同纪律）
//!
//! - 证据/计算记录条数上限（结构性防 sidecar 无界增长）；
//! - sidecar 序列化字节上限（超限拒绝**发布**——磁盘上旧文件不动）；
//! - 时间面 = 结构性有界：输入字节门（office-io ⓪）+ 表达式资源
//!   四门（s1-r2）+ 条数门（本模块）——**不设墙钟计时器**（非确定
//!   性且 wasm 面不可移植；有界输入 ⇒ 有界时间，决策登记于 S2
//!   报告）。
//!
//! # 发布纪律（临时区生成后发布）
//!
//! [`publish`]：同目录临时文件 + rename（POSIX 原子）——所有写
//! 产物（sidecar / 导出）经此单点；失败不留半成品，旧文件不动。

use std::io;
use std::path::{Path, PathBuf};

/// 每文档 sidecar 证据条数上限。
pub const MAX_EVIDENCE: usize = 1000;
/// 每文档 sidecar 计算记录条数上限。
pub const MAX_CALCULATIONS: usize = 500;
/// sidecar 序列化字节上限（超限拒绝发布）。
pub const MAX_SIDECAR_BYTES: usize = 16 * 1024 * 1024;

/// 路径策略错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// 允许根本身不可用（不存在/不可达——fail-closed）。
    RootUnavailable { root: PathBuf },
    /// 目标路径不在任何允许根内（含符号链接逃逸）。
    NotUnderAnyRoot { path: PathBuf },
    /// 目标父目录不存在（写面）。
    ParentUnavailable { path: PathBuf },
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::RootUnavailable { root } => {
                write!(f, "allowlist root unavailable: {}", root.display())
            }
            PolicyError::NotUnderAnyRoot { path } => {
                write!(f, "path outside allowlist roots: {}", path.display())
            }
            PolicyError::ParentUnavailable { path } => {
                write!(f, "destination parent unavailable: {}", path.display())
            }
        }
    }
}
impl std::error::Error for PolicyError {}

/// 路径允许列表。
#[derive(Debug, Clone)]
pub struct PathPolicy {
    /// 已 canonicalize 的允许根（空 = allow_all 面）。
    roots: Option<Vec<PathBuf>>,
}

impl PathPolicy {
    /// 受限策略：全部根必须已存在且可 canonicalize（fail-closed）。
    pub fn new(roots: &[PathBuf]) -> Result<Self, PolicyError> {
        let mut canonical = vec![];
        for r in roots {
            let c = std::fs::canonicalize(r)
                .map_err(|_| PolicyError::RootUnavailable { root: r.clone() })?;
            canonical.push(c);
        }
        Ok(PathPolicy {
            roots: Some(canonical),
        })
    }

    /// 不受限（测试/嵌入式宿主自担信任——产品面勿用）。
    pub fn allow_all() -> Self {
        PathPolicy { roots: None }
    }

    pub fn is_unrestricted(&self) -> bool {
        self.roots.is_none()
    }

    fn under_roots(&self, canonical: &Path) -> bool {
        match &self.roots {
            None => true,
            Some(roots) => roots.iter().any(|r| canonical.starts_with(r)),
        }
    }

    /// 读面检查：文件必须存在（canonicalize 成功）且在根内。
    pub fn check(&self, path: &Path) -> Result<PathBuf, PolicyError> {
        if self.is_unrestricted() {
            return Ok(path.to_path_buf());
        }
        let c = std::fs::canonicalize(path).map_err(|_| PolicyError::ParentUnavailable {
            path: path.to_path_buf(),
        })?;
        if !self.under_roots(&c) {
            return Err(PolicyError::NotUnderAnyRoot { path: c });
        }
        Ok(c)
    }

    /// 写面检查：父目录必须存在且在根内（目标文件可不存在）。
    pub fn check_dest(&self, path: &Path) -> Result<PathBuf, PolicyError> {
        if self.is_unrestricted() {
            return Ok(path.to_path_buf());
        }
        let parent = path.parent().ok_or(PolicyError::ParentUnavailable {
            path: path.to_path_buf(),
        })?;
        let pc = std::fs::canonicalize(parent).map_err(|_| PolicyError::ParentUnavailable {
            path: path.to_path_buf(),
        })?;
        if !self.under_roots(&pc) {
            return Err(PolicyError::NotUnderAnyRoot { path: pc });
        }
        let file = path.file_name().ok_or(PolicyError::ParentUnavailable {
            path: path.to_path_buf(),
        })?;
        Ok(pc.join(file))
    }
}

/// 规模检查（条数门——插入前调用）。
pub fn check_counts(
    evidence: usize,
    calculations: usize,
    adding_evidence: bool,
    adding_calc: bool,
) -> Result<(), String> {
    if adding_evidence && evidence >= MAX_EVIDENCE {
        return Err(format!("evidence count over limit {MAX_EVIDENCE}"));
    }
    if adding_calc && calculations >= MAX_CALCULATIONS {
        return Err(format!("calculation count over limit {MAX_CALCULATIONS}"));
    }
    Ok(())
}

/// 序列化字节门（发布前调用——超限拒绝写盘，旧文件不动）。
pub fn check_sidecar_size(json_len: usize) -> Result<(), String> {
    if json_len > MAX_SIDECAR_BYTES {
        return Err(format!(
            "sidecar serialized size over limit {MAX_SIDECAR_BYTES} bytes ({json_len})"
        ));
    }
    Ok(())
}

/// 发布单点：同目录临时文件 + rename（POSIX 原子；失败不留半成品）。
pub fn publish(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = dest.with_extension("publish.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 审核门槛（阶段 2——计划 §五「不同业务场景可配置的审核门槛」）
// ---------------------------------------------------------------------------

/// 审核门槛策略：会话级注入（宿主按业务场景配置），**不入 sidecar**
/// ——sidecar 只承载事实与审计，策略属调用方信任面（配置漂移不改
/// 历史数据语义）。全部维度 fail-closed：不配置 = 严格默认档。
///
/// 三个业务差异维度（增量最小面——覆盖「谁可审 / 失效可否过审 /
/// 高值需复算」三类门槛；更多维度按真实业务场景再加）：
/// 1. `reviewer_prefixes`：审核动作的操作者前缀允许列表
///    （默认 `["human:"]`——AI 不可改审核态，架构原则「AI 只提关联
///    与推导」的门面化）；
/// 2. `allow_approval_of_broken_linkage`：关联失效（Stale/Missing/
///    Ambiguous）时是否允许直接过审（默认 false——过审前必须显式
///    重验恢复 Valid）；
/// 3. `calc_required_at_or_above`：主张值门槛——达到即要求计算面
///    Passed 才可过审（默认 None 不设；仅对带 `calc_ref` 的计算型
///    主张生效——引用型主张无计算面可依）。
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewPolicy {
    pub reviewer_prefixes: Vec<String>,
    pub allow_approval_of_broken_linkage: bool,
    pub calc_required_at_or_above: Option<mdc_evidence::Decimal>,
}

impl Default for ReviewPolicy {
    fn default() -> Self {
        Self {
            reviewer_prefixes: vec!["human:".to_string()],
            allow_approval_of_broken_linkage: false,
            calc_required_at_or_above: None,
        }
    }
}

impl ReviewPolicy {
    /// 操作者是否在审核允许列表内（空列表 = 不限——测试/单机自担）。
    pub fn operator_allowed(&self, operator: &str) -> bool {
        self.reviewer_prefixes.is_empty()
            || self
                .reviewer_prefixes
                .iter()
                .any(|p| operator.starts_with(p.as_str()))
    }
}
