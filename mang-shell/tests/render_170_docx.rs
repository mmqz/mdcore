//! #170 缺口面：docx 只读/可编辑渲染正确性——**图片不丢 / 空段落与
//! 空单元格不丢 / 嵌套（多表格 + sdt 容器）结构不拍平**。
//!
//! ## 为什么（与 render_fidelity.rs 的分工）
//!
//! `render_fidelity.rs` 已覆盖 smoke.docx（单表 4 行 26 段的生成器
//! 夹具）的结构对账 / 内容锚点 / CJK 保真。本文件补 issue #170 点名
//! 的三个缺口面，用**真实标书夹具** `bid-e2e/07_export.docx`
//! （ai-bid-research e2e 全链产物——多表格 + 空段 + 空单元格 + sdt
//! 包裹的 TOC 域，结构复杂度远超生成器夹具）：
//!
//! 1. **多表格不拍平**：六表（项目概况/技术偏离/人员/设备/业绩/报价）
//!    的 `props.tables`/`props.table_rows` ↔ document.xml 独立
//!    `w:tbl`/`w:tr` 计数对账（既有测试只钉过单表）；可编辑面再对
//!    表格侧表（`payload.tables`：表数 / 行合计 / 栅格格数合计）与
//!    独立 `w:tc` 计数对账。
//! 2. **空段落 / 空单元格不丢**：独立数 body 直下空段（文末 sectPr
//!    间隔段）与空单元格（21 格），对账 `props.empty_paragraphs`
//!    与行模型空行数——空内容占行（不丢行），格也不丢。
//! 3. **段落总账（不丢段 / 不拍平）**：可编辑装载的候选段落数、
//!    行块三轨（Para/Cell/Locked）与引擎段数、独立 `w:p` 总数四方
//!    对账——367 顶层段 + 242 单元格段 + 2 sdt 盲区段 == 611。
//!
//! ## 独立解析口径（不信引擎自报）
//!
//! 与 render_fidelity.rs 同款：极简 zip 扫描 + flate2 解压直接数 XML
//! 元素。本文件扩展两类扫描器（既有 `count_element` 口径数不到
//! **自闭合**标签——`a:blip`/`w:br` 恒自闭合，必须用精确标签名扫描）：
//!
//! - `count_tag`：精确标签名计数（`<w:p>`/`<w:p …>`/`<w:p/>` 全算）；
//! - `census_paragraphs`：按上下文（body 直下 / sdt 内 / 表格内）普查
//!   段落并数空段——**宽面块序实装口径**（表格单元格段不入顶层段流；
//!   `w:sdt` 内容宽面结构性丢弃——#156 sdt 盲区）。
//!
//! ## 图片面（#204 起双面：正向对账 + 零图守卫）
//!
//! `smoke_image.docx`（#204 带图黄金夹具——scripts/make_image_docx_
//! fixture.py 手构）承载**正向对账**（`docx_image_face_positive_
//! reconciliation_*`）：两内联图（纯图片段 + 文本混排段各一），
//! props.images ↔ 独立 a:blip ↔ word/media/ 部件三方对账 + 邻接内容
//! 不丢不崩（图片不入行流——空段占行、混排段文本保留）。
//! 两个旧夹具（smoke.docx / bid-e2e/07_export.docx）仍无图——零图
//! 守卫（`docx_image_face_gap_guard_*`）原样保留：防夹具漂移成带图件
//! 后三方口径悄悄失衡。
//!
//! ## 铁律
//!
//! - 只加测试，不改产线代码；夹具缺失即 FAIL（黄金例进仓，不许 SKIP）；
//! - 发现渲染缺陷 → 报告开 issue，不在本任务修、不放宽断言。

use std::path::{Path, PathBuf};

use mang_shell::editor::docx_doc;
use mang_shell::editor::docx_edit_bridge::{LineBlock, ReadOnlyLine};

// ---------------------------------------------------------------------------
// 夹具（黄金例进仓——缺失即 FAIL）
// ---------------------------------------------------------------------------

fn fixture(relative: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative);
    assert!(
        p.is_file(),
        "夹具缺失（{}）——黄金例进仓，不许 SKIP；bid-e2e 夹具由 \
         ai-bid-research e2e 再生成（见 tests/docx_bid_e2e.rs 头注释）",
        p.display()
    );
    p
}

fn fixture_bytes(relative: &str) -> Vec<u8> {
    std::fs::read(fixture(relative)).unwrap_or_else(|e| panic!("夹具不可读（{relative}）：{e}"))
}

// ---------------------------------------------------------------------------
// 独立解析工具（zip 扫描 + flate2 解压——复制自 render_fidelity.rs；
// 集成测试是独立 crate，不能 use 既有测试文件的私有函数，复制是本仓
// 认可模式。**独立于被测代码**——对账的独立性来源）
// ---------------------------------------------------------------------------

/// 极简 zip 条目扫描（EOCD + 中央目录 + 本地头三固定结构）。
/// 返回 (name, 解压后字节)。
fn zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let n = bytes.len();
    let mut eocd = None;
    let mut i = n.saturating_sub(22);
    loop {
        if bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            eocd = Some(i);
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    let eocd = eocd.expect("EOCD（截断/非 zip 件在此早死）");
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
    let cd_off = u32::from_le_bytes([
        bytes[eocd + 16],
        bytes[eocd + 17],
        bytes[eocd + 18],
        bytes[eocd + 19],
    ]) as usize;
    let mut out = Vec::new();
    let mut p = cd_off;
    for _ in 0..count {
        assert_eq!(
            &bytes[p..p + 4],
            &[0x50, 0x4b, 0x01, 0x02],
            "中央目录条目头"
        );
        let method = u16::from_le_bytes([bytes[p + 10], bytes[p + 11]]);
        let csize = u32::from_le_bytes([bytes[p + 20], bytes[p + 21], bytes[p + 22], bytes[p + 23]])
            as usize;
        let nlen = u16::from_le_bytes([bytes[p + 28], bytes[p + 29]]) as usize;
        let xlen = u16::from_le_bytes([bytes[p + 30], bytes[p + 31]]) as usize;
        let clen = u16::from_le_bytes([bytes[p + 32], bytes[p + 33]]) as usize;
        let lho = u32::from_le_bytes([bytes[p + 42], bytes[p + 43], bytes[p + 44], bytes[p + 45]])
            as usize;
        let name = String::from_utf8_lossy(&bytes[p + 46..p + 46 + nlen]).into_owned();
        let lnlen = u16::from_le_bytes([bytes[lho + 26], bytes[lho + 27]]) as usize;
        let lxlen = u16::from_le_bytes([bytes[lho + 28], bytes[lho + 29]]) as usize;
        let data_start = lho + 30 + lnlen + lxlen;
        let data = &bytes[data_start..data_start + csize];
        let plain = if method == 0 {
            data.to_vec()
        } else {
            inflate_mini(data)
        };
        out.push((name, plain));
        p += 46 + nlen + xlen + clen;
    }
    out
}

/// 迷你 inflate（deflate 流——flate2 测试面独占依赖）。
fn inflate_mini(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut decoder = flate2::read::DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    out
}

/// 取 zip 部件文本（UTF-8）。
fn part_text(bytes: &[u8], name: &str) -> String {
    let (_, plain) = zip_entries(bytes)
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("zip 部件 {name} 不在场"));
    String::from_utf8(plain).expect("部件 UTF-8")
}

/// 数 XML 元素出现次数（`<w:p ` / `<w:p>` 开标签形态——render_fidelity
/// 同口径。**自闭合 `<w:p/>` 不算**：自闭合恒现的标签用 [`count_tag`]）。
fn count_element(xml: &str, tag: &str) -> usize {
    xml.matches(&format!("<{tag} ")).count() + xml.matches(&format!("<{tag}>")).count()
}

// ---------------------------------------------------------------------------
// 本文件扩展扫描器（精确标签名——自闭合安全 + 前缀同形标签排除）
// ---------------------------------------------------------------------------

/// 标签精确开头判定：`rest` 以 `<tag` 开头且其后紧跟空白 / `>` / `/`。
///
/// 排除前缀同形标签：`<w:tblPr`/`<w:tblGrid` 不是 `w:tbl`，`<w:pgSz`/
/// `<w:pict` 不是 `w:p`，`<w:tcW` 不是 `w:tc`，`<w:tab`/`<w:tbl` 不是
/// `w:t`——只看 `matches("<w:t ")` 这类前缀口径会把它们误计。
fn tag_is_open(rest: &str, tag: &str) -> bool {
    let pat = format!("<{tag}");
    if !rest.starts_with(&pat) {
        return false;
    }
    match rest.as_bytes().get(pat.len()) {
        None => true, // 紧贴串尾（理论形态）
        Some(b'>') | Some(b'/') => true,
        Some(b) if b.is_ascii_whitespace() => true,
        _ => false,
    }
}

/// 自闭合判定（开标签以 `/>` 收尾）。
fn tag_self_closing(rest: &str) -> bool {
    match rest.find('>') {
        Some(g) => rest.as_bytes().get(g - 1) == Some(&b'/'),
        None => false,
    }
}

/// 数标签出现次数（精确标签名：`<w:p>`/`<w:p …>`/`<w:p/>`/`<w:p …/>`
/// 全算开标签出现——**自闭合形态 [`count_element`] 会漏**，`a:blip`/
/// `w:br` 恒自闭合必须用本口径）。
fn count_tag(xml: &str, tag: &str) -> usize {
    let pat = format!("<{tag}");
    let mut n = 0usize;
    let mut i = 0usize;
    while let Some(off) = xml[i..].find(&pat) {
        let at = i + off;
        if tag_is_open(&xml[at..], tag) {
            n += 1;
        }
        i = at + pat.len();
    }
    n
}

/// span 内 `w:t` 文本按序拼接（宽面 `ParagraphData::text` 同口径：
/// run 文本直连；`w:instrText`/`w:tab`/`w:br` 不入文本）。
fn wt_text(span: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while let Some(off) = span[i..].find("<w:t") {
        let at = i + off;
        if !tag_is_open(&span[at..], "w:t") {
            i = at + 1; // w:tab/w:tbl/w:tc… 前缀同形——跳过
            continue;
        }
        let gt = match span[at..].find('>') {
            Some(g) => at + g,
            None => break,
        };
        if span[at..=gt].ends_with("/>") {
            i = gt + 1; // 自闭合 <w:t/> —— 空文本
            continue;
        }
        match span[gt + 1..].find("</w:t>") {
            Some(cl) => {
                out.push_str(&span[gt + 1..gt + 1 + cl]);
                i = gt + 1 + cl + "</w:t>".len();
            }
            None => break,
        }
    }
    out
}

/// 段落普查（一次扫描给全上下文分类——独立于被测代码）。
#[derive(Default, Debug)]
struct ParagraphCensus {
    /// body 直下段（`w:tbl`/`w:sdt` 外——宽面块序顶层段口径，
    /// 只读装载 `props.paragraphs` 的实装语义）。
    body: usize,
    /// `w:sdt` 内段（宽面结构性丢弃——#156 sdt 盲区段）。
    sdt: usize,
    /// `w:tbl` 内段（表格单元格段）。
    table: usize,
    /// body 直下**空**段（装配文本 == ""——含无 `w:t` 段与自闭合段；
    /// `props.empty_paragraphs` 的实装口径）。
    body_empty: usize,
}

fn census_paragraphs(xml: &str) -> ParagraphCensus {
    let mut c = ParagraphCensus::default();
    let mut depth_tbl = 0usize;
    let mut depth_sdt = 0usize;
    let mut i = 0usize;
    while let Some(off) = xml[i..].find('<') {
        let at = i + off;
        let rest = &xml[at..];
        if tag_is_open(rest, "w:tbl") && !tag_self_closing(rest) {
            depth_tbl += 1;
        } else if rest.starts_with("</w:tbl>") {
            depth_tbl = depth_tbl.saturating_sub(1);
        } else if tag_is_open(rest, "w:sdt") && !tag_self_closing(rest) {
            depth_sdt += 1;
        } else if rest.starts_with("</w:sdt>") {
            depth_sdt = depth_sdt.saturating_sub(1);
        } else if tag_is_open(rest, "w:p") {
            // 段落（开 / 自闭合都算）——整段处理完直接跳到段尾（w:p
            // 不可嵌套，首个 </w:p> 即配对闭合）。
            let (text, end) = paragraph_span(xml, at);
            if depth_tbl > 0 {
                c.table += 1;
            } else if depth_sdt > 0 {
                c.sdt += 1;
            } else {
                c.body += 1;
                if text.is_empty() {
                    c.body_empty += 1;
                }
            }
            i = end;
            continue;
        }
        i = match rest.find('>') {
            Some(g) => at + g + 1,
            None => break,
        };
    }
    c
}

/// 段落开标签位置 → (装配文本, 段尾后一位置)。
fn paragraph_span(xml: &str, open_at: usize) -> (String, usize) {
    let gt = match xml[open_at..].find('>') {
        Some(g) => open_at + g,
        None => return (String::new(), xml.len()),
    };
    if xml[open_at..=gt].ends_with("/>") {
        return (String::new(), gt + 1); // <w:p/> 自闭合——无 run 恒空段
    }
    match xml[gt + 1..].find("</w:p>") {
        Some(cl) => {
            let close_at = gt + 1 + cl;
            (wt_text(&xml[gt + 1..close_at]), close_at + "</w:p>".len())
        }
        None => (wt_text(&xml[gt + 1..]), xml.len()),
    }
}

/// 独立数空单元格（`w:tc` 装配文本 == ""——含无 `w:t` 的格与空
/// `<w:t/>`；一格 = 一个 `w:tc`。嵌套表的格在它自己的 `w:tc` 里单独
/// 计；外层格 span 含嵌套表 `w:t` 是登记的口径边界——本夹具无嵌套）。
fn count_empty_cells(xml: &str) -> usize {
    let mut empty = 0usize;
    let mut i = 0usize;
    while let Some(off) = xml[i..].find("<w:tc") {
        let at = i + off;
        if !tag_is_open(&xml[at..], "w:tc") {
            i = at + 1; // <w:tcW … 前缀同形——跳过
            continue;
        }
        // 深度计数找配对 </w:tc>（嵌套表内层格会让「首个闭合」错位）。
        let mut depth = 1usize;
        let mut j = match xml[at..].find('>') {
            Some(g) => at + g + 1,
            None => break,
        };
        let close = loop {
            let next_open = xml[j..]
                .find("<w:tc")
                .filter(|&o| tag_is_open(&xml[j + o..], "w:tc"));
            let next_close = xml[j..].find("</w:tc>");
            match (next_open, next_close) {
                (Some(o), Some(cl)) if o < cl => {
                    depth += 1;
                    j = j + o + "<w:tc".len();
                }
                (_, Some(cl)) => {
                    depth -= 1;
                    if depth == 0 {
                        break j + cl;
                    }
                    j = j + cl + "</w:tc>".len();
                }
                (_, None) => break xml.len(),
            }
        };
        if wt_text(&xml[at..close]).is_empty() {
            empty += 1;
        }
        i = at + "<w:tc".len();
    }
    empty
}

/// zip 内 `word/media/` 部件数（容器级媒体清单 `props.images` 的
/// 独立对账面——office-io 读面枚举同前缀）。
fn count_media_parts(bytes: &[u8]) -> usize {
    zip_entries(bytes)
        .into_iter()
        .filter(|(n, _)| n.starts_with("word/media/") && !n.ends_with('/'))
        .count()
}

// ---------------------------------------------------------------------------
// 夹具结构预检（黄金例数字——python3 zipfile 独立预检钉定；夹具漂移
// 第一道红，再生成后请同步本节与断言）
// ---------------------------------------------------------------------------
//
// `bid-e2e/07_export.docx`（174272 字节，document.xml 107827 字符）：
//   - `w:p` 总数 611 = body 直下 367 + sdt 内 2（TOC 域 begin/end 段）
//     + 表格内 242（每格恰一段——`w:tc` 242 与表格段数相等）；
//   - `w:tbl` 6（无嵌套表）/ `w:tr` 44 / `w:tc` 242；
//     `w:gridSpan` 0、`w:vMerge` 0（全行无横向/纵向合并）；
//   - 空段：body 直下 1（文末 sectPr 间隔段，pPr-only 无 run）；
//     sdt 内 2（域段，宽面丢弃不计）；空单元格 21（20 无 `w:t` +
//     1 空 `<w:t/>`）；`w:br` 0（无软换行——段落不拆行）；
//   - 图片：`a:blip` 0、`w:drawing` 0、`word/media/` 0 部件（两旧夹具
//     同判无图——零图守卫防漂移；正向面由 smoke_image.docx 承载）。
//
// `smoke.docx`：26 `w:p` / 1 `w:tbl` / 4 `w:tr` / 0 图（render_fidelity
// 已全量对账——本文件只在图片守卫里触碰）。
//
// `smoke_image.docx`（3342 字节，#204 带图黄金例）：
//   - `w:p` 13 = body 直下 9（含 1 空段——纯图片段零 `w:t`）+
//     表格内 4（一格一段）；`w:tbl` 1 / `w:tr` 2 / `w:tc` 4；
//   - 图片：`a:blip` 2 / `w:drawing` 2（均 `wp:inline`）/
//     `word/media/` 2 部件（image1.png 178 B 96×64 三横带、
//     image2.png 259 B 160×80 竖纹——一图一部件一引用：document.xml.rels
//     rId1/rId2 恰两条 image 关系）；
//   - 读面黄金数（harness 实测）：props.images = 2、blocks = 10、
//     paragraphs = 9、tables = 1、table_rows = 2、empty_paragraphs = 1、
//     warnings = 0；行模型 13 行（段4 纯图片段 = 空行、段6 混排段文本
//     「机房实拍（广角）：」保留）。

// ---------------------------------------------------------------------------
// ① 只读渲染：多表格不拍平 + 空段/空单元格不丢行
// ---------------------------------------------------------------------------

/// 只读装载（`docx_doc::load`）结构对账——六表两计数、顶层段口径、
/// 空段/空格不丢：
///
/// - `props.tables` ↔ 独立 `w:tbl`（多表总数——既有测试只钉过单表）；
/// - `props.table_rows` ↔ 独立 `w:tr`（44 行不被拍平成段）；
/// - `props.paragraphs` ↔ body 直下段普查 367（宽面实装口径：表格
///   单元格段不入顶层段流、sdt 内容结构性丢弃）；
/// - `props.empty_paragraphs` ↔ 独立空段计数 1（sectPr 间隔段**被识别
///   而不是丢掉**）；
/// - 行模型总行数 == body 段 + 单元格数（每段一行、每格一行——空段
///   空格同样占行，**不丢行**）；空行数 == 空段 + 空格（空内容不静默
///   蒸发）。
#[test]
fn bid_docx_readonly_multitable_empty_para_and_cell_not_lost() {
    let rel = "bid-e2e/07_export.docx";
    let bytes = fixture_bytes(rel);
    let xml = part_text(&bytes, "word/document.xml");
    let payload = docx_doc::load(&fixture(rel)).expect("只读装载");

    // ── 独立普查（不信引擎自报）──
    let census = census_paragraphs(&xml);
    let wp_total = count_tag(&xml, "w:p");
    let wtbl = count_element(&xml, "w:tbl");
    let wtr = count_element(&xml, "w:tr");
    let wtc = count_element(&xml, "w:tc");
    let empty_cells = count_empty_cells(&xml);

    // 扫描器自洽：三上下文之和 == 全量 w:p（自闭合安全口径）。
    assert_eq!(
        census.body + census.sdt + census.table,
        wp_total,
        "普查自洽：body({}) + sdt({}) + table({}) != w:p 总数 {wp_total}——扫描器口径分歧",
        census.body,
        census.sdt,
        census.table
    );
    // 黄金例钉定（夹具漂移第一道红）。
    assert_eq!(wp_total, 611, "w:p 总数（黄金例 611）");
    assert_eq!(census.body, 367, "body 直下段（黄金例 367）");
    assert_eq!(census.sdt, 2, "sdt 盲区段（TOC 域两段）");
    assert_eq!(census.table, 242, "表格单元格段（每格恰一段）");
    assert_eq!(census.table, wtc, "单元格段数 == w:tc 数（一格一段前提）");
    assert_eq!(wtbl, 6, "六表（黄金例）");
    assert_eq!(wtr, 44, "44 行（黄金例）");
    assert_eq!(wtc, 242, "242 格（黄金例）");
    // 行切分前提：无软换行（w:br 自闭合恒现——用精确口径数）。
    assert_eq!(
        count_tag(&xml, "w:br"),
        0,
        "行数公式（每段一行）前提：夹具应无软换行"
    );

    // ── 多表格不拍平：props ↔ 独立 XML 计数 ──
    assert_eq!(
        payload.props.tables, wtbl,
        "表格数：props（渲染产物）= {} vs document.xml 独立 w:tbl = {wtbl}——多表被合并/漏计",
        payload.props.tables
    );
    assert_eq!(
        payload.props.table_rows, wtr,
        "表格行总数：props（渲染产物）= {} vs 独立 w:tr = {wtr}——行被拍平/漏计",
        payload.props.table_rows
    );
    assert_eq!(
        payload.props.blocks,
        census.body + wtbl,
        "块数：props.blocks = {} vs 顶层段 {} + 表 {} = {}——块序丢块/多块",
        payload.props.blocks,
        census.body,
        wtbl,
        census.body + wtbl
    );

    // ── 顶层段口径（宽面实装：格段归表格面、sdt 丢弃）──
    assert_eq!(
        payload.props.paragraphs, census.body,
        "顶层段落数：props.paragraphs = {} vs body 直下段普查 = {}（表格单元格段/sdt 段不入此口径）",
        payload.props.paragraphs, census.body
    );

    // ── 空段不丢：识别 + 占行 ──
    assert_eq!(
        payload.props.empty_paragraphs, census.body_empty,
        "空段计数：props.empty_paragraphs = {} vs 独立空段普查 = {}（sectPr 间隔段被丢/漏计）",
        payload.props.empty_paragraphs, census.body_empty
    );
    assert_eq!(
        census.body_empty, 1,
        "黄金例：恰 1 个空段（文末 sectPr 间隔段）——夹具漂移请重新预检"
    );

    // ── 行模型全对账：每段一行 + 每格一行（空内容同样占行）──
    let lines: Vec<&str> = payload.text.split('\n').collect();
    assert_eq!(
        lines.len(),
        census.body + wtc,
        "总行数：渲染行 = {} vs 顶层段 {} + 单元格 {} = {}（丢段/丢格/多行都现形）",
        lines.len(),
        census.body,
        wtc,
        census.body + wtc
    );
    assert_eq!(
        lines.len(),
        609,
        "黄金例：367 顶层段 + 242 格 = 609 行（sdt 两段宽面丢弃不入行流）"
    );
    let empty_lines = lines.iter().filter(|l| l.is_empty()).count();
    assert_eq!(
        empty_lines,
        census.body_empty + empty_cells,
        "空行数：渲染 = {} vs 空段 {} + 空格 {} = {}（空段落/空单元格被丢会短缺）",
        empty_lines,
        census.body_empty,
        empty_cells,
        census.body_empty + empty_cells
    );
    assert_eq!(
        empty_cells, 21,
        "黄金例：21 个空单元格（20 无 w:t + 1 空 w:t）——夹具漂移请重新预检"
    );
    assert_eq!(
        empty_lines, 22,
        "黄金例：1 空段行 + 21 空格行 = 22 空行——空内容不静默蒸发"
    );

    // ── 图片面（本夹具无图——零口径对账 + 缺口守卫，见专项测试）──
    assert_eq!(
        payload.props.images,
        count_tag(&xml, "a:blip"),
        "图片数：props.images = {} vs 独立 a:blip 计数 = {}（无图夹具两口径都应为 0）",
        payload.props.images,
        count_tag(&xml, "a:blip")
    );
}

// ---------------------------------------------------------------------------
// ② 可编辑装载：段落总账（不丢段）+ 表格侧表（不丢格）+ 空段口径
// ---------------------------------------------------------------------------

/// 可编辑装载（`docx_doc::load_editable`）四方对账——引擎段数 / 候选
/// 段落数 / 行块三轨 / 独立 `w:p` 总数：
///
/// - 不降级（sdt 盲区对账生效——降级说明结构面变了，红）；
/// - `candidates`（行模型段落总数）↔ 引擎 `paragraph_count()` ↔ 独立
///   `w:p` 总数 611：367 可编辑段 + 242 单元格段 + 2 sdt 盲区段，一个
///   不丢、一个不虚增；
/// - 表格侧表 `payload.tables`：6 表 / 行合计 44 ↔ 独立 `w:tr`、
///   栅格格数合计 242 ↔ 独立 `w:tc`（嵌套展开后栅格不丢格）；
///   `cell_paragraphs` ↔ 独立 `w:tc`（可编辑单元格清单不缺格）；
/// - 行块三轨：`Para{cont:false}` == 367、`Cell` == 242、
///   `Locked(TableRow)` == 0（无合并格全行可编辑）——表格内容不拍平
///   成普通段落行；
/// - `props.editable_paragraphs + locked_paragraphs + cell_paragraphs +
///   sdt 盲区 == candidates`（段落总账恒等式）；
/// - 空段（sectPr 间隔段）在可编辑坐标系同样计入 `empty_paragraphs`。
#[test]
fn bid_docx_editable_paragraph_and_cell_ledger_complete() {
    let rel = "bid-e2e/07_export.docx";
    let bytes = fixture_bytes(rel);
    let xml = part_text(&bytes, "word/document.xml");
    let loaded = docx_doc::load_editable(&fixture(rel)).expect("可编辑装载");
    assert!(
        loaded.bridge.is_some(),
        "标书档可编辑装载不应降级只读（降级说明：{:?}）——结构对账失效",
        loaded.payload.notice
    );
    let port = loaded
        .port
        .as_ref()
        .expect("可编辑装载必带引擎窄面（bridge Some 则 port 必 Some）");
    let p = &loaded.payload;

    // ── 独立普查 ──
    let census = census_paragraphs(&xml);
    let wp_total = count_tag(&xml, "w:p");
    let wtbl = count_element(&xml, "w:tbl");
    let wtr = count_element(&xml, "w:tr");
    let wtc = count_element(&xml, "w:tc");
    assert_eq!(wp_total, 611, "黄金例 w:p 总数");
    assert_eq!((census.body, census.sdt, census.table), (367, 2, 242));

    // ── 引擎窄面（黄金例——与 docx_bid_e2e 同基线）──
    let engine_paras = port.paragraph_count();
    let sdt_blind = port.sdt_paragraphs().len();
    assert_eq!(engine_paras, wp_total, "引擎段数 vs 独立 w:p 总数");
    assert_eq!(port.table_cells().len(), wtbl, "引擎表格数 vs 独立 w:tbl");
    assert_eq!(sdt_blind, census.sdt, "sdt 盲区段数 vs 独立 sdt 内段普查");

    // ── 段落总账（不丢段 / 不虚增）──
    assert_eq!(
        p.candidates, engine_paras,
        "候选段落数（行模型）= {} vs 引擎段数 = {engine_paras}——宽面/窄面对账缺口",
        p.candidates
    );
    assert_eq!(p.candidates, 611, "黄金例：候选 611 段");
    assert_eq!(
        p.props.paragraphs, p.candidates,
        "可编辑装载 paragraphs 口径 = 段落游标终值（含格段与 sdt 盲区，与 candidates 同值）：{} vs {}",
        p.props.paragraphs, p.candidates
    );

    // ── 表格侧表：表数 / 行合计 / 格数合计（嵌套展开后的权威栅格）──
    assert_eq!(
        p.tables.len(),
        wtbl,
        "侧表数 = {} vs 独立 w:tbl = {wtbl}——表格被合并/漏计",
        p.tables.len()
    );
    for (i, t) in p.tables.iter().enumerate() {
        assert_eq!(t.index, i, "表序号文档序（第 {i} 表 index={}）", t.index);
    }
    let rows_sum: usize = p.tables.iter().map(|t| t.rows()).sum();
    let grid_cells: usize = p
        .tables
        .iter()
        .map(|t| t.cells.iter().map(|r| r.len()).sum::<usize>())
        .sum();
    assert_eq!(
        rows_sum, wtr,
        "侧表行合计 = {rows_sum} vs 独立 w:tr = {wtr}——行被拍平/漏计"
    );
    assert_eq!(
        grid_cells, wtc,
        "侧表栅格格数合计 = {grid_cells} vs 独立 w:tc = {wtc}——栅格丢格（合并展开后应逐格对上）"
    );
    assert_eq!(
        p.cell_paragraphs.len(),
        wtc,
        "可编辑单元格清单 = {} vs 独立 w:tc = {wtc}（无合并格——格格可编辑，一格不缺）",
        p.cell_paragraphs.len()
    );

    // ── 行块三轨（1:1 行块对齐 + 分类对账）──
    let blocks = p.blocks.as_ref().expect("可编辑装载必有行块表");
    let line_count = p.text.split('\n').count();
    assert_eq!(
        blocks.len(),
        line_count,
        "行/块 1:1：块 {} vs 行 {line_count}——三轨错位（渲染/回写都会跟着错）",
        blocks.len()
    );
    let para_heads = blocks
        .iter()
        .filter(|b| matches!(b, LineBlock::Para { cont: false, .. }))
        .count();
    let para_conts = blocks
        .iter()
        .filter(|b| matches!(b, LineBlock::Para { cont: true, .. }))
        .count();
    let cell_lines = blocks
        .iter()
        .filter(|b| matches!(b, LineBlock::Cell { .. }))
        .count();
    let locked_rows = blocks
        .iter()
        .filter(|b| matches!(b, LineBlock::Locked(ReadOnlyLine::TableRow)))
        .count();
    let aux_lines = blocks
        .iter()
        .filter(|b| matches!(b, LineBlock::Locked(ReadOnlyLine::AuxAppendix)))
        .count();
    assert_eq!(
        para_heads, census.body,
        "段落首行块 = {para_heads} vs 顶层段普查 = {}——段落被丢/被拍平进表格行",
        census.body
    );
    assert_eq!(
        para_conts, 0,
        "无软换行（w:br=0）——不该有续行块（多了说明段落拆行口径漂移）"
    );
    assert_eq!(
        cell_lines, wtc,
        "单元格行块 = {cell_lines} vs 独立 w:tc = {wtc}——空格/格被丢"
    );
    assert_eq!(
        locked_rows, 0,
        "夹具无 gridSpan/vMerge——全行可编辑，不该有只读表格行（有 = 合并语义误判）"
    );
    assert!(
        aux_lines >= 2,
        "辅助清单行 ≥ 2（空行 + 表格行——「看得见的边界」；实际 {aux_lines}）"
    );

    // ── props 面（可编辑坐标系）──
    assert_eq!(
        p.props.editable_paragraphs, census.body,
        "可编辑段落数 = {} vs 顶层段普查 = {}",
        p.props.editable_paragraphs, census.body
    );
    assert_eq!(
        p.props.locked_paragraphs, 0,
        "黄金例：顶层段无域/修订/文本框/回退副本——锁定段应 0（实际 {}）",
        p.props.locked_paragraphs
    );
    assert_eq!(
        p.props.empty_paragraphs, census.body_empty,
        "空段计数（可编辑面）= {} vs 独立空段普查 = {}——sectPr 间隔段在两坐标系都该被识别",
        p.props.empty_paragraphs, census.body_empty
    );
    assert_eq!(p.props.tables, wtbl, "props.tables vs 独立 w:tbl");
    assert_eq!(p.props.table_rows, wtr, "props.table_rows vs 独立 w:tr");

    // ── 段落总账恒等式：可编辑 + 锁定 + 单元格 + sdt 盲区 == 候选 == 引擎 ──
    let ledger = p.props.editable_paragraphs
        + p.props.locked_paragraphs
        + p.cell_paragraphs.len()
        + sdt_blind;
    assert_eq!(
        ledger,
        p.candidates,
        "段落总账：可编辑 {} + 锁定 {} + 单元格 {} + sdt 盲区 {sdt_blind} = {ledger} != 候选 {}——丢段/重复计段",
        p.props.editable_paragraphs,
        p.props.locked_paragraphs,
        p.cell_paragraphs.len(),
        p.candidates
    );
    assert_eq!(
        ledger, engine_paras,
        "总账 == 引擎段数（原文坐标系不丢不重）"
    );
}

// ---------------------------------------------------------------------------
// ③ 图片面（#204 起双面：smoke_image.docx 正向对账 + 两旧夹具零图守卫）
// ---------------------------------------------------------------------------

/// 两**旧**夹具（smoke.docx / bid-e2e/07_export.docx）的零图防漂移
/// 守卫：props.images ↔ a:blip ↔ word/media/ 部件三方 == 0。
///
/// #204 前这里是「缺口留证」（正向面无夹具可测）；#204 带图黄金夹具
/// smoke_image.docx 入库后，正向面已由
/// [`docx_image_face_positive_reconciliation_smoke_image`] 承载——本
/// 守卫转为纯防漂移：任一旧夹具再生成时漂入图片（三方任一 ≠ 0）即红
/// （props.images 口径 = 容器级媒体部件数，与 blip 计数在「多图共部件」
/// 等形态下会错位——不能让带图件静默穿过零图断言）。
#[test]
fn docx_image_face_gap_guard_no_image_fixture_in_repo() {
    for rel in ["smoke.docx", "bid-e2e/07_export.docx"] {
        let bytes = fixture_bytes(rel);
        let xml = part_text(&bytes, "word/document.xml");
        let payload =
            docx_doc::load(&fixture(rel)).unwrap_or_else(|e| panic!("{rel} 只读装载：{e}"));

        let blips = count_tag(&xml, "a:blip");
        let drawings = count_tag(&xml, "w:drawing");
        let media = count_media_parts(&bytes);

        assert_eq!(
            payload.props.images, blips,
            "{rel}：props.images = {} vs document.xml 独立 a:blip = {blips}——图片清单与引用计数不一致",
            payload.props.images
        );
        assert_eq!(
            payload.props.images, media,
            "{rel}：props.images = {} vs word/media/ 部件数 = {media}——容器级媒体清单不一致",
            payload.props.images
        );
        assert_eq!(
            blips, 0,
            "{rel}：夹具应无图（a:blip = {blips}）——夹具漂移成带图件：请补真图片面对账后更新本守卫"
        );
        assert_eq!(
            drawings, 0,
            "{rel}：夹具应无 w:drawing（= {drawings}）——夹具漂移成带图件：同上"
        );
        assert_eq!(
            media, 0,
            "{rel}：夹具应无 word/media/ 部件（= {media}）——夹具漂移成带图件：同上"
        );
        assert_eq!(
            payload.props.images, 0,
            "{rel}：props.images 应 0（= {}）——读面在无图件上报出图片数 = 真缺陷",
            payload.props.images
        );
    }
    eprintln!(
        "\n══════ 图片面覆盖（#204 已补齐）══════\n  \
         两旧夹具（smoke.docx / bid-e2e/07_export.docx）仍无图——本守卫\n  \
         钉住三方零口径一致，防夹具漂移成带图件后口径失衡。\n  \
         「图片不丢」正向面（images ≥ 1 三方对账 + 邻接内容不丢不崩）\n  \
         已由 smoke_image.docx 黄金例承载（见下一个测试）。\n\
         ══════════════════════════════════════\n"
    );
}

/// #204：图片正向对账（smoke_image.docx——两内联图，尺寸不同、分置
/// 两段；scripts/make_image_docx_fixture.py 手构，黄金例数字 =
/// python3 zipfile 预检 + 本文件独立计数 + 仓库外 harness 真实跑
/// `docx_doc::load` 三方一致）。
///
/// 断言面（读面实装语义——docx_doc.rs：图片不入正文行流，计数入
/// props.images；其口径 = **容器级媒体部件清单**（`word/media/*`
/// 枚举）而非 a:blip 计数，夹具按「一图一部件一引用」构造使三口径恒等）：
///
/// - **三方对账**：props.images == 独立 a:blip == word/media/ 部件数，
///   且 ≥ 1（黄金例 2）——带图件读面报 0 或口径错位都现形；
/// - **不丢不崩**：装载成功零警告；纯图片段被识别为空段
///   （empty_paragraphs 计数 + 占行，不静默蒸发）；混排段文本保留；
///   邻接标题/表格/段落零位移（行模型 13 行逐行对账）。
#[test]
fn docx_image_face_positive_reconciliation_smoke_image() {
    let rel = "smoke_image.docx";
    let bytes = fixture_bytes(rel);
    let xml = part_text(&bytes, "word/document.xml");
    let payload = docx_doc::load(&fixture(rel)).expect("带图夹具只读装载");

    // ── 独立计数（不信引擎自报）──
    let blips = count_tag(&xml, "a:blip");
    let drawings = count_tag(&xml, "w:drawing");
    let inlines = count_tag(&xml, "wp:inline");
    let media = count_media_parts(&bytes);
    let census = census_paragraphs(&xml);
    let wp_total = count_tag(&xml, "w:p");
    let wtbl = count_element(&xml, "w:tbl");
    let wtr = count_element(&xml, "w:tr");
    let wtc = count_element(&xml, "w:tc");

    // 扫描器自洽 + 黄金例（夹具漂移第一道红）。
    assert_eq!(
        census.body + census.sdt + census.table,
        wp_total,
        "普查自洽：body({}) + sdt({}) + table({}) != w:p 总数 {wp_total}",
        census.body,
        census.sdt,
        census.table
    );
    assert_eq!(
        (wp_total, census.body, census.sdt, census.table),
        (13, 9, 0, 4)
    );
    assert_eq!((wtbl, wtr, wtc), (1, 2, 4), "黄金例：1 表 / 2 行 / 4 格");
    assert_eq!(
        (blips, drawings, inlines, media),
        (2, 2, 2, 2),
        "黄金例：2 图（a:blip / w:drawing / wp:inline / media 部件四口径）"
    );

    // ── 图片正向对账（#170 验收面「图片不丢」——#204 补齐）──
    assert_eq!(
        payload.props.images, blips,
        "图片数：props.images = {} vs 独立 a:blip = {blips}——图片清单与引用计数不一致（图丢了读面不知道）",
        payload.props.images
    );
    assert_eq!(
        payload.props.images, media,
        "图片数：props.images = {} vs word/media/ 部件数 = {media}——容器级媒体清单不一致",
        payload.props.images
    );
    assert_eq!(
        payload.props.images, 2,
        "props.images 应 ≥ 1（黄金例 2——带图件读面报 0 = 图片面丢失）"
    );
    assert_eq!(
        payload.props.warnings, 0,
        "干净包零警告（有警告 = 夹具畸形或读面误报，实际 {}）",
        payload.props.warnings
    );

    // ── 不丢不崩：图片不入行流（读面实装语义）──
    // 纯图片段 = 空段（计数 + 占行）；混排段 = 文本保留、图不入文本。
    assert_eq!(
        payload.props.empty_paragraphs, census.body_empty,
        "空段计数：props.empty_paragraphs = {} vs 独立空段普查 = {}（纯图片段应被识别为空段——不丢段）",
        payload.props.empty_paragraphs, census.body_empty
    );
    assert_eq!(census.body_empty, 1, "黄金例：恰 1 空段（纯图片段零 w:t）");

    let lines: Vec<&str> = payload.text.split('\n').collect();
    assert_eq!(
        lines.len(),
        census.body + wtc,
        "总行数：渲染行 = {} vs 顶层段 {} + 单元格 {} = {}（图片段占行不丢）",
        lines.len(),
        census.body,
        wtc,
        census.body + wtc
    );
    assert_eq!(lines.len(), 13, "黄金例：9 段 + 4 格 = 13 行");
    // 逐行对账：图片两侧内容零位移（段4 纯图片段 = 空行；段6 混排段
    // 文本保留、图片不入文本）。
    assert_eq!(
        lines,
        vec![
            "智慧园区综合管理平台图示样张",
            "本样张验证 docx 图片读面（#204）：两张内联图片，尺寸不同、分置两段。",
            "一、网络拓扑",
            "下图为平台网络拓扑示意（纯图片段——图片不入正文行流）。",
            "", // 段4 纯图片段：空行承载（图片计数入 props.images）
            "二、机房实拍",
            "机房实拍（广角）：", // 段6 混排段：文本保留、图片不入文本
            "三、报价明细",
            "设备",
            "数量",
            "机柜",
            "42",
            "全文完——图片清单见属性面板（props.images = 2）。",
        ],
        "行模型逐行对账——图片段两侧邻接内容完好（不丢不崩）"
    );

    // ── 邻接结构完好：块数 / 表格 / 标题树 ──
    assert_eq!(
        payload.props.blocks,
        census.body + wtbl,
        "块数 = {} vs 顶层段 {} + 表 {} = {}——图片段/表被丢或虚增",
        payload.props.blocks,
        census.body,
        wtbl,
        census.body + wtbl
    );
    assert_eq!(payload.props.tables, wtbl, "表数不因图片丢");
    assert_eq!(payload.props.table_rows, wtr, "表行不因图片丢");
    assert_eq!(payload.headings.len(), 4, "标题树 4（1 H1 + 3 H2）");
    assert_eq!(
        (payload.headings[0].level, payload.headings[0].text.as_str()),
        (1, "智慧园区综合管理平台图示样张"),
        "首标题完好（H1）"
    );
    assert_eq!(
        payload.headings[3].text.as_str(),
        "三、报价明细",
        "末标题完好（混排图片段与表之间的标题零位移）"
    );
}
