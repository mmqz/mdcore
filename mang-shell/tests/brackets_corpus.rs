//! #132 任务 1：括号匹配——真实语料验证。
//!
//! 语料 = **本仓库自己的 .rs 源文件**（真实 Rust 代码：关键字、字符串字面量、
//! 块注释、生命周期、宏……远比手写夹具刁钻）。逐文件跑两组断言：
//!
//! 1. **全字符位穷举不 panic**：对每个 (行, 列) 调用 `find_match`，
//!    判定必须是三态之一；且命中的坐标必须落在文本范围内。
//! 2. **结构一致性**：`Matched { open, close }` 必然一个开一个闭、种类
//!    相同、且开在闭之前（跨行也成立）。
//!
//! 另外给出**统计量**（匹配率 / 孤立括号数），作为「真实语料下算法在干活」
//! 的正面证据——不是全 N 的退化解。
//!
//! 语料根：`MDCORE_RS_CORPUS`（缺省 = 本 crate 的 `src/` 目录，随
//! 二进制产物定位）。文件缺失即 **FAIL**（不是 SKIP——仓库自带 .rs 是
//! 恒在场的语料）。

use std::path::{Path, PathBuf};

use mang_shell::editor::brackets::{self, BracketMatch};
use mang_shell::editor::highlight;

/// 语料根目录：优先环境变量，否则本 crate 的 src/。
fn corpus_root() -> PathBuf {
    if let Ok(p) = std::env::var("MDCORE_RS_CORPUS") {
        return PathBuf::from(p);
    }
    // CARGO_MANIFEST_DIR 在测试运行时由 cargo 注入。
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("src")
}

/// 递归收集 .rs 文件（深度有限，避开 third_party）。
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_rs(&p, out, depth + 1);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// 字符种类（断言用）。
fn kind_of(c: char) -> Option<u8> {
    Some(match c {
        '(' | ')' => 0,
        '[' | ']' => 1,
        '{' | '}' => 2,
        _ => return None,
    })
}

fn is_open(c: char) -> bool {
    matches!(c, '(' | '[' | '{')
}

/// 单文件全字符位穷举 + 结构断言，返回 (命中数, 匹配数, 孤立数)。
fn sweep(name: &str, text: &str) -> (usize, usize, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut hit = 0usize;
    let mut matched = 0usize;
    let mut unmatched = 0usize;
    // 真实 .rs 文件没有 md 围栏——按文件级语言显式传入 Rust 语义
    // （否则字符串/注释排除不生效，这就是渲染层要走的入口）。
    let lang = Some(brackets::infer_code_lang(text).unwrap_or(highlight::CodeLang::Rust));
    for (li, line) in lines.iter().enumerate() {
        let n = line.chars().count();
        for ci in 0..=n {
            let m = brackets::find_match_lang(text, (li, ci), lang);
            match m {
                BracketMatch::None => {}
                BracketMatch::Unmatched { pos } => {
                    hit += 1;
                    unmatched += 1;
                    assert!(pos.0 < lines.len(), "{name}: 孤立括号行号越界 {pos:?}");
                    let line_len = lines[pos.0].chars().count();
                    assert!(
                        pos.1 < line_len,
                        "{name}: 孤立括号列号越界 {pos:?} (行宽 {line_len})"
                    );
                }
                BracketMatch::Matched { open, close } => {
                    hit += 1;
                    matched += 1;
                    let oc = lines[open.0].chars().nth(open.1).unwrap_or('?');
                    let cc = lines[close.0].chars().nth(close.1).unwrap_or('?');
                    assert!(is_open(oc), "{name}: open 端不是开括号 {oc:?} @ {open:?}");
                    assert!(
                        !is_open(cc) && kind_of(cc).is_some(),
                        "{name}: close 端不是闭括号 {cc:?} @ {close:?}"
                    );
                    assert_eq!(
                        kind_of(oc),
                        kind_of(cc),
                        "{name}: 两端种类不一致 {oc:?} @ {open:?} vs {cc:?} @ {close:?}"
                    );
                    assert!(open <= close, "{name}: 开端在闭端之后 {open:?} > {close:?}");
                }
            }
        }
    }
    (hit, matched, unmatched)
}

#[test]
fn real_rust_corpus_full_sweep() {
    let root = corpus_root();
    let mut files = Vec::new();
    collect_rs(&root, &mut files, 0);
    assert!(
        !files.is_empty(),
        "语料为空——{} 下没有 .rs 文件（真实语料缺失即 FAIL）",
        root.display()
    );

    let mut total_hit = 0usize;
    let mut total_matched = 0usize;
    let mut total_unmatched = 0usize;
    let mut swept_files = 0usize;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let (hit, matched, unmatched) = sweep(&name, &text);
        total_hit += hit;
        total_matched += matched;
        total_unmatched += unmatched;
        swept_files += 1;
    }
    assert!(swept_files > 0, "没有可读的 .rs 语料");
    // 真实 Rust 源码里括号是海量存在的——匹配位点数必然上千。
    assert!(
        total_matched > 1000,
        "真实语料匹配位点过少（{total_matched}）——算法可能整体退化为 None"
    );
    eprintln!(
        "brackets-corpus: {swept_files} files, hits={total_hit}, matched={total_matched}, unmatched={total_unmatched}"
    );
}

#[test]
fn real_rust_file_string_and_comment_are_ignored() {
    // 取本模块自身（含大量字符串与注释）作为真实样本：
    // 注释里的 // 与字符串里的括号都不该被当成括号。
    // 用显式 \n 拼接而不是 raw string：raw string 的换行取决于源文件行尾
    // （本地 CRLF / CI LF 不同），会让列偏移断言跨平台漂移。
    let text = "fn main() {\n    let s = \"(\";\n    // (\"]\n    let t = 1;\n}\n";
    // 0: "fn main() {"        -> 7'(' 8')' 10'{'
    // 1: '    let s = "(";'    -> 13'(' 在字符串内
    // 2: '    // (]'            -> 7'(' 8']' 在注释内
    // 4: '}'                   -> 0'}'
    let rust = Some(highlight::CodeLang::Rust);
    assert_eq!(
        brackets::find_match_lang(text, (0, 11), rust),
        BracketMatch::Matched {
            open: (0, 10),
            close: (4, 0)
        }
    );
    // 字符串内的 '(' ——光标贴其右侧（col 14）与左侧（col 13）都不算括号。
    assert_eq!(
        brackets::find_match_lang(text, (1, 14), rust),
        BracketMatch::None
    );
    assert_eq!(
        brackets::find_match_lang(text, (1, 13), rust),
        BracketMatch::None
    );
    // 注释内的 '(' / ']' 同理。
    assert_eq!(
        brackets::find_match_lang(text, (2, 8), rust),
        BracketMatch::None
    );
    assert_eq!(
        brackets::find_match_lang(text, (2, 9), rust),
        BracketMatch::None
    );
    // 对照：不给语言（md 语义）时字符串不排除——登记的设计差异。
    assert_eq!(
        brackets::find_match(text, (1, 14)),
        BracketMatch::Unmatched { pos: (1, 13) }
    );
}

#[test]
fn corpus_survives_pathological_input() {
    // 单行 10 万开括号（无配对）——必须快速给出 Unmatched 而非卡死。
    let text = "(".repeat(100_000);
    let m = brackets::find_match(&text, (0, 1));
    assert!(matches!(m, BracketMatch::Unmatched { pos } if pos == (0, 0)));

    // 深嵌套 5 万层：最外层配最内层的闭括号。
    let n = 50_000;
    let text = format!("{}{}", "(".repeat(n), ")".repeat(n));
    let m = brackets::find_match(&text, (0, 1));
    assert_eq!(
        m,
        BracketMatch::Matched {
            open: (0, 0),
            close: (0, 2 * n - 1)
        }
    );
}
