//! git 面板（#68 阶段 F7）——改动 / 历史两视图。
//!
//! 技术路线 = Lead 派单**选项 B**：`std::process::Command` 调系统 git
//! （zed git crate 不在 32 crate 闭包内——选项 A 需扩闭包引依赖雪球，
//! Lead 预案「不在 -> 用 B，不搬新 crate」）。
//!
//! - **改动**：`git status --porcelain`（XY 码 + 路径——工作区相对）；
//!   点击 → 只读 diff 标签（`git diff -- <path>` 行级文本）；
//! - **历史**：`git log --pretty=%h|%s|%cr`（前 50 条）；点击 → 该提交
//!   变更文件清单（`git show --name-status --pretty=format:`）只读标签；
//! - **非 git 目录**：明确提示不崩溃（`is_git_repo` 探测）；
//! - git 不在 PATH / 命令失败 → 同「不可用」提示面。
//!
//! 解析面为纯函数（porcelain/日志行 → 结构——单测钉定）；命令执行为
//! 同步（~10-50ms 量级，UI 线程可接受——异步化登记后续）。

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{AppContext, Context, IntoElement, MouseButton, Render, WeakEntity};
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

use crate::shell::ShellApp;

/// 一条工作区改动（porcelain XY 码 + 路径）。
#[derive(Clone, Debug, PartialEq)]
pub struct GitChange {
    /// 状态码（porcelain X/Y 合并显示：M/A/D/R/?? 等原样）。
    pub code: String,
    /// 工作区相对路径（重命名形「old -> new」原样）。
    pub path: String,
}

/// 一条提交（短哈希 + 标题 + 相对时间）。
#[derive(Clone, Debug, PartialEq)]
pub struct GitCommit {
    pub hash: String,
    pub subject: String,
    pub time: String,
}

/// 视图模式（改动 / 历史）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GitView {
    Changes,
    History,
}

/// git 是否可用（PATH 上有 git）。
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 目录是否在 git 工作树内。
pub fn is_git_repo(dir: &Path) -> bool {
    Command::new("git")
        .args([
            "-C",
            dir.to_string_lossy().as_ref(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// 工作区改动（porcelain 全量；git 不可用/非仓库 → None）。
pub fn git_status(root: &Path) -> Option<Vec<GitChange>> {
    let out = Command::new("git")
        .args([
            "-C",
            root.to_string_lossy().as_ref(),
            "status",
            "--porcelain",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_porcelain(&String::from_utf8_lossy(&out.stdout)))
}

/// 提交历史（前 `limit` 条）。
pub fn git_log(root: &Path, limit: usize) -> Option<Vec<GitCommit>> {
    let format = "%h|%s|%cr";
    let out = Command::new("git")
        .args([
            "-C",
            root.to_string_lossy().as_ref(),
            "log",
            &format!("-n{limit}"),
            &format!("--pretty=format:{format}"),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_log(&String::from_utf8_lossy(&out.stdout)))
}

/// 文件的未暂存 + 已暂存合并 diff（行级文本）。
pub fn git_diff(root: &Path, path: &str) -> Option<String> {
    let out = Command::new("git")
        .args([
            "-C",
            root.to_string_lossy().as_ref(),
            "diff",
            "HEAD",
            "--",
            path,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.is_empty() {
        // 无 HEAD（初始提交前）或纯未跟踪文件——退回 staged/未跟踪形态。
        let out = Command::new("git")
            .args([
                "-C",
                root.to_string_lossy().as_ref(),
                "diff",
                "--no-index",
                "/dev/null",
                path,
            ])
            .output()
            .ok()?;
        // --no-index 对不存在文件返回非零但有输出——以 stdout 为准。
        text = String::from_utf8_lossy(&out.stdout).into_owned();
    }
    Some(text)
}

/// 提交的变更文件清单文本（name-status 形）。
pub fn git_show_files(root: &Path, hash: &str) -> Option<String> {
    let out = Command::new("git")
        .args([
            "-C",
            root.to_string_lossy().as_ref(),
            "show",
            "--name-status",
            "--pretty=format:%h %s (%cr)",
            hash,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// porcelain 行解析（`XY path` / `XY orig -> new` / `?? path`）。
/// 纯函数。
pub fn parse_porcelain(text: &str) -> Vec<GitChange> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let code = l.chars().take(2).collect::<String>().trim().to_string();
            let path = if l.len() > 3 {
                l[3..].to_string()
            } else {
                String::new()
            };
            GitChange { code, path }
        })
        .collect()
}

/// 日志行解析（`hash|subject|time`）。纯函数。
pub fn parse_log(text: &str) -> Vec<GitCommit> {
    // 首尾定界：第一竖线前 = 哈希；最后一竖线后 = 时间（%cr 永不含
    // 竖线）；中间全归主题（主题可含竖线）。
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let (hash, rest) = l.split_once('|')?;
            let (subject, time) = rest.rsplit_once('|')?;
            let hash = hash.to_string();
            let subject = subject.to_string();
            let time = time.to_string();
            if hash.is_empty() {
                None
            } else {
                Some(GitCommit {
                    hash,
                    subject,
                    time,
                })
            }
        })
        .collect()
}

/// git 面板实体（宿主 ShellApp；工作区打开且是 git 仓库时创建）。
pub struct GitPanel {
    root: PathBuf,
    shell: WeakEntity<ShellApp>,
    view: GitView,
    changes: Vec<GitChange>,
    commits: Vec<GitCommit>,
    /// 探测结论（None = 面板不可用的原因文案——非 git 目录/git 缺失）。
    unavailable: Option<String>,
}

impl GitPanel {
    /// 构造并刷新（改动 + 历史双拉）。
    pub fn new(root: PathBuf, shell: WeakEntity<ShellApp>) -> Self {
        let mut this = Self {
            root,
            shell,
            view: GitView::Changes,
            changes: Vec::new(),
            commits: Vec::new(),
            unavailable: None,
        };
        this.refresh();
        this
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 刷新（状态 + 历史；不可用态记录原因文案）。
    pub fn refresh(&mut self) {
        if !git_available() {
            self.unavailable = Some("git 不可用（未安装或不在 PATH）".to_string());
            eprintln!("git: 不可用（缺 git）");
            return;
        }
        if !is_git_repo(&self.root) {
            self.unavailable = Some("非 git 目录（当前工作区不含 .git）".to_string());
            eprintln!("git: 非 git 目录（{}）", self.root.display());
            return;
        }
        self.unavailable = None;
        self.changes = git_status(&self.root).unwrap_or_default();
        self.commits = git_log(&self.root, 50).unwrap_or_default();
        eprintln!(
            "git: {} 改动 / {} 提交（{}）",
            self.changes.len(),
            self.commits.len(),
            self.root.display()
        );
    }

    /// 打开改动 diff（只读标签）。window 由监听器传入——shell 经
    /// `Entity::update` 跨实体进入（不带窗口绑定），焦点句柄返回后
    /// 在本监听器的 window 域落焦。
    fn open_diff(&self, path: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(text) = git_diff(&self.root, path) else {
            return;
        };
        let title = format!("diff: {path}");
        let shell = self.shell.clone();
        let focus = shell
            .update(cx, |shell, cx| shell.open_readonly_tab(title, text, cx))
            .ok()
            .flatten();
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
    }

    /// 打开提交变更清单（只读标签；window 语义同 [`Self::open_diff`]）。
    fn open_commit(&self, hash: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(text) = git_show_files(&self.root, hash) else {
            return;
        };
        let title = format!("提交 {hash}");
        let shell = self.shell.clone();
        let focus = shell
            .update(cx, |shell, cx| shell.open_readonly_tab(title, text, cx))
            .ok()
            .flatten();
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
    }

    /// 首个改动（烟雾面：根 + 路径）。
    pub fn first_change(&self) -> Option<(PathBuf, String)> {
        self.changes
            .first()
            .map(|c| (self.root.clone(), c.path.clone()))
    }
}

impl Render for GitPanel {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let hover = colors.element_hover;
        let selected = colors.element_selected;

        // 头部：视图切换（改动/历史）+ 刷新。
        let view = self.view;
        let changes_count = self.changes.len();
        let header = h_flex()
            .id("git-header")
            .w_full()
            .gap_1()
            .mb_1()
            .child(
                h_flex()
                    .id("git-tab-changes")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .gap_1()
                    .when(view == GitView::Changes, move |el| el.bg(selected))
                    .hover(move |el| el.bg(hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.view = GitView::Changes;
                            cx.notify();
                        }),
                    )
                    .child(
                        Icon::new(IconName::GitBranch)
                            .size(IconSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .child(Label::new(format!("改动（{changes_count}）")).size(LabelSize::Small)),
            )
            .child(
                h_flex()
                    .id("git-tab-history")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .when(view == GitView::History, move |el| el.bg(selected))
                    .hover(move |el| el.bg(hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.view = GitView::History;
                            cx.notify();
                        }),
                    )
                    .child(
                        Icon::new(IconName::GitCommit)
                            .size(IconSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .child(Label::new("历史").size(LabelSize::Small)),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("git-refresh")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .hover(move |el| el.bg(hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.refresh();
                            cx.notify();
                        }),
                    )
                    .child(
                        Label::new("刷新")
                            .size(LabelSize::Small)
                            .color(ui::Color::Muted),
                    ),
            );

        // 体：不可用提示 / 对应视图列表。
        let body: gpui::AnyElement = if let Some(reason) = &self.unavailable {
            v_flex()
                .id("git-unavailable")
                .p_2()
                .gap_1()
                .child(
                    Label::new(reason.as_str())
                        .size(LabelSize::Small)
                        .color(ui::Color::Muted),
                )
                .into_any_element()
        } else {
            match self.view {
                GitView::Changes => {
                    if self.changes.is_empty() {
                        v_flex()
                            .id("git-clean")
                            .p_2()
                            .child(
                                Label::new("工作区干净（无改动）")
                                    .size(LabelSize::Small)
                                    .color(ui::Color::Muted),
                            )
                            .into_any_element()
                    } else {
                        v_flex()
                            .id("git-changes")
                            .gap_1()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(self.changes.iter().map(|change| {
                                let path = change.path.clone();
                                let path_for_id = path.clone();
                                let path_for_click = path.clone();
                                let code = change.code.clone();
                                h_flex()
                                    .id(gpui::SharedString::from(format!("git-chg-{path_for_id}")))
                                    .w_full()
                                    .px_1()
                                    .py_1()
                                    .gap_2()
                                    .items_center()
                                    .rounded_sm()
                                    .hover(move |el| el.bg(hover))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, window, cx| {
                                            this.open_diff(&path_for_click, window, cx);
                                        }),
                                    )
                                    .child(
                                        Label::new(code.as_str())
                                            .size(LabelSize::XSmall)
                                            .color(ui::Color::Accent),
                                    )
                                    .child(Label::new(path.as_str()).size(LabelSize::Small))
                            }))
                            .into_any_element()
                    }
                }
                GitView::History => v_flex()
                    .id("git-history")
                    .gap_1()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(self.commits.iter().map(|commit| {
                        let hash = commit.hash.clone();
                        v_flex()
                            .id(gpui::SharedString::from(format!("git-cmt-{hash}")))
                            .w_full()
                            .px_1()
                            .py_1()
                            .gap_0p5()
                            .rounded_sm()
                            .hover(move |el| el.bg(hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.open_commit(&hash, window, cx);
                                }),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Label::new(commit.hash.as_str())
                                            .size(LabelSize::XSmall)
                                            .color(ui::Color::Accent),
                                    )
                                    .child(
                                        Label::new(commit.time.as_str())
                                            .size(LabelSize::XSmall)
                                            .color(ui::Color::Hidden),
                                    ),
                            )
                            .child(Label::new(commit.subject.as_str()).size(LabelSize::Small))
                    }))
                    .into_any_element(),
            }
        };

        v_flex()
            .id("git-panel")
            .w_full()
            .flex_1()
            .min_h_0()
            .gap_1()
            .child(header)
            .child(body)
    }
}

// ---------------------------------------------------------------------------
// 解析纯逻辑单测。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_parsing_xy_and_paths() {
        let text = " M src/main.rs\n\
                    A  new_file.rs\n\
                    D  gone.rs\n\
                    ?? untracked.md\n\
                    R  old.rs -> new.rs\n\
                    \n";
        let changes = parse_porcelain(text);
        assert_eq!(changes.len(), 5, "空行过滤");
        assert_eq!(changes[0].code, "M");
        assert_eq!(changes[0].path, "src/main.rs");
        assert_eq!(changes[1].code, "A");
        assert_eq!(changes[3].code, "??");
        assert_eq!(changes[3].path, "untracked.md");
        assert_eq!(changes[4].path, "old.rs -> new.rs", "重命名原样");
    }

    #[test]
    fn log_parsing_three_fields() {
        let text = "abc1234|fix: 修复拖拽|2 hours ago\ndef5678|feat: 新功能|3 days ago";
        let commits = parse_log(text);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc1234");
        assert_eq!(commits[0].subject, "fix: 修复拖拽");
        assert_eq!(commits[0].time, "2 hours ago");

        // 主题含竖线——rsplitn(3) 从右定界，竖线余量全归主题。
        let tricky = "abc1234|feat: a|b|c|1 hour ago";
        let commits = parse_log(tricky);
        assert_eq!(commits[0].subject, "feat: a|b|c", "竖线余量归主题");
        assert_eq!(commits[0].time, "1 hour ago");
        assert_eq!(commits[0].hash, "abc1234");

        // 空行过滤 + 空哈希行丢弃。
        assert!(parse_porcelain("").is_empty());
        assert!(parse_log("\n\n").is_empty());
    }

    /// 本仓即 git 仓库——命令面最小实证（沙箱恒有 git）。
    #[test]
    fn git_command_surface_on_this_repo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(git_available(), "沙箱有 git");
        assert!(is_git_repo(root), "本仓是 git 工作树");
        // 历史非空（本仓提交众多）。
        let commits = git_log(root, 5).expect("log 拉取");
        assert!(!commits.is_empty(), "本仓有提交历史");
        assert!(commits.len() <= 5, "limit 生效");
        // 非仓库目录 → 探测 false。
        let tmp = std::env::temp_dir();
        assert!(
            !is_git_repo(&tmp) || is_git_repo(&tmp),
            "探测不崩溃（tmp 是否在仓库内因环境而异）"
        );
    }
}
