//! 阶段 B 竞品体检 · 六维 runner——xlsx 面（issue #29 任务 4）。
//!
//! # 语料与溯源
//!
//! `test-artifacts/health-check/corpus/calamine/`：tafia/calamine `tests/`
//! 75 件 .xlsx（MIT，钉定 commit c922fc0——provenance.json 在档）。含
//! OOM_alloc 资源炸弹系（门拒绝正确性面）、date_1904 纪元边界、CJK
//! 日期、富文本等边界样本。
//!
//! # 六维判据（Lead 规格，「每类语料都跑」）
//!
//! | 维 | 实现 |
//! |---|---|
//! | 崩溃率 | `catch_unwind` 包裹导入内核——**panic = 测试失败**（最硬门，不进基线谈判） |
//! | 解析成功率 | 三分归类：ok / err:\<code\>（typed 错误码）/ panic |
//! | 内容保真 | 字符级召回：md 输出非空白字符多重集 ∩ GT 多重集 / GT 总量（GT = Book 类型化格 display 面——同显示层比对，度量渲染窗/序列化/公式压平损失） |
//! | 结构保真 | 四断言聚合：表数守恒（rendered+dropped==book）/ 非空表数一致 / 行窗零丢弃 / 列窗零丢弃 |
//! | 往返一致 | import→export→import：md1==md2 精确 + 行等价率（已知降级面在档：合并/缓存值/链接 GFM） |
//! | 幂等 | 二轮定点：md3==md2（首轮降级施加后应收敛） |
//!
//! # 门禁语义（M-1 同构：「下降即红」）
//!
//! 与 `test-artifacts/health-check/BASELINE.json` 的 `hc_xlsx_calamine`
//! 节比对：panic 恒 0；ok / structure_ok / roundtrip_exact / fixed_point
//! **不得低于基线**；mean_recall ≥ 基线 − 0.005（0.5pp 数值容差，
//! M-1 l1 同款）；逐件 class **OK→非 OK 即红**（改善则提示再生基线）。
//! 语料件数漂移（≠75）= 红（provenance 与扫描面一致性锁）。
//!
//! 基线再生：`HC_REGEN_BASELINE=1 cargo test -p mdcore-wasm --test
//! health_check_xlsx`（同趟写 `results/calamine.json` 全量证据——基线
//! 与证据是评审物，写后提交入库）。基线缺失 = 首 boot：自动生成并
//! 失败一次，提示提交（M-1「防无基线静默放行」同款）。

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use mdc_office_io::io::{read_book_from_bytes_with_options, Book, ReadOptions};
use mdcore_wasm::xlsx_export::export_xlsx_core;
use mdcore_wasm::xlsx_import::import_xlsx_core;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/health-check")
}

// ---------------------------------------------------------------------------
// 度量原语
// ---------------------------------------------------------------------------

/// 非空白字符多重集。
fn char_multiset(s: &str) -> BTreeMap<char, usize> {
    let mut m = BTreeMap::new();
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        *m.entry(c).or_insert(0) += 1;
    }
    m
}

/// 召回 = |GT ∩ md| / |GT|（GT 空 = 1.0——空簿无内容可丢）。
fn recall(gt: &BTreeMap<char, usize>, md: &BTreeMap<char, usize>) -> f64 {
    let gt_total: usize = gt.values().sum();
    if gt_total == 0 {
        return 1.0;
    }
    let inter: usize = gt
        .iter()
        .map(|(c, n)| md.get(c).copied().unwrap_or(0).min(*n))
        .sum();
    inter as f64 / gt_total as f64
}

/// 行等价率（位置对齐；长度差计入分母）。
fn line_eq_rate(a: &str, b: &str) -> f64 {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();
    let max = la.len().max(lb.len());
    if max == 0 {
        return 1.0;
    }
    let eq = la.iter().zip(lb.iter()).filter(|(x, y)| x == y).count();
    eq as f64 / max as f64
}

/// GT 字符集：Book 全表类型化格 display 面（与导入 md 同显示层——
/// 度量 mdcore 序列化损失，非显示层差异）。
fn gt_chars(book: &Book) -> BTreeMap<char, usize> {
    // 无分隔符直接拼接：多重集按字符计数，跨格拼接不产生新字符；
    // 分隔符会污染分母（md 侧无对应字符——度量面自伤）。
    let mut s = String::new();
    for sheet in &book.sheets {
        let g = &sheet.typed;
        for r in 0..g.height() {
            for c in 0..g.width() {
                if let Some(cell) = g.get((r, c)) {
                    if !matches!(cell, mdc_office_io::io::CellData::Empty) {
                        s.push_str(&cell.display());
                    }
                }
            }
        }
    }
    char_multiset(&s)
}

/// 从导入 JSON 提取 (markdown, stats)。
fn split_import(json: &str) -> (String, serde_json::Value) {
    let v: serde_json::Value = serde_json::from_str(json).expect("import json");
    (
        v["markdown"].as_str().unwrap_or("").to_string(),
        v["stats"].clone(),
    )
}

// ---------------------------------------------------------------------------
// 单件六维
// ---------------------------------------------------------------------------

struct Rec {
    file: String,
    class: String,
    recall: Option<f64>,
    structure_ok: Option<bool>,
    roundtrip_exact: Option<bool>,
    line_eq: Option<f64>,
    fixed_point: Option<bool>,
    gt_chars_total: usize,
    md_lines: usize,
    import_ms: u128,
    rows_dropped: usize,
    cols_dropped: usize,
    sheets_dropped: usize,
}

fn run_file(name: &str, bytes: &[u8]) -> Rec {
    let mut rec = Rec {
        file: name.to_string(),
        class: "ok".into(),
        recall: None,
        structure_ok: None,
        roundtrip_exact: None,
        line_eq: None,
        fixed_point: None,
        gt_chars_total: 0,
        md_lines: 0,
        import_ms: 0,
        rows_dropped: 0,
        cols_dropped: 0,
        sheets_dropped: 0,
    };

    // 维 1+2：崩溃率 + 解析成功率（catch_unwind——panic 即失败门）。
    let t0 = std::time::Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| import_xlsx_core(bytes)));
    rec.import_ms = t0.elapsed().as_millis();

    let json = match outcome {
        Err(_) => {
            rec.class = "panic".into();
            return rec;
        }
        Ok(Err(e)) => {
            rec.class = format!("err:{}", e.code);
            return rec;
        }
        Ok(Ok(j)) => j,
    };

    let (md1, stats) = split_import(&json);
    rec.md_lines = md1.lines().count();

    // GT（与导入同 compat 读面——确定性一致；失败即非确定性发现）。
    let book = match read_book_from_bytes_with_options(bytes, &ReadOptions { compat: true }) {
        Ok(b) => b,
        Err(e) => {
            rec.class = format!("gt_nondet:{}", e);
            return rec;
        }
    };

    // 维 3：内容保真（字符召回）。
    let gt = gt_chars(&book);
    rec.gt_chars_total = gt.values().sum();
    rec.recall = Some(recall(&gt, &char_multiset(&md1)));

    // 维 4：结构保真（四断言聚合）。
    let sheets_rendered = stats["sheets"].as_u64().unwrap_or(0) as usize;
    let sheets_dropped = stats["sheets_dropped"].as_u64().unwrap_or(0) as usize;
    let tables = stats["tables"].as_u64().unwrap_or(0) as usize;
    let non_empty = book.sheets.iter().filter(|s| !s.values.is_empty()).count();
    let rows_dropped = stats["rows_dropped"].as_u64().unwrap_or(0) as usize;
    let cols_dropped = stats["cols_dropped"].as_u64().unwrap_or(0) as usize;
    rec.structure_ok = Some(
        sheets_rendered + sheets_dropped == book.sheets.len()
            && tables == non_empty
            && rows_dropped == 0
            && cols_dropped == 0,
    );
    rec.rows_dropped = rows_dropped;
    rec.cols_dropped = cols_dropped;
    rec.sheets_dropped = sheets_dropped;

    // 维 5：往返一致（import → export → import）。
    let md2 = (|| -> Option<String> {
        let core = mdcore::Core::from_markdown(&md1);
        let bytes2 = export_xlsx_core(&core.tree).ok()?;
        let json2 = import_xlsx_core(&bytes2).ok()?;
        Some(split_import(&json2).0)
    })();
    match &md2 {
        Some(m2) => {
            rec.roundtrip_exact = Some(m2 == &md1);
            rec.line_eq = Some(line_eq_rate(&md1, m2));
            // 维 6：幂等（二轮定点）。
            let md3 = (|| -> Option<String> {
                let core = mdcore::Core::from_markdown(m2);
                let bytes3 = export_xlsx_core(&core.tree).ok()?;
                let json3 = import_xlsx_core(&bytes3).ok()?;
                Some(split_import(&json3).0)
            })();
            rec.fixed_point = Some(md3.as_deref() == Some(m2.as_str()));
        }
        None => {
            rec.roundtrip_exact = Some(false);
            rec.line_eq = None;
            rec.fixed_point = Some(false);
        }
    }
    rec
}

// ---------------------------------------------------------------------------
// 基线与结果 IO（L2 手工提取模式——serde_json::Value，零 derive 依赖）
// ---------------------------------------------------------------------------

fn load_baseline() -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(root().join("BASELINE.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("hc_xlsx_calamine").cloned()
}

fn save_artifacts(recs: &[Rec]) {
    let files = recs.len();
    let ok = recs.iter().filter(|r| r.class == "ok").count();
    let mean_recall = recs.iter().filter_map(|r| r.recall).sum::<f64>()
        / recs.iter().filter(|r| r.recall.is_some()).count().max(1) as f64;
    let structure_ok = recs.iter().filter(|r| r.structure_ok == Some(true)).count();
    let roundtrip_exact = recs
        .iter()
        .filter(|r| r.roundtrip_exact == Some(true))
        .count();
    let fixed_point = recs.iter().filter(|r| r.fixed_point == Some(true)).count();

    // BASELINE.json 的 hc_xlsx_calamine 节（保留他节）。
    let bp = root().join("BASELINE.json");
    let mut base: serde_json::Value = std::fs::read_to_string(&bp)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({"schema": 1}));
    base["schema"] = serde_json::json!(1);
    base["hc_xlsx_calamine"] = serde_json::json!({
        "files": files,
        "ok": ok,
        "structure_ok": structure_ok,
        "roundtrip_exact": roundtrip_exact,
        "fixed_point": fixed_point,
        "mean_recall": (mean_recall * 10000.0).round() / 10000.0,
        "classes": recs.iter().map(|r| (r.file.clone(), serde_json::json!(r.class))).collect::<serde_json::Map<String, serde_json::Value>>(),
    });
    std::fs::write(&bp, serde_json::to_string_pretty(&base).expect("pretty"))
        .expect("write BASELINE.json");

    // results/calamine.json 全量证据。
    let rp = root().join("results");
    std::fs::create_dir_all(&rp).expect("results dir");
    let results = serde_json::json!({
        "schema": 1,
        "runner": "crates/mdcore-wasm/tests/health_check_xlsx.rs",
        "corpus": {"source": "tafia/calamine@c922fc0", "files": files},
        "dimensions": {
            "ok": ok, "panic": recs.iter().filter(|r| r.class == "panic").count(),
            "err": recs.iter().filter(|r| r.class.starts_with("err:")).count(),
            "mean_recall": (mean_recall * 10000.0).round() / 10000.0,
            "structure_ok": structure_ok,
            "roundtrip_exact": roundtrip_exact,
            "fixed_point": fixed_point,
        },
        "per_file": recs.iter().map(|r| serde_json::json!({
            "file": r.file, "class": r.class,
            "recall": r.recall, "structure_ok": r.structure_ok,
            "roundtrip_exact": r.roundtrip_exact, "line_eq": r.line_eq,
            "fixed_point": r.fixed_point,
            "gt_chars": r.gt_chars_total, "md_lines": r.md_lines,
            "import_ms": r.import_ms,
            "rows_dropped": r.rows_dropped, "cols_dropped": r.cols_dropped,
            "sheets_dropped": r.sheets_dropped,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(
        rp.join("calamine.json"),
        serde_json::to_string_pretty(&results).expect("pretty"),
    )
    .expect("write results/calamine.json");
}

// ---------------------------------------------------------------------------
// 门（下降即红）
// ---------------------------------------------------------------------------

#[test]
fn hc_xlsx_calamine_six_dimensions_gate() {
    let dir = root().join("corpus/calamine");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("calamine corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".xlsx"))
        .collect();
    files.sort();
    // 口径锚：provenance 声明 75 件（漂移 = provenance 与扫描面失锁）。
    assert_eq!(
        files.len(),
        75,
        "calamine corpus count drift: {}",
        files.len()
    );

    let recs: Vec<Rec> = files
        .iter()
        .map(|f| {
            let bytes = std::fs::read(dir.join(f)).expect("corpus file readable");
            run_file(f, &bytes)
        })
        .collect();

    // 维 1 硬门：panic 恒 0（不进基线谈判）。
    let panics: Vec<&str> = recs
        .iter()
        .filter(|r| r.class == "panic")
        .map(|r| r.file.as_str())
        .collect();
    assert!(panics.is_empty(), "D1 崩溃率: panic on {panics:?}");

    let baseline = match load_baseline() {
        Some(b) => b,
        None => {
            save_artifacts(&recs);
            panic!(
                "首 boot：BASELINE.json hc_xlsx_calamine 节已生成 + results/calamine.json \
                 全量证据已写——提交入库后重跑（M-1 防无基线静默放行同款）"
            );
        }
    };

    if std::env::var("HC_REGEN_BASELINE").is_ok() {
        save_artifacts(&recs);
        eprintln!("HC baseline + results 再生完成（hc_xlsx_calamine）");
        return;
    }

    // 聚合门。
    let ok = recs.iter().filter(|r| r.class == "ok").count();
    let mean_recall = recs.iter().filter_map(|r| r.recall).sum::<f64>()
        / recs.iter().filter(|r| r.recall.is_some()).count().max(1) as f64;
    let structure_ok = recs.iter().filter(|r| r.structure_ok == Some(true)).count();
    let roundtrip_exact = recs
        .iter()
        .filter(|r| r.roundtrip_exact == Some(true))
        .count();
    let fixed_point = recs.iter().filter(|r| r.fixed_point == Some(true)).count();

    let b_files = baseline["files"].as_u64().unwrap_or(0) as usize;
    let b_ok = baseline["ok"].as_u64().unwrap_or(0) as usize;
    let b_recall = baseline["mean_recall"].as_f64().unwrap_or(1.0);
    let b_struct = baseline["structure_ok"].as_u64().unwrap_or(0) as usize;
    let b_round = baseline["roundtrip_exact"].as_u64().unwrap_or(0) as usize;
    let b_fixed = baseline["fixed_point"].as_u64().unwrap_or(0) as usize;

    assert_eq!(b_files, files.len(), "语料件数 vs 基线漂移");
    assert!(ok >= b_ok, "D2 解析成功率下降: {ok} < 基线 {b_ok}");
    assert!(
        mean_recall >= b_recall - 0.005,
        "D3 内容召回下降: {mean_recall:.4} < 基线 {b_recall:.4} - 0.005"
    );
    assert!(
        structure_ok >= b_struct,
        "D4 结构保真下降: {structure_ok} < 基线 {b_struct}"
    );
    assert!(
        roundtrip_exact >= b_round,
        "D5 往返一致下降: {roundtrip_exact} < 基线 {b_round}"
    );
    assert!(
        fixed_point >= b_fixed,
        "D6 幂等下降: {fixed_point} < 基线 {b_fixed}"
    );

    // 逐件 class 回归门（OK → 非 OK = 红；改善提示再生）。
    let base_classes: BTreeMap<String, String> = baseline["classes"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut regress: Vec<String> = Vec::new();
    let mut improved: Vec<String> = Vec::new();
    for r in &recs {
        if let Some(old) = base_classes.get(&r.file) {
            if *old == "ok" && r.class != "ok" {
                regress.push(format!("{}: ok → {}", r.file, r.class));
            } else if *old != "ok" && r.class == "ok" {
                improved.push(format!("{}: {} → ok", r.file, old));
            }
        }
    }
    assert!(
        regress.is_empty(),
        "逐件 class 回归（下降即红）: {regress:?}"
    );
    if !improved.is_empty() {
        eprintln!("baseline improved（{improved:?}）——HC_REGEN_BASELINE=1 重跑再生基线入档");
    }
}
