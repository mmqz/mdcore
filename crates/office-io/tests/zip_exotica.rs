//! zip_exotica —— issue #233（R10·P2）：office-io zip 容器安全负向样本
//! 与策略门。
//!
//! **移植来源**：研究仓 mmqz/ai-bid-research `bidcore/crates/bid-parse/
//! src/guard.rs` @ `50e6332^`（B4 删除前的 18 项 zip 面负向测试）——
//! 研究仓按「容器安全收敛引擎侧」原则删除自有防线（50e6332），本文件
//! 按其 audit INTEG-002（1e0bc9d）的建议方案①把五族负向样本移植为
//! 引擎集成测试。构造辅助逐字对齐研究仓实现（字节布局经研究仓 python
//! 重建件 sha256 交叉验证：03-duplicate-entries 逐字节一致）。
//!
//! **zip 8.6 语义验证**（issue 任务——研究仓防线基于 zip 4.6.1 语义，
//! 引擎用 zip 8.6.0，行为差异此前零验证）：每族断言钉死本仓实测行为，
//! 测试注释登记观察结论。
//!
//! **策略门（本 PR 新增）**：
//! - 门 1 增补：重复条目名拒绝（`prescan_family` 单点，xlsx/docx 两族
//!   共享）——五族 EOCD/zip64 绕过的端态载体全部含物理同名对；
//! - 压缩比观察门：**警告口径不拒**（`WarningKind::HighCompressionRatio`
//!   ×`LIMIT_COMPRESS_RATIO`=100）——硬拒有实证误报面（研究仓
//!   engine-probe s7b：合法 149:1 高压缩比件被引擎正常接受，6000 段
//!   全解析）；膨胀上界已由门 0/1/2 钉死（输入字节/声明总量/谎言头
//!   CountingReader），警告 = 纵深可见性（与 #226 Warning 通道同口径）。
//!
//! 断言口径（issue 验收）：拒读或 Warning——**不 panic、不静默**。

use std::io::{Read as _, Write as _};

use mdc_office_io::docx::{read_docx_from_bytes, DocxWriter};
use mdc_office_io::io::WarningKind;

// ---------------------------------------------------------------------------
// 构造辅助（移植自研究仓 guard.rs @ 50e6332^——字节级一致）
// ---------------------------------------------------------------------------

/// 内存 zip 写入（研究仓 write_zip 的 Vec 形态——读入口是字节面）。
fn write_zip(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        for (n, d) in entries {
            w.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// 高熵不可压缩字节流（限制 ASCII 可见区间——合法 UTF-8 且 deflate
/// 压不动；研究仓同款 xorshift64*）。
fn incompressible(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E3779B97F4A7C15 | ((std::process::id() as u64) << 1) | 1;
    let mut v = Vec::with_capacity(len);
    for _ in 0..(len / 8 + 1) {
        x ^= x >> 12;
        x = x.wrapping_mul(0x2545F4914F6CDD1D);
        x ^= x >> 25;
        for b in x.to_le_bytes() {
            v.push(0x21 + (b % 0x5E));
        }
    }
    v.truncate(len);
    v
}

/// patch 中央目录条目名（ZipWriter 写不出重复名——攻击面只能字节级
/// patch，攻击者同理；全文件扫 CDFH 签名，恰命中 1 处）。
fn patch_central_name(raw: &[u8], from: &str, to: &str) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let mut out = raw.to_vec();
    let mut hit = 0;
    let mut i = 0;
    while i + 46 <= out.len() {
        if &out[i..i + 4] == b"PK\x01\x02" {
            let nlen = u16::from_le_bytes(out[i + 28..i + 30].try_into().unwrap()) as usize;
            if nlen == from.len() && &out[i + 46..i + 46 + nlen] == from.as_bytes() {
                out[i + 46..i + 46 + nlen].copy_from_slice(to.as_bytes());
                hit += 1;
            }
        }
        i += 1;
    }
    assert_eq!(hit, 1, "中央目录定位 {from} 失败");
    out
}

/// 底座：两条目 zip + 中央目录名 patch 成物理同名对（EOCD 声明 2）。
fn dup_pair_base() -> Vec<u8> {
    let raw = write_zip(&[
        ("word/aaaa.xml", b"<a/>".to_vec()),
        ("word/bbbb.xml", b"<b/>".to_vec()),
    ]);
    patch_central_name(&raw, "word/bbbb.xml", "word/aaaa.xml")
}

/// zip64 改造（研究仓 make_zip64 逐字移植）：EOCD 四字段 patch 为
/// zip64 标记 + 中央目录与 EOCD 之间插 zip64 EOCD 记录（声明 count）
/// 与 locator。
fn make_zip64(raw: &[u8], count: u64) -> Vec<u8> {
    let eocd = raw
        .windows(4)
        .rposition(|w| w == b"PK\x05\x06")
        .expect("EOCD 不在位");
    let cd_size = u32::from_le_bytes(raw[eocd + 12..eocd + 16].try_into().unwrap()) as u64;
    let cd_off = u32::from_le_bytes(raw[eocd + 16..eocd + 20].try_into().unwrap()) as u64;
    let z64_off = eocd as u64;
    let mut z64 = Vec::with_capacity(56);
    z64.extend_from_slice(b"PK\x06\x06");
    z64.extend_from_slice(&44u64.to_le_bytes());
    z64.extend_from_slice(&[0x2D, 0x00]);
    z64.extend_from_slice(&[0x2D, 0x00]);
    z64.extend_from_slice(&0u32.to_le_bytes());
    z64.extend_from_slice(&0u32.to_le_bytes());
    z64.extend_from_slice(&count.to_le_bytes());
    z64.extend_from_slice(&count.to_le_bytes());
    z64.extend_from_slice(&cd_size.to_le_bytes());
    z64.extend_from_slice(&cd_off.to_le_bytes());
    assert_eq!(z64.len(), 56);
    let mut loc = Vec::with_capacity(20);
    loc.extend_from_slice(b"PK\x06\x07");
    loc.extend_from_slice(&0u32.to_le_bytes());
    loc.extend_from_slice(&z64_off.to_le_bytes());
    loc.extend_from_slice(&1u32.to_le_bytes());
    assert_eq!(loc.len(), 20);
    let mut eocd_new = raw[eocd..].to_vec();
    eocd_new[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    eocd_new[10..12].copy_from_slice(&0xFFFFu16.to_le_bytes());
    eocd_new[12..16].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    eocd_new[16..20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let mut out = Vec::with_capacity(raw.len() + 76);
    out.extend_from_slice(&raw[..eocd]);
    out.extend_from_slice(&z64);
    out.extend_from_slice(&loc);
    out.extend_from_slice(&eocd_new);
    out
}

/// typed 拒绝断言（issue 口径：拒读或 Warning——不 panic 不静默）。
/// 打印实测错误面（zip 8.6 语义观察）。
fn assert_typed_reject(bytes: &[u8], tag: &str) {
    match read_docx_from_bytes(bytes) {
        Ok(_) => panic!("{tag}: 敌意样本不得打开（静默接受 = 防线的假绿）"),
        Err(e) => {
            eprintln!("[zip_exotica] {tag} -> {e}");
            let repr = format!("{e:?}");
            assert!(
                repr.contains("PrescanRejected")
                    || repr.contains("ContainerCorrupt")
                    || repr.contains("InvalidPart")
                    || repr.contains("LegacyOfficeFormat")
                    || repr.contains("Parse("),
                "{tag}: 必须是 typed 拒读（容器门/语义层/解析层任一），得 {e:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 五族负向样本（研究仓 V6/V7/V8/zip64-dup/ratio 移植）
// ---------------------------------------------------------------------------

/// S1 尾部垃圾 × 物理同名对（研究仓 V6/V13a/V13b 三档差分——65,536B
/// 是 zip 4.6.1 旧守卫 65,557B 回扫窗的实测绕过阈值；引擎门 1 重复名
/// 检查与回扫窗无关——三档行为必须一致）。
#[test]
fn s1_tail_garbage_duplicates_rejected_all_tiers() {
    for (garbage, tag) in [(65_535usize, "v13a"), (65_536, "v13b"), (65_600, "v6")] {
        let mut out = dup_pair_base();
        out.extend(std::iter::repeat_n(b'G', garbage));
        assert_typed_reject(&out, &format!("S1 尾垃圾 {garbage}B（{tag}）"));
    }
}

/// S2 影子 EOCD（研究仓 V7）：真实 EOCD（声明 2 + 同名对）后追加伪
/// EOCD（this=1/total=1、comment_len=60,000 越 EOF）。zip 4.6.1 候选
/// 回退循环用真实 EOCD；zip 8.6 行为由本测试钉死（任一 typed 拒绝均
/// 合格——打开即撞重复名门）。
#[test]
fn s2_shadow_eocd_rejected() {
    let mut dup = dup_pair_base();
    let mut shadow = Vec::with_capacity(22);
    shadow.extend_from_slice(b"PK\x05\x06");
    shadow.extend_from_slice(&0u16.to_le_bytes());
    shadow.extend_from_slice(&0u16.to_le_bytes());
    shadow.extend_from_slice(&1u16.to_le_bytes());
    shadow.extend_from_slice(&1u16.to_le_bytes());
    shadow.extend_from_slice(&0u32.to_le_bytes());
    shadow.extend_from_slice(&0u32.to_le_bytes());
    shadow.extend_from_slice(&60_000u16.to_le_bytes());
    assert_eq!(shadow.len(), 22);
    dup.extend_from_slice(&shadow);
    assert_typed_reject(&dup, "S2 影子 EOCD");
}

/// S3 EOCD 双计数字段分歧（研究仓 V8/V8b 两方向）：zip 4.6.1 读「本盘」
/// 字段而旧守卫读「总盘」——分歧时两者比对不同一。引擎不依赖 EOCD
/// 计数（门 1 按 crate 实际解析的条目集查重）——两方向 typed 拒绝。
#[test]
fn s3_eocd_count_field_divergence_rejected_both_directions() {
    // V8：总盘 2→1。
    let mut v8 = dup_pair_base();
    let eocd = v8.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    v8[eocd + 10..eocd + 12].copy_from_slice(&1u16.to_le_bytes());
    assert_typed_reject(&v8, "S3 V8 total=1/this=2");

    // V8b：本盘 2→1。
    let mut v8b = dup_pair_base();
    let eocd = v8b.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    v8b[eocd + 8..eocd + 10].copy_from_slice(&1u16.to_le_bytes());
    assert_typed_reject(&v8b, "S3 V8b this=1/total=2");
}

/// S4 zip64 声明计数携带重复条目（研究仓 R3-S3-1：EOCD 0xFFFF 标记
/// 曾是守恒门豁免牌）。**正向对照**：合法 zip64（声明 1 == 实际 1）
/// 不得在容器层误拒——必须推进到 docx 语义层（[Content_Types].xml
/// 缺失的 InvalidPart）才拒。
#[test]
fn s4_zip64_duplicate_rejected_and_legit_not_false_rejected() {
    // 负向：zip64 声明 2 + 物理同名对。
    let dup64 = make_zip64(&dup_pair_base(), 2);
    assert_typed_reject(&dup64, "S4 zip64 重复条目");

    // 正向对照：合法 zip64（单条目）——容器门全过，拒在语义层。
    let raw = write_zip(&[("word/document.xml", b"<a>ok</a>".to_vec())]);
    let legit = make_zip64(&raw, 1);
    match read_docx_from_bytes(&legit) {
        Ok(_) => panic!("非 docx 布局不得打开"),
        Err(e) => {
            eprintln!("[zip_exotica] S4 正向对照（合法 zip64）-> {e}");
            let repr = format!("{e:?}");
            assert!(
                repr.contains("InvalidPart"),
                "合法 zip64 必须通过容器层（拒在 [Content_Types].xml 缺失的语义层），得 {e:?}"
            );
        }
    }
}

/// S5 压缩比（研究仓 zip_bomb_compress_ratio + 乘法边界对移植）。
/// 引擎口径 = **警告不拒**（硬拒误报面实证：研究仓 s7b 合法 149:1 件
/// 被引擎正常接受）。炸弹载荷走既有 typed 拒绝面（40MB 全零不是合法
/// XML → strict InvalidPart；内存/膨胀上界由门 0/1/2 钉死）。
#[test]
fn s5_ratio_bomb_typed_endstate_and_warning_observability() {
    // 炸弹形态：40MB 全零（比 ≈1000:1）→ typed 拒（不 panic 不静默）。
    let bomb = write_zip(&[("word/document.xml", vec![0u8; 40 * 1024 * 1024])]);
    assert_typed_reject(&bomb, "S5 压缩比炸弹（40MB 全零）");

    // 警告面：合法 docx + 高压缩比附件 → **打开成功** + 比例警告登记
    //（不静默、不误杀）。
    let doc = legit_docx_with_extra(&[("media/zeros.bin", vec![0u8; 40 * 1024])]);
    let opened = read_docx_from_bytes(&doc)
        .unwrap_or_else(|e| panic!("高压缩比附件不得导致合法档被拒（s7b 误报面）：{e}"));
    assert!(
        opened
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::HighCompressionRatio && w.part == "media/zeros.bin"),
        "比例警告应在场：{:?}",
        opened.warnings
    );
}

/// 乘法判据边界（研究仓 R5 P-2 移植——整数除法截断漏洞教训）：
/// Stored 条目 compressed==1000 + patch 未压缩声明：
/// 100,999（真实比 100.999）→ 警告在场；100,000（恰 100）→ 无警告
///（> 非 >=）。
#[test]
fn s5_ratio_multiplication_boundary() {
    let mk = |declared: u32| {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opt = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("media/ratio.bin", opt).unwrap();
            w.write_all(&incompressible(1000)).unwrap();
            w.finish().unwrap();
        }
        let mut raw = buf.into_inner();
        let i = raw.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        raw[i + 24..i + 28].copy_from_slice(&declared.to_le_bytes());
        raw
    };
    // 100,999：真实比 100.999 > 100 → 警告。
    let doc = legit_docx_with_extra_bytes(&mk(100_999));
    let opened =
        read_docx_from_bytes(&doc).unwrap_or_else(|e| panic!("警告口径不拒（100.999 档）：{e}"));
    assert!(
        opened
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::HighCompressionRatio),
        "100,999（真实比 100.999）必须触发警告（乘法判据）"
    );
    // 100,000：恰 100（> 非 >=）→ 无警告。
    let doc = legit_docx_with_extra_bytes(&mk(100_000));
    let opened =
        read_docx_from_bytes(&doc).unwrap_or_else(|e| panic!("警告口径不拒（恰 100 档）：{e}"));
    assert!(
        !opened
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::HighCompressionRatio),
        "恰 100 不得触发（> 非 >=）"
    );
}

/// 研究仓在库敌意夹具端态验证：hostile/03-duplicate-entries.docx
/// （901B，python zipfile 直接写同名两次——3 LFH/3 CDFH、声明 3/3、
/// 两同名 word/document.xml 内容不同[206B/217B]——by_name 取条歧义
/// 的真实端态形态）。
#[test]
fn s6_hostile_fixture_duplicate_entries_rejected() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/zip_exotica/03-duplicate-entries.docx");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("夹具缺失（研究仓 baseline/security/hostile 同源件）：{e}"));
    assert_typed_reject(&bytes, "S6 hostile 03-duplicate-entries（在库件）");
}

// ---------------------------------------------------------------------------
// 辅助：合法 docx + 附加部件（警告面测试的载体）
// ---------------------------------------------------------------------------

/// DocxWriter 最小合法档字节。
fn minimal_docx() -> Vec<u8> {
    let mut w = DocxWriter::new();
    w.paragraph_text("合法档载体。");
    w.save_to_vec().unwrap()
}

/// 合法 docx 重打包 + 附加部件（Deflated）。
fn legit_docx_with_extra(extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let base = minimal_docx();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    {
        let mut za = zip::ZipArchive::new(std::io::Cursor::new(&base)).unwrap();
        for i in 0..za.len() {
            let mut zf = za.by_index(i).unwrap();
            let mut data = Vec::new();
            zf.read_to_end(&mut data).unwrap();
            entries.push((zf.name().to_string(), data));
        }
    }
    let mut all: Vec<(&str, Vec<u8>)> = entries
        .iter()
        .map(|(n, d)| (n.as_str(), d.clone()))
        .collect();
    for (n, d) in extra {
        all.push((n, d.clone()));
    }
    write_zip(&all)
}

/// 合法 docx 重打包 + 附加原始字节部件（Stored + patch 声明的构造件
/// 直接字节级拼接——不重压缩，保住 compressed==1000 与声明 patch 的
/// 边界语义）。布局：[base 数据段][extra LFH+数据][base 中央目录]
/// [extra CDFH][EOCD（计数+1/目录大小/偏移修正）]。
fn legit_docx_with_extra_bytes(extra: &[u8]) -> Vec<u8> {
    let base = minimal_docx();
    let eocd = base
        .windows(4)
        .rposition(|w| w == b"PK\x05\x06")
        .expect("base EOCD");
    let cd_off = u32::from_le_bytes(base[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    let cd_size = u32::from_le_bytes(base[eocd + 12..eocd + 16].try_into().unwrap());
    let this = u16::from_le_bytes(base[eocd + 8..eocd + 10].try_into().unwrap());
    let total = u16::from_le_bytes(base[eocd + 10..eocd + 12].try_into().unwrap());
    let x_eocd = extra
        .windows(4)
        .rposition(|w| w == b"PK\x05\x06")
        .expect("extra EOCD");
    let x_cd = extra
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .expect("extra CDFH");
    let x_lfh = &extra[..x_cd];
    let mut x_cdfh = extra[x_cd..x_eocd].to_vec();
    // CDFH 的本地头偏移字段（+42..46）从 standalone 布局的 0 平移到
    // 合并后位置（cd_off）——不平移则 crate 按偏移 0 读到 base 首条
    // 数据 → 校验和错（#233 实施发现）。
    x_cdfh[42..46].copy_from_slice(&(cd_off as u32).to_le_bytes());
    let x_cdfh = &x_cdfh[..];
    let mut out = Vec::with_capacity(base.len() + extra.len());
    out.extend_from_slice(&base[..cd_off]);
    out.extend_from_slice(x_lfh);
    out.extend_from_slice(&base[cd_off..eocd]);
    out.extend_from_slice(x_cdfh);
    let mut eocd_new = base[eocd..].to_vec();
    eocd_new[8..10].copy_from_slice(&(this + 1).to_le_bytes());
    eocd_new[10..12].copy_from_slice(&(total + 1).to_le_bytes());
    eocd_new[12..16].copy_from_slice(&(cd_size + x_cdfh.len() as u32).to_le_bytes());
    eocd_new[16..20].copy_from_slice(&(cd_off as u32 + x_lfh.len() as u32).to_le_bytes());
    out.extend_from_slice(&eocd_new);
    out
}

/// 短输入/非 zip 垃圾不劫持（正控——既有语义恒等）。
#[test]
fn non_zip_garbage_still_container_corrupt() {
    let err = read_docx_from_bytes(b"garbage not a zip").unwrap_err();
    let repr = format!("{err:?}");
    assert!(repr.contains("ContainerCorrupt"), "得 {repr}");
}
