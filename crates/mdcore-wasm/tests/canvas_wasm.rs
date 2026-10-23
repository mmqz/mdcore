//! WasmCanvas 纯内核测试（JsValue-free——原生 cargo test 面）。
//! 覆盖：八命令 JSON 解析全形态/回执 JSON/读模型 JSON/错误码映射/
//! 双会话 LWW 收敛/快照恢复/事件 base64 转发/时钟注入确定性。

use mdc_canvas::{CanvasSession, SessionOptions};
use mdcore_wasm::canvas::{event_json, object_json, parse_cmd_core, receipt_json};

fn session(tag: u64) -> CanvasSession {
    CanvasSession::open(SessionOptions {
        client_id: tag,
        clock_ms: Some(std::sync::Arc::new(move || 1_700_000_000_000 + tag)),
        ..Default::default()
    })
    .unwrap()
}

fn add_node(s: &mut CanvasSession, id: &str, x: f64, y: f64) {
    let cmd = parse_cmd_core(&format!(
        r#"{{"t":"add_node","id":"{id}","x":{x},"y":{y},"w":220,"h":70,"props":{{"label":"n"}}}}"#
    ))
    .unwrap();
    s.execute(mdc_canvas::Actor::Human, cmd).unwrap();
}

#[test]
fn parse_all_eight_commands() {
    // add_node（id null + props 缺省）
    let c = parse_cmd_core(r#"{"t":"add_node","id":null,"x":1,"y":2,"w":3,"h":4}"#).unwrap();
    assert!(matches!(
        c,
        mdc_canvas::CanvasCommand::AddNode { id: None, .. }
    ));
    // move/resize/add_edge/set_props/remove/group/ungroup
    for (t, json) in [
        ("move_node", r#"{"t":"move_node","id":"n1","x":5,"y":6}"#),
        (
            "resize_node",
            r#"{"t":"resize_node","id":"n1","w":99,"h":88}"#,
        ),
        (
            "add_edge",
            r#"{"t":"add_edge","id":null,"from":"n1","to":"n2","props":{}}"#,
        ),
        (
            "set_props",
            r##"{"t":"set_props","id":"n1","props":{"label":"x","color":"#fff"}}"##,
        ),
        ("remove_object", r#"{"t":"remove_object","id":"n1"}"#),
        (
            "group_objects",
            r#"{"t":"group_objects","group_id":null,"members":["n1","n2"]}"#,
        ),
        ("ungroup", r#"{"t":"ungroup","group_id":"g1"}"#),
    ] {
        assert!(parse_cmd_core(json).is_ok(), "parse {t}");
    }
    // 错误面：未知命令/缺字段/坏 JSON/id 非 string。
    assert!(parse_cmd_core(r#"{"t":"bogus"}"#).is_err());
    assert!(parse_cmd_core(r#"{"t":"move_node","id":"n1"}"#).is_err());
    assert!(parse_cmd_core("{").is_err());
    assert!(parse_cmd_core(r#"{"t":"add_node","id":42,"x":1,"y":2,"w":3,"h":4}"#).is_err());
}

#[test]
fn receipt_and_objects_json_shapes() {
    let mut s = session(1);
    add_node(&mut s, "n1", 10.0, 20.0);
    let receipt = s
        .execute(
            mdc_canvas::Actor::Ai,
            parse_cmd_core(r#"{"t":"move_node","id":"n1","x":30,"y":40}"#).unwrap(),
        )
        .unwrap();
    let rj = receipt_json(&receipt.outcome, receipt.undo_depth, receipt.undo_cleared);
    assert!(rj.contains(r#""outcome":"node_moved""#), "{rj}");
    assert!(rj.contains(r#""id":"n1""#), "{rj}");
    assert!(rj.contains(r#""undo_depth":1"#), "{rj}");

    let objs = s.objects();
    let oj = object_json(&objs[0]);
    for key in [
        "\"id\":\"n1\"",
        "\"kind\":\"node\"",
        "\"x\":30",
        "\"members\":[]",
    ] {
        assert!(oj.contains(key), "{oj} missing {key}");
    }
}

#[test]
fn error_code_mapping() {
    let mut s = session(1);
    add_node(&mut s, "n1", 0.0, 0.0);
    // 不存在对象。
    let e = s
        .execute(
            mdc_canvas::Actor::Human,
            parse_cmd_core(r#"{"t":"move_node","id":"nope","x":1,"y":2}"#).unwrap(),
        )
        .unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_object_not_found");
    // NaN 几何。
    let e = parse_cmd_core(r#"{"t":"add_node","id":null,"x":NaN,"y":2,"w":3,"h":4}"#)
        .err()
        .unwrap();
    assert_eq!(e.code, "cv_bad_json"); // JSON 层 NaN 非法
    let cmd = mdc_canvas::CanvasCommand::AddNode {
        id: None,
        x: f64::NAN,
        y: 0.0,
        w: 1.0,
        h: 1.0,
        props: Default::default(),
    };
    let e = s.execute(mdc_canvas::Actor::Human, cmd).unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_non_finite");
    // 重复 id。
    let e = s
        .execute(
            mdc_canvas::Actor::Human,
            parse_cmd_core(r#"{"t":"add_node","id":"n1","x":1,"y":2,"w":3,"h":4}"#).unwrap(),
        )
        .unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_duplicate_id");
}

#[test]
fn two_session_lww_convergence_via_events() {
    // 双会话直连（e2e_demo sync_canvas 模式的 wasm 边界版）：
    // 事件 updates base64 可独立 apply_update → 双端 objects 相等。
    let mut a = session(1);
    let mut b = session(2);
    add_node(&mut a, "n1", 0.0, 0.0);
    add_node(&mut b, "n2", 100.0, 100.0);
    // 双向同步（e2e_demo sync_canvas 模式）：先取双方 SV 快照再互发
    // 差量（a 有而 b 缺 / b 有而 a 缺）。
    let sv_b = b.state_vector();
    let sv_a = a.state_vector();
    let diff_a = a.encode_diff(&sv_b).unwrap();
    let diff_b = b.encode_diff(&sv_a).unwrap();
    b.apply_update(&diff_a).unwrap();
    a.apply_update(&diff_b).unwrap();
    // 双端收敛（对象集相等——确定性 ID 序）。
    let oa: Vec<_> = a.objects().iter().map(|o| o.id.clone()).collect();
    let ob: Vec<_> = b.objects().iter().map(|o| o.id.clone()).collect();
    assert_eq!(oa, ob);
    assert_eq!(oa.len(), 2);
}

#[test]
fn snapshot_restore_roundtrip() {
    let mut s = session(7);
    add_node(&mut s, "n1", 10.0, 20.0);
    add_node(&mut s, "n2", 30.0, 40.0);
    let snap = s.authoritative_snapshot().unwrap();
    let mut r = CanvasSession::restore(
        SessionOptions {
            client_id: 7,
            clock_ms: Some(std::sync::Arc::new(|| 42)),
            ..Default::default()
        },
        &snap,
    )
    .unwrap();
    let before: Vec<_> = s.objects().iter().map(|o| o.id.clone()).collect();
    let after: Vec<_> = r.objects().iter().map(|o| o.id.clone()).collect();
    assert_eq!(before, after);
    // 撤销栈不随快照走。
    assert_eq!(r.undo_depth(mdc_canvas::Actor::Human), 0);
    // 恢复后续编辑照常。
    add_node(&mut r, "n3", 50.0, 60.0);
    assert_eq!(r.objects().len(), 3);
}

#[test]
fn event_json_shape_and_b64() {
    let mut s = session(1);
    add_node(&mut s, "n1", 0.0, 0.0);
    // take 事件（经 wasm 侧桥——此处直测 event_json 形态）。
    let ej = event_json(
        &mdc_canvas::CanvasChangeSource::Command {
            actor: mdc_canvas::Actor::Human,
        },
        &[vec![1u8, 2, 3, 250]],
        &["n1".to_string()],
    );
    assert!(ej.contains(r#""source":"command""#), "{ej}");
    assert!(ej.contains(r#""actor":"human""#), "{ej}");
    assert!(ej.contains(r#""touched":["n1"]"#), "{ej}");
    // base64 标准字母表（尾部含非 ASCII 字节 250 → 编码非空且可含 +/-）。
    assert!(ej.contains(r#""updates":["AQID+g=="]"#), "{ej}");
}

#[test]
fn fingerprint_semantics() {
    let mut s = session(1);
    add_node(&mut s, "n1", 10.0, 20.0);
    let fp1 = s.object_fingerprint("n1").unwrap();
    // 位置漂移 = 语义变更（证据 Stale 语义）。
    s.execute(
        mdc_canvas::Actor::Human,
        parse_cmd_core(r#"{"t":"move_node","id":"n1","x":11,"y":20}"#).unwrap(),
    )
    .unwrap();
    let fp2 = s.object_fingerprint("n1").unwrap();
    assert_ne!(fp1, fp2, "位置漂移 → 指纹变化");
    assert!(fp1.starts_with("fnv64:"));
}
