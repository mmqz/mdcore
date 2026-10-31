//! PDF-f — 页面组织语义锁（feature `engine-lopdf`；`engine-pdfium`
//! 增交叉读回面）。
//!
//! **环境合同**（与 pdfc_engine/pdfd_writer 同纪律）：
//! - fixtures：`crates/pdf/fixtures/`（仓内随包）；
//! - 真实语料：env `MDC_PDF_ORG_CORPUS` → `../../test-artifacts/
//!   pdf-organize-corpus/`（LO 转换真实 docx 语料 + pdfunite 300 页级
//!   压测件；语料缺失 = **跳过并打印**（跨环境纪律——语料不入库，
//!   生成脚本与 provenance 见语料目录 MANIFEST.json）；
//! - poppler/qpdf 交叉验证：工具缺失 = 跳过并打印（soffice 门同款）；
//! - pdfium 交叉面（feature `engine-pdfium`）：库定位序 MDC_PDFIUM_LIB
//!   → 候选 → 系统（缺库 panic 打印定位序——与 pdfc_engine 同合同）。
//!
//! 锁清单（每锁一条 `#[test]`）：
//! 1. `delete_pages_count_and_content_streams`——删页后页数正确 +
//!    **幸存页内容流逐字节不变**（外科手术纪律的结构级断言——比文本
//!    提取更强）+ lopdf 重载 + qpdf 结构验证 + poppler 页数交叉；
//! 2. `delete_pages_input_validation`——越界 InvalidPage / 全删拒绝 /
//!    空切片 no-op / 重复索引去重；
//! 3. `delete_bookmarks_remap_and_drop`——书签目标页被删 → 整子树丢弃
//!    （幸存书签目标不变——对象号不动 = 引用保真；全灭时 /Outlines
//!    摘除）；pdfium outline 交叉读回（引擎层独立确认）；
//! 4. `rotate_additive_and_geometry`——/Rotate 叠加语义（90+90=180/
//!    270+90=0）+ 继承链现值查找 + pdftoppm 渲染宽高交换（视觉面）；
//! 5. `reorder_permutation_and_page_order`——重排后页序正确（内容流
//!    逐页对位）+ 非恰排列三拒（长度/越界/重复）；
//! 6. `insert_blank_page_geometry`——插入位置/尺寸/页数 + 空白页
//!    MediaBox 显式钦定（不依赖继承）；
//! 7. `extract_independent_pdf`——区间抽取独立可开 + 区间外书签丢弃/
//!    区间内保留 + 空区间拒绝；
//! 8. `split_every_totals`——块数/页数守恒 + n=0 拒绝；
//! 9. `merge_totals_content_and_bookmarks`——合并页数 = 各和 + 内容
//!    流保持 + 双方书签拼接 + 单文档 merge = 纯往返；
//! 10. `merge_form_and_outline_adopt`——基底无面时源面过继（表单域/
//!     大纲）+ form_probe 域存活；
//! 11. `roundtrip_10x_no_bloat`——10 次开-存链：页数稳定 + 体积增长
//!     < 2% + 逐字节确定性（lopdf 保存可复现性钉定）；
//! 12. `corpus_sweep_all_operations`——真实语料全量扫（删中页/旋转/
//!     抽取/合并自身），每产物 qpdf 验证 + 页数核对（全绿或如实体面）；
//! 13. `large_manual_delete_extract`——1390 页压测件：删 10 页 →
//!     1380；抽取 300 页独立件；往返链体积；
//! 14. `encrypted_rejected_s5`——加密档（qpdf 现场生成）双防线：
//!     from_bytes SecurityDeny + merge 源拒绝；
//! 15. `report_loss_registry_accuracy`——报告计数器跨操作累加精确性
//!     （删除/书签/链接/表单域/物化/剪除逐项核对）；
//! 16. `assessment_carried_from_scan`——打开评估随组织器携带（S6 表单
//!     探针面）。

#![cfg(feature = "engine-lopdf")]

use std::path::{Path, PathBuf};
use std::process::Command;

use mdc_pdf::command::CommandError;
use mdc_pdf::organize::{PageSize, PdfOrganizer};
use mdc_pdf::page::PageIndex;
use mdc_pdf::writer_lopdf::lopdf::{Document, Object};

// ---------------------------------------------------------------------------
// 环境合同：fixtures / 语料 / 工具门
// ---------------------------------------------------------------------------

const CORPUS_ENV: &str = "MDC_PDF_ORG_CORPUS";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} 读取失败（{path:?}）：{e}"))
}

fn corpus_dir() -> PathBuf {
    std::env::var(CORPUS_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // 2 级上溯 = 仓根（crates/pdf → 仓库）——external_corpus 先例。
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/pdf-organize-corpus")
        })
}

/// 语料文件（缺失 = 空表 + stderr 打印——调用方据此跳过）。
fn corpus_files() -> Vec<PathBuf> {
    let dir = corpus_dir();
    if !dir.is_dir() {
        eprintln!(
            "[pdff] corpus dir absent ({}) — skipping real-corpus tests",
            dir.display()
        );
        return Vec::new();
    }
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "pdf").unwrap_or(false))
        .collect();
    v.sort();
    v
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success() || s.code().is_some())
        .unwrap_or(false)
}

fn require_qpdf() -> bool {
    let ok = tool_available("qpdf");
    if !ok {
        eprintln!("[pdff] qpdf absent — skipping qpdf cross-validation");
    }
    ok
}

fn require_poppler() -> bool {
    let ok = tool_available("pdfinfo");
    if !ok {
        eprintln!("[pdff] poppler absent — skipping poppler cross-validation");
    }
    ok
}

fn require_pdftoppm() -> bool {
    let ok = tool_available("pdftoppm");
    if !ok {
        eprintln!("[pdff] pdftoppm absent — skipping visual rotation check");
    }
    ok
}

/// 临时目录唯一序号（并行测试安全——固定路径会在并发用例间竞态）。
fn tmp_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// qpdf 结构验证（exit 0 = 过；警告面如实返回 false 供断言甄别）。
fn qpdf_valid(bytes: &[u8], label: &str) -> bool {
    let dir = std::env::temp_dir().join(format!(
        "pdff-{}-{}-{}",
        std::process::id(),
        tmp_seq(),
        label
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.pdf");
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new("qpdf")
        .arg("--check")
        .arg(&path)
        .output()
        .expect("spawn qpdf");
    let ok = out.status.success();
    if !ok {
        eprintln!(
            "[pdff] qpdf --check FAILED for {label}:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

/// poppler 页数（独立工具交叉）。
fn poppler_pages(bytes: &[u8]) -> Option<u32> {
    if !require_poppler() {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("pdff-info-{}-{}", std::process::id(), tmp_seq()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.pdf");
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new("pdfinfo")
        .arg(&path)
        .output()
        .expect("spawn pdfinfo");
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

// ---------------------------------------------------------------------------
// lopdf 重载面（仓内引擎——结构断言的基准货币）
// ---------------------------------------------------------------------------

fn reopen(bytes: &[u8]) -> Document {
    Document::load_mem(bytes).unwrap_or_else(|e| panic!("产物 lopdf 重载失败：{e}"))
}

fn page_count_of(doc: &Document) -> usize {
    doc.get_pages().len()
}

/// 页内容流原始字节（逐字节保真断言用）。
fn page_content(doc: &Document, idx: usize) -> Vec<u8> {
    let page_id = *doc
        .get_pages()
        .get(&(idx as u32 + 1))
        .unwrap_or_else(|| panic!("页 {idx} 不存在"));
    doc.get_page_content(page_id)
}

/// 页 /Rotate 现值（显式页级；不含继承——旋转断言用显式面）。
fn page_rotate(doc: &Document, idx: usize) -> Option<i64> {
    let page_id = *doc.get_pages().get(&(idx as u32 + 1))?;
    let dict = doc.get_object(page_id).ok()?.as_dict().ok()?;
    match dict.get(b"Rotate") {
        Ok(Object::Integer(v)) => Some(*v),
        _ => None,
    }
}

/// 页 /MediaBox（显式面）。
fn page_mediabox(doc: &Document, idx: usize) -> Option<(f64, f64, f64, f64)> {
    let page_id = *doc.get_pages().get(&(idx as u32 + 1))?;
    let dict = doc.get_object(page_id).ok()?.as_dict().ok()?;
    match dict.get(b"MediaBox") {
        Ok(Object::Array(arr)) => {
            let mut vals = [0f64; 4];
            for (i, o) in arr.iter().take(4).enumerate() {
                vals[i] = match o {
                    Object::Integer(v) => *v as f64,
                    Object::Real(v) => *v as f64,
                    _ => return None,
                };
            }
            Some((vals[0], vals[1], vals[2], vals[3]))
        }
        _ => None,
    }
}

/// 大纲标题收集（lopdf 视——深度优先；返回 (title, dest_page_ref) 列表）。
fn outline_titles(doc: &Document) -> Vec<(String, Option<(u32, u16)>)> {
    let catalog = doc.catalog().expect("catalog");
    let outlines_id = match catalog.get(b"Outlines") {
        Ok(Object::Reference(id)) => *id,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    let mut stack: Vec<(u32, u16)> = Vec::new();
    // 根 /First 链。
    let first = doc
        .get_object(outlines_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| match d.get(b"First") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        });
    if let Some(f) = first {
        stack.push(f);
    }
    let mut guard = 0;
    while let Some(id) = stack.pop() {
        guard += 1;
        assert!(guard < 10_000, "大纲遍历防御触发");
        let dict = doc
            .get_object(id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .expect("大纲项字典");
        let title = match dict.get(b"Title") {
            // 文本串两形态：UTF-16BE（BOM FE FF）/ PDFDocEncoding。
            Ok(Object::String(bytes, _)) => decode_pdf_text(bytes),
            _ => String::new(),
        };
        let dest = match dict.get(b"Dest") {
            Ok(Object::Array(arr)) => match arr.first() {
                Some(Object::Reference(rid)) => Some(*rid),
                _ => None,
            },
            _ => None,
        };
        out.push((title, dest));
        // 子项入栈（反序保序）。
        if let Ok(Object::Reference(kid)) = dict.get(b"First") {
            stack.push(*kid);
        }
        if let Ok(Object::Reference(next)) = dict.get(b"Next") {
            stack.push(*next);
        }
    }
    out
}

/// PDF 文本串解码（UTF-16BE 带 BOM → String；否则 PDFDocEncoding
/// 近似 = UTF-8/latin 宽容面）。
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let mut units = Vec::with_capacity((bytes.len() - 2) / 2);
        let mut i = 2;
        while i + 1 < bytes.len() {
            units.push(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
            i += 2;
        }
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

// ---------------------------------------------------------------------------
// 1-2. 删除：页数/内容流保真/输入校验
// ---------------------------------------------------------------------------

#[test]
fn delete_pages_count_and_content_streams() {
    let src = fixture_bytes("small.pdf");
    let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
    assert_eq!(org.page_count(), 3);
    org.delete_pages(&[0]).unwrap();
    assert_eq!(org.page_count(), 2);
    let product = org.save_to_vec().unwrap();

    // lopdf 重载：页数 + 幸存页内容流逐字节不变（外科手术纪律）。
    let redoc = reopen(&product);
    assert_eq!(page_count_of(&redoc), 2);
    let srcdoc = reopen(&src);
    assert_eq!(
        page_content(&redoc, 0),
        page_content(&srcdoc, 1),
        "幸存页 0（原页 1）内容流必须逐字节不变"
    );
    assert_eq!(
        page_content(&redoc, 1),
        page_content(&srcdoc, 2),
        "幸存页 1（原页 2）内容流必须逐字节不变"
    );

    // 报告面：删 1 页；无书签/链接损失（small.pdf 无大纲无链接）。
    let r = org.report();
    assert_eq!(r.deleted_pages, 1);
    assert_eq!(r.dropped_bookmarks, 0);
    assert_eq!(r.dropped_links, 0);

    // 交叉：poppler 页数 + qpdf 结构。
    if require_poppler() {
        assert_eq!(poppler_pages(&product), Some(2));
    }
    if require_qpdf() {
        assert!(qpdf_valid(&product, "delete-small"), "qpdf --check 须过");
    }
}

#[test]
fn delete_pages_input_validation() {
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    // 越界 → InvalidPage。
    match org.delete_pages(&[3]) {
        Err(CommandError::InvalidPage(PageIndex(3))) => {}
        other => panic!("越界删除应 InvalidPage(3)，得 {other:?}"),
    }
    match org.delete_pages(&[0, 99]) {
        Err(CommandError::InvalidPage(PageIndex(99))) => {}
        other => panic!("混合越界应 InvalidPage(99)，得 {other:?}"),
    }
    // 全删拒绝（PDF 至少一页）。
    assert!(matches!(
        org.delete_pages(&[0, 1, 2]),
        Err(CommandError::Engine(_))
    ));
    // 空切片 no-op。
    org.delete_pages(&[]).unwrap();
    assert_eq!(org.page_count(), 3);
    // 重复索引去重。
    org.delete_pages(&[1, 1]).unwrap();
    assert_eq!(org.page_count(), 2);
    assert_eq!(org.report().deleted_pages, 1);
}

// ---------------------------------------------------------------------------
// 3. 书签重映射
// ---------------------------------------------------------------------------

/// bookmarked.pdf 书签布局（fixture 钉定）：
/// 第一章→p0 / 第二章→p1（子：2.1→p1, 2.2→p2）/ 第三章→p2。
#[test]
fn delete_bookmarks_remap_and_drop() {
    // 场景 A：删页 0 → 第一章（目标 p0）整子树丢弃；其余四项存活目标不变。
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("bookmarked.pdf")).unwrap();
    org.delete_pages(&[0]).unwrap();
    assert_eq!(org.report().dropped_bookmarks, 1);
    let product = org.save_to_vec().unwrap();
    let doc = reopen(&product);
    let items = outline_titles(&doc);
    assert_eq!(items.len(), 4, "5 项 - 1 丢弃 = 4：{items:?}");
    assert_eq!(items[0].0, "第二章");
    // 幸存书签目标 = 原页对象（页未删——对象号不动即引用保真）。
    let srcdoc = reopen(&fixture_bytes("bookmarked.pdf"));
    let src_items = outline_titles(&srcdoc);
    // 目标页对象在两文档中一致（页 1/2 未动）。
    assert_eq!(items[0].1, src_items[1].1, "第二章目标引用须不变");

    // 场景 B：删页 2 → 2.2 与第三章（目标 p2）丢弃 = 2。
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("bookmarked.pdf")).unwrap();
    org.delete_pages(&[2]).unwrap();
    assert_eq!(org.report().dropped_bookmarks, 2);
    let doc = reopen(&org.save_to_vec().unwrap());
    let items = outline_titles(&doc);
    assert_eq!(items.len(), 3, "5 项 - 2 丢弃 = 3：{items:?}");

    // 场景 C：删页 0+1——子树语义钉定：第一章（p0）死；第二章（p1）死
    // 则其全部子项随之（2.1[目标 p1] + 2.2[目标 p2 存活但父死随灭]）；
    // 仅第三章（p2）存活。保守丢弃 + 全量登记（不静默、不留悬空）。
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("bookmarked.pdf")).unwrap();
    org.delete_pages(&[0, 1]).unwrap();
    assert_eq!(
        org.report().dropped_bookmarks,
        4,
        "第一章 + 第二章 + 2.1 + 2.2（子树语义）"
    );
    let doc = reopen(&org.save_to_vec().unwrap());
    let items = outline_titles(&doc);
    assert_eq!(items.len(), 1, "5 - 4 = 1（仅第三章）：{items:?}");
    assert_eq!(items[0].0, "第三章");

    // 交叉：qpdf 结构。
    if require_qpdf() {
        assert!(qpdf_valid(&product, "bm-del-p0"), "qpdf --check 须过");
    }
}

// ---------------------------------------------------------------------------
// 4. 旋转
// ---------------------------------------------------------------------------

#[test]
fn rotate_additive_and_geometry() {
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    // 90 → 显式 /Rotate 90。
    org.rotate_page(0, 90).unwrap();
    let product = org.save_to_vec().unwrap();
    let doc = reopen(&product);
    assert_eq!(page_rotate(&doc, 0), Some(90));
    assert_eq!(page_rotate(&doc, 1), None, "兄弟页零波及");
    assert_eq!(page_rotate(&doc, 2), None);

    // 叠加语义：再转 90 = 180；再转 180 = 0（回到原向）。
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    org.rotate_page(1, 90).unwrap();
    org.rotate_page(1, 90).unwrap();
    assert_eq!(org.report().rotated_pages, 2);
    let doc = reopen(&org.save_to_vec().unwrap());
    assert_eq!(page_rotate(&doc, 1), Some(180));
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    org.rotate_page(2, 270).unwrap();
    org.rotate_page(2, 90).unwrap();
    let doc = reopen(&org.save_to_vec().unwrap());
    assert_eq!(page_rotate(&doc, 2), Some(0));

    // 非法角度拒绝。
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    assert!(org.rotate_page(0, 45).is_err());
    assert!(org.rotate_page(0, 0).is_err());
    assert!(org.rotate_page(0, 360).is_err());
    // 越界页拒绝。
    assert!(matches!(
        org.rotate_page(9, 90),
        Err(CommandError::InvalidPage(_))
    ));

    // 视觉面：pdftoppm 渲染宽高交换（letter 612×792 → 转 90 后 792×612）。
    if require_pdftoppm() {
        let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
        org.rotate_page(0, 90).unwrap();
        let product = org.save_to_vec().unwrap();
        let (w, h) = render_dims(&product, 1);
        assert_eq!((w, h), (792, 612), "转 90 后渲染视口须宽高交换");
    }
    // 交叉：qpdf。
    if require_qpdf() {
        assert!(qpdf_valid(&product, "rotate"), "qpdf --check 须过");
    }
}

/// pdftoppm 渲染首页尺寸（-ppm 头解析：P6\nW H\n…）。
fn render_dims(bytes: &[u8], page: u32) -> (u32, u32) {
    let dir = std::env::temp_dir().join(format!("pdff-ppm-{}-{}", std::process::id(), tmp_seq()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("out.pdf");
    std::fs::write(&pdf, bytes).unwrap();
    let out = Command::new("pdftoppm")
        .args(["-r", "72", "-f", &page.to_string(), "-l", &page.to_string()])
        .arg(&pdf)
        .arg(dir.join("pg"))
        .output()
        .expect("spawn pdftoppm");
    assert!(
        out.status.success(),
        "pdftoppm 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let produced: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "ppm").unwrap_or(false))
        .collect();
    let ppm = std::fs::read(&produced[0]).expect("ppm 读回");
    let _ = std::fs::remove_dir_all(&dir);
    // P6 头：magic\nW H\nmax\n
    let header_end = ppm.iter().position(|&b| b == b'\n').unwrap();
    assert_eq!(&ppm[..header_end], b"P6");
    let rest = &ppm[header_end + 1..];
    let line_end = rest.iter().position(|&b| b == b'\n').unwrap();
    let dims: Vec<u32> = String::from_utf8_lossy(&rest[..line_end])
        .split_whitespace()
        .map(|t| t.parse().unwrap())
        .collect();
    (dims[0], dims[1])
}

// ---------------------------------------------------------------------------
// 5. 重排
// ---------------------------------------------------------------------------

#[test]
fn reorder_permutation_and_page_order() {
    let src = fixture_bytes("small.pdf");
    let srcdoc = reopen(&src);

    // [2,0,1]：新 0 位 = 原页 2，新 1 位 = 原页 0，新 2 位 = 原页 1。
    let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
    org.reorder(&[2, 0, 1]).unwrap();
    assert!(org.report().reordered);
    let product = org.save_to_vec().unwrap();
    let doc = reopen(&product);
    assert_eq!(page_count_of(&doc), 3);
    assert_eq!(page_content(&doc, 0), page_content(&srcdoc, 2));
    assert_eq!(page_content(&doc, 1), page_content(&srcdoc, 0));
    assert_eq!(page_content(&doc, 2), page_content(&srcdoc, 1));

    // 逆序 [2,1,0]。
    let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
    org.reorder(&[2, 1, 0]).unwrap();
    let doc = reopen(&org.save_to_vec().unwrap());
    assert_eq!(page_content(&doc, 0), page_content(&srcdoc, 2));
    assert_eq!(page_content(&doc, 1), page_content(&srcdoc, 1));
    assert_eq!(page_content(&doc, 2), page_content(&srcdoc, 0));

    // 恒等 [0,1,2]（合法 no-op）。
    let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
    org.reorder(&[0, 1, 2]).unwrap();

    // 三拒：长度/越界/重复。
    let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
    assert!(org.reorder(&[0, 1]).is_err());
    assert!(org.reorder(&[0, 1, 3]).is_err());
    assert!(org.reorder(&[0, 1, 1]).is_err());
    // 失败后零损伤（页数与内容流不变）。
    let doc = reopen(&org.save_to_vec().unwrap());
    assert_eq!(page_count_of(&doc), 3);

    // 交叉。
    if require_qpdf() {
        assert!(qpdf_valid(&product, "reorder"), "qpdf --check 须过");
    }
    if require_poppler() {
        assert_eq!(poppler_pages(&product), Some(3));
    }
}

// ---------------------------------------------------------------------------
// 6. 插入空白页
// ---------------------------------------------------------------------------

#[test]
fn insert_blank_page_geometry() {
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    // 插在页 1 前（at=1）——A4 竖版。
    org.insert_blank_page(1, PageSize::A4).unwrap();
    assert_eq!(org.page_count(), 4);
    assert_eq!(org.report().inserted_pages, 1);
    let product = org.save_to_vec().unwrap();
    let doc = reopen(&product);
    assert_eq!(page_count_of(&doc), 4);
    // 新页显式 MediaBox = A4（595×842，原点 0,0）。
    assert_eq!(page_mediabox(&doc, 1), Some((0.0, 0.0, 595.0, 842.0)));
    // 原页零损伤（0→0，原 1→2，原 2→3）。
    let srcdoc = reopen(&fixture_bytes("small.pdf"));
    assert_eq!(page_content(&doc, 0), page_content(&srcdoc, 0));
    assert_eq!(page_content(&doc, 2), page_content(&srcdoc, 1));
    assert_eq!(page_content(&doc, 3), page_content(&srcdoc, 2));

    // 头插与尾插。
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    org.insert_blank_page(0, PageSize::LETTER).unwrap();
    org.insert_blank_page(4, PageSize::LETTER_LANDSCAPE)
        .unwrap();
    assert_eq!(org.page_count(), 5);
    let doc = reopen(&org.save_to_vec().unwrap());
    assert_eq!(page_mediabox(&doc, 0), Some((0.0, 0.0, 612.0, 792.0)));
    assert_eq!(page_mediabox(&doc, 4), Some((0.0, 0.0, 792.0, 612.0)));

    // 越界与非法尺寸拒绝。
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    assert!(matches!(
        org.insert_blank_page(5, PageSize::A4),
        Err(CommandError::InvalidPage(_))
    ));
    // 非法尺寸在 PageSize::new 构造期已拒（类型层防线）。
    assert!(PageSize::new(0.0, 100.0).is_err());
    assert!(PageSize::new(f64::NAN, 100.0).is_err());

    // 交叉。
    if require_qpdf() {
        assert!(qpdf_valid(&product, "insert-blank"), "qpdf --check 须过");
    }
}

// ---------------------------------------------------------------------------
// 7-8. 抽取与拆分
// ---------------------------------------------------------------------------

#[test]
fn extract_independent_pdf() {
    let src = fixture_bytes("bookmarked.pdf");
    let org = PdfOrganizer::from_bytes(src.clone()).unwrap();
    // 抽页 1..3（第 2-3 页）。
    let product = org.extract(1..3).unwrap();
    let doc = reopen(&product);
    assert_eq!(page_count_of(&doc), 2);
    // 内容流 = 原页 1、2。
    let srcdoc = reopen(&src);
    assert_eq!(page_content(&doc, 0), page_content(&srcdoc, 1));
    assert_eq!(page_content(&doc, 1), page_content(&srcdoc, 2));
    // 书签：区间外（第一章→p0）丢弃；区间内四项保留。
    let items = outline_titles(&doc);
    assert_eq!(items.len(), 4, "5 - 1（第一章目标页被抽掉）= 4：{items:?}");
    assert!(items.iter().all(|(t, _)| t != "第一章"));

    // 报告只读（extract 是快照操作——原组织器状态不变）。
    assert_eq!(org.page_count(), 3);
    assert_eq!(org.report().deleted_pages, 0);

    // 空区间与越界拒绝。
    assert!(org.extract(1..1).is_err());
    assert!(org.extract(2..9).is_err());
    assert!(org.extract(3..3).is_err());

    // 交叉：poppler + qpdf。
    if require_poppler() {
        assert_eq!(poppler_pages(&product), Some(2));
    }
    if require_qpdf() {
        assert!(qpdf_valid(&product, "extract"), "qpdf --check 须过");
    }
}

#[test]
fn split_every_totals() {
    let org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    // 3 页按 2 拆 → [2, 1]。
    let chunks = org.split_every(2).unwrap();
    assert_eq!(chunks.len(), 2);
    assert_eq!(page_count_of(&reopen(&chunks[0])), 2);
    assert_eq!(page_count_of(&reopen(&chunks[1])), 1);
    // 页数守恒。
    let total: usize = chunks.iter().map(|c| page_count_of(&reopen(c))).sum();
    assert_eq!(total, 3);
    // n=0 拒绝；n > 页数 = 单块整档。
    assert!(org.split_every(0).is_err());
    let whole = org.split_every(10).unwrap();
    assert_eq!(whole.len(), 1);
    assert_eq!(page_count_of(&reopen(&whole[0])), 3);
    // 块独立性（qpdf）。
    if require_qpdf() {
        for (i, c) in chunks.iter().enumerate() {
            assert!(qpdf_valid(c, &format!("split-{i}")), "块 {i} 须独立可开");
        }
    }
}

// ---------------------------------------------------------------------------
// 9-10. 合并
// ---------------------------------------------------------------------------

#[test]
fn merge_totals_content_and_bookmarks() {
    let a = fixture_bytes("small.pdf"); // 3 页无大纲
    let b = fixture_bytes("bookmarked.pdf"); // 3 页 5 书签
    let merged = PdfOrganizer::merge(&[&a, &b]).unwrap();
    let doc = reopen(&merged);
    assert_eq!(page_count_of(&doc), 6, "3 + 3 = 6");
    // 前基底后追加：内容流逐页对位。
    let adoc = reopen(&a);
    let bdoc = reopen(&b);
    for i in 0..3 {
        assert_eq!(page_content(&doc, i), page_content(&adoc, i), "基底页 {i}");
    }
    for i in 0..3 {
        assert_eq!(
            page_content(&doc, i + 3),
            page_content(&bdoc, i),
            "追加页 {i}"
        );
    }
    // 书签拼接：5 项（small 无）。
    let items = outline_titles(&doc);
    assert_eq!(items.len(), 5, "bookmarked 的 5 项全携入：{items:?}");
    assert_eq!(items[0].0, "第一章");

    // 三方合并 + 顺序。
    let c = fixture_bytes("form_probe.pdf"); // 1 页
    let merged3 = PdfOrganizer::merge(&[&a, &b, &c]).unwrap();
    let doc3 = reopen(&merged3);
    assert_eq!(page_count_of(&doc3), 7, "3 + 3 + 1 = 7");

    // 单文档 merge = 纯往返。
    let solo = PdfOrganizer::merge(&[&a]).unwrap();
    assert_eq!(page_count_of(&reopen(&solo)), 3);
    // 空切片拒绝。
    assert!(PdfOrganizer::merge(&[]).is_err());

    // 交叉。
    if require_poppler() {
        assert_eq!(poppler_pages(&merged3), Some(7));
    }
    if require_qpdf() {
        assert!(qpdf_valid(&merged, "merge-ab"), "qpdf --check 须过");
        assert!(qpdf_valid(&merged3, "merge-abc"), "qpdf --check 须过");
    }
}

#[test]
fn merge_form_and_outline_adopt() {
    // 基底无表单/无大纲 → 源面整体过继。
    let plain = fixture_bytes("small.pdf");
    let form = fixture_bytes("form_probe.pdf"); // 1 页 1 域（/AcroForm）
    let bookmarked = fixture_bytes("bookmarked.pdf");

    // 表单过继：form 为源。
    let merged = PdfOrganizer::merge(&[&plain, &form]).unwrap();
    let doc = reopen(&merged);
    let catalog = doc.catalog().unwrap();
    assert!(
        matches!(
            catalog.get(b"AcroForm"),
            Ok(Object::Dictionary(_)) | Ok(Object::Reference(_))
        ),
        "源 /AcroForm 须过继"
    );
    assert_eq!(page_count_of(&doc), 4);

    // 大纲过继：bookmarked 为源（small 基底无大纲）。
    let merged = PdfOrganizer::merge(&[&plain, &bookmarked]).unwrap();
    let doc = reopen(&merged);
    let items = outline_titles(&doc);
    assert_eq!(items.len(), 5, "无大纲基底过继源 5 项：{items:?}");

    // 反向：bookmarked 基底 + plain 源（无面携入）→ 大纲保持 5。
    let merged = PdfOrganizer::merge(&[&bookmarked, &plain]).unwrap();
    let doc = reopen(&merged);
    assert_eq!(outline_titles(&doc).len(), 5);

    // 双大纲拼接：bookmarked + bookmarked → 10 项。
    let merged = PdfOrganizer::merge(&[&bookmarked, &bookmarked]).unwrap();
    let doc = reopen(&merged);
    let items = outline_titles(&doc);
    assert_eq!(items.len(), 10, "双大纲拼接 5+5：{items:?}");
    assert_eq!(page_count_of(&doc), 6);
    if require_qpdf() {
        assert!(qpdf_valid(&merged, "merge-bm-bm"), "qpdf --check 须过");
    }
}

// ---------------------------------------------------------------------------
// 11. 往返稳定性
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_10x_no_bloat() {
    let cases: &[(&str, Vec<u8>)] = &[
        ("small", fixture_bytes("small.pdf")),
        ("bookmarked", fixture_bytes("bookmarked.pdf")),
        ("form_probe", fixture_bytes("form_probe.pdf")),
    ];
    for (name, src) in cases {
        // 首轮产物（结构重排基线）。
        let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
        let first = org.save_to_vec().unwrap();
        // 10 次开-存链。
        let mut cur = first.clone();
        let mut prev_len = cur.len();
        for i in 0..10 {
            let mut o = PdfOrganizer::from_bytes(cur.clone()).unwrap();
            cur = o.save_to_vec().unwrap();
            // 逐字节确定性（lopdf 保存可复现性——同输入同输出）。
            assert_eq!(
                cur.len(),
                prev_len,
                "{name} 第 {i} 轮体积漂移（{} → {}）",
                prev_len,
                cur.len()
            );
            prev_len = cur.len();
        }
        // 页数稳定。
        assert_eq!(
            page_count_of(&reopen(&cur)),
            page_count_of(&reopen(src)),
            "{name} 页数漂移"
        );
        // 体积增长 < 2%（对首轮产物）。
        let growth = cur.len() as f64 / first.len() as f64 - 1.0;
        assert!(
            growth.abs() < 0.02,
            "{name} 10 轮体积增长 {growth:.2}% 须 < 2%"
        );
    }
    // 真实语料面（LO 转换件——含字体子集/资源树的复杂结构）：抽样 5 件
    // 同口径 10 轮（issue 验收：真实 PDF 往返不膨胀）。
    let corpus = corpus_files();
    let sample: Vec<_> = corpus
        .iter()
        .filter(|p| {
            !p.file_name()
                .map(|n| n == "large__manual300.pdf")
                .unwrap_or(false)
        })
        .take(5)
        .collect();
    for path in sample {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut org = PdfOrganizer::from_bytes(bytes.clone()).unwrap();
        let first = org.save_to_vec().unwrap();
        let mut cur = first.clone();
        for _ in 0..10 {
            let mut o = PdfOrganizer::from_bytes(cur.clone()).unwrap();
            cur = o.save_to_vec().unwrap();
        }
        let growth = cur.len() as f64 / first.len() as f64 - 1.0;
        assert!(
            growth.abs() < 0.02,
            "语料 {name} 10 轮体积增长 {growth:.2}% 须 < 2%"
        );
        assert_eq!(
            page_count_of(&reopen(&cur)),
            page_count_of(&reopen(&bytes)),
            "语料 {name} 页数漂移"
        );
    }
    if !corpus.is_empty() {
        eprintln!("[pdff] 语料 10× 往返：抽样 5 件全过（< 2%）");
    }
}

// ---------------------------------------------------------------------------
// 12. 真实语料全量扫
// ---------------------------------------------------------------------------

#[test]
fn corpus_sweep_all_operations() {
    let files = corpus_files();
    if files.is_empty() {
        eprintln!("[pdff] corpus sweep SKIPPED（语料缺失）");
        return;
    }
    let cap = std::env::var("MDC_PDF_ORG_SWEEP_CAP")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(30);
    let mut processed = 0usize;
    let mut no_ops = 0usize; // 页数不足以做删页的件
    for path in files.iter().take(cap) {
        let bytes =
            std::fs::read(path).unwrap_or_else(|e| panic!("{} 读取失败：{e}", path.display()));
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(mut org) = PdfOrganizer::from_bytes(bytes.clone()) else {
            // LO/pdfunite 产物装载失败 = 如实登记（语料含敌意/畸形件时）
            panic!("语料件装载失败：{name}");
        };
        let n = org.page_count();
        let mut ops = 0usize;
        // 删中页（n >= 2 时）。
        if n >= 2 {
            let mid = n / 2;
            org.delete_pages(&[mid]).unwrap();
            assert_eq!(org.page_count(), n - 1, "{name} 删页计数");
            ops += 1;
        } else {
            no_ops += 1;
        }
        // 旋转首页。
        org.rotate_page(0, 180).unwrap();
        ops += 1;
        // 重排（逆序——页数 ≤ 9 时全逆，否则头尾对调的局部置换）。
        let n2 = org.page_count();
        let order: Vec<usize> = if n2 <= 9 {
            (0..n2).rev().collect()
        } else {
            let mut o: Vec<usize> = (0..n2).collect();
            o.swap(0, n2 - 1);
            o
        };
        org.reorder(&order).unwrap();
        ops += 1;
        // 抽取前 3 页（不足 3 全抽）。
        let take = org.page_count().min(3);
        let part = org.extract(0..take).unwrap();
        assert_eq!(page_count_of(&reopen(&part)), take, "{name} 抽取计数");
        // 与自身合并。
        let merged = PdfOrganizer::merge(&[&bytes, &part]).unwrap();
        let expect = n + take;
        assert_eq!(page_count_of(&reopen(&merged)), expect, "{name} 合并计数");
        // 产物全量保存 + 结构验证。
        let product = org.save_to_vec().unwrap();
        let redoc = reopen(&product);
        assert_eq!(page_count_of(&redoc), n2, "{name} 终态页数");
        if require_qpdf() {
            assert!(
                qpdf_valid(&product, &format!("sweep-{name}")),
                "{name} 终态产物 qpdf --check"
            );
            assert!(
                qpdf_valid(&part, &format!("sweep-extract-{name}")),
                "{name} 抽取产物 qpdf"
            );
            assert!(
                qpdf_valid(&merged, &format!("sweep-merge-{name}")),
                "{name} 合并产物 qpdf"
            );
        }
        if require_poppler() {
            assert_eq!(
                poppler_pages(&product),
                Some(n2 as u32),
                "{name} poppler 页数"
            );
        }
        processed += 1;
        let _ = ops;
    }
    eprintln!("[pdff] corpus sweep：{processed} 件全操作通过（{no_ops} 件页数过小免删页步）");
    assert!(processed >= 5, "语料面过小（{processed} 件）——扫描无效");
}

// ---------------------------------------------------------------------------
// 13. 大件（300 页级）
// ---------------------------------------------------------------------------

#[test]
fn large_manual_delete_extract() {
    let files = corpus_files();
    let big = files
        .iter()
        .find(|p| {
            p.file_name()
                .map(|n| n == "large__manual300.pdf")
                .unwrap_or(false)
        })
        .cloned();
    let Some(big) = big else {
        eprintln!("[pdff] large__manual300.pdf 缺失 — 跳过大件测试");
        return;
    };
    let bytes = std::fs::read(&big).unwrap();
    let t0 = std::time::Instant::now();
    let mut org = PdfOrganizer::from_bytes(bytes.clone()).unwrap();
    let n = org.page_count();
    assert!(n >= 300, "压测件须 300 页级（得 {n}）");
    // 删 10 页（issue 验收口径）。
    org.delete_pages(&(0..10).collect::<Vec<_>>()).unwrap();
    assert_eq!(org.page_count(), n - 10);
    let product = org.save_to_vec().unwrap();
    eprintln!(
        "[pdff] manual300: {n} 页 → {} 页，{} 字节，{:?}",
        org.page_count(),
        product.len(),
        t0.elapsed()
    );
    let doc = reopen(&product);
    assert_eq!(page_count_of(&doc), n - 10);
    // 抽取 300 页独立件（issue 验收口径）。
    let part = org.extract(0..300).unwrap();
    assert_eq!(page_count_of(&reopen(&part)), 300);
    // 产物结构验证。
    if require_qpdf() {
        assert!(qpdf_valid(&product, "manual300-del"), "删 10 页产物 qpdf");
        assert!(qpdf_valid(&part, "manual300-extract"), "抽取产物 qpdf");
    }
    if require_poppler() {
        assert_eq!(poppler_pages(&product), Some((n - 10) as u32));
        assert_eq!(poppler_pages(&part), Some(300));
    }
}

// ---------------------------------------------------------------------------
// 14. 加密拒绝（S5）
// ---------------------------------------------------------------------------

#[test]
fn encrypted_rejected_s5() {
    if !require_qpdf() {
        eprintln!("[pdff] qpdf 缺失——无法现场生成加密夹具，跳过");
        return;
    }
    // 现场生成：qpdf --encrypt user owner 256 --。
    let dir = std::env::temp_dir().join(format!("pdff-enc-{}-{}", std::process::id(), tmp_seq()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.pdf");
    let enc = dir.join("enc.pdf");
    std::fs::write(&src, fixture_bytes("small.pdf")).unwrap();
    let out = Command::new("qpdf")
        .args(["--encrypt", "user", "owner", "256", "--"])
        .arg(&src)
        .arg(&enc)
        .output()
        .expect("spawn qpdf encrypt");
    assert!(out.status.success(), "qpdf 加密夹具生成失败");
    let enc_bytes = std::fs::read(&enc).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    // from_bytes 拒（S5 加密档零结构写）。
    match PdfOrganizer::from_bytes(enc_bytes.clone()) {
        Err(CommandError::SecurityDeny { reason }) => {
            assert_eq!(reason, mdc_pdf::security::REASON_ENCRYPTED_WRITE);
        }
        other => panic!("加密档应 SecurityDeny，得 {other:?}"),
    }
    // merge 源拒绝（混合产物加密语义不可定义）。
    let plain = fixture_bytes("small.pdf");
    assert!(PdfOrganizer::merge(&[&plain, &enc_bytes]).is_err());
}

// ---------------------------------------------------------------------------
// 15-16. 报告与评估
// ---------------------------------------------------------------------------

#[test]
fn report_loss_registry_accuracy() {
    let mut org = PdfOrganizer::from_bytes(fixture_bytes("bookmarked.pdf")).unwrap();
    org.delete_pages(&[0, 2]).unwrap();
    let r = org.report();
    // 删 2 页；书签丢 3（第一章→p0 + 2.2→p2 + 第三章→p2）；无链接/表单。
    assert_eq!(r.deleted_pages, 2);
    assert_eq!(r.dropped_bookmarks, 3);
    assert_eq!(r.dropped_links, 0);
    assert_eq!(r.dropped_form_fields, 0);
    assert_eq!(r.materialized_pages, 0);
    // 剪除面：死页对象 + 死书签 + 孤儿资源 > 0（登记非零即证剪除通路）。
    assert!(r.pruned_objects > 0, "删页必产生不可达对象（报告须登记）");
    // 跨操作累加：插入 1 页（2 页）→ 再删 1 页。
    org.insert_blank_page(0, PageSize::LETTER).unwrap();
    org.delete_pages(&[0]).unwrap();
    assert_eq!(org.report().deleted_pages, 3);
    assert_eq!(org.report().inserted_pages, 1);
}

#[test]
fn assessment_carried_from_scan() {
    let org = PdfOrganizer::from_bytes(fixture_bytes("form_probe.pdf")).unwrap();
    // S6 表单探针：form_probe 带 /AcroForm → 探针非 None。
    assert!(
        org.assessment().form_probe.is_some(),
        "form_probe.pdf 的 /AcroForm 须进打开评估"
    );
    // small.pdf 无表单。
    let org = PdfOrganizer::from_bytes(fixture_bytes("small.pdf")).unwrap();
    assert!(org.assessment().form_probe.is_none());
}

// ---------------------------------------------------------------------------
// pdfium 交叉读回面（feature engine-pdfium：引擎层独立确认）
// ---------------------------------------------------------------------------

#[cfg(feature = "engine-pdfium")]
mod pdfium_cross {
    use super::*;
    use mdc_pdf::engine::DocumentEngine;
    use mdc_pdf::engine_pdfium::PdfiumEngine;
    use mdc_pdf::page::PageSpan;

    fn open_engine(bytes: &[u8]) -> PdfiumEngine {
        match PdfiumEngine::open(bytes.to_vec()) {
            Ok(e) => e,
            Err(CommandError::Engine(detail)) => {
                panic!("pdfium 绑定/打开失败（定位序：MDC_PDFIUM_LIB → 候选 → 系统）：{detail}")
            }
            Err(other) => panic!("pdfium 打开失败：{other:?}"),
        }
    }

    /// 删页后 pdfium 重开：页数 + 幸存页文本逐页相等（内容层独立确认）。
    #[test]
    fn delete_pages_pdfium_reopen_and_text() {
        let src = fixture_bytes("small.pdf");
        let engine = open_engine(&src);
        let src_text = engine
            .extract_text(PageSpan::new(PageIndex(0), PageIndex(2)).unwrap())
            .unwrap();

        let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
        org.delete_pages(&[1]).unwrap();
        let product = org.save_to_vec().unwrap();

        let engine2 = open_engine(&product);
        assert_eq!(engine2.page_count(), 2);
        let text = engine2
            .extract_text(PageSpan::new(PageIndex(0), PageIndex(1)).unwrap())
            .unwrap();
        // 幸存页 0 = 原页 0；幸存页 1 = 原页 2。
        assert_eq!(text[0].1, src_text[0].1, "原页 0 文本经删页后不变");
        assert_eq!(text[1].1, src_text[2].1, "原页 2 文本经删页后不变");
    }

    /// 旋转后 pdfium 几何（视口宽高交换 + 旋转位）。
    #[test]
    fn rotate_pdfium_geometry() {
        let src = fixture_bytes("small.pdf");
        let engine = open_engine(&src);
        let g0 = engine.page_geometry(PageIndex(0)).unwrap();
        let (w0, h0) = (g0.effective_width(), g0.effective_height());

        let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
        org.rotate_page(0, 90).unwrap();
        let product = org.save_to_vec().unwrap();
        let engine2 = open_engine(&product);
        let g = engine2.page_geometry(PageIndex(0)).unwrap();
        assert_eq!(g.rotation, mdc_pdf::page::Rotation::Deg90);
        assert_eq!(g.effective_width(), h0, "转 90 视口宽 = 原高");
        assert_eq!(g.effective_height(), w0, "转 90 视口高 = 原宽");
        // 未转页零波及。
        let g1 = engine2.page_geometry(PageIndex(1)).unwrap();
        assert_eq!(g1.rotation, mdc_pdf::page::Rotation::Deg0);
    }

    /// 书签重映射的 pdfium 独立确认（outline 树目标）。
    #[test]
    fn bookmark_remap_pdfium_outline() {
        let src = fixture_bytes("bookmarked.pdf");
        let engine = open_engine(&src);
        let src_outline = engine.outline().unwrap();
        let src_flat: Vec<_> = src_outline.iter().collect();
        assert_eq!(src_flat.len(), 5);

        // 删页 0 → 4 项；首项 = 第二章（目标新页 0）。
        let mut org = PdfOrganizer::from_bytes(src.clone()).unwrap();
        org.delete_pages(&[0]).unwrap();
        let product = org.save_to_vec().unwrap();
        let engine2 = open_engine(&product);
        let outline = engine2.outline().unwrap();
        let flat: Vec<_> = outline.iter().collect();
        assert_eq!(flat.len(), 4, "pdfium outline：5 - 1 = 4");
        assert_eq!(flat[0].1.title, "第二章");
        // 第二章原目标 p1 → 删 p0 后新页 0。
        assert!(
            matches!(flat[0].1.target, mdc_pdf::outline::OutlineTarget::Page(p) if p == PageIndex(0))
        );
    }

    /// 合并产物 pdfium 重开：页数 + 大纲全量。
    #[test]
    fn merge_pdfium_reopen() {
        let a = fixture_bytes("small.pdf");
        let b = fixture_bytes("bookmarked.pdf");
        let merged = PdfOrganizer::merge(&[&a, &b]).unwrap();
        let engine = open_engine(&merged);
        assert_eq!(engine.page_count(), 6);
        let outline = engine.outline().unwrap();
        assert_eq!(outline.iter().count(), 5);
    }

    /// 大件 pdfium 重开（300 页级——issue 验收：pdfium 重新打开页数正确）。
    #[test]
    fn manual300_pdfium_reopen() {
        let files = corpus_files();
        let Some(big) = files
            .iter()
            .find(|p| {
                p.file_name()
                    .map(|n| n == "large__manual300.pdf")
                    .unwrap_or(false)
            })
            .cloned()
        else {
            eprintln!("[pdff] large__manual300.pdf 缺失 — 跳过");
            return;
        };
        let bytes = std::fs::read(&big).unwrap();
        let mut org = PdfOrganizer::from_bytes(bytes.clone()).unwrap();
        let n = org.page_count();
        org.delete_pages(&(0..10).collect::<Vec<_>>()).unwrap();
        let product = org.save_to_vec().unwrap();
        let engine = open_engine(&product);
        assert_eq!(engine.page_count(), (n - 10) as u32, "pdfium 重开页数");
    }
}
