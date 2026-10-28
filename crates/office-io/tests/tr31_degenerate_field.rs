//! #40 裁决钉定：WPS 退化域读面语义——指令文本不入正文（渲染保真）。
//!
//! 背景（ai-bid-research B3-3 全量差分 4 块 UNCLASSIFIED）：songjiang
//! 件协议名与价格条款的差异，标书侧初判为「`""` 邻接**字面正文**被
//! A2 表现型域剥离吞除」。XML 现场复核**推翻该前提**——文本位于
//! `w:instrText`（域指令面），非 `w:t`（字面正文面）：
//!
//! ```xml
//! <w:fldChar w:fldCharType="begin"/>          <!-- 无 w:dirty -->
//! <w:instrText xml:space="preserve">&quot;&quot;施工/服务</w:instrText>
//! <w:fldChar w:fldCharType="end"/>            <!-- 无 separate/缓存结果 -->
//! ```
//!
//! 裁决 = **既定语义**（非缺陷）：指令文本恒不入正文（A2 既有纪律，
//! 模块文档登记），四独立实现面共识互证：
//!
//! | 实现面 | 行为 | 证据 |
//! |---|---|---|
//! | LibreOffice 25.2 渲染 | 空（168 页 PDF `施工/服务`/`不高于` 0 命中） | docs/field-code-semantics.md §3 |
//! | LibreOffice ODT 导入 | `UNHANDLED` 域 + **空结果区** | 同上 §3 |
//! | pandoc 3.1.11 | 0 命中（与引擎一致） | 同上 §3 |
//! | python-docx | 0 命中（仅读 w:t 面） | 同上 §3 |
//!
//! 语料旁证：全语料 5 个 `""` 退化域**零缓存结果**（渲染器从未显示过
//! 该文本——WPS 保存时无渲染快照即作者所见为空）。旧实现拼接
//! instrText 属过度收集（该件差分 123 条 OLD_BUG 同族）。
//!
//! 本文件钉定三条边界：
//! 1. 退化域指令文本不入正文（单 run 与跨 run 累积两形态）；
//! 2. **字面 `w:t` 文本含 `""` 引号逐字保留**（剥离按「域结果区」
//!    判定，绝不按内容/引号形态判定——issue 前设的反面钉死）；
//! 3. 指令面丢弃不计 `fields_dropped`（该计数器只计表现型域
//!    **缓存结果**剥离——指令文本从未可见，非损失面）。
//!
//! 构造面 = docx-rs 写侧 builder（`InstrText::Unsupported` 原始指令
//! 串——读面同源上游模型，与 docx_nested_tables.rs 同模式）。

use mdc_office_io::docx::{read_docx_from_bytes, Block};

/// songjiang 段1211 形态：字面 run + 退化域（单 instrText）+ 字面 run。
fn pack(paras: Vec<docx_rs::Paragraph>) -> Vec<u8> {
    let mut docx = docx_rs::Docx::new();
    for p in paras {
        docx = docx.add_paragraph(p);
    }
    let built = docx.build();
    let mut buf = std::io::Cursor::new(Vec::new());
    built.pack(&mut buf).expect("docx-rs pack");
    buf.into_inner()
}

/// 退化域三连 run（begin + 原始指令 + end——无 separate 无缓存结果，
/// fldChar 无 dirty 标记，songjiang WPS 12.1 发射形态）。
fn degenerate_field(instr: &str) -> Vec<docx_rs::Run> {
    vec![
        docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false),
        docx_rs::Run::new().add_instr_text(docx_rs::InstrText::Unsupported(instr.to_string())),
        docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false),
    ]
}

fn paragraph_with_field(
    literal_before: &str,
    field_runs: Vec<docx_rs::Run>,
    literal_after: &str,
) -> docx_rs::Paragraph {
    let mut p = docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(literal_before));
    for r in field_runs {
        p = p.add_run(r);
    }
    p.add_run(docx_rs::Run::new().add_text(literal_after))
}

fn read_paras(bytes: &[u8]) -> Vec<(String, usize)> {
    let doc = read_docx_from_bytes(bytes).expect("read_docx_from_bytes");
    doc.blocks
        .into_iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some((p.text(), p.fields_dropped)),
            _ => None,
        })
        .collect()
}

/// 主裁决面 1：WPS 退化域（`""` 前缀不可解析指令）单 run 形态——
/// 指令文本不入正文；协议名读回 = 渲染保真形（`施工框架协议`，
/// LO/pandoc/python-docx 三方共识输出同形）；指令面非损失面
/// （fields_dropped 恒 0）。
#[test]
fn wps_degenerate_field_instruction_not_in_body() {
    let bytes = pack(vec![paragraph_with_field(
        "《[]通信工程建设项目施工",
        degenerate_field("\"\"施工/服务"),
        "框架协议》",
    )]);
    let paras = read_paras(&bytes);
    assert_eq!(paras.len(), 1);
    assert_eq!(
        paras[0].0, "《[]通信工程建设项目施工框架协议》",
        "指令文本不入正文——与 LO 渲染/pandoc/python-docx 输出同形（#40 裁决 B）"
    );
    assert_eq!(
        paras[0].1, 0,
        "指令面丢弃不进 fields_dropped（非缓存结果剥离）"
    );
}

/// 主裁决面 2：指令跨 run 累积形态（songjiang 段975——`\"\"` +
/// `不高于` + `[]的价格` 三个 instrText run 分属三 run）——累积判定
/// 面同样不入正文；价格条款读回 = 渲染保真形。
#[test]
fn multi_run_instruction_accumulates_not_emitted() {
    let runs = vec![
        docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::Begin, false),
        docx_rs::Run::new().add_instr_text(docx_rs::InstrText::Unsupported("\"\"".to_string())),
        docx_rs::Run::new().add_instr_text(docx_rs::InstrText::Unsupported("不高于".to_string())),
        docx_rs::Run::new().add_instr_text(docx_rs::InstrText::Unsupported("[]的价格".to_string())),
        docx_rs::Run::new().add_field_char(docx_rs::FieldCharType::End, false),
    ];
    let bytes = pack(vec![paragraph_with_field(
        "4.9.4保修期届满后，承包人为具体项目业主提供技术支持和服务、",
        runs,
        "收取服务费。",
    )]);
    let paras = read_paras(&bytes);
    assert_eq!(paras.len(), 1);
    assert_eq!(
        paras[0].0, "4.9.4保修期届满后，承包人为具体项目业主提供技术支持和服务、收取服务费。",
        "跨 run 指令累积面同样不入正文（渲染保真——LO 渲染该段无「不高于」限定）"
    );
    assert_eq!(paras[0].1, 0);
}

/// 边界钉定：**字面 `w:t` 文本**含 `""` 引号对逐字保留——域剥离按
/// 「域结果区」（separate→end 之间）判定，绝不按引号/内容形态判定。
/// issue 前设「字面正文因 `""` 邻接被剥离」的反面钉死。
#[test]
fn literal_quoted_wt_text_preserved_verbatim() {
    let bytes = pack(vec![docx_rs::Paragraph::new().add_run(
        docx_rs::Run::new().add_text("对照：\"\"施工/服务为字面正文（w:t 面），逐字保留。"),
    )]);
    let paras = read_paras(&bytes);
    assert_eq!(paras.len(), 1);
    assert_eq!(
        paras[0].0, "对照：\"\"施工/服务为字面正文（w:t 面），逐字保留。",
        "引号形态的字面正文永不剥离——剥离域 = 表现型域缓存结果区，按位置非内容"
    );
    assert_eq!(paras[0].1, 0);
}

/// 真语料复核探针（CI 不可达语料——显式 `--ignored` 运行）：
/// `MDCORE_SONGJIANG_DOCX=<path> cargo test -p mdc-office-io --test
/// tr31_degenerate_field -- --ignored`。
///
/// 断言 = #40 验收测试 1 的等价形（渲染保真口径）：四差异块读回为
/// 渲染可见文本；`施工/服务`/`不高于` 全文零发射（两串在 document.xml
/// 中**仅存在于 instrText 面**，w:t 面零出现——分面统计定谳）。
#[test]
#[ignore = "真实语料不在仓（/tmp/abr 部署面）——按需显式运行"]
fn real_corpus_songjiang_rendered_truth_probe() {
    let path = std::env::var("MDCORE_SONGJIANG_DOCX")
        .expect("设 MDCORE_SONGJIANG_DOCX=songjiang docx 路径后显式 --ignored 运行");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let doc = read_docx_from_bytes(&bytes).expect("read songjiang");
    let mut name_hits = 0usize;
    let mut clause_hits = 0usize;
    for b in &doc.blocks {
        if let Block::Paragraph(p) = b {
            let t = p.text();
            if t.contains("施工框架协议") {
                name_hits += 1;
            }
            if t.contains("收取服务费") {
                clause_hits += 1;
            }
            assert!(!t.contains("施工/服务"), "指令面文本不得入正文：{t:?}");
            assert!(!t.contains("不高于"), "指令面文本不得入正文：{t:?}");
        }
    }
    assert!(name_hits >= 3, "三处协议名渲染保真形（段509/1211/1339）");
    assert!(clause_hits >= 1, "价格条款渲染保真形（段975）");
}
