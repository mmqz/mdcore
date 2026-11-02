//! 设置面板（#182——纠正 #165/#172 的「移除设置钮」裁决）。
//!
//! ## 定位
//!
//! `settings.rs` 8 类存取早已全实现、`theme_switch.rs` 主题三态全套
//! 已实现——缺的只是一个 UI 壳。本模块就是这个壳：**只消费现有 API，
//! 零新增存储格式字段**（铁律 1）：
//!
//! | 项 | 控件 | 数据来源 / 生效链 |
//! |---|---|---|
//! | 主题 | 三态钮（跟随系统/浅色/深色） | `theme_switch::apply_mode` + `save_theme_mode`（`ShellApp::set_theme_mode`） |
//! | 字号 | −/值/+（8-32 步进 1） | `ShellApp::apply_font_size`（与 Ctrl+=/- 同一条链——立即应用到全部标签 + 持久化） |
//! | 缩进单位 | 三态钮（4 空格/2 空格/Tab） | `save_indent_unit` + 全部标签 `Editor::set_indent_unit`（`ShellApp::set_indent_unit_all`） |
//! | 面板宽度 | 「重置默认」钮 | `save_panel_widths` + 运行时宽度恢复（`ShellApp::reset_panel_widths`） |
//!
//! ## 形态裁决
//!
//! 占右栏（与属性/来源面板互斥的视图，非浮层）：#68 布局规格「右栏
//! 280px 可折叠」现成可用，宽度拖拽热区（G4）与折叠开关一并继承，
//! 不另造浮层层级。入口 = 图标条设置钮（`activity_bar.rs` 恢复的
//! `act-settings`），再点收起（`ShellApp::toggle_settings`）。
//!
//! ## 写法参照
//!
//! `panels.rs` 的面板骨架（v_flex + 分组标题 + hover/selected 主题色）
//! 与 `activity_bar.rs` 的 `cx.listener` 接线模式。每个可点控件都有
//! 真实行为（铁律 4：不留空闭包——`wiring_integrity` 的空闭包扫描
//! 对本文件同样适用）。

use gpui::{App, ClickEvent, Context, Hsla, Pixels, Window, px};
use ui::prelude::*;
use ui::{Color, Icon, IconName, IconSize, Label, LabelSize, Tooltip};

use crate::editor::buffer::IndentUnit;
use crate::shell::ShellApp;
use crate::theme_switch;

/// 面板主内边距（px，与 panels.rs 同规格）。
const PANEL_PADDING: Pixels = px(12.);
/// 分组标题上缘留白（px）。
const GROUP_MARGIN: Pixels = px(8.);
/// 控件钮纵向高度（px）。
const CONTROL_H: Pixels = px(26.);
/// 「标签 + 控件」行的标签列宽（px）。
const LABEL_W: Pixels = px(64.);

/// 渲染设置面板（右栏视图——`ShellApp::settings_view` 为真时挂载）。
///
/// `theme_mode` / `font_size` / `indent_unit` 均为 ShellApp 运行时状态
/// （调用处传入），点击经 `cx.listener` 回写 ShellApp（单一状态源，
/// 面板自身零状态）。
pub fn settings_panel(
    theme_mode: &'static str,
    font_size: f32,
    indent_unit: IndentUnit,
    width: Pixels,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let hover = colors.element_hover;
    let selected = colors.element_selected;

    v_flex()
        .id("settings-panel")
        .w(width)
        .flex_none()
        .h_full()
        .min_w_0()
        .bg(colors.panel_background)
        .p(PANEL_PADDING)
        .gap_1()
        .child(
            Label::new("设置")
                .size(LabelSize::Small)
                .color(Color::Muted)
                .mb(GROUP_MARGIN),
        )
        // ── 外观 ───────────────────────────────────────────
        .child(group_header("sg-appearance", "外观"))
        .child(control_row(
            "sg-theme-row",
            "主题",
            h_flex()
                .id("sg-theme-choices")
                .gap_1()
                .child(choice_button(
                    "sg-theme-system",
                    "跟随系统",
                    theme_mode == theme_switch::MODE_SYSTEM,
                    hover,
                    selected,
                    cx.listener(|this, _, _, cx| {
                        this.set_theme_mode(theme_switch::MODE_SYSTEM, cx)
                    }),
                ))
                .child(choice_button(
                    "sg-theme-light",
                    "浅色",
                    theme_mode == theme_switch::MODE_LIGHT,
                    hover,
                    selected,
                    cx.listener(|this, _, _, cx| this.set_theme_mode(theme_switch::MODE_LIGHT, cx)),
                ))
                .child(choice_button(
                    "sg-theme-dark",
                    "深色",
                    theme_mode == theme_switch::MODE_DARK,
                    hover,
                    selected,
                    cx.listener(|this, _, _, cx| this.set_theme_mode(theme_switch::MODE_DARK, cx)),
                )),
        ))
        // ── 编辑器 ─────────────────────────────────────────
        .child(group_header("sg-editor", "编辑器"))
        .child(control_row(
            "sg-font-row",
            "字号",
            h_flex()
                .id("sg-font-stepper")
                .gap_1()
                .child(step_button(
                    "sg-font-dec",
                    IconName::SquareMinus,
                    "减小字号",
                    font_size > 8.,
                    hover,
                    cx.listener(move |this, _, _, cx| {
                        this.apply_font_size(px((font_size - 1.).max(8.)), cx);
                    }),
                ))
                .child(
                    div()
                        .id("sg-font-value")
                        .h(CONTROL_H)
                        .flex()
                        .items_center()
                        .px_2()
                        .child(
                            Label::new(format!("{font_size:.0} px"))
                                .size(LabelSize::Small)
                                .color(Color::Default),
                        ),
                )
                .child(step_button(
                    "sg-font-inc",
                    IconName::Plus,
                    "增大字号",
                    font_size < 32.,
                    hover,
                    cx.listener(move |this, _, _, cx| {
                        this.apply_font_size(px((font_size + 1.).min(32.)), cx);
                    }),
                )),
        ))
        .child(control_row(
            "sg-indent-row",
            "缩进单位",
            h_flex()
                .id("sg-indent-choices")
                .gap_1()
                .child(choice_button(
                    "sg-indent-4",
                    "4 空格",
                    indent_unit == IndentUnit::Spaces4,
                    hover,
                    selected,
                    cx.listener(|this, _, _, cx| this.set_indent_unit_all(IndentUnit::Spaces4, cx)),
                ))
                .child(choice_button(
                    "sg-indent-2",
                    "2 空格",
                    indent_unit == IndentUnit::Spaces2,
                    hover,
                    selected,
                    cx.listener(|this, _, _, cx| this.set_indent_unit_all(IndentUnit::Spaces2, cx)),
                ))
                .child(choice_button(
                    "sg-indent-tab",
                    "Tab",
                    indent_unit == IndentUnit::Tab,
                    hover,
                    selected,
                    cx.listener(|this, _, _, cx| this.set_indent_unit_all(IndentUnit::Tab, cx)),
                )),
        ))
        // ── 布局 ───────────────────────────────────────────
        .child(group_header("sg-layout", "布局"))
        .child(control_row(
            "sg-width-row",
            "面板宽度",
            action_button(
                "sg-width-reset",
                "重置默认",
                hover,
                cx.listener(|this, _, _, cx| this.reset_panel_widths(cx)),
            ),
        ))
}

/// 分组标题（外观/编辑器/布局——panels.rs 分组头同形态）。
fn group_header(id: &'static str, title: &str) -> impl IntoElement {
    h_flex().id(id).mt(GROUP_MARGIN).mb(px(4.)).child(
        Label::new(title.to_string())
            .size(LabelSize::Small)
            .color(Color::Muted),
    )
}

/// 「标签 + 控件」行（标签定宽对齐，控件靠左跟随）。
fn control_row(id: &'static str, label: &str, controls: impl IntoElement) -> impl IntoElement {
    h_flex()
        .id(id)
        .gap_2()
        .py_1()
        .child(
            div()
                .id(SharedString::from(format!("{id}-label")))
                .w(LABEL_W)
                .flex_none()
                .h(CONTROL_H)
                .flex()
                .items_center()
                .child(
                    Label::new(label.to_string())
                        .size(LabelSize::Small)
                        .color(Color::Default),
                ),
        )
        .child(controls)
}

/// 单选钮（选中 = element_selected 实底 + 文字 Selected 色；未选中
/// hover 浮起——`activity_bar.rs` 的 bar_button 同款视觉语系）。
fn choice_button(
    id: &'static str,
    label: &'static str,
    is_selected: bool,
    hover_bg: Hsla,
    selected_bg: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(CONTROL_H)
        .px_2()
        .flex()
        .items_center()
        .rounded_sm()
        .when(is_selected, |el| el.bg(selected_bg))
        .hover(|el| el.bg(hover_bg))
        .child(
            Label::new(label)
                .size(LabelSize::Small)
                .color(if is_selected {
                    Color::Selected
                } else {
                    Color::Default
                }),
        )
        .on_click(on_click)
}

/// 步进钮（−/+——`enabled = false`（域界）时灰显且不挂 on_click：
/// 点不到不算死按钮，与 context_menu_view 灰显项同口径）。
fn step_button(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    enabled: bool,
    hover_bg: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(CONTROL_H)
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .when(enabled, |el| {
            el.hover(|el| el.bg(hover_bg))
                .on_click(on_click)
                .child(Icon::new(icon).size(IconSize::XSmall))
                .tooltip(Tooltip::text(tooltip))
        })
        .when(!enabled, |el| {
            el.child(
                Icon::new(icon)
                    .size(IconSize::XSmall)
                    .color(Color::Disabled),
            )
        })
}

/// 动作钮（面板宽度重置等——choice_button 的单发形态：普通底、
/// hover 浮起、按下有真实行为）。
fn action_button(
    id: &'static str,
    label: &'static str,
    hover_bg: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(CONTROL_H)
        .px_2()
        .flex()
        .items_center()
        .rounded_sm()
        .hover(|el| el.bg(hover_bg))
        .child(
            Label::new(label)
                .size(LabelSize::Small)
                .color(Color::Default),
        )
        .on_click(on_click)
}
