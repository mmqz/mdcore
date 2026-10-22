/// 发布面：本地 BlockTree → 协同会话（重映射身份 + 往返保真）。
mod common;

use common::*;
use mdc_collab::Actor;
use mdc_mdbridge::{parse_family_id, BridgeError};
use mdcore::markdown;
use mdcore::op::Op;
use mdcore::tree::BlockKind;

const MD: &str = "# 标题一\n\n正文段落甲，含**加粗**。\n\n> 引用行\n\n- 列表项一\n- 列表项二\n\n---\n\n| a | b |\n| 1 | 2 |";

fn sample_tree() -> mdcore::tree::BlockTree {
    markdown::markdown_to_tree(MD)
}

#[test]
fn publish_roundtrip_identity_and_shape() {
    let mut h = host(7_200_001);
    let src = sample_tree();
    let expect_md = markdown::tree_to_markdown(&src);
    let n_nodes = src.node_count();
    let n_top = src.top_level().len();

    let receipt = h.publish(&src, Actor::Human).expect("publish");
    // 回执：块数（全树含子块）+ 顶层 family id 形状。
    assert_eq!(receipt.blocks, n_nodes);
    assert_eq!(receipt.top_ids.len(), n_top);
    for id in &receipt.top_ids {
        let (c, _) = parse_family_id(id).expect("family 形状");
        assert_eq!(c, 7_200_001);
    }
    // 重映射身份：shadow u64 = 回执 family n（回执即映射表）。
    for (i, &top) in h.tree().top_level().iter().enumerate() {
        let (_, n) = parse_family_id(&receipt.top_ids[i]).expect("family");
        assert_eq!(
            h.tree().node(top).id.0,
            n,
            "shadow[{i}] u64 = 回执 family n（重映射一致性）"
        );
    }
    // 会话读模型：结构/文本与源树一致。
    let doc = h.session().document();
    assert_eq!(doc.len(), n_top);
    // 全链往返：源树 → collab → shadow → markdown == 源树 markdown。
    assert_eq!(md(&h), expect_md);
    // 结构等价（BlockId 忽略——重映射语义下 shadow u64 ≠ 源树 u64）。
    assert_trees_structure_eq(h.tree(), &src, "publish 结构等价");
}

#[test]
fn publish_marks_visible_and_coverage_equivalent() {
    let mut h = host(7_200_002);
    // 手工建树：段落 + 两段重叠 bold（D-2 归一化面）。
    let mut t = mdcore::tree::BlockTree::new();
    let mut op = insert_text(vec![], 0, BlockKind::Paragraph, "0123456789");
    h_must(&mut t, &mut op);
    let mut op = Op::SetMark {
        path: vec![0],
        start: 2,
        end: 7,
        kind: "bold".into(),
        add: true,
    };
    mdcore::op::apply_op(&mut t, &mut op).expect("mark1");
    let mut op = Op::SetMark {
        path: vec![0],
        start: 5,
        end: 9,
        kind: "bold".into(),
        add: true,
    };
    mdcore::op::apply_op(&mut t, &mut op).expect("mark2");

    let receipt = h.publish(&t, Actor::Ai).expect("publish");
    assert_eq!(receipt.blocks, 1);
    assert_eq!(receipt.marks, 2, "两条 SetMark 命令");
    // 会话读模型：marks 展平可见（归一化形状）。
    let doc = h.session().document();
    assert_eq!(doc.len(), 1);
    assert!(!doc[0].marks.is_empty(), "collab marks 读回非空");
    // shadow（重物化）vs 源树：结构+覆盖等价（BlockId 忽略——重映射；
    // 字面 mark 形状允许差——D-2 镜像）。
    assert_trees_structure_eq(h.tree(), &t, "publish marks");
}

#[test]
fn publish_same_tree_twice_distinct_ids() {
    let mut h = host(7_200_003);
    let a = sample_tree();
    let b = sample_tree();
    h.publish(&a, Actor::Human).expect("first");
    let r2 = h.publish(&b, Actor::Ai).expect("second");
    assert_eq!(h.session().document().len(), a.top_level().len() * 2);
    // 重映射：同形两树两次发布，family id 互不相交（fresh 分配）。
    let doc = h.session().document();
    let ids: std::collections::HashSet<&str> = doc.iter().map(|b| b.block_id.as_str()).collect();
    assert_eq!(ids.len(), a.top_level().len() * 2);
    assert_eq!(r2.blocks, b.node_count());
    // 追加序：md 视图 = 首树 + 次树两段拼接。
    assert_eq!(
        md(&h),
        markdown::tree_to_markdown(&a) + "\n" + &markdown::tree_to_markdown(&b)
    );
}

#[test]
fn publish_precheck_failures() {
    let mut h = host(7_200_005);
    // hr 带文本。
    let mut t = mdcore::tree::BlockTree::new();
    let mut op = insert_text(vec![], 0, BlockKind::ThematicBreak, "有文本");
    mdcore::op::apply_op(&mut t, &mut op).expect("源树允许（mdcore 原子层无此约束）");
    let err = h.publish(&t, Actor::Human).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(mdc_mdbridge::TranslateError::KindIncompatible(_))
    ));
    // 保留属性键。
    let mut t2 = mdcore::tree::BlockTree::new();
    let mut attrs = std::collections::BTreeMap::new();
    attrs.insert("block_id".to_string(), "x".to_string());
    let mut op = Op::InsertBlock {
        parent: vec![],
        index: 0,
        kind: BlockKind::Paragraph,
        attrs,
        text: "t".into(),
        id: None,
    };
    mdcore::op::apply_op(&mut t2, &mut op).expect("源树插入");
    let err = h.publish(&t2, Actor::Human).unwrap_err();
    assert!(matches!(
        err,
        BridgeError::Translate(mdc_mdbridge::TranslateError::ReservedAttrKey)
    ));
    // 两种失败均零副作用。
    assert_eq!(md(&h), "");
    assert!(h.session().document().is_empty());
}

fn h_must(t: &mut mdcore::tree::BlockTree, op: &mut Op) {
    mdcore::op::apply_op(t, op).expect("树构造 op");
}
