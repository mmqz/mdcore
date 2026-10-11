//! R17 — P1-4 终案：Stable ID 在三类边界域的实证规则（规则全文见
//! decisions/ADR-B5-block-id-boundary-rules.md，本文件是规则的测试形态）。
//!
//! 三域：
//! - **table internals**：core 的表 = 单块模型（§8.1 Table，text = 原始管道行
//!   按 \n 连接，markdown.rs::markdown_to_tree 的 `|` 连续行合并规则）——
//!   表内编辑/加行都是块内 ReplaceText，不产生子块、不产生新块；
//! - **复杂嵌套 list**：3+ 层 item 的 split/merge marker/depth 继承与 ID
//!   分配（新增 item 新 ID，既有 item 不变）；
//! - **跨块 paste**：CM6 粘贴 → fullResync（sync_from_markdown 按位置朴素
//!   差分）的 ID 保持规则 = **对齐位保 ID、净变尾部新 ID/删除**（可预测规则，
//!   非树编辑距离）。
//!
//! 全部规则在非 BMP 语料上同型成立（单位 = 码点，见 r17_utf16_mapping.rs）。

use mdcore::id::BlockId;
use mdcore::markdown::tree_to_markdown;
use mdcore::op::Op;
use mdcore::tree::BlockKind;
use mdcore::Core;

/// 不合并窗的核：时钟每次调用步进 10s（>COALESCE_MS 1s），连续文本编辑
/// 各自独立成历史条目（undo 粒度确定性）。
fn core_no_coalesce(md: &str) -> Core {
    let mut c = Core::from_markdown(md);
    let tick = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let t2 = tick.clone();
    c.set_clock(std::sync::Arc::new(move || {
        t2.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed)
    }));
    c
}

fn top_texts(core: &Core) -> Vec<String> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).text.to_string())
        .collect()
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

fn node_id(core: &Core, path: &[usize]) -> BlockId {
    let slot = core.tree.resolve(path).expect("resolve path");
    core.tree.node(slot).id
}

fn last_event_ops(core: &mut Core) -> Vec<Op> {
    let events = core.take_events();
    events.last().unwrap().ops.clone()
}

// ---------------------------------------------------------------------------
// 域 1：table internals（单块模型）
// ---------------------------------------------------------------------------

// 1. 表 = 单块：表内文本编辑（含跨行区间）不产生新块、ID 保持
#[test]
fn table_single_block_row_edit_keeps_identity() {
    let mut core = core_no_coalesce("P0\n| a𠮷 | 🙂 |\n| c | d |\nP1");
    assert_eq!(
        top_kinds(&core),
        vec![BlockKind::Paragraph, BlockKind::Table, BlockKind::Paragraph]
    );
    let t_id = node_id(&core, &[1]);
    // 表块文本 = 原始管道行按 \n 连接（含分隔行语义未建模，v0 如实记录）
    assert_eq!(node_text_of(&core, &[1]), "| a𠮷 | 🙂 |\n| c | d |");
    let t_len = "| a𠮷 | 🙂 |\n| c | d |".chars().count();

    // 行 2 的 'c' 在块内码点 13（行1 10 码点 + \n + "| "）——块内偏移编辑
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![1],
            start: 13,
            end: 14,
            insert: "🦊".into(),
        }],
        mdcore::ApplyMeta::view("test"),
    )
    .unwrap();
    assert_eq!(node_text_of(&core, &[1]), "| a𠮷 | 🙂 |\n| 🦊 | d |");
    assert_eq!(node_id(&core, &[1]), t_id, "表内编辑 ID 保持");
    assert_eq!(core.tree.top_level().len(), 3, "不产生新块");

    // 行内非 BMP 替换：🙂（码点 7）→ X，仍是单块文本编辑
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![1],
            start: 7,
            end: 8,
            insert: "X".into(),
        }],
        mdcore::ApplyMeta::view("test"),
    )
    .unwrap();
    assert_eq!(node_text_of(&core, &[1]), "| a𠮷 | X |\n| 🦊 | d |");
    assert_eq!(node_id(&core, &[1]), t_id);
    assert_eq!(t_len, 20, "表块文本码点数自证（防 fixture 漂移）");
    core.undo().unwrap();
    core.undo().unwrap();
    assert_eq!(node_text_of(&core, &[1]), "| a𠮷 | 🙂 |\n| c | d |");
}

fn node_text_of(core: &Core, path: &[usize]) -> String {
    let slot = core.tree.resolve(path).expect("resolve path");
    core.tree.node(slot).text.to_string()
}

// 2. 表尾加行 = 块内文本增长（非新块）：sync 差分为原位 ReplaceText
#[test]
fn table_row_append_is_text_growth_not_new_block() {
    let mut core = core_no_coalesce("P0\n| a |\n| b |\nP1");
    let ids = top_ids(&core);
    let old_t = node_text_of(&core, &[1]);
    core.sync_from_markdown("P0\n| a |\n| b |\n| c | d |\nP1")
        .unwrap();
    let ops = last_event_ops(&mut core);
    // 同位置表块 → 单个原位 ReplaceText（ID 保持）；无任何 InsertBlock
    assert_eq!(ops.len(), 1, "恰好 1 个 op: {ops:?}");
    let old_t_len = old_t.chars().count();
    assert_eq!(old_t_len, 11, "旧表文本码点数自证（\"| a |\n| b |\" = 11）");
    assert!(
        matches!(&ops[0], Op::ReplaceText { path, start: 0, end, .. }
        if path == &vec![1usize] && *end == old_t_len),
        "ReplaceText end = 旧表文本码点数"
    );
    assert_eq!(
        top_kinds(&core),
        vec![BlockKind::Paragraph, BlockKind::Table, BlockKind::Paragraph]
    );
    assert_eq!(top_ids(&core), ids, "加行不改身份");
    assert_eq!(node_text_of(&core, &[1]), "| a |\n| b |\n| c | d |");
    core.undo().unwrap();
    assert_eq!(node_text_of(&core, &[1]), old_t);
}

// 3. 表中段被非管道行打断：对齐位保 ID（首表槽保 ID + 原表槽被改写），
//    错位尾部新 ID——sync 按位置对照的可预测规则，不做树编辑距离
#[test]
fn table_middle_break_aligned_prefix_keeps_id_tail_new() {
    let mut core = core_no_coalesce("P0\n| a |\n| b |\n| c |\nP1");
    let ids = top_ids(&core);
    core.sync_from_markdown("P0\n| a |\nX\n| c |\nP1").unwrap();
    // 位置对照：[0] P0 不动；[1] T(3行)→T(1行) 原位；[2] P1 槽被改写为 "X"；
    //           [3][4] 新表 + 新 P1
    assert_eq!(
        top_kinds(&core),
        vec![
            BlockKind::Paragraph,
            BlockKind::Table,
            BlockKind::Paragraph,
            BlockKind::Table,
            BlockKind::Paragraph
        ]
    );
    assert_eq!(top_texts(&core), vec!["P0", "| a |", "X", "| c |", "P1"]);
    let now = top_ids(&core);
    assert_eq!(now[0], ids[0]);
    assert_eq!(now[1], ids[1], "首表槽保 ID");
    assert_eq!(
        now[2], ids[2],
        "对齐位 2 = 原 P1 的 ID（内容被改写——按位对齐规则的代价）"
    );
    assert!(now[3].0 > ids[2].0 && now[4].0 > now[3].0, "净增尾部新 ID");
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["P0", "| a |\n| b |\n| c |", "P1"]);
    assert_eq!(top_ids(&core), ids, "undo 后身份复原");
}

// 4. 粘贴表格落入段落槽：SetBlockType + ReplaceText，ID 保持
#[test]
fn paste_table_aligns_into_slot_keeps_id() {
    let mut core = core_no_coalesce("p1\nMID\np2");
    let mid_id = node_id(&core, &[1]);
    core.sync_from_markdown("p1\n| a |\n| b |\np2").unwrap();
    let ops = last_event_ops(&mut core);
    assert_eq!(ops.len(), 2, "SetBlockType + ReplaceText: {ops:?}");
    assert!(
        matches!(&ops[0], Op::SetBlockType { path, kind: BlockKind::Table } if path == &vec![1usize])
    );
    assert!(matches!(&ops[1], Op::ReplaceText { path, .. } if path == &vec![1usize]));
    assert_eq!(
        top_kinds(&core),
        vec![BlockKind::Paragraph, BlockKind::Table, BlockKind::Paragraph]
    );
    assert_eq!(node_id(&core, &[1]), mid_id, "对齐槽吸收粘贴，ID 保持");
}

// 5. 粘贴 表格+fence 复合内容到文档尾：全部新 ID；undo/redo 身份恒等
#[test]
fn paste_table_and_fence_tail_new_ids_undo_identity() {
    let mut core = core_no_coalesce("p1\np2");
    let ids = top_ids(&core);
    core.sync_from_markdown("p1\np2\n| a𠮷 | 🙂 |\n| c | d |\n```js\nx𠮷\n```")
        .unwrap();
    assert_eq!(
        top_kinds(&core),
        vec![
            BlockKind::Paragraph,
            BlockKind::Paragraph,
            BlockKind::Table,
            BlockKind::CodeBlock
        ]
    );
    let now = top_ids(&core);
    assert!(now[0] == ids[0] && now[1] == ids[1], "前缀对齐保 ID");
    assert!(
        now[2].0 > ids[1].0 && now[3].0 > now[2].0,
        "粘贴块全部新 ID"
    );
    let pasted_ids = now.clone();
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["p1", "p2"]);
    core.redo().unwrap();
    assert_eq!(
        top_ids(&core),
        pasted_ids,
        "redo 复原粘贴身份（inverses 物化 ID）"
    );
    assert_eq!(node_text_of(&core, &[3]), "x𠮷");
}

// ---------------------------------------------------------------------------
// 域 2：复杂嵌套 list（3+ 层）
// ---------------------------------------------------------------------------

// 6. 嵌套 item split：marker/depth 继承 + 新 ID；既有 item（含祖先）不变
#[test]
fn nested_list_split_inherits_marker_depth_new_id() {
    let mut core = core_no_coalesce("- a 𠮷\n  - b 🙂\n    - c 👨‍👩‍👧‍👦");
    // 树形：a=[0] > b=[0,0] > c=[0,0,0]（depth 栈建树）
    assert_eq!(node_text_of(&core, &[0, 0, 0]), "c 👨‍👩‍👧‍👦");
    let (a_id, b_id, c_id) = (
        node_id(&core, &[0]),
        node_id(&core, &[0, 0]),
        node_id(&core, &[0, 0, 0]),
    );
    let watermark = core.tree.alloc.watermark();

    // 在 b（"b 🙂"，3 码点）码点 2 处分段（纯文本切分不修整空白）：
    // 左 "b "（保留分隔空格），右 "🙂"（非 BMP 起头的新 item）
    core.split_block(core.revision(), vec![0, 0], 2).unwrap();
    assert_eq!(node_text_of(&core, &[0, 0]), "b ");
    let tail = &core.tree.node(core.tree.resolve(&[0, 1]).unwrap());
    assert_eq!(tail.text.to_string(), "🙂");
    assert_eq!(
        tail.attrs.get("marker"),
        Some(&"-".to_string()),
        "marker 继承"
    );
    assert_eq!(
        tail.attrs.get("depth"),
        Some(&"2".to_string()),
        "depth 继承"
    );
    assert!(tail.id.0 >= watermark, "新增 item 新分配 ID");
    assert_eq!(node_id(&core, &[0]), a_id, "祖先 item ID 不变");
    assert_eq!(node_id(&core, &[0, 0]), b_id, "被切 item（head）ID 不变");
    assert_eq!(
        node_id(&core, &[0, 0, 0]),
        c_id,
        "子 item 跟随 head，ID 不变"
    );
    // v0 规则：tail 是 b 的下一个兄弟（c 仍留在 b 下）——先序序列化顺序如实反映
    assert_eq!(
        tree_to_markdown(&core.tree),
        "- a 𠮷\n  - b \n    - c 👨‍👩‍👧‍👦\n  - 🙂"
    );
}

// 7. 嵌套 item merge：survivor = 前一个兄弟（同父），文本并回，marker 保持 survivor 的
#[test]
fn nested_list_merge_survivor_prev_sibling() {
    let mut core = core_no_coalesce("- a 𠮷\n  - b 🙂\n  - b2 β");
    // b=[0,0], b2=[0,1]
    let (b_id, b2_id) = (node_id(&core, &[0, 0]), node_id(&core, &[0, 1]));
    core.merge_block(core.revision(), vec![0, 1]).unwrap();
    assert_eq!(
        node_text_of(&core, &[0, 0]),
        "b 🙂b2 β",
        "survivor=b，尾部文本并回"
    );
    assert_eq!(node_id(&core, &[0, 0]), b_id, "survivor ID 保持");
    assert!(
        core.tree.lookup_id(b2_id).is_none(),
        "被并块身份失效不回收（§8.2）"
    );
    assert_eq!(tree_to_markdown(&core.tree), "- a 𠮷\n  - b 🙂b2 β");
    core.undo().unwrap();
    assert_eq!(node_text_of(&core, &[0, 0]), "b 🙂");
    assert_eq!(node_text_of(&core, &[0, 1]), "b2 β");
    // undo 后 b2 的 ID 复原（inverses 物化重建）
    assert_eq!(node_id(&core, &[0, 1]), b2_id);
}

// 8. 顶层 item split（带嵌套子树）：子树留在 head，tail 挂到顶层
#[test]
fn nested_top_split_children_stay_with_head() {
    let mut core = core_no_coalesce("- a 𠮷\n  - b 🙂\n    - c 👨‍👩‍👧‍👦");
    let (a_id, b_id) = (node_id(&core, &[0]), node_id(&core, &[0, 0]));
    // "a 𠮷" = 3 码点，在码点 2 分段（纯文本切分不修整空白）：左 "a "，右 "𠮷"
    core.split_block(core.revision(), vec![0], 2).unwrap();
    assert_eq!(node_text_of(&core, &[0]), "a ");
    assert_eq!(node_text_of(&core, &[1]), "𠮷");
    let tail_id = node_id(&core, &[1]);
    // head 保留全部子树（b/c 跟 a 不跟 tail）
    assert_eq!(node_id(&core, &[0, 0]), b_id, "子树留在 head");
    assert_eq!(node_id(&core, &[0]), a_id);
    let tail = &core.tree.node(core.tree.resolve(&[1]).unwrap());
    assert_eq!(tail.attrs.get("marker"), Some(&"-".to_string()));
    assert_eq!(
        tail.attrs.get("depth"),
        Some(&"1".to_string()),
        "顶层 item depth 继承"
    );
    assert!(tail.children.is_empty(), "tail 不继承子树（v0 规则）");
    let _ = tail_id;
    assert_eq!(
        tree_to_markdown(&core.tree),
        "- a \n  - b 🙂\n    - c 👨‍👩‍👧‍👦\n- 𠮷"
    );
    core.undo().unwrap();
    assert_eq!(tree_to_markdown(&core.tree), "- a 𠮷\n  - b 🙂\n    - c 👨‍👩‍👧‍👦");
}

// 9. 嵌套续行粘贴（sync）：子块插入到正确父位，既有 item ID 全保持
#[test]
fn deep_nest_sync_child_insert_keeps_existing_ids() {
    let mut core = core_no_coalesce("- a\n  - a1\n- b");
    let ids = top_ids(&core);
    let a1_id = node_id(&core, &[0, 0]);
    core.sync_from_markdown("- a\n  - a1\n  - a2\n- b").unwrap();
    let ops = last_event_ops(&mut core);
    assert_eq!(ops.len(), 1);
    assert!(
        matches!(&ops[0], Op::InsertBlock { parent, index, .. }
        if parent == &vec![0usize] && *index == 1),
        "a2 插到 a 的子位 [0,1]: {ops:?}"
    );
    assert_eq!(node_text_of(&core, &[0, 1]), "a2");
    let now = top_ids(&core);
    assert!(now[0] == ids[0] && now[1] == ids[1], "顶层 item ID 不变");
    assert_eq!(node_id(&core, &[0, 0]), a1_id, "既有子 item ID 不变");
    assert_eq!(tree_to_markdown(&core.tree), "- a\n  - a1\n  - a2\n- b");
}

// 10. 有序 marker 继承：split 后 tail 保持 "1." 字面 marker
#[test]
fn ordered_marker_split_inherits() {
    let mut core = core_no_coalesce("1. 甲𠮷乙");
    core.split_block(core.revision(), vec![0], 2).unwrap();
    // "甲𠮷" = 2 码点在左，"乙" 进 tail
    assert_eq!(node_text_of(&core, &[0]), "甲𠮷");
    assert_eq!(node_text_of(&core, &[1]), "乙");
    let tail = &core.tree.node(core.tree.resolve(&[1]).unwrap());
    assert_eq!(
        tail.attrs.get("marker"),
        Some(&"1.".to_string()),
        "有序 marker 字面继承"
    );
    assert_eq!(tree_to_markdown(&core.tree), "1. 甲𠮷\n1. 乙");
}

// ---------------------------------------------------------------------------
// 域 3：跨块 paste（sync_from_markdown 按位置朴素差分）
// ---------------------------------------------------------------------------

// 11. 中段插入：对齐前缀保 ID；错位槽被原位改写；净增尾部新 ID
#[test]
fn paste_middle_insert_aligned_prefix_keeps_ids() {
    let mut core = core_no_coalesce("a\nb");
    let ids = top_ids(&core);
    core.sync_from_markdown("a\nX\nb").unwrap();
    let ops = last_event_ops(&mut core);
    // 位置对照：[0] a 不动；[1] b 槽 → ReplaceText 为 "X"；尾部插入 "b"
    // （InsertBlock 的 id 已被 apply_op 回写物化——事件流携带具体 ID）
    assert_eq!(ops.len(), 2, "最小 op 序列 = 原位改写 + 尾插: {ops:?}");
    assert_eq!(
        ops[0],
        Op::ReplaceText {
            path: vec![1usize],
            start: 0,
            end: 1,
            insert: "X".into()
        }
    );
    assert!(
        matches!(&ops[1], Op::InsertBlock { parent, index, kind: BlockKind::Paragraph, text, id: Some(_), .. }
        if parent.is_empty() && *index == 2 && text.as_ref() == "b")
    );
    let now = top_ids(&core);
    assert_eq!(now[0], ids[0]);
    assert_eq!(
        now[1], ids[1],
        "槽 1 = 原 b 的 ID（内容已是 X——按位对齐规则的代价）"
    );
    assert!(now[2].0 > ids[1].0, "尾部 b 为新 ID");
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["a", "b"]);
    assert_eq!(top_ids(&core), ids, "undo 后身份复原");
}

// 12. 中段删除：槽位继承（后块顶上前块的 ID），被删块身份失效
#[test]
fn paste_middle_delete_slot_inheritance() {
    let mut core = core_no_coalesce("a\nb\nc");
    let ids = top_ids(&core);
    core.sync_from_markdown("a\nc").unwrap();
    let ops = last_event_ops(&mut core);
    assert_eq!(
        ops,
        vec![
            Op::ReplaceText {
                path: vec![1usize],
                start: 0,
                end: 1,
                insert: "c".into()
            },
            Op::RemoveBlock { path: vec![2usize] },
        ]
    );
    let now = top_ids(&core);
    assert_eq!(
        now,
        vec![ids[0], ids[1]],
        "c 内容落在 b 的槽 ID 下（ID 跟位置不跟内容）"
    );
    assert!(core.tree.lookup_id(ids[2]).is_none(), "原 c 的 ID 失效");
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["a", "b", "c"]);
    assert_eq!(top_ids(&core), ids);
}

// 13. 首/中/尾多块同时改写（等长）：全部原位 ReplaceText，ID 全保持
#[test]
fn paste_head_mid_tail_rewrite_all_ids_kept() {
    let mut core = core_no_coalesce("a\nb\nc");
    let ids = top_ids(&core);
    core.sync_from_markdown("A𠮷\nB🙂\nC👨‍👩‍👧‍👦").unwrap();
    let ops = last_event_ops(&mut core);
    assert_eq!(ops.len(), 3, "三个原位 ReplaceText: {ops:?}");
    for (i, text) in ["A𠮷", "B🙂", "C👨‍👩‍👧‍👦"].iter().enumerate() {
        assert!(matches!(&ops[i], Op::ReplaceText { start: 0, .. }));
        assert_eq!(node_text_of(&core, &[i]), *text);
    }
    assert_eq!(top_ids(&core), ids, "等长改写零身份变化");
}

// 14. 尾部增删往返：删从高位往低位（低位路径有效），再增为新 ID（永不复用）
#[test]
fn paste_tail_grow_shrink_id_stability() {
    let mut core = core_no_coalesce("a\nb\nc\nd");
    let ids4 = top_ids(&core);
    core.sync_from_markdown("a\nb").unwrap();
    let ops = last_event_ops(&mut core);
    assert_eq!(
        ops,
        vec![
            Op::RemoveBlock { path: vec![3usize] },
            Op::RemoveBlock { path: vec![2usize] }
        ],
        "尾部删除按高位→低位"
    );
    assert_eq!(top_ids(&core), vec![ids4[0], ids4[1]], "保留块 ID 不变");
    let ids2 = top_ids(&core);
    core.sync_from_markdown("a\nb\nc\nd").unwrap();
    let now = top_ids(&core);
    assert!(now[0] == ids2[0] && now[1] == ids2[1]);
    assert!(
        now[2].0 > ids4[3].0 && now[3].0 > now[2].0,
        "再插入为新 ID（分配器永不复用）"
    );
}

// 15. 复合内容跨块粘贴（quote+嵌套 list）：新子树先序新 ID；undo/redo 身份恒等
#[test]
fn cross_block_paste_undo_redo_identity() {
    let mut core = core_no_coalesce("p1\np2");
    let ids = top_ids(&core);
    core.sync_from_markdown("p1\np2\n> q 🚀\n- item 𠮷\n  - nested 👨‍👩‍👧‍👦")
        .unwrap();
    // 新 top：[p1, p2, quote(容器+子段落), item(含 nested)]
    assert_eq!(
        top_kinds(&core),
        vec![
            BlockKind::Paragraph,
            BlockKind::Paragraph,
            BlockKind::Blockquote,
            BlockKind::ListItem
        ]
    );
    assert_eq!(node_text_of(&core, &[2, 0]), "q 🚀");
    assert_eq!(node_text_of(&core, &[3]), "item 𠮷");
    assert_eq!(node_text_of(&core, &[3, 0]), "nested 👨‍👩‍👧‍👦");
    let now = top_ids(&core);
    assert!(now[0] == ids[0] && now[1] == ids[1], "前缀对齐保 ID");
    assert!(
        now[2].0 > ids[1].0 && now[3].0 > now[2].0,
        "粘贴子树全部新 ID"
    );
    let pasted = (
        now.clone(),
        node_id(&core, &[2, 0]),
        node_id(&core, &[3, 0]),
    );
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["p1", "p2"]);
    assert_eq!(top_ids(&core), ids);
    core.redo().unwrap();
    assert_eq!(top_ids(&core), pasted.0, "redo 身份复原（物化 ID）");
    assert_eq!(node_id(&core, &[2, 0]), pasted.1);
    assert_eq!(node_id(&core, &[3, 0]), pasted.2);
}
