//! 会话核心：构造不变量 / 撤销 / 同步 / 快照 / 维护 / 读模型。

use std::collections::HashMap;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::time::{SystemTime, UNIX_EPOCH};

use yrs::types::xml::{XmlElementPrelim, XmlIn, XmlOut, XmlTextPrelim};
use yrs::undo::Options as UndoOptions;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{
    ClientID, Doc, GetString, Options as DocOptions, Origin, ReadTxn, StateVector, StickyIndex,
    Transact, UndoManager, Update, Xml, XmlFragment,
};

use crate::identity::CrdtAnchor;
use crate::{BlockInfo, BlockKind};

/// 协作引擎错误（公开面零 yrs 类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollabError {
    /// 输入超过限额（B1.6）。
    Limit { size: usize, limit: usize },
    /// 对端 wire 数据不合法（解码失败）。
    Decode(String),
    /// 引擎 panic 被隔离捕获。
    Panic(String),
    /// 调用面语义错误（参数不合法等）。
    InvalidInput(String),
    /// 目标块不存在（可能已被删除——身份面显式错误）。
    BlockNotFound { block_id: String },
    /// 文本区间越界（char 域）。
    BadRange { start: u32, end: u32, len: u32 },
    /// 父块不可承载子块（InsertBlock/MoveBlock 的父面约束）。
    NotAContainer { block_id: String },
    /// 目标块种类不支持该操作（如 ThematicBreak 无文本面）。
    KindIncompatible { block_id: String, op: &'static str },
    /// 属性键保留（`block_id` 为载体身份键）。
    ReservedAttrKey { key: String },
    /// 锚点悬空（目标块已删除——不静默返回邻居）。
    AnchorDangling { client: u64, clock: u32 },
    /// 锚点侧车格式不合法/版本不受支持。
    SidecarFormat(String),
}

impl std::fmt::Display for CollabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollabError::Limit { size, limit } => {
                write!(f, "input {size}B exceeds limit {limit}B")
            }
            CollabError::Decode(m) => write!(f, "wire decode failed: {m}"),
            CollabError::Panic(m) => write!(f, "engine panic isolated: {m}"),
            CollabError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            CollabError::BlockNotFound { block_id } => {
                write!(f, "block not found: {block_id}")
            }
            CollabError::BadRange { start, end, len } => {
                write!(
                    f,
                    "range [{start},{end}) out of bounds (text len {len} chars)"
                )
            }
            CollabError::NotAContainer { block_id } => {
                write!(f, "block {block_id} cannot carry child blocks")
            }
            CollabError::KindIncompatible { block_id, op } => {
                write!(f, "block {block_id} kind does not support {op}")
            }
            CollabError::ReservedAttrKey { key } => {
                write!(f, "attribute key {key:?} is reserved (carrier identity)")
            }
            CollabError::AnchorDangling { client, clock } => {
                write!(f, "anchor dangling (deleted target): ({client}, {clock})")
            }
            CollabError::SidecarFormat(m) => write!(f, "anchor sidecar format: {m}"),
        }
    }
}

impl std::error::Error for CollabError {}

/// 编辑来源语义（B3.2：撤销分栈 + origin 溯源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    Human,
    Ai,
}

impl Actor {
    /// origin 字节串常量（适配层单点定义）。
    pub(crate) fn wire(self) -> Origin {
        match self {
            Actor::Human => Origin::from("mdc:human"),
            Actor::Ai => Origin::from("mdc:ai"),
        }
    }
}

/// 会话选项。
///
/// **刻意不含 skip_gc**（B1.1 全编队不变量——构造期无条件强制）。
/// **刻意不含捕获窗**（p20 缺陷缓解——硬编码 0，非运行期参数）。
#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub client_id: u64,
    /// 全入向字节限额（B1.6；默认 52MiB 对齐 SECURITY_POLICY v1.2
    /// 声明总量族口径）。
    pub input_byte_limit: usize,
    /// 撤销栈软上限（B3.4 粗粒度策略：某 origin 栈深超过该值时
    /// clear_all 该栈——代价 = 该 origin 全部撤销历史丢失，诚实
    /// 登记非静默；`None` = 不限制[内存代价见 p19 定量]）。
    pub undo_stack_limit: Option<usize>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        SessionOptions {
            client_id: 7_100_001,
            input_byte_limit: 52 * 1024 * 1024,
            undo_stack_limit: None,
        }
    }
}

/// 载体根名（XmlFragment；元素名集合 {p,h,li,blockquote,pre,tbl,hr}
/// + mark 元素 `m` 共用子节点命名域——yrs-e §2.2）。
pub(crate) const ROOT: &str = "doc";
/// 载体身份属性键（保留键）。
pub(crate) const ATTR_ID: &str = "block_id";
/// mark 元素标签。
pub(crate) const MARK_TAG: &str = "m";

/// 协作会话（人类与 AI 共用的共享文档状态）。
pub struct CollabSession {
    doc: Doc,
    undo_human: UndoManager,
    undo_ai: UndoManager,
    options: SessionOptions,
    /// 自增块号（auto id 分配面——`{client_id}-{n}`）。
    next_seq: u64,
    /// 锚点表（BlockId → CRDT ID；结构性重建自动重指）。
    anchors: HashMap<String, CrdtAnchor>,
    /// 最近一次操作是否触发撤销栈清理（execute 回执面——清理发生在
    /// 操作内部，事后重算不可见，故清理时置标志）。
    undo_cleared_last: bool,
}

impl CollabSession {
    /// 构造面唯一入口（B1.2 单点构造 + B1.1 skip_gc 无条件）。
    pub fn open(options: SessionOptions) -> Result<CollabSession, CollabError> {
        // fail-closed：client_id 53 位上界（yjs 兼容域；yrs 仅
        // debug_assert，release 超界静默破坏 ID 编码——适配层显式拒）。
        if options.client_id == 0 || options.client_id & (u64::MAX << 53) != 0 {
            return Err(CollabError::InvalidInput(format!(
                "client_id {} 超出 (0, 2^53) 域（yjs 兼容）",
                options.client_id
            )));
        }
        let mut opts = DocOptions::with_client_id(ClientID::new(options.client_id));
        opts.skip_gc = true; // B1.1：无条件，无配置面
        let doc = Doc::with_options(opts);

        // 载体根物化（XmlFragment——yrs-e 映射矩阵定谳形状）。
        let xml = doc.get_or_insert_xml_fragment(ROOT);
        let _ = xml.len(&doc.transact());

        // B3.2 双栈 + 捕获窗 0（p20 缓解，硬编码——见模块文档）。
        let mut undo_human = Self::new_undo_manager(Actor::Human);
        let mut undo_ai = Self::new_undo_manager(Actor::Ai);
        undo_human.expand_scope(&doc, &doc.get_or_insert_xml_fragment(ROOT));
        undo_ai.expand_scope(&doc, &doc.get_or_insert_xml_fragment(ROOT));

        Ok(CollabSession {
            doc,
            undo_human,
            undo_ai,
            options,
            next_seq: 0,
            anchors: HashMap::new(),
            undo_cleared_last: false,
        })
    }

    /// 撤销管理器构造（每 origin 一栈——B3.2）。
    ///
    /// `capture_transaction` 拒 None-origin 事务：远端 update 应用面
    /// 不带 origin → **远端编辑不进本地撤销栈**（撤销域 = 本地；
    /// B3.1 撤销栈本地归属 + p12「对端墓碑 2%」观察面的产品收口）。
    /// 该过滤器同时阻断 yrs 的 len==1 回退（单 origin 集合会捕获
    /// None-origin 事务——Yjs 自身编辑器约定，不适用于本产品语义）。
    fn new_undo_manager(origin: Actor) -> UndoManager {
        let mut tracked = HashSet::new();
        tracked.insert(origin.wire());
        UndoManager::with_options(UndoOptions {
            capture_timeout_millis: 0, // p20：硬编码非配置面
            tracked_origins: tracked,
            capture_transaction: Some(std::sync::Arc::new(|txn| txn.origin().is_some())),
            timestamp: std::sync::Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            }),
            init_undo_stack: vec![],
            init_redo_stack: vec![],
        })
    }

    /// 故障恢复路径：从权威快照重建会话（open + apply，panic 隔离）。
    ///
    /// 撤销栈不随快照走（B3.1 撤销栈本地恒不序列化）——恢复后的
    /// 会话从空栈开始，只能撤销恢复后发生的新编辑。
    pub fn restore(options: SessionOptions, snapshot: &[u8]) -> Result<CollabSession, CollabError> {
        let s = CollabSession::open(options)?;
        s.apply_update(snapshot)?;
        Ok(s)
    }

    pub fn options(&self) -> &SessionOptions {
        &self.options
    }

    pub(crate) fn doc(&self) -> &Doc {
        &self.doc
    }

    pub(crate) fn check_inbound(&self, size: usize) -> Result<(), CollabError> {
        if size > self.options.input_byte_limit {
            return Err(CollabError::Limit {
                size,
                limit: self.options.input_byte_limit,
            });
        }
        Ok(())
    }

    /// 分配应用级块身份（`{client_id}-{seq}`——跨端唯一由 client_id
    /// 域保证；BlockId 不进 wire 语义面，仅属性承载）。
    pub(crate) fn next_block_id(&mut self) -> String {
        self.next_seq += 1;
        format!("{}-{}", self.options.client_id, self.next_seq)
    }

    pub(crate) fn anchors_mut(&mut self) -> &mut HashMap<String, CrdtAnchor> {
        &mut self.anchors
    }

    pub(crate) fn anchors(&self) -> &HashMap<String, CrdtAnchor> {
        &self.anchors
    }

    // ── 撤销面（B3.2 分栈；捕获窗 0）────────────────────────────

    /// 撤销指定 origin 面的最近栈项（栈本地——永不进 wire）。
    pub fn undo(&mut self, actor: Actor) -> bool {
        let r = match actor {
            Actor::Human => self.undo_human.undo_blocking(),
            Actor::Ai => self.undo_ai.undo_blocking(),
        };
        self.enforce_undo_limit(actor);
        r
    }

    /// 重做指定 origin 面。
    pub fn redo(&mut self, actor: Actor) -> bool {
        match actor {
            Actor::Human => self.undo_human.redo_blocking(),
            Actor::Ai => self.undo_ai.redo_blocking(),
        }
    }

    /// 撤销栈深度（观测面——内存边界监控）。
    pub fn undo_depth(&self, actor: Actor) -> usize {
        match actor {
            Actor::Human => self.undo_human.undo_stack().len(),
            Actor::Ai => self.undo_ai.undo_stack().len(),
        }
    }

    /// 重做栈深度。
    pub fn redo_depth(&self, actor: Actor) -> usize {
        match actor {
            Actor::Human => self.undo_human.redo_stack().len(),
            Actor::Ai => self.undo_ai.redo_stack().len(),
        }
    }

    /// B3.4 粗粒度内存边界：栈深超软上限 → clear_all（撤销历史全失
    /// 的代价在 SessionOptions 文档登记；不静默——CommandReceipt
    /// 的 undo_cleared 字段显式回执）。
    pub(crate) fn enforce_undo_limit(&mut self, actor: Actor) {
        self.undo_cleared_last = false;
        if let Some(limit) = self.options.undo_stack_limit {
            let over = match actor {
                Actor::Human => self.undo_human.undo_stack().len() > limit,
                Actor::Ai => self.undo_ai.undo_stack().len() > limit,
            };
            if over {
                match actor {
                    Actor::Human => self.undo_human.clear_all(),
                    Actor::Ai => self.undo_ai.clear_all(),
                }
                self.undo_cleared_last = true;
            }
        }
    }

    /// 取最近操作的清理标志（读后复位——回执单次有效）。
    pub(crate) fn take_undo_cleared(&mut self) -> bool {
        let v = self.undo_cleared_last;
        self.undo_cleared_last = false;
        v
    }

    // ── 同步面（B1.6 限额 + panic 隔离）──────────────────────────

    /// 本端状态向量（不透明 wire 字节——对端用于差量请求）。
    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// 差量编码（remote_sv = 对端 `state_vector()` 产物）。
    pub fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>, CollabError> {
        self.check_inbound(remote_sv.len())?;
        let sv =
            StateVector::decode_v1(remote_sv).map_err(|e| CollabError::Decode(e.to_string()))?;
        Ok(self.doc.transact().encode_diff_v1(&sv))
    }

    /// 入向 update（字节限额前置 + panic 隔离）。远端编辑不带本地
    /// origin → 不进本地撤销栈（只能撤销自己的编辑——协同撤销语义）。
    pub fn apply_update(&self, bytes: &[u8]) -> Result<(), CollabError> {
        self.check_inbound(bytes.len())?;
        let doc = &self.doc;
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let update =
                Update::decode_v1(bytes).map_err(|e| CollabError::Decode(e.to_string()))?;
            let mut txn = doc.transact_mut();
            txn.apply_update(update)
                .map_err(|e| CollabError::Decode(e.to_string()))?;
            txn.commit();
            Ok(())
        }));
        match r {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(p) => Err(CollabError::Panic(panic_message(&p))),
        }
    }

    /// 全量 update 长度（观测面——wire 治理/维护决策）。
    pub fn full_update_len(&self) -> usize {
        self.doc
            .transact()
            .encode_diff_v1(&StateVector::default())
            .len()
    }

    // ── 快照面（D1③ 权威快照——单点编码）────────────────────────

    /// 权威快照：当前状态的 update 编码（skip_gc 文档专属能力）。
    /// 收敛双端的本地全量编码字节可能不同（yrs-a 发现①）——权威端
    /// 单点编码是对外唯一权威产物；对端 `apply_update` 消费。
    pub fn authoritative_snapshot(&self) -> Result<Vec<u8>, CollabError> {
        let doc = &self.doc;
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let snap = doc.transact_mut().snapshot();
            let mut enc = EncoderV1::new();
            doc.transact_mut()
                .encode_state_from_snapshot(&snap, &mut enc)
                .map_err(|e| CollabError::InvalidInput(e.to_string()))?;
            Ok(enc.to_vec())
        }))
        .unwrap_or_else(|p| Err(CollabError::Panic(panic_message(&p))))
    }

    // ── 维护面（B1.3 受控压缩点）────────────────────────────────

    /// 受控压缩：清全部撤销栈（GC 安全前置——撤销栈是墓碑引用持有
    /// 方）后显式 `txn.gc(None)`。**压缩后撤销历史不可恢复**（代价
    /// 面登记于 B1.3）。返回压缩前后全量 update 字节数。
    pub fn maintenance_gc(&mut self) -> Result<(usize, usize), CollabError> {
        let before = self.full_update_len();
        self.undo_human.clear_all();
        self.undo_ai.clear_all();
        let doc = &self.doc;
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut txn = doc.transact_mut();
            txn.gc(None);
            txn.commit();
        }))
        .map_err(|p| CollabError::Panic(panic_message(&p)))?;
        let after = self.full_update_len();
        Ok((before, after))
    }

    // ── 读模型（快照式视图）──────────────────────────────────────

    /// 顶层块树（递归快照视图）。
    pub fn document(&self) -> Vec<BlockInfo> {
        let xml = self.doc.get_or_insert_xml_fragment(ROOT);
        let txn = self.doc.transact();
        xml.children(&txn)
            .filter_map(|c| match c {
                XmlOut::Element(e) => Some(read_block(&e, &txn)),
                _ => None,
            })
            .collect()
    }

    /// 按 block_id 查找块（全树属性扫描——远端插入的块无需侧车即可
    /// 寻址；O(n) 原型口径，索引化留工程增量）。
    pub fn find_block(&self, block_id: &str) -> Option<BlockInfo> {
        let xml = self.doc.get_or_insert_xml_fragment(ROOT);
        let txn = self.doc.transact();
        find_in_children(xml.children(&txn), &txn, block_id)
    }

    /// 顶层块 id 序列（保序）。
    pub fn top_level_ids(&self) -> Vec<String> {
        self.document().into_iter().map(|b| b.block_id).collect()
    }
}

/// panic payload 转字符串（不依赖 Debug 格式稳定性）。
pub(crate) fn panic_message(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

// ── 载体读取工具（session/ops/identity 共用）────────────────────

/// 属性值 Out → String（字符串属性面；非字符串值 = None）。
pub(crate) fn attr_str(v: &yrs::Out) -> Option<String> {
    match v {
        yrs::Out::Any(yrs::Any::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

/// 元素的用户属性集（block_id 载体键剔除 + 键名反转义）。
pub(crate) fn user_attrs(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for (k, v) in el.attributes(txn) {
        if k == ATTR_ID {
            continue;
        }
        if let Some(v) = attr_str(&v) {
            out.insert(unescape_key(k), v);
        }
    }
    out
}

pub(crate) fn block_id_of(el: &yrs::XmlElementRef, txn: &yrs::Transaction) -> Option<String> {
    for (k, v) in el.attributes(txn) {
        if k == ATTR_ID {
            return attr_str(&v);
        }
    }
    None
}

/// 递归读块视图。
pub(crate) fn read_block(el: &yrs::XmlElementRef, txn: &yrs::Transaction) -> BlockInfo {
    let kind = BlockKind::from_tag(el.tag().as_ref()).unwrap_or(BlockKind::Paragraph);
    BlockInfo {
        block_id: block_id_of(el, txn).unwrap_or_default(),
        kind,
        attrs: user_attrs(el, txn),
        text: own_text(el, txn),
        children: el
            .children(txn)
            .filter_map(|c| match c {
                XmlOut::Element(e) if e.tag().as_ref() != MARK_TAG => Some(read_block(&e, txn)),
                _ => None,
            })
            .collect(),
    }
}

fn find_in_children(
    children: impl Iterator<Item = XmlOut>,
    txn: &yrs::Transaction,
    block_id: &str,
) -> Option<BlockInfo> {
    for c in children {
        if let XmlOut::Element(e) = c {
            if e.tag().as_ref() == MARK_TAG {
                continue;
            }
            if block_id_of(&e, txn).as_deref() == Some(block_id) {
                return Some(read_block(&e, txn));
            }
            if let Some(found) = find_in_children(e.children(txn), txn, block_id) {
                return Some(found);
            }
        }
    }
    None
}

/// own-text 段逻辑文本：leading Text/m 子树内全部文本按序拼接
/// （容器块 = 自有文本段，不含子块；D-3 char 域口径）。
pub(crate) fn own_text(el: &yrs::XmlElementRef, txn: &yrs::Transaction) -> String {
    let mut out = String::new();
    for c in el.children(txn) {
        match c {
            XmlOut::Text(t) => out.push_str(&t.get_string(txn)),
            XmlOut::Element(e) if e.tag().as_ref() == MARK_TAG => {
                collect_text(&e, txn, &mut out);
            }
            _ => break, // 首个非 Text/mark 子节点 = own-text 段结束
        }
    }
    out
}

fn collect_text(el: &yrs::XmlElementRef, txn: &yrs::Transaction, out: &mut String) {
    for c in el.children(txn) {
        match c {
            XmlOut::Text(t) => out.push_str(&t.get_string(txn)),
            XmlOut::Element(e) => collect_text(&e, txn, out),
            _ => {}
        }
    }
}

// ── 属性键合法化（开放键面——yrs-e §2.1）────────────────────────

/// 非法 XML Name 字符 → `_xHHHH_` 转义；字面 `_xHHHH_` 形的下划线
/// 前瞻转义（防与转义产物撞车——解码侧只会还原真转义序列）。
pub(crate) fn escape_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let mut out = String::with_capacity(key.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if !is_name_char(ch) {
            out.push_str(&format!("_x{:04X}_", ch as u32));
            i += 1;
            continue;
        }
        // 字面 `_xHHHH_` 形 → 转义其下划线（歧义消解）。
        if ch == '_'
            && i + 6 < chars.len()
            && chars[i + 1] == 'x'
            && chars[i + 6] == '_'
            && chars[i + 2..i + 6].iter().all(|c| c.is_ascii_hexdigit())
        {
            out.push_str("_x005F_");
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

pub(crate) fn unescape_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let bytes: Vec<char> = key.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '_'
            && i + 6 < bytes.len()
            && bytes[i + 1] == 'x'
            && bytes[i + 6] == '_'
            && bytes[i + 2..i + 6].iter().all(|c| c.is_ascii_hexdigit())
        {
            let hex: String = bytes[i + 2..i + 6].iter().collect();
            if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                if let Some(ch) = char::from_u32(cp) {
                    out.push(ch);
                    i += 7;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || ch == '_'
        || ch == '-'
        || ch == '.'
        || !ch.is_ascii() && ch.is_alphabetic()
}

// ── 载体构造工具（ops 共用）────────────────────────────────────

/// 深拷贝元素 → prelim（D-1/MoveBlock 重建路径；属性与子树全保）。
pub(crate) fn deep_copy(el: &yrs::XmlElementRef, txn: &yrs::Transaction) -> XmlElementPrelim {
    let mut prelim = XmlElementPrelim::empty(el.tag().as_ref());
    for (k, v) in el.attributes(txn) {
        if let Some(v) = attr_str(&v) {
            prelim.attributes.insert(k.into(), v);
        }
    }
    for c in el.children(txn) {
        match c {
            XmlOut::Text(t) => {
                prelim
                    .children
                    .push(XmlIn::Text(XmlTextPrelim::new(t.get_string(txn)).into()));
            }
            XmlOut::Element(e) => {
                prelim.children.push(XmlIn::Element(deep_copy(&e, txn)));
            }
            XmlOut::Fragment(_) => {}
        }
    }
    prelim
}

/// char 域 → UTF-8 字节域（D-3 单点换算——公开面恒 char 域）。
pub(crate) fn c2b(s: &str, char_idx: u32) -> u32 {
    if char_idx == 0 {
        return 0;
    }
    for (chars, (bi, _)) in s.char_indices().enumerate() {
        if chars as u32 == char_idx {
            return bi as u32;
        }
    }
    // char_idx == char 数 → 尾字节位
    s.len() as u32
}

/// StickyIndex 构造辅助。
pub(crate) fn sticky_from_anchor(anchor: CrdtAnchor) -> StickyIndex {
    StickyIndex::from_id(
        yrs::ID::new(ClientID::new(anchor.client), anchor.clock),
        yrs::Assoc::After,
    )
}
