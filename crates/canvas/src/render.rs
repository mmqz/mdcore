use std::fmt::Write as _;

use crate::{CanvasError, CanvasObject, CanvasSession, ObjectKind};

// 常量
const MAX_CURVE_SEGMENTS: usize = 64;
const FLATTEN_TOLERANCE: f64 = 0.1;
const MAX_PNG_SIDE: u32 = 65_536;
const MAX_PNG_PIXELS: u64 = 64 * 1024 * 1024;

// 几何
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn check(&self, field: &'static str) -> Result<(), CanvasError> {
        check_pair(self.x, self.y, field)
    }

    pub(crate) fn len(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for Point {
    type Output = Point;
    fn mul(self, k: f64) -> Point {
        Point::new(self.x * k, self.y * k)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    fn check(&self, field: &'static str) -> Result<(), CanvasError> {
        check_pair(self.x, self.y, field)?;
        check_pair(self.w, self.h, field)?;
        if self.w < 0.0 || self.h < 0.0 {
            return Err(CanvasError::InvalidInput(format!(
                "{field}: 尺寸不可为负（w={}, h={}）",
                self.w, self.h
            )));
        }
        Ok(())
    }
}

fn check_pair(a: f64, b: f64, field: &'static str) -> Result<(), CanvasError> {
    if !a.is_finite() || !b.is_finite() {
        return Err(CanvasError::NonFiniteGeometry { field });
    }
    Ok(())
}

// 样式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    pub fn is_transparent(&self) -> bool {
        self.a == 0
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    fn svg_opacity(&self) -> Option<f64> {
        if self.a == 255 {
            None
        } else {
            Some(self.a as f64 / 255.0)
        }
    }

    fn over_coverage(&self, dst: Color, coverage: f64) -> Color {
        let sa = (self.a as f64 / 255.0) * coverage.clamp(0.0, 1.0);
        if sa <= 0.0 {
            return dst;
        }
        let da = dst.a as f64 / 255.0;
        let out_a = sa + da * (1.0 - sa);
        if out_a <= 0.0 {
            return Color::TRANSPARENT;
        }
        let ch = |s: u8, d: u8| -> u8 {
            let v = (s as f64 * sa + d as f64 * da * (1.0 - sa)) / out_a;
            v.round().clamp(0.0, 255.0) as u8
        };
        Color::rgba(
            ch(self.r, dst.r),
            ch(self.g, dst.g),
            ch(self.b, dst.b),
            (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    Miter,
    #[default]
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrokeDash {
    pub dash: u16,
    pub gap: u16,
}

impl StrokeDash {
    pub const SOLID: StrokeDash = StrokeDash { dash: 0, gap: 0 };

    pub const fn new(dash: u16, gap: u16) -> StrokeDash {
        StrokeDash { dash, gap }
    }

    fn is_solid(&self) -> bool {
        self.dash == 0
    }

    fn check(&self) -> Result<(), CanvasError> {
        if self.dash != 0 && self.gap == 0 {
            return Err(CanvasError::InvalidInput(
                "虚线模式 gap 不可为 0".into(),
            ));
        }
        Ok(())
    }

    fn svg(&self) -> Option<String> {
        if self.is_solid() {
            None
        } else {
            Some(format!("{},{}", self.dash, self.gap))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeStyle {
    pub color: Color,
    pub width: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub dash: StrokeDash,
}

impl StrokeStyle {
    pub const fn new(color: Color, width: f64) -> StrokeStyle {
        StrokeStyle {
            color,
            width,
            cap: LineCap::Butt,
            join: LineJoin::Round,
            dash: StrokeDash::SOLID,
        }
    }

    pub const fn with_cap(mut self, cap: LineCap) -> StrokeStyle {
        self.cap = cap;
        self
    }

    pub const fn with_join(mut self, join: LineJoin) -> StrokeStyle {
        self.join = join;
        self
    }

    pub const fn with_dash(mut self, dash: StrokeDash) -> StrokeStyle {
        self.dash = dash;
        self
    }

    fn check(&self) -> Result<(), CanvasError> {
        check_pair(self.width, self.width, "stroke.width")?;
        if self.width < 0.0 {
            return Err(CanvasError::InvalidInput(format!(
                "stroke.width 不可为负（{}）",
                self.width
            )));
        }
        self.dash.check()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeStyle {
    pub fill: Option<Color>,
    pub stroke: Option<StrokeStyle>,
}

impl ShapeStyle {
    pub const NONE: ShapeStyle = ShapeStyle {
        fill: None,
        stroke: None,
    };

    pub const fn fill(color: Color) -> ShapeStyle {
        ShapeStyle {
            fill: Some(color),
            stroke: None,
        }
    }

    pub const fn stroke(stroke: StrokeStyle) -> ShapeStyle {
        ShapeStyle {
            fill: None,
            stroke: Some(stroke),
        }
    }

    pub const fn both(fill: Color, stroke: StrokeStyle) -> ShapeStyle {
        ShapeStyle {
            fill: Some(fill),
            stroke: Some(stroke),
        }
    }

    fn check(&self, field: &'static str) -> Result<(), CanvasError> {
        if let Some(s) = &self.stroke {
            s.check().map_err(|e| match e {
                CanvasError::InvalidInput(m) => CanvasError::InvalidInput(format!("{field}: {m}")),
                other => other,
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAnchor {
    #[default]
    Start,
    Middle,
    End,
}

impl TextAnchor {
    fn svg(self) -> &'static str {
        match self {
            TextAnchor::Start => "start",
            TextAnchor::Middle => "middle",
            TextAnchor::End => "end",
        }
    }

    fn offset_factor(self) -> f64 {
        match self {
            TextAnchor::Start => 0.0,
            TextAnchor::Middle => 0.5,
            TextAnchor::End => 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    pub size: f64,
    pub anchor: TextAnchor,
    pub family: Option<String>,
    pub bold: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            color: Color::BLACK,
            size: 16.0,
            anchor: TextAnchor::Start,
            family: None,
            bold: false,
        }
    }
}

impl TextStyle {
    pub fn new(size: f64, color: Color) -> TextStyle {
        TextStyle {
            size,
            color,
            ..Default::default()
        }
    }

    fn check(&self) -> Result<(), CanvasError> {
        check_pair(self.size, self.size, "text.size")?;
        if self.size < 0.0 {
            return Err(CanvasError::InvalidInput(format!(
                "text.size 不可为负（{}）",
                self.size
            )));
        }
        Ok(())
    }
}

// ── 路径命令 ───────────────────────────────────────────────────

/// 路径命令（SVG 路径语法的最小同构子集）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    MoveTo(Point),
    LineTo(Point),
    /// 二次贝塞尔。
    QuadTo { ctrl: Point, to: Point },
    /// 三次贝塞尔。
    CubicTo { c1: Point, c2: Point, to: Point },
    Close,
}

impl PathCmd {
    fn check(&self, field: &'static str) -> Result<(), CanvasError> {
        match self {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => p.check(field),
            PathCmd::QuadTo { ctrl, to } => {
                ctrl.check(field)?;
                to.check(field)
            }
            PathCmd::CubicTo { c1, c2, to } => {
                c1.check(field)?;
                c2.check(field)?;
                to.check(field)
            }
            PathCmd::Close => Ok(()),
        }
    }

    /// SVG 路径数据片段。
    fn svg(&self) -> String {
        match self {
            PathCmd::MoveTo(p) => format!("M{} {}", num(p.x), num(p.y)),
            PathCmd::LineTo(p) => format!("L{} {}", num(p.x), num(p.y)),
            PathCmd::QuadTo { ctrl, to } => {
                format!("Q{} {} {} {}", num(ctrl.x), num(ctrl.y), num(to.x), num(to.y))
            }
            PathCmd::CubicTo { c1, c2, to } => format!(
                "C{} {} {} {} {} {}",
                num(c1.x),
                num(c1.y),
                num(c2.x),
                num(c2.y),
                num(to.x),
                num(to.y)
            ),
            PathCmd::Close => "Z".to_string(),
        }
    }
}

/// 路径光栅化后的折线（子路径集）。
pub(crate) struct FlattenedPath {
    pub subpaths: Vec<FlattenedSubpath>,
}

pub(crate) struct FlattenedSubpath {
    pub points: Vec<Point>,
    pub closed: bool,
}

impl FlattenedPath {
    /// 把路径命令离散成折线（贝塞尔自适应分段）。
    pub(crate) fn flatten(cmds: &[PathCmd], tolerance: f64) -> FlattenedPath {
        let mut subpaths: Vec<FlattenedSubpath> = Vec::new();
        let mut cur: Vec<Point> = Vec::new();
        let mut pos = Point::new(0.0, 0.0);
        for cmd in cmds {
            match *cmd {
                PathCmd::MoveTo(p) => {
                    if cur.len() > 1 {
                        subpaths.push(FlattenedSubpath {
                            points: std::mem::take(&mut cur),
                            closed: false,
                        });
                    } else {
                        cur.clear();
                    }
                    pos = p;
                    cur.push(p);
                }
                PathCmd::LineTo(p) => {
                    if cur.is_empty() {
                        cur.push(pos);
                    }
                    cur.push(p);
                    pos = p;
                }
                PathCmd::QuadTo { ctrl, to } => {
                    if cur.is_empty() {
                        cur.push(pos);
                    }
                    let n = quad_segments(pos, ctrl, to, tolerance);
                    for i in 1..=n {
                        let t = i as f64 / n as f64;
                        cur.push(quad_point(pos, ctrl, to, t));
                    }
                    pos = to;
                }
                PathCmd::CubicTo { c1, c2, to } => {
                    if cur.is_empty() {
                        cur.push(pos);
                    }
                    let n = cubic_segments(pos, c1, c2, to, tolerance);
                    for i in 1..=n {
                        let t = i as f64 / n as f64;
                        cur.push(cubic_point(pos, c1, c2, to, t));
                    }
                    pos = to;
                }
                PathCmd::Close => {
                    if cur.len() > 1 {
                        subpaths.push(FlattenedSubpath {
                            points: std::mem::take(&mut cur),
                            closed: true,
                        });
                    } else {
                        cur.clear();
                    }
                    cur.push(pos);
                }
            }
        }
        if cur.len() > 1 {
            subpaths.push(FlattenedSubpath {
                points: cur,
                closed: false,
            });
        }
        FlattenedPath { subpaths }
    }
}

fn quad_point(p0: Point, p1: Point, p2: Point, t: f64) -> Point {
    let mt = 1.0 - t;
    p0 * (mt * mt) + p1 * (2.0 * mt * t) + p2 * (t * t)
}

fn cubic_point(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let mt = 1.0 - t;
    p0 * (mt * mt * mt) + p1 * (3.0 * mt * mt * t) + p2 * (3.0 * mt * t * t) + p3 * (t * t * t)
}

/// 自适应分段数：控制多边形长度 / 容差 的平方根近似。
fn curve_segments(cp_len: f64, tolerance: f64) -> usize {
    let t = tolerance.max(0.01);
    let n = (cp_len / t).sqrt().ceil();
    let n = if n.is_finite() {
        n as usize
    } else {
        MAX_CURVE_SEGMENTS
    };
    n.clamp(1, MAX_CURVE_SEGMENTS)
}

fn quad_segments(p0: Point, p1: Point, p2: Point, tol: f64) -> usize {
    curve_segments((p1 - p0).len() + (p2 - p1).len(), tol)
}

fn cubic_segments(p0: Point, p1: Point, p2: Point, p3: Point, tol: f64) -> usize {
    curve_segments((p1 - p0).len() + (p2 - p1).len() + (p3 - p2).len(), tol)
}

// ── 形状 ───────────────────────────────────────────────────────

/// 位图压缩格式（SVG 内联需要；光栅后端只画占位框，见模块诚实登记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageMime {
    #[default]
    Png,
    Jpeg,
    Gif,
}

impl ImageMime {
    fn svg_mime(self) -> &'static str {
        match self {
            ImageMime::Png => "image/png",
            ImageMime::Jpeg => "image/jpeg",
            ImageMime::Gif => "image/gif",
        }
    }
}

/// 绘图原语（画布局部坐标）：issue #125 的 6 种图形。
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line {
        from: Point,
        to: Point,
        style: StrokeStyle,
    },
    Rect {
        rect: Rect,
        style: ShapeStyle,
    },
    Ellipse {
        rect: Rect,
        style: ShapeStyle,
    },
    Path {
        cmds: Vec<PathCmd>,
        style: ShapeStyle,
    },
    Text {
        pos: Point,
        text: String,
        style: TextStyle,
    },
    Image {
        rect: Rect,
        data: Vec<u8>,
        mime: ImageMime,
    },
}

impl Shape {
    /// 有限性/合法性校验（渲染入口的第一道门）。
    pub fn check(&self) -> Result<(), CanvasError> {
        match self {
            Shape::Line { from, to, style } => {
                from.check("line.from")?;
                to.check("line.to")?;
                style.check()
            }
            Shape::Rect { rect, style } | Shape::Ellipse { rect, style } => {
                rect.check("shape.rect")?;
                style.check("shape.style")
            }
            Shape::Path { cmds, style } => {
                for c in cmds {
                    c.check("path.cmd")?;
                }
                style.check("path.style")
            }
            Shape::Text { pos, style, .. } => {
                pos.check("text.pos")?;
                style.check()
            }
            Shape::Image { rect, .. } => rect.check("image.rect"),
        }
    }

    /// 形状自身的支点（恒等判定 / 缺省 origin 用）。
    pub(crate) fn pivot(&self) -> Point {
        match self {
            Shape::Line { from, to, .. } => {
                Point::new((from.x + to.x) * 0.5, (from.y + to.y) * 0.5)
            }
            Shape::Rect { rect, .. } | Shape::Ellipse { rect, .. } | Shape::Image { rect, .. } => {
                rect.center()
            }
            Shape::Text { pos, .. } => *pos,
            Shape::Path { cmds, .. } => {
                for c in cmds {
                    if let PathCmd::MoveTo(p) = c {
                        return *p;
                    }
                }
                Point::new(0.0, 0.0)
            }
        }
    }
}

// ── 变换 ───────────────────────────────────────────────────────

/// 2x3 仿射矩阵（x1 = a·x + c·y + e，y1 = b·x + d·y + f）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Affine::IDENTITY
    }

    pub fn apply(&self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    /// 先 rhs 后 self（矩阵合成）。
    pub fn then(&self, rhs: &Affine) -> Affine {
        Affine {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            e: self.a * rhs.e + self.c * rhs.f + self.e,
            f: self.b * rhs.e + self.d * rhs.f + self.f,
        }
    }

    /// SVG matrix(a b c d e f) 片段。
    pub fn svg(&self) -> String {
        format!(
            "matrix({} {} {} {} {} {})",
            num(self.a),
            num(self.b),
            num(self.c),
            num(self.d),
            num(self.e),
            num(self.f)
        )
    }

    /// 均匀缩放系数（非均匀/镜像取绝对值几何平均：线宽各向同性）。
    pub fn scale_factor(&self) -> f64 {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        let sy = (self.c * self.c + self.d * self.d).sqrt();
        if sx > 0.0 && sy > 0.0 {
            (sx * sy).sqrt()
        } else {
            sx.max(sy)
        }
    }
}

/// 渲染变换（局部坐标到输出坐标）：M = T · R · S，缩放/旋转绕 origin。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translate: (f64, f64),
    /// 缩放系数（小于 0 为镜像；1.0 = 原尺寸）。
    pub scale: f64,
    /// 旋转（弧度，屏幕坐标系下顺时针为正）。
    pub rotate: f64,
    /// 缩放/旋转支点。None = 形状支点（包围盒中心）。
    pub origin: Option<Point>,
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        translate: (0.0, 0.0),
        scale: 1.0,
        rotate: 0.0,
        origin: None,
    };

    pub fn translate(x: f64, y: f64) -> Transform {
        Transform {
            translate: (x, y),
            ..Transform::IDENTITY
        }
    }

    pub fn scale(scale: f64) -> Transform {
        Transform {
            scale,
            ..Transform::IDENTITY
        }
    }

    pub fn rotate(radians: f64) -> Transform {
        Transform {
            rotate: radians,
            ..Transform::IDENTITY
        }
    }

    pub fn with_translate(mut self, x: f64, y: f64) -> Transform {
        self.translate = (x, y);
        self
    }

    pub fn with_origin(mut self, p: Point) -> Transform {
        self.origin = Some(p);
        self
    }

    pub fn check(&self) -> Result<(), CanvasError> {
        check_pair(self.translate.0, self.translate.1, "transform.translate")?;
        check_pair(self.scale, self.rotate, "transform.scale/rotate")
    }

    pub fn is_identity(&self) -> bool {
        self.translate == (0.0, 0.0) && self.scale == 1.0 && self.rotate == 0.0
    }

    /// 合成输出矩阵。pivot_fallback 是 origin = None 时的支点。
    pub fn to_affine(&self, pivot_fallback: Point) -> Affine {
        let o = self.origin.unwrap_or(pivot_fallback);
        let (sin, cos) = self.rotate.sin_cos();
        let s = self.scale;
        let sc = Affine {
            a: s,
            b: 0.0,
            c: 0.0,
            d: s,
            e: o.x - s * o.x,
            f: o.y - s * o.y,
        };
        let r = Affine {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: o.x - (cos * o.x - sin * o.y),
            f: o.y - (sin * o.x + cos * o.y),
        };
        let t = Affine {
            e: self.translate.0,
            f: self.translate.1,
            ..Affine::IDENTITY
        };
        t.then(&r.then(&sc))
    }
}

/// 把矩阵折进形状几何（线宽按缩放系数放大）。
pub(crate) fn transform_shape(shape: &Shape, m: &Affine) -> Shape {
    if m.is_identity() {
        return shape.clone();
    }
    let k = m.scale_factor();
    match shape {
        Shape::Line { from, to, style } => {
            let mut style = *style;
            style.width *= k;
            Shape::Line {
                from: m.apply(*from),
                to: m.apply(*to),
                style,
            }
        }
        Shape::Rect { rect, style } => Shape::Rect {
            rect: transform_rect(rect, m),
            style: scale_style(style, k),
        },
        Shape::Ellipse { rect, style } => Shape::Ellipse {
            rect: transform_rect(rect, m),
            style: scale_style(style, k),
        },
        Shape::Path { cmds, style } => Shape::Path {
            cmds: cmds
                .iter()
                .map(|c| match *c {
                    PathCmd::MoveTo(p) => PathCmd::MoveTo(m.apply(p)),
                    PathCmd::LineTo(p) => PathCmd::LineTo(m.apply(p)),
                    PathCmd::QuadTo { ctrl, to } => PathCmd::QuadTo {
                        ctrl: m.apply(ctrl),
                        to: m.apply(to),
                    },
                    PathCmd::CubicTo { c1, c2, to } => PathCmd::CubicTo {
                        c1: m.apply(c1),
                        c2: m.apply(c2),
                        to: m.apply(to),
                    },
                    PathCmd::Close => PathCmd::Close,
                })
                .collect(),
            style: scale_style(style, k),
        },
        Shape::Text { pos, text, style } => {
            let mut style = style.clone();
            style.size *= k;
            Shape::Text {
                pos: m.apply(*pos),
                text: text.clone(),
                style,
            }
        }
        Shape::Image { rect, data, mime } => Shape::Image {
            rect: transform_rect(rect, m),
            data: data.clone(),
            mime: *mime,
        },
    }
}

/// 矩形经矩阵变换后的轴对齐包围盒。
pub(crate) fn transform_rect(r: &Rect, m: &Affine) -> Rect {
    let pts = [
        m.apply(Point::new(r.x, r.y)),
        m.apply(Point::new(r.x + r.w, r.y)),
        m.apply(Point::new(r.x, r.y + r.h)),
        m.apply(Point::new(r.x + r.w, r.y + r.h)),
    ];
    let min_x = pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let min_y = pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_x = pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let max_y = pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn scale_style(style: &ShapeStyle, k: f64) -> ShapeStyle {
    ShapeStyle {
        fill: style.fill,
        stroke: style.stroke.map(|mut s| {
            s.width *= k;
            s
        }),
    }
}

// ── 渲染目标抽象 ───────────────────────────────────────────────

/// 渲染目标（issue #125 的 CanvasRenderer 契约）。
///
/// 契约：draw 把一个形状画出来；tf 是局部到输出的变换。后端可以忽略
/// 不支持的细节，但必须登记（见模块文档的诚实登记）。
pub trait CanvasRenderer {
    fn draw(&mut self, shape: &Shape, tf: &Transform);

    /// 顺序绘制一批形状（每形状独立变换）。
    fn draw_all(&mut self, items: &[(Shape, Transform)]) {
        for (s, tf) in items {
            self.draw(s, tf);
        }
    }
}

// ── 画布渲染模型 ───────────────────────────────────────────────

/// 可渲染画布：形状序列 + 背景。
///
/// session 的对象模型（Node/Edge/Group）没有图元概念，所以本结构是
/// 渲染面的输入契约：既能从会话直接构建（from_session，走 props 属性
/// 面），也能由调用方手工装配任意图元。
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasRendering {
    pub shapes: Vec<(Shape, Transform)>,
    pub background: Color,
}

impl Default for CanvasRendering {
    fn default() -> Self {
        CanvasRendering {
            shapes: Vec::new(),
            background: Color::TRANSPARENT,
        }
    }
}

impl CanvasRendering {
    pub fn new() -> CanvasRendering {
        CanvasRendering::default()
    }

    pub fn with_background(mut self, bg: Color) -> CanvasRendering {
        self.background = bg;
        self
    }

    pub fn push(&mut self, shape: Shape) -> &mut Self {
        self.shapes.push((shape, Transform::IDENTITY));
        self
    }

    pub fn push_with(&mut self, shape: Shape, tf: Transform) -> &mut Self {
        self.shapes.push((shape, tf));
        self
    }

    pub fn extend_shapes<I: IntoIterator<Item = Shape>>(&mut self, shapes: I) -> &mut Self {
        for s in shapes {
            self.push(s);
        }
        self
    }

    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// 全部形状的校验（NaN/inf/负尺寸的第一道门）。
    pub fn check(&self) -> Result<(), CanvasError> {
        for (i, (s, tf)) in self.shapes.iter().enumerate() {
            let map = |e: CanvasError| match e {
                CanvasError::InvalidInput(m) => {
                    CanvasError::InvalidInput(format!("shapes[{i}]: {m}"))
                }
                other => other,
            };
            s.check().map_err(map)?;
            tf.check().map_err(map)?;
        }
        Ok(())
    }

    /// 从 session 会话取渲染模型（对象到图元）。
    pub fn from_session(session: &CanvasSession) -> CanvasRendering {
        CanvasRendering::from_objects(&session.objects())
    }

    /// 从读模型快照构建（会话或纯数据皆可）。
    pub fn from_objects(objects: &[CanvasObject]) -> CanvasRendering {
        let mut out = CanvasRendering::default();
        for o in objects {
            if let Some((shape, tf)) = shape_from_object(o, objects) {
                out.shapes.push((shape, tf));
            }
        }
        out
    }
}

// ── 对象到图元 ─────────────────────────────────────────────────

/// 对象到图元（props 驱动，缺省按 ObjectKind 推导）。
///
/// props 协议（画布域开放属性面到渲染语义）：
///
/// - shape：rect / ellipse / text / image / path（缺省 Node 为 rect，Edge 为 line）
/// - label 或 text：文本（shape=text）
/// - fill 或 stroke：#rrggbb 或 rgb(...) 或调色板名
/// - stroke_width：数值；dash：d,g（虚线）
/// - rotation：度数（顺时针为正）；opacity：0..1
/// - font_size / font / bold：字号 / 字族 / 1 或 true
/// - points：x,y x,y ...（至少 2 点，shape=path）
/// - image_data：十六进制字节（shape=image）
///
/// 未知键与非法值忽略并回退缺省（渲染面永不 panic：敌意/陈旧属性面是
/// 常态）。Group 对象本身不画（成员各自成图元）。
///
/// Edge 几何：按两端 Node 的矩形中心求连线（端点缺失/悬空则不画；并发
/// 删除留下的悬空边在读模型里合法，渲染面跳过而不是 panic）。
pub fn shape_from_object(o: &CanvasObject, all: &[CanvasObject]) -> Option<(Shape, Transform)> {
    let props = &o.props;
    let rotation = props
        .get("rotation")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
        .to_radians();
    let opacity = props
        .get("opacity")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let dim = |c: Color| Color {
        a: scale_alpha(c, opacity),
        ..c
    };

    let color_of = |key: &str, default: Color| -> Color {
        match props.get(key) {
            Some(v) => parse_color(v).unwrap_or(default),
            None => default,
        }
    };
    let stroke_width = props
        .get("stroke_width")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(1.0);
    let dash = props
        .get("dash")
        .and_then(|v| parse_dash(v))
        .unwrap_or(StrokeDash::SOLID);

    let rect = Rect::new(o.x, o.y, o.w, o.h);
    let kind = props
        .get("shape")
        .map(|s| s.as_str())
        .unwrap_or(match o.kind {
            ObjectKind::Node => "rect",
            ObjectKind::Edge => "line",
            ObjectKind::Group => "none",
        });

    let (shape, pivot) = match (o.kind, kind) {
        (ObjectKind::Group, _) | (_, "none") => return None,
        (ObjectKind::Edge, _) => {
            let center_of = |id: &str| -> Option<Point> {
                all.iter()
                    .find(|c| c.id == id && c.kind == ObjectKind::Node)
                    .map(|c| Rect::new(c.x, c.y, c.w, c.h).center())
            };
            let from = center_of(o.from.as_deref()?)?;
            let to = center_of(o.to.as_deref()?)?;
            let style = StrokeStyle {
                color: dim(color_of("stroke", Color::rgb(0x33, 0x33, 0x33))),
                width: stroke_width,
                cap: LineCap::Butt,
                join: LineJoin::Round,
                dash,
            };
            (
                Shape::Line { from, to, style },
                Point::new((from.x + to.x) * 0.5, (from.y + to.y) * 0.5),
            )
        }
        (_, "ellipse") => {
            let style = ShapeStyle {
                fill: Some(dim(color_of("fill", Color::rgb(0xdd, 0xee, 0xff)))),
                stroke: Some(StrokeStyle {
                    color: dim(color_of("stroke", Color::rgb(0x22, 0x44, 0x66))),
                    width: stroke_width,
                    cap: LineCap::Butt,
                    join: LineJoin::Round,
                    dash,
                }),
            };
            (Shape::Ellipse { rect, style }, rect.center())
        }
        (_, "text") => {
            let size = props
                .get("font_size")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(14.0);
            let style = TextStyle {
                color: dim(color_of("fill", Color::BLACK)),
                size,
                anchor: TextAnchor::Start,
                family: props.get("font").cloned(),
                bold: props
                    .get("bold")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false),
            };
            let text = props
                .get("label")
                .or_else(|| props.get("text"))
                .cloned()
                .unwrap_or_default();
            (
                Shape::Text {
                    pos: Point::new(o.x, o.y),
                    text,
                    style,
                },
                Point::new(o.x, o.y),
            )
        }
        (_, "image") => {
            let data = props
                .get("image_data")
                .and_then(|hex| decode_hex(hex))
                .unwrap_or_default();
            let mime = match props.get("image_mime").map(|s| s.as_str()) {
                Some("jpeg") | Some("jpg") => ImageMime::Jpeg,
                Some("gif") => ImageMime::Gif,
                _ => ImageMime::Png,
            };
            (Shape::Image { rect, data, mime }, rect.center())
        }
        (_, "path") => {
            let pts = props
                .get("points")
                .map(|v| parse_points(v))
                .unwrap_or_default();
            if pts.len() < 2 {
                return None;
            }
            let mut cmds = Vec::with_capacity(pts.len() + 1);
            cmds.push(PathCmd::MoveTo(pts[0]));
            for p in &pts[1..] {
                cmds.push(PathCmd::LineTo(*p));
            }
            let style = ShapeStyle {
                fill: props
                    .get("fill")
                    .map(|_| dim(color_of("fill", Color::WHITE))),
                stroke: Some(StrokeStyle {
                    color: dim(color_of("stroke", Color::BLACK)),
                    width: stroke_width,
                    cap: LineCap::Round,
                    join: LineJoin::Round,
                    dash,
                }),
            };
            let pivot = centroid(&pts);
            (Shape::Path { cmds, style }, pivot)
        }
        _ => {
            let style = ShapeStyle {
                fill: Some(dim(color_of("fill", Color::rgb(0xf5, 0xf7, 0xfa)))),
                stroke: Some(StrokeStyle {
                    color: dim(color_of("stroke", Color::rgb(0x2f, 0x5d, 0x8a))),
                    width: stroke_width,
                    cap: LineCap::Butt,
                    join: LineJoin::Round,
                    dash,
                }),
            };
            (Shape::Rect { rect, style }, rect.center())
        }
    };

    let tf = Transform {
        translate: (0.0, 0.0),
        scale: 1.0,
        rotate: rotation,
        origin: Some(pivot),
    };
    Some((shape, tf))
}

fn centroid(pts: &[Point]) -> Point {
    if pts.is_empty() {
        return Point::new(0.0, 0.0);
    }
    let mut x = 0.0;
    let mut y = 0.0;
    for p in pts {
        x += p.x;
        y += p.y;
    }
    Point::new(x / pts.len() as f64, y / pts.len() as f64)
}

fn scale_alpha(c: Color, k: f64) -> u8 {
    ((c.a as f64 * k).round()).clamp(0.0, 255.0) as u8
}

/// 颜色解析：#rrggbb / #rgb / #rrggbbaa / rgb(r,g,b) / 调色板名。
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let v = u16::from_str_radix(hex, 16).ok()?;
                let r = ((v >> 8) & 0xf) as u8 * 17;
                let g = ((v >> 4) & 0xf) as u8 * 17;
                let b = (v & 0xf) as u8 * 17;
                Some(Color::rgb(r, g, b))
            }
            6 => {
                let v = u32::from_str_radix(hex, 16).ok()?;
                Some(Color::rgb(
                    ((v >> 16) & 0xff) as u8,
                    ((v >> 8) & 0xff) as u8,
                    (v & 0xff) as u8,
                ))
            }
            8 => {
                let v = u32::from_str_radix(hex, 16).ok()?;
                Some(Color::rgba(
                    ((v >> 24) & 0xff) as u8,
                    ((v >> 16) & 0xff) as u8,
                    ((v >> 8) & 0xff) as u8,
                    (v & 0xff) as u8,
                ))
            }
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 3 {
            let c = |p: &str| p.parse::<u8>().ok();
            return Some(Color::rgb(c(parts[0])?, c(parts[1])?, c(parts[2])?));
        }
        return None;
    }
    named_color(s)
}

fn named_color(s: &str) -> Option<Color> {
    let c = match s.to_ascii_lowercase().as_str() {
        "black" => Color::BLACK,
        "white" => Color::WHITE,
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "blue" => Color::rgb(0, 0, 255),
        "gray" | "grey" => Color::rgb(128, 128, 128),
        "orange" => Color::rgb(255, 165, 0),
        "yellow" => Color::rgb(255, 255, 0),
        "purple" => Color::rgb(128, 0, 128),
        "teal" => Color::rgb(0, 128, 128),
        _ => return None,
    };
    Some(c)
}

fn parse_dash(s: &str) -> Option<StrokeDash> {
    let mut it = s.split(',');
    let d = it.next()?.trim().parse::<u16>().ok()?;
    let g = it.next()?.trim().parse::<u16>().ok()?;
    if d == 0 || g == 0 {
        return None;
    }
    Some(StrokeDash::new(d, g))
}

fn parse_points(s: &str) -> Vec<Point> {
    s.split_whitespace()
        .filter_map(|pair| {
            let mut it = pair.split(',');
            let x = it.next()?.trim().parse::<f64>().ok()?;
            let y = it.next()?.trim().parse::<f64>().ok()?;
            if x.is_finite() && y.is_finite() {
                Some(Point::new(x, y))
            } else {
                None
            }
        })
        .collect()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

// ── 数字与字符串工具 ───────────────────────────────────────────

/// 数字格式化：最多 4 位小数，去尾零，负零归一成 0。
pub(crate) fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let mut s = format!("{v:.4}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    if s == "-0" {
        s = "0".to_string();
    }
    s
}

/// XML 文本转义（只转义 & < >）。
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// XML 属性值转义（文本转义 + 引号 + 不可见控制符）。
fn xml_attr_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    out
}

/// 标准 base64（带 = 补齐）。
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// ── SVG 后端 ───────────────────────────────────────────────────

/// SVG 渲染器（纯字符串，零依赖）。
#[derive(Debug, Default)]
pub struct SvgRenderer {
    body: String,
}

impl SvgRenderer {
    pub fn new() -> SvgRenderer {
        SvgRenderer::default()
    }

    pub fn into_body(self) -> String {
        self.body
    }

    fn style_attrs(out: &mut String, style: &ShapeStyle) {
        match &style.fill {
            Some(c) => {
                let _ = write!(out, " fill=\"{}\"", c.to_hex());
                if let Some(o) = c.svg_opacity() {
                    let _ = write!(out, " fill-opacity=\"{}\"", num(o));
                }
            }
            None => out.push_str(" fill=\"none\""),
        }
        match &style.stroke {
            Some(s) => {
                let _ = write!(
                    out,
                    " stroke=\"{}\" stroke-width=\"{}\"",
                    s.color.to_hex(),
                    num(s.width)
                );
                if let Some(o) = s.color.svg_opacity() {
                    let _ = write!(out, " stroke-opacity=\"{}\"", num(o));
                }
                let cap = match s.cap {
                    LineCap::Butt => "butt",
                    LineCap::Round => "round",
                    LineCap::Square => "square",
                };
                let _ = write!(out, " stroke-linecap=\"{cap}\"");
                let join = match s.join {
                    LineJoin::Miter => "miter",
                    LineJoin::Round => "round",
                    LineJoin::Bevel => "bevel",
                };
                let _ = write!(out, " stroke-linejoin=\"{join}\"");
                if let Some(d) = s.dash.svg() {
                    let _ = write!(out, " stroke-dasharray=\"{d}\"");
                }
            }
            None => out.push_str(" stroke=\"none\""),
        }
    }

    fn draw_shape(&mut self, shape: &Shape) {
        match shape {
            Shape::Line { from, to, style } => {
                let _ = write!(
                    self.body,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"",
                    num(from.x),
                    num(from.y),
                    num(to.x),
                    num(to.y)
                );
                Self::style_attrs(
                    &mut self.body,
                    &ShapeStyle {
                        fill: None,
                        stroke: Some(*style),
                    },
                );
                self.body.push_str("/>");
            }
            Shape::Rect { rect, style } => {
                let _ = write!(
                    self.body,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
                    num(rect.x),
                    num(rect.y),
                    num(rect.w),
                    num(rect.h)
                );
                Self::style_attrs(&mut self.body, style);
                self.body.push_str("/>");
            }
            Shape::Ellipse { rect, style } => {
                let c = rect.center();
                let _ = write!(
                    self.body,
                    "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"",
                    num(c.x),
                    num(c.y),
                    num(rect.w * 0.5),
                    num(rect.h * 0.5)
                );
                Self::style_attrs(&mut self.body, style);
                self.body.push_str("/>");
            }
            Shape::Path { cmds, style } => {
                let d: Vec<String> = cmds.iter().map(|c| c.svg()).collect();
                let _ = write!(self.body, "<path d=\"{}\"", d.join(" "));
                Self::style_attrs(&mut self.body, style);
                self.body.push_str("/>");
            }
            Shape::Text { pos, text, style } => {
                let _ = write!(
                    self.body,
                    "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\"",
                    num(pos.x),
                    num(pos.y),
                    num(style.size),
                    style.color.to_hex()
                );
                if let Some(o) = style.color.svg_opacity() {
                    let _ = write!(self.body, " fill-opacity=\"{}\"", num(o));
                }
                if style.anchor != TextAnchor::Start {
                    let _ = write!(self.body, " text-anchor=\"{}\"", style.anchor.svg());
                }
                if let Some(f) = &style.family {
                    let _ = write!(self.body, " font-family=\"{}\"", xml_attr_escape(f));
                }
                if style.bold {
                    self.body.push_str(" font-weight=\"bold\"");
                }
                let _ = write!(self.body, ">{}</text>", xml_escape(text));
            }
            Shape::Image { rect, data, mime } => {
                if data.is_empty() {
                    let _ = write!(
                        self.body,
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#999999\" stroke-dasharray=\"4,3\"/>",
                        num(rect.x),
                        num(rect.y),
                        num(rect.w),
                        num(rect.h)
                    );
                    return;
                }
                let _ = write!(
                    self.body,
                    "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" xlink:href=\"data:{};base64,",
                    num(rect.x),
                    num(rect.y),
                    num(rect.w),
                    num(rect.h),
                    mime.svg_mime()
                );
                self.body.push_str(&base64_encode(data));
                self.body.push_str("\"/>");
            }
        }
    }
}

impl CanvasRenderer for SvgRenderer {
    fn draw(&mut self, shape: &Shape, tf: &Transform) {
        if tf.is_identity() {
            self.draw_shape(shape);
            return;
        }
        let m = tf.to_affine(shape.pivot());
        self.draw_shape(&transform_shape(shape, &m));
    }
}

/// SVG 导出（一次性入口）。
///
/// viewBox 取自全部形状的包围盒（含描边外扩）；零形状退化为 1x1。
/// 背景不透明时画铺满的背景矩形。
pub fn export_svg(canvas: &CanvasRendering) -> String {
    let b = canvas_bounds(canvas);
    let mut out = String::with_capacity(320 + canvas.shapes.len() * 96);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" width=\"{w}\" height=\"{h}\" viewBox=\"{x} {y} {w} {h}\">",
        w = num(b.w),
        h = num(b.h),
        x = num(b.x),
        y = num(b.y)
    );
    if !canvas.background.is_transparent() {
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"{}/>",
            num(b.x),
            num(b.y),
            num(b.w),
            num(b.h),
            canvas.background.to_hex(),
            match canvas.background.svg_opacity() {
                Some(o) => format!(" fill-opacity=\"{}\"", num(o)),
                None => String::new(),
            }
        );
    }
    let mut r = SvgRenderer::new();
    for (s, tf) in &canvas.shapes {
        r.draw(s, tf);
    }
    out.push_str(&r.into_body());
    out.push('\n');
    out.push_str("</svg>\n");
    out
}

// ── 包围盒 ─────────────────────────────────────────────────────

/// 内容包围盒（**含**描边外扩半宽；零形状 = 1x1 于原点）。SVG viewBox 用。
pub fn canvas_bounds(canvas: &CanvasRendering) -> Rect {
    canvas_bounds_impl(canvas, true)
}

/// 内容**几何**包围盒（不含描边外扩）。PNG 视口与缩放用。
pub fn canvas_bounds_geometry(canvas: &CanvasRendering) -> Rect {
    canvas_bounds_impl(canvas, false)
}

fn canvas_bounds_impl(canvas: &CanvasRendering, padded: bool) -> Rect {
    let mut acc: Option<Rect> = None;
    for (shape, tf) in &canvas.shapes {
        if let Some(b) = shape_bounds_impl(shape, tf, padded) {
            acc = Some(match acc {
                None => b,
                Some(a) => union(&a, &b),
            });
        }
    }
    acc.unwrap_or_else(|| Rect::new(0.0, 0.0, 1.0, 1.0))
}

fn union(a: &Rect, b: &Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let x2 = (a.x + a.w).max(b.x + b.w);
    let y2 = (a.y + a.h).max(b.y + b.h);
    Rect::new(x, y, x2 - x, y2 - y)
}

/// 单形状**几何**包围盒（已应用变换、不含描边外扩）。
///
/// 与 shape_bounds_padded 分开是有代价的教训：PNG 视口必须用几何包围盒
/// 算缩放——用带描边外扩的包围盒会让视口从 -hw 开始，整体内容被推偏
/// hw 像素，粗描边看起来"消失"。SVG 的 viewBox 才该带外扩。
pub fn shape_bounds(shape: &Shape, tf: &Transform) -> Option<Rect> {
    shape_bounds_impl(shape, tf, false)
}

/// 单形状包围盒（含描边外扩半宽；SVG viewBox 用）。
pub fn shape_bounds_padded(shape: &Shape, tf: &Transform) -> Option<Rect> {
    shape_bounds_impl(shape, tf, true)
}

fn shape_bounds_impl(shape: &Shape, tf: &Transform, padded: bool) -> Option<Rect> {
    let m = tf.to_affine(shape.pivot());
    let k = m.scale_factor();
    let pad = |style: &ShapeStyle| -> f64 {
        if !padded {
            return 0.0;
        }
        style.stroke.map(|s| s.width * k * 0.5).unwrap_or(0.0)
    };
    let b = match shape {
        Shape::Line { from, to, style } => {
            let a1 = m.apply(*from);
            let a2 = m.apply(*to);
            let p = style.width * k * 0.5;
            Rect::new(
                a1.x.min(a2.x) - p,
                a1.y.min(a2.y) - p,
                (a1.x - a2.x).abs() + 2.0 * p,
                (a1.y - a2.y).abs() + 2.0 * p,
            )
        }
        Shape::Rect { rect, style } | Shape::Ellipse { rect, style } => {
            let r = transform_rect(rect, &m);
            let p = pad(style);
            Rect::new(r.x - p, r.y - p, r.w + 2.0 * p, r.h + 2.0 * p)
        }
        Shape::Path { cmds, style } => {
            let fp = FlattenedPath::flatten(cmds, FLATTEN_TOLERANCE);
            let mut x0 = f64::INFINITY;
            let mut y0 = f64::INFINITY;
            let mut x1 = f64::NEG_INFINITY;
            let mut y1 = f64::NEG_INFINITY;
            let mut any = false;
            for sp in &fp.subpaths {
                for p in &sp.points {
                    let q = m.apply(*p);
                    x0 = x0.min(q.x);
                    y0 = y0.min(q.y);
                    x1 = x1.max(q.x);
                    y1 = y1.max(q.y);
                    any = true;
                }
            }
            if !any {
                return None;
            }
            let p = pad(style);
            Rect::new(x0 - p, y0 - p, x1 - x0 + 2.0 * p, y1 - y0 + 2.0 * p)
        }
        Shape::Text { pos, text, style } => {
            let size = style.size * k;
            let w = text.chars().count() as f64 * size * 0.6;
            let a = m.apply(*pos);
            let dx = w * style.anchor.offset_factor();
            Rect::new(a.x - dx, a.y - size * 0.8, w, size * 1.05)
        }
        Shape::Image { rect, .. } => transform_rect(rect, &m),
    };
    Some(b)
}

// ── 位图 ───────────────────────────────────────────────────────

/// 8 位 RGBA 位图（非预乘、行优先）。
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// 新建位图并用 background 铺满。
    pub fn new(width: u32, height: u32, background: Color) -> Bitmap {
        let n = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(n * 4);
        for _ in 0..n {
            pixels.extend_from_slice(&[background.r, background.g, background.b, background.a]);
        }
        Bitmap {
            width,
            height,
            pixels,
        }
    }

    pub fn get(&self, x: u32, y: u32) -> Color {
        let i = ((y as usize * self.width as usize) + x as usize) * 4;
        Color::rgba(
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        )
    }

    /// 单像素 src over dst 混合（覆盖率缩放源 alpha）。
    fn blend_at(&mut self, x: u32, y: u32, src: Color, coverage: f64) {
        if coverage <= 0.0 || src.a == 0 {
            return;
        }
        let i = ((y as usize * self.width as usize) + x as usize) * 4;
        let dst = Color::rgba(
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        );
        let out = src.over_coverage(dst, coverage);
        self.pixels[i] = out.r;
        self.pixels[i + 1] = out.g;
        self.pixels[i + 2] = out.b;
        self.pixels[i + 3] = out.a;
    }
}

// ── 光栅器 ─────────────────────────────────────────────────────

/// 扫描线光栅器（画到内部 Bitmap，再由 into_png 编码）。
///
/// 算法：每个像素在纵向采样 SUBSAMPLES 次（采样行 y = py + (j+0.5)/N），
/// 横向对覆盖区间做精确积分。线段/边框用闭区间 [c-hw, c+hw] 判定
/// （保证 1px 线可见，且落在半像素上时不丢），填充多边形内部用半开
/// 判定（相邻行不重不漏）。
pub struct RasterRenderer {
    bmp: Bitmap,
    /// 输出像素 0 对应的画布偏移。
    off_x: f64,
    off_y: f64,
    /// 输出缩放（画布单位到像素）。
    scale: f64,
}

impl RasterRenderer {
    /// 新建光栅器：输出 size 像素，画布视口从 view 开始按 scale 放大。
    pub fn new(size: (u32, u32), view: Rect, scale: f64, background: Color) -> RasterRenderer {
        RasterRenderer {
            bmp: Bitmap::new(size.0, size.1, background),
            off_x: -view.x * scale,
            off_y: -view.y * scale,
            scale,
        }
    }

    pub fn bitmap(&self) -> &Bitmap {
        &self.bmp
    }

    /// 完成编码（消费自身）：PNG 字节。
    pub fn into_png(self) -> Result<Vec<u8>, CanvasError> {
        encode_png(&self.bmp)
    }

}

/// 采样行循环：对区间 [y0,y1] 覆盖的每个像素行回调，给出采样行 y 与
/// **该行的纵向覆盖率**（0..1）。
///
/// 纵向覆盖率是逐行解析算出来的，不是"数几条采样线"：一整行都落在
/// 区间内时覆盖率必须是 1.0，否则实心填充会永远只有 0.75 的亮度
/// （本站踩过：4px 黑描边画出 80/255 的灰）。采样行细分的意义只在
/// 边缘那一行——那里用 SUBSAMPLES 级细分近似纵向部分覆盖。
fn sample_rows(buf_h: u32, y0: f64, y1: f64, mut f: impl FnMut(u32, f64, f64)) {
    let (lo, hi) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return;
    }
    // 亚像素区间保护：区间窄于 1px 时（0.5px 宽的横线就是这样），按面积算
    // 纵向覆盖率只有 0.25，在 256 色阶下几乎看不见。抬到最小 1 像素宽，
    // 并保证**至少有一整行**被完整覆盖——否则线会在两个半行之间淡到看不出。
    let (lo, hi) = if hi - lo < 1.0 {
        let mid = (lo + hi) * 0.5;
        (mid - 0.5, mid + 0.5)
    } else {
        (lo, hi)
    };
    let top = lo.floor().max(0.0);
    let bot = hi.ceil().min(buf_h as f64);
    if bot <= top {
        return;
    }
    let mut py = top as u32;
    while (py as f64) < bot && py < buf_h {
        let y_lo = py as f64;
        let y_hi = y_lo + 1.0;
        // 纵向覆盖率 = 该像素行与区间的实际重叠长度（解析，不是数采样点）。
        let cov = (y_hi.min(hi) - y_lo.max(lo)).clamp(0.0, 1.0);
        if cov > 0.0 {
            f(py, y_lo + 0.5, cov);
        }
        py += 1;
    }
}
/// 落在像素边界上时，边界内侧那一列额外补到满覆盖。
///
/// 为什么必须这样：线宽 1 的描边区间恰好是一列宽 [x, x+1]。纯半开积分
/// 下相邻列各得 0.5，1px 边框整条变灰（实测：本该 0 的黑边画出 167 的
/// 灰）；矩形四角还会因为两段各贡献 0.5 而拼出更暗的角点。闭区间语义
/// 保证「线宽 1 恰好占一列」，同时粗线边缘仍由部分重叠得到自然抗锯齿。
fn blend_span(buf: &mut Bitmap, y: u32, p0: f64, p1: f64, weight: f64, color: Color) {
    if weight <= 0.0 || color.a == 0 {
        return;
    }
    let (a, b) = if p0 <= p1 { (p0, p1) } else { (p1, p0) };
    if !a.is_finite() || !b.is_finite() || b < a {
        return;
    }
    let w = buf.width as f64;
    if b < 0.0 || a >= w {
        return;
    }
    let eps = 1e-9;
    if (b - a).abs() <= eps {
        // 零宽区间：只按闭区间原点归属一格（避免整段消失）。
        let xi = if (a - a.floor() - 0.5).abs() <= 0.5 { a.floor() } else { a.ceil() };
        if xi >= 0.0 && xi < w {
            buf.blend_at(xi as u32, y, color, weight);
        }
        return;
    }
    let start = (a - eps).max(0.0).floor() as u32;
    let end = ((b + eps).min(w).ceil() as u32).min(buf.width);
    let mut x = start;
    while x < end {
        let l = (x as f64).max(a);
        let r = ((x + 1) as f64).min(b);
        let mut cov = (r - l).max(0.0);
        // 闭区间端点补偿：端点贴住某列的内侧边界时，该列补到满覆盖。
        let xf = x as f64;
        if (a - xf).abs() <= eps || (a - (xf + 1.0)).abs() <= eps {
            cov = 1.0;
        }
        if (b - xf).abs() <= eps || (b - (xf + 1.0)).abs() <= eps {
            cov = 1.0;
        }
        if cov > 0.0 {
            buf.blend_at(x, y, color, cov.clamp(0.0, 1.0) * weight);
        }
        x += 1;
    }
}
/// 以线段为中心、hw 为半径的胶囊：逐采样行解析求出与胶囊的水平区间。
/// 轴向部分用二次式解 t 的可行区间（不夹到 [0,1]，端帽交给圆补），
/// 端帽用圆方程补两侧——这样 1px 竖线恰好占一列宽，不会因为投影
/// 近似而比同宽度横线粗（漏掉端帽半径这个坑本站踩过：竖线画成 3 行）。
fn raster_line(buf: &mut Bitmap, a: (f64, f64), b: (f64, f64), hw: f64, color: Color) {
    if hw <= 0.0 || color.a == 0 {
        return;
    }
    // 亚像素线（宽 < 1px）必须有下限覆盖：几何上 [y-hw, y+hw] 可能整条落在
    // 两条采样线之间，按面积算覆盖率是 0，线就"消失"了。渲染器的通行做法
    // 是保证最小半像素覆盖——这里把 half-width 抬到 0.25（≥ 1/4 像素覆盖，
    // 在 256 色阶下肉眼可见而不显脏）。
    let hw = hw.max(0.25);
    let (ax, ay) = a;
    let (bx, by) = b;
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    sample_rows(buf.height, ay.min(by) - hw, ay.max(by) + hw, |py, sy, weight| {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        // 轴向部分：t 轴上距离 <= hw 的区间（半平面判定，不含端帽）。
        if len2 > 0.0 {
            let a2 = len2;
            let b2 = 2.0 * (dx * ax + dy * (ay - sy));
            let c2 = ax * ax + (ay - sy) * (ay - sy) - hw * hw;
            let disc = b2 * b2 - 4.0 * a2 * c2;
            // 判别式恰为 0 = 采样行正好相切胶囊边缘，t0 == t1 退化成一点；
            // 这时必须整条轴段都算覆盖（否则 0.5px 线整条消失，本站踩过）。
            if (0.0..1e-12).contains(&disc) {
                let xa = ax.min(bx);
                let xb = ax.max(bx);
                lo = lo.min(xa);
                hi = hi.max(xb);
            } else if disc > 0.0 {
                let sq = disc.sqrt();
                let t0 = (-b2 - sq) / (2.0 * a2);
                let t1 = (-b2 + sq) / (2.0 * a2);
                if t1 > 0.0 && t0 < 1.0 {
                    // 需要的是该行在原始直线上的 x 范围（不夹到 [0,1]），
                    // 端帽由下面的圆补——把 t 夹住会让端帽区变成矩形。
                    let xa = ax + t0 * dx;
                    let xb = ax + t1 * dx;
                    lo = lo.min(xa.min(xb));
                    hi = hi.max(xa.max(xb));
                }
            }
        }
        // 端帽（圆；退化零长线段也靠这里出像素）。
        for (cx, cy) in [(ax, ay), (bx, by)] {
            let dv = sy - cy;
            if dv.abs() < hw {
                let half = (hw * hw - dv * dv).sqrt();
                lo = lo.min(cx - half);
                hi = hi.max(cx + half);
            }
        }
        if hi > lo {
            blend_span(buf, py, lo, hi, weight, color);
        }
    });
}

/// 折线描边（首尾相接的线段；closed 时闭合）。
fn raster_polyline(buf: &mut Bitmap, pts: &[(f64, f64)], closed: bool, hw: f64, color: Color) {
    if pts.len() < 2 {
        return;
    }
    for w in pts.windows(2) {
        raster_line(buf, w[0], w[1], hw, color);
    }
    if closed {
        raster_line(buf, pts[pts.len() - 1], pts[0], hw, color);
    }
}

/// 轴对齐矩形填充。
fn raster_rect_fill(buf: &mut Bitmap, p0: (f64, f64), p1: (f64, f64), color: Color) {
    if color.a == 0 {
        return;
    }
    let x0 = p0.0.min(p1.0);
    let x1 = p0.0.max(p1.0);
    let y0 = p0.1.min(p1.1);
    let y1 = p0.1.max(p1.1);
    sample_rows(buf.height, y0, y1, |py, _sy, weight| {
        blend_span(buf, py, x0, x1, weight, color);
    });
}

/// 轴对齐矩形描边（矩形环带）。
///
/// 不用四条 raster_line 拼：四段的端帽会在四个角重叠叠加，1px 描边的
/// 角上会被涂两遍（比边上深一倍）。这里按"环带"逐采样行直接求左右两条
/// 竖边的覆盖区间——一条扫描线一次搞定，角上不重复。
fn raster_rect_stroke(buf: &mut Bitmap, p0: (f64, f64), p1: (f64, f64), hw: f64, color: Color) {
    let x0 = p0.0.min(p1.0);
    let x1 = p0.0.max(p1.0);
    let y0 = p0.1.min(p1.1);
    let y1 = p0.1.max(p1.1);
    let (iy0, iy1) = (y0 + hw, y1 - hw);
    sample_rows(buf.height, y0 - hw, y1 + hw, |py, sy, weight| {
        // 上下两条横边：整行按矩形横向区间铺满。
        if sy < iy0 || sy > iy1 {
            blend_span(buf, py, x0 - hw, x1 + hw, weight, color);
            return;
        }
        // 左右两条竖边：各自一条区间，互不相交。
        blend_span(buf, py, x0 - hw, x0 + hw, weight, color);
        blend_span(buf, py, x1 - hw, x1 + hw, weight, color);
    });
}

/// 椭圆填充（解析水平覆盖）。
fn raster_ellipse_fill(buf: &mut Bitmap, c: (f64, f64), rx: f64, ry: f64, color: Color) {
    if color.a == 0 || rx <= 0.0 || ry <= 0.0 {
        return;
    }
    sample_rows(buf.height, c.1 - ry, c.1 + ry, |py, sy, weight| {
        let dy = (sy - c.1) / ry;
        if dy.abs() >= 1.0 {
            return;
        }
        let half = rx * (1.0 - dy * dy).sqrt();
        blend_span(buf, py, c.0 - half, c.0 + half, weight, color);
    });
}

/// 椭圆描边：环带 [r-hw, r+hw]，用径向距离判定。
fn raster_ellipse_stroke(
    buf: &mut Bitmap,
    c: (f64, f64),
    rx: f64,
    ry: f64,
    hw: f64,
    color: Color,
) {
    if color.a == 0 || rx <= 0.0 || ry <= 0.0 {
        return;
    }
    sample_rows(buf.height, c.1 - ry - hw, c.1 + ry + hw, |py, sy, weight| {
        let dy = sy - c.1;
        // 解 (dx/rx)^2 + (dy/ry)^2 = 1 得到该行的水平半宽
        let k = 1.0 - (dy / ry) * (dy / ry);
        if k <= 0.0 {
            // 上下极点的圆帽
            let half = hw;
            blend_span(buf, py, c.0 - half, c.0 + half, weight, color);
            return;
        }
        let outer = (rx + hw) * k.sqrt();
        let inner = (rx - hw).max(0.0) * k.sqrt();
        blend_span(buf, py, c.0 - outer, c.0 - inner, weight, color);
        if inner > 0.0 {
            blend_span(buf, py, c.0 + inner, c.0 + outer, weight, color);
        } else {
            blend_span(buf, py, c.0 - outer, c.0 + outer, weight, color);
        }
    });
}

/// 多边形非零环绕填充（多子路径一次扫描）。
fn raster_polygons_nonzero(buf: &mut Bitmap, polys: &[Vec<(f64, f64)>], color: Color) {
    if color.a == 0 {
        return;
    }
    let mut y0 = f64::INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for pts in polys {
        for p in pts {
            y0 = y0.min(p.1);
            y1 = y1.max(p.1);
        }
    }
    if !y0.is_finite() || !y1.is_finite() {
        return;
    }
    sample_rows(buf.height, y0, y1, |py, sy, weight| {
        let mut xs: Vec<(f64, i32)> = Vec::new();
        for pts in polys {
            let n = pts.len();
            if n < 2 {
                continue;
            }
            for i in 0..n {
                let a = pts[i];
                let b = pts[(i + 1) % n];
                if (a.1 <= sy) == (b.1 <= sy) {
                    continue;
                }
                let t = (sy - a.1) / (b.1 - a.1);
                let x = a.0 + t * (b.0 - a.0);
                let dir = if b.1 > a.1 { 1 } else { -1 };
                xs.push((x, dir));
            }
        }
        if xs.is_empty() {
            return;
        }
        xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut wind = 0;
        let mut span_start = 0.0;
        for (x, d) in xs {
            let prev = wind;
            wind += d;
            if prev == 0 && wind != 0 {
                span_start = x;
            } else if prev != 0 && wind == 0 {
                blend_span(buf, py, span_start, x, weight, color);
            }
        }
    });
}

impl CanvasRenderer for RasterRenderer {
    fn draw(&mut self, shape: &Shape, tf: &Transform) {
        let m = tf.to_affine(shape.pivot());
        let scale = self.scale;
        let off = (self.off_x, self.off_y);
        let px = |p: Point| {
            let q = m.apply(p);
            (q.x * scale + off.0, q.y * scale + off.1)
        };
        let k = m.scale_factor() * scale;
        match shape {
            Shape::Line { from, to, style } => {
                if style.color.a == 0 || style.width <= 0.0 {
                    return;
                }
                let a = px(*from);
                let b = px(*to);
                let hw = style.width * k * 0.5;
                if style.dash.is_solid() {
                    raster_line(&mut self.bmp, a, b, hw, style.color);
                } else {
                    let d = style.dash.dash as f64 * scale;
                    let g = style.dash.gap as f64 * scale;
                    let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                    let (ux, uy) = if len > 0.0 {
                        ((b.0 - a.0) / len, (b.1 - a.1) / len)
                    } else {
                        (1.0, 0.0)
                    };
                    let mut t = 0.0;
                    while t < len {
                        let t2 = (t + d).min(len);
                        raster_line(
                            &mut self.bmp,
                            (a.0 + ux * t, a.1 + uy * t),
                            (a.0 + ux * t2, a.1 + uy * t2),
                            hw,
                            style.color,
                        );
                        t = t2 + g;
                    }
                }
            }
            Shape::Rect { rect, style } => {
                // 轴对齐快路径只在**无旋转**时成立。旋转过的矩形四个角不再
                // 共轴，用轴对齐矩形去画会退化成一条线（本站踩过：旋转 45 度
                // 的方块被画成一根竖线）。
                let rotated = !m.is_identity() && m.b != 0.0;
                if rotated {
                    let quad = [
                        px(Point::new(rect.x, rect.y)),
                        px(Point::new(rect.x + rect.w, rect.y)),
                        px(Point::new(rect.x + rect.w, rect.y + rect.h)),
                        px(Point::new(rect.x, rect.y + rect.h)),
                    ];
                    if let Some(fill) = style.fill {
                        raster_polygons_nonzero(&mut self.bmp, &[quad.to_vec()], fill);
                    }
                    if let Some(st) = style.stroke {
                        if st.width > 0.0 && st.color.a > 0 {
                            raster_polyline(&mut self.bmp, &quad, true, st.width * k * 0.5, st.color);
                        }
                    }
                } else {
                    let p0 = px(Point::new(rect.x, rect.y));
                    let p1 = px(Point::new(rect.x + rect.w, rect.y + rect.h));
                    if let Some(fill) = style.fill {
                        raster_rect_fill(&mut self.bmp, p0, p1, fill);
                    }
                    if let Some(st) = style.stroke {
                        if st.width > 0.0 && st.color.a > 0 {
                            raster_rect_stroke(&mut self.bmp, p0, p1, st.width * k * 0.5, st.color);
                        }
                    }
                }
            }
            Shape::Ellipse { rect, style } => {
                let c = px(rect.center());
                let rx = rect.w * 0.5 * k;
                let ry = rect.h * 0.5 * k;
                if let Some(fill) = style.fill {
                    raster_ellipse_fill(&mut self.bmp, c, rx, ry, fill);
                }
                if let Some(st) = style.stroke {
                    if st.width > 0.0 && st.color.a > 0 {
                        raster_ellipse_stroke(&mut self.bmp, c, rx, ry, st.width * k * 0.5, st.color);
                    }
                }
            }
            Shape::Path { cmds, style } => {
                let fp = FlattenedPath::flatten(cmds, FLATTEN_TOLERANCE / scale.max(1e-6));
                let polys: Vec<Vec<(f64, f64)>> = fp
                    .subpaths
                    .iter()
                    .map(|sp| sp.points.iter().map(|p| px(*p)).collect())
                    .collect();
                if let Some(fill) = style.fill {
                    raster_polygons_nonzero(&mut self.bmp, &polys, fill);
                }
                if let Some(st) = style.stroke {
                    if st.width > 0.0 && st.color.a > 0 {
                        let hw = st.width * k * 0.5;
                        for (i, pts) in polys.iter().enumerate() {
                            raster_polyline(&mut self.bmp, pts, fp.subpaths[i].closed, hw, st.color);
                        }
                    }
                }
            }
            Shape::Text { pos, text, style } => {
                // 诚实登记：光栅后端不做字形（不加载系统字体、不做 hinting）。
                // 这里画一个**带描边的空白框**而不是实心块——实心块会让人误
                // 以为「文字已经渲染了只是颜色不对」，空框一眼就能看出是占位。
                // 要真字形请走 SVG（浏览器渲染）。
                if text.is_empty() || style.size <= 0.0 || style.color.a == 0 {
                    return;
                }
                let size = style.size * k;
                let w = text.chars().count() as f64 * size * 0.6;
                let anchor = px(*pos);
                let x0 = anchor.0 - w * style.anchor.offset_factor();
                let y0 = anchor.1 - size * 0.8;
                let (x1, y1) = (x0 + w, y0 + size);
                let hw = (size * 0.03).clamp(0.2, 0.5);
                raster_rect_stroke(&mut self.bmp, (x0, y0), (x1, y1), hw, style.color);
                // 中间一条基线：让占位框看得出"这是一行字"。
                raster_line(
                    &mut self.bmp,
                    (x0, y0 + size * 0.72),
                    (x1, y0 + size * 0.72),
                    hw * 0.6,
                    style.color,
                );
            }
            Shape::Image { rect, .. } => {
                let p0 = px(Point::new(rect.x, rect.y));
                let p1 = px(Point::new(rect.x + rect.w, rect.y + rect.h));
                let color = Color::rgb(0x99, 0x99, 0x99);
                let d = 4.0 * scale;
                let g = 3.0 * scale;
                dashed_h(&mut self.bmp, p0.0, p1.0, p0.1, d, g, color);
                dashed_h(&mut self.bmp, p0.0, p1.0, p1.1, d, g, color);
                dashed_v(&mut self.bmp, p0.1, p1.1, p0.0, d, g, color);
                dashed_v(&mut self.bmp, p0.1, p1.1, p1.0, d, g, color);
            }
        }
    }
}

fn dashed_h(buf: &mut Bitmap, x0: f64, x1: f64, y: f64, d: f64, g: f64, color: Color) {
    let mut t = x0.min(x1);
    let end = x0.max(x1);
    while t < end {
        let t2 = (t + d).min(end);
        raster_line(buf, (t, y), (t2, y), 0.5, color);
        t = t2 + g;
    }
}

fn dashed_v(buf: &mut Bitmap, y0: f64, y1: f64, x: f64, d: f64, g: f64, color: Color) {
    let mut t = y0.min(y1);
    let end = y0.max(y1);
    while t < end {
        let t2 = (t + d).min(end);
        raster_line(buf, (x, t), (x, t2), 0.5, color);
        t = t2 + g;
    }
}

// ── PNG 编码 ───────────────────────────────────────────────────

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// 位图编码为 PNG（8 位 RGBA，非隔行，zlib 存储块 + 自适应过滤）。
///
/// 零依赖实现：CRC32 / Adler32 / deflate(stored) / chunk 全在本文件。
pub fn encode_png(bmp: &Bitmap) -> Result<Vec<u8>, CanvasError> {
    if bmp.width == 0 || bmp.height == 0 {
        return Err(CanvasError::InvalidInput(
            "PNG 尺寸不可为零（宽或高 = 0）".into(),
        ));
    }
    if bmp.width > MAX_PNG_SIDE || bmp.height > MAX_PNG_SIDE {
        return Err(CanvasError::InvalidInput(format!(
            "PNG 单边超限（{}x{} > {MAX_PNG_SIDE}）",
            bmp.width, bmp.height
        )));
    }
    if bmp.width as u64 * bmp.height as u64 > MAX_PNG_PIXELS {
        return Err(CanvasError::InvalidInput(format!(
            "PNG 像素数超限（{}x{}）",
            bmp.width, bmp.height
        )));
    }
    let expect = bmp.width as usize * bmp.height as usize * 4;
    if bmp.pixels.len() != expect {
        return Err(CanvasError::InvalidInput(format!(
            "位图缓冲长度不符（{} != {expect}）",
            bmp.pixels.len()
        )));
    }

    let stride = bmp.width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * bmp.height as usize);
    let mut prev = vec![0u8; stride];
    let mut cand = vec![0u8; stride];
    let mut best = vec![0u8; stride];
    for y in 0..bmp.height as usize {
        let row = &bmp.pixels[y * stride..(y + 1) * stride];
        // 只用 None/Sub/Up 三种过滤：三者都被所有主流解码器（含 PIL、
        // 浏览器）正确处理；Average(3)/Paeth(4) 在 PIL 的解码路径上会触发
        // 越界读取，把**解码方**自己的图弄坏（我们的字节是对的，但"能被
        // 正确打开"是硬要求）。代价只是体积，不是正确性。
        let mut chosen = 0u8;
        let mut chosen_sum = u64::MAX;
        for ft in 0..3u8 {
            filter_row(ft, row, &prev, &mut cand);
            let sum: u64 = cand.iter().map(|b| (*b as i8).unsigned_abs() as u64).sum();
            if sum < chosen_sum {
                chosen_sum = sum;
                chosen = ft;
                best.copy_from_slice(&cand);
            }
        }
        raw.push(chosen);
        raw.extend_from_slice(&best);
        prev.copy_from_slice(row);
    }

    let mut out = Vec::with_capacity(raw.len() + 128);
    out.extend_from_slice(&PNG_SIG);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&bmp.width.to_be_bytes());
    ihdr.extend_from_slice(&bmp.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 位 / RGBA / deflate / 无过滤 / 非隔行
    write_chunk(&mut out, b"IHDR", &ihdr);
    let z = zlib_store(&raw);
    write_chunk(&mut out, b"IDAT", &z);
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// PNG 行过滤。
///
/// 参照字节（左 a / 上 b / 左上 c）一律取**原始（未过滤）**像素：编码侧
/// 算残差、解码侧拿同一批原始值重建，两边必须同源。把 a 取成 out[i-bpp]
/// （已过滤字节）是本站踩过的坑——编码器和解码器对不上，图整体花掉。
fn filter_row(ft: u8, row: &[u8], prev: &[u8], out: &mut [u8]) {
    let bpp = 4;
    for i in 0..row.len() {
        let a = if i >= bpp { row[i - bpp] as i16 } else { 0 };
        let b = prev[i] as i16;
        let c = if i >= bpp { prev[i - bpp] as i16 } else { 0 };
        let x = row[i] as i16;
        let v = match ft {
            0 => x,
            1 => x - a,
            2 => x - b,
            3 => x - ((a + b) / 2),
            _ => {
                let p = a + b - c;
                let pa = (p - a).abs();
                let pb = (p - b).abs();
                let pc = (p - c).abs();
                let pred = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                x - pred
            }
        };
        out[i] = (v & 0xff) as u8;
    }
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

/// zlib 流（stored 块：不压缩，但结构合法，任何 PNG 解码器可读）。
fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 16);
    out.push(0x78); // CMF：deflate，32K 窗口
    out.push(0x01); // FLG：无字典，最低校验位
    let mut i = 0;
    loop {
        let n = (data.len() - i).min(65535);
        let last = i + n >= data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&(!(n as u16)).to_le_bytes());
        out.extend_from_slice(&data[i..i + n]);
        i += n;
        if last {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// CRC-32（IEEE 802.3，PNG chunk 用）。
struct Crc32 {
    table: [u32; 256],
    value: u32,
}

impl Crc32 {
    fn new() -> Crc32 {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                k += 1;
            }
            table[i] = c;
            i += 1;
        }
        Crc32 {
            table,
            value: 0xFFFF_FFFF,
        }
    }

    fn update(&mut self, data: &[u8]) {
        for b in data {
            let idx = ((self.value ^ *b as u32) & 0xff) as usize;
            self.value = self.table[idx] ^ (self.value >> 8);
        }
    }

    fn finish(&self) -> u32 {
        self.value ^ 0xFFFF_FFFF
    }
}

/// Adler-32（zlib 尾校验）。
fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// 一次性入口：把渲染模型导出成 size 像素的 PNG（RGBA8）。
///
/// 视口取自内容包围盒（等比缩放适配 size），背景不透明时铺底。
pub fn export_png(canvas: &CanvasRendering, size: (u32, u32)) -> Result<Vec<u8>, CanvasError> {
    canvas.check()?;
    if size.0 == 0 || size.1 == 0 {
        return Err(CanvasError::InvalidInput("导出尺寸不可为零".into()));
    }
    if size.0 > MAX_PNG_SIDE || size.1 > MAX_PNG_SIDE {
        return Err(CanvasError::InvalidInput(format!(
            "导出尺寸超限（{}x{}）",
            size.0, size.1
        )));
    }
    if size.0 as u64 * size.1 as u64 > MAX_PNG_PIXELS {
        return Err(CanvasError::InvalidInput("导出像素数超限".into()));
    }
    let view = canvas_bounds_geometry(canvas);
    let sx = if view.w > 0.0 { size.0 as f64 / view.w } else { 1.0 };
    let sy = if view.h > 0.0 { size.1 as f64 / view.h } else { 1.0 };
    let scale = sx.min(sy);
    let mut r = RasterRenderer::new(size, view, scale, canvas.background);
    for (s, tf) in &canvas.shapes {
        r.draw(s, tf);
    }
    r.into_png()
}
