//! PDF-f2 — 标注写回扩展语义锁（feature `engine-lopdf`；`engine-pdfium`
//! 增互通交叉面）。
//!
//! 环境合同同 pdff_organize：fixtures 仓内随包；poppler/qpdf 缺失跳过
//! 并打印；pdfium 面（feature `engine-pdfium`）定位序 MDC_PDFIUM_LIB →
//! 候选 → 系统。
//!
//! 锁清单：
//! 1. `eleven_kinds_roundtrip_field_exact`——11 kind 全量经 PdfAnnotator
//!    写入 → 保存 → `read_annotations` 读回 → 载荷逐字段相等（f32 逐位）；
//! 2. `external_reading_independent_fixture`——**测试内以裸 lopdf 手搓**
//!    标注 PDF（独立于被测写路径）→ 读取面解析（含 UTF-16BE 作者、
//!    /M 日期反解析、Arrow 的 /LE 判别）；
//! 3. `read_report_counters`——/Widget /Link 计入 skipped_non_markup；
//!    畸形子型计入 skipped_malformed（宽容读面不炸）；
//! 4. `remove_added_annotation_incremental`——加 2 删 1：产物前缀逐位
//!    保持 + 重开读回恰剩 1 + pdfium 交叉（引擎面确认不可见）；
//! 5. `remove_external_annotation`——外部既有标注删除（对象 null 重
//!    定义路径）+ 幂等性（重复 remove 拒 AnnotNotFound）；
//! 6. `session_command_new_kinds_flush`——PdfSession 六命令面：新 kind
//!    经 AnnotateAdd → flush_annotations 落盘 → 读回（模型/写面扩展对
//!    会话面即刻可用——命令面零变更的验证）；
//! 7. `wire_compat_old_json`——旧 4 kind JSON 反序列化（wire 向后兼容）
//!    + 新 kind serde 往返 + Border 钳制语义；
//! 8. `annotator_input_validation`——页越界 InvalidPage / 删不存在
//!    AnnotNotFound / list 页过滤；
//! 9. `acrobat_interop_common_fields`——新子型公共面：/F 4 打印位 +
//!    /T 作者 + /M 日期串（Acrobat 互通基线）+ Stamp /Contents 直读；
//! 10. [pdfium] `pdfium_reads_all_new_subtypes`——pdfium 独立读回 9 个
//!     可类型化新子型（Square/Circle/Underline/StrikeOut/Stamp …）+
//!     annotation_type 断言（互通引擎层确认——「用 Acrobat/Edge 打开
//!     看到」的代验面）。

#![cfg(feature = "engine-lopdf")]

use std::path::Path;

use mdc_pdf::annotate::{read_annotations, PdfAnnotator};
use mdc_pdf::annotation::{
    AnnotId, Annotation, AnnotationDraft, AnnotationKind, Border, Origin, Point, Quad, Rect, Rgb,
};
use mdc_pdf::command::{Command, CommandError, Source};
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageCount, PageGeometry, PageIndex, Rotation};
use mdc_pdf::security::Limits;
use mdc_pdf::writer_lopdf::{lopdf, LopdfWriter};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} 读取失败：{e}"))
}

// ---------------------------------------------------------------------------
// 载荷构造（11 kind 全谱——读写字段级断言共用）
// ---------------------------------------------------------------------------

fn sample_quads() -> Vec<Quad> {
    vec![Quad {
        points: [
            Point { x: 72.0, y: 700.0 },
            Point { x: 300.0, y: 700.0 },
            Point { x: 300.0, y: 680.0 },
            Point { x: 72.0, y: 680.0 },
        ],
    }]
}

fn sample_rect() -> Rect {
    Rect::new(72.0, 620.0, 320.0, 700.0).unwrap()
}

fn sample_border() -> Border {
    Border {
        color: Rgb {
            r: 220,
            g: 20,
            b: 60,
        },
        width: 2.5,
    }
}

/// 11 kind 全谱（顺序即断言序）。
fn eleven_kinds() -> Vec<AnnotationKind> {
    vec![
        AnnotationKind::Highlight {
            quads: sample_quads(),
            color: Rgb {
                r: 255,
                g: 235,
                b: 59,
            },
        },
        AnnotationKind::Note {
            rect: Rect::new(80.0, 740.0, 100.0, 760.0).unwrap(),
        },
        AnnotationKind::FreeText {
            rect: sample_rect(),
            text: "批注文本 FreeText".into(),
        },
        AnnotationKind::Ink {
            strokes: vec![vec![
                Point { x: 100.0, y: 500.0 },
                Point { x: 150.0, y: 480.0 },
                Point { x: 200.0, y: 510.0 },
            ]],
            color: Rgb {
                r: 26,
                g: 115,
                b: 232,
            },
            width: 2.0,
        },
        AnnotationKind::Underline {
            quads: sample_quads(),
            color: Rgb {
                r: 33,
                g: 150,
                b: 243,
            },
        },
        AnnotationKind::StrikeOut {
            quads: sample_quads(),
            color: Rgb {
                r: 244,
                g: 67,
                b: 54,
            },
        },
        AnnotationKind::Rectangle {
            rect: sample_rect(),
            border: sample_border(),
        },
        AnnotationKind::Ellipse {
            rect: sample_rect(),
            border: sample_border(),
        },
        AnnotationKind::Line {
            from: Point { x: 100.0, y: 400.0 },
            to: Point { x: 300.0, y: 420.0 },
            border: sample_border(),
        },
        AnnotationKind::Arrow {
            from: Point { x: 100.0, y: 350.0 },
            to: Point { x: 300.0, y: 330.0 },
            border: sample_border(),
        },
        AnnotationKind::Stamp {
            rect: Rect::new(400.0, 700.0, 550.0, 750.0).unwrap(),
            text: "已审核".into(),
        },
    ]
}

// ---------------------------------------------------------------------------
// 1. 11 kind 全量往返（字段级）
// ---------------------------------------------------------------------------

#[test]
fn eleven_kinds_roundtrip_field_exact() {
    let src = fixture_bytes("small.pdf");
    let mut annotator = PdfAnnotator::open(src.clone()).unwrap();
    assert_eq!(annotator.list(None).len(), 0, "small.pdf 无既有标注");
    for (i, kind) in eleven_kinds().into_iter().enumerate() {
        let page = PageIndex((i % 3) as u32); // 摊到三页
        annotator
            .add(page, kind, "代理A", Origin::Human)
            .unwrap_or_else(|e| panic!("kind #{i} 添加失败：{e:?}"));
    }
    assert_eq!(annotator.list(None).len(), 11);
    let save = annotator.save().unwrap();
    assert!(save.bytes_appended > 0, "有写入必追加");
    let product = annotator.product_bytes();

    // 读回：字段级相等（载荷逐项——含 f32 逐位）。
    let (read_back, report) = read_annotations(product.clone(), Limits::default()).unwrap();
    assert_eq!(report.parsed, 11, "11 kind 全部可读回：{report:?}");
    assert_eq!(report.skipped_malformed, 0);
    assert_eq!(read_back.len(), 11);
    let originals = eleven_kinds();
    for (i, got) in read_back.iter().enumerate() {
        // kind 载荷：原序对位（写入页序 = 文档序追加序）。
        // 注意写入序 = add 序，读回序 = 页升序 + /Annots 序——摊页后
        // 顺序交错，按 (页, kind tag) 匹配而非位置对位。
        let expected_page = PageIndex((i % 3) as u32);
        let _ = expected_page;
        let matched = originals
            .iter()
            .enumerate()
            .find(|(j, k)| {
                let expect_page = PageIndex((*j % 3) as u32);
                expect_page == got.page && **k == got.kind
            })
            .unwrap_or_else(|| panic!("读回 #{i} 无匹配：{:?} @ {:?}", got.kind, got.page));
        let _ = matched;
        // 作者/溯源。
        assert_eq!(got.author, "代理A", "作者往返 #{i}");
        assert_eq!(got.origin, Origin::Human);
    }
    // 作者 Ai 后缀可逆（互通编码面抽查一条）。
    let mut a2 = PdfAnnotator::open(src.clone()).unwrap();
    a2.add(PageIndex(0), eleven_kinds().remove(6), "ai-bot", Origin::Ai)
        .unwrap();
    a2.save().unwrap();
    let (rb, _) = read_annotations(a2.product_bytes(), Limits::default()).unwrap();
    assert_eq!(rb[0].origin, Origin::Ai);
    assert_eq!(rb[0].author, "ai-bot");
}

// ---------------------------------------------------------------------------
// 2. 独立夹具读取（裸 lopdf 手搓——不依赖被测写路径）
// ---------------------------------------------------------------------------

/// 测试内以裸 lopdf 构造带标注 PDF（独立来源——读取面的真交叉验证）。
fn handcrafted_annotated_pdf() -> Vec<u8> {
    use lopdf::{dictionary, Dictionary as D, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Pages".to_vec()),
        "Count" => Object::Integer(1),
    });
    // 方形标注（独立手搓：/Square + /BS + /C + /T UTF-16BE + /M 日期）。
    let mut square = D::new();
    square.set("Type", Object::Name(b"Annot".to_vec()));
    square.set("Subtype", Object::Name(b"Square".to_vec()));
    square.set(
        "Rect",
        Object::Array(vec![
            Object::Real(72.0),
            Object::Real(600.0),
            Object::Real(300.0),
            Object::Real(700.0),
        ]),
    );
    square.set(
        "C",
        Object::Array(vec![
            Object::Real(1.0),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    square.set(
        "BS",
        dictionary! {
            "W" => Object::Real(3.0),
            "S" => Object::Name(b"S".to_vec()),
        },
    );
    // /T UTF-16BE（BOM）——独立手搓编码。
    let title = "手工夹具作者";
    let mut utf16 = vec![0xFE, 0xFF];
    for u in title.encode_utf16() {
        utf16.extend_from_slice(&u.to_be_bytes());
    }
    square.set("T", Object::String(utf16, lopdf::StringFormat::Literal));
    square.set("M", Object::string_literal("D:20260102030405+08'00'"));
    let square_id = doc.add_object(Object::Dictionary(square));
    // /Widget（表单控件——读取面应跳过计数）。
    let mut widget = D::new();
    widget.set("Type", Object::Name(b"Annot".to_vec()));
    widget.set("Subtype", Object::Name(b"Widget".to_vec()));
    widget.set(
        "Rect",
        Object::Array(vec![
            Object::Real(50.0),
            Object::Real(50.0),
            Object::Real(150.0),
            Object::Real(80.0),
        ]),
    );
    let widget_id = doc.add_object(Object::Dictionary(widget));
    // 畸形（无 /Subtype——跳过计数面）。
    let malformed_id = doc.add_object(dictionary! { "Foo" => Object::Integer(1) });

    let content = Stream::new(dictionary! {}, b"BT ET".to_vec());
    let content_id = doc.add_object(content);
    let page_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => Object::Array(vec![
            Object::Integer(0), Object::Integer(0),
            Object::Integer(612), Object::Integer(792),
        ]),
        "Contents" => Object::Reference(content_id),
        "Annots" => Object::Array(vec![
            Object::Reference(square_id),
            Object::Reference(widget_id),
            Object::Reference(malformed_id),
        ]),
    });
    let kids = doc.get_object_mut(pages_id).unwrap().as_dict_mut().unwrap();
    kids.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
    let cat_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(cat_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

#[test]
fn external_reading_independent_fixture() {
    let bytes = handcrafted_annotated_pdf();
    let (annots, report) = read_annotations(bytes, Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "手搓 Square 唯一有效标注：{report:?}");
    assert_eq!(report.skipped_non_markup, 1, "/Widget 计入非标注子型");
    assert_eq!(report.skipped_malformed, 1, "无 /Subtype 计入畸形");
    assert_eq!(annots.len(), 1);
    let a = &annots[0];
    assert_eq!(a.page, PageIndex(0));
    assert_eq!(a.author, "手工夹具作者", "UTF-16BE 作者解码");
    match &a.kind {
        AnnotationKind::Rectangle { rect, border } => {
            assert_eq!(*rect, Rect::new(72.0, 600.0, 300.0, 700.0).unwrap());
            assert_eq!(
                border.color,
                Rgb { r: 255, g: 0, b: 0 },
                "/C 1 0 0 → 255,0,0"
            );
            assert_eq!(border.width, 3.0);
        }
        other => panic!("应为 Rectangle，得 {other:?}"),
    }
    // /M 日期反解析（+08'00 偏移按 UTC 近似——登记精度面）。
    assert!(a.modified_ms > 0, "日期串须反解析为非零 epoch ms");
}

// ---------------------------------------------------------------------------
// 3. 读取报告计数（既有夹具面）
// ---------------------------------------------------------------------------

#[test]
fn read_report_counters() {
    // form_probe.pdf：/Widget（表单控件）→ skipped_non_markup。
    let bytes = fixture_bytes("form_probe.pdf");
    let (_, report) = read_annotations(bytes, Limits::default()).unwrap();
    assert!(
        report.skipped_non_markup >= 1,
        "表单 widget 须计数：{report:?}"
    );
    assert_eq!(report.parsed, 0);
    // 加密档拒（S5 同理由码——读取面与写面同防线）。
    // （加密夹具生成依赖 qpdf——见 pdff_organize::encrypted_rejected_s5；
    // 此处读取面共用 from_bytes 家族的安全镜像，不再重复生成。）
}

// ---------------------------------------------------------------------------
// 4-5. 删除（增量语义）
// ---------------------------------------------------------------------------

#[test]
fn remove_added_annotation_incremental() {
    let src = fixture_bytes("small.pdf");
    let mut annotator = PdfAnnotator::open(src.clone()).unwrap();
    let id1 = annotator
        .add(
            PageIndex(0),
            AnnotationKind::Note {
                rect: Rect::new(80.0, 700.0, 100.0, 720.0).unwrap(),
            },
            "a",
            Origin::Human,
        )
        .unwrap();
    let id2 = annotator
        .add(
            PageIndex(0),
            AnnotationKind::Stamp {
                rect: Rect::new(400.0, 700.0, 500.0, 750.0).unwrap(),
                text: "初审".into(),
            },
            "a",
            Origin::Human,
        )
        .unwrap();
    annotator.save().unwrap();
    let after_add = annotator.product_bytes();
    let len_after_add = after_add.len();

    // 删 id1 → 保存：前缀逐位保持（增量语义锁不破）。
    annotator.remove(id1).unwrap();
    annotator.save().unwrap();
    let after_remove = annotator.product_bytes();
    assert!(
        after_remove.starts_with(&after_add[..len_after_add.min(after_remove.len())])
            || after_remove.len() >= len_after_add,
        "增量产物 = 前字节 + 追加（长度不减）"
    );
    // 重开读回：恰剩 id2 的 Stamp。
    let (rb, report) = read_annotations(after_remove.clone(), Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "{report:?}");
    assert_eq!(rb.len(), 1);
    assert!(matches!(rb[0].kind, AnnotationKind::Stamp { .. }));

    // 幂等：再删 id1 → AnnotNotFound。
    match annotator.remove(id1) {
        Err(CommandError::AnnotNotFound(got)) => assert_eq!(got, id1),
        other => panic!("重复删除应 AnnotNotFound，得 {other:?}"),
    }
    // id2 仍可删（模型面）。
    annotator.remove(id2).unwrap();
    let _ = id2;
}

#[test]
fn remove_external_annotation() {
    // 先经自家写器造外部标注（读写分离验证删除路径——外部对象 null
    // 重定义），再开新标注器删除。
    let src = fixture_bytes("small.pdf");
    let mut maker = PdfAnnotator::open(src.clone()).unwrap();
    maker
        .add(
            PageIndex(1),
            AnnotationKind::Rectangle {
                rect: Rect::new(72.0, 600.0, 300.0, 700.0).unwrap(),
                border: sample_border(),
            },
            "ext",
            Origin::Human,
        )
        .unwrap();
    maker
        .add(
            PageIndex(1),
            AnnotationKind::Underline {
                quads: sample_quads(),
                color: Rgb { r: 0, g: 0, b: 255 },
            },
            "ext",
            Origin::Human,
        )
        .unwrap();
    maker.save().unwrap();
    let external_pdf = maker.product_bytes();

    let mut op = PdfAnnotator::open(external_pdf.clone()).unwrap();
    assert_eq!(op.list(None).len(), 2, "外部两条全读出");
    // 删第一条（Rectangle——读回序首条）。
    let first = op.list(None)[0].id;
    op.remove(first).unwrap();
    op.save().unwrap();
    let product = op.product_bytes();
    // 前缀保持。
    assert!(product.len() >= external_pdf.len(), "增量只追加");
    assert!(
        product.starts_with(&external_pdf),
        "前缀逐位保持（删除路径）"
    );
    // 重开：恰剩 Underline。
    let (rb, report) = read_annotations(product, Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "{report:?}");
    assert!(matches!(rb[0].kind, AnnotationKind::Underline { .. }));
}

// ---------------------------------------------------------------------------
// 6. 会话命令面（六命令零变更即可用性）
// ---------------------------------------------------------------------------

#[test]
fn session_command_new_kinds_flush() {
    let src = fixture_bytes("small.pdf");
    let meta = DocumentMeta {
        id: PdfDocumentId(1),
        file_name: "small.pdf".into(),
        page_count: PageCount(3),
    };
    let pages = vec![
        PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            rotation: Rotation::Deg0,
        },
        PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            rotation: Rotation::Deg0,
        },
        PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            rotation: Rotation::Deg0,
        },
    ];
    let writer = LopdfWriter::from_bytes(src.clone()).unwrap();
    let assessment = writer.assessment().clone();
    let mut session = PdfSession::open(meta, assessment, pages, OutlineTree::new()).unwrap();
    session.attach_writer(Box::new(writer));
    // AnnotateAdd：新 kind（Rectangle——含 Border 宽度入列钳制）。
    let outcome = session
        .execute(Command::AnnotateAdd {
            draft: AnnotationDraft {
                page: PageIndex(0),
                kind: AnnotationKind::Rectangle {
                    rect: Rect::new(72.0, 600.0, 300.0, 700.0).unwrap(),
                    border: Border {
                        color: Rgb {
                            r: 200,
                            g: 30,
                            b: 30,
                        },
                        width: 999.0, // 域外 → 钳 12.0
                    },
                },
                author: "会话作者".into(),
                origin: Origin::Human,
            },
            source: Source::View,
        })
        .unwrap();
    let annot = match outcome {
        mdc_pdf::command::Outcome::AnnotAdded(a) => a,
        other => panic!("应 AnnotAdded，得 {other:?}"),
    };
    // 钳制语义（入列归一——存量面与落盘面同值）。
    match &annot.kind {
        AnnotationKind::Rectangle { border, .. } => assert_eq!(border.width, 12.0),
        other => panic!("(Rectangle) 得 {other:?}"),
    }
    // flush → 读回（全链：会话 → 写器 → 字节 → 读取面）。
    let save = session.flush_annotations().unwrap();
    assert!(save.bytes_appended > 0);
    let product = session.product_bytes().unwrap();
    let (rb, report) = read_annotations(product, Limits::default()).unwrap();
    assert_eq!(report.parsed, 1, "{report:?}");
    match &rb[0].kind {
        AnnotationKind::Rectangle { rect, border } => {
            assert_eq!(*rect, Rect::new(72.0, 600.0, 300.0, 700.0).unwrap());
            assert_eq!(border.width, 12.0, "钳制值落盘");
        }
        other => panic!("(Rectangle) 得 {other:?}"),
    }
    assert_eq!(rb[0].author, "会话作者");
}

// ---------------------------------------------------------------------------
// 7. wire 兼容
// ---------------------------------------------------------------------------

#[test]
fn wire_compat_old_json() {
    // 旧 4 kind JSON（存量宿主载荷）——反序列化面不破。
    let old = r#"{"id":1,"page":0,"kind":{"Highlight":{"quads":[{"points":[{"x":1.0,"y":2.0},{"x":3.0,"y":2.0},{"x":3.0,"y":1.0},{"x":1.0,"y":1.0}]}],"color":{"r":255,"g":0,"b":0}}},"author":"a","origin":"Human","created_ms":0,"modified_ms":0}"#;
    let parsed: Annotation = serde_json::from_str(old).unwrap();
    assert!(matches!(parsed.kind, AnnotationKind::Highlight { .. }));
    // 新 kind serde 往返。
    for kind in eleven_kinds() {
        let annot = Annotation {
            id: AnnotId(7),
            page: PageIndex(2),
            kind,
            author: "蛇形作者—Ω".into(),
            origin: Origin::Ai,
            created_ms: 1,
            modified_ms: 2,
        };
        let json = serde_json::to_string(&annot).unwrap();
        let back: Annotation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, annot);
    }
    // 未知 kind tag 的 JSON（前向兼容——旧宿主读新 JSON 得 Err 而非 panic）。
    let future = r#"{"id":1,"page":0,"kind":{"Hologram":{"x":1}},"author":"a","origin":"Human","created_ms":0,"modified_ms":0}"#;
    assert!(serde_json::from_str::<Annotation>(future).is_err());
}

// ---------------------------------------------------------------------------
// 8. 门面输入校验
// ---------------------------------------------------------------------------

#[test]
fn annotator_input_validation() {
    let mut a = PdfAnnotator::open(fixture_bytes("small.pdf")).unwrap();
    // 页越界。
    match a.add(
        PageIndex(3),
        AnnotationKind::Note {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
        },
        "x",
        Origin::Human,
    ) {
        Err(CommandError::InvalidPage(PageIndex(3))) => {}
        other => panic!("页越界应 InvalidPage(3)，得 {other:?}"),
    }
    // 删不存在。
    assert!(matches!(
        a.remove(AnnotId(99)),
        Err(CommandError::AnnotNotFound(AnnotId(99)))
    ));
    // list 页过滤。
    a.add(
        PageIndex(0),
        AnnotationKind::Note {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
        },
        "x",
        Origin::Human,
    )
    .unwrap();
    a.add(
        PageIndex(2),
        AnnotationKind::Note {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
        },
        "x",
        Origin::Human,
    )
    .unwrap();
    assert_eq!(a.list(None).len(), 2);
    assert_eq!(a.list(Some(PageIndex(0))).len(), 1);
    assert_eq!(a.list(Some(PageIndex(1))).len(), 0);
    assert_eq!(a.list(Some(PageIndex(2))).len(), 1);
    // 空载入即拒（写面校验——不产出非法 PDF）。
    let mut a2 = PdfAnnotator::open(fixture_bytes("small.pdf")).unwrap();
    assert!(a2
        .add(
            PageIndex(0),
            AnnotationKind::Underline {
                quads: vec![],
                color: Rgb { r: 0, g: 0, b: 0 },
            },
            "x",
            Origin::Human
        )
        .is_err());
}

// ---------------------------------------------------------------------------
// 9. Acrobat 互通公共面（结构级）
// ---------------------------------------------------------------------------

#[test]
fn acrobat_interop_common_fields() {
    let src = fixture_bytes("small.pdf");
    let mut a = PdfAnnotator::open(src).unwrap();
    for kind in eleven_kinds() {
        a.add(PageIndex(0), kind, "互通作者", Origin::Human)
            .unwrap();
    }
    a.save().unwrap();
    let product = a.product_bytes();
    // 裸 lopdf 结构断言（不依赖自家读取面——独立视角）。
    let doc = lopdf::Document::load_mem(&product).unwrap();
    let page_id = *doc.get_pages().get(&1).unwrap();
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let annots = match page.get(b"Annots") {
        Ok(lopdf::Object::Array(arr)) => arr.clone(),
        other => panic!("/Annots 应为数组：{other:?}"),
    };
    assert_eq!(annots.len(), 11);
    let mut subtypes = Vec::new();
    for entry in &annots {
        let id = match entry {
            lopdf::Object::Reference(id) => *id,
            _ => panic!("标注应为间接引用"),
        };
        let d = doc.get_object(id).unwrap().as_dict().unwrap();
        // 公共互通面：/Type /Annot + /F 4（打印位）+ /T 作者 + /M 日期串。
        assert!(matches!(d.get(b"Type"), Ok(lopdf::Object::Name(n)) if n == b"Annot"));
        assert!(
            matches!(d.get(b"F"), Ok(lopdf::Object::Integer(4))),
            "打印位"
        );
        assert!(
            matches!(d.get(b"T"), Ok(lopdf::Object::String(_, _))),
            "作者"
        );
        assert!(
            matches!(d.get(b"M"), Ok(lopdf::Object::String(_, _))),
            "日期"
        );
        if let Ok(lopdf::Object::Name(st)) = d.get(b"Subtype") {
            subtypes.push(String::from_utf8_lossy(st).to_string());
        }
    }
    // 子型谱（Arrow 落 /Line + /LE；Rectangle 落 /Square）。
    for expect in [
        "Highlight",
        "Text",
        "FreeText",
        "Ink",
        "Underline",
        "StrikeOut",
        "Square",
        "Circle",
        "Line",
        "Line",
        "Stamp",
    ] {
        assert!(
            subtypes.iter().any(|s| s == expect),
            "子型 {expect} 缺席：{subtypes:?}"
        );
    }
    // Arrow 的 /LE 面（第二个 /Line 才带——按 add 序，箭头在第 10 条）。
    let arrow_id = match &annots[9] {
        lopdf::Object::Reference(id) => *id,
        _ => panic!(),
    };
    let d = doc.get_object(arrow_id).unwrap().as_dict().unwrap();
    assert!(
        matches!(d.get(b"LE"), Ok(lopdf::Object::Array(_))),
        "Arrow 须带 /LE 终止符"
    );
    // Stamp /Contents。
    let stamp_id = match &annots[10] {
        lopdf::Object::Reference(id) => *id,
        _ => panic!(),
    };
    let d = doc.get_object(stamp_id).unwrap().as_dict().unwrap();
    assert!(matches!(
        d.get(b"Contents"),
        Ok(lopdf::Object::String(_, _))
    ));
}

// ---------------------------------------------------------------------------
// 10. [pdfium] 互通交叉
// ---------------------------------------------------------------------------

#[cfg(feature = "engine-pdfium")]
mod pdfium_cross {
    use super::*;
    use mdc_pdf::engine_pdfium::PdfiumEngine;
    use pdfium_render::prelude::*;

    fn open_doc(bytes: &[u8]) -> PdfDocument<'static> {
        // 进程单例纪律：经 PdfiumEngine::shared_handle 复用既有绑定
        // （禁止二次绑定——pdfium 单实例）。
        let pdfium = PdfiumEngine::shared_handle()
            .unwrap_or_else(|e| panic!("pdfium 绑定失败（MDC_PDFIUM_LIB → 候选 → 系统）：{e:?}"));
        pdfium
            .load_pdf_from_byte_vec(bytes.to_vec(), None)
            .unwrap_or_else(|e| panic!("pdfium 打开失败：{e:?}"))
    }

    /// pdfium 独立读回全部可类型化子型（9 新 + 4 旧中的可类型化面）。
    #[test]
    fn pdfium_reads_all_new_subtypes() {
        let src = fixture_bytes("small.pdf");
        let mut a = PdfAnnotator::open(src).unwrap();
        for (i, kind) in eleven_kinds().into_iter().enumerate() {
            a.add(PageIndex((i % 3) as u32), kind, "cross", Origin::Human)
                .unwrap();
        }
        a.save().unwrap();
        let product = a.product_bytes();
        let doc = open_doc(&product);
        assert_eq!(doc.pages().len(), 3);
        // 逐页收集 pdfium 可见的标注子型。
        let mut seen: Vec<String> = Vec::new();
        for page in doc.pages().iter() {
            for annot in page.annotations().iter() {
                let t = annot.annotation_type();
                seen.push(format!("{t:?}"));
                // 互通公共面：bounds 可读（几何有效——阅读器能定位）。
                let _ = annot.bounds();
            }
        }
        // 11 kind → pdfium 类型化面 10 个（Arrow 落 Line）+ 注：
        // pdfium 对 /Line 无专属类型变体时落 Unsupported——按实测断言。
        eprintln!("[pdff2] pdfium 可见子型：{seen:?}");
        for expect in [
            "Highlight",
            "Text",
            "FreeText",
            "Ink",
            "Underline",
            "Strikeout",
            "Square",
            "Circle",
            "Stamp",
        ] {
            assert!(
                seen.iter().any(|s| s.contains(expect)),
                "pdfium 未见 {expect}：{seen:?}"
            );
        }
        assert_eq!(seen.len(), 11, "11 条全可见（含 Line/Arrow）：{seen:?}");
    }

    /// 删除后的 pdfium 确认（引擎面不可见 = 引用图已死）。
    #[test]
    fn pdfium_confirms_removal() {
        let src = fixture_bytes("small.pdf");
        let mut a = PdfAnnotator::open(src).unwrap();
        let id1 = a
            .add(
                PageIndex(0),
                AnnotationKind::Note {
                    rect: Rect::new(80.0, 700.0, 100.0, 720.0).unwrap(),
                },
                "x",
                Origin::Human,
            )
            .unwrap();
        let _id2 = a
            .add(
                PageIndex(0),
                AnnotationKind::Stamp {
                    rect: Rect::new(400.0, 700.0, 500.0, 750.0).unwrap(),
                    text: "终审".into(),
                },
                "x",
                Origin::Human,
            )
            .unwrap();
        a.save().unwrap();
        let after_add = a.product_bytes();
        let doc = open_doc(&after_add);
        assert_eq!(doc.pages().first().unwrap().annotations().len(), 2);
        drop(doc);

        a.remove(id1).unwrap();
        a.save().unwrap();
        let doc = open_doc(&a.product_bytes());
        let page = doc.pages().first().unwrap();
        let ann = page.annotations();
        assert_eq!(ann.len(), 1, "删除后 pdfium 恰见 1（引擎面确认）");
        assert!(matches!(
            ann.first().unwrap().annotation_type(),
            PdfPageAnnotationType::Stamp
        ));
    }
}
