//! R24-a — 设置面核侧语义锁（主档 §31 R24-a 四条机制合同 + 加固项）。
//!
//! 锁清单：
//! 1. 默认合并：部分文件缺键 → 默认值兜底（Clean，非异常）；
//! 2. 读写往返 + 未知键/注释保留（前向兼容的字节级证据）；
//! 3. 损坏降级：TOML 解析失败 → 全默认 FileDegraded（save 可修复）；
//! 4. 键级降级：类型不符/值越界 → 该键回默认，其余键不受影响；
//! 5. 版本迁移：v0（无版本键）→ v1 补键（Migrated{from:0}）；
//! 6. 未来 schema：全默认 + save 拒写（防降写毁数据）；
//! 7. set 面：已知键封闭集 + 类型/值域校验 + doc↔快照同步 + 重载往返；
//! 8. 文件面：不存在=Created / 原子写往返 / 与核心常量对齐（COALESCE_MS）；
//! 9. JSON 形状：serde 往返稳定（web `__settings()` 面的 Rust 侧锚）。

use std::path::PathBuf;

use mdshell::settings::{
    default_document, parse_settings, LoadStatus, SaveError, SettingError, SettingValue,
    Settings, SettingsStore, SCHEMA_VERSION,
};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdshell-r24a-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

// ---------------------------------------------------------------------------
// 1. 默认合并
// ---------------------------------------------------------------------------

#[test]
fn partial_file_merges_defaults_clean() {
    // 只有三个键的部分 sidecar（用户手写/旧版本形态）。
    let text = "schema_version = 1\n\n[editor]\nundo_coalesce_ms = 2500\n\n[appearance]\ntheme = \"dark\"\n";
    let (s, status) = parse_settings(text);
    assert_eq!(status, LoadStatus::Clean, "缺键=默认合并，是设计行为非异常");
    assert_eq!(s.editor.undo_coalesce_ms, 2500, "显式键保留");
    assert_eq!(s.appearance.theme, "dark");
    // 缺键全部落默认
    assert_eq!(s.editor.progressive_open_threshold_bytes, Settings::default().editor.progressive_open_threshold_bytes);
    assert_eq!(s.ai.commands_enabled, Settings::default().ai.commands_enabled);
    assert_eq!(s.storage, Settings::default().storage);
    assert_eq!(s.appearance.font_size_px, Settings::default().appearance.font_size_px);
}

// ---------------------------------------------------------------------------
// 2. 未知键/注释保留（前向兼容）
// ---------------------------------------------------------------------------

#[test]
fn unknown_keys_and_comments_survive_roundtrip() {
    // 模拟"未来版本写入的文件"：含本版本不认识的键与注释。
    let text = "# user header comment\n\
                schema_version = 1\n\
                unknown_top_level = \"keep me\"\n\n\
                [editor]\n\
                undo_coalesce_ms = 1800  # inline comment\n\
                future_editor_knob = 42\n\n\
                [future_section]\n\
                rocket = \"moon\"\n";
    let mut store = SettingsStore::from_content(text);
    assert_eq!(*store.status(), LoadStatus::Clean, "未知键不降级——原样保留");

    // set 一个已知键后重写：未知键/注释/排版必须原样在场。
    store
        .set("appearance.theme", SettingValue::Str("light".into()))
        .expect("set known key");
    let out = store.to_toml().expect("serialize");
    assert!(out.contains("unknown_top_level = \"keep me\""), "顶层未知键保留");
    assert!(out.contains("future_editor_knob = 42"), "已知节内未知键保留");
    assert!(out.contains("[future_section]"), "未来节保留");
    assert!(out.contains("rocket = \"moon\""), "未来节键值保留");
    assert!(out.contains("# user header comment"), "头部注释保留");
    assert!(out.contains("# inline comment"), "行内注释保留");
    assert!(out.contains("undo_coalesce_ms = 1800"), "未触碰的已知键原样");
    assert!(out.contains("theme = \"light\""), "set 写入生效");
}

// ---------------------------------------------------------------------------
// 3. 损坏降级（文件级）
// ---------------------------------------------------------------------------

#[test]
fn corrupt_file_degrades_to_defaults_and_save_repairs() {
    let dir = tmp_dir("corrupt");
    let path = dir.join("settings.toml");
    std::fs::write(&path, "not [valid toml {{{{").expect("write corrupt");

    let store = SettingsStore::load(&path);
    assert!(
        matches!(store.status(), LoadStatus::FileDegraded { .. }),
        "解析失败必须降级而非 panic：{:?}",
        store.status()
    );
    assert_eq!(*store.settings(), Settings::default(), "降级=全默认");

    // save 修复重写 → 重载 Clean。
    store.save(&path).expect("save repairs");
    let repaired = SettingsStore::load(&path);
    assert_eq!(*repaired.status(), LoadStatus::Clean, "修复后重载 Clean");
    assert_eq!(*repaired.settings(), Settings::default());
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 4. 键级降级（类型/值域）
// ---------------------------------------------------------------------------

#[test]
fn wrong_typed_key_degrades_that_key_only() {
    let text = "schema_version = 1\n\n[editor]\nundo_coalesce_ms = \"fast\"\nprogressive_open_threshold_bytes = 2097152\n";
    let (s, status) = parse_settings(text);
    match status {
        LoadStatus::KeyDegraded { ref path, .. } => assert_eq!(path, "editor.undo_coalesce_ms"),
        other => panic!("expected KeyDegraded, got {other:?}"),
    }
    assert_eq!(s.editor.undo_coalesce_ms, Settings::default().editor.undo_coalesce_ms, "坏键回默认");
    assert_eq!(s.editor.progressive_open_threshold_bytes, 2097152, "其余键不受影响");
}

#[test]
fn out_of_range_value_degrades_that_key_only() {
    let text = "schema_version = 1\n\n[appearance]\nfont_size_px = 40000\ntheme = \"dark\"\n";
    let (s, status) = parse_settings(text);
    match status {
        LoadStatus::KeyDegraded { ref path, .. } => assert_eq!(path, "appearance.font_size_px"),
        other => panic!("expected KeyDegraded, got {other:?}"),
    }
    assert_eq!(s.appearance.font_size_px, Settings::default().appearance.font_size_px);
    assert_eq!(s.appearance.theme, "dark");
}

#[test]
fn invalid_theme_enum_degrades_to_system() {
    let text = "schema_version = 1\n\n[appearance]\ntheme = \"blue\"\n";
    let (s, status) = parse_settings(text);
    assert!(matches!(status, LoadStatus::KeyDegraded { .. }), "非法枚举=键级降级");
    assert_eq!(s.appearance.theme, "system");
}

// ---------------------------------------------------------------------------
// 5. 版本迁移
// ---------------------------------------------------------------------------

#[test]
fn v0_file_without_version_key_migrates_to_v1() {
    // v0 = 首版 rollout 文件形态：无 schema_version 键，其余合法。
    let text = "[editor]\nundo_coalesce_ms = 3200\n";
    let store = SettingsStore::from_content(text);
    assert_eq!(*store.status(), LoadStatus::Migrated { from: 0 }, "v0→v1 迁移");

    let out = store.to_toml().expect("serialize migrated");
    assert!(out.contains("schema_version = 1"), "重写补版本键");
    assert!(out.contains("undo_coalesce_ms = 3200"), "用户值不丢");

    // 迁移重写后重载 = Clean（幂等）。
    let reloaded = SettingsStore::from_content(&out);
    assert_eq!(*reloaded.status(), LoadStatus::Clean);
    assert_eq!(reloaded.settings().editor.undo_coalesce_ms, 3200);
}

// ---------------------------------------------------------------------------
// 6. 未来 schema（拒写保护）
// ---------------------------------------------------------------------------

#[test]
fn future_schema_loads_defaults_and_refuses_save() {
    let text = "schema_version = 99\n\n[editor]\nundo_coalesce_ms = 7777\n";
    let dir = tmp_dir("future");
    let path = dir.join("settings.toml");
    std::fs::write(&path, text).expect("write future schema");

    let mut store = SettingsStore::load(&path);
    assert_eq!(*store.status(), LoadStatus::FutureSchema { found: 99 });
    assert_eq!(*store.settings(), Settings::default(), "未来版本不解释任何键");

    match store.save(&path) {
        Err(SaveError::FutureSchema { found: 99, current }) => assert_eq!(current, SCHEMA_VERSION),
        other => panic!("save must refuse future schema, got {other:?}"),
    }
    match store.set("editor.undo_coalesce_ms", SettingValue::Uint(1000)) {
        Err(SettingError::UnknownPath(_)) => {}
        other => panic!("set on future-schema file must refuse, got {other:?}"),
    }
    // 原文件字节未动（拒写 = 不落盘）。
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 7. set 面（封闭键集 + 校验 + 同步 + 往返）
// ---------------------------------------------------------------------------

#[test]
fn set_updates_doc_snapshot_and_survives_reload() {
    let mut store = SettingsStore::new_default();
    store.set("editor.undo_coalesce_ms", SettingValue::Uint(4000)).unwrap();
    store.set("storage.autosave_enabled", SettingValue::Bool(true)).unwrap();
    store.set("storage.autosave_interval_ms", SettingValue::Uint(120_000)).unwrap();
    store.set("appearance.theme", SettingValue::Str("dark".into())).unwrap();
    store.set("appearance.font_size_px", SettingValue::Uint(18)).unwrap();
    store.set("ai.commands_enabled", SettingValue::Bool(false)).unwrap();

    // 快照即时同步。
    assert_eq!(store.settings().editor.undo_coalesce_ms, 4000);
    assert!(store.settings().storage.autosave_enabled);
    assert_eq!(store.settings().appearance.theme, "dark");

    // 序列化 → 重载 → 值保持（doc 与快照无漂移）。
    let out = store.to_toml().unwrap();
    let reloaded = SettingsStore::from_content(&out);
    assert_eq!(*reloaded.settings(), *store.settings(), "doc↔快照↔重载三方一致");
    assert_eq!(*reloaded.status(), LoadStatus::Clean);
}

#[test]
fn set_rejects_unknown_path_and_reserved_keys() {
    let mut store = SettingsStore::new_default();
    assert!(matches!(
        store.set("editor.nonexistent", SettingValue::Uint(1)),
        Err(SettingError::UnknownPath(_))
    ));
    assert!(matches!(
        store.set("schema_version", SettingValue::Uint(2)),
        Err(SettingError::UnknownPath(_)) // 保留键：迁移框架独占
    ));
    assert!(matches!(
        store.set("no-dot-path", SettingValue::Bool(true)),
        Err(SettingError::UnknownPath(_))
    ));
    assert!(matches!(
        store.set("future.section.key", SettingValue::Uint(1)),
        Err(SettingError::UnknownPath(_))
    ));
    // Err 后文档零触碰。
    assert_eq!(store.to_toml().unwrap(), default_document().to_string());
}

#[test]
fn set_rejects_wrong_types_and_out_of_range() {
    let mut store = SettingsStore::new_default();
    assert!(matches!(
        store.set("editor.undo_coalesce_ms", SettingValue::Str("1000".into())),
        Err(SettingError::WrongType { .. })
    ));
    assert!(matches!(
        store.set("appearance.theme", SettingValue::Uint(1)),
        Err(SettingError::WrongType { .. })
    ));
    assert!(matches!(
        store.set("editor.undo_coalesce_ms", SettingValue::Uint(0)),
        Err(SettingError::OutOfRange { .. }) // 下界 1
    ));
    assert!(matches!(
        store.set("appearance.font_size_px", SettingValue::Uint(99)),
        Err(SettingError::OutOfRange { .. }) // 上界 48
    ));
    assert!(matches!(
        store.set("storage.autosave_interval_ms", SettingValue::Uint(500)),
        Err(SettingError::OutOfRange { .. }) // 下界 1000
    ));
    assert!(matches!(
        store.set("appearance.theme", SettingValue::Str("blue".into())),
        Err(SettingError::OutOfRange { .. }) // 枚举外
    ));
    assert_eq!(store.to_toml().unwrap(), default_document().to_string(), "全部拒绝=零触碰");
}

// ---------------------------------------------------------------------------
// 8. 文件面
// ---------------------------------------------------------------------------

#[test]
fn missing_file_is_created_status_and_save_persists_full_defaults() {
    let dir = tmp_dir("missing");
    let path = dir.join("nested").join("settings.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    let store = SettingsStore::load(&path);
    assert_eq!(*store.status(), LoadStatus::Created);
    store.save(&path).expect("first save");

    let text = std::fs::read_to_string(&path).unwrap();
    // 首次落盘 = 完整默认文档（人可读全键，而非稀疏骨架）。
    assert!(text.contains("[editor]"));
    assert!(text.contains("[ai]"));
    assert!(text.contains("[storage]"));
    assert!(text.contains("[appearance]"));
    assert!(text.contains("session_restore = true"));

    let reloaded = SettingsStore::load(&path);
    assert_eq!(*reloaded.status(), LoadStatus::Clean);
    assert_eq!(*reloaded.settings(), Settings::default());
    // 无 .tmp 残留（原子写收尾干净）。
    assert!(!path.with_extension("toml.tmp").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn defaults_align_with_core_constants() {
    // 跨 crate 常量锁：settings 默认 = 核合并窗常量（防双侧漂移）。
    assert_eq!(Settings::default().editor.undo_coalesce_ms, mdcore::history::COALESCE_MS);
}

// ---------------------------------------------------------------------------
// 9. JSON 形状（web `__settings()` 面的 Rust 侧锚）
// ---------------------------------------------------------------------------

#[test]
fn settings_json_shape_roundtrips() {
    let s = Settings::default();
    let json = serde_json::to_string(&s).expect("serialize");
    assert!(json.contains("\"schema_version\":1"), "版本键在 JSON 面");
    assert!(json.contains("\"undo_coalesce_ms\":1000"), "COALESCE_MS 默认值进 JSON");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, s);

    // 结构面：四层 section 全在场（web contract gate 同款断言的 Rust 锚）。
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    for section in ["editor", "ai", "storage", "appearance"] {
        assert!(v.get(section).is_some(), "{section} 层必须在 JSON 面");
    }
}
