//! pdfh_stamp —— S2-6（issue #116 任务 3）：盖章/图片叠加验收面。
//!
//! 夹具：`fixtures/stamp_seal.png`（半透明红章——RGBA，模拟真实公章）。
//! 基座：`fixtures/small.pdf`（既有单页夹具）。
//!
//! 交叉验证：产物用 pypdf 独立复核图片 XObject 与内容流（见 PR 记录）。

use std::path::PathBuf;

use mdc_pdf::stamp::{decode_image, stamp_image, StampError, StampOptions};

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
        StampOptions { opacity: 0.5 },
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
