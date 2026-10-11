//! R24 质量审计轮修复锁（R24-a2：SetMark 求逆不对称 + 区间校验）。
//!
//! 修复前缺陷：`invert_op` 对 SetMark 无条件翻转 `add`——独立条目中的
//! 冗余 add（apply no-op）会让 undo 删掉既有 mark；移除不存在的 span
//! （apply no-op）会让 undo 凭空造出 mark。两者均违反「undo 精确恢复
//! 前态」不变量。修复后逆的方向由 state-before 的 span 存在性决定
//! （no-op → 同向幂等；有效变更 → 反向）。
//!
//! 锁清单（时钟控制分条，避免合并窗吸收）：
//! 1. 独立条目中的冗余 add（no-op）→ 该条目 undo 必须 no-op；
//! 2. 移除不存在的 span → 独立条目 no-op → undo 必须 no-op；
//! 3. 合并窗内连击 add+add → 单条目 undo 撤销整组（mark 移除，正确对照）；
//! 4. SetMark 越界区间 Reject（start > end / end > len，与 ReplaceText
//!    同款校验），Reject 后树零污染。

use std::sync::Arc;

use mdcore::op::Op;
use mdcore::{ApplyMeta, Core};

fn set_mark(add: bool) -> Op {
    Op::SetMark {
        path: vec![0],
        start: 0,
        end: 4,
        kind: "bold".into(),
        add,
    }
}

fn marks_of(c: &Core) -> usize {
    c.tree.node(c.tree.resolve(&[0]).unwrap()).marks.len()
}

#[test]
fn probe_redundant_add_mark_undo_must_be_noop() {
    // 固定时钟分段：t=0 第一次 add（有效）；t=5000 第二次 add（超出 1s
    // 合并窗 → 独立条目，且是对已存在 span 的 no-op）。
    let mut c = Core::from_markdown_with_clock("hello world", Arc::new(|| 0));
    c.apply_ops(vec![set_mark(true)], ApplyMeta::view("mark"))
        .unwrap();
    assert_eq!(marks_of(&c), 1, "第一次 add 生效");
    assert_eq!(c.history.undo_depth(), 1);

    c.set_clock(Arc::new(|| 5_000));
    c.apply_ops(vec![set_mark(true)], ApplyMeta::view("mark"))
        .unwrap();
    assert_eq!(marks_of(&c), 1, "重复 add 是 no-op");
    assert_eq!(c.history.undo_depth(), 2, "出窗 → 独立条目");

    // undo 独立的 no-op 条目：状态必须不变（修复前：mark 被删）
    c.undo().unwrap();
    assert_eq!(marks_of(&c), 1, "no-op 条目的 undo 必须 no-op（R24-a2）");

    // 再 undo 第一条：mark 移除（有效变更的正常回退）
    c.undo().unwrap();
    assert_eq!(marks_of(&c), 0);
}

#[test]
fn probe_remove_absent_mark_undo_must_be_noop() {
    // 移除从不存在的 span：独立条目 no-op → undo 必须 no-op
    let mut c = Core::from_markdown_with_clock("hello world", Arc::new(|| 0));
    c.apply_ops(vec![set_mark(false)], ApplyMeta::view("unmark"))
        .unwrap();
    assert_eq!(marks_of(&c), 0);
    assert_eq!(c.history.undo_depth(), 1);

    c.undo().unwrap();
    assert_eq!(
        marks_of(&c),
        0,
        "no-op 的 undo 必须 no-op（修复前：凭空造 mark）"
    );
}

#[test]
fn probe_coalesced_adds_undo_reverts_whole_group() {
    // 同窗连击（合并窗语义）：add + 冗余 add 并成一条 → 单次 undo 撤销
    // 整组（回到组前状态 = 无 mark）。逆序应用：inv(add#2)=no-op，
    // inv(add#1)=remove → remove。修复前后此路径均正确，作为对照锁。
    let mut c = Core::from_markdown_with_clock("hello world", Arc::new(|| 1_000));
    c.apply_ops(vec![set_mark(true)], ApplyMeta::view("mark"))
        .unwrap();
    c.set_clock(Arc::new(|| 1_500)); // 同窗（<1000ms）
    c.apply_ops(vec![set_mark(true)], ApplyMeta::view("mark"))
        .unwrap();
    assert_eq!(marks_of(&c), 1);
    assert_eq!(c.history.undo_depth(), 1, "同窗并条");

    c.undo().unwrap();
    assert_eq!(marks_of(&c), 0, "整组回退到组前状态");
}

#[test]
fn probe_setmark_range_validation() {
    // R24-a2 加固：SetMark 越界区间必须 Reject（与 ReplaceText 同款校验）
    let mut c = Core::from_markdown("hello");
    let bad = Op::SetMark {
        path: vec![0],
        start: 5,
        end: 2,
        kind: "bold".into(),
        add: true,
    };
    assert!(
        c.apply_ops(vec![bad], ApplyMeta::view("mark")).is_err(),
        "start > end 必须 Reject"
    );
    let bad2 = Op::SetMark {
        path: vec![0],
        start: 0,
        end: 99,
        kind: "bold".into(),
        add: true,
    };
    assert!(
        c.apply_ops(vec![bad2], ApplyMeta::view("mark")).is_err(),
        "end > len 必须 Reject"
    );
    assert_eq!(marks_of(&c), 0, "Reject 后树零污染（事务回滚）");
}
