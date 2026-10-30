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
/// 每条对应 `assets/icons/` 下一个文件（15 个，r39 骨架所需全集）。
/// 新增图标 = 拷贝 SVG + 此表加一行 + `ICON_PATHS` 加一项。
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
    (
        "icons/magnifying_glass.svg",
        include_bytes!("../assets/icons/magnifying_glass.svg"),
    ),
    (
        "icons/maximize.svg",
        include_bytes!("../assets/icons/maximize.svg"),
    ),
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
}
