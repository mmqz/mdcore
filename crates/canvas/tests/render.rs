//! 渲染引擎单测：六图元、变换、线宽规则、PNG 结构、边界与敌意输入。
//!
//! 这里的断言都对着**可观测事实**写（像素值、字节结构、包围盒），不写
//! 「跑通就算过」。像素级断言的容差写在每条断言的注释里。

use mdc_canvas::render::*;
use mdc_canvas::{Actor, CanvasCommand, CanvasObject, CanvasSession, ObjectKind, SessionOptions};
use std::collections::BTreeMap;

// ── 辅助 ───────────────────────────────────────────────────────

fn white_canvas() -> CanvasRendering {
    CanvasRendering::new().with_background(Color::WHITE)
}

/// 单形状渲染成位图（视口 = 形状包围盒，缩放 1:1 便于读像素）。
///
/// 画布放到 (2,2) 起画、位图四周留 2 像素边：视口以几何包围盒为准，
/// 描边会溢出包围盒，不留下边距的话 1px 边框会被裁掉一半。
fn raster_one(shape: Shape) -> Bitmap {
    let mut c = white_canvas();
    c.push(shape);
    // 视口从包围盒左上角外扩 2 像素起，位图再加 2 像素边距——描边会
    // 溢出几何包围盒，不留边距的话 1px 边框会被裁掉。
    // 用**带描边外扩**的包围盒定位（这里要的是"把描边也装进去"，
    // 与 export_png 用几何包围盒算缩放是两回事）。
    let g = canvas_bounds(&c);
    let view = Rect::new(g.x - 2.0, g.y - 2.0, g.w + 4.0, g.h + 4.0);
    let w = view.w.ceil() as u32 + 1;
    let h = view.h.ceil() as u32 + 1;
    let mut r = RasterRenderer::new((w.max(8), h.max(8)), view, 1.0, Color::WHITE);
    for (s, tf) in &c.shapes {
        r.draw(s, tf);
    }
    r.bitmap().clone()
}

fn lum(c: Color) -> f64 {
    (c.r as f64 + c.g as f64 + c.b as f64) / 3.0
}

fn count_dark(b: &Bitmap) -> usize {
    let mut n = 0;
    for y in 0..b.height {
        for x in 0..b.width {
            if lum(b.get(x, y)) < 128.0 {
                n += 1;
            }
        }
    }
    n
}

/// 会话 -> 渲染模型的端到端路径（props 协议）在本文件的末尾有专门测试。
fn session_opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

// ── 6 种图元都能画 ─────────────────────────────────────────────

#[test]
fn line_single_pixel_is_visible_and_symmetric() {
    // 线宽 1、水平：整数落点与半像素落点的平均亮度必须一致。
    let mk = |y: f64| {
        raster_one(Shape::Line {
            from: Point::new(0.0, y),
            to: Point::new(20.0, y),
            style: StrokeStyle::new(Color::BLACK, 1.0),
        })
    };
    let int_row = mk(2.0);
    let half_row = mk(2.5);
    let n_int = count_dark(&int_row);
    let n_half = count_dark(&half_row);
    assert!(n_int > 0, "整数落点：1px 线必须画得出来（不能整条消失）");
    assert!(n_half > 0, "半像素落点：1px 线必须画得出来");
    // 两种落点占的「暗像素总亮度」都应=长度；允许 1 像素的边界差。
    assert!(
        (n_int as i64 - n_half as i64).abs() <= 1,
        "整数落点 {n_int} 与半像素落点 {n_half} 的暗像素数应基本相同"
    );
}

#[test]
fn line_is_not_dropped_by_thin_width() {
    // 线宽 0.5 也必须留下痕迹（不能悄无声息地消失）。
    let b = raster_one(Shape::Line {
        from: Point::new(0.0, 5.0),
        to: Point::new(20.0, 5.0),
        style: StrokeStyle::new(Color::BLACK, 0.5),
    });
    let mut any = false;
    for y in 0..b.height {
        for x in 0..b.width {
            if b.get(x, y) != Color::WHITE {
                any = true;
            }
        }
    }
    assert!(any, "0.5px 线不能整条丢失");
}

#[test]
fn rect_fills_and_strokes() {
    let b = raster_one(Shape::Rect {
        rect: Rect::new(0.0, 0.0, 10.0, 6.0),
        style: ShapeStyle::both(
            Color::rgb(0x2d, 0x6c, 0xdf),
            StrokeStyle::new(Color::BLACK, 1.0),
        ),
    });
    // 中心是填充色 #2d6cdf = rgb(45,108,223)：蓝通道必须远高于红通道。
    let c = b.get(5, 3);
    assert!(c.b > 200 && c.r < 130, "矩形中心应为填充色，实际 {c:?}");
    // 描边必须比填充暗。1px 描边落在半像素上会被抗锯齿摊成两行，
    // 所以取「上下边缘两行里的最暗那一行」来比，而不是钉死某一行。
    let mut edge_min = f64::INFINITY;
    for y in 0..b.height {
        for x in 0..b.width {
            let l = lum(b.get(x, y));
            if l < edge_min {
                edge_min = l;
            }
        }
    }
    assert!(
        edge_min < lum(c) - 20.0,
        "描边应明显比填充暗：最暗像素 {edge_min} vs 填充 {}",
        lum(c)
    );
}

#[test]
fn thick_stroke_is_fully_opaque_mid_edge() {
    // 4px 描边：边中部必须是纯黑满覆盖（抗锯齿只该出现在两侧）。
    let b = raster_one(Shape::Rect {
        rect: Rect::new(0.0, 0.0, 20.0, 12.0),
        style: ShapeStyle::both(Color::WHITE, StrokeStyle::new(Color::BLACK, 4.0)),
    });
    let mid = b.get(10, 2);
    assert!(lum(mid) < 30.0, "4px 描边中部应近乎纯黑，实际 {mid:?}");
}

#[test]
fn ellipse_is_round_not_rectangular() {
    let b = raster_one(Shape::Ellipse {
        rect: Rect::new(0.0, 0.0, 40.0, 40.0),
        style: ShapeStyle::fill(Color::BLACK),
    });
    // 四条边中点是黑，四个角是白 —— 这是"圆"的判据。
    let cx = 20u32;
    let cy = 20u32;
    assert!(lum(b.get(cx, cy)) < 128.0, "椭圆中心应实心");
    assert!(lum(b.get(2, 2)) > 200.0, "椭圆左上角应在圆外（是白色）");
    assert!(lum(b.get(37, 37)) > 200.0, "椭圆右下角应在圆外（是白色）");
}

#[test]
fn path_with_curves_draws() {
    let b = raster_one(Shape::Path {
        cmds: vec![
            PathCmd::MoveTo(Point::new(0.0, 20.0)),
            PathCmd::CubicTo {
                c1: Point::new(10.0, 0.0),
                c2: Point::new(20.0, 40.0),
                to: Point::new(30.0, 20.0),
            },
            PathCmd::Close,
        ],
        style: ShapeStyle::fill(Color::BLACK),
    });
    assert!(count_dark(&b) > 50, "贝塞尔路径必须画出实质内容");
}

#[test]
fn text_placeholder_box_is_drawn() {
    // 诚实登记：PNG 里文字 = 实心占位框。这里断言"确实画了东西"，
    // 而不是断言字形（字形由 SVG 交给浏览器）。
    let b = raster_one(Shape::Text {
        pos: Point::new(2.0, 20.0),
        text: "abc".into(),
        style: TextStyle::new(16.0, Color::BLACK),
    });
    assert!(count_dark(&b) > 20, "文字占位框必须可见（不能是空白）");
}

#[test]
fn image_without_data_draws_placeholder_frame() {
    let b = raster_one(Shape::Image {
        rect: Rect::new(0.0, 0.0, 20.0, 12.0),
        data: Vec::new(),
        mime: ImageMime::Png,
    });
    // 虚线框：至少有几个灰点。
    let mut gray = 0;
    for y in 0..b.height {
        for x in 0..b.width {
            let c = b.get(x, y);
            if c != Color::WHITE {
                gray += 1;
            }
        }
    }
    assert!(gray > 5, "无数据位图应画虚线占位框，实际非白像素 {gray}");
}

// ── 变换 ───────────────────────────────────────────────────────

#[test]
fn translate_shifts_bounds() {
    let s = Shape::Rect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        style: ShapeStyle::fill(Color::BLACK),
    };
    let b0 = shape_bounds(&s, &Transform::IDENTITY).unwrap();
    let b1 = shape_bounds(&s, &Transform::translate(100.0, 50.0)).unwrap();
    assert!((b1.x - b0.x - 100.0).abs() < 1e-9);
    assert!((b1.y - b0.y - 50.0).abs() < 1e-9);
    assert!((b1.w - b0.w).abs() < 1e-9, "平移不改变尺寸");
}

#[test]
fn scale_multiplies_size_about_center() {
    let s = Shape::Rect {
        rect: Rect::new(0.0, 0.0, 10.0, 20.0),
        style: ShapeStyle::fill(Color::BLACK),
    };
    let b0 = shape_bounds(&s, &Transform::IDENTITY).unwrap();
    let b2 = shape_bounds(&s, &Transform::scale(2.0)).unwrap();
    assert!((b2.w - b0.w * 2.0).abs() < 1e-9, "缩放 2 倍宽度翻倍");
    assert!((b2.h - b0.h * 2.0).abs() < 1e-9, "缩放 2 倍高度翻倍");
    // 支点 = 包围盒中心 → 中心不动。
    let c0 = b0.center();
    let c2 = b2.center();
    assert!(
        (c2.x - c0.x).abs() < 1e-9 && (c2.y - c0.y).abs() < 1e-9,
        "绕中心缩放，中心不动"
    );
}

#[test]
fn rotate_90_maps_axes() {
    // 单位方块的 rotate(90°)：宽高互换、中心不动。
    let s = Shape::Rect {
        rect: Rect::new(0.0, 0.0, 40.0, 10.0),
        style: ShapeStyle::fill(Color::BLACK),
    };
    let b0 = shape_bounds(&s, &Transform::IDENTITY).unwrap();
    let b90 = shape_bounds(&s, &Transform::rotate(std::f64::consts::FRAC_PI_2)).unwrap();
    assert!((b90.w - b0.h).abs() < 1e-6, "旋转 90°：新宽 = 原高");
    assert!((b90.h - b0.w).abs() < 1e-6, "旋转 90°：新高 = 原宽");
}

#[test]
fn identity_transform_is_exact_noop() {
    let m = Transform::IDENTITY.to_affine(Point::new(0.0, 0.0));
    assert!(m.is_identity());
    let p = m.apply(Point::new(3.25, 7.75));
    assert_eq!(p, Point::new(3.25, 7.75), "恒等变换必须零漂移");
}

#[test]
fn rotate_is_clockwise_in_screen_coords() {
    // y 轴向下时，原点右方一点绕原点顺时针 90° 落到下方。
    let m = Transform::rotate(std::f64::consts::FRAC_PI_2).with_origin(Point::new(0.0, 0.0));
    let a = m.to_affine(Point::new(0.0, 0.0));
    let p = a.apply(Point::new(10.0, 0.0));
    assert!(
        (p.x).abs() < 1e-9 && (p.y - 10.0).abs() < 1e-9,
        "顺时针为正，实际 {p:?}"
    );
}

// ── SVG 导出 ───────────────────────────────────────────────────

#[test]
fn svg_has_all_six_primitives() {
    let mut c = white_canvas();
    c.push(Shape::Line {
        from: Point::new(0.0, 0.0),
        to: Point::new(10.0, 10.0),
        style: StrokeStyle::new(Color::BLACK, 1.0),
    });
    c.push(Shape::Rect {
        rect: Rect::new(0.0, 0.0, 5.0, 5.0),
        style: ShapeStyle::fill(Color::BLACK),
    });
    c.push(Shape::Ellipse {
        rect: Rect::new(0.0, 0.0, 5.0, 3.0),
        style: ShapeStyle::fill(Color::BLACK),
    });
    c.push(Shape::Path {
        cmds: vec![
            PathCmd::MoveTo(Point::new(0.0, 0.0)),
            PathCmd::LineTo(Point::new(3.0, 3.0)),
        ],
        style: ShapeStyle::stroke(StrokeStyle::new(Color::BLACK, 1.0)),
    });
    c.push(Shape::Text {
        pos: Point::new(0.0, 10.0),
        text: "hi".into(),
        style: TextStyle::new(10.0, Color::BLACK),
    });
    c.push(Shape::Image {
        rect: Rect::new(0.0, 0.0, 4.0, 4.0),
        data: vec![1, 2, 3],
        mime: ImageMime::Png,
    });
    let svg = export_svg(&c);
    for tag in ["<line", "<rect", "<ellipse", "<path", "<text", "<image"] {
        assert!(svg.contains(tag), "SVG 缺少 {tag}");
    }
    assert!(svg.starts_with("<?xml"), "SVG 应有 XML 声明");
    assert!(svg.trim_end().ends_with("</svg>"), "SVG 必须闭合");
}

#[test]
fn svg_escapes_hostile_text() {
    let mut c = white_canvas();
    c.push(Shape::Text {
        pos: Point::new(0.0, 10.0),
        text: "<script>alert(\"x\")</script> & more".into(),
        style: TextStyle::new(10.0, Color::BLACK),
    });
    let svg = export_svg(&c);
    assert!(!svg.contains("<script>"), "文本内容不得原样注入标签");
    assert!(svg.contains("&lt;script&gt;"), "尖括号必须转义");
    assert!(svg.contains("&amp;"), "& 必须转义");
}

#[test]
fn svg_transform_folds_into_geometry() {
    let mut c = white_canvas();
    c.push_with(
        Shape::Rect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            style: ShapeStyle::fill(Color::BLACK),
        },
        Transform::translate(50.0, 0.0),
    );
    let svg = export_svg(&c);
    assert!(svg.contains("x=\"50\""), "平移必须体现在输出几何上：{svg}");
    assert!(
        !svg.contains("<g transform"),
        "走几何折入，不输出 <g transform>"
    );
}

#[test]
fn empty_canvas_still_produces_valid_svg() {
    let svg = export_svg(&CanvasRendering::new());
    assert!(svg.contains("<svg"));
    assert!(svg.contains("viewBox"));
}

// ── PNG 结构与编码 ─────────────────────────────────────────────

#[test]
fn png_has_valid_signature_and_chunks() {
    let mut c = white_canvas();
    c.push(Shape::Rect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        style: ShapeStyle::fill(Color::BLACK),
    });
    let png = export_png(&c, (16, 16)).unwrap();
    assert_eq!(
        &png[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        "PNG 签名"
    );
    assert_eq!(&png[12..16], b"IHDR", "第一个 chunk 必须是 IHDR");
    assert!(png.windows(4).any(|w| w == b"IDAT"), "必须有 IDAT");
    assert_eq!(
        &png[png.len() - 8..png.len() - 4],
        b"IEND",
        "必须以 IEND 收尾"
    );
}

#[test]
fn png_ihdr_dimensions_match_request() {
    let c = white_canvas();
    let png = export_png(&c, (37, 19)).unwrap();
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    assert_eq!((w, h), (37, 19), "IHDR 尺寸必须等于请求尺寸");
    // 位深 8 / 色彩类型 6（RGBA）
    assert_eq!(png[24], 8);
    assert_eq!(png[25], 6);
}

#[test]
fn png_rejects_zero_size() {
    let c = white_canvas();
    assert!(export_png(&c, (0, 10)).is_err(), "零宽必须报错");
    assert!(export_png(&c, (10, 0)).is_err(), "零高必须报错");
}

#[test]
fn png_is_deterministic() {
    let mut c = white_canvas();
    c.push(Shape::Ellipse {
        rect: Rect::new(1.0, 2.0, 20.0, 12.0),
        style: ShapeStyle::fill(Color::rgb(10, 20, 30)),
    });
    let a = export_png(&c, (40, 30)).unwrap();
    let b = export_png(&c, (40, 30)).unwrap();
    assert_eq!(a, b, "同一输入两次导出必须字节相同（可做黄金例）");
}

#[test]
fn encode_png_checks_buffer_length() {
    let bad = Bitmap {
        width: 2,
        height: 2,
        pixels: vec![0; 4], // 应为 16
    };
    assert!(encode_png(&bad).is_err(), "位图缓冲长度不符必须报错");
}

// ── 敌意输入 ───────────────────────────────────────────────────

#[test]
fn non_finite_geometry_is_rejected() {
    let mut c = CanvasRendering::new();
    c.push(Shape::Line {
        from: Point::new(f64::NAN, 0.0),
        to: Point::new(1.0, 1.0),
        style: StrokeStyle::new(Color::BLACK, 1.0),
    });
    assert!(c.check().is_err(), "NaN 坐标必须被拒");
    assert!(export_png(&c, (10, 10)).is_err(), "导出前必须校验");
}

#[test]
fn negative_size_is_rejected() {
    let mut c = CanvasRendering::new();
    c.push(Shape::Rect {
        rect: Rect::new(0.0, 0.0, -5.0, 5.0),
        style: ShapeStyle::fill(Color::BLACK),
    });
    assert!(c.check().is_err(), "负尺寸必须被拒");
}

#[test]
fn zero_dash_gap_is_rejected() {
    let mut c = CanvasRendering::new();
    c.push(Shape::Line {
        from: Point::new(0.0, 0.0),
        to: Point::new(5.0, 0.0),
        style: StrokeStyle::new(Color::BLACK, 1.0).with_dash(StrokeDash::new(3, 0)),
    });
    assert!(c.check().is_err(), "dash gap = 0 必须被拒");
}

#[test]
fn parsing_helpers_are_total() {
    assert_eq!(parse_color("#ff0000"), Some(Color::rgb(255, 0, 0)));
    assert_eq!(parse_color("#f00"), Some(Color::rgb(255, 0, 0)));
    assert_eq!(parse_color("rgb(1,2,3)"), Some(Color::rgb(1, 2, 3)));
    assert_eq!(parse_color("nope"), None);
    assert_eq!(parse_color("#12345"), None);
}
// ── 会话 -> 渲染模型（props 协议端到端）──────────────────────

/// 画布会话里建的节点/连线，能经 props 协议渲染成图元。
#[test]
fn session_objects_become_shapes_via_props() {
    let mut c = CanvasSession::open(session_opts(90)).unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("a".into()),
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 20.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("b".into()),
            x: 100.0,
            y: 0.0,
            w: 40.0,
            h: 20.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddEdge {
            id: Some("e".into()),
            from: "a".into(),
            to: "b".into(),
            props: BTreeMap::new(),
        },
    )
    .unwrap();

    let rendering = CanvasRendering::from_session(&c);
    assert_eq!(rendering.len(), 3, "两节点 + 一连线，三个图元");
    // 节点缺省渲染成矩形；连线是 Line，且端点取两节点中心。
    let mut rects = 0;
    let mut lines = 0;
    for (s, _) in &rendering.shapes {
        match s {
            Shape::Rect { .. } => rects += 1,
            Shape::Line { .. } => lines += 1,
            _ => {}
        }
    }
    assert_eq!(rects, 2, "两个节点应渲染成矩形");
    assert_eq!(lines, 1, "一条边应渲染成线段");
    let svg = export_svg(&rendering);
    assert!(svg.contains("<rect") && svg.contains("<line"));
    assert!(export_png(&rendering, (160, 60)).is_ok());
}

/// props 协议：shape/fill/rotation 覆盖缺省推导。
#[test]
fn props_protocol_overrides_defaults() {
    let mut props = BTreeMap::new();
    props.insert("shape".to_string(), "ellipse".to_string());
    props.insert("fill".to_string(), "#ff0000".to_string());
    let obj = CanvasObject {
        id: "x".into(),
        kind: ObjectKind::Node,
        x: 10.0,
        y: 20.0,
        w: 30.0,
        h: 15.0,
        from: None,
        to: None,
        members: vec![],
        props,
        ..Default::default()
    };
    let (shape, tf) = shape_from_object(&obj, &[]).expect("应能转成图元");
    match shape {
        Shape::Ellipse { rect, style } => {
            assert_eq!(rect, Rect::new(10.0, 20.0, 30.0, 15.0));
            assert_eq!(style.fill, Some(Color::rgb(255, 0, 0)));
        }
        other => panic!("shape=ellipse 应产生 Ellipse，实际 {other:?}"),
    }
    assert!(tf.is_identity() && tf.rotate == 0.0);
}

/// Group 对象本身不成图元（成员各自成图元）。
#[test]
fn group_object_itself_is_not_drawn() {
    let obj = CanvasObject {
        id: "g".into(),
        kind: ObjectKind::Group,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
        from: None,
        to: None,
        members: vec!["a".into()],
        props: BTreeMap::new(),
        ..Default::default()
    };
    assert!(shape_from_object(&obj, &[]).is_none(), "分组不直接画");
}

/// 悬空边（端点已被删）不得 panic，直接跳过。
#[test]
fn dangling_edge_is_skipped_not_panicked() {
    let obj = CanvasObject {
        id: "e".into(),
        kind: ObjectKind::Edge,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
        from: Some("gone-1".into()),
        to: Some("gone-2".into()),
        members: vec![],
        props: BTreeMap::new(),
        ..Default::default()
    };
    assert!(shape_from_object(&obj, &[]).is_none(), "悬空边应跳过");
}

/// 非法属性值一律回退缺省，不 panic（敌意/陈旧属性面）。
#[test]
fn bad_props_fall_back_to_defaults() {
    let mut props = BTreeMap::new();
    props.insert("shape".into(), "no-such-shape".into());
    props.insert("fill".into(), "not-a-color".into());
    props.insert("rotation".into(), "NaN".into());
    props.insert("opacity".into(), "999".into());
    props.insert("stroke_width".into(), "-5".into());
    let obj = CanvasObject {
        id: "x".into(),
        kind: ObjectKind::Node,
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
        from: None,
        to: None,
        members: vec![],
        props,
        ..Default::default()
    };
    let (shape, tf) = shape_from_object(&obj, &[]).expect("非法属性也应回退缺省并出图元");
    assert!(matches!(shape, Shape::Rect { .. }), "未知 shape 值回退矩形");
    assert_eq!(tf.rotate, 0.0, "非法 rotation 回退 0");
    assert!(tf.rotate.is_finite());
}

// ── 笔迹面（#350——#44 第 1 步：引擎渲染中线真相）─────────────

fn stroke_obj(points: Vec<(f64, f64)>, tool: Option<mdc_canvas::StrokeTool>) -> CanvasObject {
    CanvasObject {
        id: "s".into(),
        kind: ObjectKind::Stroke,
        points,
        color: Some("#1a73e8".into()),
        width: Some(2.5),
        tool,
        ..Default::default()
    }
}

/// 笔迹 → Path 图元：中线折线（MoveTo + LineTo 链）+ 圆头不透明描边。
#[test]
fn stroke_maps_to_centerline_path_pen() {
    let obj = stroke_obj(
        vec![(10.0, 20.0), (40.0, 25.0), (80.0, 60.0)],
        Some(mdc_canvas::StrokeTool::Pen),
    );
    let (shape, _) = shape_from_object(&obj, &[]).expect("笔迹应成图元");
    match shape {
        Shape::Path { cmds, style } => {
            assert_eq!(cmds.len(), 3, "3 点 = MoveTo + 2 LineTo");
            assert!(matches!(cmds[0], PathCmd::MoveTo(p) if p == Point::new(10.0, 20.0)));
            assert!(matches!(cmds[1], PathCmd::LineTo(p) if p == Point::new(40.0, 25.0)));
            assert!(matches!(cmds[2], PathCmd::LineTo(p) if p == Point::new(80.0, 60.0)));
            let st = style.stroke.expect("笔迹恒有描边");
            assert_eq!(st.color, Color::rgb(0x1a, 0x73, 0xe8), "Pen 不透明");
            assert_eq!(st.width, 2.5);
            assert_eq!(st.cap, LineCap::Round, "Pen 圆头");
            assert!(style.fill.is_none(), "笔迹无填充");
        }
        other => panic!("笔迹应映射 Path，实际 {other:?}"),
    }
}

/// Highlighter：平头 + 半透明（α×0.35 渲染态——模型颜色恒不透明）。
#[test]
fn stroke_highlighter_translucent_butt_cap() {
    let obj = stroke_obj(
        vec![(0.0, 0.0), (50.0, 0.0)],
        Some(mdc_canvas::StrokeTool::Highlighter),
    );
    let (shape, _) = shape_from_object(&obj, &[]).expect("荧光笔应成图元");
    match shape {
        Shape::Path { style, .. } => {
            let st = style.stroke.expect("笔迹恒有描边");
            assert_eq!(st.cap, LineCap::Butt, "Highlighter 平头");
            assert_eq!(st.color.a, 89, "α = round(255×0.35) = 89");
            assert_eq!((st.color.r, st.color.g, st.color.b), (0x1a, 0x73, 0xe8));
        }
        other => panic!("实际 {other:?}"),
    }
}

/// 退化笔迹（<2 点）不成图元、不 panic（敌意/陈旧载体面）。
#[test]
fn degenerate_stroke_is_skipped_not_panicked() {
    let obj = stroke_obj(vec![(0.0, 0.0)], Some(mdc_canvas::StrokeTool::Pen));
    assert!(shape_from_object(&obj, &[]).is_none());
    // 无工具/无颜色/无线宽的陈旧载体 → 回退缺省（黑 1pt Pen），不 panic。
    let mut obj2 = stroke_obj(vec![(0.0, 0.0), (9.0, 9.0)], None);
    obj2.color = None;
    obj2.width = None;
    let (shape, _) = shape_from_object(&obj2, &[]).expect("陈旧载体回退仍成图元");
    match shape {
        Shape::Path { style, .. } => {
            let st = style.stroke.unwrap();
            assert_eq!(st.color, Color::BLACK, "缺色回退黑");
            assert_eq!(st.width, 2.0, "缺宽回退 2pt");
        }
        other => panic!("实际 {other:?}"),
    }
}

/// 会话 → 渲染全链：加笔后 from_session 含 Path 图元，canvas_bounds
/// 覆盖笔迹几何包围盒（缺映射 = export_svg/PNG 丢笔迹 + 视口裁错）。
#[test]
fn session_stroke_renders_and_bounds_cover_it() {
    use mdc_canvas::StrokeTool;
    let mut s = CanvasSession::open(SessionOptions {
        client_id: 1,
        ..Default::default()
    })
    .unwrap();
    s.execute(
        Actor::Human,
        mdc_canvas::CanvasCommand::AddStroke {
            id: Some("s-1".into()),
            points: vec![(100.0, 120.0), (200.0, 180.0), (300.0, 120.0)],
            color: "#ff0000".into(),
            width: 3.0,
            tool: StrokeTool::Pen,
        },
    )
    .unwrap();
    let rendering = CanvasRendering::from_session(&s);
    assert_eq!(rendering.shapes.len(), 1, "一笔 = 一图元");
    // 几何包围盒 = 中线点列精确范围（canvas_bounds_geometry 无描边外扩）。
    let bg = canvas_bounds_geometry(&rendering);
    assert_eq!((bg.x, bg.y, bg.w, bg.h), (100.0, 120.0, 200.0, 60.0));
    // 含描边外扩 = 每侧 + width/2（3pt → 1.5——视口裁剪面）。
    let b = canvas_bounds(&rendering);
    assert_eq!((b.x, b.y, b.w, b.h), (98.5, 118.5, 203.0, 63.0));
}
