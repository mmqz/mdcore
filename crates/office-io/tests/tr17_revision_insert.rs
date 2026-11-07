//! #17（P0）：修订插入（w:ins）当前文本提取 + delText 登记面。
//!
//! 夹具 `test-artifacts/corpusR/D2a-nonbody.docx`（源：ai-bid-research
//! 仓 `fixtures/corpus-R/D2-nonbody/D2a-非正文条款.docx`——澄清文件
//! 样本，含 w:ins/w:del 修订对 + 文本框 + 批注通道）。
//!
//! OOXML 语义：`w:ins` 内 = **当前生效文本**（必须提取——跳过整容器
//! 会静默丢失生效条款，截止时间类为废标级）；`w:delText` = 已删除
//! 内容（不提取，计数登记——`WarningKind::RevisionDelete`）。
//!
//! 验收（issue #17）：ins 内文本在读回文本且 run 带
//! `revision_inserted=true`；delText 不在读回文本、warning 计数 ≥1。

use mdc_office_io::docx::read_docx_from_bytes_with_options;
use mdc_office_io::docx::Block;
use mdc_office_io::io::{ReadOptions, WarningKind};

const FIXTURE: &str = "../../test-artifacts/corpusR/D2a-nonbody.docx";
const INS_TEXT: &str = "（修订插入：响应截止时间顺延至 2026-12-01 09:30）";
const HOST_PARA: &str =
    "澄清四：响应截止时间口径如下——（修订插入：响应截止时间顺延至 2026-12-01 09:30）";
const DEL_TEXT: &str = "原表述：响应截止时间 2026-11-20 09:30";

fn read_fixture() -> mdc_office_io::docx::DocxDocument {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("compat read of revision fixture")
}

#[test]
fn revision_insert_text_extracted_with_flag() {
    let doc = read_fixture();
    // 宿主段完整（ins 文本并入段落流——非独立段）。
    let all_text: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    assert!(
        all_text.iter().any(|t| t == HOST_PARA),
        "宿主段完整（ins 文本内联）: {all_text:?}"
    );
    // run 级标记：ins 内 run revision_inserted=true；普通 run false。
    let mut ins_runs = 0usize;
    let mut flagged_text = String::new();
    let mut normal_flagged = 0usize;
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            for r in &p.runs {
                if r.revision_inserted {
                    ins_runs += 1;
                    flagged_text.push_str(&r.text);
                } else if r.text.contains("修订插入") {
                    normal_flagged += 1;
                }
            }
        }
    }
    assert_eq!(flagged_text, INS_TEXT, "ins 容器内文本完整提取且带标记");
    assert!(ins_runs >= 1, "至少一个 revision_inserted run");
    assert_eq!(normal_flagged, 0, "普通 run 零误标");
    // delText 不出现在任何读回文本。
    assert!(
        all_text.iter().all(|t| !t.contains(DEL_TEXT)),
        "已删除文本零提取: {all_text:?}"
    );
    assert!(
        !flagged_text.contains("原表述"),
        "del 容器内容不入 ins 标记面"
    );
}

#[test]
fn revision_delete_registered_as_warning() {
    let doc = read_fixture();
    let rev_warnings: Vec<_> = doc
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::RevisionDelete)
        .collect();
    assert!(
        !rev_warnings.is_empty(),
        "warning 计数 ≥1（issue 验收）：{:?}",
        doc.warnings
    );
    assert!(
        rev_warnings
            .iter()
            .any(|w| w.detail.contains("revision-deleted text run")),
        "登记面消息形态"
    );
}

// ---------------------------------------------------------------------------
// #343（#318 D3）：hyperlink 内嵌 w:ins 的 run 提取。
// ---------------------------------------------------------------------------

/// 实锚：结构账本 #260 GT 对照发现的内容损失件——「as the lease
/// says.」中 *lease* 一词在 `w:hyperlink > w:ins > w:r`（修订插入的
/// 超链接文本），原先 Hyperlink 分支只提 Run 直接子 → 静默丢词。
const HC_FIXTURE: &str =
    "../../test-artifacts/health-check/corpus/docx4j/revisions_editor_review_review-for-word.docx";
/// 宿主段完整文本（ins 三段 + 超链接词 + 尾段——修复后应零缺词）。
const HOST_PARA_FULL: &str = "The tenant pays rent monthly in advance, as the lease says.";
/// 超链接目标（word/_rels/document.xml.rels rIdmrgxhxfn → External）。
const LEASE_TARGET: &str = "https://www.docx4java.org/";

#[test]
fn hyperlink_insert_run_extracted_with_flag_and_target() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(HC_FIXTURE);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("compat read of review-for-word");

    // ① 宿主段完整（hyperlink > ins 文本内联——不再缺词）。
    let all_text: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect();
    assert!(
        all_text.iter().any(|t| t == HOST_PARA_FULL),
        "宿主段零缺词（lease 在场）: {all_text:?}"
    );

    // ② run 三元：text=lease + revision_inserted 标记 + 目标挂接。
    let mut lease_runs = 0usize;
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            for r in &p.runs {
                if r.text == "lease" {
                    assert!(
                        r.revision_inserted,
                        "hyperlink > ins 的 run 带修订标记（#17 语义）"
                    );
                    match &r.hyperlink {
                        Some(mdc_office_io::docx::HyperlinkTarget::External { target }) => {
                            assert_eq!(target, LEASE_TARGET, "目标关系联接")
                        }
                        other => panic!("lease run 目标应为 External：{other:?}"),
                    }
                    lease_runs += 1;
                }
            }
        }
    }
    assert_eq!(lease_runs, 1, "lease run 恰一（提取不重复）");

    // ③ 同文件段内裸 ins（id=43/44——hyperlink 外）维持原提取面，
    //    回归保护（修复只扩 hyperlink 容器内，不动段落级）。
    let mut flagged = 0usize;
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            flagged += p.runs.iter().filter(|r| r.revision_inserted).count();
        }
    }
    assert!(
        flagged >= lease_runs + 5,
        "段落级 ins run 面不回归（id=43 五 run + id=44 一 run + lease）: {flagged}"
    );
}
