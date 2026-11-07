//! S1 多级列表自动编号——真实语料 + LibreOffice 开档验证
//! （feat/79-multilevel-numbering）。
//!
//! 语料 = `test-artifacts/health-check/corpus/`（630 件——同
//! style_corpus.rs，三子目录递归收集；#262 修复：顶层 read_dir
//! 空转 2 天，目录 untracked 缺席时 skip、在场空/缩水立红）。
//!
//! numbering 面实况（#262 重钉轮，2026-11-06，630 件）：有
//! numbering.xml 137 / 多级 ≥2 级 129 / lvlRestart 2 / isLgl 2 /
//! startOverride 35（per-num）/ 最深 9 级。
//!
//! 钉定样本（防跑空；#262 重钉——原 lo__* 两件随语料换血缺席，
//! 锚点按新语料实值迁移）：
//! - `docx4j/bug65649.docx`：**真实文档的序号计算全链**——86 个列
//!   表段：前 8 项 `1.→8.` 连续 decimal，后接新列表 `1. 2. 3.`
//!   再深入多级 `3.1. → 3.1.1. → 3.1.2.`（重置 + 多级实档锚点）；
//! - `docx4j/bug59058.docx`：79 项 decimal 长序列（前 14 项
//!   `1.→14.` 无补零连续）；
//! - `poi-docx/NumberingWOverrides.docx`：编号全形态样本（ordinal
//!   lvl0 + `%1` 无点 lvlText——原 poi__ 同件同形，路径迁移）。

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

/// 递归收集 .docx（#262：同 style_corpus.rs——不依赖目录层级，
/// 语料三子目录化后顶层 read_dir 空转 2 天）。
fn corpus_files() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map(|x| x == "docx").unwrap_or(false) {
                out.push(p);
            }
        }
    }
    let dir = corpus_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut v = Vec::new();
    walk(&dir, &mut v);
    v.sort();
    v
}

#[test]
fn real_corpus_numbering_stats_and_pinned() {
    let files = corpus_files();
    if files.is_empty() {
        // 目录缺席 = CI/新克隆合法 skip；在场空 = 结构又变 → 红
        //（#262：同 style_corpus.rs 的静默空转修复）。
        assert!(
            !corpus_dir().is_dir(),
            "corpus dir 在场但递归收集到 0 件 .docx——目录结构又变了？({})",
            corpus_dir().display()
        );
        eprintln!(
            "[numbering-corpus] corpus dir absent ({}) — skipping real-document sweep",
            corpus_dir().display()
        );
        return;
    }
    println!(
        "[numbering-corpus] 检查了 {} 件真实语料（{}）",
        files.len(),
        corpus_dir().display()
    );
    assert!(files.len() >= 600, "corpus shrank: {}", files.len());

    let (mut ok, mut with_num, mut multi, mut restart, mut is_lgl, mut start_over) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut deepest = 0usize;
    let mut seq_649: Option<Vec<String>> = None;
    let mut seq_058: Option<Vec<String>> = None;
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
        // 钉定①③：真实文档的序号计算全链（body 驱动）。
        if name == "bug65649.docx" || name == "bug59058.docx" {
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
                if name == "bug65649.docx" {
                    seq_649 = Some(seq);
                } else {
                    seq_058 = Some(seq);
                }
            }
        }
        if name == "NumberingWOverrides.docx" {
            if let Some(lv) = b.table.effective_levels(1) {
                if let Some(l0) = lv.iter().find(|l| l.ilvl == 0) {
                    assert_eq!(l0.num_fmt, "ordinal");
                    assert_eq!(l0.lvl_text, "%1"); // 无点 lvlText（POI 样本形态）
                    pinned_overrides = true;
                }
            }
        }
    }

    // 容差断言（#262 重钉轮实况见模块文档——630 件：ok 613 /
    // with_num 137 / multi 129 / restart 2 / is_lgl 2 /
    // start_over 35 / deepest 9）。
    assert!(ok >= 600, "readable shrank: {ok}");
    assert!(with_num >= 130, "{with_num}");
    assert!(multi >= 120, "{multi}");
    assert!(restart >= 1, "{restart}");
    assert!(is_lgl >= 1, "{is_lgl}");
    assert!(start_over >= 30, "{start_over}");
    assert!(deepest >= 9, "deepest: {deepest}");

    // 钉定①：bug65649（docx4j）——序号计算全链：前 8 项连续 decimal
    // + 新列表重置 1.2.3. + 多级深入 3.1.→3.1.1.→3.1.2.（重置+
    // 多级实档——原 lo__2129393649 锚点的 #262 迁移）。
    let s1 = seq_649.expect("bug65649.docx not in corpus");
    let want1: Vec<String> = [
        "1.", "2.", "3.", "4.", "5.", "6.", "7.", "8.", "1.", "2.", "3.", "3.1.", "3.1.1.",
        "3.1.2.",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(&s1[..14.min(s1.len())], &want1[..]);
    // 钉定②：bug59058（docx4j）——79 项 decimal 长序列前 14 连续
    //（原 lo__cjklist30 锚点的 #262 迁移；无补零）。
    let s2 = seq_058.expect("bug59058.docx not in corpus");
    let want2: Vec<String> = [
        "1.", "2.", "3.", "4.", "5.", "6.", "7.", "8.", "9.", "10.", "11.", "12.", "13.", "14.",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(&s2[..14.min(s2.len())], &want2[..]);
    // 钉定③：编号全形态样本（原 poi__ 同件，#262 路径迁移）。
    assert!(pinned_overrides, "NumberingWOverrides.docx structure");
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
// ---------------------------------------------------------------------------
// #266：产品接线端到端——read_docx 直出 resolved_number（非手工驱动
// tracker；正文/页眉页脚 wiring + 中文/多级/重置全链实档验证）。
// ---------------------------------------------------------------------------

/// 递归统计有序段/已解析段（正文 + 文本框内块序）。
fn count_ordered_resolved(blocks: &[Block], ordered: &mut usize, resolved: &mut usize) {
    for blk in blocks {
        match blk {
            Block::Paragraph(pd) => {
                if let Some(num) = &pd.numbering {
                    if num.kind == Some(mdc_office_io::docx::ListKind::Ordered) {
                        *ordered += 1;
                        if pd.resolved_number.is_some() {
                            *resolved += 1;
                        }
                    }
                }
            }
            Block::TextBox(tb) => count_ordered_resolved(&tb.blocks, ordered, resolved),
            Block::Table(_) => {}
        }
    }
}

/// 正文块序里的有序段 resolved_number 序列（跳过 bullet/悬空/none）。
fn resolved_sequence(doc: &mdc_office_io::docx::DocxDocument) -> Vec<String> {
    let mut seq = Vec::new();
    for blk in &doc.blocks {
        if let Block::Paragraph(pd) = blk {
            if let Some(rn) = &pd.resolved_number {
                seq.push(rn.clone());
            }
        }
    }
    seq
}

#[test]
fn read_docx_resolved_numbers_pinned_corpus() {
    let files = corpus_files();
    if files.is_empty() {
        assert!(
            !corpus_dir().is_dir(),
            "corpus dir 在场但 0 件 .docx（{}）",
            corpus_dir().display()
        );
        eprintln!("[numbering-corpus] corpus dir absent — skipping #266 wiring gate");
        return;
    }
    let by_name = |want: &str| -> Option<Vec<u8>> {
        files
            .iter()
            .find(|p| p.file_name().map(|n| n == want).unwrap_or(false))
            .and_then(|p| std::fs::read(p).ok())
    };

    // ① bug65649（docx4j）——手工驱动序列的镜像（同一文档序经产品
    //    wiring 出同序：前 8 连续 + 新列表 1.2.3. + 多级 3.1.→3.1.1.→3.1.2.）。
    if let Some(bytes) = by_name("bug65649.docx") {
        let doc = read_docx_from_bytes(&bytes).expect("read bug65649");
        let seq = resolved_sequence(&doc);
        let want: Vec<String> = [
            "1.", "2.", "3.", "4.", "5.", "6.", "7.", "8.", "1.", "2.", "3.", "3.1.", "3.1.1.",
            "3.1.2.",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert!(
            !seq.is_empty(),
            "bug65649：产品接线零 resolved_number——wiring 断链"
        );
        assert_eq!(
            &seq[..14.min(seq.len())],
            &want[..],
            "bug65649 产品序号序列"
        );
    } else {
        eprintln!("[numbering-corpus] bug65649.docx 缺席——钉定①跳过");
    }

    // ② bug59058（docx4j）——79 项长序列前 14 连续 decimal。
    if let Some(bytes) = by_name("bug59058.docx") {
        let doc = read_docx_from_bytes(&bytes).expect("read bug59058");
        let seq = resolved_sequence(&doc);
        let want: Vec<String> = [
            "1.", "2.", "3.", "4.", "5.", "6.", "7.", "8.", "9.", "10.", "11.", "12.", "13.", "14.",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert!(
            !seq.is_empty(),
            "bug59058：产品接线零 resolved_number——wiring 断链"
        );
        assert_eq!(
            &seq[..14.min(seq.len())],
            &want[..],
            "bug59058 产品序号序列"
        );
    } else {
        eprintln!("[numbering-corpus] bug59058.docx 缺席——钉定②跳过");
    }

    // ③ ≥5 件带编号定义的真实语料扫面：每个 kind=Ordered 段要么有
    //    resolved_number、要么 numbering 定义链断（kind 解析面已滤
    //    悬空——feed 返回 None 的只剩超域 ilvl，允许少数）；统计
    //    resolved 覆盖率 ≥ 90%（实况钉定：语料 137 件带编号，绝大多数
    //    全可达）。bullet 段恒无 resolved_number（ListKind 面）。
    let mut numbered_docs = 0usize;
    let mut ordered_paras = 0usize;
    let mut resolved_paras = 0usize;
    for p in &files {
        let bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let doc = match read_docx_from_bytes(&bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let (mut o, mut r) = (0usize, 0usize);
        count_ordered_resolved(&doc.blocks, &mut o, &mut r);
        for hf in doc.headers.iter().chain(doc.footers.iter()) {
            count_ordered_resolved(&hf.blocks, &mut o, &mut r);
        }
        if o > 0 {
            numbered_docs += 1;
        }
        ordered_paras += o;
        resolved_paras += r;
    }
    println!(
        "[numbering-corpus #266 wiring] 带有序列表段的真实语料 {numbered_docs} 件 / \
ordered 段 {ordered_paras} / resolved {resolved_paras}"
    );
    assert!(numbered_docs >= 5, "带编号语料不足 5 件（{numbered_docs}）");
    assert!(
        resolved_paras * 10 >= ordered_paras * 9,
        "resolved 覆盖率 {}/{} < 90%——定义链大面积断？",
        resolved_paras,
        ordered_paras
    );
}
