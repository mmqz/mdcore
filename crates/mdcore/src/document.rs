//! 文件层 — document_id（§8.3 workspace metadata / UUIDv7）+ 开档/存档往返。
//!
//! - `DocumentId`：UUIDv7 形状（RFC 9562：48bit 毫秒时间戳 + version 7 +
//!   rand_a + variant 10 + rand_b），**零外部依赖**——时间从注入时钟取
//!   （`generate(now_ms)`），随机用 std 熵源（`RandomState` 的 OS 种子 +
//!   进程内计数器 + 栈地址 ASLR 熵混合，非加密强度，按任务书豁免）。
//! - Display/FromStr = 标准 8-4-4-4-12 小写十六进制；FromStr 校验
//!   version 半字节 == 7 与 variant 顶位 == 0b10。
//! - 持久身份：genesis 日志记录携带 document_id（JSONL 首条），重放恢复；
//!   `save_document` 把 id + revision + 历史栈写进 meta JSON，
//!   `open_document` = from_markdown + 全量状态复原（重开匹配）。
//!
//! 设计说明（两处与任务书字面的偏差，理由见 worklog）：
//! 1. `set_document_id(Option<DocumentId>)`——None = 用核内注入时钟现场
//!    生成一枚（宿主无身份时一行接入）；Some = 显式指定。
//! 2. `open_document -> Result<Core, Reject>`——meta 畸形（坏 id/坏历史）
//!    必须显式失败，静默开档会造成重开身份错配。

use serde::{Deserialize, Serialize};

use crate::id::BlockId;
use crate::markdown;
use crate::Reject;

/// #244 路线 A：meta 顶层格式版本。
///
/// - **v2**（当前）：携带 `blocks` = 文档先序持久化 ID 表（父先子后，
///   跳过虚根）——save→open 身份稳定（I2），跨会话 undo 物化 ID 不与
///   重解析存活块撞号（I3）。
/// - **v1（缺省 = 旧档）**：无 blocks 表。open 按先序重发 ID（旧语义）
///   ——旧档携带的历史栈若与重发 ID 撞号，由 register 守卫（I4）在
///   undo 时响亮拦截（typed error + 状态完整保留），不再静默毒化。
///   旧档再保存即升 v2（单向收敛，与 history 版本闸门同哲学）。
/// - **未来版本（> 2）**：blocks 表语义未知不解读，先序回退；其余字段
///   （正文/身份/revision/history 各自闸门）照常（前向保护）。
pub const META_FORMAT_VERSION: u32 = 2;

/// 进程内熵混合计数器（每次生成都翻搅一次哈希输入）。
static ENTROPY_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// std 手段熵源：`RandomState` 每次新建都从 OS 种子派生新键（进程内唯一），
/// 再混入单调计数器与栈地址（ASLR）。非加密强度，足以做 UUIDv7 的随机位。
fn std_entropy() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let seq = ENTROPY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stack_probe = 0u8;
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(seq);
    hasher.write_usize(&stack_probe as *const u8 as usize);
    hasher.finish()
}

/// 文档身份（UUIDv7 形状）。序列化 = 小写十六进制字符串。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId {
    bytes: [u8; 16],
}

impl DocumentId {
    /// 以注入时钟的毫秒时间戳生成一枚。时间取低 48 位（RFC 9562 布局）。
    pub fn generate(now_ms: u64) -> Self {
        let ts = now_ms & ((1u64 << 48) - 1);
        let rand_a = std_entropy() & 0x0fff; // 12 bit
        let rand_b = std_entropy() & 0x3fff_ffff_ffff_ffff; // 62 bit
        let mut bytes = [0u8; 16];
        bytes[0] = (ts >> 40) as u8;
        bytes[1] = (ts >> 32) as u8;
        bytes[2] = (ts >> 24) as u8;
        bytes[3] = (ts >> 16) as u8;
        bytes[4] = (ts >> 8) as u8;
        bytes[5] = ts as u8;
        bytes[6] = 0x70 | ((rand_a >> 8) as u8); // version 7 + rand_a 高 4 位
        bytes[7] = rand_a as u8;
        bytes[8] = 0x80 | ((rand_b >> 56) as u8); // variant 10 + rand_b 高 6 位
        bytes[9] = (rand_b >> 48) as u8;
        bytes[10] = (rand_b >> 40) as u8;
        bytes[11] = (rand_b >> 32) as u8;
        bytes[12] = (rand_b >> 24) as u8;
        bytes[13] = (rand_b >> 16) as u8;
        bytes[14] = (rand_b >> 8) as u8;
        bytes[15] = rand_b as u8;
        Self { bytes }
    }

    /// 48bit 毫秒时间戳（生成时钟的读数）。
    pub fn timestamp_ms(&self) -> u64 {
        let b = &self.bytes;
        ((b[0] as u64) << 40)
            | ((b[1] as u64) << 32)
            | ((b[2] as u64) << 24)
            | ((b[3] as u64) << 16)
            | ((b[4] as u64) << 8)
            | (b[5] as u64)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.bytes
    }

    fn hex_group(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = &self.bytes;
        write!(
            f,
            "{}-{}-{}-{}-{}",
            Self::hex_group(&b[0..4]),
            Self::hex_group(&b[4..6]),
            Self::hex_group(&b[6..8]),
            Self::hex_group(&b[8..10]),
            Self::hex_group(&b[10..16]),
        )
    }
}

/// 解析失败原因（Display 直出，供 wasm 边界转 JsValue）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentIdError(pub String);

impl std::fmt::Display for DocumentIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid document id: {}", self.0)
    }
}

impl std::error::Error for DocumentIdError {}

impl std::str::FromStr for DocumentId {
    type Err = DocumentIdError;

    /// 标准 8-4-4-4-12 十六进制（大小写均可），硬校验 version=7 与 variant=10。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = |why: &str| DocumentIdError(format!("\"{s}\": {why}"));
        let bytes = s.as_bytes();
        if bytes.len() != 36 {
            return Err(bad("expect 36 chars (8-4-4-4-12)"));
        }
        for &i in &[8usize, 13, 18, 23] {
            if bytes[i] != b'-' {
                return Err(bad("expect dash at 8/13/18/23"));
            }
        }
        let mut hex = [0u8; 32];
        let mut hi = 0;
        for (i, &c) in bytes.iter().enumerate() {
            if [8, 13, 18, 23].contains(&i) {
                continue;
            }
            let v = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(bad("non-hex digit")),
            };
            hex[hi] = v;
            hi += 1;
        }
        let mut out = [0u8; 16];
        for i in 0..16 {
            out[i] = (hex[2 * i] << 4) | hex[2 * i + 1];
        }
        if out[6] >> 4 != 0x7 {
            return Err(bad("version nibble must be 7 (UUIDv7)"));
        }
        if out[8] >> 6 != 0b10 {
            return Err(bad("variant bits must be 10 (RFC 9562)"));
        }
        Ok(Self { bytes: out })
    }
}

impl Serialize for DocumentId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DocumentId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<DocumentId>().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Core 文件层
// ---------------------------------------------------------------------------

use crate::Core;

impl Core {
    /// 文档身份（§8.3）。未设置 = None（匿名文档，如 from_markdown 直构）。
    pub fn document_id(&self) -> Option<DocumentId> {
        self.document_id
    }

    /// 设置文档身份：幂等（同值重复设置 = Ok）；已持不同值 = 拒绝
    /// （重开身份错配绝不静默换牌）。None = 用注入时钟现场生成。
    ///
    /// 落定后回写 genesis 日志记录（若存在且尚未携带），保证此后导出的
    /// JSONL 自含 document_id，重放即可恢复。
    pub fn set_document_id(&mut self, id: Option<DocumentId>) -> Result<(), Reject> {
        let id = match id {
            Some(x) => x,
            None => DocumentId::generate(self.now_ms()),
        };
        if let Some(existing) = self.document_id {
            if existing != id {
                return Err(Reject::Invalid(format!(
                    "document_id already set to {existing}; refusing to replace with {id}"
                )));
            }
            return Ok(());
        }
        self.document_id = Some(id);
        // genesis 回写：首条记录且确为 genesis（revision 0 + command 标记）。
        if let Some(rec) = self.log.records.first_mut() {
            if rec.revision == 0
                && rec.command.as_deref() == Some("genesis")
                && rec.document_id.is_none()
            {
                rec.document_id = Some(id);
            }
        }
        Ok(())
    }

    /// 存档：返回 (markdown, meta json)。meta 自含 document_id / revision /
    /// 历史栈（整栈 JSON 嵌入，携带 `HISTORY_FORMAT_VERSION` 版本字段）/
    /// **blocks 先序 ID 表**（#244 路线 A，携带 `META_FORMAT_VERSION`），
    /// 重开 = `open_document` 一行复原（含块身份）。
    pub fn save_document(&self) -> (String, String) {
        let md = markdown::tree_to_markdown(&self.tree);
        let history: serde_json::Value =
            serde_json::from_str(&self.history.to_json()).unwrap_or(serde_json::Value::Null);
        let blocks: Vec<u64> = self.tree.preorder_block_ids().iter().map(|b| b.0).collect();
        let meta = serde_json::json!({
            "meta_format": META_FORMAT_VERSION,
            "document_id": self.document_id.map(|id| id.to_string()),
            "revision": self.revision,
            "blocks": blocks,
            "history": history,
        });
        (md, meta.to_string())
    }

    /// 开档：from_markdown + meta 复原（document_id / revision / 历史栈 /
    /// 块身份表）。meta 畸形 = 显式 Reject（宁可开不了档，不静默错配身份）。
    ///
    /// 块身份（#244 路线 A）：meta v2 携 blocks 先序表 → 按表恢复原 ID
    /// （save→open 身份稳定）；表内重复/非 u64 = 畸形 meta 显式 Reject；
    /// **数量失配**（正文被外部改动/部分写入）= 降级：先序重发 + 丢弃
    /// 历史栈（history 路径引用旧结构，照常恢复会静默写错位置——宁可
    /// 丢撤销不赌树）。旧档（无表）按先序重发，撞号面由 register 守卫
    /// 响亮拦截。
    ///
    /// 历史栈过 `History::from_json` 版本闸门（#221 补充修复）：缺版本/
    /// 旧版本/未知未来版本的栈不可信（#220 修复前的 inverses 为整体
    /// 反转形态，非叶删除条目 undo 必坏），降级丢弃 undo/redo 栈；
    /// 正文 / document_id / revision 照常复原（旧档升级后可继续编辑，
    /// 只是丢失撤销历史，不丢失文档）。
    pub fn open_document(markdown_src: &str, meta_json: &str) -> Result<Self, Reject> {
        let bad = |why: String| Reject::Invalid(format!("open_document meta: {why}"));
        let meta: serde_json::Value =
            serde_json::from_str(meta_json).map_err(|e| bad(format!("bad json: {e}")))?;

        let doc_id = match meta.get("document_id") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(s)) => Some(
                s.parse::<DocumentId>()
                    .map_err(|e| bad(format!("bad document_id: {e}")))?,
            ),
            Some(other) => return Err(bad(format!("document_id must be a string, got {other}"))),
        };
        let revision = meta.get("revision").and_then(|v| v.as_u64()).unwrap_or(0);
        let history_json = match meta.get("history") {
            None | Some(serde_json::Value::Null) => None,
            Some(h) => Some(serde_json::to_string(h).map_err(|e| bad(format!("history: {e}")))?),
        };

        // #244：blocks 表仅当前 meta 版本解读（v1 缺省/未来版本 → 忽略；
        // 字段在但非数字 = 畸形 meta 显式 Reject）。表畸形（非数组/非 u64/
        // 重复）同样 Reject；数量失配在 from_markdown_with_ids 里报出，
        // 由降级分支接管。
        let meta_format: Option<u64> = match meta.get("meta_format") {
            None => None, // v1 旧档（本字段 v2 起写入）
            Some(v) => Some(
                v.as_u64()
                    .ok_or_else(|| bad(format!("meta_format must be a number, got {v}")))?,
            ),
        };
        let blocks: Option<Vec<BlockId>> = match meta_format {
            Some(v) if v == META_FORMAT_VERSION as u64 => match meta.get("blocks") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::Array(arr)) => {
                    let mut ids = Vec::with_capacity(arr.len());
                    let mut seen = std::collections::BTreeSet::new();
                    for entry in arr {
                        let raw = entry
                            .as_u64()
                            .ok_or_else(|| bad(format!("blocks entry must be u64, got {entry}")))?;
                        if !seen.insert(raw) {
                            return Err(bad(format!("duplicate block id {raw} in blocks table")));
                        }
                        ids.push(BlockId(raw));
                    }
                    Some(ids)
                }
                Some(other) => return Err(bad(format!("blocks must be an array, got {other}"))),
            },
            _ => None,
        };

        // 降级标记：数量失配（meta 与正文失同步）时丢弃历史栈——
        // 旧结构的路径/物化 ID 对不上重发后的树，照常恢复会静默写错位置。
        let mut history_dropped = false;
        let mut core = match blocks {
            Some(ids) => match Self::from_markdown_with_ids(markdown_src, &ids) {
                Ok(c) => c,
                Err(_) => {
                    history_dropped = true;
                    Self::from_markdown(markdown_src)
                }
            },
            None => Self::from_markdown(markdown_src),
        };
        if let Some(id) = doc_id {
            core.set_document_id(Some(id))?; // 会回写 genesis
        }
        core.revision = revision;
        if let Some(h) = history_json {
            if !history_dropped {
                core.set_history_json(&h)
                    .map_err(|e| bad(format!("bad history json: {e}")))?;
            }
            // 降级分支：留 default History（新会话，无合并窗残留）
        }
        Ok(core)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuidv7_layout_bits() {
        let now = 1_700_000_000_123u64; // < 2^48
        let id = DocumentId::generate(now);
        let b = id.as_bytes();
        assert_eq!(b[6] >> 4, 0x7, "version nibble = 7");
        assert_eq!(b[8] >> 6, 0b10, "variant top bits = 10");
        assert_eq!(id.timestamp_ms(), now, "48bit 时间戳逐位还原");
        // 同钟连发不撞车（随机位熵混合生效）
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(DocumentId::generate(now)), "随机位不得重复");
        }
    }
}
