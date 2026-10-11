//! R17 — P1-9 终案：Rope 坐标模型（码点 char）× CM6 UTF-16 码元 的桥接语义。
//!
//! 单位宪法（ADR-V2）：
//! - 核侧（ropey / op.start / op.end / Anchor.offset / MarkSpan）= **Unicode 码点**
//!   （`len_chars()`、`slice(start..end)` 的 char 语义）；
//! - CM6 / JS 串 = **UTF-16 码元**；
//! - 桥（web/src/main.ts）以单一函数对 utf16ToCp / cpToUtf16 双向换算。
//!
//! 本文件锁定核侧的码点语义在**非 BMP 语料**（代理对、ZWJ 家族、CJK 扩展 B、
//! 变体选择符 U+FE0F）下零误差：此前 W1-W12 与全部 Rust 测试均为 BMP/ASCII
//! 语料，非 BMP 用例为零——这就是 P1-9「未收口」的实质。
//!
//! 基础事实（本文件反复引用，第一条测试自证）：
//! - "𠮷" U+20BB7：1 码点 / 2 UTF-16 码元 / 4 字节；
//! - "🙂" U+1F642：1 码点 / 2 UTF-16 码元；
//! - "👨‍👩‍👧‍👦"：7 码点（4 人 + 3 ZWJ U+200D）/ 11 UTF-16 码元；
//! - "𠮷️" = U+20BB7 + U+FE0F（变体选择符）：2 码点。

use mdcore::anchor::{Anchor, AnchorError, TextTarget};
use mdcore::id::BlockId;
use mdcore::markdown::tree_to_markdown;
use mdcore::op::Op;
use mdcore::tree::BlockKind;
use mdcore::{ApplyMeta, Core};

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

const ZWJ_FAMILY: &str = "👨‍👩‍👧‍👦"; // 7 码点 / 11 UTF-16 码元

/// 非 BMP 混合长文档（canonical：无空行，fence 闭合，表格行连续）。
/// 覆盖：CJK 扩展 B + 变体选择符、ZWJ 家族、日文、希腊字母、表格、fence、
/// 3 层嵌套 list、blockquote、thematic break。
const FIXTURE: &str = concat!(
    "# 𠮷️ 見出し\n",
    "段落 αβγ 𠮷 🙂 👨‍👩‍👧‍👦 **bold𠮷** `code😀` 尾\n",
    "- item 𠮷 α\n",
    "  - nested 🙂 β\n",
    "    - deep 👨‍👩‍👧‍👦 γ\n",
    "> quoted 🚀 δ\n",
    "```rs\n",
    "let s = \"𠮷🙂\"; // 👨‍👩‍👧‍👦\n",
    "```\n",
    "| 見出し𠮷 | 🙂 |\n",
    "| α | 👨‍👩‍👧‍👦 |\n",
    "---\n",
    "🦊 狐 paragraph 👨‍👩‍👧‍👦",
);

fn top_texts(core: &Core) -> Vec<String> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).text.to_string())
        .collect()
}

fn top_ids(core: &Core) -> Vec<BlockId> {
    core.tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).id)
        .collect()
}

fn node_text(core: &Core, path: &[usize]) -> String {
    let slot = core.tree.resolve(path).expect("resolve path");
    core.tree.node(slot).text.to_string()
}

fn node_id(core: &Core, path: &[usize]) -> BlockId {
    let slot = core.tree.resolve(path).expect("resolve path");
    core.tree.node(slot).id
}

/// 码点切分工具：按码点索引替换（等价于核内 ReplaceText 的语义视角）。
fn cp_replace(s: &str, start: usize, end: usize, insert: &str) -> String {
    let head: String = s.chars().take(start).collect();
    let tail: String = s.chars().skip(end).collect();
    format!("{head}{insert}{tail}")
}

// ---------------------------------------------------------------------------
// 0. 基础事实自证（防止 fixture 注释与实际码点数漂移）
// ---------------------------------------------------------------------------

#[test]
fn fixture_units_sanity() {
    assert_eq!("𠮷".chars().count(), 1);
    assert_eq!("𠮷".encode_utf16().count(), 2);
    assert_eq!("🙂".chars().count(), 1);
    assert_eq!("🙂".encode_utf16().count(), 2);
    assert_eq!(ZWJ_FAMILY.chars().count(), 7);
    assert_eq!(ZWJ_FAMILY.encode_utf16().count(), 11);
    assert_eq!("𠮷️".chars().count(), 2, "U+20BB7 + U+FE0F 变体选择符");
    assert!(
        FIXTURE.chars().any(|c| (c as u32) > 0xFFFF),
        "fixture 必须真实含非 BMP 字符"
    );
    assert!(
        FIXTURE.contains('\u{200D}') && FIXTURE.contains('\u{FE0F}'),
        "fixture 必须含 ZWJ 与变体选择符"
    );
}

// ---------------------------------------------------------------------------
// ① from_markdown → to_markdown 全文往返恒等（非 BMP 语料）
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_identity_non_bmp() {
    let core = Core::from_markdown(FIXTURE);
    let md = tree_to_markdown(&core.tree);
    assert_eq!(
        md, FIXTURE,
        "parse → serialize 逐字符恒等（无空行 canonical 输入）"
    );
    // 二次 parse → serialize 幂等（序列化产物仍是 canonical 输入）
    let core2 = Core::from_markdown(&md);
    assert_eq!(tree_to_markdown(&core2.tree), FIXTURE);

    // 结构抽查：非 BMP 不影响行级解析（块型与层级按预期建树）
    let kinds: Vec<BlockKind> = core
        .tree
        .top_level()
        .iter()
        .map(|&c| core.tree.node(c).kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            BlockKind::Heading,
            BlockKind::Paragraph,
            BlockKind::ListItem,
            BlockKind::Blockquote,
            BlockKind::CodeBlock,
            BlockKind::Table,
            BlockKind::ThematicBreak,
            BlockKind::Paragraph,
        ]
    );
    // 3 层嵌套 list 的文本逐字符保真（含 ZWJ 家族）
    assert_eq!(node_text(&core, &[2]), "item 𠮷 α");
    assert_eq!(node_text(&core, &[2, 0]), "nested 🙂 β");
    assert_eq!(node_text(&core, &[2, 0, 0]), "deep 👨‍👩‍👧‍👦 γ");
    // fence 体与表格行内的非 BMP 原样保留
    assert_eq!(node_text(&core, &[4]), "let s = \"𠮷🙂\"; // 👨‍👩‍👧‍👦");
    assert_eq!(node_text(&core, &[5]), "| 見出し𠮷 | 🙂 |\n| α | 👨‍👩‍👧‍👦 |");
}

#[test]
fn heading_and_marker_prefix_offsets_are_code_points() {
    // heading text 不含 "# " 前缀；非 BMP 紧邻前缀也不影响切分
    let core = Core::from_markdown("# 𠮷️ 見出し");
    assert_eq!(node_text(&core, &[0]), "𠮷️ 見出し");
    // "𠮷️ 見出し" = 6 码点 / 7 UTF-16 码元 / 17 字节——三套单位各不相同，
    // 核侧 len_chars 必须落在码点口径上
    let slot = core.tree.resolve(&[0]).unwrap();
    let text = core.tree.node(slot).text.to_string();
    assert_eq!(core.tree.node(slot).text.len_chars(), text.chars().count());
    assert_eq!(core.tree.node(slot).text.len_chars(), 6);
    assert_eq!(
        text.encode_utf16().count(),
        7,
        "UTF-16 码元数 ≠ 码点数（代理对在块文本内）"
    );
    assert_eq!(text.len(), 17);

    // list marker "-" 是 ASCII：marker 长度在两单位系下恒等（桥 prefix 不变量的依据）
    let li = Core::from_markdown("- 𠮷");
    let slot = li.tree.resolve(&[0]).unwrap();
    assert_eq!(
        li.tree.node(slot).attrs.get("marker"),
        Some(&"-".to_string())
    );
    assert_eq!(li.tree.node(slot).text.to_string(), "𠮷");
}

// ---------------------------------------------------------------------------
// ② replace_text / split_block / merge_block 的 op 偏移语义（码点）恒成立
// ---------------------------------------------------------------------------

#[test]
fn replace_text_offsets_are_code_points() {
    let mut core = Core::from_markdown("𠮷a🙂b"); // 4 码点 / 6 UTF-16 码元
    let id0 = node_id(&core, &[0]);
    assert_eq!(node_text(&core, &[0]), "𠮷a🙂b");

    // 在码点 [1,2)（'a'）替换为 7 码点的 ZWJ 家族：偏移按码点计
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 1,
            end: 2,
            insert: ZWJ_FAMILY.into(),
        }],
        ApplyMeta::view("test"),
    )
    .unwrap();
    assert_eq!(node_text(&core, &[0]), format!("𠮷{ZWJ_FAMILY}🙂b"));

    // undo/redo 后文本与 ID 恒等
    core.undo().unwrap();
    assert_eq!(node_text(&core, &[0]), "𠮷a🙂b");
    assert_eq!(node_id(&core, &[0]), id0, "undo 不改写身份");
    core.redo().unwrap();
    assert_eq!(node_text(&core, &[0]), format!("𠮷{ZWJ_FAMILY}🙂b"));

    // 跨非 BMP 边界的删除：[0,1) 恰好吞掉整个 𠮷（码点边界，不产生孤代理对）
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 0,
            end: 1,
            insert: "許".into(),
        }],
        ApplyMeta::view("test"),
    )
    .unwrap();
    assert_eq!(node_text(&core, &[0]), format!("許{ZWJ_FAMILY}🙂b"));
    core.undo().unwrap();
    assert_eq!(node_text(&core, &[0]), format!("𠮷{ZWJ_FAMILY}🙂b"));

    // 越界拒绝以码点判定：现文本 10 码点（𠮷+ZWJ家族+🙂+b），end=11 越界；
    // 若错按 UTF-16 码元口径（19 码元）判定，end=11 反而会「合法」——本断言
    // 同时锁死「单位不许是码元」
    let err = core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 0,
            end: 11,
            insert: "x".into(),
        }],
        ApplyMeta::view("test"),
    );
    assert!(
        matches!(err, Err(mdcore::Reject::Invalid(_))),
        "BadRange 以码点判定: {err:?}"
    );
}

#[test]
fn replace_text_via_json_boundary_keeps_cp_offsets() {
    // wasm 桥的 op 全部走 serde JSON：非 BMP insert 与码点偏移过 JSON 边界不变
    let mut core = Core::from_markdown("𠮷a🙂b");
    let json = r#"[{"t":"replace_text","path":[0],"start":1,"end":2,"insert":"👨‍👩‍👧‍👦"}]"#;
    let ops: Vec<Op> = serde_json::from_str(json).expect("serde parse");
    core.apply_ops(ops, ApplyMeta::view("json-boundary"))
        .unwrap();
    assert_eq!(node_text(&core, &[0]), format!("𠮷{ZWJ_FAMILY}🙂b"));

    // 事件流出回的 op 与入参一致（op 流即审计语言）
    let events = core.take_events();
    let last = events.last().unwrap();
    assert_eq!(
        last.ops[0],
        Op::ReplaceText {
            path: vec![0],
            start: 1,
            end: 2,
            insert: ZWJ_FAMILY.into()
        }
    );
}

#[test]
fn split_merge_non_bmp_offsets() {
    let mut core = Core::from_markdown("𠮷one🙂two"); // 8 码点
    let id_head = node_id(&core, &[0]);

    // 码点 5 处分段：左 "𠮷one🙂"（5 码点 / 6 UTF-16），右 "two"
    core.split_block(core.revision(), vec![0], 5).unwrap();
    assert_eq!(top_texts(&core), vec!["𠮷one🙂", "two"]);
    assert_eq!(node_id(&core, &[0]), id_head, "split 左保 ID");
    assert_ne!(node_id(&core, &[1]), id_head, "split 右新 ID");
    assert_eq!(tree_to_markdown(&core.tree), "𠮷one🙂\ntwo");

    // merge 回并：survivor = 前块（ID 保持），文本逐字符复原
    let ids2 = top_ids(&core);
    core.merge_block(core.revision(), vec![1]).unwrap();
    assert_eq!(node_text(&core, &[0]), "𠮷one🙂two");
    assert_eq!(node_id(&core, &[0]), id_head);
    assert_eq!(top_ids(&core), vec![id_head]);

    // undo 回到 split 后 → 再 undo 回到原始；redo 全程 ID/文本恒等
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["𠮷one🙂", "two"]);
    assert_eq!(top_ids(&core), ids2);
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["𠮷one🙂two"]);
    assert_eq!(top_ids(&core), vec![id_head]);
    core.redo().unwrap();
    core.redo().unwrap();
    assert_eq!(top_texts(&core), vec!["𠮷one🙂two"]);
    assert_eq!(top_ids(&core), vec![id_head]);
}

#[test]
fn split_list_item_non_bmp_inherits_marker_and_depth() {
    let mut core = Core::from_markdown("- item 𠮷 α\n  - nested 🙂 β");
    let ids = top_ids(&core);
    let nested_id = node_id(&core, &[0, 0]);
    let watermark_before = core.tree.alloc.watermark();
    // 嵌套 item 文本 "nested 🙂 β"（10 码点：nested=0..6 空格=6 🙂=7 空格=8 β=9）
    // 在码点 7 处分段。纯文本切分不修整空白：head 原样保留分隔空格，
    // tail = 原文 [7..) = "🙂 β"（非 BMP 起头的新块）
    core.split_block(core.revision(), vec![0, 0], 7).unwrap();
    assert_eq!(node_text(&core, &[0, 0]), "nested ");
    assert_eq!(node_text(&core, &[0, 1]), "🙂 β");
    let tail = &core.tree.node(core.tree.resolve(&[0, 1]).unwrap());
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
    assert!(
        tail.id.0 >= watermark_before,
        "tail 为新分配 ID（≥ 分配器水位）"
    );
    // 既有 item ID 不变（顶层 item 与被切分的嵌套 item 均保持原身份）
    assert_eq!(top_ids(&core)[0], ids[0]);
    assert_eq!(node_id(&core, &[0, 0]), nested_id);
    assert_eq!(
        tree_to_markdown(&core.tree),
        "- item 𠮷 α\n  - nested \n  - 🙂 β"
    );
}

#[test]
fn code_block_multiline_non_bmp_text_offsets() {
    // 多行 fence 体：行内偏移 = 块内码点偏移（\n 计 1 码点，与 UTF-16 一致）
    let mut core = core_no_coalesce("```js\n𠮷 = 1;\n🙂 = 2;\n```");
    assert_eq!(node_text(&core, &[0]), "𠮷 = 1;\n🙂 = 2;");
    // "𠮷 = 1;" = 6 码点，"\n" 计 1 → 第二行行首（🙂）在码点 7
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 7,
            end: 8,
            insert: "犬".into(),
        }],
        ApplyMeta::view("test"),
    )
    .unwrap();
    assert_eq!(node_text(&core, &[0]), "𠮷 = 1;\n犬 = 2;");
    // 行界编辑：跨 \n 替换 [5,8) = ";" + "\n" + "犬"
    core.apply_ops(
        vec![Op::ReplaceText {
            path: vec![0],
            start: 5,
            end: 8,
            insert: "; //😀\n𠮷".into(),
        }],
        ApplyMeta::view("test"),
    )
    .unwrap();
    assert_eq!(node_text(&core, &[0]), "𠮷 = 1; //😀\n𠮷 = 2;");
    core.undo().unwrap();
    core.undo().unwrap();
    assert_eq!(node_text(&core, &[0]), "𠮷 = 1;\n🙂 = 2;");
}

// ---------------------------------------------------------------------------
// ③ 锚点 (BlockId, offset) 在非 BMP 内的解析
// ---------------------------------------------------------------------------

#[test]
fn anchor_resolution_non_bmp() {
    let mut core = Core::from_markdown("𠮷🙂b"); // 3 码点 / 5 UTF-16 码元
    let id = node_id(&core, &[0]);

    // ByAnchor + expect：码点 [1,2) = "🙂"（若按 UTF-16 计则是半个代理对）
    core.replace_text(
        TextTarget::ByAnchor {
            block: id,
            start: 1,
            end: 2,
            expect: Some("🙂".into()),
        },
        "γ".into(),
    )
    .unwrap();
    assert_eq!(node_text(&core, &[0]), "𠮷γb");

    // OutOfRange 的 len 以码点报告（3），不是 UTF-16 码元（5）
    let err = core.replace_text(
        TextTarget::ByAnchor {
            block: id,
            start: 1,
            end: 6,
            expect: None,
        },
        "x".into(),
    );
    match err {
        Err(mdcore::Reject::Anchor(AnchorError::OutOfRange { block, len })) => {
            assert_eq!(block, id);
            assert_eq!(len, 3, "锚点长度单位 = 码点");
        }
        other => panic!("期望 OutOfRange，得到 {other:?}"),
    }

    // expect 内容保护按码点切取比较
    let err = core.replace_text(
        TextTarget::ByAnchor {
            block: id,
            start: 1,
            end: 2,
            expect: Some("✗".into()),
        },
        "x".into(),
    );
    assert!(matches!(
        err,
        Err(mdcore::Reject::Anchor(AnchorError::ExpectMismatch { .. }))
    ));

    // resolve_anchor：offset == len 合法（块尾）；超出 → OutOfRange
    assert!(core
        .resolve_anchor(&Anchor {
            block: id,
            offset: 3
        })
        .is_ok());
    assert!(matches!(
        core.resolve_anchor(&Anchor {
            block: id,
            offset: 4
        }),
        Err(AnchorError::OutOfRange { len: 3, .. })
    ));
}

// ---------------------------------------------------------------------------
// ④ sync_from_markdown（桥 fallback）在非 BMP 上的 ID 保持
// ---------------------------------------------------------------------------

#[test]
fn sync_from_markdown_non_bmp() {
    let mut core = Core::from_markdown("𠮷A\n🙂B\n👨‍👩‍👧‍👦C");
    let ids = top_ids(&core);
    // 首块原位改 + 尾部插入：对齐位保 ID、净增尾部新 ID（P1-4 规则在非 BMP 下同型）
    core.sync_from_markdown("𠮷A'\n🙂B\n👨‍👩‍👧‍👦C\n🦊D").unwrap();
    assert_eq!(top_texts(&core), vec!["𠮷A'", "🙂B", "👨‍👩‍👧‍👦C", "🦊D"]);
    let now = top_ids(&core);
    assert_eq!(now[0], ids[0], "对齐块保 ID");
    assert_eq!(now[1], ids[1]);
    assert_eq!(now[2], ids[2]);
    assert!(now[3].0 > ids[2].0, "净增块新 ID");

    // 中间块的 ReplaceText 偏移 = 码点（"𠮷A" = 2 码点）
    let events = core.take_events();
    let ops = &events.last().unwrap().ops;
    assert!(
        ops.iter().any(|o| matches!(
            o,
            Op::ReplaceText { path, start: 0, end: 2, .. } if path == &vec![0usize]
        )),
        "原位 ReplaceText 的 end = len_chars（码点）: {ops:?}"
    );

    // undo 后文本与 ID 恒等（P1-9 的 undo 恒等要求）
    core.undo().unwrap();
    assert_eq!(top_texts(&core), vec!["𠮷A", "🙂B", "👨‍👩‍👧‍👦C"]);
    assert_eq!(top_ids(&core), ids);
}

// ---------------------------------------------------------------------------
// ⑤ 性质测试：混合字母类别长文档 × 伪随机码点编辑 × undo/redo 恒等
// ---------------------------------------------------------------------------

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn property_mixed_scripts_edits_undo_redo_identity() {
    // 60 段 × 4 类字母系（ASCII / CJK+扩展B / emoji+ZWJ / 混合），全部以
    // "para N " 开头避免被解析成其他块型
    let alphabets: [&str; 4] = [
        "lorem ipsum dolor",
        "中文𠮷日本語",
        "😀🙂🦊👨‍👩‍👧‍👦🚀",
        "a𠮷b😀c中文d🙂",
    ];
    let mut mirror: Vec<String> = Vec::new();
    let mut src = String::new();
    for i in 0..60 {
        let line = format!("para {i} {}", alphabets[i % 4]);
        mirror.push(line.clone());
        src.push_str(&line);
        src.push('\n');
    }
    let mut core = Core::from_markdown(&src);
    assert_eq!(core.tree.top_level().len(), 60);

    // 时钟步进 10s/次 → 合并窗永不触发，每次编辑独立历史条目
    let tick = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let tick2 = tick.clone();
    core.set_clock(std::sync::Arc::new(move || {
        tick2.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed)
    }));

    // 往返恒等（非 BMP 全谱系）
    assert_eq!(tree_to_markdown(&core.tree), src.trim_end_matches('\n'));

    let original_texts = mirror.clone();
    let original_ids = top_ids(&core);

    // 80 次伪随机码点编辑（含纯插入 start==end 与跨非 BMP 边界的区间替换）
    let inserts = ["X", "𠮷", "😀", ZWJ_FAMILY, "中文", "→"];
    let mut rng: u64 = 0x517C_C1B5_27D3_4F1D;
    for _ in 0..80 {
        let idx = (xorshift(&mut rng) as usize) % 60;
        let insert = inserts[(xorshift(&mut rng) as usize) % inserts.len()];
        let len = mirror[idx].chars().count();
        let start = (xorshift(&mut rng) as usize) % (len + 1);
        let end = start + (xorshift(&mut rng) as usize) % (len - start + 1);
        core.apply_ops(
            vec![Op::ReplaceText {
                path: vec![idx],
                start,
                end,
                insert: insert.into(),
            }],
            ApplyMeta::view("property"),
        )
        .unwrap();
        mirror[idx] = cp_replace(&mirror[idx], start, end, insert);
    }
    assert_eq!(
        tree_to_markdown(&core.tree),
        mirror.join("\n"),
        "编辑后核文本 = 逐码点镜像"
    );

    // 全量 undo：文本与 ID 恒等（undo 粒度不受合并窗影响——时钟已隔离）
    let mut undos = 0;
    while core.undo().is_some() {
        undos += 1;
    }
    assert_eq!(undos, 80, "每次编辑一条历史条目");
    assert_eq!(top_texts(&core), original_texts);
    assert_eq!(top_ids(&core), original_ids, "undo 后身份复原");

    // 全量 redo：回到编辑后状态，文本恒等
    for _ in 0..undos {
        assert!(core.redo().is_some(), "redo 深度 = undo 深度");
    }
    assert_eq!(tree_to_markdown(&core.tree), mirror.join("\n"));
    assert_eq!(top_texts(&core), mirror);
}
