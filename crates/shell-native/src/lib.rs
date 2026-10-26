//! mdshell-native — R10 原生宿主壳（tao + wry + GTK3，ADR-H1 定谳：Slint
//! 因 GPL 证与 ADR-C1 排除）胶水。
//!
//! ⚠️ 状态史：设计定稿 →（s5a-env 轮）**依赖已实体化**。沙箱曾因无 sudo
//! 无法安装 webkit2gtk-4.1-dev；s5a-env 以 `apt download` + `dpkg -x` 提取树
//! （/home/z/my-project/optroot/root + env.sh）完成 dev 面自愈后，本 crate
//! 从注释态转为实体态（wry/tao/gtk + path 依赖），`cargo check -p
//! mdshell-native` 随 workspace default-members 正常编译（构建前先
//! `source /home/z/my-project/optroot/env.sh`）。蓝图形态的冒烟锁
//! `blueprint_constants_present` 保留：常量面在任何环境可测。
//!
//! 结构（与 mdshell 契约层、web/dist 的对接关系）：
//!
//! ```text
//! ┌─ tao 事件循环（窗口/文件对话框）─────────────────────┐
//! │  GTK3 原生控件：标题栏 / 打开按钮 / 进度条（OpenProgress 驱动）│
//! │  wry WebView：加载 dist/index.html（R9/R10 bundle 自包含）   │
//! │    │ host.surface_load_markdown(md)  ← mdshell::Host 实现    │
//! │    │   = webview.evaluate_script("window.__inject(md)")      │
//! │  文件层：open_document / open_document_progressive / save    │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! 对接点清单（实现时逐项落）：
//! 1. 窗口创建：tao WindowBuilder → wry WebViewBuilder::new().with_url(file://dist/index.html)
//! 2. 文本注入：main.ts 增加 `window.__inject(md)` 入口（读取参数或 postMessage
//!    触发 `new WasmCore(md)` 重建），替代 `?doc=` URL 参数（大文档不宜走 URL）；
//! 3. 渐进打开进度：mdshell::open_document_progressive 的 on_progress 回调
//!    → GTK3 进度条 + 完成后 surface_load_markdown 注入终态；
//! 4. 保存：Ctrl+S 拦截 → window.__md() 取全文 → mdshell::save_document
//!    → tao 文件对话框写盘（markdown + meta JSON 两文件或伴随文件）；
//! 5. document_id 重开匹配：meta JSON 存档后 open_document 走同一身份。

#![allow(dead_code)]

/// 注入脚本模板（对接点 2）。`__md_payload` 由宿主 JSON 转义后替换。
pub const INJECT_SNIPPET: &str = r#"
window.__inject = (md) => {
  // 宿主文件打开路径：整体重建核（渐进打开走 __inject_chunk，见下）
  window.dispatchEvent(new CustomEvent('mdc-inject', { detail: md }));
};
window.__inject_chunk = (chunk, first) => {
  // 渐进打开路径：first=true 重建核，否则 extend_from_markdown 追加
  window.dispatchEvent(new CustomEvent('mdc-inject-chunk', { detail: { chunk, first } }));
};
"#;

/// 渐进打开宿主侧伪代码（实现蓝图，非可运行体——依赖未启用）。
///
/// ```ignore
/// let mut host = WryHost::new(&webview);          // impl mdshell::Host
/// let session = mdshell::open_document_progressive(
///     &chunks, None, &mut host,
///     |p| gtk_progress.update(p),               // GTK3 进度条
/// )?;
/// ```
pub const BLUEPRINT_NOTE: &str = "see module docs; enable deps in Cargo.toml on a native host";

/// s5a-env：依赖实体化后的最小原生实现面（tao + wry + GTK3/Linux）。
///
/// 范围纪律：仅为实机连通性服务（cargo check + Xvfb 冒烟）——对接点
/// 清单 1–5 的完整壳功能不在 s5a-env 范围。三个构件：
/// 1. [`native::WryHost`]：`mdshell::Host` 在 wry WebView 上的最小实现
///    （evaluate_script 面，对接点 2 的宿主侧）；
/// 2. [`native::ShellConfig`] / [`native::run_smoke`]：tao 事件循环 +
///    wry WebView 的可退出冒烟运行器（examples/xvfb_smoke.rs 消费）；
/// 3. `escape_js_string`：注入脚本的 JSON 字符串转义（无三方 serde 依赖）。
#[cfg(target_os = "linux")]
pub mod native {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use tao::event::{Event, StartCause, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;

    /// 冒烟/最小壳运行配置。
    pub struct ShellConfig {
        /// 初始 URL（`about:blank`、`data:` 或 `file://…/dist/index.html`）。
        pub url: String,
        pub title: String,
        /// 逻辑尺寸（width, height）。
        pub size: (u32, u32),
        /// 存活看门狗：到时自动退出（None = 常驻直至窗口关闭）。
        pub quit_after: Option<Duration>,
        /// 探针发出后的回传等待窗（默认 700ms；异步探针/轮询式探针可加大）。
        pub flush_millis: u64,
        /// s5a-env 增强：看门狗到时执行的可选探针 JS（默认仅响应探测）。
        ///
        /// wry 0.46 `evaluate_script` 无返回值——探针结果经导航拦截通道回传：
        /// 探针 JS 自行触发 `location.assign("probe://r/" + encodeURIComponent(...))`，
        /// 宿主截获解码后打印 `smoke: probe: <payload>` 并记为证据（取消导航）。
        pub probe_js: Option<String>,
        /// mdapp:// 本地 bundle 根（指向 dist/）。设置后注册 mdapp:// scheme
        /// serve 该目录，初始 URL 改为 mdapp://localhost/。
        ///
        /// 为什么不用 file://：WebKit file:// 下 wasm fetch/ESM 被安全策略拦
        /// （Xvfb 实证：页面 title 出现但全部探针空、轮询 12 次不初始化；
        /// 同 bundle http:// 下全绿）——dist 自包含声明在 file:// 不成立。
        /// mdapp:// 是桌面壳加载本地 bundle 的产品级形态。
        pub dist_root: Option<String>,
    }

    /// wry WebView 宿主面（对接点 2 宿主侧的最小实现）。
    ///
    /// `surface_load_markdown` = 先装注入脚本（INJECT_SNIPPET）再调
    /// `window.__inject(md)`；错误透传 wry evaluate_script。
    pub struct WryHost<'a> {
        view: &'a wry::WebView,
    }

    impl<'a> WryHost<'a> {
        pub fn new(view: &'a wry::WebView) -> Self {
            Self { view }
        }
    }

    impl mdshell::Host for WryHost<'_> {
        fn surface_load_markdown(&mut self, markdown: &str) -> Result<(), String> {
            let js = format!(
                "{}\nwindow.__inject && window.__inject({});",
                crate::INJECT_SNIPPET,
                escape_js_string(markdown)
            );
            self.view
                .evaluate_script(&js)
                .map_err(|e| format!("wry evaluate_script: {e:?}"))
        }
    }

    /// JSON 风格字符串字面量转义（JS 注入安全面：无三方依赖，控制字符
    /// 全走 \uXXXX；未来换 serde_json 时可退役）。
    pub fn escape_js_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    /// percent-decoding（探针导航 URL 的 payload 还原；无三方依赖）。
    /// 非法转义序列（孤立 '%' 或截断的 %XX）原样保留——解码是尽力而为面，
    /// 不因敌意输入失败。
    pub fn percent_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            let take_hex = |idx: usize| -> Option<u8> {
                if idx >= bytes.len() {
                    return None;
                }
                match bytes[idx] {
                    b'0'..=b'9' => Some(bytes[idx] - b'0'),
                    b'a'..=b'f' => Some(bytes[idx] - b'a' + 10),
                    b'A'..=b'F' => Some(bytes[idx] - b'A' + 10),
                    _ => None,
                }
            };
            if bytes[i] == b'%' {
                if let (Some(h), Some(l)) = (take_hex(i + 1), take_hex(i + 2)) {
                    out.push((h << 4) | l);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// mdapp:// 静态文件服务（探针壳的产品级 bundle 加载面）。
    ///
    /// 职责：path → dist 下文件（/ 或空 → index.html）→ 200 + mime；
    /// 目录穿越（../ 或逃逸 root）→ 404；读失败 → 404。无缓存（每请求读盘，
    /// 冒烟/开发态优先简单；未来需要时再加）。
    pub fn serve_dist(
        root: &std::path::Path,
        raw_path: &str,
    ) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
        use std::borrow::Cow;

        let decoded = percent_decode(raw_path);
        let rel = decoded.trim_start_matches('/');
        let rel = if rel.is_empty() { "index.html" } else { rel };
        // 目录穿越防护：拒绝任何 .. 段（含编码后已被 percent_decode 展开的）。
        let segs: Vec<&str> = rel.split('/').collect();
        if segs.contains(&"..") {
            return not_found();
        }
        let full = root.join(rel);
        match std::fs::read(&full) {
            Ok(bytes) => {
                let mime = mime_of(rel);
                wry::http::Response::builder()
                    .status(200)
                    .header("Content-Type", mime)
                    .body(Cow::Owned(bytes))
                    .unwrap_or_else(|_| not_found())
            }
            Err(_) => not_found(),
        }
    }

    fn not_found() -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
        wry::http::Response::builder()
            .status(404)
            .body(std::borrow::Cow::Borrowed(&b"not found"[..]))
            .unwrap()
    }

    fn mime_of(path: &str) -> &'static str {
        let ext = path.rsplit('.').next().unwrap_or("");
        match ext {
            "html" => "text/html; charset=utf-8",
            "js" | "mjs" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "json" => "application/json",
            "wasm" => "application/wasm",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "ico" => "image/x-icon",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            _ => "application/octet-stream",
        }
    }

    /// 最小可退出壳：tao 事件循环 + wry WebView（Linux/X11 via GTK3）。
    ///
    /// 返回 `Err` 仅覆盖构建期失败；运行期存活/退出结论以 stdout 冒烟
    /// 日志为准（`smoke:` 前缀，examples/xvfb_smoke.rs 消费并置退出码）。
    pub fn run_smoke(cfg: ShellConfig) -> Result<(), String> {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .with_title(cfg.title)
            .with_inner_size(tao::dpi::LogicalSize::new(
                cfg.size.0 as f64,
                cfg.size.1 as f64,
            ))
            .build(&event_loop)
            .map_err(|e| format!("tao window: {e:?}"))?;

        // 探针回传面（导航拦截通道）：probe_js 触发 `location.assign("probe://r/<enc>")`，
        // 导航决策回调（String 通道，无 parse 陷阱）截获 → 解码存 slot → 返回
        // false 取消导航，页面留在原地。
        //
        // 为什么不用 with_ipc_handler：wry 0.46.3 webkitgtk 实现在 IPC 回调里做
        // `Request::builder().uri(webview.uri().unwrap().to_string())`——file:///
        // （空 authority）在 http::Uri 下 Err(InvalidUri(InvalidFormat)) → 宿主
        // panic（Xvfb 实证；about:blank 合法、probe://r/... 合法——本通道与该
        // parse 路径解耦）。缺陷注记：s5a-env 发现并绕开，未修上游。
        let probe_rx = Rc::new(std::cell::RefCell::new(None));
        let probe_tx = Rc::clone(&probe_rx);
        let mut builder = wry::WebViewBuilder::new();
        let mut url = cfg.url.clone();
        if let Some(root) = cfg.dist_root.clone() {
            builder = builder.with_custom_protocol("mdapp".to_string(), move |_id, request| {
                serve_dist(std::path::Path::new(&root), request.uri().path())
            });
            url = "mdapp://localhost/".to_string();
        }
        let webview = builder
            .with_url(url.as_str()) // wry 0.46：with_url 不落盘不失败（无 Result）
            .with_navigation_handler(move |url: String| {
                const PREFIX: &str = "probe://r/";
                if let Some(rest) = url.strip_prefix(PREFIX) {
                    // 只记首条（探针一次性）；幂等重放不改变结论。
                    let decoded = percent_decode(rest);
                    let mut slot = probe_tx.borrow_mut();
                    if slot.is_none() {
                        *slot = Some(decoded.clone());
                        println!("smoke: probe: {decoded}");
                    }
                    return false; // 取消导航：页面留在原处
                }
                true // 其余导航放行
            })
            .build(&window)
            .map_err(|e| format!("wry build: {e:?}"))?;

        let deadline = cfg.quit_after.map(|d| Instant::now() + d);
        let alive = Cell::new(false);
        let exited = Cell::new(false);
        let logged = Cell::new(false);
        // 探针发出后给 IPC 回传留一个 flush 窗（WebKit postMessage 异步
        // 到达宿主）——窗内不退出；窗尽再 Exit。LoopDestroyed 处兜底打印。
        let flush_until = Cell::<Option<Instant>>::new(None);
        event_loop.run(move |event, _, control_flow| {
            match event {
                Event::NewEvents(StartCause::Init) => {
                    println!("smoke: tao loop init; webview built (url={url})");
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    exited.set(true);
                    *control_flow = ControlFlow::Exit;
                }
                Event::MainEventsCleared => {
                    if let Some(d) = deadline {
                        if Instant::now() >= d && !exited.get() {
                            match flush_until.get() {
                                None => {
                                    // 首达看门狗：执行探针（默认仅响应探测）。
                                    let probe_src = cfg.probe_js.as_deref().unwrap_or(
                                        "window.__mdc_smoke_probe = 1",
                                    );
                                    let probe = webview.evaluate_script(probe_src);
                                    alive.set(probe.is_ok());
                                    if !logged.replace(true) {
                                        println!(
                                            "smoke: quit_after reached; evaluate_script probe ok={} \
                                             (webview responsive)",
                                            probe.is_ok()
                                        );
                                    }
                                    flush_until.set(Some(
                                        Instant::now() + Duration::from_millis(cfg.flush_millis.max(1)),
                                    ));
                                }
                                Some(f) if Instant::now() >= f => {
                                    exited.set(true);
                                    *control_flow = ControlFlow::Exit;
                                }
                                _ => {} // flush 窗内继续等 IPC
                            }
                        }
                    }
                }
                Event::LoopDestroyed => {
                    let probe_msg = probe_rx.borrow_mut().take();
                    if let Some(m) = probe_msg {
                        println!("smoke: loop destroyed; alive={}; probe={m:?}", alive.get());
                    } else {
                        println!("smoke: loop destroyed; alive={}", alive.get());
                    }
                    let ok = alive.get();
                    std::process::exit(if ok { 0 } else { 1 });
                }
                _ => {}
            }
            if !exited.get() {
                *control_flow = match flush_until.get().or(deadline) {
                    Some(t) => ControlFlow::WaitUntil(t),
                    None => ControlFlow::Wait,
                };
            }
        })
    }
}

#[cfg(test)]
mod tests {
    /// 零依赖冒烟：保证本 crate 在任何环境都可编译（蓝图常量非空）。
    #[test]
    fn blueprint_constants_present() {
        assert!(super::INJECT_SNIPPET.contains("__inject_chunk"));
        assert!(!super::BLUEPRINT_NOTE.is_empty());
    }

    /// s5a-env：JS 字符串转义面（无三方依赖路径的自锁）。
    #[test]
    fn js_escape_roundtrip_samples() {
        let e = super::native::escape_js_string("a\"b\\c\nd\te\u{1}");
        assert_eq!(e, "\"a\\\"b\\\\c\\nd\\te\\u0001\"");
        // 空串与 unicode 边界
        assert_eq!(super::native::escape_js_string(""), "\"\"");
        assert_eq!(super::native::escape_js_string("中文"), "\"中文\"");
    }

    /// s5a-env：percent_decode 面（合法/非法转义/中文/孤立 %）。
    #[test]
    fn percent_decode_samples() {
        use super::native::percent_decode;
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87"), "中文");
        // 非法序列：孤立 %、截断 %E、非 hex——原样保留不 panic
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%E"), "%E");
        assert_eq!(percent_decode("%zz"), "%zz");
        // probe://r/ 实际载荷形态（encodeURIComponent 产物）
        assert_eq!(
            percent_decode("%7B%22title%22%3A%22x%22%7D"),
            "{\"title\":\"x\"}"
        );
    }

    /// s5a-env：mdapp serve_dist 面（默认页/200+mime/穿越拒绝/404）。
    /// 用临时目录构造最小 bundle，不依赖 web/dist（测试自包含）。
    #[test]
    fn serve_dist_mapping_and_traversal() {
        use super::native::serve_dist;
        use std::fs;

        let tmp = std::env::temp_dir().join(format!("mdshell_native_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("pkg")).expect("mkdir");
        fs::write(tmp.join("index.html"), "<html>ok</html>").expect("write");
        fs::write(tmp.join("pkg/a.wasm"), b"\0asm-payload").expect("write");

        // / → index.html；路径形 → 文件；mime 面
        let r = serve_dist(&tmp, "/");
        assert_eq!(r.status(), 200);
        assert_eq!(
            r.headers().get("Content-Type").unwrap(),
            "text/html; charset=utf-8"
        );
        let r = serve_dist(&tmp, "/pkg/a.wasm");
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers().get("Content-Type").unwrap(), "application/wasm");
        assert_eq!(r.body().as_ref(), b"\0asm-payload");

        // 目录穿越四形（含 percent 编码形）——全 404
        for p in [
            "/../etc/passwd",
            "/..%2F..%2Fetc/passwd",
            "/pkg/../../x",
            "/a/..",
        ] {
            let r = serve_dist(&tmp, p);
            assert_eq!(r.status(), 404, "path {p} must 404");
        }

        // 不存在文件 → 404
        assert_eq!(serve_dist(&tmp, "/nope.js").status(), 404);

        let _ = fs::remove_dir_all(&tmp);
    }
}
