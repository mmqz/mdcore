//! Markdown <-> Block Tree（解析边界；spike markdown.ts 的树化升级）。
//!
//! v0 规则（刻意保持最小，偏差已登记 README）：
//! - 行级解析：fence 聚合多行；`|` 开头连续行合并为 table；其余一行一块。
//! - 数学块（#261）：`$$` 定界——单独 `$$` 行 fence 同构聚合（开行起、收行止）；
//!   `$$x$$` 同行自含形态（行首尾皆 `$$` 且内容非空）；其余含 `$` 行不猜
//!   （段落，行内 `$...$` 由 [`crate::inline`] 消费时派生，不进块级）。
//! - 嵌套：list_item / blockquote 参与深度栈建树；quote 同深度连续行
//!   复用同一容器（children = 段落）；paragraph/heading/fence/math/thematic 落根层。
//! - 序列化：list 祖先贡献 2 空格缩进，quote 祖先贡献 "> " 前缀；
//!   marks 不写入 MD（spike 同行为）；ID 不写入 MD（§8.3）。
//! - round-trip 保证方向：parse → serialize → parse 结构等价（canonical 输入；
//!   数学块恒规范化为 fence 形态，单行形态序列化后不再保留——结构等价口径）。

use crate::tree::{Attrs, BlockKind, BlockTree, NodeId};

// ---------- 渐进打开的 chunk 完整性（R10-a） ----------

/// chunk 是否收在块边界上（无悬挂状态）。与 `markdown_to_tree` 的 fence/math
/// 判定同规（状态机互斥：fence 内的 `$$` 行与 math 内的 ``` 行不翻对面状态——
/// 两类围栏内文都是逐字面，不猜）；`$$x$$` 同行自含形态无悬挂态。
/// 悬挂 = 扫描结束时任一围栏仍开启（闭合收在下一个 chunk）→ 调用方必须
/// 重切边界；核内 `extend_from_markdown` 据此返回 Reject::IncompleteChunk。
/// 其余块型（段落/列表/quote/table）单行自含，无悬挂态可检测。
pub fn is_complete_chunk(md: &str) -> bool {
    let mut fence_open = false;
    let mut math_open = false;
    for line in md.split('\n') {
        if fence_open {
            // fence 判定不 trim——与解析器 strip_prefix 同规（历史行为，
            // 不在本单改动面）。
            if line.starts_with("```") {
                fence_open = false;
            }
        } else if math_open {
            if line.trim() == "$$" {
                math_open = false;
            }
        } else if line.starts_with("```") {
            fence_open = true;
        } else if line.trim() == "$$" {
            math_open = true;
        }
    }
    !fence_open && !math_open
}

// ---------- 序列化 ----------

pub fn tree_to_markdown(tree: &BlockTree) -> String {
    let mut lines: Vec<String> = Vec::new();
    for &child in tree.top_level() {
        render(tree, child, "", "", &mut lines);
    }
    lines.join("\n")
}

fn render(
    tree: &BlockTree,
    id: NodeId,
    list_indent: &str,
    quote_prefix: &str,
    out: &mut Vec<String>,
) {
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
        // 数学块恒规范化为 fence 形态（单行自含形态序列化后不再保留——
        // round-trip 结构等价口径，见模块文档）。
        BlockKind::Math => {
            emit("$$", out);
            emit(&node.text.to_string(), out);
            emit("$$", out);
        }
        BlockKind::ThematicBreak => emit("---", out),
        BlockKind::ListItem => {
            let marker = node
                .attrs
                .get("marker")
                .cloned()
                .unwrap_or_else(|| "-".into());
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

/// 嵌套深度门（#278 审计 B-4）：与 [`crate::html`] 侧同常数同语义
/// （html.rs MAX_DEPTH=256，超深跳过子树）。此前 markdown 建树无
/// 深度上限，同仓双标；敌意逐行递进（`>` / `>>` / `>>>` …）或缩进
/// 列表递进可造万层深树，blocks_json/to_markdown/word_count 等递归
/// 消费者栈溢出（wasm = trap / 原生 = 崩溃）。行深度（quote 的 >
/// 计数 / list 的缩进层号）即嵌套容器层编号（1 基）：拦
/// depth ≥ MAX_DEPTH = 容器层封顶 255（quote 内容段落叶子 +1）
/// ——树深硬顶 256，与 html 侧树深上限同构。
const MAX_DEPTH: usize = 256;

pub fn markdown_to_tree(md: &str) -> BlockTree {
    // TR-1（issue #9）：行尾规范化前置——CRLF / 孤立 CR 统一为 LF。
    // CommonMark 规范三种行尾等价；模型 = 权威层持有 LF 规范形，使
    // CM6 视图（输入即规范化 LF）、__md() 投影（to_markdown）、
    // docx/xlsx 导出面三面一致（保存内容 = 所见）。三条解析入口
    // （from_markdown / sync_from_markdown / extend_from_markdown）
    // 全部漏斗经本函数，单点覆盖；快路径零分配（无 \r 时 contains
    // 单扫通过，不进入 replace 分支）。
    let normalized;
    let md: &str = if md.contains('\r') {
        normalized = md.replace("\r\n", "\n").replace('\r', "\n");
        &normalized
    } else {
        md
    };
    let mut tree = BlockTree::new();
    let lines: Vec<&str> = md.split('\n').collect();
    // 深度栈：(kind, depth, node)
    let mut stack: Vec<(BlockKind, usize, NodeId)> = Vec::new();
    let root = tree.root();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("```") {
            let lang = rest.trim().to_string();
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
            );
            attach(&mut tree, root, id);
            i = j + 1;
            continue;
        }
        // 数学块（#261）：单独 `$$` 行 = fence 同构聚合；`$$x$$` 同行自含。
        // 其余形态（`$$ x` 无收界 / 行中 `$`）不猜——落 paragraph（行内
        // `$...$` 是消费时派生面，见 inline.rs，不进块级判定）。
        let trimmed = line.trim();
        if trimmed == "$$" {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim() != "$$" {
                j += 1;
            }
            let inner = lines[i + 1..j.min(lines.len())].join("\n");
            let id = tree.create_node(BlockKind::Math, &inner, Attrs::new(), Vec::new());
            attach(&mut tree, root, id);
            i = j + 1;
            continue;
        }
        if trimmed.starts_with("$$") && trimmed.ends_with("$$") && trimmed.len() > 4 {
            let inner = &trimmed[2..trimmed.len() - 2];
            let id = tree.create_node(BlockKind::Math, inner, Attrs::new(), Vec::new());
            attach(&mut tree, root, id);
            i += 1;
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
            );
            attach(&mut tree, root, id);
            i += 1;
            continue;
        }
        if is_thematic(line) {
            let id = tree.create_node(BlockKind::ThematicBreak, "", Attrs::new(), Vec::new());
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
            let id = tree.create_node(BlockKind::Table, &rows.join("\n"), Attrs::new(), Vec::new());
            attach(&mut tree, root, id);
            i = j;
            continue;
        }
        if let Some(bq) = blockquote_of(line) {
            let depth = bq.0;
            // #278 深度门：超深行跳过（不建节点、不出内容——与 html
            // 侧超深子树跳过同语义；无 Loss 通道故不登记，行为由
            // spike_semantics.rs 深度门测试钉定）。
            if depth >= MAX_DEPTH {
                i += 1;
                continue;
            }
            // 同深度同一 quote 容器复用
            if let Some((k, d, id)) = stack.last().copied() {
                if k == BlockKind::Blockquote && d == depth {
                    let p = tree.create_node(BlockKind::Paragraph, &bq.1, Attrs::new(), Vec::new());
                    attach(&mut tree, id, p);
                    i += 1;
                    continue;
                }
            }
            pop_ge(&mut stack, depth);
            let q = tree.create_node(BlockKind::Blockquote, "", Attrs::new(), Vec::new());
            attach_under_stack(&mut tree, root, &stack, q);
            let p = tree.create_node(BlockKind::Paragraph, &bq.1, Attrs::new(), Vec::new());
            attach(&mut tree, q, p);
            stack.push((BlockKind::Blockquote, depth, q));
            i += 1;
            continue;
        }
        if let Some(li) = list_item_of(line) {
            let depth = li.0;
            // #278 深度门：同 blockquote 面（缩进层号即嵌套层编号）。
            if depth >= MAX_DEPTH {
                i += 1;
                continue;
            }
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
            );
            attach_under_stack(&mut tree, root, &stack, id);
            stack.push((BlockKind::ListItem, depth, id));
            i += 1;
            continue;
        }
        // paragraph（一行一块，v0 规则）
        let id = tree.create_node(BlockKind::Paragraph, line, Attrs::new(), Vec::new());
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
            } else {
                let r = after_digits.strip_prefix(") ")?;
                (format!("{digits})"), r)
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
