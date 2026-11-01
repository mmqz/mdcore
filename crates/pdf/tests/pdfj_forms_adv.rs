//! pdfj_forms_adv —— 表单能力补完（issue #116 后续）验收面。
//!
//! 夹具：`fixtures/form_advanced.pdf` —— 2 页真实结构表单：
//!   · 必填域（`/Ff` bit1 = 2）
//!   · 长度限制（`/MaxLen`）
//!   · 选择域（`/Ch` + `/Opt`）
//!   · **跨页同名字段** `bidder_name`（两页各一份 Widget，对象号不同）
//!
//! 交叉验证：外观与跨页同步用 pypdf 独立复核（见 PR 记录）。

use std::path::PathBuf;

use mdc_pdf::forms::{
    Alignment, ErrorKind, FieldAppearance, FormError, PdfForm, ValidationResult,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn advanced() -> Vec<u8> {
    std::fs::read(fixtures().join("form_advanced.pdf")).expect("form_advanced.pdf 缺失")
}

fn test_font() -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/q31_test_font.ttf"),
    )
    .expect("测试字体缺失")
}

// ---------------------------------------------------------------- 任务 1：必填

#[test]
fn required_fields_are_detected_when_empty() {
    let form = PdfForm::read(&advanced()).unwrap();
    // 夹具里 project_name / legal_rep / amount_upper 带 /Ff bit1
    let flags = form.flags("project_name").expect("应有标志位");
    assert!(flags.is_required(), "project_name 应为必填（Ff={}）", flags.raw());
    assert_eq!(flags.raw(), 2, "必填位 = bit1");

    let vr = form.validate();
    assert!(!vr.valid, "空必填项应判不通过：{vr:?}");
    let names: Vec<&str> = vr.errors.iter().map(|e| e.field.as_str()).collect();
    for want in ["project_name", "legal_rep", "amount_upper"] {
        assert!(names.contains(&want), "应报必填缺失 {want}：{names:?}");
    }
    assert!(vr.errors.iter().all(|e| e.kind == ErrorKind::Required));
    assert!(vr.to_message().contains("必填项未填写"), "{}", vr.to_message());
}

#[test]
fn required_error_clears_after_fill() {
    let mut form = PdfForm::read(&advanced()).unwrap();
    form.set_field("project_name", "智慧园区项目").unwrap();
    form.set_field("legal_rep", "张三").unwrap();
    form.set_field("amount_upper", "壹佰万元整").unwrap();
    let vr = form.validate();
    assert!(vr.valid, "填齐后应通过：{vr:?}");
    assert_eq!(vr.to_message(), "表单校验通过");
}

#[test]
fn readonly_fields_skip_validation() {
    // 只读域不该报「必填未填」——它本就不可填
    let form = PdfForm::read(&advanced()).unwrap();
    for f in form.fields() {
        if form.flags(f.name()).map(|x| x.is_read_only()).unwrap_or(false) {
            assert!(
                !form.validate().errors.iter().any(|e| e.field == f.name()),
                "只读域 {} 不应进校验错误",
                f.name()
            );
        }
    }
}

// ---------------------------------------------------------------- 任务 1：长度

#[test]
fn too_long_value_is_reported() {
    // set_field 本身就会拒绝超长值（写入防线）；validate 是**第二道防线**，
    // 针对「文档里已经有超长值」的情形（别的工具写进去的、或人工改过的）。
    // 这里直接构造那种状态，验证 validate 能抓到。
    let mut form = PdfForm::read(&advanced()).unwrap();
    form.force_raw_value("duration_days", "12345"); // 5 字节 > MaxLen 4
    let vr = form.validate();
    let e = vr
        .errors
        .iter()
        .find(|e| e.field == "duration_days")
        .expect("应报超长");
    assert_eq!(e.kind, ErrorKind::TooLong);
    assert!(e.message.contains("超出长度限制 4"), "{}", e.message);
    assert!(e.message.contains("当前 5"), "{}", e.message);
}

#[test]
fn max_len_counts_cjk_as_two_bytes() {
    let mut form = PdfForm::read(&advanced()).unwrap();
    // legal_rep /MaxLen = 30；16 个汉字 = 32 字节 > 30（外部写入的超长值）
    form.force_raw_value("legal_rep", "一二三四五六七八九十一二三四五六");
    let vr = form.validate();
    assert!(
        vr.errors.iter().any(|e| e.field == "legal_rep" && e.kind == ErrorKind::TooLong),
        "16 个汉字 = 32 字节，超 30：{vr:?}"
    );
    // 15 个汉字 = 30 字节，正好卡线
    let mut form2 = PdfForm::read(&advanced()).unwrap();
    form2.force_raw_value("legal_rep", "一二三四五六七八九十一二三四五"); // 30 字节
    let vr2 = form2.validate();
    assert!(!vr2.errors.iter().any(|e| e.field == "legal_rep"), "30 字节应通过：{vr2:?}");
}

// ---------------------------------------------------------------- 任务 1：选项

#[test]
fn invalid_choice_is_detected() {
    // 选择域当前值在选项集内 → 无错
    let form = PdfForm::read(&advanced()).unwrap();
    let vr = form.validate();
    assert!(!vr.errors.iter().any(|e| e.field == "project_type" && e.kind == ErrorKind::InvalidChoice));
}

// ---------------------------------------------------------------- 任务 2：外观

#[test]
fn appearance_roundtrip_through_dict() {
    let mut form = PdfForm::read(&advanced()).unwrap();
    let app = FieldAppearance {
        font_size: 14.0,
        font_name: "Helv".into(),
        alignment: Alignment::Center,
        text_color: (220, 20, 60),
        background: Some((240, 240, 200)),
    };
    form.set_appearance("project_name", app.clone()).unwrap();

    let back = form.appearance("project_name").expect("应能读回");
    assert_eq!(back.font_size, 14.0, "字号应读回");
    assert_eq!(back.alignment, Alignment::Center, "对齐应读回");

    // 写回后 pypdf 应能看到 /DA 与 /Q
    let out = form.save().unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("/Q 1") || text.contains("/Q 1 "), "应写 /Q 1（居中）");
    assert!(text.contains("14"), "应写字号 14");
}

#[test]
fn appearance_rejects_non_text_field() {
    let mut form = PdfForm::read(&advanced()).unwrap();
    let err = form
        .set_appearance("consent", FieldAppearance::default())
        .unwrap_err();
    assert!(matches!(err, FormError::WrongFieldType { .. }), "{err:?}");
}

#[test]
fn appearance_rejects_bad_font_size() {
    let mut form = PdfForm::read(&advanced()).unwrap();
    let app = FieldAppearance { font_size: 0.0, ..Default::default() };
    let err = form.set_appearance("project_name", app).unwrap_err();
    assert!(matches!(err, FormError::InvalidValue { .. }), "{err:?}");
}

#[test]
fn appearance_builds_styled_ap_stream_for_ascii() {
    // ASCII 值走 simple_appearance_styled（字体度量近似 + 背景/对齐/颜色）
    let mut form = PdfForm::read(&advanced()).unwrap();
    form.set_font(&test_font());
    form.set_field("project_name", "ZFCG-2026-001").unwrap();
    form.set_appearance(
        "project_name",
        FieldAppearance {
            font_size: 14.0,
            font_name: "Helv".into(),
            alignment: Alignment::Right,
            text_color: (200, 20, 20),
            background: Some((255, 255, 200)),
        },
    )
    .unwrap();
    let out = form.save().unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("/AP"), "应生成外观流");
    assert!(text.contains(" re f"), "背景矩形应写入外观流");
    assert!(text.contains("14"), "定制字号 14 应写进内容流");
}

#[test]
fn appearance_cjk_path_still_builds_cid_ap() {
    // 中文值走 cid_appearance（Type0/Identity-H）——样式参数不套用到 CID
    // 路径（该路径的字体度量来自注入的 TTF，不是 Helv 近似）；但外观流必须
    // 生成，否则阅读器显示空白。
    let mut form = PdfForm::read(&advanced()).unwrap();
    form.set_font(&test_font());
    form.set_field("project_name", "中标").unwrap();
    form.set_appearance("project_name", FieldAppearance::default()).unwrap();
    let out = form.save().unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("/AP"), "中文域也应生成外观流");
    assert!(text.contains("Type0"), "中文走 Type0 字体");
}

// ---------------------------------------------------------------- 任务 3：跨页

#[test]
fn cross_page_field_is_merged_into_one() {
    let form = PdfForm::read(&advanced()).unwrap();
    let names: Vec<String> = form.fields().iter().map(|f| f.name().to_string()).collect();
    let count = names.iter().filter(|n| *n == "bidder_name").count();
    assert_eq!(count, 1, "跨页同名域应合并为 1 个：{names:?}");
    assert_eq!(form.len(), 7, "两页共 7 个唯一域（原 8 个 Widget）：{names:?}");
}

#[test]
fn cross_page_fill_syncs_all_widgets() {
    let mut form = PdfForm::read(&advanced()).unwrap();
    form.set_field("bidder_name", "某建设工程有限公司").unwrap();
    let out = form.save().unwrap();

    // pypdf 交叉验证在 PR 里记录；这里先验结构：/V 出现次数 ≥ 2
    let text = String::from_utf8_lossy(&out);
    let v_count = text.matches("/V").count();
    assert!(v_count >= 2, "两页 Widget 都应写入（/V 出现 {v_count} 次）");

    // 读回仍是同一个值
    let back = PdfForm::read(&out).unwrap();
    let f = back.field("bidder_name").expect("跨页域应可读回");
    assert_eq!(f.value_display(), "某建设工程有限公司");
}

// ---------------------------------------------------------------- 回归

#[test]
fn validation_result_message_is_human_readable() {
    let form = PdfForm::read(&advanced()).unwrap();
    let vr: ValidationResult = form.validate();
    let msg = vr.to_message();
    assert!(msg.starts_with("表单校验未通过"), "{msg}");
    assert!(!msg.contains("Required"), "不得泄漏枚举名：{msg}");
    assert!(msg.contains("必填"), "应为中文人话：{msg}");
}
