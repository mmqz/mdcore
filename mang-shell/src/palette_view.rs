//! 命令面板的分组渲染（#63 打磨项）。
//!
//! 数据与顺序在 [`crate::command_groups`]（纯逻辑、有单测）；本模块只负责
//! 把 [`Section`] 画成「段标题 + 条目 + 右侧快捷键」。
//!
//! 面板本体（输入框、键处理）在 `editor/mod.rs`——那里持有 `PaletteState`。
//! 本模块提供 [`palette_sections`]，让编辑器在渲染时按分组取数据。

use gpui::{App, SharedString, px};
use ui::prelude::*;

use crate::command_groups::{Entry, Group, RecentRing, Section, build_sections, flatten};
use crate::editor::commands::{self, Cmd};

/// 最近使用容量（面板顶部固定区长度）。
pub const RECENT_CAPACITY: usize = 4;

/// 计算面板当前应显示的段（查询 → 分组）。
pub fn palette_sections(query: &str, recent: &RecentRing) -> Vec<Section> {
    let results = commands::search(query);
    build_sections(&results, recent)
}

/// 扁平命令序（键盘上下移动的遍历域）。
pub fn palette_flat(sections: &[Section]) -> Vec<Cmd> {
    flatten(sections)
}

/// 渲染面板列表区（段标题 + 行的绝对序）。
///
/// `selected` = 扁平序里的选中下标；`max_rows` 限制总行数（面板不无限长）。
pub fn palette_list(
    sections: &[Section],
    selected: usize,
    max_rows: usize,
    cx: &App,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let mut col = div().id("palette-sections").flex().flex_col().gap_1();

    let mut drawn = 0usize;
    for section in sections {
        if drawn >= max_rows {
            break;
        }
        // 段标题（最近使用 / 文件 / 编辑 / 导航 / 视图 / 帮助）
        col = col.child(
            div()
                .id(SharedString::from(format!("pal-sec-{:?}", section.group)))
                .px_2()
                .pt_1()
                .text_size(px(10.))
                .text_color(colors.text_muted)
                .child(section.group.title()),
        );
        for Entry { cmd, index } in &section.entries {
            if drawn >= max_rows {
                break;
            }
            drawn += 1;
            let is_sel = *index == selected;
            col = col.child(
                div()
                    .id(SharedString::from(format!("pal-row-{index}")))
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .h(px(22.))
                    .px_2()
                    .mx_1()
                    .rounded_sm()
                    .when(is_sel, |e| e.bg(colors.element_hover))
                    .child(div().text_color(colors.text).child(cmd.title()))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.text_muted)
                            .child(cmd.default_key().unwrap_or("")),
                    ),
            );
        }
    }

    if drawn == 0 {
        col = col.child(
            div()
                .px_2()
                .py_2()
                .text_size(px(12.))
                .text_color(colors.text_muted)
                .child("无匹配命令"),
        );
    }
    col
}

/// 段标题是否应显示（最近组为空时不显示"最近使用"标题）。
pub fn section_visible(group: Group, entries: &[Entry]) -> bool {
    if group == Group::Recent {
        return !entries.is_empty();
    }
    !entries.is_empty()
}
