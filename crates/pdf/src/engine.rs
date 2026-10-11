//! 引擎集成点（PDF-c 实装）——骨架轮只冻结 trait 面，不引入引擎依赖。
//!
//! ADR-P1 定谳：pdfium（经 pdfium-render，**non-V8 构建**——S1.1 JS
//! 不接 / S1.2 XFA 不启用，取件脚本断言 `args.gn`）承担渲染/文本/搜索；
//! lopdf 承担标注增量写（PDF-d，不在本 trait——写路径是文档级会话面）。
//! v0 会话数据由宿主注入（[`crate::document`]）；PDF-c 起以本 trait 的
//! 引擎实装替换数据源——会话与命令面结构不变。
//!
//! S4 前瞻：object-safe（`&dyn` 可命名）——渲染工人池与读写分线程
//! 快照隔离需要对象引用形态传递引擎句柄。

use crate::command::CommandError;
use crate::outline::OutlineTree;
use crate::page::{PageGeometry, PageIndex, PageSpan};

/// 文档引擎抽象（只读数据面；标注写入走 PDF-d 会话增量写路径）。
pub trait DocumentEngine {
    /// 文档页数（引擎侧权威）。消费点：
    /// [`crate::document::PdfSession::attach_engine`] 以此对
    /// `meta.page_count` 做装配期一致性校验（双权威不唯一致时 typed
    /// 拒绝——T1 审计 O3 项：Search/ExtractText 以 meta 页数为上界，
    /// 引擎页多 → 静默漏末尾页 / 页少 → 整命令 InvalidPage，两条静默
    /// 失败路径在装配面收口）；宿主构造 [`crate::document::DocumentMeta`]
    /// 亦以此为准（同源构造，不独立拼装）。
    fn page_count(&self) -> u32;

    /// 单页几何（原始 MediaBox 尺寸 + `/Rotate`；归一化取
    /// [`PageGeometry::effective_width`] 等——消费方不自行处理旋转）。
    fn page_geometry(&self, idx: PageIndex) -> Result<PageGeometry, CommandError>;

    /// 文档大纲（书签树；无大纲文档返回空树而非 Err）。
    fn outline(&self) -> Result<OutlineTree, CommandError>;

    /// 页区间文本抽取（页号升序，载荷形状与
    /// [`crate::command::Outcome::ExtractedText`] 一致）。
    fn extract_text(&self, span: PageSpan) -> Result<Vec<(PageIndex, String)>, CommandError>;
}
