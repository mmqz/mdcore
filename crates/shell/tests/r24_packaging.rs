//! R24-c — 打包分发基建锁（版本单源 + 工件纪律 + 定名门）。
//!
//! 锁清单：
//! 1. 版本单源：五 crate 全部 `version.workspace = true`（禁止独立声明）+
//!    根 `[workspace.package].version` == `mdcore::VERSION`（运行时唯一
//!    出口）+ 本 crate（shell）编译期版本一致（发布产物四处注入同源）；
//! 2. 打包工件在位且命名域合规：桌面入口（mime + scheme 注册面）/
//!    NSIS / Info.plist / AppRun / 构建脚本——可执行名与 scheme 一律
//!    mdcore 自研命名域（TRADEMARK_RULES §1）；
//! 3. 构建脚本版本注入纪律：从产物 `--version`（= mdcore::VERSION）读取，
//!    禁止手抄（checklist B2 的静态锚）；
//! 4. 定名阻塞门在档：正式名未定前 {{APP_NAME}} 占位纪律 + 商标检索
//!    强制门（checklist A 节）显式存在于 README 与 checklist。

use std::path::Path;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("读取 {rel}: {e}"))
}

// ---------------------------------------------------------------------------
// 1. 版本单源（CARGO_PKG_VERSION 单源机制合同）
// ---------------------------------------------------------------------------

#[test]
fn version_single_source_via_workspace() {
    // 运行时唯一出口 = mdcore::VERSION；本 crate（shell）与其编译期一致
    // （两者都继承 workspace.package → cargo 侧无第二手写点即恒等）。
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        "0.1.0",
        "shell crate 版本必须来自 workspace 继承（根 Cargo.toml 唯一手写点）"
    );
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        mdcore::VERSION,
        "shell 与 mdcore 版本必须同源"
    );

    // 根清单：[workspace.package].version 与 mdcore::VERSION 一致。
    let root = read("Cargo.toml");
    assert!(
        root.contains("[workspace.package]"),
        "workspace.package 节必须在（版本唯一手写点）"
    );
    let ws_version = root
        .lines()
        .find_map(|l| {
            l.strip_prefix("version = ")
                .map(|v| v.trim().trim_matches('"').to_string())
        })
        .expect("workspace.package 内必须有 version");
    assert_eq!(ws_version, mdcore::VERSION, "根手写点与运行时常量必须一致");

    // 五 crate 全部继承（独立声明 = 第二手写点 = 漂移源，禁止）。
    // 清单含 crates/pdf（mdc-pdf，PDF-b 起 workspace 成员——T1 审计 F7
    // 补齐：此前清单漏列，仅由 pdfb_skeleton 的版本自锁补位）。
    for c in ["mdcore", "mdcore-wasm", "shell", "shell-native", "pdf"] {
        let mf = read(&format!("crates/{c}/Cargo.toml"));
        assert!(
            mf.contains("version.workspace = true"),
            "crates/{c} 必须以 version.workspace 继承版本（禁止独立声明）"
        );
        assert!(
            !mf.lines().any(|l| l.starts_with("version = ")),
            "crates/{c} 不得存在独立 version 声明"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. 打包工件在位 + 命名域合规
// ---------------------------------------------------------------------------

#[test]
fn packaging_artifacts_present_and_naming_scoped() {
    let files = [
        "packaging/README.md",
        "packaging/checklist.md",
        "packaging/shared/app.desktop.tpl",
        "packaging/appimage/AppRun",
        "packaging/appimage/build-appimage.sh",
        "packaging/deb/control.tpl",
        "packaging/deb/postinst.tpl",
        "packaging/deb/build-deb.sh",
        "packaging/nsis/installer.nsi.tpl",
        "packaging/macos/Info.plist.tpl",
    ];
    for f in files {
        assert!(repo_root().join(f).is_file(), "打包工件缺失：{f}");
    }

    // 桌面入口：mime 关联 + scheme 注册（checklist D1 的模板面锚）。
    let desktop = read("packaging/shared/app.desktop.tpl");
    assert!(desktop.contains("text/markdown"), ".md mime 关联必须在");
    assert!(
        desktop.contains("x-scheme-handler/mdcore"),
        "mdcore:// scheme 注册必须在"
    );

    // macOS：URL scheme 声明。
    let plist = read("packaging/macos/Info.plist.tpl");
    assert!(
        plist.contains("<string>mdcore</string>"),
        "CFBundleURLSchemes 必须含 mdcore"
    );

    // NSIS：安装 ID 自研命名域。
    let nsi = read("packaging/nsis/installer.nsi.tpl");
    assert!(
        nsi.contains("dev.mdcore.editor"),
        "APPID 必须用 mdcore 命名域"
    );

    // deb：webkit2gtk-4.1 运行时依赖（ADR-H1 GTK3 架构的声明面锚）。
    let control = read("packaging/deb/control.tpl");
    assert!(
        control.contains("libwebkit2gtk-4.1-0"),
        "deb 依赖必须声明 webkit2gtk-4.1"
    );
}

// ---------------------------------------------------------------------------
// 3. 构建脚本版本注入纪律（禁止手抄版本号）
// ---------------------------------------------------------------------------

#[test]
fn build_scripts_inject_version_from_single_source() {
    for script in [
        "packaging/appimage/build-appimage.sh",
        "packaging/deb/build-deb.sh",
    ] {
        let src = read(script);
        assert!(
            src.contains("--version"),
            "{script} 必须从产物 --version 读取版本（mdcore::VERSION 单源，禁止手抄）"
        );
        assert!(
            !src.contains("VERSION=\"0."),
            "{script} 不得内嵌具体版本号字符串"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. 定名阻塞门在档（{{APP_NAME}} 占位纪律 + 商标检索强制门）
// ---------------------------------------------------------------------------

#[test]
fn naming_gate_documented_before_placeholder_replacement() {
    let readme = read("packaging/README.md");
    assert!(
        readme.contains("TRADEMARK_RULES §4"),
        "定名纪律必须引用商标规范 §4"
    );
    assert!(
        readme.contains("{{APP_NAME}}"),
        "显示名占位符约定必须在（checklist A4 消费）"
    );

    let checklist = read("packaging/checklist.md");
    for gate in ["商标检索五步完成", "占位符全量替换", "版本单源"] {
        assert!(checklist.contains(gate), "核对单必须含门：{gate}");
    }
    // 模板尚未定名（占位符在位）——定名提交必须同步改写本断言 +
    // checklist A4 勾选记录 + packaging/README 变量表（三处一体）。
    let desktop = read("packaging/shared/app.desktop.tpl");
    assert!(
        desktop.contains("{{APP_NAME}}"),
        "未定名：显示名占位符必须在位（定名时随 A4 门替换）"
    );
}
