//! Command 边界——PDF 面的统一状态变更入口（PDF-b 骨架轮）。
//!
//! Human 与 AI 走同一命令路径（pdf_carrier_plan §4）：[`Source`] 溯源
//! **镜像** mdcore 事件源语义（View/Undo/Redo/Ai/System——语义镜像而非
//! 类型复用，crate 间零 path 依赖；serde 名与 `mdcore::history::Source`
//! 逐词面一致，语义锁背书防两向漂移）。单一调度入口 =
//! [`crate::document::PdfSession::execute`]。
//!
//! 错误面纪律：typed（[`CommandError`] 五类）——不 panic、不 String 错误
//! （调用方可穷举分支，协议层可稳定序列化）。

use serde::{Deserialize, Serialize};

use crate::annotation::{
    AnnotId, Annotation, AnnotationDraft, AnnotationKindTag, AnnotationPatch, Origin,
};
use crate::outline::OutlineTree;
use crate::page::{PageIndex, PageSpan};

/// 命令源（镜像 `mdcore::history::Source` 五值语义：View = 人手交互 /
/// Undo / Redo / Ai = AI 代理 / System = 核自驱——装载/注入类非交互事务，
/// 不参与任何合并窗语义）。serde 名 = snake_case，与 mdcore 侧逐词面一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    View,
    Undo,
    Redo,
    Ai,
    System,
}

/// 文本作用域（v0 **页粒度**；字符级范围留给 PDF-c 文本层——变体扩展
/// 非破坏，页粒度保持兼容底线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextTarget {
    Pages(PageSpan),
    All,
}

/// 搜索选项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    /// 命中收集上限（**请求值**——会话按 S3 限额钳制：超上限收敛 +
    /// `truncated: true` 告知，见 [`crate::security::Limits`]）。
    pub max_hits: u32,
}

/// 搜索命中（v0 骨架：页 + 摘录；rect 命中面 PDF-c 文本层接入时扩展）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub page: PageIndex,
    /// 命中点上下文摘录（char 边界安全；长度受 S3 `snippet_max_chars` 钳制）。
    pub snippet: String,
}

/// 标注列表过滤（多维度**合取**；`None` = 该维度不过滤）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationFilter {
    /// 空集语义不存在——`None` 才是"不过滤"；`Some(vec)` = kind ∈ 集合。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<AnnotationKindTag>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageIndex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}

impl AnnotationFilter {
    /// 合取匹配（`None` 维度恒真）。
    pub fn matches(&self, annot: &Annotation) -> bool {
        if let Some(kinds) = &self.kinds {
            if !kinds.contains(&annot.kind.tag()) {
                return false;
            }
        }
        if let Some(page) = self.page {
            if annot.page != page {
                return false;
            }
        }
        if let Some(origin) = self.origin {
            if annot.origin != origin {
                return false;
            }
        }
        true
    }
}

/// PDF 面命令（每变体必带 `source`——溯源是第一公民，事后不可补录）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// 文本抽取（页粒度）。
    ExtractText { target: TextTarget, source: Source },
    /// 全文搜索（空 query = `CommandError::EmptyQuery`，S3 ❶硬拒绝）。
    Search {
        query: String,
        opts: SearchOptions,
        source: Source,
    },
    /// 文档大纲（书签树）。
    Outline { source: Source },
    /// 新增标注（id/时间戳由会话分配）。
    AnnotateAdd {
        draft: AnnotationDraft,
        source: Source,
    },
    /// 编辑标注（部分更新；kind 不可变——见 [`AnnotationPatch`]）。
    AnnotateEdit {
        id: AnnotId,
        patch: AnnotationPatch,
        source: Source,
    },
    /// 标注列表（过滤 + id 升序确定性）。
    AnnotateList {
        filter: AnnotationFilter,
        source: Source,
    },
}

/// 命令成功载荷（与 [`Command`] 六变体一一对应）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    /// 抽取文本（页号升序；v0 文本层 = 宿主注入面，未注入页为空串——
    /// 页集形状先行冻结，文本载荷 PDF-c 由引擎填充）。
    ExtractedText { pages: Vec<(PageIndex, String)> },
    /// 搜索结果（`truncated` = S3 ❷钳制告知：命中超上限被截断）。
    SearchResults {
        hits: Vec<SearchHit>,
        truncated: bool,
    },
    /// 大纲（树快照）。
    Outline(OutlineTree),
    /// 新增产物（含会话分配的 id 与时间戳）。
    AnnotAdded(Annotation),
    /// 编辑后产物（no-op patch 时与库内现值逐字段一致）。
    AnnotEdited(Annotation),
    /// 过滤结果（id 升序）。
    AnnotListed(Vec<Annotation>),
}

/// 命令错误（typed；只序列化不反序列化——错误面为 Rust → 宿主单向）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CommandError {
    /// **协议层专用**：命令信封指向无会话的文档槽。会话内不可达——
    /// [`crate::document::PdfSession`] 只有打开后形态（未打开态在类型层
    /// 不存在），本变体由宿主协议层（[`crate::protocol`]）在槽位空时产出。
    NotOpen,
    /// 页号越界（携带肇事页号，供宿主定位报告）。
    InvalidPage(PageIndex),
    /// 标注不存在（可能已被未来版本删除——v0 无删除命令，预留面）。
    AnnotNotFound(AnnotId),
    /// 安全策略拒绝（S3 限额 ❶ / S5 加密写拒；`reason` = 稳定策略码，
    /// 见 [`crate::security`] 常量——协议面冻结锚）。
    SecurityDeny { reason: &'static str },
    /// 空查询（S3 ❶硬拒绝：空串无语义，"全匹配"是资源滥用面）。
    EmptyQuery,
}
