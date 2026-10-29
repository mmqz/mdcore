//! mang 桌面壳（路线 3：wry + tao）
//!
//! 设计：
//!   - 单一 WebView 填满窗口，外壳用 HTML/CSS（现有 web/ + 新增 shell UI）
//!   - custom protocol `mang://` 加载 dist，避开 file:// 的 wasm CORS 限制
//!   - 后续要脱 WebView 时，外壳可以换成 GPUI（已验证组件可用）

use std::borrow::Cow;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

fn main() -> wry::Result<()> {
    // dist 目录
    let dist = std::env::args().nth(1).unwrap_or_else(|| {
        "E:/Code/ai/md/audit/mdcore/web/dist".to_string()
    });
    let dist = std::path::PathBuf::from(dist);
    if !dist.join("index.html").exists() {
        eprintln!("[mang] 错误：找不到 {}", dist.join("index.html").display());
        std::process::exit(1);
    }
    println!("[mang] dist = {}", dist.display());

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("mang")
        .with_inner_size(LogicalSize::new(1280.0, 860.0))
        .with_min_inner_size(LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .expect("窗口创建失败");

    let root = dist.clone();
    let webview = WebViewBuilder::new()
        // 自定义协议：mang://localhost/<path> -> dist/<path>
        .with_custom_protocol("mang".into(), move |_id, req| {
            serve_file(&root, req)
        })
        .with_url("mang://localhost/index.html")
        .build(&window)
        .expect("WebView 创建失败");

    println!("[mang] WebView 就绪");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                // wry 的 build(&window) 模式会自动跟随窗口尺寸
            }
            _ => {
                let _ = &webview;
            }
        }
    });
}

/// 把 mang://localhost/<path> 映射到 dist 目录
fn serve_file(
    root: &std::path::Path,
    req: wry::http::Request<Vec<u8>>,
) -> wry::http::Response<Cow<'static, [u8]>> {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let full = root.join(path);

    let mime = match full.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };

    match std::fs::read(&full) {
        Ok(bytes) => wry::http::Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Access-Control-Allow-Origin", "*")
            .body(Cow::Owned(bytes))
            .unwrap(),
        Err(_) => wry::http::Response::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body(Cow::Borrowed(b"404 not found".as_slice()))
            .unwrap(),
    }
}