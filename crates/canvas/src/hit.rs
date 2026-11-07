//! Canvas 几何命中测试（#407）——「点哪里选中哪个」。
//!
//! # 语义
//!
//! - **所见即所得**：命中几何与渲染同源（`shape_from_object` 的
//!   `(Shape, Transform)` → `transform_shape` 世界系）——渲染画什么，
//!   命中测什么（含渲染面的既有简化：旋转变换后矩形按轴对齐包围
//!   盒测——与 SVG/光栅两后端的实际绘制一致）。
//! - **分层**：按绘制序**自顶向下**（后绘制的先命中）。当前绘制序 =
//!   `objects()`（ID 升序）；#402 z 序合入后切换 `objects_z_ordered()`
//!   （一处调用点，PR 已登记跟进项）。
//! - **组**：命中成员 = 选中组（多组嵌套取绘制序最上的组）；组本身
//!   无几何（渲染不画），框选亦不含组——框选是几何对象面。
//! - **边/笔迹**：点到线段距离 ≤ max(tolerance, 描边半宽)——点击
//!   落在描带上即命中（与视觉宽度一致）。
//! - **边界闭**：点恰在矩形边上 = 命中；距离恰等于容差 = 命中。
//! - **敌意面不 panic**：非有限点/容差钳制为安全值；非法框选矩形
//!   （非有限/非正尺寸）返回空。
//!
//! # 接口（issue #407 指定签名）
//!
//! [`hit_test`]：单点命中（顶到底第一个）。[`hit_test_rect`]：框选
//! （与对象包围盒**含描边外扩**相交的全部对象，绘制序返回）。

use crate::render::{
    shape_bounds, shape_bounds_padded, shape_from_object, transform_shape, FlattenedPath, Point,
    Rect, Shape, Transform,
};
use crate::{CanvasObject, CanvasSession, ObjectKind};

/// 命中结果（issue 指定枚举——组/边/普通对象三分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitTarget {
    /// 空白（或敌意输入）。
    None,
    /// 单对象（Node / Stroke）。
    Object(String),
    /// 组（点在组内任一成员上）。
    Group(String),
    /// 边。
    Edge(String),
}

/// 路径离散容差（命中面精度——粗于渲染的 0.1 即可，点击不需要亚像素）。
const HIT_FLATTEN_TOL: f64 = 0.5;

/// 单点命中：按绘制序自顶向下，返回第一个被点中的目标。
///
/// `tolerance` 是线类（边/笔迹/路径）的点击容差（域单位）；非有限或
/// 负值钳制为 0。面类（矩形/椭圆/图片/文本盒）用包含判定（闭边界），
/// 不受容差影响。
pub fn hit_test(session: &CanvasSession, x: f64, y: f64, tolerance: f64) -> HitTarget {
    if !x.is_finite() || !y.is_finite() {
        return HitTarget::None;
    }
    let tol = if tolerance.is_finite() {
        tolerance.max(0.0)
    } else {
        0.0
    };
    let objects = session.objects();
    let p = Point::new(x, y);
    // 自顶向下：后绘制（序列末）先命中。
    for o in objects.iter().rev() {
        if let Some((shape, tf)) = shape_from_object(o, &objects) {
            if shape_hit(&shape, &tf, p, tol) {
                return resolve_target(&objects, o);
            }
        }
    }
    HitTarget::None
}

/// 框选：与对象包围盒（含描边外扩）相交的全部对象，绘制序返回。
///
/// 组无几何不入框选（成员各自入）；非法矩形（非有限/非正尺寸）→ 空。
pub fn hit_test_rect(session: &CanvasSession, rect: Rect) -> Vec<String> {
    let valid = rect.x.is_finite()
        && rect.y.is_finite()
        && rect.w.is_finite()
        && rect.h.is_finite()
        && rect.w > 0.0
        && rect.h > 0.0;
    if !valid {
        return Vec::new();
    }
    let objects = session.objects();
    let mut out = Vec::new();
    for o in &objects {
        if let Some((shape, tf)) = shape_from_object(o, &objects) {
            if let Some(b) = shape_bounds_padded(&shape, &tf) {
                if rects_intersect(&b, &rect) {
                    out.push(o.id.clone());
                }
            }
        }
    }
    out
}

// ── 目标解析 ───────────────────────────────────────────────────

/// 命中对象 → 目标：组优先（成员被点 = 选组，多组取绘制序最上），
/// 其余按种类分（Edge 单列——消费侧常对边做专门处理）。
fn resolve_target(objects: &[CanvasObject], hit: &CanvasObject) -> HitTarget {
    for g in objects.iter().rev() {
        if g.kind == ObjectKind::Group && g.members.contains(&hit.id) {
            return HitTarget::Group(g.id.clone());
        }
    }
    match hit.kind {
        ObjectKind::Edge => HitTarget::Edge(hit.id.clone()),
        _ => HitTarget::Object(hit.id.clone()),
    }
}

// ── 几何 ───────────────────────────────────────────────────────

/// 单形状命中：变换到世界系后按种类测。
fn shape_hit(shape: &Shape, tf: &Transform, p: Point, tol: f64) -> bool {
    let m = tf.to_affine(shape.pivot());
    let world = transform_shape(shape, &m);
    match &world {
        // 面类：闭边界包含（transform_shape 对矩形/图片产出轴对齐盒——
        // 与渲染绘制同口径）
        Shape::Rect { rect, .. } => point_in_rect(p, rect),
        Shape::Image { rect, .. } => point_in_rect(p, rect),
        Shape::Ellipse { rect, .. } => point_in_ellipse(p, rect),
        // 文本：估算布局盒（与 shape_bounds 同口径——粗排版估宽）
        Shape::Text { .. } => {
            shape_bounds(&world, &Transform::IDENTITY).is_some_and(|r| point_in_rect(p, &r))
        }
        // 线类：点到线段距离 ≤ max(容差, 描边半宽)——描带可点
        Shape::Line { from, to, style } => {
            point_segment_distance(p, *from, *to) <= tol.max(stroke_half_width(style.width))
        }
        Shape::Path { cmds, style } => {
            let threshold = tol.max(stroke_half_width(
                style.stroke.map(|s| s.width).unwrap_or(0.0),
            ));
            let flat = FlattenedPath::flatten(cmds, HIT_FLATTEN_TOL);
            flat.subpaths.iter().any(|sp| {
                sp.points
                    .windows(2)
                    .any(|w| point_segment_distance(p, w[0], w[1]) <= threshold)
            })
        }
    }
}

/// 描边半宽（非有限/非正值 → 0——敌意样式面不 panic）。
fn stroke_half_width(width: f64) -> f64 {
    if width.is_finite() && width > 0.0 {
        width * 0.5
    } else {
        0.0
    }
}

/// 点在矩形内（闭边界：恰在边上 = 命中）。
fn point_in_rect(p: Point, r: &Rect) -> bool {
    p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h
}

/// 点在椭圆内（闭边界；退化椭圆（零半径）不命中）。
fn point_in_ellipse(p: Point, r: &Rect) -> bool {
    let rx = r.w * 0.5;
    let ry = r.h * 0.5;
    if rx <= 0.0 || ry <= 0.0 {
        return false;
    }
    let dx = (p.x - (r.x + rx)) / rx;
    let dy = (p.y - (r.y + ry)) / ry;
    dx * dx + dy * dy <= 1.0
}

/// 点到线段的最短距离。
fn point_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    if len2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len2).clamp(0.0, 1.0);
    dist(p, Point::new(a.x + abx * t, a.y + aby * t))
}

fn dist(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// 矩形相交（闭：贴边算相交——框选恰好碰到对象边缘即选中）。
fn rects_intersect(a: &Rect, b: &Rect) -> bool {
    a.x <= b.x + b.w && b.x <= a.x + a.w && a.y <= b.y + b.h && b.y <= a.y + a.h
}
