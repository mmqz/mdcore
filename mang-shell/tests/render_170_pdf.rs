//! #170 pdf 补测：第二条结构断言（书签/页锚点 + 跨页文本）+ 超大坏件。
//!
//! 与 `render_fidelity.rs` 的分工（不重复口径）：
//!
//! - 既有 `pdf_structure_matches_source_when_engine_available` 断：页数对账
//!   （props vs 独立 `/Type /Page` 计数）、三章书签 `any` 在场、scanned/chars/
//!   empty_pages、**首页**区间有正文行、全文 CJK 守卫；
//! - 本文件补：**书签树精确对账**（数量==3==源文件 /Title 串双口径、level、
//!   书签 line 锚 ↔ 目标页区间/页分隔行形态——静态勘察：lopdf Dest obj 8=页 1、
//!   obj 10=页 2）、**第 2 页**（非首页）文本层锚点、页表区间结构、行/span
//!   双轨对齐、超大件（>100MB 门）前置拒绝、尾部 30% 截断不 panic。
//!
//! ## 夹具静态勘察事实（smoke.pdf，112980 字节，PDF-1.5；python ToUnicode
//! CMap + lopdf 书签注入清单双源钉死——夹具再生成时同步本表）
//!
//! - 2 页（obj 8=页 1、obj 10=页 2）；书签 3 条（UTF-16BE 标题，单层）：
//!   第一章/第二章/第三章 → 页 1 / 页 2 / 页 2（0 基 0/1/1）；
//! - 第 1 页 19 文本行（尾行「机柜42台单价18000元」）；第 2 页 17 文本行
//!   （首行「UPS电源2套350000元」——第三章后半 + 第四章 + 第五章）。
//!
//! ## 环境纪律（与 render_fidelity 同款）
//!
//! 无 libpdfium（如 CI windows-latest）→ **引擎面**断言 SKIP 横幅
//! （eprintln 留证 + return，不伪装通过）；**源文件独立对账**（Phase A）
//! 与**坏件测试**不依赖引擎，两环境照常真跑（红即真缺陷）。
//!
//! ## 铁律
//!
//! 只加测试不改产线；临时坏件放 `std::env::temp_dir()`，Drop guard 清理
//! （panic 时也清理）；超大件一次分配一次写盘（禁逐字节 push）。

use std::path::{Path, PathBuf};

use mang_shell::editor::pdf_doc;
// 敌意夹具现场构造（④号测试——dev-dep lopdf，同 pdfe_security 惯例）。
use lopdf::{Object, dictionary};

/// 最小 CJK 占比（乱码近似 0，真实中文 0.4+——只抓「明显坏」）。
const MIN_CJK_RATIO: f64 = 0.25;

/// 夹具书签清单（`gen_smoke_pdf` inject_outlines 实参——**带空格**形态；
/// 正文标题行是不带空格形态「第一章项目概述」，勿混）。
const BOOKMARKS: [&str; 3] = ["第一章 项目概述", "第二章 技术方案", "第三章 报价明细"];

/// 书签 → 目标页（0 基；lopdf `/Dest[8 0 R]`=页 1、`/Dest[10 0 R]`=页 2）。
const BOOKMARK_PAGES: [usize; 3] = [0, 1, 1];

/// 第 2 页正文锚（ToUnicode CMap 解码勘察：第 2 页 17 行，含第四章/第五章
/// 全部——纯 CJK 锚 2 + ASCII 数字锚 1，跨三个 section）。
const PAGE2_ANCHORS: [&str; 3] = ["第五章服务承诺", "总价包含五年原厂维保", "96000"];

/// 第 1 页正文锚（纯 CJK——既有测试只断「首页有正文行」存在性，本文件
/// 升级为逐字锚点）。
const PAGE1_ANCHORS: [&str; 2] = ["服务网格统一治理东西向流量", "冷热通道隔离设计"];

// ---------------------------------------------------------------------------
// 辅助（复制自 render_fidelity.rs——集成测试各自独立 crate，复制是本仓
// 认可模式；坏件清理 guard 为本文件新增）
// ---------------------------------------------------------------------------

fn fixture(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    if !p.is_file() {
        panic!(
            "夹具缺失（{}）——黄金例进仓，不许 SKIP；\
             由仓库内生成器再生成：cargo test gen_smoke_pdf -- --ignored",
            p.display()
        );
    }
    p
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("夹具不可读（{name}）：{e}"))
}

/// 独立数 PDF 页：`/Type /Page`（`/Type/Page`），`/Pages` 用负字符类排除。
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

/// CJK 占比（中日韩表意 / 非空白字符）。
fn cjk_ratio(s: &str) -> f64 {
    let total = s.chars().filter(|c| !c.is_whitespace()).count();
    if total == 0 {
        return 0.;
    }
    let cjk = s
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            (0x4E00..=0x9FFF).contains(&cp)
                || (0x3400..=0x4DBF).contains(&cp)
                || (0xF900..=0xFAFF).contains(&cp)
        })
        .count();
    cjk as f64 / total as f64
}

/// UTF-16BE 编码（夹具书签标题在源 PDF 里的存储形态，无 BOM）。
fn utf16be(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for u in s.encode_utf16() {
        out.push((u >> 8) as u8);
        out.push((u & 0xFF) as u8);
    }
    out
}

/// 字节包含（windows 匹配——与 `String::contains` 同语义的字节面）。
fn bytes_contain(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// 临时目录 guard：Drop 时整目录删除（含 panic unwind 路径——测试末尾
/// 必清理，不残留 100MB 级文件）。
struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("render-170-pdf-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&p)
            .unwrap_or_else(|e| panic!("建临时目录（{}）：{e}", p.display()));
        Self(p)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// ① 第二条结构断言：书签树精确对账 + line↔页锚点 + 第 2 页文本层
// ---------------------------------------------------------------------------

/// pdf 第二条结构断言：书签树（数量/层级/line 锚 ↔ 目标页区间与页分隔行
/// 形态）+ 第 2 页（非首页）文本层锚点 + 页表区间结构 + 行/span 双轨对齐。
///
/// Phase A（源文件独立对账——不依赖引擎，两环境真跑，红即夹具漂移）；
/// Phase B（引擎面——无 libpdfium SKIP 横幅 + return，有则全量真跑）。
#[test]
fn pdf_bookmark_tree_and_cross_page_anchors_when_engine_available() {
    let path = fixture("smoke.pdf");
    let bytes = fixture_bytes("smoke.pdf");

    // ── Phase A：源文件独立对账（python 勘察双源：/Title 串 + /Dest 页）──
    let independent_pages = count_pdf_pages(&bytes);
    assert_eq!(
        independent_pages, 2,
        "夹具漂移：smoke.pdf 已知 2 页（实数 {independent_pages}）"
    );
    assert!(
        bytes_contain(&bytes, b"/Outlines"),
        "夹具漂移：smoke.pdf 应有 /Outlines（书签根）"
    );
    for t in BOOKMARKS {
        assert!(
            bytes_contain(&bytes, &utf16be(t)),
            "夹具漂移：源 PDF 缺书签标题（UTF-16BE）：{t:?}"
        );
    }

    // ── Phase B：引擎门控（同 render_fidelity 口径）──
    let payload = match pdf_doc::load(&path) {
        Ok(p) => p,
        Err(e) if e.contains("libpdfium") => {
            eprintln!(
                "\n══════ SKIP（环境缺件，不伪装通过）══════\n  \
                 本机无 libpdfium——pdf 第二条结构断言（书签/跨页锚点）未真跑。\n  \
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

    let lines: Vec<&str> = payload.text.split('\n').collect();

    // ── 书签树精确对账（数量双口径 + 层级）──
    assert_eq!(
        payload.headings.len(),
        BOOKMARKS.len(),
        "书签数：headings={} vs 源文件 /Title 串实数={}（多丢书签都算错——已有测试只断 >=3）",
        payload.headings.len(),
        BOOKMARKS.len()
    );
    assert_eq!(
        payload.props.bookmarks,
        payload.headings.len() as usize,
        "props.bookmarks={} vs headings.len()={}（两计数面失联）",
        payload.props.bookmarks,
        payload.headings.len()
    );
    for h in &payload.headings {
        assert_eq!(
            h.level, 1,
            "夹具书签为单层（lopdf 注入无嵌套），level 应 1：{h:?}"
        );
    }

    // ── 书签 line 锚 ↔ 目标页区间 + 页分隔行形态（跨页映射核心）──
    let total = payload.props.pages;
    for (title, page) in BOOKMARKS.iter().zip(BOOKMARK_PAGES) {
        let h = payload
            .headings
            .iter()
            .find(|h| h.text == *title)
            .unwrap_or_else(|| panic!("书签缺失：{title:?}（现有 {:?}）", BOOKMARKS));
        let p = payload
            .pages
            .get(page)
            .unwrap_or_else(|| panic!("书签目标页 {page} 不在页表（共 {total} 页）"));
        assert!(
            (p.first_line..=p.last_line).contains(&h.line),
            "书签「{title}」line={} 越出第 {} 页区间 [{}..={}]（跨页锚错位）",
            h.line,
            page + 1,
            p.first_line,
            p.last_line
        );
        let expect_sep = format!("── 第 {} / {} 页 ──", page + 1, total);
        assert_eq!(
            lines.get(h.line).copied(),
            Some(expect_sep.as_str()),
            "书签「{title}」line={} 应指向第 {} 页分隔行（页锚文本被改写/错位）",
            h.line,
            page + 1
        );
    }

    // ── 页表区间结构（文档序单调不重叠 + 每页首/末行形态）──
    assert_eq!(payload.pages.len(), total, "页表长度 vs props.pages");
    for (i, p) in payload.pages.iter().enumerate() {
        assert_eq!(p.index, i, "页表 index 应文档序连续");
        assert!(
            p.first_line <= p.last_line,
            "第 {} 页区间倒挂：[{}..={}]",
            i + 1,
            p.first_line,
            p.last_line
        );
        assert_eq!(
            lines.get(p.first_line).copied(),
            Some(format!("── 第 {} / {} 页 ──", i + 1, total).as_str()),
            "第 {} 页区间首行应为页分隔行",
            i + 1
        );
        assert_eq!(
            lines.get(p.last_line).copied(),
            Some(""),
            "第 {} 页区间末行应为页尾空行（块间呼吸）",
            i + 1
        );
        if i > 0 {
            assert!(
                payload.pages[i - 1].last_line < p.first_line,
                "第 {} 页与第 {} 页区间重叠/乱序：{} >= {}",
                i,
                i + 1,
                payload.pages[i - 1].last_line,
                p.first_line
            );
        }
    }

    // ── 行/span 双轨对齐 + 页分隔行 Quote 面 ──
    assert_eq!(
        payload.spans.len(),
        lines.len(),
        "spans 行数 {} vs 文本行数 {}（双轨失联）",
        payload.spans.len(),
        lines.len()
    );
    let first = &payload.pages[0];
    assert!(
        !payload.spans[first.first_line].is_empty(),
        "页分隔行应有整行 Quote span（muted 视觉面）"
    );

    // ── 第 2 页（非首页）文本层锚点——既有测试只覆盖首页 ──
    let p2 = &payload.pages[1];
    let page2_lines: Vec<&str> = lines[p2.first_line..=p2.last_line]
        .iter()
        .copied()
        .filter(|l| !l.is_empty() && !l.starts_with("──"))
        .collect();
    assert!(
        !page2_lines.is_empty(),
        "第 2 页（行 {}..={}）无正文行——跨页丢文本",
        p2.first_line,
        p2.last_line
    );
    for anchor in PAGE2_ANCHORS {
        assert!(
            page2_lines.iter().any(|l| l.contains(anchor)),
            "第 2 页锚点缺失：{anchor:?}\n（第 2 页实文：{page2_lines:?}）"
        );
    }
    // 第 1 页锚（纯 CJK——升级既有「存在性」断言为逐字锚点）。
    let p1 = &payload.pages[0];
    let page1_body: Vec<&str> = lines[p1.first_line..=p1.last_line]
        .iter()
        .copied()
        .filter(|l| !l.is_empty() && !l.starts_with("──"))
        .collect();
    for anchor in PAGE1_ANCHORS {
        assert!(
            page1_body.iter().any(|l| l.contains(anchor)),
            "第 1 页锚点缺失：{anchor:?}\n（第 1 页实文：{page1_body:?}）"
        );
    }

    // ── 第 2 页 CJK 守卫（跨页解码保真——不只首页）──
    let page2_text = page2_lines.join("\n");
    let ratio = cjk_ratio(&page2_text);
    assert!(
        ratio > MIN_CJK_RATIO,
        "第 2 页 CJK 占比 {ratio:.3} ≤ {MIN_CJK_RATIO}——第 2 页解码成碎片（乱码形态）"
    );
}

// ---------------------------------------------------------------------------
// ② 超大坏件：>100MB 门（INPUT_CAP_BYTES 前置拒绝）不 panic
// ---------------------------------------------------------------------------

/// 超大伪 pdf（合法 `%PDF-1.5` 头 + ~105MB 填充）→ 走 `load` 前置门：
/// **明确 Err**（含「上限」与实际字节数，且非 libpdfium 引擎装载失败——
/// 门控行为不依赖引擎可用性，有无 libpdfium 两环境同判），绝不 panic、
/// 绝不 Ok（本门实装形态是**错误拒绝**，无降级空文档路径）。
///
/// 门实装：`pdf_doc.rs` `INPUT_CAP_BYTES = 100*1024*1024`（100MB），
/// `load` 读盘前 `fs::metadata` 超限即 `Err("文件超过 pdf 输入上限（…字节 > …字节）")`。
#[test]
fn oversized_pdf_is_rejected_before_read() {
    // 105MB（110_100_480 字节）> 门限 100MB（104_857_600）——任务口径 ~105MB。
    const SIZE: usize = 105 * 1024 * 1024;
    let dir = TempDirGuard::new("oversized");
    let path = dir.path("oversized_105mb.pdf");

    // 一次分配 + 一次写盘（105MB 实测 ~0.1s；禁逐字节 push）。
    let mut buf = vec![0u8; SIZE];
    let head: &[u8] = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n";
    assert!(head.len() < SIZE);
    buf[..head.len()].copy_from_slice(head);
    std::fs::write(&path, &buf)
        .unwrap_or_else(|e| panic!("写超大伪 pdf（{}）：{e}", path.display()));
    drop(buf); // 及时释放 105MB。

    match pdf_doc::load(&path) {
        Err(e) => {
            assert!(
                e.contains("上限"),
                "超大件 Err 应为前置门拒绝（含「上限」），实际：{e:?}"
            );
            assert!(
                e.contains("字节"),
                "超大件 Err 应含字节数对账（「…字节 > …字节」），实际：{e:?}"
            );
            assert!(
                e.contains(&SIZE.to_string()),
                "Err 应含实际字节数 {SIZE}（证明门真读了 metadata），实际：{e:?}"
            );
            assert!(
                !e.contains("libpdfium"),
                "Err 应来自前置门而非引擎装载失败（门不依赖引擎可用性）：{e:?}"
            );
        }
        Ok(p) => panic!(
            "超大件（{SIZE} 字节 > 100MB 门限）必须前置 Err，\
             不应 Ok（pages={}）——本门实装无降级空文档路径",
            p.props.pages
        ),
    }
    // TempDirGuard Drop 清理（panic 时同样清理）。
}

// ---------------------------------------------------------------------------
// ③ 尾部截断对象树（30%）不 panic——与既有「前 1000 字节头部截断」区分口径
// ---------------------------------------------------------------------------

/// smoke.pdf 去掉**尾部 30%**（xref/startxref/%%EOF 消失，前 70% 对象树
/// 保留——与既有 `truncated_files_do_not_panic` 的**头部截断**（前 1000
/// 字节，几乎无完整对象）不同质：这里是「对象树尾部损坏」口径）。
///
/// 判据：不 panic——Err（消息非空；无 libpdfium 环境为引擎装载失败，合法）
/// 或 Ok（产物有界：页数 ≤ 完整件 2 页）。
#[test]
fn tail_truncated_pdf_does_not_panic() {
    let full = fixture_bytes("smoke.pdf");
    let cut = full.len() * 7 / 10; // 112980 * 0.7 = 79086（尾部 30% 截掉）
    let dir = TempDirGuard::new("tail-trunc");
    let path = dir.path("tail_truncated.pdf");
    std::fs::write(&path, &full[..cut])
        .unwrap_or_else(|e| panic!("写尾部截断件（{}）：{e}", path.display()));

    match pdf_doc::load(&path) {
        Ok(payload) => {
            let full_pages = count_pdf_pages(&full);
            assert!(
                payload.props.pages <= full_pages,
                "尾部截断件装载出 {} 页 > 完整件 {full_pages} 页（无界产物）",
                payload.props.pages
            );
        }
        Err(e) => assert!(
            !e.is_empty(),
            "尾部截断件的 Err 空消息（fail-loud 破坏）：{e:?}"
        ),
    }
    // TempDirGuard Drop 清理。
}

// ---------------------------------------------------------------------------
// ④ S2/S6 打开扫描接线 e2e（#277 / 审计 P1-11 桌面接线——引擎门控）
// ---------------------------------------------------------------------------

/// 危险对象 PDF（catalog 挂 `/OpenAction`）打开 → 行模型首行落
/// 【安全提示】告警行（S2.4 只读告警——**不阻断打开**：页表照常建立）。
///
/// 夹具现场构造（lopdf dev-dep——pdfe_security 同款惯例，零仓外依赖）：
/// smoke.pdf 是良性件（零告警——①号测试已锁），敌意件不进仓（黄金例
/// 纪律只收良性基线，敌意载荷按需现场造）。
///
/// 引擎门控（同 ①号 Phase B 口径）：无 libpdfium SKIP 横幅 + return；
/// 敌意件本身合法可开，引擎在场则全量真跑。
#[test]
fn dangerous_pdf_opens_with_security_notice_when_engine_available() {
    // 现场构造：最小单页（无文本层）+ catalog /OpenAction（S2 族）。
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ],
    }));
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "OpenAction" => dictionary! { "S" => Object::Name(b"GoTo".to_vec()) },
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("敌意夹具序列化");
    // Phase A 独立对账（不依赖引擎）：源里确有 /OpenAction（防构造漂移
    // 把 S2 锁退化成「良性件零告警」恒真锁）。
    assert!(
        bytes_contain(&bytes, b"/OpenAction"),
        "夹具漂移：敌意件缺 /OpenAction（S2 检测面失锚）"
    );

    let dir = TempDirGuard::new("s2-notice");
    let path = dir.path("openaction.pdf");
    std::fs::write(&path, &bytes).unwrap_or_else(|e| panic!("写敌意件：{e}"));

    match pdf_doc::load(&path) {
        Ok(payload) => {
            let lines: Vec<&str> = payload.text.split('\n').collect();
            // S2 告警行落行模型最首（先于扫描件提示——安全语义优先）。
            let first = lines[0];
            assert!(
                first.contains("【安全提示】"),
                "S2 命中必须落首行告警（S2.4 只读告警），实际首行：{first:?}"
            );
            assert!(
                first.contains("打开动作") || first.contains("/OpenAction"),
                "告警应指认族名（OpenAction）：{first:?}"
            );
            assert!(
                first.contains("只读不执行"),
                "告警应声明只读语义（不阻断 + 零执行面）：{first:?}"
            );
            // 无 /AcroForm → S6 不误报（误报 = 告警面失效）。
            assert!(
                !payload.text.contains("【表单提示】"),
                "本夹具无 /AcroForm——S6 不应误报"
            );
            // 不阻断打开：页表照常建立；页锚点行 = 页分隔行（提示行之后）。
            assert_eq!(payload.props.pages, 1, "告警不阻断打开（页数照常）");
            let anchor = payload.pages[0].first_line;
            assert!(anchor > 0, "页锚点应在提示行区之后：{anchor}");
            assert!(
                lines[anchor].starts_with("── 第 1 / 1 页 ──"),
                "页锚点行应为页分隔行，实际：{:?}",
                lines[anchor]
            );
        }
        Err(e) if e.contains("libpdfium") => {
            eprintln!(
                "\n══════ SKIP（环境缺件，不伪装通过）══════\n  \
                 本机无 libpdfium——S2 告警接线 e2e 未真跑。\n  \
                 引擎错误：{e}\n  \
                 有 libpdfium 的环境（./smoke.sh 5 tests/fixtures/smoke.pdf）全量执行。\n\
                 ══════════════════════════════════════\n"
            );
        }
        Err(e) => panic!("敌意件打开失败（非引擎缺件——接线缺陷）：{e:?}"),
    }
    // TempDirGuard Drop 清理。
}
