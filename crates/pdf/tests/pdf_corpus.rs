//! PDF-f2 语料门——真实 PDF 语料全件行为锁（#264 交付）。
//!
//! 语料 = `test-artifacts/pdf-corpus/`（78 件真实 PDF，10 类：加密/
//! 表单/注释/书签/扫描/多栏/图注/表格/混合/损坏；来源 pdf.js +
//! PDFBox 测试仓，Apache-2.0，逐件 provenance 见语料目录
//! PROVENANCE.json 与 README——**二进制不入库**，取件 =
//! `python3 test-artifacts/pdf-corpus/fetch_corpus.py`，与
//! pdf-organize-corpus 先例同构）。
//!
//! 门设计（#262 教训内建——语料门必须防空转）：
//! 1. **三态防御**：目录缺席（CI/新克隆）= 合法 skip（响亮横幅）；
//!    目录在场但收集 0 件 = **立红**（结构又变了）；件数 < 下限 =
//!    立红（防缩水）。
//! 2. **每类数量下限**（分类目录空 = 该类断言红）。
//! 3. **全件 catch_unwind**：真实件解析永不 panic（panic = 测试失败）。
//! 4. 行为分布锁（实测 2026-11-06，78 件）：
//!    · 零 panic；
//!    · encrypted/ 8 件 SecurityDeny（S5 加密档拒绝实证；余 3 件
//!      为空用户密码可开形态——真实灰区，允许 OK）；
//!    · broken/ 允许 Engine 拒（零页/spec 违例是合理拒绝）但
//!      OK ≥ 8（结构异常 ≠ 全废）；
//!    · 其余 8 类全件 OK（真实世界件必须能开）；
//!    · freeculture.pdf = 352 页长文档在语料（大文档覆盖锚点）。

#![cfg(feature = "engine-lopdf")]

use mdc_pdf::organize::PdfOrganizer;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn corpus_root() -> PathBuf {
    std::env::var("MDC_PDF_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // 2 级上溯 = 仓根（crates/pdf → 仓库）——external_corpus
            // 先例同构。
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/pdf-corpus")
        })
}

/// 递归收集 (分类, 路径)（#262 模式：不依赖目录层级 + 分类 =
/// 父目录名）。
fn corpus_files() -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map(|x| x == "pdf").unwrap_or(false) {
                let cat = p
                    .parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                out.push((cat, p));
            }
        }
    }
    let root = corpus_root();
    if !root.is_dir() {
        return Vec::new();
    }
    let mut v = Vec::new();
    walk(&root, &mut v);
    v.sort();
    v
}

/// PROVENANCE.json 里登记的语料件是否**任一在盘**（「已取件」信号，
/// 与递归收集相互独立的两条代码路径——walk 退化时此处仍能发现件，
/// #262 原事故形态照红）。清单是脚本生成的稳定格式（json indent=1，
/// `"category"` 行在前 `"name"` 行在后），行级扫描与
/// provenance_manifest_consistency 同风格（测试面不引 serde）。
fn provenance_any_present(root: &Path) -> bool {
    fn quoted_value(line: &str, key: &str) -> Option<String> {
        let i = line.find(key)? + key.len();
        let rest = &line[i..];
        let a = rest.find('"')? + 1;
        let b = rest[a..].find('"')? + a;
        let v = &rest[a..b];
        (!v.is_empty()).then(|| v.to_string())
    }
    let Ok(text) = std::fs::read_to_string(root.join("PROVENANCE.json")) else {
        return false;
    };
    let mut cat = String::new();
    for line in text.lines() {
        if let Some(c) = quoted_value(line, "\"category\"") {
            cat = c;
        } else if let Some(name) = quoted_value(line, "\"name\"") {
            if name.ends_with(".pdf") && root.join(&cat).join(&name).is_file() {
                return true;
            }
        }
    }
    false
}

/// 打开判定（三态 + panic 探测）。
enum Verdict {
    Ok(usize),
    Denied,
    EngineErr,
    Panicked,
}

fn open_verdict(bytes: Vec<u8>) -> Verdict {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        // from_bytes 消耗 Vec——move 进闭包后再用于 page_count。
        PdfOrganizer::from_bytes(bytes)
    }));
    match r {
        Err(_) => Verdict::Panicked,
        Ok(Ok(o)) => Verdict::Ok(o.page_count()),
        Ok(Err(e)) => match e {
            mdc_pdf::CommandError::SecurityDeny { .. } => Verdict::Denied,
            _ => Verdict::EngineErr,
        },
    }
}

#[test]
fn real_pdf_corpus_behavior_lock() {
    let files = corpus_files();
    if files.is_empty() {
        // 三态防御（#262，Lead 评审 2026-11-06 修正）：目录在任何
        // clone 里都在场（README/取件脚本已提交）——「已取件」看
        // **清单件在场性**：PROVENANCE.json 任一登记件在盘但收集
        // 0 件 = 收集逻辑坏 → 红；清单件全缺 = 未取件 → 响亮 skip。
        assert!(
            !provenance_any_present(&corpus_root()),
            "corpus 已取件（PROVENANCE 登记件在盘）但递归收集到 0 件 .pdf——收集逻辑坏了？({})",
            corpus_root().display()
        );
        eprintln!(
            "[pdf-corpus] corpus 未取件（清单件全缺，{}）— skipping real-corpus tests；取件：python3 test-artifacts/pdf-corpus/fetch_corpus.py",
            corpus_root().display()
        );
        return;
    }
    println!(
        "[pdf-corpus] 检查了 {} 件真实语料（{}）",
        files.len(),
        corpus_root().display()
    );
    assert!(files.len() >= 70, "corpus shrank: {}", files.len());

    // 每类数量下限（分类目录空/被清 = 该类红）。
    let mut per_cat: BTreeMap<String, usize> = BTreeMap::new();
    for (cat, _) in &files {
        *per_cat.entry(cat.clone()).or_insert(0) += 1;
    }
    let floors = [
        ("annotations", 12usize),
        ("bookmarks", 4),
        ("broken", 10),
        ("captions", 2),
        ("encrypted", 10),
        ("forms", 10),
        ("mixed", 15),
        ("multicolumn", 3),
        ("scanned", 3),
        ("tables", 2),
    ];
    for (cat, floor) in floors {
        let got = per_cat.get(cat).copied().unwrap_or(0);
        assert!(
            got >= floor,
            "category {cat}: {got} < floor {floor}（分类目录空或语料缩水）"
        );
    }

    // 全件行为锁。
    let (mut panics, mut denied, mut engine_err, mut ok) = (0, 0, 0, 0);
    let mut ok_by_cat: BTreeMap<&str, usize> = BTreeMap::new();
    let mut max_pages = 0usize;
    let mut long_doc_seen = false;
    for (cat, p) in &files {
        let bytes = std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        match open_verdict(bytes) {
            Verdict::Panicked => {
                panics += 1;
                eprintln!("[pdf-corpus] PANIC on {}/{}", cat, p.display());
            }
            Verdict::Denied => denied += 1,
            Verdict::EngineErr => engine_err += 1,
            Verdict::Ok(pages) => {
                ok += 1;
                *ok_by_cat.entry(cat.as_str()).or_insert(0) += 1;
                max_pages = max_pages.max(pages);
                if pages >= 100 {
                    long_doc_seen = true;
                }
            }
        }
    }
    println!(
        "[pdf-corpus] ok={ok} denied={denied} engine_err={engine_err} panics={panics} max_pages={max_pages}"
    );

    // 断言面（实测基线：ok 68 / denied 8 / engine_err 2 / panics 0）。
    assert_eq!(panics, 0, "真实件解析不得 panic");
    assert!(denied >= 8, "S5 加密拒绝实证：{denied}");
    assert!(engine_err >= 1, "坏件合理拒绝在档：{engine_err}");
    assert!(ok >= 60, "可开真实件下限：{ok}");
    // 8 类全件可开（真实世界件必须能开）。
    for cat in [
        "annotations",
        "captions",
        "forms",
        "mixed",
        "multicolumn",
        "scanned",
        "tables",
    ] {
        let got = ok_by_cat.get(cat).copied().unwrap_or(0);
        let total = per_cat.get(cat).copied().unwrap_or(0);
        assert_eq!(
            got, total,
            "category {cat}: {got}/{total} 件可开（真实件应全开）"
        );
    }
    // bookmarks 4 件中 1 件零页 Engine 拒（spec 7.7.4）——OK ≥ 3。
    assert!(
        ok_by_cat.get("bookmarks").copied().unwrap_or(0) >= 3,
        "bookmarks OK 下限"
    );
    // broken 允许 Engine 拒，但结构异常 ≠ 全废：OK ≥ 8（实测 10/12）。
    assert!(
        ok_by_cat.get("broken").copied().unwrap_or(0) >= 8,
        "broken OK 下限（结构异常件多数仍应可开）"
    );
    // 长文档覆盖锚点（freeculture.pdf 352 页）。
    assert!(long_doc_seen, "语料须含 ≥100 页长文档（大文档覆盖）");
}

#[test]
fn provenance_manifest_consistency() {
    // PROVENANCE.json 与实际文件一致性（防空转的另一面：清单漂移
    // = 语料被手工改动未登记）。
    let prov_path = corpus_root().join("PROVENANCE.json");
    if !corpus_root().is_dir() {
        eprintln!("[pdf-corpus] corpus dir absent — skipping provenance gate");
        return;
    }
    let text = std::fs::read_to_string(&prov_path)
        .unwrap_or_else(|e| panic!("PROVENANCE.json missing ({e})——取件后必须重建"));
    let files = corpus_files();
    let mut listed = 0usize;
    for line in text.lines() {
        if line.contains("\"name\":") && line.contains(".pdf") {
            listed += 1;
        }
    }
    assert!(
        listed >= 70,
        "PROVENANCE 登记 {listed} 件 < 实际 {} 件——取件后未重建清单？",
        files.len()
    );
}
