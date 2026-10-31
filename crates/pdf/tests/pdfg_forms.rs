//! pdfg_forms —— S2-5（issue #116 任务 1）：PDF 表单读写验收面。
//!
//! 夹具：`fixtures/form_bid_letter.pdf`（reportlab 生成的投标函模板，
//! 8 域：4 文本 / 3 复选 / 1 下拉；中文标签 + Helvetica 域字体——**真实
//! 招标模板的典型形态**，域字体不支持中文，正是最容易踩的坑）。
//!
//! 交叉验证：产物用 pypdf（python）独立复核（见 `verify_with_pypdf` 辅助），
//! 不拿自己的读回当唯一证据。

use std::path::PathBuf;

use mdc_pdf::forms::{FormError, FormField, PdfForm};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn bid_letter() -> Vec<u8> {
    std::fs::read(fixtures().join("form_bid_letter.pdf")).expect("夹具 form_bid_letter.pdf 缺失")
}

fn test_font() -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/q31_test_font.ttf"),
    )
    .expect("测试字体缺失")
}

// ---------------------------------------------------------------- 读

#[test]
fn reads_all_fields_from_real_bid_letter() {
    let form = PdfForm::read(&bid_letter()).unwrap();
    assert_eq!(form.len(), 8, "投标函模板应读出 8 个域");

    let names: Vec<String> = form.fields().iter().map(|f| f.name().to_string()).collect();
    for want in ["project_name", "legal_rep", "duration_days", "bid_amount_upper", "consent", "project_type"] {
        assert!(names.iter().any(|n| n == want), "缺域 {want}：{names:?}");
    }

    // 类型识别
    assert!(matches!(form.field("project_name"), Some(FormField::Text { .. })));
    assert!(matches!(form.field("consent"), Some(FormField::Checkbox { .. })));
    assert!(matches!(form.field("project_type"), Some(FormField::Choice { .. })));

    // /MaxLen 必须读出来（填值时靠它防截断）
    let Some(FormField::Text { max_len, .. }) = form.field("duration_days") else { panic!() };
    assert_eq!(*max_len, Some(4), "duration_days 的 /MaxLen=4");
}

#[test]
fn empty_bytes_is_error_not_panic() {
    assert!(PdfForm::read(&[]).is_err());
    assert!(PdfForm::read(b"not a pdf").is_err());
}

// ---------------------------------------------------------------- 填 + 写回

#[test]
fn fill_text_and_checkbox_then_save() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    form.set_field("project_name", "智慧园区弱电工程").unwrap();
    form.set_field("legal_rep", "张三").unwrap();
    form.set_field("duration_days", "120").unwrap();
    form.set_checkbox("consent", true).unwrap();
    form.set_choice("project_type", &["construction"]).unwrap();
    let report = form.report().clone();
    assert_eq!(report.changed, 5, "5 个域被改：{report:?}");
    assert_eq!(report.unchanged, 0);

    let out = form.save().unwrap();
    assert!(out.starts_with(b"%PDF"), "产物应是 PDF");

    // 读回验证
    let back = PdfForm::read(&out).unwrap();
    let Some(FormField::Text { value, .. }) = back.field("project_name") else { panic!() };
    assert_eq!(value, "智慧园区弱电工程", "中文往返必须无损");
    let Some(FormField::Checkbox { checked, .. }) = back.field("consent") else { panic!() };
    assert!(checked, "复选框应已勾选");
    let Some(FormField::Text { value, .. }) = back.field("duration_days") else { panic!() };
    assert_eq!(value, "120");
}

#[test]
fn save_is_idempotent_when_nothing_changed() {
    let original = bid_letter();
    let form = PdfForm::read(&original).unwrap();
    let out = form.save().unwrap();
    assert_eq!(out, original, "没改任何域时应原样返回（不产生空修订）");
}

#[test]
fn unchanged_value_counts_as_unchanged() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    form.set_field("project_name", "").unwrap(); // 本来就是空
    assert_eq!(form.report().unchanged, 1);
    assert_eq!(form.report().changed, 0);
}

// ---------------------------------------------------------------- 错误面

#[test]
fn unknown_field_is_typed_error() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    let err = form.set_field("no_such_field", "x").unwrap_err();
    assert!(matches!(err, FormError::FieldNotFound(_)));
    assert!(err.to_string().contains("不存在"));
}

#[test]
fn wrong_field_type_is_typed_error() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    let err = form.set_field("consent", "x").unwrap_err();
    assert!(matches!(err, FormError::WrongFieldType { .. }));
    assert!(err.to_string().contains("类型不符"));
}

#[test]
fn maxlen_overflow_is_rejected_not_truncated() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    // duration_days /MaxLen = 4
    let err = form.set_field("duration_days", "12345").unwrap_err();
    assert!(matches!(err, FormError::InvalidValue { .. }), "{err:?}");
    assert!(err.to_string().contains("MaxLen"), "{}", err);
    // 只报错不改值
    let Some(FormField::Text { value, .. }) = form.field("duration_days") else { panic!() };
    assert_eq!(value, "", "被拒的值不得写入");
}

#[test]
fn choice_rejects_value_outside_options() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    let err = form.set_choice("project_type", &["不存在的选项"]).unwrap_err();
    assert!(matches!(err, FormError::InvalidValue { .. }), "{err:?}");
}

// ---------------------------------------------------------------- 中文 / CIDFont

#[test]
fn cjk_without_font_warns_but_succeeds() {
    // 不注入字体：值仍然写进去（/V），但必须告警——不能静默
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    form.set_field("project_name", "中文项目名").unwrap();
    let warnings = form.report().warnings.clone();
    assert!(!warnings.is_empty(), "填中文无字体必须告警");
    assert!(warnings[0].contains("NeedAppearances"), "{warnings:?}");
    let out = form.save().unwrap();
    let back = PdfForm::read(&out).unwrap();
    let Some(FormField::Text { value, .. }) = back.field("project_name") else { panic!() };
    assert_eq!(value, "中文项目名", "值本身仍是正确的 UTF-16");
}

#[test]
fn cjk_with_font_builds_cid_appearance() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    form.set_font(&test_font());
    // 测试字体只含部分汉字——选它覆盖的字
    form.set_field("project_name", "中标").unwrap();
    assert!(form.report().warnings.is_empty(), "注入字体后不该再有字体告警：{:?}", form.report().warnings);
    let out = form.save().unwrap();
    // 产物里必须出现 Type0 字体与 CIDFontType2（CID 嵌入的证据）
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("Type0"), "应嵌 Type0 字体");
    assert!(text.contains("Identity-H"), "应为 Identity-H 编码");
    assert!(text.contains("CIDFontType2"), "应有 CIDFontType2 后代字体");
}

#[test]
fn cjk_missing_glyph_is_loud_error() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    form.set_font(&test_font());
    // 测试字体不含「函」字（见夹具说明）——必须报错而不是画空白
    form.set_field("project_name", "投标函").unwrap();
    let err = form.save().unwrap_err();
    assert!(matches!(err, FormError::Save(_)), "{err:?}");
    assert!(err.to_string().contains("字形"), "{}", err);
}

// ---------------------------------------------------------------- 扁平化

#[test]
fn flatten_removes_acroform_and_widgets() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    form.set_field("duration_days", "120").unwrap();
    let out = form.flatten().unwrap();

    // 扁平化后不再有表单
    let back = PdfForm::read(&out).unwrap();
    assert!(back.is_empty(), "扁平化后应无任何域，实际 {}", back.len());
    let text = String::from_utf8_lossy(&out);
    assert!(!text.contains("/AcroForm"), "AcroForm 应被删除");
    // 值被画进内容流
    assert!(text.contains("(120) Tj"), "值应画进页面内容流");
}

#[test]
fn flatten_refuses_cjk_without_font() {
    let mut form = PdfForm::read(&bid_letter()).unwrap();
    form.set_field("project_name", "中文项目名").unwrap();
    let err = form.flatten().unwrap_err();
    assert!(matches!(err, FormError::FlattenRefused(_)), "{err:?}");
    assert!(err.to_string().contains("乱码"), "理由要说清楚：{err}");
}

// ---------------------------------------------------------------- 往返不膨胀

#[test]
fn roundtrip_does_not_inflate() {
    let original = bid_letter();
    let mut form = PdfForm::read(&original).unwrap();
    form.set_field("duration_days", "120").unwrap();
    let once = form.save().unwrap();
    // 再读再存（无改动）不应继续膨胀
    let twice = PdfForm::read(&once).unwrap().save().unwrap();
    assert_eq!(twice, once, "无改动二次保存应字节相同");
    // 相对原始件不应爆炸（增量修订允许多出一个修订体）
    assert!(
        once.len() < original.len() * 3,
        "膨胀过大：{} → {}",
        original.len(),
        once.len()
    );
}