//! 安全策略编码——SECURITY_POLICY.md v1 的类型层落地（PDF-b 骨架轮；
//! PDF-e 安全姿态对齐轮接线打开面/渲染面实装）。
//!
//! 本模块是策略的**权威数值与判定语义**所在（限额 = 逐条配回归测试的
//! 资产——pdfb_skeleton.rs 逐项锁）：
//! - **S2** 危险对象族（八族——PDF-e 对齐 S2.2/S2.3 全清单）：打开时
//!   只读检测 + 告警 + 不执行（执行面在 v0 不存在 = 能力面即防御）；
//!   检测实装 = [`crate::security_scan`]（lopdf 对象图全扫，feature
//!   `engine-lopdf`，PDF-e 接线——`open_scan` / `LopdfWriter::from_bytes`
//!   装载后扫描）；
//! - **S6** 表单探针（[`FormProbe`]）：/AcroForm 字段枚举进
//!   [`OpenAssessment::form_probe`]（被动可读面告警——字段名/现值只读，
//!   永不初始化交互 formfill）；
//! - **S3** 限额五层 + 三行为学（❶硬拒绝 / ❷钳制 / ❸弃置）——接线
//!   状态逐字段见 [`Limits`] 注释（PDF-e 起：看门狗/打开预算/输入
//!   字节/渲染目标缓冲四处消费点在档）；
//! - **S5** 仍加密态拒绝一切写类命令（语义锁背书）。

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::command::CommandError;

// ---------------------------------------------------------------------------
// S2 危险对象族
// ---------------------------------------------------------------------------

/// S2 危险对象族（八族——打开时检测 + 告警 + **不执行**；PDF-e 对齐
/// SECURITY_POLICY S2.2/S2.3 全清单：5 族 → 8 族）。变体名 = 代码语义
/// （PDF 键的斜杠形属引擎映射层，不进协议面）；serde 名 = 协议面冻结
/// 锚（语义锁背书，防变体名漂移）。声明序 = 扫描报告排序序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DangerousFamily {
    /// `/OpenAction`——文档打开动作。
    OpenAction,
    /// `/AA`——附加动作族（页面/控件事件触发器）。
    AdditionalActions,
    /// `/Launch`——启动外部程序动作（`/S /Launch`）。
    LaunchAction,
    /// `/URI`——URI 出站动作（`/S /URI` + `/URI` 载荷；PDF-e 增，
    /// S2.2）。
    UriAction,
    /// `/EmbeddedFiles`——嵌入文件树（Names 子树 + `/Type /EmbeddedFile`
    /// 流）。
    EmbeddedFiles,
    /// `/RichMedia`——富媒体注释/激活。
    RichMedia,
    /// `/JavaScript`——JavaScript 动作（`/S /JavaScript` + `/JS` 载荷；
    /// PDF-e 增，S2.3）。
    JavaScript,
    /// `/Names → /JavaScript`——文档级 JS 名字树（PDF-e 增，S2.3）。
    NamesJavaScript,
}

/// 单条危险对象命中（族 + 定位细节——对象号等；`security_scan` 对象图
/// 扫描填充，PDF-e 起有生产构造点）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DangerousHit {
    pub family: DangerousFamily,
    pub detail: String,
}

/// S6 表单探针——单字段项（/T 全名[层级以 `.` 连接] + 可读现值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormField {
    /// 字段全名（部分名按字段树层级 `.` 连接——PDF 全名语义）。
    pub name: String,
    /// `/V` 现值（文本可读时；None = 无值/非文本形态——探针只读不写）。
    pub value: Option<String>,
}

/// S6 表单探针（/AcroForm 字段枚举——被动可读面告警）。SECURITY_POLICY
/// S6 定谳：表单字段是被动可读面而非执行面（JS 引擎由构建层排除）；
/// 处置纪律 = 字段枚举进 SecurityReport 元数据并告警，**永不初始化
/// 交互式 formfill 环境执行动作**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormProbe {
    /// /AcroForm /Fields 字段总数（全量计数——`fields` 截断时仍精确）。
    pub field_count: u32,
    /// 枚举字段（上限 = `Limits::search_max_hits_cap` 复用——枚举面
    /// 有界；截断以 `field_count > fields.len()` 可判）。
    pub fields: Vec<FormField>,
}

/// 打开评估（S2.4 语义：**结构化报告，只读不阻断打开**——用户自行决定
/// 去留；v0 无杀进程面）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAssessment {
    /// S5：**仍加密态**（未解密）——一切写类命令拒绝（语义锁背书）。
    /// 只读命令放行（S5.1：v0 只读支持加密档打开）。
    pub encrypted: bool,
    /// S2 危险对象命中清单（空 = 未检出；`security_scan` 扫描填充）。
    pub warnings: Vec<DangerousHit>,
    /// S6 表单探针（None = 无 /AcroForm；PDF-e 增。serde 增量兼容：
    /// 旧 JSON 缺省反序列化为 None，None 序列化时省略字段）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_probe: Option<FormProbe>,
}

/// S5 写拒理由码（`CommandError::SecurityDeny.reason` 稳定标识——协议面
/// 冻结，宿主可据此本地化文案）。
pub const REASON_ENCRYPTED_WRITE: &str = "S5-encrypted-write";
/// S3-L7 作者名超限理由码（❶硬拒绝）。
pub const REASON_AUTHOR_LENGTH: &str = "S3-L7-author-length";
/// S3-L7 打开文件字节数超限理由码（❶硬拒绝——open 家族入口统一执行，
/// PDF-e 接线）。
pub const REASON_OPEN_FILE_BYTES: &str = "S3-L7-open-file-bytes";
/// S3-L4 打开预算超限理由码（❶硬拒绝——v0 打开面语义，PDF-e 接线）。
pub const REASON_OPEN_BUDGET: &str = "S3-L4-open-budget";
/// S3-L0 渲染目标缓冲超限理由码（❶硬拒绝——先于分配，PDF-e 接线）。
pub const REASON_DECODE_OUTPUT: &str = "S3-L0-decode-output";

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
    /// 路径，PDF-e 实装：[`crate::worker::RenderWorker::watchdog`] 超时
    /// → `Timeout` 回执 + 工人换代；被弃作业的迟到回执由包装器静默
    /// 丢弃——「结果永不采用」的机械保证）。
    Drop,
}

/// S3 限额表（数值抄录自 SECURITY_POLICY.md v1——**字段先行冻结 =
/// 数值面不漂移**；消费点状态逐字段在注，PDF-e 起接线状态与在档
/// 姿态一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    // --- L0 结构层 ---
    /// 解压输出上限（防 zip 炸弹）：1 GiB。**消费点**：渲染目标缓冲面
    /// （宽×高×4 BGRA，`engine_pdfium` rasterize 内 L2 钳制后、分配前
    /// ❶校验——PDF-e 接线；冻结数值下 64M px × 4B = 256 MiB < 1 GiB
    /// 不触发，属未来限额调整的纵深硬闸）。内容流 flate2 解压本体在
    /// lopdf 内部无挂点——[v0 边界登记] 升级 lopdf/自研解析层时接线。
    pub decode_output_max_bytes: u64,
    /// 对象嵌套深度上限：128。**消费点**：`security_scan` 对象图游走
    /// 深度护栏（内联嵌套 + /AcroForm 字段树引用链，PDF-e 接线）；
    /// lopdf 解析器内部逐对象深度无挂点——[v0 边界登记] 同上。
    pub object_depth_max: u32,
    /// xref 对象数上限（ISO 32000 附录 C）：8,388,607。lopdf 解析面
    /// 无逐对象钩子——[v0 边界登记] 升级 lopdf/自研解析层时接线。
    pub xref_objects_max: u32,
    // --- L2 渲染层（PDF-c 启用）---
    /// 位图单边上限（px）：8192。
    pub raster_max_side_px: u32,
    /// 位图总像素上限：64M。
    pub raster_max_pixels: u64,
    /// 渲染看门狗超时（ms）：20s——超时走 ❸弃置（`Timeout` 回执 +
    /// 工人毒化换代；PDF-e 接线，[`crate::worker::RenderWorker::watchdog`]
    /// 工厂构造）。
    pub render_watchdog_ms: u64,
    // --- L4 会话层 ---
    /// 整档打开预算（ms）：3s——装载+扫描耗时超限 → ❶ typed 拒绝
    /// （PDF-e 接线：lopdf `from_bytes` / `open_scan` 与 pdfium
    /// `PdfiumEngine::open` 三入口）。策略表 ❷降级形态（只读前 N 页 +
    /// 告知）需会话降级机制——[v0 边界登记] 未实装，当前语义 = 打开面
    /// 硬拒绝。
    pub open_budget_ms: u64,
    // --- L7 输入层 ---
    /// 打开文件大小上限（字节）：1 GiB。**消费点**（PDF-e 接线，open
    /// 家族入口统一——S5 双防线风格的打开面泛化）：lopdf `from_bytes`
    /// / `open_scan` 与 pdfium `PdfiumEngine::open`（会话 open 本体无
    /// 字节面，字节长度在两个上游入口收口）。
    pub open_file_max_bytes: u64,
    /// 标注作者名长度上限（**字符**）：200（❶硬拒绝；v0 生效——
    /// 命令层执行）。
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
            render_watchdog_ms: 20_000,       // S3-L2（PDF-e 接线，❸弃置）
            open_budget_ms: 3_000,            // S3-L4（PDF-e 接线，❶）
            open_file_max_bytes: 1 << 30,     // 1 GiB（S3-L7，PDF-e 接线）
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

    /// ❶硬拒绝族（S3-L7 输入层）：打开文件大小（open 家族入口统一
    /// 执行——PDF-e 接线；宿主 open 前执行与引擎装载入口双面同款）。
    pub fn enforce_open_file_bytes(&self, requested: u64) -> (u64, LimitBehavior) {
        (
            requested.min(self.open_file_max_bytes),
            LimitBehavior::HardReject,
        )
    }

    /// ❶硬拒绝族（S3-L0 结构层 → 渲染目标面）：渲染目标缓冲字节
    /// （宽×高×4——BGRA，先于分配校验；PDF-e 接线）。
    pub fn enforce_decode_output_bytes(&self, requested: u64) -> (u64, LimitBehavior) {
        (
            requested.min(self.decode_output_max_bytes),
            LimitBehavior::HardReject,
        )
    }

    /// S3-L7 输入层打开入口检查（❶ typed 拒绝——`SecurityDeny` 携
    /// [`REASON_OPEN_FILE_BYTES`]；PDF-e 接线）：open 家族入口统一
    /// 执行（lopdf `from_bytes` / `open_scan` 与 pdfium
    /// `PdfiumEngine::open`）。
    pub fn check_open_input_bytes(&self, len: u64) -> Result<(), CommandError> {
        let (effective, behavior) = self.enforce_open_file_bytes(len);
        if behavior == LimitBehavior::HardReject && effective < len {
            return Err(CommandError::SecurityDeny {
                reason: REASON_OPEN_FILE_BYTES,
            });
        }
        Ok(())
    }

    /// S3-L4 打开预算检查（❶ typed 拒绝——`SecurityDeny` 携
    /// [`REASON_OPEN_BUDGET`]；PDF-e 接线）：`started` 起算的装载 +
    /// 扫描耗时超 `open_budget_ms` 即拒（Duration 全精度比较——测试
    /// 可注入 0 预算确定性触发）。
    pub fn check_open_budget(&self, started: Instant) -> Result<(), CommandError> {
        if started.elapsed() > Duration::from_millis(self.open_budget_ms) {
            return Err(CommandError::SecurityDeny {
                reason: REASON_OPEN_BUDGET,
            });
        }
        Ok(())
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
