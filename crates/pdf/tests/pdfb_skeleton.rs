//! PDF-b — mdc-pdf 骨架语义锁（pdf_carrier_plan §5 PDF-b 交付定义 +
//! SECURITY_POLICY.md v1 安全规格 + ADR-P1 引擎选型前提）。
//!
//! 锁清单（每锁一条 `#[test]`，断言消息讲清**为什么**）：
//! 1. Rotation 90/270 交换宽高（归一化语义——effective_* 是取维唯一
//!    出口，消费点不各自处理旋转）；
//! 2. Rotation 0/180 不交换 + from_degrees 四值映射/非法拒绝；
//! 3. PageSpan 反转区间构造拒绝（start<=end 类型层不变量）；
//! 4. PageSpan 越界在**命令层**拒绝（span 无页数上下文——分层纪律）；
//! 5. 大纲深度优先文档序（3 层树 (depth,title) 序列）；
//! 6. 大纲边界：空树零产出 / 单根 depth=0；
//! 7. Rect 反序拒绝（PDF 用户空间不变量在类型层）；
//! 8. Quad/Point serde 往返（几何面协议稳定）；
//! 9. Annotation serde 往返（含 Origin::Ai 溯源；Origin 词面冻结）；
//! 10. AnnotId 从 1 单调分配（0 = 永不签发哨兵）；
//! 11. patch 永不改 kind（类型层无 kind 字段 + JSON 未知字段忽略 =
//!     改 kind 请求退化为 no-op——「忽略」语义锁死）；
//! 12. patch 三可变面正例（FreeText text / Note rect / Highlight color）
//!     + 变更推进 modified_ms；
//! 13. patch 不适用面忽略（无对应字段者静默 no-op，modified_ms 不动）；
//! 14. AnnotateList 三维过滤（kind/page/origin）正反例 + id 升序确定性；
//! 15. 搜索 max_hits 钳制 → truncated=true（S3 ❷：收敛继续 + 告知）；
//! 16. 空 query → EmptyQuery（S3 ❶硬拒绝：空串无语义）；
//! 17. 大小写不敏感 Unicode 折叠 + snippet char 边界安全（CJK/长度钳制）；
//! 18. S5：仍加密态 AnnotateAdd/Edit 双拒（策略门**先于**存在性检查，
//!     锁 SecurityDeny 而非 AnnotNotFound；只读命令放行）；
//! 19. DangerousFamily 五族 serde 名稳定（协议面冻结锚；斜杠形拒绝）；
//! 20. Source 五值镜像 mdcore（serde 名逐词面一致 + mdcore 源面文件级
//!     对齐——零 path 依赖下的跨 crate 镜像证据）；
//! 21. 协议常量词面锁（SCHEME/FACE/URL 前缀/事件族——防漂移）；
//! 22. 版本单源：CARGO_PKG_VERSION = [workspace.package].version +
//!     version.workspace 继承声明（与 r24_packaging 的 mdcore::VERSION
//!     锁合取 = 跨 crate 同值——对齐 r24_packaging 先例）；
//! 23. open 三校验（空档 / 页几何数不符 / 大纲目标越界 fail-fast）；
//! 24. S3-L7 ❶：作者名 200 字符上限硬拒绝（SecurityDeny 稳定理由码）；
//! 25. Limits 默认数值 = SECURITY_POLICY.md S3 表逐项（限额 = 逐条配
//!     回归测试的资产）；
//! 26. Limits enforce 族行为学（❶/❷ 生效值合同：min 收敛 + 行为学标记）；
//! 27. ExtractText 页集形状（All/区间展开 + v0 空文本语义 + 注入越界）；
//! 28. CommandEnvelope serde 往返 + Command JSON tag 面（协议草案锚）；
//! 29. DocumentEngine object-safe（`&dyn` 可命名——S4 对象引用形态）；
//! 30. serde 面反转 PageSpan 拒收（宿主 JSON 面不变量——pdf-b-1r 审计
//!     阻断项回归锁：派生 Deserialize 曾绕过构造校验，畸形信封可致
//!     panic/abort；字段私有 + 自定义 Deserialize 后必须锁死拒收语义）；
//! 31. serde 面反序 Rect 拒收（pdf-b-1r 审计重要项回归锁：反序矩形曾可
//!     经 patch 静默落库，PDF-d 落盘将产出非法 /Rect）；
//! 32. 搜索两模式非重叠语义一致（"aaaa"×"aa"=2——审计轻微项回归锁：
//!     不敏感路径曾产出重叠命中，与 match_indices 非重叠计数不一致）；
//! 33. attach_engine 页数一致性装配期校验（T1 审计 O3 项回归锁：引擎
//!     页多/页少双侧 typed 拒绝（携带双侧数值）+ 一致时装配成功 +
//!     拒绝后会话保持 v0 注入路径——双权威不一致的两条静默失败路径
//!     在装配面收口）。

use std::fs;
use std::path::Path;

use mdc_pdf::annotation::{
    AnnotId, Annotation, AnnotationDraft, AnnotationKind, AnnotationKindTag, AnnotationPatch,
    Origin, Point, Quad, Rect, RectError, Rgb,
};
use mdc_pdf::command::{
    AnnotationFilter, Command, CommandError, Outcome, SearchOptions, Source, TextTarget,
};
use mdc_pdf::document::{AttachEngineError, DocumentMeta, OpenError, PdfDocumentId, PdfSession};
use mdc_pdf::engine::DocumentEngine;
use mdc_pdf::outline::{OutlineNode, OutlineTarget, OutlineTree};
use mdc_pdf::page::{PageCount, PageGeometry, PageIndex, PageSpan, PageSpanError, Rotation};
use mdc_pdf::protocol::{self, CommandEnvelope};
use mdc_pdf::security::{
    DangerousFamily, DangerousHit, LimitBehavior, Limits, OpenAssessment, REASON_AUTHOR_LENGTH,
    REASON_ENCRYPTED_WRITE,
};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

/// Letter 页几何（0° 基准）。
fn letter(rotation: Rotation) -> PageGeometry {
    PageGeometry {
        width_pt: 612.0,
        height_pt: 792.0,
        rotation,
    }
}

/// 标准会话：n 页 Letter 0°、无大纲、无告警。
fn session(pages: u32) -> PdfSession {
    session_with(pages, OpenAssessment::default())
}

fn session_with(pages: u32, assessment: OpenAssessment) -> PdfSession {
    let meta = DocumentMeta {
        id: PdfDocumentId(7),
        file_name: "spec.pdf".into(),
        page_count: PageCount(pages),
    };
    let geoms: Vec<PageGeometry> = (0..pages).map(|_| letter(Rotation::Deg0)).collect();
    PdfSession::open(meta, assessment, geoms, OutlineTree::new()).expect("标准会话打开")
}

/// 便捷 add：返回新增后的 Annotation。
fn add_annotation(
    s: &mut PdfSession,
    page: PageIndex,
    kind: AnnotationKind,
    origin: Origin,
) -> Annotation {
    let draft = AnnotationDraft {
        page,
        kind,
        author: "tester".into(),
        origin,
    };
    match s.execute(Command::AnnotateAdd {
        draft,
        source: Source::View,
    }) {
        Ok(Outcome::AnnotAdded(a)) => a,
        other => panic!("AnnotateAdd 应成功，得 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1-2. Rotation 归一化
// ---------------------------------------------------------------------------

#[test]
fn rotation_90_270_swaps_effective_dimensions() {
    let g90 = letter(Rotation::Deg90);
    assert_eq!(
        g90.effective_width(),
        792.0,
        "90° 观看系宽 = 原高（旋转后宽高互换）"
    );
    assert_eq!(g90.effective_height(), 612.0, "90° 观看系高 = 原宽");
    let g270 = letter(Rotation::Deg270);
    assert_eq!(
        g270.effective_width(),
        792.0,
        "270° 同为交换轴语义（与 90° 一致）"
    );
    assert_eq!(g270.effective_height(), 612.0, "270° 高 = 原宽");
    assert_eq!(
        letter(Rotation::Deg270).effective_width(),
        g90.effective_width()
    );
}

#[test]
fn rotation_0_180_keep_dimensions_and_degrees_mapping() {
    for rot in [Rotation::Deg0, Rotation::Deg180] {
        let g = letter(rot);
        assert_eq!(g.effective_width(), 612.0, "{rot:?} 不交换宽（横竖轴未变）");
        assert_eq!(g.effective_height(), 792.0, "{rot:?} 不交换高");
    }
    for (deg, want) in [
        (0, Rotation::Deg0),
        (90, Rotation::Deg90),
        (180, Rotation::Deg180),
        (270, Rotation::Deg270),
    ] {
        assert_eq!(
            Rotation::from_degrees(deg),
            Some(want),
            "/Rotate 四值逐一映射"
        );
    }
    assert_eq!(
        Rotation::from_degrees(45),
        None,
        "45° 非规范值必须显式拒绝——静默取模会错转渲染面"
    );
    assert_eq!(
        Rotation::from_degrees(-90),
        None,
        "负值同拒（/Rotate 只有四合法值）"
    );
}

// ---------------------------------------------------------------------------
// 3-4. PageSpan
// ---------------------------------------------------------------------------

#[test]
fn page_span_rejects_inverted_range_at_construction() {
    let err = PageSpan::new(PageIndex(3), PageIndex(1))
        .expect_err("start>end 必须在构造期拒绝——反转区间不是合法闭区间");
    assert!(matches!(
        err,
        PageSpanError::Inverted {
            start: PageIndex(3),
            end: PageIndex(1)
        }
    ));
    assert!(
        PageSpan::new(PageIndex(2), PageIndex(2)).is_ok(),
        "start==end = 单页闭区间，合法"
    );
    assert_eq!(
        PageSpan::new(PageIndex(0), PageIndex(4)).unwrap().len(),
        5,
        "闭区间含两端"
    );
    assert!(
        !PageSpan::new(PageIndex(0), PageIndex(4))
            .unwrap()
            .is_empty(),
        "闭区间恒非空"
    );
}

#[test]
fn page_span_out_of_range_rejected_at_command_layer() {
    let mut s = session(3);
    let span = PageSpan::new(PageIndex(0), PageIndex(5)).expect("0..5 构造合法（无页数上下文）");
    let r = s.execute(Command::ExtractText {
        target: TextTarget::Pages(span),
        source: Source::View,
    });
    match r {
        Err(CommandError::InvalidPage(p)) => assert_eq!(p, PageIndex(5), "携带肇事上界页号"),
        other => panic!("越界 span 应得 InvalidPage，得 {other:?}"),
    }
    // 单点越界同样拒绝（下界在档、上界越档）。
    let bad = PageSpan::new(PageIndex(2), PageIndex(3)).unwrap();
    assert!(matches!(
        s.execute(Command::ExtractText {
            target: TextTarget::Pages(bad),
            source: Source::View
        }),
        Err(CommandError::InvalidPage(PageIndex(3)))
    ));
}

// ---------------------------------------------------------------------------
// 5-6. 大纲
// ---------------------------------------------------------------------------

#[test]
fn outline_iterates_depth_first_in_document_order() {
    let page = |n: u32| OutlineTarget::Page(PageIndex(n));
    let tree = OutlineTree::from_roots(vec![
        OutlineNode::branch(
            "A",
            page(0),
            vec![
                OutlineNode::leaf("A.1", page(1)),
                OutlineNode::branch("A.2", page(2), vec![OutlineNode::leaf("A.2.1", page(2))]),
            ],
        ),
        OutlineNode::leaf("B", page(2)),
    ]);
    let seq: Vec<(usize, String)> = tree.iter().map(|(d, n)| (d, n.title.clone())).collect();
    assert_eq!(
        seq,
        vec![
            (0, "A".into()),
            (1, "A.1".into()),
            (1, "A.2".into()),
            (2, "A.2.1".into()),
            (0, "B".into()),
        ],
        "前序深度优先：父先于子、同级按声明序、跨根回 depth 0——导航面板唯一排序出口"
    );
}

#[test]
fn outline_empty_tree_and_single_root_boundaries() {
    let empty = OutlineTree::new();
    assert_eq!(
        empty.iter().count(),
        0,
        "空树零产出（无大纲文档是合法形态）"
    );
    assert!(empty.is_empty());
    let single = OutlineTree::from_roots(vec![OutlineNode::leaf(
        "only",
        OutlineTarget::Page(PageIndex(0)),
    )]);
    let collected: Vec<(usize, &str)> = single.iter().map(|(d, n)| (d, n.title.as_str())).collect();
    assert_eq!(collected, vec![(0, "only")], "单根 depth=0");
    assert_eq!(single.roots().len(), 1);
}

// ---------------------------------------------------------------------------
// 7. Rect
// ---------------------------------------------------------------------------

#[test]
fn rect_rejects_inverted_coordinates() {
    let x_inv = Rect::new(10.0, 10.0, 5.0, 20.0);
    assert!(
        matches!(
            x_inv,
            Err(RectError::Inverted {
                x0: 10.0,
                x1: 5.0,
                ..
            })
        ),
        "x 反序拒绝"
    );
    let y_inv = Rect::new(0.0, 30.0, 10.0, 20.0);
    assert!(
        matches!(
            y_inv,
            Err(RectError::Inverted {
                y0: 30.0,
                y1: 20.0,
                ..
            })
        ),
        "y 反序拒绝（用户空间 y 向上为正）"
    );
    let degenerate = Rect::new(5.0, 5.0, 5.0, 5.0);
    assert!(
        degenerate.is_ok(),
        "零面积退化矩形合法（空锚定位是有效形态）"
    );
    let r = Rect::new(1.0, 2.0, 4.0, 6.0).unwrap();
    assert_eq!(r.width(), 3.0);
    assert_eq!(r.height(), 4.0);
}

// ---------------------------------------------------------------------------
// 8-9. serde 往返（几何/标注）
// ---------------------------------------------------------------------------

#[test]
fn quad_point_serde_roundtrip() {
    let quad = Quad {
        points: [
            Point { x: 72.0, y: 700.0 },
            Point { x: 200.0, y: 700.0 },
            Point { x: 200.0, y: 688.0 },
            Point { x: 72.0, y: 688.0 },
        ],
    };
    let json = serde_json::to_string(&quad).expect("Quad 序列化");
    assert_eq!(
        serde_json::from_str::<Quad>(&json).unwrap(),
        quad,
        "Quad 几何面协议往返恒等"
    );
    let pt = Point { x: 1.5, y: -2.25 };
    let pj = serde_json::to_string(&pt).unwrap();
    assert_eq!(
        serde_json::from_str::<Point>(&pj).unwrap(),
        pt,
        "Point 往返"
    );
}

#[test]
fn annotation_serde_roundtrip_with_ai_origin() {
    let a = Annotation {
        id: AnnotId(3),
        page: PageIndex(1),
        kind: AnnotationKind::Ink {
            strokes: vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 5.5, y: 5.5 }]],
        },
        author: "assistant".into(),
        origin: Origin::Ai,
        created_ms: 100,
        modified_ms: 200,
    };
    let json = serde_json::to_string(&a).expect("Annotation 序列化");
    assert!(
        json.contains("\"origin\":\"Ai\""),
        "Ai 溯源字段词面冻结（plan §4：author 字段区分 Human/AI）"
    );
    assert_eq!(
        serde_json::to_string(&Origin::Human).unwrap(),
        "\"Human\"",
        "Origin 词面冻结：Human/Ai 两值"
    );
    assert_eq!(
        serde_json::from_str::<Annotation>(&json).unwrap(),
        a,
        "标注整实体往返恒等"
    );
}

// ---------------------------------------------------------------------------
// 10. AnnotId 单调
// ---------------------------------------------------------------------------

#[test]
fn annot_ids_are_monotonic_from_one() {
    let mut s = session(2);
    s.set_clock_ms(1_000);
    let a = add_annotation(
        &mut s,
        PageIndex(0),
        AnnotationKind::Note {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
        },
        Origin::Human,
    );
    let b = add_annotation(
        &mut s,
        PageIndex(1),
        AnnotationKind::Note {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
        },
        Origin::Ai,
    );
    assert_eq!(a.id, AnnotId(1), "首签发 id=1");
    assert_eq!(b.id, AnnotId(2), "次签发 id=2（单调）");
    assert_eq!(a.created_ms, 1_000, "时间戳取会话时钟（宿主注入）");
    assert_eq!(a.modified_ms, 1_000, "创建时 modified=created");
    assert_eq!(s.annotation_count(), 2);
}

// ---------------------------------------------------------------------------
// 11-13. patch 面
// ---------------------------------------------------------------------------

#[test]
fn patch_never_changes_kind() {
    // (a) 类型层：AnnotationPatch 无 kind 字段——JSON 改 kind 的请求按
    //     serde 默认忽略未知字段 → 空 patch → no-op（「忽略」语义锁死）。
    let hostile = r#"{"kind":{"Ink":{"strokes":[[{"x":0,"y":0}]]}}}"#;
    let patch: AnnotationPatch = serde_json::from_str(hostile).expect("未知字段忽略不报错");
    assert!(
        patch.is_empty(),
        "kind 不在 patch 面——恶意改型请求退化为空 patch"
    );

    let mut s = session(1);
    let created = add_annotation(
        &mut s,
        PageIndex(0),
        AnnotationKind::FreeText {
            rect: Rect::new(10.0, 10.0, 100.0, 60.0).unwrap(),
            text: "keep".into(),
        },
        Origin::Human,
    );
    s.set_clock_ms(2_000);
    let out = s
        .execute(Command::AnnotateEdit {
            id: created.id,
            patch: AnnotationPatch {
                contents: Some("changed".into()),
                ..Default::default()
            },
            source: Source::View,
        })
        .expect("合法 patch 编辑成功");
    match out {
        Outcome::AnnotEdited(a) => {
            assert!(
                matches!(a.kind, AnnotationKind::FreeText { .. }),
                "kind 仍是 FreeText——text 变更不改型"
            );
            assert_eq!(
                a.kind,
                AnnotationKind::FreeText {
                    rect: Rect::new(10.0, 10.0, 100.0, 60.0).unwrap(),
                    text: "changed".into()
                },
                "text 面已变更"
            );
        }
        other => panic!("应得 AnnotEdited，得 {other:?}"),
    }
}

#[test]
fn patch_applies_three_mutable_faces_and_advances_mtime() {
    let mut s = session(3);
    s.set_clock_ms(1_000);
    let freetext = add_annotation(
        &mut s,
        PageIndex(0),
        AnnotationKind::FreeText {
            rect: Rect::new(10.0, 10.0, 100.0, 60.0).unwrap(),
            text: "旧文本".into(),
        },
        Origin::Human,
    );
    let note = add_annotation(
        &mut s,
        PageIndex(1),
        AnnotationKind::Note {
            rect: Rect::new(0.0, 700.0, 40.0, 720.0).unwrap(),
        },
        Origin::Human,
    );
    let hl = add_annotation(
        &mut s,
        PageIndex(2),
        AnnotationKind::Highlight {
            quads: vec![Quad {
                points: [Point { x: 0.0, y: 0.0 }; 4],
            }],
            color: Rgb { r: 255, g: 0, b: 0 },
        },
        Origin::Ai,
    );
    s.set_clock_ms(5_000);

    let edit = |s: &mut PdfSession, id: AnnotId, patch: AnnotationPatch| -> Annotation {
        match s.execute(Command::AnnotateEdit {
            id,
            patch,
            source: Source::Ai,
        }) {
            Ok(Outcome::AnnotEdited(a)) => a,
            other => panic!("edit 应成功，得 {other:?}"),
        }
    };

    // FreeText.text（contents → /Contents 语义）
    let ft = edit(
        &mut s,
        freetext.id,
        AnnotationPatch {
            contents: Some("新文本".into()),
            ..Default::default()
        },
    );
    assert_eq!(
        ft.kind,
        AnnotationKind::FreeText {
            rect: Rect::new(10.0, 10.0, 100.0, 60.0).unwrap(),
            text: "新文本".into()
        }
    );
    assert_eq!(
        ft.modified_ms, 5_000,
        "内容变更推进 modified_ms（反映内容非命令次数）"
    );
    assert_eq!(ft.created_ms, 1_000, "created_ms 不可变");

    // Note.rect
    let n = edit(
        &mut s,
        note.id,
        AnnotationPatch {
            rect: Some(Rect::new(5.0, 600.0, 50.0, 640.0).unwrap()),
            ..Default::default()
        },
    );
    assert_eq!(
        n.kind,
        AnnotationKind::Note {
            rect: Rect::new(5.0, 600.0, 50.0, 640.0).unwrap()
        }
    );
    assert_eq!(n.modified_ms, 5_000);

    // Highlight.color
    let h = edit(
        &mut s,
        hl.id,
        AnnotationPatch {
            color: Some(Rgb {
                r: 0,
                g: 128,
                b: 255,
            }),
            ..Default::default()
        },
    );
    assert!(
        matches!(
            h.kind,
            AnnotationKind::Highlight {
                color: Rgb {
                    r: 0,
                    g: 128,
                    b: 255
                },
                ..
            },
        ),
        "换色后面貌变、quads 结构不变"
    );
    assert_eq!(h.modified_ms, 5_000);
}

#[test]
fn patch_inapplicable_faces_are_ignored_as_noop() {
    let mut s = session(1);
    s.set_clock_ms(1_000);
    // Highlight：有 quads/color，无 rect/text 字段 → rect/contents patch 不适用。
    let hl = add_annotation(
        &mut s,
        PageIndex(0),
        AnnotationKind::Highlight {
            quads: vec![Quad {
                points: [Point { x: 1.0, y: 1.0 }; 4],
            }],
            color: Rgb {
                r: 255,
                g: 255,
                b: 0,
            },
        },
        Origin::Human,
    );
    s.set_clock_ms(9_000);
    let out = s
        .execute(Command::AnnotateEdit {
            id: hl.id,
            patch: AnnotationPatch {
                rect: Some(Rect::new(0.0, 0.0, 999.0, 999.0).unwrap()),
                contents: Some("不应出现".into()),
                ..Default::default()
            },
            source: Source::View,
        })
        .expect("不适用面忽略而非报错——错误面收窄五类，无 per-field 变体");
    match out {
        Outcome::AnnotEdited(a) => {
            assert_eq!(
                a.kind,
                AnnotationKind::Highlight {
                    quads: vec![Quad {
                        points: [Point { x: 1.0, y: 1.0 }; 4]
                    }],
                    color: Rgb {
                        r: 255,
                        g: 255,
                        b: 0
                    },
                },
                "rect/contents 对 Highlight 无字段可写——原样返回"
            );
            assert_eq!(
                a.modified_ms, 1_000,
                "no-op 不推进 modified_ms（无内容变更）"
            );
        }
        other => panic!("应得 AnnotEdited，得 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 14. AnnotateList 过滤
// ---------------------------------------------------------------------------

#[test]
fn annotate_list_filters_kind_page_origin() {
    let mut s = session(2);
    add_annotation(
        &mut s,
        PageIndex(0),
        AnnotationKind::Highlight {
            quads: vec![],
            color: Rgb { r: 255, g: 0, b: 0 },
        },
        Origin::Human,
    );
    add_annotation(
        &mut s,
        PageIndex(1),
        AnnotationKind::Note {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
        },
        Origin::Human,
    );
    add_annotation(
        &mut s,
        PageIndex(1),
        AnnotationKind::FreeText {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
            text: "t".into(),
        },
        Origin::Ai,
    );
    add_annotation(
        &mut s,
        PageIndex(0),
        AnnotationKind::Ink { strokes: vec![] },
        Origin::Ai,
    );

    let list = |s: &mut PdfSession, filter: AnnotationFilter| -> Vec<AnnotId> {
        match s.execute(Command::AnnotateList {
            filter,
            source: Source::View,
        }) {
            Ok(Outcome::AnnotListed(v)) => v.iter().map(|a| a.id).collect(),
            other => panic!("list 应成功，得 {other:?}"),
        }
    };

    // 空过滤 = 全量（id 升序）。
    assert_eq!(
        list(&mut s, AnnotationFilter::default()),
        vec![AnnotId(1), AnnotId(2), AnnotId(3), AnnotId(4)],
        "无过滤 = 全库，id 升序确定性"
    );
    // kind 过滤正反例：[Highlight, Ink] → 1,4；Note/FreeText 不在。
    assert_eq!(
        list(
            &mut s,
            AnnotationFilter {
                kinds: Some(vec![AnnotationKindTag::Highlight, AnnotationKindTag::Ink]),
                ..Default::default()
            }
        ),
        vec![AnnotId(1), AnnotId(4)],
        "kind 集合命中 Highlight(1) 与 Ink(4)——Note(2)/FreeText(3) 反例排除"
    );
    // page 过滤：页 1 → 2,3。
    assert_eq!(
        list(
            &mut s,
            AnnotationFilter {
                page: Some(PageIndex(1)),
                ..Default::default()
            }
        ),
        vec![AnnotId(2), AnnotId(3)]
    );
    // origin 过滤：Ai → 3,4。
    assert_eq!(
        list(
            &mut s,
            AnnotationFilter {
                origin: Some(Origin::Ai),
                ..Default::default()
            }
        ),
        vec![AnnotId(3), AnnotId(4)],
        "Ai 溯源可过滤——Human 与 AI 共库共路径，按需分视图"
    );
    // 合取：Note + page1 → 2。
    assert_eq!(
        list(
            &mut s,
            AnnotationFilter {
                kinds: Some(vec![AnnotationKindTag::Note]),
                page: Some(PageIndex(1)),
                ..Default::default()
            }
        ),
        vec![AnnotId(2)]
    );
}

// ---------------------------------------------------------------------------
// 15-17. 搜索
// ---------------------------------------------------------------------------

#[test]
fn search_max_hits_clamps_with_truncated_flag() {
    let mut s = session(1);
    s.set_page_text(PageIndex(0), "alpha beta alpha beta alpha")
        .expect("页注入");
    // ❷ 单位面：请求钳制。
    let (eff, behavior) = Limits::default().enforce_search_hits(5_000);
    assert_eq!(
        (eff, behavior),
        (1_000, LimitBehavior::Clamp),
        "超上限收敛到 1000，行为学=钳制"
    );
    let (eff2, _) = Limits::default().enforce_search_hits(10);
    assert_eq!(eff2, 10, "未超限原值放行");

    // 执行面：3 个命中、max_hits=2 → 2 条 + truncated=true（溢出告知）。
    let out = s
        .execute(Command::Search {
            query: "alpha".into(),
            opts: SearchOptions {
                case_insensitive: false,
                max_hits: 2,
            },
            source: Source::View,
        })
        .expect("搜索执行");
    match out {
        Outcome::SearchResults { hits, truncated } => {
            assert_eq!(hits.len(), 2, "钳制到 2——收敛继续（❷）而非拒绝");
            assert!(
                truncated,
                "仍有第 3 个命中——必须 truncated 告知（不静默丢弃）"
            );
            assert!(hits.iter().all(|h| h.page == PageIndex(0)));
            assert!(hits[0].snippet.contains("alpha"));
        }
        other => panic!("应得 SearchResults，得 {other:?}"),
    }
    // 未溢出时 truncated=false（无假告知）。
    let out2 = s
        .execute(Command::Search {
            query: "alpha".into(),
            opts: SearchOptions {
                case_insensitive: false,
                max_hits: 10,
            },
            source: Source::View,
        })
        .expect("搜索执行 2");
    assert!(
        matches!(&out2, Outcome::SearchResults { hits, truncated: false } if hits.len() == 3),
        "max_hits 充足时全量 + 无截断标记"
    );
}

#[test]
fn search_empty_query_is_hard_rejected() {
    let mut s = session(1);
    s.set_page_text(PageIndex(0), "some text").expect("页注入");
    let r = s.execute(Command::Search {
        query: String::new(),
        opts: SearchOptions {
            case_insensitive: false,
            max_hits: 10,
        },
        source: Source::Ai,
    });
    assert!(
        matches!(r, Err(CommandError::EmptyQuery)),
        "空 query = ❶硬拒绝（空串无语义，全匹配是资源滥用面）"
    );
}

#[test]
fn search_case_folding_and_snippet_char_safety() {
    let mut s = session(1);
    // 大小写不敏感：Unicode 简单折叠（Ö→ö；GRÖSSE 匹配 grösse 查询）。
    s.set_page_text(PageIndex(0), "Vorwort. Die GRÖSSE matters und grösse auch.")
        .expect("页注入");
    let out = s
        .execute(Command::Search {
            query: "grösse".into(),
            opts: SearchOptions {
                case_insensitive: true,
                max_hits: 10,
            },
            source: Source::View,
        })
        .expect("搜索执行");
    match out {
        Outcome::SearchResults { hits, truncated } => {
            assert_eq!(hits.len(), 2, "GRÖSSE 与 grösse 都命中（折叠后等价）");
            assert!(!truncated);
        }
        other => panic!("应得 SearchResults，得 {other:?}"),
    }
    // 大小写敏感：仅小写形命中。
    let out2 = s
        .execute(Command::Search {
            query: "grösse".into(),
            opts: SearchOptions {
                case_insensitive: false,
                max_hits: 10,
            },
            source: Source::View,
        })
        .expect("搜索执行 2");
    assert!(
        matches!(&out2, Outcome::SearchResults { hits, truncated: false } if hits.len() == 1),
        "敏感模式只命中小写形"
    );

    // snippet char 边界与长度钳制：306 字符文本、命中居中。
    let long = format!("{}NEEDLE{}", "x".repeat(150), "y".repeat(150));
    let mut s2 = session(1);
    s2.set_page_text(PageIndex(0), long).expect("页注入");
    let out3 = s2
        .execute(Command::Search {
            query: "NEEDLE".into(),
            opts: SearchOptions {
                case_insensitive: false,
                max_hits: 1,
            },
            source: Source::View,
        })
        .expect("搜索执行 3");
    match out3 {
        Outcome::SearchResults { hits, .. } => {
            let snip = &hits[0].snippet;
            assert_eq!(
                snip.chars().count(),
                200,
                "S3 snippet 上限 200 字符（❷钳制）"
            );
            assert!(snip.contains("NEEDLE"), "命中本体必须在摘录内");
            assert_eq!(&snip[..100], &"x".repeat(100), "左半 = 命中前 100 字符");
            assert!(
                snip.ends_with(&"y".repeat(94)),
                "右半补满至 200（不撕裂字符）"
            );
        }
        other => panic!("应得 SearchResults，得 {other:?}"),
    }
    // CJK 边界：全中文短文本摘录整段（无撕裂、无 panic）。
    let mut s3 = session(1);
    s3.set_page_text(PageIndex(0), "边界测试。目标词。结束。")
        .expect("页注入");
    let out4 = s3
        .execute(Command::Search {
            query: "目标词".into(),
            opts: SearchOptions {
                case_insensitive: false,
                max_hits: 5,
            },
            source: Source::View,
        })
        .expect("搜索执行 4");
    assert!(
        matches!(&out4, Outcome::SearchResults { hits, truncated: false } if hits[0].snippet == "边界测试。目标词。结束。"),
        "中文文本摘录完整段"
    );
}

// ---------------------------------------------------------------------------
// 18. S5 加密写拒
// ---------------------------------------------------------------------------

#[test]
fn encrypted_assessment_denies_writes_policy_first() {
    let assessment = OpenAssessment {
        encrypted: true,
        warnings: vec![DangerousHit {
            family: DangerousFamily::OpenAction,
            detail: "Catalog /OpenAction (launch)".into(),
        }],
    };
    let mut s = session_with(2, assessment);

    // 写拒：AnnotateAdd。
    let add = s.execute(Command::AnnotateAdd {
        draft: AnnotationDraft {
            page: PageIndex(0),
            kind: AnnotationKind::Note {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
            },
            author: "tester".into(),
            origin: Origin::Human,
        },
        source: Source::View,
    });
    assert!(
        matches!(add, Err(CommandError::SecurityDeny { reason }) if reason == REASON_ENCRYPTED_WRITE),
        "S5：仍加密态拒绝增量写（理由码稳定：{REASON_ENCRYPTED_WRITE:?}）"
    );

    // 写拒：AnnotateEdit——**先于存在性检查**（id=1 并不存在，但必须报
    // SecurityDeny 而非 AnnotNotFound：策略门在最前，防侧信道探测库内容）。
    let edit = s.execute(Command::AnnotateEdit {
        id: AnnotId(1),
        patch: AnnotationPatch {
            contents: Some("x".into()),
            ..Default::default()
        },
        source: Source::View,
    });
    assert!(
        matches!(
            edit,
            Err(CommandError::SecurityDeny {
                reason: REASON_ENCRYPTED_WRITE
            })
        ),
        "策略拒绝先于存在性——不给「库里有没有这条标注」的探测面"
    );

    // 只读放行（S5.1：v0 只读支持加密档打开）。
    assert!(
        s.execute(Command::Outline {
            source: Source::View
        })
        .is_ok(),
        "读类命令不受 S5 影响"
    );
    assert!(s
        .execute(Command::AnnotateList {
            filter: AnnotationFilter::default(),
            source: Source::View
        })
        .is_ok());
    // 库保持零写入。
    assert_eq!(s.annotation_count(), 0, "加密态全程零落库");
}

// ---------------------------------------------------------------------------
// 19. DangerousFamily serde 名
// ---------------------------------------------------------------------------

#[test]
fn dangerous_family_serde_names_frozen() {
    let pairs = [
        (DangerousFamily::OpenAction, "OpenAction"),
        (DangerousFamily::AdditionalActions, "AdditionalActions"),
        (DangerousFamily::LaunchAction, "LaunchAction"),
        (DangerousFamily::EmbeddedFiles, "EmbeddedFiles"),
        (DangerousFamily::RichMedia, "RichMedia"),
    ];
    for (family, name) in pairs {
        let json = serde_json::to_string(&family).expect("族序列化");
        assert_eq!(
            json,
            format!("\"{name}\""),
            "五族 serde 名冻结——协议面锚，变体名漂移=破坏性变更"
        );
        let back: DangerousFamily = serde_json::from_str(&json).expect("族反序列化");
        assert_eq!(back, family);
    }
    // 斜杠形（PDF 原始键）不进协议面——引擎映射层职责。
    assert!(
        serde_json::from_str::<DangerousFamily>("\"/OpenAction\"").is_err(),
        "斜杠形拒绝：协议面只认代码语义名"
    );
}

// ---------------------------------------------------------------------------
// 20. Source 镜像 mdcore
// ---------------------------------------------------------------------------

#[test]
fn source_mirrors_mdcore_five_values() {
    // (a) 本 crate 五值 serde 名（snake_case）——镜像的词面侧。
    let pairs = [
        (Source::View, "view"),
        (Source::Undo, "undo"),
        (Source::Redo, "redo"),
        (Source::Ai, "ai"),
        (Source::System, "system"),
    ];
    for (src, name) in pairs {
        assert_eq!(
            serde_json::to_string(&src).unwrap(),
            format!("\"{name}\""),
            "Source serde 名与 mdcore::history::Source 逐词面一致（镜像纪律）"
        );
    }
    // (b) mdcore 源面对齐（文件级证据——零 path 依赖下的跨 crate 镜像锁，
    //     手法同 contract_freeze.rs 的蓝图词面锁）。
    let md = fs::read_to_string(repo_root().join("crates/mdcore/src/history.rs"))
        .expect("mdcore history.rs 在位");
    let idx = md.find("pub enum Source").expect("mdcore Source 枚举在位");
    // 属性前置断言（pdf-b-1r 审计增严）：rename_all 策略若漂移，serde 名
    // 镜像锁会在无属性时报假阳——先锁属性存在（作用域=枚举声明前紧邻
    // 200 字符，避免被文件中其他枚举的属性假阳）。
    let attr_start = idx.saturating_sub(200);
    let attr_zone = &md[attr_start..idx];
    assert!(
        attr_zone.contains("rename_all = \"snake_case\"")
            || attr_zone.contains("rename_all=\"snake_case\""),
        "mdcore Source 带 rename_all=snake_case 属性——镜像锁前提（改名策略漂移必须在此报警）"
    );
    let block = &md[idx..idx + 400];
    for v in ["View", "Undo", "Redo", "Ai", "System"] {
        assert!(
            block.contains(&format!("    {v},")),
            "mdcore Source 含变体 {v}——五值镜像缺一即漂移"
        );
    }
}

// ---------------------------------------------------------------------------
// 21. 协议常量
// ---------------------------------------------------------------------------

#[test]
fn protocol_constants_are_frozen() {
    assert_eq!(
        protocol::SCHEME,
        "mdcore",
        "scheme 与 MD 面 mdcore://app 同域（CONTRACT §2）"
    );
    assert_eq!(protocol::FACE, "pdf");
    assert_eq!(
        protocol::URL_PREFIX,
        "mdcore://pdf/",
        "URL 形状 = mdcore://pdf/<document-slot>/…（草案）"
    );
    assert_eq!(
        protocol::EVENT_COMMAND,
        "mdc-pdf-cmd",
        "命令事件名（mdc-* 命名域，RC 轮词面纪律）"
    );
    assert_eq!(
        protocol::EVENT_EVENT,
        "mdc-pdf-event",
        "回执事件名（mdc-* 命名域）"
    );
}

// ---------------------------------------------------------------------------
// 22. 版本单源
// ---------------------------------------------------------------------------

#[test]
fn version_single_source_with_workspace_package() {
    // (a) 本 crate 版本 = 根 [workspace.package].version（唯一手写点）。
    //     与 r24_packaging.rs 的「workspace.version == mdcore::VERSION」锁
    //     合取 ⟹ env!(CARGO_PKG_VERSION) == mdcore::VERSION（跨 crate 同值
    //     的传递闭包——本仓不引入对 mdcore 的 path 依赖来直证）。
    let root = fs::read_to_string(repo_root().join("Cargo.toml")).expect("根 Cargo.toml");
    let section = root
        .split("[workspace.package]")
        .nth(1)
        .expect("[workspace.package] 节在位（版本唯一手写点）");
    let ws_version = section
        .lines()
        .find_map(|l| l.trim().strip_prefix("version = "))
        .expect("workspace.package 内必须有 version")
        .trim()
        .trim_matches('"');
    assert_eq!(
        ws_version,
        env!("CARGO_PKG_VERSION"),
        "mdc-pdf 版本必须继承 workspace 唯一手写点"
    );

    // (b) 继承声明面：独立 version 声明 = 第二手写点 = 漂移源（r24_packaging 同款纪律）。
    let own = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("本 crate 清单");
    assert!(
        own.contains("version.workspace = true"),
        "必须以 version.workspace 继承"
    );
    assert!(
        !own.lines()
            .any(|l| l.trim_start().starts_with("version = ")),
        "不得存在独立 version 声明"
    );
}

// ---------------------------------------------------------------------------
// 23. open 校验
// ---------------------------------------------------------------------------

#[test]
fn open_validates_geometry_outline_and_emptiness() {
    let meta = |n: u32| DocumentMeta {
        id: PdfDocumentId(1),
        file_name: "spec.pdf".into(),
        page_count: PageCount(n),
    };
    let geom: Vec<PageGeometry> = (0..3).map(|_| letter(Rotation::Deg0)).collect();

    // 0 页文档拒绝。
    assert_eq!(
        PdfSession::open(
            meta(0),
            OpenAssessment::default(),
            vec![],
            OutlineTree::new()
        )
        .err(),
        Some(OpenError::EmptyDocument),
        "PDF 规范：Page tree 至少一页——0 页 = 畸形输入 fail-fast"
    );
    // 页几何数不符。
    assert_eq!(
        PdfSession::open(
            meta(3),
            OpenAssessment::default(),
            vec![letter(Rotation::Deg0)],
            OutlineTree::new()
        )
        .err(),
        Some(OpenError::PageGeometryMismatch {
            expected: 3,
            provided: 1
        }),
        "注入页几何数与元数据页数不符 = 宿主接线错误，显式报错"
    );
    // 大纲目标越界。
    let bad_outline = OutlineTree::from_roots(vec![OutlineNode::leaf(
        "ghost",
        OutlineTarget::Page(PageIndex(5)),
    )]);
    assert_eq!(
        PdfSession::open(
            meta(3),
            OpenAssessment::default(),
            geom.clone(),
            bad_outline
        )
        .err(),
        Some(OpenError::OutlineTargetOutOfRange(PageIndex(5))),
        "越界大纲在打开期剔除——防止命令层消费幽灵页"
    );
    // 合法路径（3 页 + 页内大纲）。
    let ok_outline = OutlineTree::from_roots(vec![OutlineNode::leaf(
        "p2",
        OutlineTarget::Page(PageIndex(2)),
    )]);
    assert!(PdfSession::open(meta(3), OpenAssessment::default(), geom, ok_outline).is_ok());
}

// ---------------------------------------------------------------------------
// 24. S3-L7 作者名
// ---------------------------------------------------------------------------

#[test]
fn author_length_limit_hard_rejects_over_200_chars() {
    let mut s = session(1);
    let draft_at = |author: String| AnnotationDraft {
        page: PageIndex(0),
        kind: AnnotationKind::Note {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
        },
        author,
        origin: Origin::Human,
    };
    // 边界内：恰好 200 字符放行（L7 上限含端点）。
    let ok = s.execute(Command::AnnotateAdd {
        draft: draft_at("a".repeat(200)),
        source: Source::View,
    });
    assert!(ok.is_ok(), "200 字符 = 上限内（含端点），不拒");

    // 超限：201 → ❶硬拒绝（SecurityDeny 稳定理由码）。
    let over = s.execute(Command::AnnotateAdd {
        draft: draft_at("a".repeat(201)),
        source: Source::View,
    });
    assert!(
        matches!(over, Err(CommandError::SecurityDeny { reason }) if reason == REASON_AUTHOR_LENGTH),
        "S3-L7 ❶：作者名超 200 字符硬拒绝，理由码 = {REASON_AUTHOR_LENGTH:?}"
    );
    assert_eq!(s.annotation_count(), 1, "超限请求零落库");
    // 中文按字符计（200 个汉字 ≠ 600 字节——计字符不误伤多字节文本）。
    let cjk = s.execute(Command::AnnotateAdd {
        draft: AnnotationDraft {
            page: PageIndex(0),
            kind: AnnotationKind::Note {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
            },
            author: "汉".repeat(201),
            origin: Origin::Human,
        },
        source: Source::View,
    });
    assert!(
        matches!(cjk, Err(CommandError::SecurityDeny { .. })),
        "字符计数（非字节）——201 个汉字同样超限"
    );
}

// ---------------------------------------------------------------------------
// 25. Limits 数值面
// ---------------------------------------------------------------------------

#[test]
fn limits_defaults_match_security_policy_table() {
    let l = Limits::default();
    assert_eq!(l.decode_output_max_bytes, 1 << 30, "L0 解压输出 1 GiB");
    assert_eq!(l.object_depth_max, 128, "L0 嵌套深度 128");
    assert_eq!(l.xref_objects_max, 8_388_607, "L0 xref 上限（ISO 附录 C）");
    assert_eq!(
        l.raster_max_side_px, 8_192,
        "L2 位图单边 8192（PDF-c 启用）"
    );
    assert_eq!(
        l.raster_max_pixels, 64_000_000,
        "L2 位图总像素 64M（PDF-c 启用）"
    );
    assert_eq!(
        l.render_watchdog_ms, 20_000,
        "L2 看门狗 20s（PDF-c 启用，❸弃置）"
    );
    assert_eq!(
        l.open_budget_ms, 3_000,
        "L4 打开预算 3s（PDF-c 接入，❷降级）"
    );
    assert_eq!(
        l.open_file_max_bytes,
        1 << 30,
        "L7 文件大小 1 GiB（宿主 open 前执行）"
    );
    assert_eq!(l.author_max_chars, 200, "L7 作者名 200 字符（v0 生效，❶）");
    assert_eq!(l.search_max_hits_cap, 1_000, "搜索面上限（PDF-b 定稿，❷）");
    assert_eq!(
        l.snippet_max_chars, 200,
        "摘录上限 200 字符（PDF-b 定稿，❷）"
    );
}

#[test]
fn limits_enforce_family_behaviors() {
    let l = Limits::default();
    // ❷钳制族：生效值 = min；超限以行为学标记告知调用方。
    assert_eq!(l.enforce_search_hits(2_000), (1_000, LimitBehavior::Clamp));
    assert_eq!(l.enforce_search_hits(500), (500, LimitBehavior::Clamp));
    assert_eq!(
        l.enforce_snippet_chars(usize::MAX),
        (200, LimitBehavior::Clamp)
    );
    // ❶硬拒绝族：生效值 < 请求值 = 超限（调用方必须拒绝）。
    assert_eq!(
        l.enforce_author_chars(201),
        (200, LimitBehavior::HardReject)
    );
    assert_eq!(l.enforce_author_chars(80), (80, LimitBehavior::HardReject));
    assert_eq!(
        l.enforce_open_file_bytes((1 << 30) + 1),
        (1 << 30, LimitBehavior::HardReject)
    );
    assert_eq!(
        l.enforce_open_file_bytes(1 << 20),
        (1 << 20, LimitBehavior::HardReject)
    );
}

// ---------------------------------------------------------------------------
// 26. ExtractText 页集形状
// ---------------------------------------------------------------------------

#[test]
fn extract_text_span_shape_and_v0_empty_payload() {
    let mut s = session(3);
    s.set_page_text(PageIndex(1), "page one text")
        .expect("页注入");
    // All = 全档页集。
    let out = s
        .execute(Command::ExtractText {
            target: TextTarget::All,
            source: Source::View,
        })
        .expect("抽取");
    match out {
        Outcome::ExtractedText { pages } => {
            assert_eq!(pages.len(), 3, "All 展开 = 全档页集");
            assert_eq!(
                pages[0],
                (PageIndex(0), String::new()),
                "v0 未注入页 = 空文本载荷（页集形状先行冻结，PDF-c 引擎填充）"
            );
            assert_eq!(pages[1], (PageIndex(1), "page one text".into()));
            assert_eq!(pages[2], (PageIndex(2), String::new()));
        }
        other => panic!("应得 ExtractedText，得 {other:?}"),
    }
    // 区间 = 子页集。
    let span = PageSpan::new(PageIndex(1), PageIndex(1)).unwrap();
    let out2 = s
        .execute(Command::ExtractText {
            target: TextTarget::Pages(span),
            source: Source::Ai,
        })
        .expect("抽取 2");
    assert!(
        matches!(&out2, Outcome::ExtractedText { pages } if pages.len() == 1 && pages[0].0 == PageIndex(1)),
        "单页区间只含该页（AI 源可走同一抽取面）"
    );
    // 注入越界拒绝。
    assert!(
        matches!(
            s.set_page_text(PageIndex(9), "x"),
            Err(CommandError::InvalidPage(PageIndex(9)))
        ),
        "页文本注入同样受页界校验"
    );
}

// ---------------------------------------------------------------------------
// 27. 信封与命令 JSON 面
// ---------------------------------------------------------------------------

#[test]
fn command_envelope_roundtrip_and_json_tags() {
    let cmd = Command::Search {
        query: "needle".into(),
        opts: SearchOptions {
            case_insensitive: true,
            max_hits: 50,
        },
        source: Source::Ai,
    };
    let env = CommandEnvelope::new(42, &cmd).expect("信封构造");
    assert_eq!(env.id, 42);
    // JSON tag 面：externally tagged enum——变体名即 tag（协议草案锚）。
    assert!(
        env.command.get("Search").is_some(),
        "命令 tag = 变体名 Search"
    );
    assert_eq!(
        env.command["Search"]["source"], "ai",
        "source 序列化为 snake_case 溯源名"
    );
    assert_eq!(env.command["Search"]["opts"]["max_hits"], 50);
    // 信封 serde 往返（宿主↔视图层 transport 面）。
    let json = serde_json::to_string(&env).expect("信封序列化");
    let back: CommandEnvelope = serde_json::from_str(&json).expect("信封反序列化");
    assert_eq!(back, env, "信封往返恒等");
    let cmd_back: Command = serde_json::from_value(env.command.clone()).expect("command 反序列化");
    assert_eq!(cmd_back, cmd, "typed 命令经 JSON 往返恒等");
}

// ---------------------------------------------------------------------------
// 28. 引擎集成点 object-safety
// ---------------------------------------------------------------------------

#[test]
fn document_engine_trait_is_object_safe() {
    // 编译期锁：&dyn 可命名（S4 渲染工人池/快照隔离需要对象引用形态）。
    let _witness: fn(&dyn DocumentEngine) = |_| ();
    // trait 方法签名在位（PDF-c 绑定点编译期锚）；复杂签名经别名声明
    // （type_complexity 纪律）。
    type ExtractFn =
        fn(&dyn DocumentEngine, PageSpan) -> Result<Vec<(PageIndex, String)>, CommandError>;
    let _page_geometry: fn(&dyn DocumentEngine, PageIndex) -> Result<PageGeometry, CommandError> =
        |e, i| e.page_geometry(i);
    let _outline: fn(&dyn DocumentEngine) -> Result<OutlineTree, CommandError> = |e| e.outline();
    let _extract: ExtractFn = |e, s| e.extract_text(s);
}

// ---------------------------------------------------------------------------
// 30. serde 面反转 PageSpan 拒收（pdf-b-1r 审计阻断项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn serde_reversed_page_span_is_rejected() {
    // 宿主 JSON 面不变量：派生 Deserialize 曾绕过 new() 校验，畸形信封
    // {"start":3,"end":1} 可致 len() 下溢 panic（debug）/ capacity overflow
    // abort（release，不可捕获）。字段私有 + 自定义 Deserialize 后，反转
    // 区间必须在反序列化边界直接拒收——这是宿主入口的第一道防线。
    let bad = serde_json::from_str::<PageSpan>(r#"{"start":3,"end":1}"#);
    assert!(bad.is_err(), "反转区间 JSON 必须拒收（宿主面不变量）");
    // 合法区间仍可往返（防线不误伤）。
    let good: PageSpan =
        serde_json::from_str(r#"{"start":1,"end":3}"#).expect("合法区间可反序列化");
    assert_eq!(good.len(), 3, "1..=3 闭区间三页");
    assert_eq!(good.start(), PageIndex(1));
    assert_eq!(good.end(), PageIndex(3));
    // 相等页（单页闭区间）合法。
    assert!(
        serde_json::from_str::<PageSpan>(r#"{"start":2,"end":2}"#).is_ok(),
        "start == end = 单页闭区间，合法"
    );
}

// ---------------------------------------------------------------------------
// 31. serde 面反序 Rect 拒收（pdf-b-1r 审计重要项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn serde_inverted_rect_is_rejected() {
    // 反序矩形曾可经 AnnotationPatch.rect 从宿主 JSON 静默落库（派生
    // Deserialize 绕过构造校验）；PDF-d 落盘时将写出非法 /Rect，破坏与
    // Acrobat 的互通承诺。自定义 Deserialize 后必须在边界拒收。
    let bad = serde_json::from_str::<Rect>(r#"{"x0":10,"y0":10,"x1":5,"y1":20}"#);
    assert!(bad.is_err(), "x 轴反序矩形必须拒收（互通性不变量）");
    let bad_y = serde_json::from_str::<Rect>(r#"{"x0":0,"y0":30,"x1":10,"y1":20}"#);
    assert!(bad_y.is_err(), "y 轴反序矩形必须拒收");
    // 合法/退化（零面积）矩形仍可往返。
    let ok: Rect = serde_json::from_str(r#"{"x0":0,"y0":0,"x1":10,"y1":20}"#).expect("合法矩形");
    assert_eq!(ok.width(), 10.0);
    assert!(
        serde_json::from_str::<Rect>(r#"{"x0":5,"y0":5,"x1":5,"y1":5}"#).is_ok(),
        "退化零面积矩形合法（点锚定）"
    );
    // 修复后端到端：反序 rect 的 patch 不可能再构造——经 JSON 全链路验证。
    let mut s = session(2);
    let add = s
        .execute(Command::AnnotateAdd {
            draft: AnnotationDraft {
                page: PageIndex(0),
                kind: AnnotationKind::Note {
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0).unwrap(),
                },
                author: "rev".into(),
                origin: Origin::Human,
            },
            source: Source::View,
        })
        .expect("合法 add");
    let Outcome::AnnotAdded(added) = add else {
        panic!("应得 AnnotAdded")
    };
    // JSON 层构造反序 rect patch → 反序列化失败，永远到不了 execute。
    let inverted_patch = r#"{"rect":{"x0":10,"y0":10,"x1":5,"y1":20}}"#;
    assert!(
        serde_json::from_str::<AnnotationPatch>(inverted_patch).is_err(),
        "反序 rect 的 patch 在 JSON 反序列化边界即被拒——静默落库通道已封死"
    );
    let _ = added; // id 存在性（防未读告警），错误面已由上锁覆盖
}

// ---------------------------------------------------------------------------
// 32. 搜索两模式非重叠语义一致（pdf-b-1r 审计轻微项回归锁）
// ---------------------------------------------------------------------------

#[test]
fn search_overlap_semantics_is_consistent_across_modes() {
    // "aaaa" 搜 "aa"：敏感路径 match_indices = 非重叠 2 次；不敏感路径
    // 曾逐窗口扫描 = 重叠 3 次。两模式必须同语义（非重叠），否则同一
    // 查询仅因大小写开关而计数漂移，限额钳制行为不可预测。
    for ci in [false, true] {
        let mut s = session(1);
        s.set_page_text(PageIndex(0), "aaaa").expect("页注入");
        let out = s
            .execute(Command::Search {
                query: "aa".into(),
                opts: SearchOptions {
                    case_insensitive: ci,
                    max_hits: 10,
                },
                source: Source::View,
            })
            .expect("搜索执行");
        let Outcome::SearchResults { hits, truncated } = out else {
            panic!("应得 SearchResults")
        };
        assert_eq!(
            hits.len(),
            2,
            "非重叠语义与大小写开关无关（ci={ci}：aaaa×aa 恒 2 次）"
        );
        assert!(!truncated);
    }
}

// ---------------------------------------------------------------------------
// 33. attach_engine 页数一致性装配期校验（T1 审计 O3 项回归锁）
// ---------------------------------------------------------------------------

/// mock 引擎（O3 回归锁专用）：页数可配置；数据面保底实现（装配期校验
/// 只读 `page_count`，数据方法在锁内不可达——保底不 panic，误用时以
/// 可判别的空载荷/typed 错误暴露）。
struct StubEngine {
    pages: u32,
}

impl DocumentEngine for StubEngine {
    fn page_count(&self) -> u32 {
        self.pages
    }

    fn page_geometry(&self, _idx: PageIndex) -> Result<PageGeometry, CommandError> {
        Err(CommandError::engine("StubEngine 无几何数据（mock 保底）"))
    }

    fn outline(&self) -> Result<OutlineTree, CommandError> {
        Ok(OutlineTree::new())
    }

    fn extract_text(&self, span: PageSpan) -> Result<Vec<(PageIndex, String)>, CommandError> {
        Ok(span.iter().map(|idx| (idx, String::new())).collect())
    }
}

#[test]
fn attach_engine_validates_page_count_consistency() {
    // O3 回归锁：attach_engine 曾只登记「调用方合同」——双权威不一致时
    // 引擎页少 → 整命令 InvalidPage（显式失败），引擎页多 → 静默漏
    // 末尾页（Search/ExtractText 以 meta.page_count 为上界）。修复后
    // 装配期 typed 拒绝（AttachEngineError::PageCountMismatch 携带双侧
    // 数值），两条静默失败路径收口为单一显式失败。
    let mut s = session(2); // meta.page_count = 2（打开期冻结的权威）

    // 引擎页多（3 > 2）：旧行为 = 静默漏第 3 页（不可见的静默错果）。
    assert_eq!(
        s.attach_engine(Box::new(StubEngine { pages: 3 })),
        Err(AttachEngineError::PageCountMismatch {
            expected: 2,
            engine: 3
        }),
        "引擎页多 → 装配期 typed 拒绝（双侧数值携带）"
    );
    assert!(
        !s.engine_attached(),
        "拒绝后引擎不装配（会话保持 v0 注入路径）"
    );

    // 引擎页少（1 < 2）：旧行为 = 首个越界命令整命令 InvalidPage。
    assert_eq!(
        s.attach_engine(Box::new(StubEngine { pages: 1 })),
        Err(AttachEngineError::PageCountMismatch {
            expected: 2,
            engine: 1
        }),
        "引擎页少 → 装配期 typed 拒绝（与页多同款，方向不对称即锁不住）"
    );
    assert!(!s.engine_attached(), "拒绝无副作用累积");

    // 拒绝后 v0 降级通道完整（注入路径照常服务——拒绝不破坏会话）。
    s.set_page_text(PageIndex(0), "injected")
        .expect("页注入（v0 降级通道完整）");
    let out = s
        .execute(Command::ExtractText {
            target: TextTarget::All,
            source: Source::View,
        })
        .expect("ExtractText 走注入路径");
    let Outcome::ExtractedText { pages } = out else {
        panic!("应得 ExtractedText")
    };
    assert_eq!(pages.len(), 2, "页集形状仍以 meta.page_count 为准");
    assert_eq!(pages[0].1, "injected", "注入文本可见（引擎未装配）");

    // 正控：页数一致 → 装配成功（校验不误伤合法装配）且引擎路径真实
    // 接管（stub 空串旁路注入文本——数据源切换发生，非仅标记在位）。
    s.attach_engine(Box::new(StubEngine { pages: 2 }))
        .expect("页数一致装配成功");
    assert!(s.engine_attached(), "一致引擎在位");
    let out = s
        .execute(Command::ExtractText {
            target: TextTarget::All,
            source: Source::View,
        })
        .expect("ExtractText 走引擎路径");
    let Outcome::ExtractedText { pages } = out else {
        panic!("应得 ExtractedText")
    };
    assert_eq!(pages.len(), 2, "引擎路径页集形状不变（载荷形状两路径一致）");
    assert_eq!(pages[0].1, "", "stub 空串接管（注入文本被旁路）");
}
