//! 底部状态栏（#63 Lead：底部状态栏）。
//!
//! 左：应用/文档状态；右：版本与引擎标识。背景色差（title_bar_background）
//! 与主编辑区区分，无 1px 边框。

use gpui::{App, Pixels, px};
use ui::prelude::*;
use ui::{Label, LabelSize};

/// 状态栏高度（px）。
pub const STATUS_BAR_HEIGHT: Pixels = px(24.);

pub fn status_bar(cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    h_flex()
        .id("status-bar")
        .h(STATUS_BAR_HEIGHT)
        .flex_none()
        .w_full()
        .bg(colors.title_bar_background)
        .px_3()
        .justify_between()
        .child(
            Label::new("就绪 —— GPUI 外壳骨架")
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
        )
        .child(
            h_flex()
                .gap_3()
                .child(
                    Label::new("mdcore 0.1.0")
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(
                    Label::new("GPL-3.0-or-later")
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                ),
        )
}
