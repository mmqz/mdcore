//! GPUI Probe —— 验证 GPUI 0.2.2 在本机能否跑三栏布局 + 中文
//!
//! 对应 issue: mmqz/mdcore#58

use gpui::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

struct ProbeApp {
    status: SharedString,
}

impl Render for ProbeApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0xffffff))
            // ── 顶部标题栏
            .child(
                div()
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .px_4()
                    .bg(rgb(0xf7f8fa))
                    .border_b_1()
                    .border_color(rgb(0xcfd4dc))
                    .text_lg()
                    .child("标书文档引擎 — GPUI Probe"),
            )
            // ── 三栏主体
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    // 左栏 200px
                    .child(
                        div()
                            .w(px(200.0))
                            .h_full()
                            .bg(rgb(0xf7f8fa))
                            .border_r_1()
                            .border_color(rgb(0xcfd4dc))
                            .p_2()
                            .text_sm()
                            .child("文件树"),
                    )
                    // 中栏 flex
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .p_4()
                            .child(div().text_2xl().child("中文渲染测试")),
                    )
                    // 右栏 250px
                    .child(
                        div()
                            .w(px(250.0))
                            .h_full()
                            .bg(rgb(0xf7f8fa))
                            .border_l_1()
                            .border_color(rgb(0xcfd4dc))
                            .p_2()
                            .text_sm()
                            .child("大纲 / 属性"),
                    ),
            )
            // ── 状态栏
            .child(
                div()
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .px_3()
                    .bg(rgb(0xf7f8fa))
                    .border_t_1()
                    .border_color(rgb(0xcfd4dc))
                    .text_xs()
                    .child(format!("{}", self.status)),
            )
    }
}

fn main() {
    println!("[probe-gpui] starting...");
    let t0 = std::time::Instant::now();
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| ProbeApp {
                    status: "GPUI 0.2.2 · 三栏布局 · 中文 OK".into(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
        println!("[probe-gpui] window opened in {:?}", t0.elapsed());
    });
}