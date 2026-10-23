//! 证据工作台 wasm 出口测试（纯内核面——JsValue-free）。
//! 聚焦：边界 JSON 互译（提议解析/决策/策略）+ 序列化形态 +
//! 错误码映射 + wasm 壳等价的八步 JSON 全链（JS 调用序列模拟）。

use mdc_evidence::{LinkageStatus, ReviewStatus};
use mdc_workbench::Workbench;
use mdcore_wasm::workbench::{
    paragraphs_json, parse_binding_core, parse_calc_core, parse_decision_core,
    parse_review_policy_core, WbError,
};

fn sample_xlsx_bytes() -> Vec<u8> {
    let mut wb = mdc_office_io::write::WorkbookWriter::new();
    {
        let mut sh = wb.add_sheet("Sheet1").unwrap();
        use mdc_office_io::write::CellValue;
        sh.write(6, 1, CellValue::Number(12.5), None).unwrap(); // B7
        sh.write(6, 2, CellValue::Number(336.8), None).unwrap(); // C7
    }
    wb.save_to_vec().unwrap()
}

fn sample_docx_bytes() -> Vec<u8> {
    let mut w = mdc_office_io::docx::DocxWriter::new();
    w.heading(1, "Report").unwrap();
    w.paragraph_text("Intro.");
    w.paragraph_text("Unit price 336.8 and qty 12.5.");
    w.paragraph_text("Subtotal registered 4210.000.");
    w.save_to_vec().unwrap()
}

const BINDING_JSON: &str = r#"{"evidence_id":"e-1","claim":"Unit price 336.8","claimed_value":"336.8","paragraph_index":2,"source_doc_id":"src-1","sheet":"Sheet1","coord":"C7","relation":"direct_quote","calc_ref":null,"generation_batch":"g1"}"#;

const CALC_JSON: &str = r#"{"calc_id":"calc-1","expression":"qty * price","unit":null,"precision":{"target_scale":3,"rounding":"half_up"},"inputs":[{"name":"qty","source":{"cell":{"doc_id":"src-1","sheet":"Sheet1","coord":"B7"}},"unit":null},{"name":"price","source":{"constant":"336.8"},"unit":null}]}"#;

#[test]
fn parse_binding_all_faces() {
    let p = parse_binding_core(BINDING_JSON).unwrap();
    assert_eq!(p.evidence_id, "e-1");
    assert_eq!(p.claimed_value.as_ref().unwrap().to_string(), "336.8");
    assert_eq!(p.paragraph_index, 2);
    assert!(matches!(
        p.relation,
        mdc_evidence::RelationKind::DirectQuote
    ));
    assert!(p.calc_ref.is_none());
    // relation 四枚 + 错误面。
    for (r, ok) in [
        ("direct_quote", true),
        ("calculated", true),
        ("synthesized", true),
        ("human_attested", true),
        ("bogus", false),
    ] {
        let j = BINDING_JSON.replace("direct_quote", r);
        assert_eq!(parse_binding_core(&j).is_ok(), ok, "relation {r}");
    }
    // claimed_value null → None。
    let j = BINDING_JSON.replace("\"336.8\"", "null");
    assert!(parse_binding_core(&j).unwrap().claimed_value.is_none());
    // 非 JSON / 缺字段。
    assert!(parse_binding_core("{").is_err());
    let j = BINDING_JSON.replace("\"paragraph_index\":2", "");
    assert!(parse_binding_core(&j).is_err());
}

#[test]
fn parse_calc_cell_and_constant() {
    let p = parse_calc_core(CALC_JSON).unwrap();
    assert_eq!(p.calc_id, "calc-1");
    assert_eq!(p.inputs.len(), 2);
    assert!(matches!(
        p.inputs[0].source,
        mdc_workbench::InputSource::Cell { .. }
    ));
    assert!(matches!(
        p.inputs[1].source,
        mdc_workbench::InputSource::Constant { .. }
    ));
    // rounding 三枚 + 默认 half_up。
    for (r, ok) in [
        ("half_up", true),
        ("half_even", true),
        ("truncate", true),
        ("x", false),
    ] {
        let j = CALC_JSON.replace("half_up", r);
        assert_eq!(parse_calc_core(&j).is_ok(), ok, "rounding {r}");
    }
    assert!(parse_calc_core("{}").is_err());
}

#[test]
fn parse_decision_and_policy() {
    assert!(matches!(
        parse_decision_core("approved").unwrap(),
        ReviewStatus::Approved
    ));
    assert!(parse_decision_core("nope").is_err());
    let p = parse_review_policy_core("{}").unwrap();
    assert_eq!(p.reviewer_prefixes, vec!["human:".to_string()]);
    assert!(!p.allow_approval_of_broken_linkage);
    let p2 = parse_review_policy_core(
        r#"{"reviewer_prefixes":["op:"],"allow_approval_of_broken_linkage":true}"#,
    )
    .unwrap();
    assert_eq!(p2.reviewer_prefixes, vec!["op:".to_string()]);
    assert!(p2.allow_approval_of_broken_linkage);
}

#[test]
fn wasm_face_eight_step_json_flow() {
    // JS 调用序列模拟：全部经 wasm 边界纯内核路径（parse_* + 内存会话）。
    let mut t = 1000u64;
    let mut tick = || {
        t += 100;
        t
    };
    let body = sample_docx_bytes();
    let mut wb = Workbench::start_memory(&body, "body-web", tick(), "human:setup").unwrap();
    wb.ingest_source_bytes("src-1", &sample_xlsx_bytes(), tick(), "ai:web")
        .unwrap();
    wb.propose_binding(parse_binding_core(BINDING_JSON).unwrap(), tick(), "ai:web")
        .unwrap();
    let v = wb
        .propose_calculation(parse_calc_core(CALC_JSON).unwrap(), tick(), "ai:web")
        .unwrap();
    assert_eq!(v.to_string(), "4210.000");

    // 序列化面：evidence serde 原生形。
    let ev_json = serde_json::to_string(&wb.sidecar().evidence).unwrap();
    assert!(ev_json.contains("\"evidence_id\":\"e-1\""));
    assert!(ev_json.contains("\"linkage\":\"valid\""));
    assert!(ev_json.contains("\"review\":\"pending\""));

    // 段落 JSON（跳转数据源）。
    let paras = wb.current_paragraphs().unwrap();
    let pj = paragraphs_json(&paras);
    assert!(pj.contains("\"index\":2"));
    assert!(pj.contains("336.8"));

    // 审核（human 面）→ export 字节。
    wb.review("e-1", ReviewStatus::Approved, tick(), "human:web")
        .unwrap();
    let bytes = wb.export_clean_bytes(tick(), "human:web").unwrap();
    assert_eq!(bytes, body);

    // sidecar JSON 往返（restore 全链）。
    let json = wb.sidecar_json().unwrap();
    let mut reopened = Workbench::open_memory(&json, &body, &[]).unwrap();
    reopened
        .attach_source_bytes("src-1", &sample_xlsx_bytes())
        .unwrap();
    let ev = &reopened.sidecar().evidence[0];
    assert!(matches!(ev.review, ReviewStatus::Approved));
    assert!(matches!(ev.linkage, LinkageStatus::Valid));
}

#[test]
fn error_code_mapping_faces() {
    // 抽查错误码映射（WorkbenchError → wb_*）。
    let e: WbError = mdc_workbench::WorkbenchError::BodyStale.into();
    assert_eq!(e.code, "wb_body_stale");
    let e: WbError =
        mdc_workbench::WorkbenchError::SourceNotRegistered { doc_id: "x".into() }.into();
    assert_eq!(e.code, "wb_source_not_registered");
    // 垃圾正文 → wb_docx_read。
    let e: WbError = match Workbench::start_memory(b"junk", "b", 1, "human:setup") {
        Err(e) => e.into(),
        Ok(_) => panic!("expected error"),
    };
    assert_eq!(e.code, "wb_docx_read");
    // 损坏 sidecar → wb_sidecar。
    let e: WbError = match Workbench::open_memory("{", b"b", &[]) {
        Err(e) => e.into(),
        Ok(_) => panic!("expected error"),
    };
    assert_eq!(e.code, "wb_sidecar");
}
