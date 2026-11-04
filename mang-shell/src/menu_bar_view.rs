//! 菜单栏渲染层（#209）——结构与导航在 [`crate::menu_bar`]（纯数据/纯
//! 状态机，带单测）；本模块只做「画出来 + 点击接线」。
//!
//! 形态严格循 `context_menu_view.rs` 的既有语系：
//! - **浮层**：绝对定位 + 透明遮罩承接「点外部关闭」；遮罩先画在菜单
//!   之下（后画的兄弟覆盖先画的）。差异一处：遮罩从标题栏**下缘**起
//!   （`top(TITLE_BAR_HEIGHT)`）——标题按钮保持可点，已开菜单时点
//!   其他标题**一键直达切换**（全屏遮罩会逼用户点两下）；
//! - **条目行**：勾选列 + 标签 + 右侧快捷键（11px muted）+ 置灰不可点
//!   （enabled 才挂 on_click——「点不到不算死按钮」，右键菜单/设置面板
//!   步进钮同口径）；
//! - **子菜单**：右侧「›」箭头 + 点击/Enter 展开，摆放复用
//!   `context_menu::place_submenu`（边缘翻转/夹紧——那套几何有暴力扫描
//!   测试，不重写第二份）。
//!
//! 本模块零状态：菜单开合/选中/锚点全部在 `ShellApp.menu_bar`
//!（`menu_bar::MenuBarState`），键盘处理在 shell 根 on_key_down
//!（模态焦点域 `menu_focus` 随浮层挂载——确认框 confirm_focus 同款）。

use gpui::{AnyElement, Context, FocusHandle, MouseButton, px};
use theme::ThemeColors;
use ui::prelude::*;

use crate::context_menu::{self, Rect as MenuRect};
use crate::menu_bar::{
    MenuBarState, MenuEntry, MenuItemSpec, TopMenu, row_y_offset, to_context_items,
};
use crate::shell::ShellApp;
use crate::title_bar::TITLE_BAR_HEIGHT;

/// 勾选列宽（px——主题/侧栏子菜单「✓」的定位列；空列占位保持标签
/// 左对齐恒定，勾选态切换不横跳）。
const CHECK_COL_W: f32 = 16.;

/// 标题栏里的菜单标题行（文件/编辑/查看/帮助——title_bar 左侧标题之后
/// 挂载；`state.open` 决定展开态高亮）。
pub fn menu_titles(
    menus: &[TopMenu],
    state: &MenuBarState,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let hover = colors.element_hover;
    let selected_bg = colors.element_selected;
    h_flex()
        .id("menu-bar-titles")
        .h_full()
        .flex_none()
        .items_center()
        .gap(px(crate::menu_bar::TITLES_GAP))
        .children(menus.iter().enumerate().map(|(idx, menu)| {
            let is_open = state.open == Some(idx);
            div()
                .id(SharedString::from(format!("menu-title-{}", menu.id)))
                .h_full()
                .flex()
                .items_center()
                .px(px(crate::menu_bar::TITLE_BTN_PAD_X))
                .rounded_sm()
                .text_size(px(13.))
                .when(is_open, move |el| el.bg(selected_bg))
                .hover(move |el| el.bg(hover))
                .child(div().text_color(colors.text).child(menu.label))
                // on_mouse_down（非 on_click）：按下瞬间捕获 event.position.x
                // 作下拉锚点（context_menu 右键定位同源——panel_drag 的
                // 命中取 x 同款用法）；已开同菜单 = 关闭，未开/别的菜单 =
                // 打开（一键切换，遮罩从标题栏下缘起、不挡标题按钮）。
                // 开关判定走 is_menu_bar_open_at 访问器（menu_bar 状态
                // 字段私有——跨模块不直取）。
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                        let x = f32::from(event.position.x);
                        if this.is_menu_bar_open_at(idx) {
                            this.close_menu_bar(window, cx);
                        } else {
                            this.open_menu_bar(idx, x, window, cx);
                        }
                    }),
                )
        }))
}

/// 菜单栏浮层（遮罩 + 下拉 + 子菜单——shell 渲染链尾挂载，后画在上层；
/// 仅在 `state.open` 时由 shell 调用，防御性处理未开/越界态）。
///
/// `screen` = 窗口客户区矩形（摆放翻转的边界域，shell 从 `window.bounds()`
/// 采集——与右键菜单同一采集点）。
pub fn menu_bar_overlay(
    menus: &[TopMenu],
    state: &MenuBarState,
    focus: &FocusHandle,
    screen: MenuRect,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let Some(open_idx) = state.open else {
        // 防御：未开态不该渲染（shell 仅在 open 时挂载）——空层兜底。
        return div().id("menu-bar-layer-none");
    };
    let Some(menu) = menus.get(open_idx) else {
        return div().id("menu-bar-layer-none");
    };

    // 遮罩：标题栏下缘起（标题按钮保持可点——一键切换菜单），左右键
    // 点一下即关。size_full 自窗口顶起算 + top(标题栏高) = 覆盖标题栏
    // 以下全部可见区（溢出窗口底部 36px 在窗外不可见不可点，等效从下缘
    // 铺到底——context_menu_view 遮罩的 size_full 同款）。焦点域挂遮罩
    // 上（确认框同款——键事件沿本层→shell 根冒泡，编辑器隔离；↑↓←→/
    // Enter/Esc 由 shell 根监听器统一处理）。
    let backdrop = div()
        .id("menu-bar-backdrop")
        .absolute()
        .top(TITLE_BAR_HEIGHT)
        .left_0()
        .size_full()
        .track_focus(focus)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.close_menu_bar(window, cx)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _, window, cx| this.close_menu_bar(window, cx)),
        );

    // 下拉摆放：复用 context_menu 的 place_menu（右越界左翻 / 下越界上翻
    // / 夹紧），锚点 = 标题点击 x（键盘切换时为估算重锚值），y = 标题栏
    // 下缘。宽度按 menu_size 估算（右键菜单同一口径）。
    let items = &menu.entries;
    let (w, h) = context_menu::menu_size(&to_context_items(items));
    let anchor = context_menu::Point::new(state.anchor_x, f32::from(TITLE_BAR_HEIGHT));
    let pos = context_menu::place_menu(anchor, (w, h), screen);
    let menu_rect = context_menu::Rect::new(pos.x, pos.y, w, h);

    let mut list = div()
        .id(SharedString::from(format!("menu-drop-{}", menu.id)))
        .absolute()
        .left(px(pos.x))
        .top(px(pos.y))
        .w(px(w))
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.background)
        .shadow_lg()
        .py(px(context_menu::MENU_PADDING_Y))
        .flex()
        .flex_col()
        .text_size(px(13.));

    // 子菜单面板（展开时才建；作为下拉的兄弟后画——盖在遮罩上。
    // into_any_element 类型擦除：面板链是 Stateful<Div>，与 shell.rs
    // 设置/属性面板二选一同款手法）。
    let mut submenu_layer: Option<AnyElement> = None;

    for (idx, entry) in items.iter().enumerate() {
        match entry {
            MenuEntry::Separator => {
                list = list.child(
                    div()
                        .id(SharedString::from(format!("menu-sep-{}-{idx}", menu.id)))
                        .h(px(1.))
                        .my(px(4.))
                        .mx_1()
                        .bg(colors.border),
                );
            }
            MenuEntry::Item(spec) => {
                // 键盘高亮：子菜单展开时顶层普通项不再高亮（焦点已入子层）。
                let highlight = state.open_submenu.is_none() && state.selected == idx;
                list = list.child(action_row(spec, highlight, &colors, cx));
            }
            MenuEntry::Submenu {
                label,
                items: sub_items,
            } => {
                // 父行高亮：键盘选中即高亮（含子菜单展开时——桌面菜单惯例
                // 父行保持高亮）。
                let highlight = state.selected == idx;
                list = list.child(submenu_parent_row(
                    menu.id, idx, label, highlight, &colors, cx,
                ));
                if state.open_submenu == Some(idx) {
                    // 子菜单摆放：place_submenu（右飞/左翻/夹紧），row_y =
                    // 父行在菜单内的偏移（含上内边距——见 menu_bar::row_y_offset）。
                    let (sw, sh) = context_menu::menu_size(&to_context_items(sub_items));
                    let row_y = row_y_offset(items, idx);
                    let sub_pos = context_menu::place_submenu(menu_rect, row_y, (sw, sh), screen);
                    let mut sub = div()
                        .id(SharedString::from(format!("menu-sub-{}-{idx}", menu.id)))
                        .absolute()
                        .left(px(sub_pos.x))
                        .top(px(sub_pos.y))
                        .w(px(sw))
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border)
                        .bg(colors.background)
                        .shadow_lg()
                        .py(px(context_menu::MENU_PADDING_Y))
                        .flex()
                        .flex_col()
                        .text_size(px(13.));
                    for (sub_idx, sub_entry) in sub_items.iter().enumerate() {
                        match sub_entry {
                            MenuEntry::Item(spec) => {
                                let highlight = state.submenu_selected == sub_idx;
                                sub = sub.child(action_row(spec, highlight, &colors, cx));
                            }
                            // 子菜单内不再嵌套子菜单（build_menus 结构保证）；
                            // 分隔线照画。
                            MenuEntry::Separator => {
                                sub = sub.child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "menu-sub-sep-{}-{idx}-{sub_idx}",
                                            menu.id
                                        )))
                                        .h(px(1.))
                                        .my(px(4.))
                                        .mx_1()
                                        .bg(colors.border),
                                );
                            }
                            MenuEntry::Submenu { .. } => {}
                        }
                    }
                    submenu_layer = Some(sub.into_any_element());
                }
            }
        }
    }

    div()
        .id("menu-bar-layer")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(backdrop)
        .child(list)
        .when_some(submenu_layer, |el, sub| el.child(sub))
}

/// 动作行：勾选列 + 标签（弹性）+ 右侧快捷键；置灰（enabled=false）
/// 只换 muted 色不挂 on_click——「点不到不算死按钮」，context_menu_view
/// 灰显项同口径。
fn action_row(
    spec: &MenuItemSpec,
    highlight: bool,
    colors: &ThemeColors,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let text_color = if spec.enabled {
        colors.text
    } else {
        colors.text_muted
    };
    let hover_bg = colors.element_hover;
    let mut row = div()
        .id(SharedString::from(format!("mb-{}", spec.id)))
        .flex()
        .items_center()
        .gap_2()
        .h(px(context_menu::ITEM_HEIGHT - 2.))
        .px_2()
        .mx_1()
        .rounded_sm()
        .when(spec.enabled, |el| {
            el.cursor_pointer().hover(move |e| e.bg(hover_bg))
        })
        .when(highlight, move |el| el.bg(hover_bg))
        // 勾选列（空列占位——勾选切换不横跳；「✓」与标签同色）。
        .child(
            div()
                .w(px(CHECK_COL_W))
                .flex()
                .items_center()
                .text_color(text_color)
                .child(if spec.checked { "✓" } else { "" }),
        )
        .child(
            div()
                .flex_1()
                .text_color(text_color)
                .child(spec.label.clone()),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(colors.text_muted)
                .child(spec.shortcut.unwrap_or("")),
        );
    if spec.enabled {
        let action = spec.action;
        row = row.on_click(cx.listener(move |this, _, window, cx| {
            this.activate_menu_action(action, window, cx);
        }));
    }
    row
}

/// 子菜单父行：右侧「›」箭头（context_menu_view 子菜单提示行同符号）；
/// 点击 = 开/转子菜单（键盘 Enter/→ 同效——`toggle_menu_submenu`）。
fn submenu_parent_row(
    menu_id: &'static str,
    idx: usize,
    label: &str,
    highlight: bool,
    colors: &ThemeColors,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let hover_bg = colors.element_hover;
    div()
        .id(SharedString::from(format!("mb-sub-{menu_id}-{idx}")))
        .flex()
        .items_center()
        .gap_2()
        .h(px(context_menu::ITEM_HEIGHT - 2.))
        .px_2()
        .mx_1()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |e| e.bg(hover_bg))
        .when(highlight, move |el| el.bg(hover_bg))
        // 勾选列占位（父行无勾选态——与动作行对齐）。
        .child(div().w(px(CHECK_COL_W)))
        .child(
            div()
                .flex_1()
                .text_color(colors.text)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(colors.text_muted)
                .child("›"),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.toggle_menu_submenu(idx, cx);
        }))
}
