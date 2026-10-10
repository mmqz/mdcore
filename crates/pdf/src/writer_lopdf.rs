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

use crate::annotation::{AnnotId, Annotation, AnnotationKind, Point, Quad, Rect, Rgb};
use crate::command::CommandError;
use crate::page::PageIndex;
use crate::security::REASON_ENCRYPTED_WRITE;
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
}

impl LopdfWriter {
    /// 从 PDF 字节构造（全内存）。失败面：
    /// - 解析失败（垃圾字节等）→ `Engine`；
    /// - 加密档（仍加密或曾加密）→ `SecurityDeny`（S5 引擎侧独立防线，
    ///   先于一切写入——与会话层 S5 判定双防线）。
    pub fn from_bytes(pdf: Vec<u8>) -> Result<Self, CommandError> {
        let doc = Document::load_mem(&pdf)
            .map_err(|e| CommandError::engine(format!("lopdf 装载失败：{e}")))?;
        if doc.is_encrypted() || doc.was_encrypted() {
            // 仍加密（口令不可得，lopdf 以未解密形态装载）或曾加密
            // （空口令可解，但文件本体是加密档）——S5：加密档零增量写。
            return Err(CommandError::SecurityDeny {
                reason: REASON_ENCRYPTED_WRITE,
            });
        }
        Ok(Self {
            inc: IncrementalDocument::create_from(pdf, doc),
            staged: HashMap::new(),
            pending: false,
        })
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
        // 重建：产物 = 新基线（后续 flush 在其上追加下一修订——修订内容
        // 恒为「本轮脏对象」，不重复写已落盘对象）。staged **不清**：
        // AnnotId → ObjectId 映射跨 flush 持久（编辑重写复用同对象号）。
        let reloaded = Document::load_mem(&out)
            .map_err(|e| CommandError::engine(format!("产物重载失败：{e}")))?;
        self.inc = IncrementalDocument::create_from(out, reloaded);
        self.pending = false;
        Ok(IncrementalSave {
            bytes_appended,
            prefix_bytes,
        })
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
        AnnotationKind::Ink { strokes } => {
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
            // /BS：实线 1pt（标准边框样式字典）。
            dict.set(
                "BS",
                dictionary! {
                    "Type" => Object::Name(b"Border".to_vec()),
                    "W" => Object::Integer(1),
                    "S" => Object::Name(b"S".to_vec()),
                },
            );
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
