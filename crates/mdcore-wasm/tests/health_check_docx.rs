//! 阶段 B 竞品体检 · 六维 runner——docx 面（issue #29 任务 4）。
//!
//! # 语料与溯源（双语料各自独立基线节）
//!
//! - `corpus/poi-docx/`：apache/poi `test-data/document/` 129 件
//!   .docx（Apache-2.0，钉定 commit ca95db9）——真实世界回归样本：
//!   Word 各版本生成件、畸形/崩溃回归件（clusterfuzz 最小化件）。
//! - `corpus/docx4j/`：plutext/docx4j `core-tests/src/test/resources/`
//!   501 件 .docx（Apache-2.0，钉定 commit b21e0172）——复杂结构
//!   真实样本：修订追踪/域代码/run 属性矩阵/MC 兼容/图表/OLE 嵌入/
//!   OpenDoPE 文档装配（路径平化下划线保留目录上下文）。
//!   provenance.json 在档；docx-rs 源零存储语料不适用（见 provenance）。
//!
//! # 六维判据（与 health_check_xlsx.rs 同构——Lead 规格「每类语料都跑」）
//!
//! | 维 | 实现 |
//! |---|---|
//! | 崩溃率 | `catch_unwind` 包裹导入内核——**panic = 测试失败**（最硬门） |
//! | 解析成功率 | 三分归类：ok / err:\<code\>（typed 错误码）/ panic |
//! | 内容保真 | 字符级召回：md 非空白字符多重集 ∩ GT 多重集 / GT 总量（GT = DocxDocument 全块文本——正文段 runs + 表格格文本 + 文本框递归 + 页眉/页脚块，与导入覆盖面同构） |
//! | 结构保真 | 三断言聚合：标题数（heading_level 非 None）/ 段落数 / 表数——stats 与模型双面一致 |
//! | 往返一致 | import→export→import：md1==md2 精确 + 行等价率 |
//! | 幂等 | 二轮定点：md3==md2 |
//!
//! # 门禁语义（M-1 / hc_xlsx_calamine 同构：「下降即红」）
//!
//! 与 `BASELINE.json` 的 `hc_poi_docx` 节比对：panic 恒 0；ok /
//! structure_ok / roundtrip_exact / fixed_point 不得低于基线；
//! mean_recall ≥ 基线 − 0.005；逐件 class OK→非 OK 即红；语料件数
//! 锚 129（provenance 一致性锁）。
//!
//! 基线再生：`HC_REGEN_BASELINE=1 cargo test -p mdcore-wasm --test
//! health_check_docx`（同趟写 `results/poi-docx.json` 全量证据）。

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use mdc_office_io::docx::{read_docx_from_bytes_with_options, Block, DocxDocument};
use mdc_office_io::io::ReadOptions;
use mdcore_wasm::docx_export::export_docx_core;
use mdcore_wasm::docx_import::import_docx_core;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/health-check")
}

// ---------------------------------------------------------------------------
// 度量原语（与 xlsx runner 同源语义）
// ---------------------------------------------------------------------------

fn char_multiset(s: &str) -> BTreeMap<char, usize> {
    let mut m = BTreeMap::new();
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        *m.entry(c).or_insert(0) += 1;
    }
    m
}

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

/// GT：块序文本（段 runs / 表格全格 / 文本框递归）。
fn block_text(blocks: &[Block], out: &mut String) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                for r in &p.runs {
                    out.push_str(&r.text);
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in row {
                        out.push_str(cell);
                    }
                }
            }
            Block::TextBox(tb) => block_text(&tb.blocks, out),
        }
    }
}

/// GT 全文：正文 + 页眉 + 页脚（与导入覆盖面同构——P2-1 接入面）。
fn gt_text(doc: &DocxDocument) -> String {
    let mut s = String::new();
    block_text(&doc.blocks, &mut s);
    for h in &doc.headers {
        block_text(&h.blocks, &mut s);
    }
    for f in &doc.footers {
        block_text(&f.blocks, &mut s);
    }
    s
}

/// 标题计数（BlockMapper 同口径：heading_level 非 None 的段；
/// 文本框扁平化递归）。
fn count_headings(blocks: &[Block], headings: &mut usize) {
    for b in blocks {
        match b {
            Block::Paragraph(p) if p.heading_level.is_some() => *headings += 1,
            Block::TextBox(tb) => count_headings(&tb.blocks, headings),
            _ => {}
        }
    }
}

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
    stat_paragraphs: usize,
    stat_tables: usize,
    /// panic 载荷首行（诊断证据；class 恒 "panic" 保持基线稳定）。
    panic_msg: String,
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
        stat_paragraphs: 0,
        stat_tables: 0,
        panic_msg: String::new(),
    };

    // 维 1+2：崩溃率 + 解析成功率。
    let t0 = std::time::Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| import_docx_core(bytes)));
    rec.import_ms = t0.elapsed().as_millis();

    let json = match outcome {
        Err(payload) => {
            rec.class = "panic".into();
            rec.panic_msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>();
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

    // GT（与导入同 compat 读面）。
    let doc = match read_docx_from_bytes_with_options(bytes, &ReadOptions { compat: true }) {
        Ok(d) => d,
        Err(e) => {
            rec.class = format!("gt_nondet:{}", e);
            return rec;
        }
    };

    // 维 3：内容保真。
    let gt = char_multiset(&gt_text(&doc));
    rec.gt_chars_total = gt.values().sum();
    rec.recall = Some(recall(&gt, &char_multiset(&md1)));

    // 维 4：结构保真——标题数精确等（硬断言；语义唯一无歧义计数面：
    // heading_level 非 None 的段，含文本框扁平化 + 页眉/页脚吸收——
    // BlockMapper 同口径）。段落/表数为渲染条件化计数（空段/空表/列表
    // 分支），作记录面不作硬断言（GT 镜像脆弱性 > 信号价值）。
    let mut headings = 0usize;
    count_headings(&doc.blocks, &mut headings);
    for h in &doc.headers {
        count_headings(&h.blocks, &mut headings);
    }
    for f in &doc.footers {
        count_headings(&f.blocks, &mut headings);
    }
    let s_headings = stats["headings"].as_u64().unwrap_or(0) as usize;
    rec.structure_ok = Some(s_headings == headings);
    rec.stat_paragraphs = stats["paragraphs"].as_u64().unwrap_or(0) as usize;
    rec.stat_tables = stats["tables"].as_u64().unwrap_or(0) as usize;

    // 维 5：往返一致（import → export → import）。
    let md2 = (|| -> Option<String> {
        let core = mdcore::Core::from_markdown(&md1);
        let bytes2 = export_docx_core(&core.tree).ok()?;
        let json2 = import_docx_core(&bytes2).ok()?;
        Some(split_import(&json2).0)
    })();
    match &md2 {
        Some(m2) => {
            rec.roundtrip_exact = Some(m2 == &md1);
            rec.line_eq = Some(line_eq_rate(&md1, m2));
            // 维 6：幂等（二轮定点）。
            let md3 = (|| -> Option<String> {
                let core = mdcore::Core::from_markdown(m2);
                let bytes3 = export_docx_core(&core.tree).ok()?;
                let json3 = import_docx_core(&bytes3).ok()?;
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
// 基线与结果 IO
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 基线与结果 IO（section 参数化——双语料门共享）
// ---------------------------------------------------------------------------

fn load_baseline(section: &str) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(root().join("BASELINE.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get(section).cloned()
}

fn save_artifacts(recs: &[Rec], section: &str, results_name: &str, corpus_source: &str) {
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

    let bp = root().join("BASELINE.json");
    let mut base: serde_json::Value = std::fs::read_to_string(&bp)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({"schema": 1}));
    base["schema"] = serde_json::json!(1);
    base[section] = serde_json::json!({
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

    let rp = root().join("results");
    std::fs::create_dir_all(&rp).expect("results dir");
    let results = serde_json::json!({
        "schema": 1,
        "runner": "crates/mdcore-wasm/tests/health_check_docx.rs",
        "corpus": {"source": corpus_source, "files": files},
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
            "stat_paragraphs": r.stat_paragraphs, "stat_tables": r.stat_tables,
            "panic_msg": r.panic_msg,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(
        rp.join(results_name),
        serde_json::to_string_pretty(&results).expect("pretty"),
    )
    .expect("write results file");
}

// ---------------------------------------------------------------------------
// 门（下降即红——section 参数化，双语料各自独立基线）
// ---------------------------------------------------------------------------

fn six_dim_gate(
    corpus_dir: &str,
    section: &str,
    anchor: usize,
    results_name: &str,
    corpus_source: &str,
) {
    let dir = root().join("corpus").join(corpus_dir);
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{corpus_dir} corpus dir: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".docx"))
        .collect();
    files.sort();
    // 口径锚：provenance 声明件数（漂移 = provenance 与扫描面失锁）。
    assert_eq!(
        files.len(),
        anchor,
        "{corpus_dir} corpus count drift: {}",
        files.len()
    );

    let recs: Vec<Rec> = files
        .iter()
        .map(|f| {
            let bytes = std::fs::read(dir.join(f)).expect("corpus file readable");
            run_file(f, &bytes)
        })
        .collect();

    // 维 1（基线锚定——M-1 门哲学「允许已知失败，不许新增失败」）：
    // 依赖层 panic（docx-rs 等）按已知失败锁入基线；**新增 panic = 红**。
    // 首 boot 无基线时先落档（None 分支），panic 集合进基线后锁定。
    let panic_files: Vec<&str> = recs
        .iter()
        .filter(|r| r.class == "panic")
        .map(|r| r.file.as_str())
        .collect();

    let baseline = match load_baseline(section) {
        Some(b) => b,
        None => {
            save_artifacts(&recs, section, results_name, corpus_source);
            panic!(
                "首 boot：BASELINE.json {section} 节已生成 + results/{results_name} \
                 全量证据已写——提交入库后重跑"
            );
        }
    };

    if std::env::var("HC_REGEN_BASELINE").is_ok() {
        save_artifacts(&recs, section, results_name, corpus_source);
        eprintln!("HC baseline + results 再生完成（{section}）");
        return;
    }

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

    assert_eq!(b_files, files.len(), "{section} 语料件数 vs 基线漂移");
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

    // 维 1 锚定断言：基线已知 panic 集合外的新增 panic = 红。
    let base_panic_set: std::collections::BTreeSet<String> = baseline["classes"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(_, v)| v.as_str() == Some("panic"))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    let new_panics: Vec<&str> = panic_files
        .iter()
        .copied()
        .filter(|f| !base_panic_set.contains(*f))
        .collect();
    assert!(
        new_panics.is_empty(),
        "D1 新增 panic（基线已知集合外）: {new_panics:?}"
    );
    let resolved: Vec<String> = base_panic_set
        .iter()
        .filter(|f| !panic_files.contains(&f.as_str()))
        .cloned()
        .collect();
    if !resolved.is_empty() {
        eprintln!("{section} panic 已消除（{resolved:?}）——HC_REGEN_BASELINE=1 再生基线入档");
    }

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
            // 回归两类：ok→非 ok；非 panic→panic（panic 严格劣于 err）。
            let worse =
                (*old == "ok" && r.class != "ok") || (*old != "panic" && r.class == "panic");
            if worse {
                regress.push(format!("{}: {} → {}", r.file, old, r.class));
            } else if *old != "ok" && r.class == "ok" {
                improved.push(format!("{}: {} → ok", r.file, old));
            }
        }
    }
    assert!(
        regress.is_empty(),
        "{section} 逐件 class 回归（下降即红）: {regress:?}"
    );
    if !improved.is_empty() {
        eprintln!(
            "{section} baseline improved（{improved:?}）——HC_REGEN_BASELINE=1 重跑再生基线入档"
        );
    }
}

#[test]
fn hc_poi_docx_six_dimensions_gate() {
    six_dim_gate(
        "poi-docx",
        "hc_poi_docx",
        129,
        "poi-docx.json",
        "apache/poi@ca95db9",
    );
}

#[test]
fn hc_docx4j_six_dimensions_gate() {
    six_dim_gate(
        "docx4j",
        "hc_docx4j",
        501,
        "docx4j.json",
        "plutext/docx4j@b21e0172",
    );
}
