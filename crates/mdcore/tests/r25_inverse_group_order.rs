//! r25 — issue #220 / 审计 P1-27：`apply_ops` 逆序构造的组语义（复现 + 锁定）。
//!
//! 背景：`invert_op` 返回的是**逆组**（多数长度 1；`RemoveBlock` 的逆 =
//! 先序重建整棵子树，组内顺序 = 重建顺序：父先子后，`build_rebuild_ops`）。
//! undo 按数组序应用 `entry.inverses`，其正确语义是
//! **组间逆序、组内保序**（后事务的逆组在前；组内保持重建发射序）。
//!
//! 现状缺陷（本文件复现）：`apply_ops` 把各组摊平后整体 `reverse()`
//! （lib.rs 旧 :312）——单 op 逆组（长度 1）摊平反转 == 组反转，故既有
//! 测试全绿；但**非叶块删除**（`delete_block` 对非叶无限制，子树删除公开
//! 可达）与**含非叶 RemoveBlock 的多 op 事务**会产生长度 ≥2 的逆组，
//! 组内序被倒转。undo 时先应用「子先于父」的 InsertBlock：其 parent
//! 路径在移位后的树里要么解析失败（BadPath → `apply_raw_batch` 失败 →
//! T1-F10 条目压回 → **undo 永久卡死**），要么解析到顶替被删块位置的
//! **错误节点**（InsertBlock 是路径寻址，静默错挂，undo 返回 Some 但树
//! 损坏）。对应 ADR-B5（研究仓侧）「逆组组间逆序组内保序」裁决项。
//!
//! 断言口径：全部「精确恢复前态」——结构（路径/类型）+ 文本 + 身份 ID
//! （先序 dump 逐行相等）+ to_markdown 逐字节。
//!
//! 边界登记：
//! - marks 不在断言面——RemoveBlock 逆不恢复 marks 是**另一缺陷**（审计
//!   03 §1，P1 级），与本文件的序缺陷正交，另行治理；在此断言 marks 会
//!   把两个缺陷耦死。
//! - 旧版本保存的历史 JSON 中逆组存储序已是错的，本修复不追溯（载入
//!   后按其存储原样应用）；只有新产生的条目按组语义存储。
//!
//! 控制组（既有语义锁定，修复前后都必须通过）：
//! - `leaf_delete_undo_still_exact`：单叶删除逆组长度 1，不受影响——
//!   这正是既有测试全绿、缺陷长期漏网的原因。
//! - `coalesced_text_entry_undo_order_unchanged`：合并条目只含文本类 op
//!   （逆组全长度 1），`coalesce_into_last` 前插语义在组序下不变。

use std::sync::Arc;

use mdcore::anchor::TextTarget;
use mdcore::op::Op;
use mdcore::tree::{BlockKind, NodePath};
use mdcore::Core;

// ---------------------------------------------------------------------------
// 公共工具
// ---------------------------------------------------------------------------

/// 先序 dump：每块一行（路径, 类型, 文本, ID）。身份 ID 进 dump —— undo
/// 复原必须携带原 ID（§8.2：物化 `InsertBlock.id`，身份不改写）。
fn dump(core: &Core) -> Vec<(NodePath, BlockKind, String, u64)> {
    fn walk(core: &Core, path: &mut Vec<usize>, out: &mut Vec<(NodePath, BlockKind, String, u64)>) {
        let id = core.tree.resolve(path).expect("valid path");
        let n = core.tree.node(id);
        out.push((path.clone(), n.kind, n.text.to_string(), n.id.0));
        let n_children = n.children.len();
        for i in 0..n_children {
            path.push(i);
            walk(core, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    for i in 0..core.tree.top_level().len() {
        let mut path = vec![i];
        walk(core, &mut path, &mut out);
    }
    out
}

fn md_of(core: &Core) -> String {
    mdcore::markdown::tree_to_markdown(&core.tree)
}

/// 步进时钟（每次 +10s）：文本命令各自成条目，不受合并窗干扰。
fn step_clock() -> Arc<dyn Fn() -> u64 + Send + Sync> {
    let tick = std::sync::atomic::AtomicU64::new(1000);
    Arc::new(move || tick.fetch_add(10_000, std::sync::atomic::Ordering::SeqCst) + 10_000)
}

/// 自检断言：钉住 markdown 解析出的初始形状（解析器漂移时在此处响亮失败，
/// 而不是让 undo 断言吸收掉）。
fn assert_shape_three_roots(core: &Core) {
    let d = dump(core);
    assert_eq!(d.len(), 5, "F3 形状：A + P(C1,C2) + B = 5 块");
    assert_eq!(d[0], (vec![0], BlockKind::Paragraph, "A".into(), d[0].3));
    assert_eq!(d[1].0, vec![1], "P 在 [1]");
    assert_eq!(d[1].1, BlockKind::ListItem, "P 是列表项（非叶容器）");
    assert_eq!(
        (d[2].0.as_slice(), d[2].2.as_str()),
        (&[1usize, 0][..], "C1"),
        "C1 是 P 首子"
    );
    assert_eq!(
        (d[3].0.as_slice(), d[3].2.as_str()),
        (&[1usize, 1][..], "C2"),
        "C2 是 P 次子"
    );
    assert_eq!(d[4].0, vec![2], "B 在 [2]（P 删除后移位到 [1]）");
}

// ---------------------------------------------------------------------------
// 1. 单 op 子树删除 → undo 精确复原（Case A：root ≥3 子块）
// ---------------------------------------------------------------------------

/// F3 = "A\n- P\n  - C1\n  - C2\nB"：删除 [1]（列表项 P，带两子）。
/// 现状预期：undo 返回 None（首个逆 op `ins(C2)@parent=[1],index=1`
/// 解析到移位后的 B（无子），BadIndex → apply 失败 → 条目压回 → 卡死）。
#[test]
fn nonleaf_delete_undo_exact_restore_three_roots() {
    let mut core = Core::from_markdown("A\n- P\n  - C1\n  - C2\nB");
    assert_shape_three_roots(&core);
    let pre = dump(&core);
    let pre_md = md_of(&core);
    assert_eq!(pre_md, "A\n- P\n  - C1\n  - C2\nB");

    let base = core.revision();
    core.delete_block(base, vec![1]).unwrap();
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A", "B"],
        "删除后只剩 A、B（子树整体消失）"
    );

    let receipt = core.undo();
    assert!(
        receipt.is_some(),
        "undo 必须成功并返回 Receipt（现状：嵌套子树删除 undo 硬失败 None）"
    );
    assert_eq!(
        dump(&core),
        pre,
        "undo 后先序 dump 逐行复原（结构+文本+ID）"
    );
    assert_eq!(md_of(&core), pre_md, "undo 后 to_markdown 逐字节复原");
    assert!(core.history.redo_depth() >= 1, "undo 后条目进 redo 栈");

    // redo 重放 entry.ops：必须精确回到删除后状态
    assert!(core.redo().is_some(), "redo 必须成功");
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A", "B"],
        "redo 重放后回到删除后状态"
    );
    assert_eq!(core.history.undo_depth(), 1, "redo 后条目回 undo 栈");
}

/// 静默错挂变体（同 Case A，但 P 仅一子 + 顶位者 B 无子）：
/// 现状下首个逆 op `ins(C)@parent=[1],index=0` 恰好合法（index 0 ≤ 0），
/// C 被错挂进 B——undo 返回 Some 但树损坏。这是最危险的形态：
/// 无任何报错，损坏随后续编辑/redo 继续放大。
#[test]
fn nonleaf_delete_undo_silent_misgraft_container_successor() {
    let mut core = Core::from_markdown("A\n- P\n  - C\nB");
    let d = dump(&core);
    assert_eq!(d.len(), 4, "形状自检：A + P(C) + B");
    assert_eq!(d[1].0, vec![1], "P 在 [1]");
    assert_eq!(d[2].0, vec![1, 0], "C 是 P 独子");
    assert_eq!(d[3].0, vec![2], "B 在 [2]");
    let pre = dump(&core);

    let base = core.revision();
    core.delete_block(base, vec![1]).unwrap();

    let receipt = core.undo();
    assert!(
        receipt.is_some(),
        "undo 必须成功（现状：错挂形态下返回 Some 但树损坏，更糟）"
    );
    assert_eq!(
        dump(&core),
        pre,
        "undo 后精确复原（现状：C 错挂到顶位者 B 名下、P 空壳）"
    );

    assert!(core.redo().is_some(), "redo 必须成功");
    assert_eq!(
        dump(&core).len(),
        2,
        "redo 后回到删除后状态（现状：redo 会把错挂残留一并保留）"
    );
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A", "B"],
        "redo 重放 entry.ops 精确回到删除后状态"
    );
}

// ---------------------------------------------------------------------------
// 2. 同上但 root 恰 2 子块（[0]=A, [1]=P）——路径解析失败变体
// ---------------------------------------------------------------------------

/// F2 = "A\n- P\n  - C1\n  - C2"：删除 [1] 后 root 只剩 [A]，
/// 首个逆 op 的 parent 路径 [1] 无从解析（BadPath）。
/// 现状预期：undo 硬失败 None，且重复调用同样失败（整栈卡死在该条目）。
#[test]
fn nonleaf_delete_undo_exact_restore_two_roots() {
    let mut core = Core::from_markdown("A\n- P\n  - C1\n  - C2");
    let d = dump(&core);
    assert_eq!(d.len(), 4, "形状自检：A + P(C1,C2)");
    assert_eq!(d[1].0, vec![1], "P 在 [1]");
    let pre = dump(&core);

    let base = core.revision();
    core.delete_block(base, vec![1]).unwrap();
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A"],
        "删除后只剩 A"
    );

    let receipt = core.undo();
    assert!(
        receipt.is_some(),
        "undo 必须成功（现状：parent 路径解析失败 → 硬失败 None → 整栈卡死）"
    );
    assert_eq!(dump(&core), pre, "undo 后精确复原");
    assert!(core.redo().is_some(), "redo 必须成功");
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A"],
        "redo 回到删除后状态"
    );
}

// ---------------------------------------------------------------------------
// 3. 多 op 事务混组（[ReplaceText, RemoveBlock(子树)] 与反序）
// ---------------------------------------------------------------------------

/// 事务序一：文本编辑在前、子树删除在后。
/// 正确存储序 = [g(删除逆组：ins(P),ins(C1),ins(C2)), inv_text]。
#[test]
fn transaction_text_then_subtree_delete_undo_exact_restore() {
    let mut core = Core::from_markdown_with_clock("A\n- P\n  - C1\n  - C2\nB", step_clock());
    assert_shape_three_roots(&core);
    let pre = dump(&core);
    let pre_md = md_of(&core);

    let base = core.revision();
    let ops = vec![
        Op::ReplaceText {
            path: vec![0],
            start: 1,
            end: 1,
            insert: " v2".into(),
        },
        Op::RemoveBlock { path: vec![1] },
    ];
    core.commit_transaction(base, ops).unwrap();
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A v2", "B"],
        "事务后：A 改写 + 子树删除"
    );

    assert!(
        core.undo().is_some(),
        "undo 必须成功（现状：逆组内序被倒转 → 首个逆 op 命中移位后的 B）"
    );
    assert_eq!(dump(&core), pre, "undo 后整事务精确复原");
    assert_eq!(md_of(&core), pre_md, "undo 后 to_markdown 逐字节复原");
    assert!(core.redo().is_some(), "redo 必须成功");
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A v2", "B"],
        "redo 回到事务后状态"
    );
}

/// 事务序二（反序）：子树删除在前、文本编辑（落在移位后的路径）在后。
/// 正确存储序 = [inv_text, g(删除逆组：ins(P),ins(C1),ins(C2))]。
#[test]
fn transaction_subtree_delete_then_text_undo_exact_restore() {
    let mut core = Core::from_markdown_with_clock("A\n- P\n  - C1\n  - C2\nB", step_clock());
    assert_shape_three_roots(&core);
    let pre = dump(&core);

    let base = core.revision();
    // 删除 P 后 B 移位到 [1]；文本 op 按顺序状态语义寻址移位后的 B。
    let ops = vec![
        Op::RemoveBlock { path: vec![1] },
        Op::ReplaceText {
            path: vec![1],
            start: 1,
            end: 1,
            insert: "!".into(),
        },
    ];
    core.commit_transaction(base, ops).unwrap();
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A", "B!"],
        "事务后：子树删除 + B 追加感叹号"
    );

    assert!(
        core.undo().is_some(),
        "undo 必须成功（现状：逆组内序被倒转 → ins(C2) 先于 ins(P) 应用）"
    );
    assert_eq!(dump(&core), pre, "undo 后整事务精确复原");
    assert!(core.redo().is_some(), "redo 必须成功");
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A", "B!"],
        "redo 回到事务后状态"
    );
}

// ---------------------------------------------------------------------------
// 4. 两个子树删除同事务
// ---------------------------------------------------------------------------

/// F4：root = [A, P1(X,Y), P2(U,V), B]；事务 = 连删 P1、P2
/// （第二条路径按移位后语义指向 P2）。
#[test]
fn transaction_two_subtree_deletes_undo_exact_restore() {
    let mut core = Core::from_markdown("A\n- P1\n  - X\n  - Y\n- P2\n  - U\n  - V\nB");
    let d = dump(&core);
    assert_eq!(d.len(), 8, "形状自检：A + P1(X,Y) + P2(U,V) + B = 8 块");
    assert_eq!(d[1].0, vec![1], "P1 在 [1]");
    assert_eq!(d[1].1, BlockKind::ListItem, "P1 非叶");
    assert_eq!(d[4].0, vec![2], "P2 在 [2]");
    assert_eq!(d[7].0, vec![3], "B 在 [3]");
    let pre = dump(&core);
    let pre_md = md_of(&core);

    let base = core.revision();
    let ops = vec![
        Op::RemoveBlock { path: vec![1] }, // 删 P1 → P2 移位到 [1]
        Op::RemoveBlock { path: vec![1] }, // 按移位后语义删 P2
    ];
    core.commit_transaction(base, ops).unwrap();
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A", "B"],
        "事务后两子树全删"
    );

    assert!(
        core.undo().is_some(),
        "undo 必须成功（现状：两组重建序均被倒转）"
    );
    assert_eq!(dump(&core), pre, "undo 后两子树连同身份精确复原");
    assert_eq!(md_of(&core), pre_md, "undo 后 to_markdown 逐字节复原");
    assert!(core.redo().is_some(), "redo 必须成功");
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A", "B"],
        "redo 回到事务后状态"
    );
}

// ---------------------------------------------------------------------------
// 5. 链式：编辑 → 子树删除 → 编辑 → undo×3 → redo×3
// ---------------------------------------------------------------------------

#[test]
fn chain_edit_delete_edit_undo3_redo3_exact_restore() {
    let mut core = Core::from_markdown_with_clock("A\n- P\n  - C1\n  - C2\nB", step_clock());
    assert_shape_three_roots(&core);
    let pre = dump(&core);
    let pre_md = md_of(&core);

    // 编辑 1（文本，独立条目）
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 1,
            end: 1,
        },
        " v2".into(),
    )
    .unwrap();
    // 子树删除（结构，独立条目）
    let base = core.revision();
    core.delete_block(base, vec![1]).unwrap();
    // 编辑 2：B 已移位到 [1]
    core.replace_text(
        TextTarget::ByPath {
            path: vec![1],
            start: 1,
            end: 1,
        },
        "!".into(),
    )
    .unwrap();
    let final_dump = dump(&core);
    let final_md = md_of(&core);
    assert_eq!(
        final_dump.iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["A v2", "B!"],
        "链末状态"
    );

    // undo×3：最近条目（编辑 2）→ 删除 → 编辑 1
    for i in 1..=3 {
        assert!(
            core.undo().is_some(),
            "第 {i} 次 undo 必须成功（现状：第 2 次卡死在删除条目）"
        );
    }
    assert_eq!(dump(&core), pre, "三次 undo 后回到初始精确前态");
    assert_eq!(md_of(&core), pre_md, "三次 undo 后 to_markdown 逐字节复原");

    // redo×3
    for i in 1..=3 {
        assert!(
            core.redo().is_some(),
            "第 {i} 次 redo 必须成功（现状：序损坏随 redo 放大）"
        );
    }
    assert_eq!(dump(&core), final_dump, "三次 redo 后回到链末精确状态");
    assert_eq!(
        md_of(&core),
        final_md,
        "三次 redo 后 to_markdown 逐字节复原"
    );
}

// ---------------------------------------------------------------------------
// 6. 存储序直接锁定（serde 面：Entry.inverses 仍为扁平 Vec<Op>，
//    只改序——历史 JSON 格式不破坏）
// ---------------------------------------------------------------------------

/// 删除 F3 的 [1] 后，undo 栈顶条目的 inverses 必须是
/// [ins(P)@[].1, ins(C1)@[1].0, ins(C2)@[1].1]（组间逆序 = 单组原样；
/// 组内保序 = 先序重建序）。现状：[ins(C2), ins(C1), ins(P)]（组内倒转）。
///
/// 经公开 history JSON 面读取（`History.undo` 字段私有）：同时把
/// 「历史 JSON 格式不变、只改序」钉进 serde 面。
fn last_undo_inverses(core: &Core) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(&core.history_json()).unwrap();
    let undo = v["undo"].as_array().expect("undo array");
    let entry = undo.last().expect("undo 栈顶条目");
    entry["inverses"].clone()
}

fn ins_field(op: &serde_json::Value, field: &str) -> serde_json::Value {
    op[field].clone()
}

#[test]
fn stored_inverse_order_is_group_aware() {
    let mut core = Core::from_markdown("A\n- P\n  - C1\n  - C2\nB");
    assert_shape_three_roots(&core);
    let pre = dump(&core);
    let p_id = pre[1].3;
    let c1_id = pre[2].3;
    let c2_id = pre[3].3;

    let base = core.revision();
    core.delete_block(base, vec![1]).unwrap();

    let inv = last_undo_inverses(&core);
    let inv = inv.as_array().expect("inverses array");
    assert_eq!(inv.len(), 3, "逆组摊平后仍为 3 个 op");

    // 序：ins(P) → ins(C1) → ins(C2)（父先子后，路径/index/ID 全对位）
    assert_eq!(
        (
            ins_field(&inv[0], "parent"),
            ins_field(&inv[0], "index"),
            ins_field(&inv[0], "id"),
            ins_field(&inv[0], "text")
        ),
        (
            serde_json::json!([]),
            serde_json::json!(1),
            serde_json::json!(p_id),
            serde_json::json!("P")
        ),
        "逆组首 = ins(P)@root.1（携带原 ID）"
    );
    assert_eq!(
        (
            ins_field(&inv[1], "parent"),
            ins_field(&inv[1], "index"),
            ins_field(&inv[1], "id"),
            ins_field(&inv[1], "text")
        ),
        (
            serde_json::json!([1]),
            serde_json::json!(0),
            serde_json::json!(c1_id),
            serde_json::json!("C1")
        ),
        "其次 ins(C1)@[1].0"
    );
    assert_eq!(
        (
            ins_field(&inv[2], "parent"),
            ins_field(&inv[2], "index"),
            ins_field(&inv[2], "id"),
            ins_field(&inv[2], "text")
        ),
        (
            serde_json::json!([1]),
            serde_json::json!(1),
            serde_json::json!(c2_id),
            serde_json::json!("C2")
        ),
        "逆组末 = ins(C2)@[1].1"
    );
}

/// 多 op 事务的存储序：组间逆序 + 组内保序的完整形态。
/// 事务 = [ReplaceText(A), RemoveBlock([1])]（F3 基座）：
/// inverses = [ins(P), ins(C1), ins(C2), inv_text] —— 删除逆组（后事务）
/// 在前、文本逆（前事务）殿后；组内保持先序重建序。
#[test]
fn stored_inverse_order_mixed_transaction_group_aware() {
    let mut core = Core::from_markdown("A\n- P\n  - C1\n  - C2\nB");
    assert_shape_three_roots(&core);

    let base = core.revision();
    let ops = vec![
        Op::ReplaceText {
            path: vec![0],
            start: 1,
            end: 1,
            insert: " v2".into(),
        },
        Op::RemoveBlock { path: vec![1] },
    ];
    core.commit_transaction(base, ops).unwrap();

    let inv = last_undo_inverses(&core);
    let inv = inv.as_array().expect("inverses array");
    assert_eq!(inv.len(), 4);

    assert_eq!(
        (ins_field(&inv[0], "parent"), ins_field(&inv[0], "index")),
        (serde_json::json!([]), serde_json::json!(1)),
        "逆组首 = ins(P)@root.1"
    );
    assert_eq!(
        (ins_field(&inv[1], "parent"), ins_field(&inv[1], "index")),
        (serde_json::json!([1]), serde_json::json!(0)),
        "其次 ins(C1)@[1].0"
    );
    assert_eq!(
        (ins_field(&inv[2], "parent"), ins_field(&inv[2], "index")),
        (serde_json::json!([1]), serde_json::json!(1)),
        "再次 ins(C2)@[1].1"
    );
    assert_eq!(
        (
            inv[3]["t"].clone(),
            inv[3]["path"].clone(),
            inv[3]["start"].clone(),
            inv[3]["end"].clone(),
            inv[3]["insert"].clone()
        ),
        (
            serde_json::json!("replace_text"),
            serde_json::json!([0]),
            serde_json::json!(1),
            serde_json::json!(4),
            serde_json::json!("")
        ),
        "文本逆殿后（组间逆序：后事务的删除逆组在前）；\
         逆的 insert 携带被删文本——纯插入的逆 = 删 [1,4)（撤销 ' v2'）"
    );
}

// ---------------------------------------------------------------------------
// 控制组：既有语义锁定（修复前后都必须通过）
// ---------------------------------------------------------------------------

/// 单叶删除（逆组长度 1）：扁平反转 == 组反转，语义本就正确。
/// 这是既有测试全绿、缺陷长期漏网的原因，锁定之防止未来回归。
#[test]
fn leaf_delete_undo_still_exact() {
    let mut core = Core::from_markdown("A\n- P\n  - C1\n  - C2\nB");
    assert_shape_three_roots(&core);
    let pre = dump(&core);

    let base = core.revision();
    core.delete_block(base, vec![0]).unwrap(); // A 是叶子
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["P", "C1", "C2", "B"]
    );

    assert!(core.undo().is_some());
    assert_eq!(dump(&core), pre, "叶删除 undo 一直正确（长度 1 逆组）");
}

/// 合并条目只含文本类 op（`should_coalesce` 要求 kind==Text ⇒ 全部
/// is_text_class ⇒ 逆组全长度 1）：`coalesce_into_last` 的「前插」在组
/// 语义下 = 最新组在前，行为与扁平时代逐位一致。锁定不变。
#[test]
fn coalesced_text_entry_undo_order_unchanged() {
    // 固定时钟：两笔文本事务必然落进 1s 合并窗
    let mut core = Core::from_markdown_with_clock("hello", Arc::new(|| 0));
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 5,
            end: 5,
        },
        " world".into(),
    )
    .unwrap();
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0],
            start: 11,
            end: 11,
        },
        "!".into(),
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), 1, "两笔并入同一条目");

    let inv = last_undo_inverses(&core);
    let inv = inv.as_array().expect("inverses array");
    assert_eq!(inv.len(), 2, "合并条目逆 = 两个文本逆");
    // 前插语义：最新事务的逆在前（undo 序 = 新组在前）。
    // 注意逆的 insert 字段携带的是**被删除文本**：纯插入的逆 = 删除
    // （"hello world"→"hello world!" 的逆 = 删 [11,12)、insert=""）。
    assert_eq!(
        (
            inv[0]["start"].clone(),
            inv[0]["end"].clone(),
            inv[0]["insert"].clone()
        ),
        (
            serde_json::json!(11),
            serde_json::json!(12),
            serde_json::json!("")
        ),
        "最新逆在前：删 [11,12)（撤销 '!' 插入）"
    );
    assert_eq!(
        (
            inv[1]["start"].clone(),
            inv[1]["end"].clone(),
            inv[1]["insert"].clone()
        ),
        (
            serde_json::json!(5),
            serde_json::json!(11),
            serde_json::json!("")
        ),
        "较旧逆在后：删 [5,11)（撤销 ' world' 插入）"
    );

    assert!(core.undo().is_some());
    assert_eq!(
        dump(&core).iter().map(|d| d.2.clone()).collect::<Vec<_>>(),
        vec!["hello"],
        "一次 undo 撤销整组合并文本（序正确则文本精确复原）"
    );
}
