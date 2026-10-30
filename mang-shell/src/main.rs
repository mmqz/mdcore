//! mang 桌面外壳入口 —— GPUI 主线 UI（#63）。
//!
//! 用法：
//! ```text
//! mang-shell                 正常启动
//! mang-shell --smoke-sec N   烟雾模式：开窗 N 秒后自动退出（exit 0 +
//!                            stdout 末行 "smoke: quit reached"——Xvfb 无头
//!                            验证通道，循 mdshell-native xvfb_smoke 先例）
//! mang-shell --version       版本号
//! ```
//!
//! 引导链：`gpui_platform::application()`（OS 后端选择器）→ 内嵌资产源
//! （图标）→ `theme::init`（内置基础主题）→ `set_theme_settings_provider`
//! （ui 组件字体面契约）→ 开窗（1280×800，X11 客户端装饰请求，无合成器
//! 自动回退服务端装饰）。

use std::time::Duration;

use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions,
    point, px, size,
};
use gpui_platform::application;

mod activity_bar;
mod assets;
mod panels;
mod settings;
mod shell;
mod status_bar;
mod title_bar;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mang-shell {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let smoke_sec: Option<f64> = args
        .iter()
        .position(|a| a == "--smoke-sec")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            theme::init(theme::LoadThemes::JustBase, cx);
            theme::set_theme_settings_provider(Box::new(settings::ShellThemeSettings), cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(96.), px(64.)),
                    size: size(px(1280.), px(800.)),
                })),
                titlebar: Some(TitlebarOptions {
                    title: Some("mang".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                // X11 客户端装饰（CSD）需合成器；无合成器时 gpui_linux
                // 自动回退服务端装饰并记日志——两种形态外壳均完整。
                window_decorations: Some(WindowDecorations::Client),
                window_min_size: Some(size(px(760.), px(480.))),
                ..Default::default()
            };
            cx.open_window(options, |window, cx| {
                cx.new(|cx| shell::ShellApp::new(window, cx))
            })
            .expect("mang-shell: 打开主窗口失败");

            if let Some(sec) = smoke_sec {
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs_f64(sec))
                        .await;
                    let _ = cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });

    println!("smoke: quit reached");
}
