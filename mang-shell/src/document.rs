//! 打开文档分派（#68 阶段 1）。
//!
//! 按 扩展名把磁盘文件分派到编辑器形态：
//!
//! - `.md` / `.markdown` / `.txt` → **可编辑**（源文本编辑——批 1）
//! - `.docx` → **只读渲染**（批 3：mdc-office-io 块序 → 行模型，
//!   标题树 + 样式 span——见 [`crate::editor::docx_doc`]）
//! - `.pdf` → **只读渲染**（批 4：mdc-pdf 文本层 → 行模型，书签树 +
//!   页分隔 + 页码标签——见 [`crate::editor::pdf_doc`]；100MB 前置门）
//! - `.xlsx` → **只读渲染**（G6：mdc-office-io 读面 → 行模型，工作表
//!   树 + 列字母/行号网格——见 [`crate::editor::xlsx_doc`]）
//! - `.canvas` → **画布视图**（#133 任务 1：yrs 快照 → CanvasSession
//!   → 全彩位面 + 视口——见 [`crate::canvas_view`]）
//! - 其余 → `Unsupported`（空态提示）
//!
//! 大小写不敏感匹配扩展名；读取失败返回 `OpenError`（空态呈现原因）。

use std::path::{Path, PathBuf};

use gpui::{App, AppContext, Entity};

use crate::canvas_view::{self, CanvasView};
use crate::editor::Editor;
use crate::editor::docx_doc::{self, DocxPayload, EditablePayload};
use crate::editor::pdf_doc::{self, PdfPayload};
use crate::editor::xlsx_doc::{self, XlsxPayload};

/// #164 1c：实体构造计时包装（editor-built 段——六形态统一口径；
/// MANG_PERF 未开时 `since` 为 None，零开销直通）。
fn build_ed(
    since: Option<std::time::Instant>,
    cx: &mut App,
    f: impl FnOnce(&mut gpui::Context<Editor>) -> Editor,
) -> Entity<Editor> {
    let entity = cx.new(f);
    if let Some(since) = since {
        crate::perf::stage("editor-built", since);
    }
    entity
}

/// #164 1c：文档类型标识（扩展名小写——doc-open 证据行的 kind 字段）。
fn kind_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "other".into())
}

/// #164 1c：文件字节数（doc-open 证据行的规模面）。
fn file_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// #164 1c：规模单位数（文本=行数 / docx=段数 / pdf=页数 / xlsx=表数；
/// canvas 与失败态无统一单位计 0——kind 字段已区分）。
fn doc_units(opened: &OpenedDocument) -> usize {
    match opened {
        OpenedDocument::EditableText { content, .. } => content.split('\n').count(),
        OpenedDocument::EditableDocx { item, .. } => item.payload.props.paragraphs,
        OpenedDocument::ReadOnlyDocx { payload, .. } => payload.props.paragraphs,
        OpenedDocument::ReadOnlyPdf { payload, .. } => payload.props.pages,
        OpenedDocument::ReadOnlyXlsx { payload, .. } => payload.props.sheets,
        _ => 0,
    }
}

/// 打开结果。
pub enum OpenedDocument {
    /// 可编辑文本文档（md / txt）。
    EditableText { path: PathBuf, content: String },
    /// docx 只读渲染（批 3）：mdc-office-io 解析 + 行模型转换产物。
    ReadOnlyDocx { path: PathBuf, payload: DocxPayload },
    /// #115：docx **可编辑**装载（行模型 + 编辑桥 + 引擎窄面；桥 `None`
    /// 时编辑器自动降级只读——原因在 `payload.notice`）。
    EditableDocx {
        path: PathBuf,
        item: EditablePayload,
    },
    /// pdf 只读渲染（批 4）：mdc-pdf 文本层/书签抽取 + 行模型转换产物。
    ReadOnlyPdf { path: PathBuf, payload: PdfPayload },
    /// xlsx 只读渲染（G6）：office-io 读面 + 行模型转换产物。
    ReadOnlyXlsx { path: PathBuf, payload: XlsxPayload },
    /// canvas 画布视图（#133 任务 1：yrs 快照装载 + 视口位面——只读；
    /// 路线 A′ 依据见 canvas_view 模块文档）。
    Canvas { path: PathBuf, view: CanvasView },
    /// 不支持的扩展名。
    Unsupported { path: PathBuf },
    /// 读取失败（IO 错误 / 超大等）。
    OpenError { path: PathBuf, message: String },
}

/// 打开一个文件（分派 + 读取）。
pub fn open(path: &Path) -> OpenedDocument {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "txt" => match std::fs::read_to_string(path) {
            Ok(content) => OpenedDocument::EditableText {
                path: path.to_path_buf(),
                content,
            },
            Err(err) => OpenedDocument::OpenError {
                path: path.to_path_buf(),
                message: err.to_string(),
            },
        },
        // #115：docx 可编辑装载（宽面行模型 + 窄面引擎，段落序对账；
        // 对账/门限/严格门任一不过 → 载荷自带 notice 并降级只读）。
        "docx" => match docx_doc::load_editable(path) {
            Ok(item) => OpenedDocument::EditableDocx {
                path: path.to_path_buf(),
                item,
            },
            Err(message) => OpenedDocument::OpenError {
                path: path.to_path_buf(),
                message,
            },
        },
        // 批 4：pdf 只读渲染（应用层 100MB 前置门 + 引擎层 S3 双防线；
        // 扫描件/空页 → 标记行明确提示而非白屏）。
        "pdf" => match pdf_doc::load(path) {
            Ok(payload) => OpenedDocument::ReadOnlyPdf {
                path: path.to_path_buf(),
                payload,
            },
            Err(message) => OpenedDocument::OpenError {
                path: path.to_path_buf(),
                message,
            },
        },
        // G6：xlsx 只读渲染（office-io 三门 + calamine 五面读面；兼容
        // 模式畸形部件降级警告——查看器语义；空表/超限渲染护栏非静默）。
        "xlsx" => match xlsx_doc::load(path) {
            Ok(payload) => OpenedDocument::ReadOnlyXlsx {
                path: path.to_path_buf(),
                payload,
            },
            Err(message) => OpenedDocument::OpenError {
                path: path.to_path_buf(),
                message,
            },
        },
        // #133 任务 1：canvas 画布（yrs 权威快照；空文件 = 空会话——
        // 新文件语义。52MB 输入门在引擎 restore 层，同 mdc-collab 口径）。
        "canvas" => match canvas_view::CanvasView::load(path) {
            Ok(view) => OpenedDocument::Canvas {
                path: path.to_path_buf(),
                view,
            },
            Err(message) => OpenedDocument::OpenError {
                path: path.to_path_buf(),
                message,
            },
        },
        _ => OpenedDocument::Unsupported {
            path: path.to_path_buf(),
        },
    }
}

/// 打开结果（呈现层语义——main CLI 启动文档与 `ShellApp::open_document`
/// 共用 [`open_editor`] 通道时消费）。
pub enum OpenOutcome {
    /// 成功打开：编辑器实体（可编辑 md/txt 或只读 docx/pdf）。
    Opened(Entity<Editor>),
    /// 类型不支持——`message` 已是面向用户的完整文案（含支持清单；
    /// 区分「已知类型但渲染面未接」与「未知类型」）。
    Unsupported { path: PathBuf, message: String },
    /// 读取/解析失败（IO 错误 / 超大 / 畸形文件）。
    Failed { path: PathBuf, message: String },
}

/// 打开文件为编辑器实体（分派 + 读取 + 构造）。
///
/// main CLI（启动文档）与 `ShellApp::open_document`（Ctrl+O 对话框 /
/// 文件树 / 最近列表——F1/F2/F5）的共用通道，同一分派语义；docx/pdf
/// 附启动证据行（stderr——烟雾断言通道）。
pub fn open_editor(path: &Path, cx: &mut App) -> OpenOutcome {
    // #164 1c：doc-open 分段计时（读 + 解析段）——MANG_PERF 门控，
    // 未开时两次 `then` 分支预测级开销，无计时无分配。
    let since_open = crate::perf::on().then(std::time::Instant::now);
    let opened = open(path);
    if let Some(since) = since_open {
        crate::perf::doc_open(&kind_of(path), file_bytes(path), doc_units(&opened), since);
    }
    // #164 1c：editor-built 分段计时（实体构造段——六形态统一走 build_ed）。
    let since_build = crate::perf::on().then(std::time::Instant::now);
    match opened {
        OpenedDocument::EditableText { path, content } => {
            OpenOutcome::Opened(build_ed(since_build, cx, |cx| {
                let mut ed = Editor::new(Some(path), content, cx);
                // #427：折叠持久化**读侧**接线——文档装载完成即恢复已
                // 落盘折叠（写侧 persist_folds 挂在折叠命令路径，此前
                // 只写不读，重启即丢）。时机语义 = 装载即恢复（无渲染
                // 依赖）；「宁缺毋错」（头行漂移自动弃权）在
                // restore_folds 实现内。F7 只读对照视图不经此通道
                //（persist 只发生在可编辑命令路径，读写面对称）。
                ed.restore_folds();
                ed
            }))
        }
        // #115：可编辑 docx——启动日志一行（属性面 + 编辑面 + 降级原因）。
        OpenedDocument::EditableDocx { path, item } => {
            let payload = &item.payload;
            let mode = if item.bridge.is_some() {
                "可编辑"
            } else {
                "只读（降级）"
            };
            eprintln!(
                "docx: {} 段（可编辑 {} / 锁定 {}）/ {} 表（{} 行）/ {} 标题 / {} 图片 / {} 警告（{mode}，{}）",
                payload.props.paragraphs,
                payload.props.editable_paragraphs,
                payload.props.locked_paragraphs,
                payload.props.tables,
                payload.props.table_rows,
                payload.headings.len(),
                payload.props.images,
                payload.props.warnings,
                path.display()
            );
            if let Some(notice) = &payload.notice {
                eprintln!("docx: 降级说明——{notice}（{}）", path.display());
            }
            OpenOutcome::Opened(build_ed(since_build, cx, |cx| {
                Editor::new_docx_editable(path, item, cx)
            }))
        }
        // 批 3 证据通道：启动日志一行（属性面 + 只读标记）。
        OpenedDocument::ReadOnlyDocx { path, payload } => {
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
            OpenOutcome::Opened(build_ed(since_build, cx, |cx| {
                Editor::new_docx(path, payload, cx)
            }))
        }
        // 批 4 证据通道：启动日志一行（属性面 + 只读标记）。
        OpenedDocument::ReadOnlyPdf { path, payload } => {
            let scanned = if payload.props.scanned {
                " / 全文无文本层（扫描件提示）"
            } else {
                ""
            };
            eprintln!(
                "pdf: {} 页 / {} 字符 / {} 书签 / {} 空文本页{}（只读，{}）",
                payload.props.pages,
                payload.props.chars,
                payload.headings.len(),
                payload.props.empty_pages,
                scanned,
                path.display()
            );
            OpenOutcome::Opened(build_ed(since_build, cx, |cx| {
                Editor::new_pdf(path, payload, cx)
            }))
        }
        // G6 证据通道：启动日志一行（属性面 + 只读标记）。
        OpenedDocument::ReadOnlyXlsx { path, payload } => {
            let truncated = if payload.props.truncated_sheets > 0 {
                format!(" / {} 表超渲染护栏", payload.props.truncated_sheets)
            } else {
                String::new()
            };
            eprintln!(
                "xlsx: {} 表 / {} 非空格 / {} 公式 / {} 合并 / {} 超链接 / {} 警告{}（只读，{}）",
                payload.props.sheets,
                payload.props.cells,
                payload.props.formulas,
                payload.props.merges,
                payload.props.hyperlinks,
                payload.props.warnings,
                truncated,
                path.display()
            );
            OpenOutcome::Opened(build_ed(since_build, cx, |cx| {
                Editor::new_xlsx(path, payload, cx)
            }))
        }
        // #133 证据通道：启动日志一行（对象/图元/内容/视口 + 只读标记）。
        OpenedDocument::Canvas { path, view } => {
            eprintln!("{}", view.open_evidence(&path));
            OpenOutcome::Opened(build_ed(since_build, cx, |cx| {
                Editor::new_canvas(path, view, cx)
            }))
        }
        OpenedDocument::Unsupported { path } => OpenOutcome::Unsupported {
            message: unsupported_message(&path),
            path,
        },
        OpenedDocument::OpenError { path, message } => OpenOutcome::Failed { path, message },
    }
}

/// 不支持类型的面向用户文案：区分「已知类型但编辑器渲染面未接」与
/// 「未知类型」——前者明确告知边界（G6 起 xlsx 已接只读渲染，
/// .xlsm/.xls 旧/宏格式仍在范围外），后者给出支持清单。
fn unsupported_message(path: &Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "xlsm" | "xls" => {
            "电子表格旧/宏格式（.xlsm/.xls）渲染面未接——支持 .md/.markdown/.txt（可编辑）与 .docx/.pdf/.xlsx/.canvas（只读）"
                .to_string()
        }
        _ => "暂不支持的文件类型——支持 .md/.markdown/.txt（可编辑）与 .docx/.pdf/.xlsx/.canvas（只读）".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F1→G6：不支持文案分叉——.xls（已知未接）与未知类型各走各的
    /// 口径；.xlsx 自 G6 起接只读渲染，不再落「已知未接」文案。
    #[test]
    fn unsupported_message_distinguishes_known_unrendered() {
        let xls = unsupported_message(Path::new("a/b/旧版报价.xls"));
        assert!(xls.contains("渲染面未接"), "xls 走已知未接文案：{xls}");
        assert!(xls.contains(".xlsx"), "文案支持清单含 xlsx：{xls}");

        // G6：xlsx 已接——不再出现「渲染面未接」边界文案。
        let xlsx = unsupported_message(Path::new("a/b/报价单.xlsx"));
        assert!(
            !xlsx.contains("渲染面未接"),
            "xlsx 不再走已知未接文案：{xlsx}"
        );

        let unknown = unsupported_message(Path::new("a/b/data.xyz"));
        assert!(
            unknown.contains("暂不支持"),
            "未知类型走通用文案：{unknown}"
        );
        assert!(!unknown.contains("渲染面未接"));
    }

    /// F1：扩展名大小写不敏感（与 `open` 分派同口径）。
    #[test]
    fn unsupported_message_ext_case_insensitive() {
        assert!(unsupported_message(Path::new("X.XLS")).contains("渲染面未接"));
    }

    /// #133 任务 1：canvas 分派接线——夹具经 `open` 落 Canvas（对象/
    /// 图元计数 + 视口初值；夹具由 `gen_smoke_canvas`（ignored）生成
    /// 入库——真实会话命令链 → yrs 权威快照）。
    #[test]
    fn open_dispatches_canvas_to_view_payload() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/smoke.canvas");
        match open(std::path::Path::new(path)) {
            OpenedDocument::Canvas { view, .. } => {
                let (total, nodes, edges, groups) = view.object_counts();
                assert_eq!((total, nodes, edges, groups), (9, 6, 2, 1));
                assert_eq!(view.shape_count(), 8, "分组不画，成员各自成图元");
                assert_eq!(
                    view.viewport(),
                    canvas_view::Viewport::default(),
                    "初值 = 适配居中"
                );
            }
            OpenedDocument::Unsupported { .. } | OpenedDocument::OpenError { .. } => {
                panic!(
                    "夹具缺失或不可读：先跑 `cargo test gen_smoke_canvas -- --ignored` 生成；夹具必须进仓"
                )
            }
            _ => panic!("canvas 分派应落 Canvas（实得其他形态）"),
        }
    }

    /// G6：xlsx 分派接线——夹具经 `open` 落 ReadOnlyXlsx（载荷含
    /// 工作表树/属性；夹具由 `gen_smoke_xlsx`（ignored）生成入库）。
    #[test]
    fn open_dispatches_xlsx_to_readonly_payload() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/smoke.xlsx");
        match open(std::path::Path::new(path)) {
            OpenedDocument::ReadOnlyXlsx { payload, .. } => {
                assert_eq!(payload.headings.len(), 2, "两表树");
                assert_eq!(payload.props.sheets, 2);
                assert_eq!(payload.props.formulas, 1, "一格公式");
                assert!(
                    payload.text.contains("── 工作表: 销售明细 ──"),
                    "表分隔行入文本：{}",
                    payload.text.split('\n').next().unwrap_or_default()
                );
            }
            OpenedDocument::Unsupported { .. } => {
                panic!("夹具缺失：先跑 `cargo test gen_smoke_xlsx -- --ignored` 生成")
            }
            _ => panic!("xlsx 分派应落 ReadOnlyXlsx（实得其他形态——IO 错误/读失败）"),
        }
    }

    /// #427：折叠持久化端到端——命令路径落盘（run_command(FoldAll) →
    /// persist_folds → settings::save_folds）→ **同路径重开**（open_editor
    /// 装载即恢复 restore_folds）→ 折叠集一致；失效头（非标题行/越界）
    /// 宁缺毋错弃权。
    ///
    /// 隔离：XDG_CONFIG_HOME 指向临时目录（`config_dir` 只认绝对路径）。
    /// 窗口期内并行用例读配置得缺省值（`load_indent_unit` 等——无害）；
    /// 全局配置写方仅 CMD 全量测试的 persist（load-modify-write，与本
    /// 用例的理论交错窗口毫秒级——与既有全局配置测试同类风险，如实
    /// 登记不假装零）。
    #[gpui::test]
    fn fold_persistence_roundtrip_on_reopen(cx: &mut gpui::TestAppContext) {
        let cfg = std::env::temp_dir().join(format!("mang-fold-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).expect("建隔离配置目录");
        let doc = cfg.join("notes.md");
        // 三小节文档：行 0/2/4 是标题，行 1/3/5 是正文。
        std::fs::write(&doc, "# A\nbody1\n# B\nbody2\n# C\nbody3").expect("写测试文档");
        let key = doc.to_string_lossy().into_owned();

        let prev = std::env::var_os("XDG_CONFIG_HOME");
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &cfg) };

        // 窗口宿主（run_command 的焦点面需要真实 Window——与 editor
        // 单测同模式：最小宿主开窗）。
        struct Host(gpui::Entity<Editor>);
        impl gpui::Render for Host {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                gpui::div()
            }
        }

        // ① 首开：隔离配置为空 → 无历史折叠。
        let a = match cx.update(|cx| open_editor(&doc, cx)) {
            OpenOutcome::Opened(ed) => ed,
            other => panic!("首开应成功（实得非 Opened 形态）"),
        };
        let fresh = cx.update(|cx| a.read(cx).fold_ranges());
        assert!(fresh.is_empty(), "隔离配置下首开应无折叠（实得 {fresh:?}）");

        // ② 真实用户链折全部：run_command(FoldAll) → persist_folds 落盘。
        let a_for_host = a.clone();
        let window = cx.add_window(move |_, _| Host(a_for_host.clone()));
        window.update(cx, |_host, window, cx| {
            a.update(cx, |ed, cx| {
                ed.run_command(crate::editor::commands::Cmd::FoldAll, window, cx)
            });
        });
        let folded = cx.update(|cx| a.read(cx).fold_ranges());
        assert_eq!(folded, vec![(0, 2), (2, 4), (4, 6)], "三个小节全折（顶层）");

        // ③ 重开同路径：装载即恢复（#427 修复面——此前 restore_folds
        //    零调用方，重启即丢）。
        let b = match cx.update(|cx| open_editor(&doc, cx)) {
            OpenOutcome::Opened(ed) => ed,
            other => panic!("重开应成功（实得非 Opened 形态）"),
        };
        let restored = cx.update(|cx| b.read(cx).fold_ranges());
        assert_eq!(restored, folded, "重开后折叠集应与关闭前一致");

        // ④ 宁缺毋错：失效头（行 1 正文非标题 / 行 9 越界）全部弃权。
        crate::settings::save_folds(&key, &[1, 9]);
        let c = match cx.update(|cx| open_editor(&doc, cx)) {
            OpenOutcome::Opened(ed) => ed,
            other => panic!("三开应成功（实得非 Opened 形态）"),
        };
        let dropped = cx.update(|cx| c.read(cx).fold_ranges());
        assert!(dropped.is_empty(), "失效头应全部弃权（实得 {dropped:?}）");

        // 恢复 env + 清理（失败路径也尽量不残留：先 env 后目录）。
        match prev {
            Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
