//! #13（A1）：DOCX 导入标题样式识别——真 Word 夹具回归（TR-2D
//! batch2 P1-2 复现收口）。
//!
//! 夹具 `test-artifacts/TR2/fixtures/word-complex.docx`（真 Word COM
//! 生成）：中文 Word 的标题样式 **styleId = "1"-"9"**（样式名
//! `heading 1`-`heading 9` + outlineLvl 0-8），非英文 `Heading1` 形——
//! TR-2D 实测读面 `headings: 0`（四级标题全落普通段）的根因。
//!
//! 修复面：`ParagraphData::heading_level`（读面四链解析单一真源——
//! 段落 outlineLvl 直接格式 > pStyle 前缀 > 样式名 heading/紧凑/中文
//! 形 > 样式 outlineLvl）。
//!
//! 注意：issue #13 验收行写「headings == 3、层级 1,2,3」沿自 batch2
//! 报告的记忆性表述；夹具实含 **4 个标题段**（1/2/3/2——COM 原始输出
//! [1][2][6][13] 行），本测试按夹具真实结构钉定（4 段超集覆盖验收
//! 意图：三级层次全命中）。

use mdc_office_io::docx::read_docx_from_bytes_with_options;
use mdc_office_io::docx::Block;
use mdc_office_io::io::ReadOptions;

const FIXTURE: &str = "../../test-artifacts/TR2/fixtures/word-complex.docx";

fn fixture_bytes() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "fixture missing: {} ({e}) — test-artifacts/TR2/fixtures/word-complex.docx \
             是 #13 验收证据件（f8c860a 合入），缺失 = 检出不全",
            path.display()
        )
    })
}

#[test]
fn real_word_chinese_styleids_map_to_heading_levels() {
    let bytes = fixture_bytes();
    // wasm 导入面同款兼容模式（t16 纪律：真机文件走 compat）。
    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("compat read of real-Word fixture");

    let mut headings = Vec::new();
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            if let Some(level) = p.heading_level {
                headings.push((level, p.style.clone(), p.text()));
            }
        }
    }
    // 夹具真实结构（COM 原始输出 [1][2][6][13]）：4 个标题段，
    // 层级 1/2/3/2；styleId 为中文 Word 形 "1"/"2"/"3"。
    assert_eq!(
        headings.iter().map(|(l, _, _)| *l).collect::<Vec<_>>(),
        vec![1, 2, 3, 2],
        "四级标题全命中（headings: 0 修复）+ 层级序列"
    );
    for (level, style, _) in &headings {
        assert_eq!(
            style.as_deref(),
            Some(match level {
                1 => "1",
                2 => "2",
                _ => "3",
            }),
            "中文 Word styleId 透传（1/2/3 形——样式名链命中的载荷面）"
        );
    }
    // 标题文本锚点（COM 对照）：[1] 项目技术方案书 / [6] 1.1 表格
    //（PAGE 域缓存结果污染是 #14 面——本测试只锚标题性）/
    // [13] 第二章 报价。
    assert_eq!(headings[0].2, "项目技术方案书");
    assert!(
        headings[2].2.ends_with("表格"),
        "h3 文本（域结果前缀见 #14）"
    );
    assert_eq!(headings[3].2, "第二章 报价");
}

/// 非标题样式不误判（issue 要求 4）：夹具正文段（样式 "a"/None）
/// 零 heading_level；标题段恰 4（与主测试互补的负向钉定）。
#[test]
fn non_heading_styles_stay_paragraphs() {
    let bytes = fixture_bytes();
    let doc = read_docx_from_bytes_with_options(&bytes, &ReadOptions { compat: true })
        .expect("compat read");
    let mut with_level = 0usize;
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            if p.heading_level.is_some() {
                with_level += 1;
            } else {
                // 不误判面：非标题段的 styleId 只能是 "a"（正文）/
                // None（无 pStyle）——任何 "1"-"9" 段必然已被判为标题。
                assert!(
                    p.style.as_deref().is_none_or(|s| s == "a"),
                    "非标题段携带样式 {:?} 却零层级——样式表解析漏判",
                    p.style
                );
            }
        }
    }
    assert_eq!(with_level, 4, "恰 4 标题段（夹具真实结构）");
}
