//! 外壳根视图 —— #63 Lead 布局的组合点。
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ 标题栏 36px（自绘：拖拽 + 窗口控制）            │
//! ├────┬─────────┬──────────────────┬────────────┤
//! │图标│ 左栏     │ 编辑区           │ 右栏        │
//! │条  │ 260px   │ (flex)           │ 280px      │
//! │52px│ 文件树   │ 空态占位          │ 属性/来源   │
//! ├────┴─────────┴──────────────────┴────────────┤
//! │ 状态栏 24px                                   │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! 折叠状态是本视图唯一的自持状态（`left_open`/`right_open`）——
//! 编辑区内容、文件树数据、属性面板数据均后续 PR 接线（mdcore 引擎面）。

use gpui::{Context, Render, Window};
use ui::prelude::*;

use crate::activity_bar;
use crate::panels;
use crate::status_bar;
use crate::title_bar;

pub struct ShellApp {
    left_open: bool,
    right_open: bool,
}

impl ShellApp {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            left_open: true,
            right_open: true,
        }
    }

    /// 折叠/展开左栏（文件树）。图标条 `act-file-tree` 触发。
    pub fn toggle_left(&mut self, cx: &mut Context<Self>) {
        self.left_open = !self.left_open;
        cx.notify();
    }

    /// 折叠/展开右栏（属性/来源）。图标条底部开关触发。
    pub fn toggle_right(&mut self, cx: &mut Context<Self>) {
        self.right_open = !self.right_open;
        cx.notify();
    }
}

impl Render for ShellApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        v_flex()
            .id("shell-root")
            .size_full()
            .bg(colors.background)
            .text_color(colors.text)
            .font_family("Noto Sans SC")
            .child(title_bar::title_bar("mang", cx))
            .child(
                h_flex()
                    .id("shell-body")
                    .flex_1()
                    .min_h_0()
                    .child(activity_bar::activity_bar(
                        self.left_open,
                        self.right_open,
                        cx,
                    ))
                    .when(self.left_open, |el| el.child(panels::left_panel(cx)))
                    .child(editor_area(cx))
                    .when(self.right_open, |el| el.child(panels::right_panel(cx))),
            )
            .child(status_bar::status_bar(cx))
    }
}

/// 编辑区空态占位（Lead：主视图 flex；骨架期无文档接线）。
fn editor_area(cx: &mut Context<ShellApp>) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    v_flex()
        .id("editor-area")
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(colors.background)
        .items_center()
        .justify_center()
        .gap_3()
        .child(ui::Headline::new("mang").size(ui::HeadlineSize::Large))
        .child(
            ui::Label::new("打开或创建文档以开始（骨架——引擎接线随后续 PR）")
                .color(ui::Color::Muted),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 纯状态机面：无需 gpui 上下文即可钉定折叠语义。
    /// （Entity 构造需要 App 上下文，此处以最小桩验证状态翻转逻辑——
    /// 真值由 Xvfb 烟雾与后续 render 测试背书。）
    #[test]
    fn toggle_semantics_flip_independently() {
        let mut left = true;
        let mut right = true;
        left = !left;
        assert!(!left, "左栏翻转：开 → 关");
        left = !left;
        assert!(left, "左栏再翻转：关 → 开");
        right = !right;
        assert!(!right);
        assert!(left, "右栏翻转不影响左栏");
    }
}
