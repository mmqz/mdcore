//! `PdfSession`——PDF 文档会话状态机（PDF-b 骨架轮）。
//!
//! 状态机只有**一个形态：已打开**（未打开态在类型层不存在——`open` 是
//! 唯一构造出口，失败走 [`OpenError`]；协议层的"槽位无会话"由宿主以
//! [`crate::command::CommandError::NotOpen`] 表达，见 protocol.rs）。
//!
//! 引擎数据 v0 由宿主注入：页几何/大纲走 `open` 参数，页文本走
//! [`PdfSession::set_page_text`]（PDF-c 起由 [`crate::engine::DocumentEngine`]
//! 的 pdfium 实装替换数据源——会话与命令面结构不变）。时间戳取会话
//! 时钟（宿主注入——核不取系统时间，对齐 mdcore 时钟注入纪律：wasm32
//! 无 std 时钟，宿主是唯一时间源）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::annotation::{AnnotId, Annotation, AnnotationDraft, AnnotationPatch};
use crate::command::{
    AnnotationFilter, Command, CommandError, Outcome, SearchHit, SearchOptions, TextTarget,
};
use crate::outline::{OutlineTarget, OutlineTree};
use crate::page::{PageCount, PageGeometry, PageIndex, PageSpan};
use crate::security::{
    LimitBehavior, Limits, OpenAssessment, REASON_AUTHOR_LENGTH, REASON_ENCRYPTED_WRITE,
};

/// 文档身份（宿主分配；会话内不解释——跨会话复原属宿主域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PdfDocumentId(pub u64);

/// 打开元数据（宿主域）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub id: PdfDocumentId,
    pub file_name: String,
    pub page_count: PageCount,
}

/// 打开失败（typed——宿主接线错误与畸形输入 fail-fast，不静默降级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// 0 页文档（PDF 规范要求 Page tree 至少一页——畸形输入直接拒绝）。
    EmptyDocument,
    /// 注入页几何数与元数据页数不符（宿主接线错误）。
    PageGeometryMismatch { expected: u32, provided: usize },
    /// 大纲目标页越界（注入数据不自洽——越界大纲在打开期剔除而非静默
    /// 存留，防止命令层 Outline 消费到幽灵页）。
    OutlineTargetOutOfRange(PageIndex),
}

/// PDF 文档会话（打开后单形态；命令的唯一调度入口 = [`PdfSession::execute`]）。
#[derive(Debug, Clone)]
pub struct PdfSession {
    meta: DocumentMeta,
    assessment: OpenAssessment,
    limits: Limits,
    pages: Vec<PageGeometry>,
    outline: OutlineTree,
    /// v0 文本层（宿主注入；PDF-c 换 DocumentEngine::extract_text）。
    /// BTreeMap = 页号升序确定性。
    page_text: BTreeMap<u32, String>,
    /// 标注库（id 升序迭代 = BTreeMap 键序，确定性）。
    annotations: BTreeMap<AnnotId, Annotation>,
    /// 下一标注 id（单调；1 起步——0 = 永不签发哨兵）。
    next_annot_id: u64,
    /// 会话时钟（epoch ms，宿主注入）。
    now_ms: u64,
}

impl PdfSession {
    /// 打开会话（唯一构造出口）。校验（fail-fast）：
    /// 页数非零 / 页几何数与元数据一致 / 大纲目标页全部在档。
    /// `encrypted = true` 的评估**不拒绝打开**（S5.1：v0 只读支持——
    /// 写类命令在执行层拒）。
    pub fn open(
        meta: DocumentMeta,
        assessment: OpenAssessment,
        pages: Vec<PageGeometry>,
        outline: OutlineTree,
    ) -> Result<Self, OpenError> {
        if meta.page_count.0 == 0 {
            return Err(OpenError::EmptyDocument);
        }
        if pages.len() != meta.page_count.0 as usize {
            return Err(OpenError::PageGeometryMismatch {
                expected: meta.page_count.0,
                provided: pages.len(),
            });
        }
        for (_, node) in outline.iter() {
            // 直接 `let` 解构（非 if let）：v0 单变体下不可反驳；PDF-c
            // 追加命名/URI 目标变体时此处**编译期失败**，强制重访校验。
            let OutlineTarget::Page(idx) = node.target;
            if !meta.page_count.contains(idx) {
                return Err(OpenError::OutlineTargetOutOfRange(idx));
            }
        }
        Ok(Self {
            meta,
            assessment,
            limits: Limits::default(),
            pages,
            outline,
            page_text: BTreeMap::new(),
            annotations: BTreeMap::new(),
            next_annot_id: 1,
            now_ms: 0,
        })
    }

    /// 单一调度入口（六命令；成功载荷/typed 错误见 command.rs）。
    ///
    /// `source` v0 = 溯源载荷（先冻结在命令面防协议漂移；事件流出口 =
    /// PDF-c 视图事件面 `mdc-pdf-event`）——以下以 `_source` 解构占位。
    pub fn execute(&mut self, cmd: Command) -> Result<Outcome, CommandError> {
        match cmd {
            Command::ExtractText {
                target,
                source: _source,
            } => self.extract_text(target),
            Command::Search {
                query,
                opts,
                source: _source,
            } => self.search(query, opts),
            Command::Outline { source: _source } => Ok(Outcome::Outline(self.outline.clone())),
            Command::AnnotateAdd {
                draft,
                source: _source,
            } => self.annotate_add(draft),
            Command::AnnotateEdit {
                id,
                patch,
                source: _source,
            } => self.annotate_edit(id, patch),
            Command::AnnotateList {
                filter,
                source: _source,
            } => Ok(Outcome::AnnotListed(self.list_annotations(&filter))),
        }
    }

    // -----------------------------------------------------------------------
    // 宿主注入面
    // -----------------------------------------------------------------------

    /// 注入页文本（v0 文本层——宿主/测试面；PDF-c 起由引擎数据源替换，
    /// 本方法保留为无引擎环境的降级通道）。页界校验即时执行。
    pub fn set_page_text(
        &mut self,
        page: PageIndex,
        text: impl Into<String>,
    ) -> Result<(), CommandError> {
        if !self.meta.page_count.contains(page) {
            return Err(CommandError::InvalidPage(page));
        }
        self.page_text.insert(page.0, text.into());
        Ok(())
    }

    /// 会话时钟注入（epoch ms）。标注 `created_ms`/`modified_ms` 取自
    /// 会话时钟——核不取系统时间（确定性/可测性/wasm32 兼容）。
    pub fn set_clock_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    // -----------------------------------------------------------------------
    // 只读访问面
    // -----------------------------------------------------------------------

    pub fn meta(&self) -> &DocumentMeta {
        &self.meta
    }

    pub fn assessment(&self) -> &OpenAssessment {
        &self.assessment
    }

    pub fn page_count(&self) -> PageCount {
        self.meta.page_count
    }

    /// 单页几何（越界 None——查询面不报错，与命令面的 InvalidPage 分工）。
    pub fn page_geometry(&self, idx: PageIndex) -> Option<PageGeometry> {
        self.pages.get(idx.0 as usize).copied()
    }

    pub fn annotation_count(&self) -> usize {
        self.annotations.len()
    }

    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    // -----------------------------------------------------------------------
    // 命令实现
    // -----------------------------------------------------------------------

    /// ExtractText：页集形状先行冻结（v0 文本 = 注入面，未注入页空串；
    /// 文本载荷 PDF-c 由引擎填充——载荷形状不变）。
    fn extract_text(&self, target: TextTarget) -> Result<Outcome, CommandError> {
        let span = self.resolve_span(target)?;
        let mut pages = Vec::with_capacity(span.len() as usize);
        for idx in span.iter() {
            let text = self.page_text.get(&idx.0).cloned().unwrap_or_default();
            pages.push((idx, text));
        }
        Ok(Outcome::ExtractedText { pages })
    }

    /// Search：v0 = 宿主注入文本面的朴素扫描（页号升序；Unicode 简单
    /// 折叠大小写不敏感；snippet char 边界安全）。PDF-c 换 pdfium 文本
    /// 引擎检索——命令/载荷形状不变。
    fn search(&self, query: String, opts: SearchOptions) -> Result<Outcome, CommandError> {
        if query.is_empty() {
            // S3 ❶硬拒绝：空串无语义，"全匹配"是资源滥用面。
            return Err(CommandError::EmptyQuery);
        }
        let (max_hits, _behavior) = self.limits.enforce_search_hits(opts.max_hits);
        // 摘录上限与命中无关（循环不变量，提出循环——pdf-b-1r 审计备忘）。
        let (snippet_chars, _) = self.limits.enforce_snippet_chars(usize::MAX);
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut truncated = false;
        'pages: for page_no in 0..self.meta.page_count.0 {
            let Some(text) = self.page_text.get(&page_no) else {
                continue;
            };
            for byte_idx in find_matches(text, &query, opts.case_insensitive) {
                if hits.len() >= max_hits as usize {
                    // 已收满且仍见新命中 → ❷钳制告知（不静默丢弃）。
                    truncated = true;
                    break 'pages;
                }
                let snippet = snippet_around(text, byte_idx, snippet_chars);
                hits.push(SearchHit {
                    page: PageIndex(page_no),
                    snippet,
                });
            }
        }
        Ok(Outcome::SearchResults { hits, truncated })
    }

    /// AnnotateAdd：策略门（S5）→ 页界（InvalidPage）→ S3-L7 作者名 ❶ →
    /// 分配 id（单调）+ 时间戳（会话时钟）。
    fn annotate_add(&mut self, draft: AnnotationDraft) -> Result<Outcome, CommandError> {
        // S5：仍加密态拒绝一切写类命令（先于一切存在性检查——策略门最前）。
        if self.assessment.denies_writes() {
            return Err(CommandError::SecurityDeny {
                reason: REASON_ENCRYPTED_WRITE,
            });
        }
        if !self.meta.page_count.contains(draft.page) {
            return Err(CommandError::InvalidPage(draft.page));
        }
        let requested = draft.author.chars().count();
        let (effective, behavior) = self.limits.enforce_author_chars(requested);
        if behavior == LimitBehavior::HardReject && effective < requested {
            return Err(CommandError::SecurityDeny {
                reason: REASON_AUTHOR_LENGTH,
            });
        }
        let id = AnnotId(self.next_annot_id);
        self.next_annot_id += 1;
        let annot = Annotation {
            id,
            page: draft.page,
            kind: draft.kind,
            author: draft.author,
            origin: draft.origin,
            created_ms: self.now_ms,
            modified_ms: self.now_ms,
        };
        self.annotations.insert(id, annot.clone());
        Ok(Outcome::AnnotAdded(annot))
    }

    /// AnnotateEdit：策略门（S5）→ 存在性（AnnotNotFound）→ patch 应用
    /// （kind 不可变；不适用面忽略）。no-op patch 不推进 modified_ms
    /// （时间戳反映内容变更，非命令次数）。
    fn annotate_edit(
        &mut self,
        id: AnnotId,
        patch: AnnotationPatch,
    ) -> Result<Outcome, CommandError> {
        if self.assessment.denies_writes() {
            return Err(CommandError::SecurityDeny {
                reason: REASON_ENCRYPTED_WRITE,
            });
        }
        let annot = self
            .annotations
            .get_mut(&id)
            .ok_or(CommandError::AnnotNotFound(id))?;
        if annot.apply_patch(&patch) {
            annot.modified_ms = self.now_ms;
        }
        Ok(Outcome::AnnotEdited(annot.clone()))
    }

    /// AnnotateList：过滤（合取）+ id 升序（BTreeMap 键序——确定性，
    /// 同库同序与遍历历史无关）。
    fn list_annotations(&self, filter: &AnnotationFilter) -> Vec<Annotation> {
        self.annotations
            .values()
            .filter(|a| filter.matches(a))
            .cloned()
            .collect()
    }

    /// 文本作用域 → 已校验页区间（All = 全档展开；越界 = InvalidPage
    /// 携带肇事上界页号）。
    fn resolve_span(&self, target: TextTarget) -> Result<PageSpan, CommandError> {
        let span = match target {
            // All = 全档展开：open 已拒 0 页档，减法无下溢；0..=count-1
            // 恒满足 start <= end，构造不会失败（expect 理由 = 上述不变量）。
            TextTarget::All => PageSpan::new(PageIndex(0), PageIndex(self.meta.page_count.0 - 1))
                .expect("全档展开恒非反转（open 已拒 0 页档）"),
            TextTarget::Pages(span) => span,
        };
        if !span.within(self.meta.page_count) {
            return Err(CommandError::InvalidPage(span.end()));
        }
        Ok(span)
    }
}

// ---------------------------------------------------------------------------
// 搜索内部：匹配扫描与摘录（v0 注入文本面；PDF-c 换引擎后本节移除）
// ---------------------------------------------------------------------------

/// 全匹配扫描（返回匹配**首字节偏移**，升序；两模式一致的非重叠语义——
/// pdf-b-1r 审计修复：原不敏感路径逐窗口扫描产出重叠命中，
/// 与敏感路径 `match_indices` 的非重叠计数不一致）。
///
/// 大小写敏感 = `match_indices` 直扫（子串搜索最优复杂度路径）；
/// 不敏感 = Unicode 简单折叠（`char::to_lowercase`）逐位对齐——折叠后
/// 字节长可能改变（如 `İ` → `i̇` 两字符），**不能**整串 lower 后索引
/// 对位（会错位报告），故按字符窗口对齐折叠序列；命中后跳过已消费
/// 字符（非重叠）。
fn find_matches(text: &str, needle: &str, case_insensitive: bool) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new(); // 空模式无语义（命令层已拒空 query，此处防御）
    }
    if !case_insensitive {
        return text.match_indices(needle).map(|(i, _)| i).collect();
    }
    let folded: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    let hay: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut window = 0usize;
    while window < hay.len() {
        let mut ni = 0usize; // folded 游标
        let mut hi = window; // hay 游标
        let mut matched = true;
        'attempt: while ni < folded.len() {
            let Some(&(_, hay_char)) = hay.get(hi) else {
                matched = false; // 尾窗口提前耗尽
                break 'attempt;
            };
            for folded_char in hay_char.to_lowercase() {
                let Some(&want) = folded.get(ni) else {
                    matched = false;
                    break 'attempt;
                };
                if folded_char != want {
                    matched = false;
                    break 'attempt;
                }
                ni += 1;
            }
            hi += 1;
        }
        if matched {
            out.push(hay[window].0);
            // 非重叠：从已消费字符的下一位置继续（对齐 match_indices 语义）。
            window = hi;
        } else {
            window += 1;
        }
    }
    out
}

/// 命中点摘录：以匹配首字节为中心，向两侧各取至多 `max_chars / 2` 个
/// **字符**（char 边界安全——中文/emoji 不撕裂），总长钳制在
/// `max_chars` 内（S3 ❷）。窗口不含前导/后缀省略号装饰（摘录是数据
/// 非展示；展示层自行加省略标记）。
fn snippet_around(text: &str, match_byte: usize, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let half = max_chars / 2;
    let start = step_back_chars(text, match_byte, half);
    let end = step_forward_chars(text, match_byte, half);
    let window = &text[start..end];
    // 双侧各 half 可能超出 max_chars 一个字符（奇数上限的中心对齐）。
    if window.chars().count() > max_chars {
        window.chars().take(max_chars).collect()
    } else {
        window.to_string()
    }
}

/// 从字节位向左走 `n` 个字符（返回字符边界字节位）。
fn step_back_chars(text: &str, from: usize, n: usize) -> usize {
    let mut i = align_char_boundary(text, from);
    let mut left = n;
    while left > 0 && i > 0 {
        i -= 1;
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        left -= 1;
    }
    i
}

/// 从字节位向右走 `n` 个字符（返回字符边界字节位）。
fn step_forward_chars(text: &str, from: usize, n: usize) -> usize {
    let mut i = align_char_boundary(text, from);
    let mut left = n;
    while left > 0 && i < text.len() {
        i += 1;
        while i < text.len() && !text.is_char_boundary(i) {
            i += 1;
        }
        left -= 1;
    }
    i
}

/// 任意字节位 → 就近字符边界（匹配起点恒为边界，此处为防御性对齐）。
fn align_char_boundary(text: &str, mut i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}
