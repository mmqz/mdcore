//! #170：xlsx 只读渲染——第二条结构断言（公式/合并/超链接对账 + 坐标级值锚点）。
//!
//! ## 为什么
//!
//! `render_fidelity::xlsx_structure_and_values_match_source` 是 xlsx 唯一
//! 的结构测试（#170 要求 ≥2）。它的两面弱点正是本文件补的：
//!
//! - **公式/合并/超链接只有 `>= 1` 下限**：props 自报数 ≥1 既没对账源
//!   文件实数，也没钉「第二表为 0」的分布面——读面若把两表计数翻倍
//!   （或漏计 0 值表）依然绿。
//! - **值锚点是全文子串匹配**（`text.contains`）：任何一行出现即绿，
//!   无坐标定位——「A1 显示月份」与「月份这两个字在文内某处」不可分。
//!
//! ## 两个测试
//!
//! 1. [`xlsx_formula_merge_hyperlink_counts_match_source_xml`]——结构对账
//!    （不信引擎自报）：极简 zip 扫描 → workbook.xml `<sheet>` 簿序 →
//!    rels → sheet 部件，逐部件独立数 `<f>` / `<mergeCell` / `<hyperlink` /
//!    有值 `<c>`，分 sheet 钉定 + 全簿汇总对 props.formulas / merges /
//!    hyperlinks / cells；另钉合并范围 ref、超链接锚定格与外链 URL
//!    （sheet 部件 rels 独立还原）。
//! 2. [`xlsx_cell_value_anchors_match_source_xml`]——**坐标级**值锚点：
//!    首格 / 中部数值 / 中部公式（缓存值形态）/ 空格 / 末行超链接 /
//!    末行合并（左上 + 非左上）/ 末表末行布尔，8 格逐一
//!    「源 XML 独立解析值 == 渲染行模型取格」。共享字符串表
//!    （sharedStrings）间接由独立 SST 解析承载；布尔格钉 Rust 小写
//!    `true`（`CellData::Boolean` display 口径）。
//!
//! ## 独立性与假设
//!
//! - zip/部件解析工具复制自 render_fidelity.rs（集成测试各自独立
//!   crate——复制是本仓认可的模式）；所有期望值从源字节直接读出。
//! - 夹具 `smoke.xlsx` 由 `gen_smoke_xlsx`（--ignored）生成，写面
//!   （rust_xlsxwriter）恒产：`<c>` 首属性 `r`、简单 `<f>`（无共享公式
//!   si 形态）、自闭合 `<sheet/>`。钉定断言的红 = 夹具漂移信号
//!   （python3 + zipfile 再预检）。
//! - 合并范围非左上格（B5:C5 的 C5）源 XML 无 `<c>` 元素；calamine
//!   0.36.1 值面不展开合并（worksheet_range 值路径不触 merged_regions）
//!   → 渲染空串。本文件按源字面语义钉定（xlsx_doc.rs 模块注释的
//!   「calamine 复制左上值到范围」表述与实现不符——#203 已勘误为
//!   实装语义：左上单点显示，非左上格空串）。
//!
//! ## 铁律
//!
//! 只加测试不改产线；真实夹具（黄金例进仓，不许 SKIP）；失败信息
//! 指明哪个面、期望 vs 实际。

use std::path::{Path, PathBuf};

use mang_shell::editor::xlsx_doc::{self, XlsxPayload};

// ---------------------------------------------------------------------------
// 夹具（黄金例进仓——缺失即红，不许 SKIP）
// ---------------------------------------------------------------------------

fn fixture(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    if !p.is_file() {
        panic!(
            "夹具缺失（{}）——黄金例进仓，不许 SKIP；\
             由仓库内生成器再生成：cargo test gen_smoke_xlsx -- --ignored",
            p.display()
        );
    }
    p
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("夹具不可读（{name}）：{e}"))
}

// ---------------------------------------------------------------------------
// 独立解析工具（复制自 render_fidelity.rs——独立于被测代码的验证面）
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

/// 数 XML 元素出现次数（`<f>` / `<mergeCell …/>` 的开标签形态都算；
/// 前缀同名容器（`<mergeCells` / `<hyperlinks`）因尾随空格不误配）。
fn count_element(xml: &str, tag: &str) -> usize {
    xml.matches(&format!("<{tag} ")).count() + xml.matches(&format!("<{tag}>")).count()
}

// ---------------------------------------------------------------------------
// 本文件的独立解析面（xlsx 专属：部件寻址 / 属性 / SST / 单格值）
// ---------------------------------------------------------------------------

/// 取片段内属性值（`name="value"`——首次出现；找不到 → None）。
fn attr_in<'a>(frag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = frag.find(&needle)? + needle.len();
    let end = frag[start..].find('"')? + start;
    Some(&frag[start..end])
}

/// 取全部 `<tag …/>` 自闭合元素片段（属性都在片段内——workbook 的
/// `<sheet/>`、rels 的 `<Relationship/>`、sheet 的 `<mergeCell/>` 与
/// `<hyperlink/>` 四形态；`<tag ` 尾随空格保证不误配同名前缀容器）。
fn self_closing_fragments<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let needle = format!("<{tag} ");
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = xml[i..].find(&needle) {
        let start = i + rel;
        let end = xml[start..]
            .find("/>")
            .map(|e| start + e + 2)
            .unwrap_or(xml.len());
        out.push(&xml[start..end]);
        i = end;
    }
    out
}

/// rels Target 规范化为 zip 部件名（相对 xl/ → 前缀 xl/；前导 / 剥除）。
fn normalize_part(target: &str) -> String {
    let t = target.trim_start_matches('/');
    if t.starts_with("xl/") {
        t.to_string()
    } else {
        format!("xl/{t}")
    }
}

/// 工作表清单独立解析：workbook.xml `<sheet>` 簿序（name + r:id）→
/// workbook rels（r:id → Target）→ 部件 XML。
/// 返回 (表名, 部件名, 部件 XML)——**不信引擎自报的工作表树**。
fn sheets_with_xml(bytes: &[u8]) -> Vec<(String, String, String)> {
    let wb = part_text(bytes, "xl/workbook.xml");
    let rels = part_text(bytes, "xl/_rels/workbook.xml.rels");
    let mut out = Vec::new();
    for frag in self_closing_fragments(&wb, "sheet") {
        let name = attr_in(frag, "name")
            .unwrap_or_else(|| panic!("<sheet> 缺 name 属性：{frag}"))
            .to_string();
        let rid = attr_in(frag, "r:id").unwrap_or_else(|| panic!("<sheet {name}> 缺 r:id 属性"));
        let target = self_closing_fragments(&rels, "Relationship")
            .into_iter()
            .find(|r| attr_in(*r, "Id") == Some(rid))
            .and_then(|r| attr_in(r, "Target").map(str::to_string))
            .unwrap_or_else(|| panic!("workbook rels 缺 {rid}（sheet {name}）"));
        let part = normalize_part(&target);
        let xml = part_text(bytes, &part);
        out.push((name, part, xml));
    }
    out
}

/// sheet 部件的 rels 部件名（`xl/worksheets/sheet1.xml` →
/// `xl/worksheets/_rels/sheet1.xml.rels`——外链 Target 的存放地）。
fn sheet_rels_name(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => format!("{}/_rels/{}.rels", &part[..i], &part[i + 1..]),
        None => format!("_rels/{part}.rels"),
    }
}

/// XML 5 实体还原（与 office-io unescape_xml_entities5 同序——`&amp;`
/// 最后，避免 `&amp;amp;` 双解）。
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 拼接片段内全部 `<t>` 文本（简单 `<t>` 与带属性 `<t …>` 皆可；富文本
/// run `<r><t>…</t></r>` 的多 `<t>` 连接同覆盖）。
fn collect_t(fragment: &str) -> String {
    let mut text = String::new();
    let mut i = 0usize;
    while let Some(rel) = fragment[i..].find("<t") {
        let ts = i + rel;
        let Some(gt) = fragment[ts..].find('>') else {
            break;
        };
        let text_start = ts + gt + 1;
        let text_end = fragment[text_start..]
            .find("</t>")
            .map(|e| text_start + e)
            .unwrap_or(fragment.len());
        text.push_str(&xml_unescape(&fragment[text_start..text_end]));
        i = (text_end + 4).min(fragment.len());
    }
    text
}

/// 共享字符串表（SST）独立解析：`<si>` 逐条 → 文本。
fn shared_strings(bytes: &[u8]) -> Vec<String> {
    let sst = part_text(bytes, "xl/sharedStrings.xml");
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = sst[i..].find("<si>") {
        let start = i + rel + 4;
        let end = sst[start..]
            .find("</si>")
            .map(|e| start + e)
            .unwrap_or(sst.len());
        out.push(collect_t(&sst[start..end]));
        i = (end + 5).min(sst.len());
    }
    out
}

/// 取 `body` 中首个 `<open>…</close>` 内文（无 → None）。
fn inner(body: &str, open: &str, close: &str) -> Option<String> {
    let s = body.find(open)? + open.len();
    let e = body[s..].find(close)? + s;
    Some(body[s..e].to_string())
}

/// 独立解析 sheet XML 单格显示值（`<c r="X">` 定位——写面恒产 r 首属性；
/// 引号终结保证 A1 不误配 A10）。
///
/// 类型分派（与渲染层 `CellData::display` 同口径）：
/// `t="s"` → SST 间接；`t="b"` → `true`/`false`（Rust 小写）；
/// `t="str"` → 公式串结果原样；`t="inlineStr"` → `<is>` 内 `<t>`；
/// 无 `t` → 数值（Rust f64 格式化——公式缓存值同面）。
/// `None` = 该格无 `<c>` 元素（渲染空串——列宽由包围盒补齐）；
/// 有元素无 `<v>` 无 `<f>` → `Some("")`（空格）；
/// 有 `<f>` 无 `<v>` → `=公式`（无缓存回退形态——cell_display 同口径）。
fn xml_cell_display(sheet_xml: &str, sst: &[String], cell_ref: &str) -> Option<String> {
    let needle = format!("<c r=\"{cell_ref}\"");
    let start = sheet_xml.find(&needle)?;
    let open_end = start + sheet_xml[start..].find('>')? + 1;
    let open_tag = &sheet_xml[start..open_end];
    let body = if open_tag.ends_with("/>") {
        ""
    } else {
        let close = open_end + sheet_xml[open_end..].find("</c>")?;
        &sheet_xml[open_end..close]
    };
    let v = inner(body, "<v>", "</v>");
    let f = inner(body, "<f>", "</f>");
    if v.is_none() && f.is_none() {
        return Some(String::new());
    }
    Some(match attr_in(open_tag, "t") {
        Some("s") => {
            let idx: usize = v
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("{cell_ref} 共享串索引缺失/非法"));
            sst.get(idx)
                .cloned()
                .unwrap_or_else(|| panic!("sharedStrings[{idx}] 越界（共 {} 条）", sst.len()))
        }
        Some("b") => (v.as_deref() == Some("1")).to_string(),
        Some("str") => v.map(|s| xml_unescape(&s)).unwrap_or_default(),
        Some("inlineStr") => inner(body, "<is>", "</is>")
            .map(|is| collect_t(&is))
            .unwrap_or_default(),
        Some("e") => v.unwrap_or_default(),
        _ => match (v, f) {
            (Some(v), _) => {
                let n: f64 = v
                    .parse()
                    .unwrap_or_else(|_| panic!("{cell_ref} 数值体非法：{v}"));
                format!("{n}")
            }
            (None, Some(f)) => format!("={f}"),
            (None, None) => String::new(),
        },
    })
}

/// 独立数「有值格」：`<c …>` 元素体含 `<v>`（无元素 / 自闭合 / 无
/// `<v>` 体 = 空格不计——与渲染层 CellData::Empty 同口径；合并范围
/// 非左上格源 XML 无 `<c>` 元素，天然不计）。
fn count_cells_with_value(xml: &str) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while let Some(rel) = xml[i..].find("<c ") {
        let start = i + rel;
        let open_end = match xml[start..].find('>') {
            Some(e) => start + e + 1,
            None => break,
        };
        let open_tag = &xml[start..open_end];
        let body: &str = if open_tag.ends_with("/>") {
            ""
        } else {
            match xml[open_end..].find("</c>") {
                Some(e) => &xml[open_end..open_end + e],
                None => break,
            }
        };
        if body.contains("<v>") {
            n += 1;
        }
        i = open_end + body.len();
    }
    n
}

// ---------------------------------------------------------------------------
// 渲染模型取格（行模型 → 坐标）
// ---------------------------------------------------------------------------

/// 0 基列号 → 列字母（与 xlsx_doc::col_letter 同 base-26——消息可读用）。
fn col_name(mut col: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII 列字母")
}

/// 工作表区块行界（表分隔行之后 .. 下一表分隔行之前）。
fn sheet_section(payload: &XlsxPayload, lines_len: usize, sheet_idx: usize) -> (usize, usize) {
    let start = payload.headings[sheet_idx].line + 1;
    let end = payload
        .headings
        .get(sheet_idx + 1)
        .map(|h| h.line)
        .unwrap_or(lines_len);
    (start, end)
}

/// 定位区块内数据行（1 基绝对行号——行号槽右对齐 + 两空格分隔 +
/// `|` 分列是 xlsx_doc 数据行固定形态；列字母/分隔/提示行无行号前缀，
/// 天然跳过）。
fn rendered_row_line<'a>(lines: &'a [&'a str], section: (usize, usize), row_no: u32) -> &'a str {
    for &line in lines[section.0..section.1].iter() {
        let t = line.trim_start();
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && digits.parse::<u32>() == Ok(row_no) {
            return line;
        }
    }
    panic!(
        "区块（行 {}..{}）内未找到数据行 {row_no}",
        section.0, section.1
    );
}

/// 渲染模型取格：数据行按 `|` 分列后取第 col 列（0 基），尾部补齐
/// 空格剥除（trim——单元格文本本体不含首尾空格，夹具内容清单口径）。
fn rendered_cell(lines: &[&str], section: (usize, usize), row_no: u32, col: usize) -> String {
    let line = rendered_row_line(lines, section, row_no);
    let t = line.trim_start();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[digits.len()..];
    assert!(
        rest.starts_with("  "),
        "数据行 {row_no} 行号槽后应两空格分隔：{line:?}"
    );
    let cells: Vec<&str> = rest[2..].split('|').collect();
    assert!(
        col < cells.len(),
        "数据行 {row_no} 仅 {} 列（取第 {} 列越界）：{line:?}",
        cells.len(),
        col_name(col)
    );
    cells[col].trim().to_string()
}

// ---------------------------------------------------------------------------
// 测试 1：公式/合并/超链接/有值格——props ↔ 源 XML 独立对账
// ---------------------------------------------------------------------------

/// 结构对账（不信引擎自报）：分 sheet 独立数 `<f>` / `<mergeCell` /
/// `<hyperlink` / 有值 `<c>` ↔ props.formulas / merges / hyperlinks /
/// cells 汇总；工作表清单（workbook.xml 簿序 + rels 部件寻址）独立
/// 解析 ↔ 工作表树；合并范围 ref、超链接锚定格与外链 URL 逐项钉定。
#[test]
fn xlsx_formula_merge_hyperlink_counts_match_source_xml() {
    let bytes = fixture_bytes("smoke.xlsx");
    let payload = xlsx_doc::load(&fixture("smoke.xlsx")).expect("只读装载");

    // ── 工作表清单（簿序）独立解析 ↔ 渲染层工作表树 ──
    let parts = sheets_with_xml(&bytes);
    let names: Vec<&str> = parts.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["销售明细", "参数"],
        "工作表清单（workbook.xml 簿序独立解析）——夹具漂移请重新预检"
    );
    assert_eq!(
        payload.props.sheets,
        parts.len(),
        "工作表数：props={} vs workbook.xml <sheet> 独立实数={}",
        payload.props.sheets,
        parts.len()
    );
    let heading_names: Vec<&str> = payload.headings.iter().map(|h| h.text.as_str()).collect();
    assert_eq!(heading_names, names, "工作表树与源清单簿序逐字一致");

    // ── 分 sheet 独立计数（公式/合并/超链接/有值格）──
    let per_sheet: Vec<(&str, usize, usize, usize, usize)> = parts
        .iter()
        .map(|(name, _, xml)| {
            (
                name.as_str(),
                count_element(xml, "f"),
                count_element(xml, "mergeCell"),
                count_element(xml, "hyperlink"),
                count_cells_with_value(xml),
            )
        })
        .collect();
    // 夹具完整性钉定（生成器 gen_smoke_xlsx 内容清单——红即漂移信号）：
    // 表 1：1 公式（B4 SUM）+ 1 合并（B5:C5）+ 1 超链接（A5）+ 13 有值格；
    // 表 2：无公式/合并/超链接 + 4 有值格（分布面——0 值表不许漏计/翻倍）。
    assert_eq!(
        per_sheet[0],
        ("销售明细", 1, 1, 1, 13),
        "表 1 独立结构（公式/合并/超链接/有值格）——夹具漂移请重新预检"
    );
    assert_eq!(
        per_sheet[1],
        ("参数", 0, 0, 0, 4),
        "表 2 独立结构（0 值面钉定——读面把 0 计丢/翻倍在此现形）"
    );

    // ── 全簿汇总对账：props ↔ 独立实数 ──
    let (f_total, m_total, h_total, cell_total) = per_sheet
        .iter()
        .fold((0usize, 0usize, 0usize, 0usize), |(f, m, h, c), t| {
            (f + t.1, m + t.2, h + t.3, c + t.4)
        });
    assert_eq!(
        payload.props.formulas, f_total,
        "公式格数：props.formulas={} vs 全簿 <f> 独立实数={f_total}（分 sheet：{per_sheet:?}）",
        payload.props.formulas
    );
    assert_eq!(
        payload.props.merges, m_total,
        "合并范围数：props.merges={} vs 全簿 <mergeCell> 独立实数={m_total}（分 sheet：{per_sheet:?}）",
        payload.props.merges
    );
    assert_eq!(
        payload.props.hyperlinks, h_total,
        "超链接数：props.hyperlinks={} vs 全簿 <hyperlink> 独立实数={h_total}（分 sheet：{per_sheet:?}）",
        payload.props.hyperlinks
    );
    assert_eq!(
        payload.props.cells, cell_total,
        "非空数据格数：props.cells={} vs 全簿有值 <c> 独立实数={cell_total}\
         （合并范围非左上格源无 <c> 元素——不展开语义；分 sheet：{per_sheet:?}）",
        payload.props.cells
    );

    // ── 结构细节锚定：合并范围 ref + 超链接锚定格 + 外链 URL ──
    let (_, sheet1_part, sheet1_xml) = &parts[0];
    let merge_frag = self_closing_fragments(sheet1_xml, "mergeCell")
        .first()
        .copied()
        .expect("表 1 应有 <mergeCell>（上文计数已钉 1）");
    assert_eq!(
        attr_in(merge_frag, "ref"),
        Some("B5:C5"),
        "合并范围 ref（生成器内容清单：外部资料区落 B5:C5）"
    );
    let link_frag = self_closing_fragments(sheet1_xml, "hyperlink")
        .first()
        .copied()
        .expect("表 1 应有 <hyperlink>（上文计数已钉 1）");
    assert_eq!(
        attr_in(link_frag, "ref"),
        Some("A5"),
        "超链接锚定格（生成器内容清单：A5 → 季度报表门户）"
    );
    let rid = attr_in(link_frag, "r:id").expect("hyperlink r:id 属性");
    let rels_name = sheet_rels_name(sheet1_part);
    let srels = part_text(&bytes, &rels_name);
    let url = self_closing_fragments(&srels, "Relationship")
        .into_iter()
        .find(|r| attr_in(*r, "Id") == Some(rid))
        .and_then(|r| attr_in(r, "Target").map(str::to_string))
        .unwrap_or_else(|| panic!("{rels_name} 缺 {rid}"));
    assert_eq!(
        url, "https://example.com/report",
        "超链接外链 URL（{rels_name} 独立还原）"
    );

    // ── 护栏/降级面：小夹具不应触发 ──
    assert_eq!(
        payload.props.truncated_sheets, 0,
        "5 行 × 3 列小表不应触发行模型护栏"
    );
    assert_eq!(
        payload.props.warnings, 0,
        "干净夹具 compat 模式不应有降级警告"
    );
}

// ---------------------------------------------------------------------------
// 测试 2：坐标级单元格值锚点（源 XML 独立解析 ↔ 渲染行模型取格）
// ---------------------------------------------------------------------------

/// 8 格锚点（首格 / 中部数值 / 中部公式 / 空格 / 末行超链接 / 末行合并
/// 左上与非左上 / 末表末行布尔）：期望值全部从源 XML 独立解析（含
/// sharedStrings 间接、t="b" 布尔、公式缓存值），与渲染模型的对应
/// 坐标取格逐字相等——contains 式全文匹配不可定位的面在此补齐。
#[test]
fn xlsx_cell_value_anchors_match_source_xml() {
    let bytes = fixture_bytes("smoke.xlsx");
    let payload = xlsx_doc::load(&fixture("smoke.xlsx")).expect("只读装载");
    let lines: Vec<&str> = payload.text.split('\n').collect();

    // SST 独立解析（文本格 t="s" 的间接层——不信引擎自报）。
    let sst = shared_strings(&bytes);
    assert_eq!(sst.len(), 12, "sharedStrings 12 条——夹具漂移请重新预检");

    let parts = sheets_with_xml(&bytes);
    let heading_names: Vec<&str> = payload.headings.iter().map(|h| h.text.as_str()).collect();
    let part_names: Vec<&str> = parts.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(
        heading_names, part_names,
        "前置：工作表树与源清单簿序一致（主断言见测试 1）"
    );

    // (表序, 格引用, 1 基行, 0 基列, 说明)
    let anchors: [(usize, &str, u32, usize, &str); 8] = [
        (0, "A1", 1, 0, "首格（共享字符串表间接：sst[0]）"),
        (0, "B3", 3, 1, "中部数值格（Rust f64 格式化面）"),
        (0, "B4", 4, 1, "中部公式格（显示缓存值非公式文本）"),
        (0, "C3", 3, 2, "空格（源无 <c> 元素 → 空串，列不丢）"),
        (0, "A5", 5, 0, "末行超链接格（显示文本 sst[8]）"),
        (0, "B5", 5, 1, "末行合并格左上（范围值落点 sst[9]）"),
        (0, "C5", 5, 2, "末行合并格非左上（源无 <c> 元素 → 空串）"),
        (1, "B2", 2, 1, "末表末行布尔格（t=\"b\" → Rust 小写 true）"),
    ];
    for (si, cell_ref, row, col, what) in anchors {
        let (name, _, xml) = &parts[si];
        // None（源无元素）与 Some("")（空值体）都应渲染空串——列由
        // 包围盒补齐，空格不丢列。
        let expected = xml_cell_display(xml, &sst, cell_ref).unwrap_or_default();
        let section = sheet_section(&payload, lines.len(), si);
        let rendered = rendered_cell(&lines, section, row, col);
        assert_eq!(
            rendered,
            expected,
            "{what}——{name}!{cell_ref}（行 {row} 列 {}）：渲染 {rendered:?} != 源 XML 独立解析 {expected:?}",
            col_name(col)
        );
    }

    // 公式文本负向锚点（坐标级）：B4 所在数据行不得出现公式文本。
    let sec0 = sheet_section(&payload, lines.len(), 0);
    let row4 = rendered_row_line(&lines, sec0, 4);
    assert!(
        !row4.contains("SUM"),
        "公式格 B4 应显示缓存值而非公式文本：{row4:?}"
    );
}
