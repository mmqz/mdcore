//! 右键菜单（#63 Lead 打磨项）。
//!
//! # 分层：数据 / 渲染 / 接线 三段分离
//!
//! - [`MenuItem`] / [`ContextMenu`]：**纯数据**，零 GPUI 依赖——可单测、
//!   可在无窗口环境构造。菜单长什么样、点哪一项干什么，全在这一层定死。
//! - [`menu_geometry`]：菜单**几何**（尺寸/边距/每项行高）纯函数——
//!   屏幕边缘翻转、子菜单飞出方向这些「到底弹在哪」的判断必须可测，
//!   不能埋在渲染闭包里。
//! - 渲染与点击接线在 `shell.rs` / `file_tree.rs`（需要 `Window`）。
//!
//! # 为什么几何要单独抽出来
//!
//! 右键菜单最容易出的 bug 不是「菜单不显示」，而是**在屏幕右下角弹到
//! 屏幕外**——用户看不见但程序一切正常。这类问题只有把摆放算法做成
//! 纯函数、对着边界写断言才抓得住（见本文件测试 `flip_at_edges` 一族）。

/// 屏幕/窗口坐标点（逻辑像素，f32——与 gpui 的 `Point<Pixels>` 同域，
/// 但本模块不依赖 gpui，故自带一份最小结构）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 矩形（左上角 + 宽高）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// 菜单项。
///
/// `enabled: false` 用于「有菜单项但此刻不可用」——灰显而不是消失：
/// 消失会让菜单长短跳变，用户刚瞄准的项会跑位。
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    /// 可点击动作。`id` 是派发键（渲染层用它做 element id 与回调匹配）。
    Action {
        id: &'static str,
        label: String,
        shortcut: Option<String>,
        enabled: bool,
    },
    /// 分隔线。
    Separator,
    /// 子菜单（点开后横向飞出；本批只做一层）。
    Submenu { label: String, items: Vec<MenuItem> },
}

impl MenuItem {
    /// 动作项构造（缺省可用）。
    pub fn action(id: &'static str, label: impl Into<String>) -> Self {
        MenuItem::Action {
            id,
            label: label.into(),
            shortcut: None,
            enabled: true,
        }
    }

    /// 附加快捷键提示。
    pub fn with_shortcut(mut self, key: &str) -> Self {
        if let MenuItem::Action { shortcut, .. } = &mut self {
            *shortcut = Some(key.to_string());
        }
        self
    }

    /// 从 keymap 表取该动作当前键位标签（#217 单一事实源：菜单标签
    /// 与实际键位同源，用户换绑后跟随；无绑定 → 不标）。
    pub fn with_shortcut_for(mut self, action_id: &str) -> Self {
        if let MenuItem::Action { shortcut, .. } = &mut self {
            *shortcut = crate::keymap::store().label(action_id);
        }
        self
    }

    /// 置灰/启用。
    pub fn with_enabled(mut self, on: bool) -> Self {
        if let MenuItem::Action { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }

    /// 占位高度（行）：动作/子菜单 1 行，分隔线按「半行+上下留白」占位。
    pub fn height_units(&self) -> f32 {
        match self {
            MenuItem::Separator => SEPARATOR_UNITS,
            _ => 1.0,
        }
    }

    pub fn is_action(&self) -> bool {
        matches!(self, MenuItem::Action { .. })
    }
}

/// 单行高度（px）。
pub const ITEM_HEIGHT: f32 = 26.;
/// 分隔线占位（行高倍数）。
pub const SEPARATOR_UNITS: f32 = 0.4;
/// 菜单上下内边距（px）。
pub const MENU_PADDING_Y: f32 = 4.;
/// 菜单左右内边距（px）。
pub const MENU_PADDING_X: f32 = 4.;
/// 菜单最小宽度（px）——再窄标签会折行。
pub const MENU_MIN_WIDTH: f32 = 160.;
/// 宽度估算：每字符像素（中文按 1.5 倍算，见 [`estimate_width`]）。
const CHAR_W: f32 = 7.5;

/// 文本宽度粗估（不加载字体——菜单宽度只需要「够不够放」，不需要精确）。
///
/// CJK 字符按 1.6 倍宽度估：菜单标签大量是中文，按拉丁字符宽估会窄一半，
/// 渲染时标签被截断。两侧各留 `PADDING` 余量。
pub fn estimate_width(text: &str) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { CHAR_W } else { CHAR_W * 1.6 })
        .sum()
}

/// 菜单几何：给定项集，算出菜单自身应有尺寸。
pub fn menu_size(items: &[MenuItem]) -> (f32, f32) {
    let mut w = MENU_MIN_WIDTH;
    let mut h = MENU_PADDING_Y * 2.;
    for item in items {
        h += ITEM_HEIGHT * item.height_units();
        let need = match item {
            MenuItem::Action {
                label, shortcut, ..
            } => {
                // 标签 + 间隔 + 快捷键 + 左右内边距
                let sc = shortcut.as_deref().unwrap_or("");
                estimate_width(label)
                    + if sc.is_empty() {
                        0.
                    } else {
                        32. + estimate_width(sc)
                    }
                    + MENU_PADDING_X * 2.
                    + 24.
            }
            MenuItem::Submenu { label, .. } => estimate_width(label) + 40. + MENU_PADDING_X * 2.,
            MenuItem::Separator => 0.,
        };
        w = w.max(need);
    }
    (w, h)
}

/// 把菜单摆到屏幕内：优先在点击点右下方向展开；越界则翻转/夹紧。
///
/// `anchor` = 点击点（屏幕坐标），`size` = 菜单尺寸，`screen` = 可用区域
/// （窗口客户区）。返回菜单左上角坐标。
///
/// 规则（顺序有讲究）：
/// 1. 默认左上角 = 点击点；
/// 2. 右边越界 → 左翻（菜单右边缘对齐点击点）；
/// 3. 下边越界 → 上翻；
/// 4. 翻转后仍越界（菜单比屏幕还大）→ 夹紧到 [0, screen - size]，
///    保证菜单**至少完整可见地贴在屏幕内**，不会一半在屏幕外。
pub fn place_menu(anchor: Point, size: (f32, f32), screen: Rect) -> Point {
    let (w, h) = size;
    let mut x = anchor.x;
    let mut y = anchor.y;

    // 右越界 → 左翻
    if x + w > screen.right() {
        x = anchor.x - w;
    }
    // 下越界 → 上翻
    if y + h > screen.bottom() {
        y = anchor.y - h;
    }
    // 翻转后仍越界 → 夹紧
    x = x.clamp(screen.x, (screen.right() - w).max(screen.x));
    y = y.clamp(screen.y, (screen.bottom() - h).max(screen.y));
    Point::new(x, y)
}

/// 子菜单飞出方向：默认右飞；右侧放不下则左飞。
///
/// 返回子菜单左上角。`parent` = 父菜单矩形（已摆放好）。
pub fn place_submenu(parent: Rect, row_y: f32, size: (f32, f32), screen: Rect) -> Point {
    let (w, h) = size;
    // 与父项左/上边对齐，再向右飞一个菜单宽。
    let mut x = parent.right();
    let mut y = parent.y + row_y - MENU_PADDING_Y;
    if x + w > screen.right() {
        x = parent.x - w;
    }
    y = y.clamp(screen.y, (screen.bottom() - h).max(screen.y));
    x = x.clamp(screen.x, (screen.right() - w).max(screen.x));
    Point::new(x, y)
}

/// 已打开的右键菜单（位置 + 项集）。
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    pub items: Vec<MenuItem>,
    /// 请求的展开点（点击处，屏幕坐标）。实际摆放由 [`Self::layout`] 算。
    pub pos: Point,
}

impl ContextMenu {
    pub fn new(items: Vec<MenuItem>, pos: Point) -> Self {
        Self { items, pos }
    }

    /// 算出菜单在 `screen` 内的最终矩形（含边缘翻转）。
    pub fn layout(&self, screen: Rect) -> Rect {
        let (w, h) = menu_size(&self.items);
        let p = place_menu(self.pos, (w, h), screen);
        Rect::new(p.x, p.y, w, h)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 所有动作项 id（接线层批绑定用；顺序 = 菜单顺序）。
    pub fn action_ids(&self) -> Vec<&'static str> {
        self.items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }
}

// ── 预置菜单 ───────────────────────────────────────────────────

/// 编辑器右键菜单。
///
/// `has_selection` 决定剪切/复制/删除是否可用（无选区时灰显，不消失）；
/// `read_only` 决定所有改写类项是否可用。
pub fn editor_menu(has_selection: bool, read_only: bool) -> Vec<MenuItem> {
    let writable = !read_only;
    vec![
        MenuItem::action("ed-cut", "剪切")
            .with_shortcut_for("cut")
            .with_enabled(has_selection && writable),
        MenuItem::action("ed-copy", "复制")
            .with_shortcut_for("copy")
            .with_enabled(has_selection),
        MenuItem::action("ed-paste", "粘贴")
            .with_shortcut_for("paste")
            .with_enabled(writable),
        MenuItem::Separator,
        MenuItem::action("ed-select-all", "全选").with_shortcut_for("select_all"),
        MenuItem::action("ed-find", "查找").with_shortcut_for("open_find"),
        MenuItem::action("ed-replace", "替换")
            .with_shortcut_for("open_replace")
            .with_enabled(writable),
        MenuItem::Separator,
        MenuItem::action("ed-goto", "跳转到行").with_shortcut_for("goto_line"),
        MenuItem::action("ed-fold", "折叠/展开").with_shortcut_for("toggle_fold"),
    ]
}

/// 文件树右键菜单。`is_dir` 决定「新建文件」是建在目录里还是同级。
pub fn file_tree_menu(is_dir: bool) -> Vec<MenuItem> {
    vec![
        MenuItem::action("ft-new-file", "新建文件"),
        MenuItem::action("ft-new-folder", "新建文件夹").with_enabled(is_dir),
        MenuItem::Separator,
        MenuItem::action("ft-open", "打开").with_enabled(!is_dir),
        MenuItem::action("ft-rename", "重命名").with_shortcut("F2"),
        MenuItem::action("ft-delete", "删除").with_shortcut("Delete"),
        MenuItem::Separator,
        MenuItem::action("ft-reveal", "在资源管理器中显示"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::new(0., 0., 1200., 800.)
    }

    #[test]
    fn menu_size_grows_with_item_count() {
        let a = menu_size(&editor_menu(false, false));
        let b = menu_size(&editor_menu(false, false)[..2]);
        assert!(a.1 > b.1, "项多则高，{} vs {}", a.1, b.1);
    }

    #[test]
    fn menu_width_fits_cjk_labels() {
        // 中文标签不能被按拉丁字符宽估成 160 最小宽就完事。
        let items = vec![MenuItem::action("x", "在资源管理器中显示").with_shortcut("Ctrl+Shift+E")];
        let (w, _) = menu_size(&items);
        assert!(w > MENU_MIN_WIDTH + 40., "CJK 长标签应把菜单撑宽，实际 {w}");
    }

    #[test]
    fn place_menu_default_down_right() {
        let p = place_menu(Point::new(100., 100.), (200., 300.), screen());
        assert_eq!(p, Point::new(100., 100.), "有空间时就在点击点右下");
    }

    #[test]
    fn place_menu_flips_left_at_right_edge() {
        let p = place_menu(Point::new(1150., 100.), (200., 300.), screen());
        assert!(p.x + 200. <= 1200., "右越界必须左翻，实际 x={}", p.x);
        assert!(p.x >= 0.);
    }

    #[test]
    fn place_menu_flips_up_at_bottom_edge() {
        let p = place_menu(Point::new(100., 750.), (200., 300.), screen());
        assert!(p.y + 300. <= 800., "下越界必须上翻，实际 y={}", p.y);
    }

    #[test]
    fn place_menu_clamps_when_larger_than_screen() {
        // 菜单比屏幕还大：翻不动，只能夹紧到左上角。
        let p = place_menu(Point::new(600., 400.), (2000., 1000.), screen());
        assert_eq!(p, Point::new(0., 0.));
    }

    #[test]
    fn submenu_flyout_right_by_default() {
        let parent = Rect::new(100., 100., 200., 300.);
        let p = place_submenu(parent, 26., (180., 120.), screen());
        assert!((p.x - 300.).abs() < 0.01, "子菜单默认贴父菜单右侧飞出");
    }

    #[test]
    fn submenu_flips_left_when_no_room() {
        let parent = Rect::new(1100., 100., 90., 300.);
        let p = place_submenu(parent, 26., (180., 120.), screen());
        assert!(p.x < parent.x, "右侧放不下时左翻，实际 x={}", p.x);
    }

    #[test]
    fn editor_menu_disables_cut_without_selection() {
        let items = editor_menu(false, false);
        let cut = items.iter().find_map(|i| match i {
            MenuItem::Action { id, enabled, .. } if *id == "ed-cut" => Some(*enabled),
            _ => None,
        });
        assert_eq!(cut, Some(false), "无选区时剪切应灰显");
    }

    #[test]
    fn editor_menu_disables_edits_when_read_only() {
        let items = editor_menu(true, true);
        for id in ["ed-cut", "ed-paste", "ed-replace"] {
            let enabled = items.iter().find_map(|i| match i {
                MenuItem::Action {
                    id: got, enabled, ..
                } if *got == id => Some(*enabled),
                _ => None,
            });
            assert_eq!(enabled, Some(false), "只读态 {id} 应灰显");
        }
    }

    #[test]
    fn readonly_still_allows_copy_and_find() {
        let items = editor_menu(true, true);
        for id in ["ed-copy", "ed-select-all", "ed-find", "ed-goto"] {
            let enabled = items.iter().find_map(|i| match i {
                MenuItem::Action {
                    id: got, enabled, ..
                } if *got == id => Some(*enabled),
                _ => None,
            });
            assert_eq!(enabled, Some(true), "只读态 {id} 仍应可用");
        }
    }

    #[test]
    fn file_tree_menu_new_folder_only_for_dirs() {
        for (is_dir, want) in [(true, true), (false, false)] {
            let items = file_tree_menu(is_dir);
            let enabled = items.iter().find_map(|i| match i {
                MenuItem::Action { id, enabled, .. } if *id == "ft-new-folder" => Some(*enabled),
                _ => None,
            });
            assert_eq!(enabled, Some(want), "is_dir={is_dir} 时新建文件夹可用性");
        }
    }

    #[test]
    fn menu_ids_are_unique() {
        for items in [editor_menu(true, false), file_tree_menu(true)] {
            let mut ids: Vec<&'static str> = Vec::new();
            for i in &items {
                if let MenuItem::Action { id, .. } = i {
                    assert!(!ids.contains(id), "菜单项 id 重复：{id}");
                    ids.push(id);
                }
            }
        }
    }

    #[test]
    fn layout_never_leaves_screen() {
        // 暴力扫一圈展开点：任何位置算出的菜单矩形都必须在屏幕内。
        let menu = ContextMenu::new(editor_menu(false, false), Point::new(0., 0.));
        let (w, h) = menu_size(&menu.items);
        for x in (0..1200).step_by(37) {
            for y in (0..800).step_by(41) {
                let m = ContextMenu::new(menu.items.clone(), Point::new(x as f32, y as f32));
                let r = m.layout(screen());
                assert!(r.x >= -0.01, "x 越界 {r:?}");
                assert!(r.y >= -0.01, "y 越界 {r:?}");
                assert!(r.right() <= 1200.01, "右边越界 {r:?}（宽 {w}）");
                assert!(r.bottom() <= 800.01, "下边越界 {r:?}（高 {h}）");
            }
        }
    }
}
