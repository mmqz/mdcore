//! `mdcore` 宿主壳二进制（R24-c/s5a-env 实体面）。
//!
//! 职责（最小冒烟级——完整壳功能是 R24-d 开发环境轮范围）：
//! - `--version`：打印 `mdcore <VERSION>`（单源 = mdcore::VERSION =
//!   CARGO_PKG_VERSION；packaging/deb/build-deb.sh 与 appimage 脚本以
//!   `awk '{print $2}'` 消费本输出，改格式需同步改脚本）。
//! - 启动 tao+wry 壳加载工作台 bundle（mdapp://）。
//!
//! dist 解析序（首个存在者胜出）：
//! 1. `--dist <dir>` 参数
//! 2. `MDCORE_DIST` 环境变量
//! 3. `/usr/share/mdcore/dist`（deb/AppImage 布局——packaging 捆绑位）
//! 4. `<cwd>/web/dist`（开发树运行形态）
//!
//! 自动化冒烟参数（CI/Xvfb）：`--smoke-sec <n>`（看门狗）、`--probe-js <js>`
//! （导航通道探针）、`--flush-ms <n>`。无参数运行 = 常驻窗口直至关闭。

#[cfg(target_os = "linux")]
use mdshell_native::native::{run_smoke, ShellConfig};

fn dist_candidates(explicit: Option<&str>) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = explicit {
        v.push(std::path::PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("MDCORE_DIST") {
        if !d.is_empty() {
            v.push(std::path::PathBuf::from(d));
        }
    }
    v.push(std::path::PathBuf::from("/usr/share/mdcore/dist"));
    v.push(std::path::PathBuf::from("web/dist"));
    v
}

fn main() {
    let mut explicit_dist: Option<String> = None;
    let mut url: Option<String> = None;
    let mut smoke_sec: Option<u64> = None;
    let mut flush_ms: u64 = 700;
    let mut probe_js: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" => {
                println!("mdcore {}", mdcore::VERSION);
                return;
            }
            "--dist" => explicit_dist = Some(args.next().expect("--dist 需要值")),
            "--url" => url = Some(args.next().expect("--url 需要值")),
            "--smoke-sec" => {
                smoke_sec = Some(
                    args.next()
                        .expect("--smoke-sec 需要值")
                        .parse()
                        .expect("--smoke-sec 需为整数"),
                )
            }
            "--flush-ms" => {
                flush_ms = args
                    .next()
                    .expect("--flush-ms 需要值")
                    .parse()
                    .expect("--flush-ms 需为整数")
            }
            "--probe-js" => probe_js = Some(args.next().expect("--probe-js 需要值")),
            other => {
                eprintln!(
                    "mdcore: 未知参数 {other}（--version/--dist/--url/--smoke-sec/--flush-ms/--probe-js）"
                );
                std::process::exit(2);
            }
        }
    }

    // dist 解析：显式 URL 时不需要 dist；否则按序找首个存在的目录。
    let dist_root: Option<String> = if url.is_some() {
        None
    } else {
        match dist_candidates(explicit_dist.as_deref())
            .into_iter()
            .find(|p| p.is_dir())
        {
            Some(p) => Some(p.to_string_lossy().into_owned()),
            None => {
                eprintln!(
                    "mdcore: 找不到工作台 bundle（候选：--dist 参数 / MDCORE_DIST / \
                     /usr/share/mdcore/dist / ./web/dist）；或用 --url 指定"
                );
                std::process::exit(2);
            }
        }
    };

    #[cfg(target_os = "linux")]
    {
        let cfg = ShellConfig {
            url: url.unwrap_or_else(|| "mdapp://localhost/".into()),
            title: "mdcore".into(),
            size: (1280, 900),
            quit_after: smoke_sec.map(std::time::Duration::from_secs),
            flush_millis: flush_ms,
            probe_js,
            dist_root,
        };
        if let Err(e) = run_smoke(cfg) {
            eprintln!("mdcore: 壳构建失败：{e}");
            std::process::exit(2);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // 保编译 stub（issue #4）：GUI 面 = Linux-only（ADR-H1 GTK3/
        // WebKitGTK 栈）。非 Linux 目标本 bin 仅保编译与 --version 面。
        let _ = (explicit_dist, url, smoke_sec, flush_ms, probe_js, dist_root);
        eprintln!(
            "mdcore: GUI 壳仅支持 Linux（ADR-H1：tao+wry+GTK3/WebKitGTK 栈）；本目标为仅保编译面。"
        );
    }
}
