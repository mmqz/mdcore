//! N-3（issue #29）`WasmCore::word_count_json` 出口测试（纯内核面——
//! String 返回无 JsValue，原生 cargo test 可直达）。
//!
//! 覆盖：JSON 形状（恰七字段）/ 数值与口径表一致 / 与原生
//! `mdcore::Core::word_count` 序列化逐字段等值（wasm 壳零语义加工）。

use mdcore_wasm::WasmCore;

const SAMPLE_MD: &str = "## 测试标题\n\n正文 hello 世界 42\n\n```text\nlet ok = true;\n```\n\n| 甲 | b |\n| --- | --- |\n| 丙 | d |";

#[test]
fn word_count_json_shape_and_values() {
    let core = WasmCore::new(Some(SAMPLE_MD.to_string()));
    let json = core.word_count_json();
    let v: serde_json::Value = serde_json::from_str(&json).expect("合法 JSON");
    let obj = v.as_object().expect("JSON 对象");
    let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    let mut expected = [
        "cjk_chars",
        "western_words",
        "total_words",
        "total_chars",
        "paragraphs",
        "headings",
        "tables",
    ];
    expected.sort_unstable();
    let mut got = keys.clone();
    got.sort_unstable();
    assert_eq!(got, expected, "恰七字段（口径表字段名）");

    // 口径手算：标题 4 汉字；段落 4 汉字 + hello/42 两词；代码块 3 词
    //（let/ok/true）；表格 甲丙 2 汉字 + b/d 2 词。
    assert_eq!(v["cjk_chars"], serde_json::json!(10));
    assert_eq!(v["western_words"], serde_json::json!(7));
    assert_eq!(v["total_words"], serde_json::json!(17));
    assert_eq!(v["total_chars"], serde_json::json!(30));
    assert_eq!(v["paragraphs"], serde_json::json!(1));
    assert_eq!(v["headings"], serde_json::json!(1));
    assert_eq!(v["tables"], serde_json::json!(1));
}

#[test]
fn word_count_json_matches_native_core() {
    // wasm 壳 = 原生 Core::word_count 的透传序列化（零语义加工钉定）。
    let md = "# 一级标题\n\n中文段落 mixed 42 **加粗** `code`\n\n> 引用段落甲\n\n- 列表项一\n- 列表项二\n\n```python\n# 注释甲\nprint(\"hello\")\n```\n\n| 列一 | 列二 |\n| --- | --- |\n| **甲** | 1 台 |\n\n---\n\n尾部段落 ISO-9001 don't";
    let native = mdcore::Core::from_markdown(md).word_count();
    let wasm = WasmCore::new(Some(md.to_string())).word_count_json();
    let parsed: mdcore::WordCount = serde_json::from_str(&wasm).expect("往返反序列化");
    assert_eq!(native, parsed);
    // 空文档面：新核零计数（{} 兜底路径外的正常序列化）。
    let empty = WasmCore::new(None).word_count_json();
    let parsed: mdcore::WordCount = serde_json::from_str(&empty).expect("空文档 JSON");
    assert_eq!(parsed, mdcore::WordCount::default());
}
