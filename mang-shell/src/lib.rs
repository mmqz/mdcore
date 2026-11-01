//! mang-shell 库目标（#115）：把外壳源码树暴露给集成测试。
//!
//! 外壳本身是 bin（main.rs——GPUI 入口）；集成测试（tests/）要复用
//! 「docx 行模型 → 编辑桥 → 引擎 → 字节」这条链时，需要一个库目标把模块
//! 树搬出来。做法：库根按路径包含主模块（editor 及其子模块 + settings），
//! 与 bin 共用同一份源码——不复制模块定义，避免两处漂移。
#![allow(dead_code, unused_imports)]
#![allow(clippy::all)]

/// #133 任务 1：canvas 画布视图（会话装载 + 视口 + 光栅位面——纯逻辑
/// 面单测复用；GPUI 绘制接线在 editor::render_canvas_area）。
#[path = "canvas_view.rs"]
pub mod canvas_view;

/// 文档事件面（#111 订阅模式——buffer 的 revision/pending 依赖它）。
#[path = "doc_events.rs"]
pub mod doc_events;

#[path = "editor/mod.rs"]
pub mod editor;

/// 外壳设置面（编辑器构造读缩进单位/面板宽度——src/settings.rs 同源）。
#[path = "settings.rs"]
pub mod settings;
/// 右键菜单数据/几何面（#63 打磨项）：纯数据，零 GPUI 依赖——菜单摆放
/// 算法（边缘翻转/夹紧）必须能脱离窗口单测，见模块文档。
#[path = "context_menu.rs"]
pub mod context_menu;

/// 状态栏信息面（git 分支/编码/换行符/选中字数）：纯函数，零 GPUI
/// 依赖——全部对着字节与目录写断言。
#[path = "status_info.rs"]
pub mod status_info;

/// 命令面板分组/最近使用（#63 打磨项）：纯数据，零 GPUI 依赖。
#[path = "command_groups.rs"]
pub mod command_groups;
