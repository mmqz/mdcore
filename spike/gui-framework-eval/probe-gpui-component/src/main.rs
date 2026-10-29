//! gpui-component Probe —— 用官方 API 验证三栏文档工作台外壳

use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

struct DocWorkbench {
    status: SharedString,
}

impl Render for DocWorkbench {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex().flex_col().size_full()
            .bg(rgb(0xffffff))
            // 标题栏
            .child(
                div().h(px(40.0)).flex().items_center().px_4()
                    .bg(rgb(0xf7f8fa))
                    .border_b_1().border_color(rgb(0xcfd4dc))
                    .text_lg()
                    .child("标书文档引擎 — gpui-component"),
            )
            // 三栏
            .child(
                div().flex().flex_1().min_h_0()
                    // 图标条
                    .child(
                        div().w(px(48.0)).h_full()
                            .flex().flex_col().items_center().py_2().gap_2()
                            .bg(rgb(0xf7f8fa))
                            .border_r_1().border_color(rgb(0xcfd4dc))
                            .child(icon_btn("📁", true))
                            .child(icon_btn("🔍", false))
                            .child(icon_btn("★", false))
                            .child(icon_btn("🕐", false))
                    )
                    // 侧栏
                    .child(
                        div().w(px(260.0)).h_full().flex().flex_col().p_2()
                            .bg(rgb(0xfafbfc))
                            .border_r_1().border_color(rgb(0xcfd4dc))
                            .child(div().text_sm().font_weight(gpui_kit::FontWeight::BOLD).mb_2().child("文件"))
                            .child(div().text_sm().child("▾ 项目 A"))
                            .child(div().text_sm().pl_4().child("├ 招标文件.docx"))
                            .child(div().text_sm().pl_4().child("├ 技术标.md"))
                            .child(div().text_sm().pl_4().child("└ 附件.pdf"))
                    )
                    // 编辑区
                    .child(
                        div().flex_1().h_full().flex().flex_col()
                            .child(
                                div().h(px(34.0)).flex().items_center().px_2().gap_1()
                                    .bg(rgb(0xf7f8fa))
                                    .border_b_1().border_color(rgb(0xcfd4dc))
                                    .child(tab_chip("招标文件.docx", true))
                                    .child(tab_chip("技术标.md", false))
                            )
                            .child(
                                div().flex_1().p_6()
                                    .child(div().text_2xl().mb_2().child("招标文件"))
                                    .child(div().child("这是用 gpui-component 拼出的文档工作台外壳。")),
                            )
                    )
            )
            // 状态栏
            .child(
                div().h(px(26.0)).flex().items_center().px_3()
                    .bg(rgb(0xf7f8fa))
                    .border_t_1().border_color(rgb(0xcfd4dc))
                    .text_xs()
                    .child(self.status.clone()),
            )
    }
}

fn icon_btn(icon: &str, active: bool) -> impl IntoElement {
    div().size(px(36.0))
        .flex().items_center().justify_center()
        .rounded_md().cursor_pointer()
        .when(active, |d| d.bg(rgb(0xe3e7ee)))
        .child(div().text_lg().child(icon.to_string()))
}

fn tab_chip(label: &str, active: bool) -> impl IntoElement {
    div().px_3().h(px(26.0))
        .flex().items_center().rounded_sm().text_sm().cursor_pointer()
        .when(active, |d| d.bg(rgb(0xffffff)).border_1().border_color(rgb(0xcfd4dc)))
        .child(label.to_string())
}

fn main() {
    let t0 = std::time::Instant::now();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(
                    Bounds::centered(None, size(px(1280.0), px(860.0)), cx)
                )),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| DocWorkbench {
                status: "gpui-kit 0.8.1 · 三栏工作台 · 中文 OK".into(),
            }),
        ).expect("open window failed");
        println!("[probe] window opened in {:?}", t0.elapsed());
    });
}