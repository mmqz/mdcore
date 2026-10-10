//! 安全策略编码——SECURITY_POLICY.md v1 的类型层落地（PDF-b 骨架轮）。
//!
//! 本模块是策略的**权威数值与判定语义**所在（限额 = 逐条配回归测试的
//! 资产——pdfb_skeleton.rs 逐项锁）：
//! - **S2** 危险对象族：打开时只读检测 + 告警 + 不执行（执行面在 v0
//!   不存在 = 能力面即防御；检测实装 = lopdf 对象图扫描，PDF-d 打开链
//!   接线，本轮先冻结类型面）；
//! - **S3** 限额五层 + 三行为学（❶硬拒绝 / ❷钳制 / ❸弃置）；
//! - **S5** 仍加密态拒绝一切写类命令（语义锁背书）。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// S2 危险对象族
// ---------------------------------------------------------------------------

/// S2 危险对象族（五族——打开时检测 + 告警 + **不执行**）。变体名 =
/// 代码语义（PDF 键的斜杠形属引擎映射层，不进协议面）；serde 名 =
/// 协议面冻结锚（语义锁背书，防变体名漂移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DangerousFamily {
    /// `/OpenAction`——文档打开动作。
    OpenAction,
    /// `/AA`——附加动作族（页面/控件事件触发器）。
    AdditionalActions,
    /// `/Launch`——启动外部程序动作。
    LaunchAction,
    /// `/EmbeddedFiles`——嵌入文件树。
    EmbeddedFiles,
    /// `/RichMedia`——富媒体注释/激活。
    RichMedia,
}

/// 单条危险对象命中（族 + 定位细节——对象号/页位等，引擎扫描实装填充；
/// v0 宿主注入）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerousHit {
    pub family: DangerousFamily,
    pub detail: String,
}

/// 打开评估（S2.4 语义：**结构化报告，只读不阻断打开**——用户自行决定
/// 去留；v0 无杀进程面）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAssessment {
    /// S5：**仍加密态**（未解密）——一切写类命令拒绝（语义锁背书）。
    /// 只读命令放行（S5.1：v0 只读支持加密档打开）。
    pub encrypted: bool,
    /// S2 危险对象命中清单（空 = 未检出）。
    pub warnings: Vec<DangerousHit>,
}

/// S5 写拒理由码（`CommandError::SecurityDeny.reason` 稳定标识——协议面
/// 冻结，宿主可据此本地化文案）。
pub const REASON_ENCRYPTED_WRITE: &str = "S5-encrypted-write";
/// S3-L7 作者名超限理由码（❶硬拒绝）。
pub const REASON_AUTHOR_LENGTH: &str = "S3-L7-author-length";

// ---------------------------------------------------------------------------
// S3 限额 + 三行为学
// ---------------------------------------------------------------------------

/// S3 三行为学（限额触发的统一处置语义；全库一致，不逐点发明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitBehavior {
    /// ❶ 硬拒绝 = 结构化错误返回（调用方决定中止/跳过）。
    HardReject,
    /// ❷ 钳制/降级 = 收敛到安全值继续 + 告知（如搜索截断 `truncated`）。
    Clamp,
    /// ❸ 弃置 = 承认无法中断，执行体留在后台、结果永不采用（渲染看门狗
    /// 路径，PDF-c 启用；v0 无消费点——枚举先行冻结语义，不假装能杀线程）。
    Drop,
}

/// S3 限额表（数值抄录自 SECURITY_POLICY.md v1；渲染面字段 PDF-c 接线，
/// 结构面字段 PDF-d 打开链接入——字段先行冻结 = 数值面不漂移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    // --- L0 结构层（解析面；lopdf 对象图 walk 于 PDF-d 接入）---
    /// 解压输出上限（防 zip 炸弹）：1 GiB。
    pub decode_output_max_bytes: u64,
    /// 对象嵌套深度上限：128。
    pub object_depth_max: u32,
    /// xref 对象数上限（ISO 32000 附录 C）：8,388,607。
    pub xref_objects_max: u32,
    // --- L2 渲染层（PDF-c 启用——骨架轮留字段冻结数值，渲染面未接线）---
    /// 位图单边上限（px）：8192。
    pub raster_max_side_px: u32,
    /// 位图总像素上限：64M。
    pub raster_max_pixels: u64,
    /// 渲染看门狗超时（ms）：20s——超时走 ❸弃置。
    pub render_watchdog_ms: u64,
    // --- L4 会话层（引擎打开预算；PDF-c 打开链接入）---
    /// 整档打开预算（ms）：3s——超限 ❷降级（只读前 N 页 + 告知）。
    pub open_budget_ms: u64,
    // --- L7 输入层（v0 生效：文件大小 = 宿主 open 前执行；作者名 = 命令层执行）---
    /// 打开文件大小上限（字节）：1 GiB。
    pub open_file_max_bytes: u64,
    /// 标注作者名长度上限（**字符**）：200（❶硬拒绝）。
    pub author_max_chars: usize,
    // --- 搜索/抽取面（v0 生效——PDF-b 定稿数值）---
    /// `SearchOptions.max_hits` 请求钳制上限（❷：超限收敛到该值，
    /// 命中溢出以 `truncated: true` 告知）。
    pub search_max_hits_cap: u32,
    /// 命中摘录长度上限（字符，❷钳制——char 边界安全截断）。
    pub snippet_max_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            decode_output_max_bytes: 1 << 30, // 1 GiB（S3-L0）
            object_depth_max: 128,            // S3-L0
            xref_objects_max: 8_388_607,      // S3-L0（ISO 附录 C 上限）
            raster_max_side_px: 8_192,        // S3-L2（PDF-c 启用）
            raster_max_pixels: 64_000_000,    // S3-L2（PDF-c 启用）
            render_watchdog_ms: 20_000,       // S3-L2（PDF-c 启用，❸弃置）
            open_budget_ms: 3_000,            // S3-L4（PDF-c 接入，❷降级）
            open_file_max_bytes: 1 << 30,     // 1 GiB（S3-L7，宿主 open 前执行）
            author_max_chars: 200,            // S3-L7（v0 命令层生效，❶）
            search_max_hits_cap: 1_000,       // S3 搜索面（PDF-b 定稿，❷）
            snippet_max_chars: 200,           // S3 搜索面（PDF-b 定稿，❷）
        }
    }
}

/// enforce 族方法的统一合同：返回 `(生效值, 行为学)`——
/// - `Clamp` 族：直接采用生效值；`生效值 < 请求值` = 发生了钳制，
///   调用方按 ❷ 语义告知（如 `truncated = true`）；
/// - `HardReject` 族：`生效值 < 请求值` = 超限，调用方必须拒绝命令
///   （❶ 语义——生效值仅供错误报告，不得采用）。
impl Limits {
    /// ❷钳制族（搜索面）：命中数请求。生效值 = min(请求, 上限)。
    pub fn enforce_search_hits(&self, requested: u32) -> (u32, LimitBehavior) {
        (
            requested.min(self.search_max_hits_cap),
            LimitBehavior::Clamp,
        )
    }

    /// ❷钳制族（搜索面）：摘录长度请求（char 计）。
    pub fn enforce_snippet_chars(&self, requested: usize) -> (usize, LimitBehavior) {
        (requested.min(self.snippet_max_chars), LimitBehavior::Clamp)
    }

    /// ❶硬拒绝族（S3-L7 输入层）：标注作者名长度（char 计）。
    pub fn enforce_author_chars(&self, requested: usize) -> (usize, LimitBehavior) {
        (
            requested.min(self.author_max_chars),
            LimitBehavior::HardReject,
        )
    }

    /// ❶硬拒绝族（S3-L7 输入层）：打开文件大小（宿主 open 前执行——
    /// 会话无文件字节面，本方法是宿主侧唯一判定出口）。
    pub fn enforce_open_file_bytes(&self, requested: u64) -> (u64, LimitBehavior) {
        (
            requested.min(self.open_file_max_bytes),
            LimitBehavior::HardReject,
        )
    }
}

impl OpenAssessment {
    /// S5 判定：仍加密态是否拒绝写类命令（AnnotateAdd / AnnotateEdit；
    /// 只读命令放行——S5.1 v0 只读支持）。写路径在会话层调用本判定，
    /// **先于一切存在性检查**（策略门最前——语义锁背书）。
    pub fn denies_writes(&self) -> bool {
        self.encrypted
    }
}
