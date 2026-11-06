//! PDF 原生投影适配器（issue #239 Phase 4——Emphasis → /Highlight +
//! QuadPoints）。
//!
//! 两阶段契约实装：
//! - [`PdfNativeAdapter::plan_with_geometry`]：几何注入面（纯函数——
//!   字符布局由调用方提供；plan 的语义分类/报告/计划组装与引擎无关，
//!   本地全测）；
//! - `NativeAnnotationAdapter::plan`（feature `engine-pdfium`）：几何
//!   源 = pdfium `positioned_chars`（生产路径）；
//! - `NativeAnnotationAdapter::execute`（feature `engine-lopdf`）：
//!   源指纹核对（StaleSource 防线）→ `mdc_pdf::PdfAnnotator` 增量写
//!   → **新文件字节**（不落盘——落盘是宿主的显式动作）。

use crate::PageChars;
use mdc_evidence::{DocumentRef, Fingerprinter, Fnv64};
use mdc_overlay::anchor::PrimaryLocator;
use mdc_overlay::contract::{
    AdapterError, ProjectionOperation, ProjectionPlan, ProjectionReport, ProjectionResult,
    ProjectionTarget,
};
use mdc_overlay::mark::OverlayEntry;
use mdc_overlay::Format;

/// 投影产物的 /T（author）缺省值（溯源约定——Acrobat 标注作者栏
/// 可读；宿主可用 [`PdfNativeAdapter::with_author`] 覆盖）。
pub const PROJECTION_AUTHOR_DEFAULT: &str = "mdcore-overlay";

/// PDF 原生标注适配器。
#[derive(Debug, Clone)]
pub struct PdfNativeAdapter {
    author: String,
}

impl Default for PdfNativeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfNativeAdapter {
    /// 适配器身份（固有面——trait `id()` 委托此处）。
    pub fn adapter_id(&self) -> &'static str {
        "mdc-overlay-pdf"
    }

    pub fn new() -> Self {
        PdfNativeAdapter {
            author: PROJECTION_AUTHOR_DEFAULT.to_string(),
        }
    }

    /// 自定义 /T 作者（溯源面）。
    pub fn with_author(author: impl Into<String>) -> Self {
        PdfNativeAdapter {
            author: author.into(),
        }
    }

    /// /T 作者（execute 写入面读取——常开取值面）。
    pub fn author(&self) -> &str {
        &self.author
    }

    /// plan（几何注入面——纯函数，零引擎 feature）。
    ///
    /// 语义分类纪律（三原则③「不能表达的标注不能悄悄丢弃」）：
    /// - Emphasis + PdfSpan + 页布局在场 + 几何派生成功 →
    ///   `operations`（PdfHighlight target）+ `exported`；
    /// - 任何其他组合 → `unsupported`（id + 人读原因）——**绝不静默
    ///   跳过**；
    /// - `source_fingerprint` = 源字节现算指纹（execute 前核对的
    ///   基线）。
    pub fn plan_with_geometry(
        &self,
        source_doc: &DocumentRef,
        source_bytes: &[u8],
        entries: &[OverlayEntry],
        pages: &[PageChars],
    ) -> Result<ProjectionPlan, AdapterError> {
        let mut operations = Vec::new();
        let mut report = ProjectionReport::default();
        for entry in entries {
            let mut unsupported = |reason: String| {
                report.unsupported.push((entry.id.clone(), reason));
            };
            match &entry.mark {
                mdc_overlay::OverlayMark::Emphasis { anchor, .. } => {
                    if anchor.format != Format::Pdf {
                        unsupported(format!(
                            "Emphasis 锚点格式 {} 与 PDF 适配器不符",
                            anchor.format.as_str()
                        ));
                        continue;
                    }
                    if let Err(e) = anchor.validate() {
                        unsupported(format!("锚点不变量破坏：{e}"));
                        continue;
                    }
                    let PrimaryLocator::PdfSpan {
                        page, char_start, char_end, ..
                    } = &anchor.primary
                    else {
                        unsupported("锚点主定位非 PdfSpan".into());
                        continue;
                    };
                    let Some(page_chars) = pages.iter().find(|p| p.page == *page) else {
                        unsupported(format!("第 {page} 页的字符布局不在注入源"));
                        continue;
                    };
                    match mdc_overlay::geometry::quads_for_range(
                        &page_chars.chars,
                        *char_start as usize,
                        *char_end as usize,
                        &page_chars.page_box,
                    ) {
                        Ok(quads) if !quads.is_empty() => {
                            operations.push(ProjectionOperation {
                                annotation_id: entry.id.clone(),
                                mark: entry.mark.clone(),
                                target: ProjectionTarget::PdfHighlight {
                                    page: *page,
                                    quads,
                                },
                            });
                            report.exported.push(entry.id.clone());
                        }
                        Ok(_) => {
                            unsupported("几何派生产出空四边形集（区间空？）".into());
                        }
                        Err(e) => {
                            unsupported(format!("锚点 → QuadPoints 派生失败：{e}"));
                        }
                    }
                }
                mdc_overlay::OverlayMark::Comment { .. } => unsupported(
                    "PDF Comment → /Text Note 投影未实装（本轮仅 Emphasis → /Highlight——issue #239 §四映射表）".into(),
                ),
                mdc_overlay::OverlayMark::Freehand { .. } => unsupported(
                    "PDF 适配器 v0 不支持 Freehand（→ /Ink 属能力矩阵后续轮）".into(),
                ),
                mdc_overlay::OverlayMark::Region { .. } => unsupported(
                    "PDF 适配器 v0 不支持 Region（→ /Square 属能力矩阵后续轮）".into(),
                ),
            }
        }
        let plan = ProjectionPlan {
            source_doc: source_doc.clone(),
            source_fingerprint: Fnv64.fingerprint(source_bytes).0,
            adapter_id: self.adapter_id().to_string(),
            operations,
            report,
        };
        mdc_overlay::contract::validate_plan(&plan)?;
        Ok(plan)
    }
}

impl mdc_overlay::NativeAnnotationAdapter for PdfNativeAdapter {
    fn id(&self) -> &str {
        self.adapter_id()
    }

    fn format(&self) -> Format {
        Format::Pdf
    }

    fn capabilities(&self) -> mdc_overlay::AdapterCapabilities {
        mdc_overlay::AdapterCapabilities {
            emphasis: true,
            comment: false,
            freehand: false,
            region: false,
        }
    }

    /// plan（生产路径——feature `engine-pdfium`：字符几何 =
    /// `positioned_chars`；只取标注涉及页）。
    fn plan(
        &self,
        source_doc: &DocumentRef,
        source_bytes: &[u8],
        entries: &[OverlayEntry],
    ) -> Result<ProjectionPlan, AdapterError> {
        #[cfg(feature = "engine-pdfium")]
        {
            use mdc_overlay::anchor::PrimaryLocator;
            use mdc_overlay::geometry::{CharBox, PageBox};
            use mdc_pdf::engine::DocumentEngine;

            // 只取涉及页（几何源按需——大文档不整本扫描）。
            let mut wanted: Vec<u32> = Vec::new();
            for e in entries {
                if let Some(a) = e.mark.anchor() {
                    if let PrimaryLocator::PdfSpan { page, .. } = &a.primary {
                        if !wanted.contains(page) {
                            wanted.push(*page);
                        }
                    }
                }
            }
            let engine = mdc_pdf::engine_pdfium::PdfiumEngine::open(source_bytes.to_vec())
                .map_err(|e| AdapterError::Engine(format!("pdfium 打开失败：{e:?}")))?;
            // 页界预检：锚点指向不存在的页 = 该标注不能表达（plan_with_
            // geometry 按「布局不在注入源」走 unsupported——一坏不拖
            // 全批），不是 plan 整体失败。
            let page_count = engine.page_count();
            let mut pages = Vec::new();
            for page_no in wanted {
                if page_no == 0 || page_no > page_count {
                    continue;
                }
                let idx = mdc_pdf::page::PageIndex(page_no - 1);
                let chars = engine.positioned_chars(idx).map_err(|e| {
                    AdapterError::Engine(format!("第 {page_no} 页字符布局失败：{e:?}"))
                })?;
                let geom = engine
                    .page_geometry(idx)
                    .map_err(|e| AdapterError::Engine(format!("第 {page_no} 页几何失败：{e:?}")))?;
                pages.push(PageChars {
                    page: page_no,
                    chars: chars
                        .into_iter()
                        .map(|c| CharBox {
                            ch: c.ch,
                            left: c.left as f64,
                            bottom: c.bottom as f64,
                            right: c.right as f64,
                            top: c.top as f64,
                            font_size: c.font_size as f64,
                        })
                        .collect(),
                    page_box: PageBox {
                        width_pt: geom.width_pt as f64,
                        height_pt: geom.height_pt as f64,
                    },
                });
            }
            self.plan_with_geometry(source_doc, source_bytes, entries, &pages)
        }
        #[cfg(not(feature = "engine-pdfium"))]
        {
            let _ = (source_doc, source_bytes, entries);
            Err(AdapterError::InvalidInput(
                "plan() 需要 feature engine-pdfium（字符几何来自 pdfium positioned_chars）；无库环境请用 plan_with_geometry 注入几何".into(),
            ))
        }
    }

    /// execute（feature `engine-lopdf`）：指纹核对 → 增量写 →
    /// 新文件字节。
    fn execute(
        &self,
        source_bytes: &[u8],
        plan: &ProjectionPlan,
    ) -> Result<ProjectionResult, AdapterError> {
        #[cfg(feature = "engine-lopdf")]
        {
            use mdc_pdf::annotate::PdfAnnotator;
            use mdc_pdf::annotation::{
                AnnotationKind, Origin, Point as PdfPoint, Quad as PdfQuad, Rgb,
            };

            // 版本核对（issue §六铁律）：源字节现算指纹 ≠ 计划指纹 =
            // 拒绝执行（旧计划不写新文件）。
            let found = Fnv64.fingerprint(source_bytes).0;
            if found != plan.source_fingerprint {
                return Err(AdapterError::StaleSource {
                    expected: plan.source_fingerprint.clone(),
                    found,
                });
            }
            if plan.operations.is_empty() {
                return Err(AdapterError::EmptyPlan);
            }

            let mut annotator = PdfAnnotator::open(source_bytes.to_vec())
                .map_err(|e| AdapterError::Engine(format!("打开失败：{e:?}")))?;
            for op in &plan.operations {
                match &op.target {
                    ProjectionTarget::PdfHighlight { page, quads } => {
                        let pdf_quads: Vec<PdfQuad> = quads
                            .iter()
                            .map(|q| PdfQuad {
                                points: [
                                    PdfPoint {
                                        x: q.points[0].x as f32,
                                        y: q.points[0].y as f32,
                                    },
                                    PdfPoint {
                                        x: q.points[1].x as f32,
                                        y: q.points[1].y as f32,
                                    },
                                    PdfPoint {
                                        x: q.points[2].x as f32,
                                        y: q.points[2].y as f32,
                                    },
                                    PdfPoint {
                                        x: q.points[3].x as f32,
                                        y: q.points[3].y as f32,
                                    },
                                ],
                            })
                            .collect();
                        annotator
                            .add(
                                mdc_pdf::page::PageIndex(page - 1),
                                AnnotationKind::Highlight {
                                    quads: pdf_quads,
                                    color: Rgb {
                                        r: crate::EMPHASIS_COLOR.r,
                                        g: crate::EMPHASIS_COLOR.g,
                                        b: crate::EMPHASIS_COLOR.b,
                                    },
                                },
                                self.author.clone(),
                                Origin::Human,
                            )
                            .map_err(|e| {
                                AdapterError::Engine(format!(
                                    "标注 {} 写入失败：{e:?}",
                                    op.annotation_id.as_str()
                                ))
                            })?;
                    }
                    ProjectionTarget::PdfTextNote { .. } => {
                        return Err(AdapterError::InvalidInput(format!(
                            "标注 {} 的 PdfTextNote 目标未实装（v0 计划不产出——防御拒绝）",
                            op.annotation_id.as_str()
                        )));
                    }
                }
            }
            annotator
                .save()
                .map_err(|e| AdapterError::Engine(format!("增量保存失败：{e:?}")))?;
            Ok(ProjectionResult {
                bytes: annotator.product_bytes(),
                report: plan.report.clone(),
            })
        }
        #[cfg(not(feature = "engine-lopdf"))]
        {
            let _ = (source_bytes, plan);
            Err(AdapterError::InvalidInput(
                "execute() 需要 feature engine-lopdf（lopdf 增量写）".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdc_evidence::{ContentFingerprint, DocId, EngineKind, VersionStamp};
    use mdc_overlay::anchor::SemanticAnchor;
    use mdc_overlay::geometry::{CharBox, PageBox, Quad};
    use mdc_overlay::mark::{AnnotationId, OverlayMark};
    use mdc_overlay::NativeAnnotationAdapter;

    fn doc() -> DocumentRef {
        DocumentRef::new(
            DocId::new("tf-0002"),
            EngineKind::Pdf,
            VersionStamp(1),
            ContentFingerprint("fnv64:doc".into()),
        )
    }

    fn page_chars() -> PageChars {
        let chars: Vec<CharBox> = "逐项对比招标要求"
            .chars()
            .enumerate()
            .map(|(i, c)| CharBox {
                ch: c,
                left: 72.0 + i as f64 * 12.0,
                bottom: 700.0,
                right: 72.0 + (i as f64 + 1.0) * 12.0,
                top: 712.0,
                font_size: 12.0,
            })
            .collect();
        PageChars {
            page: 1,
            chars,
            page_box: PageBox {
                width_pt: 595.0,
                height_pt: 842.0,
            },
        }
    }

    fn emphasis_entry(id: &str, cs: u32, ce: u32) -> OverlayEntry {
        OverlayEntry::new(
            AnnotationId::new(id).unwrap(),
            doc(),
            OverlayMark::Emphasis {
                anchor: SemanticAnchor::new(
                    Format::Pdf,
                    PrimaryLocator::PdfSpan {
                        page: 1,
                        line: 1,
                        char_start: cs,
                        char_end: ce,
                    },
                    vec![],
                    "fnv64:seg",
                )
                .unwrap(),
                note: None,
            },
            1_000,
        )
        .unwrap()
    }

    fn freehand_entry(id: &str) -> OverlayEntry {
        let span = mdc_overlay::GeometricSpan::new(
            mdc_overlay::CoordSpace::PdfPage {
                width_pt: 595.0,
                height_pt: 842.0,
                origin: mdc_overlay::PdfOrigin::BottomLeft,
            },
            Some(1),
            vec![Quad::from_rect(0.0, 0.0, 10.0, 10.0)],
            None,
            "fnv64:g",
        )
        .unwrap();
        OverlayEntry::new(
            AnnotationId::new(id).unwrap(),
            doc(),
            OverlayMark::Freehand {
                document: doc(),
                span,
                note: None,
            },
            1_000,
        )
        .unwrap()
    }

    #[test]
    fn plan_classifies_emphasis_and_freehand() {
        let a = PdfNativeAdapter::new();
        let entries = vec![emphasis_entry("m1", 0, 3), freehand_entry("m2")];
        let plan = a
            .plan_with_geometry(&doc(), b"pdf-bytes", &entries, &[page_chars()])
            .unwrap();
        assert_eq!(plan.report.exported.len(), 1);
        assert_eq!(plan.report.exported[0].as_str(), "m1");
        // Freehand 显式 unsupported（验收第 7 条的形状面）。
        assert_eq!(plan.report.unsupported.len(), 1);
        assert_eq!(plan.report.unsupported[0].0.as_str(), "m2");
        assert!(plan.report.unsupported[0].1.contains("Freehand"));

        // exported 目标携带 QuadPoints（验收第 4 条形状面）。
        match &plan.operations[0].target {
            ProjectionTarget::PdfHighlight { page, quads } => {
                assert_eq!(*page, 1);
                assert_eq!(quads.len(), 1);
                assert_eq!(quads[0].points[0].x, 72.0);
            }
            other => panic!("非预期目标：{other:?}"),
        }
        // 指纹绑定（execute 核对基线）。
        assert!(plan.source_fingerprint.starts_with("fnv64:"));
        assert_eq!(plan.adapter_id, "mdc-overlay-pdf");
    }

    #[test]
    fn plan_page_not_in_geometry_source_is_unsupported() {
        let a = PdfNativeAdapter::new();
        let mut e = emphasis_entry("m1", 0, 3);
        if let OverlayMark::Emphasis { anchor, .. } = &mut e.mark {
            anchor.primary = PrimaryLocator::PdfSpan {
                page: 9,
                line: 1,
                char_start: 0,
                char_end: 3,
            };
        }
        let plan = a
            .plan_with_geometry(&doc(), b"x", &[e], &[page_chars()])
            .unwrap();
        assert!(plan.report.unsupported[0].1.contains("第 9 页"));
        assert!(plan.operations.is_empty());
    }

    #[test]
    fn execute_without_feature_is_explicit_error() {
        // 默认 feature 面（CI 本地步骤可能不开 lopdf）：显式错误而非
        // 静默不可用。
        let a = PdfNativeAdapter::new();
        let plan = a
            .plan_with_geometry(&doc(), b"x", &[emphasis_entry("m1", 0, 3)], &[page_chars()])
            .unwrap();
        #[cfg(not(feature = "engine-lopdf"))]
        {
            let err = a.execute(b"x", &plan).unwrap_err();
            assert!(err.to_string().contains("engine-lopdf"));
        }
        #[cfg(feature = "engine-lopdf")]
        {
            let _ = a.execute(b"x", &plan);
        }
    }
}
