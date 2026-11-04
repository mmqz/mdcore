//! #169：命令系统「零桩」声称的全覆盖断言。
//!
//! ## 现状（被验证的声称）
//!
//! `commands.rs:10` 注释：**「面板只列可执行的——零桩：未接线的不入
//! 表」**。`Cmd::ALL` 22 条命令，面板/键位/RPC 三入口都声称可用——
//! 但「零桩」没有任何测试保证：
//!
//! ```
//! ① Cmd 枚举加了变体，忘加进 ALL      → 面板搜不到（用户不可发现）
//! ② ALL 里的命令，exec_command 空体   → 面板能选中、执行无效果（死桩）
//! ③ exec_command 靠 `_ =>` 通配兜底    → 某命令静默漏接，编译器不报
//! ④ default_key 声称了键位，dispatch_key 没有分派 → 按键无反应
//! ⑤ 标题/键位重复                     → 面板去重错乱、键位冲突
//! ```
//!
//! 这五种形态各自有断言钉死（见下表）。全部当前绿——命令系统现有实现
//! 是齐的，本文件把「齐」变成**回归保护**：任何人破坏其中一条，CI 立红。
//!
//! | # | 断言 | 手段 |
//! |---|---|---|
//! | ① | `enum_variants_match_all_list` | 静态解析 commands.rs 的枚举体与 ALL 表 |
//! | ② | `exec_command_arms_cover_all_without_stubs` | 静态解析 mod.rs 的 match arm：全量在场 + 无空体 |
//! | ③ | 同上（`_ =>` 通配检测） | 静态 |
//! | ④ | `default_keys_reach_dispatch_key` | default_key 键序 → keymap 默认表解析（行为级）+ dispatch_key_inner 执行臂（源码级）——#217 表化双证据 |
//! | ⑤ | `titles_and_keys_unique` | 可执行（title/default_key 全局唯一） |
//!
//! ## 为什么静态解析而不是构造 Editor 执行
//!
//! `exec_command` 需要 GPUI `Window`/`Context`（`cargo test` 无窗口环境
//! 构造不出）。「不改产线」铁律下（git diff 只含 tests/），静态 match arm
//! 对账是可达的最强验证面：match 无 `_` 时 Rust 编译器已保证枚举穷尽
//! （arm 缺失编不过），所以剩余风险恰是 ②空体桩 与 ③通配兜底——静态
//! 扫描精确覆盖这两条。行为面由 RPC 烟雾通道（rpc_smoke.sh）承担。
//!
//! ## 铁律
//!
//! - 只加测试，不改产线代码；
//! - 发现空壳命令 → 开 issue，不写成「已知问题」放过；
//! - 本文件断言失败时指明具体命令名。

use std::path::{Path, PathBuf};

use mang_shell::editor::commands::{self, Cmd};
use mang_shell::keymap;

// ---------------------------------------------------------------------------
// 源码读取与函数体提取（与 wiring_integrity.rs 同款工具——本文件独立
// 自足，不跨测试文件共享：每个集成测试是独立 crate）
// ---------------------------------------------------------------------------

fn src(rel: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("源码文件缺失/不可读（{}）：{e}", path.display()))
}

/// 函数体/枚举体提取：`decl`（如 `"fn exec_command("` / `"enum Cmd {"`）
/// 起首个 `{` 到配对 `}`（字符串字面量感知——`"{"`/`format!("{x}")` 不计深度）。
fn brace_block(file: &str, decl: &str) -> String {
    let start = file.find(decl).unwrap_or_else(|| {
        panic!("静态锚点缺失：源码里找不到 `{decl}`——命令面结构变化，请同步本测试")
    });
    let open = file[start..]
        .find('{')
        .unwrap_or_else(|| panic!("`{decl}` 后无 `{{`"));
    let bytes = file.as_bytes();
    let mut i = start + open;
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
                return file[start + open..=i].to_string();
            }
        }
        i += 1;
    }
    panic!("`{decl}` 大括号不配对（字符串感知扫描到文件尾）");
}

// ---------------------------------------------------------------------------
// ① 枚举变体 ↔ Cmd::ALL 双向对账
// ---------------------------------------------------------------------------

/// 提取 `pub enum Cmd { ... }` 的变体名（4 空格缩进、无字段形态）。
fn enum_variants(file: &str) -> Vec<String> {
    let body = brace_block(file, "enum Cmd {");
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim_end();
        if let Some(rest) = t.strip_prefix("    ") {
            let rest = rest.trim_end();
            if let Some(name) = rest.strip_suffix(',') {
                let name = name.trim();
                // 变体行：`MoveLeft,`（无字段；文档注释/属性行跳过）。
                if !name.is_empty()
                    && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

/// 提取 `const ALL: &[Cmd] = &[ ... ];` 列表项。
fn all_list(file: &str) -> Vec<String> {
    let seg = file
        .split("const ALL: &[Cmd]")
        .nth(1)
        .unwrap_or_else(|| panic!("Cmd::ALL 表缺失"));
    let end = seg.find("];").expect("ALL 表未闭合");
    let mut out = Vec::new();
    for line in seg[..end].lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Cmd::") {
            if let Some(name) = rest.strip_suffix(',') {
                out.push(name.trim().to_string());
            }
        }
    }
    out
}

/// ①`enum_variants_match_all_list`：枚举每加一个变体，`ALL` 必须同步——
/// 否则面板/RPC `list_commands` 都看不到该命令（用户不可发现 ≠ 零桩）。
#[test]
fn enum_variants_match_all_list() {
    let file = src("editor/commands.rs");
    let variants = enum_variants(&file);
    let all = all_list(&file);

    assert!(
        variants.len() >= 20,
        "Cmd 枚举变体数 {} 异常偏少（被重构？请同步测试）",
        variants.len()
    );
    assert_eq!(
        variants.len(),
        Cmd::ALL.len(),
        "源码枚举变体 {} 条 vs 运行时 Cmd::ALL {} 条——测试与源码不同步",
        variants.len(),
        Cmd::ALL.len()
    );

    let missing: Vec<_> = variants
        .iter()
        .filter(|v| !all.iter().any(|a| a == *v))
        .collect();
    assert!(
        missing.is_empty(),
        "枚举变体未列入 Cmd::ALL（面板/RPC 不可见）：{missing:?}——\
         新命令必须同时进 ALL（commands.rs 注释：「零桩：未接线的不入表」\
         的前提是接线了就入表）"
    );

    let ghosts: Vec<_> = all
        .iter()
        .filter(|a| !variants.iter().any(|v| v == *a))
        .collect();
    assert!(
        ghosts.is_empty(),
        "Cmd::ALL 列出了枚举没有的变体：{ghosts:?}——ALL 表引用了不存在的命令"
    );
}

// ---------------------------------------------------------------------------
// ②③ exec_command：arm 全量在场 + 无空桩 + 无通配兜底
// ---------------------------------------------------------------------------

/// ② `exec_command_arms_cover_all_without_stubs`：零桩声称的核心断言。
///
/// 三层：
/// - 每个 `Cmd::ALL` 命令在 `exec_command` 的 match 里**有专属 arm**；
/// - 没有任何 arm 是空体（`=> {}` / `=> ()`——面板选中执行无效果的死桩）；
/// - match **无 `_ =>` 通配**（有通配时删掉某命令的 arm 仍能编译——
///   「加了命令但没实现」从编译错误退化为静默漏接）。
#[test]
fn exec_command_arms_cover_all_without_stubs() {
    let file = src("editor/mod.rs");
    let body = brace_block(&file, "fn exec_command(");

    // ③ 无 `_ =>` 通配（先查——通配存在时「有专属 arm」断言会误导）。
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("_ =>") {
            panic!(
                "exec_command 存在 `_ =>` 通配分支（{}）——\
                 命令漏接会静默通过编译。每个 Cmd 变体必须有专属 arm",
                t
            );
        }
    }

    // ② arm 在场对账。
    let arms: Vec<String> = body
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix("Cmd::")
                .map(|r| r.split_whitespace().next().unwrap_or(r).to_string())
        })
        .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .collect();
    for &c in Cmd::ALL {
        let variant = format!("{c:?}");
        assert!(
            arms.iter().any(|a| *a == variant),
            "{variant} 在 Cmd::ALL 里（面板可选中）但 exec_command 无对应 arm——\
             死桩：面板/键位/RPC 三入口执行都无效果"
        );
    }

    // ② 空桩检测：`Cmd::X => {}` / `Cmd::X => ()`（跨行空体——匹配 =>
    // 后仅空白到闭合括号/下一个 arm）。
    let bytes = body.as_bytes();
    let mut i = 0;
    while let Some(pos) = body[i..].find("=>") {
        let at = i + pos;
        let mut j = at + 2;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        // 空体三种形态：`{}`（同行空块）、`()`（单元）、`{ \n }`（跨行空块）。
        if bytes.get(j..j + 2) == Some(b"{}") || bytes.get(j..j + 2) == Some(b"()") {
            panic_stub(&body, at, "空体 arm");
        }
        if bytes.get(j) == Some(&b'{') {
            // 块体：找到配对 } ——若中间只有空白则是空块。
            let mut k = j + 1;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if bytes.get(k) == Some(&b'}') {
                panic_stub(&body, at, "空块 arm（体仅空白）");
            }
        }
        i = at + 2;
    }
}

fn panic_stub(body: &str, at: usize, kind: &str) -> ! {
    // 失败信息回溯到 arm 头（`Cmd::Xxx =>`）。
    let head_start = body[..at].rfind("Cmd::").unwrap_or(0);
    let head: String = body[head_start..at + 2]
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    panic!(
        "exec_command 存在{kind}：`{head}`——零桩声称被打破：\
         该命令在面板/键位/RPC 可被触发，但执行无任何效果。\
         要么实现它，要么从 Cmd::ALL 移除（#169 铁律：发现空壳开 issue，不放过）"
    );
}

// ---------------------------------------------------------------------------
// ④ default_key → dispatch_key 键位分派可达
// ---------------------------------------------------------------------------

/// display 键序解析（`Ctrl+K Ctrl+0` → [(k,ctrl), (0,ctrl)]——chord
/// 空格分段、`+` 分修饰/主键、箭头形转键名；修饰词大小写不敏感）。
fn key_sequence(display: &str) -> Vec<(String, bool, bool, bool)> {
    let mut out = Vec::new();
    for chord in display.split(' ') {
        let mods: Vec<&str> = chord.split('+').collect();
        let main = mods.last().copied().unwrap_or("");
        let ctrl = mods.iter().any(|m| m.eq_ignore_ascii_case("ctrl"));
        let shift = mods.iter().any(|m| m.eq_ignore_ascii_case("shift"));
        let alt = mods.iter().any(|m| m.eq_ignore_ascii_case("alt"));
        let name = match main {
            "←" => "left",
            "→" => "right",
            "↑" => "up",
            "↓" => "down",
            other => other,
        }
        .to_lowercase();
        out.push((name, ctrl, shift, alt));
    }
    out
}

fn query(seq: &(String, bool, bool, bool)) -> keymap::KeyQuery {
    keymap::KeyQuery {
        key: seq.0.clone(),
        ctrl: seq.1,
        shift: seq.2,
        alt: seq.3,
    }
}

/// ④ `default_keys_reach_dispatch_key`：每个命令声称的默认键位必须真有
/// 分派——#217 键位表化后的**双证据**：
///
/// ① **行为级**：default_key 键序在 keymap 默认表（= 原两处匹配臂的
///    精确镜像）解析出动作（chord 两步：首键挂起 + 续接命中）；
/// ② **源码级**：该动作在 `dispatch_key_inner` 执行层有对应臂/查表点
///    （表动作无执行体 = 按键无反应）。
///
/// 键条件本身（哪个键绑哪个动作）由 keymap.rs 内嵌单测逐臂钉定
/// （`editor_move_family_all_modifier_combos` / `shell_single_key_branches`
/// 等——harness `#[path]` 双环境同源执行）；本测试只钉「声称→分派」链
/// 不脱节。#164 1c：dispatch_key 拆为计时包装 + `dispatch_key_inner`
/// 实体（键分派臂全部在 inner；包装层只做 MANG_PERF 计时）——两体并集
/// 取证，意图不变。
#[test]
fn default_keys_reach_dispatch_key() {
    let file = src("editor/mod.rs");
    let body = format!(
        "{}\n{}",
        brace_block(&file, "fn dispatch_key("),
        brace_block(&file, "fn dispatch_key_inner(")
    );
    let store = keymap::KeymapStore::default_tables();

    for &c in Cmd::ALL {
        let Some(display) = c.default_key() else {
            panic!("{c:?} 无默认键位——commands.rs 的基线断言应已抓到");
        };
        let seq = key_sequence(display);
        assert!(
            !seq.is_empty(),
            "{c:?} 键位 {display:?} 解析不出键序——key_sequence 需适配新键形态"
        );
        // ① 行为级：默认表解析（GotoLine 在前置层查表点，其余主层）。
        let action = if c == Cmd::GotoLine {
            match store.resolve_pre(keymap::Site::EditorGoto, &query(&seq[0])) {
                Some(keymap::Action::Editor(a)) => a,
                other => panic!("{c:?} 键位 {display:?} 前置层未解析出动作：{other:?}"),
            }
        } else {
            let first = query(&seq[0]);
            let outcome = match seq.len() {
                1 => store.resolve_editor_main(&first, None),
                2 => {
                    assert!(
                        matches!(
                            store.resolve_editor_main(&first, None),
                            keymap::EditorOutcome::Pending
                        ),
                        "{c:?} 键位 {display:?} 首键未挂起（chord 机制断链）"
                    );
                    store.resolve_editor_main(&query(&seq[1]), Some(&first))
                }
                n => panic!("{c:?} 键位 {display:?} {n} 段——keymap 至多两键"),
            };
            match outcome {
                keymap::EditorOutcome::Matched(a) => a,
                other => panic!("{c:?} 键位 {display:?} 主层未解析出动作：{other:?}"),
            }
        };
        // ② 源码级：执行层臂在场（表动作 → dispatch_key_inner 执行体）。
        let variant = format!("{action:?}")
            .split('(')
            .next()
            .unwrap_or_default()
            .to_string();
        assert!(
            body.contains(&format!("EditorAction::{variant}")),
            "{c:?} 声称默认键位 {display:?}（→ {variant}），但 dispatch_key_inner \
             无 {variant} 执行臂——按键无反应（键位声称与实现脱节）"
        );
    }

    // 序列键机制在位（#217：`ctrl_k_pending` 布尔 → `chord_pending`
    // 挂起快照——证据面同步）。
    if Cmd::ALL
        .iter()
        .any(|c| c.default_key().is_some_and(|k| k.contains(' ')))
    {
        assert!(
            body.contains("chord_pending"),
            "存在序列键位（Ctrl+K …）但 dispatch_key 无 chord_pending 挂起机制"
        );
    }
}

// ---------------------------------------------------------------------------
// ⑤ 标题 / 键位唯一性（commands.rs 注释声称「面板去重依赖」——钉死）
// ---------------------------------------------------------------------------

/// ⑤ title 与 default_key 全局唯一。`every_wired_cmd_has_title_and_key`
/// （commands.rs 内嵌测试）断言了非空，但注释里写的「唯一（面板去重
/// 依赖）」从未被断言——重复标题会让模糊搜索命中两条同标题命令。
#[test]
fn titles_and_keys_unique() {
    let n = Cmd::ALL.len();

    let mut titles: Vec<&str> = Cmd::ALL.iter().map(|c| c.title()).collect();
    titles.sort();
    let before = titles.len();
    titles.dedup();
    assert_eq!(
        titles.len(),
        before,
        "命令标题重复（面板去重依赖标题唯一）：{titles:?}"
    );

    let mut keys: Vec<&str> = Cmd::ALL.iter().filter_map(|c| c.default_key()).collect();
    keys.sort();
    let before = keys.len();
    keys.dedup();
    assert_eq!(
        keys.len(),
        before,
        "默认键位冲突（两命令同键，先到者吞键）：{keys:?}"
    );
    let _ = n;
}

// ---------------------------------------------------------------------------
// RPC 机器名面：run_command / list_commands 全覆盖
// ---------------------------------------------------------------------------

/// RPC `run_command` 按机器名调命令（#134 口径）：
/// - 每个 `Cmd::ALL` 命令的机器名可经 `Cmd::from_id` 反查命中自身
///   （外控通道可达）；
/// - `cmd_run_command` 的实现确实走 `Cmd::from_id`（fail-loud：未知名
///   Err 指引 `list_commands`，不静默吞）；
/// - `cmd_list_commands` 以 `Cmd::ALL` 为全集（面板/键位/RPC 三入口
///   同一候选集）。
#[test]
fn rpc_machine_names_cover_all_commands() {
    // ① 可执行面：机器名 ↔ 命令双射（RPC 外控可达性）。
    for &c in Cmd::ALL {
        let id = c.id();
        assert_eq!(
            commands::Cmd::from_id(id),
            Some(c),
            "机器名 {id} 反查不中自身——RPC run_command 无法触达 {c:?}"
        );
    }
    // 机器名全局唯一（外控通道无歧义）。
    let mut ids: Vec<&str> = Cmd::ALL.iter().map(|c| c.id()).collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "RPC 机器名重复：{ids:?}");

    // ②③ 静态面：RPC 处理器确实走 from_id / ALL（不经别的旁路）。
    let file = src("rpc.rs");
    let run = brace_block(&file, "fn cmd_run_command(");
    assert!(
        run.contains("Cmd::from_id"),
        "cmd_run_command 不经 Cmd::from_id——RPC 命令入口被旁路（覆盖面断裂）"
    );
    assert!(
        run.contains("run_command"),
        "cmd_run_command 未落到 Editor::run_command——RPC 只查不执行"
    );
    let list = brace_block(&file, "fn cmd_list_commands(");
    assert!(
        list.contains("Cmd::ALL"),
        "cmd_list_commands 不以 Cmd::ALL 为全集——RPC 与面板候选集漂移"
    );
}

// ---------------------------------------------------------------------------
// 面板搜索可达性（每条命令都能被用户搜到）
// ---------------------------------------------------------------------------

/// 每条命令必须可经标题搜到（完整标题 / 去掉快捷键括号的标题两种
/// 查询都命中自身）——面板搜索是命令的主发现路径，搜不到 = 功能性
/// 不可达（虽在候选集里，用户永远找不到）。
#[test]
fn palette_search_reaches_every_command() {
    for &c in Cmd::ALL {
        let title = c.title();
        // ① 完整标题查询（子序列自含——必命中）。
        let full = commands::search(title);
        assert!(
            full.contains(&c),
            "{c:?} 用自身标题 {title:?} 搜索竟不命中——search 子序列匹配有缺陷"
        );
        // ② 去掉「（Ctrl+X）」快捷键注记后的纯标题（用户实际输入形态）。
        if let Some(open) = title.rfind('（') {
            let bare = &title[..open];
            if !bare.is_empty() {
                let hit = commands::search(bare);
                assert!(
                    hit.contains(&c),
                    "{c:?} 用纯标题 {bare:?} 搜索不命中——用户按命令名搜不到"
                );
            }
        }
    }
}
