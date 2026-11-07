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
//!
//! #308 摘要一致性（验签第②层）：夹具的 messageDigest 基线已用 Python
//! 独立复算比对（sha256 两段拼接 == CMS signedAttrs 基线，见 PR 记录）
//! ——Rust 侧 Some(true) 与独立实现互证。

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

// ---------------------------------------------------------------- #308 摘要一致性

/// 验收 1：未改动的已签名 PDF → digest_matches = Some(true)。
/// 同时锁 serde 往返（新字段 #[serde(default)] 的 wire 向后兼容面）。
#[test]
fn digest_matches_on_pristine_signed_pdf() {
    let sigs = read_signatures(&signed_pdf()).unwrap();
    let s = &sigs[0];
    assert_eq!(
        s.digest_matches,
        Some(true),
        "未改动的签名文件：签名覆盖字节与签时基线一致（issue 验收 1）"
    );
    assert!(
        s.digest_issue.is_none(),
        "匹配时不应有诊断：{:?}",
        s.digest_issue
    );

    // serde 往返：新增字段不破坏既有 wire 面（旧 JSON 反序列化 → None 默认）。
    let json = serde_json::to_string(s).unwrap();
    let back: mdc_pdf::signature::SignatureInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.digest_matches, Some(true), "serde 往返保持摘要判定");
    // 旧形态 JSON（无新字段）可反序列化（#[serde(default)] 兼容面）。
    let legacy = serde_json::json!({
        "field_name": "Signature1",
        "signer_name": null,
        "sign_time": null,
        "reason": null,
        "location": null,
        "sub_filter": "ETSI.CAdES.detached",
        "byte_range": [0, 14228, 18720, 608],
        "structurally_valid": true,
        "structural_issue": null,
        "certificates": [],
        "digest_algorithm": "sha256",
        "signature_algorithm": null
    });
    let legacy: mdc_pdf::signature::SignatureInfo =
        serde_json::from_value(legacy).expect("旧 wire 形态应可反序列化（serde(default)）");
    assert_eq!(legacy.digest_matches, None, "无字段的旧形态 → None 默认");
}

/// 验收 2 + 4：改动后的已签名 PDF → digest_matches = Some(false)。
///
/// 篡改点选 range 1 内的**字段名字符串值**（`(project_name)` 的字符，
/// 偏移 ~3167）：PDF 结构保持可解析（lopdf 正常装载，structural_valid
/// 仍为 true）但摘要必变——这正是本层要抓的「结构完好但字节已改」
/// 隐蔽形态（全量重写式篡改连结构检查都骗不过，字符串值篡改骗得过）。
#[test]
fn digest_mismatch_detected_after_tampering() {
    let pristine = signed_pdf();
    let sigs = read_signatures(&pristine).unwrap();
    let br = sigs[0].byte_range.clone();
    assert_eq!(br.len(), 4);

    // 篡改：range 1 内的字段名字符串（project_name → project_namX）。
    let needle = b"(project_name)";
    let pos = pristine
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("夹具 range 1 内应有字段名字符串");
    assert!(
        pos + needle.len() <= br[0] + br[1],
        "篡改点必须在签名覆盖段内（pos {pos} vs range1 终点 {}）",
        br[0] + br[1]
    );
    let mut tampered = pristine.clone();
    // 字符串值内翻转（'e' → 'E'，仍在合法 PDF 字符串字符集内）。
    tampered[pos + needle.len() - 2] = b'E';
    assert_ne!(tampered, pristine, "篡改必须真的改了字节");

    let sigs = read_signatures(&tampered).unwrap();
    let s = &sigs[0];
    // 结构仍完整（结构层抓不住这种篡改——第①层的盲区即第②层的存在意义）。
    assert!(
        s.structurally_valid,
        "字符串值篡改不破坏结构（结构面的盲区）：{:?}",
        s.structural_issue
    );
    // 摘要必不匹配（验签第②层的主力断言）。
    assert_eq!(
        s.digest_matches,
        Some(false),
        "签名覆盖字节已改 → 摘要不匹配（issue 验收 2）"
    );
    let issue = s.digest_issue.as_deref().unwrap_or("");
    assert!(issue.contains("改动"), "诊断应说明字节被改：{issue}");
}

/// 摘要覆盖域诚实面：签名只盖 ByteRange 盖住的字节——范围外的追加
/// （如增量修订）不位判定为篡改。篡改点选在文件末尾**之后**追加一个
/// 字节（模拟增量修订追加形态），digest 仍 Some(true)。
#[test]
fn digest_covers_only_byte_range() {
    let pristine = signed_pdf();
    let sigs = read_signatures(&pristine).unwrap();
    let br = sigs[0].byte_range.clone();
    let covered_end = br[2] + br[3];
    assert_eq!(
        covered_end,
        pristine.len(),
        "前置钉定：夹具 range 2 盖到文件末尾（追加才在覆盖域外）"
    );

    // 覆盖域外追加（增量修订形态：旧签名对其 ByteRange 仍有效）。
    let mut appended = pristine.clone();
    appended.extend_from_slice(b"\n%appended-after-signature\n");
    let sigs = read_signatures(&appended).unwrap();
    assert_eq!(
        sigs[0].digest_matches,
        Some(true),
        "覆盖域外的追加不影响旧签名（PAdES 增量语义——签名只盖自己的 ByteRange）"
    );
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
