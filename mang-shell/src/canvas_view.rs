//! canvas 画布视图（#133 任务 1：canvas GPUI 集成——引擎有，UI 接上）。
//!
//! ## 路线裁定：A → A′（svg() 元素是单色蒙版通道，源码有据否决原案）
//!
//! issue #133 原案路线 A「把 export_svg 的产物交给 GPUI 的 SVG 渲染器」
//! 在源码复核下不成立：
//!
//! - `Window::paint_svg` 文档即「Paint a **monochrome** SVG into the
//!   scene」（third_party/zed/crates/gpui/src/window.rs）；
//! - 底层 `SvgRenderer::render_alpha_mask` 把光栅结果逐像素压成 alpha
//!   蒙版（svg_renderer.rs：`p.pixels().iter().map(|p| p.alpha())`），
//!   再按 `text_color` 单色着色——多色画布会被渲染成单色剪影。
//!
//! 改走 **A′**（仍属「预渲染位面 + 视口变换」的静态路线，不碰 B 路线
//! 的 PathBuilder 交互面）：
//!
//! ```text
//! .canvas 文件（CanvasSession 的 yrs 权威快照，二进制）
//!   → CanvasSession::restore
//!   → CanvasRendering::from_session（对象 → 图元）
//!   → RasterRenderer（引擎自带光栅器，视口由本模块几何参数驱动）
//!   → Bitmap（RGBA8 裸面）→ R↔B 通道交换（gpui RenderImage 是 BGRA
//!     字节序——先例 elements/img.rs「Convert from RGBA to BGRA」）
//!   → gpui::RenderImage → Window::paint_image（全彩位面 + 绘制期
//!     四边形定位，接线在 editor::render_canvas_area）
//! ```
//!
//! ## 视口（缩放/平移）
//!
//! - `zoom=1、pan=(0,0)`：内容包围盒等比适配视图并居中；
//! - 平移只挪绘制四边形——**不重光栅**（缓存键不含 pan）；
//! - 缩放按需重光栅：paint 期惰性重建，缓存键 =（内容版本 rev，光栅
//!   缩放，位面尺寸）；上限护栏 4096 边 / 8M 像素（超出等比降采样交
//!   GPU 放大——极端缩放略糊，v1 接受并登记）；
//! - 交互（接线在 editor 层）：滚轮平移 / Ctrl+滚轮缩放 / 左键拖拽
//!   平移 / Ctrl+= Ctrl+- Ctrl+0 / 方向键·PageUp·PageDown。
//!
//! ## 已知边界（v1 登记）
//!
//! - 显示面只读：会话命令通路（execute/undo/redo）已备，编辑 UI 未接
//!   （B 路线交互面是后续任务）；
//! - 缩放围绕视图中心（非指针锚点）；
//! - 命令面板/查找/跳行在 canvas 态不接（键域整段归视口命令）。

use std::cell::{Cell, RefCell};
#[cfg(test)]
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use gpui::{Pixels, RenderImage, Size};
use image::{Frame, ImageBuffer};
use mdc_canvas::ObjectKind;
use mdc_canvas::render::{
    CanvasRenderer, CanvasRendering, RasterRenderer, Rect, canvas_bounds_geometry,
};
use mdc_canvas::session::{Actor, CanvasCommand, CanvasSession, SessionOptions};
use smallvec::SmallVec;

/// 缩放域（相对「适配视图」基准的倍率）。
pub const ZOOM_MIN: f32 = 0.1;
pub const ZOOM_MAX: f32 = 8.0;
/// 键盘缩放步进倍率（Ctrl+= / Ctrl+-）。
pub const ZOOM_STEP: f32 = 1.25;
/// 滚轮缩放：每「行」步进倍率（Ctrl+滚轮）。
const ZOOM_WHEEL_STEP: f32 = 1.1;
/// 滚轮「行 → 像素」换算（gpui 惯例约定；editor 缩放档位共用）。
pub const WHEEL_LINE_PX: f32 = 40.0;
/// 方向键平移步长（逻辑 px）。
pub const PAN_KEY_PX: f32 = 60.0;
/// PageUp/PageDown 平移步长（逻辑 px）。
pub const PAN_PAGE_PX: f32 = 400.0;
/// 光栅位面护栏：单边上限 / 总像素上限（纹理安全 + 内存护栏）。
const RASTER_MAX_SIDE: u32 = 4096;
const RASTER_MAX_PIXELS: u64 = 8 * 1024 * 1024;

/// 画布视口：`zoom=1、pan=(0,0)` = 内容包围盒等比适配视图并居中。
///
/// pan 是**屏幕逻辑 px**（缩放后的坐标系——拖拽 1:1 跟手）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

/// 一帧的绘制几何（内容四边形在视图元素内的逻辑 px 参数）。
///
/// 屏幕 → 画布映射：`canvas = (screen - (x, y)) / scale`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameGeom {
    /// zoom=1 时的基准缩放（px / 画布单位——由视图尺寸适配内容而来）。
    pub fit: f64,
    /// 有效缩放 = fit × zoom。
    pub scale: f64,
    /// 内容四边形左上角（视图元素内逻辑 px，含居中 + pan）。
    pub x: f64,
    pub y: f64,
    /// 内容四边形尺寸（逻辑 px）。
    pub w: f64,
    pub h: f64,
}

/// canvas 画布视图态（Editor 载荷——纯数据 + 光栅缓存，非独立实体；
/// 行模型不承载自由 2D 画布，渲染分派见 editor::render_canvas_area）。
pub struct CanvasView {
    session: CanvasSession,
    /// 渲染模型（与 rev 对应）。
    rendering: CanvasRendering,
    /// 内容几何包围盒（渲染模型派生缓存）。
    content: Rect,
    viewport: Viewport,
    /// 内容语义版本（execute/undo/redo → +1 → 光栅缓存失效）。
    rev: u64,
    /// 光栅缓存（paint 期惰性重建；RefCell——paint 闭包经只读实体句柄
    /// 触达，循 editor 行 bounds 记录的内部可变先例）。
    raster: RefCell<Option<RasterEntry>>,
    /// 光栅重建计数（缓存行为单测断言面）。
    builds: Cell<u32>,
}

struct RasterEntry {
    rev: u64,
    /// 光栅缩放的位键（f64 to_bits——缓存键组成部分）。
    scale_bits: u64,
    width: u32,
    height: u32,
    image: Arc<RenderImage>,
}

impl CanvasView {
    /// 从 `.canvas` 文件装载（yrs 权威快照；空文件 = 空会话——新文件
    /// 语义。输入字节数门在引擎 restore 层，52MB 同 mdc-collab 口径）。
    pub fn load(path: &Path) -> Result<CanvasView, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let session = if bytes.is_empty() {
            CanvasSession::open(SessionOptions::default()).map_err(|e| e.to_string())?
        } else {
            CanvasSession::restore(SessionOptions::default(), &bytes).map_err(|e| e.to_string())?
        };
        Ok(CanvasView::from_session(session))
    }

    fn from_session(session: CanvasSession) -> CanvasView {
        let rendering = CanvasRendering::from_session(&session);
        let content = canvas_bounds_geometry(&rendering);
        CanvasView {
            session,
            rendering,
            content,
            viewport: Viewport::default(),
            rev: 0,
            raster: RefCell::new(None),
            builds: Cell::new(0),
        }
    }

    /// 视口（测试断言面 + 后续交互/RPC 面预留——bin 面暂无直接消费）。
    #[allow(dead_code)]
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn shape_count(&self) -> usize {
        self.rendering.shapes.len()
    }

    /// 对象计数（按种类分桶）。
    pub fn object_counts(&self) -> (usize, usize, usize, usize) {
        let objects = self.session.objects();
        let nodes = objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Node)
            .count();
        let edges = objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Edge)
            .count();
        let groups = objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Group)
            .count();
        (objects.len(), nodes, edges, groups)
    }

    /// 内容包围盒（测试断言面）。引擎几何坐标（非屏幕像素）。
    #[allow(dead_code)]
    pub fn content_bounds(&self) -> Rect {
        self.content
    }

    /// 光栅重建计数（缓存行为断言面：平移不重建、缩放/内容变重建）。
    #[allow(dead_code)]
    pub fn raster_builds(&self) -> u32 {
        self.builds.get()
    }

    // ── 会话命令通路（v1 无编辑 UI——测试面在用，后续 RPC/交互面的
    // 模型面；bin 面暂无直接消费，allow(dead_code) 登记）──

    /// 受控命令执行（人类与 AI 同一入口——引擎语义）。
    #[allow(dead_code)]
    pub fn execute(&mut self, cmd: CanvasCommand) -> Result<(), String> {
        self.session
            .execute(Actor::Human, cmd)
            .map_err(|e| e.to_string())?;
        self.regenerate();
        Ok(())
    }

    #[allow(dead_code)]
    pub fn undo(&mut self) -> bool {
        let done = self.session.undo(Actor::Human);
        if done {
            self.regenerate();
        }
        done
    }

    #[allow(dead_code)]
    pub fn redo(&mut self) -> bool {
        let done = self.session.redo(Actor::Human);
        if done {
            self.regenerate();
        }
        done
    }

    #[allow(dead_code)]
    fn regenerate(&mut self) {
        self.rendering = CanvasRendering::from_session(&self.session);
        self.content = canvas_bounds_geometry(&self.rendering);
        self.rev += 1;
    }

    /// 会话权威快照（`.canvas` 落盘字节——保存链用）。
    #[allow(dead_code)]
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        self.session
            .authoritative_snapshot()
            .map_err(|e| e.to_string())
    }

    // ── 视口操作 ─────────────────────────────────────────────

    pub fn zoom_in(&mut self) {
        self.set_zoom(self.viewport.zoom * ZOOM_STEP);
    }

    pub fn zoom_out(&mut self) {
        self.set_zoom(self.viewport.zoom / ZOOM_STEP);
    }

    /// 滚轮缩放（steps 正 = 放大；每行 ZOOM_WHEEL_STEP 倍）。
    pub fn zoom_wheel(&mut self, steps: f32) {
        let factor = ZOOM_WHEEL_STEP.powf(steps);
        self.set_zoom(self.viewport.zoom * factor);
    }

    pub fn reset_viewport(&mut self) {
        self.viewport = Viewport::default();
    }

    /// 平移（屏幕逻辑 px；正 y = 内容下移）。
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.viewport.pan_x += dx;
        self.viewport.pan_y += dy;
    }

    fn set_zoom(&mut self, zoom: f32) {
        self.viewport.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    }

    // ── 几何与光栅 ───────────────────────────────────────────

    /// 一帧几何：内容四边形在视图元素内的位置与尺寸（逻辑 px）。
    pub fn frame_geom(&self, view: Size<Pixels>) -> FrameGeom {
        let vw = f64::from(view.width);
        let vh = f64::from(view.height);
        let b = self.content;
        let fit = if b.w > 0.0 && b.h > 0.0 {
            (vw / b.w).min(vh / b.h)
        } else {
            1.0
        };
        let scale = fit * self.viewport.zoom as f64;
        let w = b.w * scale;
        let h = b.h * scale;
        FrameGeom {
            fit,
            scale,
            x: (vw - w) / 2.0 + self.viewport.pan_x as f64,
            y: (vh - h) / 2.0 + self.viewport.pan_y as f64,
            w,
            h,
        }
    }

    /// 取当前视口的位面（缓存命中直取；未命中在调用点光栅化）。
    ///
    /// 返回 `(RenderImage, FrameGeom)`：image 以 geom 四边形绘制
    /// （`Window::paint_image`——全幅贴到 image_bounds）。空画布 /
    /// 零尺寸 → None（调用方走空态提示）。
    pub fn raster_for(
        &self,
        view: Size<Pixels>,
        dpr: f32,
    ) -> Option<(Arc<RenderImage>, FrameGeom)> {
        if self.rendering.shapes.is_empty() {
            return None;
        }
        let geom = self.frame_geom(view);
        if !(geom.w > 0.0 && geom.h > 0.0) {
            return None;
        }
        let (bw, bh, raster_scale) = self.raster_size(geom, dpr);
        let key = raster_scale.to_bits();
        {
            let cache = self.raster.borrow();
            if let Some(entry) = cache.as_ref()
                && entry.rev == self.rev
                && entry.scale_bits == key
                && entry.width == bw
                && entry.height == bh
            {
                return Some((entry.image.clone(), geom));
            }
        }
        let rgba = self.rasterize(bw, bh, raster_scale)?;
        self.builds.set(self.builds.get() + 1);
        let buffer = ImageBuffer::from_raw(bw, bh, rgba_to_bgra(rgba))?;
        let image = Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buffer), 1)));
        *self.raster.borrow_mut() = Some(RasterEntry {
            rev: self.rev,
            scale_bits: key,
            width: bw,
            height: bh,
            image: image.clone(),
        });
        Some((image, geom))
    }

    /// 光栅位面尺寸与光栅缩放（护栏钳制：单边 ≤4096、总像素 ≤8M）。
    fn raster_size(&self, geom: FrameGeom, dpr: f32) -> (u32, u32, f64) {
        let mut scale = geom.scale * dpr as f64;
        let mut w = ((self.content.w * scale).ceil() as u32).max(1);
        let mut h = ((self.content.h * scale).ceil() as u32).max(1);
        let side_over = w.max(h) > RASTER_MAX_SIDE;
        let px_over = w as u64 * h as u64 > RASTER_MAX_PIXELS;
        if side_over || px_over {
            let mut k = 1.0f64;
            if w.max(h) as f64 * k > RASTER_MAX_SIDE as f64 {
                k = RASTER_MAX_SIDE as f64 / w.max(h) as f64;
            }
            let pixels = w as f64 * h as f64 * k * k;
            if pixels > RASTER_MAX_PIXELS as f64 {
                k *= (RASTER_MAX_PIXELS as f64 / pixels).sqrt();
            }
            w = ((w as f64 * k).ceil() as u32).max(1);
            h = ((h as f64 * k).ceil() as u32).max(1);
            scale = (w as f64 / self.content.w.max(1e-9)).min(h as f64 / self.content.h.max(1e-9));
        }
        (w, h, scale)
    }

    /// 光栅化（引擎 RasterRenderer；返回 RGBA8 裸面——引擎字节序）。
    fn rasterize(&self, w: u32, h: u32, scale: f64) -> Option<Vec<u8>> {
        let mut renderer =
            RasterRenderer::new((w, h), self.content, scale, self.rendering.background);
        for (shape, tf) in &self.rendering.shapes {
            renderer.draw(shape, tf);
        }
        let bmp = renderer.bitmap();
        if bmp.pixels.len() == (w as usize * h as usize * 4) {
            Some(bmp.pixels.clone())
        } else {
            None
        }
    }

    // ── 证据行 ───────────────────────────────────────────────

    /// 打开证据行（document.rs open_editor——循 docx/pdf/xlsx 口径）。
    pub fn open_evidence(&self, path: &Path) -> String {
        let (total, nodes, edges, groups) = self.object_counts();
        format!(
            "canvas: {} 对象（{nodes} 节点/{edges} 连线/{groups} 分组）→ {} 图元 / 内容 {:.0}×{:.0} / 视口 zoom={:.2} pan=({:.0},{:.0})（只读，{}）",
            total,
            self.rendering.shapes.len(),
            self.content.w,
            self.content.h,
            self.viewport.zoom,
            self.viewport.pan_x,
            self.viewport.pan_y,
            path.display()
        )
    }

    /// 烟雾探针行（`--smoke-canvas` 断言通道：图形数量 + 视口参数）。
    pub fn smoke_probe_lines(&self) -> Vec<String> {
        let (total, nodes, edges, groups) = self.object_counts();
        vec![
            format!(
                "canvas-viewport: zoom={:.2} pan=({:.0},{:.0})",
                self.viewport.zoom, self.viewport.pan_x, self.viewport.pan_y
            ),
            format!(
                "canvas-objects: {total} 对象（{nodes} 节点/{edges} 连线/{groups} 分组）→ {} 图元",
                self.rendering.shapes.len()
            ),
        ]
    }
}

/// RGBA → BGRA 通道交换（gpui `RenderImage` 是 BGRA 字节序——先例
/// elements/img.rs「Convert from RGBA to BGRA」`pixel.swap(0, 2)`）。
fn rgba_to_bgra(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    rgba
}

// ---------------------------------------------------------------------------
// 夹具（真实会话命令链 → yrs 权威快照；进仓纪律：缺失即 FAIL 不是 SKIP）
// ---------------------------------------------------------------------------

#[cfg(test)]
fn fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/smoke.canvas")
}

#[cfg(test)]
fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// 构建烟雾会话：3 矩形节点（三种填色）+ 3 文本节点（标签）+ 2 连线 +
/// 1 分组——真实命令链（与人类/AI 操作同一入口）。
#[cfg(test)]
fn smoke_session() -> CanvasSession {
    let mut s = CanvasSession::open(SessionOptions::default()).expect("开会话");
    for (id, x, fill) in [
        ("n1", 40.0, "#4f8ef7"),
        ("n2", 300.0, "#f7b84f"),
        ("n3", 560.0, "#67c23a"),
    ] {
        s.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some(id.to_string()),
                x,
                y: 40.0,
                w: 180.0,
                h: 70.0,
                props: props(&[("fill", fill)]),
            },
        )
        .expect("加节点");
    }
    for (id, x, label) in [
        ("t1", 100.0, "输入"),
        ("t2", 360.0, "处理"),
        ("t3", 620.0, "输出"),
    ] {
        s.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some(id.to_string()),
                x,
                y: 82.0,
                w: 0.0,
                h: 0.0,
                props: props(&[("shape", "text"), ("label", label), ("font_size", "16")]),
            },
        )
        .expect("加文本");
    }
    for (id, from, to) in [("e1", "n1", "n2"), ("e2", "n2", "n3")] {
        s.execute(
            Actor::Human,
            CanvasCommand::AddEdge {
                id: Some(id.to_string()),
                from: from.to_string(),
                to: to.to_string(),
                props: props(&[("stroke", "#333333"), ("stroke_width", "2")]),
            },
        )
        .expect("加连线");
    }
    s.execute(
        Actor::Human,
        CanvasCommand::GroupObjects {
            group_id: Some("g1".to_string()),
            members: vec!["n1".to_string(), "n2".to_string(), "n3".to_string()],
        },
    )
    .expect("分组");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    fn load_fixture() -> CanvasView {
        CanvasView::load(&fixture_path()).unwrap_or_else(|e| {
            panic!(
                "夹具 smoke.canvas 缺失或不可读：{e}\n用 `cargo test gen_smoke_canvas -- --ignored` 生成；夹具必须进仓。"
            )
        })
    }

    /// 生成/刷新夹具（ignored——显式跑）：
    /// `cargo test gen_smoke_canvas -- --ignored`
    #[test]
    #[ignore]
    fn gen_smoke_canvas() {
        let bytes = smoke_session().authoritative_snapshot().expect("权威快照");
        std::fs::write(fixture_path(), &bytes).expect("写夹具");
        eprintln!("smoke.canvas: {} 字节", bytes.len());
    }

    /// 夹具必须存在且非空壳（进仓纪律——防「删了夹具测试还绿」）。
    #[test]
    fn fixture_is_present_and_non_trivial() {
        let bytes = std::fs::read(fixture_path()).expect("夹具必须进仓");
        assert!(bytes.len() > 64, "快照太小，疑似截断：{}B", bytes.len());
    }

    /// 真实会话数据装载：对象/图元计数与视口初值（9 对象 → 8 图元：
    /// 3 矩形 + 3 文本 + 2 连线；分组不画）。
    #[test]
    fn load_fixture_counts_and_viewport() {
        let view = load_fixture();
        let (total, nodes, edges, groups) = view.object_counts();
        assert_eq!((total, nodes, edges, groups), (9, 6, 2, 1));
        assert_eq!(view.shape_count(), 8, "分组对象不画，成员各自成图元");
        assert_eq!(view.viewport(), Viewport::default(), "初值 = 适配居中");
        let b = view.content_bounds();
        assert!(
            b.w > 600.0 && b.h > 30.0,
            "内容包围盒应覆盖 3 节点布局：{b:?}"
        );
    }

    /// 位面是**全彩**的：三种节点填色必须以引擎 RGBA 字节序出现
    /// （这是 svg() 单色蒙版通道给不了的性质——路线 A′ 的存在理由）。
    #[test]
    fn raster_is_full_color() {
        let view = load_fixture();
        let geom = view.frame_geom(size(px(1280.), px(800.)));
        let (w, h, scale) = view.raster_size(geom, 1.0);
        let rgba = view.rasterize(w, h, scale).expect("光栅化");
        let count = |needle: [u8; 4]| {
            rgba.chunks_exact(4)
                .filter(|p| *p == needle.as_slice())
                .count()
        };
        // 填色（引擎 RGBA 字节序）：#4f8ef7 / #f7b84f / #67c23a。
        let blue = count([0x4f, 0x8e, 0xf7, 0xff]);
        let orange = count([0xf7, 0xb8, 0x4f, 0xff]);
        let green = count([0x67, 0xc2, 0x3a, 0xff]);
        assert!(blue > 500, "蓝节点填色像素过少：{blue}");
        assert!(orange > 500, "橙节点填色像素过少：{orange}");
        assert!(green > 500, "绿节点填色像素过少：{green}");
    }

    /// 字节序契约：位面进 gpui 前必须 R↔B 交换（RenderImage = BGRA）。
    #[test]
    fn rgba_to_bgra_swaps_channels() {
        assert_eq!(
            rgba_to_bgra(vec![0x4f, 0x8e, 0xf7, 0xff]),
            vec![0xf7, 0x8e, 0x4f, 0xff]
        );
    }

    /// 视口操作 + 缓存行为：平移不重光栅；缩放/内容变更才重建。
    #[test]
    fn viewport_ops_and_raster_cache() {
        let mut view = load_fixture();
        let vs = size(px(1280.), px(800.));
        assert!(view.raster_for(vs, 1.0).is_some(), "首帧光栅");
        assert_eq!(view.raster_builds(), 1, "首帧重建一次");
        // 平移：只挪四边形，缓存命中。
        view.pan(120.0, -80.0);
        assert_eq!(view.viewport().pan_x, 120.0);
        let (_, g1) = view.raster_for(vs, 1.0).unwrap();
        assert_eq!(view.raster_builds(), 1, "平移不重光栅");
        assert_eq!(g1.x, g1.x, "geom 存在");
        // 缩放：缓存失效重建。
        view.zoom_in();
        assert!((view.viewport().zoom - 1.25).abs() < 1e-3, "Ctrl+= ×1.25");
        assert!(view.raster_for(vs, 1.0).is_some());
        assert_eq!(view.raster_builds(), 2, "缩放重光栅");
        // 域钳制。
        view.reset_viewport();
        assert_eq!(view.viewport(), Viewport::default(), "Ctrl+0 重置");
        // 内容变更（命令通路）：rev+1 → 重光栅。
        view.execute(CanvasCommand::MoveNode {
            id: "n1".to_string(),
            x: 60.0,
            y: 60.0,
        })
        .expect("移动节点");
        assert!(view.raster_for(vs, 1.0).is_some());
        assert_eq!(view.raster_builds(), 3, "内容变更重光栅");
        // 撤销恢复几何 + rev 再进。
        assert!(view.undo());
        assert!(view.raster_for(vs, 1.0).is_some());
        assert_eq!(view.raster_builds(), 4, "撤销重光栅");
    }

    /// 会话快照全环：装载 → 命令 → 快照 → 再装载（`.canvas` 文件语义）。
    #[test]
    fn snapshot_roundtrip_via_view() {
        let mut view = load_fixture();
        let before = view.object_counts().0;
        view.execute(CanvasCommand::AddNode {
            id: Some("n9".to_string()),
            x: 900.0,
            y: 200.0,
            w: 120.0,
            h: 60.0,
            props: props(&[("fill", "#cc66cc")]),
        })
        .expect("加节点");
        let bytes = view.snapshot_bytes().expect("快照");
        let dir = std::env::temp_dir().join("mang-canvas-roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.canvas");
        std::fs::write(&path, &bytes).unwrap();
        let reloaded = CanvasView::load(&path).expect("重装载");
        assert_eq!(reloaded.object_counts().0, before + 1, "快照往返对象数");
        assert_eq!(reloaded.shape_count(), view.shape_count());
        let _ = std::fs::remove_file(&path);
    }

    /// 空文件 = 空会话（新文件语义）；空画布不出位面。
    #[test]
    fn empty_file_is_empty_session() {
        let dir = std::env::temp_dir().join("mang-canvas-roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.canvas");
        std::fs::write(&path, b"").unwrap();
        let view = CanvasView::load(&path).expect("空文件可开");
        assert_eq!(view.object_counts().0, 0);
        assert_eq!(view.shape_count(), 0);
        assert!(view.raster_for(size(px(1280.), px(800.)), 1.0).is_none());
        let _ = std::fs::remove_file(&path);
    }

    /// 破损快照显式报错（不静默白屏）。
    #[test]
    fn corrupt_snapshot_errors() {
        let dir = std::env::temp_dir().join("mang-canvas-roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.canvas");
        std::fs::write(&path, b"not-a-yrs-snapshot").unwrap();
        assert!(CanvasView::load(&path).is_err(), "非法快照必须报错");
        let _ = std::fs::remove_file(&path);
    }
}
