//! mang 桌面外壳入口 —— GPUI 主线 UI（#63 骨架 / #68 阶段 1 编辑区接线）。
//!
//! 用法：
//! ```text
//! mang-shell [FILE]            正常启动（FILE = .md/.markdown/.txt 打开为
//!                               可编辑文档；.docx 只读渲染（批 3）；
//!                               .pdf 只读渲染（批 4））
//! mang-shell --smoke-sec N     烟雾模式：开窗 N 秒后自动退出（exit 0 +
//!                               stdout 末行 "smoke: quit reached"——Xvfb
//!                               无头验证通道）
//! mang-shell --smoke-mouse     批 2 鼠标烟雾：与 --smoke-sec 组合——合成
//!                               PlatformInput 序列（单击/拖选/双击/三击/
//!                               滚轮）过 Window::dispatch_event 完整
//!                               派发管线，命中结果由 Editor 日志输出
//!                               （需同时带 FILE——空态无编辑区）
//! mang-shell --smoke-open FILE2  F1 替换管线烟雾：与 --smoke-sec 组合——
//!                               开窗 2.5s 后经 ShellApp::open_document
//!                               把 FILE2 换入编辑区（Ctrl+O 对话框回调
//!                               同通道；对话框本体需真机/portal，沙箱
//!                               以此钩子验证替换链）
//! mang-shell --version         版本号
//! ```
//!
//! 引导链：`gpui_platform::application()`（OS 后端选择器）→ `zlog`
//! 日志面（读 `ZED_LOG`/`RUST_LOG`——未设则静默；适配器选择/GPU 枚举
//! 等可观测性前提，#68 批 5 前置）→ 内嵌资产源（图标）→ `theme::init`
//! （内置基础主题）→ `set_theme_settings_provider`（ui 组件字体面契约）
//! → 文档分派（`document::open`）→ `Editor` 实体 → 开窗（1280×800，
//! X11 客户端装饰请求，无合成器自动回退服务端装饰）。
//! 开窗失败（GPU/图形栈不可用）≠ panic 白屏：stderr 双语错误链 +
//! 面向用户的提示文案 + 退出码 70（EX_SOFTWARE）。

use std::time::Duration;

use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions,
    point, px, size,
};
use gpui_platform::application;

mod activity_bar;
mod assets;
mod document;
mod editor;
mod file_tree;
mod git_panel;
mod panels;
mod recent;
#[cfg(unix)]
mod rpc;
mod settings;
mod shell;
mod status_bar;
mod tab_bar;
mod theme_switch;
mod title_bar;

use document::OpenOutcome;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mang-shell {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // 日志面（#68）：zlog 读 ZED_LOG/RUST_LOG（env_config 过滤语法与
    // env_logger 兼容，如 `gpui_wgpu=info,wgpu_hal=debug`）；未设环境变量时
    // 静默——不产生输出面。GPU 适配器枚举（gpui_wgpu wgpu_context）与
    // DirectX 设备选择（gpui_windows directx_devices）的既有 log 调用
    // 由此激活——此前二进制无 logger，RUST_LOG 完全无效。
    zlog::init();
    zlog::init_output_stderr();

    let smoke_sec: Option<f64> = args
        .iter()
        .position(|a| a == "--smoke-sec")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    // F1/F3：标签烟雾钩子（--smoke-open FILE——可多次：每次追加一个
    // 标签，与 Ctrl+O 对话框回调同走 ShellApp::open_document）。
    let smoke_opens: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--smoke-open")
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect();
    // F3：标签键位烟雾（--smoke-tab-keys——与 --smoke-open 组合：开窗
    // 3.5s 合成 Ctrl+Tab 循环 + 4.5s 合成 Ctrl+W 关闭，经完整键派发
    // 管线断言 tabs 证据行）。
    let smoke_tab_keys = args.iter().any(|a| a == "--smoke-tab-keys");
    // F4：保存流烟雾（--smoke-save-flow——启动 FILE 为标签：3.5s 合成
    // 可打印键弄脏 + 4.5s Ctrl+W 弹确认 + 5.5s Enter 保存并关——断言
    // confirm/saved/tabs 证据行 + 落盘内容外部校验）。
    let smoke_save_flow = args.iter().any(|a| a == "--smoke-save-flow");
    // F4：另存为烟雾（--smoke-save-as PATH——3.5s 弄脏 + 4.5s 另存到
    // PATH（与对话框回调同走 save_active_as）+ 5.5s Ctrl+W 干净直关
    // ——断言 saved-as 证据行 + 无 confirm 行 + 落盘内容外部校验）。
    let smoke_save_as = args
        .iter()
        .position(|a| a == "--smoke-save-as")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // F7：git 面板烟雾（--smoke-git-diff——与 --smoke-workspace 组合：
    // 4.5s 打开首个改动的 diff 只读标签；断言 git: 证据行 + diff: 标签）。
    let smoke_git_diff = args.iter().any(|a| a == "--smoke-git-diff");
    // G1：查找替换烟雾（--smoke-find-replace——与启动 FILE 组合：3.5s
    // 合成 Ctrl+H 开替换条 → 输入 query/替换文本 → Tab 切行 →
    // Ctrl+Alt+Enter 全部替换 → Esc 关条 → Ctrl+Z 一次整批撤销；
    // 断言 find-replace: 三条证据行 + quit reached）。
    let smoke_find_replace = args.iter().any(|a| a == "--smoke-find-replace");
    // G2：跳行烟雾（--smoke-goto-line——与启动 FILE 组合：3.5s 合成
    // Ctrl+G → 输入行号 → Enter 跳转；再一轮越界行号 → 断言 goto: 两条
    // 证据行（正常跳转 + 越界 clamp）+ quit reached）。
    let smoke_goto_line = args.iter().any(|a| a == "--smoke-goto-line");
    // G3：缩进烟雾（--smoke-indent——与启动 FILE 组合：home+Shift+↓×3
    // 选 3 行 → Tab 缩进 → Shift+Tab 反缩进 → Ctrl+Z×2 连撤——断言
    // indent:/outdent: 证据行 + quit reached）。
    let smoke_indent = args.iter().any(|a| a == "--smoke-indent");
    // G4：面板拖拽烟雾（--smoke-panel-resize——合成鼠标经完整派发管线：
    // 左栏手柄拖宽 +40 → 右栏手柄拖宽 +60 → 断言 panel-resize: persist
    // 证据行 + settings.json 落盘 + quit）。
    let smoke_panel_resize = args.iter().any(|a| a == "--smoke-panel-resize");
    // F6：主题循环烟雾（--smoke-theme-cycle——3.5s 合成 Ctrl+T；
    // 断言 theme: 证据行 + settings.json theme 键 + 全窗重绘不崩）。
    let smoke_theme_cycle = args.iter().any(|a| a == "--smoke-theme-cycle");
    // F6：字号烟雾（--smoke-zoom N——3.5s 设字号 N[8-32]；断言
    // font-size: 证据行 + settings.json font_size 键）。
    let smoke_zoom = args
        .iter()
        .position(|a| a == "--smoke-zoom")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f32>().ok());
    // F2：工作区烟雾钩子（--smoke-workspace DIR）。
    let smoke_workspace = args
        .iter()
        .position(|a| a == "--smoke-workspace")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // 批 2：鼠标烟雾脚本（需与 --smoke-sec + FILE 组合）。
    let smoke_mouse = args.iter().any(|a| a == "--smoke-mouse");
    // G5：语法高亮烟雾（--smoke-syntax-hl——与启动 FILE 组合：3.5s 经
    // 真实渲染分派路径探针取证；断言 syntax-hl: 证据行——围栏语言态
    // [rust] + kw/str 命中 span + script done）。
    let smoke_syntax_hl = args.iter().any(|a| a == "--smoke-syntax-hl");
    // #104 任务 2：RPC 控制通道旗标（--rpc-sock PATH 覆盖端点；
    // --no-rpc 显式关闭。默认开——端点 $XDG_RUNTIME_DIR/mang-shell.sock
    // 缺省 /tmp/mang-shell-$UID.sock，见 rpc::resolve_socket_path）。
    let rpc_sock: Option<String> = args
        .iter()
        .position(|a| a == "--rpc-sock")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let rpc_disabled = args.iter().any(|a| a == "--no-rpc");

    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            theme::init(theme::LoadThemes::JustBase, cx);
            theme::set_theme_settings_provider(Box::new(settings::ShellThemeSettings), cx);

            // 文档分派：首个非 flag 参数为文件路径（#68 阶段 1.1；跳过
            // 各 flag 的值参数，避免误判为文件）。F1 起与 Ctrl+O 运行时
            // 打开共用 document::open_editor 通道（同一分派语义）。
            let flag_value_positions: Vec<usize> = [
                "--smoke-sec",
                "--smoke-open",
                "--smoke-workspace",
                "--smoke-save-as",
                "--rpc-sock",
            ]
            .iter()
            .flat_map(|flag| {
                args.iter()
                    .enumerate()
                    .filter(|(_, a)| a == flag)
                    .map(|(i, _)| i + 1)
                    .collect::<Vec<_>>()
            })
            .collect();
            let file_arg = args
                .iter()
                .enumerate()
                .skip(1)
                .find(|(i, a)| !a.starts_with('-') && !flag_value_positions.contains(i))
                .map(|(_, a)| a.clone());
            let editor_entity = file_arg.as_ref().and_then(|p| {
                match document::open_editor(std::path::Path::new(p), cx) {
                    OpenOutcome::Opened(editor) => Some(editor),
                    OpenOutcome::Unsupported { path, message } => {
                        eprintln!("mang-shell: {message}（{}）", path.display());
                        None
                    }
                    OpenOutcome::Failed { path, message } => {
                        eprintln!("mang-shell: 打开失败（{}）：{message}", path.display());
                        None
                    }
                }
            });

            // F2：启动恢复上次工作区（settings.json 记忆；目录已不存在
            // 则静默丢弃——不提示不阻断）。
            let remembered_workspace =
                settings::remembered_workspace().filter(|root| root.is_dir());

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(96.), px(64.)),
                    size: size(px(1280.), px(800.)),
                })),
                titlebar: Some(TitlebarOptions {
                    title: Some("mang".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                // X11 客户端装饰（CSD）需合成器；无合成器时 gpui_linux
                // 自动回退服务端装饰并记日志——两种形态外壳均完整。
                window_decorations: Some(WindowDecorations::Client),
                window_min_size: Some(size(px(760.), px(480.))),
                ..Default::default()
            };
            let editor_for_log = editor_entity.clone();
            // 开窗失败产品化（#68）：GPU/图形栈不可用时给出可读错误链与
            // 面向用户的提示（双语——老机器用户 locale 考量），退出码 70
            // （EX_SOFTWARE）——替代原 .expect 的裸 panic（exit 101 裸
            // 错误链，桌面双击启动时 stderr 无处落 ≈ 静默失败）。
            let window_handle = match cx.open_window(options, |window, cx| {
                cx.new(|cx| shell::ShellApp::new(editor_entity, remembered_workspace, window, cx))
            }) {
                Ok(handle) => handle,
                Err(error) => {
                    eprintln!("mang-shell: 打开主窗口失败 / failed to open main window:");
                    eprintln!("  {error:#}");
                    eprintln!(
                        "提示：图形初始化需要 GPU 支持——Linux 需 Vulkan 驱动或 \
                         OpenGL 4.3+（软件渲染 llvmpipe 亦可）；Windows 需 \
                         Direct3D 11（FL 10.1+）。请更新显卡驱动后重试；\
                         仍失败时运行 `RUST_LOG=gpui_wgpu=info mang-shell` 采集日志。"
                    );
                    std::process::exit(70);
                }
            };

            // #104 任务 2：本机控制通道启动（默认开；失败降级不致命——
            // 端点被占/绑定失败只损失 RPC 能力，桌面本体照常）。服务
            // 句柄挂 GPUI 应用全局（run 闭包返回后事件循环才开跑，
            // 局部绑定会在 setup 尾被 Drop 把 socket 立即解除链接）；
            // App 退出时 Drop 清 socket 文件，异常退出残留由下次启动
            // 探活清理。
            #[cfg(unix)]
            if rpc_disabled {
                eprintln!("rpc: disabled by --no-rpc");
            } else {
                // WindowHandle 实现 Copy——直传免 clone（后续烟雾块仍可用）。
                rpc::ControlServer::start(
                    rpc::resolve_socket_path(rpc_sock.as_deref()),
                    window_handle,
                    cx,
                );
            }
            #[cfg(not(unix))]
            {
                let _ = (&rpc_sock, rpc_disabled);
            }

            // 批 2：鼠标烟雾脚本（Editor 日志开启 + 合成事件序列驱动）。
            if smoke_mouse {
                if let Some(ed) = &editor_for_log {
                    ed.update(cx, |ed, _| ed.set_mouse_log(true));
                }
                if let Some(sec) = smoke_sec {
                    cx.spawn(async move |cx| {
                        run_mouse_script(&window_handle, cx).await;
                        let _ = sec; // 脚本总时长 < smoke_sec（外部控制退出）
                    })
                    .detach();
                }
            }

            // F1/F3：标签烟雾——开窗 2.5s 起依次打开各 FILE（每个追加
            // 一标签；与 Ctrl+O 对话框回调同走 open_document）；可选
            // 3.5s 合成 Ctrl+Tab 循环 + 4.5s 合成 Ctrl+W 关闭（--smoke-
            // tab-keys——完整键派发管线，断言 tabs 证据行）。
            if !smoke_opens.is_empty() {
                let handle = window_handle.clone();
                let keys = smoke_tab_keys;
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(2500))
                        .await;
                    let _ = cx.update(|cx| {
                        handle.update(cx, |shell, window, cx| {
                            for file in &smoke_opens {
                                shell.open_document(std::path::PathBuf::from(file), window, cx);
                            }
                        })
                    });
                    if keys {
                        dispatch_key(&handle, cx, 1000, "tab", true, false, None).await;
                        dispatch_key(&handle, cx, 1000, "w", true, false, None).await;
                    }
                })
                .detach();
            }

            // F2：工作区烟雾——开窗 2.5s 后打开 DIR 为工作区（与
            // Ctrl+K Ctrl+O 对话框回调同走 ShellApp::open_workspace；
            // 断言通道 = stderr "workspace: <路径>" + settings.json 落盘
            // + 下一轮启动的 "workspace:" 恢复行 + quit reached）。
            if let Some(dir) = smoke_workspace {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(2500))
                        .await;
                    let _ = cx.update(|cx| {
                        handle.update(cx, |shell, _window, cx| {
                            shell.open_workspace(std::path::PathBuf::from(dir), cx);
                        })
                    });
                })
                .detach();
            }

            // F7：git diff 烟雾（工作区打开后 2s——shell 层直开：handle
            // 闭包自带 window，跨实体 update_in 在任务上下文解析不到
            // 窗口[F7 实测]，故 shell 内完成全部开标签+落焦）。
            if smoke_git_diff {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(4500))
                        .await;
                    let _ = cx.update(|cx| {
                        handle.update(cx, |shell, window, cx| {
                            let Some(panel) = shell.git_panel() else {
                                return;
                            };
                            let Some((root, path)) = panel.read(cx).first_change() else {
                                return;
                            };
                            let Some(text) = git_panel::git_diff(&root, &path) else {
                                return;
                            };
                            if let Some(focus) =
                                shell.open_readonly_tab(format!("diff: {path}"), text, cx)
                            {
                                window.focus(&focus, cx);
                            }
                        })
                    });
                })
                .detach();
            }

            // F6：主题循环烟雾（完整键派发管线）。
            if smoke_theme_cycle {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    dispatch_key(&handle, cx, 3500, "t", true, false, None).await;
                })
                .detach();
            }

            // F6：字号烟雾（与键位同走 apply_font_size）。
            if let Some(size) = smoke_zoom {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(3500))
                        .await;
                    let _ = cx.update(|cx| {
                        handle.update(cx, |shell, _window, cx| {
                            shell.apply_font_size(gpui::px(size), cx);
                        })
                    });
                })
                .detach();
            }

            // F4：保存流烟雾——弄脏 → Ctrl+W 弹确认 → Enter 保存并关
            //（与用户实操同链：可打印键入 buffer / 键位经完整派发管线）。
            if smoke_save_flow {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    dispatch_key(&handle, cx, 3500, "x", false, false, Some("x")).await;
                    dispatch_key(&handle, cx, 1000, "w", true, false, None).await;
                    dispatch_key(&handle, cx, 1000, "enter", false, false, None).await;
                })
                .detach();
            }

            // F4：另存为烟雾——弄脏 → 另存到 PATH（对话框回调同链）→
            // Ctrl+W 干净直关（断言无 confirm 行）。
            if let Some(target) = smoke_save_as {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    dispatch_key(&handle, cx, 3500, "x", false, false, Some("x")).await;
                    cx.background_executor()
                        .timer(Duration::from_millis(1000))
                        .await;
                    let _ = cx.update(|cx| {
                        handle.update(cx, |shell, _window, cx| {
                            shell.save_active_as(std::path::PathBuf::from(target), cx);
                        })
                    });
                    dispatch_key(&handle, cx, 1000, "w", true, false, None).await;
                })
                .detach();
            }

            // G1：查找替换烟雾（--smoke-find-replace——与启动 FILE 组合）。
            // 序列（全部经完整键派发管线）：
            //   3.5s Ctrl+H 开替换条（断言 "find-replace: bar open"）
            //   4.2s 输入 query "mang" → 4.8s Tab 切替换行 → 输入 "MANG"
            //   5.8s Ctrl+Alt+Enter 全部替换（断言 "replace-all N matches"）
            //   6.6s Esc 关条（焦点回编辑区——Ctrl+Z 需达编辑器分发）
            //   7.2s Ctrl+Z 一次整批撤销（断言 "undo batch N edits"）
            if smoke_find_replace {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    dispatch_key(&handle, cx, 3500, "h", true, false, None).await;
                    // query：mang（virtual 键走 key_char 通道——IME 语义同路径）
                    for ch in "mang".chars() {
                        dispatch_key(
                            &handle,
                            cx,
                            80,
                            "virtual",
                            false,
                            false,
                            Some(ch.to_string().as_str()),
                        )
                        .await;
                    }
                    dispatch_key(&handle, cx, 400, "tab", false, false, None).await;
                    // 替换文本：MANG（shift + key_char 大写直输）
                    for ch in "MANG".chars() {
                        dispatch_key(
                            &handle,
                            cx,
                            80,
                            "virtual",
                            false,
                            true,
                            Some(ch.to_string().as_str()),
                        )
                        .await;
                    }
                    // Ctrl+Alt+Enter 全部替换
                    dispatch_key_mods(
                        &handle,
                        cx,
                        700,
                        "enter",
                        true,
                        true,
                        false,
                        None,
                    )
                    .await;
                    dispatch_key(&handle, cx, 800, "escape", false, false, None).await;
                    dispatch_key(&handle, cx, 600, "z", true, false, None).await;
                    eprintln!("find-replace: script done (ctrl+h → query → tab → repl → ctrl+alt+enter → esc → ctrl+z)");
                })
                .detach();
            }

            // G2：跳行烟雾（--smoke-goto-line——与启动 FILE 组合）。
            if smoke_goto_line {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    // 第一轮：Ctrl+G → 行号 4 → Enter（正常跳转）
                    dispatch_key(&handle, cx, 3500, "g", true, false, None).await;
                    dispatch_key(&handle, cx, 500, "4", false, false, Some("4")).await;
                    dispatch_key(&handle, cx, 500, "enter", false, false, None).await;
                    // 第二轮：Ctrl+G → 9999 → Enter（越界 clamp 到末行）
                    dispatch_key(&handle, cx, 800, "g", true, false, None).await;
                    for ch in "9999".chars() {
                        dispatch_key(
                            &handle,
                            cx,
                            60,
                            "virtual",
                            false,
                            false,
                            Some(ch.to_string().as_str()),
                        )
                        .await;
                    }
                    dispatch_key(&handle, cx, 400, "enter", false, false, None).await;
                    eprintln!(
                        "goto-smoke: script done (ctrl+g → 4 → enter → ctrl+g → 9999 → enter)"
                    );
                })
                .detach();
            }

            // G5：语法高亮烟雾——3.5s 后经真实渲染分派路径探针取证
            //（ShellApp → 活动编辑器 → line_tokens 同源；不合成键——
            // 高亮是渲染态，证据直接来自真实应用态缓存与扫描器）。
            if smoke_syntax_hl {
                // WindowHandle 实现 Copy——直传免 clone（clippy::clone_on_copy；
                // 既有 spawn 块的 .clone() 形态为 main 基线遗留，不在本 PR 清理）。
                let handle = window_handle;
                cx.spawn(async move |cx| {
                    run_syntax_hl_probe(&handle, cx).await;
                })
                .detach();
            }

            // G3：缩进烟雾（--smoke-indent——与启动 FILE 组合）。
            if smoke_indent {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    dispatch_key(&handle, cx, 3500, "home", false, false, None).await;
                    for _ in 0..3 {
                        dispatch_key(&handle, cx, 120, "down", false, true, None).await;
                    }
                    dispatch_key(&handle, cx, 400, "tab", false, false, None).await;
                    dispatch_key(&handle, cx, 600, "tab", false, true, None).await;
                    dispatch_key(&handle, cx, 600, "z", true, false, None).await;
                    dispatch_key(&handle, cx, 500, "z", true, false, None).await;
                    eprintln!("indent-smoke: script done (home → shift+down×3 → tab → shift+tab → ctrl+z ×2)");
                })
                .detach();
            }

            // G4：面板拖拽烟雾（--smoke-panel-resize——合成鼠标事件）。
            if smoke_panel_resize {
                // WindowHandle 实现 Copy——直传免 clone（clippy::clone_on_copy；
                // 既有 spawn 块的 .clone() 形态为 main 基线遗留，不在本 PR 清理）。
                let handle = window_handle;
                cx.spawn(async move |cx| {
                    run_panel_resize_script(&handle, cx).await;
                })
                .detach();
            }

            if let Some(sec) = smoke_sec {
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs_f64(sec))
                        .await;
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });

    println!("smoke: quit reached");
}

// ---------------------------------------------------------------------------
// G5：语法高亮烟雾（--smoke-syntax-hl）
// ---------------------------------------------------------------------------

/// 经真实应用态取证：读活动编辑器的 G5 证据行（围栏语言态 + 代码 token
/// 命中 span——探针内部走 render_lines 同源的 `line_tokens` 分派），
/// 全量 eprintln 后打 script done 证据行（gate summary 断言）。
async fn run_syntax_hl_probe(
    window: &gpui::WindowHandle<shell::ShellApp>,
    cx: &mut gpui::AsyncApp,
) {
    cx.background_executor()
        .timer(Duration::from_millis(3500))
        .await;
    let evidence: Vec<String> = cx.update(|cx| {
        window
            .update(cx, |shell, _window, cx| {
                shell
                    .editor()
                    .map(|ed| ed.read(cx).syntax_hl_evidence())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    });
    for line in &evidence {
        eprintln!("{line}");
    }
    eprintln!(
        "syntax-hl: script done ({} evidence lines via render path)",
        evidence.len()
    );
}

// ---------------------------------------------------------------------------
// F3：标签键位烟雾辅助（合成 KeyDown——与鼠标烟雾同经
// Window::dispatch_event 完整派发管线）。
// ---------------------------------------------------------------------------

/// 延时 `delay_ms` 后向窗口派发一键（`ctrl`/`shift` 修饰）。键事件走
/// 与真实输入同路径的派发链（焦点路由 + 冒泡）——shell 级钩子/
/// 编辑器分派均真实触达。
async fn dispatch_key(
    window: &gpui::WindowHandle<shell::ShellApp>,
    cx: &mut gpui::AsyncApp,
    delay_ms: u64,
    key: &str,
    ctrl: bool,
    shift: bool,
    key_char: Option<&str>,
) {
    use gpui::{KeyDownEvent, Keystroke, Modifiers, PlatformInput};
    cx.background_executor()
        .timer(Duration::from_millis(delay_ms))
        .await;
    let event = PlatformInput::KeyDown(KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: ctrl,
                shift,
                ..Default::default()
            },
            key: key.to_string(),
            key_char: key_char.map(|c| c.to_string()),
        },
        is_held: false,
        prefer_character_input: false,
    });
    // update_window（闭包拿 AnyView 而非根视图实体——官方窗口事件派发
    // 口径，window.rs 测试同款）：不包根视图 update，shell 级监听器
    //（cx.listener）不重入。此前用 window.update 会包住 ShellApp
    // 实体更新——键事件再触发其监听器即「already being updated」panic。
    let any_handle: gpui::AnyWindowHandle = window.clone().into();
    let result = cx.update_window(any_handle, |_, window, cx| {
        window.dispatch_event(event, cx);
    });
    if let Err(err) = &result {
        eprintln!("dispatch_key({key}): update_window 失败: {err}");
    }
}

/// G1：[`dispatch_key`] 的全修饰键变体（Ctrl+Alt+Enter 三修饰组合——
/// 查找替换烟雾的"全部替换"合成事件；派发口径与 [`dispatch_key`] 一致：
/// update_window 闭包 + `Window::dispatch_event` 完整管线）。
#[allow(clippy::too_many_arguments)]
async fn dispatch_key_mods(
    window: &gpui::WindowHandle<shell::ShellApp>,
    cx: &mut gpui::AsyncApp,
    delay_ms: u64,
    key: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    key_char: Option<&str>,
) {
    use gpui::{KeyDownEvent, Keystroke, Modifiers, PlatformInput};
    cx.background_executor()
        .timer(Duration::from_millis(delay_ms))
        .await;
    let event = PlatformInput::KeyDown(KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: ctrl,
                alt,
                shift,
                ..Default::default()
            },
            key: key.to_string(),
            key_char: key_char.map(|c| c.to_string()),
        },
        is_held: false,
        prefer_character_input: false,
    });
    let any_handle: gpui::AnyWindowHandle = window.clone().into();
    let result = cx.update_window(any_handle, |_, window, cx| {
        window.dispatch_event(event, cx);
    });
    if let Err(err) = &result {
        eprintln!("dispatch_key_mods({key}): update_window 失败: {err}");
    }
}

// ---------------------------------------------------------------------------
// G4：面板拖拽烟雾脚本（--smoke-panel-resize）
// ---------------------------------------------------------------------------

/// 合成鼠标事件驱动面板 resize 手柄（与真实事件同派发路径——hitbox
/// 路由 + drag 会话）。
///
/// 坐标（1280×800 烟雾窗；左栏手柄 x∈[312,316]（图标条 52 + 左栏 260），
/// 右栏手柄 x∈[996,1000]（右栏 280 + 手柄 4——编辑区 flex 自适应））：
///
/// 1. 左手柄 (314, 400) 按下 → 拖到 (354, 400)（+40 → 左栏 260→300）
/// 2. 右手柄 (998, 300) 按下 → 拖到 (938, 300)（-60 → 右栏 280→340）
///
/// 每侧首步 move 激活 drag 会话（on_drag 构造），后续步进 drag_move
/// ——与鼠标烟雾同坑位口径。
async fn run_panel_resize_script(
    window: &gpui::WindowHandle<shell::ShellApp>,
    cx: &mut gpui::AsyncApp,
) {
    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, point,
        px,
    };

    let down = |x: f32, y: f32| {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(x), px(y)),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        })
    };
    let move_to = |x: f32, y: f32| {
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        })
    };
    let up_at = |x: f32, y: f32| {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: point(px(x), px(y)),
            modifiers: Modifiers::default(),
            click_count: 1,
        })
    };

    async fn step(
        window: &gpui::WindowHandle<shell::ShellApp>,
        cx: &mut gpui::AsyncApp,
        delay_ms: u64,
        ev: PlatformInput,
    ) {
        cx.background_executor()
            .timer(Duration::from_millis(delay_ms))
            .await;
        // #89 坑3：不能经 window.update 派发（包住 ShellApp 实体更新
        // ——鼠标事件再触发 shell 监听器即重入 panic，面板手柄实测）。
        // update_window 拿 AnyView 闭包——与 dispatch_key 同口径。
        let any_handle: gpui::AnyWindowHandle = (*window).into();
        let _ = cx.update_window(any_handle, |_, window, cx| {
            window.dispatch_event(ev, cx);
        });
    }

    // ① 左栏手柄：down → 激活 → 步进 → up（宽 260 → 300）
    step(window, cx, 3500, down(314., 400.)).await;
    step(window, cx, 120, move_to(330., 400.)).await; // 首步：激活 drag 会话
    step(window, cx, 120, move_to(354., 400.)).await; // 步进：+40
    step(window, cx, 300, up_at(354., 400.)).await; // 收尾：persist + 证据行
    // ② 右栏手柄：down → 激活 → 步进 → up（宽 280 → 340）
    step(window, cx, 900, down(998., 300.)).await;
    step(window, cx, 120, move_to(968., 300.)).await;
    step(window, cx, 120, move_to(938., 300.)).await; // 步进：-60
    step(window, cx, 300, up_at(938., 300.)).await;
    eprintln!("panel-resize-smoke: script done (left +40 / right +60)");
}

// ---------------------------------------------------------------------------
// 批 2：鼠标烟雾脚本（--smoke-mouse）
// ---------------------------------------------------------------------------

/// 合成鼠标事件序列驱动完整派发管线（`Window::dispatch_event`——hitbox
/// 路由/拖拽状态机与真实事件同路径，仅平台翻译层除外）。
///
/// 坐标为窗口坐标（1280×800 烟雾窗；编辑区 x∈[312,1000]、
/// y∈[66,776]（F3 标签栏 30px 后——标题栏 36 + 标签栏 30 起），
/// 正文原点 x≈372。命中结果由 Editor `mouse_log` 输出到 stdout——
/// 脚本不假设精确行号（截图 + 日志双通道断言）。
///
/// 序列（总约 5s，配 --smoke-sec ≥ 7）：
///
/// 1. 单击 (500,100)——定位光标（状态栏行/列变化）
/// 2. 拖选 (500,100)→(500,240)——约 6 行选区
/// 3. 双击 (500,100)——选词
/// 4. 三击 (500,100)——选行
/// 5. 滚轮 (600,400)——虚拟化列表滚动
async fn run_mouse_script(window: &gpui::WindowHandle<shell::ShellApp>, cx: &mut gpui::AsyncApp) {
    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
        ScrollDelta, ScrollEvent, point, px,
    };

    let click = |x: f32, y: f32, count: usize| {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(x), px(y)),
            modifiers: Modifiers::default(),
            click_count: count,
            first_mouse: false,
        })
    };
    let move_to = |x: f32, y: f32| {
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        })
    };
    let up_at = |x: f32, y: f32| {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: point(px(x), px(y)),
            modifiers: Modifiers::default(),
            click_count: 1,
        })
    };
    let scroll = |x: f32, y: f32| {
        PlatformInput::Scroll(ScrollEvent {
            position: point(px(x), px(y)),
            // X11 约定：向下滚 = 负 y（gpui_linux client.rs ScrollDirection::Down
            // → -SCROLL_LINES）；正 y 在文档顶部无可见变化。
            delta: ScrollDelta::Lines(point(0., -3.)),
            ..Default::default()
        })
    };

    // 步进：延时（等帧渲染）→ 派发事件（完整管线）。
    async fn step(
        window: &gpui::WindowHandle<shell::ShellApp>,
        cx: &mut gpui::AsyncApp,
        delay_ms: u64,
        ev: PlatformInput,
    ) {
        cx.background_executor()
            .timer(Duration::from_millis(delay_ms))
            .await;
        let _ = cx.update(|cx| window.update(cx, |_, window, cx| window.dispatch_event(ev, cx)));
    }

    // 1) 单击定位（首步 2.5s 吸收启动滞后：进程启动→首帧 ~1.2s；
    //    F3 后 y+30——标签栏 30px 下移编辑区，保持同一行目标）。
    step(window, cx, 2500, click(500., 130., 1)).await;
    step(window, cx, 300, up_at(500., 130.)).await;
    // 2) 拖选（约 6 行；阶段间隔 2s——截图稳定窗）
    step(window, cx, 2000, click(500., 130., 1)).await;
    step(window, cx, 300, move_to(500., 200.)).await;
    step(window, cx, 300, move_to(500., 270.)).await;
    step(window, cx, 400, up_at(500., 270.)).await;
    // 3) 双击选词
    step(window, cx, 2000, click(500., 130., 2)).await;
    step(window, cx, 300, up_at(500., 130.)).await;
    // 4) 三击选行
    step(window, cx, 2000, click(500., 130., 3)).await;
    step(window, cx, 300, up_at(500., 130.)).await;
    // 5) 滚轮滚动（向下）
    step(window, cx, 2100, scroll(600., 400.)).await;
    // 6) 出界拖拽：拖到窗口底缘之下（y=830 > 窗高 800）——渐进式 clamp
    //    到末可见行 + 40ms 自动滚动逐步扩展选区。注：首个 move 事件被
    //    drag 激活消费（on_drag 构造），on_drag_move 从第二个起回调——
    //    故两步 move（先界内激活、再出界触发）。
    step(window, cx, 1500, click(500., 700., 1)).await;
    step(window, cx, 300, move_to(500., 770.)).await;
    step(window, cx, 200, move_to(500., 830.)).await;
    step(window, cx, 900, up_at(500., 830.)).await;
}
