//! 页变换——设备↔用户空间变换（#304：旋转页/CropBox 文档上标注落点
//! 偏移的修复基座；常开模块：纯数学零引擎依赖，mock/单测可测）。
//!
//! # 为什么需要它（接 UI 的前置条件）
//!
//! 标注落点流程：用户在屏幕上点击（设备像素）→ 转 PDF 用户空间点 →
//! 写进 QuadPoints。但两个坐标系之间隔着四个变换，**少任何一个标注
//! 都落错位置**：
//!
//! ```text
//! ① CropBox vs MediaBox 偏移   —— 渲染显示域取 CropBox（FPDF_GetPageWidthF
//!                                语义，engine_pdfium 自述同源），不是 MediaBox
//! ② /Rotate 旋转（90/180/270） —— 显示 = 用户空间页顺时针旋 θ
//! ③ Y 轴翻转                   —— PDF 原点在左下，屏幕/位图原点在左上
//! ④ DPI 缩放                   —— 像素 ↔ 点（1 pt = 1/72 inch）
//! ```
//!
//! # 变换模型（推导锚点：显示 = CropBox 顺时针旋 θ，原点左上 y 向下）
//!
//! 设 CropBox = [cx0, cy0, cx1, cy1]（用户空间），显示域尺寸
//! W×H（θ=90/270 时宽高互换：W = ch/H = cw；θ=0/180 时 W = cw/H = ch）。
//! 用户空间四角在显示域的落点：
//!
//! ```text
//! θ=0    : BL(cx0,cy0)→(0,H)   TL(cx0,cy1)→(0,0)   TR(cx1,cy1)→(W,0)   BR(cx1,cy0)→(W,H)
//! θ=90   : BL→(0,0)            TL→(W,0)            TR→(W,H)            BR→(0,H)
//! θ=180  : BL→(W,0)            TL→(W,H)            TR→(0,H)            BR→(0,0)
//! θ=270  : BL→(W,H)            TL→(0,H)            TR→(0,0)            BR→(W,0)
//! ```
//!
//! 由此展开的双向公式（screen↔user 的实现体，`u2d`/`d2u`）：
//!
//! ```text
//! u2d（用户→显示点）                  d2u（显示→用户点）
//! θ=0    : dx = ux-cx0   dy = cy1-uy   ux = dx+cx0   uy = cy1-dy
//! θ=90   : dx = uy-cy0   dy = ux-cx0   ux = dy+cx0   uy = dx+cy0
//! θ=180  : dx = cx1-ux   dy = uy-cy0   ux = cx1-dx   uy = dy+cy0
//! θ=270  : dx = cy1-uy   dy = cx1-ux   ux = cx1-dy   uy = cy1-dx
//! ```
//!
//! 屏幕像素 ↔ 显示点：`d = px × 72 / dpi`（`px = d × dpi / 72`）。
//! 每对公式互为严格逆 → 往返恒等（语义锁 `roundtrip` 族）。
//!
//! # 与渲染面的一致性
//!
//! `engine_pdfium::rasterize` 以 `FPDF_GetPageWidthF/HeightF`（显示域
//! 尺寸 = CropBox + /Rotate 归一）定纵横比、`FPDF_RenderPageBitmap`
//! 渲染显示域——本模块的「显示域」与渲染位图像素空间同构：
//! `[`PageTransform::display_width_pt`] × display_height_pt` 的点域按
//! DPI 放大即位图像素域。**标注落点管线**（UI 接线时）：
//! 点击像素 → [`PageTransform::screen_to_user`] → 用户空间点 →
//! [`PageTransform::rect_to_quads`]（TL,TR,BL,BR 序）→ QuadPoints。
//!
//! # 构造源
//!
//! - 引擎实装（[`crate::engine_pdfium::PdfiumEngine`] 的
//!   `page_transform` 覆写）：`FPDFPage_GetMediaBox` + `FPDFPage_GetCropBox`
//!   （**CropBox 缺失时 pdfium 回落 MediaBox** = 渲染显示域语义）+
//!   `/Rotate`——双盒真实原点/偏移全量；
//! - [`PageTransform::from_geometry`]（[`crate::page::PageGeometry`] 无
//!   原点/CropBox 信息的诚实近似：双盒 = [0,0,w,h] @ MediaBox 尺寸，
//!   trait 默认实现走此路）。
//!
//! # 诚实边界
//!
//! - `media` 仅作观测/校验面，**变换计算只用 crop + rotate**（渲染显示
//!   域即 CropBox 域——MediaBox 不参与像素映射）；
//! - 不做 CropBox ⊆ MediaBox 强校验（真实世界畸形文档两盒可交错，
//!   数学对此自洽；[`Rect::new`] 已保证双盒自身良构）；
//! - DPI ≤ 0 / 非有限 → typed 错误（[`TransformError::InvalidDpi`]），
//!   不静默产出 NaN 落点。

use serde::{Deserialize, Serialize};

use crate::annotation::{Point, Quad, Rect};
use crate::page::{PageGeometry, Rotation};

/// 页变换构造/求值错误。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformError {
    /// DPI 非法（≤ 0 或非有限）——像素↔点换算的除数。
    InvalidDpi { dpi: f32 },
    /// 屏幕矩形坐标非有限（NaN/Inf 输入——拒绝静默产出毒落点）。
    InvalidScreenCoord { x: f32, y: f32 },
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransformError::InvalidDpi { dpi } => {
                write!(f, "DPI 非法（{dpi}）：须为有限正数（像素↔点换算除数）")
            }
            TransformError::InvalidScreenCoord { x, y } => {
                write!(f, "屏幕坐标非有限（x={x}, y={y}）")
            }
        }
    }
}

impl std::error::Error for TransformError {}

/// pt ↔ inch 常数（1 pt = 1/72 inch——PDF 用户空间单位定义）。
pub const POINTS_PER_INCH: f32 = 72.0;

/// 页变换：MediaBox / CropBox / `/Rotate` 的组合，设备（渲染位图像素）
/// ↔ 用户空间（pt）双向映射。
///
/// 构造面见模块文档（引擎实装 / [`from_geometry`](Self::from_geometry)
/// 近似）；公式推导与四旋转锚点表见模块文档「变换模型」。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageTransform {
    /// MediaBox（用户空间；观测/校验面——不参与像素映射，见诚实边界）。
    media: Rect,
    /// CropBox（用户空间；渲染显示域——像素映射的基准盒）。
    crop: Rect,
    /// `/Rotate`（显示时顺时针旋转角）。
    rotate: Rotation,
}

impl PageTransform {
    /// 构造（双盒良构由 [`Rect::new`] 类型层保证；不做 CropBox ⊆
    /// MediaBox 强校验——真实世界畸形文档容忍，见模块文档诚实边界）。
    pub fn new(media: Rect, crop: Rect, rotate: Rotation) -> Self {
        Self {
            media,
            crop,
            rotate,
        }
    }

    /// 由 [`PageGeometry`] 推导（**诚实近似**）：CropBox 信息缺失时
    /// 双盒 = [0, 0, w, h] @ MediaBox 尺寸、原点 (0,0)。零原点/零偏移
    /// 文档上与真实变换一致；**CropBox ⊄ 原点 MediaBox 的文档上有
    /// 偏移**（引擎实装 [`crate::engine::DocumentEngine::page_transform`]
    /// 取真实双盒——本构造是 trait 默认实现的降级路径）。
    pub fn from_geometry(geo: &PageGeometry) -> Self {
        let media = Rect::new(0.0, 0.0, geo.width_pt, geo.height_pt)
            .expect("PageGeometry 尺寸非负（引擎/构造面不变量）");
        Self {
            crop: media,
            media,
            rotate: geo.rotation,
        }
    }

    /// MediaBox（观测面）。
    pub fn media(&self) -> &Rect {
        &self.media
    }

    /// CropBox（像素映射基准盒）。
    pub fn crop(&self) -> &Rect {
        &self.crop
    }

    /// `/Rotate`。
    pub fn rotate(&self) -> Rotation {
        self.rotate
    }

    /// 显示域宽（pt）：CropBox 宽经旋转换算（90/270 交换宽高）——与
    /// `FPDF_GetPageWidthF` 同语义（渲染位图的点域宽）。
    pub fn display_width_pt(&self) -> f32 {
        if self.rotate.swaps_axes() {
            self.crop.height()
        } else {
            self.crop.width()
        }
    }

    /// 显示域高（pt）：与 [`display_width_pt`](Self::display_width_pt)
    /// 同语义的纵向面。
    pub fn display_height_pt(&self) -> f32 {
        if self.rotate.swaps_axes() {
            self.crop.width()
        } else {
            self.crop.height()
        }
    }

    /// 屏幕像素 → PDF 用户空间点。
    ///
    /// `(x_px, y_px)` = 渲染位图内像素坐标（原点左上，y 向下——与
    /// 位图行序一致）；`dpi` = 该位图的渲染 DPI。DPI 非法 → typed
    /// 错误（不静默产出 NaN 落点）。
    pub fn screen_to_user(&self, x_px: f32, y_px: f32, dpi: f32) -> Result<Point, TransformError> {
        let (dx, dy) = screen_to_display(x_px, y_px, dpi)?;
        let (x, y) = self.display_to_user(dx, dy);
        Ok(Point { x, y })
    }

    /// PDF 用户空间点 → 屏幕像素（`(x_px, y_px)`，原点左上）。
    ///
    /// [`screen_to_user`](Self::screen_to_user) 的严格逆（语义锁
    /// `roundtrip` 族：双向往返误差 = 0）。
    pub fn user_to_screen(&self, p: Point, dpi: f32) -> Result<(f32, f32), TransformError> {
        let (dx, dy) = self.user_to_display(p.x, p.y);
        display_to_screen(dx, dy, dpi)
    }

    /// 屏幕拖框（像素，任意拖拽方向）→ 用户空间矩形。
    ///
    /// UI 主消费面：鼠标拖选（起点→终点任意方向）→ 归一化（min/max
    /// 重排）→ 双点 [`Rect::new`](Self::screen_to_user) 构造。反向拖拽
    /// 的归一化是必须的（用户空间 [`Rect`] 的 x0≤x1/y0≤y1 不变量 +
    /// QuadPoints 落点语义）。
    pub fn screen_rect_to_user(
        &self,
        x0_px: f32,
        y0_px: f32,
        x1_px: f32,
        y1_px: f32,
        dpi: f32,
    ) -> Result<Rect, TransformError> {
        let a = self.screen_to_user(x0_px, y0_px, dpi)?;
        let b = self.screen_to_user(x1_px, y1_px, dpi)?;
        // 归一化：拖拽方向任意，用户空间矩形取两点的 min/max 包络。
        Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
            .map_err(|_| TransformError::InvalidScreenCoord { x: a.x, y: a.y })
    }

    /// 用户空间矩形 → QuadPoints 四角（**TL, TR, BL, BR 序**——与
    /// 模型 [`Quad`] 约定/写面实测一致，pdfd_writer.rs:222-228 的
    /// 语义同源）。
    ///
    /// 纯用户空间几何（旋转无关——`&self` 仅取齐 API 族；QuadPoints
    /// 落盘恒为用户空间坐标，旋转在显示侧消化）。
    pub fn rect_to_quads(&self, rect: &Rect) -> Quad {
        Quad {
            points: [
                Point {
                    x: rect.x0(),
                    y: rect.y1(),
                },
                Point {
                    x: rect.x1(),
                    y: rect.y1(),
                },
                Point {
                    x: rect.x0(),
                    y: rect.y0(),
                },
                Point {
                    x: rect.x1(),
                    y: rect.y0(),
                },
            ],
        }
    }

    // ------------------------------------------------------------------
    // 内部：显示域（pt，原点左上 y 向下）↔ 用户空间（pt，原点左下 y 向上）
    // ------------------------------------------------------------------

    /// 用户空间点 → 显示域点（公式表见模块文档；θ 经四角锚点推导——
    /// 180° 的 dy 是 `uy-cy0`（上下翻转：用户顶 → 显示底），锚点锁背书）。
    fn user_to_display(&self, ux: f32, uy: f32) -> (f32, f32) {
        let c = self.crop;
        match self.rotate {
            Rotation::Deg0 => (ux - c.x0(), c.y1() - uy),
            Rotation::Deg90 => (uy - c.y0(), ux - c.x0()),
            Rotation::Deg180 => (c.x1() - ux, uy - c.y0()),
            Rotation::Deg270 => (c.y1() - uy, c.x1() - ux),
        }
    }

    /// 显示域点 → 用户空间点（`user_to_display` 的严格逆）。
    fn display_to_user(&self, dx: f32, dy: f32) -> (f32, f32) {
        let c = self.crop;
        match self.rotate {
            Rotation::Deg0 => (dx + c.x0(), c.y1() - dy),
            Rotation::Deg90 => (dy + c.x0(), dx + c.y0()),
            Rotation::Deg180 => (c.x1() - dx, dy + c.y0()),
            Rotation::Deg270 => (c.x1() - dy, c.y1() - dx),
        }
    }
}

/// 屏幕像素 → 显示域点（DPI 换算 + 非法 DPI typed 拒绝）。
fn screen_to_display(x_px: f32, y_px: f32, dpi: f32) -> Result<(f32, f32), TransformError> {
    check_dpi(dpi)?;
    let scale = POINTS_PER_INCH / dpi;
    Ok((x_px * scale, y_px * scale))
}

/// 显示域点 → 屏幕像素（`screen_to_display` 的逆 + 同款 DPI 校验）。
fn display_to_screen(dx: f32, dy: f32, dpi: f32) -> Result<(f32, f32), TransformError> {
    check_dpi(dpi)?;
    let scale = dpi / POINTS_PER_INCH;
    Ok((dx * scale, dy * scale))
}

/// DPI 合法性（有限 && > 0——换算除数；失败 = typed 错误不产 NaN）。
fn check_dpi(dpi: f32) -> Result<(), TransformError> {
    if dpi.is_finite() && dpi > 0.0 {
        Ok(())
    } else {
        Err(TransformError::InvalidDpi { dpi })
    }
}

// ---------------------------------------------------------------------------
// 单元测试（常开：变换表的逐角锚点 = 公式正确性的直接证据）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn box_(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::new(x0, y0, x1, y1).unwrap()
    }

    /// 旋转页锚点表（模块文档「变换模型」表的逐角断言——公式错则锚点红）。
    #[test]
    fn rotate_anchor_table() {
        // CropBox = [100, 50, 712, 842]（非零原点——偏移面一并验证）：
        // cw = 612（裁剪宽）、ch = 792（裁剪高）。
        let crop = box_(100.0, 50.0, 712.0, 842.0);
        let media = box_(0.0, 0.0, 712.0, 842.0);
        let cw = 612.0;
        let ch = 792.0;

        // 显示域尺寸：θ=0/180 → (cw, ch)；θ=90/270 → (ch, cw)（互换）。
        let anchors = [
            (Rotation::Deg0, (0.0, ch), (0.0, 0.0), (cw, 0.0), (cw, ch)),
            (Rotation::Deg90, (0.0, 0.0), (ch, 0.0), (ch, cw), (0.0, cw)),
            (Rotation::Deg180, (cw, 0.0), (cw, ch), (0.0, ch), (0.0, 0.0)),
            (Rotation::Deg270, (ch, cw), (0.0, cw), (0.0, 0.0), (ch, 0.0)),
        ];
        for (rot, bl, tl, tr, br) in anchors {
            let t = PageTransform::new(media, crop, rot);
            let user_bl = Point {
                x: crop.x0(),
                y: crop.y0(),
            };
            let user_tl = Point {
                x: crop.x0(),
                y: crop.y1(),
            };
            let user_tr = Point {
                x: crop.x1(),
                y: crop.y1(),
            };
            let user_br = Point {
                x: crop.x1(),
                y: crop.y0(),
            };
            for (user, expect) in [(user_bl, bl), (user_tl, tl), (user_tr, tr), (user_br, br)] {
                let (dx, dy) = t.user_to_display(user.x, user.y);
                assert!(
                    (dx - expect.0).abs() < 1e-4 && (dy - expect.1).abs() < 1e-4,
                    "{rot:?}：用户 {user:?} → 显示 ({dx}, {dy})，期望 {expect:?}（四角锚点表）"
                );
                // 逆向同锚（公式互逆的直接证据）。
                let (ux, uy) = t.display_to_user(dx, dy);
                assert!(
                    (ux - user.x).abs() < 1e-4 && (uy - user.y).abs() < 1e-4,
                    "{rot:?}：显示 {expect:?} → 用户 ({ux}, {uy})，期望 {user:?}"
                );
            }
        }
    }

    /// 90° 旋转页落点主径（issue 场景①）：点击显示域中心 → 用户空间
    /// 应为 CropBox 中心（旋转不动中心——强不变量）。
    #[test]
    fn rotate90_center_is_invariant() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let crop = box_(0.0, 0.0, 612.0, 792.0);
        let t = PageTransform::new(media, crop, Rotation::Deg90);
        let dw = t.display_width_pt();
        let dh = t.display_height_pt();
        assert_eq!((dw, dh), (792.0, 612.0), "90° 显示域宽高互换");
        let center = t.screen_to_user(dw / 2.0, dh / 2.0, 72.0).unwrap();
        assert!(
            (center.x - 306.0).abs() < 1e-3 && (center.y - 396.0).abs() < 1e-3,
            "旋转不动 CropBox 中心：得 {center:?}"
        );
    }

    /// CropBox 非零偏移（issue 场景②）：同 MediaBox 两档 CropBox，
    /// 同一屏幕点击的用户落点差 = CropBox 原点差（平移可观测）。
    #[test]
    fn crop_offset_shifts_user_landing() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let full = box_(0.0, 0.0, 612.0, 792.0);
        let inset = box_(36.0, 54.0, 576.0, 738.0); // ⊂ MediaBox 且非零偏移
        let t_full = PageTransform::new(media, full, Rotation::Deg0);
        let t_inset = PageTransform::new(media, inset, Rotation::Deg0);

        // 同一像素（各显示域中心附近取同一点位）。
        let px = 100.0_f32;
        let py = 100.0_f32;
        let u_full = t_full.screen_to_user(px, py, 96.0).unwrap();
        let u_inset = t_inset.screen_to_user(px, py, 96.0).unwrap();
        // θ=0：ux 差 = cx0 差（36 - 0）；uy 差 = -(cy0 差)（54，Y 翻转方向）。
        assert!(
            (u_inset.x - u_full.x - 36.0).abs() < 1e-3,
            "x 落点平移 = CropBox 原点差"
        );
        assert!(
            (u_full.y - u_inset.y - 54.0).abs() < 1e-3,
            "y 落点平移 = CropBox 原点差（Y 翻转）"
        );
    }

    /// 往返恒等（issue 验收 3+5）：DPI 72/96/150/300 × 四旋转 × 随机点，
    /// screen→user→screen 误差 < 1e-3 px（公式互逆的数值面）。
    #[test]
    fn roundtrip_all_dpi_and_rotations() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let crop = box_(18.0, 24.0, 594.0, 768.0); // 非零偏移 CropBox
        let pts = [
            (0.0f32, 0.0f32),
            (5.5, 7.25),
            (100.0, 63.0),
            (312.0, 400.5),
            (999.75, 888.25),
        ];
        for rot in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let t = PageTransform::new(media, crop, rot);
            for &dpi in &[72.0f32, 96.0, 150.0, 300.0] {
                for &(x, y) in &pts {
                    let u = t.screen_to_user(x, y, dpi).unwrap();
                    let (sx, sy) = t.user_to_screen(u, dpi).unwrap();
                    assert!(
                        (sx - x).abs() < 1e-3 && (sy - y).abs() < 1e-3,
                        "{rot:?} @ {dpi} DPI：({x},{y}) → {u:?} → ({sx},{sy})"
                    );
                }
            }
        }
    }

    /// DPI 换算数值锚点：72 DPI 时像素=点（1:1）；96 DPI 时 96 px = 72 pt。
    #[test]
    fn dpi_scale_anchors() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let t = PageTransform::new(media, media, Rotation::Deg0);
        // 72 DPI：像素坐标 = 显示点坐标（原点 (0,0) = CropBox 左上）。
        let u = t.screen_to_user(100.0, 200.0, 72.0).unwrap();
        assert!((u.x - 100.0).abs() < 1e-4 && (u.y - (792.0 - 200.0)).abs() < 1e-4);
        // 96 DPI：96 px = 1 inch = 72 pt → x=96px → 72pt。
        let u96 = t.screen_to_user(96.0, 0.0, 96.0).unwrap();
        assert!((u96.x - 72.0).abs() < 1e-4, "96 DPI：96px = 72pt");
    }

    /// 四角序（issue 关联约定：TL,TR,BL,BR——pdfd_writer.rs:222-228 同源）。
    #[test]
    fn quad_order_is_tl_tr_bl_br() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let t = PageTransform::new(media, media, Rotation::Deg0);
        let rect = Rect::new(72.0, 300.0, 300.0, 360.0).unwrap();
        let q = t.rect_to_quads(&rect);
        assert_eq!(
            q.points,
            [
                Point { x: 72.0, y: 360.0 },  // TL
                Point { x: 300.0, y: 360.0 }, // TR
                Point { x: 72.0, y: 300.0 },  // BL
                Point { x: 300.0, y: 300.0 }, // BR
            ],
            "四角序 = TL,TR,BL,BR（与写面/模型约定一致）"
        );
    }

    /// 屏幕拖框（任意方向）→ 用户空间矩形（归一化 + 不变量）。
    #[test]
    fn screen_rect_drag_directions_normalize() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let t = PageTransform::new(media, media, Rotation::Deg0);
        // 正向拖（左上→右下）与反向拖（右下→左上）产物相等。
        let fwd = t
            .screen_rect_to_user(100.0, 80.0, 200.0, 160.0, 96.0)
            .unwrap();
        let bwd = t
            .screen_rect_to_user(200.0, 160.0, 100.0, 80.0, 96.0)
            .unwrap();
        assert_eq!(fwd, bwd, "拖拽方向不影响用户空间矩形");
        // 90° 旋转 + 反向拖：归一化在用户空间发生（旋转后 min/max 语义不破）。
        let t90 = PageTransform::new(media, media, Rotation::Deg90);
        let r = t90
            .screen_rect_to_user(200.0, 160.0, 100.0, 80.0, 96.0)
            .unwrap();
        assert!(r.x0() <= r.x1() && r.y0() <= r.y1(), "用户空间矩形不变量");
    }

    /// 非法 DPI → typed 错误（不静默产出 NaN 落点）。
    #[test]
    fn invalid_dpi_is_typed_error() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let t = PageTransform::new(media, media, Rotation::Deg0);
        for bad in [0.0f32, -96.0, f32::NAN, f32::INFINITY] {
            let err = t.screen_to_user(1.0, 1.0, bad).unwrap_err();
            assert!(matches!(err, TransformError::InvalidDpi { .. }), "{err:?}");
            let err2 = t.user_to_screen(Point { x: 1.0, y: 1.0 }, bad).unwrap_err();
            assert!(
                matches!(err2, TransformError::InvalidDpi { .. }),
                "{err2:?}"
            );
        }
    }

    /// from_geometry 近似：双盒 = [0,0,w,h] @ MediaBox 尺寸 + 显示域 =
    /// effective 面（与 PageGeometry 归一口径一致）。
    #[test]
    fn from_geometry_approximates_media_at_origin() {
        let geo = PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            rotation: Rotation::Deg90,
        };
        let t = PageTransform::from_geometry(&geo);
        assert_eq!(*t.media(), box_(0.0, 0.0, 612.0, 792.0));
        assert_eq!(*t.crop(), box_(0.0, 0.0, 612.0, 792.0));
        assert_eq!(t.display_width_pt(), geo.effective_width());
        assert_eq!(t.display_height_pt(), geo.effective_height());
    }

    /// 显示域尺寸：CropBox 基 + 旋转归一（与 FPDF_GetPageWidthF 语义同源）。
    #[test]
    fn display_dims_follow_crop_and_rotation() {
        let media = box_(0.0, 0.0, 612.0, 792.0);
        let crop = box_(12.0, 18.0, 600.0, 774.0); // 588 × 756
        let t0 = PageTransform::new(media, crop, Rotation::Deg0);
        assert_eq!(
            (t0.display_width_pt(), t0.display_height_pt()),
            (588.0, 756.0)
        );
        let t90 = PageTransform::new(media, crop, Rotation::Deg90);
        assert_eq!(
            (t90.display_width_pt(), t90.display_height_pt()),
            (756.0, 588.0),
            "90° 显示域宽高互换（CropBox 基）"
        );
    }
}
