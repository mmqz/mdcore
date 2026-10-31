//! 底部状态栏（#63 Lead 布局 + #68 阶段 1 文档状态联动 + F2 工作区）。
//!
//! 左：文档状态（文件名 + 修改标记 ● + 行,列 + 字数——无文档时「就绪」）
//! + 工作区路径（F2——打开文件夹后显示，`~` 缩写/超长截前保尾）；
//! 右：版本与许可标识。背景色差（title_bar_background）区分主编辑区，
//! 无 1px 边框。

use gpui::{App, Entity, Pixels, px};
use ui::prelude::*;
use ui::{Label, LabelSize};

use crate::editor::Editor;
use crate::file_tree::workspace_display;

/// 状态栏高度（px）。
pub const STATUS_BAR_HEIGHT: Pixels = px(24.);

/// 渲染状态栏。`editor`：打开的编辑器实体（`None` = 无文档就绪态）；
/// `workspace`：工作区根（F2——`None` = 未打开）。
pub fn status_bar(
    editor: Option<&Entity<Editor>>,
    workspace: Option<std::path::PathBuf>,
    cx: &App,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let workspace_label = workspace
        .as_deref()
        .map(workspace_display)
        .map(|ws| format!("  ·  工作区 {ws}"));
    let left = match editor {
        Some(ed) => {
            let ed = ed.read(cx);
            let modified = if ed.is_read_only() {
                " · 只读"
            } else if ed.is_modified() {
                " ●"
            } else {
                ""
            };
            // 页码标签（批 4：pdf 态「第 N / M 页」——光标行经页表映射；
            // md/docx 态无页语义，不占位）。
            let page = ed
                .page_label()
                .map(|l| format!("  ·  {l}"))
                .unwrap_or_default();
            format!(
                "{}{modified}  ·  行, 列 {}{page}  ·  {}",
                ed.display_name(),
                ed.cursor_label(),
                ed.char_label()
            )
        }
        None => "就绪 —— Ctrl+O 打开文件 · Ctrl+K Ctrl+O 打开文件夹".to_string(),
    }
    .to_string();
    let left = match workspace_label {
        Some(ws) => format!("{left}{ws}"),
        None => left,
    };

    h_flex()
        .id("status-bar")
        .h(STATUS_BAR_HEIGHT)
        .flex_none()
        .w_full()
        .bg(colors.title_bar_background)
        .px_3()
        .justify_between()
        .child(
            Label::new(left)
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
        )
        .child(
            h_flex()
                .gap_3()
                .child(
                    Label::new("mdcore 0.1.0")
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(
                    Label::new("GPL-3.0-or-later")
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                ),
        )
}
