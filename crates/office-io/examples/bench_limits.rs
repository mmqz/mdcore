//! r4-t2 资源上限可复现基准（合法大载荷——**不设性能门禁阈值，只记
//! 可复现基准**；数字落档 = 分析仓 SECURITY_POLICY v1.2 §3 + scratch
//! r4t2 基准记录节）。
//!
//! 用法（仓库根，release 构建）：
//!
//! ```text
//! cargo run --release -p mdc-office-io --example bench_limits -- gen500k
//! cargo run --release -p mdc-office-io --example bench_limits -- read500k
//! cargo run --release -p mdc-office-io --example bench_limits -- gen4m
//! cargo run --release -p mdc-office-io --example bench_limits -- read4m
//! cargo run --release -p mdc-office-io --example bench_limits -- write1m
//! cargo run --release -p mdc-office-io --example bench_limits -- write1m_stream
//! cargo run --release -p mdc-office-io --example bench_limits -- docx100k
//! cargo run --release -p mdc-office-io --example bench_limits -- docx1m
//! cargo run --release -p mdc-office-io --example bench_limits -- slice500k
//! cargo run --release -p mdc-office-io --example bench_limits -- edit500k
//! ```
//!
//! 计量口径：每子命令单进程跑单场景；**耗时 = Instant 实测**，
//! **峰值 RSS = /proc/self/status `VmHWM`**（内核高水位计数器——与
//! GNU `/usr/bin/time -v` 的 Maximum resident set size 同源
//! ru_maxrss/VmHWM；本沙箱无 /usr/bin/time 二进制，登记口径替代）。
//! 读基准文件生成（gen*）单独跑——生成不计入读基准进程。
//!
//! 读文件生成**直用引擎 API**（rust_xlsxwriter constant_memory 流式
//! ——低 RSS 生成），绕过包装层 MAX_CELLS：读面限值 = 4M 格，生成
//! 4M 格档恰在读限上界（包装层 1M 是写面独立限——写读两面分立，
//! 写面基准即 write1m*）。

#[cfg(not(target_arch = "wasm32"))]
mod bench {
    use std::time::Instant;

    /// 峰值 RSS（kB——/proc/self/status VmHWM；不可读平台返回 None）。
    fn peak_rss_kib() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                return rest.trim().trim_end_matches("kB").trim().parse().ok();
            }
        }
        None
    }

    /// 场景收尾打印（统一口径：elapsed + VmHWM）。
    fn report(label: &str, start: Instant, detail: String) {
        let ms = start.elapsed().as_millis();
        let rss = peak_rss_kib();
        println!("[bench_limits] {label}: {detail}");
        println!(
            "[bench_limits] {label}: elapsed {ms} ms, peak RSS {}",
            rss.map(|k| format!("{k} kB (= {:.1} MiB)", k as f64 / 1024.0))
                .unwrap_or_else(|| "n/a (no /proc)".into())
        );
    }

    fn bench_path(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    /// 生成 rows×cols 全数值网格 xlsx（引擎 constant_memory 流式——
    /// 低 RSS 生成；逐格 r*1000+c 变值，避免全零的非常规压缩面）。
    fn gen_grid(path: &str, rows: u32, cols: u16) {
        let start = Instant::now();
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet_with_constant_memory();
        ws.set_name("Bench").expect("set name");
        for r in 0..rows {
            for c in 0..cols {
                ws.write_number(r, c, f64::from(r) * 1000.0 + f64::from(c))
                    .expect("write cell");
            }
        }
        wb.save(path).expect("save generated file");
        let size = std::fs::metadata(path).expect("stat").len();
        report(
            "gen",
            start,
            format!("{rows}x{cols} grid -> {path} ({size} bytes)"),
        );
    }

    pub fn run() {
        let mode = std::env::args().nth(1).unwrap_or_else(|| {
            eprintln!(
                "usage: bench_limits <gen500k|read500k|gen4m|read4m|write1m|write1m_stream|docx100k|docx1m|slice500k|edit500k>"
            );
            std::process::exit(2);
        });
        match mode.as_str() {
            "gen500k" => gen_grid(&bench_path("bench_r4t2_read500k.xlsx"), 500, 1000),
            "gen4m" => gen_grid(&bench_path("bench_r4t2_read4m.xlsx"), 2000, 2000),
            "read500k" => read_bench(&bench_path("bench_r4t2_read500k.xlsx"), 500_000),
            "read4m" => read_bench(&bench_path("bench_r4t2_read4m.xlsx"), 4_000_000),
            "write1m" => write_bench(false),
            "write1m_stream" => write_bench(true),
            "docx100k" => docx_bench(100_000),
            "docx1m" => docx_bench(1_000_000),
            "slice500k" => slice_bench(),
            "edit500k" => edit_bench(),
            other => {
                eprintln!("unknown mode: {other}");
                std::process::exit(2);
            }
        }
    }

    /// 读基准：read_book（path 入口——门 0/1/2+3 全门 + GridScan 面积
    /// 检查 + calamine 提取 + typed 网格）全链耗时与峰值。
    fn read_bench(path: &str, expect_cells: u64) {
        if !std::path::Path::new(path).exists() {
            eprintln!("bench file missing: {path} — run gen first (see module docs)");
            std::process::exit(2);
        }
        let start = Instant::now();
        let book = mdc_office_io::io::read_book(path)
            .unwrap_or_else(|e| panic!("read failed (限内合法档): {e:?}"));
        let cells: u64 = book
            .sheets
            .iter()
            .map(|s| s.values.width() as u64 * s.values.height() as u64)
            .sum();
        report(
            "read",
            start,
            format!(
                "{path}: {} sheets, grid area {cells} (expect {expect_cells}), non-empty {}",
                book.sheets.len(),
                book.non_empty_cells()
            ),
        );
        assert_eq!(cells, expect_cells, "网格面积 = 生成面");
    }

    /// 写基准（包装层 API）：1M 格恰 = MAX_CELLS（预算增量制允许恰达限）
    /// ——非流式（全内存模型 + save_to_vec）与流式（tempfile +
    /// save_to_path）双模式。
    fn write_bench(streaming: bool) {
        use mdc_office_io::write::{CellValue, WorkbookWriter, MAX_CELLS};
        let start = Instant::now();
        let mut w = WorkbookWriter::new();
        {
            let mut s = if streaming {
                w.add_sheet_streaming("Bench").expect("add streaming sheet")
            } else {
                w.add_sheet("Bench").expect("add sheet")
            };
            let row: Vec<CellValue> = (0..1000).map(|c| CellValue::Number(c as f64)).collect();
            for r in 0..1000u32 {
                if streaming {
                    s.write_row(r, &row).expect("write row");
                } else {
                    for (c, v) in row.iter().enumerate() {
                        s.write(r, c as u16, v.clone(), None).expect("write cell");
                    }
                }
            }
        }
        let (cells, out_bytes) = if streaming {
            let p = bench_path("bench_r4t2_write1m_stream.xlsx");
            w.save_to_path(&p).expect("save");
            (MAX_CELLS, std::fs::metadata(&p).expect("stat").len())
        } else {
            let bytes = w.save_to_vec().expect("save");
            (MAX_CELLS, bytes.len() as u64)
        };
        report(
            if streaming {
                "write1m_stream"
            } else {
                "write1m"
            },
            start,
            format!("{cells} cells (in-memory Vec / tempfile), output {out_bytes} bytes"),
        );
    }

    /// 阶段 1 基准②：可视区切片读取（500k 格读入 + 万次 50行×20列
    /// 窗口提取——typed 网格 get_abs；渲染面可视区滚动的引擎侧下界）。
    fn slice_bench() {
        let path = bench_path("bench_r4t2_read500k.xlsx");
        if !std::path::Path::new(&path).exists() {
            eprintln!("bench file missing: {path} — run gen500k first");
            std::process::exit(2);
        }
        let t0 = Instant::now();
        let book =
            mdc_office_io::io::read_book(&path).unwrap_or_else(|e| panic!("read failed: {e:?}"));
        let read_ms = t0.elapsed().as_millis();
        let sheet = &book.sheets[0];
        let (h, w) = (sheet.typed.height(), sheet.typed.width());
        // 确定性伪随机窗口位置（LCG——可复现）。
        let mut seed: u64 = 0x2545F4914F6CDD1D;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seed >> 33
        };
        const WINDOWS: usize = 10_000;
        const WIN_H: u32 = 50;
        const WIN_W: u32 = 20;
        let t = Instant::now();
        let mut sum = 0u64;
        for _ in 0..WINDOWS {
            let r0 = (next() as u32) % (h as u32 - WIN_H);
            let c0 = (next() as u32) % (w as u32 - WIN_W);
            for r in r0..r0 + WIN_H {
                for c in c0..c0 + WIN_W {
                    if sheet.typed.get_abs(r, c).is_some() {
                        sum += 1;
                    }
                }
            }
        }
        let per_win_us = t.elapsed().as_micros() / WINDOWS as u128;
        println!(
            "[bench_limits] slice500k: read {read_ms} ms (grid {h}x{w}); 10000 windows (50x20) → {per_win_us} µs/窗（非空命中 {sum}）"
        );
        println!(
            "[bench_limits] slice500k: elapsed {} ms, peak RSS {}",
            t.elapsed().as_millis(),
            peak_rss_kib()
                .map(|k| format!("{} KiB", k))
                .unwrap_or_else(|| "n/a".into())
        );
    }

    /// 阶段 1 基准②：单格编辑提交（C3 读—改—重建架构的提交下界：
    /// 全量读 + kind 保持逐格重放 + 全量写——单笔编辑不可回避的代价）。
    fn edit_bench() {
        let path = bench_path("bench_r4t2_read500k.xlsx");
        if !std::path::Path::new(&path).exists() {
            eprintln!("bench file missing: {path} — run gen500k first");
            std::process::exit(2);
        }
        let t = Instant::now();
        let book =
            mdc_office_io::io::read_book(&path).unwrap_or_else(|e| panic!("read failed: {e:?}"));
        let read_ms = t.elapsed().as_millis();
        let t2 = Instant::now();
        let mut w = mdc_office_io::write::WorkbookWriter::new();
        let mut replayed = 0u64;
        for sheet in &book.sheets {
            let mut sw = w.add_sheet(&sheet.name).expect("add sheet");
            let (start, end) = match (sheet.typed.start(), sheet.typed.end()) {
                (Some(s), Some(e)) => (s, e),
                _ => continue,
            };
            for r in start.0..=end.0 {
                for c in start.1..=end.1 {
                    if let Some(cell) = sheet.typed.get_abs(r, c) {
                        if let (Some((v, style)), _) = (cell.to_write_parts(), ()) {
                            sw.write(r, c as u16, v, style.as_ref())
                                .expect("replay write");
                            replayed += 1;
                        }
                    }
                }
            }
        }
        let rebuild_ms = t2.elapsed().as_millis();
        let out = bench_path("bench_p1_edit500k_rebuilt.xlsx");
        let t3 = Instant::now();
        w.save_to_path(&out).expect("save");
        let save_ms = t3.elapsed().as_millis();
        println!(
            "[bench_limits] edit500k: C3 单笔编辑提交全链 = read {read_ms} ms + replay {rebuild_ms} ms ({replayed} cells) + save {save_ms} ms"
        );
        println!(
            "[bench_limits] edit500k: elapsed {} ms, peak RSS {}",
            t.elapsed().as_millis(),
            peak_rss_kib()
                .map(|k| format!("{} KiB", k))
                .unwrap_or_else(|| "n/a".into())
        );
    }

    /// DOCX 写基准（包装层 API）：N 段（每段 1 run）→ save_to_vec。
    /// 100k = 基线档；1m = MAX_PARAGRAPHS 恰达限档。
    fn docx_bench(n: u64) {
        use mdc_office_io::docx::{DocxWriter, MAX_PARAGRAPHS};
        assert!(n <= MAX_PARAGRAPHS, "基准档须在读限内");
        let start = Instant::now();
        let mut w = DocxWriter::new();
        for i in 0..n {
            w.paragraph_text(&format!("para {i}"));
        }
        let bytes = w.save_to_vec().expect("save");
        report(
            "docx",
            start,
            format!("{n} paragraphs x 1 run, output {} bytes", bytes.len()),
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    bench::run();
}

/// wasm32：基准面结构性缺席（引擎 constant_memory 流式 + fs 计量均
/// native-only）；空壳保持 --all-targets 编译面通过（t21 收严门禁）。
#[cfg(target_arch = "wasm32")]
fn main() {}
