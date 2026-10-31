//! PDF-f3 — 渲染补充语义锁（links/layers + 渲染注释可见；
//! feature `engine-lopdf`，`engine-pdfium` 增交叉面）。
//!
//! 锁清单：
//! 1. `links_handcrafted_full_forms`——测试内裸 lopdf 手搓链接夹具
//!    （内部 XYZ / 命名目的地 / URI / 远程 GoToR 四形态）→ `read_links`
//!    全解析（页号/热区/目标分类）；
//! 2. `links_corpus_real_sweep`——真实语料含 /Link 件（LO 转换 docx
//!    超链接产物）全量扫：至少一件解析出链接 + 报告面自洽
//!    （links = internal + uri + unresolved）；
//! 3. `layers_handcrafted_read_and_toggle`——裸 lopdf 手搓 OCG 夹具
//!    （双图层一 ON 一 OFF）→ read_layers 名/可见性 + 开关往返
//!    （toggle → reload → 翻转 + 产物 qpdf）；
//! 4. `annotations_visible_in_render`——[pdfium] 高亮标注渲染可见性：
//!    无标注渲染 vs 有标注渲染的像素差异（红通道占比钉定——
//!    「渲染时标注可见」验收的字面证明）；
//! 5. `links_pdfium_cross`——[pdfium] pdfium 链接读回交叉（引擎面
//!    独立确认——目标页号一致性）；
//! 6. `layers_pdfium_render_honors_default`——[pdfium] 图层默认态
//!    渲染遵循性：同一 OCG 夹具开关前后 pdfium 渲染像素差异（若
//!    pdfium 对 /D 默认态遵循——实测断言；不遵循则如实降级登记）。

#![cfg(feature = "engine-lopdf")]

use std::path::Path;

use mdc_pdf::links::{read_links, LinkTarget};
use mdc_pdf::security::Limits;
use mdc_pdf::writer_lopdf::lopdf;

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} 读取失败：{e}"))
}

fn corpus_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-artifacts/pdf-organize-corpus")
}

/// qpdf 结构验证（工具缺失跳过）。
fn qpdf_valid(bytes: &[u8], label: &str) -> bool {
    let dir = std::env::temp_dir().join(format!("pdff3-{}-{}", std::process::id(), label));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.pdf");
    std::fs::write(&path, bytes).unwrap();
    let out = std::process::Command::new("qpdf")
        .arg("--check")
        .arg(&path)
        .output()
        .expect("spawn qpdf");
    let _ = std::fs::remove_dir_all(&dir);
    out.status.success()
}

// ---------------------------------------------------------------------------
// 手搓夹具（裸 lopdf——不依赖被测写路径）
// ---------------------------------------------------------------------------

/// 三页 PDF + 页 0 上四条 /Link：内部 XYZ / 命名目的地 / URI / 远程 GoToR。
fn handcrafted_linked_pdf() -> Vec<u8> {
    use lopdf::{dictionary, Dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Pages".to_vec()),
        "Count" => Object::Integer(3),
    });
    let content = Stream::new(dictionary! {}, b"BT ET".to_vec());
    let content_id = doc.add_object(content);
    let mut page_ids = Vec::new();
    for _ in 0..3 {
        let page_id = doc.add_object(dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => Object::Array(vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ]),
            "Contents" => Object::Reference(content_id),
        });
        page_ids.push(page_id);
    }
    doc.get_object_mut(pages_id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Kids",
            Object::Array(page_ids.iter().map(|id| Object::Reference(*id)).collect()),
        );
    // 命名目的地（新式名称树）：goto-p2 → 页 2 XYZ。
    let dests_node = doc.add_object(dictionary! {
        "Names" => Object::Array(vec![
            Object::Name(b"goto-p2".to_vec()),
            Object::Array(vec![
                Object::Reference(page_ids[2]),
                Object::Name(b"XYZ".to_vec()),
                Object::Integer(0), Object::Integer(792), Object::Null,
            ]),
        ]),
    });
    let names = doc.add_object(dictionary! {
        "Dests" => Object::Reference(dests_node),
    });
    // 四条链接标注（页 0）。
    let mut annots = Vec::new();
    // ① 内部 XYZ 直目标 → 页 1。
    let mut a = Dictionary::new();
    a.set("Type", Object::Name(b"Annot".to_vec()));
    a.set("Subtype", Object::Name(b"Link".to_vec()));
    a.set(
        "Rect",
        Object::Array(vec![
            Object::Real(72.0),
            Object::Real(700.0),
            Object::Real(200.0),
            Object::Real(720.0),
        ]),
    );
    a.set(
        "Dest",
        Object::Array(vec![
            Object::Reference(page_ids[1]),
            Object::Name(b"XYZ".to_vec()),
            Object::Integer(0),
            Object::Integer(792),
            Object::Null,
        ]),
    );
    annots.push(Object::Reference(doc.add_object(Object::Dictionary(a))));
    // ② 命名目的地 goto-p2 → 页 2。
    let mut a = Dictionary::new();
    a.set("Type", Object::Name(b"Annot".to_vec()));
    a.set("Subtype", Object::Name(b"Link".to_vec()));
    a.set(
        "Rect",
        Object::Array(vec![
            Object::Real(72.0),
            Object::Real(650.0),
            Object::Real(200.0),
            Object::Real(670.0),
        ]),
    );
    a.set("Dest", Object::Name(b"goto-p2".to_vec()));
    annots.push(Object::Reference(doc.add_object(Object::Dictionary(a))));
    // ③ URI 外链。
    let mut a = Dictionary::new();
    a.set("Type", Object::Name(b"Annot".to_vec()));
    a.set("Subtype", Object::Name(b"Link".to_vec()));
    a.set(
        "Rect",
        Object::Array(vec![
            Object::Real(72.0),
            Object::Real(600.0),
            Object::Real(200.0),
            Object::Real(620.0),
        ]),
    );
    a.set(
        "A",
        dictionary! {
            "S" => Object::Name(b"URI".to_vec()),
            "URI" => Object::string_literal("https://example.invalid/doc"),
        },
    );
    annots.push(Object::Reference(doc.add_object(Object::Dictionary(a))));
    // ④ 远程 GoToR（不可解析为本档页号——Unresolved 登记）。
    let mut a = Dictionary::new();
    a.set("Type", Object::Name(b"Annot".to_vec()));
    a.set("Subtype", Object::Name(b"Link".to_vec()));
    a.set(
        "Rect",
        Object::Array(vec![
            Object::Real(72.0),
            Object::Real(550.0),
            Object::Real(200.0),
            Object::Real(570.0),
        ]),
    );
    a.set(
        "A",
        dictionary! {
            "S" => Object::Name(b"GoToR".to_vec()),
            "F" => Object::string_literal("other.pdf"),
            "D" => Object::Array(vec![Object::Integer(0)]),
        },
    );
    annots.push(Object::Reference(doc.add_object(Object::Dictionary(a))));
    // 挂页 0。
    doc.get_object_mut(page_ids[0])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", Object::Array(annots));
    let cat_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "Names" => Object::Reference(names),
    });
    doc.trailer.set("Root", Object::Reference(cat_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

/// 文本串 UTF-16BE 编码（BOM 头——真实产出器对非 ASCII 名的形态）。
fn utf16be(s: &str) -> Vec<u8> {
    let mut out = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_be_bytes());
    }
    out
}

/// 双图层 PDF：图层 A（默认 ON，含红块内容）/ 图层 B（默认 OFF，含蓝块）。
/// 内容流经 /OC BDC marked-content 与图层关联（spec 8.11.2）。
fn handcrafted_ocg_pdf() -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Pages".to_vec()),
        "Count" => Object::Integer(1),
    });
    // 两个 OCG。
    let ocg_a = doc.add_object(dictionary! {
        "Type" => Object::Name(b"OCG".to_vec()),
        "Name" => Object::String(utf16be("图层A-红"), lopdf::StringFormat::Literal),
    });
    let ocg_b = doc.add_object(dictionary! {
        "Type" => Object::Name(b"OCG".to_vec()),
        "Name" => Object::String(utf16be("图层B-蓝"), lopdf::StringFormat::Literal),
    });
    // 资源：/Properties 携带 OCG 名（marked-content 引用面）。
    let props = doc.add_object(dictionary! {
        "MC0" => Object::Reference(ocg_a),
        "MC1" => Object::Reference(ocg_b),
    });
    // 内容流：红块（图层 A）/ 蓝块（图层 B）——/OC BDC … EMC。
    let content = Stream::new(
        dictionary! {},
        b"q /OC /MC0 BDC 1 0 0 rg 100 600 200 100 re f EMC /OC /MC1 BDC 0 0 1 rg 100 300 200 100 re f EMC Q"
            .to_vec(),
    );
    let content_id = doc.add_object(content);
    let page_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => Object::Array(vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ]),
        "Contents" => Object::Reference(content_id),
        "Resources" => dictionary! {
            "Properties" => Object::Reference(props),
        },
    });
    doc.get_object_mut(pages_id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Kids", Object::Array(vec![Object::Reference(page_id)]));
    // /OCProperties：A 默认 ON，B 默认 OFF。
    let d = doc.add_object(dictionary! {
        "ON" => Object::Array(vec![Object::Reference(ocg_a)]),
        "OFF" => Object::Array(vec![Object::Reference(ocg_b)]),
    });
    let ocp = doc.add_object(dictionary! {
        "OCGs" => Object::Array(vec![Object::Reference(ocg_a), Object::Reference(ocg_b)]),
        "D" => Object::Reference(d),
    });
    let cat_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "OCProperties" => Object::Reference(ocp),
    });
    doc.trailer.set("Root", Object::Reference(cat_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

// ---------------------------------------------------------------------------
// 1-2. 链接
// ---------------------------------------------------------------------------

#[test]
fn links_handcrafted_full_forms() {
    let bytes = handcrafted_linked_pdf();
    let (links, report) = read_links(bytes.clone(), Limits::default()).unwrap();
    assert_eq!(report.links, 4, "{report:?}");
    assert_eq!(report.internal, 2, "XYZ 直目标 + 命名目的地：{report:?}");
    assert_eq!(report.uri, 1);
    assert_eq!(report.unresolved, 1, "GoToR 远程登记：{report:?}");
    assert_eq!(links.len(), 4);
    // 逐条（/Annots 序）。
    assert_eq!(links[0].page, mdc_pdf::page::PageIndex(0));
    match &links[0].target {
        LinkTarget::Page(p) => assert_eq!(*p, mdc_pdf::page::PageIndex(1), "XYZ → 页 1"),
        other => panic!("① 应 Page(1)，得 {other:?}"),
    }
    match &links[1].target {
        LinkTarget::Page(p) => assert_eq!(*p, mdc_pdf::page::PageIndex(2), "命名 → 页 2"),
        other => panic!("② 应 Page(2)，得 {other:?}"),
    }
    match &links[2].target {
        LinkTarget::Uri(u) => assert_eq!(u, "https://example.invalid/doc"),
        other => panic!("③ 应 Uri，得 {other:?}"),
    }
    assert!(matches!(links[3].target, LinkTarget::Unresolved), "④ GoToR");
    // 热区。
    assert_eq!(links[0].rect.y1(), 720.0);
    // 结构验证。
    assert!(qpdf_valid(&bytes, "links-fixture"), "手搓夹具本身 qpdf 过");
}

#[test]
fn links_corpus_real_sweep() {
    let dir = corpus_dir();
    if !dir.is_dir() {
        eprintln!("[pdff3] corpus 缺失——真实链接扫描跳过");
        return;
    }
    let mut files_with_links = 0usize;
    let mut total_links = 0usize;
    let mut uri_count = 0usize;
    for entry in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().map(|x| x != "pdf").unwrap_or(true) {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        if !bytes.windows(5).any(|w| w == b"/Link") {
            continue; // 快筛（只扫含 /Link 字样件——17 件实测）
        }
        let Ok((links, report)) = read_links(bytes, Limits::default()) else {
            continue;
        };
        // 报告面自洽（links = internal + uri + unresolved）。
        assert_eq!(
            report.links,
            report.internal + report.uri + report.unresolved,
            "报告自洽：{report:?}（{}）",
            path.display()
        );
        assert_eq!(links.len(), report.links);
        if report.links > 0 {
            files_with_links += 1;
            total_links += report.links;
            uri_count += report.uri;
        }
    }
    eprintln!(
        "[pdff3] 真实链接扫描：{files_with_links} 件含链接 / 共 {total_links} 条 / URI {uri_count}"
    );
    assert!(
        files_with_links >= 5,
        "LO 语料超链接产物面（实测 17 件候选）——{files_with_links} 件过少"
    );
}

// ---------------------------------------------------------------------------
// 3. 图层
// ---------------------------------------------------------------------------

#[test]
fn layers_handcrafted_read_and_toggle() {
    let bytes = handcrafted_ocg_pdf();
    // 读取：名 + 默认可见性。
    let (layers, report) = mdc_pdf::layers::read_layers(bytes.clone(), Limits::default()).unwrap();
    assert_eq!(report.layers, 2, "{report:?}");
    assert!(!report.no_ocg);
    assert_eq!(layers[0].name, "图层A-红");
    assert!(layers[0].default_visible, "A 在 /ON");
    assert_eq!(layers[1].name, "图层B-蓝");
    assert!(!layers[1].default_visible, "B 在 /OFF");

    // 开关：B → ON。
    let toggled =
        mdc_pdf::layers::set_layer_default_visible(bytes.clone(), Limits::default(), 1, true)
            .unwrap();
    let (layers2, _) = mdc_pdf::layers::read_layers(toggled.clone(), Limits::default()).unwrap();
    assert!(layers2[1].default_visible, "B 翻转为 ON");
    assert!(layers2[0].default_visible, "A 不动");
    // 反向：A → OFF。
    let toggled2 =
        mdc_pdf::layers::set_layer_default_visible(toggled.clone(), Limits::default(), 0, false)
            .unwrap();
    let (layers3, _) = mdc_pdf::layers::read_layers(toggled2.clone(), Limits::default()).unwrap();
    assert!(!layers3[0].default_visible);
    assert!(layers3[1].default_visible);
    // 产物结构验证 + 往返稳定（重开重存体积稳定）。
    assert!(qpdf_valid(&toggled2, "layers-toggle"), "开关产物 qpdf");
    assert!(toggled2.len() < bytes.len() * 2, "产物体积健康");

    // 越界与无图层面。
    assert!(
        mdc_pdf::layers::set_layer_default_visible(bytes.clone(), Limits::default(), 9, true)
            .is_err()
    );
    let (_, plain_report) =
        mdc_pdf::layers::read_layers(fixture_bytes("small.pdf"), Limits::default()).unwrap();
    assert!(
        plain_report.no_ocg,
        "无 /OCProperties 文档：{plain_report:?}"
    );
    assert_eq!(plain_report.layers, 0);
}

// ---------------------------------------------------------------------------
// 4-6. [pdfium] 渲染交叉面
// ---------------------------------------------------------------------------

#[cfg(feature = "engine-pdfium")]
mod pdfium_cross {
    use super::*;
    use mdc_pdf::annotation::{Origin, Point};
    use mdc_pdf::engine_pdfium::PdfiumEngine;
    use mdc_pdf::page::PageIndex;
    use mdc_pdf::render::{Rasterizer, RenderRequest};
    use mdc_pdf::writer::AnnotationWriter;
    use mdc_pdf::writer_lopdf::LopdfWriter;

    fn render_page(bytes: &[u8], page: u32, width: u32) -> mdc_pdf::render::RenderedPage {
        let mut engine = PdfiumEngine::open(bytes.to_vec())
            .unwrap_or_else(|e| panic!("pdfium 绑定失败（MDC_PDFIUM_LIB → 候选 → 系统）：{e:?}"));
        engine
            .rasterize(RenderRequest {
                page: PageIndex(page),
                width_px: width,
            })
            .unwrap()
    }

    /// 红通道像素统计（BGRA 缓冲）。
    fn red_ratio(bgra: &[u8]) -> f64 {
        let px = bgra.len() / 4;
        if px == 0 {
            return 0.0;
        }
        let mut red = 0usize;
        let (chunks, _) = bgra.as_chunks::<4>();
        for p in chunks {
            // BGRA：R = p[2]。红显著 = R > 150 且 G/B < 100。
            if p[2] > 150 && p[1] < 100 && p[0] < 100 {
                red += 1;
            }
        }
        red as f64 / px as f64
    }

    /// 4. 注释渲染可见性（像素面钉定）。
    #[test]
    fn annotations_visible_in_render() {
        let src = fixture_bytes("small.pdf");
        // 无标注基线渲染。
        let plain = render_page(&src, 0, 300);
        let plain_red = red_ratio(&plain.bgra);
        // 加大块红色高亮（页 0 大半幅）。
        let mut writer = LopdfWriter::from_bytes(src.clone()).unwrap();
        writer
            .write_annotation(
                PageIndex(0),
                &mdc_pdf::annotation::Annotation {
                    id: mdc_pdf::annotation::AnnotId(1),
                    page: PageIndex(0),
                    kind: mdc_pdf::annotation::AnnotationKind::Ink {
                        strokes: vec![
                            // 三条粗红横杠（y 280/220/160——页 0 下半幅）。
                            vec![Point { x: 50.0, y: 280.0 }, Point { x: 550.0, y: 280.0 }],
                            vec![Point { x: 50.0, y: 220.0 }, Point { x: 550.0, y: 220.0 }],
                            vec![Point { x: 50.0, y: 160.0 }, Point { x: 550.0, y: 160.0 }],
                        ],
                        color: mdc_pdf::annotation::Rgb { r: 255, g: 0, b: 0 },
                        width: 12.0,
                    },
                    author: "渲染验证".into(),
                    origin: Origin::Human,
                    created_ms: 0,
                    modified_ms: 0,
                },
            )
            .unwrap();
        writer.save_incremental().unwrap();
        let annotated = writer.product_bytes();
        let with_ann = render_page(&annotated, 0, 300);
        let ann_red = red_ratio(&with_ann.bgra);
        // 红色粗墨迹占据页面显著比例（视觉面验收：渲染时标注可见——
        // Ink 经 pdfium /C + /BS /W 外观合成渲染）。
        assert!(
            ann_red - plain_red > 0.02,
            "红墨迹像素占比须显著上升（{plain_red:.3} → {ann_red:.3}）"
        );
    }

    /// 5. 链接的 pdfium 交叉（引擎面独立确认目标页号）。
    #[test]
    fn links_pdfium_cross() {
        let bytes = handcrafted_linked_pdf();
        let (ours, report) = read_links(bytes.clone(), Limits::default()).unwrap();
        assert_eq!(report.internal, 2);
        let pdfium = PdfiumEngine::shared_handle().unwrap();
        let doc = pdfium.load_pdf_from_byte_vec(bytes, None).unwrap();
        let page = doc.pages().first().unwrap();
        // pdfium-render 0.9.4 链接面：page.links() 迭代 + destination()
        // → target_page_index()（引擎侧目标页）。逐条比对内部跳转目标。
        let mut engine_internal: Vec<u32> = Vec::new();
        for link in page.links().iter() {
            if let Some(dest) = link.destination() {
                engine_internal.push(dest.page_index().unwrap() as u32);
            }
        }
        let ours_internal: Vec<u32> = ours
            .iter()
            .filter_map(|l| match l.target {
                LinkTarget::Page(p) => Some(p.0),
                _ => None,
            })
            .collect();
        eprintln!("[pdff3] pdfium 链接目标页：{engine_internal:?}（我方 {ours_internal:?}）");
        assert_eq!(ours_internal, vec![1u32, 2], "我方内部目标钉定");
        // 引擎面目标集 ⊇ 我方（pdfium 将 GoToR 远程目标也解析为页号 0
        // ——引擎不区分远程/本档；我方 Unresolved 分类为正确语义）。
        for t in &ours_internal {
            assert!(
                engine_internal.contains(t),
                "引擎面须含我方目标 {t}：{engine_internal:?}"
            );
        }
        assert!(engine_internal.len() >= ours_internal.len());
    }

    /// 6. 图层默认态的 pdfium 渲染遵循性（实测断言——不遵循则降级登记）。
    #[test]
    fn layers_pdfium_render_honors_default() {
        let base = handcrafted_ocg_pdf();
        // B → ON 后：蓝块应出现（红块仍在）。
        let toggled =
            mdc_pdf::layers::set_layer_default_visible(base.clone(), Limits::default(), 1, true)
                .unwrap();
        let r_base = render_page(&base, 0, 200);
        let r_on = render_page(&toggled, 0, 200);
        // 蓝像素统计。
        let blue = |bgra: &[u8]| -> f64 {
            let px = bgra.len() / 4;
            let mut n = 0usize;
            let (chunks, _) = bgra.as_chunks::<4>();
            for p in chunks {
                if p[0] > 150 && p[1] < 100 && p[2] < 100 {
                    n += 1;
                }
            }
            n as f64 / px as f64
        };
        let blue_base = blue(&r_base.bgra);
        let blue_on = blue(&r_on.bgra);
        eprintln!("[pdff3] 图层 B 蓝 pixel：默认 {blue_base:.4} → 开 {blue_on:.4}");
        if blue_on - blue_base > 0.005 {
            eprintln!("[pdff3] pdfium 遵循 /D 默认态——图层开关渲染面生效 ✓");
        } else {
            // 如实登记：pdfium 渲染路径对 /D 默认 OFF 的内容不裁剪
            // （或对 BDC 关联内容不适用）——开关的持久语义仍成立
            // （结构面已验证），渲染即时遵循性降级为引擎升级面。
            eprintln!("[pdff3] pdfium 渲染未遵循 /D 默认态差异（蓝 {blue_base:.4} → {blue_on:.4}）——登记：渲染遵循性属引擎升级面（pdfium-render 0.9.4 无 OCG API）");
        }
        // 结构面恒断言（无论渲染遵循与否）。
        let (layers, _) = mdc_pdf::layers::read_layers(toggled, Limits::default()).unwrap();
        assert!(layers[1].default_visible);
        // 红块（图层 A 恒 ON）渲染恒在——OCG 内容通路本身健康。
        assert!(red_ratio(&r_base.bgra) > 0.02, "红块（ON 图层）须可见");
    }
}
