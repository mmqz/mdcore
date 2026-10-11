//! T1 质量审计轮修复锁（T1-fix-a：F2 / O4 / F10，survey+review 双确认）。
//!
//! 修复前缺陷：
//! - F2：`resolve_text_target` 的 ByAnchor 分支只校验 start ≤ len / end ≤ len，
//!   expect=Some 时 `Rope::slice(start..end)` 在 start > end 上直接 panic
//!   （ropey 1.6 文档明示；wasm replace_text_at_anchor 公开可达）。
//!   expect=None 虽经 op 层 BadRange 安全拒绝，但两路径防线不统一；
//! - O4：`Snapshot::from_json` 直接反序列化 + `build` 原样重建，恶意/损坏
//!   snapshot 可注入 duplicate BlockId / next_id 水位倒挂 / 越界 marks /
//!   非法 Document 子块，绕过 op 层不变量直写树内；
//! - F10：undo/redo 的 apply_raw_batch 失败路径静默丢一条历史（pop 后
//!   条目不回栈）。可达面：set_history_json / open_document 载入与当前
//!   树不一致的历史。
//!
//! 锁清单：
//! 1. ByAnchor 反转区间 × expect=Some / expect=None → 均入口 typed Reject
//!    （AnchorError::Inverted，树零污染）；start == end 空区间不受影响；
//! 2. 程序化构造的损坏 snapshot → `validate()` 逐变体 typed Err；
//!    JSON 面（wasm restore 同路）的损坏 snapshot → `from_json` 拒收
//!    （错误信息可诊断）；合法快照 round-trip 不受影响；
//! 3. 不一致历史触发的 undo/redo 失败 → 条目按原位回栈、合并窗边界拨回，
//!    History 整栈 JSON 与调用前逐字节一致，树 / revision 不变。

use std::collections::BTreeMap;

use mdcore::anchor::{AnchorError, TextTarget};
use mdcore::debug;
use mdcore::history::Kind;
use mdcore::snapshot::{SnapNode, Snapshot, SnapshotError};
use mdcore::tree::{BlockKind, MarkSpan};
use mdcore::{Core, Reject};

fn text_at(c: &Core, path: &[usize]) -> String {
    let slot = c.tree.resolve(path).expect("resolve path");
    c.tree.node(slot).text.to_string()
}

fn block_id(c: &Core, path: &[usize]) -> mdcore::id::BlockId {
    let slot = c.tree.resolve(path).expect("resolve path");
    c.tree.node(slot).id
}

// ---------------------------------------------------------------------------
// F2 — ByAnchor 反转区间（start > end）
// ---------------------------------------------------------------------------

#[test]
fn probe_byanchor_inverted_range_rejects_before_slice() {
    let mut c = Core::from_markdown("hello");
    let id = block_id(&c, &[0]);
    let before = debug::dump_tree(&c.tree);

    // expect=Some：修复前在本分支的 `slice(start..end)` 处 panic（ropey
    // 反转区间明示 panic，start=4/end/1 双双 ≤ len 均过旧校验）。
    let r = c.replace_text(
        TextTarget::ByAnchor {
            block: id,
            start: 4,
            end: 1,
            expect: Some("lo".into()),
        },
        "X".into(),
    );
    assert_eq!(
        r,
        Err(Reject::Anchor(AnchorError::Inverted {
            block: id,
            start: 4,
            end: 1
        })),
        "expect=Some 形态：入口 typed Reject（不 panic）"
    );

    // expect=None：修复前经 op 层 BadRange 安全拒绝；入口统一后同一 typed 错误
    let r2 = c.replace_text(
        TextTarget::ByAnchor {
            block: id,
            start: 4,
            end: 1,
            expect: None,
        },
        "X".into(),
    );
    assert_eq!(
        r2,
        Err(Reject::Anchor(AnchorError::Inverted {
            block: id,
            start: 4,
            end: 1
        })),
        "expect=None 形态：与 expect=Some 同一入口防线"
    );

    assert_eq!(debug::dump_tree(&c.tree), before, "拒绝后树零污染");

    // 边界对照：start == end（空区间 = 纯插入）不受影响，expect 空串匹配
    c.replace_text(
        TextTarget::ByAnchor {
            block: id,
            start: 2,
            end: 2,
            expect: Some("".into()),
        },
        "OK".into(),
    )
    .unwrap();
    assert_eq!(text_at(&c, &[0]), "heOKllo");
}

// ---------------------------------------------------------------------------
// O4 — snapshot 反序列化校验
// ---------------------------------------------------------------------------

fn snap_node(
    id: u64,
    kind: BlockKind,
    text: &str,
    marks: Vec<MarkSpan>,
    children: Vec<SnapNode>,
) -> SnapNode {
    SnapNode {
        id,
        kind,
        text: text.into(),
        attrs: BTreeMap::new(),
        marks,
        children,
    }
}

/// 程序化构造面（pub 字段宿主同款）：`validate()` 逐变体 typed Err。
#[test]
fn probe_snapshot_validate_typed_rejects() {
    let root = |next_id: u64, children: Vec<SnapNode>| Snapshot {
        revision: 7,
        next_id,
        root: snap_node(0, BlockKind::Document, "", Vec::new(), children),
    };

    // (a) duplicate：子块与虚根保留 ID 0 撞号
    let dup_root = snap_node(
        0,
        BlockKind::Paragraph,
        "x",
        Vec::new(),
        vec![snap_node(
            0,
            BlockKind::Paragraph,
            "y",
            Vec::new(),
            Vec::new(),
        )],
    );
    let s = Snapshot {
        revision: 0,
        next_id: 2,
        root: dup_root,
    };
    assert_eq!(
        s.validate(),
        Err(SnapshotError::DuplicateBlockId { id: 0 }),
        "ID 0 为虚根保留（BlockTree::new 注释语义），子块不得撞号"
    );

    // (a') duplicate：两个兄弟共用 ID 5
    let s = root(
        6,
        vec![
            snap_node(5, BlockKind::Paragraph, "a", Vec::new(), Vec::new()),
            snap_node(5, BlockKind::Paragraph, "b", Vec::new(), Vec::new()),
        ],
    );
    assert_eq!(s.validate(), Err(SnapshotError::DuplicateBlockId { id: 5 }));

    // (b) next_id 倒挂：max id 5，next_id 3（合法 capture 恒 watermark > 全部 ID）
    let s = root(
        3,
        vec![snap_node(
            5,
            BlockKind::Paragraph,
            "a",
            Vec::new(),
            Vec::new(),
        )],
    );
    assert_eq!(
        s.validate(),
        Err(SnapshotError::NextIdNotAbove {
            next_id: 3,
            max_id: 5
        })
    );

    // (c) marks 越界：end 超出块文本码点数（"hello" = 5）
    let s = root(
        2,
        vec![snap_node(
            1,
            BlockKind::Paragraph,
            "hello",
            vec![MarkSpan {
                start: 0,
                end: 99,
                kind: "bold".into(),
            }],
            Vec::new(),
        )],
    );
    assert_eq!(
        s.validate(),
        Err(SnapshotError::MarkOutOfRange {
            id: 1,
            start: 0,
            end: 99,
            len: 5
        })
    );

    // (c') marks 反转：start > end（op 层 SetMark 同款拒绝）
    let s = root(
        2,
        vec![snap_node(
            1,
            BlockKind::Paragraph,
            "hello",
            vec![MarkSpan {
                start: 4,
                end: 1,
                kind: "bold".into(),
            }],
            Vec::new(),
        )],
    );
    assert_eq!(
        s.validate(),
        Err(SnapshotError::MarkOutOfRange {
            id: 1,
            start: 4,
            end: 1,
            len: 5
        })
    );

    // (d) 非根节点携带 Document 类型（apply_op 的 InsertBlock/SetBlockType
    // 均拒绝；restore 面不得绕过）
    let s = root(
        2,
        vec![snap_node(
            1,
            BlockKind::Document,
            "",
            Vec::new(),
            Vec::new(),
        )],
    );
    assert_eq!(s.validate(), Err(SnapshotError::ReservedKind { id: 1 }));

    // 合法形态：嵌套 + mark + next_id 水位留隙（删除不回收的常态）→ Ok
    let s = root(
        9,
        vec![
            snap_node(
                1,
                BlockKind::Paragraph,
                "hello",
                vec![MarkSpan {
                    start: 0,
                    end: 5,
                    kind: "bold".into(),
                }],
                Vec::new(),
            ),
            snap_node(
                2,
                BlockKind::Blockquote,
                "",
                Vec::new(),
                vec![snap_node(
                    3,
                    BlockKind::Paragraph,
                    "quote body",
                    Vec::new(),
                    Vec::new(),
                )],
            ),
        ],
    );
    assert_eq!(s.validate(), Ok(()));
}

/// JSON 面（wasm `restore(snapshot_json)` 同路）：损坏 snapshot 在
/// `from_json` 反序列化边界拒收（错误信息可诊断），合法 round-trip 不受影响。
#[test]
fn probe_corrupt_snapshot_json_rejected_at_from_json() {
    // 基样：P + 嵌套 quote + mark，覆盖全部校验项的上下文
    let mut c = Core::from_markdown("hello\n> quote body");
    c.apply_ops(
        vec![mdcore::op::Op::SetMark {
            path: vec![0],
            start: 0,
            end: 5,
            kind: "bold".into(),
            add: true,
        }],
        mdcore::ApplyMeta::view("mark"),
    )
    .unwrap();
    let good = c.snapshot().to_json();
    assert_eq!(c.revision(), 1);
    assert_eq!(
        block_id(&c, &[1]).0,
        2,
        "基样 ID 布局自证（防 fixture 漂移）"
    );

    let mutate = |f: fn(&mut serde_json::Value)| -> String {
        let mut v: serde_json::Value = serde_json::from_str(&good).unwrap();
        f(&mut v);
        v.to_string()
    };

    // 控制组：未损坏 JSON 可解析（合法 round-trip 不受影响）
    let back = Snapshot::from_json(&good).expect("合法快照照常放行");
    let restored = Core::restore(back);
    assert_eq!(debug::dump_tree(&restored.tree), debug::dump_tree(&c.tree));
    assert_eq!(restored.revision(), 1);

    // (a) 重复 ID：quote 容器撞第一段的号
    let bad = mutate(|v| v["root"]["children"][1]["id"] = v["root"]["children"][0]["id"].clone());
    let err = Snapshot::from_json(&bad).expect_err("duplicate 必须拒收");
    assert!(
        err.to_string().contains("duplicate block id 1"),
        "错误面可诊断: {err}"
    );

    // (b) next_id 倒挂（树中最大数值 ID = 3）
    let bad = mutate(|v| v["next_id"] = serde_json::json!(1));
    let err = Snapshot::from_json(&bad).expect_err("倒挂水位必须拒收");
    assert!(
        err.to_string().contains("not above max block id 3"),
        "错误面可诊断: {err}"
    );

    // (c) marks 越界（"hello" = 5 码点）
    let bad = mutate(|v| v["root"]["children"][0]["marks"][0]["end"] = serde_json::json!(99));
    let err = Snapshot::from_json(&bad).expect_err("越界 mark 必须拒收");
    assert!(
        err.to_string().contains("out of bounds (len 5)"),
        "错误面可诊断: {err}"
    );

    // (d) 非根 Document 子块
    let bad = mutate(|v| v["root"]["children"][1]["kind"] = serde_json::json!("document"));
    let err = Snapshot::from_json(&bad).expect_err("Document 子块必须拒收");
    assert!(
        err.to_string().contains("reserved root kind"),
        "错误面可诊断: {err}"
    );
}

// ---------------------------------------------------------------------------
// F10 — undo/redo 失败路径回滚（不丢历史）
// ---------------------------------------------------------------------------

/// 不一致历史的坏 op：ReplaceText 指向不存在路径（apply → BadPath）。
fn bad_path_op() -> serde_json::Value {
    serde_json::json!({ "t": "replace_text", "path": [9], "start": 0, "end": 1, "insert": "x" })
}

#[test]
fn probe_undo_failure_keeps_entry_stack_and_tree() {
    let mut c = Core::from_markdown_with_clock("hello", std::sync::Arc::new(|| 0));
    c.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 0,
            end: 5,
        },
        "HELLO".into(),
    )
    .unwrap();
    assert_eq!(c.history.undo_depth(), 1);
    assert_eq!(c.history.last_kind(), Some(Kind::Text));

    // crafted 不一致历史：唯一条目的 inverses 指向不存在路径 → undo 应用必败
    let mut v: serde_json::Value = serde_json::from_str(&c.history_json()).unwrap();
    v["undo"][0]["inverses"] = serde_json::json!([bad_path_op()]);
    c.set_history_json(&v.to_string()).unwrap();
    let history_before = c.history.to_json();
    let tree_before = debug::dump_tree(&c.tree);
    let (rev, depth) = (c.revision(), c.history.undo_depth());

    assert!(c.undo().is_none(), "不一致历史：undo 应用失败必须返回 None");
    assert_eq!(
        c.history.undo_depth(),
        depth,
        "条目按原位压回 undo 栈（不丢历史）"
    );
    assert_eq!(
        debug::dump_tree(&c.tree),
        tree_before,
        "树由 apply_raw_batch backup 恢复"
    );
    assert_eq!(c.revision(), rev, "失败不推进 revision");
    assert_eq!(
        c.history.last_kind(),
        Some(Kind::Text),
        "合并窗边界拨回（pop 会置 None）"
    );
    assert_eq!(
        c.history.to_json(),
        history_before,
        "History 与调用前逐字节一致"
    );
}

#[test]
fn probe_redo_failure_keeps_entry_stack_and_tree() {
    let mut c = Core::from_markdown_with_clock("hello", std::sync::Arc::new(|| 0));
    c.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 0,
            end: 5,
        },
        "HELLO".into(),
    )
    .unwrap();
    c.undo().unwrap(); // 合法 undo：条目落 redo 栈

    // crafted 不一致历史：redo 条目的 ops 指向不存在路径 → redo 应用必败
    let mut v: serde_json::Value = serde_json::from_str(&c.history_json()).unwrap();
    v["redo"][0]["ops"] = serde_json::json!([bad_path_op()]);
    c.set_history_json(&v.to_string()).unwrap();
    let history_before = c.history.to_json();
    let tree_before = debug::dump_tree(&c.tree);
    let (rev, depth) = (c.revision(), c.history.redo_depth());

    assert!(c.redo().is_none(), "不一致历史：redo 应用失败必须返回 None");
    assert_eq!(
        c.history.redo_depth(),
        depth,
        "条目按原位压回 redo 栈（不丢历史）"
    );
    assert_eq!(
        debug::dump_tree(&c.tree),
        tree_before,
        "树由 apply_raw_batch backup 恢复"
    );
    assert_eq!(c.revision(), rev, "失败不推进 revision");
    assert_eq!(
        c.history.to_json(),
        history_before,
        "History 与调用前逐字节一致"
    );
}
