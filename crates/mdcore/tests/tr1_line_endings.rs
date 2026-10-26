//! TR-1（issue #9）行尾镜像一致性锁——`__md()` 与 CM6 视图 CR 镜像修复。
//!
//! 修复前缺陷：`markdown_to_tree` 以 `split('\n')` 切行，CRLF / 孤立 CR
//! 输入的行尾 `\r` 直接进入块文本（模型持有 CR）；CM6 视图输入即规范化
//! LF → 宿主经 `__md()`（= `to_markdown()` 投影，Ctrl+S 契约路径）保存
//! 的内容与用户所见不同（Windows 粘贴文本常见）。
//!
//! 修复（单点，`markdown.rs::markdown_to_tree` 入口）：CRLF / 孤立 CR
//! 统一规范化为 LF（CommonMark 规范三种行尾等价）——模型 = 权威层持有
//! LF 规范形，视图 / 投影 / 导出三面自然一致。三条解析入口
//! （from_markdown / sync_from_markdown / extend_from_markdown）全部
//! 漏斗经该函数，单点覆盖。
//!
//! 锁清单：
//! 1. CRLF 段落文本零 \r（issue 复现形：`"line one\r\nline two\r\n"`）；
//! 2. 孤立 CR（旧 Mac 形）同样按行尾处理切分；
//! 3. 写读全环（`tree_to_markdown` 投影）输出零 \r —— `__md()` 镜像
//!    保证（保存内容 = 所见）；
//! 4. 代码块 / 标题 / 表格内部 CRLF 同样规范化（含 lang 属性面）；
//! 5. `sync_from_markdown` 与 `extend_from_markdown` 两条追加/重同步
//!    入口与主入口行为一致；
//! 6. 纯 LF 快路径行为零变化（无 \r 输入不进规范化分支）。

use mdcore::markdown::{markdown_to_tree, tree_to_markdown};
use mdcore::tree::{BlockKind, BlockTree};
use mdcore::Core;

fn top_texts(tree: &BlockTree) -> Vec<String> {
    tree.top_level()
        .iter()
        .map(|&c| tree.node(c).text.to_string())
        .collect()
}

#[test]
fn tr1_crlf_paragraph_texts_are_lf_canonical() {
    // issue #9 复现形：修复前文本 = "line one\r" / "line two\r"
    let tree = markdown_to_tree("line one\r\nline two\r\n");
    assert_eq!(top_texts(&tree), vec!["line one", "line two"]);
}

#[test]
fn tr1_lone_cr_is_a_line_ending() {
    // 旧 Mac 形：孤立 CR 与 CRLF/LF 等价（CommonMark 行尾规范）
    let tree = markdown_to_tree("line one\rline two\r");
    assert_eq!(top_texts(&tree), vec!["line one", "line two"]);
}

#[test]
fn tr1_roundtrip_projection_has_zero_cr() {
    // __md() 投影路径（= tree_to_markdown）——保存内容与 CM6 所见一致
    let md = "line one\r\nline two\r\n";
    let out = tree_to_markdown(&markdown_to_tree(md));
    assert!(!out.contains('\r'), "projection must be CR-free: {out:?}");
    assert_eq!(out, "line one\nline two");
}

#[test]
fn tr1_crlf_inside_code_fence_heading_and_table() {
    let tree = markdown_to_tree("```rust\r\nlet x = 1;\r\nlet y = 2;\r\n```\r\n# Title\r\n| A | B |\r\n|---|---|\r\n| a1 | b1 |\r\n");
    let texts = top_texts(&tree);
    assert_eq!(texts.len(), 3, "code + heading + table");
    // 代码块内部行以 LF 连接，lang 属性干净
    assert_eq!(texts[0], "let x = 1;\nlet y = 2;");
    let code = tree.node(tree.top_level()[0]);
    assert_eq!(code.attrs.get("lang").map(String::as_str), Some("rust"));
    // 标题文本零 \r
    assert_eq!(texts[1], "Title");
    assert_eq!(tree.node(tree.top_level()[1]).kind, BlockKind::Heading);
    // 表格行合并零 \r
    assert_eq!(texts[2], "| A | B |\n|---|---|\n| a1 | b1 |");
}

#[test]
fn tr1_sync_and_extend_entry_points_mirror_main_entry() {
    // sync_from_markdown：重同步入口
    let mut core = Core::from_markdown("");
    core.sync_from_markdown("a\r\nb\r\n").expect("sync ok");
    assert_eq!(tree_to_markdown(&core.tree), "a\nb");

    // extend_from_markdown：渐进追加入口
    let mut core2 = Core::from_markdown("a\r\n");
    core2.extend_from_markdown("b\r\n").expect("extend ok");
    assert_eq!(tree_to_markdown(&core2.tree), "a\nb");
}

#[test]
fn tr1_lf_fast_path_unchanged() {
    // 纯 LF 输入不进规范化分支（快路径），行为与修复前逐字节一致
    let tree = markdown_to_tree("line one\nline two\n");
    assert_eq!(top_texts(&tree), vec!["line one", "line two"]);
    assert_eq!(tree_to_markdown(&tree), "line one\nline two");
}
