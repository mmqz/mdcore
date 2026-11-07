//! S2-7（issue #116 任务 2）· 数字签名**读取/结构验证/摘要一致性**面。
//!
//! # 验签三层（#308 补第②层）
//!
//! ```text
//! ① 结构完整（ByteRange 合法 + CMS 可解析）          ← 一直有
//! ② 摘要一致（签的是这些字节吗）                       ← #308 新增
//! ③ 信任链（证书可信吗）                               ← 需要信任根，明确不做
//! ```
//!
//! ② 的语义（[`SignatureInfo::digest_matches`]）：把 `/ByteRange` 各段
//! 字节按序算摘要，与 CMS SignerInfo `signedAttrs` 里的 `messageDigest`
//! 比对——**签名的字节变了就是 Some(false)**（用户最关心的「这份合同
//! 签了之后有没有被改过」从此能答）。`None` = 无法验证（算法不支持/
//! 区间越界/无 signedAttrs——诚实不猜）。
//!
//! ⚠ **digest_matches = Some(true) 不等于证书可信**：只证明「字节没
//! 变过 + 与摘要一致」，不证明签名者身份（自签证书也过这关）；也不
//! 验证签名本身的数学有效性（那需要从内嵌证书取公钥做非对称验算+
//! 信任链判定，属第③层）。
//!
//! # 范围（第一版）
//!
//! - 读 PDF 的 `/Sig` 域（`/V` → 签名字典）；
//! - `/Contents` 是**十六进制编码的 DER**，解出 PKCS#7/CMS `SignedData`；
//! - 遍历 CMS 结构，报出签名人/时间/原因/位置/子过滤器/字节范围/证书；
//! - 判定「结构是否完整」（**不验证证书链、不验时间戳**——那需要信任根，
//!   且第一版范围明确不含）；
//! - 摘要一致性（#308）：sha256/sha384/sha512（sha2 = 0.11 锁内既有）；
//!   sha1/md5 等旧算法不支持 → `digest_matches = None`。
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
    /// 摘要一致性（#308 验签第②层）：`/ByteRange` 各段字节按序算摘要，
    /// 与 CMS `signedAttrs` 的 `messageDigest` 比对。
    ///
    /// - `Some(true)`：签名的字节未被改动（摘要一致）；
    /// - `Some(false)`：**摘要不匹配——签名覆盖的字节已变**（签名失效
    ///   的机制面证据）；
    /// - `None`：无法验证（算法不支持 / ByteRange 越界 / 无 signedAttrs
    ///   摘要——诚实不猜）。
    ///
    /// ⚠ `Some(true)` **不等于证书可信**，也不验证签名数学有效性——
    /// 见模块文档「验签三层」。
    #[serde(default)]
    pub digest_matches: Option<bool>,
    /// 无法验证摘要时的原因（`digest_matches = None` 时在位；人类可读）。
    #[serde(default)]
    pub digest_issue: Option<String>,
    /// 证书清单（CMS 内嵌的证书，按出现序）。
    pub certificates: Vec<CertInfo>,
    /// 摘要算法名（`sha256` 等；无则 None）。
    pub digest_algorithm: Option<String>,
    /// 签名算法名（`rsassa_pkcs1v15` / `sha256_rsa` 等，取 CMS 里的 OID 名）。
    pub signature_algorithm: Option<String>,
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
    pub const OCTET_STRING: u8 = 0x04;
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
    pub certificates: Vec<CertInfo>,
    pub signer_cn: Option<String>,
    /// 结构问题（None = 结构完整）。
    pub issue: Option<String>,
    /// #308：SignerInfo **自身**的 digestAlgorithm 算法名（messageDigest
    /// 属性的计算算法权威在这里，非外层 digestAlgorithms SET——两者
    /// 应当一致但标准只约束 SignerInfo）。
    pub signer_digest_algorithm: Option<String>,
    /// #308：SignerInfo `signedAttrs` 里的 `messageDigest` 属性值
    /// （OID 1.2.840.113549.1.9.4 的 OCTET STRING 原始字节）——
    /// 摘要一致性比对的「签时基线」。无 signedAttrs / 无该属性 → None。
    pub message_digest: Option<Vec<u8>>,
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
    // SignerInfo 自身的 digestAlgorithm（#308：messageDigest 的算法权威）。
    if let Some(Ok(alg)) = s.next() {
        if let Some(Ok(oid)) = alg.children().next() {
            if oid.tag == tags::OID {
                out.signer_digest_algorithm = Some(algorithm_name(&oid_to_string(oid.value)));
            }
        }
    }
    let mut next = s.next().and_then(|r| r.ok());
    if let Some(n) = next {
        if n.tag == 0xA0 {
            // signedAttrs [0] IMPLICIT SET OF Attribute（#308：提取
            // messageDigest——摘要一致性的签时基线）。
            out.message_digest = find_message_digest(&n);
            next = s.next().and_then(|r| r.ok()); // signatureAlgorithm
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

/// 在 `signedAttrs [0] IMPLICIT SET OF Attribute` 里找 `messageDigest`
/// 属性（OID 1.2.840.113549.1.9.4）的 OCTET STRING 原始字节。
///
/// ```text
/// Attribute ::= SEQUENCE { attrType OID, attrValues SET OF ANY }
/// messageDigest 属性的 attrValues 里恰一个 OCTET STRING（CMS §5.3）。
/// ```
///
/// 找不到 / 形态不对 → None（宽容读面纪律：不猜，验证面如实报
/// 无法验证）。
fn find_message_digest(signed_attrs: &Tlv<'_>) -> Option<Vec<u8>> {
    const OID_MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
    for attr in signed_attrs.children().flatten() {
        if attr.tag != tags::SEQUENCE {
            continue;
        }
        let mut fields = attr.children().flatten();
        let Some(attr_type) = fields.next() else {
            continue;
        };
        if attr_type.tag != tags::OID || oid_to_string(attr_type.value) != OID_MESSAGE_DIGEST {
            continue;
        }
        let Some(values) = fields.next() else {
            continue;
        };
        // attrValues SET → 首个 OCTET STRING 即摘要字节。
        for v in values.children().flatten() {
            if v.tag == tags::OCTET_STRING {
                return Some(v.value.to_vec());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// #308：摘要一致性（验签第②层）
// ---------------------------------------------------------------------------

/// 按 ByteRange 各段计算摘要，与签时基线（messageDigest）比对。
///
/// 返回 `(Option<bool>, Option<String>)`：`(Some(true/false), None)` /
/// `(None, Some(原因))`——与 [`SignatureInfo::digest_matches`] /
/// [`SignatureInfo::digest_issue`] 同形。
///
/// 无法验证的诚实路径（全部 None + 原因）：
/// - CMS 里没有 signedAttrs / messageDigest（无签时基线可比）；
/// - 摘要算法不支持（sha1/md5 等旧算法——sha2 面只有 256/384/512）；
/// - ByteRange 空洞（空 / 奇数个 / 越界 / 段间重叠）。
///
/// 算法权威 = SignerInfo 自身 digestAlgorithm（无则退外层
/// digestAlgorithms SET——两处都无 → 无法验证）。
fn verify_digest(
    bytes: &[u8],
    byte_range: &[usize],
    message_digest: Option<&[u8]>,
    signer_digest_algorithm: Option<&str>,
    outer_digest_algorithm: Option<&str>,
) -> (Option<bool>, Option<String>) {
    use sha2::Digest;

    let Some(baseline) = message_digest else {
        return (
            None,
            Some("CMS 无 signedAttrs messageDigest（无签时基线可比）".into()),
        );
    };
    let algo = signer_digest_algorithm.or(outer_digest_algorithm);
    let Some(algo) = algo else {
        return (None, Some("CMS 未报摘要算法".into()));
    };

    // 区间合法性：偶数个 + 非空 + 不越界 + 不重叠（重叠即结构异常——
    // 无法确定签名覆盖的字节面，宁不验证不猜）。
    if byte_range.is_empty() || !byte_range.len().is_multiple_of(2) {
        return (None, Some("ByteRange 空洞（空或奇数个）".into()));
    }
    let mut covered_end = 0usize;
    for chunk in byte_range.chunks(2) {
        let (start, len) = (chunk[0], chunk[1]);
        let Some(end) = start.checked_add(len) else {
            return (None, Some("ByteRange 算术溢出".into()));
        };
        if end > bytes.len() || start < covered_end {
            return (
                None,
                Some(format!(
                    "ByteRange 越界或重叠（[{start}, {end}) 超出文件 {} 字节 / 与前段重叠）",
                    bytes.len()
                )),
            );
        }
        covered_end = end;
    }

    // 按序喂各段字节 → 与签时基线比对（等长 + 逐位相同才匹配）。
    let computed: Option<Vec<u8>> = match algo {
        "sha256" => {
            let mut h = sha2::Sha256::new();
            for chunk in byte_range.chunks(2) {
                h.update(&bytes[chunk[0]..chunk[0] + chunk[1]]);
            }
            Some(h.finalize().to_vec())
        }
        "sha384" => {
            let mut h = sha2::Sha384::new();
            for chunk in byte_range.chunks(2) {
                h.update(&bytes[chunk[0]..chunk[0] + chunk[1]]);
            }
            Some(h.finalize().to_vec())
        }
        "sha512" => {
            let mut h = sha2::Sha512::new();
            for chunk in byte_range.chunks(2) {
                h.update(&bytes[chunk[0]..chunk[0] + chunk[1]]);
            }
            Some(h.finalize().to_vec())
        }
        other => {
            return (
                None,
                Some(format!("摘要算法不支持（{other}）——仅支持 sha256/384/512")),
            );
        }
    };
    let computed = computed.expect("不支持算法已提前返回");
    let matches = computed.len() == baseline.len() && computed == baseline;
    if matches {
        (Some(true), None)
    } else {
        (
            Some(false),
            Some("摘要不匹配：签名覆盖的字节已被改动（签名失效）".into()),
        )
    }
}
// ---------------------------------------------------------------------------
// PDF 层：读 /Sig 域
// ---------------------------------------------------------------------------

/// 读 PDF 里的全部签名（按域树出现序）。
///
/// 找不到任何 /Sig 域 → 返回空 vec（不是错误：多数文档本来就没有签名）。
pub fn read_signatures(bytes: &[u8]) -> Result<Vec<SignatureInfo>, SignatureError> {
    let doc = lopdf::Document::load_mem(bytes)
        .map_err(|e| SignatureError::Open(format!("lopdf 装载失败：{e}")))?;
    if doc.is_encrypted() {
        return Err(SignatureError::Encrypted);
    }

    let mut out = Vec::new();
    for (name, dict) in collect_sig_fields(&doc) {
        out.push(build_signature_info(bytes, &doc, &name, dict));
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
/// 由签名域字典组装 [`SignatureInfo`]。`bytes` = 原始文件字节（#308：
/// 摘要一致性要对 ByteRange 覆盖的**原字节**算摘要，不是重解析产物）。
fn build_signature_info(
    bytes: &[u8],
    doc: &lopdf::Document,
    field_name: &str,
    dict: lopdf::Dictionary,
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
        digest_matches: None,
        digest_issue: None,
        certificates: Vec::new(),
        digest_algorithm: None,
        signature_algorithm: None,
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
            // #308 验签第②层：摘要一致性（先于字段移动——算法名/基线都
            // 借自 summary；只在 CMS 可解析时才有基线可谈，签名域未签署/
            // 无 /Contents 的早退路径保持 None + 结构原因）。
            let (matches, issue) = verify_digest(
                bytes,
                &info.byte_range,
                summary.message_digest.as_deref(),
                summary.signer_digest_algorithm.as_deref(),
                summary.digest_algorithm.as_deref(),
            );
            info.digest_matches = matches;
            info.digest_issue = issue;
            info.digest_algorithm = summary.digest_algorithm;
            info.signature_algorithm = summary.signature_algorithm;
            info.certificates = summary.certificates;
            if info.signer_name.is_none() {
                info.signer_name = summary.signer_cn;
            }
            match summary.issue {
                None => info.structurally_valid = true,
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

    // ---------------------------------------------------------------- #308

    /// messageDigest 属性提取：手工 DER（Attribute 形态与真实 CMS 一致）。
    #[test]
    fn message_digest_attribute_is_extracted() {
        // Attribute ::= SEQUENCE { attrType OID, attrValues SET OF ANY }
        // messageDigest 属性：SET 里恰一个 OCTET STRING。
        let attr_md: Vec<u8> = [
            0x30, 0x1D, // SEQUENCE，内容 29 字节
            0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09,
            0x04, // OID messageDigest
            0x31, 0x10, // SET，内容 16 字节
            0x04, 0x0E, // OCTET STRING，内容 14 字节
        ]
        .iter()
        .copied()
        .chain(b"hello-digest!!".iter().copied())
        .collect();
        // signingTime 属性（同构异 OID——判别面的干扰项）。
        let attr_st: Vec<u8> = [
            0x30, 0x0D, // SEQUENCE，内容 13 字节
            0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09,
            0x05, // OID signingTime
            0x31, 0x00, // SET 空
        ]
        .to_vec();
        // signedAttrs [0] IMPLICIT SET OF Attribute：内容 = 各 Attribute 原样拼接。
        let wrap = |content: &[Vec<u8>]| -> Vec<u8> {
            let body: Vec<u8> = content.concat();
            let mut node = vec![0xA0, body.len() as u8];
            node.extend_from_slice(&body);
            node
        };

        // 单属性：直接提取。
        let node = wrap(std::slice::from_ref(&attr_md));
        let t = parse_tlv(&node, 0).unwrap();
        assert_eq!(
            find_message_digest(&t).as_deref(),
            Some(&b"hello-digest!!"[..])
        );

        // 多属性（signingTime 在前）：按 OID 精确定位，不误取。
        let node2 = wrap(&[attr_st.clone(), attr_md]);
        let t2 = parse_tlv(&node2, 0).unwrap();
        assert_eq!(
            find_message_digest(&t2).as_deref(),
            Some(&b"hello-digest!!"[..]),
            "多属性 signedAttrs 里按 OID 精确取 messageDigest"
        );

        // 无 messageDigest 属性 → None（诚实不猜）。
        let node3 = wrap(&[attr_st]);
        let t3 = parse_tlv(&node3, 0).unwrap();
        assert_eq!(find_message_digest(&t3), None);
    }

    /// 摘要计算与比对主径：sha256 两段拼接；篡改覆盖内字节 → Some(false)；
    /// 覆盖域外字节 → 仍 Some(true)（签名只盖 ByteRange）。
    #[test]
    fn verify_digest_sha256_segments_and_tamper() {
        // 20 字节：range [0,8) + [12,16)；16..20 在覆盖域外。
        let bytes: Vec<u8> = (0u8..20).collect();
        let mut h = sha2::Sha256::new();
        use sha2::Digest;
        h.update(&bytes[0..8]);
        h.update(&bytes[12..16]);
        let baseline = h.finalize().to_vec();
        let range = [0usize, 8, 12, 4];

        let (m, issue) = verify_digest(&bytes, &range, Some(&baseline), Some("sha256"), None);
        assert_eq!(m, Some(true));
        assert!(issue.is_none());

        // 篡改覆盖内字节（段 1 中部）→ Some(false)。
        let mut tampered = bytes.clone();
        tampered[3] ^= 0xFF;
        let (m2, issue2) = verify_digest(&tampered, &range, Some(&baseline), Some("sha256"), None);
        assert_eq!(m2, Some(false));
        assert!(
            issue2.as_deref().unwrap_or("").contains("改动"),
            "诊断要说清字节被改：{issue2:?}"
        );

        // 篡改覆盖域**外**字节（16..20 未被签名覆盖）→ 仍 Some(true)。
        let mut outside = bytes.clone();
        outside[18] ^= 0xFF;
        let (m3, _) = verify_digest(&outside, &range, Some(&baseline), Some("sha256"), None);
        assert_eq!(m3, Some(true), "覆盖域外字节不影响签名（ByteRange 语义）");
    }

    /// 算法不支持 → None + 原因（诚实不猜——验收 3）。
    #[test]
    fn verify_digest_unsupported_algorithm_is_none() {
        let bytes = vec![0u8; 8];
        let (m, issue) = verify_digest(&bytes, &[0, 8], Some(&[0u8; 20]), Some("sha1"), None);
        assert_eq!(m, None, "sha1 不支持 → 无法验证（不猜）");
        assert!(
            issue.as_deref().unwrap_or("").contains("不支持"),
            "原因要说清不支持：{issue:?}"
        );
    }

    /// 基线缺失 / 区间非法 → None + 原因。
    #[test]
    fn verify_digest_none_paths() {
        let bytes = vec![0u8; 8];
        // 无签时基线（CMS 无 signedAttrs messageDigest）。
        let (m, issue) = verify_digest(&bytes, &[0, 8], None, Some("sha256"), None);
        assert_eq!(m, None);
        assert!(issue.as_deref().unwrap_or("").contains("基线"));

        // ByteRange 越界（end > 文件长）。
        let (m2, issue2) = verify_digest(&bytes, &[0, 100], Some(&[0u8; 32]), Some("sha256"), None);
        assert_eq!(m2, None);
        assert!(issue2.as_deref().unwrap_or("").contains("越界"));

        // ByteRange 奇数个。
        let (m3, issue3) =
            verify_digest(&bytes, &[0, 4, 5], Some(&[0u8; 32]), Some("sha256"), None);
        assert_eq!(m3, None);
        assert!(issue3.as_deref().unwrap_or("").contains("奇数"));

        // 段重叠（第二段起点在第一段覆盖内）。
        let (m4, issue4) = verify_digest(
            &bytes,
            &[0, 5, 3, 5],
            Some(&[0u8; 32]),
            Some("sha256"),
            None,
        );
        assert_eq!(m4, None);
        assert!(issue4.as_deref().unwrap_or("").contains("重叠"));

        // 算法两处（SignerInfo + 外层）都无 → None。
        let (m5, issue5) = verify_digest(&bytes, &[0, 8], Some(&[0u8; 32]), None, None);
        assert_eq!(m5, None);
        assert!(issue5.as_deref().unwrap_or("").contains("算法"));
    }
}
