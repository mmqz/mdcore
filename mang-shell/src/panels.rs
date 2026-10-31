//! 左右面板（#63 Lead：左栏文件树/大纲 260px 可折叠；右侧属性/来源
//! 280px 可折叠）。
//!
//! #68 阶段 1 接线：左栏文件树首组呈现当前打开文档（真实数据），
//! docx 文档第二组呈现「文档结构」标题树（批 3——层级缩进 + 点击
//! 跳转，#68 阶段 3.1 docx 面提前落地）；pdf 同组呈现书签树（批 4
//! ——页号前缀 + 点击跳页分隔行）；磁盘目录树随阶段 2 接线。
//! 右栏属性面板：md 态文件名/字数联动；docx 态引擎实测属性面
//! （块/段/表/图/页眉页脚/批注/警告——批 3）；pdf 态引擎实测属性面
//! （页/字符/书签/扫描件——批 4）。面板边框一律背景色差（Lead：
//! 用背景色差代替 1px 边框）。

use gpui::{App, Entity, Hsla, MouseButton, Pixels, px};
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

use crate::editor::Editor;
use crate::file_tree::FileTree;
use crate::git_panel::GitPanel;

/// 左栏宽度（px）。
pub const LEFT_PANEL_WIDTH: Pixels = px(260.);
/// 右栏宽度（px）。
pub const RIGHT_PANEL_WIDTH: Pixels = px(280.);
/// 面板主内边距（px，Lead：主要区域 12-16px）。
const PANEL_PADDING: Pixels = px(12.);
/// 面板头部下缘留白（px）。
const HEADER_MARGIN: Pixels = px(8.);

/// 左栏：工作区文件树（F2——真实目录懒加载/虚拟化，占剩余高度）+
/// 首组「打开的文档」+ docx/pdf「文档结构」树。
pub fn left_panel(
    editor: Option<&Entity<Editor>>,
    workspace: Option<&Entity<FileTree>>,
    git_panel: Option<&Entity<GitPanel>>,
    git_view: bool,
    cx: &App,
) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let hover = colors.element_hover;
    let open_docs: Vec<(String, bool)> = match editor {
        Some(ed) => vec![(ed.read(cx).display_name().to_string(), true)],
        None => vec![],
    };
    let open_docs: Vec<(&str, bool)> = open_docs.iter().map(|(n, a)| (n.as_str(), *a)).collect();
    // 结构树（docx = 标题树 / pdf = 书签树；标签带 Hn/页号前缀，
    // 点击跳转锚点携带行号）。
    let headings: Vec<(String, usize, usize)> = match editor {
        Some(ed) => {
            let e = ed.read(cx);
            e.headings()
                .iter()
                .map(|h| (e.heading_label(h), h.level, h.line))
                .collect()
        }
        None => vec![],
    };
    v_flex()
        .id("left-panel")
        .w(LEFT_PANEL_WIDTH)
        .flex_none()
        .h_full()
        .min_w_0()
        .bg(colors.panel_background)
        .p(PANEL_PADDING)
        .gap_1()
        // F2：工作区文件树占剩余高度（uniform_list 内部滚动/虚拟化）
        // ——移除面板级 overflow_y_scroll 防双层滚动互踩；无工作区时
        // 自然高度内容（打开的文档/结构树）不溢出。
        .child(
            Label::new("资源管理器")
                .size(LabelSize::Small)
                .color(ui::Color::Muted)
                .mb(HEADER_MARGIN),
        )
        .when_some(workspace, |el, tree| el.child(tree.clone()))
        .child(tree_group(
            "tg-docs",
            "打开的文档",
            true,
            &open_docs,
            hover,
            colors.element_selected,
        ))
        .when_some((!headings.is_empty()).then_some(()), |el, _| {
            el.child(heading_tree("tg-outline", &headings, editor, hover))
        })
        .child(tree_group(
            "tg-import",
            "导入（阶段 2 接线）",
            false,
            &[],
            hover,
            colors.element_selected,
        ))
        .child(tree_group(
            "tg-export",
            "导出（阶段 5 接线）",
            false,
            &[],
            hover,
            colors.element_selected,
        ))
}

/// 结构树（批 3 docx 标题树 + 批 4 pdf 书签树）：层级缩进 +
/// 点击跳转（光标落条目行首 + 滚入可视区顶部）。
fn heading_tree(
    id: &'static str,
    headings: &[(String, usize, usize)],
    editor: Option<&Entity<Editor>>,
    hover_bg: Hsla,
) -> impl IntoElement {
    v_flex()
        .id(id)
        .gap_1()
        .child(
            h_flex()
                .id(SharedString::from(format!("{id}-head")))
                .gap_1()
                .rounded_sm()
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size(IconSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(
                    Icon::new(IconName::ListTree)
                        .size(IconSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(Label::new("文档结构").size(LabelSize::Small)),
        )
        .children(
            headings
                .iter()
                .enumerate()
                .map(|(i, (label, level, line))| {
                    let editor = editor.cloned();
                    let line = *line;
                    h_flex()
                        .id(SharedString::from(format!("{id}-item-{i}")))
                        // 层级缩进：每级 12px（1 级不缩进——条目已带 Hn/页号前缀）。
                        .pl(px(12. + (level.saturating_sub(1)) as f32 * 12.))
                        .gap_1()
                        .rounded_sm()
                        .hover(move |el| el.bg(hover_bg))
                        .when_some(editor, |el, ed| {
                            el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                ed.update(cx, |ed, _| ed.jump_to_line(line));
                                // 点击后面板焦点不影响编辑区键盘域；防窗口默认行为。
                                window.prevent_default();
                            })
                        })
                        .child(Label::new(label.as_str()).size(LabelSize::Small))
                }),
        )
}

/// 右栏：属性 / 来源（md 态：文件名/字数联动；docx 态：引擎实测
/// 属性面——批 3；pdf 态：页/字符/书签/扫描件——批 4）。
pub fn right_panel(editor: Option<&Entity<Editor>>, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    let (name, chars, modified, mode, engine) = match editor {
        Some(ed) => {
            let ed = ed.read(cx);
            (
                ed.display_name().to_string(),
                ed.char_label(),
                if ed.is_read_only() {
                    "只读渲染".to_string()
                } else if ed.is_modified() {
                    "已修改（未保存）".to_string()
                } else {
                    "无未保存修改".to_string()
                },
                match ed.doc_kind() {
                    "pdf" => "只读（pdf 文本层 → 行模型）".to_string(),
                    "docx" => "只读（docx 块序 → 行模型）".to_string(),
                    _ => "源文本（md）".to_string(),
                },
                match ed.doc_kind() {
                    "pdf" => "mdc-pdf（pdfium chromium/8086 non-V8）".to_string(),
                    "docx" => "mdc-office-io（docx-rs）".to_string(),
                    _ => "mdcore 0.1.0".to_string(),
                },
            )
        }
        None => (
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
        ),
    };
    // docx 实测属性面（批 3）。
    let docx_rows: Vec<(String, String)> = match editor {
        Some(ed) => ed
            .read(cx)
            .docx_props()
            .map(|p| {
                vec![
                    (
                        "块 / 段落".into(),
                        format!("{} / {}", p.blocks, p.paragraphs),
                    ),
                    (
                        "表格 / 行".into(),
                        format!("{} / {}", p.tables, p.table_rows),
                    ),
                    ("图片".into(), format!("{}（不入行流）", p.images)),
                    (
                        "页眉 / 页脚".into(),
                        format!("{} / {}（不入行流）", p.headers, p.footers),
                    ),
                    (
                        "批注 / 脚注 / 尾注".into(),
                        format!("{} / {} / {}", p.comments, p.footnotes, p.endnotes),
                    ),
                    ("文本框".into(), format!("{}（平铺渲染）", p.textboxes)),
                    ("兼容警告".into(), format!("{}", p.warnings)),
                ]
            })
            .unwrap_or_default(),
        None => vec![],
    };
    // pdf 实测属性面（批 4）。
    let pdf_rows: Vec<(String, String)> = match editor {
        Some(ed) => ed
            .read(cx)
            .pdf_props()
            .map(|p| {
                vec![
                    ("页数".into(), format!("{}", p.pages)),
                    ("可提取字符".into(), format!("{}（非空白）", p.chars)),
                    ("内容行".into(), format!("{}", p.content_lines)),
                    ("无文本页".into(), format!("{}", p.empty_pages)),
                    ("书签".into(), format!("{}", p.bookmarks)),
                    (
                        "扫描件".into(),
                        if p.scanned {
                            "是（全文无文本层）".to_string()
                        } else {
                            "否".to_string()
                        },
                    ),
                ]
            })
            .unwrap_or_default(),
        None => vec![],
    };
    v_flex()
        .id("right-panel")
        .w(RIGHT_PANEL_WIDTH)
        .flex_none()
        .h_full()
        .min_w_0()
        .bg(colors.panel_background)
        .p(PANEL_PADDING)
        .gap_2()
        .overflow_y_scroll()
        .child(
            Label::new("属性 / 来源")
                .size(LabelSize::Small)
                .color(ui::Color::Muted)
                .mb(HEADER_MARGIN),
        )
        .child(property_row("文件", &name))
        .child(property_row("字数", &chars))
        .child(property_row("状态", &modified))
        .child(property_row("引擎", &engine))
        .child(property_row("编辑器", &mode))
        .children(docx_rows.iter().map(|(k, v)| {
            let k = k.clone();
            let v = v.clone();
            property_row_owned(k, v)
        }))
        .children(pdf_rows.iter().map(|(k, v)| {
            let k = k.clone();
            let v = v.clone();
            property_row_owned(k, v)
        }))
        .child(
            Label::new("块级锚定 / 边注 / 证据来源随 #68 阶段 4 接线")
                .size(LabelSize::XSmall)
                .color(ui::Color::Hidden),
        )
}

/// 属性键值行（_owned 变体——docx 行动态 String 键；视觉与
/// [`property_row`] 恒同）。
fn property_row_owned(key: String, value: String) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("prop-{key}")))
        .w_full()
        .justify_between()
        .child(
            Label::new(key)
                .size(LabelSize::Small)
                .color(ui::Color::Muted),
        )
        .child(Label::new(value).size(LabelSize::Small))
}

/// 文件树分组：折叠箭头 + 文件夹图标 + 组名，展开时子行缩进。
fn tree_group(
    id: &'static str,
    name: &'static str,
    expanded: bool,
    children: &[(&str, bool)],
    hover_bg: Hsla,
    selected_bg: Hsla,
) -> impl IntoElement {
    let (chevron, folder) = if expanded {
        (IconName::ChevronDown, IconName::FolderOpen)
    } else {
        (IconName::ChevronRight, IconName::Folder)
    };
    v_flex()
        .id(id)
        .gap_1()
        .child(
            h_flex()
                .id(SharedString::from(format!("{id}-head")))
                .gap_1()
                .rounded_sm()
                .hover(move |el| el.bg(hover_bg))
                .child(
                    Icon::new(chevron)
                        .size(IconSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(
                    Icon::new(folder)
                        .size(IconSize::XSmall)
                        .color(ui::Color::Muted),
                )
                .child(Label::new(name).size(LabelSize::Small)),
        )
        .when(expanded, |el| {
            el.children(children.iter().map(|(child, active)| {
                h_flex()
                    .id(SharedString::from(format!("{id}-{child}")))
                    .pl_5()
                    .gap_1()
                    .rounded_sm()
                    .when(*active, |el| el.bg(selected_bg))
                    .hover(move |el| el.bg(hover_bg))
                    .child(
                        Icon::new(IconName::File)
                            .size(IconSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .child(Label::new(*child).size(LabelSize::Small))
            }))
        })
}

/// 属性键值行：键（muted）— 值（默认色），两端对齐。
fn property_row(key: &str, value: &str) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("prop-{key}")))
        .w_full()
        .justify_between()
        .child(
            Label::new(key)
                .size(LabelSize::Small)
                .color(ui::Color::Muted),
        )
        .child(Label::new(value).size(LabelSize::Small))
}
