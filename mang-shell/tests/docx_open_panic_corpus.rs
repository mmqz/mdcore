//! #226 R2：docx 打开入口 panic 兜底——docx4j 语料 4 件实锤回归。
//!
//! 语料：`test-artifacts/health-check/corpus/docx4j/` 中 health-check
//! 批 4（REPORT #E1）实锤的 4 件 panic 源（Word 365 styles 三件 +
//! strict OOXML 一件——docx-rs 0.4.22 `run_property.rs:110/155`
//! index OOB + `width.rs:18` ParseFloat unwrap）。缺省路径相对仓根
//! （mang-shell 独立 workspace——与 docx_edit_corpus.rs 同口径），
//! `MDCORE_DOCX4J_CORPUS` 可覆盖；目录不存在即跳过（不红——语料在
//! 引擎仓 test-artifacts，本 workspace 不硬依赖）。
//!
//! 断言口径 = issue #226 验收：打开不崩溃（成功或 typed 错误）。
//! office-io 预检防御（同 PR 前置 commit）落地后四件全部 Ok——比
//! 验收下限更强，直接钉 Ok（回退到 Err 即红，防防御面退化）。
use std::path::PathBuf;

use mang_shell::editor::docx_doc;

/// health-check 批 4 #E1 实锤的 4 件 panic 源（docx-rs 0.4.22）。
const PANIC_FILES: &[&str] = &[
    "runprops_word365_props-b-styles.docx",
    "runprops_word365_props-enum-styles.docx",
    "runprops_word365_props-u-styles.docx",
    "strict_strict-sample-docx.docx",
];

fn corpus_dir() -> Option<PathBuf> {
    let dir = std::env::var("MDCORE_DOCX4J_CORPUS")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../test-artifacts/health-check/corpus/docx4j"));
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    dir.is_dir().then_some(dir)
}

/// 四件 panic 源经打开入口（只读 + 可编辑）均不崩溃且可读。
#[test]
fn docx4j_panic_corpus_opens_without_panic() {
    let Some(dir) = corpus_dir() else {
        eprintln!("docx4j 语料缺席——跳过（不红）");
        return;
    };
    for name in PANIC_FILES {
        let path = dir.join(name);
        assert!(path.is_file(), "语料件缺席：{}", path.display());

        // 只读装载：不 panic 即过（Ok 或 typed Err 均合法——验收口径）；
        // office-io 预检防御落地后预期 Ok（更强，直接钉）。
        let loaded = docx_doc::load(&path);
        assert!(
            loaded.is_ok(),
            "{name} 应可读（office-io 预检防御 + 引擎警告通道）：{:?}",
            loaded.err()
        );

        // 可编辑装载：同样不崩溃——Ok 含「降级只读」（宽面可看即 Ok），
        // Err 仅允许 typed 拒（此处四件宽面已 Ok，故 Err 即红）。
        let editable = docx_doc::load_editable(&path);
        assert!(
            editable.is_ok(),
            "{name} 可编辑装载应 Ok（降级只读也算 Ok——不崩溃）"
        );
    }
}
