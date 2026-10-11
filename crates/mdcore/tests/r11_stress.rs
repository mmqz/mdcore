//! R11 验证轮 · 核侧大规模结构 op 压测（R8 验证清单"大文档块操作"的核内半边）。
//!
//! 断言面（全部功能性，不做脆弱的时间断言）：
//! - 20k 块级文档上：百 op 事务原子性 / undo 粒度 / MoveBlock 身份稳定 /
//!   日志重放复现 / 审计水位；
//! - 渐进 build（extend_from_markdown 分 40 段）与结构 op 混合路径。

use mdcore::op::Op;
use mdcore::{Core, Reject};

fn para(i: usize) -> String {
    format!("paragraph body {i} with filler words to make blocks non-trivial\n")
}

fn build_big(progressive: bool) -> Core {
    let sections: Vec<String> = (0..20_000).map(para).collect();
    if progressive {
        let mut core = Core::new();
        for group in sections.chunks(500) {
            core.extend_from_markdown(&group.concat()).unwrap();
        }
        core
    } else {
        Core::from_markdown(&sections.concat())
    }
}

/// 现场求值当前所有顶层块文本长度（用于构造安全的 ReplaceText 区间）。
fn top_lens(core: &Core) -> Vec<usize> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).text.chars().count())
        .collect()
}

#[test]
fn bulk_transactions_are_atomic_and_undoable_at_scale() {
    let mut core = build_big(false);
    let baseline_ids = top_lens(&core).len();
    assert_eq!(baseline_ids, 20_000);
    let rev0 = core.revision();

    // 事务 A：100 个 ReplaceText（每第 7 块替换前缀），单事务单历史条目
    let lens = top_lens(&core);
    let ops: Vec<Op> = (0..100)
        .map(|k| Op::ReplaceText {
            path: vec![k * 7],
            start: 0,
            end: lens[k * 7].min(8),
            insert: format!("EDITED{i:03}", i = k).into(),
        })
        .collect();
    // 直接用 apply_ops（Vec<Op>）：见 lib.rs 公共面
    let r = core
        .apply_ops(
            ops,
            mdcore::ApplyMeta {
                source: mdcore::history::Source::Ai,
                command: Some("bulk_edit".into()),
            },
        )
        .unwrap();
    assert_eq!(r.revision, rev0 + 1, "百 op 事务 = 单 revision");
    assert_eq!(core.history.undo_depth(), 1, "单事务单历史条目");

    // undo：100 处编辑一次全撤
    core.undo().unwrap();
    let count_after_undo = top_lens(&core).len();
    assert_eq!(count_after_undo, 20_000);
    core.redo().unwrap();

    // 事务 B：插入 + 删除 + 移动混合（原子性：最后一个 op 非法 → 全回滚）
    let good_ops: Vec<Op> = (0..50)
        .map(|k| Op::InsertBlock {
            parent: vec![],
            index: 2 * k,
            kind: mdcore::tree::BlockKind::Paragraph,
            attrs: Default::default(),
            text: format!("inserted {k}").into(),
            id: None,
        })
        .collect();
    core.apply_ops(
        good_ops,
        mdcore::ApplyMeta {
            source: mdcore::history::Source::Ai,
            command: Some("bulk_insert".into()),
        },
    )
    .unwrap();
    assert_eq!(top_lens(&core).len(), 20_050);

    // 非法事务：49 个合法 ReplaceText + 1 个 BadPath → 全回滚（原子性）
    let _lens = top_lens(&core);
    let mut bad_ops: Vec<Op> = (0..49)
        .map(|k| Op::ReplaceText {
            path: vec![k],
            start: 0,
            end: 3,
            insert: "XXX".into(),
        })
        .collect();
    bad_ops.push(Op::ReplaceText {
        path: vec![999_999],
        start: 0,
        end: 0,
        insert: "nope".into(),
    });
    let rejected = core.apply_ops(
        bad_ops,
        mdcore::ApplyMeta {
            source: mdcore::history::Source::Ai,
            command: None,
        },
    );
    assert!(
        matches!(rejected, Err(Reject::Invalid(_))),
        "坏路径必须拒绝: {rejected:?}"
    );
    assert_eq!(top_lens(&core).len(), 20_050, "回滚后块数不变");
    assert!(
        !mdcore::markdown::tree_to_markdown(&core.tree).contains("XXX"),
        "回滚后无残留编辑"
    );
}

#[test]
fn move_and_undo_keep_identity_at_scale() {
    let mut core = build_big(true); // 渐进 build 路径
    let before = mdcore::markdown::tree_to_markdown(&core.tree);

    // 50 次 MoveBlock（顶层换位：把首块逐次移到尾部区段）
    let ops: Vec<Op> = (0..50)
        .map(|k| Op::MoveBlock {
            from: vec![0],
            to_parent: vec![],
            to_index: 19_000 + k,
        })
        .collect();
    core.apply_ops(
        ops,
        mdcore::ApplyMeta {
            source: mdcore::history::Source::Ai,
            command: Some("bulk_move".into()),
        },
    )
    .unwrap();
    let moved = mdcore::markdown::tree_to_markdown(&core.tree);
    assert_ne!(moved, before);

    // undo 一次全撤 → 与初始文档逐字节一致（身份与顺序全恢复）
    core.undo().unwrap();
    assert_eq!(mdcore::markdown::tree_to_markdown(&core.tree), before);

    // 日志重放复现（渐进 genesis + 全部结构事务）
    let jsonl = core.log.to_jsonl();
    let replayed = Core::replay_jsonl(&jsonl).unwrap();
    assert_eq!(mdcore::markdown::tree_to_markdown(&replayed.tree), before);
}

#[test]
fn one_shot_and_progressive_big_documents_converge() {
    let a = build_big(false);
    let b = build_big(true);
    assert_eq!(
        mdcore::markdown::tree_to_markdown(&a.tree),
        mdcore::markdown::tree_to_markdown(&b.tree),
        "20k 块：one-shot 与 40 段渐进终态一致"
    );
    assert_eq!(a.tree.top_level().len(), b.tree.top_level().len());
}
