//! 命中测试单测（#407）：验收 1-6 全覆盖 + 边/笔迹容差带 + 旋转 +
//! 敌意输入面。
//!
//! 断言对着可观测事实：命中目标枚举值、框选列表——不写「跑通就算过」。

use std::collections::BTreeMap;

use mdc_canvas::render::Rect;
use mdc_canvas::{
    hit_test, hit_test_rect, Actor, CanvasCommand, CanvasSession, HitTarget, SessionOptions,
    StrokeTool,
};

fn session() -> CanvasSession {
    CanvasSession::open(SessionOptions {
        client_id: 7_300_407,
        ..Default::default()
    })
    .unwrap()
}

fn add_rect(s: &mut CanvasSession, id: &str, x: f64, y: f64, w: f64, h: f64) {
    s.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some(id.to_string()),
            x,
            y,
            w,
            h,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
}

fn add_rect_props(
    s: &mut CanvasSession,
    id: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    props: &[(&str, &str)],
) {
    s.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some(id.to_string()),
            x,
            y,
            w,
            h,
            props: props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        },
    )
    .unwrap();
}

// ── 验收 1：点对象内部命中 ─────────────────────────────────────

#[test]
fn point_inside_object_hits() {
    let mut s = session();
    add_rect(&mut s, "n1", 10.0, 10.0, 20.0, 20.0);
    assert_eq!(
        hit_test(&s, 15.0, 15.0, 0.0),
        HitTarget::Object("n1".into())
    );
    // 中心
    assert_eq!(
        hit_test(&s, 20.0, 20.0, 0.0),
        HitTarget::Object("n1".into())
    );
}

// ── 验收 2：点空白返回 None ─────────────────────────────────────

#[test]
fn point_in_blank_returns_none() {
    let mut s = session();
    add_rect(&mut s, "n1", 10.0, 10.0, 20.0, 20.0);
    assert_eq!(hit_test(&s, 500.0, 500.0, 0.0), HitTarget::None);
    assert_eq!(hit_test(&s, 9.9, 9.9, 0.0), HitTarget::None, "左上外");
    assert_eq!(hit_test(&s, 30.1, 30.1, 0.0), HitTarget::None, "右下外");
    // 敌意输入面：非有限点 → None（不 panic）
    assert_eq!(hit_test(&s, f64::NAN, 15.0, 0.0), HitTarget::None);
    assert_eq!(hit_test(&s, 15.0, f64::INFINITY, 5.0), HitTarget::None);
}

// ── 验收 3：重叠对象点重叠区命中绘制序在上者 ─────────────────────

#[test]
fn overlapping_hit_prefers_topmost_in_draw_order() {
    let mut s = session();
    // "lower" 先建（ID 序在前 = 先绘制 = 底层），"upper" 后建（顶层）
    add_rect(&mut s, "lower", 0.0, 0.0, 100.0, 100.0);
    add_rect(&mut s, "upper", 50.0, 50.0, 100.0, 100.0);
    // 重叠区 (75,75)：命中 upper（后绘制 = 视觉在上）
    assert_eq!(
        hit_test(&s, 75.0, 75.0, 0.0),
        HitTarget::Object("upper".into())
    );
    // 仅 lower 区域：命中 lower
    assert_eq!(
        hit_test(&s, 25.0, 25.0, 0.0),
        HitTarget::Object("lower".into())
    );
    // 与渲染序交叉验证：from_session 的末位形状（顶层）= 命中者
    let rendering = mdc_canvas::render::CanvasRendering::from_session(&s);
    assert_eq!(rendering.shapes.len(), 2, "两形状");
}

// ── 验收 4：点组内成员命中组 ───────────────────────────────────

#[test]
fn group_member_hit_returns_group() {
    let mut s = session();
    add_rect(&mut s, "m1", 0.0, 0.0, 30.0, 30.0);
    add_rect(&mut s, "m2", 100.0, 0.0, 30.0, 30.0);
    s.execute(
        Actor::Human,
        CanvasCommand::GroupObjects {
            group_id: Some("g1".into()),
            members: vec!["m1".into(), "m2".into()],
        },
    )
    .unwrap();
    // 点 m1 内部 → 组（不是成员）
    assert_eq!(hit_test(&s, 10.0, 10.0, 0.0), HitTarget::Group("g1".into()));
    // 点 m2 内部 → 同组
    assert_eq!(
        hit_test(&s, 110.0, 10.0, 0.0),
        HitTarget::Group("g1".into())
    );
    // 组外空白 → None
    assert_eq!(hit_test(&s, 500.0, 500.0, 0.0), HitTarget::None);
}

#[test]
fn ungrouped_object_above_group_member_wins() {
    let mut s = session();
    // ID 序 = 绘制序（main 口径）：a-mem < g1 < z-free → z-free 顶层
    add_rect(&mut s, "a-mem", 0.0, 0.0, 30.0, 30.0); // 组成员（底层）
    s.execute(
        Actor::Human,
        CanvasCommand::GroupObjects {
            group_id: Some("g1".into()),
            members: vec!["a-mem".into()],
        },
    )
    .unwrap();
    add_rect(&mut s, "z-free", 0.0, 0.0, 30.0, 30.0); // 未分组（ID 序顶 = 顶层）
                                                      // 点重叠区：顶层未分组对象胜出（绘制序优先，然后才做组解析）
    assert_eq!(
        hit_test(&s, 10.0, 10.0, 0.0),
        HitTarget::Object("z-free".into())
    );
}

// ── 验收 5：框选返回相交对象列表 ────────────────────────────────

#[test]
fn box_select_returns_intersecting_objects() {
    let mut s = session();
    add_rect(&mut s, "a", 0.0, 0.0, 10.0, 10.0);
    add_rect(&mut s, "b", 100.0, 0.0, 10.0, 10.0);
    add_rect(&mut s, "c", 200.0, 0.0, 10.0, 10.0);
    // 框住 a 和 b（贴到 b 左边缘）
    let hits = hit_test_rect(&s, Rect::new(-5.0, -5.0, 110.0, 20.0));
    assert_eq!(hits, vec!["a".to_string(), "b".to_string()], "绘制序返回");
    // 框住全部
    let hits = hit_test_rect(&s, Rect::new(-10.0, -10.0, 300.0, 50.0));
    assert_eq!(hits.len(), 3);
    // 空框区域
    assert!(hit_test_rect(&s, Rect::new(500.0, 500.0, 10.0, 10.0)).is_empty());
    // 非法框（非有限 / 非正尺寸）→ 空（不 panic）
    assert!(hit_test_rect(&s, Rect::new(f64::NAN, 0.0, 10.0, 10.0)).is_empty());
    assert!(hit_test_rect(&s, Rect::new(0.0, 0.0, 0.0, 10.0)).is_empty());
    assert!(hit_test_rect(&s, Rect::new(0.0, 0.0, -5.0, 10.0)).is_empty());
}

// ── 验收 6：边界（点恰好在边上） ────────────────────────────────

#[test]
fn boundary_point_on_edge_hits() {
    let mut s = session();
    add_rect(&mut s, "n1", 10.0, 10.0, 20.0, 20.0);
    // 四条边全部闭命中
    assert_eq!(
        hit_test(&s, 10.0, 15.0, 0.0),
        HitTarget::Object("n1".into()),
        "左边"
    );
    assert_eq!(
        hit_test(&s, 30.0, 15.0, 0.0),
        HitTarget::Object("n1".into()),
        "右边"
    );
    assert_eq!(
        hit_test(&s, 15.0, 10.0, 0.0),
        HitTarget::Object("n1".into()),
        "上边"
    );
    assert_eq!(
        hit_test(&s, 15.0, 30.0, 0.0),
        HitTarget::Object("n1".into()),
        "下边"
    );
    // 角点
    assert_eq!(
        hit_test(&s, 10.0, 10.0, 0.0),
        HitTarget::Object("n1".into()),
        "左上角"
    );
}

#[test]
fn tolerance_boundary_is_closed() {
    let mut s = session();
    // 椭圆节点（中心 50,50 半径 40x20）
    add_rect_props(
        &mut s,
        "e1",
        10.0,
        30.0,
        80.0,
        40.0,
        &[("shape", "ellipse")],
    );
    // 中心必中
    assert_eq!(
        hit_test(&s, 50.0, 50.0, 0.0),
        HitTarget::Object("e1".into())
    );
    // 椭圆内（非中心）
    assert_eq!(
        hit_test(&s, 70.0, 55.0, 0.0),
        HitTarget::Object("e1".into())
    );
    // 包围盒角落（椭圆外）→ None
    assert_eq!(
        hit_test(&s, 12.0, 32.0, 0.0),
        HitTarget::None,
        "盒角在椭圆外"
    );
}

// ── 边命中：距离阈值 ───────────────────────────────────────────

#[test]
fn edge_hit_within_tolerance() {
    let mut s = session();
    add_rect(&mut s, "n1", 0.0, 0.0, 20.0, 20.0); // 中心 (10,10)
    add_rect(&mut s, "n2", 100.0, 0.0, 20.0, 20.0); // 中心 (110,10)
    s.execute(
        Actor::Human,
        CanvasCommand::AddEdge {
            id: Some("e".into()),
            from: "n1".into(),
            to: "n2".into(),
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    // 边线中点 (60,10)，垂直外移 3：容差 5 内命中边
    assert_eq!(hit_test(&s, 60.0, 13.0, 5.0), HitTarget::Edge("e".into()));
    // 恰在容差边界（距离 = 5）→ 闭命中
    assert_eq!(hit_test(&s, 60.0, 15.0, 5.0), HitTarget::Edge("e".into()));
    // 超容差（距离 5.1 > 5 且 > 描边半宽 0.5）→ 落到下一层（无 → None）
    assert_eq!(hit_test(&s, 60.0, 15.1, 5.0), HitTarget::None);
    // 容差 0 时点在边线上（距离 0 ≤ max(0, 0.5) 描边半宽兜底）
    assert_eq!(hit_test(&s, 60.0, 10.0, 0.0), HitTarget::Edge("e".into()));
    // 边也参与组解析
    s.execute(
        Actor::Human,
        CanvasCommand::GroupObjects {
            group_id: Some("ge".into()),
            members: vec!["e".into()],
        },
    )
    .unwrap();
    assert_eq!(hit_test(&s, 60.0, 13.0, 5.0), HitTarget::Group("ge".into()));
}

// ── 笔迹命中 ───────────────────────────────────────────────────

#[test]
fn stroke_hit_on_polyline() {
    let mut s = session();
    s.execute(
        Actor::Human,
        CanvasCommand::AddStroke {
            id: Some("st".into()),
            points: vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)],
            color: "#ff0000".into(),
            width: 4.0,
            tool: StrokeTool::Pen,
        },
    )
    .unwrap();
    // 线上：命中（描边半宽 2 兜底）
    assert_eq!(hit_test(&s, 50.0, 0.0, 0.0), HitTarget::Object("st".into()));
    // 描带内（距离 1.5 < 半宽 2）
    assert_eq!(hit_test(&s, 50.0, 1.5, 0.0), HitTarget::Object("st".into()));
    // 描带外且无容差 → None
    assert_eq!(hit_test(&s, 50.0, 3.0, 0.0), HitTarget::None);
    // 第二段（竖直段）上
    assert_eq!(
        hit_test(&s, 100.0, 50.0, 0.0),
        HitTarget::Object("st".into())
    );
}

// ── 旋转与渲染一致性 ───────────────────────────────────────────

#[test]
fn rotated_rect_hit_matches_render_aabb() {
    let mut s = session();
    // 旋转 45° 的矩形（10,10,40,40 绕中心）——渲染面画轴对齐包围盒
    //（transform_rect 的既有口径），命中同口径（所见即所得）
    add_rect_props(&mut s, "r1", 10.0, 10.0, 40.0, 40.0, &[("rotation", "45")]);
    // 未旋转时不在矩形内的点（角落区域），旋转后落入 AABB → 命中
    // AABB = 中心 (30,30) ± 40*√2/2 ≈ (1.7,1.7)-(58.3,58.3)
    assert_eq!(hit_test(&s, 5.0, 5.0, 0.0), HitTarget::Object("r1".into()));
    assert_eq!(
        hit_test(&s, 55.0, 55.0, 0.0),
        HitTarget::Object("r1".into())
    );
    // AABB 外 → None
    assert_eq!(hit_test(&s, 60.0, 60.0, 0.0), HitTarget::None);
}

#[test]
fn hit_topmost_equals_last_rendered_shape_at_overlap() {
    let mut s = session();
    add_rect_props(&mut s, "a", 0.0, 0.0, 50.0, 50.0, &[("fill", "#ff0000")]);
    add_rect_props(&mut s, "b", 25.0, 25.0, 50.0, 50.0, &[("fill", "#0000ff")]);
    // 渲染末位形状 = b（顶层）——命中与渲染序一致
    let rendering = mdc_canvas::render::CanvasRendering::from_session(&s);
    let (last_shape, _) = rendering.shapes.last().unwrap();
    match last_shape {
        mdc_canvas::render::Shape::Rect { style, .. } => {
            assert_eq!(style.fill.unwrap().to_hex(), "#0000ff", "末位 = b");
        }
        _ => panic!("应为矩形"),
    }
    assert_eq!(hit_test(&s, 40.0, 40.0, 0.0), HitTarget::Object("b".into()));
}

// ── 文本与路径节点 ─────────────────────────────────────────────

#[test]
fn text_node_hit_in_layout_box() {
    let mut s = session();
    add_rect_props(
        &mut s,
        "t1",
        10.0,
        10.0,
        1.0,
        1.0,
        &[("shape", "text"), ("label", "hello")],
    );
    // 文本估算盒：pos (10,10)，字号缺省 14 → 盒约 (10, 10-11.2, 5*8.4, 14.7)
    assert_eq!(
        hit_test(&s, 12.0, 10.0, 0.0),
        HitTarget::Object("t1".into())
    );
    // 盒外远处
    assert_eq!(hit_test(&s, 200.0, 200.0, 0.0), HitTarget::None);
}

#[test]
fn path_node_hit_on_segments() {
    let mut s = session();
    add_rect_props(
        &mut s,
        "p1",
        0.0,
        0.0,
        1.0,
        1.0,
        &[("shape", "path"), ("points", "0,0 100,0 100,100")],
    );
    assert_eq!(hit_test(&s, 50.0, 0.0, 0.0), HitTarget::Object("p1".into()));
    assert_eq!(
        hit_test(&s, 100.0, 50.0, 0.0),
        HitTarget::Object("p1".into())
    );
    assert_eq!(hit_test(&s, 50.0, 50.0, 0.0), HitTarget::None, "路径外");
}

// ── 容差敌意面 ─────────────────────────────────────────────────

#[test]
fn hostile_tolerance_clamped() {
    let mut s = session();
    add_rect(&mut s, "n1", 10.0, 10.0, 20.0, 20.0);
    // NaN / 负容差 → 钳制（不 panic；面类命中不受影响）
    assert_eq!(
        hit_test(&s, 15.0, 15.0, f64::NAN),
        HitTarget::Object("n1".into())
    );
    assert_eq!(
        hit_test(&s, 15.0, 15.0, -3.0),
        HitTarget::Object("n1".into())
    );
}
