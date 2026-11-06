//! T3 复用地基轮——复用方须知行为的测试锁（t8 审计 P3 §5 缺口表收口）。
//!
//! 来源：boundary-audit P3（编辑事务/撤销审计）登记的测试锁缺口 G1/G4/G6
//! （G2 = 设计事实非缺陷——undo 不恢复选区属 View 域；G5 = undo 栈无上限
//! 属资源策略，留产品裁决，不在本文件伪造"行为"）。
//!
//! 锁清单：
//! 1. G1：新条目入栈清空 redo（history.rs push_entry）——此前仅有代码级
//!    保证，无专属用例（r12 的 !can_redo() 断言上下文是栈耗尽非清空）；
//! 2. G4a：结构性插入后 save→open 块 ID **稳定**（#244 路线 A：blocks 表
//!    按位恢复原 ID，物化 ID 跨档存活）——原「先序重编号漂移」现状锁
//!    已随 #244 改写为语义锁（该漂移正是审计 P0-1 的第一失效点）；
//! 3. G4b：跨会话 undo 重插物化 ID 不再撞号（#244 I1/I3）；旧档（无
//!    blocks 表）撞号面由 register 守卫响亮拦截（I4/I5：typed error +
//!    状态完整保留，不静默毒化）；
//! 4. G6：合并窗内 coalesced 事务与组首**共享同一 revision**（事件面
//!    消费契约：revision 不是事件流的全局唯一序号，按 revision 去重/
//!    排序的消费者必错）。
//!
//! 历史：G4 两条原为现状锁（固化「ID 按位置漂移 + 撞号静默覆盖」），
//! #244（审计 P0-1）落地后按本文件头部旧约（「若 ID 持久化进 meta，
//! 同步更新本文件与契约文档」）改写为语义锁。

use std::sync::Arc;

use mdcore::op::Op;
use mdcore::tree::{BlockKind, NodeId};
use mdcore::{ApplyMeta, Core};

/// 先序遍历收集块 ID（跳过虚根 BlockId(0)）——镜像 blocks_json 的先序。
fn preorder_ids(c: &Core) -> Vec<u64> {
    fn walk(c: &Core, nid: NodeId, out: &mut Vec<u64>) {
        let node = c.tree.node(nid);
        if node.id.0 != 0 {
            out.push(node.id.0);
        }
        for &ch in &node.children {
            walk(c, ch, out);
        }
    }
    let mut out = Vec::new();
    walk(c, NodeId(0), &mut out);
    out
}

fn replace_text(start: usize, end: usize, insert: &str) -> Op {
    Op::ReplaceText {
        path: vec![0],
        start,
        end,
        insert: insert.into(),
    }
}

/// G1：push_entry 清空 redo 栈——专属用例（此前无锁）。
///
/// 场景：文本编辑 → undo（redo 栈有条目）→ 同一合并窗内再编辑。
/// 合并被两道闸门禁止（last_kind=None 破组 + redo 非空禁合并），
/// 新条目必走 push_entry → redo.clear()。断言 redo 深度归零且 redo()
/// 返回 None。
#[test]
fn push_entry_clears_redo_stack_on_new_entry() {
    let mut c = Core::from_markdown_with_clock("hello world", Arc::new(|| 0));
    c.apply_ops(vec![replace_text(0, 0, "X")], ApplyMeta::view("edit"))
        .unwrap();
    assert_eq!(c.history.undo_depth(), 1, "首条目在栈");

    c.undo().expect("undo 成功");
    assert!(c.history.redo_depth() >= 1, "undo 后 redo 栈有条目");
    assert!(c.history.can_redo());

    // 同窗（t=0，时钟未动）新编辑：破组（pop_undo 置 last_kind=None）+
    // redo 非空双闸 → 必开新条目 → push_entry 清 redo。
    c.apply_ops(vec![replace_text(1, 1, "Y")], ApplyMeta::view("edit"))
        .unwrap();
    assert_eq!(
        c.history.redo_depth(),
        0,
        "新条目入栈即清 redo（history.rs push_entry）——重做机会被废弃"
    );
    assert!(!c.history.can_redo());
    assert!(c.redo().is_none(), "redo() 对清空栈返回 None");
}

/// G4a：结构性插入后 save→open 块 ID 稳定——语义锁（#244 路线 A）。
///
/// 机制：`save_document` 把活树先序 ID 表写进 meta（blocks 字段，
/// `meta_format` v2）；`open_document` 按表恢复原 ID。结构性插入产生
/// 的物化 ID（如 4）跨 save→open 存活——跨档持有物化 BlockId 的消费
/// 者（AI 锚点 / 协作端 / 证据定位）不再失联。内容与历史栈照常复原。
#[test]
fn save_open_preserves_ids_after_structural_edit() {
    let mut c = Core::from_markdown_with_clock("p1\n\np2\n\np3", Arc::new(|| 0));
    assert_eq!(preorder_ids(&c), vec![1, 2, 3], "解析按先序分配 1..n");

    // 结构性插入（物化 ID = 4，单调接续水位）。顶层插入：parent = 空路径
    // （虚根），index = 1（第二位）。
    c.insert_block(
        c.revision(),
        vec![],
        1,
        BlockKind::Paragraph,
        Default::default(),
        "MID".to_string(),
    )
    .unwrap();
    assert_eq!(preorder_ids(&c), vec![1, 4, 2, 3], "活树先序含物化 ID 4");

    let (md, meta) = c.save_document();
    let reopened = Core::open_document(&md, &meta).unwrap();

    // 语义锁（#244）：blocks 表按位恢复原 ID——物化 ID 4 跨档存活
    // （修复前为先序重编号：[1,4,2,3] 漂移为 [1,2,3,4]，ID 4 的语义
    // 从 MID 转移给 p3——审计 P0-1 第一失效点，已消除）。
    assert_eq!(
        preorder_ids(&reopened),
        vec![1, 4, 2, 3],
        "#244 语义锁：save→open 块身份稳定（物化 ID 跨档存活）"
    );
    // 内容与历史完整复原（save_document meta 嵌整栈）。
    assert_eq!(
        mdcore::markdown::tree_to_markdown(&reopened.tree),
        mdcore::markdown::tree_to_markdown(&c.tree),
        "内容无损"
    );
    assert_eq!(
        reopened.history.undo_depth(),
        c.history.undo_depth(),
        "历史栈经 meta 复原"
    );
}

/// G4b：跨会话 undo 重插物化 ID 不再撞号——语义锁（#244 路线 A + I1/I3）。
///
/// 构型（沿审计 P2 §3 连带边界，撞点经实测校准）：活树 [1,2,3,4] → 插入
/// MID（物化 5）→ [1,5,2,3,4] → 删除 p3（路径 [3]，逆 = 重建 p3 物化
/// id 3）→ 活树 [1,5,2,4] → save。重开按 blocks 表恢复原 ID
/// [1,5,2,4]（修复前为先序重发 [1,2,3,4]——p2 顶占被删 p3 的旧 ID 3，
/// 正是撞号根源）。undo 删除：物化 InsertBlock(id=3) 落树——3 不在
/// 活树在册集 → 无撞号，树恢复 [1,5,2,3,4] 唯一。
#[test]
fn cross_session_undo_rematerialization_no_collision() {
    let mut c = Core::from_markdown_with_clock("p1\n\np2\n\np3\n\np4", Arc::new(|| 0));
    assert_eq!(preorder_ids(&c), vec![1, 2, 3, 4]);

    c.insert_block(
        c.revision(),
        vec![],
        1,
        BlockKind::Paragraph,
        Default::default(),
        "MID".to_string(),
    )
    .unwrap();
    assert_eq!(preorder_ids(&c), vec![1, 5, 2, 3, 4], "插入物化 ID 5");

    // 删除 p3（插入后顶层第 4 位，路径 [3]）：活树 [1,5,2,4]，条目逆 =
    // 重建 p3 物化 id 3。
    c.delete_block(c.revision(), vec![3]).unwrap();
    assert_eq!(preorder_ids(&c), vec![1, 5, 2, 4]);

    let (md, meta) = c.save_document();
    let mut reopened = Core::open_document(&md, &meta).unwrap();
    // blocks 表按位恢复：p1=1, MID=5, p2=2, p4=4（修复前先序重发
    // [1,2,3,4]，p2 顶占 ID 3）。
    assert_eq!(preorder_ids(&reopened), vec![1, 5, 2, 4]);

    // undo 删除：重插 p3（物化 id=3）→ 3 空闲 → 无撞号（I3）。
    reopened
        .undo()
        .expect("跨会话 undo 必须成功（#244 I3：物化 ID 与持久化活树不再撞号）");
    let ids = preorder_ids(&reopened);
    assert_eq!(
        ids,
        vec![1, 5, 2, 3, 4],
        "undo 后树恢复删除前构型 [1,5,2,3,4]"
    );
    assert_eq!(
        ids.iter().filter(|&&i| i == 3).count(),
        1,
        "I1：ID 3 在活树唯一（修复前为重复 2 次）"
    );
    // 先注册者不再失联（修复前：BTreeMap::insert 覆盖 → p2 从 id_index
    // 失联）。注册表对每个在册 ID 恰好可解。
    assert!(
        reopened.tree.lookup_id(mdcore::id::BlockId(2)).is_some(),
        "p2 仍在册（修复前被静默覆盖失联）"
    );
    assert!(
        reopened.tree.lookup_id(mdcore::id::BlockId(3)).is_some(),
        "重插的 p3 可按 ID 寻址"
    );
}

/// G4b-legacy：旧档（无 blocks 表）跨会话 undo 撞号 → register 守卫响亮
/// 拦截——语义锁（#244 I4/I5）。
///
/// 旧档 = #244 修复前形态（meta 无 meta_format / blocks 字段）：open 按
/// 先序重发 ID，历史栈照常恢复——undo 重插物化 ID 3 与存活 p2（重发
/// 顶占 3）撞号。守卫语义：typed error（IdCollision）拒绝落树，条目压
/// 回栈 + 边界拨回（T1-fix F10），调用后状态与调用前完全一致——
/// **绝不静默覆盖毒化活树**（修复前：树内重复 BlockId + p2 失联）。
#[test]
fn legacy_meta_cross_session_undo_collision_fails_loud() {
    let mut c = Core::from_markdown_with_clock("p1\n\np2\n\np3\n\np4", Arc::new(|| 0));
    c.insert_block(
        c.revision(),
        vec![],
        1,
        BlockKind::Paragraph,
        Default::default(),
        "MID".to_string(),
    )
    .unwrap();
    c.delete_block(c.revision(), vec![3]).unwrap();
    let (md, meta) = c.save_document();

    // 降格为旧档形态：剥掉 meta_format + blocks（#244 前的 meta 形状）。
    let mut v: serde_json::Value = serde_json::from_str(&meta).unwrap();
    let obj = v.as_object_mut().unwrap();
    obj.remove("meta_format");
    obj.remove("blocks");
    let legacy_meta = v.to_string();

    let mut reopened = Core::open_document(&md, &legacy_meta).unwrap();
    assert_eq!(
        preorder_ids(&reopened),
        vec![1, 2, 3, 4],
        "旧档：先序重发（p2 顶占 ID 3——#244 前旧语义，撞号面打开）"
    );

    // 撞号 undo：物化 id=3 vs 存活 p2(3) → IdCollision 拒绝 → undo 返回
    // None，条目压回，树/revision/栈深与调用前一致。
    let depth_before = reopened.history.undo_depth();
    let rev_before = reopened.revision();
    assert!(
        reopened.undo().is_none(),
        "I5：撞号 undo 响亮失败（typed IdCollision），不得静默毒化"
    );
    assert_eq!(
        preorder_ids(&reopened),
        vec![1, 2, 3, 4],
        "状态与调用前一致（守卫拒绝 + 条目压回，树零变更）"
    );
    assert_eq!(reopened.history.undo_depth(), depth_before, "条目压回栈");
    assert_eq!(reopened.revision(), rev_before, "revision 不动（未 bump）");
    // 注册表未被静默覆盖：ID 3 仍指向先注册的 p2（而非重插条目）。
    assert!(
        reopened.tree.lookup_id(mdcore::id::BlockId(3)).is_some(),
        "id_index 未被覆盖（守卫先于插入拒绝）"
    );
    // 旧档升级路径：继续编辑并再保存 → 升 v2（blocks 表写入，单向收敛）。
    reopened
        .apply_ops(
            vec![Op::ReplaceText {
                path: vec![0],
                start: 0,
                end: 0,
                insert: "x".into(),
            }],
            ApplyMeta::view("edit"),
        )
        .unwrap();
    let (_, meta2) = reopened.save_document();
    let v2: serde_json::Value = serde_json::from_str(&meta2).unwrap();
    assert_eq!(
        v2["meta_format"],
        serde_json::json!(mdcore::document::META_FORMAT_VERSION),
        "旧档再存即升 v2（blocks 表随档写入）"
    );
}

/// G6：coalesced 事务的事件与组首共享同一 revision——消费契约锁。
///
/// 事件面消费者不得以 revision 作为全局唯一序号做去重/排序：合并窗内
/// 第二笔事务的事件 revision 与组首相同（不 bump），仅 meta.coalesced
/// 区分。对应 p2 §6 / P3 §5-G6。
#[test]
fn coalesced_event_shares_revision_with_group_head() {
    let mut c = Core::from_markdown_with_clock("hello world", Arc::new(|| 0));
    c.apply_ops(vec![replace_text(0, 0, "X")], ApplyMeta::view("typing"))
        .unwrap();
    let head_rev = c.revision();
    let head_events = c.take_events();
    assert_eq!(head_events.len(), 1);
    assert!(!head_events[0].meta.coalesced, "组首事件非合并");

    // 同窗（t=500 < 1000）第二笔 View 文本事务 → 合并进组首条目。
    c.set_clock(Arc::new(|| 500));
    c.apply_ops(vec![replace_text(1, 1, "Y")], ApplyMeta::view("typing"))
        .unwrap();

    assert_eq!(c.revision(), head_rev, "合并事务不 bump revision");
    assert_eq!(c.history.undo_depth(), 1, "同窗击键并入单条目");

    let events = c.take_events();
    assert_eq!(events.len(), 1, "合并事务仍各发一事件");
    assert_eq!(
        events[0].revision, head_rev,
        "coalesced 事件 revision == 组首 revision（消费契约：revision 非唯一序号）"
    );
    assert!(events[0].meta.coalesced, "以 meta.coalesced 区分");
}
