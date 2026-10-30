//! 自绘标题栏（#63 Lead：顶部标题栏，可拖拽窗口）。
//!
//! 结构：`[标题 16px 边距] [flex 拖拽区] [窗口控制钮 3 个]`。
//! 拖拽 = 拖拽区 `start_window_move()`；双击 = `titlebar_double_click()`
//! （缩放/还原）。窗口控制走平台 API（`minimize_window` / `zoom_window` /
//! 退出）——处理器闭包参数自带 `&mut Window`，无需捕获。
//!
//! 注：X11 下客户端装饰（CSD）需合成器支持，无合成器时 gpui_linux 自动
//! 回退服务端装饰（WM 标题栏）——本栏恒在客户区内渲染，两种形态不塌。

use gpui::{App, MouseButton, Pixels, px};
use ui::prelude::*;
use ui::{IconButton, IconName, Label, LabelSize};

/// 标题栏高度（px）。
pub const TITLE_BAR_HEIGHT: Pixels = px(36.);

pub fn title_bar(title: &str, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    h_flex()
        .id("title-bar")
        .h(TITLE_BAR_HEIGHT)
        .flex_none()
        .w_full()
        .bg(colors.title_bar_background)
        // 拖拽与双击只挂中央弹性区——避免点控制钮时连带触发窗口移动。
        .child(
            div()
                .id("title-bar-drag")
                .flex_1()
                .h_full()
                .on_mouse_down(MouseButton::Left, |_, window, _| {
                    window.start_window_move();
                })
                .on_click(|event, window, _| {
                    if let gpui::ClickEvent::Mouse(mouse) = event
                        && mouse.up.click_count >= 2
                    {
                        window.titlebar_double_click();
                    }
                })
                .child(
                    h_flex().h_full().items_center().pl_4().child(
                        Label::new(title)
                            .size(LabelSize::Small)
                            .color(ui::Color::Muted),
                    ),
                ),
        )
        .child(window_controls())
}

fn window_controls() -> impl IntoElement {
    h_flex()
        .id("window-controls")
        .h_full()
        .flex_none()
        .gap_1()
        .pr_2()
        .child(
            IconButton::new("win-minimize", IconName::SquareMinus)
                .on_click(|_, window, _| window.minimize_window()),
        )
        .child(
            IconButton::new("win-maximize", IconName::Maximize)
                .on_click(|_, window, _| window.zoom_window()),
        )
        .child(IconButton::new("win-close", IconName::Close).on_click(|_, _, cx| cx.quit()))
}
