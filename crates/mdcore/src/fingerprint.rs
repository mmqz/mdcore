//! 块指纹 + 结构匹配（P1-7 定谳，ADR-B6）——外部文件重载的身份保持基础。
//!
//! 职责边界：
//! - **指纹**是匹配提示，不是身份（ADR-B1 的边界不破：BlockId 仍是唯一身份，
//!   内容哈希绝不充当 ID）。指纹分两层：
//!   - 结构层 = 块型 + 属性（BTreeMap 序）+ 子数 + 子结构指纹（递归）；
//!   - 内容层 = 空白折叠规范化文本的逐码点哈希（码点语义，非 BMP 安全）。
//! - **匹配**把外部新解析树与现有树（带 id）对位，产出五类清单
//!   （preserved / moved / edited / added / removed），三级策略：
//!   a. manifest 快路径（侧车清单完整且与新树逐位核验一致 → 直接对位）；
//!   b. 结构锚走查（同型同序同指纹的 front/back 游标推进，嵌套递归）；
//!   c. 中段兜底（精确指纹全局配对捕移动 → 窗内同型相似兜底 → 拆/并归组）。
//! - 确定性契约：全流程无 HashMap 迭代序（BTreeMap / Vec / 下标 memo）、
//!   哈希逐码点写入、平局一律取最小序位——同输入恒同输出，CI 可复现。
//! - 本模块全部 API 为只读检视：不碰 revision / 事件 / 历史 / 树。
//!
//! 匹配局限（v0 刻意收缩，见 ADR-B6）：跨容器移动 = remove+add；块型翻转
//! （如段落↔标题）= remove+add；同型同文双胞胎的歧义按最小序位确定性分配。

use serde::{Deserialize, Serialize};

use crate::id::BlockId;
use crate::markdown;
use crate::tree::{BlockKind, BlockTree, NodeId, NodePath};
use crate::Core;

// ---------------------------------------------------------------------------
// FNV-1a 64（手写，零依赖；选型理由见 ADR-B6）
// ---------------------------------------------------------------------------

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 域隔离的 FNV-1a 64 累加器。写入顺序即语义：调用方负责长度前缀防连拼歧义。
struct Hasher(u64);

impl Hasher {
    /// `tag` 做域隔离（结构域 / 内容域互不串味）。
    fn new(tag: u8) -> Self {
        let mut h = Hasher(FNV_OFFSET);
        h.write_bytes(&[tag]);
        h
    }
    fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }
    fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }
    fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// 规范化 + 指纹
// ---------------------------------------------------------------------------

/// 内容规范化：掐头去尾空白 + 内部空白游程折叠为单空格。
/// 理由（ADR-B6）：块文本的换行/缩进差异不构成身份信号；除此之外不做任何
/// 语义抹除——行内标记在 v0 文本模型里是字面内容（marks 是独立 payload），
/// 启发式剥除会造成假匹配，故保留。
pub fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_ws = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_ws = true;
            continue;
        }
        if pending_ws && !out.is_empty() {
            out.push(' ');
        }
        pending_ws = false;
        out.push(c);
    }
    out
}

/// 单块指纹（结构层 + 内容层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockFingerprint {
    pub structural: u64,
    pub content: u64,
}

/// 逐树指纹会话：下标 memo（NodeId.0 → 槽位数组），每节点至多算一次。
/// 不用 HashMap——彻底排除迭代序进入任何输出路径。
pub struct TreeFingerprint<'a> {
    tree: &'a BlockTree,
    structural: Vec<Option<u64>>,
    content: Vec<Option<u64>>,
}

impl<'a> TreeFingerprint<'a> {
    pub fn new(tree: &'a BlockTree) -> Self {
        let n = tree.node_count() + 1; // 含虚根槽位
        Self {
            tree,
            structural: vec![None; n],
            content: vec![None; n],
        }
    }

    /// 单块双指纹（memo 化）。
    pub fn block_fingerprint(&mut self, id: NodeId) -> BlockFingerprint {
        BlockFingerprint {
            structural: self.structural_of(id),
            content: self.content_of(id),
        }
    }

    /// 结构指纹：kind + attrs（BTreeMap 序）+ 子数 + 子结构指纹（递归）。
    /// marks 不入指纹（payload，由匹配后的逐字段比较捕捉）。
    pub fn structural_of(&mut self, id: NodeId) -> u64 {
        if let Some(v) = self.structural[id.0] {
            return v;
        }
        let node = self.tree.node(id);
        let mut h = Hasher::new(0xB7);
        h.write_bytes(&[kind_tag(node.kind)]);
        h.write_u64(node.attrs.len() as u64);
        for (k, v) in &node.attrs {
            h.write_u64(k.len() as u64);
            h.write_bytes(k.as_bytes());
            h.write_u64(v.len() as u64);
            h.write_bytes(v.as_bytes());
        }
        h.write_u64(node.children.len() as u64);
        for &c in &node.children {
            h.write_u64(self.structural_of(c));
        }
        let v = h.finish();
        self.structural[id.0] = Some(v);
        v
    }

    /// 内容指纹：规范化文本的「码点数 + 逐码点（u32 LE）」哈希。
    /// 码点语义：一个增补平面字符（如 U+1F389）作为单个 u32 写入，
    /// 不是 UTF-8 字节截断也不是代理对半。
    pub fn content_of(&mut self, id: NodeId) -> u64 {
        if let Some(v) = self.content[id.0] {
            return v;
        }
        let chars: Vec<char> = normalize_text(&self.tree.node(id).text.to_string())
            .chars()
            .collect();
        let mut h = Hasher::new(0xC1);
        h.write_u64(chars.len() as u64);
        for &c in &chars {
            h.write_u32(c as u32);
        }
        let v = h.finish();
        self.content[id.0] = Some(v);
        v
    }
}

/// kind → 稳定单字节标签（显式表，不依赖派生序）。
fn kind_tag(kind: BlockKind) -> u8 {
    match kind {
        BlockKind::Document => 0,
        BlockKind::Paragraph => 1,
        BlockKind::Heading => 2,
        BlockKind::ListItem => 3,
        BlockKind::Blockquote => 4,
        BlockKind::CodeBlock => 5,
        // #261：新变体取新值 8（0-7 稳定——旧指纹零漂移；显式表
        // 不依赖派生序，中途插入变体不影响既有判别值）。
        BlockKind::Math => 8,
        BlockKind::Table => 6,
        BlockKind::ThematicBreak => 7,
    }
}

// ---------------------------------------------------------------------------
// 内容相似度（编辑窗兜底用）
// ---------------------------------------------------------------------------

/// LCS 逐格预算护栏：超过即退 bigram Dice（O(n)），保证大块相似度有界。
const LCS_CELL_BUDGET: usize = 1_000_000;

/// 内容相似度（定谳，ADR-B6）：containment 比 = LCS / min(len)，
/// 在「空白规范化 + 大小写折叠」文本上逐码点计算。
/// 选型理由：编辑窗兜底要捕获「同型 + 主要内容相承」的小改动——短块编辑是
/// 常态，containment 对比例式插入最鲁棒（"alpha"→"alpha edited" = 1.0）；
/// 误匹配的代价是身份保守保留，优于误杀（remove+add）。空对空 = 1.0，
/// 空对非空 = 0.0。跨运行确定（无随机、无迭代序依赖）。
pub fn text_similarity(a: &str, b: &str) -> f64 {
    let la: Vec<char> = normalize_text(a).to_lowercase().chars().collect();
    let lb: Vec<char> = normalize_text(b).to_lowercase().chars().collect();
    if la.is_empty() && lb.is_empty() {
        return 1.0;
    }
    if la.is_empty() || lb.is_empty() {
        return 0.0;
    }
    if la.len().saturating_mul(lb.len()) > LCS_CELL_BUDGET {
        return bigram_dice(&la, &lb);
    }
    let lcs = lcs_len(&la, &lb);
    let min_len = la.len().min(lb.len()) as f64;
    lcs as f64 / min_len
}

/// 逐格 DP 的 LCS 长度（滚动两行）。
fn lcs_len(a: &[char], b: &[char]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for &ca in a {
        cur[0] = 0;
        for j in 1..=b.len() {
            cur[j] = if ca == b[j - 1] {
                prev[j - 1] + 1
            } else {
                prev[j].max(cur[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 超预算长文本的 O(n) 代理：码点 bigram 多重集 Dice（排序双指针计数）。
fn bigram_dice(a: &[char], b: &[char]) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return if a == b { 1.0 } else { 0.0 };
    }
    let mut ba: Vec<(char, char)> = a.windows(2).map(|w| (w[0], w[1])).collect();
    let mut bb: Vec<(char, char)> = b.windows(2).map(|w| (w[0], w[1])).collect();
    ba.sort_unstable();
    bb.sort_unstable();
    let (mut i, mut j, mut common) = (0usize, 0usize, 0usize);
    while i < ba.len() && j < bb.len() {
        match ba[i].cmp(&bb[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                common += 1;
                i += 1;
                j += 1;
            }
        }
    }
    2.0 * common as f64 / (ba.len() + bb.len()) as f64
}

// ---------------------------------------------------------------------------
// 匹配报告
// ---------------------------------------------------------------------------

/// 身份保住的块（preserved / moved / edited 三类的公共形状）。
/// `content_changed` = 文本（精确逐码点）/ marks / attrs 任一不等；
/// 指纹只驱动配对，本字段驱动 R23-c 的重载冲突状态机。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedBlock {
    pub id: BlockId,
    pub old_path: NodePath,
    pub new_path: NodePath,
    pub content_changed: bool,
}

/// 新树里对不上的块（子树根）。`id` = manifest 快路径带来的可信新身份
/// （合作写手场景）；无 manifest 时为 None。`subtree_size` = 整棵子树节点数。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedBlock {
    pub id: Option<BlockId>,
    pub path: NodePath,
    pub kind: BlockKind,
    pub subtree_size: usize,
}

/// 旧树里对不上的块（子树根，身份作废，整棵子树随根消失）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedBlock {
    pub id: BlockId,
    pub path: NodePath,
    pub kind: BlockKind,
    pub subtree_size: usize,
}

/// 三级策略的用量统计（可观测性 + 语义锁断言策略路径）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchTierStats {
    /// manifest 快路径直接对位的块数（0 = 未走快路径或核验失败回退）。
    pub manifest: usize,
    /// 结构锚走查（front/back 游标）配对数。
    pub anchor: usize,
    /// 中段精确指纹配对数（移动检测）。
    pub exact: usize,
    /// 窗内相似兜底 + 单例收尾配对数。
    pub similar: usize,
    /// 拆块/并块归组配对数。
    pub split_merge: usize,
}

/// 匹配报告（五类清单 + 策略用量）。列表内按路径字典序（= 文档序）排列。
/// 归类规则（ADR-B6 定谳）：
/// - moved = 匹配成功但同父序位变化；祖先搬移内部相对位不变的子块归
///   preserved（移动归因于祖先，符合「子树整体搬移 → 子内 preserved」）；
/// - preserved / edited = 同父序位不变，按 payload 是否变化二分；
/// - added / removed = 只报未配对子树的**根**（整棵子树随根计入）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockMatchReport {
    pub preserved: Vec<MatchedBlock>,
    pub moved: Vec<MatchedBlock>,
    pub edited: Vec<MatchedBlock>,
    pub added: Vec<AddedBlock>,
    pub removed: Vec<RemovedBlock>,
    pub stats: MatchTierStats,
}

impl BlockMatchReport {
    /// 五类全空（两树逐块等价）。
    pub fn is_trivial(&self) -> bool {
        self.preserved.is_empty()
            && self.moved.is_empty()
            && self.edited.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
    }

    /// 外部内容相对现树**有没有变化**（R23-c 重载决策口径）：preserved
    /// 不算变化——全同文档 preserved 满额但零变化（is_trivial 是"连
    /// preserved 都没有"的更强断言，勿混用）。
    pub fn has_changes(&self) -> bool {
        !(self.moved.is_empty()
            && self.edited.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty())
    }

    /// 身份保住的全部块 id（preserved + moved + edited，按 id 升序去重）。
    pub fn kept_ids(&self) -> Vec<BlockId> {
        let mut ids: Vec<BlockId> = self
            .preserved
            .iter()
            .chain(self.moved.iter())
            .chain(self.edited.iter())
            .map(|m| m.id)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}

// ---------------------------------------------------------------------------
// 匹配参数 + manifest 快路径
// ---------------------------------------------------------------------------

/// 匹配参数（定谳值见 Default；语义锁 tests/r23_fingerprint.rs 锁定行为）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchParams {
    /// 相似兜底阈值（containment 比，0.0–1.0）。
    pub similarity_threshold: f64,
    /// 编辑窗宽度：中段候选的同父相对位偏差上限。
    pub edit_window: usize,
}

impl Default for MatchParams {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.7,
            edit_window: 8,
        }
    }
}

/// manifest 快路径的单条声明：新树先序第 p 位 = 该 id + 该双指纹。
/// 由 Conforming 写手（我方编辑器存档）写入 meta 侧车；匹配端**全有全无**
/// 核验：条数与新树先序节点数一致，且每条的 structural/content 指纹与
/// 新树对应块逐位相等——清单描述的确实是这棵树，才允许直接对位；
/// 任何一位不符 = 清单过期/异源，整体弃用走 b/c 级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: BlockId,
    pub structural: u64,
    pub content: u64,
}

// ---------------------------------------------------------------------------
// 匹配器
// ---------------------------------------------------------------------------

/// 外部新解析树 ↔ 现有树（带 id）的结构匹配。纯函数：不改两棵树。
pub fn match_trees(
    old: &BlockTree,
    new: &BlockTree,
    manifest: Option<&[ManifestEntry]>,
    params: &MatchParams,
) -> BlockMatchReport {
    let mut m = Matcher {
        old,
        new,
        fp_old: TreeFingerprint::new(old),
        fp_new: TreeFingerprint::new(new),
        params: *params,
        report: BlockMatchReport::default(),
    };
    if let Some(entries) = manifest {
        if m.try_manifest_fast_path(entries) {
            m.sort_lists();
            return m.report;
        }
    }
    let old_kids = old.top_level().to_vec();
    let new_kids = new.top_level().to_vec();
    m.match_lists(old_kids, new_kids, Vec::new(), Vec::new());
    m.sort_lists();
    m.report
}

struct Matcher<'a> {
    old: &'a BlockTree,
    new: &'a BlockTree,
    fp_old: TreeFingerprint<'a>,
    fp_new: TreeFingerprint<'a>,
    params: MatchParams,
    report: BlockMatchReport,
}

/// 配对来源（进 stats）。
#[derive(Clone, Copy, PartialEq)]
enum Tier {
    Anchor,
    Exact,
    Similar,
    SplitMerge,
}

impl<'a> Matcher<'a> {
    // ---- manifest 快路径（tier a）----

    /// 全有全无核验 + 直接对位。成功 = true（report 已填充）。
    fn try_manifest_fast_path(&mut self, entries: &[ManifestEntry]) -> bool {
        let preorder = preorder_with_paths(self.new);
        if entries.len() != preorder.len() {
            return false;
        }
        for (entry, (node, _)) in entries.iter().zip(preorder.iter()) {
            let fp = self.fp_new.block_fingerprint(*node);
            if fp.structural != entry.structural || fp.content != entry.content {
                return false;
            }
        }
        // 直接对位：id 可解析 → 配对归类；不可解析 → 带可信 id 的 added。
        for (entry, (node, new_path)) in entries.iter().zip(preorder.iter()) {
            match self.old.lookup_id(entry.id) {
                Some(old_node) => {
                    let old_path = self.old.path_of(old_node).unwrap_or_default();
                    let changed = self.payload_changed(old_node, *node);
                    let mb = MatchedBlock {
                        id: entry.id,
                        old_path,
                        new_path: new_path.clone(),
                        content_changed: changed,
                    };
                    // moved 判据 = 同父序位变化（末段比较；祖先搬移归因祖先）。
                    if mb.old_path.last() != mb.new_path.last() {
                        self.report.moved.push(mb);
                    } else if changed {
                        self.report.edited.push(mb);
                    } else {
                        self.report.preserved.push(mb);
                    }
                    self.report.stats.manifest += 1;
                }
                None => {
                    self.report.added.push(AddedBlock {
                        id: Some(entry.id),
                        path: new_path.clone(),
                        kind: self.new.node(*node).kind,
                        subtree_size: subtree_size(self.new, *node),
                    });
                    // 可信 added 同属快路径直接对位（携带声明身份）——计入 manifest 桶
                    self.report.stats.manifest += 1;
                }
            }
        }
        true
    }

    /// payload 精确比较（文本逐码点 / marks / attrs）。
    fn payload_changed(&self, old_node: NodeId, new_node: NodeId) -> bool {
        let o = self.old.node(old_node);
        let n = self.new.node(new_node);
        o.text != n.text || o.marks != n.marks || o.attrs != n.attrs
    }

    // ---- 递归主流程（tier b/c）----

    /// 处理一对已对齐的兄弟列表（虚根层或某对已配对块的子层）。
    fn match_lists(
        &mut self,
        old_kids: Vec<NodeId>,
        new_kids: Vec<NodeId>,
        old_base: NodePath,
        new_base: NodePath,
    ) {
        let n_old = old_kids.len();
        let n_new = new_kids.len();
        let mut old_taken = vec![false; n_old];
        let mut new_taken = vec![false; n_new];
        let mut pairs: Vec<(usize, usize, Tier)> = Vec::new();

        // --- tier b：结构锚走查（front）---
        let (mut fi, mut fj) = (0usize, 0usize);
        while fi < n_old && fj < n_new && self.same_fp(old_kids[fi], new_kids[fj]) {
            old_taken[fi] = true;
            new_taken[fj] = true;
            pairs.push((fi, fj, Tier::Anchor));
            fi += 1;
            fj += 1;
        }
        // --- tier b：结构锚走查（back）---
        let (mut bi, mut bj) = (n_old as isize - 1, n_new as isize - 1);
        while bi >= fi as isize
            && bj >= fj as isize
            && self.same_fp(old_kids[bi as usize], new_kids[bj as usize])
        {
            old_taken[bi as usize] = true;
            new_taken[bj as usize] = true;
            pairs.push((bi as usize, bj as usize, Tier::Anchor));
            bi -= 1;
            bj -= 1;
        }
        // 中段未配对区间（front/back 之间；全被吃满则为空）。
        let old_mid: Vec<usize> = if bi >= fi as isize {
            (fi..=bi as usize).collect()
        } else {
            Vec::new()
        };
        let new_mid: Vec<usize> = if bj >= fj as isize {
            (fj..=bj as usize).collect()
        } else {
            Vec::new()
        };

        // --- tier c1：精确指纹全局配对（移动检测；最小序位平局）---
        let mut buckets: std::collections::BTreeMap<(u64, u64), Vec<usize>> = Default::default();
        for &i in &old_mid {
            let fp = self.fp_old.block_fingerprint(old_kids[i]);
            buckets
                .entry((fp.structural, fp.content))
                .or_default()
                .push(i);
        }
        for &j in &new_mid {
            let fp = self.fp_new.block_fingerprint(new_kids[j]);
            if let Some(list) = buckets.get_mut(&(fp.structural, fp.content)) {
                if let Some(&i) = list.first() {
                    list.remove(0);
                    old_taken[i] = true;
                    new_taken[j] = true;
                    pairs.push((i, j, Tier::Exact));
                }
            }
        }

        // --- tier c2：窗内同型相似兜底（贪心，相似最高，平局取最小旧序位）---
        for &j in &new_mid {
            if new_taken[j] {
                continue;
            }
            let n_node = self.new.node(new_kids[j]);
            let n_kind = n_node.kind;
            let n_text = n_node.text.to_string();
            let j_off = j - fj;
            let mut best: Option<(usize, f64)> = None;
            for &i in &old_mid {
                if old_taken[i] {
                    continue;
                }
                if (i - fi).abs_diff(j_off) > self.params.edit_window {
                    continue;
                }
                let o_node = self.old.node(old_kids[i]);
                if o_node.kind != n_kind {
                    continue;
                }
                let sim = text_similarity(&o_node.text.to_string(), &n_text);
                if sim >= self.params.similarity_threshold && best.is_none_or(|(_, bs)| sim > bs) {
                    best = Some((i, sim));
                }
            }
            if let Some((i, _)) = best {
                old_taken[i] = true;
                new_taken[j] = true;
                pairs.push((i, j, Tier::Similar));
            }
        }

        // --- tier d：拆块/并块归组（连续未配对游程；join 相似核验）---
        self.split_merge_pass(
            &old_mid,
            &new_mid,
            &mut old_taken,
            &mut new_taken,
            &mut pairs,
            &old_kids,
            &new_kids,
        );

        // --- 归类 + 子树递归 ---
        for (i, j, tier) in pairs {
            let old_path = child_path(&old_base, i);
            let new_path = child_path(&new_base, j);
            let id = self.old.node(old_kids[i]).id;
            let changed = self.payload_changed(old_kids[i], new_kids[j]);
            let mb = MatchedBlock {
                id,
                old_path: old_path.clone(),
                new_path: new_path.clone(),
                content_changed: changed,
            };
            if i != j {
                self.report.moved.push(mb);
            } else if changed {
                self.report.edited.push(mb);
            } else {
                self.report.preserved.push(mb);
            }
            match tier {
                Tier::Anchor => self.report.stats.anchor += 1,
                Tier::Exact => self.report.stats.exact += 1,
                Tier::Similar => self.report.stats.similar += 1,
                Tier::SplitMerge => self.report.stats.split_merge += 1,
            }
            let oc = self.old.node(old_kids[i]).children.clone();
            let nc = self.new.node(new_kids[j]).children.clone();
            self.match_lists(oc, nc, old_path, new_path);
        }

        // --- tier e：残渣 = added / removed（未配对子树的根）---
        for &i in &old_mid {
            if !old_taken[i] {
                let node = self.old.node(old_kids[i]);
                self.report.removed.push(RemovedBlock {
                    id: node.id,
                    path: child_path(&old_base, i),
                    kind: node.kind,
                    subtree_size: subtree_size(self.old, old_kids[i]),
                });
            }
        }
        for &j in &new_mid {
            if !new_taken[j] {
                let node = self.new.node(new_kids[j]);
                self.report.added.push(AddedBlock {
                    id: None,
                    path: child_path(&new_base, j),
                    kind: node.kind,
                    subtree_size: subtree_size(self.new, new_kids[j]),
                });
            }
        }
    }

    /// 连续未配对游程的 1↔k / k↔1 归组（join 相似 ≥ 阈值；对齐 ADR-B1：
    /// split 左保右新 / merge 存者=前）。命中即配游程首对，其余留给 tier e。
    // 算法内部私有函数：双游程+双占用位图+配对槽，8 参是分层匹配器的自然形状；
    // 语义由 r23_fingerprint 15 锁背书，非契约面。为风格拆结构体反增间接层。
    #[allow(clippy::too_many_arguments)]
    fn split_merge_pass(
        &mut self,
        old_mid: &[usize],
        new_mid: &[usize],
        old_taken: &mut [bool],
        new_taken: &mut [bool],
        pairs: &mut Vec<(usize, usize, Tier)>,
        old_kids: &[NodeId],
        new_kids: &[NodeId],
    ) {
        let old_runs = consecutive_runs(old_mid, old_taken);
        let new_runs = consecutive_runs(new_mid, new_taken);
        // 1 ↔ k（拆块：老块 ≈ 新游程文本 join）
        for &(osi, ose) in &old_runs {
            if ose - osi != 1 || old_taken[old_mid[osi]] {
                continue;
            }
            for &(njs, nje) in &new_runs {
                if nje - njs < 2 || new_taken[new_mid[njs]] {
                    continue;
                }
                let o = old_kids[old_mid[osi]];
                let first = new_kids[new_mid[njs]];
                let joined = join_run(self.new, &new_mid[njs..nje], new_kids);
                let old_text = self.old.node(o).text.to_string();
                if self.old.node(o).kind == self.new.node(first).kind
                    && text_similarity(&joined, &old_text) >= self.params.similarity_threshold
                {
                    old_taken[old_mid[osi]] = true;
                    new_taken[new_mid[njs]] = true;
                    pairs.push((old_mid[osi], new_mid[njs], Tier::SplitMerge));
                    break;
                }
            }
        }
        // k ↔ 1（并块：老游程文本 join ≈ 新块；存者 = 前，对齐 ADR-B1）
        for &(njs, nje) in &new_runs {
            if nje - njs != 1 || new_taken[new_mid[njs]] {
                continue;
            }
            for &(osi, ose) in &old_runs {
                if ose - osi < 2 || old_taken[old_mid[osi]] {
                    continue;
                }
                let n = new_kids[new_mid[njs]];
                let first = old_kids[old_mid[osi]];
                let joined = join_run(self.old, &old_mid[osi..ose], old_kids);
                let new_text = self.new.node(n).text.to_string();
                if self.new.node(n).kind == self.old.node(first).kind
                    && text_similarity(&joined, &new_text) >= self.params.similarity_threshold
                {
                    old_taken[old_mid[osi]] = true;
                    new_taken[new_mid[njs]] = true;
                    pairs.push((old_mid[osi], new_mid[njs], Tier::SplitMerge));
                    break;
                }
            }
        }
    }

    fn same_fp(&mut self, a: NodeId, b: NodeId) -> bool {
        let fa = self.fp_old.block_fingerprint(a);
        let fb = self.fp_new.block_fingerprint(b);
        fa.structural == fb.structural && fa.content == fb.content
    }

    fn sort_lists(&mut self) {
        self.report
            .preserved
            .sort_by(|a, b| a.old_path.cmp(&b.old_path));
        self.report
            .moved
            .sort_by(|a, b| a.old_path.cmp(&b.old_path));
        self.report
            .edited
            .sort_by(|a, b| a.old_path.cmp(&b.old_path));
        self.report.added.sort_by(|a, b| a.path.cmp(&b.path));
        self.report.removed.sort_by(|a, b| a.path.cmp(&b.path));
    }
}

fn child_path(base: &NodePath, index: usize) -> NodePath {
    let mut p = base.clone();
    p.push(index);
    p
}

/// 子树节点数（含根）。
fn subtree_size(tree: &BlockTree, root: NodeId) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root];
    while let Some(cur) = stack.pop() {
        count += 1;
        stack.extend(tree.node(cur).children.iter().copied());
    }
    count
}

/// 游程集：mid 列表里连续未配对下标的 [起,止) 段（相对 mid 自身下标）。
fn consecutive_runs(mid: &[usize], taken: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < mid.len() {
        if !taken[mid[i]] {
            let start = i;
            while i < mid.len() && !taken[mid[i]] {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }
    runs
}

/// 游程文本 join（单空格）——拆/并归组的核验输入。
fn join_run(tree: &BlockTree, run: &[usize], kids: &[NodeId]) -> String {
    run.iter()
        .map(|&m| tree.node(kids[m]).text.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 先序平铺（含路径），manifest 快路径与 Core 指纹清单共用。
fn preorder_with_paths(tree: &BlockTree) -> Vec<(NodeId, NodePath)> {
    fn walk(tree: &BlockTree, id: NodeId, base: &NodePath, out: &mut Vec<(NodeId, NodePath)>) {
        out.push((id, base.to_vec()));
        for (k, &c) in tree.node(id).children.iter().enumerate() {
            let mut p = base.clone();
            p.push(k);
            walk(tree, c, &p, out);
        }
    }
    let mut out = Vec::new();
    // 顶层路径约定与 match_lists 一致：顶层块 = [i]，其子块 = [i, j]…
    // （此前顶层给 []，manifest 快路径的 moved/edited 归类全部失准——已修）
    for (k, &c) in tree.top_level().iter().enumerate() {
        walk(tree, c, &vec![k], &mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// Core 出入口（只读；命名进我方命名域，中性词）
// ---------------------------------------------------------------------------

impl Core {
    /// 现树先序指纹清单（R23-c 存进 meta 侧车的形状）。
    pub fn fingerprint_manifest(&self) -> Vec<ManifestEntry> {
        let mut fp = TreeFingerprint::new(&self.tree);
        preorder_with_paths(&self.tree)
            .into_iter()
            .map(|(node, _)| {
                let f = fp.block_fingerprint(node);
                ManifestEntry {
                    id: self.tree.node(node).id,
                    structural: f.structural,
                    content: f.content,
                }
            })
            .collect()
    }

    /// 外部重载匹配：把 `md` 解析成新树与现树对位（只读，零状态触碰）。
    /// `manifest` = 侧车清单（None = 直接走结构锚 + 兜底）。
    pub fn match_external_markdown(
        &self,
        md: &str,
        manifest: Option<&[ManifestEntry]>,
    ) -> BlockMatchReport {
        let parsed = markdown::markdown_to_tree(md);
        match_trees(&self.tree, &parsed, manifest, &MatchParams::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_whitespace_only() {
        assert_eq!(normalize_text("  a \n b\t c  "), "a b c");
        assert_eq!(normalize_text("   "), "");
        assert_eq!(normalize_text("ab"), "ab");
    }

    #[test]
    fn hasher_is_deterministic_and_domain_tagged() {
        let mut h1 = Hasher::new(0xB7);
        h1.write_u32(0x1F389);
        let mut h2 = Hasher::new(0xB7);
        h2.write_u32(0x1F389);
        assert_eq!(h1.finish(), h2.finish());
        let mut g = Hasher::new(0xC1);
        g.write_u32(0x1F389);
        assert_ne!(h1.finish(), g.finish(), "域标签必须隔离结构域与内容域");
    }

    #[test]
    fn similarity_corners() {
        assert_eq!(text_similarity("", ""), 1.0);
        assert_eq!(text_similarity("", "x"), 0.0);
        assert_eq!(text_similarity("one", "ONE"), 1.0, "大小写不敏感");
        assert!(text_similarity("a  b", "a b") > 0.999, "空白折叠后全等");
        assert!(text_similarity("beta", "gamma") < 0.3, "不相干文本低分");
        assert!(
            text_similarity("alpha", "alpha edited") > 0.999,
            "containment 语义"
        );
    }
}
