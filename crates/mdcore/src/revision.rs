//! Revision 日志 — op 流持久化（append-only 设计）。
//!
//! - 每个**已封口**的前向事务一条记录（coalesced 追加同 revision 号多记录，
//!   重放按序合并语义等价）。
//! - undo/redo 入日志（R11 审查修正：此前「不入日志、可由前向流推导」
//!   的说法不成立——不记会导致重放与实时状态分叉，违背「日志自含 +
//!   确定性重放」承诺；以 Source::Undo / Source::Redo 记录，重放侧按
//!   普通 op 序列应用）。精确状态复原走 snapshot。
//! - JSONL 行式存储：一行一条记录，便于增量 append 与截断。

use serde::{Deserialize, Serialize};

use crate::history::Source;
use crate::op::Op;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRecord {
    pub revision: u64,
    pub ops: Vec<Op>,
    pub source: Source,
    #[serde(default)]
    pub command: Option<String>,
    /// 文档身份（§8.3）：仅 genesis 记录携带（R10-a 文件层）。
    /// serde default 保持旧 JSONL（无此字段）可读。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<crate::document::DocumentId>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RevisionLog {
    pub records: Vec<LogRecord>,
}

/// R20 内存审计：日志组件级活数据估计（字节）。
#[derive(Debug, Serialize)]
pub struct LogMemStats {
    pub records: usize,
    pub ops: usize,
    /// op 内 String/Attr 的内容字节（不含结构体头）。
    pub content_bytes: usize,
    /// op 内 Vec/String 结构体头估计（每个 24B）。
    pub headers_est_bytes: usize,
    /// NodePath 元素总量 × 8B。
    pub path_elem_bytes: usize,
    /// to_jsonl 近似长度（重放文件尺寸量级参考）。
    pub json_est_bytes: usize,
}

impl RevisionLog {
    pub fn record(&mut self, revision: u64, ops: &[Op], source: Source, command: Option<String>) {
        self.records.push(LogRecord {
            revision,
            ops: ops.to_vec(),
            source,
            command,
            document_id: None,
        });
    }

    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for r in &self.records {
            out.push_str(&serde_json::to_string(r).expect("log record serialize"));
            out.push('\n');
        }
        out
    }

    pub fn from_jsonl(jsonl: &str) -> Result<Self, serde_json::Error> {
        let mut records = Vec::new();
        for line in jsonl.lines().filter(|l| !l.trim().is_empty()) {
            records.push(serde_json::from_str(line)?);
        }
        Ok(Self { records })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// R20 内存审计：遍历记录累计 op 内容字节（活数据近似）。
    pub fn mem_stats(&self) -> LogMemStats {
        let mut ops = 0usize;
        let mut content = 0usize;
        let mut headers = 0usize;
        let mut path_elems = 0usize;
        let mut json_est = 0usize;
        for r in &self.records {
            json_est += 120 + r.command.as_deref().map_or(0, str::len);
            for o in &r.ops {
                ops += 1;
                headers += 5; // op 枚举体 + 内部 Vec/String 头近似
                match o {
                    Op::InsertBlock {
                        parent,
                        attrs,
                        text,
                        ..
                    } => {
                        content += text.len();
                        path_elems += parent.len();
                        for (k, v) in attrs {
                            content += k.len() + v.len();
                            headers += 2;
                        }
                    }
                    Op::ReplaceText { path, insert, .. } => {
                        content += insert.len();
                        path_elems += path.len();
                    }
                    Op::SetBlockType { path, .. } => path_elems += path.len(),
                    Op::SetAttrs { path, attrs, .. } => {
                        path_elems += path.len();
                        for (k, v) in attrs {
                            content += k.len() + v.len();
                            headers += 2;
                        }
                    }
                    Op::SetMark { path, kind, .. } => {
                        path_elems += path.len();
                        content += kind.len();
                    }
                    Op::MoveBlock {
                        from, to_parent, ..
                    } => {
                        path_elems += from.len() + to_parent.len();
                    }
                    Op::RemoveBlock { path } => path_elems += path.len(),
                }
            }
        }
        path_elems *= std::mem::size_of::<usize>();
        LogMemStats {
            records: self.records.len(),
            ops,
            content_bytes: content,
            headers_est_bytes: headers * 24,
            path_elem_bytes: path_elems,
            json_est_bytes: json_est,
        }
    }
}
