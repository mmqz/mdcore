//! pdfl_forms_incremental —— issue #227：PDF forms 保存「真增量修订」
//! 语义锁（feature `engine-lopdf`）。
//!
//! # 背景（审计 P0-3）
//!
//! 修复前 `PdfForm::save()` 走 lopdf `Document::save_to` **全量重写**，
//! 而模块文档与 `save()` 都声称「增量修订、前缀逐位不变、既有签名仍
//! 有效」——声称与实现不符。目标场景「填已签名投标函」：填表保存 →
//! 既有数字签名被**静默摧毁**（PAdES 硬约束违反）。
//!
//! 本文件锁增量写语义（与 `pdfd_writer.rs` 的 LopdfWriter 语义锁同款
//! 纪律，落在表单面）：
//!
//! 1. `save_appends_one_revision_prefix_bitwise`——填表保存 = 追加
//!    **恰好一个**修订段（`%%EOF` 计数 +1）；原字节为产物逐位相同前缀；
//!    重载后新值可读（增量更新语义：同对象号后修订覆盖前修订）；
//! 2. `incremental_chain_transitive`——两代保存链传递：一代产物是二代
//!    产物的逐位前缀，两代填的值都可见；
//! 3. `new_objects_land_in_appended_revision`——无字体填中文：值对象
//!    重定义 + `/NeedAppearances`（AcroForm 重定义）都落在追加段里，
//!    且原字节段里不含这些新定义；
//! 4. `save_refuses_reconstructed_xref_input`——xref 链守卫：扫描重建
//!    装载档（行尾污染形态）上拒绝写并给可操作指引（2026-10-31 事故
//!    防线，`writer_lopdf.rs` 同款）。
//!
//! 已签名文档的签名有效性保持（ByteRange 覆盖段逐位不变）验收面在
//! `signed_sample.pdf` 上——见本文件第二部分。

#![cfg(feature = "engine-lopdf")]

use std::path::PathBuf;

use mdc_pdf::forms::{FormError, FormField, PdfForm};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn bid_letter() -> Vec<u8> {
    std::fs::read(fixtures().join("form_bid_letter.pdf")).expect("夹具 form_bid_letter.pdf 缺失")
}

fn signed_pdf() -> Vec<u8> {
    std::fs::read(fixtures().join("signed_sample.pdf")).expect("signed_sample.pdf 缺失")
}

fn eof_count(bytes: &[u8]) -> usize {
    bytes.windows(5).filter(|w| w == b"%%EOF").count()
}

/// 模拟 Windows autocrlf 检出污染（每个 LF 前插 CR——与 pdfi_xref_chain
/// 同款构造：lopdf 对该形态退化为扫描重建装载，xref_start = 0）。
fn crlf_mangled(data: &[u8]) -> Vec<u8> {
    data.iter()
        .flat_map(|&b| {
            if b == b'\n' {
                vec![b'\r', b'\n']
            } else {
                vec![b]
            }
        })
        .collect()
}

// ---------------------------------------------------------------- 1. 增量语义

#[test]
fn save_appends_one_revision_prefix_bitwise() {
    let original = bid_letter();
    let mut form = PdfForm::read(&original).unwrap();
    form.set_field("project_name", "智慧园区弱电工程").unwrap();
    form.set_field("duration_days", "120").unwrap();
    let out = form.save().unwrap();

    // 核心断言：原字节逐位前缀（全量重写在此必红——这就是修复前的缺陷形态）。
    assert!(
        out.starts_with(&original),
        "增量保存产物必须以原字节为逐位相同前缀（got len {} vs original {}）",
        out.len(),
        original.len()
    );
    assert!(out.len() > original.len(), "应追加了非空修订段");
    assert_eq!(
        eof_count(&out),
        eof_count(&original) + 1,
        "恰好追加一个修订段（%%EOF 计数 +1）"
    );

    // 增量更新语义：重载后新值生效（同对象号后修订覆盖前修订）。
    let back = PdfForm::read(&out).unwrap();
    let Some(FormField::Text { value, .. }) = back.field("project_name") else {
        panic!()
    };
    assert_eq!(value, "智慧园区弱电工程", "中文值往返无损");
    let Some(FormField::Text { value, .. }) = back.field("duration_days") else {
        panic!()
    };
    assert_eq!(value, "120");
}

#[test]
fn incremental_chain_transitive() {
    let original = bid_letter();
    // 一代：填工期
    let mut form = PdfForm::read(&original).unwrap();
    form.set_field("duration_days", "120").unwrap();
    let gen1 = form.save().unwrap();
    assert!(gen1.starts_with(&original));

    // 二代：在一代产物上再填项目名
    let mut form2 = PdfForm::read(&gen1).unwrap();
    form2.set_field("project_name", "智慧园区弱电工程").unwrap();
    let gen2 = form2.save().unwrap();
    assert!(
        gen2.starts_with(&gen1),
        "二代产物必须以一代产物为逐位前缀（增量链传递性）"
    );
    assert_eq!(eof_count(&gen2), eof_count(&original) + 2);

    // 两代填的值都可见
    let back = PdfForm::read(&gen2).unwrap();
    let Some(FormField::Text { value, .. }) = back.field("duration_days") else {
        panic!()
    };
    assert_eq!(value, "120", "一代的值在二代产物中仍可见");
    let Some(FormField::Text { value, .. }) = back.field("project_name") else {
        panic!()
    };
    assert_eq!(value, "智慧园区弱电工程");
}

#[test]
fn new_objects_land_in_appended_revision() {
    let original = bid_letter();
    let mut form = PdfForm::read(&original).unwrap();
    form.set_field("project_name", "中文项目名").unwrap();
    let out = form.save().unwrap();

    let split = original.len();
    let (prefix, appended) = out.split_at(split);
    assert_eq!(prefix, &original[..], "前缀段 = 原字节");
    // 新定义只出现在追加段：值（UTF-16BE 带 BOM）与 NeedAppearances。
    // 原件里 project_name 值为空串、AcroForm 无 NeedAppearances 键。
    assert!(
        appended.windows(2).any(|w| w == [0xFE, 0xFF]),
        "追加段应含 UTF-16BE BOM（中文值的新定义）"
    );
    let appended_text = String::from_utf8_lossy(appended);
    assert!(
        appended_text.contains("/NeedAppearances"),
        "无字体填中文应置 NeedAppearances（AcroForm 重定义在追加段）"
    );
    let prefix_text = String::from_utf8_lossy(prefix);
    assert!(
        !prefix_text.contains("/NeedAppearances true"),
        "前缀段（原字节）不得被改动"
    );
}

// ---------------------------------------------------------------- 2. xref 链守卫

#[test]
fn save_refuses_reconstructed_xref_input() {
    let pristine = bid_letter();
    let mangled = crlf_mangled(&pristine);

    // 前置钉定：篡改档可装载可读域（宽容重建）——这正是事故的隐蔽面。
    let probe = PdfForm::read(&mangled).unwrap();
    assert!(!probe.is_empty(), "重建装载下域仍可读（隐蔽面）");
    drop(probe);

    // 填表后保存必须响亮拒绝（不得静默产出链断裂产物）。
    let mut form = PdfForm::read(&mangled).unwrap();
    form.set_field("project_name", "x").unwrap();
    let err = form.save().unwrap_err();
    match err {
        FormError::Save(msg) => {
            assert!(msg.contains("xref"), "拒绝理由应指向 xref 链：{msg}");
            assert!(
                msg.contains("重取") || msg.contains("修复"),
                "应给可操作指引：{msg}"
            );
        }
        other => panic!("应为 Save 错误，得 {other:?}"),
    }
}

// ---------------------------------------------------------------- 3. 已签名文档

#[test]
fn signed_bid_letter_fill_keeps_signature_valid() {
    let original = signed_pdf();

    // 前置钉定：夹具确为已签署形态（1 个已签署签名域）。
    let sigs = mdc_pdf::signature::read_signatures(&original).unwrap();
    assert_eq!(sigs.len(), 1, "夹具应含 1 个签名域");
    assert!(sigs[0].structurally_valid, "夹具签名应结构完整");

    // 目标场景：填已签名投标函。
    let mut form = PdfForm::read(&original).unwrap();
    form.set_field("project_name", "智慧园区弱电工程").unwrap();
    form.set_field("duration_days", "120").unwrap();
    form.set_checkbox("consent", true).unwrap();
    let out = form.save().unwrap();

    // ① 前缀逐位不变（PAdES 硬约束的机制面——追加内容全部落在既有
    //    %%EOF 之后）。
    assert!(
        out.starts_with(&original),
        "已签名档保存必须保持原字节前缀（否则签名被毁）"
    );

    // ② /ByteRange 覆盖段逐位相同（签名摘要输入不变——签名有效性保持
    //    的直接证据；修复前全量重写在这里必红）。
    let sigs_after = mdc_pdf::signature::read_signatures(&out).unwrap();
    assert_eq!(sigs_after.len(), 1, "签名域应仍存在");
    let s = &sigs_after[0];
    assert!(
        s.structurally_valid,
        "签名应仍结构完整：{:?}",
        s.structural_issue
    );
    assert_eq!(s.field_name, "Signature1");
    assert_eq!(
        s.byte_range.len(),
        4,
        "ByteRange 应为 4 元：{:?}",
        s.byte_range
    );
    for chunk in s.byte_range.chunks(2) {
        let (start, len) = (chunk[0], chunk[1]);
        assert!(
            out[start..start + len] == original[start..start + len],
            "ByteRange 覆盖段 [{start}, {start}+{len}] 必须逐位不变（签名摘要输入）"
        );
    }

    // ③ 填的值在产物中生效（增量更新语义 + 签名共存）。
    let back = PdfForm::read(&out).unwrap();
    let Some(FormField::Text { value, .. }) = back.field("project_name") else {
        panic!()
    };
    assert_eq!(value, "智慧园区弱电工程");
    let Some(FormField::Checkbox { checked, .. }) = back.field("consent") else {
        panic!()
    };
    assert!(checked);
    // 签名域本身仍在（未被动过——增量修订只加不改）。
    let sig_field = back
        .fields()
        .into_iter()
        .find(|f| matches!(f, FormField::Signature { .. }));
    assert!(sig_field.is_some(), "签名域应保留在表单域集中");
}
