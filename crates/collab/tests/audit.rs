//! 外部 wire 数据审计与修复面测试（交付报告 §4.2 撤销耦合防护收口）。
//!
//! 钉定面：
//! - 本引擎作为唯一写入方 = 规范形状（全操作矩阵后 audit_self
//!   Error/Warning 双零——含 set_block_type 承载面守卫后的全转换域）。
//! - 外部数据全违规类逐项检出（Error 四类 + Warning 十类）。
//! - 审计零副作用（暂存 Doc 隔离）+ 限额/解码 fail-closed。
//! - repair：撤销耦合防护端到端证明（对照组并发内容丢失 vs 修复组
//!   存活——§4.2 危害与收口双面钉定）；幂等/栈零污染/事件可转发。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mdc_collab::{
    Actor, AuditViolation, BlockKind, BlockSpec, ChangeSource, CollabCommand, CollabError,
    CollabSession, SessionOptions,
};
use yrs::types::xml::{XmlElementPrelim, XmlIn, XmlTextPrelim};
use yrs::{
    Array, ClientID, Doc, Map, Options as DocOptions, ReadTxn, StateVector, Transact, XmlFragment,
};

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

fn spec(kind: BlockKind, text: &str) -> BlockSpec {
    BlockSpec {
        kind,
        attrs: BTreeMap::new(),
        text: text.into(),
    }
}

/// 外部载体构造（绕过本引擎直写 wire——敌意/迁移形状的唯一通道）。
fn external_doc(client_id: u64) -> Doc {
    let o = DocOptions {
        skip_gc: true,
        client_id: ClientID::new(client_id),
        ..Default::default()
    };
    Doc::with_options(o)
}

/// 外部 Doc 全量编码（对端无状态面的 update）。
fn full_update(doc: &Doc) -> Vec<u8> {
    doc.transact()
        .encode_state_as_update_v1(&StateVector::default())
}

/// 外部元素便捷构造：tag + block_id + 可选子节点。
fn elem(tag: &str, block_id: Option<&str>, children: Vec<XmlIn>) -> XmlElementPrelim {
    let mut e = XmlElementPrelim::empty(tag);
    if let Some(id) = block_id {
        e.attributes.insert("block_id".into(), id.to_string());
    }
    e.children.extend(children);
    e
}

fn text_child(s: &str) -> XmlIn {
    XmlIn::Text(XmlTextPrelim::new(s).into())
}

// ── 1. 本引擎规范形状（全操作矩阵）──────────────────────────────

#[test]
fn self_audit_clean_after_full_op_matrix() {
    let mut s = CollabSession::open(opts(11)).unwrap();
    // 九 kind 装载 + 容器嵌套。
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![
                spec(BlockKind::Heading, "标题"),
                spec(BlockKind::Paragraph, "正文"),
                spec(BlockKind::CodeBlock, "code"),
                spec(BlockKind::ThematicBreak, ""),
                spec(BlockKind::Blockquote, "引用首段"),
            ],
        },
    )
    .unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: Some(s.top_level_ids()[4].clone()),
            index: 0,
            spec: spec(BlockKind::ListItem, "列表项"),
            id: None,
        },
    )
    .unwrap();
    // 文本编辑 + marks（重叠归一）。
    let p_id = &s.document()[1].block_id;
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: p_id.clone(),
            start: 0,
            end: 0,
            insert: "前缀 ".into(),
        },
    )
    .unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::SetMark {
            block_id: p_id.clone(),
            start: 0,
            end: 3,
            mark: "em".into(),
            add: true,
        },
    )
    .unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::SetMark {
            block_id: p_id.clone(),
            start: 1,
            end: 4,
            mark: "strong".into(),
            add: true,
        },
    )
    .unwrap();
    // 类型转换（承载面守卫后的合法域）+ 撤销 + 属性。
    let h_id = s.document()[0].block_id.clone();
    s.execute(
        Actor::Human,
        CollabCommand::SetBlockType {
            block_id: h_id.clone(),
            kind: BlockKind::Paragraph,
        },
    )
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::SetAttrs {
            block_id: h_id.clone(),
            attrs: BTreeMap::from([("lang".into(), "zh".into())]),
        },
    )
    .unwrap();
    assert!(s.undo(Actor::Ai)); // 撤一笔 mark（归一化重建路径）
                                // 审计：Error/Warning 双零 + 计数面。
    let r = s.audit_self();
    assert!(r.is_clean(), "errors: {:?}", r.errors);
    assert!(r.warnings.is_empty(), "warnings: {:?}", r.warnings);
    assert_eq!(r.blocks_total, 6);
    assert!(r.marks_total >= 1);
}

/// set_block_type 承载面守卫（audit 走查发现的形状外泄缺口——修复钉定）。
#[test]
fn set_block_type_carries_invariant_guards() {
    let mut s = CollabSession::open(opts(12)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "有文本"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    // 带文本 → hr：拒（文本无去处——镜像 insert 时守卫）。
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::SetBlockType {
                block_id: "p-1".into(),
                kind: BlockKind::ThematicBreak
            }
        ),
        Err(CollabError::KindIncompatible { op: "text", .. })
    ));
    // 容器带子块 → 非容器：拒。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec(BlockKind::Blockquote, ""),
            id: Some("bq-1".into()),
        },
    )
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: Some("bq-1".into()),
            index: 0,
            spec: spec(BlockKind::Paragraph, "子"),
            id: None,
        },
    )
    .unwrap();
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::SetBlockType {
                block_id: "bq-1".into(),
                kind: BlockKind::Paragraph
            }
        ),
        Err(CollabError::KindIncompatible { op: "children", .. })
    ));
    // hr → p：转换后出生文本节点（撤销耦合防护——audit Error 零）。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 2,
            spec: spec(BlockKind::ThematicBreak, ""),
            id: Some("hr-1".into()),
        },
    )
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::SetBlockType {
            block_id: "hr-1".into(),
            kind: BlockKind::Paragraph,
        },
    )
    .unwrap();
    // 空 p → hr：允许（空承载丢弃）且形状规范（hr 无文本节点）。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 3,
            spec: spec(BlockKind::Paragraph, ""),
            id: Some("p-2".into()),
        },
    )
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::SetBlockType {
            block_id: "p-2".into(),
            kind: BlockKind::ThematicBreak,
        },
    )
    .unwrap();
    let r = s.audit_self();
    assert!(r.is_clean(), "errors: {:?}", r.errors);
    assert!(r.warnings.is_empty(), "warnings: {:?}", r.warnings);
    assert_eq!(s.find_block("hr-1").unwrap().text, "");
    assert_eq!(s.find_block("p-2").unwrap().kind, BlockKind::ThematicBreak);
}

// ── 2. 对端干净往返 ─────────────────────────────────────────────

#[test]
fn audit_update_clean_peer_roundtrip() {
    let mut peer = CollabSession::open(opts(21)).unwrap();
    peer.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![
                spec(BlockKind::Heading, "Peer 标题"),
                spec(BlockKind::Paragraph, "Peer 正文"),
                spec(BlockKind::ThematicBreak, ""),
            ],
        },
    )
    .unwrap();
    peer.execute(
        Actor::Ai,
        CollabCommand::SetMark {
            block_id: peer.top_level_ids()[1].clone(),
            start: 0,
            end: 4,
            mark: "em".into(),
            add: true,
        },
    )
    .unwrap();
    // 全量差量（对空 SV）：本引擎产出必为规范形状。
    let fresh = CollabSession::open(opts(22)).unwrap();
    let diff = peer.encode_diff(&fresh.state_vector()).unwrap();
    let local = CollabSession::open(opts(23)).unwrap();
    let r = local.audit_update(&diff).unwrap();
    assert!(r.is_clean(), "errors: {:?}", r.errors);
    assert!(r.warnings.is_empty(), "warnings: {:?}", r.warnings);
    assert_eq!(r.blocks_total, 3);
    assert_eq!(r.marks_total, 1);
    // 应用后本端自审亦净。
    local.apply_update(&diff).unwrap();
    let r2 = local.audit_self();
    assert!(r2.is_clean());
    assert!(r2.warnings.is_empty());
}

// ── 3. Error 级违规检出 ─────────────────────────────────────────

/// 撤销耦合防护破坏面：外部块出生无文本节点。
fn dirty_no_text_node_update() -> Vec<u8> {
    let doc = external_doc(901);
    {
        let frag = doc.get_or_insert_xml_fragment("doc");
        let mut txn = doc.transact_mut();
        frag.insert(&mut txn, 0, elem("p", Some("X"), vec![])); // 无文本子节点
        txn.commit();
    } // p18：commit 不释放锁，Drop 才释放——出域后再读
    full_update(&doc)
}

#[test]
fn audit_update_detects_missing_text_node() {
    let s = CollabSession::open(opts(31)).unwrap();
    let r = s.audit_update(&dirty_no_text_node_update()).unwrap();
    assert!(!r.is_clean());
    assert_eq!(
        r.errors,
        vec![AuditViolation::MissingTextNode {
            block_id: "X".into(),
            tag: "p".into()
        }]
    );
    assert_eq!(r.blocks_total, 1);
    // 稳定可标记块不触发（hr 无文本面为规范形状）。
    let doc = external_doc(902);
    {
        let frag = doc.get_or_insert_xml_fragment("doc");
        let mut txn = doc.transact_mut();
        frag.insert(&mut txn, 0, elem("hr", Some("H"), vec![]));
        txn.commit();
    }
    let r2 = s.audit_update(&full_update(&doc)).unwrap();
    assert!(r2.is_clean());
    assert!(r2.warnings.is_empty());
}

#[test]
fn audit_update_detects_identity_violations() {
    let s = CollabSession::open(opts(32)).unwrap();
    let doc = external_doc(903);
    {
        let frag = doc.get_or_insert_xml_fragment("doc");
        let mut txn = doc.transact_mut();
        // 缺 block_id / 空 block_id / 重复 block_id。
        frag.insert(&mut txn, 0, elem("p", None, vec![text_child("a")]));
        let mut empty_id = elem("p", Some(""), vec![text_child("b")]);
        empty_id.attributes.insert("block_id".into(), String::new());
        frag.insert(&mut txn, 1, empty_id);
        frag.insert(&mut txn, 2, elem("p", Some("dup"), vec![text_child("c")]));
        frag.insert(&mut txn, 3, elem("h", Some("dup"), vec![text_child("d")]));
        txn.commit();
    }
    let r = s.audit_update(&full_update(&doc)).unwrap();
    assert_eq!(
        r.errors,
        vec![
            AuditViolation::MissingBlockId { tag: "p".into() },
            AuditViolation::MissingBlockId { tag: "p".into() },
            AuditViolation::DuplicateBlockId {
                block_id: "dup".into()
            },
        ]
    );
    assert_eq!(r.blocks_total, 4);
}

#[test]
fn audit_update_detects_shape_drift_violations() {
    let s = CollabSession::open(opts(33)).unwrap();
    let doc = external_doc(904);
    {
        let frag = doc.get_or_insert_xml_fragment("doc");
        let mut txn = doc.transact_mut();
        // 根级裸文本。
        frag.insert(&mut txn, 0, text_child("root stray")); // XmlIn → frag 插 Text
                                                            // hr 带文本。
        frag.insert(&mut txn, 1, elem("hr", Some("H"), vec![text_child("t")]));
        // 未知标签。
        frag.insert(
            &mut txn,
            2,
            elem("custom", Some("U"), vec![text_child("u")]),
        );
        // p 带非规范属性键 + mark 缺 kind + mark 内异质子元素 + 死承载文本。
        let mut p = elem("p", Some("P"), vec![]);
        p.attributes.insert("a b".into(), "非规范键".into());
        p.children
            .push(XmlIn::Element(elem("m", None, vec![text_child("mk")])));
        p.children
            .push(XmlIn::Element(elem("m", Some("m2"), vec![])));
        let mut m3 = XmlElementPrelim::empty("m");
        m3.attributes.insert("kind".into(), "em".into());
        m3.children.push(text_child("ok"));
        m3.children.push(XmlIn::Element(elem(
            "p",
            Some("in-mark"),
            vec![text_child("x")],
        )));
        p.children.push(XmlIn::Element(m3));
        p.children.push(XmlIn::Element(elem(
            "li",
            Some("child"),
            vec![text_child("子块")],
        ))); // 非容器挂载
        p.children.push(text_child("after children")); // 死承载
        frag.insert(&mut txn, 3, XmlIn::Element(p));
        txn.commit();
    }
    let r = s.audit_update(&full_update(&doc)).unwrap();
    // Error：p 无 own-text 直接承载？——p 有 mark 内文本（嵌套），无直接
    // MissingTextNode（耦合面以嵌套节点计，见模块文档口径）。
    let has = |v: &AuditViolation| r.warnings.contains(v);
    assert!(has(&AuditViolation::RootLevelText));
    assert!(has(&AuditViolation::ThematicBreakWithText {
        block_id: "H".into()
    }));
    assert!(has(&AuditViolation::UnknownTag {
        tag: "custom".into()
    }));
    assert!(has(&AuditViolation::NonCanonicalAttrKey {
        key: "a b".into()
    }));
    assert!(has(&AuditViolation::MarkMissingKind {
        block_id: "P".into()
    }));
    assert!(has(&AuditViolation::MarkForeignChild {
        block_id: "P".into()
    }));
    assert!(has(&AuditViolation::NestedUnderNonContainer {
        parent_block_id: "P".into(),
        parent_tag: "p".into(),
        child_block_id: "child".into()
    }));
    assert!(has(&AuditViolation::TextAfterChildren {
        block_id: "P".into()
    }));
    assert!(
        r.is_clean(),
        "Error 级应空（形状漂移为 Warning）: {:?}",
        r.errors
    );
    assert_eq!(r.blocks_total, 4); // H + U + P + child（根文本不计）
    assert_eq!(r.marks_total, 3);
}

// ── 4. 外域根 / 根类型混淆 ─────────────────────────────────────

#[test]
fn audit_update_detects_foreign_roots() {
    let s = CollabSession::open(opts(34)).unwrap();
    let doc = external_doc(905);
    // 外域根（Map 承载数据）。
    {
        let map = doc.get_or_insert_map("other");
        let mut txn = doc.transact_mut();
        map.insert(&mut txn, "k", "v");
        txn.commit();
    }
    let r = s.audit_update(&full_update(&doc)).unwrap();
    assert!(r.is_clean());
    assert_eq!(
        r.warnings,
        vec![AuditViolation::ForeignRootData {
            name: "other".into()
        }]
    );
    assert_eq!(r.blocks_total, 0);

    // 根内容混淆：根 "doc" 为 Map（键值条目——读模型不可见 → Error）。
    let doc2 = external_doc(906);
    {
        let map = doc2.get_or_insert_map("doc");
        let mut txn = doc2.transact_mut();
        map.insert(&mut txn, "k", "v");
        txn.commit();
    }
    let r2 = s.audit_update(&full_update(&doc2)).unwrap();
    assert!(!r2.is_clean());
    assert_eq!(r2.errors, vec![AuditViolation::ForeignRootType]);

    // 根内容混淆：根 "doc" 为 Array（序列条目——children() 零产出，
    // 列表长差额判别 → Error）。
    let doc3 = external_doc(907);
    {
        let arr = doc3.get_or_insert_array("doc");
        let mut txn = doc3.transact_mut();
        arr.push_back(&mut txn, "str-item");
        txn.commit();
    }
    let r3 = s.audit_update(&full_update(&doc3)).unwrap();
    assert!(!r3.is_clean());
    assert_eq!(r3.errors, vec![AuditViolation::ForeignRootType]);
}

// ── 5. 隔离与 fail-closed ───────────────────────────────────────

#[test]
fn audit_update_isolation_and_fail_closed() {
    let mut s = CollabSession::open(opts(35)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec(BlockKind::Paragraph, "本端状态")],
        },
    )
    .unwrap();
    let before = s.document();
    let depth_before = s.undo_depth(Actor::Human);
    // 脏数据审计：零本端副作用。
    let r = s.audit_update(&dirty_no_text_node_update()).unwrap();
    assert!(!r.is_clean());
    assert_eq!(s.document(), before);
    assert_eq!(s.undo_depth(Actor::Human), depth_before);
    // 垃圾字节：Decode fail-closed。
    assert!(matches!(
        s.audit_update(&[0xFF, 0xEE, 0xDD]),
        Err(CollabError::Decode(_))
    ));
    // 超限：Limit（B1.6 同门）。
    let tight = CollabSession::open(SessionOptions {
        client_id: 36,
        input_byte_limit: 8,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        tight.audit_update(&[0u8; 64]),
        Err(CollabError::Limit { limit: 8, .. })
    ));
}

// ── 6. repair：端到端撤销耦合防护（§4.2 危害与收口双面）────────

/// 双端同步便捷（差量互发至收敛）。
fn sync(a: &CollabSession, b: &mut CollabSession) {
    let diff = a.encode_diff(&b.state_vector()).unwrap();
    b.apply_update(&diff).unwrap();
}

#[test]
fn repair_coupling_protection_end_to_end() {
    let ext = dirty_no_text_node_update();

    // ── 对照组（不修复）：undo 首笔 = 删整个节点含对端并发内容 ──
    let mut a = CollabSession::open(opts(41)).unwrap();
    let mut b = CollabSession::open(opts(42)).unwrap();
    a.apply_update(&ext).unwrap();
    b.apply_update(&ext).unwrap();
    a.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "X".into(),
            start: 0,
            end: 0,
            insert: "hello".into(),
        },
    )
    .unwrap();
    sync(&a, &mut b); // B 见 "hello"
    b.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "X".into(),
            start: 5,
            end: 5,
            insert: " world".into(),
        },
    )
    .unwrap();
    sync(&b, &mut a); // A 见 "hello world"
    assert_eq!(a.find_block("X").unwrap().text, "hello world");
    assert!(a.undo(Actor::Human)); // 撤销首笔（节点创建事务）
    assert_eq!(
        a.find_block("X").unwrap().text,
        "",
        "对照组：整节点删除（B 的并发内容随节点消失）"
    );
    sync(&a, &mut b);
    assert_eq!(
        b.find_block("X").unwrap().text,
        "",
        "对照组：删除传播对端——危害全编队"
    );

    // ── 修复组：repair 后 undo 恒只撤字符项，并发内容存活 ──
    let mut a2 = CollabSession::open(opts(43)).unwrap();
    let mut b2 = CollabSession::open(opts(44)).unwrap();
    a2.apply_update(&ext).unwrap();
    b2.apply_update(&ext).unwrap();
    // 修复 + 事件捕获转发（宿主转发链）。
    let repaired_updates: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(vec![]));
    let sink = Arc::clone(&repaired_updates);
    let _sub = a2.subscribe(move |e| {
        if matches!(e.source, ChangeSource::Maintenance) {
            sink.lock().unwrap().extend(e.updates.iter().cloned());
        }
    });
    assert_eq!(a2.repair_text_nodes().unwrap(), 1);
    let updates = repaired_updates.lock().unwrap().clone();
    assert!(!updates.is_empty(), "修复产物可转发（updates 非空）");
    for u in &updates {
        b2.apply_update(u).unwrap();
    }
    // 双端审计：耦合面 Error 清零。
    assert!(a2.audit_self().is_clean());
    assert!(b2.audit_self().is_clean());
    // 同编辑序列。
    a2.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "X".into(),
            start: 0,
            end: 0,
            insert: "hello".into(),
        },
    )
    .unwrap();
    sync(&a2, &mut b2);
    b2.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "X".into(),
            start: 5,
            end: 5,
            insert: " world".into(),
        },
    )
    .unwrap();
    sync(&b2, &mut a2);
    assert!(a2.undo(Actor::Human));
    assert_eq!(
        a2.find_block("X").unwrap().text,
        " world",
        "修复组：只撤自身字符，B 的并发内容存活"
    );
    sync(&a2, &mut b2);
    assert_eq!(b2.find_block("X").unwrap().text, " world");
}

#[test]
fn repair_semantics() {
    let mut s = CollabSession::open(opts(45)).unwrap();
    // 干净会话：零修复零事件。
    let events: Arc<Mutex<Vec<ChangeSource>>> = Arc::new(Mutex::new(vec![]));
    let sink = Arc::clone(&events);
    let _sub = s.subscribe(move |e| {
        sink.lock().unwrap().push(e.source);
    });
    // 干净会话：零修复零事件。
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec(BlockKind::Paragraph, "本体")],
        },
    )
    .unwrap();
    assert_eq!(s.repair_text_nodes().unwrap(), 0);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [ChangeSource::Command {
            actor: Actor::Human
        }],
        "干净会话零修复零事件（仅装载命令自身事件）"
    );
    // 脏数据并入后修复。
    s.apply_update(&dirty_no_text_node_update()).unwrap();
    assert!(!s.audit_self().is_clean());
    let depth_before = s.undo_depth(Actor::Human); // 装载命令自身栈项
    assert_eq!(s.repair_text_nodes().unwrap(), 1);
    assert!(s.audit_self().is_clean());
    // 幂等 + 撤销栈零污染（maintenance origin 不在 Human/Ai 追踪集
    // ——栈深恒为修复前值，含装载命令自身栈项）。
    assert_eq!(s.repair_text_nodes().unwrap(), 0);
    assert_eq!(s.undo_depth(Actor::Human), depth_before);
    assert_eq!(s.undo_depth(Actor::Ai), 0);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            ChangeSource::Command {
                actor: Actor::Human
            },
            ChangeSource::Remote,
            ChangeSource::Maintenance,
        ],
        "事件序：装载 → 远端并入 → 维护修复（一入口一事件）"
    );
    // 修复后块照常可编辑可撤销。
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "X".into(),
            start: 0,
            end: 0,
            insert: "内容".into(),
        },
    )
    .unwrap();
    assert!(s.undo(Actor::Human));
    assert_eq!(s.find_block("X").unwrap().text, "");
}
