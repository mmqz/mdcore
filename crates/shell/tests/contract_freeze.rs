//! R23-d — 宿主↔桥↔核契约冻结锁（CONTRACT.md v1）。
//!
//! 母本与产品仓（mdcore/）对同一契约面必须逐词面一致：本文件锁 Rust 侧
//! 可静态断言的面（shell-native 蓝图注入脚本 + 契约文档存在性与版本）；
//! window 运行时面由 web/tests/contract_gate.py 锁。

use std::path::Path;

fn prototype_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap()
}

/// 蓝图注入脚本词面锁：事件族 = mdc-inject*（RC 轮统一命名域），
/// 入口 = __inject / __inject_chunk；旧名 md-inject* 禁止复活。
#[test]
fn shell_native_blueprint_uses_frozen_event_family() {
    let src = include_str!("../../shell-native/src/lib.rs");
    assert!(src.contains("'mdc-inject'"), "mdc-inject 事件名必须在蓝图");
    assert!(src.contains("'mdc-inject-chunk'"), "mdc-inject-chunk 事件名必须在蓝图");
    assert!(src.contains("__inject_chunk"), "渐进入口必须在蓝图");
    assert!(!src.contains("'md-inject'"), "旧事件名 md-inject 禁止复活（CONTRACT.md §2）");
    assert!(!src.contains("'md-inject-chunk'"), "旧事件名 md-inject-chunk 禁止复活");
}

/// 契约文档存在 + 版本标记 + 关键面词面（双侧锁的共同锚）。
#[test]
fn contract_doc_frozen_v1_present() {
    let doc = std::fs::read_to_string(prototype_root().join("CONTRACT.md"))
        .expect("CONTRACT.md 必须存在于原型根");
    assert!(doc.contains("冻结 v1"), "版本标记必须在");
    assert!(doc.contains("__inject") && doc.contains("__inject_chunk"), "注入面必须在");
    assert!(doc.contains("mdc-inject"), "事件族必须在");
    assert!(doc.contains("__contractVersion"), "版本常量必须在");
    assert!(doc.contains("surface_load_markdown"), "mdshell Host 契约必须在");
    assert!(doc.contains("FirstChunkInteractive"), "OpenProgress 契约必须在");
    assert!(doc.contains("adopt_external"), "外部重载接线（ADR-B7）必须在");
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
