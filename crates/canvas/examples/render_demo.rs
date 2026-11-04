//! 渲染样本导出器：把 6 种图元各画一遍，落盘 SVG 与 PNG，供人眼与
//! 外部渲染器比对。
//!
//! 用法（仓库根）：
//!
//!     cargo run -p mdc-canvas --example render_demo -- <输出目录>
//!
//! 产物：demo.svg / demo.png / fixtures.svg / fixtures.png / perf 报告。

use std::fs;
use std::path::PathBuf;

use mdc_canvas::render::{
    CanvasRendering, Color, LineCap, PathCmd, Point, Rect, Shape, ShapeStyle, StrokeDash,
    StrokeStyle, TextAnchor, TextStyle,
};

fn stroke(c: Color, w: f64) -> StrokeStyle {
    StrokeStyle::new(c, w)
}

/// 六种图元各一份（同一张图里全部出现）。
fn demo_canvas() -> CanvasRendering {
    let mut c = CanvasRendering::new().with_background(Color::rgb(0xff, 0xff, 0xff));
    // 1. Line
    c.push(Shape::Line {
        from: Point::new(20.0, 20.0),
        to: Point::new(180.0, 60.0),
        style: stroke(Color::rgb(0xd0, 0x32, 0x2f), 3.0).with_cap(LineCap::Round),
    });
    // 2. Rect（填充 + 描边）
    c.push(Shape::Rect {
        rect: Rect::new(20.0, 90.0, 120.0, 70.0),
        style: ShapeStyle::both(
            Color::rgb(0x2d, 0x6c, 0xdf),
            stroke(Color::rgb(0x12, 0x29, 0x59), 2.0),
        ),
    });
    // 3. Ellipse
    c.push(Shape::Ellipse {
        rect: Rect::new(170.0, 90.0, 110.0, 70.0),
        style: ShapeStyle::both(
            Color::rgb(0x5c, 0xb8, 0x5c),
            stroke(Color::rgb(0x1e, 0x4f, 0x1e), 2.0),
        ),
    });
    // 4. Path（三次贝塞尔 + 闭合子路径）
    c.push(Shape::Path {
        cmds: vec![
            PathCmd::MoveTo(Point::new(30.0, 210.0)),
            PathCmd::CubicTo {
                c1: Point::new(90.0, 170.0),
                c2: Point::new(150.0, 250.0),
                to: Point::new(210.0, 210.0),
            },
            PathCmd::LineTo(Point::new(210.0, 250.0)),
            PathCmd::QuadTo {
                ctrl: Point::new(120.0, 290.0),
                to: Point::new(30.0, 250.0),
            },
            PathCmd::Close,
        ],
        style: ShapeStyle::both(
            Color::rgb(0xf2, 0xc9, 0x4c),
            stroke(Color::rgb(0x8a, 0x6d, 0x06), 2.0),
        ),
    });
    // 5. Text
    c.push(Shape::Text {
        pos: Point::new(20.0, 330.0),
        text: "Canvas 1.0 <render>".to_string(),
        style: TextStyle {
            color: Color::rgb(0x22, 0x22, 0x22),
            size: 20.0,
            anchor: TextAnchor::Start,
            family: Some("sans-serif".into()),
            bold: true,
        },
    });
    // 6. Image（1x1 红点 PNG，base64 解码后的字节）
    c.push(Shape::Image {
        rect: Rect::new(260.0, 300.0, 40.0, 40.0),
        data: tiny_png(),
        mime: mdc_canvas::render::ImageMime::Png,
    });
    // 虚线样例（描边样式面）
    c.push(Shape::Line {
        from: Point::new(20.0, 360.0),
        to: Point::new(300.0, 360.0),
        style: stroke(Color::rgb(0x55, 0x55, 0x55), 1.0).with_dash(StrokeDash::new(6, 4)),
    });
    c
}

/// 1x1 红色 PNG（真实字节，不是占位）。
fn tiny_png() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, b'I', b'D', b'A', b'T', 0x08, 0xd7, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00,
        0x00, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ]
}

fn main() {
    let out: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("canvas-out"));
    fs::create_dir_all(&out).unwrap();
    let c = demo_canvas();
    let svg = mdc_canvas::export_svg(&c);
    fs::write(out.join("demo.svg"), &svg).unwrap();
    let png = mdc_canvas::export_png(&c, (320, 380)).expect("png");
    fs::write(out.join("demo.png"), &png).unwrap();
    println!("demo.svg {} bytes", svg.len());
    println!("demo.png {} bytes", png.len());
}
