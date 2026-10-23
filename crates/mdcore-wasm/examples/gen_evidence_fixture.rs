//! 阶段 5 §八.1 e2e 夹具生成器（证据工作台 UI）：八步闭环经典坐标
//! 样例 → `web/tests/fixtures/`（web/tests/evidence_panel.py 消费）。
//!
//! 产物三件：
//! - `evidence_sample.xlsx`：Sheet1 — B7=12.5（数量，number）、
//!   C7=336.8（单价，number）、D2="报价"（text）。B7/C7 是
//!   `qty * price = 4210.000`（scale 3 half_up）的输入格。
//! - `evidence_body.docx`：5 段落——P0 标题、P1 引言、
//!   P2 "单价 12.5 元，数量 336.8。"（claimed_value "12.5" 对照面）、
//!   P3 "小计待计算，登记值 4210.000 元。"（calculated 主张对照面——
//!   claimed_value 必须真实出现在段落文本中，workbench 提议门）、
//!   P4 收尾。
//! - `evidence_body_v2.docx`：P2 改文（"337.0"）——`refresh_body`
//!   失效注入用（P2 段落指纹变 → claim 锚断 → linkage Stale；其余
//!   段落不变 → e-calc 不受累）。
//!
//! 产线 = office-io 写面（xlsx `WorkbookWriter` / docx `DocxWriter`
//! 直出——写→读全环复证）。
//!
//! 用法（仓库根）：`cargo run -p mdcore-wasm --example gen_evidence_fixture`。
//! 生成后自检：xlsx 读面 → 使用区断言；docx 读面 → 段落数 + 关键文本
//! 断言（夹具不过读面门则 panic，不带病出厂）。

use mdc_office_io::docx::{read_docx_from_bytes, Block, DocxWriter};
use mdc_office_io::io::read_book_from_bytes;
use mdc_office_io::write::{CellValue, WorkbookWriter};

fn build_sample_xlsx() -> Vec<u8> {
    let mut w = WorkbookWriter::new();
    let mut s = w.add_sheet("Sheet1").expect("add sheet Sheet1");
    // 八步闭环经典坐标：D2 报价标签、B7 数量 12.5、C7 单价 336.8
    s.write(1, 3, CellValue::Text("报价".into()), None)
        .expect("write D2");
    s.write(6, 1, CellValue::Number(12.5), None)
        .expect("write B7");
    s.write(6, 2, CellValue::Number(336.8), None)
        .expect("write C7");
    w.save_to_vec().expect("save xlsx")
}

/// 5 段落正文（P2 可参数化——v2 夹具改文用）。
fn build_body(p2: &str) -> Vec<u8> {
    let mut d = DocxWriter::new();
    d.paragraph_text("审计报告");
    d.paragraph_text("本报告基于受证来源。");
    d.paragraph_text(p2);
    d.paragraph_text("小计待计算，登记值 4210.000 元。");
    d.paragraph_text("完");
    d.save_to_vec().expect("save docx")
}

fn paragraph_texts(bytes: &[u8]) -> Vec<String> {
    let doc = read_docx_from_bytes(bytes).expect("docx self-check read");
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.text()),
            _ => None,
        })
        .collect()
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "web/tests/fixtures".to_string());
    let xlsx = build_sample_xlsx();
    let body = build_body("单价 12.5 元，数量 336.8。");
    let body_v2 = build_body("单价 12.5 元，数量 337.0。");

    // ---- 自检 1：xlsx 读面 → 使用区（锚 B2 起 6×3；绝对域 7×4——
    //      非原点锚定表，rows/cols=锚定维度、ext_*=绝对域，见
    //      xlsx_grid sheets_core 契约）----
    let book = read_book_from_bytes(&xlsx)
        .unwrap_or_else(|e| panic!("xlsx fixture self-check read failed: {e}"));
    let sheets = mdcore_wasm::xlsx_grid::sheets_core(&book);
    assert_eq!(
        sheets, r#"[{"name":"Sheet1","rows":6,"cols":3,"ext_rows":7,"ext_cols":4}]"#,
        "used-range mismatch"
    );

    // ---- 自检 2：docx 读面 → 恰 5 段落 + 关键文本在场 ----
    for (label, bytes, p2_text) in [
        ("body", &body, "单价 12.5 元，数量 336.8。"),
        ("body_v2", &body_v2, "单价 12.5 元，数量 337.0。"),
    ] {
        let texts = paragraph_texts(bytes);
        assert_eq!(texts.len(), 5, "{label}: paragraph count");
        assert!(texts[2].contains(p2_text), "{label}: P2 text mismatch");
        assert!(texts[2].contains("12.5"), "{label}: P2 must contain 12.5");
        assert!(
            texts[3].contains("4210.000"),
            "{label}: P3 must contain 4210.000 (claimed_value gate)"
        );
        assert_eq!(texts[4], "完", "{label}: P4 closing");
    }
    // 两版正文 P2 指纹必须不同（refresh_body 失效注入的前提）
    assert_ne!(body, body_v2, "v2 body must differ from v1");

    let root = std::path::Path::new(&dir);
    std::fs::create_dir_all(root)
        .unwrap_or_else(|e| panic!("create fixture dir {}: {e}", root.display()));
    for (name, bytes) in [
        ("evidence_sample.xlsx", &xlsx),
        ("evidence_body.docx", &body),
        ("evidence_body_v2.docx", &body_v2),
    ] {
        let path = root.join(name);
        std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        println!(
            "fixture written: {} ({} bytes)",
            path.display(),
            bytes.len()
        );
    }
    println!(
        "self-check passed: Sheet1 anchored 6x3 (ext 7x4); 5 paragraphs; P2 12.5 / P3 4210.000 in place"
    );
}
