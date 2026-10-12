//! 依赖闭包锁：mdc-settings 的域无关性（对 mdcore 零依赖）。
//!
//! T3 复用地基轮抽取纪律：设置面是三域（MD/PDF/Office）可共用的基础
//! 能力，本 crate 不得引入 mdcore（或任何域核）依赖——一旦倒灌域语义，
//! "基础能力独立消费"的抽取目的即失效。Cargo.toml 无依赖项 = 编译级
//! 事实（代码无法引用不可达的 crate）；本锁以文件文本证据固化该事实，
//! 防未来无意再耦合（文件级证据手法同 contract_freeze / pdfb_skeleton
//! 镜像锁）。

use std::fs;

#[test]
fn dependency_closure_is_domain_agnostic() {
    let mf = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("本 crate Cargo.toml 在位");
    // 行首匹配 = 依赖声明键（`mdcore = { path = ... }`）；描述/注释中的
    // 文字性提及（如"对 mdcore 零依赖"的纪律陈述）不算依赖。
    assert!(
        !mf.lines().any(|l| l.trim_start().starts_with("mdcore")),
        "mdc-settings 不得依赖 mdcore（域无关抽取纪律——mdshell 侧 \
         defaults_align_with_core_constants 已跨 crate 锁定数值对齐，\
         依赖倒灌会破坏三域独立消费）"
    );
    assert!(
        mf.lines().any(|l| l.trim_start().starts_with("toml_edit")),
        "TOML sidecar 格式保持依赖在位（格式保持是本 crate 核心机制）"
    );
}
