//! `PdfSession`——PDF 文档会话状态机（PDF-b 骨架轮；PDF-c 阅读面增
//! [`PdfSession::attach_engine`] 引擎数据源；PDF-d 标注面增
//! [`PdfSession::attach_writer`] 增量写器 + [`PdfSession::flush_annotations`]
//! 保存事务）。
//!
//! 状态机只有**一个形态：已打开**（未打开态在类型层不存在——`open` 是
//! 唯一构造出口，失败走 [`OpenError`]；协议层的"槽位无会话"由宿主以
//! [`crate::command::CommandError::NotOpen`] 表达，见 protocol.rs）。
//!
//! 数据源三级：
//! - **v0 宿主注入**（无引擎）：页几何/大纲走 `open` 参数，页文本走
//!   [`PdfSession::set_page_text`]（PDF-b 骨架路径，保留为无引擎环境的
//!   降级通道）；
//! - **引擎实装**（[`PdfSession::attach_engine`]，PDF-c）：ExtractText/
//!   Search/Outline 命令与 `page_geometry` 查询改走
//!   [`crate::engine::DocumentEngine`] 实数据（pdfium），**命令面/载荷/
//!   错误语义零变化**。引擎可 [`PdfSession::detach_engine`] 取出（移交
//!   渲染工人）；
//! - **增量写器**（[`PdfSession::attach_writer`]，PDF-d）：标注落盘走
//!   [`crate::writer::AnnotationWriter`]。**flush 是会话 API 非命令**
//!   （六命令零变化）：宿主触发的保存事务，不是命令面成员。
//!
//! flush 语义（语义锁背书，tests/pdfd_session_flush.rs）：
//! - **S5 先于一切**（加密态拒——早于写器装配检查与零脏短路）；
//! - **脏集合精确跟踪**：AnnotateAdd 入脏；AnnotateEdit 仅实际内容变更
//!   重新标脏（no-op edit 不脏——与 modified_ms 不推进同语义：脏 = 内容
//!   面变更非命令次数）；flush 成功才清脏；
//! - **幂等**：零脏 flush 不追加字节（bytes_appended 最小化）；
//! - **重试安全**：写器幂等合同（同 id 重写 = 原位替换）背书——保存
//!   失败后 dirty 原样保留，重试不产生重复对象。
//!
//! 结构面变更（gates PDF-c 在档）：`Clone` derive 移除（引擎句柄
//! （pdfium 文档）不可语义复制）；`Debug` 改手写（引擎字段打印在位
//! 标记）。时间戳取会话时钟（宿主注入——核不取系统时间，对齐 mdcore
//! 时钟注入纪律：wasm32 无 std 时钟，宿主是唯一时间源）。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::annotation::{AnnotId, Annotation, AnnotationDraft, AnnotationPatch};
use crate::command::{
    AnnotationFilter, Command, CommandError, Outcome, SearchHit, SearchOptions, TextTarget,
};
use crate::engine::DocumentEngine;
use crate::outline::{OutlineTarget, OutlineTree};
use crate::page::{PageCount, PageGeometry, PageIndex, PageSpan};
use crate::security::{
    LimitBehavior, Limits, OpenAssessment, REASON_AUTHOR_LENGTH, REASON_ENCRYPTED_WRITE,
};
use crate::writer::{AnnotationWriter, IncrementalSave};

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

/// 引擎装配失败（typed——宿主接线错误 fail-fast，不静默降级；与
/// [`OpenError`] 同款纪律：装配期校验优于执行期静默错果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachEngineError {
    /// 引擎页数与会话元数据页数不符（双侧数值携带——T1 审计 O3 项：
    /// Search/ExtractText 以 `meta.page_count` 为上界，双权威不一致的
    /// 两条静默失败路径（引擎页多 → 漏末尾页 / 页少 → 整命令
    /// `InvalidPage`）在装配面收口为单一显式拒绝）。拒绝时引擎 drop
    /// （与 `open` 消耗参数的失败语义一致），会话数据源不变（首次
    /// 装配拒绝 → 仍无引擎 = v0 注入路径；二次注入拒绝 → 旧引擎保持
    /// 在位）。
    PageCountMismatch {
        /// 会话元数据页数（`meta.page_count`——打开期冻结的权威）。
        expected: u32,
        /// 引擎侧页数（[`DocumentEngine::page_count`]）。
        engine: u32,
    },
}

/// PDF 文档会话（打开后单形态；命令的唯一调度入口 = [`PdfSession::execute`]）。
pub struct PdfSession {
    meta: DocumentMeta,
    assessment: OpenAssessment,
    limits: Limits,
    pages: Vec<PageGeometry>,
    outline: OutlineTree,
    /// v0 文本层（宿主注入；无引擎时的降级通道）。BTreeMap = 页号升序确定性。
    page_text: BTreeMap<u32, String>,
    /// 标注库（id 升序迭代 = BTreeMap 键序，确定性）。
    annotations: BTreeMap<AnnotId, Annotation>,
    /// 引擎数据源（PDF-c：注入后 ExtractText/Search/Outline/几何走实数据；
    /// None = v0 宿主注入路径）。
    engine: Option<Box<dyn DocumentEngine>>,
    /// 标注增量写器（PDF-d：flush 的落盘通道；None = 未装配——零脏时
    /// flush 幂等返回，有脏时 flush 报 Engine 错（写器装配是保存事务的
    /// 前置，非打开前置——与引擎独立装配，不互为前提）。
    annot_writer: Option<Box<dyn AnnotationWriter>>,
    /// 脏标注集（PDF-d：待落盘标注——Add 入脏 / Edit 实际变更重标 / flush
    /// 成功清空；升序遍历 = BTreeSet 键序确定性）。
    dirty_annots: BTreeSet<AnnotId>,
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
            engine: None,
            annot_writer: None,
            dirty_annots: BTreeSet::new(),
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
            Command::Outline { source: _source } => Ok(self.outline_outcome()?),
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

    /// 注入页文本（v0 文本层——宿主/测试面；无引擎环境的降级通道，
    /// PDF-b 路径保留）。页界校验即时执行。
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

    /// 注入引擎数据源（PDF-c 阅读面）：ExtractText/Search/Outline 命令与
    /// [`PdfSession::page_geometry`] 查询改走引擎实数据（pdfium）；
    /// 命令面/载荷/限额语义零变化（语义锁背书：注入与引擎两路径同锁）。
    /// 二次注入直接替换在位引擎（旧引擎 drop）。
    ///
    /// **装配期一致性校验**（T1 审计 O3 项，替代旧调用方合同）：引擎
    /// 页数（[`DocumentEngine::page_count`]）≠ `meta.page_count` 时
    /// typed 拒绝（[`AttachEngineError::PageCountMismatch`，双侧数值
    /// 携带）——Search/ExtractText 以 `meta.page_count` 为上界，双权威
    /// 不一致的两条静默失败路径（引擎页多 → 静默漏末尾页；引擎页少 →
    /// 整命令 `InvalidPage`）在装配面收口为单一显式失败。拒绝时引擎
    /// drop（与 `open` 消耗参数的失败语义一致），会话数据源不变
    /// （首次装配拒绝 → 仍无引擎 = v0 注入路径；二次注入拒绝 → 旧
    /// 引擎保持在位——拒绝路径零副作用）；宿主接线层仍应同源构造两者
    /// （校验是防线非许可）。
    pub fn attach_engine(
        &mut self,
        engine: Box<dyn DocumentEngine>,
    ) -> Result<(), AttachEngineError> {
        let engine_pages = engine.page_count();
        if engine_pages != self.meta.page_count.0 {
            return Err(AttachEngineError::PageCountMismatch {
                expected: self.meta.page_count.0,
                engine: engine_pages,
            });
        }
        self.engine = Some(engine);
        Ok(())
    }

    /// 取出引擎（会话回到 v0 宿主注入路径；引擎可移交渲染工人——
    /// [`crate::worker::RenderWorker`] 以所有权接收 `Rasterizer`）。
    pub fn detach_engine(&mut self) -> Option<Box<dyn DocumentEngine>> {
        self.engine.take()
    }

    /// 引擎在位观测（数据源级别判定：false = 宿主注入路径）。
    pub fn engine_attached(&self) -> bool {
        self.engine.is_some()
    }

    /// 注入标注增量写器（PDF-d 标注面）：落盘通道。与引擎装配**独立**
    /// （无引擎会话可落盘标注；有引擎会话可不装配写器——阅读与落盘
    /// 两个能力面互不为前提）。二次装配直接替换在位写器（旧写器 drop）。
    ///
    /// **调用方合同**（与 `attach_engine` 页数合同同款纪律）：写器字节与
    /// 会话 `meta.page_count` 必须同源同档——写器文档页少时 flush 以
    /// `InvalidPage` 显式失败（安全侧：显式失败优于静默错页）。
    pub fn attach_writer(&mut self, writer: Box<dyn AnnotationWriter>) {
        self.annot_writer = Some(writer);
    }

    /// 写器在位观测。
    pub fn writer_attached(&self) -> bool {
        self.annot_writer.is_some()
    }

    /// 会话时钟注入（epoch ms）。标注 `created_ms`/`modified_ms` 取自
    /// 会话时钟——核不取系统时间（确定性/可测性/wasm32 兼容）。
    pub fn set_clock_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    /// flush：保存事务（会话 API 非命令——六命令面零变化）。语义序：
    /// ① S5 加密态拒（先于一切——含零脏与写器装配检查，策略门最前）
    /// → ② 零脏幂等短路（不追加字节）→ ③ 逐条写（脏集升序）→
    /// ④ save_incremental → 成功才清脏。任一步失败：dirty 原样保留
    /// （重试经写器幂等合同安全——同 id 重写 = 原位替换）。
    pub fn flush_annotations(&mut self) -> Result<IncrementalSave, CommandError> {
        // ① S5 先于一切（与 AnnotateAdd/Edit 同纪律——策略门不因零脏
        // 或未装配写器而短路漂移）。
        if self.assessment.denies_writes() {
            return Err(CommandError::SecurityDeny {
                reason: REASON_ENCRYPTED_WRITE,
            });
        }
        // ② 零脏幂等：不追加字节。写器在位 → 委托 save_incremental
        // （写器侧零待保存 = 零追加 + 真产物长度）；无写器且零脏 → 无
        // 产物面（双 0 常数——未落盘会话的诚实占位，非伪造长度）。
        if self.dirty_annots.is_empty() {
            return match self.annot_writer.as_mut() {
                Some(writer) => writer.save_incremental(),
                None => Ok(IncrementalSave {
                    bytes_appended: 0,
                    prefix_bytes: 0,
                }),
            };
        }
        let Some(writer) = self.annot_writer.as_mut() else {
            return Err(CommandError::engine(
                "标注写器未装配（attach_writer）：有脏标注待落盘",
            ));
        };
        // ③ 逐条写（脏集升序——BTreeSet 键序确定性，与 AnnotateList 的
        // id 升序纪律同源）。
        let ids: Vec<AnnotId> = self.dirty_annots.iter().copied().collect();
        for id in &ids {
            let annot = self
                .annotations
                .get(id)
                .expect("脏标注必在库（v0 无删除命令——dirty 只经 Add/Edit 产生）");
            writer.write_annotation(annot.page, annot)?;
        }
        // ④ 保存 + 清脏（成功才清：保存失败 → dirty 原样保留，重试安全）。
        let stats = writer.save_incremental()?;
        self.dirty_annots.clear();
        Ok(stats)
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
    /// 引擎在位时走引擎实数据（引擎错误折叠为 None——查询面不报错纪律
    /// 保持；引擎面错误经命令面可见）。
    pub fn page_geometry(&self, idx: PageIndex) -> Option<PageGeometry> {
        if let Some(engine) = self.engine.as_deref() {
            return engine.page_geometry(idx).ok();
        }
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

    /// ExtractText：页集形状先行冻结（引擎路径 = 引擎逐页实数据；注入
    /// 路径 = 注入面，未注入页空串——载荷形状两路径一致）。
    fn extract_text(&self, target: TextTarget) -> Result<Outcome, CommandError> {
        let span = self.resolve_span(target)?;
        if let Some(engine) = self.engine.as_deref() {
            // PDF-c 引擎路径：真实文本数据源（命令面/载荷形状不变）。
            return Ok(Outcome::ExtractedText {
                pages: engine.extract_text(span)?,
            });
        }
        let mut pages = Vec::with_capacity(span.len() as usize);
        for idx in span.iter() {
            let text = self.page_text.get(&idx.0).cloned().unwrap_or_default();
            pages.push((idx, text));
        }
        Ok(Outcome::ExtractedText { pages })
    }

    /// Search：限额/摘录/截断语义单一出口（两路径同锁）；引擎路径逐页
    /// 抽取后复用同一收集器（find_matches/snippet_around——注入路径的
    /// 非重叠/Unicode 折叠/char 边界语义逐字保持）。
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
        // 收集器：单页命中收集；返回 true = 收满（❷钳制告知，见收满分支）。
        let mut collect = |page_no: u32, text: &str| -> bool {
            for byte_idx in find_matches(text, &query, opts.case_insensitive) {
                if hits.len() >= max_hits as usize {
                    // 已收满且仍见新命中 → ❷钳制告知（不静默丢弃）。
                    truncated = true;
                    return true;
                }
                let snippet = snippet_around(text, byte_idx, snippet_chars);
                hits.push(SearchHit {
                    page: PageIndex(page_no),
                    snippet,
                });
            }
            false
        };
        if let Some(engine) = self.engine.as_deref() {
            // PDF-c 引擎路径：逐页抽取（流式，不整档驻留文本）。
            'engine: for page_no in 0..self.meta.page_count.0 {
                // start == end 单页区间——构造恒非反转（PageSpan 不变量）。
                let span = PageSpan::new(PageIndex(page_no), PageIndex(page_no))
                    .expect("单页区间恒非反转");
                let text = engine
                    .extract_text(span)?
                    .into_iter()
                    .next()
                    .map(|(_, text)| text)
                    .unwrap_or_default();
                if collect(page_no, &text) {
                    break 'engine;
                }
            }
        } else {
            'injected: for page_no in 0..self.meta.page_count.0 {
                let Some(text) = self.page_text.get(&page_no) else {
                    continue;
                };
                if collect(page_no, text) {
                    break 'injected;
                }
            }
        }
        Ok(Outcome::SearchResults { hits, truncated })
    }

    /// Outline：引擎在位 → 引擎实数据（引擎侧已做页界防御）；否则
    /// v0 注入树（open 期已校验目标页在档）。
    fn outline_outcome(&self) -> Result<Outcome, CommandError> {
        match self.engine.as_deref() {
            Some(engine) => Ok(Outcome::Outline(engine.outline()?)),
            None => Ok(Outcome::Outline(self.outline.clone())),
        }
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
        // PDF-d：新增即脏（待落盘集）。
        self.dirty_annots.insert(id);
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
            // PDF-d：实际内容变更 → 重新标脏（no-op edit 不脏——与
            // modified_ms 不推进同语义：脏 = 内容面变更非命令次数）。
            self.dirty_annots.insert(id);
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
// 手写 Debug（PDF-c）：引擎/写器字段不可推导（Box<dyn …>）——打印在位
// 标记与计数面；Clone 已移除（结构面变更，gates 在档）。
// ---------------------------------------------------------------------------

impl fmt::Debug for PdfSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdfSession")
            .field("meta", &self.meta)
            .field("assessment", &self.assessment)
            .field("limits", &self.limits)
            .field("pages", &self.pages.len())
            .field("outline", &self.outline)
            .field("page_text_pages", &self.page_text.len())
            .field("annotations", &self.annotations.len())
            .field("engine_attached", &self.engine.is_some())
            .field("writer_attached", &self.annot_writer.is_some())
            .field("dirty_annots", &self.dirty_annots.len())
            .field("next_annot_id", &self.next_annot_id)
            .field("now_ms", &self.now_ms)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// 搜索内部：匹配扫描与摘录（注入与引擎两路径共用的单一语义出口）
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
