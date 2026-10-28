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
//! 9. JSON 形状：serde 往返稳定（web `__settings()` 面的 Rust 侧锚）；
//! 10. R24-a1 修复锁：写读形态对称——节缺失 set 产出标准表（非 inline
//!     表）且重载存活；inline 节升格保值；读侧容错认 inline 节。

use std::path::PathBuf;

use mdshell::settings::{
    default_document, parse_settings, LoadStatus, SaveError, SettingError, SettingValue, Settings,
    SettingsStore, SCHEMA_VERSION,
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
    assert_eq!(
        s.editor.progressive_open_threshold_bytes,
        Settings::default().editor.progressive_open_threshold_bytes
    );
    assert_eq!(
        s.ai.commands_enabled,
        Settings::default().ai.commands_enabled
    );
    assert_eq!(s.storage, Settings::default().storage);
    assert_eq!(
        s.appearance.font_size_px,
        Settings::default().appearance.font_size_px
    );
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
    assert!(
        out.contains("unknown_top_level = \"keep me\""),
        "顶层未知键保留"
    );
    assert!(
        out.contains("future_editor_knob = 42"),
        "已知节内未知键保留"
    );
    assert!(out.contains("[future_section]"), "未来节保留");
    assert!(out.contains("rocket = \"moon\""), "未来节键值保留");
    assert!(out.contains("# user header comment"), "头部注释保留");
    assert!(out.contains("# inline comment"), "行内注释保留");
    assert!(
        out.contains("undo_coalesce_ms = 1800"),
        "未触碰的已知键原样"
    );
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
    assert_eq!(
        s.editor.undo_coalesce_ms,
        Settings::default().editor.undo_coalesce_ms,
        "坏键回默认"
    );
    assert_eq!(
        s.editor.progressive_open_threshold_bytes, 2097152,
        "其余键不受影响"
    );
}

#[test]
fn out_of_range_value_degrades_that_key_only() {
    let text = "schema_version = 1\n\n[appearance]\nfont_size_px = 40000\ntheme = \"dark\"\n";
    let (s, status) = parse_settings(text);
    match status {
        LoadStatus::KeyDegraded { ref path, .. } => assert_eq!(path, "appearance.font_size_px"),
        other => panic!("expected KeyDegraded, got {other:?}"),
    }
    assert_eq!(
        s.appearance.font_size_px,
        Settings::default().appearance.font_size_px
    );
    assert_eq!(s.appearance.theme, "dark");
}

#[test]
fn invalid_theme_enum_degrades_to_system() {
    let text = "schema_version = 1\n\n[appearance]\ntheme = \"blue\"\n";
    let (s, status) = parse_settings(text);
    assert!(
        matches!(status, LoadStatus::KeyDegraded { .. }),
        "非法枚举=键级降级"
    );
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
    assert_eq!(
        *store.status(),
        LoadStatus::Migrated { from: 0 },
        "v0→v1 迁移"
    );

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
    assert_eq!(
        *store.settings(),
        Settings::default(),
        "未来版本不解释任何键"
    );

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
    store
        .set("editor.undo_coalesce_ms", SettingValue::Uint(4000))
        .unwrap();
    store
        .set("storage.autosave_enabled", SettingValue::Bool(true))
        .unwrap();
    store
        .set("storage.autosave_interval_ms", SettingValue::Uint(120_000))
        .unwrap();
    store
        .set("appearance.theme", SettingValue::Str("dark".into()))
        .unwrap();
    store
        .set("appearance.font_size_px", SettingValue::Uint(18))
        .unwrap();
    store
        .set("ai.commands_enabled", SettingValue::Bool(false))
        .unwrap();

    // 快照即时同步。
    assert_eq!(store.settings().editor.undo_coalesce_ms, 4000);
    assert!(store.settings().storage.autosave_enabled);
    assert_eq!(store.settings().appearance.theme, "dark");

    // 序列化 → 重载 → 值保持（doc 与快照无漂移）。
    let out = store.to_toml().unwrap();
    let reloaded = SettingsStore::from_content(&out);
    assert_eq!(
        *reloaded.settings(),
        *store.settings(),
        "doc↔快照↔重载三方一致"
    );
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
    assert_eq!(
        store.to_toml().unwrap(),
        default_document().to_string(),
        "全部拒绝=零触碰"
    );
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
    assert_eq!(
        Settings::default().editor.undo_coalesce_ms,
        mdcore::history::COALESCE_MS
    );
}

// ---------------------------------------------------------------------------
// 9. JSON 形状（web `__settings()` 面的 Rust 侧锚）
// ---------------------------------------------------------------------------

#[test]
fn settings_json_shape_roundtrips() {
    let s = Settings::default();
    let json = serde_json::to_string(&s).expect("serialize");
    assert!(json.contains("\"schema_version\":1"), "版本键在 JSON 面");
    assert!(
        json.contains("\"undo_coalesce_ms\":1000"),
        "COALESCE_MS 默认值进 JSON"
    );
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, s);

    // 结构面：四层 section 全在场（web contract gate 同款断言的 Rust 锚）。
    // #16（A4）：第五层 docx_export（v1.2 增补）。
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    for section in ["editor", "ai", "storage", "appearance", "docx_export"] {
        assert!(v.get(section).is_some(), "{section} 层必须在 JSON 面");
    }
}

// ---------------------------------------------------------------------------
// #16（A4）：docx 导出格式层四键（CONTRACT §4 v1.2 增补）。
// ---------------------------------------------------------------------------

#[test]
fn tr16_docx_export_keys_set_reload_and_domain() {
    let mut store = SettingsStore::new_default();
    // 全链：预设 → 自定义三值逐键写 → 快照 → 落盘重载读回。
    assert!(store
        .set(
            "docx_export.preset",
            SettingValue::Str("official-a4".into())
        )
        .is_ok());
    assert!(store
        .set("docx_export.preset", SettingValue::Str("custom".into()))
        .is_ok());
    assert!(store
        .set("docx_export.size_half_points", SettingValue::Uint(24))
        .is_ok());
    assert!(store
        .set("docx_export.line_spacing", SettingValue::Uint(480))
        .is_ok());
    assert!(store
        .set("docx_export.margin_twips", SettingValue::Uint(1440))
        .is_ok());
    let dir = tmp_dir("tr16");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.toml");
    store.save(&path).expect("save");
    let reloaded = SettingsStore::load(&path);
    let s = reloaded.settings();
    assert_eq!(s.docx_export.preset, "custom");
    assert_eq!(s.docx_export.size_half_points, 24);
    assert_eq!(s.docx_export.line_spacing, 480);
    assert_eq!(s.docx_export.margin_twips, 1440);
    std::fs::remove_dir_all(&dir).ok();

    // 值域 typed 拒（枚举外预设/离散域外三值）+ 零触碰。
    let mut store2 = SettingsStore::new_default();
    assert!(matches!(
        store2.set("docx_export.preset", SettingValue::Str("a4".into())),
        Err(SettingError::OutOfRange { .. })
    ));
    assert!(matches!(
        store2.set("docx_export.preset", SettingValue::Uint(1)),
        Err(SettingError::WrongType { .. })
    ));
    for (path, bad) in [
        ("docx_export.size_half_points", 22u64),
        ("docx_export.line_spacing", 300u64),
        ("docx_export.margin_twips", 2000u64),
    ] {
        assert!(
            matches!(
                store2.set(path, SettingValue::Uint(bad)),
                Err(SettingError::OutOfRange { .. })
            ),
            "{path}={bad} 必须越域拒"
        );
    }
    assert_eq!(
        store2.to_toml().unwrap(),
        default_document().to_string(),
        "全部拒绝=零触碰"
    );

    // 文件面越域降级：越域值落盘后重载 → 该键回默认 + KeyDegraded。
    let dir2 = tmp_dir("tr16b");
    std::fs::create_dir_all(&dir2).unwrap();
    let path2 = dir2.join("settings.toml");
    std::fs::write(
        &path2,
        "schema_version = 1\n\n[docx_export]\npreset = \"unknown\"\nsize_half_points = 99\n",
    )
    .unwrap();
    let degraded = SettingsStore::load(&path2);
    let s2 = degraded.settings();
    assert_eq!(s2.docx_export.preset, "default", "枚举外回默认");
    assert_eq!(s2.docx_export.size_half_points, 28, "域外回默认");
    assert_eq!(s2.docx_export.line_spacing, 360);
    assert_eq!(s2.docx_export.margin_twips, 1800);
    assert!(matches!(
        degraded.status(),
        mdshell::settings::LoadStatus::KeyDegraded { .. }
    ));
    std::fs::remove_dir_all(&dir2).ok();
}

// ---------------------------------------------------------------------------
// 10. R24-a1 修复锁：写读形态对称（set 在节缺失/inline 形态上的落盘形态）
// ---------------------------------------------------------------------------

/// R24-a1 主锁（bug 复现路径）：sidecar 的 `[editor]` 整节缺失（缺键 =
/// 默认是设计行为）→ `set()` 已知键 → 落盘必须是**标准表头**形态，且
/// 重载读回写入值。修复前：toml_edit 的 IndexMut 把缺失节自动升格为
/// **inline 表**（`editor = { undo_coalesce_ms = 500 }`），而读侧
/// `as_table()` 只认标准表 → 值落盘但下次 load 静默回默认（设置丢失）。
#[test]
fn set_on_missing_section_writes_standard_table_and_survives_reload() {
    let text = "schema_version = 1\n\n[appearance]\ntheme = \"dark\"\n";
    let mut store = SettingsStore::from_content(text);
    assert!(matches!(store.status(), LoadStatus::Clean), "缺节=默认合并");

    store
        .set("editor.undo_coalesce_ms", SettingValue::Uint(500))
        .expect("节缺失时 set 必须成功（默认合并语义下节按需创建）");
    let out = store.to_toml().expect("序列化");

    assert!(
        out.contains("[editor]"),
        "写入形态必须是标准表头，不得是 inline 表"
    );
    assert!(
        !out.contains("editor = {"),
        "inline 形态 = 读不回 = 静默丢设置（R24-a1 修复点）"
    );
    assert!(out.contains("undo_coalesce_ms = 500"));

    // 往返闭环：load 必须读回写入值（修复前此处回默认 1000）。
    let (s2, st2) = parse_settings(&out);
    assert_eq!(st2, LoadStatus::Clean);
    assert_eq!(s2.editor.undo_coalesce_ms, 500, "写入值必须在重载后存活");
    // 其余键不受影响
    assert_eq!(s2.appearance.theme, "dark");
    assert_eq!(
        s2.storage,
        Settings::default().storage,
        "未触碰的缺失节照常默认合并"
    );
}

/// R24-a1 升格锁：文件中已存在 inline 节（手写 / 旧版本产物）→ `set()`
/// 该节键 → 节**升格**为标准表且**值保留**（含未知键——前向兼容不丢字节）。
#[test]
fn set_promotes_inline_section_preserving_values() {
    let text = concat!(
        "schema_version = 1\n",
        "editor = { undo_coalesce_ms = 700, future_knob = \"keep\" }\n"
    );
    let mut store = SettingsStore::from_content(text);
    store
        .set(
            "editor.progressive_open_threshold_bytes",
            SettingValue::Uint(2048),
        )
        .expect("inline 节上的 set 必须成功");

    let out = store.to_toml().expect("序列化");
    assert!(out.contains("[editor]"), "inline 节升格为标准表");
    assert!(out.contains("undo_coalesce_ms = 700"), "既有值保留");
    assert!(
        out.contains("future_knob = \"keep\""),
        "未知键随升格保留（前向兼容）"
    );
    assert!(out.contains("progressive_open_threshold_bytes = 2048"));

    // 升格后的往返：全部值读回。
    let (s2, st2) = parse_settings(&out);
    assert_eq!(st2, LoadStatus::Clean);
    assert_eq!(s2.editor.undo_coalesce_ms, 700);
    assert_eq!(s2.editor.progressive_open_threshold_bytes, 2048);
}

/// R24-a1 读侧容错锁：手写 inline 节（未经 set 改写）也必须能读——
/// `item_of` 用 `as_table_like`（TableLike 覆盖 Table + InlineTable）。
#[test]
fn load_accepts_handwritten_inline_section() {
    let text =
        "schema_version = 1\nstorage = { autosave_enabled = true, autosave_interval_ms = 1500 }\n";
    let (s, status) = parse_settings(text);
    assert_eq!(
        status,
        LoadStatus::Clean,
        "inline 节是合法 TOML，读侧不得丢"
    );
    assert!(s.storage.autosave_enabled, "inline 节键值必须读回");
    assert_eq!(s.storage.autosave_interval_ms, 1500);
    // 未涉及的层照常默认
    assert_eq!(s.appearance, Settings::default().appearance);
}

// ---------------------------------------------------------------------------
// W-1/W-2（issue #29 阶段 A0）：docx_export 层 toc/anonymize 两布尔键
//（CONTRACT §4 v1.5 增补——设置键注册模式同 tr16 四键）。
// ---------------------------------------------------------------------------

/// W-1/W-2 主链：两键 set（typed 校验）→ 快照 → 落盘重载读回 +
/// 类型 typed 拒 + 零触碰 + 文件面类型降级（bool 无值域面）。
#[test]
fn w2a_docx_toc_anonymize_keys_set_reload_and_domain() {
    let mut store = SettingsStore::new_default();
    // 默认态：两键 false（缺省 = 既有导出行为——纯加性契约）。
    assert!(!store.settings().docx_export.toc);
    assert!(!store.settings().docx_export.anonymize);

    // set 双键 → 快照生效 → 落盘重载存活。
    store
        .set("docx_export.toc", SettingValue::Bool(true))
        .expect("toc set");
    store
        .set("docx_export.anonymize", SettingValue::Bool(true))
        .expect("anonymize set");
    assert!(store.settings().docx_export.toc);
    assert!(store.settings().docx_export.anonymize);
    let dir = tmp_dir("w2a");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.toml");
    store.save(&path).expect("save");
    let reloaded = SettingsStore::load(&path);
    let s = reloaded.settings();
    assert!(s.docx_export.toc, "toc 重载存活");
    assert!(s.docx_export.anonymize, "anonymize 重载存活");
    // 既有四键不受影响（默认态）。
    assert_eq!(s.docx_export.preset, "default");
    assert_eq!(s.docx_export.size_half_points, 28);
    std::fs::remove_dir_all(&dir).ok();

    // 类型 typed 拒（Str/Uint → WrongType；bool 无值域面故无 OutOfRange 路）。
    let mut store2 = SettingsStore::new_default();
    for (path, bad) in [
        ("docx_export.toc", SettingValue::Str("yes".into())),
        ("docx_export.toc", SettingValue::Uint(1)),
        ("docx_export.anonymize", SettingValue::Str("true".into())),
        ("docx_export.anonymize", SettingValue::Uint(0)),
    ] {
        assert!(
            matches!(
                store2.set(path, bad.clone()),
                Err(SettingError::WrongType { .. })
            ),
            "{path}={bad:?} 必须类型拒"
        );
    }
    assert_eq!(
        store2.to_toml().unwrap(),
        default_document().to_string(),
        "全部拒绝=零触碰"
    );

    // 文件面类型降级：非 bool 值落盘后重载 → 该键回默认 false + KeyDegraded。
    let dir2 = tmp_dir("w2ab");
    std::fs::create_dir_all(&dir2).unwrap();
    let path2 = dir2.join("settings.toml");
    std::fs::write(
        &path2,
        "schema_version = 1\n\n[docx_export]\ntoc = \"yes\"\nanonymize = 1\n",
    )
    .unwrap();
    let degraded = SettingsStore::load(&path2);
    let s2 = degraded.settings();
    assert!(!s2.docx_export.toc, "非 bool 回默认 false");
    assert!(!s2.docx_export.anonymize, "非 bool 回默认 false");
    assert!(matches!(
        degraded.status(),
        mdshell::settings::LoadStatus::KeyDegraded { .. }
    ));
    std::fs::remove_dir_all(&dir2).ok();
}

/// #43（CONTRACT §4 v1.6）：`appearance.language` 键——set 全值域 +
/// 非 str 类型拒 + 越域拒 + 落盘重载存活 + load 旧 TOML 缺键回默认。
#[test]
fn i18n_language_key_set_reload_and_domain() {
    let mut store = SettingsStore::new_default();
    // 默认态：auto（跟随 navigator/宿主语言——纯加性契约）。
    assert_eq!(store.settings().appearance.language, "auto");

    // set 全值域（auto 已默认；en / zh-CN 显式覆盖）。
    store
        .set("appearance.language", SettingValue::Str("zh-CN".into()))
        .expect("zh-CN set");
    assert_eq!(store.settings().appearance.language, "zh-CN");
    store
        .set("appearance.language", SettingValue::Str("en".into()))
        .expect("en set");
    assert_eq!(store.settings().appearance.language, "en");

    // 非 str 类型拒（WrongType）+ 越域值拒（OutOfRange）——快照零触碰。
    assert!(store
        .set("appearance.language", SettingValue::Uint(1))
        .is_err());
    assert!(store
        .set("appearance.language", SettingValue::Str("zh_TW".into()))
        .is_err());
    assert!(store
        .set("appearance.language", SettingValue::Str("english".into()))
        .is_err());
    assert_eq!(store.settings().appearance.language, "en", "拒后快照不变");

    // 落盘重载存活 + TOML 面字段在档。
    store
        .set("appearance.language", SettingValue::Str("zh-CN".into()))
        .expect("zh-CN re-set");
    let dir = tmp_dir("i18n_lang");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.toml");
    store.save(&path).expect("save");
    let toml_text = std::fs::read_to_string(&path).unwrap();
    assert!(
        toml_text.contains("language = \"zh-CN\""),
        "TOML 面必须在档"
    );
    let reloaded = SettingsStore::load(&path);
    assert_eq!(reloaded.settings().appearance.language, "zh-CN", "重载存活");

    // 旧 TOML（无 language 键）→ serde/parse 缺省 auto（前向兼容）。
    let old = dir.join("old.toml");
    std::fs::write(
        &old,
        "schema_version = 1\n\n[appearance]\ntheme = \"dark\"\n",
    )
    .unwrap();
    let reloaded_old = SettingsStore::load(&old);
    assert_eq!(reloaded_old.settings().appearance.language, "auto");
    assert_eq!(reloaded_old.settings().appearance.theme, "dark");

    // 越域值 load → KeyDegraded 回默认 auto（load 与 set 同表）。
    let bad = dir.join("bad.toml");
    std::fs::write(
        &bad,
        "schema_version = 1\n\n[appearance]\nlanguage = \"fr\"\n",
    )
    .unwrap();
    let reloaded_bad = SettingsStore::load(&bad);
    assert_eq!(
        reloaded_bad.settings().appearance.language,
        "auto",
        "越域回默认"
    );
    assert!(matches!(
        reloaded_bad.status(),
        LoadStatus::KeyDegraded { ref path, .. } if path == "appearance.language"
    ));
}
