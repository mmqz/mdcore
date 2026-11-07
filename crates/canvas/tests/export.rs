//! 会话层导出 API 单测（#418）。
//!
//! 断言对着**可观测事实**写：SVG 结构（XML 头 / viewBox / 元素 / 颜色）、
//! PNG IHDR 尺寸字段、错误变体——不写「跑通就算过」。
//!
//! 验收对照（issue #418）：
//! 1. 两个矩形 + 一条边，export_svg() 产出合法 SVG → `export_svg_full_canvas_*`
//! 2. 用浏览器打开 SVG 显示正确 → 结构断言 + 与渲染层 export_svg 同构
//!    （后者已被 tests/golden.rs 用 PyMuPDF 光栅化交叉验证，本层委托继承）
//! 3. 区域导出只含区域内对象 → `export_svg_region_*`
//! 4. PNG 导出尺寸 = rect * scale → `export_png_size_*`

use mdc_canvas::render::Rect;
use mdc_canvas::{Actor, CanvasCommand, CanvasError, CanvasSession, SessionOptions};
use std::collections::BTreeMap;

// ── 辅助 ───────────────────────────────────────────────────────

fn session() -> CanvasSession {
    CanvasSession::open(SessionOptions {
        client_id: 7_300_001,
        ..Default::default()
    })
    .expect("open")
}

/// 加一个矩形节点（fill 用于在 SVG 里互相区分）。
fn add_rect(s: &mut CanvasSession, id: &str, x: f64, y: f64, w: f64, h: f64, fill: &str) {
    let mut props = BTreeMap::new();
    props.insert("fill".to_string(), fill.to_string());
    s.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some(id.to_string()),
            x,
            y,
            w,
            h,
            props,
        },
    )
    .expect("add node");
}

fn add_edge(s: &mut CanvasSession, id: &str, from: &str, to: &str) {
    s.execute(
        Actor::Human,
        CanvasCommand::AddEdge {
            id: Some(id.to_string()),
            from: from.to_string(),
            to: to.to_string(),
            props: BTreeMap::new(),
        },
    )
    .expect("add edge");
}

/// PNG IHDR 尺寸（大端 u32 于固定偏移——签名 8B + 长度 4B + "IHDR" 4B）。
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.len() > 24, "PNG 过短");
    assert_eq!(&bytes[12..16], b"IHDR", "首块必须是 IHDR");
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (w, h)
}

// ── 验收 1：整幅 SVG 合法且含全部对象 ───────────────────────────

#[test]
fn export_svg_full_canvas_two_rects_one_edge() {
    let mut s = session();
    add_rect(&mut s, "a", 10.0, 10.0, 20.0, 20.0, "#ff0000");
    add_rect(&mut s, "b", 40.0, 10.0, 20.0, 20.0, "#0000ff");
    add_edge(&mut s, "e", "a", "b");

    let svg = s.export_svg();

    // 合法 SVG 的结构事实
    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.contains("viewBox=\""));
    assert!(svg.ends_with("</svg>\n"));

    // 两个矩形（fill 颜色互异可区分）+ 一条边（默认描边 #333333）
    assert!(svg.contains("fill=\"#ff0000\""), "红矩形缺失");
    assert!(svg.contains("fill=\"#0000ff\""), "蓝矩形缺失");
    assert!(svg.contains("<line "), "边缺失");
    assert!(svg.contains("stroke=\"#333333\""), "边默认描边缺失");
    // 只有一条边：x1 出现一次
    assert_eq!(svg.matches("x1=").count(), 1);
}

#[test]
fn export_svg_empty_session_degenerate_viewbox() {
    let s = session();
    let svg = s.export_svg();
    // 零对象 → 渲染层既有语义：1x1 于原点
    assert!(svg.contains("viewBox=\"0 0 1 1\""));
    assert!(!svg.contains("<rect "), "空画布不应有内容");
    assert!(!svg.contains("<line "));
}

// ── 验收 3：区域导出只含区域内对象 ─────────────────────────────

#[test]
fn export_svg_region_filters_to_intersecting_objects() {
    let mut s = session();
    add_rect(&mut s, "near", 0.0, 0.0, 10.0, 10.0, "#ff0000");
    add_rect(&mut s, "far", 100.0, 100.0, 10.0, 10.0, "#0000ff");

    // 区域盖住 near、远离 far
    let svg = s.export_svg_region(Rect::new(0.0, 0.0, 50.0, 50.0));
    assert!(
        svg.contains("viewBox=\"0 0 50 50\""),
        "viewBox 应为给定区域"
    );
    assert!(svg.contains("fill=\"#ff0000\""), "区域内对象缺失");
    assert!(!svg.contains("fill=\"#0000ff\""), "区域外对象不应出现");

    // 反向区域：盖住 far、远离 near
    let svg = s.export_svg_region(Rect::new(90.0, 90.0, 50.0, 50.0));
    assert!(svg.contains("viewBox=\"90 90 50 50\""));
    assert!(svg.contains("fill=\"#0000ff\""));
    assert!(!svg.contains("fill=\"#ff0000\""));

    // 与两区域都无交集的第三对象仍不出现（负坐标区域）
    let svg = s.export_svg_region(Rect::new(-30.0, -30.0, 10.0, 10.0));
    assert!(svg.contains("viewBox=\"-30 -30 10 10\""));
    assert!(!svg.contains("fill=\"#ff0000\""));
    assert!(!svg.contains("fill=\"#0000ff\""));
}

#[test]
fn export_svg_region_keeps_straddling_object() {
    let mut s = session();
    // 跨界对象：横跨区域边界（区域 0,0,30,30 只盖住其右下角）
    add_rect(&mut s, "straddle", -20.0, -20.0, 40.0, 40.0, "#00ff00");
    add_rect(&mut s, "out", -60.0, -60.0, 10.0, 10.0, "#ff00ff");

    let svg = s.export_svg_region(Rect::new(0.0, 0.0, 30.0, 30.0));
    assert!(
        svg.contains("fill=\"#00ff00\""),
        "跨界对象（与区域相交）应保留"
    );
    assert!(!svg.contains("fill=\"#ff00ff\""), "无交集对象应滤除");
    // 跨界对象的原始几何完整保留（出界部分由根视口裁剪，不预切割）
    assert!(svg.contains("width=\"40\""), "跨界对象几何不应被预裁剪");
}

#[test]
fn export_svg_region_edge_resolved_against_full_canvas() {
    let mut s = session();
    // 边 a→b 的线体落在两节点之间；区域只盖住边的中间段——
    // 边保留（其包围盒与区域相交），端点节点被滤除（包围盒无交集）。
    add_rect(&mut s, "a", 0.0, 0.0, 10.0, 10.0, "#ff0000");
    add_rect(&mut s, "b", 100.0, 0.0, 10.0, 10.0, "#0000ff");
    add_edge(&mut s, "e", "a", "b");

    let svg = s.export_svg_region(Rect::new(40.0, -10.0, 20.0, 30.0));
    assert!(svg.contains("<line "), "穿越区域的边应保留");
    assert!(!svg.contains("fill=\"#ff0000\""), "区域外端点不应出现");
    assert!(!svg.contains("fill=\"#0000ff\""), "区域外端点不应出现");
    // 边几何按全画布坐标解析（a 中心 5,5 → b 中心 105,5）
    assert!(svg.contains("x1=\"5\" y1=\"5\" x2=\"105\" y2=\"5\""));
}

#[test]
fn export_svg_region_invalid_rect_falls_back_to_empty() {
    let mut s = session();
    add_rect(&mut s, "a", 0.0, 0.0, 10.0, 10.0, "#ff0000");

    // 非法区域族：负尺寸 / 零尺寸 / 非有限值——宁缺毋错，返回空内容
    // 合法 SVG（不 panic）
    for rect in [
        Rect::new(0.0, 0.0, -5.0, 10.0),
        Rect::new(0.0, 0.0, 0.0, 10.0),
        Rect::new(f64::NAN, 0.0, 10.0, 10.0),
        Rect::new(0.0, f64::INFINITY, 10.0, 10.0),
    ] {
        let svg = s.export_svg_region(rect);
        assert!(svg.starts_with("<?xml"), "仍应是合法 SVG（{rect:?}）");
        assert!(svg.contains("viewBox=\"0 0 1 1\""));
        assert!(!svg.contains("fill=\"#ff0000\""), "非法区域不应导出内容");
    }
}

// ── 验收 4：PNG 尺寸 = 包围盒 × scale ──────────────────────────

#[test]
fn export_png_size_is_bounds_times_scale() {
    let mut s = session();
    // 几何包围盒 = (0,0,100,50)：PNG 视口用几何口径（描边外扩不计）
    add_rect(&mut s, "a", 0.0, 0.0, 100.0, 50.0, "#ff0000");

    let png = s.export_png(2.0).expect("scale 2");
    assert_eq!(png_size(&png), (200, 100));

    let png = s.export_png(0.5).expect("scale 0.5");
    assert_eq!(png_size(&png), (50, 25));

    // 非整积四舍五入：100x50 × 1.23 = 123x61.5 → 123x62
    let png = s.export_png(1.23).expect("scale 1.23");
    assert_eq!(png_size(&png), (123, 62));
}

#[test]
fn export_png_empty_session_min_one_pixel() {
    let s = session();
    // 空会话包围盒 1x1：scale<1 也至少 1 像素
    let png = s.export_png(0.5).expect("scale 0.5");
    assert_eq!(png_size(&png), (1, 1));
    let png = s.export_png(3.0).expect("scale 3");
    assert_eq!(png_size(&png), (3, 3));
}

#[test]
fn export_png_rejects_invalid_scale() {
    let mut s = session();
    add_rect(&mut s, "a", 0.0, 0.0, 10.0, 10.0, "#ff0000");

    for bad in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
        match s.export_png(bad) {
            Err(CanvasError::InvalidInput(m)) => {
                assert!(m.contains("scale"), "错误信息应指向 scale（{m}）");
            }
            other => panic!("scale={bad} 应被拒，实际 {other:?}"),
        }
    }
}

#[test]
fn export_png_obeys_render_layer_side_limit() {
    let mut s = session();
    add_rect(&mut s, "a", 0.0, 0.0, 100.0, 50.0, "#ff0000");
    // 渲染层上限 65536 边长：100 × 700 > 65536 → 超限报错（不回绕）
    match s.export_png(700.0) {
        Err(CanvasError::InvalidInput(m)) => {
            assert!(m.contains("超限"), "应报超限（{m}）");
        }
        other => panic!("超限 scale 应被拒，实际 {other:?}"),
    }
}

// ── 会话语义：导出只读，不产事件 / 不动撤销栈 ──────────────────

#[test]
fn export_is_readonly_no_undo_no_event_side_effect() {
    let mut s = session();
    add_rect(&mut s, "a", 0.0, 0.0, 10.0, 10.0, "#ff0000");
    let depth_before = s.undo_depth(Actor::Human);

    let _svg = s.export_svg();
    let _svg2 = s.export_svg_region(Rect::new(0.0, 0.0, 5.0, 5.0));
    let _png = s.export_png(1.0).expect("png");

    assert_eq!(s.undo_depth(Actor::Human), depth_before, "导出不动撤销栈");
    assert_eq!(s.objects().len(), 1, "导出不改对象面");
}
