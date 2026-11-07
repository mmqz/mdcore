//! LRU 页缓存——字节预算型 O(1) 双向链表缓存（PDF-c 阅读面，常开模块）。
//!
//! 语义（语义锁背书，tests/pdfc_cache_worker.rs）：
//! - **O(1) get/put**：`HashMap` 键→槽位索引 + arena 式双向链表（槽位
//!   Vec + u32 索引，槽 0 = 哨兵；空闲槽走 free 栈复用）——零 unsafe、
//!   零依赖（常开纪律：无 feature 也可用，mock 渲染链路可测）；
//! - **命中即提权**：`get` 命中条目移至链表头（MRU 端）；未命中与
//!   `contains` 观测**不**改变顺序；
//! - **驱逐从尾**（LRU 端）：`put` 超预算时自尾驱逐并返回被驱逐键集
//!   （升序遍历成本 O(驱逐数)；调用方可据此作废关联资源——如 GPU
//!   纹理/宿主位图句柄）；
//! - **字节预算精确性**：预算 = 字节数而非条目数（S3 限额精神：以
//!   实际内存占用计）。不变量 `bytes() <= byte_budget`（例外见下）；
//!   计数为 saturating（usize 溢出防御，不 panic）；
//! - **重复 put 更新不重复计费**：同键二次 put 原位更新（退旧成本、
//!   计新成本、提权），不产生第二条目；
//! - **超大条目自逐**：单条成本 > 预算时，其余条目先逐、该条目最后
//!   自逐（驱逐循环自尾收敛的均匀语义）——预算不变量保持成立；
//!   **零成本条目**（cost = 0）不占预算，零预算缓存仍可存零成本条目；
//! - **观测面**：`len`/`bytes`/`entries`（LRU→MRU 驱逐候选序）/
//!   `contains`（不提权）/ `clear`（全量复位，值即刻释放）；
//! - **失效面**（#305 方案 B——flush 后旧位图不得继续服役）：
//!   `invalidate`（单键精准摘除，返回值供调用方作废关联资源）+
//!   `invalidate_matching`（谓词批量摘除，返回被摘除键集，序 = LRU→MRU
//!   与 `put` 驱逐键集同款载荷面）。失效不等于 `clear`：条目级摘除
//!   保住未受影响条目的命中（页面视图翻页性能不因单页失效全量冷启动）；
//!   `PageCache<RenderKey, V>` 的 PDF 感知失效粒度见 [`crate::render`]。
//!
//! 键约束 `Copy + Eq + Hash`：驱逐键集以值返回（调用方作废关联资源
//! 需要键的拷贝）；值为任意 `V`（阅读面 = [`crate::render::RenderedPage`]，
//! 字节成本 = `bgra.len()`）。

use std::collections::HashMap;
use std::hash::Hash;

/// 槽位：链表节点（prev/next，u32 槽位索引）+ 条目单元。
///
/// 槽 0 恒为**哨兵**（`entry = None`，`prev` = LRU 端，`next` = MRU 端；
/// 空表时 prev = next = 0）。链表环形：尾节点的 `next` = 0（哨兵）、
/// 哨兵的 `prev` = 尾节点。空闲槽（已驱逐待复用）同样 `entry = None`。
struct Slot<K, V> {
    prev: u32,
    next: u32,
    entry: Option<EntryCell<K, V>>,
}

struct EntryCell<K, V> {
    key: K,
    value: Option<V>,
    cost: usize,
}

/// 字节预算 LRU 缓存（通用键值；阅读面典型形态
/// `PageCache<(u32, u32), RenderedPage>`——键 = (页号, 目标宽)，值 = 位图）。
pub struct PageCache<K, V>
where
    K: Copy + Eq + Hash,
{
    /// 键 → 槽位下标（含哨兵的槽表下标）。
    map: HashMap<K, u32>,
    /// 槽位 arena：slots[0] = 哨兵；活跃/空闲槽随后。只增不复位（clear 除外），
    /// free 栈驱动复用——索引稳定性 = 链表指针不被搬移失效（arena 的意义）。
    slots: Vec<Slot<K, V>>,
    /// 空闲槽栈（LIFO 复用）。
    free: Vec<u32>,
    byte_budget: usize,
    bytes: usize,
}

impl<K, V> PageCache<K, V>
where
    K: Copy + Eq + Hash,
{
    /// 构造（字节预算；预算以字节计非条目数）。
    pub fn new(byte_budget: usize) -> Self {
        let mut cache = Self {
            map: HashMap::new(),
            slots: Vec::new(),
            free: Vec::new(),
            byte_budget,
            bytes: 0,
        };
        cache.slots.push(Slot {
            prev: 0,
            next: 0,
            entry: None,
        }); // 槽 0 = 哨兵
        cache
    }

    /// 字节预算（构造值）。
    pub fn byte_budget(&self) -> usize {
        self.byte_budget
    }

    /// 当前条目数。
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 空缓存判定（len == 0）。
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 当前字节占用（Σ 生效条目成本；saturating 计数）。
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// 键在位观测——**不提权**（纯查询，与 `get` 的命中提权语义分工）。
    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// 取值——命中即提权（移至 MRU 端）；未命中返回 None 且**不**改变
    /// 任何条目的顺序。
    pub fn get(&mut self, key: &K) -> Option<&V> {
        let idx = *self.map.get(key)?;
        self.unlink(idx);
        self.push_front(idx);
        self.slots[idx as usize]
            .entry
            .as_ref()
            .and_then(|cell| cell.value.as_ref())
    }

    /// 写入（`byte_cost` = 该条目的字节成本，调用方申报——阅读面 =
    /// 位图缓冲长度）。
    ///
    /// 返回**被驱逐键集**（驱逐序 = LRU→MRU；超大条目自逐时含其自身
    /// 键）。同键重复 put = 原位更新（不重复计费）+ 提权。
    pub fn put(&mut self, key: K, value: V, byte_cost: usize) -> Vec<K> {
        match self.map.get(&key).copied() {
            Some(idx) => {
                // 原位更新：map 命中的槽必有 entry（不变量——evict/clear
                // 同步移除 map 项，二者不分离）。
                let cell = self.slots[idx as usize]
                    .entry
                    .as_mut()
                    .expect("map 命中槽必有条目（evict 与 map 移除同步）");
                self.bytes = self.bytes.saturating_sub(cell.cost);
                cell.value = Some(value);
                cell.cost = byte_cost;
                self.bytes = self.bytes.saturating_add(byte_cost);
                self.unlink(idx);
                self.push_front(idx);
            }
            None => {
                let idx = self.alloc_slot(key, value, byte_cost);
                self.map.insert(key, idx);
                self.bytes = self.bytes.saturating_add(byte_cost);
                self.push_front(idx);
            }
        }
        // 驱逐至预算内：自尾（LRU）驱逐；新条目在头（MRU），其余条目
        // 先逐，超大条目（成本 > 预算）最后自逐——不变量 bytes <= budget。
        let mut evicted = Vec::new();
        while self.bytes > self.byte_budget {
            match self.evict_tail() {
                Some(key) => evicted.push(key),
                None => break, // 空表（bytes = 0 <= 预算恒成立——防御分支）
            }
        }
        evicted
    }

    /// 全量复位：条目与值即刻释放，链表/空闲栈/映射清空，字节归零。
    /// 预算保持构造值（预算不属于可变状态——S3 限额数值权威在 Limits）。
    pub fn clear(&mut self) {
        self.map.clear();
        self.slots.clear();
        self.slots.push(Slot {
            prev: 0,
            next: 0,
            entry: None,
        });
        self.free.clear();
        self.bytes = 0;
    }

    /// 条目快照（LRU→MRU 序 = 驱逐候选序；观测面，非热路径——Vec 收集）。
    pub fn entries(&self) -> Vec<(K, &V)> {
        let mut out = Vec::with_capacity(self.map.len());
        let mut idx = self.slots[0].prev; // 尾 = LRU 端
        while idx != 0 {
            if let Some(cell) = self.slots[idx as usize].entry.as_ref() {
                if let Some(value) = cell.value.as_ref() {
                    out.push((cell.key, value));
                }
            }
            idx = self.slots[idx as usize].prev;
        }
        out
    }

    // ------------------------------------------------------------------
    // 失效面（#305 方案 B）
    // ------------------------------------------------------------------

    /// 单键精准摘除：摘链 + 移除映射 + 退成本 + 槽入空闲栈，返回被摘除
    /// 条目的**值**（调用方据此作废关联资源——GPU 纹理/宿主位图句柄；
    /// 返回即所有权移交，本缓存不再持有）。键不在位 → `None`（幂等：
    /// 重复失效不报错）。
    ///
    /// 与 `clear` 的分工：`clear` 是全量复位（会话级重开），本方法是
    /// 条目级失效（标注/内容变更后的精准作废——未受影响条目的命中
    /// 不受扰动）。
    pub fn invalidate(&mut self, key: &K) -> Option<V> {
        let idx = *self.map.get(key)?;
        let cell = self.remove_slot(idx)?;
        cell.value
    }

    /// 谓词批量摘除：摘除所有键满足 `pred` 的条目，返回被摘除键集
    /// （**LRU→MRU 序**——与 `put` 驱逐键集同款载荷面，调用方可按序
    /// 作废关联资源）。不满足谓词的条目原序保持在位（命中率保护——
    /// 失效粒度是本方法存在的意义）。
    ///
    /// 典型消费方：`PageCache<RenderKey, V>` 的 PDF 感知失效
    /// （`invalidate_page` / `invalidate_stale_revisions` /
    /// `invalidate_all`——见 [`crate::render`]）。
    pub fn invalidate_matching(&mut self, pred: impl Fn(&K) -> bool) -> Vec<K> {
        // 第一趟（只读）：沿链表自尾（LRU）向头（MRU）收集命中槽位——
        // 遍历序即返回序（确定性，与 entries()/put 驱逐序同源）。
        let mut hit_slots: Vec<u32> = Vec::new();
        let mut idx = self.slots[0].prev;
        while idx != 0 {
            if let Some(cell) = self.slots[idx as usize].entry.as_ref() {
                if pred(&cell.key) {
                    hit_slots.push(idx);
                }
            }
            idx = self.slots[idx as usize].prev;
        }
        // 第二趟（可变）：逐槽摘除（各槽独立——unlink 对既有指针成立，
        // 先后摘除互不依赖）。
        hit_slots
            .into_iter()
            .filter_map(|idx| self.remove_slot(idx).map(|cell| cell.key))
            .collect()
    }

    /// 摘除槽位（内部原语）：摘链 + 移除映射 + 退成本 + 槽入空闲栈。
    /// 返回被摘除的条目单元（槽上无条目 = 防御分支 → None，正常路径
    /// 不可达——map 命中槽必有条目，与 evict 的同步不变量同源）。
    fn remove_slot(&mut self, idx: u32) -> Option<EntryCell<K, V>> {
        let cell = self.slots[idx as usize].entry.take()?;
        self.unlink(idx);
        self.map.remove(&cell.key);
        self.bytes = self.bytes.saturating_sub(cell.cost);
        self.free.push(idx);
        Some(cell)
    }

    // ------------------------------------------------------------------
    // 内部：arena/链表原语（全部 O(1)）
    // ------------------------------------------------------------------

    /// 分配槽位：空闲栈优先复用，否则追加新槽。
    fn alloc_slot(&mut self, key: K, value: V, cost: usize) -> u32 {
        let cell = EntryCell {
            key,
            value: Some(value),
            cost,
        };
        match self.free.pop() {
            Some(idx) => {
                self.slots[idx as usize] = Slot {
                    prev: 0,
                    next: 0,
                    entry: Some(cell),
                };
                idx
            }
            None => {
                self.slots.push(Slot {
                    prev: 0,
                    next: 0,
                    entry: Some(cell),
                });
                // 槽位索引 u32：缓存条目数超 u32::MAX 前内存早已先崩——
                // debug 断言而非静默截断。
                debug_assert!(self.slots.len() <= u32::MAX as usize);
                (self.slots.len() - 1) as u32
            }
        }
    }

    /// 摘除节点（双向链接互指复原；对唯一节点/首尾节点均成立——环形不变量）。
    fn unlink(&mut self, idx: u32) {
        let (prev, next) = {
            let slot = &self.slots[idx as usize];
            (slot.prev, slot.next)
        };
        self.slots[prev as usize].next = next;
        self.slots[next as usize].prev = prev;
    }

    /// 节点插至哨兵之后（MRU 端）。
    fn push_front(&mut self, idx: u32) {
        let first = self.slots[0].next;
        self.slots[idx as usize].prev = 0;
        self.slots[idx as usize].next = first;
        self.slots[first as usize].prev = idx;
        self.slots[0].next = idx;
    }

    /// 驱逐尾节点（LRU 端）：摘链 + 移除映射 + 退成本 + 值即刻释放 +
    /// 槽入空闲栈。返回被驱逐键（空表返回 None）。
    fn evict_tail(&mut self) -> Option<K> {
        let tail = self.slots[0].prev;
        if tail == 0 {
            return None; // 空表
        }
        // 键副本先取（K: Copy）——随后的 take 会移走条目单元。
        let key = self.slots[tail as usize].entry.as_ref()?.key;
        let cell = self.slots[tail as usize]
            .entry
            .take()
            .expect("活跃槽必有条目（evict 与 map 移除同步）");
        self.unlink(tail);
        self.map.remove(&key);
        self.bytes = self.bytes.saturating_sub(cell.cost);
        drop(cell); // 值随驱逐即刻释放（字节预算的意义所在）
        self.free.push(tail);
        Some(key)
    }
}
