//! PDF-d — lopdf 标注增量写语义锁（feature `engine-lopdf` + `engine-pdfium`
//! 双引擎面；语义基线 = PDF-a spike 探针 3：增量写 → 前缀保持 → lopdf
//! 重载 → pdfium 交叉读回）。
//!
//! **环境合同（不静默 skip，与 pdfc_engine 同纪律）**：库定位走
//! env `MDC_PDFIUM_LIB` → 编译期候选清单（spike 取件位）→ 系统；
//! fixtures 走 env `MDC_PDF_TEST_FIXTURES` → 编译期候选。任一缺失 =
//! panic 打印全部已尝试候选。
//!
//! 锁清单（每锁一条 `#[test]`；①-⑭ = 任务面约定 + ⑮ 增补）：
//! 1. `four_kinds_roundtrip_field_exact`——四 kind 经 flush → lopdf 重载
//!    /Annots：subtype / Rect / T / Contents / QuadPoints / InkList 逐字段
//!    与模型相等（f32 逐位往返）；
//! 2. `original_prefix_bitwise_preserved`——原字节前缀逐位保持（产物
//!    前缀 == 原 small.pdf 字节；两轮 flush 后仍成立——增量链传递）；
//! 3. `pdfium_cross_read_subtypes_and_author`——pdfium 交叉读回（互通
//!    引擎层预证）：四种 subtype 可读 + /T（creator）可读；
//! 4. `origin_roundtrip_ai_suffix_reversible`——Origin 往返：Human 原文 /
//!    Ai `" [ai]"` 后缀可逆还原（含非 ASCII 作者 = UTF-16BE 文本串路径）；
//! 5. `modification_date_pdf_string`——/M = `pdf_date_string(modified_ms)`
//!    （纯函数 + 值时钟：已知时钟值 → 字面日期串）；
//! 6. `encrypted_from_bytes_denied_and_flush_precedes_writer`——加密档
//!    双防线：`from_bytes` 拒（引擎侧独立防线，仍加密/曾加密两形态）+
//!    会话加密态 flush 拒（S5 先于写器——产物零追加）；
//! 7. `idempotent_flush_zero_append`——幂等 flush：二次 flush 零脏 →
//!    bytes_appended = 0 且产物长度不变；
//! 8. `edit_redirty_no_duplicate_reflow`——编辑后重脏：AnnotateEdit →
//!    flush 追加且重载可见新值（同对象号重定义——无重复标注）；
//! 9. `highlight_quad_and_ink_stroke_envelopes`——包络 Rect：Highlight =
//!    quad 包络 / Ink = stroke 包络（Y 轴用户空间直通）；
//! 10. `garbage_bytes_rejected`——垃圾字节/空/截断 → `from_bytes` Err
//!     （typed Engine，不 panic）；
//! 11. `writer_without_engine_session_end_to_end`——写器无引擎会话共存
//!     （attach_writer 不要求 attach_engine：真实写器端到端）；
//! 12. `second_flush_writes_only_dirty_revision`——脏集精确性（真写器
//!     面）：首轮全量修订 > 二轮编辑修订（只写脏对象）+ 无重复标注；
//! 13. `freetext_da_default_appearance`——FreeText /DA 存在且 =
//!     `/Helv 12 Tf 0 g`（Acrobat 免 AP 渲染前提）；
//! 14. `bulk_100_annotations_smoke`——大量标注冒烟：100 条混合 kind 单轮
//!     flush + 重载全数还原（计数 + subtype 逐条）；
//! 15. `mount_preserves_existing_page_annots`——既有 /Annots 挂载面：
//!     form_probe.pdf（/AcroForm widget）上追加不破坏既有标注。

#![cfg(all(feature = "engine-lopdf", feature = "engine-pdfium"))]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use mdc_pdf::annotation::{
    AnnotId, Annotation, AnnotationDraft, AnnotationKind, AnnotationPatch, Origin, Point, Quad,
    Rect, Rgb,
};
use mdc_pdf::command::{Command, CommandError, Outcome, Source};
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageCount, PageGeometry, PageIndex, Rotation};
use mdc_pdf::security::{OpenAssessment, REASON_ENCRYPTED_WRITE};
use mdc_pdf::writer::{decode_author, encode_author, pdf_date_string, AnnotationWriter};
use mdc_pdf::writer_lopdf::{lopdf, LopdfWriter, FREETEXT_DEFAULT_DA};

use lopdf::Object;
use pdfium_render::prelude::*;

// ---------------------------------------------------------------------------
// 环境合同：库 + fixtures（与 pdfc_engine 同款定位序）
// ---------------------------------------------------------------------------

const FIXTURES_ENV: &str = "MDC_PDF_TEST_FIXTURES";
const PDFIUM_LIB_ENV: &str = "MDC_PDFIUM_LIB";

fn fixture_candidates() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // crate 本地优先 / spike 兜底（顺序依据见 pdfc_engine.rs——T1 审计 F12）。
    vec![
        manifest.join("fixtures"),
        manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/fixtures"),
    ]
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let mut tried: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var(FIXTURES_ENV) {
        dirs.push(PathBuf::from(dir));
    }
    dirs.extend(fixture_candidates());
    for dir in &dirs {
        let path = dir.join(name);
        match std::fs::read(&path) {
            Ok(bytes) => return bytes,
            Err(why) => tried.push(format!("{} → {why}", path.display())),
        }
    }
    panic!(
        "fixture {name} 定位失败（定位序：{FIXTURES_ENV} → 编译期候选）；已尝试：{}",
        tried.join("；")
    );
}

/// 进程级 pdfium 实例（测试二进制内单绑定；与 pdfc_engine 二进制互相
/// 独立——无跨文件绑定竞争）。
static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

fn pdfium() -> &'static Pdfium {
    PDFIUM.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(path) = std::env::var(PDFIUM_LIB_ENV) {
            candidates.push(PathBuf::from(path));
        }
        candidates.extend([
            manifest.join("../../../cleanroom-analysis/md-line/pdf-a/spike/bin/lib"), // spike 取件位
            manifest.join("../../bin/lib"), // 产品仓预留位
            manifest.join("bin/lib"),       // crate 本地
        ]);
        for dir in candidates {
            let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
            if let Ok(bindings) = Pdfium::bind_to_library(&path) {
                return Pdfium::new(bindings);
            }
        }
        // 系统兜底（bind_to_system_library）——失败 = panic（环境合同：
        // 不静默 skip）。
        let sys = Pdfium::bind_to_system_library()
            .expect("libpdfium 定位失败（定位序：MDC_PDFIUM_LIB → 候选 → 系统）");
        Pdfium::new(sys)
    })
}

// ---------------------------------------------------------------------------
// 装配：会话 + 共享写器（Arc<Mutex<LopdfWriter>> 包装——flush 后仍可
// 经观测柄取产物；宿主保存事务同款形态）
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct SharedLopdf(Arc<Mutex<LopdfWriter>>);

impl AnnotationWriter for SharedLopdf {
    fn write_annotation(
        &mut self,
        page: PageIndex,
        annot: &Annotation,
    ) -> Result<(), CommandError> {
        self.0.lock().unwrap().write_annotation(page, annot)
    }
    fn save_incremental(&mut self) -> Result<mdc_pdf::writer::IncrementalSave, CommandError> {
        self.0.lock().unwrap().save_incremental()
    }
    // p5-t4 加性 trait 法（产物取回面）：测试包装器同源透传——
    // 本文件 10-23 起在 engine-lopdf 特性下编译断裂（trait 法入库
    // 时仅同步三实现漏本 impl，特性门同期未跑=覆盖回归，本轮复验
    // 暴露）。
    fn product_bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().product_bytes()
    }
}

/// 装配会话（页几何占位——写路径不消费几何；时钟注入使 /M 确定）。
fn session_with_writer(
    page_count: u32,
    original: Vec<u8>,
    encrypted: bool,
) -> (PdfSession, SharedLopdf) {
    let writer = LopdfWriter::from_bytes(original).expect("写器构造（fixture 合法）");
    let shared = SharedLopdf(Arc::new(Mutex::new(writer)));
    let mut s = PdfSession::open(
        DocumentMeta {
            id: PdfDocumentId(1),
            file_name: "small.pdf".into(),
            page_count: PageCount(page_count),
        },
        OpenAssessment {
            encrypted,
            warnings: vec![],
            form_probe: None,
        },
        vec![
            PageGeometry {
                width_pt: 612.0,
                height_pt: 792.0,
                rotation: Rotation::Deg0,
            };
            page_count as usize
        ],
        OutlineTree::new(),
    )
    .expect("会话装配自洽");
    s.set_clock_ms(1_700_000_000_000);
    s.attach_writer(Box::new(shared.clone()));
    (s, shared)
}

fn add(
    s: &mut PdfSession,
    page: PageIndex,
    kind: AnnotationKind,
    author: &str,
    origin: Origin,
) -> AnnotId {
    let Outcome::AnnotAdded(a) = s
        .execute(Command::AnnotateAdd {
            draft: AnnotationDraft {
                page,
                kind,
                author: author.into(),
                origin,
            },
            source: match origin {
                Origin::Human => Source::View,
                Origin::Ai => Source::Ai,
            },
        })
        .expect("AnnotateAdd 成功")
    else {
        unreachable!("AnnotateAdd 载荷形状");
    };
    a.id
}

fn quad(x0: f32, y0: f32, x1: f32, y1: f32) -> Quad {
    Quad {
        points: [
            Point { x: x0, y: y1 },
            Point { x: x1, y: y1 },
            Point { x: x0, y: y0 },
            Point { x: x1, y: y0 },
        ],
    }
}

// ---------------------------------------------------------------------------
// lopdf 重载解析辅助
// ---------------------------------------------------------------------------

/// lopdf 重载产物（全内存）。
fn reload(product: &[u8]) -> lopdf::Document {
    lopdf::Document::load_mem(product).expect("产物重载（写路径产出必须自洽）")
}

/// 页 /Annots 字典列表（引用解析；1 基页号）。
fn page_annots(doc: &lopdf::Document, page_no: u32) -> Vec<&lopdf::Dictionary> {
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .unwrap_or_else(|| panic!("页 {page_no} 在档"));
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    match page.get(b"Annots") {
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let id = o.as_reference().expect("/Annots 项为间接引用");
                doc.get_object(id).unwrap().as_dict().unwrap()
            })
            .collect(),
        Ok(arr) => arr
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let id = o.as_reference().expect("/Annots 项为间接引用");
                doc.get_object(id).unwrap().as_dict().unwrap()
            })
            .collect(),
        Err(_) => vec![],
    }
}

/// 按 /Subtype 取标注字典（页内唯一时用）。
fn find_annot<'a>(doc: &'a lopdf::Document, page_no: u32, subtype: &[u8]) -> &'a lopdf::Dictionary {
    page_annots(doc, page_no)
        .into_iter()
        .find(|d| d.get(b"Subtype").unwrap().as_name().unwrap() == subtype)
        .unwrap_or_else(|| {
            panic!(
                "页 {page_no} 未找到 /Subtype {}",
                String::from_utf8_lossy(subtype)
            )
        })
}

/// 对象 → f32（Integer/Real 双面——`as_float`：整数写出的坐标（如
/// `72`）与实数（`72.5`）同归 f32）。
fn f32_of(obj: &Object) -> f32 {
    obj.as_float().unwrap_or_else(|e| panic!("数值对象：{e}"))
}

/// 文本字符串对象 → Rust String（ASCII 字面量 / UTF-16BE BOM 双面）。
fn text_of(obj: &Object) -> String {
    let bytes = match obj {
        Object::String(bytes, _) => bytes,
        other => panic!("文本字符串对象（得 {other:?}）"),
    };
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16(&units).expect("UTF-16BE 合法")
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// 数组对象按 f32 收集。
fn f32_array(obj: &Object) -> Vec<f32> {
    obj.as_array().unwrap().iter().map(f32_of).collect()
}

/// Rect 断言（4 元数组 == 模型值逐位）。
fn assert_rect(dict: &lopdf::Dictionary, key: &[u8], expect: [f32; 4], why: &str) {
    let got = f32_array(dict.get(key).unwrap());
    assert_eq!(got, expect.to_vec(), "{key:?} 逐位往返（{why}）");
}

// ---------------------------------------------------------------------------
// 1：四 kind 全字段往返
// ---------------------------------------------------------------------------

/// 四 kind 模型面（锁 1/3/12/14 共用底稿）。
fn four_kinds(s: &mut PdfSession) {
    add(
        s,
        PageIndex(0),
        AnnotationKind::Highlight {
            quads: vec![
                quad(72.0, 700.5, 300.25, 716.0),
                quad(72.0, 680.0, 220.75, 696.0),
            ],
            color: Rgb {
                r: 255,
                g: 128,
                b: 0,
            },
        },
        "human-author",
        Origin::Human,
    );
    add(
        s,
        PageIndex(0),
        AnnotationKind::Note {
            rect: Rect::new(100.0, 600.0, 130.5, 630.0).unwrap(),
        },
        "note-taker",
        Origin::Human,
    );
    add(
        s,
        PageIndex(0),
        AnnotationKind::FreeText {
            rect: Rect::new(72.0, 500.0, 372.25, 530.0).unwrap(),
            text: "free text payload".into(),
        },
        "ai-agent",
        Origin::Ai,
    );
    add(
        s,
        PageIndex(2),
        AnnotationKind::Ink {
            strokes: vec![
                vec![
                    Point { x: 10.5, y: 20.25 },
                    Point { x: 30.75, y: 40.0 },
                    Point { x: 50.0, y: 60.5 },
                ],
                vec![Point { x: 90.0, y: 12.5 }],
            ],
            // #45：红 4pt（issue 验收口径——/C=[1,0,0]、/BS/W=4）。
            color: Rgb { r: 255, g: 0, b: 0 },
            width: 4.0,
        },
        "inker",
        Origin::Ai,
    );
}

#[test]
fn four_kinds_roundtrip_field_exact() {
    // 四 kind 经会话 flush → lopdf 重载 /Annots：subtype/Rect/T/Contents/
    // QuadPoints/InkList 逐字段与模型相等（f32 Display 最短表示 = 往返
    // 逐位保真；任何字段漂移 = 落盘面语义破坏）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    four_kinds(&mut s);
    let stats = s.flush_annotations().expect("flush 四 kind");
    assert!(stats.bytes_appended > 0, "非空修订必然追加");
    let product = shared.0.lock().unwrap().product_bytes();
    let doc = reload(&product);

    // 页 0 = 3 条；页 2 = 1 条。
    assert_eq!(
        page_annots(&doc, 1).len(),
        3,
        "页 1（0 基 0）= H+Note+FreeText"
    );
    assert_eq!(page_annots(&doc, 2).len(), 0, "页 1（0 基 1）无标注");
    assert_eq!(page_annots(&doc, 3).len(), 1, "页 2（0 基 2）= Ink");

    // Highlight：QuadPoints 16 数（2 quad × 8）逐位 + 颜色 + 包络 Rect。
    let hl = find_annot(&doc, 1, b"Highlight");
    let expect_qp: Vec<f32> = [
        72.0, 716.0, 300.25, 716.0, 72.0, 700.5, 300.25, 700.5, // quad 1（环序）
        72.0, 696.0, 220.75, 696.0, 72.0, 680.0, 220.75, 680.0, // quad 2
    ]
    .to_vec();
    assert_eq!(
        f32_array(hl.get(b"QuadPoints").unwrap()),
        expect_qp,
        "QuadPoints 逐位往返"
    );
    assert_rect(
        hl,
        b"Rect",
        [72.0, 680.0, 300.25, 716.0],
        "Highlight /Rect = quad 包络",
    );
    let c = f32_array(hl.get(b"C").unwrap());
    assert_eq!(c, vec![1.0, 128.0 / 255.0, 0.0], "/C 0-1 归一（255,128,0）");
    assert_eq!(
        text_of(hl.get(b"T").unwrap()),
        "human-author",
        "/T Human 原文"
    );
    assert_eq!(f32_of(hl.get(b"F").unwrap()), 4.0, "/F 打印位");

    // Note：/Text + /Rect 直通。
    let note = find_annot(&doc, 1, b"Text");
    assert_rect(
        note,
        b"Rect",
        [100.0, 600.0, 130.5, 630.0],
        "Note /Rect 模型直通",
    );
    assert_eq!(text_of(note.get(b"T").unwrap()), "note-taker");

    // FreeText：/Contents + /DA + /Rect。
    let ft = find_annot(&doc, 1, b"FreeText");
    assert_eq!(
        text_of(ft.get(b"Contents").unwrap()),
        "free text payload",
        "/Contents 往返"
    );
    assert_rect(
        ft,
        b"Rect",
        [72.0, 500.0, 372.25, 530.0],
        "FreeText /Rect 模型直通",
    );
    assert_eq!(
        text_of(ft.get(b"T").unwrap()),
        "ai-agent [ai]",
        "/T Ai 后缀"
    );

    // Ink：/InkList 两划（3 点 + 1 点）逐位 + /BS 边框。
    let ink = find_annot(&doc, 3, b"Ink");
    let ink_list = ink.get(b"InkList").unwrap().as_array().unwrap();
    assert_eq!(ink_list.len(), 2, "笔划数保持");
    assert_eq!(
        f32_array(&ink_list[0]),
        vec![10.5, 20.25, 30.75, 40.0, 50.0, 60.5],
        "笔划 1 逐位往返"
    );
    assert_eq!(f32_array(&ink_list[1]), vec![90.0, 12.5], "笔划 2 逐位往返");
    let bs = ink.get(b"BS").unwrap().as_dict().unwrap();
    // #45：W 从写死 1pt 改为载荷宽（Real 面——4pt 逐位）。
    assert_eq!(f32_of(bs.get(b"W").unwrap()), 4.0, "/BS /W 载荷宽 4pt");
    assert_eq!(
        bs.get(b"S").unwrap().as_name().unwrap(),
        b"S",
        "/BS /S 实线"
    );
    // #45：/C 颜色（红 255,0,0 → 0-1 归一 [1,0,0]）。
    assert_eq!(
        f32_array(ink.get(b"C").unwrap()),
        vec![1.0, 0.0, 0.0],
        "/C 0-1 归一（255,0,0）"
    );
}

// ---------------------------------------------------------------------------
// 2：原字节前缀逐位保持
// ---------------------------------------------------------------------------

#[test]
fn original_prefix_bitwise_preserved() {
    // 增量语义核心（CONTRACT §6 S5 在档承诺）：产物以**原文件字节**为
    // 逐位相同前缀——签名文档友好性。两轮 flush 后仍成立（修订链只追加
    // 不改写——传递性）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    four_kinds(&mut s);
    s.flush_annotations().expect("首轮 flush");
    // 编辑 + 新增 → 二轮修订。
    let Outcome::AnnotEdited(_) = s
        .execute(Command::AnnotateEdit {
            id: AnnotId(3),
            patch: AnnotationPatch {
                contents: Some("edited".into()),
                ..Default::default()
            },
            source: Source::Ai,
        })
        .unwrap()
    else {
        unreachable!()
    };
    add(
        &mut s,
        PageIndex(1),
        AnnotationKind::Note {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0).unwrap(),
        },
        "second-round",
        Origin::Human,
    );
    s.flush_annotations().expect("二轮 flush");
    let product = shared.0.lock().unwrap().product_bytes();
    assert!(
        product.len() > original.len(),
        "两轮修订必然增长（{} > {}）",
        product.len(),
        original.len()
    );
    assert_eq!(
        &product[..original.len()],
        &original[..],
        "原字节前缀逐位保持（两轮修订后——增量链传递性）"
    );
    // 产物自身可重载（自洽性）。
    let doc = reload(&product);
    assert_eq!(doc.get_pages().len(), 3, "页数不变（修订不改结构）");
}

// ---------------------------------------------------------------------------
// 3：pdfium 交叉读回（互通引擎层预证）
// ---------------------------------------------------------------------------

#[test]
fn pdfium_cross_read_subtypes_and_author() {
    // Acrobat 互通的引擎层预证：lopdf 写 → pdfium 读。四种 subtype 可读
    // + /T（pdfium creator 面 = FPDFAnnot /T）可读。GUI Acrobat 人工
    // 验证 = 开发环境轮事项（gates.md 诚实登记，不冒认）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    four_kinds(&mut s);
    s.flush_annotations().expect("flush");
    let product = shared.0.lock().unwrap().product_bytes();

    let doc = pdfium()
        .load_pdf_from_byte_vec(product, None)
        .expect("pdfium 装载产物");
    let page0 = doc.pages().first().expect("首页");
    let anns = page0.annotations();
    assert_eq!(anns.len(), 3, "pdfium 读到页 0 三条标注（互通）");
    let mut seen = Vec::new();
    for ann in anns.iter() {
        seen.push((ann.annotation_type(), ann.creator()));
    }
    seen.sort_by_key(|(t, _)| format!("{t:?}"));
    let mut expect = vec![
        (
            PdfPageAnnotationType::Highlight,
            Some("human-author".to_string()),
        ),
        (PdfPageAnnotationType::Text, Some("note-taker".to_string())),
        (
            PdfPageAnnotationType::FreeText,
            Some("ai-agent [ai]".to_string()),
        ),
    ];
    expect.sort_by_key(|(t, _)| format!("{t:?}"));
    assert_eq!(
        seen, expect,
        "页 0 三 subtype + /T 全对上（lopdf 写 → pdfium 读）"
    );

    let page2 = doc.pages().get(2).expect("第三页");
    let ink_anns = page2.annotations();
    assert_eq!(ink_anns.len(), 1, "pdfium 读到页 2 一条 Ink");
    assert_eq!(
        ink_anns.first().unwrap().annotation_type(),
        PdfPageAnnotationType::Ink
    );
    assert_eq!(
        ink_anns.first().unwrap().creator().as_deref(),
        Some("inker [ai]"),
        "Ink /T 可读（Ai 后缀）"
    );
    // FreeText /Contents 亦经 pdfium 可读（Acrobat 侧文本面）。
    let ft = page0
        .annotations()
        .iter()
        .find(|a| a.annotation_type() == PdfPageAnnotationType::FreeText)
        .expect("FreeText 在页 0");
    assert_eq!(ft.contents().as_deref(), Some("free text payload"));
}

// ---------------------------------------------------------------------------
// 4：Origin 往返（含非 ASCII 作者 = UTF-16BE 路径）
// ---------------------------------------------------------------------------

#[test]
fn origin_roundtrip_ai_suffix_reversible() {
    // /T origin 编码约定可逆：Human 原文 / Ai " [ai]" 后缀；重载后
    // decode_author 还原模型 (author, origin)。非 ASCII 作者走 UTF-16BE
    // 文本串（PDF 32000 7.9.2——Acrobat 系阅读器稳妥形态）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    add(
        &mut s,
        PageIndex(0),
        AnnotationKind::Note {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0).unwrap(),
        },
        "中文人类",
        Origin::Human,
    );
    add(
        &mut s,
        PageIndex(0),
        AnnotationKind::Note {
            rect: Rect::new(50.0, 10.0, 80.0, 40.0).unwrap(),
        },
        "中文智能体",
        Origin::Ai,
    );
    s.flush_annotations().expect("flush");
    let product = shared.0.lock().unwrap().product_bytes();
    let doc = reload(&product);
    let annots = page_annots(&doc, 1);
    assert_eq!(annots.len(), 2);
    let mut by_t: Vec<String> = annots
        .iter()
        .map(|d| text_of(d.get(b"T").unwrap()))
        .collect();
    by_t.sort();
    assert_eq!(
        by_t,
        vec!["中文人类".to_string(), "中文智能体 [ai]".to_string()],
        "非 ASCII 作者（Human 原文 / Ai 后缀）经 UTF-16BE 往返"
    );
    // 可逆还原（decode_author = 编码逆）。
    for t in by_t {
        let (author, origin) = decode_author(&t);
        match origin {
            Origin::Human => assert_eq!(author, "中文人类"),
            Origin::Ai => assert_eq!(author, "中文智能体"),
        }
    }
    // 与 encode_author 闭环（模型 → /T → 模型）。
    assert_eq!(encode_author("中文智能体", Origin::Ai), "中文智能体 [ai]");
}

// ---------------------------------------------------------------------------
// 5：/M 日期串
// ---------------------------------------------------------------------------

#[test]
fn modification_date_pdf_string() {
    // /M = modified_ms 的 PDF 日期串（纯函数 + 值时钟：会话时钟
    // 1_700_000_000_000 → D:20231114221320+00'00'——与时钟注入值锁定）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    s.set_clock_ms(1_700_000_000_000);
    add(
        &mut s,
        PageIndex(0),
        AnnotationKind::Note {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0).unwrap(),
        },
        "dated",
        Origin::Human,
    );
    s.flush_annotations().expect("flush");
    let doc = reload(&shared.0.lock().unwrap().product_bytes());
    let note = find_annot(&doc, 1, b"Text");
    let m = text_of(note.get(b"M").unwrap());
    assert_eq!(
        m, "D:20231114221320+00'00'",
        "/M 与会话时钟逐字面锁定（值时钟语义——无系统时区）"
    );
    assert_eq!(
        m,
        pdf_date_string(1_700_000_000_000),
        "/M 与纯函数同源（单一出口）"
    );
}

// ---------------------------------------------------------------------------
// 6：加密双防线（from_bytes 引擎侧 + 会话 S5 先于写器）
// ---------------------------------------------------------------------------

/// 构造加密 fixture（lopdf V1 RC4 40-bit + 非空用户口令——装载为
/// 「仍加密」形态）。
fn encrypted_fixture() -> Vec<u8> {
    use lopdf::{dictionary, EncryptionVersion, Permissions};
    let mut doc = lopdf::Document::with_version("1.7");
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"mdc-pdf-test-id-0"),
            Object::string_literal(b"mdc-pdf-test-id-0"),
        ]),
    );
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Page".to_vec()),
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(612), Object::Integer(792)],
    });
    doc.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let state: lopdf::EncryptionState = EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner-secret",
        user_password: "user-secret",
        permissions: Permissions::all(),
    }
    .try_into()
    .expect("加密状态构造");
    doc.encrypt(&state).expect("对象加密");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("加密档保存");
    bytes
}

#[test]
fn encrypted_from_bytes_denied_and_flush_precedes_writer() {
    // 双防线：① from_bytes（引擎侧独立防线）——仍加密档拒（SecurityDeny
    // + S5 理由码，先于任何写入）；② 会话加密态 flush 拒——先于写器
    // 调度（真实写器产物零追加 = 零接触证据）。
    let enc = encrypted_fixture();
    // ① 引擎侧。
    match LopdfWriter::from_bytes(enc.clone()) {
        Err(CommandError::SecurityDeny { reason }) => {
            assert_eq!(
                reason, REASON_ENCRYPTED_WRITE,
                "S5 稳定理由码（与命令面同锚）"
            );
        }
        other => panic!("加密档 from_bytes 应 SecurityDeny（得 {other:?}）"),
    }
    // ② 会话侧：加密评估 + 真实写器在位 + flush → 拒且产物零变化。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), true); // encrypted
    let err = s.flush_annotations().expect_err("加密态 flush 必拒");
    assert_eq!(
        err,
        CommandError::SecurityDeny {
            reason: REASON_ENCRYPTED_WRITE
        },
        "会话 S5 拒（先于写器调度）"
    );
    assert_eq!(
        shared.0.lock().unwrap().product_bytes(),
        original,
        "写器产物零变化 = 写路径零接触"
    );
}

// ---------------------------------------------------------------------------
// 7：幂等 flush
// ---------------------------------------------------------------------------

#[test]
fn idempotent_flush_zero_append() {
    // 幂等：首轮 flush 追加 > 0；零脏二次 flush bytes_appended = 0 且
    // 产物长度不变（bytes_appended 最小化——空修订不落盘）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    four_kinds(&mut s);
    let first = s.flush_annotations().expect("首轮");
    assert!(first.bytes_appended > 0);
    let len_after_first = shared.0.lock().unwrap().product_bytes().len();
    let second = s.flush_annotations().expect("二次（零脏）");
    assert_eq!(second.bytes_appended, 0, "零脏 flush 零追加");
    assert_eq!(
        shared.0.lock().unwrap().product_bytes().len(),
        len_after_first,
        "产物长度不变（幂等）"
    );
    // 三轮同（稳定性）。
    let third = s.flush_annotations().expect("三轮");
    assert_eq!(third.bytes_appended, 0);
}

// ---------------------------------------------------------------------------
// 8：编辑后重脏（reflow 无重复）
// ---------------------------------------------------------------------------

#[test]
fn edit_redirty_no_duplicate_reflow() {
    // AnnotateEdit → 重脏 → flush 追加 → 重载可见新值。同对象号重定义
    // （增量更新语义）：/Annots 计数不增（无重复标注）+ /Contents 更新。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    add(
        &mut s,
        PageIndex(0),
        AnnotationKind::FreeText {
            rect: Rect::new(72.0, 500.0, 372.0, 530.0).unwrap(),
            text: "before edit".into(),
        },
        "editor",
        Origin::Human,
    );
    let first = s.flush_annotations().expect("首轮 flush");
    assert!(first.bytes_appended > 0);
    // 编辑（时钟推进——/M 应反映新时间）。
    s.set_clock_ms(1_700_000_000_000 + 60_000);
    let Outcome::AnnotEdited(_) = s
        .execute(Command::AnnotateEdit {
            id: AnnotId(1),
            patch: AnnotationPatch {
                contents: Some("after edit".into()),
                ..Default::default()
            },
            source: Source::View,
        })
        .unwrap()
    else {
        unreachable!()
    };
    let second = s.flush_annotations().expect("编辑后 flush");
    assert!(second.bytes_appended > 0, "重脏 flush 追加");
    let product = shared.0.lock().unwrap().product_bytes();
    assert_eq!(
        &product[..original.len()],
        &original[..],
        "编辑轮仍保持原字节前缀"
    );
    let doc = reload(&product);
    let annots = page_annots(&doc, 1);
    assert_eq!(annots.len(), 1, "无重复标注（同对象号重定义）");
    let ft = find_annot(&doc, 1, b"FreeText");
    assert_eq!(
        text_of(ft.get(b"Contents").unwrap()),
        "after edit",
        "重载可见新值"
    );
    assert_eq!(
        text_of(ft.get(b"M").unwrap()),
        pdf_date_string(1_700_000_000_000 + 60_000),
        "/M 反映编辑时间（modified_ms 推进）"
    );
}

// ---------------------------------------------------------------------------
// 9：包络 Rect（Highlight quad / Ink stroke）
// ---------------------------------------------------------------------------

#[test]
fn highlight_quad_and_ink_stroke_envelopes() {
    // /Rect 语义：Highlight = quad 包络 / Ink = stroke 包络（Y 轴 PDF
    // 用户空间直通——模型即用户空间坐标，无翻转）。
    let original = fixture_bytes("small.pdf");
    let mut writer = LopdfWriter::from_bytes(original.clone()).unwrap();
    // 直接使用面（不经会话——包络是写器纯映射面）。
    let hl = Annotation {
        id: AnnotId(1),
        page: PageIndex(0),
        kind: AnnotationKind::Highlight {
            // 乱序覆盖面的 quad（min/max 与点序无关）。
            quads: vec![
                quad(300.0, 680.0, 72.0, 716.0), // x/y 反序输入（包络归一）
                quad(72.0, 600.25, 250.5, 640.0),
            ],
            color: Rgb::default(),
        },
        author: "env".into(),
        origin: Origin::Human,
        created_ms: 0,
        modified_ms: 0,
    };
    let ink = Annotation {
        id: AnnotId(2),
        page: PageIndex(0),
        kind: AnnotationKind::Ink {
            strokes: vec![
                vec![
                    Point { x: 100.5, y: 300.0 },
                    Point {
                        x: 400.25,
                        y: 350.75,
                    },
                ],
                vec![
                    Point { x: 50.0, y: 200.5 },
                    Point {
                        x: 300.0,
                        y: 100.25,
                    },
                ],
            ],
            color: Rgb {
                r: 0x1a,
                g: 0x73,
                b: 0xe8,
            },
            width: 2.0,
        },
        author: "env".into(),
        origin: Origin::Human,
        created_ms: 0,
        modified_ms: 0,
    };
    writer.write_annotation(PageIndex(0), &hl).unwrap();
    writer.write_annotation(PageIndex(0), &ink).unwrap();
    writer.save_incremental().unwrap();
    let doc = reload(&writer.product_bytes());
    let hl_obj = find_annot(&doc, 1, b"Highlight");
    assert_rect(
        hl_obj,
        b"Rect",
        [72.0, 600.25, 300.0, 716.0],
        "Highlight 包络 = 全部 quad 角点 min/max（与点序无关）",
    );
    let ink_obj = find_annot(&doc, 1, b"Ink");
    assert_rect(
        ink_obj,
        b"Rect",
        [50.0, 100.25, 400.25, 350.75],
        "Ink 包络 = 全部笔划采样点 min/max",
    );
    // 幂等重写（同 id）→ 包络不变（重定义替换旧对象）。
    writer.write_annotation(PageIndex(0), &hl).unwrap();
    writer.save_incremental().unwrap();
    let doc2 = reload(&writer.product_bytes());
    assert_eq!(page_annots(&doc2, 1).len(), 2, "重写不产生重复标注");
    assert_rect(
        find_annot(&doc2, 1, b"Highlight"),
        b"Rect",
        [72.0, 600.25, 300.0, 716.0],
        "重写后包络恒定",
    );
}

// ---------------------------------------------------------------------------
// 10：垃圾字节
// ---------------------------------------------------------------------------

#[test]
fn garbage_bytes_rejected() {
    // typed 错误面：垃圾/空/截断 → Err（Engine），不 panic。
    for (label, bytes) in [
        ("纯文本", b"this is not a pdf at all".to_vec()),
        ("空字节", Vec::new()),
        ("截断头", b"%PDF-1.7\n%%EOF".to_vec()),
        (
            "二进制噪声",
            vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        ),
    ] {
        match LopdfWriter::from_bytes(bytes) {
            Err(CommandError::Engine(_)) => {} // typed 拒绝
            other => panic!("{label} 应 Engine 错误（得 {other:?}）"),
        }
    }
}

// ---------------------------------------------------------------------------
// 11：写器无引擎会话共存
// ---------------------------------------------------------------------------

#[test]
fn writer_without_engine_session_end_to_end() {
    // attach_writer 不要求 attach_engine：无引擎会话（注入占位几何）
    // + 真实写器 → 六命令照常 + flush 落盘 + 产物可重载。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    assert!(!s.engine_attached(), "前提：无引擎在位");
    // 命令面照常（AnnotateList 与 AnnotateAdd 混跑）。
    four_kinds(&mut s);
    let Outcome::AnnotListed(list) = s
        .execute(Command::AnnotateList {
            filter: Default::default(),
            source: Source::System,
        })
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(list.len(), 4, "命令面与写器共存无干扰");
    let stats = s.flush_annotations().expect("无引擎会话 flush");
    assert!(stats.bytes_appended > 0);
    let doc = reload(&shared.0.lock().unwrap().product_bytes());
    assert_eq!(
        page_annots(&doc, 1).len() + page_annots(&doc, 3).len(),
        4,
        "产物全数在位"
    );
}

// ---------------------------------------------------------------------------
// 12：二轮修订只写脏对象（脏集精确性——真写器面）
// ---------------------------------------------------------------------------

#[test]
fn second_flush_writes_only_dirty_revision() {
    // 脏集精确性的真写器证据：首轮 = 全量修订（4 标注）；二轮 = 仅
    // 1 编辑对象的重定义修订——追加量显著小于首轮（只写脏对象，不
    // 重复写已落盘对象）+ 重载计数精确（无重复）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    four_kinds(&mut s);
    let first = s.flush_annotations().expect("首轮");
    let Outcome::AnnotEdited(_) = s
        .execute(Command::AnnotateEdit {
            id: AnnotId(3), // FreeText（页 0）
            patch: AnnotationPatch {
                contents: Some("dirty-only".into()),
                ..Default::default()
            },
            source: Source::View,
        })
        .unwrap()
    else {
        unreachable!()
    };
    let second = s.flush_annotations().expect("二轮（仅编辑脏）");
    assert!(
        second.bytes_appended < first.bytes_appended,
        "二轮修订严格小于首轮（only-dirty：{} < {}；差值含页对象重定义 + xref/trailer 固定开销，比值面不硬锁）",
        second.bytes_appended,
        first.bytes_appended
    );
    let doc = reload(&shared.0.lock().unwrap().product_bytes());
    assert_eq!(
        page_annots(&doc, 1).len(),
        3,
        "页 0 计数精确（未动标注不重复）"
    );
    assert_eq!(page_annots(&doc, 3).len(), 1, "页 2 计数精确");
    let ft = find_annot(&doc, 1, b"FreeText");
    assert_eq!(text_of(ft.get(b"Contents").unwrap()), "dirty-only");
}

// ---------------------------------------------------------------------------
// 13：FreeText /DA（Acrobat 免 AP 渲染前提）
// ---------------------------------------------------------------------------

#[test]
fn freetext_da_default_appearance() {
    // FreeText /DA 存在且 = "/Helv 12 Tf 0 g"——Acrobat 对无 /AP 的
    // FreeText 按 /DA 免外观流渲染（v0 不生成 /AP 的互通前提）。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    add(
        &mut s,
        PageIndex(0),
        AnnotationKind::FreeText {
            rect: Rect::new(72.0, 500.0, 372.0, 530.0).unwrap(),
            text: "da check".into(),
        },
        "da",
        Origin::Human,
    );
    s.flush_annotations().expect("flush");
    let doc = reload(&shared.0.lock().unwrap().product_bytes());
    let ft = find_annot(&doc, 1, b"FreeText");
    assert_eq!(
        text_of(ft.get(b"DA").unwrap()),
        FREETEXT_DEFAULT_DA,
        "/DA 默认外观（常量单一出口）"
    );
    assert_eq!(FREETEXT_DEFAULT_DA, "/Helv 12 Tf 0 g", "词面冻结");
    // 无 /AP（v0 不生成——/DA 免外观流路径）。
    assert!(ft.get(b"AP").is_err(), "v0 不写 /AP（/DA 渲染路径）");
}

// ---------------------------------------------------------------------------
// 14：大量标注冒烟（100 条）
// ---------------------------------------------------------------------------

#[test]
fn bulk_100_annotations_smoke() {
    // 100 条混合 kind 单轮 flush + 重载全数还原：页计数 + subtype 逐条
    // （id i（1 基）：页 = (i-1)%3，kind = (i-1)%4，/T 含序号）。规模面
    // 冒烟——对象图/xref 链在百量级不漂移。
    let original = fixture_bytes("small.pdf");
    let (mut s, shared) = session_with_writer(3, original.clone(), false);
    for i in 0..100u64 {
        let kind = match i % 4 {
            0 => AnnotationKind::Highlight {
                quads: vec![quad(72.0 + i as f32, 700.0, 300.0 + i as f32, 716.0)],
                color: Rgb {
                    r: 255,
                    g: 255,
                    b: 0,
                },
            },
            1 => AnnotationKind::Note {
                rect: Rect::new(10.0, 10.0 + i as f32, 40.0, 40.0 + i as f32).unwrap(),
            },
            2 => AnnotationKind::FreeText {
                rect: Rect::new(50.0, 500.0, 350.0, 530.0).unwrap(),
                text: format!("bulk #{i}"),
            },
            _ => AnnotationKind::Ink {
                strokes: vec![vec![
                    Point { x: 10.0, y: 20.0 },
                    Point {
                        x: 30.0,
                        y: 40.0 + i as f32,
                    },
                ]],
                color: Rgb {
                    r: 0x1a,
                    g: 0x73,
                    b: 0xe8,
                },
                width: 2.0,
            },
        };
        add(
            &mut s,
            PageIndex((i % 3) as u32),
            kind,
            &format!("bulk-{i}"),
            Origin::Human,
        );
    }
    let stats = s.flush_annotations().expect("百条单轮 flush");
    assert!(stats.bytes_appended > 0);
    let product = shared.0.lock().unwrap().product_bytes();
    assert_eq!(
        &product[..original.len()],
        &original[..],
        "百条规模前缀仍保持"
    );
    let doc = reload(&product);
    let expect_subtype = |i: u64| match i % 4 {
        0 => "Highlight",
        1 => "Text",
        2 => "FreeText",
        _ => "Ink",
    };
    let mut total = 0usize;
    for page_no in 1..=3u32 {
        let annots = page_annots(&doc, page_no);
        // 页 p（0 基）条数 = |{i : i%3 == p}|（i ∈ 0..100）。
        let expect_count = (0..100).filter(|i| i % 3 == page_no - 1).count();
        assert_eq!(annots.len(), expect_count, "页 {page_no} 计数精确");
        // 写入序 = id 升序 → /Annots 序 = 页内 id 升序；id i+1 的 subtype
        // 与 /T 可全数对回。
        for (slot, dict) in annots.iter().enumerate() {
            let i = page_no - 1 + 3 * slot as u32; // 0 基模型序号
            assert_eq!(
                dict.get(b"Subtype").unwrap().as_name().unwrap(),
                expect_subtype(i as u64).as_bytes(),
                "页 {page_no} 槽 {slot}（模型 #{i}）subtype 对回"
            );
            assert_eq!(
                text_of(dict.get(b"T").unwrap()),
                format!("bulk-{i}"),
                "页 {page_no} 槽 {slot}（模型 #{i}）/T 对回"
            );
        }
        total += annots.len();
    }
    assert_eq!(total, 100, "全数还原");
}

// ---------------------------------------------------------------------------
// 15：既有 /Annots 挂载面（form_probe.pdf——AcroForm widget 在位）
// ---------------------------------------------------------------------------

#[test]
fn mount_preserves_existing_page_annots() {
    // 挂载不破坏既有标注：form_probe.pdf 页 /Annots 已有 /Widget（AcroForm
    // 文本域）——追加 Highlight 后：两条并存，widget 的 /FT /Tx 原样。
    let original = fixture_bytes("form_probe.pdf");
    let mut writer = LopdfWriter::from_bytes(original.clone()).unwrap();
    let hl = Annotation {
        id: AnnotId(1),
        page: PageIndex(0),
        kind: AnnotationKind::Highlight {
            quads: vec![quad(72.0, 100.0, 300.0, 116.0)],
            color: Rgb { r: 0, g: 255, b: 0 },
        },
        author: "guest".into(),
        origin: Origin::Human,
        created_ms: 0,
        modified_ms: 0,
    };
    writer.write_annotation(PageIndex(0), &hl).unwrap();
    writer.save_incremental().unwrap();
    let product = writer.product_bytes();
    assert_eq!(
        &product[..original.len()],
        &original[..],
        "form fixture 前缀保持"
    );
    let doc = reload(&product);
    let annots = page_annots(&doc, 1);
    assert_eq!(annots.len(), 2, "既有 widget + 新增 Highlight 并存");
    let widget = annots
        .iter()
        .find(|d| d.get(b"Subtype").unwrap().as_name().unwrap() == b"Widget")
        .expect("既有 widget 存活");
    assert_eq!(
        widget.get(b"FT").unwrap().as_name().unwrap(),
        b"Tx",
        "widget /FT /Tx 原样（挂载不破坏既有标注）"
    );
    // pdfium 也读到两条（既有 widget 面 + 新标注面）。
    let pdf = pdfium()
        .load_pdf_from_byte_vec(product, None)
        .expect("pdfium 装载");
    let page = pdf.pages().first().expect("首页");
    assert_eq!(page.annotations().len(), 2, "pdfium 视角两条并存");
}

// ---------------------------------------------------------------------------
// 16. 间接 /Annots 形态（pdf-d-1r 审计重要项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn indirect_annots_reference_form_appends_preserving_existing() {
    // 页 `/Annots 3 0 R`（指向独立数组对象）是 ISO 32000 合法且常见的
    // 第三形态（既有直挂数组 / 无 /Annots 之外）——首段实现有该分支但
    // 零锁覆盖（审计判定：功能正确但覆盖缺口）。本锁构造该形态：
    // 既有数组挂一条 Note（模拟宿主历史标注），追加两条后断言：
    // ①既有标注存活（不覆盖不破坏）；②新增按序追加；③前缀逐位保持。
    use lopdf::dictionary;

    // fixture 现场构造：small.pdf → 页 /Annots 改为间接数组引用（内含
    // 一条既有 /Text 标注）。
    let original = fixture_bytes("small.pdf");
    let mut doc = lopdf::Document::load_mem(&original).expect("基准档装载");
    let page_id = *doc.get_pages().get(&1).expect("首页");
    let existing_id = doc.add_object(dictionary! {
        "Type" => lopdf::Object::Name(b"Annot".to_vec()),
        "Subtype" => lopdf::Object::Name(b"Text".to_vec()),
        "Rect" => vec![lob_num(10), lob_num(10), lob_num(60), lob_num(60)],
        "T" => lob_str("prior-host"),
        "Contents" => lob_str("既有标注"),
    });
    let arr_id = doc.add_object(lopdf::Object::Array(vec![lopdf::Object::Reference(
        existing_id,
    )]));
    doc.get_object_mut(page_id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", lopdf::Object::Reference(arr_id));
    let mut base = Vec::new();
    doc.save_to(&mut base).expect("间接形态基准保存");
    let base_len = base.len();

    // 写入两条（Note + FreeText）→ flush（直接构造模型值——write_annotation
    // 的输入是模型层 Annotation，不经会话命令面）。
    let note = Annotation {
        id: AnnotId(1),
        page: PageIndex(0),
        kind: AnnotationKind::Note {
            rect: Rect::new(100.0, 500.0, 130.0, 530.0).unwrap(),
        },
        author: "human-author".into(),
        origin: Origin::Human,
        created_ms: 1_000,
        modified_ms: 1_000,
    };
    let free = Annotation {
        id: AnnotId(2),
        page: PageIndex(0),
        kind: AnnotationKind::FreeText {
            rect: Rect::new(200.0, 400.0, 360.0, 430.0).unwrap(),
            text: "indirect form probe".into(),
        },
        author: "ai-author".into(),
        origin: Origin::Ai,
        created_ms: 1_001,
        modified_ms: 1_001,
    };
    let mut writer = LopdfWriter::from_bytes(base.clone()).expect("间接形态 from_bytes");
    writer
        .write_annotation(PageIndex(0), &note)
        .expect("第一条写入");
    writer
        .write_annotation(PageIndex(0), &free)
        .expect("第二条写入");
    let save = writer.save_incremental().expect("增量保存");
    let product = writer.into_bytes();

    // ① 前缀逐位保持（含间接 /Annots 数组对象引用本身）。
    assert_eq!(&product[..base_len], &base[..], "间接形态前缀逐位保持");
    assert!(save.bytes_appended > 0, "有脏必有追加");

    // ②③ 重载：既有 1 条存活 + 新增 2 条在位（合计 3 条，顺序无关）。
    let reloaded = reload(&product);
    let annots = page_annots(&reloaded, 1);
    assert_eq!(annots.len(), 3, "既有 1 + 新增 2，无覆盖无丢失");
    let subtypes: Vec<&[u8]> = annots
        .iter()
        .map(|d| d.get(b"Subtype").unwrap().as_name().unwrap())
        .collect();
    assert!(
        subtypes.contains(&b"Text".to_vec().as_slice()),
        "既有 /Text 存活"
    );
    assert!(
        subtypes.iter().any(|s| *s == b"FreeText"),
        "新增 /FreeText 落位"
    );
    // 既有标注的 /T 原样（宿主数据不被改写）。
    let prior = annots
        .iter()
        .find(|d| d.get(b"Subtype").unwrap().as_name().unwrap() == b"Text")
        .unwrap();
    assert_eq!(
        prior.get(b"T").unwrap().as_str().unwrap_or(b""),
        b"prior-host",
        "宿主既有标注零触碰（/T 原样）"
    );
    let _ = existing_id;
}

/// 测试内联小工具：整数对象（避免与顶层 Object 导入冲突的独立名）。
fn lob_num(v: i64) -> lopdf::Object {
    lopdf::Object::Integer(v)
}

fn lob_str(v: &str) -> lopdf::Object {
    lopdf::Object::string_literal(v.as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// 17. 「曾加密」形态拒写（pdf-d-1r 审计轻微项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn was_encrypted_empty_password_still_denied() {
    // 空用户口令加密档：lopdf 装载时自动解密（is_encrypted() = false）
    // 但 was_encrypted() = true——「曾加密」形态。S5 语义 = 仍加密态
    // 拒绝增量写（源档密文，追加明文修订破坏保密语义与签名链）——
    // from_bytes 双防线必须同时拦住「仍加密」与「曾加密」两种形态。
    use lopdf::{dictionary, EncryptionVersion, Permissions};

    let mut doc = lopdf::Document::with_version("1.7");
    doc.trailer.set(
        "ID",
        lopdf::Object::Array(vec![
            lopdf::Object::string_literal(b"mdc-pdf-enc-empty-0"),
            lopdf::Object::string_literal(b"mdc-pdf-enc-empty-0"),
        ]),
    );
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => lopdf::Object::Name(b"Page".to_vec()),
        "Parent" => lopdf::Object::Reference(pages_id),
        "MediaBox" => vec![lopdf::Object::Integer(0), lopdf::Object::Integer(0), lopdf::Object::Integer(612), lopdf::Object::Integer(792)],
    });
    doc.set_object(
        pages_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => lopdf::Object::Name(b"Pages".to_vec()),
            "Kids" => vec![lopdf::Object::Reference(page_id)],
            "Count" => lopdf::Object::Integer(1),
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => lopdf::Object::Name(b"Catalog".to_vec()),
        "Pages" => lopdf::Object::Reference(pages_id),
    });
    doc.trailer
        .set("Root", lopdf::Object::Reference(catalog_id));
    let state: lopdf::EncryptionState = EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner-secret",
        user_password: "", // 空口令：装载即自动解密 → 「曾加密」形态
        permissions: Permissions::all(),
    }
    .try_into()
    .expect("空口令加密状态构造");
    doc.encrypt(&state).expect("对象加密");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("空口令加密档保存");

    // from_bytes 必须拒（was_encrypted 分支——不是 is_encrypted）。
    match LopdfWriter::from_bytes(bytes) {
        Err(CommandError::SecurityDeny { reason }) => {
            assert_eq!(reason, REASON_ENCRYPTED_WRITE, "S5 理由码与仍加密形态同锚");
        }
        other => panic!("曾加密档 from_bytes 应 SecurityDeny（得 {other:?}）"),
    }
}
