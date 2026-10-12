//! T3 复用地基轮——复用方须知行为的测试锁（t8 审计 P3 §5 缺口表收口）。
//!
//! 来源：boundary-audit P3（编辑事务/撤销审计）登记的测试锁缺口 G1/G4/G6
//! （G2 = 设计事实非缺陷——undo 不恢复选区属 View 域；G5 = undo 栈无上限
//! 属资源策略，留产品裁决，不在本文件伪造"行为"）。四条锁全部为
//! **现状锁（characterization）**：锁定的是当前实际行为，供复用方作为
//! 消费契约依据；其中 G4 两条固化的是 save→open 的 **ID 漂移边界**
//! （审计 P2 §3 路 A 的【推断】在此转为【测试验证】）——它们不是
//! "应当如此"的语义承诺，未来若刻意改变该行为（如 ID 持久化进 meta），
//! 请同步更新本文件与契约文档，而非视红灯为回归。
//!
//! 锁清单：
//! 1. G1：新条目入栈清空 redo（history.rs push_entry）——此前仅有代码级
//!    保证，无专属用例（r12 的 !can_redo() 断言上下文是栈耗尽非清空）；
//! 2. G4a：结构性插入后 save→open，块 ID 按文档先序重解析重编号（漂移
//!    实证：物化 ID 不跨 save→open 存活，跨档引用须知）；
//! 3. G4b：save→open 后 undo 重插物化 ID 与重解析存活块**撞号**的边界
//!    （BTreeMap 注册覆盖无守卫 → 树内重复 BlockId，旧块从 id_index
//!    失联——审计 P2 §3 连带边界的可复现构型）；
//! 4. G6：合并窗内 coalesced 事务与组首**共享同一 revision**（事件面
//!    消费契约：revision 不是事件流的全局唯一序号，按 revision 去重/
//!    排序的消费者必错）。

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

/// G4a：结构性插入后 save→open 的块 ID 漂移——现状锁。
///
/// 机制（审计 P2 §3 路 A）：md 不含 ID；open = 重解析按文档先序重新
/// 分配 1..n。结构性插入产生的物化 ID（如 4）不跨 save→open 存活——
/// 重开后 ID 序列回到位置编号。内容与历史栈完整复原（save_document
/// meta 嵌整栈），**只有块级身份漂移**。
#[test]
fn save_open_renumbers_ids_after_structural_edit_current_behavior() {
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
    let live = preorder_ids(&c);
    assert_eq!(live, vec![1, 4, 2, 3], "活树先序含物化 ID 4");

    let (md, meta) = c.save_document();
    let reopened = Core::open_document(&md, &meta).unwrap();

    // 现状锁：重开 4 块按文档先序重编号 [1,2,3,4]——活树中 MID 的物化
    // ID 4（位置 1）漂移为 2，ID 4 的语义从 MID 转移给 p3。物化 ID 不跨
    // save→open 存活。
    assert_eq!(
        preorder_ids(&reopened),
        vec![1, 2, 3, 4],
        "现状锁（非语义承诺）：save→open 重解析按先序重编号——活树 \
         [1,4,2,3] 漂移为 [1,2,3,4]，跨档持有物化 BlockId 的消费者失联"
    );
    // 内容与历史完整复原（漂移仅限块级身份）。
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

/// G4b：save→open 后 undo 重插物化 ID 与重解析存活块撞号——现状锁。
///
/// 构型（审计 P2 §3 连带边界，撞点经实测校准）：活树 [1,2,3,4] → 插入
/// MID（物化 5）→ [1,5,2,3,4] → 删除 p3（路径 [3]，逆 = 重建 p3 物化
/// id 3）→ 活树 [1,5,2,4] → save。重开重解析 4 块（p1,MID,p2,p4）按
/// 先序 [1,2,3,4]——p2 顶占了被删 p3 的旧 ID 3。此时 undo 删除：逆序
/// 物化 InsertBlock(id=3) 落树 → 与重解析 p2 的 ID 3 **撞号**（注册表
/// BTreeMap::insert 覆盖无守卫）→ 树内重复 BlockId，先注册的 p2 从
/// id_index 失联（仍可按路径访问）。
#[test]
fn save_open_undo_rematerialization_id_collision_boundary() {
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
    // 重解析 4 块（p1,MID,p2,p4）按先序 [1,2,3,4]——p2 顶占 ID 3（原
    // p3 身份）。
    assert_eq!(preorder_ids(&reopened), vec![1, 2, 3, 4]);

    // undo 删除：重插 p3（物化 id=Some(3)）→ 与存活 p2 的 ID 3 撞号。
    reopened.undo().expect("undo 删除成功");
    let ids = preorder_ids(&reopened);
    assert_eq!(
        ids,
        vec![1, 2, 3, 3, 4],
        "现状锁（非语义承诺）：树内出现重复 BlockId 3——重解析存活块与\
         历史物化 ID 撞号，注册表覆盖无守卫"
    );
    let id3_count = ids.iter().filter(|&&i| i == 3).count();
    assert_eq!(id3_count, 2);
    // 覆盖语义：lookup_id(3) 指向最后注册者（重插的 p3），先注册的 p2
    // 从 id_index 失联（但仍可按路径访问——树本体无损）。
    let lookup = reopened.tree.lookup_id(mdcore::id::BlockId(3));
    assert!(lookup.is_some(), "注册表仍可查 ID 3（指向最后注册者）");
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
