//! PDF 页面位图视图（#289-④——「快捷键打开文件 pdf 显示却是只有文字
//! 没有格式像 md」的修复面）。
//!
//! ## 定位
//!
//! 旧版 pdf 标签 = 文本行模型（[`crate::editor::pdf_doc`]——pdfium
//! `extract_text` → 行 + span，查找/书签/光标全复用 Editor 管线）。
//! 用户实测反馈：**PDF 必须长得像 PDF**——表格/图片/字体/版面要原样
//! 呈现，纯文本列表糊弄不过去。
//!
//! 本模块 = 真·页面渲染视图：mdc-pdf [`Rasterizer`]（pdfium）把每页
//! 光栅化为 BGRA 位图 → gpui `img`（`ImageSource::Render`）按页竖排。
//! **文本行模型不废弃**——两视图并存可切（状态栏「页面视图 / 文本
//! 视图」钮；Ctrl+F 在页面视图下自动切回文本视图再开查找条）：
//!
//! ```text
//! PdfPagesView（本模块——页面位图，主显示面）
//!      │  engine-pdfium 光栅化（RenderWorker 专用线程 + 看门狗换代）
//!      │  PageCache（RenderKey 键控 → RenderImage，LRU 字节预算）
//!      │  右键拖框批注：高亮/下划线/便签三手势（#302④ 验收 1：
//!      │    PdfAnnotateController 增量写面；退化单击 = 工具切换）
//!      └─ Editor（既有 pdf 文本模型——查找/书签锚/属性面板/字数）
//! ```
//!
//! ## 渲染管线（懒加载）
//!
//! - **uniform_list 虚拟化**：页槽等高（首页纵横比 × 适宽），只有
//!   可见页（±1 页过扫描）进渲染——百页档不爆内存；
//! - **懒光栅**：render 期按滚动偏移算可见区间，未缓存页经
//!   [`RenderWorker::submit`] 派单（专用渲染线程——UI 不卡帧）；
//! - **轮询收账**：提交时起一个 16ms 定时循环 `try_recv` 收回执，
//!   `Done` → `PageCache::put` + `cx.notify()`（下一帧换上真图），
//!   待办清空即退出循环（空闲零开销）；
//! - **宽度量子化**：缓存键的宽取 64px 倍数（窗口拖动时连续 resize
//!   不至于页页重光栅）；
//! - **#305 RenderKey 键控**：缓存键含 `annotation_rev`——批注
//!   flush（#302④）后旧位图自然 miss，重渲染即见新批注；旧 rev
//!   条目经 `invalidate_stale_revisions` 即刻回收预算。
//!
//! ## 批注交互（#302 阶段 2·④——验收 1 三手势）
//!
//! **右键拖框 = 按当前工具提交批注**（右键不占列表滚动）：按下记录
//! 页内起点 → 松开由 [`PdfAnnotateController`] 走完整写路径（#304
//! 坐标变换 → `AnnotateAdd` → 增量 flush）→ 本视图 `reload_after_flush`
//! （换字节快照 + 重建渲染工人 + 缓存失效）→ 下一帧重光栅即见批注。
//! 控制器首次使用时懒初始化（open_scan + 写器门一次性成本）。
//!
//! **退化右键单击（无拖框意图）= 循环切换工具**：高亮 → 下划线 →
//! 便签 → 高亮；底部横幅明示当前工具（下一次手势提交时清零——不
//! 永久占屏）。三手势分别落 `/Highlight`、`/Underline`（QuadPoints 面）
//! 与 `/Text`（锚定 Rect 面）——引擎写面同一条命令链，签名保持语义
//! 同锁。
//!
//! ## 已知边界（v1 登记）
//!
//! - **页尺寸不均的档**：uniform_list 等高槽——异形页按首页纵横比
//!   定槽高，真图内接呈现（`ObjectFit::Contain` 缺省），不裁不拉；
//!   异形页上的右键拖框坐标按等比槽近似（拖框落点在异形页上有
//!   letterbox 偏移——与「异形页内接呈现」同源边界，登记待后续轮）；
//! - **页面视图下无文本查找**：Ctrl+F 自动切文本视图（提示横幅说明）
//!   ——跨视图查找/选区属后续轮；
//! - **无缩放**：适宽单档（Ctrl+= 字号缩放对 pdf 页不适用——页面
//!   缩放属后续轮）；
//! - 扫描件（无文本层）在页面视图下**正是正确呈现**（位图就是内容）
//!   ——文本视图的「OCR 不支持」提示不再挡路；
//! - **沙箱无 GUI**：右键拖框交互层只有编译面验证（本机 cargo check），
//!   手势体验待真机；写路径逻辑面（坐标变换/命令/flush/落盘/签名
//!   保持）在 `tests/pdf_annotate_e2e.rs` 全量真跑（无 GUI 依赖）。
//!
//! ## 引擎绑定纪律
//!
//! 与 [`crate::editor::pdf_doc`] 同源：libpdfium.so 定位序
//! `MDC_PDFIUM_LIB` → 编译期候选 → 系统；`PdfiumEngine::open` 触发
//! 进程级单绑定。无 libpdfium 环境（CI）下 `PdfPagesView::new` 走
//! `error` 态（面板明示「pdfium 未绑定」）——不崩不白屏。

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, DispatchPhase, FocusHandle, Focusable, ImageSource, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, UniformListScrollHandle, Window, px,
};
use image::{Frame, ImageBuffer};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::page::PageIndex;
use mdc_pdf::render::{RenderKey, RenderRequest, RenderedPage};
use mdc_pdf::worker::{RenderWorker, WorkerReply};
use mdc_pdf::writer::IncrementalSave;
use smallvec::SmallVec;
use ui::prelude::*;

use crate::pdf_annotate::{AnnotateTool, PdfAnnotateController};

/// 光栅宽量子化步长（px）——缓存键稳定性 vs 重光栅频率的折中。
pub const WIDTH_QUANTUM: u32 = 64;
/// 光栅宽下限（px）——再窄不可读，光栅成本也不降。
pub const WIDTH_MIN: u32 = 256;
/// 光栅宽上限（px）——4K 屏适宽 + 余量；引擎侧 S3-L2 仍有独立钳制。
pub const WIDTH_MAX: u32 = 4096;
/// 页面缓存字节预算（192MB——A4 适宽 BGRA ≈ 1.4MB/页，可容百页级
/// 工作集；超预算 LRU 逐出，回滚再光栅）。
pub const CACHE_BUDGET_BYTES: usize = 192 * 1024 * 1024;
/// 看门狗预算（单页光栅超时——与 mdc-pdf worker 文档的 20s 预算同源）。
const RENDER_WATCHDOG: Duration = Duration::from_secs(20);
/// 回执轮询间隔（渲染中 ~60fps 足够；空闲零轮询）。
const POLL_INTERVAL: Duration = Duration::from_millis(16);
/// 页左右留白（px）——页面与视口缘的呼吸。
const PAGE_MARGIN_X: Pixels = px(12.);

/// 预渲染身份键：（0 基页号, 量子化光栅宽）——渲染前不知引擎侧
/// 实际高（页各自纵横比），缓存全键（[`RenderKey`]）待回执落地后
/// 才能组装（高度由引擎报告）。
type PageKey = (u32, u32);

/// PDF 页面位图视图实体（每 pdf 标签一份，由 ShellApp 持有/切换）。
pub struct PdfPagesView {
    /// 源字节（看门狗换代工厂重开引擎的原料——Arc 免拷贝）。
    bytes: Arc<Vec<u8>>,
    /// 总页数（构造期主线程一次性读取；0 = 引擎面失败走 error 态）。
    page_count: usize,
    /// 首页纵横比（高/宽，含旋转归一）——uniform 槽高的基准。
    aspect: f32,
    /// 渲染工人（专用线程 + 看门狗换代；error 态为 None）。
    worker: Option<RenderWorker<PdfiumEngine>>,
    /// 页面缓存（RenderKey 键控——含 annotation_rev；LRU 字节预算）。
    cache: mdc_pdf::cache::PageCache<RenderKey, Arc<gpui::RenderImage>>,
    /// 在途作业：job_id → 缓存键（回执对账 + 失败不再无限重提）。
    pending: HashMap<u64, PageKey>,
    /// 失败页（Done(Err)/Timeout 一次性登记——不无限重试敌意页）。
    failed: HashSet<PageKey>,
    /// 引擎侧实际光栅高（页各自纵横比）：(页, 宽) → 高——RenderKey
    /// 组装素材（渲染前未知，回执落地后记录；同键重复渲染零成本）。
    rendered_heights: HashMap<PageKey, u32>,
    /// #302④：标注写会话（首次右键拖框时懒初始化——open_scan +
    /// 写器门的一次性成本不落在纯阅读路径上）。
    annotate: Option<PdfAnnotateController>,
    /// 标注修订号镜像（控制器 flush 后同步——RenderKey 键控源；
    /// 0 = 无批注会话/从未 flush）。
    annotate_rev: u64,
    /// 右键拖框会话（按下时的页号 + 页内像素起点；None = 非拖框中）。
    drag_start: Option<(u32, f32, f32)>,
    /// #302 验收 1：当前批注工具（三手势——高亮/下划线/便签；退化
    /// 右键单击循环切换，见 [`Self::finish_drag_annotation`]）。
    annotate_tool: AnnotateTool,
    /// 批注路径错误（最近一次——面板明示，不静默丢；用户可重试）。
    annotate_error: Option<String>,
    /// 工具切换提示（最近一次退化单击——底部横幅明示当前工具与切换
    /// 手势；下一次手势提交时清零——不永久占屏）。
    annotate_hint: Option<String>,
    /// #386：有未落盘批注（首次 [`Self::commit_annotation`] 成功后
    /// 置位，[`Self::save_annotations`] 写盘成功清零）。Shell 据此
    /// 驱动关闭确认与标题栏脏标。
    dirty: bool,
    /// 回执轮询循环进行中（防重复起循环）。
    polling: bool,
    /// 虚拟列表滚动句柄（scroll_to_page / 可见区间推算共用）。
    scroll: UniformListScrollHandle,
    /// 焦点域（键盘域占位——v1 无键位面，滚轮/拖滚动条不依赖焦点）。
    focus_handle: FocusHandle,
    /// 视口尺寸（宽, 高）——最近一次消费值（覆盖 canvas 每帧回写
    /// `viewport` Cell，render 期取用；首帧 0 = 未记录，下帧起算）。
    viewport_size: (f32, f32),
    /// 视口尺寸回写通道（覆盖 canvas paint 回调持有同款 Rc——零实体
    /// 更新零重入；不注册命中区，不挡列表滚轮）。
    viewport: Rc<Cell<Option<(f32, f32)>>>,
    /// 引擎面错误（构造期 open 失败——面板明示，不白屏）。
    error: Option<String>,
}

impl PdfPagesView {
    /// 构造：主线程一次性 open 读页数/几何（毫秒级），随即把引擎交
    /// 给看门狗工人在渲染线程重建（工厂持字节快照——换代同源）。
    pub fn new(bytes: Vec<u8>, cx: &mut App) -> Self {
        let bytes = Arc::new(bytes);
        let focus_handle = cx.focus_handle();
        // 几何预读（一次性）：页数 + 首页纵横比（前 3 页取首个有效几何
        // ——封面页偶有异形，首页族比单首页稳）。失败 → error 态面板。
        let (page_count, aspect) = match PdfiumEngine::open((*bytes).clone()) {
            Ok(engine) => {
                let count = engine.page_count() as usize;
                let aspect = (0..count.min(3))
                    .filter_map(|i| engine.page_geometry(PageIndex(i as u32)).ok())
                    .map(|g| {
                        let w = g.effective_width();
                        let h = g.effective_height();
                        if w > 0. && h > 0. { h / w } else { 1.414 }
                    })
                    .next()
                    .unwrap_or(1.414);
                (count, aspect)
            }
            Err(err) => {
                return Self::error_state(
                    bytes,
                    focus_handle,
                    format!("pdfium 引擎打开失败：{err:?}"),
                );
            }
        };
        // 看门狗工人：工厂持字节快照，首代/替代代都在渲染线程内构造
        // （工厂 panic 不炸宿主帧——worker.rs 既有纪律）。
        let factory_bytes = bytes.clone();
        let worker = RenderWorker::watchdog(
            move || {
                PdfiumEngine::open((*factory_bytes).clone())
                    .expect("pdfium 看门狗工厂：字节快照重开失败（几何预读已验证可开）")
            },
            RENDER_WATCHDOG,
        );
        Self {
            bytes,
            page_count,
            aspect,
            worker: Some(worker),
            cache: mdc_pdf::cache::PageCache::new(CACHE_BUDGET_BYTES),
            pending: HashMap::new(),
            failed: HashSet::new(),
            rendered_heights: HashMap::new(),
            annotate: None,
            annotate_rev: 0,
            drag_start: None,
            annotate_tool: AnnotateTool::Highlight,
            annotate_error: None,
            annotate_hint: None,
            dirty: false,
            polling: false,
            scroll: UniformListScrollHandle::new(),
            focus_handle,
            viewport_size: (0., 0.),
            viewport: Rc::new(Cell::new(None)),
            error: None,
        }
    }

    /// error 态构造（引擎绑定/打开失败——面板明示）。
    fn error_state(bytes: Arc<Vec<u8>>, focus_handle: FocusHandle, message: String) -> Self {
        Self {
            bytes,
            page_count: 0,
            aspect: 1.414,
            worker: None,
            cache: mdc_pdf::cache::PageCache::new(CACHE_BUDGET_BYTES),
            pending: HashMap::new(),
            failed: HashSet::new(),
            rendered_heights: HashMap::new(),
            annotate: None,
            annotate_rev: 0,
            drag_start: None,
            annotate_tool: AnnotateTool::Highlight,
            annotate_error: None,
            annotate_hint: None,
            dirty: false,
            polling: false,
            scroll: UniformListScrollHandle::new(),
            focus_handle,
            viewport_size: (0., 0.),
            viewport: Rc::new(Cell::new(None)),
            error: Some(message),
        }
    }

    /// 适宽（px）——视口宽 - 页边距；量子化 + 域钳制（纯函数面，
    /// [`quantize_width`] 单测锚点）。
    fn fit_width(viewport_w: f32) -> u32 {
        let w = (viewport_w - 2. * f32::from(PAGE_MARGIN_X)).max(WIDTH_MIN as f32);
        quantize_width(w as u32)
    }

    /// 槽高（px）：适宽 × 首页纵横比（纯函数——单测锚点）。
    fn slot_height(fit_w: u32, aspect: f32) -> f32 {
        fit_w as f32 * aspect
    }

    /// 滚到指定页（0 基——书签跳转锚；越界钳到末页）。
    pub fn scroll_to_page(&self, page: usize) {
        if self.page_count == 0 {
            return;
        }
        let idx = page.min(self.page_count - 1);
        self.scroll.scroll_to_item(idx, gpui::ScrollStrategy::Top);
    }

    /// render 期派单：按滚动偏移 + 视口高推算可见区间（±1 页过扫描），
    /// 未缓存且未在途且未失败的页逐个 submit。先收上一帧视口尺寸。
    fn submit_visible(&mut self, cx: &mut Context<Self>) {
        if self.worker.is_none() || self.page_count == 0 {
            return;
        }
        // 消费覆盖 canvas 上帧记录的视口尺寸。
        if let Some((w, h)) = self.viewport.get() {
            if w > 0. && h > 0. {
                self.viewport_size = (w, h);
            }
        }
        let (vw, vh) = self.viewport_size;
        if vw <= 0. {
            return; // 首帧视口未记录——覆盖 canvas 本帧落位，下一帧起算。
        }
        let fit_w = Self::fit_width(vw);
        let slot_h = Self::slot_height(fit_w, self.aspect).max(1.);
        // 滚动偏移（内容坐标——base_handle.offset）。
        let offset_y = f32::from(self.scroll.0.borrow().base_handle.offset().y).max(0.);
        let first = (((offset_y / slot_h) as i64).saturating_sub(1)).max(0) as u32;
        let visible_rows = (((vh / slot_h).ceil() as i64) + 2).max(1) as u32;
        let last = (first + visible_rows).min(self.page_count as u32);
        let mut submitted = false;
        for page in first..last {
            let key = (page, fit_w);
            if self
                .render_key(&key)
                .is_some_and(|k| self.cache.contains(&k))
                || self.failed.contains(&key)
                || self.pending.values().any(|k| *k == key)
            {
                continue;
            }
            let id = self.worker.as_mut().unwrap().submit(RenderRequest {
                page: PageIndex(key.0),
                width_px: key.1,
            });
            self.pending.insert(id, key);
            submitted = true;
        }
        if submitted && !self.polling {
            self.polling = true;
            Self::spawn_poll_loop(cx);
        }
    }

    /// 预渲染键 → 缓存全键（#305 RenderKey）：高度取引擎侧实测
    /// （`rendered_heights`——首次渲染前无记录 → None = 必 miss，
    /// 渲染回执落地后同键即命中）。rev = 标注修订号（flush 推进）。
    fn render_key(&self, key: &PageKey) -> Option<RenderKey> {
        let height = *self.rendered_heights.get(key)?;
        Some(RenderKey::new(key.0, key.1, height, self.annotate_rev))
    }

    /// 回执轮询循环（16ms 定时 try_recv——待办清空退出；实体释放自然止）。
    fn spawn_poll_loop(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let mut done = false;
                let _ = this.update(cx, |view, cx| {
                    view.drain_replies(cx);
                    if view.pending.is_empty() {
                        view.polling = false;
                        done = true;
                    }
                });
                if done {
                    break;
                }
            }
        })
        .detach();
    }

    /// 收回执（Done → 缓存 + notify；Cancelled/Timeout/Err → failed 登记）。
    fn drain_replies(&mut self, cx: &mut Context<Self>) {
        let Some(worker) = self.worker.as_mut() else {
            return;
        };
        while let Some(reply) = worker.try_recv() {
            match reply {
                WorkerReply::Done { id, result } => {
                    let Some(key) = self.pending.remove(&id) else {
                        continue; // 迟到回执（换代竞态）——机械丢弃。
                    };
                    match result {
                        Ok(page) => {
                            if let Some(image) = rendered_page_to_image(&page) {
                                let cost = page.bgra.len();
                                // #305：全键落地（引擎侧实测高 + 当前 rev）
                                // + 高度记录（后续帧组装键的素材）。
                                self.rendered_heights.insert(key, page.height_px);
                                let full =
                                    RenderKey::new(key.0, key.1, page.height_px, self.annotate_rev);
                                let _evicted = self.cache.put(full, Arc::new(image), cost);
                                cx.notify();
                            } else {
                                self.failed.insert(key);
                                cx.notify();
                            }
                        }
                        Err(_) => {
                            self.failed.insert(key);
                            cx.notify();
                        }
                    }
                }
                WorkerReply::Cancelled { id } | WorkerReply::Timeout { id } => {
                    if let Some(key) = self.pending.remove(&id) {
                        self.failed.insert(key);
                        cx.notify();
                    }
                }
            }
        }
    }

    /// 可见页元素（uniform_list 回调——读缓存出位图，未就绪出占位框：
    /// 渲染中 / 失败两态）。#302④：页面槽上叠透明 canvas——paint 期
    /// 注册右键拖框观察器（窗口级 on_mouse_event + 自身 bounds 过滤，
    /// 与 shell 面板拖拽收尾钩同模式；无命中区不挡滚动）。
    fn render_page_slot(&mut self, idx: usize, cx: &App) -> AnyElement {
        let colors = cx.theme().colors().clone();
        let (vw, _) = self.viewport_size;
        let fit_w = if vw > 0. {
            Self::fit_width(vw)
        } else {
            WIDTH_MIN
        };
        let key = (idx as u32, fit_w);
        let slot_h = Self::slot_height(fit_w, self.aspect);
        let page_no = idx + 1;
        let image = self
            .render_key(&key)
            .and_then(|k| self.cache.get(&k).cloned());
        match image {
            Some(image) => gpui::img(ImageSource::Render(image))
                .id(gpui::ElementId::NamedInteger("pdf-page".into(), idx as u64))
                .w(px(fit_w as f32))
                .h(px(slot_h))
                .flex_none()
                .rounded_sm()
                .shadow_md()
                .into_any_element(),
            None => {
                let failed = self.failed.contains(&key);
                div()
                    .id(gpui::SharedString::from(format!("pdf-page-pending-{idx}")))
                    .w(px(fit_w as f32))
                    .h(px(slot_h))
                    .flex_none()
                    .rounded_sm()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.panel_background)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        ui::Label::new(if failed {
                            format!("第 {page_no} 页渲染失败")
                        } else {
                            format!("第 {page_no} 页渲染中…")
                        })
                        .size(ui::LabelSize::Small)
                        .color(ui::Color::Muted),
                    )
                    .into_any_element()
            }
        }
    }

    // ------------------------------------------------------------------
    // #302 阶段 2·④：右键拖框批注（三手势）→ 增量写 → 渲染刷新
    // ------------------------------------------------------------------

    /// 右键拖框收尾：页内拖框（跨页丢弃）→ 按当前工具提交完整写路径
    /// → flush 后视图刷新。**退化单击 = 循环切换工具**（高亮→下划线
    /// →便签→高亮——无拖框意图的单击不当零面积批注，改当工具选择；
    /// 底部横幅明示当前工具，下一次手势提交时清零）。错误不静默丢
    /// （面板明示，用户可重试）。
    fn finish_drag_annotation(&mut self, page: u32, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some((drag_page, x0, y0)) = self.drag_start.take() else {
            return; // 非拖框会话（拖框起点不在本页——按下观察器未命中）。
        };
        if drag_page != page {
            return; // 跨页拖框（v1 页内语义——丢弃不猜）。
        }
        if (x - x0).abs() < 3.0 && (y - y0).abs() < 3.0 {
            // 退化单击：无框选意图——循环切换批注工具并明示。
            self.annotate_tool = self.annotate_tool.next();
            self.annotate_hint = Some(format!(
                "批注工具：{}（右键拖框使用；单击继续切换：高亮→下划线→便签）",
                self.annotate_tool.label()
            ));
            cx.notify();
            return;
        }
        match self.commit_annotation(page, x0, y0, x, y) {
            Ok(()) => {
                self.annotate_error = None;
                // 提交成功 → 工具提示使命完成（不永久占屏）。
                self.annotate_hint = None;
            }
            Err(e) => self.annotate_error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// 提交批注（三手势派发）：懒初始化控制器 → #304 坐标变换（屏幕
    /// 拖框→用户空间）→ AnnotateAdd → 增量 flush → [`Self::reload_after_flush`]。
    /// #386：`pub(crate)`——e2e 测试从手势提交入口驱动（与右键拖框
    /// 收尾 [`Self::finish_drag_annotation`] 同一调用面）。
    pub(crate) fn commit_annotation(
        &mut self,
        page: u32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    ) -> Result<(), crate::pdf_annotate::AnnotateError> {
        if self.annotate.is_none() {
            let controller = PdfAnnotateController::open((*self.bytes).clone())?;
            self.annotate = Some(controller);
        }
        let controller = self.annotate.as_mut().expect("刚初始化");
        // DPI：位图 px / 显示域 pt × 72（等比槽近似——异形页 letterbox
        // 边界见模块文档 v1 登记）。
        let transform = controller.page_transform(page)?;
        let display_w = transform.display_size().width.max(1.0);
        let (vw, _) = self.viewport_size;
        let fit_w = if vw > 0. {
            Self::fit_width(vw)
        } else {
            WIDTH_MIN
        };
        let dpi = fit_w as f32 / display_w * 72.0;
        // 三手势派发（issue #302 验收 1：高亮/下划线/便签）。
        match self.annotate_tool {
            AnnotateTool::Highlight => {
                controller.add_highlight_from_screen(
                    page,
                    x0,
                    y0,
                    x1,
                    y1,
                    dpi,
                    "mang-shell 用户",
                )?;
            }
            AnnotateTool::Underline => {
                controller.add_underline_from_screen(
                    page,
                    x0,
                    y0,
                    x1,
                    y1,
                    dpi,
                    "mang-shell 用户",
                )?;
            }
            AnnotateTool::Note => {
                controller.add_note_from_screen(page, x0, y0, x1, y1, dpi, "mang-shell 用户")?;
            }
        }
        let (bytes, _stats) = controller.flush()?;
        self.reload_after_flush(bytes);
        // #386：内存已刷新但盘上未落——脏标记（Shell 据此驱动关闭
        // 确认与保存分流；save_annotations 清零）。
        self.dirty = true;
        Ok(())
    }

    /// #386：是否有未落盘批注（Shell 据此驱动关闭确认与标题栏脏标；
    /// e2e 断言面）。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// #386 e2e 断言面：构造期引擎错误（None = 页视图在位可渲染；
    /// Some = error 态面板文案）。测试据此在无 pdfium 环境 SKIP
    /// （render_170_pdf 同口径——不伪装通过）。
    pub(crate) fn engine_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// #386：批注落盘（增量——产物字节以原文件为逐位前缀，签名保持
    /// #227/#308 语义）。路径由 Shell 传入（单一真源：Shell 的
    /// `pdf_pages` 槽持有 `PathBuf`，视图不重复存）。**写盘成功才清
    /// 脏标**（原子语义）；错误透传（调用方必须显式示警，不许静默）。
    ///
    /// - `Ok(None)`：无未落盘批注（干净 no-op）；
    /// - `Ok(Some(stats))`：已增量写盘（`stats` = 追加修订统计）。
    pub fn save_annotations(
        &mut self,
        path: &Path,
    ) -> Result<Option<IncrementalSave>, crate::pdf_annotate::AnnotateError> {
        if !self.dirty {
            return Ok(None);
        }
        let Some(controller) = self.annotate.as_mut() else {
            // 脏标在位而控制器不在（不可达——dirty 只经
            // commit_annotation 成功路径置位，控制器随之在位）。防御
            // 态：清标按干净处理，不虚构写盘。
            self.dirty = false;
            return Ok(None);
        };
        let stats = controller.save(path)?;
        self.dirty = false;
        eprintln!("pdf-annot-save: {}（增量落盘）", path.display());
        Ok(Some(stats))
    }

    /// flush 后刷新：换字节快照 + 重建渲染工人（引擎重开见追加修订）
    /// + 旧 rev 缓存即刻回收（#305）+ rev 镜像同步（RenderKey 键控）。
    /// 几何未变（批注不改页树）——`rendered_heights` 保留，同键新 rev
    /// 重光栅后即命中。
    fn reload_after_flush(&mut self, bytes: Vec<u8>) {
        self.bytes = Arc::new(bytes);
        self.annotate_rev = self
            .annotate
            .as_ref()
            .map(|c| c.annotation_rev())
            .unwrap_or(0);
        // 在途/失败清零：旧工人即将退役，其回执不再落地（换代竞态的
        // 机械丢弃路径）；新工人重提时这些键不在位即会重派。
        self.pending.clear();
        self.failed.clear();
        // #305：旧 rev 条目即刻回收（不等 LRU 驱逐——预算即时归位）。
        let _evicted = self.cache.invalidate_stale_revisions(self.annotate_rev);
        // 重建工人：工厂持新字节快照（原字节前缀 + 追加修订——增量
        // 产物结构有效，与打开路径同构）。
        if self.worker.is_some() {
            let factory_bytes = self.bytes.clone();
            self.worker = Some(RenderWorker::watchdog(
                move || {
                    PdfiumEngine::open((*factory_bytes).clone())
                        .expect("pdfium 看门狗工厂：批注后字节快照重开失败（增量产物结构有效）")
                },
                RENDER_WATCHDOG,
            ));
        }
    }
}

impl Focusable for PdfPagesView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PdfPagesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 派单（render 期直接改 self——uniform_list 回调期只读，无重入面）。
        self.submit_visible(cx);
        let colors = cx.theme().colors().clone();
        // error 态：明示面板（不白屏）。
        if self.error.is_some() || self.worker.is_none() {
            let message = self.error.clone().unwrap_or_else(|| "pdfium 未绑定".into());
            return v_flex()
                .id("pdf-pages-error")
                .size_full()
                .items_center()
                .justify_center()
                .bg(colors.background)
                .child(ui::Label::new(message).color(ui::Color::Warning))
                .child(
                    ui::Label::new("页面渲染不可用——文本视图仍可查看（状态栏切换）")
                        .size(ui::LabelSize::Small)
                        .color(ui::Color::Muted),
                )
                .into_any_element();
        }
        let entity = cx.entity().downgrade();
        let viewport_cell = self.viewport.clone();
        let page_count = self.page_count;
        let fit_w_hint = Self::fit_width(self.viewport_size.0.max(320.));
        let _slot_h = Self::slot_height(fit_w_hint, self.aspect);
        let annotate_error = self.annotate_error.clone();
        let annotate_hint = self.annotate_hint.clone();
        let list = gpui::uniform_list(
            "pdf-pages",
            page_count,
            move |visible: std::ops::Range<usize>, _window: &mut Window, cx: &mut App| {
                visible
                    .clone()
                    .filter_map(|idx| {
                        let slot = entity
                            .update(cx, |view, cx| view.render_page_slot(idx, cx))
                            .ok()?;
                        Some(annotate_overlay(slot, idx, entity.clone()))
                    })
                    .collect::<Vec<AnyElement>>()
            },
        )
        .track_scroll(&self.scroll)
        .h_full();
        // #302④：批注路径错误横幅（最近一次——不静默丢，用户可重试）。
        let error_banner = annotate_error.map(|message| {
            div()
                .id("pdf-annotate-error")
                .absolute()
                .top_4()
                .left_12()
                .right_12()
                .opacity(0.95)
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .bg(colors.panel_background)
                .px_3()
                .py_2()
                .child(
                    ui::Label::new(format!("批注失败：{message}"))
                        .size(ui::LabelSize::Small)
                        .color(ui::Color::Warning),
                )
        });
        // #302 验收 1：工具提示横幅（退化右键单击后——当前工具明示；
        // 底部与错误横幅分位，提交成功即清零）。
        let hint_banner = annotate_hint.map(|message| {
            div()
                .id("pdf-annotate-tool-hint")
                .absolute()
                .bottom_4()
                .left_12()
                .right_12()
                .opacity(0.95)
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .bg(colors.panel_background)
                .px_3()
                .py_2()
                .child(
                    ui::Label::new(message)
                        .size(ui::LabelSize::Small)
                        .color(ui::Color::Muted),
                )
        });
        div()
            .id("pdf-pages-root")
            .relative()
            .size_full()
            .min_w_0()
            .bg(colors.background)
            .flex()
            .flex_col()
            .items_center()
            .child(list)
            // #302④：批注错误横幅（在位时叠加顶部）。
            .children(error_banner)
            // #302 验收 1：工具提示横幅（在位时叠加底部）。
            .children(hint_banner)
            // 覆盖 canvas：透明、绝对铺满——paint 期回写视口尺寸（Cell
            // 共享，零实体更新零重入；不注册命中区，不挡列表滚轮）。
            .child(
                div()
                    .id("pdf-viewport-probe")
                    .absolute()
                    .size_full()
                    .child(gpui::canvas(
                        move |bounds, _window, _cx| bounds,
                        move |bounds: gpui::Bounds<gpui::Pixels>,
                              _t,
                              _window: &mut Window,
                              _cx: &mut App| {
                            viewport_cell.set(Some((
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                            )));
                        },
                    )),
            )
            .into_any_element()
    }
}

/// 光栅产物 → gpui 图像（BGRA 直通——`RenderedPage::bgra` 与
/// `RenderImage` 同为 BGRA 字节序，无通道交换；canvas_view 的 RGBA
/// 面才需要换 R↔B）。尺寸乘积不一致（防御）→ None 走 failed 面。
fn rendered_page_to_image(page: &RenderedPage) -> Option<gpui::RenderImage> {
    let expected = (page.width_px as usize) * (page.height_px as usize) * 4;
    if page.bgra.len() != expected || page.width_px == 0 || page.height_px == 0 {
        return None;
    }
    let buffer = ImageBuffer::from_raw(page.width_px, page.height_px, page.bgra.clone())?;
    Some(gpui::RenderImage::new(SmallVec::from_elem(
        Frame::new(buffer),
        1,
    )))
}

// ---------------------------------------------------------------------------
// #302④：右键拖框观察器（页槽叠加透明 canvas——paint 期窗口级注册）
// ---------------------------------------------------------------------------

/// 页槽包装：叠加透明 canvas，paint 期注册右键按下/松开观察器
/// （窗口级 `on_mouse_event` + 自身 bounds 过滤——与 shell 面板拖拽
/// 收尾钩同模式：每帧重注册，事件只落在自己 bounds 内才动作；
/// 无命中区不挡列表滚轮，右键不占左键交互）。
fn annotate_overlay(
    slot: AnyElement,
    idx: usize,
    entity: gpui::WeakEntity<PdfPagesView>,
) -> AnyElement {
    let page = idx as u32;
    div()
        .id(gpui::SharedString::from(format!("pdf-annot-overlay-{idx}")))
        .relative()
        .flex_none()
        .child(slot)
        .child(
            div()
                .id(gpui::SharedString::from(format!("pdf-annot-canvas-{idx}")))
                .absolute()
                .size_full()
                .child(gpui::canvas(
                    move |bounds, _window, _cx| bounds,
                    move |bounds: gpui::Bounds<gpui::Pixels>,
                          _t,
                          window: &mut Window,
                          _cx: &mut App| {
                        // 右键按下：记录页内像素起点。
                        let ent_down = entity.clone();
                        window.on_mouse_event(
                            move |event: &MouseDownEvent,
                                  phase: DispatchPhase,
                                  _w: &mut Window,
                                  cx: &mut App| {
                                if phase != DispatchPhase::Bubble
                                    || event.button != MouseButton::Right
                                {
                                    return;
                                }
                                let Some((x, y)) = local_point(event.position, bounds) else {
                                    return;
                                };
                                let _ = ent_down.update(cx, |view, _cx| {
                                    view.drag_start = Some((page, x, y));
                                });
                            },
                        );
                        // 右键松开：拖框收尾（跨页/退化单击在收尾面丢弃）。
                        let ent_up = entity.clone();
                        window.on_mouse_event(
                            move |event: &MouseUpEvent,
                                  phase: DispatchPhase,
                                  _w: &mut Window,
                                  cx: &mut App| {
                                if phase != DispatchPhase::Bubble
                                    || event.button != MouseButton::Right
                                {
                                    return;
                                }
                                let Some((x, y)) = local_point(event.position, bounds) else {
                                    return;
                                };
                                let _ = ent_up.update(cx, |view, cx| {
                                    view.finish_drag_annotation(page, x, y, cx);
                                });
                            },
                        );
                    },
                )),
        )
        .into_any_element()
}

/// 窗口坐标 → 元素内局部像素（bounds 外 → None）。
fn local_point(position: gpui::Point<Pixels>, bounds: gpui::Bounds<Pixels>) -> Option<(f32, f32)> {
    let in_x = position.x >= bounds.origin.x && position.x <= bounds.origin.x + bounds.size.width;
    let in_y = position.y >= bounds.origin.y && position.y <= bounds.origin.y + bounds.size.height;
    (in_x && in_y).then_some((
        f32::from(position.x - bounds.origin.x),
        f32::from(position.y - bounds.origin.y),
    ))
}

/// 光栅宽量子化 + 域钳制（纯函数——单测锚点）。
pub fn quantize_width(w: u32) -> u32 {
    let clamped = w.clamp(WIDTH_MIN, WIDTH_MAX);
    (clamped.div_ceil(WIDTH_QUANTUM) * WIDTH_QUANTUM).min(WIDTH_MAX)
}

// ─── 纯函数面单测（零引擎依赖——CI 无 libpdfium 同样真跑） ──────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn quantize_width_clamps_and_rounds_up() {
        assert_eq!(quantize_width(0), WIDTH_MIN, "下限兜底");
        assert_eq!(quantize_width(100), WIDTH_MIN);
        assert_eq!(quantize_width(300), 320, "向上取 64 倍数");
        assert_eq!(quantize_width(320), 320, "已是倍数不抖动");
        assert_eq!(quantize_width(321), 384);
        assert_eq!(quantize_width(100_000), WIDTH_MAX, "上限钳制");
        // 上限对齐：4096 本身是 64 倍数，钳后不变。
        assert_eq!(quantize_width(WIDTH_MAX), WIDTH_MAX);
    }

    #[test]
    fn slot_height_follows_aspect() {
        assert!((PdfPagesView::slot_height(800, 1.5) - 1200.).abs() < 1e-6);
        assert!((PdfPagesView::slot_height(640, 1.4142) - 905.088).abs() < 0.01);
    }

    #[test]
    fn fit_width_subtracts_margin_and_quantizes() {
        // 1024 视口 - 24 边距 = 1000 → 量子化 1024。
        assert_eq!(PdfPagesView::fit_width(1024.), 1024);
        // 极窄视口 → 下限。
        assert_eq!(PdfPagesView::fit_width(10.), WIDTH_MIN);
    }

    /// 尺寸/缓冲不一致 → None（failed 面，不 panic）。
    #[test]
    fn rendered_page_to_image_rejects_mismatched_buffer() {
        let bad = RenderedPage {
            width_px: 4,
            height_px: 4,
            bgra: vec![0u8; 15], // 应为 64 字节。
        };
        assert!(rendered_page_to_image(&bad).is_none());
    }

    /// 合法缓冲 → Some（尺寸承接到 RenderImage frame）。
    #[test]
    fn rendered_page_to_image_accepts_valid_buffer() {
        let good = RenderedPage {
            width_px: 2,
            height_px: 2,
            bgra: vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
        };
        assert!(rendered_page_to_image(&good).is_some());
    }

    // -----------------------------------------------------------------------
    // #386：批注落盘——手势级 e2e（从用户手势开始，不跳过视图层）
    // -----------------------------------------------------------------------
    // 与 tests/pdf_annotate_e2e.rs（#336 控制器面）的分工：那批证明
    // 「控制器**能**存盘」，本组证明「用户操作（右键拖框手势 → 视图
    // 脏标 → 保存）**最终调到**存盘」——#386 的验收缺口正是「视图 →
    // 文件」这一环。断言基线与 #336 同源：盘上前缀保持（增量非重写）
    // + mdc-pdf 读面独立第二意见（与写面无共享实现路径）。
    //
    // 环境门控（render_170_pdf 同口径）：`PdfPagesView::new` 的几何
    // 预读需 pdfium——无库环境 SKIP 横幅（不伪装通过），有则全量
    // 真跑。批注链本身纯 Rust（lopdf），但本组钉的是页视图在位的
    // 完整产品面。
    // -----------------------------------------------------------------------

    /// smoke.pdf 夹具字节（两页——与 #336 控制器测试同源夹具）。
    fn smoke_pdf_bytes() -> Vec<u8> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/smoke.pdf");
        std::fs::read(&p).unwrap_or_else(|e| panic!("夹具不可读（{}）：{e}", p.display()))
    }

    /// 临时落盘路径守卫（Drop 清理——panic 也清）。
    struct TempPdf(PathBuf);
    impl Drop for TempPdf {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// #386 核心闭环：手势 → 脏标 → 落盘 → 独立读回在档 → 脏标清零
    /// → 干净重存 no-op（验收 1/2/5/6 的视图层全链）。
    #[gpui::test]
    fn gesture_marks_dirty_and_save_roundtrips(cx: &mut gpui::TestAppContext) {
        let original = smoke_pdf_bytes();
        let entity = cx.update(|cx| cx.new(|cx| PdfPagesView::new(original.clone(), cx)));
        // 环境门控：构造期引擎失败（无 libpdfium）→ SKIP；其他失败 =
        // 夹具/真缺陷（panic，不放过）。
        if let Some(e) = cx.update(|cx| entity.read(cx).error.clone()) {
            if e.contains("pdfium") {
                eprintln!(
                    "\n══════ SKIP（环境缺件，不伪装通过）══════\n  \
                     本机无 libpdfium——#386 视图级手势 e2e 未真跑。\n  \
                     有 libpdfium 的环境（MDC_PDFIUM_LIB=<库路径> \
                     cargo test）全量执行。\n═══════════════════════════\n"
                );
                return;
            }
            panic!("页视图构造失败（夹具/真缺陷，非环境缺件）：{e}");
        }
        // ① 手势（生产入口 = finish_drag_annotation 收尾同款
        //    commit_annotation；视口未记录 → fit_w=WIDTH_MIN=256，
        //    屏幕坐标按 256px 宽视图取页 0 上部——同 #336 的 96DPI
        //    量级意图）。
        cx.update(|cx| {
            entity.update(cx, |v, _| {
                v.commit_annotation(0, 40., 60., 200., 120.)
                    .expect("#386 起点：用户手势提交");
            })
        });
        // ② 视图脏标（未落盘前必真——Shell 据此驱动关闭确认）。
        assert!(
            cx.update(|cx| entity.read(cx).is_dirty()),
            "#386 验收：手势后视图必脏"
        );
        // ③ 保存（Shell 传入路径——单一真源；此处直驱同款调用面）。
        let file = TempPdf(
            std::env::temp_dir().join(format!("mang-386-gesture-{}.pdf", std::process::id())),
        );
        let stats = cx
            .update(|cx| entity.update(cx, |v, _| v.save_annotations(&file.0)))
            .expect("落盘应成功")
            .expect("脏视图保存 = Ok(Some)");
        // 手势内 commit_annotation 已 flush（产出修订段）——save 的
        // flush 零脏幂等（零追加，不产空修订——#336 语义锁同源）。
        assert_eq!(
            stats.bytes_appended, 0,
            "手势已 flush → save 零脏幂等：{stats:?}"
        );
        // ④ 盘上字节真变了（≠原文件）且保持原前缀（增量非重写——
        // 验收 2 机制面，全量重写在此必红）。
        let on_disk = std::fs::read(&file.0).expect("盘上文件");
        assert!(
            on_disk.starts_with(&original),
            "#386 验收 2：盘上保持原字节前缀（增量）"
        );
        assert!(
            on_disk.len() > original.len(),
            "批注修订段非空（得 {} → {}）",
            original.len(),
            on_disk.len()
        );
        // ⑤ 独立第二意见（mdc-pdf 读面）：1 条高亮在档 + 页号往返。
        let (annots, report) = mdc_pdf::annotate::read_annotations(
            on_disk.clone(),
            mdc_pdf::security::Limits::default(),
        )
        .expect("独立读回");
        assert_eq!(report.parsed, 1, "重开应见 1 条批注：{report:?}");
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].page.0, 0, "页号往返");
        // ⑥ 脏标清零（写盘成功才清——原子语义）。
        assert!(
            !cx.update(|cx| entity.read(cx).is_dirty()),
            "保存成功后脏标必清"
        );
        // ⑦ 干净重存 = no-op（Ok(None)，盘上不动）。
        let noop = cx
            .update(|cx| entity.update(cx, |v, _| v.save_annotations(&file.0)))
            .expect("干净保存不报错");
        assert!(noop.is_none(), "干净视图保存 = no-op");
        assert_eq!(
            std::fs::read(&file.0).unwrap(),
            on_disk,
            "no-op 不动盘上字节"
        );
    }

    /// #386 连续会话：保存后再画 → 再脏 → 再存 → 修订累积（两次手势
    /// 两条批注在档——「保存后继续工作」的常见流不丢不重置）。
    #[gpui::test]
    fn second_gesture_after_save_accumulates(cx: &mut gpui::TestAppContext) {
        let original = smoke_pdf_bytes();
        let entity = cx.update(|cx| cx.new(|cx| PdfPagesView::new(original.clone(), cx)));
        if let Some(e) = cx.update(|cx| entity.read(cx).error.clone()) {
            if e.contains("pdfium") {
                eprintln!(
                    "\n══════ SKIP（环境缺件，不伪装通过）══════\n  \
                     本机无 libpdfium——#386 视图级手势 e2e 未真跑。\n═════\
                     ════════════════════════════\n"
                );
                return;
            }
            panic!("页视图构造失败（夹具/真缺陷，非环境缺件）：{e}");
        }
        let file = TempPdf(
            std::env::temp_dir().join(format!("mang-386-accum-{}.pdf", std::process::id())),
        );
        // 第一笔：手势 → 落盘。
        cx.update(|cx| {
            entity.update(cx, |v, _| {
                v.commit_annotation(0, 40., 60., 200., 120.)
                    .expect("手势一");
            })
        });
        cx.update(|cx| entity.update(cx, |v, _| v.save_annotations(&file.0)))
            .expect("落盘一")
            .expect("脏视图必写");
        let after_first = std::fs::read(&file.0).unwrap();
        // 第二笔：再手势（保存后的新改动）→ 再脏 → 再落盘。
        cx.update(|cx| {
            entity.update(cx, |v, _| {
                v.commit_annotation(0, 60., 200., 220., 260.)
                    .expect("手势二");
            })
        });
        assert!(
            cx.update(|cx| entity.read(cx).is_dirty()),
            "保存后再画必再脏"
        );
        cx.update(|cx| entity.update(cx, |v, _| v.save_annotations(&file.0)))
            .expect("落盘二")
            .expect("脏视图必写");
        let after_second = std::fs::read(&file.0).unwrap();
        // 增量链：二跳仍以一跳为前缀（连续修订不重写全文）。
        assert!(
            after_second.starts_with(&after_first),
            "第二跳以第一跳产物为前缀（增量链）"
        );
        assert!(after_second.len() > after_first.len());
        // 独立读回：两条批注都在档（不重置不丢）。
        let (annots, report) =
            mdc_pdf::annotate::read_annotations(after_second, mdc_pdf::security::Limits::default())
                .expect("独立读回");
        assert_eq!(report.parsed, 2, "两笔手势 = 两条批注：{report:?}");
        assert_eq!(annots.len(), 2);
        assert!(
            !cx.update(|cx| entity.read(cx).is_dirty()),
            "二次保存后脏标清零"
        );
    }

    /// #386 干净视图：从未画过 → 保存是显式 no-op（Ok(None)）且**不
    /// 创建文件**（干净路径零写盘——「没东西可存」不许产生空动作痕
    /// 迹）。
    #[gpui::test]
    fn clean_view_save_is_explicit_noop(cx: &mut gpui::TestAppContext) {
        let _original = smoke_pdf_bytes();
        let entity = cx.update(|cx| cx.new(|cx| PdfPagesView::new(_original.clone(), cx)));
        if let Some(e) = cx.update(|cx| entity.read(cx).error.clone()) {
            if e.contains("pdfium") {
                eprintln!(
                    "\n══════ SKIP（环境缺件，不伪装通过）══════\n  \
                     本机无 libpdfium——#386 视图级手势 e2e 未真跑。\n═════\
                     ════════════════════════════\n"
                );
                return;
            }
            panic!("页视图构造失败（夹具/真缺陷，非环境缺件）：{e}");
        }
        let file =
            TempPdf(std::env::temp_dir().join(format!("mang-386-noop-{}.pdf", std::process::id())));
        let outcome = cx.update(|cx| entity.update(cx, |v, _| v.save_annotations(&file.0)));
        assert!(
            matches!(&outcome, Ok(None)),
            "干净视图保存 = Ok(None) no-op（得 {outcome:?}）"
        );
        assert!(!file.0.exists(), "no-op 不得创建文件（零写盘）");
        assert!(
            !cx.update(|cx| entity.read(cx).is_dirty()),
            "干净视图恒不脏"
        );
    }
}
