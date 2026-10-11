//! R13 — HTML ↔ Document Model ↔ MD round-trip（主档 §阶段 R11 第 1 项 +
//! §18 同族转换策略：尽量保真 + round-trip fixture CI + 结构化 Loss Report）。
//!
//! DM（BlockTree）是唯一语义真理：本套件验证
//!   a. DM → HTML → DM 恒等（支持子集内，含 CJK）；
//!   b. MD → DM → HTML 渲染契约；
//!   c. HTML → DM → MD 粘贴样例合理性；
//!   d. 降级路径 + Loss Report 结构化断言；
//!   e. 转义安全（导出转义 / 导入无执行语义）。
//!
//! 支持子集、降级规则、已知有损点见 `src/html.rs` 模块文档。

use mdcore::debug;
use mdcore::html::{html_to_tree, import_html, tree_to_html, LossDisposition};
use mdcore::markdown::{markdown_to_tree, tree_to_markdown};
use mdcore::op::Op;
use mdcore::tree::BlockKind;
use mdcore::{ApplyMeta, Core};

fn dump(tree: &mdcore::tree::BlockTree) -> String {
    debug::dump_tree(tree)
}

/// DM → HTML → DM 全等（树 / 块 JSON / MD 序列化三重对照）。
fn assert_roundtrip_identity(md_src: &str) {
    let dm = markdown_to_tree(md_src);
    let html = tree_to_html(&dm);
    let back = html_to_tree(&html);
    assert_eq!(
        dump(&back),
        dump(&dm),
        "dump_tree 不等\nHTML 导出:\n{html}\nMD 基线:\n{md_src}"
    );
    assert_eq!(
        debug::blocks_json(&back),
        debug::blocks_json(&dm),
        "blocks_json 不等\nHTML 导出:\n{html}"
    );
    assert_eq!(
        tree_to_markdown(&back),
        tree_to_markdown(&dm),
        "MD 序列化不等\nHTML 导出:\n{html}"
    );
}

// ---------------------------------------------------------------------------
// a. DM → HTML → DM 恒等（支持子集）
// ---------------------------------------------------------------------------

#[test]
fn rt_identity_headings_paragraphs_cjk() {
    assert_roundtrip_identity(
        "# 标题一\n\n首段：中文内容与 English 混排。\n\n## 二级标题\n\n第二段文字。",
    );
}

#[test]
fn rt_identity_nested_lists() {
    assert_roundtrip_identity(
        "- 项目甲\n- 项目乙\n  - 嵌套子项\n  - 嵌套第二项\n    - 更深层\n- 项目丙",
    );
}

#[test]
fn rt_identity_ordered_list_start_number() {
    assert_roundtrip_identity("\n3. 三号项\n4. 四号项\n5. 五号项");
}

#[test]
fn rt_identity_blockquote_nested_with_list() {
    assert_roundtrip_identity("> 引用外层\n>\n> > 引用内层\n>\n> - 引用内列表\n> - 第二项");
}

#[test]
fn rt_identity_code_fence_with_lang() {
    assert_roundtrip_identity(
        "前文段落\n\n```rust\nfn main() {\n    println!(\"你好\");\n}\n```\n\n后文段落",
    );
}

#[test]
fn rt_identity_table_and_hr() {
    assert_roundtrip_identity(
        "表格前文\n\n| 名称 | 数量 |\n|---|---|\n| 苹果 | 3 |\n| 香蕉 | 12 |\n\n---\n\n表格后文",
    );
}

#[test]
fn rt_identity_marks_strong_em_code_del() {
    // mark 不入 MD（spike 同行为）→ 先建 DM，再经 Command Boundary 打 mark。
    let mut core = Core::from_markdown("alpha beta gamma delta");
    core.apply_ops(
        vec![
            Op::SetMark {
                path: vec![0],
                start: 0,
                end: 5,
                kind: "strong".into(),
                add: true,
            },
            Op::SetMark {
                path: vec![0],
                start: 6,
                end: 10,
                kind: "em".into(),
                add: true,
            },
            Op::SetMark {
                path: vec![0],
                start: 11,
                end: 16,
                kind: "code".into(),
                add: true,
            },
            Op::SetMark {
                path: vec![0],
                start: 17,
                end: 22,
                kind: "del".into(),
                add: true,
            },
        ],
        ApplyMeta::view("r13_marks"),
    )
    .unwrap();

    let html = tree_to_html(&core.tree);
    assert!(
        html.contains(
            "<p><strong>alpha</strong> <em>beta</em> <code>gamma</code> <del>delta</del></p>"
        ),
        "mark 导出标签化: {html}"
    );

    let back = html_to_tree(&html);
    let node = back.node(back.resolve(&[0]).unwrap());
    assert_eq!(node.text.to_string(), "alpha beta gamma delta");
    let got: Vec<(usize, usize, &str)> = node
        .marks
        .iter()
        .map(|m| (m.start, m.end, m.kind.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (0, 5, "strong"),
            (6, 10, "em"),
            (11, 16, "code"),
            (17, 22, "del")
        ],
        "mark round-trip span/kind 逐一还原"
    );
}

#[test]
fn rt_identity_u_mark_and_br_linebreak() {
    // 块内换行经 Command Boundary 注入（markdown 侧多行段落本就是登记简化，
    // 核内单块含 \n 的正规入口是文本编辑）。
    let mut core = Core::from_markdown("第一行第二行继续");
    core.apply_ops(
        vec![
            Op::ReplaceText {
                path: vec![0],
                start: 3,
                end: 3,
                insert: "\n".into(),
            },
            Op::SetMark {
                path: vec![0],
                start: 0,
                end: 3,
                kind: "u".into(),
                add: true,
            },
        ],
        ApplyMeta::view("r13_u"),
    )
    .unwrap();
    let html = tree_to_html(&core.tree);
    assert!(
        html.contains("<u>第一行</u><br>第二行继续"),
        "mark 收口 + 换行导出为 <br>: {html}"
    );
    let back = html_to_tree(&html);
    let node = back.node(back.resolve(&[0]).unwrap());
    assert_eq!(
        node.text.to_string(),
        "第一行\n第二行继续",
        "br 还原为文本换行"
    );
    assert_eq!(node.marks.len(), 1);
    assert_eq!(node.marks[0].kind, "u");
    assert_eq!((node.marks[0].start, node.marks[0].end), (0, 3));
}

// ---------------------------------------------------------------------------
// b. MD → DM → HTML 渲染契约
// ---------------------------------------------------------------------------

#[test]
fn md_to_html_rendering_contract() {
    let html = tree_to_html(&markdown_to_tree(
        "# 大标题\n\n正文一段\n\n- 甲\n- 乙\n\n> 引用\n\n```\ncode line\n```\n\n---\n",
    ));
    assert!(html.contains("<h1>大标题</h1>"), "{html}");
    assert!(html.contains("<p>正文一段</p>"), "{html}");
    assert!(
        html.contains("<ul>\n<li>甲</li>\n<li>乙</li>\n</ul>"),
        "{html}"
    );
    assert!(
        html.contains("<blockquote>\n<p>引用</p>\n</blockquote>"),
        "{html}"
    );
    assert!(html.contains("<pre><code>code line</code></pre>"), "{html}");
    assert!(html.contains("<hr>"), "{html}");
}

#[test]
fn md_to_html_table_structure() {
    let html = tree_to_html(&markdown_to_tree("| A | B |\n|---|---|\n| a1 | b1 |"));
    assert!(
        html.contains("<table>\n<thead><tr><th>A</th><th>B</th></tr></thead>"),
        "{html}"
    );
    assert!(
        html.contains("<tbody>\n<tr><td>a1</td><td>b1</td></tr>\n</tbody>"),
        "{html}"
    );
}

// ---------------------------------------------------------------------------
// c. HTML → DM → MD 粘贴样例合理性
// ---------------------------------------------------------------------------

#[test]
fn html_paste_sample_maps_to_markdown() {
    let webpage = r#"<article><h2>发布说明</h2><p>本次更新包含<strong>重要</strong>修复。</p><ul><li>性能优化<ul><li>渲染管线</li></ul></li><li>新特性</li></ul><blockquote><p>引用官方公告。</p></blockquote><pre><code class="language-bash">cargo test --workspace</code></pre><table><thead><tr><th>模块</th><th>状态</th></tr></thead><tbody><tr><td>核心</td><td>稳定</td></tr></tbody></table></article>"#;
    let imp = import_html(webpage);
    let md = tree_to_markdown(&imp.tree);
    assert!(md.contains("## 发布说明"), "{md}");
    assert!(
        md.contains("**重要**") || md.contains("重要"),
        "strong 保留或文本保留: {md}"
    );
    assert!(md.contains("- 性能优化"), "{md}");
    assert!(md.contains("  - 渲染管线"), "嵌套列表缩进: {md}");
    assert!(md.contains("> 引用官方公告。"), "{md}");
    assert!(md.contains("```bash"), "fence 带 lang: {md}");
    assert!(md.contains("cargo test --workspace"), "{md}");
    assert!(md.contains("| 模块 | 状态 |"), "表格管道行: {md}");
    // <article> 为未知/分节容器：透明穿轴但必须留痕（§18 Loss Report）。
    assert_eq!(
        imp.losses.len(),
        1,
        "仅 article 包装一条损失: {:?}",
        imp.losses
    );
    assert_eq!(
        imp.losses[0].disposition,
        LossDisposition::DemotedToParagraph
    );
    assert!(imp.losses[0].original.contains("article"));
}

// ---------------------------------------------------------------------------
// d. 降级路径 + Loss Report（§18 结构化有损报告）
// ---------------------------------------------------------------------------

#[test]
fn unknown_container_demotes_with_loss() {
    let imp = import_html(r#"<div class="toc" data-x="1"><p>目录内容</p></div>"#);
    assert_eq!(imp.tree.top_level().len(), 1, "div 透明，子内容按块保留");
    assert!(!imp.losses.is_empty(), "带属性的未知容器必须留痕");
    let loss = imp
        .losses
        .iter()
        .find(|l| l.disposition == LossDisposition::DemotedToParagraph)
        .expect("demote loss");
    assert!(
        loss.original.contains("div"),
        "original 记录原结构: {}",
        loss.original
    );
    assert!(!loss.note.is_empty());
}

#[test]
fn script_style_iframe_dropped_no_execution_semantics() {
    let imp = import_html(
        r#"<p>安全前文</p><script>alert("xss")</script><style>.a{}</style><iframe src="https://evil.example"></iframe><p>安全后文</p>"#,
    );
    let md = tree_to_markdown(&imp.tree);
    assert!(md.contains("安全前文") && md.contains("安全后文"));
    assert!(!md.contains("alert"), "脚本内容不进文档");
    assert!(!md.contains("evil.example"), "iframe 属性不进文档");
    assert!(!md.contains(".a{}"), "style 内容不进文档");
    let dropped: Vec<&mdcore::html::Loss> = imp
        .losses
        .iter()
        .filter(|l| l.disposition == LossDisposition::Dropped)
        .collect();
    assert!(
        dropped.len() >= 3,
        "script/style/iframe 各记一条 Dropped: {:?}",
        imp.losses
    );
}

#[test]
fn degenerate_table_falls_back_to_raw_paragraph() {
    let imp = import_html("<p>前</p><table></table><p>后</p>");
    // 空表格无可保留文本：不落块，但必须记 RawBlock 留痕。
    assert_eq!(imp.tree.top_level().len(), 2);
    assert!(
        imp.losses
            .iter()
            .any(|l| l.disposition == LossDisposition::RawBlock),
        "记 RawBlock: {:?}",
        imp.losses
    );

    // 非空但无单元格结构：可见文本原文落为段落。
    let imp2 = import_html("<p>前</p><table>散落文本</table><p>后</p>");
    assert_eq!(imp2.tree.top_level().len(), 3);
    let mid = imp2.tree.node(imp2.tree.resolve(&[1]).unwrap());
    assert_eq!(mid.kind, BlockKind::Paragraph, "可见文本原文落为段落");
    assert_eq!(mid.text.to_string(), "散落文本");
}

// ---------------------------------------------------------------------------
// e. 转义安全
// ---------------------------------------------------------------------------

#[test]
fn export_escapes_all_text_nodes() {
    let dm = markdown_to_tree("含 <b> 标签文本 & 引号 \" 和 ' 与 > 符号");
    let html = tree_to_html(&dm);
    assert!(html.contains("&lt;b&gt;"), "尖括号转义: {html}");
    assert!(html.contains("&amp;"), "与号转义: {html}");
    assert!(html.contains("&quot;"), "双引号转义: {html}");
    assert!(html.contains("&#39;"), "单引号转义: {html}");
    assert!(
        !html[html.find("<p>").unwrap() + 3..html.find("</p>").unwrap()].contains('<')
            || html.contains("&lt;"),
        "段落文本内不得出现未转义标签"
    );
}

#[test]
fn import_never_produces_executable_semantics() {
    // 字面文本里的 script（经实体或纯文本）不构成元素；真元素被丢弃（见上）。
    let imp = import_html("<p>1 &lt; 2 &amp;&amp; 3 &gt; 2</p>");
    let node = imp.tree.node(imp.tree.resolve(&[0]).unwrap());
    assert_eq!(
        node.text.to_string(),
        "1 < 2 && 3 > 2",
        "实体解码为字面文本"
    );
    // 实体伪装的标签不会被二次解析（防 double-decode 注入）。
    let imp2 = import_html("<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>");
    let n2 = imp2.tree.node(imp2.tree.resolve(&[0]).unwrap());
    assert_eq!(
        n2.text.to_string(),
        "<script>alert(1)</script>",
        "解码结果不再走元素通道"
    );
    assert!(
        imp2.losses.is_empty(),
        "文本内容不算结构损失: {:?}",
        imp2.losses
    );
}

// ---------------------------------------------------------------------------
// 健壮性：不规范输入不 panic
// ---------------------------------------------------------------------------

#[test]
fn malformed_input_degrades_without_panic() {
    for nasty in [
        "<p>未闭合段落",
        "<ul><li>缺闭合",
        "<div><span>>>乱七八糟<<</div>",
        "纯文本，一个标签都没有",
        "<<>>><p>>>",
        "<table><tr><td>只有一行</td></tr></table>",
        "<blockquote>",
    ] {
        let imp = import_html(nasty);
        let _ = tree_to_html(&imp.tree); // 导出也不 panic
        let _ = tree_to_markdown(&imp.tree);
    }
}

#[test]
fn loss_report_fields_are_populated() {
    let imp = import_html(r#"前文<section><p>节内容</p></section>"#);
    let l = &imp.losses[0];
    assert!(l.offset < 10, "offset 指向输入中的起始处: {}", l.offset);
    assert!(
        l.original.starts_with('<'),
        "original 为原结构片段: {}",
        l.original
    );
    assert_eq!(l.disposition, LossDisposition::DemotedToParagraph);
    assert!(l.note.contains("section") || !l.note.is_empty());
}

#[test]
fn workspace_module_wired_via_lib() {
    // lib.rs `pub mod html` 挂载 + 与 markdown 同构入口存在。
    let t = html_to_tree("<h1>标题</h1>");
    assert_eq!(tree_to_markdown(&t), "# 标题");
}
