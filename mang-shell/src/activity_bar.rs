//! 左侧图标条（#63 Lead：图标条 48px 宽 + 间距修正「每个按钮 36×36、
//! 间距 4px、外边距 8px」）。
//!
//! 实宽取 52px = 8 + 36 + 8——间距要求是 Lead 对「图标条内边距太挤」的
//! 修正项，优先于布局草图的 48px 字面值（36 + 2×8 > 48，两者不可同时
//! 满足；按钮本体 36×36 与 8px 边距为硬规格，宽度取其和）。
//!
//! 主面板钮（文件树/搜索）在上，右栏开关沉底。无 AI 面板——
//! agent 能力全部走 MCP 暴露（#63 Lead 裁决），不比 Zed/VS Code 照抄。
//!
//! #165：搜索钮接线 `Cmd::OpenFind`（与 Ctrl+F / 右键菜单「查找」同一
//! 分派路径 `Editor::run_command`——不另造入口）。设置钮已移除：全仓
//! 无设置面板 UI（settings.rs 是持久化 + 字体/主题 provider，无入口可
//! 接；file_tree.rs 的忽略名单注释也写明「可配置面随 F6 设置件落位」）。
//! 按 #165 铁律「接不了就从界面移除」执行；设置面板建好后钮再回来。
//!
//! 按钮为自绘单元而非 `IconButton`：后者方形尺寸由 IconSize 的 rem 机器
//! 推导（icon + 2×padding），无法钉定 36×36 字面规格；自绘单元同时掌控
//! hover/selected 主题色与精确几何。a11y（键盘焦点/读屏）记为后续 PR。

use gpui::{App, ClickEvent, Context, Hsla, Pixels, Window, px};
use ui::prelude::*;
use ui::{Color, Icon, IconName, IconSize, Tooltip};

use crate::editor::commands::Cmd;
use crate::shell::ShellApp;

/// 图标条实宽（px）：8 边距 + 36 按钮 + 8 边距。
pub const ACTIVITY_BAR_WIDTH: Pixels = px(52.);
/// 按钮边长（px，Lead 硬规格）。
pub const BUTTON_SIZE: Pixels = px(36.);
/// 按钮纵向间距（px）。
const BUTTON_GAP: Pixels = px(4.);
/// 图标条四周内边距（px）。
const BAR_PADDING: Pixels = px(8.);

/// 渲染图标条。`left_open`/`right_open` 决定选中态与右栏开关钮图标方向。
pub fn activity_bar(
    left_open: bool,
    git_view: bool,
    right_open: bool,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    v_flex()
        .id("activity-bar")
        .w(ACTIVITY_BAR_WIDTH)
        .flex_none()
        .h_full()
        .bg(colors.panel_background)
        .p(BAR_PADDING)
        .gap(BUTTON_GAP)
        .child(bar_button(
            "act-file-tree",
            IconName::FileTree,
            left_open,
            "文件树",
            colors.element_hover,
            colors.element_selected,
            cx.listener(|this, _, _, cx| this.toggle_left(cx)),
        ))
        // F7：git 改动/历史视图（文件树钮与 git 钮互切视图）。
        .child(bar_button(
            "act-git",
            IconName::GitBranch,
            left_open && git_view,
            "改动 / 历史（git）",
            colors.element_hover,
            colors.element_selected,
            cx.listener(|this, _, _, cx| this.toggle_git(cx)),
        ))
        // #165：搜索钮 → Cmd::OpenFind（与 Ctrl+F 同链——Editor::run_command
        // 统一分派；无打开文档时与 Ctrl+F 同为无操作，行为一致不特殊化）。
        .child(bar_button(
            "act-search",
            IconName::MagnifyingGlass,
            false,
            "搜索（Ctrl+F）",
            colors.element_hover,
            colors.element_selected,
            cx.listener(|this, _, window, cx| {
                if let Some(editor) = this.editor().cloned() {
                    editor.update(cx, |ed, cx| ed.run_command(Cmd::OpenFind, window, cx));
                }
            }),
        ))
        .child(div().flex_1())
        .child({
            let icon = if right_open {
                IconName::ThreadsSidebarRightOpen
            } else {
                IconName::ThreadsSidebarRightClosed
            };
            bar_button(
                "act-toggle-right",
                icon,
                right_open,
                "属性 / 来源",
                colors.element_hover,
                colors.element_selected,
                cx.listener(|this, _, _, cx| this.toggle_right(cx)),
            )
        })
}

/// 36×36 图标按钮单元：hover/selected 主题色（色值由调用方从当前主题注入），
/// 图标 16px（IconSize::Medium），tooltip 走 ui::Tooltip。
#[allow(clippy::too_many_arguments)]
fn bar_button(
    id: &'static str,
    icon: IconName,
    selected: bool,
    tooltip: &'static str,
    hover_bg: Hsla,
    selected_bg: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(BUTTON_SIZE)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_md()
        .when(selected, |el| el.bg(selected_bg))
        .hover(|el| el.bg(hover_bg))
        .tooltip(Tooltip::text(tooltip))
        .child(
            Icon::new(icon)
                .size(IconSize::Medium)
                .when(selected, |el| el.color(Color::Selected)),
        )
        .on_click(on_click)
}
