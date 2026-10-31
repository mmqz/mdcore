//! 工作区文件树（#68 阶段 F2）——真实磁盘目录的懒加载树。
//!
//! 分层：
//! - [`TreeState`] 纯状态机（根/展开集/清单缓存/活动文件——无 gpui
//!   依赖，单测直接钉定）；
//! - [`FileTree`] gpui 实体（渲染 + 交互；宿主 [`ShellApp`]）。
//!
//! 设计要点（Lead 派单口径）：
//! - **懒加载**：目录清单只在首次展开时读取（`listings` 缓存）——
//!   打开大工作区只读根目录一层；
//! - **虚拟化**：可见行扁平化（[`TreeState::visible_rows`]）后经
//!   `uniform_list` 渲染——1000+ 文件目录只布局可视行，不卡；
//! - **排序**：目录优先 + 名称大小写不敏感字典序（[`entry_order`]）；
//! - **忽略**：隐藏项（`.` 前缀）与 `node_modules` / `target`
//!   （[`is_ignored_name`]；可配置面随 F6 设置件落位）；
//! - **符号链接不跟随**（`DirEntry::metadata` 不跟随面按文件呈现）：
//!   防循环展开（a → 父目录会在 DFS 中无限递归）；另加深度上限
//!   双保险。
//!
//! 交互：单击目录 = 展开/折叠；单击文件 = 经 `ShellApp::open_document`
//! 打开（F1 通道复用）；右键 = 在文件管理器中显示（`reveal_path`——
//! 文件行揭示其所在目录）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{App, Context, IntoElement, MouseButton, Render, WeakEntity, px, uniform_list};
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, Label, LabelSize};

use crate::shell::ShellApp;

/// 目录条目。
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// 忽略名单（可配置面随 F6 设置件；当前为常量口径——`.git` 已被
/// `.` 前缀规则覆盖，列出以自文档）。
pub fn is_ignored_name(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target")
}

/// 排序：目录优先 → 名称大小写不敏感字典序 → 原文序（大小写仅歧义时）。
pub fn entry_order(a: &FileEntry, b: &FileEntry) -> std::cmp::Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

/// 读取目录一层（忽略规则过滤 + 排序；读失败返回空——权限/消失容错，
/// 空态即「该目录不可读」，不崩溃）。
pub fn list_dir(dir: &Path) -> Vec<FileEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<FileEntry> = entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| !is_ignored_name(n))
                .unwrap_or(false)
        })
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // DirEntry::metadata 不跟随符号链接——链接按文件呈现
            //（防循环展开；见模块注释）。
            let is_dir = e.metadata().map(|m| m.is_dir()).unwrap_or(false);
            FileEntry {
                path: e.path(),
                name,
                is_dir,
            }
        })
        .collect();
    out.sort_by(entry_order);
    out
}

/// 扁平化后的可见行（uniform_list 渲染单元）。
#[derive(Clone, Debug)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub is_active: bool,
}

/// 深度上限（DFS 双保险——符号链接/异常环防御）。
const MAX_DEPTH: usize = 24;

/// 文件树纯状态机（单测面——无 gpui 依赖）。
pub struct TreeState {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, Rc<Vec<FileEntry>>>,
    active_file: Option<PathBuf>,
}

impl TreeState {
    /// 构造：根目录展开态 + 根层清单即读（打开工作区的首次 IO）。
    pub fn new(root: PathBuf) -> Self {
        let mut this = Self {
            root,
            expanded: HashSet::new(),
            listings: HashMap::new(),
            active_file: None,
        };
        this.expanded.insert(this.root.clone());
        this.ensure_listing(&this.root.clone());
        this
    }

    /// 工作区根。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 清单缓存命中查询（懒加载语义测试面）。
    pub fn listing_loaded(&self, dir: &Path) -> bool {
        self.listings.contains_key(dir)
    }

    /// 当前打开文件高亮（ShellApp::open_document 后同步）。
    pub fn set_active_file(&mut self, path: Option<PathBuf>) {
        self.active_file = path;
    }

    /// 展开/折叠目录（展开时懒加载清单）。
    pub fn toggle(&mut self, dir: &Path) {
        if self.expanded.contains(dir) {
            self.expanded.remove(dir);
        } else {
            self.ensure_listing(dir);
            self.expanded.insert(dir.to_path_buf());
        }
    }

    fn ensure_listing(&mut self, dir: &Path) {
        if !self.listings.contains_key(dir) {
            self.listings
                .insert(dir.to_path_buf(), Rc::new(list_dir(dir)));
        }
    }

    /// 可见行扁平化（DFS：只进展开目录；行序 = 渲染序）。
    pub fn visible_rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        let root_expanded = self.expanded.contains(&self.root);
        rows.push(TreeRow {
            path: self.root.clone(),
            name: self
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.root.display().to_string()),
            depth: 0,
            is_dir: true,
            expanded: root_expanded,
            is_active: false,
        });
        if root_expanded {
            let root = self.root.clone();
            self.push_children(&root, 1, &mut rows);
        }
        rows
    }

    fn push_children(&self, dir: &Path, depth: usize, rows: &mut Vec<TreeRow>) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(entries) = self.listings.get(dir) else {
            return;
        };
        for entry in entries.iter() {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            rows.push(TreeRow {
                path: entry.path.clone(),
                name: entry.name.clone(),
                depth,
                is_dir: entry.is_dir,
                expanded,
                is_active: self.active_file.as_deref() == Some(entry.path.as_path()),
            });
            if expanded {
                let path = entry.path.clone();
                self.push_children(&path, depth + 1, rows);
            }
        }
    }
}

/// 工作区文件树实体（左栏呈现；宿主 [`ShellApp`]——文件点击经其
/// `open_document` 打开）。
pub struct FileTree {
    state: TreeState,
    shell: WeakEntity<ShellApp>,
}

impl FileTree {
    pub fn new(root: PathBuf, shell: WeakEntity<ShellApp>) -> Self {
        Self {
            state: TreeState::new(root),
            shell,
        }
    }

    /// 工作区根（状态栏显示面）。
    pub fn root(&self) -> &Path {
        self.state.root()
    }

    /// 当前打开文件高亮同步（ShellApp::open_document 后调用）。
    pub fn set_active_file(&mut self, path: Option<PathBuf>) {
        self.state.set_active_file(path);
    }
}

impl Render for FileTree {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row_count = self.state.visible_rows().len();
        let weak = cx.entity().downgrade();
        uniform_list(
            "file-tree-rows",
            row_count,
            move |visible: std::ops::Range<usize>, window: &mut gpui::Window, cx: &mut App| {
                weak.update(cx, |this, cx| this.render_rows(visible, window, cx))
                    .unwrap_or_default()
            },
        )
        .flex_1()
        .min_h_0()
    }
}

impl FileTree {
    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let rows = self.state.visible_rows();
        range
            .filter_map(|i| rows.get(i).map(|row| self.render_row(row, cx)))
            .collect()
    }

    fn render_row(&self, row: &TreeRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = cx.theme().colors().clone();
        let hover = colors.element_hover;
        let selected = colors.element_selected;
        let indent = px(4. + row.depth as f32 * 14.);
        let is_dir = row.is_dir;
        let path = row.path.clone();
        let path_for_reveal = row.path.clone();

        let (chevron, icon) = if is_dir {
            (
                if row.expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                if row.expanded {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                },
            )
        } else {
            (IconName::ChevronRight, IconName::File)
        };

        h_flex()
            .id(gpui::SharedString::from(format!(
                "ft-{}-{}",
                row.depth,
                row.path.display()
            )))
            .pl(indent)
            .pr_2()
            .gap_1()
            .h(px(22.))
            .flex_none()
            .items_center()
            .rounded_sm()
            .when(row.is_active, |el| el.bg(selected))
            .hover(move |el| el.bg(hover))
            // 左键：目录展开/折叠；文件经 F1 通道打开。
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _window, cx| {
                    if is_dir {
                        this.state.toggle(&path);
                        cx.notify();
                    } else {
                        let shell = this.shell.clone();
                        let _ = shell.update_in(cx, |shell, window, cx| {
                            shell.open_document(path.clone(), window, cx);
                        });
                    }
                }),
            )
            // 右键：在文件管理器中显示（文件行揭示所在目录）。
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |_, _, _, cx| {
                    let target = if is_dir {
                        path_for_reveal.clone()
                    } else {
                        path_for_reveal
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| path_for_reveal.clone())
                    };
                    cx.reveal_path(&target);
                }),
            )
            .child(
                Icon::new(chevron)
                    .size(IconSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Icon::new(icon)
                    .size(IconSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(row.name.as_str())
                    .size(LabelSize::Small)
                    .when(row.is_active, |l| l.color(ui::Color::Accent)),
            )
            .into_any_element()
    }
}

/// 状态栏工作区显示文案：`~` 缩写家目录；超长截前保尾（尾部含文件名
/// 更可辨）。纯函数（单测钉定）。
pub fn workspace_display(root: &Path) -> String {
    let full = root.display().to_string();
    let shortened = match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && full.starts_with(&home) => {
            let rest = &full[home.len()..];
            if rest.is_empty() {
                "~".to_string()
            } else {
                format!("~{rest}")
            }
        }
        _ => full.clone(),
    };
    const MAX_LEN: usize = 56;
    let count = shortened.chars().count();
    if count > MAX_LEN {
        let tail: String = shortened.chars().skip(count - MAX_LEN + 1).collect();
        format!("…{tail}")
    } else {
        shortened
    }
}

// ---------------------------------------------------------------------------
// 纯逻辑单测（临时目录夹具——真实 fs 面；无 gpui 上下文）。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_fixture(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mang-ft-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub").join("deep")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("b.md"), "b").unwrap();
        std::fs::write(dir.join("A.md"), "a").unwrap();
        std::fs::write(dir.join("sub").join("c.md"), "c").unwrap();
        std::fs::write(dir.join("sub").join("deep").join("d.md"), "d").unwrap();
        dir
    }

    #[test]
    fn ignored_names_cover_hidden_and_build_dirs() {
        assert!(is_ignored_name(".git"));
        assert!(is_ignored_name(".hidden"));
        assert!(is_ignored_name("node_modules"));
        assert!(is_ignored_name("target"));
        assert!(!is_ignored_name("src"));
        assert!(!is_ignored_name("main.md"));
        assert!(!is_ignored_name("target_.md"), "整名匹配（前缀不算）");
    }

    #[test]
    fn list_dir_sorts_dirs_first_ci_alpha() {
        let dir = temp_fixture("sort");
        let names: Vec<String> = list_dir(&dir).iter().map(|e| e.name.clone()).collect();
        assert_eq!(
            names,
            vec!["sub".to_string(), "A.md".to_string(), "b.md".to_string()],
            "目录优先 + 大小写不敏感字典序"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_dir_filters_ignored() {
        let dir = temp_fixture("filter");
        let names: Vec<String> = list_dir(&dir).iter().map(|e| e.name.clone()).collect();
        assert!(
            !names.contains(&"node_modules".to_string()),
            "node_modules 过滤"
        );
        assert!(
            !names.iter().any(|n| n.starts_with('.')),
            "隐藏项（.git 等）过滤"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn visible_rows_flatten_lazily() {
        let dir = temp_fixture("flatten");
        let mut tree = TreeState::new(dir.clone());
        // 初始：仅根展开 → 根 + 一层子项（sub/A.md/b.md）。
        let rows = tree.visible_rows();
        assert_eq!(rows.len(), 4, "根 + sub + A.md + b.md");
        assert!(rows[0].is_dir && rows[0].depth == 0, "根行");
        assert_eq!(rows[1].name, "sub", "目录序首");
        assert_eq!(rows[1].depth, 1);

        // 懒加载：sub 未展开前其清单未读取。
        assert!(tree.listing_loaded(&dir), "根清单已缓存");
        assert!(
            !tree.listing_loaded(&dir.join("sub")),
            "sub 清单未加载（懒加载语义）"
        );

        // 展开 sub → c.md + deep 出现，深度 +1。
        tree.toggle(&dir.join("sub"));
        let rows = tree.visible_rows();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"c.md"), "sub 子项出现：{names:?}");
        assert!(names.contains(&"deep"), "子目录出现：{names:?}");
        let deep_row = rows.iter().find(|r| r.name == "deep").unwrap();
        assert_eq!(deep_row.depth, 2, "深度递增");
        assert!(tree.listing_loaded(&dir.join("sub")), "展开触发清单加载");

        // 再折叠 sub → 深层行消失（清单缓存保留——再展开零 IO）。
        tree.toggle(&dir.join("sub"));
        assert_eq!(tree.visible_rows().len(), 4, "折叠恢复");
        assert!(tree.listing_loaded(&dir.join("sub")), "缓存保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_file_marks_exactly_one_row() {
        let dir = temp_fixture("active");
        let mut tree = TreeState::new(dir.clone());
        tree.set_active_file(Some(dir.join("A.md")));
        let rows = tree.visible_rows();
        let active: Vec<&TreeRow> = rows.iter().filter(|r| r.is_active).collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "A.md");
        // 清除高亮。
        tree.set_active_file(None);
        assert!(tree.visible_rows().iter().all(|r| !r.is_active));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_display_abbreviates_and_truncates() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/tester".into());
        let display = workspace_display(Path::new(&format!("{home}/proj/mdcore")));
        assert!(
            display.starts_with('~') || display.contains("proj/mdcore"),
            "家目录缩写或原样保留：{display}"
        );
        let long = format!("{home}/{}", "very-long-dir-name/".repeat(12));
        let display = workspace_display(Path::new(&long));
        assert!(display.starts_with('…'), "超长截断：{display}");
        assert!(
            display.ends_with("very-long-dir-name/"),
            "截前保尾：{display}"
        );
        assert!(display.chars().count() <= 57, "长度受控：{display}");
    }
}
