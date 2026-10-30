//! mang 桌面外壳入口 —— GPUI 主线 UI（#63 骨架 / #68 阶段 1 编辑区接线）。
//!
//! 用法：
//! ```text
//! mang-shell [FILE]            正常启动（FILE = .md/.markdown/.txt 打开为
//!                               可编辑文档；.docx/.pdf 只读面待批 2）
//! mang-shell --smoke-sec N     烟雾模式：开窗 N 秒后自动退出（exit 0 +
//!                               stdout 末行 "smoke: quit reached"——Xvfb
//!                               无头验证通道）
//! mang-shell --smoke-mouse     批 2 鼠标烟雾：与 --smoke-sec 组合——合成
//!                               PlatformInput 序列（单击/拖选/双击/三击/
//!                               滚轮）过 Window::dispatch_event 完整
//!                               派发管线，命中结果由 Editor 日志输出
//!                               （需同时带 FILE——空态无编辑区）
//! mang-shell --version         版本号
//! ```
//!
//! 引导链：`gpui_platform::application()`（OS 后端选择器）→ 内嵌资产源
//! （图标）→ `theme::init`（内置基础主题）→ `set_theme_settings_provider`
//! （ui 组件字体面契约）→ 文档分派（`document::open`）→ `Editor` 实体 →
//! 开窗（1280×800，X11 客户端装饰请求，无合成器自动回退服务端装饰）。

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
mod panels;
mod settings;
mod shell;
mod status_bar;
mod title_bar;

use document::OpenedDocument;
use editor::Editor;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mang-shell {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let smoke_sec: Option<f64> = args
        .iter()
        .position(|a| a == "--smoke-sec")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    let smoke_pos = args.iter().position(|a| a == "--smoke-sec");
    // 批 2：鼠标烟雾脚本（需与 --smoke-sec + FILE 组合）。
    let smoke_mouse = args.iter().any(|a| a == "--smoke-mouse");

    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            theme::init(theme::LoadThemes::JustBase, cx);
            theme::set_theme_settings_provider(Box::new(settings::ShellThemeSettings), cx);

            // 文档分派：首个非 flag 参数为文件路径（#68 阶段 1.1；
            // 跳过 `--smoke-sec` 的值参数，避免数字误判为文件）。
            let file_arg = args
                .iter()
                .enumerate()
                .skip(1)
                .find(|(i, a)| !a.starts_with('-') && !matches!(smoke_pos, Some(p) if *i == p + 1))
                .map(|(_, a)| a.clone());
            let opened = file_arg
                .as_ref()
                .map(|p| document::open(std::path::Path::new(p)));
            let editor_entity = match opened {
                Some(OpenedDocument::EditableText { path, content }) => {
                    Some(cx.new(|cx| Editor::new(Some(path), content, cx)))
                }
                Some(OpenedDocument::ReadOnlyDocx { path, payload }) => {
                    // 批 3 证据通道：启动日志一行（属性面 + 只读标记）。
                    eprintln!(
                        "docx: {} 段 / {} 表（{} 行）/ {} 标题 / {} 图片 / {} 警告（只读，{}）",
                        payload.props.paragraphs,
                        payload.props.tables,
                        payload.props.table_rows,
                        payload.headings.len(),
                        payload.props.images,
                        payload.props.warnings,
                        path.display()
                    );
                    Some(cx.new(|cx| Editor::new_docx(path, payload, cx)))
                }
                Some(OpenedDocument::ReadOnlyPending { path, kind }) => {
                    eprintln!(
                        "mang-shell: {kind} 只读渲染面随批 4 接线（{}）",
                        path.display()
                    );
                    None
                }
                Some(OpenedDocument::Unsupported { path }) => {
                    eprintln!(
                        "mang-shell: 暂不支持该文件类型（{}）——支持 .md/.markdown/.txt",
                        path.display()
                    );
                    None
                }
                Some(OpenedDocument::OpenError { path, message }) => {
                    eprintln!("mang-shell: 打开失败（{}）：{message}", path.display());
                    None
                }
                None => None,
            };

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
            let window_handle = cx
                .open_window(options, |window, cx| {
                    cx.new(|cx| shell::ShellApp::new(editor_entity, window, cx))
                })
                .expect("mang-shell: 打开主窗口失败");

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
