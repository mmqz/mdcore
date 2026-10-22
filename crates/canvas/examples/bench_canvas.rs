//! 阶段 1 基准④：Canvas 画布（headless 面——渲染器选型前置基线）。
//!
//! 用法（仓库根，release 构建）：
//!
//! ```text
//! cargo run --release -p mdc-canvas --example bench_canvas -- load 1000
//! cargo run --release -p mdc-canvas --example bench_canvas -- load 10000
//! cargo run --release -p mdc-canvas --example bench_canvas -- move 10000 1000
//! cargo run --release -p mdc-canvas --example bench_canvas -- find 10000 1000
//! cargo run --release -p mdc-canvas --example bench_canvas -- readmodel 10000
//! cargo run --release -p mdc-canvas --example bench_canvas -- fingerprint 10000
//! cargo run --release -p mdc-canvas --example bench_canvas -- sync 10000
//! cargo run ---release -p mdc-canvas --example bench_canvas -- delete 10000 200
//! ```
//!
//! **测量面与渲染面边界（诚实登记）**：本基准覆盖模型面——装载
//! （AddNode×n）、拖拽提交（MoveNode——拖动预览的提交粒度）、命中
//! 查找（find_object——模型级）、读模型快照（objects()——渲染器
//! 每帧消费面）、全画布指纹（证据重验面）、协同同步（差量/应用）。
//! "帧表现/拖动跟手性" = 渲染器面，待宿主原型补测；本基准为其提供
//! 模型侧下界。节点规模：1000 = 基准数据集，10000 = 压力数据集
//! （分开记录，不预诺帧率）。
//!
//! 计量口径与 bench_limits 一致：Instant 实测 + VmHWM 峰值；采样
//! 场景报 p50/p95（万次采样内排序取分位）。

#[cfg(not(target_arch = "wasm32"))]
mod bench {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use mdc_canvas::{Actor, CanvasCommand, CanvasSession, SessionOptions};

    fn peak_rss_kib() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                return rest.trim().trim_end_matches("kB").trim().parse().ok();
            }
        }
        None
    }

    fn rss_str() -> String {
        peak_rss_kib()
            .map(|k| format!("{} KiB", k))
            .unwrap_or_else(|| "n/a".into())
    }

    fn opts(id: u64) -> SessionOptions {
        SessionOptions {
            client_id: id,
            ..Default::default()
        }
    }

    /// 确定性伪随机（LCG——可复现）。
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn f(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (self.next() as f64 / u64::MAX as f64) * (hi - lo)
        }
    }

    /// 装载 n 节点 + n/10 边（网格布局——边连接近邻）。
    fn load(c: &mut CanvasSession, n: u32) {
        let cols = (n as f64).sqrt().ceil() as u32;
        for i in 0..n {
            let (x, y) = ((i % cols) as f64 * 120.0, (i / cols) as f64 * 90.0);
            let mut props = BTreeMap::new();
            props.insert("label".to_string(), format!("节点 {i}"));
            c.execute(
                Actor::Human,
                CanvasCommand::AddNode {
                    id: Some(format!("n{i}")),
                    x,
                    y,
                    w: 80.0,
                    h: 40.0,
                    props,
                },
            )
            .unwrap();
        }
        for i in 0..(n / 10) {
            let a = i * 10;
            let b = (a + 1).min(n - 1);
            if a != b {
                c.execute(
                    Actor::Ai,
                    CanvasCommand::AddEdge {
                        id: Some(format!("e{i}")),
                        from: format!("n{a}"),
                        to: format!("n{b}"),
                        props: BTreeMap::new(),
                    },
                )
                .unwrap();
            }
        }
    }

    fn session_with(n: u32) -> CanvasSession {
        let mut c = CanvasSession::open(opts(7_200_101)).unwrap();
        load(&mut c, n);
        c
    }

    /// 分位数（samples 已采集——排序取 p50/p95/p99）。
    fn percentiles(mut v: Vec<u128>) -> (u128, u128, u128) {
        v.sort_unstable();
        let at = |p: f64| -> u128 {
            if v.is_empty() {
                return 0;
            }
            let i = ((v.len() as f64 - 1.0) * p) as usize;
            v[i.min(v.len() - 1)]
        };
        (at(0.50), at(0.95), at(0.99))
    }

    pub fn run() {
        let args: Vec<String> = std::env::args().collect();
        let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
        let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1_000);
        let m: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1_000);
        println!("[bench_canvas] scenario={cmd} nodes={n} samples={m}");

        match cmd {
            // 装载（批量创建——含逐命令事务/撤销栈）。
            "load" => {
                let mut c = CanvasSession::open(opts(7_200_101)).unwrap();
                let t = Instant::now();
                load(&mut c, n);
                let per = t.elapsed().as_micros() / (n + n / 10).max(1) as u128;
                println!(
                    "[bench_canvas] load: {n} 节点 + {} 边装载（每命令一事务一撤销栈项），均 {per} µs/op，undo 深度 {}",
                    n / 10,
                    c.undo_depth(Actor::Human)
                );
                println!(
                    "[bench_canvas] load: elapsed {} ms, peak RSS {}",
                    t.elapsed().as_millis(),
                    rss_str()
                );
            }
            // 拖拽提交（MoveNode——键级 LWW；高频拖拽的提交粒度）。
            "move" => {
                let mut c = session_with(n);
                let mut rng = Lcg(0x2545F4914F6CDD1D);
                let mut samples = Vec::with_capacity(m as usize);
                for _ in 0..m {
                    let id = format!("n{}", rng.next() as u32 % n);
                    let (x, y) = (rng.f(0.0, 5000.0), rng.f(0.0, 5000.0));
                    let t = Instant::now();
                    c.execute(Actor::Human, CanvasCommand::MoveNode { id, x, y })
                        .unwrap();
                    samples.push(t.elapsed().as_micros());
                }
                let (p50, p95, p99) = percentiles(samples);
                println!(
                    "[bench_canvas] move: MoveNode 提交 {m} 次（{n} 节点在场）p50 {p50} µs / p95 {p95} µs / p99 {p99} µs"
                );
                println!("[bench_canvas] move: peak RSS {}", rss_str());
            }
            // 命中查找（find_object——模型级命中代理）。
            "find" => {
                let c = session_with(n);
                let mut rng = Lcg(0x2545F4914F6CDD1D);
                let mut samples = Vec::with_capacity(m as usize);
                for _ in 0..m {
                    let id = format!("n{}", rng.next() as u32 % n);
                    let t = Instant::now();
                    let _ = c.find_object(&id);
                    samples.push(t.elapsed().as_micros());
                }
                let (p50, p95, p99) = percentiles(samples);
                println!(
                    "[bench_canvas] find: find_object {m} 次（{n} 节点）p50 {p50} µs / p95 {p95} µs / p99 {p99} µs"
                );
            }
            // 读模型快照（objects()——渲染器每帧消费面）。
            "readmodel" => {
                let c = session_with(n);
                let mut samples = Vec::new();
                for _ in 0..100 {
                    let t = Instant::now();
                    let objs = c.objects();
                    samples.push((t.elapsed().as_micros(), objs.len()));
                }
                let times: Vec<u128> = samples.iter().map(|(t, _)| *t).collect();
                let (p50, p95, p99) = percentiles(times);
                println!(
                    "[bench_canvas] readmodel: objects() 全量快照 ×100（{} 对象）p50 {p50} µs / p95 {p95} µs / p99 {p99} µs",
                    samples[0].1
                );
                println!("[bench_canvas] readmodel: peak RSS {}", rss_str());
            }
            // 全画布指纹（证据重验面——refresh 全对照的画布侧形态）。
            "fingerprint" => {
                let c = session_with(n);
                let mut times = Vec::new();
                for _ in 0..10 {
                    let t = Instant::now();
                    let _ = c.canvas_fingerprint();
                    times.push(t.elapsed().as_micros());
                }
                let (p50, p95, _) = percentiles(times);
                println!(
                    "[bench_canvas] fingerprint: canvas_fingerprint ×10（{n} 节点）p50 {p50} µs / p95 {p95} µs"
                );
            }
            // 协同同步（差量编码 + 新端应用——加入追平面）。
            "sync" => {
                let c = session_with(n);
                let t = Instant::now();
                let full = c
                    .encode_diff(&{
                        // 空 SV = 新端全量差量。
                        let fresh = CanvasSession::open(opts(7_200_102)).unwrap();
                        fresh.state_vector()
                    })
                    .unwrap();
                let enc_ms = t.elapsed().as_millis();
                let peer = CanvasSession::open(opts(7_200_103)).unwrap();
                let t2 = Instant::now();
                peer.apply_update(&full).unwrap();
                let apply_ms = t2.elapsed().as_millis();
                let same = peer.objects().len() == c.objects().len();
                println!(
                    "[bench_canvas] sync: 全量差量编码 {enc_ms} ms（{} B）+ 新端应用 {apply_ms} ms（对象数一致 = {same}）",
                    full.len()
                );
                println!("[bench_canvas] sync: peak RSS {}", rss_str());
            }
            // 删除 + 撤销复原（Map 键身份回归——删除撤销同 ID）。
            "delete" => {
                let mut c = session_with(n);
                let mut rng = Lcg(0x2545F4914F6CDD1D);
                let mut samples = Vec::with_capacity(m as usize);
                for _ in 0..m {
                    let id = format!("n{}", rng.next() as u32 % n);
                    let t = Instant::now();
                    c.execute(Actor::Human, CanvasCommand::RemoveObject { id: id.clone() })
                        .unwrap();
                    let undone = c.undo(Actor::Human);
                    samples.push(t.elapsed().as_micros());
                    assert!(undone, "撤销复原");
                    assert!(c.find_object(&id).is_some(), "删除撤销同 ID 回归");
                }
                let (p50, p95, p99) = percentiles(samples);
                println!(
                    "[bench_canvas] delete: 删除+撤销复原 {m} 次（{n} 节点）p50 {p50} µs / p95 {p95} µs / p99 {p99} µs"
                );
            }
            _ => {
                eprintln!("用法：bench_canvas <load|move|find|readmodel|fingerprint|sync|delete> <nodes> <samples>");
                std::process::exit(2);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    bench::run();
}
