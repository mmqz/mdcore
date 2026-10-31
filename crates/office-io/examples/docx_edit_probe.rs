//! docx_edit 验证探针（#79 T1——真实文档验证驱动器）。
//!
//! 用法：
//! ```text
//! cargo run -p mdc-office-io --example docx_edit_probe -- list <in.docx>
//! cargo run -p mdc-office-io --example docx_edit_probe -- edit <in.docx> <idx> <新文本> <out.docx>
//! cargo run -p mdc-office-io --example docx_edit_probe -- roundtrip10 <in.docx> <idx> <新文本> <out.docx>
//! ```
//!
//! `list` 输出段落索引面（旗标 + 文本前 60 chars）；`edit` 单次编辑落盘；
//! `roundtrip10` 连续 10 轮「开→改→存→重开」并打印每轮字节数（膨胀
//! 检查——验收线 <1%）。真实文档验证流程 = list 定位目标段 → edit →
//! LibreOffice `--headless --convert-to pdf` 开档验证 + XML diff（见
//! docs/docx-fidelity-matrix.md 复现节）。

use mdc_office_io::docx_edit::{DocxEditor, RevisionPolicy};

fn policy_from_env() -> RevisionPolicy {
    match std::env::var("MDC_EDIT_POLICY").as_deref() {
        Ok("edit") => RevisionPolicy::Edit,
        _ => RevisionPolicy::Refuse,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: docx_edit_probe list <in.docx> | edit <in> <idx> <text> <out> | roundtrip10 <in> <idx> <text> <out>");
        std::process::exit(2);
    }
    let mode = args[1].as_str();
    let input = args[2].clone();
    match mode {
        "list" => list(&input),
        "edit" => {
            let (idx, text, out) = parse_edit_args(&args, 3, 4, 5);
            let bytes =
                std::fs::read(&input).unwrap_or_else(|e| die(&format!("read {input}: {e}")));
            let policy = policy_from_env();
            let mut ed = DocxEditor::open_with_policy(&bytes, policy)
                .unwrap_or_else(|e| die(&format!("open: {e}")));
            let before = ed.paragraph_text(idx).to_string();
            ed.set_paragraph_text(idx, &text)
                .unwrap_or_else(|e| die(&format!("set: {e}")));
            let saved = ed
                .save_to_vec()
                .unwrap_or_else(|e| die(&format!("save: {e}")));
            std::fs::write(&out, &saved).unwrap_or_else(|e| die(&format!("write {out}: {e}")));
            println!(
                "edit ok: paragraph {idx} ({before:?} -> {text:?}); bytes {} -> {}",
                bytes.len(),
                saved.len()
            );
        }
        "roundtrip10" => {
            let (idx, text, out) = parse_edit_args(&args, 3, 4, 5);
            let bytes =
                std::fs::read(&input).unwrap_or_else(|e| die(&format!("read {input}: {e}")));
            let mut cur = bytes.clone();
            let mut last = 0usize;
            let policy = policy_from_env();
            for r in 1..=10 {
                let mut ed = DocxEditor::open_with_policy(&cur, policy)
                    .unwrap_or_else(|e| die(&format!("open r{r}: {e}")));
                ed.set_paragraph_text(idx, &text)
                    .unwrap_or_else(|e| die(&format!("set r{r}: {e}")));
                let saved = ed
                    .save_to_vec()
                    .unwrap_or_else(|e| die(&format!("save r{r}: {e}")));
                println!("round {r}: {} bytes", saved.len());
                last = saved.len();
                cur = saved;
            }
            std::fs::write(&out, &cur).unwrap_or_else(|e| die(&format!("write {out}: {e}")));
            let growth = (last as f64) / (bytes.len() as f64) - 1.0;
            println!(
                "roundtrip10: original {} -> final {} bytes ({:+.3}% 相对原件；验收线 = 同态稳定 <1%)",
                bytes.len(),
                last,
                growth * 100.0
            );
        }
        _ => die("unknown mode"),
    }
}

fn parse_edit_args(args: &[String], i: usize, t: usize, o: usize) -> (usize, String, String) {
    let idx: usize = args
        .get(i)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| die("paragraph index expected"));
    let text = args.get(t).cloned().unwrap_or_else(|| die("text expected"));
    let out = args
        .get(o)
        .cloned()
        .unwrap_or_else(|| die("out path expected"));
    (idx, text, out)
}

fn list(input: &str) {
    let bytes = std::fs::read(input).unwrap_or_else(|e| die(&format!("read {input}: {e}")));
    let ed = DocxEditor::open(&bytes).unwrap_or_else(|e| die(&format!("open: {e}")));
    println!(
        "file {input}: {} bytes, {} paragraphs (policy Refuse; fields/revision-marked 段列旗标 F/R)",
        bytes.len(),
        ed.paragraphs().len()
    );
    for (i, p) in ed.paragraphs().iter().enumerate() {
        let flags = format!(
            "{}{}{}{}{}",
            if p.field_code { "F" } else { "-" },
            if p.revision { "R" } else { "-" },
            if p.fallback_copy { "B" } else { "-" },
            if p.in_table { "T" } else { "-" },
            if p.in_textbox { "X" } else { "-" },
        );
        let text = ed.paragraph_text(i);
        let head: String = text.chars().take(60).collect();
        println!(
            "#{i:4} [{flags}] span={:?} len={} {head:?}",
            p.span,
            text.chars().count()
        );
    }
}

fn die(msg: &str) -> ! {
    eprintln!("docx_edit_probe: {msg}");
    std::process::exit(1);
}
