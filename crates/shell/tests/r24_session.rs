//! R24-b — 多窗口核侧语义锁（主档 §31 R24-b 机制合同）。
//!
//! 锁清单：
//! 1. 生命周期：open/focus/close + 焦点序（MRU 末位 = 最近聚焦）；
//! 2. 资源上限：超限 Err（不静默驱逐）、close 释放名额；
//! 3. 重复窗口拒绝；未知窗口 close/focus/get 拒绝；
//! 4. 一窗一档（v0）：同 document_id 二开 → DocumentAlreadyOpen（携带
//!    持有窗口）；匿名文档多开不受限；close 后同文档可重开；
//! 5. 反向映射 window_of_document；
//! 6. Core 脏标记生命周期：View 置位 / mark_saved 清位 / undo 置位 /
//!    System 装载（extend_from_markdown）不置位 / adopt_external 清位；
//! 7. 会话记录：快照 → JSON 往返 + 焦点序保持 + 无路径会话不入恢复集；
//! 8. 未来 schema 记录拒绝（宿主降级冷启动的输入面）；
//! 9. 记录确定性：同态两次快照逐字节相同；
//! 10. dirty_windows 查询（BTreeMap 键序）。

use mdcore::document::DocumentId;
use mdcore::op::Op;
use mdcore::{ApplyMeta, Core};
use mdshell::session::{
    SessionError, SessionManager, SessionRecord, WindowId as W, DEFAULT_MAX_SESSIONS,
};
use mdshell::{open_document, DocumentSession};

fn session_with_doc(md: &str, ts: u64) -> DocumentSession {
    let mut core = Core::from_markdown(md);
    core.set_document_id(Some(DocumentId::generate(ts)))
        .unwrap();
    DocumentSession::new(core)
}

fn type_a_char(s: &mut DocumentSession) {
    s.core
        .apply_ops(
            vec![Op::ReplaceText {
                path: vec![0],
                start: 0,
                end: 0,
                insert: "x".into(),
            }],
            ApplyMeta::view("type"),
        )
        .unwrap();
}

// ---------------------------------------------------------------------------
// 1. 生命周期 + 焦点序
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_open_focus_close_tracks_mru_order() {
    let mut sm = SessionManager::default();
    sm.open(W(1), session_with_doc("one", 1)).unwrap();
    sm.open(W(2), session_with_doc("two", 2)).unwrap();
    sm.open(W(3), session_with_doc("three", 3)).unwrap();
    assert_eq!(sm.count(), 3);
    assert_eq!(
        sm.windows_in_focus_order(),
        vec![W(1), W(2), W(3)],
        "开档序 = 初始焦点序"
    );
    assert_eq!(sm.focused_window(), Some(W(3)), "最近打开 = 最近聚焦");

    sm.focus(W(1)).unwrap();
    assert_eq!(
        sm.windows_in_focus_order(),
        vec![W(2), W(3), W(1)],
        "聚焦移到 MRU 末位"
    );
    assert_eq!(sm.focused_window(), Some(W(1)));

    // 重复聚焦幂等
    sm.focus(W(1)).unwrap();
    assert_eq!(sm.windows_in_focus_order(), vec![W(2), W(3), W(1)]);

    // 关闭中间窗口：焦点序同步收缩
    let closed = sm.close(W(3)).unwrap();
    assert!(closed.id.is_some(), "close 返回会话供宿主收尾");
    assert_eq!(sm.windows_in_focus_order(), vec![W(2), W(1)]);
    assert_eq!(sm.count(), 2);

    // 关闭的窗口 id 可复用（宿主窗口句柄回收语义）
    sm.open(W(3), session_with_doc("three-again", 33)).unwrap();
    assert_eq!(sm.focused_window(), Some(W(3)));
}

// ---------------------------------------------------------------------------
// 2/3. 资源上限 + 拒绝面
// ---------------------------------------------------------------------------

#[test]
fn resource_limit_enforced_and_freed_by_close() {
    assert_eq!(
        DEFAULT_MAX_SESSIONS, 16,
        "默认上限常量锁（session.rs 单元同款）"
    );

    let mut sm = SessionManager::new_with_limit(3);
    for i in 0..3 {
        sm.open(W(i), session_with_doc("doc", 100 + i)).unwrap();
    }
    match sm.open(W(9), session_with_doc("overflow", 999)) {
        Err(SessionError::LimitReached { limit }) => assert_eq!(limit, 3),
        other => panic!("期望 LimitReached, got {other:?}"),
    }
    assert_eq!(sm.count(), 3, "超限 open 零副作用");

    // 释放一个名额后可再开
    sm.close(W(1)).unwrap();
    sm.open(W(9), session_with_doc("overflow", 999)).unwrap();
    assert_eq!(sm.count(), 3);
}

#[test]
fn duplicate_and_unknown_window_rejected() {
    let mut sm = SessionManager::default();
    sm.open(W(1), session_with_doc("a", 1)).unwrap();
    assert_eq!(
        sm.open(W(1), session_with_doc("b", 2)),
        Err(SessionError::DuplicateWindow(W(1))),
        "窗口已持有会话必须显式拒绝（宿主 id 复用 bug 的核侧防线）"
    );

    assert!(matches!(
        sm.close(W(42)),
        Err(SessionError::UnknownWindow(W(42)))
    ));
    assert!(matches!(
        sm.focus(W(42)),
        Err(SessionError::UnknownWindow(W(42)))
    ));
    assert!(matches!(sm.get(W(42)), Err(SessionError::UnknownWindow(_))));
    assert!(sm.get(W(1)).is_ok());
}

// ---------------------------------------------------------------------------
// 4/5. 一窗一档 + 反向映射
// ---------------------------------------------------------------------------

#[test]
fn one_document_one_window_and_reverse_mapping() {
    let mut sm = SessionManager::default();
    let s1 = session_with_doc("same", 7);
    let doc_id = s1.id.unwrap();
    sm.open(W(1), s1).unwrap();

    // 同 document_id 二开 → 拒绝并告知持有窗口（宿主 reveal-existing 语义）
    // 构造：无身份核 + 显式设同一 id（重开场景 = 同一文件的 meta 复原）
    let mut s2 = DocumentSession::new(Core::from_markdown("same-content-different-session"));
    s2.core.set_document_id(Some(doc_id)).unwrap();
    s2.id = s2.core.document_id();
    match sm.open(W(2), s2) {
        Err(SessionError::DocumentAlreadyOpen {
            document_id,
            window,
        }) => {
            assert_eq!(document_id, doc_id);
            assert_eq!(window, W(1), "错误面携带持有窗口（宿主聚焦用）");
        }
        other => panic!("期望 DocumentAlreadyOpen, got {other:?}"),
    }

    // 反向映射
    assert_eq!(sm.window_of_document(&doc_id), Some(W(1)));
    let other_id = DocumentId::generate(999);
    assert_eq!(sm.window_of_document(&other_id), None);

    // 匿名文档（id=None）多开不受限（新未存文档场景）
    let anon1 = DocumentSession::new(Core::from_markdown("scratch"));
    let anon2 = DocumentSession::new(Core::from_markdown("scratch2"));
    sm.open(W(5), anon1).unwrap();
    sm.open(W(6), anon2).unwrap();

    // close 后同文档可重开（映射随窗口注销）
    sm.close(W(1)).unwrap();
    assert_eq!(sm.window_of_document(&doc_id), None);
    let mut s3 = DocumentSession::new(Core::from_markdown("reopened"));
    s3.core.set_document_id(Some(doc_id)).unwrap();
    s3.id = s3.core.document_id();
    sm.open(W(7), s3).unwrap();
    assert_eq!(sm.window_of_document(&doc_id), Some(W(7)));
}

// ---------------------------------------------------------------------------
// 6. Core 脏标记生命周期（R24-b 置于 Core：单一变异入口挂钩）
// ---------------------------------------------------------------------------

#[test]
fn core_dirty_lifecycle_view_system_undo_adopt() {
    // 新核 clean；View 事务置位；mark_saved 清位；undo 再置位
    let mut c = Core::from_markdown("hello");
    assert!(!c.is_dirty(), "新核 = clean（开档态）");
    c.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 0,
            end: 0,
            insert: "x".into(),
        }],
        ApplyMeta::view("type"),
    )
    .unwrap();
    assert!(c.is_dirty(), "View 事务置脏");
    c.mark_saved();
    assert!(!c.is_dirty(), "宿主落盘成功后 mark_saved 清位");
    c.undo().unwrap();
    assert!(c.is_dirty(), "undo 相对保存点变更 = 置位（粘性安全向）");
    c.mark_saved();

    // System 装载事务不置位：渐进打开 chunk 追加保持 clean
    c.extend_from_markdown("\nappended section\n").unwrap();
    assert!(!c.is_dirty(), "System 装载（extend_from_markdown）不置脏");

    // adopt_external（take-external）成功后清位：缓冲 == 外部磁盘内容
    let outcome = c
        .adopt_external("totally different content\n", None)
        .unwrap();
    assert!(
        outcome.revision_after > outcome.revision_before,
        "采纳事务推进 revision"
    );
    assert!(!c.is_dirty(), "adopt 后缓冲 == 外部内容 → clean");
}

#[test]
fn core_dirty_via_session_get_mut_feeds_plan_reload() {
    // 会话面挂钩：dirty_windows 查询 = plan_reload(local_dirty) 的权威取值
    let mut sm = SessionManager::default();
    sm.open(W(1), session_with_doc("clean-doc", 1)).unwrap();
    sm.open(W(2), session_with_doc("dirty-doc", 2)).unwrap();
    sm.open(W(3), session_with_doc("clean-doc-2", 3)).unwrap();
    type_a_char(sm.get_mut(W(2)).unwrap());
    assert_eq!(sm.dirty_windows(), vec![W(2)], "BTreeMap 键序确定性");

    type_a_char(sm.get_mut(W(1)).unwrap());
    assert_eq!(
        sm.dirty_windows(),
        vec![W(1), W(2)],
        "键序（1 < 2）与打开序无关"
    );
}

// ---------------------------------------------------------------------------
// 7/8/9. 会话记录：快照 / 往返 / 焦点序 / 未来 schema / 确定性
// ---------------------------------------------------------------------------

#[test]
fn session_record_roundtrip_preserves_focus_order_and_skips_pathless() {
    let mut sm = SessionManager::default();
    let mut s1 = session_with_doc("one", 1);
    s1.path = Some("/notes/one.md".into());
    let mut s2 = session_with_doc("two", 2);
    s2.path = Some("/notes/two.md".into());
    let anon = DocumentSession::new(Core::from_markdown("scratch")); // 无路径
    sm.open(W(10), s1).unwrap();
    sm.open(W(20), s2).unwrap();
    sm.open(W(30), anon).unwrap();
    sm.focus(W(10)).unwrap(); // 焦点序：20, 30, 10

    let rec = sm.snapshot_record();
    // 无路径会话（未另存新文档）不入恢复集
    assert_eq!(rec.windows.len(), 2, "pathless 窗口被跳过");
    assert_eq!(rec.windows[0].window, 20, "焦点序（旧→新）");
    assert_eq!(rec.windows[1].window, 10);
    assert_eq!(rec.windows[1].document_id.as_deref(), Some(s1_id_hex(&rec)));
    assert_eq!(rec.windows[0].path, "/notes/two.md");

    // JSON 往返
    let json = rec.to_json();
    let back = SessionRecord::from_json(&json).unwrap();
    assert_eq!(back, rec);
    assert!(json.contains("\"schema_version\":1"));

    // 宿主恢复语义锚：按序重开，末位获得最终焦点
    let order: Vec<u64> = back.windows.iter().map(|w| w.window).collect();
    assert_eq!(order, vec![20, 10], "恢复顺序 = 焦点序");
}

fn s1_id_hex(rec: &SessionRecord) -> &str {
    // 辅助：取出 10 号窗口记录里的 document_id（比较用）
    rec.windows
        .iter()
        .find(|w| w.window == 10)
        .and_then(|w| w.document_id.as_deref())
        .expect("window 10 carries document_id")
}

#[test]
fn future_session_record_rejected_and_malformed_rejected() {
    let future = r#"{"schema_version":2,"windows":[]}"#;
    let err = SessionRecord::from_json(future).unwrap_err();
    assert!(
        err.contains("unsupported session record schema"),
        "错误面可诊断：{err}"
    );

    let malformed = "{not json";
    assert!(SessionRecord::from_json(malformed).is_err());

    // 本版本记录仍可解析
    let current =
        r#"{"schema_version":1,"windows":[{"window":1,"document_id":null,"path":"/a.md"}]}"#;
    let rec = SessionRecord::from_json(current).unwrap();
    assert_eq!(rec.windows.len(), 1);
    assert_eq!(
        rec.windows[0].document_id, None,
        "匿名开档记录（meta sidecar 缺席场景）"
    );
}

#[test]
fn session_record_json_is_deterministic() {
    let mut sm = SessionManager::default();
    let mut sa = session_with_doc("a", 1);
    sa.path = Some("/a.md".into());
    let mut sb = session_with_doc("b", 2);
    sb.path = Some("/b.md".into());
    sm.open(W(1), sa).unwrap();
    sm.open(W(2), sb).unwrap();

    let j1 = sm.snapshot_record().to_json();
    let j2 = sm.snapshot_record().to_json();
    assert_eq!(j1, j2, "同态两次快照逐字节相同（字段序 = 结构序）");

    // 状态变更后输出确实变化（防假确定性）
    sm.focus(W(1)).unwrap();
    let j3 = sm.snapshot_record().to_json();
    assert_ne!(j1, j3, "焦点序变化反映到记录序");
}

// ---------------------------------------------------------------------------
// 10. 开档构造器与会话管理器的集成（open_document → with_path → 恢复链路）
// ---------------------------------------------------------------------------

#[test]
fn open_document_feeds_session_restore_pipeline() {
    // 宿主恢复链路的核侧环节：record.path → open_document(md, meta) →
    // with_path → SessionManager::open（身份复原 + 一窗一档一致性）
    let mut core = Core::from_markdown("persisted body\n");
    core.set_document_id(Some(DocumentId::generate(1_700_000_000_000)))
        .unwrap();
    let (md, meta) = core.save_document();

    let mut sm = SessionManager::default();
    let session = open_document(&md, Some(&meta))
        .unwrap()
        .with_path("/restored/note.md");
    let restored_id = session.id;
    sm.open(W(1), session).unwrap();
    assert_eq!(
        sm.window_of_document(restored_id.as_ref().unwrap()),
        Some(W(1))
    );

    // 同一 meta 再开（宿主 record 与 meta sidecar 双源同档）→ 一窗一档拒绝
    let dup = open_document(&md, Some(&meta))
        .unwrap()
        .with_path("/restored/note.md");
    assert!(matches!(
        sm.open(W(2), dup),
        Err(SessionError::DocumentAlreadyOpen { .. })
    ));
}
