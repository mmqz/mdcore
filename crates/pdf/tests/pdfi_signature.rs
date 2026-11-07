//! pdfi_signature —— S2-7（issue #116 任务 2）：数字签名读取验收面。
//! #308 增补：摘要一致性（digest_matches）验收。
//!
//! 夹具：`fixtures/signed_sample.pdf` —— **真实结构的签名 PDF**：
//! pyhanko 生成，真 PKCS#7/CMS SignedData + 真 X.509 证书，
//! SubFilter = `ETSI.CAdES.detached`（PAdES 基线）。
//!
//! ⚠️ 证书是**自签**的（本机无真实 CA 签名样本，如实登记）。
//! 结构面与真实签名完全一致；证书链验证本就不在v1范围。
//!
//! 交叉验证：证书主体/签发者/序列号已用 pypdf + asn1crypto 独立读出并比对
//! （见 PR 记录）：subject CN=投标人签字人, O=某建设工程有限公司, C=CN,
//! serial=0x1234567890abcdef, ByteRange=[0, 14228, 18720, 608]。
//!
//! #308 的语义边界（钉死）：digest_matches 说的是「签名字节有没有被改」，
//! **不是**「证书可信」——三层验签模型（结构/摘要/信任链）见
//! signature.rs 模块文档。摘要一致性的判别力（Some(true)/Some(false)
//! 分化）已用 pyhanko 独立验证：未改动档 intact=True、覆盖段篡改
//! intact=False（#303 关闭报告的双向证据，同款方法）。

use std::path::PathBuf;

use mdc_pdf::signature::{read_signatures, SignatureError};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn signed_pdf() -> Vec<u8> {
    std::fs::read(fixtures().join("signed_sample.pdf")).expect("signed_sample.pdf 缺失")
}

// ---------------------------------------------------------------- 读签名

#[test]
fn reads_real_signature_from_signed_pdf() {
    let sigs = read_signatures(&signed_pdf()).unwrap();
    assert_eq!(sigs.len(), 1, "应读出 1 个签名域");
    let s = &sigs[0];

    assert_eq!(s.field_name, "Signature1");
    assert!(s.structurally_valid, "结构应完整：{:?}", s.structural_issue);
    assert_eq!(s.sub_filter, "ETSI.CAdES.detached");
    assert_eq!(s.reason.as_deref(), Some("投标文件签署"));
    assert_eq!(s.location.as_deref(), Some("北京市"));
    assert_eq!(s.digest_algorithm.as_deref(), Some("sha256"));
}

#[test]
fn extracts_certificate_details() {
    let sigs = read_signatures(&signed_pdf()).unwrap();
    let s = &sigs[0];
    assert_eq!(s.certificates.len(), 1, "CMS 内嵌 1 张证书");
    let c = &s.certificates[0];
    assert!(c.subject.contains("投标人签字人"), "subject: {}", c.subject);
    assert!(
        c.subject.contains("某建设工程有限公司"),
        "subject: {}",
        c.subject
    );
    assert!(c.issuer.contains("投标人签字人"), "issuer: {}", c.issuer);
    assert_eq!(c.serial, "1234567890abcdef", "serial: {}", c.serial);
    assert!(c.not_before.is_some(), "notBefore 应可解析");
    assert!(c.not_after.is_some(), "notAfter 应可解析");
    assert!(
        c.not_before.as_deref().unwrap().starts_with("2026-01-01"),
        "{:?}",
        c.not_before
    );
}

#[test]
fn signer_name_falls_back_to_cert_cn() {
    let sigs = read_signatures(&signed_pdf()).unwrap();
    let s = &sigs[0];
    // /Name 未设时取证书 CN
    assert!(s.signer_name.is_some(), "应能取到签名人");
    assert!(
        s.signer_name.as_deref().unwrap().contains("投标人签字人"),
        "{:?}",
        s.signer_name
    );
}

#[test]
fn byte_range_is_even_and_ordered() {
    let sigs = read_signatures(&signed_pdf()).unwrap();
    let br = &sigs[0].byte_range;
    assert_eq!(
        br.len(),
        4,
        "ByteRange 应为 [start1 len1 start2 len2]：{br:?}"
    );
    assert_eq!(br[0], 0, "第一段应从 0 开始");
    assert!(br[1] > 0 && br[2] > br[1], "段应递增：{br:?}");
    assert!(br[3] > 0, "第二段长度非零：{br:?}");
}

#[test]
fn signature_algorithm_is_reported() {
    let sigs = read_signatures(&signed_pdf()).unwrap();
    let alg = sigs[0].signature_algorithm.as_deref().unwrap_or("");
    assert!(!alg.is_empty(), "应报出签名算法");
    // pyhanko PAdES + RSA => sha256_rsa 或 rsassa 系
    assert!(alg.contains("sha") || alg.contains("rsa"), "算法名：{alg}");
}

// ---------------------------------------------------------------- 无签名文档

#[test]
fn unsigned_pdf_returns_empty_vec() {
    let bytes = std::fs::read(fixtures().join("form_bid_letter.pdf")).unwrap();
    let sigs = read_signatures(&bytes).unwrap();
    assert!(sigs.is_empty(), "无签名文档应返回空（不是错误）：{sigs:?}");
}

#[test]
fn garbage_bytes_is_typed_error() {
    let err = read_signatures(b"not a pdf").unwrap_err();
    assert!(matches!(err, SignatureError::Open(_)), "{err:?}");
}

// ---------------------------------------------------------------- #308 摘要一致性

/// 验收 1：未改动的已签名 PDF → digest_matches = Some(true)。
#[test]
fn digest_matches_true_on_untouched_signed_pdf() {
    let sigs = read_signatures(&signed_pdf()).unwrap();
    let s = &sigs[0];
    assert!(s.structurally_valid, "前置：结构完整");
    assert_eq!(
        s.digest_matches,
        Some(true),
        "未改动档摘要应一致（不等于证书可信——自签夹具）"
    );
}

/// 验收 2：改动后的已签名 PDF → digest_matches = Some(false)。
///
/// 篡改点取第一覆盖段中点（byte_range[1] / 2）——落在对象/流数据区
/// （rev0 的 xref 表在其尾部，远离中点），单字节翻转不破坏 lopdf 装载，
/// 但改变签名摘要输入。
#[test]
fn digest_matches_false_on_tampered_covered_byte() {
    let original = signed_pdf();
    let sigs = read_signatures(&original).unwrap();
    let br = &sigs[0].byte_range;
    assert!(br.len() >= 4, "前置：两段式 ByteRange：{br:?}");

    let pos = br[1] / 2; // 第一段中点，必在覆盖段内
    assert!(pos < br[0] + br[1], "篡改点应在覆盖段内：pos={pos}");

    let mut tampered = original.clone();
    tampered[pos] ^= 0xFF;
    let sigs2 = read_signatures(&tampered).expect("单字节翻转不应破坏结构装载（否则测试环境异常）");
    assert!(
        sigs2[0].structurally_valid,
        "结构面应不受影响（翻转点在流数据区）：{:?}",
        sigs2[0].structural_issue
    );
    assert_eq!(
        sigs2[0].digest_matches,
        Some(false),
        "覆盖段被改 → 摘要不匹配 → 「签完后被改过」应被检出"
    );
}

/// 验收 3（增量语义互锁）：追加字节（#227 增量写形态）→ 仍 Some(true)。
///
/// ByteRange 覆盖段未变 → 摘要不变。这与 pdfl_forms_incremental 的
/// 「填表保存 → 签名存活」是同一枚硬币的两面：增量追加合法、覆盖段
/// 篡改非法——digest_matches 把两者的分界线变成可断言的机器判据。
#[test]
fn digest_matches_true_after_incremental_append() {
    let original = signed_pdf();
    // 模拟增量修订的追加形态（EOF 后追加——覆盖段字节逐位不变）
    let mut appended = original.clone();
    appended.extend_from_slice(b"\n% appended revision marker (digest test)\n%%EOF\n");
    let sigs = read_signatures(&appended).expect("EOF 后追加注释性内容不应破坏装载");
    assert_eq!(
        sigs[0].digest_matches,
        Some(true),
        "追加段不改变覆盖段字节——摘要应仍一致（增量写合法形态）"
    );
}

/// 验收（API 兼容）：旧序列化形态（无 digest_matches 字段）反序列化 → None。
///
/// SignatureInfo 是 shell/wasm 消费面的序列化边界——新增字段带
/// `#[serde(default)]`，旧 JSON 必须能无损读回（None = 未验证，不是 false）。
#[test]
fn signature_info_serde_backward_compat() {
    let old_json = r#"{
        "field_name": "Signature1",
        "signer_name": null,
        "sign_time": null,
        "reason": null,
        "location": null,
        "sub_filter": "adbe.pkcs7.detached",
        "byte_range": [0, 10, 20, 5],
        "structurally_valid": true,
        "structural_issue": null,
        "certificates": [],
        "digest_algorithm": "sha256",
        "signature_algorithm": "sha256_rsa"
    }"#;
    let info: mdc_pdf::signature::SignatureInfo =
        serde_json::from_str(old_json).expect("旧形态 JSON（无 digest_matches）应可反序列化");
    assert_eq!(
        info.digest_matches, None,
        "缺字段 → None（未验证），不是 false"
    );
    // 往返：新形态序列化 → 反序列化保值
    let round: mdc_pdf::signature::SignatureInfo =
        serde_json::from_str(&serde_json::to_string(&info).unwrap()).unwrap();
    assert_eq!(round, info);
}
