//! 设置弹窗（#182 立项；#289-⑤ 改形态为模态弹窗）。
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
//! ## 形态裁决（#289-⑤ 重裁）
//!
//! **模态弹窗**（zed「设置是独立界面不是 dock 视图」/ vscode「设置
//! 打开为独立窗格」的同款语义）：
//! - 旧版（#182）：占右栏、与属性/来源互斥——用户实测反馈「左侧边栏
//!   成了右侧按钮的详情菜单」（左图标条设置钮驱动右栏视图，语义
//!   错位）；
//! - 新版：`ShellApp::settings_open` 为真时渲染居中卡片 + 半透明遮罩
//!   （确认框 confirm_overlay 同款模态机制）；入口 = 左图标条底部
//!   设置钮 / 菜单栏「查看 → 设置」；Esc 或点遮罩关闭。
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

/// 弹窗卡片宽度（px）。
const MODAL_W: Pixels = px(400.);
/// 卡片主内边距（px，与 panels.rs 同规格）。
const PANEL_PADDING: Pixels = px(16.);
/// 分组标题上缘留白（px）。
const GROUP_MARGIN: Pixels = px(8.);
/// 控件钮纵向高度（px）。
const CONTROL_H: Pixels = px(26.);
/// 「标签 + 控件」行的标签列宽（px）。
const LABEL_W: Pixels = px(64.);

/// 渲染设置弹窗（`ShellApp::settings_open` 为真时由 shell 渲染链尾
/// 挂载——模态遮罩拦截外部点按，后画的卡片承接内部交互）。
///
/// `theme_mode` / `font_size` / `indent_unit` 均为 ShellApp 运行时状态
/// （调用处传入），点击经 `cx.listener` 回写 ShellApp（单一状态源，
/// 弹窗自身零状态）。
pub fn settings_modal(
    theme_mode: &'static str,
    font_size: f32,
    indent_unit: IndentUnit,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    div()
        .id("settings-modal-layer")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        // 半透明遮罩：点外关闭（弹窗内卡片是后画兄弟，点击不穿透）。
        .bg(gpui::black().opacity(0.35))
        .on_mouse_down(gpui::MouseButton::Left, {
            cx.listener(|this, _, _, cx| this.close_settings(cx))
        })
        .child(settings_card(
            theme_mode,
            font_size,
            indent_unit,
            &colors,
            cx,
        ))
}

/// 弹窗卡片（居中、固定宽、标题行 + 可滚动内容体）。
fn settings_card(
    theme_mode: &'static str,
    font_size: f32,
    indent_unit: IndentUnit,
    colors: &theme::ThemeColors,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let hover = colors.element_hover;
    let selected = colors.element_selected;
    v_flex()
        .id("settings-card")
        .w(MODAL_W)
        .max_h(px(520.))
        .min_h_0()
        .bg(colors.elevated_surface_background)
        .border_1()
        .border_color(colors.border)
        .rounded_md()
        .shadow_lg()
        .p(PANEL_PADDING)
        .gap_1()
        .overflow_y_scroll()
        // 标题行：「设置」+ 右侧关闭钮（与 Esc / 点遮罩同一条 close 链）。
        .child(
            h_flex()
                .id("settings-header")
                .w_full()
                .justify_between()
                .items_center()
                .mb(GROUP_MARGIN)
                .child(
                    Label::new("设置")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    div()
                        .id("settings-close")
                        .size(CONTROL_H)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .hover(|el| el.bg(hover))
                        .tooltip(Tooltip::text("关闭（Esc）"))
                        .child(Icon::new(IconName::Close).size(IconSize::XSmall))
                        .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx))),
                ),
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
            Label::new(label.to_string())
                .size(LabelSize::Small)
                .color(Color::Default),
        )
        .on_click(on_click)
}
