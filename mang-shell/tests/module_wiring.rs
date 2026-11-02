//! #183 B-2：模块级接线完整性——「已实现但 UI 无入口」零容忍。
//!
//! ## 与 wiring_integrity.rs 的分工
//!
//! | 文件 | 钉什么 |
//! |---|---|
//! | `wiring_integrity.rs` | UI 交互接线（按钮/菜单/标签的点击是否落到处理函数） |
//! | 本文件 | **模块挂载**（src/ 里有分量的模块是否有消费面证据——被谁调用/渲染） |
//!
//! 两者互补，共同构成 #183 的「界面上不存在已实现但没接 UI 的模块」验收面。
//!
//! ## 依据
//!
//! #183 判据①：「mang-shell 里不存在『已实现但没接 UI』的模块——典型证据：
//! 代码有 500+ 行、有单元测试，但界面上没有入口」。
//! B-1 勘察（2026-11-02，A-2 同批扫描）结论：14 个模块活线（正例表），
//! 5 项断线（真红，各自指回 issue）。
//!
//! ## 红绿账（2026-11-02，随 A-3 接线进展滚动更新）
//!
//! | 测试 | 状态 | 依据 |
//! |---|---|---|
//! | `every_wired_module_has_ui_entry` | 🟢 20 条正例锚点全绿 | B-1 勘察实证（文件:行） |
//! | `git_panel_is_mounted_in_left_panel` | 🔴 | panels.rs `left_panel` 收 git_panel/git_view 形参但函数体从不使用——左栏恒渲染文件树，act-git 点击只有高亮变化 |
//! | `command_palette_uses_grouped_assembly` | 🔴 | editor/mod.rs 面板渲染扁平列表（`pal.items.take(8)`），palette_view.rs + command_groups.rs 分组装配零调用者 |
//! | `status_bar_consumes_status_info` | 🔴 | status_info.rs 四项纯函数（git 分支/编码/换行符/选中字数）零消费 |
//! | `settings_panel_ui_exists` | 🔴 | #182：设置面板必须实现（#165 移除设置钮是错误裁决）；settings.rs 8 类存取已全实现只缺 UI 壳 |
//! | `editor_area_context_menu_is_triggered` | 🔴 | #179：shell.rs `open_editor_context_menu` 零调用者，ed-* 八个动作处理器经 UI 不可达 |
//!
//! **绝不允许 xfail/#[ignore] 放过未接线模块**（#183：「不许把测试写成 xfail
//! 放过未接线的模块」）——A-3/#179/#182 接线落地后这些测试转绿。
//!
//! ## 锚点纪律
//!
//! 正例表的锚点是**消费面符号**（调用/渲染证据），不是模块自身的定义——
//! 「模块存在」编译器已保证，「被用起来」才是本文件要钉的。新模块加入
//! src/ 时请同步正例表；模块接线方式重构时请同步锚点（换实现不换能力）。
//!
//! ## 铁律
//!
//! - 只加测试，不改产线代码（git diff 只含 tests/）；
//! - 源码文件缺失 = FAIL（源码进仓，不是环境缺件）；
//! - 失败信息必须指明「哪个模块缺什么入口、属哪个 issue 范围」。

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// 源码读取与函数体提取（与 wiring_integrity.rs 同款工具）
// ---------------------------------------------------------------------------

/// 读 `mang-shell/src/<rel>` 源码。缺失即 FAIL（源码进仓，不是环境件）。
fn src(rel: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("源码文件缺失/不可读（{}）：{e}", path.display()))
}

/// 递归列出 `src/` 下全部 `.rs` 文件（相对路径）——设置面板存在性全仓扫描用。
fn all_src_files() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).unwrap_or_else(|e| panic!("src/ 不可读（{}）：{e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == OsStr::new("rs")) {
                let rel = path
                    .strip_prefix(&root)
                    .expect("src/ 前缀")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// 提取 `decl`（如 `"fn left_panel("`）所在函数的**函数体**（含首尾大括号）。
///
/// 大括号配对为字符串字面量感知（`"{"` 不计深度）。
fn fn_body(file: &str, decl: &str) -> String {
    let start = file.find(decl).unwrap_or_else(|| {
        panic!("静态锚点缺失：源码里找不到 `{decl}`——函数被改名/删除？接线检查失效，请同步测试")
    });
    let body_open = file
        .find('{', start)
        .unwrap_or_else(|| panic!("`{decl}` 后无函数体开括号"));
    let bytes = file.as_bytes();
    let mut i = body_open;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return file[body_open..=i].to_string();
            }
        }
        i += 1;
    }
    panic!("`{decl}` 函数体大括号不配对（字符串感知扫描到文件尾）");
}

// ---------------------------------------------------------------------------
// 正例表：已接线模块 → 消费面证据
// ---------------------------------------------------------------------------

/// 模块接线正例表：(模块, 消费文件, 消费符号集)。
///
/// 语义：**全部**符号须出现在消费文件里（AND）——每个符号都是 B-1 勘察
/// 实证过的调用/挂载点。新模块入 src/ 请同步本表（#183 判据①的常规守卫）。
const WIRED_MODULES: &[(&str, &str, &[&str])] = &[
    // —— 编辑器内核（阶段 A-D 的模块）——
    // 多光标：mod.rs 持 Cursors 并按 MultiMove 分派（Ctrl+D/Alt+拖列选择同链）。
    (
        "editor/multi_cursor.rs（多光标）",
        "editor/mod.rs",
        &["Cursors::", "MultiMove"],
    ),
    // 查找引擎：find_bar.refresh() 经 buffer_find 扫描（含正则路）。
    (
        "editor/buffer_find.rs（查找引擎）",
        "editor/find_bar.rs",
        &["find_matches"],
    ),
    // 软换行+折叠映射：mod.rs 的渲染/滚动/hit-test 全经 display_map 换算。
    (
        "editor/display_map.rs（软换行+折叠）",
        "editor/mod.rs",
        &["DisplayMap"],
    ),
    // 语法高亮：渲染主路径 line_tokens → highlight::（md 全量 + 围栏 3 语言）。
    (
        "editor/highlight.rs（语法高亮）",
        "editor/mod.rs",
        &["highlight::"],
    ),
    // docx 编辑桥：装载/编辑回写/保存落账/锁定判定四环节。
    (
        "editor/docx_edit_bridge.rs（docx 编辑桥）",
        "editor/mod.rs",
        &["DocxEditing"],
    ),
    // 鼠标纯函数集：mod.rs 9 个调用点（点击/拖拽/滚动的几何换算）。
    ("editor/mouse.rs（鼠标几何）", "editor/mod.rs", &["mouse::"]),
    // 查找条 UI：Ctrl+F/Ctrl+H/命令面板/图标条四入口同链渲染。
    (
        "editor/find_bar.rs（查找条 UI）",
        "editor/mod.rs",
        &["render_find_bar"],
    ),
    // 文档事件总线：after_edit 单点收割 → publish_doc_event。
    (
        "editor/doc_events.rs（文档事件）",
        "editor/mod.rs",
        &["publish_doc_event"],
    ),
    // —— 文档装载分派（document.rs 是五类文档的统一入口）——
    ("canvas_view.rs（画布视图）", "document.rs", &["CanvasView"]),
    (
        "editor/docx_doc.rs（docx 读面）",
        "document.rs",
        &["docx_doc::"],
    ),
    (
        "editor/pdf_doc.rs（pdf 读面）",
        "document.rs",
        &["pdf_doc::"],
    ),
    (
        "editor/xlsx_doc.rs（xlsx 读面）",
        "document.rs",
        &["xlsx_doc::"],
    ),
    // —— 外壳层 ——
    // 文档分派：Ctrl+O/文件树/最近列表/git 标签的共用打开通道。
    ("document.rs（文档分派）", "shell.rs", &["document::"]),
    // 最近打开：record_file/record_workspace + 欢迎页最近列表。
    ("recent.rs（最近打开）", "shell.rs", &["recent::"]),
    // 菜单数据面：file_tree/editor 两 origin 的动作派发单点。
    (
        "context_menu.rs（菜单数据）",
        "shell.rs",
        &["activate_context_menu_item"],
    ),
    // 标签栏：shell 渲染挂载（点击切换/Ctrl+Tab 环切）。
    ("tab_bar.rs（标签栏）", "shell.rs", &["tab_bar("]),
    // 主题三态：Ctrl+T → cycle_theme（含持久化）。
    ("theme_switch.rs（主题三态）", "shell.rs", &["cycle_theme"]),
    // 状态栏：shell 底部挂载。
    ("status_bar.rs（状态栏壳）", "shell.rs", &["status_bar::"]),
    // 标题栏：窗口控制钮 + 拖拽区。
    ("title_bar.rs（标题栏）", "shell.rs", &["title_bar::"]),
    // 左右面板壳：left_panel/right_panel 挂载（git 视图路由是 A-3 待接线项，
    // 见 `git_panel_is_mounted_in_left_panel`）。
    (
        "panels.rs（面板壳）",
        "shell.rs",
        &["panels::left_panel(", "panels::right_panel("],
    ),
];

/// #183 判据①：正例表里每个模块，其消费文件必须含全部消费符号。
///
/// 新增模块不入表 = 表外无守卫（人为流程兜底）；入表但符号消失 = 本测试
/// 红，失败信息指明模块与缺失符号——「模块存在但没人用」当场现形。
#[test]
fn every_wired_module_has_ui_entry() {
    let mut missing = Vec::new();
    for (module, consumer, symbols) in WIRED_MODULES {
        let file = src(consumer);
        for sym in *symbols {
            // sym: &&str —— 解引用后作 Pattern（避免 &&str 作 Pattern 的歧义）。
            if !file.contains(*sym) {
                missing.push(format!("  - {module}：{consumer} 不再引用 `{sym}`\n"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "以下模块的消费面证据缺失（模块存在但没人用——#183 判据①违规）：\n{}\
         模块被移除/重构时请同步 WIRED_MODULES 表；模块还在却没消费者 = 已实现未接线。",
        missing.join("")
    );
}

// ---------------------------------------------------------------------------
// 待接线模块（当前真红，各自指回 issue——绝不 xfail）
// ---------------------------------------------------------------------------

/// 🔴 A-3 范围：GitPanel 实体必须挂载渲染（数据链与按钮已通，断的是渲染面）。
///
/// 现状（B-1 勘察）：`panels.rs` 的 `left_panel` 签名接收 `git_panel` /
/// `git_view` 形参（F7 提交所为），**函数体却从不使用**——左栏恒渲染文件树，
/// 图标条 act-git 点击后只有按钮高亮变化，改动/历史面板本体不可见。
/// 该提交的烟雾验证走的是 main.rs 直调数据层（git_diff + open_readonly_tab），
/// 未经过面板 UI——「左栏双视图路由」名不副实。
#[test]
fn git_panel_is_mounted_in_left_panel() {
    let panels = src("panels.rs");
    let body = fn_body(&panels, "fn left_panel(");
    assert!(
        body.contains("git_panel"),
        "GitPanel 实体从未挂载渲染：panels.rs 的 left_panel 收到 git_panel/git_view \
         形参，函数体却从不使用——git_view 路由（文件树↔git 双视图）未实现。\
         git_panel.rs 560 行（改动/历史/diff 标签/刷新钮）已实现，图标条按钮已接，\
         只差左栏路由这一段。接线属 #183 A-3 范围。"
    );
}

/// 🔴 A-3 范围：命令面板渲染必须用分组装配（palette_view + command_groups）。
///
/// 现状：editor/mod.rs 的面板渲染是**扁平列表**（`pal.items.iter().take(8)`），
/// 而 palette_view.rs（108 行，palette_sections/palette_flat/palette_list）与
/// command_groups.rs 的分组装配（Group/RecentRing/build_sections，342 行，带
/// 完整单测）**零调用者**——450 行打磨版写了没接。现行可用的是无分组、
/// 无「最近使用」的 v1 版。
#[test]
fn command_palette_uses_grouped_assembly() {
    let mod_rs = src("editor/mod.rs");
    assert!(
        mod_rs.contains("build_sections") || mod_rs.contains("palette_view::"),
        "命令面板渲染未用分组装配：editor/mod.rs 现渲染扁平列表（pal.items），\
         command_groups::build_sections（分组+最近使用，带单测）与 palette_view \
         渲染层零调用者。接线属 #183 A-3 范围。"
    );
}

/// 🔴 A-3 范围：状态栏必须消费 status_info 的四项状态信息。
///
/// 现状：status_info.rs（388 行）的 `detect_eol` / `detect_encoding` /
/// `format_selection` / `git_branch_at` 四项纯函数（各自带单测）**零调用者**——
/// 状态栏现状只用 Editor 自带的 display_name/cursor_label/char_label。
#[test]
fn status_bar_consumes_status_info() {
    let sb = src("status_bar.rs");
    assert!(
        sb.contains("status_info::"),
        "status_bar 未消费 status_info：git 分支/文件编码/换行符/选中字数四项\
         纯函数写完没接——状态栏增强项全部悬空。接线属 #183 A-3 范围。"
    );
}

/// 🔴 #182 范围：设置面板 UI 必须存在（纠正 #165 的「移除」错误裁决）。
///
/// #183 定调：「该有但还没做」= 补实现，不是删入口。#165 把设置钮移除是
/// 错误裁决，#182 要求实现设置面板并恢复设置钮。settings.rs 的 8 类设置
/// 存取（workspace/indent/panel_widths/theme/font_size/folds/find_history/
/// find_regex）已全实现且被消费——只缺 UI 壳。
#[test]
fn settings_panel_ui_exists() {
    // ① 图标条设置钮必须恢复（#182 明确要求）。
    let ab = src("activity_bar.rs");
    assert!(
        ab.contains("act-settings"),
        "图标条无设置钮（act-settings 缺失）：#182 纠正 #165 的错误裁决——\
         设置面板必须实现，设置钮必须恢复。settings.rs 8 类存取已全实现。"
    );

    // ② 设置面板本体：src/ 全仓必须出现设置面板渲染符号（实现载体不限：
    // settings_view.rs / shell.rs / panels.rs 均可——锚点取常见命名）。
    let panel_syms = [
        "render_settings",
        "settings_view",
        "SettingsPanel",
        "settings_panel",
        "SettingsView",
    ];
    let hits: Vec<String> = all_src_files()
        .iter()
        .filter_map(|rel| {
            let file = src(rel);
            for sym in panel_syms {
                if file.contains(sym) {
                    return Some(format!("{rel}（`{sym}`）"));
                }
            }
            None
        })
        .collect();
    assert!(
        !hits.is_empty(),
        "src/ 全仓无设置面板 UI 符号（render_settings/settings_view/SettingsPanel/\
         settings_panel/SettingsView 零命中）：#182 要求实现设置面板——\
         settings.rs 的 8 类存取是地基，现在缺 UI 壳。"
    );
}

/// 🔴 #179 范围：编辑器区右键菜单触发器必须接线。
///
/// 现状：shell.rs 的 `open_editor_context_menu` 零调用者；editor/mod.rs 与
/// mouse.rs 无任何 `MouseButton::Right` 处理——编辑区右键无反应。ed-* 八个
/// 动作处理器（剪切/复制/粘贴/全选/查找/替换/跳行/折叠）在 shell.rs 已接，
/// 但经 UI 不可达（#166 只接了文件树触发器）。
#[test]
fn editor_area_context_menu_is_triggered() {
    let mod_rs = src("editor/mod.rs");
    assert!(
        mod_rs.contains("MouseButton::Right") || mod_rs.contains("open_editor_context_menu"),
        "编辑器区右键菜单触发器未接线：editor/mod.rs 无 MouseButton::Right 处理、\
         不调 open_editor_context_menu——ed-* 八个动作处理器已接但经 UI 不可达\
         （#179）。对照：文件树右键已接（open_file_tree_context_menu）。"
    );
}
