//! R14 — 第一阶段硬 Gate 量化收口 · 核侧半边（主档 §15.3 + §28 P0-3）。
//!
//! 职责划分：§15.3 Gate 3（snapshot/restore 完整性）在核侧**判定**；
//! Gate 1/2 的浏览器半边见 web_smoke.py G1/G2（gates.md 汇总判定表）。
//! 核侧时间全部 println! 打点供 gates.md 引用，不做脆弱的时间硬断言
//! （策略与 r11_stress 一致：功能性断言 + 计量留痕）。

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use mdcore::debug;
use mdcore::history::Source;
use mdcore::markdown::{markdown_to_tree, tree_to_markdown};
use mdcore::op::Op;
use mdcore::{ApplyMeta, Core};

/// 计时测试串行化：并行 build_big 会互相污染内存带宽，p95 不可信
/// （实测：并行时单 op p95 33ms，串行后 6ms）。
fn gate_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn para(i: usize) -> String {
    format!("paragraph body {i} with filler words to make blocks non-trivial\n")
}

/// ~20k 块 ≈ 1.2MB 文档（与 r11_stress 同源 fixture）。
fn build_big() -> Core {
    let sections: Vec<String> = (0..20_000).map(para).collect();
    Core::from_markdown(&sections.concat())
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() as f64 - 1.0) * q) as usize]
}

fn top_lens(core: &Core) -> Vec<usize> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).text.chars().count())
        .collect()
}

// ---------------------------------------------------------------------------
// Gate 3 — Document State 完整 snapshot / restore（判定：恒等 + 可继续编辑）
// ---------------------------------------------------------------------------

#[test]
fn gate3_snapshot_restore_identity_at_1mb() {
    let _g = gate_lock();
    let core = build_big();
    let md_live = tree_to_markdown(&core.tree);

    let t0 = Instant::now();
    let snap = core.snapshot().to_json();
    let snap_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    let restored = Core::restore(mdcore::snapshot::Snapshot::from_json(&snap).unwrap());
    let restore_ms = t1.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(
        debug::dump_tree(&restored.tree),
        debug::dump_tree(&core.tree),
        "逐块恒等（含身份）"
    );
    assert_eq!(
        debug::blocks_json(&restored.tree),
        debug::blocks_json(&core.tree),
        "块树 JSON 逐字节恒等"
    );
    assert_eq!(tree_to_markdown(&restored.tree), md_live, "MD 序列化恒等");
    assert_eq!(restored.revision(), core.revision(), "revision 复原");
    println!(
        "[gate3] 1MB snapshot {snap_ms:.1}ms / restore {restore_ms:.1}ms / blocks {}",
        core.tree.top_level().len()
    );
}

#[test]
fn gate3_restore_then_editing_and_undo_continue() {
    let _g = gate_lock();
    let mut core = build_big();
    // 固定时钟：pre-save 编辑 last_at_ms=1000，确保与重开后编辑隔窗（确定性分组）。
    core.set_clock(Arc::new(|| 1_000));
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 0,
            end: 9,
            insert: "EDITED".into(),
        }],
        ApplyMeta {
            source: Source::View,
            command: Some("pre-save".into()),
        },
    )
    .unwrap();
    let snap = core.snapshot().to_json();
    let hj = core.history_json();

    let mut reopened = Core::restore(mdcore::snapshot::Snapshot::from_json(&snap).unwrap());
    reopened.set_history_json(&hj).unwrap();
    // 重开后时钟远隔存档前（>1s 窗）→ 新历史条目，不与存档前击键合并。
    reopened.set_clock(Arc::new(|| 5_000_000));
    let depth = reopened.history.undo_depth();
    assert!(depth >= 1, "撤销栈随 history_json 复原");

    // 重开后继续编辑 + undo 链跨存档边界工作
    reopened
        .apply_ops(
            vec![Op::ReplaceText {
                path: vec![1],
                start: 0,
                end: 9,
                insert: "AFTER".into(),
            }],
            ApplyMeta {
                source: Source::View,
                command: Some("post-restore".into()),
            },
        )
        .unwrap();
    assert_eq!(reopened.history.undo_depth(), depth + 1);
    reopened.undo().unwrap();
    let _lens = top_lens(&reopened);
    assert_eq!(
        &reopened
            .tree
            .node(reopened.tree.resolve(&[1]).unwrap())
            .text
            .to_string()[..9],
        "paragraph",
        "post-restore 编辑被 undo 撤净"
    );
    reopened.undo().unwrap();
    assert_eq!(
        &reopened
            .tree
            .node(reopened.tree.resolve(&[0]).unwrap())
            .text
            .to_string()[..9],
        "paragraph",
        "跨存档边界的 pre-save 编辑也被撤净"
    );
    println!("[gate3] restore+history 复原 undo_depth={depth}，跨档 undo 链可用");
}

// ---------------------------------------------------------------------------
// Gate 2 核内份额 — 单 op / 批量 / undo 耗时打点（浏览器 G1 出总额与归因）
// ---------------------------------------------------------------------------

#[test]
fn gate2_core_op_latency_at_1mb() {
    let _g = gate_lock();
    let mut core = build_big();
    let lens = top_lens(&core);

    // 100 次单块 ReplaceText（逐次 apply_ops），p95 记录
    let mut singles = Vec::new();
    for k in 0..100 {
        let t = Instant::now();
        core.apply_ops(
            vec![Op::ReplaceText {
                path: vec![k * 11 % 20_000],
                start: 0,
                end: lens[k * 11 % 20_000].min(4),
                insert: format!("E{k:03}").into(),
            }],
            ApplyMeta {
                source: Source::View,
                command: Some("g2-typing".into()),
            },
        )
        .unwrap();
        singles.push(t.elapsed().as_secs_f64() * 1000.0);
    }

    // 100-op 批量事务 + undo（R11 复测口径）
    let lens = top_lens(&core);
    let ops: Vec<Op> = (0..100)
        .map(|k| Op::ReplaceText {
            path: vec![k * 3],
            start: 0,
            end: lens[k * 3].min(4),
            insert: format!("B{k:03}").into(),
        })
        .collect();
    let t = Instant::now();
    core.apply_ops(
        ops,
        ApplyMeta {
            source: Source::Ai,
            command: Some("g2-batch".into()),
        },
    )
    .unwrap();
    let batch_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    core.undo().unwrap();
    let undo_ms = t.elapsed().as_secs_f64() * 1000.0;

    println!(
        "[gate2][core] single-op p50 {:.3}ms / p95 {:.3}ms / max {:.3}ms / 100-op batch {batch_ms:.1}ms / undo(100-op entry) {undo_ms:.1}ms",
        pct(&mut singles, 0.5),
        pct(&mut singles, 0.95),
        singles.last().copied().unwrap_or(0.0)
    );
    assert_eq!(core.tree.top_level().len(), 20_000, "压测期间块数稳定");

    // 归因探针：小文档（10 块）同路径 100 次单 op —— 若 p95 与 1MB 级同量，
    // 开销是每调用固定成本；若显著更低，则随文档/日志规模增长。
    let small_src: String = (0..10).map(para).collect();
    let mut small = Core::from_markdown(&small_src);
    let mut smalls = Vec::new();
    for k in 0..100 {
        let t = Instant::now();
        small
            .apply_ops(
                vec![Op::ReplaceText {
                    path: vec![k % 10],
                    start: 0,
                    end: 4,
                    insert: format!("S{k:03}").into(),
                }],
                ApplyMeta {
                    source: Source::View,
                    command: Some("g2-small".into()),
                },
            )
            .unwrap();
        smalls.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "[gate2][core] small-doc(10 blocks) single-op p50 {:.3}ms / p95 {:.3}ms —— 量级对照",
        pct(&mut smalls, 0.5),
        pct(&mut smalls, 0.95)
    );
}

// ---------------------------------------------------------------------------
// Gate 1 核内份额 — genesis 解析与日志重放耗时（TTI 的核侧构成）
// ---------------------------------------------------------------------------

#[test]
fn gate1_core_ingest_and_replay_at_1mb() {
    let _g = gate_lock();
    let sections: Vec<String> = (0..20_000).map(para).collect();
    let src = sections.concat();
    assert!(
        src.len() > 1_000_000,
        "fixture 应 ≥1MB: {} bytes",
        src.len()
    );

    let t0 = Instant::now();
    let mut core = Core::from_markdown(&src);
    let genesis_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // 追加一轮编辑让日志携带非 genesis 记录
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 0,
            end: 9,
            insert: "EDITED".into(),
        }],
        ApplyMeta {
            source: Source::View,
            command: Some("g1".into()),
        },
    )
    .unwrap();
    let jsonl = core.log.to_jsonl();
    let live_md = tree_to_markdown(&core.tree);

    let t1 = Instant::now();
    let replayed = Core::replay_jsonl(&jsonl).unwrap();
    let replay_ms = t1.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(tree_to_markdown(&replayed.tree), live_md, "重放=实时状态");
    println!(
        "[gate1][core] from_markdown(1.2MB) {genesis_ms:.1}ms / log {} records / replay {replay_ms:.1}ms",
        jsonl.lines().count()
    );
}

// ---------------------------------------------------------------------------
// Gate 3 补强 — 从 markdown_to_tree 到 snapshot 的 1MB 全链路（内存形态检查）
// ---------------------------------------------------------------------------

#[test]
fn gate3_tree_build_and_snapshot_chain() {
    let _g = gate_lock();
    let sections: Vec<String> = (0..20_000).map(para).collect();
    let src = sections.concat();
    let t0 = Instant::now();
    let tree = markdown_to_tree(&src);
    let build_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = Instant::now();
    let snap = mdcore::snapshot::Snapshot::capture(&tree, 0).to_json();
    let snap_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let restored = Core::restore(mdcore::snapshot::Snapshot::from_json(&snap).unwrap());
    assert_eq!(
        debug::blocks_json(&restored.tree),
        debug::blocks_json(&tree)
    );
    println!("[gate3] markdown_to_tree {build_ms:.1}ms / snapshot-to-json {snap_ms:.1}ms / snap bytes {}", snap.len());
}
