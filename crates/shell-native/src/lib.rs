//! mdshell-native — R10 原生宿主壳（wry + Slint）胶水。
//!
//! ⚠️ 状态：**设计定稿、编译待原生环境**（沙箱无 sudo，webkit2gtk-4.1-dev
//! 不可安装；本 crate 未列入 workspace members，`cargo test` 不会触碰）。
//! 本文件是结构蓝图：在有系统依赖的机器上按 Cargo.toml 注释启用依赖后
//! 即为可编译起点。
//!
//! 结构（与 mdshell 契约层、web/dist 的对接关系）：
//!
//! ```text
//! ┌─ tao 事件循环（窗口/文件对话框）─────────────────────┐
//! │  Slint 面板：标题栏 / 打开按钮 / 进度条（OpenProgress 驱动）│
//! │  wry WebView：加载 dist/index.html（R9/R10 bundle 自包含）   │
//! │    │ host.surface_load_markdown(md)  ← mdshell::Host 实现    │
//! │    │   = webview.evaluate_script("window.__inject(md)")      │
//! │  文件层：open_document / open_document_progressive / save    │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! 对接点清单（实现时逐项落）：
//! 1. 窗口创建：tao WindowBuilder → wry WebViewBuilder::new().with_url(file://dist/index.html)
//! 2. 文本注入：main.ts 增加 `window.__inject(md)` 入口（读取参数或 postMessage
//!    触发 `new WasmCore(md)` 重建），替代 `?doc=` URL 参数（大文档不宜走 URL）；
//! 3. 渐进打开进度：mdshell::open_document_progressive 的 on_progress 回调
//!    → Slint 进度条 + 完成后 surface_load_markdown 注入终态；
//! 4. 保存：Ctrl+S 拦截 → window.__md() 取全文 → mdshell::save_document
//!    → tao 文件对话框写盘（markdown + meta JSON 两文件或伴随文件）；
//! 5. document_id 重开匹配：meta JSON 存档后 open_document 走同一身份。

#![allow(dead_code)]

/// 注入脚本模板（对接点 2）。`__md_payload` 由宿主 JSON 转义后替换。
pub const INJECT_SNIPPET: &str = r#"
window.__inject = (md) => {
  // 宿主文件打开路径：整体重建核（渐进打开走 __inject_chunk，见下）
  window.dispatchEvent(new CustomEvent('mdc-inject', { detail: md }));
};
window.__inject_chunk = (chunk, first) => {
  // 渐进打开路径：first=true 重建核，否则 extend_from_markdown 追加
  window.dispatchEvent(new CustomEvent('mdc-inject-chunk', { detail: { chunk, first } }));
};
"#;

/// 渐进打开宿主侧伪代码（实现蓝图，非可运行体——依赖未启用）。
///
/// ```ignore
/// let mut host = WryHost::new(&webview);          // impl mdshell::Host
/// let session = mdshell::open_document_progressive(
///     &chunks, None, &mut host,
///     |p| slint_progress.update(p),               // 进度条
/// )?;
/// ```
pub const BLUEPRINT_NOTE: &str = "see module docs; enable deps in Cargo.toml on a native host";

#[cfg(test)]
mod tests {
    /// 零依赖冒烟：保证本 crate 在任何环境都可编译（蓝图常量非空）。
    #[test]
    fn blueprint_constants_present() {
        assert!(super::INJECT_SNIPPET.contains("__inject_chunk"));
        assert!(!super::BLUEPRINT_NOTE.is_empty());
    }
}
