//! 外壳主题设置 —— `ThemeSettingsProvider` 实现。
//!
//! `ui` 组件（`font_ui` / `text_ui_size` 等）会经 `theme::theme_settings(cx)`
//! 读取全局 provider，未注册时 panic（theme crate 契约）——本类型在启动时
//! 经 `theme::set_theme_settings_provider` 落位。
//!
//! 字体走 fontconfig 系统面：`Noto Sans SC`（本仓目标环境常备，含拉丁与
//! CJK 字形，Regular/Bold 双字重）。不内嵌字体文件——系统缺字时升级为
//! bundled 字体是后续 PR 的事。

use gpui::{App, Font, Pixels, px};
use theme::{ThemeSettingsProvider, UiDensity};

/// UI 字体（界面元素）。
pub fn ui_font() -> &'static Font {
    static FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
    FONT.get_or_init(|| Font {
        family: "Noto Sans SC".into(),
        ..Font::default()
    })
}

/// 缓冲区字体（编辑区/等宽面）。
pub fn buffer_font() -> &'static Font {
    static FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
    FONT.get_or_init(|| Font {
        family: "DejaVu Sans Mono".into(),
        ..Font::default()
    })
}

pub struct ShellThemeSettings;

impl ThemeSettingsProvider for ShellThemeSettings {
    fn ui_font<'a>(&'a self, _cx: &'a App) -> &'a Font {
        ui_font()
    }

    fn buffer_font<'a>(&'a self, _cx: &'a App) -> &'a Font {
        buffer_font()
    }

    fn ui_font_size(&self, _cx: &App) -> Pixels {
        px(15.)
    }

    fn buffer_font_size(&self, _cx: &App) -> Pixels {
        px(14.)
    }

    fn ui_density(&self, _cx: &App) -> UiDensity {
        UiDensity::default()
    }
}
