//! #304 设备↔用户空间变换——标注落点的坐标基座（审计 P2-4）。
//!
//! # 为什么必须有这个模块
//!
//! 标注流程：① 用户在屏幕上点击（设备像素）→ ② 换算成 PDF 用户空间
//! 点 → ③ 写进 `QuadPoints`。四类变换必须有人管，此前零设施（旋转页/
//! 裁剪页上标注必落错位）：
//!
//! ```text
//! · CropBox vs MediaBox 偏移（渲染显示域是 CropBox，不是 MediaBox）
//! · /Rotate 旋转（90/180/270，显示时顺时针）
//! · Y 轴翻转（PDF 原点在左下 y 向上；屏幕原点在左上 y 向下）
//! · DPI 缩放（渲染像素 ≠ PDF 点，1pt = 1/72 inch）
//! ```
//!
//! # 推导表（显示域 = CropBox；W/H = CropBox 宽/高，用户空间）
//!
//! 设 `(u, v)` = 用户空间点**相对 CropBox 左下角**的坐标；
//! `(dx, dy)` = 显示域坐标（**y 向上**，原点在显示域左下）；
//! 显示域尺寸 `DW × DH`（旋转后：90/270 时 `DW = H, DH = W`）。
//!
//! `/Rotate` 语义 = 显示时把页面**顺时针**转 N 度（ISO 32000-1 §7.3.3）。
//! 顺时针旋转下四条边的去向：上边→右边、右边→下边、下边→左边、
//! 左边→上边。由此逐旋转推导（四角点代入验证过）：
//!
//! ```text
//! Rotate   显示 ← 用户            用户 ← 显示（逆推）
//! 0        (dx, dy) = (u, v)        u = dx,     v = dy
//! 90       (dx, dy) = (v, W - u)    u = W - dy, v = dx
//! 180      (dx, dy) = (W-u, H - v)  u = W - dx, v = H - dy
//! 270      (dx, dy) = (H - v, u)    u = dy,     v = H - dx
//! ```
//!
//! 屏幕像素 `(sx, sy)`（y 向下，原点=显示域左上）与显示域坐标的桥：
//! `dx = sx · 72/dpi`，`dy = DH - sy · 72/dpi`。
//!
//! 最终用户空间绝对坐标 = CropBox 原点 + 相对坐标：
//! `user_x = cx0 + u`，`user_y = cy0 + v`。
//!
//! # 语义边界（诚实声明）
//!
//! - 显示域尺寸口径 = **CropBox + 旋转**，与 pdfium 渲染
//!   （`FPDF_GetPageWidth/Height`）一致；与
//!   [`PageGeometry::effective_width`](crate::page::PageGeometry) 的
//!   MediaBox 口径在 `CropBox ⊊ MediaBox` 的档上**有意不同**（effective
//!   面是模型语义，显示面是渲染真值——差异本身就是要暴露的信息）。
//! - 变换是**纯函数**：构造期一次性校验几何（crop ⊆ media、非退化、
//!   有限值），之后 `screen_to_user`/`user_to_screen` 全 total——
//!   越界点击/非法 DPI 不报错，映射到数学上的对应点（是否裁剪是
//!   调用方策略，不是变换职责）。
//! - 多个 Box 退化共存时（CropBox 缺省）取 MediaBox 兜底——与
//!   spec 7.7.3.3「CropBox 缺省值 = MediaBox」一致。
//!
//! # 与写面对接
//!
//! 产物 [`Quad`] 的四角序 = `TL, TR, BL, BR`（实测与 Acrobat 一致，
//! `pdfd_writer.rs` 同款——`p0→p1` 为文本行方向边），直接可进
//! [`AnnotationKind::Highlight`](crate::annotation::AnnotationKind) 等
//! 写面载荷。

use crate::annotation::{Point, Quad};
use crate::page::{PageBox, PageGeometry, Rotation};

/// 显示域尺寸（pt，含旋转）——与渲染位图同口径。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplaySize {
    /// 显示宽（pt）：90/270° = CropBox 高，否则 = CropBox 宽。
    pub width: f32,
    /// 显示高（pt）。
    pub height: f32,
}

/// 变换构造错误（typed——畸形几何显式拒绝，不产毒变换）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformError {
    /// CropBox 不在 MediaBox 内（容差 0.5pt 浮点比较；含非有限值形态）。
    CropOutsideMedia { crop: PageBox, media: PageBox },
    /// 显示域退化（CropBox/MediaBox 宽或高 ≤ 0——空页无变换意义）。
    Degenerate { box_: PageBox },
    /// 几何含非有限值（NaN/Inf——引擎数据被污染或上游算错）。
    NonFinite { box_: PageBox },
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransformError::CropOutsideMedia { crop, media } => write!(
                f,
                "CropBox {crop:?} 不在 MediaBox {media:?} 内（容差 0.5pt）——\
                 畸形页几何，拒绝构造变换"
            ),
            TransformError::Degenerate { box_ } => {
                write!(f, "页框 {box_:?} 退化（宽或高 ≤ 0）——空显示域无变换意义")
            }
            TransformError::NonFinite { box_ } => {
                write!(f, "页框 {box_:?} 含非有限值（NaN/Inf）——几何数据被污染")
            }
        }
    }
}

impl std::error::Error for TransformError {}

/// 设备↔用户空间变换（一页一实例，构造期校验 + 纯函数映射）。
///
/// 构造来源：[`PageTransform::new`]（显式三参）或
/// [`PageTransform::from_geometry`]（引擎产出的 [`PageGeometry`]）。
///
/// ```ignore
/// use mdc_pdf::page::{PageBox, Rotation};
/// use mdc_pdf::transform::PageTransform;
///
/// let t = PageTransform::new(
///     PageBox { x0: 0.0, y0: 0.0, x1: 612.0, y1: 792.0 },
///     Some(PageBox { x0: 30.0, y0: 40.0, x1: 582.0, y1: 752.0 }),
///     Rotation::Deg90,
/// ).unwrap();
/// // 屏幕点击 (100px, 50px) @ 96dpi → 用户空间点
/// let p = t.screen_to_user(100.0, 50.0, 96.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageTransform {
    media: PageBox,
    /// 渲染显示域（= CropBox，缺省时 = MediaBox——spec 7.7.3.3）。
    crop: PageBox,
    rotate: Rotation,
}

/// 浮点盒比较容差（pt）——PDF 页框的浮点噪声兜底。
const EPS: f32 = 0.5;

impl PageTransform {
    /// 显式构造（三参齐全——引擎/测试面）。
    ///
    /// `crop = None` = 页无 `/CropBox` → 显示域取 `media`。
    pub fn new(
        media: PageBox,
        crop: Option<PageBox>,
        rotate: Rotation,
    ) -> Result<Self, TransformError> {
        let crop = crop.unwrap_or(media);
        for b in [&media, &crop] {
            if !(b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite()) {
                return Err(TransformError::NonFinite { box_: *b });
            }
        }
        // crop ⊆ media（四向，含容差）。
        if crop.x0 < media.x0 - EPS
            || crop.y0 < media.y0 - EPS
            || crop.x1 > media.x1 + EPS
            || crop.y1 > media.y1 + EPS
        {
            return Err(TransformError::CropOutsideMedia { crop, media });
        }
        // 显示域非退化（media 退化同理拒绝——兜底显示域就是它）。
        for b in [&media, &crop] {
            if b.width() <= 0.0 || b.height() <= 0.0 {
                return Err(TransformError::Degenerate { box_: *b });
            }
        }
        Ok(Self {
            media,
            crop,
            rotate,
        })
    }

    /// 从引擎页几何构造（UI 接线主路径）。
    ///
    /// 兼容旧序列化形态（#304 前的 JSON 无 `media_box` 字段 →
    /// `#[serde(default)]` 给出零盒）：宽高在场且合法时重建
    /// `(0, 0, width_pt, height_pt)`（原点 (0,0)——预 #304 世界观里
    /// 宽高就是全部几何；非零原点的旧档不存在可识别信号，不猜）。
    pub fn from_geometry(g: &PageGeometry) -> Result<Self, TransformError> {
        let media = if g.media_box.width() > 0.0 && g.media_box.height() > 0.0 {
            g.media_box
        } else if g.width_pt > 0.0 && g.height_pt > 0.0 {
            PageBox {
                x0: 0.0,
                y0: 0.0,
                x1: g.width_pt,
                y1: g.height_pt,
            }
        } else {
            g.media_box // 两部数据都不健全——交给构造器报 typed 错误
        };
        Self::new(media, g.crop_box, g.rotation)
    }

    /// MediaBox（用户空间）。
    pub fn media(&self) -> PageBox {
        self.media
    }

    /// 显示域（= CropBox；页无 `/CropBox` 时 = MediaBox）。
    pub fn crop(&self) -> PageBox {
        self.crop
    }

    /// 页旋转。
    pub fn rotate(&self) -> Rotation {
        self.rotate
    }

    /// 显示域尺寸（pt，含旋转）——渲染位图的期望宽高（同
    /// `FPDF_GetPageWidth/Height` 口径：CropBox 基）。
    pub fn display_size(&self) -> DisplaySize {
        if self.rotate.swaps_axes() {
            DisplaySize {
                width: self.crop.height(),
                height: self.crop.width(),
            }
        } else {
            DisplaySize {
                width: self.crop.width(),
                height: self.crop.height(),
            }
        }
    }

    /// 屏幕像素 → PDF 用户空间点。
    ///
    /// `(x, y)` = 渲染位图像素坐标（y 向下，原点 = 位图左上角）；
    /// `dpi` = 位图分辨率（像素/英寸）。total 函数：越界坐标映射到
    /// 数学对应点（裁剪策略归调用方），非法 dpi 产生 NaN（构造期已
    /// 校验几何，此处不重复报错——见模块文档「语义边界」）。
    pub fn screen_to_user(&self, x: f32, y: f32, dpi: f32) -> Point {
        let pt_per_px = 72.0 / dpi;
        let size = self.display_size();
        // 屏幕像素 → 显示域坐标（y 翻转：屏幕左上原点 → 显示域左下原点）
        let dx = x * pt_per_px;
        let dy = size.height - y * pt_per_px;
        // 显示域 → 用户空间（相对 CropBox 原点；推导表见模块文档）
        let (u, v) = self.display_to_user_rel(dx, dy);
        Point {
            x: self.crop.x0 + u,
            y: self.crop.y0 + v,
        }
    }

    /// PDF 用户空间点 → 屏幕像素 `(x, y)`（y 向下，原点 = 位图左上角）。
    ///
    /// [`screen_to_user`] 的逆映射——往返误差为 f32 舍入量级（< 0.01 像素，
    /// 见测试 `roundtrip_all_rotations_crop_offsets_dpi_sweep`）。
    pub fn user_to_screen(&self, p: Point, dpi: f32) -> (f32, f32) {
        let size = self.display_size();
        let (u, v) = (p.x - self.crop.x0, p.y - self.crop.y0);
        let (dx, dy) = self.user_rel_to_display(u, v);
        let px_per_pt = dpi / 72.0;
        (dx * px_per_pt, (size.height - dy) * px_per_pt)
    }

    /// 用户空间矩形 → QuadPoints 四角 `[TL, TR, BL, BR]`
    /// （`pdfd_writer.rs` 实测序，与 Acrobat 一致；`p0→p1` = 文本行
    /// 方向边）。矩形已 在用户空间——变换本身不改坐标，此 API 的
    /// 价值是把四角**序**钉在变换模块（单一出口，防写面各自排错序）。
    pub fn rect_to_quads(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> [Point; 4] {
        [
            Point { x: x0, y: y1 }, // TL
            Point { x: x1, y: y1 }, // TR
            Point { x: x0, y: y0 }, // BL
            Point { x: x1, y: y0 }, // BR
        ]
    }

    /// 屏幕选择矩形 → 用户空间 [`Quad`]（UI 标注主流程：框选高亮区域）。
    ///
    /// `(x0, y0)`/`(x1, y1)` = 屏幕像素矩形（**任意对角序**——内部
    /// 归一化为屏幕 TL/BR）。产物四角在用户空间按 `TL, TR, BL, BR` 序
    /// （用户空间自身取向，非屏幕取向的逐角映射——90° 倍旋转下屏幕
    /// 轴对齐矩形映到用户空间仍是轴对齐矩形，但屏幕 TL 在旋转页上
    /// 对应用户空间其他角；序按用户空间取，QuadPoints 语义才正确）。
    pub fn screen_rect_to_quad(&self, x0: f32, y0: f32, x1: f32, y1: f32, dpi: f32) -> Quad {
        let (sx0, sx1) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        let (sy0, sy1) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        // 屏幕矩形四角 → 用户空间（左/右/上/下分头映射，再组装）
        let bl = self.screen_to_user(sx0, sy1, dpi); // 屏幕左下 → 用户空间某角
        let br = self.screen_to_user(sx1, sy1, dpi);
        let tl = self.screen_to_user(sx0, sy0, dpi);
        let tr = self.screen_to_user(sx1, sy0, dpi);
        // 用户空间轴对齐化：旋转是 90° 倍数 → 上述四点仍构成轴对齐
        // 矩形；取 min/max 组装成 TL,TR,BL,BR（用户空间取向）。
        let xs = [tl.x, tr.x, bl.x, br.x];
        let ys = [tl.y, tr.y, bl.y, br.y];
        let (min_x, max_x) = (
            xs.iter().copied().fold(f32::INFINITY, f32::min),
            xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        );
        let (min_y, max_y) = (
            ys.iter().copied().fold(f32::INFINITY, f32::min),
            ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        );
        Quad {
            points: [
                Point { x: min_x, y: max_y }, // TL
                Point { x: max_x, y: max_y }, // TR
                Point { x: min_x, y: min_y }, // BL
                Point { x: max_x, y: min_y }, // BR
            ],
        }
    }

    /// 显示域坐标（y 向上）→ 用户空间相对 CropBox 原点坐标。
    fn display_to_user_rel(&self, dx: f32, dy: f32) -> (f32, f32) {
        let w = self.crop.width();
        let h = self.crop.height();
        match self.rotate {
            Rotation::Deg0 => (dx, dy),
            Rotation::Deg90 => (w - dy, dx),
            Rotation::Deg180 => (w - dx, h - dy),
            Rotation::Deg270 => (dy, h - dx),
        }
    }

    /// 用户空间相对坐标 → 显示域坐标（y 向上）。
    fn user_rel_to_display(&self, u: f32, v: f32) -> (f32, f32) {
        let w = self.crop.width();
        let h = self.crop.height();
        match self.rotate {
            Rotation::Deg0 => (u, v),
            Rotation::Deg90 => (v, w - u),
            Rotation::Deg180 => (w - u, h - v),
            Rotation::Deg270 => (h - v, u),
        }
    }
}

// ---------------------------------------------------------------------------
// 单元测试（纯数学面——零引擎依赖，CI 全环境可跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试基准几何：信纸页 MediaBox + 偏移 CropBox（左下角 (30,40)）。
    fn letter_with_offset_crop() -> (PageBox, PageBox) {
        let media = PageBox {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0,
        };
        let crop = PageBox {
            x0: 30.0,
            y0: 40.0,
            x1: 582.0,
            y1: 752.0,
        };
        (media, crop)
    }

    fn t(rot: Rotation) -> PageTransform {
        let (media, crop) = letter_with_offset_crop();
        PageTransform::new(media, Some(crop), rot).unwrap()
    }

    #[test]
    fn constructor_validates_crop_inside_media() {
        let media = PageBox {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0,
        };
        // 越界 CropBox → 拒绝
        let bad = PageBox {
            x0: -10.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0,
        };
        assert!(matches!(
            PageTransform::new(media, Some(bad), Rotation::Deg0),
            Err(TransformError::CropOutsideMedia { .. })
        ));
        // 容差内浮点噪声（0.1pt < 0.5pt EPS）→ 放行
        let noisy = PageBox {
            x0: -0.1,
            y0: 0.0,
            x1: 612.05,
            y1: 792.0,
        };
        assert!(PageTransform::new(media, Some(noisy), Rotation::Deg0).is_ok());
        // 退化框 → 拒绝
        let degenerate = PageBox {
            x0: 0.0,
            y0: 0.0,
            x1: 0.0,
            y1: 100.0,
        };
        assert!(matches!(
            PageTransform::new(media, Some(degenerate), Rotation::Deg0),
            Err(TransformError::Degenerate { .. })
        ));
        // NaN → 拒绝
        let nan_box = PageBox {
            x0: f32::NAN,
            y0: 0.0,
            x1: 100.0,
            y1: 100.0,
        };
        assert!(matches!(
            PageTransform::new(media, Some(nan_box), Rotation::Deg0),
            Err(TransformError::NonFinite { .. })
        ));
        // crop = None → 显示域取 MediaBox
        let tr = PageTransform::new(media, None, Rotation::Deg0).unwrap();
        assert_eq!(tr.crop(), media);
    }

    /// 显示域尺寸口径：CropBox 基 + 旋转交换。
    #[test]
    fn display_size_is_crop_based_and_rotated() {
        // CropBox 552×712（偏移裁剪的信纸页）
        assert_eq!(t(Rotation::Deg0).display_size().width, 552.0);
        assert_eq!(t(Rotation::Deg0).display_size().height, 712.0);
        assert_eq!(t(Rotation::Deg90).display_size().width, 712.0);
        assert_eq!(t(Rotation::Deg90).display_size().height, 552.0);
        assert_eq!(t(Rotation::Deg180).display_size().width, 552.0);
        assert_eq!(t(Rotation::Deg270).display_size().width, 712.0);
        // 无 CropBox → MediaBox 兜底
        let media = PageBox {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0,
        };
        let tr = PageTransform::new(media, None, Rotation::Deg90).unwrap();
        assert_eq!(tr.display_size().width, 792.0);
        assert_eq!(tr.display_size().height, 612.0);
    }

    /// 已知答案表（推导表的直接断言）：屏幕四角 → 用户空间四角。
    ///
    /// 几何：CropBox (30,40)-(582,752)，W=552 H=712。
    /// 屏幕 (0,0) = 显示域左上；屏幕 (DW,DH) = 显示域右下。
    #[test]
    fn screen_corners_map_to_known_user_points() {
        // Rotate 0：显示域左上 = 用户空间 (cx0, cy1) = (30, 752)
        let p = t(Rotation::Deg0).screen_to_user(0.0, 0.0, 72.0);
        assert!(
            (p.x - 30.0).abs() < 1e-3 && (p.y - 752.0).abs() < 1e-3,
            "{p:?}"
        );
        // 屏幕右下 (552, 712)px@72dpi = 显示域右下 = 用户 (582, 40)
        let p = t(Rotation::Deg0).screen_to_user(552.0, 712.0, 72.0);
        assert!(
            (p.x - 582.0).abs() < 1e-3 && (p.y - 40.0).abs() < 1e-3,
            "{p:?}"
        );

        // Rotate 90（显示域 712×552）：屏幕左上 = 显示域左上 =
        // 用户空间推导：dx=0, dy=DH=552=W → u = W-dy = 0, v = dx = 0
        // → 用户 (cx0, cy0) = (30, 40)（原页左下角——顺时针转后成显示左上）
        let p = t(Rotation::Deg90).screen_to_user(0.0, 0.0, 72.0);
        assert!(
            (p.x - 30.0).abs() < 1e-3 && (p.y - 40.0).abs() < 1e-3,
            "{p:?}"
        );
        // 屏幕右上 (712, 0) = 显示域 (712, 552) = u=W-552=0, v=712
        // → 用户 (30, 40+712=752)（原页左上角 → 显示右上）
        let p = t(Rotation::Deg90).screen_to_user(712.0, 0.0, 72.0);
        assert!(
            (p.x - 30.0).abs() < 1e-3 && (p.y - 752.0).abs() < 1e-3,
            "{p:?}"
        );
        // 屏幕左下 (0, 552) = 显示域 (0, 0) = u=W=552, v=0 → 用户 (582, 40)
        let p = t(Rotation::Deg90).screen_to_user(0.0, 552.0, 72.0);
        assert!(
            (p.x - 582.0).abs() < 1e-3 && (p.y - 40.0).abs() < 1e-3,
            "{p:?}"
        );

        // Rotate 180：屏幕左上 = 显示域左上（dx=0, dy=DH=712=H）
        // → u = W-dx = 552, v = H-dy = 0
        // → 用户 (582, 40)（原页右下角 → 180° 后成显示左上）
        let p = t(Rotation::Deg180).screen_to_user(0.0, 0.0, 72.0);
        assert!(
            (p.x - 582.0).abs() < 1e-3 && (p.y - 40.0).abs() < 1e-3,
            "{p:?}"
        );

        // Rotate 270（显示域 712×552）：屏幕左上 = 显示域 (0, DH=552)
        // → u = dy = 552, v = H-dx = 712 → 用户 (582, 752)（原页右上角
        // → 逆时针 90° 后成显示左上）
        let p = t(Rotation::Deg270).screen_to_user(0.0, 0.0, 72.0);
        assert!(
            (p.x - 582.0).abs() < 1e-3 && (p.y - 752.0).abs() < 1e-3,
            "{p:?}"
        );
    }

    /// DPI 缩放：同一点在不同 DPI 下的像素坐标按 72/dpi 比例缩放。
    #[test]
    fn dpi_scaling_is_linear() {
        let tr = t(Rotation::Deg0);
        // 96dpi 下屏幕中心 (276, 356)px → 72dpi 等价 (207, 267)pt
        let a = tr.screen_to_user(276.0, 356.0, 96.0);
        let b = tr.screen_to_user(207.0, 267.0, 72.0);
        assert!(
            (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01,
            "{a:?} {b:?}"
        );
    }

    /// 往返测试（验收 5）：全旋转 × CropBox 偏移 × DPI 扫描，
    /// screen_to_user → user_to_screen 回原点，误差 < 0.01px。
    #[test]
    fn roundtrip_all_rotations_crop_offsets_dpi_sweep() {
        let media = PageBox {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0,
        };
        let crops = [
            None, // 无 CropBox
            Some(PageBox {
                x0: 0.0,
                y0: 0.0,
                x1: 612.0,
                y1: 792.0,
            }), // 全幅（= media）
            Some(PageBox {
                x0: 30.0,
                y0: 40.0,
                x1: 582.0,
                y1: 752.0,
            }), // 居中偏移裁剪
            Some(PageBox {
                x0: 100.0,
                y0: 20.0,
                x1: 400.0,
                y1: 700.0,
            }), // 非对称偏移裁剪
        ];
        let rotations = [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ];
        let dpis = [72.0f32, 96.0, 150.0, 300.0];
        for crop in &crops {
            for rot in rotations {
                let tr = PageTransform::new(media, *crop, rot).unwrap();
                let size = tr.display_size();
                for dpi in dpis {
                    let (w_px, h_px) = (size.width * dpi / 72.0, size.height * dpi / 72.0);
                    // 采样 5×5 网格 + 四角
                    for i in 0..=5 {
                        for j in 0..=5 {
                            let sx = w_px * i as f32 / 5.0;
                            let sy = h_px * j as f32 / 5.0;
                            let p = tr.screen_to_user(sx, sy, dpi);
                            let (bx, by) = tr.user_to_screen(p, dpi);
                            assert!(
                                (bx - sx).abs() < 0.01 && (by - sy).abs() < 0.01,
                                "往返断裂 rot={rot:?} crop={crop:?} dpi={dpi} \
                                 ({sx},{sy}) → {p:?} → ({bx},{by})"
                            );
                        }
                    }
                }
            }
        }
    }

    /// 逆映射方向：user → screen → user 同样回原点（误差 < 0.01pt）。
    #[test]
    fn roundtrip_user_direction() {
        for rot in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let tr = t(rot);
            // 用户空间 CropBox 内 5×5 采样（含边界外一点——total 语义）
            let (cx, cy) = (tr.crop().x0, tr.crop().y0);
            let (cw, ch) = (tr.crop().width(), tr.crop().height());
            for i in 0..=5 {
                for j in 0..=5 {
                    let u = cx + cw * i as f32 / 5.0;
                    let v = cy + ch * j as f32 / 5.0;
                    let (sx, sy) = tr.user_to_screen(Point { x: u, y: v }, 96.0);
                    let p = tr.screen_to_user(sx, sy, 96.0);
                    assert!(
                        (p.x - u).abs() < 0.01 && (p.y - v).abs() < 0.01,
                        "user 方向往返断裂 rot={rot:?} ({u},{v}) → ({sx},{sy}) → {p:?}"
                    );
                }
            }
        }
    }

    /// QuadPoints 四角序 = TL,TR,BL,BR（写面约定锁定）。
    #[test]
    fn rect_to_quads_order_is_tl_tr_bl_br() {
        let tr = t(Rotation::Deg0);
        let q = tr.rect_to_quads(10.0, 20.0, 110.0, 220.0);
        let [tl, tr_, bl, br] = q;
        assert!((tl.x - 10.0).abs() < 1e-5 && (tl.y - 220.0).abs() < 1e-5);
        assert!((tr_.x - 110.0).abs() < 1e-5 && (tr_.y - 220.0).abs() < 1e-5);
        assert!((bl.x - 10.0).abs() < 1e-5 && (bl.y - 20.0).abs() < 1e-5);
        assert!((br.x - 110.0).abs() < 1e-5 && (br.y - 20.0).abs() < 1e-5);
    }

    /// 屏幕选择矩形 → 用户空间 Quad：旋转页上仍轴对齐 + 与角点映射一致。
    #[test]
    fn screen_rect_to_quad_is_axis_aligned_and_consistent() {
        for rot in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let tr = t(rot);
            let size = tr.display_size();
            let dpi = 96.0;
            let (w_px, h_px) = (size.width * dpi / 72.0, size.height * dpi / 72.0);
            // 屏幕上选中间 1/4 区域（对角序乱给——内部归一化）
            let quad =
                tr.screen_rect_to_quad(w_px * 0.75, h_px * 0.75, w_px * 0.25, h_px * 0.25, dpi);
            let [tl, tr_, bl, br] = quad.points;
            // 轴对齐 + 角序（用户空间取向）
            assert!(tl.y == tr_.y && bl.y == br.y, "rot={rot:?} 上下边不水平");
            assert!(tl.x == bl.x && tr_.x == br.x, "rot={rot:?} 左右边不垂直");
            assert!(
                tl.y > bl.y && tl.x < tr_.x,
                "rot={rot:?} 角序非 TL,TR,BL,BR"
            );
            // 与逐点映射一致：屏幕 (25%,25%)（屏幕向 TL）映射点应恰为
            // quad 四角之一（旋转页上屏幕 TL 对应用户空间的某个角——
            // 0°=TL、90°=BL、180°=BR、270°=TR）
            let p = tr.screen_to_user(w_px * 0.25, h_px * 0.25, dpi);
            let is_corner = quad
                .points
                .iter()
                .any(|c| (c.x - p.x).abs() < 0.01 && (c.y - p.y).abs() < 0.01);
            assert!(
                is_corner,
                "rot={rot:?} 屏幕角点 {p:?} 应等于 quad 四角之一：{:?}",
                quad.points
            );
            // quad 的宽高 = 显示域 1/2 对应的用户空间尺寸（旋转不变度量）
            let qw = (tr_.x - tl.x).abs();
            let qh = (tl.y - bl.y).abs();
            let expect_w = tr.crop().width() * 0.5;
            let expect_h = tr.crop().height() * 0.5;
            assert!(
                (qw - expect_w).abs() < 0.01 && (qh - expect_h).abs() < 0.01,
                "rot={rot:?} quad 尺寸 {qw}×{qh} ≠ 期望 {expect_w}×{expect_h}"
            );
        }
    }

    /// from_geometry 接线：PageGeometry → PageTransform 字段直通。
    #[test]
    fn from_geometry_wiring() {
        let (media, crop) = letter_with_offset_crop();
        let g = PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            rotation: Rotation::Deg90,
            media_box: media,
            crop_box: Some(crop),
        };
        let tr = PageTransform::from_geometry(&g).unwrap();
        assert_eq!(tr.media(), media);
        assert_eq!(tr.crop(), crop);
        assert_eq!(tr.rotate(), Rotation::Deg90);
        // 无 CropBox 的几何 → 显示域 = MediaBox
        let g2 = PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            rotation: Rotation::Deg0,
            media_box: media,
            crop_box: None,
        };
        let tr2 = PageTransform::from_geometry(&g2).unwrap();
        assert_eq!(tr2.crop(), media);
    }

    /// 旧序列化形态兼容（#304 前的 JSON：media_box 缺省零盒）→
    /// 宽高重建，不 Degenerate 误报。
    #[test]
    fn from_geometry_legacy_json_rebuilds_media_from_size() {
        let legacy_json = r#"{"width_pt":612.0,"height_pt":792.0,"rotation":"Deg90"}"#;
        let g: PageGeometry = serde_json::from_str(legacy_json).unwrap();
        let tr =
            PageTransform::from_geometry(&g).expect("旧形态零盒应从宽高重建，而不是 Degenerate");
        assert_eq!(
            (tr.media().x0, tr.media().y0, tr.media().x1, tr.media().y1),
            (0.0, 0.0, 612.0, 792.0)
        );
        // 重建后变换可用（往返不死）
        let p = tr.screen_to_user(10.0, 10.0, 96.0);
        let (bx, by) = tr.user_to_screen(p, 96.0);
        assert!((bx - 10.0).abs() < 0.01 && (by - 10.0).abs() < 0.01);
        // 两部数据全零 → 构造器 typed 错误（不猜）
        let broken = PageGeometry {
            width_pt: 0.0,
            height_pt: 0.0,
            rotation: Rotation::Deg0,
            media_box: PageBox::default(),
            crop_box: None,
        };
        assert!(matches!(
            PageTransform::from_geometry(&broken),
            Err(TransformError::Degenerate { .. })
        ));
    }
}
