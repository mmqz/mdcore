//! PDF-d — lopdf 标注增量写实装（feature `engine-lopdf`）。
//!
//! 技术范本 = PDF-a spike 探针 3（`IncrementalDocument` 增量写 Highlight →
//! ①原字节前缀逐位保持 ②lopdf 重载在位 ③pdfium 交叉读回）——本模块将
//! 探针路径产品化为 [`LopdfWriter`]（四 kind 标准 Annot 对象，**零私有
//! 扩展**：v0 只用 PDF 32000 标准子型，互通性从第一天成立）。
//!
//! 增量语义（语义锁背书，tests/pdfd_writer.rs）：
//! - **原字节前缀逐位保持**——`save_incremental` 产物以保存前字节为
//!   逐位相同前缀（签名文档友好性的基础；CONTRACT §6 S5 在档承诺），
//!   违约时返回 typed 错误而非产出损坏文件；
//! - 每次 flush 追加**恰好一个修订**：保存后从产物重建
//!   `IncrementalDocument`（产物 = 新基线），后续 flush 在其上追加——
//!   代价 = 每次保存一次重解析（保存路径，非渲染热路径——阅读面性能
//!   不降级）；换来的是修订内容精确 = 本轮脏对象，不重复写已落盘对象；
//! - 幂等：`write_annotation` 同 id = 同对象号重定义（对象 dict 整体
//!   重建自模型 + /Annots 挂载查重；AnnotId→ObjectId 映射跨 flush
//!   持久）——flush 失败重试与编辑重写共用该合同。
//!
//! 安全防线（S5 引擎侧独立防线，与 [`crate::document::PdfSession`]
//! 会话层 S5 判定互补）：
//! - `from_bytes` 拒绝加密档：**仍加密**（口令不可得——lopdf 以未解密
//!   形态装载）与**曾加密**（空口令可解——文件本体仍是加密档）两形态
//!   均拒（`SecurityDeny`，与 S5 同理由码）；
//! - 载荷健全性（annotation.rs 在档的「落盘时由引擎侧校验」承诺）：
//!   空 quads / 空 strokes / 空笔划 / 非有限坐标（NaN/Inf——serde 面
//!   已拒，Rust 直构面兜底）→ `Engine` 错误，不产出非法 PDF。
//!
//! 调用方合同（与 `attach_engine` 页数合同同款纪律）：`from_bytes` 的
//! 字节与宿主会话 `meta.page_count` 必须**同源同档**——写器文档页少时
//! `write_annotation` 以 `InvalidPage` 显式失败，不静默错页挂载。

use std::collections::HashMap;

// 版本钉住的 lopdf 再导出（feature 门内）：测试/宿主经本 crate 引用与
// 实装同版本的 lopdf 实例——避免 dev-dependency 把 lopdf 带进默认测试
// 构建面（默认零引擎依赖纪律保持：default 构建不编译 lopdf）。
pub use lopdf;

use lopdf::{
    dictionary, encode_utf16_be, Dictionary, Document, IncrementalDocument, Object, ObjectId,
};

use crate::annotation::{AnnotId, Annotation, AnnotationKind, Border, Point, Quad, Rect, Rgb};
use crate::command::CommandError;
use crate::page::PageIndex;
use crate::security::{Limits, OpenAssessment, REASON_ENCRYPTED_WRITE};
use crate::writer::{encode_author, pdf_date_string, AnnotationWriter, IncrementalSave};

/// FreeText 默认外观（/DA）：`/Helv 12 Tf 0 g`——Acrobat 对无 /AP 的
/// FreeText 按 /DA 免外观流渲染（v0 不生成 /AP 的前提）。
pub const FREETEXT_DEFAULT_DA: &str = "/Helv 12 Tf 0 g";

/// lopdf 增量写器（产物取回见 [`LopdfWriter::into_bytes`]）。
#[derive(Debug)]
pub struct LopdfWriter {
    /// 增量文档（prev = 当前产物字节 + 解析态；new_document = 待追加修订）。
    inc: IncrementalDocument,
    /// 标注装载位（AnnotId → ObjectId）。**跨 flush 持久**：编辑重写
    /// 复用同一对象号在新修订里重定义（PDF 增量更新语义——后修订覆盖
    /// 前修订，同 id 重写 = 原位替换），不产生重复对象/重复挂载。
    staged: HashMap<AnnotId, ObjectId>,
    /// 自上次成功保存以来是否有新写入（幂等保存判据——与 staged 分离：
    /// staged 是终身映射，pending 是本轮修订判据）。
    pending: bool,
    /// 打开评估（from_bytes 装载后 S2/S6 扫描——PDF-e 接线；产品面
    /// 出口 [`LopdfWriter::assessment`]：宿主取出喂 `PdfSession::open`
    /// 的 assessment 参数）。
    assessment: OpenAssessment,
}

impl LopdfWriter {
    /// 从 PDF 字节构造（全内存；限额 = [`Limits::default`]——自定义
    /// 限额用 [`LopdfWriter::from_bytes_with_limits`]）。失败面：
    /// - S3-L7 输入字节超限 / S3-L4 打开预算超限 → `SecurityDeny`
    ///   （open 家族入口统一检查——先于解析 / 后于扫描，PDF-e 接线）；
    /// - 解析失败（垃圾字节等）→ `Engine`；
    /// - 加密档（仍加密或曾加密）→ `SecurityDeny`（S5 引擎侧独立防线，
    ///   先于一切写入——与会话层 S5 判定双防线）；
    /// - **xref 链不可信档**（lopdf 以扫描重建装载，`xref_start=0`）→
    ///   `Engine`（2026-10-31 行尾污染事故防线——见
    ///   [`LopdfWriter::from_bytes_with_limits`] 内联注释）。
    pub fn from_bytes(pdf: Vec<u8>) -> Result<Self, CommandError> {
        Self::from_bytes_with_limits(pdf, Limits::default())
    }

    /// 带限额构造（打开面入口，限额注入形态——测试/宿主同款）：
    /// S3-L7 输入字节 ❶ → 装载 → S5 加密拒 → **xref 链守卫** →
    /// S2/S6 扫描 → S3-L4 打开预算 ❶。
    pub fn from_bytes_with_limits(pdf: Vec<u8>, limits: Limits) -> Result<Self, CommandError> {
        // S3-L7 输入层（open 家族入口统一——与 open_scan /
        // PdfiumEngine::open 同款；先于解析：敌意大文档的无界解析面
        // 在入口收口）。
        limits.check_open_input_bytes(pdf.len() as u64)?;
        let started = crate::security::portable_now();
        let doc = Document::load_mem(&pdf)
            .map_err(|e| CommandError::engine(format!("lopdf 装载失败：{e}")))?;
        if doc.is_encrypted() || doc.was_encrypted() {
            // 仍加密（口令不可得，lopdf 以未解密形态装载）或曾加密
            // （空口令可解，但文件本体是加密档）——S5：加密档零增量写。
            return Err(CommandError::SecurityDeny {
                reason: REASON_ENCRYPTED_WRITE,
            });
        }
        // xref 链守卫（2026-10-31 行尾污染事故）：lopdf 对 xref 不可解析
        // 的档退化为**全文件扫描重建**装载——能读（宽容），但
        // `Document::xref_start == 0` 标记「无已知 xref 起点」。在其上
        // 做增量写是静默产毒：`new_from_prev` 无从回指（lopdf 对
        // xref_start=0 刻意省略 /Prev——写 0 会把链指到文件头），产物
        // xref 只覆盖本轮新对象，目录/页树经链不可达——严格阅读器打
        // 不开，宽容阅读器靠再次扫描重建才可见（实测事故形态：
        // read_annotations 静默 parsed:0，「写入成功但读不回来」）。
        // 增量语义要求前序 xref 链可信——重建装载档在入口响亮拒绝，
        // 不静默产出结构损坏的产物。常见成因：二进制 PDF 被行尾转换
        // 污染（Windows git autocrlf 检出未标 binary 的文件——仓内已
        // 加 .gitattributes 防复发）或 xref 表真损坏（修复源文件后重试）。
        if doc.xref_start == 0 {
            return Err(CommandError::engine(
                "xref 链不可信：档以扫描重建装载（无已知 xref 起点），增量写产物将丢失 \
                 /Prev 链导致页树不可达。常见成因：二进制被行尾转换污染（如 Windows \
                 git autocrlf）或 xref 损坏。请重取未污染原件或修复源文件后重试",
            ));
        }
        // S2/S6 扫描（装载后——告警/表单探针随写器携带；加密拒在前，
        // 携带的 assessment 恒为非加密态）。
        let assessment = crate::security_scan::scan_document(&doc, &limits);
        // S3-L4 打开预算（装载 + 扫描整段计时）。
        limits.check_open_budget(started)?;
        Ok(Self {
            inc: IncrementalDocument::create_from(pdf, doc),
            staged: HashMap::new(),
            pending: false,
            assessment,
        })
    }

    /// 打开评估（S2 危险族告警 + S6 表单探针 + S5 加密旗标——
    /// from_bytes 装载时扫描；喂 `PdfSession::open` 的 assessment
    /// 参数——PDF-e 接线的产品面出口）。
    pub fn assessment(&self) -> &OpenAssessment {
        &self.assessment
    }

    /// 取回当前产物字节（原字节 + 已 flush 修订；未 flush 时 = 输入
    /// 原样）。消耗形态——直接使用面的终止取回。会话内写器的非消耗
    /// 取回见 [`LopdfWriter::product_bytes`]。
    pub fn into_bytes(self) -> Vec<u8> {
        self.product_bytes()
    }

    /// 当前产物字节的非消耗取回（供交叉验证：lopdf 重载 / pdfium 读回；
    /// 会话包装器模式下 flush 后仍可取——宿主保存事务的真实形态：flush
    /// 后会话继续存活，产物取回不能要求消耗写器）。
    pub fn product_bytes(&self) -> Vec<u8> {
        // IncrementalDocument 字段私有 → 经只读视图拷出（保存路径单次
        // 拷贝，可接受；无 drift 风险——该视图即内部权威字节）。
        self.inc.get_prev_documents_bytes().to_vec()
    }

    /// 当前产物长度（零脏 flush 的幂等统计基线）。
    fn product_len(&self) -> u64 {
        self.inc.get_prev_documents_bytes().len() as u64
    }

    /// 页号 → 前序修订页对象 id（lopdf get_pages 为 1 基；越界 =
    /// `InvalidPage`——引擎层不信任调用方页号）。
    fn page_object_id(&self, page: PageIndex) -> Result<ObjectId, CommandError> {
        let key = page
            .0
            .checked_add(1)
            .ok_or(CommandError::InvalidPage(page))?; // u32::MAX + 1 防御
        self.inc
            .get_prev_documents()
            .get_pages()
            .get(&key)
            .copied()
            .ok_or(CommandError::InvalidPage(page))
    }

    /// 标注对象挂载到页 /Annots（页对象克隆入新修订 + 引用追加）。
    /// /Annots 三形态全覆盖：缺省 → 建；直接数组 → 追加；间接引用 →
    /// 数组对象克隆入新修订后追加。重复挂载（同 id 重写路径）查重跳过。
    fn mount_on_page(&mut self, page_id: ObjectId, annot_id: ObjectId) -> Result<(), CommandError> {
        // 页对象入新修订（幂等：已在位则无操作）——先克隆后解析：首次
        // 写入时 new_document 尚无页对象，解析须在克隆后的 new_document
        // 视图上进行。
        self.inc
            .opt_clone_object_to_new_document(page_id)
            .map_err(|e| CommandError::engine(format!("页对象克隆失败：{e}")))?;
        // 挂载点形态解析（new_document 视图）：
        enum Site {
            Missing,
            Direct,
            Indirect(ObjectId),
        }
        let site = {
            let page = self
                .inc
                .new_document
                .get_object(page_id)
                .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
                .as_dict()
                .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
            match page.get(b"Annots") {
                Ok(Object::Array(_)) => Site::Direct,
                Ok(Object::Reference(id)) => Site::Indirect(*id),
                Ok(other) => {
                    return Err(CommandError::engine(format!(
                        "/Annots 非数组/引用形态：{other:?}"
                    )))
                }
                Err(_) => Site::Missing,
            }
        };
        let arr = match site {
            Site::Missing => {
                let page = self
                    .inc
                    .new_document
                    .get_object_mut(page_id)
                    .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
                    .as_dict_mut()
                    .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
                page.set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
                return Ok(());
            }
            Site::Direct => {
                let page = self
                    .inc
                    .new_document
                    .get_object_mut(page_id)
                    .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
                    .as_dict_mut()
                    .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
                page.get_mut(b"Annots")
                    .map_err(|e| CommandError::engine(format!("/Annots 读取失败：{e}")))?
                    .as_array_mut()
                    .map_err(|e| CommandError::engine(format!("/Annots 非数组：{e}")))?
            }
            Site::Indirect(arr_id) => {
                self.inc
                    .opt_clone_object_to_new_document(arr_id)
                    .map_err(|e| CommandError::engine(format!("/Annots 数组克隆失败：{e}")))?;
                self.inc
                    .new_document
                    .get_object_mut(arr_id)
                    .map_err(|e| CommandError::engine(format!("/Annots 数组读取失败：{e}")))?
                    .as_array_mut()
                    .map_err(|e| CommandError::engine(format!("/Annots 非数组：{e}")))?
            }
        };
        push_if_absent(arr, annot_id);
        Ok(())
    }
}

impl AnnotationWriter for LopdfWriter {
    fn write_annotation(
        &mut self,
        page: PageIndex,
        annot: &Annotation,
    ) -> Result<(), CommandError> {
        let page_id = self.page_object_id(page)?;
        let dict = annot_object(annot)?;
        // 装载：新对象 or 原位替换（同 id 重写 = 编辑路径——同对象号在新
        // 修订重定义，后修订覆盖前修订，不产生重复对象）。
        let annot_obj_id = match self.staged.get(&annot.id) {
            Some(&id) => {
                self.inc
                    .new_document
                    .set_object(id, Object::Dictionary(dict));
                id
            }
            None => {
                let id = self.inc.new_document.add_object(Object::Dictionary(dict));
                self.staged.insert(annot.id, id);
                id
            }
        };
        self.pending = true;
        self.mount_on_page(page_id, annot_obj_id)
    }

    fn save_incremental(&mut self) -> Result<IncrementalSave, CommandError> {
        let prefix_bytes = self.product_len();
        // 零脏幂等：本轮无新写入 → 不落空修订（bytes_appended = 0）。
        if !self.pending {
            return Ok(IncrementalSave {
                bytes_appended: 0,
                prefix_bytes,
            });
        }
        let mut out = Vec::new();
        self.inc
            .save_to(&mut out)
            .map_err(|e| CommandError::engine(format!("lopdf 增量保存失败：{e}")))?;
        // 增量语义核心断言：产物前缀 == 保存前字节（逐位）。违约 =
        // 引擎面错误（不产出、不污染基线），不 panic（typed 错误纪律）。
        let prefix_ok = out.len() >= prefix_bytes as usize
            && out[..prefix_bytes as usize] == *self.inc.get_prev_documents_bytes();
        if !prefix_ok {
            return Err(CommandError::engine(
                "增量前缀违约：lopdf 产物未逐位保持原字节前缀",
            ));
        }
        let bytes_appended = out.len() as u64 - prefix_bytes;
        // 产物自检（2026-10-31 行尾污染事故防线）：重载后页树必须可达。
        // 零页 = 产物 xref 链断裂（目录/页对象解析不到——「写入成功但
        // 阅读器看不到」的静默产毒形态）。已知的唯一触发路径（前序档
        // 扫描重建装载 → /Prev 省略）已被 from_bytes 的 xref 链守卫在
        // 入口拒绝，此检查为纵深防御（兜未来写面回归），拒绝交付而非
        // 污染基线。on-error 写器状态保持保存前形态（inc 未替换、
        // pending 仍真）——可修复后重试。
        let reloaded = Document::load_mem(&out)
            .map_err(|e| CommandError::engine(format!("产物重载失败：{e}")))?;
        if reloaded.get_pages().is_empty() {
            return Err(CommandError::engine(
                "增量产物自检失败：重载后页树为空（产物 xref 链断裂，页对象经链不可达）\
                 ——拒绝交付结构损坏产物。该态不应出现（写入口已拒重建装载档），\
                 如触发请附样张提 issue",
            ));
        }
        // 重建：产物 = 新基线（后续 flush 在其上追加下一修订——修订内容
        // 恒为「本轮脏对象」，不重复写已落盘对象）。staged **不清**：
        // AnnotId → ObjectId 映射跨 flush 持久（编辑重写复用同对象号）。
        self.inc = IncrementalDocument::create_from(out, reloaded);
        self.pending = false;
        Ok(IncrementalSave {
            bytes_appended,
            prefix_bytes,
        })
    }
    /// 当前产物字节（PDF-e 视图层：宿主取回面——前缀 + 既往修订）。
    fn product_bytes(&self) -> Vec<u8> {
        LopdfWriter::product_bytes(self)
    }
}

// ---------------------------------------------------------------------------
// 模型 → PDF Annot 对象（纯函数；四 kind 标准子型，零私有扩展）
// ---------------------------------------------------------------------------

/// 模型标注 → PDF Annot 字典。kind 载荷映射 + 公共字段（/T origin 编码
/// /M 日期串 /F 打印位）。载荷健全性校验在此（annotation.rs 在档的
/// 「落盘时引擎侧校验」承诺）。
fn annot_object(annot: &Annotation) -> Result<Dictionary, CommandError> {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"Annot".to_vec()));
    match &annot.kind {
        AnnotationKind::Highlight { quads, color } => {
            if quads.is_empty() {
                return Err(CommandError::engine("Highlight 载荷不健全：空四边形列表"));
            }
            dict.set("Subtype", Object::Name(b"Highlight".to_vec()));
            dict.set("Rect", rect_array(&quad_envelope(quads)?));
            // QuadPoints：每 quad 8 数（四角点环序展开）。
            let mut points = Vec::with_capacity(quads.len() * 8);
            for quad in quads {
                push_quad_points(&mut points, quad)?;
            }
            dict.set("QuadPoints", Object::Array(points));
            dict.set("C", color_array(color));
        }
        AnnotationKind::Note { rect } => {
            dict.set("Subtype", Object::Name(b"Text".to_vec()));
            dict.set("Rect", rect_array(rect));
        }
        AnnotationKind::FreeText { rect, text } => {
            dict.set("Subtype", Object::Name(b"FreeText".to_vec()));
            dict.set("Rect", rect_array(rect));
            dict.set("Contents", text_string(text));
            dict.set("DA", Object::string_literal(FREETEXT_DEFAULT_DA));
        }
        AnnotationKind::Ink {
            strokes,
            color,
            width,
        } => {
            if strokes.is_empty() {
                return Err(CommandError::engine("Ink 载荷不健全：空笔划列表"));
            }
            if strokes.iter().any(Vec::is_empty) {
                return Err(CommandError::engine("Ink 载荷不健全：存在空笔划"));
            }
            dict.set("Subtype", Object::Name(b"Ink".to_vec()));
            dict.set("Rect", rect_array(&stroke_envelope(strokes)?));
            let mut list = Vec::with_capacity(strokes.len());
            for stroke in strokes {
                let mut pts = Vec::with_capacity(stroke.len() * 2);
                for p in stroke {
                    push_point(&mut pts, p)?;
                }
                list.push(Object::Array(pts));
            }
            dict.set("InkList", Object::Array(list));
            // #45：/C 标注颜色（Highlight 同款 0-1 归一 RGB——阅读器
            // 互通面；缺省 = 旧视觉蓝）。
            dict.set("C", color_array(color));
            // /BS：实线可变宽（#45：W 从写死 1pt 改为载荷宽——入列已
            // 钦制 0.5..=12.0，写前不再二次防御）。
            dict.set(
                "BS",
                dictionary! {
                    "Type" => Object::Name(b"Border".to_vec()),
                    "W" => Object::Real(*width),
                    "S" => Object::Name(b"S".to_vec()),
                },
            );
        }
        // ===== S2-2 七新子型（issue #103 任务 2——全标准子型零私有扩展）=====
        AnnotationKind::Underline { quads, color } => {
            if quads.is_empty() {
                return Err(CommandError::engine("Underline 载荷不健全：空四边形列表"));
            }
            dict.set("Subtype", Object::Name(b"Underline".to_vec()));
            dict.set("Rect", rect_array(&quad_envelope(quads)?));
            let mut points = Vec::with_capacity(quads.len() * 8);
            for quad in quads {
                push_quad_points(&mut points, quad)?;
            }
            dict.set("QuadPoints", Object::Array(points));
            dict.set("C", color_array(color));
        }
        AnnotationKind::StrikeOut { quads, color } => {
            if quads.is_empty() {
                return Err(CommandError::engine("StrikeOut 载荷不健全：空四边形列表"));
            }
            dict.set("Subtype", Object::Name(b"StrikeOut".to_vec()));
            dict.set("Rect", rect_array(&quad_envelope(quads)?));
            let mut points = Vec::with_capacity(quads.len() * 8);
            for quad in quads {
                push_quad_points(&mut points, quad)?;
            }
            dict.set("QuadPoints", Object::Array(points));
            dict.set("C", color_array(color));
        }
        AnnotationKind::Rectangle { rect, border } => {
            dict.set("Subtype", Object::Name(b"Square".to_vec()));
            dict.set("Rect", rect_array(rect));
            dict.set("C", color_array(&border.color));
            dict.set("BS", border_dict(border));
        }
        AnnotationKind::Ellipse { rect, border } => {
            dict.set("Subtype", Object::Name(b"Circle".to_vec()));
            dict.set("Rect", rect_array(rect));
            dict.set("C", color_array(&border.color));
            dict.set("BS", border_dict(border));
        }
        AnnotationKind::Line { from, to, border } => {
            check_finite(from)?;
            check_finite(to)?;
            dict.set("Subtype", Object::Name(b"Line".to_vec()));
            dict.set("Rect", rect_array(&line_envelope(from, to)?));
            dict.set("L", line_array(from, to));
            dict.set("C", color_array(&border.color));
            dict.set("BS", border_dict(border));
        }
        AnnotationKind::Arrow { from, to, border } => {
            // PDF 无原生 /Arrow 子型——标准 /Line + /LE 终止符（零私有扩展
            // 的通行做法；Acrobat/Edge 渲染带箭头端）。
            check_finite(from)?;
            check_finite(to)?;
            dict.set("Subtype", Object::Name(b"Line".to_vec()));
            dict.set("Rect", rect_array(&line_envelope(from, to)?));
            dict.set("L", line_array(from, to));
            dict.set("C", color_array(&border.color));
            dict.set("BS", border_dict(border));
            dict.set(
                "LE",
                Object::Array(vec![
                    Object::Name(b"None".to_vec()),
                    Object::Name(b"ClosedArrow".to_vec()),
                ]),
            );
        }
        AnnotationKind::Stamp { rect, text } => {
            dict.set("Subtype", Object::Name(b"Stamp".to_vec()));
            dict.set("Rect", rect_array(rect));
            dict.set("Contents", text_string(text));
            // 标准图章名（/Name /Approved 等）映射 v0 不做——阅读器回退按
            // /Contents 显示（登记：docs/pdf-annotate.md）。
        }
    }
    // 公共字段：/T origin 编码（Acrobat 作者栏可读）+ /M 值时钟日期串 +
    // /F bit 2（print——无此位标注默认不打印；spike 探针 3 同款）。
    dict.set(
        "T",
        text_string(&encode_author(&annot.author, annot.origin)),
    );
    dict.set(
        "M",
        Object::string_literal(pdf_date_string(annot.modified_ms)),
    );
    dict.set("F", Object::Integer(4));
    Ok(dict)
}

/// 模型矩形 → /Rect 数组（PDF 用户空间直通——模型即用户空间坐标，
/// 无 Y 翻转；Rect 不变量 x0<=x1 已在类型层）。
fn rect_array(rect: &Rect) -> Object {
    Object::Array(vec![
        Object::Real(rect.x0()),
        Object::Real(rect.y0()),
        Object::Real(rect.x1()),
        Object::Real(rect.y1()),
    ])
}

/// quad 包络（全部 quad 角点的 min/max——Highlight /Rect 语义）。
fn quad_envelope(quads: &[Quad]) -> Result<Rect, CommandError> {
    envelope(quads.iter().flat_map(|q| q.points.iter()))
}

/// stroke 包络（全部笔划采样点的 min/max——Ink /Rect 语义）。
fn stroke_envelope(strokes: &[Vec<Point>]) -> Result<Rect, CommandError> {
    envelope(strokes.iter().flatten())
}

/// 点集包络 → Rect（min/max 恒有序——构造失败分支不可达，仍以 typed
/// 错误兜底而非 panic；调用方已保证非空）。
fn envelope<'a>(points: impl Iterator<Item = &'a Point>) -> Result<Rect, CommandError> {
    let mut iter = points.peekable();
    if iter.peek().is_none() {
        return Err(CommandError::engine("包络计算：空点集"));
    }
    let mut x0 = f32::MAX;
    let mut y0 = f32::MAX;
    let mut x1 = f32::MIN;
    let mut y1 = f32::MIN;
    for p in iter {
        check_finite(p)?;
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Rect::new(x0, y0, x1, y1)
        .map_err(|_| CommandError::engine("包络构造失败（不可达：min/max 恒有序）"))
}

/// 单点坐标对追加（有限性校验——NaN/Inf 不进 PDF 数值面）。
fn push_point(out: &mut Vec<Object>, p: &Point) -> Result<(), CommandError> {
    check_finite(p)?;
    out.push(Object::Real(p.x));
    out.push(Object::Real(p.y));
    Ok(())
}

/// 单 quad 四角点展开（QuadPoints 环序 = 模型 [Point;4] 序直通）。
fn push_quad_points(out: &mut Vec<Object>, quad: &Quad) -> Result<(), CommandError> {
    for p in &quad.points {
        push_point(out, p)?;
    }
    Ok(())
}

fn check_finite(p: &Point) -> Result<(), CommandError> {
    if p.x.is_finite() && p.y.is_finite() {
        Ok(())
    } else {
        Err(CommandError::engine("坐标非有限值（NaN/Inf 不落盘）"))
    }
}

/// 边框字典（/BS /W 宽 + /S /S 实线——形状/线形共用；#45 同族语义）。
fn border_dict(border: &Border) -> Object {
    Object::Dictionary(dictionary! {
        "Type" => Object::Name(b"Border".to_vec()),
        "W" => Object::Real(border.width),
        "S" => Object::Name(b"S".to_vec()),
    })
}

/// 线端点对 → /L [x1 y1 x2 y2]。
fn line_array(from: &Point, to: &Point) -> Object {
    Object::Array(vec![
        Object::Real(from.x),
        Object::Real(from.y),
        Object::Real(to.x),
        Object::Real(to.y),
    ])
}

/// 线端点包络 → /Rect（min/max 恒有序）。
fn line_envelope(from: &Point, to: &Point) -> Result<Rect, CommandError> {
    envelope([from, to].into_iter())
}

/// sRGB → /C [r g b]（0-1 归一）。
fn color_array(color: &Rgb) -> Object {
    Object::Array(vec![
        Object::Real(f32::from(color.r) / 255.0),
        Object::Real(f32::from(color.g) / 255.0),
        Object::Real(f32::from(color.b) / 255.0),
    ])
}

/// PDF 文本字符串（PDF 32000 7.9.2）：ASCII → 字面量（PDFDocEncoding
/// 子集）；非 ASCII → UTF-16BE + BOM（Acrobat 系阅读器互通的稳妥形态
/// ——lopdf encode_utf16_be 自带 BOM）。
fn text_string(s: &str) -> Object {
    if s.is_ascii() {
        Object::string_literal(s)
    } else {
        let mut bytes = Vec::new();
        encode_utf16_be(s, &mut bytes);
        Object::String(bytes, lopdf::StringFormat::Literal)
    }
}

/// PDF 文本串解码（S2-2 读取面）：UTF-16BE（BOM FE FF）→ String；
/// 否则 PDFDocEncoding 近似（ASCII 子集直读）。写面 [`text_string`]
/// 的可逆对应。
pub fn decode_pdf_text(bytes: &[u8]) -> String {
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

/// 引用去重追加（同对象已在 /Annots → 跳过：编辑重写路径不产生重复
/// 引用；Acrobat 对重复引用宽容，但干净数据面从源头不做重复）。
fn push_if_absent(arr: &mut Vec<Object>, id: ObjectId) {
    if !arr
        .iter()
        .any(|o| matches!(o.as_reference(), Ok(found) if found == id))
    {
        arr.push(Object::Reference(id));
    }
}

// ---------------------------------------------------------------------------
// S2-2 删除面（annotate.rs PdfAnnotator 消费——摘引用 + null 重定义）
// ---------------------------------------------------------------------------

impl LopdfWriter {
    /// staged 对象号（AnnotId → 本会话写入对象——删除面消费：会话
    /// 新增标注的摘除定位）。
    pub(crate) fn staged_object_id(&self, id: AnnotId) -> Option<ObjectId> {
        self.staged.get(&id).copied()
    }

    /// 删除标注（增量语义）：页对象克隆入新修订 → /Annots 数组过滤掉
    /// 该引用 → 标注对象 null 重定义（spec 7.5.10：后修订 null = 逻辑
    /// 删除）。产物前缀逐位保持不变（既有语义锁不破）——对象字节仍在
    /// 前修订（增量语义物理事实），引用图上已死，阅读器不再显示。
    pub(crate) fn remove_annotation(
        &mut self,
        page: PageIndex,
        annot_obj: ObjectId,
    ) -> Result<(), CommandError> {
        // 页对象 id（1 基换算——write_annotation 的 page_object_id 私有
        // 面同款逻辑[增量文档视图差异：remove 须在 prev 视图取页表]）。
        let key = page
            .0
            .checked_add(1)
            .ok_or(CommandError::InvalidPage(page))?;
        let page_id = self
            .inc
            .get_prev_documents()
            .get_pages()
            .get(&key)
            .copied()
            .ok_or(CommandError::InvalidPage(page))?;
        // 页对象入新修订（幂等克隆）。
        self.inc
            .opt_clone_object_to_new_document(page_id)
            .map_err(|e| CommandError::engine(format!("页对象克隆失败：{e}")))?;
        // /Annots 三形态过滤（缺省=无删面幂等 / 直接数组 / 间接引用——
        // mount_on_page 同构）。
        let arr_id: Option<ObjectId> = {
            let p = self
                .inc
                .new_document
                .get_object(page_id)
                .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
                .as_dict()
                .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
            match p.get(b"Annots") {
                Ok(Object::Reference(id)) => Some(*id),
                Ok(Object::Array(_)) => None,
                _ => return Ok(()),
            }
        };
        match arr_id {
            Some(id) => {
                self.inc
                    .opt_clone_object_to_new_document(id)
                    .map_err(|e| CommandError::engine(format!("/Annots 数组克隆失败：{e}")))?;
                let arr = self
                    .inc
                    .new_document
                    .get_object_mut(id)
                    .map_err(|e| CommandError::engine(format!("/Annots 数组读取失败：{e}")))?
                    .as_array_mut()
                    .map_err(|e| CommandError::engine(format!("/Annots 非数组：{e}")))?;
                arr.retain(|o| !matches!(o, Object::Reference(r) if *r == annot_obj));
            }
            None => {
                let p = self
                    .inc
                    .new_document
                    .get_object_mut(page_id)
                    .map_err(|e| CommandError::engine(format!("页对象读取失败：{e}")))?
                    .as_dict_mut()
                    .map_err(|e| CommandError::engine(format!("页对象非字典：{e}")))?;
                if let Ok(Object::Array(arr)) = p.get_mut(b"Annots") {
                    arr.retain(|o| !matches!(o, Object::Reference(r) if *r == annot_obj));
                }
            }
        }
        // 标注对象 null 重定义（新修订显式 null 覆盖前修订定义——spec
        // 7.5.10 逻辑删除；staged 同号对象同样置 null）。
        self.inc.new_document.set_object(annot_obj, Object::Null);
        // 零脏短路戳破（remove 也是待保存内容）。
        self.pending = true;
        Ok(())
    }
}
