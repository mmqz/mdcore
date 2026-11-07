//! PDF 标注写会话（#302 阶段 2·④——桌面接 mdc-pdf 增量写面）。
//!
//! # 定位
//!
//! 页面视图（[`crate::pdf_pages::PdfPagesView`]）的**非 GUI 写面协调器**：
//! 右键拖框按当前工具（[`AnnotateTool`]——高亮/下划线/便签三手势，
//! issue #302 验收 1）→ 屏幕矩形经 [`PageTransform`]（#304）转用户空间
//! → `PdfSession` 六命令面（`AnnotateAdd`）→ `flush_annotations` 增量保存
//! （原字节前缀逐位保持——签名有效，#227 语义锁背书）→ 产物落盘。
//! 渲染刷新（flush 后旧位图问题）由视图侧 RenderKey 键控（#305）承接。
//!
//! 本模块**零 GPUI 依赖**（纯 Rust + mdc-pdf engine-lopdf 面）——整条
//! 写路径可在无 GUI 环境全量测试（`tests/pdf_annotate_e2e.rs`）；
//! GPUI 交互层（拖框手势）在 [`crate::pdf_pages`]，那里只有编译面
//! 验证（沙箱无 GUI——诚实边界，见该模块文档）。
//!
//! # 安全门序（与读路径 #277 同纪律，写面前置）
//!
//! ```text
//! S2/S6 open_scan（对象图评估——危险族告警透出）
//!   → S5 加密态拒（写面：加密档不可批注——stamp/forms 同款双拒）
//!     → S3-L7 LopdfWriter 输入门（限额 + xref 链守卫——扫描重建档拒写）
//!       → PdfSession::open + attach_writer（命令面六命令 + flush 事务）
//! ```
//!
//! # 几何源（lopdf 直读——无引擎环境可用）
//!
//! 页几何（MediaBox/CropBox/`/Rotate`）经 lopdf 字典直读（含 `/MediaBox`
//! `/CropBox` 的**父节点继承**——页树上层定义下层可省略）。pdfium 在位
//! 时视图侧引擎几何（`engine_pdfium::page_geometry`，#304 起带双盒）与
//! 本解析同源同档；无引擎环境（测试/降级）本解析即权威。
//!
//! # 保存语义（增量——非重写）
//!
//! `save(path)` 落盘的是**产物字节** = 原字节前缀 + 追加修订段
//! （`LopdfWriter` 三层守卫保证前缀逐位相同）——文件系统上的旧内容
//! 被前缀保持地覆写。对已签名档：`/ByteRange` 覆盖段不变 → 签名保持
//! 有效（e2e 以 #308 `digest_matches` 交叉验证）。

use std::path::Path;

use mdc_pdf::annotation::{
    AnnotId, Annotation, AnnotationDraft, AnnotationKind, Origin, Rect, Rgb,
};
use mdc_pdf::command::{Command, CommandError, Outcome, Source};
use mdc_pdf::document::{DocumentMeta, PdfDocumentId, PdfSession};
use mdc_pdf::outline::OutlineTree;
use mdc_pdf::page::{PageCount, PageGeometry, PageIndex, Rotation};
use mdc_pdf::security::{Limits, OpenAssessment};
use mdc_pdf::security_scan::open_scan;
use mdc_pdf::transform::PageTransform;
use mdc_pdf::writer::{AnnotationWriter, IncrementalSave};
use mdc_pdf::writer_lopdf::{LopdfWriter, lopdf};

/// 高亮缺省色（Acrobat 黄）。
pub const HIGHLIGHT_COLOR: Rgb = Rgb {
    r: 255,
    g: 233,
    b: 90,
};

/// 下划线缺省色（#45 笔迹蓝同源——仓内既定缺省视觉，零新而异）。
pub const UNDERLINE_COLOR: Rgb = Rgb {
    r: 0x1a,
    g: 0x73,
    b: 0xe8,
};

/// 视图侧批注工具（issue #302 验收 1 三手势：高亮/下划线/便签）。
///
/// 手势→工具派发在 [`crate::pdf_pages`]；本枚举放在控制器模块
/// 是为了让 e2e（无 GUI）与视图（有 GUI）共享同一枚举面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotateTool {
    Highlight,
    Underline,
    Note,
}

impl AnnotateTool {
    /// 用户可见名（提示横幅用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Highlight => "高亮",
            Self::Underline => "下划线",
            Self::Note => "便签",
        }
    }

    /// 循环切换：高亮 → 下划线 → 便签 → 高亮（退化右键单击驱动）。
    pub fn next(self) -> Self {
        match self {
            Self::Highlight => Self::Underline,
            Self::Underline => Self::Note,
            Self::Note => Self::Highlight,
        }
    }
}

/// 标注写会话错误（typed——不 panic 不字符串裸传）。
#[derive(Debug)]
pub enum AnnotateError {
    /// 文档加密（S5 写面拒——先于一切）。
    Encrypted,
    /// 打开/解析失败（lopdf 或写器门）。
    Open(String),
    /// 页号越界（调用方参数面）。
    PageOutOfRange { page: u32, count: u32 },
    /// 会话命令/flush 失败（typed 透传 + 上下文）。
    Command(CommandError),
    /// 落盘 I/O 失败。
    Io(std::io::Error),
}

impl std::fmt::Display for AnnotateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnnotateError::Encrypted => {
                write!(f, "文档已加密：批注写面被拒（S5——与 stamp/forms 同款双拒）")
            }
            AnnotateError::Open(e) => write!(f, "PDF 打开失败：{e}"),
            AnnotateError::PageOutOfRange { page, count } => {
                write!(f, "页号越界（{page}，共 {count} 页）")
            }
            AnnotateError::Command(e) => write!(f, "标注命令失败：{e:?}"),
            AnnotateError::Io(e) => write!(f, "落盘失败：{e}"),
        }
    }
}

impl std::error::Error for AnnotateError {}

impl From<CommandError> for AnnotateError {
    fn from(e: CommandError) -> Self {
        AnnotateError::Command(e)
    }
}

/// PDF 标注写会话（非 GUI 协调器——见模块文档）。
pub struct PdfAnnotateController {
    session: PdfSession,
    /// 页几何快照（lopdf 直读——`add_highlight` 的变换源；会话内几何
    /// 不随批注变更——批注不改页树）。
    geometries: Vec<PageGeometry>,
    /// 标注修订号镜像（[`PdfSession::annotation_rev`]——视图渲染缓存
    /// 键控的 rev 源）。
    rev: u64,
    page_count: u32,
}

impl PdfAnnotateController {
    /// 打开写会话（安全门序见模块文档；`bytes` = 文件原字节）。
    pub fn open(bytes: Vec<u8>) -> Result<Self, AnnotateError> {
        // S2/S6 评估（读路径 #277 同款）；失败按无评估处理（宽容读面
        // 纪律——批注写仍走写器门把守）。
        let assessment = open_scan(&bytes, &Limits::default()).unwrap_or_default();
        // S5：加密档写面拒（先于一切写路径）。
        if assessment.encrypted {
            return Err(AnnotateError::Encrypted);
        }
        // lopdf 几何直读（页数 + 双盒 + 旋转——含父节点继承）。
        let geometries = parse_page_geometries(&bytes)?;
        let page_count = geometries.len() as u32;
        if page_count == 0 {
            return Err(AnnotateError::Open("零页文档（页树空）".into()));
        }
        // S3-L7 写器输入门（限额 + xref 链守卫——扫描重建档在此拒写，
        // 2026-10-31 事故防线）。
        let writer = LopdfWriter::from_bytes_with_limits(bytes, Limits::default())
            .map_err(|e| AnnotateError::Open(format!("写器打开失败：{e:?}")))?;
        // 会话装配（六命令面 + flush 事务；meta/几何同源——写器与解析
        // 读的是同一份字节）。
        let mut session = PdfSession::open(
            DocumentMeta {
                id: PdfDocumentId(1),
                file_name: "annotating.pdf".into(),
                page_count: PageCount(page_count),
            },
            assessment,
            geometries.clone(),
            OutlineTree::new(),
        )
        .map_err(|e| AnnotateError::Open(format!("会话装配失败：{e:?}")))?;
        session.attach_writer(Box::new(writer));
        Ok(Self {
            session,
            geometries,
            rev: 0,
            page_count,
        })
    }

    /// 总页数。
    pub fn page_count(&self) -> u32 {
        self.page_count
    }

    /// 标注修订号（flush 推进——#305 渲染缓存键控源）。
    pub fn annotation_rev(&self) -> u64 {
        self.rev
    }

    /// 单页变换（#304——右键拖框 → 用户空间的坐标桥）。
    pub fn page_transform(&self, page: u32) -> Result<PageTransform, AnnotateError> {
        let geo = self
            .geometries
            .get(page as usize)
            .ok_or(AnnotateError::PageOutOfRange {
                page,
                count: self.page_count,
            })?;
        PageTransform::from_geometry(geo)
            .map_err(|e| AnnotateError::Open(format!("页变换构造失败：{e}")))
    }

    /// 右键拖框高亮：屏幕矩形（px，**任意对角序**）→ 用户空间
    /// QuadPoints（#304 `screen_rect_to_quad`——归一化 + 旋转 + Y 翻转 +
    /// DPI 四合一）→ `AnnotateAdd` 命令（命令面即产品接线面）。
    ///
    /// `dpi` = 渲染位图分辨率（像素/英寸）——视图侧由
    /// `位图宽 / 显示域宽_pt × 72` 给出。
    pub fn add_highlight_from_screen(
        &mut self,
        page: u32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        dpi: f32,
        author: &str,
    ) -> Result<AnnotId, AnnotateError> {
        let transform = self.page_transform(page)?;
        let quad = transform.screen_rect_to_quad(x0, y0, x1, y1, dpi);
        let draft = AnnotationDraft {
            page: PageIndex(page),
            kind: AnnotationKind::Highlight {
                quads: vec![quad],
                color: HIGHLIGHT_COLOR,
            },
            author: author.into(),
            origin: Origin::Human,
        };
        self.commit_draft(draft)
    }

    /// 右键拖框下划线（#302 验收 1 第二手势）：与高亮同变换面
    /// （`/Underline` 同样吃 QuadPoints），仅色缺省不同。
    pub fn add_underline_from_screen(
        &mut self,
        page: u32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        dpi: f32,
        author: &str,
    ) -> Result<AnnotId, AnnotateError> {
        let transform = self.page_transform(page)?;
        let quad = transform.screen_rect_to_quad(x0, y0, x1, y1, dpi);
        let draft = AnnotationDraft {
            page: PageIndex(page),
            kind: AnnotationKind::Underline {
                quads: vec![quad],
                color: UNDERLINE_COLOR,
            },
            author: author.into(),
            origin: Origin::Human,
        };
        self.commit_draft(draft)
    }

    /// 右键拖框便签（#302 验收 1 第三手势）：屏幕矩形 → 用户空间
    /// 锚定 [`Rect`]（四角 `screen_to_user` 后 min/max 轴对齐化——与
    /// `screen_rect_to_quad` 同旋转鲁棒面；便签锚点即拖框区域）。
    /// 弹出评语文本面 v0 不建模（引擎 `Note` 同语义——`/Contents` 面
    /// 属 PDF-d 定稿，不在本 tranche）。
    pub fn add_note_from_screen(
        &mut self,
        page: u32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        dpi: f32,
        author: &str,
    ) -> Result<AnnotId, AnnotateError> {
        let transform = self.page_transform(page)?;
        // 任意对角序归一化（与 screen_rect_to_quad 同输入契约）。
        let (sx0, sx1) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        let (sy0, sy1) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        // 两对角 → 用户空间，min/max 轴对齐（90° 倍旋转下映射仍是
        // 轴对齐矩形——取包络不引入歪斜）。
        let tl = transform.screen_to_user(sx0, sy0, dpi);
        let br = transform.screen_to_user(sx1, sy1, dpi);
        let (min_x, max_x) = (tl.x.min(br.x), tl.x.max(br.x));
        let (min_y, max_y) = (tl.y.min(br.y), tl.y.max(br.y));
        let rect = Rect::new(min_x, min_y, max_x, max_y).map_err(|e| {
            AnnotateError::Command(CommandError::engine(format!(
                "便签矩形退化/非有限（{e:?}）"
            )))
        })?;
        let draft = AnnotationDraft {
            page: PageIndex(page),
            kind: AnnotationKind::Note { rect },
            author: author.into(),
            origin: Origin::Human,
        };
        self.commit_draft(draft)
    }

    /// 草稿 → `AnnotateAdd`（三手势共享出口——页界检查 + 载荷形状
    /// 断言单点化，防三处复制漂移）。
    fn commit_draft(&mut self, draft: AnnotationDraft) -> Result<AnnotId, AnnotateError> {
        if draft.page.0 >= self.page_count {
            return Err(AnnotateError::PageOutOfRange {
                page: draft.page.0,
                count: self.page_count,
            });
        }
        let Outcome::AnnotAdded(added) = self.session.execute(Command::AnnotateAdd {
            draft,
            source: Source::View,
        })?
        else {
            return Err(AnnotateError::Command(CommandError::engine(
                "AnnotateAdd 载荷形状意外（应得 AnnotAdded）",
            )));
        };
        Ok(added.id)
    }

    /// 列出批注（模型视图——命令面 `AnnotateList`；页过滤可选）。
    pub fn list(&mut self, page: Option<u32>) -> Result<Vec<Annotation>, AnnotateError> {
        let filter = mdc_pdf::command::AnnotationFilter {
            page: page.map(PageIndex),
            ..Default::default()
        };
        let Outcome::AnnotListed(list) = self.session.execute(Command::AnnotateList {
            filter,
            source: Source::System,
        })?
        else {
            return Err(AnnotateError::Command(CommandError::engine(
                "AnnotateList 载荷形状意外（应得 AnnotListed）",
            )));
        };
        Ok(list)
    }

    /// flush（增量保存事务）→ 产物字节 + 统计。**幂等零脏不追加**
    /// （`bytes_appended = 0`——`IncrementalSave` 语义锁）；产物 =
    /// 原字节前缀 + 追加修订（逐位保持——见模块文档「保存语义」）。
    pub fn flush(&mut self) -> Result<(Vec<u8>, IncrementalSave), AnnotateError> {
        let stats = self.session.flush_annotations()?;
        self.rev = self.session.annotation_rev();
        let bytes = self
            .session
            .product_bytes()
            .expect("写器在位（open 时装配）");
        Ok((bytes, stats))
    }

    /// flush + 落盘原路径（增量保存：写出的字节以原文件字节为逐位
    /// 前缀——旧内容被前缀保持地覆写，非全量重写）。
    pub fn save(&mut self, path: &Path) -> Result<IncrementalSave, AnnotateError> {
        let (bytes, stats) = self.flush()?;
        std::fs::write(path, bytes).map_err(AnnotateError::Io)?;
        Ok(stats)
    }
}

// ---------------------------------------------------------------------------
// lopdf 几何直读（页数 + 双盒 + 旋转，含父节点继承）
// ---------------------------------------------------------------------------

/// 解析每页几何（MediaBox/CropBox/`/Rotate`）。`/MediaBox` 与
/// `/CropBox` 按 PDF 继承语义沿 `/Parent` 上溯（深度上限 = 环防御）。
fn parse_page_geometries(bytes: &[u8]) -> Result<Vec<PageGeometry>, AnnotateError> {
    use mdc_pdf::page::PageBox;

    let doc = lopdf::Document::load_mem(bytes)
        .map_err(|e| AnnotateError::Open(format!("lopdf 装载失败：{e}")))?;
    let pages = doc.get_pages();
    let mut out = Vec::with_capacity(pages.len());
    for (_no, page_id) in pages {
        let dict = doc
            .get_dictionary(page_id)
            .map_err(|e| AnnotateError::Open(format!("页字典读取失败：{e}")))?;
        // 继承上溯（/MediaBox /CropBox /Rotate 皆可继承——spec 7.7.3.2）。
        let media = inherit_box(&doc, dict, b"MediaBox");
        let crop = inherit_box(&doc, dict, b"CropBox");
        let rotate = inherit_rotate(&doc, dict);
        let media = media.unwrap_or(PageBox {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0,
        }); // 无 MediaBox = 畸形档；Letter 兜底（宽容读面——写器门终把守）
        out.push(PageGeometry {
            width_pt: media.width(),
            height_pt: media.height(),
            rotation: rotate,
            media_box: media,
            crop_box: crop,
        });
    }
    Ok(out)
}

/// 沿 `/Parent` 上溯读页框（数组形态 `[x0 y0 x1 y1]`；非有限值由
/// `PageTransform::new` 构造期 typed 拒——本层原样照录）。
fn inherit_box(
    doc: &lopdf::Document,
    dict: &lopdf::Dictionary,
    key: &[u8],
) -> Option<mdc_pdf::page::PageBox> {
    let mut current = Some(dict);
    let mut depth = 0;
    while let Some(d) = current {
        if depth > 64 {
            return None; // /Parent 环防御
        }
        if let Ok(arr) = d.get(key).and_then(|o| o.as_array()) {
            let n = |i: usize| -> Option<f32> {
                arr.get(i).and_then(|o| {
                    o.as_f32()
                        .ok()
                        .or_else(|| o.as_i64().ok().map(|v| v as f32))
                })
            };
            if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (n(0), n(1), n(2), n(3)) {
                return Some(mdc_pdf::page::PageBox { x0, y0, x1, y1 });
            }
            return None; // 畸形数组——不猜（None 上溯继续无意义，直接放弃）
        }
        current = d
            .get(b"Parent")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .and_then(|id| doc.get_dictionary(id).ok());
        depth += 1;
    }
    None
}

/// 沿 `/Parent` 上溯读 `/Rotate`（合法值 0/90/180/270；其它按 0——
/// 宽容读面，`Rotation::from_degrees` 规范面拒绝语义在此不放大）。
fn inherit_rotate(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Rotation {
    let mut current = Some(dict);
    let mut depth = 0;
    while let Some(d) = current {
        if depth > 64 {
            return Rotation::Deg0;
        }
        if let Ok(deg) = d.get(b"Rotate").and_then(|o| o.as_i64()) {
            return Rotation::from_degrees(deg as i32).unwrap_or(Rotation::Deg0);
        }
        current = d
            .get(b"Parent")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .and_then(|id| doc.get_dictionary(id).ok());
        depth += 1;
    }
    Rotation::Deg0
}
