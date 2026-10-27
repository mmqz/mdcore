//! #14（A2）：读面域状态机——表现型域缓存结果剥离（TR-2D batch2
//! P1-1 真根因收口）。
//!
//! 真根因（夹具解剖证伪「编号叠加」说）：`word-complex.docx` 的 h3
//! 标题段内嵌 **PAGE 域**（`fldChar begin` → instrText ` PAGE \*
//! MERGEFORMAT ` → `separate` → 缓存结果 run `<w:t>1</w:t>` → `end`），
//! 读面把缓存结果当普通文本 → 标题文本 "1" + "1.1 表格" = **"11.1
//! 表格"**（Word COM 显示 "1.1 表格"）。修复 = [`FieldMachine`] 域
//! 类型分派（表现型剥离 / 内容型保留）。
//!
//! 验收对照（issue #14）：
//! - 导入 word-complex.docx → 标题文本 **"1.1 表格"**（非 "11.1 表格"）
//! - 回归：无编号标题「第一章 概述」保持不变（导入恒不加号——
//!   vacuous 满足 + 本测试钉定）
//! - numPr 来源走**剥离**（标题赢——r5-t1 既有纪律，docx_import 模块
//!   文档登记；宁缺毋错不发明数字）

use mdc_office_io::docx::read_docx_from_bytes_with_options;
use mdc_office_io::docx::Block;
use mdc_office_io::io::ReadOptions;

const FIXTURE: &str = "../../test-artifacts/TR2/fixtures/word-complex.docx";

fn read_fixture() -> mdc_office_io::docx::DocxDocument {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("fixture missing: {} ({e})", path.display()));
    read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("compat read of real-Word fixture")
}

/// 真夹具主验收：PAGE 域缓存结果剥离 → h3 文本 "1.1 表格"；
/// stats 载荷（fields_dropped）计数。
#[test]
fn real_word_page_field_result_stripped() {
    let doc = read_fixture();
    let mut h3_text = None;
    let mut total_dropped = 0usize;
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            total_dropped += p.fields_dropped;
            if p.heading_level == Some(3) {
                h3_text = Some(p.text());
            }
        }
    }
    assert_eq!(
        h3_text.as_deref(),
        Some("1.1 表格"),
        "PAGE 域缓存结果 \"1\" 剥离——非 \"11.1 表格\"（TR-2D P1-1 真根因）"
    );
    assert_eq!(
        total_dropped, 1,
        "恰一个表现型域被剥离（夹具仅 h3 内嵌 PAGE 域）"
    );
}

/// 回归钉定（issue 要求 4：不过度修正）：无编号标题逐字保留——
/// 导入链从不添加编号（「第一章 概述」不变；数字型 "1.1" 前缀已
/// 在文本中，同样逐字保留）。
#[test]
fn unnumbered_headings_verbatim_no_overcorrection() {
    let doc = read_fixture();
    let texts: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) if p.heading_level.is_some() => Some(p.text()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            "项目技术方案书".to_string(),
            // #17 后正确形态：该标题段含 w:ins（REV-INSERT- 前缀=当前生效
            // 文本，带 revision_inserted 标记提取——逐字保留仍成立）。
            "REV-INSERT-第一章 概述".to_string(),
            "1.1 表格".to_string(),
            "第二章 报价".to_string(),
        ],
        "四标题逐字（中文序数形不变形 + 已有数字前缀不重复不剥除）"
    );
}

/// 内容型域保留环（写→读全环）：写面 TOC 域发射 begin + instr +
/// separate + 缓存结果 + end —— 读面 TOC 非表现型 → 结果保留
/// （既有 roundtrip 行为零变化的面钉定）。
#[test]
fn content_field_toc_result_kept_roundtrip() {
    use mdc_office_io::docx::{DocxWriter, RunSpec};
    let mut w = DocxWriter::new();
    w.paragraph(&[RunSpec::field_toc()]);
    let bytes = w.save_to_vec().expect("save");
    let doc =
        read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true }).expect("read");
    let paras: Vec<&mdc_office_io::docx::ParagraphData> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(paras.len(), 1);
    assert!(
        paras[0].text().contains("目录域"),
        "TOC 域缓存结果（占位文本）保留：{:?}",
        paras[0].text()
    );
    assert_eq!(paras[0].fields_dropped, 0, "内容型域零剥离");
}
