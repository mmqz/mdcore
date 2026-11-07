//! mdc-office-io — Office 域 XLSX 读写 + DOCX 切片引擎
//!（xlsx-r1 阶段 B + xlsx-r2 三阶段）。
//!
//! workspace 第八成员（双 crate 方案 A：mdc-office 保持纯 std 零三方
//! 依赖的自锁不变量，IO 三方依赖承载于本 crate——分析仓
//! xlsx-r1/REPORT.md §2 架构决策）。三域纪律：对 mdcore 零编译依赖，
//! 与 mdc-office/ mdc-pdf 并列，不建跨格式抽象。
//!
//! ## 写路径（[`write`]，xlsx-r2 Phase 1——2026-10-14）
//!
//! 新建工作簿写 API（[`write::WorkbookWriter`]）：rust_xlsxwriter
//! 包装，**引擎类型零泄漏**（公开签名只含本 crate 类型）；输入验证
//! 前移（表名规则/坐标边界/合并次序 → typed [`write::WriteError::
//! InvalidInput`]）；流式表（constant_memory）行序纪律 typed 强制
//! （引擎对已刷写行的回退写**静默丢弃**——源级一手核验，包装层
//! 拒绝，绝不静默丢数据）；值/样式/合并/超链接四面 + 公式缓存值；
//! XML 级控制字符转义为引擎内建（`_xHHHH_` 约定，写产物恒为合法
//! XML 1.0）。
//!
//! ## DOCX 切片（[`docx`]，xlsx-r2 Phase 3——2026-10-14）
//!
//! 小而可用的**模型级**读/写切片（docx-rs 0.4.22 引擎，类型零
//! 泄漏）：读 = 块序（段落/表）+ 样式 ID 透传 + run 级文本/加粗/
//! 斜体 + 超链接内文本 + 表格单元格拼接；写 = 标题（Heading1-9
//! 样式 + outline 级）/富文本段落/文本表格。容器防线复用 XLSX
//! 同一门结构（门 1/2/3——zip+OPC 同构，基础设施共享非领域抽象）；
//! 不支持面与数据损失在 [`docx`] 模块文档逐项登记；**不做渲染
//! 语义**（布局/分页/字体度量面外）。
//!
//! ## DOCX 外科手术式编辑（[`docx_edit`]，#79 T1——2026-10-31）
//!
//! 已有 docx 的**文本编辑**面：解压 → 定位 `word/document.xml` 段落
//! （`w:p`）→ 只改目标段落文本节点（`w:t`）→ 其余 XML 原样 → 重打包
//! （非目标部件 `raw_copy_file` 逐字节透传）。**不走块模型重建**
//! （会丢样式——xlsx FIDELITY v2 §4 定谳）；七坑映射/文本约定/落账
//! 模型见 [`docx_edit`] 模块文档；保真矩阵 = docs/docx-fidelity-matrix.md
//! + tests/docx_roundtrip_matrix.rs（T2 同批）。
//!
//! ## XLSX 原位编辑（[`xlsx_edit`]，#177——xlsx 版 DocxEditor）
//!
//! 已有富工作簿的**单元格级**编辑面：门 1/2/3（恒严格）→
//! workbook.xml/rels 表名解析 → sheet 部件单趟扫描建行/格索引 →
//! 只改目标 `<c>` 元素（值三形态：文本恒 inlineStr/数字/布尔；
//! 样式 `@s` 保留）→ 其余 XML 原样 + 非目标部件逐字节透传。公式格
//! 拒改（v1 边界——不重算）；插行/插格/自闭合空表展开支持；
//! dimension/spans hint 不更新（Excel 自算——宽容面）。
//! C3 宣称面更新：**单元格级原位编辑 v1**（读-改-重建仍不宣称——
//! 本面与其是正交路径：手术不重建）。

//! ## 读路径门结构（[`io::read_book`]）
//!
//! 1. **门 1 声明值预扫**（[`io::prescan`]）：条目数 / 单条目尺寸 /
//!    声明总量 / 路径逃逸四检查，全部基于 zip 中央目录声明值，零条目
//!    数据读取，敌意输入 typed reject（[`io::ReadError::PrescanRejected`]）；
//! 2. **门 2 尺寸诚实性 + 门 3 部件良构性（融合单趟，Phase 2）**：
//!    逐条目有界读取计数（实际解压量 > 声明值即拒——zip 8.6 读界
//!    无内建强制的策略层补面，lie86 探针定谳）；同一趟内对 .xml/.rels
//!    部件做流式结构校验（标签配对/根在位唯一/EOF 未闭合[截断]/
//!    深度上限 [`io::LIMIT_XML_DEPTH`]）——**畸形部件不再静默变空/
//!    部分数据**：默认严格 typed reject
//!    （[`io::ReadError::InvalidPart`]），[`io::ReadOptions::compat`]
//!    兼容模式降级为结构化警告（[`io::Book::warnings`]）；
//! 3. **calamine 五 API 面**（v2）：表名单与顺序 / 值矩阵（含公式缓存值）/
//!    公式文本 / 合并范围 / 超链接（target/location/displayed_text/tooltip
//!    四字段，IO 边界实体补偿——calamine 0.36.1 读侧 rels 属性值未做
//!    XML 实体反转义的怪癖由本层统一还原，消费方拿逻辑 URL）。
//!
//! 门在前：敌意输入零解析层接触。错误面：拒绝（门 1/2/3）/ 容器
//! 损坏 / 解析错误（[`io::ReadError`]）。门 1/门 2 与全部安全上限在
//! 严格/兼容两模式恒等（兼容不放松安全防线）。
//!
//! ## Capabilities（能力宣称边界——唯一真源 = 分析仓
//! xlsx-r1/CAPABILITIES.md，本节为引用摘要）
//!
//! - **C1 读取单元格数据：已验证可宣称**（7 维语料 17 件 harness
//!   17/17 零 panic + 50 万格 139ms 量级）。不宣称：样式/关系引用/
//!   图表部件/公式求值/其他容器格式。
//! - **C2 新建 XLSX：已验证，产品面已落地（[`write`] 模块，xlsx-r2
//!   Phase 1）**——值（字符串/数字/布尔/公式+缓存值）/样式（bold/
//!   字色/填充/thin 边框/¥ 与百分比 numFmt/居中——style_probe 9/9
//!   验证面）/合并/超链接（五形态）/流式大表。写读往返全环测试在档
//!   （tests/write_roundtrip.rs）。
//! - **C3 修改已有工作簿：不宣称支持**（[`write`] 模块仅新建语义）。
//!   已知替代路径两条（#283 产品导出编排 [`rebuild::export_edited`]
//!   按**保真优先**分派）：
//!   ① 原位编辑 [`xlsx_edit`]（[`xlsx_edit::XlsxEditor`]——单元格级
//!   手术，非目标部件逐字节透传：样式/图表/条件格式/数据验证保真；
//!   公式格拒改值、缓存经重算回写）；
//!   ② 读-改-重建 [`rebuild`]（[`io::read_book`] 提取 → [`write`]
//!   从零重建），实测保真边界 = 保 表序/值/公式文本/合并/超链接；
//!   丢 样式/文档元数据（FIDELITY.md v2 五档丢失矩阵定谳）——
//!   结构性丢失由 `skipped` 诚实账本逐类登记（宿主须警示，
//!   非静默降级）。重放不可信读数据须先过 mdc-office 消毒链
//!   （消费方责任，见 [`write`] 模块文档）。
//!
//! ## 容忍度发现（实测钉定，随宣称发布；Phase 2 后口径）
//!
//! - 0x00 读侧容忍入值（读侧不过滤——再输出须过 mdc-office
//!   [`mdc_office::sanitize`] 消毒链）；Phase 2 起恒登记
//!   ControlChars 结构化警告（两模式同，[`io::Book::warnings`]）；
//! - 垃圾部件静默降级空表（mal7e）**已闭环**（Phase 2 门 3）：默认
//!   严格 typed reject；兼容模式降级为结构化警告（数据照常提取）；
//! - 标签边界外截断（缺根闭合 `</worksheet>`）容忍**已闭环**（同上）；
//!   标签内截断在门 3 为语法错拒绝（兼容模式下引擎自身仍报
//!   ParseError——分界双测钉定）。
//!
//! ## 限额常量
//!
//! [`io::LIMIT_TOTAL_DECLARED`]（300MB）/ [`io::LIMIT_SINGLE_ENTRY`] /
//! [`io::LIMIT_ENTRY_COUNT`]（10,000）/ [`io::LIMIT_XML_DEPTH`]（1,000）
//! = 分析仓 SECURITY_POLICY §3 限额表接线；升版（zip 或解析库大
//! 版本）触发该策略 §5 复核纪律。

pub mod docx;
pub mod docx_edit;
// #226 R2：docx-rs 读层 panic 预检防御（私有 seam——docx 读面内部消费，
// 模块文档含位点清单与修复策略）。
mod docx_panic_guard;
// #307 阶段①：DOCX → PDF 导出桥（LibreOffice headless 通道——探测/
// 独立 profile/超时/分类错误含无 LO 安装指引）。
pub mod convert;
pub mod io;
pub mod numbering;
// #307 ②功能独立项：PDF→DOCX 文本版导出桥（纯数据边界模型 + 段落
// 启发式 + docx 写面——对 mdc-pdf 零依赖，装配在组合根）。
pub mod pdf_import;
pub mod rebuild;
pub mod style;
pub mod write;
pub mod xlsx_chart;
pub mod xlsx_condfmt;
pub mod xlsx_edit;
// #265：结构面读取——隐藏行列/外部链接/xf 继承链（calamine 值面之外
// 的结构增强，独立 zip 解析零嵌主流程）。
pub mod xlsx_structural;
