//! R9 — sync_from_markdown（全量重同步兜底）语义测试。
//!
//! 覆盖任务要求的最低三项（revision 前进 / undo 撤销 sync / diff 最小性）
//! 外加：尾部增删、嵌套子块 diff、原位改保身份、空差分不动 revision、
//! 以及 commit_source 重建路径的 ID 冲突回归（R9 修复项）。

use mdcore::history::Source;
use mdcore::id::BlockId;
use mdcore::op::Op;
use mdcore::tree::BlockKind;
use mdcore::Core;

fn top_kinds(core: &Core) -> Vec<BlockKind> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).kind)
        .collect()
}

fn top_texts(core: &Core) -> Vec<String> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).text.to_string())
        .collect()
}

fn top_ids(core: &Core) -> Vec<BlockId> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).id)
        .collect()
}

fn last_event_ops(core: &mut Core) -> Vec<Op> {
    let events = core.take_events();
    assert!(!events.is_empty(), "sync 必须在事件流上留痕");
    events.last().unwrap().ops.clone()
}

// 1. sync 后 revision 前进（非空差分恰好 +1）
#[test]
fn sync_advances_revision() {
    let mut core = Core::from_markdown("one\ntwo");
    let r0 = core.revision();
    let receipt = core.sync_from_markdown("one\ntwo\nthree").unwrap();
    assert_eq!(receipt.revision, r0 + 1);
    assert_eq!(top_texts(&core), vec!["one", "two", "three"]);
    // 空差分：不 bump（apply_ops 早退）
    let r1 = core.revision();
    core.sync_from_markdown("one\ntwo\nthree").unwrap();
    assert_eq!(core.revision(), r1, "幂等 sync 不得推进 revision");
}

// 2. undo 能撤掉 sync（文本类与结构类各一）
#[test]
fn undo_reverts_sync() {
    let mut core = Core::from_markdown("one\ntwo");
    let ids_before = top_ids(&core);

    // 结构类 sync（尾部插入）
    core.sync_from_markdown("one\ntwo\nthree").unwrap();
    core.sync_from_markdown("one").unwrap(); // 尾部删除
    assert_eq!(top_texts(&core), vec!["one"]);
    core.undo().unwrap();
    assert_eq!(
        top_texts(&core),
        vec!["one", "two", "three"],
        "undo 恢复删除前状态"
    );
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["one", "two"]);
    assert_eq!(top_ids(&core), ids_before, "undo 后身份复原");

    // 文本类 sync（原位改）
    core.sync_from_markdown("ONE\ntwo").unwrap();
    assert_eq!(top_texts(&core), vec!["ONE", "two"]);
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["one", "two"]);
}

// 3. diff 最小性抽查：同位置只换文本 → 恰好 1 个 ReplaceText，邻块不动
#[test]
fn diff_minimality_inplace_text_only() {
    let mut core = Core::from_markdown("alpha\nbeta");
    let ids = top_ids(&core);
    core.sync_from_markdown("alpha\ngamma").unwrap();
    let ops = last_event_ops(&mut core);
    assert_eq!(
        ops,
        vec![Op::ReplaceText {
            path: vec![1usize],
            start: 0,
            end: 4,
            insert: "gamma".into()
        }],
        "最小差分 = 单个原位 ReplaceText"
    );
    assert_eq!(top_ids(&core), ids, "原位改不得改写身份");
}

// 4. kind / attrs 变化 → SetType + SetAttrs 原位（ID 保持）
#[test]
fn diff_inplace_type_and_attrs_change() {
    let mut core = Core::from_markdown("plain text");
    let id = top_ids(&core)[0];
    core.sync_from_markdown("## plain text").unwrap();
    let ops = last_event_ops(&mut core);
    assert_eq!(ops.len(), 2, "SetType + SetAttrs，无 ReplaceText/结构 op");
    assert!(matches!(
        &ops[0],
        Op::SetBlockType {
            kind: BlockKind::Heading,
            ..
        }
    ));
    assert!(matches!(&ops[1], Op::SetAttrs { .. }));
    assert_eq!(core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id, id);
    assert_eq!(top_kinds(&core), vec![BlockKind::Heading]);
    assert_eq!(
        core.tree
            .node(core.tree.resolve(&[0usize]).unwrap())
            .attrs
            .get("level"),
        Some(&"2".to_string())
    );
}

// 5. 尾部增删混合：中位对齐 + 高位先删
#[test]
fn diff_tail_remove_then_insert() {
    let mut core = Core::from_markdown("a\nb\nc\nd");
    core.sync_from_markdown("a\nX\nd\nnew").unwrap();
    let ops = last_event_ops(&mut core);
    // 位置对照：[0] a 不动；[1] b→X；[2] c→d；[3] d→new
    assert_eq!(
        ops,
        vec![
            Op::ReplaceText {
                path: vec![1usize],
                start: 0,
                end: 1,
                insert: "X".into()
            },
            Op::ReplaceText {
                path: vec![2usize],
                start: 0,
                end: 1,
                insert: "d".into()
            },
            Op::ReplaceText {
                path: vec![3usize],
                start: 0,
                end: 1,
                insert: "new".into()
            },
        ],
        "等长对照全部走原位 ReplaceText"
    );
    assert_eq!(top_texts(&core), vec!["a", "X", "d", "new"]);
    assert_eq!(top_kinds(&core), vec![BlockKind::Paragraph; 4]);
}

// 6. 嵌套子块 diff：quote 内段落原位改 + 子块插入（NodePath 路径正确性）
#[test]
fn diff_nested_children() {
    let mut core = Core::from_markdown("> old line");
    // quote 容器 [0]，子段落 [0,0]
    let quote_id = top_ids(&core)[0];
    core.sync_from_markdown("> new line\n> second").unwrap();
    let ops = last_event_ops(&mut core);
    assert!(
        ops.contains(&Op::ReplaceText {
            path: vec![0usize, 0],
            start: 0,
            end: 8,
            insert: "new line".into()
        }),
        "子块原位 ReplaceText，路径 [0,0]：ops={ops:?}"
    );
    assert!(
        ops.iter().any(|o| matches!(o, Op::InsertBlock { parent, index, .. } if parent == &vec![0usize] && *index == 1)),
        "第二个 quote 行 = 容器内新子块（parent=[0], index=1）：ops={ops:?}"
    );
    assert_eq!(top_ids(&core)[0], quote_id, "容器身份保持");
    let md = mdcore::markdown::tree_to_markdown(&core.tree);
    assert_eq!(md, "> new line\n> second");
}

// 7. sync 走 apply_ops 管道：事件 source/command、历史条目、合并窗语义一致
#[test]
fn sync_goes_through_pipeline() {
    let mut core = Core::from_markdown("a\nb");
    core.sync_from_markdown("a\nb\nc").unwrap();
    let events = core.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].meta.source, Source::View);
    assert_eq!(
        events[0].meta.command.as_deref(),
        Some("sync_from_markdown")
    );
    assert_eq!(events[0].ops.len(), 1, "单个 InsertBlock");
    assert_eq!(core.history.undo_depth(), 1);

    // 与快路径打字同窗合并（同为 View 文本类——桥 IME 兜底依赖此语义）
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![2usize],
            start: 1,
            end: 1,
        },
        "x".into(),
    )
    .unwrap();
    assert_eq!(
        core.history.undo_depth(),
        2,
        "replace_text 是 text 类，同窗应并入 sync 条目"
    );
    core.undo().unwrap();
    assert_eq!(
        top_texts(&core),
        vec!["a", "b", "c"],
        "一次 undo 同时撤掉打字+sync"
    );
}

// 8. 回归（R9 修复）：commit_source 重建路径不得把解析树自造 ID 物化进活树
#[test]
fn commit_source_rebuild_does_not_collide_ids() {
    let mut core = Core::from_markdown("alpha\nbeta");
    let alpha_id = top_ids(&core)[0];
    // 单块含子块 → 走重建路径；解析树 ID 从 1 起号，与现树冲突
    core.commit_source(core.revision(), vec![1usize], "- item one\n  - nested")
        .unwrap();
    // alpha 的 ID 仍解析到 alpha（冲突会覆盖注册表使其指向新块）
    let slot = core.tree.lookup_id(alpha_id).expect("alpha 必须仍在注册表");
    assert_eq!(core.tree.node(slot).text.to_string(), "alpha");
    // 新子树的 ID 现场分配（高于既有水位），绝不与活块冲突；
    // 子块 [1,0] 同样是分配出来的新 ID（修复前会拿到解析器自造的 1/2）。
    let item_id = core.tree.node(core.tree.resolve(&[1usize]).unwrap()).id;
    let nested_id = core.tree.node(core.tree.resolve(&[1usize, 0]).unwrap()).id;
    assert!(
        item_id.0 >= 3 && nested_id.0 > item_id.0,
        "新 ID 现场分配且不冲突: {item_id:?}/{nested_id:?}"
    );
    assert_eq!(top_texts(&core), vec!["alpha", "item one"]);
    assert_eq!(
        core.tree
            .node(core.tree.resolve(&[1usize, 0]).unwrap())
            .text
            .to_string(),
        "nested"
    );
}

// 9. 块数收缩 + 结构类型混杂（fence 吞行场景）
#[test]
fn diff_shrink_and_fence() {
    let mut core = Core::from_markdown("one\ntwo\nthree");
    core.sync_from_markdown("```js\none\n```\nthree").unwrap();
    let kinds = top_kinds(&core);
    assert_eq!(kinds, vec![BlockKind::CodeBlock, BlockKind::Paragraph]);
    assert_eq!(top_texts(&core), vec!["one", "three"]);
    // undo 复原回三段
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["one", "two", "three"]);
}

// 10. 空文档边界：从空核 sync 建块，再 sync 回空
#[test]
fn sync_empty_doc_boundaries() {
    let mut core = Core::new();
    core.sync_from_markdown("hello").unwrap();
    assert_eq!(top_texts(&core), vec!["hello"]);
    assert_eq!(core.revision(), 1);
    core.sync_from_markdown("").unwrap();
    assert!(core.tree.top_level().is_empty());
    assert_eq!(core.revision(), 2);
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["hello"], "undo 恢复被清空的块");
}
