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

/// G4：面板宽度读写（`panel_widths` 键：`{"left": px, "right": px}`
/// 对象——拖拽热区收尾时落盘；消费方钳制到面板拖拽域）。
pub fn save_panel_widths(left: f32, right: f32) {
    if let Some(dir) = config_dir() {
        let mut value = load_from(&dir);
        value["panel_widths"] = serde_json::json!({ "left": left, "right": right });
        save_to(&dir, &value);
    }
}

pub fn load_panel_widths() -> Option<(f32, f32)> {
    let dir = config_dir()?;
    let value = load_from(&dir);
    let widths = value.get("panel_widths")?;
    let left = widths.get("left")?.as_f64()? as f32;
    let right = widths.get("right")?.as_f64()? as f32;
    Some((left, right))
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

/// #217：键位覆盖读取（`keymap` 键：对象数组——`{"bind": "ctrl+alt+p",
/// "action": "toggle_command_palette"}` 换绑 / `{"unbind": "ctrl+shift+p"}`
/// 解绑，按序应用）。`keymap` 键存在但非数组 → stderr 警告后忽略
/// （#218 review：不再静默——配置类型错误应可发现）；结构不符的
/// **单条**跳过（stderr 警告——与 `keymap::KeymapStore::apply_user`
/// 的逐条容错同口径：一条坏配置不废掉全部自定义）；键串/动作名的
/// 合法性由 keymap 层检测并警告。解析体见 [`parse_keymap_entries`]
/// （纯函数——单测面）。
pub fn load_keymap_overrides() -> Vec<crate::keymap::UserOverride> {
    let Some(dir) = config_dir() else {
        return Vec::new();
    };
    parse_keymap_entries(&load_from(&dir))
}

/// `keymap` 键解析（纯函数——仅依赖 serde_json + std，`load_from`
/// 产物或任意等价 JSON 树）：
///
/// - 键缺失 → 空（静默——未配置即默认键位）；
/// - 键存在但非数组 → stderr 警告（类型中文名）+ 空；
/// - 数组 → 逐条解析（单条结构错误跳过 + 警告）。
fn parse_keymap_entries(value: &serde_json::Value) -> Vec<crate::keymap::UserOverride> {
    let Some(entries) = value.get("keymap") else {
        return Vec::new();
    };
    let Some(arr) = entries.as_array() else {
        eprintln!(
            "keymap: 配置应为数组（当前为{}），已忽略",
            json_type_name_cn(entries)
        );
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, entry) in arr.iter().enumerate() {
        let Some(obj) = entry.as_object() else {
            eprintln!("keymap: 第 {i} 条不是对象，已跳过");
            continue;
        };
        if let (Some(bind), Some(action)) = (
            obj.get("bind").and_then(|v| v.as_str()),
            obj.get("action").and_then(|v| v.as_str()),
        ) {
            out.push(crate::keymap::UserOverride::Bind {
                bind: bind.to_string(),
                action: action.to_string(),
            });
        } else if let Some(bind) = obj.get("unbind").and_then(|v| v.as_str()) {
            out.push(crate::keymap::UserOverride::Unbind {
                bind: bind.to_string(),
            });
        } else {
            eprintln!("keymap: 第 {i} 条缺 bind/action 或 unbind 键，已跳过");
        }
    }
    out
}

/// JSON 值类型中文名（keymap 非数组警告的「当前为{类型}」填充——
/// 与逐条警告「不是对象」同风格的中文可读面）。
fn json_type_name_cn(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "布尔",
        serde_json::Value::Number(_) => "数字",
        serde_json::Value::String(_) => "字符串",
        serde_json::Value::Array(_) => "数组",
        serde_json::Value::Object(_) => "对象",
    }
}

/// #132-3：折叠状态读写（`folds` 键：`{"<文件路径>": [起始行, ...]}`）。
///
/// 语义：**按文件记住「折叠头行的行号集合」**（0 基）。存头行而非区间，
/// 是因为区间尾行在编辑后会漂移；头行在同一块没被编辑时相对稳定，
/// 恢复时由调用方按当前文本重算区间尾（算不出就丢弃该条——宁缺毋错）。
///
/// 容量：每文件最多 [`FOLD_PERSIST_CAP`] 条、全局最多
/// [`FOLD_FILE_CAP`] 个文件（超限丢最旧写入的那个键，避免配置文件
/// 无限增长）。
pub const FOLD_PERSIST_CAP: usize = 200;
/// 全局最多记住多少个文件的折叠状态。
pub const FOLD_FILE_CAP: usize = 64;

pub fn save_folds(key: &str, heads: &[usize]) {
    if let Some(dir) = config_dir() {
        save_folds_in(&dir, key, heads);
    }
}

pub fn load_folds(key: &str) -> Vec<usize> {
    let Some(dir) = config_dir() else {
        return Vec::new();
    };
    load_folds_in(&dir, key)
}

fn save_folds_in(dir: &Path, key: &str, heads: &[usize]) {
    // 空键 = 未命名文档：不持久化（无稳定身份）。
    if key.is_empty() {
        return;
    }
    let mut value = load_from(dir);
    if value.get("folds").is_none_or(|v| !v.is_object()) {
        value["folds"] = serde_json::json!({});
    }
    let Some(map) = value["folds"].as_object_mut() else {
        return;
    };
    if heads.is_empty() {
        // 全展开 = 清掉该文件的记录（不积空数组垃圾）。
        map.remove(key);
    } else {
        let mut sorted: Vec<usize> = heads.iter().copied().take(FOLD_PERSIST_CAP).collect();
        sorted.sort_unstable();
        sorted.dedup();
        map.insert(
            key.to_string(),
            serde_json::Value::Array(
                sorted
                    .into_iter()
                    .map(|h| serde_json::Value::from(h as u64))
                    .collect(),
            ),
        );
        // 容量控制：超限时丢掉最旧写入的**别的**文件（Map 保插入序）。
        if map.len() > FOLD_FILE_CAP
            && let Some(oldest) = map.keys().find(|k| k.as_str() != key).cloned()
        {
            map.remove(&oldest);
        }
    }
    save_to(dir, &value);
}

fn load_folds_in(dir: &Path, key: &str) -> Vec<usize> {
    if key.is_empty() {
        return Vec::new();
    }
    let value = load_from(dir);
    let Some(arr) = value
        .get("folds")
        .and_then(|f| f.get(key))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    let mut out: Vec<usize> = arr
        .iter()
        .filter_map(|v| v.as_u64())
        .map(|v| v as usize)
        .take(FOLD_PERSIST_CAP)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn remember_workspace_in(dir: &Path, root: &Path) {
    let mut value = load_from(dir);
    value["workspace"] = serde_json::Value::String(root.to_string_lossy().into_owned());
    save_to(dir, &value);
}

fn remembered_workspace_in(dir: &Path) -> Option<PathBuf> {
    load_from(dir).get("workspace")?.as_str().map(PathBuf::from)
}

/// #132 任务 2：查找历史读写（`find_history` 键：字符串数组，新→旧）。
///
/// 容错口径与其余键一致：目录/文件不可得 → 静默跳过（历史是增强不是依赖）。
pub fn save_find_history(entries: &[String]) {
    if let Some(dir) = config_dir() {
        save_find_history_in(&dir, entries);
    }
}

pub fn load_find_history() -> Vec<String> {
    config_dir()
        .map(|dir| load_find_history_in(&dir))
        .unwrap_or_default()
}

/// #132 任务 2：正则开关读写（`find_regex` 键：布尔）。
pub fn save_find_regex(on: bool) {
    if let Some(dir) = config_dir() {
        save_find_regex_in(&dir, on);
    }
}

pub fn load_find_regex() -> bool {
    config_dir()
        .map(|dir| load_find_regex_in(&dir))
        .unwrap_or(false)
}

// ---- 目录显式传入的底层实现（单测用——并行测试不互相污染全局态） ----

fn save_find_history_in(dir: &Path, entries: &[String]) {
    let mut value = load_from(dir);
    value["find_history"] = serde_json::Value::Array(
        entries
            .iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect(),
    );
    save_to(dir, &value);
}

fn load_find_history_in(dir: &Path) -> Vec<String> {
    load_from(dir)
        .get("find_history")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn save_find_regex_in(dir: &Path, on: bool) {
    let mut value = load_from(dir);
    value["find_regex"] = serde_json::Value::Bool(on);
    save_to(dir, &value);
}

fn load_find_regex_in(dir: &Path) -> bool {
    load_from(dir)
        .get("find_regex")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
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

    /// G4：面板宽度键往返 + 与既有键共存（含覆盖写）。
    ///
    /// 注：与 `indent_unit_roundtrip` 同理——`save_panel_widths`/
    /// `load_panel_widths` 走全局 `config_dir()`，并行测试互相污染；
    /// 此处测底层 `save_to`/`load_from` 的 `panel_widths` 键往返语义
    ///（与生产同路径，无全局态）。
    #[test]
    fn panel_widths_roundtrip() {
        let dir = temp_config("g4");
        remember_workspace_in(&dir, Path::new("/ws"));
        for (left, right) in [(300., 340.), (500., 600.)] {
            let mut value = load_from(&dir);
            value["panel_widths"] = serde_json::json!({ "left": left, "right": right });
            save_to(&dir, &value);
            let back = load_from(&dir);
            assert_eq!(
                back["panel_widths"]["left"].as_f64(),
                Some(left),
                "左宽往返"
            );
            assert_eq!(
                back["panel_widths"]["right"].as_f64(),
                Some(right),
                "右宽往返（第二轮即覆盖写生效）"
            );
        }
        let value = load_from(&dir);
        assert_eq!(value["workspace"].as_str(), Some("/ws"), "既有键保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #132-3：折叠状态键往返（头行号数组 + 与既有键共存 + 空列表即删除）。
    ///
    /// 注：`save_folds`/`load_folds` 走全局 `config_dir()`，并行测试互相
    /// 污染；此处测底层 `save_folds_in`/`load_folds_in`（与生产同路径，
    /// 无全局态）。
    #[test]
    fn folds_roundtrip_per_file() {
        let dir = temp_config("fold");
        remember_workspace_in(&dir, Path::new("/ws"));
        save_folds_in(&dir, "/ws/a.md", &[7, 0, 3, 3]);
        save_folds_in(&dir, "/ws/b.md", &[1]);
        assert_eq!(load_folds_in(&dir, "/ws/a.md"), vec![0, 3, 7], "排序去重");
        assert_eq!(load_folds_in(&dir, "/ws/b.md"), vec![1], "按文件隔离");
        assert_eq!(
            load_folds_in(&dir, "/ws/c.md"),
            Vec::<usize>::new(),
            "未知文件空"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #132：查找历史键往返 + 截断语义（超 20 条由消费方截——本层如实存）。
    #[test]
    fn find_history_roundtrip_and_coexist() {
        let dir = temp_config("find-history");
        remember_workspace_in(&dir, Path::new("/ws"));
        let entries: Vec<String> = vec!["\\bfn\\s+\\w+".into(), "foo".into(), "中文".into()];
        save_find_history_in(&dir, &entries);
        assert_eq!(
            load_find_history_in(&dir),
            entries,
            "历史往返（含正则/中文）"
        );

        // 覆盖写：新历史替换旧历史
        save_find_history_in(&dir, &["bar".to_string()]);
        assert_eq!(load_find_history_in(&dir), vec!["bar".to_string()]);

        // 既有键保留
        let value = load_from(&dir);
        assert_eq!(value["workspace"].as_str(), Some("/ws"), "既有键保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #132：正则开关键往返 + 缺省 false + 与既有键共存。
    #[test]
    fn find_regex_roundtrip() {
        let dir = temp_config("find-regex");
        assert!(!load_find_regex_in(&dir), "缺省关闭");
        save_find_regex_in(&dir, true);
        assert!(load_find_regex_in(&dir));
        save_find_regex_in(&dir, false);
        assert!(!load_find_regex_in(&dir), "覆盖写生效");

        remember_workspace_in(&dir, Path::new("/ws"));
        save_find_regex_in(&dir, true);
        let value = load_from(&dir);
        assert_eq!(value["workspace"].as_str(), Some("/ws"), "既有键保留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 空头行集合 = 删除该文件记录（全展开后不留垃圾）。
    #[test]
    fn folds_empty_clears_file_entry() {
        let dir = temp_config("fold-clear");
        save_folds_in(&dir, "/ws/a.md", &[2, 5]);
        assert_eq!(load_folds_in(&dir, "/ws/a.md"), vec![2, 5]);
        save_folds_in(&dir, "/ws/a.md", &[]);
        assert_eq!(load_folds_in(&dir, "/ws/a.md"), Vec::<usize>::new());
        let value = load_from(&dir);
        assert!(
            value.get("folds").and_then(|f| f.get("/ws/a.md")).is_none(),
            "空列表应删除键：{value}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 未命名文档（空键）不落盘——无稳定身份。
    #[test]
    fn folds_empty_key_is_not_persisted() {
        let dir = temp_config("fold-nokey");
        save_folds_in(&dir, "", &[1, 2]);
        assert_eq!(load_folds_in(&dir, ""), Vec::<usize>::new());
        assert!(
            load_from(&dir).get("folds").is_none(),
            "空键不产生 folds 段"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #217/#218：keymap 键缺失 → 空（静默——未配置即默认键位；
    /// 空对象 / 整树 null / 只有别的键 同口径）。
    #[test]
    fn keymap_entries_missing_key_is_silent_empty() {
        assert!(parse_keymap_entries(&serde_json::json!({})).is_empty());
        assert!(parse_keymap_entries(&serde_json::Value::Null).is_empty());
        assert!(
            parse_keymap_entries(&serde_json::json!({ "theme": "dark", "font_size": 15 }))
                .is_empty()
        );
    }

    /// #218 review：keymap 键存在但非数组 → 警告 + 空（五种非数组类型
    /// 全覆盖——对象/字符串/数字/布尔/null；不再静默吞掉）。
    #[test]
    fn keymap_entries_non_array_types_warn_and_empty() {
        for value in [
            serde_json::json!({ "keymap": { "bind": "ctrl+alt+p" } }),
            serde_json::json!({ "keymap": "ctrl+alt+p" }),
            serde_json::json!({ "keymap": 42 }),
            serde_json::json!({ "keymap": true }),
            serde_json::json!({ "keymap": null }),
        ] {
            assert!(
                parse_keymap_entries(&value).is_empty(),
                "非数组 keymap 应解析为空：{value}"
            );
        }
    }

    /// #217：正常条目解析（bind/unbind 两形态）+ 坏条目混排逐条容错
    /// （非对象 / 缺键 / 值类型错——好条目照常产出，坏条目跳过）。
    #[test]
    fn keymap_entries_parses_good_and_skips_bad_mixed() {
        let value = serde_json::json!({
            "keymap": [
                { "bind": "ctrl+alt+p", "action": "toggle_command_palette" },
                "not an object",
                { "unbind": "ctrl+shift+p" },
                { "missing": "fields" },
                { "bind": 3, "action": "save" },
                { "unbind": 7 }
            ]
        });
        assert_eq!(
            parse_keymap_entries(&value),
            vec![
                crate::keymap::UserOverride::Bind {
                    bind: "ctrl+alt+p".into(),
                    action: "toggle_command_palette".into()
                },
                crate::keymap::UserOverride::Unbind {
                    bind: "ctrl+shift+p".into()
                },
            ],
            "好条目按序产出，坏条目（非对象/缺键/值类型错）跳过"
        );
    }
}
