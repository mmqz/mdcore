//! 页模型——PDF Document Model 的空间基座（PDF-b 骨架轮）。
//!
//! 归一化语义（语义锁背书）：[`PageGeometry::effective_width`] /
//! [`PageGeometry::effective_height`] 按 `/Rotate` 归一——90/270° 交换
//! 宽高（观看坐标系），0/180° 不交换。引擎侧（PDF-c pdfium）的原始
//! MediaBox + `/Rotate` 直接进本类型；上层渲染与命中测试一律取
//! effective 面，避免每个消费点各自处理旋转（单一归一化出口 = 无漂移）。
//!
//! 分层纪律：[`PageSpan`] **构造期只校验区间序**（start <= end——span
//! 无文档页数上下文）；页数越界校验属命令层（`PdfSession::execute` →
//! `CommandError::InvalidPage`）。

use serde::{Deserialize, Serialize};

/// serde 裸字段镜像（仅用于反序列化入口——校验后升格为 [`PageSpan`]）。
#[derive(Deserialize)]
struct RawPageSpan {
    start: PageIndex,
    end: PageIndex,
}

/// 0 基页号。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct PageIndex(pub u32);

/// 文档总页数（PDF 规范：Page tree 至少一页——0 页在 [`crate::document`]
/// 的 `open` 处拒绝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageCount(pub u32);

impl PageCount {
    /// 页号是否在本文档内（0 基半开区间判断）。
    pub fn contains(&self, idx: PageIndex) -> bool {
        idx.0 < self.0
    }
}

/// PDF `/Rotate` 页旋转（规范合法值只有 0/90/180/270，顺时针）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    /// 从 `/Rotate` 数值构造。非四值之一 → `None`（规范面拒绝，不做
    /// 静默取模降级——畸形输入必须显式失败，防止渲染面错转）。
    pub fn from_degrees(deg: i32) -> Option<Self> {
        match deg {
            0 => Some(Self::Deg0),
            90 => Some(Self::Deg90),
            180 => Some(Self::Deg180),
            270 => Some(Self::Deg270),
            _ => None,
        }
    }

    /// 归一化后是否交换宽高（90/270）。
    pub fn swaps_axes(&self) -> bool {
        matches!(self, Self::Deg90 | Self::Deg270)
    }
}

/// 页框（用户空间轴对齐矩形：`(x0, y0)` = 左下角，`(x1, y1)` = 右上角）。
///
/// **原样照录语义**：引擎报什么就存什么——畸形档（非有限值/退化框/
/// CropBox ⊄ MediaBox）不在本层纠偏，纠偏是 [`crate::transform::
/// PageTransform`] 构造期的显式校验职责（typed 错误，见 #304）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PageBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl PageBox {
    /// 宽（`x1 - x0`；退化/畸形原样返回，不在此层报错）。
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    /// 高（`y1 - y0`）。
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

/// 页几何（PDF 用户空间，单位 pt = 1/72 inch；`width`/`height` = 原始
/// MediaBox 尺寸，不含旋转）。
///
/// #304 起：`media_box`/`crop_box` 同步暴露（此前只有 MediaBox 宽高——
/// 裁剪页上的标注落点无从计算，审计 P2-4）。渲染显示域 = `crop_box`
/// （`None` = 页无 `/CropBox`，显示域取 MediaBox）；设备↔用户空间
/// 变换见 [`crate::transform::PageTransform`]。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageGeometry {
    pub width_pt: f32,
    pub height_pt: f32,
    pub rotation: Rotation,
    /// MediaBox 全矩形（含原点——非零原点档如裁切/出血流在宽高之外
    /// 还需要偏移基线）。等于 `(0,0,width_pt,height_pt)` 当且仅当
    /// MediaBox 原点为 (0,0)。
    #[serde(default)]
    pub media_box: PageBox,
    /// CropBox（用户空间）。`None` = 页无 `/CropBox` 条目 → 渲染显示域
    /// = MediaBox。
    #[serde(default)]
    pub crop_box: Option<PageBox>,
}

/// `PageGeometry` 的 `Default`（#304 加字段后的兼容面）。
///
/// **为什么需要**：`#[serde(default)]` 只让反序列化兼容——Rust 结构体
/// 字面量初始化仍是全字段必需。加 `media_box`/`crop_box` 后，
/// 17 处旧字面量（`PageGeometry { width_pt, height_pt, rotation }`）
/// 全部 E0063 编译失败（2026-11-07 main 因此变红）。
///
/// 有了 `Default` 后，新增字段只改这一处，调用方写
/// `PageGeometry { width_pt, ..Default::default() }` 即可。
///
/// 缺省语义：A4 纵向（595×842 pt）、无旋转、MediaBox 原点 (0,0)、无 CropBox。
impl Default for PageGeometry {
    fn default() -> Self {
        Self {
            width_pt: 595.0,
            height_pt: 842.0,
            rotation: Rotation::Deg0,
            media_box: PageBox {
                x0: 0.0,
                y0: 0.0,
                x1: 595.0,
                y1: 842.0,
            },
            crop_box: None,
        }
    }
}

impl PageGeometry {
    /// 简单页构造（无裁剪、原点 (0,0)）：`media_box = (0, 0, w, h)`、
    /// `crop_box = None`（显示域 = MediaBox）。测试与合成几何的便捷
    /// 出口——真实档几何走引擎（`engine_pdfium::page_geometry`）。
    pub fn simple(width_pt: f32, height_pt: f32, rotation: Rotation) -> Self {
        Self {
            width_pt,
            height_pt,
            rotation,
            media_box: PageBox {
                x0: 0.0,
                y0: 0.0,
                x1: width_pt,
                y1: height_pt,
            },
            crop_box: None,
        }
    }

    /// 归一化视口宽（含旋转）：90/270° 交换宽高。渲染与命中测试的
    /// **唯一**取宽出口（语义锁背书）。
    pub fn effective_width(&self) -> f32 {
        if self.rotation.swaps_axes() {
            self.height_pt
        } else {
            self.width_pt
        }
    }

    /// 归一化视口高（含旋转）：90/270° 交换宽高。
    pub fn effective_height(&self) -> f32 {
        if self.rotation.swaps_axes() {
            self.width_pt
        } else {
            self.height_pt
        }
    }
}

/// [`PageSpan`] 构造错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSpanError {
    /// start > end（反转区间；start == end 合法 = 单页闭区间）。
    Inverted { start: PageIndex, end: PageIndex },
}

/// 页区间（0 基**闭**区间 `[start, end]`，含两端）。
///
/// 不变量（start <= end）三重防线：字段私有（仓内只能经 [`PageSpan::new`]
/// 构造）+ 自定义 Deserialize（宿主 JSON 面反转区间直接拒收——pdf-b-1r
/// 审计修复：派生 Deserialize 曾绕过构造校验，畸形信封可致 panic/abort）
/// + `resolve_span` 页界检查前置。Serialize 保持派生（出向无需校验）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PageSpan {
    start: PageIndex,
    end: PageIndex,
}

impl<'de> Deserialize<'de> for PageSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPageSpan::deserialize(deserializer)?;
        PageSpan::new(raw.start, raw.end)
            .map_err(|_| serde::de::Error::custom("inverted page span: start > end"))
    }
}

impl PageSpan {
    /// 构造即校验 start <= end（类型层不变量：合法 span 恒非空）。
    /// 页数上限校验属命令层（见模块文档——分层纪律）。
    pub fn new(start: PageIndex, end: PageIndex) -> Result<Self, PageSpanError> {
        if start.0 <= end.0 {
            Ok(Self { start, end })
        } else {
            Err(PageSpanError::Inverted { start, end })
        }
    }

    /// 区间页数（含两端；start <= end 由类型层三重防线保证，减法无溢出）。
    /// 闭区间恒非空（最小 = 单页）。
    pub fn len(&self) -> u64 {
        u64::from(self.end.0) - u64::from(self.start.0) + 1
    }

    /// 区间首页。
    pub fn start(&self) -> PageIndex {
        self.start
    }

    /// 区间末页。
    pub fn end(&self) -> PageIndex {
        self.end
    }

    /// 闭区间恒非空（`len >= 1`——与 `len()` 配套的空性语义）。
    pub fn is_empty(&self) -> bool {
        false
    }

    /// 展开为页号迭代（升序文档序）。
    pub fn iter(&self) -> impl Iterator<Item = PageIndex> {
        (self.start.0..=self.end.0).map(PageIndex)
    }

    /// 是否落在页数 `count` 内（start <= end 已保证，只需查上界）。
    pub fn within(&self, count: PageCount) -> bool {
        count.contains(self.end)
    }
}
