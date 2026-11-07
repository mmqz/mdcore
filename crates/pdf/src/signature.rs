//! S2-7（issue #116 任务 2）· 数字签名**读取/结构验证**面。
//!
//! # 范围
//!
//! - 读 PDF 的 `/Sig` 域（`/V` → 签名字典）；
//! - `/Contents` 是**十六进制编码的 DER**，解出 PKCS#7/CMS `SignedData`；
//! - 遍历 CMS 结构，报出签名人/时间/原因/位置/子过滤器/字节范围/证书；
//! - 判定「结构是否完整」（**不验证证书链、不验时间戳**——那需要信任根，
//!   且第一版范围明确不含）；
//! - **#308 起补「摘要一致性」层**：CMS `signedAttrs` 的 `messageDigest`
//!   属性与 `/ByteRange` 覆盖段实际字节的摘要比对（`digest_matches`）。
//!   这回答用户最关心的问题：「这份文件签了之后有没有被改过？」
//!
//! # 验签的三层模型（#308 口径）
//!
//! ```text
//! ① 结构完整（ByteRange 合法 + CMS 可解析）   ← structurally_valid（既有）
//! ② 摘要一致（签的是这些字节吗）              ← digest_matches（本次新增）
//! ③ 信任链（证书可信吗）                      ← 需要信任根，明确不做
//! ```
//!
//! ② 不需要信任根——只是「把 ByteRange 各段字节算摘要，和 CMS
//! `SignedData` 里 signerInfo.signedAttrs 的 `messageDigest` 比对」。
//! 这是与 Acrobat 验签的最小差距层（研究仓 §6 定性）。
//!
//! **digest_matches 的语义边界（必须说清）**：
//!
//! - `Some(true)`：签名字节未被改动（摘要一致）。**不等于证书可信**——
//!   签名人身份、证书链、时间戳均不在验证范围；
//! - `Some(false)`：覆盖段被改动，签名已失效——「签完后被改过」的密码学证据；
//! - `None`：无法验证，诚实报告不猜——成因：无 `signedAttrs`（旧式直签
//!   形态，明文摘要不在 CMS 里）、摘要算法不支持（本版仅 sha256/384/512，
//!   sha1/md5 出于弱算法与零新增依赖纪律不背书）、`ByteRange` 越界或
//!   非偶数元、CMS 解析失败（此时 `structurally_valid` 也为假）。
//!
//! # 为什么手写 DER（零依赖）
//!
//! `der-parser`/`asn1-rs` 虽是纯 Rust，但引入它们会让本 crate 多一条
//! 依赖链（且构建机在本轮无法访问 crates.io）。DER 是**自描述的三段式
//! 编码**（Tag-Length-Value），读 CMS 所需的子集很小——与 [`crate::stamp`]
//! 手写 zlib inflate 同一条纪律：能自己写清楚的，不引依赖。
//!
//! 支持范围：BERTLV 的 **DER 形态**（定长编码）。CMS 签名规范要求 DER，
//! 真实 PDF 签名（adbe.pkcs7.detached / ETSI.CAdES.detached）都是 DER。
//! 不定长（BER indeterminate）只在极老的实现里出现——遇到就如实拒绝，
//! 不猜。

use serde::{Deserialize, Serialize};

use crate::forms::dict_text;

// ---------------------------------------------------------------------------
// 公开类型
// ---------------------------------------------------------------------------

/// 证书摘要信息（能提取多少算多少——不猜）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertInfo {
    /// 主体（`CN=xxx, O=xxx, C=CN` 形式的可读串）。
    pub subject: String,
    /// 签发者（同上）。
    pub issuer: String,
    /// 序列号（十六进制，小写，去前导 0）。
    pub serial: String,
    /// 生效时间（ISO 风格可读串，原文照录不做时区换算）。
    pub not_before: Option<String>,
    /// 失效时间。
    pub not_after: Option<String>,
}

/// PDF 签名信息（读面）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureInfo {
    /// 签名域名称（/T）。
    pub field_name: String,
    /// 签名人（/Name 优先；无则取证书 subject 的 CN）。
    pub signer_name: Option<String>,
    /// 签名时间（/M 原样——PDF 日期串 `D:YYYYMMDDHHmmSS+HH'mm'`）。
    pub sign_time: Option<String>,
    /// 签名原因（/Reason）。
    pub reason: Option<String>,
    /// 位置（/Location）。
    pub location: Option<String>,
    /// 子过滤器（`adbe.pkcs7.detached` / `ETSI.CAdES.detached` / …）。
    pub sub_filter: String,
    /// 字节范围：`[start1, len1, start2, len2]`（**偶数个**；见 spec 12.8.1）。
    pub byte_range: Vec<usize>,
    /// CMS 结构是否完整（解析成功 + 必要字段在位）。
    ///
    /// **不表示签名有效**——证书链/时间戳/内容比对都不在本版范围。
    pub structurally_valid: bool,
    /// 结构不完整时的原因（人类可读）。
    pub structural_issue: Option<String>,
    /// 证书清单（CMS 内嵌的证书，按出现序）。
    pub certificates: Vec<CertInfo>,
    /// 摘要算法名（`sha256` 等；无则 None）。
    pub digest_algorithm: Option<String>,
    /// 签名算法名（`rsassa_pkcs1v15` / `sha256_rsa` 等，取 CMS 里的 OID 名）。
    pub signature_algorithm: Option<String>,
    /// **摘要一致性**（#308）：CMS `signedAttrs` 的 `messageDigest` 与
    /// `/ByteRange` 覆盖段实际字节算出的摘要是否一致。
    ///
    /// - `Some(true)`：签名字节未被改动（摘要一致）——**不等于证书可信**，
    ///   证书链/时间戳验证不在本版范围（见模块文档「三层模型」）；
    /// - `Some(false)`：覆盖段被改动——「这份文件签完后被改过」的密码学证据；
    /// - `None`：无法验证（无 `signedAttrs`/`messageDigest`、算法不支持
    ///   （本版仅 sha256/384/512）、`ByteRange` 越界或非偶数元、CMS 解析
    ///   失败）——诚实报告，不猜。
    ///
    /// 多签名人档只验第一个 signerInfo（与字段提取口径一致）。
    #[serde(default)]
    pub digest_matches: Option<bool>,
}

/// 签名读取错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// PDF 装载失败。
    Open(String),
    /// 文档加密（读签名字典无意义——S5 同款纪律）。
    Encrypted,
    /// DER 结构解析失败（定位到偏移与原因）。
    Der { offset: usize, reason: String },
    /// CMS 结构不合规（不是 SignedData、缺 signerInfos 等）。
    Cms(String),
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignatureError::Open(e) => write!(f, "PDF 装载失败：{e}"),
            SignatureError::Encrypted => write!(f, "文档已加密（签名读取被拒）"),
            SignatureError::Der { offset, reason } => {
                write!(f, "DER 解析失败（偏移 {offset}）：{reason}")
            }
            SignatureError::Cms(e) => write!(f, "CMS 结构不合规：{e}"),
        }
    }
}

impl std::error::Error for SignatureError {}

// ---------------------------------------------------------------------------
// DER / BER 最小解析器
// ---------------------------------------------------------------------------

/// 一个 TLV 节点（value 是相对 `data` 的切片）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tlv<'a> {
    /// 单字节 tag（本模块只用低 tag 号 < 31 的通用类型）。
    pub tag: u8,
    /// 内容字节。
    pub value: &'a [u8],
    /// 该 TLV 在输入里的起始偏移。
    pub offset: usize,
    /// 该 TLV 的**总长度**（含 tag+length 头）。
    pub total_len: usize,
}

impl<'a> Tlv<'a> {
    /// 内容里逐个子 TLV（仅对 constructed 有意义）。
    pub fn children(&self) -> DerIter<'_> {
        DerIter {
            data: self.value,
            pos: 0,
            base: self.offset + (self.total_len - self.value.len()),
        }
    }
}

/// DER 顺序迭代器（宽容：坏数据返回 Err 即停）。
pub(crate) struct DerIter<'a> {
    data: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Iterator for DerIter<'a> {
    type Item = Result<Tlv<'a>, SignatureError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.data.len() {
            return None;
        }
        match parse_tlv(&self.data[self.pos..], self.base + self.pos) {
            Ok(t) => {
                self.pos += t.total_len;
                Some(Ok(t))
            }
            Err(e) => {
                self.pos = self.data.len(); // 出错后终止
                Some(Err(e))
            }
        }
    }
}

/// 解析单个 TLV。
pub(crate) fn parse_tlv(data: &[u8], offset: usize) -> Result<Tlv<'_>, SignatureError> {
    let bad = |o: usize, r: &str| SignatureError::Der {
        offset: o,
        reason: r.to_string(),
    };
    if data.is_empty() {
        return Err(bad(offset, "数据为空"));
    }
    let tag = data[0];
    if tag & 0x1F == 0x1F {
        return Err(bad(offset, "多字节 tag（高 tag 号）不在支持范围"));
    }
    if data.len() < 2 {
        return Err(bad(offset, "长度字节缺失"));
    }
    let first = data[1];
    // tag + length 头长度
    let (hdr, len) = if first & 0x80 == 0 {
        (2usize, first as usize)
    } else {
        let n = (first & 0x7F) as usize;
        if n == 0 {
            return Err(bad(
                offset,
                "不定长编码（BER indeterminate）——CMS 应为 DER 定长",
            ));
        }
        if n > 4 {
            return Err(bad(offset, "长度字段超过 4 字节（可疑）"));
        }
        if data.len() < 2 + n {
            return Err(bad(offset, "长度字段截断"));
        }
        let mut v = 0usize;
        for b in &data[2..2 + n] {
            v = (v << 8) | (*b as usize);
        }
        (2 + n, v)
    };
    let end = hdr
        .checked_add(len)
        .ok_or_else(|| bad(offset, "长度溢出"))?;
    if end > data.len() {
        return Err(bad(
            offset,
            &format!("内容越界（需要 {len} 字节，实有 {}）", data.len() - hdr),
        ));
    }
    Ok(Tlv {
        tag,
        value: &data[hdr..end],
        offset,
        total_len: end,
    })
}

/// DER 通用 tag 常量。
pub(crate) mod tags {
    pub const INTEGER: u8 = 0x02;
    pub const OID: u8 = 0x06;
    pub const UTF8_STRING: u8 = 0x0C;
    pub const SEQUENCE: u8 = 0x30;
    pub const SET: u8 = 0x31;
    pub const PRINTABLE_STRING: u8 = 0x13;
    pub const T61_STRING: u8 = 0x14;
    pub const IA5_STRING: u8 = 0x16;
    pub const UTC_TIME: u8 = 0x17;
    pub const GENERALIZED_TIME: u8 = 0x18;
    pub const BMP_STRING: u8 = 0x1E;
}

/// OID 字节 → 点分字符串（`1.2.840.113549.1.7.2`）。
pub(crate) fn oid_to_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let first = bytes[0] as u32;
    out.push_str(&format!("{}.{}", first / 40, first % 40));
    let mut acc: u64 = 0;
    for &b in &bytes[1..] {
        acc = (acc << 7) | ((b & 0x7F) as u64);
        if b & 0x80 == 0 {
            out.push_str(&format!(".{acc}"));
            acc = 0;
        }
    }
    out
}

/// 字符串类型解码（UTF8 / Printable / IA5 / BMP / T61）。
pub(crate) fn der_string(tag: u8, value: &[u8]) -> Option<String> {
    use tags::*;
    match tag {
        UTF8_STRING => Some(String::from_utf8_lossy(value).into_owned()),
        PRINTABLE_STRING | IA5_STRING => Some(String::from_utf8_lossy(value).into_owned()),
        T61_STRING => Some(value.iter().map(|&b| b as char).collect()),
        BMP_STRING => {
            let units: Vec<u16> = value
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes(*c))
                .collect();
            Some(String::from_utf16_lossy(&units))
        }
        _ => None,
    }
}

/// INTEGER → 十六进制串（去前导 0）。
pub(crate) fn integer_hex(value: &[u8]) -> String {
    let v = value
        .iter()
        .skip_while(|&&b| b == 0)
        .copied()
        .collect::<Vec<_>>();
    if v.is_empty() {
        return "0".to_string();
    }
    v.iter().map(|b| format!("{b:02x}")).collect()
}
// ---------------------------------------------------------------------------
// X.509 证书摘要
// ---------------------------------------------------------------------------

/// 解析一张 DER 证书的摘要（够报告用——不求全）。
pub(crate) fn parse_certificate(der: &[u8]) -> Option<CertInfo> {
    let cert = parse_tlv(der, 0).ok()?;
    if cert.tag != tags::SEQUENCE {
        return None;
    }
    let mut it = cert.children();
    let tbs = it.next()?.ok()?;
    if tbs.tag != tags::SEQUENCE {
        return None;
    }
    let mut t = tbs.children();
    // [0] version（可选）
    let mut first = t.next()?.ok()?;
    if first.tag == 0xA0 {
        first = t.next()?.ok()?;
    }
    // serialNumber
    let serial = if first.tag == tags::INTEGER {
        integer_hex(first.value)
    } else {
        return None;
    };
    // signature algorithm（跳过）
    let _ = t.next()?;
    // issuer
    let issuer = t.next()?.ok()?;
    // validity
    let validity = t.next()?.ok()?;
    // subject
    let subject = t.next()?.ok()?;

    let not_before = validity
        .children()
        .next()
        .and_then(|r| r.ok())
        .map(|x| der_time(&x));
    let not_after = validity
        .children()
        .nth(1)
        .and_then(|r| r.ok())
        .map(|x| der_time(&x));

    Some(CertInfo {
        subject: name_to_string(&subject),
        issuer: name_to_string(&issuer),
        serial,
        not_before,
        not_after,
    })
}

/// Name（RDNSequence）→ 可读串：`CN=xxx, O=xxx, C=CN`。
fn name_to_string(name: &Tlv<'_>) -> String {
    if name.tag != tags::SEQUENCE {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    for rdn in name.children().flatten() {
        for atv in rdn.children().flatten() {
            let mut a = atv.children();
            let Some(Ok(oid)) = a.next() else { continue };
            let Some(Ok(val)) = a.next() else { continue };
            if oid.tag != tags::OID {
                continue;
            }
            let oid_str = oid_to_string(oid.value);
            let label = match oid_str.as_str() {
                "2.5.4.3" => "CN",
                "2.5.4.6" => "C",
                "2.5.4.7" => "L",
                "2.5.4.8" => "ST",
                "2.5.4.10" => "O",
                "2.5.4.11" => "OU",
                other => other,
            };
            if let Some(s) = der_string(val.tag, val.value) {
                parts.push(format!("{label}={s}"));
            }
        }
    }
    // 可读序：CN 打头（与 Acrobat 的显示口径一致）
    parts.sort_by_key(|p| if p.starts_with("CN=") { 0 } else { 1 });
    parts.join(", ")
}

/// UTCTime / GeneralizedTime → 可读串（原文照录，不做时区换算）。
fn der_time(t: &Tlv<'_>) -> String {
    let raw = String::from_utf8_lossy(t.value).into_owned();
    match t.tag {
        tags::UTC_TIME if raw.len() >= 12 => {
            // YYMMDDHHMMSS(Z|±hhmm) → 20YY-MM-DD HH:MM:SS
            let y = &raw[0..2];
            let yy: u32 = y.parse().unwrap_or(0);
            let century = if yy >= 50 { "19" } else { "20" };
            format!(
                "{century}{}-{}-{} {}:{}:{}",
                &raw[0..2],
                &raw[2..4],
                &raw[4..6],
                &raw[6..8],
                &raw[8..10],
                &raw[10..12]
            )
        }
        tags::GENERALIZED_TIME if raw.len() >= 14 => format!(
            "{}-{}-{} {}:{}:{}",
            &raw[0..4],
            &raw[4..6],
            &raw[6..8],
            &raw[8..10],
            &raw[10..12],
            &raw[12..14]
        ),
        _ => raw,
    }
}

// ---------------------------------------------------------------------------
// CMS SignedData 解析
// ---------------------------------------------------------------------------

/// CMS 解析结果（内部面）。
#[derive(Debug, Default)]
pub(crate) struct CmsSummary {
    pub digest_algorithm: Option<String>,
    pub signature_algorithm: Option<String>,
    /// signerInfo 级摘要算法名（messageDigest 用的就是它；与
    /// SignedData.digestAlgorithms 集合级不同——验证以 signer 级为准，
    /// 缺失时回退集合级）。
    pub signer_digest_algorithm: Option<String>,
    /// `signedAttrs` 里 `messageDigest` 属性的值（OID
    /// 1.2.840.113549.1.9.4）——签名时对内容（ByteRange 覆盖段）算的摘要。
    /// 无 `signedAttrs` 或缺该属性时为 None（旧式直签形态——明文摘要
    /// 不在 CMS 里，本层无法验证）。
    pub message_digest: Option<Vec<u8>>,
    pub certificates: Vec<CertInfo>,
    pub signer_cn: Option<String>,
    /// 结构问题（None = 结构完整）。
    pub issue: Option<String>,
}

/// OID → 算法可读名（只列常见项，其余报文里出点分串）。
pub(crate) fn algorithm_name(oid: &str) -> String {
    match oid {
        "1.2.840.113549.2.5" => "md5".into(),
        "1.3.14.3.2.26" => "sha1".into(),
        "2.16.840.1.101.3.4.2.1" => "sha256".into(),
        "2.16.840.1.101.3.4.2.2" => "sha384".into(),
        "2.16.840.1.101.3.4.2.3" => "sha512".into(),
        "1.2.840.113549.1.1.1" => "rsa".into(),
        "1.2.840.113549.1.1.5" => "sha1_rsa".into(),
        "1.2.840.113549.1.1.11" => "sha256_rsa".into(),
        "1.2.840.113549.1.1.12" => "sha384_rsa".into(),
        "1.2.840.113549.1.1.13" => "sha512_rsa".into(),
        "1.2.840.10045.4.3.2" => "sha256_ecdsa".into(),
        "1.2.840.10045.4.3.3" => "sha384_ecdsa".into(),
        "1.2.840.113549.1.9.16.2.47" => "signing_certificate_v2".into(),
        "1.2.840.113549.1.9.3" => "content_type".into(),
        "1.2.840.113549.1.9.4" => "message_digest".into(),
        "1.2.840.113549.1.9.5" => "signing_time".into(),
        "1.2.840.113549.1.9.16.2.14" => "signature_time_stamp".into(),
        other => other.to_string(),
    }
}

/// CMS `ContentInfo` → `SignedData` 摘要。
///
/// ```text
/// ContentInfo ::= SEQUENCE { contentType OID, content [0] EXPLICIT ANY }
/// SignedData  ::= SEQUENCE {
///     version INTEGER, digestAlgorithms SET OF AlgorithmIdentifier,
///     encapContentInfo SEQUENCE, certificates [0] IMPLICIT SET OF Certificate OPTIONAL,
///     crls [1] IMPLICIT OPTIONAL, signerInfos SET OF SignerInfo }
/// ```
pub(crate) fn parse_signed_data(der: &[u8]) -> Result<CmsSummary, SignatureError> {
    const OID_SIGNED_DATA: &str = "1.2.840.113549.1.7.2";

    let ci = parse_tlv(der, 0)?;
    if ci.tag != tags::SEQUENCE {
        return Err(SignatureError::Cms("ContentInfo 不是 SEQUENCE".into()));
    }
    let mut ci_it = ci.children();
    let ct = ci_it
        .next()
        .ok_or_else(|| SignatureError::Cms("ContentInfo 缺 contentType".into()))??;
    if ct.tag != tags::OID {
        return Err(SignatureError::Cms("contentType 不是 OID".into()));
    }
    let ct_oid = oid_to_string(ct.value);
    if ct_oid != OID_SIGNED_DATA {
        return Err(SignatureError::Cms(format!(
            "不是 SignedData（contentType={ct_oid}）"
        )));
    }
    let content = ci_it
        .next()
        .ok_or_else(|| SignatureError::Cms("ContentInfo 缺 content（签名内容为空）".into()))??;
    // [0] EXPLICIT → 里面才是 SignedData
    let sd = content
        .children()
        .next()
        .ok_or_else(|| SignatureError::Cms("[0] 包装为空".into()))??;
    if sd.tag != tags::SEQUENCE {
        return Err(SignatureError::Cms("SignedData 不是 SEQUENCE".into()));
    }

    let mut out = CmsSummary::default();
    let mut it = sd.children();
    // version
    it.next();
    // digestAlgorithms
    if let Some(Ok(das)) = it.next() {
        if let Some(Ok(alg)) = das.children().next() {
            if let Some(Ok(oid)) = alg.children().next() {
                if oid.tag == tags::OID {
                    out.digest_algorithm = Some(algorithm_name(&oid_to_string(oid.value)));
                }
            }
        }
    }
    // encapContentInfo（跳过）
    it.next();

    let mut signer_infos: Option<Tlv<'_>> = None;
    for item in it {
        let Ok(node) = item else { continue };
        match node.tag {
            0xA0 => {
                // certificates [0] IMPLICIT SET OF Certificate
                for c in node.children().flatten() {
                    if c.tag == tags::SEQUENCE {
                        if let Some(ci) = parse_certificate(&der[c.offset..c.offset + c.total_len])
                        {
                            out.certificates.push(ci);
                        }
                    }
                }
            }
            0xA1 => {} // crls
            tags::SET => signer_infos = Some(node),
            _ => {}
        }
    }

    let Some(sis) = signer_infos else {
        out.issue = Some("缺 signerInfos（CMS 结构不完整）".into());
        return Ok(out);
    };
    let Some(Ok(si)) = sis.children().next() else {
        out.issue = Some("signerInfos 为空".into());
        return Ok(out);
    };

    // SignerInfo ::= SEQUENCE { version, sid, digestAlgorithm,
    //                           signedAttrs [0] OPTIONAL, signatureAlgorithm,
    //                           signature OCTET STRING, unsignedAttrs [1] OPTIONAL }
    let mut s = si.children();
    s.next(); // version
    let sid = s.next().and_then(|r| r.ok());
    // signerInfo 级 digestAlgorithm（messageDigest 实际用的算法——#308
    // 验证以此为准，与 SignedData.digestAlgorithms 集合级可能有差）
    let signer_digest_alg = s.next().and_then(|r| r.ok());
    if let Some(alg) = &signer_digest_alg {
        if let Some(Ok(oid)) = alg.children().next() {
            if oid.tag == tags::OID {
                out.signer_digest_algorithm = Some(algorithm_name(&oid_to_string(oid.value)));
            }
        }
    }
    let mut next = s.next().and_then(|r| r.ok());
    let mut signed_attrs: Option<Tlv<'_>> = None;
    if let Some(n) = next {
        if n.tag == 0xA0 {
            // signedAttrs [0] IMPLICIT SET OF Attribute——#308：不再跳过，
            // 提取 messageDigest（OID 1.2.840.113549.1.9.4）。
            signed_attrs = Some(n);
            next = s.next().and_then(|r| r.ok()); // signatureAlgorithm
        }
    }
    if let Some(attrs) = signed_attrs {
        // Attribute ::= SEQUENCE { attrType OID, attrValues SET OF ANY }
        const OID_MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
        for attr in attrs.children().flatten() {
            let mut a = attr.children();
            let (Some(Ok(oid)), Some(Ok(values))) = (a.next(), a.next()) else {
                continue;
            };
            if oid.tag != tags::OID || oid_to_string(oid.value) != OID_MESSAGE_DIGEST {
                continue;
            }
            // attrValues SET 里第一个元素是 OCTET STRING（摘要本身）
            if let Some(Ok(octet)) = values.children().next() {
                if octet.tag == 0x04 {
                    out.message_digest = Some(octet.value.to_vec());
                }
            }
            break; // 只取第一个 messageDigest（重复属非合规形态，不背书）
        }
    }
    if let Some(alg) = next {
        if let Some(Ok(oid)) = alg.children().next() {
            if oid.tag == tags::OID {
                out.signature_algorithm = Some(algorithm_name(&oid_to_string(oid.value)));
            }
        }
    }

    // 签名人：sid 若带 issuerAndSerial，按序列号匹配证书取 CN；
    // 否则（subjectKeyIdentifier）退回首张证书的 CN。
    let cn_of = |c: &CertInfo| -> String {
        c.subject
            .split(',')
            .find_map(|p| p.trim().strip_prefix("CN=").map(str::to_string))
            .unwrap_or_else(|| c.subject.clone())
    };
    let sid_serial: Option<String> = match sid {
        Some(sid) if sid.tag == tags::SEQUENCE => {
            let mut it2 = sid.children();
            let ias = it2
                .next()
                .and_then(|r| r.ok())
                .filter(|x| x.tag == tags::SEQUENCE);
            ias.and_then(|ias| {
                let serial = ias.children().flatten().last();
                serial
                    .filter(|ser| ser.tag == tags::INTEGER)
                    .map(|ser| integer_hex(ser.value))
            })
        }
        _ => None,
    };
    out.signer_cn = sid_serial
        .and_then(|hex| out.certificates.iter().find(|c| c.serial == hex).map(cn_of))
        .or_else(|| out.certificates.first().map(cn_of));
    Ok(out)
}
// ---------------------------------------------------------------------------
// PDF 层：读 /Sig 域
// ---------------------------------------------------------------------------

/// 读 PDF 里的全部签名（按域树出现序）。
///
/// 找不到任何 /Sig 域 → 返回空 vec（不是错误：多数文档本来就没有签名）。
///
/// # 摘要一致性（#308）
///
/// 本函数同时计算每个签名的 [`SignatureInfo::digest_matches`]（对
/// ByteRange 覆盖段算摘要与 CMS messageDigest 比对）——需要传入**完整
/// 原始字节**（不是装载后的重建形态：ByteRange 偏移只对原始字节有意义）。
pub fn read_signatures(bytes: &[u8]) -> Result<Vec<SignatureInfo>, SignatureError> {
    let doc = lopdf::Document::load_mem(bytes)
        .map_err(|e| SignatureError::Open(format!("lopdf 装载失败：{e}")))?;
    if doc.is_encrypted() {
        return Err(SignatureError::Encrypted);
    }

    let mut out = Vec::new();
    for (name, dict) in collect_sig_fields(&doc) {
        out.push(build_signature_info(&doc, &name, dict, bytes));
    }
    Ok(out)
}

/// 收集 `/FT /Sig` 的域字典（字段树 + 页 /Annots 双源，同 forms.rs 口径）。
fn collect_sig_fields(doc: &lopdf::Document) -> Vec<(String, lopdf::Dictionary)> {
    use lopdf::Object;
    let mut out: Vec<(String, lopdf::Dictionary)> = Vec::new();
    let mut seen: std::collections::BTreeSet<lopdf::ObjectId> = std::collections::BTreeSet::new();

    // 源一：/AcroForm/Fields
    let root = doc
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|o| o.as_reference().ok());
    if let Some(root) = root {
        if let Ok(catalog) = doc.get_dictionary(root) {
            let acro = match catalog.get(b"AcroForm").ok() {
                Some(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
                Some(Object::Dictionary(d)) => Some(d),
                _ => None,
            };
            if let Some(acro) = acro {
                if let Ok(fields) = acro.get(b"Fields").and_then(|o| o.as_array()) {
                    for f in fields {
                        if let Ok(id) = f.as_reference() {
                            visit_sig_field(doc, id, "", &mut out, &mut seen, 0);
                        }
                    }
                }
            }
        }
    }

    // 源二：页 /Annots（未登记进 /Fields 的签名控件）
    for (_n, page_id) in doc.get_pages() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Ok(annots) = page.get(b"Annots").and_then(|o| o.as_array()) else {
            continue;
        };
        for a in annots {
            if let Ok(id) = a.as_reference() {
                if seen.contains(&id) {
                    continue;
                }
                if let Ok(w) = doc.get_dictionary(id) {
                    let is_widget = w.get(b"Subtype").ok().and_then(|s| s.as_name().ok())
                        == Some(&b"Widget"[..]);
                    let is_sig =
                        w.get(b"FT").ok().and_then(|o| o.as_name().ok()) == Some(&b"Sig"[..]);
                    if is_widget && is_sig {
                        visit_sig_field(doc, id, "", &mut out, &mut seen, 0);
                    }
                }
            }
        }
    }
    out
}

/// 递归走域树，收集 `/FT /Sig` 的域（含 `/T` 全名）。
fn visit_sig_field(
    doc: &lopdf::Document,
    id: lopdf::ObjectId,
    prefix: &str,
    out: &mut Vec<(String, lopdf::Dictionary)>,
    seen: &mut std::collections::BTreeSet<lopdf::ObjectId>,
    depth: u32,
) {
    if depth > 64 || !seen.insert(id) {
        return;
    }
    let Ok(d) = doc.get_dictionary(id) else {
        return;
    };
    let local = d
        .get(b"T")
        .ok()
        .and_then(|o| o.as_str().ok())
        .map(crate::forms::decode_pdf_string)
        .unwrap_or_default();
    let full = if local.is_empty() {
        prefix.to_string()
    } else if prefix.is_empty() {
        local
    } else {
        format!("{prefix}.{local}")
    };
    let ft = d
        .get(b"FT")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n.to_vec());
    let kids: Vec<lopdf::ObjectId> = d
        .get(b"Kids")
        .ok()
        .and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(|k| k.as_reference().ok()).collect())
        .unwrap_or_default();
    if ft.as_deref() == Some(b"Sig") && !full.is_empty() {
        out.push((full, d.clone()));
    } else {
        for k in kids {
            visit_sig_field(doc, k, &full, out, seen, depth + 1);
        }
    }
}
/// 由签名域字典组装 [`SignatureInfo`]。`bytes` 是完整原始字节
/// （ByteRange 偏移的参照系——#308 摘要一致性用）。
fn build_signature_info(
    doc: &lopdf::Document,
    field_name: &str,
    dict: lopdf::Dictionary,
    bytes: &[u8],
) -> SignatureInfo {
    use lopdf::Object;

    // /V → 签名字典（可能是引用）
    let sig_dict: Option<lopdf::Dictionary> = match dict.get(b"V").ok() {
        Some(Object::Reference(id)) => doc.get_dictionary(*id).ok().cloned(),
        Some(Object::Dictionary(d)) => Some(d.clone()),
        _ => None,
    };

    let mut info = SignatureInfo {
        field_name: field_name.to_string(),
        signer_name: None,
        sign_time: None,
        reason: None,
        location: None,
        sub_filter: String::new(),
        byte_range: Vec::new(),
        structurally_valid: false,
        structural_issue: None,
        certificates: Vec::new(),
        digest_algorithm: None,
        signature_algorithm: None,
        digest_matches: None,
    };

    let Some(sd) = sig_dict else {
        info.structural_issue = Some("签名域未签署（/V 缺失或为空）".into());
        return info;
    };

    info.reason = dict_text(&sd, b"Reason");
    info.location = dict_text(&sd, b"Location");
    info.signer_name = dict_text(&sd, b"Name");
    info.sign_time = dict_text(&sd, b"M");
    // /SubFilter 是**名字对象**（/ETSI.CAdES.detached）——用 dict_text 兼容
    // 名字/字符串两形态（as_str 对名字静默返回 None，见 crate 文档「三形态」）
    info.sub_filter = dict_text(&sd, b"SubFilter").unwrap_or_default();

    // /ByteRange [start1 len1 start2 len2 ...]
    if let Ok(arr) = sd.get(b"ByteRange").and_then(|o| o.as_array()) {
        info.byte_range = arr
            .iter()
            .filter_map(|o| o.as_i64().ok().and_then(|v| usize::try_from(v).ok()))
            .collect();
    }

    // /Contents 是**十六进制串形态的 DER**（也可能带尖括号被 lopdf 当字符串）
    let contents = sd.get(b"Contents").ok().and_then(|o| o.as_str().ok());
    let Some(raw) = contents else {
        info.structural_issue = Some("缺少 /Contents（签名内容为空）".into());
        return info;
    };
    let der = strip_trailing_zeros(raw);
    if der.is_empty() {
        info.structural_issue = Some("/Contents 为空".into());
        return info;
    }

    match parse_signed_data(&der) {
        Ok(summary) => {
            // #308 摘要一致性：先于字段搬移计算（summary 后续会被部分 move）。
            // 结构完整才有比对的基础（CMS 解析失败时 messageDigest 本就
            // 拿不到，这里只是把语义钉死）。
            let digest_matches = if summary.issue.is_none() {
                verify_digest_matches(bytes, &info.byte_range, &summary)
            } else {
                None
            };
            info.digest_algorithm = summary.digest_algorithm;
            info.signature_algorithm = summary.signature_algorithm;
            info.certificates = summary.certificates;
            if info.signer_name.is_none() {
                info.signer_name = summary.signer_cn;
            }
            match summary.issue {
                None => {
                    info.structurally_valid = true;
                    info.digest_matches = digest_matches;
                }
                Some(issue) => info.structural_issue = Some(issue),
            }
        }
        Err(e) => {
            info.structural_issue = Some(format!("{e}"));
        }
    }
    info
}

/// 去掉 `/Contents` 十六进制块的尾部填充 0（PDF 用 0 填充保留槽位）。
pub(crate) fn strip_trailing_zeros(raw: &[u8]) -> Vec<u8> {
    let end = raw
        .iter()
        .rposition(|&b| b != 0)
        .map(|p| p + 1)
        .unwrap_or(0);
    raw[..end].to_vec()
}

// ---------------------------------------------------------------------------
// #308 摘要一致性：ByteRange 覆盖段 × CMS messageDigest
// ---------------------------------------------------------------------------

/// 对 `/ByteRange` 覆盖段字节算摘要。
///
/// `byte_range = [start₁, len₁, start₂, len₂, …]`（spec 12.8.1）：签名覆盖
/// 的是这些段拼接后的字节流（`/Contents` 槽位被跳过——这就是为什么通常
/// 是两段）。增量追加不改变覆盖段字节 → 摘要不斐（合法形态）。
///
/// 返回 `None`（无法计算，诚实不猜）：
/// - 元数为奇数 / 空数组（非合规 ByteRange）；
/// - 起始+长度溢出 usize 或越出文件边界（重建/裁剪过的形态——偏移已
///   失去对原始字节的参照意义）；
/// - 算法不支持（本版 sha256/sha384/sha512；sha1/md5 弱算法不背书，
///   且零新增依赖纪律不引入额外 crate）。
pub(crate) fn byte_range_digest(bytes: &[u8], byte_range: &[usize], algo: &str) -> Option<Vec<u8>> {
    use sha2::Digest;

    if byte_range.is_empty() || !byte_range.len().is_multiple_of(2) {
        return None;
    }
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(byte_range.len() / 2);
    for c in byte_range.chunks(2) {
        let end = c[0].checked_add(c[1])?;
        if end > bytes.len() {
            return None;
        }
        spans.push((c[0], end));
    }
    match algo {
        "sha256" => {
            let mut h = sha2::Sha256::new();
            for (s, e) in &spans {
                Digest::update(&mut h, &bytes[*s..*e]);
            }
            Some(h.finalize().to_vec())
        }
        "sha384" => {
            let mut h = sha2::Sha384::new();
            for (s, e) in &spans {
                Digest::update(&mut h, &bytes[*s..*e]);
            }
            Some(h.finalize().to_vec())
        }
        "sha512" => {
            let mut h = sha2::Sha512::new();
            for (s, e) in &spans {
                Digest::update(&mut h, &bytes[*s..*e]);
            }
            Some(h.finalize().to_vec())
        }
        _ => None,
    }
}

/// 比对 CMS `messageDigest` 与覆盖段实际摘要（三层验签的②层，#308）。
///
/// - `Some(true)`：一致——签名字节未被改动（**不等于证书可信**）；
/// - `Some(false)`：不一致——签完后被改过；
/// - `None`：无法验证（缺 `messageDigest`/算法不支持/范围不可信）。
fn verify_digest_matches(bytes: &[u8], byte_range: &[usize], summary: &CmsSummary) -> Option<bool> {
    let algo = summary
        .signer_digest_algorithm
        .as_deref()
        .or(summary.digest_algorithm.as_deref())?;
    let expected = summary.message_digest.as_ref()?;
    let actual = byte_range_digest(bytes, byte_range, algo)?;
    Some(actual == *expected)
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_tlv() {
        // SEQUENCE { INTEGER 1, OID 1.2.840.113549.1.7.2 }
        let der = [
            0x30, 0x0F, // SEQUENCE, 15 bytes
            0x02, 0x01, 0x01, // INTEGER 1
            0x06, 0x0A, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02, 0x00,
        ];
        // 注：最后补 0x00 让长度对齐（真实 OID 编码 9 字节，这里演示用）
        let t = parse_tlv(&der, 0).unwrap();
        assert_eq!(t.tag, tags::SEQUENCE);
        let kids: Vec<_> = t.children().filter_map(|r| r.ok()).collect();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].tag, tags::INTEGER);
        assert_eq!(kids[1].tag, tags::OID);
    }

    #[test]
    fn oid_decoding() {
        // 1.2.840.113549.1.7.2 (signedData)
        let oid = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02];
        assert_eq!(oid_to_string(&oid), "1.2.840.113549.1.7.2");
        // 2.5.4.3 (commonName)
        assert_eq!(oid_to_string(&[0x55, 0x04, 0x03]), "2.5.4.3");
    }

    #[test]
    fn rejects_indefinite_length() {
        let err = parse_tlv(&[0x30, 0x80, 0x00, 0x00], 0).unwrap_err();
        assert!(matches!(err, SignatureError::Der { .. }));
        assert!(err.to_string().contains("不定长"));
    }

    #[test]
    fn rejects_truncated() {
        let err = parse_tlv(&[0x30, 0x10, 0x01], 0).unwrap_err();
        assert!(err.to_string().contains("越界"));
    }

    #[test]
    fn integer_hex_strips_leading_zeros() {
        assert_eq!(integer_hex(&[0x00, 0x12, 0x34]), "1234");
        assert_eq!(integer_hex(&[0x00]), "0");
    }

    #[test]
    fn utc_time_decoding() {
        let t = Tlv {
            tag: tags::UTC_TIME,
            value: b"260101120000Z",
            offset: 0,
            total_len: 15,
        };
        assert_eq!(der_time(&t), "2026-01-01 12:00:00");
    }

    // ------------------------------------------------------------ #308 摘要一致性

    #[test]
    fn byte_range_digest_two_span_concat() {
        // 覆盖段 = bytes[0..2]("AX") + bytes[3..5]("YC")（两段拼接——
        // /Contents 槽位 bytes[2]("B") 被跳过）。期望值与 Python
        // hashlib.sha256(b"AXYC") 对照过。
        let bytes = b"AXBYCZD";
        let br = [0usize, 2, 3, 2];
        let got = byte_range_digest(bytes, &br, "sha256").unwrap();
        let expect: [u8; 32] = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(b"AX");
            h.update(b"YC");
            h.finalize().into()
        };
        assert_eq!(got, expect.to_vec());
    }

    #[test]
    fn byte_range_digest_rejects_bad_ranges() {
        let bytes = b"0123456789";
        // 奇数元
        assert!(byte_range_digest(bytes, &[0, 5, 6], "sha256").is_none());
        // 空
        assert!(byte_range_digest(bytes, &[], "sha256").is_none());
        // 越界
        assert!(byte_range_digest(bytes, &[0, 20], "sha256").is_none());
        // 溢出（usize 上界附近：start + len 回绕）
        let huge = usize::MAX;
        assert!(byte_range_digest(bytes, &[huge, 2], "sha256").is_none());
        // 算法不支持（sha1/md5 弱算法不背书——诚实 None）
        assert!(byte_range_digest(bytes, &[0, 10], "sha1").is_none());
        assert!(byte_range_digest(bytes, &[0, 10], "md5").is_none());
        // 支持的三种都应能算
        for algo in ["sha256", "sha384", "sha512"] {
            assert!(
                byte_range_digest(bytes, &[0, 10], algo).is_some(),
                "{algo} 应支持"
            );
        }
    }

    #[test]
    fn verify_digest_matches_none_paths() {
        let mut summary = CmsSummary::default();
        // 无算法、无 messageDigest → None
        assert!(verify_digest_matches(b"xy", &[0, 2], &summary).is_none());
        // 有算法无摘要 → None
        summary.signer_digest_algorithm = Some("sha256".into());
        assert!(verify_digest_matches(b"xy", &[0, 2], &summary).is_none());
        // 算法不支持 → None（有摘要也验不了）
        summary.signer_digest_algorithm = Some("sha1".into());
        summary.message_digest = Some(vec![0u8; 20]);
        assert!(verify_digest_matches(b"xy", &[0, 2], &summary).is_none());
        // 算法支持 + 摘要在场 → Some(比对结果)
        summary.signer_digest_algorithm = Some("sha256".into());
        let wrong = vec![0u8; 32];
        summary.message_digest = Some(wrong);
        assert_eq!(verify_digest_matches(b"xy", &[0, 2], &summary), Some(false));
        // 正确摘要 → Some(true)
        let right: Vec<u8> = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(b"xy");
            h.finalize().to_vec()
        };
        summary.message_digest = Some(right);
        assert_eq!(verify_digest_matches(b"xy", &[0, 2], &summary), Some(true));
    }
}
