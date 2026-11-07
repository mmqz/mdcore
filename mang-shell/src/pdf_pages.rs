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
//!      │  PageCache（(页, 宽) → RenderImage，LRU 字节预算）
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
//!   不至于页页重光栅）。
//!
//! ## 已知边界（v1 登记）
//!
//! - **页尺寸不均的档**：uniform_list 等高槽——异形页按首页纵横比
//!   定槽高，真图内接呈现（`ObjectFit::Contain` 缺省），不裁不拉；
//! - **页面视图下无文本查找**：Ctrl+F 自动切文本视图（提示横幅说明）
//!   ——跨视图查找/选区属后续轮；
//! - **无缩放**：适宽单档（Ctrl+= 字号缩放对 pdf 页不适用——页面
//!   缩放属后续轮）；
//! - 扫描件（无文本层）在页面视图下**正是正确呈现**（位图就是内容）
//!   ——文本视图的「OCR 不支持」提示不再挡路。
//!
//! ## 引擎绑定纪律
//!
//! 与 [`crate::editor::pdf_doc`] 同源：libpdfium.so 定位序
//! `MDC_PDFIUM_LIB` → 编译期候选 → 系统；`PdfiumEngine::open` 触发
//! 进程级单绑定。无 libpdfium 环境（CI）下 `PdfPagesView::new` 走
//! `error` 态（面板明示「pdfium 未绑定」）——不崩不白屏。

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, FocusHandle, Focusable, ImageSource, Pixels, UniformListScrollHandle,
    Window, px,
};
use image::{Frame, ImageBuffer};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::engine_pdfium::PdfiumEngine;
use mdc_pdf::page::PageIndex;
use mdc_pdf::render::{RenderRequest, RenderedPage};
use mdc_pdf::worker::{RenderWorker, WorkerReply};
use smallvec::SmallVec;
use ui::prelude::*;

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

/// 缓存键：（0 基页号, 量子化光栅宽）。
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
    /// 页面缓存（LRU 字节预算）。
    cache: mdc_pdf::cache::PageCache<PageKey, Arc<gpui::RenderImage>>,
    /// 在途作业：job_id → 缓存键（回执对账 + 失败不再无限重提）。
    pending: HashMap<u64, PageKey>,
    /// 失败页（Done(Err)/Timeout 一次性登记——不无限重试敌意页）。
    failed: HashSet<PageKey>,
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
            if self.cache.contains(&key)
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
                                let _evicted = self.cache.put(key, Arc::new(image), cost);
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
    /// 渲染中 / 失败两态）。
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
        let image = self.cache.get(&key).cloned();
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
        let list = gpui::uniform_list(
            "pdf-pages",
            page_count,
            move |visible: std::ops::Range<usize>, _window: &mut Window, cx: &mut App| {
                visible
                    .clone()
                    .filter_map(|idx| {
                        entity
                            .update(cx, |view, cx| view.render_page_slot(idx, cx))
                            .ok()
                    })
                    .collect::<Vec<AnyElement>>()
            },
        )
        .track_scroll(&self.scroll)
        .h_full();
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

/// 光栅宽量子化 + 域钳制（纯函数——单测锚点）。
pub fn quantize_width(w: u32) -> u32 {
    let clamped = w.clamp(WIDTH_MIN, WIDTH_MAX);
    (clamped.div_ceil(WIDTH_QUANTUM) * WIDTH_QUANTUM).min(WIDTH_MAX)
}

// ─── 纯函数面单测（零引擎依赖——CI 无 libpdfium 同样真跑） ──────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
