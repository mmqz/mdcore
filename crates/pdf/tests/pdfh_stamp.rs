//! pdfh_stamp —— S2-6（issue #116 任务 3）：盖章/图片叠加验收面。
//!
//! 夹具：`fixtures/stamp_seal.png`（半透明红章——RGBA，模拟真实公章）。
//! 基座：`fixtures/small.pdf`（既有单页夹具）。
//!
//! 交叉验证：产物用 pypdf 独立复核图片 XObject 与内容流（见 PR 记录）。
//!
//! 安全门锁（#277 / 审计 P1-11——加密档盖章静默损坏）：
//! - `still_encrypted_pdf_stamp_is_rejected`——S5 写拒（仍加密态：lopdf
//!   未解密装载、对象图不解析，此前保存产物静默损坏）；
//! - `was_encrypted_pdf_stamp_is_rejected`——S5 双拒（曾加密态：空口令
//!   自动解密后对象图可用，但保存 = 静默剥离加密层——文件本体是
//!   加密档即拒，与 writer_lopdf 同判定）；
//! - `oversized_input_is_rejected_before_parsing`——S3-L7 输入门（注入
//!   面确定性触发——默认 1 GiB 不在单测里真造，与 pdfe_security 7 号
//!   同款策略）。
//!
//! #408 签名门（已签名档盖章必毁签——默认 fail-closed）：
//! - `signed_pdf_stamp_is_rejected`——验收 1：默认参数拒；
//! - `signed_pdf_stamp_succeeds_when_break_explicit`——验收 2：显式放行
//!   可盖（签名确实失效——如实）；
//! - 验收 3（未签名档正常）：既有 `stamps_seal_into_page_content_stream`
//!   等全套 small.pdf 用例即覆盖，不重复。
//! 判定面三路分支（SigFlags / /FT /Sig / /Perms /DocMDP）的确定性单测
//! 在 stamp.rs 内联 tests（构造目录标记，不依赖夹具）。

use std::path::PathBuf;

use mdc_pdf::security::Limits;
use mdc_pdf::stamp::{
    decode_image, stamp_image, stamp_image_with_limits, StampError, StampOptions,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn base_pdf() -> Vec<u8> {
    std::fs::read(fixtures().join("small.pdf")).expect("small.pdf 缺失")
}

fn seal_png() -> Vec<u8> {
    std::fs::read(fixtures().join("stamp_seal.png")).expect("stamp_seal.png 缺失")
}

// ---------------------------------------------------------------- 解码

#[test]
fn decodes_transparent_png_seal() {
    let img = decode_image(&seal_png()).unwrap();
    assert_eq!(img.width, 400);
    assert_eq!(img.height, 400);
    assert_eq!(img.components, 3, "RGB");
    assert_eq!(img.bits, 8);
    assert!(img.alpha.is_some(), "半透明公章必须解出 alpha 通道");
    let alpha = img.alpha.as_ref().unwrap();
    assert_eq!(alpha.len(), 400 * 400);
    // 四角应当是透明的（圆外）
    assert_eq!(alpha[0], 0, "左上角应为全透明");
    // 中心应当不透明（印章笔画）
    let center = alpha[200 * 400 + 200];
    assert!(center > 100, "中心应有印章笔画，实际 alpha={center}");
    assert!(
        alpha.contains(&0) && alpha.iter().any(|&a| a > 0),
        "应同时有透明与不透明像素"
    );
}

#[test]
fn jpeg_path_is_recognised_for_embedding() {
    // 最小 JPEG 头——只验证分类路由（真解码走 DCTDecode，不需要像素）
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x10, 0x03]);
    jpeg.extend_from_slice(&[0u8; 32]);
    let err = decode_image(&jpeg).unwrap_err();
    assert!(
        err.to_string().contains("DCTDecode"),
        "JPEG 应走嵌入而非解码：{err}"
    );
}

// ---------------------------------------------------------------- 盖章

#[test]
fn stamps_seal_into_page_content_stream() {
    let out = stamp_image(
        base_pdf(),
        0,
        [100.0, 100.0, 300.0, 300.0],
        &seal_png(),
        StampOptions::default(),
    )
    .unwrap();

    let text = String::from_utf8_lossy(&out).to_string();
    assert!(text.contains("/Subtype/Image"), "应嵌入图片 XObject");
    assert!(text.contains("/Im0"), "资源里应有 /Im0");
    assert!(text.contains("/SMask"), "半透明 PNG 应带软蒙版");
    assert!(text.contains("Do"), "内容流应有绘制算子");
    // 图要真画进内容流（打印才带），不是注释
    assert!(
        text.contains("200 0 0 200 100 100 cm") || text.contains("200 0 0 200 100.0 100.0 cm"),
        "应出现铺满 rect 的 cm 矩阵"
    );
}

#[test]
fn opacity_multiplies_into_alpha() {
    let out = stamp_image(
        base_pdf(),
        0,
        [50.0, 50.0, 150.0, 150.0],
        &seal_png(),
        StampOptions {
            opacity: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    // SMask 仍在（alpha 已按 0.5 缩放）
    let text = String::from_utf8_lossy(&out).to_string();
    assert!(text.contains("/SMask"));
}

#[test]
fn page_out_of_range_is_typed_error() {
    let err = stamp_image(
        base_pdf(),
        99,
        [0.0, 0.0, 10.0, 10.0],
        &seal_png(),
        StampOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(err, StampError::BadPage(99)), "{err:?}");
}

#[test]
fn zero_area_rect_rejected() {
    let err = stamp_image(
        base_pdf(),
        0,
        [10.0, 10.0, 10.0, 50.0],
        &seal_png(),
        StampOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err, StampError::BadRect);
}

#[test]
fn bad_image_is_typed_error() {
    let err = stamp_image(
        base_pdf(),
        0,
        [0.0, 0.0, 10.0, 10.0],
        b"nope",
        StampOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(err, StampError::BadImage(_)), "{err:?}");
}

#[test]
fn stamping_preserves_existing_content() {
    let base = base_pdf();
    let out = stamp_image(
        base.clone(),
        0,
        [10.0, 10.0, 60.0, 60.0],
        &seal_png(),
        StampOptions::default(),
    )
    .unwrap();
    // 原有页面文本仍在（内容流是追加不是替换）
    let base_text = String::from_utf8_lossy(&base).to_string();
    let out_text = String::from_utf8_lossy(&out).to_string();
    assert!(out.len() > base.len(), "产物应变大");
    // 基座页面的资源与页数不变
    assert!(out_text.matches("/Type/Page").count() >= base_text.matches("/Type/Page").count());
}

// ---------------------------------------------------------------- 安全门（#277 / 审计 P1-11）

/// 现场构造加密档（lopdf V1 RC4——pdfe_security 5 号同款，零仓外依赖）。
///
/// `user_password = ""` → 曾加密形态（装载自动解密：is_encrypted=false /
/// was_encrypted=true）；非空 → 仍加密形态（对象图不解析）。
fn encrypted_pdf(user_password: &str) -> Vec<u8> {
    use lopdf::{dictionary, Object, StringFormat};
    let text_string = |v: &str| Object::String(v.as_bytes().to_vec(), StringFormat::Literal);
    let mut doc = lopdf::Document::with_version("1.7");
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            text_string("mdc-pdf-pdfh-stamp-id"),
            text_string("mdc-pdf-pdfh-stamp-id"),
        ]),
    );
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ],
    }));
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let state: lopdf::EncryptionState = lopdf::EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner-secret",
        user_password,
        permissions: lopdf::Permissions::all(),
    }
    .try_into()
    .expect("加密状态构造");
    doc.encrypt(&state).expect("对象加密");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("加密档序列化");
    bytes
}

#[test]
fn still_encrypted_pdf_stamp_is_rejected() {
    // S5 写拒的原始事故形态：仍加密档（口令不可得）lopdf 以未解密
    // 形态装载、对象图不解析——此前盖章「成功」保存出的产物既非
    // 合法修订也非原档语义（静默损坏）。
    let err = stamp_image(
        encrypted_pdf("user-secret"),
        0,
        [10.0, 10.0, 60.0, 60.0],
        &seal_png(),
        StampOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err, StampError::Encrypted, "仍加密档必须 S5 写拒：{err:?}");
    assert!(
        err.to_string().contains("加密"),
        "错误文案应说明加密态：{err}"
    );
}

#[test]
fn was_encrypted_pdf_stamp_is_rejected() {
    // S5 双拒：曾加密态（空口令自动解密）对象图可用、is_encrypted=false
    // ——但文件本体是加密档，盖章保存 = 静默剥离加密层（安全降级）。
    // 与 writer_lopdf 同判定（v0 S5 = 加密档零写）。
    let bytes = encrypted_pdf("");
    // 前置自证：该夹具确实是「曾加密」形态（防夹具漂移把双拒锁
    // 退化成仍加密单拒锁）。
    let doc = lopdf::Document::load_mem(&bytes).expect("曾加密档可装载（自动解密）");
    assert!(!doc.is_encrypted(), "夹具前置：is_encrypted=false");
    assert!(doc.was_encrypted(), "夹具前置：was_encrypted=true");

    let err = stamp_image(
        bytes,
        0,
        [10.0, 10.0, 60.0, 60.0],
        &seal_png(),
        StampOptions::default(),
    )
    .unwrap_err();
    assert_eq!(
        err,
        StampError::Encrypted,
        "曾加密档同样 S5 写拒（双拒）：{err:?}"
    );
}

#[test]
fn oversized_input_is_rejected_before_parsing() {
    // S3-L7 输入门（open 家族入口统一——先于装载）：注入小限额确定
    // 性触发；HardReject 语义 = 生效值 < 请求值才拒，等长边界放行。
    let tiny = Limits {
        open_file_max_bytes: 16,
        ..Limits::default()
    };
    let err = stamp_image_with_limits(
        base_pdf(),
        0,
        [10.0, 10.0, 60.0, 60.0],
        &seal_png(),
        StampOptions::default(),
        tiny,
    )
    .unwrap_err();
    assert_eq!(err, StampError::TooLarge, "超限必须 typed 拒：{err:?}");

    let exact = Limits {
        open_file_max_bytes: base_pdf().len() as u64,
        ..Limits::default()
    };
    let ok = stamp_image_with_limits(
        base_pdf(),
        0,
        [10.0, 10.0, 60.0, 60.0],
        &seal_png(),
        StampOptions::default(),
        exact,
    );
    assert!(ok.is_ok(), "等长边界应放行：{:?}", ok.err());
}

// ---------------------------------------------------------------- #408 签名门

fn signed_pdf() -> Vec<u8> {
    std::fs::read(fixtures().join("signed_sample.pdf")).expect("signed_sample.pdf 缺失")
}

#[test]
fn signed_pdf_stamp_is_rejected() {
    // 验收 1：已签名档 + 默认参数 → SignedDocument（fail-closed）。
    // 夹具 = pyhanko 真结构签名（/AcroForm + /FT /Sig 在档）。
    let err = stamp_image(
        signed_pdf(),
        0,
        [50.0, 50.0, 150.0, 150.0],
        &seal_png(),
        StampOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err, StampError::SignedDocument, "默认必须拒：{err:?}");
    assert!(
        err.to_string().contains("已签名"),
        "错误文案可指向 UI 提示：{err}"
    );
}

#[test]
fn signed_pdf_stamp_succeeds_when_break_explicit() {
    // 验收 2：allow_break_signature=true → 盖章成功（签名确实失效——
    // 如实；产物应比原档大——新增图对象，且含 /XObject 面）。
    let opts = StampOptions {
        allow_break_signature: true,
        ..Default::default()
    };
    let out = stamp_image(
        signed_pdf(),
        0,
        [50.0, 50.0, 150.0, 150.0],
        &seal_png(),
        opts,
    )
    .expect("显式放行应可盖");
    assert!(out.len() > signed_pdf().len(), "产物应包含新增图对象");
    let text = String::from_utf8_lossy(&out).to_string();
    assert!(text.contains("/XObject"), "图对象应在产物内");
}
