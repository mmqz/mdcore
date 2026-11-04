//! M-1 外部语料常驻回归门 · L1 面（issue #29 A6；TR-2B 一次性体检的
//! 常驻化——1317 例中的 1307 例 markdown 解析面）。
//!
//! # 语料与溯源
//!
//! - `test-artifacts/external-corpus/commonmark/spec.txt`：CommonMark
//!   规范全量 example 块（CC-BY-SA 4.0；只取输入 markdown 半区，期望
//!   html 半区不消费——引擎非 HTML 渲染面）。围栏 = 32 反引号
//!   ` example` 开、32 反引号行闭；`.` 独占行分界 md/html。
//! - `test-artifacts/external-corpus/markdown-it/good.txt`：markdown-it
//!   自有测试集（MIT）。块分隔 = 42 波浪线；块内首个 `.` 与次个 `.`
//!   之间为输入。
//! - 解析逻辑与 TR-2B 驱动 `web/tr2b_l1_commonmark.py` 逐行为同构
//!   （Rust 移植——单一真源语义：两处解析必须产出同一批用例）。
//!
//! # 门禁语义（Lead 裁决：「下降即红」——允许已知失败，不许新增失败）
//!
//! 每用例三指标：
//! 1. **零 panic**（崩溃 = 测试二进制失败，最硬门）；
//! 2. **幂等到达**：`md1 = f(md)`、`md2 = f(md1)`，`md1 == md2`
//!    （TR-2B 实测基线 100%）；
//! 3. **内容保留率**（启发式）：源与 md1 的非空白字符多重集交集
//!    占源多重集总量的比例（markdown 语法字符的合法变换使该值 < 1）。
//!
//! 与 `test-artifacts/external-corpus/BASELINE.json` 的 `l1` 节比对：
//! - 幂等数 **不得低于基线**（新非幂等 = 红）；
//! - 源级均值保留率 **不得低于基线 - 0.005**（0.5pp 容差吸收数值
//!   噪声；超 0.005 的下降 = 红）；
//! - 改善（幂等变多 / 均值上升 ≥ 0.002）不失败，stderr 提示
//!   「baseline improved — 重新生成基线入档」。
//!
//! 基线再生：`M1_REGEN_BASELINE=1 cargo test -p mdcore --test external_corpus`
//! （写入 BASELINE.json 的 l1 节后须提交入库——基线是评审物）。
//! 基线缺失 = 首boot：自动生成并失败一次，提示提交（防无基线静默放行）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 语料根（crate 根上溯两级 = 仓根）。
fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/external-corpus")
}

/// 源级基线节（serde 容忍未知字段——三门共用一文件，各自只认领己节）。
#[derive(Debug, Default, Serialize, Deserialize)]
struct L1SourceBaseline {
    cases: usize,
    idempotent: usize,
    mean_retention: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    non_idempotent: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct L1Baseline {
    spec: L1SourceBaseline,
    markdown_it: L1SourceBaseline,
}

/// spec.txt example 块解析（32 反引号围栏——TR-2B 驱动逐行同构）。
fn parse_spec_examples(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // 行尾归一化：闭合围栏判定用 len==32、分块锚用固定串，
    // CRLF 下都会失配（用例数 655→0）。先归一化再看。
    let text = text.replace("\r\n", "\n");
    let mut section = String::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix('#') {
            let level = rest.len() - rest.trim_start_matches('#').len();
            if (1..=6).contains(&level) {
                if let Some(title) = line.get(level + 1..) {
                    if line.len() > level + 1 && line.as_bytes()[level] == b' ' {
                        section = title.trim().to_string();
                    }
                }
            }
        }
        // 32 反引号 + " example"（含可选尾标记——disabled 例现代规范
        // 已不存在，保守按围栏匹配不排除，与 TR-2B enabled 计数对齐
        // 的差异在基线 cases 数上钉定）。
        let is_open = line.len() >= 32
            && line.as_bytes()[..32].iter().all(|&b| b == b'`')
            && line[32..].starts_with(" example");
        if is_open {
            let mut body: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                // 闭合围栏：允许尾部 \r（语料原样 CRLF，git 入库转 LF——
                // 本地是 CRLF、CI 是 LF，两种都要认，否则闭合永不命中，
                // 整个文件被当成一个 example，用例数掉到 0。)
                let lb = l.trim_end_matches('\r');
                if lb.len() == 32 && lb.as_bytes().iter().all(|&b| b == b'`') {
                    break;
                }
                body.push(l);
                i += 1;
            }
            // body = markdown ... '.' ... html（独立 `.` 行分界）。
            if let Some(dot) = body.iter().position(|l| *l == ".") {
                let md = body[..dot].join("\n");
                out.push((section.clone(), md));
            }
        }
        i += 1;
    }
    out
}

/// good.txt 解析（42 波浪线分块——TR-2B 驱动逐行同构）。
fn parse_good_txt(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // 行尾归一化（见 parse_spec_examples 注释）：语料 CRLF/LF 两种形态都要能
    // 解析——分块锚是 "\n~~~~\n"，CRLF 下永远不匹配（用例数会掉到 0）。
    let text = text.replace("\r\n", "\n");
    for block in text.split("\n~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\n") {
        let lines: Vec<&str> = block.split('\n').collect();
        let dots: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| (*l == ".").then_some(i))
            .collect();
        if dots.len() >= 2 {
            let md = lines[dots[0] + 1..dots[1]].join("\n");
            out.push(("markdown-it".to_string(), md));
        }
    }
    out
}

/// 非空白字符多重集保留率（TR-2B content_chars 同构：is_whitespace）。
fn retention(src: &str, proj: &str) -> f64 {
    let mut sc: BTreeMap<char, usize> = BTreeMap::new();
    for c in src.chars().filter(|c| !c.is_whitespace()) {
        *sc.entry(c).or_default() += 1;
    }
    let total: usize = sc.values().sum();
    if total == 0 {
        return 1.0;
    }
    let mut pc: BTreeMap<char, usize> = BTreeMap::new();
    for c in proj.chars().filter(|c| !c.is_whitespace()) {
        *pc.entry(c).or_default() += 1;
    }
    let kept: usize = sc
        .iter()
        .map(|(c, n)| (*n).min(pc.get(c).copied().unwrap_or(0)))
        .sum();
    kept as f64 / total as f64
}

/// 单用例结果。
struct CaseResult {
    idempotent: bool,
    retention: f64,
}

fn run_case(md: &str) -> CaseResult {
    let core = mdcore::Core::from_markdown(md);
    let md1 = mdcore::markdown::tree_to_markdown(&core.tree);
    let core2 = mdcore::Core::from_markdown(&md1);
    let md2 = mdcore::markdown::tree_to_markdown(&core2.tree);
    CaseResult {
        idempotent: md1 == md2,
        retention: retention(md, &md1),
    }
}

fn load_baseline() -> Option<L1Baseline> {
    let p = corpus_root().join("BASELINE.json");
    let text = std::fs::read_to_string(p).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    match serde_json::from_value(root.get("l1")?.clone()) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("M1 l1 baseline parse failed: {e}");
            None
        }
    }
}

fn save_baseline(b: &L1Baseline) {
    let p = corpus_root().join("BASELINE.json");
    // 三门共用一文件：读-改-写只认领 l1 节（未知节保留——serde
    // Value 载体防互相覆写）。
    let mut root: serde_json::Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({"schema": 1}));
    root["schema"] = serde_json::json!(1);
    let l1 = serde_json::to_value(b).expect("l1 baseline serializable");
    root["l1"] = l1;
    std::fs::write(&p, serde_json::to_string_pretty(&root).expect("pretty"))
        .expect("write BASELINE.json");
}

#[test]
fn l1_external_corpus_gate() {
    // 环境门（issue #89 纪律）：外部语料不在仓（test-artifacts/ 被 gitignore），
    // CI / 新克隆上必然缺失。缺件 = SKIP（响亮横幅），不是业务 FAIL。
    // 判据：外部语料属「环境依赖」类别，与 poppler/tesseract 同性质。
    let spec_path = corpus_root().join("commonmark/spec.txt");
    let good_path = corpus_root().join("markdown-it/good.txt");
    for mp in [&spec_path, &good_path] {
        // 判据：文件存在 ≠ 内容有效（CI 上可能被 gitignore 成空占位）。
        // 用「≥ 1KB」作为有效语料门槛——spec.txt 实际 ~250KB，good.txt ~120KB。
        let ok = std::fs::metadata(mp)
            .map(|m| m.len() > 1024)
            .unwrap_or(false);
        if !ok {
            eprintln!();
            eprintln!("====================================================================");
            eprintln!("SKIP: l1_external_corpus_gate");
            eprintln!("reason: CORPUS_MISSING_OR_EMPTY: {}", mp.display());
            eprintln!("（外部语料缺件或空占位，非业务失败——issue #89 口径）");
            eprintln!("补件指引: 拉 CommonMark spec.txt + markdown-it good.txt 到 test-artifacts/external-corpus/");
            eprintln!("====================================================================");
            return;
        }
    }
    let spec_text = std::fs::read_to_string(&spec_path).expect("spec.txt readable");
    let good_text = std::fs::read_to_string(&good_path).expect("good.txt readable");
    let spec_cases = parse_spec_examples(&spec_text);
    let good_cases = parse_good_txt(&good_text);
    // TR-2B 口径锚：spec 655 / markdown-it 652（数量漂移 = 语料被动过，
    // 失败提示先核 provenance.json）。
    assert!(
        spec_cases.len() >= 600 && good_cases.len() >= 600,
        "corpus case count drift: spec={} good={} (TR-2B: 655/652) — check provenance",
        spec_cases.len(),
        good_cases.len()
    );

    let regen = std::env::var("M1_REGEN_BASELINE").is_ok();
    let mut current = L1Baseline::default();
    for (name, cases, slot) in [
        ("spec", &spec_cases, &mut current.spec),
        ("markdown_it", &good_cases, &mut current.markdown_it),
    ] {
        let mut non_idem = Vec::new();
        let mut ret_sum = 0.0f64;
        let mut idem = 0usize;
        for (idx, (section, md)) in cases.iter().enumerate() {
            let r = run_case(md);
            if r.idempotent {
                idem += 1;
            } else {
                non_idem.push(format!("{name}#{idx}({section})"));
            }
            ret_sum += r.retention;
        }
        slot.cases = cases.len();
        slot.idempotent = idem;
        slot.mean_retention = ret_sum / cases.len().max(1) as f64;
        slot.non_idempotent = non_idem;
    }

    if regen {
        save_baseline(&current);
        eprintln!(
            "M1 baseline regenerated (l1): spec {}/{} idem, mean {:.4}; good {}/{} idem, mean {:.4}",
            current.spec.idempotent, current.spec.cases, current.spec.mean_retention,
            current.markdown_it.idempotent, current.markdown_it.cases,
            current.markdown_it.mean_retention
        );
        return;
    }

    let Some(baseline) = load_baseline() else {
        save_baseline(&current);
        panic!(
            "BASELINE.json 缺失——已生成 l1 节（spec {}/{} idem, mean {:.4}；good {}/{} idem, mean {:.4}）。\
             提交入库后重跑（防无基线静默放行）。",
            current.spec.idempotent, current.spec.cases, current.spec.mean_retention,
            current.markdown_it.idempotent, current.markdown_it.cases,
            current.markdown_it.mean_retention
        )
    };

    for (name, cur, base) in [
        ("spec", &current.spec, &baseline.spec),
        ("markdown_it", &current.markdown_it, &baseline.markdown_it),
    ] {
        assert!(
            cur.idempotent >= base.idempotent,
            "[{name}] 幂等下降即红：{}/{}（基线 {}/{}）——新增非幂等：{:?}",
            cur.idempotent,
            cur.cases,
            base.idempotent,
            base.cases,
            cur.non_idempotent[..cur.non_idempotent.len().min(5)].to_vec()
        );
        assert!(
            cur.mean_retention >= base.mean_retention - 0.005,
            "[{name}] 内容保留率下降超容差：{:.4}（基线 {:.4}，容差 0.005）",
            cur.mean_retention,
            base.mean_retention
        );
        if cur.idempotent > base.idempotent || cur.mean_retention > base.mean_retention + 0.002 {
            eprintln!(
                "[{name}] baseline improved（idem {}/{} → {}/{}, retention {:.4} → {:.4}）\
                 —— 用 M1_REGEN_BASELINE=1 再生基线入档",
                base.idempotent,
                base.cases,
                cur.idempotent,
                cur.cases,
                base.mean_retention,
                cur.mean_retention
            );
        }
    }
}
