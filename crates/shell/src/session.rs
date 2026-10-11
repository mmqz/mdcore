//! R24-b 多窗口核侧（主档 §31 R24-b）——SessionManager / 窗口↔document_id
//! 映射 / 资源上限 / 会话持久化。
//!
//! 职责边界（与宿主分工，同 settings/FileStamp 纪律）：
//! - **窗口是宿主概念**（tao 窗口句柄、GTK3 生命周期）；核侧只维护
//!   `WindowId → DocumentSession` 的**纯状态映射**。headless 测试驱动
//!   同一套状态机；原生壳（R24-d）把 WindowId 绑定到真实窗口。
//! - **会话持久化是纯数据**：`SessionRecord`（serde JSON）由宿主写盘
//!   （settings 侧 `storage.session_restore` 开关消费）；核不碰 fs。
//!   记录是可再生缓存（非用户数据）：未来 schema → 解析拒绝 → 宿主
//!   以空会话冷启动（与 settings 的 FutureSchema 拒写保护不同级——
//!   设置丢了是用户损失，会话记录丢了只是少恢复几个窗口）。
//!
//! 机制合同（r24_session.rs 语义锁背书）：
//! - **一窗一档**（v0）：同一 document_id 不得同时开两窗
//!   （`DocumentAlreadyOpen`——宿主 UX 应改为聚焦既有窗口 reveal；
//!   匿名文档 id=None 不受此限，可多开）。放宽为多窗共享属契约级变更，
//!   走 CONTRACT 升版纪律。
//! - **资源上限**：并发会话数 ≤ `limit`（默认 [`DEFAULT_MAX_SESSIONS`]）。
//!   超限 `open` 显式 Err（不静默驱逐——驱逐策略是宿主 UX 决策，
//!   核侧提供 `dirty_windows` / `windows_in_focus_order` 供其决策）。
//! - **焦点序**：`focus` 维护 MRU 序（末位 = 最近聚焦）；会话记录按
//!   焦点序导出，恢复时按序重开、末位窗口获得最终焦点。
//! - **确定性**：BTreeMap + 显式焦点 Vec，全路径无迭代序歧义
//!   （同 fingerprint 纪律）；记录 JSON 字段序 = 结构序，同态同串。
//!
//! 契约：宿主面（Rust 对 Rust，非 window 面）见 CONTRACT.md §5；
//! `__contractVersion` 不变（v1.1，window 面零新增）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::DocumentSession;
use mdcore::document::DocumentId;

/// 默认并发会话上限（资源上限；16 = 桌面编辑器惯例量级，
/// 每 session 一个 wry WebView + wasm 核，内存为主要约束）。
pub const DEFAULT_MAX_SESSIONS: usize = 16;

/// 会话记录 schema 版本。
pub const SESSION_RECORD_VERSION: u64 = 1;

/// 窗口标识（宿主分配；核侧不解释，仅作映射键。u64 保证 JS/FFI 边界无损）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// 会话面拒绝原因。
#[derive(Debug, Clone, PartialEq)]
pub enum SessionError {
    /// 并发会话达到上限（资源上限硬门）。
    LimitReached { limit: usize },
    /// 窗口已持有会话（先 close 再 open；或宿主复用了已用 WindowId）。
    DuplicateWindow(WindowId),
    /// 窗口未注册（close/focus/get 作用于未知窗口）。
    UnknownWindow(WindowId),
    /// 同一文档已在别窗打开（v0 一窗一档；宿主应改为聚焦该窗）。
    DocumentAlreadyOpen {
        document_id: DocumentId,
        window: WindowId,
    },
}

/// 多窗口会话管理器：窗口→会话映射 + 焦点序 + 资源上限。
///
/// headless 可验证（原生壳的窗口状态机替身）；不持有 GUI 资源。
/// （Core 含注入时钟闭包，不可 Clone/Debug——检视一律走查询方法。）
pub struct SessionManager {
    sessions: BTreeMap<WindowId, DocumentSession>,
    /// MRU 序（末位 = 最近聚焦/最近打开）。与 sessions 键集严格同步。
    focus_order: Vec<WindowId>,
    limit: usize,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new_with_limit(DEFAULT_MAX_SESSIONS)
    }
}

impl SessionManager {
    pub fn new_with_limit(limit: usize) -> Self {
        Self {
            sessions: BTreeMap::new(),
            focus_order: Vec::new(),
            limit,
        }
    }

    // ---- 检视 ----

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// 窗口列表（焦点序：旧→新，末位 = 最近聚焦）。确定性迭代。
    pub fn windows_in_focus_order(&self) -> Vec<WindowId> {
        self.focus_order.clone()
    }

    /// 最近聚焦的窗口（无会话 = None）。
    pub fn focused_window(&self) -> Option<WindowId> {
        self.focus_order.last().copied()
    }

    pub fn get(&self, window: WindowId) -> Result<&DocumentSession, SessionError> {
        self.sessions
            .get(&window)
            .ok_or(SessionError::UnknownWindow(window))
    }

    pub fn get_mut(&mut self, window: WindowId) -> Result<&mut DocumentSession, SessionError> {
        self.sessions
            .get_mut(&window)
            .ok_or(SessionError::UnknownWindow(window))
    }

    /// 反向映射：document_id → 持有它的窗口（一窗一档 → 唯一）。
    /// 匿名文档（None）不在映射内。宿主「重复开档 → 聚焦既有窗」用。
    pub fn window_of_document(&self, document_id: &DocumentId) -> Option<WindowId> {
        self.sessions
            .iter()
            .find(|(_, s)| s.id.as_ref() == Some(document_id))
            .map(|(w, _)| *w)
    }

    /// 脏窗口清单（BTreeMap 键序，确定性）。宿主「关闭前确认」/
    /// 驱逐候选决策（只驱逐干净会话）输入。
    pub fn dirty_windows(&self) -> Vec<WindowId> {
        self.sessions
            .iter()
            .filter(|(_, s)| s.core.is_dirty())
            .map(|(w, _)| *w)
            .collect()
    }

    // ---- 生命周期 ----

    /// 注册会话到窗口（新窗口成为最近聚焦）。三重硬门：
    /// 重复窗口 / 文档已被别窗持有 / 资源上限。
    pub fn open(&mut self, window: WindowId, session: DocumentSession) -> Result<(), SessionError> {
        if self.sessions.contains_key(&window) {
            return Err(SessionError::DuplicateWindow(window));
        }
        if let Some(id) = &session.id {
            if let Some(holder) = self.window_of_document(id) {
                return Err(SessionError::DocumentAlreadyOpen {
                    document_id: *id,
                    window: holder,
                });
            }
        }
        if self.sessions.len() >= self.limit {
            return Err(SessionError::LimitReached { limit: self.limit });
        }
        self.sessions.insert(window, session);
        self.focus_order.push(window);
        Ok(())
    }

    /// 注销窗口（返回会话供宿主收尾——如未存改动的保存确认）。
    pub fn close(&mut self, window: WindowId) -> Result<DocumentSession, SessionError> {
        let session = self
            .sessions
            .remove(&window)
            .ok_or(SessionError::UnknownWindow(window))?;
        self.focus_order.retain(|&w| w != window);
        Ok(session)
    }

    /// 聚焦窗口（移到 MRU 末位）。
    pub fn focus(&mut self, window: WindowId) -> Result<(), SessionError> {
        self.get_mut(window)?;
        self.focus_order.retain(|&w| w != window);
        self.focus_order.push(window);
        Ok(())
    }

    // ---- 会话持久化（恢复上次打开集）----

    /// 导出会话记录（焦点序）。无路径会话（未另存的新文档）不入恢复集
    /// ——v0 无草稿暂存，不可恢复即不记录（诚实语义）。
    pub fn snapshot_record(&self) -> SessionRecord {
        let windows = self
            .focus_order
            .iter()
            .filter_map(|&w| {
                let s = &self.sessions[&w];
                s.path.as_ref().map(|p| WindowRecord {
                    window: w.0,
                    document_id: s.id.map(|id| id.to_string()),
                    path: p.clone(),
                })
            })
            .collect();
        SessionRecord {
            schema_version: SESSION_RECORD_VERSION,
            windows,
        }
    }
}

/// 会话持久化记录（宿主写盘的纯数据；可再生缓存）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub schema_version: u64,
    /// 焦点序（旧→新）；恢复按序重开，末位窗口获得最终焦点。
    pub windows: Vec<WindowRecord>,
}

/// 单窗口恢复项。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowRecord {
    pub window: u64,
    /// document_id hex（诊断/宿主侧去重用；权威身份以文件 meta sidecar 为准）。
    pub document_id: Option<String>,
    /// 宿主域路径（核侧不解释；恢复时宿主按自己的语义解析）。
    pub path: String,
}

impl SessionRecord {
    /// 序列化（字段序 = 结构序，同态同串——确定性锚）。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("session record serialize")
    }

    /// 解析。未来 schema（version > 本构建）→ Err（宿主降级为空会话
    /// 冷启动，不误解释新字段）；畸形 JSON → Err。未知字段忽略
    /// （机器写缓存，无手工编辑往返合同——与 settings 的字节级
    /// 保留语义不同级，settings 是用户数据本记录是缓存）。
    pub fn from_json(json: &str) -> Result<Self, String> {
        let rec: SessionRecord =
            serde_json::from_str(json).map_err(|e| format!("bad session record json: {e}"))?;
        if rec.schema_version > SESSION_RECORD_VERSION {
            return Err(format!(
                "unsupported session record schema {} (this build supports <= {}); \
                 starting with an empty session",
                rec.schema_version, SESSION_RECORD_VERSION
            ));
        }
        Ok(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 结构锁：默认上限常量（防漂移——资源上限是产品参数）。
    #[test]
    fn default_limit_is_sixteen() {
        assert_eq!(DEFAULT_MAX_SESSIONS, 16);
        assert_eq!(SessionManager::default().limit(), 16);
    }
}
