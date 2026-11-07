//! mdc-canvas —— Canvas 引擎（独立领域模型：节点/连线/分组 + 几何 +
//! 开放属性；人类与 AI 经同一命令接口操作同一份画布状态）。
//!
//! # 产品语义
//!
//! 画布 = **图形状**的协作共享状态：节点（几何矩形 + 属性）、连线
//! （端点引用 + 属性）、分组（成员引用集）。与文档域（mdc-collab
//! 的块树）并列且独立——各自 CRDT Doc，不建跨引擎统一 CRDT。
//!
//! # 身份特性（画布域专属）
//!
//! 对象 ID = 载体 Map 键 → **删除 → 撤销复原 = 同 ID 回归**（Map 键
//! 身份稳定；对比文档域 XML 元素复活 = 新 ID 族）。证据锚（Locator::
//! CanvasObject）因此获得删除复原的语义连续性。
//!
//! # 语义指纹（证据关联面）
//!
//! [`CanvasSession::object_fingerprint`]：对象语义字段确定性序列化 →
//! FNV-1a 64（与 mdc-evidence 同指纹器）。几何/属性/边端点/组成员
//! 任一变化 → 指纹变 → 证据失效（Stale）。位置漂移即语义变更——
//! 画布域的证据粒度决策，注册于 Locator::CanvasObject 文档。

pub mod events;
pub mod render;
pub mod session;

pub use events::{CanvasChangeSource, CanvasEvent, Subscription};
pub use render::{
    canvas_bounds, canvas_bounds_geometry, encode_png, export_png, export_svg, parse_color,
    shape_bounds, shape_bounds_padded, shape_from_object, Affine, Bitmap, CanvasRenderer,
    CanvasRendering, Color, ImageMime, LineCap, LineJoin, PathCmd, Point, RasterRenderer, Rect,
    Shape, ShapeStyle, StrokeDash, StrokeStyle, SvgRenderer, TextAnchor, TextStyle, Transform,
};
pub use session::{
    Actor, CanvasCommand, CanvasError, CanvasObject, CanvasSession, CommandOutcome, CommandReceipt,
    ObjectKind, SessionOptions, StrokeSpec, StrokeTool,
};
