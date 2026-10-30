//! 打开文档分派（#68 阶段 1.1）。
//!
//! 按 扩展名把磁盘文件分派到编辑器形态：
//!
//! - `.md` / `.markdown` / `.txt` → **可编辑**（源文本编辑——批 1）
//! - `.docx` / `.pdf` → 只读渲染面（**批 2 接线**：mdc-office-io /
//!   mdc-pdf 引擎依赖引入；当前返回 `ReadOnlyPending` 呈提示态）
//! - 其余 → `Unsupported`（空态提示）
//!
//! 大小写不敏感匹配扩展名；读取失败返回 `OpenError`（空态呈现原因）。

use std::path::{Path, PathBuf};

/// 打开结果。
pub enum OpenedDocument {
    /// 可编辑文本文档（md / txt）。
    EditableText { path: PathBuf, content: String },
    /// docx / pdf——只读渲染面待批 2 引擎接线。
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
        "docx" => OpenedDocument::ReadOnlyPending {
            path: path.to_path_buf(),
            kind: "Word 文档（.docx）",
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
