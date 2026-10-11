//! inspector — Headless Core v0 检视面演示（对象级检视能力说明）。
//!
//! 运行：`cargo run -p mdcore --example inspector`
//! 流程：加载样例 MD → 人类击键（合并窗口）→ AI 命令 → stale 演示 →
//!       undo/redo → snapshot/restore → Revision 日志重放，全程 dump。

use std::sync::Arc;

use mdcore::anchor::TextTarget;
use mdcore::debug;
use mdcore::op::Op;
use mdcore::tree::BlockKind;
use mdcore::{ApplyMeta, Core};

const SAMPLE: &str = "# Release notes\nshipping v0.1 today\n- core landed\n- wasm bound\n";

fn main() {
    let now = Arc::new(std::sync::Mutex::new(0u64));
    let clock = {
        let now = Arc::clone(&now);
        Arc::new(move || *now.lock().unwrap()) as Arc<dyn Fn() -> u64 + Send + Sync>
    };
    let mut core = Core::from_markdown_with_clock(SAMPLE, clock);

    println!("=== initial document ===\n{}", debug::dump_tree(&core.tree));

    // 人类击键：同一合并窗口内的三次编辑 = 一条历史
    for (i, s) in ["今日", "发布 ", "v0.1!"].iter().enumerate() {
        *now.lock().unwrap() = i as u64 * 200;
        core.replace_text(
            TextTarget::ByPath {
                path: vec![1usize],
                start: 0,
                end: 0,
            },
            s.to_string(),
        )
        .unwrap();
    }
    println!("=== after human typing (coalesced) ===");
    println!(
        "undo depth = {}, revision = {}",
        core.history.undo_depth(),
        core.revision()
    );
    println!("{}\n", debug::dump_tree(&core.tree));

    // AI 命令：独立条目 + 锚点寻址（拿到列表块 ID）
    let item_id = core.tree.node(core.tree.resolve(&[2usize]).unwrap()).id;
    core.apply_ops(
        vec![
            Op::ReplaceText {
                path: vec![2usize],
                start: 0,
                end: 0,
                insert: "RC".into(),
            },
            Op::SetBlockType {
                path: vec![3usize],
                kind: BlockKind::Paragraph,
            },
        ],
        ApplyMeta::ai("ai.polish"),
    )
    .unwrap();
    println!("=== after ai command (own entry) ===");
    println!(
        "undo depth = {}, revision = {}",
        core.history.undo_depth(),
        core.revision()
    );
    println!(
        "inspect: {}\n",
        debug::inspect(&core.tree, item_id).unwrap_or_default()
    );

    // stale 演示：用旧 base 提交结构命令
    let stale = core.revision() - 1;
    match core.delete_block(stale, vec![0usize]) {
        Err(e) => println!("=== stale rejected as expected ===\n{e:?}\n"),
        Ok(_) => panic!("stale command must be rejected"),
    }

    // Revision 日志重放（此点无 undo/redo，前向流 = 当前状态）
    let replayed = Core::replay_jsonl(&core.log.to_jsonl()).unwrap();
    println!("=== replayed from revision log ===");
    println!(
        "revision = {}, state identical = {}\n",
        replayed.revision(),
        debug::dump_tree(&replayed.tree) == debug::dump_tree(&core.tree)
    );

    // snapshot（在 undo 前取）
    let snap_json = core.snapshot().to_json();

    // undo 两步（AI → 人类合并条目）再 redo；日志只记前向流，undo 可由前向流推导
    core.undo().unwrap();
    core.undo().unwrap();
    println!("=== after 2x undo ===\n{}", debug::dump_tree(&core.tree));
    core.redo().unwrap();
    println!("=== after redo ===\n{}", debug::dump_tree(&core.tree));

    // restore 回 undo 前的精确状态（含 revision）
    let restored = Core::restore(mdcore::snapshot::Snapshot::from_json(&snap_json).unwrap());
    println!("=== restored from snapshot ===");
    println!(
        "revision = {}, nodes = {}",
        restored.revision(),
        restored.tree.node_count()
    );

    println!("\n=== history json (first 400 chars) ===");
    let hj = core.history_json();
    println!("{}", &hj[..hj.len().min(400)]);
}
