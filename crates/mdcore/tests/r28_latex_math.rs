//! r28 — LaTeX 数学承载面（#261：排期 R7 / #161 能力矩阵「LaTeX 支持」）。
//!
//! 覆盖面（对应 issue #261 验收矩阵）：
//!   a. `$$` 块级解析（fence 同构聚合 + 同行自含形态 + 悬挂吃到尾）；
//!   b. parse → serialize → parse 结构等价（canonical 输出 = fence 规范形；
//!      单行自含形态序列化后规范化，结构等价口径）；
//!   c. `is_complete_chunk` 悬挂判定（fence/math 状态机互斥——fence 内
//!      `$$` 行 / math 内 ``` 行不翻对面状态；fence 判定不 trim 原口径
//!      零漂移）；
//!   d. HTML 发射锚点（`<math-block>` 块 / `<math>` 行内——标签即锚点，
//!      textContent 即 LaTeX 源）+ 导入回读 round-trip + 转义安全；
//!   e. word_count / pagination / chunk 同构口径（LaTeX 源逐字计入）；
//!   f. op 面（SetBlockType ↔ Math / ReplaceText own-text）；
//!   g. fingerprint 判别值（Math = 8 新值，0-7 零漂移）。
//!
//! 行内解析器矩阵（`$...$` 真阳/价格假阳/转义/连跑/code 优先）在
//! `src/inline.rs` 内模块测试；docx 降级面在 `mdcore-wasm/src/docx_export.rs`
//! 内模块测试；协作域往返在 `mdbridge/tests/bridge_ops.rs`。

use mdcore::debug;
use mdcore::history::Source;
use mdcore::html::{html_to_tree, tree_to_html};
use mdcore::markdown::{is_complete_chunk, markdown_to_tree, tree_to_markdown};
use mdcore::tree::{BlockKind, BlockTree};
use mdcore::{ApplyMeta, Core};

/// 顶层块 kind 序。
fn kinds(tree: &BlockTree) -> Vec<BlockKind> {
    tree.top_level()
        .iter()
        .map(|&id| tree.node(id).kind)
        .collect()
}

/// 顶层块 text 序。
fn texts(tree: &BlockTree) -> Vec<String> {
    tree.top_level()
        .iter()
        .map(|&id| tree.node(id).text.to_string())
        .collect()
}

fn top_kind_texts(md: &str) -> (Vec<BlockKind>, Vec<String>) {
    let tree = markdown_to_tree(md);
    (kinds(&tree), texts(&tree))
}

// ---------------------------------------------------------------------------
// a. $$ 块级解析
// ---------------------------------------------------------------------------

#[test]
fn math_block_fence_form() {
    let (ks, ts) = top_kind_texts("前文\n$$\nE=mc^2\n$$\n后文");
    assert_eq!(
        ks,
        vec![BlockKind::Paragraph, BlockKind::Math, BlockKind::Paragraph]
    );
    assert_eq!(
        ts,
        vec!["前文".to_string(), "E=mc^2".to_string(), "后文".to_string()]
    );
}

#[test]
fn math_block_same_line_form() {
    let (ks, ts) = top_kind_texts("$$E=mc^2$$");
    assert_eq!(ks, vec![BlockKind::Math]);
    assert_eq!(ts, vec!["E=mc^2".to_string()]);
}

#[test]
fn math_block_same_line_form_with_spaces() {
    // 行首尾缘空白容忍（trim 后判定），内容保形（含空格）。
    let (ks, ts) = top_kind_texts("  $$ x = 1 $$  ");
    assert_eq!(ks, vec![BlockKind::Math]);
    assert_eq!(ts, vec![" x = 1 ".to_string()]);
}

#[test]
fn math_block_multiline_latex() {
    // 多行 LaTeX（aligned 环境）逐字保留。
    let src = "$$\n\\begin{aligned}\n  a &= b + c \\\\\n  d &= e\n\\end{aligned}\n$$";
    let (ks, ts) = top_kind_texts(src);
    assert_eq!(ks, vec![BlockKind::Math]);
    assert_eq!(
        ts,
        vec!["\\begin{aligned}\n  a &= b + c \\\\\n  d &= e\n\\end{aligned}".to_string()]
    );
}

#[test]
fn math_block_empty() {
    let (ks, ts) = top_kind_texts("$$\n$$");
    assert_eq!(ks, vec![BlockKind::Math]);
    assert_eq!(ts, vec![String::new()]);
}

#[test]
fn math_block_unclosed_fence_consumes_rest() {
    // 悬挂 fence 同构：无闭合行 → 内容吃到文档尾（fence 行为镜像）。
    let (ks, ts) = top_kind_texts("$$\nE=1\nstill math");
    assert_eq!(ks, vec![BlockKind::Math]);
    assert_eq!(ts, vec!["E=1\nstill math".to_string()]);
}

#[test]
fn math_block_non_boundary_lines_fall_to_paragraph() {
    // `$$ x`（开行无收界）与 `$$$$`（空自含）不猜——落段落字面量。
    let (ks, ts) = top_kind_texts("$$ x\n$$$$\n$5");
    assert_eq!(
        ks,
        vec![
            BlockKind::Paragraph,
            BlockKind::Paragraph,
            BlockKind::Paragraph
        ]
    );
    assert_eq!(
        ts,
        vec!["$$ x".to_string(), "$$$$".to_string(), "$5".to_string()]
    );
}

#[test]
fn math_block_crlf_input_normalized() {
    // TR-1 同构：CRLF 行尾规范化后解析（Windows 粘贴面）。
    let (ks, ts) = top_kind_texts("$$\r\nE=mc^2\r\n$$\r\n");
    assert_eq!(ks, vec![BlockKind::Math]);
    assert_eq!(ts, vec!["E=mc^2".to_string()]);
}

#[test]
fn math_block_mixed_document_structure() {
    // 混排文档：段落 / math / 代码块 / math 的结构序与内容。
    let (ks, ts) = top_kind_texts("# 标题\n$$\n公式一\n$$\n```rust\nfn main() {}\n```\n$$公式二$$");
    assert_eq!(
        ks,
        vec![
            BlockKind::Heading,
            BlockKind::Math,
            BlockKind::CodeBlock,
            BlockKind::Math,
        ]
    );
    assert_eq!(
        ts,
        vec![
            "标题".to_string(),
            "公式一".to_string(),
            "fn main() {}".to_string(),
            "公式二".to_string(),
        ]
    );
}

// ---------------------------------------------------------------------------
// b. round-trip 结构等价（canonical = fence 规范形）
// ---------------------------------------------------------------------------

#[test]
fn math_roundtrip_structure_equivalence() {
    // fence 形态：parse → serialize → parse 全等。
    let src = "前文\n$$\nE=mc^2\n$$\n后文";
    let t1 = markdown_to_tree(src);
    let md = tree_to_markdown(&t1);
    assert_eq!(md, src, "fence 规范形序列化逐字还原");
    let t2 = markdown_to_tree(&md);
    assert_eq!(debug::dump_tree(&t2), debug::dump_tree(&t1), "结构等价");
}

#[test]
fn math_roundtrip_single_line_normalized_to_fence() {
    // 单行自含形态 → 序列化规范化为 fence 形（结构等价口径）。
    let t1 = markdown_to_tree("$$E=mc^2$$");
    let md = tree_to_markdown(&t1);
    assert_eq!(md, "$$\nE=mc^2\n$$", "单行形态规范化为 fence");
    let t2 = markdown_to_tree(&md);
    assert_eq!(kinds(&t2), kinds(&t1));
    assert_eq!(texts(&t2), texts(&t1));
}

#[test]
fn math_roundtrip_multiline_and_cjk() {
    for src in ["$$\n\\sum_{i=1}^{n} 总分_i\n$$", "$$中文公式$$", "$$\n$$"] {
        let t1 = markdown_to_tree(src);
        let md = tree_to_markdown(&t1);
        let t2 = markdown_to_markdown_back(&md);
        assert_eq!(debug::dump_tree(&t2), debug::dump_tree(&t1), "src={src:?}");
    }
}

fn markdown_to_markdown_back(md: &str) -> BlockTree {
    markdown_to_tree(&tree_to_markdown(&markdown_to_tree(md)))
}

// ---------------------------------------------------------------------------
// c. is_complete_chunk 悬挂判定
// ---------------------------------------------------------------------------

#[test]
fn chunk_completeness_math_states() {
    // math 开无闭 → 悬挂（fence 同构）。
    assert!(!is_complete_chunk("前文\n$$\nE=1\n"));
    // 闭合 → 完整。
    assert!(is_complete_chunk("$$\nE=1\n$$"));
    // 同行自含 → 完整（无悬挂态）。
    assert!(is_complete_chunk("$$E=1$$"));
    // 既有 fence 行为零漂移锚点。
    assert!(!is_complete_chunk("```\ncode"));
    assert!(is_complete_chunk("```\ncode\n```"));
}

#[test]
fn chunk_completeness_state_machine_mutual_exclusion() {
    // fence 内的 `$$` 行：内容逐字，不翻 math 态 → chunk 完整。
    assert!(is_complete_chunk("```\n$$\n```"));
    // math 内的 ``` 行：内容逐字，不翻 fence 态 → chunk 完整。
    assert!(is_complete_chunk("$$\n```\n$$"));
    // 混合串：fence 内 $$（不动 math）+ 独立 math 完整闭合。
    assert!(is_complete_chunk("```\n$$\n```\n$$\nx\n$$"));
    // 交叉悬挂：fence 开着（收尾行不是 fence 开头）→ 悬挂。
    assert!(!is_complete_chunk("```\n$$\n还在 fence 里"));
}

// ---------------------------------------------------------------------------
// d. HTML 发射锚点 + 导入回读
// ---------------------------------------------------------------------------

#[test]
fn html_export_block_math_anchor() {
    // 块锚点：<math-block>，textContent 即 LaTeX 源（消费方
    // katex.render(el.textContent, el) 直接可得）。
    let tree = markdown_to_tree("$$\nE=mc^2\n$$");
    let html = tree_to_html(&tree);
    assert_eq!(html, "<math-block>E=mc^2</math-block>\n");
}

#[test]
fn html_export_block_math_escape_safety() {
    // LaTeX 源含 HTML 敏感字符：导出转义、导入解码回读（往返零丢失）。
    let tree = markdown_to_tree("$$\na < b & c > d\n$$");
    let html = tree_to_html(&tree);
    assert!(html.contains("a &lt; b &amp; c &gt; d"), "html={html}");
    let back = html_to_tree(&html);
    assert_eq!(kinds(&back), vec![BlockKind::Math]);
    assert_eq!(texts(&back), vec!["a < b & c > d".to_string()]);
}

#[test]
fn html_import_block_math_roundtrip() {
    let html = "<p>前文</p>\n<math-block>x = \\frac{1}{2}</math-block>\n<p>后文</p>\n";
    let tree = html_to_tree(html);
    assert_eq!(
        kinds(&tree),
        vec![BlockKind::Paragraph, BlockKind::Math, BlockKind::Paragraph]
    );
    assert_eq!(texts(&tree)[1], "x = \\frac{1}{2}");
    // 导出再生：锚点形态稳定。
    let out = tree_to_html(&tree);
    assert!(
        out.contains("<math-block>x = \\frac{1}{2}</math-block>"),
        "out={out}"
    );
}

#[test]
fn html_import_math_block_leading_newline_kept() {
    // 与 pre 的差异锚点：不剥前导换行（机器格式惯例；pre 按 HTML5 剥）。
    let tree = html_to_tree("<math-block>\nx\n</math-block>");
    assert_eq!(texts(&tree), vec!["\nx\n".to_string()]);
}

#[test]
fn html_inline_math_mark_roundtrip() {
    // 行内锚点：<math> 标签（mark kind 即标签名——零特判面）。
    // 注意：markdown 解析不产 marks（源文本层——`**粗**` 同口径），
    // 行内 math mark 的 html 面走「导入建 marks / op 建 marks」两路。
    let tree = html_to_tree("<p>能量 <math>E=mc^2</math> 守恒</p>");
    let node = tree.node(tree.top_level()[0]);
    assert_eq!(node.text.to_string(), "能量 E=mc^2 守恒");
    assert_eq!(node.marks.len(), 1);
    assert_eq!(node.marks[0].kind, "math");
    assert_eq!((node.marks[0].start, node.marks[0].end), (3, 9));
    // 导出再生：锚点形态稳定。
    let out = tree_to_html(&tree);
    assert!(out.contains("<math>E=mc^2</math>"), "out={out}");
}

#[test]
fn html_inline_math_wild_mathml_flattens() {
    // 野生 MathML `<math>` 同词汇导入：文本扁平化入 math mark，结构丢弃。
    let tree = html_to_tree("<p><math><mrow><mi>x</mi><mo>+</mo><mn>1</mn></mrow></math></p>");
    let node = tree.node(tree.top_level()[0]);
    assert_eq!(node.text.to_string(), "x+1");
    assert_eq!(node.marks.len(), 1);
    assert_eq!(node.marks[0].kind, "math");
}

#[test]
fn html_full_document_roundtrip_with_math() {
    // DM → HTML → DM 全等（r13 assert_roundtrip_identity 同构口径）。
    let src = "# 数学章\n\n$$\n\\int_0^1 x\\,dx = \\frac{1}{2}\n$$\n\n正文段落。\n";
    let dm = markdown_to_tree(src);
    let html = tree_to_html(&dm);
    let back = html_to_tree(&html);
    assert_eq!(debug::dump_tree(&back), debug::dump_tree(&dm));
    assert_eq!(tree_to_markdown(&back), tree_to_markdown(&dm));
}

// ---------------------------------------------------------------------------
// e. word_count / pagination / chunk 同构口径
// ---------------------------------------------------------------------------

#[test]
fn word_count_math_verbatim_same_as_codeblock() {
    // 数学文本逐字计入（CodeBlock 同构）：四项计数全等。
    let math = markdown_to_tree("$$\n能量E等于mc平方\n$$");
    let code = markdown_to_tree("```\n能量E等于mc平方\n```");
    let m = mdcore::word_count::count(&math);
    let c = mdcore::word_count::count(&code);
    assert_eq!(
        (m.cjk_chars, m.western_words, m.total_words, m.total_chars),
        (c.cjk_chars, c.western_words, c.total_words, c.total_chars)
    );
    // 段落路径对照：同文本作段落时计入口径相同（逐字面）。
    let para = markdown_to_tree("能量E等于mc平方");
    let p = mdcore::word_count::count(&para);
    assert_eq!(m.total_chars, p.total_chars);
}

#[test]
fn chunk_projection_math_verbatim_not_inline_stripped() {
    // chunk 投影（裁决 3）：Math 文本逐字（定界符是字面内容），
    // 与段落路径（行内定界符剥除）形成对照。注：单块恒产父子两级
    // chunk（既有结构，CodeBlock 同构），断言取全体 chunk 文本。
    let policy = mdcore::chunk::ChunkPolicy::default();
    let math_core = Core::from_markdown("$$\na *b* c\n$$");
    let chunks = math_core.chunk_document(&policy).expect("chunks");
    assert!(!chunks.is_empty());
    assert!(
        chunks.iter().all(|c| c.text == "a *b* c"),
        "Math 投影逐字（CodeBlock 同构）：{:?}",
        chunks.iter().map(|c| c.text.clone()).collect::<Vec<_>>()
    );

    let para_core = Core::from_markdown("a *b* c");
    let chunks = para_core.chunk_document(&policy).expect("chunks");
    assert!(
        chunks.iter().all(|c| c.text == "a b c"),
        "段落投影剥行内定界符（em 派生）：{:?}",
        chunks.iter().map(|c| c.text.clone()).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// f. op 面：SetBlockType / ReplaceText / undo
// ---------------------------------------------------------------------------

/// Core 的 markdown 投影（to_markdown 同构面——wasm 入口封装在此）。
fn md_of(core: &Core) -> String {
    tree_to_markdown(&core.tree)
}

fn apply(core: &mut Core, ops: Vec<mdcore::op::Op>) {
    core.apply_ops(
        ops,
        ApplyMeta {
            source: Source::View,
            command: Some("test".into()),
        },
    )
    .expect("apply_ops");
}

#[test]
fn op_set_block_type_to_math_and_back() {
    let mut core = Core::from_markdown("普通段落");
    apply(
        &mut core,
        vec![mdcore::op::Op::SetBlockType {
            path: vec![0],
            kind: BlockKind::Math,
        }],
    );
    let md = md_of(&core);
    assert_eq!(md, "$$\n普通段落\n$$", "段落 → 数学块（序列化 fence 形）");
    apply(
        &mut core,
        vec![mdcore::op::Op::SetBlockType {
            path: vec![0],
            kind: BlockKind::Paragraph,
        }],
    );
    assert_eq!(md_of(&core), "普通段落");
}

#[test]
fn op_replace_text_math_own_text() {
    // Math 是 own-text 叶块：ReplaceText 合法（CodeBlock 同构）。
    let mut core = Core::from_markdown("$$\n旧公式\n$$");
    apply(
        &mut core,
        vec![mdcore::op::Op::ReplaceText {
            path: vec![0],
            start: 0,
            end: 3,
            insert: "新公式".into(),
        }],
    );
    assert_eq!(md_of(&core), "$$\n新公式\n$$");
}

#[test]
fn op_undo_math_type_change() {
    let mut core = Core::from_markdown("可撤销段");
    apply(
        &mut core,
        vec![mdcore::op::Op::SetBlockType {
            path: vec![0],
            kind: BlockKind::Math,
        }],
    );
    apply(
        &mut core,
        vec![mdcore::op::Op::SetBlockType {
            path: vec![0],
            kind: BlockKind::Paragraph,
        }],
    );
    core.undo();
    assert_eq!(md_of(&core), "$$\n可撤销段\n$$", "undo 恢复数学块");
    core.undo();
    assert_eq!(md_of(&core), "可撤销段");
}

// ---------------------------------------------------------------------------
// g. fingerprint 判别值（Math = 8；0-7 零漂移）
// ---------------------------------------------------------------------------

#[test]
fn fingerprint_math_uses_new_tag_and_is_deterministic() {
    use mdcore::fingerprint::TreeFingerprint;
    let math = markdown_to_tree("$$\n内容\n$$");
    let para = markdown_to_tree("内容");
    let mut fm_math = TreeFingerprint::new(&math);
    let mut fm_para = TreeFingerprint::new(&para);
    let f_math = fm_math.structural_of(math.top_level()[0]);
    let f_para = fm_para.structural_of(para.top_level()[0]);
    // 同文本不同 kind → 结构指纹不同（kind_tag 参与哈希；Math = 8 新值）。
    assert_ne!(f_math, f_para, "kind 判别值必须区分 Math/Paragraph");
    // 确定性：重复计算稳定。
    let mut fm2 = TreeFingerprint::new(&math);
    assert_eq!(fm2.structural_of(math.top_level()[0]), f_math);
    // 判别值不与既有 0-7 撞：Math vs CodeBlock 同文本同属性也不同。
    let code = markdown_to_tree("```\n内容\n```");
    let mut fm_code = TreeFingerprint::new(&code);
    assert_ne!(
        fm_math.structural_of(math.top_level()[0]),
        fm_code.structural_of(code.top_level()[0])
    );
}
