//! #168 / #196 收尾：界面层接线完整性——六面锚点（一面一测）。
//!
//! ## 与既有测试的分工
//!
//! | 文件 | 覆盖面 |
//! |---|---|
//! | `wiring_integrity.rs`（代理 B） | 模块内交互细节（无死闭包/菜单动作对账/基线计数） |
//! | `module_wiring.rs` | 模块挂载 + 派发安全 + perf 采集点 |
//! | `command_coverage.rs` | Cmd 枚举↔ALL↔执行臂静态对账（编辑器级键位） |
//! | **本文件** | **#196 任务 2 的六条接线面**——每面一条**全链路**锚点（入口 → 处理函数 → 状态/落盘），断言跨文件：任何一环被摘除即红 |
//!
//! ## 六面（#196 原文口径）
//!
//! 1. 菜单栏项 → 命令分发
//! 2. 快捷键 → Cmd 映射（本文件钉 **shell 级**键位——编辑器级键臂已由
//!    command_coverage 钉住，两级互补）
//! 3. 面板切换 → 状态变更
//! 4. 文件树点击 → 打开文件（实装为**单击**打开——双击语义从未实装，
//!    锚点按实装口径钉，防「声称双击实际没接」）
//! 5. 状态栏 → 数据源
//! 6. 设置项 → 生效
//!
//! ## 铁律
//!
//! - 只读源码断言，不改产线代码（git diff 只含 tests/）；
//! - 失败信息指明「哪面哪环断了」；
//! - 每条都验证过红（删锚点即红——见 PR 描述的红绿证据）。

use std::fs;
use std::path::{Path, PathBuf};

/// 读 `mang-shell/src/<rel>` 源码。缺失即 FAIL（源码进仓，不是环境件）。
fn src(rel: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("源码文件缺失/不可读（{}）：{e}", path.display()))
}

/// 提取函数体（`fn <decl>` 到配对大括号——与 module_wiring 同款口径）。
fn fn_body(file: &str, decl: &str) -> String {
    let start = file
        .find(decl)
        .unwrap_or_else(|| panic!("函数缺失：{decl}（接线被拆除或重命名——请同步锚点）"));
    let body_start = file[start..].find('{').expect("函数签名后无函数体") + start;
    let mut depth = 0usize;
    for (i, b) in file[body_start..].bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return file[body_start..body_start + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("函数体未闭合：{decl}");
}

/// 多文件全链断言（锚点缺失即失败，消息带面名与断环位置）。
fn chain(face: &str, links: &[(&str, &str, &str)]) {
    for (file_rel, needle, what) in links {
        let content = src(file_rel);
        assert!(
            content.contains(needle),
            "【面：{face}】断链——{file_rel} 缺「{what}」（锚点 `{needle}`）"
        );
    }
}

// ---------------------------------------------------------------------------
// 面 1：菜单栏项 → 命令分发
// ---------------------------------------------------------------------------

/// 菜单项从数据定义到执行的完整链路：
/// `context_menu.rs`（MenuItem::action 数据面）→ `context_menu_view.rs`
/// （点击回调）→ `shell.rs activate_context_menu_item`（分派体，按
/// MenuOrigin 区分 Editor/FileTree 两源）。
#[test]
fn face1_menu_items_reach_command_dispatch() {
    let menu = src("context_menu.rs");
    assert!(
        menu.contains("pub fn editor_menu(") && menu.contains("MenuItem::action("),
        "【面 1】菜单数据面缺失：context_menu.rs 无 editor_menu/MenuItem::action——\
         菜单项定义被拆除"
    );
    chain(
        "菜单栏项 → 命令分发",
        &[
            (
                "context_menu_view.rs",
                "activate_context_menu_item",
                "菜单点击回调 → shell 分派",
            ),
            (
                "shell.rs",
                "fn activate_context_menu_item",
                "菜单动作分派处理函数",
            ),
            ("shell.rs", "MenuOrigin::Editor", "编辑器菜单源分派臂"),
            ("shell.rs", "MenuOrigin::FileTree", "文件树菜单源分派臂"),
        ],
    );
}

// ---------------------------------------------------------------------------
// 面 2：快捷键 → Cmd 映射（shell 级键位——编辑器级由 command_coverage 钉）
// ---------------------------------------------------------------------------

/// shell 级键钩子的六大命令臂：Ctrl+Tab 循环 / Ctrl+W 关标签 / Ctrl+O
/// 打开 / Ctrl+Shift+S 另存为 / Ctrl+=/−/0 字号 / Ctrl+T 主题。
/// （编辑器级键臂 ↔ Cmd 声称的对账在 command_coverage.rs
/// `default_keys_reach_dispatch_key`——两级互补。）
///
/// #217：键位条件改 keymap 表驱动（哪个键绑哪个动作由 keymap.rs 内嵌
/// 单测逐臂钉定 + harness `#[path]` 双环境执行）——本面证据升级为
/// **动作变体 → 处理调用** 的执行臂对账（表动作无执行体 = 按键无反应）。
#[test]
fn face2_shell_shortcuts_reach_commands() {
    let shell = src("shell.rs");
    // 键钩子区域：`on_key_down(cx.listener(…))` 起、下一锚点止（不取
    // 整个 render 体——format! 字符串里的花括号会干扰函数体提取）。
    let hook_start = shell
        .find("on_key_down(cx.listener")
        .expect("shell 级键钩子缺失（on_key_down listener）——全部 shell 级快捷键失灵");
    let hook_end = shell[hook_start..]
        .find("fn dir_for_new")
        .map(|i| hook_start + i)
        .unwrap_or(shell.len());
    let key_block = &shell[hook_start..hook_end];
    for (variant, call, what) in [
        ("ShellAction::NextTab", "cycle_tab", "Ctrl+Tab 循环标签"),
        (
            "ShellAction::CloseTab",
            "close_active_tab",
            "Ctrl+W 关闭标签",
        ),
        (
            "ShellAction::PromptOpenFile",
            "prompt_open_file",
            "Ctrl+O 打开文件",
        ),
        (
            "ShellAction::PromptSaveAs",
            "prompt_save_as",
            "Ctrl+Shift+S 另存为",
        ),
        (
            "ShellAction::FontBigger",
            "apply_font_size",
            "Ctrl+= 字号放大",
        ),
        ("ShellAction::CycleTheme", "cycle_theme", "Ctrl+T 主题循环"),
    ] {
        assert!(
            key_block.contains(variant) && key_block.contains(call),
            "【面 2：快捷键 → Cmd 映射】shell 级键臂断链——{what}：\
             动作臂 `{variant}` 或处理调用 `{call}(` 不在键钩子内"
        );
    }
}

// ---------------------------------------------------------------------------
// 面 3：面板切换 → 状态变更
// ---------------------------------------------------------------------------

/// 图标条钮（文件树/git/设置）+ 右缘 dock 条钮（属性/来源）→ shell
/// toggle 方法 → 状态字段翻转 → `cx.notify()`（状态变更必须触发重绘，
/// 否则「点了没反应」）。
///
/// #289-⑤：设置钮打开**弹窗**（`toggle_settings` → `settings_open` 翻转），
/// 右栏开关移至右缘 dock 条（`right_dock.rs` 的 `dock-toggle-right`）——
/// 左图标条只管左栏。
#[test]
fn face3_panel_toggle_changes_state() {
    let bar = src("activity_bar.rs");
    for (id, method) in [
        ("act-file-tree", "toggle_left"),
        ("act-git", "toggle_git"),
        ("act-settings", "toggle_settings"),
    ] {
        assert!(
            bar.contains(id) && bar.contains(&format!("this.{method}(cx)")),
            "【面 3：面板切换 → 状态变更】图标条断链——{id} 未接 this.{method}(cx)"
        );
    }
    // 右缘 dock 条（#289-⑤）：右栏开关归右缘。
    let dock = src("right_dock.rs");
    assert!(
        dock.contains("dock-toggle-right") && dock.contains("this.toggle_right(cx)"),
        "【面 3：面板切换 → 状态变更】右缘 dock 条断链——dock-toggle-right 未接 this.toggle_right(cx)"
    );
    // 左图标条不再承载右栏开关（#289-⑤ 语义收口）。
    assert!(
        !bar.contains("act-toggle-right"),
        "【面 3：面板切换 → 状态变更】act-toggle-right 不应在左图标条（#289-⑤：右栏开关归右缘 dock 条）"
    );
    // 状态变更体：字段翻转 + notify（渲染层可见的前提）。
    let shell = src("shell.rs");
    for (decl, flip, what) in [
        (
            "fn toggle_left(",
            "left_open = !self.left_open",
            "左栏开合字段翻转",
        ),
        ("fn toggle_git(", "git_view = true", "git 视图置位"),
        (
            "fn toggle_settings(",
            "settings_open = !self.settings_open",
            "设置弹窗开合字段翻转",
        ),
        (
            "fn toggle_right(",
            "right_open = !self.right_open",
            "右栏开合字段翻转",
        ),
    ] {
        let body = fn_body(&shell, decl);
        assert!(
            body.contains(flip) && body.contains("cx.notify()"),
            "【面 3：面板切换 → 状态变更】{decl} 状态变更断链——缺「{what}」\
             或缺 cx.notify()（状态变了但不重绘 = 用户看不见）"
        );
    }
}

// ---------------------------------------------------------------------------
// 面 4：文件树点击 → 打开文件（实装为单击打开）
// ---------------------------------------------------------------------------

/// 文件树行左键 → `open_document_tab`（shell 通道）→ shell 方法存在且
/// 被 F1（Ctrl+O 对话框回调）共用——「从树里打开文件」与「对话框打开」
/// 同一条链。
#[test]
fn face4_file_tree_click_opens_document() {
    let row = fn_body(&src("file_tree.rs"), "fn render_row(");
    assert!(
        row.contains("MouseButton::Left") && row.contains("open_document_tab"),
        "【面 4：文件树点击 → 打开文件】行点击断链——render_row 缺左键处理或\
         open_document_tab 调用（实装口径：单击打开）"
    );
    chain(
        "文件树点击 → 打开文件",
        &[
            (
                "shell.rs",
                "pub fn open_document_tab(",
                "打开文件入标签的 shell 方法（文件树与 Ctrl+O 共用通道）",
            ),
            (
                "shell.rs",
                "fn open_document(",
                "对话框回调同通道（F1）——两条入口汇聚",
            ),
        ],
    );
}

// ---------------------------------------------------------------------------
// 面 5：状态栏 → 数据源
// ---------------------------------------------------------------------------

/// 状态栏四项活数据（选中字数 / git 分支 / 编码 / 换行符）全部经
/// `status_info` 纯函数计算 + shell 渲染状态栏（消费 Editor 现场态）。
#[test]
fn face5_status_bar_binds_data_sources() {
    let sb = src("status_bar.rs");
    for (call, what) in [
        ("status_info::selection_info", "选中字数（现场 selection）"),
        ("status_info::find_git_root", "git 仓库探测"),
        ("status_info::git_branch_at", "git 分支名"),
        ("status_info::detect_encoding", "文件编码探测"),
        ("status_info::detect_eol", "换行符探测"),
    ] {
        assert!(
            sb.contains(call),
            "【面 5：状态栏 → 数据源】数据项断链——{what}（{call}）未被状态栏消费"
        );
    }
    chain(
        "状态栏 → 数据源",
        &[(
            "shell.rs",
            "status_bar::status_bar(",
            "shell 渲染状态栏（Editor 现场态注入）",
        )],
    );
}

// ---------------------------------------------------------------------------
// 面 6：设置项 → 生效
// ---------------------------------------------------------------------------

/// 设置面板四项（主题/字号/缩进/面板宽度）→ shell 生效方法 → 持久化
/// （save_* 落盘）。「改了设置界面有变化 + 重启还在」的完整链。
#[test]
fn face6_settings_changes_take_effect() {
    let panel = src("settings_panel.rs");
    for (call, what) in [
        ("this.set_theme_mode(", "主题三态钮"),
        ("this.apply_font_size(", "字号 −/＋ 步进"),
        ("this.set_indent_unit_all(", "缩进单位三态钮"),
        ("this.reset_panel_widths(", "面板宽度重置钮"),
    ] {
        assert!(
            panel.contains(call),
            "【面 6：设置项 → 生效】设置面板断链——{what} 未接 {call}…)"
        );
    }
    // 生效体：运行时应用 + 落盘（重启还在）。
    let shell = src("shell.rs");
    for (decl, apply, persist, what) in [
        (
            "fn set_theme_mode(",
            "theme_switch::apply_mode",
            "save_theme_mode",
            "主题",
        ),
        (
            "fn apply_font_size(",
            "set_font_size",
            "save_font_size",
            "字号",
        ),
        (
            "fn set_indent_unit_all(",
            "set_indent_unit",
            "save_indent_unit",
            "缩进单位",
        ),
        (
            "fn reset_panel_widths(",
            "LEFT_PANEL_WIDTH",
            "save_panel_widths",
            "面板宽度",
        ),
    ] {
        let body = fn_body(&shell, decl);
        assert!(
            body.contains(apply) && body.contains(persist),
            "【面 6：设置项 → 生效】{what}生效断链——{decl} 缺运行时应用\
             （{apply}）或缺持久化（{persist}——重启丢设置）"
        );
    }
}
