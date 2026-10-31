// 黄金例画布定义（被 examples/gen_fixtures.rs 与 tests/golden.rs 共用）。
//
// 放在这里而不是各自写一份：夹具生成器和比对测试必须画**同一张图**，
// 两处分叉就等于自己跟自己比。
//
// 刻意覆盖容易翻车的组合：1px 细描边、半像素落点、4px 粗描边、
// 贝塞尔路径、虚线、旋转图元、圆端帽、位图占位。

use mdc_canvas::render::{
    CanvasRendering, Color, ImageMime, LineCap, PathCmd, Point, Rect, Shape, ShapeStyle,
    StrokeDash, StrokeStyle, TextAnchor, TextStyle,
};

/// 黄金例的导出尺寸（PNG 用）。
pub const GOLDEN_PNG_SIZE: (u32, u32) = (240, 290);

/// 黄金例画布。
pub fn golden_canvas() -> CanvasRendering {
    let mut c = CanvasRendering::new().with_background(Color::rgb(0xff, 0xff, 0xff));

    // 1px 细线
    c.push(Shape::Line {
        from: Point::new(40.0, 40.0),
        to: Point::new(160.0, 40.0),
        style: StrokeStyle::new(Color::rgb(0x33, 0x33, 0x33), 1.0),
    });
    // 半像素落点的 1px 线：闭区间补偿不到位就会淡一半
    c.push(Shape::Line {
        from: Point::new(40.0, 52.5),
        to: Point::new(160.0, 52.5),
        style: StrokeStyle::new(Color::rgb(0xd0, 0x32, 0x2f), 1.0),
    });
    // 4px 粗描边矩形：覆盖率归一化不到位内部就不是纯色
    c.push(Shape::Rect {
        rect: Rect::new(40.0, 70.0, 60.0, 36.0),
        style: ShapeStyle::both(
            Color::rgb(0x2d, 0x6c, 0xdf),
            StrokeStyle::new(Color::rgb(0x12, 0x29, 0x59), 4.0),
        ),
    });
    // 椭圆
    c.push(Shape::Ellipse {
        rect: Rect::new(120.0, 70.0, 60.0, 36.0),
        style: ShapeStyle::both(
            Color::rgb(0x5c, 0xb8, 0x5c),
            StrokeStyle::new(Color::rgb(0x1e, 0x4f, 0x1e), 2.0),
        ),
    });
    // 路径：三次 + 二次 + 闭合
    c.push(Shape::Path {
        cmds: vec![
            PathCmd::MoveTo(Point::new(48.0, 150.0)),
            PathCmd::CubicTo {
                c1: Point::new(90.0, 122.0),
                c2: Point::new(130.0, 168.0),
                to: Point::new(172.0, 140.0),
            },
            PathCmd::LineTo(Point::new(172.0, 172.0)),
            PathCmd::QuadTo {
                ctrl: Point::new(110.0, 196.0),
                to: Point::new(48.0, 172.0),
            },
            PathCmd::Close,
        ],
        style: ShapeStyle::both(
            Color::rgb(0xf2, 0xc9, 0x4c),
            StrokeStyle::new(Color::rgb(0x8a, 0x6d, 0x06), 2.0),
        ),
    });
    // 文字（SVG 出真字形；PNG 出已登记的占位框）
    c.push(Shape::Text {
        pos: Point::new(40.0, 220.0),
        text: "canvas render 1.0".to_string(),
        style: TextStyle {
            color: Color::rgb(0x22, 0x22, 0x22),
            size: 14.0,
            anchor: TextAnchor::Start,
            family: Some("sans-serif".into()),
            bold: false,
        },
    });
    // 虚线
    c.push(Shape::Line {
        from: Point::new(40.0, 240.0),
        to: Point::new(180.0, 240.0),
        style: StrokeStyle::new(Color::rgb(0x66, 0x66, 0x66), 1.0)
            .with_dash(StrokeDash::new(6, 4)),
    });
    // 旋转 45 度的方块：轴向快路径用错会退化成一条线
    c.push_with(
        Shape::Rect {
            rect: Rect::new(150.0, 200.0, 22.0, 22.0),
            style: ShapeStyle::fill(Color::rgb(0xaa, 0x4d, 0xd0)),
        },
        mdc_canvas::Transform::rotate(std::f64::consts::FRAC_PI_4)
            .with_origin(Point::new(161.0, 211.0)),
    );
    // 圆端帽粗线
    c.push(Shape::Line {
        from: Point::new(40.0, 262.0),
        to: Point::new(120.0, 262.0),
        style: StrokeStyle::new(Color::rgb(0x22, 0x88, 0x88), 5.0).with_cap(LineCap::Round),
    });
    // 无数据位图占位框
    c.push(Shape::Image {
        rect: Rect::new(150.0, 250.0, 26.0, 26.0),
        data: Vec::new(),
        mime: ImageMime::Png,
    });
    c
}
