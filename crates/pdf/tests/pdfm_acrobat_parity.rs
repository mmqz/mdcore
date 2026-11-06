//! pdfm_acrobat_parity —— issue #229：Acrobat Reader 1:1 功能对齐验收面
//! （高频六面之五：打开/渲染/表单/签名/标注；打印 = 已登记缺口，见
//! `docs/pdf-acrobat-parity.md` §缺口清单与跟进 issue）。
//!
//! 定位：**验收聚合**（用户旅程口径），不是新语义——各面精细语义锁在
//! 既有测试组（pdfc_engine / pdfg_forms / pdfj_forms_adv / pdfi_signature /
//! pdff2_annotate / pdfl_forms_incremental …），本文件把高频面串成
//! 「Acrobat 用户能完成这件事」的最小闭环断言，作为 parity 清单
//! （`docs/pdf-acrobat-parity.md`）的自动化锚点——清单每行的「测试锚点」
//! 列指向本文件对应 `parity_*` 函数（或既有测试组）。
//!
//! 高频面 × 本文件映射：
//! 1. 打开 → `parity_open_all_fixtures_pdfium_loads_and_counts` /
//!    `parity_open_all_fixtures_lopdf_writer_parses`（双路装载全夹具谱）
//! 2. 渲染 → `parity_render_first_page_bitmap`（位图合同 + 真实落笔）
//! 3. 表单 → `parity_form_fill_save_reopen`（填值→增量保存→重开可见）
//! 4. 签名 → `parity_signature_read_and_signed_probe`（读签名 + 已签名探针）
//! 5. 标注 → `parity_annotation_add_persist_remove`（增→存→重开→删→零残留）
//! 6. 打印 → `parity_print_render_dpi_range_annotations`（#240：DPI/页区间/注释开关三合一——引擎原语面；壳面系统打印流与 web 打印态在 mang-shell/web，真机打印验收待登记）。
//!
//! 环境合同与 pdfc_engine.rs 同源（不静默 skip）：pdfium 库走引擎定位序
//! （env `MDC_PDFIUM_LIB` → 候选清单 → 系统），fixtures 走 crate 本地
//! `fixtures/`（env `MDC_PDF_TEST_FIXTURES` 覆盖面见 pdfc_engine.rs）。

#![cfg(any(feature = "engine-pdfium", feature = "engine-lopdf"))]

use std::path::PathBuf;

use mdc_pdf::page::PageIndex;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = fixtures().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} 读取失败：{e}"))
}

/// parity 打开面夹具谱：普通/书签/表单×3/签名/扫描——覆盖仓内全部
/// 真实形态 PDF（`crates/pdf/fixtures/`，生成器 gen_*.py 在档）。
const PARITY_FIXTURES: &[&str] = &[
    "small.pdf",
    "bookmarked.pdf",
    "form_probe.pdf",
    "form_advanced.pdf",
    "form_bid_letter.pdf",
    "signed_sample.pdf",
    "scanned_mixed.pdf",
];

// ---------------------------------------------------------------- 1. 打开

/// 高频面①打开（pdfium 读路）：全夹具谱装载成功 + 页数 ≥1 + 基线夹具
/// 页数精确锚（small.pdf = 3 页，Letter 构造参数——pdfc_engine 锁 1 同锚）。
#[cfg(feature = "engine-pdfium")]
#[test]
fn parity_open_all_fixtures_pdfium_loads_and_counts() {
    use mdc_pdf::engine::DocumentEngine;
    use mdc_pdf::engine_pdfium::PdfiumEngine;
    for name in PARITY_FIXTURES {
        let engine = PdfiumEngine::open(fixture_bytes(name))
            .unwrap_or_else(|e| panic!("{name} pdfium 打开失败：{e:?}"));
        assert!(
            engine.page_count() >= 1,
            "{name} 页数应 ≥ 1（0 页 = OpenError::EmptyDocument 语义）"
        );
    }
    let small = PdfiumEngine::open(fixture_bytes("small.pdf")).unwrap();
    assert_eq!(small.page_count(), 3, "基线夹具页数精确锚");
}

/// 高频面①打开（lopdf 写路装载）：写侧管线对全夹具谱可解析
/// （`LopdfWriter::from_bytes`——注释/表单保存的装载入口）。
#[cfg(feature = "engine-lopdf")]
#[test]
fn parity_open_all_fixtures_lopdf_writer_parses() {
    use mdc_pdf::writer_lopdf::LopdfWriter;
    for name in PARITY_FIXTURES {
        LopdfWriter::from_bytes(fixture_bytes(name))
            .unwrap_or_else(|e| panic!("{name} lopdf 装载失败：{e:?}"));
    }
}

// ---------------------------------------------------------------- 2. 渲染

/// 高频面②渲染：首页位图合同（缓冲 w*h*4 + Letter 等比 300→388）+
/// 真实落笔（非全零 + 白底主导——pdfc_engine 锁 8/9 的验收聚合）。
#[cfg(feature = "engine-pdfium")]
#[test]
fn parity_render_first_page_bitmap() {
    use mdc_pdf::engine_pdfium::PdfiumEngine;
    use mdc_pdf::render::{Rasterizer, RenderRequest};

    let mut engine = PdfiumEngine::open(fixture_bytes("small.pdf")).unwrap();
    let page = engine
        .rasterize(RenderRequest {
            page: PageIndex(0),
            width_px: 300,
        })
        .unwrap();

    assert_eq!(page.width_px, 300, "渲染宽度 = 请求宽");
    assert_eq!(page.height_px, 388, "Letter 等比高（612×792 → 300×388）");
    assert_eq!(page.bgra.len(), 300 * 388 * 4, "位图缓冲合同 w*h*4（BGRA）");

    let nonzero = page.bgra.iter().filter(|&&b| b != 0).count();
    assert!(nonzero > 0, "渲染缓冲非全零（pdfium 确实落笔）");
    let white: usize = page
        .bgra
        .chunks_exact(4)
        .filter(|px| px[0] >= 240 && px[1] >= 240 && px[2] >= 240 && px[3] == 255)
        .count();
    assert!(
        white * 2 > 300 * 388,
        "白底应主导（白像素 {} / 总 {}——真实文档页非噪声图）",
        white,
        300 * 388
    );
}

// ---------------------------------------------------------------- 3. 表单

/// 高频面③表单：填值 → **增量保存**（前缀逐位不变——#227 语义）→
/// 重开新值可见（Acrobat「填表并保存」旅程的最小闭环）。
/// 精细语义（修订段计数/链传递/守卫）锁在 pdfl_forms_incremental。
#[cfg(feature = "engine-lopdf")]
#[test]
fn parity_form_fill_save_reopen() {
    use mdc_pdf::forms::{FormField, PdfForm};

    let original = fixture_bytes("form_bid_letter.pdf");
    let mut form = PdfForm::read(&original).unwrap();
    form.set_field("project_name", "智慧园区弱电工程").unwrap();
    let out = form.save().unwrap();

    assert!(
        out.starts_with(&original),
        "增量保存产物必须以原字节为逐位前缀（全量重写 = #227 修复前缺陷形态）"
    );

    let back = PdfForm::read(&out).unwrap();
    let Some(FormField::Text { value, .. }) = back.field("project_name") else {
        panic!("project_name 应为文本域");
    };
    assert_eq!(value, "智慧园区弱电工程", "填值重开可见（中文往返无损）");
}

// ---------------------------------------------------------------- 4. 签名

/// 高频面④签名：读签名（签名人/结构有效）+ 已签名探针（表单保存保护面
/// 的判定原语，#227）。密码学验签不在 v1 范围（pdfi_signature 头注登记
/// ——自签证书夹具，结构面与真实签名一致）。
#[cfg(feature = "engine-lopdf")]
#[test]
fn parity_signature_read_and_signed_probe() {
    use mdc_pdf::forms::PdfForm;
    use mdc_pdf::signature::read_signatures;

    let sigs = read_signatures(&fixture_bytes("signed_sample.pdf")).unwrap();
    assert_eq!(sigs.len(), 1, "应读出 1 个签名域");
    assert!(
        sigs[0].structurally_valid,
        "结构应完整：{:?}",
        sigs[0].structural_issue
    );
    assert!(
        sigs[0].signer_name.is_some(),
        "签名人应可读（/Name 缺省回退证书 CN）"
    );

    let signed_form = PdfForm::read(&fixture_bytes("signed_sample.pdf")).unwrap();
    assert!(
        signed_form.has_signed_signature_fields(),
        "已签名探针：signed_sample.pdf 应阳性"
    );
    let plain_form = PdfForm::read(&fixture_bytes("form_bid_letter.pdf")).unwrap();
    assert!(
        !plain_form.has_signed_signature_fields(),
        "已签名探针：未签署表单应阴性（不误伤）"
    );
}

// ---------------------------------------------------------------- 5. 标注

/// 高频面⑤标注：增（Note）→ 存（增量）→ **重开可见** → 删 → 存 →
/// 重开零残留（Acrobat「加批注/保存/删除」旅程的最小闭环）。
/// 11 kind 字段级往返锁在 pdff2_annotate；增量/脏集锁在 pdfd_writer。
#[cfg(feature = "engine-lopdf")]
#[test]
fn parity_annotation_add_persist_remove() {
    use mdc_pdf::annotate::PdfAnnotator;
    use mdc_pdf::annotation::{AnnotationKind, Origin, Rect};

    let src = fixture_bytes("small.pdf");
    let mut writer = PdfAnnotator::open(src.clone()).unwrap();
    assert_eq!(writer.list(None).len(), 0, "small.pdf 无既有标注");

    let id = writer
        .add(
            PageIndex(0),
            AnnotationKind::Note {
                rect: Rect::new(80.0, 700.0, 100.0, 720.0).unwrap(),
            },
            "parity",
            Origin::Human,
        )
        .unwrap();
    let _ = id;
    writer.save().unwrap();
    let product = writer.product_bytes();
    assert!(
        product.starts_with(&src),
        "标注保存 = 增量修订（前缀逐位不变——PDF-d 语义）"
    );

    // 重开可见（持久化实证——非仅内存态）。
    let mut reopened = PdfAnnotator::open(product).unwrap();
    assert_eq!(reopened.list(None).len(), 1, "保存产物重开应含 1 条标注");

    // 删除 → 保存 → 重开零残留。
    let id = reopened.list(None)[0].id;
    reopened.remove(id).unwrap();
    reopened.save().unwrap();
    let after = PdfAnnotator::open(reopened.product_bytes()).unwrap();
    assert_eq!(after.list(None).len(), 0, "删除保存后重开应零标注");
}

// ---------------------------------------------------------------- 6. 打印

/// #240 打印面（引擎原语）：DPI 换算 + 页区间 + 注释打印开关——
/// 三合一行为闭环（#240 验收：输出与屏幕渲染一致 + 注释开关行为
/// 符合预期 + 页区间可选）。
///
/// - DPI：Letter 612pt @ 150dpi → 1275px（72pt 基准 × DPI/72）；
/// - 页区间：闭区间 [1,2] → 2 页（small.pdf = 3 页）；
/// - 注释开关：Ink 红墨迹经 [`RenderOptions::render_annotations`]
///   翻转——false = 「仅文档」档（红占比回落基线），true = 「文档和
///   标注」档（红占比显著上升——与 pdff3 锁 4 同一像素口径）。
#[cfg(feature = "engine-pdfium")]
#[test]
fn parity_print_render_dpi_range_annotations() {
    use mdc_pdf::engine_pdfium::PdfiumEngine;
    use mdc_pdf::page::PageIndex;
    use mdc_pdf::print::{render_for_print, PageSelection, PrintSpec};
    use mdc_pdf::writer::AnnotationWriter;
    use mdc_pdf::writer_lopdf::LopdfWriter;

    // --- ① DPI 换算 + 页区间（small.pdf：3 页 Letter 612×792pt） ---
    let mut engine = PdfiumEngine::open(fixture_bytes("small.pdf")).unwrap();
    let spec = PrintSpec::new(150, true);
    let pages = render_for_print(
        &mut engine,
        &spec,
        &PageSelection::Range {
            from: PageIndex(1),
            to: PageIndex(2),
        },
    )
    .unwrap();
    assert_eq!(pages.len(), 2, "闭区间 [1,2] → 2 页");
    assert_eq!(pages[0].width_px, 1275, "Letter 612pt @ 150dpi → 1275px");
    assert_eq!(pages[0].height_px, 1650, "等比高 792pt → 1650px");
    assert_eq!(pages[0].bgra.len(), 1275 * 1650 * 4, "位图合同 w*h*4");
    // 72dpi = 1:1 屏显基准（与屏幕渲染同口径的最小锚）。
    let base =
        render_for_print(&mut engine, &PrintSpec::new(72, true), &PageSelection::All).unwrap();
    assert_eq!(base.len(), 3, "All → 3 页");
    assert_eq!(base[0].width_px, 612, "72dpi = 1:1 pt→px");

    // --- ② 注释打印开关（红墨迹可见性翻转——像素口径） ---
    fn red_ratio(bgra: &[u8]) -> f64 {
        let px = bgra.len() / 4;
        if px == 0 {
            return 0.0;
        }
        let mut red = 0usize;
        let (chunks, _) = bgra.as_chunks::<4>();
        for p in chunks {
            // BGRA：R = p[2]。
            if p[2] > 150 && p[1] < 100 && p[0] < 100 {
                red += 1;
            }
        }
        red as f64 / px as f64
    }

    // 基线（无标注）与「仅文档」档应同口径（红占比 ≈ 基线）。
    let plain_red = red_ratio(&base[0].bgra);

    // 加大块红色 Ink（同 pdff3 锁 4 的构造——页 0 下半幅三条粗红杠）。
    let mut writer = LopdfWriter::from_bytes(fixture_bytes("small.pdf")).unwrap();
    writer
        .write_annotation(
            PageIndex(0),
            &mdc_pdf::annotation::Annotation {
                id: mdc_pdf::annotation::AnnotId(1),
                page: PageIndex(0),
                kind: mdc_pdf::annotation::AnnotationKind::Ink {
                    strokes: vec![
                        vec![
                            mdc_pdf::annotation::Point { x: 50.0, y: 280.0 },
                            mdc_pdf::annotation::Point { x: 550.0, y: 280.0 },
                        ],
                        vec![
                            mdc_pdf::annotation::Point { x: 50.0, y: 220.0 },
                            mdc_pdf::annotation::Point { x: 550.0, y: 220.0 },
                        ],
                        vec![
                            mdc_pdf::annotation::Point { x: 50.0, y: 160.0 },
                            mdc_pdf::annotation::Point { x: 550.0, y: 160.0 },
                        ],
                    ],
                    color: mdc_pdf::annotation::Rgb { r: 255, g: 0, b: 0 },
                    width: 12.0,
                },
                author: "打印验证".into(),
                origin: mdc_pdf::annotation::Origin::Human,
                created_ms: 0,
                modified_ms: 0,
            },
        )
        .unwrap();
    writer.save_incremental().unwrap();
    let annotated = writer.product_bytes();

    // 「文档和标注」档（render_annotations: true）。
    let mut ann_engine = PdfiumEngine::open(annotated.clone()).unwrap();
    let with_ann = render_for_print(
        &mut ann_engine,
        &PrintSpec::new(72, true),
        &PageSelection::All,
    )
    .unwrap();
    let ann_red = red_ratio(&with_ann[0].bgra);
    assert!(
        ann_red - plain_red > 0.02,
        "注释打印开：红墨迹显著可见（{plain_red:.3} → {ann_red:.3}）"
    );

    // 「仅文档」档（render_annotations: false）——开关行为符合预期。
    let mut ann_engine2 = PdfiumEngine::open(annotated).unwrap();
    let without_ann = render_for_print(
        &mut ann_engine2,
        &PrintSpec::new(72, false),
        &PageSelection::All,
    )
    .unwrap();
    let off_red = red_ratio(&without_ann[0].bgra);
    assert!(
        (off_red - plain_red).abs() < 0.005,
        "注释打印关：红墨迹不可见（基线 {plain_red:.3} vs 关档 {off_red:.3}）"
    );
}
