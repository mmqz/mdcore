//! 标注模型——复用 PDF 标准 Annot 对象语义（PDF-b 骨架轮，v0 内存态）。
//!
//! v0 四种（pdf_carrier_plan §5 PDF-d 子集前置定型）：Highlight / Note /
//! FreeText / Ink——**不自研锚定层**（与 mdcore anchor 哲学的分叉点：
//! PDF 有标准标注模型，v0 目标是与阅读器互通）。溯源经 PDF Annot `/T`
//! （author）字段承载 Human/Ai 区分（plan §4：AI 与 Human 走同一
//! Command 路径，author 字段区分）。落盘（lopdf 增量写）在 PDF-d；
//! 本轮为内存态 + serde 协议面。

use serde::{Deserialize, Serialize};

use crate::page::PageIndex;

/// 标注 id（会话内单调分配；**0 永不签发**——保留为"无/未定"哨兵，
/// 1 起步使 id 恒为真值，便于协议层判空）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnnotId(pub u64);

/// PDF 用户空间点（pt）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// 高亮四边形（PDF QuadPoints 语义：四角点序 = 环序，p0→p1 为文本行
/// 方向边）。一页多条高亮 = 多个 Quad（连续行各自成块）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quad {
    pub points: [Point; 4],
}

/// sRGB 通道值（0-255）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// [`Rect`] 构造错误（反序坐标——PDF 用户空间不变量破坏）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RectError {
    Inverted { x0: f32, y0: f32, x1: f32, y1: f32 },
}

/// serde 裸字段镜像（仅反序列化入口——校验后升格为 [`Rect`]）。
#[derive(Deserialize)]
struct RawRect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

/// 轴对齐矩形（PDF 用户空间；**x0<=x1 且 y0<=y1 由构造保证**——不变量
/// 在类型层，消费方免于逐点防御；退化零面积矩形合法）。
///
/// 不变量双重防线：字段私有（仓内只能经 [`Rect::new`] 构造）+ 自定义
/// Deserialize（宿主 JSON 面反序矩形直接拒收——pdf-b-1r 审计修复：派生
/// Deserialize 曾绕过构造校验，反序矩形可经 patch 静默落库，PDF-d 落盘
/// 将产出非法 `/Rect`）。NaN/Inf 由 serde_json 天然拒绝（实测）。Serialize
/// 保持派生（出向无需校验）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl<'de> Deserialize<'de> for Rect {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawRect::deserialize(deserializer)?;
        Rect::new(raw.x0, raw.y0, raw.x1, raw.y1)
            .map_err(|_| serde::de::Error::custom("inverted rect: x0 > x1 or y0 > y1"))
    }
}

impl Rect {
    /// 构造即校验（任一轴反序 → `RectError::Inverted` 携带原坐标供报告）。
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Result<Self, RectError> {
        if x0 <= x1 && y0 <= y1 {
            Ok(Self { x0, y0, x1, y1 })
        } else {
            Err(RectError::Inverted { x0, y0, x1, y1 })
        }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// 左下角 x（PDF 用户空间原点在左下）。
    pub fn x0(&self) -> f32 {
        self.x0
    }

    /// 左下角 y。
    pub fn y0(&self) -> f32 {
        self.y0
    }

    /// 右上角 x。
    pub fn x1(&self) -> f32 {
        self.x1
    }

    /// 右上角 y。
    pub fn y1(&self) -> f32 {
        self.y1
    }
}

/// 标注种类（v0 四变体；括号内为对应 PDF Annot `/Subtype`——v0 只用
/// 标准子型，零私有扩展，互通性从第一天成立）。
///
/// 载荷健全性（如非空 quads/strokes）v0 **不校验**——错误面收窄五类
/// 无 per-field 变体；PDF-d 落盘时由引擎侧（lopdf 写前）校验。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnnotationKind {
    /// 高亮（`/Highlight`）：四边形列表 + 颜色。
    Highlight { quads: Vec<Quad>, color: Rgb },
    /// 便签（`/Text`）：锚定矩形（弹出评语文本面 v0 不建模——`/Contents`
    /// 落盘映射 PDF-d 定稿）。
    Note { rect: Rect },
    /// 自由文本（`/FreeText`）：锚定矩形 + 文本。
    FreeText { rect: Rect, text: String },
    /// 手写墨迹（`/Ink`）：笔划列表（每划 = 采样点序列）。
    Ink { strokes: Vec<Vec<Point>> },
}

/// 种类判别（无载荷判据——过滤与 patch 适用面的判定用；与
/// [`AnnotationKind`] 的 payload 解耦，协议面稳定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnnotationKindTag {
    Highlight,
    Note,
    FreeText,
    Ink,
}

impl AnnotationKind {
    pub fn tag(&self) -> AnnotationKindTag {
        match self {
            Self::Highlight { .. } => AnnotationKindTag::Highlight,
            Self::Note { .. } => AnnotationKindTag::Note,
            Self::FreeText { .. } => AnnotationKindTag::FreeText,
            Self::Ink { .. } => AnnotationKindTag::Ink,
        }
    }
}

/// 标注溯源：PDF Annot author（`/T`）字段承载的 Human/Ai 区分
/// （plan §4——AI 生成标注与 Human 走同一 Command 路径，落盘时由
/// author 字段约定区分，PDF-d 定稿映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Origin {
    Human,
    Ai,
}

/// 标注实体（会话态）。id 与时间戳由会话分配（非调用方输入——单调性
/// 与时钟纪律由会话保证）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotId,
    pub page: PageIndex,
    pub kind: AnnotationKind,
    /// PDF `/T`（author）——同时承载 [`Origin`] 区分（落盘约定 PDF-d）。
    pub author: String,
    pub origin: Origin,
    /// 创建时间（epoch ms，会话时钟——宿主注入，核不取系统时间）。
    pub created_ms: u64,
    /// 最近一次**内容**修改时间（no-op edit 不推进——见 apply_patch）。
    pub modified_ms: u64,
}

/// `AnnotateAdd` 输入（无 id / 时间戳——由会话分配）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationDraft {
    pub page: PageIndex,
    pub kind: AnnotationKind,
    pub author: String,
    pub origin: Origin,
}

/// `AnnotateEdit` 输入（部分更新；未设置面不出现在 JSON——
/// `skip_serializing_if`，wire 面只携带变化项）。
///
/// **kind 不可变**：本类型无 kind 字段（结构层排除）；JSON 面改 kind
/// 的请求按 serde 默认**忽略未知字段**退化 no-op——「忽略」语义锁死
/// （PDF `/Subtype` 语义上也不可换装，改型 = 删旧加新）。
///
/// 可变面与适用矩阵（不适用面静默忽略——错误面收窄为五类
/// [`crate::command::CommandError`]，v0 无 per-field 错误变体）：
/// - `contents` → FreeText.text（PDF `/Contents` 语义）；
/// - `rect` → Note.rect / FreeText.rect；
/// - `color` → Highlight.color（Ink v0 无颜色字段——PDF-d 外观流面再议）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnnotationPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgb>,
}

impl AnnotationPatch {
    /// 是否空 patch（无可变面——应用为 no-op）。
    pub fn is_empty(&self) -> bool {
        self.contents.is_none() && self.rect.is_none() && self.color.is_none()
    }
}

impl Annotation {
    /// 就地应用 patch。**kind 永不触碰**（类型层保证）；不适用面忽略。
    /// 返回是否产生实际内容变更（no-op → false，调用方据此不推进
    /// `modified_ms`——时间戳反映内容而非命令次数）。
    pub fn apply_patch(&mut self, patch: &AnnotationPatch) -> bool {
        let mut changed = false;
        match &mut self.kind {
            AnnotationKind::FreeText { rect, text } => {
                if let Some(new_rect) = patch.rect {
                    if *rect != new_rect {
                        *rect = new_rect;
                        changed = true;
                    }
                }
                if let Some(new_text) = &patch.contents {
                    if text != new_text {
                        *text = new_text.clone();
                        changed = true;
                    }
                }
            }
            AnnotationKind::Note { rect } => {
                if let Some(new_rect) = patch.rect {
                    if *rect != new_rect {
                        *rect = new_rect;
                        changed = true;
                    }
                }
            }
            AnnotationKind::Highlight { color, .. } => {
                if let Some(new_color) = patch.color {
                    if *color != new_color {
                        *color = new_color;
                        changed = true;
                    }
                }
            }
            // Ink v0 无可变面（笔划编辑 = 删旧加新；外观流 PDF-d）。
            AnnotationKind::Ink { .. } => {}
        }
        changed
    }
}
