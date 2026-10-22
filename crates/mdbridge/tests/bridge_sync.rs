//! 同步与身份面：双端 wire 收敛 / foreign 稳定映射 / 同族伪造 /
//! restore 换端 / 敌意空 id。

mod common;

use common::*;
use mdc_collab::{Actor, BlockKind as CKind, BlockSpec, CollabCommand, CollabSession};
use mdc_mdbridge::{family_id, MdCollabHost};
use mdcore::markdown;
use mdcore::op::Op;
use mdcore::tree::BlockKind;

const A: u64 = 7_400_001;
const B: u64 = 7_400_002;

fn raw_session(client_id: u64) -> CollabSession {
    CollabSession::open(opts(client_id)).expect("raw session")
}

/// B 侧裸会话对 A 注入一个带指定 block_id 的块（伪造通道）。
fn inject_fake_block(raw_b: &mut CollabSession, block_id: &str) {
    raw_b
        .execute(
            Actor::Ai,
            CollabCommand::InsertBlock {
                parent: None,
                index: 0,
                spec: BlockSpec {
                    kind: CKind::Paragraph,
                    attrs: Default::default(),
                    text: "伪造块".into(),
                },
                id: Some(block_id.to_string()),
            },
        )
        .expect("raw inject");
}

#[test]
fn two_host_convergence_and_cross_edit() {
    let mut a = host(A);
    let mut b = host(B);
    // A 发布 + 本地编辑。
    let src = markdown::markdown_to_tree("# 标题\n\n段落一");
    a.publish(&src, Actor::Human).expect("publish");
    let mut o = insert_text(vec![], 2, BlockKind::Paragraph, "A 追加段");
    a.apply_op(Actor::Human, &mut o).expect("a insert");
    sync(&mut a, &mut b);
    assert_trees_structure_eq(a.tree(), b.tree(), "A/B 双端收敛");
    assert_eq!(md(&a), md(&b));
    // B 本地编辑（foreign 面——A 的 family id 对 B 是 foreign）→ 回传 A。
    let target = b.session().document()[2].block_id.clone();
    let path = b.path_of(&target).expect("b resolves foreign");
    let mut o = Op::ReplaceText {
        path,
        start: 0,
        end: 1,
        insert: "甲".into(),
    };
    b.apply_op(Actor::Ai, &mut o).expect("b edit foreign");
    sync(&mut a, &mut b);
    assert_trees_structure_eq(a.tree(), b.tree(), "回传收敛");
    assert_eq!(md(&a), md(&b));
    assert!(md(&a).contains("甲 追加段"), "got {}", md(&a));
}

#[test]
fn foreign_mapping_stable_across_rematerialize_rounds() {
    let mut a = host(A);
    let mut b = host(B);
    let src = markdown::markdown_to_tree("# 标题\n\n段落一");
    a.publish(&src, Actor::Human).expect("publish");
    sync(&mut a, &mut b);
    // B 首轮物化：A 的 family id 全落 foreign 表。
    let snap1 = b.foreign_snapshot();
    assert_eq!(snap1.len(), 2, "两块 foreign");
    // 两轮空同步（重物化 ×2）→ 映射不变。
    sync(&mut a, &mut b);
    sync(&mut a, &mut b);
    let snap2 = b.foreign_snapshot();
    assert_eq!(snap1, snap2, "foreign 映射跨轮稳定");
    // 行为锚：B 对 foreign 块的本地编辑在多轮物化后仍可达。
    let target = b.session().document()[1].block_id.clone();
    let path = b.path_of(&target).expect("path");
    let mut o = Op::ReplaceText {
        path,
        start: 0,
        end: 2,
        insert: "段".into(),
    };
    b.apply_op(Actor::Ai, &mut o).expect("edit after rounds");
    sync(&mut a, &mut b);
    assert!(md(&a).contains("段落一") || md(&a).contains("段一"));
}

#[test]
fn fake_family_injection_isolated() {
    // （拆分实现——上一测试占位重写。）
    let mut a = host(A);
    let mut raw_b = raw_session(B);
    // A 建块（u64 1）。
    let mut o = insert_text(vec![], 0, BlockKind::Paragraph, "A-1");
    let r = a.apply_op(Actor::Human, &mut o).expect("a1");
    let a1_id = match r.outcome {
        mdc_collab::CommandOutcome::BlockInserted { block_id } => block_id,
        _ => unreachable!(),
    };
    // B 注入伪造 family id "md:{A}:999"（A 的命名空间、未分配的 n）。
    inject_fake_block(&mut raw_b, &family_id(A, 999));
    // 同步（裸会话 ↔ 宿主 A）。
    let sv_a = a.state_vector();
    let sv_b = raw_b.state_vector();
    a.apply_remote(&raw_b.encode_diff(&sv_a).expect("diff b→a"))
        .expect("a applies b");
    raw_b
        .apply_update(&a.encode_diff(&sv_b).expect("diff a→b"))
        .expect("b applies a");
    // A 物化：伪造块按 family 分类（u64 999 + 水位观察）。
    assert_eq!(a.session().document().len(), 2);
    assert_eq!(a.foreign_snapshot().len(), 0, "伪造 family 不入 foreign 表");
    let md_after = md(&a);
    assert!(md_after.contains("A-1"));
    assert!(md_after.contains("伪造块"));
    // A 后续本地插入：分配器永不复用 999（水位已观察）。
    let mut o = insert_text(vec![], 2, BlockKind::Paragraph, "A-new");
    let r = a.apply_op(Actor::Human, &mut o).expect("a new insert");
    let new_id = match r.outcome {
        mdc_collab::CommandOutcome::BlockInserted { block_id } => block_id,
        _ => unreachable!(),
    };
    let (_, n) = mdc_mdbridge::parse_family_id(&new_id).expect("family");
    assert_ne!(n, 999, "后续本地分配不复用伪造 n");
    assert_ne!(new_id, a1_id);
}

#[test]
fn snapshot_restore_same_and_changed_client() {
    let mut a = host(A);
    let src = markdown::markdown_to_tree("# 标题\n\n段落一\n\n- 项");
    a.publish(&src, Actor::Human).expect("publish");
    let snap = a.authoritative_snapshot().expect("snapshot");
    let before_md = md(&a);

    // 同 client 恢复：family u64 全保留（覆盖等价含 BlockId 逐位）。
    let mut r1 = MdCollabHost::restore(opts(A), &snap).expect("restore same");
    assert_trees_coverage_eq(r1.tree(), a.tree(), "同 client 恢复");
    assert_eq!(md(&r1), before_md);
    // 恢复后本地编辑可达（family 寻址）。
    let mut o = Op::ReplaceText {
        path: vec![1],
        start: 0,
        end: 2,
        insert: "段".into(),
    };
    r1.apply_op(Actor::Human, &mut o)
        .expect("edit after restore");

    // 换 client 恢复：原 family 落 foreign（身份换新族，结构保真）。
    let mut r2 = MdCollabHost::restore(opts(B), &snap).expect("restore changed");
    assert_eq!(md(&r2), before_md, "结构保真");
    assert_eq!(r2.foreign_snapshot().len(), 3, "原 family 全落 foreign");
    let target = r2.session().document()[1].block_id.clone();
    let path = r2.path_of(&target).expect("foreign 寻址");
    let mut o = Op::ReplaceText {
        path,
        start: 0,
        end: 2,
        insert: "段".into(),
    };
    r2.apply_op(Actor::Ai, &mut o).expect("edit via foreign");
    assert!(md(&r2).contains("段一"));
}

#[test]
fn hostile_empty_block_id_orphan() {
    let mut a = host(A);
    let mut raw_b = raw_session(B);
    // 空串 block_id（协作域公开 API 可造——读模型 block_id 丢空面）。
    inject_fake_block(&mut raw_b, "");
    let sv_a = a.state_vector();
    a.apply_remote(&raw_b.encode_diff(&sv_a).expect("diff b→a"))
        .expect("a applies b");
    // 物化：孤儿（不入 foreign 表——身份跨轮不稳定，结构保真）。
    assert_eq!(a.foreign_snapshot().len(), 0);
    assert!(md(&a).contains("伪造块"), "结构保真（文本面在）");
    // 再物化一轮：结构仍保真。
    a.materialize().expect("rematerialize");
    assert!(md(&a).contains("伪造块"));
    // path_of("") 不可寻址（空 id 非法寻址面——诚实 None）。
    assert_eq!(a.path_of(""), None);
}

#[test]
fn undo_local_only_after_remote() {
    let mut a = host(A);
    let src = markdown::markdown_to_tree("# 标题\n\n段落一");
    a.publish(&src, Actor::Human).expect("publish");
    // B 裸会话远端插入（无 origin → 不进 A 的撤销栈）。
    let mut raw = CollabSession::open(opts(B)).expect("raw");
    raw.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: BlockSpec {
                kind: CKind::Paragraph,
                attrs: Default::default(),
                text: "远端块".into(),
            },
            id: None,
        },
    )
    .expect("raw insert");
    let sv_a = a.state_vector();
    a.apply_remote(&raw.encode_diff(&sv_a).expect("diff raw→a"))
        .expect("a applies raw");
    assert_eq!(
        a.undo_depth(Actor::Human),
        2,
        "publish 两插入在栈（每块一项）"
    );
    // 本地 undo 撤最近一块（LIFO——段落一）；远端块与首块不受影响。
    a.undo(Actor::Human).expect("undo");
    let doc = a.session().document();
    assert!(
        doc.iter().any(|b| b.text == "远端块"),
        "远端编辑不受本地撤销影响"
    );
    assert!(
        !doc.iter().any(|b| b.text == "段落一"),
        "最近一块被撤（LIFO）"
    );
    assert!(doc.iter().any(|b| b.text == "标题"), "更早的块保留");
}
