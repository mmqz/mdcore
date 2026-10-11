//! R23-c — 外部文件重载语义锁（P1-8 核侧，ADR-B7）。
//!
//! 锁定面：①探针 stale 判定；②三态决策（Noop/SilentAdopt/OfferChoice）；
//! ③采纳身份保持（edited/moved/added/removed 四象限 + 嵌套子树）；④新鲜 id
//! 单调不撞旧身份；⑤take-external 交互史作废 + 重载后首个 View 击键独立；
//! ⑥Noop 幂等（零事务零 revision 推进）；⑦空文档边界；⑧manifest 可信
//! added 携带声明身份；⑨跨运行确定性。

use mdcore::external::{plan_reload, FileStamp, ReloadAction};
use mdcore::fingerprint::{ManifestEntry, TreeFingerprint};
use mdcore::id::BlockId;
use mdcore::markdown::markdown_to_tree;
use mdcore::op::Op;
use mdcore::tree::NodePath;
use mdcore::{ApplyMeta, Core};

fn ids(core: &Core) -> Vec<BlockId> {
    fn walk(core: &Core, id: mdcore::tree::NodeId, out: &mut Vec<BlockId>) {
        out.push(core.tree.node(id).id);
        for &c in &core.tree.node(id).children {
            walk(core, c, out);
        }
    }
    let mut out = Vec::new();
    for &c in core.tree.top_level() {
        walk(core, c, &mut out);
    }
    out
}

/// 身份集合（升序）——重排场景比较集合而非先序（文档序本就变了）。
fn id_set(core: &Core) -> Vec<BlockId> {
    let mut v = ids(core);
    v.sort();
    v
}

fn type_text(core: &mut Core, path: NodePath, start: usize, end: usize, insert: &str) {
    core.apply_ops(
        vec![Op::ReplaceText {
            path,
            start,
            end,
            insert: insert.into(),
        }],
        ApplyMeta::view("type"),
    )
    .unwrap();
}

// 1. 探针：任一字段不同即 stale；全同才 fresh
#[test]
fn stamp_stale_semantics() {
    let base = FileStamp {
        modified_ms: 1000,
        size: 42,
    };
    assert!(!base.is_stale(&FileStamp {
        modified_ms: 1000,
        size: 42
    }));
    assert!(
        base.is_stale(&FileStamp {
            modified_ms: 1001,
            size: 42
        }),
        "mtime 变"
    );
    assert!(
        base.is_stale(&FileStamp {
            modified_ms: 1000,
            size: 43
        }),
        "size 变"
    );
    assert!(
        base.is_stale(&FileStamp {
            modified_ms: 1000,
            size: 0
        }),
        "截断必报"
    );
}

// 2. 决策：无变化 → Noop（即使本地脏——mtime 回声/自存不扰民）
#[test]
fn plan_noop_when_unchanged() {
    let core = Core::from_markdown("alpha\nbeta");
    let report = core.match_external_markdown("alpha\nbeta", None);
    assert!(!report.has_changes());
    assert_eq!(plan_reload(&report, false), ReloadAction::Noop);
    assert_eq!(
        plan_reload(&report, true),
        ReloadAction::Noop,
        "外部没变，本地脏也不重载"
    );
}

// 3. 决策：有变化 + 本地净 → SilentAdopt
#[test]
fn plan_silent_adopt_when_clean() {
    let core = Core::from_markdown("alpha");
    let report = core.match_external_markdown("alpha\nnew", None);
    assert!(report.has_changes());
    assert_eq!(plan_reload(&report, false), ReloadAction::SilentAdopt);
}

// 4. 决策：有变化 + 本地脏 → OfferChoice（v0 不做区域级安全合并推断）
#[test]
fn plan_offer_choice_when_dirty() {
    let core = Core::from_markdown("alpha");
    let report = core.match_external_markdown("alpha\nnew", None);
    assert_eq!(plan_reload(&report, true), ReloadAction::OfferChoice);
}

// 5. 采纳：中段编辑 → 身份全保（edited 携带旧 id，文本更新）
#[test]
fn adopt_edit_preserves_identity() {
    let mut core = Core::from_markdown("alpha\nbeta\ngamma");
    let all = ids(&core);
    let out = core
        .adopt_external("alpha\nBETA EDITED\ngamma", None)
        .unwrap();
    assert_eq!(out.report.edited.len(), 1);
    assert_eq!(out.carried_ids, 3, "三块身份全保");
    assert_eq!(out.fresh_ids, 0);
    assert_eq!(ids(&core), all, "id 集恒等");
    assert_eq!(
        out.revision_after,
        out.revision_before + 1,
        "System 事务推进 revision"
    );
    assert_eq!(
        core.tree.node(core.tree.lookup_id(all[1]).unwrap()).text,
        "BETA EDITED"
    );
}

// 6. 采纳：整体重排 → 身份全保（moved 携带旧 id）
#[test]
fn adopt_reorder_preserves_identity() {
    let mut core = Core::from_markdown("alpha\nbeta\ngamma");
    let all = ids(&core);
    let out = core.adopt_external("gamma\nalpha\nbeta", None).unwrap();
    assert_eq!(out.report.moved.len(), 3);
    assert_eq!(out.carried_ids, 3);
    assert_eq!(out.fresh_ids, 0);
    assert_eq!(
        id_set(&core),
        all,
        "身份集合恒等（文档序已变，比集合不比先序）"
    );
    // 新序内容正确
    let texts: Vec<String> = core
        .tree
        .top_level()
        .iter()
        .map(|&n| core.tree.node(n).text.to_string())
        .collect();
    assert_eq!(texts, ["gamma", "alpha", "beta"]);
}

// 7. 采纳：新增块 → 新鲜 id 严格大于全部旧 id（分配器单调）
#[test]
fn adopt_added_fresh_ids_monotonic() {
    let mut core = Core::from_markdown("alpha");
    let all = ids(&core);
    let out = core.adopt_external("alpha\nbrand new block", None).unwrap();
    assert_eq!(out.carried_ids, 1);
    assert_eq!(out.fresh_ids, 1);
    let new_ids = ids(&core);
    assert_eq!(new_ids.len(), 2);
    let added = *new_ids.iter().max().unwrap();
    assert!(
        added.0 > all.iter().map(|b| b.0).max().unwrap(),
        "新鲜 id 不撞旧身份"
    );
    assert!(!all.contains(&added));
}

// 8. 采纳：删除块 → 身份作废，报告 removed 与树一致
#[test]
fn adopt_removed_drops_identity() {
    let mut core = Core::from_markdown("alpha\nbeta\ngamma");
    let all = ids(&core);
    let out = core.adopt_external("alpha\ngamma", None).unwrap();
    assert_eq!(out.report.removed.len(), 1);
    assert_eq!(out.report.removed[0].id, all[1], "beta 身份作废");
    assert_eq!(out.carried_ids, 2);
    let now = ids(&core);
    assert!(!now.contains(&all[1]));
    assert_eq!(now, vec![all[0], all[2]]);
}

// 9. 采纳：take-external 交互史作废 + 重载后首个 View 击键独立可撤销
#[test]
fn adopt_take_external_drains_undo() {
    let mut core = Core::from_markdown("alpha\nbeta");
    type_text(&mut core, vec![0], 5, 5, "X"); // 本地未存改动 1
    type_text(&mut core, vec![1], 4, 4, "Y"); // 本地未存改动 2
    assert!(
        core.history.can_undo(),
        "本地未存交互在栈（两次击键 1s 窗内可能并条，不断言深度）"
    );

    let out = core
        .adopt_external("alpha\nEXTERNAL REPLACED BOTH", None)
        .unwrap();
    assert!(out.report.has_changes());
    assert!(!core.history.can_undo(), "被放弃文档态的 undo 全作废");
    assert!(!core.history.can_redo(), "redo 同作废");

    // 重载后的第一个 View 击键：独立新条目，可撤销且不回溯到重载前
    type_text(&mut core, vec![1], 0, 0, "Z");
    assert!(core.history.can_undo());
    assert_eq!(core.history.undo_depth(), 1, "只含重载后的击键");
    core.undo().unwrap();
    assert!(!core.history.can_undo());
}

// 10. 采纳：无变化 → 零事务幂等（revision 不动、历史不动）
#[test]
fn adopt_noop_idempotent() {
    let mut core = Core::from_markdown("alpha\nbeta");
    type_text(&mut core, vec![0], 5, 5, "X");
    let (rev0, depth0) = (core.revision(), core.history.undo_depth());
    let out = core.adopt_external("alphaX\nbeta", None).unwrap(); // 与现树逐块等价
    assert!(!out.report.has_changes());
    assert_eq!(out.revision_after, rev0, "Noop 不推进 revision");
    assert_eq!(out.carried_ids + out.fresh_ids, 0);
    assert_eq!(core.history.undo_depth(), depth0, "Noop 不清交互史");
}

// 11. 采纳：空文档边界（全删 / 全增）
#[test]
fn adopt_empty_and_clear() {
    let mut core = Core::from_markdown("one\ntwo");
    let all = ids(&core);
    let out = core.adopt_external("", None).unwrap();
    assert_eq!(out.report.removed.len(), 2);
    assert_eq!(core.tree.top_level().len(), 0);
    assert_eq!(out.carried_ids, 0);

    let out2 = core.adopt_external("three", None).unwrap();
    assert_eq!(out2.report.added.len(), 1);
    assert_eq!(out2.fresh_ids, 1);
    assert_eq!(core.tree.node(core.tree.top_level()[0]).text, "three");
    assert!(!ids(&core).contains(&all[0]), "旧身份不复活");
}

// 12. 采纳：manifest 可信 added 携带声明身份（合作写手跨端场景）
#[test]
fn adopt_manifest_trusted_added_carries_id() {
    let mut core = Core::from_markdown("alpha");
    let old_alpha = ids(&core)[0];
    let new_md = "alpha\nsynced from other device";
    let new_tree = markdown_to_tree(new_md);
    let mut fp = TreeFingerprint::new(&new_tree);
    let trusted = BlockId(9_999);
    let manifest: Vec<ManifestEntry> = vec![
        ManifestEntry {
            id: old_alpha,
            ..entry_of(&new_tree, &mut fp, 0)
        },
        ManifestEntry {
            id: trusted,
            ..entry_of(&new_tree, &mut fp, 1)
        },
    ];
    let out = core.adopt_external(new_md, Some(&manifest)).unwrap();
    assert_eq!(out.report.stats.manifest, 2, "快路径直接对位");
    assert_eq!(out.carried_ids, 2, "可信 added 携带声明 id，不 fresh");
    assert_eq!(out.fresh_ids, 0);
    let nid = core.tree.lookup_id(trusted).expect("声明身份落树");
    assert_eq!(core.tree.node(nid).text, "synced from other device");
}

fn entry_of(
    tree: &mdcore::tree::BlockTree,
    fp: &mut TreeFingerprint,
    top_index: usize,
) -> ManifestEntry {
    let f = fp.block_fingerprint(tree.top_level()[top_index]);
    ManifestEntry {
        id: BlockId(0),
        structural: f.structural,
        content: f.content,
    }
}

// 13. 采纳：嵌套子树整体替换 → 内层身份随映射携带（列表重排 + 子项不变）
#[test]
fn adopt_nested_move_inner_carried() {
    let mut core = Core::from_markdown("- p1\n  - c1\n  - c2\n- p2\n");
    let all = ids(&core);
    let out = core
        .adopt_external("- p2\n- p1\n  - c1\n  - c2\n", None)
        .unwrap();
    assert!(out.report.added.is_empty() && out.report.removed.is_empty());
    assert_eq!(out.carried_ids, all.len(), "嵌套子块身份全保");
    assert_eq!(out.fresh_ids, 0);
    assert_eq!(id_set(&core), all, "身份集合恒等（子树整体搬移后先序不同）");
}

// 14. 采纳：跨运行确定性（同输入恒同报告/计数/revision）
#[test]
fn adopt_deterministic() {
    let build = || {
        let mut core = Core::from_markdown("alpha\nbeta\ngamma");
        type_text(&mut core, vec![0], 5, 5, "!");
        let out = core
            .adopt_external("gamma!\nalpha\nbrand new", None)
            .unwrap();
        (
            out.report,
            out.carried_ids,
            out.fresh_ids,
            out.revision_after,
        )
    };
    let (r1, c1, f1, v1) = build();
    let (r2, c2, f2, v2) = build();
    assert_eq!(r1, r2, "报告（含 stats）逐字段相等");
    assert_eq!((c1, f1, v1), (c2, f2, v2));
}
