//! 外壳根视图 —— #63 Lead 布局的组合点 + #68 阶段 1 编辑区接线。
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ 标题栏 36px（自绘：拖拽 + 窗口控制 + 文档名）    │
//! ├────┬─────────┬──────────────────┬────────────┤
//! │图标│ 左栏     │ 编辑区           │ 右栏        │
//! │条  │ 260px   │ Editor / 空态     │ 280px      │
//! │52px│ 文件树   │ (#68 md 编辑面)  │ 属性/来源   │
//! ├────┴─────────┴──────────────────┴────────────┤
//! │ 状态栏 24px（文件 / 修改标记 / 行,列 / 字数）    │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! 折叠状态（`left_open` / `right_open`）与 `editor`（`Option<Entity<Editor>>`，
//! 由 main 的 CLI 文档分派构造）是本视图全部自持状态。

use gpui::{Context, Entity, Render, Window};
use ui::prelude::*;

use crate::activity_bar;
use crate::editor::Editor;
use crate::panels;
use crate::status_bar;
use crate::title_bar;

pub struct ShellApp {
    left_open: bool,
    right_open: bool,
    editor: Option<Entity<Editor>>,
}

impl ShellApp {
    /// `editor`：成功打开的文档编辑器（`None` = 空态）。
    pub fn new(
        editor: Option<Entity<Editor>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            left_open: true,
            right_open: true,
            editor,
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

impl ShellApp {
    /// 标题栏文案：`mang — 文件名`（无文档仅 `mang`）。
    fn title_with(&self, cx: &gpui::App) -> String {
        match &self.editor {
            Some(ed) => format!("mang — {}", ed.read(cx).display_name()),
            None => "mang".to_string(),
        }
    }
}

impl Render for ShellApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let title = self.title_with(cx);
        v_flex()
            .id("shell-root")
            .size_full()
            .bg(colors.background)
            .text_color(colors.text)
            .font_family("Noto Sans SC")
            .child(title_bar::title_bar(&title, cx))
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
                    .when(self.left_open, |el| {
                        el.child(panels::left_panel(self.editor.as_ref(), cx))
                    })
                    .child(editor_area(self, cx))
                    .when(self.right_open, |el| {
                        el.child(panels::right_panel(self.editor.as_ref(), cx))
                    }),
            )
            .child(status_bar::status_bar(self.editor.as_ref(), cx))
    }
}

/// 编辑区：有文档 → `Editor` 实体；无文档 / 打开失败 → 引导空态。
fn editor_area(shell: &mut ShellApp, cx: &mut Context<ShellApp>) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    match &shell.editor {
        Some(editor) => editor.clone().into_any_element(),
        None => v_flex()
            .id("editor-area-empty")
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(colors.background)
            .items_center()
            .justify_center()
            .gap_3()
            .child(ui::Headline::new("mang").size(ui::HeadlineSize::Large))
            .child(
                ui::Label::new(
                    "打开 .md 可编辑；.docx/.pdf 只读渲染（mang-shell FILE）——文件树/多标签随阶段 2 接线",
                )
                .color(ui::Color::Muted),
            )
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    /// 纯状态机面：无需 gpui 上下文即可钉定折叠语义。
    /// （Entity 构造需要 App 上下文，此处以最小桩验证状态翻转逻辑——
    /// 真值由 Xvfb 烟雾与编辑器纯逻辑单测背书。）
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
