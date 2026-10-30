//! 左右面板（#63 Lead：左栏文件树/大纲 260px 可折叠；右侧属性/来源
//! 280px 可折叠）。
//!
//! 骨架面：静态结构占位（文件树两层 / 属性键值行），真实数据接线
//! （mdcore 引擎文档模型）在后续 PR。面板边框一律背景色差
//! （Lead：用背景色差代替 1px 边框）。

use gpui::{App, Hsla, Pixels, px};
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

/// 左栏宽度（px）。
pub const LEFT_PANEL_WIDTH: Pixels = px(260.);
/// 右栏宽度（px）。
pub const RIGHT_PANEL_WIDTH: Pixels = px(280.);
/// 面板主内边距（px，Lead：主要区域 12-16px）。
const PANEL_PADDING: Pixels = px(12.);
/// 面板头部下缘留白（px）。
const HEADER_MARGIN: Pixels = px(8.);

/// 左栏：文件树骨架（静态两层）。
pub fn left_panel(cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let hover = colors.element_hover;
    v_flex()
        .id("left-panel")
        .w(LEFT_PANEL_WIDTH)
        .flex_none()
        .h_full()
        .min_w_0()
        .bg(colors.panel_background)
        .p(PANEL_PADDING)
        .gap_1()
        .overflow_y_scroll()
        .child(
            Label::new("资源管理器")
                .size(LabelSize::Small)
                .color(ui::Color::Muted)
                .mb(HEADER_MARGIN),
        )
        .child(tree_group(
            "tg-docs",
            "我的文档",
            true,
            &[("标书 v3.md", true), ("会议纪要.md", false)],
            hover,
            colors.element_selected,
        ))
        .child(tree_group(
            "tg-import",
            "导入",
            false,
            &[],
            hover,
            colors.element_selected,
        ))
        .child(tree_group(
            "tg-export",
            "导出",
            false,
            &[],
            hover,
            colors.element_selected,
        ))
}

/// 右栏：属性 / 来源骨架（静态键值行）。
pub fn right_panel(cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    v_flex()
        .id("right-panel")
        .w(RIGHT_PANEL_WIDTH)
        .flex_none()
        .h_full()
        .min_w_0()
        .bg(colors.panel_background)
        .p(PANEL_PADDING)
        .gap_2()
        .overflow_y_scroll()
        .child(
            Label::new("属性 / 来源")
                .size(LabelSize::Small)
                .color(ui::Color::Muted)
                .mb(HEADER_MARGIN),
        )
        .child(property_row("来源", "本地文件"))
        .child(property_row("引擎", "mdcore 0.1.0"))
        .child(property_row("语言", "中文"))
        .child(property_row("字数", "—"))
        .child(property_row("修改时间", "—"))
        .child(
            Label::new("骨架占位——文档属性接线随 mdcore 引擎面引入")
                .size(LabelSize::XSmall)
                .color(ui::Color::Hidden),
        )
}

/// 文件树分组：折叠箭头 + 文件夹图标 + 组名，展开时子行缩进。
fn tree_group(
    id: &'static str,
    name: &'static str,
    expanded: bool,
    children: &[(&'static str, bool)],
    hover_bg: Hsla,
    selected_bg: Hsla,
) -> impl IntoElement {
    let (chevron, folder) = if expanded {
        (IconName::ChevronDown, IconName::FolderOpen)
    } else {
        (IconName::ChevronRight, IconName::Folder)
    };
    v_flex()
        .id(id)
        .gap_1()
        .child(
            h_flex()
                .id(SharedString::from(format!("{id}-head")))
                .gap_1()
                .rounded_sm()
                .hover(move |el| el.bg(hover_bg))
                .child(
                    Icon::new(chevron)
                        .size(IconSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(
                    Icon::new(folder)
                        .size(IconSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(Label::new(name).size(LabelSize::Small)),
        )
        .when(expanded, |el| {
            el.children(children.iter().map(|(child, active)| {
                h_flex()
                    .id(SharedString::from(format!("{id}-{child}")))
                    .pl_5()
                    .gap_1()
                    .rounded_sm()
                    .when(*active, |el| el.bg(selected_bg))
                    .hover(move |el| el.bg(hover_bg))
                    .child(
                        Icon::new(IconName::File)
                            .size(IconSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .child(Label::new(*child).size(LabelSize::Small))
            }))
        })
}

/// 属性键值行：键（muted）— 值（默认色），两端对齐。
fn property_row(key: &'static str, value: &'static str) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("prop-{key}")))
        .w_full()
        .justify_between()
        .child(
            Label::new(key)
                .size(LabelSize::Small)
                .color(ui::Color::Muted),
        )
        .child(Label::new(value).size(LabelSize::Small))
}
