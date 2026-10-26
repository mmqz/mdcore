//! R23-b — 块指纹 / 结构匹配语义锁（P1-7 定谳，ADR-B6）。
//!
//! 锁定面：
//! ① 修饰性编辑：结构指纹稳定 / 内容指纹变化 / 匹配 preserved+edited；
//! ② 块重排 → moved（非 add+remove）；③ 拆块 1→2 / 并块 2→1 合理归类
//! （左保右新 / 存者=前，对齐 ADR-B1）；④ 嵌套子树整体搬移 → 子树 moved
//! 且子内 preserved；⑤ 表格单块身份（ADR-B4）；⑥ 空文档/全删边界；
//! ⑦ 非 BMP 码点语义；⑧ manifest 快路径（完整→直接对位；核验失败→整体
//! 弃用回退 b/c）两路都锁；⑨ 相似阈值定谳值 0.7 的行为双向锁定；
//! ⑩ 跨运行确定性；⑪ 1MB 性能冒烟（实测记录）；⑫ 匹配只读（零状态触碰）；
//! ⑬ 属性关键位（标题层级）翻转结构指纹但身份保持。

use mdcore::fingerprint::{
    match_trees, text_similarity, ManifestEntry, MatchParams, TreeFingerprint,
};
use mdcore::id::BlockId;
use mdcore::markdown::markdown_to_tree;
use mdcore::tree::BlockKind;
use mdcore::Core;

fn ids(core: &Core) -> Vec<BlockId> {
    // 先序全树（含嵌套子块）——嵌套场景的身份全保断言依赖完整集。
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

fn texts(core: &Core) -> Vec<String> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).text.to_string())
        .collect()
}

// 1. 修饰性编辑：结构指纹稳定、内容指纹变化、匹配 edited（身份保持）
#[test]
fn decorative_edit_keeps_structure_changes_content() {
    let old = markdown_to_tree("hello world");
    let new = markdown_to_tree("hello brave world");
    let mut fo = TreeFingerprint::new(&old);
    let mut fn_ = TreeFingerprint::new(&new);
    let fo0 = fo.block_fingerprint(old.top_level()[0]);
    let fn0 = fn_.block_fingerprint(new.top_level()[0]);
    assert_eq!(
        fo0.structural, fn0.structural,
        "同型叶子段落结构指纹必须稳定"
    );
    assert_ne!(fo0.content, fn0.content, "改字必须翻内容指纹");

    let core = Core::from_markdown("hello world");
    let report = core.match_external_markdown("hello brave world", None);
    assert!(report.preserved.is_empty());
    assert_eq!(report.edited.len(), 1, "同位小改 = edited");
    assert_eq!(report.edited[0].id, ids(&core)[0], "身份保持");
    assert_eq!(report.edited[0].old_path, vec![0]);
    assert_eq!(report.edited[0].new_path, vec![0]);
    assert!(report.edited[0].content_changed);
    assert!(report.added.is_empty() && report.removed.is_empty());
    assert_eq!(report.stats.similar, 1, "走相似兜底路径");
}

// 2. 块重排 → moved（非 add+remove）
#[test]
fn block_reorder_reports_moved() {
    let core = Core::from_markdown("a\nb\nc\nd");
    let before = ids(&core);
    let report = core.match_external_markdown("d\na\nb\nc", None);
    assert!(
        report.added.is_empty() && report.removed.is_empty(),
        "重排不得误报增删"
    );
    assert_eq!(report.moved.len(), 4);
    assert_eq!(report.kept_ids(), before, "四块身份全保");
    for m in &report.moved {
        assert!(!m.content_changed);
    }
    // 路径对照：d 3→0，a 0→1，b 1→2，c 2→3（按旧路径排序）。
    let pairs: Vec<(usize, usize)> = report
        .moved
        .iter()
        .map(|m| (m.old_path[0], m.new_path[0]))
        .collect();
    assert_eq!(pairs, vec![(0, 1), (1, 2), (2, 3), (3, 0)]);
    assert_eq!(report.stats.exact, 4, "中段精确指纹配对");
}

// 3. 拆块 1→2：左保右新（ADR-B1 split 语义）
#[test]
fn split_block_one_to_two() {
    let core = Core::from_markdown("hello world");
    let old_id = ids(&core)[0];
    let report = core.match_external_markdown("hello\nworld", None);
    assert_eq!(report.edited.len(), 1, "前半段继承旧身份");
    assert_eq!(report.edited[0].id, old_id);
    assert_eq!(report.edited[0].new_path, vec![0]);
    assert!(report.edited[0].content_changed);
    assert_eq!(report.added.len(), 1, "后半段是新块");
    assert_eq!(report.added[0].path, vec![1]);
    assert_eq!(report.added[0].subtree_size, 1);
    assert_eq!(report.added[0].kind, BlockKind::Paragraph);
    assert!(report.removed.is_empty() && report.moved.is_empty());
}

// 4. 并块 2→1：存者=前（ADR-B1 merge 语义）
#[test]
fn merge_blocks_two_to_one() {
    let core = Core::from_markdown("hello\nworld");
    let all = ids(&core);
    let report = core.match_external_markdown("hello world", None);
    assert_eq!(report.edited.len(), 1, "前块存续（合并后内容）");
    assert_eq!(report.edited[0].id, all[0], "survivor = 前");
    assert!(report.edited[0].content_changed);
    assert_eq!(report.removed.len(), 1);
    assert_eq!(report.removed[0].id, all[1], "后块身份作废");
    assert!(report.added.is_empty() && report.moved.is_empty());
}

// 5. 嵌套列表子树整体搬移 → 子树 moved 且子内 preserved
#[test]
fn nested_subtree_move_inner_preserved() {
    let before = "- p1\n  - c1\n  - c2\n- p2\n";
    let after = "- p2\n- p1\n  - c1\n  - c2\n";
    let core = Core::from_markdown(before);
    let report = core.match_external_markdown(after, None);
    assert!(report.added.is_empty() && report.removed.is_empty());
    assert_eq!(report.moved.len(), 2, "两个顶层项都换位");
    assert_eq!(report.preserved.len(), 2, "子项相对位不变 → preserved");
    for m in &report.preserved {
        assert_eq!(m.old_path.len(), 2);
        assert_eq!(m.new_path.len(), 2);
        assert_eq!(m.old_path.last(), m.new_path.last(), "相对位一致");
        assert!(!m.old_path.eq(&m.new_path), "绝对路径随祖先变化");
    }
    assert_eq!(report.kept_ids(), ids(&core), "四块身份全保");
    // to_markdown 往返确认新树形状正确解析
    let reparsed = markdown_to_tree(after);
    assert_eq!(reparsed.top_level().len(), 2);
    assert_eq!(
        reparsed.node(reparsed.top_level()[1]).children.len(),
        2,
        "搬移后子项仍挂在 p1 下"
    );
}

// 6. 表格单块身份（ADR-B4）：单元格改动 = edited，身份保持
#[test]
fn table_single_block_fingerprint_and_match() {
    let src = "| a | b |\n| c | d |";
    let edited = "| a | B |\n| c | d |";
    let old = markdown_to_tree(src);
    let new = markdown_to_tree(edited);
    assert_eq!(old.top_level().len(), 1, "连续表格行 = 单块");
    assert_eq!(old.node(old.top_level()[0]).kind, BlockKind::Table);
    let mut fo = TreeFingerprint::new(&old);
    let mut fn_ = TreeFingerprint::new(&new);
    let fo0 = fo.block_fingerprint(old.top_level()[0]);
    let fn0 = fn_.block_fingerprint(new.top_level()[0]);
    assert_eq!(fo0.structural, fn0.structural, "表格结构指纹稳定");
    assert_ne!(fo0.content, fn0.content, "单元格改动翻内容指纹");

    let core = Core::from_markdown(src);
    let report = core.match_external_markdown(edited, None);
    assert_eq!(report.edited.len(), 1);
    assert_eq!(report.edited[0].id, ids(&core)[0]);
    // 表格整块搬移 → moved
    let core2 = Core::from_markdown("head\n| a | b |\n| c | d |");
    let r2 = core2.match_external_markdown("| a | b |\n| c | d |\nhead", None);
    assert_eq!(r2.moved.len(), 2, "head 与表格互换 = 双 moved");
    assert_eq!(r2.moved[0].id, ids(&core2)[0]);
}

// 7. 空文档 / 全删 / 双空边界
#[test]
fn empty_document_and_full_delete() {
    let empty = Core::new();
    let r = empty.match_external_markdown("a\nb", None);
    assert_eq!(r.added.len(), 2);
    assert!(r.removed.is_empty() && r.preserved.is_empty());

    let core = Core::from_markdown("a\nb");
    let all = ids(&core);
    let r = core.match_external_markdown("", None);
    assert_eq!(r.removed.len(), 2);
    assert_eq!(r.removed[0].subtree_size, 1);
    assert_eq!(r.removed.iter().map(|x| x.id).collect::<Vec<_>>(), all);
    assert!(
        r.added.is_empty() && r.kept_ids().is_empty(),
        "全删 = 身份全灭"
    );

    let both = Core::new();
    let r = both.match_external_markdown("", None);
    assert!(r.is_trivial(), "双空 = 平凡报告");
}

// 8. 非 BMP（🎉）：码点语义指纹 + 匹配
#[test]
fn non_bmp_fingerprint_is_codepoint_semantics() {
    let t1 = markdown_to_tree("🎉 party");
    let t2 = markdown_to_tree("🎉 party");
    let t3 = markdown_to_tree("🚀 party");
    let t4 = markdown_to_tree("🎉🎉");
    let mut f1 = TreeFingerprint::new(&t1);
    let mut f2 = TreeFingerprint::new(&t2);
    let mut f3 = TreeFingerprint::new(&t3);
    let mut f4 = TreeFingerprint::new(&t4);
    let a = f1.block_fingerprint(t1.top_level()[0]);
    let b = f2.block_fingerprint(t2.top_level()[0]);
    let c = f3.block_fingerprint(t3.top_level()[0]);
    let d = f4.block_fingerprint(t4.top_level()[0]);
    assert_eq!(a, b, "同输入恒同指纹（跨解析会话）");
    assert_ne!(a.content, c.content, "不同增补平面字符必须不同指纹");
    assert_ne!(
        a.content, d.content,
        "单 🎉 与双 🎉 不同（码点计数进入哈希）"
    );
    assert_eq!(a.structural, d.structural, "同为叶子段落，结构指纹相同");

    let core = Core::from_markdown("🎉 hello");
    let report = core.match_external_markdown("🎉 hello 👋", None);
    assert_eq!(report.edited.len(), 1, "非 BMP 内容编辑照常 edited");
    assert_eq!(report.edited[0].id, ids(&core)[0]);
}

// 9. manifest 快路径：完整清单 → 直接对位（重写块身份保住）
#[test]
fn manifest_fast_path_preserves_identity_through_rewrite() {
    let core = Core::from_markdown("alpha\nbeta");
    let all = ids(&core);
    let new_md = "alpha\nGAMMA REWRITTEN ENTIRELY";

    // 无 manifest 兜底：重写块相似度过低 → removed + added
    let fallback = core.match_external_markdown(new_md, None);
    assert_eq!(fallback.removed.len(), 1);
    assert_eq!(fallback.removed[0].id, all[1]);
    assert_eq!(fallback.added.len(), 1);
    assert_eq!(fallback.stats.manifest, 0);

    // 构造与新树逐位一致的 manifest（Conforming 写手场景）：未动块用真身份，
    // 重写块声明沿用 beta 的 id（其结构指纹同为叶子段落）。
    let new_tree = markdown_to_tree(new_md);
    let mut fnew = TreeFingerprint::new(&new_tree);
    let manifest: Vec<ManifestEntry> = new_tree
        .top_level()
        .iter()
        .zip(all.iter())
        .map(|(&n, &id)| {
            let f = fnew.block_fingerprint(n);
            ManifestEntry {
                id,
                structural: f.structural,
                content: f.content,
            }
        })
        .collect();
    let fast = core.match_external_markdown(new_md, Some(&manifest));
    assert_eq!(fast.stats.manifest, 2, "快路径直接对位");
    assert_eq!(fast.edited.len(), 1, "重写块 = edited 而非 add+remove");
    assert_eq!(fast.edited[0].id, all[1], "身份由 manifest 保住");
    assert_eq!(fast.preserved.len(), 1);
    assert!(fast.added.is_empty() && fast.removed.is_empty());
    assert_eq!(fast.kept_ids(), all);
}

// 10. manifest 核验失败（过期/异源）→ 全有全无弃用 → b/c 级兜底
#[test]
fn manifest_verification_failure_falls_back() {
    let core = Core::from_markdown("alpha\nbeta");
    let all = ids(&core);
    let new_md = "alpha\nGAMMA REWRITTEN ENTIRELY";

    // 长度不符 → 弃用
    let short: Vec<ManifestEntry> = Vec::new();
    let r = core.match_external_markdown(new_md, Some(&short));
    assert_eq!(r.stats.manifest, 0);
    assert_eq!(r.removed.len(), 1, "回退后重写块照旧 remove+add");

    // 单条指纹不符 → 整体弃用
    let new_tree = markdown_to_tree(new_md);
    let mut fnew = TreeFingerprint::new(&new_tree);
    let mut manifest: Vec<ManifestEntry> = new_tree
        .top_level()
        .iter()
        .zip(all.iter())
        .map(|(&n, &id)| {
            let f = fnew.block_fingerprint(n);
            ManifestEntry {
                id,
                structural: f.structural,
                content: f.content,
            }
        })
        .collect();
    manifest[1].content = manifest[1].content.rotate_left(13);
    let r = core.match_external_markdown(new_md, Some(&manifest));
    assert_eq!(r.stats.manifest, 0, "一位不符 = 整体弃用");
    assert_eq!(r.removed.len(), 1);
    assert_eq!(r.added.len(), 1);
}

// 11. 相似阈值 0.7 定谳：行为双向锁定
#[test]
fn similarity_threshold_semantics_locked() {
    // 相似不足（0.25）→ 默认阈值下不配对
    let core = Core::from_markdown("beta");
    let r = core.match_external_markdown("gamma", None);
    assert_eq!(r.removed.len(), 1);
    assert_eq!(r.added.len(), 1);
    assert!(r.edited.is_empty());
    assert!(text_similarity("beta", "gamma") < 0.3);

    // 阈值放宽到 0.2 → 同一对照配对成功（edited）
    let old_tree = markdown_to_tree("beta");
    let new_tree = markdown_to_tree("gamma");
    let loose = match_trees(
        &old_tree,
        &new_tree,
        None,
        &MatchParams {
            similarity_threshold: 0.2,
            edit_window: 8,
        },
    );
    assert_eq!(loose.edited.len(), 1, "0.25 ≥ 0.2 → 配对");
    // 阈值收到 0.99：高相似对照（hello world → hello brave world，containment 1.0）仍配对
    let strict = match_trees(
        &markdown_to_tree("hello world"),
        &markdown_to_tree("hello brave world"),
        None,
        &MatchParams {
            similarity_threshold: 0.99,
            edit_window: 8,
        },
    );
    assert_eq!(
        strict.edited.len(),
        1,
        "containment 满分对照在严阈值下仍配对"
    );
    // 窗宽锁：同型相似但相对位偏差超窗 → 不配对
    let far = match_trees(
        &markdown_to_tree(
            "alpha\nkeep0\nkeep1\nkeep2\nkeep3\nkeep4\nkeep5\nkeep6\nkeep7\nkeep8\nomega",
        ),
        &markdown_to_tree("alpha\nins0\nins1\nins2\nins3\nins4\nins5\nins6\nins7\nins8\nomega"),
        None,
        &MatchParams {
            similarity_threshold: 0.7,
            edit_window: 2,
        },
    );
    assert!(
        far.removed.len() == 9
            && far.added.len() == 9
            && far.edited.is_empty()
            && far.moved.is_empty()
            && far.stats.similar == 0,
        "中段 10×10 相异块、窗宽 2：无法对位的对照不硬凑（9 个未配对根各自 reported）"
    );
    assert!(far.stats.anchor >= 2, "两端 alpha/omega 仍被结构锚走查捕获");
}

// 12. 跨运行确定性：同输入恒同报告（含指纹值与 stats）
#[test]
fn determinism_across_runs() {
    let build = || {
        let core = Core::from_markdown(
            "head\n- item one\n  - sub\n- item two\n| a | b |\n```\ncode\n```\ntail",
        );
        core.match_external_markdown(
            "tail\n| a | b |\n```\ncode\n```\n- item two\n  - sub\n- item one\nhead",
            None,
        )
    };
    let r1 = build();
    let r2 = build();
    assert_eq!(r1, r2, "报告（含 stats）必须逐字段相等");
    assert!(!r1.moved.is_empty());
    // 序列化形状也确定
    assert_eq!(
        serde_json::to_string(&r1).unwrap(),
        serde_json::to_string(&r2).unwrap()
    );
    // 指纹值确定
    let t = markdown_to_tree("🎉 a\nb");
    let mut f1 = TreeFingerprint::new(&t);
    let mut f2 = TreeFingerprint::new(&t);
    for &n in t.top_level() {
        assert_eq!(f1.block_fingerprint(n), f2.block_fingerprint(n));
    }
}

// 13. 1MB 性能冒烟：<200ms 量级（实测数字 eprintln 记录）
#[test]
fn perf_1mb_smoke() {
    let line = |i: usize, tag: &str| {
        if tag.is_empty() {
            format!("para {i:05}: {}", "x".repeat(150))
        } else {
            format!("para {i:05}: {tag} {}", "x".repeat(130))
        }
    };
    let n = 6300; // ≈ 163 B × 6300 ≈ 1.03 MB
    let doc: String = (0..n).map(|i| line(i, "")).collect::<Vec<_>>().join("\n");
    assert!(
        doc.len() > 900_000 && doc.len() < 1_400_000,
        "规模口径 ≈1MB: {} B",
        doc.len()
    );
    let mut perturbed = doc.clone();
    let target = line(3000, "");
    let replaced = line(3000, "EDITED");
    // ⚠️ str::replace 不可变（返回新 String）——必须回赋，否则 perturbed == doc
    perturbed = perturbed.replace(&target, &replaced);
    assert!(perturbed.contains("EDITED"), "扰动必须真实落进文档");
    assert_ne!(perturbed, doc, "扰动后文档必须与原文不同");
    let _ = perturbed.len();

    let core = Core::from_markdown(&doc);

    // 解析移出计时：本门锁的是**匹配算法**（P1-7 交付物）；1MB 解析性能已由
    // G2 渐进 TTI 门覆盖，debug 构建的解析耗时不应绑架算法门（实测 ~200ms/1MB dev）。
    let t_p = std::time::Instant::now();
    let new_same = markdown_to_tree(&doc);
    let new_pert = markdown_to_tree(&perturbed);
    let parse_ms = t_p.elapsed().as_millis();

    // (a) 全同文档：全 preserved
    let t0 = std::time::Instant::now();
    let r = match_trees(&core.tree, &new_same, None, &MatchParams::default());
    let identical_ms = t0.elapsed().as_millis();
    assert_eq!(r.preserved.len(), n, "1MB 全同 → 全 preserved");
    assert!(r.is_trivial() || (r.added.is_empty() && r.removed.is_empty() && r.edited.is_empty()));

    // (b) 单段改写：1 edited，其余 preserved
    let t1 = std::time::Instant::now();
    let r2 = match_trees(&core.tree, &new_pert, None, &MatchParams::default());
    let edited_ms = t1.elapsed().as_millis();
    assert_eq!(r2.edited.len(), 1);
    assert_eq!(r2.preserved.len(), n - 1);
    assert!(r2.added.is_empty() && r2.removed.is_empty());

    eprintln!(
        "R23-b perf (debug build): parse 1MB = {parse_ms} ms; match-only identical = {identical_ms} ms; match-only single-edit = {edited_ms} ms"
    );
    // debug 构建常数大（release 实测 ≈1/15）；门职责 = 抓 O(n²) 级退化而非绝对毫秒。
    //
    // R25-a（相对判据）：原绝对 400ms 门在低频 CPU / 机械盘机器上会假红（实测
    // 428ms 通过功能断言后挂在性能门）。改为「相对 parse 基准的倍数」判据——
    // 1MB 文档解析本身 O(n)，匹配是同类量级工作量；若匹配退化成 O(n²)，
    // 其绝对值会远超 parse 的倍数上限。同机同构建下比值稳定，跨机器可比。
    //
    // 判据：match-only ≤ parse × 4 且 ≤ 3000ms（后者为极端慢机的硬兜底，
    // 只防挂死，不参与退化判定）。
    let parse_floor = parse_ms.max(1);
    let ratio_cap = 4u128;
    assert!(
        identical_ms <= (parse_floor * ratio_cap).max(1) || identical_ms <= 3000,
        "identical {identical_ms} ms 超 parse({parse_ms} ms) × {ratio_cap} 且 > 3000ms —— 匹配退化为 O(n²)？"
    );
    assert!(
        edited_ms <= (parse_floor * ratio_cap).max(1) || edited_ms <= 3000,
        "single-edit {edited_ms} ms 超 parse({parse_ms} ms) × {ratio_cap} 且 > 3000ms —— 匹配退化为 O(n²)？"
    );
}

// 14. 匹配只读：revision / 事件 / 树全部零触碰
#[test]
fn matching_is_readonly() {
    let mut core = Core::from_markdown("one\ntwo");
    let r0 = core.revision();
    let before_texts = texts(&core);
    let before_ids = ids(&core);

    let _ = core.match_external_markdown("one\ntwo\nthree", None);
    let _ = core.match_external_markdown("x", None);
    let manifest = core.fingerprint_manifest();
    let _ = core.match_external_markdown("one\ntwo", Some(&manifest));

    assert_eq!(core.revision(), r0, "匹配不得推进 revision");
    assert!(core.take_events().is_empty(), "匹配不得产生事件");
    assert!(!core.history.can_undo(), "匹配不得入 undo 栈");
    assert_eq!(texts(&core), before_texts);
    assert_eq!(ids(&core), before_ids);
    assert_eq!(manifest.len(), 2, "清单 = 先序块数");
}

// 15. 属性关键位（标题层级）翻转结构指纹但身份保持
#[test]
fn heading_level_change_flips_structural_keeps_identity() {
    let old = markdown_to_tree("## Title");
    let new = markdown_to_tree("### Title");
    let mut fo = TreeFingerprint::new(&old);
    let mut fn_ = TreeFingerprint::new(&new);
    let fo0 = fo.block_fingerprint(old.top_level()[0]);
    let fn0 = fn_.block_fingerprint(new.top_level()[0]);
    assert_ne!(
        fo0.structural, fn0.structural,
        "层级在属性关键位 → 结构指纹必变"
    );
    assert_eq!(fo0.content, fn0.content, "标题文本未动 → 内容指纹不变");

    let core = Core::from_markdown("## Title");
    let report = core.match_external_markdown("### Title", None);
    assert_eq!(
        report.edited.len(),
        1,
        "层级改动 = edited（attrs 进 payload 比较）"
    );
    assert_eq!(report.edited[0].id, ids(&core)[0]);
    assert!(report.edited[0].content_changed);
    assert_eq!(report.stats.similar, 1);
}
