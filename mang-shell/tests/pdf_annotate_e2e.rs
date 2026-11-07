//! #302 阶段 2·④：PDF 标注写会话端到端（无 GUI 全量真跑——
//! `PdfAnnotateController` 是纯 Rust 面，GPUI 交互层另见 pdf_pages.rs
//! 的编译面验证 + 真机登记）。
//!
//! 锁清单（每锁一条 `#[test]`，断言消息讲清**为什么**）：
//! 1. `highlight_flush_is_incremental`——加高亮 → flush = 增量修订：
//!    **产物以原字节为逐位前缀**（issue 验收 2 的机制面——全量重写
//!    在此必红）+ `bytes_appended > 0` + rev 0→1；
//! 2. `idle_flush_appends_zero_keeps_rev`——零脏幂等 flush：零追加
//!    （不产生空修订）+ rev 不动（#305 命中率面）；
//! 3. `save_to_disk_roundtrips_annotation`——落盘 → 独立读回
//!    （`read_annotations` 第二意见）：高亮在档 + QuadPoints 逐点
//!    相等（用户空间坐标经写面往返无损——issue 验收 1 的落盘形态）；
//! 4. `signed_document_signature_stays_valid`——**签名保持主锁**
//!    （issue 验收 2 的验收面）：已签名档加高亮 + flush → 前缀逐位
//!    保持 + `digest_matches == Some(true)`（#308 验签第②层交叉
//!    验证——「填批注不毁签名」的密码学级证据链）；
//! 5. `rotated_page_geometry_and_landing`——旋转 + 偏移 CropBox 页
//!    （手写夹具）：控制器 lopdf 几何解析（双盒 + /Rotate）+ 屏幕拖框
//!    → QuadPoints 落点（#304 变换的桌面接线面——issue 验收 4）；
//! 6. `list_and_page_bounds`——命令面观测（AnnotateList）+ 越界 typed
//!    错误（页号防御）；
//! 7. `underline_flush_roundtrips`——下划线（验收 1 第二手势）：
//!    增量前缀 + 独立读回 `/Underline` kind + QuadPoints 逐点相等
//!    + 落盘子型原子证据（lopdf 直读 /Subtype）；
//! 8. `note_flush_roundtrips`——便签（验收 1 第三手势）：增量前缀
//!    + 独立读回 `/Text` kind + 锚定 Rect 四边相等 + lopdf 直读
//!    /Subtype /Text；
//! 9. `three_kinds_coexist_after_save`——**验收 1 主锁**：高亮 +
//!    下划线 + 便签同档混批 → 一次 save 落盘 → 三 kind 全量回读
//!    （「桌面能加批注（高亮/下划线/便签）并保存」的端到端形态）；
//! 10. `rotated_page_note_landing`——旋转页便签落点（Rect 面新变换
//!    路径——与 QuadPoints 面互补的 #304 接线证据）。
//!
//! 夹具：`tests/fixtures/smoke.pdf`（既有）+ `../crates/pdf/fixtures/
//! signed_sample.pdf`（跨 crate 仓内引用——#227/#303 的签名验证面同源）
//! + 手写旋转夹具（④号测试——零依赖字节确定）。

use std::path::{Path, PathBuf};

use mang_shell::pdf_annotate::{AnnotateError, PdfAnnotateController, UNDERLINE_COLOR};
use mdc_pdf::annotate::read_annotations;
use mdc_pdf::page::Rotation;
use mdc_pdf::security::Limits;
use mdc_pdf::signature::read_signatures;
use mdc_pdf::writer_lopdf::lopdf;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

fn fixture_bytes(name: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("夹具 {name} 不可读：{e}"))
}

/// mdc-pdf 仓内签名夹具（跨 crate 同仓引用——签名验证面与 #227 同源）。
fn signed_fixture() -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/pdf/fixtures/signed_sample.pdf");
    std::fs::read(&p).unwrap_or_else(|e| panic!("签名夹具不可读：{e}"))
}

/// 手写旋转 + 偏移 CropBox 夹具（字节确定——xref 偏移精确计算）：
/// 1 页，MediaBox [0,0,612,792] / CropBox [50,40,562,752] / /Rotate 90。
fn rotated_crop_pdf() -> Vec<u8> {
    let content: &[u8] = b"";
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /CropBox [50 40 562 752] /Rotate 90 /Resources << >> /Contents 4 0 R >>"
            .into(),
        format!("<< /Length {} >>\nstream\n", content.len()),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = [0usize; 4];
    for (i, body) in objects.iter().enumerate() {
        offsets[i] = out.len();
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body.as_bytes());
        if i == 3 {
            out.extend_from_slice(content);
            out.extend_from_slice(b"\nendstream");
        }
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_pos = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(xref_pos.to_string().as_bytes());
    out.extend_from_slice(b"\n%%EOF\n");
    out
}

/// 临时文件守卫（Drop 清理——panic 也清）。
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_path(tag: &str) -> TempFile {
    TempFile(std::env::temp_dir().join(format!(
        "mang-302-annotate-{tag}-{}.pdf",
        std::process::id()
    )))
}

// ---------------------------------------------------------------------------
// 1-2：增量语义
// ---------------------------------------------------------------------------

#[test]
fn highlight_flush_is_incremental() {
    let original = fixture_bytes("smoke.pdf");
    let mut controller = PdfAnnotateController::open(original.clone()).unwrap();
    assert_eq!(controller.annotation_rev(), 0, "打开即 0");
    assert_eq!(controller.page_count(), 2, "smoke.pdf 两页");

    // 屏幕拖框（1024 宽视图 @ 96 DPI 量级的矩形——页 0 上部区域）。
    let id = controller
        .add_highlight_from_screen(0, 120.0, 80.0, 520.0, 160.0, 96.0, "测试者")
        .expect("加高亮应成功");
    assert!(id.0 > 0, "id 单调非零");

    let (product, stats) = controller.flush().expect("flush 应成功");
    assert!(
        stats.bytes_appended > 0,
        "有脏 flush 必追加（得 {stats:?}）"
    );
    assert!(
        product.starts_with(&original),
        "增量产物必须以原字节为逐位前缀（issue 验收 2 机制面——全量重写在此必红）"
    );
    assert!(product.len() > original.len(), "追加了非空修订段");
    assert_eq!(controller.annotation_rev(), 1, "产物变了 → rev +1");
}

#[test]
fn idle_flush_appends_zero_keeps_rev() {
    let original = fixture_bytes("smoke.pdf");
    let mut controller = PdfAnnotateController::open(original.clone()).unwrap();
    controller
        .add_highlight_from_screen(1, 100.0, 100.0, 400.0, 200.0, 96.0, "测试者")
        .unwrap();
    let _ = controller.flush().unwrap();

    // 零脏幂等 flush：零追加 + rev 不动（位图没变——#305 命中率面）。
    let (product2, stats2) = controller.flush().expect("幂等 flush");
    assert_eq!(stats2.bytes_appended, 0, "零脏 flush 零追加");
    assert_eq!(controller.annotation_rev(), 1, "无内容变更不推进 rev");
    // 产物与上次一致（前缀 + 既有修订——未新增）。
    assert_eq!(
        product2.len() as u64,
        stats2.prefix_bytes,
        "产物长度 = 前缀 + 0"
    );
}

// ---------------------------------------------------------------------------
// 3：落盘往返
// ---------------------------------------------------------------------------

#[test]
fn save_to_disk_roundtrips_annotation() {
    let original = fixture_bytes("smoke.pdf");
    let mut controller = PdfAnnotateController::open(original.clone()).unwrap();
    controller
        .add_highlight_from_screen(0, 120.0, 80.0, 520.0, 160.0, 96.0, "测试者")
        .unwrap();
    let (expected_product, _) = controller.flush().unwrap();

    // 落盘（save = flush + 前缀保持覆写）。已 flush 过 → save 的 flush
    // 是零脏幂等（零追加——同一产物，不产生空修订）。
    let path = temp_path("roundtrip");
    let stats = controller.save(&path.0).expect("落盘应成功");
    assert_eq!(
        stats.bytes_appended, 0,
        "已 flush 后 save = 零脏幂等（不追加空修订）"
    );
    let on_disk = std::fs::read(&path.0).unwrap();
    assert_eq!(
        on_disk, expected_product,
        "盘上字节 = flush 产物（save 语义）"
    );
    assert!(
        on_disk.starts_with(&original),
        "盘上字节保持原前缀（增量非重写）"
    );

    // 独立第二意见：mdc-pdf 读面回读（与写面无共享实现路径）。
    let (annots, report) = read_annotations(on_disk, Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "独立读回应见 1 条：{report:?}");
    assert_eq!(annots.len(), 1);
    let annot = &annots[0];
    assert_eq!(annot.author, "测试者", "作者往返");
    assert_eq!(annot.page.0, 0, "页号往返");

    // QuadPoints 落点 = 控制器变换算出的用户空间四角（写面往返无损）。
    let transform = controller.page_transform(0).unwrap();
    let expected_quad = transform.screen_rect_to_quad(120.0, 80.0, 520.0, 160.0, 96.0);
    match &annot.kind {
        mdc_pdf::annotation::AnnotationKind::Highlight { quads, .. } => {
            assert_eq!(quads.len(), 1, "单拖框 = 单 quad");
            for (got, expect) in quads.iter().zip(std::iter::once(&expected_quad)) {
                for (g, e) in got.points.iter().zip(expect.points.iter()) {
                    assert!(
                        (g.x - e.x).abs() < 0.01 && (g.y - e.y).abs() < 0.01,
                        "四角往返（得 {g:?} 期望 {e:?}）"
                    );
                }
            }
        }
        other => panic!("应为高亮，得 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4：签名保持（#303/#308 链——issue 验收 2 的验收面）
// ---------------------------------------------------------------------------

#[test]
fn signed_document_signature_stays_valid() {
    let original = signed_fixture();
    // 前置钉定：夹具签名基线（未改动态 digest == Some(true)）。
    let before = read_signatures(&original).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].digest_matches, Some(true), "夹具基线应为真");

    let mut controller = PdfAnnotateController::open(original.clone()).unwrap();
    controller
        .add_highlight_from_screen(0, 150.0, 120.0, 450.0, 200.0, 96.0, "批注人")
        .expect("已签名档加高亮应放行（增量路径不碰签名覆盖段）");
    let (product, _) = controller.flush().unwrap();

    // ① 前缀逐位保持（签名字节域未被触碰的机制面）。
    assert!(
        product.starts_with(&original),
        "批注产物必须以原字节为前缀（否则签名被毁——#227 语义）"
    );
    // ② 摘要一致性：签名仍 Some(true)（#308 验签第②层——「批注不毁
    //    签名」的密码学级证据，非仅结构完整）。
    let after = read_signatures(&product).unwrap();
    assert_eq!(after.len(), 1, "签名域仍在");
    assert_eq!(
        after[0].digest_matches,
        Some(true),
        "签名摘要一致性保持（issue 验收 2——加批注后签名有效）"
    );
    assert!(after[0].structurally_valid, "结构面同时完整");
}

// ---------------------------------------------------------------------------
// 5：旋转 + 偏移 CropBox（#304 接线面——issue 验收 4）
// ---------------------------------------------------------------------------

#[test]
fn rotated_page_geometry_and_landing() {
    let bytes = rotated_crop_pdf();
    let mut controller = PdfAnnotateController::open(bytes.clone()).unwrap();

    // lopdf 几何解析：双盒 + 旋转全量（与引擎 pdfium 面同构）。
    assert_eq!(controller.page_count(), 1);
    let transform = controller.page_transform(0).unwrap();
    let media = transform.media();
    assert!((media.x1 - 612.0).abs() < 1e-3 && (media.y1 - 792.0).abs() < 1e-3);
    let crop = transform.crop();
    assert!(
        (crop.x0 - 50.0).abs() < 1e-3
            && (crop.y0 - 40.0).abs() < 1e-3
            && (crop.x1 - 562.0).abs() < 1e-3
            && (crop.y1 - 752.0).abs() < 1e-3,
        "偏移 CropBox 解析（得 {crop:?}）"
    );
    assert_eq!(transform.rotate(), Rotation::Deg90, "/Rotate 90");
    // 显示域：90° 宽高互换（712 × 512——FPDF_GetPageWidthF 同口径）。
    let display = transform.display_size();
    assert!((display.width - 712.0).abs() < 1e-3 && (display.height - 512.0).abs() < 1e-3);

    // 旋转页拖框 → 落点（#304：四角经 screen_rect_to_quad 用户空间化）。
    controller
        .add_highlight_from_screen(0, 100.0, 60.0, 300.0, 180.0, 96.0, "旋转页测试")
        .unwrap();
    let (product, _) = controller.flush().unwrap();
    assert!(product.starts_with(&bytes), "旋转页产物同为增量");

    // 独立读回：QuadPoints = 变换算出的用户空间四角（旋转页落点正确性
    // 的落盘证据——issue 验收 4）。
    let (annots, report) = read_annotations(product, Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "{report:?}");
    let expected = transform.screen_rect_to_quad(100.0, 60.0, 300.0, 180.0, 96.0);
    match &annots[0].kind {
        mdc_pdf::annotation::AnnotationKind::Highlight { quads, .. } => {
            for (g, e) in quads[0].points.iter().zip(expected.points.iter()) {
                assert!(
                    (g.x - e.x).abs() < 0.01 && (g.y - e.y).abs() < 0.01,
                    "旋转页四角落点（得 {g:?} 期望 {e:?}）"
                );
            }
        }
        other => panic!("应为高亮，得 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6：命令面观测 + 边界
// ---------------------------------------------------------------------------

#[test]
fn list_and_page_bounds() {
    let mut controller = PdfAnnotateController::open(fixture_bytes("smoke.pdf")).unwrap();
    assert!(controller.list(None).unwrap().is_empty(), "初始无批注");
    controller
        .add_highlight_from_screen(0, 10.0, 10.0, 200.0, 60.0, 96.0, "a")
        .unwrap();
    controller
        .add_highlight_from_screen(1, 10.0, 10.0, 200.0, 60.0, 96.0, "b")
        .unwrap();
    assert_eq!(controller.list(None).unwrap().len(), 2, "全量列表");
    assert_eq!(controller.list(Some(1)).unwrap().len(), 1, "页过滤列表");

    // 越界页号 typed 错误（防御面——不 panic）。
    let err = controller
        .add_highlight_from_screen(2, 0.0, 0.0, 10.0, 10.0, 96.0, "x")
        .unwrap_err();
    assert!(
        matches!(err, AnnotateError::PageOutOfRange { page: 2, count: 2 }),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------------
// 7-10：验收 1 三手势收口（tranche ②——下划线/便签/混合/旋转落点）
// ---------------------------------------------------------------------------

/// 落盘产物的批注子型原子证据（lopdf 直读——与引擎读面互为独立意见）。
fn disk_subtypes(bytes: &[u8]) -> Vec<String> {
    let doc = lopdf::Document::load_mem(bytes).expect("lopdf 装载");
    let mut subs = Vec::new();
    for (_no, page_id) in doc.get_pages() {
        let dict = doc.get_dictionary(page_id).expect("页字典");
        if let Ok(annots) = dict.get(b"Annots").and_then(|o| o.as_array()) {
            for obj in annots {
                if let Ok(id) = obj.as_reference() {
                    if let Ok(adict) = doc.get_dictionary(id) {
                        if let Ok(sub) = adict.get(b"Subtype").and_then(|o| o.as_name()) {
                            subs.push(String::from_utf8_lossy(sub).into_owned());
                        }
                    }
                }
            }
        }
    }
    subs
}

#[test]
fn underline_flush_roundtrips() {
    let original = fixture_bytes("smoke.pdf");
    let mut controller = PdfAnnotateController::open(original.clone()).unwrap();

    // 屏幕拖框（与高亮同坐标系——同页同区域便于对照）。
    controller
        .add_underline_from_screen(0, 120.0, 80.0, 520.0, 160.0, 96.0, "下划线者")
        .expect("加下划线应成功");
    let (product, stats) = controller.flush().unwrap();
    assert!(stats.bytes_appended > 0, "有脏 flush 必追加（{stats:?}）");
    assert!(
        product.starts_with(&original),
        "下划线同为增量前缀（验收 2 机制面对三手势一致）"
    );

    // 落盘 + 独立第二意见：引擎读面回读 Underline kind + QuadPoints。
    let path = temp_path("underline");
    controller.save(&path.0).unwrap();
    let on_disk = std::fs::read(&path.0).unwrap();
    let (annots, report) = read_annotations(on_disk.clone(), Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "{report:?}");
    let transform = controller.page_transform(0).unwrap();
    let expected_quad = transform.screen_rect_to_quad(120.0, 80.0, 520.0, 160.0, 96.0);
    match &annots[0].kind {
        mdc_pdf::annotation::AnnotationKind::Underline { quads, color } => {
            assert_eq!(quads.len(), 1, "单拖框 = 单 quad");
            for (g, e) in quads[0].points.iter().zip(expected_quad.points.iter()) {
                assert!(
                    (g.x - e.x).abs() < 0.01 && (g.y - e.y).abs() < 0.01,
                    "下划线四角往返（得 {g:?} 期望 {e:?}）"
                );
            }
            assert_eq!(color, &UNDERLINE_COLOR, "缺省色往返（写面单源）");
        }
        other => panic!("应为下划线，得 {other:?}"),
    }

    // 子型原子证据：盘上 /Subtype /Underline（阅读器互通面——零私有扩展）。
    let subs = disk_subtypes(&on_disk);
    assert!(
        subs.iter().any(|s| s == "Underline"),
        "落盘子型应为 /Underline（得 {subs:?}）"
    );
}

#[test]
fn note_flush_roundtrips() {
    let original = fixture_bytes("smoke.pdf");
    let mut controller = PdfAnnotateController::open(original.clone()).unwrap();

    controller
        .add_note_from_screen(0, 200.0, 300.0, 420.0, 420.0, 96.0, "便签者")
        .expect("加便签应成功");
    let (product, stats) = controller.flush().unwrap();
    assert!(stats.bytes_appended > 0, "{stats:?}");
    assert!(
        product.starts_with(&original),
        "便签同为增量前缀（同一 flush 事务面）"
    );

    let path = temp_path("note");
    controller.save(&path.0).unwrap();
    let on_disk = std::fs::read(&path.0).unwrap();
    let (annots, report) = read_annotations(on_disk.clone(), Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "{report:?}");
    assert_eq!(annots[0].author, "便签者", "作者往返");
    assert_eq!(annots[0].page.0, 0, "页号往返");

    // 锚定 Rect = 控制器同一变换算出的用户空间包络（两对角 min/max）。
    let transform = controller.page_transform(0).unwrap();
    let tl = transform.screen_to_user(200.0, 300.0, 96.0);
    let br = transform.screen_to_user(420.0, 420.0, 96.0);
    let (exp_x0, exp_x1) = (tl.x.min(br.x), tl.x.max(br.x));
    let (exp_y0, exp_y1) = (tl.y.min(br.y), tl.y.max(br.y));
    match &annots[0].kind {
        mdc_pdf::annotation::AnnotationKind::Note { rect } => {
            for (got, expect) in [
                (rect.x0(), exp_x0),
                (rect.y0(), exp_y0),
                (rect.x1(), exp_x1),
                (rect.y1(), exp_y1),
            ] {
                assert!(
                    (got - expect).abs() < 0.01,
                    "便签 Rect 边往返（得 {got} 期望 {expect}）"
                );
            }
            assert!(
                rect.width() > 0.0 && rect.height() > 0.0,
                "便签锚框非退化（拖框区域即锚点）"
            );
        }
        other => panic!("应为便签，得 {other:?}"),
    }

    // 子型原子证据：盘上 /Subtype /Text（PDF 便签标准子型）。
    let subs = disk_subtypes(&on_disk);
    assert!(
        subs.iter().any(|s| s == "Text"),
        "落盘子型应为 /Text（得 {subs:?}）"
    );
}

#[test]
fn three_kinds_coexist_after_save() {
    // issue 验收 1 主锁：「桌面能对 PDF 加批注（高亮/下划线/便签）并
    // 保存」——三手势同档混批，一次落盘，三 kind 全量回读。
    let original = fixture_bytes("smoke.pdf");
    let mut controller = PdfAnnotateController::open(original.clone()).unwrap();

    controller
        .add_highlight_from_screen(0, 120.0, 80.0, 520.0, 160.0, 96.0, "混批者")
        .unwrap();
    controller
        .add_underline_from_screen(0, 120.0, 200.0, 520.0, 280.0, 96.0, "混批者")
        .unwrap();
    controller
        .add_note_from_screen(1, 200.0, 300.0, 420.0, 420.0, 96.0, "混批者")
        .unwrap();
    let path = temp_path("mixed");
    let stats = controller.save(&path.0).expect("三手势落盘");
    assert!(stats.bytes_appended > 0, "{stats:?}");

    let on_disk = std::fs::read(&path.0).unwrap();
    assert!(
        on_disk.starts_with(&original),
        "三手势产物仍为增量前缀（验收 2 对混合批注一致）"
    );

    // 独立第二意见：三 kind 各一（页 0 两条 QuadPoints 系 + 页 1 一条）。
    let (annots, report) = read_annotations(on_disk.clone(), Limits::default()).unwrap();
    assert_eq!(report.parsed, 3, "{report:?}");
    assert_eq!(report.skipped_malformed, 0, "无畸形跳过");
    let mut kinds = std::collections::BTreeSet::new();
    for a in &annots {
        kinds.insert(format!("{:?}", a.kind.tag()));
    }
    assert_eq!(
        kinds,
        ["Highlight", "Note", "Underline"]
            .iter()
            .map(|s| s.to_string())
            .collect::<std::collections::BTreeSet<_>>(),
        "三 kind 齐备（验收 1）"
    );
    // 页分布：页 0 高亮+下划线、页 1 便签。
    assert_eq!(controller.list(Some(0)).unwrap().len(), 2, "页 0 两条");
    assert_eq!(controller.list(Some(1)).unwrap().len(), 1, "页 1 一条");

    // 子型原子证据：三个标准子型同档共存。
    let subs = disk_subtypes(&on_disk);
    assert!(subs.contains(&"Highlight".to_string()), "{subs:?}");
    assert!(subs.contains(&"Underline".to_string()), "{subs:?}");
    assert!(subs.contains(&"Text".to_string()), "{subs:?}");
}

#[test]
fn rotated_page_note_landing() {
    // 旋转页便签落点（#304 变换的 Rect 面——与 QuadPoints 面
    // `rotated_page_geometry_and_landing` 互补）。
    let bytes = rotated_crop_pdf();
    let mut controller = PdfAnnotateController::open(bytes.clone()).unwrap();
    assert_eq!(controller.page_count(), 1);

    controller
        .add_note_from_screen(0, 100.0, 60.0, 300.0, 180.0, 96.0, "旋转便签")
        .expect("旋转页加便签应成功");
    let (product, _) = controller.flush().unwrap();
    assert!(product.starts_with(&bytes), "旋转页便签同为增量");

    // 独立读回：Rect = 同变换两对角包络（旋转页上锚框仍轴对齐）。
    let (annots, report) = read_annotations(product, Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "{report:?}");
    let transform = controller.page_transform(0).unwrap();
    let tl = transform.screen_to_user(100.0, 60.0, 96.0);
    let br = transform.screen_to_user(300.0, 180.0, 96.0);
    let (exp_x0, exp_x1) = (tl.x.min(br.x), tl.x.max(br.x));
    let (exp_y0, exp_y1) = (tl.y.min(br.y), tl.y.max(br.y));
    match &annots[0].kind {
        mdc_pdf::annotation::AnnotationKind::Note { rect } => {
            for (got, expect) in [
                (rect.x0(), exp_x0),
                (rect.y0(), exp_y0),
                (rect.x1(), exp_x1),
                (rect.y1(), exp_y1),
            ] {
                assert!(
                    (got - expect).abs() < 0.01,
                    "旋转页便签 Rect 落点（得 {got} 期望 {expect}）"
                );
            }
        }
        other => panic!("应为便签，得 {other:?}"),
    }
}
