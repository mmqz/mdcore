//! op 矩阵：七个原子 op 逐面过桥（形状/身份/undo/约束镜像/自愈）。
//!
//! 核心方法：同一 op 序列双跑——(a) 过桥（shadow → collab）与
//! (b) 裸 mdcore 树直施——两树覆盖等价断言**钉死翻译语义不改写**
//! （MoveBlock 的 to_index 两侧坐标系若有差，此断言即红）。

mod common;

use common::*;
use mdc_collab::Actor;
use mdc_mdbridge::{parse_family_id, BridgeError, MdCollabHost, TranslateError};
use mdcore::op;
use mdcore::op::Op;
use mdcore::tree::{BlockKind, BlockTree};

fn must_op(h: &mut MdCollabHost, o: &mut Op) -> mdc_collab::CommandReceipt {
    h.apply_op(Actor::Human, o).expect("apply_op")
}

fn must_bare(t: &mut BlockTree, o: &mut Op) {
    op::apply_op(t, o).expect("bare apply");
}

/// 双跑 + 覆盖等价（含 BlockId 逐位——family u64 在 shadow 与裸树
/// 同为 mdcore 分配器语义，插入序一致时逐位相同）。
fn dual_run(seq: Vec<Op>, ctx: &str) -> (MdCollabHost, BlockTree) {
    let mut h = host(7_300_000);
    let mut bare = BlockTree::new();
    for o in seq {
        must_op(&mut h, &mut o.clone());
        let mut o2 = o;
        must_bare(&mut bare, &mut o2);
    }
    assert_trees_coverage_eq(h.tree(), &bare, ctx);
    (h, bare)
}

#[test]
fn insert_toplevel_nested_and_id_writeback() {
    let mut h = host(7_300_001);
    // 顶层插入（现场分配 → op.id 回写 → family id）。
    let mut o = insert_text(vec![], 0, BlockKind::Blockquote, "");
    let r = must_op(&mut h, &mut o);
    let bid = match r.outcome {
        mdc_collab::CommandOutcome::BlockInserted { block_id } => block_id,
        o => panic!("outcome {o:?}"),
    };
    let (c, _) = parse_family_id(&bid).expect("family");
    assert_eq!(c, 7_300_001);
    // op.id 回写（物化 u64——mdcore apply_op 语义延续）。
    match o {
        Op::InsertBlock { id, .. } => assert_eq!(id, Some(1)),
        _ => unreachable!(),
    }
    // 嵌套插入（父 = blockquote 路径解析）。
    let parent = h.path_of(&bid).expect("path");
    let mut o = insert_text(parent.clone(), 0, BlockKind::Paragraph, "子段");
    let r2 = must_op(&mut h, &mut o);
    assert!(matches!(
        r2.outcome,
        mdc_collab::CommandOutcome::BlockInserted { .. }
    ));
    assert_eq!(md(&h), "> 子段");
    let doc = h.session().document();
    assert_eq!(doc.len(), 1);
    assert_eq!(doc[0].children.len(), 1);
    assert_eq!(doc[0].children[0].text, "子段");
}

#[test]
fn replace_text_char_domain_and_bad_range() {
    let seq = vec![
        insert_text(vec![], 0, BlockKind::Paragraph, "你好世界"),
        Op::ReplaceText {
            path: vec![0],
            start: 1,
            end: 3,
            insert: "个".into(),
        },
    ];
    let (mut h, _) = dual_run(seq, "replace text");
    assert_eq!(md(&h), "你个界");
    // 越界 → mdcore BadRange（plan 通过[carries_text 面合法]、shadow
    // 原子拒、collab 零执行）。
    let mut o = Op::ReplaceText {
        path: vec![0],
        start: 0,
        end: 99,
        insert: "x".into(),
    };
    let err = h.apply_op(Actor::Human, &mut o).unwrap_err();
    assert!(matches!(err, BridgeError::Md(_)));
    assert_eq!(md(&h), "你个界", "越界零副作用");
}

#[test]
fn set_type_attrs_mark_dual() {
    let seq = vec![
        insert_text(vec![], 0, BlockKind::Paragraph, "标题文本"),
        Op::SetBlockType {
            path: vec![0],
            kind: BlockKind::Heading,
        },
        Op::SetAttrs {
            path: vec![0],
            attrs: [("level".to_string(), "3".to_string())]
                .into_iter()
                .collect(),
        },
        Op::SetMark {
            path: vec![0],
            start: 0,
            end: 3,
            kind: "bold".into(),
            add: true,
        },
        Op::SetMark {
            path: vec![0],
            start: 2,
            end: 4,
            kind: "bold".into(),
            add: true,
        },
    ];
    let (mut h, bare) = dual_run(seq, "set type/attrs/mark");
    assert_eq!(md(&h), md_of(&bare));
    assert_eq!(md(&h), "### 标题文本");
    // shadow 增量 vs 全量重物化：覆盖等价（D-2 字面形状允许差）。
    let incremental = h.tree().clone();
    h.materialize().expect("rematerialize");
    assert_trees_coverage_eq(&incremental, h.tree(), "D-2 覆盖等价");
    // 协作域 D-1：载体身份换新族、block_id 连续；shadow u64 稳定。
    let doc = h.session().document();
    assert_eq!(doc[0].kind, mdc_collab::BlockKind::Heading);
    assert_eq!(h.tree().node(h.tree().top_level()[0]).id.0, 1);
}

#[test]
fn move_block_same_parent_all_positions() {
    // 同父搬运全位钉定：to_index ∈ {0, 1, 2}（[P1,P2,P3] 搬 P2）。
    for to in [0usize, 1, 2] {
        let seq = vec![
            insert_text(vec![], 0, BlockKind::Paragraph, "P1"),
            insert_text(vec![], 1, BlockKind::Paragraph, "P2"),
            insert_text(vec![], 2, BlockKind::Paragraph, "P3"),
            Op::MoveBlock {
                from: vec![1],
                to_parent: vec![],
                to_index: to,
            },
        ];
        let (h, bare) = dual_run(seq, &format!("same-parent move to={to}"));
        assert_eq!(md(&h), md_of(&bare), "过桥与裸树同序");
    }
    // 显式期望锚定（to_index 语义的可读面）：
    // to=0 → [P2,P1,P3]；to=1 → [P1,P2,P3]（原位）；to=2 → [P1,P3,P2]。
    let (h, _) = dual_run(
        vec![
            insert_text(vec![], 0, BlockKind::Paragraph, "P1"),
            insert_text(vec![], 1, BlockKind::Paragraph, "P2"),
            insert_text(vec![], 2, BlockKind::Paragraph, "P3"),
            Op::MoveBlock {
                from: vec![1],
                to_parent: vec![],
                to_index: 0,
            },
        ],
        "move to 0",
    );
    assert_eq!(md(&h), "P2\nP1\nP3");
    let (h, _) = dual_run(
        vec![
            insert_text(vec![], 0, BlockKind::Paragraph, "P1"),
            insert_text(vec![], 1, BlockKind::Paragraph, "P2"),
            insert_text(vec![], 2, BlockKind::Paragraph, "P3"),
            Op::MoveBlock {
                from: vec![1],
                to_parent: vec![],
                to_index: 2,
            },
        ],
        "move to 2",
    );
    assert_eq!(md(&h), "P1\nP3\nP2");
}

#[test]
fn move_block_cross_parent_and_back() {
    let seq = vec![
        insert_text(vec![], 0, BlockKind::Paragraph, "P1"),
        insert_text(vec![], 1, BlockKind::Paragraph, "P2"),
        // 建引用容器 + 把 P2 移入。
        insert_text(vec![], 2, BlockKind::Blockquote, ""),
        Op::MoveBlock {
            from: vec![1],
            to_parent: vec![2],
            to_index: 0,
        },
        // 再移回顶层末位（首搬后 bq 在 top 位 1 → 子路径 [1, 0]）。
        Op::MoveBlock {
            from: vec![1, 0],
            to_parent: vec![],
            to_index: 1,
        },
    ];
    let (h, bare) = dual_run(seq, "cross-parent move");
    assert_eq!(md(&h), md_of(&bare));
    // 显式：[P1, P2, 空 bq]（空 bq 渲染为空行）。
    assert_eq!(md(&h), "P1\nP2\n");
}

#[test]
fn remove_then_undo_identity_continuity() {
    let mut h = host(7_300_005);
    let mut o = insert_text(vec![], 0, BlockKind::Paragraph, "正文甲");
    let r = must_op(&mut h, &mut o);
    let bid = match r.outcome {
        mdc_collab::CommandOutcome::BlockInserted { block_id } => block_id,
        _ => unreachable!(),
    };
    assert_eq!(h.undo_depth(Actor::Human), 1);
    // 删除 → 会话 + shadow 双消失；undo 深度 +1。
    let mut o = Op::RemoveBlock { path: vec![0] };
    let r = must_op(&mut h, &mut o);
    assert!(matches!(
        r.outcome,
        mdc_collab::CommandOutcome::BlockRemoved { .. }
    ));
    assert!(h.session().document().is_empty());
    assert_eq!(md(&h), "");
    assert_eq!(h.undo_depth(Actor::Human), 2);
    // 撤销 → block_id 连续（同一字符串回归）+ shadow u64 同一。
    assert!(h.undo(Actor::Human).expect("undo"));
    let doc = h.session().document();
    assert_eq!(doc.len(), 1);
    assert_eq!(doc[0].block_id, bid, "撤销复原：block_id 连续");
    assert_eq!(doc[0].text, "正文甲");
    assert_eq!(
        h.tree().node(h.tree().top_level()[0]).id.0,
        1,
        "shadow u64 同一"
    );
    // redo。
    assert!(h.redo(Actor::Human).expect("redo"));
    assert!(h.session().document().is_empty());
}

#[test]
fn origin_split_stacks_dual_actor() {
    let mut h = host(7_300_006);
    let mut o = insert_text(vec![], 0, BlockKind::Paragraph, "人工块");
    h.apply_op(Actor::Human, &mut o).expect("human insert");
    let mut o = insert_text(vec![], 1, BlockKind::Paragraph, "AI 块");
    h.apply_op(Actor::Ai, &mut o).expect("ai insert");
    assert_eq!(h.undo_depth(Actor::Human), 1);
    assert_eq!(h.undo_depth(Actor::Ai), 1);
    // 人工撤销只撤人工面。
    assert!(h.undo(Actor::Human).expect("undo human"));
    let doc = h.session().document();
    assert_eq!(doc.len(), 1);
    assert_eq!(doc[0].text, "AI 块");
}

#[test]
fn container_and_text_face_prechecks() {
    let mut h = host(7_300_007);
    let mut o = insert_text(vec![], 0, BlockKind::Heading, "标题");
    must_op(&mut h, &mut o);
    let before = md(&h);
    let before_doc = h.session().document();

    // Heading 为父 → NotAContainer（协作域容器面镜像）。
    let mut o = insert_text(vec![0], 0, BlockKind::Paragraph, "子段");
    let err = h.apply_op(Actor::Human, &mut o).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(TranslateError::NotAContainer(_))
    ));
    assert_eq!(md(&h), before, "预检零副作用：shadow");
    assert_eq!(h.session().document(), before_doc, "预检零副作用：会话");

    // hr 无文本面（插入带文本 / ReplaceText / SetMark 三路）。
    let mut o = insert_text(vec![], 1, BlockKind::ThematicBreak, "带文本");
    let err = h.apply_op(Actor::Human, &mut o).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(TranslateError::KindIncompatible(_))
    ));
    // （先建一个合法 hr 再打文本面。）
    let mut o = insert_text(vec![], 1, BlockKind::ThematicBreak, "");
    must_op(&mut h, &mut o);
    let mut o = Op::ReplaceText {
        path: vec![1],
        start: 0,
        end: 0,
        insert: "x".into(),
    };
    let err = h.apply_op(Actor::Human, &mut o).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(TranslateError::KindIncompatible(_))
    ));
    let mut o = Op::SetMark {
        path: vec![1],
        start: 0,
        end: 1,
        kind: "bold".into(),
        add: true,
    };
    let err = h.apply_op(Actor::Human, &mut o).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(TranslateError::KindIncompatible(_))
    ));

    // 保留属性键 + 空 mark。
    let mut o = Op::SetAttrs {
        path: vec![0],
        attrs: [("block_id".to_string(), "z".to_string())]
            .into_iter()
            .collect(),
    };
    let err = h.apply_op(Actor::Human, &mut o).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(TranslateError::ReservedAttrKey)
    ));
    let mut o = Op::SetMark {
        path: vec![0],
        start: 0,
        end: 1,
        kind: "".into(),
        add: true,
    };
    let err = h.apply_op(Actor::Human, &mut o).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(TranslateError::EmptyMark)
    ));

    // 全程零副作用复核。
    assert_eq!(md(&h), "# 标题\n---");
    assert_eq!(h.undo_depth(Actor::Human), 2, "仅合法两插入成栈");
}

#[test]
fn shadow_full_rematerialize_equivalence_after_mixed_ops() {
    let seq = vec![
        insert_text(vec![], 0, BlockKind::Paragraph, "第一段"),
        insert_text(vec![], 1, BlockKind::Blockquote, ""),
        insert_text(vec![1], 0, BlockKind::Paragraph, "引用内"),
        insert_text(vec![], 2, BlockKind::ListItem, "列表项"),
        Op::SetMark {
            path: vec![0],
            start: 0,
            end: 2,
            kind: "bold".into(),
            add: true,
        },
        Op::SetBlockType {
            path: vec![0],
            kind: BlockKind::Heading,
        },
        Op::MoveBlock {
            from: vec![2],
            to_parent: vec![1],
            to_index: 1,
        },
        Op::ReplaceText {
            path: vec![0],
            start: 0,
            end: 1,
            insert: "第".into(),
        },
    ];
    let (mut h, _) = dual_run(seq.clone(), "mixed pre");
    // shadow 增量投影 vs 全量重物化。
    let incremental = h.tree().clone();
    h.materialize().expect("rematerialize");
    assert_trees_coverage_eq(&incremental, h.tree(), "增量 vs 全量重物化");
    // 重物化后再跑一轮（物化态继续可编辑——family/foreign 均可寻址）。
    let mut o = Op::ReplaceText {
        path: vec![0],
        start: 0,
        end: 1,
        insert: "头".into(),
    };
    must_op(&mut h, &mut o);
    assert!(md(&h).starts_with("# 头"), "got {}", md(&h));
}

// ---------------------------------------------------------------------------
// #261：Math 块过桥（协作域 tag "math" 双射 + 重物化 kind 保真）
// ---------------------------------------------------------------------------

#[test]
fn math_block_bridge_roundtrip_kind_preserved() {
    // mdcore Math 过桥 → 协作域元素 tag = "math" → 全量重物化回 Math。
    let mut h = host(7_300_777);
    let mut o = insert_text(vec![], 0, BlockKind::Math, "\\int_0^1 x\\,dx");
    let r = must_op(&mut h, &mut o);
    let bid = match r.outcome {
        mdc_collab::CommandOutcome::BlockInserted { block_id } => block_id,
        o => panic!("outcome {o:?}"),
    };
    // 协作域读回：kind = Math（wire 元素 tag math → from_tag 双射）。
    let doc = h.session().document();
    assert_eq!(doc.len(), 1);
    assert_eq!(doc[0].kind, mdc_collab::BlockKind::Math);
    assert_eq!(doc[0].text, "\\int_0^1 x\\,dx");
    // shadow 增量投影（kind + 文本）。
    assert_eq!(md(&h), "$$\n\\int_0^1 x\\,dx\n$$");
    // 全量重物化：kind 保真（增量 vs 重物化覆盖等价）。
    let incremental = h.tree().clone();
    h.materialize().expect("rematerialize");
    assert_trees_coverage_eq(&incremental, h.tree(), "math rematerialize");
    assert_eq!(h.tree().node(h.tree().top_level()[0]).kind, BlockKind::Math);
    // block_id 稳定（family 身份跨物化不变）。
    assert!(h.path_of(&bid).is_some(), "family 身份跨物化稳定");
}

#[test]
fn math_block_dual_run_matches_bare_tree() {
    // 双跑对照：Math 插入过桥 vs 裸树直施——覆盖等价（翻译不改写）。
    let seq = vec![
        insert_text(vec![], 0, BlockKind::Paragraph, "前文"),
        insert_text(vec![], 1, BlockKind::Math, "E=mc^2"),
        Op::ReplaceText {
            path: vec![1],
            start: 0,
            end: 0,
            insert: "公式：".into(),
        },
    ];
    let (h, bare) = dual_run(seq, "math dual run");
    assert_eq!(md(&h), md_of(&bare));
    assert_eq!(md(&h), "前文\n$$\n公式：E=mc^2\n$$");
}
