//! mang-shell 库目标（#115）：把外壳源码树暴露给集成测试。
//!
//! 外壳本身是 bin（main.rs——GPUI 入口）；集成测试（tests/）要复用
//! 「docx 行模型 → 编辑桥 → 引擎 → 字节」这条链时，需要一个库目标把模块
//! 树搬出来。做法：库根按路径包含主模块（editor 及其子模块 + settings），
//! 与 bin 共用同一份源码——不复制模块定义，避免两处漂移。
#![allow(dead_code, unused_imports)]
#![allow(clippy::all)]

/// 文档事件面（#111 订阅模式——buffer 的 revision/pending 依赖它）。
#[path = "doc_events.rs"]
pub mod doc_events;

#[path = "editor/mod.rs"]
pub mod editor;

/// 外壳设置面（编辑器构造读缩进单位/面板宽度——src/settings.rs 同源）。
#[path = "settings.rs"]
pub mod settings;
