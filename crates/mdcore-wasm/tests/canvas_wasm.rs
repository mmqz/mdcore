//! WasmCanvas 纯内核测试（JsValue-free——原生 cargo test 面）。
//! 覆盖：八命令 JSON 解析全形态/回执 JSON/读模型 JSON/错误码映射/
//! 双会话 LWW 收敛/快照恢复/事件 base64 转发/时钟注入确定性。

use mdc_canvas::{CanvasSession, SessionOptions};
use mdcore_wasm::canvas::{event_json, object_json, parse_cmd_core, parse_cmds_core, receipt_json};

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

// ── 笔迹面（#350——#44 第 1 步：wasm 互译零语义）──────────────
// 注：JSON 含 "# 字符序列（颜色值），原始字符串一律 r##…##（#161 坑1 同源）。

#[test]
fn parse_stroke_commands_full_and_error_shapes() {
    // 全字段（points [[x,y],…] = perfect-freehand 消费形状）。
    let c = parse_cmd_core(
        r##"{"t":"add_stroke","id":"s1","points":[[10,20],[40,25],[80.5,60.25]],"color":"#1a73e8","width":2.5,"tool":"pen"}"##,
    )
    .unwrap();
    match c {
        mdc_canvas::CanvasCommand::AddStroke {
            id,
            points,
            color,
            width,
            tool,
        } => {
            assert_eq!(id.as_deref(), Some("s1"));
            assert_eq!(points, vec![(10.0, 20.0), (40.0, 25.0), (80.5, 60.25)]);
            assert_eq!(color, "#1a73e8");
            assert_eq!(width, 2.5);
            assert_eq!(tool, mdc_canvas::StrokeTool::Pen);
        }
        other => panic!("{other:?}"),
    }
    // set_stroke_style。
    let c = parse_cmd_core(
        r##"{"t":"set_stroke_style","id":"s1","color":"#ff0","width":8,"tool":"highlighter"}"##,
    )
    .unwrap();
    assert!(matches!(
        c,
        mdc_canvas::CanvasCommand::SetStrokeStyle { ref id, tool: mdc_canvas::StrokeTool::Highlighter, .. } if id == "s1"
    ));
    // id 缺省 = 自动分配。
    assert!(matches!(
        parse_cmd_core(r##"{"t":"add_stroke","id":null,"points":[[0,0],[1,1]],"color":"#000","width":1,"tool":"pen"}"##).unwrap(),
        mdc_canvas::CanvasCommand::AddStroke { id: None, .. }
    ));
    // 错误面：坏工具 / 坏点对形状 / 缺字段。
    for bad in [
        r##"{"t":"add_stroke","points":[[0,0],[1,1]],"color":"#000","width":1,"tool":"eraser"}"##,
        r##"{"t":"add_stroke","points":[[0,0,1],[1,1]],"color":"#000","width":1,"tool":"pen"}"##,
        r##"{"t":"add_stroke","points":"flat","color":"#000","width":1,"tool":"pen"}"##,
        r##"{"t":"add_stroke","points":[[0,"y"],[1,1]],"color":"#000","width":1,"tool":"pen"}"##,
        r##"{"t":"add_stroke","points":[[0,0],[1,1]],"width":1,"tool":"pen"}"##,
    ] {
        let e = parse_cmd_core(bad).err().unwrap();
        assert_eq!(e.code, "cv_bad_json", "{bad} → {e:?}");
    }
}

#[test]
fn stroke_objects_json_and_receipts() {
    let mut s = session(1);
    s.execute(
        mdc_canvas::Actor::Human,
        parse_cmd_core(
            r##"{"t":"add_stroke","id":"s1","points":[[10,20],[40,25]],"color":"#1a73e8","width":2.5,"tool":"pen"}"##,
        )
        .unwrap(),
    )
    .unwrap();
    // #44 验收原句：加一笔 → objects_json 含该笔（笔迹四字段全露）。
    let oj = object_json(&s.objects()[0]);
    for key in [
        "\"id\":\"s1\"",
        "\"kind\":\"stroke\"",
        "\"points\":[[10,20],[40,25]]",
        "\"color\":\"#1a73e8\"",
        "\"width\":2.5",
        "\"tool\":\"pen\"",
    ] {
        assert!(oj.contains(key), "{oj} missing {key}");
    }
    // 非 Stroke 对象的笔迹字段缺省形状（空点列/null——同 from/to null 纪律）。
    add_node(&mut s, "n1", 0.0, 0.0);
    for o in s.objects() {
        let oj = object_json(&o);
        if o.kind == mdc_canvas::ObjectKind::Node {
            assert!(oj.contains("\"points\":[]"), "{oj}");
            assert!(oj.contains("\"color\":null"), "{oj}");
            assert!(oj.contains("\"width\":null"), "{oj}");
            assert!(oj.contains("\"tool\":null"), "{oj}");
        }
    }
    // 回执 JSON 形状。
    let r = s
        .execute(
            mdc_canvas::Actor::Ai,
            parse_cmd_core(
                r##"{"t":"set_stroke_style","id":"s1","color":"#ff0000","width":6,"tool":"highlighter"}"##,
            )
            .unwrap(),
        )
        .unwrap();
    let rj = receipt_json(&r.outcome, r.undo_depth, r.undo_cleared);
    assert!(rj.contains(r#""outcome":"stroke_styled""#), "{rj}");
    let r = s
        .execute(
            mdc_canvas::Actor::Human,
            parse_cmd_core(
                r##"{"t":"add_stroke","id":"s2","points":[[0,0],[9,9]],"color":"#000","width":1,"tool":"pen"}"##,
            )
            .unwrap(),
        )
        .unwrap();
    let rj = receipt_json(&r.outcome, r.undo_depth, r.undo_cleared);
    assert!(rj.contains(r#""outcome":"stroke_added""#), "{rj}");
    assert!(rj.contains(r#""id":"s2""#), "{rj}");
}

#[test]
fn stroke_error_codes_reuse_existing_variants() {
    let mut s = session(1);
    // 引擎校验面 → 既有错误码（无新码——错误面封闭）。
    let e = s
        .execute(
            mdc_canvas::Actor::Human,
            parse_cmd_core(r##"{"t":"add_stroke","id":null,"points":[[0,0]],"color":"#000","width":1,"tool":"pen"}"##).unwrap(),
        )
        .unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_invalid_input");
    let e = s
        .execute(
            mdc_canvas::Actor::Human,
            parse_cmd_core(r##"{"t":"add_stroke","id":null,"points":[[0,0],[1,1]],"color":"oops","width":1,"tool":"pen"}"##).unwrap(),
        )
        .unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_invalid_input");
    let e = s
        .execute(
            mdc_canvas::Actor::Human,
            parse_cmd_core(
                r##"{"t":"set_stroke_style","id":"nope","color":"#000","width":1,"tool":"pen"}"##,
            )
            .unwrap(),
        )
        .unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_notfound");
}


// ── 批量面（#352）────────────────────────────────────────────────

#[test]
fn parse_cmds_core_shapes() {
    // 合法批（两命令）。
    let cmds = parse_cmds_core(
        r#"[{"t":"add_node","id":"n1","x":1,"y":2,"w":3,"h":4},{"t":"move_node","id":"n1","x":5,"y":6}]"#,
    )
    .unwrap();
    assert_eq!(cmds.len(), 2);
    assert!(matches!(
        cmds[0],
        mdc_canvas::CanvasCommand::AddNode { ref id, .. } if id.as_deref() == Some("n1")
    ));
    // 空批 / 非数组 / 批内坏命令。
    let e = parse_cmds_core("[]").err().unwrap();
    assert_eq!(e.code, "cv_bad_json");
    let e = parse_cmds_core(r#"{"t":"add_node"}"#).err().unwrap();
    assert_eq!(e.code, "cv_bad_json");
    let e = parse_cmds_core(r#"[{"t":"bogus"}]"#).err().unwrap();
    assert_eq!(e.code, "cv_bad_json");
    let e = parse_cmds_core("not json").err().unwrap();
    assert_eq!(e.code, "cv_bad_json");
}

#[test]
fn execute_batch_end_to_end_and_receipt_json() {
    use mdcore_wasm::canvas::batch_receipt_json;
    let mut s = session(1);
    add_node(&mut s, "a", 0.0, 0.0);
    let receipt = s
        .execute_batch(
            mdc_canvas::Actor::Ai,
            vec![
                mdc_canvas::CanvasCommand::AddNode {
                    id: Some("n1".into()),
                    x: 10.0,
                    y: 10.0,
                    w: 5.0,
                    h: 5.0,
                    props: Default::default(),
                },
                mdc_canvas::CanvasCommand::MoveNode {
                    id: "n1".into(),
                    x: 20.0,
                    y: 20.0,
                },
            ],
        )
        .unwrap();
    let rj = batch_receipt_json(&receipt);
    assert!(
        rj.contains(
            r#""outcomes":[{"outcome":"node_added","id":"n1"},{"outcome":"node_moved","id":"n1"}]"#
        ),
        "{rj}"
    );
    // 撤销栈按 origin 分栈：基线 add_node 是 Human 栈；批是 Ai 栈
    // （整批一单元）→ Ai 深度 = 1。
    assert!(rj.contains(r#""undo_depth":1"#), "{rj}");
    assert!(rj.contains(r#""undo_cleared":false"#), "{rj}");
    // 批效果读回（依赖序列可见）。
    assert_eq!(s.find_object("n1").unwrap().x, 20.0);
    // 一次 undo 全撤。
    assert!(s.undo(mdc_canvas::Actor::Ai));
    assert!(s.find_object("n1").is_none());
    // 失败批：错误码映射 + 零落地。
    let e = s
        .execute_batch(
            mdc_canvas::Actor::Ai,
            vec![
                mdc_canvas::CanvasCommand::AddNode {
                    id: Some("bad1".into()),
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                    props: Default::default(),
                },
                mdc_canvas::CanvasCommand::MoveNode {
                    id: "ghost".into(),
                    x: 0.0,
                    y: 0.0,
                },
            ],
        )
        .unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_object_not_found");


    assert!(s.find_object("bad1").is_none(), "失败批零落地");
    // 空批。
    let e = s.execute_batch(mdc_canvas::Actor::Ai, vec![]).unwrap_err();
    let e: mdcore_wasm::canvas::CvError = e.into();
    assert_eq!(e.code, "cv_invalid_input");
}

/// #402：reorder 命令 JSON 解析（四 op + 错误面）+ z_index 入读模型
/// JSON + 端到端重排经 JSON 命令通道。
#[test]
fn parse_reorder_command_and_z_index_json() {
    // 四 op 全形态
    for (op_json, expect) in [
        ("front", "front"),
        ("back", "back"),
        ("forward", "forward"),
        ("backward", "backward"),
    ] {
        let cmd = parse_cmd_core(&format!(r#"{{"t":"reorder","id":"n1","op":"{op_json}"}}"#))
            .unwrap_or_else(|e| panic!("parse {expect}: {e:?}"));
        assert!(matches!(cmd, mdc_canvas::CanvasCommand::Reorder { .. }));
    }
    // 错误面：坏 op / 缺 op / 缺 id
    assert!(parse_cmd_core(r#"{"t":"reorder","id":"n1","op":"sideways"}"#).is_err());
    assert!(parse_cmd_core(r#"{"t":"reorder","id":"n1"}"#).is_err());
    assert!(parse_cmd_core(r#"{"t":"reorder","op":"front"}"#).is_err());

    // 端到端：两对象 → JSON 通道把顶层 n2 置底 → 回执 + z 序变化 +
    // z_index 字段（n1 底层 z=a0、n2 顶层 z=a1——新建置顶语义）
    let mut s = session(41);
    add_node(&mut s, "n1", 0.0, 0.0);
    add_node(&mut s, "n2", 100.0, 0.0);
    let z2_before = s.find_object("n2").unwrap().z_index.clone();
    assert!(z2_before.as_str() > s.find_object("n1").unwrap().z_index.as_str());
    let cmd = parse_cmd_core(r#"{"t":"reorder","id":"n2","op":"back"}"#).unwrap();
    let receipt = s.execute(mdc_canvas::Actor::Human, cmd).unwrap();
    assert_eq!(
        receipt_json(&receipt.outcome, receipt.undo_depth, receipt.undo_cleared),
        r#"{"outcome":"reordered","id":"n2","undo_depth":3,"undo_cleared":false}"#
    );
    let z2_after = s.find_object("n2").unwrap().z_index.clone();
    let z1 = s.find_object("n1").unwrap().z_index.clone();
    assert!(
        z2_after.as_str() < z1.as_str(),
        "置底后 n2 的 z 降到 n1 之下"
    );

    // 读模型 JSON 含 z_index 字段（字符串）
    let json = object_json(&s.find_object("n2").unwrap());
    assert!(json.contains("\"z_index\":\""), "z_index 应入 JSON：{json}");
}
