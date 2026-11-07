//! Spike A-E 语义测试（主档 §13 + §22 十条不变量的 R8 子集）。
//!
//! - A：Document / Block Tree（父子、序列化、restore、identity）
//! - B：Block Identity（§8.2 全操作矩阵硬断言）
//! - C：Command Protocol（十原语 + transaction）
//! - D：History（undo/redo/replay/snapshot restore + AI 独立条目）
//! - E：Stale Revision + Anchor（结构 stale reject / 锚点映射 / expect 保护）
//!
//! Spike F（真 IME + 渐进打开 TTI）需要真实 WebView，归 R9。
//! 已知语义边界：mark 与编辑区间部分重叠的截断为有损操作
//! （undo 不复原被截掉的 span——与 CM6 映射语义一致，README 登记）。

use std::collections::BTreeMap;
use std::sync::Arc;

use mdcore::anchor::{AnchorError, TextTarget};
use mdcore::history::Source;
use mdcore::id::BlockId;
use mdcore::op::Op;
use mdcore::tree::{BlockKind, MarkSpan};
use mdcore::{ApplyMeta, Core, Reject};

const SAMPLE: &str = "# Title\ntext para\n- item one\n- item two\n  - nested\n> quoted\n```rust\nfn main() {}\nlet x = 1;\n```\n| a | b |\n| 1 | 2 |\n---";

fn clock_of(cell: &Arc<std::sync::Mutex<u64>>) -> Arc<dyn Fn() -> u64 + Send + Sync> {
    let cell = Arc::clone(cell);
    Arc::new(move || *cell.lock().unwrap())
}

fn top_kinds(core: &Core) -> Vec<BlockKind> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).kind)
        .collect()
}

fn top_ids(core: &Core) -> Vec<BlockId> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).id)
        .collect()
}

fn text_at(core: &Core, path: &[usize]) -> String {
    core.tree
        .node(core.tree.resolve(path).unwrap())
        .text
        .to_string()
}

fn kind_at(core: &Core, path: &[usize]) -> BlockKind {
    core.tree.node(core.tree.resolve(path).unwrap()).kind
}

// ---------------------------------------------------------------------------
// Spike A — Document / Block Tree
// ---------------------------------------------------------------------------

#[test]
fn spike_a_tree_shape_and_nesting() {
    let core = Core::from_markdown(SAMPLE);
    assert_eq!(
        top_kinds(&core),
        vec![
            BlockKind::Heading,
            BlockKind::Paragraph,
            BlockKind::ListItem,
            BlockKind::ListItem,
            BlockKind::Blockquote,
            BlockKind::CodeBlock,
            BlockKind::Table,
            BlockKind::ThematicBreak,
        ]
    );
    // 嵌套：item two 挂 nested 子项；quote 挂段落子块
    let items = core.tree.top_level();
    let item_two = items[3];
    let nested = core.tree.node(item_two).children[0];
    assert_eq!(core.tree.node(nested).kind, BlockKind::ListItem);
    assert_eq!(core.tree.node(nested).text.to_string(), "nested");

    let quote = items[4];
    assert_eq!(
        core.tree
            .node(core.tree.node(quote).children[0])
            .text
            .to_string(),
        "quoted"
    );
    // code fence 多行内容完整
    assert_eq!(text_at(&core, &[5]), "fn main() {}\nlet x = 1;");
}

#[test]
fn spike_a_markdown_roundtrip_idempotent() {
    let core = Core::from_markdown(SAMPLE);
    let md1 = mdcore::markdown::tree_to_markdown(&core.tree);
    let core2 = Core::from_markdown(&md1);
    let md2 = mdcore::markdown::tree_to_markdown(&core2.tree);
    assert_eq!(md1, md2, "serialize(parse(md)) 必须是不动点");
    assert_eq!(top_kinds(&core), top_kinds(&core2));
}

#[test]
fn spike_a_snapshot_roundtrip_preserves_state_and_ids() {
    let core = Core::from_markdown(SAMPLE);
    let before_ids = top_ids(&core);
    let before_dump = mdcore::debug::dump_tree(&core.tree);

    let json = core.snapshot().to_json();
    let restored = Core::restore(mdcore::snapshot::Snapshot::from_json(&json).unwrap());

    assert_eq!(mdcore::debug::dump_tree(&restored.tree), before_dump);
    assert_eq!(top_ids(&restored), before_ids, "restore 必须复原身份");
    assert_eq!(restored.revision(), core.revision());
}

// ---------------------------------------------------------------------------
// Spike B — Block Identity（§8.2 硬断言矩阵）
// ---------------------------------------------------------------------------

#[test]
fn spike_b_identity_invariants_matrix() {
    let mut core = Core::from_markdown(SAMPLE);
    let base = core.revision();
    let para_path = vec![1usize];
    let para_id = core.tree.node(core.tree.resolve(&para_path).unwrap()).id;

    // 文本修改 → 保持
    core.replace_text(
        TextTarget::ByPath {
            path: para_path.clone(),
            start: 0,
            end: 0,
        },
        "X".into(),
    )
    .unwrap();
    assert_eq!(kind_at(&core, &para_path), BlockKind::Paragraph);
    assert_eq!(
        core.tree.node(core.tree.resolve(&para_path).unwrap()).id,
        para_id
    );

    // 属性修改 → 保持
    let mut attrs = BTreeMap::new();
    attrs.insert("k".to_string(), "v".to_string());
    core.set_attrs(core.revision(), para_path.clone(), attrs)
        .unwrap();
    assert_eq!(
        core.tree.node(core.tree.resolve(&para_path).unwrap()).id,
        para_id
    );

    // 类型变更 → 保持
    core.set_block_type(core.revision(), para_path.clone(), BlockKind::Heading)
        .unwrap();
    assert_eq!(
        core.tree.node(core.tree.resolve(&para_path).unwrap()).id,
        para_id
    );

    // 移动 → 保持（ID 跟随到新路径）
    core.move_block(core.revision(), para_path.clone(), vec![], 0)
        .unwrap();
    assert_eq!(
        core.tree
            .path_of(core.tree.lookup_id(para_id).unwrap())
            .unwrap(),
        vec![0usize],
        "移动后 ID 解析到新路径"
    );

    // split → 原 ID 留左侧，右侧新 ID
    core.split_block(core.revision(), vec![0usize], 3).unwrap();
    assert_eq!(
        core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id,
        para_id
    );
    let right_id = core.tree.node(core.tree.resolve(&[1usize]).unwrap()).id;
    assert_ne!(right_id, para_id);
    assert_eq!(text_at(&core, &[0]), "Xte");
    assert_eq!(text_at(&core, &[1]), "xt para");

    // merge → survivor = 前一个（默认前/目标 ID），右侧 ID 失效
    core.merge_block(core.revision(), vec![1usize]).unwrap();
    assert_eq!(
        core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id,
        para_id
    );
    assert_eq!(core.tree.lookup_id(right_id), None, "被并块 ID 失效");
    assert_eq!(text_at(&core, &[0]), "Xtext para");

    assert_eq!(core.revision(), base + 6, "六个命令各推进一个 revision");
}

#[test]
fn spike_b_delete_invalidates_copy_reallocates() {
    let mut core = Core::from_markdown(SAMPLE);

    // delete → 失效
    let id = core.tree.node(core.tree.resolve(&[1usize]).unwrap()).id;
    core.delete_block(core.revision(), vec![1usize]).unwrap();
    assert_eq!(core.tree.lookup_id(id), None);

    // copy（tree.deep_clone）→ 全部新 ID（§8.2 copy 规则）
    let src = core.tree.top_level()[1]; // item one
    let data = core.tree.collect_subtree(src);
    let fresh = core.tree.deep_clone(&data);
    assert_ne!(core.tree.node(fresh).id, data.id, "copy 不得继承身份");
    assert_eq!(
        core.tree.node(fresh).children.len(),
        data.children.len(),
        "子树形状一致"
    );

    // 新分配 ID 只增不减（水位）
    let wm = core.tree.alloc.watermark();
    core.insert_block(
        core.revision(),
        vec![],
        0,
        BlockKind::Paragraph,
        BTreeMap::new(),
        "new".into(),
    )
    .unwrap();
    assert!(core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id.0 >= wm);
}

// ---------------------------------------------------------------------------
// Spike C — Command Protocol（十原语 + transaction）
// ---------------------------------------------------------------------------

#[test]
fn spike_c_all_first_stage_primitives() {
    let mut core = Core::from_markdown(SAMPLE);
    let rev = core.revision();

    // 1 insert_block（H2 到最前）
    let mut attrs = BTreeMap::new();
    attrs.insert("level".to_string(), "2".to_string());
    core.insert_block(rev, vec![], 0, BlockKind::Heading, attrs, "H2".into())
        .unwrap();
    assert_eq!(top_kinds(&core)[0], BlockKind::Heading);

    // 2 replace_text（path 快路径）
    core.replace_text(
        TextTarget::ByPath {
            path: vec![1usize],
            start: 0,
            end: 0,
        },
        ">> ".into(),
    )
    .unwrap();
    // 3 set_block_type
    core.set_block_type(core.revision(), vec![1usize], BlockKind::Blockquote)
        .unwrap();
    // 4 set_attrs（code fence 改语言）
    let mut attrs = BTreeMap::new();
    attrs.insert("lang".to_string(), "python".to_string());
    core.set_attrs(core.revision(), vec![5usize], attrs)
        .unwrap();
    assert_eq!(
        core.tree
            .node(core.tree.resolve(&[5usize]).unwrap())
            .attrs
            .get("lang")
            .map(String::as_str),
        Some("python")
    );
    // 5 set_mark（加粗段落前 5 字符）
    core.set_mark(
        TextTarget::ByPath {
            path: vec![1usize],
            start: 0,
            end: 5,
        },
        "bold".into(),
        true,
    )
    .unwrap();
    // 6 move_block（代码块移到最前；插入后原 [6] 为 CodeBlock）
    core.move_block(core.revision(), vec![6usize], vec![], 0)
        .unwrap();
    assert_eq!(top_kinds(&core)[0], BlockKind::CodeBlock);
    // 7 split_block（[3] 为 Heading "Title"，len 5）
    core.split_block(core.revision(), vec![3usize], 3).unwrap();
    assert_eq!(core.tree.top_level().len(), 10);
    // 8 merge_block（并回上一个）
    core.merge_block(core.revision(), vec![2usize]).unwrap();
    assert_eq!(core.tree.top_level().len(), 9);
    // 9 delete_block
    core.delete_block(core.revision(), vec![0usize]).unwrap();
    assert_eq!(core.tree.top_level().len(), 8);

    // 10 transaction：一批 op 一条历史
    let depth_before = core.history.undo_depth();
    core.commit_transaction(
        core.revision(),
        vec![
            Op::InsertBlock {
                parent: vec![],
                index: 0,
                kind: BlockKind::Paragraph,
                attrs: Default::default(),
                text: "a".into(),
                id: None,
            },
            Op::InsertBlock {
                parent: vec![],
                index: 1,
                kind: BlockKind::Paragraph,
                attrs: Default::default(),
                text: "b".into(),
                id: None,
            },
            Op::ReplaceText {
                path: vec![0usize],
                start: 1,
                end: 1,
                insert: "!".into(),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        core.history.undo_depth(),
        depth_before + 1,
        "事务 = 单条历史"
    );

    // commit_source（View A 块编辑回解析；单原子块原位改 → ID 保持）
    let before = core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id;
    core.commit_source(core.revision(), vec![0usize], "## committed")
        .unwrap();
    assert_eq!(kind_at(&core, &[0]), BlockKind::Heading);
    assert_eq!(
        core.tree
            .node(core.tree.resolve(&[0usize]).unwrap())
            .attrs
            .get("level")
            .map(String::as_str),
        Some("2")
    );
    assert_eq!(text_at(&core, &[0]), "committed");
    assert_eq!(
        core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id,
        before
    );

    // 全程事件在流上
    assert!(core.take_events().len() >= 11);
}

// ---------------------------------------------------------------------------
// Spike D — History
// ---------------------------------------------------------------------------

#[test]
fn spike_d_coalescing_window_and_semantic_break() {
    let now = Arc::new(std::sync::Mutex::new(0u64));
    let mut core = Core::with_clock(clock_of(&now));
    core.tree = mdcore::markdown::markdown_to_tree("para");

    // 同窗口内连续击键 → 单条目（§11.1 无 revision storm）
    for t in [0u64, 100, 200, 300] {
        *now.lock().unwrap() = t;
        core.replace_text(
            TextTarget::ByPath {
                path: vec![0usize],
                start: 4,
                end: 4,
            },
            "x".into(),
        )
        .unwrap();
    }
    assert_eq!(text_at(&core, &[0]), "paraxxxx");
    assert_eq!(core.history.undo_depth(), 1);
    assert_eq!(core.revision(), 1, "合并击键共享一个 revision");

    // 语义破组：结构命令立即开新条目
    *now.lock().unwrap() = 400;
    core.set_block_type(core.revision(), vec![0usize], BlockKind::Heading)
        .unwrap();
    assert_eq!(core.history.undo_depth(), 2);

    // 窗口过期后的文本编辑 → 新条目
    *now.lock().unwrap() = 10_000;
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0usize],
            start: 8,
            end: 8,
        },
        "y".into(),
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), 3);

    // undo 链：y-entry → heading-entry
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[0]), "paraxxxx");
    core.undo().unwrap();
    assert_eq!(kind_at(&core, &[0]), BlockKind::Paragraph);
    assert_eq!(text_at(&core, &[0]), "paraxxxx");
    core.redo().unwrap();
    assert_eq!(kind_at(&core, &[0]), BlockKind::Heading);
}

#[test]
fn spike_d_ai_ops_are_first_class_history() {
    let mut core = Core::from_markdown("hello");
    let id = core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id;
    let before = core.history.undo_depth();

    // AI 变更：独立条目（source != View 永不合并）+ 可 undo
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0usize],
            start: 5,
            end: 5,
            insert: " world".into(),
        }],
        ApplyMeta {
            source: Source::Ai,
            command: Some("ai.append".into()),
        },
    )
    .unwrap();
    core.apply_ops(
        vec![Op::SetBlockType {
            path: vec![0usize],
            kind: BlockKind::Heading,
        }],
        ApplyMeta {
            source: Source::Ai,
            command: Some("ai.rewrite".into()),
        },
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), before + 2);

    // 与人类击键不合并：紧跟一次 View 文本编辑 → 又一条新条目
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0usize],
            start: 11,
            end: 11,
        },
        "!".into(),
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), before + 3);

    // undo 链：撤销 "!" → 撤销 heading → 撤销 " world"
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[0]), "hello world");
    core.undo().unwrap();
    assert_eq!(kind_at(&core, &[0]), BlockKind::Paragraph);
    core.undo().unwrap();
    assert_eq!(
        core.tree
            .node(core.tree.lookup_id(id).unwrap())
            .text
            .to_string(),
        "hello"
    );

    let events = core.take_events();
    assert!(events.iter().any(|e| e.meta.source == Source::Undo));
    assert!(events
        .iter()
        .any(|e| e.meta.command.as_deref() == Some("ai.append")));
}

#[test]
fn spike_d_history_json_roundtrip_and_log_replay() {
    let mut core = Core::from_markdown(SAMPLE);
    core.replace_text(
        TextTarget::ByPath {
            path: vec![1usize],
            start: 4,
            end: 8,
        },
        "TEXT".into(),
    )
    .unwrap();
    core.split_block(core.revision(), vec![1usize], 3).unwrap();
    core.apply_ops(
        vec![Op::SetBlockType {
            path: vec![2usize],
            kind: BlockKind::Heading,
        }],
        ApplyMeta::ai("ai.style"),
    )
    .unwrap();
    // split 后：[0]H [1]"tex" [2]"t TEXT"(AI 改 heading) ...
    assert_eq!(kind_at(&core, &[2]), BlockKind::Heading);

    // 历史整栈 JSON 往返（S6）→ 恢复后 undo 栈可用
    let hj = core.history_json();
    let snap_json = core.snapshot().to_json();
    let mut restored = Core::restore(mdcore::snapshot::Snapshot::from_json(&snap_json).unwrap());
    restored.set_history_json(&hj).unwrap();
    assert_eq!(restored.history.undo_depth(), core.history.undo_depth());
    restored.undo().unwrap();
    assert_eq!(
        kind_at(&restored, &[2]),
        BlockKind::Paragraph,
        "恢复的 undo 撤销 AI 改写"
    );
    restored.redo().unwrap();
    assert_eq!(kind_at(&restored, &[2]), BlockKind::Heading);

    // Revision 日志重放：状态 + revision 复原（前向流确定性）
    let jsonl = core.log.to_jsonl();
    let replayed = Core::replay_jsonl(&jsonl).unwrap();
    assert_eq!(
        mdcore::debug::dump_tree(&replayed.tree),
        mdcore::debug::dump_tree(&core.tree)
    );
    assert_eq!(replayed.revision(), core.revision());
}

#[test]
fn spike_d_snapshot_midstream_and_allocator_watermark() {
    let mut core = Core::from_markdown("one\ntwo");
    let snap = core.snapshot();

    // 快照点之后继续编辑
    core.delete_block(core.revision(), vec![0usize]).unwrap();
    core.insert_block(
        core.revision(),
        vec![],
        0,
        BlockKind::Paragraph,
        BTreeMap::new(),
        "fresh".into(),
    )
    .unwrap();

    // restore 到快照点：两块俱在，revision 回退
    let mut restored = Core::restore(snap);
    assert_eq!(restored.tree.node_count(), 2);
    assert_eq!(restored.revision(), 0);
    let wm = restored.tree.alloc.watermark();
    assert!(wm >= 3, "水位 ≥ 快照时已分配 ID 数");

    // restore 后新分配不与历史撞车
    restored
        .insert_block(
            restored.revision(),
            vec![],
            0,
            BlockKind::Paragraph,
            BTreeMap::new(),
            "post".into(),
        )
        .unwrap();
    assert!(core.tree.node(core.tree.resolve(&[0usize]).unwrap()).id.0 >= 1);
    let new_id = restored
        .tree
        .node(restored.tree.resolve(&[0usize]).unwrap())
        .id;
    assert!(new_id.0 >= wm);
}

// ---------------------------------------------------------------------------
// Spike E — Stale Revision + Anchor
// ---------------------------------------------------------------------------

#[test]
fn spike_e_structural_stale_must_reject() {
    let mut core = Core::from_markdown("a\nb");
    let depth0 = core.history.undo_depth();

    core.replace_text(
        TextTarget::ByPath {
            path: vec![0usize],
            start: 0,
            end: 1,
        },
        "x".into(),
    )
    .unwrap(); // revision → 1
    let dump_after_edit = mdcore::debug::dump_tree(&core.tree);
    let events_before = core.take_events().len();

    // 旧 base_revision 的结构命令 → reject（§11.2，不静默 rebase）
    let err = core.delete_block(0, vec![1usize]).unwrap_err();
    assert_eq!(err, Reject::Stale { current: 1 });
    assert_eq!(
        mdcore::debug::dump_tree(&core.tree),
        dump_after_edit,
        "reject 不得改动状态"
    );
    assert_eq!(core.history.undo_depth(), depth0 + 1, "reject 不产生历史");
    assert_eq!(core.take_events().len(), 0, "reject 不产生事件");
    let _ = events_before;

    // 用当前 base 重试 → 成功
    core.delete_block(core.revision(), vec![1usize]).unwrap();
    assert_eq!(core.tree.node_count(), 1);
}

#[test]
fn spike_e_anchor_mapping_and_rejects() {
    let mut core = Core::from_markdown("alpha\nbeta\ngamma");
    let b_beta = core.tree.node(core.tree.resolve(&[1usize]).unwrap()).id;
    let b_gamma = core.tree.node(core.tree.resolve(&[2usize]).unwrap()).id;

    // ① 移动块之后：锚点仍解析到新路径（ADR-B1 混合式的价值时刻）
    core.move_block(core.revision(), vec![0usize], vec![], 2)
        .unwrap();
    // [0]=beta [1]=gamma [2]=alpha
    assert_eq!(text_at(&core, &[0]), "beta");
    core.replace_text(
        TextTarget::ByAnchor {
            block: b_beta,
            start: 0,
            end: 4,
            expect: Some("beta".into()),
        },
        "BETA".into(),
    )
    .unwrap();
    assert_eq!(text_at(&core, &[0]), "BETA");

    // ② expect 不匹配 → reject
    let err = core.replace_text(
        TextTarget::ByAnchor {
            block: b_beta,
            start: 0,
            end: 4,
            expect: Some("xxxx".into()),
        },
        "y".into(),
    );
    assert_eq!(
        err,
        Err(Reject::Anchor(AnchorError::ExpectMismatch {
            block: b_beta
        }))
    );

    // ③ 越界 → reject
    let err = core.replace_text(
        TextTarget::ByAnchor {
            block: b_beta,
            start: 0,
            end: 99,
            expect: None,
        },
        "y".into(),
    );
    assert!(matches!(
        err,
        Err(Reject::Anchor(AnchorError::OutOfRange { .. }))
    ));

    // ④ 块被删 → Missing → reject（绝不猜测，§8.4 原则）
    core.delete_block(core.revision(), vec![1usize]).unwrap();
    let err = core.replace_text(
        TextTarget::ByAnchor {
            block: b_gamma,
            start: 0,
            end: 2,
            expect: None,
        },
        "z".into(),
    );
    assert_eq!(err, Err(Reject::Anchor(AnchorError::Missing(b_gamma))));
}

#[test]
fn spike_e_marks_edit_semantics() {
    let now = Arc::new(std::sync::Mutex::new(0u64));
    let mut core = Core::with_clock(clock_of(&now));
    core.tree = mdcore::markdown::markdown_to_tree("hello world");
    let p = vec![0usize];
    let tick = |now: &Arc<std::sync::Mutex<u64>>| {
        let mut t = now.lock().unwrap();
        *t += 10_000;
        *t
    };

    *now.lock().unwrap() = tick(&now);
    core.set_mark(
        TextTarget::ByPath {
            path: p.clone(),
            start: 0,
            end: 5,
        },
        "bold".into(),
        true,
    )
    .unwrap();

    // 区间内插入 → span 吞下插入（CM6 同款，可逆）；时间拉开确保独立历史条目
    *now.lock().unwrap() = tick(&now);
    core.replace_text(
        TextTarget::ByPath {
            path: p.clone(),
            start: 2,
            end: 2,
        },
        "XX".into(),
    )
    .unwrap();
    let node = core.tree.node(core.tree.resolve(&p).unwrap());
    assert_eq!(node.text.to_string(), "heXXllo world");
    assert_eq!(
        node.marks,
        vec![MarkSpan {
            start: 0,
            end: 7,
            kind: "bold".into()
        }]
    );
    core.undo().unwrap();
    let node = core.tree.node(core.tree.resolve(&p).unwrap());
    assert_eq!(node.text.to_string(), "hello world");
    assert_eq!(
        node.marks,
        vec![MarkSpan {
            start: 0,
            end: 5,
            kind: "bold".into()
        }]
    );

    // 编辑区后方平移（可逆）
    *now.lock().unwrap() = tick(&now);
    core.replace_text(
        TextTarget::ByPath {
            path: p.clone(),
            start: 6,
            end: 6,
        },
        "dear ".into(),
    )
    .unwrap();
    let node = core.tree.node(core.tree.resolve(&p).unwrap());
    assert_eq!(
        node.marks,
        vec![MarkSpan {
            start: 0,
            end: 5,
            kind: "bold".into()
        }]
    );
    core.undo().unwrap();
    assert_eq!(
        core.tree.node(core.tree.resolve(&p).unwrap()).marks,
        vec![MarkSpan {
            start: 0,
            end: 5,
            kind: "bold".into()
        }]
    );

    // 跨越区间的删除 → span 截断（v0 有损语义，登记 README）
    *now.lock().unwrap() = tick(&now);
    core.replace_text(
        TextTarget::ByPath {
            path: p.clone(),
            start: 3,
            end: 8,
        },
        String::new(),
    )
    .unwrap();
    let node = core.tree.node(core.tree.resolve(&p).unwrap());
    assert_eq!(node.text.to_string(), "helrld");
    assert_eq!(
        node.marks,
        vec![MarkSpan {
            start: 0,
            end: 3,
            kind: "bold".into()
        }]
    );

    // set_mark(false) 移除
    *now.lock().unwrap() = tick(&now);
    core.set_mark(
        TextTarget::ByPath {
            path: p.clone(),
            start: 0,
            end: 3,
        },
        "bold".into(),
        false,
    )
    .unwrap();
    assert!(core
        .tree
        .node(core.tree.resolve(&p).unwrap())
        .marks
        .is_empty());
}

// ---------------------------------------------------------------------------
// §22 不变量 1/2/10：Human 与 AI 同一 Document State、同一 op 流；
// Core 不含任何 View 概念（结构性保证，编译期成立）。
// ---------------------------------------------------------------------------

#[test]
fn invariants_shared_op_stream() {
    let mut core = Core::from_markdown("base");
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0usize],
            start: 4,
            end: 4,
        },
        " human".into(),
    )
    .unwrap();
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0usize],
            start: 0,
            end: 0,
            insert: "AI: ".into(),
        }],
        ApplyMeta::ai("ai.prefix"),
    )
    .unwrap();

    // 同一事件流 = View 与 AI 的共同语言
    let events = core.take_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].meta.source, Source::View);
    assert_eq!(events[1].meta.source, Source::Ai);
    assert_eq!(events[1].meta.command.as_deref(), Some("ai.prefix"));
    assert_eq!(text_at(&core, &[0]), "AI: base human");
}

// ---------------------------------------------------------------------------
// #278 — markdown 嵌套深度门（审计 B-4：与 html.rs MAX_DEPTH=256 对齐）
// ---------------------------------------------------------------------------

/// 树的最大深度（root 的孩子 = 深度 1；递归 walk——测试专用，敌意深树
/// 已被门封顶，此处递归安全）。
fn tree_max_depth(tree: &mdcore::tree::BlockTree) -> usize {
    fn walk(tree: &mdcore::tree::BlockTree, id: mdcore::tree::NodeId, depth: usize) -> usize {
        let node = tree.node(id);
        node.children
            .iter()
            .map(|&c| walk(tree, c, depth + 1))
            .max()
            .unwrap_or(depth)
    }
    tree.top_level()
        .iter()
        .map(|&c| walk(tree, c, 1))
        .max()
        .unwrap_or(0)
}

#[test]
fn md_depth_guard_blocks_hostile_nesting() {
    use mdcore::markdown::{markdown_to_tree, tree_to_markdown};

    // ① 逐行递进 quote（300 层容器）：depth ≥256 的行被跳过，容器层
    //    封顶 255（+内容段落叶子 = 树深硬顶 256）。无门时 300 层照建
    //    ——万层级敌意即此形态放大。
    let hostile: String = (1..=300)
        .map(|n| format!("{} layer {n}\n", "> ".repeat(n)))
        .collect();
    let tree = markdown_to_tree(&hostile);
    assert!(
        tree_max_depth(&tree) <= 256,
        "敌意递进 quote 树深必须封顶 256：实际 {}",
        tree_max_depth(&tree)
    );

    // ② 单行超大 depth（一行 1 万个 >）：整行跳过（不建节点、不出内容
    //    ——与 html 侧超深子树跳过同语义）。
    let one_line = format!("{} x\n", ">".repeat(10_000));
    let tree = markdown_to_tree(&one_line);
    assert_eq!(tree_max_depth(&tree), 0, "单行超深跳过：零节点零内容");

    // ③ 缩进列表逐层递进（300 层）：同门（缩进层号即嵌套层编号）。
    let list_hostile: String = (1..=300)
        .map(|n| format!("{}- item {n}\n", "  ".repeat(n)))
        .collect();
    let tree = markdown_to_tree(&list_hostile);
    assert!(
        tree_max_depth(&tree) <= 256,
        "敌意递进列表树深必须封顶 256：实际 {}",
        tree_max_depth(&tree)
    );

    // ④ 合法浅嵌套（3 层 quote）零回归：3 层容器 + 1 内容叶子 = 深度 4。
    let legal = "> a\n> > b\n> > > c\n";
    let tree = markdown_to_tree(legal);
    assert_eq!(tree_max_depth(&tree), 4, "3 层 quote + 段落叶子");

    // ⑤ 递归消费者全链不爆栈：1 万行递进（无门时万层深树 →
    //    to_markdown 递归 walk 每层数百字节栈帧，远超测试线程默认
    //    2MB 栈 → 必爆；wasm = trap）。有门封顶后全链可跑。
    //    （规模取舍：10 万行构造串本身 ~10GB 会先 OOM——1 万层已
    //    充分越界默认栈。）
    let deep: String = (1..=10_000)
        .map(|n| format!("{} d{n}\n", "> ".repeat(n)))
        .collect();
    let tree = markdown_to_tree(&deep);
    assert!(tree_max_depth(&tree) <= 256);
    // to_markdown（递归消费者）对封顶树跑通即证不爆栈。
    let _ = tree_to_markdown(&tree);
}
