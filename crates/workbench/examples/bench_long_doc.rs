//! 阶段 1 基准①：长文档与证据侧栏（headless 面——宿主选型前置基线）。
//!
//! 用法（仓库根，release 构建）：
//!
//! ```text
//! cargo run --release -p mdc-workbench --example bench_long_doc -- ingest 10000
//! cargo run --release -p mdc-workbench --example bench_long_doc -- ingest 50000
//! cargo run --release -p mdc-workbench --example bench_long_doc -- ingest 100000
//! cargo run --release -p mdc-workbench --example bench_long_doc -- bindings 100000 1000
//! cargo run --release -p mdc-workbench --example bench_long_doc -- refresh 100000 1000
//! cargo run --release -p mdc-workbench --example bench_long_doc -- reverify 100000 1000
//! cargo run --release -p mdc-workbench --example bench_long_doc -- persistence 100000 1000
//! cargo run --release -p mdc-workbench --example bench_long_doc -- report 100000 1000
//! ```
//!
//! **测量面与宿主依赖面的边界（诚实登记）**：本基准覆盖 headless
//! 可测的引擎面——正文摄取、绑定登记（含 sidecar 持久化逐笔代价）、
//! 选择性失效扫描（refresh_body——"正文定位到来源及高亮延迟"的
//! 核心代理）、重验恢复、会话重开、报告生成。"输入到界面更新的
//! 延迟 / 侧栏更新是否导致正文重排" = 宿主渲染面，待阶段 1 选型
//! 后在宿主原型上补测；本基准为其提供引擎侧下界。
//!
//! 计量口径与 bench_limits 一致：耗时 = Instant 实测；峰值 RSS =
//! /proc/self/status VmHWM。每子命令单进程单场景；文档生成（DOCX/
//! XLSX 写盘）不计入测量段。

#[cfg(not(target_arch = "wasm32"))]
mod bench {
    use std::path::PathBuf;
    use std::time::Instant;

    use mdc_evidence::{Decimal, RelationKind};
    use mdc_office_io::docx::DocxWriter;
    use mdc_office_io::write::{CellValue, WorkbookWriter};
    use mdc_workbench::{BindingProposal, PathPolicy, Workbench};

    fn peak_rss_kib() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                return rest.trim().trim_end_matches("kB").trim().parse().ok();
            }
        }
        None
    }

    fn report(label: &str, start: Instant, detail: String) {
        let ms = start.elapsed().as_millis();
        let rss = peak_rss_kib();
        println!("[bench_long_doc] {label}: {detail}");
        println!(
            "[bench_long_doc] {label}: elapsed {ms} ms, peak RSS {}",
            rss.map(|k| format!("{} KiB", k))
                .unwrap_or_else(|| "n/a".into())
        );
    }

    fn bench_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("bench-long-doc-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 生成 n 段 DOCX 正文（不计时——测量段外）。
    /// 段文本带序号与变化词（指纹互异）；claim 锚段嵌入主张值
    /// （绑定校验要求：claimed_value 必须出现在锚段文本）。
    fn gen_body(
        dir: &std::path::Path,
        n: u32,
        changed: Option<u32>,
        claims: &[(u32, u32)],
    ) -> PathBuf {
        let name = if changed.is_some() {
            "body_edited.docx"
        } else {
            "body.docx"
        };
        let p = dir.join(name);
        let mut w = DocxWriter::new();
        w.heading(1, "基准正文（合成）").unwrap();
        for i in 0..n {
            let text = if let Some(c) = changed {
                if c == i {
                    format!("第{i}段【已被外部编辑改写】")
                } else {
                    format!("第{i}段：基准合成正文，内容互异以使指纹独立。")
                }
            } else if let Some((_, v)) = claims.iter().find(|(idx, _)| *idx == i + 1) {
                // 锚段：含主张值（值 = 来源行数量）。
                format!("第{i}段：基准合成正文，数量 {v} 件，内容互异。")
            } else {
                format!("第{i}段：基准合成正文，内容互异以使指纹独立。")
            };
            w.paragraph_text(&text);
        }
        w.save_to_path(&p.to_string_lossy()).unwrap();
        p
    }

    /// 生成 k 行 XLSX 来源（数量 B 列 / 单价 C 列——从第 2 行起）。
    fn gen_source(dir: &std::path::Path, k: u32) -> PathBuf {
        let p = dir.join("source.xlsx");
        let mut w = WorkbookWriter::new();
        let mut sh = w.add_sheet("报价表").unwrap();
        sh.write(1, 1, CellValue::Text("数量".into()), None)
            .unwrap();
        sh.write(1, 2, CellValue::Text("单价".into()), None)
            .unwrap();
        for i in 0..k {
            let r = i + 2;
            sh.write(r, 1, CellValue::Number(i as f64 + 1.0), None)
                .unwrap();
            sh.write(r, 2, CellValue::Number(336.8), None).unwrap();
        }
        w.save_to_path(&p.to_string_lossy()).unwrap();
        p
    }

    /// 装载会话 + 来源 + k 绑定（返回会话与 claim 段索引集）。
    /// 返回 (workbench, body 路径, claim 段索引, XLSX 路径)。
    fn setup(n: u32, k: u32) -> (Workbench, PathBuf, Vec<u32>, PathBuf) {
        let dir = bench_dir();
        // claim 锚段：均匀铺开（段 1 起避开标题）。
        let idxs: Vec<u32> = (0..k)
            .map(|i| 1 + (i as u64 * (n as u64 - 2) / k.max(1) as u64) as u32)
            .collect();
        let anchors: Vec<(u32, u32)> = idxs
            .iter()
            .cloned()
            .zip((1..=k).collect::<Vec<u32>>())
            .collect();
        let body = gen_body(&dir, n, None, &anchors);
        let xlsx = gen_source(&dir, k);
        let sidecar = dir.join("sidecar.json");
        let policy = PathPolicy::new(std::slice::from_ref(&dir)).unwrap();
        let mut wb =
            Workbench::start(policy, &sidecar, &body, "body-1", 1000, "bench:setup").unwrap();
        wb.ingest_source("src-1", &xlsx, 1100, "bench:setup")
            .unwrap();
        (wb, body, idxs, xlsx)
    }

    /// k 绑定（计时段内含逐笔 sidecar publish——诚实代价）。
    fn do_bindings(wb: &mut Workbench, idxs: &[u32]) {
        for (i, idx) in idxs.iter().enumerate() {
            let r = i + 2;
            wb.propose_binding(
                BindingProposal {
                    evidence_id: format!("e-{i}"),
                    claim: format!("第{i}条数量主张"),
                    claimed_value: Some(Decimal::parse(&(i + 1).to_string()).unwrap()),
                    paragraph_index: *idx,
                    source_doc_id: "src-1".into(),
                    sheet: "报价表".into(),
                    coord: format!("B{r}"),
                    relation: RelationKind::DirectQuote,
                    calc_ref: None,
                    generation_batch: "bench".into(),
                },
                1200 + i as u64,
                "bench:bind",
            )
            .unwrap();
        }
    }

    pub fn run() {
        let args: Vec<String> = std::env::args().collect();
        let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
        let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10_000);
        let k: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1000);
        println!("[bench_long_doc] scenario={cmd} paragraphs={n} claims={k}");

        match cmd {
            // 正文摄取（DOCX 解析 + 指纹 + sidecar 首次发布）。
            "ingest" => {
                let dir = bench_dir();
                let body = gen_body(&dir, n, None, &[]);
                let sidecar = dir.join("sidecar.json");
                let policy = PathPolicy::new(std::slice::from_ref(&dir)).unwrap();
                let t = Instant::now();
                let _wb =
                    Workbench::start(policy, &sidecar, &body, "body-1", 1000, "bench").unwrap();
                report("ingest", t, format!("DOCX {n} 段摄取 + sidecar 发布"));
            }
            // k 绑定登记（证据侧栏规模化——含逐笔持久化）。
            "bindings" => {
                let (mut wb, _, idxs, _) = setup(n, k);
                let t = Instant::now();
                do_bindings(&mut wb, &idxs);
                let per = t.elapsed().as_micros() / k.max(1) as u128;
                report(
                    "bindings",
                    t,
                    format!("{k} 绑定（每笔含 sidecar publish），均 {per} µs/笔"),
                );
            }
            // 选择性失效（外部改写 1 段 → refresh_body 全指纹对照）。
            "refresh" => {
                let (mut wb, body, idxs, _) = setup(n, k);
                do_bindings(&mut wb, &idxs);
                // 外部改写：claim[0] 锚段被编辑（段文本变化→指纹变化）。
                let changed = idxs[0];
                let dir = body.parent().unwrap().to_path_buf();
                drop(wb); // 会话关闭（逐笔持久化——状态全在盘）
                let body2 = gen_body(&dir, n, Some(changed), &[]);
                std::fs::copy(&body2, &body).unwrap();
                let _ = std::fs::remove_file(&body2);
                // 重开会话（body 文件已变——open 不校验正文，校验在
                // refresh/reverify 面；此路径 = 会话重开 + 失效扫描）。
                let sidecar = dir.join("sidecar.json");
                let xlsx = dir.join("source.xlsx");
                let policy = PathPolicy::new(std::slice::from_ref(&dir)).unwrap();
                let t = Instant::now();
                let mut wb2 =
                    Workbench::open(policy, &sidecar, &body, &[("src-1".into(), xlsx)]).unwrap();
                let open_ms = t.elapsed().as_millis();
                println!(
                    "[bench_long_doc] refresh: 会话重开（sidecar 载入 + 正文摄取）{open_ms} ms"
                );
                let t2 = Instant::now();
                let stale = wb2.refresh_body(2000, "bench:refresh").unwrap();
                report(
                    "refresh_body",
                    t2,
                    format!("{n} 段全指纹对照 → 失效 {stale} 条（选择性：恰为被改段锚定的主张）"),
                );
                assert_eq!(stale, 1, "选择性失效：恰 1 条（被改段的 claim）");
            }
            // 重验恢复通道（k 条全部重验——恢复面规模化上界）。
            "reverify" => {
                let (mut wb, _, idxs, _) = setup(n, k);
                do_bindings(&mut wb, &idxs);
                let t = Instant::now();
                for (i, idx) in idxs.iter().enumerate() {
                    let r = i + 2;
                    wb.reverify_source(
                        &format!("e-{i}"),
                        "src-1",
                        "报价表",
                        &format!("B{r}"),
                        3000 + i as u64,
                        "bench:reverify",
                    )
                    .unwrap();
                    let _ = idx;
                }
                let per = t.elapsed().as_micros() / k.max(1) as u128;
                report(
                    "reverify_source",
                    t,
                    format!(
                        "{k} 条来源重验（每笔含 XLSX 重摄取 + sidecar publish），均 {per} µs/条"
                    ),
                );
            }
            // 会话持久化（sidecar 文件尺寸 + 重开全链）。
            "persistence" => {
                let (wb, body, _, _) = setup(n, k);
                let _ = body;
                drop(wb);
                let dir = bench_dir(); // setup 用独立目录
                let _ = &dir;
                // 直接测上一次 setup 的产物（重新走一遍保证独立进程口径）
                let (mut wb2, body2, idxs, _) = setup(n, k);
                do_bindings(&mut wb2, &idxs);
                drop(wb2);
                let sidecar = body2.parent().unwrap().join("sidecar.json");
                let size = std::fs::metadata(&sidecar).unwrap().len();
                let xlsx = body2.parent().unwrap().join("source.xlsx");
                let body = body2;
                let policy_root = body.parent().unwrap().to_path_buf();
                let policy = PathPolicy::new(std::slice::from_ref(&policy_root)).unwrap();
                let t = Instant::now();
                let _wb3 =
                    Workbench::open(policy, &sidecar, &body, &[("src-1".into(), xlsx)]).unwrap();
                report(
                    "persistence",
                    t,
                    format!("sidecar {size} B 重开（载入 + 正文 {n} 段摄取 + 来源注册）"),
                );
            }
            // 报告生成（审核报告——侧栏内容代理）。
            "report" => {
                let (mut wb, _, idxs, _) = setup(n, k);
                do_bindings(&mut wb, &idxs);
                let t = Instant::now();
                let r = wb.report();
                let len = r.len();
                report("report", t, format!("审核报告生成（{k} 主张）→ {len} 字节"));
            }
            _ => {
                eprintln!("用法：bench_long_doc <ingest|bindings|refresh|reverify|persistence|report> <paragraphs> <claims>");
                std::process::exit(2);
            }
        }
    }
}

// wasm32 check --all-targets 面：示例不可执行但须可编译——占位 main
//（真实现见下方 not(wasm32) 块；本占位使全目标检查口径完整）。
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    bench::run();
}
