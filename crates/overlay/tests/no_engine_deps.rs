//! 验收第 1 条（#239）：mdc-overlay 不依赖任何具体文档引擎。
//!
//! 证明：`cargo tree -p mdc-overlay` 的普通依赖边里无
//! mdc-pdf / mdc-office-io / mdc-canvas / mang-shell（依赖
//! mdc-evidence 是允许的——issue #239 第七节用户裁定）。
//!
//! 用 cargo tree（只读 metadata/lock，不构建、不加 build 锁）在测试
//! 内运行时断言——纪律进回归套件，不靠评审记忆。

use std::process::Command;

#[test]
fn overlay_depends_on_no_document_engine() {
    let out = Command::new("cargo")
        .args([
            "tree",
            "-p",
            "mdc-overlay",
            "--prefix",
            "none",
            "--edges",
            "normal",
        ])
        .output()
        .expect("cargo tree 可执行（测试环境 cargo 在 PATH）");
    assert!(
        out.status.success(),
        "cargo tree 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8 输出");
    let banned = [
        "mdc-pdf",
        "mdc-office-io",
        "md-office-io",
        "mdc-canvas",
        "mang-shell",
        "mdshell",
    ];
    for b in banned {
        assert!(
            !stdout.contains(b),
            "三原则①破坏：mdc-overlay 依赖树出现 {b}\n依赖树：\n{stdout}"
        );
    }
    // 允许并预期的契约层依赖必须在场（白名单不是摆设——复用
    // DocumentRef/指纹/LinkageStatus，不复制平行实现）。
    assert!(
        stdout.contains("mdc-evidence"),
        "预期依赖 mdc-evidence 不在树中（契约复用面丢失）\n{stdout}"
    );
}
