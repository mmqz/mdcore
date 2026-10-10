//! R16 — blocks_json 分片/计数出口（blocks_count + blocks_json_range）语义测试。
//!
//! 背景：§15.1 非门 10MB 终态 JS 堆 FLAG——每次渐进 extend 后全量
//! dump+parse 20 万块是首要嫌疑。核侧新增按先序区间的分片查询后，
//! JS 侧 BlockMap 只解析新增区间。本文件断言分片语义三件事：
//! ① blocks_count ≡ blocks_json 列表长度（含嵌套块型）；
//! ② 任意区间切片拼接 ≡ 全量（逐字段含 id/depth/attrs/text）；
//! ③ 渐进 extend 场景：one-shot vs 分段 extend 的「首段全量 + 每段增量
//!    区间」拼接结果与 one-shot 全量 JSON 解析相等（ID 序列一致性由
//!    document_progressive 保证，这里连同 id 一起比）。
//! ④ 越界截断：from 越界 → "[]"；超尾收到文档尾。

use mdcore::debug;
use mdcore::Core;

/// 混合块型文档（heading/quote 嵌套/list 嵌套/fence/table/thematic）。
fn build_doc() -> String {
    [
        "# Head one",
        "",
        "intro paragraph with filler",
        "",
        "> quoted line one",
        "> quoted line two",
        "",
        "- item alpha",
        "  - sub item one",
        "  - sub item two",
        "- item beta",
        "",
        "```rust",
        "fn main() {",
        "    println!(\"fence body\");",
        "}",
        "```",
        "",
        "| col a | col b |",
        "| v1 | v2 |",
        "",
        "---",
        "",
        "tail paragraph",
        "",
    ]
    .join("\n")
}

fn parse(json: &str) -> serde_json::Value {
    serde_json::from_str(json).expect("blocks json 必须合法")
}

#[test]
fn blocks_count_matches_blocks_json_length() {
    let core = Core::from_markdown(&build_doc());
    let n = debug::blocks_count(&core.tree);
    let full = parse(&debug::blocks_json(&core.tree));
    assert!(n > 10, "混合文档块数应可观，实际 {n}");
    assert_eq!(n, full.as_array().unwrap().len(), "计数 ≡ 全量列表长度");

    // 嵌套被计入：quote 容器 + 子段落、嵌套 list_item 都在先序列列
    let kinds: Vec<&str> = full
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["type"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"blockquote"));
    assert!(kinds.contains(&"code_block"));
    assert_eq!(kinds.iter().filter(|k| **k == "list_item").count(), 4);
}

#[test]
fn range_slices_concatenate_to_full() {
    let core = Core::from_markdown(&build_doc());
    let n = debug::blocks_count(&core.tree);
    let full = parse(&debug::blocks_json(&core.tree));

    // 变长切片拼接：[0,3) + [3,3) + [6,n) ≡ 全量
    let mut merged = Vec::new();
    for (from, count) in [(0usize, 3usize), (3, 3), (6, n)] {
        let slice = parse(&debug::blocks_json_range(&core.tree, from, count));
        for b in slice.as_array().unwrap() {
            merged.push(b.clone());
        }
    }
    assert_eq!(serde_json::Value::Array(merged), full, "切片拼接 ≡ 全量");

    // 单块切片与全量对应元素逐字段一致（id/type/depth/attrs/text）
    for i in 0..n {
        let one = parse(&debug::blocks_json_range(&core.tree, i, 1));
        assert_eq!(one.as_array().unwrap().len(), 1);
        assert_eq!(one.as_array().unwrap()[0], full.as_array().unwrap()[i]);
    }
}

#[test]
fn range_out_of_bounds_truncates() {
    let core = Core::from_markdown(&build_doc());
    let n = debug::blocks_count(&core.tree);

    // from 越界 → 空列表
    assert_eq!(debug::blocks_json_range(&core.tree, n, 4), "[]");
    assert_eq!(debug::blocks_json_range(&core.tree, n + 100, 1), "[]");
    // count = 0 → 空；超尾收到文档尾
    assert_eq!(debug::blocks_json_range(&core.tree, 0, 0), "[]");
    let tail = parse(&debug::blocks_json_range(&core.tree, n - 2, 100));
    assert_eq!(tail.as_array().unwrap().len(), 2, "超尾截断到文档尾");
    // 空文档
    let empty = Core::new();
    assert_eq!(debug::blocks_count(&empty.tree), 0);
    assert_eq!(debug::blocks_json_range(&empty.tree, 0, 10), "[]");
}

#[test]
fn progressive_extend_range_feeds_match_oneshot() {
    // 模拟 JS 侧 R16 消费方式：构造期取全量，随后每次 extend 只取
    // [prev, prev+added) 区间；拼接结果与 one-shot 全量解析相等。
    let sections: Vec<String> = (0..120)
        .map(|i| match i % 4 {
            0 => format!("# Section {i}\n"),
            1 => format!("paragraph body {i} filler text\n"),
            2 => format!("- item a {i}\n  - sub {i}\n- item b {i}\n"),
            _ => format!("```js\nlet x = {i};\n```\n"),
        })
        .collect();

    let one_shot = Core::from_markdown(&sections.concat());

    let mut chunked = Core::from_markdown(&sections[0..7].concat());
    let mut slices = vec![debug::blocks_json(&chunked.tree)]; // 首段全量（构造器路径）
    for chunk in sections[7..].chunks(5) {
        let prev = debug::blocks_count(&chunked.tree);
        chunked.extend_from_markdown(&chunk.concat()).unwrap();
        let added = debug::blocks_count(&chunked.tree) - prev;
        slices.push(debug::blocks_json_range(&chunked.tree, prev, added));
    }

    let mut merged = Vec::new();
    for s in &slices {
        for b in parse(s).as_array().unwrap() {
            merged.push(b.clone());
        }
    }
    let full = parse(&debug::blocks_json(&one_shot.tree));
    assert_eq!(
        serde_json::Value::Array(merged),
        full,
        "增量区间拼接（含 id/depth/attrs/text）≡ one-shot 全量"
    );
    assert!(full.as_array().unwrap().len() > 100, "块数应足够大");
}
