//! 右缘 dock 条（#289-⑤）。
//!
//! ## 为什么存在
//!
//! 用户原话：「左侧边栏成了右侧按钮的详情菜单了这个很不好，不管是
//! vscode或者zed都没有这样设计」——旧版把右栏（属性/来源）的开关放在
//! **左**图标条底部，点左条按钮却开右侧面板，语义错位。
//!
//! ## 形态（zed 折叠 dock 同构）
//!
//! ```text
//! ┌─────────────────────────────┬──┐
//! │         编辑区              │⚙ │ ← 52px 竖条（与左图标条同宽
//! │                             │  │    同按钮规格——视觉同语系）
//! │                             │  │
//! │                             │☰ │ ← 沉底：属性/来源 开关
//! └─────────────────────────────┴──┘
//! ```
//!
//! - 恒在（右栏开/关都在）——右栏收起时按钮是唯一入口（zed 折叠 dock
//!   的既有形态），展开时按钮即选中态指示；
//! - 只承载右栏自己的事（当前 = 属性/来源单视图）；设置走左条底部
//!   的设置弹窗（[`crate::settings_panel`]），不占右栏。

use gpui::Context;
use ui::prelude::*;

use crate::activity_bar::{ACTIVITY_BAR_WIDTH, BAR_PADDING, bar_button};
use crate::shell::ShellApp;

/// 渲染右缘 dock 条。`right_open` 决定属性/来源钮的选中态与图标方向
/// （开 = ThreadsSidebarRightOpen / 关 = ThreadsSidebarRightClosed）。
pub fn right_dock_strip(right_open: bool, cx: &mut Context<ShellApp>) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let icon = if right_open {
        ui::IconName::ThreadsSidebarRightOpen
    } else {
        ui::IconName::ThreadsSidebarRightClosed
    };
    v_flex()
        .id("right-dock-strip")
        .w(ACTIVITY_BAR_WIDTH)
        .flex_none()
        .h_full()
        .bg(colors.panel_background)
        .p(BAR_PADDING)
        .child(div().flex_1())
        .child(bar_button(
            "dock-toggle-right",
            icon,
            right_open,
            "属性 / 来源",
            colors.element_hover,
            colors.element_selected,
            cx.listener(|this, _, _, cx| this.toggle_right(cx)),
        ))
}
