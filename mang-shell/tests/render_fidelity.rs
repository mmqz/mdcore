//! #170：docx/xlsx/pdf 渲染正确性——结构断言 + 坏件不 panic。
//!
//! ## 为什么
//!
//! `document::open` 把 docx/xlsx/pdf 分派到只读渲染
//! （`panels.rs`），但**没有任何测试证明渲染是正确的**：打开一个
//! docx 界面显示 5 段 2 表——没人知道对不对。渲染层的单测
//! （docx_doc/xlsx_doc/pdf_doc 内嵌）用的是**合成数据**，真实夹具
//! 走全链（读盘 → 解析 → 转换 → props）的「结构正确性」从未验过。
//!
//! ## 三层验证
//!
//! 1. **结构对账（不信引擎自报）**：渲染层 props（段落数/表格数/行数/
//!    工作表数/页数）↔ **独立解析源文件字节**（极简 zip 扫描 + 数
//!    XML 元素 / PDF 对象）。props 说 26 段，document.xml 里就得真有
//!    26 个 `w:p`——转换层丢块、读面漏表都当场现形。
//! 2. **已知内容锚点（真实夹具，不合成）**：`tests/fixtures/smoke.*`
//!    由仓库内生成器（`gen_smoke_docx/xlsx/pdf`，--ignored 落盘）
//!    产出——**生成器源码就是内容清单**：标题树、表格 4 行、公式
//!    缓存值 300.5、合并格、超链接、书签三章……逐项对账。
//! 3. **保真度守卫**：CJK 占比（乱码近似 0，真实中文 0.4+——判据落
//!    在被测对象自身，不查环境工具装没装）；表格结构不被拍平（xlsx
//!    管道对齐 / docx 单元格段落 12 个）；空单元格不丢弃（同行等宽）。
//!
//! ## 坏件不 panic（三种 × 三格式）
//!
//! 零字节 / 非 zip 垃圾字节套 office 扩展名 / 截断件——断言：要么
//! `Err`（明确错误），要么 `Ok`（有界产物），**绝不 panic**。
//!
//! ## 环境缺件纪律（issue #9 / #171 口径）
//!
//! pdf 装载需要 libpdfium 动态库。CI（windows-latest）没有——pdf 的
//! **结构断言**在缺件环境打 SKIP 横幅（不伪装通过，eprintln 留证），
//! 有 libpdfium 的环境（本地烟雾通道 `./smoke.sh 5`）全量真跑。
//! **坏件测试不受此限**：有无 libpdfium 两种环境对坏字节同判 `Err`
//! （`load_rejects_invalid_bytes` 已钉定的两环境同判语义）。
//!
//! ## 铁律
//!
//! - 只加测试，不改产线代码（git diff 只含 tests/）；
//! - 夹具缺失即 FAIL（黄金例进仓，不许 SKIP——docx_bid_e2e 同款）；
//! - 发现渲染错误 → 开 issue，不在本任务修。

use std::path::{Path, PathBuf};

use mang_shell::editor::{docx_doc, pdf_doc, xlsx_doc};

/// 最小 CJK 占比（真实中文文档 0.4+；乱码/丢失接近 0——阈值取宽裕值
/// 只抓「明显坏」，不做中文率 KPI）。
const MIN_CJK_RATIO: f64 = 0.25;

fn fixture(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    if !p.is_file() {
        panic!(
            "夹具缺失（{}）——黄金例进仓，不许 SKIP；\
             由仓库内生成器再生成：cargo test gen_smoke_docx / gen_smoke_xlsx / gen_smoke_pdf -- --ignored",
            p.display()
        );
    }
    p
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("夹具不可读（{name}）：{e}"))
}

// ---------------------------------------------------------------------------
// 独立解析工具（与 docx_bid_e2e.rs 同款——独立于被测代码的验证面）
// ---------------------------------------------------------------------------

/// 极简 zip 条目扫描（EOCD + 中央目录 + 本地头三固定结构）。
/// 返回 (name, 解压后字节)。**不依赖被测的 office-io 读面**——这是
/// 对账的独立性来源。
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
    let eocd = eocd.expect("EOCD（截断/非 zip 件在此早死——这正是坏件测试的期望）");
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

/// 数 XML 元素出现次数（`<w:p ` / `<w:p>` / `<w:p/>` 都算开标签出现）。
fn count_element(xml: &str, tag: &str) -> usize {
    xml.matches(&format!("<{tag} ")).count() + xml.matches(&format!("<{tag}>")).count()
}

/// 数 **body 直下**的 `w:p`（跳过 `w:tbl` 内部——表格单元格段不计）。
///
/// 只读装载的 `props.paragraphs` 口径 = 块序顶层段（表格单元格段落
/// 归 cell_paragraphs 面——可编辑坐标系）；全量 `w:p`（含单元格段）
/// 由 [`count_element`] 承担。
fn count_top_level_paragraphs(xml: &str) -> usize {
    let mut depth = 0usize;
    let mut n = 0usize;
    let mut i = 0usize;
    while i < xml.len() {
        if xml[i..].starts_with("<w:tbl>") || xml[i..].starts_with("<w:tbl ") {
            depth += 1;
        } else if xml[i..].starts_with("</w:tbl>") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && (xml[i..].starts_with("<w:p>") || xml[i..].starts_with("<w:p ")) {
            n += 1;
        }
        match xml[i + 1..].find('<') {
            Some(next) => i = i + 1 + next,
            None => break,
        }
    }
    n
}

/// East Asian 显示宽（CJK 2 cell——与 xlsx_doc::display_width 同口径：
/// 网格对齐按 EA cell 计，chars().count() 在 CJK/空格混合下会误判）。
fn ea_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            let cjk = (0x1100..=0x115F).contains(&cp)
                || (0x2E80..=0xA4CF).contains(&cp)
                || (0xAC00..=0xD7A3).contains(&cp)
                || (0xF900..=0xFAFF).contains(&cp)
                || (0xFE30..=0xFE4F).contains(&cp)
                || (0xFF00..=0xFF60).contains(&cp)
                || (0xFFE0..=0xFFE6).contains(&cp);
            if cjk { 2 } else { 1 }
        })
        .sum()
}

/// 独立数 PDF 页：`/Type /Page`（或 `/Type/Page`），`s` 后缀（`/Pages`）
/// 用负字符类排除——不依赖任何 pdf 库。
fn count_pdf_pages(bytes: &[u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 5] == b"/Type" {
            let mut j = i + 5;
            while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            if j + 5 <= bytes.len()
                && &bytes[j..j + 5] == b"/Page"
                && bytes.get(j + 5) != Some(&b's')
            {
                n += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    n
}

/// CJK 占比（中日韩表意文字 / 非空白字符）。
fn cjk_ratio(s: &str) -> f64 {
    let total = s.chars().filter(|c| !c.is_whitespace()).count();
    if total == 0 {
        return 0.;
    }
    let cjk = s
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            (0x4E00..=0x9FFF).contains(&cp) // CJK 统一表意
                || (0x3400..=0x4DBF).contains(&cp) // 扩展 A
                || (0xF900..=0xFAFF).contains(&cp) // 兼容表意
        })
        .count();
    cjk as f64 / total as f64
}

// ---------------------------------------------------------------------------
// docx：结构对账 + 内容锚点 + 可编辑面
// ---------------------------------------------------------------------------

/// docx 渲染结构 == 源文件真实结构（独立对账，不信引擎自报）。
///
/// 已知事实（生成器 `gen_smoke_docx` 的内容清单 + Python zipfile 独立
/// 预检）：26 个 `w:p`（14 顶层段 + 12 表格单元格段）、1 表、4 行、
/// 5 级标题树。
#[test]
fn docx_structure_matches_source_bytes() {
    let bytes = fixture_bytes("smoke.docx");
    let xml = part_text(&bytes, "word/document.xml");

    let payload = docx_doc::load(&fixture("smoke.docx")).expect("只读装载");

    // ── 独立对账（核心：props vs 源 XML 实数）──
    // 口径分层：只读 props.paragraphs = body 直下段（块序顶层）；全量
    // w:p（含表格单元格段）由可编辑装载面承担（candidates == 26，见
    // docx_editable_load_keeps_table_grid——两口径互补钉满）。
    let wp_total = count_element(&xml, "w:p");
    let wp_top = count_top_level_paragraphs(&xml);
    let wtbl = count_element(&xml, "w:tbl");
    let wtr = count_element(&xml, "w:tr");
    assert_eq!(
        payload.props.paragraphs, wp_top,
        "顶层段落数：props={} vs document.xml body 直下实数={wp_top}（转换层丢段/多段）",
        payload.props.paragraphs
    );
    assert_eq!(
        payload.props.tables, wtbl,
        "表格数：props={} vs 实数={wtbl}",
        payload.props.tables
    );
    assert_eq!(
        payload.props.table_rows, wtr,
        "表格行数：props={} vs 实数={wtr}（行被拍平/漏计）",
        payload.props.table_rows
    );
    // 夹具完整性（生成器内容清单——夹具若被再生成，这里同步）。
    assert_eq!(
        wp_total, 26,
        "smoke.docx 已知 26 段（14 顶层 + 12 单元格）——夹具漂移请重新预检"
    );
    assert_eq!(wp_top, 14, "已知 14 顶层段");
    assert_eq!(
        wp_total - wp_top,
        12,
        "单元格段应恰 12（4×3 栅格）——与可编辑面 cell_paragraphs 对账"
    );
    assert_eq!(wtbl, 1);
    assert_eq!(wtr, 4, "报价表 1 表头 + 3 数据行");

    // ── 标题树（文档序逐字对账）──
    let want: Vec<(usize, &str)> = vec![
        (1, "智慧园区综合管理平台投标文件"),
        (2, "一、项目概述"),
        (2, "二、技术方案"),
        (2, "三、报价明细"),
        (2, "四、实施计划"),
    ];
    assert_eq!(
        payload.headings.len(),
        want.len(),
        "标题树 {} 项 != 已知 {}",
        payload.headings.len(),
        want.len()
    );
    for (h, (level, text)) in payload.headings.iter().zip(want) {
        assert_eq!(h.level, level, "标题层级：{text:?}");
        assert_eq!(h.text, text, "标题逐字（被改写/截断即红）");
        // 标题行锚点：line 指向的行就是标题文本。
        let line = payload.text.split('\n').nth(h.line).unwrap_or_default();
        assert_eq!(
            line, text,
            "标题行锚点错位：line={} 的行是 {line:?}",
            h.line
        );
    }
}

/// docx 内容保真：正文锚点（富格式 run 合并、列表、表格内容、链接、
/// 引用块）+ CJK 占比守卫。
#[test]
fn docx_content_anchors_and_cjk_fidelity() {
    let payload = docx_doc::load(&fixture("smoke.docx")).expect("只读装载");
    let text = &payload.text;

    // 富格式 run 段（多 run 合并成一行——丢 run 即丢字）。
    for anchor in [
        "本项目承建 智慧园区综合管理平台，含数据中心机房、安防与物联网接入三大部分。",
        "总工期 180 个日历天，详见 项目门户。",
    ] {
        assert!(
            text.contains(anchor),
            "正文锚点缺失：{anchor:?}\n（多 run 段被拆散/丢 run？）"
        );
    }
    // 列表（有序 + 无序）。
    for anchor in [
        "接入网关集群（双活）",
        "消息中间件（Kafka）",
        "时序数据库（TDengine）",
        "现场勘测已完成",
    ] {
        assert!(text.contains(anchor), "列表项缺失：{anchor:?}");
    }
    // 表格内容进正文（#117 的反面：表格做完了没接线到渲染）。
    for anchor in [
        "设备",
        "机柜",
        "18000",
        "UPS 电源",
        "350000",
        "核心交换机",
        "128000",
    ] {
        assert!(text.contains(anchor), "表格内容未进渲染：{anchor:?}");
    }
    // 引用块。
    assert!(
        text.contains("注：本样张由 mdc-office-io 写面生成"),
        "引用块缺失"
    );

    // CJK 保真守卫（乱码时占比崩到 0 附近）。
    let ratio = cjk_ratio(text);
    assert!(
        ratio > MIN_CJK_RATIO,
        "CJK 占比 {ratio:.3} ≤ {MIN_CJK_RATIO}——中文被解码成碎片（乱码形态）"
    );
}

/// docx 可编辑装载面：宽窄面段落对账 + 表格单元格段落清单
/// （表格结构在可编辑坐标系里不丢格）。
#[test]
fn docx_editable_load_keeps_table_grid() {
    let item = docx_doc::load_editable(&fixture("smoke.docx")).expect("可编辑装载");
    // 生成器自校验基线：smoke.docx 不降级（bridge 在场）。
    assert!(
        item.bridge.is_some(),
        "smoke.docx 可编辑装载不应降级只读——{}",
        item.payload.notice.unwrap_or_default()
    );
    let p = &item.payload;
    // 宽面候选段落数 == 独立 w:p 计数（26）——宽窄对账是装载门的前提，
    // 这里对独立面再钉一次。
    assert_eq!(p.candidates, 26, "候选段落数（宽面）vs 独立 w:p 计数");
    // 单元格段落清单：4 行 × 3 列 = 12（无合并格）——表格栅格不丢格。
    assert_eq!(
        p.cell_paragraphs.len(),
        12,
        "表格单元格段落数 {} != 12（4×3 栅格丢格）",
        p.cell_paragraphs.len()
    );
    assert_eq!(p.props.tables, 1);
}

// ---------------------------------------------------------------------------
// xlsx：结构对账 + 值锚点 + 网格对齐 + 空格保留
// ---------------------------------------------------------------------------

/// xlsx 渲染结构 == 源文件真实结构：工作表树 ↔ workbook.xml 独立对账；
/// 值保真：公式缓存值、数值格式化、合并格、超链接显示文本。
#[test]
fn xlsx_structure_and_values_match_source() {
    let bytes = fixture_bytes("smoke.xlsx");
    let payload = xlsx_doc::load(&fixture("smoke.xlsx")).expect("只读装载");

    // ── 独立对账：workbook.xml 的 sheet 名单 vs 渲染层工作表树 ──
    let wb = part_text(&bytes, "xl/workbook.xml");
    let independent_sheets: Vec<&str> = vec!["销售明细", "参数"];
    for name in &independent_sheets {
        assert!(
            wb.contains(&format!("name=\"{name}\"")),
            "workbook.xml 无工作表 {name}——夹具漂移请重新预检"
        );
    }
    assert_eq!(
        payload.props.sheets, 2,
        "工作表数：props={} vs 独立实数 2",
        payload.props.sheets
    );
    let got: Vec<&str> = payload.headings.iter().map(|h| h.text.as_str()).collect();
    assert_eq!(got, independent_sheets, "工作表树（簿序逐字）");

    // ── 值锚点（生成器内容清单）──
    let text = &payload.text;
    for anchor in [
        "一月",
        "春节备货", // 文本格
        "200.5",
        "300.5",        // 数值格（Rust f64 格式化）+ 公式缓存值
        "外部资料区",   // 合并格左上值（非左上格空串——#203 勘误）
        "季度报表门户", // 超链接显示文本
        "税率",
        "0.13", // 第二表
    ] {
        assert!(text.contains(anchor), "单元格值缺失：{anchor:?}");
    }
    // 公式缓存值形态：显示 300.5 而非公式文本 SUM。
    let march_line = text
        .split('\n')
        .find(|l| l.contains("三月"))
        .unwrap_or_else(|| panic!("三月数据行缺失"));
    assert!(
        march_line.contains("300.5"),
        "公式格应显示缓存值：{march_line:?}"
    );
    assert!(
        !march_line.contains("SUM"),
        "不该显示公式文本：{march_line:?}"
    );
    assert_eq!(payload.props.formulas, 1, "公式格计数");
    assert!(payload.props.merges >= 1, "合并范围计数");
    assert!(payload.props.hyperlinks >= 1, "超链接计数");

    // ── 网格结构不被拍平：数据行管道位对齐（EA 列宽）──
    let lines: Vec<&str> = text.split('\n').collect();
    let data_rows: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| l.contains("一月") || l.contains("二月") || l.contains("三月"))
        .collect();
    assert_eq!(data_rows.len(), 3, "三个数据行（一月/二月/三月）");
    let pipes: Vec<Vec<usize>> = data_rows
        .iter()
        .map(|l| l.match_indices('|').map(|(i, _)| i).collect())
        .collect();
    for (name, p) in data_rows.iter().zip(&pipes) {
        assert_eq!(p.len(), 2, "数据行应恰两列分隔（3 列）：{name:?}");
    }
    assert_eq!(
        pipes[0], pipes[1],
        "一月/二月行管道位不对齐（列宽漂移/网格拍平）：\n  {:?}\n  {:?}",
        data_rows[0], data_rows[1]
    );
    assert_eq!(pipes[0], pipes[2], "一月/三月行管道位不对齐");

    // ── 空单元格不丢弃：二月行备注为空串，但列仍在（同行 EA 显示宽
    // 一致——渲染对齐按 EA cell 计，不能用 chars().count()：CJK 1 字
    // 2 cell，空列补齐的是单宽空格，字符数天然不等而显示宽必等）。──
    let w_jan = ea_width(data_rows[0]);
    let w_feb = ea_width(data_rows[1]);
    let w_mar = ea_width(data_rows[2]);
    assert_eq!(
        w_jan, w_feb,
        "空备注行与实值行 EA 显示宽不等（空列被丢弃会短一截）：\n  {:?}\n  {:?}",
        data_rows[0], data_rows[1]
    );
    assert_eq!(w_jan, w_mar, "三月行与一月行 EA 显示宽不等");

    // ── CJK 保真 ──
    let ratio = cjk_ratio(text);
    assert!(
        ratio > MIN_CJK_RATIO,
        "CJK 占比 {ratio:.3} ≤ {MIN_CJK_RATIO}——乱码形态"
    );
}

// ---------------------------------------------------------------------------
// pdf：结构断言（libpdfium 缺件 → SKIP 横幅，不伪装通过）
// ---------------------------------------------------------------------------

/// pdf 渲染结构 + 保真：页数 ↔ 独立 `/Type /Page` 计数对账、书签三章、
/// 首页文本层非空、CJK 守卫。
///
/// **环境纪律**：无 libpdfium（如 CI windows-latest）→ SKIP 横幅 +
/// `return`（不伪装通过）；有 libpdfium（本地烟雾通道）→ 全量真跑。
/// 夹具坏 / 渲染真缺陷 → 红到底，**不 SKIP**。
#[test]
fn pdf_structure_matches_source_when_engine_available() {
    let path = fixture("smoke.pdf");
    let payload = match pdf_doc::load(&path) {
        Ok(p) => p,
        Err(e) if e.contains("libpdfium") => {
            eprintln!(
                "\n══════ SKIP（环境缺件，不伪装通过）══════\n  \
                 本机无 libpdfium——pdf 渲染结构断言未真跑。\n  \
                 引擎错误：{e}\n  \
                 有 libpdfium 的环境（./smoke.sh 5 tests/fixtures/smoke.pdf）全量执行。\n\
                 ════════════════════════════════════════\n"
            );
            return;
        }
        Err(e) => {
            panic!("smoke.pdf 装载失败（夹具进仓且非环境缺件——真缺陷，开 issue 不要改产线）：{e}")
        }
    };

    // ── 独立对账：页数 vs /Type /Page 计数（不依赖引擎自报）──
    let bytes = fixture_bytes("smoke.pdf");
    let independent_pages = count_pdf_pages(&bytes);
    assert!(
        independent_pages >= 2,
        "独立计数 {} 页（夹具漂移请重新预检）",
        independent_pages
    );
    assert_eq!(
        payload.props.pages, independent_pages,
        "页数：props={} vs 独立 /Type /Page 计数={independent_pages}",
        payload.props.pages
    );

    // ── 书签三章（生成器注入——outline 提取面的逐字对账）──
    let want = ["第一章 项目概述", "第二章 技术方案", "第三章 报价明细"];
    assert!(
        payload.headings.len() >= 3,
        "书签 {} 条 < 3（生成器注入的三章书签被丢）",
        payload.headings.len()
    );
    for w in want {
        assert!(
            payload.headings.iter().any(|h| h.text == w),
            "书签缺失：{w:?}（现有：{:?}）",
            payload
                .headings
                .iter()
                .map(|h| h.text.clone())
                .collect::<Vec<_>>()
        );
    }

    // ── 首页文本层非空（扫描件判定为否——本夹具是文本层 PDF）──
    assert!(
        !payload.props.scanned,
        "smoke.pdf 是文本层 PDF，不该判扫描件"
    );
    assert!(payload.props.chars > 0, "可提取字符数为 0");
    assert_eq!(payload.props.empty_pages, 0, "无空文本页");
    // 首页区间内存在非分隔、非空的正文行。
    let lines: Vec<&str> = payload.text.split('\n').collect();
    let first = &payload.pages[0];
    let has_body = lines[first.first_line..=first.last_line]
        .iter()
        .any(|l| !l.is_empty() && !l.starts_with("──"));
    assert!(
        has_body,
        "首页（行 {}..={}）无正文行",
        first.first_line, first.last_line
    );

    // ── CJK 保真（Type0/CID 嵌入子集 skipped=0 生成——解码坏则崩）──
    let ratio = cjk_ratio(&payload.text);
    assert!(
        ratio > MIN_CJK_RATIO,
        "CJK 占比 {ratio:.3} ≤ {MIN_CJK_RATIO}——CID 解码成碎片（乱码形态）"
    );
}

// ---------------------------------------------------------------------------
// 坏件不 panic（三种 × 三格式——有无 libpdfium 两环境同判）
// ---------------------------------------------------------------------------

/// 坏件写盘（临时目录——不碰仓库）。
fn temp_bad_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mang-rf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let p = dir.join(name);
    std::fs::write(&p, bytes).expect("写坏件");
    p
}

/// 「不 panic 且有界」判据：Err（明确错误——最好形态）或 Ok（产物
/// 页/段数有上界，不无限增长）。panic = 红。
fn assert_bounded<T>(result: &Result<T, String>, what: &str) {
    if let Err(e) = result {
        assert!(!e.is_empty(), "{what} 的 Err 空消息（fail-loud 破坏）");
    }
}

/// ① 零字节件 × 三格式：明确 Err，绝不 panic。
#[test]
fn zero_byte_files_do_not_panic() {
    // docx（只读 + 可编辑两入口）。
    let p = temp_bad_file("zero.docx", b"");
    let r = docx_doc::load(&p);
    assert!(r.is_err(), "零字节 docx 应 Err（不是空文档伪装成功）");
    assert_bounded(&r, "零字节 docx");
    assert!(
        docx_doc::load_editable(&p).is_err(),
        "零字节 docx 可编辑装载应 Err"
    );

    // xlsx。
    let p = temp_bad_file("zero.xlsx", b"");
    let r = xlsx_doc::load(&p);
    assert!(r.is_err(), "零字节 xlsx 应 Err");
    assert_bounded(&r, "零字节 xlsx");

    // pdf（有无 libpdfium 两环境同判 Err——库装载失败/空文档装载失败）。
    let p = temp_bad_file("zero.pdf", b"");
    let r = pdf_doc::load(&p);
    assert!(r.is_err(), "零字节 pdf 应 Err（两环境同判）");
    assert_bounded(&r, "零字节 pdf");
}

/// ② 非 office 内容套 office 扩展名 × 三格式：明确 Err，绝不 panic。
#[test]
fn plain_text_with_office_extension_does_not_panic() {
    let garbage =
        b"this is not a zip/pdf file, just plain text pretending to have an office extension\n"
            .repeat(3);
    let cases: Vec<(&str, u8)> = vec![("fake.docx", 0), ("fake.xlsx", 1), ("fake.pdf", 2)];
    for (name, kind) in cases {
        let p = temp_bad_file(name, &garbage);
        let what = format!("垃圾字节套 {name}");
        match kind {
            0 => {
                let r = docx_doc::load(&p);
                assert!(r.is_err(), "{what} 应 Err");
                assert_bounded(&r, &what);
                assert!(
                    docx_doc::load_editable(&p).is_err(),
                    "{what} 可编辑装载应 Err"
                );
            }
            1 => {
                let r = xlsx_doc::load(&p);
                assert!(r.is_err(), "{what} 应 Err");
                assert_bounded(&r, &what);
            }
            _ => {
                let r = pdf_doc::load(&p);
                assert!(r.is_err(), "{what} 应 Err（两环境同判）");
                assert_bounded(&r, &what);
            }
        }
    }
}

/// ③ 截断件 × 三格式：docx/xlsx 的 zip 骨架必 Err（EOCD 不在尾）；
/// pdf 对截断的宽容度是引擎实现细节——**不 panic + 有界**即合格。
#[test]
fn truncated_files_do_not_panic() {
    let docx_full = fixture_bytes("smoke.docx");
    let xlsx_full = fixture_bytes("smoke.xlsx");
    let pdf_full = fixture_bytes("smoke.pdf");

    // docx：截到 1000 字节（EOCD 缺失——zip 结构坏）。
    let p = temp_bad_file("trunc.docx", &docx_full[..1000.min(docx_full.len())]);
    let r = docx_doc::load(&p);
    assert!(r.is_err(), "截断 docx 应 Err（EOCD 缺失）");
    assert_bounded(&r, "截断 docx");
    assert!(
        docx_doc::load_editable(&p).is_err(),
        "截断 docx 可编辑装载应 Err"
    );

    // xlsx：截到 500 字节。
    let p = temp_bad_file("trunc.xlsx", &xlsx_full[..500.min(xlsx_full.len())]);
    let r = xlsx_doc::load(&p);
    assert!(r.is_err(), "截断 xlsx 应 Err");
    assert_bounded(&r, "截断 xlsx");

    // pdf：截到 1000 字节——宽松判据（不 panic 即过；Err 时消息非空）。
    let p = temp_bad_file("trunc.pdf", &pdf_full[..1000.min(pdf_full.len())]);
    match pdf_doc::load(&p) {
        Ok(payload) => {
            // 引擎宽容装载了：产物必须有界（不超过源文件完整页数）。
            let full_pages = count_pdf_pages(&pdf_full);
            assert!(
                payload.props.pages <= full_pages,
                "截断 pdf 装载出 {} 页 > 完整件 {full_pages} 页（无界产物）",
                payload.props.pages
            );
        }
        Err(e) => assert!(!e.is_empty(), "截断 pdf 的 Err 空消息"),
    }
}
