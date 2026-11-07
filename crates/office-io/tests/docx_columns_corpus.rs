//! #268：分栏读取面——真实多栏语料门（docx-columns-corpus）。
//!
//! 语料 = `test-artifacts/docx-columns-corpus/`（13 件，LibreOffice
//! 测试仓 `sw/qa/extras/ooxmlexport/data/`——MPL-2.0，Word/LO 互操作
//! 回归样本；来源/sha256 见 `PROVENANCE.json` + `fetch_corpus.py`）。
//! **docx 二进制直接入库**（<21KB/件——与 health-check/corpus 642 件
//! docx 同例，CI 无网可跑），缺席 = 红（非 SKIP——入库件被删是事故）。
//!
//! 覆盖面（#268 验收「≥5 件真实多栏 docx」→ 13 件）：
//!
//! | 形态 | 件 |
//! |---|---|
//! | num=2/3/4 基础栏 | 2col-header / tdf121669 / tdf135343 等 |
//! | equalWidth=false + w:col 子表 | tdf121665（3 栏三宽全自定义） |
//! | 栏分隔线 sep | multi-column-separator-with-line / tdf153592 |
//! | 显式分栏符 | columnbreak / tdf121665 / tdf153592 |
//! | 多节异构分栏 | tdf121670 / tdf135343 / tdf46940（None/2 栏交替） |
//!
//! 顺带钉定 **#257 遗留缺陷的修复**：docx-rs 0.4.22 读面不解析
//! `w:cols`（`SectionProperty::read` 无 cols 分支——`columns` 恒 1），
//! `SectionData.columns` 此前对任何多栏文档都误报 1；#268 改由
//! office-io 自有 XML 第二趟真源派生（本门的多栏断言即修复证据）。

use mdc_office_io::docx::read_docx_from_bytes;
use mdc_office_io::docx::ColumnWidth;

fn corpus_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-artifacts/docx-columns-corpus")
}

fn read(name: &str) -> mdc_office_io::docx::DocxDocument {
    let p = corpus_dir().join(name);
    let bytes =
        std::fs::read(&p).unwrap_or_else(|e| panic!("语料 {name} 缺席（入库件被删？）: {e}"));
    read_docx_from_bytes(&bytes).unwrap_or_else(|e| panic!("语料 {name} 可读性门失败: {e}"))
}

/// 全 13 件在场（入库二进制——缺席即红，防静默空转）。
#[test]
fn corpus_all_present() {
    let dir = corpus_dir();
    let mut n = 0;
    for name in [
        "2col-header.docx",
        "alphabeticalIndex_MultipleColumns.docx",
        "columnbreak.docx",
        "default-sect-break-cols.docx",
        "multi-column-line-separator-SAVED.docx",
        "multi-column-separator-with-line.docx",
        "tdf121665_back2backColumnBreaks.docx",
        "tdf121669_equalColumns.docx",
        "tdf121670_columnsInSectionsOnly.docx",
        "tdf135343_columnSectionBreak_c12v3.docx",
        "tdf153592_columnBreaks.docx",
        "tdf46940_dontEquallyDistributeColumns.docx",
        "unbalanced-columns.docx",
    ] {
        assert!(dir.join(name).exists(), "入库语料缺席: {name}");
        n += 1;
    }
    assert_eq!(n, 13);
}

/// cols 全字段钉定（各形态代表件——值取自各件 document.xml 实值）。
#[test]
fn columns_spec_pinned_shapes() {
    // ① 基础双栏 + 显式分栏符（栏归属可判）。
    let doc = read("columnbreak.docx");
    assert_eq!(doc.sections.len(), 1);
    let s = &doc.sections[0];
    assert_eq!(s.columns, 2, "num=2（#257 缺陷修复面：此前恒 1）");
    let spec = s.columns_spec.as_ref().unwrap();
    assert_eq!(spec.num, Some(2));
    assert_eq!(spec.space, Some(708));
    assert!(!s.column_break_blocks.is_empty(), "显式分栏符可观测");
    // 符后块栏号 1（栏归属）。
    let &brk = s.column_break_blocks.first().unwrap();
    assert_eq!(s.column_of(brk), Some(0), "含符块取起始栏");
    let after = (s.block_range.0..s.block_range.1)
        .find(|&b| b > brk)
        .expect("符后有块");
    assert_eq!(s.column_of(after), Some(1), "符后块 → 栏 1");

    // ② equalWidth=false + w:col 子表（全自定义三栏宽）。
    let doc = read("tdf121665_back2backColumnBreaks.docx");
    let s = &doc.sections[0];
    assert_eq!(s.columns, 3);
    let spec = s.columns_spec.as_ref().unwrap();
    assert_eq!(spec.num, Some(3));
    assert_eq!(spec.space, None, "space 缺席不代填");
    assert_eq!(spec.equal_width, Some(false));
    assert_eq!(spec.separator, Some(false));
    assert_eq!(
        spec.columns,
        vec![
            ColumnWidth {
                width: Some(2669),
                space: Some(708)
            },
            ColumnWidth {
                width: Some(2315),
                space: Some(708)
            },
            ColumnWidth {
                width: Some(2671),
                space: None
            },
        ],
        "w:col 子表（文档序）"
    );
    assert!(!s.column_break_blocks.is_empty());

    // ③ 栏分隔线 sep=1 + 多节。
    let doc = read("multi-column-separator-with-line.docx");
    assert_eq!(doc.sections.len(), 2);
    let s0 = &doc.sections[0];
    assert_eq!(s0.columns, 2);
    assert_eq!(s0.columns_spec.as_ref().unwrap().separator, Some(true));
    let s1 = &doc.sections[1];
    assert_eq!(
        s1.columns_spec.as_ref().unwrap().separator,
        None,
        "第二节 sep 缺席"
    );

    // ④ 多节异构：None/2 栏交替（无 cols 的节 = 缺省单栏）。
    let doc = read("tdf46940_dontEquallyDistributeColumns.docx");
    assert_eq!(doc.sections.len(), 6);
    for (i, s) in doc.sections.iter().enumerate() {
        if i % 2 == 0 {
            assert!(s.columns_spec.is_none(), "节 {i}：无 cols → None");
            assert_eq!(s.columns, 1);
        } else {
            assert_eq!(s.columns, 2, "节 {i}：双栏");
            let spec = s.columns_spec.as_ref().unwrap();
            assert_eq!(spec.num, Some(2));
            assert_eq!(spec.equal_width, Some(true));
            assert_eq!(spec.separator, Some(true));
        }
    }

    // ⑤ 多节异构栏数（4 → 2）+ 双节三栏全属性 + 三节 3/3/缺省。
    let doc = read("tdf135343_columnSectionBreak_c12v3.docx");
    assert_eq!(doc.sections.len(), 2);
    assert_eq!(doc.sections[0].columns, 4);
    assert_eq!(doc.sections[1].columns, 2);

    let doc = read("tdf153592_columnBreaks.docx");
    assert_eq!(doc.sections.len(), 2);
    for s in &doc.sections {
        assert_eq!(s.columns, 3);
        let spec = s.columns_spec.as_ref().unwrap();
        assert_eq!(spec.num, Some(3));
        assert_eq!(spec.space, Some(850));
        assert_eq!(spec.equal_width, Some(true));
        assert_eq!(spec.separator, Some(false));
    }

    let doc = read("tdf121670_columnsInSectionsOnly.docx");
    assert_eq!(doc.sections.len(), 3);
    assert_eq!(doc.sections[0].columns, 3);
    assert_eq!(doc.sections[1].columns, 3);
    assert_eq!(doc.sections[2].columns, 1, "末节无 num → 缺省单栏");
    assert_eq!(
        doc.sections[2].columns_spec.as_ref().unwrap().space,
        Some(720)
    );
}

/// 全语料结构不变量：节区间连续切分 + cols 一致性（spec.num == columns）
/// + 分栏符块在节内 + column_of 与符计数一致（含钳制）。
#[test]
fn corpus_structural_invariants() {
    let files = [
        "2col-header.docx",
        "alphabeticalIndex_MultipleColumns.docx",
        "columnbreak.docx",
        "default-sect-break-cols.docx",
        "multi-column-line-separator-SAVED.docx",
        "multi-column-separator-with-line.docx",
        "tdf121665_back2backColumnBreaks.docx",
        "tdf121669_equalColumns.docx",
        "tdf121670_columnsInSectionsOnly.docx",
        "tdf135343_columnSectionBreak_c12v3.docx",
        "tdf153592_columnBreaks.docx",
        "tdf46940_dontEquallyDistributeColumns.docx",
        "unbalanced-columns.docx",
    ];
    let mut multicol_sections = 0usize;
    let mut total_breaks = 0usize;
    for name in files {
        let doc = read(name);
        // 节区间连续切分（分区不变量——#257 同门复跑）。
        assert!(!doc.sections.is_empty());
        assert_eq!(doc.sections[0].block_range.0, 0, "{name}: 首节起点 0");
        for w in doc.sections.windows(2) {
            assert_eq!(
                w[0].block_range.1, w[1].block_range.0,
                "{name}: 节 {} end = 节 {} start",
                w[0].index, w[1].index
            );
        }
        assert_eq!(
            doc.sections.last().unwrap().block_range.1,
            doc.blocks.len(),
            "{name}: 末节 end = blocks.len()"
        );
        for s in &doc.sections {
            // cols 一致性：spec.num（XML 真源）与 columns 派生一致。
            if let Some(spec) = &s.columns_spec {
                if let Some(n) = spec.num {
                    assert_eq!(
                        s.columns,
                        n.max(1) as usize,
                        "{name}: 节 {} num 一致",
                        s.index
                    );
                }
            }
            if s.columns >= 2 {
                multicol_sections += 1;
            }
            // 分栏符块 ∈ 节区间 + 升序。
            let (start, end) = s.block_range;
            for pair in s.column_break_blocks.windows(2) {
                assert!(pair[0] < pair[1], "{name}: 节 {} 符块升序", s.index);
            }
            for &b in &s.column_break_blocks {
                assert!(
                    b >= start && b < end,
                    "{name}: 节 {} 符块 {b} ∈ [{start},{end})",
                    s.index
                );
            }
            total_breaks += s.column_break_blocks.len();
            // column_of 一致性：符前计数（钳制）。
            for &b in &s.column_break_blocks {
                let prior = s.column_break_blocks.iter().filter(|&&x| x < b).count();
                let expect = prior.min(s.columns.saturating_sub(1));
                assert_eq!(
                    s.column_of(b),
                    Some(expect),
                    "{name}: 节 {} 含符块 {b} 归属",
                    s.index
                );
            }
            // 首块恒栏 0。
            if start < end {
                assert_eq!(
                    s.column_of(start),
                    Some(0),
                    "{name}: 节 {} 首块栏 0",
                    s.index
                );
            }
        }
    }
    // 防跑空下限：多栏节 ≥13（13 件 ≥1 各 1）+ 分栏符 ≥3 件面。
    assert!(multicol_sections >= 13, "多栏节 {multicol_sections} < 13");
    assert!(total_breaks >= 3, "显式分栏符观测 {total_breaks} < 3");
}

/// 词内语料交叉面：health-check 语料唯一多栏件（Word 365 产 num=2）
/// ——与 LO 语料产线互补（双产线钉定）。
#[test]
fn healthcheck_corpus_word365_two_column() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-artifacts/health-check/corpus/docx4j")
        .join("fields_cr067-phase3_phase3-probe-r1-word365.docx");
    let Ok(bytes) = std::fs::read(&p) else {
        eprintln!("[SKIP] health-check 语料缺席（CI 无语料目录）— Word365 交叉面未验证");
        return;
    };
    let doc = read_docx_from_bytes(&bytes).expect("Word365 件可读");
    let s = doc
        .sections
        .iter()
        .find(|s| s.columns >= 2)
        .unwrap_or_else(|| panic!("Word365 件应含双栏节（此前恒 1 = #257 缺陷）"));
    assert_eq!(s.columns, 2);
    assert_eq!(s.columns_spec.as_ref().unwrap().num, Some(2));
    assert_eq!(s.columns_spec.as_ref().unwrap().space, Some(720));
}
