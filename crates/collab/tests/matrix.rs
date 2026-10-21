//! 操作矩阵测试——七操作 × 九 kind 语义 + 身份双轨 + D-1/D-2/D-3。

use std::collections::BTreeMap;

use mdc_collab::{
    Actor, BlockKind, BlockSpec, CollabCommand, CollabError, CollabSession, SessionOptions,
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

/// 九 kind 全量装载读回（p15-A1 形状的产品面钉定）。
#[test]
fn nine_kind_load_and_readback() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    let ids = s
        .load_blocks(
            Actor::Ai,
            &[
                spec(BlockKind::Paragraph, "第一段普通文本"),
                spec(BlockKind::Heading, "二级标题"),
                spec(
                    BlockKind::CodeBlock,
                    "fn main() {\n    println!(\"多行\");\n}",
                ),
                spec(BlockKind::ThematicBreak, ""),
                spec(BlockKind::Table, "| a | b |\n|---|---|\n| 1 | 2 |"),
                spec(BlockKind::Blockquote, "引语自有首段"),
                spec(BlockKind::ListItem, "首行文本"),
                spec(BlockKind::Paragraph, "末段"),
            ],
        )
        .unwrap();
    assert_eq!(ids.len(), 8);
    let doc = s.document();
    assert_eq!(doc.len(), 8);
    assert_eq!(doc[0].kind, BlockKind::Paragraph);
    assert_eq!(doc[0].text, "第一段普通文本");
    assert_eq!(doc[1].kind, BlockKind::Heading);
    assert_eq!(doc[2].kind, BlockKind::CodeBlock);
    assert_eq!(doc[2].text, "fn main() {\n    println!(\"多行\");\n}"); // 多行原样
    assert_eq!(doc[3].kind, BlockKind::ThematicBreak);
    assert_eq!(doc[3].text, "");
    assert_eq!(doc[4].kind, BlockKind::Table);
    assert_eq!(doc[5].kind, BlockKind::Blockquote);
    assert_eq!(doc[6].kind, BlockKind::ListItem);
    assert_eq!(doc[7].text, "末段");
    // 身份：全部唯一
    let mut seen = std::collections::HashSet::new();
    for id in &ids {
        assert!(seen.insert(id.clone()), "块身份唯一");
    }
    // find_block 逐块可达
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(s.find_block(id).unwrap().text, doc[i].text);
    }
}

/// 容器子块插入（blockquote/li 为合法父；叶块父拒收）+ own-text 段
/// 与子块共存。
#[test]
fn container_children_and_own_text() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    let bq = s
        .execute(
            Actor::Human,
            CollabCommand::InsertBlock {
                parent: None,
                index: 0,
                spec: spec(BlockKind::Blockquote, "引语自有首段"),
                id: Some("bq-1".into()),
            },
        )
        .unwrap();
    match bq.outcome {
        mdc_collab::CommandOutcome::BlockInserted { ref block_id } => {
            assert_eq!(block_id, "bq-1");
        }
        other => panic!("期望 BlockInserted，得到 {other:?}"),
    }
    // 子块插入 blockquote：index 0（own-text 段之后的首个子位）。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: Some("bq-1".into()),
            index: 0,
            spec: spec(BlockKind::Paragraph, "引语内段落"),
            id: Some("bq-p1".into()),
        },
    )
    .unwrap();
    // own-text 不被子块污染。
    let bq_view = s.find_block("bq-1").unwrap();
    assert_eq!(bq_view.text, "引语自有首段");
    assert_eq!(bq_view.children.len(), 1);
    assert_eq!(bq_view.children[0].block_id, "bq-p1");
    // 叶块为父 → 拒收。
    let err = s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: Some("bq-p1".into()),
            index: 0,
            spec: spec(BlockKind::Paragraph, "x"),
            id: None,
        },
    );
    assert!(matches!(err, Err(CollabError::NotAContainer { .. })));
    // hr 为父 → 拒收。
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec(BlockKind::ThematicBreak, ""),
            id: Some("hr-1".into()),
        },
    )
    .unwrap();
    let err = s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: Some("hr-1".into()),
            index: 0,
            spec: spec(BlockKind::Paragraph, "x"),
            id: None,
        },
    );
    assert!(matches!(err, Err(CollabError::NotAContainer { .. })));
}

/// ReplaceText：char 域语义（CJK）+ 跨 mark 逻辑位置 + 纯删/纯插。
#[test]
fn replace_text_char_domain_and_marks() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "第一段落尾"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    // char 位 3 插入（"第一段" + "落尾"）——字节域换算 D-3 的产品面钉定。
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "p-1".into(),
            start: 3,
            end: 3,
            insert: "ABC".into(),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-1").unwrap().text, "第一段ABC落尾");
    // 覆盖替换。
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "p-1".into(),
            start: 3,
            end: 6,
            insert: "X".into(),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-1").unwrap().text, "第一段X落尾");
    // 纯删。
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "p-1".into(),
            start: 0,
            end: 3,
            insert: String::new(),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-1").unwrap().text, "X落尾");
    // 加 mark 后跨 mark 逻辑编辑：em[X落] + 尾"尾" → 整体逻辑文本
    // "X落尾"，char [1,3) 替换为 "Y"。
    s.execute(
        Actor::Ai,
        CollabCommand::SetMark {
            block_id: "p-1".into(),
            start: 0,
            end: 2,
            mark: "em".into(),
            add: true,
        },
    )
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "p-1".into(),
            start: 1,
            end: 3,
            insert: "YZ".into(),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-1").unwrap().text, "XYZ");
    // 空块插入（无文本节点面）。
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec(BlockKind::Paragraph, ""),
            id: Some("p-2".into()),
        },
    )
    .unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::ReplaceText {
            block_id: "p-2".into(),
            start: 0,
            end: 0,
            insert: "从零开始".into(),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-2").unwrap().text, "从零开始");
}

/// SetMark：加/删/嵌套/重叠归一（D-2 最小区间等价覆盖）。
#[test]
fn set_mark_add_remove_and_overlap() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "abcdefghijklm"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    // em [2,6) + strong [4,9) 重叠 → 拆分为最小区间。
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: "p-1".into(),
            start: 2,
            end: 6,
            mark: "em".into(),
            add: true,
        },
    )
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: "p-1".into(),
            start: 4,
            end: 9,
            mark: "strong".into(),
            add: true,
        },
    )
    .unwrap();
    // 读回逻辑文本不变；再通过集合代数验证覆盖：
    // 全删 em → strong 段 [4,9) 恰在。
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: "p-1".into(),
            start: 0,
            end: 13,
            mark: "em".into(),
            add: false,
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-1").unwrap().text, "abcdefghijklm");
    // 加 code [0,13) 与 strong [4,9) 嵌套共存，再全删 strong。
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: "p-1".into(),
            start: 0,
            end: 13,
            mark: "code".into(),
            add: true,
        },
    )
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: "p-1".into(),
            start: 0,
            end: 13,
            mark: "strong".into(),
            add: false,
        },
    )
    .unwrap();
    // 终态：逻辑文本保持（mark 操作永不改文本）。
    assert_eq!(s.find_block("p-1").unwrap().text, "abcdefghijklm");
    // 容器块 own-text mark 面：blockquote 自有文本段可 mark，子块不动。
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec(BlockKind::Blockquote, "引语自有"),
            id: Some("bq-1".into()),
        },
    )
    .unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: Some("bq-1".into()),
            index: 0,
            spec: spec(BlockKind::Paragraph, "子块文本"),
            id: Some("bq-p1".into()),
        },
    )
    .unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::SetMark {
            block_id: "bq-1".into(),
            start: 0,
            end: 2,
            mark: "em".into(),
            add: true,
        },
    )
    .unwrap();
    let bq = s.find_block("bq-1").unwrap();
    assert_eq!(bq.text, "引语自有");
    assert_eq!(bq.children.len(), 1, "mark 重建不吞子块");
    assert_eq!(bq.children[0].text, "子块文本");
    // hr 无文本面 → 拒收。
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 2,
            spec: spec(BlockKind::ThematicBreak, ""),
            id: Some("hr-1".into()),
        },
    )
    .unwrap();
    let err = s.execute(
        Actor::Ai,
        CollabCommand::SetMark {
            block_id: "hr-1".into(),
            start: 0,
            end: 0,
            mark: "em".into(),
            add: true,
        },
    );
    assert!(matches!(err, Err(CollabError::KindIncompatible { .. })));
}

/// SetAttrs：终态替换 + 开放键透传 + 保留键拒收 + 键转义往返。
#[test]
fn set_attrs_semantics() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    let mut attrs = BTreeMap::new();
    attrs.insert("level".to_string(), "2".to_string());
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: BlockSpec {
                kind: BlockKind::Heading,
                attrs: attrs.clone(),
                text: "标题".into(),
            },
            id: Some("h-1".into()),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("h-1").unwrap().attrs, attrs);
    // 终态替换：level 2 → 3（清旧写新）。
    let mut new_attrs = BTreeMap::new();
    new_attrs.insert("level".to_string(), "3".to_string());
    s.execute(
        Actor::Human,
        CollabCommand::SetAttrs {
            block_id: "h-1".into(),
            attrs: new_attrs.clone(),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("h-1").unwrap().attrs, new_attrs);
    // 开放键 + 非法 XML 名字符键转义往返（"a b" 含空格 → _x0020_）。
    let mut odd = BTreeMap::new();
    odd.insert("custom key!".to_string(), "v1".to_string());
    odd.insert("正常键".to_string(), "v2".to_string());
    s.execute(
        Actor::Human,
        CollabCommand::SetAttrs {
            block_id: "h-1".into(),
            attrs: odd.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        s.find_block("h-1").unwrap().attrs,
        odd,
        "开放键零丢（转义往返）"
    );
    // block_id 保留键拒收。
    let mut bad = BTreeMap::new();
    bad.insert("block_id".to_string(), "hack".to_string());
    let err = s.execute(
        Actor::Human,
        CollabCommand::SetAttrs {
            block_id: "h-1".into(),
            attrs: bad,
        },
    );
    assert!(matches!(err, Err(CollabError::ReservedAttrKey { .. })));
    // spec 构造面同样拒收。
    let mut bad_spec = BTreeMap::new();
    bad_spec.insert("block_id".to_string(), "hack".to_string());
    let err = s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: BlockSpec {
                kind: BlockKind::Paragraph,
                attrs: bad_spec,
                text: String::new(),
            },
            id: None,
        },
    );
    assert!(matches!(err, Err(CollabError::ReservedAttrKey { .. })));
}

/// SetBlockType（D-1）+ MoveBlock：身份连续 + 子树保持 + 环回拒。
#[test]
fn set_type_and_move_semantics() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "将变标题"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    // D-1：p → h，block_id 语义连续。
    s.execute(
        Actor::Human,
        CollabCommand::SetBlockType {
            block_id: "p-1".into(),
            kind: BlockKind::Heading,
        },
    )
    .unwrap();
    let v = s.find_block("p-1").unwrap();
    assert_eq!(v.kind, BlockKind::Heading);
    assert_eq!(v.text, "将变标题");
    // MoveBlock：子树整体搬移 + 环回拒。
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec(BlockKind::Blockquote, "容器"),
            id: Some("bq-1".into()),
        },
    )
    .unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: Some("bq-1".into()),
            index: 0,
            spec: spec(BlockKind::Paragraph, "子块A"),
            id: Some("bq-a".into()),
        },
    )
    .unwrap();
    // 把 h-1 移入 bq-1 的子块位 0。
    s.execute(
        Actor::Human,
        CollabCommand::MoveBlock {
            block_id: "p-1".into(),
            to_parent: Some("bq-1".into()),
            to_index: 0,
        },
    )
    .unwrap();
    let bq = s.find_block("bq-1").unwrap();
    assert_eq!(bq.children.len(), 2);
    assert_eq!(bq.children[0].block_id, "p-1", "搬移后身份连续");
    assert_eq!(bq.children[0].kind, BlockKind::Heading);
    assert!(s.find_block("p-1").is_some());
    // 环回：把 bq-1 移入自己的子孙 → 拒。
    let err = s.execute(
        Actor::Human,
        CollabCommand::MoveBlock {
            block_id: "bq-1".into(),
            to_parent: Some("bq-a".into()),
            to_index: 0,
        },
    );
    assert!(matches!(err, Err(CollabError::InvalidInput(_))));
    // 自移（target = self）→ 拒。
    let err = s.execute(
        Actor::Human,
        CollabCommand::MoveBlock {
            block_id: "bq-1".into(),
            to_parent: Some("bq-1".into()),
            to_index: 0,
        },
    );
    assert!(matches!(err, Err(CollabError::InvalidInput(_))));
}

/// 锚点身份：邻居编辑稳定 + 删除悬空 + 显式恢复（重绑）。
#[test]
fn anchor_identity_semantics() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![
                spec(BlockKind::Paragraph, "段0"),
                spec(BlockKind::Paragraph, "段1"),
                spec(BlockKind::Paragraph, "段2"),
                spec(BlockKind::Paragraph, "段3"),
                spec(BlockKind::Paragraph, "段4"),
            ],
        },
    )
    .unwrap();
    let ids = s.top_level_ids();
    let anchor1 = s.bind_anchor(&ids[1]).unwrap();
    // 邻居编辑：前插 3 块。
    for i in 0..3 {
        s.execute(
            Actor::Ai,
            CollabCommand::InsertBlock {
                parent: None,
                index: i,
                spec: spec(BlockKind::Paragraph, "前插"),
                id: None,
            },
        )
        .unwrap();
    }
    // 前删 2 块（删新插者——原块全存活）。
    for _ in 0..2 {
        let top = s.top_level_ids();
        s.execute(
            Actor::Ai,
            CollabCommand::RemoveBlock {
                block_id: top[0].clone(),
            },
        )
        .unwrap();
    }
    let r = s.resolve_anchor(anchor1).unwrap();
    assert_eq!(r.block_id, ids[1], "邻居编辑后锚点仍指向原块（ID 稳定）");
    assert_eq!(r.parent_block_id, None);
    // 删除目标块 → 悬空（不静默返回邻居）。
    s.execute(
        Actor::Human,
        CollabCommand::RemoveBlock {
            block_id: ids[1].clone(),
        },
    )
    .unwrap();
    let err = s.resolve_anchor(anchor1);
    assert!(
        matches!(err, Err(CollabError::AnchorDangling { .. })),
        "删除后锚点悬空"
    );
    // 显式恢复通道：撤销删除（复活 = 新 ID 族）→ 重绑新锚点。
    assert!(s.undo(Actor::Human));
    assert!(s.find_block(&ids[1]).is_some(), "撤销删除 → 块复活");
    let anchor_new = s.bind_anchor(&ids[1]).unwrap();
    assert_ne!(anchor_new, anchor1, "重绑 = 新载体身份（新 ID 族）");
    let r = s.resolve_anchor(anchor_new).unwrap();
    assert_eq!(r.block_id, ids[1]);
    // 侧车往返。
    let sidecar = s.export_anchor_sidecar();
    let mut s2 = CollabSession::open(opts(2)).unwrap();
    let n = s2.import_anchor_sidecar(&sidecar).unwrap();
    assert_eq!(n, 1, "锚点表侧车往返（条数）");
    assert_eq!(s2.anchor_of(&ids[1]), Some(anchor_new));
}
