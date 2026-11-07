//! 内嵌资产源 —— 仅图标面（SVG 来自 Zed 上游 `assets/icons/`，
//! Lucide/Phosphor 图标系，随仓 GPL-3.0-or-later 口径，见
//! `third_party/zed/crates/icons/README.md` 出处标注）。
//!
//! 字体不内嵌：文本系统走 fontconfig 系统面（`Noto Sans SC`，
//! 见 `settings.rs`）。主题 JSON 不内嵌：`theme::init(JustBase)`
//! 用内置基础主题。

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// 内嵌图标清单：路径 → 字节。
///
/// 每条对应 `assets/icons/` 下一个文件（19 个）。新增图标 = 拷贝 SVG
/// + 此表加一行 + [`SHELL_ICON_SET`] 覆盖表加一项（#289-③ 回归门）。
/// 图标源 = Zed 上游 `assets/icons/`（commit `1663af41`，见
/// `third_party/zed/PROVENANCE.md`）——渲染按 alpha mask 重着色，源色
/// 不影响观感。
const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/chevron_down.svg",
        include_bytes!("../assets/icons/chevron_down.svg"),
    ),
    (
        "icons/chevron_right.svg",
        include_bytes!("../assets/icons/chevron_right.svg"),
    ),
    (
        "icons/close.svg",
        include_bytes!("../assets/icons/close.svg"),
    ),
    ("icons/file.svg", include_bytes!("../assets/icons/file.svg")),
    (
        "icons/file_tree.svg",
        include_bytes!("../assets/icons/file_tree.svg"),
    ),
    (
        "icons/folder.svg",
        include_bytes!("../assets/icons/folder.svg"),
    ),
    (
        "icons/folder_open.svg",
        include_bytes!("../assets/icons/folder_open.svg"),
    ),
    // #289-③：git 系图标（activity_bar 的 git 钮 + git_panel 提交行）
    // ——此前缺位，图标条 git 钮渲染为空白（用户报告「有一个按钮是
    // 透明的」的直接根因）。来源 = 上游同 commit assets/icons/（
    // 1663af41，见 third_party/zed/PROVENANCE.md 来源快照），口径与
    // 其余 15 枚一致。
    (
        "icons/git_branch.svg",
        include_bytes!("../assets/icons/git_branch.svg"),
    ),
    (
        "icons/git_commit.svg",
        include_bytes!("../assets/icons/git_commit.svg"),
    ),
    (
        "icons/list_tree.svg",
        include_bytes!("../assets/icons/list_tree.svg"),
    ),
    (
        "icons/magnifying_glass.svg",
        include_bytes!("../assets/icons/magnifying_glass.svg"),
    ),
    (
        "icons/maximize.svg",
        include_bytes!("../assets/icons/maximize.svg"),
    ),
    // #289-③：settings_panel 步进钮（+）——此前缺位。
    ("icons/plus.svg", include_bytes!("../assets/icons/plus.svg")),
    (
        "icons/settings.svg",
        include_bytes!("../assets/icons/settings.svg"),
    ),
    (
        "icons/square_minus.svg",
        include_bytes!("../assets/icons/square_minus.svg"),
    ),
    (
        "icons/threads_sidebar_left_closed.svg",
        include_bytes!("../assets/icons/threads_sidebar_left_closed.svg"),
    ),
    (
        "icons/threads_sidebar_left_open.svg",
        include_bytes!("../assets/icons/threads_sidebar_left_open.svg"),
    ),
    (
        "icons/threads_sidebar_right_closed.svg",
        include_bytes!("../assets/icons/threads_sidebar_right_closed.svg"),
    ),
    (
        "icons/threads_sidebar_right_open.svg",
        include_bytes!("../assets/icons/threads_sidebar_right_open.svg"),
    ),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let names = ICONS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_icons_load_non_empty() {
        for (path, bytes) in ICONS {
            assert!(!bytes.is_empty(), "{path} 内嵌为空");
            assert!(
                bytes.starts_with(b"<svg"),
                "{path} 不是 SVG：{:?}",
                &bytes[..16.min(bytes.len())]
            );
        }
    }

    #[test]
    fn load_returns_none_for_unknown_path() {
        assert!(Assets.load("icons/does_not_exist.svg").unwrap().is_none());
        assert!(Assets.load("fonts/whatever").unwrap().is_none());
    }

    #[test]
    fn list_root_covers_all_icons() {
        let all = Assets.list("icons/").unwrap();
        assert_eq!(all.len(), ICONS.len());
    }

    /// #289-③ 回归门：mang-shell 源码里实际渲染的**全部** IconName 变体
    /// 必须有内嵌资产——缺一枚 = 该按钮渲染成空白（用户报告的「透明
    /// 按钮」事故：git_branch/git_commit/plus/list_tree 四枚曾缺位）。
    ///
    /// 维护口径：在 src/ 里新增 `Icon::new(IconName::X)` 用法时，把 X
    /// 加进本清单（对齐 `rg -o "IconName::[A-Za-z0-9]+" src/ -N | sort -u`
    /// 的输出）。
    const SHELL_ICON_SET: &[ui::IconName] = &[
        ui::IconName::ChevronDown,
        ui::IconName::ChevronRight,
        ui::IconName::Close,
        ui::IconName::File,
        ui::IconName::FileTree,
        ui::IconName::Folder,
        ui::IconName::FolderOpen,
        ui::IconName::GitBranch,
        ui::IconName::GitCommit,
        ui::IconName::ListTree,
        ui::IconName::MagnifyingGlass,
        ui::IconName::Maximize,
        ui::IconName::Plus,
        ui::IconName::Settings,
        ui::IconName::SquareMinus,
        ui::IconName::ThreadsSidebarLeftClosed,
        ui::IconName::ThreadsSidebarLeftOpen,
        ui::IconName::ThreadsSidebarRightClosed,
        ui::IconName::ThreadsSidebarRightOpen,
    ];

    #[test]
    fn every_shell_icon_has_embedded_asset() {
        for name in SHELL_ICON_SET {
            let path = name.path().to_string();
            assert!(
                Assets.load(&path).unwrap().is_some(),
                "{path} 未内嵌——该图标会渲染成空白（透明按钮）\
                 （assets/icons/ 拷贝 SVG + ICONS 表加一行 + 本清单加一项）"
            );
        }
    }
}
