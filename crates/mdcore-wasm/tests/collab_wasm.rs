//! WasmCollabSession 纯内核测试（JsValue-free——原生 cargo test 面）。
//! 覆盖：八命令 JSON 解析全形态/回执 JSON/读模型 JSON/错误码映射/
//! 双会话并发收敛（跨 actor）/撤销重做 origin 分栈/删除复原身份保持/
//! 快照恢复/事件 base64 转发/时钟注入确定性/敌意命令面。

use mdc_collab::{Actor, CollabSession, SessionOptions};
use mdcore_wasm::collab::{block_json, event_json, parse_cmd_core, receipt_json};

fn session(client_id: u64) -> CollabSession {
    CollabSession::open(SessionOptions {
        client_id,
        clock_ms: Some(std::sync::Arc::new(move || 1_700_000_000_000 + client_id)),
        ..Default::default()
    })
    .unwrap()
}

fn load(s: &mut CollabSession, actor: Actor, text: &str) -> Vec<String> {
    let cmd = parse_cmd_core(&format!(
        r#"{{"t":"load_blocks","blocks":[{{"kind":"p","attrs":{{}},"text":"{text}"}}]}}"#
    ))
    .unwrap();
    let r = s.execute(actor, cmd).unwrap();
    // 回执 JSON 可解析且 outcome 正确
    let j: serde_json::Value =
        serde_json::from_str(&receipt_json(&r.outcome, r.undo_depth, r.undo_cleared)).unwrap();
    assert_eq!(j["outcome"], "blocks_loaded");
    j["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn parse_all_eight_commands() {
    // load_blocks（attrs 缺省 + text 缺省）
    let c = parse_cmd_core(r#"{"t":"load_blocks","blocks":[{"kind":"p"}]}"#).unwrap();
    assert!(matches!(
        c,
        mdc_collab::CollabCommand::LoadBlocks { ref blocks } if blocks.len() == 1
    ));
    for (t, json) in [
        (
            "insert_block",
            r#"{"t":"insert_block","parent":null,"index":0,"spec":{"kind":"h","attrs":{"level":"2"},"text":"T"},"id":null}"#,
        ),
        ("remove_block", r#"{"t":"remove_block","block_id":"1-1"}"#),
        (
            "replace_text",
            r#"{"t":"replace_text","block_id":"1-1","start":0,"end":2,"insert":"xy"}"#,
        ),
        (
            "set_block_type",
            r#"{"t":"set_block_type","block_id":"1-1","kind":"li"}"#,
        ),
        (
            "set_attrs",
            r#"{"t":"set_attrs","block_id":"1-1","attrs":{"lang":"rs"}}"#,
        ),
        (
            "set_mark",
            r#"{"t":"set_mark","block_id":"1-1","start":0,"end":1,"mark":"bold","add":true}"#,
        ),
        (
            "move_block",
            r#"{"t":"move_block","block_id":"1-1","to_parent":null,"to_index":0}"#,
        ),
    ] {
        assert!(parse_cmd_core(json).is_ok(), "parse {t}");
    }
    // 错误面：未知命令/缺字段/坏 JSON/类型错。
    assert!(parse_cmd_core(r#"{"t":"bogus"}"#).is_err());
    assert!(parse_cmd_core(r#"{"t":"replace_text","block_id":"1-1"}"#).is_err());
    assert!(parse_cmd_core("{").is_err());
    assert!(parse_cmd_core(r#"{"t":"load_blocks","blocks":[{"kind":"zz"}]}"#).is_err());
    assert!(
        parse_cmd_core(r#"{"t":"replace_text","block_id":1,"start":0,"end":0,"insert":""}"#)
            .is_err()
    );
}

#[test]
fn receipt_and_document_json_shapes() {
    let mut s = session(7_100_101);
    let ids = load(&mut s, Actor::Human, "hello");
    assert_eq!(ids.len(), 1);
    // document_json：块树 JSON（kind/attrs/text/children 递归面）
    let doc: serde_json::Value = serde_json::from_str(&block_json(&s.document()[0])).unwrap();
    assert_eq!(doc["kind"], "p");
    assert_eq!(doc["text"], "hello");
    assert_eq!(doc["block_id"], ids[0]);
    // replace_text 回执：outcome + new_len + undo_depth
    let cmd = parse_cmd_core(&format!(
        r#"{{"t":"replace_text","block_id":"{}","start":0,"end":5,"insert":"world"}}"#,
        ids[0]
    ))
    .unwrap();
    let r = s.execute(Actor::Ai, cmd).unwrap();
    let j: serde_json::Value =
        serde_json::from_str(&receipt_json(&r.outcome, r.undo_depth, r.undo_cleared)).unwrap();
    assert_eq!(j["outcome"], "text_replaced");
    assert_eq!(j["new_len"], 5);
    assert_eq!(j["undo_depth"], 1);
    assert_eq!(j["undo_cleared"], false);
    // 顶层 ID 面
    let tops: serde_json::Value = serde_json::from_str(&format!(
        "[{}]",
        s.top_level_ids()
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(",")
    ))
    .unwrap();
    assert_eq!(tops.as_array().unwrap().len(), 1);
}

#[test]
fn two_session_concurrent_convergence_and_undo_locality() {
    let mut a = session(7_100_201);
    let mut b = session(7_100_202);
    let _a_ids = load(&mut a, Actor::Human, "A-text");
    let _b_ids = load(&mut b, Actor::Ai, "B-text");

    // 全量交换（模拟 wire 传输——事件 updates 面的 encode_diff 等价路径）
    let a_sv = a.state_vector();
    let b_sv = b.state_vector();
    a.apply_update(&b.encode_diff(&a_sv).unwrap()).unwrap();
    b.apply_update(&a.encode_diff(&b_sv).unwrap()).unwrap();

    // 收敛：双端块数一致且都含对方文本
    let da = a.document();
    let db = b.document();
    assert_eq!(da.len(), db.len());
    let texts_a: Vec<&str> = da.iter().map(|b| b.text.as_str()).collect();
    let texts_b: Vec<&str> = db.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts_a, texts_b);
    assert!(texts_a.contains(&"A-text") && texts_a.contains(&"B-text"));

    // 撤销本地性：A 撤销自己的 load（human 面），不影响 B 的块
    assert!(a.undo(Actor::Human));
    let da2 = a.document();
    assert_eq!(da2.len(), 1); // 只剩 B-text
    assert_eq!(da2[0].text, "B-text");
    // 撤销字节传播到 B：A 的撤销产出经事件面转发
    let b_sv2 = b.state_vector();
    b.apply_update(&a.encode_diff(&b_sv2).unwrap()).unwrap();
    assert_eq!(b.document().len(), 1);
}

#[test]
fn delete_then_undo_restores_identity() {
    let mut s = session(7_100_301);
    let ids = load(&mut s, Actor::Human, "keep");
    let id = ids[0].clone();

    let cmd = parse_cmd_core(&format!(r#"{{"t":"remove_block","block_id":"{id}"}}"#)).unwrap();
    let r = s.execute(Actor::Human, cmd).unwrap();
    let j: serde_json::Value =
        serde_json::from_str(&receipt_json(&r.outcome, r.undo_depth, r.undo_cleared)).unwrap();
    assert_eq!(j["outcome"], "block_removed");
    assert!(s.document().is_empty());

    assert!(s.undo(Actor::Human));
    let doc = s.document();
    assert_eq!(doc.len(), 1);
    // 身份保持：同一 block_id（证据锚点语义——删除复原不换 ID）
    assert_eq!(doc[0].block_id, id);
    assert_eq!(doc[0].text, "keep");
    assert!(s.redo(Actor::Human));
    assert!(s.document().is_empty());
}

#[test]
fn snapshot_restore_and_event_json() {
    let mut s = session(7_100_401);
    let _ = load(&mut s, Actor::Human, "snap");
    let snap = s.authoritative_snapshot().unwrap();

    // 快照恢复（撤销栈不随快照走——B3.1）
    let r = session(7_100_402);
    r.apply_update(&snap).unwrap();
    let doc = r.document();
    assert_eq!(doc.len(), 1);
    assert_eq!(doc[0].text, "snap");

    // 事件 JSON：命令面 → source/actor/updates(base64)
    let _ = load(&mut s, Actor::Ai, "more");
    // （take_events_json 在 wasm 句柄内；原生面直接验 event_json 形状）
    let ej: serde_json::Value = serde_json::from_str(&event_json(
        &mdc_collab::ChangeSource::Command { actor: Actor::Ai },
        &[vec![1, 2, 3]],
    ))
    .unwrap();
    assert_eq!(ej["source"], "command");
    assert_eq!(ej["actor"], "ai");
    assert_eq!(ej["updates"][0], "AQID"); // b64(1,2,3)
    let er: serde_json::Value =
        serde_json::from_str(&event_json(&mdc_collab::ChangeSource::Remote, &[])).unwrap();
    assert_eq!(er["source"], "remote");
    assert!(er.get("actor").is_none());
}

#[test]
fn hostile_and_typed_error_faces() {
    let mut s = session(7_100_501);
    // 目标不存在 → BlockNotFound（typed 码在 wasm 边界映射；内核面直接验变体）
    let cmd = parse_cmd_core(r#"{"t":"remove_block","block_id":"nope"}"#).unwrap();
    assert!(matches!(
        s.execute(Actor::Human, cmd),
        Err(mdc_collab::CollabError::BlockNotFound { .. })
    ));
    // 区间越界
    let ids = load(&mut s, Actor::Human, "abc");
    let cmd = parse_cmd_core(&format!(
        r#"{{"t":"replace_text","block_id":"{}","start":0,"end":99,"insert":"x"}}"#,
        ids[0]
    ))
    .unwrap();
    assert!(matches!(
        s.execute(Actor::Human, cmd),
        Err(mdc_collab::CollabError::BadRange { .. })
    ));
    // 敌意 wire：垃圾字节 fail-closed
    assert!(s.apply_update(&[0xff, 0xff, 0xff]).is_err());
    // 零污染：失败命令/wire 不改变读模型
    let doc = s.document();
    assert_eq!(doc.len(), 1);
    assert_eq!(doc[0].text, "abc");
    // 保留属性键拒收
    let cmd = parse_cmd_core(&format!(
        r#"{{"t":"set_attrs","block_id":"{}","attrs":{{"block_id":"evil"}}}}"#,
        ids[0]
    ))
    .unwrap();
    assert!(matches!(
        s.execute(Actor::Human, cmd),
        Err(mdc_collab::CollabError::ReservedAttrKey { .. })
    ));
}

#[test]
fn clock_injection_deterministic_and_undo_limit() {
    // 注入时钟路径：默认 None（原生系统时钟）与 Some（注入）都能开
    let s = CollabSession::open(SessionOptions::default()).unwrap();
    drop(s);
    // 软上限路径：undo_stack_limit=1 → 第二命令触发 clear_all 显式回执
    let mut s = CollabSession::open(SessionOptions {
        client_id: 7_100_601,
        undo_stack_limit: Some(1),
        clock_ms: Some(std::sync::Arc::new(|| 42)),
        ..Default::default()
    })
    .unwrap();
    let _ = load(&mut s, Actor::Human, "one");
    let r = s
        .execute(
            Actor::Human,
            parse_cmd_core(r#"{"t":"load_blocks","blocks":[{"kind":"p","text":"two"}]}"#).unwrap(),
        )
        .unwrap();
    let j: serde_json::Value =
        serde_json::from_str(&receipt_json(&r.outcome, r.undo_depth, r.undo_cleared)).unwrap();
    assert_eq!(j["undo_cleared"], true);
}

// receipt_json 独立形状锁（pub 面稳定字段）。
#[test]
fn receipt_json_shape_lock() {
    use mdc_collab::CommandOutcome;
    let r = receipt_json(
        &CommandOutcome::BlocksLoaded {
            ids: vec!["7-1".into()],
        },
        2,
        false,
    );
    let j: serde_json::Value = serde_json::from_str(&r).unwrap();
    assert_eq!(j["outcome"], "blocks_loaded");
    assert_eq!(j["ids"][0], "7-1");
    assert_eq!(j["undo_depth"], 2);
}
