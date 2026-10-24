//! s5a-env Xvfb 实机冒烟（最高目标面）：tao 事件循环 + wry WebView 在
//! 无显示环境（xvfb-run）下的存活验证。
//!
//! 运行（沙箱自愈环境）：
//! ```sh
//! source /home/z/my-project/optroot/env.sh
//! xvfb-run -a cargo run -p mdshell-native --example xvfb_smoke -- --smoke-sec 6
//! ```
//! 判定：stdout 出现 `smoke: quit_after reached; evaluate_script probe ok=true`
//! 且进程退出码 0（run_smoke 在 LoopDestroyed 里置码）。
//!
//! 可选参数：`--url <url>`（默认 about:blank；web/dist 构建后可传
//! file://…/web/dist/index.html）、`--smoke-sec <n>`（默认 6）。

use mdshell_native::native::{run_smoke, ShellConfig};

fn main() {
    let mut url = String::from("about:blank");
    let mut sec: u64 = 6;
    let mut flush_ms: u64 = 700;
    let mut probe_js: Option<String> = None;
    let mut dist: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--url" => url = args.next().expect("--url 需要值"),
            "--smoke-sec" => {
                sec = args
                    .next()
                    .expect("--smoke-sec 需要值")
                    .parse()
                    .expect("--smoke-sec 需为整数")
            }
            "--flush-ms" => {
                flush_ms = args
                    .next()
                    .expect("--flush-ms 需要值")
                    .parse()
                    .expect("--flush-ms 需为整数")
            }
            "--dist" => {
                dist = Some(args.next().expect("--dist 需要值（web/dist 目录）"));
            }
            "--probe-js" => probe_js = Some(args.next().expect("--probe-js 需要值")),
            other => eprintln!("xvfb_smoke: 未知参数 {other}（支持 --url/--dist/--smoke-sec/--flush-ms/--probe-js）"),
        }
    }
    let cfg = ShellConfig {
        url,
        title: "mdcore s5a-env Xvfb smoke".into(),
        size: (1024, 768),
        quit_after: Some(std::time::Duration::from_secs(sec)),
        flush_millis: flush_ms,
        probe_js,
        dist_root: dist,
    };
    println!("xvfb_smoke: starting url={} quit_after={sec}s", cfg.url);
    match run_smoke(cfg) {
        Ok(()) => println!("xvfb_smoke: run_smoke returned Ok (常驻模式自然退出)"),
        Err(e) => {
            eprintln!("xvfb_smoke: build-phase failure: {e}");
            std::process::exit(2);
        }
    }
}
