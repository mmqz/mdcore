//! S2-2（PDF-f2）· 标注写回扩展——外部读取 + PdfAnnotator 门面
//! （feature `engine-lopdf`）。
//!
//! # 定位
//!
//! issue #103 任务 2：在 PDF-d 增量写器（4 kind）之上补齐
//! **读外部 PDF 的标注**（`read_annotations`）+ **独立标注器门面**
//! （[`PdfAnnotator`]：add / remove / list / save——issue 草图形状，
//! id 面用 crate 流通货币 [`AnnotId`] 而非字符串）。新增 7 kind 的
//! 模型/写面分别在 [`crate::annotation`]（`AnnotationKind` 加性变体）
//! 与 [`crate::writer_lopdf`]（`annot_object` 新臂）——本模块是**读取
//! 反向映射 + 门面编排**层。
//!
//! # 读取语义（`read_annotations`）
//!
//! - 遍历页树 → 各页 `/Annots` → 逐字典解析 `/Subtype` → 模型 11 kind
//!   反向映射（含 `/T` 作者 → [`Origin`] 可逆解码、`/M` 日期串 → epoch
//!   ms 反解析、`/QuadPoints` → quads、`/L` → 端点、`/BS` → 边框）；
//! - **非标注子型跳过并计数**（`/Widget` 表单控件、`/Link` 链接——
//!   阅读面消费，非标注模型；skipped 登记）；
//! - 未知/畸形子型（无 `/Subtype`、载荷缺字段、非有限坐标）**跳过并
//!   计数**——读面宽容（敌意文档不炸），不静默：`ReadReport` 全量登记
//!   （S2-1 报告纪律同款）；
//! - **零页树响亮失败**（2026-10-31 行尾污染事故防线）：装载成功但
//!   页表为空（目录/页对象经 xref 链不可达——修订链断裂或结构损坏）
//!   → `Engine` 错误而非静默 parsed:0（PDF 规范页数 ≥ 1，零页必为结构
//!   异常——「读到 0 条标注」与「没有标注」不可区分是事故隐身所在）；
//! - id 分配：**文档序单调**（页升序 → /Annots 序），`AnnotId` 从 1 起
//!   ——与外部对象号解耦（对象号是存储细节，模型身份是会话身份）。
//!
//! # 删除语义（增量写）
//!
//! PDF 增量修订**不能改写字节**——删除 = 新修订里页 `/Annots` 数组
//! 摘除该引用 + 标注对象以 `null` 重定义（spec 7.5.10：后修订 null =
//! 逻辑删除）。产物前缀逐位保持不变（既有语义锁不破）；对象字节仍
//! 在前修订里（不可消除——增量语义的物理事实），但引用图上已死，
//! 阅读器（Acrobat/pdfium）不再显示。全量保存路径（save-to-new-file
//! 后再开）才会物理消除。
//!
//! # 门面编排（`PdfAnnotator`）
//!
//! ```text
//! PdfAnnotator::open(bytes)?
//!   .list(None)?              // 外部 + 本会话新增（页过滤可选）
//!   .add(page, kind, author, origin)?   // → AnnotId
//!   .remove(id)?              // 外部或新增皆可删（摘引用 + null 重定义）
//!   .save()?                  // 增量修订（adds/edits 写入 + removes 摘除）
//! ```
//!
//! 会话命令面（六命令）零触碰——`PdfSession` 的 `AnnotateAdd/Edit/List`
//! 对新 kind 即刻可用（模型/写面已扩展）；删除命令属命令面变更，
//! 由 Lead 决策（本门面先行供宿主直用）。

use std::collections::BTreeMap;

use lopdf::{Document, Object, ObjectId};

use crate::annotation::{
    AnnotId, Annotation, AnnotationKind, Border, Origin, Point, Quad, Rect, Rgb,
};
use crate::command::CommandError;
use crate::page::PageIndex;
use crate::security::Limits;
use crate::writer::AnnotationWriter;
use crate::writer_lopdf::LopdfWriter;

// ---------------------------------------------------------------------------
// 读取报告（宽容读面 + 全量登记——不静默）
// ---------------------------------------------------------------------------

/// 读取报告：解析成功/跳过/时间戳回退计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadReport {
    /// 成功解析为模型 11 kind 的标注数。
    pub parsed: usize,
    /// 跳过的非标注子型（/Widget /Link /Popup——阅读面消费，非本模型）。
    pub skipped_non_markup: usize,
    /// 跳过的未知/畸形子型（无 /Subtype / 载荷缺字段 / 非有限坐标）。
    pub skipped_malformed: usize,
    /// /M 日期串不可反解析（时间戳回退 0）的条数——内容面仍保留。
    pub date_fallback: usize,
}

// ---------------------------------------------------------------------------
// 外部标注读取
// ---------------------------------------------------------------------------

/// 读取 PDF 全部标注（页升序 + 页内 /Annots 序）。
///
/// 安全镜像打开面（S3-L7 → 解析 → S5 加密拒 → S3-L4 预算——与
/// [`LopdfWriter::from_bytes_with_limits`] 同序）。
pub fn read_annotations(
    pdf: Vec<u8>,
    limits: Limits,
) -> Result<(Vec<Annotation>, ReadReport), CommandError> {
    limits.check_open_input_bytes(pdf.len() as u64)?;
    let started = crate::security::portable_now();
    let doc = Document::load_mem(&pdf)
        .map_err(|e| CommandError::engine(format!("lopdf 装载失败：{e}")))?;
    if doc.is_encrypted() || doc.was_encrypted() {
        return Err(CommandError::SecurityDeny {
            reason: crate::security::REASON_ENCRYPTED_WRITE,
        });
    }
    limits.check_open_budget(started)?;
    let mut out = Vec::new();
    let mut report = ReadReport::default();
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    // 零页守卫（事故防线，理由见模块文档）：零页 = 结构异常（链断裂/
    // 损坏），响亮失败而非静默 parsed:0。
    if pages.is_empty() {
        return Err(CommandError::engine(
            "页树不可达：档装载成功但页表为空（目录/页对象未能经 xref 链解析\
             ——修订链断裂或结构损坏）。零页档非合法形态（PDF 规范页数 ≥ 1），\
             拒绝静默返回空标注集",
        ));
    }
    let mut next_id: u64 = 1;
    for (page_idx, &page_id) in pages.iter().enumerate() {
        let annots = page_annots(&doc, page_id);
        for entry in annots {
            let dict = match &entry {
                Object::Reference(id) => match doc.get_object(*id).map(|o| o.as_dict().ok()) {
                    Ok(Some(d)) => d,
                    _ => {
                        report.skipped_malformed += 1;
                        continue;
                    }
                },
                Object::Dictionary(d) => d,
                _ => {
                    report.skipped_malformed += 1;
                    continue;
                }
            };
            let subtype = match dict.get(b"Subtype") {
                Ok(Object::Name(n)) => n.clone(),
                _ => {
                    report.skipped_malformed += 1;
                    continue;
                }
            };
            // 非标注子型（阅读面消费）。
            if matches!(subtype.as_slice(), b"Widget" | b"Link" | b"Popup") {
                report.skipped_non_markup += 1;
                continue;
            }
            let kind = match parse_kind(dict, &subtype) {
                Some(k) => k,
                None => {
                    report.skipped_malformed += 1;
                    continue;
                }
            };
            // 作者（/T → 可逆解码）+ 时间戳（/M → 反解析，回退 0 登记）。
            let (author, origin) = match dict.get(b"T") {
                Ok(Object::String(bytes, _)) => {
                    let text = crate::writer_lopdf::decode_pdf_text(bytes);
                    let (a, o) = crate::writer::decode_author(&text);
                    (a.to_string(), o)
                }
                _ => (String::new(), Origin::Human),
            };
            let modified_ms = match dict.get(b"M") {
                Ok(Object::String(bytes, _)) => {
                    match parse_pdf_date(&crate::writer_lopdf::decode_pdf_text(bytes)) {
                        Some(ms) => ms,
                        None => {
                            report.date_fallback += 1;
                            0
                        }
                    }
                }
                _ => 0,
            };
            out.push(Annotation {
                id: AnnotId(next_id),
                page: PageIndex(page_idx as u32),
                kind,
                author,
                origin,
                created_ms: modified_ms,
                modified_ms,
            });
            next_id += 1;
            report.parsed += 1;
        }
    }
    Ok((out, report))
}

/// 页 /Annots 数组内容（两形态；缺省 = 空）。
fn page_annots(doc: &Document, page_id: ObjectId) -> Vec<Object> {
    let Ok(Some(page)) = doc.get_object(page_id).map(|o| o.as_dict().ok()) else {
        return Vec::new();
    };
    match page.get(b"Annots") {
        Ok(Object::Array(arr)) => arr.clone(),
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// 标注字典 → 模型 kind（11 子型反向映射；失败 = None 由调用方计数）。
fn parse_kind(dict: &lopdf::Dictionary, subtype: &[u8]) -> Option<AnnotationKind> {
    match subtype {
        b"Highlight" => Some(AnnotationKind::Highlight {
            quads: parse_quads(dict)?,
            color: parse_color(dict)?,
        }),
        b"Underline" => Some(AnnotationKind::Underline {
            quads: parse_quads(dict)?,
            color: parse_color(dict)?,
        }),
        b"StrikeOut" => Some(AnnotationKind::StrikeOut {
            quads: parse_quads(dict)?,
            color: parse_color(dict)?,
        }),
        b"Text" => Some(AnnotationKind::Note {
            rect: parse_rect(dict)?,
        }),
        b"FreeText" => Some(AnnotationKind::FreeText {
            rect: parse_rect(dict)?,
            text: parse_text(dict)?,
        }),
        b"Ink" => {
            let strokes = parse_strokes(dict)?;
            // /C 缺省 = 旧视觉蓝（写入面 serde 缺省同值——往返诚实）。
            let color = parse_color(dict).unwrap_or(Rgb {
                r: 0x1a,
                g: 0x73,
                b: 0xe8,
            });
            let width = parse_border(dict).map(|b| b.width).unwrap_or(2.0);
            Some(AnnotationKind::Ink {
                strokes,
                color,
                width,
            })
        }
        b"Square" => Some(AnnotationKind::Rectangle {
            rect: parse_rect(dict)?,
            border: parse_border_required(dict)?,
        }),
        b"Circle" => Some(AnnotationKind::Ellipse {
            rect: parse_rect(dict)?,
            border: parse_border_required(dict)?,
        }),
        b"Line" => {
            let (from, to) = parse_line(dict)?;
            let border = parse_border_required(dict)?;
            // /LE 终止符区分 Line/Arrow（ClosedArrow/OpenArrow → Arrow）。
            let is_arrow = match dict.get(b"LE") {
                Ok(Object::Array(le)) => le.iter().any(|o| {
                    matches!(o, Object::Name(n)
                        if n == b"ClosedArrow" || n == b"OpenArrow")
                }),
                _ => false,
            };
            if is_arrow {
                Some(AnnotationKind::Arrow { from, to, border })
            } else {
                Some(AnnotationKind::Line { from, to, border })
            }
        }
        b"Stamp" => Some(AnnotationKind::Stamp {
            rect: parse_rect(dict)?,
            text: parse_text(dict).unwrap_or_default(),
        }),
        _ => None,
    }
}

/// /QuadPoints → quads（8 数一组；非有限 = None）。
fn parse_quads(dict: &lopdf::Dictionary) -> Option<Vec<Quad>> {
    let arr = match dict.get(b"QuadPoints") {
        Ok(Object::Array(a)) => a,
        _ => return None,
    };
    if arr.is_empty() || arr.len() % 8 != 0 {
        return None;
    }
    let mut quads = Vec::with_capacity(arr.len() / 8);
    let mut nums = Vec::with_capacity(8);
    for o in arr {
        nums.push(as_f32(o)?);
    }
    let (chunks, _) = nums.as_chunks::<8>();
    for chunk in chunks {
        quads.push(Quad {
            points: [
                Point {
                    x: chunk[0],
                    y: chunk[1],
                },
                Point {
                    x: chunk[2],
                    y: chunk[3],
                },
                Point {
                    x: chunk[4],
                    y: chunk[5],
                },
                Point {
                    x: chunk[6],
                    y: chunk[7],
                },
            ],
        });
    }
    Some(quads)
}

/// /C → Rgb（0-1 归一 → 0-255；缺省 = None[调用方决定回退]）。
fn parse_color(dict: &lopdf::Dictionary) -> Option<Rgb> {
    let arr = match dict.get(b"C") {
        Ok(Object::Array(a)) => a,
        _ => return None,
    };
    if arr.len() < 3 {
        return None;
    }
    Some(Rgb {
        r: (as_f32(&arr[0])? * 255.0).round() as u8,
        g: (as_f32(&arr[1])? * 255.0).round() as u8,
        b: (as_f32(&arr[2])? * 255.0).round() as u8,
    })
}

/// /Rect → Rect（4 数；反序 = None）。
fn parse_rect(dict: &lopdf::Dictionary) -> Option<Rect> {
    let arr = match dict.get(b"Rect") {
        Ok(Object::Array(a)) => a,
        _ => return None,
    };
    if arr.len() < 4 {
        return None;
    }
    Rect::new(
        as_f32(&arr[0])?,
        as_f32(&arr[1])?,
        as_f32(&arr[2])?,
        as_f32(&arr[3])?,
    )
    .ok()
}

/// /Contents → 文本（PDFDocEncoding/UTF-16BE 双形态；缺省 None）。
fn parse_text(dict: &lopdf::Dictionary) -> Option<String> {
    match dict.get(b"Contents") {
        Ok(Object::String(bytes, _)) => Some(crate::writer_lopdf::decode_pdf_text(bytes)),
        _ => None,
    }
}

/// /InkList → 笔划（每划点序列）。
fn parse_strokes(dict: &lopdf::Dictionary) -> Option<Vec<Vec<Point>>> {
    let arr = match dict.get(b"InkList") {
        Ok(Object::Array(a)) => a,
        _ => return None,
    };
    if arr.is_empty() {
        return None;
    }
    let mut strokes = Vec::with_capacity(arr.len());
    for stroke in arr {
        let pts = match stroke {
            Object::Array(pa) => pa,
            _ => return None,
        };
        if pts.is_empty() || pts.len() % 2 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(pts.len() / 2);
        let mut i = 0;
        while i < pts.len() {
            out.push(Point {
                x: as_f32(&pts[i])?,
                y: as_f32(&pts[i + 1])?,
            });
            i += 2;
        }
        strokes.push(out);
    }
    Some(strokes)
}

/// /L → (from, to)。
fn parse_line(dict: &lopdf::Dictionary) -> Option<(Point, Point)> {
    let arr = match dict.get(b"L") {
        Ok(Object::Array(a)) => a,
        _ => return None,
    };
    if arr.len() < 4 {
        return None;
    }
    Some((
        Point {
            x: as_f32(&arr[0])?,
            y: as_f32(&arr[1])?,
        },
        Point {
            x: as_f32(&arr[2])?,
            y: as_f32(&arr[3])?,
        },
    ))
}

/// /BS → Border（/W 宽 + /C 色；色缺省黑[Acrobat 缺省同值]）。
fn parse_border(dict: &lopdf::Dictionary) -> Option<Border> {
    let bs = match dict.get(b"BS") {
        Ok(Object::Dictionary(d)) => d,
        _ => return None,
    };
    let width = match bs.get(b"W") {
        Ok(o) => as_f32(o)?,
        Err(_) => 1.0,
    };
    let color = parse_color(dict).unwrap_or(Rgb { r: 0, g: 0, b: 0 });
    Some(Border { color, width })
}

/// /BS 必需形态（Square/Circle/Line 的边框为模型必需字段）。
fn parse_border_required(dict: &lopdf::Dictionary) -> Option<Border> {
    // /BS 缺省 = 1pt 黑边（Acrobat 缺省外观——宽容读取登记面）。
    Some(parse_border(dict).unwrap_or(Border {
        color: Rgb { r: 0, g: 0, b: 0 },
        width: 1.0,
    }))
}

/// 数值对象 → f32（非有限 = None）。
fn as_f32(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(v) => Some(*v as f32),
        Object::Real(v) => Some(*v),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

/// PDF 日期串（`D:YYYYMMDDHHmmSSOHH'mm`）→ epoch ms（本地时区近似——
/// 精度面登记：偏移解析到分钟，无时区库依赖）。
fn parse_pdf_date(s: &str) -> Option<u64> {
    let body = s.strip_prefix("D:").unwrap_or(s);
    if body.len() < 4 || !body[..4].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let num = |range: std::ops::Range<usize>, default: u32| -> u32 {
        if body.len() >= range.end && body[range.clone()].chars().all(|c| c.is_ascii_digit()) {
            body[range].parse().unwrap_or(default)
        } else {
            default
        }
    };
    let year = num(0..4, 2000);
    let month = num(4..6, 1).clamp(1, 12);
    let day = num(6..8, 1).clamp(1, 31);
    let hour = num(8..10, 0);
    let minute = num(10..12, 0);
    let second = num(12..14, 0);
    // 简历日历 → 天数（1970 起；闰年面完整）。
    let days = days_from_civil(year as i64, month as i64, day as i64)?;
    let secs = days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    // 模型时间戳 = epoch **毫秒**（与会话时钟同单位）；时区偏移
    // （O±HH'mm 尾缀）近似按 UTC 计（无时区库；精度面登记）。
    u64::try_from(secs.max(0))
        .ok()
        .and_then(|s| s.checked_mul(1000))
}

/// 公历 → 1970 起天数（Howard Hinnant days_from_civil 算法——无 Chrono
/// 依赖的确定性日历）。
fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

// ---------------------------------------------------------------------------
// PdfAnnotator 门面
// ---------------------------------------------------------------------------

/// 独立标注器（issue #103 任务 2 草图形状；id = crate 流通货币）。
///
/// 打开（读外部标注 + 安全镜像）→ add / remove / list（外部 + 本会话
/// 合并视图）→ save（增量修订：新增/编辑写入 + 删除摘除）。
#[derive(Debug)]
pub struct PdfAnnotator {
    writer: LopdfWriter,
    /// 模型视图（外部 + 新增；id 单调——外部 1..=N，新增 N+1..）。
    annots: BTreeMap<AnnotId, Annotation>,
    next_id: u64,
    /// 外部标注 id → (页号, 对象号)——remove 的摘除定位面。
    external: BTreeMap<AnnotId, (PageIndex, ObjectId)>,
    /// 待删除集（save 时统一摘除——外部对象号或 staged 对象号）。
    pending_removals: Vec<(PageIndex, ObjectId)>,
    /// 标注修订号（#305：与 [`crate::document::PdfSession::annotation_rev`]
    /// 同款合同——save 成功且实际追加字节时 +1；渲染缓存键控源头）。
    rev: u64,
    /// 页数缓存（open 定位遍历时取——页界校验权威面）。
    page_count_hint: u32,
    read_report: ReadReport,
}

impl PdfAnnotator {
    /// 打开（读取外部标注；限额 = [`Limits::default`]）。
    pub fn open(pdf: Vec<u8>) -> Result<Self, CommandError> {
        Self::open_with_limits(pdf, Limits::default())
    }

    /// 带限额打开。写器与读取共用安全面（S3-L7/S5/S3-L4 双过——写器
    /// 面先开[产物基线]，读取面再开[模型视图]）。
    pub fn open_with_limits(pdf: Vec<u8>, limits: Limits) -> Result<Self, CommandError> {
        let writer = LopdfWriter::from_bytes_with_limits(pdf.clone(), limits)?;
        let (annots, read_report) = read_annotations(pdf, limits)?;
        let mut next_id = 1u64;
        for annot in &annots {
            next_id = next_id.max(annot.id.0 + 1);
        }
        let (annots, ext_ids, page_count) = locate_external(&writer, annots)?;
        let mut external = BTreeMap::new();
        for (id, page, obj) in ext_ids {
            external.insert(id, (page, obj));
        }
        Ok(Self {
            writer,
            annots: annots.into_iter().map(|a| (a.id, a)).collect(),
            next_id,
            external,
            pending_removals: Vec::new(),
            rev: 0,
            page_count_hint: page_count,
            read_report,
        })
    }

    /// 标注修订号（#305：save 推进——与 PdfSession 同款语义合同，
    /// rev 相同 ⟺ 产物字节相同；渲染缓存以此键控）。
    pub fn annotation_rev(&self) -> u64 {
        self.rev
    }

    /// 读取报告（打开时外部标注解析面）。
    pub fn read_report(&self) -> &ReadReport {
        &self.read_report
    }

    /// 列出标注（外部 + 本会话新增；页过滤可选——None = 全部）。
    pub fn list(&self, page: Option<PageIndex>) -> Vec<&Annotation> {
        self.annots
            .values()
            .filter(|a| page.is_none_or(|p| a.page == p))
            .collect()
    }

    /// 新增标注（页界校验 + id 分配；时间戳 = 0[值时钟由会话面管——
    /// 门面无时钟注入面，宿主经 PdfSession 走 Add 命令可得值时钟]）。
    pub fn add(
        &mut self,
        page: PageIndex,
        kind: AnnotationKind,
        author: impl Into<String>,
        origin: Origin,
    ) -> Result<AnnotId, CommandError> {
        let page_count = self.page_count();
        if page.0 >= page_count {
            return Err(CommandError::InvalidPage(page));
        }
        let id = AnnotId(self.next_id);
        self.next_id += 1;
        let annot = Annotation {
            id,
            page,
            kind,
            author: author.into(),
            origin,
            created_ms: 0,
            modified_ms: 0,
        };
        self.writer.write_annotation(page, &annot)?;
        self.annots.insert(id, annot);
        Ok(id)
    }

    /// 删除标注（外部或本会话新增皆可；立即从模型视图移除，物理摘除
    /// 延迟到 [`PdfAnnotator::save`]——增量修订摘引用 + null 重定义）。
    pub fn remove(&mut self, id: AnnotId) -> Result<Annotation, CommandError> {
        let annot = self
            .annots
            .remove(&id)
            .ok_or(CommandError::AnnotNotFound(id))?;
        // 摘除定位双源：外部标注（open 时对象号定位表）or 本会话新增
        // （写器 staged 映射——含已 flush 对象[staged 跨 flush 持久]）。
        if let Some(&(page, obj)) = self.external.get(&id) {
            self.pending_removals.push((page, obj));
        } else if let Some(obj) = self.writer.staged_object_id(id) {
            self.pending_removals.push((annot.page, obj));
        }
        Ok(annot)
    }

    /// 增量保存（新增/编辑写入 + 删除摘除；前缀逐位保持语义锁沿用）。
    /// save 成功且实际追加字节时标注修订号 +1（#305——产物变了 rev
    /// 才变；零追加幂等 save 不 bump）。
    pub fn save(&mut self) -> Result<crate::writer::IncrementalSave, CommandError> {
        // 删除摘除（页 /Annots 数组过滤 + 对象 null 重定义）。
        for (page, obj) in std::mem::take(&mut self.pending_removals) {
            self.writer.remove_annotation(page, obj)?;
        }
        let stats = self.writer.save_incremental()?;
        if stats.bytes_appended > 0 {
            self.rev = self.rev.saturating_add(1);
        }
        Ok(stats)
    }

    /// 产物字节（前缀 + 既往修订——宿主取回面）。
    pub fn product_bytes(&self) -> Vec<u8> {
        self.writer.product_bytes()
    }

    fn page_count(&self) -> u32 {
        self.page_count_hint
    }
}

// ---------------------------------------------------------------------------
// 内部：外部标注对象定位 + 写器删除面
// ---------------------------------------------------------------------------

/// locate_external 产物：模型 + 外部定位表（id → 页/对象号）+ 页数缓存。
type LocatedExternal = (Vec<Annotation>, Vec<(AnnotId, PageIndex, ObjectId)>, u32);

/// 外部标注定位：模型 id → (页, /Annots 数组内对象号)。文档序重走
/// read_annotations 的遍历序（页升序 + /Annots 序）与 id 分配序一致——
/// 双遍对位（简单可靠；外部 N ≤ 数千，成本可忽略）。附带页数返回
/// （门面页界校验缓存）。
fn locate_external(
    writer: &LopdfWriter,
    annots: Vec<Annotation>,
) -> Result<LocatedExternal, CommandError> {
    // 经写器产物重载（lopdf Document 视图——写器字段私有面的绕行；
    // 单次成本 = 一次 parse，打开路径非热路径）。
    let bytes = writer.product_bytes();
    let doc = Document::load_mem(&bytes)
        .map_err(|e| CommandError::engine(format!("定位遍历装载失败：{e}")))?;
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let page_count = pages.len() as u32;
    let mut located = Vec::new();
    let mut idx = 0usize;
    'pages: for (page_idx, &page_id) in pages.iter().enumerate() {
        for entry in page_annots(&doc, page_id) {
            if let Object::Reference(obj) = entry {
                if let Some(annot) = annots.get(idx) {
                    located.push((annot.id, PageIndex(page_idx as u32), obj));
                    idx += 1;
                    if idx >= annots.len() {
                        break 'pages;
                    }
                }
            }
        }
    }
    Ok((annots, located, page_count))
}

// ---------------------------------------------------------------------------
// 模块测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_roundtrip_known_values() {
        // pdf_date_string 正反映射锚点（写入面同一时钟约定的闭环）。
        let ms = crate::writer::pdf_date_string(0);
        assert!(ms.starts_with("D:1970"), "epoch 0 → 1970：{ms}");
        assert_eq!(parse_pdf_date("D:19700101000000Z"), Some(0));
        assert_eq!(parse_pdf_date("D:20260102030405"), Some(1_767_323_045_000));
        assert_eq!(parse_pdf_date("garbage"), None);
        assert_eq!(parse_pdf_date(""), None);
        assert_eq!(parse_pdf_date("D:20"), None);
    }

    #[test]
    fn days_from_civil_epoch_anchors() {
        assert_eq!(days_from_civil(1970, 1, 1), Some(0));
        assert_eq!(days_from_civil(1970, 1, 2), Some(1));
        assert_eq!(days_from_civil(1969, 12, 31), Some(-1));
        // 闰年锚：2000-02-29 存在（2000 是闰年）。
        assert!(days_from_civil(2000, 2, 29).is_some());
    }
}
