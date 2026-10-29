//! Probe Shell v3 —— 全部用 gpui-component 现成组件 + WebView2
//!
//! 已验证的关键点（踩过的坑）：
//!   1. 必须 `.with_assets(AllAssets::new(""))`，否则所有图标空白
//!   2. 标题栏用 `TitleBar::window_options()` 才能自绘
//!   3. WebView 用 `build_as_child(window)`（GPUI Window 实现 HasWindowHandle）
//!   4. gpui_component::IconName 只有 106 个默认图标

use gpui_kit::*;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, TitleBar, h_flex, v_flex, Sizable as _,
    button::Toggle,
    status_bar::StatusBar,
    Size,
};
use std::cell::RefCell;
use std::rc::Rc;

const ICONBAR_W: f32 = 44.0;
const TITLEBAR_H: f32 = 34.0;
const STATUSBAR_H: f32 = 26.0;

struct WebViewSlot {
    webview: Option<wry::WebView>,
    bounds: Option<(f32, f32, f32, f32)>,
}

#[derive(Clone, Copy, PartialEq)]
enum View { Files, Search, Star, Recent, Settings }

struct Shell {
    active: View,
    webview: Rc<RefCell<WebViewSlot>>,
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let vw: f32 = vp.width.into();
        let vh: f32 = vp.height.into();

        let cx0 = ICONBAR_W;
        let cy0 = TITLEBAR_H;
        let cw = (vw - ICONBAR_W).max(1.0);
        let ch = (vh - TITLEBAR_H - STATUSBAR_H).max(1.0);

        {
            let mut s = self.webview.borrow_mut();
            let new = (cx0, cy0, cw, ch);
            let changed = match s.bounds {
                Some(b) => (b.0-new.0).abs()>0.5 || (b.1-new.1).abs()>0.5
                    || (b.2-new.2).abs()>0.5 || (b.3-new.3).abs()>0.5,
                None => true,
            };
            if changed {
                if let Some(wv) = s.webview.as_ref() {
                    let _ = wv.set_bounds(wry::Rect {
                        position: wry::dpi::Position::Physical(
                            wry::dpi::PhysicalPosition::new(cx0 as i32, cy0 as i32)),
                        size: wry::dpi::Size::Physical(
                            wry::dpi::PhysicalSize::new(cw as u32, ch as u32)),
                    });
                    s.bounds = Some(new);
                }
            }
        }

        // 诊断：打印实际尺寸
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            println!("[dims] viewport={vw}x{vh} webview=({cx0},{cy0},{cw},{ch})");
        });

        let th = cx.theme();
        let active = self.active;

        v_flex().size_full().bg(th.background)
            // 自绘标题栏
            .child(
                TitleBar::new().child(
                    h_flex().px_2()
                        .child(div().text_sm().child("mang")),
                ),
            )
            // 主体
            .child(
                h_flex().flex_1().min_h_0()
                    // 图标条
                    .child(
                        v_flex()
                            .w(px(ICONBAR_W)).h_full()
                            .items_center().py_2().gap_1()
                            .bg(th.sidebar)
                            .border_r_1().border_color(th.border)
                            .child(toggle_btn(0, IconName::Folder, "文件", active == View::Files, cx.listener(move |this: &mut Shell, _: &bool, _w, cx| { this.active = View::Files; cx.notify(); })))
                            .child(toggle_btn(1, IconName::Search, "搜索", active == View::Search, cx.listener(move |this: &mut Shell, _: &bool, _w, cx| { this.active = View::Search; cx.notify(); })))
                            .child(toggle_btn(2, IconName::Star, "收藏", active == View::Star, cx.listener(move |this: &mut Shell, _: &bool, _w, cx| { this.active = View::Star; cx.notify(); })))
                            .child(toggle_btn(3, IconName::FileText, "最近", active == View::Recent, cx.listener(move |this: &mut Shell, _: &bool, _w, cx| { this.active = View::Recent; cx.notify(); })))
                            .child(div().flex_1())
                            .child(toggle_btn(4, IconName::Settings, "设置", active == View::Settings, cx.listener(move |this: &mut Shell, _: &bool, _w, cx| { this.active = View::Settings; cx.notify(); })))
                    )
                    // WebView 占位区
                    .child(div().flex_1().h_full()),
            )
            // 状态栏
            .child(
                StatusBar::new().left(
                    div().text_xs().child("GPUI 外壳 + WebView2 内容"),
                ),
            )
    }
}

fn toggle_btn(
    idx: usize,
    icon: IconName,
    label: &str,
    checked: bool,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Toggle::new(("nav", idx))
        .icon(Icon::new(icon))
        .checked(checked)
        .tooltip(label.to_string())
        .with_size(Size::Small)
        .on_click(on_click)
}

fn main() {
    // 默认用 custom protocol（避开 file:// 的 fetch/CORS 限制，wasm 才能加载）
    let dist = std::env::args().nth(1)
        .unwrap_or_else(|| "E:/Code/ai/md/audit/mdcore/web/dist".to_string());
    let dist = std::path::PathBuf::from(dist);
    println!("[probe-shell] dist = {}", dist.display());

    let slot: Rc<RefCell<WebViewSlot>> = Rc::new(RefCell::new(WebViewSlot { webview: None, bounds: None }));
    let slot2 = slot.clone();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets::new(""))
        .run(move |cx| {
            gpui_kit::init(cx);
            let b = Bounds::centered(None, size(px(1280.0), px(860.0)), cx);
            let dist_c = dist.clone();
            let s = slot2.clone();

            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(b)),
                    ..TitleBar::window_options()
                },
                cx,
                move |window, cx| {
                    let root = dist_c.clone();
                    let protocol_root = dist_c.clone();

                    let result = wry::WebViewBuilder::new()
                        .with_custom_protocol("mang".into(), move |_id, req| {
                            serve_file(&protocol_root, req)
                        })
                        .with_url("mang://localhost/index.html")
                        .with_bounds(wry::Rect {
                            position: wry::dpi::Position::Physical(
                                wry::dpi::PhysicalPosition::new(44, 34)),
                            size: wry::dpi::Size::Physical(
                                wry::dpi::PhysicalSize::new(1236, 800)),
                        })
                        .build_as_child(window);

                    match result {
                        Ok(wv) => {
                            println!("[probe-shell] WebView OK (mang:// protocol, dist={})", root.display());
                            s.borrow_mut().webview = Some(wv);
                            s.borrow_mut().bounds = Some((44.0, 34.0, 1236.0, 800.0));
                        }
                        Err(e) => eprintln!("[probe-shell] WebView FAIL: {e}"),
                    }
                    cx.new(|_| Shell { active: View::Files, webview: s.clone() })
                },
            ).expect("open window failed");
            cx.activate(true);
        });
}

/// 把 mang://localhost/<path> 映射到 dist 目录下的文件。
fn serve_file(root: &std::path::Path, req: wry::http::Request<Vec<u8>>) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
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
        _ => "application/octet-stream",
    };

    match std::fs::read(&full) {
        Ok(bytes) => {
            println!("[proto] 200 {} ({} bytes)", path, bytes.len());
            wry::http::Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .header("Access-Control-Allow-Origin", "*")
                .body(std::borrow::Cow::Owned(bytes))
                .unwrap()
        }
        Err(e) => {
            println!("[proto] 404 {} ({})", path, e);
            wry::http::Response::builder()
                .status(404)
                .header("Content-Type", "text/plain")
                .body(std::borrow::Cow::Borrowed(b"not found".as_slice()))
                .unwrap()
        }
    }
}