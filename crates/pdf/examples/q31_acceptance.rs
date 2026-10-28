//! Q3-1 R1 验收例程（#29）：font + sample + out 三参 → generate_text_pdf。
use mdc_pdf::generate::{generate_text_pdf, PageLayout};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let font_path = args
        .get(1)
        .expect("usage: q31_acceptance <font.ttf> <sample.txt> <out.pdf>");
    let sample_path = args.get(2).expect("sample");
    let out_path = args.get(3).expect("out");
    let font = std::fs::read(font_path).expect("font readable");
    let text = std::fs::read_to_string(sample_path).expect("sample readable");
    let out = generate_text_pdf(&font, &text, &PageLayout::default()).expect("generate");
    std::fs::write(out_path, &out.bytes).expect("write");
    eprintln!(
        "generated: {} pages, {} skipped, {} bytes",
        out.pages,
        out.skipped_chars,
        out.bytes.len()
    );
}
