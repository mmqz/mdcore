//! 笔迹对象（#351 / #44 步1）引擎面测试。
//!
//! 覆盖：加一笔读回（objects 全字段）／样式终态写／点列不可改面／
//! 敌意输入校验全矩阵／撤销重做（同 ID 复原）／快照往返／双会话 CRDT
//! 收敛／事件 touched／指纹语义（样式与点列皆入指纹）／渲染最小面。

use std::collections::BTreeMap;
use std::sync::Arc;

use mdc_canvas::{
    render::{shape_from_object, Color, LineCap, Shape},
    Actor, CanvasCommand, CanvasError, CanvasObject, CanvasSession, ObjectKind, SessionOptions,
    StrokeSpec, StrokeTool,
};

fn session(tag: u64) -> CanvasSession {
    CanvasSession::open(SessionOptions {
        client_id: tag,
        clock_ms: Some(Arc::new(move || 1_700_000_000_000 + tag)),
        ..Default::default()
    })
    .unwrap()
}

fn zigzag(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| (i as f64 * 10.0, if i % 2 == 0 { 20.0 } else { 80.0 }))
        .collect()
}

fn add_stroke(s: &mut CanvasSession, id: &str, points: Vec<(f64, f64)>) -> String {
    s.execute(
        Actor::Human,
        CanvasCommand::AddStroke {
            id: Some(id.to_string()),
            points,
            width: 2.5,
            color: "#1a73e8".into(),
            tool: StrokeTool::Pen,
        },
    )
    .unwrap()
    .outcome
    .id_of()
    .expect("StrokeAdded 回执")
}

/// 回执 → 对象 ID（测试面小工具）。
trait IdOf {
    fn id_of(&self) -> Option<String>;
}
impl IdOf for mdc_canvas::CommandOutcome {
    fn id_of(&self) -> Option<String> {
        match self {
            mdc_canvas::CommandOutcome::StrokeAdded { id }
            | mdc_canvas::CommandOutcome::StrokeStyled { id } => Some(id.clone()),
            _ => None,
        }
    }
}

// ── 读模型面 ─────────────────────────────────────────────────────

#[test]
fn add_stroke_roundtrips_through_read_model() {
    let mut s = session(1);
    let id = add_stroke(&mut s, "s1", zigzag(4));
    assert_eq!(id, "s1");
    let objs = s.objects();
    assert_eq!(objs.len(), 1, "objects() 应含且仅含该笔");
    let o = &objs[0];
    assert_eq!(o.kind, ObjectKind::Stroke);
    assert_eq!(o.stroke.as_ref().unwrap().points, zigzag(4));
    assert_eq!(o.stroke.as_ref().unwrap().width, 2.5);
    assert_eq!(o.stroke.as_ref().unwrap().color, "#1a73e8");
    assert_eq!(o.stroke.as_ref().unwrap().tool, StrokeTool::Pen);
    // 几何字段恒 0（不存派生数据）。
    assert_eq!((o.x, o.y, o.w, o.h), (0.0, 0.0, 0.0, 0.0));
    // 非 Stroke 对象的 stroke 字段 = None。
    s.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n1".into()),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    let n = s.find_object("n1").unwrap();
    assert!(n.stroke.is_none());
}

#[test]
fn auto_id_allocation_for_strokes() {
    let mut s = session(42);
    let id = s
        .execute(
            Actor::Human,
            CanvasCommand::AddStroke {
                id: None,
                points: zigzag(2),
                width: 2.0,
                color: "#000000".into(),
                tool: StrokeTool::Pen,
            },
        )
        .unwrap()
        .outcome
        .id_of()
        .expect("StrokeAdded 回执");
    assert_eq!(
        id, "42-1",
        "自动 ID = {{client_id}}-{{seq}}（与 Node/Edge 同序）"
    );
    assert!(s.find_object("42-1").is_some());
}

#[test]
fn highlighter_and_eraser_tool_roundtrip() {
    let mut s = session(1);
    for (id, tool) in [("h1", StrokeTool::Highlighter), ("e1", StrokeTool::Eraser)] {
        s.execute(
            Actor::Human,
            CanvasCommand::AddStroke {
                id: Some(id.into()),
                points: zigzag(3),
                width: 8.0,
                color: "#ffee00".into(),
                tool,
            },
        )
        .unwrap();
    }
    let h = s.find_object("h1").unwrap();
    assert_eq!(h.stroke.as_ref().unwrap().tool, StrokeTool::Highlighter);
    let e = s.find_object("e1").unwrap();
    assert_eq!(e.stroke.as_ref().unwrap().tool, StrokeTool::Eraser);
}

// ── 样式面 ───────────────────────────────────────────────────────

#[test]
fn set_stroke_style_updates_width_color_and_fingerprint() {
    let mut s = session(1);
    add_stroke(&mut s, "s1", zigzag(5));
    let fp_before = s.object_fingerprint("s1").unwrap();
    s.execute(
        Actor::Human,
        CanvasCommand::SetStrokeStyle {
            id: "s1".into(),
            width: 6.0,
            color: "#ff0000".into(),
        },
    )
    .unwrap();
    let o = s.find_object("s1").unwrap();
    let sp = o.stroke.as_ref().unwrap();
    assert_eq!((sp.width, sp.color.as_str()), (6.0, "#ff0000"));
    // 点列不动（样式写不触点列——删旧加新纪律的另一半）。
    assert_eq!(sp.points, zigzag(5));
    // 样式变化 = 语义变更（证据 Stale 语义）。
    let fp_after = s.object_fingerprint("s1").unwrap();
    assert_ne!(fp_before, fp_after, "样式变化 → 指纹变化");
}

#[test]
fn set_stroke_style_rejects_non_stroke_objects() {
    let mut s = session(1);
    s.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n1".into()),
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    let e = s
        .execute(
            Actor::Human,
            CanvasCommand::SetStrokeStyle {
                id: "n1".into(),
                width: 1.0,
                color: "#000000".into(),
            },
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::KindMismatch { .. }), "{e:?}");
    let e = s
        .execute(
            Actor::Human,
            CanvasCommand::SetStrokeStyle {
                id: "nope".into(),
                width: 1.0,
                color: "#000000".into(),
            },
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::ObjectNotFound { .. }), "{e:?}");
}

#[test]
fn set_props_still_works_on_strokes() {
    // 开放属性面对全部对象开放（笔迹同权——扩展面如自定义元数据）。
    let mut s = session(1);
    add_stroke(&mut s, "s1", zigzag(2));
    let mut props = BTreeMap::new();
    props.insert("label".to_string(), "签名".to_string());
    s.execute(
        Actor::Human,
        CanvasCommand::SetProps {
            id: "s1".into(),
            props,
        },
    )
    .unwrap();
    assert_eq!(
        s.find_object("s1").unwrap().props.get("label"),
        Some(&"签名".to_string())
    );
}

// ── 敌意输入校验矩阵 ─────────────────────────────────────────────

#[test]
fn add_stroke_validation_matrix() {
    let mut s = session(1);
    let try_add =
        |s: &mut CanvasSession, id: &str, points: Vec<(f64, f64)>, width: f64, color: &str| {
            s.execute(
                Actor::Human,
                CanvasCommand::AddStroke {
                    id: Some(id.into()),
                    points,
                    width,
                    color: color.into(),
                    tool: StrokeTool::Pen,
                },
            )
        };
    // ── 拒收面（每例独立 id——失败零副作用）──
    // 空点列 / 单点。
    for (i, pts) in [vec![], vec![(0.0, 0.0)]].into_iter().enumerate() {
        let e = try_add(&mut s, &format!("bad-pts-{i}"), pts, 2.0, "#000000").unwrap_err();
        assert!(matches!(e, CanvasError::InvalidInput(_)), "{e:?}");
    }
    // 点数超上界（8193 点）。
    let e = try_add(&mut s, "bad-n", zigzag(8193), 2.0, "#000000").unwrap_err();
    assert!(
        matches!(
            e,
            CanvasError::Limit {
                size: 8193,
                limit: 8192
            }
        ),
        "{e:?}"
    );
    // 非有限坐标（NaN / inf，x 轴与 y 轴各一）。
    for (i, bad) in [
        vec![(f64::NAN, 0.0), (1.0, 1.0)],
        vec![(0.0, f64::INFINITY), (1.0, 1.0)],
    ]
    .into_iter()
    .enumerate()
    {
        let e = try_add(&mut s, &format!("bad-finite-{i}"), bad, 2.0, "#000000").unwrap_err();
        assert!(matches!(e, CanvasError::NonFiniteGeometry { .. }), "{e:?}");
    }
    // 宽度域：0 / 负 / 超 64 / NaN。
    for (i, w) in [0.0, -1.0, 64.1, f64::NAN].into_iter().enumerate() {
        let e = try_add(&mut s, &format!("bad-w-{i}"), zigzag(2), w, "#000000").unwrap_err();
        assert!(matches!(e, CanvasError::InvalidInput(_)), "w={w}: {e:?}");
    }
    // 颜色非严格 #RRGGBB。
    for (i, c) in [
        "red", "#fff", "fff", "#GGGGGG", "#12345", "#1234567", "", "#12 456",
    ]
    .into_iter()
    .enumerate()
    {
        let e = try_add(&mut s, &format!("bad-c-{i}"), zigzag(2), 2.0, c).unwrap_err();
        assert!(matches!(e, CanvasError::InvalidInput(_)), "c={c:?}: {e:?}");
    }
    // 空对象 ID。
    let e = try_add(&mut s, "", zigzag(2), 2.0, "#000000").unwrap_err();
    assert!(matches!(e, CanvasError::InvalidInput(_)), "{e:?}");
    // ── 通过面（域内边界，独立 id）──
    try_add(&mut s, "ok-max-pts", zigzag(8192), 2.0, "#000000").unwrap();
    try_add(&mut s, "ok-w-lo", zigzag(2), 0.1, "#000000").unwrap();
    try_add(&mut s, "ok-w-hi", zigzag(2), 64.0, "#000000").unwrap();
    try_add(&mut s, "ok-c-upper", zigzag(2), 2.0, "#AbCdEf").unwrap(); // 大写十六进制合法
                                                                       // 重复 ID。
    try_add(&mut s, "dup", zigzag(2), 2.0, "#000000").unwrap();
    let e = try_add(&mut s, "dup", zigzag(2), 2.0, "#000000").unwrap_err();
    assert!(matches!(e, CanvasError::DuplicateId { .. }), "{e:?}");
    // 拒收面全部零副作用：仅成功样例入列。
    assert_eq!(
        s.objects().len(),
        5,
        "5 = ok-max-pts / ok-w-lo / ok-w-hi / ok-c-upper / dup"
    );
}

// ── 撤销/重做 ────────────────────────────────────────────────────

#[test]
fn undo_redo_add_and_style() {
    let mut s = session(1);
    add_stroke(&mut s, "s1", zigzag(3));
    s.execute(
        Actor::Human,
        CanvasCommand::SetStrokeStyle {
            id: "s1".into(),
            width: 9.0,
            color: "#00ff00".into(),
        },
    )
    .unwrap();
    assert_eq!(s.undo_depth(Actor::Human), 2);
    // 撤样式 → 回 2.5/#1a73e8。
    assert!(s.undo(Actor::Human));
    let sp = s.find_object("s1").unwrap().stroke.unwrap();
    assert_eq!((sp.width, sp.color.as_str()), (2.5, "#1a73e8"));
    // 撤加笔 → 同 ID 消失（Map 键身份——复原时同 ID 回归）。
    assert!(s.undo(Actor::Human));
    assert!(s.find_object("s1").is_none());
    // 重做两步 → 样式与点列完整回归。
    assert!(s.redo(Actor::Human));
    assert!(s.redo(Actor::Human));
    let sp = s.find_object("s1").unwrap().stroke.unwrap();
    assert_eq!(sp.points, zigzag(3));
    assert_eq!((sp.width, sp.color.as_str()), (9.0, "#00ff00"));
}

#[test]
fn remove_object_removes_stroke() {
    let mut s = session(1);
    add_stroke(&mut s, "s1", zigzag(2));
    s.execute(
        Actor::Human,
        CanvasCommand::RemoveObject { id: "s1".into() },
    )
    .unwrap();
    assert!(s.objects().is_empty());
    assert!(s.undo(Actor::Human));
    assert_eq!(s.objects().len(), 1, "删除撤销 → 同 ID 复原");
}

// ── 快照与协同 ───────────────────────────────────────────────────

#[test]
fn snapshot_restore_preserves_strokes() {
    let mut s = session(7);
    add_stroke(&mut s, "s1", zigzag(6));
    s.execute(
        Actor::Human,
        CanvasCommand::SetStrokeStyle {
            id: "s1".into(),
            width: 4.0,
            color: "#ab12cd".into(),
        },
    )
    .unwrap();
    let snap = s.authoritative_snapshot().unwrap();
    let r = CanvasSession::restore(
        SessionOptions {
            client_id: 7,
            clock_ms: Some(Arc::new(|| 42)),
            ..Default::default()
        },
        &snap,
    )
    .unwrap();
    let sp = r.find_object("s1").unwrap().stroke.unwrap();
    assert_eq!(sp.points, zigzag(6));
    assert_eq!((sp.width, sp.color.as_str()), (4.0, "#ab12cd"));
}

#[test]
fn two_session_stroke_convergence() {
    let mut a = session(1);
    let mut b = session(2);
    add_stroke(&mut a, "s1", zigzag(4));
    let sv_b = b.state_vector();
    let diff = a.encode_diff(&sv_b).unwrap();
    b.apply_update(&diff).unwrap();
    let (oa, ob) = (a.objects(), b.objects());
    assert_eq!(oa, ob, "双端读模型应逐字段收敛");
}

// ── 事件与指纹 ───────────────────────────────────────────────────

#[test]
fn command_event_touched_stroke_id() {
    use mdc_canvas::CanvasChangeSource;
    let mut s = session(1);
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = seen.clone();
    // 订阅句柄必须保持（Drop = 退订）。
    let _sub = s.subscribe(move |ev| {
        if let CanvasChangeSource::Command { .. } = ev.source {
            sink.lock().unwrap().push(ev.touched.clone());
        }
    });
    add_stroke(&mut s, "s1", zigzag(2));
    let got = seen.lock().unwrap();
    assert_eq!(got.len(), 1, "恰一个 Command 事件");
    assert_eq!(got[0], vec!["s1".to_string()], "touched = 笔迹 ID");
}

#[test]
fn fingerprint_reflects_points_width_color_tool() {
    let mut s = session(1);
    add_stroke(&mut s, "a", zigzag(3));
    add_stroke(&mut s, "b", zigzag(4)); // 点列不同
    s.execute(
        Actor::Human,
        CanvasCommand::AddStroke {
            id: Some("c".into()),
            points: zigzag(3),
            width: 2.5,
            color: "#1a73e8".into(),
            tool: StrokeTool::Highlighter, // 工具不同
        },
    )
    .unwrap();
    let fa = s.object_fingerprint("a").unwrap();
    let fb = s.object_fingerprint("b").unwrap();
    let fc = s.object_fingerprint("c").unwrap();
    assert_ne!(fa, fb, "点列不同 → 指纹不同");
    assert_ne!(fa, fc, "工具不同 → 指纹不同");
    assert!(fa.starts_with("fnv64:"));
    // 样式变化 → 对象指纹与全画布指纹皆变（证据 Stale 语义）。
    let fp_all_before = s.canvas_fingerprint();
    s.execute(
        Actor::Human,
        CanvasCommand::SetStrokeStyle {
            id: "a".into(),
            width: 3.0,
            color: "#000000".into(),
        },
    )
    .unwrap();
    assert_ne!(s.object_fingerprint("a").unwrap(), fa);
    assert_ne!(s.canvas_fingerprint(), fp_all_before);
}

// ── 渲染最小面 ───────────────────────────────────────────────────

#[test]
fn stroke_renders_as_polyline_path() {
    let obj = CanvasObject {
        id: "s1".into(),
        kind: ObjectKind::Stroke,
        stroke: Some(StrokeSpec {
            points: zigzag(3),
            width: 3.0,
            color: "#1a73e8".into(),
            tool: StrokeTool::Pen,
        }),
        ..Default::default()
    };
    let (shape, _tf) = shape_from_object(&obj, &[]).expect("笔迹应成图元");
    match shape {
        Shape::Path { cmds, style } => {
            assert_eq!(cmds.len(), 3, "3 点折线 = MoveTo + 2 LineTo");
            assert!(style.fill.is_none(), "笔迹不填充");
            let st = style.stroke.expect("有描边");
            assert_eq!(st.color, Color::rgb(0x1a, 0x73, 0xe8));
            assert_eq!(st.width, 3.0);
            assert_eq!(st.cap, LineCap::Round, "圆端帽");
        }
        other => panic!("笔迹应为 Path，实际 {other:?}"),
    }
}

#[test]
fn highlighter_is_transparent_and_eraser_dashed_gray() {
    let mk_obj = |tool: StrokeTool| CanvasObject {
        id: "s".into(),
        kind: ObjectKind::Stroke,
        stroke: Some(StrokeSpec {
            points: zigzag(2),
            width: 8.0,
            color: "#ffee00".into(),
            tool,
        }),
        ..Default::default()
    };
    let (h, _) = shape_from_object(&mk_obj(StrokeTool::Highlighter), &[]).unwrap();
    match h {
        Shape::Path { style, .. } => {
            let st = style.stroke.unwrap();
            assert_eq!(st.color.a, 115, "荧光笔 alpha 0.45 → round(255*0.45)=115");
        }
        _ => panic!(),
    }
    let (e, _) = shape_from_object(&mk_obj(StrokeTool::Eraser), &[]).unwrap();
    match e {
        Shape::Path { style, .. } => {
            let st = style.stroke.unwrap();
            assert_eq!(
                st.color,
                Color {
                    r: 0x9a,
                    g: 0x9a,
                    b: 0x9a,
                    a: 204
                },
                "橡皮擦灰色（alpha 0.8 → 204）"
            );
            assert_eq!(st.dash, mdc_canvas::render::StrokeDash::new(6, 4));
        }
        _ => panic!(),
    }
}

#[test]
fn stroke_with_fewer_than_two_points_is_skipped() {
    for pts in [vec![], vec![(1.0, 2.0)]] {
        let obj = CanvasObject {
            id: "s".into(),
            kind: ObjectKind::Stroke,
            stroke: Some(StrokeSpec {
                points: pts,
                width: 2.0,
                color: "#000000".into(),
                tool: StrokeTool::Pen,
            }),
            ..Default::default()
        };
        assert!(shape_from_object(&obj, &[]).is_none(), "<2 点跳过不画");
    }
    // 载荷缺失（kind=Stroke 但 stroke=None——敌意/腐损面）同样跳过。
    let obj = CanvasObject {
        id: "s".into(),
        kind: ObjectKind::Stroke,
        ..Default::default()
    };
    assert!(shape_from_object(&obj, &[]).is_none());
}

#[test]
fn hostile_stroke_payload_falls_back_without_panic() {
    // 敌意远端载荷：NaN 宽度 + 非法颜色——渲染面回退缺省，不 panic。
    let obj = CanvasObject {
        id: "s".into(),
        kind: ObjectKind::Stroke,
        stroke: Some(StrokeSpec {
            points: vec![(0.0, 0.0), (10.0, f64::NAN)],
            width: f64::NAN,
            color: "not-a-color".into(),
            tool: StrokeTool::Pen,
        }),
        ..Default::default()
    };
    let (shape, _) = shape_from_object(&obj, &[]).expect("敌意载荷也出图元");
    match shape {
        Shape::Path { style, .. } => {
            let st = style.stroke.unwrap();
            assert_eq!(st.width, 1.0, "非有限宽度回退 1.0");
            assert_eq!(st.color, Color::BLACK, "非法颜色回退黑");
        }
        other => panic!("应为 Path，实际 {other:?}"),
    }
}
