//! S2-6（issue #116 任务 3）· 盖章/图片叠加——公章的半透明落点。
//!
//! # 为什么单开一个模块
//!
//! [`crate::annotate::PdfAnnotator`] 的 11 kind 都是**矢量标注**（高亮/
//! 墨迹/矩形…），没有「把一张图贴到页上」的能力。而真实标书提交最需要的
//! 恰恰是这一件：公章、法人手签的扫描图、骑缝章。
//!
//! # 为什么不用注释而用内容流
//!
//! 注释（/Annot）可以被阅读器隐藏、可以被删除、**打印时默认可能不被带上**
//! （取决于阅读器与打印设置）——对公章来说这是致命的：屏幕上看着盖了，
//! 打印出来没有。
//!
//! 所以本模块把图**直接画进页面内容流**（与正文同等地位），产物与打印件
//! 一致。代价是不可再编辑——这正是盖章想要的语义（盖了就是盖了）。
//!
//! # 半透明（公章的标准做法）
//!
//! 真实公章照片/PNG 通常带 alpha 通道，直接贴会把底下的文字盖死。本模块：
//!
//! - **PNG 带 alpha** → 用 `/SMask`（软蒙版）保留逐像素透明度；
//! - **PNG 无 alpha** → 走 `/CA`（恒定 alpha）——调用方给的 `opacity` 生效；
//! - **JPEG / 无 alpha 的 PNG** → 仍按不透明处理，`opacity < 1.0` 时用 `/CA`。
//!
//! 不支持「把 JPEG 抠成透明」——JPEG 没有 alpha，猜不出来，不假装能。
//!
//! # 零 image crate 纪律
//!
//! 与 `engine_pdfium` 渲染面同一条纪律：**不用 image crate**。PNG 自己做
//! 解码（8-bit RGB/RGBA/灰度 + 非隔行），JPEG 直接原样嵌入（PDF 的
//! DCTDecode 就是 JPEG，连解码都不用——比转码更忠实）。

use crate::security::Limits;
use lopdf::{dictionary, Document, Object, Stream};

// ---------------------------------------------------------------------------
// 错误面
// ---------------------------------------------------------------------------

/// 盖章错误（窄面——不污染 [`crate::command::CommandError`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampError {
    /// 图片格式不支持/解码失败。
    BadImage(String),
    /// 页面越界。
    BadPage(u32),
    /// 矩形非法（零面积/非有限）。
    BadRect,
    /// 文档装载/保存失败。
    Io(String),
    /// 不支持的加密文档（#277 / 审计 P1-11：S5 纪律——加密写拒，与
    /// [`crate::forms`] / [`crate::writer_lopdf`] 同门哲学，判定取
    /// 后者强形态 `is_encrypted() || was_encrypted()`——文件本体是
    /// 加密档即拒（v0 S5 = 加密档零写）。此前对加密档盖章会静默
    /// 损坏：仍加密态 lopdf 以未解密形态装载、对象图不解析，保存
    /// 出的产物既非合法修订也非原档语义；曾加密态（空口令自动
    /// 解密）则静默剥离加密层——安全降级不可默许。
    Encrypted,
    /// 输入超过 S3-L7 上限（open 家族入口统一——先于一切解析）。
    TooLarge,
    /// 已签名档守卫（#408：盖章是全量重写，保存即毁签）。默认 fail-closed
    /// 拒盖；显式放行 [`StampOptions::allow_break_signature`]（放行后
    /// 签名确实失效——如实发生，调用方负责向用户揭示后果）。
    ///
    /// 对比：[`crate::writer_lopdf`] 增量写保签；[`crate::organize`]
    /// 全量重写但在 organize.rs 头部如实声明「签名档不验签」；stamp
    /// 此前两者皆无——本门补齐。判定三路（`doc_declares_signature`）：
    /// /Perms /DocMDP 在位、/AcroForm /SigFlags 置位、字段表含 /FT /Sig，
    /// 任一命中即拒。宽拒窄放：误拒（结构标记在但无实效签名）由调用
    /// 方显式放行兑底；误放（真签名档静默毁签）不可逆。
    SignedDocument,
}

impl std::fmt::Display for StampError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StampError::BadImage(e) => write!(f, "图片无法嵌入：{e}"),
            StampError::BadPage(p) => write!(f, "页号 {p} 越界"),
            StampError::BadRect => write!(f, "盖章矩形非法（零面积或非有限坐标）"),
            StampError::Io(e) => write!(f, "文档操作失败：{e}"),
            StampError::Encrypted => write!(f, "文档已加密（盖章被拒——S5 纪律：加密档零写）"),
            StampError::TooLarge => write!(f, "输入字节超过 S3-L7 上限（盖章被拒）"),
            StampError::SignedDocument => write!(
                f,
                "文档已签名（盖章被拒——全量重写将毁签；确需盖章置 allow_break_signature 显式放行）"
            ),
        }
    }
}

impl std::error::Error for StampError {}

// ---------------------------------------------------------------------------
// 图片解码（零 image 依赖）
// ---------------------------------------------------------------------------

/// 解码后的位图（PDF 可直接消费的形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    /// 像素宽度。
    pub width: u32,
    /// 像素高度。
    pub height: u32,
    /// 色彩分量数（1=灰度 3=RGB）。
    pub components: u8,
    /// 位深（恒 8——16-bit PNG 会降采样，见实现说明）。
    pub bits: u8,
    /// 原始像素字节（逐行、无过滤——已解压并去过滤）。
    pub data: Vec<u8>,
    /// 逐像素 alpha（长度 = width*height；无 alpha 图为 None）。
    pub alpha: Option<Vec<u8>>,
}

/// 识别并解码图片（PNG / JPEG）。
pub fn decode_image(bytes: &[u8]) -> Result<DecodedImage, StampError> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        decode_png(bytes)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Err(StampError::BadImage(
            "JPEG 无需解码（可直接以 DCTDecode 嵌入）——请用 embed_jpeg 路径".into(),
        ))
    } else {
        Err(StampError::BadImage(
            "无法识别的图片格式（支持 PNG / JPEG）".into(),
        ))
    }
}

/// PNG 解码（8-bit 灰度 / RGB / RGBA，非隔行；16-bit 降采样到 8-bit）。
fn decode_png(bytes: &[u8]) -> Result<DecodedImage, StampError> {
    let bad = |m: &str| StampError::BadImage(m.to_string());
    let mut pos = 8usize; // 跳过签名
    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 8u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut palette: Option<Vec<[u8; 3]>> = None;
    let mut trns: Option<Vec<u8>> = None;
    let mut idat: Vec<u8> = Vec::new();

    while pos + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let tag = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(len)
            .ok_or_else(|| bad("chunk 长度溢出"))?;
        if data_end > bytes.len() {
            return Err(bad("chunk 越界（文件截断？）"));
        }
        let data = &bytes[data_start..data_end];
        match tag {
            b"IHDR" => {
                if data.len() < 13 {
                    return Err(bad("IHDR 长度不足"));
                }
                width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                bit_depth = data[8];
                color_type = data[9];
                interlace = data[12];
            }
            b"PLTE" => {
                palette = Some(data.as_chunks::<3>().0.to_vec());
            }
            b"tRNS" => trns = Some(data.to_vec()),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        pos = data_end + 4; // 跳过 CRC
    }

    if width == 0 || height == 0 {
        return Err(bad("IHDR 缺失或尺寸为零"));
    }
    if interlace != 0 {
        return Err(bad("不支持隔行扫描（Adam7）PNG——请另存为非隔行"));
    }
    if bit_depth != 8 && bit_depth != 16 {
        return Err(bad(&format!("不支持位深 {bit_depth}（仅 8/16）")));
    }
    let src_components: u8 = match color_type {
        0 => 1, // 灰度
        2 => 3, // RGB
        4 => 2, // 灰度 + alpha
        6 => 4, // RGBA
        3 => 1, // 调色板（索引）
        t => return Err(bad(&format!("不支持 color type {t}"))),
    };
    if color_type == 3 && palette.is_none() {
        return Err(bad("调色板 PNG 缺 PLTE"));
    }

    let raw = inflate_zlib(&idat)?;
    let bpp = (src_components as usize) * (bit_depth as usize / 8);
    let stride = width as usize * bpp;
    let expected = (stride + 1) * height as usize;
    if raw.len() < expected {
        return Err(bad("解压后像素数据不足"));
    }

    // 去过滤（PNG 五种过滤器）
    let mut out = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let ft = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..y * (stride + 1) + 1 + stride];
        let (prev_rows, cur_rows) = out.split_at_mut(y * stride);
        let prev = if y == 0 {
            &[][..]
        } else {
            &prev_rows[(y - 1) * stride..]
        };
        let cur = &mut cur_rows[..stride];
        cur.copy_from_slice(line);
        unfilter(ft, cur, prev, bpp)?;
    }

    // 归一化：灰度/RGB/alpha → components + alpha 分离；16-bit 取高字节
    let px16 = bit_depth == 16;
    let step = if px16 { 2 } else { 1 };
    let npix = (width as usize) * (height as usize);
    let mut components: u8 = 0;
    let mut data = Vec::with_capacity(npix * 4);
    let mut alpha: Option<Vec<u8>> = if trns.is_some() {
        Some(vec![255u8; npix])
    } else {
        None
    };

    for i in 0..npix {
        let base = i * bpp;
        let get = |k: usize| -> u8 { out[base + k * step] };
        match color_type {
            0 => {
                components = 1.max(components);
                data.push(get(0));
                if let Some(t) = &trns {
                    let tv = t.first().copied().unwrap_or(0);
                    if get(0) == tv {
                        alpha.as_mut().unwrap()[i] = 0;
                    }
                }
            }
            4 => {
                components = 1.max(components);
                data.push(get(0));
                let a = get(1);
                let al = alpha.get_or_insert_with(|| vec![255u8; npix]);
                al[i] = a;
            }
            2 => {
                components = 3;
                data.extend_from_slice(&[get(0), get(1), get(2)]);
            }
            6 => {
                components = 3;
                data.extend_from_slice(&[get(0), get(1), get(2)]);
                let a = get(3);
                let al = alpha.get_or_insert_with(|| vec![255u8; npix]);
                al[i] = a;
            }
            3 => {
                components = 3;
                let idx = get(0) as usize;
                let pal = palette.as_ref().unwrap();
                let c = pal.get(idx).copied().unwrap_or([0, 0, 0]);
                data.extend_from_slice(&c);
                if let Some(t) = &trns {
                    if let Some(&a) = t.get(idx) {
                        let al = alpha.get_or_insert_with(|| vec![255u8; npix]);
                        al[i] = a;
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    // 全不透明的 alpha 视为无（不写无意义的 SMask）
    if let Some(a) = &alpha {
        if a.iter().all(|&v| v == 255) {
            alpha = None;
        }
    }

    Ok(DecodedImage {
        width,
        height,
        components,
        bits: 8,
        data,
        alpha,
    })
}

/// PNG 反过滤（ft: 0 None / 1 Sub / 2 Up / 3 Average / 4 Paeth）。
fn unfilter(ft: u8, cur: &mut [u8], prev: &[u8], bpp: usize) -> Result<(), StampError> {
    let n = cur.len();
    match ft {
        0 => {}
        1 => {
            for i in bpp..n {
                cur[i] = cur[i].wrapping_add(cur[i - bpp]);
            }
        }
        2 => {
            if !prev.is_empty() {
                for i in 0..n {
                    cur[i] = cur[i].wrapping_add(prev[i]);
                }
            }
        }
        3 => {
            for i in 0..n {
                let left = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                let up = if prev.is_empty() { 0 } else { prev[i] as u16 };
                cur[i] = cur[i].wrapping_add(((left + up) / 2) as u8);
            }
        }
        4 => {
            for i in 0..n {
                let a = if i >= bpp { cur[i - bpp] as i16 } else { 0 };
                let b = if prev.is_empty() { 0 } else { prev[i] as i16 };
                let c = if i >= bpp && !prev.is_empty() {
                    prev[i - bpp] as i16
                } else {
                    0
                };
                let p = a + b - c;
                let pa = (p - a).abs();
                let pb = (p - b).abs();
                let pc = (p - c).abs();
                let pred = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                cur[i] = cur[i].wrapping_add(pred as u8);
            }
        }
        other => {
            return Err(StampError::BadImage(format!("未知 PNG 过滤器 {other}")));
        }
    }
    Ok(())
}

/// zlib 解压（PNG IDAT）——用 lopdf 已引入的 flate2 不成立（它依赖
/// lopdf 特性），这里走 std 之外的最小实现：调用 flate2 由调用方 crate
/// 提供，本 crate 目前无直接依赖 → 用一个**只认 stored/deflate 的**
/// 解析器会太脆。因此改为经 lopdf 暴露的 flate2（见 Cargo 说明）。
fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, StampError> {
    // lopdf 的 flate2 是其内部依赖，本 crate 未直接声明 → 统一在此处
    // 说明并改用 `lopdf::Stream::new` 的 decompress 面（见 embed 路径）。
    // 为避免解析歧义，PNG 解压走 crate 内 `flate` 模块。
    crate::stamp::flate::zlib_inflate(data).map_err(StampError::BadImage)
}

// ---------------------------------------------------------------------------
// 最小 zlib 解压（stored + fixed + dynamic Huffman）
// ---------------------------------------------------------------------------

/// 最小 zlib/DEFLATE 解压实现。
///
/// 为什么自己做：本 crate 的依赖纪律是「零 image crate」，而 flate2 是
/// lopdf 的内部依赖（未在本 crate 直接声明）。PNG 的 IDAT 必须是 zlib，
/// 没有它读不了任何真实公章 PNG。三百行不到的 inflate 比引入一个
/// 新三方依赖更符合本 crate 的既有纪律（`engine_pdfium` 渲染面同理不
/// 引 image）。
pub(crate) mod flate {
    /// zlib 包装（2 字节头 + deflate + 4 字节 adler32）。
    pub fn zlib_inflate(data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 6 {
            return Err("zlib 数据过短".into());
        }
        if data[0] & 0x0F != 8 {
            return Err(format!("非 deflate 压缩方法：{}", data[0] & 0x0F));
        }
        let header = data[0] as u16 * 256 + data[1] as u16;
        if !header.is_multiple_of(31) {
            return Err("zlib 头校验失败".into());
        }
        let mut out = Vec::new();
        inflate_deflate(&data[2..data.len().saturating_sub(4)], &mut out)?;
        Ok(out)
    }

    struct Bits<'a> {
        data: &'a [u8],
        pos: usize,
        bit: u32,
    }

    impl<'a> Bits<'a> {
        fn new(data: &'a [u8]) -> Self {
            Bits {
                data,
                pos: 0,
                bit: 0,
            }
        }
        fn get(&mut self, n: u32) -> Result<u32, String> {
            let mut v = 0u32;
            for i in 0..n {
                let byte = *self.data.get(self.pos).ok_or("deflate 数据提前结束")?;
                let b = (byte >> self.bit) & 1;
                v |= (b as u32) << i;
                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                    self.pos += 1;
                }
            }
            Ok(v)
        }
        fn align(&mut self) {
            if self.bit != 0 {
                self.bit = 0;
                self.pos += 1;
            }
        }
    }

    struct Huff {
        counts: [u16; 16],
        symbols: Vec<u16>,
    }

    impl Huff {
        fn new(lengths: &[u8]) -> Self {
            let mut counts = [0u16; 16];
            for &l in lengths {
                counts[l as usize] += 1;
            }
            counts[0] = 0;
            let mut offs = [0u16; 16];
            for i in 1..16 {
                offs[i] = offs[i - 1] + counts[i - 1];
            }
            let mut symbols = vec![0u16; lengths.len()];
            for (sym, &l) in lengths.iter().enumerate() {
                if l != 0 {
                    symbols[offs[l as usize] as usize] = sym as u16;
                    offs[l as usize] += 1;
                }
            }
            Huff { counts, symbols }
        }

        fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, String> {
            let mut code = 0i32;
            let mut first = 0i32;
            let mut index = 0i32;
            for len in 1..16 {
                code |= bits.get(1)? as i32;
                let count = self.counts[len] as i32;
                if code - count < first {
                    let i = (index + (code - first)) as usize;
                    return self
                        .symbols
                        .get(i)
                        .copied()
                        .ok_or_else(|| "符号越界".to_string());
                }
                index += count;
                first = (first + count) << 1;
                code <<= 1;
            }
            Err("无效 Huffman 码".into())
        }
    }

    fn inflate_deflate(input: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
        let mut bits = Bits::new(input);
        loop {
            let last = bits.get(1)?;
            let btype = bits.get(2)?;
            match btype {
                0 => {
                    bits.align();
                    let len = (bits.get(16)?) as usize;
                    let _nlen = bits.get(16)?;
                    for _ in 0..len {
                        out.push(bits.get(8)? as u8);
                    }
                }
                1 => {
                    let mut lit = [0u8; 288];
                    for (i, v) in lit.iter_mut().enumerate() {
                        *v = if i < 144 {
                            8
                        } else if i < 256 {
                            9
                        } else if i < 280 {
                            7
                        } else {
                            8
                        };
                    }
                    let lit_h = Huff::new(&lit);
                    let dist_h = Huff::new(&[5u8; 30]);
                    inflate_block(&mut bits, out, &lit_h, &dist_h)?;
                }
                2 => {
                    let hlit = bits.get(5)? as usize + 257;
                    let hdist = bits.get(5)? as usize + 1;
                    let hclen = bits.get(4)? as usize + 4;
                    const ORDER: [usize; 19] = [
                        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                    ];
                    let mut cl = [0u8; 19];
                    for i in 0..hclen {
                        cl[ORDER[i]] = bits.get(3)? as u8;
                    }
                    let cl_h = Huff::new(&cl);
                    let mut lengths = Vec::with_capacity(hlit + hdist);
                    while lengths.len() < hlit + hdist {
                        let sym = cl_h.decode(&mut bits)?;
                        match sym {
                            0..=15 => lengths.push(sym as u8),
                            16 => {
                                let prev = *lengths.last().ok_or("重复码无前值")?;
                                let rep = bits.get(2)? + 3;
                                for _ in 0..rep {
                                    lengths.push(prev);
                                }
                            }
                            17 => {
                                let rep = bits.get(3)? + 3;
                                lengths.resize(lengths.len() + rep as usize, 0);
                            }
                            18 => {
                                let rep = bits.get(7)? + 11;
                                lengths.resize(lengths.len() + rep as usize, 0);
                            }
                            _ => return Err("无效重复码".into()),
                        }
                    }
                    let lit_h = Huff::new(&lengths[..hlit]);
                    let dist_h = Huff::new(&lengths[hlit..]);
                    inflate_block(&mut bits, out, &lit_h, &dist_h)?;
                }
                _ => return Err("无效 block type 3".into()),
            }
            if last == 1 {
                break;
            }
        }
        Ok(())
    }

    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    fn inflate_block(
        bits: &mut Bits<'_>,
        out: &mut Vec<u8>,
        lit: &Huff,
        dist: &Huff,
    ) -> Result<(), String> {
        loop {
            let sym = lit.decode(bits)?;
            match sym {
                0..=255 => out.push(sym as u8),
                256 => break,
                257..=285 => {
                    let i = (sym - 257) as usize;
                    let len = LEN_BASE[i] as usize + bits.get(LEN_EXTRA[i] as u32)? as usize;
                    let dsym = dist.decode(bits)? as usize;
                    if dsym >= DIST_BASE.len() {
                        return Err("无效距离码".into());
                    }
                    let d = DIST_BASE[dsym] as usize + bits.get(DIST_EXTRA[dsym] as u32)? as usize;
                    if d > out.len() {
                        return Err("回溯距离超出输出".into());
                    }
                    let start = out.len() - d;
                    for k in 0..len {
                        let b = out[start + k];
                        out.push(b);
                    }
                }
                _ => return Err("无效字面码".into()),
            }
        }
        Ok(())
    }
}
// ---------------------------------------------------------------------------
// 嵌入 + 盖章
// ---------------------------------------------------------------------------

/// 图片放不进文档的形态（JPEG 原样 / PNG 解码后）。
enum ImageSource {
    /// JPEG 字节（DCTDecode 直接嵌入，不解码——比转码更忠实）。
    Jpeg(Vec<u8>),
    /// PNG 解码结果。
    Raw(DecodedImage),
}

/// 识别图片并按最省路径准备。
fn classify(bytes: &[u8]) -> Result<ImageSource, StampError> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Ok(ImageSource::Raw(decode_png(bytes)?));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return Ok(ImageSource::Jpeg(bytes.to_vec()));
    }
    Err(StampError::BadImage(
        "无法识别的图片格式（支持 PNG / JPEG）".into(),
    ))
}

/// 构造图片 XObject（含 SMask / CA 透明度面）。
///
/// `opacity` ∈ (0,1]：无 alpha 图用 `/CA`；有 alpha 图由 `/SMask` 逐像素
/// 精确表达，`opacity` 再乘进 alpha（两者可同时生效，符合阅读器语义）。
/// 构造图片 XObject（占位——SMask 由调用方作为**独立对象**挂接）。
///
/// 返回 `(主图流, 可选软蒙版流)`。**SMask 必须是间接引用**：内联成
/// ``/SMask<<...>>`` 会让 MuPDF/Acrobat 报 "invalid key in dict" 且图不显示
/// （实测踩坑——pymupdf 交叉验证抓到的）。
fn image_xobject(src: &ImageSource, opacity: f32) -> (Stream, Option<Stream>) {
    match src {
        ImageSource::Jpeg(bytes) => {
            let (w, h, comps) = jpeg_dimensions(bytes).unwrap_or((1, 1, 3));
            let cs = if comps == 1 {
                "DeviceGray"
            } else {
                "DeviceRGB"
            };
            let mut s = Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => w as i64,
                    "Height" => h as i64,
                    "ColorSpace" => cs,
                    "BitsPerComponent" => 8,
                    "Filter" => "DCTDecode",
                },
                bytes.clone(),
            );
            if opacity < 1.0 {
                s.dict.set("CA", Object::Real(opacity));
            }
            (s, None)
        }
        ImageSource::Raw(img) => {
            let cs = if img.components == 1 {
                "DeviceGray"
            } else {
                "DeviceRGB"
            };
            let mut s = Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => img.width as i64,
                    "Height" => img.height as i64,
                    "ColorSpace" => cs,
                    "BitsPerComponent" => 8,
                },
                img.data.clone(),
            );
            if let Some(a) = &img.alpha {
                let scaled: Vec<u8> = if opacity < 1.0 {
                    a.iter().map(|&v| ((v as f32) * opacity) as u8).collect()
                } else {
                    a.clone()
                };
                let smask = Stream::new(
                    dictionary! {
                        "Type" => "XObject",
                        "Subtype" => "Image",
                        "Width" => img.width as i64,
                        "Height" => img.height as i64,
                        "ColorSpace" => "DeviceGray",
                        "BitsPerComponent" => 8,
                    },
                    scaled,
                );
                (s, Some(smask))
            } else {
                if opacity < 1.0 {
                    s.dict.set("CA", Object::Real(opacity));
                }
                (s, None)
            }
        }
    }
}
/// JPEG 尺寸与分量数（扫 SOF 段）。
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // SOF0..SOF15（排除 DHT=C4 / JPG=C8 / DAC=CC）
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            let comps = bytes[i + 9];
            return Some((w, h, comps));
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        i += 2 + len;
    }
    None
}

/// 盖章参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StampOptions {
    /// 透明度 0.0~1.0（公章常见 0.6~0.85）。1.0 = 完全不透明。
    pub opacity: f32,
    /// #408：显式允许在已签名档上盖章（默认 `false`——fail-closed）。
    ///
    /// 盖章是**全量重写**：放行后签名必然失效（如实发生，不是「尽量
    /// 保留」）。此开关是桌面确认框「仍然盖章（签名将失效）」的引擎
    /// 侧锚点——调用方必须先把后果呈现给用户，再置位。
    pub allow_break_signature: bool,
}

impl Default for StampOptions {
    fn default() -> Self {
        StampOptions {
            opacity: 1.0,
            allow_break_signature: false,
        }
    }
}

// ---------------------------------------------------------------------------
// #408 签名门判定
// ---------------------------------------------------------------------------

/// 把对象解析成字典（直接字典 / 间接引用两形态——AcroForm 与 /Perms
/// 两形态都常见，与 organize.rs 字段摘除同款宽容）。
fn as_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a lopdf::Dictionary> {
    match obj {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => match doc.get_object(*id) {
            Ok(Object::Dictionary(d)) => Some(d),
            _ => None,
        },
        _ => None,
    }
}

/// 档是否声明了签名纪律（#408 守卫的判定面，三路任一命中即 true）：
///
/// 1. catalog `/Perms /DocMDP` 在位（DocMDP 变换 = 签名在场的规范标记）；
/// 2. `/AcroForm /SigFlags` bit0（SignaturesExist）置位；
/// 3. `/AcroForm /Fields`（含 /Kids 递归）里有 `/FT /Sig` 域——兑底：
///    部分生成器不写 SigFlags（`fixtures/signed_sample.pdf` 即是）。
///
/// 宽拒窄放：误拒（无实效签名但结构标记在）由调用方显式放行兑底；
/// 误放（真签名档静默毁签）不可逆。
fn doc_declares_signature(doc: &Document) -> bool {
    let Ok(catalog) = doc.catalog() else {
        return false;
    };
    // ① /Perms /DocMDP（直接字典 / 间接引用两形态）。
    if let Ok(perms) = catalog.get(b"Perms") {
        if let Some(p) = as_dict(doc, perms) {
            if p.get(b"DocMDP").is_ok() {
                return true;
            }
        }
    }
    // ②③ /AcroForm。
    let Ok(form_obj) = catalog.get(b"AcroForm") else {
        return false;
    };
    let Some(form) = as_dict(doc, form_obj) else {
        return false;
    };
    if let Ok(Object::Integer(flags)) = form.get(b"SigFlags") {
        if *flags & 1 != 0 {
            return true;
        }
    }
    if let Ok(Object::Array(fields)) = form.get(b"Fields") {
        for f in fields {
            if field_tree_has_sig(doc, f, 0) {
                return true;
            }
        }
    }
    false
}

/// 单个字段（含 /Kids 子树，限深 16 防环）是否为 `/FT /Sig`。
fn field_tree_has_sig(doc: &Document, node: &Object, depth: u8) -> bool {
    if depth > 16 {
        return false;
    }
    let Some(d) = as_dict(doc, node) else {
        return false;
    };
    if matches!(d.get(b"FT"), Ok(Object::Name(n)) if n == b"Sig") {
        return true;
    }
    if let Ok(Object::Array(kids)) = d.get(b"Kids") {
        return kids.iter().any(|k| field_tree_has_sig(doc, k, depth + 1));
    }
    false
}

/// 给 PDF 盖章（把图片画进页面内容流——与正文同等地位，打印必带）。
///
/// `rect` = `[x0, y0, x1, y1]`（PDF 用户坐标，左下原点）。
/// 图片按 rect 拉伸铺满（调用方自行保证宽高比；公章是正方形，通常无损）。
///
/// 安全门（#277 / 审计 P1-11，open 家族统一）：S3-L7 输入字节 →
/// S5 加密双拒（仍加密 || 曾加密）——详见 [`stamp_image_with_limits`]。
pub fn stamp_image(
    pdf: Vec<u8>,
    page: u32,
    rect: [f32; 4],
    image: &[u8],
    opts: StampOptions,
) -> Result<Vec<u8>, StampError> {
    stamp_image_with_limits(pdf, page, rect, image, opts, Limits::default())
}

/// 带限额盖章（[`stamp_image`] 的注入面——限额面确定性测试用，与
/// [`crate::forms::PdfForm::read_with_limits`] 同款双入口纪律）。
///
/// 门序（open 家族入口统一——与 [`crate::security_scan::open_scan`] /
/// [`crate::writer_lopdf::LopdfWriter::from_bytes`] 同序）：
/// 1. **S3-L7 输入字节**❶——先于一切解析；
/// 2. 参数校验（透明度/矩形——纯参数面零解析，不耗预算）；
/// 3. lopdf 装载 → **S5 加密双拒**（仍加密 || 曾加密——加密档零写）；
/// 4. **#408 签名门**——`doc_declares_signature` 命中且未显式放行 →
///    [`StampError::SignedDocument`]（全量重写毁签，fail-closed）；
/// 5. 页面定位与写面。
pub fn stamp_image_with_limits(
    pdf: Vec<u8>,
    page: u32,
    rect: [f32; 4],
    image: &[u8],
    opts: StampOptions,
    limits: Limits,
) -> Result<Vec<u8>, StampError> {
    // S3-L7 输入门（先于装载——open 家族入口统一检查）。
    limits
        .check_open_input_bytes(pdf.len() as u64)
        .map_err(|_| StampError::TooLarge)?;
    if !(0.0..=1.0).contains(&opts.opacity) || opts.opacity <= 0.0 {
        return Err(StampError::BadImage(format!(
            "透明度 {} 越界（应在 (0, 1]）",
            opts.opacity
        )));
    }
    let w = (rect[2] - rect[0]).abs();
    let h = (rect[3] - rect[1]).abs();
    if !w.is_finite() || !h.is_finite() || w < 0.01 || h < 0.01 {
        return Err(StampError::BadRect);
    }

    let mut doc = Document::load_mem(&pdf).map_err(|e| StampError::Io(format!("装载失败：{e}")))?;
    // S5 加密写拒（双拒——与 writer_lopdf 同判定，#277 / 审计 P1-11）：
    // - **仍加密**：lopdf 未解密装载、对象图不解析——盖章保存出的产物
    //   既非合法修订也非原档语义（静默损坏，本 issue 的原始形态）；
    // - **曾加密**（空口令自动解密）：对象图可用，但保存 = 静默剥离
    //   加密层（安全降级）——文件本体是加密档即拒（v0 S5）。
    if doc.is_encrypted() || doc.was_encrypted() {
        return Err(StampError::Encrypted);
    }
    // #408 签名门（S5 之后第二道写前守卫）：盖章 = 全量重写，保存即毁签
    // ——对比 writer_lopdf 增量写保签。默认拒；显式放行 = 调用方已向
    // 用户揭示「签名将失效」（与 organize 头部「声明毁签」同哲学，
    // 但 stamp 此前连声明都没有——本门补齐）。
    if !opts.allow_break_signature && doc_declares_signature(&doc) {
        return Err(StampError::SignedDocument);
    }
    let pages = doc.get_pages();
    let page_id = *pages.get(&(page + 1)).ok_or(StampError::BadPage(page))?;

    let src = classify(image)?;
    let (mut xobj, smask) = image_xobject(&src, opts.opacity);
    // SMask 必须先入对象表再以引用挂接（内联字典 MuPDF/Acrobat 不认）
    if let Some(sm) = smask {
        let sm_id = doc.add_object(sm);
        xobj.dict.set("SMask", Object::Reference(sm_id));
    }
    let xobj_id = doc.add_object(xobj);

    // 页资源挂 /XObject /Im0
    {
        let page_dict = doc
            .get_dictionary_mut(page_id)
            .map_err(|e| StampError::Io(format!("页对象读取失败：{e}")))?;
        let mut res = match page_dict.get(b"Resources").ok() {
            Some(Object::Dictionary(d)) => d.clone(),
            _ => lopdf::Dictionary::new(),
        };
        let mut xo = match res.get(b"XObject").ok() {
            Some(Object::Dictionary(d)) => d.clone(),
            _ => lopdf::Dictionary::new(),
        };
        xo.set(b"Im0".to_vec(), Object::Reference(xobj_id));
        res.set(b"XObject".to_vec(), Object::Dictionary(xo));
        page_dict.set(b"Resources".to_vec(), Object::Dictionary(res));
    }

    // 内容流：把图按 rect 铺开画（q/cm/Do/Q）
    let content = format!(
        "q\n{} 0 0 {} {} {} cm\n/Im0 Do\nQ\n",
        w, h, rect[0], rect[1]
    )
    .into_bytes();
    let stream_id = doc.add_object(Stream::new(dictionary! {}, content));
    {
        let page_dict = doc
            .get_dictionary_mut(page_id)
            .map_err(|e| StampError::Io(format!("页对象读取失败：{e}")))?;
        let existing = page_dict.get(b"Contents").ok().cloned();
        match existing {
            Some(Object::Array(mut arr)) => {
                arr.push(Object::Reference(stream_id));
                page_dict.set(b"Contents".to_vec(), Object::Array(arr));
            }
            Some(obj) => {
                page_dict.set(
                    b"Contents".to_vec(),
                    Object::Array(vec![obj, Object::Reference(stream_id)]),
                );
            }
            None => {
                page_dict.set(b"Contents".to_vec(), Object::Reference(stream_id));
            }
        }
    }

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| StampError::Io(format!("保存失败：{e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_rect_and_opacity() {
        assert_eq!(
            stamp_image(
                vec![],
                0,
                [0.0, 0.0, 0.0, 0.0],
                &[],
                StampOptions::default()
            ),
            Err(StampError::BadRect)
        );
        let opts = StampOptions {
            opacity: 1.5,
            ..Default::default()
        };
        assert!(matches!(
            stamp_image(vec![], 0, [0.0, 0.0, 10.0, 10.0], &[], opts),
            Err(StampError::BadImage(_))
        ));
    }

    #[test]
    fn unknown_image_format_is_typed_error() {
        let err = decode_image(b"not an image").unwrap_err();
        assert!(matches!(err, StampError::BadImage(_)));
        assert!(err.to_string().contains("无法识别"));
    }

    #[test]
    fn signature_gate_detection_branches() {
        // #408：doc_declares_signature 三路判定（SigFlags / /FT /Sig /
        // /Perms /DocMDP）——small.pdf 无签名基座，逐路注入目录标记
        // 验证（真夹具路径由 pdfh_stamp.rs 验收面覆盖）。
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/small.pdf"),
        )
        .expect("small.pdf 缺失");

        // 基座：无任何签名标记。
        let doc = Document::load_mem(&bytes).unwrap();
        assert!(!doc_declares_signature(&doc), "基座不应声明签名");

        // ② /AcroForm /SigFlags bit0（SignaturesExist；bit1 顺带在位）。
        let mut doc = Document::load_mem(&bytes).unwrap();
        doc.catalog_mut()
            .unwrap()
            .set("AcroForm", dictionary! { "SigFlags" => 3 });
        assert!(doc_declares_signature(&doc), "SigFlags 置位应命中");

        // ③ 字段表 /FT /Sig（SigFlags 缺失的生成器兜底——夹具即此形态）。
        let mut doc = Document::load_mem(&bytes).unwrap();
        doc.catalog_mut().unwrap().set(
            "AcroForm",
            dictionary! {
                "Fields" => Object::Array(vec![Object::Dictionary(
                    dictionary! { "FT" => "Sig" },
                )]),
            },
        );
        assert!(doc_declares_signature(&doc), "/FT /Sig 字段应命中");

        // ① /Perms /DocMDP（DocMDP 变换在场——P=2 形态）。
        let mut doc = Document::load_mem(&bytes).unwrap();
        doc.catalog_mut().unwrap().set(
            "Perms",
            dictionary! {
                "DocMDP" => dictionary! {
                    "TransformParams" => dictionary! { "P" => 2 },
                },
            },
        );
        assert!(doc_declares_signature(&doc), "/Perms /DocMDP 应命中");

        // 控制组：/FT /Tx（文本域）不命中——宽拒不等于见 AcroForm 就拒。
        let mut doc = Document::load_mem(&bytes).unwrap();
        doc.catalog_mut().unwrap().set(
            "AcroForm",
            dictionary! {
                "Fields" => Object::Array(vec![Object::Dictionary(
                    dictionary! { "FT" => "Tx" },
                )]),
            },
        );
        assert!(!doc_declares_signature(&doc), "/FT /Tx 不应命中");
    }
}
