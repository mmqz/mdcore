//! 并发与同步测试——多会话收敛、并发修改、双向同步、撤销后同步。

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

/// 双端同步原语：把 from 的增量送到 to（to 缺的都补）。
fn sync(from: &CollabSession, to: &CollabSession) -> Result<(), CollabError> {
    let sv = to.state_vector();
    let diff = from.encode_diff(&sv)?;
    to.apply_update(&diff)
}

#[test]
fn two_session_full_then_incremental_convergence() {
    let mut alice = CollabSession::open(opts(101)).unwrap();
    let mut bob = CollabSession::open(opts(102)).unwrap();

    // Alice 建文档（人类），Bob 收全量。
    alice
        .execute(
            Actor::Human,
            CollabCommand::LoadBlocks {
                blocks: vec![
                    spec(BlockKind::Paragraph, "段一"),
                    spec(BlockKind::Paragraph, "段二"),
                ],
            },
        )
        .unwrap();
    sync(&alice, &bob).unwrap();
    assert_eq!(
        bob.document()
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>(),
        vec!["段一".to_string(), "段二".to_string()]
    );

    // 增量：Bob（AI）编辑段一文本 + 插入新块；Alice 收增量。
    let ids = bob.top_level_ids();
    bob.execute(
        Actor::Ai,
        CollabCommand::ReplaceText {
            block_id: ids[0].clone(),
            start: 0,
            end: 2,
            insert: "首段".into(),
        },
    )
    .unwrap();
    bob.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 2,
            spec: spec(BlockKind::Heading, "Bob 新增标题"),
            id: None,
        },
    )
    .unwrap();
    sync(&bob, &alice).unwrap();
    let a_texts: Vec<String> = alice.document().iter().map(|b| b.text.clone()).collect();
    assert_eq!(
        a_texts,
        vec![
            "首段".to_string(),
            "段二".to_string(),
            "Bob 新增标题".to_string()
        ]
    );
    // 双端读模型一致（收敛的读面证明）。
    let b_texts: Vec<String> = bob.document().iter().map(|b| b.text.clone()).collect();
    assert_eq!(a_texts, b_texts);
    // block_id 在对端可寻址（远端块免侧车寻址）。
    assert!(alice.find_block(&ids[0]).is_some());
}

/// 并发修改：双端同块同区间文本编辑 → 收敛不丢块；双端结构并发 →
/// 序收敛一致。
#[test]
fn concurrent_edits_converge() {
    let mut alice = CollabSession::open(opts(201)).unwrap();
    let mut bob = CollabSession::open(opts(202)).unwrap();
    alice
        .execute(
            Actor::Human,
            CollabCommand::LoadBlocks {
                blocks: vec![spec(BlockKind::Paragraph, "0123456789")],
            },
        )
        .unwrap();
    sync(&alice, &bob).unwrap();
    let id = alice.top_level_ids()[0].clone();

    // 并发：Alice 删 [0,5)，Bob 删 [5,10)（同一文本的两个不相交段）。
    alice
        .execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: id.clone(),
                start: 0,
                end: 5,
                insert: String::new(),
            },
        )
        .unwrap();
    bob.execute(
        Actor::Ai,
        CollabCommand::ReplaceText {
            block_id: id.clone(),
            start: 5,
            end: 10,
            insert: String::new(),
        },
    )
    .unwrap();
    // 交换增量（双向）。
    sync(&alice, &bob).unwrap();
    sync(&bob, &alice).unwrap();
    let a = alice.find_block(&id).unwrap().text;
    let b = bob.find_block(&id).unwrap().text;
    assert_eq!(a, b, "并发删后双端收敛同一");
    assert_eq!(a, "");

    // 并发结构：双端各插一块到 index 0 → 收敛后双端一致（两个新块
    // 都在，序由 CRDT 决定——只断言集合与一致性，不断言具体序）。
    alice
        .execute(
            Actor::Human,
            CollabCommand::InsertBlock {
                parent: None,
                index: 0,
                spec: spec(BlockKind::Paragraph, "alice-insert"),
                id: Some("a-ins".into()),
            },
        )
        .unwrap();
    bob.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "bob-insert"),
            id: Some("b-ins".into()),
        },
    )
    .unwrap();
    sync(&alice, &bob).unwrap();
    sync(&bob, &alice).unwrap();
    let a_ids = alice.top_level_ids();
    let b_ids = bob.top_level_ids();
    assert_eq!(a_ids, b_ids, "并发插入后双端结构一致");
    assert!(a_ids.contains(&"a-ins".to_string()));
    assert!(a_ids.contains(&"b-ins".to_string()));
}

/// 三方收敛（A↔B, B↔C, C↔A 链式同步）。
#[test]
fn three_way_chain_convergence() {
    let mut a = CollabSession::open(opts(301)).unwrap();
    let mut b = CollabSession::open(opts(302)).unwrap();
    let mut c = CollabSession::open(opts(303)).unwrap();
    a.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec(BlockKind::Paragraph, "基线")],
        },
    )
    .unwrap();
    sync(&a, &b).unwrap();
    b.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec(BlockKind::Paragraph, "B 加的"),
            id: Some("b-1".into()),
        },
    )
    .unwrap();
    sync(&b, &c).unwrap();
    c.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "b-1".into(),
            start: 0,
            end: 1,
            insert: "【B".into(),
        },
    )
    .unwrap();
    sync(&c, &a).unwrap();
    sync(&a, &b).unwrap();
    let texts: Vec<Vec<String>> = [&a, &b, &c]
        .iter()
        .map(|s| s.document().iter().map(|b| b.text.clone()).collect())
        .collect();
    assert_eq!(texts[0], texts[1]);
    assert_eq!(texts[1], texts[2]);
    assert_eq!(texts[0], vec!["基线".to_string(), "【B 加的".to_string()]);
}

/// 撤销跨端语义：Bob 只能撤销自己的编辑（Alice 的编辑保留）；
/// 撤销产物经同步在对端同样生效。
#[test]
fn undo_semantics_across_sessions() {
    let mut alice = CollabSession::open(opts(401)).unwrap();
    let mut bob = CollabSession::open(opts(402)).unwrap();
    alice
        .execute(
            Actor::Human,
            CollabCommand::LoadBlocks {
                blocks: vec![spec(BlockKind::Paragraph, "原文本")],
            },
        )
        .unwrap();
    sync(&alice, &bob).unwrap();
    let id = alice.top_level_ids()[0].clone();

    // Alice 编辑 → Bob 收到。
    alice
        .execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: id.clone(),
                start: 0,
                end: 0,
                insert: "[A]".into(),
            },
        )
        .unwrap();
    sync(&alice, &bob).unwrap();
    // Bob（AI）编辑。
    bob.execute(
        Actor::Ai,
        CollabCommand::ReplaceText {
            block_id: id.clone(),
            start: 0,
            end: 0,
            insert: "[B]".into(),
        },
    )
    .unwrap();
    sync(&bob, &alice).unwrap();
    assert_eq!(alice.find_block(&id).unwrap().text, "[B][A]原文本");

    // Bob 撤销自己的 Ai 编辑 → 只撤 [B]；[A]（远端 Alice 的）保留。
    assert!(bob.undo(Actor::Ai));
    sync(&bob, &alice).unwrap();
    assert_eq!(alice.find_block(&id).unwrap().text, "[A]原文本");
    assert_eq!(bob.find_block(&id).unwrap().text, "[A]原文本");
    // Bob 的 Human 栈为空（[B] 是 Ai origin）。
    assert_eq!(bob.undo_depth(Actor::Human), 0);
    // Alice 撤销自己的 Human 编辑。
    assert!(alice.undo(Actor::Human));
    sync(&alice, &bob).unwrap();
    assert_eq!(bob.find_block(&id).unwrap().text, "原文本");
}
