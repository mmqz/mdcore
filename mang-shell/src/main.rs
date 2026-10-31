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
mod panels;
mod settings;
mod shell;
mod status_bar;
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
    // F1：替换管线烟雾钩子（--smoke-open FILE2）。
    let smoke_open = args
        .iter()
        .position(|a| a == "--smoke-open")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // F2：工作区烟雾钩子（--smoke-workspace DIR）。
    let smoke_workspace = args
        .iter()
        .position(|a| a == "--smoke-workspace")
        .and_then(|i| args.get(i + 1))
        .cloned();
    // 批 2：鼠标烟雾脚本（需与 --smoke-sec + FILE 组合）。
    let smoke_mouse = args.iter().any(|a| a == "--smoke-mouse");

    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            theme::init(theme::LoadThemes::JustBase, cx);
            theme::set_theme_settings_provider(Box::new(settings::ShellThemeSettings), cx);

            // 文档分派：首个非 flag 参数为文件路径（#68 阶段 1.1；跳过
            // 各 flag 的值参数，避免误判为文件）。F1 起与 Ctrl+O 运行时
            // 打开共用 document::open_editor 通道（同一分派语义）。
            let flag_value_positions: Vec<usize> =
                ["--smoke-sec", "--smoke-open", "--smoke-workspace"]
                    .iter()
                    .filter_map(|flag| args.iter().position(|a| a == flag).map(|pos| pos + 1))
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

            // F1：替换管线烟雾——开窗 2.5s 后把 FILE2 换入编辑区
            // （与 Ctrl+O 对话框回调同走 ShellApp::open_document；
            // 断言通道 = stderr "open: <路径>" + docx/pdf 证据行 +
            // "smoke: quit reached"）。
            if let Some(extra) = smoke_open {
                let handle = window_handle.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(2500))
                        .await;
                    let _ = cx.update(|cx| {
                        handle.update(cx, |shell, window, cx| {
                            shell.open_document(std::path::PathBuf::from(extra), window, cx);
                        })
                    });
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
// 批 2：鼠标烟雾脚本（--smoke-mouse）
// ---------------------------------------------------------------------------

/// 合成鼠标事件序列驱动完整派发管线（`Window::dispatch_event`——hitbox
/// 路由/拖拽状态机与真实事件同路径，仅平台翻译层除外）。
///
/// 坐标为窗口坐标（1280×800 烟雾窗；编辑区 x∈[312,1000]、y∈[36,776]，
/// 正文原点 x≈372）。命中结果由 Editor `mouse_log` 输出到 stdout——
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

    // 1) 单击定位（首步 2.5s 吸收启动滞后：进程启动→首帧 ~1.2s）
    step(window, cx, 2500, click(500., 100., 1)).await;
    step(window, cx, 300, up_at(500., 100.)).await;
    // 2) 拖选（约 6 行；阶段间隔 2s——截图稳定窗）
    step(window, cx, 2000, click(500., 100., 1)).await;
    step(window, cx, 300, move_to(500., 170.)).await;
    step(window, cx, 300, move_to(500., 240.)).await;
    step(window, cx, 400, up_at(500., 240.)).await;
    // 3) 双击选词
    step(window, cx, 2000, click(500., 100., 2)).await;
    step(window, cx, 300, up_at(500., 100.)).await;
    // 4) 三击选行
    step(window, cx, 2000, click(500., 100., 3)).await;
    step(window, cx, 300, up_at(500., 100.)).await;
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
