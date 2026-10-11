//! R10-a — 文件层（document_id）+ 渐进打开（extend_from_markdown）语义测试。
//!
//! 硬断言清单：
//! 1. one-shot vs K 段 extend：树指纹 + 先序 ID 序列 + to_markdown 三重相等
//!    （1MB 级文档，fence/task/quote/list/table 混合）；
//! 2. R21/ADR-B4：extend 不入 undo 栈（can_undo=false）+ System 边界破组
//!    （View→extend→View 的 undo 只撤后段 View 编辑，ingest 与前段不动）
//!    + 水位不回退；
//! 3. extend 的 op 流进事件队列 + revision 前进（Source::System）+ undo 栈
//!    零条目（mem_stats undo_entries==0）；
//! 4. 未闭合 fence chunk 被 Reject::IncompleteChunk 且树零污染；
//! 5. document_id 三路恢复一致（save/open/replay）；
//! 6. 重复 set_document_id：幂等同值、拒绝异值；
//! 7. DocumentId Display/FromStr 往返 + version/variant 位校验；
//! 8. genesis 携带 document_id 的 JSONL 重放恢复 + 旧格式向后兼容。
//!
//! 分块纪律（测试即分块器参考实现）：section = 自含块组（fence 成对、
//! table 行连续、quote 组连续、列表组以 depth-1 项开头），chunk 只在
//! section 边界切开。quote 组全文档唯一且置于开头——v0 语法的 quote
//! 容器跨行合并/入栈不弹出，中途切 quote 组会破坏 one-shot 等价性。

use std::sync::Arc;

use mdcore::anchor::TextTarget;
use mdcore::document::DocumentId;
use mdcore::history::Source;
use mdcore::op::Op;
use mdcore::{Core, Reject};

type Fp = (u64, mdcore::tree::BlockKind, String, String);

/// 先序平铺指纹：(id, kind, text, attrs)——树结构 + 身份 + 内容一起比。
fn preorder_fingerprint(core: &Core) -> Vec<Fp> {
    fn walk(tree: &mdcore::tree::BlockTree, id: mdcore::tree::NodeId, out: &mut Vec<Fp>) {
        let n = tree.node(id);
        out.push((n.id.0, n.kind, n.text.to_string(), format!("{:?}", n.attrs)));
        for &c in &n.children {
            walk(tree, c, out);
        }
    }
    let mut out = Vec::new();
    for &child in core.tree.top_level() {
        walk(&core.tree, child, &mut out);
    }
    out
}

fn preorder_ids(core: &Core) -> Vec<u64> {
    preorder_fingerprint(core).iter().map(|f| f.0).collect()
}

fn top_texts(core: &Core) -> Vec<String> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).text.to_string())
        .collect()
}

/// 文档 section 生成器：每个 section 是自含块组（以 \n 结尾）。
/// quote 组全文档唯一、置于最前；列表组一律以 depth-1 项开头（自清栈）。
fn build_sections(target_bytes: usize) -> Vec<String> {
    let mut sections = vec![
        // quote 组（唯一；栈干净时解析，三段落并入同一容器）
        "> quoted line one\n> quoted line two\n> quoted line three\n".to_string(),
    ];
    let mut i = 0usize;
    let mut budget = target_bytes;
    while budget > 0 {
        let next = match i % 6 {
            0 => format!(
                "paragraph block {i} line a with some filler text to grow the document\n\
                 paragraph block {i} line b\n\
                 paragraph block {i} line c\n"
            ),
            1 => format!("## heading section {i}\n"),
            2 => format!(
                "```rust\nfn block_{i}() {{\n    let x = {i}; // filler\n    println(x);\n}}\n```\n"
            ),
            3 => format!("| col a {i} | col b | col c |\n| v1 | v2 | v3 |\n| v4 | v5 | v6 |\n"),
            4 => "---\n".to_string(),
            _ => format!(
                "- task todo {i} unchecked\n\
                  - [ ] sub task of {i}\n\
                  1. ordered nested step\n\
                 - [x] done item\n\
                 * star marker item\n"
            ),
        };
        budget = budget.saturating_sub(next.len());
        sections.push(next);
        i += 1;
    }
    sections
}

/// 参考分块器：只在 section 边界切（每 chunk 约 `per` 个 section）。
fn chunk_sections(sections: &[String], per: usize) -> Vec<String> {
    sections.chunks(per).map(|group| group.concat()).collect()
}

// ---------------------------------------------------------------------------
// ① one-shot vs K 段 extend：三重相等（1MB 级 + 混合块型）
// ---------------------------------------------------------------------------

#[test]
fn oneshot_vs_chunked_extend_triple_equality() {
    let sections = build_sections(1_000_000);
    let full: String = sections.concat();
    assert!(
        full.len() >= 1_000_000,
        "必须覆盖 1MB 级文档，实际 {}",
        full.len()
    );

    let one_shot = Core::from_markdown(&full);

    let chunks = chunk_sections(&sections, 16);
    let mut chunked = Core::new();
    for (k, chunk) in chunks.iter().enumerate() {
        assert!(mdcore::markdown::is_complete_chunk(chunk), "chunk {k} 悬挂");
        chunked
            .extend_from_markdown(chunk)
            .unwrap_or_else(|r| panic!("chunk {k} 被拒: {r:?}"));
    }

    // 三重相等：树指纹（kind/text/attrs，先序含嵌套）+ 先序 ID 序列 + to_markdown
    assert_eq!(
        preorder_fingerprint(&chunked),
        preorder_fingerprint(&one_shot),
        "树结构与内容必须一致"
    );
    assert_eq!(
        preorder_ids(&chunked),
        preorder_ids(&one_shot),
        "先序 ID 序列必须完全一致（两边都是文档序先序分配）"
    );
    assert_eq!(
        mdcore::markdown::tree_to_markdown(&chunked.tree),
        mdcore::markdown::tree_to_markdown(&one_shot.tree),
        "序列化输出必须一致"
    );
    assert!(one_shot.tree.top_level().len() > 5_000, "块数应足够大");
}

// ---------------------------------------------------------------------------
// ② R21/ADR-B4：extend 不入 undo 栈 + System 边界破组 + 水位不回退
// ---------------------------------------------------------------------------

#[test]
fn extend_not_undoable_and_system_boundary_breaks_group() {
    let sections = build_sections(2_000);
    // 固定时钟：全部事务同一毫秒——若 System 边界未推进（bug），后段 View
    // 击键会因 1s 窗吸进前段 View 条目，本测试的破组断言即失败。
    let mut core = Core::with_clock(Arc::new(|| 42_000));

    // 段 1：ingest（System）→ 不可 undo（行为锚点：打开文档不可撤销）
    core.extend_from_markdown(&sections[0..2].concat()).unwrap();
    assert!(!core.history.can_undo(), "ADR-B4：System 事务不入 undo 栈");
    assert_eq!(core.history.undo_depth(), 0);

    // 前段 View 编辑（历史条目 1）
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0usize, 0],
            start: 0,
            end: 6,
        },
        "QUOTED".into(),
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), 1);

    // 段 2/3：ingest → 不新增条目（栈上仍是前段 View 条目），但合并窗
    // 边界必须推进为 System
    core.extend_from_markdown(&sections[2..4].concat()).unwrap();
    core.extend_from_markdown(&sections[4..6].concat()).unwrap();
    assert_eq!(
        core.history.undo_depth(),
        1,
        "System 边界不入栈，前段 View 条目不受影响"
    );
    let before_view_b = preorder_fingerprint(&core); // = edit A + 全部 ingest 段

    // 后段 View 编辑：last_source == System → 必开新条目（边界破组），
    // 不得吸进前段 View 条目（错误合并 = 一次 undo 连 ingest 前的人手编辑一起吞）
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0usize, 0],
            start: 0,
            end: 6,
        },
        "EDITED".into(),
    )
    .unwrap();
    assert_eq!(
        core.history.undo_depth(),
        2,
        "System 边界破组：后段 View 击键必开新条目"
    );

    // undo 只撤后段 View 编辑：ingest 段与前段编辑原样
    core.undo().unwrap();
    assert_eq!(
        preorder_fingerprint(&core),
        before_view_b,
        "undo 只撤后段编辑：ingest 段与前段 View 编辑均不动"
    );
    assert_eq!(core.history.undo_depth(), 1);
    core.undo().unwrap();
    assert_eq!(core.history.undo_depth(), 0);
    assert!(!core.history.can_undo(), "两条 View 编辑全撤后栈空");

    // revision log 完全不变：System 事务仍逐条记录（重放 == 实时状态）
    let system_records = core
        .log
        .records
        .iter()
        .filter(|r| r.source == Source::System)
        .count();
    assert_eq!(
        system_records, 3,
        "三段 ingest 全在 revision log（ADR-B4 只过滤 undo 栈）"
    );
    assert!(
        core.log
            .records
            .iter()
            .filter(|r| r.source == Source::System)
            .all(|r| r.command.as_deref() == Some("extend_from_markdown")
                && r.ops.iter().all(|o| matches!(o, Op::InsertBlock { .. }))),
        "ingest 记录 = InsertBlock op 流 + extend_from_markdown 署名"
    );

    // 事件流完全不变：System 事务仍 push_event（视图仍被通知）
    let system_events = core
        .take_events()
        .iter()
        .filter(|e| e.meta.source == Source::System)
        .count();
    assert_eq!(system_events, 3, "三段 ingest 全部发出事件");

    // undo 后再 extend：新 ID 继续抬升水位（§8.2 永不回收/复用）
    let max_id_ever = preorder_fingerprint(&core)
        .iter()
        .map(|f| f.0)
        .max()
        .unwrap();
    core.extend_from_markdown(&sections[6..7].concat()).unwrap();
    assert!(!core.history.can_undo(), "extend 后栈仍空");
    let fp = preorder_fingerprint(&core);
    let new_ids = &fp[fp.len() - 1..];
    assert!(
        new_ids.iter().all(|f| f.0 > max_id_ever),
        "新块 ID 必须高于历史水位，不得复用: {new_ids:?}"
    );
}

// ---------------------------------------------------------------------------
// ③ extend 的 op 流进事件队列 + revision 前进
// ---------------------------------------------------------------------------

#[test]
fn extend_pipeline_events_revision_and_log() {
    let mut core = Core::new();
    let r = core.extend_from_markdown("first\nsecond\n").unwrap();
    assert_eq!(r.revision, 1, "首个非空 chunk 使 revision 0→1");

    let events = core.take_events();
    assert_eq!(events.len(), 1, "一个 chunk = 一个事务 = 一条事件");
    assert_eq!(events[0].meta.source, Source::System, "核自驱来源");
    assert_eq!(
        events[0].meta.command.as_deref(),
        Some("extend_from_markdown")
    );
    assert!(!events[0].meta.coalesced);
    assert!(events[0]
        .ops
        .iter()
        .all(|o| matches!(o, Op::InsertBlock { .. })));
    assert!(
        events[0]
            .ops
            .iter()
            .any(|o| matches!(o, Op::InsertBlock { id: Some(_), .. })),
        "op 内 ID 已物化（事件/历史/日志同源）"
    );

    // 嵌套块全量展开进 op 流：quote 容器 + 2 子段落 = 3 个 InsertBlock
    let mut nested = Core::new();
    nested.extend_from_markdown("> a\n> b\n").unwrap();
    let ops = &nested.take_events()[0].ops;
    assert_eq!(ops.len(), 3);
    assert!(matches!(
        &ops[0],
        Op::InsertBlock {
            kind: mdcore::tree::BlockKind::Blockquote,
            ..
        }
    ));
    assert!(matches!(&ops[1], Op::InsertBlock { parent, .. } if parent == &vec![0usize]));

    // System 不进合并窗且不入 undo 栈（R21/ADR-B4）：同钟两次 extend
    // 仍是两条 revision（log 各一条），undo 栈保持空
    let mut fixed = Core::with_clock(Arc::new(|| 42_000));
    fixed.extend_from_markdown("one\n").unwrap();
    fixed.extend_from_markdown("two\n").unwrap();
    assert_eq!(
        fixed.history.undo_depth(),
        0,
        "ADR-B4：System 事务不入 undo 栈"
    );
    assert!(!fixed.history.can_undo());
    assert_eq!(fixed.revision(), 2);

    // R21 mem_stats：ingest 后 undo 栈零条目零 op（10MB 开档省 ~24MB 的核侧不变量）
    let stats = fixed.history.mem_stats();
    assert_eq!(stats.undo_entries, 0);
    assert_eq!(stats.redo_entries, 0);
    assert_eq!(stats.undo_ops, 0);

    // 日志同源：最后一条记录 source=System、command 正确
    let last = fixed.log.records.last().unwrap();
    assert_eq!(last.source, Source::System);
    assert_eq!(last.command.as_deref(), Some("extend_from_markdown"));

    // 空 chunk = 无 op 无 bump 无事件（apply_ops 早退）
    let _ = fixed.take_events(); // 先排空前两次 extend 的事件队列
    let r = fixed.extend_from_markdown("").unwrap();
    assert_eq!(r.revision, 2);
    assert!(fixed.take_events().is_empty());
    assert_eq!(fixed.history.undo_depth(), 0, "空 chunk 不改变 ADR-B4 状态");
}

// ---------------------------------------------------------------------------
// ③' history JSON roundtrip：含 System 边界状态无损（ADR-B4）
// ---------------------------------------------------------------------------

#[test]
fn history_json_roundtrip_with_system_boundary_state() {
    let mut core = Core::from_markdown("seed\n");
    // System 边界状态：extend 不入栈但推进合并窗边界（last_source=System）
    core.extend_from_markdown("ingested tail\n").unwrap();
    let hj_system = core.history_json();
    assert!(
        hj_system.contains("\"last_source\":\"system\""),
        "边界状态必须随 JSON 落盘（ADR-B4 无损前提）: {hj_system}"
    );
    assert!(
        hj_system.contains("\"undo\":[]"),
        "System 事务不入栈: {hj_system}"
    );

    // System 后的 View 编辑：新条目 + 边界推进为 View（栈上有一条交互编辑）
    core.set_clock(Arc::new(|| 5_000_000));
    core.replace_text(
        TextTarget::ByPath {
            path: vec![0usize],
            start: 0,
            end: 4,
        },
        "SEED".into(),
    )
    .unwrap();
    assert_eq!(core.history.undo_depth(), 1);
    let hj_full = core.history_json();

    // 往返 ①：纯 System 边界态逐字节无损
    let mut a = Core::from_markdown("seed\ningested tail\n");
    a.set_history_json(&hj_system).unwrap();
    assert_eq!(
        a.history_json(),
        hj_system,
        "System 边界态 roundtrip 必须无损"
    );
    assert_eq!(a.history.undo_depth(), 0);
    assert!(!a.history.can_undo());

    // 往返 ②：System 边界 + View 条目混合态逐字节无损，且 undo 链立即可用
    let mut b = Core::from_markdown("seed\ningested tail\n");
    b.set_history_json(&hj_full).unwrap();
    assert_eq!(b.history_json(), hj_full, "混合态 roundtrip 必须无损");
    b.undo().unwrap();
    assert_eq!(
        top_texts(&b),
        vec!["seed", "ingested tail"],
        "重开后 undo 撤掉存档前 View 编辑"
    );

    // 往返 ③：恢复出的 System 边界状态下交互照常——View 击键开新条目
    // （边界字段随 JSON 复原、分组不跨档错误合并；时钟任意取值）
    a.set_clock(Arc::new(|| 42_000));
    a.replace_text(
        TextTarget::ByPath {
            path: vec![0usize],
            start: 0,
            end: 4,
        },
        "SEED".into(),
    )
    .unwrap();
    assert_eq!(
        a.history.undo_depth(),
        1,
        "恢复的 System 边界态下 View 击键开新条目，undo 链可用"
    );
}

// ---------------------------------------------------------------------------
// ④ 未闭合 fence chunk 被 Reject 且树零污染（回滚验证）
// ---------------------------------------------------------------------------

#[test]
fn extend_rejects_incomplete_fence_without_pollution() {
    let mut core = Core::from_markdown("stable alpha\n");
    let before = preorder_fingerprint(&core);
    let before_rev = core.revision();

    let bad_chunks = [
        "```rust\nlet x = 1;\n",      // fence 开在 EOF
        "```\n",                      // 孤立开栏
        "ok line\n```js\ninner\n",    // 先完整后悬挂
        "> q\n```python\nprint(1)\n", // quote 段之后悬挂
    ];
    for bad in bad_chunks {
        let err = core.extend_from_markdown(bad).unwrap_err();
        assert!(matches!(err, Reject::IncompleteChunk(_)), "got {err:?}");
        // 树零污染：指纹 / revision / 事件 / 历史全不变
        assert_eq!(preorder_fingerprint(&core), before, "树被污染: {bad:?}");
        assert_eq!(core.revision(), before_rev);
        assert!(core.take_events().is_empty(), "拒绝不得留事件");
        assert_eq!(core.history.undo_depth(), 0);
    }

    // 拒绝可过序列化边界（wasm Err 通道同款 JSON）
    let json = serde_json::to_string(&Reject::IncompleteChunk("x".into())).unwrap();
    assert!(json.contains("IncompleteChunk"), "reject json = {json}");

    // 拒绝后核仍健康：正常 chunk 照常追加
    core.extend_from_markdown("recovered tail\n").unwrap();
    assert_eq!(top_texts(&core), vec!["stable alpha", "recovered tail"]);
}

// ---------------------------------------------------------------------------
// ⑤ document_id 三路恢复一致（save / open / replay）
// ---------------------------------------------------------------------------

#[test]
fn document_id_survives_save_open_and_replay() {
    let mut core = Core::from_markdown("doc body one\ndoc body two");
    let id = DocumentId::generate(1_700_000_123_456);
    core.set_document_id(Some(id)).unwrap();
    core.extend_from_markdown("appended tail\n").unwrap();

    // 路 1：save → open
    let (md, meta) = core.save_document();
    let opened = Core::open_document(&md, &meta).unwrap();

    // 路 2：JSONL 重放（genesis 首条携带 id）
    let replayed = Core::replay_jsonl(&core.log.to_jsonl()).unwrap();

    for other in [&opened, &replayed] {
        assert_eq!(other.document_id(), Some(id), "三路 document_id 必须一致");
        assert_eq!(preorder_fingerprint(other), preorder_fingerprint(&core));
    }
    assert_eq!(opened.revision(), core.revision());
    assert_eq!(replayed.revision(), core.revision());

    // meta 形状：document_id 为 hex 字符串字段
    let meta_val: serde_json::Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(
        meta_val["document_id"].as_str(),
        Some(id.to_string().as_str())
    );
}

// ---------------------------------------------------------------------------
// ⑤' save→open 全量状态往返（树/revision/document_id/历史栈）
// ---------------------------------------------------------------------------

#[test]
fn save_open_roundtrip_restores_all_state() {
    let mut core = Core::from_markdown("alpha\nbeta\ngamma");
    core.set_document_id(Some(DocumentId::generate(1_700_000_000_000)))
        .unwrap();
    core.replace_text(
        TextTarget::ByPath {
            path: vec![1usize],
            start: 0,
            end: 4,
        },
        "BETA".into(),
    )
    .unwrap();
    let (md, meta) = core.save_document();

    let mut reopened = Core::open_document(&md, &meta).unwrap();
    assert_eq!(reopened.document_id(), core.document_id());
    assert_eq!(reopened.revision(), core.revision());
    assert_eq!(reopened.history_json(), core.history_json(), "历史栈全一致");
    assert_eq!(preorder_fingerprint(&reopened), preorder_fingerprint(&core));

    // 重开后的历史功能等价：undo 撤掉存档前那次文本编辑
    reopened.undo().unwrap();
    assert_eq!(top_texts(&reopened), vec!["alpha", "beta", "gamma"]);

    // meta 畸形显式失败（不静默错配身份）
    assert!(Core::open_document(&md, "not json").is_err());
    assert!(Core::open_document(&md, "{\"document_id\":\"not-an-uuid\"}").is_err());
    assert!(
        Core::open_document(&md, "{\"history\":{}}").is_err(),
        "坏历史 JSON 拒绝"
    );
}

// ---------------------------------------------------------------------------
// ⑥ 重复 set_document_id：幂等同值、拒绝异值
// ---------------------------------------------------------------------------

#[test]
fn set_document_id_idempotent_conflict_rejected() {
    let mut core = Core::from_markdown("x\n");
    let a = DocumentId::generate(111);
    let b = DocumentId::generate(222);
    assert!(core.document_id().is_none());

    core.set_document_id(Some(a)).unwrap();
    assert_eq!(core.document_id(), Some(a));
    core.set_document_id(Some(a)).unwrap(); // 幂等
    assert_eq!(core.document_id(), Some(a));

    let err = core.set_document_id(Some(b)).unwrap_err();
    assert!(matches!(err, Reject::Invalid(_)), "异值必须拒绝: {err:?}");
    assert_eq!(core.document_id(), Some(a), "拒绝后身份不变");

    // None = 用注入时钟现场生成
    let mut fresh = Core::with_clock(Arc::new(|| 1_700_000_000_000));
    fresh.set_document_id(None).unwrap();
    assert_eq!(
        fresh.document_id().unwrap().timestamp_ms(),
        1_700_000_000_000
    );
    // 已持有身份后再生成 = 异值 = 拒绝
    assert!(fresh.set_document_id(None).is_err());
    assert!(fresh.set_document_id(Some(a)).is_err());
}

// ---------------------------------------------------------------------------
// ⑦ DocumentId Display/FromStr 往返 + version/variant 位校验
// ---------------------------------------------------------------------------

#[test]
fn document_id_display_parse_validation() {
    let id = DocumentId::generate(1_698_888_777_666);
    let s = id.to_string();

    // 形状：36 字符 8-4-4-4-12 小写十六进制
    assert_eq!(s.len(), 36);
    assert!(s.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{s}");
    assert_eq!(s, s.to_lowercase());
    for (i, b) in s.as_bytes().iter().enumerate() {
        if [8, 13, 18, 23].contains(&i) {
            assert_eq!(*b, b'-', "dash 位置 {i}");
        } else {
            assert_ne!(*b, b'-');
        }
    }

    // 往返（含大写输入）
    let back: DocumentId = s.parse().unwrap();
    assert_eq!(back, id);
    assert_eq!(back.timestamp_ms(), 1_698_888_777_666);
    let upper: DocumentId = s.to_uppercase().parse().unwrap();
    assert_eq!(upper, id);

    // version 半字节 ≠ 7 拒绝（标准 UUID 字符串第 14 位 = byte6 高 nibble）
    let mut m = s.clone().into_bytes();
    assert_eq!(m[14], b'7');
    m[14] = b'4';
    assert!(
        String::from_utf8(m).unwrap().parse::<DocumentId>().is_err(),
        "version=4 形状必须拒绝"
    );

    // variant 顶位 ≠ 10 拒绝（第 19 位 = byte8 高 nibble）
    for bad_nibble in *b"0c" {
        let mut m = s.clone().into_bytes();
        m[19] = bad_nibble;
        assert!(
            String::from_utf8(m).unwrap().parse::<DocumentId>().is_err(),
            "variant 非法形状必须拒绝: {}",
            bad_nibble as char
        );
    }

    // 长度 / 分隔 / 非十六进制
    assert!("01822e4d".parse::<DocumentId>().is_err());
    assert!(format!("{s}x").parse::<DocumentId>().is_err());
    let mut m = s.clone().into_bytes();
    m[8] = b'g';
    assert!(String::from_utf8(m).unwrap().parse::<DocumentId>().is_err());
}

// ---------------------------------------------------------------------------
// ⑧ genesis 携带 document_id 的 JSONL 重放恢复 + 旧格式向后兼容
// ---------------------------------------------------------------------------

#[test]
fn genesis_carries_document_id_and_replay_restores() {
    let mut core = Core::from_markdown("genesis line one\ngenesis line two");
    core.set_document_id(Some(DocumentId::generate(1_700_000_000_001)))
        .unwrap();

    let jsonl = core.log.to_jsonl();
    let first_line = jsonl.lines().next().unwrap();
    assert!(
        first_line.contains("document_id"),
        "genesis 记录必须自含 document_id: {first_line}"
    );

    let restored = Core::replay_jsonl(&jsonl).unwrap();
    assert_eq!(restored.document_id(), core.document_id(), "重放恢复身份");
    assert_eq!(preorder_fingerprint(&restored), preorder_fingerprint(&core));
    assert_eq!(restored.revision(), core.revision());

    // 旧格式（无 document_id 字段）向后兼容：剥掉字段后重放 = 无身份
    let mut v: serde_json::Value = serde_json::from_str(first_line).unwrap();
    let obj = v.as_object_mut().unwrap();
    assert!(obj.remove("document_id").is_some());
    let legacy_jsonl = format!("{v}\n{}", &jsonl[first_line.len() + 1..]);
    let legacy_core = Core::replay_jsonl(&legacy_jsonl).unwrap();
    assert!(legacy_core.document_id().is_none(), "旧格式重放 = 无身份");
    assert_eq!(
        top_texts(&legacy_core),
        vec!["genesis line one", "genesis line two"]
    );

    // 日志内 id 冲突 = 重放拒绝（防拼接/篡改）
    let mut other = Core::from_markdown("other doc");
    other
        .set_document_id(Some(DocumentId::generate(1_700_000_000_002)))
        .unwrap();
    let other_line = other.log.to_jsonl().lines().next().unwrap().to_string();
    let conflict = format!("{first_line}\n{other_line}\n");
    let err = Core::replay_jsonl(&conflict)
        .err()
        .expect("replay 必须拒绝冲突 document_id");
    assert!(err.contains("conflicting document_id"), "err = {err}");
}

// ---------------------------------------------------------------------------
// ⑤'' extend 只追加、不碰既有块（ID 与内容原样，新 ID 高于水位）
// ---------------------------------------------------------------------------

#[test]
fn extend_appends_never_touches_existing() {
    let mut core = Core::from_markdown("root alpha\n## root head\n");
    let before = preorder_fingerprint(&core);
    let max_id_before = *preorder_ids(&core).iter().max().unwrap();

    core.extend_from_markdown("- appended item\n  - nested\nafter list\n")
        .unwrap();

    let after = preorder_fingerprint(&core);
    assert_eq!(&after[..before.len()], &before[..], "既有块（含 ID）原样");
    assert!(
        after[before.len()..].iter().all(|f| f.0 > max_id_before),
        "新增块 ID 全部高于既有水位: {:?}",
        &after[before.len()..]
    );
    assert_eq!(
        mdcore::markdown::tree_to_markdown(&core.tree),
        "root alpha\n## root head\n- appended item\n  - nested\nafter list",
        "追加语义 = 文档后缀原样入树"
    );
}
