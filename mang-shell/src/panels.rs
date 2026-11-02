//! 左右面板（#63 Lead：左栏文件树/大纲 260px 可折叠；右侧属性/来源
//! 280px 可折叠）。
//!
//! #68 阶段 1 接线：左栏文件树首组呈现当前打开文档（真实数据），
//! docx 文档第二组呈现「文档结构」标题树（批 3——层级缩进 + 点击
//! 跳转，#68 阶段 3.1 docx 面提前落地）；pdf 同组呈现书签树（批 4
//! ——页号前缀 + 点击跳页分隔行）；磁盘目录树随 F2 工作区落位。
//! 右栏属性面板：md 态文件名/字数联动；docx 态引擎实测属性面
//! （块/段/表/图/页眉页脚/批注/警告——批 3）；pdf 态引擎实测属性面
//! （页/字符/书签/扫描件——批 4）。面板边框一律背景色差（Lead：
//! 用背景色差代替 1px 边框）。
//!
//! #166：原「导入（阶段 2 接线）」「导出（阶段 5 接线）」两个占位组
//! 已移除——零子行、零点击处理，括号里明写「阶段 N 接线」即有意留白。
//! 按 #166 判定表归档②（不该出现在界面）：真实导入/导出功能属标书
//! 业务面（#171 裁决：标书侧后端未迁移前不接入），等能力就位再随
//! 功能一起回来，不先摆死节点。

use gpui::{App, Entity, Hsla, MouseButton, Pixels, px};
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

use crate::editor::Editor;
use crate::file_tree::FileTree;
use crate::git_panel::GitPanel;

/// 左栏缺省宽度（px）——G4 起实际宽度是 ShellApp 状态（可拖拽，
/// settings `panel_widths` 键持久化；域 [`LEFT_PANEL_MIN`]-[`LEFT_PANEL_MAX`]）。
pub const LEFT_PANEL_WIDTH: Pixels = px(260.);
/// 右栏缺省宽度（px）。
pub const RIGHT_PANEL_WIDTH: Pixels = px(280.);
/// G4：左栏拖拽域（px）。
pub const LEFT_PANEL_MIN: Pixels = px(150.);
pub const LEFT_PANEL_MAX: Pixels = px(500.);
/// G4：右栏拖拽域（px）。
pub const RIGHT_PANEL_MIN: Pixels = px(150.);
pub const RIGHT_PANEL_MAX: Pixels = px(600.);
/// G4：面板边缘拖拽热区宽（px）。
pub const PANEL_RESIZE_HOTZONE: Pixels = px(4.);
/// 面板主内边距（px，Lead：主要区域 12-16px）。
const PANEL_PADDING: Pixels = px(12.);
/// 面板头部下缘留白（px）。
const HEADER_MARGIN: Pixels = px(8.);

/// G4：步进位移 → 宽度增量（左栏右移增宽 / 右栏左移增宽——纯函数，
/// 单测锚点）。
pub fn width_delta(left: bool, dx: f32) -> f32 {
    if left { dx } else { -dx }
}

/// G4：宽度域钳制（左栏 [150,500] / 右栏 [150,600]）。
pub fn clamp_panel_width(left: bool, width: f32) -> f32 {
    let (min, max) = if left {
        (f32::from(LEFT_PANEL_MIN), f32::from(LEFT_PANEL_MAX))
    } else {
        (f32::from(RIGHT_PANEL_MIN), f32::from(RIGHT_PANEL_MAX))
    };
    width.clamp(min, max)
}

/// 左栏：工作区文件树（F2——真实目录懒加载/虚拟化，占剩余高度）+
/// 首组「打开的文档」+ docx/pdf「文档结构」树。
///
/// #183 A-3（#188）：`git_view` 双视图路由——true 渲染 GitPanel 本体
/// （改动/历史双视图 + 刷新钮；图标条 act-git 入口），false 渲染文件树。
/// 此前形参收了 git_panel/git_view 却从不使用——左栏恒为文件树，
/// git_panel.rs 560 行本体不可见，属「已实现未接线」。
pub fn left_panel(
    editor: Option<&Entity<Editor>>,
    workspace: Option<&Entity<FileTree>>,
    git_panel: Option<&Entity<GitPanel>>,
    git_view: bool,
    width: Pixels,
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
        .w(width)
        .flex_none()
        .h_full()
        .min_w_0()
        .bg(colors.panel_background)
        .p(PANEL_PADDING)
        .gap_1()
        // 栏头随视图切换（VS Code 语义：资源管理器 / 源代码管理）。
        .child(
            Label::new(if git_view {
                "源代码管理"
            } else {
                "资源管理器"
            })
            .size(LabelSize::Small)
            .color(ui::Color::Muted)
            .mb(HEADER_MARGIN),
        )
        // #183 A-3（#188）：git 视图——GitPanel 本体占剩余高度（自带
        // overflow_y_scroll 的列表体；实体 clone 挂载，改动/历史点击/
        // 刷新钮均在其 Render 内已接）。未开工作区时呈现引导提示，
        // 不回退文件树（act-git 的点击意图明确是看 git）。
        .when(git_view, |el| {
            el.when_some(git_panel, |el, panel| el.child(panel.clone()))
                .when(git_panel.is_none(), |el| {
                    el.child(
                        Label::new("未打开工作区（Ctrl+K Ctrl+O）")
                            .size(LabelSize::Small)
                            .color(ui::Color::Muted),
                    )
                })
        })
        // 文件树视图（缺省）：F2 工作区树占剩余高度（uniform_list 内部
        // 滚动/虚拟化——面板级 overflow_y_scroll 已移除防双层滚动互踩；
        // 无工作区时自然高度内容不溢出）。
        .when(!git_view, |el| {
            el.when_some(workspace, |el, tree| el.child(tree.clone()))
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
        })
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
pub fn right_panel(editor: Option<&Entity<Editor>>, width: Pixels, cx: &App) -> impl IntoElement {
    let colors = cx.theme().colors().clone();
    // #167：编辑器状态行按**真实态**呈现（is_read_only），不再无条件写
    // 「只读」——可编辑 docx（#115 链路）标「可编辑」，降级只读标原因
    // （docx_notice 行）。pdf/xlsx/canvas 无编辑引擎，只读是设计事实。
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
                    "pdf" => "只读（pdf 文本层 → 行模型；无 pdf 编辑引擎）".to_string(),
                    "docx" => {
                        if ed.is_read_only() {
                            "只读（docx 块序 → 行模型；降级——见降级说明）".to_string()
                        } else {
                            "可编辑（docx 段落 ↔ 引擎；锁定行只读）".to_string()
                        }
                    }
                    "xlsx" => "只读（xlsx 读面 → 行模型；无原位编辑引擎）".to_string(),
                    "canvas" => "只读（canvas 会话 → 位面渲染）".to_string(),
                    _ => "源文本（md）".to_string(),
                },
                match ed.doc_kind() {
                    "pdf" => "mdc-pdf（pdfium chromium/8086 non-V8）".to_string(),
                    "docx" => "mdc-office-io（docx-rs）".to_string(),
                    "xlsx" => "mdc-office-io（calamine）".to_string(),
                    "canvas" => "mdc-canvas（yrs 会话 + RasterRenderer）".to_string(),
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
    // docx 实测属性面（批 3；#167 补可编辑段/锁定段计数——状态诚实面）。
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
                    // #167：编辑面计数（可编辑态的「锁定行只读」不再是黑箱）。
                    (
                        "可编辑段 / 锁定段".into(),
                        format!("{} / {}", p.editable_paragraphs, p.locked_paragraphs),
                    ),
                ]
            })
            .unwrap_or_default(),
        None => vec![],
    };
    // #167：docx 降级说明（只读原因面向用户呈现——此前只在 stderr）。
    let docx_notice: Option<String> =
        editor.and_then(|ed| ed.read(cx).docx_notice().map(String::from));
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
    // xlsx 实测属性面（G6）。
    let xlsx_rows: Vec<(String, String)> = match editor {
        Some(ed) => ed
            .read(cx)
            .xlsx_props()
            .map(|p| {
                vec![
                    ("工作表".into(), format!("{}", p.sheets)),
                    ("非空数据格".into(), format!("{}", p.cells)),
                    ("公式格".into(), format!("{}（缓存值入行流）", p.formulas)),
                    ("合并范围".into(), format!("{}", p.merges)),
                    ("超链接".into(), format!("{}（计数入属性）", p.hyperlinks)),
                    ("兼容警告".into(), format!("{}", p.warnings)),
                    (
                        "渲染护栏".into(),
                        if p.truncated_sheets > 0 {
                            format!("{} 表超限截断（非静默）", p.truncated_sheets)
                        } else {
                            "未触发".to_string()
                        },
                    ),
                ]
            })
            .unwrap_or_default(),
        None => vec![],
    };
    v_flex()
        .id("right-panel")
        .w(width)
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
        .children(xlsx_rows.iter().map(|(k, v)| {
            let k = k.clone();
            let v = v.clone();
            property_row_owned(k, v)
        }))
        // #167：docx 降级说明行（原因可见——不再只写 stderr）。
        .when_some(docx_notice, |el, notice| {
            el.child(property_row_owned("降级说明".into(), notice))
        })
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

// =============================================================================
// G4 面板拖拽——纯函数面专项单测
// =============================================================================

#[cfg(test)]
mod g4_resize_tests {
    use super::*;

    #[test]
    fn width_delta_sign_by_side() {
        assert_eq!(width_delta(true, 40.), 40., "左栏右移增宽");
        assert_eq!(width_delta(false, 40.), -40., "右栏右移减宽（左移才增）");
        assert_eq!(width_delta(false, -60.), 60., "右栏左移增宽");
    }

    #[test]
    fn clamp_left_panel_domain_150_to_500() {
        assert_eq!(clamp_panel_width(true, 260.), 260., "域内原值");
        assert_eq!(clamp_panel_width(true, 100.), 150., "低于下限钳到 150");
        assert_eq!(clamp_panel_width(true, 999.), 500., "高于上限钳到 500");
        assert_eq!(clamp_panel_width(true, 150.), 150., "边界含端");
        assert_eq!(clamp_panel_width(true, 500.), 500.);
    }

    #[test]
    fn clamp_right_panel_domain_150_to_600() {
        assert_eq!(clamp_panel_width(false, 280.), 280.);
        assert_eq!(clamp_panel_width(false, 100.), 150.);
        assert_eq!(clamp_panel_width(false, 700.), 600., "右栏上限 600");
        assert_eq!(clamp_panel_width(false, 600.), 600.);
    }

    #[test]
    fn hotzone_is_4px_and_defaults_unchanged() {
        assert_eq!(f32::from(PANEL_RESIZE_HOTZONE), 4.);
        assert_eq!(f32::from(LEFT_PANEL_WIDTH), 260., "缺省宽不变（G4 零漂移）");
        assert_eq!(f32::from(RIGHT_PANEL_WIDTH), 280.);
    }
}
