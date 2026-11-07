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

/// 文档打开分派 + 编辑器构造（open/open_editor——#427 折叠持久化
/// 读侧接线在 open_editor 的 EditableText 分支）。进 lib 树的理由：
/// CI 的 mang-shell job 只跑 `--lib`/`--tests`（bin 单测不执行），
/// 该模块的单测挂在 bin 目标 = 永不运行（假绿）；依赖面
///（canvas_view/editor/perf/settings）本就全在 lib 树。
#[path = "document.rs"]
pub mod document;

#[path = "editor/mod.rs"]
pub mod editor;

/// 右键菜单数据/几何面（#63 打磨项）：纯数据，零 GPUI 依赖——菜单摆放
/// 算法（边缘翻转/夹紧）必须能脱离窗口单测，见模块文档。
#[path = "context_menu.rs"]
pub mod context_menu;
/// 外壳设置面（编辑器构造读缩进单位/面板宽度——src/settings.rs 同源）。
#[path = "settings.rs"]
pub mod settings;

/// #209：顶部菜单栏模型层（菜单结构/Cmd 映射/键盘导航状态机/下拉几何
/// 估算）——纯数据零 GPUI 依赖，与 context_menu 同分层进 lib 树（渲染层
/// menu_bar_view.rs 是 bin 专属）。菜单项只映射既有动作（Cmd 枚举 /
/// Editor 菜单方法 / ShellApp 既有方法），无既有命令的项置灰不绑动作。
#[path = "menu_bar.rs"]
pub mod menu_bar;

/// #217：键位表层（默认表 = editor dispatch_key / shell on_key_down
/// 两处匹配臂的精确镜像；匹配引擎 + settings.json 用户覆盖合并）——
/// 纯逻辑零 GPUI，菜单标签与键位派发的单一事实源。
#[path = "keymap.rs"]
pub mod keymap;

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

/// #302 阶段 2·④：PDF 标注写会话（右键拖框高亮 → #304 坐标变换 →
/// PdfSession 命令面 → 增量 flush 落盘）。零 GPUI 依赖（纯 Rust +
/// mdc-pdf engine-lopdf 面）——写路径全量可无 GUI 测试。
#[path = "pdf_annotate.rs"]
pub mod pdf_annotate;

/// #307 ②功能独立项：PDF→DOCX 文本版导出桥组合根（mdc-pdf 提取 ×
/// mdc-office-io 写面的唯一装配处——三域纪律组合点）。
pub mod pdf_export_docx;

/// #289-④：PDF 页面位图视图（pdfium 光栅化 → uniform_list 竖排；
/// 纯函数面（宽量子化/槽高/位图转换）进 lib 树供单测——CI 无
/// libpdfium 同样真跑）。
#[path = "pdf_pages.rs"]
pub mod pdf_pages;

/// #389 阶段 1：XLSX 网格视图（真网格渲染骨架——列头/行号槽固定 +
/// 双向虚拟化 + 单元格点选；数据面/几何纯函数零 GPUI 依赖可测，
/// GPUI 层编译面验证 + 真机登记——pdf_pages 同纪律）。
#[path = "xlsx_grid_view.rs"]
pub mod xlsx_grid_view;
