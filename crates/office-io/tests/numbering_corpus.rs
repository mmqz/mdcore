//! S1 多级列表自动编号——真实语料 + LibreOffice 开档验证
//! （feat/79-multilevel-numbering）。
//!
//! 语料 = `test-artifacts/health-check/corpus/`（204 件——同
//! style_corpus.rs；目录 untracked，缺席时空跑）。
//!
//! numbering 面实况（2026-10-31 首轮）：有 numbering.xml 52 / 多级
//! ≥2 级 49 / lvlRestart 2 / isLgl 3 / startOverride 5 / 整级替换
//! 2 / 最深 9 级。
//!
//! 钉定样本（防跑空）：
//! - `lo__2129393649.docx`：**真实文档的序号计算全链**——body 10
//!   个列表段的计算器输出 `IX.A. → IX.A.1-4 → IX.B. → …`（upperRoman
//!   渲染 + 上级新项级联重置——Word 语义实档锚点）；
//! - `lo__cjklist30.docx`：`1. → 10.`（decimal 长序列）；
//! - `poi__NumberingWOverrides.docx`：POI 编号全形态样本（ordinal
//!   lvl0 + `%1` 无点 lvlText + 深引用字面混合形）。

use mdc_office_io::docx::{read_docx_from_bytes, Block, RunSpec};
use mdc_office_io::numbering::{
    read_docx_numbering_from_bytes, LevelSpec, NumberingDef, NumberingTracker, SuffixKind,
};

fn corpus_dir() -> std::path::PathBuf {
    std::env::var("NUMBERING_CORPUS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            // 2 级上溯 = 仓根（crates/office-io → 仓库）；与
            // external_corpus.rs 的 ../../test-artifacts 先例同构。
            // 3 级上溯会越出仓根（parent-of-repo），任何 clone 布局
            // 下都找不到语料——v2 rebase 时修正（同 style_corpus.rs）。
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test-artifacts/health-check/corpus")
        })
}

fn corpus_files() -> Vec<std::path::PathBuf> {
    let dir = corpus_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "docx").unwrap_or(false))
        .collect();
    v.sort();
    v
}

#[test]
fn real_corpus_numbering_stats_and_pinned() {
    let files = corpus_files();
    if files.is_empty() {
        eprintln!(
            "[numbering-corpus] corpus dir absent ({}) — skipping real-document sweep",
            corpus_dir().display()
        );
        return;
    }
    assert!(files.len() >= 200, "corpus shrank: {}", files.len());

    let (mut ok, mut with_num, mut multi, mut restart, mut is_lgl, mut start_over) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut deepest = 0usize;
    let mut seq_212: Option<Vec<String>> = None;
    let mut seq_cjk: Option<Vec<String>> = None;
    let mut pinned_overrides = false;

    for p in &files {
        let bytes = std::fs::read(p).expect("read corpus file");
        let b = match read_docx_numbering_from_bytes(&bytes) {
            Ok(b) => b,
            Err(_) => continue,
        };
        ok += 1;
        let ids = b.table.num_ids();
        if !ids.is_empty() {
            with_num += 1;
        }
        let mut doc_multi = false;
        let mut doc_restart = false;
        let mut doc_is_lgl = false;
        for id in &ids {
            if let Some(lv) = b.table.effective_levels(*id) {
                if lv.len() >= 2 {
                    doc_multi = true;
                }
                if lv.iter().any(|l| l.lvl_restart.is_some()) {
                    doc_restart = true;
                }
                if lv.iter().any(|l| l.is_lgl) {
                    doc_is_lgl = true;
                }
            }
            if let Some(n) = b.table.num_def(*id) {
                if n.overrides.iter().any(|o| o.start_override.is_some()) {
                    start_over += 1; // per-num 计（非 per-doc——统计面
                }
            }
        }
        for aid in b.table.all_abstract_ids() {
            if let Some(a) = b.table.abstract_def(aid) {
                deepest = deepest.max(a.levels.len());
            }
        }
        if doc_multi {
            multi += 1;
        }
        if doc_restart {
            restart += 1;
        }
        if doc_is_lgl {
            is_lgl += 1;
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // 钉定①：真实文档的序号计算全链（body 驱动）。
        if name == "lo__2129393649.docx" || name == "lo__cjklist30.docx" {
            if let Ok(doc) = read_docx_from_bytes(&bytes) {
                let mut tr = NumberingTracker::new(&b.table);
                let mut seq = Vec::new();
                for blk in &doc.blocks {
                    if let Block::Paragraph(pd) = blk {
                        if let Some(num) = &pd.numbering {
                            if let Some(r) = tr.feed(num.num_id, num.ilvl) {
                                seq.push(r.text);
                            }
                        }
                    }
                }
                if name == "lo__2129393649.docx" {
                    seq_212 = Some(seq);
                } else {
                    seq_cjk = Some(seq);
                }
            }
        }
        if name == "poi__NumberingWOverrides.docx" {
            if let Some(lv) = b.table.effective_levels(1) {
                if let Some(l0) = lv.iter().find(|l| l.ilvl == 0) {
                    assert_eq!(l0.num_fmt, "ordinal");
                    assert_eq!(l0.lvl_text, "%1"); // 无点 lvlText（POI 样本形态）
                    pinned_overrides = true;
                }
            }
        }
    }

    // 容差断言（首轮实况见模块文档）。
    assert!(ok >= 180, "readable shrank: {ok}");
    assert!(with_num >= 45, "{with_num}");
    assert!(multi >= 40, "{multi}");
    assert!(restart >= 1, "{restart}");
    assert!(is_lgl >= 2, "{is_lgl}");
    assert!(start_over >= 3, "{start_over}");
    assert!(deepest >= 9, "deepest: {deepest}");

    // 钉定①：upperRoman "IX" + 上级新项级联重置（A→B 时 1-4 重起）。
    let s1 = seq_212.expect("lo__2129393649.docx not in corpus");
    let want1: Vec<String> = [
        "IX.A.", "IX.A.1.", "IX.A.2.", "IX.A.3.", "IX.A.4.", "IX.B.", "IX.B.1.", "IX.B.2.",
        "IX.B.3.", "IX.B.4.",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(&s1[..10.min(s1.len())], &want1[..]);
    // 钉定②：decimal 长序列 1..10（无补零）。
    let s2 = seq_cjk.expect("lo__cjklist30.docx not in corpus");
    let want2: Vec<String> = ["1.", "2.", "3.", "4.", "5.", "6.", "7.", "8.", "9.", "10."]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(&s2[..10.min(s2.len())], &want2[..]);
    // 钉定③：POI 全形态样本。
    assert!(pinned_overrides, "poi__NumberingWOverrides.docx structure");
}

#[test]
fn libreoffice_opens_define_numbering_output() {
    if std::env::var_os("CI").is_some() {
        eprintln!("[numbering-corpus] CI env — skipping LibreOffice gate");
        return;
    }
    if std::process::Command::new("which")
        .arg("soffice")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        // soffice 在场。
    } else {
        eprintln!("[numbering-corpus] soffice not in PATH — skipping LibreOffice gate");
        return;
    }
    let mut w = mdc_office_io::docx::DocxWriter::new();
    w.define_numbering(NumberingDef {
        id: 300,
        levels: vec![
            LevelSpec {
                ilvl: 0,
                num_fmt: "upperRoman".into(),
                lvl_text: "%1.".into(),
                start: 1,
                lvl_restart: None,
                is_lgl: false,
                suffix: SuffixKind::Tab,
            },
            LevelSpec {
                ilvl: 1,
                num_fmt: "lowerLetter".into(),
                lvl_text: "%1.%2.".into(),
                start: 1,
                lvl_restart: Some(0), // 持续计数跨级
                is_lgl: false,
                suffix: SuffixKind::Space,
            },
            LevelSpec {
                ilvl: 2,
                num_fmt: "lowerRoman".into(),
                lvl_text: "%1.%2.%3.".into(),
                start: 1,
                lvl_restart: None,
                is_lgl: true,
                suffix: SuffixKind::Nothing,
            },
        ],
    })
    .unwrap();
    for (nid, ilvl, text) in [
        (300usize, 0usize, "章一"),
        (300, 1, "节 a"),
        (300, 1, "节 b"),
        (300, 2, "目 i"),
        (300, 0, "章二"),
        (300, 1, "节 c"), // lvlRestart=0：lvl1 持续 → c
        (300, 2, "目 ii"),
    ] {
        w.numbered_list_item(&[RunSpec::text(text)], nid, ilvl)
            .unwrap();
    }
    let dir = std::env::temp_dir().join(format!("lo_num_gate_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let docx = dir.join("gate.docx");
    w.save_to_path(docx.to_str().unwrap()).unwrap();

    let out = std::process::Command::new("soffice")
        .arg("--headless")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&dir)
        .arg(&docx)
        .output()
        .expect("run soffice");
    assert!(
        out.status.success(),
        "soffice failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pdf = dir.join("gate.pdf");
    let meta = std::fs::metadata(&pdf).expect("pdf output missing");
    assert!(meta.len() > 500, "pdf suspiciously small: {}", meta.len());
    let _ = std::fs::remove_dir_all(&dir);
}
