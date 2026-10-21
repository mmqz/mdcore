//! 对象身份：CRDT 锚点（证据锚本位币）+ 侧车序列化。
//!
//! 身份双轨（yrs-e §4.3）：
//! - `block_id`（应用级字符串，载体属性承载）——操作寻址面；
//! - [`CrdtAnchor`]（CRDT 首项 ID 的不透明包装）——证据锚面：
//!   结构性编辑（邻居增删/搬移）后仍可解算到同一块；**删除后解算
//!   = 显式 [`CollabError::AnchorDangling`]（不静默返回邻居）**；
//!   undo 复活 = 新 ID 族（d1e-t1 定谳——旧锚点保持悬空，恢复走
//!   显式重验通道）。

use yrs::{IndexedSequence, Transact, XmlFragment};

use crate::session::{block_id_of, sticky_from_anchor, CollabError, CollabSession, ROOT};
use crate::BlockKind;

/// CRDT 锚点（跨模型身份本位币——不透明，永不重编）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CrdtAnchor {
    pub client: u64,
    pub clock: u32,
}

/// 锚点解算结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAnchor {
    pub block_id: String,
    pub kind: BlockKind,
    /// 父块（None = 顶层）。
    pub parent_block_id: Option<String>,
    /// 父序列内物理子位（含 own-text 段偏移）。
    pub index: u32,
}

impl CollabSession {
    /// 绑定证据锚（block_id → 当前载体 ID；重复绑定 = 重指最新）。
    pub fn bind_anchor(&mut self, block_id: &str) -> Result<CrdtAnchor, CollabError> {
        self.check_inbound(block_id.len())?;
        let anchor = self.bind_anchor_internal(block_id)?;
        self.anchors_mut().insert(block_id.to_string(), anchor);
        Ok(anchor)
    }

    /// 已登记锚点查询（未绑定 = None）。
    pub fn anchor_of(&self, block_id: &str) -> Option<CrdtAnchor> {
        self.anchors().get(block_id).copied()
    }

    /// 解算锚点：全树比对各块元素的 sticky 首项 ID（悬空面显式化——
    /// 目标块已删除/身份换新族时 `AnchorDangling`，绝不返回邻居）。
    pub fn resolve_anchor(&self, anchor: CrdtAnchor) -> Result<ResolvedAnchor, CollabError> {
        let xml = self.doc().get_or_insert_xml_fragment(ROOT);
        let txn = self.doc().transact();
        let sticky = sticky_from_anchor(anchor);
        // 先直接解算（get_offset 对已删除目标返回 None 或塌缩位——
        // 塌缩位必须再过全树身份比对，不信任位值）。
        let _ = &sticky;
        if let Some(resolved) = resolve_in_fragment(&xml, &txn, anchor) {
            return Ok(resolved);
        }
        Err(CollabError::AnchorDangling {
            client: anchor.client,
            clock: anchor.clock,
        })
    }

    /// 锚点表侧车导出（确定性字节——按 block_id 排序）。
    pub fn export_anchor_sidecar(&self) -> Vec<u8> {
        let mut entries: Vec<(&String, &CrdtAnchor)> = self.anchors().iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let mut out = Vec::with_capacity(16 + entries.len() * 32);
        out.extend_from_slice(b"MDCC");
        out.push(1u8); // 格式版本
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (id, anchor) in entries {
            push_str(&mut out, id);
            out.extend_from_slice(&anchor.client.to_le_bytes());
            out.extend_from_slice(&anchor.clock.to_le_bytes());
        }
        out
    }

    /// 侧车导入（替换式；返回条数）。敌意载荷 fail-closed：字节门 +
    /// count 物理上界（条目最小物理尺寸 = 4B 长度前缀 + 12B 载荷 =
    /// 16B）+ 尾部零冗余。
    pub fn import_anchor_sidecar(&mut self, bytes: &[u8]) -> Result<usize, CollabError> {
        self.check_inbound(bytes.len())?;
        if bytes.len() < 9 || &bytes[0..4] != b"MDCC" {
            return Err(CollabError::SidecarFormat("magic 缺失".into()));
        }
        if bytes[4] != 1u8 {
            return Err(CollabError::SidecarFormat(format!(
                "不支持的版本 {}",
                bytes[4]
            )));
        }
        let count = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        let max_entries = bytes.len().saturating_sub(9) / 16;
        if count > max_entries {
            return Err(CollabError::SidecarFormat(format!(
                "count {count} 超物理上界 {max_entries}（载荷长度不可达）"
            )));
        }
        let mut pos = 9usize;
        let mut table = std::collections::HashMap::with_capacity(count);
        let read_str = |pos: &mut usize| -> Result<String, CollabError> {
            if *pos + 4 > bytes.len() {
                return Err(CollabError::SidecarFormat("字符串长度越界".into()));
            }
            let n = u32::from_le_bytes([
                bytes[*pos],
                bytes[*pos + 1],
                bytes[*pos + 2],
                bytes[*pos + 3],
            ]) as usize;
            *pos += 4;
            if *pos + n > bytes.len() {
                return Err(CollabError::SidecarFormat("字符串体越界".into()));
            }
            let s = String::from_utf8(bytes[*pos..*pos + n].to_vec())
                .map_err(|_| CollabError::SidecarFormat("UTF-8 解码失败".into()))?;
            *pos += n;
            Ok(s)
        };
        for _ in 0..count {
            let id = read_str(&mut pos)?;
            if pos + 12 > bytes.len() {
                return Err(CollabError::SidecarFormat("anchor 载荷越界".into()));
            }
            let client = u64::from_le_bytes([
                bytes[pos],
                bytes[pos + 1],
                bytes[pos + 2],
                bytes[pos + 3],
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]);
            pos += 8;
            let clock =
                u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
            pos += 4;
            table.insert(id, CrdtAnchor { client, clock });
        }
        if pos != bytes.len() {
            return Err(CollabError::SidecarFormat(format!(
                "尾部冗余 {}B（声明条目后必须恰好读完）",
                bytes.len() - pos
            )));
        }
        let n = table.len();
        *self.anchors_mut() = table;
        Ok(n)
    }

    /// 锚点表条数（观测面）。
    pub fn anchor_count(&self) -> usize {
        self.anchors().len()
    }
}

/// 全树身份比对解算（父序列物理位 + sticky 首项 ID 全等判据——
/// 删除塌缩位不信任位值，只信身份全等）。
fn resolve_in_fragment(
    xml: &yrs::XmlFragmentRef,
    txn: &yrs::Transaction,
    anchor: CrdtAnchor,
) -> Option<ResolvedAnchor> {
    let len = xml.len(txn);
    for i in 0..len {
        let child = xml.get(txn, i)?;
        if let yrs::types::xml::XmlOut::Element(e) = child {
            if e.tag().as_ref() == crate::session::MARK_TAG {
                continue;
            }
            let sticky = xml.sticky_index(txn, i, yrs::Assoc::After)?;
            let id = sticky.id()?;
            if id.client.get() == anchor.client && id.clock == anchor.clock {
                return Some(ResolvedAnchor {
                    block_id: block_id_of(&e, txn).unwrap_or_default(),
                    kind: BlockKind::from_tag(e.tag().as_ref()).unwrap_or(BlockKind::Paragraph),
                    parent_block_id: None,
                    index: i,
                });
            }
            if let Some(r) = resolve_in_element(&e, txn, anchor, i) {
                return Some(r);
            }
        }
    }
    None
}

fn resolve_in_element(
    el: &yrs::XmlElementRef,
    txn: &yrs::Transaction,
    anchor: CrdtAnchor,
    top_index: u32,
) -> Option<ResolvedAnchor> {
    let _ = top_index;
    let parent_id = block_id_of(el, txn);
    let len = el.len(txn);
    for i in 0..len {
        let child = el.get(txn, i)?;
        if let yrs::types::xml::XmlOut::Element(e) = child {
            if e.tag().as_ref() == crate::session::MARK_TAG {
                continue;
            }
            let sticky = el.sticky_index(txn, i, yrs::Assoc::After)?;
            let id = sticky.id()?;
            if id.client.get() == anchor.client && id.clock == anchor.clock {
                return Some(ResolvedAnchor {
                    block_id: block_id_of(&e, txn).unwrap_or_default(),
                    kind: BlockKind::from_tag(e.tag().as_ref()).unwrap_or(BlockKind::Paragraph),
                    parent_block_id: parent_id.clone(),
                    index: i,
                });
            }
            if let Some(r) = resolve_in_element(&e, txn, anchor, i) {
                return Some(r);
            }
        }
    }
    None
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}
