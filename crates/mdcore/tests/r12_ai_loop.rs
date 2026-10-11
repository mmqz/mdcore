//! R12 — Human + AI 闭环最小实现（主档 §阶段 R10，对应 §10 Command Boundary /
//! §11 Revision / §22 十条不变量）。
//!
//! 验证闭环：`Human command → Document State ← AI command`，四条验证点：
//! 1. AI read current state → `ai_reads_human_edited_state_as_decision_input`
//! 2. AI 生成 deterministic cmds → `scripted_ai_plan_is_deterministic_and_primitive_only`
//!    + `ai_fills_empty_paragraph_via_command_boundary`
//! 3. Core execute + revision → 同上 + `ai_text_command_bumps_own_revision_never_coalesces`
//!    + `ai_batch_is_atomic_on_bad_op`
//! 4. undo / replay / stale → `ai_batch_undo_reverts_whole_batch_not_human_edit`
//!    + `ai_stale_transaction_rejected_then_retries_with_fresh_base`
//!    + `ai_inclusive_log_replays_to_identical_state`
//!    + `human_and_ai_alternate_on_one_document_state`
//!    + `ai_entries_survive_save_open_and_undo_whole_batch`
//!
//! AI 替身 `ScriptedAi`：headless、确定性——只经公开检视面（`blocks_json`，
//! 辅以 `take_events` 事件流）读状态，只产出 §10.2 原子 op 序列；提交只走
//! `commit_transaction_as`（base_revision 硬门禁 + Source::Ai 溯源）或
//! `apply_ops` 管道，无任何旁路。自然语言永不进核（主档 §10.2 红线）。

use std::sync::Arc;

use mdcore::debug;
use mdcore::history::Source;
use mdcore::op::Op;
use mdcore::tree::{BlockKind, NodePath};
use mdcore::{ApplyMeta, Core, Reject};

fn clock_of(cell: &Arc<std::sync::Mutex<u64>>) -> Arc<dyn Fn() -> u64 + Send + Sync> {
    let cell = Arc::clone(cell);
    Arc::new(move || *cell.lock().unwrap())
}

fn text_at(core: &Core, path: &[usize]) -> String {
    core.tree
        .node(core.tree.resolve(path).unwrap())
        .text
        .to_string()
}

fn kind_at(core: &Core, path: &[usize]) -> BlockKind {
    core.tree.node(core.tree.resolve(path).unwrap()).kind
}

fn id_at(core: &Core, path: &[usize]) -> u64 {
    core.tree.node(core.tree.resolve(path).unwrap()).id.0
}

fn md(core: &Core) -> String {
    mdcore::markdown::tree_to_markdown(&core.tree)
}

// ---------------------------------------------------------------------------
// AI 替身：observe（只读）→ plan（确定性规则）→ submit（Command Boundary）
// ---------------------------------------------------------------------------

/// AI 从 `blocks_json` 读到的单个块：平铺字段 + stack-by-depth 复原的
/// NodePath（debug.rs 契约：先序平铺同序可复原层级）。
#[derive(Clone, Debug, PartialEq)]
struct ObservedBlock {
    id: u64,
    kind: String,
    depth: usize,
    text: String,
    path: NodePath,
}

/// Headless AI 代理测试替身。状态只有一份：`observed`（决策输入）。
struct ScriptedAi {
    observed: Vec<ObservedBlock>,
}

impl ScriptedAi {
    fn new() -> Self {
        Self {
            observed: Vec::new(),
        }
    }

    /// 验证点 1 的读通道：blocks_json（机器可读块树平铺）。AI 不 touch 树，
    /// 不绕过检视面——这正是 AI 的"看"。
    fn observe(&mut self, core: &Core) {
        let json = debug::blocks_json(&core.tree);
        let raw: serde_json::Value =
            serde_json::from_str(&json).expect("blocks_json is valid JSON");
        let blocks = raw.as_array().expect("blocks_json is an array");
        let mut observed = Vec::new();
        // stack = 当前祖先路径；counters[d] = depth-d 层已用到的下一个 index。
        // 先序 + depth → 路径：子块挂在刚访问块的路径下（最后一个赋值者）。
        let mut stack: Vec<usize> = Vec::new();
        let mut counters: Vec<usize> = Vec::new();
        for b in blocks {
            let depth = b["depth"].as_u64().expect("depth field") as usize;
            counters.truncate(depth + 1);
            while counters.len() <= depth {
                counters.push(0);
            }
            stack.truncate(depth);
            let mut path: NodePath = stack.clone();
            path.push(counters[depth]);
            counters[depth] += 1;
            stack = path.clone();
            observed.push(ObservedBlock {
                id: b["id"].as_u64().expect("id field"),
                kind: b["type"].as_str().expect("type field").to_string(),
                depth,
                text: b["text"].as_str().expect("text field").to_string(),
                path,
            });
        }
        self.observed = observed;
    }

    /// 确定性规则（observed 的纯函数）："摘要助手"——
    ///   1) 第一个空段落 → 填入摘要行并提级为 heading；
    ///   2) 文末追加一个 AI summary 块。
    ///
    /// 输出全部是 §10.2 原子 op（split/merge/transaction 属命令层组合，
    /// op 层即这 7 种原子）；同一状态永远产出同一 op 序列。
    fn plan_summarize(&self) -> Vec<Op> {
        let n_top = self.observed.iter().filter(|b| b.depth == 0).count();
        let first = self
            .observed
            .iter()
            .find(|b| b.depth == 0)
            .map(|b| b.text.clone())
            .unwrap_or_default();
        let summary = format!("AI summary: {n_top} top-level blocks; first: {first}");

        let mut ops = Vec::new();
        if let Some(empty) = self
            .observed
            .iter()
            .find(|b| b.depth == 0 && b.kind == "paragraph" && b.text.is_empty())
        {
            ops.push(Op::ReplaceText {
                path: empty.path.clone(),
                start: 0,
                end: 0,
                insert: summary.clone().into(),
            });
            ops.push(Op::SetBlockType {
                path: empty.path.clone(),
                kind: BlockKind::Heading,
            });
        }
        ops.push(Op::InsertBlock {
            parent: vec![],
            index: n_top,
            kind: BlockKind::Paragraph,
            attrs: Default::default(),
            text: summary.into(),
            id: None,
        });
        ops
    }

    /// 完整 AI 轮：observe → plan → 提交。observe 时的 revision 作为
    /// base_revision 随事务声明（§10.3 最小协议）；Source::Ai 入历史/事件/
    /// 日志；一批 op = 单条历史条目（整批 undo）。
    fn run_summarize_turn(&mut self, core: &mut Core) -> Result<u64, Reject> {
        self.observe(core);
        let base = core.revision();
        let ops = self.plan_summarize();
        let receipt = core.commit_transaction_as(
            base,
            ops,
            ApplyMeta {
                source: Source::Ai,
                command: Some("ai.summarize".into()),
            },
        )?;
        Ok(receipt.revision)
    }
}

// ---------------------------------------------------------------------------
// 验证点 1 — AI read current state
// ---------------------------------------------------------------------------

#[test]
fn ai_reads_human_edited_state_as_decision_input() {
    let mut core = Core::from_markdown("# Notes\nintro text\n- item one\n- item two\n  - nested");

    // Human 编辑（三条命令，经同一 Command Boundary）
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![0],
            start: 5,
            end: 5,
        },
        " (week 3)".into(),
    )
    .unwrap();
    core.delete_block(core.revision(), vec![2]).unwrap();
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![2, 0],
            start: 6,
            end: 6,
        },
        " care".into(),
    )
    .unwrap();

    // AI 读状态：块树平铺 = Human 编辑后的现场（内容 / 身份 / 层级 / 路径）
    let mut ai = ScriptedAi::new();
    ai.observe(&core);
    let obs = &ai.observed;
    assert_eq!(
        obs.len(),
        4,
        "先序含子块：heading + intro + item two + nested"
    );
    assert_eq!(obs[0].kind, "heading");
    assert_eq!(obs[0].text, "Notes (week 3)", "Human 文本编辑对 AI 可见");
    assert_eq!(obs[0].path, vec![0usize]);
    assert_eq!(obs[1].text, "intro text");
    assert_eq!(
        obs[2].text, "item two",
        "Human 删除的 item one 已不在 AI 视野"
    );
    assert_eq!(
        obs[3].path,
        vec![2usize, 0],
        "嵌套块路径由 stack-by-depth 复原"
    );
    assert_eq!(obs[3].depth, 1);
    assert_eq!(obs[3].text, "nested care");
    for b in obs {
        assert_eq!(
            b.id,
            id_at(&core, &b.path),
            "AI 读到的 id 与核内按路径解析一致"
        );
    }

    // 第二读通道：事件流（View 与 AI 的共同 op 流，§22 不变量 1）
    let events = core.take_events();
    assert_eq!(events.len(), 3);
    assert!(events.iter().all(|e| e.meta.source == Source::View));
    assert_eq!(
        events[1].ops[0],
        Op::RemoveBlock { path: vec![2] },
        "AI 从事件流看到 Human 的结构命令"
    );

    // 决策输入闭环：计划摘要行引用的正是读到的现场
    let plan = ai.plan_summarize();
    match &plan[0] {
        Op::InsertBlock {
            text,
            index,
            parent,
            ..
        } => {
            assert_eq!(*index, 3);
            assert!(parent.is_empty());
            assert_eq!(
                text.as_ref(),
                "AI summary: 3 top-level blocks; first: Notes (week 3)",
                "AI 的输出由它读到的 Human 后状态决定"
            );
        }
        other => panic!("计划首 op 应为 InsertBlock: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 验证点 2 — AI 生成 deterministic commands
// ---------------------------------------------------------------------------

#[test]
fn scripted_ai_plan_is_deterministic_and_primitive_only() {
    // 两个独立构建的相同状态（人手插入一个空段落）
    let build = || {
        let mut c = Core::from_markdown("para one");
        c.insert_block(
            c.revision(),
            vec![],
            1,
            BlockKind::Paragraph,
            Default::default(),
            String::new(),
        )
        .unwrap();
        c
    };
    let core_a = build();
    let core_b = build();

    let mut a = ScriptedAi::new();
    a.observe(&core_a);
    let plan_a1 = a.plan_summarize();
    let plan_a2 = a.plan_summarize();
    assert_eq!(plan_a1, plan_a2, "同一观察 → 同一计划（纯函数）");
    assert_eq!(plan_a1.len(), 3, "填充 + 提级 + 追加三 op 一批");

    let mut b = ScriptedAi::new();
    b.observe(&core_b);
    assert_eq!(a.observed, b.observed, "相同状态读出相同观察");
    let plan_b = b.plan_summarize();
    assert_eq!(
        serde_json::to_string(&plan_a1).unwrap(),
        serde_json::to_string(&plan_b).unwrap(),
        "跨核逐字节确定：同状态 → 同 op JSON"
    );

    // 只允许 §10.2 原语（十原语中 split/merge/transaction 在命令层展开，
    // op 层即这 7 种原子）——没有自然语言、没有自由格式指令。
    const PRIMITIVES: [&str; 7] = [
        "insert_block",
        "remove_block",
        "replace_text",
        "set_block_type",
        "set_attrs",
        "set_mark",
        "move_block",
    ];
    for op in &plan_a1 {
        let tag = serde_json::to_value(op).unwrap()["t"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(PRIMITIVES.contains(&tag.as_str()), "非原语 op: {tag}");
    }

    // 计划是状态的函数：换一个状态（无空段落）→ 不同计划
    let core_c = Core::from_markdown("para one");
    let mut c = ScriptedAi::new();
    c.observe(&core_c);
    let plan_c = c.plan_summarize();
    assert_eq!(plan_c.len(), 1, "无空段落 → 仅追加分支");
    assert_ne!(plan_c, plan_a1);
}

#[test]
fn ai_fills_empty_paragraph_via_command_boundary() {
    // Human 先编辑：把 "draft" 在尾部 split 出一个空段落（双击回车的核内等价）
    let mut core = Core::from_markdown("meeting notes\ndraft");
    core.split_block(core.revision(), vec![1], 5).unwrap();
    assert_eq!(text_at(&core, &[2]), "", "现场确有空段落");
    assert_eq!(core.revision(), 1);

    // AI 一轮：读 → 计划 → base_revision + Source::Ai 提交
    let mut ai = ScriptedAi::new();
    let ai_rev = ai.run_summarize_turn(&mut core).unwrap();

    assert_eq!(ai_rev, 2, "AI 事务独立 bump revision");
    assert_eq!(
        text_at(&core, &[2]),
        "AI summary: 3 top-level blocks; first: meeting notes",
        "空段落被确定性填充"
    );
    assert_eq!(kind_at(&core, &[2]), BlockKind::Heading, "提级为 heading");
    assert_eq!(
        text_at(&core, &[3]),
        "AI summary: 3 top-level blocks; first: meeting notes"
    );
    assert_eq!(core.history.undo_depth(), 2, "Human split 一条 + AI 批一条");

    // 事件与日志溯源（Source::Ai 全程留痕）
    let events = core.take_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].meta.source, Source::Ai);
    assert_eq!(events[1].meta.command.as_deref(), Some("ai.summarize"));
    assert!(!events[1].meta.coalesced);
    assert_eq!(events[1].revision, 2);
    let jsonl = core.log.to_jsonl();
    assert!(
        jsonl.contains("\"source\":\"ai\""),
        "AI 进 Revision 日志: {jsonl}"
    );
    assert!(jsonl.contains("\"command\":\"ai.summarize\""));
}

// ---------------------------------------------------------------------------
// 验证点 3 — Core execute + revision（AI 独立 revision，不进 1s 合并窗）
// ---------------------------------------------------------------------------

#[test]
fn ai_text_command_bumps_own_revision_never_coalesces() {
    let now = Arc::new(std::sync::Mutex::new(0u64));
    let mut core = Core::with_clock(clock_of(&now));
    core.tree = mdcore::markdown::markdown_to_tree("para");

    // Human 连续击键（1s 窗内）→ 合并成单条目单 revision（§11.1）
    for t in [0u64, 100, 200] {
        *now.lock().unwrap() = t;
        core.replace_text(
            mdcore::anchor::TextTarget::ByPath {
                path: vec![0],
                start: 4,
                end: 4,
            },
            "x".into(),
        )
        .unwrap();
    }
    assert_eq!(text_at(&core, &[0]), "paraxxx");
    assert_eq!(core.history.undo_depth(), 1);
    assert_eq!(core.revision(), 1);

    // AI 纯文本命令（同一时钟、同为 text kind）→ 仍独立成条目
    *now.lock().unwrap() = 400;
    let r = core
        .commit_transaction_as(
            core.revision(),
            vec![Op::ReplaceText {
                path: vec![0],
                start: 7,
                end: 7,
                insert: " [AI]".into(),
            }],
            ApplyMeta {
                source: Source::Ai,
                command: Some("ai.rephrase".into()),
            },
        )
        .unwrap();
    assert_eq!(r.revision, 2, "AI 命令各自 bump，不并入 Human 合并窗");
    assert_eq!(core.history.undo_depth(), 2);

    // R12 收紧：AI 条目之后的 Human 击键开新条目（不吸进 AI 批）
    *now.lock().unwrap() = 500;
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![0],
            start: 12,
            end: 12,
        },
        "!".into(),
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), 3, "合并窗两端都必须是 View 击键");
    assert_eq!(core.revision(), 3);

    // undo 链：! → [AI] → xxx（AI 整批一条，Human 各自一条）
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[0]), "paraXXX [AI]".replace("XXX", "xxx"));
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[0]), "paraxxx");
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[0]), "para");

    // 事件流按序留痕：3 合并击键 + AI + 人手感叹号 + 3 次 undo
    let events = core.take_events();
    assert_eq!(events.len(), 8);
    assert_eq!(events[0].meta.source, Source::View);
    assert!(!events[0].meta.coalesced);
    assert!(events[1].meta.coalesced);
    assert!(events[2].meta.coalesced);
    assert_eq!(events[3].meta.source, Source::Ai);
    assert!(!events[3].meta.coalesced, "AI 事件永不标记 coalesced");
    assert_eq!(events[4].meta.source, Source::View);
    assert!(!events[4].meta.coalesced);
    assert!(events[5..].iter().all(|e| e.meta.source == Source::Undo));
}

// ---------------------------------------------------------------------------
// 验证点 4a — undo：AI 批量 = 单条历史条目，整批回退不吞 Human 编辑
// ---------------------------------------------------------------------------

#[test]
fn ai_batch_undo_reverts_whole_batch_not_human_edit() {
    let mut core = Core::from_markdown("title line\nbody text");

    // Human 编辑（先）：文本一笔 + 插入空段落一笔
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![1],
            start: 5,
            end: 9,
        },
        "EDITED".into(),
    )
    .unwrap();
    core.insert_block(
        core.revision(),
        vec![],
        1,
        BlockKind::Paragraph,
        Default::default(),
        String::new(),
    )
    .unwrap();
    let after_human = md(&core);
    let human_entries = core.history.undo_depth();
    assert_eq!(human_entries, 2);

    // AI 批量事务（fill + 提级 + 追加 = 3 op 一条历史条目）
    let mut ai = ScriptedAi::new();
    let ai_rev = ai.run_summarize_turn(&mut core).unwrap();
    assert_eq!(
        core.history.undo_depth(),
        human_entries + 1,
        "多 op 批 = 单条目"
    );
    let ai_state = md(&core);
    assert_ne!(ai_state, after_human);
    let ai_block_id = id_at(&core, &[3]);

    // undo 一次 → AI 整批回退，Human 编辑分毫不动
    core.undo().unwrap();
    assert_eq!(md(&core), after_human, "一次 undo 回退 AI 整批");
    assert_eq!(text_at(&core, &[2]), "body EDITED", "Human 编辑未被吞");
    assert_eq!(core.revision(), ai_rev + 1);

    // 继续 undo 到位 → 与初始文档一致
    core.undo().unwrap();
    core.undo().unwrap();
    let fresh = Core::from_markdown("title line\nbody text");
    assert_eq!(md(&core), md(&fresh));

    // redo：AI 整批单条恢复，块身份物化还原
    core.redo().unwrap();
    core.redo().unwrap();
    core.redo().unwrap();
    assert_eq!(md(&core), ai_state);
    assert_eq!(id_at(&core, &[3]), ai_block_id, "redo 复原 AI 追加块身份");
    assert!(!core.history.can_redo());
}

#[test]
fn ai_entries_survive_save_open_and_undo_whole_batch() {
    let mut core = Core::from_markdown("persist me");
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![0],
            start: 10,
            end: 10,
        },
        " please".into(),
    )
    .unwrap();
    let mut ai = ScriptedAi::new();
    ai.run_summarize_turn(&mut core).unwrap();
    let after_ai = md(&core);

    // save/open：snapshot（状态）+ history_json（undo 栈）往返
    let snap = core.snapshot().to_json();
    let hj = core.history_json();
    let mut reopened = Core::restore(mdcore::snapshot::Snapshot::from_json(&snap).unwrap());
    reopened.set_history_json(&hj).unwrap();
    assert_eq!(reopened.history.undo_depth(), 2);

    reopened.undo().unwrap();
    assert_eq!(
        text_at(&reopened, &[0]),
        "persist me please",
        "重开后 undo 仍不吞 Human 编辑"
    );
    assert_eq!(reopened.history.undo_depth(), 1);
    reopened.redo().unwrap();
    assert_eq!(md(&reopened), after_ai, "重开后 AI 整批可单条 redo");
}

// ---------------------------------------------------------------------------
// 验证点 4b — stale：base_revision 硬门禁 + 重读重试，零污染
// ---------------------------------------------------------------------------

#[test]
fn ai_stale_transaction_rejected_then_retries_with_fresh_base() {
    let mut core = Core::from_markdown("intro\nbody");
    core.insert_block(
        core.revision(),
        vec![],
        1,
        BlockKind::Paragraph,
        Default::default(),
        String::new(),
    )
    .unwrap();

    // AI 观察（base = 1）并制定计划
    let mut ai = ScriptedAi::new();
    ai.observe(&core);
    let stale_plan = ai.plan_summarize();
    let stale_base = core.revision();
    assert_eq!(stale_base, 1);
    assert_eq!(stale_plan.len(), 3);

    // Human 在 AI 提交前抢先填掉了那个空段落（真实并发时序）
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![1],
            start: 0,
            end: 0,
        },
        "human note".into(),
    )
    .unwrap();
    let current = core.revision();
    assert_eq!(current, stale_base + 1);
    let tree_before = debug::dump_tree(&core.tree);
    let undo_before = core.history.undo_depth();
    core.take_events(); // 取走 Human 两笔的事件，专测 reject 自身零事件

    // stale 提交 → Reject::Stale，状态零污染（§11.2：绝不静默 rebase）
    let rejected = core.commit_transaction_as(
        stale_base,
        stale_plan.clone(),
        ApplyMeta {
            source: Source::Ai,
            command: Some("ai.summarize".into()),
        },
    );
    assert_eq!(
        rejected,
        Err(Reject::Stale { current }),
        "base_revision 不等即 reject"
    );
    assert_eq!(debug::dump_tree(&core.tree), tree_before, "树零污染");
    assert_eq!(core.history.undo_depth(), undo_before, "无历史条目");
    assert_eq!(core.revision(), current, "revision 不动");
    assert!(core.take_events().is_empty(), "无事件泄漏");

    // AI 重读状态 → 确定性重规划（空段落没了 → 只剩追加分支）→ 新 base 重试
    ai.observe(&core);
    let fresh_plan = ai.plan_summarize();
    assert_ne!(fresh_plan, stale_plan, "重读后计划随状态更新");
    assert_eq!(fresh_plan.len(), 1);
    let r = core
        .commit_transaction_as(
            core.revision(),
            fresh_plan,
            ApplyMeta {
                source: Source::Ai,
                command: Some("ai.summarize".into()),
            },
        )
        .unwrap();
    assert_eq!(r.revision, current + 1);
    assert_eq!(text_at(&core, &[1]), "human note", "人手编辑未被覆盖");
    assert_eq!(
        md(&core).matches("AI summary").count(),
        1,
        "重试恰好生效一次，无重复应用"
    );
}

// ---------------------------------------------------------------------------
// 验证点 3（补充）— AI 批量事务原子性：一个坏 op 拖垮整批且零污染
// ---------------------------------------------------------------------------

#[test]
fn ai_batch_is_atomic_on_bad_op() {
    let mut core = Core::from_markdown("one\ntwo");
    let mut ai = ScriptedAi::new();
    ai.observe(&core);
    let mut plan = ai.plan_summarize();
    // 故障注入：计划尾部混入一个坏路径 op（AI 侧 bug 的替身）
    plan.push(Op::ReplaceText {
        path: vec![99],
        start: 0,
        end: 0,
        insert: "nope".into(),
    });

    let tree_before = debug::dump_tree(&core.tree);
    let base = core.revision();
    let rejected = core.commit_transaction_as(
        base,
        plan,
        ApplyMeta {
            source: Source::Ai,
            command: Some("ai.summarize".into()),
        },
    );
    assert!(
        matches!(rejected, Err(Reject::Invalid(_))),
        "坏 op 必须 reject: {rejected:?}"
    );
    assert_eq!(
        debug::dump_tree(&core.tree),
        tree_before,
        "整批原子回滚，零污染"
    );
    assert_eq!(core.revision(), base);
    assert_eq!(core.history.undo_depth(), 0);
    assert!(core.take_events().is_empty());

    // 修正后（重新观察 + 重规划）重试成功
    let mut ai2 = ScriptedAi::new();
    ai2.run_summarize_turn(&mut core).unwrap();
    assert!(text_at(&core, &[2]).starts_with("AI summary"));
}

// ---------------------------------------------------------------------------
// 闭环端到端 — Human command → Document State ← AI command 交替三轮
// ---------------------------------------------------------------------------

#[test]
fn human_and_ai_alternate_on_one_document_state() {
    let mut core = Core::from_markdown("kickoff");
    let mut ai = ScriptedAi::new();

    // Round 1：Human 续写 → AI 追加摘要
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![0],
            start: 7,
            end: 7,
        },
        " doc".into(),
    )
    .unwrap();
    ai.run_summarize_turn(&mut core).unwrap();

    // Round 2：Human 留空段落 → AI 填充 + 追加
    core.insert_block(
        core.revision(),
        vec![],
        1,
        BlockKind::Paragraph,
        Default::default(),
        String::new(),
    )
    .unwrap();
    ai.run_summarize_turn(&mut core).unwrap();

    // Round 3：Human 改写标题
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![0],
            start: 0,
            end: 7,
        },
        "FINAL".into(),
    )
    .unwrap();

    // 终态：Human 与 AI 的改动共存于同一 Document State（§22 不变量 1）
    assert_eq!(text_at(&core, &[0]), "FINAL doc");
    assert_eq!(kind_at(&core, &[1]), BlockKind::Heading);
    assert_eq!(
        text_at(&core, &[1]),
        "AI summary: 3 top-level blocks; first: kickoff doc"
    );
    assert_eq!(
        text_at(&core, &[2]),
        "AI summary: 1 top-level blocks; first: kickoff doc"
    );
    assert_eq!(
        text_at(&core, &[3]),
        "AI summary: 3 top-level blocks; first: kickoff doc"
    );

    // 同一事件流，时序完整：View/Ai 交替，command 各自署名
    let events = core.take_events();
    let seq: Vec<(Source, Option<&str>)> = events
        .iter()
        .map(|e| (e.meta.source, e.meta.command.as_deref()))
        .collect();
    assert_eq!(
        seq,
        vec![
            (Source::View, Some("replace_text")),
            (Source::Ai, Some("ai.summarize")),
            (Source::View, Some("insert_block")),
            (Source::Ai, Some("ai.summarize")),
            (Source::View, Some("replace_text")),
        ]
    );

    // 单一历史所有权：undo 依次回退 R3 人手 → R2 AI 整批 → R2 人手 → R1 AI 整批 → R1 人手
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[0]), "kickoff doc");
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[1]), "", "R2 空段落随 AI 整批复原");
    assert_eq!(kind_at(&core, &[1]), BlockKind::Paragraph);
    assert_eq!(core.tree.top_level().len(), 3, "R2 追加块同时消失");
    core.undo().unwrap();
    assert_eq!(core.tree.top_level().len(), 2);
    core.undo().unwrap();
    assert_eq!(core.tree.top_level().len(), 1, "R1 AI 摘要随整批消失");
    core.undo().unwrap();
    assert_eq!(text_at(&core, &[0]), "kickoff");
    assert!(!core.history.can_undo());
}

// ---------------------------------------------------------------------------
// 验证点 4c — replay：含 AI/undo/redo 记录的日志重放 = 实时状态（逐字节）
// ---------------------------------------------------------------------------

#[test]
fn ai_inclusive_log_replays_to_identical_state() {
    let mut core = Core::from_markdown("alpha\nbeta");

    // Human
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![1],
            start: 4,
            end: 4,
        },
        " v2".into(),
    )
    .unwrap();
    // AI 事务
    let mut ai = ScriptedAi::new();
    ai.run_summarize_turn(&mut core).unwrap();
    // undo / redo 往返
    core.undo().unwrap();
    core.redo().unwrap();
    // AI 再来一轮（追加第二个摘要块）
    ai.run_summarize_turn(&mut core).unwrap();
    // Human 收尾
    core.replace_text(
        mdcore::anchor::TextTarget::ByPath {
            path: vec![0],
            start: 0,
            end: 5,
        },
        "alpha final".into(),
    )
    .unwrap();

    let jsonl = core.log.to_jsonl();
    assert!(jsonl.contains("\"source\":\"ai\""), "日志含 AI 记录");
    assert!(
        jsonl.contains("\"source\":\"undo\""),
        "日志含 undo 记录（R11 修复面）"
    );
    assert!(jsonl.contains("\"source\":\"redo\""));

    // R11 硬断言风格：重放 = 实时状态（块树/序列化/块 JSON/revision 逐一相等）
    let replayed = Core::replay_jsonl(&jsonl).unwrap();
    assert_eq!(
        debug::dump_tree(&replayed.tree),
        debug::dump_tree(&core.tree),
        "重放逐块一致（含身份）"
    );
    assert_eq!(md(&replayed), md(&core), "重放序列化逐字节一致");
    assert_eq!(
        debug::blocks_json(&replayed.tree),
        debug::blocks_json(&core.tree),
        "重放块树 JSON 逐字节一致（含 id/attrs）"
    );
    assert_eq!(replayed.revision(), core.revision(), "revision 复原");
}
