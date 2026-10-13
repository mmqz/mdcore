//! mdc-office 消毒五面针对性安全测试（xlsx-r1 t10-e §1-§5 落地，
//! 阶段 B 2026-10-13）。
//!
//! 与既有 `sanitize_gates.rs`（23 项性质/对照测试）的分界：性质测试
//! 回答「任意输入下不变量是否成立」（防回归面）；本文件 = **针对性
//! 安全测试**——逐攻击向量精确输入 → 精确期望输出（防绕过面）。
//! 两者不可互替。
//!
//! 期望值口径：SANITIZE_TESTS.md 设计表逐条 `assert_eq!`（非包含/
//! 不包含弱断言）；**API 形状以 mdcore 实际为准**（runbook §3.4
//! 纪律）——三处设计-实现出入已按实测钉定并在设计文档回写（详见
//! 各用例注记与 xlsx-r1 REPORT 阶段 B 记录）：
//! - X-03 逐级消歧精确串（设计表首版拼接笔误，实测
//!   `_x005F_x005F_x005F_x0000_`）；
//! - F-06 引号包裹黑名单 token（实测放行——Word 域语法中引号内为
//!   字符串字面量，无执行语义；裸 token 与后缀粘连两形态均拦截）；
//! - F-10 前缀超集 token（实测拦截——`starts_with` 前缀扫描是
//!   后缀粘连捕获的设计代价，与 F-09 同类保守误杀）。
//!
//! 敌意样本全部净室自构造（零外部恶意样本引入）。

use mdc_office::sanitize::csv_formula::{guard_csv_cell, CsvFormulaMode, CsvGuardError};
use mdc_office::sanitize::field_whitelist::{FieldGuard, FieldPolicy, FieldRejected};
use mdc_office::sanitize::html_escape::{escape_html_attr, escape_html_text};
use mdc_office::sanitize::xml_control::{escape_control_chars, unescape_control_chars};
use mdc_office::sanitize::xml_entities::{escape_xml_attr, escape_xml_attr_full, escape_xml_text};

// ---------------------------------------------------------------------------
// 面 1：csv_formula_guard（C-01..C-10 + 模式对照）
// ---------------------------------------------------------------------------

#[test]
fn c01_dde_injection_prefixed() {
    // 经典 DDE 注入，默认模式（前置 '）
    assert_eq!(
        guard_csv_cell("=cmd|'/c calc'!A1", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "'=cmd|'/c calc'!A1"
    );
}

#[test]
fn c02_plus_trigger_prefixed() {
    // `+` 触发集成员
    assert_eq!(
        guard_csv_cell("+cmd|'/c calc'!A1", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "'+cmd|'/c calc'!A1"
    );
}

#[test]
fn c03_minus_trigger_prefixed() {
    // `-` 触发集成员（负数陷阱：-2 是合法数值形态但 Excel 仍按公式
    // 解释——必须消毒，业界惯例一致）
    assert_eq!(
        guard_csv_cell("-2+3+cmd|'...'", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "'-2+3+cmd|'...'"
    );
}

#[test]
fn c04_at_trigger_prefixed() {
    // `@` 触发集成员（隐式交集引用）
    assert_eq!(
        guard_csv_cell("@SUM(A1:A9)", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "'@SUM(A1:A9)"
    );
}

#[test]
fn c05_leading_space_untouched() {
    // 前置空格在 Excel 语义下已中和公式解释——消毒即重复编码，须防
    let v = " =cmd|'/c calc'!A1";
    assert_eq!(
        guard_csv_cell(v, CsvFormulaMode::PrefixApostrophe).unwrap(),
        v
    );
}

#[test]
fn c06_already_disinfected_untouched() {
    // 防重复编码：已带 ' 前缀再叠加 = ''=... 损坏内容（幂等性）
    let v = "'=cmd|'/c calc'!A1";
    assert_eq!(
        guard_csv_cell(v, CsvFormulaMode::PrefixApostrophe).unwrap(),
        v
    );
}

#[test]
fn c07_legal_apostrophe_text_untouched() {
    // 防合法内容损坏：撇号开头 ≠ 已消毒的公式（'h 不构成公式风险，
    // 保守判定不动——宁漏消毒不损坏合法内容）
    assert_eq!(
        guard_csv_cell("'hello", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "'hello"
    );
}

#[test]
fn c08_fullwidth_equals_untouched() {
    // Unicode 同形攻击：Excel 不按全角＝ U+FF1D 解释公式——动了反而损坏
    assert_eq!(
        guard_csv_cell("＝cmd", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "＝cmd"
    );
}

#[test]
fn c09_plain_formula_still_prefixed() {
    // 非选择性消毒：guard 不做恶意判定（无法判定），全量触发——
    // 「无启发式」契约钉死
    assert_eq!(
        guard_csv_cell("=1+1", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "'=1+1"
    );
}

#[test]
fn c10_tab_prefix_untouched() {
    // 边界钉定：触发判定 = 字面首字符，\t 非触发集成员（前导空白
    // 不剥离不扩集 = 模块审计规格最小面，调用方按需预处理）
    assert_eq!(
        guard_csv_cell("\t=cmd", CsvFormulaMode::PrefixApostrophe).unwrap(),
        "\t=cmd"
    );
}

#[test]
fn c01_mode_comparisons() {
    // 模式二：前置空格
    assert_eq!(
        guard_csv_cell("=cmd|'/c calc'!A1", CsvFormulaMode::PrefixSpace).unwrap(),
        " =cmd|'/c calc'!A1"
    );
    // 模式三：typed reject（含触发字符与原值）
    assert_eq!(
        guard_csv_cell("=cmd|'/c calc'!A1", CsvFormulaMode::Reject).unwrap_err(),
        CsvGuardError::FormulaPrefixRejected {
            ch: '=',
            cell: "=cmd|'/c calc'!A1".into()
        }
    );
    // C-05/C-06/C-07 三例在全部三模式下均不动（幂等/合法保护 =
    // 模式无关不变量）
    for v in [" =cmd|'/c calc'!A1", "'=cmd|'/c calc'!A1", "'hello"] {
        for mode in [
            CsvFormulaMode::PrefixApostrophe,
            CsvFormulaMode::PrefixSpace,
            CsvFormulaMode::Reject,
        ] {
            assert_eq!(
                guard_csv_cell(v, mode).unwrap(),
                v,
                "模式无关不变量：{v:?} @ {mode:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 面 2：xml_control_chars（X-01..X-10）
// ---------------------------------------------------------------------------

#[test]
fn x01_nul_escaped() {
    // 基线：NUL 转义
    assert_eq!(escape_control_chars("a\u{0}b"), "a_x0000_b");
}

#[test]
fn x02_literal_form_disambiguated() {
    // 防护绕过（解码面走私）：若不消歧，原文照排 → 下游解码变真 NUL
    let esc = escape_control_chars("_x0000_");
    assert_eq!(esc, "_x005F_x0000_");
    // 消歧后解码回原文零失真
    assert_eq!(unescape_control_chars(&esc), "_x0000_");
}

#[test]
fn x03_reencode_prefix_grows_level_by_level() {
    // 防重复编码（可解码性口径）：已消毒文本再过一遍不产生解码歧义。
    // 幂等语义 = decode(encode(x)) == x 恒成立；字符串级
    // encode(encode(x)) == encode(x) 不成立（消歧前缀逐级增长——
    // 有意设计，否则解码歧义）。
    // 注：设计表首版此行期望串有拼接笔误（xlsx-r1 SANITIZE_TESTS
    // 回写登记）；本断言 = mdcore 实测精确值。
    let once = escape_control_chars("_x0000_");
    assert_eq!(once, "_x005F_x0000_");
    let twice = escape_control_chars(&once);
    assert_eq!(twice, "_x005F_x005F_x005F_x0000_");
    // 可解码性不变量（安全属性本体）：decode(encode(x)) == x 恒成立，
    // 含 x 本身已是消歧形态（此处 x = once，非其原像 _x0000_）
    assert_eq!(unescape_control_chars(&twice), once);
}

#[test]
fn x04_legal_controls_and_del_untouched() {
    // 防合法内容损坏：XML 1.0 合法字符放行集钉死（\t \n \r 0x7F）
    let v = "a\tb\nc\rd\u{7F}e";
    assert_eq!(escape_control_chars(v), v);
}

#[test]
fn x05_legal_char_spoof_form_disambiguated() {
    // 同 X-02：合法字符（\n）的伪装形态同样须消歧（类型走私）
    let esc = escape_control_chars("_x000A_");
    assert_eq!(esc, "_x005F_x000A_");
    assert_eq!(unescape_control_chars(&esc), "_x000A_");
}

#[test]
fn x06_uppercase_x_not_a_form() {
    // 转义约定大小写敏感（小写 _x）——大写是合法字面量，解码器不认
    assert_eq!(escape_control_chars("_X0000_"), "_X0000_");
}

#[test]
fn x07_wrong_hex_length_not_a_form() {
    // 非法形态（HHHH 恰四位）：3 位 / 5 位十六进制都不是转义形态
    assert_eq!(escape_control_chars("_x000_"), "_x000_");
    assert_eq!(escape_control_chars("_x00000_"), "_x00000_");
}

#[test]
fn x08_non_hex_digit_not_a_form() {
    // G 非十六进制位 → 不构成形态
    assert_eq!(escape_control_chars("_x00G0_"), "_x00G0_");
}

#[test]
fn x09_ffff_two_paths_symmetric() {
    // 非字符字面量消歧 + 该码位本身在转义集——两路径对称钉死
    let esc = escape_control_chars("_xFFFF_");
    assert_eq!(esc, "_x005F_xFFFF_");
    assert_eq!(unescape_control_chars(&esc), "_xFFFF_");
    // 真出现 U+FFFF 则直接转义
    assert_eq!(escape_control_chars("\u{FFFF}"), "_xFFFF_");
}

#[test]
fn x10_fffe_real_char_escaped() {
    // 转义集上界成员（多字节字符级判定，逃逸边界闭合的实测点）
    assert_eq!(escape_control_chars("\u{FFFE}"), "_xFFFE_");
    assert_eq!(unescape_control_chars("_xFFFE_"), "\u{FFFE}");
}

// ---------------------------------------------------------------------------
// 面 3：xml_entities（E-01..E-07）
// ---------------------------------------------------------------------------

#[test]
fn e01_all_entities_baseline() {
    // 基线全实体（属性位 5 实体）
    assert_eq!(
        escape_xml_attr("<tag>&\"attr\"</tag>"),
        "&lt;tag&gt;&amp;&quot;attr&quot;&lt;/tag&gt;"
    );
}

#[test]
fn e02_already_escaped_treated_as_literal() {
    // 契约钉死（防重复编码误设计）：纯转义器必须把已转义形态当字面
    // 文本（否则解码失真）；幂等性故意不成立——测试锁死防「优化」
    // 成幂等（那会破坏往返）
    assert_eq!(escape_xml_text("&amp;"), "&amp;amp;");
}

#[test]
fn e03_attribute_injection_neutralized() {
    // 防护绕过：属性注入经典载荷——引号转义后无法闭合属性
    assert_eq!(
        escape_xml_attr("x\" onmouseover=\"alert(1)"),
        "x&quot; onmouseover=&quot;alert(1)"
    );
}

#[test]
fn e04_cdata_terminator_escaped() {
    // CDATA 逃逸形态（XML 注入面）
    assert_eq!(escape_xml_text("]]>"), "]]&gt;");
}

#[test]
fn e05_control_char_integrated_path() {
    // 控制字符一体化兜底（与面 2 同输出——单一实现两出口）
    assert_eq!(escape_xml_attr_full("a\u{0}b"), "a_x0000_b");
}

#[test]
fn e06_numeric_entity_smuggling_neutralized() {
    // 防护绕过（解码面走私）：不转义 & 则下游解析器把数字实体还原成
    // NUL——& 转义即中和
    assert_eq!(escape_xml_text("&#x00;"), "&amp;#x00;");
}

#[test]
fn e07_tag_smuggling_neutralized() {
    // 同 E-06，标签走私形态
    assert_eq!(
        escape_xml_text("&#60;script&#62;"),
        "&amp;#60;script&amp;#62;"
    );
}

// ---------------------------------------------------------------------------
// 面 4：html_escape（H-01..H-06）
// ---------------------------------------------------------------------------

#[test]
fn h01_tag_injection_neutralized() {
    // 基线：标签注入中和
    assert_eq!(
        escape_html_text("<img src=x onerror=alert(1)>"),
        "&lt;img src=x onerror=alert(1)&gt;"
    );
}

#[test]
fn h02_attribute_breakout_neutralized() {
    // 属性闭合 breakout
    assert_eq!(
        escape_html_attr("\" onmouseover=\"alert(1)"),
        "&quot; onmouseover=&quot;alert(1)"
    );
}

#[test]
fn h03_url_scheme_attack_ineffective_to_char_escaping() {
    // 边界实证（本测试组的关键发现）：字符转义对 URL scheme 攻击
    // **无效**——scheme 无特殊字符可转。本用例把该边界钉死为测试，
    // 作为面 6（url_policy）必要性的在档证据。
    assert_eq!(
        escape_html_attr("javascript:alert(1)"),
        "javascript:alert(1)"
    );
}

#[test]
fn h04_data_uri_ineffective_to_char_escaping() {
    // 同 H-03：data URI 载荷
    assert_eq!(
        escape_html_attr("data:text/html;base64,PHNjcmlwdD4="),
        "data:text/html;base64,PHNjcmlwdD4="
    );
}

#[test]
fn h05_single_quote_breakout_neutralized() {
    // 单引号属性位 breakout（五实体全覆盖；HTML 面 ' 用 &#39;）
    assert_eq!(escape_html_attr("' onclick='x"), "&#39; onclick=&#39;x");
}

#[test]
fn h06_already_escaped_literal_roundtrip() {
    // 同 E-02 契约：字面已转义文本按字面处理（往返保真）
    assert_eq!(escape_html_attr("&amp;lt;"), "&amp;amp;lt;");
}

// ---------------------------------------------------------------------------
// 面 5：field_whitelist（F-01..F-10）
// ---------------------------------------------------------------------------

#[test]
fn f01_includetext_rejected() {
    // 外部资源面基线
    assert!(matches!(
        FieldGuard::default().check("INCLUDETEXT \"c:\\evil.docx\""),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn f02_ddeauto_rejected() {
    // 执行面基线
    assert!(matches!(
        FieldGuard::default().check("DDEAUTO cmd /c calc"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn f03_nested_injection_after_switch_rejected() {
    // 防护绕过：嵌套注入（合法域后挂黑名单域）——全 token 扫描靶心
    assert!(matches!(
        FieldGuard::default().check("TOC \\o \"1-3\" INCLUDETEXT evil"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn f04_switch_position_injection_rejected() {
    // 同 F-03，开关位后挂
    assert!(matches!(
        FieldGuard::default().check("MERGEFIELD x \\d DDEAUTO"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn f05_case_evasion_rejected() {
    // 大小写逃逸（域关键字大小写不敏感——Word 同样不敏感，须对称）
    assert!(matches!(
        FieldGuard::default().check("incluDeText \"x\""),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn f06_quoted_deny_keyword_word_semantics_pinned() {
    // 设计-实现出入钉定（2026-10-13 阶段 B 实测）：引号**包裹**黑
    // 关键字（`"INCLUDETEXT"`）实测**放行**——mdcore 实现为空白
    // token 化 + token 级前缀扫描（前缀匹配从 token 首字符起），
    // 引号内 token 不命中。语义依据：Word 域语法中引号内为字符串
    // 字面量参数（无执行语义——嵌套域须花括号 `{ }`，花括号内的
    // 裸 token 由全 token 扫描覆盖，见 f03_quoted_braces 对测）；
    // 已拦截的两形态 = 首 token 裸关键字（f01）与后缀粘连
    // `INCLUDETEXT"x"`（既有 sanitize_gates 形态二）。设计表 F-06
    // 首版期望 reject 已在分析仓回写修正。
    assert!(FieldGuard::default().check("TOC \"INCLUDETEXT\"").is_ok());
    // 花括号嵌套域形态（真实嵌套注入面）：裸 token 被全扫描拦截
    assert!(matches!(
        FieldGuard::default().check("TOC { INCLUDETEXT x }"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn f07_whitelist_families_allowed() {
    // 防合法内容损坏：七白成员精确放行
    let guard = FieldGuard::default();
    for ok in ["DATE", "PAGE", "MERGEFIELD name"] {
        assert!(guard.check(ok).is_ok(), "白名单应放行：{ok:?}");
    }
}

#[test]
fn f08_shorter_prefix_not_keyword_allowed() {
    // 前缀扫描的误杀边界（短前缀形态）：DATED / PAGES / INCLUDETX
    // 都不是黑名单关键字（前缀不足），拒了就损坏合法（用户自造域）。
    // 注：隔离黑名单边界须用黑名单制策略——默认白名单制下未知域族
    // （DATED/PAGES）会因 NotInWhitelist 被拒（正确行为：未知域族
    // 不放行），那是白名单门的职责面，非黑名单误杀。设计表 F-08
    // 首版未区分两门，已在分析仓回写。
    let bl = FieldGuard::new(FieldPolicy::Blacklist);
    for ok in ["DATED", "PAGES", "INCLUDETX"] {
        assert!(bl.check(ok).is_ok(), "短前缀不应被黑名单误杀：{ok:?}");
    }
    // 同三例在默认白名单制下的真实归类：非白名单域族 → 拒
    // （NotInWhitelist——白名单门语义正确，与黑名单边界分立钉定）
    let guard = FieldGuard::default();
    for unknown in ["DATED", "PAGES", "INCLUDETX"] {
        assert!(
            matches!(
                guard.check(unknown),
                Err(FieldRejected::NotInWhitelist { .. })
            ),
            "未知域族默认拒绝：{unknown:?}"
        );
    }
}

#[test]
fn f09_conservative_false_positive_registered() {
    // 已知保守误杀：数据字段名以黑名单字开头——安全优先于可用性，
    // 登记不修复（病理形态；crate 文档消费契约注记 3）
    assert!(matches!(
        FieldGuard::default().check("MERGEFIELD INCLUDETEXT_list"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}

#[test]
fn f10_superset_prefix_caught_same_class_as_f09() {
    // 设计-实现出入钉定（2026-10-13 阶段 B 实测）：前缀**超集**
    // token（INCLUDETEXTX）实测**拦截**——token.startsWith(family)
    // 前缀扫描是后缀粘连形态（INCLUDETEXT"x" / DDEEXCEL|...）捕获
    // 的设计代价，与 F-09 同类保守误杀（真实域关键字不含此形态；
    // 白名单首 token 判定仍精确）。设计表 F-10 首版期望放行已回写。
    assert!(matches!(
        FieldGuard::default().check("TOC \\o INCLUDETEXTX"),
        Err(FieldRejected::DeniedFamily { .. })
    ));
}
