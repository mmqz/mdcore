//! mdc-office — Office 域引擎侧基础设施。
//!
//! office-impl T1（2026-10-12，office-line 开发启动轮）：第一模块
//! `sanitize` = 敌意输出面消毒五面。三域纪律（用户裁决 2026-10-12）：
//! MD / PDF / Office 保持独立领域模型与引擎实现——本 crate 对 mdcore
//! **零编译依赖**、零三方依赖（纯 std），循 mdc-pdf 并列引擎先例接入
//! workspace（版本单源随 workspace.package）。
//!
//! ## 消毒五面（规格 = 分析仓 office-d §5 一手代码级裁决）
//!
//! 背景：生态写库在三个安全面全部缺位或损坏——CSV 公式注入零消毒
//! （其一主动以 `=` 前缀重建公式文本）、XML 非法控制字符三档实况
//! （损坏档：非法字符原样透传记产物拒开；剥除档：合法 XML 但静默丢
//! 字符；唯保字符档达标）、域指令任意直通零白名单。**消毒责任在管线
//! 层**——本 crate 即引擎侧工具包，宿主/生成态管线在出口与喂入位调用。
//!
//! | 面 | 模块 | 职位 |
//! |----|------|------|
//! | CSV 公式首字符（`=` `+` `-` `@`） | [`sanitize::csv_formula`] | CSV/TSV 出口 |
//! | XML 控制字符（保字符档 `_xHHHH_`） | [`sanitize::xml_control`] | XML 文本喂入前 |
//! | XML 实体（含属性位兜底） | [`sanitize::xml_entities`] | XML 文本/属性喂入前 |
//! | HTML 出口转义（href 属性注入面） | [`sanitize::html_escape`] | HTML 出口 |
//! | wordprocessing 域指令白名单 | [`sanitize::field_whitelist`] | 生成侧（LLM 面） |
//!
//! ## 消费契约注记
//!
//! 1. **xlsx 写路线豁免 CSV 消毒**：目标表格应用对包内共享字符串
//!    （t="s"）不重解释为公式——公式注入只发生在 CSV/TSV 粘贴与打开
//!    路径（office-d §5.1 裁决 2）。对 xlsx 出口误用本模块是规格外
//!    行为（多一层无害但无意义的前缀）。
//! 2. **_xHHHH_ 保字符档**：对 XML 1.0 非法控制字符与 U+FFFE/U+FFFF
//!    以 `_xHHHH_` 约定转义（ECMA-376 电子表格字符串既定约定），
//!    编码器含**字面量消歧**（正文恰含 `_xHHHH_` 形态时前置转义其
//!    下划线）——任意输入编码-解码往返零失真（性质测试锁死）；该
//!    消歧是本 crate 超出生态参照库的登记项（office-d §5.2 裁决 3
//!    的【推断】歧义面在此解决）。
//! 3. **域白名单默认口径**：白名单 DATE/PAGE/NUMPAGES/TOC/PAGEREF/
//!    MERGEFIELD/SEQ；黑名单首列 INCLUDETEXT/INCLUDEPICTURE（外部
//!    资源面）/DDE（执行面）——拒绝族**全 token 扫描**（指令参数
//!    内嵌拒绝族名同样拦截，防嵌套域注入）。

pub mod sanitize;
