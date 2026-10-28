//! N-1/N-5 实测 harness（#29 任务池 · N 组）：
//!
//! - `export <fixture.md> <out.docx> [profile]` —— markdown → BlockTree →
//!   `export_docx_styled_core` 导出（profile = `default` | `tender`——
//!   后者 = 中文标书典型排版：仿宋四号 + 1.5 倍行距 + Word 中文边距）。
//!   stdout = JSON 摘要（耗时 / 字数 / 块统计——N-1 能力上限度量面）。
//! - `estimate <fixture.md> [profile]` —— 同一排版参数下的分页预估
//!   （N-5 验收对照面；估算器 = `mdcore::pagination`）。
//!
//! 用法（仓库根）：
//! `cargo run -p mdcore-wasm --release --example n1_n5_harness -- export <md> <docx> tender`

use mdcore::markdown::markdown_to_tree;
use mdcore::tree::BlockTree;
use mdcore_wasm::docx_export::export_docx_styled_core;

/// 中文标书典型排版（仿宋四号 14pt + 1.5 倍行距 + Word 中文默认边距——
/// 上下 2.54cm=1440、左右 3.17cm=1800 twips）。
fn tender_options() -> String {
    serde_json::json!({
        "typography": {
            "ascii_font": "Times New Roman",
            "east_asia_font": "FangSong",
            "complex_script_font": "Times New Roman",
            "size_half_points": 28,
            "line_spacing": { "line": 360, "line_rule": "auto" }
        },
        "page_geometry": {
            "width": 11906,
            "height": 16838,
            "margin_top": 1440,
            "margin_right": 1800,
            "margin_bottom": 1440,
            "margin_left": 1800,
            "margin_header": 851,
            "margin_footer": 992,
            "gutter": 0
        }
    })
    .to_string()
}

fn options_for(profile: &str) -> String {
    // `{` 开头 = 原始 options JSON 直传（N-2 TOC 验收等需要非预设组合）。
    if profile.starts_with('{') {
        return profile.to_string();
    }
    match profile {
        "default" => "{}".to_string(),
        "tender" => tender_options(),
        other => panic!("unknown profile: {other} (default | tender | {{raw json}})"),
    }
}

fn tree_stats(tree: &BlockTree) -> serde_json::Value {
    let wc = mdcore::word_count::count(tree);
    serde_json::json!({
        "total_words": wc.total_words,
        "cjk_chars": wc.cjk_chars,
        "western_words": wc.western_words,
        "total_chars": wc.total_chars,
        "paragraphs": wc.paragraphs,
        "headings": wc.headings,
        "tables": wc.tables,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: {} export <fixture.md> <out.docx> [default|tender]\n       {} estimate <fixture.md> [default|tender]",
            args[0], args[0]
        );
        std::process::exit(2);
    }
    let mode = args[1].as_str();
    let fixture = &args[2];
    let profile = args
        .get(if mode == "export" { 4 } else { 3 })
        .cloned()
        .unwrap_or_else(|| "default".to_string());

    let md = std::fs::read_to_string(fixture).unwrap_or_else(|e| panic!("read {fixture}: {e}"));
    let t0 = std::time::Instant::now();
    let tree = markdown_to_tree(&md);
    let parse_ms = t0.elapsed().as_millis();
    let stats = tree_stats(&tree);
    let options = options_for(&profile);

    match mode {
        "export" => {
            let out = &args[3];
            let t1 = std::time::Instant::now();
            let bytes = export_docx_styled_core(&tree, &options)
                .unwrap_or_else(|e| panic!("export: {} ({})", e.code, e.message));
            let export_ms = t1.elapsed().as_millis();
            std::fs::write(out, &bytes).unwrap_or_else(|e| panic!("write {out}: {e}"));
            println!(
                "{}",
                serde_json::json!({
                    "mode": "export",
                    "profile": profile,
                    "fixture": fixture,
                    "docx": out,
                    "docx_bytes": bytes.len(),
                    "parse_ms": parse_ms,
                    "export_ms": export_ms,
                    "stats": stats,
                })
            );
        }
        "estimate" => {
            let est = mdcore_wasm::docx_export::estimate_pages_core(&tree, &options)
                .unwrap_or_else(|e| panic!("estimate: {} ({})", e.code, e.message));
            println!(
                "{}",
                serde_json::json!({
                    "mode": "estimate",
                    "profile": profile,
                    "fixture": fixture,
                    "parse_ms": parse_ms,
                    "estimate": serde_json::from_str::<serde_json::Value>(&est)
                        .expect("estimate json"),
                    "stats": stats,
                })
            );
        }
        other => panic!("unknown mode: {other}"),
    }
}
