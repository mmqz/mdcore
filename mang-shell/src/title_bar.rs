//! 自绘标题栏（#63 Lead：顶部标题栏，可拖拽窗口；#209：左侧标题后
//! 挂菜单栏）。
//!
//! 结构：`[标题 16px 边距] [菜单栏 文件/编辑/查看/帮助] [flex 拖拽区]
//! [窗口控制钮 3 个]`。菜单栏渲染在 [`crate::menu_bar_view`]（结构与
//! 状态在 `crate::menu_bar` / `ShellApp.menu_bar`）；窗口控制钮与拖拽
//! 语义本模块自持，#209 未改动。
//!
//! 拖拽的跨平台路径（#68 Lead Windows 真机验证后修正——原
//! `start_window_move()` 单路径在 Windows 失效）：
//! - **Windows**：拖拽区注册 `window_control_area(Drag)` 命中区——后端
//!   `WM_NCHITTEST` 返回 `HTCAPTION`，鼠标事件未被 GPUI 消费时
//!   `DefWindowProcW` 原生接管拖拽与双击（最大化 toggle）。两个关键事实：
//!   ① `start_window_move()` 在 gpui_windows 是 trait 默认空体（静默
//!   no-op，仅 Linux/macOS 有真实现）；② 任何消费鼠标事件的监听器都会
//!   阻止 DefWindowProc 接管——故 Windows 分支**不得**再挂
//!   on_mouse_down/on_click。
//! - **Linux**：`start_window_move()` 有真实现（X11 `_NET_WM_MOVERESIZE`
//!   / Wayland `xdg_toplevel._move()`）——保留显式调用。
//! - 双击最大化 `titlebar_double_click()` 仅 macOS 有实现（Linux/Windows
//!   均为 trait 默认空体）——不挂双击分支：Windows 由 HTCAPTION 双击
//!   原生接管，Linux 走 WM 装饰协议。
//!
//! 窗口控制走平台 API（`minimize_window` / `zoom_window` / 退出）——
//! 处理器闭包参数自带 `&mut Window`，无需捕获。`zoom_window()` 的
//! toggle 语义由 gpui_windows 补丁对齐三后端（见
//! third_party/zed/PROVENANCE.md「源码补丁登记」P1）。
//!
//! 注：X11 下客户端装饰（CSD）需合成器支持，无合成器时 gpui_linux 自动
//! 回退服务端装饰（WM 标题栏）——本栏恒在客户区内渲染，两种形态不塌。

use gpui::{Context, MouseButton, Pixels, px};
use ui::prelude::*;
use ui::{IconButton, IconName, Label, LabelSize};

use crate::menu_bar;
use crate::menu_bar_view;
use crate::shell::ShellApp;

/// 标题栏高度（px）。
pub const TITLE_BAR_HEIGHT: Pixels = px(36.);

/// 渲染标题栏。`menus` / `menu_state` = #209 菜单栏（结构与状态由
/// shell 提供——本模块只做布局挂载；`cx.listener` 需要 ShellApp 上下文，
/// activity_bar/tab_bar 同款签名口径）。
pub fn title_bar(
    title: &str,
    menus: &[menu_bar::TopMenu],
    menu_state: &menu_bar::MenuBarState,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    h_flex()
        .id("title-bar")
        .h(TITLE_BAR_HEIGHT)
        .flex_none()
        .w_full()
        .bg(colors.title_bar_background)
        // 标题（固定列——不参与拖拽；菜单栏紧随其后，见 issue #209 布局）。
        .child(
            h_flex()
                .id("title-bar-caption")
                .h_full()
                .flex_none()
                .items_center()
                .pl_4()
                .child(
                    Label::new(title)
                        .size(LabelSize::Small)
                        .color(ui::Color::Muted),
                ),
        )
        // #209：菜单栏（文件/编辑/查看/帮助——点击下拉；快捷键标签、
        // 键盘导航、灰显态见 menu_bar / menu_bar_view）。
        .child(menu_bar_view::menu_titles(menus, menu_state, cx))
        // 拖拽只挂中央弹性区——避免点控制钮/菜单标题时连带触发窗口移动。
        .child(
            div()
                .id("title-bar-drag")
                .flex_1()
                .h_full()
                // Windows：注册窗口控制命中区（Drag）→ WM_NCHITTEST 返回
                // HTCAPTION → GPUI 不消费时 DefWindowProcW 原生接管拖拽
                // 与双击最大化（gpui_windows/src/events.rs 既有链路）。
                // 不得再挂 on_mouse_down/on_click——消费即阻断
                // DefWindowProc（events.rs handle_nc_mouse_down_msg）。
                .when(cfg!(target_os = "windows"), |el| {
                    el.window_control_area(gpui::WindowControlArea::Drag)
                })
                // Linux/macOS：start_window_move 有真实现，保留显式调用。
                .when(cfg!(not(target_os = "windows")), |el| {
                    el.on_mouse_down(MouseButton::Left, |_, window, _| {
                        window.start_window_move();
                    })
                }),
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
