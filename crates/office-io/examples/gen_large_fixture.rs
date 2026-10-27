//! R-1 大文件夹具生成器（#22 任务池 · test-artifacts/large-file）。
//!
//! 经引擎自有写面生成导入边界夹具（写面即产线——顺带在 50MB/100MB
//! 量级压测本 crate 写路径；零外部工具依赖）：
//!
//! - docx：`target_bytes` 量级的变体文本段落文档（每段带序号——
//!   反共享去重/反压缩，模拟真实标书文本密度）；
//! - xlsx：恒 1,000,000 格（写面上限 [`crate::write::MAX_CELLS`]——
//!   T3 探针 100 万格实测面）× 变体文本，流式表（constant_memory
//!   行序前向纪律）；总字节数取决于文本长度——`cells` 行数参数控制。
//!
//! 用法（仓库根）：
//! ```text
//! cargo run -p mdc-office-io --example gen_large_fixture -- docx out.docx 50000000
//! cargo run -p mdc-office-io --example gen_large_fixture -- xlsx out.xlsx 250 250
//! ```
//!
//! 产物不入库（.gitignore：test-artifacts/large-file/*.docx|xlsx|pdf）。

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (kind, out) = match (args.get(1).map(String::as_str), args.get(2)) {
        (Some("docx"), Some(p)) => ("docx", p.clone()),
        (Some("xlsx"), Some(p)) => ("xlsx", p.clone()),
        _ => {
            eprintln!("usage: gen_large_fixture <docx|xlsx> <out-path> [param]");
            eprintln!("  docx param = 目标字节量（默认 50_000_000 ≈ 50MB）");
            eprintln!("  xlsx param = 每表行数（默认 250；列恒 250 → 每表 62,500 格 × 16 表 = 1M 格上限）");
            std::process::exit(2);
        }
    };
    let t0 = Instant::now();
    let bytes = match kind {
        "docx" => {
            let target: u64 = args
                .get(3)
                .and_then(|s| s.parse().ok())
                .unwrap_or(50_000_000);
            gen_docx(target)
        }
        _ => {
            let rows: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(250);
            gen_xlsx(rows)
        }
    };
    let gen_ms = t0.elapsed().as_millis();
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("write {out}: {e}"));
    println!(
        "kind={kind} path={out} bytes={} gen_ms={gen_ms}",
        bytes.len()
    );
}

/// 变体段落文本（序号前缀反压缩/反去重 + 固定语料池拼接——模拟标书
/// 文本密度；UTF-8 CJK 混合）。
fn para_text(i: usize) -> String {
    let pool = [
        "本项目为智能文档平台建设，涵盖需求解析、协同编辑与格式保真三大子系统。",
        "投标人应具备同类项目实施经验，并提供近三年内不少于两个案例证明材料。",
        "系统须支持国产化环境部署，包括信创操作系统与主流数据库适配能力。",
        "服务响应时间要求：工作日 9:00-18:00 电话支持，2 小时内远程响应。",
        "验收标准以本技术要求书逐项核对为准，任何偏离项须在投标文件中明示。",
    ];
    let mut s = format!("第{i}段：");
    for k in 0..3 {
        s.push_str(pool[(i + k) % pool.len()]);
        s.push('。');
    }
    s
}

fn gen_docx(target_bytes: u64) -> Vec<u8> {
    use mdc_office_io::docx::{DocxWriter, RunSpec};
    let mut w = DocxWriter::new();
    w.heading(1, "大文件边界测试文档（R-1 夹具）")
        .expect("heading");
    // 估算（实测校准：每段 ≈ 426B zip 后——350B UTF-8 文本 + 标签
    // 开销，DEFLATE ≈ 40%；91.6k 段 → 39.1MB 实测 → 段数 = target/426）。
    let est_paras = (target_bytes as f64 / 426.0) as usize;
    for i in 0..est_paras {
        w.paragraph(&[RunSpec::text(para_text(i))]);
    }
    w.save_to_vec().expect("save")
}

fn gen_xlsx(rows: u32) -> Vec<u8> {
    use mdc_office_io::write::{CellValue, WorkbookWriter};
    const COLS: u32 = 250; // 250×250=62,500 格/表 × 16 表 = 1M 格（写面上限）
    let mut w = WorkbookWriter::new();
    // 确定性伪随机（xorshift64*——固定种子，夹具可重现）：抗压缩填充
    // 使 1M 格在 ~100MB 量级（真实语料重复度高、zip 后仅 ~9MB——
    // 无法达到边界档；随机基36文本压缩率 ~65%，1M×~155B ≈ 100MB）。
    let mut state: u64 = 0x5A17C0DE;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state.wrapping_mul(0x2545F4914F6CDD1D)
    };
    let mut text_of = |r: u32, c: u32| {
        let mut s = format!("R{r}C{c}");
        // ~126 个基36字符（126 × 0.66 ≈ 100MB 实测校准）（抗压缩主体）。
        let mut v = next();
        for i in 0..126 {
            if i % 11 == 10 {
                v = next();
            }
            s.push((b'a' + (v % 36) as u8) as char);
            v /= 36;
        }
        s
    };
    for s in 0..16 {
        let mut sheet = w
            .add_sheet_streaming(&format!("S{s}"))
            .expect("streaming sheet");
        for r in 0..rows {
            let mut row = Vec::with_capacity(COLS as usize);
            for c in 0..COLS {
                row.push(CellValue::Text(text_of(r, c)));
            }
            sheet.write_row(r, &row).expect("row");
        }
    }
    w.save_to_vec().expect("save")
}
