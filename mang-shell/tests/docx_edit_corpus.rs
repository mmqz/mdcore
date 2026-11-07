//! #115：真实语料回归（Apache POI docx 语料）。#!/bin/true
//!
//! 语料路径：`MDCORE_DOCX_CORPUS`（缺省 `../../test-artifacts/health-check/
//! corpus/poi-docx`）；不存在即跳过（不红）。
//!
//! 逐件断言（单件崩溃隔离——一档挂不影响全批）：
//! 1. **可编辑装载**：段落序对账通过 → 给出桥+引擎；否则**降级**
//!    （不允许「带着错位映射开工」）；
//! 2. **零编辑零手术**：不编辑 → 回写计划必须为空（引擎一字未动）；
//! 3. **改一段只改这一段**：改首段可编辑文本 → 其余段文本全等；
//! 4. **改完能重开**：保存字节可被引擎重新打开。
//!
//! 用法：
//! ```text
//! MDCORE_DOCX_CORPUS=<dir> cargo test --test docx_edit_corpus -- --ignored --nocapture
//! ```
use std::path::{Path, PathBuf};

use mang_shell::editor::docx_doc::{self, DocxEnginePort};
use mang_shell::editor::docx_edit_bridge::{DocxBridge, DocxPort, LineBlock};
use mdc_office_io::docx::DocxDocument;
use mdc_office_io::docx_edit::DocxEditor;
use mdc_office_io::io;

/// 单件结论。
enum Verdict {
    Editable { sets: usize },
    Degraded(String),
    Unreadable,
    Failed(String),
}

/// 降级原因拆解（决定后续任务的优先级——见 docs/docx-table-edit-design.md）。
#[derive(Default)]
struct DegradeStats {
    table_only: usize,
    textbox: usize,
    engine_gate: usize,
    other: usize,
}

impl DegradeStats {
    fn tally(&mut self, why: &str) {
        if why.contains("表格") {
            self.table_only += 1;
        } else if why.contains("文本框") {
            self.textbox += 1;
        } else if why.contains("引擎") {
            self.engine_gate += 1;
        } else {
            self.other += 1;
        }
    }
}

fn corpus_dir() -> Option<PathBuf> {
    let dir = std::env::var("MDCORE_DOCX_CORPUS")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../test-artifacts/health-check/corpus/poi-docx"));
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    if dir.is_dir() { Some(dir) } else { None }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("语料目录可读")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "docx"))
        .collect();
    out.sort();
    out
}

/// 宽面文档序段落总数（含表格单元格/文本框内段落——与引擎 w:p 同口径）。
///
/// **表格部分用引擎权威栅格**（`DocxEditor::table_cells()` 的去重段落号）：
/// 宽面 `TableData::rows` 是栅格展开形（`gridSpan`/`vMerge` 覆盖列重复
/// 补列），格数 ≠ 引擎 `w:p` 数——拿展开格数去对账就是设计文档 §1 说的
/// 「两个扫描器对第 N 段的定义不同」。`tables == None` 时退回宽面计数。
fn width_paragraph_count(doc: &DocxDocument, tables: Option<&[Vec<Vec<usize>>]>) -> usize {
    fn walk(
        blocks: &[mdc_office_io::docx::Block],
        tables: Option<&[Vec<Vec<usize>>]>,
        cursor: &mut usize,
    ) -> usize {
        blocks
            .iter()
            .map(|b| match b {
                mdc_office_io::docx::Block::Paragraph(_) => 1,
                mdc_office_io::docx::Block::Table(t) => {
                    let idx = *cursor;
                    *cursor += 1;
                    let fenced = tables
                        .and_then(|ts| ts.get(idx))
                        .map(|g| {
                            let mut seen: Vec<usize> = g.iter().flatten().copied().collect();
                            seen.sort_unstable();
                            seen.dedup();
                            seen.len()
                        })
                        .unwrap_or_else(|| t.rows.iter().map(|r| r.len()).sum::<usize>());
                    fenced.max(t.rows.len())
                }
                mdc_office_io::docx::Block::TextBox(tb) => walk(&tb.blocks, tables, cursor),
            })
            .sum()
    }
    let mut cursor = 0usize;
    walk(&doc.blocks, tables, &mut cursor)
}

/// 装行模型 + 建桥（与 `docx_doc::load_editable` 同口径）。
fn mount(bytes: &[u8]) -> Result<(docx_doc::DocxPayload, DocxEnginePort, DocxBridge), String> {
    let opts = io::ReadOptions { compat: true };
    let doc = mdc_office_io::docx::read_docx_from_bytes_with_options(bytes, &opts)
        .map_err(|e| format!("宽面读失败：{e}"))?;
    let port = DocxEnginePort::open(bytes).map_err(|e| format!("引擎门拒：{e}"))?;
    let engine_paras = port.paragraph_count();
    let mut payload = docx_doc::convert_editable(&doc, &port);
    for i in 0..engine_paras {
        payload.projections.entry(i).or_default();
        payload.fallbacks.entry(i).or_default();
    }
    // 三面对账：宽面文档序段落数（+#156 sdt 盲区）= 行模型候选数 =
    // 引擎 w:p 数。任一不等 → 降级（宽面块模型与 document.xml 扫描口径
    // 分歧：文本框嵌套表等）。表格部分用引擎权威栅格数（见
    // width_paragraph_count 文档）。sdt 盲区 = 引擎可见、宽面结构性丢弃
    // 的段落（docx-rs `StructuredDataTag`——与 `docx_doc::load_editable`
    // 门 1 同口径：`width + sdt == engine` 才放行）。
    let engine_tables = mdc_office_io::docx_edit::DocxEditor::open(bytes)
        .map(|e| e.table_cells())
        .unwrap_or_default();
    let sdt_blind = port.sdt_paragraphs().len();
    let width = width_paragraph_count(&doc, Some(&engine_tables));
    if payload.candidates != engine_paras || width + sdt_blind != engine_paras {
        let tables = payload.props.tables;
        let textboxes = payload.props.textboxes;
        let kind = if tables > 0 && textboxes == 0 {
            format!("段落数对账不一致（表格形态：{tables} 表）")
        } else if textboxes > 0 && tables == 0 {
            format!("段落数对账不一致（文本框形态：{textboxes} 个）")
        } else if tables > 0 && textboxes > 0 {
            format!("段落数对账不一致（表格 {tables} + 文本框 {textboxes}）")
        } else {
            "段落数对账不一致（无表格无文本框）".to_string()
        };
        return Err(format!(
            "{kind}：candidates={} width={} engine={engine_paras}",
            payload.candidates, width
        ));
    }
    let mut bridge = DocxBridge::new(
        payload.blocks.clone().ok_or("无块表")?,
        (0..engine_paras)
            .map(|i| Some(port.paragraph_text(i)))
            .collect(),
    );
    for (p, t) in &payload.projections {
        if *p >= engine_paras {
            return Err(format!("投影键越界：{p} >= {engine_paras}"));
        }
        bridge.note_projection(*p, t);
    }
    for (p, t) in &payload.fallbacks {
        if *p >= engine_paras {
            return Err(format!("兜底键越界：{p} >= {engine_paras}"));
        }
        bridge.note_fallback(*p, t);
    }
    bridge.note_cells(payload.cell_paragraphs.clone());
    Ok((payload, port, bridge))
}

/// 单件全链：装载 → 零编辑计划 → 改首段 → 落账 → 重开。
fn one_file(path: &Path) -> Verdict {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return Verdict::Unreadable,
    };
    let (payload, port, bridge) = match mount(&bytes) {
        Ok(v) => v,
        Err(why) => return Verdict::Degraded(why),
    };
    let n = port.paragraph_count();
    let lines: Vec<String> = payload.text.split('\n').map(|s| s.to_string()).collect();
    // ① 零编辑零手术。
    let plan0 = bridge.compute(&lines);
    if !plan0.sets.is_empty() {
        let mut detail = String::new();
        for (p, new) in plan0.sets.iter().take(3) {
            detail.push_str(&format!(
                " | p={p} new={new:?} orig={:?} proj={:?} fb={:?}",
                port.paragraph_text(*p),
                payload.projections.get(p),
                payload.fallbacks.get(p)
            ));
            if *p >= port.paragraph_count() {
                detail.push_str(" (KEY OUT OF RANGE)");
            }
        }
        return Verdict::Failed(format!("零编辑却出 {} 条 set{detail}", plan0.sets.len()));
    }
    // ② 改首段可编辑文本。
    let blocks = payload.blocks.clone().unwrap_or_default();
    let Some(line) = blocks
        .iter()
        .position(|b| matches!(b, LineBlock::Para { .. }))
    else {
        return Verdict::Editable { sets: 0 };
    };
    let mut edited = lines.clone();
    edited[line] = format!("{}（#115 语料改写）", edited[line]);
    let plan = bridge.compute(&edited);
    if plan.sets.is_empty() {
        return Verdict::Editable { sets: 0 };
    }
    let mut ed = match DocxEditor::open(&bytes) {
        Ok(e) => e,
        Err(e) => return Verdict::Degraded(format!("引擎门拒：{e}")),
    };
    for (p, t) in &plan.sets {
        if let Err(e) = ed.set_paragraph_text(*p, t) {
            return Verdict::Failed(format!("set(p={p}) 被引擎拒：{e}"));
        }
    }
    let out = match ed.save_to_vec() {
        Ok(b) => b,
        Err(e) => return Verdict::Failed(format!("落账失败：{e}")),
    };
    let re = match DocxEditor::open(&out) {
        Ok(e) => e,
        Err(e) => return Verdict::Failed(format!("改后重开失败：{e}")),
    };
    let src = DocxEditor::open(&bytes).expect("mount 已证可开");
    if std::env::var("CORPUS_DBG").is_ok() {
        eprintln!("DBG file ok sets={}", plan.sets.len());
    }
    let m = re.paragraphs().len().min(n);
    for i in 0..m {
        if plan.sets.iter().any(|(p, _)| *p == i) {
            continue;
        }
        if re.paragraph_text(i) != src.paragraph_text(i) {
            return Verdict::Failed(format!("非目标段 {i} 文本变了"));
        }
    }
    Verdict::Editable {
        sets: plan.sets.len(),
    }
}

#[test]
#[ignore = "真实语料回归（MDCORE_DOCX_CORPUS 指目录）"]
fn corpus_load_roundtrip_and_single_edit() {
    let Some(dir) = corpus_dir() else {
        eprintln!("corpus: 跳过（目录不存在——设 MDCORE_DOCX_CORPUS）");
        return;
    };
    let all = files(&dir);
    let (mut editable, mut degraded, mut unreadable, mut set_total) = (0, 0, 0, 0usize);
    let mut dstats = DegradeStats::default();
    let mut degraded_files: Vec<(String, String)> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for path in &all {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let p2 = path.clone();
        // 单件隔离：一档 panic（对账口径分歧等）记失败但不中断全批。
        let verdict = match std::panic::catch_unwind(move || one_file(&p2)) {
            Ok(v) => v,
            Err(e) => {
                let why = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "未知 panic".to_string());
                failures.push(format!("{name}：装载/回写崩溃 → {why}"));
                continue;
            }
        };
        match verdict {
            Verdict::Editable { sets } => {
                editable += 1;
                set_total += sets;
            }
            Verdict::Degraded(why) => {
                degraded += 1;
                dstats.tally(&why);
                degraded_files.push((name.clone(), why));
            }
            Verdict::Unreadable => unreadable += 1,
            Verdict::Failed(why) => {
                editable += 1;
                failures.push(format!("{name}：{why}"));
            }
        }
    }
    eprintln!(
        "corpus: 共 {} 件 / 可编辑 {} / 降级 {} / 不可读 {} / set 合计 {}",
        all.len(),
        editable,
        degraded,
        unreadable,
        set_total
    );
    eprintln!(
        "corpus-degrade: 表格形态 {} / 文本框形态 {} / 引擎门拒 {} / 其他 {}",
        dstats.table_only, dstats.textbox, dstats.engine_gate, dstats.other
    );
    for (n, why) in &degraded_files {
        eprintln!("corpus-degrade-file: {n} — {why}");
    }
    for f in &failures {
        eprintln!("corpus-fail: {f}");
    }
    assert!(
        failures.is_empty(),
        "语料回归失败 {} 条（见 corpus-fail 行）",
        failures.len()
    );
}
