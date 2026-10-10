//! 宿主协议草案 v0（PDF-b——**草案**：PDF 面协议在 PDF-c 视图层落地时
//! 定稿，本轮只冻结词面与信封形状）。
//!
//! PDF 面不经 CM6：`mdcore://pdf/<document-slot>/…` 是独立轻量视图层
//! （pdf_carrier_plan §1——web 桥的 CM6 只服务 MD 面）。命令/事件走
//! `mdc-*` 命名域事件族（CONTRACT §2/§4 同域），与 web window 面零
//! 耦合（`__contractVersion` 不因 PDF 面变化——CONTRACT §6）。

use serde::{Deserialize, Serialize};

use crate::command::Command;

/// 自定义协议 scheme（与 MD 面 `mdcore://app` 同 scheme——CONTRACT §2）。
pub const SCHEME: &str = "mdcore";

/// PDF 面 face 段。
pub const FACE: &str = "pdf";

/// URL 前缀：`mdcore://pdf/<document-slot>/…`。
///
/// 草案语义：`<document-slot>` = 宿主分配的文档槽位标识（v0 = 会话
/// 槽号十进制串；多窗路由与槽位复用规则属宿主域）。路径段（`…`）由
/// PDF-c 视图层定稿（静态视图资源 + 深链页位预留），本轮不冻结。
pub const URL_PREFIX: &str = "mdcore://pdf/";

/// 命令事件名（宿主 → PDF 面：detail = [`CommandEnvelope`] 的 JSON——
/// `mdc-*` 命名域，对齐 CONTRACT §2 注入族与 §4 设置族的词面风格）。
pub const EVENT_COMMAND: &str = "mdc-pdf-cmd";

/// 回执事件名（PDF 面 → 宿主：命令回执（信封 id 回关联）与自发事件；
/// 载荷形状 PDF-c 视图层定稿）。
pub const EVENT_EVENT: &str = "mdc-pdf-event";

/// 命令信封（草案）：`id` = 宿主侧回执关联的请求号（单调，回执原样
/// 带回）；`command` = [`Command`] 的 serde JSON 形。信封层保持透传
/// （不 typed 到具体命令版本）——协议演进只动 command 面，信封形状
/// 稳定。无会话槽位的信封由宿主直接回 `CommandError::NotOpen` 形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub id: u64,
    pub command: serde_json::Value,
}

impl CommandEnvelope {
    /// 从 typed 命令构造信封（序列化失败 = 内部不变量破坏，向上传播
    /// 而非 panic——Command 为纯数据面，实践中不可达）。
    pub fn new(id: u64, command: &Command) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id,
            command: serde_json::to_value(command)?,
        })
    }
}
