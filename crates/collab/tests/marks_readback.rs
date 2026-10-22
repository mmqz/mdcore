//! marks 读回面：`BlockInfo.marks` 展平区间（mdbridge 对接轮引入的
//! collab 加性读面——预存 carrier 形状的逻辑视图）。

use mdc_collab::{
    Actor, BlockInfo, BlockKind, BlockSpec, CollabCommand, CollabSession, SessionOptions,
};

fn session() -> CollabSession {
    CollabSession::open(SessionOptions {
        client_id: 7_500_001,
        ..SessionOptions::default()
    })
    .expect("session")
}

fn spec(text: &str) -> BlockSpec {
    BlockSpec {
        kind: BlockKind::Paragraph,
        attrs: Default::default(),
        text: text.into(),
    }
}

#[test]
fn plain_text_no_marks() {
    let mut s = session();
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec("纯文本")],
        },
    )
    .expect("load");
    let doc = s.document();
    assert_eq!(doc[0].text, "纯文本");
    assert!(doc[0].marks.is_empty(), "无标记面零区间");
}

#[test]
fn single_and_nested_marks_readback() {
    let mut s = session();
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec("0123456789")],
        },
    )
    .expect("load");
    let bid = s.document()[0].block_id.clone();
    // 单标记 [2,7)。
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: bid.clone(),
            start: 2,
            end: 7,
            mark: "bold".into(),
            add: true,
        },
    )
    .expect("mark1");
    let doc = s.document();
    assert_eq!(
        doc[0].marks,
        vec![mdc_collab::MarkInfo {
            start: 2,
            end: 7,
            mark: "bold".into()
        }]
    );
    // 嵌套加层 [4,6) italic → D-2 归一化读回：[2,7)bold 拆为
    // [2,4)bold + [4,6)bold/italic 嵌套 + [6,7)bold（字面区间序确定）。
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: bid.clone(),
            start: 4,
            end: 6,
            mark: "italic".into(),
            add: true,
        },
    )
    .expect("mark2");
    let doc = s.document();
    let got: Vec<(u32, u32, &str)> = doc[0]
        .marks
        .iter()
        .map(|m| (m.start, m.end, m.mark.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (2, 4, "bold"),
            (4, 6, "bold"),
            (4, 6, "italic"),
            (6, 7, "bold"),
        ],
        "归一化嵌套读回"
    );
    // 区间 start 升序 + 同 start kind 字典序。
    let sorted = {
        let mut v = got.clone();
        v.sort();
        v
    };
    assert_eq!(got, sorted, "序确定");
    // 文本不受标记影响。
    assert_eq!(doc[0].text, "0123456789");
}

#[test]
fn mark_removal_readback() {
    let mut s = session();
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec("abcdef")],
        },
    )
    .expect("load");
    let bid = s.document()[0].block_id.clone();
    for (a, b) in [(0u32, 3u32), (2u32, 5u32)] {
        s.execute(
            Actor::Human,
            CollabCommand::SetMark {
                block_id: bid.clone(),
                start: a,
                end: b,
                mark: "bold".into(),
                add: true,
            },
        )
        .expect("add");
    }
    // 归一化后 [0,5) bold 覆盖；撤 [1,4) → 残 [0,1) 与 [4,5)。
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: bid.clone(),
            start: 1,
            end: 4,
            mark: "bold".into(),
            add: false,
        },
    )
    .expect("remove");
    let doc = s.document();
    let got: Vec<(u32, u32)> = doc[0].marks.iter().map(|m| (m.start, m.end)).collect();
    assert_eq!(got, vec![(0, 1), (4, 5)], "撤中段残两端");
}

#[test]
fn find_block_marks_view() {
    let mut s = session();
    s.execute(
        Actor::Human,
        CollabCommand::LoadBlocks {
            blocks: vec![spec("定位块文本")],
        },
    )
    .expect("load");
    let bid = s.document()[0].block_id.clone();
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: bid.clone(),
            start: 0,
            end: 2,
            mark: "code".into(),
            add: true,
        },
    )
    .expect("mark");
    let info: BlockInfo = s.find_block(&bid).expect("find");
    assert_eq!(info.marks.len(), 1);
    assert_eq!(info.marks[0].mark, "code");
    assert_eq!(info.marks[0].start, 0);
    assert_eq!(info.marks[0].end, 2);
}
