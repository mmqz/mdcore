//! 打开文档分派（#68 阶段 1）。
//!
//! 按 扩展名把磁盘文件分派到编辑器形态：
//!
//! - `.md` / `.markdown` / `.txt` → **可编辑**（源文本编辑——批 1）
//! - `.docx` → **只读渲染**（批 3：mdc-office-io 块序 → 行模型，
//!   标题树 + 样式 span——见 [`crate::editor::docx_doc`]）
//! - `.pdf` → 只读渲染面（批 4 接线：mdc-pdf）
//! - 其余 → `Unsupported`（空态提示）
//!
//! 大小写不敏感匹配扩展名；读取失败返回 `OpenError`（空态呈现原因）。

use std::path::{Path, PathBuf};

use crate::editor::docx_doc::{self, DocxPayload};

/// 打开结果。
pub enum OpenedDocument {
    /// 可编辑文本文档（md / txt）。
    EditableText { path: PathBuf, content: String },
    /// docx 只读渲染（批 3）：mdc-office-io 解析 + 行模型转换产物。
    ReadOnlyDocx { path: PathBuf, payload: DocxPayload },
    /// pdf——只读渲染面待批 4 引擎接线。
    ReadOnlyPending { path: PathBuf, kind: &'static str },
    /// 不支持的扩展名。
    Unsupported { path: PathBuf },
    /// 读取失败（IO 错误 / 超大等）。
    OpenError { path: PathBuf, message: String },
}

/// 打开一个文件（分派 + 读取）。
pub fn open(path: &Path) -> OpenedDocument {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "txt" => match std::fs::read_to_string(path) {
            Ok(content) => OpenedDocument::EditableText {
                path: path.to_path_buf(),
                content,
            },
            Err(err) => OpenedDocument::OpenError {
                path: path.to_path_buf(),
                message: err.to_string(),
            },
        },
        // 批 3：docx 只读渲染（兼容模式——安全门恒严，畸形部件降级
        // 为警告入属性面）。
        "docx" => match docx_doc::load(path) {
            Ok(payload) => OpenedDocument::ReadOnlyDocx {
                path: path.to_path_buf(),
                payload,
            },
            Err(message) => OpenedDocument::OpenError {
                path: path.to_path_buf(),
                message,
            },
        },
        "pdf" => OpenedDocument::ReadOnlyPending {
            path: path.to_path_buf(),
            kind: "PDF 文档（.pdf）",
        },
        _ => OpenedDocument::Unsupported {
            path: path.to_path_buf(),
        },
    }
}
