//! 画布操作矩阵测试——命令全集语义 + 读模型 + 语义指纹。

use std::collections::BTreeMap;

use mdc_canvas::{Actor, CanvasCommand, CanvasError, CanvasSession, ObjectKind, SessionOptions};

fn opts(id: u64) -> SessionOptions {
    SessionOptions {
        client_id: id,
        ..Default::default()
    }
}

fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn node_lifecycle_and_read_model() {
    let mut c = CanvasSession::open(opts(1)).unwrap();
    // 自动 ID + 显式 ID。
    let n1 = c
        .execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some("n-1".into()),
                x: 10.0,
                y: 20.0,
                w: 120.0,
                h: 60.0,
                props: props(&[("label", "设备报价")]),
            },
        )
        .unwrap();
    assert_eq!(
        match n1.outcome {
            mdc_canvas::CommandOutcome::NodeAdded { ref id } => id.clone(),
            other => panic!("期望 NodeAdded，得到 {other:?}"),
        },
        "n-1"
    );
    let n2 = c
        .execute(
            Actor::Ai,
            CanvasCommand::AddNode {
                id: None,
                x: 200.0,
                y: 40.0,
                w: 100.0,
                h: 50.0,
                props: BTreeMap::new(),
            },
        )
        .unwrap();
    let n2_id = match n2.outcome {
        mdc_canvas::CommandOutcome::NodeAdded { id } => id,
        other => panic!("{other:?}"),
    };
    assert!(
        n2_id.starts_with("1-"),
        "自动 ID = {{client}}-{{seq}}：{n2_id}"
    );

    // 移动/缩放（键级 LWW 写）。
    c.execute(
        Actor::Human,
        CanvasCommand::MoveNode {
            id: "n-1".into(),
            x: 30.0,
            y: 40.0,
        },
    )
    .unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::ResizeNode {
            id: "n-1".into(),
            w: 200.0,
            h: 80.0,
        },
    )
    .unwrap();
    let o = c.find_object("n-1").unwrap();
    assert_eq!((o.x, o.y, o.w, o.h), (30.0, 40.0, 200.0, 80.0));
    assert_eq!(o.kind, ObjectKind::Node);
    assert_eq!(o.props.get("label").map(|s| s.as_str()), Some("设备报价"));

    // 属性终态替换。
    c.execute(
        Actor::Ai,
        CanvasCommand::SetProps {
            id: "n-1".into(),
            props: props(&[("label", "设备报价（修订）"), ("color", "#0af")]),
        },
    )
    .unwrap();
    let o = c.find_object("n-1").unwrap();
    assert_eq!(o.props.len(), 2, "终态替换（旧键清除）");

    // 读模型：确定性序（按 ID 字典序）。
    let ids: Vec<String> = c.objects().iter().map(|o| o.id.clone()).collect();
    let mut want = vec!["n-1".to_string(), n2_id.clone()];
    want.sort();
    assert_eq!(ids, want);

    // 删除。
    c.execute(
        Actor::Human,
        CanvasCommand::RemoveObject { id: n2_id.clone() },
    )
    .unwrap();
    assert!(c.find_object(&n2_id).is_none());
    assert_eq!(c.objects().len(), 1);
}

#[test]
fn edge_and_group_semantics() {
    let mut c = CanvasSession::open(opts(2)).unwrap();
    for (id, x) in [("a", 0.0), ("b", 100.0), ("g", 500.0)] {
        c.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some(id.into()),
                x,
                y: 0.0,
                w: 50.0,
                h: 50.0,
                props: BTreeMap::new(),
            },
        )
        .unwrap();
    }
    // 合法边。
    c.execute(
        Actor::Human,
        CanvasCommand::AddEdge {
            id: Some("e-1".into()),
            from: "a".into(),
            to: "b".into(),
            props: props(&[("label", "依赖")]),
        },
    )
    .unwrap();
    let e = c.find_object("e-1").unwrap();
    assert_eq!(e.kind, ObjectKind::Edge);
    assert_eq!(e.from.as_deref(), Some("a"));
    assert_eq!(e.to.as_deref(), Some("b"));
    // 端点不存在 → 拒。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddEdge {
                id: None,
                from: "a".into(),
                to: "ghost".into(),
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::EndpointNotFound { .. })
    ));
    // 端点非节点 → 拒。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddEdge {
                id: None,
                from: "a".into(),
                to: "e-1".into(),
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::InvalidInput(_))
    ));
    // 自环 → 拒。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddEdge {
                id: None,
                from: "a".into(),
                to: "a".into(),
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::InvalidInput(_))
    ));
    // 分组：成员必须存在。
    c.execute(
        Actor::Human,
        CanvasCommand::GroupObjects {
            group_id: Some("grp".into()),
            members: vec!["a".into(), "b".into()],
        },
    )
    .unwrap();
    let g = c.find_object("grp").unwrap();
    assert_eq!(g.kind, ObjectKind::Group);
    assert_eq!(g.members, vec!["a".to_string(), "b".to_string()]);
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::GroupObjects {
                group_id: None,
                members: vec!["ghost".into()],
            },
        ),
        Err(CanvasError::MemberNotFound { .. })
    ));
    // 解组：组删、成员留。
    c.execute(
        Actor::Human,
        CanvasCommand::Ungroup {
            group_id: "grp".into(),
        },
    )
    .unwrap();
    assert!(c.find_object("grp").is_none());
    assert!(c.find_object("a").is_some());
    assert!(c.find_object("b").is_some());
    // 非组解组 → 拒。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::Ungroup {
                group_id: "a".into()
            }
        ),
        Err(CanvasError::KindMismatch { .. })
    ));
    // 几何命令对非节点 → 拒。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "e-1".into(),
                x: 0.0,
                y: 0.0
            },
        ),
        Err(CanvasError::KindMismatch { .. })
    ));
}

#[test]
fn semantic_fingerprints() {
    let mut c = CanvasSession::open(opts(3)).unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-1".into()),
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
            props: props(&[("label", "报价")]),
        },
    )
    .unwrap();
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("n-2".into()),
            x: 300.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    let fp1 = c.object_fingerprint("n-1").unwrap();
    let canvas_fp1 = c.canvas_fingerprint();
    // 位置漂移 = 语义变更（画布域证据粒度决策——Locator::CanvasObject
    // 文档登记）。
    c.execute(
        Actor::Human,
        CanvasCommand::MoveNode {
            id: "n-1".into(),
            x: 99.0,
            y: 20.0,
        },
    )
    .unwrap();
    assert_ne!(c.object_fingerprint("n-1").unwrap(), fp1, "移动即语义变更");
    assert_ne!(c.canvas_fingerprint(), canvas_fp1);
    // 无关对象变更不改变 n-2 指纹。
    let fp2 = c.object_fingerprint("n-2").unwrap();
    c.execute(
        Actor::Ai,
        CanvasCommand::MoveNode {
            id: "n-1".into(),
            x: 0.0,
            y: 0.0,
        },
    )
    .unwrap();
    assert_eq!(c.object_fingerprint("n-2").unwrap(), fp2, "对象级指纹隔离");
    // 属性变更 = 语义变更。
    let fp1b = c.object_fingerprint("n-1").unwrap();
    c.execute(
        Actor::Ai,
        CanvasCommand::SetProps {
            id: "n-1".into(),
            props: props(&[("label", "报价（改）")]),
        },
    )
    .unwrap();
    assert_ne!(c.object_fingerprint("n-1").unwrap(), fp1b);
    // 删除后指纹查询 → 显式错误。
    c.execute(
        Actor::Human,
        CanvasCommand::RemoveObject { id: "n-1".into() },
    )
    .unwrap();
    assert!(matches!(
        c.object_fingerprint("n-1"),
        Err(CanvasError::ObjectNotFound { .. })
    ));
}

#[test]
fn hostile_inputs() {
    let mut c = CanvasSession::open(opts(4)).unwrap();
    // NaN/inf 几何。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: None,
                x: f64::NAN,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::NonFiniteGeometry { .. })
    ));
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: None,
                x: 0.0,
                y: 0.0,
                w: f64::INFINITY,
                h: 1.0,
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::NonFiniteGeometry { .. })
    ));
    // 负尺寸。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: None,
                x: 0.0,
                y: 0.0,
                w: -1.0,
                h: 1.0,
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::InvalidInput(_))
    ));
    // 空显式 ID。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some(String::new()),
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::InvalidInput(_))
    ));
    // 撞号。
    c.execute(
        Actor::Human,
        CanvasCommand::AddNode {
            id: Some("dup".into()),
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            props: BTreeMap::new(),
        },
    )
    .unwrap();
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::AddNode {
                id: Some("dup".into()),
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                props: BTreeMap::new(),
            },
        ),
        Err(CanvasError::DuplicateId { .. })
    ));
    // 不存在对象的全操作面。
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::MoveNode {
                id: "ghost".into(),
                x: 0.0,
                y: 0.0
            },
        ),
        Err(CanvasError::ObjectNotFound { .. })
    ));
    assert!(matches!(
        c.execute(
            Actor::Human,
            CanvasCommand::RemoveObject { id: "ghost".into() }
        ),
        Err(CanvasError::ObjectNotFound { .. })
    ));
    // client_id 域。
    assert!(matches!(
        CanvasSession::open(SessionOptions {
            client_id: 1 << 53,
            ..Default::default()
        }),
        Err(CanvasError::InvalidInput(_))
    ));
    // 字节限额。
    let mut small = CanvasSession::open(SessionOptions {
        client_id: 5,
        input_byte_limit: 128,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        small.execute(
            Actor::Ai,
            CanvasCommand::AddNode {
                id: None,
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                props: props(&[("k", &"v".repeat(300))]),
            },
        ),
        Err(CanvasError::Limit { .. })
    ));
    // 会话零污染。
    assert_eq!(small.objects().len(), 0);
}
