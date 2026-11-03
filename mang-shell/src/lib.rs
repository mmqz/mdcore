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

/// 命令面板的分组渲染（#183 A-3 接线）：数据装配（search → 分组 →
/// 扁平序）+ 段标题/条目渲染组件。依赖面全在 lib 树内
/// （command_groups + editor::commands + gpui/ui 渲染原语）——
/// 与 canvas_view 同型进 lib，供 editor/mod.rs（lib+bin 共用源码）
/// 的面板浮层消费。
#[path = "palette_view.rs"]
pub mod palette_view;

/// #185：系统原生错误对话框（GPU/图形栈初始化失败——GPUI 起不来时
/// 的「用户看得见」兜底）。纯 std（Command + 候选链构造），进 lib 树
/// 供单测断言候选序/转义/受控 PATH 送达矩阵。
#[path = "native_dialog.rs"]
pub mod native_dialog;

/// #68 批 5 / #164：性能测量桩（`MANG_PERF=1` 门控——未开时零开销：
/// 桩点一次分支预测，无计时无分配）。纯 std（OnceLock + Instant），
/// 进 lib 树供单测钉门控/输出契约；接线面（main 启动分段 / dispatch_key
/// 逐键 / find_matches 全文查找）由 #164 后续提交按两行式接入。
#[path = "perf.rs"]
pub mod perf;
