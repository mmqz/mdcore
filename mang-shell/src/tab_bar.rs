//! 多标签（#68 阶段 F3）——标签栏 + 标签集纯逻辑。
//!
//! 分层：
//! - 纯逻辑（无 gpui 依赖，单测钉定）：[`TabItem`] 载体、
//!   [`active_after_close`]（关闭后邻位规则）、[`cycle_active`]（循环
//!   切换）、[`tab_label`]（同名消歧——文件名/工作区相对路径/父目录
//!   前缀三级）；
//! - 渲染：[`tab_bar`]（标题栏下方一条——文件名 + 未保存 ● +
//!   active 态 + 点击切换）。
//!
//! 快捷键（shell 级）：Ctrl+W 关闭当前（切邻位）；Ctrl+Tab /
//! Ctrl+Shift+Tab 循环切换。编辑器 `tab` 键臂已加 `!ctrl` 守卫、
//! 只读门禁放行 `tab + ctrl`（导航组合）——Ctrl+Tab 从编辑态冒泡。

use std::path::Path;

use gpui::{Context, Entity, MouseButton, px};
use ui::prelude::*;
use ui::{Label, LabelSize};

use crate::editor::Editor;
use crate::shell::ShellApp;

/// 一个标签（打开的文档）。
#[derive(Clone)]
pub struct TabItem {
    pub path: std::path::PathBuf,
    pub editor: Entity<Editor>,
}

/// 关闭 `active` 后的新 active 序（关闭前长度 `old_len`）：
/// 右邻优先（被关位的后继前移补位）；关末位回退左邻；关唯一标签 → 0
/// （空态）。VS Code 同款惯例。
pub fn active_after_close(old_len: usize, active: usize) -> usize {
    if old_len <= 1 {
        0
    } else if active + 1 < old_len {
        active
    } else {
        active - 1
    }
}

/// 循环切换（wrap）：forward = Ctrl+Tab 下一；backward = Ctrl+Shift+Tab
/// 上一。空集/单标签返回原值（调用方以此短路）。
pub fn cycle_active(len: usize, active: usize, forward: bool) -> usize {
    if len < 2 {
        return active;
    }
    (active + if forward { 1 } else { len - 1 }) % len
}

/// 标签文案（同名消歧三级）：
/// 1. 无同名 → 文件名；
/// 2. 同名且在工作区内 → 工作区相对路径（`sub/a.md`——相对路径含父
///    信息即消歧）；
/// 3. 其余（同名且不在工作区/相对路径无父信息）→ `父目录/文件名`。
pub fn tab_label(path: &Path, all: &[&Path], workspace: Option<&Path>) -> String {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return path.display().to_string();
    };
    let duplicated = all
        .iter()
        .any(|p| p.as_os_str() != path.as_os_str() && p.file_name() == path.file_name());
    if !duplicated {
        return name;
    }
    if let Some(ws) = workspace
        && let Ok(rel) = path.strip_prefix(ws)
        && rel.parent().is_some_and(|p| !p.as_os_str().is_empty())
    {
        return rel.display().to_string();
    }
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    }
}

/// 标签栏（标题栏下方一条，仅在有标签时渲染）。
pub fn tab_bar(
    tabs: &[TabItem],
    active: usize,
    workspace: Option<&Path>,
    cx: &mut Context<ShellApp>,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let paths: Vec<&Path> = tabs.iter().map(|t| t.path.as_path()).collect();
    let rows: Vec<(String, bool)> = tabs
        .iter()
        .map(|t| {
            (
                tab_label(&t.path, &paths, workspace),
                t.editor.read(cx).is_modified(),
            )
        })
        .collect();

    h_flex()
        .id("tab-bar")
        .w_full()
        .h(px(30.))
        .flex_none()
        .bg(colors.title_bar_background)
        .px_1()
        .gap_1()
        .overflow_x_scroll()
        .children(rows.into_iter().enumerate().map(|(idx, (label, dirty))| {
            let is_active = idx == active;
            let selected = colors.element_selected;
            let hover = colors.element_hover;
            h_flex()
                .id(gpui::SharedString::from(format!("tab-{idx}")))
                .px_2()
                .h_full()
                .max_w(px(220.))
                .flex_none()
                .items_center()
                .gap_1()
                .rounded_sm()
                .when(is_active, move |el| el.bg(selected))
                .hover(move |el| el.bg(hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.activate_tab(idx, window, cx);
                    }),
                )
                .child(
                    Label::new(label)
                        .size(LabelSize::XSmall)
                        .color(if is_active {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                )
                .when(dirty, |el| {
                    el.child(
                        Label::new("●")
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Accent),
                    )
                })
        }))
}

// ---------------------------------------------------------------------------
// 纯逻辑单测。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// 关闭后邻位规则：右邻优先；末位回左邻；唯一标签归零。
    #[test]
    fn close_picks_right_neighbor_then_left() {
        // [A,B,C] 关 B(active 1) → C 前移补位，active 仍 1。
        assert_eq!(active_after_close(3, 1), 1);
        // 关末位 C(active 2) → 回退左邻 B → active 1。
        assert_eq!(active_after_close(3, 2), 1);
        // 关首位 A(active 0) → B 补位，active 0。
        assert_eq!(active_after_close(3, 0), 0);
        // 唯一标签 → 空态。
        assert_eq!(active_after_close(1, 0), 0);
        // 空集防御。
        assert_eq!(active_after_close(0, 0), 0);
    }

    /// 循环切换 wrap 语义。
    #[test]
    fn cycle_wraps_both_directions() {
        // 3 标签：0→1→2→0（forward）；0→2→1→0（backward）。
        assert_eq!(cycle_active(3, 0, true), 1);
        assert_eq!(cycle_active(3, 1, true), 2);
        assert_eq!(cycle_active(3, 2, true), 0, "末位 wrap 回首");
        assert_eq!(cycle_active(3, 0, false), 2, "首位 wrap 回末");
        assert_eq!(cycle_active(3, 2, false), 1);
        // 单标签/空集不变。
        assert_eq!(cycle_active(1, 0, true), 0);
        assert_eq!(cycle_active(0, 0, true), 0);
    }

    /// 同名消歧三级：无同名文件名 / 工作区相对 / 父目录前缀。
    #[test]
    fn label_disambiguates_duplicates() {
        let a = Path::new("/ws/sub/a.md");
        let b = Path::new("/ws/other/a.md");
        let c = Path::new("/elsewhere/a.md");
        let ws = Path::new("/ws");

        // 无同名 → 文件名。
        let solo = [a, Path::new("/ws/b.md")];
        assert_eq!(tab_label(a, &solo, Some(ws)), "a.md");

        // 同名且都在工作区内 → 相对路径。
        let pair = [a, b];
        assert_eq!(tab_label(a, &pair, Some(ws)), "sub/a.md");
        assert_eq!(tab_label(b, &pair, Some(ws)), "other/a.md");

        // 同名但不在工作区 → 父目录/文件名。
        let outside = [a, c];
        assert_eq!(tab_label(c, &outside, Some(ws)), "elsewhere/a.md");

        // 无工作区 → 父目录/文件名。
        assert_eq!(tab_label(a, &pair, None), "sub/a.md");
        // 工作区直下同名（rel 无父信息）→ 父目录兜底。
        let root_a = Path::new("/ws/a.md");
        let root_pair = [root_a, a];
        assert_eq!(tab_label(root_a, &root_pair, Some(ws)), "ws/a.md");
    }
}
