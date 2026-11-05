//! Q3-1 R1 · 文本 → PDF 生成器（方案 (a) 引擎内建——issue #29）。
//!
//! # 谱系与口径
//!
//! **spike 实证后生产化**（#29 评论 6459274884 的 R1 步）：CJK PDF 的
//! Type0/CID 字体面在 lopdf `font_embedding`（单字节简单字体）之外，
//! 由本模块自建。spike 两轮调试定谳的**码位 rank 序**是提取召回
//! 100% 的充分条件（见 [`ToUnicodeBuilder`] 文档），已随子集测试
//! 字体回归锁死。
//!
//! # R1 范围（A4 固定几何 + 纯文本流）
//!
//! - 输入：TTF 字体字节 + 纯文本 + [`PageLayout`]（A4 595×842pt）；
//! - 输出：PDF 字节（每页独立内容流，全角近似均匀排版——DW=1000）；
//! - 降级显式化：字体未覆盖字符**跳过并计数**（[`GeneratedPdf::
//!   skipped_chars`]——映射层不静默丢，镜像导入面 stats 哲学）；
//! - R2 扩展位：BlockTree 映射 + PageGeometry（twips→pt 与 G2 同源）
//!   + 标题阶梯/粗体（双字体实例）/表格线框。
//!
//! # 字体策略（部署纪律）
//!
//! 字体文件**不入库**（>2MB 纪律）：运行时由调用方注入 TTF 字节
//! （部署方自带字体）；测试面用 51KB→152KB 子集夹具
//! （`tests/fixtures/q31_test_font.ttf`——fontTools 子集，含康熙
//! 部首块锁 rank 回归）；验收脚本用环境字体（见
//! `test-artifacts/pdf-generate/`）。

use std::collections::BTreeMap;

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

/// 生成失败（typed——不静默）。
#[derive(Debug, PartialEq)]
pub enum GenerateError {
    /// 非 TrueType 字体（sfnt 版本字不识）。
    NotTrueType,
    /// 无可用 cmap 子表（format 4/12 均缺席）。
    NoCmap,
    /// 文本为空（零页文档无意义——R1 拒绝，调用方自决空态）。
    EmptyText,
}

/// R1 页面布局（A4 固定；R2 换 PageGeometry 全参）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLayout {
    /// 页边距（pt，四边同值）。
    pub margin_pt: f64,
    /// 字号（pt）。
    pub font_size_pt: f64,
    /// 行距（pt）。
    pub leading_pt: f64,
}

impl Default for PageLayout {
    fn default() -> Self {
        Self {
            margin_pt: 72.0,
            font_size_pt: 12.0,
            leading_pt: 17.0,
        }
    }
}

/// A4 页几何（pt）——Q3-1 验收口径（595×842）。
pub const A4_W_PT: f64 = 595.0;
pub const A4_H_PT: f64 = 842.0;

/// 生成产物（降级面显式化）。
#[derive(Debug)]
pub struct GeneratedPdf {
    /// PDF 字节。
    pub bytes: Vec<u8>,
    /// 生成页数。
    pub pages: usize,
    /// 字体未覆盖而跳过的字符数（显式降级——非静默丢）。
    pub skipped_chars: usize,
}

// ---------------------------------------------------------------------------
// S2-5（issue #116 任务 1）：CIDFont 复用面
// ---------------------------------------------------------------------------

/// 字符的 GID 查找面（表单中文外观/扁平化共用——本模块字体解析的出口）。
///
/// `None` = 字体缺该字形（调用方必须**显式**处理：报错或计数跳过，
/// 绝不静默画乱码）。
pub(crate) fn font_gid(ttf: &[u8], ch: char) -> Option<u16> {
    TtfFace::parse(ttf).ok()?.char_gid(ch)
}

/// 构造可内嵌的 Type0/CIDFontType2 字体对象（自包含：FontFile2 /
/// FontDescriptor / CIDFont / ToUnicode 全部内联进返回的字典）。
///
/// 与 [`generate_text_pdf`] 同一个字体面，区别是**对象直接内联**——
/// 表单外观流与扁平化的资源字典要能就地塞这个对象，不需要跨文档引用。
/// 返回 `None` = 字体不可解析或缺必需表。
pub(crate) fn font_object(ttf: &[u8]) -> Option<lopdf::Dictionary> {
    let face = TtfFace::parse(ttf).ok()?;
    let scale = 1000.0 / face.units_per_em as f64;
    let font_file = Stream::new(
        dictionary! { "Length1" => Object::Integer(ttf.len() as i64) },
        ttf.to_vec(),
    );
    let desc = dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(b"EmbeddedCJK".to_vec()),
        "Flags" => 4,
        "FontBBox" => Object::Array(
            [face.bbox.0, face.bbox.1, face.bbox.2, face.bbox.3]
                .iter()
                .map(|v| Object::Integer((*v as f64 * scale) as i64))
                .collect(),
        ),
        "ItalicAngle" => 0,
        "Ascent" => Object::Integer((face.ascent as f64 * scale) as i64),
        "Descent" => Object::Integer((face.descent as f64 * scale) as i64),
        "CapHeight" => Object::Integer((face.ascent as f64 * scale) as i64),
        "StemV" => 80,
        "FontFile2" => Object::Stream(font_file),
    };
    let mut csi = Dictionary::new();
    csi.set(
        "Registry",
        Object::String(b"Adobe".to_vec(), lopdf::StringFormat::Literal),
    );
    csi.set(
        "Ordering",
        Object::String(b"Identity".to_vec(), lopdf::StringFormat::Literal),
    );
    csi.set("Supplement", 0);
    let mut cid = Dictionary::new();
    cid.set("Type", "Font");
    cid.set("Subtype", "CIDFontType2");
    cid.set("BaseFont", Object::Name(b"EmbeddedCJK".to_vec()));
    cid.set("CIDSystemInfo", csi);
    cid.set("FontDescriptor", Object::Dictionary(desc));
    cid.set("DW", 1000);
    cid.set("CIDToGIDMap", "Identity");
    let to_unicode = Stream::new(dictionary! {}, to_unicode_cmap(&face).into_bytes());
    let mut t0 = Dictionary::new();
    t0.set("Type", "Font");
    t0.set("Subtype", "Type0");
    t0.set("BaseFont", Object::Name(b"EmbeddedCJK".to_vec()));
    t0.set("Encoding", "Identity-H");
    t0.set(
        "DescendantFonts",
        Object::Array(vec![Object::Dictionary(cid)]),
    );
    t0.set("ToUnicode", Object::Stream(to_unicode));
    Some(t0)
}

/// R1 生成入口：纯文本 → A4 PDF（确定性——同输入逐字节同输出）。
///
/// 排版口径（全角近似）：每字符宽 = 字号（DW=1000）；每行字符数 =
/// (页宽 - 2×边距) / 字号 向下取整；每页行数 = (页高 - 2×边距) /
/// 行距 向下取整。拉丁比例宽精化属 R2（hmtx 面）。
pub fn generate_text_pdf(
    font_bytes: &[u8],
    text: &str,
    layout: &PageLayout,
) -> Result<GeneratedPdf, GenerateError> {
    let face = TtfFace::parse(font_bytes)?;
    let usable_w = A4_W_PT - 2.0 * layout.margin_pt;
    let usable_h = A4_H_PT - 2.0 * layout.margin_pt;
    let chars_per_line = (usable_w / layout.font_size_pt).floor().max(1.0) as usize;
    let lines_per_page = (usable_h / layout.leading_pt).floor().max(1.0) as usize;
    if text.chars().all(|c| c.is_whitespace()) {
        return Err(GenerateError::EmptyText);
    }

    // 折行 + GID 编码（未覆盖字符跳过计数）。
    let mut skipped = 0usize;
    let mut lines: Vec<String> = Vec::new();
    let mut cur: Vec<u16> = Vec::with_capacity(chars_per_line);
    for ch in text.chars() {
        if ch == '\n' {
            if !cur.is_empty() {
                lines.push(gid_hex(&cur));
                cur.clear();
            }
            lines.push(String::new()); // 显式空行（段界）
            continue;
        }
        if ch.is_whitespace() {
            continue;
        }
        match face.chars.get(&(ch as u32)) {
            Some(g) => cur.push(*g),
            None => skipped += 1,
        }
        if cur.len() >= chars_per_line {
            lines.push(gid_hex(&cur));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        lines.push(gid_hex(&cur));
    }

    let pages = build_pdf(&face, font_bytes, &lines, lines_per_page, layout, skipped)
        .ok_or(GenerateError::EmptyText)?;
    Ok(pages)
}

fn gid_hex(gids: &[u16]) -> String {
    let mut s = String::with_capacity(gids.len() * 4);
    for g in gids {
        s.push_str(&format!("{g:04X}"));
    }
    s
}

// ---------------------------------------------------------------------------
// 最小 TTF 解析（cmap fmt4/12 + maxp/head/hhea——spike 移植）
// ---------------------------------------------------------------------------

struct TtfFace {
    chars: BTreeMap<u32, u16>,
    #[allow(dead_code)]
    num_glyphs: u16,
    units_per_em: u16,
    bbox: (i16, i16, i16, i16),
    ascent: i16,
    descent: i16,
}

fn be16(b: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([b[off], b[off + 1]])
}
fn be32(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

impl TtfFace {
    /// 单字符 GID（S2-5 复用面——表单外观/扁平化的字形查表出口）。
    pub(crate) fn char_gid(&self, ch: char) -> Option<u16> {
        self.chars.get(&(ch as u32)).copied()
    }

    fn parse(bytes: &[u8]) -> Result<Self, GenerateError> {
        if bytes.len() < 12 || be32(bytes, 0) != 0x0001_0000 {
            return Err(GenerateError::NotTrueType);
        }
        let num_tables = be16(bytes, 4) as usize;
        let table = |name: &str| -> Option<(usize, usize)> {
            for i in 0..num_tables {
                let rec = 12 + i * 16;
                let tag = bytes.get(rec..rec + 4)?;
                if tag == name.as_bytes() {
                    return Some((
                        be32(bytes, rec + 8) as usize,
                        be32(bytes, rec + 12) as usize,
                    ));
                }
            }
            None
        };
        let (cmap_off, _) = table("cmap").ok_or(GenerateError::NoCmap)?;
        let (maxp_off, _) = table("maxp").ok_or(GenerateError::NotTrueType)?;
        let (head_off, _) = table("head").ok_or(GenerateError::NotTrueType)?;
        let (hhea_off, _) = table("hhea").ok_or(GenerateError::NotTrueType)?;

        let num_glyphs = be16(bytes, maxp_off + 4);
        let units_per_em = be16(bytes, head_off + 18);
        let bbox = (
            be16(bytes, head_off + 36) as i16,
            be16(bytes, head_off + 38) as i16,
            be16(bytes, head_off + 40) as i16,
            be16(bytes, head_off + 42) as i16,
        );
        let ascent = be16(bytes, hhea_off + 4) as i16;
        let descent = be16(bytes, hhea_off + 6) as i16;

        // cmap 子表：优先 format 12（全平面），回落 format 4（BMP）。
        let n_sub = be16(bytes, cmap_off + 2) as usize;
        let mut sub4: Option<usize> = None;
        let mut sub12: Option<usize> = None;
        for i in 0..n_sub {
            let rec = cmap_off + 4 + i * 8;
            let platform = be16(bytes, rec);
            let off = be32(bytes, rec + 4) as usize;
            let abs = cmap_off + off;
            let fmt = be16(bytes, abs);
            if platform == 3 || platform == 0 {
                if fmt == 4 {
                    sub4 = Some(abs);
                } else if fmt == 12 {
                    sub12 = Some(abs);
                }
            }
        }

        let mut chars = BTreeMap::new();
        if let Some(abs) = sub12 {
            let n_groups = be32(bytes, abs + 12) as usize;
            for g in 0..n_groups {
                let rec = abs + 16 + g * 12;
                let start = be32(bytes, rec);
                let end = be32(bytes, rec + 4);
                let start_gid = be32(bytes, rec + 8);
                for (k, cp) in (start..=end).enumerate() {
                    let gid = start_gid + k as u32;
                    if gid != 0 && (gid as u64) < num_glyphs as u64 {
                        chars.insert(cp, gid as u16);
                    }
                }
            }
        } else if let Some(abs) = sub4 {
            let seg_count = be16(bytes, abs + 6) as usize / 2;
            let end_off = abs + 14;
            let start_off = end_off + seg_count * 2 + 2;
            let delta_off = start_off + seg_count * 2;
            let range_off = delta_off + seg_count * 2;
            for i in 0..seg_count {
                let end = be16(bytes, end_off + i * 2);
                let start = be16(bytes, start_off + i * 2);
                let delta = be16(bytes, delta_off + i * 2);
                let roff = be16(bytes, range_off + i * 2) as usize;
                if start == 0xFFFF {
                    continue;
                }
                for cp in start..=end {
                    let gid = if roff == 0 {
                        cp.wrapping_add(delta)
                    } else {
                        let gp = range_off + i * 2 + roff + (cp - start) as usize * 2;
                        be16(bytes, gp).wrapping_add(delta)
                    };
                    if gid != 0 && (gid as usize) < num_glyphs as usize {
                        chars.insert(cp as u32, gid);
                    }
                }
            }
        } else {
            return Err(GenerateError::NoCmap);
        }

        Ok(Self {
            chars,
            num_glyphs,
            units_per_em,
            bbox,
            ascent,
            descent,
        })
    }
}

// ---------------------------------------------------------------------------
// ToUnicode（码位 rank 序——spike 两轮调试定谳）
// ---------------------------------------------------------------------------

/// 码位 rank：**提取召回 100% 的充分条件**（spike 实证）。
///
/// 同一 GID 常被多码位共享同字形：CJK 兼容表意区（U+F900..=U+FAD9，
/// 理 U+7406 = 理 U+F9A4）与**康熙部首区（U+2F00..=U+2FDF，一 U+4E00
/// = ⼀ U+2F00——码位比正区小，「保最小」会错选部首形，spike 实测
/// 召回跌 89.3%）**。rank 序：正区（URO+扩展A/B+）0 > 常规 1 > 兼容
/// 表意 2 > 康熙部首 3；同 rank 保最小码位。
fn cp_rank(cp: u32) -> u8 {
    match cp {
        0x3400..=0x9FFF => 0,
        0x20000..=0x3FFFF => 0,
        0xF900..=0xFAD9 => 2,
        0x2F00..=0x2FDF => 3,
        _ => 1,
    }
}

fn to_unicode_cmap(face: &TtfFace) -> String {
    let mut best: BTreeMap<u16, u32> = BTreeMap::new();
    for (cp, g) in &face.chars {
        match best.get(g) {
            Some(prev) if cp_rank(*cp) > cp_rank(*prev) => {}
            Some(prev) if cp_rank(*cp) == cp_rank(*prev) && *cp > *prev => {}
            _ => {
                best.insert(*g, *cp);
            }
        }
    }
    let mut tu = String::new();
    tu.push_str("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
    tu.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    tu.push_str("/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n");
    tu.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    let entries: Vec<(u16, u32)> = best.iter().map(|(g, cp)| (*g, *cp)).collect();
    for chunk in entries.chunks(100) {
        tu.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, cp) in chunk {
            tu.push_str(&format!("<{g:04X}> <{cp:04X}>\n"));
        }
        tu.push_str("endbfchar\n");
    }
    tu.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    tu
}

// ---------------------------------------------------------------------------
// PDF 对象树（Type0/CIDFontType2 + 页树 + 内容流）
// ---------------------------------------------------------------------------

fn build_pdf(
    face: &TtfFace,
    font_bytes: &[u8],
    lines: &[String],
    lines_per_page: usize,
    layout: &PageLayout,
    skipped: usize,
) -> Option<GeneratedPdf> {
    if lines.is_empty() {
        return None;
    }
    let mut doc = Document::with_version("1.5");

    // FontFile2。
    let font_stream = Stream::new(
        dictionary! { "Length1" => Object::Integer(font_bytes.len() as i64) },
        font_bytes.to_vec(),
    );
    let font_file_id = doc.add_object(font_stream);

    // FontDescriptor（head/hhea 实测度量，1000/em 归一）。
    let scale = 1000.0 / face.units_per_em as f64;
    let desc_id = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(b"EmbeddedCJK".to_vec()),
        "Flags" => 4,
        "FontBBox" => Object::Array(
            [face.bbox.0, face.bbox.1, face.bbox.2, face.bbox.3]
                .iter()
                .map(|v| Object::Integer((*v as f64 * scale) as i64))
                .collect()
        ),
        "ItalicAngle" => 0,
        "Ascent" => Object::Integer((face.ascent as f64 * scale) as i64),
        "Descent" => Object::Integer((face.descent as f64 * scale) as i64),
        "CapHeight" => Object::Integer((face.ascent as f64 * scale) as i64),
        "StemV" => 80,
        "FontFile2" => Object::Reference(font_file_id),
    });

    // ToUnicode。
    let tu_stream = Stream::new(dictionary! {}, to_unicode_cmap(face).into_bytes());
    let tu_id = doc.add_object(tu_stream);

    // CIDFontType2（Identity：CID=GID；DW=1000 全角近似——R2 hmtx 精宽）。
    let mut cid = Dictionary::new();
    cid.set("Type", "Font");
    cid.set("Subtype", "CIDFontType2");
    cid.set("BaseFont", Object::Name(b"EmbeddedCJK".to_vec()));
    let mut csi = Dictionary::new();
    csi.set(
        "Registry",
        Object::String(b"Adobe".to_vec(), lopdf::StringFormat::Literal),
    );
    csi.set(
        "Ordering",
        Object::String(b"Identity".to_vec(), lopdf::StringFormat::Literal),
    );
    csi.set("Supplement", 0);
    cid.set("CIDSystemInfo", csi);
    cid.set("FontDescriptor", Object::Reference(desc_id));
    cid.set("DW", 1000);
    cid.set("CIDToGIDMap", "Identity");
    let cid_id = doc.add_object(cid);

    // Type0。
    let mut t0 = Dictionary::new();
    t0.set("Type", "Font");
    t0.set("Subtype", "Type0");
    t0.set("BaseFont", Object::Name(b"EmbeddedCJK".to_vec()));
    t0.set("Encoding", "Identity-H");
    t0.set(
        "DescendantFonts",
        Object::Array(vec![Object::Reference(cid_id)]),
    );
    t0.set("ToUnicode", Object::Reference(tu_id));
    let t0_id = doc.add_object(t0);

    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(t0_id) },
    });

    // 页树 + 内容流。
    let mut page_ids: Vec<ObjectId> = Vec::new();
    for page_lines in lines.chunks(lines_per_page.max(1)) {
        let mut ops = String::new();
        ops.push_str("BT\n");
        for (i, hex) in page_lines.iter().enumerate() {
            if hex.is_empty() {
                continue;
            }
            let y = A4_H_PT - layout.margin_pt - layout.font_size_pt - i as f64 * layout.leading_pt;
            ops.push_str(&format!(
                "/F1 {fs:.1} Tf 1 0 0 1 {m:.1} {y:.1} Tm <{hex}> Tj\n",
                fs = layout.font_size_pt,
                m = layout.margin_pt,
            ));
        }
        ops.push_str("ET\n");
        let content = Stream::new(dictionary! {}, ops.into_bytes());
        let content_id = doc.add_object(content);
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => Object::Array(vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(A4_W_PT as i64), Object::Integer(A4_H_PT as i64),
            ]),
            "Contents" => Object::Reference(content_id),
            "Resources" => Object::Reference(resources_id),
        });
        page_ids.push(page_id);
    }
    let kids: Vec<Object> = page_ids.iter().map(|id| Object::Reference(*id)).collect();
    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => Object::Array(kids),
        "Count" => Object::Integer(page_ids.len() as i64),
    });
    for id in &page_ids {
        doc.get_dictionary_mut(*id)
            .unwrap()
            .set("Parent", Object::Reference(pages_id));
    }
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("lopdf save");

    Some(GeneratedPdf {
        bytes: buf,
        pages: page_ids.len(),
        skipped_chars: skipped,
    })
}

// ---------------------------------------------------------------------------
// 测试（子集夹具——康熙部首共享对锁 rank 回归）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Vec<u8> {
        std::fs::read("tests/fixtures/q31_test_font.ttf").expect("subset fixture")
    }

    #[test]
    fn a4_single_page_and_font_tree() {
        let text = "第一章 招标公告\n本项目为智能文档处理平台建设项目。";
        let out = generate_text_pdf(&font(), text, &PageLayout::default()).unwrap();
        assert_eq!(out.pages, 1);
        assert_eq!(out.skipped_chars, 0);
        // 结构自证：lopdf 重读页数（ToUnicode 内容面由 rank 测试直测）。
        let doc = Document::load_from(std::io::Cursor::new(&out.bytes)).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
        assert!(
            out.bytes.len() > font().len(),
            "字体程序在产物内（FontFile2）"
        );
    }

    #[test]
    fn multi_page_pagination() {
        // 50 行短句：每 \n = 1 内容行 + 1 段界空行 → 100 行 @ 40 行/页
        // = 3 页（40+40+20——段界空行是折行语义的一部分）。
        let text: String = (0..50)
            .map(|i| format!("第{i}行测试文本\n"))
            .collect::<Vec<_>>()
            .join("");
        let out = generate_text_pdf(&font(), &text, &PageLayout::default()).unwrap();
        assert_eq!(out.pages, 3, "100 行（含段界）@ 40 行/页");
    }

    #[test]
    fn to_unicode_rank_kangxi_preferential() {
        // spike 地雷回归锁：一 U+4E00 与 ⼀ U+2F00 同 GID——ToUnicode
        // 必须选正区码位（4E00），选部首形则提取文本错字（召回跌）。
        let face = TtfFace::parse(&font()).unwrap();
        let g1 = face.chars.get(&0x4E00).copied();
        let gk = face.chars.get(&0x2F00).copied();
        assert_eq!(g1, gk, "夹具必须保留共享对（一/⼀同 GID）");
        let tu = to_unicode_cmap(&face);
        assert!(tu.contains("<4E00>") || tu.contains("<4E00"), "sanity");
        // rank 断言：映射到该 GID 的码位是 4E00 非 2F00。
        let line = tu
            .lines()
            .find(|l| l.starts_with(&format!("<{:04X}>", g1.unwrap())))
            .expect("gid entry");
        assert!(line.contains("<4E00>"), "选正区：{line}");
    }

    #[test]
    fn unmapped_chars_counted_not_silent() {
        // 「㵘」（U+3D58 扩展A）不在子集内：跳过 + 计数（其余字符
        // 均在子集——断言精确为 1）。
        let text = "文档㵘处理。";
        let out = generate_text_pdf(&font(), text, &PageLayout::default()).unwrap();
        assert_eq!(out.skipped_chars, 1, "降级显式计数");
    }

    #[test]
    fn determinism_bytes() {
        let text = "确定性验证文本：同输入逐字节同输出。\n第二章 技术要求。";
        let a = generate_text_pdf(&font(), text, &PageLayout::default()).unwrap();
        let b = generate_text_pdf(&font(), text, &PageLayout::default()).unwrap();
        assert_eq!(a.bytes, b.bytes, "同输入逐字节一致");
    }

    #[test]
    fn typed_errors() {
        assert!(matches!(
            generate_text_pdf(b"not a font", "x", &PageLayout::default()),
            Err(GenerateError::NotTrueType)
        ));
        assert!(matches!(
            generate_text_pdf(&font(), "   \n  ", &PageLayout::default()),
            Err(GenerateError::EmptyText)
        ));
    }
}
