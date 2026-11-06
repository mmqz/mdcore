//! Chunk — 结构化切块（#29 R-1 · 2.1：检索基建的边界同源层）。
//!
//! # 定位与边界
//!
//! 父块 = 章节内连续段（block 聚合）；子块 = 句组（父块文本的连续切片）。
//! 消费方（标书侧 bid-engine / 检索面）拿 [`Chunk`] 自建索引——**BM25/
//! 倒排索引（#29 2.2）按 Lead 安排为「标书侧与引擎侧二选一」的另行
//! 决策，本轮不实现**，不在本模块留任何索引态。
//!
//! 块区间与块树同源：`block_range` 的两个端点是 **先序平铺索引**，与
//! [`crate::debug::blocks_json`] 的列表下标同构（JS 侧 BlockMap /
//! 证据锚定可直接互译）。切块只读树（不改 revision / 不入历史）。
//!
//! # 设计裁决（六点，测试钉定）
//!
//! 1. **父块切分**：标题（Heading）边界优先——遇标题先冲刷累计中的
//!    父块（归属**旧**路径），再更新标题栈；**标题净文本进父块 text
//!    开头**（章节首行）并更新 `heading_path`（选「入文 + 改路径」：
//!    检索命中标题文本应浮出章节首块；空章节仍留标题文本块，保住
//!    「每个非空块恰好进一个父块」的内容完备不变量）。同章节内累计
//!    块文本（char 计）加块即将超 `parent_max` → 先冲刷；恰达上限或
//!    单块超限 → 立即闭块（超限单块独占父块，允许超长——块级原子性
//!    优先于长度上限）。块间以空行（`\n\n`，计 2 char）拼接。
//! 2. **子块切分**：父块平铺净文本按句末符（`SENTENCE_ENDS`：ASCII
//!    `. ! ? ;` + CJK `。！？；` + 全角 `．`——字符级探测，不做缩写/
//!    小数防御，检索面句组容忍此噪声）聚合成 ≤ `child_max` 的句组；
//!    超长句/无句界长文按**字符数硬切**（CJK 按字）。子块
//!    `block_range`/`locator`/`heading_path` **继承父块**（粒度限制：
//!    子块是文本级窗口，块级定位只到父块粒度）。不变量：父块的全部
//!    子块按序拼接 == 父块文本（逐字节）。
//! 3. **表格/代码块**：不可句子切分的块整块进父块文本。表格 = GFM
//!    行投影：规范位（第二行）的分隔行剔除，其余行剥边缘 `|` 后按
//!    `|` 拆格、每格净文本（行内定界符剥除）、格间 ` | `、行间 `\n`；
//!    代码块 = **逐字保留**（净文本投影的显式例外——代码中的 `**`/
//!    `` ` `` 是字面内容，剥除即破坏语义）。
//! 4. **chunk_id**：`sha256(规范化序列化)` = 域字节（`P`/`C` 层级位）
//!    加 block_range（u64 LE ×2）、长度前缀 text（UTF-8）、长度前缀
//!    heading_path 逐段。长度前缀防连拼歧义 → 内容相同则 id 相同、
//!    内容变则 id 变。**parent_id 不入子块 id 哈希**（父块文本变化只
//!    改写被子块文本，其余子块 id 稳定——增量友好）；id 不含 BlockId
//!    （两份独立解析的同文文档产出逐字节相同的 chunk 列表）。
//! 5. **确定性**：全流程无 HashMap 迭代序（Vec / 栈序遍历）；同文档
//!    同 policy → `Vec<Chunk>` 序列化逐字节相同（跨实例亦然）。
//! 6. **policy 校验**：`parent_max == 0` / `child_max == 0` /
//!    `child_max > parent_max` → [`ChunkError`] typed reject（先于树
//!    遍历，零副作用）。
//!
//! # locator 选型
//!
//! 复用 mdcore 自有锚定 [`crate::anchor::Anchor`]（`(BlockId, offset)`，
//! ADR-B1：结构变化后仍可经注册表解析回树）——chunk 定位器 = 首贡献块
//! 的 `Anchor { block, offset: 0 }`（父块恒起于块边界，故 offset 恒 0；
//! 子块继承父块锚点）。**不采用** evidence crate 的 `Locator`：那是电子
//! 表格 CellCoord（A1 记法）向的分引擎定位器，与文档块锚定语义不匹配
//! （该 crate 文档自述「不依赖 mdcore，三域零依赖」——反向引用会破其
//! 纪律）。原则达成：chunk 可回溯到起始块（lookup_id → NodeId → path）。
//!
//! # fingerprint 口径
//!
//! [`Chunk::fingerprint`] = FNV-1a 64（hex 16 位）×
//! [`crate::fingerprint::normalize_text`]（空白折叠）× 逐码点 u32 LE +
//! 内容域 tag `0xC1`——**与 `fingerprint.rs` 的 `TreeFingerprint::
//! content_of` 同域同口径**（同文本必同值），判变更用（轻量、空白不敏
//! 感）；`chunk_id` 是全字段 sha256（严格内容寻址）——两者分工：id 定
//! 身份，fingerprint 供「位置还在内容已换」的二级判定。
//!
//! # 其余登记
//!
//! - 深度 > 3 的标题：`heading_path` 截到前三级（标题自身可能被截出
//!   路径——四级标题的路径 = 前三级祖先）。
//! - 空文本块（主题分隔线/空容器/空白 only 块）不入文、不扩区间；区间
//!   内被跳过的结构块由 `(start, end)` 闭区间隐式覆盖。
//! - 主题分隔线（`---`）**不**破章节（只有标题破）；空文本标题只更新
//!   路径不入文。
//! - 输出序：文档序——每个父块紧随其全部子块（父块 `parent_id=None`，
//!   子块 `parent_id=Some(父 chunk_id)`）；父块文本 ≤ `child_max` 时
//!   仍产出恰 1 个子块（层级均匀，索引侧可只吃叶）。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::anchor::Anchor;
use crate::fingerprint::normalize_text;
use crate::id::BlockId;
use crate::tree::{BlockKind, BlockTree, Node, NodeId};
use crate::Core;

/// 块间拼接符（父块 text 内的块边界）。
const BLOCK_SEP: &str = "\n\n";
/// `BLOCK_SEP` 的字符数（长度预算用）。
const SEP_CHARS: usize = 2;
/// 句末符集（裁决 2；字符级，中日英 + 全角变体）。
const SENTENCE_ENDS: [char; 9] = ['.', '!', '?', ';', '。', '！', '？', '；', '．'];
/// `heading_path` 最大深度（三级）。
const HEADING_PATH_MAX: usize = 3;
/// chunk_id 域字节：父块层级位。
const DOMAIN_PARENT: u8 = b'P';
/// chunk_id 域字节：子块层级位。
const DOMAIN_CHILD: u8 = b'C';
/// FNV-1a 64 常数（与 fingerprint.rs 同源；内容域 tag 同 `0xC1`）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const FNV_CONTENT_TAG: u8 = 0xC1;

// ---------------------------------------------------------------------------
// 公共类型
// ---------------------------------------------------------------------------

/// 结构化切块结果。父块 = 章节内连续段（`parent_id == None`）；子块 =
/// 句组（`parent_id` 指向父块 `chunk_id`）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    /// 内容寻址 sha256（hex 64 位；规范化序列化见模块文档裁决 4）。
    pub chunk_id: String,
    /// 父块 id（子块才有；父块为 None）。
    pub parent_id: Option<String>,
    /// 覆盖的先序块区间（闭区间；与 `debug::blocks_json` 下标同构）。
    pub block_range: (usize, usize),
    /// 净文本（行内定界符剥除；代码块逐字例外——裁决 3）。
    pub text: String,
    /// 起始块锚点（首贡献块，offset 0；子块继承父块锚点）。
    pub locator: Anchor,
    /// 内容指纹（FNV-1a 64 × 空白折叠口径，判变更用；见模块文档）。
    pub fingerprint: String,
    /// 所属章节路径（≤ 三级；沿标题祖先链，深度截断）。
    pub heading_path: Vec<String>,
}

/// 切块策略（#29 建议区间：父块 800-1500 字 / 子块 200-300 字——默认值
/// 取区间中段；单位 = 字符（Unicode 码点，CJK 按字））。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkPolicy {
    /// 父块文本上限（char）。单块超限时独占父块（允许超长）。
    pub parent_max: usize,
    /// 子块文本上限（char）。超长句/无句界文本按字符硬切。
    pub child_max: usize,
}

impl Default for ChunkPolicy {
    /// 默认：parent_max = 1200（800-1500 中段）/ child_max = 280
    /// （200-300 内）。
    fn default() -> Self {
        Self {
            parent_max: 1200,
            child_max: 280,
        }
    }
}

impl ChunkPolicy {
    /// 策略校验（裁决 6）：零值 / 子超父 → typed reject。
    pub fn validate(&self) -> Result<(), ChunkError> {
        if self.parent_max == 0 {
            return Err(ChunkError::ZeroParentMax);
        }
        if self.child_max == 0 {
            return Err(ChunkError::ZeroChildMax);
        }
        if self.child_max > self.parent_max {
            return Err(ChunkError::ChildExceedsParent {
                parent_max: self.parent_max,
                child_max: self.child_max,
            });
        }
        Ok(())
    }
}

/// 切块错误（typed reject；先于树遍历，零副作用）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ChunkError {
    /// `parent_max == 0`。
    ZeroParentMax,
    /// `child_max == 0`。
    ZeroChildMax,
    /// `child_max > parent_max`（子块是父块内句组，超父无意义）。
    ChildExceedsParent { parent_max: usize, child_max: usize },
}

// ---------------------------------------------------------------------------
// Core 入口
// ---------------------------------------------------------------------------

impl Core {
    /// 结构化切块（只读检视：不碰 revision / 历史 / 事件）。
    ///
    /// 同文档同 policy → `Vec<Chunk>` 序列化逐字节相同（确定性契约，
    /// 测试钉定）；policy 非法 → [`ChunkError`]（树零遍历）。
    pub fn chunk_document(&self, policy: &ChunkPolicy) -> Result<Vec<Chunk>, ChunkError> {
        policy.validate()?;
        Ok(chunk_tree(&self.tree, policy))
    }
}

// ---------------------------------------------------------------------------
// 遍历与聚合
// ---------------------------------------------------------------------------

/// 一个块的文本贡献（净投影后非空才产生）。
struct Contribution {
    /// 先序平铺索引（与 `debug::blocks_json` 下标同构）。
    preorder: usize,
    block_id: BlockId,
    text: String,
}

/// 平铺条目：`level = Some` 为标题（章节边界 + 路径更新），否则为正文块。
struct Entry {
    level: Option<usize>,
    contribution: Contribution,
}

/// 先序遍历收集条目（跳过空文本贡献；计数器照常推进——索引与
/// `debug::blocks_json` 的先序平铺严格同构，含被跳过的结构块）。
fn walk_entries(tree: &BlockTree, id: NodeId, counter: &mut usize, out: &mut Vec<Entry>) {
    let preorder = *counter;
    *counter += 1;
    let n: &Node = tree.node(id);
    match n.kind {
        BlockKind::Heading => {
            let level = n
                .attrs
                .get("level")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1);
            let text = crate::inline::parse_inline_marks(&n.text.to_string()).0;
            out.push(Entry {
                level: Some(level),
                contribution: Contribution {
                    preorder,
                    block_id: n.id,
                    text,
                },
            });
        }
        BlockKind::Document => {} // 不可达（walk 自 top_level 起）
        _ => {
            let text = project_block(n.kind, &n.text.to_string());
            if !text.trim().is_empty() {
                out.push(Entry {
                    level: None,
                    contribution: Contribution {
                        preorder,
                        block_id: n.id,
                        text,
                    },
                });
            }
        }
    }
    for &c in &n.children {
        walk_entries(tree, c, counter, out);
    }
}

/// 块型 → 净文本投影（裁决 3）。
fn project_block(kind: BlockKind, text: &str) -> String {
    match kind {
        // 代码逐字：定界符是字面内容（净文本投影的显式例外）。
        BlockKind::CodeBlock => text.to_string(),
        // 数学块同构：LaTeX 源逐字（#261——$$ 定界是字面内容）。
        BlockKind::Math => text.to_string(),
        BlockKind::Table => project_table(text),
        _ => crate::inline::parse_inline_marks(text).0,
    }
}

/// GFM 表格投影：规范位（第二行）分隔行剔除；其余行剥边缘 `|`、按 `|`
/// 拆格、每格净文本 + 首尾 trim、格间 ` | `、行间 `\n`；全空行丢弃。
fn project_table(text: &str) -> String {
    let mut rows: Vec<String> = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if i == 1 && is_delimiter_row(line) {
            continue;
        }
        let inner = line.trim();
        let inner = inner.strip_prefix('|').unwrap_or(inner);
        let inner = inner.strip_suffix('|').unwrap_or(inner);
        let cells: Vec<String> = inner
            .split('|')
            .map(|cell| crate::inline::parse_inline_marks(cell).0.trim().to_string())
            .collect();
        let row = cells.join(" | ");
        if !row.trim().is_empty() {
            rows.push(row);
        }
    }
    rows.join("\n")
}

/// GFM 分隔行形态：剥边缘 `|` 后每格 = 可选首/尾冒号 + `-` 游程（≥1）。
fn is_delimiter_row(line: &str) -> bool {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').all(|cell| {
        let c = cell.trim();
        let c = c.strip_prefix(':').unwrap_or(c);
        let c = c.strip_suffix(':').unwrap_or(c);
        !c.is_empty() && c.chars().all(|ch| ch == '-')
    })
}

/// 主流程：条目流 → 章节循环（标题破段 + 长度预算）→ 父块 + 子块。
fn chunk_tree(tree: &BlockTree, policy: &ChunkPolicy) -> Vec<Chunk> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut counter = 0usize;
    for &top in tree.top_level() {
        walk_entries(tree, top, &mut counter, &mut entries);
    }

    let mut out: Vec<Chunk> = Vec::new();
    let mut heading_stack: Vec<String> = Vec::new();
    let mut acc: Vec<&Contribution> = Vec::new();
    let mut acc_chars = 0usize;

    for entry in &entries {
        if let Some(level) = entry.level {
            // 章节边界：先冲刷（累计内容归属旧路径），再改路径。
            flush_parent(&mut acc, &mut acc_chars, &heading_stack, policy, &mut out);
            heading_stack.truncate(level - 1);
            heading_stack.push(entry.contribution.text.clone());
        }
        push_acc(
            &mut acc,
            &mut acc_chars,
            &entry.contribution,
            policy,
            &heading_stack,
            &mut out,
        );
    }
    flush_parent(&mut acc, &mut acc_chars, &heading_stack, policy, &mut out);
    out
}

/// 累计一块文本（长度预算 + 恰达/超限闭块；裁决 1）。
fn push_acc<'a>(
    acc: &mut Vec<&'a Contribution>,
    acc_chars: &mut usize,
    c: &'a Contribution,
    policy: &ChunkPolicy,
    heading_stack: &[String],
    out: &mut Vec<Chunk>,
) {
    if c.text.trim().is_empty() {
        return; // 空文本标题只改路径不入文；空白 only 块不入文。
    }
    let n = c.text.chars().count();
    if !acc.is_empty() && *acc_chars + SEP_CHARS + n > policy.parent_max {
        flush_parent(acc, acc_chars, heading_stack, policy, out);
    }
    if acc.is_empty() {
        *acc_chars = n;
    } else {
        *acc_chars += SEP_CHARS + n;
    }
    acc.push(c);
    if *acc_chars >= policy.parent_max {
        // 恰达上限闭块；单块超限 → 已自成独占父块（acc 仅一块）。
        flush_parent(acc, acc_chars, heading_stack, policy, out);
    }
}

/// 冲刷累计为父块 + 派生子块（acc 空 = no-op）。
fn flush_parent(
    acc: &mut Vec<&Contribution>,
    acc_chars: &mut usize,
    heading_stack: &[String],
    policy: &ChunkPolicy,
    out: &mut Vec<Chunk>,
) {
    if acc.is_empty() {
        return;
    }
    let parent = build_parent(acc, heading_stack);
    acc.clear();
    *acc_chars = 0;
    let children = build_children(&parent, policy.child_max);
    out.push(parent);
    out.extend(children);
}

/// 父块构造（block_range = 首末贡献块闭区间；locator = 首贡献块锚点）。
fn build_parent(parts: &[&Contribution], heading_stack: &[String]) -> Chunk {
    let text = parts
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<&str>>()
        .join(BLOCK_SEP);
    let first = parts[0];
    let last = parts[parts.len() - 1];
    let range = (first.preorder, last.preorder);
    let path: Vec<String> = heading_stack
        .iter()
        .take(HEADING_PATH_MAX)
        .cloned()
        .collect();
    let fingerprint = content_fingerprint(&text);
    Chunk {
        chunk_id: compute_chunk_id(DOMAIN_PARENT, range, &text, &path),
        parent_id: None,
        block_range: range,
        text,
        locator: Anchor {
            block: first.block_id,
            offset: 0,
        },
        fingerprint,
        heading_path: path,
    }
}

/// 子块构造（句组 = 父块文本的连续切片；字段全继承父块——粒度限制见
/// 模块文档裁决 2）。
fn build_children(parent: &Chunk, child_max: usize) -> Vec<Chunk> {
    let units = split_units(&parent.text, child_max);
    let mut out: Vec<Chunk> = Vec::new();
    let mut start = 0usize; // 当前组起始字节
    let mut end = 0usize; // 当前组止字节（开区间端点）
    let mut chars = 0usize; // 当前组字符数
    let mut open = false;
    for (s, e) in units {
        let n = parent.text[s..e].chars().count();
        if open && chars + n > child_max {
            out.push(build_child(parent, start, end));
            open = false;
        }
        if !open {
            start = s;
            chars = 0;
            open = true;
        }
        end = e;
        chars += n;
    }
    if open {
        out.push(build_child(parent, start, end));
    }
    out
}

/// 单个子块（`parent.text[byte_start..byte_end]` 切片）。
fn build_child(parent: &Chunk, byte_start: usize, byte_end: usize) -> Chunk {
    let text = parent.text[byte_start..byte_end].to_string();
    let fingerprint = content_fingerprint(&text);
    Chunk {
        chunk_id: compute_chunk_id(
            DOMAIN_CHILD,
            parent.block_range,
            &text,
            &parent.heading_path,
        ),
        parent_id: Some(parent.chunk_id.clone()),
        block_range: parent.block_range,
        text,
        locator: parent.locator,
        fingerprint,
        heading_path: parent.heading_path.clone(),
    }
}

/// 文本 → 单元序列（句；超长句硬切），返回字节区间（开区间端点）。
/// 句末符收尾归句（含句末符）；无句界文本整段为一单元再硬切。
fn split_units(text: &str, child_max: usize) -> Vec<(usize, usize)> {
    let mut sentences: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        if SENTENCE_ENDS.contains(&c) {
            let end = i + c.len_utf8();
            sentences.push((start, end));
            start = end;
        }
    }
    if start < text.len() {
        sentences.push((start, text.len()));
    }
    // 超长句/无句界长文：按字符数硬切（child_max 字一片，尾片为余量）。
    let mut units: Vec<(usize, usize)> = Vec::new();
    for (s, e) in sentences {
        let mut piece_start = s;
        let mut counted = 0usize;
        for (i, c) in text[s..e].char_indices() {
            counted += 1;
            if counted == child_max {
                let piece_end = s + i + c.len_utf8();
                units.push((piece_start, piece_end));
                piece_start = piece_end;
                counted = 0;
            }
        }
        if piece_start < e {
            units.push((piece_start, e));
        }
    }
    units
}

// ---------------------------------------------------------------------------
// id / 指纹
// ---------------------------------------------------------------------------

/// 内容寻址 id：sha256(域字节 + 区间 + 长度前缀 text + 长度前缀 path)。
/// hex 64 字符。长度前缀防连拼歧义；域字节 = 父/子层级位。
fn compute_chunk_id(
    domain: u8,
    block_range: (usize, usize),
    text: &str,
    heading_path: &[String],
) -> String {
    let mut h = Sha256::new();
    h.update([domain]);
    h.update((block_range.0 as u64).to_le_bytes());
    h.update((block_range.1 as u64).to_le_bytes());
    h.update((text.len() as u64).to_le_bytes());
    h.update(text.as_bytes());
    h.update((heading_path.len() as u64).to_le_bytes());
    for seg in heading_path {
        h.update((seg.len() as u64).to_le_bytes());
        h.update(seg.as_bytes());
    }
    hex_encode(&h.finalize())
}

/// 内容指纹：FNV-1a 64（hex 16 位）× normalize_text × 逐码点 u32 LE +
/// 内容域 tag——与 `fingerprint.rs::TreeFingerprint::content_of` 同域
/// 同口径（同文本必同值）。
fn content_fingerprint(text: &str) -> String {
    let mut h = FNV_OFFSET;
    fnv_step(&mut h, &[FNV_CONTENT_TAG]);
    let chars: Vec<char> = normalize_text(text).chars().collect();
    fnv_step(&mut h, &(chars.len() as u64).to_le_bytes());
    for &c in &chars {
        fnv_step(&mut h, &(c as u32).to_le_bytes());
    }
    format!("{h:016x}")
}

fn fnv_step(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

// ---------------------------------------------------------------------------
// 测试（边界矩阵 + 确定性验收 + 内容寻址性质 + 路径正确性）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 混合块型文档：三级标题 + 段落 + 表格 + 嵌套列表 + 长段落。
    fn mixed_doc() -> String {
        [
            "# 一级标题甲",
            "",
            "甲章节导言段落，含 **加粗** 与 `code` 行内标记。",
            "",
            "## 二级标题乙",
            "",
            "| 名称 | **金额** | 备注 |",
            "| --- | --- | --- |",
            "| 甲项 | 100 | 优质 |",
            "| 乙项 | 200 | 合格 |",
            "",
            "- 列表项一",
            "  - 嵌套项 A",
            "  - 嵌套项 B",
            "- 列表项二",
            "",
            "### 三级标题丙",
            "",
            "丙章节长段落。第二句在此。第三句收尾。",
            "",
        ]
        .join("\n")
    }

    fn parents(chunks: &[Chunk]) -> Vec<&Chunk> {
        chunks.iter().filter(|c| c.parent_id.is_none()).collect()
    }

    fn children(chunks: &[Chunk]) -> Vec<&Chunk> {
        chunks.iter().filter(|c| c.parent_id.is_some()).collect()
    }

    // ---- 边界矩阵 ----

    #[test]
    fn empty_document_no_chunks() {
        let c = Core::new();
        assert_eq!(c.chunk_document(&ChunkPolicy::default()).unwrap(), vec![]);
        // 仅主题分隔线：无文本贡献 → 空。
        let c2 = Core::from_markdown("---");
        assert_eq!(c2.chunk_document(&ChunkPolicy::default()).unwrap(), vec![]);
    }

    #[test]
    fn plain_text_no_headings_single_section() {
        let c = Core::from_markdown("第一段短文。\n\n第二段短文。\n\n第三段短文。");
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 1, "无标题平文 = 单章节单父块");
        assert_eq!(ps[0].heading_path, Vec::<String>::new());
        assert_eq!(ps[0].text, "第一段短文。\n\n第二段短文。\n\n第三段短文。");
        assert_eq!(ps[0].block_range, (0, 2), "三个段落块的先序闭区间");
    }

    #[test]
    fn plain_text_length_split_multi_parents() {
        // 4 段 × 6 字 = 27 char（含分隔）> parent_max=20 → 长度切分。
        let c = Core::from_markdown("一一一一一。\n\n二二二二二。\n\n三三三三三。\n\n四四四四四。");
        let p = ChunkPolicy {
            parent_max: 20,
            child_max: 10,
        };
        let out = c.chunk_document(&p).unwrap();
        let ps = parents(&out);
        assert!(ps.len() >= 2, "超预算必切多父块，实际 {}", ps.len());
        // 多块父块不得超 parent_max（单块超限独占除外）。
        for ch in &ps {
            assert!(
                ch.text.chars().count() <= p.parent_max || ch.block_range.0 == ch.block_range.1
            );
        }
    }

    #[test]
    fn heading_enters_text_and_updates_path() {
        let c = Core::from_markdown("# 标题甲\n\n正文段落。");
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 1);
        // 裁决 1：标题净文本进父块开头。
        assert!(ps[0].text.starts_with("标题甲"));
        assert!(ps[0].text.contains("正文段落。"));
        assert_eq!(ps[0].heading_path, vec!["标题甲".to_string()]);
        assert_eq!(ps[0].block_range, (0, 1));
    }

    #[test]
    fn section_boundary_flushes_with_old_path() {
        let c = Core::from_markdown("# 甲\n\n甲正文。\n\n# 乙\n\n乙正文。");
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].heading_path, vec!["甲".to_string()]);
        assert_eq!(ps[0].text, "甲\n\n甲正文。");
        assert_eq!(ps[1].heading_path, vec!["乙".to_string()]);
        assert_eq!(ps[1].text, "乙\n\n乙正文。");
    }

    #[test]
    fn exact_parent_limit_closes_chunk() {
        // 恰达上限（10 char）→ 立即闭块；后随块另起父块。
        let c = Core::from_markdown("1234567890\n\nabcde");
        let p = ChunkPolicy {
            parent_max: 10,
            child_max: 5,
        };
        let out = c.chunk_document(&p).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 2, "恰达上限闭块 + 尾块，实际 {}", ps.len());
        assert_eq!(ps[0].text, "1234567890");
        assert_eq!(ps[0].block_range, (0, 0));
        assert_eq!(ps[1].text, "abcde");
        assert_eq!(ps[1].block_range, (1, 1));
    }

    #[test]
    fn oversized_single_block_own_parent() {
        // 单块 60 字 > parent_max=50 → 独占父块；子块按 child_max 硬切。
        let long = "字".repeat(60);
        let md = format!("{long}\n\n尾巴段。");
        let c = Core::from_markdown(&md);
        let p = ChunkPolicy {
            parent_max: 50,
            child_max: 20,
        };
        let out = c.chunk_document(&p).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].text.chars().count(), 60, "超限单块独占且不截断");
        assert_eq!(ps[0].block_range, (0, 0));
        assert_eq!(ps[1].text, "尾巴段。");
        // 独占父块的子块 = 硬切窗口（无句界 → 20 字/片）。
        let kids: Vec<&Chunk> = children(&out)
            .into_iter()
            .filter(|k| k.parent_id == Some(ps[0].chunk_id.clone()))
            .collect();
        assert_eq!(kids.len(), 3, "60 字 / 20 字 = 3 片");
        assert!(kids.iter().all(|k| k.text.chars().count() <= 20));
        let joined: String = kids.iter().map(|k| k.text.as_str()).collect();
        assert_eq!(joined, long, "子块拼接 == 父块文本（逐字节）");
    }

    #[test]
    fn no_sentence_boundary_hard_split() {
        // 12 个无句界 CJK 字 + child_max=5 → 5/5/2 硬切。
        let c = Core::from_markdown("一二三四五六七八九十甲乙");
        let p = ChunkPolicy {
            parent_max: 50,
            child_max: 5,
        };
        let out = c.chunk_document(&p).unwrap();
        let kids = children(&out);
        assert_eq!(kids.len(), 3);
        assert_eq!(kids[0].text, "一二三四五");
        assert_eq!(kids[1].text, "六七八九十");
        assert_eq!(kids[2].text, "甲乙");
    }

    #[test]
    fn cjk_and_ascii_sentence_boundaries() {
        let c = Core::from_markdown("一二三。四五六七！八九；十");
        let p = ChunkPolicy {
            parent_max: 50,
            child_max: 10,
        };
        let out = c.chunk_document(&p).unwrap();
        let kids = children(&out);
        // 句单元：4/5/3/1 → 贪心组：[4+5=9]、[3+1=4]。
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].text, "一二三。四五六七！");
        assert_eq!(kids[1].text, "八九；十");
        // ASCII 句末符同口径（. 与 ! 均为句界）。
        let c2 = Core::from_markdown("AAAA. BBBB!");
        let out2 = c2.chunk_document(&p).unwrap();
        let kids2 = children(&out2);
        // 句单元："AAAA."(5) + " BBBB!"(6) = 11 > child_max=10 → 两组。
        assert_eq!(kids2.len(), 2);
        assert_eq!(kids2[0].text, "AAAA.");
        assert_eq!(kids2[1].text, " BBBB!");
    }

    #[test]
    fn nested_list_and_quote_traversal() {
        let c = Core::from_markdown("- 项甲\n  - 嵌甲\n  - 嵌乙\n- 项乙\n\n> 引用一\n> 引用二");
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 1, "无标题 → 单章节");
        // 先序：项甲(0) 嵌甲(1) 嵌乙(2) 项乙(3) quote容器(4) 引用一(5) 引用二(6)
        assert_eq!(ps[0].block_range, (0, 6));
        assert!(ps[0].text.contains("项甲"));
        assert!(ps[0].text.contains("嵌甲"));
        assert!(ps[0].text.contains("引用一"));
        // 容器块（quote，preorder 4）被区间隐式覆盖。
        assert!(ps[0].text.contains("项乙\n\n引用一"), "块间空行拼接");
    }

    #[test]
    fn thematic_break_skipped_but_spanned() {
        let c = Core::from_markdown("前段。\n\n---\n\n后段。");
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 1, "主题分隔线不破章节（裁决：只标题破）");
        assert_eq!(ps[0].block_range, (0, 2), "分隔线(1)被闭区间隐式覆盖");
        assert_eq!(ps[0].text, "前段。\n\n后段。", "分隔线无文本贡献");
    }

    #[test]
    fn heading_path_three_levels_and_deep_truncation() {
        let c = Core::from_markdown(
            "# 一级\n\n一级正文。\n\n## 二级\n\n二级正文。\n\n### 三级\n\n三级正文。\n\n#### 四级\n\n四级正文。",
        );
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 4);
        assert_eq!(ps[0].heading_path, vec!["一级"]);
        assert_eq!(ps[1].heading_path, vec!["一级", "二级"]);
        assert_eq!(ps[2].heading_path, vec!["一级", "二级", "三级"]);
        // 深度 > 3：路径截到前三级（四级标题自身被截出路径）。
        assert_eq!(ps[3].heading_path, vec!["一级", "二级", "三级"]);
        assert_eq!(ps[3].text, "四级\n\n四级正文。");
    }

    #[test]
    fn heading_path_ancestor_chain_correctness() {
        // 沿祖先链：h2 内容的路径含最近 h1；h3 跳级（无 h2）也沿最近祖先。
        let c = Core::from_markdown("# 甲\n\n## 乙\n\n乙正文。\n\n# 丙\n\n### 丁\n\n丁正文。");
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 4); // 甲(空节) 乙节 丙(空节) 丁节
        assert_eq!(ps[1].heading_path, vec!["甲", "乙"]);
        assert_eq!(
            ps[3].heading_path,
            vec!["丙", "丁"],
            "跳级标题路径 = 最近祖先链"
        );
    }

    // ---- 表格 / 代码块投影（裁决 3）----

    #[test]
    fn table_projection_net_cells_delimiter_dropped() {
        let c = Core::from_markdown(
            "| 名称 | **金额** | 备注 |\n| --- | --- | --- |\n| 甲项 | 100 | 优质 |",
        );
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "名称 | 金额 | 备注\n甲项 | 100 | 优质");
        assert!(!ps[0].text.contains("**"), "行内定界符剥除");
        assert!(!ps[0].text.contains("---"), "GFM 分隔行剔除");
    }

    #[test]
    fn code_block_verbatim_no_inline_strip() {
        let c = Core::from_markdown("```rust\nlet s = \"**not bold**\";\n```");
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert_eq!(ps.len(), 1);
        assert_eq!(
            ps[0].text, "let s = \"**not bold**\";",
            "代码逐字（净投影例外）"
        );
    }

    // ---- policy 校验（裁决 6）----

    #[test]
    fn policy_validation_rejects() {
        let c = Core::new();
        let bad = [
            ChunkPolicy {
                parent_max: 0,
                child_max: 10,
            },
            ChunkPolicy {
                parent_max: 10,
                child_max: 0,
            },
            ChunkPolicy {
                parent_max: 10,
                child_max: 11,
            },
        ];
        assert_eq!(c.chunk_document(&bad[0]), Err(ChunkError::ZeroParentMax));
        assert_eq!(c.chunk_document(&bad[1]), Err(ChunkError::ZeroChildMax));
        assert_eq!(
            c.chunk_document(&bad[2]),
            Err(ChunkError::ChildExceedsParent {
                parent_max: 10,
                child_max: 11
            })
        );
        // 边界合法：child == parent。
        assert!(c
            .chunk_document(&ChunkPolicy {
                parent_max: 10,
                child_max: 10
            })
            .is_ok());
    }

    // ---- 确定性验收（#29 R-1 验收条款）----

    #[test]
    fn determinism_same_document_two_passes_byte_equal() {
        let c = Core::from_markdown(&mixed_doc());
        let policy = ChunkPolicy::default();
        let a = serde_json::to_string(&c.chunk_document(&policy).unwrap()).unwrap();
        let b = serde_json::to_string(&c.chunk_document(&policy).unwrap()).unwrap();
        assert_eq!(a, b, "同一文档切两次：序列化逐字节相同");
        // 跨实例：独立解析的同文文档（id 不含 BlockId）→ 逐字节一致。
        let c2 = Core::from_markdown(&mixed_doc());
        let d = serde_json::to_string(&c2.chunk_document(&policy).unwrap()).unwrap();
        assert_eq!(a, d, "两份独立解析实例：chunk 列表逐字节相同");
    }

    #[test]
    fn determinism_custom_policy_and_plain_doc() {
        let c = Core::from_markdown("平文一段。第二句。\n\n# 标题\n\n内容。");
        let policy = ChunkPolicy {
            parent_max: 30,
            child_max: 8,
        };
        let a = serde_json::to_string(&c.chunk_document(&policy).unwrap()).unwrap();
        let b = serde_json::to_string(&c.chunk_document(&policy).unwrap()).unwrap();
        assert_eq!(a, b);
        // policy 变化 → 切分变化（参数可配实的生效）。
        let other = c
            .chunk_document(&ChunkPolicy {
                parent_max: 10,
                child_max: 5,
            })
            .unwrap();
        assert_ne!(a, serde_json::to_string(&other).unwrap());
    }

    // ---- chunk_id 内容寻址性质（增量友好）----

    #[test]
    fn content_addressed_id_change_localized() {
        let doc = "# 标题甲\n\n第一段第一句。第一段第二句。第一段第三句。\n\n# 标题乙\n\n第二段第一句。第二段第二句。";
        let c1 = Core::from_markdown(doc);
        // 同字数换字（二 → 九）：不移动句界/组界 → 恰一个子块受影响。
        let doc2 = doc.replace("第一段第二句", "第一段第九句");
        let c2 = Core::from_markdown(&doc2);
        let p = ChunkPolicy {
            parent_max: 500,
            child_max: 20,
        };
        let o1 = c1.chunk_document(&p).unwrap();
        let o2 = c2.chunk_document(&p).unwrap();
        assert_eq!(o1.len(), o2.len());
        let changed: Vec<usize> = (0..o1.len())
            .filter(|&i| o1[i].chunk_id != o2[i].chunk_id)
            .collect();
        assert_eq!(
            changed.len(),
            2,
            "恰父块 + 命中子块变，其余 id 稳定：{changed:?}"
        );
        let (i, j) = (changed[0], changed[1]);
        assert!(o1[i].parent_id.is_none(), "第一个变更的是父块");
        assert_eq!(
            o1[j].parent_id.as_deref(),
            Some(o1[i].chunk_id.as_str()),
            "第二个是其子块"
        );
        assert_ne!(o1[i].fingerprint, o2[i].fingerprint, "命中块指纹随内容变");
        // 未命中块：自身内容字段全稳定；parent_id 是**引用**不是内容——
        // 同一变更父块下的未变子块，其 parent_id 随父块新 id 改写（id
        // 本身不变，增量友好：索引主键稳定，仅需回填父引用）。
        for (k, (x, y)) in o1.iter().zip(o2.iter()).enumerate() {
            if !changed.contains(&k) {
                assert_eq!(x.chunk_id, y.chunk_id, "未命中块 #{k} id 稳定");
                assert_eq!(x.text, y.text, "未命中块 #{k} 文本稳定");
                assert_eq!(x.fingerprint, y.fingerprint, "未命中块 #{k} 指纹稳定");
                assert_eq!(x.block_range, y.block_range);
                assert_eq!(x.locator, y.locator);
                assert_eq!(x.heading_path, y.heading_path);
            }
        }
    }

    #[test]
    fn parent_child_id_domain_separation() {
        // 父块文本 ≤ child_max → 单子块与父块同文：域字节保证 id 不同。
        let c = Core::from_markdown("短句。");
        let out = c
            .chunk_document(&ChunkPolicy {
                parent_max: 100,
                child_max: 100,
            })
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, out[1].text);
        assert_ne!(out[0].chunk_id, out[1].chunk_id);
        assert_eq!(out[1].parent_id.as_deref(), Some(out[0].chunk_id.as_str()));
    }

    // ---- 子块继承面 + 与块树同源 ----

    #[test]
    fn children_inherit_parent_fields_and_concat() {
        let c = Core::from_markdown(&mixed_doc());
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let ps = parents(&out);
        assert!(!ps.is_empty());
        for p in &ps {
            let kids: Vec<&Chunk> = children(&out)
                .into_iter()
                .filter(|k| k.parent_id.as_deref() == Some(p.chunk_id.as_str()))
                .collect();
            assert!(!kids.is_empty(), "父块恒有 ≥1 子块");
            let joined: String = kids.iter().map(|k| k.text.as_str()).collect();
            assert_eq!(joined, p.text, "子块拼接 == 父块文本（逐字节）");
            for k in &kids {
                assert_eq!(k.block_range, p.block_range, "子块继承 block_range");
                assert_eq!(k.locator, p.locator, "子块继承 locator");
                assert_eq!(k.heading_path, p.heading_path, "子块继承 heading_path");
                assert!(k.text.chars().count() <= 280, "子块 ≤ child_max");
            }
        }
        // 输出序：父块紧随其全部子块，无孤儿。
        let mut i = 0usize;
        while i < out.len() {
            assert!(out[i].parent_id.is_none(), "孤儿子块：#{i} 无前置父块");
            let pid = out[i].chunk_id.clone();
            let mut j = i + 1;
            while j < out.len() && out[j].parent_id.is_some() {
                assert_eq!(
                    out[j].parent_id.as_deref(),
                    Some(pid.as_str()),
                    "子块紧随其父块"
                );
                j += 1;
            }
            i = j;
        }
    }

    #[test]
    fn block_range_matches_blocks_json_preorder() {
        // 「切块边界与块树同源」验收：block_range 下标 == debug::blocks_json
        // 平铺下标；locator.block == 该下标块的 id。
        let c = Core::from_markdown(&mixed_doc());
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&crate::debug::blocks_json(&c.tree)).unwrap();
        let list = json.as_array().unwrap();
        for p in parents(&out) {
            let (s, e) = p.block_range;
            assert!(s <= e && e < list.len(), "区间在平铺范围内");
            let id_at_start = list[s]["id"].as_u64().unwrap();
            assert_eq!(
                id_at_start, p.locator.block.0,
                "locator 锚定 = 区间首块的 BlockId"
            );
        }
        // 全覆盖：所有贡献块都在某个父块区间内（首块从 0 起）。
        assert_eq!(parents(&out)[0].block_range.0, 0);
    }

    // ---- 指纹口径复用（与 fingerprint.rs 同域）----

    #[test]
    fn fingerprint_matches_fingerprint_content_domain() {
        // 同文本 → 与 TreeFingerprint::content_of（单块树）同值。
        use crate::fingerprint::TreeFingerprint;
        let text = "指纹口径 互证 文本。";
        let c = Core::from_markdown(text);
        let out = c.chunk_document(&ChunkPolicy::default()).unwrap();
        let mut tf = TreeFingerprint::new(&c.tree);
        let node = c.tree.root();
        let first_child = c.tree.node(node).children[0];
        let expect = format!("{:016x}", tf.content_of(first_child));
        assert_eq!(out[0].text, text, "单段文档：父块文本 = 原文");
        assert_eq!(out[0].fingerprint, expect, "FNV 内容域口径互证");
        // 空白折叠：尾随空白不改变指纹。
        let c2 = Core::from_markdown("指纹口径 互证 文本。  ");
        let out2 = c2.chunk_document(&ChunkPolicy::default()).unwrap();
        assert_eq!(out2[0].fingerprint, out[0].fingerprint);
    }
}
