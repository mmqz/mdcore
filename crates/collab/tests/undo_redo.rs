//! 撤销/重做测试——origin 分栈、每 op 类型撤销、p20 交错回归锁、
//! 栈软上限、redo。

use std::collections::BTreeMap;

use mdc_collab::{Actor, BlockKind, BlockSpec, CollabCommand, CollabSession, SessionOptions};

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

/// 各操作类型的撤销/重做全环（结构/文本/属性/mark/搬移/种类）。
#[test]
fn undo_redo_all_op_types() {
    let mut s = CollabSession::open(opts(1)).unwrap();
    // 插入 → 撤销消失 → 重做复原（身份物化：同 block_id 回放）。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "块一"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    assert!(s.undo(Actor::Human));
    assert!(s.find_block("p-1").is_none(), "撤销插入 → 块消失");
    assert!(s.redo(Actor::Human));
    assert_eq!(
        s.find_block("p-1").unwrap().text,
        "块一",
        "重做 → 块复原（block_id 物化）"
    );
    // 文本编辑撤销（替换 [0,1)："块一" → "XX一"）。
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "p-1".into(),
            start: 0,
            end: 1,
            insert: "XX".into(),
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-1").unwrap().text, "XX一");
    assert!(s.undo(Actor::Human));
    assert_eq!(s.find_block("p-1").unwrap().text, "块一");
    // 属性撤销。
    let mut attrs = BTreeMap::new();
    attrs.insert("level".into(), "3".into());
    s.execute(
        Actor::Human,
        CollabCommand::SetAttrs {
            block_id: "p-1".into(),
            attrs,
        },
    )
    .unwrap();
    assert!(s.undo(Actor::Human));
    assert!(s.find_block("p-1").unwrap().attrs.is_empty());
    // mark 撤销。
    s.execute(
        Actor::Human,
        CollabCommand::SetMark {
            block_id: "p-1".into(),
            start: 0,
            end: 1,
            mark: "em".into(),
            add: true,
        },
    )
    .unwrap();
    assert!(s.undo(Actor::Human));
    assert_eq!(s.find_block("p-1").unwrap().text, "块一");
    // 种类撤销。
    s.execute(
        Actor::Human,
        CollabCommand::SetBlockType {
            block_id: "p-1".into(),
            kind: BlockKind::Heading,
        },
    )
    .unwrap();
    assert_eq!(s.find_block("p-1").unwrap().kind, BlockKind::Heading);
    assert!(s.undo(Actor::Human));
    assert_eq!(s.find_block("p-1").unwrap().kind, BlockKind::Paragraph);
}

/// p20 交错回归锁：双栈（Human/Ai）+ 窗 0 下同 origin 交错编辑，
/// undo/redo 全量复原不 panic、不部分应用（缺陷触发形状的产品面
/// 钉定——若上游回归或配置回退，本测试必失败强制解钉）。
#[test]
fn p20_interleaved_undo_regression_lock() {
    let mut s = CollabSession::open(opts(2)).unwrap();
    // 交错序列：H/A/H/A 各 4 轮文本追加（非连续同 origin 事务——
    // p20 触发形状：捕获窗内同 origin 非连续合并）。
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, ""),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    for _ in 0..4 {
        let len = s.find_block("p-1").unwrap().text.chars().count() as u32;
        s.execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "p-1".into(),
                start: len,
                end: len,
                insert: "h ".into(),
            },
        )
        .unwrap();
        let len = s.find_block("p-1").unwrap().text.chars().count() as u32;
        s.execute(
            Actor::Ai,
            CollabCommand::ReplaceText {
                block_id: "p-1".into(),
                start: len,
                end: len,
                insert: "a ".into(),
            },
        )
        .unwrap();
    }
    let counts = |t: &str| {
        (
            t.matches('h').count(),
            t.matches('a').count(),
            t.matches(' ').count(),
        )
    };
    assert_eq!(counts(&s.find_block("p-1").unwrap().text), (4, 4, 8));
    // Human 栈 5 = 建块插入 + 4 笔文本（窗 0 = 每事务一栈项）；
    // Ai 栈 4（建块是 Human origin）。
    assert_eq!(s.undo_depth(Actor::Human), 5);
    assert_eq!(s.undo_depth(Actor::Ai), 4);
    // Human 面撤 4 笔文本（LIFO——建块插入仍在栈底不动）。
    for i in 0..4 {
        assert!(s.undo(Actor::Human), "第 {} 步 Human 撤销", i + 1);
    }
    // 4 笔 Human 编辑（"h "×4）全部消失，4 笔 Ai 编辑保留。
    assert_eq!(
        counts(&s.find_block("p-1").unwrap().text),
        (0, 4, 4),
        "Human 面全撤后恰余 Ai 编辑（空格随各自对撤除）"
    );
    assert_eq!(s.undo_depth(Actor::Human), 1, "建块插入仍在栈");
    // redo 全量复原。
    for _ in 0..4 {
        assert!(s.redo(Actor::Human));
    }
    assert_eq!(counts(&s.find_block("p-1").unwrap().text), (4, 4, 8));
    // Ai 面同样全环。
    for _ in 0..4 {
        assert!(s.undo(Actor::Ai));
    }
    assert_eq!(
        counts(&s.find_block("p-1").unwrap().text),
        (4, 0, 4),
        "Ai 面全撤后恰余 Human 编辑"
    );
    for _ in 0..4 {
        assert!(s.redo(Actor::Ai));
    }
    assert_eq!(counts(&s.find_block("p-1").unwrap().text), (4, 4, 8));
}

/// 撤销栈软上限（B3.4 粗粒度策略）：超限 clear_all 显式回执。
#[test]
fn undo_stack_soft_limit() {
    let mut s = CollabSession::open(SessionOptions {
        client_id: 3,
        undo_stack_limit: Some(3),
        ..Default::default()
    })
    .unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "x"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    // 2 笔追加编辑恰在限内（insert 已占 depth 1；1+2 = 3 ≤ 3 不清）。
    for i in 0..2 {
        let r = s
            .execute(
                Actor::Human,
                CollabCommand::ReplaceText {
                    block_id: "p-1".into(),
                    start: 1 + i,
                    end: 1 + i,
                    insert: "y".into(),
                },
            )
            .unwrap();
        assert_eq!(r.undo_depth, (i + 2) as usize);
        assert!(!r.undo_cleared);
    }
    assert_eq!(s.undo_depth(Actor::Human), 3);
    // 第 3 笔追加 → depth 4 > 3 → clear_all（回执显式）。
    let r = s
        .execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "p-1".into(),
                start: 3,
                end: 3,
                insert: "z".into(),
            },
        )
        .unwrap();
    assert!(r.undo_cleared, "软上限触发 clear_all 的显式回执");
    assert_eq!(r.undo_depth, 0, "清理发生在操作内——回执深度已归零");
    assert_eq!(s.undo_depth(Actor::Human), 0, "撤销历史全失（代价面）");
    // 编辑本身成功（撤销边界不影响数据面）。
    assert!(s.find_block("p-1").unwrap().text.contains('z'));
    // Ai 栈不受 Human 栈清理影响（分栈独立）。
    s.execute(
        Actor::Ai,
        CollabCommand::ReplaceText {
            block_id: "p-1".into(),
            start: 0,
            end: 0,
            insert: "a".into(),
        },
    )
    .unwrap();
    assert_eq!(s.undo_depth(Actor::Ai), 1);
}

/// 删除 → 撤销 → 复活身份语义（复活 = 新载体 ID 族，block_id 语义
/// 连续——d1e-t1/p17-C 双轴定谳的产品面钉定）。
#[test]
fn remove_then_undo_identity() {
    let mut s = CollabSession::open(opts(4)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, "会被删的"),
            id: Some("p-1".into()),
        },
    )
    .unwrap();
    let anchor = s.bind_anchor("p-1").unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::RemoveBlock {
            block_id: "p-1".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        s.resolve_anchor(anchor),
        Err(mdc_collab::CollabError::AnchorDangling { .. })
    ));
    // 撤销删除 → 块复活，block_id 连续。
    assert!(s.undo(Actor::Human));
    assert_eq!(s.find_block("p-1").unwrap().text, "会被删的");
    // 旧锚点保持悬空（复活 = 新 ID 族——重验通道恢复，不静默跟随）。
    assert!(matches!(
        s.resolve_anchor(anchor),
        Err(mdc_collab::CollabError::AnchorDangling { .. })
    ));
    // 重绑恢复。
    let anchor2 = s.bind_anchor("p-1").unwrap();
    assert_ne!(anchor, anchor2);
    assert!(s.resolve_anchor(anchor2).is_ok());
}

/// 阶段 3 §六.1「检查普通输入的撤销粒度」——实证钉定 + 语义边界。
///
/// 现状（捕获窗 0 = p20 缓解的既定代价）：**每命令一项**。逐字符
/// 输入 = 每字符一项（本测试钉定）；宿主侧输入聚合（如 300ms 去抖
/// 合并为单命令）= 阶段 5 表面实施项（HANDOFF 既有限制登记——p20
/// 窗>0 与交错 origin 是缺陷路径，聚合必须在命令构造侧而非捕获窗）。
/// 本测试同时验证：逐项撤销全链回退正确（语义完整性不因粒度细而破）。
#[test]
fn typing_granularity_pinned_per_command() {
    let mut s = CollabSession::open(opts(41)).unwrap();
    s.execute(
        Actor::Human,
        CollabCommand::InsertBlock {
            parent: None,
            index: 0,
            spec: spec(BlockKind::Paragraph, ""),
            id: Some("p-gran".into()),
        },
    )
    .unwrap();
    let base = s.undo_depth(Actor::Human);

    // 模拟逐字符输入 "hello"（宿主未聚合的最细粒度——每字符一命令）
    for (i, ch) in ['h', 'e', 'l', 'l', 'o'].iter().enumerate() {
        s.execute(
            Actor::Human,
            CollabCommand::ReplaceText {
                block_id: "p-gran".into(),
                start: i as u32,
                end: i as u32,
                insert: ch.to_string(),
            },
        )
        .unwrap();
    }
    assert_eq!(s.find_block("p-gran").unwrap().text, "hello");
    // 粒度钉定：5 命令 = 5 栈项（捕获窗 0——p20 缓解既定代价）
    assert_eq!(
        s.undo_depth(Actor::Human),
        base + 5,
        "每命令一项（逐字符输入 = 逐字符栈项——阶段5宿主聚合前的钉定现状）"
    );

    // 逐项撤销全链回退正确（5 次 → 空文本；第 6 次 = 撤销插入本身）
    for _ in 0..5 {
        assert!(s.undo(Actor::Human));
    }
    assert_eq!(s.find_block("p-gran").unwrap().text, "");

    // 宿主聚合形态（单命令多字符 = 正常路径）：1 命令 1 栈项
    s.execute(
        Actor::Human,
        CollabCommand::ReplaceText {
            block_id: "p-gran".into(),
            start: 0,
            end: 0,
            insert: "hello".into(),
        },
    )
    .unwrap();
    assert_eq!(s.undo_depth(Actor::Human), base + 1, "聚合输入 = 单栈项");
    assert!(s.undo(Actor::Human));
    assert_eq!(s.find_block("p-gran").unwrap().text, "");
}
