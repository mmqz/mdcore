//! 左右面板（#63 Lead：左栏文件树/大纲 260px 可折叠；右侧属性/来源
//! 280px 可折叠）。
//!
//! #68 阶段 1 接线：左栏文件树首组呈现当前打开文档（真实数据），
//! 磁盘目录树与大纲面板随阶段 2/3 接线；右栏属性面板的文件名/字数
//! 已联动，其余键值行仍为骨架占位（mdcore 引擎面后续批）。
//! 面板边框一律背景色差（Lead：用背景色差代替 1px 边框）。

use gpui::{App, Entity, Hsla, Pixels, px};
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

use crate::editor::Editor;

/// 左栏宽度（px）。
pub const LEFT_PANEL_WIDTH: Pixels = px(260.);
/// 右栏宽度（px）。
pub const RIGHT_PANEL_WIDTH: Pixels = px(280.);
/// 面板主内边距（px，Lead：主要区域 12-16px）。
const PANEL_PADDING: Pixels = px(12.);
/// 面板头部下缘留白（px）。
const HEADER_MARGIN: Pixels = px(8.);

/// 左栏：文件树（首组 = 当前打开文档；磁盘目录树随阶段 2 接线）。
pub fn left_panel(editor: Option<&Entity<Editor>>, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let hover = colors.element_hover;
    let open_docs: Vec<(String, bool)> = match editor {
        Some(ed) => vec![(ed.read(cx).display_name().to_string(), true)],
        None => vec![],
    };
    let open_docs: Vec<(&str, bool)> = open_docs.iter().map(|(n, a)| (n.as_str(), *a)).collect();
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
            "打开的文档",
            true,
            &open_docs,
            hover,
            colors.element_selected,
        ))
        .child(tree_group(
            "tg-import",
            "导入（阶段 2 接线）",
            false,
            &[],
            hover,
            colors.element_selected,
        ))
        .child(tree_group(
            "tg-export",
            "导出（阶段 5 接线）",
            false,
            &[],
            hover,
            colors.element_selected,
        ))
}

/// 右栏：属性 / 来源（文件名/字数联动；其余骨架占位）。
pub fn right_panel(editor: Option<&Entity<Editor>>, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let (name, chars, modified) = match editor {
        Some(ed) => {
            let ed = ed.read(cx);
            (
                ed.display_name().to_string(),
                ed.char_label(),
                if ed.is_modified() {
                    "已修改（未保存）".to_string()
                } else {
                    "无未保存修改".to_string()
                },
            )
        }
        None => ("—".to_string(), "—".to_string(), "—".to_string()),
    };
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
        .child(property_row("文件", &name))
        .child(property_row("字数", &chars))
        .child(property_row("修改状态", &modified))
        .child(property_row("引擎", "mdcore 0.1.0"))
        .child(property_row("编辑器", "源文本（md）"))
        .child(
            Label::new("块级锚定 / 边注 / 证据来源随 #68 阶段 4 接线")
                .size(LabelSize::XSmall)
                .color(ui::Color::Hidden),
        )
}

/// 文件树分组：折叠箭头 + 文件夹图标 + 组名，展开时子行缩进。
fn tree_group(
    id: &'static str,
    name: &'static str,
    expanded: bool,
    children: &[(&str, bool)],
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
fn property_row(key: &str, value: &str) -> impl IntoElement {
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
