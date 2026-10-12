//! mdshell — R10 宿主契约层（headless 可验证实现）。
//!
//! 职责边界（主档 §9：宿主 = DocumentStore/Clipboard/Settings 最小面）：
//! - 文档生命周期：open（全量 / 渐进 K 段）、save、document_id 匹配重开；
//! - 打开进度事件（宿主 UI 据此画进度条）；
//! - 设置面（R24-a）：`settings` re-export——T3 复用地基轮抽取至独立
//!   crate mdc-settings（域无关，对 mdcore 零依赖）；本 crate 经
//!   `pub use mdc_settings as settings` 保持 `mdshell::settings::*`
//!   路径零破坏；
//! - **不含** WebView / 窗口管理——原生后端在 crates/shell-native
//!   （tao + wry + GTK3，ADR-H1 定谳：Slint 因 GPL 证与 ADR-C1 排除；需 webkit2gtk-4.1 系统库，沙箱不可编译，不入 workspace）。
//!
//! 契约（`Host` trait）是原生胶水与 headless 测试的公共面：原生壳实现
//! `Host::surface()` 把 dist/ 装进 wry；headless 测试实现验证同一套
//! 文档状态机。

pub mod session;
pub use mdc_settings as settings;

use mdcore::document::DocumentId;
use mdcore::Core;

/// 打开进度事件（渐进打开时宿主 UI 的输入源）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenProgress {
    /// 首段已入核并可交互（TTI 时刻）。
    FirstChunkInteractive { blocks: usize },
    /// 第 `chunk_index`/`total_chunks` 段已追加。
    ChunkAppended {
        chunk_index: usize,
        total_chunks: usize,
    },
    /// 全文档就绪（终态 = 一次性打开的等价树，Rust 侧已有三重相等测试背书）。
    Completed { blocks: usize },
}

/// 宿主面抽象：原生壳（tao+wry/GTK3）与 headless 测试实现同一契约。
pub trait Host {
    /// 文本注入面（原生 = WebView 导航到 dist/ 后 postMessage；
    /// headless = 直接进核）。返回注入是否成功。
    fn surface_load_markdown(&mut self, markdown: &str) -> Result<(), String>;
}

/// 正在打开的文档会话：核 + 身份 + 进度。
pub struct DocumentSession {
    pub core: Core,
    pub id: Option<DocumentId>,
    /// 宿主域路径提示（R24-b 会话持久化用；核不碰 fs，纯透传字符串，
    /// 语义由宿主定义——显示名、完整路径或 URI 均可）。
    pub path: Option<String>,
}

impl DocumentSession {
    /// 新建会话（路径未设 = 匿名/未另存文档，不入恢复集）。
    pub fn new(core: Core) -> Self {
        Self {
            id: core.document_id(),
            core,
            path: None,
        }
    }

    /// 附路径提示（链式；宿主开档后设置）。
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

/// 全量打开（小文档路径）：markdown + 可选 meta JSON（save_document 产物）。
pub fn open_document(markdown: &str, meta_json: Option<&str>) -> Result<DocumentSession, String> {
    let core = match meta_json {
        Some(meta) => Core::open_document(markdown, meta).map_err(|e| format!("{e:?}"))?,
        None => Core::from_markdown(markdown),
    };
    Ok(DocumentSession {
        id: core.document_id(),
        core,
        path: None,
    })
}

/// 渐进打开（大文档路径）：分块追加进核，每步回调进度。
///
/// 分块责任在调用方（宿主读文件流时按块边界切；web 侧 `splitChunks`
/// 是同一语义的 JS 参考实现）。`is_complete_chunk` 是核侧最终防线：
/// 悬挂 fence 会在 `extend_from_markdown` 处 Reject 且不污染树。
pub fn open_document_progressive<H: Host>(
    chunks: &[String],
    meta_json: Option<&str>,
    host: &mut H,
    mut on_progress: impl FnMut(OpenProgress),
) -> Result<DocumentSession, String> {
    let mut iter = chunks.iter().enumerate();
    let (_, first) = iter.next().ok_or("empty chunk list")?;
    let mut core = Core::from_markdown(first);
    if let Some(meta) = meta_json {
        // meta 携带 document_id/历史：先建核再注入（保持与 open_document 同一恢复路径）
        core = Core::open_document(first, meta).map_err(|e| format!("{e:?}"))?;
    }
    on_progress(OpenProgress::FirstChunkInteractive {
        blocks: core.tree.top_level().len(),
    });
    host.surface_load_markdown(&mdcore::markdown::tree_to_markdown(&core.tree))?;

    let total = chunks.len();
    for (i, chunk) in iter {
        core.extend_from_markdown(chunk)
            .map_err(|e| format!("chunk {i} rejected: {e:?}"))?;
        on_progress(OpenProgress::ChunkAppended {
            chunk_index: i,
            total_chunks: total,
        });
    }
    on_progress(OpenProgress::Completed {
        blocks: core.tree.top_level().len(),
    });
    Ok(DocumentSession {
        id: core.document_id(),
        core,
        path: None,
    })
}

/// 保存：markdown 全文 + meta JSON（document_id + 整栈历史）。
pub fn save_document(session: &DocumentSession) -> (String, String) {
    session.core.save_document()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// headless 宿主：记录注入面调用（原生 wry 后端的可验证替身）。
    struct RecordingHost {
        loads: RefCell<Vec<usize>>,
    }
    impl RecordingHost {
        fn new() -> Self {
            Self {
                loads: RefCell::new(vec![]),
            }
        }
    }
    impl Host for RecordingHost {
        fn surface_load_markdown(&mut self, markdown: &str) -> Result<(), String> {
            self.loads.borrow_mut().push(markdown.len());
            Ok(())
        }
    }

    fn section(i: usize) -> String {
        format!(
            "# Section {i}\n\nParagraph {i} filler body line.\n\n- item alpha {i}\n- item beta {i}\n\n"
        )
    }

    #[test]
    fn full_open_save_roundtrip_keeps_identity() {
        let mut core = mdcore::Core::from_markdown("alpha\nbeta\n");
        core.set_document_id(Some(DocumentId::generate(1_700_000_000_000)))
            .unwrap();
        let (md, meta) = core.save_document();

        let session = open_document(&md, Some(&meta)).expect("open");
        assert_eq!(session.id, core.document_id(), "重开匹配同一身份");
        assert_eq!(mdcore::markdown::tree_to_markdown(&session.core.tree), md);
    }

    #[test]
    fn progressive_open_matches_one_shot_and_reports_progress() {
        let chunks: Vec<String> = (0..8).map(section).collect();
        let full: String = chunks.concat();
        let one_shot = mdcore::Core::from_markdown(&full);

        let mut host = RecordingHost::new();
        let events = RefCell::new(vec![]);
        let session =
            open_document_progressive(&chunks, None, &mut host, |p| events.borrow_mut().push(p))
                .expect("progressive open");

        // 终态与一次性打开等价（Rust 侧三重相等已证；此处断言指纹面）
        assert_eq!(
            mdcore::markdown::tree_to_markdown(&session.core.tree),
            mdcore::markdown::tree_to_markdown(&one_shot.tree)
        );
        // 进度事件序列完整：1 首段 + 7 追加 + 1 完成
        let ev = events.borrow();
        assert_eq!(ev.len(), 9);
        assert!(matches!(ev[0], OpenProgress::FirstChunkInteractive { .. }));
        assert!(matches!(ev[8], OpenProgress::Completed { .. }));
        // 注入面：首段一次（TTI 时刻），终态由调用方再注入或由 WebView 端
        // extend 通道补齐——headless 替身只断言首段语义。
        assert_eq!(host.loads.borrow().len(), 1);
    }

    #[test]
    fn progressive_open_rejects_dangling_fence_without_partial_state() {
        let mut host = RecordingHost::new();
        let chunks = vec![section(0), "```rust\nfn dangling() {\n".to_string()];
        let r = open_document_progressive(&chunks, None, &mut host, |_| {});
        assert!(r.is_err(), "悬挂 fence 必须拒绝");
        assert!(
            r.err().unwrap().contains("rejected"),
            "错误面可定位 chunk 序号"
        );
    }

    #[test]
    fn save_document_roundtrip_via_session() {
        let session = open_document("one\ntwo\n", None).expect("open");
        let (md, meta) = save_document(&session);
        let reopened = open_document(&md, Some(&meta)).expect("reopen");
        assert_eq!(reopened.id, session.id, "session 身份往返");
    }
}
