//! 右键菜单渲染层（#63 打磨项）——数据与几何在 [`crate::context_menu`]。
//!
//! 本模块只做「把已算好的矩形画出来」，不做摆放判断：
//! 尺寸/翻转/夹紧全部由 `context_menu::ContextMenu::layout` 在点击那一刻
//! 算好（那里有单测），这里拿到的是一个已经保证在屏幕内的 [`Rect`]。
//!
//! 菜单是**浮层**：绝对定位 + 全屏透明遮罩承接「点空白处关闭」。
//! 遮罩必须在菜单之前渲染（先画的在下层），否则遮罩会盖住菜单吞掉点击。

use gpui::{App, Context, SharedString, px};
use ui::prelude::*;

use crate::context_menu::{ContextMenu, ITEM_HEIGHT, MENU_PADDING_Y, MenuItem, Rect as MenuRect};
use crate::shell::ShellApp;

/// 右键菜单浮层（返回 (遮罩, 菜单) 两件——遮罩先渲染）。
///
/// `rect` = 已由 `ContextMenu::layout` 算好的屏幕内矩形。
pub fn context_menu_overlay(
    menu: &ContextMenu,
    rect: MenuRect,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();

    // 遮罩：全屏透明，左键点一下即关菜单。
    let backdrop = div()
        .id("ctx-menu-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.close_context_menu(cx);
            }),
        )
        .on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(|this, _, _, cx| {
                this.close_context_menu(cx);
            }),
        );

    let mut list = div()
        .id("ctx-menu")
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.background)
        .shadow_lg()
        .py(px(MENU_PADDING_Y))
        .flex()
        .flex_col()
        .text_size(px(13.));

    for (i, item) in menu.items.iter().enumerate() {
        match item {
            MenuItem::Separator => {
                list = list.child(
                    div()
                        .id(SharedString::from(format!("ctx-sep-{i}")))
                        .h(px(1.))
                        .my(px(4.))
                        .mx_1()
                        .bg(colors.border),
                );
            }
            MenuItem::Action {
                id,
                label,
                shortcut,
                enabled,
            } => {
                let colors2 = colors.clone();
                let mut row = div()
                    .id(SharedString::from(format!("ctx-item-{id}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .h(px(ITEM_HEIGHT - 2.))
                    .px_2()
                    .mx_1()
                    .rounded_sm()
                    .when(*enabled, |el| {
                        el.cursor_pointer().hover(|e| e.bg(colors2.element_hover))
                    });
                let text_color = if *enabled {
                    colors.text
                } else {
                    colors.text_muted
                };
                row = row.child(div().text_color(text_color).child(label.clone()));
                row = row.child(
                    div()
                        .text_size(px(11.))
                        .text_color(colors.text_muted)
                        .child(shortcut.clone().unwrap_or_default()),
                );
                if *enabled {
                    let action_id = *id;
                    row = row.on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_context_menu_item(action_id, window, cx);
                    }));
                }
                list = list.child(row);
            }
            MenuItem::Submenu { label, .. } => {
                // 本批只做一层菜单；子菜单项渲染为带箭头但不可点的提示行，
                // 不假装支持（诚实登记：真子菜单需要在 layout 层再算一次
                // place_submenu，接线留待后续）。
                list = list.child(
                    div()
                        .id(SharedString::from(format!("ctx-sub-{i}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .h(px(ITEM_HEIGHT - 2.))
                        .px_2()
                        .mx_1()
                        .text_color(colors.text_muted)
                        .child(label.clone())
                        .child(div().text_size(px(11.)).child("›")),
                );
            }
        }
    }

    div()
        .id("ctx-menu-layer")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(backdrop)
        .child(list)
}

/// 供 `shell.rs` 引用的空上下文（避免未使用导入警告时占位）。
pub fn _unused(_: &App) {}
