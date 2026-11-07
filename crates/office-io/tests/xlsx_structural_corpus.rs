//! XLSX 结构面语料门——隐藏行列/外部链接/样式继承/数字格式四类
//! 真实语料断言（#265 交付）。
//!
//! 语料 = `test-artifacts/xlsx-structural-corpus/`（28 件，四类目录：
//! Apache POI test-data/spreadsheet 真实件——bug 附件/真实文档；
//! Apache-2.0；逐件 provenance 见语料目录 PROVENANCE.md 与 README。
//! **二进制不入库**，取件 = `python3 test-artifacts/
//! xlsx-structural-corpus/fetch_corpus.py`）。
//!
//! 门设计（#262 教训内建）：三态防御（缺席 skip / 在场空红 / 件数
//! 下限红）+ 每类数量下限 + 全件 catch_unwind + 形态实值断言
//!（隐藏行号/外部链接目标/xf 继承链/格式串——实测钉定见各断言）。
//!
//! 实测基线（2026-11-06，28 件）：
//! ```text
//! hidden_rows/    8 件：4-47 隐藏行/列不等（55640=[1,2,3,4]，
//!                  FillWithoutColor 47 行 + AA..宽列段）
//! external_links/ 8 件：全部 ≥1 外部链接（file:///UNC/相对路径）
//! num_formats/    6 件：custom_fmt 1-5（builtin 日期/百分比在档）
//! style_inherit/  6 件：cellXfs/cellStyleXfs 1-3088（53568 巨型）
//! ```

use mdc_office_io::xlsx_structural::read_structural_from_bytes;
use std::path::{Path, PathBuf};

fn corpus_root() -> PathBuf {
    std::env::var("MDC_XLSX_STRUCT_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test-artifacts/xlsx-structural-corpus")
        })
}

/// 递归收集 (分类, 路径)（#262 模式）。
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
            } else if p.extension().map(|x| x == "xlsx").unwrap_or(false) {
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

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

/// PROVENANCE.json 里登记的语料件是否**任一在盘**（「已取件」信号，
/// 与递归收集相互独立的两条代码路径——walk 退化时此处仍能发现件，
/// #262 原事故形态照红）。清单是脚本生成的稳定格式（json indent=1，
/// `"category"` 行在前 `"name"` 行在后），行级扫描不引 serde
/// （测试面依赖纪律）。
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
            if name.ends_with(".xlsx") && root.join(&cat).join(&name).is_file() {
                return true;
            }
        }
    }
    false
}

#[test]
fn real_xlsx_structural_corpus_lock() {
    let files = corpus_files();
    if files.is_empty() {
        // 三态防御（#262，Lead 评审 2026-11-06 修正）：目录在任何
        // clone 里都在场（README/取件脚本已提交）——「已取件」看
        // **清单件在场性**：PROVENANCE.json 任一登记件在盘但收集
        // 0 件 = 收集逻辑坏 → 红；清单件全缺 = 未取件 → 响亮 skip。
        assert!(
            !provenance_any_present(&corpus_root()),
            "corpus 已取件（PROVENANCE 登记件在盘）但递归收集到 0 件 .xlsx——收集逻辑坏了？({})",
            corpus_root().display()
        );
        eprintln!(
            "[xlsx-struct-corpus] corpus 未取件（清单件全缺，{}）— skipping；取件：python3 test-artifacts/xlsx-structural-corpus/fetch_corpus.py",
            corpus_root().display()
        );
        return;
    }
    println!(
        "[xlsx-struct-corpus] 检查了 {} 件真实语料（{}）",
        files.len(),
        corpus_root().display()
    );
    assert!(files.len() >= 26, "corpus shrank: {}", files.len());

    // 每类数量下限（#265 验收：每类 ≥5——实收 8/8/6/6）。
    let mut per_cat: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (cat, _) in &files {
        *per_cat.entry(cat.as_str()).or_insert(0) += 1;
    }
    for (cat, floor) in [
        ("hidden_rows", 5usize),
        ("external_links", 5),
        ("num_formats", 5),
        ("style_inherit", 5),
    ] {
        let got = per_cat.get(cat).copied().unwrap_or(0);
        assert!(got >= floor, "category {cat}: {got} < {floor}");
    }

    // 全件解析（零 panic——真实件不炸结构面）。
    let mut reports = Vec::new();
    for (cat, p) in &files {
        let bytes = std::fs::read(p).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_structural_from_bytes(&bytes)
        }))
        .unwrap_or_else(|_| panic!("PANIC on {cat}/{}", name(p)));
        let rep = r.unwrap_or_else(|e| panic!("read {cat}/{}: {e}", name(p)));
        reports.push((cat.as_str(), name(p), rep));
    }

    // ---- 隐藏行列形态锁 ----
    let mut files_with_hidden = 0;
    let mut total_hidden_rows = 0;
    for (cat, f, rep) in &reports {
        if *cat != "hidden_rows" {
            continue;
        }
        let hr: usize = rep.sheets.iter().map(|s| s.hidden_rows.len()).sum();
        let hc: usize = rep.sheets.iter().map(|s| s.hidden_cols.len()).sum();
        assert!(
            hr + hc > 0,
            "{f} 应含隐藏行列（分类目录成员——#265 盘点缺口面）"
        );
        files_with_hidden += 1;
        total_hidden_rows += hr;
        // 钉定：55640 = sheet1 四行隐藏（POI bug 附件实值）。
        if f == "55640.xlsx" {
            let s = rep.sheet("deleted rows").or_else(|| rep.sheets.first());
            if let Some(s) = s {
                assert!(!s.hidden_rows.is_empty(), "55640 hidden rows");
            }
        }
    }
    assert_eq!(files_with_hidden, 8, "hidden_rows 8 件全含形态");
    assert!(total_hidden_rows >= 30, "隐藏行总量：{total_hidden_rows}");

    // ---- 外部链接形态锁 ----
    let mut ext_total = 0;
    for (cat, f, rep) in &reports {
        if *cat != "external_links" {
            continue;
        }
        assert!(
            !rep.external_links.is_empty(),
            "{f} 应含外部链接（分类目录成员）"
        );
        for l in &rep.external_links {
            assert!(
                !l.target.is_empty(),
                "{f} 外部链接目标非空（file:///…/UNC/相对路径）"
            );
        }
        ext_total += rep.external_links.len();
        // 钉定：54399 = 双外部链接（POI 附件实值——Top 150 列表 +
        // Craft venue 列表两个外部源）。
        if f == "54399.xlsx" {
            assert!(
                rep.external_links.len() >= 2,
                "54399 双外部链接：{}",
                rep.external_links.len()
            );
        }
    }
    assert!(ext_total >= 9, "外部链接总量：{ext_total}");

    // ---- 数字格式形态锁 ----
    let mut custom_fmt_total = 0;
    let mut builtin_date_seen = false;
    for (cat, f, rep) in &reports {
        if *cat != "num_formats" {
            continue;
        }
        assert!(!rep.styles.custom_fmts.is_empty(), "{f} 应含自定义 numFmt");
        custom_fmt_total += rep.styles.custom_fmts.len();
        // effective 链上 builtin 命中（48962 的 m/d/yyyy 日期段）。
        for i in 0..rep.styles.cell_xfs.len() {
            if let Some(fmt) = rep.styles.effective_num_fmt(i) {
                if fmt == "m/d/yyyy" || fmt == "d-mmm" {
                    builtin_date_seen = true;
                }
            }
        }
    }
    assert!(custom_fmt_total >= 20, "自定义格式总量：{custom_fmt_total}");
    assert!(
        builtin_date_seen,
        "builtin 日期格式（m/d/yyyy）应在 effective 链命中"
    );

    // ---- 样式继承形态锁 ----
    for (cat, f, rep) in &reports {
        if *cat != "style_inherit" {
            continue;
        }
        assert!(
            !rep.styles.cell_xfs.is_empty() || !rep.styles.cell_style_xfs.is_empty(),
            "{f} 应含 xf 定义"
        );
        // 钉定：53568 = 巨型 cellStyleXfs（3088——POI 样式洪水样本，
        // 结构面吞吐锚点）。
        if f == "53568.xlsx" {
            assert!(
                rep.styles.cell_style_xfs.len() >= 3000,
                "53568 巨型 cellStyleXfs：{}",
                rep.styles.cell_style_xfs.len()
            );
        }
        // 钉定：55406 = 641 cellStyleXfs（条件格式样式族）。
        if f == "55406_Conditional_formatting_sample.xlsx" {
            assert!(
                rep.styles.cell_style_xfs.len() >= 600,
                "55406 cellStyleXfs：{}",
                rep.styles.cell_style_xfs.len()
            );
        }
    }
    // effective_num_fmt 越界优雅降级（全语料抽样——None 不 panic）。
    for (_, _, rep) in &reports {
        let _ = rep.styles.effective_num_fmt(usize::MAX);
    }
}
