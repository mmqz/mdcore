//! mdc-settings — 域无关设置面基础设施。
//!
//! T3 复用地基轮（2026-10-12）自 mdshell（crates/shell）整体抽取——
//! 对 mdcore 零依赖（依赖闭包由 tests/closure_lock.rs 固化），
//! mdshell 经 `pub use mdc_settings as settings` re-export 保持
//! `mdshell::settings::*` 路径零破坏。消费面：三域（MD/PDF/Office）
//! 均可直接依赖本 crate，不再连带 mdcore 块核。
//!
//! R24-a 设置面核侧（主档 §31 R24-a）。
//!
//! 机制合同（四条硬语义，r24_settings 语义锁背书）：
//! - **默认值合并**：sidecar 缺键 → 层内默认值兜底（`Settings::default`）。
//! - **未知键前向兼容保留**：读写往返不丢未知键/注释/排版（toml_edit
//!   格式保持编辑；新版本写入的键对本版本是不可变透传字节）。
//! - **schema 版本迁移**：`schema_version` 顶层键；v0（无版本键，首版
//!   文件形态）→ v1 补版本键；后续版本按序追加迁移步，禁止跳版。
//! - **损坏文件降级加载**：TOML 解析失败/读盘失败 → 全默认 + 状态报告
//!   （`FileDegraded`），`save` 可重写修复；已知键类型不符或值越界 →
//!   该键回默认（`KeyDegraded`），其余键不受影响；**未来版本拒写保护**
//!   （`FutureSchema`：本构建不明就里不得降写新 schema 文件）。
//!
//! 持久化形态：TOML sidecar（与 md+meta 既有 sidecar 纪律一致——伴随文件、
//! 原子写 tmp+rename、绝不写半截文件）。宿主接线（快照注入 web 面、落盘
//! 时机）随 R24-d 壳实体轮落地；本模块 = headless 可验证的核侧机制。
//!
//! 契约：window 面读写项（`__settings` / `__setSetting` /
//! `mdc-settings-changed`）见 CONTRACT.md §4（v1.1）。默认值表与值域
//! 约束双侧一致（CONTRACT §4 默认值表为共同锚），Rust 侧为本表权威。
//!
//! R24-a1 修复（2026-10-10，质量审计轮）：`set()` 的写侧规范化
//! （`ensure_section`：节缺失/inline 形态 → 标准表，值保留）+ 读侧容错
//! （`item_of` 用 `as_table_like` 认 inline 节）——修复「节缺失时 set
//! 产出 inline 表、load 读不回 → 设置静默丢失」的写读形态不对称。

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

/// 当前设置 schema 版本。
pub const SCHEMA_VERSION: u64 = 1;

/// 值域表（`set` 校验与 load 降级共用；CONTRACT §4 双侧一致）。
pub mod bounds {
    /// undo 合并窗毫秒。
    pub const UNDO_COALESCE_MS: (u64, u64) = (1, 10_000);
    /// 渐进打开阈值字节。
    pub const PROGRESSIVE_THRESHOLD_BYTES: (u64, u64) = (0, 1 << 30);
    /// 自动保存间隔毫秒。
    pub const AUTOSAVE_INTERVAL_MS: (u64, u64) = (1_000, 3_600_000);
    /// 编辑区字号 px。
    pub const FONT_SIZE_PX: (u64, u64) = (8, 48);
    /// 外观主题枚举。
    pub const THEMES: [&str; 3] = ["light", "dark", "system"];

    /// #43（CONTRACT §4 v1.6）：界面语言值域（auto = 跟随系统/浏览器）。
    pub const LANGUAGES: [&str; 3] = ["auto", "en", "zh-CN"];
    /// #16（A4）：docx 导出格式预设枚举（与 web DOCX_EXPORT_PRESETS
    /// 表同源——web 面键值锁背书：contract_gate + docx_export e2e）。
    pub const DOCX_EXPORT_PRESETS: [&str; 5] =
        ["default", "official-a4", "compact", "large-print", "custom"];
    /// #16：自定义字号（半磅）——UI 三选值域（五号/小四/四号/三号）。
    pub const DOCX_SIZE_HALF_POINTS: [u64; 4] = [21, 24, 28, 32];
    /// #16：自定义行距（line 值——单倍/1.5 倍/2 倍）。
    pub const DOCX_LINE_SPACING: [u64; 3] = [240, 360, 480];
    /// #16：自定义四边边距（twips——2cm/1 英寸/公文 90pt）。
    pub const DOCX_MARGIN_TWIPS: [u64; 3] = [1134, 1440, 1800];
}

// ---------------------------------------------------------------------------
// 类型与默认值
// ---------------------------------------------------------------------------

/// 设置快照（已应用默认合并的解析值；serde JSON 形状 = web `__settings()` 面）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub schema_version: u64,
    pub editor: EditorSettings,
    pub ai: AiSettings,
    pub storage: StorageSettings,
    pub appearance: AppearanceSettings,
    /// #16（A4）：docx 导出格式层（v1.2 增补——纯增量，v1.1 面零破坏）。
    pub docx_export: DocxExportSettings,
}

/// undo 合并窗默认值（毫秒）。
///
/// 与 mdcore::history::COALESCE_MS 数值同源——但本 crate 对 mdcore
/// **零依赖**（域无关抽取纪律），不直接引用该常量；数值一致性由
/// mdshell 侧 r24_settings::defaults_align_with_core_constants 跨
/// crate 锁背书（双侧任一漂移即测试红，单源纪律不断）。
const UNDO_COALESCE_MS_DEFAULT: u64 = 1000;

/// 编辑行为层。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorSettings {
    /// undo 合并窗（毫秒）——默认值与 mdcore::history::COALESCE_MS
    /// 对齐（跨 crate 常量锁背书，见 UNDO_COALESCE_MS_DEFAULT 注）。
    pub undo_coalesce_ms: u64,
    /// 超过该字节数的文档走渐进打开（R10-a 路径选择；1MB = W4 基准参照）。
    pub progressive_open_threshold_bytes: u64,
}

/// AI 通道层。v0 仅宿主侧 feature flag（R12 通道开关）；键集随 AI 面
/// 产品化扩充——新键即新 schema 版本，走迁移框架。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AiSettings {
    pub commands_enabled: bool,
}

/// 存储层。`session_restore` 由 R24-b 会话持久化消费。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StorageSettings {
    /// md+meta 双文件纪律（document_id/历史复原）。
    pub meta_sidecar: bool,
    pub autosave_enabled: bool,
    pub autosave_interval_ms: u64,
    /// 启动时恢复上次打开集（R24-b SessionManager 消费）。
    pub session_restore: bool,
}

/// 外观层。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AppearanceSettings {
    /// `light` | `dark` | `system`。
    pub theme: String,
    pub font_size_px: u16,
    /// #43（CONTRACT §4 v1.6）：界面语言——`auto`（跟随 navigator.language
    /// / 宿主系统语言）| `en` | `zh-CN`。web 消费面 = i18n `initI18n`
    /// 单一真源；旧快照缺键 = serde 默认 `auto`（前向兼容）。
    #[serde(default = "default_language")]
    pub language: String,
}

/// #43：language 键 serde 缺省（旧 JSON/TOML 快照无键 → auto）。
fn default_language() -> String {
    "auto".into()
}

/// #16（A4）：docx 导出格式层（v1.2 增补；W-1/W-2 两布尔键 v1.5 增补）。
///
/// 消费链：⚙ 面板/`__setSetting` 写 → 快照 + localStorage 镜像 →
/// 导出按钮 `export_docx_styled(options JSON)`（wasm 面——issue #10
/// 已供）。预设 = 排版+几何全量（web 预设表）；自定义 = 三选值
///（字号/行距/边距）+ 固定字体槽。默认 `default` = 空 options ≡
/// 旧 `export_docx` 字节级等价（零破坏——Rust 侧 E2 钉定）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocxExportSettings {
    /// `default` | `official-a4` | `compact` | `large-print` | `custom`。
    pub preset: String,
    /// 自定义字号（半磅；预设模式不消费）。
    pub size_half_points: u16,
    /// 自定义行距（line 值：240 单倍 / 360 1.5 倍 / 480 2 倍）。
    pub line_spacing: u16,
    /// 自定义四边边距（twips）。
    pub margin_twips: u16,
    /// W-1（issue #29）：目录域开关——true = 导出 options 携带
    /// `toc:{enabled:true, level_from:1, level_to:3}`（层级用默认
    /// 1-3，不 UI 化——web 面板仅开关）。默认 false（零发射，字节稳定）。
    pub toc: bool,
    /// W-2（issue #29 / G3 / R-11）：暗标匿名导出开关——true = 导出
    /// options 携带 `anonymize:true`（docProps 元数据清零后处理）。
    /// 默认 false。
    pub anonymize: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            editor: EditorSettings {
                undo_coalesce_ms: UNDO_COALESCE_MS_DEFAULT,
                progressive_open_threshold_bytes: 1_048_576,
            },
            ai: AiSettings {
                commands_enabled: true,
            },
            storage: StorageSettings {
                meta_sidecar: true,
                autosave_enabled: false,
                autosave_interval_ms: 30_000,
                session_restore: true,
            },
            appearance: AppearanceSettings {
                theme: "system".into(),
                font_size_px: 15,
                language: "auto".into(),
            },
            docx_export: DocxExportSettings {
                preset: "default".into(),
                size_half_points: 28,
                line_spacing: 360,
                margin_twips: 1800,
                toc: false,
                anonymize: false,
            },
        }
    }
}

/// 设置值（契约面窄类型：web 桥 JSON 标量的直映射）。
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Uint(u64),
    Bool(bool),
    Str(String),
}

/// `set` 拒绝原因。
#[derive(Debug, Clone, PartialEq)]
pub enum SettingError {
    /// 未知路径（保留给未来 schema 版本；本构建只写已知键）。
    UnknownPath(String),
    /// 类型不符。
    WrongType {
        path: String,
        expected: &'static str,
    },
    /// 越界/非法枚举。
    OutOfRange { path: String, detail: String },
}

/// `save` 拒绝原因。
#[derive(Debug, Clone, PartialEq)]
pub enum SaveError {
    /// 未来 schema 文件：本构建不得降写（防毁新版本数据）。
    FutureSchema {
        found: u64,
        current: u64,
    },
    Io(String),
}

/// 加载状态（降级不 panic，状态可报告可测试）。
#[derive(Debug, Clone, PartialEq)]
pub enum LoadStatus {
    /// 正常（默认合并不报告——缺键即默认是设计行为，非异常）。
    Clean,
    /// 文件不存在 → 全默认（首次运行；save 即落盘）。
    Created,
    /// 版本迁移发生（from=0 表示无版本键的 v0 形态）。
    Migrated { from: u64 },
    /// 已知键类型不符/值越界 → 该键回默认（首个报告；其余键不受影响）。
    KeyDegraded { path: String, reason: String },
    /// 解析失败/读盘失败 → 全默认（save 可修复重写）。
    FileDegraded { reason: String },
    /// 文件版本高于本构建 → 全默认 + save 拒写。
    FutureSchema { found: u64 },
}

// ---------------------------------------------------------------------------
// 读取（默认合并 + 类型/值域降级）
// ---------------------------------------------------------------------------

fn as_u64(item: &Item) -> Option<u64> {
    let v = item.as_value()?;
    v.as_integer().and_then(|i| u64::try_from(i).ok())
}

fn as_bool(item: &Item) -> Option<bool> {
    item.as_value().and_then(|v| v.as_bool())
}

fn as_str(item: &Item) -> Option<String> {
    item.as_value().and_then(|v| v.as_str()).map(str::to_string)
}

fn item_of<'a>(doc: &'a DocumentMut, section: &str, key: &str) -> Option<&'a Item> {
    // as_table_like（非 as_table）：读侧容错——手写/旧版本 inline 节
    // （`editor = { ... }`）同样可读（TableLike 覆盖 Table + InlineTable）。
    doc.get(section)?.as_table_like()?.get(key)
}

/// 保证 `section` 是**标准表**（`[section]` 表头形态）并返回可变引用：
/// - 已是标准表 → 原样返回；
/// - inline 节（`section = { ... }`，手写或旧版本产物）→ **值保留升格**
///   为标准表（未知键随迁，前向兼容不丢字节）；
/// - 缺失/其它异形（标量/数组）→ 新建空标准表。
///
/// R24-a1 修复：直接 `doc[section][key] = ...` 在节缺失时，toml_edit 的
/// IndexMut 把 `Item::None` 自动升格为 **InlineTable**（index.rs 的
/// `if let Item::None = *v` 分支），产出 `section = { key = ... }`；
/// 而读侧 `as_table()` 只认标准表 → 写入值在下次 load 静默丢失
/// （探针 probe_settings_edge 抓出：set 后 save → load 回默认）。
/// 本函数保证**写入形态 = 读取形态**（写侧规范化）。
fn ensure_section<'a>(doc: &'a mut DocumentMut, section: &str) -> &'a mut toml_edit::Table {
    if doc.get(section).is_some_and(Item::is_table) {
        return doc
            .get_mut(section)
            .and_then(Item::as_table_mut)
            .expect("is_table checked above");
    }
    let mut promoted = toml_edit::Table::new();
    if let Some(like) = doc.get(section).and_then(Item::as_table_like) {
        for (k, v) in like.iter() {
            promoted.insert(k, v.clone());
        }
    }
    doc.insert(section, Item::Table(promoted));
    doc.get_mut(section)
        .and_then(Item::as_table_mut)
        .expect("just inserted a Table")
}

fn read_file_version(doc: &DocumentMut) -> u64 {
    doc.get("schema_version").and_then(as_u64).unwrap_or(0)
}

/// 解析文本为（设置, 状态）（机制核心：默认合并 / 未知键保留 / 迁移 / 降级）。
///
/// 降级语义分层：类型不符或值越界 = **该键**回默认（其余键照常解析）；
/// TOML 解析失败 = **整文件**回默认（save 重写修复）；版本高于本构建 =
/// 整文件回默认且 save 拒写。未知键不降级——原样保留（前向兼容）。
pub fn parse_settings(text: &str) -> (Settings, LoadStatus) {
    let (_, settings, status) = parse_document(text);
    (settings, status)
}

/// 解析全量：返回（已迁移文档, 设置, 状态）。`SettingsStore` 的单次
/// parse 入口——迁移落在返回的 doc 上（from_content 二次 parse 原文会把
/// 迁移丢掉，r24_settings v0 往返锁背书此约束）。
fn parse_document(text: &str) -> (DocumentMut, Settings, LoadStatus) {
    let mut doc: DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            return (
                default_document(),
                Settings::default(),
                LoadStatus::FileDegraded {
                    reason: e.to_string(),
                },
            );
        }
    };

    let found = read_file_version(&doc);
    if found > SCHEMA_VERSION {
        // 未来版本：本构建不解释任何键（防半懂状态），全默认 + 拒写。
        // doc 保留原文（诊断展示；save 已拒，不会被写回）。
        return (doc, Settings::default(), LoadStatus::FutureSchema { found });
    }

    // 迁移（按序；当前仅 v0→v1）。v0 = 无版本键（首版 rollout 文件形态）。
    let status = if found < SCHEMA_VERSION {
        if found != 0 {
            return (
                default_document(),
                Settings::default(),
                LoadStatus::FileDegraded {
                    reason: format!("no migration path from schema version {found}"),
                },
            );
        }
        migrate_v0_v1(&mut doc);
        LoadStatus::Migrated { from: found }
    } else {
        LoadStatus::Clean
    };

    let mut s = Settings::default();
    let mut degraded: Option<LoadStatus> = None;

    // ---- 类型读取（缺键 = 保持默认）----
    if let Some(item) = item_of(&doc, "editor", "undo_coalesce_ms") {
        match as_u64(item) {
            Some(v) => s.editor.undo_coalesce_ms = v,
            None => degrade(
                &mut degraded,
                "editor.undo_coalesce_ms",
                "expected unsigned integer",
            ),
        }
    }
    if let Some(item) = item_of(&doc, "editor", "progressive_open_threshold_bytes") {
        match as_u64(item) {
            Some(v) => s.editor.progressive_open_threshold_bytes = v,
            None => degrade(
                &mut degraded,
                "editor.progressive_open_threshold_bytes",
                "expected unsigned integer",
            ),
        }
    }
    if let Some(item) = item_of(&doc, "ai", "commands_enabled") {
        match as_bool(item) {
            Some(v) => s.ai.commands_enabled = v,
            None => degrade(&mut degraded, "ai.commands_enabled", "expected boolean"),
        }
    }
    if let Some(item) = item_of(&doc, "storage", "meta_sidecar") {
        match as_bool(item) {
            Some(v) => s.storage.meta_sidecar = v,
            None => degrade(&mut degraded, "storage.meta_sidecar", "expected boolean"),
        }
    }
    if let Some(item) = item_of(&doc, "storage", "autosave_enabled") {
        match as_bool(item) {
            Some(v) => s.storage.autosave_enabled = v,
            None => degrade(
                &mut degraded,
                "storage.autosave_enabled",
                "expected boolean",
            ),
        }
    }
    if let Some(item) = item_of(&doc, "storage", "autosave_interval_ms") {
        match as_u64(item) {
            Some(v) => s.storage.autosave_interval_ms = v,
            None => degrade(
                &mut degraded,
                "storage.autosave_interval_ms",
                "expected unsigned integer",
            ),
        }
    }
    if let Some(item) = item_of(&doc, "storage", "session_restore") {
        match as_bool(item) {
            Some(v) => s.storage.session_restore = v,
            None => degrade(&mut degraded, "storage.session_restore", "expected boolean"),
        }
    }
    if let Some(item) = item_of(&doc, "appearance", "theme") {
        match as_str(item) {
            Some(v) => s.appearance.theme = v,
            None => degrade(&mut degraded, "appearance.theme", "expected string"),
        }
    }
    // #43（CONTRACT §4 v1.6）：界面语言键（缺省 auto——旧 TOML 无键
    // 不降级，与 theme 同语义）。
    if let Some(item) = item_of(&doc, "appearance", "language") {
        match as_str(item) {
            Some(v) => s.appearance.language = v,
            None => degrade(&mut degraded, "appearance.language", "expected string"),
        }
    }
    if let Some(item) = item_of(&doc, "appearance", "font_size_px") {
        match as_u64(item).and_then(|v| u16::try_from(v).ok()) {
            Some(v) => s.appearance.font_size_px = v,
            None => degrade(
                &mut degraded,
                "appearance.font_size_px",
                "expected integer in u16 range",
            ),
        }
    }
    // #16（A4）：docx 导出格式层四键。
    if let Some(item) = item_of(&doc, "docx_export", "preset") {
        match as_str(item) {
            Some(v) => s.docx_export.preset = v,
            None => degrade(&mut degraded, "docx_export.preset", "expected string"),
        }
    }
    if let Some(item) = item_of(&doc, "docx_export", "size_half_points") {
        match as_u64(item).and_then(|v| u16::try_from(v).ok()) {
            Some(v) => s.docx_export.size_half_points = v,
            None => degrade(
                &mut degraded,
                "docx_export.size_half_points",
                "expected integer in u16 range",
            ),
        }
    }
    if let Some(item) = item_of(&doc, "docx_export", "line_spacing") {
        match as_u64(item).and_then(|v| u16::try_from(v).ok()) {
            Some(v) => s.docx_export.line_spacing = v,
            None => degrade(
                &mut degraded,
                "docx_export.line_spacing",
                "expected integer in u16 range",
            ),
        }
    }
    if let Some(item) = item_of(&doc, "docx_export", "margin_twips") {
        match as_u64(item).and_then(|v| u16::try_from(v).ok()) {
            Some(v) => s.docx_export.margin_twips = v,
            None => degrade(
                &mut degraded,
                "docx_export.margin_twips",
                "expected integer in u16 range",
            ),
        }
    }
    // W-1/W-2（issue #29）：目录域/匿名化两布尔键（bool 无值域面——
    // 仅类型降级）。
    if let Some(item) = item_of(&doc, "docx_export", "toc") {
        match as_bool(item) {
            Some(v) => s.docx_export.toc = v,
            None => degrade(&mut degraded, "docx_export.toc", "expected boolean"),
        }
    }
    if let Some(item) = item_of(&doc, "docx_export", "anonymize") {
        match as_bool(item) {
            Some(v) => s.docx_export.anonymize = v,
            None => degrade(&mut degraded, "docx_export.anonymize", "expected boolean"),
        }
    }

    // ---- 值域校验（与 set() 同表：越界键回默认，理由与类型降级一致）----
    if !in_range(s.editor.undo_coalesce_ms, bounds::UNDO_COALESCE_MS) {
        s.editor.undo_coalesce_ms = Settings::default().editor.undo_coalesce_ms;
        degrade(
            &mut degraded,
            "editor.undo_coalesce_ms",
            "value out of range",
        );
    }
    if !in_range(
        s.editor.progressive_open_threshold_bytes,
        bounds::PROGRESSIVE_THRESHOLD_BYTES,
    ) {
        s.editor.progressive_open_threshold_bytes =
            Settings::default().editor.progressive_open_threshold_bytes;
        degrade(
            &mut degraded,
            "editor.progressive_open_threshold_bytes",
            "value out of range",
        );
    }
    if !in_range(s.storage.autosave_interval_ms, bounds::AUTOSAVE_INTERVAL_MS) {
        s.storage.autosave_interval_ms = Settings::default().storage.autosave_interval_ms;
        degrade(
            &mut degraded,
            "storage.autosave_interval_ms",
            "value out of range",
        );
    }
    if !bounds::THEMES.contains(&s.appearance.theme.as_str()) {
        s.appearance.theme = Settings::default().appearance.theme;
        degrade(
            &mut degraded,
            "appearance.theme",
            "not in light|dark|system",
        );
    }
    // #43：语言值域（load 与 set 同表）。
    if !bounds::LANGUAGES.contains(&s.appearance.language.as_str()) {
        s.appearance.language = Settings::default().appearance.language;
        degrade(&mut degraded, "appearance.language", "not in auto|en|zh-CN");
    }
    if !in_range(s.appearance.font_size_px as u64, bounds::FONT_SIZE_PX) {
        s.appearance.font_size_px = Settings::default().appearance.font_size_px;
        degrade(
            &mut degraded,
            "appearance.font_size_px",
            "value out of range",
        );
    }
    // #16（A4）：docx 导出格式层值域（与 set() 同表）。
    if !bounds::DOCX_EXPORT_PRESETS.contains(&s.docx_export.preset.as_str()) {
        s.docx_export.preset = Settings::default().docx_export.preset;
        degrade(&mut degraded, "docx_export.preset", "value out of range");
    }
    if !bounds::DOCX_SIZE_HALF_POINTS.contains(&(s.docx_export.size_half_points as u64)) {
        s.docx_export.size_half_points = Settings::default().docx_export.size_half_points;
        degrade(
            &mut degraded,
            "docx_export.size_half_points",
            "value out of range",
        );
    }
    if !bounds::DOCX_LINE_SPACING.contains(&(s.docx_export.line_spacing as u64)) {
        s.docx_export.line_spacing = Settings::default().docx_export.line_spacing;
        degrade(
            &mut degraded,
            "docx_export.line_spacing",
            "value out of range",
        );
    }
    if !bounds::DOCX_MARGIN_TWIPS.contains(&(s.docx_export.margin_twips as u64)) {
        s.docx_export.margin_twips = Settings::default().docx_export.margin_twips;
        degrade(
            &mut degraded,
            "docx_export.margin_twips",
            "value out of range",
        );
    }

    match degraded {
        Some(d) => (doc, s, d),
        None => (doc, s, status),
    }
}

fn key_degraded(path: &str, reason: &str) -> LoadStatus {
    LoadStatus::KeyDegraded {
        path: path.to_string(),
        reason: reason.to_string(),
    }
}

/// 记录首个键级降级（后续键降级不覆盖首个报告）。
fn degrade(degraded: &mut Option<LoadStatus>, path: &str, reason: &str) {
    if degraded.is_none() {
        *degraded = Some(key_degraded(path, reason));
    }
}

fn in_range(v: u64, (lo, hi): (u64, u64)) -> bool {
    v >= lo && v <= hi
}

/// v0 → v1 迁移：补 `schema_version = 1`（v0 文件 = 首版 rollout，无版本键）。
fn migrate_v0_v1(doc: &mut DocumentMut) {
    doc["schema_version"] = toml_edit::value(SCHEMA_VERSION as i64);
}

// ---------------------------------------------------------------------------
// 默认文档（首次落盘 / 修复重写形态）
// ---------------------------------------------------------------------------

/// 默认 sidecar 文档。键序 = Settings 字段序；人可读、机器可往返。
pub fn default_document() -> DocumentMut {
    let d = Settings::default();
    let mut doc = DocumentMut::new();
    doc["schema_version"] = toml_edit::value(SCHEMA_VERSION as i64);

    let mut editor = toml_edit::Table::new();
    editor["undo_coalesce_ms"] = toml_edit::value(d.editor.undo_coalesce_ms as i64);
    editor["progressive_open_threshold_bytes"] =
        toml_edit::value(d.editor.progressive_open_threshold_bytes as i64);
    doc["editor"] = Item::Table(editor);

    let mut ai = toml_edit::Table::new();
    ai["commands_enabled"] = toml_edit::value(d.ai.commands_enabled);
    doc["ai"] = Item::Table(ai);

    let mut storage = toml_edit::Table::new();
    storage["meta_sidecar"] = toml_edit::value(d.storage.meta_sidecar);
    storage["autosave_enabled"] = toml_edit::value(d.storage.autosave_enabled);
    storage["autosave_interval_ms"] = toml_edit::value(d.storage.autosave_interval_ms as i64);
    storage["session_restore"] = toml_edit::value(d.storage.session_restore);
    doc["storage"] = Item::Table(storage);

    let mut appearance = toml_edit::Table::new();
    appearance["theme"] = toml_edit::value(d.appearance.theme.as_str());
    appearance["font_size_px"] = toml_edit::value(d.appearance.font_size_px as i64);
    doc["appearance"] = Item::Table(appearance);

    doc
}

// ---------------------------------------------------------------------------
// 存储（sidecar 文件 + 原子写）
// ---------------------------------------------------------------------------

/// 设置存储：TOML 文档（保留未知键/注释）+ 解析快照 + 加载状态。
#[derive(Debug, Clone)]
pub struct SettingsStore {
    doc: DocumentMut,
    settings: Settings,
    status: LoadStatus,
    /// 未来 schema 保护（save 拒绝重写）。
    future_schema: Option<u64>,
}

impl SettingsStore {
    /// 从 sidecar 文本构建（测试 / 宿主已有内容时）。
    pub fn from_content(text: &str) -> Self {
        match parse_document(text) {
            (doc, settings, LoadStatus::FutureSchema { found }) => Self {
                doc,
                settings,
                status: LoadStatus::FutureSchema { found },
                future_schema: Some(found),
            },
            (_, settings, status @ LoadStatus::FileDegraded { .. }) => Self {
                doc: default_document(),
                settings,
                status,
                future_schema: None,
            },
            (doc, settings, status) => Self {
                doc,
                settings,
                status,
                future_schema: None,
            },
        }
    }

    /// 从默认文档构建（首次运行 / 全新建存储）。
    pub fn new_default() -> Self {
        Self {
            doc: default_document(),
            settings: Settings::default(),
            status: LoadStatus::Created,
            future_schema: None,
        }
    }

    /// 读 sidecar 文件（不存在 = Created 首次运行；IO/解析失败 = 降级）。
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_content(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::new_default(),
            Err(e) => Self {
                doc: default_document(),
                settings: Settings::default(),
                status: LoadStatus::FileDegraded {
                    reason: e.to_string(),
                },
                future_schema: None,
            },
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn status(&self) -> &LoadStatus {
        &self.status
    }

    /// 序列化（保留未知键/注释/排版；未来 schema 拒绝）。
    pub fn to_toml(&self) -> Result<String, SaveError> {
        if let Some(found) = self.future_schema {
            return Err(SaveError::FutureSchema {
                found,
                current: SCHEMA_VERSION,
            });
        }
        Ok(self.doc.to_string())
    }

    /// 原子写盘（同目录 tmp + rename，绝不写半截文件）。
    pub fn save(&self, path: &Path) -> Result<(), SaveError> {
        let text = self.to_toml()?;
        let mut tmp = path.as_os_str().to_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, text).map_err(|e| SaveError::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| SaveError::Io(e.to_string()))?;
        Ok(())
    }

    /// 写已知键（点路径 `section.key`）。doc 与快照同步更新；
    /// 未知路径/类型不符/越界 → Err（文档零触碰）。
    pub fn set(&mut self, path: &str, value: SettingValue) -> Result<(), SettingError> {
        if self.future_schema.is_some() {
            return Err(SettingError::UnknownPath(format!(
                "{path} (future-schema file is read-only)"
            )));
        }
        let (section, key) = path
            .split_once('.')
            .ok_or_else(|| SettingError::UnknownPath(path.to_string()))?;

        // 逐键显式分派（已知键封闭集；类型/值域与 load 降级同一张表）。
        match (section, key) {
            ("editor", "undo_coalesce_ms") => {
                let v = as_uint(path, &value)?;
                ensure_range(path, v, bounds::UNDO_COALESCE_MS)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v as i64);
                self.settings.editor.undo_coalesce_ms = v;
            }
            ("editor", "progressive_open_threshold_bytes") => {
                let v = as_uint(path, &value)?;
                ensure_range(path, v, bounds::PROGRESSIVE_THRESHOLD_BYTES)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v as i64);
                self.settings.editor.progressive_open_threshold_bytes = v;
            }
            ("ai", "commands_enabled") => {
                let v = as_bool_val(path, &value)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v);
                self.settings.ai.commands_enabled = v;
            }
            ("storage", "meta_sidecar") => {
                let v = as_bool_val(path, &value)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v);
                self.settings.storage.meta_sidecar = v;
            }
            ("storage", "autosave_enabled") => {
                let v = as_bool_val(path, &value)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v);
                self.settings.storage.autosave_enabled = v;
            }
            ("storage", "autosave_interval_ms") => {
                let v = as_uint(path, &value)?;
                ensure_range(path, v, bounds::AUTOSAVE_INTERVAL_MS)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v as i64);
                self.settings.storage.autosave_interval_ms = v;
            }
            ("storage", "session_restore") => {
                let v = as_bool_val(path, &value)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v);
                self.settings.storage.session_restore = v;
            }
            ("appearance", "theme") => {
                let v = match &value {
                    SettingValue::Str(v) => v.clone(),
                    _ => {
                        return Err(SettingError::WrongType {
                            path: path.to_string(),
                            expected: "string",
                        })
                    }
                };
                if !bounds::THEMES.contains(&v.as_str()) {
                    return Err(SettingError::OutOfRange {
                        path: path.to_string(),
                        detail: "theme must be light | dark | system".into(),
                    });
                }
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v.as_str());
                self.settings.appearance.theme = v;
            }
            ("appearance", "language") => {
                let v = match &value {
                    SettingValue::Str(v) => v.clone(),
                    _ => {
                        return Err(SettingError::WrongType {
                            path: path.to_string(),
                            expected: "string",
                        })
                    }
                };
                if !bounds::LANGUAGES.contains(&v.as_str()) {
                    return Err(SettingError::OutOfRange {
                        path: path.to_string(),
                        detail: "language must be auto | en | zh-CN".into(),
                    });
                }
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v.as_str());
                self.settings.appearance.language = v;
            }
            ("appearance", "font_size_px") => {
                let v = as_uint(path, &value)?;
                ensure_range(path, v, bounds::FONT_SIZE_PX)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v as i64);
                self.settings.appearance.font_size_px = v as u16;
            }
            // #16（A4）：docx 导出格式层四键（枚举/离散值域——与 UI
            // 三选值域一致；未知预设/越域值 typed 拒）。
            ("docx_export", "preset") => {
                let v = match &value {
                    SettingValue::Str(v) => v.clone(),
                    _ => {
                        return Err(SettingError::WrongType {
                            path: path.to_string(),
                            expected: "string",
                        })
                    }
                };
                if !bounds::DOCX_EXPORT_PRESETS.contains(&v.as_str()) {
                    return Err(SettingError::OutOfRange {
                        path: path.to_string(),
                        detail:
                            "preset must be default | official-a4 | compact | large-print | custom"
                                .into(),
                    });
                }
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v.as_str());
                self.settings.docx_export.preset = v;
            }
            ("docx_export", "size_half_points") => {
                let v = as_uint(path, &value)?;
                if !bounds::DOCX_SIZE_HALF_POINTS.contains(&v) {
                    return Err(SettingError::OutOfRange {
                        path: path.to_string(),
                        detail: "size_half_points must be one of 21 | 24 | 28 | 32".into(),
                    });
                }
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v as i64);
                self.settings.docx_export.size_half_points = v as u16;
            }
            ("docx_export", "line_spacing") => {
                let v = as_uint(path, &value)?;
                if !bounds::DOCX_LINE_SPACING.contains(&v) {
                    return Err(SettingError::OutOfRange {
                        path: path.to_string(),
                        detail: "line_spacing must be one of 240 | 360 | 480".into(),
                    });
                }
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v as i64);
                self.settings.docx_export.line_spacing = v as u16;
            }
            ("docx_export", "margin_twips") => {
                let v = as_uint(path, &value)?;
                if !bounds::DOCX_MARGIN_TWIPS.contains(&v) {
                    return Err(SettingError::OutOfRange {
                        path: path.to_string(),
                        detail: "margin_twips must be one of 1134 | 1440 | 1800".into(),
                    });
                }
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v as i64);
                self.settings.docx_export.margin_twips = v as u16;
            }
            // W-1/W-2（issue #29）：目录域/匿名化两布尔键（bool 无值域
            // 面——仅类型 typed 拒）。
            ("docx_export", "toc") => {
                let v = as_bool_val(path, &value)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v);
                self.settings.docx_export.toc = v;
            }
            ("docx_export", "anonymize") => {
                let v = as_bool_val(path, &value)?;
                ensure_section(&mut self.doc, section)[key] = toml_edit::value(v);
                self.settings.docx_export.anonymize = v;
            }
            // schema_version 为保留键：不可经 set 面（迁移框架独占）。
            _ => return Err(SettingError::UnknownPath(path.to_string())),
        }

        // 写入成功后状态归一（用户显式写 = 不再是 Created/Migrated 语义；
        // 降级状态保留——降级键仍待 save 修复）。
        if matches!(
            self.status,
            LoadStatus::Created | LoadStatus::Migrated { .. }
        ) {
            self.status = LoadStatus::Clean;
        }
        Ok(())
    }
}

fn as_uint(path: &str, value: &SettingValue) -> Result<u64, SettingError> {
    match value {
        SettingValue::Uint(v) => Ok(*v),
        _ => Err(SettingError::WrongType {
            path: path.to_string(),
            expected: "uint",
        }),
    }
}

fn as_bool_val(path: &str, value: &SettingValue) -> Result<bool, SettingError> {
    match value {
        SettingValue::Bool(v) => Ok(*v),
        _ => Err(SettingError::WrongType {
            path: path.to_string(),
            expected: "bool",
        }),
    }
}

fn ensure_range(path: &str, v: u64, (lo, hi): (u64, u64)) -> Result<(), SettingError> {
    if !in_range(v, (lo, hi)) {
        return Err(SettingError::OutOfRange {
            path: path.to_string(),
            detail: format!("value must be in {lo}..={hi}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_document_roundtrips_to_defaults() {
        let text = default_document().to_string();
        let (s, status) = parse_settings(&text);
        assert_eq!(s, Settings::default());
        assert_eq!(status, LoadStatus::Clean);
        assert!(text.contains("schema_version = 1"), "版本键必须在默认文档");
    }
}
