//! office-impl T1 — 消毒五面门禁测试。
//!
//! 结构（office-d §5 逐面对应）：
//! 1. csv_formula——触发集四字符 / 模式三选项 / 审计锚定载荷
//!    （§5.1：四库零消毒、其一主动产 `=` 前缀的对照语义）；
//! 2. xml_control——码位边界全扫（§5.2 charegex 口径）+ 三档对照
//!    断言（损坏档：产物必合法 XML；剥除档：往返零丢失）+ 字面量
//!    消歧回归组（含逃逸边界闭合链）+ LCG 往返性质测试（零三方
//!    依赖的确定性随机）；
//! 3. xml_entities——文本最小集 / 属性五实体 / 一体化；
//! 4. html_escape——href 注入载荷（§5.1 裁决 3 锚定形态）；
//! 5. field_whitelist——白名单七族 + 黑名单族 + 嵌套 token 注入
//!    （含引号粘连形态，§5.3）；
//! 6. crate 卫生——零依赖自锁（纯 std 承诺的机械验证）。

use mdc_office::sanitize::csv_formula::{guard_csv_cell, is_formula_trigger, CsvFormulaMode};
use mdc_office::sanitize::field_whitelist::{FieldGuard, FieldPolicy, FieldRejected};
use mdc_office::sanitize::html_escape::{escape_html_attr, escape_html_text};
use mdc_office::sanitize::xml_control::{
    escape_control_chars, is_escaped_code_point, unescape_control_chars,
};
use mdc_office::sanitize::xml_entities::{
    escape_xml_attr, escape_xml_attr_full, escape_xml_text, escape_xml_text_full,
};

// ---------------------------------------------------------------------------
// 1. CSV 公式首字符（office-d §5.1）
// ---------------------------------------------------------------------------

#[test]
fn csv_trigger_set_exactly_four_chars() {
    for ch in "=+-@".chars() {
        assert!(is_formula_trigger(ch), "触发字符 {ch:?}");
    }
    // 非触发首字符样例（含数字负号语境外的普通文本、字母、空白、引号）
    for ch in "abc 09',\\".chars() {
        assert!(!is_formula_trigger(ch), "非触发字符 {ch:?}");
    }
}

#[test]
fn csv_prefix_modes_rewrite_and_passthrough() {
    let payload = "=HYPERLINK(\"http://evil.example\",\"click me\")";
    assert_eq!(
        guard_csv_cell(payload, CsvFormulaMode::PrefixApostrophe).unwrap(),
        format!("'{payload}")
    );
    assert_eq!(
        guard_csv_cell(payload, CsvFormulaMode::PrefixSpace).unwrap(),
        format!(" {payload}")
    );
    // 正常值原样放行（含数字、负号不在首位的文本）
    for ok in ["plain", "a=b", "1-2", "x@y", "  lead", ""] {
        assert_eq!(
            guard_csv_cell(ok, CsvFormulaMode::PrefixApostrophe).unwrap(),
            ok,
            "放行值 {ok:?}"
        );
    }
}

#[test]
fn csv_reject_mode_typed_error() {
    // 审计锚定载荷族（§5.1：DDE 命令 / 超链接 / 算式改写）
    for payload in [
        "=cmd|' /C calc'!A0",
        "+1+1",
        "-2+3+cmd|' /C calc'!A0",
        "@SUM(A1:A9)*cmd|' /C calc'!A0",
    ] {
        match guard_csv_cell(payload, CsvFormulaMode::Reject) {
            Err(mdc_office::sanitize::csv_formula::CsvGuardError::FormulaPrefixRejected {
                ch,
                cell,
            }) => {
                assert_eq!(ch, payload.chars().next().unwrap());
                assert_eq!(cell, payload);
            }
            other => panic!("拒写模式应 typed reject：{payload:?} → {other:?}"),
        }
    }
    // 放行面：拒写模式下正常值仍通过
    assert_eq!(
        guard_csv_cell("safe value", CsvFormulaMode::Reject).unwrap(),
        "safe value"
    );
}

#[test]
fn csv_all_four_triggers_all_modes_matrix() {
    for ch in "=+-@".chars() {
        let cell = format!("{ch}rest");
        assert_eq!(
            guard_csv_cell(&cell, CsvFormulaMode::PrefixApostrophe).unwrap(),
            format!("'{ch}rest")
        );
        assert!(guard_csv_cell(&cell, CsvFormulaMode::Reject).is_err());
    }
}

// ---------------------------------------------------------------------------
// 2. XML 控制字符保字符档（office-d §5.2）
// ---------------------------------------------------------------------------

#[test]
fn xml_escape_set_boundary_sweep() {
    // 码位边界全扫：转义集精确 = XML 1.0 非法控制字符 + FFFE/FFFF
    for cp in 0u32..0x2000 {
        if let Some(c) = char::from_u32(cp) {
            let in_set = is_escaped_code_point(c);
            let expected = matches!(
                c,
                '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}'
            ) || matches!(c, '\u{FFFE}' | '\u{FFFF}');
            assert_eq!(in_set, expected, "码位 U+{cp:04X} 的转义集归属");
            // 转义行为一致 + 往返零丢失
            let esc = escape_control_chars(&c.to_string());
            if in_set {
                assert_eq!(esc, format!("_x{cp:04X}_"), "U+{cp:04X} 应转义");
            } else {
                assert_eq!(esc, c.to_string(), "U+{cp:04X} 应放行");
            }
            assert_eq!(
                unescape_control_chars(&esc),
                c.to_string(),
                "U+{cp:04X} 往返"
            );
        }
    }
    // 星面字符（含非字符相邻区、CJK、emoji）
    for c in ['中', '😀', 'Ａ', ''] {
        assert!(!is_escaped_code_point(c));
        assert_eq!(escape_control_chars(&c.to_string()), c.to_string());
    }
}

#[test]
fn xml_legal_controls_and_del_pass_through() {
    // \t \n \r 与 0x7F：XML 1.0 合法 → 放行（剥除档对 0x7F 的连带
    // 剥除是登记误伤面，本档不剥——对照断言）
    assert_eq!(
        escape_control_chars("a\tb\nc\rd\u{7F}e"),
        "a\tb\nc\rd\u{7F}e"
    );
}

#[test]
fn xml_corrupt_tier_contrast_output_is_legal() {
    // 损坏档对照断言：转义产物不含任何转义集码位（= 产物必为合法
    // XML——损坏档的故障面在此消除）
    let hostile = "a\u{0}b\u{8}c\u{B}d\u{C}e\u{E}f\u{1F}g\u{FFFE}h\u{FFFF}i";
    let esc = escape_control_chars(hostile);
    assert!(
        esc.chars().all(|c| !is_escaped_code_point(c)),
        "产物必须零残留转义集码位"
    );
    // 剥除档对照断言：往返零丢失（剥除档的静默丢字符面在此消除）
    assert_eq!(unescape_control_chars(&esc), hostile);
}

#[test]
fn xml_literal_pattern_disambiguation_regressions() {
    // 字面量消歧回归组（设计推导链的全部手工用例，含逃逸边界闭合）
    let cases: &[(&str, &str)] = &[
        // 基础形态：下划线前置转义
        ("_x0041_", "_x005F_x0041_"),
        // 小写十六进制字面量同受消歧
        ("_x000b_", "_x005F_x000b_"),
        // 链式形态（闭合位是下一形态的开位）
        ("_x00AB_x00CD_", "_x005F_x00AB_x005F_x00CD_"),
        // 逃逸边界闭合：闭合位是转义集字符（其转义产物前导 _ 闭合假形态）
        ("_x00AB\u{B}", "_x005F_x00AB_x000B_"),
        // 链式 + 逃逸边界
        ("_x00AB_x00CD\u{B}", "_x005F_x00AB_x005F_x00CD_x000B_"),
        // 非完整形态（尾部截断）不消歧
        ("_x00AB", "_x00AB"),
        ("_x00A", "_x00A"),
        ("_x", "_x"),
        // 非十六进制位不消歧
        ("_x00G1_", "_x00G1_"),
        // 大写 X 前缀不是约定形态（约定 = 小写 x）
        ("_X0041_", "_X0041_"),
        // 形态前后正常文本不受波及
        ("pre_x0041_post", "pre_x005F_x0041_post"),
    ];
    for (input, expected) in cases {
        let got = escape_control_chars(input);
        assert_eq!(&got, expected, "编码 {input:?}");
        // 往返零失真（性质对每个回归用例同样成立）
        let back = unescape_control_chars(&got);
        assert_eq!(&back, input, "往返 {input:?}");
    }
}

#[test]
fn xml_escape_boundary_closer_multibyte_members() {
    // 闭合位为多字节转义集成员（U+FFFE/U+FFFF）：其转义产物前导 _
    // 同样闭合假形态 → 必须消歧
    let s = "_x00AB\u{FFFE}";
    let esc = escape_control_chars(s);
    assert_eq!(esc, "_x005F_x00AB_xFFFE_");
    assert_eq!(unescape_control_chars(&esc), s);
    // 对照：闭合位为普通多字节字符（非转义集）→ 无假形态 → 不消歧
    let s2 = "_x00AB中";
    assert_eq!(escape_control_chars(s2), s2);
}

#[test]
fn xml_decode_external_convention_and_edges() {
    // 外部约定产物解码（大小写均受）
    assert_eq!(unescape_control_chars("a_x000B_b"), "a\u{B}b");
    assert_eq!(unescape_control_chars("a_x000b_b"), "a\u{B}b");
    assert_eq!(unescape_control_chars("_xFFFE_"), "\u{FFFE}");
    // 代理区码位：无效标量 → 原样透传
    assert_eq!(unescape_control_chars("_xD800_"), "_xD800_");
    // 非形态文本透传
    assert_eq!(unescape_control_chars("plain_text"), "plain_text");
    // 形态跨字符串尾截断透传
    assert_eq!(unescape_control_chars("_x000B"), "_x000B");
    assert_eq!(unescape_control_chars("尾_x00"), "尾_x00");
}

#[test]
fn xml_roundtrip_property_lcg() {
    // 往返性质测试：确定性 LCG（零三方依赖）在敌意字母表上的任意
    // 输入 encode→decode 零失真。字母表覆盖：形态构件（_ x X 0-9
    // a-f A-F）、转义集成员（单字节 + 多字节）、放行控制字符、
    // 多字节正文、实体字符。
    let alphabet: Vec<char> = "_xX0123456789abcdefABCDEF"
        .chars()
        .chain(['\u{0}', '\u{8}', '\u{B}', '\u{C}', '\u{E}', '\u{1F}'])
        .chain(['\t', '\n', '\r', '\u{7F}'])
        .chain(['\u{FFFE}', '\u{FFFF}'])
        .chain(['中', '😀', '&', '<', '>', '"', '\'', ' '])
        .collect::<Vec<_>>();
    let mut rng = 0x243F6A8885A308D3u64;
    let mut next = move || {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (rng >> 33) as usize
    };
    for case in 0..4000u32 {
        let len = next() % 56;
        let s: String = (0..len)
            .map(|_| alphabet[next() % alphabet.len()])
            .collect();
        let esc = escape_control_chars(&s);
        // 不变量 1：产物零转义集残留（合法 XML 面）
        assert!(
            esc.chars().all(|c| !is_escaped_code_point(c)),
            "case {case} 产物残留转义集码位：{s:?}"
        );
        // 不变量 2：往返零失真
        let back = unescape_control_chars(&esc);
        assert_eq!(back, s, "case {case} 往返失真：{s:?} → {esc:?} → {back:?}");
    }
}

// ---------------------------------------------------------------------------
// 3. XML 实体（office-d §5.2 属性位兜底）
// ---------------------------------------------------------------------------

#[test]
fn xml_entities_text_minimal_and_attr_five() {
    assert_eq!(escape_xml_text("a<b & c>d"), "a&lt;b &amp; c&gt;d");
    assert_eq!(escape_xml_text("]]>"), "]]&gt;");
    assert_eq!(
        escape_xml_attr("q\"v\" & <tag> 'q'"),
        "q&quot;v&quot; &amp; &lt;tag&gt; &apos;q&apos;"
    );
    // 一体化：实体 + 保字符档
    assert_eq!(escape_xml_text_full("a<b\u{B}"), "a&lt;b_x000B_");
    assert_eq!(escape_xml_attr_full("k\u{B}\"v\""), "k_x000B_&quot;v&quot;");
    // 一体化往返（decode 后实体不还原是预期——实体属 XML 层，
    // 控制字符属约定层；控制字符面零丢失）
    assert_eq!(
        unescape_control_chars(&escape_xml_attr_full("k\u{B}\"v\"")),
        "k\u{B}&quot;v&quot;"
    );
}

// ---------------------------------------------------------------------------
// 4. HTML 出口（office-d §5.1 裁决 3 href 面）
// ---------------------------------------------------------------------------

#[test]
fn html_href_injection_neutralized() {
    // 审计锚定形态：链接目标含引号/事件属性直接注入属性位
    let evil_target = "\"><img src=x onerror=alert(1)>";
    let esc = escape_html_attr(evil_target);
    assert!(esc.starts_with("&quot;&gt;"), "引号注入必须消除：{esc}");
    assert!(!esc.contains("\">"), "原样引号闭合不得残留：{esc}");
    assert!(!esc.contains("\""), "属性位零裸引号：{esc}");
    // 单引号定界的调用方面同样安全（五实体全转）
    let esc2 = escape_html_attr("javascript:alert('x')");
    assert_eq!(esc2, "javascript:alert(&#39;x&#39;)");
    // 文本位
    assert_eq!(escape_html_text("<b>a&b</b>"), "&lt;b&gt;a&amp;b&lt;/b&gt;");
}

// ---------------------------------------------------------------------------
// 5. 域指令白名单（office-d §5.3）
// ---------------------------------------------------------------------------

#[test]
fn field_whitelist_default_allows_audit_set() {
    let guard = FieldGuard::default();
    // §5.3 建议白名单七族（裸形 + 带开关/参数形）
    for ok in [
        "DATE",
        "DATE \\@ \"d MMM yyyy\"",
        "PAGE",
        "NUMPAGES",
        "TOC \\o \"1-3\" \\h \\z \\u",
        "PAGEREF _Toc12345 \\h",
        "MERGEFIELD FirstName",
        "SEQ Figure \\* ARABIC",
        "page", // 大小写不敏感
        "date", // 小写域族
    ] {
        assert!(guard.check(ok).is_ok(), "白名单应放行：{ok:?}");
    }
}

#[test]
fn field_deny_families_rejected() {
    let guard = FieldGuard::default();
    // §5.3 黑名单首列 + 安全补全族
    for rejected in [
        "INCLUDETEXT \"c:/evil.docx\"",
        "INCLUDEPICTURE \"http://evil/x.png\"",
        "DDEEXCEL|system!calc",
        "DDEAUTO system cmd",
        "LINK Excel.Sheet \"c:\\x.xlsx\"",
        "RD \"c:/reference.docx\"",
        "includetext \"lowercase\"", // 大小写不敏感
    ] {
        match guard.check(rejected) {
            Err(FieldRejected::DeniedFamily { family, .. }) => {
                assert!(!family.is_empty(), "typed reject 携带族名：{rejected:?}");
            }
            other => panic!("黑名单族应拒绝：{rejected:?} → {other:?}"),
        }
    }
}

#[test]
fn field_embedded_deny_token_injection_blocked() {
    let guard = FieldGuard::default();
    // 嵌套注入形态一：白名单族 + 空格 + 拒绝族
    assert!(matches!(
        guard.check("MERGEFIELD x INCLUDETEXT y"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
    // 形态二：引号粘连（token 前缀匹配）
    assert!(matches!(
        guard.check("MERGEFIELD x INCLUDETEXT\"y\""),
        Err(FieldRejected::DeniedFamily { .. })
    ));
    // 形态三：非首 token 的裸拒绝族
    assert!(matches!(
        guard.check("SEQ a DDE b"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn field_policy_modes_and_unknown_family() {
    // 白名单制：未知域族 typed reject
    let guard = FieldGuard::default();
    assert!(matches!(
        guard.check("TIME \\@ \"HH:mm\""),
        Err(FieldRejected::NotInWhitelist { .. })
    ));
    assert!(matches!(
        guard.check(""),
        Err(FieldRejected::NotInWhitelist { .. })
    ));
    // 黑名单制：非拒绝族放行（TIME 通过）
    let bl = FieldGuard::new(FieldPolicy::Blacklist);
    assert!(bl.check("TIME \\@ \"HH:mm\"").is_ok());
    assert!(bl.check("INCLUDETEXT x").is_err());
    // 全拒制
    let deny_all = FieldGuard::new(FieldPolicy::DenyAll);
    assert!(matches!(
        deny_all.check("DATE"),
        Err(FieldRejected::AllDenied)
    ));
    // 自定义名单
    let custom = FieldGuard::new(FieldPolicy::Whitelist).with_allow(["TIME", "PAGE"]);
    assert!(custom.check("TIME").is_ok());
    assert!(custom.check("DATE").is_err()); // 默认白名单被替换
}

// ---------------------------------------------------------------------------
// 6. crate 卫生：零依赖自锁（纯 std 承诺的机械验证）
// ---------------------------------------------------------------------------

#[test]
fn office_crate_zero_dependencies_self_lock() {
    // mdc-office 纯 std 承诺：清单不得存在任何依赖节（[dependencies]/
    // [dev-dependencies] 不进 Cargo.lock 的发布面——dev 面由本测试
    // 文件自身零 use 外部 crate 佐证）。版本单源由 r24_packaging 的
    // workspace 继承检查覆盖（此处不重复）。
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let src = std::fs::read_to_string(&manifest).expect("读取 office Cargo.toml");
    assert!(
        !src.lines()
            .any(|l| l.trim_start().starts_with("[dependencies]")),
        "mdc-office 必须保持零三方依赖（office-impl PLAN T1 验收项）"
    );
    assert!(
        src.contains("version.workspace = true"),
        "版本必须经 workspace 单源继承"
    );
    // 对 mdcore 零依赖断言（三域纪律）：纯 std 下编译期即不可能，
    // 此断言为清单级双保险（按行首依赖键判定——description 文本中的
    // 纪律陈述不构成依赖引用）。
    assert!(
        !src.lines().any(|l| l.trim_start().starts_with("mdcore")),
        "mdc-office 清单不得以 mdcore 为依赖键（三域纪律：零编译依赖）"
    );
}
