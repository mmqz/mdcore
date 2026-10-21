//! 敌意与边界输入测试——非法参数全路径 fail-closed。

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

#[test]
fn invalid_constructor_inputs() {
    // client_id 0 / 超 53 位 → 拒（release 静默污染的 fail-closed 面）。
    assert!(matches!(
        CollabSession::open(SessionOptions {
            client_id: 0,
            ..Default::default()
        }),
        Err(CollabError::InvalidInput(_))
    ));
    assert!(matches!(
        CollabSession::open(SessionOptions {
            client_id: 1 << 53,
            ..Default::default()
        }),
        Err(CollabError::InvalidInput(_))
    ));
    // 53 位域内合法。
    assert!(CollabSession::open(SessionOptions {
        client_id: (1 << 53) - 1,
        ..Default::default()
    })
    .is_ok());
}

#[test]
fn bad_ranges_and_missing_blocks() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "三个字"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    // start > end。
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "p-1".into(),
                start: 2,
                end: 1,
                insert: String::new(),
            },
        ),
        Err(CollabError::BadRange { .. })
    ));
    // end 超长（char 域 3——字节域 9 的换算面前置校验）。
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "p-1".into(),
                start: 0,
                end: 4,
                insert: String::new(),
            },
        ),
        Err(CollabError::BadRange {
            start: 0,
            end: 4,
            len: 3
        })
    ));
    // mark 越界。
    assert!(matches!(
        s.execute(
            Actor::Ai,
            CollabCommand::SetMark {
                block_id: "p-1".into(),
                start: 0,
                end: 9,
                mark: "em".into(),
                add: true,
            },
        ),
        Err(CollabError::BadRange { .. })
    ));
    // 不存在的块。
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "ghost".into(),
                start: 0,
                end: 0,
                insert: "x".into(),
            },
        ),
        Err(CollabError::BlockNotFound { .. })
    ));
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::RemoveBlock {
                block_id: "ghost".into()
            },
        ),
        Err(CollabError::BlockNotFound { .. })
    ));
    // 绑锚不存在块。
    assert!(matches!(
        s.bind_anchor("ghost"),
        Err(CollabError::BlockNotFound { .. })
    ));
    // 插入 index 越界。
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::InsertBlock {
                parent: None,
                index: 5,
                spec: spec(BlockKind::Paragraph, "x"),
                id: None,
            },
        ),
        Err(CollabError::InvalidInput(_))
    ));
    // 物化 id 撞现存块。
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::InsertBlock {
                parent: None,
                index: 0,
                spec: spec(BlockKind::Paragraph, "x"),
                id: Some("p-1".into()),
            },
        ),
        Err(CollabError::InvalidInput(_))
    ));
    // hr 拒文本。
    assert!(matches!(
        s.execute(
            Actor::Ai,
            CollabCommand::InsertBlock {
                parent: None,
                index: 1,
                spec: spec(BlockKind::ThematicBreak, "有文本"),
                id: None,
            },
        ),
        Err(CollabError::KindIncompatible { .. })
    ));
    // hr 拒文本编辑。
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
    assert!(matches!(
        s.execute(
            Actor::Ai,
            CollabCommand::ReplaceText {
                block_id: "hr-1".into(),
                start: 0,
                end: 0,
                insert: "x".into(),
            },
        ),
        Err(CollabError::KindIncompatible { .. })
    ));
    // 空 mark kind。
    assert!(matches!(
        s.execute(
            Actor::Ai,
            CollabCommand::SetMark {
                block_id: "p-1".into(),
                start: 0,
                end: 1,
                mark: String::new(),
                add: true,
            },
        ),
        Err(CollabError::InvalidInput(_))
    ));
}

#[test]
fn input_byte_limits() {
    let mut s = CollabSession::open(SessionOptions {
        client_id: 2,
        input_byte_limit: 256,
        ..Default::default()
    })
    .unwrap();
    // 装载超限（文本 400B + 块记账 128B > 256）。
    let big = spec(BlockKind::Paragraph, &"x".repeat(400));
    assert!(matches!(
        s.execute(Actor::Ai, CollabCommand::LoadBlocks { blocks: vec![big] }),
        Err(CollabError::Limit { limit: 256, .. })
    ));
    // 编辑插入超限。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "seed"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    assert!(matches!(
        s.execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "p-1".into(),
                start: 0,
                end: 0,
                insert: "y".repeat(400),
            },
        ),
        Err(CollabError::Limit { .. })
    ));
    // apply_update 超限。
    assert!(matches!(
        s.apply_update(&vec![0u8; 400]),
        Err(CollabError::Limit { .. })
    ));
    // 锚点侧车超限。
    assert!(matches!(
        s.import_anchor_sidecar(&vec![0u8; 400]),
        Err(CollabError::Limit { .. })
    ));
    // 会话状态零污染。
    assert_eq!(s.document().len(), 1);
}

#[test]
fn anchor_sidecar_hostile_payloads() {
    let mut s = CollabSession::open(opts(3)).unwrap();
    // 坏 magic。
    assert!(matches!(
        s.import_anchor_sidecar(b"XXXX\x01\x00\x00\x00\x00"),
        Err(CollabError::SidecarFormat(_))
    ));
    // 版本不受支持。
    assert!(matches!(
        s.import_anchor_sidecar(b"MDCC\x02\x00\x00\x00\x00"),
        Err(CollabError::SidecarFormat(_))
    ));
    // 敌意 count 预分配（物理上界拒绝——13B 载荷声称 2^32 条）。
    let mut hostile = vec![b'M', b'D', b'C', b'C', 1u8];
    hostile.extend_from_slice(&u32::MAX.to_le_bytes());
    hostile.extend_from_slice(&[0u8; 8]);
    assert!(matches!(
        s.import_anchor_sidecar(&hostile),
        Err(CollabError::SidecarFormat(_))
    ));
    // 尾部冗余字节。
    let good = {
        let mut x = b"MDCC\x01".to_vec();
        x.extend_from_slice(&1u32.to_le_bytes()); // count = 1
        x.extend_from_slice(&2u32.to_le_bytes()); // id 长度 2
        x.extend_from_slice(b"id");
        x.extend_from_slice(&1u64.to_le_bytes());
        x.extend_from_slice(&1u32.to_le_bytes());
        x
    };
    let mut with_tail = good.clone();
    with_tail.push(0xFF);
    assert!(matches!(
        s.import_anchor_sidecar(&with_tail),
        Err(CollabError::SidecarFormat(_))
    ));
    // 合法载荷导入成功。
    assert_eq!(s.import_anchor_sidecar(&good).unwrap(), 1);
    assert!(s.anchor_of("id").is_some());
}
