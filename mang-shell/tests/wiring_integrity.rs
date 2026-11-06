//! #168：界面层「接线完整性」回归测试。
//!
//! ## 为什么
//!
//! mang-shell 约 2.8 万行，`tests/` 只有 docx/括号测试——界面层零回归
//! 保护。「按钮是空闭包」这类问题（owner 2026-11-02 release 实测发现，
//! 登记 #165/#166）能一路存活到发布，因为没有任何测试会抓它。
//!
//! ## 测什么（边界：不测 UI 渲染，只测接线表的完整性）
//!
//! | 模块 | 手段 |
//! |---|---|
//! | activity_bar.rs | 静态扫描：`bar_button` 的 on_click 空闭包 = 0；tooltip 无「未接线/骨架」 |
//! | file_tree.rs | ① 数据面（`context_menu::file_tree_menu` 的 action id）↔ 接线面（shell.rs 的 match 分支）双向对账；② 行点击 → 打开文档 / 右键 → reveal 的接线存在性 |
//! | tab_bar.rs | 标签点击 → `activate_tab` 接线存在 + 空闭包 = 0 |
//! | palette_view.rs | 数据链（`commands::search` → `command_groups::build_sections` → `flatten`）零丢失——面板能列出全部命令 |
//! | context_menu_view.rs | 菜单项 on_click → `activate_context_menu_item` 接线存在 + 空闭包 = 0 |
//! | title_bar.rs | 窗口控制钮（最小化/最大化/关闭）接线存在 + 空闭包 = 0 |
//!
//! ## 静态扫描的取舍（issue #168 方案 2 + 3 混合）
//!
//! GPUI 组件（`Context`/`Window`）无法在无窗口环境构造，按钮回调是
//! 渲染期闭包、无注册表可枚举——「不改产线」约束下（铁律：git diff
//! 只含 tests/），静态源码扫描是唯一可行的接线验证面。文本匹配固有
//! 边界：换一种写法的空回调（如引用空函数）抓不到——但当前形态
//! （`|_event, _window, _cx| {}`）与历史 bug 形态一致，能抓真问题。
//! 数据面对账（file_tree/editor 菜单）不受此限：菜单定义是纯数据
//! （`context_menu.rs`），接线分支是静态可解析的 match arm——这条
//! 是**双向强对账**。
//!
//! ## 红绿账（2026-11-02 全绿——协作模式验证成功）
//!
//! #171 预告的协作模式：「B 先写失败态 → A 接线后转绿」。已闭环：
//!
//! | 测试 | 初始 | 转绿依据 |
//! |---|---|---|
//! | `activity_bar_no_dead_buttons` | 🔴（act-search/act-settings 空闭包） | PR #172（#165）：搜索钮接 OpenFind、设置钮按「接不了就移除」铁律删除 |
//! | `file_tree_context_menu_actions_all_wired` | 🔴（ft-* 五项无接线） | PR #174（#166）：触发器/浮层挂载/六动作全接 |
//!
//! 绝不允许 xfail 放过（#171：「那等于给断线发许可证」）——未来新断线
//! 出现时，这两条测试会重新变红，那是它们的职责。
//!
//! ## 铁律
//!
//! - 只加测试，不改产线代码（git diff 只含 tests/）；
//! - 源码文件缺失 = FAIL（不是环境缺件——源码就在仓库里）；
//! - 失败信息必须指明「哪个按钮/哪个菜单项没接线」。

use std::path::{Path, PathBuf};

use regex::Regex;

use mang_shell::command_groups::{RecentRing, build_sections, flatten};
use mang_shell::context_menu::{MenuItem, editor_menu, file_tree_menu};
use mang_shell::editor::commands::{self, Cmd};

// ---------------------------------------------------------------------------
// 源码读取与函数体提取（静态检查面工具）
// ---------------------------------------------------------------------------

/// 读 `mang-shell/src/<rel>` 源码。缺失即 FAIL（源码进仓，不是环境件）。
fn src(rel: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("源码文件缺失/不可读（{}）：{e}", path.display()))
}

/// 源码字节偏移 → 1 基行号（失败信息定位用）。
fn line_of(file: &str, byte_idx: usize) -> usize {
    file[..byte_idx.min(file.len())].matches('\n').count() + 1
}

/// 提取 `decl`（如 `"fn exec_command("`）所在函数的**函数体**（含首尾
/// 大括号）。
///
/// 大括号配对，字符串字面量感知（`"{"` 不计深度）；目标函数（mod.rs 的
/// 方法、shell.rs 的方法）无 char 字面量与生命周期标注，无需处理 `'`。
fn fn_body(file: &str, decl: &str) -> String {
    let start = file.find(decl).unwrap_or_else(|| {
        panic!("静态锚点缺失：源码里找不到 `{decl}`——函数被改名/删除？接线检查失效，请同步测试")
    });
    let body_open = file[start..]
        .find('{')
        .unwrap_or_else(|| panic!("`{decl}` 后无函数体开括号"));
    let bytes = file.as_bytes();
    let mut i = start + body_open;
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
                return file[start + body_open..=i].to_string();
            }
        }
        i += 1;
    }
    panic!("`{decl}` 函数体大括号不配对（字符串感知扫描到文件尾）");
}

/// 文件里所有字符串字面量（粗提取——供「未接线」措辞扫描）。
fn string_literals(file: &str) -> Vec<(usize, String)> {
    let bytes = file.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            let mut escaped = false;
            while j < bytes.len() {
                let c = bytes[j];
                if escaped {
                    escaped = false;
                } else if c == b'\\' {
                    escaped = true;
                } else if c == b'"' {
                    break;
                }
                j += 1;
            }
            if j < bytes.len() {
                out.push((start, file[start..j].to_string()));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 空闭包（参数全下划线 + 空体）扫描——「死按钮」的直接形态。
///
/// 命中形态：`|_event, _window, _cx| {}` / `|_, _| {}` /
/// `|_, _, _, cx| {}` 不命中（`cx` 非下划线参数、体非空都不算）。
fn dead_closures(file: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let bytes = file.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'|' {
            // 尝试从这里的管道参数列表解析。
            if let Some((close, params)) = parse_closure_params(bytes, i) {
                // close = `|` 位置；体 = 其后跳过空白后恰为 `{}`（可含空白）。
                let mut j = close + 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j + 1 < bytes.len() && bytes[j] == b'{' && bytes[j + 1] == b'}' {
                    let body = format!("|{params}| {{}}");
                    out.push((i, body));
                }
            }
        }
        i += 1;
    }
    out
}

/// 从 `pos`（`|` 处）解析闭包参数列表：返回 (收尾 `|` 的字节位, 参数文本)。
/// 参数**全部**为 `_` / `_name` 形态才返回 Some——有任何具名参数即 None。
fn parse_closure_params(bytes: &[u8], pos: usize) -> Option<(usize, String)> {
    let mut j = pos + 1;
    let mut params = String::new();
    let mut saw_param = false;
    loop {
        // 跳过空白。
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            params.push(bytes[j] as char);
            j += 1;
        }
        if j >= bytes.len() {
            return None;
        }
        if bytes[j] == b'|' {
            return if saw_param { Some((j, params)) } else { None };
        }
        // 一个参数：必须 `_` 或 `_ident`。
        if bytes[j] != b'_' {
            return None;
        }
        saw_param = true;
        params.push('_');
        j += 1;
        while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
            params.push(bytes[j] as char);
            j += 1;
        }
        // 参数后必须是 `,` 或 `|`（空白已由循环头处理）。
        if j < bytes.len() && bytes[j] == b',' {
            params.push(',');
            j += 1;
            continue;
        }
        if j < bytes.len() && bytes[j] == b'|' {
            return Some((j, params));
        }
        return None;
    }
}

/// 按钮上下文定位：从空闭包位置向前找最近的按钮 id 字面量与其后的
/// tooltip 字面量（bar_button 实参序：id → icon → selected → tooltip）。
fn nearest_button_context(file: &str, closure_pos: usize) -> (String, String) {
    let mut id = String::from("<未知 id>");
    let mut tooltip = String::new();
    for (pos, lit) in string_literals(file) {
        if pos >= closure_pos {
            break;
        }
        if lit.starts_with("act-") {
            id = lit;
            tooltip.clear(); // id 变了，旧 tooltip 作废。
        } else if tooltip.is_empty() {
            // id 之后的第一个字面量即 tooltip（后续字面量不覆盖）。
            tooltip = lit;
        }
    }
    (id, tooltip)
}

// ---------------------------------------------------------------------------
// activity_bar：图标条按钮接线
// ---------------------------------------------------------------------------

/// #165/#171：图标条不得有死按钮（空闭包）、「未接线——骨架」措辞。
///
/// 红绿史：初始红（act-search/act-settings 空闭包）→ PR #172（#165）
/// 合入后绿：搜索钮接 Cmd::OpenFind，设置钮按「接不了就从界面移除」
/// 铁律删除。
#[test]
fn activity_bar_no_dead_buttons() {
    let file = src("activity_bar.rs");

    // ① 空闭包 = 0（失败信息带按钮 id / tooltip / 行号）。
    let dead = dead_closures(&file);
    if !dead.is_empty() {
        let mut msg = String::from("图标条存在死按钮（on_click 空闭包）——#165 待接线：\n");
        for (pos, body) in &dead {
            let (id, tooltip) = nearest_button_context(&file, *pos);
            msg.push_str(&format!(
                "  - activity_bar.rs:{} 按钮 id={id} tooltip={tooltip:?} 闭包={body}\n",
                line_of(&file, *pos)
            ));
        }
        panic!("{msg}铁律（#171）：接不了就从界面移除，不装作能点。");
    }

    // ② tooltip / 可见文案不得自曝「未接线」「骨架」（#171 验收项）。
    for (pos, lit) in string_literals(&file) {
        for word in ["未接线", "骨架"] {
            if lit.contains(word) {
                panic!(
                    "activity_bar.rs:{} 的界面文案 {:?} 含「{word}」——\
                     #171 验收：界面/悬停提示里不出现「未接线」「骨架」字样",
                    line_of(&file, pos),
                    lit
                );
            }
        }
    }

    // ③ 按钮基线：图标条 4 个按钮（文件树/git/搜索/设置）。
    // #289-⑤：右栏开关移至右缘 dock 条（act-toggle-right → dock-
    // toggle-right，基线 5→4）；设置钮保留（#182 恢复，现开弹窗）。
    // 这不是放过死按钮，而是锚定「现存按钮全部真实接线」这一事实；
    // 未来新增/删除按钮时请同步本基线。
    let buttons = string_literals(&file)
        .into_iter()
        .filter(|(_, lit)| lit.starts_with("act-"))
        .count();
    assert_eq!(
        buttons, 4,
        "图标条按钮数 {buttons} ≠ 4（文件树/git/搜索/设置——#289-⑤ 后基线；按钮增删时同步本基线）"
    );

    // ④ #289-⑤：右缘 dock 条同口径扫描（空闭包 = 0 + 无自曝措辞）
    //    ——bar_button 复用自 activity_bar，同一套铁律。
    let dock = src("right_dock.rs");
    let dead_dock = dead_closures(&dock);
    assert!(
        dead_dock.is_empty(),
        "右缘 dock 条存在死按钮（on_click 空闭包）：{dead_dock:?}——
         铁律（#171）：接不了就从界面移除，不装作能点。"
    );
    for (pos, lit) in string_literals(&dock) {
        for word in ["未接线", "骨架"] {
            assert!(
                !lit.contains(word),
                "right_dock.rs:{} 的界面文案 {:?} 含「{word}」——#171 验收口径同样适用",
                line_of(&dock, pos),
                lit
            );
        }
    }
    assert!(
        dock.contains("dock-toggle-right"),
        "右缘 dock 条缺少 dock-toggle-right（#289-⑤：右栏开关归右缘）"
    );
}

// ---------------------------------------------------------------------------
// 右键菜单：数据面 ↔ 接线面双向对账
// ---------------------------------------------------------------------------

/// 从 shell.rs 的 `activate_context_menu_item` 提取某 origin 的已接线
/// action id 集（match arm `(MenuOrigin::X, "id")` 形态——正则匹配，
/// 不依赖行尾是 `=> {` 还是 `=> expr`）。
fn wired_menu_ids(origin: &str) -> Vec<String> {
    let shell = src("shell.rs");
    let body = fn_body(&shell, "fn activate_context_menu_item(");
    let re = Regex::new(&format!(r#"\({origin},\s*\"([a-z-]+)\"\)"#)).expect("菜单接线分支正则");
    re.captures_iter(&body).map(|c| c[1].to_string()).collect()
}

/// 菜单数据面：id → label（失败信息可读）。
fn menu_labels(items: &[MenuItem]) -> Vec<(&'static str, String)> {
    items
        .iter()
        .filter_map(|i| match i {
            MenuItem::Action { id, label, .. } => Some((*id, label.clone())),
            _ => None,
        })
        .collect()
}

/// 双向对账内核：数据面每个 action id 必须有接线分支（缺 = 死菜单项：
/// 点了只会走 eprintln 兜底）；接线面每个 id 必须在数据面存在（幽灵
/// 接线：接了一个菜单里没有的动作——配置漂移）。
fn assert_menu_fully_wired(origin: &str, data_items: &[MenuItem]) {
    let wired = wired_menu_ids(origin);
    let data = menu_labels(data_items);
    let data_ids: Vec<&str> = data.iter().map(|(id, _)| *id).collect();

    let unwired: Vec<_> = data
        .iter()
        .filter(|(id, _)| !wired.iter().any(|w| w == id))
        .collect();
    if !unwired.is_empty() {
        let mut msg =
            format!("{origin} 菜单存在未接线动作（点击只走 eprintln 兜底，无实际效果）：\n");
        for (id, label) in unwired {
            msg.push_str(&format!("  - id={id} label={label:?}\n"));
        }
        panic!("{msg}接线或从菜单移除（#171「不留死按钮」）——#166 范围。");
    }

    let ghosts: Vec<_> = wired
        .iter()
        .filter(|w| !data_ids.contains(&w.as_str()))
        .collect();
    assert!(
        ghosts.is_empty(),
        "{origin} 接线面存在数据面没有的幽灵动作 id：{ghosts:?}——\
         菜单定义（context_menu.rs）与接线（shell.rs）漂移"
    );
}

/// #166：文件树右键菜单动作全部接线。
///
/// 红绿史：初始红（ft-new-file/ft-new-folder/ft-open/ft-rename/
/// ft-delete 五项无接线分支）→ PR #174（#166）合入后绿：六动作全接、
/// 导入导出死组移除。
#[test]
fn file_tree_context_menu_actions_all_wired() {
    assert_menu_fully_wired("MenuOrigin::FileTree", &file_tree_menu(true));
    // is_dir 只影响 enabled，不影响 id 集——两形态都对账。
    assert_menu_fully_wired("MenuOrigin::FileTree", &file_tree_menu(false));
}

/// 编辑器右键菜单动作全部接线（当前全接——本测试同时是
/// `assert_menu_fully_wired` 检查器的**正例校准**：它对已接线的菜单
/// 必须绿，否则说明检查器本身有 bug、见谁都红）。
#[test]
fn editor_context_menu_actions_all_wired() {
    for (sel, ro) in [(false, false), (true, false), (true, true)] {
        assert_menu_fully_wired("MenuOrigin::Editor", &editor_menu(sel, ro));
    }
}

// ---------------------------------------------------------------------------
// file_tree / tab_bar / title_bar / context_menu_view：交互接线存在性
// ---------------------------------------------------------------------------

/// 文件树行点击：左键打开文档（F1 通道）/ 右键开上下文菜单。
///
/// 右键语义随 #166 演进：旧接线是直接 `reveal_path`（资源管理器定位），
/// PR #174 改为打开上下文菜单（`open_file_tree_context_menu`，携带
/// 按下点坐标与目标路径），「在资源管理器中显示」成为菜单项
/// ft-reveal——其可达性由 `file_tree_context_menu_actions_all_wired`
/// 的数据面↔接线面对账保证（更强：菜单项缺失或未接都会被抓）。
#[test]
fn file_tree_row_clicks_wired() {
    let file = src("file_tree.rs");
    let body = fn_body(&file, "fn render_row(");
    assert!(
        body.contains("MouseButton::Left"),
        "文件树行无左键处理（展开/打开）"
    );
    assert!(
        body.contains("open_document_tab"),
        "文件树左键未接 F1 打开通道（open_document_tab）"
    );
    assert!(
        body.contains("MouseButton::Right") && body.contains("open_file_tree_context_menu"),
        "文件树右键未接上下文菜单（open_file_tree_context_menu）——\
         ft-reveal 等六项菜单动作的入口"
    );
    assert!(
        dead_closures(&file).is_empty(),
        "file_tree.rs 存在空闭包（死交互）"
    );
}

/// 标签栏：点击切换（activate_tab）接线 + 无死闭包。
#[test]
fn tab_bar_click_wired_to_activate() {
    let file = src("tab_bar.rs");
    let body = fn_body(&file, "fn tab_bar(");
    assert!(body.contains("MouseButton::Left"), "标签无左键点击处理");
    assert!(
        body.contains("activate_tab"),
        "标签点击未接 activate_tab（切换失效）"
    );
    assert!(
        dead_closures(&file).is_empty(),
        "tab_bar.rs 存在空闭包（死交互）"
    );
}

/// 标题栏窗口控制钮：最小化/最大化/关闭（owner 实测可用的正例基线）。
#[test]
fn title_bar_window_controls_wired() {
    let file = src("title_bar.rs");
    for evidence in ["minimize_window", "zoom_window", "quit"] {
        assert!(
            file.contains(evidence),
            "标题栏缺 {evidence} 接线——窗口控制钮基线（owner 实测项）"
        );
    }
    assert!(
        dead_closures(&file).is_empty(),
        "title_bar.rs 存在空闭包（死交互）"
    );
}

/// 右键菜单渲染层：可点击项的 on_click 必须落到 shell 的动作派发
/// （activate_context_menu_item）——渲染与接线不脱钩。
#[test]
fn context_menu_view_clicks_dispatch_to_shell() {
    let file = src("context_menu_view.rs");
    let body = fn_body(&file, "fn context_menu_overlay(");
    assert!(
        body.contains("activate_context_menu_item"),
        "菜单项 on_click 未接 activate_context_menu_item（点击无效果）"
    );
    // 灰显项（enabled=false）不挂 on_click 是设计（点不到不算死按钮）。
    assert!(
        dead_closures(&file).is_empty(),
        "context_menu_view.rs 存在空闭包（死交互）"
    );
}

/// #209：菜单栏渲染层接线——标题点击开菜单（open_menu_bar）、遮罩点
/// 击关菜单（close_menu_bar）、条目点击派发动作（activate_menu_action，
/// 与右键菜单 activate_context_menu_item 同款单点）。灰显项不挂
/// on_click 是设计（点不到不算死按钮）。
#[test]
fn menu_bar_view_clicks_dispatch_to_shell() {
    let file = src("menu_bar_view.rs");
    assert!(
        file.contains("open_menu_bar"),
        "菜单标题未接 open_menu_bar（点击不开下拉）"
    );
    assert!(
        file.contains("close_menu_bar"),
        "菜单遮罩未接 close_menu_bar（点外部不关闭）"
    );
    assert!(
        file.contains("activate_menu_action"),
        "菜单项 on_click 未接 activate_menu_action（点击无效果）"
    );
    assert!(
        dead_closures(&file).is_empty(),
        "menu_bar_view.rs 存在空闭包（死交互）"
    );
}

// ---------------------------------------------------------------------------
// palette：命令面板数据链零丢失
// ---------------------------------------------------------------------------

/// 命令面板数据链（search → build_sections → flatten）不丢命令：
/// 空查询下面板能列出全部 `Cmd::ALL`，且键盘导航 index 连续。
///
/// `palette_view::palette_sections` 未导出到 lib（渲染模块），但它只是
/// 这三个已导出纯函数的组合——对组合原料钉定等价于对面板数据面钉定；
/// 另以静态断言钉住 palette_view.rs 的组合关系不变。
#[test]
fn palette_data_covers_all_commands() {
    // ① 空查询：搜索结果 = 全部命令（面板候选集完整性）。
    let results = commands::search("");
    assert_eq!(
        results.len(),
        Cmd::ALL.len(),
        "空查询应列出全部 {} 条命令，实际 {} 条",
        Cmd::ALL.len(),
        results.len()
    );
    for &c in Cmd::ALL {
        assert!(
            results.contains(&c),
            "{c:?} 未进面板候选集（search 漏列——面板搜不到该命令）"
        );
    }

    // ② 分组装配 + 扁平化零丢失（键盘上下移动的遍历域 = 全部命令）。
    let sections = build_sections(&results, &RecentRing::new(4));
    let flat = flatten(&sections);
    assert_eq!(
        flat.len(),
        Cmd::ALL.len(),
        "面板分组装配丢失命令：扁平 {} != 全集 {}",
        flat.len(),
        Cmd::ALL.len()
    );
    for &c in Cmd::ALL {
        assert!(flat.contains(&c), "{c:?} 在分组装配后丢失（键盘选不到）");
    }
    // index 连续从 0 起（键盘导航依赖）。
    for (i, e) in sections.iter().flat_map(|s| s.entries.iter()).enumerate() {
        assert_eq!(
            e.index, i,
            "面板条目 index 不连续：第 {i} 项 index={}",
            e.index
        );
    }

    // ③ 静态钉住 palette_view.rs 的数据来源（组合关系不被换掉）。
    let pv = src("palette_view.rs");
    assert!(
        pv.contains("commands::search"),
        "palette_view 数据源不再是 commands::search"
    );
    assert!(
        pv.contains("build_sections"),
        "palette_view 不再经 build_sections 装配分组"
    );
}
