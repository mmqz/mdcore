//! Markdown <-> Block Tree（解析边界；spike markdown.ts 的树化升级）。
//!
//! v0 规则（刻意保持最小，偏差已登记 README）：
//! - 行级解析：fence 聚合多行；`|` 开头连续行合并为 table；其余一行一块。
//! - 嵌套：list_item / blockquote 参与深度栈建树；quote 同深度连续行
//!   复用同一容器（children = 段落）；paragraph/heading/fence/thematic 落根层。
//! - 序列化：list 祖先贡献 2 空格缩进，quote 祖先贡献 "> " 前缀；
//!   marks 不写入 MD（spike 同行为）；ID 不写入 MD（§8.3）。
//! - round-trip 保证方向：parse → serialize → parse 结构等价（canonical 输入）。

use crate::tree::{Attrs, BlockKind, BlockTree, NodeId};

// ---------- 渐进打开的 chunk 完整性（R10-a） ----------

/// chunk 是否收在块边界上（无悬挂状态）。与 `markdown_to_tree` 的 fence
/// 判定同规：任何以 ``` 开头的行翻转开合态（v0 语法不查闭合行 info string）。
/// 悬挂 = 扫描结束时 fence 仍开启（闭合收在下一个 chunk）→ 调用方必须
/// 重切边界；核内 `extend_from_markdown` 据此返回 Reject::IncompleteChunk。
/// 其余块型（段落/列表/quote/table）单行自含，无悬挂态可检测。
pub fn is_complete_chunk(md: &str) -> bool {
    let mut open = false;
    for line in md.split('\n') {
        if line.starts_with("```") {
            open = !open;
        }
    }
    !open
}

// ---------- 序列化 ----------

pub fn tree_to_markdown(tree: &BlockTree) -> String {
    let mut lines: Vec<String> = Vec::new();
    for &child in tree.top_level() {
        render(tree, child, "", "", &mut lines);
    }
    lines.join("\n")
}

fn render(tree: &BlockTree, id: NodeId, list_indent: &str, quote_prefix: &str, out: &mut Vec<String>) {
    let node = tree.node(id);
    let emit = |content: &str, out: &mut Vec<String>| {
        for line in content.split('\n') {
            out.push(format!("{quote_prefix}{list_indent}{line}"));
        }
    };
    match node.kind {
        BlockKind::Document => {}
        BlockKind::Paragraph | BlockKind::Table => emit(&node.text.to_string(), out),
        BlockKind::Heading => {
            let level: usize = node
                .attrs
                .get("level")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            let hashes = "#".repeat(level.clamp(1, 6));
            emit(&format!("{hashes} {}", node.text), out);
        }
        BlockKind::CodeBlock => {
            let lang = node.attrs.get("lang").cloned().unwrap_or_default();
            emit(&format!("```{lang}"), out);
            emit(&node.text.to_string(), out);
            emit("```", out);
        }
        BlockKind::ThematicBreak => emit("---", out),
        BlockKind::ListItem => {
            let marker = node.attrs.get("marker").cloned().unwrap_or_else(|| "-".into());
            let head = format!("{quote_prefix}{list_indent}{marker} {}", node.text);
            for (i, line) in head.split('\n').enumerate() {
                if i == 0 {
                    out.push(line.to_string());
                } else {
                    out.push(format!("{quote_prefix}{list_indent}  {line}"));
                }
            }
            for &c in &node.children {
                render(tree, c, &format!("{list_indent}  "), quote_prefix, out);
            }
        }
        BlockKind::Blockquote => {
            if node.children.is_empty() {
                emit(&node.text.to_string(), out);
            }
            for &c in &node.children {
                render(tree, c, list_indent, &format!("{quote_prefix}> "), out);
            }
        }
    }
}

// ---------- 解析 ----------

pub fn markdown_to_tree(md: &str) -> BlockTree {
    let mut tree = BlockTree::new();
    let lines: Vec<&str> = md.split('\n').collect();
    // 深度栈：(kind, depth, node)
    let mut stack: Vec<(BlockKind, usize, NodeId)> = Vec::new();
    let root = tree.root();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("```") {
            let lang = line[3..].trim().to_string();
            let mut j = i + 1;
            while j < lines.len() && !lines[j].starts_with("```") {
                j += 1;
            }
            let inner = lines[i + 1..j.min(lines.len())].join("\n");
            let id = tree.create_node(
                BlockKind::CodeBlock,
                &inner,
                attr("lang", &lang),
                Vec::new(),
                None,
            );
            attach(&mut tree, root, id);
            i = j + 1;
            continue;
        }
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some(h) = heading_of(line) {
            let id = tree.create_node(
                BlockKind::Heading,
                &h.1,
                attr("level", &h.0.to_string()),
                Vec::new(),
                None,
            );
            attach(&mut tree, root, id);
            i += 1;
            continue;
        }
        if is_thematic(line) {
            let id = tree.create_node(BlockKind::ThematicBreak, "", Attrs::new(), Vec::new(), None);
            attach(&mut tree, root, id);
            i += 1;
            continue;
        }
        if line.starts_with('|') {
            // 连续表格行合并为一个 table 块
            let mut j = i;
            let mut rows = Vec::new();
            while j < lines.len() && lines[j].starts_with('|') {
                rows.push(lines[j]);
                j += 1;
            }
            let id = tree.create_node(
                BlockKind::Table,
                &rows.join("\n"),
                Attrs::new(),
                Vec::new(),
                None,
            );
            attach(&mut tree, root, id);
            i = j;
            continue;
        }
        if let Some(bq) = blockquote_of(line) {
            let depth = bq.0;
            // 同深度同一 quote 容器复用
            if let Some((k, d, id)) = stack.last().copied() {
                if k == BlockKind::Blockquote && d == depth {
                    let p = tree.create_node(BlockKind::Paragraph, &bq.1, Attrs::new(), Vec::new(), None);
                    attach(&mut tree, id, p);
                    i += 1;
                    continue;
                }
            }
            pop_ge(&mut stack, depth);
            let q = tree.create_node(BlockKind::Blockquote, "", Attrs::new(), Vec::new(), None);
            attach_under_stack(&mut tree, root, &stack, q);
            let p = tree.create_node(BlockKind::Paragraph, &bq.1, Attrs::new(), Vec::new(), None);
            attach(&mut tree, q, p);
            stack.push((BlockKind::Blockquote, depth, q));
            i += 1;
            continue;
        }
        if let Some(li) = list_item_of(line) {
            let depth = li.0;
            pop_ge(&mut stack, depth);
            let id = tree.create_node(
                BlockKind::ListItem,
                &li.2,
                {
                    let mut a = Attrs::new();
                    a.insert("marker".into(), li.1);
                    a.insert("depth".into(), depth.to_string());
                    a
                },
                Vec::new(),
                None,
            );
            attach_under_stack(&mut tree, root, &stack, id);
            stack.push((BlockKind::ListItem, depth, id));
            i += 1;
            continue;
        }
        // paragraph（一行一块，v0 规则）
        let id = tree.create_node(BlockKind::Paragraph, line, Attrs::new(), Vec::new(), None);
        attach(&mut tree, root, id);
        i += 1;
    }
    tree
}

fn attach(tree: &mut BlockTree, parent: NodeId, node: NodeId) {
    let len = tree.node(parent).children.len();
    tree.insert_child(parent, len, node).expect("parse attach");
}

fn attach_under_stack(
    tree: &mut BlockTree,
    root: NodeId,
    stack: &[(BlockKind, usize, NodeId)],
    node: NodeId,
) {
    match stack.last() {
        Some((_, _, id)) => attach(tree, *id, node),
        None => attach(tree, root, node),
    }
}

fn pop_ge(stack: &mut Vec<(BlockKind, usize, NodeId)>, depth: usize) {
    while stack.last().map(|(_, d, _)| *d >= depth).unwrap_or(false) {
        stack.pop();
    }
}

fn heading_of(line: &str) -> Option<(usize, String)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) {
        if let Some(text) = line[hashes..].strip_prefix(' ') {
            return Some((hashes, text.to_string()));
        }
    }
    None
}

fn is_thematic(line: &str) -> bool {
    matches!(line.trim(), "---" | "***" | "___")
}

fn blockquote_of(line: &str) -> Option<(usize, String)> {
    let mut depth = 0usize;
    let mut rest = line;
    loop {
        if let Some(r) = rest.strip_prefix("> ") {
            depth += 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('>') {
            depth += 1;
            rest = r.strip_prefix(' ').unwrap_or(r);
        } else {
            break;
        }
    }
    if depth > 0 {
        Some((depth, rest.to_string()))
    } else {
        None
    }
}

fn list_item_of(line: &str) -> Option<(usize, String, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let (marker, after) = if let Some(r) = rest.strip_prefix("- ") {
        ("-".to_string(), r)
    } else if let Some(r) = rest.strip_prefix("* ") {
        ("*".to_string(), r)
    } else if let Some(r) = rest.strip_prefix("+ ") {
        ("+".to_string(), r)
    } else {
        // 有序列表 1. / 1)
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let after_digits = &rest[digits.len()..];
            if let Some(r) = after_digits.strip_prefix(". ") {
                (format!("{digits}."), r)
            } else if let Some(r) = after_digits.strip_prefix(") ") {
                (format!("{digits})"), r)
            } else {
                return None;
            }
        } else {
            return None;
        }
    };
    Some((indent / 2 + 1, marker, after.to_string()))
}

fn attr(k: &str, v: &str) -> Attrs {
    let mut a = Attrs::new();
    if !v.is_empty() {
        a.insert(k.into(), v.into());
    }
    a
}
