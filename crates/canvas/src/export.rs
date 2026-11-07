//! Canvas 会话层导出入口（#418）。
//!
//! 渲染层已有 SVG/PNG 后端与自由函数（[`crate::render::export_svg`] /
//! [`crate::render::export_png`]，输入是 [`CanvasRendering`]），但宿主
//! 持有的是 [`CanvasSession`]——本模块把「会话 → 渲染模型 → 导出」
//! 一步接通，产品面无需感知渲染模型的装配细节。
//!
//! # 语义要点
//!
//! - **整幅导出**：viewBox = 全部对象的内容包围盒（含描边外扩，
//!   与渲染层口径一致）；零对象退化为 1x1（渲染层既有语义）。
//! - **区域导出**：viewBox = 给定 [`Rect`]；只保留与区域**相交**的
//!   对象（跨界对象保留，出界部分由 SVG 根视口裁剪——outer `<svg>`
//!   默认 overflow:hidden）。对象→图元的转换先全量做完再过滤：
//!   Edge 端点 / Group 成员解析需要完整对象面。
//! - **PNG 导出**：尺寸 = 内容**几何**包围盒 × scale（1:1 等比映射，
//!   视口 = 几何包围盒——与渲染层 PNG 口径一致，粗描边不推偏）。
//! - **背景**：会话模型无背景概念（背景是渲染模型层的装配选项），
//!   会话级导出恒为透明背景；需要背景的调用方走渲染层
//!   `CanvasRendering::with_background`。
//! - **永不 panic**（渲染面纪律）：非法区域走「宁缺毋错」返回空内容
//!   的合法 SVG；非法 scale 走 `InvalidInput` 错误。

use std::fmt::Write as _;

use crate::render::{
    canvas_bounds_geometry, export_png as render_export_png, export_svg as render_export_svg, num,
    shape_bounds_padded, CanvasRenderer, CanvasRendering, Rect, SvgRenderer,
};
use crate::{CanvasError, CanvasSession};

impl CanvasSession {
    /// 导出整个画布为 SVG 字符串。
    ///
    /// 等价于 `render::export_svg(&CanvasRendering::from_session(self))`
    /// （props 协议驱动对象→图元，悬空边等敌意读模型自动跳过）。
    pub fn export_svg(&self) -> String {
        render_export_svg(&CanvasRendering::from_session(self))
    }

    /// 导出指定区域为 SVG：viewBox = `rect`，只含与区域相交的对象。
    ///
    /// 非法区域（字段非有限 / 尺寸非正）走「宁缺毋错」：返回空内容的
    /// 合法 SVG（viewBox 0 0 1 1），不 panic——区域来自用户框选或
    /// 持久化面时，陈旧/敌意值是常态。
    pub fn export_svg_region(&self, rect: Rect) -> String {
        if !region_valid(&rect) {
            return wrap_svg(Rect::new(0.0, 0.0, 1.0, 1.0), "");
        }
        // 先全量转图元再过滤：Edge 端点 / Group 成员解析需要完整
        // 对象面（from_objects 内部以 all 引用解析），按对象子集转换
        // 会让跨界边悬空。
        let full = CanvasRendering::from_objects(&self.objects());
        let mut r = SvgRenderer::new();
        for (s, tf) in &full.shapes {
            if let Some(b) = shape_bounds_padded(s, tf) {
                if rects_overlap(&b, &rect) {
                    r.draw(s, tf);
                }
            }
        }
        wrap_svg(rect, &r.into_body())
    }

    /// 导出为 PNG（RGBA8）：尺寸 = 内容几何包围盒 × `scale`。
    ///
    /// - `scale` 非有限或 ≤ 0 → [`CanvasError::InvalidInput`]；
    /// - 尺寸四舍五入、至少 1 像素；空会话包围盒为 1x1 → 产出
    ///   scale×scale 像素；
    /// - 边长 / 像素数上限由渲染层统一守门（超限报错）。
    pub fn export_png(&self, scale: f32) -> Result<Vec<u8>, CanvasError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(CanvasError::InvalidInput(format!(
                "导出缩放非法（scale={scale}，须为有限正数）"
            )));
        }
        let c = CanvasRendering::from_session(self);
        c.check()?;
        let b = canvas_bounds_geometry(&c);
        let k = f64::from(scale);
        // f64→u32 饱和转换：天文数字 scale 安全落入渲染层上限守门，
        // 不会回绕成小尺寸。
        let w = ((b.w * k).round() as u32).max(1);
        let h = ((b.h * k).round() as u32).max(1);
        render_export_png(&c, (w, h))
    }
}

// ── 内部工具 ───────────────────────────────────────────────────

/// 区域合法性：全字段有限 + 尺寸为正（零尺寸 viewBox 退化，一并拒收）。
fn region_valid(r: &Rect) -> bool {
    r.x.is_finite()
        && r.y.is_finite()
        && r.w.is_finite()
        && r.h.is_finite()
        && r.w > 0.0
        && r.h > 0.0
}

/// 开区间相交（贴边不算——零面积接触无可见内容）。
fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// SVG 外壳：viewBox = 给定视口 + 内容体（与渲染层 export_svg 同构，
/// 仅视口来源不同——那里是内容包围盒，这里是指定区域）。
fn wrap_svg(view: Rect, body: &str) -> String {
    let mut out = String::with_capacity(320 + body.len());
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" width=\"{w}\" height=\"{h}\" viewBox=\"{x} {y} {w} {h}\">",
        w = num(view.w),
        h = num(view.h),
        x = num(view.x),
        y = num(view.y)
    );
    out.push_str(body);
    out.push('\n');
    out.push_str("</svg>\n");
    out
}
