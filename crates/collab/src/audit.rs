//! 外部 wire 数据审计与修复面（交付报告 §4.2 撤销耦合防护收口）。
//!
//! # 产品语义
//!
//! 撤销耦合防护依赖**写入方约定**：文本承载块"出生即带文本子节点"
//! （`build_prelim`——否则首笔文本编辑才建节点，undo 首笔 = 删整个
//! 节点含后续并发内容）。本 crate 为唯一写入方时不变量恒成立；但
//! 外部直写 wire 的数据（Yjs 客户端直操作载体 / 旧数据迁移）无此
//! 保证——交付报告 §4.2 登记"集成时需校验或迁移"，本模块即该面：
//!
//! - [`CollabSession::audit_update`]：**应用前审计**——在隔离暂存
//!   Doc 上应用 update 后走查读模型，零本端副作用；集成方对外部
//!   数据先审后用（`is_clean` 才 `apply_update`）。
//! - [`CollabSession::audit_self`]：**应用后验证**——走查本端当前
//!   状态（外部数据已合并后的在位校验）。
//! - [`CollabSession::repair_text_nodes`]：**机械修复**——为缺失
//!   文本节点的文本承载块补建空文本节点（撤销耦合防护的唯一
//!   Error 级可机械修复面；origin = `mdc:maintenance` 不进撤销栈，
//!   产物经事件可转发——全编队防护）。
//!
//! # 审计口径（本引擎作为唯一写入方时恒成立的不变量）
//!
//! **Error 级**（引擎不变量破坏——集成面应修复/迁移后再用）：
//! - `MissingTextNode`：文本承载块 own-text 段无任何文本节点
//!   （撤销耦合防护破坏面——修复通道 = `repair_text_nodes`）。
//! - `MissingBlockId` / `DuplicateBlockId`：身份面（`block_id`
//!   缺失/空串/重复——`find_block`/锚点/物化身份语义破坏）。
//! - `ForeignRootType`：根 `doc` 不是 XmlFragment（读模型整体失效）。
//! - `WalkFailure`：走查中引擎读面 panic（形状超出已知域——防御
//!   纵深收敛，不波及进程；`audit_update` 面对应 `Err(Panic)`）。
//!
//! **Warning 级**（形状漂移——读模型有定义行为但非本引擎书写形状，
//! 静默丢弃/强转面逐项登记，集成方按需处置）：
//! - `UnknownTag`：未知元素标签（读模型强转 Paragraph）。
//! - `RootLevelText` / `FragmentChild` / `TextAfterChildren`：读模型
//!   静默丢弃的承载位置。
//! - `ThematicBreakWithText`：hr 带文本（引擎操作面拒绝，形状漂移）。
//! - `MarkMissingKind` / `MarkForeignChild`：mark 元素形状缺陷
//!   （读回静默丢标记/丢后续内容）。
//! - `NonCanonicalAttrKey`：属性键非规范形（`escape(unescape(k)) != k`
//!   ——键名往返不对称面；本引擎恒写规范形）。
//! - `NestedUnderNonContainer`：子块挂非容器块下（引擎操作面不可
//!   产生/不可再插入）。
//! - `ForeignRootData`：`doc` 之外的根承载数据（读模型不可见——
//!   update 携带外域根时集成侧"为何数据不显现"的诊断面）。
//!
//! # 边界（诚实登记）
//!
//! - 审计反映 update 在**空暂存 Doc 上的独立终态**，非与本端状态
//!   合并后的终态（差量依赖不全时形状可能不同）；合并后验证用
//!   `audit_self`。
//! - `repair_text_nodes` 只修复 `MissingTextNode` 一类（可机械修复
//!   且语义无歧义）；其余违规（身份缺失/重复、形状漂移）不可机械
//!   修复——发明身份或形状决策属产品语义，不在本模块。
//! - 修复产物为空文本节点插入（origin = `mdc:maintenance`——不在
//!   Human/Ai 追踪集，不进任何撤销栈；宿主转发 updates 即对端同
//!   步获得防护）。

use std::collections::HashSet;
use std::panic::AssertUnwindSafe;

use yrs::types::xml::XmlTextPrelim;
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, Map, Options as DocOptions, Origin, Out, ReadTxn, Transact, Update, Xml, XmlFragment,
};

use crate::session::{
    attr_str, block_id_of, escape_key, panic_message_pub, unescape_key, ATTR_ID, MARK_TAG, ROOT,
};
use crate::{BlockKind, CollabError, CollabSession};

/// mark 元素的标记种类属性键（`set_mark` 写入面单点常量的审计镜像）。
const MARK_KIND: &str = "kind";
/// 维护修复事务 origin（不在 Human/Ai 追踪集——不进任何撤销栈）。
const MAINTENANCE_ORIGIN: &str = "mdc:maintenance";
/// 走查中身份缺失块的报告占位（不与真实 block_id 撞车的显式记号）。
const MISSING_ID: &str = "<missing-id>";

/// 审计违规（Error/Warning 分级见模块文档）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditViolation {
    // ── Error 级（引擎不变量破坏）────────────────────────────────
    /// 文本承载块 own-text 段无文本节点（撤销耦合防护破坏面）。
    MissingTextNode { block_id: String, tag: String },
    /// 块缺 `block_id` 属性（含空串——身份面退化，读模型 block_id = ""）。
    MissingBlockId { tag: String },
    /// `block_id` 重复（身份冲突——寻址/锚点语义破坏）。
    DuplicateBlockId { block_id: String },
    /// 根 `doc` 承载非 XmlFragment 形态内容（本地类型异型 / 键值或
    /// 序列条目混入——读模型不可见，探针实证 children() 零产出）。
    ForeignRootType,
    /// 走查中引擎读面 panic（形状超出已知域——防御纵深收敛面）。
    WalkFailure,
    // ── Warning 级（形状漂移——静默丢弃/强转面登记）──────────────
    /// 未知元素标签（读模型强转 Paragraph）。
    UnknownTag { tag: String },
    /// 根级裸文本节点（读模型丢弃）。
    RootLevelText,
    /// 嵌套 XML fragment 子节点（读模型丢弃）。
    FragmentChild,
    /// hr 块带文本节点（引擎操作面拒绝，形状漂移）。
    ThematicBreakWithText { block_id: String },
    /// mark 元素缺 `kind` 属性（或空串——读回静默丢标记，文本保留）。
    MarkMissingKind { block_id: String },
    /// mark 元素内含非 Text/mark 子元素（读模型丢弃其后内容）。
    MarkForeignChild { block_id: String },
    /// 属性键非规范形（`escape(unescape(k)) != k`——往返不对称面）。
    NonCanonicalAttrKey { key: String },
    /// 子块挂非容器块下（引擎不可产生/不可再插入）。
    NestedUnderNonContainer {
        parent_block_id: String,
        parent_tag: String,
        child_block_id: String,
    },
    /// own-text 段结束后的 Text/mark 子节点（死承载——读模型丢弃）。
    TextAfterChildren { block_id: String },
    /// `doc` 之外的根承载数据（读模型不可见）。
    ForeignRootData { name: String },
}

/// 审计报告（`audit_update` / `audit_self` 产物）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditReport {
    /// 非根块元素总数（含未知标签元素；不含 mark 元素）。
    pub blocks_total: usize,
    /// mark 元素总数（嵌套层各计一）。
    pub marks_total: usize,
    /// Error 级违规（引擎不变量破坏）。
    pub errors: Vec<AuditViolation>,
    /// Warning 级违规（形状漂移）。
    pub warnings: Vec<AuditViolation>,
}

impl AuditReport {
    /// Error 级为零（引擎不变量成立——可安全协作）。
    ///
    /// Warning 不阻断（形状漂移由集成方按需处置）；`errors` 与
    /// `warnings` 双空 = 本引擎规范形状。
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

impl CollabSession {
    /// 应用前审计：update 在隔离暂存 Doc 上的独立终态走查
    /// （零本端副作用——本会话状态与撤销栈均不动）。
    ///
    /// 集成纪律：外部数据（非本引擎产出的 wire 字节）先审后用——
    /// `is_clean()` 才 `apply_update()`；`MissingTextNode` 可经
    /// [`Self::repair_text_nodes`] 修复后复审。
    pub fn audit_update(&self, bytes: &[u8]) -> Result<AuditReport, CollabError> {
        self.check_inbound(bytes.len())?;
        // 暂存 Doc：B1.1 skip_gc 同构；client_id 无关（零本地操作）。
        let opts = DocOptions {
            skip_gc: true,
            ..Default::default()
        };
        let scratch = Doc::with_options(opts);
        // 应用（panic 隔离——与 apply_update 同构）。
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let update =
                Update::decode_v1(bytes).map_err(|e| CollabError::Decode(e.to_string()))?;
            let mut txn = scratch.transact_mut();
            txn.apply_update(update)
                .map_err(|e| CollabError::Decode(e.to_string()))?;
            txn.commit();
            Ok(())
        }))
        .map_err(|p| CollabError::Panic(panic_message_pub(&p)))??;
        // 走查（panic 隔离——敌意形状防御纵深）。
        std::panic::catch_unwind(AssertUnwindSafe(|| audit_doc(&scratch)))
            .map_err(|p| CollabError::Panic(panic_message_pub(&p)))
    }

    /// 应用后验证：本端当前状态走查（外部数据合并后的在位校验）。
    ///
    /// 走查 panic 收敛为 `WalkFailure` Error（防御纵深——本端状态
    /// 可被既往外部 update 写入任意形状）。
    pub fn audit_self(&self) -> AuditReport {
        std::panic::catch_unwind(AssertUnwindSafe(|| audit_doc(self.doc()))).unwrap_or_else(|_| {
            let mut r = AuditReport::default();
            r.errors.push(AuditViolation::WalkFailure);
            r
        })
    }

    /// 机械修复：为缺失文本节点的文本承载块补建空文本节点
    /// （撤销耦合防护收口——修复后首笔编辑必落入既有节点，undo
    /// 恒只撤字符项）。
    ///
    /// - 单事务原子（全成或全不成）；origin = `mdc:maintenance`
    ///   ——不进任何撤销栈（Human/Ai 栈深不变）。
    /// - 产物派发 [`crate::events::ChangeSource::Maintenance`] 事件
    ///   （updates 非空——宿主转发对端即全编队防护）。
    /// - 幂等：已修复/本就完好的会话返回 0（零事务零事件）。
    /// - 只修复 `MissingTextNode` 一类；其余违规不可机械修复
    ///   （见模块文档边界）。
    pub fn repair_text_nodes(&mut self) -> Result<usize, CollabError> {
        // 读事务收集缺失面（p18 纪律：get_or_insert 内部开写事务——
        // 必须先于读事务调用；读事务出域后再开修复写事务）。
        let missing: Vec<(yrs::XmlElementRef, String)> = {
            let frag = self.doc().get_or_insert_xml_fragment(ROOT);
            let txn = self.doc().transact();
            let mut out = Vec::new();
            collect_missing_text_nodes(&frag, &txn, &mut out);
            out
        };
        if missing.is_empty() {
            return Ok(0);
        }
        let n = missing.len();
        self.collector.begin();
        let doc = self.doc();
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut txn = doc.transact_mut_with(Origin::from(MAINTENANCE_ORIGIN));
            for (el, _) in &missing {
                el.insert(&mut txn, 0, XmlTextPrelim::new(""));
            }
            txn.commit();
        }));
        match r {
            Ok(()) => {
                let updates = self.collector.drain();
                if !updates.is_empty() {
                    self.hub.dispatch(&crate::events::CollabEvent {
                        source: crate::events::ChangeSource::Maintenance,
                        updates,
                    });
                }
                Ok(n)
            }
            Err(p) => {
                let _ = self.collector.drain();
                Err(CollabError::Panic(panic_message_pub(&p)))
            }
        }
    }
}

// ── 走查实现 ─────────────────────────────────────────────────────

/// Doc 根盘点 + `doc` 根走查（audit_update / audit_self 共用）。
///
/// 根类型判别（探针实证）：update 应用后的根为 `UndefinedRef`
/// （yrs 根类型 = 本地约定，wire 不携带）——按 XmlFragment 视图
/// 走查，非 XML 承载由内容判别面甄别：Map 视图键值条目 / 列表长
/// 与 XML 子节点数差额（数组式条目对 children() 不可见）。
fn audit_doc(doc: &Doc) -> AuditReport {
    let mut report = AuditReport::default();
    let txn = doc.transact();
    let mut doc_root: Option<(yrs::XmlFragmentRef, yrs::branch::BranchPtr)> = None;
    for (name, out) in txn.root_refs() {
        if name == ROOT {
            match out {
                Out::YXmlFragment(f) => {
                    let ptr = {
                        let b: &yrs::branch::Branch = f.as_ref();
                        yrs::branch::BranchPtr::from(b)
                    };
                    doc_root = Some((f, ptr));
                }
                Out::UndefinedRef(ptr) => {
                    doc_root = Some((yrs::XmlFragmentRef::from(ptr), ptr));
                }
                _ => report.errors.push(AuditViolation::ForeignRootType),
            }
        } else {
            report
                .warnings
                .push(AuditViolation::ForeignRootData { name: name.into() });
        }
    }
    if let Some((frag, ptr)) = doc_root {
        let map_entries = yrs::MapRef::from(ptr).len(&txn);
        let mut walked = 0u32;
        let mut seen = HashSet::new();
        for c in frag.children(&txn) {
            walked += 1;
            match c {
                yrs::XmlOut::Text(_) => {
                    report.warnings.push(AuditViolation::RootLevelText);
                }
                yrs::XmlOut::Element(e) => {
                    audit_element(&e, &txn, &mut seen, &mut report);
                }
                yrs::XmlOut::Fragment(_) => {
                    report.warnings.push(AuditViolation::FragmentChild);
                }
            }
        }
        // 非 XmlFragment 形态承载（键值/序列条目）——读模型不可见。
        if map_entries > 0 || frag.len(&txn) > walked {
            report.errors.push(AuditViolation::ForeignRootType);
        }
    }
    report
}

/// 单块元素走查（递归）。
fn audit_element(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    seen: &mut HashSet<String>,
    report: &mut AuditReport,
) {
    report.blocks_total += 1;
    let tag = el.tag().to_string();
    let kind = BlockKind::from_tag(&tag);
    if kind.is_none() {
        report
            .warnings
            .push(AuditViolation::UnknownTag { tag: tag.clone() });
    }
    // 身份面（Error 级）。
    let id = match block_id_of(el, txn) {
        Some(s) if !s.is_empty() => {
            if !seen.insert(s.clone()) {
                report.errors.push(AuditViolation::DuplicateBlockId {
                    block_id: s.clone(),
                });
            }
            Some(s)
        }
        _ => {
            report
                .errors
                .push(AuditViolation::MissingBlockId { tag: tag.clone() });
            None
        }
    };
    let bid = id.unwrap_or_else(|| MISSING_ID.into());
    // 属性键规范形（ATTR_ID 载体键恒规范，跳过）。
    for (k, _) in el.attributes(txn) {
        if k != ATTR_ID && escape_key(&unescape_key(k)) != k {
            report
                .warnings
                .push(AuditViolation::NonCanonicalAttrKey { key: k.into() });
        }
    }
    // own-text 段构成分析（单遍：直接 Text / mark 递归 / 子块 / 死承载）。
    let mut own_text_nodes = 0usize;
    let mut in_own_segment = true;
    let mut child_blocks: Vec<yrs::XmlElementRef> = Vec::new();
    for c in el.children(txn) {
        match c {
            yrs::XmlOut::Text(_) => {
                if in_own_segment {
                    own_text_nodes += 1;
                } else {
                    report.warnings.push(AuditViolation::TextAfterChildren {
                        block_id: bid.clone(),
                    });
                }
            }
            yrs::XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG => {
                if in_own_segment {
                    own_text_nodes += audit_mark(&e, &bid, txn, report);
                } else {
                    report.warnings.push(AuditViolation::TextAfterChildren {
                        block_id: bid.clone(),
                    });
                }
            }
            yrs::XmlOut::Element(e) => {
                in_own_segment = false;
                child_blocks.push(e);
            }
            yrs::XmlOut::Fragment(_) => {
                report.warnings.push(AuditViolation::FragmentChild);
            }
        }
    }
    // 撤销耦合防护面（Error 级——仅已知种类；未知标签元素经操作面
    // 恒 BlockNotFound，无耦合路径）。
    if let Some(k) = kind {
        if k.carries_text() && own_text_nodes == 0 {
            report.errors.push(AuditViolation::MissingTextNode {
                block_id: bid.clone(),
                tag: tag.clone(),
            });
        }
        if k == BlockKind::ThematicBreak && own_text_nodes > 0 {
            report.warnings.push(AuditViolation::ThematicBreakWithText {
                block_id: bid.clone(),
            });
        }
    }
    // 子块挂载面 + 递归。
    for child in &child_blocks {
        if let Some(pk) = kind {
            if !pk.carries_children() {
                let child_bid = block_id_of(child, txn).unwrap_or_else(|| MISSING_ID.into());
                report
                    .warnings
                    .push(AuditViolation::NestedUnderNonContainer {
                        parent_block_id: bid.clone(),
                        parent_tag: tag.clone(),
                        child_block_id: child_bid,
                    });
            }
        }
        audit_element(child, txn, seen, report);
    }
}

/// mark 元素走查（递归嵌套层）：返回子树内文本节点数；
/// 形状缺陷登记（kind 缺失/空串 / 非 Text-mark 子元素）。
fn audit_mark(
    el: &yrs::XmlElementRef,
    block_id: &str,
    txn: &yrs::Transaction,
    report: &mut AuditReport,
) -> usize {
    report.marks_total += 1;
    let mut kind_ok = false;
    for (k, v) in el.attributes(txn) {
        if k == MARK_KIND {
            if let Some(s) = attr_str(&v) {
                if !s.is_empty() {
                    kind_ok = true;
                }
            }
        }
    }
    if !kind_ok {
        report.warnings.push(AuditViolation::MarkMissingKind {
            block_id: block_id.into(),
        });
    }
    let mut text_nodes = 0usize;
    for c in el.children(txn) {
        match c {
            yrs::XmlOut::Text(_) => text_nodes += 1,
            yrs::XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG => {
                text_nodes += audit_mark(&e, block_id, txn, report);
            }
            yrs::XmlOut::Element(_) => {
                report.warnings.push(AuditViolation::MarkForeignChild {
                    block_id: block_id.into(),
                });
            }
            yrs::XmlOut::Fragment(_) => {
                report.warnings.push(AuditViolation::FragmentChild);
            }
        }
    }
    text_nodes
}

/// 收集缺失文本节点的文本承载块（repair 专用轻量走查）。
fn collect_missing_text_nodes(
    parent: &yrs::XmlFragmentRef,
    txn: &yrs::Transaction,
    out: &mut Vec<(yrs::XmlElementRef, String)>,
) {
    for c in parent.children(txn) {
        if let yrs::XmlOut::Element(e) = c {
            collect_missing_in_element(&e, txn, out);
        }
    }
}

fn collect_missing_in_element(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    out: &mut Vec<(yrs::XmlElementRef, String)>,
) {
    let tag = el.tag();
    let kind = match BlockKind::from_tag(tag.as_ref()) {
        Some(k) => k,
        None => return, // 未知标签：操作面不可达（BlockNotFound），无耦合路径
    };
    // own-text 段文本节点计数（与 audit_element 同口径的复用单点）。
    if kind.carries_text() && crate::ops::own_text_nodes(el, txn).is_empty() {
        let id = block_id_of(el, txn).unwrap_or_default();
        out.push((el.clone(), id));
    }
    // 递归子块（含非容器挂载面——防御完备）。
    for c in el.children(txn) {
        if let yrs::XmlOut::Element(e) = c {
            if e.tag().as_ref() != MARK_TAG {
                collect_missing_in_element(&e, txn, out);
            }
        }
    }
}
