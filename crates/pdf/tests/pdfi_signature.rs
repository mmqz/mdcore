//! pdfi_signature —— S2-7（issue #116 任务 2）：数字签名读取验收面。
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
