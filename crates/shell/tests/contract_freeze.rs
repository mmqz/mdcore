//! R23-d — 宿主↔桥↔核契约冻结锁（CONTRACT.md v1）。
//!
//! 母本与产品仓（mdcore/）对同一契约面必须逐词面一致：本文件锁 Rust 侧
//! 可静态断言的面（shell-native 蓝图注入脚本 + 契约文档存在性与版本）；
//! window 运行时面由 web/tests/contract_gate.py 锁。

use std::path::Path;

fn prototype_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

/// 蓝图注入脚本词面锁：事件族 = mdc-inject*（RC 轮统一命名域），
/// 入口 = __inject / __inject_chunk；旧名 md-inject* 禁止复活。
#[test]
fn shell_native_blueprint_uses_frozen_event_family() {
    let src = include_str!("../../shell-native/src/lib.rs");
    assert!(src.contains("'mdc-inject'"), "mdc-inject 事件名必须在蓝图");
    assert!(
        src.contains("'mdc-inject-chunk'"),
        "mdc-inject-chunk 事件名必须在蓝图"
    );
    assert!(src.contains("__inject_chunk"), "渐进入口必须在蓝图");
    assert!(
        !src.contains("'md-inject'"),
        "旧事件名 md-inject 禁止复活（CONTRACT.md §2）"
    );
    assert!(
        !src.contains("'md-inject-chunk'"),
        "旧事件名 md-inject-chunk 禁止复活"
    );
}

/// 契约文档存在 + 版本标记 + 关键面词面（双侧锁的共同锚）。
#[test]
fn contract_doc_frozen_v1_present() {
    let doc = std::fs::read_to_string(prototype_root().join("CONTRACT.md"))
        .expect("CONTRACT.md 必须存在于原型根");
    assert!(doc.contains("冻结 v1"), "版本标记必须在");
    assert!(
        doc.contains("__inject") && doc.contains("__inject_chunk"),
        "注入面必须在"
    );
    assert!(doc.contains("mdc-inject"), "事件族必须在");
    assert!(doc.contains("__contractVersion"), "版本常量必须在");
    assert!(
        doc.contains("surface_load_markdown"),
        "mdshell Host 契约必须在"
    );
    assert!(
        doc.contains("FirstChunkInteractive"),
        "OpenProgress 契约必须在"
    );
    assert!(
        doc.contains("adopt_external"),
        "外部重载接线（ADR-B7）必须在"
    );
}

/// R24-a 契约 v1.1 设置面词面锁：设置读写项 + 事件族 + 版本升级记录
/// （与 web/tests/contract_gate.py 的 settings_* 三检同锚）。
#[test]
fn contract_doc_v1_1_settings_face_present() {
    let doc = std::fs::read_to_string(prototype_root().join("CONTRACT.md"))
        .expect("CONTRACT.md 必须存在于原型根");
    assert!(doc.contains("v1.1"), "minor 版本标记必须在");
    assert!(doc.contains("__settings"), "设置读面必须在");
    assert!(doc.contains("__setSetting"), "设置写面必须在");
    assert!(
        doc.contains("mdc-settings-changed"),
        "设置事件族必须在（mdc-* 命名域）"
    );
    assert!(
        doc.contains("undo_coalesce_ms"),
        "默认值表必须在（双侧一致锚）"
    );
    // Rust 侧权威实现锚：mdshell::settings 模块在位（防文档与实现漂移）。
    // 编译期常量锁（比运行时 assert 更早失败）。
    const _: () = assert!(mdshell::settings::SCHEMA_VERSION == 1);
}

/// R24-b 契约 §5 会话面词面锁（纯宿主面：window 面零新增、
/// `__contractVersion` 保持 1.1——这一"不升版"事实本身是契约语义，
/// 与 session 面词面一起锁死，防漂移）。
#[test]
fn contract_doc_v1_1_session_face_present() {
    let doc = std::fs::read_to_string(prototype_root().join("CONTRACT.md"))
        .expect("CONTRACT.md 必须存在于原型根");
    assert!(doc.contains("§5 会话面"), "会话面章节必须在");
    assert!(doc.contains("SessionManager"), "会话管理器契约必须在");
    assert!(doc.contains("DocumentAlreadyOpen"), "一窗一档错误面必须在");
    assert!(doc.contains("DEFAULT_MAX_SESSIONS"), "资源上限常量面必须在");
    assert!(
        doc.contains("snapshot_record"),
        "会话持久化（恢复上次打开集）必须在"
    );
    assert!(doc.contains("session_restore"), "settings 消费联动必须在");
    assert!(doc.contains("mark_saved"), "Core 脏标记契约必须在");
    // #16/A4（CONTRACT v1.4）：§5/§6 的 window 面零新增事实维持，
    // 但 `__contractVersion` 已随 §4 v1.4 升 1.2——锁两向事实：
    // §6 不碰 window 面 + 当前版本号来自 §4（防误升版/漏升版漂移）。
    assert!(
        doc.contains("window 面零新增（§6 从未触碰 web window 面"),
        "§6 不升版裁决必须显式在档"
    );
    assert!(
        doc.contains("`__contractVersion` 1.1 → 1.2"),
        "§4 v1.4 升版记录必须在档（历史记录——v1.5 后仍保留）"
    );
    // W-1/W-2（CONTRACT v1.5）：§4 docx_export 层两布尔键 + 当前
    // window 面版本 1.3（防误升版/漏升版漂移——两向钉定）。
    assert!(
        doc.contains("`__contractVersion` 1.2 → 1.3"),
        "§4 v1.5 升版记录必须在档（历史记录——v1.6 后仍保留）"
    );
    assert!(
        doc.contains("| docx_export.toc | false |"),
        "W-1 toc 键默认值表必须在档"
    );
    assert!(
        doc.contains("| docx_export.anonymize | false |"),
        "W-2 anonymize 键默认值表必须在档"
    );
    // #43（CONTRACT v1.6）：§4 appearance.language 键 + 当前 window
    // 面版本 1.4（防误升版/漏升版漂移——两向钉定）。
    assert!(
        doc.contains("`__contractVersion` 1.3 → 1.4"),
        "§4 v1.6 升版记录必须在档（当前 window 面版本 1.4）"
    );
    assert!(
        doc.contains("| appearance.language | \"auto\" |"),
        "#43 language 键默认值表必须在档"
    );
    assert!(
        doc.contains("auto \\| en \\| zh-CN"),
        "#43 language 值域三值必须在档"
    );
    // Rust 侧权威实现锚（编译期常量锁）。
    const _: () = assert!(mdshell::session::DEFAULT_MAX_SESSIONS == 16);
    const _: () = assert!(mdshell::session::SESSION_RECORD_VERSION == 1);
}

/// mdshell 契约层签名冻结（编译即锁；此处锁语义形状）。
#[test]
fn mdshell_surface_shapes_frozen() {
    // OpenProgress 三变体 + Host trait + open/save 签名由 mdshell 自身
    // 编译保证；此处锁渐进打开的进度序列不变量（契约语义锚）。
    use mdshell::{open_document, open_document_progressive, save_document};
    use std::cell::RefCell;

    struct NullHost;
    impl mdshell::Host for NullHost {
        fn surface_load_markdown(&mut self, _markdown: &str) -> Result<(), String> {
            Ok(())
        }
    }

    let mut host = NullHost;
    let events = RefCell::new(vec![]);
    let chunks = vec!["# a\n\nx\n\n".to_string(), "# b\n\ny\n\n".to_string()];
    let session = open_document_progressive(&chunks, None, &mut host, |p| {
        events.borrow_mut().push(format!("{p:?}"))
    })
    .expect("progressive open");
    let ev = events.borrow();
    assert_eq!(ev.len(), 3);
    assert!(ev[0].starts_with("FirstChunkInteractive"), "TTI 事件首位");
    assert!(ev[1].starts_with("ChunkAppended"), "追加事件");
    assert!(ev[2].starts_with("Completed"), "完成事件末位");

    let (md, meta) = save_document(&session);
    let reopened = open_document(&md, Some(&meta)).expect("reopen");
    assert_eq!(reopened.id, session.id, "身份复原契约");
}
