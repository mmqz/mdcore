//! 故障恢复测试——权威快照重建、同步面畸形输入、维护压缩。

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

/// 权威快照 → 新会话恢复：全量状态 + 可继续编辑 + 可继续同步。
#[test]
fn snapshot_restore_and_continue() {
    let mut orig = CollabSession::open(opts(501)).unwrap();
    orig.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![
                spec(BlockKind::Paragraph, "快照段落一"),
                spec(BlockKind::Blockquote, "容器"),
            ],
        },
    )
    .unwrap();
    let bq_id = orig.top_level_ids()[1].clone();
    orig.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: Some(bq_id.clone()),
            index: 0,
            spec: spec(BlockKind::Paragraph, "容器子块"),
            id: Some("child-1".into()),
        },
    )
    .unwrap();
    // 撤销历史存在（不随快照走——B3.1 本地恒不序列化）。
    assert!(orig.undo_depth(Actor::Human) > 0 || orig.undo_depth(Actor::Ai) > 0);

    let snap = orig.authoritative_snapshot().unwrap();
    let mut restored = CollabSession::restore(opts(601), &snap).unwrap();
    // 状态全量恢复。
    let texts: Vec<String> = restored.document().iter().map(|b| b.text.clone()).collect();
    assert_eq!(texts, vec!["快照段落一".to_string(), "容器".to_string()]);
    let bq = restored.find_block(&bq_id).unwrap();
    assert_eq!(bq.children.len(), 1);
    assert_eq!(bq.children[0].block_id, "child-1");
    // 撤销栈从空开始（只能撤销恢复后的新编辑）。
    assert_eq!(restored.undo_depth(Actor::Human), 0);
    assert_eq!(restored.undo_depth(Actor::Ai), 0);
    // 恢复会话可继续编辑 + 与原会话互相同步。
    restored
        .execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "child-1".into(),
                start: 0,
                end: 0,
                insert: "恢复后编辑 ".into(),
            },
        )
        .unwrap();
    let sv = orig.state_vector();
    let diff = restored.encode_diff(&sv).unwrap();
    orig.apply_update(&diff).unwrap();
    assert_eq!(
        orig.find_block("child-1").unwrap().text,
        "恢复后编辑 容器子块"
    );
    // 恢复会话自己也能撤销新编辑。
    assert!(restored.undo(Actor::Human));
    assert_eq!(restored.find_block("child-1").unwrap().text, "容器子块");
}

/// 快照与差量互通：恢复会话可与从未见面的第三方同步。
#[test]
fn snapshot_interops_with_diff_sync() {
    let mut a = CollabSession::open(opts(701)).unwrap();
    a.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec(BlockKind::Paragraph, "互通基线")],
        },
    )
    .unwrap();
    let snap = a.authoritative_snapshot().unwrap();
    let c = CollabSession::restore(opts(702), &snap).unwrap();
    // a 继续编辑 → c 经差量追平。
    a.execute(
        Actor::Ai,
        CollabCommand::InsertBlock {
            parent: None,
            index: 1,
            spec: spec(BlockKind::Paragraph, "增量"),
            id: Some("inc-1".into()),
        },
    )
    .unwrap();
    let sv = c.state_vector();
    let diff = a.encode_diff(&sv).unwrap();
    c.apply_update(&diff).unwrap();
    assert!(c.find_block("inc-1").is_some());
    assert_eq!(c.document().len(), 2);
}

/// 维护压缩：clear_all + gc 后 wire 收缩、会话仍可用。
#[test]
fn maintenance_gc_semantics() {
    let mut s = CollabSession::open(opts(801)).unwrap();
    s.execute(
        Actor::Ai,
        CollabCommand::LoadBlocks {
            blocks: (0..50)
                .map(|i| spec(BlockKind::Paragraph, &format!("段{i}垃圾字节")))
                .collect(),
        },
    )
    .unwrap();
    // 删掉一半（产生墓碑）。
    for _ in 0..25 {
        let top = s.top_level_ids();
        s.execute(
            Actor::Human,
            CollabCommand::RemoveBlock {
                block_id: top[0].clone(),
            },
        )
        .unwrap();
    }
    let (before, after) = s.maintenance_gc().unwrap();
    assert!(after < before, "压缩生效（wire 字节 {before} → {after}）");
    // 撤销历史全失（代价面）。
    assert_eq!(s.undo_depth(Actor::Human), 0);
    assert_eq!(s.undo_depth(Actor::Ai), 0);
    // 会话仍可正常编辑。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "压缩后新块"),
            id: Some("post-gc".into()),
        },
    )
    .unwrap();
    assert!(s.find_block("post-gc").is_some());
    assert_eq!(s.document().len(), 26);
}

/// 同步面畸形输入：坏字节/截断 update → 显式 Decode 错误，会话
/// 状态零污染。
#[test]
fn hostile_wire_inputs_fail_closed() {
    let mut s = CollabSession::open(opts(901)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec(BlockKind::Paragraph, "基线")],
        },
    )
    .unwrap();
    let before = s.document();
    // 随机坏字节。
    let garbage = vec![0xFF, 0xFE, 0x00, 0x01, 0x02];
    assert!(matches!(
        s.apply_update(&garbage),
        Err(CollabError::Decode(_))
    ));
    // 截断真 update。
    let peer = CollabSession::open(opts(902)).unwrap();
    let sv = s.state_vector();
    let mut diff = peer.encode_diff(&sv).unwrap();
    if diff.len() > 4 {
        diff.truncate(diff.len() / 2);
        assert!(matches!(s.apply_update(&diff), Err(CollabError::Decode(_))));
    }
    // 坏 state vector。
    assert!(matches!(
        s.encode_diff(&[0xFF, 0xFF, 0xFF]),
        Err(CollabError::Decode(_))
    ));
    // 会话零污染。
    assert_eq!(s.document(), before);
    // 后续合法同步照常。
    let mut ok_peer = CollabSession::open(opts(903)).unwrap();
    ok_peer
        .execute(
            Actor::Ai,
            CollabCommand::InsertBlock {
                parent: None,
                index: 0,
                spec: spec(BlockKind::Paragraph, "合法块"),
                id: Some("ok-1".into()),
            },
        )
        .unwrap();
    let sv = s.state_vector();
    let diff = ok_peer.encode_diff(&sv).unwrap();
    s.apply_update(&diff).unwrap();
    assert!(s.find_block("ok-1").is_some());
}
