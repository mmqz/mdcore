//! 外壳主题设置 —— `ThemeSettingsProvider` 实现 + 配置落盘（F2 起）。
//!
//! `ui` 组件（`font_ui` / `text_ui_size` 等）会经 `theme::theme_settings(cx)`
//! 读取全局 provider，未注册时 panic（theme crate 契约）——本类型在启动时
//! 经 `theme::set_theme_settings_provider` 落位。
//!
//! 字体走 fontconfig 系统面：`Noto Sans SC`（本仓目标环境常备，含拉丁与
//! CJK 字形，Regular/Bold 双字重）。不内嵌字体文件——系统缺字时升级为
//! bundled 字体是后续 PR 的事。
//!
//! ## 配置落盘（F2：工作区记忆；F5 最近列表同宿主扩展）
//!
//! - 位置：`$XDG_CONFIG_HOME/mang/settings.json`（缺省 `$HOME/.config/mang`
//!   ——Linux 口径；Windows/macOS 平台目录规范随 F5 落盘件统一）；
//! - 格式：serde_json 对象（键渐进扩展——F2 仅 `workspace`）；
//! - 容错：目录不可建/不可写一律静默跳过（记忆是增强不是依赖——
//!   写失败不阻断打开工作区；读失败返回 None 走无工作区启动）。

use std::path::{Path, PathBuf};

use gpui::{App, Font, Pixels, px};
use theme::{ThemeSettingsProvider, UiDensity};

/// 配置目录（XDG 口径；不可得 → None——调用方静默跳过落盘/恢复）。
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("mang"))
}

fn settings_path_in(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

fn load_from(dir: &Path) -> serde_json::Value {
    std::fs::read_to_string(settings_path_in(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn save_to(dir: &Path, value: &serde_json::Value) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let _ = std::fs::write(settings_path_in(dir), text);
    }
}

/// G3：缩进单位读写（`indent_unit` 键："2" / "4" / "tab"——消费方
/// [`crate::editor::buffer::IndentUnit::from_setting`] 解析，未知值
/// 归 4 空格）。
pub fn save_indent_unit(key: &str) {
    if let Some(dir) = config_dir() {
        let mut value = load_from(&dir);
        value["indent_unit"] = serde_json::Value::String(key.to_string());
        save_to(&dir, &value);
    }
}

pub fn load_indent_unit() -> Option<String> {
    let dir = config_dir()?;
    load_from(&dir)
        .get("indent_unit")?
        .as_str()
        .map(|s| s.to_string())
}

/// 记住工作区（`workspace` 键——路径以 lossy 写入，非 UTF-8 路径的
/// 记忆为近似值[边界登记]）。
pub fn remember_workspace(root: &Path) {
    if let Some(dir) = config_dir() {
        remember_workspace_in(&dir, root);
    }
}

/// 恢复上次工作区（无记忆/读失败 → None）。
pub fn remembered_workspace() -> Option<PathBuf> {
    let dir = config_dir()?;
    remembered_workspace_in(&dir)
}

/// F6：主题模式读写（`theme` 键："system"/"light"/"dark"——未知值由
/// [`crate::theme_switch::parse_mode`] 归一到 system）。
pub fn save_theme_mode(mode: &str) {
    if let Some(dir) = config_dir() {
        let mut value = load_from(&dir);
        value["theme"] = serde_json::Value::String(mode.to_string());
        save_to(&dir, &value);
    }
}

pub fn load_theme_mode() -> Option<String> {
    let dir = config_dir()?;
    load_from(&dir)
        .get("theme")?
        .as_str()
        .map(|s| s.to_string())
}

/// F6：编辑器字号读写（`font_size` 键——pt 数；8-32 域外由消费方
/// clamp）。
pub fn save_font_size(size: f32) {
    if let Some(dir) = config_dir() {
        let mut value = load_from(&dir);
        value["font_size"] = serde_json::Value::from(size);
        save_to(&dir, &value);
    }
}

pub fn load_font_size() -> Option<f32> {
    let dir = config_dir()?;
    load_from(&dir).get("font_size")?.as_f64().map(|v| v as f32)
}

fn remember_workspace_in(dir: &Path, root: &Path) {
    let mut value = load_from(dir);
    value["workspace"] = serde_json::Value::String(root.to_string_lossy().into_owned());
    save_to(dir, &value);
}

fn remembered_workspace_in(dir: &Path) -> Option<PathBuf> {
    load_from(dir).get("workspace")?.as_str().map(PathBuf::from)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mang-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 工作区记忆往返（写入 → 读回 → 覆盖写 → 读回）。
    #[test]
    fn workspace_memory_roundtrip() {
        let dir = temp_config("roundtrip");
        let root = Path::new("/tmp/some-workspace");
        remember_workspace_in(&dir, root);
        assert_eq!(remembered_workspace_in(&dir).as_deref(), Some(root));

        let root2 = Path::new("/tmp/another-one");
        remember_workspace_in(&dir, root2);
        assert_eq!(
            remembered_workspace_in(&dir).as_deref(),
            Some(root2),
            "覆盖写生效"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 无记忆（目录空/文件不存在）→ None。
    #[test]
    fn missing_settings_returns_none() {
        let dir = temp_config("missing");
        assert_eq!(remembered_workspace_in(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 既有其它键不被工作区写入冲掉（渐进扩展契约——F5/F6 共用此文件）。
    #[test]
    fn other_keys_preserved() {
        let dir = temp_config("preserve");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            settings_path_in(&dir),
            r#"{ "theme": "dark", "workspace": "/old" }"#,
        )
        .unwrap();
        remember_workspace_in(&dir, Path::new("/new"));
        let value = load_from(&dir);
        assert_eq!(value["theme"].as_str(), Some("dark"), "既有键保留");
        assert_eq!(value["workspace"].as_str(), Some("/new"), "新值生效");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F6：主题/字号键往返 + 与既有键共存。
    #[test]
    fn theme_and_font_size_roundtrip() {
        let dir = temp_config("f6");
        remember_workspace_in(&dir, Path::new("/ws"));
        let mut value = load_from(&dir);
        value["theme"] = serde_json::Value::String("light".into());
        value["font_size"] = serde_json::Value::from(18.0f32);
        save_to(&dir, &value);

        let value = load_from(&dir);
        assert_eq!(value["theme"].as_str(), Some("light"));
        assert_eq!(value["font_size"].as_f64(), Some(18.0));
        assert_eq!(value["workspace"].as_str(), Some("/ws"), "既有键保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G3：缩进单位键往返 + 与既有键共存。
    /// G3：缩进单位键往返 + 与既有键共存。
    ///
    /// 注：`save_indent_unit`/`load_indent_unit` 走全局 `config_dir()`，
    /// 测试并行时互相污染（各用例写同一文件）。此处测底层
    /// `save_to`/`load_from` 的键往返语义（与生产同路径，无全局态）。
    #[test]
    fn indent_unit_roundtrip() {
        let dir = temp_config("g3");
        remember_workspace_in(&dir, Path::new("/ws"));
        for key in ["2", "4", "tab"] {
            let mut value = load_from(&dir);
            value["indent_unit"] = serde_json::Value::String(key.to_string());
            save_to(&dir, &value);
            let back = load_from(&dir);
            assert_eq!(back["indent_unit"].as_str(), Some(key), "键值往返 {key}");
        }
        let value = load_from(&dir);
        assert_eq!(value["workspace"].as_str(), Some("/ws"), "既有键保留");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
