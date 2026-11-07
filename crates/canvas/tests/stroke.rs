//! 笔迹对象面测试（#350——#44 落地顺序第 1 步）。
//! 覆盖：加笔读模型往返 / 校验面 / 样式终态替换 + 指纹 / 删除复原身份 /
//! 撤销重做 / CRDT 双端收敛 / 快照恢复 / 全画布指纹含笔迹。

use mdc_canvas::{
    Actor, CanvasCommand, CanvasError, CanvasSession, ObjectKind, SessionOptions, StrokeTool,
};

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

fn pen(id: Option<&str>) -> CanvasCommand {
    CanvasCommand::AddStroke {
        id: id.map(str::to_string),
        points: vec![(10.0, 20.0), (40.0, 25.0), (80.0, 60.0)],
        color: "#1a73e8".into(),
        width: 2.5,
        tool: StrokeTool::Pen,
    }
}

#[test]
fn add_stroke_roundtrip_read_model() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    let r = c.execute(Actor::Human, pen(Some("s-1"))).unwrap();
    assert!(
        matches!(r.outcome, mdc_canvas::CommandOutcome::StrokeAdded { ref id } if id == "s-1"),
        "回执应为 StrokeAdded：{:?}",
        r.outcome
    );
    let o = c.find_object("s-1").expect("读模型应含新笔迹");
    assert_eq!(o.kind, ObjectKind::Stroke);
    assert_eq!(o.points, vec![(10.0, 20.0), (40.0, 25.0), (80.0, 60.0)]);
    assert_eq!(o.color.as_deref(), Some("#1a73e8"));
    assert_eq!(o.width, Some(2.5));
    assert_eq!(o.tool, Some(StrokeTool::Pen));
    // 非笔迹专属字段缺省形状（同 Edge 无几何的纪律）。
    assert_eq!((o.x, o.y, o.w, o.h), (0.0, 0.0, 0.0, 0.0));
    assert!(o.from.is_none() && o.to.is_none() && o.members.is_empty());
    // objects() 全量面同样含（ID 确定性序）。
    assert!(c.objects().iter().any(|o| o.id == "s-1"));
}

#[test]
fn ai_actor_same_entry_point() {
    // 人类与 AI 同一入口（受控命令集——AI 无笔迹特例）。
    let mut c = CanvasSession::open(opts(1)).unwrap();
    c.execute(Actor::Ai, pen(None)).unwrap();
    let objs = c.objects();
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0].kind, ObjectKind::Stroke);
    assert!(
        objs[0].id.starts_with("1-"),
        "自动 ID = client-seq：{}",
        objs[0].id
    );
}

#[test]
fn add_stroke_validation_rejections() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    // 点数 < 2（单击不成笔）。
    let e = c
        .execute(
            Actor::Human,
            CanvasCommand::AddStroke {
                id: None,
                points: vec![(0.0, 0.0)],
                color: "#000".into(),
                width: 1.0,
                tool: StrokeTool::Pen,
            },
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::InvalidInput(_)), "{e:?}");
    // NaN 坐标。
    let e = c
        .execute(
            Actor::Human,
            CanvasCommand::AddStroke {
                id: None,
                points: vec![(0.0, f64::NAN), (1.0, 1.0)],
                color: "#000".into(),
                width: 1.0,
                tool: StrokeTool::Pen,
            },
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::NonFiniteGeometry { .. }), "{e:?}");
    // 坏颜色（无 # / 8 位 RGBA / 非十六进制）。
    for bad in ["red", "#12345", "#11223344", "#12g456"] {
        let e = c
            .execute(
                Actor::Human,
                CanvasCommand::AddStroke {
                    id: None,
                    points: vec![(0.0, 0.0), (1.0, 1.0)],
                    color: bad.into(),
                    width: 1.0,
                    tool: StrokeTool::Pen,
                },
            )
            .unwrap_err();
        assert!(
            matches!(e, CanvasError::InvalidInput(_)),
            "颜色 {bad:?} 应拒：{e:?}"
        );
    }
    // 线宽 ≤ 0 / NaN。
    for bad_w in [0.0, -1.0, f64::NAN] {
        let e = c
            .execute(
                Actor::Human,
                CanvasCommand::AddStroke {
                    id: None,
                    points: vec![(0.0, 0.0), (1.0, 1.0)],
                    color: "#000".into(),
                    width: bad_w,
                    tool: StrokeTool::Pen,
                },
            )
            .unwrap_err();
        assert!(
            matches!(
                e,
                CanvasError::InvalidInput(_) | CanvasError::NonFiniteGeometry { .. }
            ),
            "线宽 {bad_w} 应拒：{e:?}"
        );
    }
    // 空 ID / 撞 ID。
    let e = c
        .execute(
            Actor::Human,
            CanvasCommand::AddStroke {
                id: Some(String::new()),
                points: vec![(0.0, 0.0), (1.0, 1.0)],
                color: "#000".into(),
                width: 1.0,
                tool: StrokeTool::Pen,
            },
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::InvalidInput(_)), "{e:?}");
    c.execute(Actor::Human, pen(Some("s-1"))).unwrap();
    let e = c.execute(Actor::Human, pen(Some("s-1"))).unwrap_err();
    assert!(matches!(e, CanvasError::DuplicateId { .. }), "{e:?}");
    // 校验失败零残留（命令拒绝 = 无事务字节）。
    assert_eq!(c.objects().len(), 1);
}

#[test]
fn set_stroke_style_terminal_replacement_and_fingerprint() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    c.execute(Actor::Human, pen(Some("s-1"))).unwrap();
    let fp0 = c.object_fingerprint("s-1").unwrap();
    // 改笔色 = 语义变更（证据锚应失效）。
    c.execute(
        Actor::Human,
        CanvasCommand::SetStrokeStyle {
            id: "s-1".into(),
            color: "#ff0000".into(),
            width: 6.0,
            tool: StrokeTool::Highlighter,
        },
    )
    .unwrap();
    let o = c.find_object("s-1").unwrap();
    assert_eq!(o.color.as_deref(), Some("#ff0000"));
    assert_eq!(o.width, Some(6.0));
    assert_eq!(o.tool, Some(StrokeTool::Highlighter));
    // 样式替换不动点列（改粗细零数据迁移——#44 存点不存 outline 的红利）。
    assert_eq!(o.points.len(), 3);
    let fp1 = c.object_fingerprint("s-1").unwrap();
    assert_ne!(fp0, fp1, "改笔色/粗细/工具 = 语义变更 → 指纹变");
    // 撤销样式 → 指纹复原。
    assert!(c.undo(Actor::Human));
    assert_eq!(
        c.object_fingerprint("s-1").unwrap(),
        fp0,
        "撤销样式应复原指纹"
    );
    // 对 Node 调 SetStrokeStyle → KindMismatch；不存在 → 404。
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-1".into()),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            props: Default::default(),
        },
    )
    .unwrap();
    let e = c
        .execute(
            Actor::Human,
            CanvasCommand::SetStrokeStyle {
                id: "n-1".into(),
                color: "#000".into(),
                width: 1.0,
                tool: StrokeTool::Pen,
            },
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::KindMismatch { .. }), "{e:?}");
    let e = c
        .execute(
            Actor::Human,
            CanvasCommand::SetStrokeStyle {
                id: "no-such".into(),
                color: "#000".into(),
                width: 1.0,
                tool: StrokeTool::Pen,
            },
        )
        .unwrap_err();
    assert!(matches!(e, CanvasError::ObjectNotFound { .. }), "{e:?}");
}

#[test]
fn remove_then_undo_restores_same_id_and_payload() {
    // Map 键身份（lib.rs 模块文档承诺）：删除 → 撤销复原 = 同 ID 回归。
    let mut c = CanvasSession::open(opts(1)).unwrap();
    c.execute(Actor::Human, pen(Some("s-1"))).unwrap();
    let fp = c.object_fingerprint("s-1").unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::RemoveObject { id: "s-1".into() },
    )
    .unwrap();
    assert!(c.find_object("s-1").is_none());
    assert!(c.undo(Actor::Human));
    let o = c.find_object("s-1").expect("撤销删除应复原笔迹");
    assert_eq!(o.kind, ObjectKind::Stroke);
    assert_eq!(o.points.len(), 3);
    assert_eq!(c.object_fingerprint("s-1").unwrap(), fp, "复原载荷指纹恒等");
}

#[test]
fn undo_redo_add_stroke() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    let r = c.execute(Actor::Human, pen(Some("s-1"))).unwrap();
    assert_eq!(r.undo_depth, 1);
    assert!(c.undo(Actor::Human));
    assert!(c.find_object("s-1").is_none(), "撤销加笔 → 笔消失");
    assert!(c.redo(Actor::Human));
    assert_eq!(
        c.find_object("s-1").unwrap().points.len(),
        3,
        "重做 → 笔回来"
    );
}

#[test]
fn crdt_two_session_convergence() {
    // 双端各画一笔 → 互发差量 → 收敛（CRDT 面零笔迹特判）。
    let mut a = CanvasSession::open(opts(1)).unwrap();
    let mut b = CanvasSession::open(opts(2)).unwrap();
    a.execute(Actor::Human, pen(Some("s-a"))).unwrap();
    b.execute(
        Actor::Ai,
        CanvasCommand::AddStroke {
            id: Some("s-b".into()),
            points: vec![(0.0, 0.0), (100.0, 100.0)],
            color: "#ff0".into(),
            width: 8.0,
            tool: StrokeTool::Highlighter,
        },
    )
    .unwrap();
    let sv_a = a.state_vector();
    let diff_b = b.encode_diff(&sv_a).unwrap();
    a.apply_update(&diff_b).unwrap();
    let sv_b = b.state_vector();
    let diff_a = a.encode_diff(&sv_b).unwrap();
    b.apply_update(&diff_a).unwrap();
    // 收敛：两端读模型逐对象相等 + 全画布指纹相等。
    let (oa, ob) = (a.objects(), b.objects());
    assert_eq!(oa.len(), 2);
    assert_eq!(oa, ob, "双端对象集应收敛一致");
    assert_eq!(a.canvas_fingerprint(), b.canvas_fingerprint());
    // AI 笔在 b 端的撤销栈，a 端不入（origin 分栈纪律不变）。
    assert_eq!(b.undo_depth(Actor::Ai), 1);
    assert_eq!(a.undo_depth(Actor::Ai), 0);
}

#[test]
fn snapshot_restore_roundtrip() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    c.execute(Actor::Human, pen(Some("s-1"))).unwrap();
    let snap = c.authoritative_snapshot().unwrap();
    let restored = CanvasSession::restore(opts(9), &snap).unwrap();
    let o = restored.find_object("s-1").expect("快照恢复应含笔迹");
    assert_eq!(o.points, vec![(10.0, 20.0), (40.0, 25.0), (80.0, 60.0)]);
    assert_eq!(o.color.as_deref(), Some("#1a73e8"));
    assert_eq!(o.width, Some(2.5));
    assert_eq!(o.tool, Some(StrokeTool::Pen));
}

#[test]
fn canvas_fingerprint_covers_strokes() {
    // 全画布指纹含笔迹语义（画布作为受证文档面）。
    let mut c = CanvasSession::open(opts(1)).unwrap();
    let fp0 = c.canvas_fingerprint();
    c.execute(Actor::Human, pen(None)).unwrap();
    let fp1 = c.canvas_fingerprint();
    assert_ne!(fp0, fp1, "加笔 = 全画布语义变更");
    // 点序语义：同点集不同序 = 不同笔迹（按存序指纹）。
    let mut c2 = CanvasSession::open(opts(1)).unwrap();
    c2.execute(
        Actor::Human,
        CanvasCommand::AddStroke {
            id: None,
            points: vec![(80.0, 60.0), (40.0, 25.0), (10.0, 20.0)],
            color: "#1a73e8".into(),
            width: 2.5,
            tool: StrokeTool::Pen,
        },
    )
    .unwrap();
    assert_ne!(
        c.canvas_fingerprint(),
        c2.canvas_fingerprint(),
        "逆序点列是不同笔迹（存序指纹，非排序）"
    );
    // 指纹形状（证据锚语义：fnv64 前缀 + 长度稳定）。
    let sid = c.objects()[0].id.clone();
    let fp = c.object_fingerprint(&sid).unwrap();
    assert!(fp.starts_with("fnv64:"));
    assert_eq!(fp.len(), "fnv64:".len() + 16, "FNV-1a 64 = 16 位十六进制");
}
